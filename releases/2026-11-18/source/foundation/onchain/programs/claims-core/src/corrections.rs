//! Negotiated bill corrections preserve delivery and historical cash lineage.
//! OwnerCash funding is explicit and voluntary; it creates no hidden claim.
use anchor_lang::prelude::*;
use anchor_spl::{associated_token, token::{self, Mint, Token, TokenAccount}};
use solana_sha256_hasher::hashv;
use dexter_contracts::{plus, minus, CORE_ID, POOL_ID, REGISTRY_CONSUMER_SEED};
use dexter_contracts::corrections::{CORRECTION_VERSION, CorrectionAmounts, CorrectionPlan,
    CollectionCorrectionUsage, RepaymentRefundUsage};
use dexter_contracts::financing::PrincipalAmounts;
use dexter_contracts::funded::*;
use principal_registry::{Principal, Budget, Reservation, program::PrincipalRegistry};
use crate::{CoreError, funding_state::*, funding_journal::{journal, inc, dec},
    historical_refunds::{validate_refund_destination, credit_refund_custody},
    receivables::{registry_service, interest_accrual_operation_id, interest_release_operation_id}};

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
    pub expected_claim_revision: u64,
    pub expected_ownership_revision: u64,
    pub company_owner_generation: u64,
    pub supplier_owner_generation: u64,
    pub creditor_owner_generation: u64,
    pub funding_owner_generation: u64,
    pub service_without_pool: bool,
}

#[account]
#[derive(InitSpace)]
pub struct CollectionCorrectionRecord {
    pub version: u16, pub bump: u8,
    pub source_receipt: Pubkey, pub claim: Pubkey, pub run: Pubkey,
    pub usage: CollectionCorrectionUsage,
}
impl CollectionCorrectionRecord {
    pub fn check(&self, key: Pubkey, source: Pubkey, claim: Pubkey, run: Pubkey) -> Result<()> {
        let (expected,bump)=Pubkey::find_program_address(&[b"correction-collection",source.as_ref()],&CORE_ID);
        require!(self.version==CORRECTION_VERSION && self.bump==bump && key==expected
            && self.source_receipt==source && self.claim==claim && self.run==run,CoreError::FundingCorrectionLineage);
        Ok(())
    }
    pub fn initialize_or_check(&mut self, key: Pubkey, source: Pubkey, claim: Pubkey, run: Pubkey) -> Result<()> {
        if self.version==0 {
            let (_,bump)=Pubkey::find_program_address(&[b"correction-collection",source.as_ref()],&CORE_ID);
            *self=Self{version:CORRECTION_VERSION,bump,source_receipt:source,claim,run,usage:CollectionCorrectionUsage::default()};
        }
        self.check(key,source,claim,run)
    }
}

/// Principal and both interest refund capacities share one original repayment
/// identity. A later interest-refund handler must use this same usage account.
#[account]
#[derive(InitSpace)]
pub struct RepaymentRefundRecord {
    pub version: u16, pub bump: u8,
    pub source_receipt: Pubkey, pub claim: Pubkey, pub run: Pubkey,
    pub usage: RepaymentRefundUsage,
}
impl RepaymentRefundRecord {
    pub fn check(&self, key: Pubkey, source: Pubkey, claim: Pubkey, run: Pubkey) -> Result<()> {
        let (expected,bump)=Pubkey::find_program_address(&[b"correction-repayment",source.as_ref()],&CORE_ID);
        require!(self.version==CORRECTION_VERSION && self.bump==bump && key==expected
            && self.source_receipt==source && self.claim==claim && self.run==run,CoreError::FundingCorrectionLineage);
        Ok(())
    }
    pub fn initialize_or_check(&mut self, key: Pubkey, source: Pubkey, claim: Pubkey, run: Pubkey) -> Result<()> {
        if self.version==0 {
            let (_,bump)=Pubkey::find_program_address(&[b"correction-repayment",source.as_ref()],&CORE_ID);
            *self=Self{version:CORRECTION_VERSION,bump,source_receipt:source,claim,run,usage:RepaymentRefundUsage::default()};
        }
        self.check(key,source,claim,run)
    }
}

/// Fixed-size immutable result supplements the unchanged financial-journal ABI.
/// The journal commits this account's address; this result commits its effect
/// hash. Neither record requires rewriting an original capture or payment.
#[account]
#[derive(InitSpace)]
pub struct CorrectionReceipt {
    pub version: u16, pub bump: u8,
    pub company: Pubkey, pub claim: Pubkey, pub run: Pubkey,
    pub operation_id: [u8; 32], pub request_digest: [u8; 32],
    pub request: CorrectFinancedRunArgs,
    pub capture_receipt: Pubkey, pub capture_digest: [u8; 32], pub acceptance_hash: [u8; 32],
    pub supplier_principal: Pubkey, pub frozen_supplier_wallet: Pubkey,
    pub company_owner: Pubkey, pub supplier_owner: Pubkey, pub creditor_owner: Pubkey,
    pub current_creditor: Pubkey, pub creditor_custody: Pubkey, pub creditor_destination: Pubkey,
    pub funding_principal: Pubkey, pub funding_authority: Pubkey, pub funding_from: Pubkey,
    pub collection_receipt: Pubkey, pub repayment_receipt: Pubkey,
    pub refund_principal: Pubkey, pub refund_authority: Pubkey, pub refund_destination: Pubkey,
    pub plan: CorrectionPlan,
    pub principal_before: PrincipalAmounts, pub principal_after: PrincipalAmounts,
    pub corrections_before: CorrectionAmounts, pub corrections_after: CorrectionAmounts,
    pub newly_accrued_interest: u64, pub interest_waived: u64, pub unused_charge_authority_released: u64,
    pub resulting_claim_revision: u64,
    pub financial_receipt: Pubkey, pub financial_effect_hash: [u8; 32],
    pub result_hash: [u8; 32],
}
impl CorrectionReceipt {
    #[inline(never)]
    pub fn hash(&self) -> Result<[u8; 32]> {
        // Every field is fixed-size. A bounded stack buffer preserves exactly
        // the original Borsh hash preimage without consuming the bump heap.
        let mut bytes=[0u8;Self::INIT_SPACE];
        let mut cursor=&mut bytes[..];
        self.serialize(&mut cursor)?;
        require!(cursor.is_empty(),CoreError::FundingCorrectionReplay);
        bytes[Self::INIT_SPACE-32..].fill(0);
        Ok(hashv(&[b"DEXTER:FUNDED-CORRECTION-RESULT:1",CORE_ID.as_ref(),&bytes]).to_bytes())
    }
    pub fn check(&self, key: Pubkey) -> Result<()> {
        let (expected,bump)=Pubkey::find_program_address(&[b"correction-result",self.run.as_ref(),&self.operation_id],&CORE_ID);
        require!(self.version==CORRECTION_VERSION && key==expected && self.bump==bump
            && self.operation_id==correction_operation_id(self.run,&self.request.request_id)
            && self.result_hash==self.hash()?,CoreError::FundingCorrectionReplay);
        Ok(())
    }
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
    /// CHECK: Registry authenticates its own interest-accrual operation PDA.
    #[account(mut)] pub registry_accrual_operation: UncheckedAccount<'info>,
    /// CHECK: Independent Registry domain for genuinely unused charge release.
    #[account(mut)] pub registry_release_operation: UncheckedAccount<'info>,
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
    /// CHECK: Full immutable original collection receipt verified by Core reader.
    pub original_collection: Option<UncheckedAccount<'info>>,
    #[account(init_if_needed,payer=payer,space=8+CollectionCorrectionRecord::INIT_SPACE,
        seeds=[b"correction-collection",original_collection.as_ref().map(|a|a.key()).unwrap_or_default().as_ref()],bump)]
    pub collection_usage: Option<Box<Account<'info, CollectionCorrectionRecord>>>,
    /// CHECK: Full immutable original repayment receipt verified by Core reader.
    pub original_repayment: Option<UncheckedAccount<'info>>,
    #[account(init_if_needed,payer=payer,space=8+RepaymentRefundRecord::INIT_SPACE,
        seeds=[b"correction-repayment",original_repayment.as_ref().map(|a|a.key()).unwrap_or_default().as_ref()],bump)]
    pub repayment_usage: Option<Box<Account<'info, RepaymentRefundRecord>>>,
    /// CHECK: Exact original payer source and owner from the repayment receipt.
    #[account(mut,dup)] pub refund_destination: Option<UncheckedAccount<'info>>,
    pub mint: Box<Account<'info, Mint>>,
    #[account(init_if_needed,payer=payer,space=8+FinanceReceipt::INIT_SPACE,
        seeds=[b"finance-operation",principal.key().as_ref(),&correction_operation_id(run.key(),&args.request_id)],bump)]
    pub receipt: Box<Account<'info, FinanceReceipt>>,
    #[account(init_if_needed,payer=payer,space=8+CorrectionReceipt::INIT_SPACE,
        seeds=[b"correction-result",run.key().as_ref(),&correction_operation_id(run.key(),&args.request_id)],bump)]
    pub correction: Box<Account<'info, CorrectionReceipt>>,
    /// CHECK: Accepted servicing can preserve Core effects without Pool CPI.
    #[account(address=POOL_ID)] pub pool_program: UncheckedAccount<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    /// CHECK: Exact original governed payer custody, authenticated from its
    /// repayment receipt; may alias the typed source or unchecked creditor.
    #[account(mut,dup)] pub refund_custody: Option<UncheckedAccount<'info>>,
}

fn read_token(info: &AccountInfo, mint: Pubkey) -> Result<TokenAccount> {
    require_keys_eq!(*info.owner,token::ID,CoreError::FundingTokenRouting);
    let value=TokenAccount::try_deserialize(&mut &info.try_borrow_data()?[..])?;
    require_keys_eq!(value.mint,mint,CoreError::FundingTokenRouting); Ok(value)
}

/// Source receipts stay immutable. Their per-receipt usage is the independent
/// capacity guard against using another correction ID or a later run payment.
pub fn source_receipt(info: &AccountInfo, kind: u8, company: Pubkey,
    claim: Pubkey, run: Pubkey, mint: Pubkey) -> Result<FinanceReceiptData> {
    let receipt=read_finance_receipt(info)?;
    require!(receipt.kind==kind && receipt.principal==company && receipt.claim==claim
        && receipt.run==run && receipt.mint==mint,CoreError::FundingCorrectionLineage);
    Ok(receipt)
}

// Keep immutable-source parsing in separate SBF frames. Each full receipt has
// a large fixed header as well as its effects vector; it need not live beside
// the complete correction result on the handler's 4096-byte stack frame.
#[inline(never)]
fn record_collection_return(a: &mut CorrectFinancedRun, amount: u64) -> Result<()> {
    if amount==0 {
        require!(a.original_collection.is_none() && a.collection_usage.is_none(),CoreError::FundingCorrectionLineage);
        return Ok(());
    }
    let original=a.original_collection.as_ref().ok_or_else(||error!(CoreError::FundingCorrectionLineage))?;
    let source=source_receipt(&original.to_account_info(),FINANCE_COLLECT,a.principal.key(),a.claim.key(),a.run.key(),a.mint.key())?;
    require!(plus(source.supplier_net,source.seller_fee)?==source.amount,CoreError::FundingCorrectionLineage);
    let usage=a.collection_usage.as_deref_mut().ok_or_else(||error!(CoreError::FundingCorrectionLineage))?;
    let usage_key=usage.key();
    usage.initialize_or_check(usage_key,original.key(),a.claim.key(),a.run.key())?;
    usage.usage=math(usage.usage.record(amount,source.amount))?;
    Ok(())
}

#[inline(never)]
fn record_principal_refund(a: &mut CorrectFinancedRun, amount: u64) -> Result<(Pubkey,Pubkey)> {
    if amount==0 {
        require!(a.original_repayment.is_none() && a.repayment_usage.is_none()
            && a.refund_destination.is_none() && a.refund_custody.is_none(),CoreError::FundingCorrectionLineage);
        return Ok((Pubkey::default(),Pubkey::default()));
    }
    let original=a.original_repayment.as_ref().ok_or_else(||error!(CoreError::FundingCorrectionLineage))?;
    let source=source_receipt(&original.to_account_info(),FINANCE_REPAY,a.principal.key(),a.claim.key(),a.run.key(),a.mint.key())?;
    let dest=a.refund_destination.as_ref().ok_or_else(||error!(CoreError::FundingTokenRouting))?;
    validate_refund_destination(&source,&dest.to_account_info(),a.mint.key(),a.refund_custody.as_ref())?;
    let usage=a.repayment_usage.as_deref_mut().ok_or_else(||error!(CoreError::FundingCorrectionLineage))?;
    let usage_key=usage.key();
    usage.initialize_or_check(usage_key,original.key(),a.claim.key(),a.run.key())?;
    usage.usage=math(usage.usage.record(amount,0,0,source.principal_paid,source.interest_paid,source.protocol_interest))?;
    Ok((source.debit_principal,source.debit_authority))
}

#[inline(never)]
fn request_digest(a: &CorrectFinancedRun, args: &CorrectFinancedRunArgs) -> Result<[u8;32]> {
    let mut economics=args.clone(); economics.service_without_pool=false;
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
            a.original_collection.as_ref().map(|x|x.key()),a.original_repayment.as_ref().map(|x|x.key()),a.refund_destination.as_ref().map(|x|x.key()))))
}

// Current fixed args, eleven fixed public keys and ten optional public keys.
// Hash only the consumed prefix: absent optional routes occupy one Borsh byte.
const MAX_REQUEST_PREIMAGE: usize = CorrectFinancedRunArgs::INIT_SPACE + 11*32 + 10*33;
#[inline(never)]
fn bounded_request_hash<T: AnchorSerialize>(preimage: &T) -> Result<[u8;32]> {
    let mut bytes=[0u8;MAX_REQUEST_PREIMAGE];
    let remaining={
        let mut output=&mut bytes[..];
        preimage.serialize(&mut output)?;
        output.len()
    };
    Ok(hashv(&[b"DEXTER:FUNDED-CORRECTION:1",CORE_ID.as_ref(),
        &bytes[..MAX_REQUEST_PREIMAGE-remaining]]).to_bytes())
}

pub fn correct<'info>(ctx: Context<'info, CorrectFinancedRun<'info>>, args: CorrectFinancedRunArgs) -> Result<()> {
    let a=ctx.accounts;
    let op=correction_operation_id(a.run.key(),&args.request_id);
    let digest=request_digest(a,&args)?;
    if replay(&a.receipt,&digest)? {
        a.correction.check(a.correction.key())?;
        require!(a.correction.request_digest==digest && a.correction.financial_receipt==a.receipt.key()
            && a.correction.financial_effect_hash==a.receipt.data.effects_hash
            && a.receipt.data.kind==FINANCE_CORRECT && a.receipt.data.related_receipt==a.correction.key(),CoreError::FundingCorrectionReplay);
        return Ok(());
    }
    require!(a.correction.version==0 && args.reason_hash!=[0;32],CoreError::FundingCorrection);
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
    require!(a.claim.revision==args.expected_claim_revision && a.run.ownership_revision==args.expected_ownership_revision,
        CoreError::FundingRevision);
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

    record_collection_return(a,args.collected_return)?;
    let (refund_principal,refund_authority)=record_principal_refund(a,plan.prior_repayment_refund)?;

    let mut creditor_copy=None;
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
        creditor_copy=Some(c);
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
    registry_service(&a.principal,&a.budget,&a.reservation,&a.registry_authority,&a.payer,
        &a.registry_program,&a.system_program,&a.registry_accrual_operation,
        interest_accrual_operation_id(&op,&a.run.key())?,new_interest,false,args.expected_claim_revision,args.service_without_pool)?;
    registry_service(&a.principal,&a.budget,&a.reservation,&a.registry_authority,&a.payer,
        &a.registry_program,&a.system_program,&a.registry_release_operation,
        interest_release_operation_id(&op,&a.run.key())?,unused_charge,true,args.expected_claim_revision,args.service_without_pool)?;

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
    a.claim.revision=plus(a.claim.revision,1)?;
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
    record.related_receipt=a.correction.key(); record.debit_principal=funder;
    record.debit_authority=funding_authority; record.debit_from=funding_from;
    record.current_creditor=creditor; record.ownership_revision=a.run.ownership_revision;
    record.creditor_destination=a.creditor_destination.as_ref().map(|x|x.key()).unwrap_or_default();
    journal(&mut a.receipt,record,&changes,ctx.remaining_accounts,&a.pool_program,args.service_without_pool,false)?;
    a.correction.set_inner(CorrectionReceipt{version:CORRECTION_VERSION,bump:ctx.bumps.correction,
        company,claim:a.claim.key(),run:a.run.key(),operation_id:op,request_digest:digest,request:args.clone(),
        capture_receipt:a.run.capture_receipt,capture_digest:a.run.capture_digest,acceptance_hash:a.claim.acceptance_hash,
        supplier_principal:a.claim.supplier_principal,frozen_supplier_wallet:a.claim.terms.supplier,
        company_owner:a.company_owner.key(),supplier_owner:a.supplier_owner.as_ref().map(|x|x.key()).unwrap_or_default(),
        creditor_owner:a.creditor_owner.as_ref().map(|x|x.key()).unwrap_or_default(),
        current_creditor:creditor,creditor_custody:a.run.creditor_custody,
        creditor_destination:a.creditor_destination.as_ref().map(|x|x.key()).unwrap_or_default(),
        funding_principal:funder,funding_authority,funding_from,
        collection_receipt:a.original_collection.as_ref().map(|x|x.key()).unwrap_or_default(),
        repayment_receipt:a.original_repayment.as_ref().map(|x|x.key()).unwrap_or_default(),
        refund_principal,refund_authority,refund_destination:a.refund_destination.as_ref().map(|x|x.key()).unwrap_or_default(),
        plan,principal_before,principal_after,corrections_before:before,corrections_after:after,
        newly_accrued_interest:new_interest,interest_waived:args.interest_waiver,unused_charge_authority_released:unused_charge,
        resulting_claim_revision:a.claim.revision,financial_receipt:a.receipt.key(),financial_effect_hash:a.receipt.data.effects_hash,
        result_hash:[0;32]});
    a.correction.result_hash=a.correction.hash()?;
    // Seeds were checked by Anchor and every field was constructed above.
    // Do not allocate a second serialized hash buffer merely to re-read our
    // own result. Historical recovery authenticates the immutable result in
    // CorrectionReceipt::check, and independent readers verify this hash.
    Ok(())
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
                (optional,optional,optional,optional,optional,optional,optional,optional));
            let actual=bounded_request_hash(&preimage).unwrap();
            let original=financial_digest(b"DEXTER:FUNDED-CORRECTION:1",&preimage).unwrap();
            assert_eq!(actual,original);
            let mut serialized=Vec::new(); preimage.serialize(&mut serialized).unwrap();
            assert_eq!(serialized.len(),MAX_REQUEST_PREIMAGE-if optional.is_none(){10*32}else{0});
        }
    }

    #[test]
    fn bounded_result_hash_preserves_the_original_borsh_preimage() {
        let bytes=vec![0u8;8+CorrectionReceipt::INIT_SPACE];
        let mut result=CorrectionReceipt::try_deserialize_unchecked(&mut &bytes[..]).unwrap();
        result.version=CORRECTION_VERSION;
        result.company=Pubkey::new_from_array([11;32]);
        result.request.request_id=[12;32];
        result.request.reason_hash=[13;32];
        result.request.collected_return=17;
        result.request.service_without_pool=true;
        result.plan.total=17;
        result.financial_effect_hash=[14;32];
        result.result_hash=[15;32];
        let actual=result.hash().unwrap();
        result.result_hash=[0;32];
        let original=financial_digest(b"DEXTER:FUNDED-CORRECTION-RESULT:1",&result).unwrap();
        assert_eq!(actual,original);
    }
}
