//! Current-owner binding and terminal helpers for quarantined legacy V1 Tabs.
//!
//! Historical V1 vouchers sign only `channel || cumulative || sequence`. They
//! do not identify a Vault, SessionAccount, counterparty, recovery receipt, or
//! terminal. Recovery therefore leaves them inert. This instruction uses the
//! current owner's passkey to create one permanent global
//! `(session_pubkey, channel_id)` binding for one exact voucher and exactly one
//! selected terminal. The existing settle/lock handlers consume that binding;
//! seller possession alone is never authority.

use anchor_lang::prelude::*;
use anchor_lang::solana_program::sysvar;
use solana_sha256_hasher::hashv;

use crate::constants::{
    LEGACY_V1_TAB_BINDING_SEED, LEGACY_V6_ATTESTATION_SEED, LEGACY_V6_VOUCHER_RESOLUTION_SEED,
    PASSKEY_AUTHORIZATION_SEED, SESSION_SEED,
};
use crate::instructions::resolve_legacy_v6_vouchers::{
    exact_historical_pending_count, maybe_complete_legacy_v6_resolution,
    normalized_recovered_session_sha256, require_exact_session, retire_recovered_session,
    unreserved_historical_pending_count, validate_resolution,
};
use crate::state::*;
use crate::verify::voucher::{build_session_voucher_message, is_v2_registration_nonce};
use crate::verify::webauthn::verify_and_consume_passkey_authorization_one_assertion;

const LEGACY_V1_BINDING_DOMAIN_V1: &[u8; 32] = b"DEXTER_V1_TAB_BINDING_V1\0\0\0\0\0\0\0\0";
const SOLANA_MAINNET_BETA_DOMAIN: &[u8; 32] = b"SOLANA_MAINNET_BETA\0\0\0\0\0\0\0\0\0\0\0\0\0";
const LEGACY_V1_BINDING_OPERATION_LEN: usize = 501;
const CLIENT_DATA_MAX_LEN: usize = 400;
const AUTHENTICATOR_DATA_LEN: usize = 37;
const LEGACY_V1_TAB_POLICY_LOCK: u8 = 1 << 0;
const LEGACY_V1_TAB_POLICY_CONSUME_COUNTER: u8 = 1 << 1;
const LEGACY_V1_TAB_POLICY_MASK: u8 =
    LEGACY_V1_TAB_POLICY_LOCK | LEGACY_V1_TAB_POLICY_CONSUME_COUNTER;

#[derive(Accounts)]
#[instruction(args: AuthorizeLegacyV1TabArgs)]
pub struct AuthorizeLegacyV1Tab<'info> {
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
        seeds = [
            SESSION_SEED,
            vault.key().as_ref(),
            session.session.allowed_counterparty.as_ref(),
        ],
        bump = session.bump,
        constraint = session.vault == vault.key() @ VaultError::LegacyV6SessionSnapshotMismatch,
    )]
    pub session: Box<Account<'info, SessionAccount>>,

    #[account(
        init,
        payer = payer,
        space = 8 + LegacyV1TabBinding::INIT_SPACE,
        seeds = [
            LEGACY_V1_TAB_BINDING_SEED,
            session.session.session_pubkey.as_ref(),
            args.channel_id.as_ref(),
        ],
        bump,
    )]
    pub binding: Box<Account<'info, LegacyV1TabBinding>>,

    #[account(
        mut,
        seeds = [PASSKEY_AUTHORIZATION_SEED, vault.key().as_ref()],
        bump = passkey_authorization.bump,
    )]
    pub passkey_authorization: Box<Account<'info, PasskeyAuthorization>>,

    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,

    /// CHECK: canonical instruction sysvar used by the one-assertion verifier.
    #[account(address = sysvar::instructions::ID)]
    pub instructions_sysvar: AccountInfo<'info>,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct AuthorizeLegacyV1TabArgs {
    pub channel_id: [u8; 32],
    pub cumulative_amount: u64,
    pub sequence_number: u32,
    /// Bit 0 selects lock instead of direct settle; bit 1 reserves one exact
    /// historical pending-counter unit. Every other bit must be zero.
    pub policy_flags: u8,
    pub expected_pending_count: u32,
    pub lock_holder: Pubkey,
    pub client_data_json: Vec<u8>,
    pub authenticator_data: [u8; AUTHENTICATOR_DATA_LEN],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LegacyV1TerminalPermit {
    pub session_index: usize,
    pub consume_pending_counter: bool,
}

fn require_compact_assertion(client_data_json: &[u8]) -> Result<()> {
    require!(
        client_data_json.len() <= CLIENT_DATA_MAX_LEN,
        VaultError::PasskeyVerificationFailed
    );
    Ok(())
}

fn decode_policy_flags(policy_flags: u8) -> Result<(u8, bool)> {
    require!(
        policy_flags & !LEGACY_V1_TAB_POLICY_MASK == 0,
        VaultError::LegacyV1TabTerminalMismatch
    );
    let terminal = if policy_flags & LEGACY_V1_TAB_POLICY_LOCK != 0 {
        LEGACY_V1_TAB_TERMINAL_LOCK
    } else {
        LEGACY_V1_TAB_TERMINAL_SETTLE
    };
    Ok((
        terminal,
        policy_flags & LEGACY_V1_TAB_POLICY_CONSUME_COUNTER != 0,
    ))
}

fn selected_lock_policy(terminal: u8) -> (Option<i64>, Option<i64>) {
    if terminal == LEGACY_V1_TAB_TERMINAL_LOCK {
        // Crystallization creates a durable transferable claim. It must not
        // become buyer-recoverable merely because the pre-lock seller window
        // elapsed, especially when the lock lands at that boundary.
        (None, None)
    } else {
        (None, None)
    }
}

fn legacy_v1_binding_pda(
    program_id: &Pubkey,
    session_pubkey: &[u8; 32],
    channel_id: &[u8; 32],
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[
            LEGACY_V1_TAB_BINDING_SEED,
            session_pubkey.as_ref(),
            channel_id.as_ref(),
        ],
        program_id,
    )
}

fn require_full_seller_window(now: i64, binding_expiry: i64, seller_deadline: i64) -> Result<()> {
    require!(
        binding_expiry == seller_deadline && now <= seller_deadline,
        VaultError::LegacyV1TabBindingMismatch
    );
    Ok(())
}

pub(crate) fn require_deterministic_lock_policy(
    maturity_at: Option<i64>,
    holder_recovery_at: Option<i64>,
) -> Result<()> {
    require!(
        maturity_at.is_none() && holder_recovery_at.is_none(),
        VaultError::VoucherLockPolicyMismatch
    );
    Ok(())
}

fn require_selected_policy(
    terminal: u8,
    lock_holder: Pubkey,
    lock_maturity_at: Option<i64>,
    lock_holder_recovery_at: Option<i64>,
) -> Result<()> {
    match terminal {
        LEGACY_V1_TAB_TERMINAL_SETTLE => require!(
            lock_holder == Pubkey::default()
                && lock_maturity_at.is_none()
                && lock_holder_recovery_at.is_none(),
            VaultError::LegacyV1TabTerminalMismatch
        ),
        LEGACY_V1_TAB_TERMINAL_LOCK => {
            require!(
                lock_holder != Pubkey::default(),
                VaultError::LegacyV1TabTerminalMismatch
            );
            require_deterministic_lock_policy(lock_maturity_at, lock_holder_recovery_at)?;
        }
        _ => return err!(VaultError::LegacyV1TabTerminalMismatch),
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn build_binding_operation(
    program_id: &Pubkey,
    vault: &Pubkey,
    session: &Pubkey,
    counterparty: &Pubkey,
    attestation: &Pubkey,
    resolution: &Pubkey,
    original_data_sha256: &[u8; 32],
    session_data_sha256: &[u8; 32],
    session_pubkey: &[u8; 32],
    channel_id: &[u8; 32],
    voucher_digest: &[u8; 32],
    cumulative_amount: u64,
    sequence_number: u32,
    session_index: u8,
    remaining_outstanding: u64,
    terminal: u8,
    consume_pending_counter: bool,
    expected_pending_count: u32,
    expires_at: i64,
    lock_holder: &Pubkey,
    lock_maturity_at: Option<i64>,
    lock_holder_recovery_at: Option<i64>,
) -> Vec<u8> {
    let mut operation = Vec::with_capacity(LEGACY_V1_BINDING_OPERATION_LEN);
    operation.extend_from_slice(LEGACY_V1_BINDING_DOMAIN_V1);
    operation.extend_from_slice(SOLANA_MAINNET_BETA_DOMAIN);
    operation.extend_from_slice(program_id.as_ref());
    operation.extend_from_slice(vault.as_ref());
    operation.extend_from_slice(session.as_ref());
    operation.extend_from_slice(counterparty.as_ref());
    operation.extend_from_slice(attestation.as_ref());
    operation.extend_from_slice(resolution.as_ref());
    operation.extend_from_slice(original_data_sha256);
    operation.extend_from_slice(session_data_sha256);
    operation.extend_from_slice(session_pubkey);
    operation.extend_from_slice(channel_id);
    operation.extend_from_slice(voucher_digest);
    operation.extend_from_slice(&cumulative_amount.to_le_bytes());
    operation.extend_from_slice(&sequence_number.to_le_bytes());
    operation.push(session_index);
    operation.extend_from_slice(&remaining_outstanding.to_le_bytes());
    operation.push(terminal);
    operation.push(u8::from(consume_pending_counter));
    operation.extend_from_slice(&expected_pending_count.to_le_bytes());
    operation.extend_from_slice(&expires_at.to_le_bytes());
    operation.extend_from_slice(lock_holder.as_ref());
    push_option_i64(&mut operation, lock_maturity_at);
    push_option_i64(&mut operation, lock_holder_recovery_at);
    debug_assert_eq!(operation.len(), LEGACY_V1_BINDING_OPERATION_LEN);
    operation
}

fn push_option_i64(output: &mut Vec<u8>, value: Option<i64>) {
    output.push(u8::from(value.is_some()));
    output.extend_from_slice(&value.unwrap_or_default().to_le_bytes());
}

fn exact_v1_voucher(
    session: &SessionRegistration,
    channel_id: &[u8; 32],
    cumulative_amount: u64,
    sequence_number: u32,
) -> Result<(Vec<u8>, [u8; 32], u64)> {
    require!(
        !is_v2_registration_nonce(session.nonce),
        VaultError::LegacyV1TabVoucherMismatch
    );
    let frontier = session.spent.max(session.crystallized_cumulative);
    let delta = cumulative_amount
        .checked_sub(frontier)
        .ok_or_else(|| error!(VaultError::LegacyV1TabVoucherMismatch))?;
    require!(
        delta > 0
            && delta == session.current_outstanding
            && cumulative_amount <= session.max_amount,
        VaultError::LegacyV1TabVoucherMismatch
    );
    let message = build_session_voucher_message(
        &crate::ID,
        &Pubkey::default(),
        &Pubkey::default(),
        session,
        channel_id,
        cumulative_amount,
        sequence_number,
    );
    let digest = hashv(&[&message]).to_bytes();
    Ok((message, digest, delta))
}

pub fn handler(ctx: Context<AuthorizeLegacyV1Tab>, args: AuthorizeLegacyV1TabArgs) -> Result<()> {
    require_compact_assertion(&args.client_data_json)?;
    let (terminal, consume_pending_counter) = decode_policy_flags(args.policy_flags)?;
    let vault_key = ctx.accounts.vault.key();
    let attestation_key = ctx.accounts.attestation.key();
    let resolution_key = ctx.accounts.voucher_resolution.key();
    let session_key = ctx.accounts.session.key();
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
    let deadline = ctx.accounts.voucher_resolution.settlement_deadline;
    require!(now <= deadline, VaultError::LegacyV1TabBindingMismatch);
    let (lock_maturity_at, lock_holder_recovery_at) = selected_lock_policy(terminal);
    require_selected_policy(
        terminal,
        args.lock_holder,
        lock_maturity_at,
        lock_holder_recovery_at,
    )?;

    let index = ctx
        .accounts
        .voucher_resolution
        .sessions
        .iter()
        .take(usize::from(ctx.accounts.voucher_resolution.session_count))
        .position(|state| state.present && state.session == session_key)
        .ok_or(VaultError::LegacyV6SessionSnapshotMismatch)?;
    let snapshot = ctx.accounts.attestation.session_attestations[index];
    let state = ctx.accounts.voucher_resolution.sessions[index];
    require!(
        snapshot.present
            && state.present
            && snapshot.session_account == session_key
            && state.binding == Pubkey::default()
            && !state.counter_reserved
            && state.state == LEGACY_V6_SESSION_RESOLUTION_NEUTRAL
            && state.remaining_outstanding == snapshot.current_outstanding
            && state.remaining_outstanding > 0,
        VaultError::LegacyV1TabBindingMismatch
    );

    let session_data_sha256 =
        normalized_recovered_session_sha256(&ctx.accounts.session.to_account_info())?;
    require_exact_session(
        &vault_key,
        session_key,
        &ctx.accounts.session,
        session_data_sha256,
        &snapshot,
    )?;

    let (_, voucher_digest, _) = exact_v1_voucher(
        &ctx.accounts.session.session,
        &args.channel_id,
        args.cumulative_amount,
        args.sequence_number,
    )?;
    let exact_counter = exact_historical_pending_count(&ctx.accounts.voucher_resolution)?;
    let unreserved_counter = unreserved_historical_pending_count(&ctx.accounts.voucher_resolution)?;
    require!(
        args.expected_pending_count == exact_counter
            && ctx.accounts.vault.pending_voucher_count == exact_counter
            && (!consume_pending_counter || unreserved_counter > 0),
        VaultError::LegacyV1TabCounterMismatch
    );
    let session_index =
        u8::try_from(index).map_err(|_| error!(VaultError::LegacyV6SessionSnapshotMismatch))?;

    let operation = build_binding_operation(
        ctx.program_id,
        &vault_key,
        &session_key,
        &snapshot.allowed_counterparty,
        &attestation_key,
        &resolution_key,
        &ctx.accounts.attestation.original_data_sha256,
        &session_data_sha256,
        &snapshot.session_pubkey,
        &args.channel_id,
        &voucher_digest,
        args.cumulative_amount,
        args.sequence_number,
        session_index,
        state.remaining_outstanding,
        terminal,
        consume_pending_counter,
        args.expected_pending_count,
        deadline,
        &args.lock_holder,
        lock_maturity_at,
        lock_holder_recovery_at,
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

    let binding = &mut ctx.accounts.binding;
    binding.version = LEGACY_V1_TAB_BINDING_VERSION_V1;
    binding.bump = ctx.bumps.binding;
    binding.vault = vault_key;
    binding.session = session_key;
    binding.attestation = attestation_key;
    binding.voucher_resolution = resolution_key;
    binding.original_data_sha256 = ctx.accounts.attestation.original_data_sha256;
    binding.session_data_sha256 = session_data_sha256;
    binding.session_pubkey = snapshot.session_pubkey;
    binding.channel_id = args.channel_id;
    binding.voucher_digest = voucher_digest;
    binding.cumulative_amount = args.cumulative_amount;
    binding.sequence_number = args.sequence_number;
    binding.allowed_counterparty = snapshot.allowed_counterparty;
    binding.session_index = session_index;
    binding.remaining_outstanding = state.remaining_outstanding;
    binding.terminal = terminal;
    binding.consume_pending_counter = consume_pending_counter;
    binding.expected_pending_count = args.expected_pending_count;
    binding.expires_at = deadline;
    binding.authorized_at = now;
    binding.lock_holder = args.lock_holder;
    binding.lock_maturity_at = lock_maturity_at;
    binding.lock_holder_recovery_at = lock_holder_recovery_at;
    binding.consumed = false;
    binding.consumed_at = None;

    let state = &mut ctx.accounts.voucher_resolution.sessions[index];
    state.binding = binding.key();
    state.counter_reserved = consume_pending_counter;
    state.state = LEGACY_V6_SESSION_RESOLUTION_OWNER_BOUND;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn validate_legacy_v1_terminal(
    program_id: &Pubkey,
    now: i64,
    terminal: u8,
    vault_key: Pubkey,
    vault: &Vault,
    session_key: Pubkey,
    session: &SessionAccount,
    session_data_sha256: [u8; 32],
    channel_id: &[u8; 32],
    cumulative_amount: u64,
    sequence_number: u32,
    lock_holder: Pubkey,
    lock_maturity_at: Option<i64>,
    lock_holder_recovery_at: Option<i64>,
    attestation_key: Pubkey,
    attestation: &LegacyV6Attestation,
    resolution_key: Pubkey,
    resolution: &LegacyV6VoucherResolution,
    binding_key: Pubkey,
    binding: &LegacyV1TabBinding,
) -> Result<LegacyV1TerminalPermit> {
    validate_resolution(
        program_id,
        &vault_key,
        vault,
        attestation_key,
        attestation,
        resolution_key,
        resolution,
    )?;
    let (expected_binding, binding_bump) =
        legacy_v1_binding_pda(program_id, &session.session.session_pubkey, channel_id);
    require_full_seller_window(now, binding.expires_at, resolution.settlement_deadline)?;
    require!(
        binding_key == expected_binding
            && binding.version == LEGACY_V1_TAB_BINDING_VERSION_V1
            && binding.bump == binding_bump
            && binding.vault == vault_key
            && binding.session == session_key
            && binding.attestation == attestation_key
            && binding.voucher_resolution == resolution_key
            && binding.original_data_sha256 == attestation.original_data_sha256
            && binding.session_data_sha256 == session_data_sha256
            && binding.session_pubkey == session.session.session_pubkey
            && binding.channel_id == *channel_id
            && binding.cumulative_amount == cumulative_amount
            && binding.sequence_number == sequence_number
            && binding.allowed_counterparty == session.session.allowed_counterparty
            && binding.terminal == terminal
            && !binding.consumed,
        VaultError::LegacyV1TabBindingMismatch
    );

    let index = usize::from(binding.session_index);
    require!(
        index < usize::from(resolution.session_count),
        VaultError::LegacyV1TabBindingMismatch
    );
    let snapshot = attestation.session_attestations[index];
    let state = resolution.sessions[index];
    require_exact_session(
        &vault_key,
        session_key,
        session,
        session_data_sha256,
        &snapshot,
    )?;
    require!(
        state.present
            && state.session == session_key
            && state.binding == binding_key
            && state.state == LEGACY_V6_SESSION_RESOLUTION_OWNER_BOUND
            && state.counter_reserved == binding.consume_pending_counter
            && state.remaining_outstanding == snapshot.current_outstanding
            && state.remaining_outstanding == binding.remaining_outstanding,
        VaultError::LegacyV1TabBindingMismatch
    );

    let (_, voucher_digest, delta) = exact_v1_voucher(
        &session.session,
        channel_id,
        cumulative_amount,
        sequence_number,
    )?;
    require!(
        voucher_digest == binding.voucher_digest && delta == binding.remaining_outstanding,
        VaultError::LegacyV1TabVoucherMismatch
    );
    require_selected_policy(
        terminal,
        lock_holder,
        lock_maturity_at,
        lock_holder_recovery_at,
    )?;
    require!(
        binding.lock_holder == lock_holder
            && binding.lock_maturity_at == lock_maturity_at
            && binding.lock_holder_recovery_at == lock_holder_recovery_at,
        VaultError::LegacyV1TabTerminalMismatch
    );

    let exact_counter = exact_historical_pending_count(resolution)?;
    require!(
        exact_counter <= binding.expected_pending_count
            && vault.pending_voucher_count == exact_counter
            && (!binding.consume_pending_counter || exact_counter > 0),
        VaultError::LegacyV1TabCounterMismatch
    );
    Ok(LegacyV1TerminalPermit {
        session_index: index,
        consume_pending_counter: binding.consume_pending_counter,
    })
}

pub(crate) fn consume_legacy_v1_terminal(
    now: i64,
    terminal: u8,
    vault: &mut Vault,
    session: &mut SessionAccount,
    resolution: &mut LegacyV6VoucherResolution,
    binding: &mut LegacyV1TabBinding,
    permit: LegacyV1TerminalPermit,
) -> Result<()> {
    let exact_counter = exact_historical_pending_count(resolution)?;
    require!(
        exact_counter <= binding.expected_pending_count
            && vault.pending_voucher_count == exact_counter,
        VaultError::LegacyV1TabCounterMismatch
    );
    let state = &mut resolution.sessions[permit.session_index];
    require!(
        state.state == LEGACY_V6_SESSION_RESOLUTION_OWNER_BOUND
            && state.counter_reserved == permit.consume_pending_counter
            && state.remaining_outstanding == binding.remaining_outstanding
            && !binding.consumed
            && binding.terminal == terminal,
        VaultError::LegacyV1TabBindingMismatch
    );
    if permit.consume_pending_counter {
        vault.pending_voucher_count = vault
            .pending_voucher_count
            .checked_sub(1)
            .ok_or_else(|| error!(VaultError::LegacyV1TabCounterMismatch))?;
        resolution.terminal_consumed_pending_count = resolution
            .terminal_consumed_pending_count
            .checked_add(1)
            .ok_or_else(|| error!(VaultError::LegacyV1TabCounterMismatch))?;
    }
    state.state = match terminal {
        LEGACY_V1_TAB_TERMINAL_SETTLE => LEGACY_V6_SESSION_RESOLUTION_SETTLED,
        LEGACY_V1_TAB_TERMINAL_LOCK => LEGACY_V6_SESSION_RESOLUTION_LOCKED,
        _ => return err!(VaultError::LegacyV1TabTerminalMismatch),
    };
    state.counter_reserved = false;
    state.remaining_outstanding = 0;
    binding.consumed = true;
    binding.consumed_at = Some(now);
    retire_recovered_session(vault, session)?;
    maybe_complete_legacy_v6_resolution(vault, resolution)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn static_v0_transaction_wire_size(
        signatures: usize,
        account_keys: usize,
        instruction_account_counts: &[usize],
        instruction_data_lengths: &[usize],
    ) -> usize {
        // A v0 message with no lookups adds the version prefix and the empty
        // address-table-lookup vector to the otherwise identical legacy wire.
        legacy_transaction_wire_size(
            signatures,
            account_keys,
            instruction_account_counts,
            instruction_data_lengths,
        ) + 2
    }

    #[test]
    fn authorization_packet_fits_for_both_terminals_and_one_compute_limit() {
        for policy_flags in [
            LEGACY_V1_TAB_POLICY_CONSUME_COUNTER,
            LEGACY_V1_TAB_POLICY_LOCK | LEGACY_V1_TAB_POLICY_CONSUME_COUNTER,
        ] {
            let args = AuthorizeLegacyV1TabArgs {
                channel_id: [1u8; 32],
                cumulative_amount: u64::MAX,
                sequence_number: u32::MAX,
                policy_flags,
                expected_pending_count: u32::MAX,
                lock_holder: if policy_flags & LEGACY_V1_TAB_POLICY_LOCK != 0 {
                    Pubkey::new_unique()
                } else {
                    Pubkey::default()
                },
                client_data_json: vec![0u8; CLIENT_DATA_MAX_LEN],
                authenticator_data: [0u8; AUTHENTICATOR_DATA_LEN],
            };
            let mut body = Vec::new();
            args.serialize(&mut body).unwrap();
            assert_eq!(body.len(), 522);

            // P-256 + authorization uses eleven unique keys. The second
            // measurement includes one five-byte ComputeBudget limit
            // instruction and its program key.
            let p256_instruction_data_len = 2 + 14 + 64 + 33 + AUTHENTICATOR_DATA_LEN + 32;
            let bare = legacy_transaction_wire_size(
                1,
                11,
                &[0, 9],
                &[p256_instruction_data_len, 8 + body.len()],
            );
            let with_compute_limit = legacy_transaction_wire_size(
                1,
                12,
                &[0, 0, 9],
                &[p256_instruction_data_len, 5, 8 + body.len()],
            );
            assert_eq!(bare, 1_183);
            assert_eq!(with_compute_limit, 1_223);
            assert!(with_compute_limit <= 1_232);
        }
    }

    #[test]
    fn maximum_legacy_v1_terminal_packets_fit_without_an_alt() {
        const ED25519_V1_VOUCHER_DATA_LEN: usize = 156;
        const SETTLE_DATA_LEN: usize = 84;
        const LOCK_DATA_LEN: usize = 118;
        const COMPUTE_LIMIT_DATA_LEN: usize = 5;
        const COMPUTE_PRICE_DATA_LEN: usize = 9;
        const SWIG_ONE_TRANSFER_DATA_LEN: usize = 28;
        const SWIG_FEE_TRANSFER_DATA_LEN: usize = 46;

        // Lock has three distinct transaction signers and sixteen account
        // metas after appending the recovery attestation, resolution, and
        // binding. The production compute pair contributes one unique program
        // key and twenty compiled bytes.
        let lock = legacy_transaction_wire_size(
            3,
            18,
            &[0, 16],
            &[ED25519_V1_VOUCHER_DATA_LEN, LOCK_DATA_LEN],
        );
        let lock_with_compute = legacy_transaction_wire_size(
            3,
            19,
            &[0, 0, 0, 16],
            &[
                COMPUTE_LIMIT_DATA_LEN,
                COMPUTE_PRICE_DATA_LEN,
                ED25519_V1_VOUCHER_DATA_LEN,
                LOCK_DATA_LEN,
            ],
        );

        // Direct settlement has two transaction signers. The production
        // maximum is the fee-on Swig SignV2 with two transfers; the one-leg
        // cash shape remains covered separately to catch roster drift.
        let direct_cash = legacy_transaction_wire_size(
            2,
            20,
            &[0, 0, 0, 12, 7],
            &[
                COMPUTE_LIMIT_DATA_LEN,
                COMPUTE_PRICE_DATA_LEN,
                ED25519_V1_VOUCHER_DATA_LEN,
                SETTLE_DATA_LEN,
                SWIG_ONE_TRANSFER_DATA_LEN,
            ],
        );
        let direct_fee = legacy_transaction_wire_size(
            2,
            21,
            &[0, 0, 0, 12, 9],
            &[
                COMPUTE_LIMIT_DATA_LEN,
                COMPUTE_PRICE_DATA_LEN,
                ED25519_V1_VOUCHER_DATA_LEN,
                SETTLE_DATA_LEN,
                SWIG_FEE_TRANSFER_DATA_LEN,
            ],
        );

        assert_eq!(lock, 1_103);
        assert_eq!(lock_with_compute, 1_155);
        assert_eq!(direct_cash, 1_123);
        assert_eq!(direct_fee, 1_175);
        assert_eq!(
            static_v0_transaction_wire_size(
                3,
                19,
                &[0, 0, 0, 16],
                &[
                    COMPUTE_LIMIT_DATA_LEN,
                    COMPUTE_PRICE_DATA_LEN,
                    ED25519_V1_VOUCHER_DATA_LEN,
                    LOCK_DATA_LEN,
                ],
            ),
            1_157
        );
        assert_eq!(
            static_v0_transaction_wire_size(
                2,
                21,
                &[0, 0, 0, 12, 9],
                &[
                    COMPUTE_LIMIT_DATA_LEN,
                    COMPUTE_PRICE_DATA_LEN,
                    ED25519_V1_VOUCHER_DATA_LEN,
                    SETTLE_DATA_LEN,
                    SWIG_FEE_TRANSFER_DATA_LEN,
                ],
            ),
            1_177
        );
        assert!(lock_with_compute <= 1_232);
        assert!(direct_fee <= 1_232);
        assert!(
            static_v0_transaction_wire_size(
                2,
                21,
                &[0, 0, 0, 12, 9],
                &[
                    COMPUTE_LIMIT_DATA_LEN,
                    COMPUTE_PRICE_DATA_LEN,
                    ED25519_V1_VOUCHER_DATA_LEN,
                    SETTLE_DATA_LEN,
                    SWIG_FEE_TRANSFER_DATA_LEN,
                ],
            ) <= 1_232
        );
    }

    #[test]
    fn global_tombstone_is_identical_across_recovered_vault_identities() {
        let program = crate::ID;
        let session_pubkey = [7u8; 32];
        let channel = [9u8; 32];
        let first_vault = Pubkey::new_unique();
        let second_vault = Pubkey::new_unique();
        let first_counterparty = Pubkey::new_unique();
        let second_counterparty = Pubkey::new_unique();
        let first_session_account = Pubkey::find_program_address(
            &[
                SESSION_SEED,
                first_vault.as_ref(),
                first_counterparty.as_ref(),
            ],
            &program,
        )
        .0;
        let second_session_account = Pubkey::find_program_address(
            &[
                SESSION_SEED,
                second_vault.as_ref(),
                second_counterparty.as_ref(),
            ],
            &program,
        )
        .0;
        assert_ne!(first_vault, second_vault);
        assert_ne!(first_session_account, second_session_account);

        // Vault and SessionAccount identities are intentionally absent from
        // this PDA. Historical V1 omitted both, so the same raw signing key
        // and channel must converge on one global, permanent tombstone.
        let first = legacy_v1_binding_pda(&program, &session_pubkey, &channel);
        let second = legacy_v1_binding_pda(&program, &session_pubkey, &channel);
        assert_eq!(first, second);
        assert_ne!(
            first,
            legacy_v1_binding_pda(&program, &session_pubkey, &[10u8; 32])
        );
    }

    #[test]
    fn operation_is_fixed_width_and_binds_terminal_counter_and_lock_policy() {
        let program = Pubkey::new_unique();
        let vault = Pubkey::new_unique();
        let session = Pubkey::new_unique();
        let counterparty = Pubkey::new_unique();
        let attestation = Pubkey::new_unique();
        let resolution = Pubkey::new_unique();
        let holder = Pubkey::new_unique();
        let baseline = build_binding_operation(
            &program,
            &vault,
            &session,
            &counterparty,
            &attestation,
            &resolution,
            &[1; 32],
            &[2; 32],
            &[3; 32],
            &[4; 32],
            &[5; 32],
            77,
            9,
            2,
            77,
            LEGACY_V1_TAB_TERMINAL_LOCK,
            true,
            2,
            1_000,
            &holder,
            None,
            Some(2_000),
        );
        assert_eq!(baseline.len(), LEGACY_V1_BINDING_OPERATION_LEN);

        let direct = build_binding_operation(
            &program,
            &vault,
            &session,
            &counterparty,
            &attestation,
            &resolution,
            &[1; 32],
            &[2; 32],
            &[3; 32],
            &[4; 32],
            &[5; 32],
            77,
            9,
            2,
            77,
            LEGACY_V1_TAB_TERMINAL_SETTLE,
            false,
            2,
            1_000,
            &Pubkey::default(),
            None,
            None,
        );
        assert_ne!(baseline, direct);
    }

    #[test]
    fn selected_lock_policy_creates_a_durable_claim() {
        assert!(require_selected_policy(
            LEGACY_V1_TAB_TERMINAL_LOCK,
            Pubkey::new_unique(),
            None,
            None,
        )
        .is_ok());
        assert!(require_selected_policy(
            LEGACY_V1_TAB_TERMINAL_LOCK,
            Pubkey::new_unique(),
            None,
            Some(100),
        )
        .is_err());
        assert!(require_selected_policy(
            LEGACY_V1_TAB_TERMINAL_SETTLE,
            Pubkey::default(),
            None,
            None,
        )
        .is_ok());
    }

    #[test]
    fn seller_keeps_the_entire_recovery_window() {
        assert!(require_full_seller_window(100, 100, 100).is_ok());
        assert!(require_full_seller_window(99, 99, 100).is_err());
        assert!(require_full_seller_window(101, 100, 100).is_err());
    }

    fn recovered_test_session(vault: Pubkey) -> SessionAccount {
        SessionAccount {
            version: SESSION_VERSION_LEGACY_V6_RECOVERED_V1,
            bump: 1,
            vault,
            session: SessionRegistration {
                session_pubkey: [9u8; 32],
                max_amount: 50,
                expires_at: 100,
                allowed_counterparty: Pubkey::new_unique(),
                nonce: 1,
                spent: 0,
                current_outstanding: 0,
                max_revolving_capacity: 50,
                crystallized_cumulative: 50,
                last_locked_sequence: 1,
            },
        }
    }

    fn test_binding(
        resolution: &LegacyV6VoucherResolution,
        index: u8,
        consume_pending_counter: bool,
        expected_pending_count: u32,
    ) -> LegacyV1TabBinding {
        LegacyV1TabBinding {
            version: LEGACY_V1_TAB_BINDING_VERSION_V1,
            bump: 1,
            vault: resolution.vault,
            session: resolution.sessions[usize::from(index)].session,
            attestation: resolution.attestation,
            voucher_resolution: Pubkey::new_unique(),
            original_data_sha256: [0; 32],
            session_data_sha256: [0; 32],
            session_pubkey: [0; 32],
            channel_id: [0; 32],
            voucher_digest: [0; 32],
            cumulative_amount: 50,
            sequence_number: 1,
            allowed_counterparty: Pubkey::new_unique(),
            session_index: index,
            remaining_outstanding: 50,
            terminal: LEGACY_V1_TAB_TERMINAL_SETTLE,
            consume_pending_counter,
            expected_pending_count,
            expires_at: 100,
            authorized_at: 1,
            lock_holder: Pubkey::default(),
            lock_maturity_at: None,
            lock_holder_recovery_at: None,
            consumed: false,
            consumed_at: None,
        }
    }

    #[test]
    fn terminal_counter_consumption_is_explicit_and_exact() {
        let vault_key = Pubkey::new_unique();
        let mut vault = Vault {
            version: VAULT_VERSION_V7,
            bump: 1,
            passkey_pubkey: [2; 33],
            swig_address: Pubkey::new_unique(),
            cooling_off_seconds: 0,
            pending_voucher_count: 1,
            pending_withdrawal: None,
            identity_claim: [0; 32],
            dexter_authority: Pubkey::new_unique(),
            live_session_count: LEGACY_V6_RESOLUTION_PENDING | 1,
            outstanding_locked_amount: 0,
            total_crystallized_amount: 0,
            total_settled_amount: 0,
            node: Pubkey::default(),
        };
        let mut resolution = LegacyV6VoucherResolution {
            version: LEGACY_V6_VOUCHER_RESOLUTION_VERSION_V1,
            bump: 1,
            vault: vault_key,
            attestation: Pubkey::new_unique(),
            recovered_at: 1,
            settlement_deadline: 100,
            original_pending_count: 1,
            session_count: 1,
            sessions: [LegacyV6SessionResolution::default();
                crate::constants::LEGACY_V6_RECOVERY_MAX_SESSIONS],
            terminal_consumed_pending_count: 0,
            owner_released_pending_count: 0,
            last_owner_release_at: None,
        };
        resolution.sessions[0] = LegacyV6SessionResolution {
            present: true,
            session: Pubkey::new_unique(),
            binding: Pubkey::new_unique(),
            counter_reserved: true,
            state: LEGACY_V6_SESSION_RESOLUTION_OWNER_BOUND,
            remaining_outstanding: 50,
        };
        let mut session = recovered_test_session(vault_key);
        let mut binding = test_binding(&resolution, 0, true, 1);
        consume_legacy_v1_terminal(
            2,
            LEGACY_V1_TAB_TERMINAL_SETTLE,
            &mut vault,
            &mut session,
            &mut resolution,
            &mut binding,
            LegacyV1TerminalPermit {
                session_index: 0,
                consume_pending_counter: true,
            },
        )
        .unwrap();
        assert_eq!(vault.pending_voucher_count, 0);
        assert_eq!(vault.live_session_count, 0);
        assert_eq!(session.version, 0);
        assert_eq!(resolution.terminal_consumed_pending_count, 1);
        assert_eq!(
            resolution.sessions[0].state,
            LEGACY_V6_SESSION_RESOLUTION_SETTLED
        );
        assert!(binding.consumed);
        assert!(consume_legacy_v1_terminal(
            3,
            LEGACY_V1_TAB_TERMINAL_SETTLE,
            &mut vault,
            &mut session,
            &mut resolution,
            &mut binding,
            LegacyV1TerminalPermit {
                session_index: 0,
                consume_pending_counter: true,
            },
        )
        .is_err());
    }

    #[test]
    fn mixed_and_false_only_terminals_leave_exact_owner_releasable_residuals() {
        for (original_pending, consumes) in [(1u32, vec![false]), (2u32, vec![true, false])] {
            let vault_key = Pubkey::new_unique();
            let mut vault = Vault {
                version: VAULT_VERSION_V7,
                bump: 1,
                passkey_pubkey: [2; 33],
                swig_address: Pubkey::new_unique(),
                cooling_off_seconds: 0,
                pending_voucher_count: original_pending,
                pending_withdrawal: None,
                identity_claim: [0; 32],
                dexter_authority: Pubkey::new_unique(),
                live_session_count: LEGACY_V6_RESOLUTION_PENDING | consumes.len() as u8,
                outstanding_locked_amount: 0,
                total_crystallized_amount: 0,
                total_settled_amount: 0,
                node: Pubkey::default(),
            };
            let mut resolution = LegacyV6VoucherResolution {
                version: LEGACY_V6_VOUCHER_RESOLUTION_VERSION_V1,
                bump: 1,
                vault: vault_key,
                attestation: Pubkey::new_unique(),
                recovered_at: 1,
                settlement_deadline: 100,
                original_pending_count: original_pending,
                session_count: consumes.len() as u16,
                sessions: [LegacyV6SessionResolution::default();
                    crate::constants::LEGACY_V6_RECOVERY_MAX_SESSIONS],
                terminal_consumed_pending_count: 0,
                owner_released_pending_count: 0,
                last_owner_release_at: None,
            };
            for (index, consume) in consumes.iter().copied().enumerate() {
                resolution.sessions[index] = LegacyV6SessionResolution {
                    present: true,
                    session: Pubkey::new_unique(),
                    binding: Pubkey::new_unique(),
                    counter_reserved: consume,
                    state: LEGACY_V6_SESSION_RESOLUTION_OWNER_BOUND,
                    remaining_outstanding: 50,
                };
            }

            for (index, consume) in consumes.iter().copied().enumerate() {
                let mut session = recovered_test_session(vault_key);
                let mut binding = test_binding(&resolution, index as u8, consume, original_pending);
                consume_legacy_v1_terminal(
                    2,
                    LEGACY_V1_TAB_TERMINAL_SETTLE,
                    &mut vault,
                    &mut session,
                    &mut resolution,
                    &mut binding,
                    LegacyV1TerminalPermit {
                        session_index: index,
                        consume_pending_counter: consume,
                    },
                )
                .unwrap();
                assert_eq!(session.version, 0);
            }

            let consumed = consumes.iter().filter(|consume| **consume).count() as u32;
            assert_eq!(resolution.terminal_consumed_pending_count, consumed);
            assert_eq!(vault.pending_voucher_count, original_pending - consumed);
            assert_eq!(vault.live_session_count, LEGACY_V6_RESOLUTION_PENDING);

            // The residual is neither lost nor double-consumed. The existing
            // post-deadline owner path can release it exactly once.
            resolution.owner_released_pending_count += vault.pending_voucher_count;
            vault.pending_voucher_count = 0;
            maybe_complete_legacy_v6_resolution(&mut vault, &resolution).unwrap();
            assert_eq!(vault.live_session_count, 0);
            assert_eq!(
                resolution.terminal_consumed_pending_count
                    + resolution.owner_released_pending_count,
                original_pending
            );
        }
    }
}
