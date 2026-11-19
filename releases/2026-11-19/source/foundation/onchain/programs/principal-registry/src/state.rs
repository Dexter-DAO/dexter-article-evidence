use anchor_lang::prelude::*;
use dexter_contracts::{completed_run_policy, minus, plus, SCHEMA_VERSION, TabTerms};
use dexter_contracts::window::OrderWindow;
use dexter_contracts::billing::{billing_policy_digest, BillingPolicy, BILLING_NAMESPACE, BILLING_SETTLEMENT_UNIT,
    MAX_BILLING_METRICS};
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
    /// Paid this record's rent deposit; `return_excess_rent` returns excess here.
    pub deposit_payer: Pubkey,
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
    /// Exact approvals granted so far. Each grant takes the next number, so an
    /// approval's address (`["approval", principal, number]`) is never reused.
    pub approvals: u64,
}

#[account]
#[derive(InitSpace)]
pub struct Actor {
    pub deposit_payer: Pubkey, pub version: u16,
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
    pub deposit_payer: Pubkey, pub version: u16,
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

/// A permission names at most four sellers and holds at most four term rules
/// of at most four values each; an order can show at most `MAX_ORDER_TERMS`.
pub const MAX_SELLERS: usize = 4;
pub const MAX_TERM_RULES: usize = 4;
pub const MAX_RULE_VALUES: usize = 4;
pub const MAX_ORDER_TERMS: usize = 16;

/// One term of an order: the fingerprint of a field's name and the fingerprint
/// of its value. The programs compare both; they never interpret either.
/// `number` is the whole number the value fingerprints (`number_value`), shown
/// in the clear so a range can be checked. The order terms hash covers the
/// name and the value; the value commits to the number.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Term {
    pub name: [u8; 32],
    pub value: [u8; 32],
    pub number: Option<i64>,
}

/// Strictly ascending names: each name appears once, so a list holds one value
/// per name, and a set of terms has one order and therefore one hash.
pub fn names_ascend(terms: &[Term]) -> bool {
    terms.windows(2).all(|pair| pair[0].name < pair[1].name)
}

/// Value fingerprint of a whole number: sha256("DEXTER:ORDER-TERM-NUMBER:1" || i64le(n)).
/// A decimal is a whole number of its smallest step, and its name says the
/// step: size 10.5 is `size_tenths` = 105.
pub fn number_value(n: i64) -> [u8; 32] {
    hashv(&[b"DEXTER:ORDER-TERM-NUMBER:1", &n.to_le_bytes()]).to_bytes()
}

/// How a rule bounds one name of an order's terms.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, Default, PartialEq, Eq, InitSpace)]
pub enum RuleKind {
    /// The order's value is one of the rule's values.
    #[default]
    List,
    /// The order's number is from `min` to `max`. For `unit_price` it is the
    /// order's own price per completed-run unit, never a shown number, and the
    /// rule's values are the services that price may be for (`price_unit_value`).
    Range,
}

/// What a permission allows under one name. A list holds 1 to 4 values in
/// ascending order and zero bounds; a range holds `min <= max` and no values,
/// except a `unit_price` range, which also holds 1 to 4 services in ascending
/// order. Unused value slots are zero, so a rule has one stored form.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, Default, PartialEq, Eq, InitSpace)]
pub struct TermRule {
    pub name: [u8; 32],
    pub kind: RuleKind,
    pub value_count: u8,
    pub values: [[u8; 32]; MAX_RULE_VALUES],
    pub min: i64,
    pub max: i64,
}

impl TermRule {
    pub fn values(&self) -> &[[u8; 32]] {
        &self.values[..usize::from(self.value_count).min(MAX_RULE_VALUES)]
    }

    /// `delivery_signer` and `billing_tariff` values are a key and a hash, so
    /// those names take lists only. A price ceiling names the services it bounds.
    fn well_formed(&self) -> bool {
        let count = usize::from(self.value_count);
        let listed = (1..=MAX_RULE_VALUES).contains(&count) && self.values().windows(2).all(|pair| pair[0] < pair[1]);
        self.name != [0; 32] && self.values.iter().skip(count).all(|value| *value == [0; 32]) && match self.kind {
            RuleKind::List => listed && self.min == 0 && self.max == 0,
            RuleKind::Range if self.name == UNIT_PRICE_NAME => listed && self.min <= self.max,
            RuleKind::Range => count == 0 && self.min <= self.max
                && self.name != DELIVERY_SIGNER_NAME && self.name != BILLING_TARIFF_NAME,
        }
    }
}

/// The sellers an order may name and the rules its terms must meet, as the owner sets them.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct MandateRules {
    pub sellers: Vec<Pubkey>,
    pub terms: Vec<TermRule>,
}

impl MandateRules {
    /// 1 to 4 sellers in strictly ascending order, so each appears once, none
    /// the default key; at most four term rules, names strictly ascending.
    pub fn valid(&self) -> bool {
        (1..=MAX_SELLERS).contains(&self.sellers.len())
            && self.sellers.windows(2).all(|pair| pair[0] < pair[1])
            && self.sellers.iter().all(|seller| *seller != Pubkey::default())
            && self.terms.len() <= MAX_TERM_RULES
            && self.terms.windows(2).all(|pair| pair[0].name < pair[1].name)
            && self.terms.iter().all(TermRule::well_formed)
    }
}

#[account]
#[derive(InitSpace)]
pub struct Mandate {
    pub deposit_payer: Pubkey,
    pub version: u16,
    pub bump: u8,
    pub principal: Pubkey,
    pub actor: Pubkey,
    pub budget: Pubkey,
    pub mandate_id: [u8; 32],
    pub asset: Pubkey,
    /// Sellers an order may name, ascending; the first `seller_count` are in force.
    pub seller_count: u8,
    pub sellers: [Pubkey; MAX_SELLERS],
    /// Exact admitted funding source for this first issuer-backed profile.
    pub source: Pubkey,
    pub per_operation_limit: u64,
    pub expires_at: i64,
    pub revision: u64,
    pub active: bool,
    /// Rules on the order's terms, ascending by name; the first `rule_count`
    /// are in force. Zero means the terms are not restricted.
    pub rule_count: u8,
    pub rules: [TermRule; MAX_TERM_RULES],
    /// Order numbers this mandate has committed (`window.rs`). A Tab lives at
    /// `["tab", mandate, order]`; the window, not account existence, keeps a
    /// closed Tab's number from being used again.
    pub window: OrderWindow,
    /// Tabs closed under this mandate, and the chain of their final fingerprints:
    /// `head = sha256("DEXTER:CLOSED-TAB:1" || head || tab || steps || fingerprint)`.
    pub closed_tabs: u64,
    pub closed_tabs_head: [u8; 32],
    /// The longest settlement hold window, in seconds, an order under this
    /// permission may agree; 0 (the default) keeps every order final at
    /// settlement. The owner sets it with `set_mandate_hold_window`.
    pub max_hold_window_seconds: u32,
}

/// Fold one closed Tab into its mandate's chain.
pub fn closed_tab_link(head: &[u8; 32], tab: &Pubkey, steps: u32, fingerprint: &[u8; 32]) -> [u8; 32] {
    hashv(&[b"DEXTER:CLOSED-TAB:1", head, tab.as_ref(), &steps.to_le_bytes(), fingerprint]).to_bytes()
}

impl Mandate {
    pub fn sellers(&self) -> &[Pubkey] {
        &self.sellers[..usize::from(self.seller_count).min(MAX_SELLERS)]
    }

    pub fn term_rules(&self) -> &[TermRule] {
        &self.rules[..usize::from(self.rule_count).min(MAX_TERM_RULES)]
    }

    /// The permission holds exactly these sellers and rules.
    pub fn holds(&self, rules: &MandateRules) -> bool {
        self.sellers() == rules.sellers.as_slice() && self.term_rules() == rules.terms.as_slice()
    }

    /// Stores what `MandateRules::valid` accepted; unused slots are zero.
    pub fn set_rules(&mut self, rules: &MandateRules) {
        self.sellers = [Pubkey::default(); MAX_SELLERS];
        self.sellers[..rules.sellers.len()].copy_from_slice(&rules.sellers);
        self.seller_count = rules.sellers.len() as u8;
        self.rules = [TermRule::default(); MAX_TERM_RULES];
        self.rules[..rules.terms.len()].copy_from_slice(&rules.terms);
        self.rule_count = rules.terms.len() as u8;
    }
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

/// Name fingerprint of the reserved term `delivery_signer`,
/// sha256("DEXTER:ORDER-TERM-NAME:1" || "delivery_signer"). Its value is not a
/// fingerprint of text: it is the 32 bytes of the key the order names to sign
/// for delivery, so an owner can pin who signs.
pub const DELIVERY_SIGNER_NAME: [u8; 32] = [89, 240, 212, 226, 178, 135, 52, 94, 218, 29, 19, 205,
    96, 126, 159, 52, 215, 108, 20, 49, 8, 172, 114, 73, 64, 18, 225, 15, 162, 20, 195, 112];
/// Name fingerprint of the reserved term `billing_tariff`; its value is
/// `billing_tariff_hash` of an evidence order's policy.
pub const BILLING_TARIFF_NAME: [u8; 32] = [93, 149, 200, 15, 118, 101, 94, 198, 103, 217, 236, 110,
    215, 223, 103, 38, 67, 10, 169, 54, 254, 117, 181, 126, 227, 195, 94, 205, 170, 72, 248, 160];
/// Name fingerprint of the reserved term `unit_price`; its value is
/// `unit_price_value` of an order whose terms commit to a completed-run price.
pub const UNIT_PRICE_NAME: [u8; 32] = [168, 224, 2, 64, 24, 208, 226, 2, 15, 179, 23, 39, 247, 115, 38,
    191, 255, 133, 34, 107, 62, 167, 9, 241, 146, 97, 15, 93, 241, 177, 158, 85];

/// What an evidence order charges: sha256("DEXTER:BILLING-TARIFF:1" || u8(metric_count)
/// || metric_ids || price_numerators || price_denominators || max_units), all eight
/// slots of each array, integers little-endian.
pub fn billing_tariff_hash(policy: &BillingPolicy) -> [u8; 32] {
    let mut encoded = Vec::with_capacity(1 + MAX_BILLING_METRICS * (32 + 24));
    encoded.push(policy.metric_count);
    for id in &policy.metric_ids { encoded.extend_from_slice(id); }
    for values in [&policy.price_numerators, &policy.price_denominators, &policy.max_units] {
        for value in values { encoded.extend_from_slice(&value.to_le_bytes()); }
    }
    hashv(&[b"DEXTER:BILLING-TARIFF:1", &encoded]).to_bytes()
}

/// Which service a price ceiling bounds: sha256("DEXTER:PRICE-UNIT:1" || namespace || unit),
/// the seller's service and its completed-run unit.
pub fn price_unit_value(namespace: &[u8; 32], unit: &[u8; 32]) -> [u8; 32] {
    hashv(&[b"DEXTER:PRICE-UNIT:1", namespace, unit]).to_bytes()
}

/// What an order through `reserve_pinned` charges, per unit of which service:
/// sha256("DEXTER:UNIT-PRICE:1" || namespace || unit || u64le(unit_price)).
pub fn unit_price_value(namespace: &[u8; 32], unit: &[u8; 32], unit_price: u64) -> [u8; 32] {
    hashv(&[b"DEXTER:UNIT-PRICE:1", namespace, unit, &unit_price.to_le_bytes()]).to_bytes()
}

/// An order's shown list of terms and how its signed terms commit to that list.
pub(crate) struct ShownTerms { pub terms: Vec<Term>, pub commitment: Commitment }

pub(crate) enum Commitment {
    /// `external_terms_hash` = completed_run_policy(namespace, unit, unit_price, list hash).
    /// The order names no delivery signer.
    Run { namespace: [u8; 32], unit: [u8; 32], unit_price: u64 },
    /// An evidence order: `external_terms_hash` = completed_run_policy(billing namespace,
    /// billing unit, 1, policy hash), the policy's `request_hash` is the list hash and its
    /// `source` is the delivery signer the order names.
    Billing { bill: Pubkey, claim: Pubkey, policy: Box<BillingPolicy> },
}

impl ShownTerms {
    /// The list is the one the signed terms commit to.
    pub(crate) fn commits_to(&self, signed: &TabTerms) -> bool {
        let list = order_terms_hash(&self.terms);
        match &self.commitment {
            Commitment::Run { namespace, unit, unit_price } =>
                completed_run_policy(namespace, unit, *unit_price, &list) == signed.external_terms_hash,
            Commitment::Billing { bill, claim, policy } => policy.request_hash == list
                && billing_policy_digest(&signed.domain, bill, claim, policy).is_ok_and(|hash|
                    completed_run_policy(&BILLING_NAMESPACE, &BILLING_SETTLEMENT_UNIT, 1, &hash) == signed.external_terms_hash),
        }
    }

    /// The order's own value for a reserved name, or none if the order has none.
    fn own(&self, name: &[u8; 32]) -> Option<[u8; 32]> {
        match (&self.commitment, *name) {
            (Commitment::Billing { policy, .. }, DELIVERY_SIGNER_NAME) => Some(policy.source.to_bytes()),
            (Commitment::Billing { policy, .. }, BILLING_TARIFF_NAME) => Some(billing_tariff_hash(policy)),
            (Commitment::Run { namespace, unit, unit_price }, UNIT_PRICE_NAME) =>
                Some(unit_price_value(namespace, unit, *unit_price)),
            _ => None,
        }
    }

    /// A listed reserved term carries the order's own value; an order without one cannot list it.
    fn lists_own(&self, name: &[u8; 32]) -> bool {
        let own = self.own(name);
        self.terms.iter().all(|term| term.name != *name || Some(term.value) == own)
    }

    /// A delivery_signer term, if listed, carries the key the order names to sign for delivery.
    pub(crate) fn names_its_delivery_signer(&self) -> bool {
        self.lists_own(&DELIVERY_SIGNER_NAME)
    }

    /// A billing_tariff or unit_price term, if listed, carries the order's own price.
    pub(crate) fn names_its_price(&self) -> bool {
        self.lists_own(&BILLING_TARIFF_NAME) && self.lists_own(&UNIT_PRICE_NAME)
    }

    /// Every number shown beside a term is the number its value fingerprints.
    pub(crate) fn numbers_match(&self) -> bool {
        self.terms.iter().all(|term| term.number.is_none_or(|n| number_value(n) == term.value))
    }

    /// The order meets one rule of its permission: the rule's name is listed,
    /// with a value the list holds or a number in the range. A `unit_price`
    /// rule bounds an order through `reserve_pinned`: a positive price per
    /// completed-run unit outside the billing namespace, which an evidence order
    /// does not have. A ceiling also requires the order's service and unit to be
    /// one it names, and bounds the order's own price.
    pub(crate) fn meets(&self, rule: &TermRule) -> Result<()> {
        let Some(shown) = self.terms.iter().find(|term| term.name == rule.name) else {
            return err!(RegistryError::TermsNotPermitted);
        };
        let mut number = shown.number.map(i128::from);
        if rule.name == UNIT_PRICE_NAME {
            let Commitment::Run { namespace, unit, unit_price } = &self.commitment else {
                return err!(RegistryError::TermsNotPermitted);
            };
            require!(*namespace != BILLING_NAMESPACE && *unit_price > 0, RegistryError::TermsNotPermitted);
            require!(rule.kind == RuleKind::List || rule.values().contains(&price_unit_value(namespace, unit)),
                RegistryError::TermsNotPermitted);
            number = Some(i128::from(*unit_price));
        }
        match rule.kind {
            RuleKind::List => require!(rule.values().contains(&shown.value), RegistryError::TermsNotPermitted),
            RuleKind::Range => {
                let Some(number) = number else { return err!(RegistryError::TermsNotPermitted) };
                require!(i128::from(rule.min) <= number && number <= i128::from(rule.max), RegistryError::NumberOutOfRange);
            }
        }
        Ok(())
    }
}

/// The budget hold of one Tab, at `["hold", mandate, order]`. It keeps the
/// authorized terms by digest, and the budget, mandate and order, so the
/// Registry checks its own budget, mandate and conservation without relying on
/// ClaimsCore's inputs. It closes with its Tab (`close_hold`).
#[account]
#[derive(InitSpace)]
pub struct Reservation {
    pub deposit_payer: Pubkey,
    pub version: u16,
    pub bump: u8,
    pub budget: Pubkey,
    pub mandate: Pubkey,
    pub order: u64,
    /// `terms_digest` of the authorized terms (cap including the fee cap).
    pub terms_digest: [u8; 32],
    pub cap: u64,
    pub remaining: u64,
    pub consumed: u64,
    pub released: u64,
}

pub fn hold_address(mandate: &Pubkey, order: u64) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"hold", mandate.as_ref(), &order.to_le_bytes()], &crate::ID)
}

impl Reservation {
    pub fn check(&self) -> Result<()> {
        require!(self.version == SCHEMA_VERSION, RegistryError::SchemaVersion);
        require!(plus(plus(self.remaining, self.consumed)?, self.released)? == self.cap,
            RegistryError::ReservationInvariant);
        Ok(())
    }

    /// The hold was created for exactly these authorized terms.
    pub fn holds_terms(&self, terms: &TabTerms) -> Result<bool> {
        Ok(self.terms_digest == dexter_contracts::terms_digest(terms)? && self.cap == terms.cap
            && self.mandate == terms.mandate && self.order == terms.order)
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
        Budget { deposit_payer: Pubkey::default(), version: SCHEMA_VERSION, bump: 0, principal: Pubkey::new_unique(),
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
    fn reserved_names_are_the_fingerprints_of_their_text() {
        assert_eq!(hashv(&[b"DEXTER:ORDER-TERM-NAME:1", b"delivery_signer"]).to_bytes(), DELIVERY_SIGNER_NAME);
        assert_eq!(hashv(&[b"DEXTER:ORDER-TERM-NAME:1", b"billing_tariff"]).to_bytes(), BILLING_TARIFF_NAME);
        assert_eq!(hashv(&[b"DEXTER:ORDER-TERM-NAME:1", b"unit_price"]).to_bytes(), UNIT_PRICE_NAME);
    }

    /// The shared vectors the SDK and the program tests also check.
    #[test]
    fn price_values_match_the_shared_vectors() {
        let shared: serde_json::Value = serde_json::from_str(include_str!("../../../tests/order_terms_vectors.json")).unwrap();
        let bytes = |v: &serde_json::Value| -> [u8; 32] {
            let text = v.as_str().unwrap();
            let mut out = [0u8; 32];
            for (i, byte) in out.iter_mut().enumerate() { *byte = u8::from_str_radix(&text[2 * i..2 * i + 2], 16).unwrap(); }
            out
        };
        let u64s = |v: &serde_json::Value| -> [u64; MAX_BILLING_METRICS] {
            let list: Vec<u64> = v.as_array().unwrap().iter().map(|x| x.as_str().unwrap().parse().unwrap()).collect();
            list.try_into().unwrap()
        };
        let tariffs = shared["billing_tariff"]["vectors"].as_array().unwrap();
        assert!(tariffs.len() >= 4);
        for v in tariffs {
            let ids: Vec<[u8; 32]> = v["metric_ids"].as_array().unwrap().iter().map(&bytes).collect();
            let policy = BillingPolicy { schema_version: 1, source: Pubkey::default(), source_identity: [0; 32],
                work_id: [0; 32], request_hash: [0; 32], work_until: 0, evidence_deadline: 0, claim_deadline: 0,
                metric_count: v["metric_count"].as_u64().unwrap() as u8, metric_ids: ids.try_into().unwrap(),
                price_numerators: u64s(&v["price_numerators"]), price_denominators: u64s(&v["price_denominators"]),
                max_units: u64s(&v["max_units"]) };
            assert_eq!(billing_tariff_hash(&policy), bytes(&v["value"]), "{}", v["name"]);
        }
        let prices = shared["unit_price"]["vectors"].as_array().unwrap();
        let price = |v: &serde_json::Value| -> u64 { v["unit_price"].as_str().unwrap().parse().unwrap() };
        assert!(prices.iter().any(|v| price(v) == u64::MAX));
        assert!(tariffs.iter().any(|v| v["max_units"][7].as_str() == Some("18446744073709551615")));
        for v in prices {
            assert_eq!(unit_price_value(&bytes(&v["namespace"]), &bytes(&v["unit"]), price(v)),
                bytes(&v["value"]), "{}", v["name"]);
        }
    }

    /// The number vectors the SDK and the program tests also check.
    #[test]
    fn number_values_match_the_shared_vectors() {
        let shared: serde_json::Value = serde_json::from_str(include_str!("../../../tests/order_terms_vectors.json")).unwrap();
        assert_eq!(shared["number"]["domain"].as_str(), Some("DEXTER:ORDER-TERM-NUMBER:1"));
        let vectors = shared["number"]["vectors"].as_array().unwrap();
        let numbers: Vec<i64> = vectors.iter().map(|v| v["number"].as_str().unwrap().parse().unwrap()).collect();
        assert!(numbers.contains(&i64::MIN) && numbers.contains(&i64::MAX) && numbers.contains(&-1));
        for (v, n) in vectors.iter().zip(numbers) {
            let value: Vec<u8> = (0..32).map(|i| u8::from_str_radix(&v["value"].as_str().unwrap()[2 * i..2 * i + 2], 16).unwrap()).collect();
            assert_eq!(number_value(n).as_slice(), value.as_slice(), "{}", v["number"]);
        }
    }

    #[test]
    fn price_unit_values_match_the_shared_vectors() {
        let shared: serde_json::Value = serde_json::from_str(include_str!("../../../tests/order_terms_vectors.json")).unwrap();
        assert_eq!(shared["price_unit"]["domain"].as_str(), Some("DEXTER:PRICE-UNIT:1"));
        let bytes = |v: &serde_json::Value| -> [u8; 32] {
            let text = v.as_str().unwrap();
            let mut out = [0u8; 32];
            for (i, byte) in out.iter_mut().enumerate() { *byte = u8::from_str_radix(&text[2 * i..2 * i + 2], 16).unwrap(); }
            out
        };
        let vectors = shared["price_unit"]["vectors"].as_array().unwrap();
        assert!(vectors.len() >= 3);
        for v in vectors {
            assert_eq!(price_unit_value(&bytes(&v["namespace"]), &bytes(&v["unit"])), bytes(&v["value"]), "{}", v["name"]);
        }
    }

    fn list(name: [u8; 32], values: &[[u8; 32]]) -> TermRule {
        let mut rule = TermRule { name, kind: RuleKind::List, value_count: values.len() as u8, ..TermRule::default() };
        rule.values[..values.len().min(MAX_RULE_VALUES)].copy_from_slice(&values[..values.len().min(MAX_RULE_VALUES)]);
        rule
    }

    fn range(name: [u8; 32], min: i64, max: i64) -> TermRule {
        TermRule { name, kind: RuleKind::Range, min, max, ..TermRule::default() }
    }

    fn rules(sellers: Vec<Pubkey>, terms: Vec<TermRule>) -> MandateRules {
        MandateRules { sellers, terms }
    }

    #[test]
    fn a_rule_has_one_form() {
        let seller = |byte: u8| Pubkey::new_from_array([byte; 32]);
        let (size, colour) = ([1; 32], [2; 32]);
        let good = list(size, &[[3; 32], [4; 32]]);
        assert!(rules(vec![seller(1)], vec![]).valid());
        assert!(rules(vec![seller(1), seller(2), seller(3), seller(4)], vec![good, range(colour, -5, 5)]).valid());
        assert!(rules(vec![seller(1)], vec![list(size, &[[0; 32]])]).valid(), "an all-zero value can be listed");
        let service = price_unit_value(&[7; 32], &[8; 32]);
        assert!(rules(vec![seller(1)], vec![range(size, 7, 7), ceiling(&[service], 0, 185)]).valid());
        assert!(DELIVERY_SIGNER_NAME < BILLING_TARIFF_NAME);
        assert!(rules(vec![seller(1)], vec![list(DELIVERY_SIGNER_NAME, &[[5; 32]]), list(BILLING_TARIFF_NAME, &[[6; 32]])]).valid());
        // Sellers: one to four, ascending, none the default key.
        for sellers in [vec![], vec![seller(1), seller(2), seller(3), seller(4), seller(5)], vec![seller(2), seller(1)],
                vec![seller(1), seller(1)], vec![Pubkey::default()]] {
            assert!(!rules(sellers, vec![]).valid());
        }
        // Names: ascending, distinct, non-zero; at most four rules.
        let five: Vec<TermRule> = (1..=5u8).map(|byte| range([byte; 32], 0, 1)).collect();
        for terms in [vec![list(colour, &[[3; 32]]), list(size, &[[3; 32]])], vec![good, good],
                vec![list([0; 32], &[[3; 32]])], five] {
            assert!(!rules(vec![seller(1)], terms).valid());
        }
        // A list: 1 to 4 ascending values, unused slots and bounds zero.
        let mut stray = good;
        stray.values[3] = [9; 32];
        let mut bounded = good;
        bounded.max = 1;
        for rule in [list(size, &[]), list(size, &[[1; 32], [2; 32], [3; 32], [4; 32], [5; 32]]),
                list(size, &[[4; 32], [3; 32]]), list(size, &[[3; 32], [3; 32]]), stray, bounded] {
            assert!(!rules(vec![seller(1)], vec![rule]).valid(), "{rule:?}");
        }
        // A range: min <= max, no values, never on delivery_signer or billing_tariff.
        let mut valued = range(size, 0, 1);
        valued.values[0] = [3; 32];
        for rule in [range(size, 2, 1), valued, range(DELIVERY_SIGNER_NAME, 0, 1), range(BILLING_TARIFF_NAME, 0, 1)] {
            assert!(!rules(vec![seller(1)], vec![rule]).valid(), "{rule:?}");
        }
        let mut counted = range(size, 0, 1);
        counted.value_count = 1;
        assert!(!rules(vec![seller(1)], vec![counted]).valid());
        // A price ceiling names 1 to 4 services, ascending, and bounds min <= max.
        let other = price_unit_value(&[7; 32], &[9; 32]);
        let (low, high) = if service < other { (service, other) } else { (other, service) };
        assert!(rules(vec![seller(1)], vec![ceiling(&[low, high], 0, 185)]).valid());
        let mut descending = ceiling(&[low, high], 0, 185);
        descending.values.swap(0, 1);
        for rule in [range(UNIT_PRICE_NAME, 0, 185), ceiling(&[service], 186, 185), ceiling(&[service, service], 0, 185),
                ceiling(&[[1; 32], [2; 32], [3; 32], [4; 32], [5; 32]], 0, 185), descending] {
            assert!(!rules(vec![seller(1)], vec![rule]).valid(), "{rule:?}");
        }
    }

    fn ceiling(services: &[[u8; 32]], min: i64, max: i64) -> TermRule {
        TermRule { kind: RuleKind::Range, min, max, ..list(UNIT_PRICE_NAME, services) }
    }

    fn shown(terms: Vec<Term>, unit_price: u64) -> ShownTerms {
        ShownTerms { terms, commitment: Commitment::Run { namespace: [7; 32], unit: [8; 32], unit_price } }
    }

    fn number_term(name: [u8; 32], n: i64) -> Term {
        Term { name, value: number_value(n), number: Some(n) }
    }

    #[test]
    fn an_order_meets_a_rule_or_is_refused() {
        let code = |result: Result<()>| match result {
            Ok(()) => None,
            Err(Error::AnchorError(error)) => Some(error.error_name),
            Err(other) => panic!("{other:?}"),
        };
        let not_permitted = Some("TermsNotPermitted".to_string());
        let out_of_range = Some("NumberOutOfRange".to_string());
        let (size, quantity) = ([1; 32], [2; 32]);
        let sizes = list(size, &[number_value(100), number_value(105)]);
        for n in [100, 105] {
            assert_eq!(code(shown(vec![number_term(size, n)], 1).meets(&sizes)), None);
        }
        assert_eq!(code(shown(vec![number_term(size, 110)], 1).meets(&sizes)), not_permitted);
        assert_eq!(code(shown(vec![number_term(quantity, 1)], 1).meets(&sizes)), not_permitted, "name not listed");
        let quantities = range(quantity, 1, 3);
        for (n, expected) in [(0, &out_of_range), (1, &None), (3, &None), (4, &out_of_range), (i64::MIN, &out_of_range)] {
            assert_eq!(&code(shown(vec![number_term(quantity, n)], 1).meets(&quantities)), expected, "{n}");
        }
        let hidden = Term { name: quantity, value: number_value(1), number: None };
        assert_eq!(code(shown(vec![hidden], 1).meets(&quantities)), not_permitted, "a range needs the number");
        let mismatched = Term { name: quantity, value: number_value(1), number: Some(2) };
        assert!(!shown(vec![mismatched], 1).numbers_match());
        assert!(shown(vec![number_term(quantity, 2), hidden], 1).numbers_match());
        // A price ceiling reads the order's own price per unit of a service it names, never a shown number.
        let ceiling = ceiling(&[price_unit_value(&[7; 32], &[8; 32])], 0, 185);
        let price = Term { name: UNIT_PRICE_NAME, value: unit_price_value(&[7; 32], &[8; 32], 185), number: None };
        assert_eq!(code(shown(vec![price], 185).meets(&ceiling)), None);
        assert_eq!(code(shown(vec![price], 186).meets(&ceiling)), out_of_range);
        assert_eq!(code(shown(vec![price], u64::MAX).meets(&ceiling)), out_of_range);
        assert_eq!(code(shown(vec![number_term(size, 100)], 185).meets(&ceiling)), not_permitted, "price not listed");
        let served = |namespace: [u8; 32], unit: [u8; 32], unit_price: u64| ShownTerms { terms: vec![price],
            commitment: Commitment::Run { namespace, unit, unit_price } };
        assert_eq!(code(served([7; 32], [9; 32], 185).meets(&ceiling)), not_permitted, "another unit of the service");
        assert_eq!(code(served([6; 32], [8; 32], 185).meets(&ceiling)), not_permitted, "another service");
        assert_eq!(code(served(BILLING_NAMESPACE, BILLING_SETTLEMENT_UNIT, 1).meets(&ceiling)), not_permitted, "billing namespace");
        assert_eq!(code(served([7; 32], [8; 32], 0).meets(&ceiling)), not_permitted, "zero price");
        // An exact unit_price list is also refused for the billing namespace or a zero price.
        let billing_price = Term { name: UNIT_PRICE_NAME, value: unit_price_value(&BILLING_NAMESPACE, &BILLING_SETTLEMENT_UNIT, 1), number: None };
        let exact = list(UNIT_PRICE_NAME, &[billing_price.value]);
        assert_eq!(code(ShownTerms { terms: vec![billing_price], commitment: Commitment::Run { namespace: BILLING_NAMESPACE,
            unit: BILLING_SETTLEMENT_UNIT, unit_price: 1 } }.meets(&exact)), not_permitted, "billing namespace, exact price");
        let zero = Term { name: UNIT_PRICE_NAME, value: unit_price_value(&[7; 32], &[8; 32], 0), number: None };
        assert_eq!(code(ShownTerms { terms: vec![zero], commitment: Commitment::Run { namespace: [7; 32], unit: [8; 32],
            unit_price: 0 } }.meets(&list(UNIT_PRICE_NAME, &[zero.value]))), not_permitted, "zero price, exact");
        assert_eq!(code(shown(vec![price], 185).meets(&list(UNIT_PRICE_NAME, &[price.value]))), None, "the exact price");
        let policy = BillingPolicy { schema_version: 1, source: Pubkey::default(), source_identity: [0; 32],
            work_id: [0; 32], request_hash: [0; 32], work_until: 0, evidence_deadline: 0, claim_deadline: 0,
            metric_count: 1, metric_ids: [[0; 32]; MAX_BILLING_METRICS], price_numerators: [1; MAX_BILLING_METRICS],
            price_denominators: [1; MAX_BILLING_METRICS], max_units: [1; MAX_BILLING_METRICS] };
        let billing = ShownTerms { terms: vec![price], commitment: Commitment::Billing { bill: Pubkey::default(),
            claim: Pubkey::default(), policy: Box::new(policy) } };
        assert_eq!(code(billing.meets(&ceiling)), not_permitted, "an evidence order has no unit price");
    }

}
