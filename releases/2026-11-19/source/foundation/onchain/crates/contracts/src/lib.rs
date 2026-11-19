//! Versioned financial meanings shared by the production implementation lanes.
//! Local development program identities only; no mainnet deployment is implied.
pub mod financing;
pub mod billing;
pub mod recovery;
pub mod webauthn;
pub mod commercial;
pub mod funded;
pub mod corrections;
pub mod repayment;
pub mod spending;
pub mod wallet_operation;
pub mod window;
use anchor_lang::prelude::*;
use solana_sha256_hasher::hashv;

pub const SCHEMA_VERSION: u16 = 1;
pub const REGISTRY_ID: Pubkey = pubkey!("DEXregFGwJM4LsVAPeWf7pQm4JBNbZxijTRQzrxk1SQS");
pub const CORE_ID: Pubkey = pubkey!("DEXcor9xYbiaSyvdEjKprMZuJFwe3MdgwW5kVzUiHdtu");
pub const POOL_ID: Pubkey = pubkey!("DEXissgoPzcY4bDsGfyKjy1bMxh3VFZ8yDChjjLrLg6x");
pub const REGISTRY_CONSUMER_SEED: &[u8] = b"registry-consumer";
/// The two ways ClaimsCore services a Tab's Registry budget hold: an earn
/// consumes capacity, a release returns unused capacity to the budget.
pub const CAPTURE: u8 = 2;
pub const RELEASE: u8 = 5;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct TabTerms {
    pub domain: [u8; 32],
    /// The mandate's order number for this commit. Each mandate admits each
    /// number once, through the window in `window.rs`.
    pub order: u64,
    pub principal: Pubkey,
    pub actor: Pubkey,
    pub mandate: Pubkey,
    pub asset: Pubkey,
    pub supplier: Pubkey,
    pub source: Pubkey,
    pub cap: u64,
    pub expires_at: i64,
    pub credential_generation: u64,
    pub mandate_revision: u64,
    pub external_terms_hash: [u8; 32],
    /// Settlement hold window (design section 9). Zero: the Tab is final at
    /// settlement and no hold can be set. Otherwise the buyer and the seller
    /// may each set one hold, lasting this many seconds, before the Tab closes;
    /// while a hold is in force every close of the Tab is a no-op.
    pub hold_window_seconds: u32,
    /// The Registry principal whose owner may also release a hold; the default
    /// (all-zero) key names no arbiter. Fixed width, so every later Tab field
    /// keeps its offset.
    pub arbiter: Pubkey,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, InitSpace)]
pub struct AcceptArgs {
    pub operation_id: [u8; 32],
    pub terms: TabTerms,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, InitSpace)]
pub struct StageArgs {
    pub operation_id: [u8; 32],
    pub amount: u64,
    /// Optimistic concurrency guard, excluded from the stable business identity.
    /// Exact retries retrieve the prior result even after the claim advances.
    pub expected_revision: u64,
    pub service_without_pool: bool,
}

pub fn plus(a: u64, b: u64) -> Result<u64> { a.checked_add(b).ok_or_else(|| error!(ContractError::Arithmetic)) }
pub fn minus(a: u64, b: u64) -> Result<u64> { a.checked_sub(b).ok_or_else(|| error!(ContractError::Arithmetic)) }

/// Hashes fixed-width values in wire order. The explicit acceptance instruction selects the profile.
pub fn completed_run_policy(namespace: &[u8; 32], unit: &[u8; 32],
    unit_price: u64, external_contract: &[u8; 32]) -> [u8; 32] {
    hashv(&[b"DEXTER:COMPLETED-RUN-POLICY:1", namespace, unit, &unit_price.to_le_bytes(), external_contract]).to_bytes()
}

/// Canonical financial receipt identity, independent of a payment attempt or worker.
pub fn completed_run_operation_id(supplier: &Pubkey, namespace: &[u8; 32], run: &[u8; 32]) -> [u8; 32] {
    hashv(&[b"DEXTER:COMPLETED-RUN-OPERATION:1", supplier.as_ref(), namespace, run]).to_bytes()
}

pub fn terms_digest(terms: &TabTerms) -> Result<[u8; 32]> {
    let mut encoded = Vec::with_capacity(TabTerms::INIT_SPACE);
    terms.serialize(&mut encoded)?;
    Ok(hashv(&[b"DEXTER:TAB:1", CORE_ID.as_ref(), REGISTRY_ID.as_ref(), &encoded]).to_bytes())
}

#[error_code]
pub enum ContractError {
    #[msg("Arithmetic overflow or insufficient residual amount")] Arithmetic,
    #[msg("Amount must be positive")] ZeroAmount,
    #[msg("Unknown financial effect")] UnknownEffect,
    // Code 3 belonged to the retired cash-guarantee backing check; codes are never reused.
    #[msg("Unsupported schema version")] SchemaVersion = 4,
    #[msg("Account is not owned by the expected program")] WrongOwner,
    #[msg("Account has the wrong type or encoding")] WrongAccountType,
    // Code 7 belonged to the retired receipt effect check (EffectMismatch); a new
    // variant takes 8 or later.
}

pub mod trading;
pub mod token_controls;
