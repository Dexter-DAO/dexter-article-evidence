use anchor_lang::prelude::*;

/// Current vault layout version. Read from byte 0 of the account; a v2 program
/// rejects accounts whose `version` byte does not match. Future v3 layouts can
/// branch on the same byte to handle each generation explicitly. See
/// docs/DESIGN-vault-v2-session-keys.md §4 for the rationale.
pub const VAULT_VERSION_V2: u8 = 2;

/// v3 adds the credex meter fields (current_outstanding, max_revolving_capacity)
/// to SessionRegistration, enlarging Vault::INIT_SPACE. New vaults init as v3.
/// v2 vaults keep working on lock-only paths but cannot register a revolving
/// (enlarged) session. See docs/superpowers/plans/2026-06-02-revolving-capacity-meter.md.
pub const VAULT_VERSION_V3: u8 = 3;

/// V4 appends LockedClaim accounting: three u64s to `Vault`
/// (`outstanding_locked_amount`, `total_crystallized_amount`, `total_settled_amount`)
/// and `crystallized_cumulative: u64` + `last_locked_sequence: u32` to
/// `SessionRegistration`. Enlarges `Vault::INIT_SPACE`. Historical init
/// version (new vaults initialized as V4 through the V4-era builds).
pub const VAULT_VERSION_V4: u8 = 4;

/// V5 appends credit accounting: external-financier standby backing.
/// `borrowed` is the "buyer is negative" accumulator. V5 is a migration
/// waypoint only (migrate_v4_to_v5) — never an init version.
pub const VAULT_VERSION_V5: u8 = 5;

/// V6 moves sessions OUT of the Vault into per-counterparty SessionAccount PDAs.
/// The Vault loses `active_session` (the Option) and gains `live_session_count: u8`.
/// CURRENT init version: initialize_vault writes the V6 layout and stamps V6
/// directly. Pre-existing vaults reach V6 via migration (migrate_v5_to_v6).
/// (The V6-era builds before this fix wrote the V6 LAYOUT but stamped V4 —
/// the "born-broken" cohort, since repaired by the admit-mismatch migration.)
pub const VAULT_VERSION_V6: u8 = 6;
/// V7 — the post-graph-rewrite layout FINALLY getting its own number (2026-07-01).
/// The 2026-06-26 graph rewrite changed the Vault byte layout (deleted the V5-era
/// credit fields, added `node`) but kept the version byte at 6 — leaving 511
/// old-layout mainnet accounts that still PASSED `version == 6` gates while
/// deserializing garbage into `node`. V7 closes that: only accounts stamped by
/// post-rewrite code (initialize_vault, the migrations below, migrate_v6_to_v7)
/// carry 7, and every handler gates on 7. Old-layout accounts fail closed with
/// UnsupportedVaultVersion. migrate_v6_to_v7 enforces the LENGTH check (the one
/// byte-level discriminator between the two "V6" generations) at the single
/// choke point, so per-handler gates stay a cheap version compare.
pub const VAULT_VERSION_V7: u8 = 7;

/// SessionAccount layout version. ZERO on a freshly-created (runtime-zeroed)
/// account; set to this on first touch. The register handler reads `version == 0`
/// to distinguish "new create" (increment count) from "replace" (count unchanged).
pub const SESSION_VERSION_V1: u8 = 1;
/// Recovery-only marker for a SessionAccount whose exact V1 bytes were
/// attested before the version byte was restamped. Generic session handlers
/// reject this version; only the legacy resolution paths may consume it.
pub const SESSION_VERSION_LEGACY_V6_RECOVERED_V1: u8 = 2;

/// While set, the low seven bits retain the exact historical live-session
/// count and every generic session/withdrawal path fails closed. Recovery
/// proves at most six sessions, so this bit cannot collide with a valid count.
pub const LEGACY_V6_RESOLUTION_PENDING: u8 = 1 << 7;
pub const LIVE_SESSION_COUNT_MASK: u8 = LEGACY_V6_RESOLUTION_PENDING - 1;

pub fn legacy_v6_resolution_pending(live_session_count: u8) -> bool {
    live_session_count & LEGACY_V6_RESOLUTION_PENDING != 0
}

pub fn live_session_count_without_recovery_flag(live_session_count: u8) -> u8 {
    live_session_count & LIVE_SESSION_COUNT_MASK
}

#[account]
#[derive(InitSpace)]
pub struct Vault {
    /// Layout version. MUST be the first field so byte 0 of the deserialized
    /// account directly indicates which Vault generation this is. A program
    /// bound to v2 rejects (`VaultError::UnsupportedVaultVersion`) anything
    /// that isn't `VAULT_VERSION_V2`.
    pub version: u8,
    pub bump: u8,
    pub passkey_pubkey: [u8; 33],
    pub swig_address: Pubkey,
    /// Minimum delay between `request_withdrawal` and `finalize_withdrawal`.
    /// `u32` because negative is meaningless and 136 years of seconds is plenty.
    pub cooling_off_seconds: u32,
    pub pending_voucher_count: u32,
    pub pending_withdrawal: Option<PendingWithdrawal>,
    /// Operator-defined opaque identity claim (formerly `supabase_user_id`).
    /// The protocol does not interpret these bytes; Dexter writes a Supabase
    /// UUID prefix, future operators may write whatever they want. Documented
    /// in the OTS spec as "operator-defined".
    pub identity_claim: [u8; 32],
    /// The session authority recorded at init — the ONLY key permitted to
    /// mutate `pending_voucher_count` (settle_voucher / force_release).
    pub dexter_authority: Pubkey,
    /// V6: number of LIVE session PDAs for this vault. Incremented by
    /// register_session_key on first-touch (new counterparty), decremented by
    /// revoke_session_key. The register-time overcommit gate requires the caller
    /// to pass exactly this many live siblings (minus the target if it already
    /// exists), so the summed-cap gate cannot be gamed low. Range 0..=255.
    pub live_session_count: u8,
    /// Sum of unsettled LockedClaim amounts for this vault. Rises at
    /// `lock_voucher`, falls at `settle_locked_voucher` / `recover_abandoned_lock`.
    /// The crystallized (buyer-irrevocable) reservation tier. Read by
    /// `finalize_withdrawal` to reject withdrawals that would violate the
    /// reservation. (Seam spec section 1.)
    pub outstanding_locked_amount: u64,
    /// Lifetime monotonic locked-into-claim odometer at vault scope. Never decremented.
    pub total_crystallized_amount: u64,
    /// Lifetime monotonic settled-from-claim odometer at vault scope. Never decremented.
    pub total_settled_amount: u64,
    /// The wallet↔node link. `Pubkey::default()` at init = UNLINKED (this vault
    /// participates in no delegation graph yet). Set by `attach_node` (Task 10)
    /// to the `PrincipalNode` PDA whose credit authority governs this wallet's
    /// draws. The depth-N graph replaces the retired depth-1 credit fields
    /// (`borrowed` / `standby_backer` / `standby_cap` / `borrow_recovery_at`):
    /// all credit state now lives on the `PrincipalNode`, not the Vault.
    pub node: Pubkey,
}

/// Canonical per-vault replay guard for WebAuthn authorizations.
/// Kept outside `Vault` so deployed V7 vault data is not resized.
/// There is intentionally no close or reset path.
#[account]
#[derive(InitSpace)]
pub struct PasskeyAuthorization {
    pub version: u8,
    pub bump: u8,
    pub vault: Pubkey,
    pub nonce: u64,
}

pub const PASSKEY_AUTHORIZATION_VERSION_V1: u8 = 1;

/// Transient intra-tx state carrier + one permanent replay counter shared by
/// governed carry Trade and Protected Send.
///
/// One per vault at `[SWAP_BRACKET_SEED, vault]`, created lazily on the first
/// swap (`init_if_needed`, fee-payer rent). The bracket fields are meaningful
/// ONLY between `swap_for_carry` (writes) and `finish_swap` (consumes + clears)
/// inside one atomic transaction — `swap_for_carry` forward-introspects that
/// `finish_swap` is present two instructions ahead, so `in_flight = true` can
/// never persist past the transaction (the tx either reaches finish_swap or
/// reverts whole). `nonce` alone is durable: direction-2 Send increments it in
/// `swap_for_carry`; Buy/Sell increment it in `finish_swap`. Both operation
/// families sign the current value, and any later failure rolls the increment
/// back. This PDA must never be closed or separate route-specific counters
/// would make old ceremonies replayable.
#[account]
#[derive(InitSpace)]
pub struct SwapBracket {
    /// Layout version. First field per house style.
    pub version: u8,
    /// Canonical bump for this PDA.
    pub bump: u8,
    /// The vault this bracket is 1:1 with.
    pub vault: Pubkey,
    /// Durable per-vault replay counter shared by signed Trade and Send intents.
    /// HARD INVARIANT:
    /// this is monotonic and the bracket is NEVER closed — do not add a
    /// close_swap_bracket (rent-reclaim) that lets the PDA be recreated at
    /// nonce 0, or every previously-signed low-nonce intent becomes replayable.
    pub nonce: u64,
    /// True only between swap_for_carry and finish_swap within one tx.
    pub in_flight: bool,
    /// 0 = buy (USDC -> base), 1 = sell (base -> USDC), 2 = protected send.
    pub direction: u8,
    /// Maximum input the swap may spend (passkey-signed bound).
    pub amount_in: u64,
    /// Minimum output the vault must receive (passkey-signed bound).
    pub min_out: u64,
    /// The base asset of this swap (binds the AssetConfig row + base ATA).
    pub base_mint: Pubkey,
    /// Balance snapshots taken by swap_for_carry, consumed by finish_swap.
    pub usdc_before: u64,
    pub base_before: u64,
    /// Transaction-position guards: finish_swap must run in the SAME slot at
    /// EXACTLY snapshot_ix_index + 2 (marker, SignV2, finish).
    pub snapshot_slot: u64,
    pub snapshot_ix_index: u16,
}

pub const SWAP_BRACKET_VERSION_V1: u8 = 1;

/// Owner-created authorization for one unattended governed-trading role.
///
/// The Swig role itself is deliberately a NON-session ProgramExec authority
/// pinned to `vault::swap_for_carry`, with exactly TokenLimit(input mint) plus
/// Program(Jupiter v6). The agent Ed25519 signer lives here instead: every
/// agent `authorize_governed_swap` must supply this row and sign the outer
/// transaction as `agent_session_pubkey`. This composes the signer and exact
/// Vault envelope without relying on Swig's ProgramExecSession implementation
/// (whose session path does not enforce its program/prefix).
///
/// PDA: `[GOVERNED_TRADE_GRANT_SEED, vault, grant_nonce_le]`. The account is
/// never closed; `active = false` is a permanent replay tombstone.
#[account]
#[derive(InitSpace)]
pub struct GovernedTradeGrant {
    pub version: u8,
    pub bump: u8,
    pub active: bool,
    pub vault: Pubkey,
    pub swig: Pubkey,
    pub role_id: u32,
    pub grant_nonce: u64,
    pub dexter_master_pubkey: Pubkey,
    pub agent_session_pubkey: Pubkey,
    pub input_mint: Pubkey,
    pub spend_limit_atomic: u64,
    pub session_ttl_seconds: u64,
    pub granted_at_slot: u64,
    pub expires_at_slot: u64,
    pub jupiter_program: Pubkey,
}

pub const GOVERNED_TRADE_GRANT_VERSION_V1: u8 = 1;

/// One owner-approved agent authority root for autonomous governed Send.
/// Revoking this row invalidates every child grant immediately. A revoked row
/// is a permanent tombstone and cannot be reactivated; rotating authority
/// therefore requires a fresh, domain-specific agent subkey and PDA.
#[account]
#[derive(InitSpace)]
pub struct GovernedSendAuthorityRoot {
    pub version: u8,
    pub bump: u8,
    pub active: bool,
    pub vault: Pubkey,
    pub swig: Pubkey,
    /// sha256(compressed owner passkey) at enrollment. Passkey rotation makes
    /// every descendant fail closed without walking an authority list.
    pub owner_authority_binding: [u8; 32],
    /// Dedicated Send transaction signer. It is never a Tab voucher key,
    /// trade key, or the shared role-2 master by protocol convention.
    pub agent_session_pubkey: Pubkey,
    /// Owner-authored parent mandate epoch and settings commitment.
    pub authority_epoch: u64,
    pub root_settings_digest: [u8; 32],
    pub created_at_slot: u64,
    pub revoked_at_slot: u64,
}

pub const GOVERNED_SEND_AUTHORITY_ROOT_VERSION_V1: u8 = 1;

/// Permanent asset-specific child authority for autonomous governed Send.
/// PDA: `[GOVERNED_SEND_GRANT_SEED, vault, grant_nonce_le]`.
/// `active=false` is a permanent replay tombstone; the row is never closed.
#[account]
#[derive(InitSpace)]
pub struct GovernedSendGrant {
    pub version: u8,
    pub bump: u8,
    pub active: bool,
    pub vault: Pubkey,
    pub swig: Pubkey,
    pub authority_root: Pubkey,
    pub agent_session_pubkey: Pubkey,
    pub authority_epoch: u64,
    pub root_settings_digest: [u8; 32],
    pub grant_nonce: u64,
    pub grant_revision: u64,
    /// Digest of the authoritative registry record resolved before consent.
    pub registry_identity_digest: [u8; 32],
    pub asset_mint: Pubkey,
    pub token_program: Pubkey,
    pub decimals: u8,
    pub per_action_cap_atomic: u64,
    pub aggregate_cap_atomic: u64,
    pub spent_atomic: u64,
    /// Merkle root of exact destination-owner leaves for this grant.
    pub destination_root: [u8; 32],
    pub session_ttl_seconds: u64,
    pub granted_at_slot: u64,
    pub expires_at_slot: u64,
    pub allow_destination_ata_creation: bool,
    /// Exact payer allowed to fund destination ATA rent; default forbids it.
    pub ata_rent_payer: Pubkey,
    pub max_ata_rent_lamports_per_action: u64,
    pub aggregate_ata_rent_cap_lamports: u64,
    pub ata_rent_spent_lamports: u64,
    /// Monotonic successful authorization count. Transaction rollback keeps
    /// this unchanged when the adjacent transfer fails.
    pub sequence: u64,
}

pub const GOVERNED_SEND_GRANT_VERSION_V1: u8 = 1;

/// Catalog-wide governed stock authority. These rows are deliberately
/// additive and use distinct account discriminators from V1.
pub const CATALOG_POLICY_VERSION_V2: u8 = 2;
pub const GOVERNED_ASSET_IDENTITY_VERSION_V2: u8 = 2;
pub const GOVERNED_ASSET_CURRENT_VERSION_V2: u8 = 2;
pub const GOVERNED_TRADE_GRANT_VERSION_V2: u8 = 2;
pub const GOVERNED_ASSET_ROLE_VERSION_V2: u8 = 2;
pub const GOVERNED_SELL_METER_VERSION_V2: u8 = 2;

pub const GOVERNED_ASSET_CLASS_STOCK: u8 = 1;
pub const GOVERNED_TRADE_DIRECTION_BUY: u8 = 1 << 0;
pub const GOVERNED_TRADE_DIRECTION_SELL: u8 = 1 << 1;
pub const GOVERNED_TRADE_DIRECTION_ALL: u8 =
    GOVERNED_TRADE_DIRECTION_BUY | GOVERNED_TRADE_DIRECTION_SELL;

#[account]
#[derive(InitSpace)]
pub struct CatalogPolicyV2 {
    pub version: u8,
    pub bump: u8,
    pub active: bool,
    pub asset_class: u8,
    pub direction_mask: u8,
    pub admin_authority: Pubkey,
    /// Independent route/quote guard. A key change requires an epoch advance,
    /// which invalidates every owner grant bound to the previous epoch/key.
    pub quote_attestation_authority: Pubkey,
    pub policy_identity_digest: [u8; 32],
    pub epoch: u64,
    pub created_at_unix: i64,
    pub updated_at_unix: i64,
    pub updated_at_slot: u64,
}

/// Immutable versioned legal/product/execution identity for one exact mint.
/// Changed facts create the next version and advance the separate pointer.
#[account]
#[derive(InitSpace)]
pub struct GovernedAssetIdentityV2 {
    pub version: u8,
    pub bump: u8,
    pub asset_class: u8,
    pub initial_direction_mask: u8,
    pub policy: Pubkey,
    pub policy_epoch: u64,
    pub mint: Pubkey,
    pub token_program: Pubkey,
    pub decimals: u8,
    pub identity_version: u64,
    /// Governed registryIdentityDigest; never the mutable productVersionDigest.
    pub product_identity_digest: [u8; 32],
    pub legal_issuer_identity_digest: [u8; 32],
    /// Released executionProfileDigest. Its canonical preimage includes the
    /// exact finalized onchainVersionDigest selected by the catalog release.
    pub execution_profile_digest: [u8; 32],
    pub release_digest: [u8; 32],
    pub created_at_unix: i64,
    pub created_at_slot: u64,
}

/// Current policy+mint pointer. It may advance to a higher immutable version,
/// disable, or narrow directions; it can never rewrite identity facts.
#[account]
#[derive(InitSpace)]
pub struct GovernedAssetCurrentV2 {
    pub version: u8,
    pub bump: u8,
    pub active: bool,
    pub direction_mask: u8,
    pub policy: Pubkey,
    pub policy_epoch: u64,
    pub mint: Pubkey,
    pub current_identity: Pubkey,
    pub current_identity_version: u64,
    pub updated_at_unix: i64,
    pub updated_at_slot: u64,
}

#[account]
#[derive(InitSpace)]
pub struct GovernedTradeGrantV2 {
    pub version: u8,
    pub bump: u8,
    pub active: bool,
    pub direction_mask: u8,
    pub vault: Pubkey,
    pub swig: Pubkey,
    pub catalog_policy: Pubkey,
    pub policy_identity_digest: [u8; 32],
    pub policy_epoch: u64,
    pub grant_nonce: u64,
    pub provision_manager_role_id: u32,
    /// Stable wallet automation role whose disposable live signer authorizes
    /// trades. Owner consent binds the stable authority, never one ephemeral
    /// technical key, so session rotation does not renew this grant.
    pub automation_role_id: u32,
    pub automation_authority_pubkey: Pubkey,
    pub facilitator_fee_payer: Pubkey,
    pub quote_attestation_authority: Pubkey,
    pub usdc_mint: Pubkey,
    pub jupiter_program: Pubkey,
    pub buy_per_order_limit_atomic: u64,
    pub buy_window_limit_atomic: u64,
    pub buy_window_seconds: u64,
    pub sell_window_seconds: u64,
    pub sell_max_position_bps: u16,
    pub max_slippage_bps: u16,
    /// Conservative, upward-rounded impact in whole basis points.
    pub max_price_impact_bps: u16,
    pub issued_at_unix: i64,
    pub issued_at_slot: u64,
    pub buy_window_started_at_unix: i64,
    pub buy_window_ends_at_unix: i64,
    pub buy_window_started_at_slot: u64,
    pub buy_window_spent_atomic: u64,
}

#[account]
#[derive(InitSpace)]
pub struct GovernedAssetRoleV2 {
    pub version: u8,
    pub bump: u8,
    pub active: bool,
    pub direction: u8,
    pub grant: Pubkey,
    pub input_mint: Pubkey,
    pub role_id: u32,
    pub provision_sequence: u64,
    /// Nonterminal Swig defense-in-depth ceiling. Buy mirrors the owner's
    /// recurring cash window; Sell uses the type maximum because the dynamic
    /// percentage-of-position meter below is the economic authority.
    pub recurring_limit_atomic: u64,
    pub recurring_window_slots: u64,
    pub provisioned_at_unix: i64,
    pub provisioned_at_slot: u64,
    /// Durable reconciliation evidence for the exact Swig RemoveAuthority leg.
    /// Zero while active; set after explicit grant revocation only when either the exact
    /// RemoveAuthority leg lands or the recorded child is already absent.
    pub cleaned_at_unix: i64,
    pub cleaned_at_slot: u64,
    /// 0 while active, 1 for an atomically inspected RemoveAuthority leg, and
    /// 2 for manager-free reconciliation of a child role already absent from
    /// Swig. This prevents reconciliation from pretending it performed the
    /// external authority removal.
    pub cleanup_kind: u8,
}

/// Per-grant+mint human-exact Sell window. The first sell freezes the actual
/// pre-sell position and `cap=floor(baseline*bps/10_000)`.
#[account]
#[derive(InitSpace)]
pub struct GovernedSellMeterV2 {
    pub version: u8,
    pub bump: u8,
    pub grant: Pubkey,
    pub mint: Pubkey,
    pub window_started_at_unix: i64,
    pub window_ends_at_unix: i64,
    pub window_started_at_slot: u64,
    pub baseline_balance_atomic: u64,
    pub cap_atomic: u64,
    pub cumulative_sold_atomic: u64,
}

/// Persistent wallet-wide recurring automation budget. The owner-authored
/// amount is read from the canonical mode-2 Swig role at initialization. This
/// Vault-owned meter is authoritative across direct x402 payments and stock
/// Buys because Swig only debits the role that executes a SignV2 instruction;
/// a provisioned stock child role cannot debit its parent automation role.
#[account]
#[derive(InitSpace)]
pub struct AutomationBudgetV2 {
    pub version: u8,
    pub bump: u8,
    pub active: bool,
    pub vault: Pubkey,
    pub swig: Pubkey,
    pub automation_role_id: u32,
    pub automation_authority_pubkey: Pubkey,
    pub settlement_mint: Pubkey,
    pub window_slots: u64,
    pub limit_atomic: u64,
    pub window_started_slot: u64,
    pub window_spent_atomic: u64,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, InitSpace)]
pub struct PendingWithdrawal {
    pub amount: u64,
    pub destination: Pubkey,
    pub requested_at: i64,
}

/// On-chain record of an authorized session key.
///
/// The session pubkey is an ordinary ed25519 keypair the buyer's SDK generated
/// in memory at tab-open time. The passkey signed a 180-byte registration
/// message (see docs/DESIGN-vault-v2-session-keys.md §2.2) endorsing these
/// scope limits. From this point on, the seller's middleware accepts vouchers
/// signed by `session_pubkey` for this vault, up to `max_amount` and before
/// `expires_at`, only for `allowed_counterparty`.
///
/// `spent` is the running cumulative — incremented by settle paths that close
/// vouchers — so we can enforce the cap across the lifetime of the session
/// without an additional read.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, InitSpace)]
pub struct SessionRegistration {
    pub session_pubkey: [u8; 32],
    pub max_amount: u64,
    pub expires_at: i64,
    pub allowed_counterparty: Pubkey,
    pub nonce: u32,
    pub spent: u64,
    /// Live unsettled exposure. Rises at tab-open (settle_voucher increment),
    /// falls at confirmed settle (settle_tab_voucher). This is the field that
    /// REVOLVES — the credex meter.
    pub current_outstanding: u64,
    /// Admission cap the revolving meter is checked against. Set + passkey-
    /// endorsed at register_session_key. May be <= max_amount.
    pub max_revolving_capacity: u64,
    /// Session-scope monotonic locked-into-claim odometer; mirror of `spent`
    /// for the lock terminal path. Rises at `lock_voucher`. Never decremented.
    /// The XOR frontier `max(spent, crystallized_cumulative)` gates both
    /// terminal paths (seam spec section 4). (Seam spec section 1.)
    pub crystallized_cumulative: u64,
    /// Last voucher sequence number that was locked. Reserved for future
    /// out-of-order lock detection — NOT the XOR guard (the frontier is).
    pub last_locked_sequence: u32,
}

/// A single authorized session, now a standalone account instead of a Vault
/// field. PDA: seeds = [SESSION_SEED, vault.key(), session.allowed_counterparty].
/// One per (vault, counterparty); the seed binding gives "one tab per app,
/// re-register replaces in place". `version == 0` means freshly-created/cleared.
#[account]
#[derive(InitSpace)]
pub struct SessionAccount {
    /// First-touch / migration discriminator. 0 = never touched (or cleared by
    /// revoke). SESSION_VERSION_V1 once written. NOT the Anchor discriminator
    /// (Anchor sets that on init_if_needed before the handler runs) — this is a
    /// program-controlled field, the only reliable new-vs-replace signal.
    pub version: u8,
    /// Stored canonical bump → create_program_address (one-shot, no ~1500 CU
    /// find_program_address search) when re-deriving siblings in the gate.
    pub bump: u8,
    /// The vault this session authorizes spending FROM. Bound at first touch.
    pub vault: Pubkey,
    /// The session registration scope — UNCHANGED struct, reused verbatim.
    pub session: SessionRegistration,
}

/// Crystallized claim against vault USDC. Created by `lock_voucher`,
/// transferable via `transfer_lock_ownership`, settled by
/// `settle_locked_voucher`, reclaimed by `recover_abandoned_lock`. Independent
/// of any `SessionAccount` after creation — see V0.3 Decision 7. State machine
/// per V0.3 Decision 6: pending → {settled, abandoned}, both terminal one-way.
#[account]
#[derive(InitSpace)]
pub struct LockedClaim {
    /// Layout version. v0.3 is the first version.
    pub version: u8,
    pub bump: u8,
    /// Vault PDA this claim is reserved against. All reservation invariants
    /// gate on this vault's `outstanding_locked_amount`.
    pub vault: Pubkey,
    /// Session pubkey that signed the voucher being crystallized. Snapshot at
    /// lock time per V0.3 Decision 7; never re-read at settlement.
    pub session_pubkey_at_lock: [u8; 32],
    /// Voucher payload hash — sha256 of the exact signed voucher message
    /// (44-byte grandfathered V1 payload or 32-byte context-bound V2 digest).
    /// For audit / indexer correlation; not re-verified at settle.
    pub voucher_hash: [u8; 32],
    /// USDC amount this claim reserves against the vault. `delta` from
    /// `voucher.cumulative_amount - session.crystallized_cumulative` at lock.
    pub amount: u64,
    /// Wall-clock time the lock instruction landed.
    pub created_at: i64,
    /// Earliest time settlement may run. If None, claim is instantly
    /// settleable.
    pub maturity_at: Option<i64>,
    /// Earliest time the buyer's passkey may reclaim. If None, claim is
    /// indefinitely buyer-irrevocable. Invariant: when both set, MUST satisfy
    /// `holder_recovery_at > maturity_at` per V0.3 Decision 4.
    pub holder_recovery_at: Option<i64>,
    /// Current owner. Set at creation to the seller; mutated by
    /// `transfer_lock_ownership` signed by the previous holder.
    pub current_holder: Pubkey,
    /// State machine status per V0.3 Decision 6.
    pub status: LockedClaimStatus,
    /// Set when status == Settled. None otherwise.
    pub settled_at: Option<i64>,
    /// Set when status == Abandoned. None otherwise.
    pub recovered_at: Option<i64>,
    /// #77 on-chain seller binding (crystallization-cadence spec 2026-07-06
    /// §6e): the seller this claim's voucher was scoped to — the session's
    /// `allowed_counterparty`, snapshotted at lock time from the SEED-PROVEN
    /// value (the session PDA derives from it, so it cannot be forged by the
    /// lock caller). `Some` pins settle_locked_voucher's money-leg destination
    /// to THIS seller's canonical USDC ATA, so settling a claim never depends
    /// on mutable off-chain (Redis) claim→seller state and an orphaned-Pending
    /// claim (lost off-chain record) can always still pay its rightful seller.
    ///
    /// TAIL-CARVE COMPATIBILITY (why Option, why last): every pre-#77 mainnet
    /// claim is a 191-byte account (8 disc + 183 = the old INIT_SPACE) whose
    /// Borsh content is at most 175 bytes (all four Option<i64> fields Some is
    /// unreachable — settled_at/recovered_at are mutually exclusive), so at
    /// least 8 zero bytes of allocation padding always follow the content.
    /// Deserializing an old claim with this struct reads the Option tag from
    /// that guaranteed-zero padding → `None` (= legacy, no binding), and a
    /// write-back (recover_abandoned_lock's status flip) re-serializes with an
    /// explicit None tag at ≤176 bytes — still inside the old allocation. New
    /// claims are allocated 32+1 bytes larger and always carry `Some`.
    /// Pinned by the layout unit tests in `locked_claim_layout_tests` below.
    pub allowed_counterparty: Option<Pubkey>,
}

/// Claim state per V0.3 Decision 6 state machine.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq, InitSpace)]
pub enum LockedClaimStatus {
    Pending,
    Settled,
    Abandoned,
}

/// LockedClaim PDA seed prefix. Per-claim PDA derived as
/// `[LOCKED_CLAIM_SEED, vault_pda, voucher_hash]` so each unique voucher
/// crystallizes to a unique account address.
pub const LOCKED_CLAIM_SEED: &[u8] = b"locked-claim";

/// LockedClaim layout version.
pub const LOCKED_CLAIM_VERSION_V1: u8 = 1;

// ---------------------------------------------------------------------------
// Frozen legacy depth-1 recovery evidence.
// ---------------------------------------------------------------------------

/// Historical financier reserve ledger, reintroduced read/write solely so the
/// 279-byte V6 recovery can retire the old line without forgiving its promise.
///
/// The name, field order, and types are the exact pre-graph account ABI. Keeping
/// the historical type name is load-bearing: Anchor's discriminator is derived
/// from `StandbyBacker`, so renaming this reader would make every live ledger
/// look foreign.
#[account]
#[derive(InitSpace)]
pub struct StandbyBacker {
    pub version: u8,
    pub bump: u8,
    pub financier_swig: Pubkey,
    pub committed_reserve: u64,
    pub aggregate_promised: u64,
    pub reserve_kind: ReserveKind,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq, InitSpace)]
pub enum ReserveKind {
    Declared,
    Locked,
}

pub const STANDBY_BACKER_VERSION_V1: u8 = 1;

/// Immutable byte-for-byte recovery image of one canonical SessionAccount.
/// Every historical field is copied explicitly and the full allocated account
/// bytes are hashed, so later resolution never relies on an aggregate frontier
/// or on pending-counter inference.
#[derive(
    AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, Default, PartialEq, Eq, InitSpace,
)]
pub struct LegacyV6SessionAttestation {
    pub present: bool,
    pub session_account: Pubkey,
    pub account_version: u8,
    pub account_bump: u8,
    pub vault: Pubkey,
    pub session_pubkey: [u8; 32],
    pub max_amount: u64,
    pub expires_at: i64,
    pub allowed_counterparty: Pubkey,
    pub nonce: u32,
    pub spent: u64,
    pub current_outstanding: u64,
    pub max_revolving_capacity: u64,
    pub crystallized_cumulative: u64,
    pub last_locked_sequence: u32,
    pub recovered_data_sha256: [u8; 32],
}

/// Immutable lossless-recovery receipt. There is deliberately no mutation or
/// close path for this PDA. Debt-bearing recoveries also pin both
/// `PrincipalNode.controller` and `root_attestation` to this PDA, making the
/// recovered node sealed: no externally signable key can expand or re-root it.
#[account]
#[derive(InitSpace)]
pub struct LegacyV6Attestation {
    pub version: u8,
    pub bump: u8,
    pub vault: Pubkey,
    pub legacy_vault_bump: u8,
    pub dexter_authority: Pubkey,
    /// Authority written into the recovered V7 Vault. It equals the historical
    /// authority on the preserve path, or the exact signer/passkey-authorized
    /// replacement on a rotation path.
    pub recovered_dexter_authority: Pubkey,
    /// 0 = recorded authority signed; 1 = owner passkey consent was consumed.
    pub recovery_authorization_mode: u8,
    pub recovery_consent: Option<Pubkey>,
    pub recovery_consent_digest: [u8; 32],
    pub recovery_consent_generation: u64,
    /// The account that funded recovery rent; sealed-node close returns rent here.
    pub recovery_payer: Pubkey,
    /// Immutable recovery time used to bound legacy-voucher grace.
    pub recovered_at: i64,
    /// Terminal edge of the owner-bound legacy V1 voucher grace interval.
    pub legacy_voucher_settlement_deadline: i64,
    /// SHA-256 over all original 279 account bytes, discriminator and padding.
    pub original_data_sha256: [u8; 32],
    pub old_backer: Option<Pubkey>,
    pub old_cap: u64,
    pub old_borrowed: u64,
    pub old_borrow_recovery_at: Option<i64>,
    pub reserve_ledger_present: bool,
    /// Exact old reserve-ledger bytes before the full-cap retirement.
    pub reserve_data_sha256_before: [u8; 32],
    pub reserve_committed: u64,
    pub reserve_aggregate_before: u64,
    pub reserve_aggregate_after: u64,
    pub reserve_kind: Option<ReserveKind>,
    /// Existing passkey replay state is observed, never reset.
    pub passkey_nonce_snapshot: u64,
    pub passkey_valid: bool,
    /// Bitset of the explicit fail-closed conditions below.
    pub quarantine_flags: u16,
    pub swig: Pubkey,
    pub swig_binding_preexisting: bool,
    pub live_session_count: u8,
    pub session_account_count: u16,
    pub session_max_amount_sum: u64,
    pub session_current_outstanding_sum: u64,
    pub session_attestations:
        [LegacyV6SessionAttestation; crate::constants::LEGACY_V6_RECOVERY_MAX_SESSIONS],
    /// Domain-separated hash of each canonical session key + exact bytes.
    pub session_snapshot_sha256: [u8; 32],
    pub pending_claim_count: u16,
    pub pending_claim_amount: u64,
    /// Domain-separated hash of each canonical pending-claim key + exact bytes.
    pub claim_snapshot_sha256: [u8; 32],
    pub outstanding_locked_amount: u64,
    pub pending_voucher_count: u32,
    pub pending_withdrawal_snapshot: Option<PendingWithdrawal>,
    /// SHA-256 of the post-upgrade reveal salt used to derive the reserved node.
    pub recovery_salt_sha256: [u8; 32],
    /// Some for every historical nonzero cap. The frozen node preserves the
    /// facility claim while its zero velocity prevents new advances.
    pub recovery_node: Option<Pubkey>,
}

pub const LEGACY_V6_ATTESTATION_VERSION_V1: u8 = 1;
pub const LEGACY_V6_QUARANTINE_DEFAULT_SWIG: u16 = 1 << 0;
pub const LEGACY_V6_QUARANTINE_INVALID_PASSKEY: u16 = 1 << 1;
/// The legacy Vault records a non-default Swig address whose account was truly
/// absent in the finalized recovery census. It is preserved as evidence but is
/// not operational and receives no reverse binding.
pub const LEGACY_V6_QUARANTINE_DEAD_SWIG: u16 = 1 << 2;
/// The historical counter/session frontier has no owner-bound V1 channel.
/// Recovery preserves it, but no legacy settle/lock terminal is authorized
/// until a later exact owner-passkey voucher binding is created.
pub const LEGACY_V6_QUARANTINE_UNBOUND_V1_VOUCHER: u16 = 1 << 3;

pub const LEGACY_V6_RECOVERY_AUTH_RECORDED_AUTHORITY: u8 = 0;
pub const LEGACY_V6_RECOVERY_AUTH_PASSKEY_CONSENT: u8 = 1;

/// Re-authorizable owner consent for one exact legacy recovery manifest.
///
/// The row may be updated only while unconsumed, at the next passkey nonce and
/// strictly next generation. Recovery flips `consumed` permanently; the
/// attestation separately snapshots its digest and generation.
#[account]
#[derive(InitSpace)]
pub struct LegacyV6RecoveryConsent {
    pub version: u8,
    pub bump: u8,
    pub vault: Pubkey,
    pub historical_dexter_authority: Pubkey,
    pub replacement_dexter_authority: Pubkey,
    pub recovery_manifest_sha256: [u8; 32],
    pub consent_digest: [u8; 32],
    pub original_data_sha256: [u8; 32],
    pub recovery_salt_sha256: [u8; 32],
    /// SHA-256 of the exact compressed P-256 key that authorized this consent.
    pub passkey_pubkey_sha256: [u8; 32],
    pub generation: u64,
    pub passkey_nonce_before: u64,
    pub passkey_nonce_after: u64,
    pub authorized_at: i64,
    pub authorized_at_slot: u64,
    pub expires_at: i64,
    pub authorization_payer: Pubkey,
    pub consumed: bool,
    pub consumed_at: Option<i64>,
}

pub const LEGACY_V6_RECOVERY_CONSENT_VERSION_V1: u8 = 1;

/// Permanent neutral state for one immutable session attestation. It begins in
/// `NEUTRAL`; only an exact owner-passkey voucher binding may move it to a
/// terminal during grace, or the current owner passkey may release it after the
/// deadline. There is no close/reset path.
#[derive(
    AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, Default, PartialEq, Eq, InitSpace,
)]
pub struct LegacyV6SessionResolution {
    pub present: bool,
    pub session: Pubkey,
    /// Permanent global `(session_pubkey, channel_id)` tombstone selected by
    /// the owner, or default while the session remains neutral.
    pub binding: Pubkey,
    /// True only while an owner-bound terminal has exclusively reserved one
    /// unit of the historical undifferentiated pending counter.
    pub counter_reserved: bool,
    pub state: u8,
    pub remaining_outstanding: u64,
}

pub const LEGACY_V6_SESSION_RESOLUTION_NEUTRAL: u8 = 0;
pub const LEGACY_V6_SESSION_RESOLUTION_OWNER_BOUND: u8 = 1;
pub const LEGACY_V6_SESSION_RESOLUTION_SETTLED: u8 = 2;
pub const LEGACY_V6_SESSION_RESOLUTION_LOCKED: u8 = 3;
pub const LEGACY_V6_SESSION_RESOLUTION_OWNER_RELEASED: u8 = 4;

/// Mutable terminal tracker paired with the immutable recovery attestation.
/// It never infers a relationship between the Vault pending counter and a
/// session meter. Counter consumption is recorded only by an exact owner-bound
/// terminal or an exact post-deadline current-passkey release.
#[account]
#[derive(InitSpace)]
pub struct LegacyV6VoucherResolution {
    pub version: u8,
    pub bump: u8,
    pub vault: Pubkey,
    pub attestation: Pubkey,
    pub recovered_at: i64,
    pub settlement_deadline: i64,
    pub original_pending_count: u32,
    pub session_count: u16,
    pub sessions: [LegacyV6SessionResolution; crate::constants::LEGACY_V6_RECOVERY_MAX_SESSIONS],
    pub terminal_consumed_pending_count: u32,
    pub owner_released_pending_count: u32,
    pub last_owner_release_at: Option<i64>,
}

pub const LEGACY_V6_VOUCHER_RESOLUTION_VERSION_V1: u8 = 1;

/// Permanent owner-passkey authorization for one exact quarantined legacy V1
/// voucher and exactly one selected terminal. The PDA is never closed or
/// reused; `consumed` is a permanent replay tombstone.
#[account]
#[derive(InitSpace)]
pub struct LegacyV1TabBinding {
    pub version: u8,
    pub bump: u8,
    pub vault: Pubkey,
    pub session: Pubkey,
    pub attestation: Pubkey,
    pub voucher_resolution: Pubkey,
    pub original_data_sha256: [u8; 32],
    pub session_data_sha256: [u8; 32],
    pub session_pubkey: [u8; 32],
    pub channel_id: [u8; 32],
    pub voucher_digest: [u8; 32],
    pub cumulative_amount: u64,
    pub sequence_number: u32,
    pub allowed_counterparty: Pubkey,
    pub session_index: u8,
    pub remaining_outstanding: u64,
    pub terminal: u8,
    pub consume_pending_counter: bool,
    pub expected_pending_count: u32,
    pub expires_at: i64,
    pub authorized_at: i64,
    pub lock_holder: Pubkey,
    pub lock_maturity_at: Option<i64>,
    pub lock_holder_recovery_at: Option<i64>,
    pub consumed: bool,
    pub consumed_at: Option<i64>,
}

pub const LEGACY_V1_TAB_BINDING_VERSION_V1: u8 = 1;
pub const LEGACY_V1_TAB_TERMINAL_SETTLE: u8 = 1;
pub const LEGACY_V1_TAB_TERMINAL_LOCK: u8 = 2;

/// Reverse Swig→Vault binding layout version.
pub const SWIG_VAULT_BINDING_VERSION_V1: u8 = 1;
/// Binding created atomically with Dexter's canonical, program-defined Swig
/// authority spine. Historical/migrated V1 bindings prove identity only and
/// cannot open a seller reservation until separately policy-attested.
pub const SWIG_VAULT_BINDING_VERSION_ATOMIC_V2: u8 = 2;
/// Historical binding whose existing Swig authority spine was independently
/// verified on-chain and committed by the graph admin against an exact account
/// digest. This is reserve-safe without falsely claiming atomic-birth
/// provenance.
pub const SWIG_VAULT_BINDING_VERSION_ATTESTED_V3: u8 = 3;
/// Atomic mode-2 enrollment with a canonical Vault-owned recurring automation
/// budget. Direct agent spending must carry and debit that PDA; stock Buy uses
/// the same row, making the owner amount wallet-wide rather than role-local.
pub const SWIG_VAULT_BINDING_VERSION_AUTOMATION_V4: u8 = 4;
/// Top-level `activate_wallet` birth with the minimal 4-role spine (inert
/// root, finalize_withdrawal marker, provision_wallet_role manager, cold WARD).
/// Reserve-safe by construction: every later role enters only through a
/// Vault-gated manager marker.
pub const SWIG_VAULT_BINDING_VERSION_MINIMAL_V5: u8 = 5;

/// One-per-Swig reverse binding. PDA:
/// `[SWIG_VAULT_BINDING_SEED, swig]`.
///
/// This is deliberately additive: deployed Vault and LockedClaim layouts do
/// not change. A PDA may be initialized only once, so one Swig cannot acquire
/// two independent Vault reserve ledgers.
#[account]
#[derive(InitSpace)]
pub struct SwigVaultBinding {
    pub version: u8,
    pub bump: u8,
    pub swig: Pubkey,
    pub vault: Pubkey,
}

/// #77 tail-carve proof: every legacy (pre-seller-binding) mainnet claim
/// decodes under the extended struct with `allowed_counterparty == None` and
/// every reachable legacy mutation re-serializes inside the legacy 183-byte
/// allocation. Mainnet ground truth 2026-07-06: all 44 live claims are
/// exactly 191 bytes (8 + 183).
#[cfg(test)]
mod locked_claim_layout_tests {
    use super::*;

    /// The legacy account body size: 183 = LockedClaim::INIT_SPACE before the
    /// #77 tail carve (Option<Pubkey> added 33 to the derived max).
    const LEGACY_BODY_LEN: usize = 183;

    /// Byte-for-byte mirror of the pre-#77 LockedClaim field set. Serializing
    /// this reproduces exactly what the deployed program wrote into the 44
    /// live mainnet claims.
    #[derive(AnchorSerialize)]
    struct LegacyLockedClaimV1 {
        version: u8,
        bump: u8,
        vault: Pubkey,
        session_pubkey_at_lock: [u8; 32],
        voucher_hash: [u8; 32],
        amount: u64,
        created_at: i64,
        maturity_at: Option<i64>,
        holder_recovery_at: Option<i64>,
        current_holder: Pubkey,
        status: LockedClaimStatus,
        settled_at: Option<i64>,
        recovered_at: Option<i64>,
    }

    fn legacy(maturity_at: Option<i64>, holder_recovery_at: Option<i64>) -> LegacyLockedClaimV1 {
        LegacyLockedClaimV1 {
            version: LOCKED_CLAIM_VERSION_V1,
            bump: 254,
            vault: Pubkey::new_unique(),
            session_pubkey_at_lock: [7u8; 32],
            voucher_hash: [9u8; 32],
            amount: 123_456,
            created_at: 1_751_000_000,
            maturity_at,
            holder_recovery_at,
            current_holder: Pubkey::new_unique(),
            status: LockedClaimStatus::Pending,
            settled_at: None,
            recovered_at: None,
        }
    }

    /// Serialize any AnchorSerialize value to bytes (house pattern:
    /// migrate_legacy_to_v6::encode_v6_image).
    fn to_bytes<T: AnchorSerialize>(v: &T) -> Vec<u8> {
        let mut out = Vec::new();
        v.serialize(&mut out).unwrap();
        out
    }

    /// Serialize a legacy claim into a 183-byte legacy allocation (content +
    /// zero padding), exactly the account body the runtime holds today.
    fn legacy_body(claim: &LegacyLockedClaimV1) -> Vec<u8> {
        let mut body = to_bytes(claim);
        assert!(
            body.len() <= LEGACY_BODY_LEN,
            "legacy content {} exceeds legacy allocation {}",
            body.len(),
            LEGACY_BODY_LEN
        );
        body.resize(LEGACY_BODY_LEN, 0);
        body
    }

    #[test]
    fn new_init_space_is_legacy_plus_option_pubkey() {
        assert_eq!(LockedClaim::INIT_SPACE, LEGACY_BODY_LEN + 33);
    }

    /// Both live legacy shapes (all-None options at 151 content bytes; both
    /// lock-time options Some at 167) decode under the extended struct: the
    /// Option tag is read from guaranteed-zero padding → None; every legacy
    /// field survives byte-identical.
    #[test]
    fn legacy_claim_decodes_with_none_binding() {
        for (m, r) in [(None, None), (Some(1_751_100_000), Some(1_751_200_000))] {
            let legacy_claim = legacy(m, r);
            let body = legacy_body(&legacy_claim);
            let decoded = LockedClaim::deserialize(&mut body.as_slice()).unwrap();
            assert_eq!(decoded.allowed_counterparty, None, "tail must read None");
            assert_eq!(decoded.version, legacy_claim.version);
            assert_eq!(decoded.vault, legacy_claim.vault);
            assert_eq!(
                decoded.session_pubkey_at_lock,
                legacy_claim.session_pubkey_at_lock
            );
            assert_eq!(decoded.voucher_hash, legacy_claim.voucher_hash);
            assert_eq!(decoded.amount, legacy_claim.amount);
            assert_eq!(decoded.created_at, legacy_claim.created_at);
            assert_eq!(decoded.maturity_at, m);
            assert_eq!(decoded.holder_recovery_at, r);
            assert_eq!(decoded.current_holder, legacy_claim.current_holder);
            assert_eq!(decoded.status, LockedClaimStatus::Pending);
            assert_eq!(decoded.settled_at, None);
            assert_eq!(decoded.recovered_at, None);
        }
    }

    /// The worst reachable legacy write is 175 content bytes (both lock-time
    /// options Some + ONE terminal timestamp — settled_at and recovered_at are
    /// mutually exclusive), so ≥8 zero padding bytes ALWAYS follow legacy
    /// content: the None tag read above can never fall off the allocation.
    #[test]
    fn worst_reachable_legacy_content_leaves_padding() {
        let mut worst = legacy(Some(1), Some(2));
        worst.status = LockedClaimStatus::Abandoned;
        worst.recovered_at = Some(3);
        let content = to_bytes(&worst);
        assert_eq!(content.len(), 175);
        assert!(content.len() < LEGACY_BODY_LEN);
    }

    /// recover_abandoned_lock mutates a legacy claim in place and Account::exit
    /// re-serializes THE EXTENDED struct back into the legacy 183-byte
    /// allocation. The explicit None tail tag must still fit.
    #[test]
    fn legacy_mutation_reserializes_within_legacy_allocation() {
        let body = legacy_body(&legacy(Some(1_751_100_000), Some(1_751_200_000)));
        let mut decoded = LockedClaim::deserialize(&mut body.as_slice()).unwrap();
        decoded.status = LockedClaimStatus::Abandoned;
        decoded.recovered_at = Some(1_751_300_000);
        let rewritten = to_bytes(&decoded);
        assert!(
            rewritten.len() <= LEGACY_BODY_LEN,
            "abandon write-back {} must fit the legacy allocation {}",
            rewritten.len(),
            LEGACY_BODY_LEN
        );
        // Round-trips consistently after the write-back normalized the tail.
        let re = LockedClaim::deserialize(&mut rewritten.as_slice()).unwrap();
        assert_eq!(re.allowed_counterparty, None);
        assert_eq!(re.status, LockedClaimStatus::Abandoned);
    }

    /// New claims carry the binding and round-trip it.
    #[test]
    fn bound_claim_roundtrips_seller() {
        let seller = Pubkey::new_unique();
        let body = {
            let mut c =
                LockedClaim::deserialize(&mut legacy_body(&legacy(None, None)).as_slice()).unwrap();
            c.allowed_counterparty = Some(seller);
            to_bytes(&c)
        };
        let decoded = LockedClaim::deserialize(&mut body.as_slice()).unwrap();
        assert_eq!(decoded.allowed_counterparty, Some(seller));
    }
}

// ---------------------------------------------------------------------------
// Recourse-rooted delegation graph (depth-N) — the unified credit node.
// Replaces the retired depth-1 standby system (StandbyBacker / ReserveKind and
// the four Vault credit fields). A `PrincipalNode` is one identity in the
// delegation graph: authority flows DOWN (a controller grants its children a
// `RateCap`), recourse flows UP (a defaulting leaf's `shortfall` cascades to its
// ancestors). The PDA seed is the stable `node_id`.
// ---------------------------------------------------------------------------

/// PrincipalNode layout version. First version of the depth-N node.
pub const PRINCIPAL_NODE_VERSION_V1: u8 = 1;

/// A token-bucket rate limiter PLUS an optional hard ceiling for a node's draw
/// authority. `bucket` refills `rate_amount` every `period_secs`, capped at
/// `rate_amount * burst_multiple`; `last_refill` anchors the refill clock.
/// `ceiling` (when Some) is the absolute outstanding-draw cap independent of the
/// velocity bucket. Logic lands in later tasks (Task 5 grow, Task 6 draw); this
/// task only fixes the layout.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, InitSpace)]
pub struct RateCap {
    /// Tokens (draw units) added to the bucket each `period_secs`.
    pub rate_amount: u64,
    /// Refill period in seconds.
    pub period_secs: u32,
    /// Current bucket level (available draw velocity).
    pub bucket: u64,
    /// Unix time the bucket was last refilled.
    pub last_refill: i64,
    /// Absolute outstanding-draw ceiling, independent of the velocity bucket.
    /// None ⇒ velocity-only (no hard ceiling at this node).
    pub ceiling: Option<u64>,
    /// Burst cap: bucket may accumulate up to `rate_amount * burst_multiple`.
    pub burst_multiple: u8,
}

/// One identity in the recourse-rooted delegation graph. Authority flows DOWN
/// (a controller grants children a `RateCap`); recourse flows UP (a defaulting
/// leaf's `shortfall` cascades to ancestors). PDA: seeds =
/// `[PRINCIPAL_NODE_SEED, node_id]` — the `node_id` is the stable identity, so a
/// node's address survives re-rooting (emancipation). `init` (NOT
/// init_if_needed): existence is the uniqueness guard per node_id.
#[account]
#[derive(InitSpace)]
pub struct PrincipalNode {
    /// Layout version. First field per house style.
    pub version: u8,
    /// Canonical bump for this PDA.
    pub bump: u8,
    /// Stable identity + PDA seed component. Survives re-rooting.
    pub node_id: [u8; 32],
    /// The key that grants downward (creates children, sets their caps) and may
    /// freeze this subtree.
    pub controller: Pubkey,
    /// Parent node PDA. None ⇒ this is a root candidate (no upward recourse).
    pub parent: Option<Pubkey>,
    /// The attestation that roots this node (a `CreditRoot` PDA today; a future
    /// attestation type later). None ⇒ not yet rooted.
    pub root_attestation: Option<Pubkey>,
    /// This node's draw authority — velocity bucket + optional hard ceiling.
    pub cap: RateCap,
    /// This node's OWN outstanding draw (the leaf accumulator). Rises on draw,
    /// falls on repay/seize.
    pub borrowed: u64,
    /// Rolled-up subtree draw. Authoritative only at a ceiling-root; advisory
    /// elsewhere. Maintained by the draw/repay traversal (Tasks 6/7).
    pub subtree_draw: u64,
    /// Seize deadline armed on first draw, cleared when `borrowed` returns to 0.
    /// None ⇒ nothing borrowed / no deadline armed.
    pub borrow_recovery_at: Option<i64>,
    /// RUNG-3: uncovered remainder after a partial self-seize, awaiting the
    /// ancestor cascade. 0 when no default is in progress. Lives on the DEFAULTED
    /// leaf node; the cascade (Task 8) reads + reduces it.
    pub shortfall: u64,
    /// Frozen subtree flag — set by the controller (or an ancestor) to halt
    /// exposure-increasing verbs at/under this node.
    pub frozen: bool,
    /// Number of child nodes whose `parent` points here.
    pub child_count: u32,
    /// Accrued fee accumulator. Accrual fields are present now; the accrual logic
    /// lands later (rate_bps starts at 0 = no accrual).
    pub accrued_fee: u64,
    /// Fee rate in basis points. 0 ⇒ no accrual (the layout-task default).
    pub rate_bps: u16,
    /// Unix time of the last fee accrual tick.
    pub last_accrual: i64,
    /// The CAPITAL source (a swig) that funds this node's draws and is made whole
    /// on repay/seize/cascade. ROOT-DETERMINED: inherited from `parent.financier`
    /// at creation (a root-less / operator / emancipated root sets it explicitly),
    /// so a whole tree shares ONE financier. `draw_credit` binds its
    /// `financier_swig` to this; `repay`/`seize`/`seize_ancestor` bind their payout
    /// DESTINATION to this financier's ATA — making "financier-made-whole" a
    /// program invariant rather than operator trust. (Vault-of-Record = emancipate
    /// to your own root and set your own financier; descendants inherit it.)
    pub financier: Pubkey,
}

// ---------------------------------------------------------------------------
// Immutable funded-Line origination receipt (P1 foundation).
// ---------------------------------------------------------------------------

/// LineOrigination layout version. V1 is an immutable origination receipt, not
/// a transferable claim account and not an outstanding-principal subledger.
pub const LINE_ORIGINATION_VERSION_V1: u8 = 1;

/// Immutable evidence that a specific funded Line utilization was originated.
///
/// PDA: `[LINE_ORIGINATION_SEED, exposure_id]`. There is deliberately no
/// mutation, transfer, settle, or close instruction in V1. The existing
/// PrincipalNode remains the non-transferable facility and aggregate solvency
/// ledger; this receipt supplies stable per-draw identity and originated terms
/// without enlarging or reinterpreting deployed PrincipalNode accounts.
#[account]
#[derive(InitSpace)]
pub struct LineOrigination {
    /// Layout version. First field per house style.
    pub version: u8,
    /// Canonical bump for this PDA.
    pub bump: u8,
    /// Stable exposure identity:
    /// sha256(domain || vault || node || evidence_hash).
    pub exposure_id: [u8; 32],
    /// Buyer vault whose Line funded the utilization.
    pub vault: Pubkey,
    /// Opaque company/buyer identity snapshot from `Vault.identity_claim`.
    pub buyer_identity_claim: [u8; 32],
    /// Non-transferable PrincipalNode / Record governing the draw.
    pub node: Pubkey,
    /// Stable node identity snapshot (survives graph re-rooting).
    pub node_id: [u8; 32],
    /// Root/Record attestation in force at origination, if the node carried one
    /// directly. Delegate-chain evidence remains committed by `evidence_hash`.
    pub root_attestation: Option<Pubkey>,
    /// Dexter/session orchestration authority that authorized the on-chain draw.
    pub draw_authority: Pubkey,
    /// Capital source and initial economic owner of the funded exposure.
    pub financier: Pubkey,
    /// Seller wallet whose canonical USDC ATA received the funding leg.
    pub seller: Pubkey,
    /// Exact seller USDC ATA bound by the adjacent draw and SignV2.
    pub seller_destination: Pubkey,
    /// Principal funded to the seller.
    pub principal: u64,
    /// On-chain clock at receipt creation.
    pub originated_at: i64,
    /// Contractual due time supplied at origination.
    pub due_at: i64,
    /// Effective cure/grace interval from `due_at` to `default_at`.
    pub cure_period_seconds: i64,
    /// Effective default/recovery time. For a later draw this is the facility's
    /// existing first-draw deadline, never `now + requested_window`.
    pub default_at: i64,
    /// Exact recovery window carried by the adjacent legacy draw instruction.
    pub requested_recovery_window_seconds: i64,
    /// Actual seconds from this origination to `default_at`.
    pub effective_recovery_window_seconds: i64,
    /// PrincipalNode rate snapshot at origination.
    pub rate_bps: u16,
    /// Canonical commitment to buyer/company/Record, agent/session, seller,
    /// service, voucher, delivery, and source-transaction evidence.
    pub evidence_hash: [u8; 32],
    /// Canonical commitment to legal, underwriting, policy, and pricing terms.
    pub terms_hash: [u8; 32],
}

// ---------------------------------------------------------------------------
// Personhood-rooted credit identity (Phase 1 — Flow B)
// ---------------------------------------------------------------------------

/// How the current World ID merkle root was posted into `WorldIdRootCache`.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, InitSpace)]
pub enum RootSource {
    /// Phase 1: written by the admin-gated post_interim_root. NOT trustless.
    Interim,
    /// Phase 2: written by establish_root from a verified SP1 Helios proof.
    Sp1Helios,
}

/// Singleton cache of the current World ID merkle root on Solana.
/// PDA: seeds = [WORLD_ID_ROOT_SEED]. Read by establish_credit_root.
#[account]
#[derive(InitSpace)]
pub struct WorldIdRootCache {
    /// Layout version. First field per house style.
    pub version: u8,
    /// Canonical bump for this singleton PDA.
    pub bump: u8,
    /// The World ID v4 registry merkle root (bn254 field element, big-endian).
    pub root: [u8; 32],
    /// Source Ethereum block number the root was read at (advisory in Phase 1).
    pub block_number: u64,
    /// Unix time the root was last written. Freshness is enforced in Phase 2.
    pub posted_at: i64,
    /// Provenance of the root — Interim (admin-gated) or Sp1Helios (trustless).
    pub source: RootSource,
}

/// Number of historical World ID roots retained on-chain (the settlement ring).
/// A registered root need only be attested ONCE by an `establish_root` proof to
/// live here forever after — a v2 CreditRoot minted against it can then converge
/// to trustless (v1) at any later time.
///
/// NO-EVICTION-BEFORE-DRAIN INVARIANT (why 64 is safe at any scale): the keeper
/// attests at most the settlement queue's requested batch per cycle, and the host
/// caps that batch at MAX_PENDING_ROOTS = 32 (< this LEN). The queue drains each
/// batch to v1 on its very next tick — before the keeper's next (6h) attestation.
/// So the ring never holds more than one un-drained batch (≤32) of un-upgraded
/// roots; a wrap only ever evicts roots that ALREADY settled. Doubly safe: an
/// evicted-but-still-v2 root is re-requested from the capture ledger every queue
/// tick, so even a broken invariant self-heals (re-attest + retry) rather than
/// stranding. Keep MAX_PENDING_ROOTS < LEN if either constant changes.
pub const WORLD_ID_ROOT_HISTORY_LEN: usize = 64;

/// Singleton ring of SP1-Helios-attested World ID registry-HISTORY roots (seed
/// `[WORLD_ID_ROOT_HISTORY_SEED]`). PERFECT-2: `establish_root` inserts each root
/// the guest proved present in `rootTimestamps` (slot 14, nonzero) at the
/// fault-proof-anchored L2 state; `upgrade_credit_root` / `establish_credit_root`
/// accept a proof whose merkle_root matches the live cache OR ANY root in this
/// ring. Provenance is structural: only the SP1-verified `attestedRoots` output
/// can write here, so membership == "this root was in World Chain's finalized
/// registry history", the exact fact that makes a late upgrade trustless.
///
/// ZERO-COPY (`AccountLoader`): the ~2KB ring is memory-mapped, never borsh-
/// deserialized onto the ~4KB BPF stack — the groth16 π₂ verify in the credit
/// handlers already fills the frame, so a stack-resident copy overflows. `repr(C)`
/// layout is padding-free (u16s land on even offsets; the align-1 `roots` array
/// follows), so it is `Pod` for bytemuck. Created once by `init_root_history`.
#[account(zero_copy)]
#[repr(C)]
pub struct WorldIdRootHistory {
    /// Layout version. First field per house style.
    pub version: u8,
    /// Canonical bump for this singleton PDA.
    pub bump: u8,
    /// Next write index into `roots` (wraps at WORLD_ID_ROOT_HISTORY_LEN).
    pub head: u16,
    /// Count of live entries (saturates at WORLD_ID_ROOT_HISTORY_LEN once full).
    pub count: u16,
    /// The ring of attested roots (bn254 field elements, big-endian). Unused
    /// slots (index >= count while not yet wrapped) are zero and never match.
    pub roots: [[u8; 32]; WORLD_ID_ROOT_HISTORY_LEN],
}

impl WorldIdRootHistory {
    /// Serialized size (no anchor discriminator): 1+1+2+2 + 32*64 = 2054, and
    /// `repr(C)` adds no padding (verified by `size_of` in a unit test).
    pub const SIZE: usize = 1 + 1 + 2 + 2 + 32 * WORLD_ID_ROOT_HISTORY_LEN;

    /// True if `root` is a live entry. Zero never matches (unwritten slots are
    /// zero; a real World ID root is never the zero field element).
    pub fn contains(&self, root: &[u8; 32]) -> bool {
        if *root == [0u8; 32] {
            return false;
        }
        let live = self.count as usize;
        self.roots.iter().take(live).any(|r| r == root)
    }

    /// Insert `root` if absent (idempotent). Overwrites the oldest slot on wrap.
    pub fn insert(&mut self, root: [u8; 32]) {
        if root == [0u8; 32] || self.contains(&root) {
            return;
        }
        let idx = self.head as usize % WORLD_ID_ROOT_HISTORY_LEN;
        self.roots[idx] = root;
        self.head = ((idx + 1) % WORLD_ID_ROOT_HISTORY_LEN) as u16;
        if (self.count as usize) < WORLD_ID_ROOT_HISTORY_LEN {
            self.count += 1;
        }
    }
}

// The zero-copy account space (`8 + SIZE`) MUST equal the real repr(C) size, or
// the ring silently truncates on load. `repr(C)` here is padding-free; this
// fails the build if that ever changes.
const _: () = assert!(core::mem::size_of::<WorldIdRootHistory>() == WorldIdRootHistory::SIZE);

#[cfg(test)]
mod root_history_tests {
    use super::*;

    fn empty() -> WorldIdRootHistory {
        WorldIdRootHistory {
            version: 1,
            bump: 0,
            head: 0,
            count: 0,
            roots: [[0u8; 32]; WORLD_ID_ROOT_HISTORY_LEN],
        }
    }
    fn r(n: u8) -> [u8; 32] {
        let mut x = [0u8; 32];
        x[0] = n;
        x[31] = n;
        x
    }

    #[test]
    fn insert_then_contains() {
        let mut h = empty();
        assert!(!h.contains(&r(1)), "empty ring contains nothing");
        h.insert(r(1));
        h.insert(r(2));
        assert!(h.contains(&r(1)));
        assert!(h.contains(&r(2)));
        assert!(!h.contains(&r(3)));
        assert_eq!(h.count, 2);
    }

    #[test]
    fn zero_root_never_stored_or_matched() {
        let mut h = empty();
        h.insert([0u8; 32]);
        assert_eq!(h.count, 0, "zero root is never inserted");
        assert!(
            !h.contains(&[0u8; 32]),
            "zero never matches, even in an empty ring"
        );
    }

    #[test]
    fn insert_is_idempotent() {
        let mut h = empty();
        h.insert(r(7));
        h.insert(r(7));
        h.insert(r(7));
        assert_eq!(h.count, 1, "duplicate inserts do not grow the ring");
    }

    #[test]
    fn wraps_and_evicts_oldest() {
        let mut h = empty();
        // Fill exactly to capacity with distinct roots 1..=LEN.
        for i in 1..=WORLD_ID_ROOT_HISTORY_LEN {
            h.insert(r(i as u8));
        }
        assert_eq!(h.count as usize, WORLD_ID_ROOT_HISTORY_LEN);
        assert!(h.contains(&r(1)), "oldest still present at capacity");
        // One more wraps and evicts the oldest (r(1)).
        h.insert(r(200));
        assert_eq!(
            h.count as usize, WORLD_ID_ROOT_HISTORY_LEN,
            "count saturates"
        );
        assert!(!h.contains(&r(1)), "oldest evicted on wrap");
        assert!(h.contains(&r(200)), "newest present");
        assert!(h.contains(&r(2)), "second-oldest still present");
    }
}

/// SP1-Helios light-client continuity state (singleton, seed
/// `[LIGHT_CLIENT_STATE_SEED]`). Seeded ONCE from a trusted checkpoint by
/// `seed_light_client_state`; every `establish_root` proves FORWARD from it and
/// advances it. This is the on-chain half of the trustless World ID root: the
/// SP1 guest pins the genesis validators root (the BLS domain), and THIS account
/// pins the sync committee + head continuity — together they stop a prover from
/// supplying its own committee to sign a forged execution state root.
#[account]
#[derive(InitSpace)]
pub struct LightClientState {
    /// Layout version. First field per house style.
    pub version: u8,
    /// Canonical bump for this singleton PDA.
    pub bump: u8,
    /// Latest trusted finalized beacon slot.
    pub head: u64,
    /// Beacon block header tree-hash root at `head`.
    pub header: [u8; 32],
    /// Sync committee hash for the period containing `head` — the committee the
    /// NEXT proof's `startSyncCommitteeHash` must equal.
    pub sync_committee_hash: [u8; 32],
    /// Sync committee hash for the following period (zero until a proof commits it).
    pub next_sync_committee_hash: [u8; 32],
}

/// One-human-one-root. PDA: seeds = [CREDIT_ROOT_SEED, nullifier]. `init`
/// (NOT init_if_needed) → existence IS the uniqueness guard. PERMANENT: there
/// is no close instruction; this is the human's durable credit identity.
#[account]
#[derive(InitSpace)]
pub struct CreditRoot {
    /// Layout version. First field per house style.
    pub version: u8,
    /// Canonical bump for this PDA.
    pub bump: u8,
    /// The verified World ID nullifier N (the seed). Deterministic per
    /// (human, DEXTER_RP_ID, DEXTER_ACTION).
    pub nullifier: [u8; 32],
    /// Unix time this CreditRoot was established on-chain.
    pub established_at: i64,
    /// Running count of credit events recorded against this human (event seq).
    pub event_count: u64,
}

/// A single credit event keyed to the HUMAN (nullifier), not a vault.
/// PDA: seeds = [CREDIT_EVENT_SEED, nullifier, &seq.to_le_bytes()].
#[account]
#[derive(InitSpace)]
pub struct CreditEvent {
    /// Layout version. First field per house style.
    pub version: u8,
    /// Canonical bump for this PDA.
    pub bump: u8,
    /// The human nullifier this event is keyed to (mirrors the CreditRoot seed).
    pub nullifier: [u8; 32],
    /// Monotonic sequence number within the human's credit history.
    pub seq: u64,
    /// The per-agent vault this event concerns (advisory; credit follows N).
    pub vault: Pubkey,
    /// Unix time this event was recorded on-chain.
    pub recorded_at: i64,
    /// Opaque event payload kind (extended by the credit product).
    pub kind: u8,
    /// USDC amount (or product-defined unit) associated with this event.
    pub amount: u64,
}

/// GraphConfig layout version.
pub const GRAPH_CONFIG_VERSION_V1: u8 = 1;
/// V2: + withdrawal_fee_atomic + fee_treasury + reserved tail
/// (migrate_graph_config_v2 is the only V1→V2 path).
pub const GRAPH_CONFIG_VERSION_V2: u8 = 2;

/// Admin spine for the recourse-rooted delegation graph. Singleton PDA:
/// seeds = [GRAPH_CONFIG_SEED]. `init` (NOT init_if_needed) → existence IS the
/// uniqueness guard; only INTERIM_ROOT_AUTHORITY may create it. Holds the
/// runtime tunables read by later tasks (pause flags, max-depth override).
#[account]
#[derive(InitSpace)]
pub struct GraphConfig {
    /// Layout version. First field per house style.
    pub version: u8,
    /// Canonical bump for this singleton PDA.
    pub bump: u8,
    /// Cold key: may pause, unpause, and change tunables.
    pub admin_authority: Pubkey,
    /// Hot key: may pause ONLY (never unpause, never tune).
    pub pause_authority: Pubkey,
    /// Global pause flag. Gates exposure-increasing verbs only (enforced later).
    pub paused: bool,
    /// Unix time of the last pause (0 if never paused).
    pub paused_at: i64,
    /// Opaque reason code for the last pause.
    pub pause_reason: u8,
    /// Runtime max-depth override. 0 ⇒ use the MAX_DEPTH const.
    pub max_depth_override: u8,
    /// Canonical credit-settlement mint for this deployment (mainnet = USDC).
    /// The recourse-out bind (`require_financier_destination`) derives the
    /// financier's expected ATA from THIS mint, so seized/repaid funds can only
    /// land in the financier's ATA on this asset. A deploy-time parameter set
    /// ONCE at init — not a buried const — so the bind is provable on any cluster
    /// and the credit asset is explicit. Mainnet pins it to USDC_MAINNET.
    pub usdc_mint: Pubkey,
    /// Flat withdrawal fee in atomic units of `usdc_mint`, enforced on-chain by
    /// finalize_withdrawal's two-leg money bind (leg 2 must move EXACTLY this).
    /// Bounded by WITHDRAWAL_FEE_CAP so a compromised cold key cannot turn the
    /// fee knob into a skim. V2 field (migrate_graph_config_v2).
    pub withdrawal_fee_atomic: u64,
    /// Wallet that receives the fee leg. finalize_withdrawal derives this
    /// wallet's canonical `usdc_mint` ATA and pins leg 2's destination to it.
    /// V2 field.
    pub fee_treasury: Pubkey,
    /// Spread engine (2026-07-04): the protocol's share of COLLECTED interest,
    /// in bps, routed to `fee_treasury`'s ATA as a second settlement leg by
    /// repay/seize/settle_liquidation; the remainder is bound to the financier's
    /// ATA. Capped by INTEREST_TAKE_CAP_BPS (never more than half). Carved from
    /// the head of the V2 `reserved` tail — live accounts read their zeroed
    /// bytes as 0 (= all interest to the financier), so NO migration and NO
    /// version bump: this is exactly the byte-compatible reinterpretation the
    /// reserved tail exists for. Tuned by `set_interest_take` (cold admin).
    pub interest_take_bps: u16,
    /// Penny lock (crystallization-cadence spec 2026-07-06 §2): the operator's
    /// INTENT knob — the maximum dollars of un-crystallized tail a seller can
    /// have at risk to buyer abandonment, in atomic USDC units. The facilitator
    /// reads this and derives the actual lock-firing threshold from live gas
    /// (`effectiveThreshold = max(knob, gasFloor)`); the program does NOT
    /// enforce lock timing. ZERO semantics: readers treat 0 as
    /// MAX_SELLER_AT_RISK_DEFAULT (10_000 = $0.01 — the 2026-07-06 penny
    /// ruling), exactly the `max_depth_override` 0-means-default precedent —
    /// so live accounts whose zeroed `reserved` bytes now spell this field are
    /// at the penny the moment the program deploys: no admin action, no
    /// migration, no version bump (the `interest_take_bps` carve precedent).
    /// Bounded by MAX_SELLER_AT_RISK_CAP at `set_max_seller_at_risk`: a
    /// compromised cold key can loosen seller protection to at most $1.00,
    /// never disable it.
    /// NOTE (deploy-train 2026-07-25): the tab-fee-75 deploy shipped this byte
    /// position as an inert layout placeholder; this branch registers
    /// `set_max_seller_at_risk` for real, so the knob is now LIVE — deployed
    /// accounts read their zeroed bytes as the penny default with no migration.
    pub max_seller_at_risk_atomic: u64,
    /// Tab platform fee (board #75, 2026-07-24): the wallet whose canonical
    /// `usdc_mint` ATA receives the fee leg of a TWO-leg tab settle. ZERO
    /// semantics: default Pubkey = fee split DISABLED on-chain — two-leg
    /// settle envelopes are rejected until `set_tab_fee_policy` names a
    /// treasury. Carved from the `reserved` tail (the `interest_take_bps`
    /// precedent): live accounts read their zeroed bytes as disabled, so NO
    /// migration and NO version bump.
    pub tab_fee_treasury: Pubkey,
    /// Optional on-chain ceiling on the tab-fee leg, in basis points of the
    /// settle increment. 0 = NO CAP (Branch's 2026-07-24 ruling: ship the
    /// brake installed but OFF; arming it later is a `set_tab_fee_policy`
    /// call, not a redeploy). >0 → `settle_tab_voucher` requires
    /// `fee <= increment × bps / 10_000` (TabFeeExceedsCap) — the seller
    /// protection against a compromised facilitator over-allocating the
    /// split toward the treasury.
    pub tab_fee_max_bps: u16,
    /// Wallet setup fee (J-P4, 2026-10-18): the most `activate_wallet` lets a
    /// birth move from the new wallet's first deposit to `fee_treasury`'s
    /// canonical `usdc_mint` ATA, in atomic units. Carved from the `reserved`
    /// tail (the `interest_take_bps` precedent): live accounts read their
    /// zeroed bytes as 0 = no fee, so no migration and no version bump. Tuned
    /// by `set_activation_fee` (cold admin), bounded by ACTIVATION_FEE_CAP.
    pub activation_fee_atomic: u64,
    /// Reserved for future tunables — the V1→V2 realloc taught us to leave room.
    /// (64 at the V2 realloc; interest_take_bps carved 2 bytes off the head,
    /// max_seller_at_risk_atomic 8, tab_fee_treasury 32, tab_fee_max_bps 2,
    /// activation_fee_atomic 8.)
    pub reserved: [u8; 12],
}

/// AssetConfig layout version.
pub const ASSET_CONFIG_VERSION_V1: u8 = 1;

/// Per-asset row of the leveraged-carry engine's approved-base-asset allowlist
/// (Phase 0 of BUILD-SPEC-leveraged-carry-engine-2026-07-02). Keyed PDA:
/// seeds = [ASSET_CONFIG_SEED, base_mint]. `init` (NOT init_if_needed) →
/// existence per mint IS the uniqueness guard. Created and tuned by
/// `GraphConfig.admin_authority` (listings and risk params are runtime admin
/// ops, unlike the init-time-only GraphConfig tunables — Aave's CAPO incident
/// is the argument for tunable-without-redeploy).
///
/// Every future reader MUST bind the row to the mint actually being valued
/// with the full seeds constraint `[ASSET_CONFIG_SEED, base_mint]` — a config
/// for mint A must never gate a position in mint B (the singleton shortcut
/// used for GraphConfig is NOT safe here).
#[account]
#[derive(InitSpace)]
pub struct AssetConfig {
    /// Layout version. First field per house style.
    pub version: u8,
    /// Canonical bump for this PDA.
    pub bump: u8,
    /// The base asset this row approves for levered positions.
    pub base_mint: Pubkey,
    /// Listing kill switch. Disabled rows must refuse NEW leverage but still
    /// serve valuation for closes/liquidations of existing positions.
    pub enabled: bool,
    /// VALUATION_MODE_MARKET (0) or VALUATION_MODE_REDEMPTION_RATE (1) — see
    /// verify/oracle.rs for what each mode gates.
    pub valuation_mode: u8,
    /// Pinned Pyth feed id of the valuation feed (market price in mode 0, the
    /// redemption rate in mode 1).
    pub primary_feed_id: [u8; 32],
    /// Pinned Pyth feed id of the live market reference (mode 1 only; zeroed
    /// in mode 0). The divergence breaker runs RR against this.
    pub market_feed_id: [u8; 32],
    /// Staleness bound (secs) applied to every feed read for this asset. For
    /// tokenized equities this doubles as the market-hours breaker — do not
    /// widen it to "fix" weekend staleness.
    pub max_age_secs: u32,
    /// Confidence band, bps of price. Bites on market feeds; structurally a
    /// no-op on RR feeds (conf ≈ 0) — see verify/oracle.rs module doc.
    pub max_conf_bps: u16,
    /// RR↔market divergence band, bps (mode 1; Kamino Scope runs 30–100).
    pub max_divergence_bps: u16,
    /// CAPO-style upside growth cap, bps per 365d year (mode 1).
    pub max_growth_bps_per_year: u32,
    /// Growth-cap anchor: last admin-attested redemption rate (mode 1).
    pub rr_snapshot_price: i64,
    /// Exponent of the snapshot (must match the live feed's exponent).
    pub rr_snapshot_expo: i32,
    /// Unix time the snapshot was taken.
    pub rr_snapshot_at: i64,
    /// Leverage ceiling for the dial, bps (50_000 = 5x). u32: 10x T-bill-class
    /// caps exceed u16.
    pub max_leverage_bps: u32,
    /// Liquidation threshold, bps of LTV. Position at exactly this threshold
    /// is still solvent (verify/solvency.rs boundary semantic).
    pub liq_threshold_bps: u16,
    /// Borrow-spike gate: refuse NEW leverage when the position's borrow rate
    /// exceeds this. The firewall does not cover the borrow-rate regime.
    pub max_borrow_rate_bps: u16,
    /// Carry floor: refuse NEW leverage when net_carry_bps(Y, B, L) at the
    /// requested dial falls below this.
    pub min_net_carry_bps: i16,
}

/// F (credit consent) — FinancierConfig layout version.
pub const FINANCIER_CONFIG_VERSION_V1: u8 = 1;

/// Per-financier consent record. ITS EXISTENCE + the `authority` it names are
/// what let a root-less node name this financier as its capital source. Created
/// once by `establish_financier_config` (graph-admin-gated). PDA:
/// `[FINANCIER_CONFIG_SEED, financier_swig]`.
///
/// Root cause it closes (audit F, CONFIRMED exploitable): a root-less node's
/// `financier` was caller-set and never checked against the financier's own
/// consent, so anyone could name Dexter's lender Swig and drain it. Now a
/// root-less `create_node` must be signed by the key THIS record names, and a
/// draw requires this record to exist for the drawing node's financier.
#[account]
#[derive(InitSpace)]
pub struct FinancierConfig {
    /// Layout version. First field per house style.
    pub version: u8,
    /// Canonical bump for this PDA.
    pub bump: u8,
    /// The key allowed to authorize root-less nodes to draw on this financier.
    pub authority: Pubkey,
    /// The financier swig this record consents for (also the PDA seed).
    pub financier: Pubkey,
}

/// LeveredPosition layout version (BUILD-SPEC-leveraged-carry-engine §1.3).
pub const LEVERED_POSITION_VERSION_V1: u8 = 1;

/// One levered-carry position, 1:1 with its vault/node. Lean by design: every
/// risk parameter (oracle, valuation mode, liq threshold, leverage ceiling)
/// lives on `AssetConfig[base_mint]`, so this row carries only what is unique to
/// the POSITION — which asset, the intended leverage at open, and when. The
/// enforced money mechanics stay on `PrincipalNode` (`borrowed`, `cap.ceiling`,
/// `financier`); this account is the position's identity + base-asset binding.
/// PDA: `[LEVERED_POSITION_SEED, vault]` — existence per vault IS the
/// one-position-per-vault uniqueness guard (plain `init`, never init_if_needed).
#[account]
#[derive(InitSpace)]
pub struct LeveredPosition {
    /// Layout version. First field per house style.
    pub version: u8,
    /// Canonical bump for this PDA.
    pub bump: u8,
    /// The vault/node this position is 1:1 with (the isolation unit).
    pub vault: Pubkey,
    /// The base asset held in the user's swig. Binds this position to exactly
    /// one `AssetConfig[base_mint]` row for valuation + liquidation.
    pub base_mint: Pubkey,
    /// Intended leverage at open, bps (50_000 = 5x). Recorded here; bounded at
    /// open by `AssetConfig.max_leverage_bps`. The ENFORCED cap on drawn USDC is
    /// `PrincipalNode.cap.ceiling` — binding the drawn amount to live
    /// equity×leverage is a slice-2 hardening (needs a live equity read).
    pub leverage_bps: u32,
    /// Unix time the position was opened.
    pub opened_at: i64,
    /// Liquidation bracket (BUILD-SPEC §2.2, attest→settle). `attest_liquidation`
    /// snapshots the oracle-priced position value here AFTER proving the position
    /// underwater but BEFORE the unwind swap sells the base (the only moment the
    /// base is readable). `settle_liquidation` — running after the swap, when the
    /// base is gone — consumes it: a fresh, non-zero snapshot is the proof the
    /// seizure was justified at a validated price. Zero = no pending attestation.
    /// This is the user's protection: the seizable ceiling is locked in at a
    /// conservative price before any swap can distort it.
    pub pending_liq_value: u64,
    /// Unix time the pending snapshot was taken. `settle_liquidation` requires it
    /// fresh (bounded by the asset's own oracle staleness knob) so an attestation
    /// from a transient depeg cannot be replayed against a recovered price.
    pub pending_liq_at: i64,
}

#[error_code]
pub enum VaultError {
    #[msg("Cooling-off period has not elapsed")]
    CoolingOffNotElapsed,
    #[msg("Pending vouchers must settle before withdrawal can finalize")]
    PendingVouchersExist,
    #[msg("No pending withdrawal request")]
    NoPendingWithdrawal,
    #[msg("Passkey signature verification failed")]
    PasskeyVerificationFailed,
    #[msg("Voucher signature does not match Dexter session key")]
    InvalidVoucherSignature,
    #[msg("force_release grace period has not elapsed")]
    ForceReleaseTooEarly,
    #[msg("No stuck voucher to force-release")]
    NothingToRelease,
    #[msg("Vault account version is not supported by this program")]
    UnsupportedVaultVersion,
    #[msg("A session is already active on this vault and has not expired")]
    SessionAlreadyActive,
    #[msg("Session expiry must be in the future")]
    SessionExpiryInPast,
    #[msg("Session max_amount must be greater than zero")]
    SessionCapZero,
    #[msg("No active session to revoke")]
    NoActiveSession,
    #[msg("Revocation message session pubkey does not match the active session")]
    SessionPubkeyMismatch,
    #[msg("Opening this tab would exceed the session's revolving capacity")]
    RevolvingCapacityExceeded,
    #[msg("max_revolving_capacity must be greater than zero")]
    RevolvingCapacityZero,
    #[msg("Voucher's cumulative_amount does not advance the XOR frontier (already covered by spent or crystallized_cumulative)")]
    LockRangeAlreadyClaimed,
    #[msg("Locking this voucher would push outstanding_locked_amount above vault USDC balance")]
    LockWouldOvercommitVault,
    #[msg("Finalizing this withdrawal would bring vault balance below outstanding_locked_amount")]
    WithdrawalWouldViolateReservation,
    #[msg("Registering this session would push max_amount + outstanding_locked_amount above vault USDC balance + available standby credit")]
    SessionWouldOvercommitVault,
    #[msg("remaining_accounts not in strict ascending pubkey order (dedup/order)")]
    SessionAccountsNotSorted,
    #[msg("a passed session account belongs to a different vault")]
    SessionAccountForeign,
    #[msg("a passed account is not the canonical session PDA for its counterparty")]
    SessionAccountMisderived,
    #[msg("live sibling sessions passed != live_session_count (incomplete set)")]
    IncompleteSessionSet,
    #[msg("vault session fleet is at its bounded maximum; revoke one before registering")]
    SessionCountAtMax,
    #[msg("holder_recovery_at must be strictly greater than maturity_at when both are set")]
    RecoveryBeforeMaturity,
    // Task 2 layout rewrite: the depth-1 standby credit error family
    // (StandbyCapZero, CreditWouldExceedStandbyCap, NoStandbyBacker,
    // StandbyWouldExceedReserve, ReserveBelowPromised, ResizeBelowBorrowed,
    // StandbyStillBorrowed, NoStandbyBackerLedger, StandbyBackerMismatch,
    // FinancierConsentMissing) was REMOVED — each had zero remaining referents
    // after the six credit instructions were stubbed onto the graph. KEPT below:
    // - WithdrawalWouldViolatePin — re-pins against vault.node in Task 6.
    // - BorrowRecoveryTooEarly    — the seize deadline gate returns in Task 7.
    // - NothingBorrowed           — repay/seize re-raise it in Task 7.
    #[msg("Withdrawal would violate the credit pin (borrowed amount is reserved).")]
    WithdrawalWouldViolatePin,
    #[msg("Borrow recovery deadline has not passed yet.")]
    BorrowRecoveryTooEarly,
    #[msg("Nothing is borrowed on this vault.")]
    NothingBorrowed,
    #[msg("an expired sibling session must be passed WRITABLE so the register-time sweep can clear it")]
    SessionAccountNotWritable,
    #[msg("session is still live (version != 0); revoke_session_key it before close_session can reclaim its rent")]
    SessionStillLive,
    #[msg("account is not a recognized pre-version-byte legacy vault (expected exactly 151-byte V1 or 183-byte V1.5 with the Vault discriminator)")]
    NotALegacyVault,
    #[msg("claim is still Pending (a live obligation); only Settled or Abandoned claims can be closed for rent reclaim")]
    ClaimNotTerminal,
    #[msg("World ID v4 pi2 (OPRFNullifier) groth16 proof failed to verify against the production verifying key")]
    WorldIdVerificationFailed,
    #[msg("Proof merkle_root does not match the cached World ID root")]
    WorldIdRootMismatch,
    #[msg("Proof rp_id/action does not match the Dexter context")]
    WorldIdContextMismatch,
    #[msg("Proof credential/network signals (issuer_schema_id/cred_pk/depth/oprf_pk) do not match the pinned World ID values")]
    WorldIdCredentialMismatch,
    #[msg("Trustless credit-root establishment requires a math-verified SP1-Helios root (root_cache.source != Sp1Helios)")]
    WorldIdRootNotTrustless,
    #[msg("upgrade_credit_root requires an existing operator-attested (version=2) root; nothing to upgrade")]
    CreditRootNotUpgradable,
    #[msg(
        "upgrade_credit_root proof nullifier does not match the existing CreditRoot's nullifier"
    )]
    CreditRootNullifierMismatch,
    #[msg("Caller is not the interim-root authority")]
    NotInterimRootAuthority,
    #[msg("Vault identity_claim is not derived from this CreditRoot's nullifier + agent_id")]
    VaultNotRootedToCredit,
    #[msg("SP1 proof bytes are too short to contain the 4-byte vk-hash prefix")]
    Sp1ProofTooShort,
    #[msg("SP1-Helios Groth16 proof failed to verify against the pinned verifying key")]
    Sp1VerifyFailed,
    #[msg("SP1-Helios committed public values are malformed (ABI decode out of bounds)")]
    Sp1PublicValuesMalformed,
    #[msg("No proven storage slot matched the pinned World ID registry address + latest-root slot key")]
    WorldIdSlotNotFound,
    #[msg("New beacon head is older than the cached root's block_number (stale/replayed root)")]
    BeaconHeadNotMonotonic,
    #[msg("Proven beacon head (newHead) does not fit in a u64")]
    BeaconHeadOverflow,
    #[msg("Proof prevHead does not equal the trusted light-client head (not proving forward from current state)")]
    LightClientHeadMismatch,
    #[msg("Proof prevHeader does not equal the trusted light-client header")]
    LightClientHeaderMismatch,
    #[msg("Proof startSyncCommitteeHash does not equal the trusted sync committee (committee not anchored)")]
    SyncCommitteeStartMismatch,
    #[msg("Cached World ID root is older than ROOT_MAX_AGE_SECONDS (stale; a fresher root must be established first)")]
    WorldIdRootStale,

    // ──────────────────────────────────────────────────────────────────────
    // Recourse-rooted delegation graph (depth-N) — Tasks 1–15.
    // Appended now so later tasks can reference them; only the Task-1 variants
    // (NotAdminAuthority) are wired by this task.
    // ──────────────────────────────────────────────────────────────────────
    #[msg("The graph is paused; exposure-increasing verbs are disabled")]
    GraphPaused,
    #[msg("Caller is not the GraphConfig admin authority")]
    NotAdminAuthority,
    #[msg("Caller is not the GraphConfig pause authority")]
    NotPauseAuthority,
    #[msg("Only the admin authority may unpause")]
    OnlyAdminMayUnpause,
    #[msg("Delegation depth would exceed the configured maximum")]
    DelegationDepthExceeded,
    #[msg("An ancestor in the chain is not authenticated")]
    UnauthenticatedAncestorChain,
    #[msg("The ancestor chain is not rooted")]
    AncestorChainNotRooted,
    #[msg("Draw would exceed the node's ceiling")]
    DrawWouldExceedCeiling,
    #[msg("Draw would exceed the node's rate-limit bucket")]
    DrawWouldExceedBucket,
    #[msg("A child cap may not exceed its parent's cap")]
    ChildCapExceedsParent,
    #[msg("Caller is not the controller of this node")]
    NotNodeController,
    #[msg("Release would exceed the subtree's outstanding draw")]
    ReleaseExceedsSubtreeDraw,
    #[msg("This node is frozen")]
    NodeFrozenErr,
    #[msg("Node has outstanding draw and cannot perform this operation")]
    NodeHasOutstandingDraw,
    #[msg("Root attestation is missing")]
    RootAttestationMissing,
    #[msg("This node is already rooted")]
    NodeAlreadyRooted,
    #[msg("This node already has the maximum number of children")]
    ChildCountAtMax,
    #[msg("Rate cap must be greater than zero")]
    RateCapZero,
    #[msg("Wallet does not match the node")]
    WalletNodeMismatch,
    #[msg("Debt-transfer amounts do not reconcile")]
    DebtTransferMismatch,
    #[msg("This shortfall has already been covered")]
    ShortfallAlreadyCovered,
    #[msg("Caller is not an ancestor of the defaulting node")]
    NotAnAncestorOfDefaulter,
    #[msg("There is no uncovered shortfall to cascade")]
    NoUncoveredShortfall,
    #[msg("Node still has a parent edge; re-root via emancipate, not attach_root")]
    NodeStillParented,
    #[msg("This vault is welded to a credit node; its PrincipalNode must be supplied")]
    CreditNodeRequired,
    #[msg("Financier does not match the node's recorded financier (single-tree invariant)")]
    FinancierMismatch,
    #[msg(
        "Node is not closable: it still owes a draw, has children, or has a default in progress"
    )]
    NodeNotClosable,

    // ── Carry-gate family (leveraged-carry engine Phase 0: verify/oracle.rs +
    // verify/solvency.rs). Appended as a block per house convention — order is
    // ABI (6000 + index), never insert mid-enum.
    #[msg("Oracle account does not carry the pinned feed id")]
    OracleFeedMismatch,
    #[msg("Price update lacks full wormhole verification")]
    OracleUnverified,
    #[msg("Oracle price is older than the configured staleness bound")]
    OraclePriceStale,
    #[msg("Oracle price is zero or negative")]
    OracleNonPositivePrice,
    #[msg("Oracle confidence interval exceeds the configured band")]
    OracleConfidenceTooWide,
    #[msg("Valuation feed diverges from the market reference beyond the configured band")]
    OracleDivergenceExceeded,
    #[msg("Redemption-rate growth exceeds the configured yearly cap")]
    OracleGrowthCapExceeded,
    #[msg(
        "Redemption-rate snapshot is invalid (future-dated, non-positive, or exponent mismatch)"
    )]
    OracleSnapshotInvalid,
    #[msg("Redemption-rate valuation requires a market reference price")]
    OracleMarketRefMissing,
    #[msg("Redemption-rate valuation requires a growth-cap snapshot")]
    OracleRrSnapshotMissing,
    #[msg("Unknown valuation mode")]
    OracleInvalidValuationMode,
    #[msg("Carry-gate arithmetic overflow")]
    CarryMathOverflow,
    #[msg("Leverage must be at least 1x (10000 bps)")]
    LeverageBelowOne,
    #[msg("Asset config parameters are out of bounds or inconsistent for the valuation mode")]
    InvalidAssetConfig,
    // Pre-appended for Phase 1 (draw_leverage refuses disabled listings).
    #[msg("This asset's listing is disabled for new leverage")]
    AssetConfigDisabled,
    #[msg("Requested leverage exceeds this asset's configured maximum")]
    LeverageExceedsAssetMax,
    #[msg("Attested market borrow rate exceeds the asset's borrow-spike ceiling")]
    BorrowRateSpike,
    #[msg("Borrow-rate attestation is stale or future-dated")]
    BorrowAttestationStale,
    #[msg("Position is still solvent at the liquidation threshold — cannot attest liquidation")]
    PositionStillSolvent,
    #[msg("No pending liquidation attestation (never attested, or already settled)")]
    NoPendingLiquidation,
    #[msg("Liquidation attestation is stale — re-attest before settling")]
    LiquidationAttestationStale,
    #[msg("spend_limit_atomic must be user-authored and greater than zero (no program default)")]
    SpendLimitRequired,
    #[msg("session_ttl_seconds must be greater than zero (no program default)")]
    SessionTtlRequired,
    #[msg("spend_mode must be 0 (legacy allowance), 1 (later owner grant), or 2 (persistent recurring automation)")]
    UnsupportedSpendMode,
    #[msg("Withdrawal fee exceeds the on-chain WITHDRAWAL_FEE_CAP")]
    FeeExceedsCap,
    #[msg("Pending withdrawal amount does not exceed the withdrawal fee")]
    WithdrawalBelowFee,
    #[msg("fee_treasury must be a real wallet (non-default pubkey)")]
    InvalidFeeTreasury,

    // ── Spread-engine family (2026-07-04 design). Appended as a block per house
    // convention — order is ABI (6000 + index), never insert mid-enum.
    #[msg("accrual_ts is before the node's accrual clock or in the future")]
    AccrualTsInvalid,
    #[msg(
        "accrual_ts is staler than ACCRUAL_TS_MAX_STALENESS_SECS — rebuild against a fresh clock"
    )]
    AccrualTsStale,
    #[msg("rate_bps exceeds the on-chain RATE_BPS_CAP")]
    RateExceedsCap,
    #[msg("interest_take_bps exceeds the on-chain INTEREST_TAKE_CAP_BPS")]
    InterestTakeExceedsCap,
    #[msg("draw_leverage requires a clean node (no outstanding borrowed or accrued_fee) — a position always opens at its own rate")]
    NodeNotClean,

    // ── Carry swap-leg family (2026-07-15 design). Appended as a block per
    // house convention — order is ABI, never insert mid-enum.
    #[msg("swap bracket is mid-flight or does not belong to this vault")]
    SwapBracketMismatch,
    #[msg("swap intent nonce does not match the bracket's replay counter")]
    SwapNonceMismatch,
    #[msg("swap intent expired (expiry_slot passed before execution)")]
    SwapIntentExpired,
    #[msg("swap direction must be 0 (buy), 1 (sell), or 2 (protected send)")]
    SwapUnsupportedDirection,
    #[msg("the following SignV2 must wrap exactly one instruction for the pinned swap program")]
    SwapEnvelopeInvalid,
    #[msg("finish_swap missing, malformed, or not exactly two instructions after swap_for_carry")]
    SwapBracketNotClosed,
    #[msg("swap moved balances outside the passkey-signed bounds (input overspent or output below min_out)")]
    SwapDeltaShortfall,
    #[msg("buying the base asset would strip USDC that backs live obligations (locks + credit liability)")]
    SwapWouldViolateReservation,
    #[msg("swap intent expiry_slot is further than MAX_SWAP_INTENT_HORIZON_SLOTS ahead — sign a tighter window")]
    SwapIntentHorizonTooFar,
    // ── Penny lock (crystallization-cadence 2026-07-06). Appended per house
    // convention — order is ABI (6000 + index), never insert mid-enum.
    /// Slot reserved as an ABI placeholder by the tab-fee-75 deploy; the
    /// deploy-train branch registers `set_max_seller_at_risk`, making it live.
    #[msg("max_seller_at_risk_atomic exceeds the on-chain MAX_SELLER_AT_RISK_CAP")]
    MaxSellerAtRiskExceedsCap,

    // ── Tab platform fee (board #75, 2026-07-24). Appended per house
    // convention — order is ABI (6000 + index), never insert mid-enum.
    #[msg("tab fee leg exceeds graph_config.tab_fee_max_bps of the settle increment")]
    TabFeeExceedsCap,
    #[msg("tab_fee_treasury must be a real wallet (non-default pubkey)")]
    InvalidTabFeeTreasury,
    // ── Consent-at-grant (board #119, Car 3). Appended per house convention —
    // order is ABI (6000 + index), never insert mid-enum.
    #[msg("following instruction is not the exact birth-shape AddAuthorityV1 the grant ceremony authorized")]
    GrantEnvelopeMismatch,

    // ── Line origination P1 (2026-07-25). Append-only ABI family.
    #[msg("recovery_window_seconds must be strictly positive")]
    LineRecoveryWindowNotPositive,
    #[msg("recovery_window_seconds exceeds MAX_LINE_RECOVERY_WINDOW_SECONDS")]
    LineRecoveryWindowExceedsMax,
    #[msg("exposure_id is not the canonical vault/node/evidence-derived Line identity")]
    LineExposureIdMismatch,
    #[msg("Line evidence_hash and terms_hash must both be non-zero commitments")]
    LineCommitmentMissing,
    #[msg("Line due/default schedule is invalid or already expired")]
    LineScheduleInvalid,
    #[msg("the immediately following instruction is not the exact bound draw_credit")]
    LineDrawMismatch,

    // ── Governed Jupiter swap family (2026-07-26). Appended after the Line
    // family so the combined Vault/SDK error ABI remains stable.
    #[msg("governed swap intent is malformed or does not match the carry bracket")]
    GovernedSwapIntentInvalid,
    #[msg("governed swap SignV2 envelope is malformed, smuggles instructions, or is not pinned Jupiter")]
    GovernedSwapEnvelopeInvalid,
    #[msg("governed swap role or session authority is missing, ambiguous, expired, or over-broad")]
    GovernedSwapRoleInvalid,
    #[msg("governed swap route digest does not match the exact finalized SignV2 instruction")]
    GovernedSwapRouteDigestMismatch,
    #[msg("governed swap mint, token program, source, or destination binding is invalid")]
    GovernedSwapTokenBindingMismatch,
    #[msg("governed swap slippage exceeds the signed or protocol ceiling")]
    GovernedSwapSlippageExceeded,
    #[msg("governed swap transaction adjacency is invalid")]
    GovernedSwapAdjacencyInvalid,
    #[msg("governed swap destination ATA preparation is missing, substituted, or exceeds its rent ceiling")]
    GovernedSwapAtaPreparationInvalid,

    // ── Cash-availability / LockedClaim seniority P0 (2026-07-26).
    // This non-deployed family follows the published governed-swap ABI.
    #[msg("SwigVaultBinding version is unsupported")]
    UnsupportedSwigVaultBindingVersion,
    #[msg("Swig, Vault, and reverse binding do not identify one canonical cash ledger")]
    SwigVaultBindingMismatch,
    #[msg("cash source is not the bound Swig wallet's canonical settlement-mint ATA")]
    NonCanonicalVaultCashAccount,
    #[msg("junior debit would spend cash reserved for outstanding LockedClaims")]
    JuniorDebitWouldViolateReservation,
    #[msg("LockedClaim reserve accumulator is smaller than the claim being released")]
    LockedReserveAccountingUnderflow,
    #[msg("a Vault-bound Swig cannot be used as reserve-free Line capital")]
    BoundVaultCannotFundLine,
    #[msg("legacy existing-Swig enrollment is disabled; use atomic enrollment or admin migration")]
    LegacySwigEnrollmentDisabled,

    // ── Governed agent-trading grants (2026-07-26). Append-only ABI family.
    #[msg("governed trade grant operation must be grant or revoke")]
    GovernedTradeGrantOperationInvalid,
    #[msg("governed trade grant PDA, nonce, or stored Vault/Swig binding is invalid")]
    GovernedTradeGrantBindingInvalid,
    #[msg("governed trade grant is inactive, expired, or already initialized")]
    GovernedTradeGrantNotLive,
    #[msg("governed trade grant manager role is missing, ambiguous, or over-broad")]
    GovernedTradeManagerRoleInvalid,
    #[msg("following Swig AddAuthorityV1/RemoveAuthorityV1 is not the exact governed-trade authority leg")]
    GovernedTradeAuthorityEnvelopeMismatch,
    #[msg("governed trade grant expiry is invalid or exceeds the owner-authored TTL")]
    GovernedTradeGrantExpiryInvalid,

    // ── Ed25519 session-key admission hardening (2026-07-26). Append-only.
    #[msg("session public key is not a valid non-small-order Edwards point")]
    InvalidOrWeakEd25519PublicKey,

    // ── Governed portfolio Send family (2026-07-27). This deliberately
    // reuses the existing swap_for_carry ProgramExec marker and SwapBracket
    // nonce. It is appended after every pre-existing family so their ABI error
    // numbers remain stable.
    #[msg("governed send intent is malformed or does not match its exact token movement")]
    GovernedSendIntentInvalid,
    #[msg("governed send SignV2 must wrap exactly one bound TransferChecked instruction")]
    GovernedSendEnvelopeInvalid,
    #[msg("governed send role is missing, ambiguous, role 2, or not the exact owner ProgramExec marker")]
    GovernedSendRoleInvalid,
    #[msg("governed send mint, token program, source, destination, owner, amount, or decimals mismatch")]
    GovernedSendTokenBindingMismatch,
    #[msg("governed send uses an unsupported Token-2022 mint or account extension")]
    GovernedSendTokenExtensionUnsupported,
    #[msg("governed send transaction prefix, adjacency, or terminal position is invalid")]
    GovernedSendAdjacencyInvalid,
    #[msg("governed send destination ATA preparation is missing, substituted, or exceeds its rent ceiling")]
    GovernedSendAtaPreparationInvalid,

    // ── One-time operation-bound WebAuthn authorization family (2026-07-30).
    // Appended after every pre-existing V7 family to preserve deployed ABI codes.
    #[msg("passkey authorization account is missing, foreign, or unsupported")]
    PasskeyAuthorizationMismatch,
    #[msg("passkey authorization nonce is stale or exhausted")]
    PasskeyAuthorizationNonceMismatch,
    #[msg("session replacement requires current_outstanding to be zero")]
    SessionReplacementOutstanding,
    #[msg("session replacement must rotate to a different session pubkey")]
    SessionReplacementKeyUnchanged,
    #[msg("session replacement must retain the retired session's counterparty")]
    SessionReplacementCounterpartyMismatch,
    #[msg("session replacement cap or revolving capacity cannot safely preserve its live meters")]
    SessionReplacementMeterInvalid,

    // ── Legacy V6 obligation-preserving recovery (2026-07-31). Append-only:
    // existing program and SDK error numbers must never move.
    #[msg("legacy depth-1 credit state requires the obligation-preserving recovery migration")]
    LegacyCreditRequiresRecovery,
    #[msg("legacy V6 account must have the exact owner, discriminator, 279-byte allocation, version, and decodable frozen layout")]
    LegacyV6AccountMalformed,
    #[msg("legacy V6 vault PDA, stored bump, or recorded Dexter authority is wrong")]
    LegacyV6IdentityMismatch,
    #[msg("legacy V6 credit tuple is malformed or loses an obligation")]
    LegacyV6CreditMalformed,
    #[msg("legacy V6 vault has an unresolved pending-voucher or session-outstanding ambiguity")]
    LegacyV6PendingVoucherAmbiguous,
    #[msg("legacy V6 live-session set is incomplete, duplicated, foreign, or misderived")]
    LegacyV6SessionSnapshotMismatch,
    #[msg("legacy V6 pending-claim set does not exactly conserve the locked reserve")]
    LegacyV6ClaimSnapshotMismatch,
    #[msg("legacy V6 financier or StandbyBacker account is missing, foreign, or misderived")]
    LegacyV6BackerMismatch,
    #[msg("legacy V6 full-cap retirement would underflow or violate the reserve ledger")]
    LegacyV6ReserveUnderflow,
    #[msg("legacy V6 Swig binding is missing, duplicated, foreign, or inconsistent")]
    LegacyV6BindingMismatch,
    #[msg("legacy V6 recovery node or attestation collides with an existing account")]
    LegacyV6RecoveryCollision,
    #[msg("node id prefix is reserved for sealed legacy V6 recovery nodes")]
    LegacyV6NodeIdReserved,
    #[msg("legacy V6 attestation does not exactly bind this vault, node, authority, and retired ledger")]
    LegacyV6AttestationMismatch,
    #[msg("sealed legacy V6 node still carries debt, shortfall, accrual, subtree draw, deadline, or children")]
    LegacyV6NodeNotClosable,
    #[msg(
        "legacy V6 passkey is not a valid compressed P-256 public point and remains quarantined"
    )]
    LegacyV6PasskeyInvalid,
    #[msg("legacy V6 recovery inputs do not match the frozen reviewed manifest")]
    LegacyV6ManifestMismatch,
    #[msg(
        "pre-V4 public layout migration is quarantined pending a fresh owner-authorized recovery"
    )]
    LegacyLayoutQuarantined,

    // ── Native Tab V2 obligation integrity (2026-07-31). Append-only.
    #[msg("new and replacement sessions must use the context-bound voucher V2 nonce marker")]
    SessionVoucherV2NonceRequired,
    #[msg(
        "voucher V2 registration generation must match the permanent passkey authorization nonce"
    )]
    SessionVoucherV2NonceMismatch,
    #[msg("this session already has one reserved outstanding tab")]
    TabAlreadyOutstanding,
    #[msg("tab reservation amount must be greater than zero")]
    TabReservationZero,
    #[msg("voucher settlement delta exceeds this session's reserved outstanding")]
    VoucherDeltaExceedsOutstanding,
    #[msg(
        "bare operator release is disabled; a signed settle, lock, or force-release is required"
    )]
    VoucherReleaseRequiresSignedSettlement,
    #[msg("the bounded post-expiry voucher settlement window has elapsed")]
    VoucherSettlementGraceElapsed,
    #[msg("context-bound V2 direct settlement requires buyer-signed finality")]
    VoucherFinalityRequired,
    #[msg("session expiry leaves no representable bounded seller settlement window")]
    SessionExpiryTooFar,
    #[msg("a final Native Tab V2 terminal must cover the exact reserved outstanding")]
    VoucherFinalDeltaMismatch,

    // ── Owner-bound legacy V1 Tab terminal bridge (2026-07-31). Append-only.
    #[msg("legacy V1 Tab binding is missing, foreign, malformed, expired, or already consumed")]
    LegacyV1TabBindingMismatch,
    #[msg("legacy V1 Tab voucher does not exactly close the recovered indexed reservation")]
    LegacyV1TabVoucherMismatch,
    #[msg("legacy V1 Tab binding selected a different terminal or lock policy")]
    LegacyV1TabTerminalMismatch,
    #[msg("legacy V1 Tab pending counter no longer equals its explicit recovery ledger")]
    LegacyV1TabCounterMismatch,
    #[msg(
        "context-bound Native Tab crystallization policy is not the deterministic durable policy"
    )]
    VoucherLockPolicyMismatch,
    #[msg(
        "Native Tab session reservations do not exactly equal the Vault pending-voucher counter"
    )]
    NativeTabCounterMismatch,
    #[msg("session revocation consent no longer matches the exact live session and pending-counter state")]
    SessionRevocationStateChanged,
    #[msg("this Swig binding proves identity but not the canonical reserve-safe authority policy")]
    SwigAuthorityPolicyUnattested,

    // ── Historical reserve-safe Swig policy attestation (2026-08-18).
    // Append-only: every previously published program error retains its ABI.
    #[msg("current Swig account bytes do not match the reviewed attestation digest")]
    SwigPolicyAttestationHashMismatch,
    #[msg("historical Swig roles do not match the exact reserve-safe Dexter authority spine")]
    SwigPolicyAttestationProfileMismatch,

    // ── Catalog-governed stock trading V2. Append-only and isolated from V1.
    #[msg("catalog policy identity, epoch, admin, class, or direction state is invalid")]
    CatalogPolicyV2Invalid,
    #[msg("catalog policy epoch must advance monotonically and invalidates prior grants")]
    CatalogPolicyV2EpochInvalid,
    #[msg("governed stock identity is foreign, non-stock, non-current, or digest-mismatched")]
    GovernedAssetIdentityV2Invalid,
    #[msg("governed stock identity facts are immutable; publish a new version")]
    GovernedAssetIdentityV2Immutable,
    #[msg(
        "governed stock current pointer can only advance versions, disable, or narrow directions"
    )]
    GovernedAssetCurrentV2Invalid,
    #[msg("governed stock Buy/Sell direction is absent, broadened, or malformed")]
    GovernedTradeDirectionV2Invalid,
    #[msg("governed stock V2 provisioning manager role is missing, duplicated, or overpowered")]
    GovernedTradeManagerV2Invalid,
    #[msg("governed stock V2 grant fields, PDA, limits, or owner ceremony are invalid")]
    GovernedTradeGrantV2Invalid,
    #[msg("governed stock V2 grant is inactive or bound to an obsolete policy epoch")]
    GovernedTradeGrantV2NotLive,
    #[msg("governed stock V2 child-role authority envelope was substituted or reordered")]
    GovernedTradeAuthorityV2EnvelopeMismatch,
    #[msg("governed stock V2 exact-mint Swig role is missing, stale, exhausted, or overpowered")]
    GovernedTradeRoleV2Invalid,
    #[msg("governed stock V2 agent or facilitator fee-payer binding was substituted")]
    GovernedTradeActorV2Invalid,
    #[msg("governed stock V2 prepared product, issuer, profile, or release digest is stale")]
    GovernedTradeSelectionV2Stale,
    #[msg("governed stock V2 Buy order or family rolling-window authority is exhausted")]
    GovernedTradeBuyMeterV2Exceeded,
    #[msg("governed stock V2 Sell exceeds its frozen position-basis rolling-window cap")]
    GovernedTradeSellMeterV2Exceeded,
    #[msg(
        "governed stock V2 rolling window is invalid or the observed unix clock moved backwards"
    )]
    GovernedTradeWindowV2Invalid,
    #[msg(
        "governed stock V2 authorization tail, route, account, or replay bracket was substituted"
    )]
    GovernedSwapV2EnvelopeInvalid,
    #[msg("governed stock V2 quote attestation key, evidence, impact, freshness, or canonical bytes are invalid")]
    GovernedStockQuoteAttestationV2Invalid,
    #[msg("the canonical wallet automation budget account is required for this spending mode")]
    AutomationBudgetV2Required,
    #[msg("the wallet automation budget is foreign, inactive, malformed, or role-mismatched")]
    AutomationBudgetV2Invalid,
    #[msg("the wallet-wide recurring automation budget is exhausted")]
    AutomationBudgetV2Exceeded,

    // Autonomous Send follows the deployed stock/automation errors0-221.
    #[msg("governed Send authority root or child grant binding is invalid")]
    GovernedSendGrantBindingInvalid,
    #[msg("governed Send authority root or child grant is inactive, expired, or consumed")]
    GovernedSendGrantNotLive,
    #[msg("governed Send registry identity, mint, token program, decimals, or approved-asset evidence is invalid")]
    GovernedSendRegistryBindingInvalid,
    #[msg("governed Send destination is outside the grant's cryptographic scope")]
    GovernedSendDestinationNotAllowed,
    #[msg("governed Send amount exceeds its per-action or aggregate cap")]
    GovernedSendCapExceeded,
    #[msg("governed Send ATA creation or rent-payer policy exceeds the grant")]
    GovernedSendRentPolicyViolation,
    #[msg("governed Send agent signer, authority domain, or parent mandate does not match")]
    GovernedSendAgentAuthorityInvalid,
    #[msg("governed Send grant expiry or owner-authored TTL is invalid")]
    GovernedSendGrantExpiryInvalid,

    // P5 — whole-transaction Swig binding scan.
    #[msg("an unbound Swig instruction on this wallet rides in the transaction (approval reuse)")]
    UnboundSwigInstruction,

    // F — financier must consent to being named as a capital source.
    #[msg("the named financier has not consented to this node drawing on it")]
    FinancierConsentMissing,

    // J — top-level wallet activation (2026-10-18). Append-only.
    #[msg("Swig rejects CPI births since 2026-09-28; create wallets with top-level activate_wallet")]
    BirthRequiresTopLevelActivation,
    #[msg("wallet birth instructions, fee leg, or resulting Swig roles are not the canonical activation")]
    WalletActivationMismatch,
    #[msg("wallet role provisioning kind, manager, gate, or following AddAuthorityV1 is invalid")]
    WalletRoleProvisionInvalid,
    #[msg("this wallet already holds the requested role")]
    WalletRoleAlreadyProvisioned,
}

#[cfg(test)]
mod error_abi_tests {
    use super::VaultError;

    #[test]
    fn historical_attestation_errors_append_after_the_deployed_error_abi() {
        // Exact-base d9e90e65 raw enum discriminants. Anchor adds its global
        // ERROR_CODE_OFFSET when publishing numeric program errors.
        assert_eq!(VaultError::PasskeyAuthorizationMismatch as u32, 158);
        assert_eq!(VaultError::SwigAuthorityPolicyUnattested as u32, 198);
        assert_eq!(VaultError::SwigPolicyAttestationHashMismatch as u32, 199);
        assert_eq!(VaultError::SwigPolicyAttestationProfileMismatch as u32, 200);
        assert_eq!(VaultError::CatalogPolicyV2Invalid as u32, 201);
        assert_eq!(VaultError::GovernedSwapV2EnvelopeInvalid as u32, 217);
        assert_eq!(
            VaultError::GovernedStockQuoteAttestationV2Invalid as u32,
            218
        );
        assert_eq!(VaultError::AutomationBudgetV2Required as u32, 219);
        assert_eq!(VaultError::AutomationBudgetV2Invalid as u32, 220);
        assert_eq!(VaultError::AutomationBudgetV2Exceeded as u32, 221);
    }
    #[test]
    fn autonomous_send_errors_append_after_the_deployed_stock_error_abi() {
        assert_eq!(VaultError::GovernedSendGrantBindingInvalid as u32, 222);
        assert_eq!(VaultError::GovernedSendGrantNotLive as u32, 223);
        assert_eq!(VaultError::GovernedSendRegistryBindingInvalid as u32, 224);
        assert_eq!(VaultError::GovernedSendDestinationNotAllowed as u32, 225);
        assert_eq!(VaultError::GovernedSendCapExceeded as u32, 226);
        assert_eq!(VaultError::GovernedSendRentPolicyViolation as u32, 227);
        assert_eq!(VaultError::GovernedSendAgentAuthorityInvalid as u32, 228);
        assert_eq!(VaultError::GovernedSendGrantExpiryInvalid as u32, 229);
    }

}
