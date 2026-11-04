//! Grant the agent-spend session AFTER birth — the consent-at-grant ceremony
//! (board #119, Car 3 half 2; spec 2026-07-25-CAR3-owner-withdrawal-decouple).
//!
//! Mode-1 (owner-only) births create NO agent session: the wallet can receive
//! and withdraw from day one, and the spending allowance is authored the first
//! time the owner actually hands an agent spending power. This instruction is
//! that moment, enforced ON-CHAIN: the owner's passkey signs the exact
//! allowance, and the swig-side authority that gets minted is verified
//! byte-for-byte before the GRANT role is allowed to co-author it.
//!
//! Transaction shape (two top-level instructions, atomic, ORDER LAW):
//!   [N]   vault::grant_agent_session   ← this instruction
//!           verifies the owner passkey over (dexter_master, limit, ttl),
//!           then FORWARD-introspects instruction N+1 (the authority leg)
//!   [N+1] swig::AddAuthorityV1 acting as the GRANT role
//!           swig's ProgramExec authenticate BACKWARD-introspects N: program
//!           id must be this vault program, data must start with THIS
//!           instruction's discriminator (the GRANT marker), and N's account
//!           metas [0]/[1] must equal AddAuthorityV1's accounts [0]/[1]
//!           (swig config, payer) — which pins THIS accounts struct's order.
//!
//! The two introspections point at each other; neither instruction can run
//! without the other in the exact adjacent position. The GRANT role holds
//! ManageAuthority ONLY, so the sole thing it can ever do is what this bind
//! permits: mint one agent-spend session authority in the birth shape.
//!
//! AUTHORITY-LEG BIND (the money-leg-bind mirror): the following
//! AddAuthorityV1 must be EXACTLY the birth role-2 shape —
//! Ed25519Session(dexter_master, zero session key, ttl in SLOTS) with actions
//! [TokenLimit(USDC, limit), Program(dexter-vault)] and nothing else, no
//! trailing bytes.
//! Wire receipts: docs/superpowers/specs/2026-07-25-CAR3-ground-facts.json.
//! num_actions/padding/boundary bytes are NOT validated by swig (count is
//! recomputed, boundaries rewritten) — so this bind length-walks the actions
//! blob and ignores exactly those untrusted bytes, nothing else.

use anchor_lang::prelude::*;
use anchor_lang::solana_program::sysvar;
use anchor_lang::solana_program::sysvar::instructions::{
    load_current_index_checked, load_instruction_at_checked,
};

use crate::constants::{legacy_x5_2_slots, swig_cap_slots, SWIG_PROGRAM_ID};
use crate::events::AgentSessionGranted;
use crate::state::*;
use crate::verify::swig_transfer::require_swig_ixs_marker_bound;
use crate::verify::webauthn::verify_and_consume_passkey_authorization;

/// Domain separator for agent-grant messages. Distinct from every other
/// ceremony domain so a grant signature can never be reinterpreted as a
/// registration/revocation (or vice versa). 18 bytes label + 14 NUL = 32.
const GRANT_DOMAIN: &[u8; 32] = b"OTS_AGENT_GRANT_V1\0\0\0\0\0\0\0\0\0\0\0\0\0\0";

/// swig AddAuthorityV1 wire constants (rev c2e8eb4 receipts in the ground-facts
/// doc). Total data length is EXACT: args(16) + authority(72) + actions(88) +
/// acting payload(1 byte — the sysvar account index for a ProgramExec acting
/// role). Anything longer is a smuggle; anything shorter can't authenticate.
const ADD_AUTHORITY_V1_DISCRIMINATOR: u16 = 1;
const AUTHORITY_TYPE_ED25519_SESSION: u16 = 2;
const ED25519_SESSION_CREATE_LEN_U16: u16 = 72;
const ACTIONS_BLOB_LEN_U16: u16 = 88; // TokenLimit (8+40) + Program (8+32)
const EXPECTED_ADD_AUTHORITY_DATA_LEN: usize = 16 + 72 + 88 + 1;
const ACTION_TYPE_TOKEN_LIMIT: u16 = 5;
const ACTION_TYPE_PROGRAM: u16 = 3;

#[derive(Accounts)]
pub struct GrantAgentSession<'info> {
    /// Position 0 — LAW: swig's ProgramExec authenticate requires the
    /// preceding instruction's account meta[0] to equal AddAuthorityV1's
    /// account[0] (the swig config account).
    /// CHECK: address constraint binds to vault.swig_address; never deref.
    #[account(address = vault.swig_address)]
    pub swig: AccountInfo<'info>,

    /// Position 1 — LAW: swig requires meta[1] here to equal AddAuthorityV1's
    /// account[1], which for AddAuthorityV1 is the PAYER (unlike SignV2 where
    /// it is the swig wallet address). Must therefore be the same signer that
    /// pays the following AddAuthorityV1.
    #[account(mut)]
    pub fee_payer: Signer<'info>,

    /// The vault whose owner is granting agent power. Read-only: the stored
    /// passkey is the verification root; no vault state changes here.
    pub vault: Account<'info, Vault>,

    #[account(
        mut,
        seeds = [crate::constants::PASSKEY_AUTHORIZATION_SEED, vault.key().as_ref()],
        bump = passkey_authorization.bump,
    )]
    pub passkey_authorization: Account<'info, PasskeyAuthorization>,

    /// CHECK: instructions sysvar — address-constrained. The PRECEDING
    /// instruction must be the secp256r1 precompile (passkey proof); the
    /// FOLLOWING instruction is the AddAuthorityV1 this handler binds.
    #[account(address = sysvar::instructions::ID)]
    pub instructions_sysvar: AccountInfo<'info>,

    /// Graph admin spine — the canonical mint the TokenLimit must name.
    #[account(seeds = [crate::constants::GRAPH_CONFIG_SEED], bump = graph_config.bump)]
    pub graph_config: Account<'info, GraphConfig>,
}

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct GrantAgentSessionArgs {
    /// Becomes the role's identity key (the dexter master, same as mode-0
    /// birth). Bound into the passkey message so a signature can't be
    /// replayed onto a different key.
    pub dexter_master_pubkey: Pubkey,
    /// USER-AUTHORED allowance in atomic USDC. Zero REJECTS — no default,
    /// same law as birth (consent ruling 2026-07-02/03).
    pub spend_limit_atomic: u64,
    /// USER-AUTHORED session TTL in SECONDS. Zero REJECTS. Converted to
    /// slots ONLY at the swig boundary (the 1ce2102 units law).
    pub session_ttl_seconds: u64,
    /// WebAuthn ceremony bytes (challenge = sha256(grant message)).
    pub client_data_json: Vec<u8>,
    pub authenticator_data: Vec<u8>,
}

/// domain || program_id || vault_pda || dexter_master || limit LE || ttl LE.
/// Every scope arg is inside the signed bytes — a grant signature cannot be
/// replayed with a different key, amount, or duration. 144 bytes.
fn build_grant_message(
    program_id: &Pubkey,
    vault_pda: &Pubkey,
    dexter_master: &Pubkey,
    spend_limit_atomic: u64,
    session_ttl_seconds: u64,
) -> Vec<u8> {
    let mut msg = Vec::with_capacity(144);
    msg.extend_from_slice(GRANT_DOMAIN);
    msg.extend_from_slice(program_id.as_ref());
    msg.extend_from_slice(vault_pda.as_ref());
    msg.extend_from_slice(dexter_master.as_ref());
    msg.extend_from_slice(&spend_limit_atomic.to_le_bytes());
    msg.extend_from_slice(&session_ttl_seconds.to_le_bytes());
    debug_assert_eq!(msg.len(), 144);
    msg
}

pub fn handler(ctx: Context<GrantAgentSession>, args: GrantAgentSessionArgs) -> Result<()> {
    let vault = &ctx.accounts.vault;

    require!(
        vault.version == VAULT_VERSION_V7,
        VaultError::UnsupportedVaultVersion
    );
    // The swig account constraint already binds to vault.swig_address; this
    // guards the never-deployed case where that field is still the default.
    require!(
        vault.swig_address != Pubkey::default(),
        VaultError::PasskeyVerificationFailed
    );
    // Consent law: the user authors both numbers, no fallback, zero rejects.
    require!(args.spend_limit_atomic > 0, VaultError::SpendLimitRequired);
    require!(args.session_ttl_seconds > 0, VaultError::SessionTtlRequired);

    // ── OWNER PASSKEY ─────────────────────────────────────────────────────
    let vault_key = vault.key();
    let passkey = vault.passkey_pubkey;
    let grant_message = build_grant_message(
        ctx.program_id,
        &vault_key,
        &args.dexter_master_pubkey,
        args.spend_limit_atomic,
        args.session_ttl_seconds,
    );
    verify_and_consume_passkey_authorization(
        &ctx.accounts.instructions_sysvar,
        &passkey,
        &mut ctx.accounts.passkey_authorization,
        &vault_key,
        &args.client_data_json,
        &args.authenticator_data,
        &grant_message,
    )?;

    // ── AUTHORITY-LEG BIND ────────────────────────────────────────────────
    let current_index = load_current_index_checked(&ctx.accounts.instructions_sysvar)? as usize;
    let next_ix = load_instruction_at_checked(current_index + 1, &ctx.accounts.instructions_sysvar)
        .map_err(|_| error!(VaultError::GrantEnvelopeMismatch))?;
    require!(
        next_ix.program_id == SWIG_PROGRAM_ID,
        VaultError::GrantEnvelopeMismatch
    );
    // The add must target THIS vault's swig config account, not some other
    // wallet the same payer controls.
    require!(
        !next_ix.accounts.is_empty() && next_ix.accounts[0].pubkey == ctx.accounts.swig.key(),
        VaultError::GrantEnvelopeMismatch
    );
    // P5: the AddAuthorityV1 is bound at current+1 on this vault's swig. Reject
    // any other Swig instruction on this swig in the tx so a second Swig op
    // cannot back-reference this grant marker (approval-reuse defense).
    require_swig_ixs_marker_bound(
        &ctx.accounts.instructions_sysvar,
        &ctx.accounts.swig.key(),
    )?;

    let d = next_ix.data.as_slice();
    require!(
        d.len() == EXPECTED_ADD_AUTHORITY_DATA_LEN,
        VaultError::GrantEnvelopeMismatch
    );
    let u16_at = |off: usize| u16::from_le_bytes([d[off], d[off + 1]]);
    require!(
        u16_at(0) == ADD_AUTHORITY_V1_DISCRIMINATOR
            && u16_at(2) == ED25519_SESSION_CREATE_LEN_U16
            && u16_at(4) == ACTIONS_BLOB_LEN_U16
            && u16_at(6) == AUTHORITY_TYPE_ED25519_SESSION,
        VaultError::GrantEnvelopeMismatch
    );
    // bytes 8..12 (num_actions + padding) are NOT swig-validated — ignored
    // here too, per the ground-facts doc. acting_role_id (12..16) is swig's
    // to authenticate: whoever acts must still pass ManageAuthority + its own
    // authenticate; the only self-serve path through that gate is the GRANT
    // marker role, whose marker is THIS instruction.

    // Authority create bytes (16..88): dexter_master || zero session key ||
    // max_session_length in SLOTS. This is a session CAP, so the canonical
    // encoding rounds DOWN via the single `swig_cap_slots` helper (E) —
    // identical to birth; the old ×5/2 lived ~54% of its label at ~0.217 s/slot.
    //
    // TRANSITIONAL (one release): also accept the legacy ×5/2 encoding, so a
    // grant built by SDK 0.43.8 `agentSessionSecondsToSlots` (via dexter-api
    // vaultGrants.ts) still validates on upgrade day without a same-day SDK
    // release. Both are bounded by the MAX_SWIG_SECONDS clamp, so neither can
    // hand Swig's UNCHECKED expiry add a wrapping value. Drop the legacy arm
    // once the SDK ships and dexter-api adopts the ×4 helper.
    let ttl_slots = &d[80..88];
    let canonical = swig_cap_slots(args.session_ttl_seconds);
    let legacy = legacy_x5_2_slots(args.session_ttl_seconds);
    require!(
        d[16..48] == args.dexter_master_pubkey.to_bytes()
            && d[48..80] == [0u8; 32]
            && (ttl_slots == canonical.to_le_bytes() || ttl_slots == legacy.to_le_bytes()),
        VaultError::GrantEnvelopeMismatch
    );

    // Actions blob (88..176), length-walked: TokenLimit(USDC, limit) then
    // Program(dexter-vault) — the exact terminal birth role-2 shape. Boundary
    // u32s (92..96, 140..144) are client-advisory and program-rewritten:
    // ignored.
    let usdc_mint = ctx.accounts.graph_config.usdc_mint;
    require!(
        u16_at(88) == ACTION_TYPE_TOKEN_LIMIT
            && u16_at(90) == 40
            && d[96..128] == usdc_mint.to_bytes()
            && d[128..136] == args.spend_limit_atomic.to_le_bytes()
            && u16_at(136) == ACTION_TYPE_PROGRAM
            && u16_at(138) == 32
            && d[144..176] == crate::ID.to_bytes(),
        VaultError::GrantEnvelopeMismatch
    );

    emit!(AgentSessionGranted {
        vault: vault_key,
        dexter_master: args.dexter_master_pubkey,
        spend_limit_atomic: args.spend_limit_atomic,
        session_ttl_seconds: args.session_ttl_seconds,
    });

    Ok(())
}
