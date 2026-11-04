//! Canonical Native Tab voucher wire versions and settlement-window rules.
//!
//! Existing mainnet sessions use the historical 44-byte V1 payload.  A fresh
//! live census proved that none of their registration nonces has bit 31 set, so
//! that bit is the collision-free on-chain marker for the context-bound V2
//! payload.  New registrations must set it; low-bit sessions are grandfathered
//! only until their existing registration is retired.

use anchor_lang::prelude::*;
use solana_sha256_hasher::hashv;

use crate::state::{SessionRegistration, VaultError};

/// Collision-free marker for context-bound V2 vouchers.
pub const SESSION_VOUCHER_V2_NONCE_FLAG: u32 = 1 << 31;

/// V2 encodes buyer-signed terminal intent in the sequence's high bit.  The
/// complete u32 remains in the signed message, so the flag cannot be changed
/// without invalidating the signature.
pub const VOUCHER_V2_FINAL_SEQUENCE_FLAG: u32 = 1 << 31;

pub const VOUCHER_V1_MESSAGE_LEN: usize = 44;
pub const VOUCHER_V2_MESSAGE_LEN: usize = 32;
pub const VOUCHER_V2_PREIMAGE_LEN: usize = 208;

/// Sellers retain a bounded opportunity to settle or crystallize a voucher
/// after issuance expires. Revocation never shortens that collection term.
pub const TAB_SETTLEMENT_GRACE_SECONDS: i64 = 7 * 24 * 60 * 60;

/// One buyer-signed obligation can be routed automatically to immediate cash
/// settlement or crystallization. The shared monotonic frontier makes those
/// execution terminals mutually exclusive after the first one lands.
const TAB_OBLIGATION_V2_DOMAIN: &[u8; 32] = b"DEXTER_TAB_OBLIGATION_V2\0\0\0\0\0\0\0\0";

pub fn is_v2_registration_nonce(nonce: u32) -> bool {
    nonce & SESSION_VOUCHER_V2_NONCE_FLAG != 0
}

pub fn require_v2_registration_nonce(nonce: u32) -> Result<()> {
    require!(
        is_v2_registration_nonce(nonce),
        VaultError::SessionVoucherV2NonceRequired
    );
    Ok(())
}

pub fn require_fresh_v2_registration_nonce(nonce: u32, authorization_nonce: u64) -> Result<()> {
    require_v2_registration_nonce(nonce)?;
    let generation = u32::try_from(authorization_nonce)
        .ok()
        .filter(|value| value & SESSION_VOUCHER_V2_NONCE_FLAG == 0)
        .ok_or_else(|| error!(VaultError::SessionVoucherV2NonceMismatch))?;
    require!(
        nonce == SESSION_VOUCHER_V2_NONCE_FLAG | generation,
        VaultError::SessionVoucherV2NonceMismatch
    );
    Ok(())
}

pub fn is_v2_final_voucher(registration: &SessionRegistration, sequence_number: u32) -> bool {
    is_v2_registration_nonce(registration.nonce)
        && sequence_number & VOUCHER_V2_FINAL_SEQUENCE_FLAG != 0
}

/// Build exactly one of the two supported wire messages.
///
/// V1 (grandfathered low-bit registrations only):
/// `channel || cumulative_le || sequence_le`.
///
/// V2 signs `sha256(preimage)`, where the fixed 208-byte preimage is:
/// `obligation_domain || program || vault || session_pda || counterparty ||
/// registration_nonce_le || channel || cumulative_le || sequence_with_flags_le`.
/// Digesting the complete context preserves every binding without adding 164
/// bytes to each Solana transaction relative to V1.
pub fn build_session_voucher_message(
    program_id: &Pubkey,
    vault: &Pubkey,
    session_pda: &Pubkey,
    registration: &SessionRegistration,
    channel_id: &[u8; 32],
    cumulative_amount: u64,
    sequence_number: u32,
) -> Vec<u8> {
    if !is_v2_registration_nonce(registration.nonce) {
        let mut message = Vec::with_capacity(VOUCHER_V1_MESSAGE_LEN);
        message.extend_from_slice(channel_id);
        message.extend_from_slice(&cumulative_amount.to_le_bytes());
        message.extend_from_slice(&sequence_number.to_le_bytes());
        debug_assert_eq!(message.len(), VOUCHER_V1_MESSAGE_LEN);
        return message;
    }

    let preimage = build_v2_preimage(
        program_id,
        vault,
        session_pda,
        registration,
        channel_id,
        cumulative_amount,
        sequence_number,
    );
    hashv(&[&preimage]).to_bytes().to_vec()
}

#[allow(clippy::too_many_arguments)]
fn build_v2_preimage(
    program_id: &Pubkey,
    vault: &Pubkey,
    session_pda: &Pubkey,
    registration: &SessionRegistration,
    channel_id: &[u8; 32],
    cumulative_amount: u64,
    sequence_number: u32,
) -> Vec<u8> {
    let mut preimage = Vec::with_capacity(VOUCHER_V2_PREIMAGE_LEN);
    preimage.extend_from_slice(TAB_OBLIGATION_V2_DOMAIN);
    preimage.extend_from_slice(program_id.as_ref());
    preimage.extend_from_slice(vault.as_ref());
    preimage.extend_from_slice(session_pda.as_ref());
    preimage.extend_from_slice(registration.allowed_counterparty.as_ref());
    preimage.extend_from_slice(&registration.nonce.to_le_bytes());
    preimage.extend_from_slice(channel_id);
    preimage.extend_from_slice(&cumulative_amount.to_le_bytes());
    preimage.extend_from_slice(&sequence_number.to_le_bytes());
    debug_assert_eq!(preimage.len(), VOUCHER_V2_PREIMAGE_LEN);
    preimage
}

pub fn require_within_settlement_window(expires_at: i64, now: i64) -> Result<()> {
    // Historical registration accepted any future i64. A still-live session
    // needs no grace arithmetic, so do not turn a far-future legacy expiry into
    // an overflow rejection.
    if now <= expires_at {
        return Ok(());
    }
    let deadline = expires_at
        .checked_add(TAB_SETTLEMENT_GRACE_SECONDS)
        .ok_or_else(|| error!(VaultError::VoucherSettlementGraceElapsed))?;
    require!(now <= deadline, VaultError::VoucherSettlementGraceElapsed);
    Ok(())
}

pub fn require_after_settlement_window(expires_at: i64, now: i64) -> Result<()> {
    require!(now > expires_at, VaultError::ForceReleaseTooEarly);
    let deadline = expires_at
        .checked_add(TAB_SETTLEMENT_GRACE_SECONDS)
        .ok_or_else(|| error!(VaultError::ForceReleaseTooEarly))?;
    require!(now > deadline, VaultError::ForceReleaseTooEarly);
    Ok(())
}

/// New registrations must leave a representable seller grace deadline.
/// Grandfathered far-future V1 sessions remain usable through the early return
/// in `require_within_settlement_window`.
pub fn require_bounded_session_expiry(expires_at: i64) -> Result<()> {
    require!(
        expires_at
            .checked_add(TAB_SETTLEMENT_GRACE_SECONDS)
            .is_some(),
        VaultError::SessionExpiryTooFar
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registration(nonce: u32) -> SessionRegistration {
        SessionRegistration {
            session_pubkey: [7; 32],
            max_amount: 1_000,
            expires_at: 1_900_000_000,
            allowed_counterparty: Pubkey::new_unique(),
            nonce,
            spent: 0,
            current_outstanding: 100,
            max_revolving_capacity: 1_000,
            crystallized_cumulative: 0,
            last_locked_sequence: 0,
        }
    }

    #[test]
    fn low_bit_registration_is_exact_legacy_v1() {
        let channel = [5; 32];
        let message = build_session_voucher_message(
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &registration(17),
            &channel,
            42,
            9,
        );
        assert_eq!(message.len(), VOUCHER_V1_MESSAGE_LEN);
        assert_eq!(&message[..32], &channel);
        assert_eq!(&message[32..40], &42u64.to_le_bytes());
        assert_eq!(&message[40..44], &9u32.to_le_bytes());
    }

    #[test]
    fn high_bit_registration_binds_every_v2_context_and_finality() {
        let program = Pubkey::new_unique();
        let vault = Pubkey::new_unique();
        let session = Pubkey::new_unique();
        let registration = registration(SESSION_VOUCHER_V2_NONCE_FLAG | 17);
        let channel = [5; 32];
        let sequence = VOUCHER_V2_FINAL_SEQUENCE_FLAG | 9;
        let baseline_preimage = build_v2_preimage(
            &program,
            &vault,
            &session,
            &registration,
            &channel,
            42,
            sequence,
        );

        assert_eq!(baseline_preimage.len(), VOUCHER_V2_PREIMAGE_LEN);
        assert_eq!(&baseline_preimage[0..32], TAB_OBLIGATION_V2_DOMAIN);
        assert_eq!(&baseline_preimage[32..64], program.as_ref());
        assert_eq!(&baseline_preimage[64..96], vault.as_ref());
        assert_eq!(&baseline_preimage[96..128], session.as_ref());
        assert_eq!(
            &baseline_preimage[128..160],
            registration.allowed_counterparty.as_ref()
        );
        assert_eq!(
            &baseline_preimage[160..164],
            &registration.nonce.to_le_bytes()
        );
        assert_eq!(&baseline_preimage[164..196], &channel);
        assert_eq!(&baseline_preimage[196..204], &42u64.to_le_bytes());
        assert_eq!(&baseline_preimage[204..208], &sequence.to_le_bytes());
        assert!(is_v2_final_voucher(&registration, sequence));

        let baseline = build_session_voucher_message(
            &program,
            &vault,
            &session,
            &registration,
            &channel,
            42,
            sequence,
        );
        assert_eq!(baseline.len(), VOUCHER_V2_MESSAGE_LEN);
        assert_eq!(baseline, hashv(&[&baseline_preimage]).to_bytes());

        let mut changed = registration.clone();
        changed.allowed_counterparty = Pubkey::new_unique();
        assert_ne!(
            baseline,
            build_session_voucher_message(
                &program, &vault, &session, &changed, &channel, 42, sequence
            )
        );
        assert_ne!(
            baseline,
            build_session_voucher_message(
                &program,
                &vault,
                &session,
                &registration,
                &channel,
                42,
                sequence & !VOUCHER_V2_FINAL_SEQUENCE_FLAG,
            )
        );
        assert_eq!(
            baseline,
            build_session_voucher_message(
                &program,
                &vault,
                &session,
                &registration,
                &channel,
                42,
                sequence,
            )
        );
    }

    #[test]
    fn settlement_window_is_closed_after_exact_bounded_grace() {
        let expiry = 1_000;
        assert!(require_within_settlement_window(expiry, expiry).is_ok());
        assert!(
            require_within_settlement_window(expiry, expiry + TAB_SETTLEMENT_GRACE_SECONDS).is_ok()
        );
        assert!(require_within_settlement_window(
            expiry,
            expiry + TAB_SETTLEMENT_GRACE_SECONDS + 1
        )
        .is_err());
        assert!(require_within_settlement_window(i64::MAX, 0).is_ok());
        assert!(
            require_after_settlement_window(expiry, expiry + TAB_SETTLEMENT_GRACE_SECONDS).is_err()
        );
        assert!(
            require_after_settlement_window(expiry, expiry + TAB_SETTLEMENT_GRACE_SECONDS + 1)
                .is_ok()
        );
        assert!(require_after_settlement_window(i64::MAX, 0).is_err());
        assert!(require_bounded_session_expiry(i64::MAX).is_err());
        assert!(require_bounded_session_expiry(i64::MAX - TAB_SETTLEMENT_GRACE_SECONDS).is_ok());
    }

    #[test]
    fn v2_registration_generation_is_derived_from_permanent_passkey_nonce() {
        assert!(
            require_fresh_v2_registration_nonce(SESSION_VOUCHER_V2_NONCE_FLAG | 17, 17,).is_ok()
        );
        assert!(
            require_fresh_v2_registration_nonce(SESSION_VOUCHER_V2_NONCE_FLAG | 18, 17,).is_err()
        );
        assert!(require_fresh_v2_registration_nonce(17, 17).is_err());
        assert!(require_fresh_v2_registration_nonce(
            SESSION_VOUCHER_V2_NONCE_FLAG,
            SESSION_VOUCHER_V2_NONCE_FLAG as u64,
        )
        .is_err());
    }
}
