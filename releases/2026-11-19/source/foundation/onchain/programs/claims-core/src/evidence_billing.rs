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
    funding_state::*, funding_servicing::*,
    tab_events::{self, TabStepHeader, EventBytes, ReportAdmitted, ClaimAdmitted, BillFinalized, EarnDetail,
        DETAIL_SPACE, books_space}};

pub const EVIDENCE_BILL_SEED: &[u8] = b"evidence-bill";
pub const ED25519_ID: Pubkey = pubkey!("Ed25519SigVerify111111111111111111111111111");
/// A rule program signs an order's delivery reports as its address
/// `["delivery-source", policy.source_identity]`.
pub const DELIVERY_SOURCE_SEED: &[u8] = b"delivery-source";

pub fn bill_payment_id(bill: &Pubkey) -> [u8;32] {
    solana_sha256_hasher::hashv(&[b"DEXTER:BILL-PAYMENT:1", CORE_ID.as_ref(), bill.as_ref()]).to_bytes()
}
pub fn bill_release_id(bill: &Pubkey) -> [u8;32] {
    solana_sha256_hasher::hashv(&[b"DEXTER:BILL-RELEASE:1", CORE_ID.as_ref(), bill.as_ref()]).to_bytes()
}
/// The finalization step's operation id: one per bill.
pub fn bill_finalize_id(bill: &Pubkey) -> [u8;32] {
    solana_sha256_hasher::hashv(&[b"DEXTER:BILL-FINALIZE:1", CORE_ID.as_ref(), bill.as_ref()]).to_bytes()
}
/// An evidence order's bill lives under its Tab: `["evidence-bill", tab]`, so a
/// bill (and the DeliveryRules case at `["case", bill]`) is never reused.
pub fn bill_address(tab: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[EVIDENCE_BILL_SEED, tab.as_ref()], &CORE_ID)
}
// Rule programs (programs/delivery-rules) deserialize this layout; rule_program_layout pins it.
#[account]
#[derive(InitSpace)]
pub struct EvidenceBill {
    pub deposit_payer: Pubkey,
    pub version: u8, pub bump: u8, pub domain: [u8;32], pub principal: Pubkey,
    pub supplier_principal: Pubkey, pub financial_claim: Pubkey, pub supplier_claimant: Pubkey,
    pub acceptance_digest: [u8;32],
    pub policy: BillingPolicy, pub policy_hash: [u8;32], pub admitted_at: i64,
    pub source_report: Option<SourceReport>, pub source_digest: [u8;32],
    pub evidence_admitted_at: i64, pub supported_amount: u128,
    pub seller_claim: Option<SellerClaim>, pub seller_digest: [u8;32], pub claim_admitted_at: i64,
    pub finalized: bool, pub finalized_at: i64, pub established_amount: u64, pub released_amount: u64,
}

impl EvidenceBill {
    pub fn check(&self, key: Pubkey) -> Result<()> {
        let (expected, bump) = bill_address(&self.financial_claim);
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
    /// The bill's Tab: an admission is a step of its receipt book.
    #[account(mut, address = bill.financial_claim @ CoreError::BillingIdentity)]
    pub claim: Box<Account<'info, FundedClaim>>,
    /// CHECK: ClaimsCore's event authority.
    #[account(address = crate::EVENT_AUTHORITY_AND_BUMP.0)] pub core_event_authority: UncheckedAccount<'info>,
    /// CHECK: ClaimsCore itself.
    #[account(address = crate::ID)] pub core_program: UncheckedAccount<'info>,
}

/// An order whose delivery signer is a rule program's delivery-source address.
#[derive(Accounts)]
pub struct AcceptProgramSourcedFinancedTab<'info> {
    pub evidence: AcceptEvidenceFinancedTab<'info>,
    /// CHECK: Executable upgradeable-loader program, read in `check_rule_program`.
    pub rule_program: UncheckedAccount<'info>,
    /// CHECK: Its loader-owned ProgramData, read in `check_rule_program`.
    pub rule_program_data: UncheckedAccount<'info>,
    /// CHECK: ClaimsCore's own ProgramData, read in `check_rule_program`.
    pub core_program_data: UncheckedAccount<'info>,
}

/// The rule program, by CPI, signs as the bill's delivery-source address.
#[derive(Accounts)]
pub struct SubmitProgramBillRecord<'info> { // Rule programs pass these accounts in this order by CPI; rule_program_layout pins them.
    #[account(mut)] pub bill: Box<Account<'info, EvidenceBill>>,
    pub source: Signer<'info>,
    /// CHECK: Its address derives `source`; its loader state is re-checked (`check_rule_program`).
    pub rule_program: UncheckedAccount<'info>,
    /// The bill's Tab: an admission is a step of its receipt book.
    #[account(mut, address = bill.financial_claim @ CoreError::BillingIdentity)]
    pub claim: Box<Account<'info, FundedClaim>>,
    /// CHECK: The rule program's ProgramData.
    pub rule_program_data: UncheckedAccount<'info>,
    /// CHECK: ClaimsCore's ProgramData.
    pub core_program_data: UncheckedAccount<'info>,
    /// CHECK: ClaimsCore's event authority.
    #[account(address = crate::EVENT_AUTHORITY_AND_BUMP.0)] pub core_event_authority: UncheckedAccount<'info>,
    /// CHECK: ClaimsCore itself: the event program, and the program whose upgrade authority is read.
    #[account(address = crate::ID)] pub core_program: UncheckedAccount<'info>,
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
    /// The bill's Tab.
    #[account(mut, address = bill.financial_claim @ CoreError::BillingIdentity)]
    pub claim: Box<Account<'info, FundedClaim>>,
    /// CHECK: ClaimsCore's event authority.
    #[account(address = crate::EVENT_AUTHORITY_AND_BUMP.0)] pub core_event_authority: UncheckedAccount<'info>,
    /// CHECK: ClaimsCore itself.
    #[account(address = crate::ID)] pub core_program: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct FinalizeEvidenceBill<'info> {
    #[account(mut)] pub payer: Signer<'info>,
    #[account(mut)] pub bill: Box<Account<'info, EvidenceBill>>,
    pub principal: Box<Account<'info, Principal>>,
    #[account(mut)] pub budget: Box<Account<'info, Budget>>,
    #[account(mut)] pub reservation: Box<Account<'info, Reservation>>,
    /// CHECK: Existing Core-to-Registry consumer signer.
    #[account(seeds=[REGISTRY_CONSUMER_SEED],bump)] pub registry_authority: UncheckedAccount<'info>,
    pub registry_program: Program<'info, PrincipalRegistry>,
    #[account(mut)] pub line: Box<Account<'info, CustomerCreditLine>>,
    #[account(mut)] pub claim: Box<Account<'info, FundedClaim>>,
    #[account(mut)] pub facility: Box<Account<'info, FundingFacility>>,
    #[account(mut)] pub custody: Box<Account<'info, FinancierCustody>>,
    pub custody_token: Box<Account<'info, TokenAccount>>,
    /// The bill's run, under its Tab, created only when the established amount is positive.
    #[account(init_if_needed,payer=payer,space=8+FundedRun::INIT_SPACE,
        seeds=[b"run",claim.key().as_ref(),&bill.policy.work_id],bump)]
    pub run: Box<Account<'info, FundedRun>>,
    pub mint: Box<Account<'info, Mint>>,
    /// CHECK: Existing accepted-servicing projection boundary.
    #[account(address=POOL_ID)] pub pool_program: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
    /// CHECK: ClaimsCore's event authority.
    #[account(address=crate::EVENT_AUTHORITY_AND_BUMP.0)] pub core_event_authority: UncheckedAccount<'info>,
    /// CHECK: ClaimsCore itself.
    #[account(address=crate::ID)] pub core_program: UncheckedAccount<'info>,
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
        maximum_interest, seller_fee_terms, assignment_policy_hash, None, order_terms, false)
}

/// A program's upgrade authority, read as typed upgradeable-loader state: the
/// Program account names its ProgramData, which holds the authority.
fn upgrade_authority(program: &AccountInfo, program_data: &AccountInfo) -> Result<Option<Pubkey>> {
    use anchor_lang::solana_program::bpf_loader_upgradeable::{self, UpgradeableLoaderState};
    require!(program.executable && *program.owner == bpf_loader_upgradeable::ID
        && *program_data.owner == bpf_loader_upgradeable::ID, CoreError::BillingSourceProgram);
    let state = UpgradeableLoaderState::try_deserialize(&mut &program.try_borrow_data()?[..])
        .map_err(|_| error!(CoreError::BillingSourceProgram))?;
    let UpgradeableLoaderState::Program { programdata_address } = state else {
        return err!(CoreError::BillingSourceProgram);
    };
    require_keys_eq!(programdata_address, program_data.key(), CoreError::BillingSourceProgram);
    let data = ProgramData::try_deserialize(&mut &program_data.try_borrow_data()?[..])
        .map_err(|_| error!(CoreError::BillingSourceProgram))?;
    Ok(data.upgrade_authority_address)
}

/// The delivery signer is `rule_program`'s delivery-source address for this
/// source identity, and `rule_program` is trusted: frozen (no upgrade
/// authority), or upgradeable by exactly ClaimsCore's own upgrade authority.
/// A frozen ClaimsCore admits only frozen rule programs. Checked at the
/// commit and again at every report admission.
fn check_rule_program(policy: &BillingPolicy, program: &AccountInfo, program_data: &AccountInfo,
    core: &AccountInfo, core_data: &AccountInfo) -> Result<()> {
    let (source, _) = Pubkey::find_program_address(&[DELIVERY_SOURCE_SEED, &policy.source_identity], program.key);
    require_keys_eq!(source, policy.source, CoreError::BillingSourceProgram);
    if let Some(authority) = upgrade_authority(program, program_data)? {
        require_keys_eq!(core.key(), crate::ID, CoreError::BillingSourceProgram);
        require!(upgrade_authority(core, core_data)? == Some(authority), CoreError::BillingSourceProgram);
    }
    Ok(())
}

/// `accept_evidence_financed_tab` for an order whose delivery reports come from a
/// rule program. `order_terms`, when shown, hashes to `policy.request_hash`.
#[allow(clippy::too_many_arguments)]
pub fn accept_program_sourced<'info>(ctx: Context<'info, AcceptProgramSourcedFinancedTab<'info>>,
    request_id: [u8;32], terms: TabTerms, policy: BillingPolicy, maximum_interest: u64,
    seller_fee_terms: SellerFeeTerms, assignment_policy_hash: [u8;32], order_terms: Option<Vec<Term>>,
) -> Result<()> {
    check_rule_program(&policy, &ctx.accounts.rule_program, &ctx.accounts.rule_program_data,
        &ctx.accounts.evidence.funding.core_program, &ctx.accounts.core_program_data)?;
    accept_policy_bound(&mut ctx.accounts.evidence, ctx.bumps.evidence, ctx.remaining_accounts, request_id,
        terms, policy, maximum_interest, seller_fee_terms, assignment_policy_hash, None, order_terms, true)
}

/// Only the recovery-profile wrapper may supply a continuing identity and
/// payee after authenticating its canonical immutable authority/custody graph.
#[allow(clippy::too_many_arguments)]
pub(crate) fn accept_policy_bound<'info>(a: &mut AcceptEvidenceFinancedTab<'info>,
    bumps: AcceptEvidenceFinancedTabBumps, remaining_accounts: &[AccountInfo<'info>],
    request_id: [u8;32], terms: TabTerms, policy: BillingPolicy, maximum_interest: u64,
    seller_fee_terms: SellerFeeTerms, assignment_policy_hash: [u8;32],
    continuing_payee: Option<Pubkey>, order_terms: Option<Vec<Term>>, program_source: bool,
) -> Result<()> {
    let supplier_principal = a.funding.supplier_principal.key();
    let claim_key = a.funding.claim.key();
    let (bill_key, bump) = bill_address(&claim_key);
    require_keys_eq!(a.bill.key(), bill_key, CoreError::BillingIdentity);
    let policy_hash = billing_policy_digest(&terms.domain, &bill_key, &claim_key, &policy)
        .map_err(|_| error!(CoreError::BillingPolicy))?;
    require!(terms.expires_at == policy.work_until, CoreError::BillingPolicy);
    require!(terms.external_terms_hash == contracts::completed_run_policy(
        &BILLING_NAMESPACE, &BILLING_SETTLEMENT_UNIT, 1, &policy_hash), CoreError::BillingPolicy);

    // A landed commit is never applied again: its bill exists with its Tab.
    require!(*a.bill.to_account_info().owner != CORE_ID && a.funding.claim.version == 0, CoreError::NumberUsed);
    require!(*a.bill.to_account_info().owner == system_program::ID && a.bill.data_is_empty(),
        CoreError::BillingIdentity);
    policy.validate(Clock::get()?.unix_timestamp).map_err(|_| error!(CoreError::BillingPolicy))?;
    if continuing_payee.is_none() {
        require!((program_source || policy.source.is_on_curve()) && a.funding.supplier.key().is_on_curve(),
            CoreError::BillingPolicy);
    }
    let supplier_claimant = if continuing_payee.is_some() { supplier_principal } else { a.funding.supplier.key() };
    let payee = continuing_payee.unwrap_or(a.funding.supplier.key());
    let order_terms = match order_terms {
        None => OrderTerms::Hidden,
        Some(terms) => OrderTerms::Billing { bill: bill_key, policy: &policy, terms },
    };
    funding::accept_bound_payee(&mut a.funding, bumps.funding, remaining_accounts,
        request_id, terms, maximum_interest, seller_fee_terms,
        assignment_policy_hash, BILLING_NAMESPACE, BILLING_SETTLEMENT_UNIT, 1, policy_hash, payee, order_terms)?;
    create_bill(a, bill_key, bump, supplier_principal, supplier_claimant, policy, policy_hash)
}

/// The new bill, built in its own frame and written straight into its account:
/// a boxed copy would hold 1.2 KB of the never-freeing bump heap for the rest of
/// the commit.
#[inline(never)]
fn create_bill(a: &AcceptEvidenceFinancedTab, bill_key: Pubkey, bump: u8, supplier_principal: Pubkey,
    supplier_claimant: Pubkey, policy: BillingPolicy, policy_hash: [u8; 32]) -> Result<()> {
    let claim_key = a.funding.claim.key();
    let now = Clock::get()?.unix_timestamp;
    let bill = EvidenceBill { deposit_payer:a.funding.payer.key(), version:BILLING_POLICY_VERSION, bump,
        domain:a.funding.claim.terms.domain,
        principal:a.funding.principal.key(), supplier_principal, financial_claim:claim_key,
        supplier_claimant, acceptance_digest:a.funding.claim.acceptance_hash, policy, policy_hash, admitted_at:now,
        source_report:None, source_digest:[0;32], evidence_admitted_at:0, supported_amount:0,
        seller_claim:None, seller_digest:[0;32], claim_admitted_at:0, finalized:false, finalized_at:0,
        established_amount:0, released_amount:0 };
    bill.check(bill_key)?;
    let bump_seed = [bump];
    let seeds: &[&[u8]] = &[EVIDENCE_BILL_SEED, claim_key.as_ref(), &bump_seed];
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

/// Admit the delivery report once; `authenticate` proves the delivery signer
/// signed its digest. A report is admitted once: any later one, the same or
/// not, is refused. An admission is a step of the Tab's book (not a money step).
fn admit_report<'info>(bill: &mut Account<EvidenceBill>, claim: &mut Account<FundedClaim>,
    event_authority: &AccountInfo<'info>, report: SourceReport,
    authenticate: &dyn Fn(&EvidenceBill, &[u8;32]) -> Result<()>) -> Result<()> {
    let key = bill.key(); bill.check(key)?;
    let digest = source_report_digest(&bill.domain, &key, &bill.financial_claim, &bill.policy_hash, &report)
        .map_err(|_| error!(CoreError::BillingEvidence))?;
    require!(bill.source_report.is_none(), CoreError::BillingReplay);
    let now = Clock::get()?.unix_timestamp;
    require!(now <= bill.policy.evidence_deadline, CoreError::BillingDeadline);
    require!(!bill.finalized, CoreError::BillingFinality);
    let supported = bill.policy.validate_report(&report, bill.admitted_at, now)
        .map_err(|_| error!(CoreError::BillingEvidence))?;
    authenticate(bill, &digest)?;
    claim.check(claim.key())?;
    let header = TabStepHeader { tab: claim.key(), step: claim.next_step()?,
        step_id: tab_events::step_id(&key.to_bytes(), &digest) };
    let event = EventBytes::new::<ReportAdmitted>(TabStepHeader::INIT_SPACE + 32 + SourceReport::INIT_SPACE + 32 + 16 + 32
        + 8 + 32)
        .put(&header)?.put(&key)?.put(&report)?.put(&digest)?.put(&supported)?.put(&bill.policy.source)?
        .put(&0u64)?.put(&[0u8; 32])?;
    bill.source_report = Some(report); bill.source_digest = digest;
    bill.evidence_admitted_at = now; bill.supported_amount = supported;
    tab_events::record_tab_step(claim, event_authority, event, false)
}

pub fn submit_evidence(ctx: Context<SubmitBillRecord>, report: SourceReport, signature_ix_index: u16) -> Result<()> {
    let a = ctx.accounts;
    let instructions = a.instructions.to_account_info();
    admit_report(&mut a.bill, &mut a.claim, &a.core_event_authority, report, &|bill, digest|
        verify_signature(&instructions, signature_ix_index, &bill.policy.source, digest))
}

/// The delivery signer is `rule_program`'s delivery-source address and signed this
/// instruction. Only a rule-program order, checked at acceptance, has such a
/// signer; the rule program's trust is checked again here, so an authority
/// moved after the commit cannot report into the order.
pub fn submit_program_evidence(ctx: Context<SubmitProgramBillRecord>, report: SourceReport) -> Result<()> {
    let a = ctx.accounts;
    check_rule_program(&a.bill.policy, &a.rule_program, &a.rule_program_data, &a.core_program, &a.core_program_data)?;
    let rule_program = a.rule_program.key();
    let source = a.source.key();
    admit_report(&mut a.bill, &mut a.claim, &a.core_event_authority, report, &|bill, _| {
        let (expected, _) = Pubkey::find_program_address(
            &[DELIVERY_SOURCE_SEED, &bill.policy.source_identity], &rule_program);
        require!(expected == bill.policy.source && source == expected, CoreError::BillingSignature);
        Ok(())
    })
}

pub fn submit_claim(ctx: Context<SubmitFixedBillClaim>, seller_claim: SellerClaim, signature_ix_index: u16) -> Result<()> {
    let a = ctx.accounts; let key = a.bill.key(); a.bill.check(key)?;
    a.claim.check(a.claim.key())?;
    require_keys_eq!(a.supplier_principal.key(), a.bill.supplier_principal, CoreError::BillingIdentity);
    let digest = seller_claim_digest(&a.bill.domain, &key, &a.bill.financial_claim, &a.bill.policy_hash, &seller_claim)
        .map_err(|_| error!(CoreError::BillingClaim))?;
    require!(a.bill.seller_claim.is_none(), CoreError::BillingReplay);
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
    let header = TabStepHeader { tab: a.claim.key(), step: a.claim.next_step()?,
        step_id: tab_events::step_id(&key.to_bytes(), &digest) };
    let event = EventBytes::new::<ClaimAdmitted>(TabStepHeader::INIT_SPACE + 32 + SellerClaim::INIT_SPACE + 32
        + 32 + 8 + 32)
        .put(&header)?.put(&key)?.put(&seller_claim)?.put(&digest)?.put(&a.bill.supplier_claimant)?
        .put(&0u64)?.put(&[0u8; 32])?;
    a.bill.seller_claim = Some(seller_claim); a.bill.seller_digest = digest; a.bill.claim_admitted_at = now;
    tab_events::record_tab_step(&mut a.claim, &a.core_event_authority, event, false)
}

pub fn finalize<'info>(ctx: Context<'info, FinalizeEvidenceBill<'info>>, expected_steps:u32,
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
    // Finalization happens once; afterwards it is refused for anyone.
    require!(!a.bill.finalized, CoreError::BillingReplay);
    let now = Clock::get()?.unix_timestamp;
    // Both records are immutable once admitted, so the amount is already final.
    require!(a.bill.policy.finalization_ready(now)
        || (a.bill.source_report.is_some() && a.bill.seller_claim.is_some()), CoreError::BillingFinality);
    a.claim.guard(expected_steps)?;
    require!(a.claim.captured_total()? == 0
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
    let sid = tab_events::step_id(&bill_finalize_id(&bill_key), &capture_digest);
    let earned = if amount > 0 {
        let earned = establish_run(EarnedRunAccounts {
            payer:&a.payer,principal:&a.principal,budget:&mut a.budget,reservation:&mut a.reservation,
            registry_authority:&a.registry_authority,
            registry_program:&a.registry_program,line:&mut a.line,claim:&mut a.claim,
            custody:&mut a.custody,custody_token:&a.custody_token,run:&mut a.run,
            mint:&a.mint,pool_program:&a.pool_program,
        }, EarnedRunTerms { payment_id:bill_payment_id(&bill_key),namespace:BILLING_NAMESPACE,
            run_id:a.bill.policy.work_id,service_unit:BILLING_SETTLEMENT_UNIT,external_contract_hash:a.bill.policy_hash,
            units:amount,unit_price:1,service_without_pool,capture_digest,enforce_deadline:false },
            &sid,ctx.bumps.registry_authority,ctx.bumps.run,ctx.remaining_accounts)?;
        // Registry owns these accounts. The earning CPI has moved the consumed
        // amount; release must validate the new state, not this instruction's
        // deserialized snapshot.
        a.budget.reload()?;
        a.reservation.reload()?;
        Some(earned)
    } else {
        // Nothing was established: the run account Anchor allocated for this
        // instruction is closed again, so no empty run keeps a deposit.
        a.run.close(a.payer.to_account_info())?;
        None
    };
    let released = a.claim.remaining;
    let release = if released > 0 {
        Some(release_unused(ReleaseAccounts {
            principal:&a.principal,budget:&mut a.budget,reservation:&mut a.reservation,
            registry_authority:&a.registry_authority,
            registry_program:&a.registry_program,line:&mut a.line,claim:&mut a.claim,facility:&mut a.facility,
            custody:&mut a.custody,custody_token:&a.custody_token,
            mint:&a.mint,pool_program:&a.pool_program,
        },bill_release_id(&bill_key),&sid,released,service_without_pool,
            ctx.bumps.registry_authority,ctx.remaining_accounts,None,None)?)
    } else { None };
    a.bill.finalized = true; a.bill.finalized_at = now; a.bill.established_amount = amount;
    a.bill.released_amount = released;
    let header = TabStepHeader { tab: a.claim.key(), step: a.claim.next_step()?, step_id: sid };
    let event = finalized_event(&header, &bill_key, amount, released, &capture_digest, earned.as_ref(), release.as_ref())?;
    tab_events::record_tab_step(&mut a.claim, &a.core_event_authority, event, true)
}

#[inline(never)]
fn finalized_event(header: &TabStepHeader, bill: &Pubkey, established: u64, released: u64,
    capture_digest: &[u8;32], earned: Option<&Earned>,
    release: Option<&(dexter_contracts::funded::FinanceDetail, crate::funding_journal::Booked)>) -> Result<EventBytes> {
    let empty_detail = dexter_contracts::funded::FinanceDetail::default();
    let empty_earn = EarnDetail { run_id: [0;32], units: 0, unit_price: 0, capture_digest: [0;32], payment_id: [0;32],
        run_fee_cap: 0, run_deposit_payer: Pubkey::default() };
    let no_books: Vec<dexter_contracts::funded::BookStep> = Vec::new();
    let no_changes: Vec<dexter_contracts::funded::BookChange> = Vec::new();
    let (earn, earned_detail, earned_books, earned_changes) = match earned {
        Some(e) => (&e.earn, &e.detail, &e.booked.books, &e.booked.changes),
        None => (&empty_earn, &empty_detail, &no_books, &no_changes),
    };
    let (release_detail, release_books, release_changes) = match release {
        Some((detail, booked)) => (detail, &booked.books, &booked.changes),
        None => (&empty_detail, &no_books, &no_changes),
    };
    EventBytes::new::<BillFinalized>(TabStepHeader::INIT_SPACE + 32 + 16 + 32 + EarnDetail::INIT_SPACE
        + 2 * DETAIL_SPACE + books_space(earned_books, earned_changes) + books_space(release_books, release_changes))
        .put(header)?.put(bill)?.put(&established)?.put(&released)?.put(capture_digest)?.put(earn)?
        .put(earned_detail)?.put(earned_books)?.put(earned_changes)?
        .put(release_detail)?.put(release_books)?.put(release_changes)
}

/// What rule programs (programs/delivery-rules) are built against: they deserialize
/// EvidenceBill and call submit_program_bill_evidence by CPI. Change 5 changed both
/// (the bill lost its receipt links and gained its rent payer; the report CPI gained
/// the Tab, both ProgramData accounts and the event accounts). A change to any value
/// below breaks every deployed frozen rule program; such a change needs a new rule
/// program, not an edit here.
#[cfg(test)]
mod rule_program_layout {
    use super::*;
    use anchor_lang::{Discriminator, InstructionData, ToAccountMetas};
    use anchor_lang::solana_program::instruction::AccountMeta;

    const MAX_METRICS: usize = dexter_contracts::billing::MAX_BILLING_METRICS;

    fn key(n: u8) -> Pubkey { Pubkey::new_from_array([n; 32]) }

    fn report() -> SourceReport {
        SourceReport { record_id: [41; 32], request_hash: [42; 32], started_at: 0x4343_4343_4343_4343,
            completed_at: 0x4444_4444_4444_4444, units: [0x4545_4545_4545_4545; MAX_METRICS], result_hash: [46; 32],
            finality: 47, outcome: 48 }
    }

    fn bill(with_report: bool, with_claim: bool) -> EvidenceBill {
        let policy = BillingPolicy { schema_version: 11, source: key(12), source_identity: [13; 32], work_id: [14; 32],
            request_hash: [15; 32], work_until: 0x1616_1616_1616_1616, evidence_deadline: 0x1717_1717_1717_1717,
            claim_deadline: 0x1818_1818_1818_1818, metric_count: 19, metric_ids: [[20; 32]; MAX_METRICS],
            price_numerators: [0x2121_2121_2121_2121; MAX_METRICS], price_denominators: [0x2222_2222_2222_2222; MAX_METRICS],
            max_units: [0x2323_2323_2323_2323; MAX_METRICS] };
        EvidenceBill { deposit_payer: key(50), version: 1, bump: 2, domain: [3; 32], principal: key(4),
            supplier_principal: key(5), financial_claim: key(6), supplier_claimant: key(7), acceptance_digest: [9; 32],
            policy, policy_hash: [24; 32], admitted_at: 0x2525_2525_2525_2525,
            source_report: if with_report { Some(report()) } else { None }, source_digest: [26; 32],
            evidence_admitted_at: 0x2727_2727_2727_2727, supported_amount: 0x2828_2828_2828_2828_2828_2828_2828_2828,
            seller_claim: if with_claim { Some(SellerClaim { claim_id: [29; 32], amount: 0x3030_3030_3030_3030 }) } else { None },
            seller_digest: [31; 32], claim_admitted_at: 0x3232_3232_3232_3232, finalized: true,
            finalized_at: 0x3333_3333_3333_3333, established_amount: 0x3434_3434_3434_3434,
            released_amount: 0x3535_3535_3535_3535 }
    }

    fn at(data: &[u8], offset: usize, len: usize) -> &[u8] { &data[offset..offset + len] }

    #[test]
    fn evidence_bill_account_layout() {
        assert_eq!(EvidenceBill::DISCRIMINATOR, &[2, 219, 144, 198, 73, 235, 164, 92]);
        assert_eq!(DELIVERY_SOURCE_SEED, b"delivery-source");
        assert_eq!(crate::ID.to_string(), "DEXcor9xYbiaSyvdEjKprMZuJFwe3MdgwW5kVzUiHdtu");
        for (with_report, with_claim) in [(false, false), (false, true), (true, false), (true, true)] {
            let mut data = Vec::new();
            bill(with_report, with_claim).try_serialize(&mut data).unwrap();
            assert_eq!(at(&data, 0, 8), &[2, 219, 144, 198, 73, 235, 164, 92]);
            // Account offsets (discriminator included) of every field before the optional records.
            for (offset, len, byte) in [(8, 32, 50), (40, 1, 1), (41, 1, 2), (42, 32, 3), (74, 32, 4), (106, 32, 5),
                    (138, 32, 6), (170, 32, 7), (202, 32, 9)] {
                assert!(at(&data, offset, len).iter().all(|b| *b == byte), "field at {offset}");
            }
            // policy (602 bytes at 234): every field a rule program reads, at its offset.
            for (offset, len, byte) in [(234, 1, 11), (235, 32, 12), (267, 32, 13), (299, 32, 14), (331, 32, 15),
                    (363, 8, 0x16), (371, 8, 0x17), (379, 8, 0x18), (387, 1, 19), (388, 256, 20), (644, 64, 0x21),
                    (708, 64, 0x22), (772, 64, 0x23), (836, 32, 24), (868, 8, 0x25)] {
                assert!(at(&data, offset, len).iter().all(|b| *b == byte), "policy or admitted_at field at {offset}");
            }
            // source_report: tag at 876, then 178 bytes when present.
            assert_eq!(data[876], with_report as u8);
            let mut offset = 877;
            if with_report {
                for (relative, len, byte) in [(0, 32, 41), (32, 32, 42), (64, 8, 0x43), (72, 8, 0x44), (80, 64, 0x45),
                        (144, 32, 46), (176, 1, 47), (177, 1, 48)] {
                    assert!(at(&data, offset + relative, len).iter().all(|b| *b == byte), "report field {relative}");
                }
                offset += 178;
            }
            for (len, byte) in [(32, 26), (8, 0x27), (16, 0x28)] {
                assert!(at(&data, offset, len).iter().all(|b| *b == byte), "field at {offset}");
                offset += len;
            }
            assert_eq!(data[offset], with_claim as u8);
            offset += 1;
            if with_claim {
                assert!(at(&data, offset, 32).iter().all(|b| *b == 29) && at(&data, offset + 32, 8).iter().all(|b| *b == 0x30));
                offset += 40;
            }
            for (len, byte) in [(32, 31), (8, 0x32), (1, 1), (8, 0x33), (8, 0x34), (8, 0x35)] {
                assert!(at(&data, offset, len).iter().all(|b| *b == byte), "field at {offset}");
                offset += len;
            }
            assert_eq!(offset, data.len());
            assert_eq!(8 + 869 + 178 * with_report as usize + 57 + 40 * with_claim as usize + 65, data.len());
        }
        assert_eq!(8 + EvidenceBill::INIT_SPACE, 8 + 869 + 178 + 57 + 40 + 65);
    }

    #[test]
    fn submit_program_bill_evidence_interface() {
        let data = crate::instruction::SubmitProgramBillEvidence { report: report() }.data();
        assert_eq!(at(&data, 0, 8), &[226, 108, 232, 143, 249, 16, 101, 245]);
        assert_eq!(data.len(), 8 + 178);
        for (offset, len, byte) in [(8, 32, 41), (40, 32, 42), (72, 8, 0x43), (80, 8, 0x44), (88, 64, 0x45),
                (152, 32, 46), (184, 1, 47), (185, 1, 48)] {
            assert!(at(&data, offset, len).iter().all(|b| *b == byte), "report field at {offset}");
        }
        let metas = crate::accounts::SubmitProgramBillRecord { bill: key(1), source: key(2), rule_program: key(3),
            claim: key(4), rule_program_data: key(5), core_program_data: key(6), core_event_authority: key(7),
            core_program: key(8) }.to_account_metas(None);
        assert_eq!(metas, vec![AccountMeta::new(key(1), false), AccountMeta::new_readonly(key(2), true),
            AccountMeta::new_readonly(key(3), false), AccountMeta::new(key(4), false),
            AccountMeta::new_readonly(key(5), false), AccountMeta::new_readonly(key(6), false),
            AccountMeta::new_readonly(key(7), false), AccountMeta::new_readonly(key(8), false)]);
    }
}
