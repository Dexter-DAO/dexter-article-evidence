//! Revocable authority for financed repayment. These limits confer permission
//! to spend available cash; they neither reserve cash nor guarantee a creditor.
use anchor_lang::prelude::*;
use crate::financing::{FinanceError, FinanceResult};
use crate::funded::financial_digest;
use crate::spending::ExactRequest;

pub const REPAYMENT_RUN: u8 = 3;
pub const REPAYMENT_FACILITY: u8 = 4;

/// Immutable owner-granted scope, shared by standing grants and exact approvals.
/// An exact approval adds its one request (`ExactRequest`) and permits one
/// payment. A fresh grant is required to change any route, limit, executor or
/// authority generation.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct RepaymentPermissionTerms {
    pub binding: Pubkey,
    pub kind: u8,
    pub obligation: Pubkey,
    pub obligor: Pubkey,
    pub mint: Pubkey,
    pub debit_from: Pubkey,
    pub creditor: Pubkey,
    pub creditor_custody: Pubkey,
    pub creditor_destination: Pubkey,
    pub protocol_beneficiary: Pubkey,
    pub protocol_destination: Pubkey,
    pub executor: Pubkey,
    pub authority_generation: u64,
    pub not_before: i64,
    pub expires_at: i64,
    pub minimum_interval: u64,
    pub per_payment_amount: u64,
    pub per_payment_principal: u64,
    pub per_payment_interest: u64,
    pub total_amount: u64,
    pub total_principal: u64,
    pub total_interest: u64,
    pub maximum_payments: u64,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Default, Debug, PartialEq, Eq, InitSpace)]
pub struct RepaymentConsumption {
    pub amount: u64,
    pub principal: u64,
    pub interest: u64,
    pub payments: u64,
    pub last_executed_at: i64,
}

/// Core supplies authenticated live obligation/title/route facts and the actual
/// allocation after accrual. Only Core's fixed consumer PDA can consume a grant.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct RepaymentExecution {
    pub binding: Pubkey,
    pub kind: u8,
    pub obligation: Pubkey,
    pub obligor: Pubkey,
    pub mint: Pubkey,
    pub debit_from: Pubkey,
    pub creditor: Pubkey,
    pub creditor_custody: Pubkey,
    pub creditor_destination: Pubkey,
    pub protocol_beneficiary: Pubkey,
    pub protocol_destination: Pubkey,
    pub executor: Pubkey,
    pub authority_generation: u64,
    pub request_digest: [u8; 32],
    /// The payment's number: a standing grant admits each number once through
    /// its window; under an exact approval it is the approval's number.
    pub number: u64,
    pub amount: u64,
    pub principal_paid: u64,
    pub interest_paid: u64,
}

impl RepaymentPermissionTerms {
    pub fn validate(&self) -> FinanceResult<()> {
        let required = [self.binding, self.obligation, self.obligor, self.mint,
            self.debit_from, self.creditor, self.creditor_custody, self.creditor_destination,
            self.executor];
        if !matches!(self.kind, REPAYMENT_RUN | REPAYMENT_FACILITY)
            || required.contains(&Pubkey::default()) || !self.executor.is_on_curve()
            || self.authority_generation == 0 || self.not_before < 0
            || self.expires_at <= self.not_before || self.minimum_interval > i64::MAX as u64
            || self.per_payment_amount == 0 || self.total_amount == 0
            || self.maximum_payments == 0 || self.per_payment_amount > self.total_amount
            || self.per_payment_principal > self.total_principal
            || self.per_payment_interest > self.total_interest
            || (self.per_payment_principal == 0 && self.per_payment_interest == 0)
            || ((self.protocol_beneficiary == Pubkey::default()) != (self.protocol_destination == Pubkey::default())) {
            return Err(FinanceError::WrongTerms);
        }
        Ok(())
    }

    /// Terms of an exact approval: one payment, for one request.
    pub fn validate_exact(&self, exact: &ExactRequest) -> FinanceResult<()> {
        self.validate()?;
        if exact.request_digest == [0; 32] || exact.amount == 0
            || exact.amount > self.per_payment_amount || self.maximum_payments != 1 {
            return Err(FinanceError::WrongTerms);
        }
        Ok(())
    }

    /// `consume` for an exact approval: the execution must be its one request.
    pub fn consume_exact(&self, used: RepaymentConsumption, execution: &RepaymentExecution,
        now: i64, exact: &ExactRequest) -> FinanceResult<RepaymentConsumption> {
        self.validate_exact(exact)?;
        if execution.request_digest != exact.request_digest || execution.amount != exact.amount {
            return Err(FinanceError::WrongTerms);
        }
        self.consume(used, execution, now)
    }

    pub fn consume(&self, used: RepaymentConsumption, execution: &RepaymentExecution,
        now: i64) -> FinanceResult<RepaymentConsumption> {
        self.validate()?;
        if used.principal.checked_add(used.interest) != Some(used.amount)
            || used.amount > self.total_amount || used.principal > self.total_principal
            || used.interest > self.total_interest || used.payments > self.maximum_payments
            || (used.payments == 0 && (used.amount != 0 || used.last_executed_at != 0)) {
            return Err(FinanceError::InvalidAmounts);
        }
        if self.binding != execution.binding || self.kind != execution.kind
            || self.obligation != execution.obligation || self.obligor != execution.obligor
            || self.mint != execution.mint || self.debit_from != execution.debit_from
            || self.creditor != execution.creditor || self.creditor_custody != execution.creditor_custody
            || self.creditor_destination != execution.creditor_destination
            || self.protocol_beneficiary != execution.protocol_beneficiary
            || self.protocol_destination != execution.protocol_destination
            || self.executor != execution.executor || self.authority_generation != execution.authority_generation
            || execution.request_digest == [0; 32] || execution.number == 0 || execution.amount == 0
            || execution.principal_paid.checked_add(execution.interest_paid) != Some(execution.amount)
            || execution.amount > self.per_payment_amount
            || execution.principal_paid > self.per_payment_principal
            || execution.interest_paid > self.per_payment_interest
            || now < self.not_before || now >= self.expires_at {
            return Err(FinanceError::WrongTerms);
        }
        if used.payments > 0 {
            let earliest = used.last_executed_at.checked_add(self.minimum_interval as i64)
                .ok_or(FinanceError::Arithmetic)?;
            if now < earliest { return Err(FinanceError::WrongTerms); }
        }
        let next = RepaymentConsumption {
            amount: used.amount.checked_add(execution.amount).ok_or(FinanceError::Arithmetic)?,
            principal: used.principal.checked_add(execution.principal_paid).ok_or(FinanceError::Arithmetic)?,
            interest: used.interest.checked_add(execution.interest_paid).ok_or(FinanceError::Arithmetic)?,
            payments: used.payments.checked_add(1).ok_or(FinanceError::Arithmetic)?,
            last_executed_at: now,
        };
        if next.amount > self.total_amount || next.principal > self.total_principal
            || next.interest > self.total_interest || next.payments > self.maximum_payments {
            return Err(FinanceError::WrongTerms);
        }
        Ok(next)
    }
}

// Staleness guards (the Tab's step count) and transport/permission choices do
// not create another economic payment, so they stay out of the digest: a client
// that rebuilds a recent payment against a newer step count produces the same
// step id, and the Tab refuses to apply it twice.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct RunRepaymentRequest {
    pub principal: Pubkey,
    pub claim: Pubkey,
    pub run: Pubkey,
    pub repayment_principal: Pubkey,
    pub repayment_authority: Pubkey,
    pub repayment_owner_generation: u64,
    pub from: Pubkey,
    pub creditor_custody: Pubkey,
    pub destination: Pubkey,
    pub mint: Pubkey,
    pub amount: u64,
    pub maximum_interest: u64,
    pub protocol_custody: Option<Pubkey>,
    pub protocol_destination: Option<Pubkey>,
    /// A governed payment's number (the standing grant's or the exact approval's);
    /// zero for ordinary cash.
    pub permission_number: u64,
}
pub fn run_repayment_digest(request: &RunRepaymentRequest) -> Result<[u8; 32]> {
    financial_digest(b"DEXTER:FUNDED-RUN-REPAY:2", request)
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct FacilityRepaymentRequest {
    pub issuer: Pubkey,
    pub facility: Pubkey,
    pub repayment_principal: Pubkey,
    pub repayment_authority: Pubkey,
    pub repayment_owner_generation: u64,
    pub from: Pubkey,
    pub financier_custody: Pubkey,
    pub destination: Pubkey,
    pub mint: Pubkey,
    pub amount: u64,
    pub maximum_interest: u64,
    pub protocol_custody: Option<Pubkey>,
    pub protocol_destination: Option<Pubkey>,
    pub permission_number: u64,
}
pub fn facility_repayment_digest(request: &FacilityRepaymentRequest) -> Result<[u8; 32]> {
    financial_digest(b"DEXTER:FUNDING-FACILITY-REPAY:2", request)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms() -> RepaymentPermissionTerms {
        // Canonical Ed25519 base point: fixtures must use an on-curve executor.
        let mut point = [0x66; 32]; point[0] = 0x58;
        let executor = Pubkey::new_from_array(point);
        RepaymentPermissionTerms {
            binding: Pubkey::new_unique(), kind: REPAYMENT_RUN, obligation: Pubkey::new_unique(),
            obligor: Pubkey::new_unique(), mint: Pubkey::new_unique(), debit_from: Pubkey::new_unique(),
            creditor: Pubkey::new_unique(), creditor_custody: Pubkey::new_unique(),
            creditor_destination: Pubkey::new_unique(), protocol_beneficiary: Pubkey::default(),
            protocol_destination: Pubkey::default(), executor, authority_generation: 1,
            not_before: 10, expires_at: 100, minimum_interval: 5,
            per_payment_amount: 50, per_payment_principal: 45, per_payment_interest: 10,
            total_amount: 100, total_principal: 90, total_interest: 15, maximum_payments: 3,
        }
    }
    fn execution(t: &RepaymentPermissionTerms) -> RepaymentExecution {
        RepaymentExecution { binding: t.binding, kind: t.kind, obligation: t.obligation,
            obligor: t.obligor, mint: t.mint, debit_from: t.debit_from, creditor: t.creditor,
            creditor_custody: t.creditor_custody, creditor_destination: t.creditor_destination,
            protocol_beneficiary: t.protocol_beneficiary, protocol_destination: t.protocol_destination,
            executor: t.executor, authority_generation: t.authority_generation,
            request_digest: [1; 32], number: 3, amount: 50, principal_paid: 40, interest_paid: 10 }
    }

    #[test]
    fn standing_limits_count_actual_principal_interest_and_timing() {
        let t = terms(); let x = execution(&t); let initial = RepaymentConsumption::default();
        let used = t.consume(initial, &x, 10).unwrap();
        assert_eq!((used.amount, used.principal, used.interest, used.payments), (50, 40, 10, 1));
        assert!(t.consume(used, &x, 14).is_err());
        assert!(t.consume(used, &x, 15).is_err()); // cumulative interest, not just total cash
        let mut second = x.clone(); second.principal_paid = 45; second.interest_paid = 5;
        let all = t.consume(used, &second, 15).unwrap();
        assert_eq!((all.amount, all.principal, all.interest), (100, 85, 15));
        assert!(t.consume(all, &second, 20).is_err());
        assert_eq!(used, t.consume(initial, &x, 10).unwrap()); // failed proposals do not mutate input
    }

    #[test]
    fn exact_approval_binds_request_and_is_one_use() {
        let mut t = terms(); let exact = ExactRequest { request_digest: [1; 32], amount: 50 };
        let mut x = execution(&t);
        assert!(t.consume_exact(RepaymentConsumption::default(), &x, 10, &exact).is_err()); // three payments
        t.maximum_payments = 1;
        let used = t.consume_exact(RepaymentConsumption::default(), &x, 10, &exact).unwrap();
        assert!(t.consume_exact(used, &x, 20, &exact).is_err());
        x.request_digest = [2; 32]; assert!(t.consume_exact(RepaymentConsumption::default(), &x, 10, &exact).is_err());
        x.request_digest = [1; 32]; x.amount = 49; x.principal_paid = 39;
        assert!(t.consume_exact(RepaymentConsumption::default(), &x, 10, &exact).is_err());
        x.amount = 50; x.principal_paid = 40; x.number = 0;
        assert!(t.consume_exact(RepaymentConsumption::default(), &x, 10, &exact).is_err());
    }

    #[test]
    fn authority_route_and_generation_do_not_follow_assignments_or_owner_changes() {
        let t = terms(); let x = execution(&t);
        for changed in 0..13 {
            let mut y = x.clone();
            match changed {
                0 => y.binding = Pubkey::new_unique(), 1 => y.obligation = Pubkey::new_unique(),
                2 => y.obligor = Pubkey::new_unique(), 3 => y.mint = Pubkey::new_unique(),
                4 => y.debit_from = Pubkey::new_unique(), 5 => y.creditor = Pubkey::new_unique(),
                6 => y.creditor_custody = Pubkey::new_unique(), 7 => y.creditor_destination = Pubkey::new_unique(),
                8 => y.protocol_beneficiary = Pubkey::new_unique(), 9 => y.protocol_destination = Pubkey::new_unique(),
                10 => y.executor = Pubkey::new_unique(), 11 => y.authority_generation = 2,
                _ => y.kind = REPAYMENT_FACILITY,
            }
            assert!(t.consume(RepaymentConsumption::default(), &y, 10).is_err(), "changed field {changed}");
        }
        assert!(t.consume(RepaymentConsumption::default(), &x, 9).is_err());
        assert!(t.consume(RepaymentConsumption::default(), &x, 100).is_err());
    }
}
