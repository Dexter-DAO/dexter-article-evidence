//! Explicit arithmetic for voluntary corrections of financed purchases.
//!
//! A correction credits a bill; it does not rewrite delivery, gross collection,
//! historical repayment, assignment consideration, or lifetime spending authority.
//! Programs must authenticate the affected owners, original receipts, cash routes
//! and complete book effects. These pure checks confer no such authority.
use anchor_lang::prelude::*;
use crate::financing::{FinanceError, FinanceResult, PrincipalAmounts};

pub const CORRECTION_VERSION: u16 = 1;

fn add(a: u64, b: u64) -> FinanceResult<u64> { a.checked_add(b).ok_or(FinanceError::Arithmetic) }
fn sub(a: u64, b: u64) -> FinanceResult<u64> { a.checked_sub(b).ok_or(FinanceError::InvalidAmounts) }
fn positive(amount: u64) -> FinanceResult<()> {
    if amount == 0 { Err(FinanceError::ZeroAmount) } else { Ok(()) }
}

/// Gross, monotonic facts for one run or a correctly aggregated claim.
/// `principal_credited` is this lifecycle's part of the more general
/// PrincipalAmounts.extinguished counter. A prior principal payment stays in
/// `repaid` when its actual payer later receives a correction refund.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Default, Debug, PartialEq, Eq, InitSpace)]
pub struct CorrectionAmounts {
    pub unpaid_reduced: u64,
    pub collected_returned: u64,
    pub principal_credited: u64,
    pub prior_repayment_refunded: u64,
}

/// Agreed allocation for one atomic bill correction. The original supplier's
/// collected portion is applied to current debt first. Explicit compensation
/// can make the remaining noncash creditor reduction whole; it creates no new
/// debt or implicit reimbursement right for the consenting cash funder.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Default, Debug, PartialEq, Eq, InitSpace)]
pub struct CorrectionPlan {
    pub total: u64,
    pub principal_credit: u64,
    pub prior_repayment_refund: u64,
    pub creditor_cash: u64,
    pub required_cash: u64,
    pub creditor_shortfall: u64,
    /// Earned payable backing made free; this does not release unearned work.
    pub backing_released_from_payable: u64,
}

impl CorrectionAmounts {
    pub fn validate(&self, original: u64, gross_collected: u64, principal: &PrincipalAmounts) -> FinanceResult<()> {
        principal.validate()?;
        if original != principal.originated || add(gross_collected, self.unpaid_reduced)? > original
            || self.collected_returned > gross_collected
            || self.principal_credited > principal.extinguished
            || self.prior_repayment_refunded > principal.repaid
            || add(self.unpaid_reduced, self.collected_returned)?
                != add(self.principal_credited, self.prior_repayment_refunded)? {
            return Err(FinanceError::InvalidAmounts);
        }
        Ok(())
    }

    pub fn plan(&self, principal: &PrincipalAmounts, gross_collected: u64,
        unpaid_reduction: u64, collected_return: u64, creditor_compensation: u64) -> FinanceResult<CorrectionPlan> {
        self.validate(principal.originated, gross_collected, principal)?;
        let total = add(unpaid_reduction, collected_return)?;
        // Zero principal correction is useful for a separately authorized
        // accrued-interest waiver. The handler must reject a wholly empty
        // operation; this plan alone cannot authorize or earn anything.
        if unpaid_reduction > sub(sub(principal.originated, gross_collected)?, self.unpaid_reduced)?
            || collected_return > sub(gross_collected, self.collected_returned)? {
            return Err(FinanceError::LimitExceeded);
        }
        let principal_credit = total.min(principal.outstanding);
        let prior_repayment_refund = sub(total, principal_credit)?;
        if prior_repayment_refund > sub(principal.repaid, self.prior_repayment_refunded)? {
            // A generic earlier write-off cannot be turned into a cash refund.
            return Err(FinanceError::LimitExceeded);
        }
        let collected_to_creditor = collected_return.min(principal.outstanding);
        let noncash_credit = sub(principal_credit, collected_to_creditor)?;
        if creditor_compensation > noncash_credit { return Err(FinanceError::LimitExceeded); }
        let creditor_cash = add(collected_to_creditor, creditor_compensation)?;
        Ok(CorrectionPlan {
            total, principal_credit, prior_repayment_refund, creditor_cash,
            required_cash: add(creditor_cash, prior_repayment_refund)?,
            creditor_shortfall: sub(principal_credit, creditor_cash)?,
            backing_released_from_payable: unpaid_reduction,
        })
    }

    pub fn apply(&self, principal: &PrincipalAmounts, gross_collected: u64,
        unpaid_reduction: u64, collected_return: u64, creditor_compensation: u64)
        -> FinanceResult<(Self, PrincipalAmounts, CorrectionPlan)> {
        let plan = self.plan(principal, gross_collected, unpaid_reduction, collected_return, creditor_compensation)?;
        let next = Self {
            unpaid_reduced: add(self.unpaid_reduced, unpaid_reduction)?,
            collected_returned: add(self.collected_returned, collected_return)?,
            principal_credited: add(self.principal_credited, plan.principal_credit)?,
            prior_repayment_refunded: add(self.prior_repayment_refunded, plan.prior_repayment_refund)?,
        };
        let principal_next = PrincipalAmounts {
            outstanding: sub(principal.outstanding, plan.principal_credit)?,
            extinguished: add(principal.extinguished, plan.principal_credit)?,
            ..*principal
        };
        next.validate(principal.originated, gross_collected, &principal_next)?;
        Ok((next, principal_next, plan))
    }
}

/// Cumulative use of one authenticated original collection receipt. A new
/// request ID, later payment, rotation or refund cannot reset this allowance.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Default, Debug, PartialEq, Eq, InitSpace)]
pub struct CollectionCorrectionUsage { pub collected_returned: u64 }
impl CollectionCorrectionUsage {
    pub fn record(self, amount: u64, original_collection_gross: u64) -> FinanceResult<Self> {
        positive(amount)?;
        let next = Self { collected_returned: add(self.collected_returned, amount)? };
        if next.collected_returned > original_collection_gross { return Err(FinanceError::LimitExceeded); }
        Ok(next)
    }
}

/// Cumulative cash returned against one authenticated original repayment.
/// Principal, lender interest and protocol interest have separate beneficiaries
/// and refund capacities. Collected interest and historical repayments remain
/// gross facts; refunds never restore a paid debt or expenditure authority.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Default, Debug, PartialEq, Eq, InitSpace)]
pub struct RepaymentRefundUsage {
    pub principal_refunded: u64,
    pub lender_interest_refunded: u64,
    pub protocol_interest_refunded: u64,
}
impl RepaymentRefundUsage {
    pub fn validate(&self, original_principal_paid: u64, original_interest_paid: u64,
        original_protocol_interest: u64) -> FinanceResult<()> {
        let lender = sub(original_interest_paid, original_protocol_interest)?;
        if self.principal_refunded > original_principal_paid
            || self.lender_interest_refunded > lender
            || self.protocol_interest_refunded > original_protocol_interest {
            return Err(FinanceError::LimitExceeded);
        }
        Ok(())
    }
    pub fn record(self, principal: u64, lender_interest: u64, protocol_interest: u64,
        original_principal_paid: u64, original_interest_paid: u64, original_protocol_interest: u64)
        -> FinanceResult<Self> {
        positive(add(add(principal, lender_interest)?, protocol_interest)?)?;
        self.validate(original_principal_paid, original_interest_paid, original_protocol_interest)?;
        let next = Self {
            principal_refunded: add(self.principal_refunded, principal)?,
            lender_interest_refunded: add(self.lender_interest_refunded, lender_interest)?,
            protocol_interest_refunded: add(self.protocol_interest_refunded, protocol_interest)?,
        };
        next.validate(original_principal_paid, original_interest_paid, original_protocol_interest)?;
        Ok(next)
    }
}
