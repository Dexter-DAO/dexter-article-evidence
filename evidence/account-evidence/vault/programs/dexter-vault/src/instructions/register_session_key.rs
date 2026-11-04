use anchor_lang::prelude::*;
use anchor_lang::solana_program::sysvar;
use anchor_spl::token::TokenAccount;

use crate::constants::{
    GRAPH_CONFIG_SEED, PASSKEY_AUTHORIZATION_SEED, SWIG_PROGRAM_ID, SWIG_VAULT_BINDING_SEED,
    SWIG_WALLET_ADDRESS_SEED,
};
use crate::instructions::session_admission::{
    enforce_session_admission, reconciled_live_session_count, SessionAdmissionParams,
};
use crate::state::*;
use crate::verify::ed25519::require_nonweak_ed25519_public_key;
use crate::verify::reserved_cash::{verify_canonical_reserved_cash, verify_swig_vault_identity};
use crate::verify::voucher::{require_bounded_session_expiry, require_fresh_v2_registration_nonce};
use crate::verify::webauthn::verify_and_consume_passkey_authorization;

/// Domain separator for session-key REGISTRATION messages. Padded to 32 bytes
/// with NULs so the serialized message stays byte-aligned and the seller can
/// reconstruct it deterministically. Any change to this literal MUST bump the
/// version in the suffix — older sessions cannot be re-used by a newer client.
///
/// Layout note: 23 bytes of label + 9 NUL = 32 bytes total.
const REGISTER_DOMAIN: &[u8; 32] = b"OTS_SESSION_REGISTER_V2\0\0\0\0\0\0\0\0\0";

#[derive(Accounts)]
#[instruction(args: RegisterSessionKeyArgs)]
pub struct RegisterSessionKey<'info> {
    /// Receives the new `active_session`. Mutated, no signer required: the
    /// passkey signature embedded in the args (verified via the SIMD-0075
    /// precompile sibling) is what authorizes the mutation.
    #[account(mut)]
    pub vault: Account<'info, Vault>,

    #[account(
        mut,
        seeds = [PASSKEY_AUTHORIZATION_SEED, vault.key().as_ref()],
        bump = passkey_authorization.bump,
    )]
    pub passkey_authorization: Account<'info, PasskeyAuthorization>,

    /// The swig wallet's USDC ATA — read live to enforce the overcommit
    /// invariant per V0.3 Decision 1: the new session's max_amount plus
    /// the existing outstanding_locked_amount must fit within actual USDC
    /// balance. When present, the ATA's `owner` field is verified against the
    /// canonical swig wallet PDA.
    ///
    /// OPTIONAL: a $0 credit-only vault never receives a deposit, so its USDC
    /// ATA is never created (an ATA is born on first incoming transfer). Such a
    /// vault is backed entirely by its standby credit line, not its own USDC.
    /// When the account is absent (None — the program-ID sentinel), own-USDC is
    /// treated as 0 and backing is the available standby credit alone. Omitting
    /// it can only ever UNDERSTATE backing (own-USDC defaults to 0), so it is
    /// safe: a self-funded vault that needs its balance counted MUST pass the
    /// ATA, and omission only ever yields a smaller permitted session.
    pub vault_usdc_ata: Option<Account<'info, TokenAccount>>,

    /// CHECK: address constraint binds to vault.swig_address.
    #[account(address = vault.swig_address)]
    pub swig: AccountInfo<'info>,

    /// CHECK: PDA constraint validates derivation.
    #[account(
        seeds = [SWIG_WALLET_ADDRESS_SEED, swig.key().as_ref()],
        bump,
        seeds::program = SWIG_PROGRAM_ID,
    )]
    pub swig_wallet_address: AccountInfo<'info>,

    /// CHECK: instructions sysvar — address-constrained. The previous
    /// instruction in the transaction MUST be a secp256r1_verify call whose
    /// signed message is `authenticatorData || sha256(clientDataJSON)` and
    /// whose structured challenge binds the registration message hash.
    #[account(address = sysvar::instructions::ID)]
    pub instructions_sysvar: AccountInfo<'info>,

    /// The per-counterparty session PDA. init_if_needed: created on the first
    /// register to this counterparty, OVERWRITTEN (replace-in-place) on a
    /// subsequent register ONLY when the existing record is cleared
    /// (version==0) or expired — a LIVE session rejects with
    /// `SessionAlreadyActive` (§6a guard, 2026-07-06). The seed binds it to
    /// (vault, allowed_counterparty), so it cannot be redirected. Re-init
    /// safety (SOL-010): the handler fully overwrites every passkey-endorsed
    /// scope field (Task 5).
    #[account(
        init_if_needed,
        payer = payer,
        space = 8 + SessionAccount::INIT_SPACE,
        seeds = [crate::constants::SESSION_SEED, vault.key().as_ref(), args.allowed_counterparty.as_ref()],
        bump,
    )]
    pub session: Account<'info, SessionAccount>,

    /// Funds the session PDA rent on first creation.
    #[account(mut)]
    pub payer: Signer<'info>,

    pub system_program: Program<'info, System>,

    /// Reverse uniqueness proof for the cash ledger used by the admission gate.
    #[account(
        seeds = [SWIG_VAULT_BINDING_SEED, swig.key().as_ref()],
        bump = swig_vault_binding.bump,
    )]
    pub swig_vault_binding: Box<Account<'info, SwigVaultBinding>>,

    /// Pins an optional cash account to the canonical settlement mint.
    #[account(seeds = [GRAPH_CONFIG_SEED], bump = graph_config.bump)]
    pub graph_config: Box<Account<'info, GraphConfig>>,
    // remaining_accounts: ALL OTHER sibling SessionAccounts (live AND expired),
    // strict ascending order — consumed by the overcommit gate in the handler.
    // Expired siblings MUST be writable (the gate sweeps them: version→0 +
    // zeroed scope, persisted via Account::exit); live siblings may be
    // read-only, though passing everything writable (as the SDK does) is fine.
    // PLUS (optional, after every sibling): the complete read-only PrincipalNode
    // chain welded leaf→...→root. Every hop is authenticated under GraphConfig's
    // max depth and the minimum pro-forma drawable amount counts as backing.
    // Omitting the entire chain keeps own-USDC-only.
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct RegisterSessionKeyArgs {
    /// Ed25519 pubkey the buyer's SDK generated in memory. The passkey is
    /// endorsing this exact key — only this key can sign vouchers for the
    /// duration of the session.
    pub session_pubkey: [u8; 32],
    /// Cumulative cap in atomic units. The seller's middleware AND any future
    /// on-chain consumer of `active_session.spent` enforces this.
    pub max_amount: u64,
    /// Wall-clock expiry, unix seconds. Must be strictly in the future.
    pub expires_at: i64,
    /// The seller this session is bound to. Any voucher claiming a different
    /// counterparty MUST be rejected by the seller's verification path.
    pub allowed_counterparty: Pubkey,
    /// Per-session generation. For post-upgrade registrations this must equal
    /// `0x80000000 | passkey_authorization.nonce`, making every V2 generation
    /// unique under the vault's permanent monotonic authorization guard.
    pub nonce: u32,
    /// Cap the revolving meter (`current_outstanding`) is checked against.
    pub max_revolving_capacity: u64,
    /// WebAuthn `clientDataJSON`. Its `challenge` field must base64url-decode
    /// to sha256(registration_message).
    pub client_data_json: Vec<u8>,
    /// WebAuthn `authenticatorData` (37+ bytes).
    pub authenticator_data: Vec<u8>,
}

/// Authorize a session key for off-chain voucher signing.
///
/// One biometric (or hardware-key) prompt at the start of a tab; the session
/// key then signs every voucher during the stream without further prompts. The
/// program never observes voucher traffic — that all happens off-chain between
/// buyer and seller. This instruction's only job is to make the passkey's
/// endorsement of the session pubkey + scope binding part of vault state, so
/// the seller (and any future on-chain consumer) can verify the endorsement
/// deterministically.
///
/// Safety:
///  - The passkey signature is what authorizes mutation. The accounts struct
///    requires no signer; the precompile sibling does the work.
///  - `max_amount` zero is rejected (a meaningless session).
///  - `expires_at` in the past is rejected (a born-dead session).
///  - An existing unexpired session blocks a new one (`SessionAlreadyActive`).
///    Callers rotate it with `replace_session_key_v1`, which preserves every
///    accepted meter under one assertion. Expired sessions may be reused.
pub fn handler<'info>(
    ctx: Context<'_, '_, 'info, 'info, RegisterSessionKey<'info>>,
    args: RegisterSessionKeyArgs,
) -> Result<()> {
    // ── A. expiry + max_amount / revolving sanity ───────────────────────────
    require!(
        ctx.accounts.vault.version == VAULT_VERSION_V7,
        VaultError::UnsupportedVaultVersion
    );
    require!(
        !legacy_v6_resolution_pending(ctx.accounts.vault.live_session_count),
        VaultError::LegacyV1TabBindingMismatch
    );
    require_nonweak_ed25519_public_key(&args.session_pubkey)?;
    require!(args.max_amount > 0, VaultError::SessionCapZero);
    require!(
        args.max_revolving_capacity > 0,
        VaultError::RevolvingCapacityZero
    );
    require!(
        args.max_revolving_capacity <= args.max_amount,
        VaultError::RevolvingCapacityExceeded
    );
    // A fresh live census found no active nonce with bit 31 set.  Reserve that
    // collision-free marker for the context-bound voucher V2 wire.  Existing
    // low-bit accounts remain settleable, but can never be minted again.
    require_fresh_v2_registration_nonce(args.nonce, ctx.accounts.passkey_authorization.nonce)?;

    let now = Clock::get()?.unix_timestamp;
    require!(args.expires_at > now, VaultError::SessionExpiryInPast);
    require_bounded_session_expiry(args.expires_at)?;

    // ── B. complete Swig→Vault→cash identity ─────────────────────────────────
    // A same-owner secondary or junk-mint account must not inflate admission
    // backing. When the canonical ATA is genuinely absent, prove the reverse
    // Swig→Vault identity and count own cash as zero.
    if let Some(ata) = ctx.accounts.vault_usdc_ata.as_ref() {
        verify_canonical_reserved_cash(
            ctx.program_id,
            &ctx.accounts.swig_vault_binding.key(),
            &ctx.accounts.swig_vault_binding,
            &ctx.accounts.vault.key(),
            &ctx.accounts.vault,
            &ctx.accounts.swig.key(),
            &ctx.accounts.swig_wallet_address.key(),
            &ata.key(),
            &ata.owner,
            &ata.mint,
            ata.amount,
            &ctx.accounts.graph_config.usdc_mint,
        )?;
    } else {
        verify_swig_vault_identity(
            ctx.program_id,
            &ctx.accounts.swig_vault_binding.key(),
            &ctx.accounts.swig_vault_binding,
            &ctx.accounts.vault.key(),
            &ctx.accounts.vault,
            &ctx.accounts.swig.key(),
            &ctx.accounts.swig_wallet_address.key(),
        )?;
    }

    // ── C. The overcommit gate (V6 SECURITY CORE) ────────────────────────────
    // Copy the scalars the verify step needs before taking `&mut vault`, and
    // capture the immutable reads the gate needs.
    let vault_key = ctx.accounts.vault.key();
    let passkey = ctx.accounts.vault.passkey_pubkey;
    let live_session_count = ctx.accounts.vault.live_session_count;
    let outstanding_locked_amount = ctx.accounts.vault.outstanding_locked_amount;
    // Absent ATA (credit-only vault) ⟹ own-USDC is 0; backing is standby alone.
    let ata_amount = ctx
        .accounts
        .vault_usdc_ata
        .as_ref()
        .map(|a| a.amount)
        .unwrap_or(0);
    let vault_node = ctx.accounts.vault.node;

    let session_key = ctx.accounts.session.key();
    let is_new = ctx.accounts.session.version == 0;

    // ── §6a GUARD (crystallization-cadence spec 2026-07-06) ─────────────────
    // A LIVE (unexpired) target session cannot be silently overwritten. The
    // doc above always promised `SessionAlreadyActive`; this enforces it
    // (mirror of migrate_v5_to_v6.rs's live-session reject). Unaffected paths:
    //   - first register (version==0 ⇒ is_new)                     → passes
    //   - post-revoke re-register (revoke CLEARs version to 0)     → passes
    //   - a deliberate revoke followed by a later new registration sees the
    //     cleared PDA as version==0                               → passes
    //   - expired-target rotation (expires_at <= now)              → passes
    // Only overwrite-of-a-LIVE-session dies — the re-register tail-strand
    // path (a unilateral buyer zeroing the seller's un-crystallized meter
    // with only a passkey) is closed. Placed before the sibling loop so the
    // reject is cheap (no CU burned walking siblings for a doomed register).
    require!(
        is_new || ctx.accounts.session.session.expires_at <= now,
        VaultError::SessionAlreadyActive
    );
    if !is_new {
        validate_expired_registration_target(&ctx.accounts.session, &vault_key, &args, now)?;
    }

    // Shared admission gate: complete sibling set, canonical sibling PDAs,
    // expiry sweeping, welded-node backing, and overcommit protection.
    let admission = enforce_session_admission(
        ctx.program_id,
        ctx.remaining_accounts,
        SessionAdmissionParams {
            vault_key,
            vault_node,
            target_session_key: session_key,
            target_already_counted: !is_new,
            live_session_count,
            outstanding_locked_amount,
            own_usdc_amount: ata_amount,
            credit_enabled: !ctx.accounts.graph_config.paused,
            max_depth: {
                let configured = ctx.accounts.graph_config.max_depth_override;
                if configured == 0 {
                    crate::constants::MAX_DEPTH
                } else {
                    configured
                }
            },
            now,
        },
    )?;

    // ── D. passkey verify (KEPT) ─────────────────────────────────────────────
    // Reconstruct the 188-byte registration message the passkey signed.
    let registration_message = build_registration_message(ctx.program_id, &vault_key, &args);

    verify_and_consume_passkey_authorization(
        &ctx.accounts.instructions_sysvar,
        &passkey,
        &mut ctx.accounts.passkey_authorization,
        &vault_key,
        &args.client_data_json,
        &args.authenticator_data,
        &registration_message,
    )?;

    // ── E. count re-sync + first-touch + full-overwrite write ─────────────────
    ctx.accounts.vault.live_session_count = reconciled_live_session_count(
        ctx.accounts.vault.live_session_count,
        admission.swept,
        is_new,
    )?;
    let session = &mut ctx.accounts.session;
    if is_new {
        session.version = SESSION_VERSION_V1;
        session.bump = ctx.bumps.session;
        session.vault = vault_key;
    }
    // FULL overwrite of every passkey-endorsed scope field + reset historical
    // meters. New sessions are first-touch. Existing targets have separately
    // proven expiry, zero outstanding exposure, and a rotated signing key.
    session.session = registration_from_args(&args);

    Ok(())
}

fn validate_expired_registration_target(
    session: &SessionAccount,
    vault: &Pubkey,
    args: &RegisterSessionKeyArgs,
    now: i64,
) -> Result<()> {
    require!(
        session.version == SESSION_VERSION_V1
            && session.vault == *vault
            && session.session.allowed_counterparty == args.allowed_counterparty,
        VaultError::SessionAccountForeign
    );
    require!(
        session.session.expires_at <= now,
        VaultError::SessionAlreadyActive
    );
    require!(
        session.session.current_outstanding == 0,
        VaultError::SessionReplacementOutstanding
    );
    require!(
        session.session.session_pubkey != args.session_pubkey,
        VaultError::SessionReplacementKeyUnchanged
    );
    Ok(())
}

fn registration_from_args(args: &RegisterSessionKeyArgs) -> SessionRegistration {
    SessionRegistration {
        session_pubkey: args.session_pubkey,
        max_amount: args.max_amount,
        expires_at: args.expires_at,
        allowed_counterparty: args.allowed_counterparty,
        nonce: args.nonce,
        spent: 0,
        current_outstanding: 0,
        max_revolving_capacity: args.max_revolving_capacity,
        crystallized_cumulative: 0,
        last_locked_sequence: 0,
    }
}

/// Deterministic 188-byte serialization of the registration message.
///
/// Layout (offsets, little-endian for integers):
///   [  0..32) domain separator literal "OTS_SESSION_REGISTER_V2\0\0\0\0\0\0\0\0\0"
///   [ 32..64) program ID
///   [ 64..96) vault PDA
///   [ 96..128) session_pubkey
///   [128..136) max_amount (u64 LE)
///   [136..144) expires_at (i64 LE)
///   [144..176) allowed_counterparty
///   [176..180) nonce (u32 LE)
///   [180..188) max_revolving_capacity (u64 LE)
///
/// Total: 188 bytes. The seller computes this same byte sequence locally and
/// uses it to verify the registration's WebAuthn ceremony off-chain (one-time
/// per session, cached for the duration).
fn build_registration_message(
    program_id: &Pubkey,
    vault_pda: &Pubkey,
    args: &RegisterSessionKeyArgs,
) -> Vec<u8> {
    let mut msg = Vec::with_capacity(188);
    msg.extend_from_slice(REGISTER_DOMAIN);
    msg.extend_from_slice(program_id.as_ref());
    msg.extend_from_slice(vault_pda.as_ref());
    msg.extend_from_slice(&args.session_pubkey);
    msg.extend_from_slice(&args.max_amount.to_le_bytes());
    msg.extend_from_slice(&args.expires_at.to_le_bytes());
    msg.extend_from_slice(args.allowed_counterparty.as_ref());
    msg.extend_from_slice(&args.nonce.to_le_bytes());
    msg.extend_from_slice(&args.max_revolving_capacity.to_le_bytes());
    debug_assert_eq!(msg.len(), 188);
    msg
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_850_000_000;

    fn args(counterparty: Pubkey) -> RegisterSessionKeyArgs {
        RegisterSessionKeyArgs {
            session_pubkey: [0x22; 32],
            max_amount: 5_000_000,
            expires_at: 1_900_000_000,
            allowed_counterparty: counterparty,
            nonce: crate::verify::voucher::SESSION_VOUCHER_V2_NONCE_FLAG | 17,
            max_revolving_capacity: 2_000_000,
            client_data_json: vec![],
            authenticator_data: vec![],
        }
    }

    fn existing_session(vault: Pubkey, counterparty: Pubkey) -> SessionAccount {
        SessionAccount {
            version: SESSION_VERSION_V1,
            bump: 201,
            vault,
            session: SessionRegistration {
                session_pubkey: [0x11; 32],
                max_amount: 9_000_000,
                expires_at: 1_800_000_000,
                allowed_counterparty: counterparty,
                nonce: 4,
                spent: 3_000_000,
                current_outstanding: 0,
                max_revolving_capacity: 4_000_000,
                crystallized_cumulative: 2_000_000,
                last_locked_sequence: 8,
            },
        }
    }

    #[test]
    fn expired_replacement_keeps_count_binds_scope_and_resets_retired_key_meters() {
        let vault = Pubkey::new_unique();
        let counterparty = Pubkey::new_unique();
        let existing = existing_session(vault, counterparty);
        let replacement = args(counterparty);

        validate_expired_registration_target(&existing, &vault, &replacement, NOW).unwrap();
        assert_eq!(reconciled_live_session_count(1, 0, false).unwrap(), 1);
        assert_eq!(reconciled_live_session_count(255, 0, false).unwrap(), 255);

        let stored = registration_from_args(&replacement);
        assert_eq!(stored.session_pubkey, replacement.session_pubkey);
        assert_eq!(stored.max_amount, replacement.max_amount);
        assert_eq!(stored.expires_at, replacement.expires_at);
        assert_eq!(
            stored.allowed_counterparty,
            replacement.allowed_counterparty
        );
        assert_eq!(stored.nonce, replacement.nonce);
        assert_eq!(
            stored.max_revolving_capacity,
            replacement.max_revolving_capacity
        );
        assert_eq!(stored.spent, 0);
        assert_eq!(stored.current_outstanding, 0);
        assert_eq!(stored.crystallized_cumulative, 0);
        assert_eq!(stored.last_locked_sequence, 0);
    }

    #[test]
    fn replacement_rejects_live_session_exposure_and_reused_signing_key() {
        let vault = Pubkey::new_unique();
        let counterparty = Pubkey::new_unique();
        let replacement = args(counterparty);

        let mut outstanding = existing_session(vault, counterparty);
        outstanding.session.current_outstanding = 1;
        assert!(
            validate_expired_registration_target(&outstanding, &vault, &replacement, NOW).is_err()
        );

        let mut live = existing_session(vault, counterparty);
        live.session.expires_at = NOW + 1;
        assert!(validate_expired_registration_target(&live, &vault, &replacement, NOW).is_err());

        let existing = existing_session(vault, counterparty);
        let mut reused_key = replacement.clone();
        reused_key.session_pubkey = existing.session.session_pubkey;
        assert!(validate_expired_registration_target(&existing, &vault, &reused_key, NOW).is_err());

        assert!(reconciled_live_session_count(255, 0, true).is_err());
    }

    #[test]
    fn registration_consent_binds_every_new_scope_field_and_target() {
        let program = Pubkey::new_unique();
        let vault = Pubkey::new_unique();
        let base = args(Pubkey::new_unique());
        let baseline = build_registration_message(&program, &vault, &base);
        assert_eq!(baseline.len(), 188);
        assert_eq!(&baseline[0..32], REGISTER_DOMAIN);
        assert_eq!(&baseline[32..64], program.as_ref());
        assert_eq!(&baseline[64..96], vault.as_ref());

        let mut variants = Vec::new();
        let mut changed = base.clone();
        changed.session_pubkey[0] ^= 1;
        variants.push(build_registration_message(&program, &vault, &changed));
        let mut changed = base.clone();
        changed.max_amount += 1;
        variants.push(build_registration_message(&program, &vault, &changed));
        let mut changed = base.clone();
        changed.expires_at += 1;
        variants.push(build_registration_message(&program, &vault, &changed));
        let mut changed = base.clone();
        changed.allowed_counterparty = Pubkey::new_unique();
        variants.push(build_registration_message(&program, &vault, &changed));
        let mut changed = base.clone();
        changed.nonce += 1;
        variants.push(build_registration_message(&program, &vault, &changed));
        let mut changed = base.clone();
        changed.max_revolving_capacity += 1;
        variants.push(build_registration_message(&program, &vault, &changed));
        variants.push(build_registration_message(
            &Pubkey::new_unique(),
            &vault,
            &base,
        ));
        variants.push(build_registration_message(
            &program,
            &Pubkey::new_unique(),
            &base,
        ));

        for variant in variants {
            assert_ne!(variant, baseline);
        }
    }
}
