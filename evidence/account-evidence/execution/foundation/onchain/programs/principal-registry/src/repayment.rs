//! Owner-issued exact approvals and bounded standing repayment permission.
//! Revocation and owner rotation constrain future cash debits. Historical Core
//! repayment receipts remain authoritative and never consume permission twice.
use anchor_lang::prelude::*;
use anchor_lang::solana_program::instruction::{get_stack_height, Instruction, TRANSACTION_LEVEL_STACK_HEIGHT};
use dexter_contracts::{CORE_ID, REGISTRY_CONSUMER_SEED, SCHEMA_VERSION};
use dexter_contracts::financing::custody_reserve_address;
use dexter_contracts::funded::{read_finance_receipt, receipt_address, FINANCE_REPAY, FINANCE_FACILITY_REPAY};
pub use dexter_contracts::repayment::{RepaymentPermissionTerms, RepaymentConsumption, RepaymentExecution};
use dexter_contracts::repayment::{RunRepaymentRequest, FacilityRepaymentRequest,
    run_repayment_digest, facility_repayment_digest, REPAYMENT_RUN, REPAYMENT_FACILITY};
use solana_sha256_hasher::hashv;
use crate::{Principal, RegistryError, FundingWalletBinding, AuthorizeFundingWallet, AuthorizeFundingWalletArgs};
use crate::wallet::SWIG_ID;

const TOKEN_ID: Pubkey = pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
const ATA_ID: Pubkey = pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
const INSTRUCTIONS_ID: Pubkey = pubkey!("Sysvar1nstructions1111111111111111111111111");

#[account]
#[derive(InitSpace)]
pub struct RepaymentPermission {
    pub version: u16,
    pub bump: u8,
    pub principal: Pubkey,
    pub permission_id: [u8; 32],
    pub grant_owner: Pubkey,
    pub terms_digest: [u8; 32],
    pub terms: RepaymentPermissionTerms,
    pub active: bool,
    pub consumed: RepaymentConsumption,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct GrantRepaymentPermissionArgs {
    pub permission_id: [u8; 32],
    pub terms: RepaymentPermissionTerms,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct RevokeRepaymentPermissionArgs {
    pub authority_generation: u64,
}

#[derive(Accounts)]
#[instruction(args: GrantRepaymentPermissionArgs)]
pub struct GrantRepaymentPermission<'info> {
    #[account(seeds = [b"principal", principal.seed_owner.as_ref(), principal.principal_id.as_ref()], bump = principal.bump)]
    pub principal: Box<Account<'info, Principal>>,
    // Historical enrollment replay is checked before current ownership.
    pub owner: Signer<'info>,
    #[account(mut)] pub payer: Signer<'info>,
    #[account(has_one = principal, seeds = [b"funding-wallet", principal.key().as_ref(), binding.config.as_ref()], bump = binding.bump)]
    pub binding: Box<Account<'info, FundingWalletBinding>>,
    #[account(init_if_needed, payer = payer, space = 8 + RepaymentPermission::INIT_SPACE,
        seeds = [b"repayment-permission", principal.key().as_ref(), &args.permission_id], bump)]
    pub permission: Box<Account<'info, RepaymentPermission>>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct RevokeRepaymentPermission<'info> {
    #[account(seeds = [b"principal", principal.seed_owner.as_ref(), principal.principal_id.as_ref()], bump = principal.bump)]
    pub principal: Box<Account<'info, Principal>>,
    pub owner: Signer<'info>,
    #[account(mut, has_one = principal,
        seeds = [b"repayment-permission", principal.key().as_ref(), &permission.permission_id], bump = permission.bump)]
    pub permission: Box<Account<'info, RepaymentPermission>>,
}

#[derive(Accounts)]
pub struct ConsumeRepaymentPermission<'info> {
    #[account(seeds = [REGISTRY_CONSUMER_SEED], bump, seeds::program = CORE_ID)]
    pub core_authority: Signer<'info>,
    #[account(seeds = [b"principal", principal.seed_owner.as_ref(), principal.principal_id.as_ref()], bump = principal.bump)]
    pub principal: Box<Account<'info, Principal>>,
    #[account(has_one = principal,
        seeds = [b"funding-wallet", principal.key().as_ref(), binding.config.as_ref()], bump = binding.bump)]
    pub binding: Box<Account<'info, FundingWalletBinding>>,
    #[account(mut, has_one = principal,
        seeds = [b"repayment-permission", principal.key().as_ref(), &permission.permission_id], bump = permission.bump)]
    pub permission: Box<Account<'info, RepaymentPermission>>,
}

fn terms_digest(principal: Pubkey, id: [u8; 32], owner: Pubkey,
    terms: &RepaymentPermissionTerms) -> Result<[u8; 32]> {
    let mut bytes = Vec::with_capacity(RepaymentPermissionTerms::INIT_SPACE);
    terms.serialize(&mut bytes)?;
    Ok(hashv(&[b"DEXTER:REPAYMENT-PERMISSION:1", crate::ID.as_ref(), principal.as_ref(),
        &id, owner.as_ref(), &bytes]).to_bytes())
}

pub fn grant(ctx: Context<GrantRepaymentPermission>, args: GrantRepaymentPermissionArgs) -> Result<()> {
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
            && permission.terms == args.terms, RegistryError::RepaymentPermission);
        return Ok(()); // Never restore active state, counters, timing or old owner authority.
    }
    require!(principal.version == SCHEMA_VERSION && binding.version == SCHEMA_VERSION,
        RegistryError::SchemaVersion);
    require!(grant_owner.is_on_curve() && principal.owner == grant_owner,
        RegistryError::WalletOwner);
    require!(args.terms.authority_generation == principal.authority_generation,
        RegistryError::WalletGeneration);
    args.terms.validate().map_err(|_| error!(RegistryError::RepaymentPermission))?;
    require!(args.terms.binding == binding.key() && args.terms.expires_at > Clock::get()?.unix_timestamp,
        RegistryError::RepaymentPermission);
    let source = Pubkey::find_program_address(
        &[binding.wallet.as_ref(), TOKEN_ID.as_ref(), args.terms.mint.as_ref()], &ATA_ID).0;
    require_keys_eq!(args.terms.debit_from, source, RegistryError::RepaymentPermission);
    require_keys_eq!(args.terms.creditor_custody, custody_reserve_address(&args.terms.creditor_destination),
        RegistryError::RepaymentPermission);
    permission.set_inner(RepaymentPermission { version: SCHEMA_VERSION, bump: ctx.bumps.permission,
        principal: principal.key(), permission_id: args.permission_id, grant_owner, terms_digest: digest,
        terms: args.terms, active: true, consumed: RepaymentConsumption::default() });
    Ok(())
}

pub fn revoke(ctx: Context<RevokeRepaymentPermission>, args: RevokeRepaymentPermissionArgs) -> Result<()> {
    require!(get_stack_height() == TRANSACTION_LEVEL_STACK_HEIGHT, RegistryError::WalletEnvelope);
    require!(ctx.accounts.principal.version == SCHEMA_VERSION && ctx.accounts.permission.version == SCHEMA_VERSION,
        RegistryError::SchemaVersion);
    require_keys_eq!(ctx.accounts.owner.key(), ctx.accounts.principal.owner, RegistryError::WalletOwner);
    require!(args.authority_generation == ctx.accounts.principal.authority_generation,
        RegistryError::WalletGeneration);
    ctx.accounts.permission.active = false;
    Ok(())
}

pub fn consume(ctx: Context<ConsumeRepaymentPermission>, args: RepaymentExecution) -> Result<()> {
    require!(get_stack_height() > TRANSACTION_LEVEL_STACK_HEIGHT, RegistryError::WalletEnvelope);
    let permission = &mut ctx.accounts.permission;
    require!(permission.version == SCHEMA_VERSION && ctx.accounts.principal.version == SCHEMA_VERSION
        && ctx.accounts.binding.version == SCHEMA_VERSION && permission.active,
        RegistryError::RepaymentPermission);
    require!(args.binding == ctx.accounts.binding.key() && permission.terms.binding == args.binding,
        RegistryError::WalletBinding);
    require!(args.authority_generation == ctx.accounts.principal.authority_generation
        && permission.terms.authority_generation == args.authority_generation,
        RegistryError::WalletGeneration);
    permission.consumed = permission.terms.consume(permission.consumed, &args, Clock::get()?.unix_timestamp)
        .map_err(|_| error!(RegistryError::RepaymentPermission))?;
    Ok(())
}

pub fn read_repayment_permission(info: &AccountInfo) -> Result<RepaymentPermission> {
    require_keys_eq!(*info.owner, crate::ID, RegistryError::RepaymentPermission);
    require!(!info.executable && info.data_len() == 8 + RepaymentPermission::INIT_SPACE,
        RegistryError::RepaymentPermission);
    let permission = RepaymentPermission::try_deserialize(&mut &info.try_borrow_data()?[..])
        .map_err(|_| error!(RegistryError::RepaymentPermission))?;
    let (key, bump) = Pubkey::find_program_address(
        &[b"repayment-permission", permission.principal.as_ref(), &permission.permission_id], &crate::ID);
    require!(info.key() == key && permission.bump == bump && permission.version == SCHEMA_VERSION,
        RegistryError::RepaymentPermission);
    Ok(permission)
}

#[derive(AnchorDeserialize)]
struct RunArgs {
    request_id: [u8; 32], amount: u64, maximum_interest: u64,
    _expected_claim_revision: u64, expected_ownership_revision: u64,
    repayment_owner_generation: u64, _service_without_pool: bool,
}
#[derive(AnchorDeserialize)]
struct FacilityArgs {
    request_id: [u8; 32], amount: u64, maximum_interest: u64,
    _expected_facility_revision: u64, repayment_owner_generation: u64,
    _service_without_pool: bool,
}

/// Bounded request summary extracted from the exact next Swig instruction. Live
/// creditor identity and monetary split are independently enforced by Core.
struct Envelope {
    principal: Pubkey, obligation: Pubkey, claim: Pubkey, run: Pubkey,
    mint: Pubkey, from: Pubkey, creditor_custody: Pubkey, destination: Pubkey,
    protocol_destination: Option<Pubkey>,
    operation_id: [u8; 32], digest: [u8; 32], amount: u64, maximum_interest: u64,
    owner_generation: u64, permission: Pubkey, receipt: Pubkey,
}

#[inline(never)]
fn inspect_instruction(ix: &Instruction, accounts: &AuthorizeFundingWallet,
    action: u8) -> Result<Envelope> {
    require!(matches!(action, REPAYMENT_RUN | REPAYMENT_FACILITY)
        && ix.program_id == SWIG_ID && (4..=256).contains(&ix.accounts.len()), RegistryError::FundingAction);
    let meta = |i: usize| ix.accounts.get(i).ok_or_else(|| error!(RegistryError::FundingAction));
    require!(meta(0)?.pubkey == accounts.config.key() && meta(1)?.pubkey == accounts.wallet.key()
        && meta(2)?.pubkey == INSTRUCTIONS_ID && meta(0)?.is_writable && meta(1)?.is_writable
        && !meta(0)?.is_signer && !meta(1)?.is_signer && !meta(2)?.is_signer && !meta(2)?.is_writable,
        RegistryError::FundingAction);
    let d = &ix.data;
    require!(d.len() >= 12 && d[..2] == 11u16.to_le_bytes() && d[4..8] == [0; 4], RegistryError::FundingAction);
    let len = u16::from_le_bytes(d[2..4].try_into().unwrap()) as usize;
    require!(d.len() == 8 + len + 1 && d[8 + len] == 2, RegistryError::FundingAction);
    let compact = &d[8..8 + len];
    require!(compact.len() >= 3 && compact[0] == 1, RegistryError::FundingAction);
    let count = compact[2] as usize;
    let fixed = if action == REPAYMENT_RUN { 26 } else { 20 };
    require!((fixed..=fixed + 16).contains(&count) && compact.len() >= 5 + count,
        RegistryError::FundingAction);
    require_keys_eq!(meta(compact[1] as usize)?.pubkey, CORE_ID, RegistryError::FundingAction);
    let indices = &compact[3..3 + count];
    let inner_len = u16::from_le_bytes(compact[3 + count..5 + count].try_into().unwrap()) as usize;
    require!(inner_len >= 8 && compact.len() == 5 + count + inner_len, RegistryError::FundingAction);
    let inner = &compact[5 + count..];
    let key = |i: usize| meta(indices[i] as usize);
    require!(key(0)?.pubkey == accounts.caller.key() && key(0)?.is_signer && key(0)?.is_writable
        && key(1)?.pubkey == accounts.wallet.key() && key(2)?.pubkey == accounts.principal.key(),
        RegistryError::FundingAction);
    for (index, _) in ix.accounts.iter().enumerate() {
        require!(index < 3 || index == compact[1] as usize || indices.contains(&(index as u8)),
            RegistryError::FundingAction);
    }
    let opt = |i: usize| -> Result<Option<Pubkey>> {
        let value = key(i)?.pubkey; Ok(if value == CORE_ID { None } else { Some(value) })
    };
    let (from, binding, custody, permission, registry_authority, registry_program, token_program, system_program) =
        if action == REPAYMENT_RUN { (14,23,24,25,8,9,21,22) } else { (6,17,18,19,15,16,13,14) };
    require!(key(binding)?.pubkey == accounts.binding.key()
        && key(custody)?.pubkey == custody_reserve_address(&key(from)?.pubkey)
        && key(registry_authority)?.pubkey == Pubkey::find_program_address(&[REGISTRY_CONSUMER_SEED], &CORE_ID).0
        && key(registry_program)?.pubkey == crate::ID && key(token_program)?.pubkey == TOKEN_ID
        && key(system_program)?.pubkey == System::id() && key(permission)?.pubkey != CORE_ID
        && key(permission)?.is_writable && key(custody)?.is_writable, RegistryError::FundingAction);
    if action == REPAYMENT_RUN {
        require!(inner[..8] == hashv(&[b"global:repay_governed_funded_run"]).to_bytes()[..8], RegistryError::FundingAction);
        let args = RunArgs::try_from_slice(&inner[8..]).map_err(|_| error!(RegistryError::FundingAction))?;
        let request = RunRepaymentRequest { principal: key(3)?.pubkey, claim: key(11)?.pubkey, run: key(12)?.pubkey,
            repayment_principal: key(2)?.pubkey, repayment_authority: key(1)?.pubkey,
            repayment_owner_generation: args.repayment_owner_generation, from: key(14)?.pubkey,
            creditor_custody: key(13)?.pubkey, destination: key(15)?.pubkey, mint: key(18)?.pubkey,
            amount: args.amount, maximum_interest: args.maximum_interest,
            expected_ownership_revision: args.expected_ownership_revision,
            protocol_custody: opt(16)?, protocol_destination: opt(17)? };
        let operation_id = repayment_operation_id(b"repay-run", accounts.wallet.key(), &args.request_id);
        require_keys_eq!(key(19)?.pubkey, receipt_address(&request.principal, &operation_id), RegistryError::FundingAction);
        Ok(Envelope { principal: request.principal, obligation: request.run, claim: request.claim, run: request.run,
            mint: request.mint, from: request.from, creditor_custody: request.creditor_custody,
            destination: request.destination, protocol_destination: request.protocol_destination,
            operation_id, digest: run_repayment_digest(&request)?, amount: request.amount,
            maximum_interest: request.maximum_interest, owner_generation: args.repayment_owner_generation,
            permission: key(permission)?.pubkey, receipt: key(19)?.pubkey })
    } else {
        require!(inner[..8] == hashv(&[b"global:repay_governed_funding_facility"]).to_bytes()[..8], RegistryError::FundingAction);
        let args = FacilityArgs::try_from_slice(&inner[8..]).map_err(|_| error!(RegistryError::FundingAction))?;
        let request = FacilityRepaymentRequest { issuer: key(3)?.pubkey, facility: key(4)?.pubkey,
            repayment_principal: key(2)?.pubkey, repayment_authority: key(1)?.pubkey,
            repayment_owner_generation: args.repayment_owner_generation, from: key(6)?.pubkey,
            financier_custody: key(5)?.pubkey, destination: key(7)?.pubkey, mint: key(10)?.pubkey,
            amount: args.amount, maximum_interest: args.maximum_interest,
            protocol_custody: opt(8)?, protocol_destination: opt(9)? };
        let operation_id = repayment_operation_id(b"repay-facility", accounts.wallet.key(), &args.request_id);
        require_keys_eq!(key(11)?.pubkey, receipt_address(&request.issuer, &operation_id), RegistryError::FundingAction);
        Ok(Envelope { principal: request.issuer, obligation: request.facility, claim: request.facility, run: Pubkey::default(),
            mint: request.mint, from: request.from, creditor_custody: request.financier_custody,
            destination: request.destination, protocol_destination: request.protocol_destination,
            operation_id, digest: facility_repayment_digest(&request)?, amount: request.amount,
            maximum_interest: request.maximum_interest, owner_generation: args.repayment_owner_generation,
            permission: key(permission)?.pubkey, receipt: key(11)?.pubkey })
    }
}

fn repayment_operation_id(tag: &[u8], authority: Pubkey, request: &[u8; 32]) -> [u8; 32] {
    hashv(&[b"DEXTER:FUNDING-OPERATION:1", CORE_ID.as_ref(), tag, authority.as_ref(), request]).to_bytes()
}

/// Returns true for an authenticated historical payment. No live permission,
/// authority generation, nonce, title, balance or expiry is consulted then.
#[inline(never)]
pub(crate) fn authorize_envelope(ix: &Instruction, accounts: &AuthorizeFundingWallet,
    args: &AuthorizeFundingWalletArgs, remaining: &[AccountInfo]) -> Result<bool> {
    require!(remaining.len() == 2, RegistryError::FundingAction);
    let intent = inspect_instruction(ix, accounts, args.action)?;
    require!(remaining[0].key() == intent.permission && remaining[1].key() == intent.receipt,
        RegistryError::FundingAction);
    if !remaining[1].data_is_empty() {
        let receipt = read_finance_receipt(&remaining[1])?;
        let kind = if args.action == REPAYMENT_RUN { FINANCE_REPAY } else { FINANCE_FACILITY_REPAY };
        require!(receipt.request_digest == intent.digest && receipt.kind == kind
            && receipt.operation_id == intent.operation_id && receipt.principal == intent.principal
            && receipt.claim == intent.claim && receipt.run == intent.run && receipt.mint == intent.mint
            && receipt.debit_principal == accounts.principal.key() && receipt.debit_authority == accounts.wallet.key()
            && receipt.debit_from == intent.from && receipt.amount == intent.amount,
            RegistryError::RepaymentPermission);
        return Ok(true);
    }
    require_keys_eq!(*remaining[1].owner, System::id(), RegistryError::RepaymentPermission);
    let permission = read_repayment_permission(&remaining[0])?;
    let terms = &permission.terms;
    require!(permission.active && permission.principal == accounts.principal.key()
        && terms.binding == accounts.binding.key() && terms.executor == accounts.caller.key()
        && terms.kind == args.action && terms.obligation == intent.obligation && terms.obligor == intent.principal
        && terms.mint == intent.mint && terms.debit_from == intent.from
        && terms.creditor_custody == intent.creditor_custody && terms.creditor_destination == intent.destination
        && terms.protocol_destination == intent.protocol_destination.unwrap_or_default(), RegistryError::RepaymentPermission);
    require!(args.authority_generation == accounts.principal.authority_generation
        && args.authority_generation == terms.authority_generation
        && args.authority_generation == intent.owner_generation, RegistryError::WalletGeneration);
    terms.validate().map_err(|_| error!(RegistryError::RepaymentPermission))?;
    let now = Clock::get()?.unix_timestamp;
    require!(now >= terms.not_before && now < terms.expires_at && intent.amount > 0
        && intent.amount <= terms.per_payment_amount && intent.maximum_interest <= terms.per_payment_interest,
        RegistryError::RepaymentPermission);
    if terms.exact_request_digest != [0; 32] {
        require!(terms.exact_request_digest == intent.digest && terms.exact_receipt == intent.receipt
            && terms.exact_amount == intent.amount,
            RegistryError::RepaymentPermission);
    }
    // Actual accrued principal/interest and cumulative consumption are checked
    // by the Core-only consume CPI in the same atomic transaction.
    Ok(false)
}
