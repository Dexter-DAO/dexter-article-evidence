//! Change 5: close at settlement. Permissionless instructions close a Tab's
//! records once nothing more can happen to them, each deposit returning to the
//! rent payer recorded when the record was created. Every close checks its
//! conditions against current state and, when one fails, succeeds with no
//! effect, so a client can append closes to any transaction without risk.
//!
//! A run closes only once its Tab can no longer earn (`remaining == 0`, which
//! never increases), so a closed run's address can never hold a run again. A
//! repayment record closes only after its run, so no payment can recreate it at
//! its address, and the Tab only after every run and record. While a settlement
//! hold is in force on the Tab, every close is a no-op.
use anchor_lang::prelude::*;
use anchor_lang::Discriminator;
use dexter_contracts::REGISTRY_CONSUMER_SEED;
use dexter_contracts::billing::BILLING_NAMESPACE;
use principal_registry::{self, program::PrincipalRegistry, Reservation};
use crate::{CoreError, EvidenceBill};
use crate::funding_state::*;
use crate::recoverable_billing::{BillAuthorityBinding, BILL_AUTHORITY_SEED};
use crate::receiving::ReceivingCustody;
use crate::seller_fees::SellerFeeAgreement;
use crate::wallet_trading_policy::WalletTradingPolicy;
use crate::wallet_stock_custody::StockControlApproval;
use crate::tab_events::{self, TabStepHeader, EventBytes, RepaymentRecordClosed, RunClosed, TabClosed, ApprovalClosed};

#[derive(Accounts)]
pub struct CloseRepaymentRecord<'info> {
    #[account(mut)] pub claim: Box<Account<'info, FundedClaim>>,
    /// CHECK: The record's run, which must already be closed.
    #[account(address = record.run @ CoreError::CloseMismatch)] pub run: UncheckedAccount<'info>,
    #[account(mut)] pub record: Box<Account<'info, RepaymentRecord>>,
    /// CHECK: The record's rent payer receives its deposit.
    #[account(mut, address = record.deposit_payer @ CoreError::DepositPayer)]
    pub record_payer: UncheckedAccount<'info>,
    /// CHECK: ClaimsCore's event authority.
    #[account(address = crate::EVENT_AUTHORITY_AND_BUMP.0)] pub core_event_authority: UncheckedAccount<'info>,
    /// CHECK: ClaimsCore itself.
    #[account(address = crate::ID)] pub core_program: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct CloseRun<'info> {
    #[account(mut)] pub claim: Box<Account<'info, FundedClaim>>,
    #[account(mut)] pub run: Box<Account<'info, FundedRun>>,
    /// CHECK: The run's rent payer receives its deposit.
    #[account(mut, address = run.deposit_payer @ CoreError::DepositPayer)]
    pub run_payer: UncheckedAccount<'info>,
    /// CHECK: ClaimsCore's event authority.
    #[account(address = crate::EVENT_AUTHORITY_AND_BUMP.0)] pub core_event_authority: UncheckedAccount<'info>,
    /// CHECK: ClaimsCore itself.
    #[account(address = crate::ID)] pub core_program: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct CloseTab<'info> {
    #[account(mut)] pub claim: Box<Account<'info, FundedClaim>>,
    /// CHECK: The Tab's rent payer receives its deposit.
    #[account(mut, address = claim.deposit_payer @ CoreError::DepositPayer)]
    pub tab_payer: UncheckedAccount<'info>,
    /// CHECK: The Tab's Registry budget hold, closed by the Registry.
    #[account(mut)] pub hold: UncheckedAccount<'info>,
    /// CHECK: The Tab's mandate; the Registry folds the closed Tab into its chain.
    #[account(mut, address = claim.terms.mandate @ CoreError::CloseMismatch)]
    pub mandate: UncheckedAccount<'info>,
    /// CHECK: The hold's rent payer, checked by the Registry.
    #[account(mut)] pub hold_payer: UncheckedAccount<'info>,
    /// CHECK: Core's Registry consumer signer.
    #[account(seeds = [REGISTRY_CONSUMER_SEED], bump)] pub registry_authority: UncheckedAccount<'info>,
    pub registry_program: Program<'info, PrincipalRegistry>,
    /// An evidence order's bill, closed with its Tab.
    #[account(mut)] pub bill: Option<Box<Account<'info, EvidenceBill>>>,
    /// CHECK: The bill's rent payer.
    #[account(mut)] pub bill_payer: Option<UncheckedAccount<'info>>,
    /// CHECK: A recoverable evidence order's authority binding, at its canonical
    /// address; empty for other evidence orders.
    #[account(mut)] pub binding: Option<UncheckedAccount<'info>>,
    /// CHECK: The binding's rent payer.
    #[account(mut)] pub binding_payer: Option<UncheckedAccount<'info>>,
    /// CHECK: ClaimsCore's event authority.
    #[account(address = crate::EVENT_AUTHORITY_AND_BUMP.0)] pub core_event_authority: UncheckedAccount<'info>,
    /// CHECK: ClaimsCore itself.
    #[account(address = crate::ID)] pub core_program: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct CloseApproval<'info> {
    /// CHECK: A Registry exact approval, read and closed by the Registry.
    #[account(mut)] pub approval: UncheckedAccount<'info>,
    /// CHECK: The approval's rent payer, checked by the Registry.
    #[account(mut)] pub deposit_payer: UncheckedAccount<'info>,
    /// CHECK: Core's Registry consumer signer.
    #[account(seeds = [REGISTRY_CONSUMER_SEED], bump)] pub registry_authority: UncheckedAccount<'info>,
    pub registry_program: Program<'info, PrincipalRegistry>,
    /// CHECK: ClaimsCore's event authority.
    #[account(address = crate::EVENT_AUTHORITY_AND_BUMP.0)] pub core_event_authority: UncheckedAccount<'info>,
    /// CHECK: ClaimsCore itself.
    #[account(address = crate::ID)] pub core_program: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct ReturnExcessRent<'info> {
    /// CHECK: A ClaimsCore record whose type stores its rent payer first.
    #[account(mut)] pub record: UncheckedAccount<'info>,
    /// CHECK: Must equal the rent payer stored in the record.
    #[account(mut)] pub deposit_payer: UncheckedAccount<'info>,
}

/// A settlement hold in force (design section 9) makes every close of its Tab a no-op.
fn held(claim: &FundedClaim) -> Result<bool> {
    Ok(claim.hold_in_force(Clock::get()?.unix_timestamp))
}

/// Move a closed account's lamports to `destination` and hand it back to the
/// System program with no data.
fn close_info<'info>(info: &AccountInfo<'info>, destination: &AccountInfo<'info>) -> Result<u64> {
    let lamports = info.lamports();
    **destination.try_borrow_mut_lamports()? = destination.lamports().checked_add(lamports)
        .ok_or_else(|| error!(CoreError::FundingSchema))?;
    **info.try_borrow_mut_lamports()? = 0;
    info.assign(&anchor_lang::system_program::ID);
    info.resize(0)?;
    Ok(lamports)
}

/// A run account closed by `close_run` (here or earlier in this transaction).
fn closed(info: &AccountInfo) -> bool {
    *info.owner != crate::ID && info.data_is_empty()
}

pub fn handle_close_repayment_record<'info>(ctx: Context<'info, CloseRepaymentRecord<'info>>) -> Result<()> {
    let a = ctx.accounts;
    let (claim, record) = (a.claim.key(), a.record.key());
    a.claim.check(claim)?; a.record.check(record)?;
    require!(a.record.claim == claim, CoreError::CloseMismatch);
    // Only after its run has closed: a closed run can never hold a run again,
    // so no payment can recreate this record at its address.
    if !closed(&a.run) || held(&a.claim)? { return Ok(()); }
    let header = TabStepHeader { tab: claim, step: a.claim.next_step()?,
        step_id: tab_events::step_id(&record.to_bytes(), &[0x52; 32]) };
    let lamports = a.record.to_account_info().lamports();
    a.record.close(a.record_payer.to_account_info())?;
    a.claim.open_records = a.claim.open_records.checked_sub(1).ok_or_else(|| error!(CoreError::FundingSchema))?;
    let event = EventBytes::new::<RepaymentRecordClosed>(TabStepHeader::INIT_SPACE + 72)
        .put(&header)?.put(&record)?.put(&a.record_payer.key())?.put(&lamports)?;
    tab_events::record_tab_step(&mut a.claim, &a.core_event_authority, event, false)
}

pub fn handle_close_run<'info>(ctx: Context<'info, CloseRun<'info>>) -> Result<()> {
    let a = ctx.accounts;
    let (claim, run) = (a.claim.key(), a.run.key());
    a.claim.check(claim)?; a.run.check(run, claim, &a.claim)?;
    if !(a.claim.remaining == 0 && a.run.settled()?) || held(&a.claim)? { return Ok(()); }
    let header = TabStepHeader { tab: claim, step: a.claim.next_step()?,
        step_id: tab_events::step_id(&run.to_bytes(), &[0x55; 32]) };
    let lamports = a.run.to_account_info().lamports();
    a.run.close(a.run_payer.to_account_info())?;
    a.claim.open_runs = a.claim.open_runs.checked_sub(1).ok_or_else(|| error!(CoreError::FundingSchema))?;
    let event = EventBytes::new::<RunClosed>(TabStepHeader::INIT_SPACE + 72)
        .put(&header)?.put(&run)?.put(&a.run_payer.key())?.put(&lamports)?;
    tab_events::record_tab_step(&mut a.claim, &a.core_event_authority, event, false)
}

/// The bill (and binding) of an evidence Tab, checked, or `None` for other Tabs.
/// `Ok(false)` when the Tab cannot close yet.
#[inline(never)]
fn evidence_records_closable(a: &CloseTab) -> Result<bool> {
    let claim = a.claim.key();
    if a.claim.usage_namespace != BILLING_NAMESPACE {
        require!(a.bill.is_none() && a.binding.is_none(), CoreError::CloseMismatch);
        return Ok(true);
    }
    let bill = a.bill.as_ref().ok_or_else(|| error!(CoreError::CloseMismatch))?;
    let bill_key = bill.key();
    bill.check(bill_key)?;
    require!(bill.financial_claim == claim, CoreError::CloseMismatch);
    let payer = a.bill_payer.as_ref().ok_or_else(|| error!(CoreError::CloseMismatch))?;
    require_keys_eq!(payer.key(), bill.deposit_payer, CoreError::DepositPayer);
    let binding = a.binding.as_ref().ok_or_else(|| error!(CoreError::CloseMismatch))?;
    let (expected, _) = Pubkey::find_program_address(&[BILL_AUTHORITY_SEED, bill_key.as_ref()], &crate::ID);
    require_keys_eq!(binding.key(), expected, CoreError::CloseMismatch);
    Ok(bill.finalized)
}

pub fn handle_close_tab<'info>(ctx: Context<'info, CloseTab<'info>>) -> Result<()> {
    let a = ctx.accounts;
    let claim = a.claim.key();
    a.claim.check(claim)?;
    let (hold_key, _) = principal_registry::hold_address(&a.claim.terms.mandate, a.claim.terms.order);
    require_keys_eq!(a.hold.key(), hold_key, CoreError::CloseMismatch);
    if !(a.claim.open_runs == 0 && a.claim.open_records == 0 && a.claim.settled()
        && evidence_records_closable(a)?) || held(&a.claim)? { return Ok(()); }
    let hold = {
        let data = a.hold.try_borrow_data()?;
        Reservation::try_deserialize(&mut &data[..]).map_err(|_| error!(CoreError::FundingReservation))?
    };
    require_keys_eq!(*a.hold.owner, principal_registry::ID, CoreError::FundingReservation);
    if hold.remaining != 0 { return Ok(()); }
    require_keys_eq!(a.hold_payer.key(), hold.deposit_payer, CoreError::DepositPayer);
    let steps = a.claim.steps;
    let prior = a.claim.fingerprint;
    let hold_lamports = a.hold.lamports();
    // The Registry closes the hold and folds (tab, steps, h_n) into the mandate's chain.
    let bump = [ctx.bumps.registry_authority];
    let seeds: &[&[u8]] = &[REGISTRY_CONSUMER_SEED, &bump];
    principal_registry::cpi::close_hold(CpiContext::new_with_signer(a.registry_program.key(),
        principal_registry::cpi::accounts::CloseHold { core_authority: a.registry_authority.to_account_info(),
            reservation: a.hold.to_account_info(), mandate: a.mandate.to_account_info(),
            deposit_payer: a.hold_payer.to_account_info() }, &[seeds]), claim, steps, prior)?;
    let (mut bill_key, mut bill_payer, mut bill_lamports) = (Pubkey::default(), Pubkey::default(), 0u64);
    let (mut binding_key, mut binding_payer, mut binding_lamports) = (Pubkey::default(), Pubkey::default(), 0u64);
    if let (Some(bill), Some(payer)) = (a.bill.as_ref(), a.bill_payer.as_ref()) {
        bill_key = bill.key(); bill_payer = payer.key();
        bill_lamports = bill.to_account_info().lamports();
        bill.close(payer.to_account_info())?;
        if let Some(binding) = a.binding.as_ref() {
            if *binding.owner == crate::ID && !binding.data_is_empty() {
                let stored = BillAuthorityBinding::try_deserialize(&mut &binding.try_borrow_data()?[..])?;
                let payer = a.binding_payer.as_ref().ok_or_else(|| error!(CoreError::CloseMismatch))?;
                require_keys_eq!(payer.key(), stored.deposit_payer, CoreError::DepositPayer);
                binding_key = binding.key(); binding_payer = payer.key();
                binding_lamports = close_info(&binding.to_account_info(), &payer.to_account_info())?;
            }
        }
    }
    let header = TabStepHeader { tab: claim, step: a.claim.next_step()?,
        step_id: tab_events::step_id(&claim.to_bytes(), &[0x54; 32]) };
    let tab_lamports = a.claim.to_account_info().lamports();
    let event = EventBytes::new::<TabClosed>(TabStepHeader::INIT_SPACE + 32 + 4 + 32 + 8 + 32 + 32 + 8
        + 32 + 32 + 8 + 32 + 32 + 8)
        .put(&header)?.put(&prior)?.put(&steps)?.put(&a.tab_payer.key())?.put(&tab_lamports)?
        .put(&hold_key)?.put(&hold.deposit_payer)?.put(&hold_lamports)?
        .put(&bill_key)?.put(&bill_payer)?.put(&bill_lamports)?
        .put(&binding_key)?.put(&binding_payer)?.put(&binding_lamports)?;
    tab_events::emit_event(&a.core_event_authority, event)?;
    a.claim.close(a.tab_payer.to_account_info())
}

/// The fields of a Registry exact approval this close reports.
struct ApprovalFacts { principal: Pubkey, number: u64, used: bool, payer: Pubkey, expires_at: i64 }

fn approval_facts(info: &AccountInfo) -> Result<ApprovalFacts> {
    require_keys_eq!(*info.owner, principal_registry::ID, CoreError::CloseMismatch);
    let data = info.try_borrow_data()?;
    if data.len() >= 8 && data[..8] == *principal_registry::ExactSpendingApproval::DISCRIMINATOR {
        let a = principal_registry::ExactSpendingApproval::try_deserialize(&mut &data[..])?;
        return Ok(ApprovalFacts { principal: a.principal, number: a.number, used: a.consumed.operations > 0,
            payer: a.deposit_payer, expires_at: a.terms.expires_at });
    }
    let a = principal_registry::ExactRepaymentApproval::try_deserialize(&mut &data[..])
        .map_err(|_| error!(CoreError::CloseMismatch))?;
    Ok(ApprovalFacts { principal: a.principal, number: a.number, used: a.consumed.payments > 0,
        payer: a.deposit_payer, expires_at: a.terms.expires_at })
}

pub fn handle_close_approval<'info>(ctx: Context<'info, CloseApproval<'info>>) -> Result<()> {
    let a = ctx.accounts;
    let facts = approval_facts(&a.approval)?;
    // Before its expiry an approval may still be used: closing it is a no-op,
    // so a client can append the close to any transaction.
    if Clock::get()?.unix_timestamp < facts.expires_at { return Ok(()); }
    let lamports = a.approval.lamports();
    let bump = [ctx.bumps.registry_authority];
    let seeds: &[&[u8]] = &[REGISTRY_CONSUMER_SEED, &bump];
    principal_registry::cpi::close_approval(CpiContext::new_with_signer(a.registry_program.key(),
        principal_registry::cpi::accounts::CloseApproval { core_authority: a.registry_authority.to_account_info(),
            approval: a.approval.to_account_info(), deposit_payer: a.deposit_payer.to_account_info() }, &[seeds]))?;
    let event = ApprovalClosed { approval: a.approval.key(), principal: facts.principal, number: facts.number,
        used: facts.used, payer: facts.payer, lamports };
    tab_events::emit_event(&a.core_event_authority, EventBytes::new::<ApprovalClosed>(128).put(&event)?)
}

/// ClaimsCore types that record their rent payer as the first field.
fn records_payer(discriminator: &[u8]) -> bool {
    [FundedClaim::DISCRIMINATOR, FundedRun::DISCRIMINATOR, RepaymentRecord::DISCRIMINATOR,
        EvidenceBill::DISCRIMINATOR, BillAuthorityBinding::DISCRIMINATOR, FinanceBook::DISCRIMINATOR,
        FinancierCustody::DISCRIMINATOR, FundingFacility::DISCRIMINATOR, ReceivingCustody::DISCRIMINATOR,
        FacilityAuthority::DISCRIMINATOR, CustomerCreditLine::DISCRIMINATOR, SellerFeeAgreement::DISCRIMINATOR,
        WalletTradingPolicy::DISCRIMINATOR, StockControlApproval::DISCRIMINATOR]
        .iter().any(|known| *known == discriminator)
}

pub fn handle_return_excess_rent<'info>(ctx: Context<'info, ReturnExcessRent<'info>>) -> Result<()> {
    let (record, payer) = (&ctx.accounts.record, &ctx.accounts.deposit_payer);
    require_keys_eq!(*record.owner, crate::ID, CoreError::NoExcessRent);
    require!(record.key() != payer.key(), CoreError::NoExcessRent);
    let stored = {
        let data = record.try_borrow_data()?;
        require!(data.len() >= 40 && records_payer(&data[..8]), CoreError::NoExcessRent);
        Pubkey::new_from_array(data[8..40].try_into().unwrap())
    };
    require_keys_eq!(stored, payer.key(), CoreError::DepositPayer);
    let minimum = Rent::get()?.minimum_balance(record.data_len());
    let excess = record.lamports().checked_sub(minimum).unwrap_or(0);
    require!(excess > 0, CoreError::NoExcessRent);
    **record.try_borrow_mut_lamports()? -= excess;
    **payer.try_borrow_mut_lamports()? = payer.lamports().checked_add(excess)
        .ok_or_else(|| error!(CoreError::NoExcessRent))?;
    Ok(())
}
