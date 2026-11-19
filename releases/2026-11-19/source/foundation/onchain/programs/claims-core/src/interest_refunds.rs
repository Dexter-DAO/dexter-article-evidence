//! Voluntary cash returns of previously collected borrower interest.
//! Original repayment, income, principal and accepted funding remain historical.
//! Each original recipient approves its own return from current OwnerCash; the
//! exact original repayment payer receives it even after title/owner changes.
use anchor_lang::prelude::*;
use anchor_spl::{associated_token, token::{self, Mint, Token, TokenAccount}};
use dexter_contracts::{plus, POOL_ID};
use dexter_contracts::funded::*;
use principal_registry::Principal;
use crate::{CoreError, funding_state::*, funding_journal::{journal_numbered, inc},
    historical_refunds::{validate_refund_destination, credit_refund_custody},
    tab_events::{self, TabStepHeader, InterestRefunded}};

pub fn interest_refund_operation_id(run: Pubkey, request_id: &[u8; 32]) -> [u8; 32] {
    crate::funding::operation_id(b"refund-run-interest", run, request_id)
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct RefundFundedInterestArgs {
    pub request_id: [u8; 32],
    pub reason_hash: [u8; 32],
    pub lender_interest_refund: u64,
    pub protocol_interest_refund: u64,
    pub lender_owner_generation: u64,
    pub protocol_owner_generation: u64,
    /// The Tab's money-step count this refund was prepared against.
    pub expected_steps: u32,
    pub service_without_pool: bool,
    /// The paying party's number for this refund, admitted once by its book
    /// window: the lender's when it refunds any interest, otherwise the
    /// protocol beneficiary's. Part of the request digest.
    pub number: u64,
}

#[derive(Accounts)]
#[instruction(args: RefundFundedInterestArgs)]
pub struct RefundFundedInterest<'info> {
    #[account(mut)] pub payer: Signer<'info>,
    pub principal: Box<Account<'info, Principal>>,
    #[account(mut)] pub claim: Box<Account<'info, FundedClaim>>,
    pub run: Box<Account<'info, FundedRun>>,
    /// The run's repayment record whose payer gets the interest back.
    #[account(mut)] pub repayment_record: Box<Account<'info, RepaymentRecord>>,
    pub lender_principal: Option<Box<Account<'info, Principal>>>,
    pub lender_authority: Option<Signer<'info>>,
    /// CHECK: Current historical-lender OwnerCash ATA; may alias protocol_from.
    #[account(mut, dup)] pub lender_from: Option<UncheckedAccount<'info>>,
    pub protocol_principal: Option<Box<Account<'info, Principal>>>,
    pub protocol_authority: Option<Signer<'info>>,
    /// CHECK: Current frozen-beneficiary OwnerCash ATA; read fresh for each leg.
    #[account(mut, dup)] pub protocol_from: Option<UncheckedAccount<'info>>,
    /// CHECK: Exact original repayment source, owner and mint are authenticated.
    #[account(mut)] pub refund_destination: UncheckedAccount<'info>,
    pub mint: Box<Account<'info, Mint>>,
    /// CHECK: Accepted servicing can keep complete Core effects without Pool CPI.
    #[account(address = POOL_ID)] pub pool_program: UncheckedAccount<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    /// CHECK: Canonical custody of the original governed payer, authenticated
    /// from the repayment record; absent for ordinary owner cash.
    #[account(mut,dup)] pub refund_custody: Option<UncheckedAccount<'info>>,
    /// CHECK: ClaimsCore's event authority.
    #[account(address = crate::EVENT_AUTHORITY_AND_BUMP.0)] pub core_event_authority: UncheckedAccount<'info>,
    /// CHECK: ClaimsCore itself.
    #[account(address = crate::ID)] pub core_program: UncheckedAccount<'info>,
}

fn read_token(info: &AccountInfo, mint: Pubkey) -> Result<TokenAccount> {
    require_keys_eq!(*info.owner, token::ID, CoreError::FundingTokenRouting);
    let value = TokenAccount::try_deserialize(&mut &info.try_borrow_data()?[..])?;
    require_keys_eq!(value.mint, mint, CoreError::FundingTokenRouting);
    Ok(value)
}

#[derive(Clone, Copy, Default)]
struct CashLeg { principal: Pubkey, authority: Pubkey, from: Pubkey }

#[allow(clippy::too_many_arguments)]
fn return_leg<'info>(principal: Option<&Account<'info, Principal>>,
    authority: Option<&Signer<'info>>, from: Option<&UncheckedAccount<'info>>,
    expected_principal: Pubkey, generation: u64, amount: u64,
    destination: &AccountInfo<'info>, mint: &Account<'info, Mint>,
    token_program: Pubkey) -> Result<CashLeg> {
    if amount == 0 {
        require!(principal.is_none() && authority.is_none() && from.is_none() && generation == 0,
            CoreError::FundingCorrectionAuthority);
        return Ok(CashLeg::default());
    }
    let principal = principal.ok_or_else(|| error!(CoreError::FundingCorrectionAuthority))?;
    let authority = authority.ok_or_else(|| error!(CoreError::FundingCorrectionAuthority))?;
    let from = from.ok_or_else(|| error!(CoreError::FundingTokenRouting))?;
    owner(principal, authority.key(), generation)?;
    require!(principal.key() == expected_principal && authority.key().is_on_curve(),
        CoreError::FundingCorrectionAuthority);
    let token = read_token(&from.to_account_info(), mint.key())?;
    require!(token.owner == authority.key()
        && from.key() == associated_token::get_associated_token_address(&authority.key(), &mint.key()),
        CoreError::FundingTokenRouting);
    // The helper rereads both balances. Two legs from the same ATA therefore
    // observe the first transfer and cannot each reuse its original balance.
    transfer_checked_exact(&from.to_account_info(), destination, &authority.to_account_info(),
        mint, token_program, amount, &[])?;
    Ok(CashLeg { principal: principal.key(), authority: authority.key(), from: from.key() })
}

// Keep digest preimages and the immutable result out of the transition's SBF
// frame. These are ordinary function boundaries, not additional instructions.
#[inline(never)]
fn request_digest(a: &RefundFundedInterest, args: &RefundFundedInterestArgs) -> Result<[u8; 32]> {
    // The staleness guard and the projection choice are not part of the request.
    let mut economics = args.clone(); economics.service_without_pool = false; economics.expected_steps = 0;
    // The optional custody is uniquely derived from the already-bound original
    // payer source; fresh execution authenticates the actual account before
    // returning cash.
    financial_digest(b"DEXTER:FUNDED-INTEREST-REFUND:2", &(
        &economics,
        (a.principal.key(), a.claim.key(), a.run.key(), a.repayment_record.key(),
            a.refund_destination.key(), a.mint.key()),
        (a.lender_principal.as_ref().map(|x| x.key()), a.lender_authority.as_ref().map(|x| x.key()),
            a.lender_from.as_ref().map(|x| x.key()), a.protocol_principal.as_ref().map(|x| x.key()),
            a.protocol_authority.as_ref().map(|x| x.key()), a.protocol_from.as_ref().map(|x| x.key()))))
}

pub fn refund<'info>(ctx: Context<'info, RefundFundedInterest<'info>>,
    args: RefundFundedInterestArgs) -> Result<()> {
    let a = ctx.accounts;
    let operation_id = interest_refund_operation_id(a.run.key(), &args.request_id);
    let digest = request_digest(a, &args)?;
    let sid = tab_events::step_id(&operation_id, &digest);
    let amount = plus(args.lender_interest_refund, args.protocol_interest_refund)?;
    require!(args.reason_hash != [0; 32] && amount > 0, CoreError::FundingCorrection);
    valid_principal(&a.principal)?;
    a.claim.check(a.claim.key())?;
    a.run.check(a.run.key(), a.claim.key(), &a.claim)?;
    a.claim.guard(args.expected_steps)?;
    require!(a.principal.key() == a.claim.terms.principal && a.principal.domain == a.claim.terms.domain
        && a.claim.terms.asset == a.mint.key(), CoreError::FundingCorrectionLineage);
    let record_key = a.repayment_record.key();
    a.repayment_record.check(record_key)?;
    require!(a.repayment_record.run == a.run.key() && a.repayment_record.creditor != Pubkey::default(),
        CoreError::FundingCorrectionLineage);
    validate_refund_destination(&a.repayment_record, &a.refund_destination.to_account_info(), a.mint.key(),
        a.refund_custody.as_ref())?;
    let record = &a.repayment_record;
    let usage_after = math(record.usage.record(0, args.lender_interest_refund, args.protocol_interest_refund,
        record.principal_paid, record.interest_paid, record.protocol_interest_paid))?;
    let (creditor, payer_principal, payer_authority) = (record.creditor, record.payer_principal, record.payer_authority);

    let lender = return_leg(a.lender_principal.as_deref(), a.lender_authority.as_ref(), a.lender_from.as_ref(),
        creditor, args.lender_owner_generation, args.lender_interest_refund,
        &a.refund_destination.to_account_info(), &a.mint, a.token_program.key())?;
    let protocol = return_leg(a.protocol_principal.as_deref(), a.protocol_authority.as_ref(), a.protocol_from.as_ref(),
        a.run.interest_terms.protocol_beneficiary, args.protocol_owner_generation, args.protocol_interest_refund,
        &a.refund_destination.to_account_info(), &a.mint, a.token_program.key())?;
    if let Some(info) = a.refund_custody.as_ref() {
        credit_refund_custody(info, &a.refund_destination.to_account_info(), a.mint.key(),
            payer_principal, payer_authority, amount)?;
    }
    a.repayment_record.usage = usage_after;

    finish_refund(a, ctx.remaining_accounts, &args, operation_id, digest, &sid, amount,
        payer_principal, &lender, &protocol)
}

#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn finish_refund<'info>(a: &mut RefundFundedInterest<'info>, remaining: &[AccountInfo<'info>],
    args: &RefundFundedInterestArgs, operation_id: [u8; 32], digest: [u8; 32], sid: &[u8; 32], amount: u64,
    payer_principal: Pubkey, lender: &CashLeg, protocol: &CashLeg) -> Result<()> {
    // These are gross authenticated cash movements. Collected income and
    // original debt repayments are not erased or converted into new capacity.
    let mut changes = vec![inc(payer_principal, CASH_RECEIVED, amount)];
    if args.lender_interest_refund > 0 {
        changes.push(inc(lender.principal, CASH_SENT, args.lender_interest_refund));
    }
    if args.protocol_interest_refund > 0 {
        changes.push(inc(protocol.principal, CASH_SENT, args.protocol_interest_refund));
    }
    let mut record = record_base(a.principal.key(), operation_id, digest, FINANCE_INTEREST_REFUND,
        a.mint.key(), a.claim.key(), a.run.key(), amount);
    record.current_creditor = a.repayment_record.creditor;
    let refund = tab_events::InterestRefundDetail { reason_hash: args.reason_hash,
        lender_interest_refund: args.lender_interest_refund, protocol_interest_refund: args.protocol_interest_refund,
        lender_principal: lender.principal, lender_authority: lender.authority, lender_from: lender.from,
        protocol_principal: protocol.principal, protocol_authority: protocol.authority, protocol_from: protocol.from,
        refund_principal: payer_principal, refund_authority: a.repayment_record.payer_authority,
        refund_destination: a.refund_destination.key(), number: args.number };
    // The refund's payer numbers it: a landed refund, or a rebuild of it, is
    // refused by the payer's book window.
    let paying = if args.lender_interest_refund > 0 { lender.principal } else { protocol.principal };
    let booked = journal_numbered(sid, a.mint.key(), &changes, remaining, &a.pool_program.to_account_info(),
        args.service_without_pool, false, Some((paying, args.number)))?;
    let header = TabStepHeader { tab: a.claim.key(), step: a.claim.next_step()?, step_id: *sid };
    let event = tab_events::extra_event::<InterestRefunded, _>(&header, &(a.repayment_record.key(), refund),
        32 + tab_events::InterestRefundDetail::INIT_SPACE, &record, &booked.books, &booked.changes)?;
    tab_events::record_tab_step(&mut a.claim, &a.core_event_authority, event, true)
}
