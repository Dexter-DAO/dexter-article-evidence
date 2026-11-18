//! swap_for_carry — the OPENING bracket of the carry swap leg (Option-A
//! delta-sandwich, designed 2026-07-15; BUILD-SPEC component 6).
//!
//! Transaction shape (v0 + ALT, the K-T4-proven transport):
//!
//!   [i]   secp256r1 precompile — passkey signature over op_msg (below)
//!   [i+1] vault::swap_for_carry            ← THIS instruction (ProgramExec marker)
//!   [i+2] swig::SignV2(Jupiter route)      ← the swap, wrapped
//!   [i+3] vault::finish_swap               ← the CLOSING bracket (delta check)
//!
//! This instruction verifies the user's passkey intent, snapshots the vault's
//! USDC + base-asset balances into the SwapBracket PDA, and — critically —
//! FORWARD-INTROSPECTS the rest of the transaction: [i+2] must be a SignV2
//! wrapping exactly one instruction for the pinned Jupiter program
//! (envelope-lite, verify/swig_transfer.rs), and [i+3] must be this program's
//! finish_swap for this same vault. The bracket therefore cannot be opened
//! without its close in the same atomic transaction: either finish_swap runs
//! and enforces the deltas, or the whole transaction reverts and no state
//! (including `in_flight`) survives.
//!
//! op_msg = "swap_for_carry" || direction(1) || amount_in(8 LE) ||
//!          min_out(8 LE) || base_mint(32) || nonce(8 LE) || expiry_slot(8 LE)
//!
//! The nonce binds the intent to the bracket's durable replay counter
//! (incremented by finish_swap on completion), so one signature authorizes at
//! most one executed swap. The expiry_slot bounds how long an unsubmitted
//! signature stays live.

use anchor_lang::prelude::*;
use anchor_lang::solana_program::sysvar;
use anchor_lang::solana_program::sysvar::instructions::{
    load_current_index_checked, load_instruction_at_checked,
};
use anchor_spl::token::TokenAccount;
use anchor_spl::token_interface::Mint;

use crate::constants::{
    ASSET_CONFIG_SEED, GRAPH_CONFIG_SEED, JUPITER_V6_PROGRAM_ID, SWAP_BRACKET_SEED,
    SWIG_PROGRAM_ID, SWIG_WALLET_ADDRESS_SEED,
};
use crate::state::*;
use anchor_lang::Discriminator;
use crate::verify::swig_transfer::require_swap_envelope_at;
use crate::verify::webauthn::verify_passkey_signed;

pub const SWAP_DIRECTION_BUY: u8 = 0;
pub const SWAP_DIRECTION_SELL: u8 = 1;

/// Max slots between now and a swap intent's expiry_slot. ~1000 slots ≈ 7 min:
/// ample for build+submit, short enough that a leaked-but-unsubmitted intent is
/// not a durable replay token. The nonce + vault binding are the real replay
/// guards; this bounds the window.
pub const MAX_SWAP_INTENT_HORIZON_SLOTS: u64 = 1000;

#[derive(Accounts)]
#[instruction(args: SwapForCarryArgs)]
pub struct SwapForCarry<'info> {
    /// Position 0 — REQUIRED at this index by Swig's ProgramExec authority
    /// validator (same contract as finalize_withdrawal: when the following
    /// SignV2 authenticates, Swig inspects accounts[0..1] of the preceding
    /// top-level instruction and requires [swig, swig_wallet]).
    /// CHECK: address constraint binds it to vault.swig_address.
    #[account(address = vault.swig_address)]
    pub swig: AccountInfo<'info>,
    /// Position 1 — required by Swig's ProgramExec validator (see `swig`).
    /// CHECK: PDA constraint validates the canonical derivation.
    #[account(
        seeds = [SWIG_WALLET_ADDRESS_SEED, swig.key().as_ref()],
        bump,
        seeds::program = SWIG_PROGRAM_ID,
    )]
    pub swig_wallet_address: AccountInfo<'info>,
    /// Boxed: try_accounts stack frame headroom (SBF 4096-byte limit).
    pub vault: Box<Account<'info, Vault>>,
    /// The per-vault bracket: transient snapshot carrier + durable replay
    /// nonce. Created lazily on the vault's first swap; `init_if_needed` is
    /// safe here because every meaningful field is (re)written below on every
    /// pass, `in_flight` is required false on entry, and the PDA seeds bind it
    /// to exactly this vault.
    #[account(
        init_if_needed,
        payer = fee_payer,
        space = 8 + SwapBracket::INIT_SPACE,
        seeds = [SWAP_BRACKET_SEED, vault.key().as_ref()],
        bump,
    )]
    pub swap_bracket: Box<Account<'info, SwapBracket>>,
    /// The approved-asset row for the base asset being bought/sold. Full-seeds
    /// bound to `base_mint` — a config for mint A can never gate a swap in
    /// mint B. Its `enabled` flag is the listing gate.
    #[account(seeds = [ASSET_CONFIG_SEED, base_mint.key().as_ref()], bump = asset_config.bump)]
    pub asset_config: Box<Account<'info, AssetConfig>>,
    /// The base asset of this swap.
    pub base_mint: Box<InterfaceAccount<'info, Mint>>,
    /// The vault's canonical USDC ATA (owner = swig wallet PDA, mint =
    /// graph_config.usdc_mint) — snapshot source. Address-verified in the
    /// handler against the canonical ATA derivation so a caller cannot smuggle
    /// a different funded account into the snapshot.
    pub vault_usdc_ata: Box<Account<'info, TokenAccount>>,
    /// The vault's canonical base-asset ATA — snapshot source. Same canonical
    /// derivation check in the handler.
    pub vault_base_ata: Box<Account<'info, TokenAccount>>,
    /// Graph admin spine — pause gate + canonical usdc_mint.
    #[account(seeds = [GRAPH_CONFIG_SEED], bump = graph_config.bump)]
    pub graph_config: Box<Account<'info, GraphConfig>>,
    /// Pays rent for the bracket PDA on first use (the facilitator fee payer).
    #[account(mut)]
    pub fee_payer: Signer<'info>,
    /// CHECK: instructions sysvar — address-constrained.
    #[account(address = sysvar::instructions::ID)]
    pub instructions_sysvar: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct SwapForCarryArgs {
    /// 0 = buy (USDC -> base), 1 = sell (base -> USDC).
    pub direction: u8,
    /// Maximum input the swap may spend.
    pub amount_in: u64,
    /// Minimum output the vault must receive.
    pub min_out: u64,
    /// Replay counter — must equal the bracket's current nonce.
    pub nonce: u64,
    /// Last slot at which this signed intent may execute.
    pub expiry_slot: u64,
    /// WebAuthn clientDataJSON; challenge must be sha256(op_msg).
    pub client_data_json: Vec<u8>,
    pub authenticator_data: Vec<u8>,
}

pub fn handler(ctx: Context<SwapForCarry>, args: SwapForCarryArgs) -> Result<()> {
    let vault = &ctx.accounts.vault;
    let bracket = &mut ctx.accounts.swap_bracket;

    // ── global + vault gates ────────────────────────────────────────────────
    require!(!ctx.accounts.graph_config.paused, VaultError::GraphPaused);
    require!(
        vault.version == VAULT_VERSION_V7,
        VaultError::UnsupportedVaultVersion
    );
    require!(
        vault.swig_address != Pubkey::default(),
        VaultError::SwapBracketMismatch
    );
    require!(ctx.accounts.asset_config.enabled, VaultError::AssetConfigDisabled);
    require!(
        args.direction == SWAP_DIRECTION_BUY || args.direction == SWAP_DIRECTION_SELL,
        VaultError::SwapUnsupportedDirection
    );
    require!(args.amount_in > 0 && args.min_out > 0, VaultError::SwapDeltaShortfall);

    // ── replay + expiry ─────────────────────────────────────────────────────
    // A fresh bracket (init_if_needed) has nonce 0 / in_flight false, matching
    // the first signed intent. A mid-flight bracket is impossible across txs
    // (forward introspection guarantees finish_swap in the same tx), so this
    // is defense in depth against any future code path that forgets the close.
    require!(!bracket.in_flight, VaultError::SwapBracketMismatch);
    require!(args.nonce == bracket.nonce, VaultError::SwapNonceMismatch);
    let clock = Clock::get()?;
    require!(clock.slot <= args.expiry_slot, VaultError::SwapIntentExpired);
    // Horizon cap: an intent may not be signed to live forever. Bounds how long a
    // signed-but-unsubmitted intent stays a live replay token (defense-in-depth
    // atop the per-vault nonce + vault binding). ~MAX_SWAP_INTENT_HORIZON_SLOTS
    // is generous for build+submit; the service should sign a much tighter window.
    require!(
        args.expiry_slot <= clock.slot.saturating_add(MAX_SWAP_INTENT_HORIZON_SLOTS),
        VaultError::SwapIntentHorizonTooFar
    );

    // ── canonical ATA pins ──────────────────────────────────────────────────
    // Both snapshot accounts must be THE canonical ATAs of the swig wallet PDA
    // for their mints. This is stronger than owner+mint checks: the route's
    // destination must be this exact account for the delta to register, so
    // pinning the address here makes the finish_swap delta check airtight.
    let usdc_mint = ctx.accounts.graph_config.usdc_mint;
    let expected_usdc = anchor_spl::associated_token::get_associated_token_address(
        &ctx.accounts.swig_wallet_address.key(),
        &usdc_mint,
    );
    let expected_base = anchor_spl::associated_token::get_associated_token_address(
        &ctx.accounts.swig_wallet_address.key(),
        &ctx.accounts.base_mint.key(),
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

    // ── passkey intent ──────────────────────────────────────────────────────
    // The op_msg binds the intent to THIS vault (vault.key(), right after the
    // tag — same pattern as attach_node's b"attach_node" || vault.key()). Without
    // it, a signed intent could be replayed onto a DIFFERENT vault that shares
    // the same passkey_pubkey (the per-agent-vaults-under-one-human model makes
    // shared passkeys expected), because a fresh bracket starts at nonce 0 on
    // every vault. Binding the vault makes "executes at most once" true by the
    // signed bytes, not by an off-chain passkey-uniqueness assumption.
    let mut op_msg =
        Vec::with_capacity(b"swap_for_carry".len() + 32 + 1 + 8 + 8 + 32 + 8 + 8);
    op_msg.extend_from_slice(b"swap_for_carry");
    op_msg.extend_from_slice(vault.key().as_ref());
    op_msg.push(args.direction);
    op_msg.extend_from_slice(&args.amount_in.to_le_bytes());
    op_msg.extend_from_slice(&args.min_out.to_le_bytes());
    op_msg.extend_from_slice(ctx.accounts.base_mint.key().as_ref());
    op_msg.extend_from_slice(&args.nonce.to_le_bytes());
    op_msg.extend_from_slice(&args.expiry_slot.to_le_bytes());
    verify_passkey_signed(
        &ctx.accounts.instructions_sysvar,
        &vault.passkey_pubkey,
        &args.client_data_json,
        &args.authenticator_data,
        &op_msg,
    )?;

    // ── forward introspection: the bracket MUST close in this tx ───────────
    let current_index = load_current_index_checked(&ctx.accounts.instructions_sysvar)? as usize;

    // [i+2]: SignV2 wrapping exactly one inner instruction for pinned Jupiter.
    require_swap_envelope_at(
        &ctx.accounts.instructions_sysvar,
        current_index + 1,
        &JUPITER_V6_PROGRAM_ID,
    )?;

    // [i+3]: this program's finish_swap, with THIS vault at accounts[0].
    let closing = load_instruction_at_checked(current_index + 2, &ctx.accounts.instructions_sysvar)
        .map_err(|_| error!(VaultError::SwapBracketNotClosed))?;
    require!(closing.program_id == crate::ID, VaultError::SwapBracketNotClosed);
    require!(
        closing.data.len() >= 8
            && closing.data[..8] == crate::instruction::FinishSwap::DISCRIMINATOR[..],
        VaultError::SwapBracketNotClosed
    );
    let closing_vault = closing
        .accounts
        .first()
        .ok_or(error!(VaultError::SwapBracketNotClosed))?
        .pubkey;
    require_keys_eq!(closing_vault, vault.key(), VaultError::SwapBracketNotClosed);

    // ── snapshot ────────────────────────────────────────────────────────────
    bracket.version = SWAP_BRACKET_VERSION_V1;
    bracket.bump = ctx.bumps.swap_bracket;
    bracket.vault = vault.key();
    bracket.in_flight = true;
    bracket.direction = args.direction;
    bracket.amount_in = args.amount_in;
    bracket.min_out = args.min_out;
    bracket.base_mint = ctx.accounts.base_mint.key();
    bracket.usdc_before = ctx.accounts.vault_usdc_ata.amount;
    bracket.base_before = ctx.accounts.vault_base_ata.amount;
    bracket.snapshot_slot = clock.slot;
    bracket.snapshot_ix_index = current_index as u16;

    Ok(())
}
