//! Opt-in supplier cash custody with an address that survives owner recovery.
//!
//! This is a distinct receiving account, not a financier balance or a source of
//! reserved backing. Collections may credit its canonical ATA without changing
//! the accepted supplier route. Only a fresh exact current-owner instruction
//! can debit it; recovery changes that authority, never the account or assets.
//! No instruction closes the custody, resets history, sets an SPL delegate,
//! changes token authority, or admits this PDA as a financier graph. Change 5:
//! no per-operation receipt; the custody's revision guards a withdrawal and its
//! operation ring makes a recent exact retry a no-op.
use anchor_lang::prelude::*;
use anchor_lang::solana_program::instruction::{get_stack_height, TRANSACTION_LEVEL_STACK_HEIGHT};
use anchor_spl::{associated_token::{self, AssociatedToken}, token::{self, Mint, Token, TokenAccount}};
use dexter_contracts::CORE_ID;
use principal_registry::Principal;
use solana_sha256_hasher::hashv;

use crate::{CoreError, funding_state::{admit_number, transfer_checked_exact, valid_principal}};
use dexter_contracts::window::OrderWindow;
use crate::tab_events::{self, EventBytes, record_operation_id};

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
    /// The owner's number for this withdrawal, admitted once by the custody's
    /// window. Not part of the request digest.
    pub number: u64,
    /// The exact canonical classic-SPL ATA of this external native key receives
    /// the withdrawal. The key need not sign; the current owner approves it.
    pub destination_authority: Pubkey,
}

#[account]
#[derive(InitSpace)]
pub struct ReceivingCustody {
    pub deposit_payer: Pubkey,
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
    /// The opening operation's ring id.
    pub opening_operation: [u8; 32],
    /// Lifetime cash withdrawals, independent of bill debt or finance books.
    pub total_withdrawn: u128,
    /// Advances with every withdrawal.
    pub revision: u64,
    /// Numbers of the custody's withdrawals, allocated by its owner. A
    /// receiving custody never closes.
    pub window: OrderWindow,
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
            && self.opening_operation != [0; 32] && self.revision > 0, CoreError::ReceivingCustody);
        Ok(())
    }
}

/// One receiving-custody operation's event (today's `ReceivingReceipt` body).
#[event]
pub struct ReceivingOperation {
    pub action: u8,
    /// This operation's id.
    pub operation: [u8; 32],
    pub custody: Pubkey,
    /// The number the custody's window admitted (zero for the opening).
    pub number: u64,
    pub principal: Pubkey,
    pub operation_id: [u8; 32],
    pub request_digest: [u8; 32],
    pub authority: Pubkey,
    pub authority_generation: u64,
    pub mint: Pubkey,
    pub source: Pubkey,
    pub destination: Pubkey,
    pub destination_authority: Pubkey,
    pub amount: u64,
    pub source_before: u64,
    pub source_after: u64,
    pub destination_before: u64,
    pub destination_after: u64,
    pub total_withdrawn_after: u128,
    pub revision: u64,
}

pub fn receiving_address(domain: &[u8; 32], principal: &Pubkey, mint: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"receiving-custody", domain, principal.as_ref(), mint.as_ref()], &CORE_ID)
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
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
    /// CHECK: ClaimsCore's event authority.
    #[account(address = crate::EVENT_AUTHORITY_AND_BUMP.0)] pub core_event_authority: UncheckedAccount<'info>,
    /// CHECK: ClaimsCore itself.
    #[account(address = crate::ID)] pub core_program: UncheckedAccount<'info>,
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
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    /// CHECK: ClaimsCore's event authority.
    #[account(address = crate::EVENT_AUTHORITY_AND_BUMP.0)] pub core_event_authority: UncheckedAccount<'info>,
    /// CHECK: ClaimsCore itself.
    #[account(address = crate::ID)] pub core_program: UncheckedAccount<'info>,
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

#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn emit_operation<'info>(authority_account: &AccountInfo<'info>, action: u8, operation: &[u8; 32],
    custody: &Account<ReceivingCustody>, number: u64, operation_id: &[u8; 32], request_digest: &[u8; 32],
    authority: &Pubkey, generation: u64, source: &Pubkey, destination: &Pubkey, destination_authority: &Pubkey,
    amount: u64, balances: (u64, u64, u64, u64)) -> Result<()> {
    let event = ReceivingOperation { action, operation: *operation, custody: custody.key(),
        number, principal: custody.principal, operation_id: *operation_id,
        request_digest: *request_digest, authority: *authority, authority_generation: generation, mint: custody.mint,
        source: *source, destination: *destination, destination_authority: *destination_authority, amount,
        source_before: balances.0, source_after: balances.1, destination_before: balances.2, destination_after: balances.3,
        total_withdrawn_after: custody.total_withdrawn, revision: custody.revision };
    tab_events::emit_event(authority_account, EventBytes::new::<ReceivingOperation>(512).put(&event)?)
}

pub fn open(ctx: Context<OpenReceivingCustody>, args: OpenReceivingCustodyArgs) -> Result<()> {
    require!(get_stack_height() == TRANSACTION_LEVEL_STACK_HEIGHT, CoreError::ReceivingEnvelope);
    let a = ctx.accounts;
    let custody_key = a.custody.key();
    let digest = request_digest(RECEIVING_OPEN, &a.principal.domain, &a.principal.key(),
        &custody_key, &args.operation_id, &a.authority.key(), args.authority_generation,
        &a.mint.key(), &a.custody_token.key(), &a.custody_token.key(), &custody_key, 0);
    let rid = record_operation_id(&args.operation_id, &digest);
    // A receiving custody opens once: a retry after it landed is refused.
    require!(a.custody.version == 0, CoreError::ReceivingReplay);
    current_owner(&a.principal, &a.authority, args.authority_generation, ctx.remaining_accounts)?;
    require!(args.operation_id != [0; 32] && a.principal.domain != [0; 32]
        && a.custody.version == 0, CoreError::ReceivingCustody);
    let token = read_cash(&a.custody_token.to_account_info(), custody_key, a.mint.key())?;
    a.custody.set_inner(ReceivingCustody {
        deposit_payer: a.payer.key(), version: RECEIVING_VERSION, bump: ctx.bumps.custody, domain: a.principal.domain,
        principal: a.principal.key(), mint: a.mint.key(), token_program: token::ID,
        wallet: custody_key, token_account: a.custody_token.key(), opened_by: a.authority.key(),
        opened_generation: args.authority_generation, opening_operation: rid, total_withdrawn: 0,
        revision: 1, window: OrderWindow::default(),
    });
    a.custody.check(custody_key)?;
    let token_account = a.custody_token.key();
    emit_operation(&a.core_event_authority, RECEIVING_OPEN, &rid, &a.custody, 0, &args.operation_id, &digest,
        &a.authority.key(), args.authority_generation, &token_account, &token_account, &custody_key, 0,
        (token.amount, token.amount, token.amount, token.amount))
}

pub fn withdraw(ctx: Context<WithdrawReceivingTokens>, args: WithdrawReceivingTokensArgs) -> Result<()> {
    require!(get_stack_height() == TRANSACTION_LEVEL_STACK_HEIGHT, CoreError::ReceivingEnvelope);
    let a = ctx.accounts;
    let custody_key = a.custody.key();
    let digest = request_digest(RECEIVING_WITHDRAW, &a.principal.domain, &a.principal.key(),
        &custody_key, &args.operation_id, &a.authority.key(), args.authority_generation,
        &a.mint.key(), &a.custody_token.key(), &a.destination.key(), &args.destination_authority, args.amount);
    let rid = record_operation_id(&args.operation_id, &digest);
    a.custody.check(custody_key)?;
    // The owner's number is admitted once: a landed withdrawal, or a rebuild of
    // it, is refused.
    admit_number(&mut a.custody.window, args.number)?;
    current_owner(&a.principal, &a.authority, args.authority_generation, ctx.remaining_accounts)?;
    require!(args.operation_id != [0; 32] && args.amount > 0, CoreError::ReceivingAmount);
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
    a.custody.revision = a.custody.revision.checked_add(1).ok_or_else(|| error!(CoreError::ReceivingAmount))?;
    emit_operation(&a.core_event_authority, RECEIVING_WITHDRAW, &rid, &a.custody, args.number, &args.operation_id, &digest,
        &a.authority.key(), args.authority_generation, &a.custody_token.key(), &a.destination.key(),
        &args.destination_authority, args.amount, (source.amount, source_after, destination.amount, destination_after))
}
