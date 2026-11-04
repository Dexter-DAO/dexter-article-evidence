//! Separate, revocable authority for free-cash sends and exact-input swaps.
//! A grant allocates no cash and never expands a repayment permission. Core
//! authenticates custody ownership, reserve coverage and actual token deltas.
use anchor_lang::prelude::*;
use crate::financing::{FinanceError, FinanceResult};

pub const SPENDING_SEND: u8 = 5;
pub const SPENDING_SWAP: u8 = 6;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct SpendingPermissionTerms {
    pub binding: Pubkey,
    pub kind: u8,
    pub mint: Pubkey,
    pub debit_from: Pubkey,
    pub output_mint: Pubkey,
    pub destination: Pubkey,
    pub destination_authority: Pubkey,
    pub adapter_program: Pubkey,
    pub adapter_pool: Pubkey,
    pub executor: Pubkey,
    pub authority_generation: u64,
    pub not_before: i64,
    pub expires_at: i64,
    pub minimum_interval: u64,
    pub per_operation_amount: u64,
    pub total_amount: u64,
    pub maximum_operations: u64,
    /// Smallest accepted output/input rate, in the respective token atoms.
    /// Every swap must request and receive at least ceil(input * n / d).
    pub minimum_output_numerator: u64,
    pub minimum_output_denominator: u64,
    pub exact_request_digest: [u8; 32],
    pub exact_receipt: Pubkey,
    pub exact_amount: u64,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Default, Debug, PartialEq, Eq, InitSpace)]
pub struct SpendingConsumption {
    pub amount: u64,
    pub operations: u64,
    pub last_executed_at: i64,
}

/// Only Core's Registry-consumer PDA may supply these authenticated effects.
/// The request commits the quoted minimum; amount_out is the observed result.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct SpendingExecution {
    pub binding: Pubkey,
    pub kind: u8,
    pub mint: Pubkey,
    pub debit_from: Pubkey,
    pub output_mint: Pubkey,
    pub destination: Pubkey,
    pub destination_authority: Pubkey,
    pub adapter_program: Pubkey,
    pub adapter_pool: Pubkey,
    pub executor: Pubkey,
    pub authority_generation: u64,
    pub request_digest: [u8; 32],
    pub receipt: Pubkey,
    pub amount: u64,
    pub minimum_output: u64,
    pub amount_out: u64,
}

impl SpendingPermissionTerms {
    pub fn validate(&self) -> FinanceResult<()> {
        let required = [self.binding, self.mint, self.debit_from, self.output_mint,
            self.destination, self.destination_authority, self.executor];
        if !matches!(self.kind, SPENDING_SEND | SPENDING_SWAP)
            || required.contains(&Pubkey::default()) || !self.executor.is_on_curve()
            || self.debit_from == self.destination || self.authority_generation == 0
            || self.not_before < 0 || self.expires_at <= self.not_before
            || self.minimum_interval > i64::MAX as u64 || self.per_operation_amount == 0
            || self.total_amount < self.per_operation_amount || self.maximum_operations == 0 {
            return Err(FinanceError::WrongTerms);
        }
        if self.kind == SPENDING_SEND {
            if self.output_mint != self.mint || self.adapter_program != Pubkey::default()
                || self.adapter_pool != Pubkey::default() || self.minimum_output_numerator != 0
                || self.minimum_output_denominator != 0 {
                return Err(FinanceError::WrongTerms);
            }
        } else if self.output_mint == self.mint || self.adapter_program == Pubkey::default()
            || self.adapter_pool == Pubkey::default() || self.minimum_output_numerator == 0
            || self.minimum_output_denominator == 0 {
            return Err(FinanceError::WrongTerms);
        }
        if self.exact_request_digest == [0; 32] {
            if self.exact_receipt != Pubkey::default() || self.exact_amount != 0 {
                return Err(FinanceError::WrongTerms);
            }
        } else if self.exact_receipt == Pubkey::default() || self.exact_amount == 0
            || self.exact_amount > self.per_operation_amount || self.maximum_operations != 1 {
            return Err(FinanceError::WrongTerms);
        }
        Ok(())
    }

    pub fn minimum_output_for(&self, amount: u64) -> FinanceResult<u64> {
        if self.kind == SPENDING_SEND { return Ok(0); }
        if self.kind != SPENDING_SWAP || self.minimum_output_numerator == 0
            || self.minimum_output_denominator == 0 { return Err(FinanceError::WrongTerms); }
        let product = (amount as u128).checked_mul(self.minimum_output_numerator as u128)
            .ok_or(FinanceError::Arithmetic)?;
        let denominator = self.minimum_output_denominator as u128;
        // Divide first so ceiling does not overflow by adding denominator - 1.
        let quotient = product / denominator;
        let rounded = quotient.checked_add(u128::from(product % denominator != 0))
            .ok_or(FinanceError::Arithmetic)?;
        u64::try_from(rounded).map_err(|_| FinanceError::Arithmetic)
    }

    pub fn consume(&self, used: SpendingConsumption, execution: &SpendingExecution,
        now: i64) -> FinanceResult<SpendingConsumption> {
        self.validate()?;
        if used.amount > self.total_amount || used.operations > self.maximum_operations
            || (used.operations == 0 && (used.amount != 0 || used.last_executed_at != 0))
            || (used.operations > 0 && (used.amount == 0 || used.last_executed_at < self.not_before
                || used.last_executed_at >= self.expires_at)) {
            return Err(FinanceError::InvalidAmounts);
        }
        if self.binding != execution.binding || self.kind != execution.kind
            || self.mint != execution.mint || self.debit_from != execution.debit_from
            || self.output_mint != execution.output_mint || self.destination != execution.destination
            || self.destination_authority != execution.destination_authority
            || self.adapter_program != execution.adapter_program || self.adapter_pool != execution.adapter_pool
            || self.executor != execution.executor || self.authority_generation != execution.authority_generation
            || execution.request_digest == [0; 32] || execution.receipt == Pubkey::default()
            || execution.amount == 0 || execution.amount > self.per_operation_amount
            || now < self.not_before || now >= self.expires_at {
            return Err(FinanceError::WrongTerms);
        }
        if self.kind == SPENDING_SEND {
            if execution.minimum_output != 0 || execution.amount_out != execution.amount {
                return Err(FinanceError::InvalidAmounts);
            }
        } else if execution.minimum_output == 0
            || execution.minimum_output < self.minimum_output_for(execution.amount)?
            || execution.amount_out < execution.minimum_output {
            return Err(FinanceError::WrongTerms);
        }
        if self.exact_request_digest != [0; 32]
            && (self.exact_request_digest != execution.request_digest
                || self.exact_receipt != execution.receipt || self.exact_amount != execution.amount) {
            return Err(FinanceError::WrongTerms);
        }
        if used.operations > 0 {
            let earliest = used.last_executed_at.checked_add(self.minimum_interval as i64)
                .ok_or(FinanceError::Arithmetic)?;
            if now < earliest { return Err(FinanceError::WrongTerms); }
        }
        let next = SpendingConsumption {
            amount: used.amount.checked_add(execution.amount).ok_or(FinanceError::Arithmetic)?,
            operations: used.operations.checked_add(1).ok_or(FinanceError::Arithmetic)?,
            last_executed_at: now,
        };
        if next.amount > self.total_amount || next.operations > self.maximum_operations {
            return Err(FinanceError::WrongTerms);
        }
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn terms(kind: u8) -> SpendingPermissionTerms {
        let mut point = [0x66; 32]; point[0] = 0x58;
        let mint = Pubkey::new_unique();
        SpendingPermissionTerms {
            binding: Pubkey::new_unique(), kind, mint, debit_from: Pubkey::new_unique(),
            output_mint: if kind == SPENDING_SEND { mint } else { Pubkey::new_unique() },
            destination: Pubkey::new_unique(), destination_authority: Pubkey::new_unique(),
            adapter_program: if kind == SPENDING_SEND { Pubkey::default() } else { Pubkey::new_unique() },
            adapter_pool: if kind == SPENDING_SEND { Pubkey::default() } else { Pubkey::new_unique() },
            executor: Pubkey::new_from_array(point), authority_generation: 1,
            not_before: 10, expires_at: 100, minimum_interval: 5,
            per_operation_amount: 50, total_amount: 90, maximum_operations: 3,
            minimum_output_numerator: if kind == SPENDING_SEND { 0 } else { 2 },
            minimum_output_denominator: if kind == SPENDING_SEND { 0 } else { 3 },
            exact_request_digest: [0; 32], exact_receipt: Pubkey::default(), exact_amount: 0,
        }
    }
    fn execution(t: &SpendingPermissionTerms, amount: u64) -> SpendingExecution {
        let minimum_output = t.minimum_output_for(amount).unwrap();
        SpendingExecution { binding: t.binding, kind: t.kind, mint: t.mint, debit_from: t.debit_from,
            output_mint: t.output_mint, destination: t.destination,
            destination_authority: t.destination_authority, adapter_program: t.adapter_program,
            adapter_pool: t.adapter_pool, executor: t.executor, authority_generation: t.authority_generation,
            request_digest: [1; 32], receipt: Pubkey::new_unique(), amount, minimum_output,
            amount_out: if t.kind == SPENDING_SEND { amount } else { minimum_output } }
    }

    #[test]
    fn delegated_spending_consumes_finite_amount_count_and_time() {
        let t = terms(SPENDING_SEND); let x = execution(&t, 50);
        let used = t.consume(SpendingConsumption::default(), &x, 10).unwrap();
        assert_eq!((used.amount, used.operations), (50, 1));
        assert!(t.consume(used, &execution(&t, 40), 14).is_err());
        assert!(t.consume(used, &x, 15).is_err()); // cumulative amount, not per-operation limit
        let full = t.consume(used, &execution(&t, 40), 15).unwrap();
        assert_eq!((full.amount, full.operations), (90, 2));
        assert!(t.consume(full, &execution(&t, 1), 20).is_err());
        assert!(t.consume(SpendingConsumption::default(), &x, 9).is_err());
        assert!(t.consume(SpendingConsumption::default(), &x, 100).is_err());
        let mut one = t.clone(); one.maximum_operations = 1;
        assert!(one.consume(used, &execution(&one, 1), 15).is_err());
    }

    #[test]
    fn swap_rate_cannot_be_weakened_by_quote_or_rounding() {
        let t = terms(SPENDING_SWAP);
        assert_eq!(t.minimum_output_for(1).unwrap(), 1);
        assert_eq!(t.minimum_output_for(50).unwrap(), 34);
        let x = execution(&t, 50);
        assert!(t.consume(SpendingConsumption::default(), &x, 10).is_ok());
        let mut changed = x.clone(); changed.minimum_output = 33; changed.amount_out = 33;
        assert!(t.consume(SpendingConsumption::default(), &changed, 10).is_err());
        changed = x.clone(); changed.amount_out = 33;
        assert!(t.consume(SpendingConsumption::default(), &changed, 10).is_err());
        changed = x.clone(); changed.minimum_output = 40; changed.amount_out = 39;
        assert!(t.consume(SpendingConsumption::default(), &changed, 10).is_err());
        let mut huge = t.clone(); huge.minimum_output_numerator = u64::MAX;
        huge.minimum_output_denominator = 1;
        assert!(huge.minimum_output_for(u64::MAX).is_err());
        huge.minimum_output_denominator = u64::MAX;
        assert_eq!(huge.minimum_output_for(u64::MAX).unwrap(), u64::MAX);
    }

    #[test]
    fn exact_approval_binds_receipt_digest_amount_and_operation_kind() {
        let mut t = terms(SPENDING_SEND); let x = execution(&t, 40);
        t.exact_request_digest = x.request_digest; t.exact_receipt = x.receipt;
        t.exact_amount = x.amount; t.maximum_operations = 1;
        let used = t.consume(SpendingConsumption::default(), &x, 10).unwrap();
        assert!(t.consume(used, &x, 15).is_err());
        for field in 0..4 {
            let mut changed = x.clone();
            match field {
                0 => changed.request_digest = [2; 32],
                1 => changed.receipt = Pubkey::new_unique(),
                2 => { changed.amount = 39; changed.amount_out = 39; },
                _ => changed.kind = crate::repayment::REPAYMENT_RUN,
            }
            assert!(t.consume(SpendingConsumption::default(), &changed, 10).is_err());
        }
    }

    #[test]
    fn executor_generation_and_asset_route_are_not_substitutable() {
        let t = terms(SPENDING_SWAP); let x = execution(&t, 50);
        for field in 0..11 {
            let mut changed = x.clone();
            match field {
                0 => changed.binding = Pubkey::new_unique(), 1 => changed.mint = Pubkey::new_unique(),
                2 => changed.debit_from = Pubkey::new_unique(), 3 => changed.output_mint = Pubkey::new_unique(),
                4 => changed.destination = Pubkey::new_unique(), 5 => changed.destination_authority = Pubkey::new_unique(),
                6 => changed.adapter_program = Pubkey::new_unique(), 7 => changed.adapter_pool = Pubkey::new_unique(),
                8 => changed.executor = Pubkey::new_unique(), 9 => changed.authority_generation = 2,
                _ => changed.kind = SPENDING_SEND,
            }
            assert!(t.consume(SpendingConsumption::default(), &changed, 10).is_err());
        }
    }
}
