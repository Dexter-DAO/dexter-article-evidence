use anchor_lang::prelude::*;
use dexter_contracts::{minus, plus, SCHEMA_VERSION, TabTerms};
use solana_sha256_hasher::hashv;

use crate::RegistryError;

/// Generation one keeps the original PDA exactly. Later owner generations
/// receive fresh authority records; historical grants are never restamped.
pub fn authority_epoch_seed_len(generation: u64) -> usize {
    if generation == 1 { 0 } else { 8 }
}

pub fn actor_epoch_address(principal: &Pubkey, actor_id: &[u8; 32], generation: u64) -> (Pubkey, u8) {
    let epoch = generation.to_le_bytes();
    Pubkey::find_program_address(&[b"actor", principal.as_ref(), actor_id,
        &epoch[..authority_epoch_seed_len(generation)]], &crate::ID)
}

pub fn mandate_epoch_address(principal: &Pubkey, mandate_id: &[u8; 32], generation: u64) -> (Pubkey, u8) {
    let epoch = generation.to_le_bytes();
    Pubkey::find_program_address(&[b"mandate", principal.as_ref(), mandate_id,
        &epoch[..authority_epoch_seed_len(generation)]], &crate::ID)
}

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
    /// Operational native key or immutable Registry passkey-authority alias.
    /// A separately enrolled quorum may replace it without changing identity.
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

/// A permission can pin at most this many order terms; an order can show at most `MAX_ORDER_TERMS`.
pub const MAX_REQUIRED_TERMS: usize = 4;
pub const MAX_ORDER_TERMS: usize = 16;

/// One term of an order: the fingerprint of a field's name and the fingerprint
/// of its value. The programs compare both; they never interpret either.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, Default, PartialEq, Eq, InitSpace)]
pub struct Term {
    pub name: [u8; 32],
    pub value: [u8; 32],
}

/// Strictly ascending names: each name appears once, so a list holds one value
/// per name, and a set of terms has one order and therefore one hash.
pub fn names_ascend(terms: &[Term]) -> bool {
    terms.windows(2).all(|pair| pair[0].name < pair[1].name)
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
    /// Order terms the owner requires, ascending by name; the first
    /// `required_terms_count` are in force. Zero means nothing is pinned.
    pub required_terms_count: u8,
    pub required_terms: [Term; MAX_REQUIRED_TERMS],
}

impl Mandate {
    pub fn pinned_terms(&self) -> &[Term] {
        &self.required_terms[..usize::from(self.required_terms_count).min(MAX_REQUIRED_TERMS)]
    }

    /// Stores a list `pinnable` accepted; unused slots are zero.
    pub fn set_pinned_terms(&mut self, terms: &[Term]) {
        self.required_terms = [Term::default(); MAX_REQUIRED_TERMS];
        self.required_terms[..terms.len()].copy_from_slice(terms);
        self.required_terms_count = terms.len() as u8;
    }
}

/// What a permission may require: at most four terms, one value per name
/// (names strictly ascending) and no all-zero name. An empty list pins nothing.
pub fn pinnable(terms: &[Term]) -> bool {
    terms.len() <= MAX_REQUIRED_TERMS && names_ascend(terms) && terms.iter().all(|term| term.name != [0; 32])
}

/// The value an order's `external_contract_hash` carries when its terms are a
/// list of (name, value) fingerprint pairs.
pub fn order_terms_hash(terms: &[Term]) -> [u8; 32] {
    let count = [terms.len() as u8];
    let mut parts: Vec<&[u8]> = Vec::with_capacity(2 + 2 * terms.len());
    parts.extend([b"DEXTER:ORDER-TERMS:2".as_slice(), &count]);
    for term in terms {
        parts.extend([term.name.as_slice(), &term.value]);
    }
    hashv(&parts).to_bytes()
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

    #[test]
    fn authority_epoch_preserves_first_generation_and_separates_later_grants() {
        let principal = Pubkey::new_unique();
        let id = [19; 32];
        let original_actor = Pubkey::find_program_address(&[b"actor", principal.as_ref(), &id], &crate::ID);
        let original_mandate = Pubkey::find_program_address(&[b"mandate", principal.as_ref(), &id], &crate::ID);
        assert_eq!(actor_epoch_address(&principal, &id, 1), original_actor);
        assert_eq!(mandate_epoch_address(&principal, &id, 1), original_mandate);
        for generation in [2, 3, 256, u64::MAX] {
            assert_ne!(actor_epoch_address(&principal, &id, generation), original_actor);
            assert_ne!(mandate_epoch_address(&principal, &id, generation), original_mandate);
            let suffix = generation.to_le_bytes();
            assert_eq!(actor_epoch_address(&principal, &id, generation),
                Pubkey::find_program_address(&[b"actor", principal.as_ref(), &id, &suffix], &crate::ID));
            assert_eq!(mandate_epoch_address(&principal, &id, generation),
                Pubkey::find_program_address(&[b"mandate", principal.as_ref(), &id, &suffix], &crate::ID));
        }
        assert_ne!(actor_epoch_address(&principal, &id, 2), actor_epoch_address(&principal, &id, 3));
        assert_ne!(mandate_epoch_address(&principal, &id, 2), mandate_epoch_address(&principal, &id, 3));
        assert_ne!(actor_epoch_address(&principal, &id, 2), mandate_epoch_address(&principal, &id, 2));
    }

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
