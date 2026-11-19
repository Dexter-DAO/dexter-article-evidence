//! Negotiated bill corrections preserve delivery and historical cash lineage.
//! OwnerCash funding is explicit and voluntary; it creates no hidden claim.
use anchor_lang::prelude::*;
use anchor_spl::{associated_token, token::{self, Mint, Token, TokenAccount}};
use solana_sha256_hasher::hashv;
use dexter_contracts::{plus, minus, CORE_ID, POOL_ID, REGISTRY_CONSUMER_SEED};
use dexter_contracts::corrections::{CorrectionAmounts, CorrectionPlan};
use dexter_contracts::financing::PrincipalAmounts;
use dexter_contracts::funded::*;
use principal_registry::{Principal, Budget, Reservation, program::PrincipalRegistry};
use crate::{CoreError, funding_state::*, funding_journal::{journal_numbered, inc, dec, Booked},
    historical_refunds::{validate_refund_destination, credit_refund_custody},
    receivables::{registry_service, interest_accrual_operation_id, interest_release_operation_id},
    tab_events::{self, TabStepHeader, RunCorrected, EventBytes, DETAIL_SPACE, books_space}};

pub const FINANCE_CORRECT: u8 = 22;

pub fn correction_operation_id(run: Pubkey, request_id: &[u8; 32]) -> [u8; 32] {
    crate::funding::operation_id(b"correct-run", run, request_id)
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct CorrectFinancedRunArgs {
    pub request_id: [u8; 32],
    pub reason_hash: [u8; 32],
    pub unpaid_reduction: u64,
    pub collected_return: u64,
    pub creditor_compensation: u64,
    pub interest_waiver: u64,
    pub maximum_new_interest: u64,
    /// The Tab's money-step count this correction was prepared against. Not part
    /// of the request digest.
    pub expected_steps: u32,
    pub company_owner_generation: u64,
    pub supplier_owner_generation: u64,
    pub creditor_owner_generation: u64,
    pub funding_owner_generation: u64,
    pub service_without_pool: bool,
    /// The correction's number, admitted once: with cash legs, by the funding
    /// party's book window (it pays them from owner cash); without, by the
    /// company's book window (its owner signs every correction). Part of the
    /// request digest.
    pub number: u64,
}

/// What a correction did, for its event: today's correction receipt without its
/// account links.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct CorrectionDetail {
    pub request: CorrectFinancedRunArgs,
    pub capture_digest: [u8; 32], pub acceptance_hash: [u8; 32],
    pub supplier_principal: Pubkey, pub frozen_supplier_wallet: Pubkey,
    pub company_owner: Pubkey, pub supplier_owner: Pubkey, pub creditor_owner: Pubkey,
    pub current_creditor: Pubkey, pub creditor_custody: Pubkey, pub creditor_destination: Pubkey,
    pub funding_principal: Pubkey, pub funding_authority: Pubkey, pub funding_from: Pubkey,
    pub refund_principal: Pubkey, pub refund_authority: Pubkey, pub refund_destination: Pubkey,
    pub plan: CorrectionPlan,
    pub principal_before: PrincipalAmounts, pub principal_after: PrincipalAmounts,
    pub corrections_before: CorrectionAmounts, pub corrections_after: CorrectionAmounts,
    pub newly_accrued_interest: u64, pub interest_waived: u64, pub unused_charge_authority_released: u64,
}

#[derive(Accounts)]
#[instruction(args: CorrectFinancedRunArgs)]
pub struct CorrectFinancedRun<'info> {
    #[account(mut)] pub payer: Signer<'info>,
    pub company_owner: Signer<'info>,
    pub supplier_owner: Option<Signer<'info>>,
    pub principal: Box<Account<'info, Principal>>,
    pub supplier_principal: Box<Account<'info, Principal>>,
    pub creditor_principal: Box<Account<'info, Principal>>,
    pub creditor_owner: Option<Signer<'info>>,
    pub funding_principal: Option<Box<Account<'info, Principal>>>,
    pub funding_authority: Option<Signer<'info>>,
    #[account(mut)] pub funding_from: Option<Box<Account<'info, TokenAccount>>>,
    #[account(mut)] pub budget: Box<Account<'info, Budget>>,
    #[account(mut)] pub reservation: Box<Account<'info, Reservation>>,
    /// CHECK: Canonical Core-to-Registry consumer signer.
    #[account(seeds=[REGISTRY_CONSUMER_SEED],bump)] pub registry_authority: UncheckedAccount<'info>,
    pub registry_program: Program<'info, PrincipalRegistry>,
    #[account(mut)] pub line: Box<Account<'info, CustomerCreditLine>>,
    #[account(mut)] pub claim: Box<Account<'info, FundedClaim>>,
    #[account(mut)] pub run: Box<Account<'info, FundedRun>>,
    #[account(mut)] pub custody: Box<Account<'info, FinancierCustody>>,
    pub custody_token: Box<Account<'info, TokenAccount>>,
    #[account(mut)] pub facility: Box<Account<'info, FundingFacility>>,
    /// CHECK: Alias-aware exact current-creditor custody, required only for cash.
    #[account(mut,dup)] pub creditor_custody: Option<UncheckedAccount<'info>>,
    /// CHECK: Authenticated exact creditor route; can alias custody_token.
    #[account(mut,dup)] pub creditor_destination: Option<UncheckedAccount<'info>>,
    /// The run's repayment record of the payer route a prior-repayment refund returns to.
    #[account(mut)] pub repayment_record: Option<Box<Account<'info, RepaymentRecord>>>,
    /// CHECK: Exact original payer source and owner from the repayment record.
    #[account(mut,dup)] pub refund_destination: Option<UncheckedAccount<'info>>,
    pub mint: Box<Account<'info, Mint>>,
    /// CHECK: Accepted servicing can preserve Core effects without Pool CPI.
    #[account(address=POOL_ID)] pub pool_program: UncheckedAccount<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    /// CHECK: Exact original governed payer custody, authenticated from its
    /// repayment record; may alias the typed source or unchecked creditor.
    #[account(mut,dup)] pub refund_custody: Option<UncheckedAccount<'info>>,
    /// CHECK: ClaimsCore's event authority.
    #[account(address=crate::EVENT_AUTHORITY_AND_BUMP.0)] pub core_event_authority: UncheckedAccount<'info>,
    /// CHECK: ClaimsCore itself.
    #[account(address=crate::ID)] pub core_program: UncheckedAccount<'info>,
}

fn read_token(info: &AccountInfo, mint: Pubkey) -> Result<TokenAccount> {
    require_keys_eq!(*info.owner,token::ID,CoreError::FundingTokenRouting);
    let value=TokenAccount::try_deserialize(&mut &info.try_borrow_data()?[..])?;
    require_keys_eq!(value.mint,mint,CoreError::FundingTokenRouting); Ok(value)
}

/// A prior-repayment refund draws on one repayment record of this run and goes
/// back to that record's payer route.
#[inline(never)]
fn record_principal_refund(a: &mut CorrectFinancedRun, amount: u64) -> Result<(Pubkey,Pubkey)> {
    if amount==0 {
        require!(a.repayment_record.is_none()
            && a.refund_destination.is_none() && a.refund_custody.is_none(),CoreError::FundingCorrectionLineage);
        return Ok((Pubkey::default(),Pubkey::default()));
    }
    let run=a.run.key();
    let record=a.repayment_record.as_deref_mut().ok_or_else(||error!(CoreError::FundingCorrectionLineage))?;
    let record_key=record.key();
    record.check(record_key)?;
    require_keys_eq!(record.run,run,CoreError::FundingCorrectionLineage);
    let dest=a.refund_destination.as_ref().ok_or_else(||error!(CoreError::FundingTokenRouting))?;
    validate_refund_destination(record,&dest.to_account_info(),a.mint.key(),a.refund_custody.as_ref())?;
    record.usage=math(record.usage.record(amount,0,0,record.principal_paid,record.interest_paid,record.protocol_interest_paid))?;
    Ok((record.payer_principal,record.payer_authority))
}

#[inline(never)]
fn request_digest(a: &CorrectFinancedRun, args: &CorrectFinancedRunArgs) -> Result<[u8;32]> {
    // The staleness guard and the projection choice are not part of the request.
    let mut economics=args.clone(); economics.service_without_pool=false; economics.expected_steps=0;
    // refund_custody is the canonical PDA of the original receipt's debit_from,
    // not an independently selectable economic route. Authenticate it on new
    // execution without changing ordinary-cash request or recovery identities.
    bounded_request_hash(&(
        &economics,
        (a.principal.key(),a.company_owner.key(),a.supplier_principal.key(),a.supplier_owner.as_ref().map(|x|x.key()),
            a.creditor_principal.key(),a.creditor_owner.as_ref().map(|x|x.key())),
        (a.claim.key(),a.run.key(),a.line.key(),a.custody.key(),a.custody_token.key(),a.facility.key(),a.mint.key()),
        (a.funding_principal.as_ref().map(|x|x.key()),a.funding_authority.as_ref().map(|x|x.key()),a.funding_from.as_ref().map(|x|x.key()),
            a.creditor_custody.as_ref().map(|x|x.key()),a.creditor_destination.as_ref().map(|x|x.key()),
            a.repayment_record.as_ref().map(|x|x.key()),a.refund_destination.as_ref().map(|x|x.key()))))
}

// Current fixed args, eleven fixed public keys and nine optional public keys.
// Hash only the consumed prefix: absent optional routes occupy one Borsh byte.
const MAX_REQUEST_PREIMAGE: usize = CorrectFinancedRunArgs::INIT_SPACE + 11*32 + 9*33;
#[inline(never)]
fn bounded_request_hash<T: AnchorSerialize>(preimage: &T) -> Result<[u8;32]> {
    let mut bytes=[0u8;MAX_REQUEST_PREIMAGE];
    let remaining={
        let mut output=&mut bytes[..];
        preimage.serialize(&mut output)?;
        output.len()
    };
    Ok(hashv(&[b"DEXTER:FUNDED-CORRECTION:2",CORE_ID.as_ref(),
        &bytes[..MAX_REQUEST_PREIMAGE-remaining]]).to_bytes())
}

pub fn correct<'info>(ctx: Context<'info, CorrectFinancedRun<'info>>, args: CorrectFinancedRunArgs) -> Result<()> {
    let a=ctx.accounts;
    let op=correction_operation_id(a.run.key(),&args.request_id);
    let digest=request_digest(a,&args)?;
    let sid=tab_events::step_id(&op,&digest);
    require!(args.reason_hash!=[0;32],CoreError::FundingCorrection);
    a.claim.check(a.claim.key())?; a.run.check(a.run.key(),a.claim.key(),&a.claim)?; a.line.check(a.line.key())?;
    a.facility.check(a.facility.key())?;
    owner(&a.principal,a.company_owner.key(),args.company_owner_generation)?;
    valid_principal(&a.supplier_principal)?; valid_principal(&a.creditor_principal)?;
    require!(a.principal.key()==a.claim.terms.principal && a.principal.domain==a.claim.terms.domain
        && a.supplier_principal.key()==a.claim.supplier_principal && a.supplier_principal.domain==a.claim.terms.domain
        && a.creditor_principal.key()==a.run.current_creditor && a.creditor_principal.domain==a.claim.terms.domain
        && a.claim.credit_line==a.line.key() && a.line.company==a.principal.key()
        && a.line.issuer==a.claim.issuer && a.line.issuer_custody==a.claim.issuer_custody
        && a.line.facility==a.claim.facility && a.line.mint==a.mint.key()
        && a.claim.facility==a.facility.key() && a.facility.issuer==a.claim.issuer
        && a.facility.custody==a.custody.key() && a.claim.custody==a.custody.key()
        && a.facility.financier==a.custody.principal && a.facility.mint==a.mint.key()
        && a.claim.terms.asset==a.mint.key() && a.custody.domain==a.claim.terms.domain,
        CoreError::FundingCorrectionLineage);
    a.claim.guard(args.expected_steps)?;
    reservation_matches(&a.reservation,&a.claim)?;
    custody_token(&a.custody,&a.custody_token,a.mint.key())?;
    let before=a.run.corrections; let principal_before=a.run.principal;
    let (after,principal_after,plan)=math(before.apply(&principal_before,a.run.gross_paid,
        args.unpaid_reduction,args.collected_return,args.creditor_compensation))?;
    require!(plan.total>0 || args.interest_waiver>0,CoreError::FundingCorrection);
    if plan.total>0 {
        let signer=a.supplier_owner.as_ref().ok_or_else(||error!(CoreError::FundingCorrectionAuthority))?;
        owner(&a.supplier_principal,signer.key(),args.supplier_owner_generation)?;
    } else { require!(a.supplier_owner.is_none() && args.supplier_owner_generation==0,CoreError::FundingCorrectionAuthority); }
    let creditor_consent=plan.creditor_shortfall>0 || args.interest_waiver>0;
    if creditor_consent {
        let signer=a.creditor_owner.as_ref().ok_or_else(||error!(CoreError::FundingCorrectionAuthority))?;
        owner(&a.creditor_principal,signer.key(),args.creditor_owner_generation)?;
    } else { require!(a.creditor_owner.is_none() && args.creditor_owner_generation==0,CoreError::FundingCorrectionAuthority); }

    let (accrued,new_interest)=math(a.run.interest.accrue(principal_before.outstanding,Clock::get()?.unix_timestamp,&a.run.interest_terms))?;
    require!(new_interest<=args.maximum_new_interest,CoreError::FundingChargeCap);
    let interest=if args.interest_waiver==0 { accrued } else { math(accrued.waive(args.interest_waiver,&a.run.interest_terms))? };
    let unused_charge=if principal_after.outstanding==0 && math(interest.outstanding())?==0 {
        minus(minus(a.run.interest_terms.maximum_interest,interest.accrued)?,a.run.fee_authority_released)?
    } else { 0 };
    require!(plus(new_interest,unused_charge)?<=a.claim.fee_authority_remaining,CoreError::FundingChargeCap);
    require!(args.unpaid_reduction<=a.claim.payable && args.unpaid_reduction<=a.custody.payable
        && args.unpaid_reduction<=a.facility.reserved,CoreError::FundingCorrectionLimit);

    // Collected-money returns are bounded per run by `run.corrections` (the plan above).
    let (refund_principal,refund_authority)=record_principal_refund(a,plan.prior_repayment_refund)?;

    let mut creditor_copy:Option<Box<FinancierCustody>>=None;
    if plan.creditor_cash>0 {
        let info=a.creditor_custody.as_ref().ok_or_else(||error!(CoreError::FundingTokenRouting))?;
        let dest=a.creditor_destination.as_ref().ok_or_else(||error!(CoreError::FundingTokenRouting))?;
        require!(info.is_writable && *info.owner==CORE_ID,CoreError::FundingCustody);
        let c=FinancierCustody::try_deserialize(&mut &info.try_borrow_data()?[..])?;
        c.check_key(info.key())?;
        require!(info.key()==a.run.creditor_custody && c.principal==a.run.current_creditor
            && c.domain==a.claim.terms.domain && c.mint==a.mint.key() && c.token_account==dest.key()
            && dest.key()==associated_token::get_associated_token_address(&c.wallet,&a.mint.key()),CoreError::FundingCreditor);
        let token=read_token(&dest.to_account_info(),a.mint.key())?;
        require_keys_eq!(token.owner,c.wallet,CoreError::FundingTokenRouting); c.check(token.amount)?;
        creditor_copy=Some(Box::new(c));
    } else { require!(a.creditor_custody.is_none() && a.creditor_destination.is_none(),CoreError::FundingTokenRouting); }
    let mut funder=Pubkey::default(); let mut funding_authority=Pubkey::default(); let mut funding_from=Pubkey::default();
    if plan.required_cash>0 {
        let principal=a.funding_principal.as_deref().ok_or_else(||error!(CoreError::FundingCorrectionAuthority))?;
        let signer=a.funding_authority.as_ref().ok_or_else(||error!(CoreError::FundingCorrectionAuthority))?;
        let from=a.funding_from.as_ref().ok_or_else(||error!(CoreError::FundingTokenRouting))?;
        owner(principal,signer.key(),args.funding_owner_generation)?;
        require!(signer.key().is_on_curve() && from.owner==signer.key() && from.mint==a.mint.key()
            && from.key()==associated_token::get_associated_token_address(&signer.key(),&a.mint.key()),CoreError::FundingTokenRouting);
        funder=principal.key(); funding_authority=signer.key(); funding_from=from.key();
        if plan.creditor_cash>0 {
            let dest=a.creditor_destination.as_ref().ok_or_else(||error!(CoreError::FundingTokenRouting))?;
            transfer_checked_exact(&from.to_account_info(),&dest.to_account_info(),&signer.to_account_info(),
                &a.mint,a.token_program.key(),plan.creditor_cash,&[])?;
        }
        if plan.prior_repayment_refund>0 {
            let dest=a.refund_destination.as_ref().ok_or_else(||error!(CoreError::FundingTokenRouting))?;
            transfer_checked_exact(&from.to_account_info(),&dest.to_account_info(),&signer.to_account_info(),
                &a.mint,a.token_program.key(),plan.prior_repayment_refund,&[])?;
        }
    } else {
        require!(a.funding_principal.is_none() && a.funding_authority.is_none() && a.funding_from.is_none()
            && args.funding_owner_generation==0,CoreError::FundingCorrectionAuthority);
    }
    if let Some(mut creditor)=creditor_copy {
        let info=a.creditor_custody.as_ref().ok_or_else(||error!(CoreError::FundingCustody))?;
        let dest=a.creditor_destination.as_ref().ok_or_else(||error!(CoreError::FundingTokenRouting))?;
        let actual=read_token(&dest.to_account_info(),a.mint.key())?.amount;
        if info.key()==a.custody.key() { a.custody.credit(plan.creditor_cash)?; }
        else {
            creditor.credit(plan.creditor_cash)?; creditor.check(actual)?;
            creditor.try_serialize(&mut &mut info.try_borrow_mut_data()?[..])?;
        }
    }
    if let Some(info)=a.refund_custody.as_ref() {
        let dest=a.refund_destination.as_ref().ok_or_else(||error!(CoreError::FundingTokenRouting))?;
        if info.key()==a.custody.key() {
            // This typed value is serialized by Anchor on exit. Retain any
            // current-creditor credit already applied to it above.
            a.custody.credit(plan.prior_repayment_refund)?;
            a.custody.check(read_token(&dest.to_account_info(),a.mint.key())?.amount)?;
        } else {
            // Reread after creditor serialization when both destinations share
            // its unchecked custody, preventing a stale-copy overwrite.
            credit_refund_custody(info,&dest.to_account_info(),a.mint.key(),
                refund_principal,refund_authority,plan.prior_repayment_refund)?;
        }
    }
    registry_service(&a.principal,&a.budget,&a.reservation,&a.registry_authority,
        &a.registry_program,interest_accrual_operation_id(&op,&a.run.key())?,new_interest,false,args.service_without_pool)?;
    registry_service(&a.principal,&a.budget,&a.reservation,&a.registry_authority,
        &a.registry_program,interest_release_operation_id(&op,&a.run.key())?,unused_charge,true,args.service_without_pool)?;

    a.run.principal=principal_after; a.run.corrections=after; a.run.interest=interest;
    a.run.fee_authority_released=plus(a.run.fee_authority_released,unused_charge)?;
    a.claim.payable=minus(a.claim.payable,args.unpaid_reduction)?;
    a.claim.debt=minus(a.claim.debt,plan.principal_credit)?;
    a.claim.principal_extinguished=plus(a.claim.principal_extinguished,plan.principal_credit)?;
    a.claim.corrections.unpaid_reduced=plus(a.claim.corrections.unpaid_reduced,args.unpaid_reduction)?;
    a.claim.corrections.collected_returned=plus(a.claim.corrections.collected_returned,args.collected_return)?;
    a.claim.corrections.principal_credited=plus(a.claim.corrections.principal_credited,plan.principal_credit)?;
    a.claim.corrections.prior_repayment_refunded=plus(a.claim.corrections.prior_repayment_refunded,plan.prior_repayment_refund)?;
    a.claim.interest_outstanding=minus(plus(a.claim.interest_outstanding,new_interest)?,args.interest_waiver)?;
    a.claim.fee_authority_remaining=minus(a.claim.fee_authority_remaining,plus(new_interest,unused_charge)?)?;
    a.claim.released_fee_caps=plus(a.claim.released_fee_caps,unused_charge)?;
    a.line.principal_outstanding=minus(a.line.principal_outstanding,plan.principal_credit)?;
    a.line.fee_exposure=minus(a.line.fee_exposure,plus(args.interest_waiver,unused_charge)?)?;
    a.line.revision=plus(a.line.revision,1)?;
    if args.unpaid_reduction>0 {
        a.custody.payable=minus(a.custody.payable,args.unpaid_reduction)?;
        a.custody.revision=plus(a.custody.revision,1)?;
        a.facility.reserved=minus(a.facility.reserved,args.unpaid_reduction)?;
        a.facility.revision=plus(a.facility.revision,1)?;
    }
    a.custody_token.reload()?; a.custody.check(a.custody_token.amount)?;
    a.claim.check(a.claim.key())?; a.run.check(a.run.key(),a.claim.key(),&a.claim)?; a.line.check(a.line.key())?;
    a.facility.check(a.facility.key())?;
    let company=a.principal.key(); let creditor=a.run.current_creditor;
    let change_count=7 + usize::from(plan.total>0) + 3*usize::from(args.unpaid_reduction>0)
        + usize::from(plan.required_cash>0) + usize::from(plan.creditor_cash>0)
        + usize::from(plan.prior_repayment_refund>0);
    let mut changes=Vec::with_capacity(change_count);
    changes.extend([dec(company,CUSTOMER_PRINCIPAL,plan.principal_credit),
        dec(creditor,OWNED_PRINCIPAL,plan.principal_credit),
        inc(company,INTEREST_OWED,new_interest),inc(creditor,INTEREST_OWNED,new_interest),
        dec(company,INTEREST_OWED,args.interest_waiver),dec(creditor,INTEREST_OWNED,args.interest_waiver),
        dec(company,CREDIT_CHARGE_EXPOSURE,plus(args.interest_waiver,unused_charge)?)]);
    if plan.total>0 { changes.push(inc(a.supplier_principal.key(),SUPPLIER_RECEIVABLE,0)); }
    if args.unpaid_reduction>0 { changes.extend([
        dec(a.claim.issuer,UNPAID_SUPPLIER,args.unpaid_reduction),
        dec(a.supplier_principal.key(),SUPPLIER_RECEIVABLE,args.unpaid_reduction),
        dec(a.custody.principal,FUNDING_RESERVED,args.unpaid_reduction)]); }
    if plan.required_cash>0 { changes.push(inc(funder,CASH_SENT,plan.required_cash)); }
    if plan.creditor_cash>0 { changes.push(inc(creditor,CASH_RECEIVED,plan.creditor_cash)); }
    if plan.prior_repayment_refund>0 { changes.push(inc(refund_principal,CASH_RECEIVED,plan.prior_repayment_refund)); }
    let mut record=record_base(company,op,digest,FINANCE_CORRECT,a.mint.key(),a.claim.key(),a.run.key(),plan.total);
    record.debit_principal=funder;
    record.debit_authority=funding_authority; record.debit_from=funding_from;
    record.current_creditor=creditor; record.ownership_revision=a.run.ownership_revision;
    record.creditor_destination=a.creditor_destination.as_ref().map(|x|x.key()).unwrap_or_default();
    // Numbered by its payer, or by the company when no cash moves: a landed
    // correction, or a rebuild of it, can never apply again.
    let numbered=Some((if plan.required_cash>0 { funder } else { company },args.number));
    let booked=journal_numbered(&sid,a.mint.key(),&changes,ctx.remaining_accounts,&a.pool_program,
        args.service_without_pool,false,numbered)?;
    let header=TabStepHeader{tab:a.claim.key(),step:a.claim.next_step()?,step_id:sid};
    let event=corrected_event(a,&header,&args,&record,&booked,Routes{funder,funding_authority,funding_from,
        refund_principal,refund_authority},(plan,principal_before,principal_after,before,after),
        (new_interest,unused_charge))?;
    tab_events::record_tab_step(&mut a.claim,&a.core_event_authority,event, true)
}

struct Routes { funder:Pubkey, funding_authority:Pubkey, funding_from:Pubkey, refund_principal:Pubkey, refund_authority:Pubkey }

/// The correction's event, built in its own frame: the correction detail is
/// too large to sit beside the handler's state on the 4 KB SBF stack.
#[inline(never)]
#[allow(clippy::type_complexity)]
fn corrected_event(a:&CorrectFinancedRun,header:&TabStepHeader,args:&CorrectFinancedRunArgs,record:&FinanceDetail,
    booked:&Booked,routes:Routes,
    amounts:(CorrectionPlan,PrincipalAmounts,PrincipalAmounts,CorrectionAmounts,CorrectionAmounts),
    interest:(u64,u64))->Result<EventBytes> {
    let (plan,principal_before,principal_after,corrections_before,corrections_after)=amounts;
    let mut request=args.clone(); request.expected_steps=0;
    let detail=Box::new(CorrectionDetail{request,capture_digest:a.run.capture_digest,acceptance_hash:a.claim.acceptance_hash,
        supplier_principal:a.claim.supplier_principal,frozen_supplier_wallet:a.claim.terms.supplier,
        company_owner:a.company_owner.key(),supplier_owner:a.supplier_owner.as_ref().map(|x|x.key()).unwrap_or_default(),
        creditor_owner:a.creditor_owner.as_ref().map(|x|x.key()).unwrap_or_default(),
        current_creditor:a.run.current_creditor,creditor_custody:a.run.creditor_custody,
        creditor_destination:a.creditor_destination.as_ref().map(|x|x.key()).unwrap_or_default(),
        funding_principal:routes.funder,funding_authority:routes.funding_authority,funding_from:routes.funding_from,
        refund_principal:routes.refund_principal,refund_authority:routes.refund_authority,
        refund_destination:a.refund_destination.as_ref().map(|x|x.key()).unwrap_or_default(),
        plan,principal_before,principal_after,corrections_before,corrections_after,
        newly_accrued_interest:interest.0,interest_waived:args.interest_waiver,unused_charge_authority_released:interest.1});
    let repayment_record=a.repayment_record.as_ref().map(|x|x.key()).unwrap_or_default();
    EventBytes::new::<RunCorrected>(TabStepHeader::INIT_SPACE+32+CorrectionDetail::INIT_SPACE+DETAIL_SPACE
        +books_space(&booked.books,&booked.changes))
        .put(header)?.put(&repayment_record)?.put(&*detail)?.put(record)?.put(&booked.books)?.put(&booked.changes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_request_hash_preserves_optional_route_borsh_bytes() {
        let bytes=vec![0u8;CorrectFinancedRunArgs::INIT_SPACE];
        let mut args=CorrectFinancedRunArgs::deserialize(&mut &bytes[..]).unwrap();
        args.request_id=[21;32]; args.reason_hash=[22;32];
        args.unpaid_reduction=37; args.collected_return=41;
        let key=Pubkey::new_from_array([23;32]);
        for optional in [None,Some(key)] {
            let preimage=(&args,(key,key,key,optional,key,optional),
                (key,key,key,key,key,key,key),
                (optional,optional,optional,optional,optional,optional,optional));
            let actual=bounded_request_hash(&preimage).unwrap();
            let original=financial_digest(b"DEXTER:FUNDED-CORRECTION:2",&preimage).unwrap();
            assert_eq!(actual,original);
            let mut serialized=Vec::new(); preimage.serialize(&mut serialized).unwrap();
            assert_eq!(serialized.len(),MAX_REQUEST_PREIMAGE-if optional.is_none(){9*32}else{0});
        }
    }
}
