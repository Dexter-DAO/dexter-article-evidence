//! Money-leg verification — the trust root of the entire credit ledger.
//!
//! `draw_credit` books a debt of `args.amount`. The USDC that satisfies that
//! debt is NOT moved by `draw_credit` itself; it is moved by the swig `SignV2`
//! instruction that IMMEDIATELY FOLLOWS `draw_credit` in the same atomic tx. If
//! the booked debt and the moved USDC ever diverge, every up-chain credit
//! counter silently diverges from reality. This module makes that divergence
//! impossible: it parses the following SignV2 envelope, locates the wrapped SPL
//! `TransferChecked`, and returns its source + amount + destination + mint so
//! handlers can bind every material money-leg identity.
//!
//! ── THE SUBTLETY (source-verified against the swig program at /tmp/swig-check)
//! The top-level instruction at `current_index + 1` is the swig `SignV2`
//! ENVELOPE, not a bare `TransferChecked`. The real transfer is an INNER
//! instruction that swig executes via CPI, and CPIs are invisible to the
//! instructions sysvar. So we must parse swig's own instruction encoding to
//! reach the wrapped transfer. The byte layout, reverse-engineered from
//! `program/src/actions/sign_v2.rs` (`SignV2Args` + `SignV2::from_instruction_bytes`)
//! and `instructions/src/compact_instructions.rs` (`CompactInstructions::into_bytes`):
//!
//!   SignV2 top-level instruction DATA:
//!     [0..2]   u16 LE  discriminator (== 11, SwigInstruction::SignV2)
//!     [2..4]   u16 LE  instruction_payload_len  (IPL)
//!     [4..8]   u32 LE  role_id
//!     [8..8+IPL]       instruction_payload  (the compact instructions, below)
//!     [8+IPL..]        authority_payload
//!
//!   instruction_payload (compact instructions):
//!     [0]      u8      num_instructions
//!     per instruction:
//!       [+0]   u8      program_id_index
//!       [+1]   u8      num_accounts (M)
//!       [+2..2+M] u8×M account indexes
//!       [2+M]  u16 LE  data_len (D)
//!       [4+M..4+M+D]   instruction data
//!
//!   Both `program_id_index` and the account indexes index into the SignV2
//!   top-level instruction's OWN account-meta list (`next_ix.accounts`), because
//!   swig's `compact_instructions` appends program ids + inner accounts onto that
//!   same list. So we resolve each inner pubkey via `next_ix.accounts[idx]`.
//!
//!   Inner SPL TransferChecked DATA:  [tag(1)=12][amount(8) LE][decimals(1)]
//!   Inner account order: source=0, mint=1, destination=2, authority=3 — so the
//!   destination pubkey is `next_ix.accounts[ account_indexes[2] ]`.
//!
//! ── ANTI-SIPHON STANCE
//! The swig ProgramExec marker that authorizes this SignV2 pins WHICH program may
//! drive the transfer (draw_credit) but does NOT constrain the inner
//! instructions' count, amount, or destination. So a malicious envelope could
//! wrap a second transfer (siphon) or a non-transfer token op (e.g. Approve)
//! alongside the legitimate one. We therefore require the envelope to contain
//! EXACTLY ONE inner instruction and that it be an SPL `TransferChecked` to the
//! classic token program. Anything else (no following ix, wrong program, extra
//! inner instructions, wrong tag, malformed bytes) returns `DebtTransferMismatch`
//! — we never book debt against an envelope we cannot fully reconcile.

use anchor_lang::prelude::*;
use anchor_lang::solana_program::sysvar::instructions::{
    load_current_index_checked, load_instruction_at_checked,
};
use solana_sha256_hasher::hashv;

use crate::constants::SWIG_PROGRAM_ID;
use crate::state::VaultError;

/// Classic SPL Token program id. USDC (the credit unit) is a classic SPL token,
/// not Token-2022.
pub const SPL_TOKEN_PROGRAM_ID: Pubkey = pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");

/// swig `SwigInstruction::SignV2` discriminator (u16 LE).
pub(crate) const SWIG_SIGN_V2_DISCRIMINATOR: u16 = 11;
/// Length of swig's `SignV2Args` header: instruction(u16) + payload_len(u16) + role_id(u32).
pub(crate) const SWIG_SIGN_V2_ARGS_LEN: usize = 8;
/// Pinned Swig c2e8eb4 allocates exactly 100 account snapshots in SignV2.
/// Refuse larger outer account lists before Swig reaches its unchecked snapshot
/// indexing path.
pub const MAX_SAFE_SWIG_SIGN_V2_ACCOUNTS: usize = 100;
pub const GOVERNED_JUPITER_ROUTE_DOMAIN_V1: &[u8; 32] = b"DEXTER_GOVERNED_JUPITER_ROUTE_V1";
/// SPL TransferChecked instruction tag.
const SPL_TRANSFER_CHECKED_TAG: u8 = 12;
/// Minimum TransferChecked data length: tag(1) + amount(8) + decimals(1).
const SPL_TRANSFER_CHECKED_MIN_LEN: usize = 10;
/// Inner-instruction account index of the transfer SOURCE (source=0, mint=1,
/// destination=2, authority=3).
const TRANSFER_CHECKED_SOURCE_ACCOUNT_INDEX: usize = 0;
/// Inner-instruction account index of the transfer MINT (source=0, mint=1,
/// destination=2, authority=3).
const TRANSFER_CHECKED_MINT_ACCOUNT_INDEX: usize = 1;
/// Inner-instruction account index of the transfer DESTINATION (source=0, mint=1,
/// destination=2, authority=3).
const TRANSFER_CHECKED_DEST_ACCOUNT_INDEX: usize = 2;
const TRANSFER_CHECKED_AUTHORITY_ACCOUNT_INDEX: usize = 3;

pub const GOVERNED_SEND_TRANSFER_DOMAIN_V1: &[u8; 32] = b"DEXTER_GOVERNED_SEND_TRANSFER_V1";

/// The decoded money leg: exactly what the following SignV2 envelope will move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoneyLeg {
    pub amount: u64,
    /// Exact source token account. Draw paths bind this to the capital source's
    /// canonical ATA and reject source/destination aliasing; SPL Token otherwise
    /// accepts a self-transfer as a successful no-op.
    pub source: Pubkey,
    pub destination: Pubkey,
    /// The SPL mint the wrapped `TransferChecked` moves. Callers bind this to
    /// their canonical settlement mint (`graph_config.usdc_mint`) so a junk-mint
    /// transfer can never satisfy a USDC-denominated obligation. `TransferChecked`
    /// itself already proves source.mint == dest.mint == this mint on-chain.
    pub mint: Pubkey,
}

/// Exact execution facts extracted from a finalized Swig SignV2 envelope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GovernedSwapEnvelope {
    pub role_id: u32,
    pub route_digest: [u8; 32],
    pub route_program: Pubkey,
    pub authority_account: Pubkey,
}

/// Exact facts from the sole token movement inside a governed Send SignV2.
/// Unlike the credit decoder, this accepts either classic SPL Token or
/// Token-2022, but the caller must supply the exact expected program and bind
/// every returned field to live mint/account state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GovernedSendEnvelope {
    pub role_id: u32,
    pub token_program: Pubkey,
    pub source: Pubkey,
    pub mint: Pubkey,
    pub destination: Pubkey,
    pub transfer_authority: Pubkey,
    /// Swig acting-authority account selected by the one-byte SignV2 tail.
    pub authority_account: Pubkey,
    pub amount: u64,
    pub decimals: u8,
    pub transfer_instruction_digest: [u8; 32],
}

/// Decode one terminal governed Send envelope. There is deliberately no
/// generic "execute arbitrary instruction" result: the compact payload must
/// end after exactly one four-account TransferChecked and the Swig authority
/// tail must be exactly one account index.
pub fn decode_governed_send_envelope(
    ix: &anchor_lang::solana_program::instruction::Instruction,
    expected_token_program: &Pubkey,
) -> Result<GovernedSendEnvelope> {
    require!(
        ix.program_id == SWIG_PROGRAM_ID
            && (*expected_token_program == anchor_spl::token::ID
                || *expected_token_program == anchor_spl::token_2022::ID),
        VaultError::GovernedSendEnvelopeInvalid
    );
    require!(
        ix.accounts.len() <= MAX_SAFE_SWIG_SIGN_V2_ACCOUNTS,
        VaultError::GovernedSendEnvelopeInvalid
    );
    let data = ix.data.as_slice();
    require!(
        data.len() >= SWIG_SIGN_V2_ARGS_LEN,
        VaultError::GovernedSendEnvelopeInvalid
    );
    require!(
        u16::from_le_bytes([data[0], data[1]]) == SWIG_SIGN_V2_DISCRIMINATOR,
        VaultError::GovernedSendEnvelopeInvalid
    );
    let payload_len = u16::from_le_bytes([data[2], data[3]]) as usize;
    let role_id = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let payload_end = SWIG_SIGN_V2_ARGS_LEN
        .checked_add(payload_len)
        .ok_or_else(|| error!(VaultError::GovernedSendEnvelopeInvalid))?;
    require!(
        payload_end
            .checked_add(1)
            .ok_or_else(|| error!(VaultError::GovernedSendEnvelopeInvalid))?
            == data.len(),
        VaultError::GovernedSendEnvelopeInvalid
    );
    let payload = &data[SWIG_SIGN_V2_ARGS_LEN..payload_end];
    require!(
        payload.first().copied() == Some(1),
        VaultError::GovernedSendEnvelopeInvalid
    );

    let mut cursor = 1usize;
    let pid_idx = *payload
        .get(cursor)
        .ok_or_else(|| error!(VaultError::GovernedSendEnvelopeInvalid))? as usize;
    cursor += 1;
    let num_accounts = *payload
        .get(cursor)
        .ok_or_else(|| error!(VaultError::GovernedSendEnvelopeInvalid))? as usize;
    cursor += 1;
    require!(num_accounts == 4, VaultError::GovernedSendEnvelopeInvalid);
    let account_indexes_end = cursor
        .checked_add(num_accounts)
        .ok_or_else(|| error!(VaultError::GovernedSendEnvelopeInvalid))?;
    require!(
        account_indexes_end <= payload.len(),
        VaultError::GovernedSendEnvelopeInvalid
    );
    let account_indexes = &payload[cursor..account_indexes_end];
    cursor = account_indexes_end;
    let data_len_end = cursor
        .checked_add(2)
        .ok_or_else(|| error!(VaultError::GovernedSendEnvelopeInvalid))?;
    require!(
        data_len_end <= payload.len(),
        VaultError::GovernedSendEnvelopeInvalid
    );
    let inner_data_len = u16::from_le_bytes([payload[cursor], payload[cursor + 1]]) as usize;
    cursor = data_len_end;
    let inner_data_end = cursor
        .checked_add(inner_data_len)
        .ok_or_else(|| error!(VaultError::GovernedSendEnvelopeInvalid))?;
    require!(
        inner_data_end == payload.len() && inner_data_len == SPL_TRANSFER_CHECKED_MIN_LEN,
        VaultError::GovernedSendEnvelopeInvalid
    );
    let inner_data = &payload[cursor..inner_data_end];
    require!(
        inner_data[0] == SPL_TRANSFER_CHECKED_TAG,
        VaultError::GovernedSendEnvelopeInvalid
    );

    let program_meta = ix
        .accounts
        .get(pid_idx)
        .ok_or_else(|| error!(VaultError::GovernedSendEnvelopeInvalid))?;
    require_keys_eq!(
        program_meta.pubkey,
        *expected_token_program,
        VaultError::GovernedSendEnvelopeInvalid
    );
    require!(
        !program_meta.is_signer && !program_meta.is_writable,
        VaultError::GovernedSendEnvelopeInvalid
    );

    let resolve =
        |inner_index: usize| -> Result<&anchor_lang::solana_program::instruction::AccountMeta> {
            let outer_index = *account_indexes
                .get(inner_index)
                .ok_or_else(|| error!(VaultError::GovernedSendEnvelopeInvalid))?
                as usize;
            ix.accounts
                .get(outer_index)
                .ok_or_else(|| error!(VaultError::GovernedSendEnvelopeInvalid))
        };
    let source_meta = resolve(TRANSFER_CHECKED_SOURCE_ACCOUNT_INDEX)?;
    let mint_meta = resolve(TRANSFER_CHECKED_MINT_ACCOUNT_INDEX)?;
    let destination_meta = resolve(TRANSFER_CHECKED_DEST_ACCOUNT_INDEX)?;
    let transfer_authority_meta = resolve(TRANSFER_CHECKED_AUTHORITY_ACCOUNT_INDEX)?;
    require!(
        source_meta.is_writable
            && destination_meta.is_writable
            && !mint_meta.is_writable
            && !source_meta.is_signer
            && !mint_meta.is_signer
            && !destination_meta.is_signer
            && !transfer_authority_meta.is_signer,
        VaultError::GovernedSendEnvelopeInvalid
    );

    let authority_meta = ix
        .accounts
        .get(data[payload_end] as usize)
        .ok_or_else(|| error!(VaultError::GovernedSendEnvelopeInvalid))?;
    let amount = u64::from_le_bytes(
        inner_data[1..9]
            .try_into()
            .map_err(|_| error!(VaultError::GovernedSendEnvelopeInvalid))?,
    );
    let decimals = inner_data[9];

    let mut canonical = Vec::with_capacity(32 * 6 + 32);
    canonical.extend_from_slice(expected_token_program.as_ref());
    canonical.extend_from_slice(source_meta.pubkey.as_ref());
    canonical.extend_from_slice(mint_meta.pubkey.as_ref());
    canonical.extend_from_slice(destination_meta.pubkey.as_ref());
    canonical.extend_from_slice(transfer_authority_meta.pubkey.as_ref());
    canonical.extend_from_slice(&amount.to_le_bytes());
    canonical.push(decimals);
    Ok(GovernedSendEnvelope {
        role_id,
        token_program: *expected_token_program,
        source: source_meta.pubkey,
        mint: mint_meta.pubkey,
        destination: destination_meta.pubkey,
        transfer_authority: transfer_authority_meta.pubkey,
        authority_account: authority_meta.pubkey,
        amount,
        decimals,
        transfer_instruction_digest: hashv(&[GOVERNED_SEND_TRANSFER_DOMAIN_V1, &canonical])
            .to_bytes(),
    })
}

/// Require one pinned Jupiter instruction and hash its resolved ordered account
/// metas plus data. ProgramExec and Ed25519Session use a one-byte authority
/// account index; trailing compact or authority bytes are rejected.
pub fn decode_governed_swap_envelope(
    ix: &anchor_lang::solana_program::instruction::Instruction,
    pinned_program: &Pubkey,
) -> Result<GovernedSwapEnvelope> {
    Ok(decode_governed_swap_instruction(ix, pinned_program)?.0)
}

pub fn decode_governed_swap_instruction(
    ix: &anchor_lang::solana_program::instruction::Instruction,
    pinned_program: &Pubkey,
) -> Result<(
    GovernedSwapEnvelope,
    anchor_lang::solana_program::instruction::Instruction,
)> {
    require!(
        ix.program_id == SWIG_PROGRAM_ID,
        VaultError::GovernedSwapEnvelopeInvalid
    );
    require!(
        ix.accounts.len() <= MAX_SAFE_SWIG_SIGN_V2_ACCOUNTS,
        VaultError::GovernedSwapEnvelopeInvalid
    );
    let data = ix.data.as_slice();
    require!(
        data.len() >= SWIG_SIGN_V2_ARGS_LEN,
        VaultError::GovernedSwapEnvelopeInvalid
    );
    require!(
        u16::from_le_bytes([data[0], data[1]]) == SWIG_SIGN_V2_DISCRIMINATOR,
        VaultError::GovernedSwapEnvelopeInvalid
    );
    let payload_len = u16::from_le_bytes([data[2], data[3]]) as usize;
    let role_id = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let payload_end = SWIG_SIGN_V2_ARGS_LEN
        .checked_add(payload_len)
        .ok_or_else(|| error!(VaultError::GovernedSwapEnvelopeInvalid))?;
    require!(
        payload_end
            .checked_add(1)
            .ok_or_else(|| error!(VaultError::GovernedSwapEnvelopeInvalid))?
            == data.len(),
        VaultError::GovernedSwapEnvelopeInvalid
    );
    let payload = &data[SWIG_SIGN_V2_ARGS_LEN..payload_end];
    require!(
        payload.first().copied() == Some(1),
        VaultError::GovernedSwapEnvelopeInvalid
    );

    let mut cursor = 1usize;
    let pid_idx = *payload
        .get(cursor)
        .ok_or_else(|| error!(VaultError::GovernedSwapEnvelopeInvalid))? as usize;
    cursor += 1;
    let num_accounts = *payload
        .get(cursor)
        .ok_or_else(|| error!(VaultError::GovernedSwapEnvelopeInvalid))? as usize;
    cursor += 1;
    let account_indexes_end = cursor
        .checked_add(num_accounts)
        .ok_or_else(|| error!(VaultError::GovernedSwapEnvelopeInvalid))?;
    require!(
        account_indexes_end <= payload.len(),
        VaultError::GovernedSwapEnvelopeInvalid
    );
    let account_indexes = &payload[cursor..account_indexes_end];
    cursor = account_indexes_end;
    let data_len_end = cursor
        .checked_add(2)
        .ok_or_else(|| error!(VaultError::GovernedSwapEnvelopeInvalid))?;
    require!(
        data_len_end <= payload.len(),
        VaultError::GovernedSwapEnvelopeInvalid
    );
    let inner_data_len = u16::from_le_bytes([payload[cursor], payload[cursor + 1]]) as usize;
    cursor = data_len_end;
    let inner_data_end = cursor
        .checked_add(inner_data_len)
        .ok_or_else(|| error!(VaultError::GovernedSwapEnvelopeInvalid))?;
    require!(
        inner_data_end == payload.len(),
        VaultError::GovernedSwapEnvelopeInvalid
    );
    let inner_data = &payload[cursor..inner_data_end];

    let program_id = ix
        .accounts
        .get(pid_idx)
        .ok_or_else(|| error!(VaultError::GovernedSwapEnvelopeInvalid))?
        .pubkey;
    require_keys_eq!(
        program_id,
        *pinned_program,
        VaultError::GovernedSwapEnvelopeInvalid
    );

    let mut canonical = Vec::with_capacity(
        38usize
            .saturating_add(num_accounts.saturating_mul(33))
            .saturating_add(inner_data.len()),
    );
    canonical.extend_from_slice(program_id.as_ref());
    canonical.extend_from_slice(&(num_accounts as u16).to_le_bytes());
    for account_index in account_indexes {
        let meta = ix
            .accounts
            .get(*account_index as usize)
            .ok_or_else(|| error!(VaultError::GovernedSwapEnvelopeInvalid))?;
        canonical.extend_from_slice(meta.pubkey.as_ref());
        canonical.push((meta.is_signer as u8) | ((meta.is_writable as u8) << 1));
    }
    canonical.extend_from_slice(&(inner_data.len() as u32).to_le_bytes());
    canonical.extend_from_slice(inner_data);

    let authority_account = ix
        .accounts
        .get(data[payload_end] as usize)
        .ok_or_else(|| error!(VaultError::GovernedSwapEnvelopeInvalid))?
        .pubkey;
    let inner_instruction = anchor_lang::solana_program::instruction::Instruction {
        program_id,
        accounts: account_indexes
            .iter()
            .map(|index| ix.accounts[*index as usize].clone())
            .collect(),
        data: inner_data.to_vec(),
    };
    Ok((
        GovernedSwapEnvelope {
            role_id,
            route_digest: hashv(&[GOVERNED_JUPITER_ROUTE_DOMAIN_V1, &canonical]).to_bytes(),
            route_program: program_id,
            authority_account,
        },
        inner_instruction,
    ))
}

/// Bind a de-risking payout (repay / seize / cascade) to the node's recorded
/// financier. The financier (a swig) holds the credit asset (`usdc_mint` — the
/// graph's canonical settlement mint, USDC on mainnet) in its swig-wallet PDA's
/// associated token account; we derive that ATA from `financier` + `usdc_mint`
/// and require the money-leg destination to equal it. The ATA address encodes
/// BOTH owner and mint, so this pins owner AND asset in one check — repaid/seized
/// funds can only land in the financier's ATA on the canonical mint, never an
/// operator-chosen address and never a worthless substitute token. This makes
/// "financier-made-whole" a PROGRAM INVARIANT. Pairs with draw_credit's source
/// bind (financier_swig == node.financier) to close the capital loop on-chain.
pub fn require_financier_destination(
    money_leg: &MoneyLeg,
    financier: &Pubkey,
    usdc_mint: &Pubkey,
) -> Result<()> {
    let (fin_wallet, _) = Pubkey::find_program_address(
        &[
            crate::constants::SWIG_WALLET_ADDRESS_SEED,
            financier.as_ref(),
        ],
        &crate::constants::SWIG_PROGRAM_ID,
    );
    let expected =
        anchor_spl::associated_token::get_associated_token_address(&fin_wallet, usdc_mint);
    require_keys_eq!(
        money_leg.destination,
        expected,
        crate::state::VaultError::FinancierMismatch
    );
    Ok(())
}

/// Reads the instruction IMMEDIATELY FOLLOWING the current one, requires it to be
/// a swig `SignV2` envelope wrapping EXACTLY ONE SPL `TransferChecked`, and
/// returns that transfer's source + amount + destination + mint. Any deviation
/// → `DebtTransferMismatch`.
pub fn decode_following_swig_transfer(instructions_sysvar: &AccountInfo) -> Result<MoneyLeg> {
    let mut legs = decode_following_swig_transfers(instructions_sysvar, 1)?;
    Ok(legs.pop().unwrap())
}

/// Two-leg variant for finalize_withdrawal's fee model: the envelope must wrap
/// EXACTLY TWO SPL `TransferChecked`s. Returns (leg0, leg1) in payload order —
/// the caller binds leg0 = net → user ATA, leg1 = fee → treasury ATA. The same
/// anti-siphon stance applies: any third instruction, non-transfer op, or
/// malformed byte → `DebtTransferMismatch`.
pub fn decode_following_swig_transfer_pair(
    instructions_sysvar: &AccountInfo,
) -> Result<(MoneyLeg, MoneyLeg)> {
    let mut legs = decode_following_swig_transfers(instructions_sysvar, 2)?;
    let second = legs.pop().unwrap();
    let first = legs.pop().unwrap();
    Ok((first, second))
}

/// Envelope-LITE check for the carry swap leg (delta-sandwich, 2026-07-15):
/// the instruction at `index` must be a swig SignV2 wrapping EXACTLY ONE inner
/// instruction whose program is `pinned_program` (Jupiter v6). We deliberately
/// do NOT parse the route's internals — the custody guarantee is finish_swap's
/// balance-delta check, not a byte-reconciliation of a third party's encoding.
/// This check exists to kill the sibling-siphon class outright: with one inner
/// instruction and a pinned program, no extra TransferChecked/Approve can ride
/// in the envelope, so the residual attack surface is route QUALITY, which the
/// passkey-signed min_out bounds to the slippage budget.
pub fn require_swap_envelope_at(
    instructions_sysvar: &AccountInfo,
    index: usize,
    pinned_program: &Pubkey,
) -> Result<()> {
    let ix = load_instruction_at_checked(index, instructions_sysvar)
        .map_err(|_| error!(VaultError::SwapEnvelopeInvalid))?;
    require!(
        ix.program_id == SWIG_PROGRAM_ID,
        VaultError::SwapEnvelopeInvalid
    );
    require!(
        ix.accounts.len() <= MAX_SAFE_SWIG_SIGN_V2_ACCOUNTS,
        VaultError::SwapEnvelopeInvalid
    );

    let data = ix.data.as_slice();
    require!(
        data.len() >= SWIG_SIGN_V2_ARGS_LEN,
        VaultError::SwapEnvelopeInvalid
    );
    let disc = u16::from_le_bytes([data[0], data[1]]);
    require!(
        disc == SWIG_SIGN_V2_DISCRIMINATOR,
        VaultError::SwapEnvelopeInvalid
    );
    let ipl = u16::from_le_bytes([data[2], data[3]]) as usize;
    let payload_end = SWIG_SIGN_V2_ARGS_LEN
        .checked_add(ipl)
        .ok_or_else(|| error!(VaultError::SwapEnvelopeInvalid))?;
    require!(payload_end <= data.len(), VaultError::SwapEnvelopeInvalid);
    let payload = &data[SWIG_SIGN_V2_ARGS_LEN..payload_end];

    // Exactly ONE inner instruction…
    let num_instructions = *payload
        .first()
        .ok_or_else(|| error!(VaultError::SwapEnvelopeInvalid))?;
    require!(num_instructions == 1, VaultError::SwapEnvelopeInvalid);

    // …whose program id (resolved through the envelope's own account metas,
    // same indexing rule as decode_envelope) is the pinned swap program.
    let pid_idx = *payload
        .get(1)
        .ok_or_else(|| error!(VaultError::SwapEnvelopeInvalid))? as usize;
    let program_id = ix
        .accounts
        .get(pid_idx)
        .ok_or_else(|| error!(VaultError::SwapEnvelopeInvalid))?
        .pubkey;
    require!(
        program_id == *pinned_program,
        VaultError::SwapEnvelopeInvalid
    );
    Ok(())
}

/// 1-OR-2-leg variant for `settle_tab_voucher`'s optional platform fee (board
/// #75): peeks the envelope's declared inner-instruction count (after
/// validating the SignV2 header framing), requires it to be 1 or 2, then runs
/// the full anti-siphon decode at exactly that count. The CALLER decides
/// whether 2 legs are permitted (fee split enabled) and binds every byte;
/// this function only routes the count. 3+ legs, 0 legs, or any malformed
/// framing → `DebtTransferMismatch`.
pub fn decode_following_swig_transfer_one_or_two(
    instructions_sysvar: &AccountInfo,
) -> Result<Vec<MoneyLeg>> {
    let current_index = load_current_index_checked(instructions_sysvar)? as usize;
    let next_ix = load_instruction_at_checked(current_index + 1, instructions_sysvar)
        .map_err(|_| error!(VaultError::DebtTransferMismatch))?;
    let count = peek_envelope_count(&next_ix)?;
    require!(count == 1 || count == 2, VaultError::DebtTransferMismatch);
    decode_envelope(&next_ix, count)
}

/// Validate the SignV2 header framing (program id, discriminator, payload
/// bounds) and return the envelope's DECLARED inner-instruction count.
/// Routing information only — `decode_envelope` re-validates the full framing
/// including the count-vs-content reconciliation, so a lying count still
/// fails the decode.
pub(crate) fn peek_envelope_count(
    next_ix: &anchor_lang::solana_program::instruction::Instruction,
) -> Result<u8> {
    require!(
        next_ix.program_id == SWIG_PROGRAM_ID,
        VaultError::DebtTransferMismatch
    );
    require!(
        next_ix.accounts.len() <= MAX_SAFE_SWIG_SIGN_V2_ACCOUNTS,
        VaultError::DebtTransferMismatch
    );
    let data = next_ix.data.as_slice();
    require!(
        data.len() >= SWIG_SIGN_V2_ARGS_LEN,
        VaultError::DebtTransferMismatch
    );
    let disc = u16::from_le_bytes([data[0], data[1]]);
    require!(
        disc == SWIG_SIGN_V2_DISCRIMINATOR,
        VaultError::DebtTransferMismatch
    );
    let ipl = u16::from_le_bytes([data[2], data[3]]) as usize;
    let payload_end = SWIG_SIGN_V2_ARGS_LEN
        .checked_add(ipl)
        .ok_or_else(|| error!(VaultError::DebtTransferMismatch))?;
    require!(payload_end <= data.len(), VaultError::DebtTransferMismatch);
    let payload = &data[SWIG_SIGN_V2_ARGS_LEN..payload_end];
    payload
        .first()
        .copied()
        .ok_or_else(|| error!(VaultError::DebtTransferMismatch))
}

fn decode_following_swig_transfers(
    instructions_sysvar: &AccountInfo,
    expected_count: u8,
) -> Result<Vec<MoneyLeg>> {
    let current_index = load_current_index_checked(instructions_sysvar)? as usize;

    // The money move MUST be the next top-level instruction in this atomic tx.
    let next_ix = load_instruction_at_checked(current_index + 1, instructions_sysvar)
        .map_err(|_| error!(VaultError::DebtTransferMismatch))?;

    decode_envelope(&next_ix, expected_count)
}

/// Envelope core, separated from the sysvar read so it is unit-testable against
/// byte-exact fixtures. Parses the SignV2 header, requires EXACTLY
/// `expected_count` inner instructions, parses each as a classic-SPL
/// `TransferChecked`, and requires the compact payload to end exactly at the
/// last parsed instruction (no trailing bytes the count didn't account for).
pub(crate) fn decode_envelope(
    next_ix: &anchor_lang::solana_program::instruction::Instruction,
    expected_count: u8,
) -> Result<Vec<MoneyLeg>> {
    require!(
        next_ix.program_id == SWIG_PROGRAM_ID,
        VaultError::DebtTransferMismatch
    );
    require!(
        next_ix.accounts.len() <= MAX_SAFE_SWIG_SIGN_V2_ACCOUNTS,
        VaultError::DebtTransferMismatch
    );

    let data = next_ix.data.as_slice();
    require!(
        data.len() >= SWIG_SIGN_V2_ARGS_LEN,
        VaultError::DebtTransferMismatch
    );
    let disc = u16::from_le_bytes([data[0], data[1]]);
    require!(
        disc == SWIG_SIGN_V2_DISCRIMINATOR,
        VaultError::DebtTransferMismatch
    );
    let ipl = u16::from_le_bytes([data[2], data[3]]) as usize;
    let payload_end = SWIG_SIGN_V2_ARGS_LEN
        .checked_add(ipl)
        .ok_or_else(|| error!(VaultError::DebtTransferMismatch))?;
    require!(payload_end <= data.len(), VaultError::DebtTransferMismatch);
    let payload = &data[SWIG_SIGN_V2_ARGS_LEN..payload_end];

    // ── compact instructions ────────────────────────────────────────────────
    let num_instructions = *payload
        .first()
        .ok_or_else(|| error!(VaultError::DebtTransferMismatch))?;
    // Anti-siphon: the envelope must wrap EXACTLY the transfers we reconcile —
    // no extra transfer, no approve, nothing else.
    require!(
        num_instructions == expected_count,
        VaultError::DebtTransferMismatch
    );

    let mut cursor = 1usize;
    let mut legs = Vec::with_capacity(expected_count as usize);
    for _ in 0..expected_count {
        let (leg, next_cursor) = parse_inner_transfer(next_ix, payload, cursor)?;
        legs.push(leg);
        cursor = next_cursor;
    }
    // The payload must contain the counted instructions and nothing more — a
    // trailing blob the count didn't cover is a malformed envelope, not slack.
    require!(cursor == payload.len(), VaultError::DebtTransferMismatch);

    Ok(legs)
}

/// Parse ONE compact inner instruction at `cursor`, require it to be a classic
/// SPL `TransferChecked`, and return its money leg + the cursor one past its
/// data. Byte-identical to the original single-leg parse, relocated into a
/// loop body for the N-leg envelope.
fn parse_inner_transfer(
    next_ix: &anchor_lang::solana_program::instruction::Instruction,
    payload: &[u8],
    mut cursor: usize,
) -> Result<(MoneyLeg, usize)> {
    // program_id_index
    let pid_idx = *payload
        .get(cursor)
        .ok_or_else(|| error!(VaultError::DebtTransferMismatch))? as usize;
    cursor += 1;

    // num_accounts + account indexes
    let num_accounts = *payload
        .get(cursor)
        .ok_or_else(|| error!(VaultError::DebtTransferMismatch))? as usize;
    cursor += 1;
    let acc_end = cursor
        .checked_add(num_accounts)
        .ok_or_else(|| error!(VaultError::DebtTransferMismatch))?;
    require!(acc_end <= payload.len(), VaultError::DebtTransferMismatch);
    let account_indexes = &payload[cursor..acc_end];
    cursor = acc_end;

    // data_len (u16 LE) + data
    let dlen_end = cursor
        .checked_add(2)
        .ok_or_else(|| error!(VaultError::DebtTransferMismatch))?;
    require!(dlen_end <= payload.len(), VaultError::DebtTransferMismatch);
    let data_len = u16::from_le_bytes([payload[cursor], payload[cursor + 1]]) as usize;
    cursor = dlen_end;
    let idata_end = cursor
        .checked_add(data_len)
        .ok_or_else(|| error!(VaultError::DebtTransferMismatch))?;
    require!(idata_end <= payload.len(), VaultError::DebtTransferMismatch);
    let idata = &payload[cursor..idata_end];
    cursor = idata_end;

    // The inner instruction MUST be an SPL TransferChecked on the classic
    // token program.
    let program_id = next_ix
        .accounts
        .get(pid_idx)
        .ok_or_else(|| error!(VaultError::DebtTransferMismatch))?
        .pubkey;
    require!(
        program_id == SPL_TOKEN_PROGRAM_ID,
        VaultError::DebtTransferMismatch
    );
    require!(
        idata.len() >= SPL_TRANSFER_CHECKED_MIN_LEN,
        VaultError::DebtTransferMismatch
    );
    require!(
        idata[0] == SPL_TRANSFER_CHECKED_TAG,
        VaultError::DebtTransferMismatch
    );

    let amount = u64::from_le_bytes(
        idata[1..9]
            .try_into()
            .map_err(|_| error!(VaultError::DebtTransferMismatch))?,
    );

    let source_meta_idx = *account_indexes
        .get(TRANSFER_CHECKED_SOURCE_ACCOUNT_INDEX)
        .ok_or_else(|| error!(VaultError::DebtTransferMismatch))? as usize;
    let source = next_ix
        .accounts
        .get(source_meta_idx)
        .ok_or_else(|| error!(VaultError::DebtTransferMismatch))?
        .pubkey;

    let dest_meta_idx = *account_indexes
        .get(TRANSFER_CHECKED_DEST_ACCOUNT_INDEX)
        .ok_or_else(|| error!(VaultError::DebtTransferMismatch))? as usize;
    let destination = next_ix
        .accounts
        .get(dest_meta_idx)
        .ok_or_else(|| error!(VaultError::DebtTransferMismatch))?
        .pubkey;

    let mint_meta_idx = *account_indexes
        .get(TRANSFER_CHECKED_MINT_ACCOUNT_INDEX)
        .ok_or_else(|| error!(VaultError::DebtTransferMismatch))? as usize;
    let mint = next_ix
        .accounts
        .get(mint_meta_idx)
        .ok_or_else(|| error!(VaultError::DebtTransferMismatch))?
        .pubkey;

    Ok((
        MoneyLeg {
            amount,
            source,
            destination,
            mint,
        },
        cursor,
    ))
}

// ── P5: whole-transaction Swig binding scan ─────────────────────────────────
//
// Every vault ProgramExec-marker instruction binds its money/authority leg to
// the Swig `SignV2` (or `AddAuthorityV1`) at a FIXED adjacent index (e.g.
// `current_index + 1`). Swig's ProgramExec authority, however, authorizes a
// `SignV2` by checking only ONE adjacent instruction — and WHICH one depends on
// the deployed Swig revision:
//   - pinned/vendored `c2e8eb4` checks `current_index - 1` only (1-byte sysvar
//     index payload), so a second SignV2 would need its OWN preceding marker;
//   - a later mainnet Swig accepts a payload NAMING an instruction index and
//     refuses only `idx >= current_index`, so a SECOND SignV2 placed later in
//     the same transaction can point BACK at the same vault marker and pass.
//
// Under the permissive revision an attacker who assembles the transaction
// (dexter-api / facilitator) could append a second SignV2 that reuses one
// owner-signed marker to move money twice. The vault must not depend on which
// Swig is live.
//
// THE RULE (marker-union, `require_swig_ixs_marker_bound`): a Swig instruction
// on a guarded swig is legitimate ONLY at an index credited by some vault
// MARKER M in the transaction, where ALL hold:
//   (a) M is a top-level instruction of THIS program (`program_id == crate::ID`);
//   (b) M's account 0 is the swig being scanned (so a marker binding a leg on a
//       DIFFERENT wallet — e.g. draw_credit on the financier — never credits an
//       index on this one);
//   (c) the index is one of M's content-checked leg offsets (`vault_marker_bound_offsets`).
// A tx can legitimately carry SEVERAL markers, each binding its own leg (e.g.
// the one-step stock buy: `provision_governed_asset_role_v2` binds its
// `UpdateAuthorityV1` at +1 AND `swap_for_carry` binds its `SignV2` at +1), so
// crediting the UNION over all markers admits every legitimate leg while a
// per-marker "only MY +1" scan would wrongly reject a sibling marker's leg.
// A back-reference second SignV2 still dies: it lands at an index no marker
// credits (adding a marker to claim it needs that marker's own passkey/authority
// gate, and each marker binds exactly one leg), regardless of Swig's adjacency
// semantics.
//
// CLASS RULE (same pass, every top-level instruction, every scan): apart from
// this program's own instructions, the ONLY top-level instructions allowed to
// list the swig CONFIG account in any slot are Swig instructions with it at
// account 0 — which the marker rule above then binds. That closes Swig role
// management reached through another program's CPI (or through a Swig
// instruction on a different wallet) for every marker, independent of Swig's
// allowlist. ComputeBudget and the Ed25519/Secp256r1 checks list no accounts;
// ATA creates name the swig WALLET PDA, never the config.
//
// The scan is ALLOCATION-FREE (errors are built lazily, only on the failing path): it runs up to twice per tx (credit tab settle)
// under a 300k CU limit, and the BPF heap is a 32 KiB bump allocator that never
// frees — so it borrows the sysvar bytes once and reads only the three fields it
// needs per instruction, never building an owned `Instruction` on the heap.

fn blob_u16(blob: &[u8], at: usize) -> Option<usize> {
    let end = at.checked_add(2)?;
    blob.get(at..end)
        .map(|b| u16::from_le_bytes([b[0], b[1]]) as usize)
}

fn blob_pubkey(blob: &[u8], at: usize) -> Option<Pubkey> {
    let end = at.checked_add(32)?;
    blob.get(at..end).map(|b| {
        let mut k = [0u8; 32];
        k.copy_from_slice(b);
        Pubkey::new_from_array(k)
    })
}

/// One instruction's identity, parsed from the instructions-sysvar blob with
/// only stack copies (Pubkey is 32 bytes, Copy) — no heap. Serialization
/// (`solana::serialize_instructions`): `[0..2]` num_instructions (u16); then one
/// u16 absolute offset per instruction; at each offset num_accounts (u16), then
/// per account `[flags u8][pubkey 32]` (33 B), then program_id (32 B), then
/// data_len (u16), then data.
#[derive(Clone, Copy)]
struct SysvarIxRef {
    program_id: Pubkey,
    account0: Option<Pubkey>,
    disc: Option<[u8; 8]>,
    /// Blob offset of the first account meta, and the account count — so the
    /// class rule can walk every listed account in place (bounds already proven:
    /// `program_id` sits right after the last meta).
    accounts_at: usize,
    num_accounts: usize,
}

/// Parse instruction `idx`'s header. `None` on any malformed/short read, so the
/// caller fails closed (`UnboundSwigInstruction`).
fn parse_sysvar_ix(blob: &[u8], idx: usize) -> Option<SysvarIxRef> {
    let off = blob_u16(blob, 2usize.checked_add(idx.checked_mul(2)?)?)?;
    let num_accounts = blob_u16(blob, off)?;
    let accounts_start = off.checked_add(2)?;
    let account0 = if num_accounts >= 1 {
        // first account meta = [flags:u8][pubkey:32]; the pubkey is at +1.
        Some(blob_pubkey(blob, accounts_start.checked_add(1)?)?)
    } else {
        None
    };
    let program_id_at = accounts_start.checked_add(num_accounts.checked_mul(33)?)?;
    let program_id = blob_pubkey(blob, program_id_at)?;
    let data_len_at = program_id_at.checked_add(32)?;
    let data_len = blob_u16(blob, data_len_at)?;
    let data_start = data_len_at.checked_add(2)?;
    let disc = if data_len >= 8 {
        let end = data_start.checked_add(8)?;
        blob.get(data_start..end).map(|b| {
            let mut d = [0u8; 8];
            d.copy_from_slice(b);
            d
        })
    } else {
        None
    };
    Some(SysvarIxRef {
        program_id,
        account0,
        disc,
        accounts_at: accounts_start,
        num_accounts,
    })
}

/// Does instruction `ix` list `key` in ANY account slot? Compares the borrowed
/// bytes in place (no copy, no heap). `None` on a short read (fail closed).
fn sysvar_ix_lists_key(blob: &[u8], ix: &SysvarIxRef, key: &Pubkey) -> Option<bool> {
    for a in 0..ix.num_accounts {
        // meta a = [flags:u8][pubkey:32] at accounts_at + 33a; the pubkey is at +1.
        let at = ix.accounts_at.checked_add(a.checked_mul(33)?)?.checked_add(1)?;
        let end = at.checked_add(32)?;
        if blob.get(at..end)? == key.as_ref() {
            return Some(true);
        }
    }
    Some(false)
}

/// The offsets, relative to a vault marker instruction, at which that marker
/// binds (and its handler content-checks) a Swig leg on its own swig — or `None`
/// if the instruction is not a Swig-binding vault marker. Every CORE marker
/// binds exactly its immediately following leg (`+1`): finalize_withdrawal,
/// settle_tab_voucher (the cash leg; absent in the pure-credit shape, where +1
/// is draw_credit — a vault ix, not a Swig leg), settle_locked_voucher,
/// repay_credit, seize_collateral, seize_ancestor, settle_liquidation,
/// grant_agent_session, manage_governed_trade_grant,
/// provision_governed_asset_role_v2 (its ArmTrade UpdateAuthorityV1 / AddAuthorityV1),
/// swap_for_carry, draw_credit, draw_leverage.
///
/// NOT markers: the `authorize_governed_*` instructions (each requires a
/// following `swap_for_carry`, whose +1 already binds the shared SignV2),
/// `finish_*` (carry no Swig leg), `manage_governed_trade_grant_v2` and
/// `settle_voucher`/`lock_voucher`/`register_session_key` (assert no following
/// Swig ix / move no money).
///
/// ACTIVATION LANE (J): `provision_wallet_role` binds its `AddAuthorityV1` at
/// `+1` like the core markers. `activate_wallet` is the one marker whose legs
/// PRECEDE it — it runs last and verifies the top-level birth that already ran:
/// `[c-6]` CreateV1, `[c-5..c-3]` 3×AddAuthorityV1, `[c-2]` the SignV2 fee leg,
/// `[c-1]` UpdateAuthorityV1. A fee = 0 birth has no SignV2, so its handler
/// content-checks `[c-5, c-1]` and itself refuses a Swig instruction on its swig
/// at `c-6` (the one slot this static table credits but that birth leaves
/// unchecked).
fn vault_marker_bound_offsets(disc: &[u8]) -> Option<BoundLegs> {
    use crate::instruction as vix;
    const PLUS_ONE: BoundLegs = BoundLegs::After(&[1]);
    const BIRTH: BoundLegs = BoundLegs::Before(&[1, 2, 3, 4, 5, 6]);
    let is = |m: &[u8]| disc == m;
    if is(vix::FinalizeWithdrawal::DISCRIMINATOR)
        || is(vix::SettleTabVoucher::DISCRIMINATOR)
        || is(vix::SettleLockedVoucher::DISCRIMINATOR)
        || is(vix::RepayCredit::DISCRIMINATOR)
        || is(vix::SeizeCollateral::DISCRIMINATOR)
        || is(vix::SeizeAncestor::DISCRIMINATOR)
        || is(vix::SettleLiquidation::DISCRIMINATOR)
        || is(vix::GrantAgentSession::DISCRIMINATOR)
        || is(vix::ManageGovernedTradeGrant::DISCRIMINATOR)
        || is(vix::ProvisionGovernedAssetRoleV2::DISCRIMINATOR)
        || is(vix::SwapForCarry::DISCRIMINATOR)
        || is(vix::DrawCredit::DISCRIMINATOR)
        || is(vix::DrawLeverage::DISCRIMINATOR)
        || is(vix::ProvisionWalletRole::DISCRIMINATOR)
    {
        Some(PLUS_ONE)
    } else if is(vix::ActivateWallet::DISCRIMINATOR) {
        Some(BIRTH)
    } else {
        None
    }
}

/// Where a marker's content-checked Swig legs sit relative to the marker
/// instruction itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BoundLegs {
    /// Legs FOLLOW the marker, at `marker + off` (every core marker: `+1`).
    After(&'static [usize]),
    /// Legs PRECEDE the marker, at `marker - off` (`activate_wallet`).
    Before(&'static [usize]),
}

impl BoundLegs {
    /// Does a marker at index `marker` credit the Swig instruction at `leg`?
    fn credits(self, marker: usize, leg: usize) -> bool {
        match self {
            BoundLegs::After(offsets) => offsets
                .iter()
                .any(|off| marker.checked_add(*off) == Some(leg)),
            BoundLegs::Before(offsets) => offsets
                .iter()
                .any(|off| marker.checked_sub(*off) == Some(leg)),
        }
    }
}

/// Pure marker-union scan over the raw instructions-sysvar blob (unit-tested
/// against the canonical `construct_instructions_data` serializer). See THE RULE
/// above. Allocation-free: O(n²) over a single borrow, n = instruction count
/// (≤ dozens).
fn scan_marker_bound(blob: &[u8], swig: &Pubkey) -> Result<()> {
    let num = blob_u16(blob, 0).ok_or_else(|| error!(VaultError::UnboundSwigInstruction))?;
    for j in 0..num {
        let leg = parse_sysvar_ix(blob, j).ok_or_else(|| error!(VaultError::UnboundSwigInstruction))?;
        if leg.program_id == crate::ID {
            continue;
        }
        if leg.program_id != SWIG_PROGRAM_ID || leg.account0 != Some(*swig) {
            // CLASS RULE: outside this program, only a Swig instruction with the
            // swig CONFIG at account 0 may touch it (and that one must be marker-
            // bound, below). Any other top-level instruction listing the config —
            // a foreign program that could CPI into Swig role management, or a
            // Swig instruction on another wallet naming this config — is refused,
            // whatever Swig's allowlist says. Keyed on the config, not the swig
            // wallet PDA: ATA creates legitimately name the wallet as owner.
            let lists = sysvar_ix_lists_key(blob, &leg, swig)
                .ok_or_else(|| error!(VaultError::UnboundSwigInstruction))?;
            if lists {
                return Err(error!(VaultError::UnboundSwigInstruction));
            }
            continue;
        }
        let mut bound = false;
        for i in 0..num {
            let m = parse_sysvar_ix(blob, i).ok_or_else(|| error!(VaultError::UnboundSwigInstruction))?;
            if m.program_id != crate::ID || m.account0 != Some(*swig) {
                continue;
            }
            let Some(disc) = m.disc else { continue };
            if let Some(legs) = vault_marker_bound_offsets(&disc) {
                if legs.credits(i, j) {
                    bound = true;
                    break;
                }
            }
        }
        if !bound {
            return Err(error!(VaultError::UnboundSwigInstruction));
        }
    }
    Ok(())
}

/// P5 guard (marker-union, allocation-free). Borrow the instructions sysvar once
/// and reject (`UnboundSwigInstruction`) any Swig-program instruction acting on
/// `swig` that no vault marker in the transaction binds on `swig`. Call it from
/// every marker handler, passing the swig whose legs it must guard (the vault
/// swig, or the financier/ancestor swig for draw_credit/draw_leverage/
/// seize_ancestor). The union is over ALL markers in the tx, so sibling markers'
/// legs (e.g. provision's UpdateAuthorityV1 and swap_for_carry's SignV2 in the
/// one-step stock buy) are admitted while a back-reference second SignV2 is
/// refused, independent of live Swig semantics. The same pass also enforces the
/// CLASS RULE: any top-level instruction that lists `swig` (the swig CONFIG) in
/// any slot is refused unless it is this program's or a Swig instruction with
/// `swig` at account 0.
pub fn require_swig_ixs_marker_bound(
    instructions_sysvar: &AccountInfo,
    swig: &Pubkey,
) -> Result<()> {
    // The raw parse below trusts these bytes, so the account MUST be the real
    // instructions sysvar — the check `load_instruction_at_checked` used to make
    // for us. Every current caller also address-constrains it; this keeps the
    // guard self-sufficient for any new caller (e.g. the activation lane's).
    require_keys_eq!(
        *instructions_sysvar.key,
        anchor_lang::solana_program::sysvar::instructions::ID,
        VaultError::UnboundSwigInstruction
    );
    let blob = instructions_sysvar
        .try_borrow_data()
        .map_err(|_| error!(VaultError::UnboundSwigInstruction))?;
    scan_marker_bound(&blob, swig)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anchor_lang::solana_program::instruction::{AccountMeta, Instruction};

    fn transfer_data(amount: u64) -> Vec<u8> {
        let mut d = vec![SPL_TRANSFER_CHECKED_TAG];
        d.extend_from_slice(&amount.to_le_bytes());
        d.push(6); // decimals
        d
    }

    /// Build a SignV2 envelope wrapping one TransferChecked per `legs` entry.
    /// Account list: [0] = token program, then per leg [source, mint, dest, auth]
    /// — mirroring how swig's compact_instructions appends inner accounts onto
    /// the envelope's own meta list (module header, layout section).
    fn envelope(legs: &[(u64, Pubkey, Pubkey)]) -> Instruction {
        let mut accounts = vec![AccountMeta::new_readonly(SPL_TOKEN_PROGRAM_ID, false)];
        let mut payload = vec![legs.len() as u8];
        for (amount, mint, dest) in legs {
            let base = accounts.len() as u8;
            accounts.push(AccountMeta::new(Pubkey::new_unique(), false)); // source
            accounts.push(AccountMeta::new_readonly(*mint, false)); // mint
            accounts.push(AccountMeta::new(*dest, false)); // dest
            accounts.push(AccountMeta::new_readonly(Pubkey::new_unique(), false)); // authority
            payload.push(0); // pid_idx → token program at meta[0]
            payload.push(4); // num_accounts
            payload.extend_from_slice(&[base, base + 1, base + 2, base + 3]);
            let d = transfer_data(*amount);
            payload.extend_from_slice(&(d.len() as u16).to_le_bytes());
            payload.extend_from_slice(&d);
        }
        let mut data = Vec::new();
        data.extend_from_slice(&SWIG_SIGN_V2_DISCRIMINATOR.to_le_bytes());
        data.extend_from_slice(&(payload.len() as u16).to_le_bytes());
        data.extend_from_slice(&7u32.to_le_bytes()); // role_id (opaque to the decoder)
        data.extend_from_slice(&payload);
        data.extend_from_slice(&[0xAA; 16]); // authority_payload (opaque tail)
        Instruction {
            program_id: SWIG_PROGRAM_ID,
            accounts,
            data,
        }
    }

    fn governed_send_envelope(token_program: Pubkey) -> Instruction {
        let source = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let destination = Pubkey::new_unique();
        let wallet = Pubkey::new_unique();
        let accounts = vec![
            AccountMeta::new_readonly(token_program, false),
            AccountMeta::new(source, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(destination, false),
            AccountMeta::new_readonly(wallet, false),
            AccountMeta::new_readonly(anchor_lang::solana_program::sysvar::instructions::ID, false),
        ];
        let mut payload = vec![1, 0, 4, 1, 2, 3, 4];
        let transfer = transfer_data(55);
        payload.extend_from_slice(&(transfer.len() as u16).to_le_bytes());
        payload.extend_from_slice(&transfer);
        let mut data = Vec::new();
        data.extend_from_slice(&SWIG_SIGN_V2_DISCRIMINATOR.to_le_bytes());
        data.extend_from_slice(&(payload.len() as u16).to_le_bytes());
        data.extend_from_slice(&9u32.to_le_bytes());
        data.extend_from_slice(&payload);
        data.push(5);
        Instruction {
            program_id: SWIG_PROGRAM_ID,
            accounts,
            data,
        }
    }

    #[test]
    fn governed_send_decodes_one_exact_classic_or_token_2022_transfer() {
        for token_program in [anchor_spl::token::ID, anchor_spl::token_2022::ID] {
            let ix = governed_send_envelope(token_program);
            let decoded = decode_governed_send_envelope(&ix, &token_program).unwrap();
            assert_eq!(decoded.role_id, 9);
            assert_eq!(decoded.source, ix.accounts[1].pubkey);
            assert_eq!(decoded.mint, ix.accounts[2].pubkey);
            assert_eq!(decoded.destination, ix.accounts[3].pubkey);
            assert_eq!(decoded.transfer_authority, ix.accounts[4].pubkey);
            assert_eq!(
                decoded.authority_account,
                anchor_lang::solana_program::sysvar::instructions::ID
            );
            assert_eq!((decoded.amount, decoded.decimals), (55, 6));
            assert_eq!(
                decoded.transfer_instruction_digest,
                decode_governed_send_envelope(&ix, &token_program)
                    .unwrap()
                    .transfer_instruction_digest
            );
        }
    }

    #[test]
    fn governed_send_rejects_smuggling_substitution_and_unsafe_privileges() {
        let token_program = anchor_spl::token::ID;
        let original = governed_send_envelope(token_program);
        let mut mutations = Vec::new();
        let mut changed = original.clone();
        changed.data[SWIG_SIGN_V2_ARGS_LEN] = 2;
        mutations.push(changed);
        let mut changed = original.clone();
        let payload_len = u16::from_le_bytes([changed.data[2], changed.data[3]]) as usize;
        changed
            .data
            .insert(SWIG_SIGN_V2_ARGS_LEN + payload_len, 0xff);
        changed.data[2..4].copy_from_slice(&((payload_len + 1) as u16).to_le_bytes());
        mutations.push(changed);
        let mut changed = original.clone();
        changed.data.push(0);
        mutations.push(changed);
        let mut changed = original.clone();
        // payload [count,pid,nacc,4 indexes,dlen] then instruction tag
        changed.data[SWIG_SIGN_V2_ARGS_LEN + 9] = 4;
        mutations.push(changed);
        let mut changed = original.clone();
        changed.accounts[0].pubkey = anchor_spl::token_2022::ID;
        mutations.push(changed);
        let mut changed = original.clone();
        changed.accounts[1].is_writable = false;
        mutations.push(changed);
        let mut changed = original.clone();
        changed.accounts[2].is_writable = true;
        mutations.push(changed);
        let mut changed = original.clone();
        changed.accounts[3].is_writable = false;
        mutations.push(changed);
        let mut changed = original.clone();
        *changed.data.last_mut().unwrap() = u8::MAX;
        mutations.push(changed);
        for changed in mutations {
            assert!(decode_governed_send_envelope(&changed, &token_program).is_err());
        }
    }

    #[test]
    fn governed_send_digest_binds_each_transfer_fact() {
        let token_program = anchor_spl::token::ID;
        let original = governed_send_envelope(token_program);
        let expected = decode_governed_send_envelope(&original, &token_program)
            .unwrap()
            .transfer_instruction_digest;
        for changed in [
            {
                let mut ix = original.clone();
                ix.accounts[1].pubkey = Pubkey::new_unique();
                ix
            },
            {
                let mut ix = original.clone();
                ix.accounts[2].pubkey = Pubkey::new_unique();
                ix
            },
            {
                let mut ix = original.clone();
                ix.accounts[3].pubkey = Pubkey::new_unique();
                ix
            },
            {
                let mut ix = original.clone();
                ix.accounts[4].pubkey = Pubkey::new_unique();
                ix
            },
            {
                let mut ix = original.clone();
                let amount_byte = SWIG_SIGN_V2_ARGS_LEN + 10;
                ix.data[amount_byte] ^= 1;
                ix
            },
            {
                let mut ix = original.clone();
                let decimals_byte = SWIG_SIGN_V2_ARGS_LEN + 18;
                ix.data[decimals_byte] ^= 1;
                ix
            },
        ] {
            assert_ne!(
                decode_governed_send_envelope(&changed, &token_program)
                    .unwrap()
                    .transfer_instruction_digest,
                expected
            );
        }
    }

    #[test]
    fn one_leg_decodes() {
        let mint = Pubkey::new_unique();
        let dest = Pubkey::new_unique();
        let ix = envelope(&[(42, mint, dest)]);
        let source = ix.accounts[1].pubkey;
        let legs = decode_envelope(&ix, 1).unwrap();
        assert_eq!(legs.len(), 1);
        assert_eq!(legs[0].amount, 42);
        assert_eq!(legs[0].source, source);
        assert_eq!(legs[0].destination, dest);
        assert_eq!(legs[0].mint, mint);
    }

    #[test]
    fn encoded_money_leg_must_debit_the_canonical_cash_account() {
        let mint = Pubkey::new_unique();
        let destination = Pubkey::new_unique();
        let mut ix = envelope(&[(42, mint, destination)]);
        let canonical_source = ix.accounts[1].pubkey;
        let leg = decode_envelope(&ix, 1).unwrap().remove(0);
        assert!(
            crate::verify::reserved_cash::require_canonical_money_leg_source(
                &leg,
                &canonical_source,
                &mint,
            )
            .is_ok()
        );

        // The same valid TransferChecked shape from a second token account is
        // rejected by every handler that binds through the shared verifier.
        ix.accounts[1].pubkey = Pubkey::new_unique();
        let secondary_source_leg = decode_envelope(&ix, 1).unwrap().remove(0);
        assert!(
            crate::verify::reserved_cash::require_canonical_money_leg_source(
                &secondary_source_leg,
                &canonical_source,
                &mint,
            )
            .is_err()
        );
    }

    #[test]
    fn two_legs_decode_in_order() {
        let mint = Pubkey::new_unique();
        let (d1, d2) = (Pubkey::new_unique(), Pubkey::new_unique());
        let legs = decode_envelope(&envelope(&[(100, mint, d1), (7, mint, d2)]), 2).unwrap();
        assert_eq!((legs[0].amount, legs[0].destination), (100, d1));
        assert_eq!((legs[1].amount, legs[1].destination), (7, d2));
    }

    #[test]
    fn count_mismatch_rejects_both_ways() {
        let m = Pubkey::new_unique();
        let one = envelope(&[(1, m, Pubkey::new_unique())]);
        let two = envelope(&[(1, m, Pubkey::new_unique()), (2, m, Pubkey::new_unique())]);
        let three = envelope(&[
            (1, m, Pubkey::new_unique()),
            (2, m, Pubkey::new_unique()),
            (3, m, Pubkey::new_unique()),
        ]);
        assert!(decode_envelope(&one, 2).is_err()); // 1 leg when 2 required
        assert!(decode_envelope(&two, 1).is_err()); // siphon: 2 legs when 1 required
        assert!(decode_envelope(&three, 2).is_err()); // 3 legs when 2 required
    }

    #[test]
    fn trailing_payload_garbage_rejects() {
        let m = Pubkey::new_unique();
        let mut ix = envelope(&[(5, m, Pubkey::new_unique())]);
        // Splice one junk byte at the end of the payload region and bump IPL so
        // the header still frames it: the count-vs-content check must fire.
        let ipl = u16::from_le_bytes([ix.data[2], ix.data[3]]) as usize;
        ix.data.insert(SWIG_SIGN_V2_ARGS_LEN + ipl, 0xFF);
        ix.data[2..4].copy_from_slice(&((ipl + 1) as u16).to_le_bytes());
        assert!(decode_envelope(&ix, 1).is_err());
    }

    #[test]
    fn peek_count_reads_declared_count() {
        let m = Pubkey::new_unique();
        let one = envelope(&[(1, m, Pubkey::new_unique())]);
        let two = envelope(&[(1, m, Pubkey::new_unique()), (2, m, Pubkey::new_unique())]);
        let three = envelope(&[
            (1, m, Pubkey::new_unique()),
            (2, m, Pubkey::new_unique()),
            (3, m, Pubkey::new_unique()),
        ]);
        assert_eq!(peek_envelope_count(&one).unwrap(), 1);
        assert_eq!(peek_envelope_count(&two).unwrap(), 2);
        // 3 peeks fine — the one_or_two caller rejects it; decode would too.
        assert_eq!(peek_envelope_count(&three).unwrap(), 3);
    }

    #[test]
    fn peek_count_rejects_bad_framing() {
        let m = Pubkey::new_unique();
        let mut wrong_prog = envelope(&[(1, m, Pubkey::new_unique())]);
        wrong_prog.program_id = Pubkey::new_unique();
        assert!(peek_envelope_count(&wrong_prog).is_err());

        let mut wrong_disc = envelope(&[(1, m, Pubkey::new_unique())]);
        wrong_disc.data[0] = 99;
        assert!(peek_envelope_count(&wrong_disc).is_err());

        let mut truncated = envelope(&[(1, m, Pubkey::new_unique())]);
        truncated.data.truncate(4);
        assert!(peek_envelope_count(&truncated).is_err());

        // IPL overrunning the data must reject, not read out of bounds.
        let mut overrun = envelope(&[(1, m, Pubkey::new_unique())]);
        overrun.data[2..4].copy_from_slice(&u16::MAX.to_le_bytes());
        assert!(peek_envelope_count(&overrun).is_err());
    }

    #[test]
    fn wrong_inner_program_rejects() {
        let m = Pubkey::new_unique();
        let mut ix = envelope(&[(5, m, Pubkey::new_unique())]);
        ix.accounts[0] = AccountMeta::new_readonly(Pubkey::new_unique(), false); // not SPL token
        assert!(decode_envelope(&ix, 1).is_err());
    }

    /// envelope-lite: build a SignV2 wrapping N inner instructions for an
    /// arbitrary program (not TransferChecked — mirrors a Jupiter route).
    fn swap_envelope(inner_programs: &[Pubkey]) -> Instruction {
        let mut accounts: Vec<AccountMeta> = Vec::new();
        let mut payload = vec![inner_programs.len() as u8];
        for prog in inner_programs {
            let pid_idx = accounts.len() as u8;
            accounts.push(AccountMeta::new_readonly(*prog, false));
            // two opaque route accounts
            let base = accounts.len() as u8;
            accounts.push(AccountMeta::new(Pubkey::new_unique(), false));
            accounts.push(AccountMeta::new(Pubkey::new_unique(), false));
            payload.push(pid_idx);
            payload.push(2); // num_accounts
            payload.extend_from_slice(&[base, base + 1]);
            let route_data = [0xB0u8; 24]; // opaque route bytes — never parsed
            payload.extend_from_slice(&(route_data.len() as u16).to_le_bytes());
            payload.extend_from_slice(&route_data);
        }
        let mut data = Vec::new();
        data.extend_from_slice(&SWIG_SIGN_V2_DISCRIMINATOR.to_le_bytes());
        data.extend_from_slice(&(payload.len() as u16).to_le_bytes());
        data.extend_from_slice(&9u32.to_le_bytes()); // role_id (opaque)
        data.extend_from_slice(&payload);
        data.extend_from_slice(&[0xAA; 16]); // authority_payload
        Instruction {
            program_id: SWIG_PROGRAM_ID,
            accounts,
            data,
        }
    }

    /// Drive the envelope-lite core the same way require_swap_envelope_at
    /// does after the sysvar load (the sysvar plumbing is fork-suite ground).
    fn check_swap_envelope(ix: &Instruction, pinned: &Pubkey) -> Result<()> {
        // mirror of require_swap_envelope_at's body post-load
        require!(
            ix.program_id == SWIG_PROGRAM_ID,
            VaultError::SwapEnvelopeInvalid
        );
        require!(
            ix.accounts.len() <= MAX_SAFE_SWIG_SIGN_V2_ACCOUNTS,
            VaultError::SwapEnvelopeInvalid
        );
        let data = ix.data.as_slice();
        require!(
            data.len() >= SWIG_SIGN_V2_ARGS_LEN,
            VaultError::SwapEnvelopeInvalid
        );
        let disc = u16::from_le_bytes([data[0], data[1]]);
        require!(
            disc == SWIG_SIGN_V2_DISCRIMINATOR,
            VaultError::SwapEnvelopeInvalid
        );
        let ipl = u16::from_le_bytes([data[2], data[3]]) as usize;
        let payload_end = SWIG_SIGN_V2_ARGS_LEN + ipl;
        require!(payload_end <= data.len(), VaultError::SwapEnvelopeInvalid);
        let payload = &data[SWIG_SIGN_V2_ARGS_LEN..payload_end];
        let n = *payload
            .first()
            .ok_or_else(|| error!(VaultError::SwapEnvelopeInvalid))?;
        require!(n == 1, VaultError::SwapEnvelopeInvalid);
        let pid_idx = *payload
            .get(1)
            .ok_or_else(|| error!(VaultError::SwapEnvelopeInvalid))? as usize;
        let program_id = ix
            .accounts
            .get(pid_idx)
            .ok_or_else(|| error!(VaultError::SwapEnvelopeInvalid))?
            .pubkey;
        require!(program_id == *pinned, VaultError::SwapEnvelopeInvalid);
        Ok(())
    }

    #[test]
    fn swap_envelope_single_pinned_inner_passes() {
        let jup = crate::constants::JUPITER_V6_PROGRAM_ID;
        assert!(check_swap_envelope(&swap_envelope(&[jup]), &jup).is_ok());
    }

    #[test]
    fn swap_envelope_second_inner_rejects_the_siphon() {
        let jup = crate::constants::JUPITER_V6_PROGRAM_ID;
        // classic siphon: route + a trailing TransferChecked-shaped second ix
        assert!(check_swap_envelope(&swap_envelope(&[jup, SPL_TOKEN_PROGRAM_ID]), &jup).is_err());
        assert!(check_swap_envelope(&swap_envelope(&[jup, jup]), &jup).is_err());
    }

    #[test]
    fn swap_envelope_unpinned_program_rejects() {
        let jup = crate::constants::JUPITER_V6_PROGRAM_ID;
        assert!(check_swap_envelope(&swap_envelope(&[Pubkey::new_unique()]), &jup).is_err());
        // a bare TransferChecked in the envelope is NOT a swap
        assert!(check_swap_envelope(&swap_envelope(&[SPL_TOKEN_PROGRAM_ID]), &jup).is_err());
    }

    #[test]
    fn swap_envelope_rejects_unsafe_swig_account_count() {
        let jup = crate::constants::JUPITER_V6_PROGRAM_ID;
        let mut ix = swap_envelope(&[jup]);
        ix.accounts.resize(
            MAX_SAFE_SWIG_SIGN_V2_ACCOUNTS + 1,
            AccountMeta::new_readonly(Pubkey::new_unique(), false),
        );
        assert!(check_swap_envelope(&ix, &jup).is_err());
    }

    fn governed_envelope() -> Instruction {
        let jup = crate::constants::JUPITER_V6_PROGRAM_ID;
        let accounts = vec![
            AccountMeta::new_readonly(jup, false),
            AccountMeta::new(Pubkey::new_unique(), false),
            AccountMeta::new_readonly(Pubkey::new_unique(), true),
            AccountMeta::new_readonly(Pubkey::new_unique(), true),
        ];
        let route_data = [9u8, 8, 7, 6, 5];
        let mut payload = vec![1, 0, 2, 1, 2];
        payload.extend_from_slice(&(route_data.len() as u16).to_le_bytes());
        payload.extend_from_slice(&route_data);
        let mut data = Vec::new();
        data.extend_from_slice(&SWIG_SIGN_V2_DISCRIMINATOR.to_le_bytes());
        data.extend_from_slice(&(payload.len() as u16).to_le_bytes());
        data.extend_from_slice(&37u32.to_le_bytes());
        data.extend_from_slice(&payload);
        data.push(3);
        Instruction {
            program_id: SWIG_PROGRAM_ID,
            accounts,
            data,
        }
    }

    #[test]
    fn governed_envelope_binds_finalized_route_and_dynamic_role() {
        let jup = crate::constants::JUPITER_V6_PROGRAM_ID;
        let ix = governed_envelope();
        let decoded = decode_governed_swap_envelope(&ix, &jup).unwrap();
        assert_eq!(decoded.role_id, 37);
        assert_eq!(decoded.authority_account, ix.accounts[3].pubkey);
        assert_eq!(decoded, decode_governed_swap_envelope(&ix, &jup).unwrap());
    }

    #[test]
    fn governed_envelope_digest_changes_on_execution_substitution() {
        let jup = crate::constants::JUPITER_V6_PROGRAM_ID;
        let original = governed_envelope();
        let expected = decode_governed_swap_envelope(&original, &jup)
            .unwrap()
            .route_digest;
        for changed in [
            {
                let mut ix = original.clone();
                let end =
                    SWIG_SIGN_V2_ARGS_LEN + u16::from_le_bytes([ix.data[2], ix.data[3]]) as usize;
                ix.data[end - 1] ^= 1;
                ix
            },
            {
                let mut ix = original.clone();
                ix.accounts[1].pubkey = Pubkey::new_unique();
                ix
            },
            {
                let mut ix = original.clone();
                ix.accounts[2].is_writable = true;
                ix
            },
            {
                let mut ix = original.clone();
                ix.accounts[1].is_signer = true;
                ix
            },
            {
                let mut ix = original.clone();
                ix.data
                    .swap(SWIG_SIGN_V2_ARGS_LEN + 3, SWIG_SIGN_V2_ARGS_LEN + 4);
                ix
            },
        ] {
            assert_ne!(
                decode_governed_swap_envelope(&changed, &jup)
                    .unwrap()
                    .route_digest,
                expected
            );
        }
    }

    #[test]
    fn governed_envelope_rejects_smuggling_and_malformed_authority() {
        let jup = crate::constants::JUPITER_V6_PROGRAM_ID;
        let mut extra_inner = governed_envelope();
        extra_inner.data[SWIG_SIGN_V2_ARGS_LEN] = 2;
        assert!(decode_governed_swap_envelope(&extra_inner, &jup).is_err());
        let mut trailing_authority = governed_envelope();
        trailing_authority.data.push(0);
        assert!(decode_governed_swap_envelope(&trailing_authority, &jup).is_err());
        let mut trailing_payload = governed_envelope();
        let old_len =
            u16::from_le_bytes([trailing_payload.data[2], trailing_payload.data[3]]) as usize;
        trailing_payload
            .data
            .insert(SWIG_SIGN_V2_ARGS_LEN + old_len, 0);
        trailing_payload.data[2..4].copy_from_slice(&((old_len + 1) as u16).to_le_bytes());
        assert!(decode_governed_swap_envelope(&trailing_payload, &jup).is_err());
        let mut invalid_authority = governed_envelope();
        *invalid_authority.data.last_mut().unwrap() = u8::MAX;
        assert!(decode_governed_swap_envelope(&invalid_authority, &jup).is_err());
        let mut wrong_program = governed_envelope();
        wrong_program.accounts[0].pubkey = Pubkey::new_unique();
        assert!(decode_governed_swap_envelope(&wrong_program, &jup).is_err());
        let mut oversized = governed_envelope();
        oversized.accounts.resize(
            MAX_SAFE_SWIG_SIGN_V2_ACCOUNTS + 1,
            AccountMeta::new_readonly(Pubkey::new_unique(), false),
        );
        assert!(decode_governed_swap_envelope(&oversized, &jup).is_err());
    }

    #[test]
    fn non_transfer_inner_op_rejects() {
        let m = Pubkey::new_unique();
        let mut ix = envelope(&[(5, m, Pubkey::new_unique())]);
        // Flip the inner tag from TransferChecked(12) to Approve(4): same
        // shape, different op — must reject (the approve-siphon case).
        let ipl = u16::from_le_bytes([ix.data[2], ix.data[3]]) as usize;
        let payload_start = SWIG_SIGN_V2_ARGS_LEN;
        // payload: [num=1][pid][num_acc][4 idx][dlen u16][tag ...]
        let tag_pos = payload_start + 1 + 1 + 1 + 4 + 2;
        assert!(tag_pos < payload_start + ipl);
        assert_eq!(ix.data[tag_pos], SPL_TRANSFER_CHECKED_TAG);
        ix.data[tag_pos] = 4;
        assert!(decode_envelope(&ix, 1).is_err());
    }

    // ── P5: marker-union scan over the instructions-sysvar blob ─────────────
    //
    // Tests build the blob with the SAME serializer the runtime uses
    // (`construct_instructions_data`), so the allocation-free parser is proven
    // against the canonical format, not a hand-rolled layout.

    use anchor_lang::solana_program::sysvar::instructions::{
        construct_instructions_data, BorrowedAccountMeta, BorrowedInstruction,
    };

    fn build_blob(ixs: &[Instruction]) -> Vec<u8> {
        let borrowed: Vec<BorrowedInstruction> = ixs
            .iter()
            .map(|ix| BorrowedInstruction {
                program_id: &ix.program_id,
                accounts: ix
                    .accounts
                    .iter()
                    .map(|m| BorrowedAccountMeta {
                        pubkey: &m.pubkey,
                        is_signer: m.is_signer,
                        is_writable: m.is_writable,
                    })
                    .collect(),
                data: ix.data.as_slice(),
            })
            .collect();
        construct_instructions_data(&borrowed)
    }

    /// A Swig-program instruction whose config account (meta 0) is `swig`.
    fn swig_ix_on(swig: Pubkey) -> Instruction {
        Instruction {
            program_id: SWIG_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(swig, false),
                AccountMeta::new_readonly(Pubkey::new_unique(), false),
            ],
            data: vec![11u8, 0], // SignV2-ish; the scan ignores Swig instruction data
        }
    }

    /// A vault marker instruction carrying `disc`, with `acct0` at meta 0.
    fn marker_ix(disc: &[u8], acct0: Pubkey) -> Instruction {
        Instruction {
            program_id: crate::ID,
            accounts: vec![
                AccountMeta::new(acct0, false),
                AccountMeta::new_readonly(Pubkey::new_unique(), false),
            ],
            data: disc.to_vec(),
        }
    }

    /// A non-vault, non-swig filler (ComputeBudget / precompile stand-in).
    fn filler() -> Instruction {
        Instruction {
            program_id: Pubkey::new_unique(),
            accounts: vec![],
            data: vec![0u8; 4],
        }
    }

    #[test]
    fn union_marker_offsets_table_matches_the_wired_markers() {
        use crate::instruction as vix;
        for disc in [
            vix::FinalizeWithdrawal::DISCRIMINATOR,
            vix::SettleTabVoucher::DISCRIMINATOR,
            vix::SettleLockedVoucher::DISCRIMINATOR,
            vix::RepayCredit::DISCRIMINATOR,
            vix::SeizeCollateral::DISCRIMINATOR,
            vix::SeizeAncestor::DISCRIMINATOR,
            vix::SettleLiquidation::DISCRIMINATOR,
            vix::GrantAgentSession::DISCRIMINATOR,
            vix::ManageGovernedTradeGrant::DISCRIMINATOR,
            vix::ProvisionGovernedAssetRoleV2::DISCRIMINATOR,
            vix::SwapForCarry::DISCRIMINATOR,
            vix::DrawCredit::DISCRIMINATOR,
            vix::DrawLeverage::DISCRIMINATOR,
            vix::ProvisionWalletRole::DISCRIMINATOR,
        ] {
            assert_eq!(vault_marker_bound_offsets(disc), Some(BoundLegs::After(&[1])));
        }
        // activate_wallet verifies the birth that PRECEDES it: [c-6]..[c-1].
        assert_eq!(
            vault_marker_bound_offsets(vix::ActivateWallet::DISCRIMINATOR),
            Some(BoundLegs::Before(&[1, 2, 3, 4, 5, 6]))
        );
        // The retired CPI birth and the fee knob bind no Swig leg.
        for disc in [
            vix::SetSwigAtomic::DISCRIMINATOR,
            vix::SetActivationFee::DISCRIMINATOR,
        ] {
            assert_eq!(vault_marker_bound_offsets(disc), None);
        }
        // NOT markers (bind no Swig leg) — must be None, or their +1 would wrongly
        // admit an appended SignV2.
        for disc in [
            vix::FinishSwap::DISCRIMINATOR,
            vix::ManageGovernedTradeGrantV2::DISCRIMINATOR,
            vix::AuthorizeGovernedSwapV2::DISCRIMINATOR,
            vix::SettleVoucher::DISCRIMINATOR,
            vix::LockVoucher::DISCRIMINATOR,
            vix::RegisterSessionKey::DISCRIMINATOR,
        ] {
            assert_eq!(vault_marker_bound_offsets(disc), None);
        }
    }

    #[test]
    fn parser_round_trips_the_canonical_serializer() {
        // Prove parse_sysvar_ix reads program_id / account0 / disc exactly as
        // construct_instructions_data writes them.
        let swig = Pubkey::new_unique();
        let ixs = vec![
            filler(),
            marker_ix(crate::instruction::SwapForCarry::DISCRIMINATOR, swig),
            swig_ix_on(swig),
        ];
        let blob = build_blob(&ixs);
        assert_eq!(blob_u16(&blob, 0), Some(3));
        let m = parse_sysvar_ix(&blob, 1).unwrap();
        assert_eq!(m.program_id, crate::ID);
        assert_eq!(m.account0, Some(swig));
        assert_eq!(
            &m.disc.unwrap()[..],
            crate::instruction::SwapForCarry::DISCRIMINATOR
        );
        let s = parse_sysvar_ix(&blob, 2).unwrap();
        assert_eq!(s.program_id, SWIG_PROGRAM_ID);
        assert_eq!(s.account0, Some(swig));
    }

    #[test]
    fn union_admits_the_one_step_stock_buy_and_rejects_an_appended_signv2() {
        use crate::instruction as vix;
        let swig = Pubkey::new_unique();
        // 5wGmSQpnyD… shape: [CU, provision(ArmTrade), UpdateAuthorityV1, Ed25519,
        // authorize_v2, swap_for_carry, SignV2, finish_swap].
        let mut ixs = vec![
            filler(),
            marker_ix(vix::ProvisionGovernedAssetRoleV2::DISCRIMINATOR, swig), // binds +1 = 2
            swig_ix_on(swig),                                                  // [2]
            filler(),
            marker_ix(vix::AuthorizeGovernedSwapV2::DISCRIMINATOR, swig), // NOT a marker
            marker_ix(vix::SwapForCarry::DISCRIMINATOR, swig),           // binds +1 = 6
            swig_ix_on(swig),                                            // [6]
            marker_ix(vix::FinishSwap::DISCRIMINATOR, swig),            // NOT a marker
        ];
        // The former false reject — must now pass.
        assert!(scan_marker_bound(&build_blob(&ixs), &swig).is_ok());
        // Append a second SignV2 after the finish leg → bound by no marker → rejected.
        ixs.push(swig_ix_on(swig)); // [8]
        let err = scan_marker_bound(&build_blob(&ixs), &swig).unwrap_err();
        assert!(format!("{err:?}").contains("UnboundSwigInstruction"));
    }

    #[test]
    fn union_finalize_shape_passes_and_the_jp5_second_signv2_is_rejected() {
        use crate::instruction as vix;
        let swig = Pubkey::new_unique();
        let ok = vec![
            marker_ix(vix::FinalizeWithdrawal::DISCRIMINATOR, swig),
            swig_ix_on(swig),
        ];
        assert!(scan_marker_bound(&build_blob(&ok), &swig).is_ok());
        // J-P5: append SignV2 @2 back-referencing finalize → rejected.
        let attacked = vec![
            marker_ix(vix::FinalizeWithdrawal::DISCRIMINATOR, swig),
            swig_ix_on(swig),
            swig_ix_on(swig),
        ];
        assert!(scan_marker_bound(&build_blob(&attacked), &swig).is_err());
    }

    #[test]
    fn union_marker_credits_only_when_its_account0_is_the_scanned_swig() {
        use crate::instruction as vix;
        let vault_swig = Pubkey::new_unique();
        let financier = Pubkey::new_unique();
        // draw_credit's account 0 is the FINANCIER, so it must not credit a
        // vault-swig leg: [draw_credit(fin), SignV2(vault)] → leg @1 unbound.
        let ixs = vec![
            marker_ix(vix::DrawCredit::DISCRIMINATOR, financier),
            swig_ix_on(vault_swig),
        ];
        assert!(scan_marker_bound(&build_blob(&ixs), &vault_swig).is_err());
        // It DOES credit its own financier leg.
        let ixs2 = vec![
            marker_ix(vix::DrawCredit::DISCRIMINATOR, financier),
            swig_ix_on(financier),
        ];
        assert!(scan_marker_bound(&build_blob(&ixs2), &financier).is_ok());
    }

    #[test]
    fn union_settle_tab_plus_draw_isolates_vault_and_financier_swigs() {
        use crate::instruction as vix;
        let vault_swig = Pubkey::new_unique();
        let financier = Pubkey::new_unique();
        // [settle_tab(vault), cash SignV2(vault), draw_credit(fin), draw SignV2(fin)]
        let ixs = vec![
            marker_ix(vix::SettleTabVoucher::DISCRIMINATOR, vault_swig), // binds vault +1 = 1
            swig_ix_on(vault_swig),                                      // [1]
            marker_ix(vix::DrawCredit::DISCRIMINATOR, financier),        // binds fin +1 = 3
            swig_ix_on(financier),                                       // [3]
        ];
        assert!(scan_marker_bound(&build_blob(&ixs), &vault_swig).is_ok());
        assert!(scan_marker_bound(&build_blob(&ixs), &financier).is_ok());
    }

    #[test]
    fn union_rejects_nonmarker_plus_one_and_lone_swig_ix() {
        use crate::instruction as vix;
        let swig = Pubkey::new_unique();
        // finish_swap is NOT a marker: a SignV2 right after it is unbound.
        let after_finish = vec![
            marker_ix(vix::FinishSwap::DISCRIMINATOR, swig),
            swig_ix_on(swig),
        ];
        assert!(scan_marker_bound(&build_blob(&after_finish), &swig).is_err());
        // A lone swig ix with no vault marker at all is rejected.
        let lone = vec![filler(), swig_ix_on(swig)];
        assert!(scan_marker_bound(&build_blob(&lone), &swig).is_err());
    }

    #[test]
    fn guard_rejects_bytes_from_an_account_that_is_not_the_instructions_sysvar() {
        // The raw parse trusts the bytes, so the guard must refuse any account
        // other than the real instructions sysvar — otherwise a caller could hand
        // it forged bytes describing a clean transaction.
        use crate::instruction as vix;
        let swig = Pubkey::new_unique();
        let honest = build_blob(&[
            marker_ix(vix::FinalizeWithdrawal::DISCRIMINATOR, swig),
            swig_ix_on(swig),
        ]);
        let owner = anchor_lang::solana_program::sysvar::ID;
        let fake_key = Pubkey::new_unique();
        let mut fake_lamports = 0u64;
        let mut fake_data = honest.clone();
        let fake = AccountInfo::new(
            &fake_key,
            false,
            false,
            &mut fake_lamports,
            &mut fake_data,
            &owner,
            false,
            0,
        );
        assert!(require_swig_ixs_marker_bound(&fake, &swig).is_err());
        // The same bytes at the real sysvar address pass.
        let real_key = anchor_lang::solana_program::sysvar::instructions::ID;
        let mut real_lamports = 0u64;
        let mut real_data = honest;
        let real = AccountInfo::new(
            &real_key,
            false,
            false,
            &mut real_lamports,
            &mut real_data,
            &owner,
            false,
            0,
        );
        assert!(require_swig_ixs_marker_bound(&real, &swig).is_ok());
    }

    #[test]
    fn union_tolerates_many_account_metas_when_parsing_later_ixs() {
        // A realistic SignV2 carries many account metas; the parser must skip
        // them (33 bytes each) to reach later instructions' program_id/disc.
        use crate::instruction as vix;
        let swig = Pubkey::new_unique();
        let mut big_sign = swig_ix_on(swig);
        for _ in 0..40 {
            big_sign
                .accounts
                .push(AccountMeta::new_readonly(Pubkey::new_unique(), false));
        }
        let ixs = vec![marker_ix(vix::SwapForCarry::DISCRIMINATOR, swig), big_sign];
        assert!(scan_marker_bound(&build_blob(&ixs), &swig).is_ok());
    }

    #[test]
    fn union_activation_birth_credits_exactly_the_preceding_birth() {
        use crate::instruction as vix;
        let swig = Pubkey::new_unique();
        let activate = || marker_ix(vix::ActivateWallet::DISCRIMINATOR, swig);
        // [CU, CreateV1, 3×AddAuthorityV1, SignV2 fee, UpdateAuthorityV1, activate_wallet]
        let mut birth = vec![filler()];
        birth.extend((0..6).map(|_| swig_ix_on(swig)));
        birth.push(activate());
        assert!(scan_marker_bound(&build_blob(&birth), &swig).is_ok());
        // fee = 0: no SignV2, five legs.
        let mut free = vec![filler()];
        free.extend((0..5).map(|_| swig_ix_on(swig)));
        free.push(activate());
        assert!(scan_marker_bound(&build_blob(&free), &swig).is_ok());
        // A Swig ix ahead of the six birth legs is credited by nothing.
        let mut early = vec![swig_ix_on(swig)];
        early.extend(birth.iter().cloned());
        assert!(scan_marker_bound(&build_blob(&early), &swig).is_err());
        // Anything on the new swig AFTER activate_wallet is unbound (the birth's
        // credit runs backward only).
        let mut appended = birth.clone();
        appended.push(swig_ix_on(swig));
        assert!(scan_marker_bound(&build_blob(&appended), &swig).is_err());
    }

    #[test]
    fn union_activation_marker_credits_only_its_own_swig() {
        use crate::instruction as vix;
        let swig = Pubkey::new_unique();
        let other = Pubkey::new_unique();
        // A birth marker naming `swig` never credits legs on `other`.
        let mut ixs: Vec<Instruction> = (0..6).map(|_| swig_ix_on(other)).collect();
        ixs.push(marker_ix(vix::ActivateWallet::DISCRIMINATOR, swig));
        assert!(scan_marker_bound(&build_blob(&ixs), &other).is_err());
    }

    #[test]
    fn union_provision_wallet_role_binds_only_its_add_authority() {
        use crate::instruction as vix;
        let swig = Pubkey::new_unique();
        let ok = vec![
            filler(),
            marker_ix(vix::ProvisionWalletRole::DISCRIMINATOR, swig),
            swig_ix_on(swig),
        ];
        assert!(scan_marker_bound(&build_blob(&ok), &swig).is_ok());
        // A second AddAuthorityV1 back-referencing the same provision marker
        // (deployed Swig's 2-byte ProgramExec payload) lands at +2: refused.
        let mut attacked = ok.clone();
        attacked.push(swig_ix_on(swig));
        assert!(scan_marker_bound(&build_blob(&attacked), &swig).is_err());
    }

    // ── P5 class rule: only vault ixs and Swig ixs with the CONFIG at account 0
    //    may list the swig config anywhere in the transaction ──────────────────

    fn wallet_of(swig: &Pubkey) -> Pubkey {
        Pubkey::find_program_address(
            &[crate::constants::SWIG_WALLET_ADDRESS_SEED, swig.as_ref()],
            &SWIG_PROGRAM_ID,
        )
        .0
    }

    fn ix(program_id: Pubkey, accounts: &[Pubkey], data: Vec<u8>) -> Instruction {
        Instruction {
            program_id,
            accounts: accounts.iter().map(|k| AccountMeta::new(*k, false)).collect(),
            data,
        }
    }

    /// The canonical guarded swap with everything that legitimately rides along:
    /// ComputeBudget, Ed25519 + Secp256r1 checks (no accounts), a wallet-owner
    /// ATA create (names the swig WALLET, never the config), the marker, its
    /// SignV2 (config @0, wallet @1), and finish_swap listing the config mid-list.
    fn canonical_swap_with_riders(swig: Pubkey) -> Vec<Instruction> {
        use crate::instruction as vix;
        let wallet = wallet_of(&swig);
        let compute_budget = anchor_lang::pubkey!("ComputeBudget111111111111111111111111111111");
        let ed25519 = anchor_lang::pubkey!("Ed25519SigVerify111111111111111111111111111");
        let secp256r1 = anchor_lang::pubkey!("Secp256r1SigVerify1111111111111111111111111");
        let ata_program = anchor_lang::pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
        let (payer, mint) = (Pubkey::new_unique(), Pubkey::new_unique());
        let ata = Pubkey::new_unique();
        vec![
            ix(compute_budget, &[], vec![2, 0, 0, 0, 0]),
            ix(ed25519, &[], vec![1, 0]),
            ix(secp256r1, &[], vec![1, 0]),
            ix(
                ata_program,
                &[payer, ata, wallet, mint, anchor_lang::system_program::ID, anchor_spl::token::ID],
                vec![1],
            ),
            {
                let mut m = marker_ix(vix::SwapForCarry::DISCRIMINATOR, swig);
                m.accounts.push(AccountMeta::new_readonly(swig, false));
                m
            },
            ix(SWIG_PROGRAM_ID, &[swig, wallet, payer, Pubkey::new_unique()], vec![11, 0]),
            ix(crate::ID, &[Pubkey::new_unique(), payer, swig], vix::FinishSwap::DISCRIMINATOR.to_vec()),
        ]
    }

    #[test]
    fn class_rule_admits_vault_compute_budget_sig_checks_and_wallet_owner_ata_create() {
        let swig = Pubkey::new_unique();
        assert!(scan_marker_bound(&build_blob(&canonical_swap_with_riders(swig)), &swig).is_ok());
    }

    #[test]
    fn class_rule_rejects_a_foreign_program_listing_the_swig_config() {
        let swig = Pubkey::new_unique();
        // Any slot: a foreign program reaching Swig role management through CPI
        // must list the config somewhere in its top-level account list.
        for slot in 0..4 {
            let mut accounts: Vec<Pubkey> = (0..4).map(|_| Pubkey::new_unique()).collect();
            accounts[slot] = swig;
            let mut ixs = canonical_swap_with_riders(swig);
            ixs.push(ix(Pubkey::new_unique(), &accounts, vec![7]));
            assert!(scan_marker_bound(&build_blob(&ixs), &swig).is_err(), "slot {slot}");
            // ...also when it sits BEFORE the marker (index order is irrelevant).
            let mut ixs = canonical_swap_with_riders(swig);
            ixs.insert(0, ix(Pubkey::new_unique(), &accounts, vec![7]));
            assert!(scan_marker_bound(&build_blob(&ixs), &swig).is_err(), "pre slot {slot}");
        }
        // The ATA create rider keyed on the CONFIG instead of the wallet is refused.
        let mut ixs = canonical_swap_with_riders(swig);
        ixs[3].accounts[2].pubkey = swig;
        assert!(scan_marker_bound(&build_blob(&ixs), &swig).is_err());
    }

    #[test]
    fn class_rule_rejects_a_swig_ix_on_another_swig_listing_the_config() {
        let (swig, other) = (Pubkey::new_unique(), Pubkey::new_unique());
        let mut ixs = canonical_swap_with_riders(swig);
        ixs.push(ix(SWIG_PROGRAM_ID, &[other, wallet_of(&other), swig], vec![11, 0]));
        assert!(scan_marker_bound(&build_blob(&ixs), &swig).is_err());
        // A Swig ix on another swig that does NOT name this config is still ignored.
        let mut ixs = canonical_swap_with_riders(swig);
        ixs.push(ix(SWIG_PROGRAM_ID, &[other, wallet_of(&other)], vec![11, 0]));
        assert!(scan_marker_bound(&build_blob(&ixs), &swig).is_ok());
    }

    #[test]
    fn class_rule_runs_without_any_swig_ix_on_the_config() {
        // The class rule is not gated on finding a Swig ix on the config: a
        // transaction whose only other instruction is a foreign one listing the
        // config is refused.
        use crate::instruction as vix;
        let swig = Pubkey::new_unique();
        let ixs = vec![
            marker_ix(vix::GrantAgentSession::DISCRIMINATOR, swig),
            ix(Pubkey::new_unique(), &[Pubkey::new_unique(), swig], vec![]),
        ];
        assert!(scan_marker_bound(&build_blob(&ixs), &swig).is_err());
    }

    #[test]
    fn class_rule_fails_closed_on_a_truncated_account_list() {
        let swig = Pubkey::new_unique();
        let mut ixs = canonical_swap_with_riders(swig);
        ixs.push(ix(Pubkey::new_unique(), &[Pubkey::new_unique(), Pubkey::new_unique()], vec![]));
        let blob = build_blob(&ixs);
        // Cut the blob inside the last instruction's second account meta.
        let last_off = blob_u16(&blob, 2 + 2 * (ixs.len() - 1)).unwrap();
        let cut = last_off + 2 + 33 + 10;
        assert!(scan_marker_bound(&blob[..cut], &swig).is_err());
        assert!(scan_marker_bound(&blob, &swig).is_ok());
    }
}
