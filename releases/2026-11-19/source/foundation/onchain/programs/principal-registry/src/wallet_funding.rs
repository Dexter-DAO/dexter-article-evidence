//! Qualified financier custody: a fixed ProgramExec graph whose only admitted
//! actions are Core's earned collection, eligible free withdrawal, repayment
//! and separately authorized sends/swaps. Spending never reuses repayment scope.
//!
//! Authorization is public chain state. Earned collection requires neither a
//! private nonce service nor a fresh principal-owner signature. Core proves the
//! financial right and exact outcome independently, in the same transaction.
use anchor_lang::prelude::*;
use anchor_lang::InstructionData;
use anchor_lang::solana_program::instruction::{get_stack_height, Instruction, TRANSACTION_LEVEL_STACK_HEIGHT};
use dexter_contracts::{plus, CORE_ID, SCHEMA_VERSION};
use dexter_contracts::financing::custody_reserve_address;
use solana_instructions_sysvar::{load_current_index_checked, load_instruction_at_checked};
use solana_sha256_hasher::hashv;
use crate::{Principal, RegistryError};
use crate::swig_framing::CompactFrame;
use crate::wallet::{canonical_instruction, validate_graph_for, SWIG_ID};

const INSTRUCTIONS_ID: Pubkey = pubkey!("Sysvar1nstructions1111111111111111111111111");
const TOKEN_ID: Pubkey = pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
const ATA_ID: Pubkey = pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
pub const FUNDING_COLLECT: u8 = 1;
pub const FUNDING_WITHDRAW: u8 = 2;
pub const FUNDING_REPAY_RUN: u8 = dexter_contracts::repayment::REPAYMENT_RUN;
pub const FUNDING_REPAY_FACILITY: u8 = dexter_contracts::repayment::REPAYMENT_FACILITY;
pub const FUNDING_SEND: u8 = dexter_contracts::spending::SPENDING_SEND;
pub const FUNDING_SWAP: u8 = dexter_contracts::spending::SPENDING_SWAP;

#[account]
#[derive(InitSpace)]
pub struct FundingWalletBinding {
    /// Paid this record's rent deposit; `return_excess_rent` returns excess here.
    pub deposit_payer: Pubkey,
    pub version: u16,
    pub bump: u8,
    pub principal: Pubkey,
    pub config: Pubkey,
    pub wallet: Pubkey,
    pub wallet_id: [u8; 32],
    pub graph_hash: [u8; 32],
    pub nonce: u64,
}

/// Core authenticates enrollment through this strict cross-program reader.
/// Current owner permission is intentionally not part of this stable binding:
/// earned collection survives rotation, while free withdrawal checks the live
/// Principal separately. Registry rechecks the actual complete Swig graph on
/// every wallet authorization before Swig can supply its PDA signer.
pub fn read_funding_wallet_binding(info: &AccountInfo) -> Result<FundingWalletBinding> {
    require_keys_eq!(*info.owner, crate::ID, RegistryError::WalletBinding);
    require!(!info.executable, RegistryError::WalletBinding);
    let data = info.try_borrow_data()?;
    require!(data.len() == 8 + FundingWalletBinding::INIT_SPACE, RegistryError::WalletBinding);
    let value = FundingWalletBinding::try_deserialize(&mut &data[..])
        .map_err(|_| error!(RegistryError::WalletBinding))?;
    let (expected, bump) = Pubkey::find_program_address(
        &[b"funding-wallet", value.principal.as_ref(), value.config.as_ref()], &crate::ID);
    let config = Pubkey::find_program_address(&[b"swig", &value.wallet_id], &SWIG_ID).0;
    let wallet = Pubkey::find_program_address(&[b"swig-wallet-address", config.as_ref()], &SWIG_ID).0;
    require!(value.version == SCHEMA_VERSION && value.bump == bump && info.key() == expected
        && value.config == config && value.wallet == wallet && value.graph_hash != [0; 32],
        RegistryError::WalletBinding);
    Ok(value)
}

/// New-risk admission authenticates the currently installed graph as well as
/// enrollment. The caller supplies the actual Swig configuration and wallet;
/// a previously valid binding cannot stand in for present control of cash.
pub fn validate_funding_wallet(binding_info: &AccountInfo, config_info: &AccountInfo,
    wallet_info: &AccountInfo) -> Result<FundingWalletBinding> {
    let binding = read_funding_wallet_binding(binding_info)?;
    require_keys_eq!(config_info.key(), binding.config, RegistryError::WalletBinding);
    require_keys_eq!(wallet_info.key(), binding.wallet, RegistryError::WalletBinding);
    require!(validate_graph_for(config_info, wallet_info, binding_info.key, &binding.wallet_id,
        b"global:authorize_funding_wallet")? == binding.graph_hash, RegistryError::WalletGraph);
    Ok(binding)
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct BindFundingWalletArgs {
    pub wallet_id: [u8; 32],
    pub authority_generation: u64,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct AuthorizeFundingWalletArgs {
    // The first 40 bytes, including the discriminator, are the Swig
    // ProgramExec prefix. The binding key must remain the first argument.
    pub binding: Pubkey,
    pub nonce: u64,
    pub expires_at: i64,
    pub authority_generation: u64,
    pub action: u8,
    pub bundle_hash: [u8; 32],
}

#[derive(Accounts)]
#[instruction(args: BindFundingWalletArgs)]
pub struct BindFundingWallet<'info> {
    /// CHECK: Complete retained Swig graph, owner and PDA checked in the handler.
    pub config: UncheckedAccount<'info>,
    /// CHECK: Exact canonical empty System-owned Swig wallet address checked below.
    pub wallet: UncheckedAccount<'info>,
    #[account(seeds = [b"principal", principal.seed_owner.as_ref(), principal.principal_id.as_ref()], bump = principal.bump)]
    pub principal: Box<Account<'info, Principal>>,
    /// CHECK: Fresh binding requires current native or exact-operation passkey
    /// authority; an existing binding is recognized without changing its nonce.
    pub owner: UncheckedAccount<'info>,
    #[account(mut)] pub payer: Signer<'info>,
    #[account(init_if_needed, payer = payer, space = 8 + FundingWalletBinding::INIT_SPACE,
        seeds = [b"funding-wallet", principal.key().as_ref(), config.key().as_ref()], bump)]
    pub binding: Box<Account<'info, FundingWalletBinding>>,
    /// CHECK: Exact retained executable; its accepted upgrade governance remains a dependency.
    #[account(address = SWIG_ID, executable)] pub swig_program: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(args: AuthorizeFundingWalletArgs)]
pub struct AuthorizeFundingWallet<'info> {
    // ProgramExec requires these first two positions.
    /// CHECK: Complete pinned graph and canonical configuration PDA checked below.
    pub config: UncheckedAccount<'info>,
    /// CHECK: Canonical custody PDA and System ownership checked below.
    pub wallet: UncheckedAccount<'info>,
    #[account(mut, has_one = principal, has_one = config, has_one = wallet,
        seeds = [b"funding-wallet", principal.key().as_ref(), config.key().as_ref()], bump = binding.bump)]
    pub binding: Box<Account<'info, FundingWalletBinding>>,
    #[account(seeds = [b"principal", principal.seed_owner.as_ref(), principal.principal_id.as_ref()], bump = principal.bump)]
    pub principal: Box<Account<'info, Principal>>,
    pub caller: Signer<'info>,
    /// CHECK: Canonical runtime instructions sysvar read through checked APIs.
    #[account(address = INSTRUCTIONS_ID)] pub instructions: UncheckedAccount<'info>,
}

pub fn bind_funding(ctx: Context<BindFundingWallet>, args: BindFundingWalletArgs) -> Result<()> {
    require!(get_stack_height() == TRANSACTION_LEVEL_STACK_HEIGHT, RegistryError::WalletEnvelope);
    let principal = &ctx.accounts.principal;
    require!(principal.version == SCHEMA_VERSION, RegistryError::WalletGeneration);
    let graph_hash = validate_graph_for(&ctx.accounts.config, &ctx.accounts.wallet,
        &ctx.accounts.binding.key(), &args.wallet_id, b"global:authorize_funding_wallet")?;
    let binding = &mut ctx.accounts.binding;
    if binding.version != 0 {
        require!(binding.version == SCHEMA_VERSION && binding.principal == principal.key()
            && binding.config == ctx.accounts.config.key() && binding.wallet == ctx.accounts.wallet.key()
            && binding.wallet_id == args.wallet_id && binding.graph_hash == graph_hash,
            RegistryError::WalletBinding);
        return Ok(()); // Never reset a public authorization nonce on setup retry.
    }
    require!(principal.authority_generation == args.authority_generation, RegistryError::WalletGeneration);
    crate::require_owner_authority(principal, &ctx.accounts.owner.to_account_info(), ctx.remaining_accounts)?;
    binding.set_inner(FundingWalletBinding { deposit_payer: ctx.accounts.payer.key(), version: SCHEMA_VERSION, bump: ctx.bumps.binding,
        principal: principal.key(), config: ctx.accounts.config.key(), wallet: ctx.accounts.wallet.key(),
        wallet_id: args.wallet_id, graph_hash, nonce: 0 });
    Ok(())
}

pub fn authorize_funding(ctx: Context<AuthorizeFundingWallet>, args: AuthorizeFundingWalletArgs) -> Result<()> {
    require!(get_stack_height() == TRANSACTION_LEVEL_STACK_HEIGHT, RegistryError::WalletEnvelope);
    let principal = &ctx.accounts.principal;
    let binding = &ctx.accounts.binding;
    require!(principal.version == SCHEMA_VERSION && binding.version == SCHEMA_VERSION,
        RegistryError::WalletBinding);
    require_keys_eq!(args.binding, binding.key(), RegistryError::WalletBinding);
    let sysvar = ctx.accounts.instructions.to_account_info();
    let current = load_current_index_checked(&sysvar)? as usize;
    // The explicit authorized-withdrawal profile alone separates the native
    // fee/rent caller from its owner. One remaining account is that authority;
    // the instructions sysvar is already fixed above, never another guardian.
    let withdrawal_owner = if args.action == FUNDING_WITHDRAW && ctx.remaining_accounts.len() == 1 {
        Some(&ctx.remaining_accounts[0])
    } else { None };
    if withdrawal_owner.map(|owner| owner.key().is_on_curve()).unwrap_or(true) {
        // Every existing consumer and native-owner withdrawal retains the
        // original v1 envelope. Its resource settings belong to the header.
        require!(current == 0, RegistryError::WalletEnvelope);
    }
    let own = load_instruction_at_checked(current, &sysvar)?;
    require!(own.program_id == crate::ID
        && own.data == crate::instruction::AuthorizeFundingWallet { args: args.clone() }.data(),
        RegistryError::WalletEnvelope);
    let next = load_instruction_at_checked(current + 1, &sysvar)?;
    require!(load_instruction_at_checked(current + 2, &sysvar).is_err(), RegistryError::WalletEnvelope);
    let raw = canonical_instruction(&next)?;
    let bundle = hashv(&[b"DEXTER:FUNDING-SWIG-BUNDLE:1", crate::ID.as_ref(), CORE_ID.as_ref(),
        SWIG_ID.as_ref(), &principal.domain, principal.key().as_ref(), binding.key().as_ref(),
        &args.nonce.to_le_bytes(), &args.expires_at.to_le_bytes(),
        &args.authority_generation.to_le_bytes(), &[args.action], &raw]).to_bytes();
    require!(bundle == args.bundle_hash, RegistryError::WalletBundleHash);
    if matches!(args.action, FUNDING_REPAY_RUN | FUNDING_REPAY_FACILITY) {
        crate::repayment::authorize_envelope(&next, ctx.accounts, &args, ctx.remaining_accounts)?;
    } else if matches!(args.action, FUNDING_SEND | FUNDING_SWAP) {
        crate::spending::authorize_envelope(&next, ctx.accounts, &args, ctx.remaining_accounts)?;
    } else if let Some(owner) = withdrawal_owner {
        require!(args.authority_generation > 0
            && args.authority_generation == principal.authority_generation, RegistryError::WalletGeneration);
        crate::require_owner_authority(principal, owner, core::slice::from_ref(&sysvar))?;
        validate_funding_instruction(&next, ctx.accounts.config.key(), ctx.accounts.wallet.key(),
            principal.key(), binding.key(), ctx.accounts.caller.key(), args.action,
            Some((owner.key(), args.authority_generation)))?;
    } else {
        require!(ctx.remaining_accounts.is_empty(), RegistryError::FundingAction);
        validate_action_authority(args.action, args.authority_generation, ctx.accounts.caller.key(),
            principal.owner, principal.authority_generation)?;
        validate_funding_instruction(&next, ctx.accounts.config.key(), ctx.accounts.wallet.key(),
            principal.key(), binding.key(), ctx.accounts.caller.key(), args.action, None)?;
    }
    require!(args.nonce == binding.nonce, RegistryError::WalletNonce);
    require!(args.expires_at > Clock::get()?.unix_timestamp, RegistryError::WalletExpired);
    require!(validate_graph_for(&ctx.accounts.config, &ctx.accounts.wallet, &binding.key(),
        &binding.wallet_id, b"global:authorize_funding_wallet")? == binding.graph_hash,
        RegistryError::WalletGraph);
    ctx.accounts.binding.nonce = plus(args.nonce, 1)?;
    Ok(())
}

fn validate_action_authority(action: u8, generation: u64, caller: Pubkey, owner: Pubkey,
    current_generation: u64) -> Result<()> {
    match action {
        FUNDING_COLLECT => require!(generation == 0, RegistryError::WalletGeneration),
        FUNDING_WITHDRAW => {
            require!(generation > 0 && generation == current_generation, RegistryError::WalletGeneration);
            require_keys_eq!(caller, owner, RegistryError::WalletOwner);
        }
        _ => return err!(RegistryError::FundingAction),
    }
    Ok(())
}

fn validate_funding_instruction(ix: &Instruction, config: Pubkey, wallet: Pubkey,
    principal: Pubkey, binding: Pubkey, caller: Pubkey, action: u8,
    withdrawal_owner: Option<(Pubkey, u64)>) -> Result<()> {
    // SignV2's compact ABI uses u8 outer indices. This bound is the retained
    // program's encoding, not a packet-size or transaction-version restriction.
    require!(ix.program_id == SWIG_ID && ix.accounts.len() >= 4 && ix.accounts.len() <= 256,
        RegistryError::FundingAction);
    let meta = |index: usize| ix.accounts.get(index).ok_or_else(|| error!(RegistryError::FundingAction));
    require!(meta(0)?.pubkey == config && meta(1)?.pubkey == wallet && meta(2)?.pubkey == INSTRUCTIONS_ID
        && meta(0)?.is_writable && meta(1)?.is_writable && !meta(0)?.is_signer && !meta(1)?.is_signer
        && !meta(2)?.is_signer && !meta(2)?.is_writable, RegistryError::FundingAction);
    let frame = CompactFrame::parse(&ix.data, 6..=255)?;
    let count = frame.role_count();
    require!(meta(frame.program_index())?.pubkey == CORE_ID, RegistryError::FundingAction);
    let inner = frame.inner_data()?;
    let name: &[u8] = match (action, withdrawal_owner) {
        (FUNDING_WITHDRAW, Some(_)) => b"global:withdraw_financier_free_authorized",
        (FUNDING_COLLECT, None) => b"global:collect_financier",
        (FUNDING_WITHDRAW, None) if inner[..8] == hashv(&[b"global:withdraw_wallet_tokens"]).to_bytes()[..8] => b"global:withdraw_wallet_tokens",
        (FUNDING_WITHDRAW, None) => b"global:withdraw_financier_free",
        _ => return err!(RegistryError::FundingAction),
    };
    require!(inner[..8] == hashv(&[name]).to_bytes()[..8], RegistryError::FundingAction);
    let key = |index: usize| frame.role(&ix.accounts, index);
    require!(key(0)?.pubkey == caller && key(0)?.is_signer && key(0)?.is_writable
        && key(1)?.pubkey == principal && key(2)?.pubkey == binding
        && key(3)?.pubkey == custody_reserve_address(&key(4)?.pubkey)
        && key(5)?.pubkey == wallet, RegistryError::FundingAction);
    if let Some((owner, generation)) = withdrawal_owner {
        // First twelve roles remain the native withdrawal ABI (Change 5:
        // ClaimsCore's event authority in the receipt's former role 7). Owner and
        // instructions are appended, then ClaimsCore itself, then the exact
        // one-principal book suffix. The owner proof commits this entire Swig
        // instruction through the Registry bundle hash; no generic CPI financial
        // authority exists.
        require!(inner.len() == 97 && inner[64] <= 1, RegistryError::FundingAction);
        let without_pool = inner[64] == 1;
        require!(count == if without_pool { 16 } else { 17 }, RegistryError::FundingAction);
        let amount = u64::from_le_bytes(inner[40..48].try_into().unwrap());
        let inner_generation = u64::from_le_bytes(inner[56..64].try_into().unwrap());
        let destination_owner = Pubkey::new_from_array(inner[65..97].try_into().unwrap());
        require!(amount > 0 && generation > 0 && inner_generation == generation
            && destination_owner != Pubkey::default() && destination_owner.is_on_curve(),
            RegistryError::FundingAction);
        let destination = Pubkey::find_program_address(
            &[destination_owner.as_ref(), TOKEN_ID.as_ref(), key(8)?.pubkey.as_ref()], &ATA_ID).0;
        let event_authority = Pubkey::find_program_address(&[b"__event_authority"], &CORE_ID).0;
        let book = dexter_contracts::funded::book_address(&principal, &key(8)?.pubkey);
        require!(key(6)?.pubkey == destination && key(6)?.pubkey != key(4)?.pubkey
            && key(7)?.pubkey == event_authority && key(9)?.pubkey == TOKEN_ID
            && key(10)?.pubkey == System::id() && key(11)?.pubkey == dexter_contracts::POOL_ID
            && key(12)?.pubkey == owner && key(12)?.is_signer == owner.is_on_curve()
            && key(13)?.pubkey == INSTRUCTIONS_ID && !key(13)?.is_signer && !key(13)?.is_writable
            && key(14)?.pubkey == CORE_ID && key(15)?.pubkey == book, RegistryError::FundingAction);
        for index in [3, 4, 6, 15] {
            require!(key(index)?.is_writable, RegistryError::FundingAction);
        }
        if !without_pool {
            require!(key(16)?.pubkey == dexter_contracts::funded::projection_address(&book)
                && key(16)?.is_writable, RegistryError::FundingAction);
        }
    } else if name == b"global:withdraw_wallet_tokens" {
        // Change 5: ClaimsCore's event authority and ClaimsCore itself in the
        // two receipts' former roles 7 and 12.
        require!(count == 15 && inner.len() == 80, RegistryError::FundingAction);
        let event_authority = Pubkey::find_program_address(&[b"__event_authority"], &CORE_ID).0;
        let book = dexter_contracts::funded::book_address(&principal, &key(8)?.pubkey);
        require!(dexter_contracts::spending::supported_token_program(key(9)?.pubkey)
            && key(7)?.pubkey == event_authority && key(12)?.pubkey == CORE_ID
            && key(10)?.pubkey == System::id() && key(11)?.pubkey == dexter_contracts::POOL_ID
            && key(13)?.pubkey == book
            && key(14)?.pubkey == dexter_contracts::funded::projection_address(&book),
            RegistryError::FundingAction);
        for index in [3, 4, 6, 13, 14] {
            require!(key(index)?.is_writable, RegistryError::FundingAction);
        }
    }
    // Core enforces the rest of its declared Accounts and exact complete book
    // suffix. Registry commits every byte and effective privilege of that call.
    frame.require_covered_outer_indices(&ix.accounts)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use anchor_lang::solana_program::instruction::AccountMeta;
    use crate::swig_framing::test_support::{framed_data, funding_action};

    fn envelope() -> (Instruction, [Pubkey; 5]) {
        let config = Pubkey::new_unique(); let wallet = Pubkey::new_unique();
        let principal = Pubkey::new_unique(); let binding = Pubkey::new_unique();
        let caller = Pubkey::new_unique(); let token = Pubkey::new_unique();
        let custody = custody_reserve_address(&token);
        let mut compact = vec![1, 3, 6, 4, 5, 6, 7, 8, 1, 8, 0];
        compact.extend_from_slice(&hashv(&[b"global:collect_financier"]).to_bytes()[..8]);
        let mut data = Vec::from(11u16.to_le_bytes());
        data.extend_from_slice(&(compact.len() as u16).to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes()); data.extend_from_slice(&compact); data.push(2);
        let accounts = vec![AccountMeta::new(config, false), AccountMeta::new(wallet, false),
            AccountMeta::new_readonly(INSTRUCTIONS_ID, false), AccountMeta::new_readonly(CORE_ID, false),
            AccountMeta::new(caller, true), AccountMeta::new_readonly(principal, false),
            AccountMeta::new(binding, false), AccountMeta::new(custody, false), AccountMeta::new(token, false)];
        (Instruction { program_id: SWIG_ID, accounts, data }, [config, wallet, principal, binding, caller])
    }

    fn admits(ix: &Instruction, keys: &[Pubkey; 5]) -> bool {
        validate_funding_instruction(ix, keys[0], keys[1], keys[2], keys[3], keys[4], FUNDING_COLLECT, None).is_ok()
    }

    #[test]
    fn collection_survives_owner_changes_and_requires_no_owner_generation() {
        let caller = Pubkey::new_unique();
        for generation in [1, 2, u64::MAX] {
            assert!(validate_action_authority(FUNDING_COLLECT, 0, caller, Pubkey::new_unique(), generation).is_ok());
            assert!(validate_action_authority(FUNDING_COLLECT, generation, caller, caller, generation).is_err());
        }
    }

    #[test]
    fn withdrawal_requires_exact_current_owner_and_generation() {
        let old = Pubkey::new_unique(); let current = Pubkey::new_unique();
        assert!(validate_action_authority(FUNDING_WITHDRAW, 2, current, current, 2).is_ok());
        assert!(validate_action_authority(FUNDING_WITHDRAW, 1, current, current, 2).is_err());
        assert!(validate_action_authority(FUNDING_WITHDRAW, 2, old, current, 2).is_err());
        assert!(validate_action_authority(FUNDING_WITHDRAW, 0, current, current, 2).is_err());
        assert!(validate_action_authority(3, 2, current, current, 2).is_err());
    }

    #[test]
    fn authorized_withdrawal_binds_distinct_sponsor_owner_destination_and_book_suffix() {
        // Parser qualification only. The native fixture separately executes
        // owner proofs, Swig, Core, SPL transfer and reservation accounting.
        let native_key = |seed: u8| {
            for tail in 0..=u8::MAX {
                let mut bytes = [seed; 32]; bytes[31] = tail;
                let key = Pubkey::new_from_array(bytes);
                if key.is_on_curve() && key != Pubkey::default() { return key; }
            }
            panic!("no deterministic native test key");
        };
        let config = Pubkey::new_unique(); let wallet = Pubkey::new_unique();
        let principal = Pubkey::new_unique(); let binding = Pubkey::new_unique();
        let caller = native_key(61); let owner = native_key(62); let destination_owner = native_key(63);
        let token = Pubkey::new_unique(); let mint = Pubkey::new_unique();
        let request_id = [42; 32]; let generation = 3u64;
        let destination = Pubkey::find_program_address(
            &[destination_owner.as_ref(), TOKEN_ID.as_ref(), mint.as_ref()], &ATA_ID).0;
        // Change 5: ClaimsCore's event authority in the receipt's former role 7,
        // ClaimsCore itself (outer account 3) as role 14, before the book suffix.
        let event_authority = Pubkey::find_program_address(&[b"__event_authority"], &CORE_ID).0;
        let book = dexter_contracts::funded::book_address(&principal, &mint);
        let projection = dexter_contracts::funded::projection_address(&book);
        let roles = [4, 5, 6, 7, 8, 1, 9, 10, 11, 12, 13, 14, 15, 2, 3, 16, 17];
        let mut inner = hashv(&[b"global:withdraw_financier_free_authorized"]).to_bytes()[..8].to_vec();
        inner.extend_from_slice(&request_id);
        inner.extend_from_slice(&100u64.to_le_bytes());
        inner.extend_from_slice(&7u64.to_le_bytes());
        inner.extend_from_slice(&generation.to_le_bytes());
        inner.push(0);
        inner.extend_from_slice(destination_owner.as_ref());
        assert_eq!(inner.len(), 97);
        let ix = Instruction { program_id: SWIG_ID, data: framed_data(&roles, &inner), accounts: vec![
            AccountMeta::new(config, false), AccountMeta::new(wallet, false),
            AccountMeta::new_readonly(INSTRUCTIONS_ID, false), AccountMeta::new_readonly(CORE_ID, false),
            AccountMeta::new(caller, true), AccountMeta::new_readonly(principal, false),
            AccountMeta::new(binding, false), AccountMeta::new(custody_reserve_address(&token), false),
            AccountMeta::new(token, false), AccountMeta::new(destination, false),
            AccountMeta::new_readonly(event_authority, false), AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(TOKEN_ID, false), AccountMeta::new_readonly(System::id(), false),
            AccountMeta::new_readonly(dexter_contracts::POOL_ID, false), AccountMeta::new_readonly(owner, true),
            AccountMeta::new(book, false), AccountMeta::new(projection, false),
        ] };
        let check = |candidate: &Instruction| validate_funding_instruction(candidate, config, wallet,
            principal, binding, caller, FUNDING_WITHDRAW, Some((owner, generation)));
        check(&ix).unwrap();
        assert_ne!(owner, caller); assert_ne!(owner, destination_owner);
        funding_action(validate_funding_instruction(&ix, config, wallet, principal, binding,
            caller, FUNDING_WITHDRAW, None)); // New profile cannot use the old caller-as-owner branch.
        for index in [9, 10, 12, 13, 14, 15, 16, 17] {
            let mut wrong = ix.clone(); wrong.accounts[index].pubkey = Pubkey::new_unique();
            funding_action(check(&wrong));
        }
        let mut wrong = ix.clone(); wrong.accounts[15].is_signer = false;
        funding_action(check(&wrong));
        let mut wrong = ix.clone(); wrong.accounts[2].is_writable = true;
        funding_action(check(&wrong));
        for offset in [40, 56, 64, 65] {
            let mut changed = inner.clone();
            match offset {
                40 => changed[40..48].copy_from_slice(&0u64.to_le_bytes()),
                56 => changed[56..64].copy_from_slice(&(generation + 1).to_le_bytes()),
                64 => changed[64] = 2,
                _ => changed[offset] ^= 1,
            }
            let mut wrong = ix.clone(); wrong.data = framed_data(&roles, &changed);
            funding_action(check(&wrong));
        }
        let mut without_pool = ix.clone(); let mut without_pool_inner = inner;
        without_pool_inner[64] = 1;
        without_pool.accounts.pop();
        without_pool.data = framed_data(&roles[..16], &without_pool_inner);
        check(&without_pool).unwrap();
        let mut extra = without_pool.clone(); extra.accounts.push(AccountMeta::new(projection, false));
        funding_action(check(&extra));
        // A passkey-shaped alias is never elevated as a native outer signer.
        let alias = Pubkey::find_program_address(&[b"test-withdraw-owner"], &crate::ID).0;
        let mut passkey = ix; passkey.accounts[15] = AccountMeta::new(alias, false);
        validate_funding_instruction(&passkey, config, wallet, principal, binding, caller,
            FUNDING_WITHDRAW, Some((alias, generation))).unwrap();
        passkey.accounts[15].is_signer = true;
        funding_action(validate_funding_instruction(&passkey, config, wallet, principal, binding,
            caller, FUNDING_WITHDRAW, Some((alias, generation))));
    }

    #[test]
    fn exact_compact_profile_rejects_unknown_action_trailer_and_unused_accounts() {
        let (ix, keys) = envelope(); assert!(admits(&ix, &keys));
        let mut wrong = ix.clone(); wrong.data.push(0); assert!(!admits(&wrong, &keys));
        let mut wrong = ix.clone(); wrong.data[4] = 1; assert!(!admits(&wrong, &keys));
        let mut wrong = ix.clone(); wrong.data[8] = 2; assert!(!admits(&wrong, &keys));
        let mut wrong = ix.clone(); let end = wrong.data.len() - 2; wrong.data[end] ^= 1;
        assert!(!admits(&wrong, &keys));
        let mut wrong = ix.clone(); wrong.accounts.push(AccountMeta::new(Pubkey::new_unique(), false));
        assert!(!admits(&wrong, &keys));
    }

    #[test]
    fn compact_prefix_binds_caller_principal_custody_and_wallet() {
        let (ix, keys) = envelope();
        for index in [0, 1, 2, 3, 4, 5, 6, 7, 8] {
            let mut wrong = ix.clone(); wrong.accounts[index].pubkey = Pubkey::new_unique();
            assert!(!admits(&wrong, &keys), "redirected outer account {index}");
        }
        let mut wrong = ix.clone(); wrong.accounts[4].is_signer = false; assert!(!admits(&wrong, &keys));
        let mut wrong = ix.clone(); wrong.accounts[4].is_writable = false; assert!(!admits(&wrong, &keys));
        let mut wrong = ix.clone(); wrong.accounts[1].is_signer = true; assert!(!admits(&wrong, &keys));
        let mut wrong = ix.clone(); wrong.data[11] = 255; assert!(!admits(&wrong, &keys));
    }

    #[test]
    fn collection_prefix_retains_lazy_suffix_checks_and_ordered_repeated_roles() {
        let (ix, keys) = envelope();
        let inner = &hashv(&[b"global:collect_financier"]).to_bytes()[..8];
        let mut repeated = ix.clone();
        repeated.data = framed_data(&[4, 5, 6, 7, 8, 1, 4, 1, 4], inner);
        assert!(admits(&repeated, &keys));
        let mut reordered = ix.clone();
        reordered.data = framed_data(&[4, 5, 6, 7, 8, 1, 4, 4, 1], inner);
        assert!(admits(&reordered, &keys));
        assert_ne!(canonical_instruction(&repeated).unwrap(), canonical_instruction(&reordered).unwrap());

        // This is Registry prefix acceptance only. Swig/Core still own the
        // unresolved suffix and full instruction; no execution is claimed.
        let mut suffix = ix.clone();
        suffix.data = framed_data(&[4, 5, 6, 7, 8, 1, 255], inner);
        assert!(admits(&suffix, &keys));
        for role in 0..6 {
            let mut requested = ix.clone(); requested.data[11 + role] = 255;
            funding_action(validate_funding_instruction(&requested, keys[0], keys[1], keys[2], keys[3], keys[4], FUNDING_COLLECT, None));
        }
    }

    #[test]
    fn collection_prefix_covers_duplicate_outer_positions_by_index() {
        let (mut ix, keys) = envelope();
        ix.accounts.push(ix.accounts[8].clone());
        funding_action(validate_funding_instruction(&ix, keys[0], keys[1], keys[2], keys[3], keys[4], FUNDING_COLLECT, None));
        ix.data = framed_data(&[4, 5, 6, 7, 8, 1, 9], &hashv(&[b"global:collect_financier"]).to_bytes()[..8]);
        assert!(admits(&ix, &keys));
    }

    #[test]
    fn effective_meta_privileges_are_part_of_the_complete_bundle_encoding() {
        let (ix, _) = envelope(); let canonical = canonical_instruction(&ix).unwrap();
        for index in 0..ix.accounts.len() {
            let mut changed = ix.clone(); changed.accounts[index].is_writable = !changed.accounts[index].is_writable;
            assert_ne!(canonical, canonical_instruction(&changed).unwrap());
            let mut changed = ix.clone(); changed.accounts[index].is_signer = !changed.accounts[index].is_signer;
            assert_ne!(canonical, canonical_instruction(&changed).unwrap());
        }
    }

    #[test]
    fn complete_retained_graph_rejects_every_changed_byte_and_another_endpoint() {
        let wallet_id = [42; 32]; let binding = Pubkey::new_unique();
        let (config, bump) = Pubkey::find_program_address(&[b"swig", &wallet_id], &SWIG_ID);
        let (wallet, wallet_bump) = Pubkey::find_program_address(&[b"swig-wallet-address", config.as_ref()], &SWIG_ID);
        let mut original = [0u8; 152];
        original[0] = 1; original[1] = bump; original[2..34].copy_from_slice(&wallet_id);
        original[34..36].copy_from_slice(&1u16.to_le_bytes());
        original[36..40].copy_from_slice(&1u32.to_le_bytes()); original[40] = wallet_bump;
        original[48..50].copy_from_slice(&7u16.to_le_bytes());
        original[50..52].copy_from_slice(&80u16.to_le_bytes()); original[52..54].copy_from_slice(&1u16.to_le_bytes());
        original[60..64].copy_from_slice(&104u32.to_le_bytes()); original[64..96].copy_from_slice(crate::ID.as_ref());
        original[96] = 40;
        original[104..112].copy_from_slice(&hashv(&[b"global:authorize_funding_wallet"]).to_bytes()[..8]);
        original[112..144].copy_from_slice(binding.as_ref());
        original[144..146].copy_from_slice(&7u16.to_le_bytes()); original[148..152].copy_from_slice(&8u32.to_le_bytes());
        let check = |data: &mut [u8], instruction: &[u8], config_owner: &Pubkey| {
            let mut config_lamports = 0; let mut wallet_lamports = 0; let mut wallet_data = [];
            let system = System::id();
            let config_info = AccountInfo::new(&config, false, true, &mut config_lamports, data, config_owner, false);
            let wallet_info = AccountInfo::new(&wallet, false, true, &mut wallet_lamports, &mut wallet_data, &system, false);
            validate_graph_for(&config_info, &wallet_info, &binding, &wallet_id, instruction).is_ok()
        };
        assert!(check(&mut original.clone(), b"global:authorize_funding_wallet", &SWIG_ID));
        for i in 0..152 {
            let mut changed = original; changed[i] ^= 1;
            assert!(!check(&mut changed, b"global:authorize_funding_wallet", &SWIG_ID), "mutated byte {i}");
        }
        assert!(!check(&mut original.clone(), b"global:bind_funding_wallet", &SWIG_ID));
        assert!(!check(&mut original.clone(), b"global:authorize_funding_wallet", &System::id()));
        let mut extra_role = original.to_vec(); extra_role.extend_from_slice(&[0; 8]);
        assert!(!check(&mut extra_role, b"global:authorize_funding_wallet", &SWIG_ID));
    }

    #[test]
    fn core_binding_reader_rejects_owner_size_bump_and_graph_substitution() {
        let principal = Pubkey::new_unique(); let wallet_id = [43; 32];
        let config = Pubkey::find_program_address(&[b"swig", &wallet_id], &SWIG_ID).0;
        let wallet = Pubkey::find_program_address(&[b"swig-wallet-address", config.as_ref()], &SWIG_ID).0;
        let (key, bump) = Pubkey::find_program_address(&[b"funding-wallet", principal.as_ref(), config.as_ref()], &crate::ID);
        let mut binding = FundingWalletBinding { deposit_payer: Pubkey::default(), version: SCHEMA_VERSION, bump, principal, config, wallet,
            wallet_id, graph_hash: [7; 32], nonce: 19 };
        let check = |value: &FundingWalletBinding, owner: &Pubkey, extra: bool| {
            let mut data = Vec::new(); value.try_serialize(&mut data).unwrap(); if extra { data.push(0); }
            let mut lamports = 0;
            let info = AccountInfo::new(&key, false, false, &mut lamports, &mut data, owner, false);
            read_funding_wallet_binding(&info).is_ok()
        };
        assert!(check(&binding, &crate::ID, false));
        assert!(!check(&binding, &System::id(), false));
        assert!(!check(&binding, &crate::ID, true));
        binding.bump ^= 1; assert!(!check(&binding, &crate::ID, false)); binding.bump ^= 1;
        binding.graph_hash = [0; 32]; assert!(!check(&binding, &crate::ID, false));
    }
}
