//! Versioned financial meanings shared by the production implementation lanes.
//! Local development program identities only; no mainnet deployment is implied.
pub mod financing;
pub mod commercial;
pub mod funded;
pub mod corrections;
pub mod repayment;
pub mod spending;
pub mod wallet_operation;
use anchor_lang::prelude::*;
use solana_sha256_hasher::hashv;

pub const SCHEMA_VERSION: u16 = 1;
/// Same Claim payload as v1, with capture restricted to authenticated completed runs.
pub const METERED_CLAIM_VERSION: u16 = 2;
/// The outer OperationRecord version selects the allocated PDA domain. The financial effect stays v1.
pub const RUN_RECEIPT_VERSION: u16 = 2;
pub const RUN_OPERATION_SEED: &[u8] = b"run-operation";
pub const REGISTRY_ID: Pubkey = pubkey!("9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G");
pub const CORE_ID: Pubkey = pubkey!("727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz");
pub const POOL_ID: Pubkey = pubkey!("3yceApSVxLYihHjLLrCycV8T4Bg1MYggbxQhdzaqB2Sm");
pub const REGISTRY_CONSUMER_SEED: &[u8] = b"registry-consumer";
pub const POOL_DISPATCH_SEED: &[u8] = b"pool-dispatch";
pub const ACCEPT: u8 = 1;
pub const CAPTURE: u8 = 2;
pub const PAYOUT: u8 = 3;
pub const REPAY: u8 = 4;
pub const RELEASE: u8 = 5;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct TabTerms {
    pub domain: [u8; 32],
    pub tab_id: [u8; 32],
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

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Default, Debug, PartialEq, Eq, InitSpace)]
pub struct Balances {
    pub reserved: u64,
    pub payable: u64,
    pub debt: u64,
    pub protected_cash: u64,
}

impl Balances {
    pub fn transition(self, kind: u8, amount: u64) -> Result<Self> {
        require!(amount > 0, ContractError::ZeroAmount);
        let mut next = self;
        match kind {
            ACCEPT => {
                next.reserved = plus(self.reserved, amount)?;
                next.protected_cash = plus(self.protected_cash, amount)?;
            }
            CAPTURE => {
                next.reserved = minus(self.reserved, amount)?;
                next.payable = plus(self.payable, amount)?;
                next.debt = plus(self.debt, amount)?;
            }
            PAYOUT => {
                next.payable = minus(self.payable, amount)?;
                next.protected_cash = minus(self.protected_cash, amount)?;
            }
            REPAY => next.debt = minus(self.debt, amount)?,
            RELEASE => {
                next.reserved = minus(self.reserved, amount)?;
                next.protected_cash = minus(self.protected_cash, amount)?;
            }
            _ => return err!(ContractError::UnknownEffect),
        }
        require!(next.protected_cash == plus(next.reserved, next.payable)?, ContractError::BackingMismatch);
        Ok(next)
    }
}

pub fn plus(a: u64, b: u64) -> Result<u64> { a.checked_add(b).ok_or_else(|| error!(ContractError::Arithmetic)) }
pub fn minus(a: u64, b: u64) -> Result<u64> { a.checked_sub(b).ok_or_else(|| error!(ContractError::Arithmetic)) }

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default, Debug, PartialEq, Eq, InitSpace)]
pub struct SourceData {
    pub version: u16,
    pub bump: u8,
    pub pool: Pubkey,
    pub mint: Pubkey,
    pub escrow: Pubkey,
    pub treasury: Pubkey,
    pub balances: Balances,
    pub sequence: u64,
    pub head: [u8; 32],
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default, Debug, PartialEq, Eq, InitSpace)]
pub struct EffectData {
    pub version: u16,
    pub domain: [u8; 32],
    pub principal: Pubkey,
    pub operation_id: [u8; 32],
    pub request_digest: [u8; 32],
    pub claim: Pubkey,
    pub source: Pubkey,
    pub book: Pubkey,
    pub mint: Pubkey,
    pub kind: u8,
    pub amount: u64,
    pub beneficiary: Pubkey,
    pub sequence: u64,
    pub previous_hash: [u8; 32],
    pub before: Balances,
    pub after: Balances,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default, Debug, PartialEq, Eq, InitSpace)]
pub struct OperationData {
    pub version: u16,
    pub request_digest: [u8; 32],
    pub effect: EffectData,
    pub effect_hash: [u8; 32],
}

/// Hashes fixed-width values in wire order. The explicit acceptance instruction selects the profile.
pub fn completed_run_policy(namespace: &[u8; 32], unit: &[u8; 32],
    unit_price: u64, external_contract: &[u8; 32]) -> [u8; 32] {
    hashv(&[b"DEXTER:COMPLETED-RUN-POLICY:1", namespace, unit, &unit_price.to_le_bytes(), external_contract]).to_bytes()
}

/// Canonical financial receipt identity, independent of a payment attempt or worker.
pub fn completed_run_operation_id(supplier: &Pubkey, namespace: &[u8; 32], run: &[u8; 32]) -> [u8; 32] {
    hashv(&[b"DEXTER:COMPLETED-RUN-OPERATION:1", supplier.as_ref(), namespace, run]).to_bytes()
}

/// Raw IDs are application-owned. Only the authenticated originator can allocate their derived receipt.
pub fn signed_request_id(authority: Pubkey, raw_id: &[u8; 32]) -> [u8; 32] {
    hashv(&[b"DEXTER:SIGNED-REQUEST:1", authority.as_ref(), raw_id]).to_bytes()
}

/// A repayment receipt authenticates the actual token debit as well as the credited debt.
pub fn debit_digest(base: &[u8; 32], authority: Pubkey, from: Pubkey) -> [u8; 32] {
    hashv(&[b"DEXTER:DEBIT:1", base, authority.as_ref(), from.as_ref()]).to_bytes()
}

pub fn terms_digest(terms: &TabTerms) -> Result<[u8; 32]> {
    let mut encoded = Vec::with_capacity(TabTerms::INIT_SPACE);
    terms.serialize(&mut encoded)?;
    Ok(hashv(&[b"DEXTER:TAB:1", CORE_ID.as_ref(), REGISTRY_ID.as_ref(), &encoded]).to_bytes())
}

/// Identifies the authorized economic operation, independently of transaction
/// signatures, blockhashes, fee payer and optimistic revision guards.
pub fn operation_digest(terms: &TabTerms, kind: u8, operation_id: &[u8; 32], amount: u64, service_without_pool: bool) -> Result<[u8; 32]> {
    let terms_hash = terms_digest(terms)?;
    Ok(hashv(&[b"DEXTER:OPERATION:1", &terms_hash, &[kind], operation_id,
        &amount.to_le_bytes(), &[u8::from(service_without_pool)]]).to_bytes())
}

pub fn effect_digest(effect: &EffectData) -> Result<[u8; 32]> {
    require!(effect.version == SCHEMA_VERSION, ContractError::SchemaVersion);
    let mut encoded = Vec::with_capacity(EffectData::INIT_SPACE);
    effect.serialize(&mut encoded)?;
    Ok(hashv(&[b"DEXTER:BOOK_EFFECT:1", CORE_ID.as_ref(), POOL_ID.as_ref(), &encoded]).to_bytes())
}

/// Cross-program source readers share the same versioned body as Core's
/// #[account] Source wrapper; Core ownership and discriminator are mandatory.
pub fn read_core_source(account: &AccountInfo) -> Result<SourceData> {
    let value: SourceData = read_core_account(account, b"account:Source")?;
    require!(value.version == SCHEMA_VERSION, ContractError::SchemaVersion);
    let (expected, bump) = Pubkey::find_program_address(&[b"source", value.pool.as_ref(), value.mint.as_ref()], &CORE_ID);
    require_keys_eq!(account.key(), expected, ContractError::WrongAccountType);
    require!(value.bump == bump, ContractError::WrongAccountType);
    Ok(value)
}

pub fn read_core_operation(account: &AccountInfo) -> Result<OperationData> {
    let value: OperationData = read_core_account(account, b"account:OperationRecord")?;
    let seed: &[u8] = match value.version {
        SCHEMA_VERSION => b"operation",
        RUN_RECEIPT_VERSION if value.effect.kind == CAPTURE => RUN_OPERATION_SEED,
        _ => return err!(ContractError::SchemaVersion),
    };
    require!(value.effect_hash == effect_digest(&value.effect)?, ContractError::EffectMismatch);
    require!(value.request_digest == value.effect.request_digest, ContractError::EffectMismatch);
    let expected = Pubkey::find_program_address(&[seed, value.effect.principal.as_ref(),
        &value.effect.operation_id], &CORE_ID).0;
    require_keys_eq!(account.key(), expected, ContractError::WrongAccountType);
    Ok(value)
}

fn read_core_account<T: AnchorDeserialize>(account: &AccountInfo, name: &[u8]) -> Result<T> {
    require_keys_eq!(*account.owner, CORE_ID, ContractError::WrongOwner);
    let data = account.try_borrow_data()?;
    let discriminator = hashv(&[name]).to_bytes();
    require!(data.len() >= 8 && data[..8] == discriminator[..8], ContractError::WrongAccountType);
    T::try_from_slice(&data[8..]).map_err(|_| error!(ContractError::WrongAccountType))
}

#[error_code]
pub enum ContractError {
    #[msg("Arithmetic overflow or insufficient residual amount")] Arithmetic,
    #[msg("Amount must be positive")] ZeroAmount,
    #[msg("Unknown financial effect")] UnknownEffect,
    #[msg("Accounted guarantee backing does not equal reserved plus payable")] BackingMismatch,
    #[msg("Unsupported schema version")] SchemaVersion,
    #[msg("Account is not owned by the expected program")] WrongOwner,
    #[msg("Account has the wrong type or encoding")] WrongAccountType,
    #[msg("Receipt digest does not match its canonical financial effect")] EffectMismatch,
}
