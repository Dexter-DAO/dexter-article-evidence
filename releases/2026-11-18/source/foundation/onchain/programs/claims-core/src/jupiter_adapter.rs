//! Current Jupiter RouteV2, with a finite Whirlpool liquidity-route profile.
//!
//! The deployed Jupiter IDL and exact public ELF provenance are recorded beside
//! the adapter fixtures. A quote grants no authority: only the owner's economic
//! request and the observed token/custody effects authorize value movement.
use anchor_lang::prelude::*;
use anchor_lang::solana_program::{instruction::{AccountMeta, Instruction}, program::invoke};
use solana_sha256_hasher::hashv;
use dexter_contracts::{minus, plus, CORE_ID, POOL_ID, REGISTRY_ID};
use dexter_contracts::wallet_operation::{WalletRouteQuote, WalletSpendRequest, JUPITER_ID, TOKEN_ID};
use crate::{CoreError, wallet_tokens::{self, MintSnapshot, TokenSnapshot}};

pub const WHIRLPOOL_ID: Pubkey = pubkey!("whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc");
pub const MEMO_ID: Pubkey = pubkey!("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr");
pub const EVENT_AUTHORITY: Pubkey = pubkey!("D8cy77BBepLMngZx6ZukaTff5hCt1HrWyKk3Hnd9oitf");
pub const ROUTE_V2: [u8; 8] = [187, 100, 250, 204, 49, 196, 175, 20];
const WHIRLPOOL: [u8; 8] = [63, 149, 209, 12, 225, 128, 99, 9];
const ORACLE: [u8; 8] = [139, 194, 131, 179, 140, 179, 229, 244];
const FIXED_TICKS: [u8; 8] = [69, 97, 189, 190, 110, 7, 66, 187];
const DYNAMIC_TICKS: [u8; 8] = [17, 216, 246, 142, 225, 199, 218, 56];
pub const MAX_ROUTE_STEPS: usize = 4;
const MAX_ROUTE_ACCOUNTS: usize = 10 + 16 * MAX_ROUTE_STEPS;
const MAX_ROUTE_DATA: usize = 34 + 7 * MAX_ROUTE_STEPS;

#[derive(Clone, Copy)]
struct Step { v2: bool, a_to_b: bool }

fn key_at(bytes: &[u8], offset: usize) -> Pubkey {
    Pubkey::new_from_array(bytes[offset..offset + 32].try_into().unwrap())
}

/// Parse only the exercised current ABI. No token-ledger, exact-out, shared
/// account, arbitrary DEX, hook-slice or trailing-data fallback is admitted.
fn steps(data: &[u8], request: &WalletSpendRequest, quote: &WalletRouteQuote) -> Result<Vec<Step>> {
    require!(data.len() >= 34 && data[..8] == ROUTE_V2, CoreError::WalletSwapAdapter);
    let amount = u64::from_le_bytes(data[8..16].try_into().unwrap());
    let quoted = u64::from_le_bytes(data[16..24].try_into().unwrap());
    let slip = u16::from_le_bytes(data[24..26].try_into().unwrap());
    require!(amount == request.amount && amount == quote.quoted_input
        && quoted == quote.quoted_output && slip <= 10_000
        && data[26..30] == [0; 4], CoreError::WalletSwapAdapter);
    let count = u32::from_le_bytes(data[30..34].try_into().unwrap()) as usize;
    require!(count > 0 && count <= MAX_ROUTE_STEPS, CoreError::WalletSwapAdapter);
    let mut cursor = 34;
    let mut total = 0u32;
    let mut result = Vec::with_capacity(count);
    for _ in 0..count {
        require!(cursor + 6 <= data.len(), CoreError::WalletSwapAdapter);
        let tag = data[cursor];
        let direction = data[cursor + 1];
        require!((tag == 17 || tag == 47) && direction <= 1, CoreError::WalletSwapAdapter);
        cursor += 2;
        if tag == 47 {
            require!(cursor < data.len() && data[cursor] == 0, CoreError::WalletSwapAdapter);
            cursor += 1; // RemainingAccountsInfo must be None: no hooks or extra capabilities.
        }
        require!(cursor + 4 <= data.len(), CoreError::WalletSwapAdapter);
        let bps = u16::from_le_bytes(data[cursor..cursor + 2].try_into().unwrap());
        require!(bps > 0 && data[cursor + 2] == 0 && data[cursor + 3] == 1,
            CoreError::WalletSwapAdapter);
        total += u32::from(bps);
        cursor += 4;
        result.push(Step { v2: tag == 47, a_to_b: direction == 1 });
    }
    require!(cursor == data.len() && total == 10_000, CoreError::WalletSwapAdapter);
    Ok(result)
}

#[derive(Clone)]
struct PoolSnapshot<'info> {
    pool: AccountInfo<'info>,
    identity: [u8; 32],
    input: AccountInfo<'info>,
    output: AccountInfo<'info>,
    input_before: TokenSnapshot,
    output_before: TokenSnapshot,
}

/// Immutable pool routing fields; prices, liquidity, accrued fees and ticks may
/// change through the actual swap. The pool's canonical PDA binds its mint pair.
fn pool_identity(info: &AccountInfo) -> Result<[u8; 32]> {
    require!(info.owner == &WHIRLPOOL_ID && !info.executable, CoreError::WalletSwapAdapter);
    let b = info.try_borrow_data()?;
    require!(b.len() == 653 && b[..8] == WHIRLPOOL, CoreError::WalletSwapAdapter);
    let (expected, bump) = Pubkey::find_program_address(
        &[b"whirlpool", &b[8..40], &b[101..133], &b[181..213], &b[43..45]], &WHIRLPOOL_ID);
    require!(info.key() == expected && b[40] == bump, CoreError::WalletSwapAdapter);
    Ok(hashv(&[&b[..45], &b[101..165], &b[181..245]]).to_bytes())
}

/// Bound the complete current Whirlpool rate, including its adaptive component.
/// The pinned upstream FeeRateManager adds the adaptive rate to the base rate
/// and caps their total at 100,000 hundredths of a basis point. Substituting the
/// authenticated maximum volatility bounds every tick crossed during this swap;
/// checking only the pre-swap volatility or pool.fee_rate would understate it.
#[inline(never)]
fn whirlpool_fee_bps_bound(pool: &AccountInfo, oracle: &AccountInfo) -> Result<u16> {
    let p = pool.try_borrow_data()?;
    require!(p.len() == 653 && p[..8] == WHIRLPOOL, CoreError::WalletSwapAdapter);
    let base_rate = u16::from_le_bytes(p[45..47].try_into().unwrap());
    let tick_spacing = u16::from_le_bytes(p[41..43].try_into().unwrap());
    require!(tick_spacing > 0 && !oracle.executable && oracle.key()
        == Pubkey::find_program_address(&[b"oracle", pool.key.as_ref()], &WHIRLPOOL_ID).0,
        CoreError::WalletSwapAdapter);
    // Matches OracleAccessor's uninitialized-account branch. An initialized
    // canonical oracle is authoritative even if a pool's tier label is static.
    if oracle.owner == &anchor_lang::system_program::ID && oracle.data_is_empty() {
        return Ok(base_rate.div_ceil(100));
    }
    require!(oracle.owner == &WHIRLPOOL_ID, CoreError::WalletSwapAdapter);
    let b = oracle.try_borrow_data()?;
    require!(b.len() == 254 && b[..8] == ORACLE && key_at(&b, 8) == pool.key()
        && b[66..82].iter().all(|x| *x == 0) && b[110..].iter().all(|x| *x == 0),
        CoreError::WalletSwapAdapter);
    let filter = u16::from_le_bytes(b[48..50].try_into().unwrap());
    let decay = u16::from_le_bytes(b[50..52].try_into().unwrap());
    let reduction = u16::from_le_bytes(b[52..54].try_into().unwrap());
    let control = u32::from_le_bytes(b[54..58].try_into().unwrap());
    let maximum_volatility = u32::from_le_bytes(b[58..62].try_into().unwrap());
    let group = u16::from_le_bytes(b[62..64].try_into().unwrap());
    let major = u16::from_le_bytes(b[64..66].try_into().unwrap());
    let crossed = u128::from(maximum_volatility) * u128::from(group);
    require!(filter > 0 && decay > filter && reduction < 10_000 && control < 100_000
        && group > 0 && group <= tick_spacing && tick_spacing % group == 0
        && major > 0 && u32::from(major) <= u32::from(tick_spacing) * 88
        && crossed <= u128::from(u32::MAX), CoreError::WalletSwapAdapter);
    let adaptive = (u128::from(control) * crossed * crossed).div_ceil(10_000_000_000_000);
    let total = (u128::from(base_rate) + adaptive).min(100_000);
    Ok(total.div_ceil(100) as u16)
}

/// Exact shared-contract Borsh encoding, bounded on a separate SBF stack frame.
/// The default bump allocator cannot reclaim temporary Vec allocations. Building
/// a second 74-account vector plus its serialized copy needlessly exhausted the
/// 32 KiB heap after a four-pool swap while committing the second financial leg.
#[inline(never)]
fn payload_digest(program: &Pubkey, metas: &[AccountMeta], data: &[u8]) -> Result<[u8; 32]> {
    require!(metas.len() <= MAX_ROUTE_ACCOUNTS && data.len() <= MAX_ROUTE_DATA,
        CoreError::WalletSwapAdapter);
    let mut encoded = [0u8; 32 + 4 + 34 * MAX_ROUTE_ACCOUNTS + 4 + MAX_ROUTE_DATA];
    encoded[..32].copy_from_slice(program.as_ref());
    encoded[32..36].copy_from_slice(&(metas.len() as u32).to_le_bytes());
    let mut cursor = 36;
    for meta in metas {
        encoded[cursor..cursor + 32].copy_from_slice(meta.pubkey.as_ref());
        encoded[cursor + 32] = u8::from(meta.is_signer);
        encoded[cursor + 33] = u8::from(meta.is_writable);
        cursor += 34;
    }
    encoded[cursor..cursor + 4].copy_from_slice(&(data.len() as u32).to_le_bytes());
    cursor += 4;
    encoded[cursor..cursor + data.len()].copy_from_slice(data);
    cursor += data.len();
    Ok(hashv(&[b"DEXTER:WALLET-ROUTE-PAYLOAD:2", CORE_ID.as_ref(), &encoded[..cursor]]).to_bytes())
}

fn tick_array(info: &AccountInfo, pool: Pubkey) -> Result<()> {
    require!(info.owner == &WHIRLPOOL_ID && !info.executable, CoreError::WalletSwapAdapter);
    let b = info.try_borrow_data()?;
    require!(b.len() >= 12, CoreError::WalletSwapAdapter);
    let attached = if b[..8] == FIXED_TICKS {
        require!(b.len() == 9988, CoreError::WalletSwapAdapter);
        key_at(&b, 9956)
    } else {
        require!(b[..8] == DYNAMIC_TICKS && b.len() >= 148 && b.len() <= 10_004,
            CoreError::WalletSwapAdapter);
        key_at(&b, 12)
    };
    let start = i32::from_le_bytes(b[8..12].try_into().unwrap()).to_string();
    let expected = Pubkey::find_program_address(&[b"tick_array", pool.as_ref(), start.as_bytes()],
        &WHIRLPOOL_ID).0;
    require!(attached == pool && info.key() == expected, CoreError::WalletSwapAdapter);
    Ok(())
}

fn role<'info>(infos: &mut Vec<AccountInfo<'info>>, metas: &mut Vec<AccountMeta>,
    info: &AccountInfo<'info>, expected: Pubkey, signer: bool, writable: bool) -> Result<()> {
    require!(info.key() == expected && (!signer || info.is_signer)
        && (!writable || info.is_writable), CoreError::WalletSwapAdapter);
    infos.push(info.clone());
    metas.push(AccountMeta { pubkey: expected, is_signer: signer, is_writable: writable });
    Ok(())
}

pub struct ValidatedRoute<'info> {
    pub payload_digest: [u8; 32],
    instruction: Instruction,
    infos: Vec<AccountInfo<'info>>,
    source: AccountInfo<'info>,
    destination: AccountInfo<'info>,
    input_mint: AccountInfo<'info>,
    output_mint: AccountInfo<'info>,
    mint_in: MintSnapshot,
    mint_out: MintSnapshot,
    source_before: TokenSnapshot,
    destination_before: TokenSnapshot,
    pools: Vec<PoolSnapshot<'info>>,
    amount: u64,
    minimum_net_output: u64,
    maximum_input_fee: u64,
    maximum_output_fee: u64,
    input_control_digest: [u8;32],
    output_control_digest: [u8;32],
    venue_fee_bps_bound: u16,
}

#[derive(Clone, Copy, Debug)]
pub struct RouteOutcome {
    pub gross_debit: u64,
    pub net_received: u64,
    pub input_transfer_fee: u64,
    pub output_transfer_fee: u64,
}

/// The exact ten Jupiter roles are reconstructed here. `venues` contains only
/// Jupiter's ordered remaining roles, beginning with each Whirlpool executable;
/// Core/Registry/Pool bookkeeping accounts are never forwarded to the router.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub fn prepare<'info>(router: &AccountInfo<'info>, event: &AccountInfo<'info>,
    wallet: &AccountInfo<'info>, source: &AccountInfo<'info>, destination: &AccountInfo<'info>,
    input_mint: &AccountInfo<'info>, output_mint: &AccountInfo<'info>,
    input_program: &AccountInfo<'info>, output_program: &AccountInfo<'info>,
    venues: &[AccountInfo<'info>], data: &[u8], request: &WalletSpendRequest,
    quote: &WalletRouteQuote) -> Result<ValidatedRoute<'info>> {
    prepare_controlled(router,event,wallet,source,destination,input_mint,output_mint,input_program,
        output_program,venues,data,request,quote,[0;32],[0;32],u16::MAX)
}

#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub fn prepare_controlled<'info>(router: &AccountInfo<'info>, event: &AccountInfo<'info>,
    wallet: &AccountInfo<'info>, source: &AccountInfo<'info>, destination: &AccountInfo<'info>,
    input_mint: &AccountInfo<'info>, output_mint: &AccountInfo<'info>,
    input_program: &AccountInfo<'info>, output_program: &AccountInfo<'info>,
    venues: &[AccountInfo<'info>], data: &[u8], request: &WalletSpendRequest,
    quote: &WalletRouteQuote, input_control_digest: [u8;32], output_control_digest: [u8;32],
    maximum_venue_fee_bps: u16) -> Result<ValidatedRoute<'info>> {
    require!(router.key() == JUPITER_ID && router.executable && event.key() == EVENT_AUTHORITY
        && wallet.is_signer && !wallet.executable && input_program.executable && output_program.executable,
        CoreError::WalletSwapAdapter);
    require!(wallet.key() == request.wallet_authority && source.key() == request.source
        && destination.key() == request.destination && input_mint.key() == request.input_mint
        && output_mint.key() == request.output_mint && input_program.key() == request.input_token_program
        && output_program.key() == request.output_token_program, CoreError::WalletSwapAdapter);
    let steps = steps(data, request, quote)?;
    require!(venues.len() == steps.iter().map(|s| if s.v2 { 16 } else { 12 }).sum::<usize>(),
        CoreError::WalletSwapAdapter);
    for account in venues {
        require!(account.owner != &CORE_ID && account.owner != &REGISTRY_ID && account.owner != &POOL_ID
            && (!account.is_signer || account.key() == wallet.key()), CoreError::WalletSwapAdapter);
    }
    let epoch = Clock::get()?.epoch;
    let mint_in = wallet_tokens::read_mint_with_controls(input_mint, input_program.key(), epoch, input_control_digest)?;
    let mint_out = wallet_tokens::read_mint_with_controls(output_mint, output_program.key(), epoch, output_control_digest)?;
    let source_before = wallet_tokens::read_account(source, &mint_in)?;
    let destination_before = wallet_tokens::read_account(destination, &mint_out)?;
    require!(source_before.owner == wallet.key() && destination_before.owner == wallet.key()
        && source.key() != destination.key(), CoreError::WalletSwapAdapter);
    let mut infos = Vec::with_capacity(10 + venues.len());
    let mut metas = Vec::with_capacity(10 + venues.len());
    for (a, signer, writable) in [(wallet, true, false), (source, false, true),
        (destination, false, true), (input_mint, false, false), (output_mint, false, false),
        (input_program, false, false), (output_program, false, false), (router, false, false),
        (event, false, false), (router, false, false)] {
        role(&mut infos, &mut metas, a, a.key(), signer, writable)?;
    }
    let mut pools = Vec::with_capacity(steps.len());
    let mut offset = 0;
    let mut venue_fee_bps_bound=0u16;
    for step in steps {
        let v = &venues[offset..];
        let (pool_i, user_a_i, vault_a_i, user_b_i, vault_b_i, tick_i, oracle_i) =
            if step.v2 { (5, 8, 9, 10, 11, 12, 15) } else { (3, 4, 5, 6, 7, 8, 11) };
        let pool = &v[pool_i];
        require!(!pools.iter().any(|p: &PoolSnapshot| p.pool.key() == pool.key()), CoreError::WalletSwapAdapter);
        let identity = pool_identity(pool)?;
        let (mint_a, vault_a, mint_b, vault_b) = {
            let b = pool.try_borrow_data()?;
            (key_at(&b, 101), key_at(&b, 133), key_at(&b, 181), key_at(&b, 213))
        };
        let (expected_a, expected_b, user_a, user_b, program_a, program_b) = if step.a_to_b {
            (request.input_mint, request.output_mint, source.key(), destination.key(),
             input_program.key(), output_program.key())
        } else {
            (request.output_mint, request.input_mint, destination.key(), source.key(),
             output_program.key(), input_program.key())
        };
        require!(mint_a == expected_a && mint_b == expected_b && v[0].executable
            && vault_a != vault_b && vault_a != source.key() && vault_a != destination.key()
            && vault_b != source.key() && vault_b != destination.key(), CoreError::WalletSwapAdapter);
        role(&mut infos, &mut metas, &v[0], WHIRLPOOL_ID, false, false)?;
        if step.v2 {
            role(&mut infos, &mut metas, &v[1], program_a, false, false)?;
            role(&mut infos, &mut metas, &v[2], program_b, false, false)?;
            require!(v[3].executable, CoreError::WalletSwapAdapter);
            role(&mut infos, &mut metas, &v[3], MEMO_ID, false, false)?;
            role(&mut infos, &mut metas, &v[4], wallet.key(), false, false)?;
        } else {
            require!(program_a == TOKEN_ID && program_b == TOKEN_ID, CoreError::WalletSwapAdapter);
            role(&mut infos, &mut metas, &v[1], TOKEN_ID, false, false)?;
            role(&mut infos, &mut metas, &v[2], wallet.key(), false, false)?;
        }
        role(&mut infos, &mut metas, pool, pool.key(), false, true)?;
        if step.v2 {
            role(&mut infos, &mut metas, &v[6], mint_a, false, false)?;
            role(&mut infos, &mut metas, &v[7], mint_b, false, false)?;
        }
        role(&mut infos, &mut metas, &v[user_a_i], user_a, false, true)?;
        role(&mut infos, &mut metas, &v[vault_a_i], vault_a, false, true)?;
        role(&mut infos, &mut metas, &v[user_b_i], user_b, false, true)?;
        role(&mut infos, &mut metas, &v[vault_b_i], vault_b, false, true)?;
        for tick in &v[tick_i..tick_i + 3] {
            tick_array(tick, pool.key())?;
            role(&mut infos, &mut metas, tick, tick.key(), false, true)?;
        }
        let oracle = Pubkey::find_program_address(&[b"oracle", pool.key().as_ref()], &WHIRLPOOL_ID).0;
        venue_fee_bps_bound = venue_fee_bps_bound.checked_add(whirlpool_fee_bps_bound(pool, &v[oracle_i])?)
            .ok_or_else(|| error!(CoreError::WalletSwapAdapter))?;
        require!(venue_fee_bps_bound <= maximum_venue_fee_bps, CoreError::WalletSwapAdapter);
        role(&mut infos, &mut metas, &v[oracle_i], oracle, false, step.v2)?;
        let (input, output) = if step.a_to_b { (&v[vault_a_i], &v[vault_b_i]) }
            else { (&v[vault_b_i], &v[vault_a_i]) };
        let input_before = wallet_tokens::read_account(input, &mint_in)?;
        let output_before = wallet_tokens::read_account(output, &mint_out)?;
        require!(input_before.owner == pool.key() && output_before.owner == pool.key(), CoreError::WalletSwapAdapter);
        pools.push(PoolSnapshot { pool: pool.clone(), identity, input: input.clone(), output: output.clone(),
            input_before, output_before });
        offset += if step.v2 { 16 } else { 12 };
    }
    // Duplicate raw positions are retained, but signer/writable flags are the
    // union within this CPI only. Outer bookkeeping privileges grant no route powers.
    for i in 0..metas.len() {
        let (mut signer, mut writable) = (false, false);
        for m in &metas { if m.pubkey == metas[i].pubkey { signer |= m.is_signer; writable |= m.is_writable; } }
        metas[i].is_signer = signer;
        metas[i].is_writable = writable;
    }
    let payload_digest = payload_digest(&router.key(), &metas, data)?;
    require!(payload_digest == quote.payload_digest, CoreError::WalletSwapAdapter);
    Ok(ValidatedRoute { payload_digest, instruction: Instruction { program_id: router.key(), accounts: metas, data: data.to_vec() },
        infos, source: source.clone(), destination: destination.clone(), input_mint: input_mint.clone(),
        output_mint: output_mint.clone(), mint_in, mint_out, source_before, destination_before, pools,
        amount: request.amount, minimum_net_output: quote.minimum_net_output,
        maximum_input_fee: request.maximum_input_fee, maximum_output_fee: request.maximum_output_fee,
        input_control_digest,output_control_digest,venue_fee_bps_bound })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dexter_contracts::wallet_operation::{wallet_route_payload_digest, WalletRouteAccount};

    const STOCK_POOL: Pubkey = pubkey!("Djdm88bGU5NLHvMEWCZqsbLGd28vL8KeS2mxejTnPzoW");
    const STOCK_ORACLE: Pubkey = pubkey!("7V5Jzq3vUQ1r34G9V5JeMpmbtixuhfeFb8XspGZxqCQQ");
    // Exact bytes in the retained finalized stock-aaplx-buy-accounts.json,
    // slot 464284992. These retain the real pool's adaptive tier and constants.
    const STOCK_POOL_BYTES: &[u8] = include_bytes!("../tests/fixtures/stock-controls/aaplx-whirlpool.bin");
    const STOCK_ORACLE_BYTES: &[u8] = include_bytes!("../tests/fixtures/stock-controls/aaplx-oracle.bin");

    fn fee_bound(pool_bytes: &mut [u8], oracle_bytes: &mut [u8], oracle_key: Pubkey,
        oracle_owner: Pubkey) -> Result<u16> {
        let (mut pool_lamports, mut oracle_lamports) = (1, 1);
        let pool = AccountInfo::new(&STOCK_POOL, false, true, &mut pool_lamports,
            pool_bytes, &WHIRLPOOL_ID, false);
        let oracle = AccountInfo::new(&oracle_key, false, true, &mut oracle_lamports,
            oracle_bytes, &oracle_owner, false);
        whirlpool_fee_bps_bound(&pool, &oracle)
    }

    #[test]
    fn captured_stock_adaptive_rate_is_bounded_over_the_entire_swap() {
        let mut pool = STOCK_POOL_BYTES.to_vec();
        let mut oracle = STOCK_ORACLE_BYTES.to_vec();
        assert_eq!(u16::from_le_bytes(pool[45..47].try_into().unwrap()), 200);
        // 80,000 * (880,000 * 2)^2 / 10^13, rounded up, plus the
        // 200-millionth base rate: 24,981 millionths, rounded up to 250 bps.
        assert_eq!(fee_bound(&mut pool, &mut oracle, STOCK_ORACLE, WHIRLPOOL_ID).unwrap(), 250);
        // The current observed volatility can be zero; a swap can increase it.
        oracle[106..110].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(fee_bound(&mut pool, &mut oracle, STOCK_ORACLE, WHIRLPOOL_ID).unwrap(), 250);
        oracle[58..62].copy_from_slice(&1_000_000_000u32.to_le_bytes());
        assert_eq!(fee_bound(&mut pool, &mut oracle, STOCK_ORACLE, WHIRLPOOL_ID).unwrap(), 1_000);
    }

    #[test]
    fn adaptive_rate_requires_authenticated_supported_oracle_controls() {
        for (offset, value) in [(0, 0), (8, 0), (48, 0), (54, 255), (62, 0), (66, 1), (110, 1), (126, 1)] {
            let mut pool = STOCK_POOL_BYTES.to_vec();
            let mut oracle = STOCK_ORACLE_BYTES.to_vec();
            oracle[offset] = value;
            if offset == 54 { oracle[54..58].copy_from_slice(&100_000u32.to_le_bytes()); }
            assert!(fee_bound(&mut pool, &mut oracle, STOCK_ORACLE, WHIRLPOOL_ID).is_err(), "offset {offset}");
        }
        for (key, owner) in [(Pubkey::new_unique(), WHIRLPOOL_ID), (STOCK_ORACLE, TOKEN_ID)] {
            assert!(fee_bound(&mut STOCK_POOL_BYTES.to_vec(), &mut STOCK_ORACLE_BYTES.to_vec(), key, owner).is_err());
        }
        assert!(fee_bound(&mut STOCK_POOL_BYTES.to_vec(), &mut STOCK_ORACLE_BYTES[..253].to_vec(),
            STOCK_ORACLE, WHIRLPOOL_ID).is_err());
    }

    #[test]
    fn uninitialized_static_oracle_uses_rounded_base_rate() {
        let mut pool = STOCK_POOL_BYTES.to_vec();
        pool[43..45].copy_from_slice(&2u16.to_le_bytes());
        let mut empty = [];
        assert_eq!(fee_bound(&mut pool, &mut empty, STOCK_ORACLE, anchor_lang::system_program::ID).unwrap(), 2);
        pool[45..47].copy_from_slice(&201u16.to_le_bytes());
        assert_eq!(fee_bound(&mut pool, &mut empty, STOCK_ORACLE, anchor_lang::system_program::ID).unwrap(), 3);
    }

    #[test]
    fn bounded_encoding_matches_shared_contract_at_every_route_bound() {
        for steps in 1..=MAX_ROUTE_STEPS {
            let metas: Vec<_> = (0..10 + 16 * steps).map(|i| AccountMeta {
                pubkey: Pubkey::new_from_array([(i % 13) as u8; 32]),
                is_signer: i % 13 == 0, is_writable: i % 3 != 0,
            }).collect();
            let shared: Vec<_> = metas.iter().map(|m| WalletRouteAccount {
                address: m.pubkey, is_signer: m.is_signer, is_writable: m.is_writable,
            }).collect();
            let data = vec![47; 34 + 7 * steps];
            assert_eq!(payload_digest(&JUPITER_ID, &metas, &data).unwrap(),
                wallet_route_payload_digest(&JUPITER_ID, &shared, &data).unwrap());
        }
    }
}

impl ValidatedRoute<'_> {
    pub fn venue_fee_bps_bound(&self)->u16 {self.venue_fee_bps_bound}
    #[inline(never)]
    pub fn invoke_and_verify(self) -> Result<RouteOutcome> {
        invoke(&self.instruction, &self.infos)?;
        require!(wallet_tokens::read_mint_with_controls(&self.input_mint, self.mint_in.token_program, Clock::get()?.epoch, self.input_control_digest)? == self.mint_in
            && wallet_tokens::read_mint_with_controls(&self.output_mint, self.mint_out.token_program, Clock::get()?.epoch, self.output_control_digest)? == self.mint_out,
            CoreError::FundingCashControl);
        let source = wallet_tokens::read_account(&self.source, &self.mint_in)?;
        let destination = wallet_tokens::read_account(&self.destination, &self.mint_out)?;
        wallet_tokens::account_identity(&self.source_before, &source)?;
        wallet_tokens::account_identity(&self.destination_before, &destination)?;
        let gross_debit = minus(self.source_before.amount, source.amount)?;
        let net_received = minus(destination.amount, self.destination_before.amount)?;
        let output_transfer_fee = minus(destination.withheld, self.destination_before.withheld)?;
        require!(gross_debit == self.amount && net_received >= self.minimum_net_output
            && source.withheld == self.source_before.withheld, CoreError::WalletSwapOutcome);
        let (mut input_gross, mut input_transfer_fee, mut output_gross, mut expected_output_fee) = (0, 0, 0, 0);
        for pool in self.pools {
            require!(pool_identity(&pool.pool)? == pool.identity, CoreError::WalletSwapAdapter);
            let input = wallet_tokens::read_account(&pool.input, &self.mint_in)?;
            let output = wallet_tokens::read_account(&pool.output, &self.mint_out)?;
            wallet_tokens::account_identity(&pool.input_before, &input)?;
            wallet_tokens::account_identity(&pool.output_before, &output)?;
            let fee = minus(input.withheld, pool.input_before.withheld)?;
            let received = minus(input.amount, pool.input_before.amount)?;
            let gross = plus(received, fee)?;
            let sent = minus(pool.output_before.amount, output.amount)?;
            require!(gross > 0 && sent > 0 && fee == self.mint_in.fee(gross)?
                && output.withheld == pool.output_before.withheld, CoreError::WalletSwapOutcome);
            input_gross = plus(input_gross, gross)?;
            input_transfer_fee = plus(input_transfer_fee, fee)?;
            output_gross = plus(output_gross, sent)?;
            expected_output_fee = plus(expected_output_fee, self.mint_out.fee(sent)?)?;
        }
        require!(input_gross == gross_debit && output_gross == plus(net_received, output_transfer_fee)?
            && output_transfer_fee == expected_output_fee && input_transfer_fee <= self.maximum_input_fee
            && output_transfer_fee <= self.maximum_output_fee, CoreError::WalletSwapOutcome);
        Ok(RouteOutcome { gross_debit, net_received, input_transfer_fee, output_transfer_fee })
    }
}
