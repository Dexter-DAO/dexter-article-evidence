use anchor_lang::prelude::*;
use anchor_lang::solana_program::sysvar;
use anchor_spl::token::TokenAccount;

use crate::constants::{SWIG_PROGRAM_ID, SWIG_WALLET_ADDRESS_SEED};
use crate::state::*;
use crate::verify::swig_transfer::decode_following_swig_transfer_pair;
use crate::verify::webauthn::verify_passkey_signed;

#[derive(Accounts)]
pub struct FinalizeWithdrawal<'info> {
    /// Position 0 — REQUIRED at this index by Swig's ProgramExec authority
    /// validator. When a Swig::SignV2 follows this instruction in the same
    /// transaction, Swig's on-chain validator inspects accounts[0..1] of the
    /// preceding instruction and rejects unless they're [swig, swig_wallet].
    ///
    /// We additionally enforce `swig.key() == vault.swig_address` via the
    /// `address` constraint so a caller cannot pass an arbitrary Swig account
    /// in here — defense in depth: even if Swig's own validation changes in a
    /// future program upgrade, this vault keeps its own invariant.
    ///
    /// CHECK: address constraint binds it to vault.swig_address; we never
    /// deserialize or dereference it.
    #[account(address = vault.swig_address)]
    pub swig: AccountInfo<'info>,
    /// Position 1 — required by Swig's ProgramExec validator (see `swig`).
    /// The Swig wallet address is the PDA owning the SPL token ATA being
    /// debited; it is derived under the Swig program at
    /// `["swig-wallet-address", swig_pubkey]`.
    ///
    /// We independently verify the canonical derivation via Anchor's `seeds`
    /// + `seeds::program` constraint. If a caller supplied a fake account, our
    /// program rejects before any Swig CPI runs — we do not rely on Swig
    /// catching it downstream.
    ///
    /// CHECK: PDA constraint validates derivation; not deserialized.
    #[account(
        seeds = [SWIG_WALLET_ADDRESS_SEED, swig.key().as_ref()],
        bump,
        seeds::program = SWIG_PROGRAM_ID,
    )]
    pub swig_wallet_address: AccountInfo<'info>,
    #[account(mut)]
    pub vault: Account<'info, Vault>,
    /// The swig wallet's USDC ATA — read live to enforce the reservation
    /// invariant per V0.3 Decision 1: post-withdrawal balance must remain
    /// at or above `vault.outstanding_locked_amount` (+ credit liability below).
    /// The token account's `owner` field must match the swig_wallet_address PDA.
    pub vault_usdc_ata: Account<'info, TokenAccount>,
    /// The vault's welded `PrincipalNode` — REQUIRED iff this vault is welded
    /// (`vault.node != Pubkey::default()`), so the withdrawal can reserve the
    /// node's full credit liability (`subtree_draw`). Bound to `vault.node` in
    /// the handler. Omitted (None — pass the program id sentinel) for a plain,
    /// non-credit vault. Read-only.
    pub node: Option<Account<'info, PrincipalNode>>,
    /// Graph admin spine. Read for the canonical settlement mint
    /// (`usdc_mint`) that BOTH the reservation self-check and the following
    /// money leg are pinned to — a plain non-credit vault still supplies it
    /// (it's a single global PDA), so the withdrawal mint can never be forged.
    #[account(seeds = [crate::constants::GRAPH_CONFIG_SEED], bump = graph_config.bump)]
    pub graph_config: Account<'info, GraphConfig>,
    /// CHECK: instructions sysvar — address-constrained.
    #[account(address = sysvar::instructions::ID)]
    pub instructions_sysvar: AccountInfo<'info>,
}

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct FinalizeWithdrawalArgs {
    /// WebAuthn clientDataJSON; challenge must be sha256(operation_message).
    pub client_data_json: Vec<u8>,
    pub authenticator_data: Vec<u8>,
}

pub fn handler(ctx: Context<FinalizeWithdrawal>, args: FinalizeWithdrawalArgs) -> Result<()> {
    let vault = &mut ctx.accounts.vault;
    let pending = vault
        .pending_withdrawal
        .clone()
        .ok_or(VaultError::NoPendingWithdrawal)?;

    // Anchor's `address = vault.swig_address` constraint on the `swig` account
    // already enforces that the supplied swig matches the bound address. The
    // additional non-default check below is paranoia: if set_swig was never
    // called, vault.swig_address is the zero Pubkey, which Anchor would happily
    // match against a zero account. Fail fast and explicit.
    require!(
        vault.swig_address != Pubkey::default(),
        VaultError::NoPendingWithdrawal
    );

    let now = Clock::get()?.unix_timestamp;
    // i64 unix timestamps minus another i64 timestamp; clamp to non-negative
    // (a future-dated `requested_at` would otherwise wrap), then promote both
    // sides to u64 so the comparison against the u32 cooling-off field is
    // unambiguous and overflow-safe across the full timestamp range.
    let elapsed_secs = now.saturating_sub(pending.requested_at).max(0) as u64;
    require!(
        elapsed_secs >= vault.cooling_off_seconds as u64,
        VaultError::CoolingOffNotElapsed
    );
    require!(vault.pending_voucher_count == 0, VaultError::PendingVouchersExist);
    require!(
        vault.version == VAULT_VERSION_V7,
        VaultError::UnsupportedVaultVersion
    );

    // V0.3 Decision 1: the reservation invariant. The withdrawal must not
    // bring the vault's USDC balance below the sum of outstanding locked
    // claims. Live read from the token account; never trust a cached field.
    // The ATA's `owner` is cross-checked against the canonical swig wallet
    // PDA so a caller can't smuggle a funded but unrelated ATA into the
    // self-check.
    require!(
        ctx.accounts.vault_usdc_ata.owner == ctx.accounts.swig_wallet_address.key(),
        VaultError::PasskeyVerificationFailed
    );
    // Pin the self-check to the canonical mint. Without this, a junk-mint ATA
    // (owned by the SAME swig wallet, so it passes the owner check above) with a
    // fat fake balance satisfies the reservation invariant while the following
    // SignV2 drains the REAL USDC. The ATA must be USDC or the check is a lie.
    require!(
        ctx.accounts.vault_usdc_ata.mint == ctx.accounts.graph_config.usdc_mint,
        VaultError::WithdrawalWouldViolateReservation
    );
    let live_balance_after = ctx
        .accounts
        .vault_usdc_ata
        .amount
        .checked_sub(pending.amount)
        .ok_or(VaultError::WithdrawalWouldViolateReservation)?;

    // ── RESERVATION = crystallized locks + the node's FULL credit liability ────
    // CREDIT PIN (restored; Model-A cascade-correct). The V5 pin reserved
    // `outstanding_locked + vault.borrowed`; `vault.borrowed` was deleted in the
    // Task-2 layout move and the credit accumulator now lives on the PrincipalNode.
    // We re-pin against the node's `subtree_draw` — NOT just `borrowed` — because
    // under Model A the vault's collateral is seizable for the node's OWN draw
    // (seize_collateral) AND for every descendant's shortfall it is an ancestor of
    // (seize_ancestor cascade, root-bounded). `subtree_draw` already includes the
    // node's own `borrowed`, so it is the node's TOTAL liability and the exact
    // worst case. Withdrawing below it would strip recourse that backs live credit.
    // A welded vault MUST supply its bound node; a plain non-credit vault
    // (node == default) reserves only the crystallized locks.
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
            .ok_or(error!(VaultError::ReleaseExceedsSubtreeDraw))?;
        // SPREAD ENGINE: the node's interest claim is part of its liability but
        // lives OUTSIDE subtree_draw (leaf-local ledger, accrual.rs module doc).
        // Pin the full pro-forma claim — banked accrued_fee plus the un-banked
        // tail to now — so a withdrawal cannot strip the funds that owe interest.
        let now = Clock::get()?.unix_timestamp;
        reserved = reserved
            .checked_add(crate::accrual::pro_forma_accrued(node, now)?)
            .ok_or(error!(VaultError::ReleaseExceedsSubtreeDraw))?;
    }
    require!(
        live_balance_after >= reserved,
        VaultError::WithdrawalWouldViolateReservation
    );

    // ── MONEY-LEG BIND (two legs: net → user, fee → treasury) ────────────────
    // The passkey (below) authorizes an INTENT — gross amount + destination
    // WALLET, signed into op_msg. This binds the ACTUAL USDC move — the swig
    // SignV2 that FOLLOWS this instruction and is authorized by our ProgramExec
    // marker — to that intent. The program derives the wallet's canonical
    // usdc_mint ATA (same pattern as require_financier_destination): the user
    // signs the address they typed, the chain pins where tokens actually land.
    // The fee split is PROTOCOL STATE (GraphConfig, cold-admin-tunable, capped
    // by WITHDRAWAL_FEE_CAP): leg0 must move exactly gross − fee to the user,
    // leg1 exactly fee to the treasury. The legs sum to gross, so the
    // reservation self-check above prices the full outflow. This closes the v2
    // trust gap (a facilitator could deviate from the advertised fee — task
    // #186) while keeping the anti-siphon stance: exactly two legs, every byte
    // reconciled, anything else is DebtTransferMismatch. Drain (2) stays
    // closed: a 0-amount intent cannot even cover the fee (WithdrawalBelowFee),
    // and with fee = 0 it still forces a 0-amount user leg.
    let fee = ctx.accounts.graph_config.withdrawal_fee_atomic;
    let fee_treasury = ctx.accounts.graph_config.fee_treasury;
    let usdc_mint = ctx.accounts.graph_config.usdc_mint;
    let net = pending
        .amount
        .checked_sub(fee)
        .ok_or(error!(VaultError::WithdrawalBelowFee))?;
    require!(net > 0, VaultError::WithdrawalBelowFee);

    let (user_leg, fee_leg) =
        decode_following_swig_transfer_pair(&ctx.accounts.instructions_sysvar)?;
    let expected_user_ata = anchor_spl::associated_token::get_associated_token_address(
        &pending.destination,
        &usdc_mint,
    );
    let expected_treasury_ata = anchor_spl::associated_token::get_associated_token_address(
        &fee_treasury,
        &usdc_mint,
    );
    require!(
        user_leg.amount == net
            && user_leg.destination == expected_user_ata
            && user_leg.mint == usdc_mint,
        VaultError::DebtTransferMismatch
    );
    require!(
        fee_leg.amount == fee
            && fee_leg.destination == expected_treasury_ata
            && fee_leg.mint == usdc_mint,
        VaultError::DebtTransferMismatch
    );

    let mut op_msg = Vec::with_capacity(b"finalize_withdrawal".len() + 8 + 32);
    op_msg.extend_from_slice(b"finalize_withdrawal");
    op_msg.extend_from_slice(&pending.amount.to_le_bytes());
    op_msg.extend_from_slice(pending.destination.as_ref());

    verify_passkey_signed(
        &ctx.accounts.instructions_sysvar,
        &vault.passkey_pubkey,
        &args.client_data_json,
        &args.authenticator_data,
        &op_msg,
    )?;

    vault.pending_withdrawal = None;

    Ok(())
}
