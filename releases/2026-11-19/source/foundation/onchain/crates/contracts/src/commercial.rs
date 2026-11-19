//! Accepted commercial terms and pure arithmetic. These helpers do not grant
//! authority, authenticate cash, qualify an investment adapter, or move tokens.
//! Runtime callers bind the terms to the agreement and authenticate every leg.
use anchor_lang::prelude::*;
use solana_sha256_hasher::hashv;
use crate::{CORE_ID, financing::{Asset, FinanceError, FinanceResult}};

pub const COMMERCIAL_VERSION: u16 = 1;
pub const BPS_DENOMINATOR: u64 = 10_000;

fn add(a: u64, b: u64) -> FinanceResult<u64> { a.checked_add(b).ok_or(FinanceError::Arithmetic) }
fn sub(a: u64, b: u64) -> FinanceResult<u64> { a.checked_sub(b).ok_or(FinanceError::InvalidAmounts) }
fn positive(n: u64) -> FinanceResult<()> { if n == 0 { Err(FinanceError::ZeroAmount) } else { Ok(()) } }
fn hash<T: AnchorSerialize>(tag: &[u8], data: &T) -> FinanceResult<[u8; 32]> {
    crate::funded::financial_digest(tag, data).map_err(|_| FinanceError::Encoding)
}
fn mul_div(a: u64, b: u64, denominator: u64, up: bool) -> FinanceResult<u64> {
    if denominator == 0 { return Err(FinanceError::WrongTerms); }
    let numerator = (a as u128) * (b as u128);
    let d = denominator as u128;
    let value = numerator / d + u128::from(up && numerator % d != 0);
    u64::try_from(value).map_err(|_| FinanceError::Arithmetic)
}
fn asset_valid(asset: &Asset) -> FinanceResult<()> {
    if asset.mint == Pubkey::default() || asset.token_program == Pubkey::default() {
        Err(FinanceError::WrongTerms)
    } else { Ok(()) }
}

/// A missing/frozen canonical fee destination, or actual balance below its Core
/// accounted cash, waives this deduction and leaves that custody unchanged.
/// Waiver neither repairs the pre-existing deficit nor forgives another debt.
/// A wrong address, owner or mint is a binding failure, never unavailability.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq, InitSpace)]
pub enum FeeFailurePolicy { WaiveUnavailable }

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct SellerFeeTerms {
    pub version: u16,
    pub agreement_hash: [u8; 32],
    pub supplier: Pubkey,
    pub asset: Asset,
    pub gross_cap: u64,
    pub rate_bps: u16,
    /// Applied once to cumulative settlement, never once per run or retry.
    pub fixed_fee: u64,
    pub aggregate_fee_cap: u64,
    /// Cumulative upper slope; each settlement's maximum rounds UP by at most
    /// one token base unit, while the total fee rounds DOWN cumulatively.
    pub maximum_deduction_bps: u16,
    pub beneficiary: Pubkey,
    pub beneficiary_wallet: Pubkey,
    pub destination: Pubkey,
    pub failure_policy: FeeFailurePolicy,
}
impl SellerFeeTerms {
    pub fn validate(&self) -> FinanceResult<()> {
        if self.version != COMMERCIAL_VERSION { return Err(FinanceError::SchemaVersion); }
        asset_valid(&self.asset)?;
        if self.supplier == Pubkey::default() || self.agreement_hash == [0; 32]
            || self.rate_bps as u64 > BPS_DENOMINATOR
            || self.maximum_deduction_bps as u64 > BPS_DENOMINATOR {
            return Err(FinanceError::WrongTerms);
        }
        if self.aggregate_fee_cap > 0 && (self.beneficiary == Pubkey::default()
            || self.beneficiary_wallet == Pubkey::default() || self.destination == Pubkey::default()) {
            return Err(FinanceError::WrongBinding);
        }
        Ok(())
    }
    pub fn digest(&self) -> FinanceResult<[u8; 32]> {
        self.validate()?; hash(b"DEXTER:SELLER-FEE:1", self)
    }
    pub fn cumulative_fee(&self, gross: u64) -> FinanceResult<u64> {
        self.validate()?;
        if gross > self.gross_cap { return Err(FinanceError::LimitExceeded); }
        if gross == 0 { return Ok(0); }
        let proportional = mul_div(gross, self.rate_bps as u64, BPS_DENOMINATOR, false)?;
        // An accepted fixed component can exceed the realizable fee. Saturate
        // only inside the independently bounded cap, never amount accounting.
        let nominal = (proportional as u128) + (self.fixed_fee as u128);
        let deduction = mul_div(gross, self.maximum_deduction_bps as u64, BPS_DENOMINATOR, false)?;
        Ok(nominal.min(self.aggregate_fee_cap as u128).min(deduction as u128) as u64)
    }
}

/// Parent agreement allowance, distinct from buyer-financing FeeCapUsage.
/// Allocations are lifetime authorizations; settlement/refund never refills it.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Default, Debug, PartialEq, Eq, InitSpace)]
pub struct SellerFeeCapUsage { pub allocated: u64 }
impl SellerFeeCapUsage {
    pub fn allocate(self, amount: u64, accepted_aggregate_cap: u64) -> FinanceResult<Self> {
        let allocated = add(self.allocated, amount)?;
        if allocated > accepted_aggregate_cap { return Err(FinanceError::LimitExceeded); }
        Ok(Self { allocated })
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Default, Debug, PartialEq, Eq, InitSpace)]
pub struct SellerFeeState {
    pub gross_settled: u64,
    pub collected: u64,
    pub waived: u64,
    /// Economic refund and voluntary fee credit are distinct returned-cash facts.
    pub refunded: u64,
    pub credited: u64,
}
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Default, Debug, PartialEq, Eq, InitSpace)]
pub struct SellerFeeSettlement { pub gross: u64, pub supplier_net: u64, pub fee: u64, pub waived: u64 }
impl SellerFeeState {
    pub fn validate(&self, terms: &SellerFeeTerms) -> FinanceResult<()> {
        if add(self.collected, self.waived)? != terms.cumulative_fee(self.gross_settled)?
            || add(self.refunded, self.credited)? > self.collected {
            return Err(FinanceError::InvalidAmounts);
        }
        Ok(())
    }
    pub fn settle(self, gross: u64, terms: &SellerFeeTerms, destination_available: bool)
        -> FinanceResult<(Self, SellerFeeSettlement)> {
        positive(gross)?; self.validate(terms)?;
        let gross_settled = add(self.gross_settled, gross)?;
        let due = sub(terms.cumulative_fee(gross_settled)?, add(self.collected, self.waived)?)?;
        let maximum = mul_div(gross, terms.maximum_deduction_bps as u64, BPS_DENOMINATOR, true)?;
        if due > maximum || due > gross { return Err(FinanceError::InvalidAmounts); }
        let (fee, waived) = if destination_available { (due, 0) } else { (0, due) };
        let next = Self { gross_settled, collected: add(self.collected, fee)?,
            waived: add(self.waived, waived)?, ..self };
        next.validate(terms)?;
        Ok((next, SellerFeeSettlement { gross, supplier_net: sub(gross, fee)?, fee, waived }))
    }
    pub fn refund(self, amount: u64, terms: &SellerFeeTerms) -> FinanceResult<Self> {
        positive(amount)?; self.validate(terms)?;
        let next = Self { refunded: add(self.refunded, amount)?, ..self }; next.validate(terms)?; Ok(next)
    }
    pub fn credit(self, amount: u64, terms: &SellerFeeTerms) -> FinanceResult<Self> {
        positive(amount)?; self.validate(terms)?;
        let next = Self { credited: add(self.credited, amount)?, ..self }; next.validate(terms)?; Ok(next)
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq, InitSpace)]
pub enum RepaymentPriority { InterestFirst, PrincipalFirst }

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct InterestTerms {
    pub version: u16,
    pub annual_rate_bps: u32,
    /// Explicit year basis; no implicit production APR or spread policy.
    pub seconds_per_year: u32,
    pub maximum_interest: u64,
    pub accrual_cutoff: i64,
    pub protocol_share_bps: u16,
    pub protocol_beneficiary: Pubkey,
    pub protocol_destination: Pubkey,
    pub repayment_priority: RepaymentPriority,
    /// The cash runtime rejects this until it authenticates historical shortfall
    /// windows. Today's balance cannot establish yesterday's eligible duration.
    pub stop_on_shortfall: bool,
}
impl InterestTerms {
    pub fn validate(&self) -> FinanceResult<()> {
        if self.version != COMMERCIAL_VERSION { return Err(FinanceError::SchemaVersion); }
        if self.seconds_per_year == 0 || self.protocol_share_bps as u64 > BPS_DENOMINATOR
            || self.stop_on_shortfall { return Err(FinanceError::WrongTerms); }
        if self.protocol_share_bps > 0 && (self.protocol_beneficiary == Pubkey::default()
            || self.protocol_destination == Pubkey::default()) { return Err(FinanceError::WrongBinding); }
        Ok(())
    }
    pub fn digest(&self) -> FinanceResult<[u8; 32]> {
        self.validate()?; hash(b"DEXTER:INTEREST-TERMS:1", self)
    }
    fn denominator(&self) -> u128 { BPS_DENOMINATOR as u128 * self.seconds_per_year as u128 }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Default, Debug, PartialEq, Eq, InitSpace)]
pub struct InterestState {
    pub last_accrued_at: i64,
    pub accrued: u64,
    pub collected: u64,
    pub waived: u64,
    pub fractional_numerator: u64,
    pub protocol_collected: u64,
    pub protocol_waived: u64,
}
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Default, Debug, PartialEq, Eq, InitSpace)]
pub struct InterestAllocation { pub interest: u64, pub protocol: u64, pub lender: u64, pub protocol_waived: u64 }
impl InterestState {
    pub fn new(now: i64, terms: &InterestTerms) -> FinanceResult<Self> {
        terms.validate()?; Ok(Self { last_accrued_at: now, ..Self::default() })
    }
    pub fn outstanding(&self) -> FinanceResult<u64> { sub(sub(self.accrued, self.waived)?, self.collected) }
    pub fn validate(&self, terms: &InterestTerms) -> FinanceResult<()> {
        terms.validate()?; self.outstanding()?;
        if self.accrued > terms.maximum_interest || self.fractional_numerator as u128 >= terms.denominator()
            || (self.accrued == terms.maximum_interest && self.fractional_numerator != 0)
            || add(self.protocol_collected, self.protocol_waived)?
                != mul_div(self.collected, terms.protocol_share_bps as u64, BPS_DENOMINATOR, false)? {
            return Err(FinanceError::InvalidAmounts);
        }
        Ok(())
    }
    /// Call before every principal change. Interest is simple: accrued interest
    /// is not principal, and prior fractions retain their agreed ownership.
    pub fn accrue(self, principal: u64, now: i64, terms: &InterestTerms) -> FinanceResult<(Self, u64)> {
        self.validate(terms)?;
        if now < self.last_accrued_at { return Err(FinanceError::WrongRevision); }
        let end = now.min(terms.accrual_cutoff);
        let elapsed = (end as i128 - self.last_accrued_at as i128).max(0) as u128;
        let remaining = terms.maximum_interest - self.accrued;
        let mut next = Self { last_accrued_at: now, ..self };
        if remaining == 0 { next.fractional_numerator = 0; return Ok((next, 0)); }
        if elapsed == 0 || principal == 0 || terms.annual_rate_bps == 0 { return Ok((next, 0)); }
        let denominator = terms.denominator();
        let per_second = principal as u128 * terms.annual_rate_bps as u128;
        // Bound multiplication against the finite cap first. This supports all
        // u64 principals/u32 rates/i64 clocks without overflowing elapsed*rate.
        let cap_numerator = remaining as u128 * denominator;
        let needed = cap_numerator - self.fractional_numerator as u128;
        let seconds_to_cap = needed / per_second + u128::from(needed % per_second != 0);
        let delta;
        if elapsed >= seconds_to_cap {
            delta = remaining; next.fractional_numerator = 0;
        } else {
            let numerator = per_second * elapsed + self.fractional_numerator as u128;
            delta = (numerator / denominator) as u64;
            next.fractional_numerator = (numerator % denominator) as u64;
        }
        next.accrued = add(self.accrued, delta)?; next.validate(terms)?; Ok((next, delta))
    }
    pub fn collect(self, amount: u64, terms: &InterestTerms, protocol_destination_available: bool)
        -> FinanceResult<(Self, InterestAllocation)> {
        positive(amount)?; self.validate(terms)?;
        if amount > self.outstanding()? { return Err(FinanceError::LimitExceeded); }
        let collected = add(self.collected, amount)?;
        let total_protocol = mul_div(collected, terms.protocol_share_bps as u64, BPS_DENOMINATOR, false)?;
        let due = sub(total_protocol, add(self.protocol_collected, self.protocol_waived)?)?;
        let (protocol, protocol_waived) = if protocol_destination_available { (due, 0) } else { (0, due) };
        let next = Self { collected, protocol_collected: add(self.protocol_collected, protocol)?,
            protocol_waived: add(self.protocol_waived, protocol_waived)?, ..self };
        next.validate(terms)?;
        // A waived protocol take stays with the lender; borrower interest does
        // not change, and a later collection cannot revive the waived take.
        Ok((next, InterestAllocation { interest: amount, protocol, lender: sub(amount, protocol)?, protocol_waived }))
    }
    pub fn waive(self, amount: u64, terms: &InterestTerms) -> FinanceResult<Self> {
        positive(amount)?; self.validate(terms)?;
        let next = Self { waived: add(self.waived, amount)?, ..self }; next.validate(terms)?; Ok(next)
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Default, Debug, PartialEq, Eq, InitSpace)]
pub struct RepaymentSplit { pub principal: u64, pub interest: u64 }
pub fn split_repayment(amount: u64, principal: u64, interest_due: u64, priority: RepaymentPriority)
    -> FinanceResult<RepaymentSplit> {
    positive(amount)?;
    if amount as u128 > principal as u128 + interest_due as u128 { return Err(FinanceError::LimitExceeded); }
    Ok(match priority {
        RepaymentPriority::InterestFirst => { let interest = amount.min(interest_due); RepaymentSplit { principal: amount - interest, interest } },
        RepaymentPriority::PrincipalFirst => { let paid_principal = amount.min(principal); RepaymentSplit { principal: paid_principal, interest: amount - paid_principal } },
    })
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq, InitSpace)]
pub enum SponsorFundingSource { OwnerCash, EarnedSupplierProceeds }

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct SponsorQuote {
    pub version: u16,
    pub operation_id: [u8; 32],
    pub funding_source: SponsorFundingSource,
    pub payer_principal: Pubkey,
    pub payer_authority: Pubkey,
    pub authority_generation: u64,
    pub asset: Asset,
    pub from: Pubkey,
    pub destination: Pubkey,
    pub sponsor_principal: Pubkey,
    pub rent_owner: Pubkey,
    pub approved_at: i64,
    pub expires_at: i64,
    pub native_cost_lamports: u64,
    /// Approved conversion into the bound token's base units per lamport.
    pub conversion_numerator: u64,
    pub conversion_denominator: u64,
    pub reimbursement: u64,
    pub margin: u64,
    pub maximum_total_charge: u64,
    pub maximum_margin: u64,
    pub price_quote_hash: [u8; 32],
}
impl SponsorQuote {
    pub fn total(&self) -> FinanceResult<u64> { add(self.reimbursement, self.margin) }
    pub fn validate_shape(&self) -> FinanceResult<()> {
        if self.version != COMMERCIAL_VERSION { return Err(FinanceError::SchemaVersion); }
        asset_valid(&self.asset)?;
        if self.payer_principal == Pubkey::default() || self.payer_authority == Pubkey::default()
            || self.from == Pubkey::default() || self.destination == Pubkey::default()
            || self.sponsor_principal == Pubkey::default() || self.rent_owner == Pubkey::default()
            || self.price_quote_hash == [0; 32] || self.expires_at <= self.approved_at
            || self.conversion_numerator == 0 || self.conversion_denominator == 0 {
            return Err(FinanceError::WrongTerms);
        }
        if self.reimbursement != mul_div(self.native_cost_lamports, self.conversion_numerator, self.conversion_denominator, true)?
            || self.margin > self.maximum_margin || self.total()? > self.maximum_total_charge {
            return Err(FinanceError::LimitExceeded);
        }
        positive(self.total()?)
    }
    pub fn validate_at(&self, now: i64) -> FinanceResult<()> {
        self.validate_shape()?;
        if now < self.approved_at || now > self.expires_at { return Err(FinanceError::WrongTerms); }
        Ok(())
    }
    pub fn digest(&self) -> FinanceResult<[u8; 32]> { self.validate_shape()?; hash(b"DEXTER:SPONSOR-QUOTE:1", self) }
}

/// Exact owner-approved policy identifier. Zero means no assignment; an
/// arbitrary nonzero hash does not authorize this profile. Transfer stays in
/// one Registry domain and transfers the whole remaining lot, with no new
/// private-identity disclosure or change to original supplier entitlement.
pub fn within_domain_assignment_policy(domain: &[u8; 32]) -> [u8; 32] {
    hashv(&[b"DEXTER:WITHIN-DOMAIN-WHOLE-RESIDUAL:1", CORE_ID.as_ref(), domain]).to_bytes()
}

/// Policy contract only: a runtime must separately qualify its adapter,
/// redemption authority and actual availability before reserving productive
/// positions. Nominal value or a wallet balance is not a liquidity proof.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct ProductiveBackingPolicy {
    pub version: u16,
    pub asset: Asset,
    pub beneficial_owner: Pubkey,
    pub income_owner: Pubkey,
    pub loss_bearer: Pubkey,
    pub adapter_program: Pubkey,
    pub withdrawal_destination: Pubkey,
    pub ownership_policy_hash: [u8; 32],
    pub liquidity_policy_hash: [u8; 32],
    pub loss_policy_hash: [u8; 32],
    pub minimum_liquid_cash: u64,
    pub redemption_window_seconds: u32,
    pub maximum_valuation_age_seconds: u32,
    pub valuation_haircut_bps: u16,
}
impl ProductiveBackingPolicy {
    pub fn validate(&self) -> FinanceResult<()> {
        if self.version != COMMERCIAL_VERSION { return Err(FinanceError::SchemaVersion); }
        asset_valid(&self.asset)?;
        if self.beneficial_owner == Pubkey::default() || self.income_owner == Pubkey::default()
            || self.loss_bearer == Pubkey::default() || self.adapter_program == Pubkey::default()
            || self.withdrawal_destination == Pubkey::default() || self.ownership_policy_hash == [0; 32]
            || self.liquidity_policy_hash == [0; 32] || self.loss_policy_hash == [0; 32]
            || self.valuation_haircut_bps as u64 > BPS_DENOMINATOR
            || self.redemption_window_seconds == 0 || self.maximum_valuation_age_seconds == 0 {
            return Err(FinanceError::WrongTerms);
        }
        Ok(())
    }
    pub fn digest(&self) -> FinanceResult<[u8; 32]> { self.validate()?; hash(b"DEXTER:PRODUCTIVE-BACKING:1", self) }
    /// Assumes the caller authenticates that the input can be redeemed inside
    /// the accepted window. Returns a valuation bound, not spendable cash.
    pub fn eligible_redemption_value(&self, redeemable_value: u64, valued_at: i64, now: i64) -> FinanceResult<u64> {
        self.validate()?;
        let age = now as i128 - valued_at as i128;
        if age < 0 || age > self.maximum_valuation_age_seconds as i128 { return Err(FinanceError::WrongRevision); }
        mul_div(redeemable_value, BPS_DENOMINATOR - self.valuation_haircut_bps as u64, BPS_DENOMINATOR, false)
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Default, Debug, PartialEq, Eq, InitSpace)]
pub struct RealizedBackingIncome { pub net_income: u64, pub uncovered_loss: u64, pub protocol: u64, pub owner: u64 }
pub fn allocate_realized_backing_income(collected_yield: u64, realized_loss: u64, protocol_share_bps: u16)
    -> FinanceResult<RealizedBackingIncome> {
    if protocol_share_bps as u64 > BPS_DENOMINATOR { return Err(FinanceError::WrongTerms); }
    let net_income = collected_yield.saturating_sub(realized_loss);
    let uncovered_loss = realized_loss.saturating_sub(collected_yield);
    let protocol = mul_div(net_income, protocol_share_bps as u64, BPS_DENOMINATOR, false)?;
    Ok(RealizedBackingIncome { net_income, uncovered_loss, protocol, owner: net_income - protocol })
}
