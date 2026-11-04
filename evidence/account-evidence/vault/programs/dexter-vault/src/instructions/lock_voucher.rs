//! Crystallize a session-signed voucher into a vault-level LockedClaim PDA.
//!
//! `lock_voucher` is the graduation seam between the session-revocable
//! exposure tier (current_outstanding) and the vault-crystallized
//! reservation tier (outstanding_locked_amount). It performs the atomic
//! three-field mutation (seam spec §2, re-metered per the C-2 frontier
//! spec 2026-07-05 §2):
//!
//!   session.current_outstanding      -= D    (checked exact subtraction)
//!   session.crystallized_cumulative   = voucher.cumulative_amount   (SET)
//!   vault.outstanding_locked_amount  += D
//!
//! where D = voucher.cumulative_amount
//!             - max(session.spent, session.crystallized_cumulative)
//! — the uncovered span (frontier, cumulative] this lock crystallizes.
//! Every atomic span of cumulative pays out through exactly ONE terminal
//! (settle_tab_voucher or this lock); the SET odometer keeps each path's
//! odometer at the high-water cumulative it covered, so the frontier is
//! always the true covered-watermark.
//!
//! Transaction shape (two instructions, atomic):
//!
//!   [N-1]  Ed25519SigVerify precompile
//!            data = pubkey || signature || message
//!            where message is either the grandfathered 44-byte V1 payload or
//!            the 32-byte digest of the full context-bound V2 preimage.
//!   [N  ]  vault::lock_voucher  ← this instruction
//!            validates the precompile sibling matches
//!            (session_pubkey, voucher_message), applies the frontier
//!            guard, computes D, performs the graduation, creates the
//!            LockedClaim PDA in pending status. NO USDC moves — funds
//!            stay in the swig wallet ATA until settle_locked_voucher.
//!
//! Security properties this delivers (matching seam spec §4):
//!   - The XOR frontier guard `cumulative_amount > max(spent,
//!     crystallized_cumulative)` blocks any cumulative range already
//!     covered by either terminal odometer. Replay impossible; XOR
//!     enforced.
//!   - The vault-balance self-check (V0.3 Decision 1) blocks any lock
//!     that would push outstanding_locked_amount above the live USDC
//!     ATA balance.
//!   - Supported Native Tab terminals require `maturity_at=None` and
//!     `holder_recovery_at=None`. Crystallization therefore creates an
//!     immediately collectible, durable claim instead of opening a buyer
//!     recovery race at the pre-lock seller deadline.
//!   - The claim PDA derivation `["locked-claim", vault_pda, voucher_hash]`
//!     gives every unique voucher a unique on-chain address, blocking
//!     double-creation.

use anchor_lang::prelude::*;
use anchor_lang::solana_program::sysvar;
use anchor_spl::token::TokenAccount;
use solana_sha256_hasher::hashv;

use crate::constants::{
    GRAPH_CONFIG_SEED, SWIG_PROGRAM_ID, SWIG_VAULT_BINDING_SEED, SWIG_WALLET_ADDRESS_SEED,
};
use crate::instructions::authorize_legacy_v1_tab::{
    consume_legacy_v1_terminal, require_deterministic_lock_policy, validate_legacy_v1_terminal,
};
use crate::instructions::resolve_legacy_v6_vouchers::normalized_recovered_session_sha256;
use crate::state::*;
use crate::verify::ed25519::verify_session_signed;
use crate::verify::reserved_cash::verify_canonical_reserved_cash;
use crate::verify::voucher::{
    build_session_voucher_message, is_v2_final_voucher, is_v2_registration_nonce,
    require_within_settlement_window,
};

#[derive(Accounts)]
#[instruction(args: LockVoucherArgs)]
pub struct LockVoucher<'info> {
    #[account(
        mut,
        has_one = dexter_authority @ VaultError::PasskeyVerificationFailed,
    )]
    pub vault: Box<Account<'info, Vault>>,

    /// The swig wallet's USDC ATA — read to enforce the vault-balance
    /// self-check per V0.3 Decision 1. The handler cross-checks that this
    /// token account's `owner` field equals the canonical swig wallet PDA
    /// derived below so a caller cannot pass an unrelated funded ATA.
    pub vault_usdc_ata: Box<Account<'info, TokenAccount>>,

    /// CHECK: address constraint binds to vault.swig_address; never deref.
    #[account(address = vault.swig_address)]
    pub swig: AccountInfo<'info>,

    /// CHECK: PDA constraint validates derivation; never deref.
    #[account(
        seeds = [SWIG_WALLET_ADDRESS_SEED, swig.key().as_ref()],
        bump,
        seeds::program = SWIG_PROGRAM_ID,
    )]
    pub swig_wallet_address: AccountInfo<'info>,

    /// V6: the per-counterparty SessionAccount PDA whose meter graduates. The
    /// session moved OUT of `vault.active_session` into this PDA at
    /// `[SESSION_SEED, vault, allowed_counterparty]`. `mut` — the graduation
    /// mutates `current_outstanding`, `crystallized_cumulative`, and
    /// `last_locked_sequence` in place.
    #[account(
        mut,
        seeds = [crate::constants::SESSION_SEED, vault.key().as_ref(), args.allowed_counterparty.as_ref()],
        bump = session.bump,
    )]
    pub session: Box<Account<'info, SessionAccount>>,

    /// Claim PDA created in this instruction.
    #[account(
        init,
        payer = payer,
        space = 8 + LockedClaim::INIT_SPACE,
        seeds = [LOCKED_CLAIM_SEED, vault.key().as_ref(), &args.voucher_hash],
        bump,
    )]
    pub claim: Box<Account<'info, LockedClaim>>,

    /// Holder at creation — the seller who collected the voucher.
    pub seller_holder: Signer<'info>,

    /// Fee gate (matches settle_tab_voucher's discipline). Recorded on the
    /// vault at init; only this key authorizes the accumulator mutation.
    pub dexter_authority: Signer<'info>,

    /// Pays rent for the claim PDA. May be dexter_authority, may be a
    /// separate funder. Not security-load-bearing.
    #[account(mut)]
    pub payer: Signer<'info>,

    pub system_program: Program<'info, System>,

    /// CHECK: instructions sysvar — address-constrained.
    #[account(address = sysvar::instructions::ID)]
    pub instructions_sysvar: AccountInfo<'info>,

    /// Reverse uniqueness proof for the reserve accumulator being increased.
    #[account(
        seeds = [SWIG_VAULT_BINDING_SEED, swig.key().as_ref()],
        bump = swig_vault_binding.bump,
    )]
    pub swig_vault_binding: Box<Account<'info, SwigVaultBinding>>,

    /// Canonical settlement mint. A same-owner junk-mint account must never
    /// satisfy the LockedClaim reserve check.
    #[account(seeds = [GRAPH_CONFIG_SEED], bump = graph_config.bump)]
    pub graph_config: Box<Account<'info, GraphConfig>>,

    /// Recovery-only context appended after every existing account position.
    /// Context-bound V2 callers omit all three optionals.
    pub legacy_attestation: Option<Box<Account<'info, LegacyV6Attestation>>>,
    #[account(mut)]
    pub legacy_voucher_resolution: Option<Box<Account<'info, LegacyV6VoucherResolution>>>,
    #[account(mut)]
    pub legacy_v1_binding: Option<Box<Account<'info, LegacyV1TabBinding>>>,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct LockVoucherArgs {
    /// Channel id from the voucher payload. It is raw in V1 and included in the
    /// canonical preimage whose digest is signed in V2.
    pub channel_id: [u8; 32],
    /// Voucher's cumulative amount. Must strictly exceed the XOR frontier
    /// `max(session.spent, session.crystallized_cumulative)`. The claim
    /// crystallizes `cumulative_amount − frontier` (the uncovered span) and
    /// the crystallized odometer is SET to this value.
    pub cumulative_amount: u64,
    /// Sequence number from the voucher payload. Updates
    /// `session.last_locked_sequence` but is NOT load-bearing for replay
    /// defense (the frontier guard is).
    pub sequence_number: u32,
    /// sha256(voucher_message) — used as the third PDA seed for the claim.
    /// The handler verifies this matches the actually-signed message before
    /// trusting it as a seed.
    pub voucher_hash: [u8; 32],
    /// Reserved lock-policy field. Context-bound V2 and owner-bound recovered
    /// V1 terminals require `None`, producing an immediately collectible,
    /// durable claim.
    pub maturity_at: Option<i64>,
    /// Reserved buyer-recovery field. Supported Native Tab terminals require
    /// `None`; crystallization must not become buyer-recoverable when the
    /// pre-lock seller collection window expires.
    pub holder_recovery_at: Option<i64>,
    /// V6: the seller this voucher's session is bound to — equals the session
    /// PDA's `allowed_counterparty`. Carried in the args so the accounts
    /// struct can re-derive the session PDA seed. Grandfathered V1 relies on
    /// that seed binding; V2 additionally binds it inside the signed digest.
    pub allowed_counterparty: Pubkey,
}

/// Crystallize this session's exact-purchase reservation.
///
/// FINAL is true terminal consent, not permission to erase a residual. Native
/// Tab V2 therefore requires the crystallized delta to equal the exact live
/// reservation. Over-reserved streaming requires a separate
/// seller-acknowledged remainder release primitive and is not represented by
/// this P0 meter.
fn reconcile_lock_reservation(
    registration: &mut SessionRegistration,
    delta: u64,
    signed_finality: bool,
) -> Result<()> {
    require!(signed_finality, VaultError::VoucherFinalityRequired);
    require!(
        registration.current_outstanding > 0 && delta == registration.current_outstanding,
        VaultError::VoucherFinalDeltaMismatch
    );
    registration.current_outstanding = 0;
    Ok(())
}

pub fn handler(ctx: Context<LockVoucher>, args: LockVoucherArgs) -> Result<()> {
    require!(
        ctx.accounts.vault.version == VAULT_VERSION_V7,
        VaultError::UnsupportedVaultVersion
    );
    require!(
        ctx.accounts.vault.swig_address != Pubkey::default(),
        VaultError::PasskeyVerificationFailed
    );

    // V0.3 Decision 4: when both timestamps set, recovery must strictly
    // follow maturity. Strict inequality prevents same-second race.
    if let (Some(maturity), Some(recovery)) = (args.maturity_at, args.holder_recovery_at) {
        require!(recovery > maturity, VaultError::RecoveryBeforeMaturity);
    }

    // V0.3 Decision 1: prove one global Swig→Vault reserve ledger and the
    // canonical settlement-mint ATA before trusting its balance.
    verify_canonical_reserved_cash(
        ctx.program_id,
        &ctx.accounts.swig_vault_binding.key(),
        &ctx.accounts.swig_vault_binding,
        &ctx.accounts.vault.key(),
        &ctx.accounts.vault,
        &ctx.accounts.swig.key(),
        &ctx.accounts.swig_wallet_address.key(),
        &ctx.accounts.vault_usdc_ata.key(),
        &ctx.accounts.vault_usdc_ata.owner,
        &ctx.accounts.vault_usdc_ata.mint,
        ctx.accounts.vault_usdc_ata.amount,
        &ctx.accounts.graph_config.usdc_mint,
    )?;

    // The named session PDA must be live/registered (V6 replacement for the
    // old `vault.active_session.is_some()` check). version==0 means cleared or
    // never touched — locking against a dormant session is rejected.
    require!(
        ctx.accounts.session.version != 0,
        VaultError::NoActiveSession
    );

    // Snapshot the SessionRegistration for the read-only guards / verify. The
    // graduation mutation later takes a fresh &mut on the same account.
    let session = ctx.accounts.session.session.clone();
    let is_v2 = is_v2_registration_nonce(session.nonce);
    let now = Clock::get()?.unix_timestamp;
    if is_v2 {
        require_within_settlement_window(session.expires_at, now)?;
        require!(
            !legacy_v6_resolution_pending(ctx.accounts.vault.live_session_count),
            VaultError::LegacyV1TabBindingMismatch
        );
        require_deterministic_lock_policy(args.maturity_at, args.holder_recovery_at)?;
        require!(
            ctx.accounts.legacy_attestation.is_none()
                && ctx.accounts.legacy_voucher_resolution.is_none()
                && ctx.accounts.legacy_v1_binding.is_none(),
            VaultError::LegacyV1TabBindingMismatch
        );
    } else {
        require!(
            ctx.accounts.legacy_attestation.is_some()
                && ctx.accounts.legacy_voucher_resolution.is_some()
                && ctx.accounts.legacy_v1_binding.is_some(),
            VaultError::LegacyV1TabBindingMismatch
        );
    }

    // XOR frontier guard (seam spec §4). Symmetric to the same guard added
    // to settle_tab_voucher in Task 2. A voucher whose cumulative range was
    // already covered by EITHER terminal odometer (spent OR
    // crystallized_cumulative) is rejected here, regardless of which path
    // covered it. This single line enforces XOR.
    let frontier = session.spent.max(session.crystallized_cumulative);
    require!(
        args.cumulative_amount > frontier,
        VaultError::LockRangeAlreadyClaimed
    );
    require!(
        args.cumulative_amount <= session.max_amount,
        VaultError::InvalidVoucherSignature
    );

    let voucher_message = build_session_voucher_message(
        ctx.program_id,
        &ctx.accounts.vault.key(),
        &ctx.accounts.session.key(),
        &session,
        &args.channel_id,
        args.cumulative_amount,
        args.sequence_number,
    );

    // Confirm the preceding instruction was an Ed25519 precompile call
    // verifying this session_pubkey signed this message.
    verify_session_signed(
        &ctx.accounts.instructions_sysvar,
        &session.session_pubkey,
        &voucher_message,
    )?;

    let legacy_permit = if is_v2 {
        None
    } else {
        let attestation = ctx
            .accounts
            .legacy_attestation
            .as_deref()
            .ok_or(VaultError::LegacyV1TabBindingMismatch)?;
        let resolution = ctx
            .accounts
            .legacy_voucher_resolution
            .as_deref()
            .ok_or(VaultError::LegacyV1TabBindingMismatch)?;
        let binding = ctx
            .accounts
            .legacy_v1_binding
            .as_deref()
            .ok_or(VaultError::LegacyV1TabBindingMismatch)?;
        let session_data_sha256 =
            normalized_recovered_session_sha256(&ctx.accounts.session.to_account_info())?;
        Some(validate_legacy_v1_terminal(
            ctx.program_id,
            now,
            LEGACY_V1_TAB_TERMINAL_LOCK,
            ctx.accounts.vault.key(),
            &ctx.accounts.vault,
            ctx.accounts.session.key(),
            &ctx.accounts.session,
            session_data_sha256,
            &args.channel_id,
            args.cumulative_amount,
            args.sequence_number,
            ctx.accounts.seller_holder.key(),
            args.maturity_at,
            args.holder_recovery_at,
            attestation.key(),
            attestation,
            resolution.key(),
            resolution,
            binding.key(),
            binding,
        )?)
    };

    // Verify the caller's claimed voucher_hash matches the actual hash.
    // The hash is a PDA seed; if the caller lied about it, the PDA address
    // wouldn't have validated, but we belt-and-suspenders the message bytes.
    let computed_hash = hashv(&[&voucher_message]);
    require!(
        computed_hash.as_ref() == args.voucher_hash,
        VaultError::InvalidVoucherSignature
    );

    // Delta is what graduates from the session-revocable tier to the
    // vault-crystallized tier. C-2 frontier re-metering (spec 2026-07-05 §2):
    //   D = voucher.cumulative_amount - frontier
    // where `frontier` is the SAME max(spent, crystallized_cumulative)
    // binding the entry guard used — the claim covers exactly the uncovered
    // span (frontier, cumulative]. Metering from `crystallized_cumulative`
    // alone re-reserved (and later re-paid via /tab/lock/settle) any span
    // already tab-settled: settles to spent=0.10, then lock at 0.12 claimed
    // 0.12 − crystallized(0) = 0.12 — re-paying the 0.10 the seller already
    // received (spec §1 Failure B).
    let delta = args
        .cumulative_amount
        .checked_sub(frontier)
        .ok_or(VaultError::LockRangeAlreadyClaimed)?;
    require!(delta > 0, VaultError::VoucherFinalDeltaMismatch);
    let signed_finality = is_v2 && is_v2_final_voucher(&session, args.sequence_number);
    if is_v2 {
        require!(signed_finality, VaultError::VoucherFinalityRequired);
    }
    require!(
        delta == session.current_outstanding,
        VaultError::VoucherFinalDeltaMismatch
    );
    // V0.3 Decision 1 self-check: the post-lock outstanding must not exceed
    // the live USDC balance. vault_usdc_ata.amount is read live from the
    // SPL token account — never a cached field.
    let proposed_outstanding = ctx
        .accounts
        .vault
        .outstanding_locked_amount
        .checked_add(delta)
        .ok_or(VaultError::LockWouldOvercommitVault)?;
    require!(
        proposed_outstanding <= ctx.accounts.vault_usdc_ata.amount,
        VaultError::LockWouldOvercommitVault
    );

    // ── The graduation (seam spec §2, C-2 frontier re-metered). Atomic
    // mutation of three fields. V6: the session-tier fields live on the PDA
    // now. The Option is gone — the PDA exists (seed constraint) and is live
    // (version guard above), so this is unconditional.
    {
        let active = &mut ctx.accounts.session.session;
        if is_v2 {
            reconcile_lock_reservation(active, delta, signed_finality)?;
        } else {
            active.current_outstanding = 0;
        }
        // SET, not ADD (C-2 spec §2): after this op the odometer equals the
        // high-water cumulative covered VIA the lock path, so the frontier
        // max(spent, crystallized_cumulative) is the true covered-watermark.
        // With ADD, once settles push `spent` past `crystallized_cumulative`
        // the odometer lags the covered range — e.g. spent=0.10,
        // crystallized=0.04, lock at 0.12: delta=0.02, ADD would leave
        // crystallized=0.06 and frontier=0.10 even though 0.12 is covered;
        // the next settle at 0.15 would pay 0.05 when only (0.12, 0.15] =
        // 0.03 is uncovered — 0.02 double-paid. Corollary: sum(claim
        // amounts) == crystallized_cumulative NO LONGER holds; claims sum to
        // the spans actually covered by locks.
        active.crystallized_cumulative = args.cumulative_amount;
        active.last_locked_sequence = args.sequence_number;
    }

    // Vault-tier writes (separate &mut borrow; different account).
    let vault_key = ctx.accounts.vault.key();
    {
        let vault = &mut ctx.accounts.vault;
        vault.outstanding_locked_amount = proposed_outstanding;
        vault.total_crystallized_amount = vault
            .total_crystallized_amount
            .checked_add(delta)
            .ok_or_else(|| error!(VaultError::LockWouldOvercommitVault))?;
        if let Some(permit) = legacy_permit {
            let resolution = ctx
                .accounts
                .legacy_voucher_resolution
                .as_deref_mut()
                .ok_or(VaultError::LegacyV1TabBindingMismatch)?;
            let binding = ctx
                .accounts
                .legacy_v1_binding
                .as_deref_mut()
                .ok_or(VaultError::LegacyV1TabBindingMismatch)?;
            consume_legacy_v1_terminal(
                now,
                LEGACY_V1_TAB_TERMINAL_LOCK,
                vault,
                &mut ctx.accounts.session,
                resolution,
                binding,
                permit,
            )?;
        } else {
            vault.pending_voucher_count = vault
                .pending_voucher_count
                .checked_sub(1)
                .ok_or_else(|| error!(VaultError::NothingToRelease))?;
        }
    }

    // Create the LockedClaim PDA in pending state per V0.3 Decision 6.
    let claim = &mut ctx.accounts.claim;
    claim.version = LOCKED_CLAIM_VERSION_V1;
    claim.bump = ctx.bumps.claim;
    claim.vault = vault_key;
    claim.session_pubkey_at_lock = session.session_pubkey;
    claim.voucher_hash = args.voucher_hash;
    claim.amount = delta;
    claim.created_at = now;
    claim.maturity_at = args.maturity_at;
    claim.holder_recovery_at = args.holder_recovery_at;
    claim.current_holder = ctx.accounts.seller_holder.key();
    claim.status = LockedClaimStatus::Pending;
    claim.settled_at = None;
    claim.recovered_at = None;
    // #77 on-chain seller binding (crystallization-cadence 2026-07-06 §6e):
    // snapshot the seller into the claim. args.allowed_counterparty is
    // SEED-PROVEN (the session account above derives from it), so this is the
    // verified counterparty, not caller-trusted input. settle_locked_voucher
    // pins its money-leg destination to THIS pubkey's canonical ATA — the
    // claim's payout never depends on off-chain claim→seller state again.
    claim.allowed_counterparty = Some(args.allowed_counterparty);

    Ok(())
}

#[cfg(test)]
mod native_tab_meter_tests {
    use super::*;

    fn registration(nonce: u32, outstanding: u64) -> SessionRegistration {
        SessionRegistration {
            session_pubkey: [7; 32],
            max_amount: 1_000,
            expires_at: 1_900_000_000,
            allowed_counterparty: Pubkey::new_unique(),
            nonce,
            spent: 0,
            current_outstanding: outstanding,
            max_revolving_capacity: 1_000,
            crystallized_cumulative: 0,
            last_locked_sequence: 0,
        }
    }

    #[test]
    fn crystallization_terminal_requires_signed_finality_and_exact_reservation() {
        let mut v2 = registration(
            crate::verify::voucher::SESSION_VOUCHER_V2_NONCE_FLAG | 7,
            100,
        );
        assert!(reconcile_lock_reservation(&mut v2, 40, false).is_err());
        assert_eq!(v2.current_outstanding, 100);
        assert!(reconcile_lock_reservation(&mut v2, 40, true).is_err());
        assert_eq!(v2.current_outstanding, 100);
        reconcile_lock_reservation(&mut v2, 100, true).unwrap();
        assert_eq!(v2.current_outstanding, 0);
    }

    #[test]
    fn crystallization_never_closes_missing_or_excess_reservation() {
        let nonce = crate::verify::voucher::SESSION_VOUCHER_V2_NONCE_FLAG | 7;
        let mut none = registration(nonce, 0);
        assert!(reconcile_lock_reservation(&mut none, 1, true).is_err());
        let mut bounded = registration(nonce, 100);
        assert!(reconcile_lock_reservation(&mut bounded, 101, true).is_err());
        assert_eq!(bounded.current_outstanding, 100);
    }
}
