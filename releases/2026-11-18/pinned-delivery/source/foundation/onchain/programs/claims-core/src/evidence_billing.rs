//! Authenticated, finite-lot billing. Source trust is explicit; calculation,
//! deadlines, preserved backing and independent finalization are native rules.
use anchor_lang::prelude::*;
use anchor_lang::{compat::solana_instructions_sysvar as instructions, system_program};
use anchor_spl::token::{Mint, TokenAccount};
use dexter_contracts::{self as contracts, TabTerms, CORE_ID, POOL_ID, REGISTRY_CONSUMER_SEED};
use dexter_contracts::billing::{BillingPolicy, SourceReport, SellerClaim, BILLING_POLICY_VERSION,
    BILLING_NAMESPACE, BILLING_SETTLEMENT_UNIT, billing_policy_digest, source_report_digest,
    seller_claim_digest, deterministic_bill_amount};
use dexter_contracts::commercial::SellerFeeTerms;
use dexter_contracts::funded::financial_digest;
use principal_registry::{Principal, Budget, Reservation, Term, program::PrincipalRegistry};
use crate::{CoreError, funding::{self, AcceptFinancedTab, AcceptFinancedTabBumps, OrderTerms,
    __client_accounts_accept_financed_tab, __cpi_client_accounts_accept_financed_tab},
    funding_state::*, funding_servicing::*};

pub const EVIDENCE_BILL_SEED: &[u8] = b"evidence-bill";
pub const ED25519_ID: Pubkey = pubkey!("Ed25519SigVerify111111111111111111111111111");

pub fn bill_payment_id(bill: &Pubkey) -> [u8;32] {
    solana_sha256_hasher::hashv(&[b"DEXTER:BILL-PAYMENT:1", CORE_ID.as_ref(), bill.as_ref()]).to_bytes()
}
pub fn bill_release_id(bill: &Pubkey) -> [u8;32] {
    solana_sha256_hasher::hashv(&[b"DEXTER:BILL-RELEASE:1", CORE_ID.as_ref(), bill.as_ref()]).to_bytes()
}

#[account]
#[derive(InitSpace)]
pub struct EvidenceBill {
    pub version: u8, pub bump: u8, pub domain: [u8;32], pub principal: Pubkey,
    pub supplier_principal: Pubkey, pub financial_claim: Pubkey, pub supplier_claimant: Pubkey,
    pub acceptance_receipt: Pubkey, pub acceptance_digest: [u8;32],
    pub policy: BillingPolicy, pub policy_hash: [u8;32], pub admitted_at: i64,
    pub source_report: Option<SourceReport>, pub source_digest: [u8;32],
    pub evidence_admitted_at: i64, pub supported_amount: u128,
    pub seller_claim: Option<SellerClaim>, pub seller_digest: [u8;32], pub claim_admitted_at: i64,
    pub finalized: bool, pub finalized_at: i64, pub established_amount: u64, pub released_amount: u64,
    pub earned_receipt: Pubkey, pub released_receipt: Pubkey,
}

impl EvidenceBill {
    pub fn check(&self, key: Pubkey) -> Result<()> {
        let (expected, bump) = Pubkey::find_program_address(
            &[EVIDENCE_BILL_SEED, self.supplier_principal.as_ref(), &self.policy.work_id], &CORE_ID);
        require!(self.version == BILLING_POLICY_VERSION && key == expected && self.bump == bump,
            CoreError::BillingIdentity);
        require!(self.principal != Pubkey::default() && self.financial_claim != Pubkey::default()
            && self.supplier_claimant != Pubkey::default(), CoreError::BillingIdentity);
        self.policy.validate(self.admitted_at).map_err(|_| error!(CoreError::BillingPolicy))?;
        let digest = billing_policy_digest(&self.domain, &key, &self.financial_claim, &self.policy)
            .map_err(|_| error!(CoreError::BillingPolicy))?;
        require!(digest == self.policy_hash, CoreError::BillingPolicy);
        Ok(())
    }
}

#[derive(Accounts)]
pub struct AcceptEvidenceFinancedTab<'info> {
    pub funding: AcceptFinancedTab<'info>,
    /// CHECK: Canonical per-supplier work PDA, allocated and initialized only
    /// after the existing financed admission succeeds in this same transaction.
    #[account(mut)] pub bill: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct SubmitBillRecord<'info> {
    pub payer: Signer<'info>,
    #[account(mut)] pub bill: Box<Account<'info, EvidenceBill>>,
    /// CHECK: The native instructions sysvar authenticates the exact prior
    /// Ed25519 precompile and its inline key/message/signature framing.
    #[account(address=instructions::ID)] pub instructions: UncheckedAccount<'info>,
}

/// The original external-key profile never adopts a successor proof key or
/// payment route. A recovered supplier also fences fresh use of its old key;
/// already admitted records remain historical rights and replay unchanged.
#[derive(Accounts)]
pub struct SubmitFixedBillClaim<'info> {
    pub payer: Signer<'info>,
    #[account(mut)] pub bill: Box<Account<'info, EvidenceBill>>,
    pub supplier_principal: Box<Account<'info, Principal>>,
    /// CHECK: Strict native Ed25519 verification below.
    #[account(address=instructions::ID)] pub instructions: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct FinalizeEvidenceBill<'info> {
    #[account(mut)] pub payer: Signer<'info>,
    #[account(mut)] pub bill: Box<Account<'info, EvidenceBill>>,
    pub principal: Box<Account<'info, Principal>>,
    #[account(mut)] pub budget: Box<Account<'info, Budget>>,
    #[account(mut)] pub reservation: Box<Account<'info, Reservation>>,
    /// CHECK: Registry authenticates the canonical completed-run operation.
    #[account(mut)] pub earn_registry_operation: UncheckedAccount<'info>,
    /// CHECK: Registry authenticates the canonical residual release operation.
    #[account(mut)] pub release_registry_operation: UncheckedAccount<'info>,
    /// CHECK: Existing Core-to-Registry consumer signer.
    #[account(seeds=[REGISTRY_CONSUMER_SEED],bump)] pub registry_authority: UncheckedAccount<'info>,
    pub registry_program: Program<'info, PrincipalRegistry>,
    #[account(mut)] pub line: Box<Account<'info, CustomerCreditLine>>,
    #[account(mut)] pub claim: Box<Account<'info, FundedClaim>>,
    #[account(mut)] pub facility: Box<Account<'info, FundingFacility>>,
    #[account(mut)] pub custody: Box<Account<'info, FinancierCustody>>,
    pub custody_token: Box<Account<'info, TokenAccount>>,
    #[account(init_if_needed,payer=payer,space=8+FundedRun::INIT_SPACE,
        seeds=[b"usage",bill.supplier_principal.as_ref(),&BILLING_NAMESPACE,&bill.policy.work_id],bump)]
    pub run: Box<Account<'info, FundedRun>>,
    #[account(init_if_needed,payer=payer,space=8+FundedPayment::INIT_SPACE,
        seeds=[b"run-payment",principal.key().as_ref(),&bill_payment_id(&bill.key())],bump)]
    pub payment: Box<Account<'info, FundedPayment>>,
    #[account(init_if_needed,payer=payer,space=8+FinanceReceipt::INIT_SPACE,
        seeds=[b"finance-operation",principal.key().as_ref(),
            &funded_run_operation_id(&bill.supplier_principal,&BILLING_NAMESPACE,&bill.policy.work_id)],bump)]
    pub earn_receipt: Box<Account<'info, FinanceReceipt>>,
    #[account(init_if_needed,payer=payer,space=8+FinanceReceipt::INIT_SPACE,
        seeds=[b"finance-operation",principal.key().as_ref(),&bill_release_id(&bill.key())],bump)]
    pub release_receipt: Box<Account<'info, FinanceReceipt>>,
    pub mint: Box<Account<'info, Mint>>,
    /// CHECK: Existing accepted-servicing projection boundary.
    #[account(address=POOL_ID)] pub pool_program: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

/// The existing entrypoint never accepts this reserved namespace. Only this
/// instruction can pair it with an authenticated, immutable policy account.
/// `order_terms`, when shown, hashes to `policy.request_hash`.
#[allow(clippy::too_many_arguments)]
pub fn accept_evidence_policy<'info>(ctx: Context<'info, AcceptEvidenceFinancedTab<'info>>,
    request_id: [u8;32], terms: TabTerms, policy: BillingPolicy, maximum_interest: u64,
    seller_fee_terms: SellerFeeTerms, assignment_policy_hash: [u8;32], order_terms: Option<Vec<Term>>,
) -> Result<()> {
    accept_policy_bound(ctx.accounts, ctx.bumps, ctx.remaining_accounts, request_id, terms, policy,
        maximum_interest, seller_fee_terms, assignment_policy_hash, None, order_terms)
}

/// Only the recovery-profile wrapper may supply a continuing identity and
/// payee after authenticating its canonical immutable authority/custody graph.
#[allow(clippy::too_many_arguments)]
pub(crate) fn accept_policy_bound<'info>(a: &mut AcceptEvidenceFinancedTab<'info>,
    bumps: AcceptEvidenceFinancedTabBumps, remaining_accounts: &[AccountInfo<'info>],
    request_id: [u8;32], terms: TabTerms, policy: BillingPolicy, maximum_interest: u64,
    seller_fee_terms: SellerFeeTerms, assignment_policy_hash: [u8;32],
    continuing_payee: Option<Pubkey>, order_terms: Option<Vec<Term>>,
) -> Result<()> {
    let supplier_principal = a.funding.supplier_principal.key();
    let (bill_key, bump) = Pubkey::find_program_address(
        &[EVIDENCE_BILL_SEED, supplier_principal.as_ref(), &policy.work_id], &CORE_ID);
    require_keys_eq!(a.bill.key(), bill_key, CoreError::BillingIdentity);
    let claim_key = a.funding.claim.key();
    let policy_hash = billing_policy_digest(&terms.domain, &bill_key, &claim_key, &policy)
        .map_err(|_| error!(CoreError::BillingPolicy))?;
    require!(terms.expires_at == policy.work_until, CoreError::BillingPolicy);
    require!(terms.external_terms_hash == contracts::completed_run_policy(
        &BILLING_NAMESPACE, &BILLING_SETTLEMENT_UNIT, 1, &policy_hash), CoreError::BillingPolicy);

    let existing = if *a.bill.to_account_info().owner == CORE_ID {
        let info = a.bill.to_account_info();
        require!(info.data_len() == 8 + EvidenceBill::INIT_SPACE, CoreError::BillingIdentity);
        let data = info.try_borrow_data()?;
        let bill = EvidenceBill::try_deserialize(&mut data.as_ref())?;
        bill.check(bill_key)?;
        require!(bill.financial_claim == claim_key && bill.policy == policy && bill.policy_hash == policy_hash
            && bill.principal == a.funding.principal.key()
            && bill.supplier_principal == supplier_principal
            && bill.acceptance_receipt == a.funding.receipt.key()
            && bill.acceptance_digest == a.funding.claim.acceptance_hash,
            CoreError::BillingReplay);
        true
    } else {
        require!(*a.bill.to_account_info().owner == system_program::ID && a.bill.data_is_empty()
            && a.funding.claim.version == 0, CoreError::BillingIdentity);
        policy.validate(Clock::get()?.unix_timestamp).map_err(|_| error!(CoreError::BillingPolicy))?;
        if continuing_payee.is_none() {
            require!(policy.source.is_on_curve() && a.funding.supplier.key().is_on_curve(), CoreError::BillingPolicy);
        }
        false
    };
    // Native account admission and all affected reserves/books are reused.
    // Exact stored retries reach its existing receipt-first recovery path.
    let supplier_claimant = if continuing_payee.is_some() { supplier_principal } else { a.funding.supplier.key() };
    let payee = continuing_payee.unwrap_or(a.funding.supplier.key());
    let order_terms = match order_terms {
        None => OrderTerms::Hidden,
        Some(terms) => OrderTerms::Billing { bill: bill_key, policy: Box::new(policy.clone()), terms },
    };
    funding::accept_bound_payee(&mut a.funding, bumps.funding, remaining_accounts,
        request_id, terms, maximum_interest, seller_fee_terms,
        assignment_policy_hash, BILLING_NAMESPACE, BILLING_SETTLEMENT_UNIT, 1, policy_hash, payee, order_terms)?;
    if existing { return Ok(()); }

    let now = Clock::get()?.unix_timestamp;
    let bill = EvidenceBill { version:BILLING_POLICY_VERSION, bump, domain:a.funding.claim.terms.domain,
        principal:a.funding.principal.key(), supplier_principal, financial_claim:claim_key,
        supplier_claimant, acceptance_receipt:a.funding.receipt.key(),
        acceptance_digest:a.funding.claim.acceptance_hash, policy, policy_hash, admitted_at:now,
        source_report:None, source_digest:[0;32], evidence_admitted_at:0, supported_amount:0,
        seller_claim:None, seller_digest:[0;32], claim_admitted_at:0, finalized:false, finalized_at:0,
        established_amount:0, released_amount:0, earned_receipt:Pubkey::default(), released_receipt:Pubkey::default() };
    bill.check(bill_key)?;
    let bump_seed = [bump];
    let seeds: &[&[u8]] = &[EVIDENCE_BILL_SEED, supplier_principal.as_ref(), &bill.policy.work_id, &bump_seed];
    // Legitimate prefunding is tolerated; it cannot reset or prepopulate state.
    let info = a.bill.to_account_info();
    let space = 8 + EvidenceBill::INIT_SPACE;
    let rent = Rent::get()?.minimum_balance(space);
    if info.lamports() < rent {
        system_program::transfer(CpiContext::new(a.funding.system_program.key(), system_program::Transfer {
            from:a.funding.payer.to_account_info(), to:info.clone(),
        }), rent - info.lamports())?;
    }
    system_program::allocate(CpiContext::new_with_signer(a.funding.system_program.key(), system_program::Allocate {
        account_to_allocate:info.clone(),
    }, &[seeds]), space as u64)?;
    system_program::assign(CpiContext::new_with_signer(a.funding.system_program.key(), system_program::Assign {
        account_to_assign:info.clone(),
    }, &[seeds]), &CORE_ID)?;
    bill.try_serialize(&mut &mut info.try_borrow_mut_data()?[..])?;
    Ok(())
}

pub(crate) fn verify_signature(sysvar: &AccountInfo, index: u16, key: &Pubkey, digest: &[u8;32]) -> Result<()> {
    let info = sysvar.to_account_info();
    let current = instructions::load_current_index_checked(&info).map_err(|_| error!(CoreError::BillingSignature))?;
    require!(index < current, CoreError::BillingSignature);
    let ix = instructions::load_instruction_at_checked(index as usize, &info)
        .map_err(|_| error!(CoreError::BillingSignature))?;
    require!(ix.program_id == ED25519_ID && ix.accounts.is_empty() && ix.data.len() == 144,
        CoreError::BillingSignature);
    let d = &ix.data;
    let word = |n:usize| u16::from_le_bytes([d[n],d[n+1]]);
    require!(d[0] == 1 && d[1] == 0 && word(2) == 16 && word(4) == u16::MAX
        && word(6) == 80 && word(8) == u16::MAX && word(10) == 112
        && word(12) == 32 && word(14) == u16::MAX
        && &d[80..112] == key.as_ref() && &d[112..144] == digest,
        CoreError::BillingSignature);
    Ok(())
}

pub fn submit_evidence(ctx: Context<SubmitBillRecord>, report: SourceReport, signature_ix_index: u16) -> Result<()> {
    let a = ctx.accounts; let key = a.bill.key(); a.bill.check(key)?;
    let digest = source_report_digest(&a.bill.domain, &key, &a.bill.financial_claim, &a.bill.policy_hash, &report)
        .map_err(|_| error!(CoreError::BillingEvidence))?;
    if let Some(original) = &a.bill.source_report {
        require!(original == &report && a.bill.source_digest == digest, CoreError::BillingReplay);
        return Ok(()); // Never replace the original chain admission time.
    }
    let now = Clock::get()?.unix_timestamp;
    require!(now <= a.bill.policy.evidence_deadline, CoreError::BillingDeadline);
    require!(!a.bill.finalized, CoreError::BillingFinality);
    let supported = a.bill.policy.validate_report(&report, a.bill.admitted_at, now)
        .map_err(|_| error!(CoreError::BillingEvidence))?;
    verify_signature(&a.instructions, signature_ix_index, &a.bill.policy.source, &digest)?;
    a.bill.source_report = Some(report); a.bill.source_digest = digest;
    a.bill.evidence_admitted_at = now; a.bill.supported_amount = supported;
    Ok(())
}

pub fn submit_claim(ctx: Context<SubmitFixedBillClaim>, seller_claim: SellerClaim, signature_ix_index: u16) -> Result<()> {
    let a = ctx.accounts; let key = a.bill.key(); a.bill.check(key)?;
    require_keys_eq!(a.supplier_principal.key(), a.bill.supplier_principal, CoreError::BillingIdentity);
    let digest = seller_claim_digest(&a.bill.domain, &key, &a.bill.financial_claim, &a.bill.policy_hash, &seller_claim)
        .map_err(|_| error!(CoreError::BillingClaim))?;
    if let Some(original) = &a.bill.seller_claim {
        require!(original == &seller_claim && a.bill.seller_digest == digest, CoreError::BillingReplay);
        return Ok(());
    }
    valid_principal(&a.supplier_principal)?;
    require!(a.supplier_principal.domain == a.bill.domain
        && a.supplier_principal.owner == a.bill.supplier_claimant,
        CoreError::BillingAuthorityGeneration);
    let now = Clock::get()?.unix_timestamp;
    require!(now <= a.bill.policy.claim_deadline, CoreError::BillingDeadline);
    require!(!a.bill.finalized, CoreError::BillingFinality);
    a.bill.policy.validate_claim(&seller_claim, a.bill.admitted_at, now)
        .map_err(|_| error!(CoreError::BillingClaim))?;
    verify_signature(&a.instructions, signature_ix_index, &a.bill.supplier_claimant, &digest)?;
    a.bill.seller_claim = Some(seller_claim); a.bill.seller_digest = digest; a.bill.claim_admitted_at = now;
    Ok(())
}

pub fn finalize<'info>(ctx: Context<'info, FinalizeEvidenceBill<'info>>, expected_revision:u64,
    service_without_pool:bool) -> Result<()> {
    let a = ctx.accounts; let bill_key = a.bill.key(); a.bill.check(bill_key)?;
    a.claim.check(a.claim.key())?;
    require!(a.bill.financial_claim == a.claim.key() && a.bill.principal == a.principal.key()
        && a.facility.key() == a.claim.facility
        && a.bill.domain == a.claim.terms.domain && a.bill.supplier_principal == a.claim.supplier_principal
        && a.bill.acceptance_digest == a.claim.acceptance_hash
        && a.claim.usage_namespace == BILLING_NAMESPACE && a.claim.service_unit == BILLING_SETTLEMENT_UNIT
        && a.claim.unit_price == 1 && a.claim.external_contract_hash == a.bill.policy_hash,
        CoreError::BillingIdentity);
    if a.bill.finalized { return Ok(()); }
    let now = Clock::get()?.unix_timestamp;
    require!(a.bill.policy.finalization_ready(now), CoreError::BillingFinality);
    require!(a.claim.revision == expected_revision && a.claim.captured_total()? == 0
        && a.claim.released == 0 && a.claim.remaining == a.claim.terms.cap, CoreError::FundingRevision);
    let supported = match &a.bill.source_report {
        Some(report) => {
            let value = a.bill.policy.validate_report(report, a.bill.admitted_at, a.bill.evidence_admitted_at)
                .map_err(|_| error!(CoreError::BillingEvidence))?;
            require!(value == a.bill.supported_amount, CoreError::BillingEvidence); value
        },
        None => 0,
    };
    let claimed = match &a.bill.seller_claim {
        Some(claim) => {
            a.bill.policy.validate_claim(claim, a.bill.admitted_at, a.bill.claim_admitted_at)
                .map_err(|_| error!(CoreError::BillingClaim))?;
            claim.amount
        },
        None => 0,
    };
    let amount = deterministic_bill_amount(a.claim.terms.cap, claimed, supported);
    let capture_digest = financial_digest(b"DEXTER:EVIDENCE-BILL-FINALIZATION:1",
        &(bill_key,a.bill.policy_hash,a.bill.source_digest,a.bill.seller_digest,amount))?;
    if amount > 0 {
        establish_run(EarnedRunAccounts {
            payer:&a.payer,principal:&a.principal,budget:&mut a.budget,reservation:&mut a.reservation,
            registry_operation:&a.earn_registry_operation,registry_authority:&a.registry_authority,
            registry_program:&a.registry_program,line:&mut a.line,claim:&mut a.claim,
            custody:&mut a.custody,custody_token:&a.custody_token,run:&mut a.run,payment:&mut a.payment,
            receipt:&mut a.earn_receipt,mint:&a.mint,pool_program:&a.pool_program,system_program:&a.system_program,
        }, EarnedRunTerms { payment_id:bill_payment_id(&bill_key),namespace:BILLING_NAMESPACE,
            run_id:a.bill.policy.work_id,service_unit:BILLING_SETTLEMENT_UNIT,external_contract_hash:a.bill.policy_hash,
            units:amount,unit_price:1,expected_revision,service_without_pool,capture_digest,enforce_deadline:false },
            ctx.bumps.registry_authority,ctx.bumps.run,ctx.bumps.payment,ctx.remaining_accounts)?;
        // Registry owns these accounts. The earning CPI has advanced its
        // reservation revision and moved the consumed amount; release must
        // validate the new state, not this instruction's deserialized snapshot.
        a.budget.reload()?;
        a.reservation.reload()?;
    }
    let released = a.claim.remaining;
    if released > 0 {
        let revision = a.claim.revision;
        release_unused(ReleaseAccounts {
            payer:&a.payer,principal:&a.principal,budget:&mut a.budget,reservation:&mut a.reservation,
            registry_operation:&a.release_registry_operation,registry_authority:&a.registry_authority,
            registry_program:&a.registry_program,line:&mut a.line,claim:&mut a.claim,facility:&mut a.facility,
            custody:&mut a.custody,custody_token:&a.custody_token,receipt:&mut a.release_receipt,
            mint:&a.mint,pool_program:&a.pool_program,system_program:&a.system_program,
        },bill_release_id(&bill_key),released,revision,service_without_pool,
            ctx.bumps.registry_authority,ctx.remaining_accounts,None)?;
    }
    a.bill.finalized = true; a.bill.finalized_at = now; a.bill.established_amount = amount;
    a.bill.released_amount = released;
    a.bill.earned_receipt = if amount > 0 {a.earn_receipt.key()} else {Pubkey::default()};
    a.bill.released_receipt = if released > 0 {a.release_receipt.key()} else {Pubkey::default()};
    Ok(())
}
