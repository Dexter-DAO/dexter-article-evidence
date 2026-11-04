use anchor_lang::prelude::*;

use crate::state::{
    PrincipalNode, SessionAccount, SessionRegistration, VaultError, SESSION_VERSION_V1,
};
use crate::verify::voucher::is_v2_registration_nonce;

/// Inputs shared by first-touch registration, expired-target re-registration,
/// and the one-ceremony live-key replacement path.
pub(crate) struct SessionAdmissionParams {
    pub vault_key: Pubkey,
    pub vault_node: Pubkey,
    pub target_session_key: Pubkey,
    /// True when the target is already represented in `live_session_count`.
    /// Remaining accounts must contain every *other* session either way.
    pub target_already_counted: bool,
    pub live_session_count: u8,
    pub outstanding_locked_amount: u64,
    pub own_usdc_amount: u64,
    /// Paused credit is not drawable backing even when the welded node still
    /// has a nonzero velocity bucket.
    pub credit_enabled: bool,
    pub max_depth: u8,
    pub now: i64,
}

/// Exact-state inputs for opening one Native Tab V2 reservation. Unlike
/// registration admission, this gate counts existing *actual outstanding*
/// session exposure rather than every live session's lifetime cap.
pub(crate) struct TabReservationBackingParams {
    pub vault_key: Pubkey,
    pub target_session_key: Pubkey,
    pub live_session_count: u8,
    pub outstanding_locked_amount: u64,
    pub own_usdc_amount: u64,
    pub requested_reservation: u64,
    pub now: i64,
}

pub(crate) struct SessionAdmissionOutcome {
    pub swept: u8,
    /// Exact number of supplied V2 sibling sessions carrying a seller
    /// reservation. At open, the target is proven zero before this census, so
    /// this must equal the Vault's pre-open global pending counter.
    pub outstanding_sessions: u32,
}

#[derive(Clone, Copy)]
enum SiblingExposureBasis {
    /// Pre-upgrade registration/replacement basis: reserve each issuance-valid
    /// sibling's full lifetime cap. A (audit Q5a) RETIRED this basis — a live
    /// tab never sets money aside, so registration no longer blocks on
    /// cap-vs-balance. Kept only to document and test the behaviour A replaced;
    /// no production caller constructs it any more (hence allow(dead_code)).
    #[allow(dead_code)]
    RegistrationCapacity,
    /// The sole live basis: re-prove only obligations actually outstanding now.
    /// Used by both tab open and (after A) registration/replacement.
    OutstandingReservations,
}

fn sibling_exposure(
    registration: &SessionRegistration,
    issuance_valid: bool,
    basis: SiblingExposureBasis,
) -> u64 {
    match basis {
        SiblingExposureBasis::RegistrationCapacity if issuance_valid => registration.max_amount,
        SiblingExposureBasis::RegistrationCapacity
        | SiblingExposureBasis::OutstandingReservations => registration.current_outstanding,
    }
}

/// Preserve a grandfathered V1 sibling without letting generic V7 admission
/// reinterpret or erase a seller obligation.
///
/// A low-bit registration with nonzero outstanding remains exclusively behind
/// the owner-bound recovery bridge.  Once outstanding is exactly zero, there
/// is no seller claim to reinterpret: an issuance-valid sibling still reserves
/// its full lifetime cap, while an expired sibling reserves no economic
/// exposure.  Both remain byte-for-byte intact and counted in the Vault census
/// until the owner explicitly revokes them.  This lets a V2 session coexist
/// with harmless historical grants without silently retiring those grants.
fn retained_legacy_v1_exposure(
    registration: &SessionRegistration,
    issuance_valid: bool,
    basis: SiblingExposureBasis,
) -> Result<Option<u64>> {
    if is_v2_registration_nonce(registration.nonce) {
        return Ok(None);
    }

    require!(
        registration.current_outstanding == 0,
        VaultError::LegacyV1TabBindingMismatch
    );

    Ok(Some(match basis {
        SiblingExposureBasis::RegistrationCapacity if issuance_valid => registration.max_amount,
        SiblingExposureBasis::RegistrationCapacity
        | SiblingExposureBasis::OutstandingReservations => 0,
    }))
}

struct SessionBackingParams {
    vault_key: Pubkey,
    vault_node: Pubkey,
    target_session_key: Pubkey,
    target_already_counted: bool,
    live_session_count: u8,
    outstanding_locked_amount: u64,
    own_usdc_amount: u64,
    requested_exposure: u64,
    credit_enabled: bool,
    max_depth: u8,
    sibling_exposure_basis: SiblingExposureBasis,
    now: i64,
}

/// Apply the complete V7 session-admission gate.
///
/// `remaining_accounts` is the complete set of other SessionAccount PDAs in
/// strict ascending order, plus at most the vault's welded PrincipalNode at any
/// position. Issuance-valid siblings contribute their caps. Expired V2
/// siblings are cleared only when their exact outstanding is zero; an expired
/// V2 obligation remains counted and contributes its outstanding amount to
/// backing. Grandfathered V1 siblings are preserved until explicit owner
/// revocation and are accepted here only when their outstanding is exactly zero.
pub(crate) fn enforce_session_admission<'info>(
    program_id: &Pubkey,
    remaining_accounts: &'info [AccountInfo<'info>],
    params: SessionAdmissionParams,
) -> Result<SessionAdmissionOutcome> {
    enforce_session_backing(
        program_id,
        remaining_accounts,
        SessionBackingParams {
            vault_key: params.vault_key,
            vault_node: params.vault_node,
            target_session_key: params.target_session_key,
            target_already_counted: params.target_already_counted,
            live_session_count: params.live_session_count,
            outstanding_locked_amount: params.outstanding_locked_amount,
            own_usdc_amount: params.own_usdc_amount,
            // A (audit Q5a): registration admits on what is ACTUALLY OWED, not
            // on lifetime caps. A live tab never sets money aside — only an open
            // reservation does (audit Q2) — so a fresh/replaced session's own
            // contribution is its current_outstanding, which is always 0 here
            // (register/replace prove the target carries no reservation). The
            // sole money gate for a seller stays the reserve-time check
            // (settle_voucher -> enforce_tab_reservation_backing), which already
            // uses OutstandingReservations and is exact cash-only — Invariant 1
            // is preserved because settle_voucher is unchanged. new_max_amount
            // is still stored by the caller as the revolving cap; it just no
            // longer blocks registration on cap-vs-balance.
            requested_exposure: 0,
            credit_enabled: params.credit_enabled,
            max_depth: params.max_depth,
            sibling_exposure_basis: SiblingExposureBasis::OutstandingReservations,
            now: params.now,
        },
    )
}

/// Re-prove economic backing at the exact Native Tab V2 open state.
///
/// The target session is already represented in `live_session_count` and must
/// be excluded from the complete sibling tail. Every other non-cleared session
/// is still passed, ordered, and authenticated, but only its current outstanding
/// reservation consumes backing. Expired zero-outstanding V2 siblings are swept
/// by the same authoritative parser as registration; V1 siblings remain
/// preserved and count toward the complete census.
pub(crate) fn enforce_tab_reservation_backing<'info>(
    program_id: &Pubkey,
    remaining_accounts: &'info [AccountInfo<'info>],
    params: TabReservationBackingParams,
) -> Result<SessionAdmissionOutcome> {
    enforce_session_backing(
        program_id,
        remaining_accounts,
        SessionBackingParams {
            vault_key: params.vault_key,
            // Native Tab V2 lock-mode backing is deliberately cash-only. A
            // drawable graph line is not reserved between open and terminal.
            // Pubkey::default also makes any supplied node tail fail closed.
            vault_node: Pubkey::default(),
            target_session_key: params.target_session_key,
            target_already_counted: true,
            live_session_count: params.live_session_count,
            outstanding_locked_amount: params.outstanding_locked_amount,
            own_usdc_amount: params.own_usdc_amount,
            requested_exposure: params.requested_reservation,
            credit_enabled: false,
            max_depth: crate::constants::MAX_DEPTH,
            sibling_exposure_basis: SiblingExposureBasis::OutstandingReservations,
            now: params.now,
        },
    )
}

/// One parser and one conservation check for every session-backing decision.
/// The caller selects whether issuance-valid siblings contribute their full
/// registration cap or only their already-open reservation.
fn enforce_session_backing<'info>(
    program_id: &Pubkey,
    remaining_accounts: &'info [AccountInfo<'info>],
    params: SessionBackingParams,
) -> Result<SessionAdmissionOutcome> {
    let mut sibling_sum: u64 = 0;
    let mut live_counted: u8 = 0;
    let mut swept: u8 = 0;
    let mut outstanding_sessions: u32 = 0;
    let mut prev: Option<Pubkey> = None;
    let mut node_chain: Vec<&AccountInfo<'info>> = Vec::new();
    let mut node_tail_started = false;

    for acct in remaining_accounts.iter() {
        let acct_key = *acct.key;

        // PrincipalNode accounts form one explicit tail AFTER the sorted
        // SessionAccount set. The first must be the Vault's welded leaf and
        // every following account its exact parent through the rooted root.
        if acct
            .try_borrow_data()
            .map(|data| data.len() >= 8 && &data[..8] == PrincipalNode::DISCRIMINATOR)
            .unwrap_or(false)
        {
            node_tail_started = true;
            node_chain.push(acct);
            continue;
        }

        // Once the node tail begins, no further sibling can be interleaved.
        // This gives the account tail one canonical representation.
        require!(!node_tail_started, VaultError::SessionAccountsNotSorted);

        // Strict ordering is both the canonical representation and dedup gate.
        if let Some(previous) = prev {
            verify_strict_sibling_order(previous, acct_key)?;
        }
        prev = Some(acct_key);

        require_keys_neq!(
            acct_key,
            params.target_session_key,
            VaultError::SessionAccountMisderived
        );

        // Account::try_from proves this program owns the account and that its
        // Anchor discriminator is SessionAccount's discriminator.
        let mut sibling = Account::<SessionAccount>::try_from(acct)
            .map_err(|_| error!(VaultError::SessionAccountForeign))?;
        // A complete sibling fetched moments ago can be cleared by a winning
        // transaction before this one executes. Classify that version-0 race as
        // IncompleteSessionSet so the bounded SDK loop refetches and rebuilds.
        // Unsupported nonzero versions remain a hard foreign-account failure.
        validate_sibling_version(sibling.version)?;
        require_keys_eq!(
            sibling.vault,
            params.vault_key,
            VaultError::SessionAccountForeign
        );

        let expected = session_pda_from_stored_bump(
            program_id,
            &params.vault_key,
            &sibling.session.allowed_counterparty,
            sibling.bump,
        )?;
        require_keys_eq!(acct_key, expected, VaultError::SessionAccountMisderived);

        let issuance_valid = sibling.session.expires_at > params.now;
        if let Some(exposure) = retained_legacy_v1_exposure(
            &sibling.session,
            issuance_valid,
            params.sibling_exposure_basis,
        )? {
            sibling_sum = sibling_sum
                .checked_add(exposure)
                .ok_or_else(|| error!(VaultError::SessionWouldOvercommitVault))?;
            live_counted = live_counted
                .checked_add(1)
                .ok_or_else(|| error!(VaultError::SessionWouldOvercommitVault))?;
            continue;
        }

        let has_outstanding = sibling.session.current_outstanding > 0;
        if has_outstanding {
            outstanding_sessions = outstanding_sessions
                .checked_add(1)
                .ok_or_else(|| error!(VaultError::NativeTabCounterMismatch))?;
        }
        if issuance_valid || has_outstanding {
            let exposure = sibling_exposure(
                &sibling.session,
                issuance_valid,
                params.sibling_exposure_basis,
            );
            sibling_sum = sibling_sum
                .checked_add(exposure)
                .ok_or_else(|| error!(VaultError::SessionWouldOvercommitVault))?;
            live_counted = live_counted
                .checked_add(1)
                .ok_or_else(|| error!(VaultError::SessionWouldOvercommitVault))?;
        } else {
            require!(acct.is_writable, VaultError::SessionAccountNotWritable);
            sibling.version = 0;
            sibling.session = cleared_session_registration();
            sibling.exit(program_id)?;
            swept = swept
                .checked_add(1)
                .ok_or_else(|| error!(VaultError::SessionWouldOvercommitVault))?;
        }
    }

    verify_complete_session_set(
        live_counted,
        swept,
        params.live_session_count,
        params.target_already_counted,
    )?;
    let standby_available = if node_chain.is_empty() || !params.credit_enabled {
        // Omitting the chain or pausing the graph can only understate backing.
        // If a chain was supplied while paused, still authenticate it so an
        // arbitrary discriminator-shaped tail cannot be silently accepted.
        if !node_chain.is_empty() {
            crate::instructions::ancestor_traverse::pro_forma_authenticated_drawable(
                program_id,
                &params.vault_node,
                &node_chain,
                params.now,
                params.max_depth,
            )?;
        }
        0
    } else {
        crate::instructions::ancestor_traverse::pro_forma_authenticated_drawable(
            program_id,
            &params.vault_node,
            &node_chain,
            params.now,
            params.max_depth,
        )?
    };
    verify_session_backing(
        sibling_sum,
        params.requested_exposure,
        params.outstanding_locked_amount,
        params.own_usdc_amount,
        standby_available,
    )?;

    Ok(SessionAdmissionOutcome {
        swept,
        outstanding_sessions,
    })
}

pub(crate) fn reconciled_live_session_count(
    current: u8,
    swept: u8,
    increment_for_new_target: bool,
) -> Result<u8> {
    let reconciled = current
        .checked_sub(swept)
        .ok_or_else(|| error!(VaultError::IncompleteSessionSet))?;
    if increment_for_new_target {
        let next = reconciled
            .checked_add(1)
            .ok_or_else(|| error!(VaultError::SessionCountAtMax))?;
        require!(
            next <= crate::constants::SESSION_FLEET_MAX,
            VaultError::SessionCountAtMax
        );
        Ok(next)
    } else {
        Ok(reconciled)
    }
}

fn session_pda_from_stored_bump(
    program_id: &Pubkey,
    vault: &Pubkey,
    counterparty: &Pubkey,
    bump: u8,
) -> Result<Pubkey> {
    Pubkey::create_program_address(
        &[
            crate::constants::SESSION_SEED,
            vault.as_ref(),
            counterparty.as_ref(),
            &[bump],
        ],
        program_id,
    )
    .map_err(|_| error!(VaultError::SessionAccountMisderived))
}

fn validate_sibling_version(version: u8) -> Result<()> {
    if version == 0 {
        return err!(VaultError::IncompleteSessionSet);
    }
    require!(
        version == SESSION_VERSION_V1,
        VaultError::SessionAccountForeign
    );
    Ok(())
}

fn verify_complete_session_set(
    live_counted: u8,
    swept: u8,
    live_session_count: u8,
    target_already_counted: bool,
) -> Result<()> {
    let total_passed = live_counted
        .checked_add(swept)
        .ok_or_else(|| error!(VaultError::IncompleteSessionSet))?;
    let expected_total = live_session_count
        .checked_sub(u8::from(target_already_counted))
        .ok_or_else(|| error!(VaultError::IncompleteSessionSet))?;
    require!(
        total_passed == expected_total,
        VaultError::IncompleteSessionSet
    );
    Ok(())
}

fn verify_strict_sibling_order(previous: Pubkey, current: Pubkey) -> Result<()> {
    require!(current > previous, VaultError::SessionAccountsNotSorted);
    Ok(())
}

fn verify_session_backing(
    sibling_sum: u64,
    requested_exposure: u64,
    outstanding_locked_amount: u64,
    own_usdc_amount: u64,
    standby_available: u64,
) -> Result<()> {
    let combined = sibling_sum
        .checked_add(requested_exposure)
        .and_then(|sum| sum.checked_add(outstanding_locked_amount))
        .ok_or_else(|| error!(VaultError::SessionWouldOvercommitVault))?;
    let backing = own_usdc_amount
        .checked_add(standby_available)
        .ok_or_else(|| error!(VaultError::SessionWouldOvercommitVault))?;
    require!(combined <= backing, VaultError::SessionWouldOvercommitVault);
    Ok(())
}

fn cleared_session_registration() -> SessionRegistration {
    SessionRegistration {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn serialized_session_account(
        program_id: Pubkey,
        vault: Pubkey,
        counterparty: Pubkey,
        nonce: u32,
        max_amount: u64,
        expires_at: i64,
        current_outstanding: u64,
    ) -> AccountInfo<'static> {
        let (key, bump) = Pubkey::find_program_address(
            &[
                crate::constants::SESSION_SEED,
                vault.as_ref(),
                counterparty.as_ref(),
            ],
            &program_id,
        );
        let value = SessionAccount {
            version: SESSION_VERSION_V1,
            bump,
            vault,
            session: SessionRegistration {
                session_pubkey: [7; 32],
                max_amount,
                expires_at,
                allowed_counterparty: counterparty,
                nonce,
                spent: 0,
                current_outstanding,
                max_revolving_capacity: max_amount,
                crystallized_cumulative: 0,
                last_locked_sequence: 0,
            },
        };
        let mut data = vec![0_u8; 8 + SessionAccount::INIT_SPACE];
        let mut writer: &mut [u8] = &mut data;
        value.try_serialize(&mut writer).unwrap();

        AccountInfo::new(
            Box::leak(Box::new(key)),
            false,
            true,
            Box::leak(Box::new(1_000_000_u64)),
            Box::leak(data.into_boxed_slice()),
            Box::leak(Box::new(program_id)),
            false,
            0,
        )
    }

    fn shortvec_len(value: usize) -> usize {
        if value < 128 {
            1
        } else if value < 16_384 {
            2
        } else {
            3
        }
    }

    fn legacy_transaction_wire_size(
        signatures: usize,
        account_keys: usize,
        instruction_account_counts: &[usize],
        instruction_data_lengths: &[usize],
    ) -> usize {
        shortvec_len(signatures)
            + signatures * 64
            + 3
            + shortvec_len(account_keys)
            + account_keys * 32
            + 32
            + shortvec_len(instruction_account_counts.len())
            + instruction_account_counts
                .iter()
                .zip(instruction_data_lengths)
                .map(|(accounts, data)| {
                    1 + shortvec_len(*accounts) + accounts + shortvec_len(*data) + data
                })
                .sum::<usize>()
    }

    fn v0_transaction_wire_size_one_lookup(
        signatures: usize,
        static_account_keys: usize,
        instruction_account_counts: &[usize],
        instruction_data_lengths: &[usize],
        loaded_writable: usize,
        loaded_readonly: usize,
    ) -> usize {
        // Signature vector + version prefix + v0 message header/static keys.
        shortvec_len(signatures)
            + signatures * 64
            + 1
            + 3
            + shortvec_len(static_account_keys)
            + static_account_keys * 32
            + 32
            + shortvec_len(instruction_account_counts.len())
            + instruction_account_counts
                .iter()
                .zip(instruction_data_lengths)
                .map(|(accounts, data)| {
                    1 + shortvec_len(*accounts) + accounts + shortvec_len(*data) + data
                })
                .sum::<usize>()
            // One lookup: table-account index, writable index vector, and
            // readonly index vector.
            + shortvec_len(1)
            + 1
            + shortvec_len(loaded_writable)
            + loaded_writable
            + shortvec_len(loaded_readonly)
            + loaded_readonly
    }

    #[test]
    fn completeness_excludes_exactly_one_existing_target() {
        assert!(verify_complete_session_set(2, 1, 4, true).is_ok());
        assert!(verify_complete_session_set(1, 1, 4, true).is_err());
        assert!(verify_complete_session_set(3, 1, 4, true).is_err());
        assert!(verify_complete_session_set(2, 1, 3, false).is_ok());
    }

    #[test]
    fn backing_excludes_old_target_cap_and_includes_new_cap() {
        assert!(verify_session_backing(20, 30, 10, 50, 10).is_ok());
        assert!(verify_session_backing(20, 31, 10, 50, 10).is_err());
        assert!(verify_session_backing(u64::MAX, 1, 0, u64::MAX, 0).is_err());
        assert!(verify_session_backing(0, 1, 0, u64::MAX, 1).is_err());
    }

    #[test]
    fn foreign_or_noncanonical_session_pda_is_detectable() {
        let program_id = crate::ID;
        let vault = Pubkey::new_unique();
        let counterparty = Pubkey::new_unique();
        let (canonical, bump) = Pubkey::find_program_address(
            &[
                crate::constants::SESSION_SEED,
                vault.as_ref(),
                counterparty.as_ref(),
            ],
            &program_id,
        );
        assert_eq!(
            session_pda_from_stored_bump(&program_id, &vault, &counterparty, bump).unwrap(),
            canonical
        );
        assert_ne!(
            session_pda_from_stored_bump(&program_id, &vault, &Pubkey::new_unique(), bump,).ok(),
            Some(canonical)
        );
    }

    #[test]
    fn count_reconciliation_only_removes_swept_siblings_for_replace() {
        assert_eq!(reconciled_live_session_count(4, 2, false).unwrap(), 2);
        assert_eq!(
            reconciled_live_session_count(u8::MAX, 0, false).unwrap(),
            u8::MAX
        );
        assert_eq!(reconciled_live_session_count(4, 2, true).unwrap(), 3);
        assert_eq!(reconciled_live_session_count(6, 1, true).unwrap(), 6);
        assert!(reconciled_live_session_count(6, 0, true).is_err());
        assert!(reconciled_live_session_count(1, 2, false).is_err());
        assert!(reconciled_live_session_count(255, 0, true).is_err());
    }

    #[test]
    fn duplicate_or_unsorted_siblings_are_rejected() {
        let lower = Pubkey::new_from_array([1u8; 32]);
        let higher = Pubkey::new_from_array([2u8; 32]);
        assert!(verify_strict_sibling_order(lower, higher).is_ok());
        assert!(verify_strict_sibling_order(lower, lower).is_err());
        assert!(verify_strict_sibling_order(higher, lower).is_err());
    }

    #[test]
    fn concurrently_cleared_sibling_is_retryable_but_unknown_version_is_not() {
        assert!(validate_sibling_version(SESSION_VERSION_V1).is_ok());
        assert!(validate_sibling_version(0)
            .unwrap_err()
            .to_string()
            .contains("IncompleteSessionSet"));
        assert!(validate_sibling_version(SESSION_VERSION_V1 + 1)
            .unwrap_err()
            .to_string()
            .contains("SessionAccountForeign"));
    }

    #[test]
    fn expired_outstanding_is_retained_and_backed_not_swept() {
        let mut registration = cleared_session_registration();
        registration.expires_at = 100;
        registration.max_amount = 1_000;
        registration.current_outstanding = 75;
        let issuance_valid = registration.expires_at > 100;
        let has_outstanding = registration.current_outstanding > 0;
        let exposure = if issuance_valid {
            registration.max_amount
        } else {
            registration.current_outstanding
        };
        assert!(!issuance_valid);
        assert!(has_outstanding);
        assert_eq!(exposure, 75);
        assert_eq!(registration.current_outstanding, 75);
    }

    #[test]
    fn zero_outstanding_legacy_sibling_is_preserved_with_conservative_exposure() {
        let mut registration = cleared_session_registration();
        registration.nonce = 17;
        registration.expires_at = 200;
        registration.max_amount = 1_000;
        registration.current_outstanding = 0;

        assert_eq!(
            retained_legacy_v1_exposure(
                &registration,
                true,
                SiblingExposureBasis::RegistrationCapacity,
            )
            .unwrap(),
            Some(1_000)
        );
        assert_eq!(
            retained_legacy_v1_exposure(
                &registration,
                true,
                SiblingExposureBasis::OutstandingReservations,
            )
            .unwrap(),
            Some(0)
        );

        // Expiry removes future issuance exposure, but the grandfathered
        // account remains in the complete Vault census until owner revocation.
        assert_eq!(
            retained_legacy_v1_exposure(
                &registration,
                false,
                SiblingExposureBasis::RegistrationCapacity,
            )
            .unwrap(),
            Some(0)
        );
    }

    #[test]
    fn legacy_sibling_with_outstanding_remains_recovery_only() {
        let mut registration = cleared_session_registration();
        registration.nonce = 17;
        registration.expires_at = 200;
        registration.max_amount = 1_000;
        registration.current_outstanding = 1;

        for basis in [
            SiblingExposureBasis::RegistrationCapacity,
            SiblingExposureBasis::OutstandingReservations,
        ] {
            let error = retained_legacy_v1_exposure(&registration, true, basis).unwrap_err();
            assert!(error.to_string().contains("LegacyV1TabBindingMismatch"));
        }
    }

    #[test]
    fn v2_sibling_stays_on_the_existing_sweep_and_obligation_path() {
        let mut registration = cleared_session_registration();
        registration.nonce = crate::verify::voucher::SESSION_VOUCHER_V2_NONCE_FLAG | 17;
        registration.expires_at = 200;
        registration.max_amount = 1_000;

        assert_eq!(
            retained_legacy_v1_exposure(
                &registration,
                true,
                SiblingExposureBasis::RegistrationCapacity,
            )
            .unwrap(),
            None
        );
    }

    #[test]
    fn branch_shaped_legacy_census_allows_one_backed_v2_registration() {
        let now = 1_000;
        let mut live_caps = [2_000_000_u64, 500_000]
            .into_iter()
            .map(|max_amount| {
                let mut registration = cleared_session_registration();
                registration.nonce = 17;
                registration.expires_at = now + 1;
                registration.max_amount = max_amount;
                registration
            })
            .collect::<Vec<_>>();
        let mut expired = cleared_session_registration();
        expired.nonce = 17;
        expired.expires_at = now;
        expired.max_amount = 5_000_000;
        live_caps.push(expired);

        let sibling_exposure = live_caps
            .iter()
            .map(|registration| {
                retained_legacy_v1_exposure(
                    registration,
                    registration.expires_at > now,
                    SiblingExposureBasis::RegistrationCapacity,
                )
                .unwrap()
                .unwrap()
            })
            .try_fold(0_u64, u64::checked_add)
            .unwrap();

        assert_eq!(sibling_exposure, 2_500_000);
        assert!(verify_complete_session_set(3, 0, 3, false).is_ok());
        assert!(verify_session_backing(sibling_exposure, 5_000_000, 0, 22_054_000, 0,).is_ok());
        assert_eq!(reconciled_live_session_count(3, 0, true).unwrap(), 4);
    }

    #[test]
    fn branch_shaped_serialized_legacy_tail_is_accepted_and_never_swept() {
        let program_id = crate::ID;
        let vault = Pubkey::new_unique();
        let now = 1_000;
        let mut siblings = vec![
            serialized_session_account(
                program_id,
                vault,
                Pubkey::new_unique(),
                338_192_237,
                2_000_000,
                now + 1,
                0,
            ),
            serialized_session_account(
                program_id,
                vault,
                Pubkey::new_unique(),
                861_456_409,
                500_000,
                now + 1,
                0,
            ),
            serialized_session_account(
                program_id,
                vault,
                Pubkey::new_unique(),
                1_785_372_232,
                5_000_000,
                now,
                0,
            ),
        ];
        siblings.sort_by_key(|account| *account.key);
        let before = siblings
            .iter()
            .map(|account| account.try_borrow_data().unwrap().to_vec())
            .collect::<Vec<_>>();
        let siblings = Box::leak(siblings.into_boxed_slice());

        let outcome = enforce_session_admission(
            &program_id,
            siblings,
            SessionAdmissionParams {
                vault_key: vault,
                vault_node: Pubkey::default(),
                target_session_key: Pubkey::new_unique(),
                target_already_counted: false,
                live_session_count: 3,
                outstanding_locked_amount: 0,
                own_usdc_amount: 22_054_000,
                credit_enabled: false,
                max_depth: crate::constants::MAX_DEPTH,
                now,
            },
        )
        .unwrap();

        assert_eq!(outcome.swept, 0);
        assert_eq!(outcome.outstanding_sessions, 0);
        for (account, original) in siblings.iter().zip(before) {
            assert_eq!(account.try_borrow_data().unwrap().as_ref(), original);
        }
    }

    #[test]
    fn a_registration_admits_huge_cap_on_small_balance_when_nothing_outstanding() {
        // A (audit Q5a): a $100-cap tab on a $1 vault must register, because a
        // live tab reserves nothing — only an open reservation consumes backing.
        // The retired RegistrationCapacity basis rejected this (100 USDC cap >
        // 1 USDC cash); OutstandingReservations admits it (outstanding == 0).
        let program_id = crate::ID;
        let vault = Pubkey::new_unique();
        let now = 1_000;
        let siblings = vec![serialized_session_account(
            program_id,
            vault,
            Pubkey::new_unique(),
            crate::verify::voucher::SESSION_VOUCHER_V2_NONCE_FLAG | 1, // V2 session
            100_000_000,                                              // $100 cap
            now + 1_000,                                              // issuance-valid
            0,                                                        // nothing outstanding
        )];
        let siblings = Box::leak(siblings.into_boxed_slice());

        let outcome = enforce_session_admission(
            &program_id,
            siblings,
            SessionAdmissionParams {
                vault_key: vault,
                vault_node: Pubkey::default(),
                target_session_key: Pubkey::new_unique(),
                target_already_counted: false,
                live_session_count: 1,
                outstanding_locked_amount: 0,
                own_usdc_amount: 1_000_000, // $1 balance, far below the cap
                credit_enabled: false,
                max_depth: crate::constants::MAX_DEPTH,
                now,
            },
        )
        .unwrap();
        assert_eq!(outcome.swept, 0);
        assert_eq!(outcome.outstanding_sessions, 0);
    }

    #[test]
    fn a_registration_still_rejects_when_an_open_reservation_exceeds_cash() {
        // The reserve-time money gate is preserved: a sibling's already-open
        // reservation larger than cash (+ locked) must still overcommit-fail at
        // registration, so A relaxes caps without weakening the cash invariant.
        let program_id = crate::ID;
        let vault = Pubkey::new_unique();
        let now = 1_000;
        let siblings = vec![serialized_session_account(
            program_id,
            vault,
            Pubkey::new_unique(),
            crate::verify::voucher::SESSION_VOUCHER_V2_NONCE_FLAG | 1,
            100_000_000,
            now + 1_000,
            2_000_000, // $2 already reserved on this sibling
        )];
        let siblings = Box::leak(siblings.into_boxed_slice());

        let result = enforce_session_admission(
            &program_id,
            siblings,
            SessionAdmissionParams {
                vault_key: vault,
                vault_node: Pubkey::default(),
                target_session_key: Pubkey::new_unique(),
                target_already_counted: false,
                live_session_count: 1,
                outstanding_locked_amount: 0,
                own_usdc_amount: 1_000_000, // only $1 cash < $2 outstanding
                credit_enabled: false,
                max_depth: crate::constants::MAX_DEPTH,
                now,
            },
        );
        match result {
            Ok(_) => panic!("expected overcommit rejection"),
            Err(e) => assert!(format!("{e:?}").contains("SessionWouldOvercommitVault")),
        }
    }

    #[test]
    fn registration_reserves_caps_but_tab_open_counts_only_live_obligations() {
        let mut registration = cleared_session_registration();
        registration.expires_at = 200;
        registration.max_amount = 1_000;
        registration.current_outstanding = 75;

        assert_eq!(
            sibling_exposure(
                &registration,
                true,
                SiblingExposureBasis::RegistrationCapacity,
            ),
            1_000
        );
        assert_eq!(
            sibling_exposure(
                &registration,
                true,
                SiblingExposureBasis::OutstandingReservations,
            ),
            75
        );

        // Once issuance expires, either admission basis retains only the exact
        // already-open obligation until its seller terminal or recovery path.
        assert_eq!(
            sibling_exposure(
                &registration,
                false,
                SiblingExposureBasis::RegistrationCapacity,
            ),
            75
        );
    }

    #[test]
    fn native_tab_cash_conservation_is_exact_at_open() {
        // 30 locked claims + 40 in sibling reservations + 30 requested = 100.
        assert!(verify_session_backing(40, 30, 30, 100, 0).is_ok());
        assert!(verify_session_backing(40, 30, 30, 99, 0).is_err());

        // The generic registration primitive can count authenticated standby.
        // Native Tab's wrapper never supplies it: lock-mode credit is not
        // reserved and therefore cannot back a seller reservation.
        assert!(verify_session_backing(40, 30, 30, 0, 100).is_ok());
    }

    #[test]
    fn native_tab_open_with_complete_five_session_tail_fits_legacy_packet() {
        let args = crate::instructions::settle_voucher::SettleVoucherArgs {
            amount: u64::MAX,
            increment: true,
            allowed_counterparty: Pubkey::new_unique(),
        };
        let mut body = Vec::new();
        args.serialize(&mut body).unwrap();
        assert_eq!(body.len(), 41);

        // Separate fee payer + Dexter authority, two compute instructions,
        // eight fixed Open accounts, and all five possible sibling sessions.
        let wire = legacy_transaction_wire_size(2, 16, &[0, 0, 13], &[5, 9, 8 + body.len()]);
        assert_eq!(wire, 763);
        assert!(wire <= 1_232);
    }

    #[test]
    fn full_depth_registration_and_replacement_require_explicit_v0_alt() {
        const CLIENT_DATA_STRESS_LEN: usize = 400;
        const AUTHENTICATOR_DATA_LEN: usize = 37;
        const P256_INSTRUCTION_DATA_LEN: usize = 2 + 14 + 64 + 33 + AUTHENTICATOR_DATA_LEN + 32;

        let register = crate::instructions::register_session_key::RegisterSessionKeyArgs {
            session_pubkey: [7; 32],
            max_amount: u64::MAX,
            expires_at: i64::MAX,
            allowed_counterparty: Pubkey::new_unique(),
            nonce: u32::MAX,
            max_revolving_capacity: u64::MAX,
            client_data_json: vec![0; CLIENT_DATA_STRESS_LEN],
            authenticator_data: vec![0; AUTHENTICATOR_DATA_LEN],
        };
        let mut register_body = Vec::new();
        register.serialize(&mut register_body).unwrap();
        assert_eq!(register_body.len(), 537);

        let replace = crate::instructions::replace_session_key::ReplaceSessionKeyV1Args {
            authorization_nonce: u64::MAX,
            replacement_session_pubkey: [8; 32],
            replacement_max_amount: u64::MAX,
            replacement_expires_at: i64::MAX,
            replacement_allowed_counterparty: Pubkey::new_unique(),
            replacement_registration_nonce: u32::MAX,
            replacement_max_revolving_capacity: u64::MAX,
            client_data_json: vec![0; CLIENT_DATA_STRESS_LEN],
            authenticator_data: vec![0; AUTHENTICATOR_DATA_LEN],
        };
        let mut replace_body = Vec::new();
        replace.serialize(&mut replace_body).unwrap();
        assert_eq!(replace_body.len(), 545);

        // Protocol depth eight means nine node rows including the welded leaf,
        // plus all five possible siblings. Neither WebAuthn-heavy admission
        // instruction can preserve this invariant in a legacy packet.
        let register_legacy = legacy_transaction_wire_size(
            2,
            29,
            &[0, 0, 0, 25],
            &[5, 9, P256_INSTRUCTION_DATA_LEN, 8 + register_body.len()],
        );
        let replace_legacy = legacy_transaction_wire_size(
            1,
            27,
            &[0, 0, 0, 23],
            &[5, 9, P256_INSTRUCTION_DATA_LEN, 8 + replace_body.len()],
        );
        assert_eq!(register_legacy, 1_874);
        assert_eq!(replace_legacy, 1_752);
        assert!(register_legacy > 1_232);
        assert!(replace_legacy > 1_232);

        // The explicit builder contract loads every nonsigner/non-program
        // instruction account through one prepared ALT. Static keys are only
        // signers, executable programs, and the ALT account itself.
        let register_v0 = v0_transaction_wire_size_one_lookup(
            2,
            7,
            &[0, 0, 0, 25],
            &[5, 9, P256_INSTRUCTION_DATA_LEN, 8 + register_body.len()],
            8,
            15,
        );
        let replace_v0 = v0_transaction_wire_size_one_lookup(
            1,
            5,
            &[0, 0, 0, 23],
            &[5, 9, P256_INSTRUCTION_DATA_LEN, 8 + replace_body.len()],
            8,
            15,
        );
        assert_eq!(register_v0, 1_198);
        assert_eq!(replace_v0, 1_076);
        assert!(register_v0 <= 1_232);
        assert!(replace_v0 <= 1_232);
    }
}
