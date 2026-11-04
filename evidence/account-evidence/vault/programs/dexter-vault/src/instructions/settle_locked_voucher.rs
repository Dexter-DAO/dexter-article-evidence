//! Settle a LockedClaim: move the reserved USDC from the swig wallet ATA
//! to the claim's payout anchor (the on-chain-bound seller's ATA for #77
//! claims; the current_holder's ATA for legacy unbound claims), decrement
//! outstanding_locked_amount, advance total_settled_amount, mark the claim
//! Settled.
//!
//! Per V0.3 Decision 7, this instruction is FULLY INDEPENDENT of
//! `vault.active_session`. It does not read, write, or reference the
//! session. The claim contains every field needed at settle time:
//! `current_holder`, `amount`, `maturity_at`. The session that originally
//! crystallized the claim may have been revoked, expired, or replaced —
//! settlement proceeds regardless. This is the seam that makes revoke
//! unable to break settlement.
//!
//! Transaction shape (two instructions, atomic):
//!   [N]   vault::settle_locked_voucher  ← this instruction
//!           validates maturity (if any), holder signature, mutates state
//!   [N+1] swig::SignV2(TransferChecked ×1, or ×2 with the platform fee)
//!           Swig validates accounts[0..1] of THIS instruction equal
//!           [swig, swig_wallet] AND that the preceding instruction's
//!           data starts with the settle_locked_voucher discriminator
//!           (registered as a ProgramExec marker). On match, executes
//!           the SPL transfer(s) from swig_wallet_ata as a ProgramExec
//!           authority. Board #118: the envelope is 1-or-2-leg exactly
//!           like settle_tab_voucher's #75 shape — legs sum to
//!           claim.amount, optional fee leg to the tab_fee_treasury,
//!           same tab_fee_max_bps brake. One fee policy, both roads.
//!
//! IMPORTANT: the settle_locked_voucher discriminator MUST be added to
//! the Swig's ProgramExec marker list. New vaults created after the
//! marker registration will work; older Swigs (registered with only the
//! settle_tab_voucher + finalize_withdrawal markers) cannot use this
//! instruction. SDK marker-list update is Phase 2 SDK work; Phase 1
//! tests register the new marker manually on fresh enrollment.

use anchor_lang::prelude::*;
use anchor_lang::solana_program::sysvar;
use anchor_spl::token::TokenAccount;

use crate::constants::{SWIG_PROGRAM_ID, SWIG_VAULT_BINDING_SEED, SWIG_WALLET_ADDRESS_SEED};
use crate::state::*;
use crate::verify::reserved_cash::{
    require_canonical_money_leg_source, verify_canonical_reserved_cash,
};
use crate::verify::swig_transfer::{
    decode_following_swig_transfer_one_or_two, require_swig_ixs_marker_bound,
};

#[derive(Accounts)]
pub struct SettleLockedVoucher<'info> {
    /// Position 0 — required at this index by Swig's ProgramExec validator.
    /// CHECK: address constraint binds to vault.swig_address; never deref.
    #[account(address = vault.swig_address)]
    pub swig: AccountInfo<'info>,

    /// Position 1 — required by Swig's ProgramExec validator.
    /// CHECK: PDA constraint validates derivation; never deref.
    #[account(
        seeds = [SWIG_WALLET_ADDRESS_SEED, swig.key().as_ref()],
        bump,
        seeds::program = SWIG_PROGRAM_ID,
    )]
    pub swig_wallet_address: AccountInfo<'info>,

    #[account(
        mut,
        close = dexter_authority,                       // ← Task 2: reclaim claim rent on settle
        constraint = claim.status == LockedClaimStatus::Pending
            @ VaultError::LockRangeAlreadyClaimed,
        constraint = claim.vault == vault.key()
            @ VaultError::PasskeyVerificationFailed,
    )]
    pub claim: Account<'info, LockedClaim>,

    /// Accumulator vault. Mutated to decrement outstanding_locked_amount
    /// and advance total_settled_amount. Per V0.3 Decision 7, the
    /// `active_session` field is NOT read here.
    #[account(mut)]
    pub vault: Account<'info, Vault>,

    /// Must equal claim.current_holder. This signature is the sole
    /// authority for the settlement.
    #[account(
        constraint = claim.current_holder == holder.key()
            @ VaultError::PasskeyVerificationFailed,
    )]
    pub holder: Signer<'info>,

    /// Fee gate (matches settle_tab_voucher's discipline). MUST be `mut`:
    /// `close = dexter_authority` on the claim credits the reclaimed rent to
    /// this account, mutating its lamports — without `mut` the runtime rejects
    /// settle with "instruction changed the balance of a read-only account"
    /// (matches close_session, whose dexter_authority is also `mut`).
    #[account(
        mut,
        constraint = vault.dexter_authority == dexter_authority.key()
            @ VaultError::PasskeyVerificationFailed,
    )]
    pub dexter_authority: Signer<'info>,

    /// CHECK: instructions sysvar — address-constrained. The FOLLOWING swig
    /// SignV2 is the money leg this handler now introspects and binds.
    #[account(address = sysvar::instructions::ID)]
    pub instructions_sysvar: AccountInfo<'info>,

    /// Graph admin spine — read for the canonical settlement mint the money
    /// leg is pinned to. A single global PDA; supplied so the payout mint
    /// can't be forged.
    #[account(seeds = [crate::constants::GRAPH_CONFIG_SEED], bump = graph_config.bump)]
    pub graph_config: Account<'info, GraphConfig>,

    #[account(
        seeds = [SWIG_VAULT_BINDING_SEED, swig.key().as_ref()],
        bump = swig_vault_binding.bump,
    )]
    pub swig_vault_binding: Box<Account<'info, SwigVaultBinding>>,

    /// Canonical source of the senior payout. No unreserved-cash gate applies:
    /// this path consumes the reserve itself.
    pub vault_usdc_ata: Box<Account<'info, TokenAccount>>,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct SettleLockedVoucherArgs {}

fn require_current_vault_version(version: u8) -> Result<()> {
    require!(
        version == VAULT_VERSION_V7,
        VaultError::UnsupportedVaultVersion
    );
    Ok(())
}

pub fn handler(ctx: Context<SettleLockedVoucher>, _args: SettleLockedVoucherArgs) -> Result<()> {
    // Anchor's account deserializer accepts trailing bytes. Without an explicit
    // version gate, a 279-byte legacy V6 vault can deserialize through the
    // shorter current layout and this handler will write common-prefix fields
    // through the wrong schema. Every other money handler is V7-gated; locked
    // claim settlement must be as well.
    require_current_vault_version(ctx.accounts.vault.version)?;

    // ── MONEY-LEG BIND ────────────────────────────────────────────────────────
    // settle_locked had NO binding at all — it didn't even take the instructions
    // sysvar. The header claimed "the following swig::SignV2 introspects THIS
    // instruction," but the REVERSE (this instruction constraining that transfer)
    // was never enforced, so a role-6 SignV2 could move the whole balance anywhere.
    // Bind it: the following transfer must move EXACTLY `claim.amount` USDC, on the
    // canonical mint, to the claim's declared payout anchor — the on-chain-bound
    // seller's ATA for #77 claims, the holder's ATA for legacy unbound claims.
    //
    // MULTI-TOKEN SEAM: when a claim carries its own settlement mint, the
    // `== graph_config.usdc_mint` below becomes `== claim.settlement_mint`
    // (graph_config gating the allowed set). Same choke-point; obligation-carried mint.
    // #77 SELLER BINDING (crystallization-cadence 2026-07-06 §6e): a claim
    // that recorded its seller at lock time pins the payout to the SELLER's
    // canonical ATA — the holder signature authorizes WHEN settlement runs,
    // the claim itself says WHERE the money goes. This is what makes an
    // orphaned-Pending claim (lost off-chain claim→seller record) still
    // payable to its rightful seller from on-chain state alone, and what lets
    // the facilitator's keyless-seller model (current_holder = the Dexter
    // holder key) pay the seller directly instead of the holder. Legacy
    // (pre-binding, allowed_counterparty == None) claims keep the original
    // holder-ATA anchor unchanged.
    let payout_owner = ctx
        .accounts
        .claim
        .allowed_counterparty
        .unwrap_or_else(|| ctx.accounts.holder.key());
    let usdc_mint = ctx.accounts.graph_config.usdc_mint;
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
        &usdc_mint,
    )?;
    let canonical_source = ctx.accounts.vault_usdc_ata.key();
    let expected_dest =
        anchor_spl::associated_token::get_associated_token_address(&payout_owner, &usdc_mint);
    // ── PLATFORM FEE ON CRYSTALLIZED VALUE (board #118) ──────────────────────
    // Mirror of settle_tab_voucher's #75 envelope, verbatim discipline: the
    // claim bounds the TOTAL (claim.amount); this bind controls how that total
    // may be DIVIDED. Without this, crystallized value settled FEE-FREE and the
    // 1% tab fee collected only on un-crystallized closes — under the #99/#100
    // target design (most value crystallizes) the fee would have collected on
    // almost nothing, and a savvy seller would PREFER the protection path as a
    // fee discount. Fee follows value, not path (Branch ruling 2026-07-25).
    //
    //   1 leg  — the whole claim.amount to the payout anchor's canonical ATA.
    //            Bit-for-bit the pre-fee behavior; always accepted.
    //   2 legs — payout + platform fee, summing EXACTLY to claim.amount.
    //            Accepted ONLY when `graph_config.tab_fee_treasury` is set
    //            (zero pubkey = split disabled → two-leg envelopes revert):
    //            leg0 → the payout anchor's ATA (> 0 — the seller can never be
    //            zeroed out), leg1 → the treasury's ATA (> 0 — no dust legs).
    //            When `tab_fee_max_bps` > 0, the fee leg is capped at
    //            claim.amount × bps / 10_000 (TabFeeExceedsCap) — the same
    //            armable brake, one policy for both settle roads.
    // P5: the settle SignV2 (role 6) is bound at current+1. Refuse any other
    // Swig instruction on this swig in the tx (approval-reuse defense).
    require_swig_ixs_marker_bound(
        &ctx.accounts.instructions_sysvar,
        &ctx.accounts.swig.key(),
    )?;
    let legs = decode_following_swig_transfer_one_or_two(&ctx.accounts.instructions_sysvar)?;
    match legs.as_slice() {
        [payout_leg] => {
            require!(
                payout_leg.amount == ctx.accounts.claim.amount
                    && payout_leg.destination == expected_dest
                    && payout_leg.mint == usdc_mint,
                VaultError::DebtTransferMismatch
            );
            require_canonical_money_leg_source(payout_leg, &canonical_source, &usdc_mint)?;
        }
        [payout_leg, fee_leg] => {
            let treasury = ctx.accounts.graph_config.tab_fee_treasury;
            require!(
                treasury != Pubkey::default(),
                VaultError::DebtTransferMismatch
            );
            require_canonical_money_leg_source(payout_leg, &canonical_source, &usdc_mint)?;
            require_canonical_money_leg_source(fee_leg, &canonical_source, &usdc_mint)?;
            let expected_treasury_ata =
                anchor_spl::associated_token::get_associated_token_address(&treasury, &usdc_mint);
            let sum = payout_leg
                .amount
                .checked_add(fee_leg.amount)
                .ok_or(VaultError::DebtTransferMismatch)?;
            require!(
                sum == ctx.accounts.claim.amount
                    && payout_leg.amount > 0
                    && fee_leg.amount > 0
                    && payout_leg.destination == expected_dest
                    && payout_leg.mint == usdc_mint
                    && fee_leg.destination == expected_treasury_ata
                    && fee_leg.mint == usdc_mint,
                VaultError::DebtTransferMismatch
            );
            let max_bps = ctx.accounts.graph_config.tab_fee_max_bps;
            if max_bps > 0 {
                // u128 intermediate: claim.amount × 10_000 can overflow u64.
                let cap = (ctx.accounts.claim.amount as u128 * max_bps as u128 / 10_000) as u64;
                require!(fee_leg.amount <= cap, VaultError::TabFeeExceedsCap);
            }
        }
        // decode_following_swig_transfer_one_or_two only returns 1 or 2 legs.
        _ => return Err(error!(VaultError::DebtTransferMismatch)),
    }

    let claim = &mut ctx.accounts.claim;
    let vault = &mut ctx.accounts.vault;

    let now = Clock::get()?.unix_timestamp;

    // V0.3 Decision 3: if maturity_at is set, enforce it. Reuses
    // CoolingOffNotElapsed semantically — same "the future hasn't arrived"
    // shape.
    if let Some(maturity) = claim.maturity_at {
        require!(now >= maturity, VaultError::CoolingOffNotElapsed);
    }

    // V0.3 Decision 1: accumulator mutation. outstanding_locked_amount
    // FALLS by the settled amount; total_settled_amount RISES by the
    // settled amount (lifetime monotonic odometer).
    vault.outstanding_locked_amount = crate::verify::reserved_cash::release_locked_reserve(
        vault.outstanding_locked_amount,
        claim.amount,
    )?;
    vault.total_settled_amount = vault
        .total_settled_amount
        .checked_add(claim.amount)
        .ok_or_else(|| error!(VaultError::CarryMathOverflow))?;

    // V0.3 Decision 6: state machine transition. Pending → Settled,
    // terminal one-way. NOTE (Task 2): the `close = dexter_authority`
    // constraint on `claim` reclaims the rent at account-exit AFTER this
    // handler returns — draining lamports to dexter_authority, reassigning
    // to System, resizing to 0. These writes are therefore discarded with
    // the account; they're kept so the in-flight handler invariant matches
    // settle's pre-close semantics and so the audit diff stays minimal.
    // The accumulator mutations on `vault` above are persisted (vault is
    // not closed). The following swig::SignV2 introspects THIS instruction
    // via the instructions sysvar (a snapshot taken at tx assembly), not
    // the live claim account, so the close does not disturb the payout.
    claim.status = LockedClaimStatus::Settled;
    claim.settled_at = Some(now);
    // recovered_at stays None (per Decision 6: only abandonment sets it).

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settlement_accepts_only_v7_vaults() {
        assert!(require_current_vault_version(VAULT_VERSION_V7).is_ok());
        for version in 0..VAULT_VERSION_V7 {
            assert!(require_current_vault_version(version).is_err());
        }
        assert!(require_current_vault_version(VAULT_VERSION_V7 + 1).is_err());
    }
}
