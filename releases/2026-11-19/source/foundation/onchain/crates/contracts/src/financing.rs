//! Prospective financing contracts. These are NOT deployed account layouts or
//! authorization handlers. See docs/financing/FINANCIER-CONTRACT.md for the
//! required account authentication, atomic token checks, book effects and SDK work.
//! Pure amount checks below do not grant authority or qualify a wallet graph.
use anchor_lang::prelude::*;
use solana_sha256_hasher::hashv;
use crate::{terms_digest, TabTerms, CORE_ID};

pub const FINANCING_VERSION: u16 = 1;
pub const CUSTODY_RESERVE_SEED: &[u8] = b"custody-reserve";
pub type FinanceResult<T> = core::result::Result<T, FinanceError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FinanceError {
    Arithmetic, ZeroAmount, CashShortfall, LimitExceeded, InvalidAmounts,
    WrongBinding, WrongTerms, WrongRevision, WrongCreditor, DuplicateFee,
    SchemaVersion, Encoding,
}

fn add(a: u64, b: u64) -> FinanceResult<u64> { a.checked_add(b).ok_or(FinanceError::Arithmetic) }
fn sub(a: u64, b: u64) -> FinanceResult<u64> { a.checked_sub(b).ok_or(FinanceError::Arithmetic) }
fn positive(n: u64) -> FinanceResult<()> {
    if n == 0 { Err(FinanceError::ZeroAmount) } else { Ok(()) }
}
fn digest<T: AnchorSerialize>(tag: &[u8], value: &T) -> FinanceResult<[u8; 32]> {
    let mut bytes = Vec::new();
    value.serialize(&mut bytes).map_err(|_| FinanceError::Encoding)?;
    Ok(hashv(&[tag, CORE_ID.as_ref(), &bytes]).to_bytes())
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq, InitSpace)]
pub struct Asset { pub mint: Pubkey, pub token_program: Pubkey }

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq, InitSpace)]
pub enum BackingProfile { CoreEscrowCash, GovernedWalletCash }

/// External funding creates a distinct issuer -> financier obligation on draw.
/// Using the issuer's own cash must not create an issuer -> itself debt.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq, InitSpace)]
pub enum FundingArrangement { IssuerCapital, IssuerFacility { facility: Pubkey } }

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct CustodyBinding {
    pub source: Pubkey,
    pub asset: Asset,
    pub token_account: Pubkey,
    pub reserve_ledger: Pubkey,
    pub beneficial_cash_owner: Pubkey,
    pub settlement_authority: Pubkey,
    pub profile: BackingProfile,
    pub controller_program: Pubkey,
    pub wallet_binding: Pubkey,
    pub policy_revision: u64,
    pub governance_profile_hash: [u8; 32],
}

/// Exactly one Core ledger for the actual custody token account. Facility,
/// source alias, mint input, wallet generation and policy revision are NOT seeds.
/// Runtime readers must still verify the token program/mint, owner and PDA body.
pub fn custody_reserve_address(token_account: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[CUSTODY_RESERVE_SEED, token_account.as_ref()], &CORE_ID).0
}

impl CustodyBinding {
    pub fn validate_shape(&self) -> FinanceResult<()> {
        if self.reserve_ledger != custody_reserve_address(&self.token_account) {
            return Err(FinanceError::WrongBinding);
        }
        Ok(())
    }
}

/// One authoritative aggregate over ALL facilities using this token account.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Default, Debug, PartialEq, Eq, InitSpace)]
pub struct CustodyAmounts { pub unused: u64, pub earned_unpaid: u64 }

impl CustodyAmounts {
    pub fn protected(&self) -> FinanceResult<u64> { add(self.unused, self.earned_unpaid) }
    /// This is cash headroom, not withdrawal authorization or investor equity.
    pub fn uncommitted_cash(&self, actual_cash: u64) -> FinanceResult<u64> {
        actual_cash.checked_sub(self.protected()?).ok_or(FinanceError::CashShortfall)
    }
    pub fn reserve(self, amount: u64, actual_cash: u64) -> FinanceResult<Self> {
        positive(amount)?;
        if amount > self.uncommitted_cash(actual_cash)? { return Err(FinanceError::CashShortfall); }
        Ok(Self { unused: add(self.unused, amount)?, ..self })
    }
    pub fn earn(self, amount: u64) -> FinanceResult<Self> {
        positive(amount)?;
        let next = Self { unused: sub(self.unused, amount)?, earned_unpaid: add(self.earned_unpaid, amount)? };
        next.protected()?;
        Ok(next)
    }
    /// Runtime must transfer exactly this amount to the bound supplier in the
    /// same transaction. Buyer debt and external facility debt are separate.
    pub fn collect(self, amount: u64) -> FinanceResult<Self> {
        positive(amount)?; self.protected()?;
        Ok(Self { earned_unpaid: sub(self.earned_unpaid, amount)?, ..self })
    }
    /// Wallet profile: removes a reservation; no fictitious token transfer.
    pub fn release(self, amount: u64) -> FinanceResult<Self> {
        positive(amount)?; self.protected()?;
        Ok(Self { unused: sub(self.unused, amount)?, ..self })
    }
}

/// Risk usage, separate from Registry lifetime expenditure and owned assets.
/// Agreed charge exposure includes the fee amounts the accepted credit policy
/// requires counting. It must not double-count principal or supplier payable.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Default, Debug, PartialEq, Eq, InitSpace)]
pub struct CustomerCreditUsage {
    pub unused_commitments: u64,
    pub principal_outstanding: u64,
    pub agreed_charge_exposure: u64,
}
impl CustomerCreditUsage {
    pub fn committed(&self) -> FinanceResult<u64> {
        add(add(self.unused_commitments, self.principal_outstanding)?, self.agreed_charge_exposure)
    }
    pub fn headroom(&self, limit: u64) -> FinanceResult<u64> { Ok(limit.saturating_sub(self.committed()?)) }
}

/// Provider -> issuer principal commitment: reserved cash plus already advanced
/// facility principal. Company repayment is NOT repayment of this obligation.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Default, Debug, PartialEq, Eq, InitSpace)]
pub struct IssuerFacilityUsage { pub cash_reserved: u64, pub funded_principal: u64 }
impl IssuerFacilityUsage {
    pub fn committed(&self) -> FinanceResult<u64> { add(self.cash_reserved, self.funded_principal) }
    pub fn headroom(&self, limit: u64) -> FinanceResult<u64> { Ok(limit.saturating_sub(self.committed()?)) }
    pub fn draw_for_supplier(self, amount: u64) -> FinanceResult<Self> {
        positive(amount)?; self.committed()?;
        Ok(Self { cash_reserved: sub(self.cash_reserved, amount)?, funded_principal: add(self.funded_principal, amount)? })
    }
    pub fn repay_financier(self, amount: u64) -> FinanceResult<Self> {
        positive(amount)?; self.committed()?;
        Ok(Self { funded_principal: sub(self.funded_principal, amount)?, ..self })
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct FinancierFacilityTerms {
    pub version: u16,
    pub domain: [u8; 32],
    pub facility: Pubkey,
    pub financier_principal: Pubkey,
    pub issuer_obligor: Pubkey,
    pub asset: Asset,
    pub reserve_ledger: Pubkey,
    /// Principal limit applies to cash_reserved + funded_principal.
    pub principal_commitment_limit: u64,
    pub new_commitments_from: i64,
    pub new_commitments_until: i64,
    /// Includes repayment/maturity/revolving policy, eligibility and covenants.
    pub terms_hash: [u8; 32],
    pub fee_schedule_hash: [u8; 32],
    pub revision: u64,
}
impl FinancierFacilityTerms {
    pub fn digest(&self) -> FinanceResult<[u8; 32]> {
        if self.version != FINANCING_VERSION { return Err(FinanceError::SchemaVersion); }
        if self.new_commitments_until <= self.new_commitments_from
            || self.financier_principal == self.issuer_obligor {
            return Err(FinanceError::WrongTerms);
        }
        digest(b"DEXTER:FINANCIER-FACILITY:1", self)
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct FacilityAdmission {
    pub facility_terms_hash: [u8; 32],
    pub revision: u64,
    /// Disabling admission never disables accepted-right servicing.
    pub accepts_new_commitments: bool,
    pub usage: IssuerFacilityUsage,
}

/// B07: the finite supplier guarantee and its accepted service/custody profile.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct FundingCertificate {
    pub version: u16,
    pub domain: [u8; 32],
    pub guarantee: Pubkey,
    pub custody: CustodyBinding,
    pub arrangement: FundingArrangement,
    pub original_tab_terms_hash: [u8; 32],
    pub supplier: Pubkey,
    pub finite_amount: u64,
    pub release_policy_hash: [u8; 32],
    /// Permanent claim-servicing authority, not a current owner approval ticket.
    pub service_grant_hash: [u8; 32],
    pub admitted_deployment_hash: [u8; 32],
    /// Immutable accepted facility terms for external funding; zero for own capital.
    pub accepted_facility_terms_hash: [u8; 32],
}
impl FundingCertificate {
    pub fn digest(&self) -> FinanceResult<[u8; 32]> {
        if self.version != FINANCING_VERSION { return Err(FinanceError::SchemaVersion); }
        positive(self.finite_amount)?;
        self.custody.validate_shape()?;
        digest(b"DEXTER:FUNDING-CERTIFICATE:1", self)
    }
}

/// B08 belongs to Core's financial admission directory, not PrincipalRegistry.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct IssuerAdmission {
    pub issuer_principal: Pubkey,
    pub controlling_program: Pubkey,
    pub controlling_version: u32,
    pub eligibility_risk_decision_hash: [u8; 32],
    pub pricing_terms_hash: [u8; 32],
    pub accepted_credit_amount: u64,
    pub funding_certificate_hash: [u8; 32],
}

/// Separate envelope: never overwrite TabTerms.external_terms_hash, which the
/// completed-run profile already uses for its namespace/unit/price policy.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct FinancingAcceptance {
    pub funding: FundingCertificate,
    pub issuer: IssuerAdmission,
    pub customer_credit_terms_hash: [u8; 32],
    pub buyer_fee_schedule_hash: [u8; 32],
    pub buyer_fee_cap: u64,
    /// Subset of buyer fees counted against issuer credit; cash-paid charges
    /// need expenditure consent but must not silently enlarge financed debt.
    pub credit_fee_cap: u64,
    pub assignment_and_disclosure_policy_hash: [u8; 32],
}
impl FinancingAcceptance {
    pub fn authorized_charge_cap(&self) -> FinanceResult<u64> {
        add(self.funding.finite_amount, self.buyer_fee_cap)
    }
    /// Structural consistency only. Runtime verifies required parties' consent,
    /// admitted B07/B08 issuers, authority budget, source liquidity and risk books.
    pub fn validate_against(&self, tab: &TabTerms, facility: Option<&FinancierFacilityTerms>) -> FinanceResult<()> {
        let funding_hash = self.funding.digest()?;
        let tab_hash = terms_digest(tab).map_err(|_| FinanceError::Encoding)?;
        if self.funding.domain != tab.domain || self.funding.original_tab_terms_hash != tab_hash
            || self.funding.custody.source != tab.source || self.funding.custody.asset.mint != tab.asset
            || self.funding.supplier != tab.supplier || self.funding.finite_amount != tab.cap
            || self.issuer.funding_certificate_hash != funding_hash {
            return Err(FinanceError::WrongTerms);
        }
        self.authorized_charge_cap()?;
        if self.credit_fee_cap > self.buyer_fee_cap
            || self.issuer.accepted_credit_amount < add(self.funding.finite_amount, self.credit_fee_cap)? {
            return Err(FinanceError::LimitExceeded);
        }
        match self.funding.arrangement {
            FundingArrangement::IssuerCapital => {
                if facility.is_some() || self.funding.custody.beneficial_cash_owner != self.issuer.issuer_principal
                    || self.funding.accepted_facility_terms_hash != [0; 32] {
                    return Err(FinanceError::WrongBinding);
                }
            }
            FundingArrangement::IssuerFacility { facility: id } => {
                let terms = facility.ok_or(FinanceError::WrongBinding)?;
                if terms.facility != id || terms.domain != self.funding.domain
                    || terms.issuer_obligor != self.issuer.issuer_principal
                    || terms.financier_principal != self.funding.custody.beneficial_cash_owner
                    || terms.asset != self.funding.custody.asset || terms.reserve_ledger != self.funding.custody.reserve_ledger
                    || terms.digest()? != self.funding.accepted_facility_terms_hash {
                    return Err(FinanceError::WrongBinding);
                }
                if self.funding.finite_amount > terms.principal_commitment_limit { return Err(FinanceError::LimitExceeded); }
            }
        }
        Ok(())
    }
    pub fn digest(&self, tab: &TabTerms, facility: Option<&FinancierFacilityTerms>) -> FinanceResult<[u8; 32]> {
        self.validate_against(tab, facility)?;
        digest(b"DEXTER:FINANCING-ACCEPTANCE:1", self)
    }
}

/// Principal title is independent from the original supplier payable/backing.
/// Original basis and adjustments need durable book effects; impairment alone
/// does not forgive the company's debt or change these principal amounts.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct ReceivableTitle {
    pub lot: Pubkey,
    pub economic_origin: [u8; 32],
    pub accepted_guarantee: Pubkey,
    pub debtor_principal: Pubkey,
    pub asset: Asset,
    pub current_creditor: Pubkey,
    pub repayment_destination: Pubkey,
    pub creditor_book: Pubkey,
    pub ownership_revision: u64,
    pub acquisition_basis: u64,
    pub provenance_hash: [u8; 32],
    pub assignment_and_disclosure_policy_hash: [u8; 32],
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Default, Debug, PartialEq, Eq, InitSpace)]
pub struct PrincipalAmounts {
    pub originated: u64,
    pub outstanding: u64,
    pub repaid: u64,
    /// Only legally/economically extinguishing, authenticated corrections.
    pub extinguished: u64,
}
impl PrincipalAmounts {
    pub fn validate(&self) -> FinanceResult<()> {
        if add(add(self.outstanding, self.repaid)?, self.extinguished)? != self.originated {
            return Err(FinanceError::InvalidAmounts);
        }
        Ok(())
    }
    pub fn repay(self, amount: u64) -> FinanceResult<Self> {
        self.validate()?; positive(amount)?;
        let next = Self { outstanding: sub(self.outstanding, amount)?, repaid: add(self.repaid, amount)?, ..self };
        next.validate()?; Ok(next)
    }
}

/// Whole remaining lot only. Partial assignments must first split residual title
/// with conserved amounts/lineage; one mutable creditor cannot represent two.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct ResidualAssignment {
    pub lot: Pubkey,
    pub expected_ownership_revision: u64,
    pub seller: Pubkey,
    pub buyer: Pubkey,
    pub buyer_destination: Pubkey,
    pub buyer_book: Pubkey,
    pub residual_principal: u64,
    pub consideration: u64,
    /// Actual eligible acquisition costs, authenticated with the purchase.
    pub capitalized_acquisition_costs: u64,
    pub acquisition_cost_evidence_hash: [u8; 32],
    pub acquisition_basis: u64,
    /// Explicit list/receipts of transferred fee rights; never inferred from title.
    pub fee_transfer_set_hash: [u8; 32],
    pub accepted_assignment_and_disclosure_policy_hash: [u8; 32],
}
impl ResidualAssignment {
    /// Does not authorize a sale or move consideration. Runtime atomically
    /// authenticates policy/parties, cash consideration and all affected books.
    pub fn apply_title(&self, old: &ReceivableTitle, amounts: &PrincipalAmounts) -> FinanceResult<ReceivableTitle> {
        amounts.validate()?; positive(self.residual_principal)?;
        if self.lot != old.lot || self.residual_principal != amounts.outstanding
            || self.accepted_assignment_and_disclosure_policy_hash != old.assignment_and_disclosure_policy_hash {
            return Err(FinanceError::WrongTerms);
        }
        if self.expected_ownership_revision != old.ownership_revision { return Err(FinanceError::WrongRevision); }
        if self.seller != old.current_creditor { return Err(FinanceError::WrongCreditor); }
        if self.acquisition_basis != add(self.consideration, self.capitalized_acquisition_costs)? {
            return Err(FinanceError::InvalidAmounts);
        }
        let provenance_hash = digest(b"DEXTER:TITLE-TRANSFER:1", &(old.provenance_hash, self))?;
        Ok(ReceivableTitle { current_creditor: self.buyer, repayment_destination: self.buyer_destination,
            creditor_book: self.buyer_book, ownership_revision: add(old.ownership_revision, 1)?,
            acquisition_basis: self.acquisition_basis, provenance_hash, ..old.clone() })
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq, InitSpace)]
pub enum FeeKind { Protocol, FinancierCommitment, FinancierCredit, Assignment }
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq, InitSpace)]
pub enum Rounding { Down, Up }

/// An explicit fee entitlement, not an unexplained addition to principal.
/// Its terms commit trigger, basis, rate/clock and partial-payment allocation.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct FeeTerms {
    pub entitlement: Pubkey,
    /// Accepted customer agreement, issuer facility or assignment agreement.
    pub accepted_agreement_hash: [u8; 32],
    pub obligation: Pubkey,
    pub charge_origin: [u8; 32],
    pub liable_principal: Pubkey,
    pub original_beneficiary: Pubkey,
    pub asset: Asset,
    pub kind: FeeKind,
    pub schedule_trigger_basis_hash: [u8; 32],
    pub allocation_policy_hash: [u8; 32],
    pub rounding: Rounding,
    pub maximum_charge: u64,
    pub maximum_credit_charge: u64,
    pub transfer_policy_hash: [u8; 32],
}

/// Authoritative parent-agreement aggregate, not a caller-selected fee list.
/// Allocate an entitlement's caps once. Collection does not refill these caps.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Default, Debug, PartialEq, Eq, InitSpace)]
pub struct FeeCapUsage { pub allocated_buyer_cap: u64, pub allocated_credit_cap: u64 }
impl FeeCapUsage {
    pub fn allocate(self, buyer_cap: u64, credit_cap: u64, accepted: &FinancingAcceptance) -> FinanceResult<Self> {
        if credit_cap > buyer_cap || self.allocated_credit_cap > self.allocated_buyer_cap {
            return Err(FinanceError::InvalidAmounts);
        }
        let next = Self { allocated_buyer_cap: add(self.allocated_buyer_cap, buyer_cap)?,
            allocated_credit_cap: add(self.allocated_credit_cap, credit_cap)? };
        if next.allocated_buyer_cap > accepted.buyer_fee_cap || next.allocated_credit_cap > accepted.credit_fee_cap {
            return Err(FinanceError::LimitExceeded);
        }
        Ok(next)
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct FeeTitle {
    pub entitlement: Pubkey,
    pub beneficiary: Pubkey,
    pub destination: Pubkey,
    pub book: Pubkey,
    pub ownership_revision: u64,
}

/// Monotonic gross counters. Refunds do not reopen a paid debt; distribution
/// recoveries must correspond to returned cash. Economic corrections are events.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Default, Debug, PartialEq, Eq, InitSpace)]
pub struct FeeAmounts {
    pub accrued: u64,
    pub waived: u64,
    pub collected: u64,
    pub refunded: u64,
    pub distributed: u64,
    pub recovered_distributions: u64,
}
impl FeeAmounts {
    pub fn outstanding(&self) -> FinanceResult<u64> { sub(sub(self.accrued, self.waived)?, self.collected) }
    pub fn held_proceeds(&self) -> FinanceResult<u64> {
        sub(sub(self.collected, self.refunded)?, sub(self.distributed, self.recovered_distributions)?)
    }
    pub fn validate(&self, cap: u64) -> FinanceResult<()> {
        self.outstanding()?; self.held_proceeds()?;
        if self.accrued > cap { return Err(FinanceError::LimitExceeded); }
        Ok(())
    }
    pub fn accrue(self, amount: u64, cap: u64) -> FinanceResult<Self> {
        positive(amount)?; self.validate(cap)?;
        let next = Self { accrued: add(self.accrued, amount)?, ..self }; next.validate(cap)?; Ok(next)
    }
    pub fn collect(self, amount: u64) -> FinanceResult<Self> {
        positive(amount)?; self.validate(u64::MAX)?;
        let next = Self { collected: add(self.collected, amount)?, ..self }; next.validate(u64::MAX)?; Ok(next)
    }
    pub fn distribute(self, amount: u64) -> FinanceResult<Self> {
        positive(amount)?; self.validate(u64::MAX)?;
        let next = Self { distributed: add(self.distributed, amount)?, ..self }; next.validate(u64::MAX)?; Ok(next)
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct FeePayment {
    pub entitlement: Pubkey,
    pub expected_ownership_revision: u64,
    pub beneficiary: Pubkey,
    pub destination: Pubkey,
    pub amount: u64,
}

/// Exact debit allocation; the handler also validates each fee's asset, liable
/// principal, residual, title, membership in the accepted agreement/schedule,
/// aggregate charge cap and accepted waterfall, and all token destinations.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct RepaymentAllocation {
    pub lot: Pubkey,
    pub expected_ownership_revision: u64,
    pub current_creditor: Pubkey,
    pub principal_destination: Pubkey,
    pub principal: u64,
    pub fees: Vec<FeePayment>,
    pub actual_debit: u64,
}
impl RepaymentAllocation {
    pub fn validate_shape(&self, title: &ReceivableTitle, amounts: &PrincipalAmounts) -> FinanceResult<()> {
        amounts.validate()?; positive(self.actual_debit)?;
        if self.lot != title.lot || self.principal_destination != title.repayment_destination { return Err(FinanceError::WrongBinding); }
        if self.expected_ownership_revision != title.ownership_revision { return Err(FinanceError::WrongRevision); }
        if self.current_creditor != title.current_creditor { return Err(FinanceError::WrongCreditor); }
        if self.principal > amounts.outstanding { return Err(FinanceError::LimitExceeded); }
        let mut total = self.principal;
        for (i, fee) in self.fees.iter().enumerate() {
            positive(fee.amount)?;
            if self.fees[..i].iter().any(|previous| previous.entitlement == fee.entitlement) { return Err(FinanceError::DuplicateFee); }
            total = add(total, fee.amount)?;
        }
        if total != self.actual_debit { return Err(FinanceError::InvalidAmounts); }
        Ok(())
    }
}

/// Canonical payable range, authenticated by an existing earned entitlement.
/// There is no arbitrary global caller-owned ID to pre-allocate. Replay checks
/// the receipt first; otherwise paid_before must equal the lot's paid counter
/// and amount must not exceed its current collectible payable after adjustments.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct EarnedCollection {
    pub version: u16,
    pub domain: [u8; 32],
    pub claim: Pubkey,
    pub payable_lot: Pubkey,
    pub accepted_service_grant_hash: [u8; 32],
    pub reserve_ledger: Pubkey,
    pub token_account: Pubkey,
    pub asset: Asset,
    pub supplier: Pubkey,
    pub destination: Pubkey,
    pub paid_before: u64,
    pub amount: u64,
}
impl EarnedCollection {
    /// Read the authenticated residual after corrections/offsets, not historical
    /// gross earnings or the shared custody balance for unrelated suppliers.
    pub fn validate_unpaid_range(&self, remaining_payable: u64, already_paid: u64) -> FinanceResult<()> {
        self.operation_id()?;
        if self.paid_before != already_paid { return Err(FinanceError::WrongRevision); }
        if self.amount > remaining_payable { return Err(FinanceError::LimitExceeded); }
        Ok(())
    }
    /// Fixed economic identity across relayers, lost responses, attempt nonces,
    /// blockhashes and optional Pool projection. New domain; no old receipt reuse.
    pub fn operation_id(&self) -> FinanceResult<[u8; 32]> {
        if self.version != FINANCING_VERSION { return Err(FinanceError::SchemaVersion); }
        positive(self.amount)?; add(self.paid_before, self.amount)?;
        if self.reserve_ledger != custody_reserve_address(&self.token_account) { return Err(FinanceError::WrongBinding); }
        digest(b"DEXTER:EARNED-COLLECTION:1", self)
    }
}

/// Attempt authorization is generated from public chain state and the accepted
/// grant. No fresh financier/Dexter signer or private nonce service is permitted.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct CollectionAttempt {
    pub economic_operation_id: [u8; 32],
    pub relayer: Pubkey,
    pub nonce: [u8; 32],
    pub expires_at: i64,
    pub expected_claim_revision: u64,
    pub wallet_binding_revision: u64,
    pub service_without_pool: bool,
    /// Commits exact ordered downstream instructions, effective metas and data.
    pub execution_bundle_hash: [u8; 32],
}
impl CollectionAttempt {
    pub fn authorization_digest(&self) -> FinanceResult<[u8; 32]> {
        digest(b"DEXTER:COLLECTION-ATTEMPT:1", self)
    }
}
