//! Bounded current Pump curve V3, PumpSwap V2 and chained curve/pool execution.
//! The exact official ABI, deployed binaries and local liquidity provenance are
//! pinned by the program qualification. No arbitrary remaining account or mint
//! becomes spendable authority merely because the upstream venue accepts it.
use anchor_lang::prelude::*;
use anchor_lang::solana_program::{instruction::{AccountMeta, Instruction},
    program::{invoke, invoke_signed}, system_instruction};
use anchor_spl::token::spl_token;
use solana_sha256_hasher::hashv;
use dexter_contracts::{minus, plus, CORE_ID, POOL_ID, REGISTRY_ID};
use dexter_contracts::wallet_operation::{WalletRouteQuote, WalletSpendRequest, TOKEN_ID, TOKEN_2022_ID};
use dexter_contracts::trading::{PUMP_ID, PUMP_AMM_ID, WalletTradingHop};
use crate::{CoreError, wallet_tokens::{self, MintSnapshot, TokenSnapshot}};
use crate::jupiter_adapter::RouteOutcome;

pub const FEE_PROGRAM: Pubkey = pubkey!("pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ");
pub const WSOL: Pubkey = pubkey!("So11111111111111111111111111111111111111112");
pub const BRIDGE_SEED: &[u8] = b"pump-native-bridge";
pub const MAX_HOPS: usize = 4;
// The receipt reserves four slots, but current qualified creation state permits
// depth 0..=2: at most three connected markets. A fourth hop needs independent
// execution qualification before it becomes an admitted route.
const MAX_ADMITTED_HOPS: usize = 3;
const CURVE_BUY: [u8; 8] = [225,247,80,30,213,179,132,136];
const CURVE_SELL: [u8; 8] = [28,146,222,119,38,196,105,213];
const AMM_BUY: [u8; 8] = [194,171,28,70,104,77,91,47];
const AMM_SELL: [u8; 8] = [93,246,130,60,231,233,64,178];
const MULTI_HOP: [u8; 8] = [43,100,73,19,233,246,111,148];
const CURVE: [u8; 8] = [23,183,248,55,96,216,172,96];
const POOL: [u8; 8] = [241,154,109,4,17,177,109,188];
const GLOBAL: [u8; 8] = [167,232,232,177,200,108,114,127];
const AMM_GLOBAL: [u8; 8] = [149,8,156,202,160,252,176,217];
const FEE_CONFIG: [u8; 8] = [143,52,146,187,219,123,76,155];
const ACCUMULATOR: [u8; 8] = [86,255,112,14,102,53,154,250];
const MAX_ACCOUNTS: usize = 16 + 5 * MAX_ADMITTED_HOPS;

pub type PumpHopOutcome = WalletTradingHop;
pub struct PumpOutcome {
    pub movement: RouteOutcome,
    pub hop_count: u8,
    pub venue_fee_bps_bound: u16,
    pub hops: [PumpHopOutcome; MAX_HOPS],
}
pub struct PumpPolicy<'a> {
    pub maximum_venue_fee_bps: u16,
    pub allowed_intermediates: &'a [(Pubkey, Pubkey)],
}

fn key_at(data: &[u8], offset: usize) -> Pubkey {
    Pubkey::new_from_array(data[offset..offset + 32].try_into().unwrap())
}
fn number(data: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap())
}
fn address(program: &Pubkey, seeds: &[&[u8]]) -> Pubkey {
    Pubkey::find_program_address(seeds, program).0
}
fn pda(info: &AccountInfo, program: Pubkey, seeds: &[&[u8]]) -> Result<()> {
    require!(info.key() == address(&program, seeds), CoreError::WalletSwapAdapter);
    Ok(())
}
fn executable(info: &AccountInfo, expected: Pubkey) -> Result<()> {
    require!(info.key() == expected && info.executable, CoreError::WalletSwapAdapter);
    Ok(())
}
fn readonly_state(info: &AccountInfo, program: Pubkey, discriminator: [u8; 8], size: usize) -> Result<()> {
    require!(info.owner == &program && !info.executable, CoreError::WalletSwapAdapter);
    let bytes = info.try_borrow_data()?;
    require!(bytes.len() >= size && bytes[..8] == discriminator
        && bytes[size..].iter().all(|b| *b == 0), CoreError::WalletSwapAdapter);
    Ok(())
}
fn accumulator(info: &AccountInfo, program: Pubkey, wallet: Pubkey) -> Result<()> {
    pda(info, program, &[b"user_volume_accumulator", wallet.as_ref()])?;
    require!(info.owner == &program && !info.executable && info.is_writable,
        CoreError::WalletSwapAdapter);
    let b = info.try_borrow_data()?;
    // Both deployed programs allocate the full 137-byte version. Trading may
    // update volume, never spend the Principal's native funds on its creation.
    require!(b.len() >= 137 && b[..8] == ACCUMULATOR && key_at(&b, 8) == wallet,
        CoreError::WalletSwapAdapter);
    Ok(())
}
fn strict_mint(info: &AccountInfo, epoch: u64) -> Result<MintSnapshot> {
    let mint = wallet_tokens::read_mint(info, *info.owner, epoch)?;
    // The actual Pump programs' admitted profile does not promise fee-token
    // transfers or issuer-controlled intermediary custody. Other adapters keep
    // their independently qualified token-extension policies.
    require!(mint.fee(u64::MAX)? == 0, CoreError::FundingCashControl);
    Ok(mint)
}

#[derive(Clone, Copy, Default)]
struct FeeBound { lp: u64, protocol: u64, creator: u64 }
impl FeeBound {
    fn include(&mut self, bytes: &[u8], offset: usize) -> Result<()> {
        require!(offset + 24 <= bytes.len(), CoreError::WalletSwapAdapter);
        let lp = number(bytes, offset);
        let protocol = number(bytes, offset + 8);
        let creator = number(bytes, offset + 16);
        require!(lp <= 10_000 && protocol <= 10_000 && creator <= 10_000,
            CoreError::WalletSwapAdapter);
        self.lp = self.lp.max(lp); self.protocol = self.protocol.max(protocol);
        self.creator = self.creator.max(creator);
        Ok(())
    }
    fn total(&self) -> Result<u64> { plus(plus(self.lp, self.protocol)?, self.creator) }
}

/// Conservative authenticated maximum over current flat, SOL/stable tiers,
/// exotic rates and global fallback. This deliberately overestimates the
/// selected tier; unlike-mint fee atoms are never added together.
fn fee_bound(config: &AccountInfo, global: &AccountInfo, program: Pubkey,
    creator_override: u64) -> Result<FeeBound> {
    pda(config, FEE_PROGRAM, &[b"fee_config", program.as_ref()])?;
    require!(config.owner == &FEE_PROGRAM && !config.executable, CoreError::WalletSwapAdapter);
    let b = config.try_borrow_data()?;
    require!(b.len() >= 69 && b[..8] == FEE_CONFIG, CoreError::WalletSwapAdapter);
    let mut bound = FeeBound::default();
    bound.include(&b, 41)?;
    let mut cursor = 65;
    for _ in 0..2 {
        require!(cursor + 4 <= b.len(), CoreError::WalletSwapAdapter);
        let count = u32::from_le_bytes(b[cursor..cursor + 4].try_into().unwrap()) as usize;
        require!(count > 0 && count <= 64, CoreError::WalletSwapAdapter);
        cursor += 4;
        for _ in 0..count {
            require!(cursor + 40 <= b.len(), CoreError::WalletSwapAdapter);
            bound.include(&b, cursor + 16)?;
            cursor += 40;
        }
    }
    bound.include(&b, cursor)?;
    cursor += 24;
    require!(b[cursor..].iter().all(|x| *x == 0), CoreError::WalletSwapAdapter);
    drop(b);
    let g = global.try_borrow_data()?;
    if program == PUMP_ID {
        bound.lp = 0;
        bound.protocol = bound.protocol.max(number(&g, 105));
        bound.creator = bound.creator.max(number(&g, 154));
    } else {
        bound.lp = bound.lp.max(number(&g, 40));
        bound.protocol = bound.protocol.max(number(&g, 48));
        bound.creator = bound.creator.max(number(&g, 313));
    }
    require!(creator_override <= 10_000, CoreError::WalletSwapAdapter);
    bound.creator = bound.creator.max(creator_override);
    require!(bound.total()? <= 10_000, CoreError::WalletSwapAdapter);
    Ok(bound)
}

#[derive(Clone)]
struct MarketSnapshot<'info> {
    market: AccountInfo<'info>,
    base_mint_info: AccountInfo<'info>,
    quote_mint_info: AccountInfo<'info>,
    base: AccountInfo<'info>,
    quote: AccountInfo<'info>,
    base_mint: MintSnapshot,
    quote_mint: MintSnapshot,
    base_before: TokenSnapshot,
    quote_before: Option<TokenSnapshot>,
    base_lamports: u64,
    quote_lamports: u64,
    market_lamports: u64,
    identity: [u8; 32],
    protocol_before: u64,
    creator_before: u64,
    fees: FeeBound,
    native_quote: bool,
    buy: bool,
}

fn market_identity(info: &AccountInfo) -> Result<[u8; 32]> {
    let b = info.try_borrow_data()?;
    if info.owner == &PUMP_ID {
        require!(b.len() >= 166 && b[..8] == CURVE && b[166..].iter().all(|x| *x == 0),
            CoreError::WalletSwapAdapter);
        Ok(hashv(&[&b[..8], &b[49..125], &b[141..150]]).to_bytes())
    } else {
        require!(info.owner == &PUMP_AMM_ID && b.len() >= 287 && b[..8] == POOL
            && b[287..].iter().all(|x| *x == 0), CoreError::WalletSwapAdapter);
        Ok(hashv(&[&b[..245], &b[261..271]]).to_bytes())
    }
}

#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn market<'info>(state: &AccountInfo<'info>, base_info: &AccountInfo<'info>,
    quote_info: &AccountInfo<'info>, base: &AccountInfo<'info>, quote: &AccountInfo<'info>,
    global: &AccountInfo<'info>, fee_config: &AccountInfo<'info>, current: Pubkey,
    multi: bool, epoch: u64) -> Result<MarketSnapshot<'info>> {
    require!(!state.executable && state.is_writable && base.is_writable && quote.is_writable,
        CoreError::WalletSwapAdapter);
    let identity = market_identity(state)?;
    let base_mint = strict_mint(base_info, epoch)?;
    let quote_mint = strict_mint(quote_info, epoch)?;
    require!(base_mint.mint != quote_mint.mint, CoreError::WalletSwapAdapter);
    let buy = current == quote_mint.mint;
    require!(buy || current == base_mint.mint, CoreError::WalletSwapAdapter);
    let native_quote = state.owner == &PUMP_ID && quote_mint.mint == WSOL;
    let (protocol_before, creator_before, creator_override) = {
        let b = state.try_borrow_data()?;
        if state.owner == &PUMP_ID {
            pda(state, PUMP_ID, &[b"bonding-curve", base_info.key.as_ref()])?;
            let quote_key = key_at(&b, 83);
            require!(b[48] == 0 && b[81] == 0 && b[82] == 0
                && (quote_key == quote_mint.mint || (native_quote && quote_key == Pubkey::default())),
                CoreError::WalletSwapAdapter);
            require!(b[123] <= 1 && b[124] <= 1 && b[141] <= 2, CoreError::WalletSwapAdapter);
            (number(&b, 133), number(&b, 125), number(&b, 115))
        } else {
            require!(key_at(&b, 43) == base_mint.mint && key_at(&b, 75) == quote_mint.mint
                && key_at(&b, 139) == base.key() && key_at(&b, 171) == quote.key()
                && b[243] == 0 && b[244] == 0 && b[269] <= 1 && b[270] <= 1,
                CoreError::WalletSwapAdapter);
            let creator = key_at(&b, 11);
            let (expected, bump) = Pubkey::find_program_address(&[b"pool", &b[9..11], creator.as_ref(),
                base_mint.mint.as_ref(), quote_mint.mint.as_ref()], &PUMP_AMM_ID);
            require!(state.key() == expected && b[8] == bump, CoreError::WalletSwapAdapter);
            if multi {
                require!(b[9..11] == [0, 0]
                    && creator == address(&PUMP_ID, &[b"pool-authority", base_mint.mint.as_ref()]),
                    CoreError::WalletSwapAdapter);
            }
            (number(&b, 271), number(&b, 279), number(&b, 261))
        }
    };
    require!(base.key() == base_mint.ata(state.key) && quote.key() == quote_mint.ata(state.key),
        CoreError::WalletSwapAdapter);
    let base_before = wallet_tokens::read_account(base, &base_mint)?;
    require!(base_before.owner == state.key(), CoreError::WalletSwapAdapter);
    let quote_before = if native_quote { None } else {
        let value = wallet_tokens::read_account(quote, &quote_mint)?;
        require!(value.owner == state.key(), CoreError::WalletSwapAdapter);
        if state.owner == &PUMP_AMM_ID {
            let b = state.try_borrow_data()?;
            let virtual_quote = i128::from_le_bytes(b[245..261].try_into().unwrap());
            let effective = i128::from(value.amount).checked_add(virtual_quote)
                .ok_or_else(|| error!(CoreError::WalletSwapAdapter))?;
            require!(effective >= 0 && effective <= i128::from(u64::MAX), CoreError::WalletSwapAdapter);
        }
        Some(value)
    };
    let fees = fee_bound(fee_config, global, *state.owner, creator_override)?;
    Ok(MarketSnapshot { market: state.clone(), base_mint_info: base_info.clone(),
        quote_mint_info: quote_info.clone(), base: base.clone(), quote: quote.clone(),
        base_mint, quote_mint, base_before, quote_before, base_lamports: base.lamports(),
        quote_lamports: quote.lamports(), market_lamports: state.lamports(), identity,
        protocol_before, creator_before, fees, native_quote, buy })
}

fn role<'info>(infos: &mut Vec<AccountInfo<'info>>, metas: &mut Vec<AccountMeta>,
    info: &AccountInfo<'info>, expected: Pubkey, signer: bool, writable: bool) -> Result<()> {
    require!(info.key() == expected && (!signer || info.is_signer)
        && (!writable || info.is_writable), CoreError::WalletSwapAdapter);
    infos.push(info.clone());
    metas.push(AccountMeta { pubkey: expected, is_signer: signer, is_writable: writable });
    Ok(())
}

#[inline(never)]
fn payload_digest(program: Pubkey, metas: &[AccountMeta], data: &[u8]) -> Result<[u8; 32]> {
    require!(metas.len() <= MAX_ACCOUNTS && data.len() <= 25, CoreError::WalletSwapAdapter);
    let mut b = [0u8; 32 + 4 + 34 * MAX_ACCOUNTS + 4 + 25];
    b[..32].copy_from_slice(program.as_ref());
    b[32..36].copy_from_slice(&(metas.len() as u32).to_le_bytes());
    let mut offset = 36;
    for meta in metas {
        b[offset..offset + 32].copy_from_slice(meta.pubkey.as_ref());
        b[offset + 32] = u8::from(meta.is_signer); b[offset + 33] = u8::from(meta.is_writable);
        offset += 34;
    }
    b[offset..offset + 4].copy_from_slice(&(data.len() as u32).to_le_bytes()); offset += 4;
    b[offset..offset + data.len()].copy_from_slice(data); offset += data.len();
    Ok(hashv(&[b"DEXTER:WALLET-ROUTE-PAYLOAD:2", CORE_ID.as_ref(), &b[..offset]]).to_bytes())
}

pub struct ValidatedRoute<'info> {
    pub payload_digest: [u8; 32],
    instruction: Instruction,
    infos: Vec<AccountInfo<'info>>,
    venue_lamports: Vec<u64>,
    wallet: AccountInfo<'info>,
    source: AccountInfo<'info>,
    destination: AccountInfo<'info>,
    input_mint: AccountInfo<'info>,
    output_mint: AccountInfo<'info>,
    legacy_token: AccountInfo<'info>,
    system: AccountInfo<'info>,
    bridge: AccountInfo<'info>,
    mint_in: MintSnapshot,
    mint_out: MintSnapshot,
    source_before: TokenSnapshot,
    destination_before: TokenSnapshot,
    source_lamports: u64,
    destination_lamports: u64,
    wallet_lamports: u64,
    buyback: AccountInfo<'info>,
    buyback_before: u64,
    buyback_lamports: u64,
    buyback_snapshot: Option<TokenSnapshot>,
    markets: Vec<MarketSnapshot<'info>>,
    amount: u64,
    minimum: u64,
    fee_bound: u16,
}

#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub fn prepare<'info>(router: &AccountInfo<'info>, wallet: &AccountInfo<'info>,
    source: &AccountInfo<'info>, destination: &AccountInfo<'info>,
    input_mint: &AccountInfo<'info>, output_mint: &AccountInfo<'info>,
    input_program: &AccountInfo<'info>, output_program: &AccountInfo<'info>,
    system: &AccountInfo<'info>, legacy_token: &AccountInfo<'info>, token2022: &AccountInfo<'info>,
    bridge: &AccountInfo<'info>, venues: &[AccountInfo<'info>], data: &[u8],
    request: &WalletSpendRequest, quote: &WalletRouteQuote, policy: PumpPolicy<'_>) -> Result<ValidatedRoute<'info>> {
    require!(router.executable && (router.key() == PUMP_ID || router.key() == PUMP_AMM_ID)
        && wallet.is_signer && wallet.is_writable && wallet.owner == &system_program::ID
        && wallet.data_is_empty() && !wallet.executable, CoreError::WalletSwapAdapter);
    executable(system, system_program::ID)?; executable(legacy_token, TOKEN_ID)?;
    executable(token2022, TOKEN_2022_ID)?;
    require!(wallet.key() == request.wallet_authority && source.key() == request.source
        && destination.key() == request.destination && input_mint.key() == request.input_mint
        && output_mint.key() == request.output_mint && input_program.key() == request.input_token_program
        && output_program.key() == request.output_token_program && request.adapter_program == router.key()
        && input_program.executable && output_program.executable, CoreError::WalletSwapAdapter);
    require!(data.len() >= 24 && data.len() <= 25 && number(data, 8) == request.amount
        && quote.quoted_input == request.amount && number(data, 16) == quote.minimum_net_output
        && quote.minimum_net_output >= request.minimum_output && request.minimum_output > 0,
        CoreError::WalletSwapAdapter);
    let multi = router.key() == PUMP_AMM_ID && data[..8] == MULTI_HOP;
    let direct_buy = if router.key() == PUMP_ID {
        require!((data[..8] == CURVE_BUY && data.len() == 25 && data[24] == 0)
            || (data[..8] == CURVE_SELL && data.len() == 24), CoreError::WalletSwapAdapter);
        data[..8] == CURVE_BUY
    } else {
        require!(data.len() == 24 && (data[..8] == AMM_BUY || data[..8] == AMM_SELL || multi),
            CoreError::WalletSwapAdapter);
        data[..8] == AMM_BUY
    };
    let count = if multi {
        require!(venues.len() >= 26 && (venues.len() - 16) % 5 == 0, CoreError::WalletSwapAdapter);
        (venues.len() - 16) / 5
    } else { require!(venues.len() == 17, CoreError::WalletSwapAdapter); 1 };
    require!(count <= MAX_ADMITTED_HOPS && policy.maximum_venue_fee_bps <= 10_000
        && policy.allowed_intermediates.len() <= 3, CoreError::WalletSwapAdapter);
    for account in venues {
        require!(account.owner != &CORE_ID && account.owner != &REGISTRY_ID && account.owner != &POOL_ID
            && (!account.is_signer || account.key() == wallet.key()) && account.key() != bridge.key(),
            CoreError::WalletSwapAdapter);
    }
    let epoch = Clock::get()?.epoch;
    let mint_in = strict_mint(input_mint, epoch)?;
    let mint_out = strict_mint(output_mint, epoch)?;
    require!(mint_in.token_program == input_program.key() && mint_out.token_program == output_program.key(),
        CoreError::WalletSwapAdapter);
    let source_before = wallet_tokens::read_account(source, &mint_in)?;
    let destination_before = wallet_tokens::read_account(destination, &mint_out)?;
    require!(source_before.owner == wallet.key() && destination_before.owner == wallet.key()
        && source.key() != destination.key() && source.key() == mint_in.ata(wallet.key)
        && destination.key() == mint_out.ata(wallet.key), CoreError::WalletSwapAdapter);
    let mut infos = Vec::with_capacity(venues.len());
    let mut metas = Vec::with_capacity(venues.len());
    let mut markets = Vec::with_capacity(count);
    let buyback_index;
    if multi {
        let v = venues;
        readonly_state(&v[3], PUMP_AMM_ID, AMM_GLOBAL, 949)?;
        pda(&v[3], PUMP_AMM_ID, &[b"global_config"])?;
        readonly_state(&v[13], PUMP_ID, GLOBAL, 1088)?;
        pda(&v[13], PUMP_ID, &[b"global"])?;
        accumulator(&v[5], PUMP_AMM_ID, wallet.key())?;
        let expected = [wallet.key(), source.key(), destination.key(), v[3].key(),
            address(&FEE_PROGRAM, &[b"fee_config", PUMP_AMM_ID.as_ref()]), v[5].key(), v[6].key(),
            TOKEN_ID, TOKEN_2022_ID, system_program::ID,
            address(&PUMP_AMM_ID, &[b"__event_authority"]), PUMP_AMM_ID, PUMP_ID,
            v[13].key(), address(&FEE_PROGRAM, &[b"fee_config", PUMP_ID.as_ref()]),
            address(&PUMP_ID, &[b"__event_authority"])];
        for (i, key) in expected.iter().enumerate() {
            role(&mut infos, &mut metas, &v[i], *key, i == 0, matches!(i, 0|1|2|5|6))?;
        }
        for (i, key) in [(7,TOKEN_ID),(8,TOKEN_2022_ID),(9,system_program::ID),(11,PUMP_AMM_ID),(12,PUMP_ID)] {
            executable(&v[i], key)?;
        }
        let mut current = request.input_mint;
        let mut direction = None;
        for i in 0..count {
            let h = &v[16 + i * 5..16 + (i + 1) * 5];
            let (global, fees) = if h[2].owner == &PUMP_ID { (&v[13], &v[14]) } else { (&v[3], &v[4]) };
            let step = market(&h[2], &h[0], &h[1], &h[3], &h[4], global, fees, current, true, epoch)?;
            require!(direction.map_or(true, |buy| buy == step.buy), CoreError::WalletSwapAdapter);
            direction = Some(step.buy);
            current = if step.buy { step.base_mint.mint } else { step.quote_mint.mint };
            if i + 1 != count {
                let program = if step.buy { step.base_mint.token_program } else { step.quote_mint.token_program };
                require!(current != request.input_mint && current != request.output_mint
                    && policy.allowed_intermediates.contains(&(current, program)), CoreError::WalletSwapAdapter);
            }
            for (j, info) in h.iter().enumerate() { role(&mut infos, &mut metas, info, info.key(), false, j >= 2)?; }
            markets.push(step);
        }
        require!(current == request.output_mint, CoreError::WalletSwapAdapter);
        buyback_index = 6;
    } else if router.key() == PUMP_ID {
        let v = venues;
        readonly_state(&v[0], PUMP_ID, GLOBAL, 1088)?; pda(&v[0], PUMP_ID, &[b"global"])?;
        accumulator(&v[11], PUMP_ID, wallet.key())?;
        let step = market(&v[5], &v[1], &v[2], &v[6], &v[7], &v[0], &v[12], request.input_mint, false, epoch)?;
        require!(step.buy == direct_buy
            && (if step.buy {step.base_mint.mint} else {step.quote_mint.mint}) == request.output_mint,
            CoreError::WalletSwapAdapter);
        let user_base = if direct_buy { destination.key() } else { source.key() };
        let user_quote = if direct_buy { source.key() } else { destination.key() };
        let expected = [v[0].key(), step.base_mint.mint, step.quote_mint.mint, step.base_mint.token_program,
            step.quote_mint.token_program, v[5].key(), v[6].key(), v[7].key(), wallet.key(), user_base,
            user_quote, v[11].key(), v[12].key(), v[13].key(), system_program::ID,
            address(&PUMP_ID, &[b"__event_authority"]), PUMP_ID];
        for (i, key) in expected.iter().enumerate() {
            role(&mut infos, &mut metas, &v[i], *key, i == 8, matches!(i, 5|6|7|8|9|10|11|13))?;
        }
        executable(&v[3], step.base_mint.token_program)?; executable(&v[4], step.quote_mint.token_program)?;
        executable(&v[14], system_program::ID)?; executable(&v[16], PUMP_ID)?;
        markets.push(step); buyback_index = 13;
    } else {
        let v = venues;
        readonly_state(&v[2], PUMP_AMM_ID, AMM_GLOBAL, 949)?; pda(&v[2], PUMP_AMM_ID, &[b"global_config"])?;
        accumulator(&v[12], PUMP_AMM_ID, wallet.key())?;
        let step = market(&v[0], &v[3], &v[4], &v[7], &v[8], &v[2], &v[13], request.input_mint, false, epoch)?;
        require!(step.buy == direct_buy
            && (if step.buy {step.base_mint.mint} else {step.quote_mint.mint}) == request.output_mint,
            CoreError::WalletSwapAdapter);
        let expected = [v[0].key(), wallet.key(), v[2].key(), step.base_mint.mint, step.quote_mint.mint,
            if direct_buy {destination.key()} else {source.key()},
            if direct_buy {source.key()} else {destination.key()}, v[7].key(), v[8].key(),
            step.base_mint.token_program, step.quote_mint.token_program, system_program::ID,
            v[12].key(), v[13].key(), v[14].key(), address(&PUMP_AMM_ID, &[b"__event_authority"]), PUMP_AMM_ID];
        for (i, key) in expected.iter().enumerate() {
            role(&mut infos, &mut metas, &v[i], *key, i == 1, matches!(i, 0|1|5|6|7|8|12|14))?;
        }
        executable(&v[9], step.base_mint.token_program)?; executable(&v[10], step.quote_mint.token_program)?;
        executable(&v[11], system_program::ID)?; executable(&v[16], PUMP_AMM_ID)?;
        markets.push(step); buyback_index = 14;
    }
    let mut total = 0;
    for (i, step) in markets.iter().enumerate() {
        require!(!markets[..i].iter().any(|other| other.market.key() == step.market.key()),
            CoreError::WalletSwapAdapter);
        for key in [step.market.key(), step.base.key(), step.quote.key()] {
            require!(key != wallet.key() && key != source.key() && key != destination.key()
                && key != bridge.key(), CoreError::WalletSwapAdapter);
        }
        require!(!step.native_quote || (step.buy && i == 0) || (!step.buy && i + 1 == count),
            CoreError::WalletSwapAdapter);
        total = plus(total, step.fees.total()?)?;
    }
    require!(total <= u64::from(policy.maximum_venue_fee_bps), CoreError::WalletSwapOutcome);
    let currency_index = if markets[0].buy {0} else {count - 1};
    let currency = &markets[currency_index];
    let buyback = &venues[buyback_index];
    require!(buyback.key() != wallet.key() && buyback.key() != source.key()
        && buyback.key() != destination.key() && buyback.key() != bridge.key()
        && !markets.iter().any(|step| buyback.key() == step.market.key()
            || buyback.key() == step.base.key() || buyback.key() == step.quote.key()),
        CoreError::WalletSwapAdapter);
    let global = if multi { if currency.market.owner == &PUMP_ID {&venues[13]} else {&venues[3]} }
        else if router.key() == PUMP_ID {&venues[0]} else {&venues[2]};
    let native_buyback = !multi && currency.native_quote;
    let mut buyback_snapshot = if native_buyback {
        require!(buyback.owner == &system_program::ID && buyback.data_is_empty() && !buyback.executable,
            CoreError::WalletSwapAdapter); None
    } else { Some(wallet_tokens::read_account(buyback, &currency.quote_mint)?) };
    let recipient = buyback_snapshot.map_or(buyback.key(), |account| account.owner);
    {
        let b = global.try_borrow_data()?;
        let offset = if global.owner == &PUMP_ID {741} else {643};
        require!((0..8).any(|i| key_at(&b, offset + i * 32) == recipient), CoreError::WalletSwapAdapter);
        if !native_buyback { require!(buyback.key() == currency.quote_mint.ata(&recipient), CoreError::WalletSwapAdapter); }
    }
    // Alias privileges are the union inside this exact CPI only, independent of
    // unrelated outer instructions. Raw role order remains in the commitment.
    for i in 0..metas.len() {
        let key = metas[i].pubkey;
        let signer = metas.iter().any(|m| m.pubkey == key && m.is_signer);
        let writable = metas.iter().any(|m| m.pubkey == key && m.is_writable);
        metas[i].is_signer = signer; metas[i].is_writable = writable;
    }
    let payload_digest = payload_digest(router.key(), &metas, data)?;
    require!(payload_digest == quote.payload_digest, CoreError::WalletSwapOutcome);
    // The authenticated protocol recipient is foreign to Principal custody and
    // every venue vault. Normalize its existing WSOL donation before measuring
    // trading fees: native-boundary multi_hop_swap itself calls SyncNative, so
    // otherwise an unrelated old donation would masquerade as a new fee. This
    // never syncs the Principal's source, destination, or dedicated bridge.
    if currency.quote_mint.mint == WSOL && currency.quote_mint.token_program == TOKEN_ID {
        if let Some(before) = buyback_snapshot {
            let rent = before.native_reserve.ok_or_else(|| error!(CoreError::WalletSwapAdapter))?;
            let lamports = buyback.lamports();
            let synced_amount = minus(lamports, rent)?;
            require!(synced_amount >= before.amount, CoreError::WalletSwapAdapter);
            if synced_amount > before.amount {
                invoke(&spl_token::instruction::sync_native(&TOKEN_ID, buyback.key)?,
                    &[buyback.clone(), legacy_token.clone()])?;
                let after = wallet_tokens::read_account(buyback, &currency.quote_mint)?;
                wallet_tokens::account_identity(&before, &after)?;
                require!(after.amount == synced_amount && after.withheld == before.withheld
                    && buyback.lamports() == lamports, CoreError::WalletSwapOutcome);
                buyback_snapshot = Some(after);
            }
        }
    }
    let buyback_before = buyback_snapshot.map_or(buyback.lamports(), |account| account.amount);
    let venue_lamports = infos.iter().map(AccountInfo::lamports).collect();
    Ok(ValidatedRoute { payload_digest, instruction: Instruction { program_id: router.key(), accounts: metas, data: data.to_vec() },
        infos, venue_lamports, wallet: wallet.clone(), source: source.clone(), destination: destination.clone(),
        input_mint: input_mint.clone(), output_mint: output_mint.clone(), legacy_token: legacy_token.clone(),
        system: system.clone(), bridge: bridge.clone(), mint_in, mint_out, source_before, destination_before,
        source_lamports: source.lamports(), destination_lamports: destination.lamports(), wallet_lamports: wallet.lamports(),
        buyback: buyback.clone(), buyback_before, buyback_lamports: buyback.lamports(), buyback_snapshot,
        markets, amount: request.amount, minimum: quote.minimum_net_output, fee_bound: total as u16 })
}

fn token_after(info: &AccountInfo, mint: &MintSnapshot, before: &TokenSnapshot,
    before_lamports: u64) -> Result<TokenSnapshot> {
    let after = wallet_tokens::read_account(info, mint)?;
    wallet_tokens::account_identity(before, &after)?;
    require!(after.withheld == before.withheld, CoreError::WalletSwapOutcome);
    let expected_delta = if before.native_reserve.is_some() {
        i128::from(after.amount) - i128::from(before.amount)
    } else { 0 };
    require!(i128::from(info.lamports()) - i128::from(before_lamports) == expected_delta,
        CoreError::WalletSwapOutcome);
    Ok(after)
}

struct BridgeSnapshot {
    token: TokenSnapshot,
    lamports: u64,
    data_hash: [u8; 32],
}

impl<'info> ValidatedRoute<'info> {
    #[inline(never)]
    fn bridge_snapshot(&self, mint: &MintSnapshot) -> Result<BridgeSnapshot> {
        require!(mint.mint == WSOL && mint.token_program == TOKEN_ID, CoreError::WalletSwapAdapter);
        let (expected, bump) = Pubkey::find_program_address(&[BRIDGE_SEED, self.wallet.key.as_ref()], &CORE_ID);
        require!(self.bridge.key() == expected && self.bridge.is_writable, CoreError::WalletSwapAdapter);
        let _ = bump;
        let token = wallet_tokens::read_account(&self.bridge, mint)?;
        let rent = Rent::get()?.minimum_balance(165);
        require!(token.owner == self.wallet.key() && token.withheld == 0
            && token.data_len == 165 && token.native_reserve == Some(rent)
            && self.bridge.lamports() >= plus(rent, token.amount)?, CoreError::WalletSwapAdapter);
        Ok(BridgeSnapshot { token, lamports: self.bridge.lamports(),
            data_hash: hashv(&[&self.bridge.try_borrow_data()?]).to_bytes() })
    }

    /// Close only the dedicated bridge and reinitialize it with exactly rent.
    /// Preexisting WSOL and unsynced donation lamports remain temporarily in the
    /// wallet until restore_bridge returns both without adopting either into
    /// canonical custody or the Principal's financial books.
    #[inline(never)]
    fn recreate_bridge(&self, mint_info: &AccountInfo<'info>, before: &BridgeSnapshot) -> Result<()> {
        let (_, bump) = Pubkey::find_program_address(&[BRIDGE_SEED, self.wallet.key.as_ref()], &CORE_ID);
        let close = spl_token::instruction::close_account(&TOKEN_ID, self.bridge.key, self.wallet.key,
            self.wallet.key, &[])?;
        invoke(&close, &[self.bridge.clone(), self.wallet.clone(), self.legacy_token.clone()])?;
        let rent = before.token.native_reserve.ok_or_else(|| error!(CoreError::WalletSwapAdapter))?;
        let create = system_instruction::create_account(self.wallet.key, self.bridge.key, rent, 165, &TOKEN_ID);
        let bump_seed = [bump];
        invoke_signed(&create, &[self.wallet.clone(), self.bridge.clone(), self.system.clone()],
            &[&[BRIDGE_SEED, self.wallet.key.as_ref(), &bump_seed]])?;
        let initialize = spl_token::instruction::initialize_account3(&TOKEN_ID, self.bridge.key,
            &WSOL, self.wallet.key)?;
        invoke(&initialize, &[self.bridge.clone(), mint_info.clone(), self.legacy_token.clone()])?;
        Ok(())
    }

    #[inline(never)]
    fn restore_bridge(&self, mint: &MintSnapshot, before: &BridgeSnapshot) -> Result<()> {
        let rent = before.token.native_reserve.ok_or_else(|| error!(CoreError::WalletSwapAdapter))?;
        let unsolicited_lamports = minus(before.lamports, plus(rent, before.token.amount)?)?;
        // Anyone can donate WSOL to this account or call SyncNative. Preserve
        // its original spendable inventory as well as unsynced donations, so
        // neither form of unsolicited funding can permanently disable routes.
        if before.token.amount > 0 {
            invoke(&system_instruction::transfer(self.wallet.key, self.bridge.key, before.token.amount),
                &[self.wallet.clone(), self.bridge.clone(), self.system.clone()])?;
            invoke(&spl_token::instruction::sync_native(&TOKEN_ID, self.bridge.key)?,
                &[self.bridge.clone(), self.legacy_token.clone()])?;
        }
        if unsolicited_lamports > 0 {
            invoke(&system_instruction::transfer(self.wallet.key, self.bridge.key, unsolicited_lamports),
                &[self.wallet.clone(), self.bridge.clone(), self.system.clone()])?;
        }
        require!(wallet_tokens::read_account(&self.bridge, mint)? == before.token
            && self.bridge.lamports() == before.lamports
            && hashv(&[&self.bridge.try_borrow_data()?]).to_bytes() == before.data_hash,
            CoreError::WalletSwapOutcome);
        Ok(())
    }

    /// Unwrap only the exact authorized debit through the dedicated bridge.
    /// Original bridge inventory is restored before the venue executes. The
    /// canonical custody ATA and its protected reserve remain open.
    #[inline(never)]
    fn unwrap_input(&self) -> Result<()> {
        let before = self.bridge_snapshot(&self.mint_in)?;
        wallet_tokens::transfer_checked(&self.source, &self.bridge, &self.wallet, &self.input_mint,
            &self.legacy_token, self.amount, 0, &[])?;
        self.recreate_bridge(&self.input_mint, &before)?;
        self.restore_bridge(&self.mint_in, &before)?;
        require!(self.wallet.lamports() == plus(self.wallet_lamports, self.amount)?, CoreError::WalletSwapOutcome);
        Ok(())
    }

    /// Reinitialize the bridge empty, wrap only the earned native output, and
    /// transfer it into canonical custody before restoring old bridge inventory.
    /// SyncNative never touches a canonical custody ATA: preexisting unsynced
    /// lamport donations there stay unsolicited surplus.
    #[inline(never)]
    fn wrap_output(&self, received: u64) -> Result<()> {
        let before = self.bridge_snapshot(&self.mint_out)?;
        self.recreate_bridge(&self.output_mint, &before)?;
        invoke(&system_instruction::transfer(self.wallet.key, self.bridge.key, received),
            &[self.wallet.clone(), self.bridge.clone(), self.system.clone()])?;
        invoke(&spl_token::instruction::sync_native(&TOKEN_ID, self.bridge.key)?,
            &[self.bridge.clone(), self.legacy_token.clone()])?;
        wallet_tokens::transfer_checked(&self.bridge, &self.destination, &self.wallet, &self.output_mint,
            &self.legacy_token, received, 0, &[])?;
        self.restore_bridge(&self.mint_out, &before)?;
        require!(self.wallet.lamports() == self.wallet_lamports,
            CoreError::WalletSwapOutcome);
        Ok(())
    }

    #[inline(never)]
    pub fn invoke_and_verify(self) -> Result<Box<PumpOutcome>> {
        let count = self.markets.len();
        let native_buy = self.markets[0].buy && self.markets[0].native_quote;
        let native_sell = !self.markets[count - 1].buy && self.markets[count - 1].native_quote;
        if native_buy { self.unwrap_input()?; }
        invoke(&self.instruction, &self.infos)?;
        if native_sell {
            require!(self.mint_out.mint == WSOL && self.mint_out.token_program == TOKEN_ID,
                CoreError::WalletSwapAdapter);
            let received = minus(self.wallet.lamports(), self.wallet_lamports)?;
            require!(received > 0, CoreError::WalletSwapOutcome);
            self.wrap_output(received)?;
        }
        require!(self.wallet.lamports() == self.wallet_lamports && self.wallet.data_is_empty()
            && self.wallet.owner == &system_program::ID, CoreError::WalletSwapOutcome);
        for (account, before) in self.infos.iter().zip(&self.venue_lamports) {
            let key = account.key();
            let measured = key == self.wallet.key() || key == self.source.key()
                || key == self.destination.key() || key == self.buyback.key()
                || self.markets.iter().any(|s| key == s.market.key() || key == s.base.key() || key == s.quote.key());
            if !measured { require!(account.lamports() == *before, CoreError::WalletSwapOutcome); }
        }
        let epoch = Clock::get()?.epoch;
        require!(strict_mint(&self.input_mint, epoch)? == self.mint_in
            && strict_mint(&self.output_mint, epoch)? == self.mint_out, CoreError::FundingCashControl);
        let source_after = token_after(&self.source, &self.mint_in, &self.source_before, self.source_lamports)?;
        let destination_after = token_after(&self.destination, &self.mint_out, &self.destination_before, self.destination_lamports)?;
        let gross_debit = minus(self.source_before.amount, source_after.amount)?;
        let net_received = minus(destination_after.amount, self.destination_before.amount)?;
        require!(gross_debit == self.amount && net_received >= self.minimum, CoreError::WalletSwapOutcome);
        let currency_index = if self.markets[0].buy {0} else {count - 1};
        let currency = &self.markets[currency_index];
        let buyback_after = if let Some(before) = self.buyback_snapshot {
            token_after(&self.buyback, &currency.quote_mint, &before, self.buyback_lamports)?.amount
        } else {
            require!(self.buyback.owner == &system_program::ID && self.buyback.data_is_empty(), CoreError::WalletSwapOutcome);
            self.buyback.lamports()
        };
        let buyback_total = minus(buyback_after, self.buyback_before)?;
        let mut hops = [PumpHopOutcome::default(); MAX_HOPS];
        for (i, step) in self.markets.iter().enumerate() {
            require!(market_identity(&step.market)? == step.identity
                && strict_mint(&step.base_mint_info, epoch)? == step.base_mint
                && strict_mint(&step.quote_mint_info, epoch)? == step.quote_mint, CoreError::WalletSwapOutcome);
            let base_after = token_after(&step.base, &step.base_mint, &step.base_before, step.base_lamports)?;
            let base_amount = if step.buy {minus(step.base_before.amount, base_after.amount)?}
                else {minus(base_after.amount, step.base_before.amount)?};
            let quote_delta = if let Some(before) = step.quote_before {
                require!(step.market.lamports() == step.market_lamports, CoreError::WalletSwapOutcome);
                let after = token_after(&step.quote, &step.quote_mint, &before, step.quote_lamports)?;
                if step.buy {minus(after.amount, before.amount)?} else {minus(before.amount, after.amount)?}
            } else {
                require!(step.quote.lamports() == step.quote_lamports, CoreError::WalletSwapOutcome);
                if step.buy {minus(step.market.lamports(), step.market_lamports)?}
                else {minus(step.market_lamports, step.market.lamports())?}
            };
            let buyback_fee = if i == currency_index {buyback_total} else {0};
            let quote_amount = if step.buy {plus(quote_delta, buyback_fee)?} else {minus(quote_delta, buyback_fee)?};
            let b = step.market.try_borrow_data()?;
            let (protocol, creator) = if step.market.owner == &PUMP_ID {(number(&b,133),number(&b,125))}
                else {(number(&b,271),number(&b,279))};
            let retained_protocol_fee = minus(protocol, step.protocol_before)?;
            let retained_creator_fee = minus(creator, step.creator_before)?;
            require!(base_amount > 0 && quote_amount > 0, CoreError::WalletSwapOutcome);
            hops[i] = PumpHopOutcome { program: *step.market.owner, market: step.market.key(),
                base_mint: step.base_mint.mint, quote_mint: step.quote_mint.mint,
                base_token_program: step.base_mint.token_program, quote_token_program: step.quote_mint.token_program,
                base_amount, quote_amount, retained_protocol_fee, retained_creator_fee, buyback_fee,
                lp_fee_bps_bound: step.fees.lp as u16, buy: step.buy };
        }
        // Every middle asset moves directly between authenticated venue vaults.
        // Endpoint conservation and each adjacent vault delta establish that no
        // unapproved Principal-owned intermediate balance financed the route.
        for i in 0..count {
            let input = if hops[i].buy {hops[i].quote_amount} else {hops[i].base_amount};
            let output = if hops[i].buy {hops[i].base_amount} else {hops[i].quote_amount};
            if i == 0 { require!(input == gross_debit, CoreError::WalletSwapOutcome); }
            if i + 1 == count { require!(output == net_received, CoreError::WalletSwapOutcome); }
            else {
                let next_input = if hops[i+1].buy {hops[i+1].quote_amount} else {hops[i+1].base_amount};
                require!(output == next_input, CoreError::WalletSwapOutcome);
            }
        }
        Ok(Box::new(PumpOutcome { movement: RouteOutcome { gross_debit, net_received,
            input_transfer_fee: 0, output_transfer_fee: 0 }, hop_count: count as u8,
            venue_fee_bps_bound: self.fee_bound, hops }))
    }
}
