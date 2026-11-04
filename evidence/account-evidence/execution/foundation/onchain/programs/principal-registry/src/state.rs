use anchor_lang::prelude::*;
use dexter_contracts::{minus, plus, SCHEMA_VERSION, TabTerms};

use crate::RegistryError;

#[account]
#[derive(InitSpace)]
pub struct Principal {
    pub version: u16,
    pub bump: u8,
    pub principal_id: [u8; 32],
    pub domain: [u8; 32],
    /// 1: person, 2: company, 3: other recognized principal.
    pub kind: u8,
    pub governing_policy: [u8; 32],
    pub seed_owner: Pubkey,
    /// This first profile has one Ed25519 owner. Other governance profiles
    /// require distinct handlers and cannot be selected by this field.
    pub owner: Pubkey,
    pub authority_generation: u64,
}

#[account]
#[derive(InitSpace)]
pub struct Actor {
    pub version: u16,
    pub bump: u8,
    pub principal: Pubkey,
    pub actor_id: [u8; 32],
    pub current_key: Pubkey,
    pub generation: u64,
    pub valid_from: i64,
    pub valid_until: i64,
    pub active: bool,
}

#[account]
#[derive(InitSpace)]
pub struct Budget {
    pub version: u16,
    pub bump: u8,
    pub principal: Pubkey,
    pub asset: Pubkey,
    pub ceiling: u64,
    pub reserved: u64,
    /// Lifetime expenditure; repayment has no Registry entrypoint.
    pub consumed: u64,
    /// Positive revision of the owner-approved allowance policy.
    pub revision: u64,
}

impl Budget {
    pub fn check(&self) -> Result<()> {
        require!(self.version == SCHEMA_VERSION, RegistryError::SchemaVersion);
        require!(self.revision > 0, RegistryError::AllowanceRevision);
        require!(plus(self.reserved, self.consumed)? <= self.ceiling, RegistryError::BudgetExhausted);
        Ok(())
    }

    pub fn amend_ceiling(&mut self, expected_revision: u64, ceiling: u64) -> Result<()> {
        self.check()?;
        require!(self.revision == expected_revision, RegistryError::AllowanceRevision);
        require!(ceiling >= plus(self.reserved, self.consumed)?, RegistryError::AllowanceFloor);
        let revision = plus(self.revision, 1)?;
        self.ceiling = ceiling;
        self.revision = revision;
        Ok(())
    }

    pub fn reserve(&mut self, amount: u64) -> Result<()> {
        self.check()?;
        require!(amount > 0, RegistryError::ZeroAmount);
        let reserved = plus(self.reserved, amount)?;
        require!(plus(reserved, self.consumed)? <= self.ceiling, RegistryError::BudgetExhausted);
        self.reserved = reserved;
        Ok(())
    }

    pub fn consume(&mut self, amount: u64) -> Result<()> {
        self.check()?;
        require!(amount > 0, RegistryError::ZeroAmount);
        let reserved = minus(self.reserved, amount)?;
        let consumed = plus(self.consumed, amount)?;
        self.reserved = reserved;
        self.consumed = consumed;
        self.check()
    }

    pub fn release(&mut self, amount: u64) -> Result<()> {
        self.check()?;
        require!(amount > 0, RegistryError::ZeroAmount);
        self.reserved = minus(self.reserved, amount)?;
        self.check()
    }
}

#[account]
#[derive(InitSpace)]
pub struct Mandate {
    pub version: u16,
    pub bump: u8,
    pub principal: Pubkey,
    pub actor: Pubkey,
    pub budget: Pubkey,
    pub mandate_id: [u8; 32],
    pub asset: Pubkey,
    pub supplier: Pubkey,
    /// Exact admitted funding source for this first issuer-backed profile.
    pub source: Pubkey,
    pub per_operation_limit: u64,
    pub expires_at: i64,
    pub revision: u64,
    pub active: bool,
}

#[account]
#[derive(InitSpace)]
pub struct Reservation {
    pub version: u16,
    pub bump: u8,
    pub terms: TabTerms,
    pub terms_digest: [u8; 32],
    pub acceptance_operation: [u8; 32],
    pub remaining: u64,
    pub consumed: u64,
    pub released: u64,
}

impl Reservation {
    pub fn check(&self) -> Result<()> {
        require!(self.version == SCHEMA_VERSION, RegistryError::SchemaVersion);
        require!(plus(plus(self.remaining, self.consumed)?, self.released)? == self.terms.cap,
            RegistryError::ReservationInvariant);
        Ok(())
    }

    pub fn consume(&mut self, amount: u64) -> Result<()> {
        self.check()?;
        require!(amount > 0, RegistryError::ZeroAmount);
        let remaining = minus(self.remaining, amount)?;
        let consumed = plus(self.consumed, amount)?;
        self.remaining = remaining;
        self.consumed = consumed;
        self.check()
    }

    pub fn release(&mut self, amount: u64) -> Result<()> {
        self.check()?;
        require!(amount > 0, RegistryError::ZeroAmount);
        let remaining = minus(self.remaining, amount)?;
        let released = plus(self.released, amount)?;
        self.remaining = remaining;
        self.released = released;
        self.check()
    }
}

#[account]
#[derive(InitSpace)]
pub struct OperationReceipt {
    pub version: u16,
    pub bump: u8,
    pub principal: Pubkey,
    pub operation_id: [u8; 32],
    pub kind: u8,
    pub request_digest: [u8; 32],
    pub reservation: Pubkey,
    pub reservation_remaining: u64,
    pub reservation_consumed: u64,
    pub reservation_released: u64,
    pub budget_reserved: u64,
    pub budget_consumed: u64,
}

impl OperationReceipt {
    pub fn matches(&self, principal: Pubkey, operation_id: [u8; 32], kind: u8,
        request_digest: [u8; 32], reservation: Pubkey) -> Result<bool> {
        if self.version == 0 {
            return Ok(false);
        }
        require!(self.version == SCHEMA_VERSION, RegistryError::SchemaVersion);
        require!(self.principal == principal && self.operation_id == operation_id
            && self.kind == kind && self.request_digest == request_digest
            && self.reservation == reservation, RegistryError::OperationConflict);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn budget() -> Budget {
        Budget { version: SCHEMA_VERSION, bump: 0, principal: Pubkey::new_unique(),
            asset: Pubkey::new_unique(), ceiling: 100, reserved: 0, consumed: 0, revision: 1 }
    }

    #[test]
    fn jobs_share_one_lifetime_ceiling() {
        let mut b = budget();
        b.reserve(60).unwrap();
        assert!(b.reserve(41).is_err());
        assert_eq!(b.reserved, 60);
        b.reserve(40).unwrap();
        b.consume(60).unwrap();
        assert_eq!((b.reserved, b.consumed), (40, 60));
        assert!(b.reserve(1).is_err());
        b.release(40).unwrap();
        b.reserve(40).unwrap();
        assert_eq!((b.reserved, b.consumed), (40, 60));
        assert_eq!(b.revision, 1);
    }

    #[test]
    fn allowance_changes_advance_revision_and_preserve_used_authority() {
        let mut b = budget();
        b.reserve(100).unwrap();
        b.consume(60).unwrap();
        b.amend_ceiling(1, 150).unwrap();
        assert_eq!((b.ceiling, b.revision, b.reserved, b.consumed), (150, 2, 40, 60));
        b.reserve(50).unwrap();
        assert!(b.amend_ceiling(1, 200).is_err());
        assert!(b.amend_ceiling(2, 149).is_err());
        assert_eq!((b.ceiling, b.revision, b.reserved, b.consumed), (150, 2, 90, 60));
        b.release(50).unwrap();
        b.amend_ceiling(2, 100).unwrap();
        assert_eq!((b.ceiling, b.revision, b.reserved, b.consumed), (100, 3, 40, 60));
    }

    #[test]
    fn invalid_or_exhausted_policy_revision_cannot_change_allowance() {
        let mut b = budget();
        b.revision = 0;
        assert!(b.check().is_err());
        assert!(b.amend_ceiling(0, 150).is_err());
        assert_eq!((b.ceiling, b.revision), (100, 0));
        b.revision = u64::MAX;
        assert!(b.amend_ceiling(u64::MAX, 150).is_err());
        assert_eq!((b.ceiling, b.revision), (100, u64::MAX));
    }

    #[test]
    fn failed_overflow_and_shortfall_leave_budget_unchanged() {
        let mut b = budget();
        b.ceiling = u64::MAX;
        b.reserved = u64::MAX;
        assert!(b.reserve(1).is_err());
        assert_eq!(b.reserved, u64::MAX);
        b.reserved = 2;
        assert!(b.consume(3).is_err());
        assert!(b.release(3).is_err());
        assert_eq!((b.reserved, b.consumed), (2, 0));
    }

    #[test]
    fn operation_identity_is_shared_across_stages() {
        let principal = Pubkey::new_unique();
        let reservation = Pubkey::new_unique();
        let r = OperationReceipt { version: SCHEMA_VERSION, bump: 0, principal,
            operation_id: [1; 32], kind: 1, request_digest: [2; 32], reservation,
            reservation_remaining: 50, reservation_consumed: 0, reservation_released: 0,
            budget_reserved: 50, budget_consumed: 0 };
        assert!(r.matches(principal, [1; 32], 1, [2; 32], reservation).unwrap());
        assert!(r.matches(principal, [1; 32], 2, [2; 32], reservation).is_err());
        assert!(r.matches(principal, [1; 32], 1, [3; 32], reservation).is_err());
        assert!(r.matches(principal, [1; 32], 1, [2; 32], Pubkey::new_unique()).is_err());
    }
}
