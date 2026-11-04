//! Delivery-linked receivable servicing, current title and separate facility debt.
//! Ordinary owner cash and explicitly permitted qualified Swig custody are
//! distinct debit profiles. A generic PDA signer is never sufficient.
use anchor_lang::prelude::*;
use crate::CoreError;
use anchor_spl::{associated_token, token::{self, Mint, Token, TokenAccount}};
use dexter_contracts::{self as contracts, plus, minus, REGISTRY_CONSUMER_SEED, CORE_ID, POOL_ID};
use dexter_contracts::commercial::{InterestTerms, InterestState, split_repayment, within_domain_assignment_policy};
use dexter_contracts::funded::*;
use dexter_contracts::repayment::{RepaymentExecution, RunRepaymentRequest,
    FacilityRepaymentRequest, run_repayment_digest, facility_repayment_digest,
    REPAYMENT_RUN, REPAYMENT_FACILITY};
use principal_registry::{self, Principal, Budget, Reservation, program::PrincipalRegistry};
use crate::funding_state::*;
use crate::funding_journal::{journal, inc, dec};

#[derive(Clone, Copy)]
struct GovernedDebit<'a, 'info> {
    binding: &'a UncheckedAccount<'info>,
    custody: &'a UncheckedAccount<'info>,
    permission: &'a UncheckedAccount<'info>,
}

#[derive(Clone, Copy)]
struct FacilityGovernedDebit<'a, 'info> {
    debit: GovernedDebit<'a, 'info>,
    registry_authority: &'a UncheckedAccount<'info>,
    registry_program: &'a Program<'info, PrincipalRegistry>,
}

pub fn interest_accrual_operation_id(operation: &[u8; 32], run: &Pubkey) -> Result<[u8; 32]> {
    financial_digest(b"DEXTER:FUNDED-INTEREST-ACCRUAL:1", &(operation, run))
}
pub fn interest_release_operation_id(operation: &[u8; 32], run: &Pubkey) -> Result<[u8; 32]> {
    financial_digest(b"DEXTER:FUNDED-INTEREST-RELEASE:1", &(operation, run))
}

fn owner_cash(principal: &Account<Principal>, authority: Pubkey, generation: u64,
    from: &Account<TokenAccount>, mint: Pubkey) -> Result<()> {
    owner(principal, authority, generation)?;
    require!(authority.is_on_curve() && from.owner == authority && from.mint == mint,
        CoreError::FundingOwner);
    require_keys_eq!(from.key(), associated_token::get_associated_token_address(&authority, &mint),
        CoreError::FundingTokenRouting);
    Ok(())
}
fn token_data(info: &AccountInfo, mint: Pubkey) -> Result<TokenAccount> {
    require_keys_eq!(*info.owner, token::ID, CoreError::FundingTokenRouting);
    let value = TokenAccount::try_deserialize(&mut &info.try_borrow_data()?[..])?;
    require_keys_eq!(value.mint, mint, CoreError::FundingTokenRouting); Ok(value)
}
fn custody_data(info: &AccountInfo) -> Result<FinancierCustody> {
    require!(info.is_writable && *info.owner == CORE_ID, CoreError::FundingCustody);
    let value = FinancierCustody::try_deserialize(&mut &info.try_borrow_data()?[..])?;
    value.check_key(info.key())?;
    require!(value.version == FUNDED_VERSION && value.revision > 0, CoreError::FundingSchema); Ok(value)
}

/// The debit ledger is the unique aggregate for this actual token account.
/// Its accounted free cash, not token surplus or another facility's backing,
/// funds the payment. Recipient aliases are rejected before any money moves;
/// coincident economic Principals may still use distinct custody accounts.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn prepare_debit<'info>(principal: &Account<'info, Principal>, authority: &Signer<'info>,
    generation: u64, from: &Account<'info, TokenAccount>, mint: Pubkey,
    binding: Option<&UncheckedAccount<'info>>, custody: Option<&UncheckedAccount<'info>>,
    permission: Option<&UncheckedAccount<'info>>, creditor_custody: Pubkey,
    protocol_custody: Option<Pubkey>, amount: u64) -> Result<Option<Box<FinancierCustody>>> {
    if binding.is_none() && custody.is_none() && permission.is_none() {
        owner_cash(principal, authority.key(), generation, from, mint)?;
        return Ok(None);
    }
    let binding = binding.ok_or_else(|| error!(CoreError::FundingOwner))?;
    let custody_info = custody.ok_or_else(|| error!(CoreError::FundingCustody))?;
    let permission = permission.ok_or_else(|| error!(CoreError::FundingOwner))?;
    valid_principal(principal)?;
    require!(generation == principal.authority_generation && permission.is_writable
        && !authority.key().is_on_curve(), CoreError::FundingOwner);
    let enrolled = principal_registry::read_funding_wallet_binding(&binding.to_account_info())?;
    let mut source = Box::new(custody_data(&custody_info.to_account_info())?);
    require!(source.profile == WALLET_CASH && source.principal == principal.key()
        && source.domain == principal.domain && source.binding == binding.key()
        && source.wallet == authority.key() && source.token_account == from.key()
        && source.mint == mint && from.owner == source.wallet && from.mint == mint
        && enrolled.principal == principal.key() && enrolled.wallet == source.wallet
        && from.key() == associated_token::get_associated_token_address(&source.wallet, &mint),
        CoreError::FundingCustody);
    require!(custody_info.key() != creditor_custody
        && protocol_custody != Some(custody_info.key()), CoreError::FundingTokenRouting);
    admit_cash(from)?;
    source.check(from.amount)?;
    source.accounted_cash = minus(source.accounted_cash, amount)?;
    source.revision = plus(source.revision, 1)?;
    // This is the same floor used by funding admission and free withdrawal.
    require!(source.accounted_cash >= source.protected()?, CoreError::FundingCashShortfall);
    Ok(Some(source))
}

#[inline(never)]
fn finish_debit(custody: Option<Box<FinancierCustody>>, info: Option<&UncheckedAccount>,
    from: &mut Account<TokenAccount>) -> Result<()> {
    if let Some(custody) = custody {
        from.reload()?;
        custody.check(from.amount)?;
        let info = info.ok_or_else(|| error!(CoreError::FundingCustody))?;
        custody.try_serialize(&mut &mut info.try_borrow_mut_data()?[..])?;
    }
    Ok(())
}

#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn consume_permission<'info>(principal: &Account<'info, Principal>,
    binding: Option<&UncheckedAccount<'info>>, permission: Option<&UncheckedAccount<'info>>,
    authority: &UncheckedAccount<'info>, registry_program: &Program<'info, PrincipalRegistry>,
    execution: RepaymentExecution) -> Result<()> {
    if let Some(permission) = permission {
        let binding = binding.ok_or_else(|| error!(CoreError::FundingOwner))?;
        let bump = [Pubkey::find_program_address(&[REGISTRY_CONSUMER_SEED], &CORE_ID).1];
        let seeds: &[&[u8]] = &[REGISTRY_CONSUMER_SEED, &bump];
        principal_registry::cpi::consume_repayment_permission(CpiContext::new_with_signer(registry_program.key(),
            principal_registry::cpi::accounts::ConsumeRepaymentPermission {
                core_authority: authority.to_account_info(), principal: principal.to_account_info(),
                binding: binding.to_account_info(), permission: permission.to_account_info(),
            }, &[seeds]), execution)?;
    }
    Ok(())
}

/// Optional untyped custody prevents two mutable Anchor account copies from
/// overwriting one another when protocol and creditor have the same principal.
struct ProtocolRoute { custody: Option<FinancierCustody>, key: Pubkey, available: bool }
fn protocol_route(terms: &InterestTerms, mint: Pubkey, custody: Option<&UncheckedAccount>,
    destination: Option<&UncheckedAccount>) -> Result<ProtocolRoute> {
    math(terms.validate())?;
    if terms.protocol_share_bps == 0 {
        require!(custody.is_none() && destination.is_none(), CoreError::FundingTokenRouting);
        return Ok(ProtocolRoute { custody: None, key: Pubkey::default(), available: false });
    }
    let info = custody.ok_or_else(|| error!(CoreError::FundingTokenRouting))?;
    let destination = destination.ok_or_else(|| error!(CoreError::FundingTokenRouting))?;
    let c = custody_data(&info.to_account_info())?;
    require!(c.principal == terms.protocol_beneficiary && c.mint == mint
        && c.token_account == terms.protocol_destination && destination.key() == terms.protocol_destination
        && destination.key() == associated_token::get_associated_token_address(&c.wallet, &mint),
        CoreError::FundingTokenRouting);
    let available = if destination.data_is_empty() && *destination.owner == anchor_lang::system_program::ID {
        false
    } else {
        let account = token_data(&destination.to_account_info(), mint)?;
        require_keys_eq!(account.owner, c.wallet, CoreError::FundingTokenRouting);
        match account.state {
            token::spl_token::state::AccountState::Initialized => {
                if account.amount < c.accounted_cash { false } else { c.check(account.amount)?; true }
            },
            token::spl_token::state::AccountState::Frozen => false,
            _ => return err!(CoreError::FundingTokenRouting),
        }
    };
    Ok(ProtocolRoute { custody: Some(c), key: info.key(), available })
}
fn credit_protocol(route: &mut ProtocolRoute, custody_info: Option<&UncheckedAccount>,
    destination: Option<&UncheckedAccount>, primary: &mut Account<FinancierCustody>, amount: u64) -> Result<()> {
    if amount == 0 { return Ok(()); }
    require!(route.available, CoreError::FundingTokenRouting);
    if route.key == primary.key() { primary.credit(amount)?; return Ok(()); }
    let c = route.custody.as_mut().ok_or_else(|| error!(CoreError::FundingCustody))?;
    let info = custody_info.ok_or_else(|| error!(CoreError::FundingCustody))?;
    let dest = destination.ok_or_else(|| error!(CoreError::FundingTokenRouting))?;
    c.credit(amount)?; c.check(token_data(&dest.to_account_info(), c.mint)?.amount)?;
    c.try_serialize(&mut &mut info.try_borrow_mut_data()?[..])?; Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn registry_service<'info>(principal: &Account<'info, Principal>, budget: &Account<'info, Budget>,
    reservation: &Account<'info, Reservation>, authority: &UncheckedAccount<'info>,
    payer: &Signer<'info>, registry_program: &Program<'info, PrincipalRegistry>, system_program: &Program<'info, System>,
    operation: &UncheckedAccount<'info>, operation_id: [u8; 32], amount: u64, release: bool,
    expected_revision: u64, without_pool: bool) -> Result<()> {
    if amount == 0 { return Ok(()); }
    let bump = [Pubkey::find_program_address(&[REGISTRY_CONSUMER_SEED], &CORE_ID).1];
    let seeds: &[&[u8]] = &[REGISTRY_CONSUMER_SEED, &bump];
    let seed_set = [seeds];
    let context = CpiContext::new_with_signer(registry_program.key(),
        principal_registry::cpi::accounts::ServiceReservation {
            core_authority: authority.to_account_info(), payer: payer.to_account_info(),
            principal: principal.to_account_info(), budget: budget.to_account_info(),
            reservation: reservation.to_account_info(), operation: operation.to_account_info(),
            system_program: system_program.to_account_info(),
        }, &seed_set);
    let args = contracts::StageArgs { operation_id, amount, expected_revision, service_without_pool: without_pool };
    if release { principal_registry::cpi::release(context, args) } else { principal_registry::cpi::consume(context, args) }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct RepayFundedRunArgs {
    pub request_id: [u8; 32], pub amount: u64, pub maximum_interest: u64,
    pub expected_claim_revision: u64, pub expected_ownership_revision: u64,
    pub repayment_owner_generation: u64, pub service_without_pool: bool,
}
#[derive(Accounts)]
#[instruction(args: RepayFundedRunArgs)]
pub struct RepayFundedRun<'info> {
    #[account(mut)] pub payer: Signer<'info>,
    pub repayment_authority: Signer<'info>,
    pub repayment_principal: Box<Account<'info, Principal>>,
    pub principal: Box<Account<'info, Principal>>,
    #[account(mut)] pub budget: Box<Account<'info, Budget>>,
    #[account(mut)] pub reservation: Box<Account<'info, Reservation>>,
    /// CHECK: Registry authenticates the canonical operation address in CPI.
    #[account(mut)] pub registry_accrual_operation: UncheckedAccount<'info>,
    /// CHECK: Separate Registry operation domain for unused charge release.
    #[account(mut)] pub registry_release_operation: UncheckedAccount<'info>,
    /// CHECK: Core's fixed Registry consumer PDA signs only authenticated servicing.
    #[account(seeds = [REGISTRY_CONSUMER_SEED], bump)] pub registry_authority: UncheckedAccount<'info>,
    pub registry_program: Program<'info, PrincipalRegistry>,
    #[account(mut)] pub line: Box<Account<'info, CustomerCreditLine>>,
    #[account(mut)] pub claim: Box<Account<'info, FundedClaim>>,
    #[account(mut)] pub run: Box<Account<'info, FundedRun>>,
    #[account(mut)] pub creditor_custody: Box<Account<'info, FinancierCustody>>,
    #[account(mut, token::mint = mint, token::authority = repayment_authority)] pub from: Box<Account<'info, TokenAccount>>,
    #[account(mut, token::mint = mint)] pub destination: Box<Account<'info, TokenAccount>>,
    /// CHECK: Authenticated optional Core custody, alias-aware updates below.
    #[account(mut, dup)] pub protocol_custody: Option<UncheckedAccount<'info>>,
    /// CHECK: Exact accepted canonical recipient; missing/frozen can waive take.
    #[account(mut, dup)] pub protocol_destination: Option<UncheckedAccount<'info>>,
    pub mint: Box<Account<'info, Mint>>,
    #[account(init_if_needed, payer = payer, space = 8 + FinanceReceipt::INIT_SPACE,
        seeds = [b"finance-operation", principal.key().as_ref(),
            &crate::funding::operation_id(b"repay-run",repayment_authority.key(), &args.request_id)], bump)]
    pub receipt: Box<Account<'info, FinanceReceipt>>,
    /// CHECK: Accepted servicing can omit the Pool CPI when projection is unavailable.
    #[account(address = POOL_ID)] pub pool_program: UncheckedAccount<'info>,
    pub token_program: Program<'info, Token>, pub system_program: Program<'info, System>,
}

/// A distinct entrypoint preserves the ordinary-cash signed instruction layout
/// and durable recovery profile while making governed authority explicit.
#[derive(Accounts)]
pub struct RepayGovernedFundedRun<'info> {
    pub repayment: RepayFundedRun<'info>,
    /// CHECK: Strict Registry enrollment reader authenticates this binding.
    pub repayment_binding: UncheckedAccount<'info>,
    /// CHECK: Canonical aggregate source reserve; distinct recipient ledgers.
    #[account(mut, dup)] pub repayment_custody: UncheckedAccount<'info>,
    /// CHECK: Registry authenticates and atomically consumes the owner grant.
    #[account(mut)] pub repayment_permission: UncheckedAccount<'info>,
}

#[inline(never)]
fn run_digest(a: &RepayFundedRun, args: &RepayFundedRunArgs) -> Result<[u8; 32]> {
    run_repayment_digest(&RunRepaymentRequest {
        principal: a.principal.key(), claim: a.claim.key(), run: a.run.key(),
        repayment_principal: a.repayment_principal.key(), repayment_authority: a.repayment_authority.key(),
        repayment_owner_generation: args.repayment_owner_generation, from: a.from.key(),
        creditor_custody: a.creditor_custody.key(), destination: a.destination.key(), mint: a.mint.key(),
        amount: args.amount, maximum_interest: args.maximum_interest,
        expected_ownership_revision: args.expected_ownership_revision,
        protocol_custody: a.protocol_custody.as_ref().map(|x| x.key()),
        protocol_destination: a.protocol_destination.as_ref().map(|x| x.key()),
    })
}

#[inline(never)]
fn consume_run_permission<'info>(a: &RepayFundedRun<'info>, args: &RepayFundedRunArgs,
    request_digest: [u8; 32], principal_paid: u64, interest_paid: u64,
    governed: Option<&GovernedDebit<'_, 'info>>) -> Result<()> {
    let Some(governed) = governed else { return Ok(()); };
    let binding = governed.binding;
    consume_permission(&a.repayment_principal, Some(binding), Some(governed.permission),
        &a.registry_authority, &a.registry_program, RepaymentExecution {
            binding: binding.key(), kind: REPAYMENT_RUN, obligation: a.run.key(),
            obligor: a.principal.key(), mint: a.mint.key(), debit_from: a.from.key(),
            creditor: a.run.current_creditor, creditor_custody: a.creditor_custody.key(),
            creditor_destination: a.destination.key(),
            protocol_beneficiary: if a.run.interest_terms.protocol_share_bps > 0 {
                a.run.interest_terms.protocol_beneficiary } else { Pubkey::default() },
            protocol_destination: if a.run.interest_terms.protocol_share_bps > 0 {
                a.run.interest_terms.protocol_destination } else { Pubkey::default() },
            executor: a.payer.key(), authority_generation: args.repayment_owner_generation,
            receipt: a.receipt.key(), request_digest, amount: args.amount, principal_paid, interest_paid,
        })
}

pub fn repay_run<'info>(ctx: Context<'info, RepayFundedRun<'info>>, args: RepayFundedRunArgs) -> Result<()> {
    repay_run_impl(ctx.accounts, ctx.remaining_accounts, args, None)
}

pub fn repay_governed_run<'info>(ctx: Context<'info, RepayGovernedFundedRun<'info>>,
    args: RepayFundedRunArgs) -> Result<()> {
    let a = ctx.accounts;
    let governed = GovernedDebit { binding: &a.repayment_binding,
        custody: &a.repayment_custody, permission: &a.repayment_permission };
    repay_run_impl(&mut a.repayment, ctx.remaining_accounts, args, Some(governed))
}

#[inline(never)]
fn repay_run_impl<'info>(a: &mut RepayFundedRun<'info>, remaining: &[AccountInfo<'info>],
    args: RepayFundedRunArgs, governed: Option<GovernedDebit<'_, 'info>>) -> Result<()> {
    let operation_id = crate::funding::operation_id(b"repay-run",a.repayment_authority.key(), &args.request_id);
    let digest = run_digest(a, &args)?;
    if replay(&a.receipt, &digest)? { return Ok(()); }
    a.claim.check(a.claim.key())?; a.run.check(a.run.key(), a.claim.key(), &a.claim)?; a.line.check(a.line.key())?;
    valid_principal(&a.principal)?;
    require!(a.principal.key() == a.claim.terms.principal && a.principal.domain == a.claim.terms.domain
        && a.claim.credit_line == a.line.key() && a.line.company == a.principal.key()
        && a.line.issuer == a.claim.issuer && a.line.facility == a.claim.facility
        && a.line.issuer_custody == a.claim.issuer_custody
        && a.line.mint == a.mint.key() && a.claim.terms.asset == a.mint.key(), CoreError::FundingCreditLine);
    require!(a.claim.revision == args.expected_claim_revision && a.run.ownership_revision == args.expected_ownership_revision,
        CoreError::FundingRevision);
    reservation_matches(&a.reservation, &a.claim)?;
    let debit = prepare_debit(&a.repayment_principal, &a.repayment_authority,
        args.repayment_owner_generation, &a.from, a.mint.key(), governed.as_ref().map(|g| g.binding),
        governed.as_ref().map(|g| g.custody), governed.as_ref().map(|g| g.permission), a.creditor_custody.key(),
        a.protocol_custody.as_ref().map(|x| x.key()), args.amount)?;
    custody_token(&a.creditor_custody, &a.destination, a.mint.key())?;
    require!(a.creditor_custody.key() == a.run.creditor_custody
        && a.creditor_custody.principal == a.run.current_creditor, CoreError::FundingCreditor);
    let mut route = protocol_route(&a.run.interest_terms, a.mint.key(), a.protocol_custody.as_ref(), a.protocol_destination.as_ref())?;
    let (accrued, new_interest) = math(a.run.interest.accrue(a.run.principal.outstanding, Clock::get()?.unix_timestamp, &a.run.interest_terms))?;
    let split = math(split_repayment(args.amount, a.run.principal.outstanding, math(accrued.outstanding())?,
        a.run.interest_terms.repayment_priority))?;
    require!(split.interest <= args.maximum_interest, CoreError::FundingChargeCap);
    consume_run_permission(a, &args, digest, split.principal, split.interest, governed.as_ref())?;
    let (paid_interest, allocation) = if split.interest == 0 {
        (accrued, dexter_contracts::commercial::InterestAllocation::default())
    } else { math(accrued.collect(split.interest, &a.run.interest_terms, route.available))? };
    let principal_next = if split.principal == 0 { a.run.principal } else { math(a.run.principal.repay(split.principal))? };
    let unused_fee = if principal_next.outstanding == 0 && math(paid_interest.outstanding())? == 0 {
        minus(minus(a.run.interest_terms.maximum_interest, paid_interest.accrued)?, a.run.fee_authority_released)?
    } else { 0 };
    require!(plus(new_interest, unused_fee)? <= a.claim.fee_authority_remaining, CoreError::FundingChargeCap);
    registry_service(&a.principal, &a.budget, &a.reservation, &a.registry_authority, &a.payer,
        &a.registry_program, &a.system_program, &a.registry_accrual_operation,
        interest_accrual_operation_id(&operation_id, &a.run.key())?, new_interest, false,
        args.expected_claim_revision, args.service_without_pool)?;
    registry_service(&a.principal, &a.budget, &a.reservation, &a.registry_authority, &a.payer,
        &a.registry_program, &a.system_program, &a.registry_release_operation,
        interest_release_operation_id(&operation_id, &a.run.key())?, unused_fee, true,
        args.expected_claim_revision, args.service_without_pool)?;
    let credited = plus(split.principal, allocation.lender)?;
    transfer_checked_exact(&a.from.to_account_info(), &a.destination.to_account_info(), &a.repayment_authority.to_account_info(),
        &a.mint, a.token_program.key(), credited, &[])?;
    if allocation.protocol > 0 {
        let dest = a.protocol_destination.as_ref().ok_or_else(||error!(CoreError::FundingTokenRouting))?;
        transfer_checked_exact(&a.from.to_account_info(), &dest.to_account_info(), &a.repayment_authority.to_account_info(),
            &a.mint, a.token_program.key(), allocation.protocol, &[])?;
    }
    if credited > 0 { a.creditor_custody.credit(credited)?; }
    credit_protocol(&mut route, a.protocol_custody.as_ref(), a.protocol_destination.as_ref(), &mut a.creditor_custody, allocation.protocol)?;
    a.destination.reload()?; a.creditor_custody.check(a.destination.amount)?;
    finish_debit(debit, governed.as_ref().map(|g| g.custody), &mut a.from)?;
    a.run.principal = principal_next; a.run.interest = paid_interest;
    a.run.fee_authority_released = plus(a.run.fee_authority_released, unused_fee)?;
    a.claim.debt = minus(a.claim.debt, split.principal)?; a.claim.repaid = plus(a.claim.repaid, split.principal)?;
    a.claim.interest_outstanding = minus(plus(a.claim.interest_outstanding, new_interest)?, split.interest)?;
    a.claim.fee_authority_remaining = minus(a.claim.fee_authority_remaining, plus(new_interest, unused_fee)?)?;
    a.claim.released_fee_caps = plus(a.claim.released_fee_caps, unused_fee)?;
    a.claim.revision = plus(a.claim.revision, 1)?;
    a.line.principal_outstanding = minus(a.line.principal_outstanding, split.principal)?;
    a.line.fee_exposure = minus(a.line.fee_exposure, plus(split.interest, unused_fee)?)?;
    a.line.revision = plus(a.line.revision, 1)?;
    let company = a.principal.key(); let creditor = a.run.current_creditor; let payer = a.repayment_principal.key();
    let mut changes = vec![dec(company,CUSTOMER_PRINCIPAL,split.principal), dec(creditor,OWNED_PRINCIPAL,split.principal),
        inc(company,INTEREST_OWED,new_interest), inc(creditor,INTEREST_OWNED,new_interest),
        dec(company,INTEREST_OWED,split.interest), dec(creditor,INTEREST_OWNED,split.interest),
        dec(company,CREDIT_CHARGE_EXPOSURE,plus(split.interest,unused_fee)?),
        inc(payer,CASH_SENT,args.amount), inc(creditor,CASH_RECEIVED,credited), inc(creditor,LENDER_INTEREST,allocation.lender)];
    if a.run.interest_terms.protocol_share_bps > 0 { changes.extend([
        inc(a.run.interest_terms.protocol_beneficiary,CASH_RECEIVED,allocation.protocol),
        inc(a.run.interest_terms.protocol_beneficiary,PROTOCOL_INTEREST,allocation.protocol)]); }
    let mut record = record_base(company,operation_id,digest,FINANCE_REPAY,a.mint.key(),a.claim.key(),a.run.key(),args.amount);
    record.principal_paid=split.principal; record.interest_paid=split.interest;
    record.protocol_interest=allocation.protocol; record.protocol_waived=allocation.protocol_waived;
    record.debit_principal=payer; record.debit_authority=a.repayment_authority.key(); record.debit_from=a.from.key();
    record.current_creditor=creditor; record.ownership_revision=a.run.ownership_revision; record.creditor_destination=a.destination.key();
    journal(&mut a.receipt,record,&changes,remaining,&a.pool_program.to_account_info(),args.service_without_pool,false)?;
    a.claim.check(a.claim.key())?; a.run.check(a.run.key(),a.claim.key(),&a.claim)?; a.line.check(a.line.key())?;
    Ok(())
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct AssignFundedRunArgs {
    pub request_id: [u8; 32], pub consideration: u64, pub acquisition_costs: u64,
    pub acquisition_cost_evidence_hash: [u8; 32], pub expected_claim_revision: u64,
    pub expected_ownership_revision: u64, pub seller_owner_generation: u64, pub buyer_owner_generation: u64,
}
#[derive(Accounts)]
#[instruction(args: AssignFundedRunArgs)]
pub struct AssignFundedRun<'info> {
    #[account(mut)] pub payer: Signer<'info>,
    pub seller_owner: Signer<'info>, pub buyer_owner: Signer<'info>,
    pub principal: Box<Account<'info, Principal>>,
    pub seller: Box<Account<'info, Principal>>, pub buyer: Box<Account<'info, Principal>>,
    #[account(mut)] pub budget: Box<Account<'info, Budget>>,
    #[account(mut)] pub reservation: Box<Account<'info, Reservation>>,
    /// CHECK: Registry validates the independently derived accrual operation.
    #[account(mut)] pub registry_accrual_operation: UncheckedAccount<'info>,
    /// CHECK: Fixed Core Registry-consumer PDA.
    #[account(seeds = [REGISTRY_CONSUMER_SEED], bump)] pub registry_authority: UncheckedAccount<'info>,
    pub registry_program: Program<'info, PrincipalRegistry>,
    #[account(mut)] pub claim: Box<Account<'info, FundedClaim>>,
    #[account(mut)] pub run: Box<Account<'info, FundedRun>>,
    #[account(mut)] pub seller_custody: Box<Account<'info, FinancierCustody>>,
    pub buyer_custody: Box<Account<'info, FinancierCustody>>,
    #[account(mut, token::mint = mint, token::authority = buyer_owner)] pub purchase_from: Box<Account<'info, TokenAccount>>,
    #[account(mut, token::mint = mint)] pub seller_destination: Box<Account<'info, TokenAccount>>,
    #[account(token::mint = mint)] pub buyer_destination: Box<Account<'info, TokenAccount>>,
    pub cost_principal: Option<Box<Account<'info, Principal>>>,
    /// CHECK: Bound cost beneficiary custody, with alias-aware credit.
    #[account(mut, dup)] pub cost_custody: Option<UncheckedAccount<'info>>,
    /// CHECK: Exact corresponding token account; cost requires an actual transfer.
    #[account(mut, dup)] pub cost_destination: Option<UncheckedAccount<'info>>,
    pub mint: Box<Account<'info, Mint>>,
    #[account(init_if_needed, payer = payer, space = 8 + FinanceReceipt::INIT_SPACE,
        seeds = [b"finance-operation", principal.key().as_ref(),
            &crate::funding::operation_id(b"assign-run",seller_owner.key(), &args.request_id)], bump)]
    pub receipt: Box<Account<'info, FinanceReceipt>>,
    /// CHECK: Assignment is new risk and requires an executable caught-up Pool.
    #[account(address = POOL_ID)] pub pool_program: UncheckedAccount<'info>,
    pub token_program: Program<'info, Token>, pub system_program: Program<'info, System>,
}

pub fn assign_run<'info>(ctx: Context<'info, AssignFundedRun<'info>>, args: AssignFundedRunArgs) -> Result<()> {
    let a = ctx.accounts;
    let operation_id = crate::funding::operation_id(b"assign-run",a.seller_owner.key(), &args.request_id);
    let digest = financial_digest(b"DEXTER:FUNDED-RUN-ASSIGN:1", &(
        (a.principal.key(),a.claim.key(),a.run.key(),a.seller.key(),a.buyer.key()),
        (a.seller_owner.key(),a.buyer_owner.key(),args.seller_owner_generation,args.buyer_owner_generation),
        (a.seller_custody.key(),a.buyer_custody.key(),a.purchase_from.key(),a.seller_destination.key(),a.buyer_destination.key()),
        (a.mint.key(),args.consideration,args.acquisition_costs,args.acquisition_cost_evidence_hash,args.expected_ownership_revision),
        (a.cost_principal.as_ref().map(|x|x.key()),a.cost_custody.as_ref().map(|x|x.key()),a.cost_destination.as_ref().map(|x|x.key()))))?;
    if replay(&a.receipt,&digest)? { return Ok(()); }
    a.claim.check(a.claim.key())?; a.run.check(a.run.key(),a.claim.key(),&a.claim)?;
    valid_principal(&a.principal)?; owner(&a.seller,a.seller_owner.key(),args.seller_owner_generation)?;
    owner_cash(&a.buyer,a.buyer_owner.key(),args.buyer_owner_generation,&a.purchase_from,a.mint.key())?;
    require!(a.seller_owner.key().is_on_curve() && a.principal.key()==a.claim.terms.principal
        && a.principal.domain==a.claim.terms.domain && a.seller.domain==a.principal.domain
        && a.buyer.domain==a.principal.domain && a.buyer.key()!=a.seller.key()
        && a.buyer.key()!=a.principal.key(),CoreError::FundingAssignment);
    require!(a.claim.assignment_policy_hash==within_domain_assignment_policy(&a.principal.domain),CoreError::FundingAssignment);
    require!(a.claim.revision==args.expected_claim_revision && a.run.ownership_revision==args.expected_ownership_revision,
        CoreError::FundingRevision);
    require!(a.run.current_creditor==a.seller.key() && a.run.creditor_custody==a.seller_custody.key(),CoreError::FundingCreditor);
    custody_token(&a.seller_custody,&a.seller_destination,a.mint.key())?;
    custody_token(&a.buyer_custody,&a.buyer_destination,a.mint.key())?;
    require!(a.seller_custody.principal==a.seller.key() && a.buyer_custody.principal==a.buyer.key()
        && a.run.mint==a.mint.key(),CoreError::FundingTokenRouting);
    reservation_matches(&a.reservation,&a.claim)?;
    require!(args.consideration>0,CoreError::FundingAmount);
    let acquisition_basis=plus(args.consideration,args.acquisition_costs)?;
    let mut cost_route=ProtocolRoute{custody:None,key:Pubkey::default(),available:false};
    let mut cost_beneficiary=Pubkey::default(); let mut cost_destination=Pubkey::default();
    if args.acquisition_costs==0 {
        require!(a.cost_principal.is_none() && a.cost_custody.is_none() && a.cost_destination.is_none()
            && args.acquisition_cost_evidence_hash==[0;32],CoreError::FundingAssignment);
    } else {
        let p=a.cost_principal.as_ref().ok_or_else(||error!(CoreError::FundingAssignment))?;
        let c_info=a.cost_custody.as_ref().ok_or_else(||error!(CoreError::FundingAssignment))?;
        let destination=a.cost_destination.as_ref().ok_or_else(||error!(CoreError::FundingAssignment))?;
        valid_principal(p)?; let c=custody_data(&c_info.to_account_info())?;
        require!(p.key()!=a.buyer.key() && c.principal==p.key() && c.mint==a.mint.key()
            && c.token_account==destination.key() && args.acquisition_cost_evidence_hash!=[0;32]
            && destination.key()==associated_token::get_associated_token_address(&c.wallet,&a.mint.key()),CoreError::FundingAssignment);
        let token=token_data(&destination.to_account_info(),a.mint.key())?;
        require!(token.owner==c.wallet && token.state==token::spl_token::state::AccountState::Initialized,
            CoreError::FundingTokenRouting); c.check(token.amount)?;
        cost_beneficiary=p.key(); cost_destination=destination.key();
        cost_route=ProtocolRoute{custody:Some(c),key:c_info.key(),available:true};
    }
    let (interest,new_interest)=math(a.run.interest.accrue(a.run.principal.outstanding,Clock::get()?.unix_timestamp,&a.run.interest_terms))?;
    let interest_due=math(interest.outstanding())?;
    require!(a.run.principal.outstanding>0 || interest_due>0,CoreError::FundingAssignment);
    require!(new_interest<=a.claim.fee_authority_remaining,CoreError::FundingChargeCap);
    registry_service(&a.principal,&a.budget,&a.reservation,&a.registry_authority,&a.payer,&a.registry_program,
        &a.system_program,&a.registry_accrual_operation,interest_accrual_operation_id(&operation_id,&a.run.key())?,
        new_interest,false,args.expected_claim_revision,false)?;
    transfer_checked_exact(&a.purchase_from.to_account_info(),&a.seller_destination.to_account_info(),&a.buyer_owner.to_account_info(),
        &a.mint,a.token_program.key(),args.consideration,&[])?;
    a.seller_custody.credit(args.consideration)?;
    if args.acquisition_costs>0 {
        let destination=a.cost_destination.as_ref().ok_or_else(||error!(CoreError::FundingAssignment))?;
        transfer_checked_exact(&a.purchase_from.to_account_info(),&destination.to_account_info(),&a.buyer_owner.to_account_info(),
            &a.mint,a.token_program.key(),args.acquisition_costs,&[])?;
        credit_protocol(&mut cost_route,a.cost_custody.as_ref(),a.cost_destination.as_ref(),&mut a.seller_custody,args.acquisition_costs)?;
    }
    a.seller_destination.reload()?; a.seller_custody.check(a.seller_destination.amount)?;
    let old_creditor=a.run.current_creditor; let old_basis=a.run.acquisition_basis;
    let new_revision=plus(a.run.ownership_revision,1)?;
    // The whole residual includes the lender's uncollected interest entitlement.
    // The protocol allocation and all borrower/supplier terms stay frozen.
    let provenance=financial_digest(b"DEXTER:FUNDED-TITLE-TRANSFER:1", &(
        a.run.provenance_hash,digest,old_creditor,a.buyer.key(),new_revision,a.run.principal.outstanding,
        interest_due,old_basis,acquisition_basis,a.run.interest_terms.clone()))?;
    a.run.current_creditor=a.buyer.key(); a.run.creditor_custody=a.buyer_custody.key();
    a.run.ownership_revision=new_revision; a.run.acquisition_basis=acquisition_basis;
    a.run.provenance_hash=provenance; a.run.interest=interest;
    a.claim.interest_outstanding=plus(a.claim.interest_outstanding,new_interest)?;
    a.claim.fee_authority_remaining=minus(a.claim.fee_authority_remaining,new_interest)?;
    a.claim.revision=plus(a.claim.revision,1)?;
    let company=a.principal.key(); let buyer=a.buyer.key();
    let mut changes=vec![inc(company,INTEREST_OWED,new_interest),
        inc(old_creditor,INTEREST_OWNED,new_interest),
        dec(old_creditor,OWNED_PRINCIPAL,a.run.principal.outstanding),inc(buyer,OWNED_PRINCIPAL,a.run.principal.outstanding),
        dec(old_creditor,INTEREST_OWNED,interest_due),inc(buyer,INTEREST_OWNED,interest_due),
        inc(buyer,CASH_SENT,acquisition_basis),inc(old_creditor,CASH_RECEIVED,args.consideration)];
    if args.acquisition_costs>0 { changes.push(inc(cost_beneficiary,CASH_RECEIVED,args.acquisition_costs)); }
    let mut record=record_base(company,operation_id,digest,FINANCE_ASSIGN,a.mint.key(),a.claim.key(),a.run.key(),a.run.principal.outstanding);
    record.debit_principal=buyer;record.debit_authority=a.buyer_owner.key();record.debit_from=a.purchase_from.key();
    record.previous_creditor=old_creditor;record.current_creditor=buyer;record.ownership_revision=new_revision;
    record.creditor_destination=a.buyer_destination.key();record.assignment_consideration=args.consideration;
    record.assignment_acquisition_cost=args.acquisition_costs;record.previous_acquisition_basis=old_basis;
    record.new_acquisition_basis=acquisition_basis;record.assignment_cost_beneficiary=cost_beneficiary;
    record.assignment_cost_destination=cost_destination;
    journal(&mut a.receipt,record,&changes,ctx.remaining_accounts,&a.pool_program.to_account_info(),false,true)?;
    a.claim.check(a.claim.key())?;a.run.check(a.run.key(),a.claim.key(),&a.claim)?;
    Ok(())
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct RepayFundingFacilityArgs {
    pub request_id: [u8; 32], pub amount: u64, pub maximum_interest: u64,
    pub expected_facility_revision: u64, pub repayment_owner_generation: u64,
    pub service_without_pool: bool,
}
#[derive(Accounts)]
#[instruction(args: RepayFundingFacilityArgs)]
pub struct RepayFundingFacility<'info> {
    #[account(mut)] pub payer: Signer<'info>,
    pub repayment_authority: Signer<'info>,
    pub repayment_principal: Box<Account<'info, Principal>>,
    pub issuer: Box<Account<'info, Principal>>,
    #[account(mut)] pub facility: Box<Account<'info, FundingFacility>>,
    #[account(mut)] pub financier_custody: Box<Account<'info, FinancierCustody>>,
    #[account(mut, token::mint = mint, token::authority = repayment_authority)] pub from: Box<Account<'info, TokenAccount>>,
    #[account(mut, token::mint = mint)] pub destination: Box<Account<'info, TokenAccount>>,
    /// CHECK: Exact accepted protocol custody, explicitly handles creditor alias.
    #[account(mut, dup)] pub protocol_custody: Option<UncheckedAccount<'info>>,
    /// CHECK: Exact canonical recipient; absence/freeze waives only protocol take.
    #[account(mut, dup)] pub protocol_destination: Option<UncheckedAccount<'info>>,
    pub mint: Box<Account<'info, Mint>>,
    #[account(init_if_needed, payer = payer, space = 8 + FinanceReceipt::INIT_SPACE,
        seeds = [b"finance-operation", issuer.key().as_ref(),
            &crate::funding::operation_id(b"repay-facility",repayment_authority.key(), &args.request_id)], bump)]
    pub receipt: Box<Account<'info, FinanceReceipt>>,
    /// CHECK: Financial servicing can proceed with only authoritative Core books.
    #[account(address = POOL_ID)] pub pool_program: UncheckedAccount<'info>,
    pub token_program: Program<'info, Token>, pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct RepayGovernedFundingFacility<'info> {
    pub repayment: RepayFundingFacility<'info>,
    /// CHECK: Core's fixed Registry consumer signs permission consumption only.
    #[account(seeds = [REGISTRY_CONSUMER_SEED], bump)] pub registry_authority: UncheckedAccount<'info>,
    pub registry_program: Program<'info, PrincipalRegistry>,
    /// CHECK: Strict Registry enrollment reader authenticates this binding.
    pub repayment_binding: UncheckedAccount<'info>,
    /// CHECK: Canonical aggregate source reserve; distinct recipient ledgers.
    #[account(mut, dup)] pub repayment_custody: UncheckedAccount<'info>,
    /// CHECK: Registry authenticates and atomically consumes this owner grant.
    #[account(mut)] pub repayment_permission: UncheckedAccount<'info>,
}

#[inline(never)]
fn facility_digest(a: &RepayFundingFacility, args: &RepayFundingFacilityArgs) -> Result<[u8; 32]> {
    facility_repayment_digest(&FacilityRepaymentRequest {
        issuer: a.issuer.key(), facility: a.facility.key(), repayment_principal: a.repayment_principal.key(),
        repayment_authority: a.repayment_authority.key(), repayment_owner_generation: args.repayment_owner_generation,
        from: a.from.key(), financier_custody: a.financier_custody.key(), destination: a.destination.key(),
        mint: a.mint.key(), amount: args.amount, maximum_interest: args.maximum_interest,
        protocol_custody: a.protocol_custody.as_ref().map(|x| x.key()),
        protocol_destination: a.protocol_destination.as_ref().map(|x| x.key()),
    })
}

#[inline(never)]
fn consume_facility_permission<'info>(a: &RepayFundingFacility<'info>, args: &RepayFundingFacilityArgs,
    request_digest: [u8; 32], principal_paid: u64, interest_paid: u64,
    governed: Option<&FacilityGovernedDebit<'_, 'info>>) -> Result<()> {
    let Some(governed) = governed else { return Ok(()); };
    let binding = governed.debit.binding;
    consume_permission(&a.repayment_principal, Some(binding), Some(governed.debit.permission),
        governed.registry_authority, governed.registry_program, RepaymentExecution {
            binding: binding.key(), kind: REPAYMENT_FACILITY, obligation: a.facility.key(),
            obligor: a.issuer.key(), mint: a.mint.key(), debit_from: a.from.key(),
            creditor: a.facility.financier, creditor_custody: a.financier_custody.key(),
            creditor_destination: a.destination.key(),
            protocol_beneficiary: if a.facility.interest_terms.protocol_share_bps > 0 {
                a.facility.interest_terms.protocol_beneficiary } else { Pubkey::default() },
            protocol_destination: if a.facility.interest_terms.protocol_share_bps > 0 {
                a.facility.interest_terms.protocol_destination } else { Pubkey::default() },
            executor: a.payer.key(), authority_generation: args.repayment_owner_generation,
            receipt: a.receipt.key(), request_digest, amount: args.amount, principal_paid, interest_paid,
        })
}

pub fn repay_facility<'info>(ctx: Context<'info, RepayFundingFacility<'info>>,
    args: RepayFundingFacilityArgs) -> Result<()> {
    repay_facility_impl(ctx.accounts,ctx.remaining_accounts,args,None)
}

pub fn repay_governed_facility<'info>(ctx: Context<'info, RepayGovernedFundingFacility<'info>>,
    args: RepayFundingFacilityArgs) -> Result<()> {
    let a=ctx.accounts;
    let governed=FacilityGovernedDebit {
        debit:GovernedDebit{binding:&a.repayment_binding,custody:&a.repayment_custody,
            permission:&a.repayment_permission},
        registry_authority:&a.registry_authority,registry_program:&a.registry_program,
    };
    repay_facility_impl(&mut a.repayment,ctx.remaining_accounts,args,Some(governed))
}

#[inline(never)]
fn repay_facility_impl<'info>(a: &mut RepayFundingFacility<'info>, remaining: &[AccountInfo<'info>],
    args: RepayFundingFacilityArgs, governed: Option<FacilityGovernedDebit<'_, 'info>>) -> Result<()> {
    let operation_id=crate::funding::operation_id(b"repay-facility",a.repayment_authority.key(),&args.request_id);
    let digest=facility_digest(a,&args)?;
    if replay(&a.receipt,&digest)? { return Ok(()); }
    valid_principal(&a.issuer)?; a.facility.check(a.facility.key())?;
    require!(a.facility.external && a.facility.issuer==a.issuer.key() && a.facility.mint==a.mint.key()
        && a.facility.custody==a.financier_custody.key() && a.facility.financier==a.financier_custody.principal,
        CoreError::FundingFacility);
    require!(a.issuer.domain==a.financier_custody.domain,CoreError::FundingFacility);
    require!(a.facility.revision==args.expected_facility_revision,CoreError::FundingRevision);
    let debit=prepare_debit(&a.repayment_principal,&a.repayment_authority,
        args.repayment_owner_generation,&a.from,a.mint.key(),governed.as_ref().map(|g|g.debit.binding),
        governed.as_ref().map(|g|g.debit.custody),governed.as_ref().map(|g|g.debit.permission),a.financier_custody.key(),
        a.protocol_custody.as_ref().map(|x|x.key()),args.amount)?;
    custody_token(&a.financier_custody,&a.destination,a.mint.key())?;
    let mut route=protocol_route(&a.facility.interest_terms,a.mint.key(),a.protocol_custody.as_ref(),a.protocol_destination.as_ref())?;
    let (interest,new_interest)=math(a.facility.interest.accrue(a.facility.drawn,Clock::get()?.unix_timestamp,&a.facility.interest_terms))?;
    let split=math(split_repayment(args.amount,a.facility.drawn,math(interest.outstanding())?,a.facility.interest_terms.repayment_priority))?;
    require!(split.interest<=args.maximum_interest,CoreError::FundingChargeCap);
    consume_facility_permission(a,&args,digest,split.principal,split.interest,governed.as_ref())?;
    let (next_interest,allocation)=if split.interest==0 {
        (interest,dexter_contracts::commercial::InterestAllocation::default())
    } else { math(interest.collect(split.interest,&a.facility.interest_terms,route.available))? };
    let credited=plus(split.principal,allocation.lender)?;
    transfer_checked_exact(&a.from.to_account_info(),&a.destination.to_account_info(),&a.repayment_authority.to_account_info(),
        &a.mint,a.token_program.key(),credited,&[])?;
    if allocation.protocol>0 {
        let destination=a.protocol_destination.as_ref().ok_or_else(||error!(CoreError::FundingTokenRouting))?;
        transfer_checked_exact(&a.from.to_account_info(),&destination.to_account_info(),&a.repayment_authority.to_account_info(),
            &a.mint,a.token_program.key(),allocation.protocol,&[])?;
    }
    if credited>0 { a.financier_custody.credit(credited)?; }
    credit_protocol(&mut route,a.protocol_custody.as_ref(),a.protocol_destination.as_ref(),&mut a.financier_custody,allocation.protocol)?;
    a.destination.reload()?; a.financier_custody.check(a.destination.amount)?;
    finish_debit(debit,governed.as_ref().map(|g|g.debit.custody),&mut a.from)?;
    a.facility.drawn=minus(a.facility.drawn,split.principal)?;a.facility.repaid=plus(a.facility.repaid,split.principal)?;
    a.facility.interest=next_interest;a.facility.revision=plus(a.facility.revision,1)?;
    let issuer=a.issuer.key();let financier=a.facility.financier;let cash_payer=a.repayment_principal.key();
    let mut changes=vec![dec(issuer,FACILITY_DEBT,split.principal),dec(financier,FACILITY_ASSETS,split.principal),
        inc(issuer,INTEREST_OWED,new_interest),inc(financier,INTEREST_OWNED,new_interest),
        dec(issuer,INTEREST_OWED,split.interest),dec(financier,INTEREST_OWNED,split.interest),
        inc(cash_payer,CASH_SENT,args.amount),inc(financier,CASH_RECEIVED,credited),inc(financier,LENDER_INTEREST,allocation.lender)];
    if a.facility.interest_terms.protocol_share_bps>0 { changes.extend([
        inc(a.facility.interest_terms.protocol_beneficiary,CASH_RECEIVED,allocation.protocol),
        inc(a.facility.interest_terms.protocol_beneficiary,PROTOCOL_INTEREST,allocation.protocol)]); }
    let mut record=record_base(issuer,operation_id,digest,FINANCE_FACILITY_REPAY,a.mint.key(),a.facility.key(),Pubkey::default(),args.amount);
    record.principal_paid=split.principal;record.interest_paid=split.interest;
    record.protocol_interest=allocation.protocol;record.protocol_waived=allocation.protocol_waived;
    record.debit_principal=cash_payer;record.debit_authority=a.repayment_authority.key();record.debit_from=a.from.key();
    record.current_creditor=financier;record.creditor_destination=a.destination.key();
    journal(&mut a.receipt,record,&changes,remaining,&a.pool_program.to_account_info(),args.service_without_pool,false)?;
    a.facility.check(a.facility.key())?; Ok(())
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct CreditSellerFeeArgs {
    pub request_id: [u8; 32], pub amount: u64, pub owner_generation: u64,
    pub expected_claim_revision: u64, pub service_without_pool: bool,
}
#[derive(Accounts)]
#[instruction(args: CreditSellerFeeArgs)]
pub struct CreditSellerFee<'info> {
    #[account(mut)] pub payer: Signer<'info>,
    pub beneficiary_owner: Signer<'info>,
    pub beneficiary: Box<Account<'info, Principal>>,
    #[account(mut)] pub claim: Box<Account<'info, FundedClaim>>,
    pub original_receipt: Box<Account<'info, FinanceReceipt>>,
    #[account(init_if_needed,payer=payer,space=8+SellerFeeCredits::INIT_SPACE,
        seeds=[b"seller-fee-credits",original_receipt.key().as_ref()],bump)]
    pub credits: Box<Account<'info, SellerFeeCredits>>,
    #[account(mut,token::mint=mint,token::authority=beneficiary_owner)] pub from: Box<Account<'info, TokenAccount>>,
    #[account(mut,token::mint=mint)] pub destination: Box<Account<'info, TokenAccount>>,
    pub mint: Box<Account<'info, Mint>>,
    #[account(init_if_needed,payer=payer,space=8+FinanceReceipt::INIT_SPACE,
        seeds=[b"finance-operation",beneficiary.key().as_ref(),
            &crate::funding::operation_id(b"fee-credit",beneficiary_owner.key(),&args.request_id)],bump)]
    pub receipt: Box<Account<'info, FinanceReceipt>>,
    /// CHECK: Fee return does not create new risk and can service without Pool.
    #[account(address=POOL_ID)] pub pool_program: UncheckedAccount<'info>,
    pub token_program: Program<'info, Token>,pub system_program: Program<'info, System>,
}

pub fn credit_fee<'info>(ctx: Context<'info, CreditSellerFee<'info>>,args:CreditSellerFeeArgs)->Result<()> {
    let a=ctx.accounts;
    let operation_id=crate::funding::operation_id(b"fee-credit",a.beneficiary_owner.key(),&args.request_id);
    let digest=financial_digest(b"DEXTER:SELLER-FEE-CREDIT:1", &(
        a.beneficiary.key(),a.beneficiary_owner.key(),args.owner_generation,a.claim.key(),a.original_receipt.key(),
        a.from.key(),a.destination.key(),a.mint.key(),args.amount))?;
    if replay(&a.receipt,&digest)? { return Ok(()); }
    a.claim.check(a.claim.key())?;
    require!(a.claim.revision==args.expected_claim_revision,CoreError::FundingRevision);
    require!(args.amount>0 && a.beneficiary.key()==a.claim.seller_fee_terms.beneficiary
        && a.mint.key()==a.claim.terms.asset,CoreError::FundingTerms);
    owner_cash(&a.beneficiary,a.beneficiary_owner.key(),args.owner_generation,&a.from,a.mint.key())?;
    let original=read_finance_receipt(&a.original_receipt.to_account_info())?;
    require!(original.kind==FINANCE_COLLECT && original.claim==a.claim.key()
        && original.principal==a.claim.terms.principal && original.mint==a.mint.key(),CoreError::FundingTerms);
    require!(a.destination.owner==a.claim.terms.supplier
        && a.destination.key()==associated_token::get_associated_token_address(&a.claim.terms.supplier,&a.mint.key()),
        CoreError::FundingTokenRouting);
    if a.credits.version!=0 {
        require!(a.credits.version==FUNDED_VERSION && a.credits.original_receipt==a.original_receipt.key()
            && a.credits.bump==ctx.bumps.credits,CoreError::FundingTerms);
    }
    let credited=plus(a.credits.credited,args.amount)?;
    require!(credited<=original.seller_fee,CoreError::FundingChargeCap);
    let state=math(a.claim.seller_fee_state.credit(args.amount,&a.claim.seller_fee_terms))?;
    transfer_checked_exact(&a.from.to_account_info(),&a.destination.to_account_info(),&a.beneficiary_owner.to_account_info(),
        &a.mint,a.token_program.key(),args.amount,&[])?;
    a.credits.set_inner(SellerFeeCredits{version:FUNDED_VERSION,bump:ctx.bumps.credits,original_receipt:a.original_receipt.key(),credited});
    a.claim.seller_fee_state=state;a.claim.revision=plus(a.claim.revision,1)?;
    let changes=[inc(a.beneficiary.key(),CASH_SENT,args.amount),inc(a.beneficiary.key(),SELLER_FEE_REFUNDS,args.amount),
        inc(a.claim.supplier_principal,CASH_RECEIVED,args.amount),inc(a.claim.supplier_principal,SELLER_FEE_CREDITS_RECEIVED,args.amount)];
    let mut record=record_base(a.beneficiary.key(),operation_id,digest,FINANCE_REFUND,a.mint.key(),a.claim.key(),original.run,args.amount);
    record.related_receipt=a.original_receipt.key();record.seller_fee_credit=args.amount;
    record.debit_principal=a.beneficiary.key();record.debit_authority=a.beneficiary_owner.key();record.debit_from=a.from.key();
    journal(&mut a.receipt,record,&changes,ctx.remaining_accounts,&a.pool_program.to_account_info(),args.service_without_pool,false)?;
    a.claim.check(a.claim.key())?;Ok(())
}
