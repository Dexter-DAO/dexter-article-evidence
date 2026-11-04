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
use crate::wallet::{canonical_instruction, validate_graph_for, SWIG_ID};

const INSTRUCTIONS_ID: Pubkey = pubkey!("Sysvar1nstructions1111111111111111111111111");
pub const FUNDING_COLLECT: u8 = 1;
pub const FUNDING_WITHDRAW: u8 = 2;
pub const FUNDING_REPAY_RUN: u8 = dexter_contracts::repayment::REPAYMENT_RUN;
pub const FUNDING_REPAY_FACILITY: u8 = dexter_contracts::repayment::REPAYMENT_FACILITY;
pub const FUNDING_SEND: u8 = dexter_contracts::spending::SPENDING_SEND;
pub const FUNDING_SWAP: u8 = dexter_contracts::spending::SPENDING_SWAP;

#[account]
#[derive(InitSpace)]
pub struct FundingWalletBinding {
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
    #[account(address = principal.owner @ RegistryError::WalletOwner)]
    pub owner: Signer<'info>,
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
    require!(principal.version == SCHEMA_VERSION && principal.owner.is_on_curve()
        && principal.authority_generation == args.authority_generation, RegistryError::WalletGeneration);
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
    binding.set_inner(FundingWalletBinding { version: SCHEMA_VERSION, bump: ctx.bumps.binding,
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
    // Every active funding-wallet consumer uses native v1 resource headers.
    // Historical development harnesses do not justify a prefixed v0 envelope.
    require!(current == 0, RegistryError::WalletEnvelope);
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
        if crate::repayment::authorize_envelope(&next, ctx.accounts, &args, ctx.remaining_accounts)? {
            return Ok(()); // Historical money is neither reauthorized nor charged twice.
        }
    } else if matches!(args.action, FUNDING_SEND | FUNDING_SWAP) {
        if crate::spending::authorize_envelope(&next, ctx.accounts, &args, ctx.remaining_accounts)? {
            return Ok(()); // A recorded send/swap cannot consume another grant or nonce.
        }
    } else {
        require!(ctx.remaining_accounts.is_empty(), RegistryError::FundingAction);
        validate_action_authority(args.action, args.authority_generation, ctx.accounts.caller.key(),
            principal.owner, principal.authority_generation)?;
        validate_funding_instruction(&next, ctx.accounts.config.key(), ctx.accounts.wallet.key(),
            principal.key(), binding.key(), ctx.accounts.caller.key(), args.action)?;
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
    principal: Pubkey, binding: Pubkey, caller: Pubkey, action: u8) -> Result<()> {
    // SignV2's compact ABI uses u8 outer indices. This bound is the retained
    // program's encoding, not a packet-size or transaction-version restriction.
    require!(ix.program_id == SWIG_ID && ix.accounts.len() >= 4 && ix.accounts.len() <= 256,
        RegistryError::FundingAction);
    let meta = |index: usize| ix.accounts.get(index).ok_or_else(|| error!(RegistryError::FundingAction));
    require!(meta(0)?.pubkey == config && meta(1)?.pubkey == wallet && meta(2)?.pubkey == INSTRUCTIONS_ID
        && meta(0)?.is_writable && meta(1)?.is_writable && !meta(0)?.is_signer && !meta(1)?.is_signer
        && !meta(2)?.is_signer && !meta(2)?.is_writable, RegistryError::FundingAction);
    let d = &ix.data;
    require!(d.len() >= 12 && d[..2] == 11u16.to_le_bytes() && d[4..8] == [0; 4], RegistryError::FundingAction);
    let len = u16::from_le_bytes(d[2..4].try_into().unwrap()) as usize;
    require!(d.len() == 8 + len + 1 && d[8 + len] == 2, RegistryError::FundingAction);
    let compact = &d[8..8 + len];
    require!(compact.len() >= 3 && compact[0] == 1, RegistryError::FundingAction);
    let count = compact[2] as usize;
    require!(count >= 6 && compact.len() >= 5 + count, RegistryError::FundingAction);
    require!(meta(compact[1] as usize)?.pubkey == CORE_ID, RegistryError::FundingAction);
    let indices = &compact[3..3 + count];
    let inner_len = u16::from_le_bytes(compact[3 + count..5 + count].try_into().unwrap()) as usize;
    require!(inner_len >= 8 && compact.len() == 5 + count + inner_len, RegistryError::FundingAction);
    let inner = &compact[5 + count..];
    let name: &[u8] = match action {
        FUNDING_COLLECT => b"global:collect_financier",
        FUNDING_WITHDRAW => b"global:withdraw_financier_free",
        _ => return err!(RegistryError::FundingAction),
    };
    require!(inner[..8] == hashv(&[name]).to_bytes()[..8], RegistryError::FundingAction);
    let key = |index: usize| meta(indices[index] as usize);
    require!(key(0)?.pubkey == caller && key(0)?.is_signer && key(0)?.is_writable
        && key(1)?.pubkey == principal && key(2)?.pubkey == binding
        && key(3)?.pubkey == custody_reserve_address(&key(4)?.pubkey)
        && key(5)?.pubkey == wallet, RegistryError::FundingAction);
    // Core enforces the rest of its declared Accounts and exact complete book
    // suffix. Registry commits every byte and effective privilege of that call.
    for (index, _) in ix.accounts.iter().enumerate() {
        require!(index < 3 || index == compact[1] as usize || indices.contains(&(index as u8)),
            RegistryError::FundingAction);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use anchor_lang::solana_program::instruction::AccountMeta;

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
        validate_funding_instruction(ix, keys[0], keys[1], keys[2], keys[3], keys[4], FUNDING_COLLECT).is_ok()
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
        assert!(!check(&mut original.clone(), b"global:authorize_wallet", &SWIG_ID));
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
        let mut binding = FundingWalletBinding { version: SCHEMA_VERSION, bump, principal, config, wallet,
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
