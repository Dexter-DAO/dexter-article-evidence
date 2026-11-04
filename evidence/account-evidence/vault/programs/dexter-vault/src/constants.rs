use anchor_lang::prelude::*;

/// Mainnet-deployed Swig wallet program. The dexter-vault program references
/// this when an instruction's account list must satisfy Swig's ProgramExec
/// authority validator — e.g. finalize_withdrawal, whose account[0..1] of
/// [swig, swig_wallet_address] is anchored by Anchor seeds constraints whose
/// `seeds::program` is this id.
pub const SWIG_PROGRAM_ID: Pubkey = pubkey!("swigypWHEksbC64pWKwah1WTeh9JXwx8H1rJHLdbQMB");

/// Swig wallet address PDA seed prefix. The Swig program derives the wallet
/// address (the PDA that owns each Swig wallet's SPL token ATAs) as
/// `[b"swig-wallet-address", swig_pubkey]` under SWIG_PROGRAM_ID. Sourced
/// from /tmp/swig-wallet/state/src/swig.rs:50 (swig_wallet_address_seeds).
pub const SWIG_WALLET_ADDRESS_SEED: &[u8] = b"swig-wallet-address";

/// One-to-one reverse binding from a Swig config account to its Dexter Vault.
/// PDA: `[SWIG_VAULT_BINDING_SEED, swig]` under this program.
///
/// `Vault.swig_address` alone is only a forward pointer: without this reverse
/// PDA, two Vault accounts can point at the same Swig while each carries an
/// independent `outstanding_locked_amount`, making either reserve accumulator
/// incomplete. The binding PDA's existence is the global uniqueness guard.
pub const SWIG_VAULT_BINDING_SEED: &[u8] = b"swig-vault";

/// Historical depth-1 financier reserve ledger. Recovery of a 279-byte V6
/// vault must retire its entire promised cap from this exact PDA exactly once.
pub const STANDBY_BACKER_SEED: &[u8] = b"standby-backer";

/// Immutable evidence row for the lossless 279-byte V6 recovery.
/// PDA: `[LEGACY_V6_ATTESTATION_SEED, vault]`.
pub const LEGACY_V6_ATTESTATION_SEED: &[u8] = b"legacy-v6-attest";

/// Owner-authored recovery consent for a legacy V6 Vault whose recorded hot
/// Dexter authority is no longer available. PDA:
/// `[LEGACY_V6_CONSENT_SEED, vault]`.
pub const LEGACY_V6_CONSENT_SEED: &[u8] = b"legacy-v6-consent";

/// Permanent mutable resolution row for the exact legacy voucher counters
/// attested at recovery. PDA: `[LEGACY_V6_VOUCHER_RESOLUTION_SEED, vault]`.
pub const LEGACY_V6_VOUCHER_RESOLUTION_SEED: &[u8] = b"legacy-v6-vouchers";

/// Permanent owner-passkey binding for one quarantined legacy V1 voucher.
/// This is intentionally global by `(session_pubkey, channel_id)` because the
/// historical wire omitted Vault and SessionAccount identities.
pub const LEGACY_V1_TAB_BINDING_SEED: &[u8] = b"legacy-v1-tab";

/// A signed consent may remain live for at most seven days. Drift can be
/// re-authorized at the next passkey nonce/generation until one consent is
/// consumed; consumption is a permanent tombstone.
pub const LEGACY_V6_CONSENT_MAX_TTL_SECONDS: i64 = 7 * 24 * 60 * 60;

/// A legacy V1 voucher may reach a terminal only inside this bounded interval
/// after recovery and only through a fresh exact current-owner passkey binding.
/// Seller possession alone is never authorization.
pub const LEGACY_V6_VOUCHER_SETTLEMENT_WINDOW_SECONDS: i64 = 7 * 24 * 60 * 60;
pub const LEGACY_V6_RECOVERY_MAX_SESSIONS: usize = 6;
pub const LEGACY_V6_RECOVERY_MAX_CLAIMS: usize = 2;

/// Node ids beginning with this prefix belong exclusively to the legacy V6
/// recovery instruction. Public `create_node` rejects the namespace.
pub const LEGACY_V6_NODE_ID_PREFIX: &[u8; 8] = b"legacy6:";

/// Domain-separated suffix source for the deterministic recovery node id.
pub const LEGACY_V6_NODE_ID_DOMAIN: &[u8] = b"dexter-legacy-v6-node-v1";

/// Registration/replacement supplies the complete sibling set in one Solana
/// transaction. Keep that fleet bounded to the reviewed, recoverable shape.
pub const SESSION_FLEET_MAX: u8 = 6;

/// USDC mainnet mint — the credit unit. Canonical home; the financier-tier binds
/// every de-risking payout (repay / seize / cascade) to the node financier's USDC
/// ATA derived against this mint. (NOTE: instructions/set_swig_atomic.rs carries a
/// local `USDC_MAINNET` of the same value — repoint it here to dedupe; minor.)
pub const USDC_MAINNET: Pubkey = pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");

/// The two assets presently admitted to the governed portfolio Send surface.
/// This allowlist is intentionally on-chain: a caller cannot turn the
/// role-9 `All` permission into an arbitrary-token escape hatch by swapping
/// account keys in the outer transaction.
pub const DEXTER_MINT_MAINNET: Pubkey = pubkey!("EfPoo4wWgxKVToit7yX5VtXXBrhao4G8L7vrbKy6pump");
pub const SPCX_MINT_MAINNET: Pubkey = pubkey!("SPCXxcqXj6e5dJDVNovHN8744zkbhM2bYudU45BimGb");
pub const GOVERNED_SEND_DECIMALS: u8 = 6;

/// Parent authority and child grant PDAs for autonomous governed Send.
/// These are deliberately separate from Tab sessions, role-2 agent spend,
/// and governed-trade grants so no credential can be substituted across
/// authority domains.
pub const GOVERNED_SEND_AUTHORITY_ROOT_SEED: &[u8] = b"governed-send-root";
pub const GOVERNED_SEND_GRANT_SEED: &[u8] = b"governed-send-grant";

/// Session PDA seed prefix. A per-counterparty session lives at
/// `[SESSION_SEED, vault_pubkey, allowed_counterparty]` under this program.
/// One session per (vault, counterparty); re-register replaces in place.
pub const SESSION_SEED: &[u8] = b"session";

/// Per-vault WebAuthn replay-guard PDA seed. The account at
/// `[PASSKEY_AUTHORIZATION_SEED, vault_pubkey]` carries the monotonic nonce
/// consumed by every passkey-gated instruction. It has no close/reset path.
pub const PASSKEY_AUTHORIZATION_SEED: &[u8] = b"passkey_authorization";

/// World ID root cache PDA seed. Singleton: `[WORLD_ID_ROOT_SEED]`.
pub const WORLD_ID_ROOT_SEED: &[u8] = b"world_id_root";

/// World ID registry-HISTORY ring PDA seed (PERFECT-2). Singleton:
/// `[WORLD_ID_ROOT_HISTORY_SEED]`. Holds SP1-attested historical registry roots
/// so a v2 CreditRoot can converge to trustless against a root that is no longer
/// `_latestRoot`. Written only by `establish_root` from SNARK-verified outputs.
pub const WORLD_ID_ROOT_HISTORY_SEED: &[u8] = b"world_id_root_history";

/// SP1-Helios light-client continuity state PDA seed. Singleton:
/// `[LIGHT_CLIENT_STATE_SEED]`. Seeded once from a trusted checkpoint; advanced
/// by every `establish_root`. This is the on-chain anchor that makes the
/// trustless World ID root actually trustless (it pins the beacon sync committee
/// so a prover cannot substitute its own).
pub const LIGHT_CLIENT_STATE_SEED: &[u8] = b"lc_state";

/// The DEDICATED root-attestation operator (PERFECT-2, 2026-07-05). Signs the
/// operator-attested instant-mint (`establish_credit_root_trusted`) + the interim
/// root / light-client seed instructions. Moved OFF the program upgrade-authority
/// (X4o2) so the key that a live network service holds in its hot path is NOT the
/// key that can replace the program bytecode — least privilege. X4o2 remains the
/// program upgrade-authority only (set at deploy, not here). This key must be
/// funded with SOL to pay CreditRoot rent on each instant establish.
pub const INTERIM_ROOT_AUTHORITY: Pubkey =
    anchor_lang::pubkey!("Root1qgf4hpvihXWivsvHNAhDdPMhwgVkwyGJiz38iL"); // dedicated hot operator

/// CreditRoot PDA seed prefix. Per-human: `[CREDIT_ROOT_SEED, nullifier]`.
pub const CREDIT_ROOT_SEED: &[u8] = b"credit_root";

// ──────────────────────────────────────────────────────────────────────────
// ✅ PINNED 2026-07-01 — the trustless-path World ID credential constants below
// are now REAL production values, each read live and INDEPENDENTLY RE-VERIFIED
// from World Chain (chainId 480) on-chain registries. They are NO LONGER
// [0u8; 32] placeholders. (Historical note: fresh readers repeatedly, 5–6×, saw
// the old zeros and wrongly concluded "impossible" — that era is over.)
//
// These gate the TRUSTLESS path (`establish_credit_root` / `upgrade_credit_root`,
// version=1). The TRUSTED path (`establish_credit_root_trusted`, version=2) has
// been LIVE since 2026-06-23 (Branch's real Orb, CreditRoot BXsTfueSe…). Full
// record: docs/PERSONHOOD-CREDIT-MAINNET-2026-06-23.md.
//
// Provenance (a wrong oprf_pk/cred_pk = a Sybil hole → read from source, never guessed):
//   • DEXTER_ISSUER_SCHEMA_ID = 1 (Orb).  DEXTER_TREE_DEPTH = 30 (WorldIDRegistry.getTreeDepth).
//   • DEXTER_CRED_PK_0/1 = CredentialSchemaIssuerRegistry(0x9412…5d2c).issuerSchemaIdToPubkey(1) → (x,y). Orb key, y < R (clean, unlike passport 9303).
//   • DEXTER_OPRF_PK_0/1 = OprfKeyRegistry(0x0D8b461799474207A3d223553d4d5e6609cb0c69).getOprfPublicKey(uint160(rpId=0xa9ad1684bee7cfe0)) → (x,y). Sybil-critical.
//   • DEXTER_RP_ID = the RP u64 0xa9ad1684bee7cfe0 (idkit RpId→FieldElement is the raw u64, NOT hashed).
//   • DEXTER_ACTION = keccak256("dexter-credit-root-v3") >> 8 (idkit-server hashToField). All 8 values are in-field (< BN254 Fr).
//     (v3 2026-07-04: v4 uniqueness is ONCE per human per action; v1+v2 were consumed by
//     capture/test taps — Branch's real tap needs a fresh namespace. Never point tooling here.)
// The `fixture-test-context`-gated overrides below remain the TACEO test values —
// NEVER enable that feature in a production build.
//
// STILL GATED ON DEPLOY: these compile into the program bytecode, so they go LIVE
// only when the program is upgraded on mainnet (the deployed Hg3w… carries the old
// build until then). SP1_HELIOS_VKEY_HASH / WORLD_ID_REGISTRY_ADDR are already
// pinned + live (the trustless ROOT deployed 2026-07-01).
// ──────────────────────────────────────────────────────────────────────────

// ──────────────────────────────────────────────────────────────────────────
// Phase 2 — trustless SP1-Helios root (`establish_root`) pins.
//
// `establish_root` accepts an SP1-Helios Groth16 proof attesting an Ethereum
// light-client state transition, then reads the World ID registry's
// `_latestRoot` storage slot OUT of the proven `VerifiedStorageSlot[]` and
// caches it as a trustless `RootSource::Sp1Helios` root. The pins below bind
// that proof to the REAL Dexter deployment; the proving-path-dependent ones are
// zero ONLY pending the deploy pin — the proving path is BUILT + validated (2026-06-30).
//
// ✅ RESOLVED 2026-06-30: the L2-anchor proving path (L1 light-client → World Chain
// AnchorStateRegistry → resolved dispute-game output root → L2 state → registry slot
// 15) is BUILT + execute-validated against live mainnet+World Chain. The 2026-06-22
// finding below is HISTORICAL — the "single-hop L1 can't reach an L2 slot" problem is SOLVED.
// ⚠️ ARCHITECTURE FINDING (2026-06-22, cited research): the World ID v4
// `WorldIDRegistry` is deployed on **World Chain mainnet (chainId 480, an
// OP-stack L2)**, NOT on Ethereum L1. Proxy
// `0x0000000000aE079eB8a274cD51c0f44a9E4d67d4` (impl
// `0xb516AF777A24cbCE73a4b296B33ce4B815B9D162`), per
// world-id-protocol/contracts/deployments/core/production.json. An Ethereum-L1
// SP1-Helios proves L1 storage slots and CANNOT directly prove a World Chain L2
// slot. The trustless leg therefore needs an L2-aware proving path (e.g. L1
// Helios → OP-stack output root posted on L1 → L2 storage proof against that
// output root's state root), not the single-hop L1 storage proof this module
// currently assumes. The verify+ABI-decode+slot-extract machinery is reusable;
// the PROOF SOURCE + public-values shape change. Resolve before pinning the addr.
// ──────────────────────────────────────────────────────────────────────────

/// SP1-Helios program verifying-key hash, 0x-prefixed 32-byte hex string.
/// `verify_proof_v6` requires this exact `&str` form (it `hex`-decodes internally).
/// PINNED (2026-07-05, PERFECT-2) — the SP1 **v6** guest ELF's `vk.bytes32()`
/// after the registry-HISTORY change (`ProofOutputs.attestedRoots` + the
/// rootTimestamps attestation in `program/src/main.rs`). Deterministic from the
/// docker-reproducible ELF (`just update-elf`, tag v6.1.0); re-derive with
/// `cargo run -p prover --bin print_vkey`. Prior v3-era vkey was
/// `0x006b05a17a12ae3a1395c77268f446acd0b339767c9e722ca657d01cee8d0489`.
pub const SP1_HELIOS_VKEY_HASH: &str =
    "0x002c1fb04a1301c5a4ad5608cb1558cbbd51639d2023a6aee941acfe8729ea71";

/// SP1 **v6** (Hypercube) recursion VK root — the `proof[36..68]` field every v6
/// proof carries. PINNED (2026-07-01) from the first real network-generated v6
/// proof of the SP1-Helios guest. `verify_proof_v6` rejects any proof whose
/// vk_root != this (pairing also binds it, so a wrong pin only causes rejection,
/// never a forge). Stable across the v6.1.0→v6.3.1 line. See `SP1_HELIOS_VKEY_HASH`.
pub const SP1_HELIOS_VK_ROOT: [u8; 32] = [
    0x00, 0x2f, 0x85, 0x0e, 0xe9, 0x98, 0x97, 0x4d, 0x6c, 0xc0, 0x0e, 0x50, 0xcd, 0x08, 0x14, 0xb0,
    0x98, 0xc0, 0x5b, 0xfa, 0xde, 0x46, 0x6d, 0x28, 0x57, 0x32, 0x40, 0xd0, 0x57, 0xf2, 0x53, 0x52,
];

/// WorldIDRegistry proxy address (20 bytes) whose `_latestRoot` slot the proof
/// must carry. The proven slot's `contractAddress` MUST equal this.
/// = `0x0000000000aE079eB8a274cD51c0f44a9E4d67d4` on **World Chain (chainId 480)**.
/// PINNED (2026-07-01) — confirmed byte-for-byte against the first real network v6
/// proof's committed `VerifiedStorageSlot` (registry slot 15 → the World ID root).
pub const WORLD_ID_REGISTRY_ADDR: [u8; 20] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0xae, 0x07, 0x9e, 0xb8, 0xa2, 0x74, 0xcd, 0x51, 0xc0, 0xf4, 0x4a,
    0x9e, 0x4d, 0x67, 0xd4,
];

/// Storage slot index of `_latestRoot` inside the WorldIDRegistry contract
/// (32-byte big-endian EVM slot key). CONFIRMED = slot 15: `forge inspect
/// WorldIDRegistry storageLayout` at deploy commit b5fafb6 reports slot 15, and
/// a live `eth_getStorageAt(proxy, 15)` on World Chain equals `getLatestRoot()`
/// byte-for-byte (verified 2026-06-22). This is the contract's slot regardless
/// of which chain/proof path reads it, so it is safe to pin now.
pub const WORLD_ID_LATEST_ROOT_SLOT: [u8; 32] = {
    let mut s = [0u8; 32];
    s[31] = 15; // big-endian EVM slot key 0x0f
    s
};

/// Freshness window (seconds) for `WorldIdRootCache` reads. ENFORCED 2026-06-30:
/// `establish_credit_root` rejects a cached root older than this at read time
/// (`VaultError::WorldIdRootStale`). 24h default; tune for World Chain's anchor
/// finality lag (~days, see the trustless-root pickup) before mainnet.
pub const ROOT_MAX_AGE_SECONDS: i64 = 86_400;

/// CreditEvent PDA seed prefix. Per-event: `[CREDIT_EVENT_SEED, nullifier, seq_le]`.
pub const CREDIT_EVENT_SEED: &[u8] = b"credit_event";

/// World ID v4 Dexter context — Dexter's registered rp_id as a bn254 field
/// element (big-endian, 32 bytes). The nullifier is deterministic per
/// (human, rp_id, action); these are FIXED protocol constants — changing either
/// yields a different nullifier space (a deliberate migration, never routine).
///
/// REGISTRATION DONE (not pending): app `app_637…`, RP `rp_a9ad1684bee7cfe0`,
/// action `dexter-credit-root-v3` (production, active; v1+v2 burned — see header). The bn254 field element =
/// `public_inputs[8]` (rp_id) of a real production proof — read it from the existing
/// capture `/tmp/dexter-worldid-proof.json` and pin. Zero ONLY pending that pin.
/// (Tests use the fixture's own public_inputs[8] / public_inputs[9].)
///
/// The production value is `[0u8; 32]` (fail-closed: until Dexter registers a
/// real rp_id, NO real-context proof can bind). The TEST-ONLY value below,
/// gated behind the `fixture-test-context` cargo feature, is the TACEO
/// fixture's own rp_id (decimal 950325648507560155068233096743093215539447660945,
/// big-endian) so the committed fixture proof binds against a deployed test
/// program in the validator happy-path test. **NEVER enable
/// `fixture-test-context` in a production build.**
#[cfg(not(feature = "fixture-test-context"))]
pub const DEXTER_RP_ID: [u8; 32] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 169, 173, 22, 132, 190,
    231, 207, 224,
];

/// TEST-ONLY override of `DEXTER_RP_ID`. See the production definition above.
/// Active only under the `fixture-test-context` feature (TACEO fixture rp_id).
#[cfg(feature = "fixture-test-context")]
pub const DEXTER_RP_ID: [u8; 32] = [
    0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 166u8, 118u8, 19u8, 110u8, 39u8,
    200u8, 234u8, 233u8, 50u8, 91u8, 140u8, 145u8, 235u8, 223u8, 140u8, 130u8, 68u8, 143u8, 1u8,
    145u8,
];

/// World ID v4 Dexter context — Dexter's registered action as a bn254 field
/// element (big-endian, 32 bytes). See `DEXTER_RP_ID` for full rationale.
///
/// Registration DONE — see `DEXTER_RP_ID`. = `public_inputs[9]` of a real
/// `dexter-credit-root-v2`-era proof (read from `/tmp/dexter-worldid-proof.json`; that namespace is burned — historical fixture only).
/// Zero ONLY pending the deploy pin. The `fixture-test-context`-gated value is the
/// TACEO fixture's action (decimal
/// 84721944028150696728472418813119358007006361082259892623669024918011698311,
/// big-endian). **NEVER enable `fixture-test-context` in a production build.**
// PERFECT-2 (2026-07-05): re-pinned v3 → v4. v3 was consumed by Branch's
// 2026-07-04 live tap whose proof was lost client-side; v4 is the live action
// (portal action_v4_8dad927e9ae69ddb5f9f9ea7e66b03fe). Derivation verified:
// keccak256("dexter-credit-root-v4") >> 8 (same as every prior version).
//
// ⚠️ ROTATION IS ONE-WAY. The World-ID nullifier is per (human, action) and the
// CreditRoot PDA is seeded by the nullifier ALONE (no action component), so the
// one-human-one-root sybil bound holds only WITHIN a single action. Rotating the
// action therefore has TWO consequences, both acceptable ONLY because no real
// users exist yet (only Branch's disposable test roots): (1) STRANDING — a root
// established under the old action can never upgrade (its nullifier won't match a
// new-action proof); (2) DUAL-MINT — the same human can establish a SECOND,
// independent root under the new action (a different nullifier → a different PDA
// that inits cleanly), doubling their personhood-gated allocation. There is no
// cross-action uniqueness guard on-chain (nullifiers are cross-action unlinkable
// by design), so the ONLY defense is to never rotate. Once real users tap v4,
// this constant is FROZEN forever. (Confirmed by the 2026-07-05 money-core review
// — LOW severity: operator-gated, non-adversarial, closes before a real user
// population exists.)
#[cfg(not(feature = "fixture-test-context"))]
pub const DEXTER_ACTION: [u8; 32] = [
    0, 247, 176, 148, 158, 187, 138, 148, 120, 229, 193, 50, 157, 120, 181, 128, 228, 37, 220, 122,
    198, 89, 41, 154, 91, 103, 209, 113, 41, 233, 153, 9,
];

/// TEST-ONLY override of `DEXTER_ACTION`. See the production definition above.
/// Active only under the `fixture-test-context` feature (TACEO fixture action).
#[cfg(feature = "fixture-test-context")]
pub const DEXTER_ACTION: [u8; 32] = [
    0u8, 47u8, 243u8, 111u8, 143u8, 18u8, 95u8, 212u8, 144u8, 150u8, 163u8, 64u8, 103u8, 244u8,
    0u8, 204u8, 215u8, 90u8, 219u8, 132u8, 57u8, 33u8, 2u8, 158u8, 180u8, 54u8, 109u8, 50u8, 204u8,
    212u8, 220u8, 135u8,
];

// ──────────────────────────────────────────────────────────────────────────
// World ID v4 CREDENTIAL / NETWORK pins (Sybil + authenticity hardening).
//
// World ID's own on-chain reference verifier (`WorldIDVerifier.sol`) pins these
// public signals from on-chain registries, NOT from caller input. Mirroring
// that here is what makes one-human-one-root actually hold:
//
//   * `oprf_pk` (idx 10/11) — the OPRF network pubkey. The nullifier is
//     `OPRF(oprf_pk, …)`. If caller-supplied, an attacker holding ONE real
//     World ID can substitute their OWN oprf_pk, compute the OPRF response
//     offline, and mint a valid proof for ANY nullifier — unlimited CreditRoots
//     from a single human. Pinning oprf_pk closes that.
//   * `cred_pk` (idx 2/3) — the Orb issuer credential pubkey. If unbound, a
//     self-issued (non-Orb) credential is accepted, defeating personhood.
//   * `issuer_schema_id` (idx 1) — pins WHICH credential schema is accepted.
//   * `depth` (idx 7) — the Merkle tree depth, pinned for shape integrity.
//
// Same gating as DEXTER_RP_ID / DEXTER_ACTION: production values are `[0u8; 32]`
// ONLY because they're not pinned yet. These are WORLD's PUBLIC v4 protocol
// constants (Orb-issuer credential key, OPRF network key) — same for everyone; read
// them from `public_inputs[1,2,3,7,10,11]` of the real proof in
// `/tmp/dexter-worldid-proof.json`. NOT pending registration. The
// `fixture-test-context` overrides are the TACEO
// fixture's own public_inputs[1] / [2] / [3] / [7] / [10] / [11] so the
// committed fixture proof binds in tests. **NEVER enable `fixture-test-context`
// in a production build.**
// ──────────────────────────────────────────────────────────────────────────

/// World ID issuer schema id (π₂ public input idx 1). TBD production placeholder.
#[cfg(not(feature = "fixture-test-context"))]
pub const DEXTER_ISSUER_SCHEMA_ID: [u8; 32] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
];

/// TEST-ONLY override of `DEXTER_ISSUER_SCHEMA_ID` (TACEO fixture public_inputs[1]).
#[cfg(feature = "fixture-test-context")]
pub const DEXTER_ISSUER_SCHEMA_ID: [u8; 32] = [
    0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8,
    0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 1u8,
];

/// Orb issuer credential pubkey, element 0 (π₂ public input idx 2). TBD placeholder.
#[cfg(not(feature = "fixture-test-context"))]
pub const DEXTER_CRED_PK_0: [u8; 32] = [
    27, 11, 44, 210, 106, 102, 64, 179, 241, 101, 73, 42, 143, 208, 119, 161, 140, 169, 6, 159,
    197, 222, 43, 193, 27, 23, 152, 11, 189, 130, 254, 132,
];

/// TEST-ONLY override of `DEXTER_CRED_PK_0` (TACEO fixture public_inputs[2]).
#[cfg(feature = "fixture-test-context")]
pub const DEXTER_CRED_PK_0: [u8; 32] = [
    48u8, 64u8, 160u8, 212u8, 189u8, 142u8, 162u8, 204u8, 189u8, 105u8, 162u8, 31u8, 61u8, 242u8,
    171u8, 40u8, 22u8, 131u8, 100u8, 134u8, 84u8, 120u8, 25u8, 239u8, 38u8, 70u8, 120u8, 192u8,
    7u8, 93u8, 223u8, 26u8,
];

/// Orb issuer credential pubkey, element 1 (π₂ public input idx 3). TBD placeholder.
#[cfg(not(feature = "fixture-test-context"))]
pub const DEXTER_CRED_PK_1: [u8; 32] = [
    7, 116, 40, 197, 206, 136, 205, 88, 233, 163, 8, 134, 1, 215, 219, 44, 70, 170, 160, 226, 42,
    187, 243, 245, 110, 107, 20, 255, 243, 234, 179, 12,
];

/// TEST-ONLY override of `DEXTER_CRED_PK_1` (TACEO fixture public_inputs[3]).
#[cfg(feature = "fixture-test-context")]
pub const DEXTER_CRED_PK_1: [u8; 32] = [
    18u8, 111u8, 251u8, 116u8, 55u8, 198u8, 86u8, 203u8, 172u8, 65u8, 92u8, 42u8, 201u8, 12u8,
    247u8, 12u8, 21u8, 212u8, 254u8, 226u8, 220u8, 51u8, 40u8, 137u8, 251u8, 169u8, 195u8, 40u8,
    12u8, 49u8, 175u8, 196u8,
];

/// World ID Merkle tree depth (π₂ public input idx 7). TBD production placeholder.
#[cfg(not(feature = "fixture-test-context"))]
pub const DEXTER_TREE_DEPTH: [u8; 32] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 30,
];

/// TEST-ONLY override of `DEXTER_TREE_DEPTH` (TACEO fixture public_inputs[7] = 30).
#[cfg(feature = "fixture-test-context")]
pub const DEXTER_TREE_DEPTH: [u8; 32] = [
    0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8,
    0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 30u8,
];

/// OPRF network pubkey, element 0 (π₂ public input idx 10). TBD placeholder.
/// THE critical pin — see the module header above.
#[cfg(not(feature = "fixture-test-context"))]
pub const DEXTER_OPRF_PK_0: [u8; 32] = [
    1, 179, 198, 69, 177, 20, 193, 216, 136, 244, 23, 78, 11, 241, 39, 42, 62, 234, 231, 229, 173,
    180, 16, 209, 62, 121, 128, 90, 7, 209, 217, 14,
];

/// TEST-ONLY override of `DEXTER_OPRF_PK_0` (TACEO fixture public_inputs[10]).
#[cfg(feature = "fixture-test-context")]
pub const DEXTER_OPRF_PK_0: [u8; 32] = [
    35u8, 123u8, 3u8, 144u8, 197u8, 112u8, 57u8, 189u8, 250u8, 209u8, 86u8, 229u8, 218u8, 167u8,
    162u8, 187u8, 4u8, 119u8, 101u8, 167u8, 105u8, 127u8, 225u8, 190u8, 67u8, 55u8, 216u8, 20u8,
    48u8, 209u8, 108u8, 161u8,
];

/// OPRF network pubkey, element 1 (π₂ public input idx 11). TBD placeholder.
/// THE critical pin — see the module header above.
#[cfg(not(feature = "fixture-test-context"))]
pub const DEXTER_OPRF_PK_1: [u8; 32] = [
    36, 145, 143, 212, 3, 39, 7, 113, 201, 50, 104, 133, 201, 18, 199, 31, 160, 5, 235, 116, 196,
    128, 211, 27, 97, 254, 172, 101, 102, 24, 119, 114,
];

/// TEST-ONLY override of `DEXTER_OPRF_PK_1` (TACEO fixture public_inputs[11]).
#[cfg(feature = "fixture-test-context")]
pub const DEXTER_OPRF_PK_1: [u8; 32] = [
    29u8, 189u8, 13u8, 55u8, 66u8, 196u8, 130u8, 166u8, 85u8, 255u8, 222u8, 98u8, 68u8, 238u8,
    187u8, 155u8, 177u8, 110u8, 164u8, 220u8, 155u8, 129u8, 127u8, 66u8, 96u8, 209u8, 70u8, 128u8,
    71u8, 196u8, 88u8, 236u8,
];

// ──────────────────────────────────────────────────────────────────────────
// Recourse-rooted delegation graph (depth-N) — Task 1+ seeds & tunables.
// ──────────────────────────────────────────────────────────────────────────

/// PrincipalNode PDA seed prefix. Per-node: `[PRINCIPAL_NODE_SEED, node_id]`.
pub const PRINCIPAL_NODE_SEED: &[u8] = b"principal";

/// Immutable Line origination receipt PDA seed. Per exposure:
/// `[LINE_ORIGINATION_SEED, exposure_id]`. `exposure_id` is itself derived from
/// the buyer vault, PrincipalNode, and canonical evidence commitment, so the
/// same funded purchase cannot be re-labelled under a second receipt.
pub const LINE_ORIGINATION_SEED: &[u8] = b"line_origination";

/// Domain for the deterministic Line exposure ID:
/// `sha256(domain || vault || node || evidence_hash)`.
pub const LINE_EXPOSURE_ID_DOMAIN: &[u8] = b"dexter-line-origination-v1";

/// GraphConfig singleton PDA seed: `[GRAPH_CONFIG_SEED]`.
pub const GRAPH_CONFIG_SEED: &[u8] = b"graph_config";

/// AssetConfig PDA seed prefix (leveraged-carry approved-asset allowlist,
/// Phase 0). Per-asset: `[ASSET_CONFIG_SEED, base_mint]`. Underscore to match
/// the config family (`graph_config`, `credit_root`, `world_id_root`); the
/// hyphenated form is reserved for foreign-program PDAs like
/// `swig-wallet-address`.
pub const ASSET_CONFIG_SEED: &[u8] = b"asset_config";

/// F (credit consent): per-financier consent record PDA seed.
/// `[FINANCIER_CONFIG_SEED, financier_swig]`. Hyphenated because the seed
/// component is a foreign-program (Swig) account key, matching the
/// `swig-wallet-address` / `swig-vault` convention.
pub const FINANCIER_CONFIG_SEED: &[u8] = b"financier-config";

/// LeveredPosition PDA seed prefix (leveraged-carry engine, Phase 1). One row
/// per position: `[LEVERED_POSITION_SEED, vault]` — 1:1 with the vault/node so
/// a levered position is the isolation unit (Λ=0 per BUILD-SPEC §1.5).
/// Underscore to match the config family; hyphen reserved for foreign PDAs.
pub const LEVERED_POSITION_SEED: &[u8] = b"levered_position";

/// SwapBracket PDA seed prefix. Per-vault: `[SWAP_BRACKET_SEED, vault]`. Its
/// `nonce` is the one permanent replay frontier shared by governed carry Trade
/// and Protected Send; it must never be closed or split into route counters.
pub const SWAP_BRACKET_SEED: &[u8] = b"swap_bracket";

/// Owner-authored governed-trading grant. One permanent row per grant nonce:
/// `[GOVERNED_TRADE_GRANT_SEED, vault, grant_nonce_le]`.
///
/// Rows are never closed. A revoked row therefore remains a durable replay
/// tombstone, while Swig's monotonic role ids prevent a removed authority from
/// being confused with a later grant.
pub const GOVERNED_TRADE_GRANT_SEED: &[u8] = b"trade_grant";

/// Additive catalog-governed trading V2 namespaces. V1 keeps its exact
/// `trade_grant`/`swap_bracket` account and instruction ABI; every V2 account
/// has a disjoint seed so a V1 row can never deserialize or replay as V2.
pub const CATALOG_POLICY_V2_SEED: &[u8] = b"catalog_policy_v2";
pub const GOVERNED_ASSET_IDENTITY_V2_SEED: &[u8] = b"asset_identity_v2";
pub const GOVERNED_ASSET_CURRENT_V2_SEED: &[u8] = b"asset_current_v2";
pub const GOVERNED_TRADE_GRANT_V2_SEED: &[u8] = b"trade_grant_v2";
pub const GOVERNED_ASSET_ROLE_V2_SEED: &[u8] = b"asset_role_v2";
pub const GOVERNED_SELL_METER_V2_SEED: &[u8] = b"sell_meter_v2";
/// Canonical wallet-wide automation outflow meter. One persistent row per
/// Vault, shared by direct agent spending and every governed stock Buy.
/// Sells return value to the settlement asset and never debit this budget.
pub const AUTOMATION_BUDGET_V2_SEED: &[u8] = b"automation_budget_v2";

/// Jupiter v6 aggregator program. The ONLY program a carry-swap SignV2 envelope
/// may wrap (envelope-lite check in verify/swig_transfer.rs). Pinned as a
/// constant for v1; if venue flexibility is ever needed it graduates to a
/// cold-admin-tunable GraphConfig field (single-PDA migration, cheap).
pub const JUPITER_V6_PROGRAM_ID: Pubkey = pubkey!("JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4");

/// Maximum delegation depth (compile-time default). A non-zero
/// `GraphConfig.max_depth_override` wins over this const at runtime.
pub const MAX_DEPTH: u8 = 8;

/// Hard ceiling on `GraphConfig.withdrawal_fee_atomic` ($5 in atomic USDC).
/// Enforced at migrate_graph_config_v2 AND set_withdrawal_fee so no authority,
/// however privileged, can raise the fee into a withdrawal skim.
pub const WITHDRAWAL_FEE_CAP: u64 = 5_000_000;

/// Hard ceiling on `GraphConfig.activation_fee_atomic` ($2 in atomic USDC).
/// Enforced at set_activation_fee; activate_wallet then caps each birth's fee
/// leg by the knob, so no key can turn wallet setup into a deposit skim.
pub const ACTIVATION_FEE_CAP: u64 = 2_000_000;

// ── Line recovery terms ────────────────────────────────────────────────────

/// Recovery windows are signed i64s on the existing wire. Zero and negatives
/// would make a fresh draw immediately seizable (or already overdue), so every
/// new Line draw must carry at least one positive second.
pub const MIN_LINE_RECOVERY_WINDOW_SECONDS: i64 = 1;

/// Protocol ceiling for a funded Line recovery window: 30 days. The current
/// product uses seven days; this conservative ceiling also matches the longest
/// modeled short-duration Line horizon while preventing an authority mistake
/// from pinning buyer collateral for an unbounded signed-i64 interval.
pub const MAX_LINE_RECOVERY_WINDOW_SECONDS: i64 = 30 * 24 * 60 * 60;

// ── Spread engine (docs/superpowers/specs/2026-07-04-spread-engine-design.md) ─

/// Basis-point denominator shared by every bps computation in the engine.
pub const BPS_DENOMINATOR: u64 = 10_000;

/// Accrual year: 365 days of seconds. `rate_bps` is an APR against this.
pub const SECONDS_PER_YEAR: u64 = 31_536_000;

/// Hard ceiling on `PrincipalNode.rate_bps` (100% APR), enforced at EVERY rate
/// write (create_node, set_node_rate, draw_leverage's keeper stamp) so no
/// authority, however privileged, can weaponize the rate knob into a drain —
/// the WITHDRAWAL_FEE_CAP philosophy applied to interest.
pub const RATE_BPS_CAP: u16 = 10_000;

/// Hard ceiling on `GraphConfig.interest_take_bps`: the protocol treasury can
/// never take more than HALF of collected interest; the rest is bound to the
/// financier's ATA on-chain. Enforced at set_interest_take.
pub const INTEREST_TAKE_CAP_BPS: u16 = 5_000;

// ── Penny lock (dexter-fe crystallization-cadence spec 2026-07-06 §2) ───────

/// Hard ceiling on `GraphConfig.max_seller_at_risk_atomic` ($1.00 in atomic
/// USDC). Enforced at `set_max_seller_at_risk` so a compromised cold key can
/// loosen seller abandonment protection to at most a dollar, never disable it
/// — the WITHDRAWAL_FEE_CAP philosophy applied to the crystallization knob.
pub const MAX_SELLER_AT_RISK_CAP: u64 = 1_000_000;

/// What readers substitute when the on-chain knob reads 0 (the zeroed-reserved
/// state every live GraphConfig deploys with, and the fresh-init value): $0.01
/// — the 2026-07-06 penny ruling. The PROGRAM never reads this itself (lock
/// timing is facilitator-owned by design); it is pinned here so the on-chain
/// repo carries the authoritative zero-semantics next to the cap.
pub const MAX_SELLER_AT_RISK_DEFAULT: u64 = 10_000;

/// Freshness bound on the client-supplied `accrual_ts` that settlement paths
/// (repay/seize/settle_liquidation) compute interest against. Wide enough for
/// tx build→land latency, tight enough that a quote cannot be replayed against
/// meaningfully-grown debt. Mirrors draw_leverage's attested_at pattern.
pub const ACCRUAL_TS_MAX_STALENESS_SECS: i64 = 300;

// ── Swig duration conversions (E: time units) ───────────────────────────────
//
// Swig stores session lifetimes and recurring windows in SLOTS; every Dexter
// caller authors durations in SECONDS. There is exactly ONE conversion pair, so
// a cap and a guarantee are never computed with ad-hoc factors. The old ×5/2
// copies assumed 0.4 s/slot and made every converted duration last ~54% of its
// label at the measured ~0.217 s/slot (a "30 day" session lived ~16 days —
// the exact-spend session-death class). Swig's session-expiry add is UNCHECKED
// (swig-wallet state/src/authority/ed25519.rs), so both helpers clamp seconds
// to MAX_SWIG_SECONDS before multiplying — a near-u64 duration can never wrap.
pub const MAX_SWIG_SECONDS: u64 = 10 * 31_536_000; // 10 years

/// A CAP (how long a session MAY live): round DOWN at 250 ms/slot (4 slots/s).
/// The result never exceeds the labelled seconds while slots stay ≤ 250 ms, so
/// a converted session can only ever be SHORTER than its label, never longer.
pub const fn swig_cap_slots(seconds: u64) -> u64 {
    let s = if seconds > MAX_SWIG_SECONDS {
        MAX_SWIG_SECONDS
    } else {
        seconds
    };
    s.saturating_mul(4)
}

/// A MINIMUM INTERVAL (how soon a budget MAY refill): round UP at 150 ms/slot
/// (20/3 slots/s). The result never elapses before the labelled seconds while
/// slots stay ≥ 150 ms, so a budget can only ever refill SLOWER than its label,
/// never faster.
pub const fn swig_guarantee_slots(seconds: u64) -> u64 {
    let s = if seconds > MAX_SWIG_SECONDS {
        MAX_SWIG_SECONDS
    } else {
        seconds
    };
    s.saturating_mul(20).div_ceil(3)
}

/// TRANSITIONAL — remove once the SDK ships the ×4 helper and dexter-api adopts
/// it. The legacy ×5/2 (0.4 s/slot) encoding SDK 0.43.8 `agentSessionSecondsToSlots`
/// still emits. Clamped to MAX_SWIG_SECONDS like the canonical helpers, so it can
/// never hand Swig's unchecked expiry add a wrapping value. Accepted ONLY by
/// `grant_agent_session`, for one release, alongside the canonical `swig_cap_slots`.
pub const fn legacy_x5_2_slots(seconds: u64) -> u64 {
    let s = if seconds > MAX_SWIG_SECONDS {
        MAX_SWIG_SECONDS
    } else {
        seconds
    };
    s.saturating_mul(5) / 2
}

#[cfg(test)]
mod swig_duration_tests {
    use super::*;

    #[test]
    fn cap_rounds_down_and_never_exceeds_label() {
        // 30 days @ 250 ms/slot.
        assert_eq!(swig_cap_slots(30 * 24 * 60 * 60), 10_368_000);
        assert_eq!(swig_cap_slots(1), 4);
        assert_eq!(swig_cap_slots(0), 0);
    }

    #[test]
    fn legacy_x5_2_matches_the_sdk_encoding_and_is_clamped() {
        // SDK 0.43.8 agentSessionSecondsToSlots = seconds × 5 / 2. For every
        // realistic TTL (≤ MAX_SWIG_SECONDS) the clamp is a no-op, so the bytes a
        // current-SDK grant carries match exactly.
        for s in [60u64, 3_600, 86_400, 30 * 86_400, 365 * 86_400] {
            assert_eq!(legacy_x5_2_slots(s), s * 5 / 2, "s={s}");
        }
        // 1-year agent session: 31,536,000 s → 78,840,000 slots (legacy) vs
        // 126,144,000 slots (canonical ×4) — two distinct accepted encodings.
        assert_eq!(legacy_x5_2_slots(31_536_000), 78_840_000);
        assert_eq!(swig_cap_slots(31_536_000), 126_144_000);
        // Near-u64 TTLs clamp, so the legacy arm can never wrap Swig's add.
        assert_eq!(
            legacy_x5_2_slots(u64::MAX),
            MAX_SWIG_SECONDS.saturating_mul(5) / 2
        );
    }

    #[test]
    fn guarantee_rounds_up_and_refills_no_faster_than_label() {
        // 30 days @ 150 ms/slot, rounded up.
        assert_eq!(swig_guarantee_slots(30 * 24 * 60 * 60), 17_280_000);
        // Round-UP behaviour: 1 s → ceil(20/3) = 7 slots.
        assert_eq!(swig_guarantee_slots(1), 7);
        assert_eq!(swig_guarantee_slots(0), 0);
    }

    #[test]
    fn both_helpers_clamp_near_u64_without_wrapping() {
        // Swig's expiry add is unchecked — never hand it a wrapping value.
        assert_eq!(swig_cap_slots(u64::MAX), MAX_SWIG_SECONDS.saturating_mul(4));
        assert_eq!(
            swig_guarantee_slots(u64::MAX),
            MAX_SWIG_SECONDS.saturating_mul(20).div_ceil(3)
        );
        assert!(swig_cap_slots(MAX_SWIG_SECONDS) < u64::MAX);
        assert!(swig_guarantee_slots(MAX_SWIG_SECONDS) < u64::MAX);
    }

    #[test]
    fn guarantee_is_always_at_least_the_cap() {
        for s in [1u64, 60, 3600, 86_400, 30 * 86_400, 365 * 86_400] {
            assert!(swig_guarantee_slots(s) >= swig_cap_slots(s), "s={s}");
        }
    }
}
