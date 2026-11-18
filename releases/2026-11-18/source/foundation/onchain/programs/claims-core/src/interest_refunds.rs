//! Voluntary cash returns of previously collected borrower interest.
//! Original repayment, income, principal and accepted funding remain historical.
//! Each original recipient approves its own return from current OwnerCash; the
//! exact original repayment payer receives it even after title/owner changes.
use anchor_lang::prelude::*;
use anchor_spl::{associated_token, token::{self, Mint, Token, TokenAccount}};
use dexter_contracts::{plus, CORE_ID, POOL_ID};
use dexter_contracts::corrections::{CORRECTION_VERSION, RepaymentRefundUsage};
use dexter_contracts::funded::*;
use principal_registry::Principal;
use crate::{CoreError, corrections::{RepaymentRefundRecord, source_receipt},
    funding_state::*, funding_journal::{journal, inc},
    historical_refunds::{validate_refund_destination, credit_refund_custody}};

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
    pub service_without_pool: bool,
}

/// Immutable detail for the two independently approved cash legs. This sidecar
/// keeps the existing financial journal layout and gross-income fields intact.
#[account]
#[derive(InitSpace)]
pub struct InterestRefundReceipt {
    pub version: u16, pub bump: u8,
    pub company: Pubkey, pub claim: Pubkey, pub run: Pubkey,
    pub operation_id: [u8; 32], pub request_digest: [u8; 32],
    pub request: RefundFundedInterestArgs,
    pub original_repayment: Pubkey,
    pub original_request_digest: [u8; 32], pub original_effects_hash: [u8; 32],
    pub original_principal_paid: u64, pub original_interest_paid: u64,
    pub original_protocol_interest: u64,
    pub capture_receipt: Pubkey, pub capture_digest: [u8; 32], pub acceptance_hash: [u8; 32],
    pub historical_lender: Pubkey, pub historical_lender_destination: Pubkey,
    pub protocol_beneficiary: Pubkey, pub frozen_protocol_destination: Pubkey,
    pub lender_authority: Pubkey, pub lender_from: Pubkey,
    pub protocol_authority: Pubkey, pub protocol_from: Pubkey,
    pub refund_principal: Pubkey, pub refund_authority: Pubkey, pub refund_destination: Pubkey,
    pub repayment_usage: Pubkey, pub usage_before: RepaymentRefundUsage, pub usage_after: RepaymentRefundUsage,
    pub financial_receipt: Pubkey, pub financial_effect_hash: [u8; 32],
    pub result_hash: [u8; 32],
}
impl InterestRefundReceipt {
    pub fn hash(&self) -> Result<[u8; 32]> {
        let mut value = self.clone(); value.result_hash = [0; 32];
        financial_digest(b"DEXTER:FUNDED-INTEREST-REFUND-RESULT:1", &value)
    }
    pub fn check(&self, key: Pubkey) -> Result<()> {
        let (expected, bump) = Pubkey::find_program_address(
            &[b"interest-refund-result", self.run.as_ref(), &self.operation_id], &CORE_ID);
        require!(self.version == CORRECTION_VERSION && key == expected && self.bump == bump
            && self.operation_id == interest_refund_operation_id(self.run, &self.request.request_id)
            && self.result_hash == self.hash()?, CoreError::FundingCorrectionReplay);
        Ok(())
    }
}

#[derive(Accounts)]
#[instruction(args: RefundFundedInterestArgs)]
pub struct RefundFundedInterest<'info> {
    #[account(mut)] pub payer: Signer<'info>,
    pub principal: Box<Account<'info, Principal>>,
    pub claim: Box<Account<'info, FundedClaim>>,
    pub run: Box<Account<'info, FundedRun>>,
    /// CHECK: Strict Core receipt reader authenticates the original repayment.
    pub original_repayment: UncheckedAccount<'info>,
    #[account(init_if_needed, payer = payer, space = 8 + RepaymentRefundRecord::INIT_SPACE,
        seeds = [b"correction-repayment", original_repayment.key().as_ref()], bump)]
    pub repayment_usage: Box<Account<'info, RepaymentRefundRecord>>,
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
    #[account(init_if_needed, payer = payer, space = 8 + FinanceReceipt::INIT_SPACE,
        seeds = [b"finance-operation", principal.key().as_ref(),
            &interest_refund_operation_id(run.key(), &args.request_id)], bump)]
    pub receipt: Box<Account<'info, FinanceReceipt>>,
    #[account(init_if_needed, payer = payer, space = 8 + InterestRefundReceipt::INIT_SPACE,
        seeds = [b"interest-refund-result", run.key().as_ref(),
            &interest_refund_operation_id(run.key(), &args.request_id)], bump)]
    pub interest_refund: Box<Account<'info, InterestRefundReceipt>>,
    /// CHECK: Accepted servicing can keep complete Core effects without Pool CPI.
    #[account(address = POOL_ID)] pub pool_program: UncheckedAccount<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    /// CHECK: Canonical custody of the original governed payer, authenticated
    /// from the immutable repayment receipt; absent for ordinary owner cash.
    #[account(mut,dup)] pub refund_custody: Option<UncheckedAccount<'info>>,
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
    let mut economics = args.clone(); economics.service_without_pool = false;
    // The optional custody is uniquely derived from the already-bound original
    // payer source. Excluding it preserves ordinary-cash historical digests;
    // fresh execution authenticates the actual account before returning cash.
    financial_digest(b"DEXTER:FUNDED-INTEREST-REFUND:1", &(
        &economics,
        (a.principal.key(), a.claim.key(), a.run.key(), a.original_repayment.key(),
            a.repayment_usage.key(), a.refund_destination.key(), a.mint.key()),
        (a.lender_principal.as_ref().map(|x| x.key()), a.lender_authority.as_ref().map(|x| x.key()),
            a.lender_from.as_ref().map(|x| x.key()), a.protocol_principal.as_ref().map(|x| x.key()),
            a.protocol_authority.as_ref().map(|x| x.key()), a.protocol_from.as_ref().map(|x| x.key()))))
}

pub fn refund<'info>(ctx: Context<'info, RefundFundedInterest<'info>>,
    args: RefundFundedInterestArgs) -> Result<()> {
    let a = ctx.accounts;
    let operation_id = interest_refund_operation_id(a.run.key(), &args.request_id);
    let digest = request_digest(a, &args)?;
    // An exact historical request remains recoverable after ownership changes,
    // later payments or later corrections. Live OwnerCash checks follow replay.
    if replay(&a.receipt, &digest)? {
        a.interest_refund.check(a.interest_refund.key())?;
        require!(a.interest_refund.request_digest == digest
            && a.interest_refund.financial_receipt == a.receipt.key()
            && a.interest_refund.financial_effect_hash == a.receipt.data.effects_hash
            && a.receipt.data.kind == FINANCE_INTEREST_REFUND
            && a.receipt.data.related_receipt == a.interest_refund.key(), CoreError::FundingCorrectionReplay);
        return Ok(());
    }
    let amount = plus(args.lender_interest_refund, args.protocol_interest_refund)?;
    require!(a.interest_refund.version == 0 && args.reason_hash != [0; 32] && amount > 0,
        CoreError::FundingCorrection);
    valid_principal(&a.principal)?;
    a.claim.check(a.claim.key())?;
    a.run.check(a.run.key(), a.claim.key(), &a.claim)?;
    require!(a.principal.key() == a.claim.terms.principal && a.principal.domain == a.claim.terms.domain
        && a.claim.terms.asset == a.mint.key(), CoreError::FundingCorrectionLineage);
    let source = source_receipt(&a.original_repayment.to_account_info(), FINANCE_REPAY,
        a.principal.key(), a.claim.key(), a.run.key(), a.mint.key())?;
    require!(source.amount == plus(source.principal_paid, source.interest_paid)?
        && source.current_creditor != Pubkey::default(),
        CoreError::FundingCorrectionLineage);
    validate_refund_destination(&source, &a.refund_destination.to_account_info(), a.mint.key(),
        a.refund_custody.as_ref())?;
    let usage_key = a.repayment_usage.key();
    a.repayment_usage.initialize_or_check(usage_key, a.original_repayment.key(), a.claim.key(), a.run.key())?;
    let usage_before = a.repayment_usage.usage;
    let usage_after = math(usage_before.record(0, args.lender_interest_refund, args.protocol_interest_refund,
        source.principal_paid, source.interest_paid, source.protocol_interest))?;

    let lender = return_leg(a.lender_principal.as_deref(), a.lender_authority.as_ref(), a.lender_from.as_ref(),
        source.current_creditor, args.lender_owner_generation, args.lender_interest_refund,
        &a.refund_destination.to_account_info(), &a.mint, a.token_program.key())?;
    let protocol = return_leg(a.protocol_principal.as_deref(), a.protocol_authority.as_ref(), a.protocol_from.as_ref(),
        a.run.interest_terms.protocol_beneficiary, args.protocol_owner_generation, args.protocol_interest_refund,
        &a.refund_destination.to_account_info(), &a.mint, a.token_program.key())?;
    if let Some(info) = a.refund_custody.as_ref() {
        credit_refund_custody(info, &a.refund_destination.to_account_info(), a.mint.key(),
            source.debit_principal, source.debit_authority, amount)?;
    }
    a.repayment_usage.usage = usage_after;

    finish_refund(a, ctx.remaining_accounts, &args, operation_id, digest, amount,
        &source, &lender, &protocol, usage_before, usage_after, ctx.bumps.interest_refund)
}

#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn finish_refund<'info>(a: &mut RefundFundedInterest<'info>, remaining: &[AccountInfo<'info>],
    args: &RefundFundedInterestArgs, operation_id: [u8; 32], digest: [u8; 32], amount: u64,
    source: &FinanceReceiptData, lender: &CashLeg, protocol: &CashLeg,
    usage_before: RepaymentRefundUsage, usage_after: RepaymentRefundUsage, bump: u8) -> Result<()> {
    // These are gross authenticated cash movements. Collected income and
    // original debt repayments are not erased or converted into new capacity.
    let mut changes = vec![inc(source.debit_principal, CASH_RECEIVED, amount)];
    if args.lender_interest_refund > 0 {
        changes.push(inc(lender.principal, CASH_SENT, args.lender_interest_refund));
    }
    if args.protocol_interest_refund > 0 {
        changes.push(inc(protocol.principal, CASH_SENT, args.protocol_interest_refund));
    }
    let mut record = record_base(a.principal.key(), operation_id, digest, FINANCE_INTEREST_REFUND,
        a.mint.key(), a.claim.key(), a.run.key(), amount);
    record.related_receipt = a.interest_refund.key();
    journal(&mut a.receipt, record, &changes, remaining, &a.pool_program.to_account_info(),
        args.service_without_pool, false)?;
    a.interest_refund.set_inner(InterestRefundReceipt {
        version: CORRECTION_VERSION, bump,
        company: a.principal.key(), claim: a.claim.key(), run: a.run.key(),
        operation_id, request_digest: digest, request: args.clone(),
        original_repayment: a.original_repayment.key(), original_request_digest: source.request_digest,
        original_effects_hash: source.effects_hash, original_principal_paid: source.principal_paid,
        original_interest_paid: source.interest_paid, original_protocol_interest: source.protocol_interest,
        capture_receipt: a.run.capture_receipt, capture_digest: a.run.capture_digest,
        acceptance_hash: a.claim.acceptance_hash,
        historical_lender: source.current_creditor, historical_lender_destination: source.creditor_destination,
        protocol_beneficiary: a.run.interest_terms.protocol_beneficiary,
        frozen_protocol_destination: a.run.interest_terms.protocol_destination,
        lender_authority: lender.authority, lender_from: lender.from,
        protocol_authority: protocol.authority, protocol_from: protocol.from,
        refund_principal: source.debit_principal, refund_authority: source.debit_authority,
        refund_destination: a.refund_destination.key(), repayment_usage: a.repayment_usage.key(),
        usage_before, usage_after, financial_receipt: a.receipt.key(),
        financial_effect_hash: a.receipt.data.effects_hash, result_hash: [0; 32],
    });
    a.interest_refund.result_hash = a.interest_refund.hash()?;
    a.interest_refund.check(a.interest_refund.key())?;
    Ok(())
}
