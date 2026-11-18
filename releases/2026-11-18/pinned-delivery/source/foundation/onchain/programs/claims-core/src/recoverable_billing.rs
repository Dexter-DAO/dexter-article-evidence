//! Explicit opt-in continuing identity for evidence and supplier receiving custody.
//! Fixed-key #81 bills retain their original proof keys and external payout route.
use anchor_lang::prelude::*;
use anchor_lang::compat::solana_instructions_sysvar as instructions;
use dexter_contracts::{CORE_ID, billing::{BillingPolicy,SourceReport,SellerClaim,
    source_report_digest,seller_claim_digest},commercial::SellerFeeTerms,TabTerms};
use dexter_contracts::recovery::{source_report_authority_digest,seller_claim_authority_digest};
use principal_registry::{Principal,Term};
use crate::{CoreError,receiving::ReceivingCustody,funding_state::valid_principal,
    evidence_billing::{self,EvidenceBill,AcceptEvidenceFinancedTab,AcceptEvidenceFinancedTabBumps,
    __client_accounts_accept_evidence_financed_tab,__cpi_client_accounts_accept_evidence_financed_tab}};

pub const BILL_AUTHORITY_SEED:&[u8]=b"bill-authority";
/// The native detached-Ed25519 path retains its original instruction index.
/// This explicit sentinel selects the exact Registry passkey-prefix profile.
pub const PASSKEY_RECORD_SIGNATURE_IX_INDEX:u16=u16::MAX;

#[account]
#[derive(InitSpace)]
pub struct BillAuthorityBinding {
    pub version:u16, pub bump:u8, pub bill:Pubkey, pub financial_claim:Pubkey,
    pub source_principal:Pubkey, pub supplier_principal:Pubkey, pub receiving_custody:Pubkey,
    pub source_owner_at_acceptance:Pubkey, pub source_generation_at_acceptance:u64,
    pub supplier_owner_at_acceptance:Pubkey, pub supplier_generation_at_acceptance:u64,
    pub source_signer:Pubkey, pub source_generation:u64, pub source_authorization_digest:[u8;32],
    pub claim_signer:Pubkey, pub claim_generation:u64, pub claim_authorization_digest:[u8;32],
}
impl BillAuthorityBinding {
    pub fn check(&self,key:Pubkey,bill:&Account<EvidenceBill>)->Result<()> {
        let (expected,bump)=Pubkey::find_program_address(&[BILL_AUTHORITY_SEED,bill.key().as_ref()],&CORE_ID);
        require!(self.version==1 && self.bump==bump && key==expected && self.bill==bill.key()
            && self.financial_claim==bill.financial_claim && self.source_principal==bill.policy.source
            && self.supplier_principal==bill.supplier_principal
            && bill.supplier_claimant==self.supplier_principal
            && self.source_generation_at_acceptance>0 && self.supplier_generation_at_acceptance>0,
            CoreError::BillingAuthorityBinding);
        Ok(())
    }
}
#[derive(Accounts)]
pub struct AcceptRecoverableFinancedTab<'info> {
    pub evidence:AcceptEvidenceFinancedTab<'info>,
    pub source_principal:Box<Account<'info,Principal>>,
    /// CHECK: Fresh admission validates the current source's native key or
    /// immutable Registry passkey alias. It does not require source consent.
    /// Exact historical admission retains the original source-key identity.
    pub source_authority:UncheckedAccount<'info>,
    pub receiving_custody:Box<Account<'info,ReceivingCustody>>,
    #[account(init_if_needed,payer=evidence.funding.payer,space=8+BillAuthorityBinding::INIT_SPACE,
        seeds=[BILL_AUTHORITY_SEED,evidence.bill.key().as_ref()],bump)]
    pub binding:Box<Account<'info,BillAuthorityBinding>>,
    pub system_program:Program<'info,System>,
}
#[derive(Accounts)]
pub struct SubmitPrincipalBillRecord<'info> {
    pub payer:Signer<'info>,
    #[account(mut)] pub bill:Box<Account<'info,EvidenceBill>>,
    #[account(mut)] pub binding:Box<Account<'info,BillAuthorityBinding>>,
    pub authority_principal:Box<Account<'info,Principal>>,
    /// CHECK: Native Ed25519 proof or exact preceding Registry passkey prefix.
    #[account(address=instructions::ID)] pub instructions:UncheckedAccount<'info>,
}

#[allow(clippy::too_many_arguments)]
pub fn accept<'info>(ctx:Context<'info,AcceptRecoverableFinancedTab<'info>>,
    request_id:[u8;32],terms:TabTerms,policy:BillingPolicy,maximum_interest:u64,
    seller_fee_terms:SellerFeeTerms,assignment_policy_hash:[u8;32],order_terms:Option<Vec<Term>>)->Result<()> {
    let a=ctx.accounts;
    let supplier=&a.evidence.funding.supplier_principal;
    a.receiving_custody.check(a.receiving_custody.key())?;
    require!(a.receiving_custody.principal==supplier.key()
        && a.receiving_custody.domain==terms.domain && a.receiving_custody.mint==terms.asset
        && terms.supplier==a.receiving_custody.wallet
        && policy.source==a.source_principal.key(),CoreError::BillingAuthorityBinding);
    // Never adopt a preexisting external-key bill into a new authority profile.
    let existing=a.binding.version!=0;
    if existing {
        require!(a.binding.version==1 && a.binding.bump==ctx.bumps.binding
            && a.binding.bill==a.evidence.bill.key()
            && a.binding.financial_claim==a.evidence.funding.claim.key()
            && a.binding.source_principal==a.source_principal.key()
            && a.binding.source_owner_at_acceptance==a.source_authority.key()
            && a.binding.supplier_principal==supplier.key()
            && a.binding.receiving_custody==a.receiving_custody.key(),CoreError::BillingReplay);
    } else {
        require!(a.evidence.funding.claim.version==0,CoreError::BillingAuthorityBinding);
        valid_principal(&a.source_principal)?; valid_principal(supplier)?;
        require!(a.source_principal.domain==terms.domain
            && a.source_principal.owner==a.source_authority.key(),CoreError::BillingAuthorityBinding);
        if !a.source_authority.key().is_on_curve() {
            principal_registry::read_passkey_authority(&a.source_authority,&a.source_principal)?;
        }
    }
    let source_owner=a.source_principal.owner;
    let source_generation=a.source_principal.authority_generation;
    let supplier_owner=supplier.owner;
    let supplier_generation=supplier.authority_generation;
    let supplier_key=supplier.key();
    evidence_billing::accept_policy_bound(&mut a.evidence,ctx.bumps.evidence,ctx.remaining_accounts,
        request_id,terms,policy,maximum_interest,seller_fee_terms,assignment_policy_hash,
        Some(a.receiving_custody.wallet),order_terms)?;
    if existing { return Ok(()); }
    a.binding.set_inner(BillAuthorityBinding{version:1,bump:ctx.bumps.binding,bill:a.evidence.bill.key(),
        financial_claim:a.evidence.funding.claim.key(),source_principal:a.source_principal.key(),
        supplier_principal:supplier_key,receiving_custody:a.receiving_custody.key(),
        source_owner_at_acceptance:source_owner,source_generation_at_acceptance:source_generation,
        supplier_owner_at_acceptance:supplier_owner,supplier_generation_at_acceptance:supplier_generation,
        source_signer:Pubkey::default(),source_generation:0,source_authorization_digest:[0;32],
        claim_signer:Pubkey::default(),claim_generation:0,claim_authorization_digest:[0;32]});
    Ok(())
}

fn current_authority(principal:&Account<Principal>,expected:Pubkey,domain:&[u8;32],generation:u64)->Result<Pubkey> {
    valid_principal(principal)?;
    require!(principal.key()==expected && &principal.domain==domain
        && principal.authority_generation==generation && generation>0
        && principal.owner!=Pubkey::default(),
        CoreError::BillingAuthorityGeneration);
    Ok(principal.owner)
}

/// Keep the signed-record meanings separate. Native source/seller signatures
/// cover the canonical authority wrapper. Passkeys authorize this exact Core
/// target (including its record, generation and immutable signer alias) through
/// the actual preceding Registry prefix. The stored body and wrapper digests
/// retain their original attribution meanings in both cases.
fn verify_record_authority(principal:&Account<Principal>,instructions:&AccountInfo,
    remaining:&[AccountInfo],signature_ix_index:u16,signer:Pubkey,
    authorization:&[u8;32])->Result<()> {
    if signer.is_on_curve() {
        evidence_billing::verify_signature(instructions,signature_ix_index,&signer,authorization)
    } else {
        require!(signature_ix_index==PASSKEY_RECORD_SIGNATURE_IX_INDEX,CoreError::BillingSignature);
        require!(remaining.len()==1 && remaining[0].key()==signer,CoreError::BillingAuthorityBinding);
        principal_registry::require_owner_authority(principal,&remaining[0],&[instructions.clone()])
    }
}

pub fn submit_evidence(ctx:Context<SubmitPrincipalBillRecord>,report:SourceReport,
    authority_generation:u64,signature_ix_index:u16)->Result<()> {
    let a=ctx.accounts;let key=a.bill.key();a.bill.check(key)?;
    a.binding.check(a.binding.key(),&a.bill)?;
    require_keys_eq!(a.authority_principal.key(),a.binding.source_principal,CoreError::BillingAuthorityBinding);
    let digest=source_report_digest(&a.bill.domain,&key,&a.bill.financial_claim,&a.bill.policy_hash,&report)
        .map_err(|_|error!(CoreError::BillingEvidence))?;
    if let Some(original)=&a.bill.source_report {
        require!(original==&report && a.bill.source_digest==digest,CoreError::BillingReplay);
        return Ok(()); // Historical attribution remains the original admission.
    }
    let now=Clock::get()?.unix_timestamp;
    require!(now<=a.bill.policy.evidence_deadline,CoreError::BillingDeadline);
    require!(!a.bill.finalized,CoreError::BillingFinality);
    let supported=a.bill.policy.validate_report(&report,a.bill.admitted_at,now)
        .map_err(|_|error!(CoreError::BillingEvidence))?;
    let signer=current_authority(&a.authority_principal,a.binding.source_principal,&a.bill.domain,authority_generation)?;
    let authorization=source_report_authority_digest(&a.bill.domain,&key,&a.authority_principal.key(),
        &signer,authority_generation,&digest);
    verify_record_authority(&a.authority_principal,&a.instructions,ctx.remaining_accounts,
        signature_ix_index,signer,&authorization)?;
    a.bill.source_report=Some(report);a.bill.source_digest=digest;a.bill.evidence_admitted_at=now;
    a.bill.supported_amount=supported;
    a.binding.source_signer=signer;a.binding.source_generation=authority_generation;
    a.binding.source_authorization_digest=authorization;
    Ok(())
}
pub fn submit_claim(ctx:Context<SubmitPrincipalBillRecord>,seller_claim:SellerClaim,
    authority_generation:u64,signature_ix_index:u16)->Result<()> {
    let a=ctx.accounts;let key=a.bill.key();a.bill.check(key)?;
    a.binding.check(a.binding.key(),&a.bill)?;
    require_keys_eq!(a.authority_principal.key(),a.binding.supplier_principal,CoreError::BillingAuthorityBinding);
    let digest=seller_claim_digest(&a.bill.domain,&key,&a.bill.financial_claim,&a.bill.policy_hash,&seller_claim)
        .map_err(|_|error!(CoreError::BillingClaim))?;
    if let Some(original)=&a.bill.seller_claim {
        require!(original==&seller_claim && a.bill.seller_digest==digest,CoreError::BillingReplay);
        return Ok(());
    }
    let now=Clock::get()?.unix_timestamp;
    require!(now<=a.bill.policy.claim_deadline,CoreError::BillingDeadline);
    require!(!a.bill.finalized,CoreError::BillingFinality);
    a.bill.policy.validate_claim(&seller_claim,a.bill.admitted_at,now)
        .map_err(|_|error!(CoreError::BillingClaim))?;
    let signer=current_authority(&a.authority_principal,a.binding.supplier_principal,&a.bill.domain,authority_generation)?;
    let authorization=seller_claim_authority_digest(&a.bill.domain,&key,&a.authority_principal.key(),
        &signer,authority_generation,&digest);
    verify_record_authority(&a.authority_principal,&a.instructions,ctx.remaining_accounts,
        signature_ix_index,signer,&authorization)?;
    a.bill.seller_claim=Some(seller_claim);a.bill.seller_digest=digest;a.bill.claim_admitted_at=now;
    a.binding.claim_signer=signer;a.binding.claim_generation=authority_generation;
    a.binding.claim_authorization_digest=authorization;
    Ok(())
}

