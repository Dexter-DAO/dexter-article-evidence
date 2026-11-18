//! Atomic market execution under explicit immutable owner-approved constraints.
use anchor_lang::prelude::*;
use dexter_contracts::{minus,plus,POOL_ID,REGISTRY_CONSUMER_SEED};
use dexter_contracts::spending::SPENDING_SWAP;
use dexter_contracts::trading::*;
use dexter_contracts::wallet_operation::*;
use principal_registry::{Principal,program::PrincipalRegistry};
use crate::{CoreError,funding_state::*,wallet_spending::*,wallet_tokens};
use crate::wallet_trading_policy::{WalletTradingPolicy,validate_policy};

#[derive(AnchorSerialize,AnchorDeserialize,Clone)]
pub struct WalletMarketArgs {
    pub request_id:[u8;32],pub amount:u64,pub minimum_output:u64,
    pub maximum_input_fee:u64,pub maximum_output_fee:u64,pub authority_generation:u64,
    pub route_profile:u8,pub quote:WalletRouteQuote,pub route_data:Vec<u8>,
}

#[account]
#[derive(InitSpace)]
pub struct WalletTradingDetails { pub data: WalletTradingDetailsData }

#[derive(Accounts)]
#[instruction(args:WalletMarketArgs)]
pub struct WalletSwapMarket<'info> {
    #[account(mut)] pub executor:Signer<'info>,
    #[account(mut)] pub wallet_authority:Signer<'info>,
    pub principal:Box<Account<'info,Principal>>,
    /// CHECK: Strict stable enrollment and current Registry graph authentication.
    pub binding:UncheckedAccount<'info>,
    /// CHECK: Registry alone authenticates and consumes this separate permission.
    #[account(mut)] pub permission:UncheckedAccount<'info>,
    #[account(mut)] pub source_custody:Box<Account<'info,FinancierCustody>>,
    /// CHECK: Exact token program, ATA, admitted extensions and custody below.
    #[account(mut)] pub source:UncheckedAccount<'info>,
    /// CHECK: Explicit token-program and current extension/control reader below.
    pub input_mint:UncheckedAccount<'info>,
    #[account(init_if_needed,payer=executor,space=8+WalletSpendReceipt::INIT_SPACE,
        seeds=[b"wallet-operation",principal.key().as_ref(),
            &wallet_operation_id(&principal.key(),&args.request_id)],bump)]
    pub operation:Box<Account<'info,WalletSpendReceipt>>,
    #[account(init_if_needed,payer=executor,space=8+FinanceReceipt::INIT_SPACE,
        seeds=[b"finance-operation",principal.key().as_ref(),
            &wallet_leg_id(&wallet_operation_id(&principal.key(),&args.request_id),0)],bump)]
    pub input_receipt:Box<Account<'info,FinanceReceipt>>,
    /// CHECK: Fixed Core-to-Registry consumer; no router receives this authority.
    #[account(seeds=[REGISTRY_CONSUMER_SEED],bump)] pub registry_authority:UncheckedAccount<'info>,
    pub registry_program:Program<'info,PrincipalRegistry>,
    /// CHECK: New risk requires complete current Core books and Pool projections.
    #[account(address=POOL_ID,executable)] pub pool_program:UncheckedAccount<'info>,
    /// CHECK: Only the actual original Token or Token-2022 executable is admitted.
    #[account(executable)] pub token_program:UncheckedAccount<'info>,
    pub system_program:Program<'info,System>,
    #[account(mut)] pub output_custody:Box<Account<'info,FinancierCustody>>,
    /// CHECK: Canonical same-Principal receiving custody; net/withheld verified below.
    #[account(mut)] pub destination:UncheckedAccount<'info>,
    /// CHECK: Exact output token-program identity and current controls below.
    pub output_mint:UncheckedAccount<'info>,
    #[account(init_if_needed,payer=executor,space=8+FinanceReceipt::INIT_SPACE,
        seeds=[b"finance-operation",principal.key().as_ref(),
            &wallet_leg_id(&wallet_operation_id(&principal.key(),&args.request_id),1)],bump)]
    pub output_receipt:Box<Account<'info,FinanceReceipt>>,
    /// CHECK: Strict explicit output token-program admission below.
    #[account(executable)] pub output_token_program:UncheckedAccount<'info>,
    /// CHECK: Exact router identity selected by the immutable policy profile.
    #[account(executable)] pub swap_program:UncheckedAccount<'info>,
    pub trading_policy:Box<Account<'info,WalletTradingPolicy>>,
    #[account(init_if_needed,payer=executor,space=8+WalletTradingDetails::INIT_SPACE,
        seeds=[b"wallet-trading-details",operation.key().as_ref()],bump)]
    pub trading_details:Box<Account<'info,WalletTradingDetails>>,
    /// CHECK: Only the canonical dedicated bridge is writable; the Pump adapter
    /// additionally verifies its native-token state and exact restoration.
    #[account(mut,seeds=[b"pump-native-bridge",wallet_authority.key().as_ref()],bump)]
    pub native_bridge:UncheckedAccount<'info>,
    pub legacy_token_program:Program<'info,anchor_spl::token::Token>,
    /// CHECK: Exact second token executable required by Pump's current interface.
    #[account(address=TOKEN_2022_ID,executable)] pub token_2022_program:UncheckedAccount<'info>,
    /// CHECK: Canonical event authority authenticated by the selected venue adapter.
    pub event_authority:UncheckedAccount<'info>,
}


fn details_match(details:&Account<WalletTradingDetails>,operation:&Account<WalletSpendReceipt>)->Result<()> {
    let d=&details.data;let result=&operation.data;
    require!(details.key()==trading_details_address(&operation.key()) && d.version==TRADING_DETAILS_VERSION
        && d.operation==operation.key() && d.request_digest==result.request_digest
        && d.policy==result.request.adapter_pool && d.policy_digest!=[0;32]
        && d.payload_digest==result.quote.payload_digest && d.hop_count<=4
        && d.receipt_hash==trading_details_hash(d)?,CoreError::WalletSpendingReplay);
    Ok(())
}

struct MarketOutcome {
    movement: crate::jupiter_adapter::RouteOutcome,
    venue_fee_bps_bound: u16,
    pump: Option<Box<crate::pump_adapter::PumpOutcome>>,
    leg_payload_digests: [[u8;32];2],
    leg_amounts: [u64;2],
}

// Separate bounded frames keep the whole financial transition in one Core
// instruction while avoiding a route-sized return value in its caller's frame.
#[inline(never)]
fn execute_market<'info>(a:&WalletSwapMarket<'info>,args:&WalletMarketArgs,
    request:&WalletSpendRequest,venues:&[AccountInfo<'info>])->Result<MarketOutcome> {
    let terms=&a.trading_policy.terms;
    if args.route_profile==ROUTE_JUPITER_CONTROLLED {
        return execute_jupiter(a,args,request,venues);
    }
    let mut allowed=[(Pubkey::default(),Pubkey::default());MAX_INTERMEDIATES];
    for (i,item) in allowed.iter_mut().enumerate().take(terms.intermediate_count as usize) {
        *item=(terms.intermediate_mints[i],terms.intermediate_token_programs[i]);
    }
    let policy=crate::pump_adapter::PumpPolicy{maximum_venue_fee_bps:terms.maximum_venue_fee_bps,
        allowed_intermediates:&allowed[..terms.intermediate_count as usize]};
    if args.route_profile==ROUTE_JUPITER_PUMP {
        let result=crate::mixed_route_adapter::execute(&a.wallet_authority.to_account_info(),
            &a.source,&a.destination,&a.input_mint,&a.output_mint,&a.token_program,&a.output_token_program,
            &a.system_program.to_account_info(),&a.legacy_token_program.to_account_info(),&a.token_2022_program,
            &a.native_bridge,venues,&args.route_data,request,&args.quote,policy)?;
        return Ok(MarketOutcome{movement:result.movement,venue_fee_bps_bound:result.venue_fee_bps_bound,
            pump:Some(result.pump),leg_payload_digests:result.leg_payload_digests,leg_amounts:result.leg_amounts});
    }
    execute_pump(a,args,request,venues,policy)
}

#[inline(never)]
fn execute_jupiter<'info>(a:&WalletSwapMarket<'info>,args:&WalletMarketArgs,
    request:&WalletSpendRequest,venues:&[AccountInfo<'info>])->Result<MarketOutcome> {
    let terms=&a.trading_policy.terms;
    let route=crate::jupiter_adapter::prepare_controlled(&a.swap_program,&a.event_authority,
        &a.wallet_authority.to_account_info(),&a.source,&a.destination,&a.input_mint,&a.output_mint,
        &a.token_program,&a.output_token_program,venues,&args.route_data,request,&args.quote,
        terms.input_control_digest,terms.output_control_digest,terms.maximum_venue_fee_bps)?;
    let bound=route.venue_fee_bps_bound();
    Ok(MarketOutcome{movement:route.invoke_and_verify()?,venue_fee_bps_bound:bound,
        pump:None,leg_payload_digests:[[0;32];2],leg_amounts:[0;2]})
}

#[inline(never)]
fn execute_pump<'info>(a:&WalletSwapMarket<'info>,args:&WalletMarketArgs,
    request:&WalletSpendRequest,venues:&[AccountInfo<'info>],
    policy:crate::pump_adapter::PumpPolicy<'_>)->Result<MarketOutcome> {
    let route=crate::pump_adapter::prepare(&a.swap_program,&a.wallet_authority.to_account_info(),
        &a.source,&a.destination,&a.input_mint,&a.output_mint,&a.token_program,&a.output_token_program,
        &a.system_program.to_account_info(),&a.legacy_token_program.to_account_info(),&a.token_2022_program,
        &a.native_bridge,venues,&args.route_data,request,&args.quote,policy)?;
    let result=route.invoke_and_verify()?;
    Ok(MarketOutcome{movement:result.movement,venue_fee_bps_bound:result.venue_fee_bps_bound,
        pump:Some(result),leg_payload_digests:[[0;32];2],leg_amounts:[0;2]})
}

#[inline(never)]
fn write_details(details:&mut Account<WalletTradingDetails>,operation:Pubkey,
    request:&WalletSpendRequest,policy:&Account<WalletTradingPolicy>,
    outcome:&MarketOutcome,quote:&WalletRouteQuote)->Result<()> {
    let mut value=WalletTradingDetailsData{version:TRADING_DETAILS_VERSION,operation,
        request_digest:wallet_request_digest(request)?,policy:policy.key(),policy_digest:policy.terms_digest,
        input_control_digest:policy.terms.input_control_digest,output_control_digest:policy.terms.output_control_digest,
        payload_digest:quote.payload_digest,venue_fee_bps_bound:outcome.venue_fee_bps_bound,
        hop_count:outcome.pump.as_ref().map_or(0,|p|p.hop_count),
        hops:outcome.pump.as_ref().map_or([WalletTradingHop::default();4],|p|p.hops),
        leg_payload_digests:outcome.leg_payload_digests,
        leg_amounts:outcome.leg_amounts,receipt_hash:[0;32]};
    value.receipt_hash=trading_details_hash(&value)?;
    details.data=value;
    Ok(())
}

#[inline(never)]
pub fn swap_market<'info>(ctx:Context<'info,WalletSwapMarket<'info>>,args:WalletMarketArgs)->Result<()> {
    let a=ctx.accounts;
    let request=WalletSpendRequest{request_id:args.request_id,kind:SPENDING_SWAP,route_profile:args.route_profile,
        principal:a.principal.key(),binding:a.binding.key(),wallet_authority:a.wallet_authority.key(),
        source_custody:a.source_custody.key(),source:a.source.key(),input_mint:a.input_mint.key(),
        input_token_program:a.token_program.key(),destination:a.destination.key(),
        destination_authority:a.wallet_authority.key(),output_mint:a.output_mint.key(),
        output_token_program:a.output_token_program.key(),output_custody:a.output_custody.key(),
        adapter_program:a.swap_program.key(),adapter_pool:a.trading_policy.key(),amount:args.amount,
        minimum_output:args.minimum_output,maximum_input_fee:args.maximum_input_fee,
        maximum_output_fee:args.maximum_output_fee,authority_generation:args.authority_generation};
    // Immutable success wins before mutable controls, permission, owner, policy
    // generation, quote expiry, native bridge readiness or liquidity are read.
    if recorded(&a.operation,&request)? {return details_match(&a.trading_details,&a.operation);}
    require!(a.trading_details.data.version==0,CoreError::WalletSpendingReplay);
    request.validate()?;args.quote.validate_economics(&request)?;
    validate_policy(&a.trading_policy,&a.principal,&request)?;
    let terms=&a.trading_policy.terms;let clock=Clock::get()?;
    require!(clock.unix_timestamp<args.quote.expires_at,CoreError::WalletSwapOutcome);
    let input_mint=wallet_tokens::read_mint_with_controls(&a.input_mint,a.token_program.key(),clock.epoch,terms.input_control_digest)?;
    let output_mint=wallet_tokens::read_mint_with_controls(&a.output_mint,a.output_token_program.key(),clock.epoch,terms.output_control_digest)?;
    let source_before=validate_owned_custody(&a.principal,&a.binding,&a.wallet_authority,&a.source_custody,
        &a.source,&input_mint,args.authority_generation)?;
    let destination_before=validate_owned_custody(&a.principal,&a.binding,&a.wallet_authority,&a.output_custody,
        &a.destination,&output_mint,args.authority_generation)?;
    require_keys_neq!(a.source_custody.key(),a.output_custody.key(),CoreError::FundingCustody);
    require!(ctx.remaining_accounts.len()>4,CoreError::FundingBookAccounts);
    let (venues,books)=ctx.remaining_accounts.split_at(ctx.remaining_accounts.len()-4);
    a.source_custody.debit_uncommitted(source_before.amount,args.amount)?;
    let market=execute_market(a,&args,&request,venues)?;
    let moved=market.movement;
    require!(moved.gross_debit==args.amount && moved.net_received>=args.quote.minimum_net_output
        && moved.input_transfer_fee<=args.maximum_input_fee && moved.output_transfer_fee<=args.maximum_output_fee,
        CoreError::WalletSwapOutcome);
    require!(wallet_tokens::read_mint_with_controls(&a.input_mint,a.token_program.key(),clock.epoch,terms.input_control_digest)?==input_mint
        && wallet_tokens::read_mint_with_controls(&a.output_mint,a.output_token_program.key(),clock.epoch,terms.output_control_digest)?==output_mint,
        CoreError::FundingCashControl);
    let source_after=wallet_tokens::read_account(&a.source,&input_mint)?;
    let destination_after=wallet_tokens::read_account(&a.destination,&output_mint)?;
    wallet_tokens::account_identity(&source_before,&source_after)?;
    wallet_tokens::account_identity(&destination_before,&destination_after)?;
    require!(source_after.amount==minus(source_before.amount,args.amount)? && source_after.withheld==source_before.withheld
        && destination_after.amount==plus(destination_before.amount,moved.net_received)?
        && destination_after.withheld==plus(destination_before.withheld,moved.output_transfer_fee)?,CoreError::WalletSwapOutcome);
    a.source_custody.check(source_after.amount)?;a.output_custody.credit(moved.net_received)?;
    a.output_custody.check(destination_after.amount)?;
    let outcome=SpendOutcome{amount_out:moved.net_received,input_fee:moved.input_transfer_fee,
        output_fee:moved.output_transfer_fee,gross_output:plus(moved.net_received,moved.output_transfer_fee)?,quote:args.quote};
    consume(&a.principal,&a.binding,&a.permission,&a.registry_authority,&a.registry_program,
        a.executor.key(),a.operation.key(),&request,&outcome)?;
    journal_leg(&mut a.input_receipt,&request,a.operation.key(),0,args.amount,&books[..2],&a.pool_program)?;
    journal_leg(&mut a.output_receipt,&request,a.operation.key(),1,outcome.amount_out,&books[2..],&a.pool_program)?;
    write_details(&mut a.trading_details,a.operation.key(),&request,&a.trading_policy,&market,&outcome.quote)?;
    write_result(&mut a.operation,request,a.executor.key(),a.permission.key(),outcome,
        &a.input_receipt,Some(&a.output_receipt))
}
