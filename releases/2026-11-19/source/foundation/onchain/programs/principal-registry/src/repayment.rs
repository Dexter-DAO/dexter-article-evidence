//! Owner-issued exact approvals and bounded standing repayment permission.
//! Revocation and owner rotation constrain future cash debits. Change 5: a
//! standing grant admits each payment number once through its window, and an
//! exact approval is used once; neither is re-authorized by history.
use anchor_lang::prelude::*;
use anchor_lang::solana_program::instruction::{get_stack_height, Instruction, TRANSACTION_LEVEL_STACK_HEIGHT};
use dexter_contracts::{CORE_ID, REGISTRY_CONSUMER_SEED, SCHEMA_VERSION};
use dexter_contracts::financing::custody_reserve_address;
use dexter_contracts::spending::ExactRequest;
use dexter_contracts::window::{OrderWindow, WindowState};
pub use dexter_contracts::repayment::{RepaymentPermissionTerms, RepaymentConsumption, RepaymentExecution};
use dexter_contracts::repayment::{RunRepaymentRequest, FacilityRepaymentRequest,
    run_repayment_digest, facility_repayment_digest, REPAYMENT_RUN, REPAYMENT_FACILITY};
use solana_sha256_hasher::hashv;
use crate::{Principal, RegistryError, FundingWalletBinding, AuthorizeFundingWallet, AuthorizeFundingWalletArgs};
use crate::swig_framing::CompactFrame;
use crate::wallet::SWIG_ID;

const TOKEN_ID: Pubkey = pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
const ATA_ID: Pubkey = pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
const INSTRUCTIONS_ID: Pubkey = pubkey!("Sysvar1nstructions1111111111111111111111111");

/// A standing repayment grant. Each governed payment under it names a number
/// from the grant's allocator; the window admits each number once.
#[account]
#[derive(InitSpace)]
pub struct RepaymentPermission {
    pub deposit_payer: Pubkey,
    pub version: u16,
    pub bump: u8,
    pub principal: Pubkey,
    pub permission_id: [u8; 32],
    pub grant_owner: Pubkey,
    pub terms_digest: [u8; 32],
    pub terms: RepaymentPermissionTerms,
    pub active: bool,
    pub consumed: RepaymentConsumption,
    pub window: OrderWindow,
}

/// An exact repayment approval: one payment, for one request digest and amount,
/// at `["approval", principal, number]`; closes only after `expires_at`.
#[account]
#[derive(InitSpace)]
pub struct ExactRepaymentApproval {
    pub deposit_payer: Pubkey,
    pub version: u16,
    pub bump: u8,
    pub principal: Pubkey,
    pub number: u64,
    pub grant_owner: Pubkey,
    pub terms_digest: [u8; 32],
    pub terms: RepaymentPermissionTerms,
    pub exact: ExactRequest,
    pub active: bool,
    pub consumed: RepaymentConsumption,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct GrantRepaymentPermissionArgs {
    pub permission_id: [u8; 32],
    pub terms: RepaymentPermissionTerms,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct GrantExactRepaymentArgs {
    /// Must be the principal's next approval number (`principal.approvals + 1`).
    pub number: u64,
    pub terms: RepaymentPermissionTerms,
    pub exact: ExactRequest,
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
    /// CHECK: Historical enrollment replay matches the recorded identity.
    /// Fresh effects require current native or exact-operation passkey authority.
    pub owner: UncheckedAccount<'info>,
    #[account(mut)] pub payer: Signer<'info>,
    #[account(has_one = principal, seeds = [b"funding-wallet", principal.key().as_ref(), binding.config.as_ref()], bump = binding.bump)]
    pub binding: Box<Account<'info, FundingWalletBinding>>,
    #[account(init_if_needed, payer = payer, space = 8 + RepaymentPermission::INIT_SPACE,
        seeds = [b"repayment-permission", principal.key().as_ref(), &args.permission_id], bump)]
    pub permission: Box<Account<'info, RepaymentPermission>>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(args: GrantExactRepaymentArgs)]
pub struct GrantExactRepayment<'info> {
    /// Writable: the grant takes the principal's next approval number.
    #[account(mut, seeds = [b"principal", principal.seed_owner.as_ref(), principal.principal_id.as_ref()], bump = principal.bump)]
    pub principal: Box<Account<'info, Principal>>,
    /// CHECK: Current native or exact-operation passkey authority is required.
    pub owner: UncheckedAccount<'info>,
    #[account(mut)] pub payer: Signer<'info>,
    #[account(has_one = principal, seeds = [b"funding-wallet", principal.key().as_ref(), binding.config.as_ref()], bump = binding.bump)]
    pub binding: Box<Account<'info, FundingWalletBinding>>,
    #[account(init_if_needed, payer = payer, space = 8 + ExactRepaymentApproval::INIT_SPACE,
        seeds = [b"approval", principal.key().as_ref(), &args.number.to_le_bytes()], bump)]
    pub approval: Box<Account<'info, ExactRepaymentApproval>>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct RevokeRepaymentPermission<'info> {
    #[account(seeds = [b"principal", principal.seed_owner.as_ref(), principal.principal_id.as_ref()], bump = principal.bump)]
    pub principal: Box<Account<'info, Principal>>,
    /// CHECK: Current native or exact-operation passkey authority is required.
    pub owner: UncheckedAccount<'info>,
    /// CHECK: A standing grant or an exact approval of this principal; read by type.
    #[account(mut)]
    pub permission: UncheckedAccount<'info>,
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
    /// CHECK: A standing grant or an exact approval of this principal; read by type.
    #[account(mut)]
    pub permission: UncheckedAccount<'info>,
}

/// A repayment authority read by its type: a standing grant or an exact approval.
pub enum Repayment { Standing(Box<RepaymentPermission>), Exact(Box<ExactRepaymentApproval>) }

impl Repayment {
    pub fn read(info: &AccountInfo) -> Result<Self> {
        require_keys_eq!(*info.owner, crate::ID, RegistryError::RepaymentPermission);
        require!(!info.executable, RegistryError::RepaymentPermission);
        let data = info.try_borrow_data()?;
        require!(data.len() >= 8, RegistryError::RepaymentPermission);
        if data[..8] == *RepaymentPermission::DISCRIMINATOR {
            require!(data.len() == 8 + RepaymentPermission::INIT_SPACE, RegistryError::RepaymentPermission);
            let p = RepaymentPermission::try_deserialize(&mut &data[..]).map_err(|_| error!(RegistryError::RepaymentPermission))?;
            let (key, bump) = Pubkey::find_program_address(
                &[b"repayment-permission", p.principal.as_ref(), &p.permission_id], &crate::ID);
            require!(info.key() == key && p.bump == bump && p.version == SCHEMA_VERSION, RegistryError::RepaymentPermission);
            return Ok(Self::Standing(Box::new(p)));
        }
        require!(data.len() == 8 + ExactRepaymentApproval::INIT_SPACE, RegistryError::RepaymentPermission);
        let a = ExactRepaymentApproval::try_deserialize(&mut &data[..]).map_err(|_| error!(RegistryError::RepaymentPermission))?;
        let (key, bump) = crate::spending::approval_address(&a.principal, a.number);
        require!(info.key() == key && a.bump == bump && a.version == SCHEMA_VERSION, RegistryError::RepaymentPermission);
        Ok(Self::Exact(Box::new(a)))
    }
    pub fn principal(&self) -> Pubkey { match self { Self::Standing(p) => p.principal, Self::Exact(a) => a.principal } }
    pub fn active(&self) -> bool { match self { Self::Standing(p) => p.active, Self::Exact(a) => a.active } }
    pub fn terms(&self) -> &RepaymentPermissionTerms { match self { Self::Standing(p) => &p.terms, Self::Exact(a) => &a.terms } }
    /// The number a payment under this authority must name, when exact.
    pub fn exact(&self) -> Option<(&ExactRequest, u64)> {
        match self { Self::Standing(_) => None, Self::Exact(a) => Some((&a.exact, a.number)) }
    }
    pub fn consume(&mut self, execution: &RepaymentExecution, now: i64, apply: bool) -> Result<()> {
        match self {
            Self::Standing(p) => {
                let consumed = p.terms.consume(p.consumed, execution, now)
                    .map_err(|_| error!(RegistryError::RepaymentPermission))?;
                if apply {
                    crate::admit_order(&mut p.window, execution.number)?;
                    p.consumed = consumed;
                } else {
                    require!(p.window.state(execution.number) == WindowState::Open, RegistryError::OrderUsed);
                }
            }
            Self::Exact(a) => {
                require!(execution.number == a.number, RegistryError::RepaymentPermission);
                let consumed = a.terms.consume_exact(a.consumed, execution, now, &a.exact)
                    .map_err(|_| error!(RegistryError::RepaymentPermission))?;
                if apply { a.consumed = consumed; }
            }
        }
        Ok(())
    }
    pub fn write(&self, info: &AccountInfo) -> Result<()> {
        let mut data = info.try_borrow_mut_data()?;
        match self {
            Self::Standing(p) => p.try_serialize(&mut &mut data[..]),
            Self::Exact(a) => a.try_serialize(&mut &mut data[..]),
        }
    }
}

fn terms_digest(principal: Pubkey, id: [u8; 32], owner: Pubkey,
    terms: &RepaymentPermissionTerms) -> Result<[u8; 32]> {
    let mut bytes = Vec::with_capacity(RepaymentPermissionTerms::INIT_SPACE);
    terms.serialize(&mut bytes)?;
    Ok(hashv(&[b"DEXTER:REPAYMENT-PERMISSION:1", crate::ID.as_ref(), principal.as_ref(),
        &id, owner.as_ref(), &bytes]).to_bytes())
}

fn exact_digest(principal: Pubkey, number: u64, owner: Pubkey,
    terms: &RepaymentPermissionTerms, exact: &ExactRequest) -> Result<[u8; 32]> {
    let mut bytes = Vec::with_capacity(RepaymentPermissionTerms::INIT_SPACE + ExactRequest::INIT_SPACE);
    terms.serialize(&mut bytes)?; exact.serialize(&mut bytes)?;
    Ok(hashv(&[b"DEXTER:EXACT-REPAYMENT-APPROVAL:1", crate::ID.as_ref(), principal.as_ref(),
        &number.to_le_bytes(), owner.as_ref(), &bytes]).to_bytes())
}

/// Checks shared by standing grants and exact approvals.
fn check_repayment_grant(principal: &Account<Principal>, binding: &FundingWalletBinding, binding_key: Pubkey,
    owner: &AccountInfo, remaining: &[AccountInfo], terms: &RepaymentPermissionTerms) -> Result<()> {
    require!(principal.version == SCHEMA_VERSION && binding.version == SCHEMA_VERSION,
        RegistryError::SchemaVersion);
    require_keys_eq!(principal.owner, owner.key(), RegistryError::WalletOwner);
    require!(terms.authority_generation == principal.authority_generation, RegistryError::WalletGeneration);
    crate::require_owner_authority(principal, owner, remaining)?;
    require!(terms.binding == binding_key && terms.expires_at > Clock::get()?.unix_timestamp,
        RegistryError::RepaymentPermission);
    let source = Pubkey::find_program_address(
        &[binding.wallet.as_ref(), TOKEN_ID.as_ref(), terms.mint.as_ref()], &ATA_ID).0;
    require_keys_eq!(terms.debit_from, source, RegistryError::RepaymentPermission);
    require_keys_eq!(terms.creditor_custody, custody_reserve_address(&terms.creditor_destination),
        RegistryError::RepaymentPermission);
    Ok(())
}

pub fn exact_repayment_grant(ctx: Context<GrantExactRepayment>, args: GrantExactRepaymentArgs) -> Result<()> {
    require!(get_stack_height() == TRANSACTION_LEVEL_STACK_HEIGHT, RegistryError::WalletEnvelope);
    let grant_owner = ctx.accounts.owner.key();
    let principal_key = ctx.accounts.principal.key();
    let digest = exact_digest(principal_key, args.number, grant_owner, &args.terms, &args.exact)?;
    let approval = &ctx.accounts.approval;
    if approval.version != 0 {
        require!(approval.version == SCHEMA_VERSION && approval.bump == ctx.bumps.approval
            && approval.principal == principal_key && approval.number == args.number
            && approval.grant_owner == grant_owner && approval.terms_digest == digest,
            RegistryError::RepaymentPermission);
        return Ok(()); // Never restore active state, counters, timing or old owner authority.
    }
    require!(Some(args.number) == ctx.accounts.principal.approvals.checked_add(1), RegistryError::ApprovalNumber);
    check_repayment_grant(&ctx.accounts.principal, &ctx.accounts.binding, ctx.accounts.binding.key(),
        &ctx.accounts.owner.to_account_info(), ctx.remaining_accounts, &args.terms)?;
    args.terms.validate_exact(&args.exact).map_err(|_| error!(RegistryError::RepaymentPermission))?;
    ctx.accounts.principal.approvals = args.number;
    let payer = ctx.accounts.payer.key();
    ctx.accounts.approval.set_inner(ExactRepaymentApproval { deposit_payer: payer, version: SCHEMA_VERSION,
        bump: ctx.bumps.approval, principal: principal_key, number: args.number, grant_owner, terms_digest: digest,
        terms: args.terms, exact: args.exact, active: true, consumed: RepaymentConsumption::default() });
    Ok(())
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
    check_repayment_grant(principal, binding, binding.key(), &ctx.accounts.owner.to_account_info(),
        ctx.remaining_accounts, &args.terms)?;
    args.terms.validate().map_err(|_| error!(RegistryError::RepaymentPermission))?;
    let payer = ctx.accounts.payer.key();
    permission.set_inner(RepaymentPermission { deposit_payer: payer, version: SCHEMA_VERSION, bump: ctx.bumps.permission,
        principal: principal.key(), permission_id: args.permission_id, grant_owner, terms_digest: digest,
        terms: args.terms, active: true, consumed: RepaymentConsumption::default(), window: OrderWindow::default() });
    Ok(())
}

pub fn revoke(ctx: Context<RevokeRepaymentPermission>, args: RevokeRepaymentPermissionArgs) -> Result<()> {
    require!(get_stack_height() == TRANSACTION_LEVEL_STACK_HEIGHT, RegistryError::WalletEnvelope);
    require!(ctx.accounts.principal.version == SCHEMA_VERSION, RegistryError::SchemaVersion);
    let info = ctx.accounts.permission.to_account_info();
    let mut repayment = Repayment::read(&info)?;
    require_keys_eq!(repayment.principal(), ctx.accounts.principal.key(), RegistryError::RepaymentPermission);
    require_keys_eq!(ctx.accounts.owner.key(), ctx.accounts.principal.owner, RegistryError::WalletOwner);
    require!(args.authority_generation == ctx.accounts.principal.authority_generation,
        RegistryError::WalletGeneration);
    crate::require_owner_authority(&ctx.accounts.principal, &ctx.accounts.owner.to_account_info(), ctx.remaining_accounts)?;
    match &mut repayment { Repayment::Standing(p) => p.active = false, Repayment::Exact(a) => a.active = false }
    repayment.write(&info)
}

pub fn consume(ctx: Context<ConsumeRepaymentPermission>, args: RepaymentExecution) -> Result<()> {
    require!(get_stack_height() > TRANSACTION_LEVEL_STACK_HEIGHT, RegistryError::WalletEnvelope);
    let info = ctx.accounts.permission.to_account_info();
    let mut repayment = Repayment::read(&info)?;
    require!(repayment.principal() == ctx.accounts.principal.key() && ctx.accounts.principal.version == SCHEMA_VERSION
        && ctx.accounts.binding.version == SCHEMA_VERSION && repayment.active(),
        RegistryError::RepaymentPermission);
    require!(args.binding == ctx.accounts.binding.key() && repayment.terms().binding == args.binding,
        RegistryError::WalletBinding);
    require!(args.authority_generation == ctx.accounts.principal.authority_generation
        && repayment.terms().authority_generation == args.authority_generation,
        RegistryError::WalletGeneration);
    repayment.consume(&args, Clock::get()?.unix_timestamp, true)?;
    repayment.write(&info)
}

#[derive(AnchorDeserialize)]
struct RunArgs {
    request_id: [u8; 32], amount: u64, maximum_interest: u64,
    _expected_steps: u32, repayment_owner_generation: u64,
    permission_number: u64, _service_without_pool: bool,
}
#[derive(AnchorDeserialize)]
struct FacilityArgs {
    request_id: [u8; 32], amount: u64, maximum_interest: u64,
    _expected_facility_revision: u64, repayment_owner_generation: u64,
    permission_number: u64, _service_without_pool: bool,
}

/// Bounded request summary extracted from the exact next Swig instruction. Live
/// creditor identity and monetary split are independently enforced by Core.
struct Envelope {
    principal: Pubkey, obligation: Pubkey,
    mint: Pubkey, from: Pubkey, creditor_custody: Pubkey, destination: Pubkey,
    protocol_destination: Option<Pubkey>,
    digest: [u8; 32], amount: u64, maximum_interest: u64,
    owner_generation: u64, permission: Pubkey, number: u64,
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
    // Change 5: a governed run repayment keeps its 26 roles (ClaimsCore's event
    // authority and program sit where the two Registry receipts were, the
    // repayment record where Core's receipt was); a governed facility repayment
    // has one more role (ClaimsCore's program after the system program).
    let fixed = if action == REPAYMENT_RUN { 26 } else { 21 };
    let frame = CompactFrame::parse(&ix.data, fixed..=fixed + 16)?;
    require_keys_eq!(meta(frame.program_index())?.pubkey, CORE_ID, RegistryError::FundingAction);
    let inner = frame.inner_data()?;
    let key = |i: usize| frame.role(&ix.accounts, i);
    require!(key(0)?.pubkey == accounts.caller.key() && key(0)?.is_signer && key(0)?.is_writable
        && key(1)?.pubkey == accounts.wallet.key() && key(2)?.pubkey == accounts.principal.key(),
        RegistryError::FundingAction);
    frame.require_covered_outer_indices(&ix.accounts)?;
    let opt = |i: usize| -> Result<Option<Pubkey>> {
        let value = key(i)?.pubkey; Ok(if value == CORE_ID { None } else { Some(value) })
    };
    let (from, binding, custody, permission, registry_authority, registry_program, token_program, system_program) =
        if action == REPAYMENT_RUN { (14,23,24,25,8,9,21,22) } else { (6,18,19,20,16,17,13,14) };
    let event_authority = Pubkey::find_program_address(&[b"__event_authority"], &CORE_ID).0;
    let (event_index, core_index) = if action == REPAYMENT_RUN { (6, 7) } else { (11, 15) };
    require!(key(event_index)?.pubkey == event_authority && key(core_index)?.pubkey == CORE_ID,
        RegistryError::FundingAction);
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
            protocol_custody: opt(16)?, protocol_destination: opt(17)?,
            permission_number: args.permission_number };
        let _ = &args.request_id;
        Ok(Envelope { principal: request.principal, obligation: request.run,
            mint: request.mint, from: request.from, creditor_custody: request.creditor_custody,
            destination: request.destination, protocol_destination: request.protocol_destination,
            digest: run_repayment_digest(&request)?, amount: request.amount,
            maximum_interest: request.maximum_interest, owner_generation: args.repayment_owner_generation,
            permission: key(permission)?.pubkey, number: args.permission_number })
    } else {
        require!(inner[..8] == hashv(&[b"global:repay_governed_funding_facility"]).to_bytes()[..8], RegistryError::FundingAction);
        let args = FacilityArgs::try_from_slice(&inner[8..]).map_err(|_| error!(RegistryError::FundingAction))?;
        let request = FacilityRepaymentRequest { issuer: key(3)?.pubkey, facility: key(4)?.pubkey,
            repayment_principal: key(2)?.pubkey, repayment_authority: key(1)?.pubkey,
            repayment_owner_generation: args.repayment_owner_generation, from: key(6)?.pubkey,
            financier_custody: key(5)?.pubkey, destination: key(7)?.pubkey, mint: key(10)?.pubkey,
            amount: args.amount, maximum_interest: args.maximum_interest,
            protocol_custody: opt(8)?, protocol_destination: opt(9)?,
            permission_number: args.permission_number };
        let _ = &args.request_id;
        Ok(Envelope { principal: request.issuer, obligation: request.facility,
            mint: request.mint, from: request.from, creditor_custody: request.financier_custody,
            destination: request.destination, protocol_destination: request.protocol_destination,
            digest: facility_repayment_digest(&request)?, amount: request.amount,
            maximum_interest: request.maximum_interest, owner_generation: args.repayment_owner_generation,
            permission: key(permission)?.pubkey, number: args.permission_number })
    }
}

#[cfg(test)]
mod framing_tests {
    use super::*;
    use crate::swig_framing::test_support::{control_metas, framed_data, funding_action, with_authorization_accounts};

    #[test]
    fn caller_count_bounds_precede_core_target_and_core_target_precedes_body() {
        with_authorization_accounts(|accounts| {
            for (action, min, max) in [(REPAYMENT_RUN, 26, 42), (REPAYMENT_FACILITY, 21, 37)] {
                for count in [min - 1, min, max, max + 1] {
                    let mut ix = Instruction { program_id: SWIG_ID, accounts: control_metas(accounts),
                        data: framed_data(&vec![1; count], &[0; 7]) };
                    ix.accounts[3].pubkey = Pubkey::new_unique();
                    let error = funding_action(inspect_instruction(&ix, accounts, action));
                    // At admitted counts, require_keys_eq on the bad Core target
                    // must win over the independently short body. Outside the
                    // range, framing must fail before that target comparison.
                    assert_eq!(error.compared_values.is_some(), (min..=max).contains(&count), "action {action}, count {count}");
                }
            }
        });
    }
}

/// Change 5: no historical branch. A payment that already ran is refused: its
/// number is used (standing grant) or its approval consumed (exact approval).
#[inline(never)]
pub(crate) fn authorize_envelope(ix: &Instruction, accounts: &AuthorizeFundingWallet,
    args: &AuthorizeFundingWalletArgs, remaining: &[AccountInfo]) -> Result<bool> {
    require!(remaining.len() == 1, RegistryError::FundingAction);
    let intent = inspect_instruction(ix, accounts, args.action)?;
    require!(remaining[0].key() == intent.permission, RegistryError::FundingAction);
    let permission = Repayment::read(&remaining[0])?;
    let terms = permission.terms();
    require!(permission.active() && permission.principal() == accounts.principal.key()
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
    match (&permission, permission.exact()) {
        (_, Some((exact, number))) => require!(exact.request_digest == intent.digest
            && exact.amount == intent.amount && number == intent.number, RegistryError::RepaymentPermission),
        (Repayment::Standing(p), None) => require!(p.window.state(intent.number) == WindowState::Open,
            RegistryError::OrderUsed),
        _ => return err!(RegistryError::RepaymentPermission),
    }
    // Actual accrued principal/interest and cumulative consumption are checked
    // by the Core-only consume CPI in the same atomic transaction.
    Ok(false)
}
