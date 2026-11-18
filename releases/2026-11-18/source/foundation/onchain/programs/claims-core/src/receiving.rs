//! Opt-in supplier cash custody with an address that survives owner recovery.
//!
//! This is a distinct receiving account, not a financier balance or a source of
//! reserved backing. Collections may credit its canonical ATA without changing
//! the accepted supplier route. Only a fresh exact current-owner instruction
//! can debit it; recovery changes that authority, never the account or assets.
//! No instruction closes the custody/receipt, resets history, sets an SPL
//! delegate, changes token authority, or admits this PDA as a financier graph.
use anchor_lang::prelude::*;
use anchor_lang::solana_program::instruction::{get_stack_height, TRANSACTION_LEVEL_STACK_HEIGHT};
use anchor_spl::{associated_token::{self, AssociatedToken}, token::{self, Mint, Token, TokenAccount}};
use dexter_contracts::CORE_ID;
use principal_registry::Principal;
use solana_sha256_hasher::hashv;

use crate::{CoreError, funding_state::{transfer_checked_exact, valid_principal}};

pub const RECEIVING_VERSION: u16 = 1;
pub const RECEIVING_OPEN: u8 = 1;
pub const RECEIVING_WITHDRAW: u8 = 2;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, InitSpace)]
pub struct OpenReceivingCustodyArgs {
    pub operation_id: [u8; 32],
    pub authority_generation: u64,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, InitSpace)]
pub struct WithdrawReceivingTokensArgs {
    pub operation_id: [u8; 32],
    pub authority_generation: u64,
    pub amount: u64,
    /// The exact canonical classic-SPL ATA of this external native key receives
    /// the withdrawal. The key need not sign; the current owner approves it.
    pub destination_authority: Pubkey,
}

#[account]
#[derive(InitSpace)]
pub struct ReceivingCustody {
    pub version: u16,
    pub bump: u8,
    pub domain: [u8; 32],
    pub principal: Pubkey,
    pub mint: Pubkey,
    pub token_program: Pubkey,
    /// This custody PDA itself is the permanent token authority.
    pub wallet: Pubkey,
    pub token_account: Pubkey,
    pub opened_by: Pubkey,
    pub opened_generation: u64,
    pub opening_receipt: Pubkey,
    /// Lifetime cash withdrawals, independent of bill debt or finance books.
    pub total_withdrawn: u128,
}

impl ReceivingCustody {
    /// Check the immutable custody/asset/address binding without consulting the
    /// present owner. Accepted routes and historical receipts survive recovery.
    pub fn check(&self, key: Pubkey) -> Result<()> {
        let (expected, bump) = receiving_address(&self.domain, &self.principal, &self.mint);
        require!(self.version == RECEIVING_VERSION && self.bump == bump && key == expected
            && self.domain != [0; 32] && self.principal != Pubkey::default()
            && self.mint != Pubkey::default() && self.token_program == token::ID
            && self.wallet == key
            && self.token_account == associated_token::get_associated_token_address(&key, &self.mint)
            // Opening authenticates either the native owner or its canonical
            // immutable Registry passkey alias. Historical custody validation
            // does not require that former credential to remain current.
            && self.opened_by != Pubkey::default() && self.opened_generation > 0
            && self.opening_receipt != Pubkey::default(), CoreError::ReceivingCustody);
        Ok(())
    }
}

/// The request identifies the effect; observed balances record its actual token
/// movement. A historical exact replay uses this immutable receipt and does not
/// reread current balances, revive an old signature, or transfer tokens again.
#[account]
#[derive(InitSpace)]
pub struct ReceivingReceipt {
    pub version: u16,
    pub bump: u8,
    pub action: u8,
    pub domain: [u8; 32],
    pub principal: Pubkey,
    pub custody: Pubkey,
    pub operation_id: [u8; 32],
    pub request_digest: [u8; 32],
    pub authority: Pubkey,
    pub authority_generation: u64,
    pub mint: Pubkey,
    pub token_program: Pubkey,
    pub source: Pubkey,
    pub destination: Pubkey,
    pub destination_authority: Pubkey,
    pub amount: u64,
    pub source_before: u64,
    pub source_after: u64,
    pub destination_before: u64,
    pub destination_after: u64,
    pub total_withdrawn_after: u128,
    pub completed_at: i64,
}

pub fn receiving_address(domain: &[u8; 32], principal: &Pubkey, mint: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"receiving-custody", domain, principal.as_ref(), mint.as_ref()], &CORE_ID)
}

pub fn receiving_receipt_address(custody: &Pubkey, operation_id: &[u8; 32]) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"receiving-operation", custody.as_ref(), operation_id], &CORE_ID)
}

#[derive(Accounts)]
#[instruction(args: OpenReceivingCustodyArgs)]
pub struct OpenReceivingCustody<'info> {
    #[account(mut)] pub payer: Signer<'info>,
    pub principal: Box<Account<'info, Principal>>,
    /// CHECK: Fresh execution authenticates the native current-owner signer or
    /// its exact Registry passkey prefix. Historical replay needs no lost key.
    pub authority: UncheckedAccount<'info>,
    pub mint: Box<Account<'info, Mint>>,
    #[account(init_if_needed, payer = payer, space = 8 + ReceivingCustody::INIT_SPACE,
        seeds = [b"receiving-custody".as_ref(), principal.domain.as_ref(), principal.key().as_ref(), mint.key().as_ref()], bump)]
    pub custody: Box<Account<'info, ReceivingCustody>>,
    #[account(init_if_needed, payer = payer, associated_token::mint = mint,
        associated_token::authority = custody, associated_token::token_program = token_program)]
    pub custody_token: Box<Account<'info, TokenAccount>>,
    #[account(init_if_needed, payer = payer, space = 8 + ReceivingReceipt::INIT_SPACE,
        seeds = [b"receiving-operation", custody.key().as_ref(), &args.operation_id], bump)]
    pub receipt: Box<Account<'info, ReceivingReceipt>>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(args: WithdrawReceivingTokensArgs)]
pub struct WithdrawReceivingTokens<'info> {
    #[account(mut)] pub payer: Signer<'info>,
    pub principal: Box<Account<'info, Principal>>,
    /// CHECK: Same receipt-first/current-owner distinction as opening custody.
    pub authority: UncheckedAccount<'info>,
    #[account(mut)] pub custody: Box<Account<'info, ReceivingCustody>>,
    /// CHECK: Fresh execution authenticates the canonical classic-SPL source.
    /// Historical replay does not depend on its current balance or token state.
    #[account(mut)] pub custody_token: UncheckedAccount<'info>,
    /// CHECK: Fresh execution authenticates exact canonical ATA/owner/mint and
    /// no delegate/close authority. A completed destination may later close.
    #[account(mut)] pub destination: UncheckedAccount<'info>,
    pub mint: Box<Account<'info, Mint>>,
    #[account(init_if_needed, payer = payer, space = 8 + ReceivingReceipt::INIT_SPACE,
        seeds = [b"receiving-operation", custody.key().as_ref(), &args.operation_id], bump)]
    pub receipt: Box<Account<'info, ReceivingReceipt>>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

fn current_owner(principal: &Account<Principal>, authority: &AccountInfo, generation: u64,
    remaining: &[AccountInfo]) -> Result<()> {
    valid_principal(principal)?;
    require!(authority.key() == principal.owner && generation > 0
        && generation == principal.authority_generation, CoreError::ReceivingAuthority);
    // Retain the native refusal class; the shared Registry reader alone may
    // recognize an off-curve credential alias and its fresh exact-operation
    // authorization. An arbitrary PDA or website session supplies no authority.
    require!(!authority.key().is_on_curve() || authority.is_signer, CoreError::ReceivingAuthority);
    principal_registry::require_owner_authority(principal, authority, remaining)
}

fn read_cash(info: &AccountInfo, wallet: Pubkey, mint: Pubkey) -> Result<TokenAccount> {
    require_keys_eq!(*info.owner, token::ID, CoreError::ReceivingRoute);
    let value = TokenAccount::try_deserialize(&mut &info.try_borrow_data()?[..])?;
    require!(value.owner == wallet && value.mint == mint
        && info.key() == associated_token::get_associated_token_address(&wallet, &mint)
        && value.state == token::spl_token::state::AccountState::Initialized
        && value.delegate.is_none() && value.close_authority.is_none(), CoreError::ReceivingRoute);
    Ok(value)
}

fn request_digest(action: u8, domain: &[u8; 32], principal: &Pubkey, custody: &Pubkey,
    operation_id: &[u8; 32], authority: &Pubkey, generation: u64, mint: &Pubkey,
    source: &Pubkey, destination: &Pubkey, destination_authority: &Pubkey, amount: u64) -> [u8; 32]
{
    hashv(&[b"DEXTER:RECEIVING-REQUEST:1", CORE_ID.as_ref(), &[action], domain,
        principal.as_ref(), custody.as_ref(), operation_id, authority.as_ref(),
        &generation.to_le_bytes(), mint.as_ref(), token::ID.as_ref(), source.as_ref(),
        destination.as_ref(), destination_authority.as_ref(), &amount.to_le_bytes()]).to_bytes()
}

fn recorded(receipt: &Account<ReceivingReceipt>, custody: Pubkey, id: &[u8; 32],
    digest: &[u8; 32]) -> Result<bool>
{
    if receipt.version == 0 { return Ok(false); }
    let (key, bump) = receiving_receipt_address(&custody, id);
    require!(receipt.version == RECEIVING_VERSION && receipt.key() == key && receipt.bump == bump
        && receipt.custody == custody && receipt.operation_id == *id
        && receipt.request_digest == *digest, CoreError::ReceivingReplay);
    // Validate retained accounting without applying any present-owner/balance
    // condition. An opening observes the prefunded ATA but moves no tokens.
    require!(receipt.token_program == token::ID
        && match receipt.action {
            RECEIVING_OPEN => receipt.amount == 0 && receipt.source == receipt.destination
                && receipt.source_before == receipt.source_after
                && receipt.destination_before == receipt.destination_after
                && receipt.source_before == receipt.destination_before
                && receipt.total_withdrawn_after == 0,
            RECEIVING_WITHDRAW => receipt.amount > 0 && receipt.source != receipt.destination
                && receipt.source_before.checked_sub(receipt.amount) == Some(receipt.source_after)
                && receipt.destination_before.checked_add(receipt.amount) == Some(receipt.destination_after)
                && receipt.total_withdrawn_after >= receipt.amount as u128,
            _ => false,
        }, CoreError::ReceivingReplay);
    Ok(true)
}

pub fn open(ctx: Context<OpenReceivingCustody>, args: OpenReceivingCustodyArgs) -> Result<()> {
    require!(get_stack_height() == TRANSACTION_LEVEL_STACK_HEIGHT, CoreError::ReceivingEnvelope);
    let a = ctx.accounts;
    let custody_key = a.custody.key();
    let digest = request_digest(RECEIVING_OPEN, &a.principal.domain, &a.principal.key(),
        &custody_key, &args.operation_id, &a.authority.key(), args.authority_generation,
        &a.mint.key(), &a.custody_token.key(), &a.custody_token.key(), &custody_key, 0);
    if recorded(&a.receipt, custody_key, &args.operation_id, &digest)? {
        a.custody.check(custody_key)?;
        require_keys_eq!(a.custody.opening_receipt, a.receipt.key(), CoreError::ReceivingReplay);
        return Ok(());
    }
    current_owner(&a.principal, &a.authority, args.authority_generation, ctx.remaining_accounts)?;
    require!(args.operation_id != [0; 32] && a.principal.domain != [0; 32]
        && a.custody.version == 0, CoreError::ReceivingCustody);
    let token = read_cash(&a.custody_token.to_account_info(), custody_key, a.mint.key())?;
    a.custody.set_inner(ReceivingCustody {
        version: RECEIVING_VERSION, bump: ctx.bumps.custody, domain: a.principal.domain,
        principal: a.principal.key(), mint: a.mint.key(), token_program: token::ID,
        wallet: custody_key, token_account: a.custody_token.key(), opened_by: a.authority.key(),
        opened_generation: args.authority_generation, opening_receipt: a.receipt.key(), total_withdrawn: 0,
    });
    a.custody.check(custody_key)?;
    a.receipt.set_inner(ReceivingReceipt {
        version: RECEIVING_VERSION, bump: ctx.bumps.receipt, action: RECEIVING_OPEN,
        domain: a.principal.domain, principal: a.principal.key(), custody: custody_key,
        operation_id: args.operation_id, request_digest: digest, authority: a.authority.key(),
        authority_generation: args.authority_generation, mint: a.mint.key(), token_program: token::ID,
        source: a.custody_token.key(), destination: a.custody_token.key(), destination_authority: custody_key,
        amount: 0, source_before: token.amount, source_after: token.amount,
        destination_before: token.amount, destination_after: token.amount,
        total_withdrawn_after: 0, completed_at: Clock::get()?.unix_timestamp,
    });
    emit!(ReceivingOperationCompleted { custody: custody_key, principal: a.principal.key(),
        receipt: a.receipt.key(), action: RECEIVING_OPEN, amount: 0,
        authority: a.authority.key(), authority_generation: args.authority_generation });
    Ok(())
}

pub fn withdraw(ctx: Context<WithdrawReceivingTokens>, args: WithdrawReceivingTokensArgs) -> Result<()> {
    require!(get_stack_height() == TRANSACTION_LEVEL_STACK_HEIGHT, CoreError::ReceivingEnvelope);
    let a = ctx.accounts;
    let custody_key = a.custody.key();
    let digest = request_digest(RECEIVING_WITHDRAW, &a.principal.domain, &a.principal.key(),
        &custody_key, &args.operation_id, &a.authority.key(), args.authority_generation,
        &a.mint.key(), &a.custody_token.key(), &a.destination.key(), &args.destination_authority, args.amount);
    if recorded(&a.receipt, custody_key, &args.operation_id, &digest)? { return Ok(()); }
    current_owner(&a.principal, &a.authority, args.authority_generation, ctx.remaining_accounts)?;
    require!(args.operation_id != [0; 32] && args.amount > 0, CoreError::ReceivingAmount);
    a.custody.check(custody_key)?;
    require!(a.custody.domain == a.principal.domain && a.custody.principal == a.principal.key()
        && a.custody.mint == a.mint.key() && a.custody.token_account == a.custody_token.key(),
        CoreError::ReceivingCustody);
    require!(args.destination_authority.is_on_curve() && a.custody_token.key() != a.destination.key(),
        CoreError::ReceivingRoute);
    let source = read_cash(&a.custody_token, custody_key, a.mint.key())?;
    let destination = read_cash(&a.destination, args.destination_authority, a.mint.key())?;
    let source_after = source.amount.checked_sub(args.amount).ok_or_else(|| error!(CoreError::ReceivingAmount))?;
    let destination_after = destination.amount.checked_add(args.amount).ok_or_else(|| error!(CoreError::ReceivingAmount))?;
    let total_after = a.custody.total_withdrawn.checked_add(args.amount as u128)
        .ok_or_else(|| error!(CoreError::ReceivingAmount))?;
    let bump = [a.custody.bump];
    let seeds: &[&[u8]] = &[b"receiving-custody", &a.custody.domain, a.custody.principal.as_ref(),
        a.custody.mint.as_ref(), &bump];
    transfer_checked_exact(&a.custody_token, &a.destination, &a.custody.to_account_info(),
        &a.mint, a.token_program.key(), args.amount, &[seeds])?;
    let observed_source = read_cash(&a.custody_token, custody_key, a.mint.key())?;
    let observed_destination = read_cash(&a.destination, args.destination_authority, a.mint.key())?;
    require!(observed_source.amount == source_after && observed_destination.amount == destination_after,
        CoreError::FundingTokenDelta);
    a.custody.total_withdrawn = total_after;
    a.receipt.set_inner(ReceivingReceipt {
        version: RECEIVING_VERSION, bump: ctx.bumps.receipt, action: RECEIVING_WITHDRAW,
        domain: a.principal.domain, principal: a.principal.key(), custody: custody_key,
        operation_id: args.operation_id, request_digest: digest, authority: a.authority.key(),
        authority_generation: args.authority_generation, mint: a.mint.key(), token_program: token::ID,
        source: a.custody_token.key(), destination: a.destination.key(), destination_authority: args.destination_authority,
        amount: args.amount, source_before: source.amount, source_after,
        destination_before: destination.amount, destination_after,
        total_withdrawn_after: total_after, completed_at: Clock::get()?.unix_timestamp,
    });
    emit!(ReceivingOperationCompleted { custody: custody_key, principal: a.principal.key(),
        receipt: a.receipt.key(), action: RECEIVING_WITHDRAW, amount: args.amount,
        authority: a.authority.key(), authority_generation: args.authority_generation });
    Ok(())
}

#[event]
pub struct ReceivingOperationCompleted {
    pub custody: Pubkey,
    pub principal: Pubkey,
    pub receipt: Pubkey,
    pub action: u8,
    pub amount: u64,
    pub authority: Pubkey,
    pub authority_generation: u64,
}
