//! Owner-approved or bounded delegated spending of a Principal's free wallet cash.
//! Core enforces actual effects; a router address or an SDK quote grants no power.
use anchor_lang::prelude::*;
use anchor_lang::solana_program::{instruction::{AccountMeta,Instruction},program::invoke};
use anchor_spl::token::{Mint,Token,TokenAccount};
use dexter_contracts::{minus,plus,CORE_ID,POOL_ID,REGISTRY_CONSUMER_SEED};
use dexter_contracts::spending::{SpendingExecution,SPENDING_SEND,SPENDING_SWAP,
    ROUTE_NONE,ROUTE_SPL_TOKEN_SWAP,ROUTE_JUPITER_V2};
use dexter_contracts::wallet_operation::*;
use dexter_contracts::funded::{CASH_SENT,CASH_RECEIVED};
use principal_registry::{Principal,program::PrincipalRegistry};
use crate::{CoreError,funding_state::*,funding_journal::{inc,journal}};
use crate::wallet_tokens::{self,MintSnapshot,TokenSnapshot};

#[account]
#[derive(InitSpace)]
pub struct WalletSpendReceipt { pub data: WalletSpendReceiptData }

#[derive(AnchorSerialize,AnchorDeserialize,Clone,InitSpace)]
pub struct WalletSendArgs {
    pub request_id:[u8;32],pub amount:u64,pub minimum_output:u64,
    pub maximum_input_fee:u64,pub authority_generation:u64,
    pub destination_authority:Pubkey,
}
#[derive(AnchorSerialize,AnchorDeserialize,Clone,InitSpace)]
pub struct WalletSwapArgs {
    pub request_id:[u8;32],pub amount:u64,pub minimum_output:u64,
    pub maximum_input_fee:u64,pub maximum_output_fee:u64,pub authority_generation:u64,
}

#[derive(AnchorSerialize,AnchorDeserialize,Clone)]
pub struct WalletJupiterArgs {
    pub request_id:[u8;32],pub amount:u64,pub minimum_output:u64,
    pub maximum_input_fee:u64,pub maximum_output_fee:u64,pub authority_generation:u64,
    pub quote:WalletRouteQuote,
    pub route_data:Vec<u8>,
}

#[derive(Accounts)]
#[instruction(args:WalletSendArgs)]
pub struct WalletSend<'info> {
    #[account(mut)] pub executor:Signer<'info>,
    pub wallet_authority:Signer<'info>,
    pub principal:Box<Account<'info,Principal>>,
    /// CHECK: Strict Registry binding reader and canonical custody bind this graph.
    pub binding:UncheckedAccount<'info>,
    /// CHECK: Registry authenticates/consumes this distinct spending permission.
    #[account(mut)] pub permission:UncheckedAccount<'info>,
    #[account(mut)] pub source_custody:Box<Account<'info,FinancierCustody>>,
    /// CHECK: Explicit token-program ownership, admitted extensions and canonical custody below.
    #[account(mut)] pub source:UncheckedAccount<'info>,
    /// CHECK: Strict admitted mint/extension reader below, including selected token program.
    pub input_mint:UncheckedAccount<'info>,
    #[account(init_if_needed,payer=executor,space=8+WalletSpendReceipt::INIT_SPACE,
        seeds=[b"wallet-operation",principal.key().as_ref(),
            &wallet_operation_id(&principal.key(),&args.request_id)],bump)]
    pub operation:Box<Account<'info,WalletSpendReceipt>>,
    #[account(init_if_needed,payer=executor,space=8+FinanceReceipt::INIT_SPACE,
        seeds=[b"finance-operation",principal.key().as_ref(),
            &wallet_leg_id(&wallet_operation_id(&principal.key(),&args.request_id),0)],bump)]
    pub input_receipt:Box<Account<'info,FinanceReceipt>>,
    /// CHECK: Fixed Core consumer signs only the authenticated actual effects.
    #[account(seeds=[REGISTRY_CONSUMER_SEED],bump)] pub registry_authority:UncheckedAccount<'info>,
    pub registry_program:Program<'info,PrincipalRegistry>,
    /// CHECK: New spending requires caught-up projections and an actual Pool CPI.
    #[account(address=POOL_ID,executable)] pub pool_program:UncheckedAccount<'info>,
    /// CHECK: Only the actual legacy Token or Token-2022 executable is admitted below.
    #[account(executable)] pub token_program:UncheckedAccount<'info>,
    pub system_program:Program<'info,System>,
    /// CHECK: Exact approved token-program ATA and admitted token account below.
    #[account(mut)] pub destination:UncheckedAccount<'info>,
}

#[derive(Accounts)]
#[instruction(args:WalletSwapArgs)]
pub struct WalletSwapExactInput<'info> {
    #[account(mut)] pub executor:Signer<'info>,
    pub wallet_authority:Signer<'info>,
    pub principal:Box<Account<'info,Principal>>,
    /// CHECK: Authenticated stable funding wallet enrollment.
    pub binding:UncheckedAccount<'info>,
    /// CHECK: Distinct spending permission consumed through Registry.
    #[account(mut)] pub permission:UncheckedAccount<'info>,
    #[account(mut)] pub source_custody:Box<Account<'info,FinancierCustody>>,
    #[account(mut)] pub source:Box<Account<'info,TokenAccount>>,
    pub input_mint:Box<Account<'info,Mint>>,
    #[account(init_if_needed,payer=executor,space=8+WalletSpendReceipt::INIT_SPACE,
        seeds=[b"wallet-operation",principal.key().as_ref(),
            &wallet_operation_id(&principal.key(),&args.request_id)],bump)]
    pub operation:Box<Account<'info,WalletSpendReceipt>>,
    #[account(init_if_needed,payer=executor,space=8+FinanceReceipt::INIT_SPACE,
        seeds=[b"finance-operation",principal.key().as_ref(),
            &wallet_leg_id(&wallet_operation_id(&principal.key(),&args.request_id),0)],bump)]
    pub input_receipt:Box<Account<'info,FinanceReceipt>>,
    /// CHECK: Fixed Core consumer signer for the permission CPI.
    #[account(seeds=[REGISTRY_CONSUMER_SEED],bump)] pub registry_authority:UncheckedAccount<'info>,
    pub registry_program:Program<'info,PrincipalRegistry>,
    /// CHECK: Mandatory live, caught-up projection path for new wallet spending.
    #[account(address=POOL_ID,executable)] pub pool_program:UncheckedAccount<'info>,
    pub token_program:Program<'info,Token>,
    pub system_program:Program<'info,System>,
    #[account(mut)] pub output_custody:Box<Account<'info,FinancierCustody>>,
    #[account(mut)] pub destination:Box<Account<'info,TokenAccount>>,
    pub output_mint:Box<Account<'info,Mint>>,
    #[account(init_if_needed,payer=executor,space=8+FinanceReceipt::INIT_SPACE,
        seeds=[b"finance-operation",principal.key().as_ref(),
            &wallet_leg_id(&wallet_operation_id(&principal.key(),&args.request_id),1)],bump)]
    pub output_receipt:Box<Account<'info,FinanceReceipt>>,
    /// CHECK: This concrete legacy adapter requires the same legacy Token program.
    #[account(address=anchor_spl::token::ID,executable)] pub output_token_program:UncheckedAccount<'info>,
    /// CHECK: Concrete source-qualified SPL Token Swap adapter; outcomes checked below.
    #[account(address=SPL_TOKEN_SWAP_ID,executable)] pub swap_program:UncheckedAccount<'info>,
    /// CHECK: Exact initialized constant-product pool state/layout/owner checked below.
    pub swap_state:UncheckedAccount<'info>,
    /// CHECK: Canonical pool-state PDA authenticated below.
    pub swap_authority:UncheckedAccount<'info>,
    #[account(mut)] pub pool_source:Box<Account<'info,TokenAccount>>,
    #[account(mut)] pub pool_destination:Box<Account<'info,TokenAccount>>,
    #[account(mut)] pub pool_mint:Box<Account<'info,Mint>>,
    #[account(mut)] pub pool_fee:Box<Account<'info,TokenAccount>>,
}

#[derive(Accounts)]
#[instruction(args:WalletJupiterArgs)]
pub struct WalletSwapJupiter<'info> {
    #[account(mut)] pub executor:Signer<'info>,
    pub wallet_authority:Signer<'info>,
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
    /// CHECK: Current deployed Jupiter identity and selected real instruction profile.
    #[account(address=JUPITER_ID,executable)] pub swap_program:UncheckedAccount<'info>,
    /// CHECK: Canonical Jupiter event authority authenticated by the route adapter.
    pub event_authority:UncheckedAccount<'info>,
}

#[inline(never)]
pub(crate) fn validate_owned_custody<'info>(principal:&Account<'info,Principal>,binding:&UncheckedAccount<'info>,
    authority:&Signer<'info>,custody:&Account<'info,FinancierCustody>,
    token:&AccountInfo<'info>,mint:&MintSnapshot,generation:u64)->Result<TokenSnapshot> {
    valid_principal(principal)?;
    require!(principal.authority_generation==generation && !authority.key().is_on_curve(),CoreError::FundingOwner);
    require!(custody.principal==principal.key() && custody.domain==principal.domain
        && matches!(custody.profile,WALLET_CASH|wallet_tokens::WALLET_TOKENS|wallet_tokens::WALLET_STOCK_TOKENS)
        && custody.binding==binding.key(),CoreError::FundingCustody);
    let graph=principal_registry::read_funding_wallet_binding(&binding.to_account_info())?;
    require!(graph.principal==principal.key()
        && graph.wallet==authority.key() && custody.wallet==authority.key(),CoreError::FundingWalletAuthority);
    wallet_tokens::custody_snapshot(custody,token,mint)
}

pub(crate) struct SpendOutcome {
    pub(crate) amount_out:u64,
    pub(crate) input_fee:u64,
    pub(crate) output_fee:u64,
    pub(crate) gross_output:u64,
    pub(crate) quote:WalletRouteQuote,
}
impl SpendOutcome {
    fn direct(gross:u64,net:u64,fee:u64)->Self {
        Self{amount_out:net,input_fee:fee,output_fee:0,gross_output:gross,quote:WalletRouteQuote::default()}
    }
    fn fee_free_swap(net:u64)->Self {
        Self{amount_out:net,input_fee:0,output_fee:0,gross_output:net,quote:WalletRouteQuote::default()}
    }
}

#[inline(never)]
pub(crate) fn recorded(operation:&Account<WalletSpendReceipt>,request:&WalletSpendRequest)->Result<bool> {
    if operation.data.version==0 {return Ok(false);}
    let data=read_wallet_spend_receipt(&operation.to_account_info())?;
    require!(data.request==*request && data.request_digest==wallet_request_digest(request)?,CoreError::WalletSpendingReplay);
    Ok(true)
}

#[inline(never)]
pub(crate) fn consume<'info>(principal:&Account<'info,Principal>,binding:&UncheckedAccount<'info>,
    permission:&UncheckedAccount<'info>,authority:&UncheckedAccount<'info>,
    program:&Program<'info,PrincipalRegistry>,executor:Pubkey,receipt:Pubkey,
    request:&WalletSpendRequest,outcome:&SpendOutcome)->Result<()> {
    let bump=[Pubkey::find_program_address(&[REGISTRY_CONSUMER_SEED],&CORE_ID).1];
    let seeds:&[&[u8]]=&[REGISTRY_CONSUMER_SEED,&bump];
    let effects=SpendingExecution{binding:request.binding,kind:request.kind,route_profile:request.route_profile,
        mint:request.input_mint,input_token_program:request.input_token_program,
        debit_from:request.source,output_mint:request.output_mint,output_token_program:request.output_token_program,
        destination:request.destination,
        destination_authority:request.destination_authority,adapter_program:request.adapter_program,
        adapter_pool:request.adapter_pool,executor,authority_generation:request.authority_generation,
        request_digest:wallet_request_digest(request)?,receipt,amount:request.amount,
        minimum_output:request.minimum_output,amount_out:outcome.amount_out,
        input_fee:outcome.input_fee,output_fee:outcome.output_fee,gross_output:outcome.gross_output};
    principal_registry::cpi::consume_spending_permission(CpiContext::new_with_signer(program.key(),
        principal_registry::cpi::accounts::ConsumeSpendingPermission{
            core_authority:authority.to_account_info(),principal:principal.to_account_info(),
            binding:binding.to_account_info(),permission:permission.to_account_info(),
        },&[seeds]),effects)
}

#[inline(never)]
pub(crate) fn write_result(operation:&mut Account<WalletSpendReceipt>,request:WalletSpendRequest,
    executor:Pubkey,permission:Pubkey,outcome:SpendOutcome,input:&Account<FinanceReceipt>,
    output:Option<&Account<FinanceReceipt>>)->Result<()> {
    let request_digest=wallet_request_digest(&request)?;
    // The canonical helper returns zero only for the absent/default quote.
    // Every quote-bearing profile receives the same authenticated commitment.
    let quote_digest=wallet_quote_digest(&request_digest,&outcome.quote)?;
    let mut data=WalletSpendReceiptData{version:WALLET_SPEND_VERSION,
        request_digest,request,executor,permission,amount_out:outcome.amount_out,
        input_fee:outcome.input_fee,output_fee:outcome.output_fee,gross_output:outcome.gross_output,
        quote:outcome.quote,quote_digest,
        input_receipt:input.key(),input_effects_hash:input.data.effects_hash,
        output_receipt:output.map(|x|x.key()).unwrap_or_default(),
        output_effects_hash:output.map(|x|x.data.effects_hash).unwrap_or([0;32]),receipt_hash:[0;32]};
    data.receipt_hash=wallet_receipt_hash(&data)?;operation.data=data;Ok(())
}

#[inline(never)]
fn leg_data(request:&WalletSpendRequest,operation:Pubkey,leg:u8,amount:u64)->Result<dexter_contracts::funded::FinanceReceiptData> {
    let id=wallet_operation_id(&request.principal,&request.request_id);
    let kind=if request.kind==SPENDING_SEND {FINANCE_WALLET_SEND}
        else if leg==0 {FINANCE_WALLET_SWAP_INPUT}else{FINANCE_WALLET_SWAP_OUTPUT};
    let mint=if leg==0 {request.input_mint}else{request.output_mint};
    let mut data=record_base(request.principal,wallet_leg_id(&id,leg),
        wallet_leg_digest(&wallet_request_digest(request)?,leg)?,kind,mint,Pubkey::default(),Pubkey::default(),amount);
    data.related_receipt=operation;
    if leg==0 {data.debit_principal=request.principal;data.debit_authority=request.wallet_authority;
        data.debit_from=request.source;}
    Ok(data)
}

// Keep each complete financial record in its own SBF frame. Both legs remain
// inside the same instruction/transaction; this changes no atomicity boundary.
#[inline(never)]
pub(crate) fn journal_leg<'info>(receipt:&mut Account<'info,FinanceReceipt>,request:&WalletSpendRequest,
    operation:Pubkey,leg:u8,amount:u64,books:&[AccountInfo<'info>],pool:&AccountInfo<'info>)->Result<()> {
    let data=leg_data(request,operation,leg,amount)?;
    let field=if leg==0 {CASH_SENT}else{CASH_RECEIVED};
    journal(receipt,data,&[inc(request.principal,field,amount)],books,pool,false,true)
}

pub fn send<'info>(ctx:Context<'info,WalletSend<'info>>,args:WalletSendArgs)->Result<()> {
    let a=ctx.accounts;
    let request=WalletSpendRequest{request_id:args.request_id,kind:SPENDING_SEND,route_profile:ROUTE_NONE,
        principal:a.principal.key(),
        binding:a.binding.key(),wallet_authority:a.wallet_authority.key(),source_custody:a.source_custody.key(),
        source:a.source.key(),input_mint:a.input_mint.key(),input_token_program:a.token_program.key(),
        destination:a.destination.key(),
        destination_authority:args.destination_authority,output_mint:a.input_mint.key(),output_custody:Pubkey::default(),
        output_token_program:a.token_program.key(),adapter_program:Pubkey::default(),adapter_pool:Pubkey::default(),
        amount:args.amount,minimum_output:args.minimum_output,maximum_input_fee:args.maximum_input_fee,maximum_output_fee:0,
        authority_generation:args.authority_generation};
    if recorded(&a.operation,&request)? {return Ok(());}
    request.validate()?;
    let mint=wallet_tokens::read_mint(&a.input_mint.to_account_info(),a.token_program.key(),Clock::get()?.epoch)?;
    let before=
    validate_owned_custody(&a.principal,&a.binding,&a.wallet_authority,&a.source_custody,
        &a.source.to_account_info(),&mint,args.authority_generation)?;
    let destination=wallet_tokens::read_account(&a.destination.to_account_info(),&mint)?;
    require!(destination.owner==args.destination_authority && destination.mint==a.input_mint.key(),
        CoreError::FundingTokenRouting);
    let fee=mint.fee(args.amount)?;
    require!(fee<=args.maximum_input_fee && mint.net(args.amount)?>=args.minimum_output,CoreError::WalletSwapOutcome);
    a.source_custody.debit_uncommitted(before.amount,args.amount)?;
    let moved=wallet_tokens::transfer_checked(&a.source.to_account_info(),&a.destination.to_account_info(),
        &a.wallet_authority.to_account_info(),&a.input_mint.to_account_info(),&a.token_program.to_account_info(),
        args.amount,fee,&[])?;
    let after=wallet_tokens::read_account(&a.source.to_account_info(),&mint)?;
    a.source_custody.check(after.amount)?;
    let outcome=SpendOutcome::direct(moved.gross_debit,moved.net_credit,moved.withheld_fee);
    consume(&a.principal,&a.binding,&a.permission,&a.registry_authority,&a.registry_program,
        a.executor.key(),a.operation.key(),&request,&outcome)?;
    journal_leg(&mut a.input_receipt,&request,a.operation.key(),0,args.amount,
        ctx.remaining_accounts,&a.pool_program.to_account_info())?;
    write_result(&mut a.operation,request,a.executor.key(),a.permission.key(),outcome,&a.input_receipt,None)
}

#[inline(never)]
fn validate_pool(a:&WalletSwapExactInput)->Result<()> {
    require_keys_eq!(*a.swap_state.owner,SPL_TOKEN_SWAP_ID,CoreError::WalletSwapAdapter);
    require!(!a.swap_state.executable && a.swap_state.data_len()==324,CoreError::WalletSwapAdapter);
    let state=a.swap_state.try_borrow_data()?;
    let key=|offset:usize| Pubkey::new_from_array(state[offset..offset+32].try_into().unwrap());
    let (authority,bump)=Pubkey::find_program_address(&[a.swap_state.key().as_ref()],&SPL_TOKEN_SWAP_ID);
    require!(state[0]==1 && state[1]==1 && state[2]==bump && state[291]==0
        && key(3)==a.token_program.key() && authority==a.swap_authority.key(),CoreError::WalletSwapAdapter);
    let forward=key(131)==a.input_mint.key() && key(163)==a.output_mint.key();
    let reverse=key(163)==a.input_mint.key() && key(131)==a.output_mint.key();
    require!(forward || reverse,CoreError::WalletSwapAdapter);
    let (source,destination)=if forward {(key(35),key(67))}else{(key(67),key(35))};
    require!(source==a.pool_source.key() && destination==a.pool_destination.key()
        && key(99)==a.pool_mint.key() && key(195)==a.pool_fee.key(),CoreError::WalletSwapAdapter);
    require!(a.pool_source.owner==authority && a.pool_destination.owner==authority
        && a.pool_source.mint==a.input_mint.key() && a.pool_destination.mint==a.output_mint.key()
        && a.pool_fee.mint==a.pool_mint.key()
        && a.pool_mint.mint_authority==anchor_lang::solana_program::program_option::COption::Some(authority)
        && a.pool_mint.freeze_authority.is_none(),CoreError::WalletSwapAdapter);
    let keys=[a.source.key(),a.destination.key(),a.pool_source.key(),a.pool_destination.key(),
        a.pool_mint.key(),a.pool_fee.key(),a.input_mint.key(),a.output_mint.key()];
    for i in 0..keys.len() {require!(!keys[..i].contains(&keys[i]),CoreError::WalletSwapAdapter);}
    require_keys_neq!(authority,a.wallet_authority.key(),CoreError::WalletSwapAdapter);
    admit_cash(&a.pool_source)?;admit_cash(&a.pool_destination)?;admit_cash(&a.pool_fee)?;
    Ok(())
}

#[inline(never)]
fn invoke_swap<'info>(a:&WalletSwapExactInput<'info>,args:&WalletSwapArgs)->Result<()> {
    let mut data=Vec::with_capacity(17);data.push(1);data.extend_from_slice(&args.amount.to_le_bytes());
    data.extend_from_slice(&args.minimum_output.to_le_bytes());
    let ix=Instruction{program_id:SPL_TOKEN_SWAP_ID,data,accounts:vec![
        AccountMeta::new_readonly(a.swap_state.key(),false),AccountMeta::new_readonly(a.swap_authority.key(),false),
        AccountMeta::new_readonly(a.wallet_authority.key(),true),AccountMeta::new(a.source.key(),false),
        AccountMeta::new(a.pool_source.key(),false),AccountMeta::new(a.pool_destination.key(),false),
        AccountMeta::new(a.destination.key(),false),AccountMeta::new(a.pool_mint.key(),false),
        AccountMeta::new(a.pool_fee.key(),false),AccountMeta::new_readonly(a.input_mint.key(),false),
        AccountMeta::new_readonly(a.output_mint.key(),false),AccountMeta::new_readonly(a.token_program.key(),false),
        AccountMeta::new_readonly(a.token_program.key(),false),AccountMeta::new_readonly(a.token_program.key(),false),
    ]};
    invoke(&ix,&[a.swap_state.to_account_info(),a.swap_authority.to_account_info(),a.wallet_authority.to_account_info(),
        a.source.to_account_info(),a.pool_source.to_account_info(),a.pool_destination.to_account_info(),
        a.destination.to_account_info(),a.pool_mint.to_account_info(),a.pool_fee.to_account_info(),
        a.input_mint.to_account_info(),a.output_mint.to_account_info(),a.token_program.to_account_info(),
        a.token_program.to_account_info(),a.token_program.to_account_info(),a.swap_program.to_account_info()])?;
    Ok(())
}

pub fn swap_exact_input<'info>(ctx:Context<'info,WalletSwapExactInput<'info>>,args:WalletSwapArgs)->Result<()> {
    let a=ctx.accounts;
    let request=WalletSpendRequest{request_id:args.request_id,kind:SPENDING_SWAP,route_profile:ROUTE_SPL_TOKEN_SWAP,
        principal:a.principal.key(),
        binding:a.binding.key(),wallet_authority:a.wallet_authority.key(),source_custody:a.source_custody.key(),
        source:a.source.key(),input_mint:a.input_mint.key(),input_token_program:a.token_program.key(),destination:a.destination.key(),
        destination_authority:a.wallet_authority.key(),output_mint:a.output_mint.key(),output_custody:a.output_custody.key(),
        output_token_program:a.output_token_program.key(),adapter_program:a.swap_program.key(),adapter_pool:a.swap_state.key(),amount:args.amount,
        minimum_output:args.minimum_output,maximum_input_fee:args.maximum_input_fee,maximum_output_fee:args.maximum_output_fee,
        authority_generation:args.authority_generation};
    if recorded(&a.operation,&request)? {return Ok(());}
    request.validate()?;
    let input_mint=wallet_tokens::read_mint(&a.input_mint.to_account_info(),a.token_program.key(),Clock::get()?.epoch)?;
    let output_mint=wallet_tokens::read_mint(&a.output_mint.to_account_info(),a.output_token_program.key(),Clock::get()?.epoch)?;
    validate_owned_custody(&a.principal,&a.binding,&a.wallet_authority,&a.source_custody,&a.source.to_account_info(),
        &input_mint,args.authority_generation)?;
    validate_owned_custody(&a.principal,&a.binding,&a.wallet_authority,&a.output_custody,&a.destination.to_account_info(),
        &output_mint,args.authority_generation)?;
    require_keys_neq!(a.source_custody.key(),a.output_custody.key(),CoreError::FundingCustody);
    validate_pool(a)?;
    let source_before=a.source.amount;let output_before=a.destination.amount;
    let pool_source_before=a.pool_source.amount;let pool_output_before=a.pool_destination.amount;
    a.source_custody.debit_uncommitted(source_before,args.amount)?;
    invoke_swap(a,&args)?;
    a.source.reload()?;a.destination.reload()?;a.pool_source.reload()?;a.pool_destination.reload()?;
    admit_cash(&a.source)?;admit_cash(&a.destination)?;
    require!(a.source.owner==request.wallet_authority && a.source.mint==request.input_mint
        && a.destination.owner==request.wallet_authority && a.destination.mint==request.output_mint,
        CoreError::WalletSwapOutcome);
    let amount_out=minus(a.destination.amount,output_before)?;
    require!(a.source.amount==minus(source_before,args.amount)? && amount_out>=args.minimum_output
        && a.pool_source.amount==plus(pool_source_before,args.amount)?
        && a.pool_destination.amount==minus(pool_output_before,amount_out)?,CoreError::WalletSwapOutcome);
    a.source_custody.check(a.source.amount)?;
    a.output_custody.credit(amount_out)?;a.output_custody.check(a.destination.amount)?;
    let outcome=SpendOutcome::fee_free_swap(amount_out);
    consume(&a.principal,&a.binding,&a.permission,&a.registry_authority,&a.registry_program,
        a.executor.key(),a.operation.key(),&request,&outcome)?;
    require!(ctx.remaining_accounts.len()==4,CoreError::FundingBookAccounts);
    journal_leg(&mut a.input_receipt,&request,a.operation.key(),0,args.amount,
        &ctx.remaining_accounts[..2],&a.pool_program.to_account_info())?;
    journal_leg(&mut a.output_receipt,&request,a.operation.key(),1,amount_out,
        &ctx.remaining_accounts[2..],&a.pool_program.to_account_info())?;
    write_result(&mut a.operation,request,a.executor.key(),a.permission.key(),outcome,
        &a.input_receipt,Some(&a.output_receipt))
}

/// One atomic permission/custody/router/book operation. Quotes are replaceable
/// proposals within immutable economics; they never create spending authority.
pub fn swap_jupiter<'info>(ctx:Context<'info,WalletSwapJupiter<'info>>,args:WalletJupiterArgs)->Result<()> {
    let a=ctx.accounts;
    let request=WalletSpendRequest{request_id:args.request_id,kind:SPENDING_SWAP,route_profile:ROUTE_JUPITER_V2,
        principal:a.principal.key(),binding:a.binding.key(),wallet_authority:a.wallet_authority.key(),
        source_custody:a.source_custody.key(),source:a.source.key(),input_mint:a.input_mint.key(),
        input_token_program:a.token_program.key(),destination:a.destination.key(),
        destination_authority:a.wallet_authority.key(),output_mint:a.output_mint.key(),
        output_token_program:a.output_token_program.key(),output_custody:a.output_custody.key(),
        adapter_program:a.swap_program.key(),adapter_pool:Pubkey::default(),amount:args.amount,
        minimum_output:args.minimum_output,maximum_input_fee:args.maximum_input_fee,
        maximum_output_fee:args.maximum_output_fee,authority_generation:args.authority_generation};
    // Recovery authenticates the original economic result before applying a
    // current quote, clock, authority generation, mint controls or liquidity.
    if recorded(&a.operation,&request)? {return Ok(());}
    request.validate()?;
    args.quote.validate_economics(&request)?;
    let clock=Clock::get()?;
    require!(clock.unix_timestamp<args.quote.expires_at,CoreError::WalletSwapOutcome);
    let input_mint=wallet_tokens::read_mint(&a.input_mint.to_account_info(),a.token_program.key(),clock.epoch)?;
    let output_mint=wallet_tokens::read_mint(&a.output_mint.to_account_info(),a.output_token_program.key(),clock.epoch)?;
    let source_before=validate_owned_custody(&a.principal,&a.binding,&a.wallet_authority,
        &a.source_custody,&a.source.to_account_info(),&input_mint,args.authority_generation)?;
    let destination_before=validate_owned_custody(&a.principal,&a.binding,&a.wallet_authority,
        &a.output_custody,&a.destination.to_account_info(),&output_mint,args.authority_generation)?;
    require_keys_neq!(a.source_custody.key(),a.output_custody.key(),CoreError::FundingCustody);
    require!(ctx.remaining_accounts.len()>4,CoreError::FundingBookAccounts);
    let split=ctx.remaining_accounts.len()-4;
    let (venues,books)=ctx.remaining_accounts.split_at(split);
    let route=crate::jupiter_adapter::prepare(&a.swap_program.to_account_info(),&a.event_authority.to_account_info(),
        &a.wallet_authority.to_account_info(),&a.source.to_account_info(),&a.destination.to_account_info(),
        &a.input_mint.to_account_info(),&a.output_mint.to_account_info(),&a.token_program.to_account_info(),
        &a.output_token_program.to_account_info(),venues,&args.route_data,&request,&args.quote)?;
    a.source_custody.debit_uncommitted(source_before.amount,args.amount)?;
    let moved=route.invoke_and_verify()?;
    require!(moved.gross_debit==args.amount && moved.net_received>=args.quote.minimum_net_output
        && moved.input_transfer_fee<=args.maximum_input_fee && moved.output_transfer_fee<=args.maximum_output_fee,
        CoreError::WalletSwapOutcome);
    // An admitted mint may have a fee/configuration authority. The program
    // permits only the already-inspected controls throughout this execution.
    require!(wallet_tokens::read_mint(&a.input_mint.to_account_info(),a.token_program.key(),clock.epoch)?==input_mint
        && wallet_tokens::read_mint(&a.output_mint.to_account_info(),a.output_token_program.key(),clock.epoch)?==output_mint,
        CoreError::FundingCashControl);
    let source_after=wallet_tokens::read_account(&a.source.to_account_info(),&input_mint)?;
    let destination_after=wallet_tokens::read_account(&a.destination.to_account_info(),&output_mint)?;
    wallet_tokens::account_identity(&source_before,&source_after)?;
    wallet_tokens::account_identity(&destination_before,&destination_after)?;
    require!(source_after.amount==minus(source_before.amount,args.amount)?
        && source_after.withheld==source_before.withheld
        && destination_after.amount==plus(destination_before.amount,moved.net_received)?
        && destination_after.withheld==plus(destination_before.withheld,moved.output_transfer_fee)?,
        CoreError::WalletSwapOutcome);
    a.source_custody.check(source_after.amount)?;
    a.output_custody.credit(moved.net_received)?;
    a.output_custody.check(destination_after.amount)?;
    let outcome=SpendOutcome{amount_out:moved.net_received,input_fee:moved.input_transfer_fee,
        output_fee:moved.output_transfer_fee,gross_output:plus(moved.net_received,moved.output_transfer_fee)?,quote:args.quote};
    consume(&a.principal,&a.binding,&a.permission,&a.registry_authority,&a.registry_program,
        a.executor.key(),a.operation.key(),&request,&outcome)?;
    journal_leg(&mut a.input_receipt,&request,a.operation.key(),0,args.amount,&books[..2],&a.pool_program.to_account_info())?;
    journal_leg(&mut a.output_receipt,&request,a.operation.key(),1,outcome.amount_out,&books[2..],&a.pool_program.to_account_info())?;
    write_result(&mut a.operation,request,a.executor.key(),a.permission.key(),outcome,
        &a.input_receipt,Some(&a.output_receipt))
}
