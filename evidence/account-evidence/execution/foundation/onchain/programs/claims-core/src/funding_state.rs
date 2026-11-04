//! Authoritative cash funding, customer obligations and delivery-linked title.
//! Reconstructed from the accepted contracts; all execution evidence is new.
use anchor_lang::prelude::*;
use crate::CoreError;
use anchor_spl::{associated_token, token::{self, Mint, TokenAccount}};
use dexter_contracts::{self as contracts, plus, minus, TabTerms, CORE_ID, REGISTRY_ID};
use dexter_contracts::commercial::{InterestTerms, InterestState, SellerFeeTerms, SellerFeeState};
use dexter_contracts::financing::{CustodyAmounts, FinanceResult, PrincipalAmounts};
use dexter_contracts::corrections::CorrectionAmounts;
use dexter_contracts::funded::{FinanceBookData, FinanceReceiptData, FUNDED_VERSION};
use principal_registry::{Principal, Reservation};

pub const WALLET_CASH: u8 = 1;
pub const ESCROW_CASH: u8 = 2;

/// Only claim/run layouts advance; financial receipts, books and custody keep v1.
pub const FUNDED_CLAIM_VERSION: u16 = 2;
pub const FUNDED_RUN_VERSION: u16 = 2;

/// Delivered-work identity follows the continuing supplier Principal. Its
/// current approver and each claim's frozen payment wallet are separate facts.
pub fn funded_run_operation_id(supplier_principal: &Pubkey, namespace: &[u8; 32], run_id: &[u8; 32]) -> [u8; 32] {
    contracts::completed_run_operation_id(supplier_principal, namespace, run_id)
}

pub fn funded_run_address(supplier_principal: &Pubkey, namespace: &[u8; 32], run_id: &[u8; 32]) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"usage", supplier_principal.as_ref(), namespace, run_id], &CORE_ID)
}

#[account]
#[derive(InitSpace)]
pub struct FinanceBook { pub data: FinanceBookData }

#[account]
#[derive(InitSpace)]
pub struct FinanceReceipt { pub data: FinanceReceiptData }

#[account]
#[derive(InitSpace)]
pub struct FinancierCustody {
    pub version: u16, pub bump: u8, pub domain: [u8; 32], pub principal: Pubkey,
    pub binding: Pubkey, pub wallet: Pubkey, pub token_account: Pubkey, pub mint: Pubkey,
    pub profile: u8,
    /// Explicit allocation only. An unsolicited transfer does not become income.
    pub accounted_cash: u64, pub unused: u64, pub payable: u64,
    pub contributed_capital: u64,
    /// Cumulative authorized uncommitted debits: withdrawals, sends and swap input.
    /// Includes eligible earned cash; it is not a valuation or income measure.
    pub withdrawn_cash: u64,
    pub revision: u64,
}
impl FinancierCustody {
    pub fn amounts(&self) -> CustodyAmounts { CustodyAmounts { unused: self.unused, earned_unpaid: self.payable } }
    pub fn protected(&self) -> Result<u64> { math(self.amounts().protected()) }
    pub fn check(&self, actual: u64) -> Result<()> {
        require!(self.version == FUNDED_VERSION && self.revision > 0, CoreError::FundingSchema);
        require!(actual >= self.accounted_cash && self.accounted_cash >= self.protected()?, CoreError::FundingCashShortfall);
        Ok(())
    }
    pub fn check_key(&self, key: Pubkey) -> Result<()> {
        require!(self.version == FUNDED_VERSION && self.revision > 0, CoreError::FundingSchema);
        let (expected, bump) = Pubkey::find_program_address(&[b"custody-reserve", self.token_account.as_ref()], &CORE_ID);
        require!(key == expected && self.bump == bump, CoreError::FundingCustody);
        Ok(())
    }
    pub fn credit(&mut self, amount: u64) -> Result<()> {
        self.accounted_cash = plus(self.accounted_cash, amount)?;
        self.revision = plus(self.revision, 1)?; Ok(())
    }
    /// The shared withdrawal/spending floor is expressed in this mint's actual
    /// units. It neither adopts unsolicited surplus nor spends another facility's
    /// accepted backing. The caller separately authenticates capital ownership.
    pub fn debit_uncommitted(&mut self, actual: u64, amount: u64) -> Result<()> {
        self.check(actual)?;
        require!(amount>0 && amount<=minus(self.accounted_cash,self.protected()?)?,
            CoreError::FundingCashShortfall);
        self.accounted_cash=minus(self.accounted_cash,amount)?;
        self.withdrawn_cash=plus(self.withdrawn_cash,amount)?;
        self.revision=plus(self.revision,1)?;
        self.check(minus(actual,amount)?)
    }
}

#[account]
#[derive(InitSpace)]
pub struct FundingFacility {
    pub version: u16, pub bump: u8, pub facility_id: [u8; 32],
    pub financier: Pubkey, pub issuer: Pubkey, pub custody: Pubkey, pub mint: Pubkey,
    pub external: bool, pub commitment_limit: u64, pub reserved: u64,
    pub drawn: u64, pub repaid: u64, pub accepts_new: bool,
    pub accept_until: i64, pub revision: u64, pub terms_hash: [u8; 32], pub opening_hash:[u8;32],
    pub interest_terms: InterestTerms, pub interest: InterestState,
}
impl FundingFacility {
    pub fn check(&self, key: Pubkey) -> Result<()> {
        let (p, bump) = Pubkey::find_program_address(&[b"funding-facility", self.financier.as_ref(), &self.facility_id], &CORE_ID);
        require!(self.version == FUNDED_VERSION && self.revision > 0 && key == p && self.bump == bump,
            CoreError::FundingFacility);
        require!(self.external == (self.financier != self.issuer), CoreError::FundingFacility);
        plus(self.reserved, self.drawn)?; math(self.interest.validate(&self.interest_terms))?;
        Ok(())
    }
}

#[account]
#[derive(InitSpace)]
pub struct CustomerCreditLine {
    pub version: u16, pub bump: u8, pub line_id: [u8; 32],
    pub company: Pubkey, pub issuer: Pubkey, pub issuer_custody: Pubkey,
    pub facility: Pubkey, pub mint: Pubkey, pub limit: u64,
    pub reserved: u64, pub principal_outstanding: u64, pub fee_exposure: u64,
    /// Lifetime cap allocation is distinct from outstanding risk usage.
    pub allocated_fee_caps: u64,
    pub interest_terms: InterestTerms, pub enabled: bool, pub revision: u64,
    pub assignment_policy_hash: [u8; 32],
}
impl CustomerCreditLine {
    pub fn check(&self, key: Pubkey) -> Result<()> {
        let (p, bump) = Pubkey::find_program_address(&[b"credit-line", self.company.as_ref(), &self.line_id], &CORE_ID);
        require!(self.version == FUNDED_VERSION && self.revision > 0 && key == p && self.bump == bump,
            CoreError::FundingCreditLine);
        plus(plus(self.reserved, self.principal_outstanding)?, self.fee_exposure)?;
        require!(self.allocated_fee_caps <= self.interest_terms.maximum_interest, CoreError::FundingChargeCap);
        Ok(())
    }
}

#[account]
#[derive(InitSpace)]
pub struct FundedClaim {
    pub version: u16, pub bump: u8, pub terms: TabTerms,
    /// Accepted completed-run price preimage, available directly to readers.
    pub usage_namespace: [u8;32], pub service_unit: [u8;32], pub unit_price: u64,
    pub external_contract_hash: [u8;32],
    pub acceptance_hash: [u8; 32], pub credit_line: Pubkey,
    pub assignment_policy_hash: [u8; 32], pub facility: Pubkey, pub custody: Pubkey,
    pub issuer: Pubkey, pub issuer_custody: Pubkey, pub supplier_principal: Pubkey,
    pub remaining: u64, pub payable: u64, pub debt: u64, pub paid: u64,
    pub repaid: u64, pub released: u64, pub revision: u64,
    pub fee_cap: u64, pub fee_authority_remaining: u64, pub allocated_run_fee_caps: u64,
    pub released_fee_caps: u64, pub released_unallocated_fee_caps: u64,
    pub interest_outstanding: u64, pub interest_terms: InterestTerms,
    /// Immutable parent allocation. Accepted servicing never consults the parent.
    pub seller_fee_agreement: Pubkey, pub seller_fee_allocation: u64,
    pub seller_fee_terms: SellerFeeTerms, pub seller_fee_state: SellerFeeState,
    /// Append-only v2 extension. Captured principal and cash history never rewind.
    pub principal_extinguished: u64,
    pub corrections: CorrectionAmounts,
}
impl FundedClaim {
    pub fn check(&self, key: Pubkey) -> Result<()> {
        let (p,bump)=Pubkey::find_program_address(&[b"claim", self.terms.principal.as_ref(), &self.terms.tab_id], &CORE_ID);
        require!(self.version == FUNDED_CLAIM_VERSION && self.revision > 0 && p == key && bump == self.bump,
            CoreError::FundingClaim);
        let captured = self.captured_total()?;
        require!(plus(plus(self.remaining, self.released)?, captured)? == self.terms.cap,
            CoreError::FundingConservation);
        require!(plus(plus(self.payable, self.paid)?, self.corrections.unpaid_reduced)? == captured,
            CoreError::FundingConservation);
        math(self.corrections.validate(captured, self.paid, &PrincipalAmounts {
            originated: captured, outstanding: self.debt, repaid: self.repaid,
            extinguished: self.principal_extinguished,
        }))?;
        require!(self.allocated_run_fee_caps <= self.fee_cap && self.released_fee_caps <= self.fee_cap
            && self.released_unallocated_fee_caps <= self.released_fee_caps, CoreError::FundingChargeCap);
        math(self.seller_fee_state.validate(&self.seller_fee_terms))?;
        crate::seller_fees::check_claim_allocation(self.supplier_principal,&self.seller_fee_terms,
            self.seller_fee_agreement,self.seller_fee_allocation)?;
        Ok(())
    }
    pub fn captured_total(&self) -> Result<u64> {
        plus(plus(self.debt, self.repaid)?, self.principal_extinguished)
    }
    pub fn authority_terms(&self) -> Result<TabTerms> {
        let mut terms=self.terms.clone(); terms.cap=plus(terms.cap,self.fee_cap)?; Ok(terms)
    }
}

#[account]
#[derive(InitSpace)]
pub struct FundedRun {
    pub version: u16, pub bump: u8, pub operation_id: [u8; 32], pub claim: Pubkey,
    pub namespace: [u8; 32], pub run_id: [u8; 32], pub service_unit: [u8; 32],
    pub units: u64, pub unit_price: u64, pub supplier: Pubkey, pub supplier_principal: Pubkey,
    pub debtor: Pubkey, pub mint: Pubkey,
    pub gross_earned: u64, pub gross_paid: u64,
    pub current_creditor: Pubkey, pub creditor_custody: Pubkey, pub ownership_revision: u64,
    pub principal: PrincipalAmounts, pub interest_terms: InterestTerms, pub interest: InterestState,
    pub acquisition_basis: u64, pub provenance_hash: [u8; 32],
    pub capture_receipt: Pubkey, pub capture_digest: [u8; 32], pub fee_authority_released: u64,
    /// Original delivery, gross collection and principal repayments remain historical.
    pub corrections: CorrectionAmounts,
}
impl FundedRun {
    pub fn check(&self, key: Pubkey, claim: Pubkey, c: &FundedClaim) -> Result<()> {
        let (p,bump)=funded_run_address(&self.supplier_principal,&self.namespace,&self.run_id);
        require!(self.version == FUNDED_RUN_VERSION && key == p && self.bump == bump && self.claim == claim
            && self.supplier == c.terms.supplier && self.supplier_principal == c.supplier_principal
            && self.debtor == c.terms.principal && self.mint == c.terms.asset
            && self.operation_id == funded_run_operation_id(&self.supplier_principal,&self.namespace,&self.run_id),
            CoreError::FundingRun);
        math(self.principal.validate())?; math(self.interest.validate(&self.interest_terms))?;
        require!(self.principal.originated == self.gross_earned && self.gross_paid <= self.gross_earned,
            CoreError::FundingConservation);
        math(self.corrections.validate(self.gross_earned, self.gross_paid, &self.principal))?;
        Ok(())
    }
}

#[account]
#[derive(InitSpace)]
pub struct FundedPayment {
    pub version:u16, pub bump:u8, pub company:Pubkey, pub payment_id:[u8;32],
    pub run:Pubkey, pub receipt:Pubkey, pub request_digest:[u8;32],
}

#[account]
#[derive(InitSpace)]
pub struct SellerFeeCredits {
    pub version:u16, pub bump:u8, pub original_receipt:Pubkey, pub credited:u64,
}

#[account]
#[derive(InitSpace)]
pub struct FacilityPolicyReceipt {
    pub version:u16, pub bump:u8, pub facility:Pubkey, pub financier:Pubkey,
    pub request_id:[u8;32], pub operation_id:[u8;32], pub request_digest:[u8;32],
    pub owner:Pubkey, pub authority_generation:u64,
    pub previous_revision:u64, pub resulting_revision:u64,
    pub previous_limit:u64, pub resulting_limit:u64,
    pub previously_enabled:bool, pub resulting_enabled:bool,
}

pub fn math<T>(v: FinanceResult<T>) -> Result<T> { v.map_err(|_| error!(CoreError::FundingTerms)) }
pub fn valid_principal(p: &Account<Principal>) -> Result<()> {
    let (key,bump)=Pubkey::find_program_address(&[b"principal",p.seed_owner.as_ref(),&p.principal_id],&REGISTRY_ID);
    require!(p.version == contracts::SCHEMA_VERSION && p.key() == key && p.bump == bump, CoreError::FundingPrincipal);
    Ok(())
}
pub fn owner(p: &Account<Principal>, signer: Pubkey, generation: u64) -> Result<()> {
    valid_principal(p)?;
    require!(p.owner == signer && p.authority_generation == generation, CoreError::FundingOwner); Ok(())
}
pub fn custody_token(c: &Account<FinancierCustody>, token: &Account<TokenAccount>, mint: Pubkey) -> Result<()> {
    c.check_key(c.key())?;
    require!(c.mint == mint && c.token_account == token.key() && token.mint == mint && token.owner == c.wallet,
        CoreError::FundingTokenRouting);
    require_keys_eq!(token.key(), associated_token::get_associated_token_address(&c.wallet,&mint), CoreError::FundingTokenRouting);
    c.check(token.amount)
}
pub fn admit_cash(token: &Account<TokenAccount>) -> Result<()> {
    require!(token.state == token::spl_token::state::AccountState::Initialized
        && token.delegate.is_none() && token.close_authority.is_none(), CoreError::FundingCashControl); Ok(())
}
pub fn reservation_matches(r: &Account<Reservation>, c: &FundedClaim) -> Result<()> {
    let expected=c.authority_terms()?;
    require!(r.terms == expected && r.terms_digest == contracts::terms_digest(&expected)?, CoreError::FundingReservation);
    r.check()
}
pub fn replay(receipt: &Account<FinanceReceipt>, digest: &[u8;32]) -> Result<bool> {
    if receipt.data.version == 0 { return Ok(false); }
    require!(receipt.data.version == FUNDED_VERSION && receipt.data.request_digest == *digest,
        CoreError::FundingReplay);
    dexter_contracts::funded::read_finance_receipt(&receipt.to_account_info())?; Ok(true)
}
pub fn record_base(principal:Pubkey,id:[u8;32],digest:[u8;32],kind:u8,mint:Pubkey,
    claim:Pubkey,run:Pubkey,amount:u64)->FinanceReceiptData {
    FinanceReceiptData{version:FUNDED_VERSION,principal,operation_id:id,request_digest:digest,
        kind,mint,claim,run,amount,..FinanceReceiptData::default()}
}

pub fn transfer_checked_exact<'info>(from: &AccountInfo<'info>, destination:&AccountInfo<'info>,
    authority:&AccountInfo<'info>, mint:&Account<'info,Mint>, token_program:Pubkey,
    amount:u64, seeds:&[&[&[u8]]])->Result<()> {
    if amount==0 { return Ok(()); }
    require_keys_neq!(from.key(),destination.key(),CoreError::FundingTokenRouting);
    let read=|info:&AccountInfo|->Result<TokenAccount>{
        require_keys_eq!(*info.owner,token::ID,CoreError::FundingTokenRouting);
        TokenAccount::try_deserialize(&mut &info.try_borrow_data()?[..])
    };
    let before=read(from)?; let dest_before=read(destination)?;
    require!(before.mint == mint.key() && dest_before.mint == mint.key(),CoreError::FundingTokenRouting);
    token::transfer_checked(CpiContext::new_with_signer(token_program,token::TransferChecked{
        from:from.clone(),mint:mint.to_account_info(),to:destination.clone(),authority:authority.clone(),
    },seeds),amount,mint.decimals)?;
    let after=read(from)?;let dest_after=read(destination)?;
    require!(after.amount == minus(before.amount,amount)? && dest_after.amount == plus(dest_before.amount,amount)?,CoreError::FundingTokenDelta);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dexter_contracts::commercial::{COMMERCIAL_VERSION, FeeFailurePolicy, RepaymentPriority};
    use dexter_contracts::financing::Asset;

    fn accepted_run(supplier_principal: Pubkey, payment_wallet: Pubkey, tab_id: [u8; 32])
        -> (Pubkey, FundedClaim, Pubkey, FundedRun) {
        let company=Pubkey::new_from_array([20;32]);
        let mint=Pubkey::new_from_array([21;32]);
        let issuer=Pubkey::new_from_array([22;32]);
        let issuer_custody=Pubkey::new_from_array([23;32]);
        let namespace=[24;32];let run_id=[25;32];
        let (claim_key,claim_bump)=Pubkey::find_program_address(&[b"claim",company.as_ref(),&tab_id],&CORE_ID);
        let interest_terms=InterestTerms{version:COMMERCIAL_VERSION,annual_rate_bps:0,seconds_per_year:1,
            maximum_interest:0,accrual_cutoff:i64::MAX,protocol_share_bps:0,protocol_beneficiary:Pubkey::default(),
            protocol_destination:Pubkey::default(),repayment_priority:RepaymentPriority::InterestFirst,stop_on_shortfall:false};
        let terms=TabTerms{domain:[1;32],tab_id,principal:company,actor:Pubkey::new_unique(),mandate:Pubkey::new_unique(),
            asset:mint,supplier:payment_wallet,source:Pubkey::new_unique(),cap:10,expires_at:100,
            credential_generation:1,mandate_revision:1,external_terms_hash:[2;32]};
        let seller_fee_terms=SellerFeeTerms{version:COMMERCIAL_VERSION,agreement_hash:[3;32],supplier:payment_wallet,
            asset:Asset{mint,token_program:token::ID},gross_cap:10,rate_bps:0,fixed_fee:0,aggregate_fee_cap:0,
            maximum_deduction_bps:0,beneficiary:Pubkey::default(),beneficiary_wallet:Pubkey::default(),
            destination:Pubkey::default(),failure_policy:FeeFailurePolicy::WaiveUnavailable};
        let claim=FundedClaim{version:FUNDED_CLAIM_VERSION,bump:claim_bump,terms,usage_namespace:namespace,
            service_unit:[4;32],unit_price:1,external_contract_hash:[5;32],acceptance_hash:[6;32],
            credit_line:Pubkey::new_unique(),assignment_policy_hash:[0;32],facility:Pubkey::new_unique(),
            custody:Pubkey::new_unique(),issuer,issuer_custody,supplier_principal,remaining:0,payable:10,debt:10,
            paid:0,repaid:0,released:0,revision:2,fee_cap:0,fee_authority_remaining:0,allocated_run_fee_caps:0,
            released_fee_caps:0,released_unallocated_fee_caps:0,interest_outstanding:0,interest_terms:interest_terms.clone(),
            seller_fee_agreement:Pubkey::default(),seller_fee_allocation:0,seller_fee_terms,seller_fee_state:SellerFeeState::default(),principal_extinguished:0,corrections:CorrectionAmounts::default()};
        let (run_key,bump)=funded_run_address(&supplier_principal,&namespace,&run_id);
        let operation_id=funded_run_operation_id(&supplier_principal,&namespace,&run_id);
        let run=FundedRun{version:FUNDED_RUN_VERSION,bump,operation_id,claim:claim_key,namespace,run_id,
            service_unit:claim.service_unit,units:10,unit_price:1,supplier:payment_wallet,supplier_principal,
            debtor:company,mint,gross_earned:10,gross_paid:0,current_creditor:issuer,creditor_custody:issuer_custody,
            ownership_revision:1,principal:PrincipalAmounts{originated:10,outstanding:10,repaid:0,extinguished:0},
            interest:InterestState::new(1,&interest_terms).unwrap(),interest_terms,acquisition_basis:10,
            provenance_hash:[7;32],capture_receipt:dexter_contracts::funded::receipt_address(&company,&operation_id),
            capture_digest:[8;32],fee_authority_released:0,corrections:CorrectionAmounts::default()};
        claim.check(claim_key).unwrap();run.check(run_key,claim_key,&claim).unwrap();
        (claim_key,claim,run_key,run)
    }

    #[test]
    fn supplier_owner_change_preserves_usage_identity_and_original_payment_binding() {
        let supplier=Pubkey::new_unique();let first_owner=Pubkey::new_unique();let next_owner=Pubkey::new_unique();
        let (first_claim_key,mut first_claim,first_key,first_run)=accepted_run(supplier,first_owner,[31;32]);
        let (next_claim_key,next_claim,next_key,next_run)=accepted_run(supplier,next_owner,[32;32]);
        // Separately accepted Tabs for one delivered run converge on one usage
        // address and financial identity even when the accepted wallets differ.
        assert_eq!(first_key,next_key);
        assert_eq!(first_run.operation_id,next_run.operation_id);
        assert_ne!(first_claim.terms.supplier,next_claim.terms.supplier);
        // Convergence does not authorize moving the existing bill to a new Tab
        // or rewriting its accepted supplier payment wallet.
        assert!(first_run.check(first_key,next_claim_key,&next_claim).is_err());
        first_claim.terms.supplier=next_owner;
        assert!(first_run.check(first_key,first_claim_key,&first_claim).is_err());
    }

    #[test]
    fn distinct_supplier_principals_with_one_owner_keep_independent_usage_identity() {
        let owner=Pubkey::new_unique();
        let (claim_key,claim,key,mut run)=accepted_run(Pubkey::new_unique(),owner,[33;32]);
        let (_,other_claim,other_key,other_run)=accepted_run(Pubkey::new_unique(),owner,[34;32]);
        assert_eq!(claim.terms.supplier,other_claim.terms.supplier);
        assert_ne!(key,other_key);
        assert_ne!(run.operation_id,other_run.operation_id);
        // State inspection authenticates the same Principal-derived operation
        // identity that the earning instruction records.
        run.operation_id=other_run.operation_id;
        assert!(run.check(key,claim_key,&claim).is_err());
    }
}
