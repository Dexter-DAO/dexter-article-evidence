//! Task 11 — events for the depth-N recourse-rooted delegation graph.
//!
//! Every state-changing graph verb emits one of these via `emit_cpi!` (CPI-based
//! events, per IMPROVE RANK 4): the program self-invokes with the event in the
//! instruction DATA rather than a `sol_log_data` log, so the record survives CPI
//! log truncation. `#[event_cpi]` on each emitting accounts struct injects the
//! two accounts the self-CPI needs (`event_authority` + `program`).
//!
//! Field names are the authoritative Task-11 spec — keep them verbatim; the SDK
//! decoders (Task 13) bind to them.

use anchor_lang::prelude::*;

#[event]
pub struct GovernedNativeSwapExecuted {
    pub vault: Pubkey,
    pub swig: Pubkey,
    pub direction: u8,
    pub amount_in: u64,
    pub min_out: u64,
    pub nonce: u64,
    pub native_before: u64,
    pub native_after: u64,
    pub cash_before: u64,
    pub cash_after: u64,
    pub temporary_wsol: Pubkey,
}

/// `draw_credit` — a leaf booked new debt and the authenticated chain rolled it
/// up to the ceiling-root. `depth` = ancestor hops above the leaf (0 ⇒ a depth-1
/// leaf-root drawing on itself).
#[event]
pub struct CreditDrawn {
    pub node: Pubkey,
    pub root: Pubkey,
    pub amount: u64,
    pub new_borrowed: u64,
    pub new_subtree_draw: u64,
    pub depth: u8,
    /// Spread engine: interest banked by this touch's settle (pre-draw balance
    /// × rate × elapsed) plus the running total — the ledger's accrual feed.
    pub accrued_delta: u64,
    pub new_accrued_fee: u64,
    /// The rate the NEW balance accrues at (draw_leverage stamps the keeper-
    /// attested borrow rate here; draw_credit reports the standing rate).
    pub rate_bps: u16,
}

/// `create_node` — a node was created. `parent == None` for a root-less node, or
/// `Some(parent)` for a delegate (GROW consent). The cap is the granted velocity
/// (`cap_rate` per `cap_period` secs) plus any aggregate `ceiling`.
#[event]
pub struct NodeDelegated {
    pub parent: Option<Pubkey>,
    pub child: Pubkey,
    pub cap_rate: u64,
    pub cap_period: u32,
    pub ceiling: Option<u64>,
    pub controller: Pubkey,
}

/// `repay_credit` — outstanding draw was paid down (leaf + every ancestor
/// decremented in lockstep). `new_borrowed` is the leaf's remaining debt.
#[event]
pub struct CreditRepaid {
    pub node: Pubkey,
    pub root: Pubkey,
    pub amount: u64,
    pub new_borrowed: u64,
    /// Spread engine: how the payment split. `amount` = principal_paid +
    /// interest_paid; of the interest, `treasury_cut` went to the protocol
    /// treasury leg and the rest to the financier leg.
    pub principal_paid: u64,
    pub interest_paid: u64,
    pub treasury_cut: u64,
    pub new_accrued_fee: u64,
}

/// `seize_collateral` (self-seize) and `seize_ancestor` (the RUNG-3 cascade) —
/// `amount` of collateral was seized against `node`'s debt, rolling down the
/// authenticated chain to `root`.
#[event]
pub struct CollateralSeized {
    pub node: Pubkey,
    pub root: Pubkey,
    pub amount: u64,
    /// Spread engine (self-seize only; zero on cascade hops): interest
    /// collected from the collateral after principal, and the uncollected
    /// remainder written off at seizure (the earnings claim crystallizes —
    /// the cascade covers principal only).
    pub interest_seized: u64,
    pub interest_written_off: u64,
}

/// Emitted by `attest_liquidation` — the pre-swap, oracle-priced justification
/// that a levered position is underwater. The snapshot locked onto
/// LeveredPosition is the seizable ceiling `settle_liquidation` later consumes.
#[event]
pub struct PositionAttested {
    pub vault: Pubkey,
    /// Oracle-priced value of the base holding, USDC base units (conservative
    /// lower feed per §8.1).
    pub position_value: u64,
    /// Debt at attest time (node.borrowed + accrued_fee).
    pub debt: u64,
    /// LTV in bps at the attested price (reporting only; the gate is is_solvent).
    pub ltv_bps: u32,
}

/// Emitted by `settle_liquidation` — the post-swap money settlement. `seized` is
/// what the financier recovered from the USDC proceeds; `shortfall` is any
/// uncovered remainder left outstanding on the node.
#[event]
pub struct PositionLiquidated {
    pub node: Pubkey,
    pub root: Pubkey,
    pub seized: u64,
    pub shortfall: u64,
    /// Spread engine: interest collected from the proceeds after principal,
    /// and the uncollected remainder written off at settlement.
    pub interest_seized: u64,
    pub interest_written_off: u64,
}

/// `emancipate` — `emancipated_node` cut its edge from `parent` and re-rooted.
#[event]
pub struct SubtreeReleased {
    pub parent: Pubkey,
    pub emancipated_node: Pubkey,
}

/// `open_standby` (grow), `close_standby` (cut-edge) and `set_standby_reserve`
/// (set-root-ceiling) — a node's `RateCap`/edge changed. Carries only the
/// affected node; consumers re-fetch the node for the new cap.
#[event]
pub struct CapChanged {
    pub node: Pubkey,
}

/// A node's `frozen` flag was toggled `by` an authority. (Struct defined per the
/// Task-11 spec; the freeze verb itself lands in a later task — no emitter yet.)
#[event]
pub struct NodeFrozen {
    pub node: Pubkey,
    pub by: Pubkey,
    pub frozen: bool,
}

/// `attach_root` — an existing node was pointed UP at a human `credit_root`
/// (wallet-first acquire-a-root; one-human-one-root by aggregation).
#[event]
pub struct RootAttached {
    pub node: Pubkey,
    pub credit_root: Pubkey,
}

/// `attach_node` — the wallet↔node POSSESSION link (`Vault.node`) was set
/// (passkey-gated, link-once).
#[event]
pub struct NodeAttached {
    pub vault: Pubkey,
    pub node: Pubkey,
}

/// `close_node` — a terminal `PrincipalNode` was reclaimed (rent returned to
/// `rent_recipient`). `vault` is the un-welded wallet when one was linked, else
/// `None` (an orphan-node reap).
#[event]
pub struct NodeClosed {
    pub node: Pubkey,
    pub vault: Option<Pubkey>,
    pub rent_recipient: Pubkey,
}

/// `establish_financier_config` (F, credit consent) — a financier's consent
/// record was created, naming the authority allowed to authorize root-less
/// nodes to draw on it.
#[event]
pub struct FinancierConfigEstablished {
    pub financier: Pubkey,
    pub authority: Pubkey,
}

/// `init_asset_config` — a base asset was listed for the leveraged-carry
/// engine (Phase 0 allowlist row created by the graph admin). Carries the
/// security-critical fields (feed ids, gate bands) so an off-chain monitor can
/// detect a feed-id swap or a gate-loosening retune from the event stream
/// alone — the leverage/liq summary is not enough to spot a repointed feed.
#[event]
pub struct AssetConfigInitialized {
    pub base_mint: Pubkey,
    pub valuation_mode: u8,
    pub primary_feed_id: [u8; 32],
    pub market_feed_id: [u8; 32],
    pub max_age_secs: u32,
    pub max_conf_bps: u16,
    pub max_divergence_bps: u16,
    pub max_growth_bps_per_year: u32,
    pub max_leverage_bps: u32,
    pub liq_threshold_bps: u16,
    pub enabled: bool,
}

/// `update_asset_config` — an existing listing's risk parameters were retuned
/// (or the listing was enabled/disabled) by the graph admin. Full-replace, so
/// this emits the same security-critical fields as the init event: a monitor
/// diffs consecutive events to catch a primary_feed_id swap or a widened band.
#[event]
pub struct AssetConfigUpdated {
    pub base_mint: Pubkey,
    pub valuation_mode: u8,
    pub primary_feed_id: [u8; 32],
    pub market_feed_id: [u8; 32],
    pub max_age_secs: u32,
    pub max_conf_bps: u16,
    pub max_divergence_bps: u16,
    pub max_growth_bps_per_year: u32,
    pub max_leverage_bps: u32,
    pub liq_threshold_bps: u16,
    pub enabled: bool,
}

/// Spread engine — `set_node_rate` repriced a node's line. `accrued_settled`
/// is the interest banked at the OLD rate before the switch (design D7:
/// repricing never retroactively recharges old debt).
#[event]
pub struct NodeRateSet {
    pub node: Pubkey,
    pub old_rate_bps: u16,
    pub new_rate_bps: u16,
    pub accrued_settled: u64,
}

/// Spread engine — `set_interest_take` retuned the protocol's share of
/// collected interest (cold admin, capped by INTEREST_TAKE_CAP_BPS).
#[event]
pub struct InterestTakeSet {
    pub old_take_bps: u16,
    pub new_take_bps: u16,
}

/// Penny lock — `set_max_seller_at_risk` retuned the seller-abandonment
/// exposure knob (cold admin, capped by MAX_SELLER_AT_RISK_CAP). A SECURITY
/// parameter change — the maximum a seller can lose to buyer walkout — so it
/// is on-chain-auditable by event, never a silent field write (spec §2 [A13];
/// the set_withdrawal_fee no-event shape was explicitly rejected).
#[event]
pub struct MaxSellerAtRiskSet {
    pub old_atomic: u64,
    pub new_atomic: u64,
}

/// Tab platform fee — `set_tab_fee_policy` changed the fee treasury and/or the
/// on-chain fee ceiling (cold admin). A REVENUE + seller-protection parameter
/// change, so it is on-chain-auditable by event (the MaxSellerAtRiskSet
/// precedent; board #75, 2026-07-24).
#[event]
pub struct TabFeePolicySet {
    pub old_treasury: Pubkey,
    pub new_treasury: Pubkey,
    pub old_max_bps: u16,
    pub new_max_bps: u16,
}

/// `grant_agent_session` (board #119) — an owner passkey authored the agent
/// spending allowance after an owner-only (mode-1) birth. The security
/// parameter of record for the consent-at-grant law; field names are ABI.
#[event]
pub struct AgentSessionGranted {
    pub vault: Pubkey,
    pub dexter_master: Pubkey,
    pub spend_limit_atomic: u64,
    pub session_ttl_seconds: u64,
}

/// A protected owner Send was authorized and its shared per-vault replay nonce
/// consumed. The following Swig SignV2 remains in the same atomic transaction.
/// State rolls back if it fails; transaction logs may still contain this event,
/// so indexers must accept it only from a finalized transaction with no error
/// and matching transfer evidence.
#[event]
pub struct GovernedSendAuthorized {
    pub vault: Pubkey,
    pub swig: Pubkey,
    pub swig_role_id: u32,
    pub fee_payer: Pubkey,
    pub mint: Pubkey,
    pub token_program: Pubkey,
    pub source_token_account: Pubkey,
    pub destination_token_account: Pubkey,
    pub destination_owner: Pubkey,
    pub amount: u64,
    pub decimals: u8,
    pub nonce: u64,
    pub expiry_slot: u64,
    pub transfer_instruction_digest: [u8; 32],
}

/// Owner-passkey lifecycle event for the separate governed-trading role.
#[event]
pub struct GovernedTradeGrantChanged {
    pub vault: Pubkey,
    pub grant: Pubkey,
    pub role_id: u32,
    pub grant_nonce: u64,
    pub dexter_master_pubkey: Pubkey,
    pub agent_session_pubkey: Pubkey,
    pub input_mint: Pubkey,
    pub spend_limit_atomic: u64,
    pub expires_at_slot: u64,
    pub active: bool,
}

#[event]
pub struct GovernedSendGrantChanged {
    pub vault: Pubkey,
    pub authority_root: Pubkey,
    pub grant: Pubkey,
    pub agent_session_pubkey: Pubkey,
    pub authority_epoch: u64,
    pub grant_nonce: u64,
    pub grant_revision: u64,
    pub registry_identity_digest: [u8; 32],
    pub asset_mint: Pubkey,
    pub token_program: Pubkey,
    pub per_action_cap_atomic: u64,
    pub aggregate_cap_atomic: u64,
    pub expires_at_slot: u64,
    pub active: bool,
    pub root_active: bool,
}

/// Permanent lifecycle signal for the parent governed-Send authority root.
/// A root revocation invalidates every child grant without enumerating them.
#[event]
pub struct GovernedSendAuthorityRootChanged {
    pub vault: Pubkey,
    pub authority_root: Pubkey,
    pub agent_session_pubkey: Pubkey,
    pub authority_epoch: u64,
    pub root_settings_digest: [u8; 32],
    pub owner_authority_binding: [u8; 32],
    pub active: bool,
    pub revoked_at_slot: u64,
}

/// `record_line_origination` — compact index key for the permanent immutable
/// receipt. Consumers fetch `line_origination` for the full evidence and terms
/// snapshot; `CreditDrawn` remains byte-for-byte unchanged.
#[event]
pub struct LineOriginationRecorded {
    pub exposure_id: [u8; 32],
    pub line_origination: Pubkey,
    pub vault: Pubkey,
    pub node: Pubkey,
    pub principal: u64,
    pub default_at: i64,
}

#[event]
pub struct GovernedNativeSendAuthorized {
    pub vault: Pubkey,
    pub swig: Pubkey,
    pub swig_role_id: u32,
    pub fee_payer: Pubkey,
    pub source: Pubkey,
    pub destination: Pubkey,
    pub amount_lamports: u64,
    pub minimum_remaining_lamports: u64,
    pub nonce: u64,
    pub expiry_slot: u64,
    pub transfer_instruction_digest: [u8; 32],
}

#[event]
pub struct OwnerCurrencySwapExecuted {
    pub vault: Pubkey,
    pub swig: Pubkey,
    pub direction: u8,
    pub token_mint: Pubkey,
    pub token_program: Pubkey,
    pub amount_in: u64,
    pub min_out: u64,
    pub nonce: u64,
    pub token_before: u64,
    pub token_after: u64,
    pub native_before: u64,
    pub native_after: u64,
    pub temporary_wsol: Pubkey,
}
