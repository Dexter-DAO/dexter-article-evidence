//! Accepted source-report billing for one finite, identified work lot.
//!
//! Authentication establishes what the named source reported. It does not prove
//! truthful hidden work, satisfactory output, or organizational independence.
//! These shared types and calculations neither authenticate signatures nor move
//! money. Core must bind the accepted state, verify each detached signature,
//! retain actual admission times, enforce immutable replay, and atomically
//! establish the bill before releasing unused backing.
use anchor_lang::prelude::*;
use solana_sha256_hasher::hashv;

use crate::{financing::{FinanceError, FinanceResult}, CORE_ID};

pub const BILLING_POLICY_VERSION: u8 = 1;
pub const MAX_BILLING_METRICS: usize = 8;
pub const FINAL_FOR_BILLING: u8 = 1;
pub const COMPLETED_WORK: u8 = 1;

/// SHA-256("DEXTER:EVIDENCE-BILLING-NAMESPACE:1"). Reserved for the native
/// evidence profile; a legacy supplier report must not select this namespace.
pub const BILLING_NAMESPACE: [u8; 32] = [
    4, 121, 250, 145, 120, 232, 191, 155, 108, 90, 236, 0, 102, 219, 211, 197,
    135, 209, 229, 156, 100, 67, 56, 102, 78, 207, 147, 247, 232, 102, 241, 138,
];
/// SHA-256("DEXTER:EVIDENCE-BILLING-FINANCIAL-ATOMS:1"). Existing FundedRun
/// records use financial atoms at price one; actual metering stays in the bill.
pub const BILLING_SETTLEMENT_UNIT: [u8; 32] = [
    33, 159, 138, 41, 129, 252, 238, 185, 198, 118, 93, 124, 10, 110, 94, 38,
    97, 186, 232, 158, 131, 3, 87, 106, 205, 209, 204, 54, 96, 199, 120, 96,
];

/// All fields are immutable once accepted. Metric order is part of the tariff.
/// Rates are in settlement-asset base units per metric unit. Each component
/// rounds up separately; the resulting u128 sum is bounded by cap and claim
/// before conversion to a token amount.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct BillingPolicy {
    pub schema_version: u8,
    pub source: Pubkey,
    /// Accepted source identity, control relationships and assurance disclosure.
    /// A disclosure hash is not native proof of organizational independence.
    pub source_identity: [u8; 32],
    pub work_id: [u8; 32],
    pub request_hash: [u8; 32],
    pub work_until: i64,
    pub evidence_deadline: i64,
    pub claim_deadline: i64,
    pub metric_count: u8,
    pub metric_ids: [[u8; 32]; MAX_BILLING_METRICS],
    pub price_numerators: [u64; MAX_BILLING_METRICS],
    pub price_denominators: [u64; MAX_BILLING_METRICS],
    pub max_units: [u64; MAX_BILLING_METRICS],
}

/// One immutable final report. The signature binds this body to its accepted
/// policy and exact bill/claim context. Failure/retry/quality judgments are not
/// admitted by this completed-work profile.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct SourceReport {
    pub record_id: [u8; 32],
    pub request_hash: [u8; 32],
    pub started_at: i64,
    pub completed_at: i64,
    pub units: [u64; MAX_BILLING_METRICS],
    pub result_hash: [u8; 32],
    pub finality: u8,
    pub outcome: u8,
}

/// The supplier's immutable final monetary claim. Zero explicitly claims
/// nothing. The accepted supplier key authenticates it, independently of the
/// source record and the identity of the submitter or finalizer.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct SellerClaim {
    pub claim_id: [u8; 32],
    pub amount: u64,
}

impl BillingPolicy {
    /// Validate the accepted policy against the actual on-chain work admission
    /// time. Deadlines have a representable instant strictly after them, so a
    /// permissionless finalizer cannot be permanently excluded by i64::MAX.
    pub fn validate(&self, admitted_at: i64) -> FinanceResult<()> {
        self.validate_shape()?;
        if admitted_at < 0 || self.work_until <= admitted_at {
            return Err(FinanceError::WrongTerms);
        }
        Ok(())
    }

    fn validate_shape(&self) -> FinanceResult<()> {
        if self.schema_version != BILLING_POLICY_VERSION {
            return Err(FinanceError::SchemaVersion);
        }
        if self.source == Pubkey::default() || self.source_identity == [0; 32]
            || self.work_id == [0; 32] || self.request_hash == [0; 32]
        {
            return Err(FinanceError::WrongBinding);
        }
        if self.work_until <= 0 || self.evidence_deadline < self.work_until
            || self.claim_deadline < self.work_until
            || self.finalization_deadline() == i64::MAX
        {
            return Err(FinanceError::WrongTerms);
        }
        let count = self.metric_count as usize;
        if count == 0 || count > MAX_BILLING_METRICS {
            return Err(FinanceError::WrongTerms);
        }
        let mut positive_rate = false;
        let mut maximum = 0u128;
        for i in 0..MAX_BILLING_METRICS {
            if i >= count {
                if self.metric_ids[i] != [0; 32] || self.price_numerators[i] != 0
                    || self.price_denominators[i] != 0 || self.max_units[i] != 0
                {
                    return Err(FinanceError::WrongTerms);
                }
                continue;
            }
            if self.metric_ids[i] == [0; 32] || self.price_denominators[i] == 0
                || self.max_units[i] == 0
                || self.metric_ids[..i].contains(&self.metric_ids[i])
            {
                return Err(FinanceError::WrongTerms);
            }
            positive_rate |= self.price_numerators[i] != 0;
            // Validate the whole admitted range before work. Capping an
            // overflowing sum afterward would make accepted arithmetic unsafe.
            maximum = maximum.checked_add(component_amount(
                self.max_units[i], self.price_numerators[i], self.price_denominators[i],
            )?).ok_or(FinanceError::Arithmetic)?;
        }
        if !positive_rate {
            return Err(FinanceError::WrongTerms);
        }
        Ok(())
    }

    /// Exact supported monetary value, potentially larger than u64. The caller
    /// applies the accepted order cap and authenticated seller claim before any
    /// conversion to the settlement amount.
    pub fn supported_amount(&self, units: &[u64; MAX_BILLING_METRICS]) -> FinanceResult<u128> {
        self.validate_shape()?;
        self.amount_for_units(units)
    }

    fn amount_for_units(&self, units: &[u64; MAX_BILLING_METRICS]) -> FinanceResult<u128> {
        let mut amount = 0u128;
        for (i, &quantity) in units.iter().enumerate() {
            if i >= self.metric_count as usize {
                if quantity != 0 { return Err(FinanceError::WrongTerms); }
                continue;
            }
            if quantity > self.max_units[i] { return Err(FinanceError::LimitExceeded); }
            amount = amount.checked_add(component_amount(
                quantity, self.price_numerators[i], self.price_denominators[i],
            )?).ok_or(FinanceError::Arithmetic)?;
        }
        Ok(amount)
    }

    /// Validate a *fresh* report admission. Core must first recover an already
    /// stored exact replay; applying today's deadline to an admitted record
    /// would incorrectly erase accepted evidence after the cutoff.
    pub fn validate_report(&self, report: &SourceReport, admitted_at: i64,
        submitted_at: i64) -> FinanceResult<u128>
    {
        self.validate(admitted_at)?;
        if report.record_id == [0; 32] || report.result_hash == [0; 32]
            || report.request_hash != self.request_hash
        {
            return Err(FinanceError::WrongBinding);
        }
        if report.finality != FINAL_FOR_BILLING || report.outcome != COMPLETED_WORK {
            return Err(FinanceError::WrongTerms);
        }
        if submitted_at < admitted_at || submitted_at > self.evidence_deadline
            || report.started_at < admitted_at || report.completed_at < report.started_at
            || report.completed_at > self.work_until || report.completed_at > submitted_at
        {
            return Err(FinanceError::WrongTerms);
        }
        self.amount_for_units(&report.units)
    }

    /// Validate a fresh supplier claim admission. Authentication and immutable
    /// replay are Core's responsibility; no current supplier key is needed to
    /// finalize an already admitted claim.
    pub fn validate_claim(&self, claim: &SellerClaim, admitted_at: i64,
        submitted_at: i64) -> FinanceResult<()>
    {
        self.validate(admitted_at)?;
        if claim.claim_id == [0; 32] { return Err(FinanceError::WrongBinding); }
        if submitted_at < admitted_at || submitted_at > self.claim_deadline {
            return Err(FinanceError::WrongTerms);
        }
        Ok(())
    }

    pub fn finalization_deadline(&self) -> i64 {
        self.evidence_deadline.max(self.claim_deadline)
    }

    /// There is no upper time limit: inactivity cannot discard timely records.
    /// Fresh admission is inclusive at its deadline; finalization is exclusive.
    pub fn finalization_ready(&self, now: i64) -> bool {
        now > self.finalization_deadline()
    }
}

fn component_amount(units: u64, numerator: u64, denominator: u64) -> FinanceResult<u128> {
    if denominator == 0 { return Err(FinanceError::WrongTerms); }
    let product = (units as u128).checked_mul(numerator as u128)
        .ok_or(FinanceError::Arithmetic)?;
    let denominator = denominator as u128;
    let whole = product / denominator;
    whole.checked_add(u128::from(product % denominator != 0)).ok_or(FinanceError::Arithmetic)
}

/// The first immutable final bill has no prior partial earning. Missing report
/// or claim is represented by a zero input only after its admission window
/// closes. Existing claims use their own previously accepted financial paths.
pub fn deterministic_bill_amount(cap: u64, seller_claim: u64, supported: u128) -> u64 {
    // Both explicit bounds fit u64; narrow only after applying them.
    supported.min(cap as u128).min(seller_claim as u128) as u64
}

/// Fixed-width Anchor/Borsh bodies are committed without a discriminator. These
/// functions define canonical bytes, not validation or signature authority.
/// Acceptance/admission callers must separately apply the checks above.
///
/// The bodies are fixed-width and at most a policy long, so they are encoded on
/// the stack: a growing Vec keeps every outgrown buffer on the native bump heap
/// (about 2 KB for one policy digest).
#[inline(never)]
fn digest<T: AnchorSerialize>(label: &[u8], domain: &[u8; 32], bill: &Pubkey,
    claim: &Pubkey, policy_hash: Option<&[u8; 32]>, value: &T) -> FinanceResult<[u8; 32]>
{
    let mut buffer = [0u8; DIGEST_BODY_LIMIT];
    let written = {
        let mut output = &mut buffer[..];
        value.serialize(&mut output).map_err(|_| FinanceError::Encoding)?;
        DIGEST_BODY_LIMIT - output.len()
    };
    let policy = policy_hash.map(|hash| hash.as_slice()).unwrap_or(&[]);
    Ok(hashv(&[label, domain, CORE_ID.as_ref(), bill.as_ref(), claim.as_ref(), policy, &buffer[..written]]).to_bytes())
}

/// The largest committed body: a policy. Reports and seller claims are smaller.
const DIGEST_BODY_LIMIT: usize = BillingPolicy::INIT_SPACE;
const _: () = assert!(SourceReport::INIT_SPACE <= DIGEST_BODY_LIMIT && SellerClaim::INIT_SPACE <= DIGEST_BODY_LIMIT);

/// The policy hash commits its own body and execution context; report and claim
/// messages include this hash, avoiding a circular policy self-reference.
pub fn billing_policy_digest(domain: &[u8; 32], bill: &Pubkey, claim: &Pubkey,
    policy: &BillingPolicy) -> FinanceResult<[u8; 32]>
{
    digest(b"DEXTER:BILLING-POLICY:1", domain, bill, claim, None, policy)
}

/// The accepted source signs this 32-byte digest, not a transaction-specific
/// wrapper. A holder may submit the same record through another fee payer.
pub fn source_report_digest(domain: &[u8; 32], bill: &Pubkey, claim: &Pubkey,
    policy_hash: &[u8; 32], report: &SourceReport) -> FinanceResult<[u8; 32]>
{
    digest(b"DEXTER:BILLING-SOURCE-REPORT:1", domain, bill, claim, Some(policy_hash), report)
}

/// The claimant key frozen at acceptance signs this 32-byte digest. A source
/// report signature cannot be reused as the supplier's monetary claim.
pub fn seller_claim_digest(domain: &[u8; 32], bill: &Pubkey, claim: &Pubkey,
    policy_hash: &[u8; 32], seller_claim: &SellerClaim) -> FinanceResult<[u8; 32]>
{
    digest(b"DEXTER:BILLING-SELLER-CLAIM:1", domain, bill, claim, Some(policy_hash), seller_claim)
}
