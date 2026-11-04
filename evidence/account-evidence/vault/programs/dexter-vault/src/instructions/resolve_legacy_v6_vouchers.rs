//! Current-owner terminal exits for quarantined legacy V1 voucher state.
//!
//! Recovery never infers that a pending Vault counter belongs to a session
//! meter. Every SessionAccount is immutably copied field-for-field into the
//! recovery attestation and receives a permanent neutral resolution slot.
//! Before the deadline, only a fresh exact owner-passkey voucher binding may
//! authorize a settle or lock terminal. After the deadline, the current owner
//! passkey may independently:
//!   * neutralize one exact still-unchanged session meter (including zero); and
//!   * release an explicitly chosen part of the exact historical pending
//!     counter residual.
//!
//! Attestation and resolution PDAs are permanent replay tombstones.

use anchor_lang::prelude::*;
use anchor_lang::solana_program::sysvar;
use solana_sha256_hasher::hashv;

use crate::constants::{
    LEGACY_V6_ATTESTATION_SEED, LEGACY_V6_RECOVERY_MAX_SESSIONS, LEGACY_V6_VOUCHER_RESOLUTION_SEED,
    PASSKEY_AUTHORIZATION_SEED,
};
use crate::state::*;
use crate::verify::webauthn::verify_and_consume_passkey_authorization_one_assertion;

const PENDING_RESIDUAL_RELEASE_DOMAIN_V1: &[u8; 32] = b"DEXTER_V6_RESIDUAL_RELEASE_V1\0\0\0";
const SESSION_RELEASE_DOMAIN_V1: &[u8; 32] = b"DEXTER_V6_SESSION_RELEASE_V1\0\0\0\0";
const SOLANA_MAINNET_BETA_DOMAIN: &[u8; 32] = b"SOLANA_MAINNET_BETA\0\0\0\0\0\0\0\0\0\0\0\0\0";
const CLIENT_DATA_MAX_LEN: usize = 400;
const AUTHENTICATOR_DATA_LEN: usize = 37;

#[derive(Accounts)]
pub struct ReleaseLegacyV6PendingResidual<'info> {
    #[account(mut)]
    pub vault: Box<Account<'info, Vault>>,

    #[account(
        seeds = [LEGACY_V6_ATTESTATION_SEED, vault.key().as_ref()],
        bump = attestation.bump,
    )]
    pub attestation: Box<Account<'info, LegacyV6Attestation>>,

    #[account(
        mut,
        seeds = [LEGACY_V6_VOUCHER_RESOLUTION_SEED, vault.key().as_ref()],
        bump = voucher_resolution.bump,
    )]
    pub voucher_resolution: Box<Account<'info, LegacyV6VoucherResolution>>,

    #[account(
        mut,
        seeds = [PASSKEY_AUTHORIZATION_SEED, vault.key().as_ref()],
        bump = passkey_authorization.bump,
    )]
    pub passkey_authorization: Box<Account<'info, PasskeyAuthorization>>,

    /// CHECK: canonical instruction sysvar used by the one-assertion verifier.
    #[account(address = sysvar::instructions::ID)]
    pub instructions_sysvar: AccountInfo<'info>,
}

#[derive(Accounts)]
pub struct ReleaseLegacyV6SessionReservation<'info> {
    #[account(mut)]
    pub vault: Box<Account<'info, Vault>>,

    #[account(
        seeds = [LEGACY_V6_ATTESTATION_SEED, vault.key().as_ref()],
        bump = attestation.bump,
    )]
    pub attestation: Box<Account<'info, LegacyV6Attestation>>,

    #[account(
        mut,
        seeds = [LEGACY_V6_VOUCHER_RESOLUTION_SEED, vault.key().as_ref()],
        bump = voucher_resolution.bump,
    )]
    pub voucher_resolution: Box<Account<'info, LegacyV6VoucherResolution>>,

    /// Exact immutable-snapshot SessionAccount selected by `session_index`.
    #[account(mut)]
    pub session: Box<Account<'info, SessionAccount>>,

    #[account(
        mut,
        seeds = [PASSKEY_AUTHORIZATION_SEED, vault.key().as_ref()],
        bump = passkey_authorization.bump,
    )]
    pub passkey_authorization: Box<Account<'info, PasskeyAuthorization>>,

    /// CHECK: canonical instruction sysvar used by the one-assertion verifier.
    #[account(address = sysvar::instructions::ID)]
    pub instructions_sysvar: AccountInfo<'info>,
}

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct ReleaseLegacyV6PendingResidualArgs {
    pub release_count: u32,
    pub expected_pending_voucher_count: u32,
    pub client_data_json: Vec<u8>,
    pub authenticator_data: Vec<u8>,
}

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct ReleaseLegacyV6SessionReservationArgs {
    pub session_index: u8,
    pub client_data_json: Vec<u8>,
    pub authenticator_data: Vec<u8>,
}

fn require_compact_assertion(client_data_json: &[u8], authenticator_data: &[u8]) -> Result<()> {
    require!(
        client_data_json.len() <= CLIENT_DATA_MAX_LEN
            && authenticator_data.len() == AUTHENTICATOR_DATA_LEN,
        VaultError::PasskeyVerificationFailed
    );
    Ok(())
}

fn require_owner_release_window(now: i64, deadline: i64) -> Result<()> {
    require!(now > deadline, VaultError::LegacyV6PendingVoucherAmbiguous);
    Ok(())
}

pub(crate) fn consumed_pending_count(resolution: &LegacyV6VoucherResolution) -> Result<u32> {
    resolution
        .terminal_consumed_pending_count
        .checked_add(resolution.owner_released_pending_count)
        .filter(|total| *total <= resolution.original_pending_count)
        .ok_or_else(|| error!(VaultError::LegacyV6PendingVoucherAmbiguous))
}

pub(crate) fn exact_historical_pending_count(
    resolution: &LegacyV6VoucherResolution,
) -> Result<u32> {
    resolution
        .original_pending_count
        .checked_sub(consumed_pending_count(resolution)?)
        .ok_or_else(|| error!(VaultError::LegacyV6PendingVoucherAmbiguous))
}

pub(crate) fn reserved_historical_pending_count(
    resolution: &LegacyV6VoucherResolution,
) -> Result<u32> {
    resolution
        .sessions
        .iter()
        .take(usize::from(resolution.session_count))
        .try_fold(0u32, |count, state| {
            if state.counter_reserved {
                count
                    .checked_add(1)
                    .ok_or_else(|| error!(VaultError::LegacyV6PendingVoucherAmbiguous))
            } else {
                Ok(count)
            }
        })
}

pub(crate) fn unreserved_historical_pending_count(
    resolution: &LegacyV6VoucherResolution,
) -> Result<u32> {
    exact_historical_pending_count(resolution)?
        .checked_sub(reserved_historical_pending_count(resolution)?)
        .ok_or_else(|| error!(VaultError::LegacyV6PendingVoucherAmbiguous))
}

fn unresolved_session_count(resolution: &LegacyV6VoucherResolution) -> Result<u8> {
    resolution
        .sessions
        .iter()
        .take(usize::from(resolution.session_count))
        .try_fold(0u8, |count, state| {
            if matches!(
                state.state,
                LEGACY_V6_SESSION_RESOLUTION_NEUTRAL | LEGACY_V6_SESSION_RESOLUTION_OWNER_BOUND
            ) {
                count
                    .checked_add(1)
                    .ok_or_else(|| error!(VaultError::LegacyV6SessionSnapshotMismatch))
            } else {
                Ok(count)
            }
        })
}

pub(crate) fn validate_resolution(
    program_id: &Pubkey,
    vault_key: &Pubkey,
    vault: &Vault,
    attestation_key: Pubkey,
    attestation: &LegacyV6Attestation,
    resolution_key: Pubkey,
    resolution: &LegacyV6VoucherResolution,
) -> Result<()> {
    let (expected_attestation, attestation_bump) = Pubkey::find_program_address(
        &[LEGACY_V6_ATTESTATION_SEED, vault_key.as_ref()],
        program_id,
    );
    let (expected_resolution, resolution_bump) = Pubkey::find_program_address(
        &[LEGACY_V6_VOUCHER_RESOLUTION_SEED, vault_key.as_ref()],
        program_id,
    );
    require!(
        vault.version == VAULT_VERSION_V7
            && legacy_v6_resolution_pending(vault.live_session_count)
            && attestation_key == expected_attestation
            && attestation.version == LEGACY_V6_ATTESTATION_VERSION_V1
            && attestation.bump == attestation_bump
            && attestation.vault == *vault_key
            && resolution_key == expected_resolution
            && resolution.version == LEGACY_V6_VOUCHER_RESOLUTION_VERSION_V1
            && resolution.bump == resolution_bump
            && resolution.vault == *vault_key
            && resolution.attestation == attestation_key
            && resolution.recovered_at == attestation.recovered_at
            && resolution.settlement_deadline == attestation.legacy_voucher_settlement_deadline
            && resolution.original_pending_count == attestation.pending_voucher_count
            && resolution.session_count == attestation.session_account_count
            && usize::from(resolution.session_count) <= LEGACY_V6_RECOVERY_MAX_SESSIONS
            && (resolution.original_pending_count == 0 && resolution.session_count == 0
                || attestation.quarantine_flags & LEGACY_V6_QUARANTINE_UNBOUND_V1_VOUCHER != 0),
        VaultError::LegacyV6AttestationMismatch
    );
    consumed_pending_count(resolution)?;

    for index in 0..LEGACY_V6_RECOVERY_MAX_SESSIONS {
        let should_be_present = index < usize::from(resolution.session_count);
        let snapshot = attestation.session_attestations[index];
        let state = resolution.sessions[index];
        require!(
            snapshot.present == should_be_present
                && state.present == should_be_present
                && (!should_be_present
                    || (snapshot.session_account == state.session
                        && state.remaining_outstanding <= snapshot.current_outstanding
                        && match state.state {
                            LEGACY_V6_SESSION_RESOLUTION_NEUTRAL => {
                                state.binding == Pubkey::default() && !state.counter_reserved
                            }
                            LEGACY_V6_SESSION_RESOLUTION_OWNER_BOUND => {
                                state.binding != Pubkey::default()
                            }
                            LEGACY_V6_SESSION_RESOLUTION_SETTLED
                            | LEGACY_V6_SESSION_RESOLUTION_LOCKED => {
                                state.binding != Pubkey::default()
                                    && !state.counter_reserved
                                    && state.remaining_outstanding == 0
                            }
                            LEGACY_V6_SESSION_RESOLUTION_OWNER_RELEASED => {
                                !state.counter_reserved && state.remaining_outstanding == 0
                            }
                            _ => false,
                        })),
            VaultError::LegacyV6AttestationMismatch
        );
    }
    require!(
        live_session_count_without_recovery_flag(vault.live_session_count)
            == unresolved_session_count(resolution)?
            && vault.pending_voucher_count == exact_historical_pending_count(resolution)?
            && reserved_historical_pending_count(resolution)?
                <= exact_historical_pending_count(resolution)?,
        VaultError::LegacyV6PendingVoucherAmbiguous
    );
    Ok(())
}

pub(crate) fn normalized_recovered_session_sha256(
    session_info: &AccountInfo<'_>,
) -> Result<[u8; 32]> {
    let data = session_info.try_borrow_data()?;
    require!(
        data.len() == 8 + SessionAccount::INIT_SPACE
            && &data[..8] == SessionAccount::DISCRIMINATOR
            && data[8] == SESSION_VERSION_LEGACY_V6_RECOVERED_V1,
        VaultError::LegacyV6SessionSnapshotMismatch
    );
    let mut normalized = data.to_vec();
    normalized[8] = SESSION_VERSION_V1;
    Ok(hashv(&[&normalized]).to_bytes())
}

pub(crate) fn require_exact_session(
    vault_key: &Pubkey,
    session_key: Pubkey,
    session: &SessionAccount,
    raw_data_sha256: [u8; 32],
    snapshot: &LegacyV6SessionAttestation,
) -> Result<()> {
    require!(
        snapshot.present
            && snapshot.session_account == session_key
            && snapshot.account_version == SESSION_VERSION_V1
            && session.version == SESSION_VERSION_LEGACY_V6_RECOVERED_V1
            && snapshot.account_bump == session.bump
            && snapshot.vault == *vault_key
            && session.vault == *vault_key
            && snapshot.session_pubkey == session.session.session_pubkey
            && snapshot.max_amount == session.session.max_amount
            && snapshot.expires_at == session.session.expires_at
            && snapshot.allowed_counterparty == session.session.allowed_counterparty
            && snapshot.nonce == session.session.nonce
            && snapshot.spent == session.session.spent
            && snapshot.current_outstanding == session.session.current_outstanding
            && snapshot.max_revolving_capacity == session.session.max_revolving_capacity
            && snapshot.crystallized_cumulative == session.session.crystallized_cumulative
            && snapshot.last_locked_sequence == session.session.last_locked_sequence
            && snapshot.recovered_data_sha256 == raw_data_sha256,
        VaultError::LegacyV6SessionSnapshotMismatch
    );
    Ok(())
}

pub(crate) fn retire_recovered_session(
    vault: &mut Vault,
    session: &mut SessionAccount,
) -> Result<()> {
    require!(
        legacy_v6_resolution_pending(vault.live_session_count)
            && session.version == SESSION_VERSION_LEGACY_V6_RECOVERED_V1,
        VaultError::LegacyV6SessionSnapshotMismatch
    );
    let live = live_session_count_without_recovery_flag(vault.live_session_count)
        .checked_sub(1)
        .ok_or_else(|| error!(VaultError::LegacyV6SessionSnapshotMismatch))?;
    vault.live_session_count = LEGACY_V6_RESOLUTION_PENDING | live;
    session.version = 0;
    session.session = SessionRegistration {
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
    Ok(())
}

pub(crate) fn maybe_complete_legacy_v6_resolution(
    vault: &mut Vault,
    resolution: &LegacyV6VoucherResolution,
) -> Result<()> {
    let unresolved = unresolved_session_count(resolution)?;
    require!(
        live_session_count_without_recovery_flag(vault.live_session_count) == unresolved
            && vault.pending_voucher_count == exact_historical_pending_count(resolution)?,
        VaultError::LegacyV6PendingVoucherAmbiguous
    );
    if unresolved == 0 && vault.pending_voucher_count == 0 {
        vault.live_session_count = 0;
    }
    Ok(())
}

pub(crate) fn session_attestation_sha256(
    snapshot: &LegacyV6SessionAttestation,
) -> Result<[u8; 32]> {
    let mut bytes = Vec::new();
    snapshot.serialize(&mut bytes)?;
    Ok(hashv(&[&bytes]).to_bytes())
}

#[allow(clippy::too_many_arguments)]
fn build_pending_residual_release_operation(
    vault: &Pubkey,
    attestation: &Pubkey,
    resolution: &Pubkey,
    original_data_sha256: &[u8; 32],
    recovered_at: i64,
    deadline: i64,
    original_pending_count: u32,
    release_count: u32,
    expected_pending_count: u32,
) -> Vec<u8> {
    let mut operation = Vec::with_capacity(252);
    operation.extend_from_slice(PENDING_RESIDUAL_RELEASE_DOMAIN_V1);
    operation.extend_from_slice(SOLANA_MAINNET_BETA_DOMAIN);
    operation.extend_from_slice(crate::ID.as_ref());
    operation.extend_from_slice(vault.as_ref());
    operation.extend_from_slice(attestation.as_ref());
    operation.extend_from_slice(resolution.as_ref());
    operation.extend_from_slice(original_data_sha256);
    operation.extend_from_slice(&recovered_at.to_le_bytes());
    operation.extend_from_slice(&deadline.to_le_bytes());
    operation.extend_from_slice(&original_pending_count.to_le_bytes());
    operation.extend_from_slice(&release_count.to_le_bytes());
    operation.extend_from_slice(&expected_pending_count.to_le_bytes());
    debug_assert_eq!(operation.len(), 252);
    operation
}

#[allow(clippy::too_many_arguments)]
fn build_session_release_operation(
    vault: &Pubkey,
    attestation: &Pubkey,
    resolution: &Pubkey,
    original_data_sha256: &[u8; 32],
    deadline: i64,
    session_index: u8,
    session_snapshot_sha256: &[u8; 32],
    current_outstanding: u64,
) -> Vec<u8> {
    let mut operation = Vec::with_capacity(273);
    operation.extend_from_slice(SESSION_RELEASE_DOMAIN_V1);
    operation.extend_from_slice(SOLANA_MAINNET_BETA_DOMAIN);
    operation.extend_from_slice(crate::ID.as_ref());
    operation.extend_from_slice(vault.as_ref());
    operation.extend_from_slice(attestation.as_ref());
    operation.extend_from_slice(resolution.as_ref());
    operation.extend_from_slice(original_data_sha256);
    operation.extend_from_slice(&deadline.to_le_bytes());
    operation.push(session_index);
    operation.extend_from_slice(session_snapshot_sha256);
    operation.extend_from_slice(&current_outstanding.to_le_bytes());
    debug_assert_eq!(operation.len(), 273);
    operation
}

pub fn release_pending_residual_handler(
    ctx: Context<ReleaseLegacyV6PendingResidual>,
    args: ReleaseLegacyV6PendingResidualArgs,
) -> Result<()> {
    require_compact_assertion(&args.client_data_json, &args.authenticator_data)?;
    let vault_key = ctx.accounts.vault.key();
    let attestation_key = ctx.accounts.attestation.key();
    let resolution_key = ctx.accounts.voucher_resolution.key();
    validate_resolution(
        ctx.program_id,
        &vault_key,
        &ctx.accounts.vault,
        attestation_key,
        &ctx.accounts.attestation,
        resolution_key,
        &ctx.accounts.voucher_resolution,
    )?;
    let now = Clock::get()?.unix_timestamp;
    let exact_counter = exact_historical_pending_count(&ctx.accounts.voucher_resolution)?;
    let unreserved_counter = unreserved_historical_pending_count(&ctx.accounts.voucher_resolution)?;
    require!(
        now > ctx.accounts.voucher_resolution.settlement_deadline
            && args.release_count > 0
            && ctx.accounts.vault.pending_voucher_count == args.expected_pending_voucher_count
            && args.expected_pending_voucher_count == exact_counter
            && args.release_count <= unreserved_counter,
        VaultError::LegacyV6PendingVoucherAmbiguous
    );

    let operation = build_pending_residual_release_operation(
        &vault_key,
        &attestation_key,
        &resolution_key,
        &ctx.accounts.attestation.original_data_sha256,
        ctx.accounts.voucher_resolution.recovered_at,
        ctx.accounts.voucher_resolution.settlement_deadline,
        ctx.accounts.voucher_resolution.original_pending_count,
        args.release_count,
        args.expected_pending_voucher_count,
    );
    verify_and_consume_passkey_authorization_one_assertion(
        &ctx.accounts.instructions_sysvar,
        &ctx.accounts.vault.passkey_pubkey,
        &mut ctx.accounts.passkey_authorization,
        &vault_key,
        &args.client_data_json,
        &args.authenticator_data,
        &operation,
    )?;

    ctx.accounts.vault.pending_voucher_count = args
        .expected_pending_voucher_count
        .checked_sub(args.release_count)
        .ok_or_else(|| error!(VaultError::LegacyV6PendingVoucherAmbiguous))?;
    let resolution = &mut ctx.accounts.voucher_resolution;
    resolution.owner_released_pending_count = resolution
        .owner_released_pending_count
        .checked_add(args.release_count)
        .ok_or_else(|| error!(VaultError::LegacyV6PendingVoucherAmbiguous))?;
    resolution.last_owner_release_at = Some(now);
    maybe_complete_legacy_v6_resolution(&mut ctx.accounts.vault, &ctx.accounts.voucher_resolution)?;
    Ok(())
}

pub fn release_session_handler(
    ctx: Context<ReleaseLegacyV6SessionReservation>,
    args: ReleaseLegacyV6SessionReservationArgs,
) -> Result<()> {
    require_compact_assertion(&args.client_data_json, &args.authenticator_data)?;
    let vault_key = ctx.accounts.vault.key();
    let attestation_key = ctx.accounts.attestation.key();
    let resolution_key = ctx.accounts.voucher_resolution.key();
    validate_resolution(
        ctx.program_id,
        &vault_key,
        &ctx.accounts.vault,
        attestation_key,
        &ctx.accounts.attestation,
        resolution_key,
        &ctx.accounts.voucher_resolution,
    )?;
    let now = Clock::get()?.unix_timestamp;
    require_owner_release_window(now, ctx.accounts.voucher_resolution.settlement_deadline)?;

    let index = usize::from(args.session_index);
    require!(
        index < usize::from(ctx.accounts.voucher_resolution.session_count),
        VaultError::LegacyV6SessionSnapshotMismatch
    );
    let snapshot = ctx.accounts.attestation.session_attestations[index];
    let state = ctx.accounts.voucher_resolution.sessions[index];
    require!(
        state.present
            && state.session == snapshot.session_account
            && matches!(
                state.state,
                LEGACY_V6_SESSION_RESOLUTION_NEUTRAL | LEGACY_V6_SESSION_RESOLUTION_OWNER_BOUND
            )
            && state.remaining_outstanding == snapshot.current_outstanding,
        VaultError::LegacyV6SessionSnapshotMismatch
    );
    let session_data_sha256 =
        normalized_recovered_session_sha256(&ctx.accounts.session.to_account_info())?;
    require_exact_session(
        &vault_key,
        ctx.accounts.session.key(),
        &ctx.accounts.session,
        session_data_sha256,
        &snapshot,
    )?;
    let snapshot_sha256 = session_attestation_sha256(&snapshot)?;
    let operation = build_session_release_operation(
        &vault_key,
        &attestation_key,
        &resolution_key,
        &ctx.accounts.attestation.original_data_sha256,
        ctx.accounts.voucher_resolution.settlement_deadline,
        args.session_index,
        &snapshot_sha256,
        snapshot.current_outstanding,
    );
    verify_and_consume_passkey_authorization_one_assertion(
        &ctx.accounts.instructions_sysvar,
        &ctx.accounts.vault.passkey_pubkey,
        &mut ctx.accounts.passkey_authorization,
        &vault_key,
        &args.client_data_json,
        &args.authenticator_data,
        &operation,
    )?;

    if state.counter_reserved {
        ctx.accounts.vault.pending_voucher_count = ctx
            .accounts
            .vault
            .pending_voucher_count
            .checked_sub(1)
            .ok_or_else(|| error!(VaultError::LegacyV6PendingVoucherAmbiguous))?;
        ctx.accounts.voucher_resolution.owner_released_pending_count = ctx
            .accounts
            .voucher_resolution
            .owner_released_pending_count
            .checked_add(1)
            .ok_or_else(|| error!(VaultError::LegacyV6PendingVoucherAmbiguous))?;
    }
    let state = &mut ctx.accounts.voucher_resolution.sessions[index];
    state.state = LEGACY_V6_SESSION_RESOLUTION_OWNER_RELEASED;
    state.counter_reserved = false;
    state.remaining_outstanding = 0;
    retire_recovered_session(&mut ctx.accounts.vault, &mut ctx.accounts.session)?;
    maybe_complete_legacy_v6_resolution(&mut ctx.accounts.vault, &ctx.accounts.voucher_resolution)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolution() -> LegacyV6VoucherResolution {
        LegacyV6VoucherResolution {
            version: LEGACY_V6_VOUCHER_RESOLUTION_VERSION_V1,
            bump: 1,
            vault: Pubkey::new_unique(),
            attestation: Pubkey::new_unique(),
            recovered_at: 10,
            settlement_deadline: 20,
            original_pending_count: 4,
            session_count: 0,
            sessions: [LegacyV6SessionResolution::default(); LEGACY_V6_RECOVERY_MAX_SESSIONS],
            terminal_consumed_pending_count: 0,
            owner_released_pending_count: 0,
            last_owner_release_at: None,
        }
    }

    fn session_attestation() -> LegacyV6SessionAttestation {
        LegacyV6SessionAttestation {
            present: true,
            session_account: Pubkey::new_unique(),
            account_version: SESSION_VERSION_V1,
            account_bump: 250,
            vault: Pubkey::new_unique(),
            session_pubkey: [4u8; 32],
            max_amount: 20_000,
            expires_at: -1,
            allowed_counterparty: Pubkey::new_unique(),
            nonce: 7,
            spent: 123,
            current_outstanding: 10_000,
            max_revolving_capacity: 10_000,
            crystallized_cumulative: 99,
            last_locked_sequence: 4,
            recovered_data_sha256: [5u8; 32],
        }
    }

    #[test]
    fn exact_counter_accounting_preserves_partial_residual_without_inference() {
        let mut value = resolution();
        assert_eq!(exact_historical_pending_count(&value).unwrap(), 4);
        value.terminal_consumed_pending_count = 1;
        assert_eq!(exact_historical_pending_count(&value).unwrap(), 3);
        value.owner_released_pending_count = 2;
        assert_eq!(exact_historical_pending_count(&value).unwrap(), 1);
        value.owner_released_pending_count = 4;
        assert!(exact_historical_pending_count(&value).is_err());
    }

    #[test]
    fn owner_bound_release_cannot_race_the_sellers_full_deadline() {
        assert!(require_owner_release_window(19, 20).is_err());
        assert!(require_owner_release_window(20, 20).is_err());
        assert!(require_owner_release_window(21, 20).is_ok());
    }

    #[test]
    fn residual_release_cannot_consume_an_owner_bound_counter_reservation() {
        let mut value = resolution();
        value.original_pending_count = 2;
        value.session_count = 2;
        value.sessions[0] = LegacyV6SessionResolution {
            present: true,
            session: Pubkey::new_unique(),
            binding: Pubkey::new_unique(),
            counter_reserved: true,
            state: LEGACY_V6_SESSION_RESOLUTION_OWNER_BOUND,
            remaining_outstanding: 10,
        };
        value.sessions[1] = LegacyV6SessionResolution {
            present: true,
            session: Pubkey::new_unique(),
            binding: Pubkey::new_unique(),
            counter_reserved: false,
            state: LEGACY_V6_SESSION_RESOLUTION_OWNER_BOUND,
            remaining_outstanding: 10,
        };
        assert_eq!(exact_historical_pending_count(&value).unwrap(), 2);
        assert_eq!(reserved_historical_pending_count(&value).unwrap(), 1);
        assert_eq!(unreserved_historical_pending_count(&value).unwrap(), 1);
    }

    #[test]
    fn release_operations_bind_distinct_terminal_and_every_session_field() {
        let vault = Pubkey::new_unique();
        let attestation = Pubkey::new_unique();
        let resolution = Pubkey::new_unique();
        let snapshot = session_attestation();
        let snapshot_hash = session_attestation_sha256(&snapshot).unwrap();
        let residual = build_pending_residual_release_operation(
            &vault,
            &attestation,
            &resolution,
            &[1u8; 32],
            2,
            3,
            4,
            1,
            4,
        );
        let session = build_session_release_operation(
            &vault,
            &attestation,
            &resolution,
            &[1u8; 32],
            3,
            0,
            &snapshot_hash,
            snapshot.current_outstanding,
        );
        assert_eq!(residual.len(), 252);
        assert_eq!(session.len(), 273);
        assert_ne!(hashv(&[&residual]), hashv(&[&session]));

        let mut changed = snapshot;
        changed.last_locked_sequence += 1;
        let changed_hash = session_attestation_sha256(&changed).unwrap();
        assert_ne!(snapshot_hash, changed_hash);
        assert_ne!(
            hashv(&[&session]),
            hashv(&[&build_session_release_operation(
                &vault,
                &attestation,
                &resolution,
                &[1u8; 32],
                3,
                0,
                &changed_hash,
                changed.current_outstanding,
            )])
        );
    }
}
