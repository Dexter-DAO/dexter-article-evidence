//! Separate, revocable authority for free-cash sends and exact-input swaps.
//! A grant allocates no cash and never expands a repayment permission. Core
//! authenticates custody ownership, reserve coverage and actual token deltas.
use anchor_lang::prelude::*;
use crate::financing::{FinanceError, FinanceResult};
use crate::wallet_operation::{TOKEN_ID, TOKEN_2022_ID, SPL_TOKEN_SWAP_ID, JUPITER_ID};

use crate::trading::{policy_route, route_program};

pub const SPENDING_SEND: u8 = 5;
pub const SPENDING_SWAP: u8 = 6;
pub const SPENDING_PERMISSION_VERSION: u16 = 2;
pub const ROUTE_NONE: u8 = 0;
pub const ROUTE_SPL_TOKEN_SWAP: u8 = 1;
pub const ROUTE_JUPITER_V2: u8 = 2;

pub fn supported_token_program(program: Pubkey) -> bool {
    matches!(program, TOKEN_ID | TOKEN_2022_ID)
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct SpendingPermissionTerms {
    pub binding: Pubkey,
    pub kind: u8,
    pub route_profile: u8,
    pub mint: Pubkey,
    pub input_token_program: Pubkey,
    pub debit_from: Pubkey,
    pub output_mint: Pubkey,
    pub output_token_program: Pubkey,
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
    /// Absolute per-operation ceilings, in each endpoint mint's token atoms.
    pub maximum_input_fee: u64,
    pub maximum_output_fee: u64,
}

/// The one request an exact approval permits: its digest and amount. An exact
/// approval also requires `maximum_operations == 1`.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq, InitSpace)]
pub struct ExactRequest {
    pub request_digest: [u8; 32],
    pub amount: u64,
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
    pub route_profile: u8,
    pub mint: Pubkey,
    pub input_token_program: Pubkey,
    pub debit_from: Pubkey,
    pub output_mint: Pubkey,
    pub output_token_program: Pubkey,
    pub destination: Pubkey,
    pub destination_authority: Pubkey,
    pub adapter_program: Pubkey,
    pub adapter_pool: Pubkey,
    pub executor: Pubkey,
    pub authority_generation: u64,
    pub request_digest: [u8; 32],
    /// The trade's number: a standing grant admits each number once through its
    /// window; under an exact approval it is the approval's number.
    pub number: u64,
    pub amount: u64,
    pub minimum_output: u64,
    pub amount_out: u64,
    pub input_fee: u64,
    pub output_fee: u64,
    pub gross_output: u64,
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
        if !supported_token_program(self.input_token_program)
            || !supported_token_program(self.output_token_program)
            || self.maximum_input_fee > self.per_operation_amount {
            return Err(FinanceError::WrongTerms);
        }
        if self.kind == SPENDING_SEND {
            if self.route_profile != ROUTE_NONE || self.output_mint != self.mint
                || self.output_token_program != self.input_token_program
                || self.adapter_program != Pubkey::default() || self.adapter_pool != Pubkey::default()
                || self.minimum_output_numerator == 0 || self.minimum_output_denominator == 0
                || self.minimum_output_numerator > self.minimum_output_denominator
                || self.maximum_output_fee != 0 {
                return Err(FinanceError::WrongTerms);
            }
        } else {
            if self.output_mint == self.mint || self.minimum_output_numerator == 0
                || self.minimum_output_denominator == 0 {
                return Err(FinanceError::WrongTerms);
            }
            match self.route_profile {
                ROUTE_SPL_TOKEN_SWAP if self.adapter_program == SPL_TOKEN_SWAP_ID
                    && self.adapter_pool != Pubkey::default() && self.input_token_program == TOKEN_ID
                    && self.output_token_program == TOKEN_ID => {},
                ROUTE_JUPITER_V2 if self.adapter_program == JUPITER_ID
                    && self.adapter_pool == Pubkey::default() => {},
                profile if policy_route(profile) && route_program(profile) == Some(self.adapter_program)
                    && self.adapter_pool != Pubkey::default() => {},
                _ => return Err(FinanceError::WrongTerms),
            }
        }
        if (self.input_token_program == TOKEN_ID && self.maximum_input_fee != 0)
            || (self.output_token_program == TOKEN_ID && self.maximum_output_fee != 0) {
            return Err(FinanceError::WrongTerms);
        }
        Ok(())
    }

    /// Terms of an exact approval: one operation, for one request.
    pub fn validate_exact(&self, exact: &ExactRequest) -> FinanceResult<()> {
        self.validate()?;
        if exact.request_digest == [0; 32] || exact.amount == 0
            || exact.amount > self.per_operation_amount || self.maximum_operations != 1 {
            return Err(FinanceError::WrongTerms);
        }
        Ok(())
    }

    /// `consume` for an exact approval: the execution must be its one request.
    pub fn consume_exact(&self, used: SpendingConsumption, execution: &SpendingExecution,
        now: i64, exact: &ExactRequest) -> FinanceResult<SpendingConsumption> {
        self.validate_exact(exact)?;
        if exact.request_digest != execution.request_digest || exact.amount != execution.amount {
            return Err(FinanceError::WrongTerms);
        }
        self.consume(used, execution, now)
    }

    pub fn minimum_output_for(&self, amount: u64) -> FinanceResult<u64> {
        if !matches!(self.kind, SPENDING_SEND | SPENDING_SWAP) || self.minimum_output_numerator == 0
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
            || self.route_profile != execution.route_profile
            || self.mint != execution.mint || self.debit_from != execution.debit_from
            || self.input_token_program != execution.input_token_program
            || self.output_token_program != execution.output_token_program
            || self.output_mint != execution.output_mint || self.destination != execution.destination
            || self.destination_authority != execution.destination_authority
            || self.adapter_program != execution.adapter_program || self.adapter_pool != execution.adapter_pool
            || self.executor != execution.executor || self.authority_generation != execution.authority_generation
            || execution.request_digest == [0; 32] || execution.number == 0
            || execution.amount == 0 || execution.amount > self.per_operation_amount
            || now < self.not_before || now >= self.expires_at {
            return Err(FinanceError::WrongTerms);
        }
        if execution.input_fee > self.maximum_input_fee
            || execution.output_fee > self.maximum_output_fee
            || execution.input_fee > execution.amount || execution.minimum_output == 0
            || execution.amount_out < execution.minimum_output
            || execution.minimum_output < self.minimum_output_for(execution.amount)? {
            return Err(FinanceError::InvalidAmounts);
        }
        if self.kind == SPENDING_SEND {
            if execution.gross_output != execution.amount || execution.output_fee != 0
                || execution.amount_out.checked_add(execution.input_fee) != Some(execution.amount) {
                return Err(FinanceError::InvalidAmounts);
            }
        } else if execution.amount_out.checked_add(execution.output_fee) != Some(execution.gross_output) {
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
            binding: Pubkey::new_unique(), kind,
            route_profile: if kind == SPENDING_SEND { ROUTE_NONE } else { ROUTE_SPL_TOKEN_SWAP },
            mint, input_token_program: TOKEN_ID, debit_from: Pubkey::new_unique(),
            output_mint: if kind == SPENDING_SEND { mint } else { Pubkey::new_unique() },
            output_token_program: TOKEN_ID, destination: Pubkey::new_unique(), destination_authority: Pubkey::new_unique(),
            adapter_program: if kind == SPENDING_SEND { Pubkey::default() } else { SPL_TOKEN_SWAP_ID },
            adapter_pool: if kind == SPENDING_SEND { Pubkey::default() } else { Pubkey::new_unique() },
            executor: Pubkey::new_from_array(point), authority_generation: 1,
            not_before: 10, expires_at: 100, minimum_interval: 5,
            per_operation_amount: 50, total_amount: 90, maximum_operations: 3,
            minimum_output_numerator: if kind == SPENDING_SEND { 1 } else { 2 },
            minimum_output_denominator: if kind == SPENDING_SEND { 1 } else { 3 },
            maximum_input_fee: 0, maximum_output_fee: 0,
        }
    }
    fn execution(t: &SpendingPermissionTerms, amount: u64) -> SpendingExecution {
        let minimum_output = if t.kind == SPENDING_SEND { amount } else { t.minimum_output_for(amount).unwrap() };
        SpendingExecution { binding: t.binding, kind: t.kind, route_profile: t.route_profile,
            mint: t.mint, input_token_program: t.input_token_program, debit_from: t.debit_from,
            output_mint: t.output_mint, output_token_program: t.output_token_program, destination: t.destination,
            destination_authority: t.destination_authority, adapter_program: t.adapter_program,
            adapter_pool: t.adapter_pool, executor: t.executor, authority_generation: t.authority_generation,
            request_digest: [1; 32], number: 7, amount, minimum_output,
            amount_out: minimum_output, input_fee: 0, output_fee: 0, gross_output: minimum_output }
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
    fn exact_approval_binds_digest_amount_and_operation_kind() {
        let mut t = terms(SPENDING_SEND); let x = execution(&t, 40);
        let exact = ExactRequest { request_digest: x.request_digest, amount: x.amount };
        assert!(t.consume_exact(SpendingConsumption::default(), &x, 10, &exact).is_err()); // three operations
        t.maximum_operations = 1;
        let used = t.consume_exact(SpendingConsumption::default(), &x, 10, &exact).unwrap();
        assert!(t.consume_exact(used, &x, 15, &exact).is_err());
        for field in 0..4 {
            let mut changed = x.clone();
            match field {
                0 => changed.request_digest = [2; 32],
                1 => changed.number = 0,
                2 => { changed.amount = 39; changed.amount_out = 39; },
                _ => changed.kind = crate::repayment::REPAYMENT_RUN,
            }
            assert!(t.consume_exact(SpendingConsumption::default(), &changed, 10, &exact).is_err());
        }
        assert!(t.validate_exact(&ExactRequest { request_digest: [0; 32], amount: 40 }).is_err());
        assert!(t.validate_exact(&ExactRequest { request_digest: [1; 32], amount: 51 }).is_err());
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
    #[test]
    fn token2022_send_binds_gross_spend_net_floor_and_fee_ceiling() {
        let mut t = terms(SPENDING_SEND);
        t.input_token_program = TOKEN_2022_ID; t.output_token_program = TOKEN_2022_ID;
        t.per_operation_amount = 100; t.total_amount = 200; t.maximum_input_fee = 5;
        t.minimum_output_numerator = 95; t.minimum_output_denominator = 100;
        let mut x = execution(&t, 100);
        x.minimum_output = 95; x.amount_out = 95; x.input_fee = 5; x.gross_output = 100;
        assert_eq!(t.consume(SpendingConsumption::default(), &x, 10).unwrap().amount, 100);
        let mut weaker = x.clone(); weaker.minimum_output = 94;
        assert!(t.consume(SpendingConsumption::default(), &weaker, 10).is_err());
        let mut fee = x.clone(); fee.input_fee = 6; fee.amount_out = 94;
        assert!(t.consume(SpendingConsumption::default(), &fee, 10).is_err());
        let mut wrong_program = x.clone(); wrong_program.input_token_program = TOKEN_ID;
        assert!(t.consume(SpendingConsumption::default(), &wrong_program, 10).is_err());
        let mut unaccounted_fee = x.clone(); unaccounted_fee.input_fee = 4;
        assert!(t.consume(SpendingConsumption::default(), &unaccounted_fee, 10).is_err());
    }

    #[test]
    fn jupiter_profile_and_endpoint_fees_are_explicit_owner_constraints() {
        let mut t = terms(SPENDING_SWAP);
        t.route_profile = ROUTE_JUPITER_V2; t.adapter_program = JUPITER_ID;
        t.adapter_pool = Pubkey::default(); t.input_token_program = TOKEN_2022_ID;
        t.maximum_input_fee = 2;
        let mut x = execution(&t, 50); x.input_fee = 2;
        assert!(t.consume(SpendingConsumption::default(), &x, 10).is_ok());
        let mut arbitrary = t.clone(); arbitrary.adapter_program = Pubkey::new_unique();
        assert!(arbitrary.validate().is_err());
        let mut mixed_profile = t.clone(); mixed_profile.adapter_pool = Pubkey::new_unique();
        assert!(mixed_profile.validate().is_err());
        let mut wrong_fee = x.clone(); wrong_fee.output_fee = 1; wrong_fee.gross_output += 1;
        assert!(t.consume(SpendingConsumption::default(), &wrong_fee, 10).is_err());
        let mut too_much = x.clone(); too_much.input_fee = 3;
        assert!(t.consume(SpendingConsumption::default(), &too_much, 10).is_err());
    }

}
