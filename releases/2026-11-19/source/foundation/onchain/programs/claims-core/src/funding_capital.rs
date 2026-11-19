//! Explicit own-capital allocation. Contribution records new transferred cash;
//! surplus adoption records an owner attestation without inventing an inflow.
use anchor_lang::prelude::*;
use crate::CoreError;
use anchor_spl::{associated_token, token::{Mint, Token, TokenAccount}};
use dexter_contracts::{plus, POOL_ID};
use dexter_contracts::funded::{financial_digest, CAPITAL_ALLOCATED, CASH_RECEIVED,
    FINANCE_CONTRIBUTE, FINANCE_ADOPT_SURPLUS};
use principal_registry::Principal;
use crate::funding_state::*;
use crate::funding_journal::{inc, journal};
use crate::funding::{operation_id, emit_record_operation};
use crate::tab_events::record_operation_id;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct ContributeFinancierCapitalArgs {
    /// Caller-owned raw ID; the operation id is scoped to the owner signer.
    pub request_id: [u8; 32],
    pub amount: u64,
    /// The owner's number for this move, admitted once by the custody's window.
    pub number: u64,
    pub authority_generation: u64,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct AdoptFinancierSurplusArgs {
    pub request_id: [u8; 32],
    pub amount: u64,
    /// The owner's number for this move, admitted once by the custody's window.
    pub number: u64,
    pub authority_generation: u64,
    /// Signed ownership/provenance attestation; not proof adjudicating third-party title.
    pub provenance_hash: [u8; 32],
}

#[derive(Accounts)]
pub struct ContributeFinancierCapital<'info> {
    pub owner: Signer<'info>,
    pub principal: Box<Account<'info, Principal>>,
    #[account(mut)] pub custody: Box<Account<'info, FinancierCustody>>,
    #[account(mut)] pub from: Box<Account<'info, TokenAccount>>,
    #[account(mut)] pub custody_token: Box<Account<'info, TokenAccount>>,
    pub mint: Box<Account<'info, Mint>>,
    /// CHECK: Exact executable Pool, with strict complete journals/caught-up projections below.
    #[account(address = POOL_ID, executable)] pub pool_program: UncheckedAccount<'info>,
    pub token_program: Program<'info, Token>,
    /// CHECK: ClaimsCore's event authority.
    #[account(address = crate::EVENT_AUTHORITY_AND_BUMP.0)] pub core_event_authority: UncheckedAccount<'info>,
    /// CHECK: ClaimsCore itself.
    #[account(address = crate::ID)] pub core_program: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct AdoptFinancierSurplus<'info> {
    pub owner: Signer<'info>,
    pub principal: Box<Account<'info, Principal>>,
    #[account(mut)] pub custody: Box<Account<'info, FinancierCustody>>,
    pub custody_token: Box<Account<'info, TokenAccount>>,
    pub mint: Box<Account<'info, Mint>>,
    /// CHECK: Exact executable Pool, with strict complete journals/caught-up projections below.
    #[account(address = POOL_ID, executable)] pub pool_program: UncheckedAccount<'info>,
    /// CHECK: ClaimsCore's event authority.
    #[account(address = crate::EVENT_AUTHORITY_AND_BUMP.0)] pub core_event_authority: UncheckedAccount<'info>,
    /// CHECK: ClaimsCore itself.
    #[account(address = crate::ID)] pub core_program: UncheckedAccount<'info>,
}

pub fn contribute<'info>(ctx: Context<'info, ContributeFinancierCapital<'info>>,
    args: ContributeFinancierCapitalArgs) -> Result<()> {
    let principal = ctx.accounts.principal.key();
    let authority = ctx.accounts.owner.key();
    let id = operation_id(b"capital-contribute", authority, &args.request_id);
    let digest = financial_digest(b"DEXTER:CONTRIBUTE-FINANCIER-CAPITAL:1", &(
        principal, authority, args.authority_generation, id, ctx.accounts.custody.key(),
        ctx.accounts.from.key(), ctx.accounts.custody_token.key(), ctx.accounts.mint.key(), args.amount))?;
    // The number is admitted once by the custody's window, so neither a retry
    // nor a rebuild can debit the source a second time.
    let rid = record_operation_id(&id, &digest);
    ctx.accounts.custody.check_key(ctx.accounts.custody.key())?;
    admit_number(&mut ctx.accounts.custody.window, args.number)?;
    owner(&ctx.accounts.principal, authority, args.authority_generation)?;
    require!(authority.is_on_curve() && args.amount > 0, CoreError::FundingAmount);
    require!(ctx.accounts.custody.principal == principal
        && ctx.accounts.custody.domain == ctx.accounts.principal.domain, CoreError::FundingRevision);
    custody_token(&ctx.accounts.custody, &ctx.accounts.custody_token, ctx.accounts.mint.key())?;
    admit_cash(&ctx.accounts.custody_token)?;
    require!(ctx.accounts.from.owner == authority && ctx.accounts.from.mint == ctx.accounts.mint.key()
        && ctx.accounts.from.key() == associated_token::get_associated_token_address(&authority, &ctx.accounts.mint.key()),
        CoreError::FundingTokenRouting);
    require_keys_neq!(ctx.accounts.from.key(), ctx.accounts.custody_token.key(), CoreError::FundingTokenRouting);
    transfer_checked_exact(&ctx.accounts.from.to_account_info(), &ctx.accounts.custody_token.to_account_info(),
        &ctx.accounts.owner.to_account_info(), &ctx.accounts.mint, ctx.accounts.token_program.key(), args.amount, &[])?;
    ctx.accounts.custody_token.reload()?;
    ctx.accounts.custody.credit(args.amount)?;
    ctx.accounts.custody.contributed_capital = plus(ctx.accounts.custody.contributed_capital, args.amount)?;
    ctx.accounts.custody.check(ctx.accounts.custody_token.amount)?;
    let mut data = record_base(principal, id, digest, FINANCE_CONTRIBUTE, ctx.accounts.mint.key(),
        Pubkey::default(), Pubkey::default(), args.amount);
    data.capital_custody = ctx.accounts.custody.key();
    data.capital_allocated = args.amount;
    data.capital_token_inflow = args.amount;
    data.capital_authority = authority;
    data.capital_from = ctx.accounts.from.key();
    data.capital_owner_generation = args.authority_generation;
    data.capital_provenance_hash = digest; // Actual authenticated transfer provenance.
    let booked = journal(&rid, ctx.accounts.mint.key(),
        &[inc(principal, CAPITAL_ALLOCATED, args.amount), inc(principal, CASH_RECEIVED, args.amount)],
        ctx.remaining_accounts, &ctx.accounts.pool_program.to_account_info(), false, true)?;
    emit_record_operation(&ctx.accounts.core_event_authority, FINANCE_CONTRIBUTE, &rid, &ctx.accounts.custody.key(),
        args.number, &data, &booked.books, &booked.changes)
}

pub fn adopt<'info>(ctx: Context<'info, AdoptFinancierSurplus<'info>>,
    args: AdoptFinancierSurplusArgs) -> Result<()> {
    let principal = ctx.accounts.principal.key();
    let authority = ctx.accounts.owner.key();
    let id = operation_id(b"capital-adopt", authority, &args.request_id);
    let digest = financial_digest(b"DEXTER:ADOPT-FINANCIER-SURPLUS:1", &(
        principal, authority, args.authority_generation, id, ctx.accounts.custody.key(),
        ctx.accounts.custody_token.key(), ctx.accounts.mint.key(), args.amount, args.provenance_hash))?;
    let rid = record_operation_id(&id, &digest);
    ctx.accounts.custody.check_key(ctx.accounts.custody.key())?;
    admit_number(&mut ctx.accounts.custody.window, args.number)?;
    owner(&ctx.accounts.principal, authority, args.authority_generation)?;
    require!(authority.is_on_curve() && args.amount > 0 && args.provenance_hash != [0; 32], CoreError::FundingAmount);
    require!(ctx.accounts.custody.principal == principal
        && ctx.accounts.custody.domain == ctx.accounts.principal.domain, CoreError::FundingRevision);
    custody_token(&ctx.accounts.custody, &ctx.accounts.custody_token, ctx.accounts.mint.key())?;
    admit_cash(&ctx.accounts.custody_token)?;
    // check() above refuses a deficit. Adoption cannot relabel a shortfall as
    // capital, spend accepted backing, or count the same surplus twice.
    let surplus = dexter_contracts::minus(ctx.accounts.custody_token.amount, ctx.accounts.custody.accounted_cash)?;
    require!(args.amount <= surplus, CoreError::FundingAmount);
    ctx.accounts.custody.credit(args.amount)?;
    ctx.accounts.custody.contributed_capital = plus(ctx.accounts.custody.contributed_capital, args.amount)?;
    ctx.accounts.custody.check(ctx.accounts.custody_token.amount)?;
    let mut data = record_base(principal, id, digest, FINANCE_ADOPT_SURPLUS, ctx.accounts.mint.key(),
        Pubkey::default(), Pubkey::default(), args.amount);
    data.capital_custody = ctx.accounts.custody.key();
    data.capital_allocated = args.amount;
    data.capital_token_inflow = 0;
    data.capital_authority = authority;
    data.capital_from = Pubkey::default();
    data.capital_owner_generation = args.authority_generation;
    data.capital_provenance_hash = args.provenance_hash;
    let booked = journal(&rid, ctx.accounts.mint.key(), &[inc(principal, CAPITAL_ALLOCATED, args.amount)],
        ctx.remaining_accounts, &ctx.accounts.pool_program.to_account_info(), false, true)?;
    emit_record_operation(&ctx.accounts.core_event_authority, FINANCE_ADOPT_SURPLUS, &rid, &ctx.accounts.custody.key(),
        args.number, &data, &booked.books, &booked.changes)
}
