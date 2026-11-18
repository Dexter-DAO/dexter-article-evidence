//! Explicit immutable policy consent and non-economic Pump account setup.
use anchor_lang::prelude::*;
use anchor_lang::solana_program::{instruction::{AccountMeta, Instruction}, program::{invoke, invoke_signed}};
use anchor_spl::token::{self, Token};
use dexter_contracts::{plus, CORE_ID, SCHEMA_VERSION};
use dexter_contracts::trading::*;
use dexter_contracts::wallet_operation::TOKEN_ID;
use principal_registry::Principal;
use solana_sha256_hasher::hashv;
use crate::{CoreError, wallet_tokens};

pub const WSOL: Pubkey = pubkey!("So11111111111111111111111111111111111111112");
pub const NATIVE_BRIDGE_SEED: &[u8] = b"pump-native-bridge";

#[account]
#[derive(InitSpace)]
pub struct WalletTradingPolicy {
    pub version: u16,
    pub bump: u8,
    pub principal: Pubkey,
    pub policy_id: [u8; 32],
    pub owner: Pubkey,
    pub terms_digest: [u8; 32],
    pub terms: WalletTradingPolicyTerms,
}
#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct ApproveWalletTradingPolicyArgs {
    pub policy_id: [u8; 32],
    pub terms: WalletTradingPolicyTerms,
}
#[derive(Accounts)]
#[instruction(args: ApproveWalletTradingPolicyArgs)]
pub struct ApproveWalletTradingPolicy<'info> {
    #[account(mut)] pub owner: Signer<'info>,
    pub principal: Box<Account<'info, Principal>>,
    /// CHECK: Canonical current Registry binding is read below.
    pub binding: UncheckedAccount<'info>,
    #[account(init_if_needed, payer=owner, space=8+WalletTradingPolicy::INIT_SPACE,
        seeds=[b"wallet-trading-policy",principal.key().as_ref(),&args.policy_id],bump)]
    pub policy: Box<Account<'info, WalletTradingPolicy>>,
    pub system_program: Program<'info, System>,
}
pub fn approve(ctx: Context<ApproveWalletTradingPolicy>, args: ApproveWalletTradingPolicyArgs) -> Result<()> {
    let a=ctx.accounts;
    require_keys_eq!(a.binding.key(),args.terms.binding,CoreError::FundingWalletAuthority);
    let digest=trading_policy_digest(&a.principal.key(),&args.policy_id,&a.owner.key(),&args.terms)?;
    if a.policy.version != 0 {
        require!(a.policy.version==TRADING_POLICY_VERSION && a.policy.bump==ctx.bumps.policy
            && a.policy.principal==a.principal.key() && a.policy.policy_id==args.policy_id
            && a.policy.owner==a.owner.key() && a.policy.terms_digest==digest && a.policy.terms==args.terms,
            CoreError::WalletSpendingReplay);
        return Ok(());
    }
    require!(a.principal.version==SCHEMA_VERSION && a.principal.owner==a.owner.key()
        && a.owner.key().is_on_curve() && args.terms.authority_generation==a.principal.authority_generation,
        CoreError::FundingOwner);
    args.terms.validate()?;
    let binding=principal_registry::read_funding_wallet_binding(&a.binding)?;
    require!(binding.principal==a.principal.key() && args.terms.binding==a.binding.key(),CoreError::FundingWalletAuthority);
    a.policy.set_inner(WalletTradingPolicy{version:TRADING_POLICY_VERSION,bump:ctx.bumps.policy,
        principal:a.principal.key(),policy_id:args.policy_id,owner:a.owner.key(),terms_digest:digest,terms:args.terms});
    Ok(())
}
pub fn validate_policy(policy: &Account<WalletTradingPolicy>, principal: &Account<Principal>,
    request: &dexter_contracts::wallet_operation::WalletSpendRequest) -> Result<()> {
    require!(policy.version==TRADING_POLICY_VERSION && policy.principal==principal.key()
        && policy.key()==trading_policy_address(&policy.principal,&policy.policy_id)
        && policy.key()==request.adapter_pool && policy.owner==principal.owner
        && policy.terms.authority_generation==principal.authority_generation
        && policy.terms_digest==trading_policy_digest(&policy.principal,&policy.policy_id,&policy.owner,&policy.terms)?,
        CoreError::WalletSwapAdapter);
    policy.terms.matches_request(request)
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct InitializeWalletPumpContextArgs { pub authority_generation: u64 }
#[derive(Accounts)]
pub struct InitializeWalletPumpContext<'info> {
    #[account(mut)] pub owner: Signer<'info>,
    pub principal: Box<Account<'info, Principal>>,
    /// CHECK: Full current binding graph authenticated by Registry.
    pub binding: UncheckedAccount<'info>,
    /// CHECK: Full current binding graph authenticated by Registry.
    pub wallet_config: UncheckedAccount<'info>,
    /// CHECK: The bound wallet receives no signer authority during setup.
    pub wallet_authority: UncheckedAccount<'info>,
    /// CHECK: Canonical legacy native mint is decoded before initialization.
    #[account(address=WSOL)] pub native_mint: UncheckedAccount<'info>,
    /// CHECK: Canonical native bridge is created or strictly validated below;
    /// any unsolicited existing WSOL inventory remains outside custody/books.
    #[account(mut,seeds=[NATIVE_BRIDGE_SEED,wallet_authority.key().as_ref()],bump)]
    pub native_bridge: UncheckedAccount<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    /// CHECK: Actual deployed Pump identity, used only for fixed accumulator initialization.
    #[account(address=PUMP_ID,executable)] pub pump_program: UncheckedAccount<'info>,
    /// CHECK: Actual deployed PumpSwap identity, used only for fixed accumulator initialization.
    #[account(address=PUMP_AMM_ID,executable)] pub pump_amm_program: UncheckedAccount<'info>,
    /// CHECK: Canonical Pump user accumulator, authenticated before and after fixed CPI.
    #[account(mut)] pub pump_volume: UncheckedAccount<'info>,
    /// CHECK: Canonical PumpSwap user accumulator, authenticated before and after fixed CPI.
    #[account(mut)] pub pump_amm_volume: UncheckedAccount<'info>,
    /// CHECK: Fixed Pump event authority.
    pub pump_event: UncheckedAccount<'info>,
    /// CHECK: Fixed PumpSwap event authority.
    pub pump_amm_event: UncheckedAccount<'info>,
}

fn volume_identity(info:&AccountInfo,program:Pubkey,wallet:Pubkey)->Result<()> {
    require!(info.owner==&program && !info.executable && info.data_len()>=137,CoreError::WalletSwapAdapter);
    let b=info.try_borrow_data()?;
    require!(b[..8]==hashv(&[b"account:UserVolumeAccumulator"]).to_bytes()[..8]
        && b[8..40]==wallet.to_bytes(),CoreError::WalletSwapAdapter);
    Ok(())
}
#[inline(never)]
fn setup_volume<'info>(payer:&AccountInfo<'info>,wallet:&AccountInfo<'info>,volume:&AccountInfo<'info>,
    system:&AccountInfo<'info>,event:&AccountInfo<'info>,program:&AccountInfo<'info>)->Result<()> {
    let expected=Pubkey::find_program_address(&[b"user_volume_accumulator",wallet.key.as_ref()],program.key).0;
    let expected_event=Pubkey::find_program_address(&[b"__event_authority"],program.key).0;
    require!(volume.key()==expected && event.key()==expected_event,CoreError::WalletSwapAdapter);
    if !volume.data_is_empty() {return volume_identity(volume,program.key(),wallet.key());}
    require_keys_eq!(*volume.owner,System::id(),CoreError::WalletSwapAdapter);
    let ix=Instruction{program_id:program.key(),data:vec![94,6,202,115,255,96,232,183],accounts:vec![
        AccountMeta::new(payer.key(),true),AccountMeta::new_readonly(wallet.key(),false),
        AccountMeta::new(volume.key(),false),AccountMeta::new_readonly(system.key(),false),
        AccountMeta::new_readonly(event.key(),false),AccountMeta::new_readonly(program.key(),false)]};
    invoke(&ix,&[payer.clone(),wallet.clone(),volume.clone(),system.clone(),event.clone(),program.clone()])?;
    volume_identity(volume,program.key(),wallet.key())
}

pub fn initialize_pump_context(ctx:Context<InitializeWalletPumpContext>,args:InitializeWalletPumpContextArgs)->Result<()> {
    let a=ctx.accounts;
    require!(a.principal.version==SCHEMA_VERSION && a.owner.key()==a.principal.owner
        && args.authority_generation==a.principal.authority_generation,CoreError::FundingOwner);
    let binding=principal_registry::validate_funding_wallet(&a.binding,&a.wallet_config,&a.wallet_authority)?;
    require!(binding.principal==a.principal.key() && binding.wallet==a.wallet_authority.key(),CoreError::FundingWalletAuthority);
    let mint=wallet_tokens::read_mint(&a.native_mint,TOKEN_ID,Clock::get()?.epoch)?;
    let wallet=a.wallet_authority.key();let bump=[ctx.bumps.native_bridge];
    let seeds:&[&[u8]]=&[NATIVE_BRIDGE_SEED,wallet.as_ref(),&bump];
    let bridge=a.native_bridge.to_account_info();let payer=a.owner.to_account_info();
    let system=a.system_program.to_account_info();let token=a.token_program.to_account_info();
    if bridge.data_is_empty() {
        require_keys_eq!(*bridge.owner,System::id(),CoreError::WalletSwapAdapter);
        let rent=Rent::get()?.minimum_balance(165);let before=bridge.lamports();
        // Preserve unsolicited prefunding as unallocated lamports, never as
        // wrapped spendable tokens. Initialization sees exactly the rent floor.
        if before>rent {
            invoke_signed(&anchor_lang::solana_program::system_instruction::transfer(bridge.key,payer.key,before-rent),
                &[bridge.clone(),payer.clone(),system.clone()],&[seeds])?;
        } else if before<rent {
            invoke(&anchor_lang::solana_program::system_instruction::transfer(payer.key,bridge.key,rent-before),
                &[payer.clone(),bridge.clone(),system.clone()])?;
        }
        invoke_signed(&anchor_lang::solana_program::system_instruction::allocate(bridge.key,165),
            &[bridge.clone(),system.clone()],&[seeds])?;
        invoke_signed(&anchor_lang::solana_program::system_instruction::assign(bridge.key,&TOKEN_ID),
            &[bridge.clone(),system.clone()],&[seeds])?;
        let ix=token::spl_token::instruction::initialize_account3(&TOKEN_ID,bridge.key,a.native_mint.key,&wallet)?;
        invoke(&ix,&[bridge.clone(),a.native_mint.to_account_info(),token.clone()])?;
        if before>rent {
            invoke(&anchor_lang::solana_program::system_instruction::transfer(payer.key,bridge.key,before-rent),
                &[payer.clone(),bridge.clone(),system.clone()])?;
        }
    }
    let state=wallet_tokens::read_account(&bridge,&mint)?;
    let reserve=state.native_reserve.ok_or_else(||error!(CoreError::WalletSwapAdapter))?;
    require!(state.owner==wallet && bridge.lamports()>=plus(reserve,state.amount)?,CoreError::WalletSwapAdapter);
    setup_volume(&payer,&a.wallet_authority,&a.pump_volume,&system,&a.pump_event,&a.pump_program)?;
    setup_volume(&payer,&a.wallet_authority,&a.pump_amm_volume,&system,&a.pump_amm_event,&a.pump_amm_program)
}
