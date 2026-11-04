use anchor_lang::prelude::*;
use anchor_lang::solana_program::sysvar;

use crate::state::*;
use crate::verify::voucher::is_v2_registration_nonce;
use crate::verify::webauthn::verify_and_consume_passkey_authorization;

/// Domain separator for session-key REVOCATION messages. Distinct from the
/// registration domain so a registration signature can never be reinterpreted
/// as a revocation (or vice versa).
///
/// V3 binds every live session meter and the exact Vault pending counter. A
/// signature prepared while the target is empty therefore cannot be withheld
/// and applied after a seller reservation opens.
///
/// Layout note: 21 bytes of label + 11 NUL = 32 bytes total.
const REVOKE_DOMAIN: &[u8; 32] = b"OTS_SESSION_REVOKE_V3\0\0\0\0\0\0\0\0\0\0\0";

#[derive(Accounts)]
#[instruction(args: RevokeSessionKeyArgs)]
pub struct RevokeSessionKey<'info> {
    /// Mutated to decrement `live_session_count`. No signer required; the passkey
    /// signature in the args is the authorization.
    #[account(mut)]
    pub vault: Account<'info, Vault>,
    #[account(
        mut,
        seeds = [crate::constants::PASSKEY_AUTHORIZATION_SEED, vault.key().as_ref()],
        bump = passkey_authorization.bump,
    )]
    pub passkey_authorization: Account<'info, PasskeyAuthorization>,
    /// The session PDA being revoked. mut (we clear it in place). bump from stored.
    /// The seed binds it to (vault, allowed_counterparty), so it cannot be
    /// redirected to a foreign counterparty's session.
    #[account(
        mut,
        seeds = [crate::constants::SESSION_SEED, vault.key().as_ref(), args.allowed_counterparty.as_ref()],
        bump = session.bump,
    )]
    pub session: Account<'info, SessionAccount>,
    /// CHECK: instructions sysvar — address-constrained. The previous
    /// instruction in the transaction MUST be a secp256r1_verify call.
    #[account(address = sysvar::instructions::ID)]
    pub instructions_sysvar: AccountInfo<'info>,
}

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct RevokeSessionKeyArgs {
    /// The counterparty whose session PDA is being revoked. Goes in the args so
    /// the accounts struct can re-derive the PDA seed.
    pub allowed_counterparty: Pubkey,
    /// Exact global reservation count observed when the owner consented. This
    /// is part of the V3 challenge and must still match at execution.
    pub expected_pending_voucher_count: u32,
    pub client_data_json: Vec<u8>,
    pub authenticator_data: Vec<u8>,
}

/// Buyer's passkey explicitly tears down a named session (V6).
///
/// Two uses:
///  - The buyer closed their tab and wants to make absolutely sure no further
///    vouchers can be signed under the previous session (defense in depth;
///    expiry would do it eventually anyway).
///  - The buyer suspects the session key leaked and wants to kill it NOW
///    rather than wait for expiry.
///
/// Revocation closes future issuance without changing the expiry/collection
/// terms a seller already relied upon. If this session still carries an
/// outstanding reservation, its key, nonce, PDA identity, counterparty,
/// original expiry, and every accounting frontier remain intact; setting the
/// revolving capacity to zero makes any later tab-open fail. Only an
/// already-zero session is cleared and removed from the live-count census.
fn apply_revocation(registration: &mut SessionRegistration) -> bool {
    if registration.current_outstanding > 0 {
        registration.max_revolving_capacity = 0;
        return false;
    }

    *registration = SessionRegistration {
        session_pubkey: [0u8; 32],
        max_amount: 0,
        expires_at: 0,
        allowed_counterparty: Pubkey::default(),
        nonce: 0,
        spent: 0,
        current_outstanding: 0,
        max_revolving_capacity: 0,
        crystallized_cumulative: 0,
        last_locked_sequence: 0,
    };
    true
}

pub fn handler(ctx: Context<RevokeSessionKey>, args: RevokeSessionKeyArgs) -> Result<()> {
    // ── Version gate → V6 (only V6 vaults carry SessionAccount PDAs). ─────────
    require!(
        ctx.accounts.vault.version == VAULT_VERSION_V7,
        VaultError::UnsupportedVaultVersion
    );
    require!(
        !legacy_v6_resolution_pending(ctx.accounts.vault.live_session_count),
        VaultError::LegacyV1TabBindingMismatch
    );

    // ── Guard: the session must be live/registered. version==0 means it was
    //    never touched, or already cleared by a prior revoke / expiry sweep. ──
    require!(
        ctx.accounts.session.version != 0,
        VaultError::NoActiveSession
    );

    // The passkey's revocation message includes the specific session pubkey
    // being revoked — now read from the session PDA, not vault.active_session.
    // This prevents a stale-revocation replay from being accepted against a NEW
    // session: if the buyer rotates the session for this counterparty, an
    // attacker holding an old revocation signature can't use it to kill the new
    // one because the session_pubkey field won't match.
    //
    // Copy the scalars the verify step needs BEFORE taking any `&mut` borrow.
    let vault_key = ctx.accounts.vault.key();
    let passkey = ctx.accounts.vault.passkey_pubkey;
    let session_pda = ctx.accounts.session.key();
    require_keys_eq!(
        ctx.accounts.session.session.allowed_counterparty,
        args.allowed_counterparty,
        VaultError::SessionAccountForeign
    );
    // A recovered low-bit V1 obligation is byte-attested. Mutating even its
    // revolving cap would invalidate the exact owner-bound bridge and let the
    // owner deny the seller's fresh recovery window. Zero-outstanding V1 may
    // still be retired; nonzero V1 is recovery-controlled.
    require!(
        ctx.accounts.session.session.current_outstanding == 0
            || is_v2_registration_nonce(ctx.accounts.session.session.nonce),
        VaultError::LegacyV1TabBindingMismatch
    );
    require!(
        args.expected_pending_voucher_count == ctx.accounts.vault.pending_voucher_count,
        VaultError::SessionRevocationStateChanged
    );

    let revocation_message = build_revocation_message(
        ctx.program_id,
        &vault_key,
        &session_pda,
        &ctx.accounts.session.session,
        args.expected_pending_voucher_count,
    );

    verify_and_consume_passkey_authorization(
        &ctx.accounts.instructions_sysvar,
        &passkey,
        &mut ctx.accounts.passkey_authorization,
        &vault_key,
        &args.client_data_json,
        &args.authenticator_data,
        &revocation_message,
    )?;

    let session = &mut ctx.accounts.session;
    let cleared = apply_revocation(&mut session.session);
    if cleared {
        session.version = 0;
        let vault = &mut ctx.accounts.vault;
        vault.live_session_count = vault
            .live_session_count
            .checked_sub(1)
            .ok_or_else(|| error!(VaultError::IncompleteSessionSet))?;
    }

    Ok(())
}

/// Deterministic 252-byte serialization of the revocation message.
///
/// Layout:
///   [  0..32) domain separator literal "OTS_SESSION_REVOKE_V3\0\0\0\0\0\0\0\0\0\0\0"
///   [ 32..64) program ID
///   [ 64..96) vault PDA
///   [ 96..128) session PDA
///   [128..160) session pubkey
///   [160..168) max amount (u64 LE)
///   [168..176) expiry (i64 LE)
///   [176..208) allowed counterparty
///   [208..212) registration nonce (u32 LE)
///   [212..220) spent frontier (u64 LE)
///   [220..228) current outstanding (u64 LE)
///   [228..236) max revolving capacity (u64 LE)
///   [236..244) crystallized cumulative frontier (u64 LE)
///   [244..248) last locked sequence (u32 LE)
///   [248..252) expected Vault pending-voucher count (u32 LE)
fn build_revocation_message(
    program_id: &Pubkey,
    vault_pda: &Pubkey,
    session_pda: &Pubkey,
    registration: &SessionRegistration,
    expected_pending_voucher_count: u32,
) -> Vec<u8> {
    let mut msg = Vec::with_capacity(252);
    msg.extend_from_slice(REVOKE_DOMAIN);
    msg.extend_from_slice(program_id.as_ref());
    msg.extend_from_slice(vault_pda.as_ref());
    msg.extend_from_slice(session_pda.as_ref());
    msg.extend_from_slice(&registration.session_pubkey);
    msg.extend_from_slice(&registration.max_amount.to_le_bytes());
    msg.extend_from_slice(&registration.expires_at.to_le_bytes());
    msg.extend_from_slice(registration.allowed_counterparty.as_ref());
    msg.extend_from_slice(&registration.nonce.to_le_bytes());
    msg.extend_from_slice(&registration.spent.to_le_bytes());
    msg.extend_from_slice(&registration.current_outstanding.to_le_bytes());
    msg.extend_from_slice(&registration.max_revolving_capacity.to_le_bytes());
    msg.extend_from_slice(&registration.crystallized_cumulative.to_le_bytes());
    msg.extend_from_slice(&registration.last_locked_sequence.to_le_bytes());
    msg.extend_from_slice(&expected_pending_voucher_count.to_le_bytes());
    debug_assert_eq!(msg.len(), 252);
    msg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revocation_consent_binds_counterparty_and_session_target() {
        let program = Pubkey::new_unique();
        let vault = Pubkey::new_unique();
        let session = Pubkey::new_unique();
        let counterparty = Pubkey::new_unique();
        let registration = SessionRegistration {
            session_pubkey: [0x55; 32],
            max_amount: 123,
            expires_at: 456,
            allowed_counterparty: counterparty,
            nonce: 789,
            spent: 12,
            current_outstanding: 34,
            max_revolving_capacity: 56,
            crystallized_cumulative: 78,
            last_locked_sequence: 90,
        };
        let expected_pending_voucher_count = 3;
        let message = build_revocation_message(
            &program,
            &vault,
            &session,
            &registration,
            expected_pending_voucher_count,
        );

        assert_eq!(message.len(), 252);
        assert_eq!(&message[0..32], REVOKE_DOMAIN);
        assert_eq!(&message[32..64], program.as_ref());
        assert_eq!(&message[64..96], vault.as_ref());
        assert_eq!(&message[96..128], session.as_ref());
        assert_eq!(&message[128..160], &registration.session_pubkey);
        assert_eq!(&message[160..168], &registration.max_amount.to_le_bytes());
        assert_eq!(&message[168..176], &registration.expires_at.to_le_bytes());
        assert_eq!(&message[176..208], counterparty.as_ref());
        assert_eq!(&message[208..212], &registration.nonce.to_le_bytes());
        assert_eq!(&message[212..220], &registration.spent.to_le_bytes());
        assert_eq!(
            &message[220..228],
            &registration.current_outstanding.to_le_bytes()
        );
        assert_eq!(
            &message[228..236],
            &registration.max_revolving_capacity.to_le_bytes()
        );
        assert_eq!(
            &message[236..244],
            &registration.crystallized_cumulative.to_le_bytes()
        );
        assert_eq!(
            &message[244..248],
            &registration.last_locked_sequence.to_le_bytes()
        );
        assert_eq!(
            &message[248..252],
            &expected_pending_voucher_count.to_le_bytes()
        );

        let mut redirected = registration.clone();
        redirected.allowed_counterparty = Pubkey::new_unique();
        assert_ne!(
            message,
            build_revocation_message(
                &program,
                &vault,
                &session,
                &redirected,
                expected_pending_voucher_count,
            )
        );
        assert_ne!(
            message,
            build_revocation_message(
                &program,
                &vault,
                &Pubkey::new_unique(),
                &registration,
                expected_pending_voucher_count,
            )
        );
        assert_ne!(
            message,
            build_revocation_message(
                &program,
                &vault,
                &session,
                &registration,
                expected_pending_voucher_count + 1,
            )
        );

        // Every mutable meter is part of the consent. A zero-outstanding
        // revocation signature cannot land after a reservation opens.
        let mutations: [fn(&mut SessionRegistration); 5] = [
            |r: &mut SessionRegistration| r.spent += 1,
            |r: &mut SessionRegistration| r.current_outstanding += 1,
            |r: &mut SessionRegistration| r.max_revolving_capacity += 1,
            |r: &mut SessionRegistration| r.crystallized_cumulative += 1,
            |r: &mut SessionRegistration| r.last_locked_sequence += 1,
        ];
        for mutate in mutations {
            let mut changed = registration.clone();
            mutate(&mut changed);
            assert_ne!(
                message,
                build_revocation_message(
                    &program,
                    &vault,
                    &session,
                    &changed,
                    expected_pending_voucher_count,
                )
            );
        }
    }

    #[test]
    fn revoke_preserves_outstanding_identity_and_frontiers_but_stops_issuance() {
        let counterparty = Pubkey::new_unique();
        let mut registration = SessionRegistration {
            session_pubkey: [0x55; 32],
            max_amount: 123,
            expires_at: 1_000,
            allowed_counterparty: counterparty,
            nonce: 789,
            spent: 12,
            current_outstanding: 34,
            max_revolving_capacity: 56,
            crystallized_cumulative: 78,
            last_locked_sequence: 90,
        };
        let before = registration.clone();
        assert!(!apply_revocation(&mut registration));
        assert_eq!(registration.expires_at, before.expires_at);
        assert_eq!(registration.max_revolving_capacity, 0);
        assert_eq!(registration.session_pubkey, before.session_pubkey);
        assert_eq!(
            registration.allowed_counterparty,
            before.allowed_counterparty
        );
        assert_eq!(registration.nonce, before.nonce);
        assert_eq!(registration.spent, before.spent);
        assert_eq!(registration.current_outstanding, before.current_outstanding);
        assert_eq!(
            registration.crystallized_cumulative,
            before.crystallized_cumulative
        );
        assert_eq!(
            registration.last_locked_sequence,
            before.last_locked_sequence
        );
    }

    #[test]
    fn revoke_clears_only_an_already_zero_session() {
        let mut registration = SessionRegistration {
            session_pubkey: [0x55; 32],
            max_amount: 123,
            expires_at: 1_000,
            allowed_counterparty: Pubkey::new_unique(),
            nonce: 789,
            spent: 12,
            current_outstanding: 0,
            max_revolving_capacity: 56,
            crystallized_cumulative: 78,
            last_locked_sequence: 90,
        };
        assert!(apply_revocation(&mut registration));
        assert_eq!(registration.session_pubkey, [0; 32]);
        assert_eq!(registration.allowed_counterparty, Pubkey::default());
        assert_eq!(registration.current_outstanding, 0);
    }
}
