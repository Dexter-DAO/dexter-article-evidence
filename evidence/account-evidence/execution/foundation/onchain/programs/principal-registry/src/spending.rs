//! Explicit owner-approved sends and bounded delegated exact-input swaps.
//! This account and action domain is separate from repayment permission.
use anchor_lang::prelude::*;
use anchor_lang::solana_program::instruction::{get_stack_height, Instruction, TRANSACTION_LEVEL_STACK_HEIGHT};
use dexter_contracts::{CORE_ID, POOL_ID, REGISTRY_CONSUMER_SEED, SCHEMA_VERSION};
pub use dexter_contracts::spending::{SpendingPermissionTerms, SpendingConsumption, SpendingExecution};
use dexter_contracts::spending::{SPENDING_SEND, SPENDING_SWAP};
use dexter_contracts::funded::{book_address, projection_address, receipt_address};
use dexter_contracts::wallet_operation::{WalletSpendRequest, SPL_TOKEN_SWAP_ID,
    wallet_operation_id, wallet_operation_address, wallet_leg_id, wallet_request_digest,
    read_wallet_spend_receipt};
use solana_sha256_hasher::hashv;
use crate::{Principal, RegistryError, FundingWalletBinding, AuthorizeFundingWallet, AuthorizeFundingWalletArgs};
use crate::wallet::SWIG_ID;

const TOKEN_ID: Pubkey = pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
const ATA_ID: Pubkey = pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
const INSTRUCTIONS_ID: Pubkey = pubkey!("Sysvar1nstructions1111111111111111111111111");

#[account]
#[derive(InitSpace)]
pub struct SpendingPermission {
    pub version: u16,
    pub bump: u8,
    pub principal: Pubkey,
    pub permission_id: [u8; 32],
    pub grant_owner: Pubkey,
    pub terms_digest: [u8; 32],
    pub terms: SpendingPermissionTerms,
    pub active: bool,
    pub consumed: SpendingConsumption,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct GrantSpendingPermissionArgs {
    pub permission_id: [u8; 32],
    pub terms: SpendingPermissionTerms,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct RevokeSpendingPermissionArgs {
    pub authority_generation: u64,
}

#[derive(Accounts)]
#[instruction(args: GrantSpendingPermissionArgs)]
pub struct GrantSpendingPermission<'info> {
    #[account(seeds = [b"principal", principal.seed_owner.as_ref(), principal.principal_id.as_ref()], bump = principal.bump)]
    pub principal: Box<Account<'info, Principal>>,
    pub owner: Signer<'info>,
    #[account(mut)] pub payer: Signer<'info>,
    #[account(has_one = principal,
        seeds = [b"funding-wallet", principal.key().as_ref(), binding.config.as_ref()], bump = binding.bump)]
    pub binding: Box<Account<'info, FundingWalletBinding>>,
    #[account(init_if_needed, payer = payer, space = 8 + SpendingPermission::INIT_SPACE,
        seeds = [b"spending-permission", principal.key().as_ref(), &args.permission_id], bump)]
    pub permission: Box<Account<'info, SpendingPermission>>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct RevokeSpendingPermission<'info> {
    #[account(seeds = [b"principal", principal.seed_owner.as_ref(), principal.principal_id.as_ref()], bump = principal.bump)]
    pub principal: Box<Account<'info, Principal>>,
    pub owner: Signer<'info>,
    #[account(mut, has_one = principal,
        seeds = [b"spending-permission", principal.key().as_ref(), &permission.permission_id], bump = permission.bump)]
    pub permission: Box<Account<'info, SpendingPermission>>,
}

#[derive(Accounts)]
pub struct ConsumeSpendingPermission<'info> {
    #[account(seeds = [REGISTRY_CONSUMER_SEED], bump, seeds::program = CORE_ID)]
    pub core_authority: Signer<'info>,
    #[account(seeds = [b"principal", principal.seed_owner.as_ref(), principal.principal_id.as_ref()], bump = principal.bump)]
    pub principal: Box<Account<'info, Principal>>,
    #[account(has_one = principal,
        seeds = [b"funding-wallet", principal.key().as_ref(), binding.config.as_ref()], bump = binding.bump)]
    pub binding: Box<Account<'info, FundingWalletBinding>>,
    #[account(mut, has_one = principal,
        seeds = [b"spending-permission", principal.key().as_ref(), &permission.permission_id], bump = permission.bump)]
    pub permission: Box<Account<'info, SpendingPermission>>,
}

fn terms_digest(principal: Pubkey, id: [u8; 32], owner: Pubkey,
    terms: &SpendingPermissionTerms) -> Result<[u8; 32]> {
    let mut bytes = Vec::with_capacity(SpendingPermissionTerms::INIT_SPACE);
    terms.serialize(&mut bytes)?;
    Ok(hashv(&[b"DEXTER:SPENDING-PERMISSION:1", crate::ID.as_ref(), principal.as_ref(),
        &id, owner.as_ref(), &bytes]).to_bytes())
}

pub fn grant_spending(ctx: Context<GrantSpendingPermission>, args: GrantSpendingPermissionArgs) -> Result<()> {
    require!(get_stack_height() == TRANSACTION_LEVEL_STACK_HEIGHT, RegistryError::WalletEnvelope);
    let principal = &ctx.accounts.principal;
    let binding = &ctx.accounts.binding;
    let grant_owner = ctx.accounts.owner.key();
    let digest = terms_digest(principal.key(), args.permission_id, grant_owner, &args.terms)?;
    let permission = &mut ctx.accounts.permission;
    if permission.version != 0 {
        require!(permission.version == SCHEMA_VERSION && permission.bump == ctx.bumps.permission
            && permission.principal == principal.key() && permission.permission_id == args.permission_id
            && permission.grant_owner == grant_owner && permission.terms_digest == digest
            && permission.terms == args.terms, RegistryError::SpendingPermission);
        return Ok(()); // Creation retry cannot renew authority, consumption or revocation.
    }
    require!(principal.version == SCHEMA_VERSION && binding.version == SCHEMA_VERSION,
        RegistryError::SchemaVersion);
    require!(grant_owner.is_on_curve() && principal.owner == grant_owner,
        RegistryError::WalletOwner);
    require!(args.terms.authority_generation == principal.authority_generation,
        RegistryError::WalletGeneration);
    args.terms.validate().map_err(|_| error!(RegistryError::SpendingPermission))?;
    require!(args.terms.binding == binding.key() && args.terms.expires_at > Clock::get()?.unix_timestamp,
        RegistryError::SpendingPermission);
    let ata = |authority: Pubkey, mint: Pubkey| Pubkey::find_program_address(
        &[authority.as_ref(), TOKEN_ID.as_ref(), mint.as_ref()], &ATA_ID).0;
    require_keys_eq!(args.terms.debit_from, ata(binding.wallet, args.terms.mint),
        RegistryError::SpendingPermission);
    require_keys_eq!(args.terms.destination, ata(args.terms.destination_authority, args.terms.output_mint),
        RegistryError::SpendingPermission);
    if args.terms.kind == SPENDING_SWAP {
        require_keys_eq!(args.terms.destination_authority, binding.wallet, RegistryError::SpendingPermission);
        require_keys_eq!(args.terms.adapter_program, SPL_TOKEN_SWAP_ID, RegistryError::SpendingPermission);
    }
    permission.set_inner(SpendingPermission { version: SCHEMA_VERSION, bump: ctx.bumps.permission,
        principal: principal.key(), permission_id: args.permission_id, grant_owner, terms_digest: digest,
        terms: args.terms, active: true, consumed: SpendingConsumption::default() });
    Ok(())
}

pub fn revoke_spending(ctx: Context<RevokeSpendingPermission>, args: RevokeSpendingPermissionArgs) -> Result<()> {
    require!(get_stack_height() == TRANSACTION_LEVEL_STACK_HEIGHT, RegistryError::WalletEnvelope);
    require!(ctx.accounts.principal.version == SCHEMA_VERSION && ctx.accounts.permission.version == SCHEMA_VERSION,
        RegistryError::SchemaVersion);
    require_keys_eq!(ctx.accounts.owner.key(), ctx.accounts.principal.owner, RegistryError::WalletOwner);
    require!(args.authority_generation == ctx.accounts.principal.authority_generation,
        RegistryError::WalletGeneration);
    ctx.accounts.permission.active = false;
    Ok(())
}

pub fn consume_spending(ctx: Context<ConsumeSpendingPermission>, args: SpendingExecution) -> Result<()> {
    require!(get_stack_height() > TRANSACTION_LEVEL_STACK_HEIGHT, RegistryError::WalletEnvelope);
    let permission = &mut ctx.accounts.permission;
    require!(permission.version == SCHEMA_VERSION && ctx.accounts.principal.version == SCHEMA_VERSION
        && ctx.accounts.binding.version == SCHEMA_VERSION && permission.active,
        RegistryError::SpendingPermission);
    require!(args.binding == ctx.accounts.binding.key() && permission.terms.binding == args.binding,
        RegistryError::WalletBinding);
    require!(args.authority_generation == ctx.accounts.principal.authority_generation
        && permission.terms.authority_generation == args.authority_generation,
        RegistryError::WalletGeneration);
    permission.consumed = permission.terms.consume(permission.consumed, &args, Clock::get()?.unix_timestamp)
        .map_err(|_| error!(RegistryError::SpendingPermission))?;
    Ok(())
}

pub fn read_spending_permission(info: &AccountInfo) -> Result<SpendingPermission> {
    require_keys_eq!(*info.owner, crate::ID, RegistryError::SpendingPermission);
    require!(!info.executable && info.data_len() == 8 + SpendingPermission::INIT_SPACE,
        RegistryError::SpendingPermission);
    let permission = SpendingPermission::try_deserialize(&mut &info.try_borrow_data()?[..])
        .map_err(|_| error!(RegistryError::SpendingPermission))?;
    let (key, bump) = Pubkey::find_program_address(
        &[b"spending-permission", permission.principal.as_ref(), &permission.permission_id], &crate::ID);
    require!(info.key() == key && permission.bump == bump && permission.version == SCHEMA_VERSION,
        RegistryError::SpendingPermission);
    Ok(permission)
}

#[derive(AnchorDeserialize)]
struct SendArgs {
    request_id: [u8; 32], amount: u64, authority_generation: u64,
    destination_authority: Pubkey,
}
#[derive(AnchorDeserialize)]
struct SwapArgs {
    request_id: [u8; 32], amount: u64, minimum_output: u64, authority_generation: u64,
}
struct SpendingIntent {
    request: WalletSpendRequest,
    digest: [u8; 32],
    permission: Pubkey,
    operation: Pubkey,
}

/// Parse the entire admitted one-instruction SignV2 payload. Core proves all
/// cash/reserve/capital/adapter postconditions; no router identity grants spend.
#[inline(never)]
fn inspect_instruction(ix: &Instruction, accounts: &AuthorizeFundingWallet,
    action: u8) -> Result<SpendingIntent> {
    require!(matches!(action, SPENDING_SEND | SPENDING_SWAP) && ix.program_id == SWIG_ID
        && (4..=256).contains(&ix.accounts.len()), RegistryError::FundingAction);
    let meta = |i: usize| ix.accounts.get(i).ok_or_else(|| error!(RegistryError::FundingAction));
    require!(meta(0)?.pubkey == accounts.config.key() && meta(1)?.pubkey == accounts.wallet.key()
        && meta(2)?.pubkey == INSTRUCTIONS_ID && meta(0)?.is_writable && meta(1)?.is_writable
        && !meta(0)?.is_signer && !meta(1)?.is_signer && !meta(2)?.is_signer && !meta(2)?.is_writable,
        RegistryError::FundingAction);
    let d = &ix.data;
    require!(d.len() >= 12 && d[..2] == 11u16.to_le_bytes() && d[4..8] == [0; 4],
        RegistryError::FundingAction);
    let len = u16::from_le_bytes(d[2..4].try_into().unwrap()) as usize;
    require!(d.len() == 8 + len + 1 && d[8 + len] == 2, RegistryError::FundingAction);
    let compact = &d[8..8 + len];
    require!(compact.len() >= 3 && compact[0] == 1, RegistryError::FundingAction);
    let fixed = if action == SPENDING_SEND { 16 } else { 26 };
    let book_count = if action == SPENDING_SEND { 2 } else { 4 };
    let count = compact[2] as usize;
    require!(count == fixed + book_count && compact.len() >= 5 + count,
        RegistryError::FundingAction);
    require_keys_eq!(meta(compact[1] as usize)?.pubkey, CORE_ID, RegistryError::FundingAction);
    let indices = &compact[3..3 + count];
    let inner_len = u16::from_le_bytes(compact[3 + count..5 + count].try_into().unwrap()) as usize;
    require!(inner_len >= 8 && compact.len() == 5 + count + inner_len, RegistryError::FundingAction);
    let inner = &compact[5 + count..];
    let key = |i: usize| meta(indices[i] as usize);
    require!(key(0)?.pubkey == accounts.caller.key() && key(0)?.is_signer && key(0)?.is_writable
        && key(1)?.pubkey == accounts.wallet.key() && key(2)?.pubkey == accounts.principal.key()
        && key(3)?.pubkey == accounts.binding.key() && key(4)?.pubkey != CORE_ID
        && key(10)?.pubkey == Pubkey::find_program_address(&[REGISTRY_CONSUMER_SEED], &CORE_ID).0
        && key(11)?.pubkey == crate::ID && key(12)?.pubkey == POOL_ID
        && key(13)?.pubkey == TOKEN_ID && key(14)?.pubkey == System::id(),
        RegistryError::FundingAction);
    for index in [4, 5, 6, 8, 9] {
        require!(key(index)?.is_writable, RegistryError::FundingAction);
    }
    for (index, _) in ix.accounts.iter().enumerate() {
        require!(index < 3 || index == compact[1] as usize || indices.contains(&(index as u8)),
            RegistryError::FundingAction);
    }
    let request = if action == SPENDING_SEND {
        require!(inner[..8] == hashv(&[b"global:wallet_send"]).to_bytes()[..8], RegistryError::FundingAction);
        let args = SendArgs::try_from_slice(&inner[8..]).map_err(|_| error!(RegistryError::FundingAction))?;
        require!(key(15)?.is_writable, RegistryError::FundingAction);
        WalletSpendRequest { request_id: args.request_id, kind: action,
            principal: key(2)?.pubkey, binding: key(3)?.pubkey, wallet_authority: key(1)?.pubkey,
            source_custody: key(5)?.pubkey, source: key(6)?.pubkey, input_mint: key(7)?.pubkey,
            destination: key(15)?.pubkey, destination_authority: args.destination_authority,
            output_mint: key(7)?.pubkey, output_custody: Pubkey::default(),
            adapter_program: Pubkey::default(), adapter_pool: Pubkey::default(), amount: args.amount,
            minimum_output: 0, authority_generation: args.authority_generation }
    } else {
        require!(inner[..8] == hashv(&[b"global:wallet_swap_exact_input"]).to_bytes()[..8], RegistryError::FundingAction);
        let args = SwapArgs::try_from_slice(&inner[8..]).map_err(|_| error!(RegistryError::FundingAction))?;
        for index in [15, 16, 18, 22, 23, 24, 25] {
            require!(key(index)?.is_writable, RegistryError::FundingAction);
        }
        WalletSpendRequest { request_id: args.request_id, kind: action,
            principal: key(2)?.pubkey, binding: key(3)?.pubkey, wallet_authority: key(1)?.pubkey,
            source_custody: key(5)?.pubkey, source: key(6)?.pubkey, input_mint: key(7)?.pubkey,
            destination: key(16)?.pubkey, destination_authority: key(1)?.pubkey,
            output_mint: key(17)?.pubkey, output_custody: key(15)?.pubkey,
            adapter_program: key(19)?.pubkey, adapter_pool: key(20)?.pubkey, amount: args.amount,
            minimum_output: args.minimum_output, authority_generation: args.authority_generation }
    };
    request.validate().map_err(|_| error!(RegistryError::FundingAction))?;
    let operation_id = wallet_operation_id(&request.principal, &request.request_id);
    let operation = wallet_operation_address(&request.principal, &request.request_id);
    require_keys_eq!(key(8)?.pubkey, operation, RegistryError::FundingAction);
    require_keys_eq!(key(9)?.pubkey, receipt_address(&request.principal, &wallet_leg_id(&operation_id, 0)),
        RegistryError::FundingAction);
    let input_book = book_address(&request.principal, &request.input_mint);
    require!(key(fixed)?.pubkey == input_book && key(fixed + 1)?.pubkey == projection_address(&input_book),
        RegistryError::FundingAction);
    if action == SPENDING_SWAP {
        let output_book = book_address(&request.principal, &request.output_mint);
        require!(key(18)?.pubkey == receipt_address(&request.principal, &wallet_leg_id(&operation_id, 1))
            && key(fixed + 2)?.pubkey == output_book && key(fixed + 3)?.pubkey == projection_address(&output_book),
            RegistryError::FundingAction);
    }
    for index in fixed..count { require!(key(index)?.is_writable, RegistryError::FundingAction); }
    let digest = wallet_request_digest(&request)?;
    Ok(SpendingIntent { request, digest, permission: key(4)?.pubkey, operation })
}

/// Recorded effects are authenticated before any current permission, nonce,
/// owner-generation or expiry check; no grant is consumed for historical money.
#[inline(never)]
pub(crate) fn authorize_envelope(ix: &Instruction, accounts: &AuthorizeFundingWallet,
    args: &AuthorizeFundingWalletArgs, remaining: &[AccountInfo]) -> Result<bool> {
    require!(remaining.len() == 2, RegistryError::FundingAction);
    let intent = inspect_instruction(ix, accounts, args.action)?;
    require!(remaining[0].key() == intent.permission && remaining[1].key() == intent.operation,
        RegistryError::FundingAction);
    if !remaining[1].data_is_empty() {
        let receipt = read_wallet_spend_receipt(&remaining[1])?;
        require!(receipt.request == intent.request && receipt.request_digest == intent.digest,
            RegistryError::SpendingPermission);
        return Ok(true);
    }
    require_keys_eq!(*remaining[1].owner, System::id(), RegistryError::SpendingPermission);
    let permission = read_spending_permission(&remaining[0])?;
    require!(permission.active && permission.principal == accounts.principal.key(), RegistryError::SpendingPermission);
    let request = &intent.request;
    require!(args.authority_generation == accounts.principal.authority_generation
        && args.authority_generation == request.authority_generation, RegistryError::WalletGeneration);
    // This preflight uses the smallest quoted output. Core supplies observed
    // deltas to the same validator after its checked adapter execution.
    let execution = SpendingExecution { binding: request.binding, kind: request.kind,
        mint: request.input_mint, debit_from: request.source, output_mint: request.output_mint,
        destination: request.destination, destination_authority: request.destination_authority,
        adapter_program: request.adapter_program, adapter_pool: request.adapter_pool,
        executor: accounts.caller.key(), authority_generation: request.authority_generation,
        request_digest: intent.digest, receipt: intent.operation, amount: request.amount,
        minimum_output: request.minimum_output,
        amount_out: if request.kind == SPENDING_SEND { request.amount } else { request.minimum_output } };
    permission.terms.consume(permission.consumed, &execution, Clock::get()?.unix_timestamp)
        .map_err(|_| error!(RegistryError::SpendingPermission))?;
    Ok(false)
}
