//! finish_swap — the CLOSING bracket of the carry swap leg (Option-A
//! delta-sandwich; see swap_for_carry.rs for the transaction shape).
//!
//! Runs EXACTLY two instructions after swap_for_carry, in the same slot, and
//! turns the custody promise into a program invariant measured on balances:
//!
//!   BUY  (USDC -> base):  usdc_after == usdc_before - amount_in   (input EXACT)
//!                         base_after >= base_before + min_out
//!                         usdc_after >= reservation (locks + credit liability)
//!   SELL (base -> USDC):  base_after == base_before - amount_in   (input EXACT)
//!                         usdc_after >= usdc_before + min_out
//!
//! The input side is bound to an EXACT decrease (not just an upper bound): the
//! committed input asset must be the thing actually spent. Without this a single
//! valid Jupiter route could fund the output from some OTHER token the vault
//! holds (leaving the watched input untouched) — the two-ATA delta check only
//! watches USDC+base, so a third asset would be invisible. Exact-input closes
//! that "third-token siphon" with pure balances and no Jupiter parsing (routes
//! MUST be ExactIn; a route that under- or over-spends the input reverts).
//! If the route sent output anywhere but the vault's own canonical ATA, or
//! delivered less than min_out, the ENTIRE transaction — including the swap —
//! reverts. No parse of Jupiter's bytes is load-bearing for custody.
//!
//! The BUY reservation check exists because USDC is the settlement asset for
//! the reserved obligations — crystallized locks + the credit node's liability
//! (subtree_draw + pro-forma accrued interest). (Uncrystallized session/tab
//! exposure is admission-gated at register, not reserved here — same as
//! finalize_withdrawal.) Converting reserved USDC into a yield asset would strip
//! that collateral exactly like an over-withdrawal, so a BUY is gated by the
//! same invariant as finalize_withdrawal. Base-asset holdings back NOTHING
//! today; the SELL direction strictly increases USDC and needs no reservation.

use anchor_lang::prelude::*;
use anchor_lang::solana_program::sysvar;
use anchor_lang::solana_program::sysvar::instructions::load_current_index_checked;
use anchor_spl::token::TokenAccount;

use crate::constants::{GRAPH_CONFIG_SEED, SWAP_BRACKET_SEED};
use crate::instructions::swap_for_carry::{SWAP_DIRECTION_BUY, SWAP_DIRECTION_SELL};
use crate::state::*;

#[derive(Accounts)]
pub struct FinishSwap<'info> {
    /// Position 0 — swap_for_carry's forward introspection pins THIS account
    /// as the vault the closing bracket serves; keep it first.
    pub vault: Box<Account<'info, Vault>>,
    /// The bracket swap_for_carry armed earlier in this transaction.
    #[account(
        mut,
        seeds = [SWAP_BRACKET_SEED, vault.key().as_ref()],
        bump = swap_bracket.bump,
    )]
    pub swap_bracket: Box<Account<'info, SwapBracket>>,
    /// The vault's canonical USDC ATA — post-swap balance read. Key-verified
    /// in the handler against the canonical derivation (same pin as the
    /// opening bracket, so both reads measure the same account).
    pub vault_usdc_ata: Box<Account<'info, TokenAccount>>,
    /// The vault's canonical base-asset ATA — post-swap balance read.
    pub vault_base_ata: Box<Account<'info, TokenAccount>>,
    /// The vault's welded PrincipalNode — REQUIRED iff welded (same contract
    /// as finalize_withdrawal): a BUY must reserve the node's full credit
    /// liability in USDC. Omitted for a plain vault.
    pub node: Option<Account<'info, PrincipalNode>>,
    /// Graph admin spine — canonical usdc_mint for the reservation math.
    #[account(seeds = [GRAPH_CONFIG_SEED], bump = graph_config.bump)]
    pub graph_config: Box<Account<'info, GraphConfig>>,
    /// CHECK: instructions sysvar — address-constrained.
    #[account(address = sysvar::instructions::ID)]
    pub instructions_sysvar: AccountInfo<'info>,
}

pub fn handler(ctx: Context<FinishSwap>) -> Result<()> {
    let bracket = &mut ctx.accounts.swap_bracket;
    let vault = &ctx.accounts.vault;

    // ── bracket integrity: armed, same vault, same slot, exact position ────
    require!(bracket.in_flight, VaultError::SwapBracketMismatch);
    require_keys_eq!(bracket.vault, vault.key(), VaultError::SwapBracketMismatch);
    let clock = Clock::get()?;
    require!(
        clock.slot == bracket.snapshot_slot,
        VaultError::SwapBracketMismatch
    );
    let current_index = load_current_index_checked(&ctx.accounts.instructions_sysvar)? as usize;
    require!(
        current_index == bracket.snapshot_ix_index as usize + 2,
        VaultError::SwapBracketMismatch
    );

    // ── same accounts as the snapshot: canonical ATA re-derivation ─────────
    // The swig wallet PDA is derivable from vault.swig_address without the
    // swig account itself being passed here.
    let (swig_wallet, _) = Pubkey::find_program_address(
        &[
            crate::constants::SWIG_WALLET_ADDRESS_SEED,
            vault.swig_address.as_ref(),
        ],
        &crate::constants::SWIG_PROGRAM_ID,
    );
    let usdc_mint = ctx.accounts.graph_config.usdc_mint;
    let expected_usdc =
        anchor_spl::associated_token::get_associated_token_address(&swig_wallet, &usdc_mint);
    let expected_base = anchor_spl::associated_token::get_associated_token_address(
        &swig_wallet,
        &bracket.base_mint,
    );
    require_keys_eq!(
        ctx.accounts.vault_usdc_ata.key(),
        expected_usdc,
        VaultError::SwapBracketMismatch
    );
    require_keys_eq!(
        ctx.accounts.vault_base_ata.key(),
        expected_base,
        VaultError::SwapBracketMismatch
    );

    let usdc_after = ctx.accounts.vault_usdc_ata.amount;
    let base_after = ctx.accounts.vault_base_ata.amount;

    // ── the delta invariant ─────────────────────────────────────────────────
    match bracket.direction {
        SWAP_DIRECTION_BUY => {
            // Input bound: USDC must decrease by EXACTLY amount_in. This is the
            // input-asset guard — a BUY's input is definitionally USDC, so an
            // exact-input (ExactIn) route consumes exactly amount_in USDC. A route
            // that instead funds the base purchase from some OTHER token the vault
            // holds leaves USDC UNCHANGED (drop = 0 != amount_in) and reverts here.
            // That closes the "third-token siphon": the two-ATA delta check alone
            // watches only USDC+base, so without this a single valid Jupiter route
            // could spend an unwatched asset. Requiring the committed input to be
            // spent in full forces USDC to be the source. (Routes MUST be ExactIn;
            // a non-ExactIn route that under-spends reverts, fail-closed.)
            let exact_usdc_after = bracket
                .usdc_before
                .checked_sub(bracket.amount_in)
                .ok_or(error!(VaultError::SwapDeltaShortfall))?;
            require!(usdc_after == exact_usdc_after, VaultError::SwapDeltaShortfall);
            // Output bound: gained at least min_out base, in the vault's ATA.
            let min_base_after = bracket
                .base_before
                .checked_add(bracket.min_out)
                .ok_or(error!(VaultError::SwapDeltaShortfall))?;
            require!(base_after >= min_base_after, VaultError::SwapDeltaShortfall);

            // Reservation invariant (finalize_withdrawal's, verbatim logic):
            // post-buy USDC must still cover crystallized locks + the node's
            // full credit liability + pro-forma accrued interest.
            let mut reserved = vault.outstanding_locked_amount;
            if vault.node != Pubkey::default() {
                let node = ctx
                    .accounts
                    .node
                    .as_ref()
                    .ok_or(error!(VaultError::CreditNodeRequired))?;
                require_keys_eq!(node.key(), vault.node, VaultError::WalletNodeMismatch);
                reserved = reserved
                    .checked_add(node.subtree_draw)
                    .ok_or(error!(VaultError::SwapWouldViolateReservation))?;
                reserved = reserved
                    .checked_add(crate::accrual::pro_forma_accrued(node, clock.unix_timestamp)?)
                    .ok_or(error!(VaultError::SwapWouldViolateReservation))?;
            }
            require!(
                usdc_after >= reserved,
                VaultError::SwapWouldViolateReservation
            );
        }
        SWAP_DIRECTION_SELL => {
            // Input bound: base must decrease by EXACTLY amount_in — the mirror of
            // the BUY guard. A SELL's input is the base asset; an ExactIn route
            // consumes exactly amount_in base. A route funding the USDC output from
            // some other token leaves base UNCHANGED and reverts here, forcing the
            // committed base to be the source.
            let exact_base_after = bracket
                .base_before
                .checked_sub(bracket.amount_in)
                .ok_or(error!(VaultError::SwapDeltaShortfall))?;
            require!(base_after == exact_base_after, VaultError::SwapDeltaShortfall);
            // Output bound: gained at least min_out USDC, in the vault's ATA.
            let min_usdc_after = bracket
                .usdc_before
                .checked_add(bracket.min_out)
                .ok_or(error!(VaultError::SwapDeltaShortfall))?;
            require!(usdc_after >= min_usdc_after, VaultError::SwapDeltaShortfall);
        }
        _ => return err!(VaultError::SwapUnsupportedDirection),
    }

    // ── close the bracket: burn the nonce, clear the flight state ──────────
    bracket.nonce = bracket
        .nonce
        .checked_add(1)
        .ok_or(error!(VaultError::SwapNonceMismatch))?;
    bracket.in_flight = false;
    bracket.direction = 0;
    bracket.amount_in = 0;
    bracket.min_out = 0;
    bracket.usdc_before = 0;
    bracket.base_before = 0;
    bracket.snapshot_slot = 0;
    bracket.snapshot_ix_index = 0;

    Ok(())
}
