//! One economic exchange through Jupiter/Orca and Pump, with transient WSOL.
//! The signed plan explicitly marks the second input as a zero template. Only
//! the observed first-leg net credit supplies it; no existing WSOL is consumed.
use anchor_lang::prelude::*;
use solana_sha256_hasher::hashv;
use dexter_contracts::{minus, plus, CORE_ID};
use dexter_contracts::financing::custody_reserve_address;
use dexter_contracts::wallet_operation::{WalletSpendRequest, WalletRouteQuote,
    JUPITER_ID, TOKEN_ID};
use dexter_contracts::trading::{PUMP_ID, PUMP_AMM_ID};
use crate::{CoreError, wallet_tokens::{self, MintSnapshot, TokenSnapshot},
    jupiter_adapter::{self, RouteOutcome}, pump_adapter::{self, PumpPolicy, PumpOutcome, WSOL}};

const JUPITER: u8 = 1;
const CURVE: u8 = 2;
const AMM: u8 = 3;
const MAX_LEG_ACCOUNTS: usize = 74;
const MAX_PLAN_ACCOUNTS: usize = MAX_LEG_ACCOUNTS + 36;
const MAX_LEG_DATA: usize = 62;
const CURVE_BUY: [u8;8] = [225,247,80,30,213,179,132,136];
const CURVE_SELL: [u8;8] = [28,146,222,119,38,196,105,213];
const AMM_BUY: [u8;8] = [194,171,28,70,104,77,91,47];
const AMM_SELL: [u8;8] = [93,246,130,60,231,233,64,178];
const MULTI: [u8;8] = [43,100,73,19,233,246,111,148];

pub struct MixedOutcome {
    pub movement: RouteOutcome,
    pub pump: Box<PumpOutcome>,
    pub venue_fee_bps_bound: u16,
    pub leg_payload_digests: [[u8;32];2],
    pub leg_amounts: [u64;2],
}
struct Plan<'a> { first_kind: u8, second_kind: u8, first: &'a [u8], second: &'a [u8], first_count: usize }
fn number(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset+8].try_into().unwrap())
}
fn program(kind: u8) -> Result<Pubkey> {
    match kind { JUPITER => Ok(JUPITER_ID), CURVE => Ok(PUMP_ID), AMM => Ok(PUMP_AMM_ID),
        _ => err!(CoreError::WalletSwapAdapter) }
}
fn plan(bytes: &[u8]) -> Result<Plan<'_>> {
    // 1,u8 kind,u16LE length,raw first,u8 kind,u16LE length,raw second,u16LE first role count.
    require!(bytes.len() >= 9 && bytes.len() <= 256 && bytes[0] == 1, CoreError::WalletSwapAdapter);
    let first_kind = bytes[1];
    let first_len = u16::from_le_bytes(bytes[2..4].try_into().unwrap()) as usize;
    let tail = 4usize.checked_add(first_len).ok_or_else(|| error!(CoreError::WalletSwapAdapter))?;
    require!(first_len >= 24 && first_len <= MAX_LEG_DATA && tail + 5 <= bytes.len(), CoreError::WalletSwapAdapter);
    let second_kind = bytes[tail];
    let second_len = u16::from_le_bytes(bytes[tail+1..tail+3].try_into().unwrap()) as usize;
    let end = tail + 3 + second_len;
    require!(second_len >= 24 && second_len <= MAX_LEG_DATA && end + 2 == bytes.len()
        && ((first_kind == JUPITER && matches!(second_kind,CURVE|AMM))
            || (second_kind == JUPITER && matches!(first_kind,CURVE|AMM))), CoreError::WalletSwapAdapter);
    let first_count = u16::from_le_bytes(bytes[end..end+2].try_into().unwrap()) as usize;
    let first = &bytes[4..tail]; let second = &bytes[tail+3..end];
    require!(number(second,8) == 0 && first_count > 0 && first_count <= MAX_LEG_ACCOUNTS,
        CoreError::WalletSwapAdapter);
    Ok(Plan {first_kind,second_kind,first,second,first_count})
}

/// Construct privileges from qualified ABIs, never from effective privileges
/// inherited from the outer transaction. Real adapters authenticate all keys.
fn flags(kind: u8, bytes: &[u8], accounts: &[AccountInfo], out: &mut [[u8;2]]) -> Result<()> {
    require!(out.len() == accounts.len() && out.len() <= MAX_LEG_ACCOUNTS, CoreError::WalletSwapAdapter);
    out.fill([0,0]);
    match kind {
        JUPITER => {
            require!(bytes.len() >= 34 && bytes[..8] == jupiter_adapter::ROUTE_V2 && out.len() >= 10,
                CoreError::WalletSwapAdapter);
            out[0] = [1,0]; out[1] = [0,1]; out[2] = [0,1];
            let count = u32::from_le_bytes(bytes[30..34].try_into().unwrap()) as usize;
            require!(count > 0 && count <= 4, CoreError::WalletSwapAdapter);
            let mut offset = 34; let mut role = 10;
            for _ in 0..count {
                require!(offset + 6 <= bytes.len(), CoreError::WalletSwapAdapter);
                match bytes[offset] {
                    17 => {
                        require!(role+12 <= out.len(), CoreError::WalletSwapAdapter);
                        for item in &mut out[role+3..role+11] { *item = [0,1]; }
                        offset += 6; role += 12;
                    }
                    47 => {
                        require!(offset+7 <= bytes.len() && bytes[offset+2] == 0 && role+16 <= out.len(),
                            CoreError::WalletSwapAdapter);
                        out[role+5] = [0,1];
                        for item in &mut out[role+8..role+16] { *item = [0,1]; }
                        offset += 7; role += 16;
                    }
                    _ => return err!(CoreError::WalletSwapAdapter),
                }
            }
            require!(offset == bytes.len() && role == out.len(), CoreError::WalletSwapAdapter);
        }
        CURVE => {
            require!(out.len() == 17 && ((bytes.len() == 25 && bytes[..8] == CURVE_BUY && bytes[24] == 0)
                || (bytes.len() == 24 && bytes[..8] == CURVE_SELL)), CoreError::WalletSwapAdapter);
            for i in [5,6,7,8,9,10,11,13] {out[i][1] = 1;}
            out[8][0] = 1;
        }
        AMM => {
            require!(bytes.len() == 24, CoreError::WalletSwapAdapter);
            if bytes[..8] == MULTI {
                require!(out.len() >= 26 && out.len() <= 36 && (out.len()-16)%5 == 0,
                    CoreError::WalletSwapAdapter);
                for i in [0,1,2,5,6] {out[i][1] = 1;} out[0][0] = 1;
                for i in 16..out.len() {if (i-16)%5 >= 2 {out[i][1] = 1;}}
            } else {
                require!(out.len() == 17 && (bytes[..8] == AMM_BUY || bytes[..8] == AMM_SELL),
                    CoreError::WalletSwapAdapter);
                for i in [0,1,5,6,7,8,12,14] {out[i][1] = 1;} out[1][0] = 1;
            }
        }
        _ => return err!(CoreError::WalletSwapAdapter),
    }
    union(accounts,out);
    Ok(())
}
fn union(accounts: &[AccountInfo], values: &mut [[u8;2]]) {
    for i in 0..accounts.len() {
        for j in 0..accounts.len() {
            if accounts[i].key == accounts[j].key {
                values[i][0] |= values[j][0]; values[i][1] |= values[j][1];
            }
        }
    }
}
#[inline(never)]
fn digest(program: Pubkey, accounts: &[AccountInfo], values: &[[u8;2]], bytes: &[u8]) -> Result<[u8;32]> {
    require!(accounts.len() == values.len() && accounts.len() <= MAX_PLAN_ACCOUNTS,
        CoreError::WalletSwapAdapter);
    // Metadata only occupies this separate SBF frame. Prefix/length/data slices
    // go directly to the hash syscall; no second route-sized heap allocation.
    let mut encoded = [0u8;34*MAX_PLAN_ACCOUNTS];
    for (i,(account,value)) in accounts.iter().zip(values).enumerate() {
        encoded[i*34..i*34+32].copy_from_slice(account.key.as_ref());
        encoded[i*34+32..i*34+34].copy_from_slice(value);
    }
    Ok(hashv(&[b"DEXTER:WALLET-ROUTE-PAYLOAD:2",CORE_ID.as_ref(),program.as_ref(),
        &(accounts.len() as u32).to_le_bytes(),&encoded[..accounts.len()*34],
        &(bytes.len() as u32).to_le_bytes(),bytes]).to_bytes())
}
fn minimum(kind: u8, data: &[u8]) -> Result<u64> {
    let out = number(data,16);
    if kind == JUPITER {
        require!(data.len() >= 26, CoreError::WalletSwapAdapter);
        let bps = u16::from_le_bytes(data[24..26].try_into().unwrap());
        require!(bps <= 10_000, CoreError::WalletSwapAdapter);
        Ok(((u128::from(out) * u128::from(10_000-bps)) / 10_000) as u64)
    } else {Ok(out)}
}
fn midpoint<'a,'info>(kind:u8, data:&[u8], infos:&'a [AccountInfo<'info>]) -> Result<&'a AccountInfo<'info>> {
    let index = match kind {
        JUPITER => 2,
        CURVE => if data[..8] == CURVE_BUY {9} else {10},
        AMM => if data[..8] == MULTI {2} else if data[..8] == AMM_BUY {5} else {6},
        _ => return err!(CoreError::WalletSwapAdapter),
    };
    infos.get(index).ok_or_else(|| error!(CoreError::WalletSwapAdapter))
}

struct LegOutcome { movement:RouteOutcome, fee_bound:u16, payload:[u8;32], pump:Option<Box<PumpOutcome>> }
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn run_leg<'info>(kind:u8, raw:&[u8], infos:&[AccountInfo<'info>],
    wallet:&AccountInfo<'info>, source:&AccountInfo<'info>, destination:&AccountInfo<'info>,
    mint_in:&AccountInfo<'info>, mint_out:&AccountInfo<'info>,
    program_in:&AccountInfo<'info>, program_out:&AccountInfo<'info>,
    system:&AccountInfo<'info>, legacy:&AccountInfo<'info>, token2022:&AccountInfo<'info>, bridge:&AccountInfo<'info>,
    original:&WalletSpendRequest, outer_quote:&WalletRouteQuote, floor:u64, maximum_fee:u16,
    allowed_intermediates:&[(Pubkey,Pubkey)]) -> Result<LegOutcome> {
    let id = program(kind)?;
    let mut roles = [[0u8;2];MAX_LEG_ACCOUNTS];
    flags(kind,raw,infos,&mut roles[..infos.len()])?;
    let payload = digest(id,infos,&roles[..infos.len()],raw)?;
    let mut request = original.clone();
    request.route_profile = match kind {JUPITER=>2,CURVE=>3,_=>4};
    request.source = source.key();request.source_custody = custody_reserve_address(source.key);
    request.destination = destination.key();request.output_custody = custody_reserve_address(destination.key);
    request.input_mint = mint_in.key();request.output_mint = mint_out.key();
    request.input_token_program = program_in.key();request.output_token_program = program_out.key();
    request.adapter_program = id; request.adapter_pool = if kind == JUPITER {Pubkey::default()} else {original.adapter_pool};
    request.amount = number(raw,8);request.minimum_output = floor;
    request.maximum_input_fee = 0;request.maximum_output_fee = 0;
    request.validate()?;
    let quote = WalletRouteQuote {quoted_input:request.amount,
        quoted_output:if kind==JUPITER {number(raw,16)} else {floor},
        minimum_net_output:floor,expires_at:outer_quote.expires_at,payload_digest:payload};
    quote.validate_economics(&request)?;
    if kind == JUPITER {
        run_jupiter(infos,raw,wallet,source,destination,mint_in,mint_out,program_in,program_out,
            &request,&quote,maximum_fee,payload)
    } else {
        run_pump(infos,raw,wallet,source,destination,mint_in,mint_out,program_in,program_out,
            system,legacy,token2022,bridge,&request,&quote,maximum_fee,allowed_intermediates,payload)
    }
}

#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn run_jupiter<'info>(infos:&[AccountInfo<'info>],raw:&[u8],wallet:&AccountInfo<'info>,
    source:&AccountInfo<'info>,destination:&AccountInfo<'info>,mint_in:&AccountInfo<'info>,mint_out:&AccountInfo<'info>,
    program_in:&AccountInfo<'info>,program_out:&AccountInfo<'info>,request:&WalletSpendRequest,
    quote:&WalletRouteQuote,maximum_fee:u16,payload:[u8;32])->Result<LegOutcome> {
    require!(infos.len() >= 10 && infos[0].key == wallet.key && infos[1].key == source.key
        && infos[2].key == destination.key && infos[3].key == mint_in.key && infos[4].key == mint_out.key
        && infos[5].key == program_in.key && infos[6].key == program_out.key
        && infos[7].key() == JUPITER_ID && infos[8].key() == jupiter_adapter::EVENT_AUTHORITY
        && infos[9].key() == JUPITER_ID,CoreError::WalletSwapAdapter);
    let route=jupiter_adapter::prepare_controlled(&infos[7],&infos[8],wallet,source,destination,
        mint_in,mint_out,program_in,program_out,&infos[10..],raw,request,quote,[0;32],[0;32],maximum_fee)?;
    let fee_bound=route.venue_fee_bps_bound();
    let movement=route.invoke_and_verify()?;
    require!(movement.input_transfer_fee==0 && movement.output_transfer_fee==0,CoreError::WalletSwapOutcome);
    Ok(LegOutcome{movement,fee_bound,payload,pump:None})
}

#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn run_pump<'info>(infos:&[AccountInfo<'info>],raw:&[u8],wallet:&AccountInfo<'info>,
    source:&AccountInfo<'info>,destination:&AccountInfo<'info>,mint_in:&AccountInfo<'info>,mint_out:&AccountInfo<'info>,
    program_in:&AccountInfo<'info>,program_out:&AccountInfo<'info>,system:&AccountInfo<'info>,legacy:&AccountInfo<'info>,
    token2022:&AccountInfo<'info>,bridge:&AccountInfo<'info>,request:&WalletSpendRequest,quote:&WalletRouteQuote,
    maximum_fee:u16,allowed_intermediates:&[(Pubkey,Pubkey)],payload:[u8;32])->Result<LegOutcome> {
    let router=infos.iter().find(|info| info.key()==request.adapter_program && info.executable)
        .ok_or_else(||error!(CoreError::WalletSwapAdapter))?;
    let route=pump_adapter::prepare(router,wallet,source,destination,mint_in,mint_out,program_in,program_out,
        system,legacy,token2022,bridge,infos,raw,request,quote,
        PumpPolicy{maximum_venue_fee_bps:maximum_fee,allowed_intermediates})?;
    let pump=route.invoke_and_verify()?;
    Ok(LegOutcome{movement:pump.movement,fee_bound:pump.venue_fee_bps_bound,payload,pump:Some(pump)})
}

fn strict(info:&AccountInfo,program:Pubkey,epoch:u64) -> Result<MintSnapshot> {
    let value = wallet_tokens::read_mint(info,program,epoch)?;
    require!(value.fee(u64::MAX)? == 0,CoreError::FundingCashControl);Ok(value)
}
fn exact_identity(info:&AccountInfo,mint:&MintSnapshot,before:&TokenSnapshot,lamports:u64) -> Result<()> {
    require!(wallet_tokens::read_account(info,mint)? == *before && info.lamports()==lamports,
        CoreError::WalletSwapOutcome);Ok(())
}
fn lamport_delta(info:&AccountInfo,before:&TokenSnapshot,after:&TokenSnapshot,lamports:u64) -> Result<()> {
    let expected=if before.native_reserve.is_some() {i128::from(after.amount)-i128::from(before.amount)} else {0};
    require!(i128::from(info.lamports())-i128::from(lamports)==expected,CoreError::WalletSwapOutcome);Ok(())
}

#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub fn execute<'info>(wallet:&AccountInfo<'info>,source:&AccountInfo<'info>,destination:&AccountInfo<'info>,
    input_mint:&AccountInfo<'info>,output_mint:&AccountInfo<'info>,
    input_program:&AccountInfo<'info>,output_program:&AccountInfo<'info>,
    system:&AccountInfo<'info>,legacy:&AccountInfo<'info>,token2022:&AccountInfo<'info>,bridge:&AccountInfo<'info>,
    venues:&[AccountInfo<'info>],plan_data:&[u8],request:&WalletSpendRequest,quote:&WalletRouteQuote,
    policy:PumpPolicy<'_>) -> Result<MixedOutcome> {
    require!(request.route_profile == 6 && request.adapter_program == CORE_ID && venues.len() <= MAX_PLAN_ACCOUNTS,
        CoreError::WalletSwapAdapter);
    require!(policy.allowed_intermediates.contains(&(WSOL,TOKEN_ID)),CoreError::WalletSwapAdapter);
    let parsed = plan(plan_data)?;
    require!(parsed.first_count < venues.len() && venues.len()-parsed.first_count <= MAX_LEG_ACCOUNTS
        && number(parsed.first,8)==request.amount,CoreError::WalletSwapAdapter);
    let (first,second) = venues.split_at(parsed.first_count);
    let mut plan_flags = [[0u8;2];MAX_PLAN_ACCOUNTS];
    flags(parsed.first_kind,parsed.first,first,&mut plan_flags[..first.len()])?;
    flags(parsed.second_kind,parsed.second,second,&mut plan_flags[first.len()..venues.len()])?;
    union(venues,&mut plan_flags[..venues.len()]);
    require!(digest(CORE_ID,venues,&plan_flags[..venues.len()],plan_data)? == quote.payload_digest,
        CoreError::WalletSwapOutcome);
    let mid = midpoint(parsed.first_kind,parsed.first,first)?;
    let mid_mint = first.iter().find(|info| info.key()==WSOL)
        .ok_or_else(|| error!(CoreError::WalletSwapAdapter))?;
    let epoch=Clock::get()?.epoch;
    let mint_in = strict(input_mint,input_program.key(),epoch)?;
    let mint_out = strict(output_mint,output_program.key(),epoch)?;
    let mint_mid = strict(mid_mint,TOKEN_ID,epoch)?;
    require!(mid.key()==mint_mid.ata(wallet.key) && mid.key()!=source.key() && mid.key()!=destination.key()
        && input_mint.key()!=WSOL && output_mint.key()!=WSOL,CoreError::WalletSwapAdapter);
    let source_before=wallet_tokens::read_account(source,&mint_in)?;
    let destination_before=wallet_tokens::read_account(destination,&mint_out)?;
    let mid_before=wallet_tokens::read_account(mid,&mint_mid)?;
    require!(mid_before.owner==wallet.key() && mid_before.native_reserve.is_some(),CoreError::WalletSwapAdapter);
    let mid_lamports=mid.lamports();let wallet_lamports=wallet.lamports();
    let source_lamports=source.lamports();let destination_lamports=destination.lamports();
    let mid_bytes_hash=hashv(&[&mid.try_borrow_data()?]).to_bytes();
    let first_floor=minimum(parsed.first_kind,parsed.first)?;
    require!(first_floor>0 && minimum(parsed.second_kind,parsed.second)?>=quote.minimum_net_output,
        CoreError::WalletSwapAdapter);
    // An unrelated outer endpoint is not a route-controlled writable role.
    for (i,info) in first.iter().enumerate() {
        require!(info.key()!=destination.key() || plan_flags[i][1]==0,CoreError::WalletSwapAdapter);
    }
    for (i,info) in second.iter().enumerate() {
        require!(info.key()!=source.key() || plan_flags[first.len()+i][1]==0,CoreError::WalletSwapAdapter);
    }
    let a=run_leg(parsed.first_kind,parsed.first,first,wallet,source,mid,input_mint,mid_mint,input_program,legacy,
        system,legacy,token2022,bridge,request,quote,first_floor,policy.maximum_venue_fee_bps,policy.allowed_intermediates)?;
    let mid_now=wallet_tokens::read_account(mid,&mint_mid)?;
    wallet_tokens::account_identity(&mid_before,&mid_now)?;
    let observed=minus(mid_now.amount,mid_before.amount)?;
    require!(observed>0 && observed==a.movement.net_received && a.movement.gross_debit==request.amount
        && mid_now.withheld==mid_before.withheld
        && mid.lamports()==plus(mid_lamports,observed)?,CoreError::WalletSwapOutcome);
    exact_identity(destination,&mint_out,&destination_before,destination_lamports)?;
    let mut dynamic=[0u8;MAX_LEG_DATA];dynamic[..parsed.second.len()].copy_from_slice(parsed.second);
    dynamic[8..16].copy_from_slice(&observed.to_le_bytes());
    let remaining_fee=policy.maximum_venue_fee_bps.checked_sub(a.fee_bound)
        .ok_or_else(||error!(CoreError::WalletSwapOutcome))?;
    let second_floor=if parsed.second_kind==JUPITER {quote.minimum_net_output} else {minimum(parsed.second_kind,parsed.second)?};
    let b=run_leg(parsed.second_kind,&dynamic[..parsed.second.len()],second,wallet,mid,destination,
        mid_mint,output_mint,legacy,output_program,system,legacy,token2022,bridge,request,quote,
        second_floor,remaining_fee,policy.allowed_intermediates)?;
    exact_identity(mid,&mint_mid,&mid_before,mid_lamports)?;
    require!(hashv(&[&mid.try_borrow_data()?]).to_bytes()==mid_bytes_hash,CoreError::WalletSwapOutcome);
    require!(strict(input_mint,input_program.key(),epoch)?==mint_in
        && strict(output_mint,output_program.key(),epoch)?==mint_out
        && strict(mid_mint,TOKEN_ID,epoch)?==mint_mid && wallet.lamports()==wallet_lamports,
        CoreError::WalletSwapOutcome);
    let final_source=wallet_tokens::read_account(source,&mint_in)?;
    let final_destination=wallet_tokens::read_account(destination,&mint_out)?;
    wallet_tokens::account_identity(&source_before,&final_source)?;
    wallet_tokens::account_identity(&destination_before,&final_destination)?;
    lamport_delta(source,&source_before,&final_source,source_lamports)?;
    lamport_delta(destination,&destination_before,&final_destination,destination_lamports)?;
    let gross_debit=minus(source_before.amount,final_source.amount)?;
    let net_received=minus(final_destination.amount,destination_before.amount)?;
    require!(gross_debit==request.amount && net_received>=quote.minimum_net_output
        && b.movement.gross_debit==observed && b.movement.net_received==net_received
        && final_source.withheld==source_before.withheld && final_destination.withheld==destination_before.withheld,
        CoreError::WalletSwapOutcome);
    let fee_bound=a.fee_bound.checked_add(b.fee_bound).ok_or_else(||error!(CoreError::WalletSwapOutcome))?;
    let payloads=[a.payload,b.payload];
    let pump=a.pump.or(b.pump).ok_or_else(||error!(CoreError::WalletSwapAdapter))?;
    Ok(MixedOutcome {movement:RouteOutcome {gross_debit,net_received,input_transfer_fee:0,output_transfer_fee:0},
        pump,venue_fee_bps_bound:fee_bound,leg_payload_digests:payloads,leg_amounts:[request.amount,observed]})
}
