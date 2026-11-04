use anchor_lang::prelude::*;
use anchor_spl::{associated_token::AssociatedToken, token::{self, Mint, Token, TokenAccount, TransferChecked}};
use dexter_contracts::{self as contracts, Balances, EffectData, SourceData, ACCEPT, CAPTURE, PAYOUT, REPAY, RELEASE, SCHEMA_VERSION, CORE_ID, POOL_DISPATCH_SEED};

pub mod funding;
pub use funding::*;

declare_id!("3yceApSVxLYihHjLLrCycV8T4Bg1MYggbxQhdzaqB2Sm");

#[program]
pub mod pool_issuer {
    use super::*;

    pub fn initialize_financial_projection(ctx: Context<InitializeFinancialProjection>) -> Result<()> {
        funding::initialize(ctx)
    }

    pub fn project_financing(ctx: Context<ProjectFinancing>) -> Result<()> {
        funding::project_one(ctx)
    }

    pub fn project_financing_batch(ctx: Context<ProjectFinancingBatch>) -> Result<()> {
        funding::project_batch(ctx)
    }

    pub fn initialize_pool(ctx: Context<InitializePool>, pool_id: [u8; 32]) -> Result<()> {
        let pool_key = ctx.accounts.pool.key();
        let mint = ctx.accounts.mint.key();
        let source = Pubkey::find_program_address(&[b"source", pool_key.as_ref(), mint.as_ref()], &CORE_ID).0;
        ctx.accounts.pool.set_inner(PoolBook {
            version: SCHEMA_VERSION, bump: ctx.bumps.pool, pool_id,
            manager: ctx.accounts.manager.key(), mint, treasury: ctx.accounts.treasury.key(), source,
            operating_cash: 0, funded_capital: 0, projected: Balances::default(),
            applied_sequence: 0, applied_hash: [0; 32], admission_open: true,
        });
        Ok(())
    }

    /// Records actual external cash and its contributor. Investor unit issuance,
    /// ranking and redemption are separate required implementation work.
    pub fn fund_pool(ctx: Context<FundPool>, operation_id: [u8; 32], amount: u64) -> Result<()> {
        let receipt = &ctx.accounts.contribution;
        if receipt.version != 0 {
            require!(receipt.version == SCHEMA_VERSION && receipt.pool == ctx.accounts.pool.key()
                && receipt.funder == ctx.accounts.funder.key() && receipt.operation_id == operation_id
                && receipt.amount == amount && receipt.from == ctx.accounts.from.key(), PoolError::OperationConflict);
            return Ok(());
        }
        require!(amount > 0, PoolError::ZeroAmount);
        require_keys_neq!(ctx.accounts.from.key(), ctx.accounts.treasury.key(), PoolError::TokenRouting);
        let source = checked_source(&ctx.accounts.source, &ctx.accounts.pool)?;
        caught_up(&ctx.accounts.pool, &source)?;
        let before = ctx.accounts.treasury.amount;
        let from_before = ctx.accounts.from.amount;
        require!(before >= ctx.accounts.pool.operating_cash, PoolError::CashDeficit);
        token::transfer_checked(CpiContext::new(ctx.accounts.token_program.key(), TransferChecked {
            from: ctx.accounts.from.to_account_info(), mint: ctx.accounts.mint.to_account_info(),
            to: ctx.accounts.treasury.to_account_info(), authority: ctx.accounts.funder.to_account_info(),
        }), amount, ctx.accounts.mint.decimals)?;
        ctx.accounts.treasury.reload()?;
        ctx.accounts.from.reload()?;
        require!(ctx.accounts.treasury.amount == contracts::plus(before, amount)?
            && ctx.accounts.from.amount == contracts::minus(from_before, amount)?, PoolError::TokenDelta);
        ctx.accounts.pool.operating_cash = contracts::plus(ctx.accounts.pool.operating_cash, amount)?;
        ctx.accounts.pool.funded_capital = contracts::plus(ctx.accounts.pool.funded_capital, amount)?;
        ctx.accounts.contribution.set_inner(CapitalContribution {
            version: SCHEMA_VERSION, pool: ctx.accounts.pool.key(), funder: ctx.accounts.funder.key(),
            operation_id, from: ctx.accounts.from.key(), amount,
        });
        Ok(())
    }

    /// Only Core can reserve cash. The Core receipt and new head must already
    /// be serialized before this CPI; every failure rolls the whole transaction back.
    pub fn reserve(ctx: Context<Reserve>) -> Result<()> {
        require!(ctx.accounts.pool.admission_open, PoolError::AdmissionClosed);
        let source = checked_source(&ctx.accounts.source, &ctx.accounts.pool)?;
        let operation = contracts::read_core_operation(&ctx.accounts.operation)?;
        let effect = &operation.effect;
        validate_effect(&ctx.accounts.pool, ctx.accounts.source.key(), &source, effect, operation.effect_hash, true)?;
        require!(effect.kind == ACCEPT, PoolError::WrongEffect);
        require_keys_eq!(ctx.accounts.escrow.key(), source.escrow, PoolError::TokenRouting);
        require_keys_eq!(ctx.accounts.escrow.owner, ctx.accounts.source.key(), PoolError::TokenRouting);
        require_keys_neq!(ctx.accounts.treasury.key(), ctx.accounts.escrow.key(), PoolError::TokenRouting);
        let before = ctx.accounts.treasury.amount;
        let escrow_before = ctx.accounts.escrow.amount;
        require!(before >= ctx.accounts.pool.operating_cash && ctx.accounts.pool.operating_cash >= effect.amount, PoolError::CashDeficit);
        require!(escrow_before >= effect.before.protected_cash, PoolError::CashDeficit);
        let manager = ctx.accounts.pool.manager;
        let mint = ctx.accounts.pool.mint;
        let pool_id = ctx.accounts.pool.pool_id;
        let bump = [ctx.accounts.pool.bump];
        let seeds: &[&[u8]] = &[b"pool", manager.as_ref(), mint.as_ref(), &pool_id, &bump];
        token::transfer_checked(CpiContext::new_with_signer(ctx.accounts.token_program.key(), TransferChecked {
            from: ctx.accounts.treasury.to_account_info(), mint: ctx.accounts.mint.to_account_info(),
            to: ctx.accounts.escrow.to_account_info(), authority: ctx.accounts.pool.to_account_info(),
        }, &[seeds]), effect.amount, ctx.accounts.mint.decimals)?;
        ctx.accounts.treasury.reload()?;
        ctx.accounts.escrow.reload()?;
        require!(ctx.accounts.treasury.amount == contracts::minus(before, effect.amount)?
            && ctx.accounts.escrow.amount == contracts::plus(escrow_before, effect.amount)?, PoolError::TokenDelta);
        apply(&mut ctx.accounts.pool, effect, operation.effect_hash)
    }

    pub fn project(ctx: Context<Project>) -> Result<()> {
        let source = checked_source(&ctx.accounts.source, &ctx.accounts.pool)?;
        let operation = contracts::read_core_operation(&ctx.accounts.operation)?;
        require!(operation.effect.kind != ACCEPT, PoolError::WrongEffect);
        validate_effect(&ctx.accounts.pool, ctx.accounts.source.key(), &source, &operation.effect, operation.effect_hash, true)?;
        apply(&mut ctx.accounts.pool, &operation.effect, operation.effect_hash)
    }

    /// Permissionless ordered catchup consumes Core-owned immutable receipts.
    /// It cannot manufacture a receipt, skip a predecessor or move tokens twice.
    pub fn catch_up(ctx: Context<CatchUp>) -> Result<()> {
        let source = checked_source(&ctx.accounts.source, &ctx.accounts.pool)?;
        let operation = contracts::read_core_operation(&ctx.accounts.operation)?;
        require!(operation.effect.kind != ACCEPT, PoolError::WrongEffect);
        validate_effect(&ctx.accounts.pool, ctx.accounts.source.key(), &source, &operation.effect, operation.effect_hash, false)?;
        apply(&mut ctx.accounts.pool, &operation.effect, operation.effect_hash)
    }

    pub fn set_admission(ctx: Context<ManagePool>, enabled: bool) -> Result<()> {
        ctx.accounts.pool.admission_open = enabled;
        Ok(())
    }

    /// The executed cash-guarantee subset of the authoritative NAV equation.
    /// No investor share price or complete valuation engine is advertised here.
    pub fn read_book_value(ctx: Context<ReadBook>) -> Result<u64> {
        let source = checked_source(&ctx.accounts.source, &ctx.accounts.pool)?;
        caught_up(&ctx.accounts.pool, &source)?;
        let p = &ctx.accounts.pool;
        contracts::minus(contracts::plus(contracts::plus(p.operating_cash, p.projected.protected_cash)?, p.projected.debt)?, p.projected.payable)
    }
}

fn checked_source(account: &AccountInfo, pool: &Account<PoolBook>) -> Result<SourceData> {
    require_keys_eq!(account.key(), pool.source, PoolError::WrongSource);
    let source = contracts::read_core_source(account)?;
    require_keys_eq!(source.pool, pool.key(), PoolError::WrongSource);
    require_keys_eq!(source.mint, pool.mint, PoolError::WrongSource);
    require_keys_eq!(source.treasury, pool.treasury, PoolError::WrongSource);
    let expected = Pubkey::find_program_address(&[b"source", pool.key().as_ref(), pool.mint.as_ref()], &CORE_ID).0;
    require_keys_eq!(account.key(), expected, PoolError::WrongSource);
    Ok(source)
}

fn caught_up(pool: &PoolBook, source: &SourceData) -> Result<()> {
    require!(pool.applied_sequence == source.sequence && pool.applied_hash == source.head
        && pool.projected == source.balances, PoolError::StaleBook);
    Ok(())
}

fn validate_effect(pool: &Account<PoolBook>, source_key: Pubkey, source: &SourceData, effect: &EffectData, hash: [u8; 32], latest: bool) -> Result<()> {
    require!(effect.version == SCHEMA_VERSION, PoolError::WrongEffect);
    require_keys_eq!(effect.source, source_key, PoolError::WrongEffect);
    require_keys_eq!(effect.book, pool.key(), PoolError::WrongEffect);
    require_keys_eq!(effect.mint, pool.mint, PoolError::WrongEffect);
    require!(effect.sequence == contracts::plus(pool.applied_sequence, 1)? && effect.sequence <= source.sequence
        && effect.previous_hash == pool.applied_hash, PoolError::EffectOrder);
    require!(effect.before == pool.projected && effect.after == effect.before.transition(effect.kind, effect.amount)?, PoolError::WrongEffect);
    if latest || effect.sequence == source.sequence {
        require!(effect.sequence == source.sequence && hash == source.head && effect.after == source.balances, PoolError::WrongEffect);
    }
    Ok(())
}

fn apply(pool: &mut PoolBook, effect: &EffectData, hash: [u8; 32]) -> Result<()> {
    pool.operating_cash = match effect.kind {
        ACCEPT => contracts::minus(pool.operating_cash, effect.amount)?,
        REPAY | RELEASE => contracts::plus(pool.operating_cash, effect.amount)?,
        CAPTURE | PAYOUT => pool.operating_cash,
        _ => return err!(PoolError::WrongEffect),
    };
    pool.projected = effect.after;
    pool.applied_sequence = effect.sequence;
    pool.applied_hash = hash;
    Ok(())
}

#[derive(Accounts)]
#[instruction(pool_id: [u8; 32])]
pub struct InitializePool<'info> {
    #[account(mut)] pub manager: Signer<'info>,
    pub mint: Box<Account<'info, Mint>>,
    #[account(init, payer = manager, space = 8 + PoolBook::INIT_SPACE,
        seeds = [b"pool", manager.key().as_ref(), mint.key().as_ref(), &pool_id], bump)]
    pub pool: Box<Account<'info, PoolBook>>,
    #[account(init_if_needed, payer = manager, associated_token::mint = mint, associated_token::authority = pool)]
    pub treasury: Box<Account<'info, TokenAccount>>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(operation_id: [u8; 32])]
pub struct FundPool<'info> {
    #[account(mut)] pub funder: Signer<'info>,
    #[account(mut, has_one = mint, has_one = treasury, has_one = source)] pub pool: Box<Account<'info, PoolBook>>,
    /// CHECK: Core ownership, discriminator, PDA and bindings checked by checked_source.
    pub source: UncheckedAccount<'info>,
    #[account(mut, token::mint = mint, token::authority = funder)] pub from: Box<Account<'info, TokenAccount>>,
    #[account(mut, token::mint = mint, token::authority = pool)] pub treasury: Box<Account<'info, TokenAccount>>,
    pub mint: Box<Account<'info, Mint>>,
    #[account(init_if_needed, payer = funder, space = 8 + CapitalContribution::INIT_SPACE,
        seeds = [b"capital", pool.key().as_ref(), funder.key().as_ref(), &operation_id], bump)]
    pub contribution: Box<Account<'info, CapitalContribution>>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Reserve<'info> {
    #[account(seeds = [POOL_DISPATCH_SEED], bump, seeds::program = CORE_ID)] pub dispatcher: Signer<'info>,
    #[account(mut, has_one = mint, has_one = treasury, has_one = source)] pub pool: Box<Account<'info, PoolBook>>,
    /// CHECK: Validated shared Core account reader.
    pub source: UncheckedAccount<'info>,
    /// CHECK: Validated shared Core operation reader and effect hash.
    pub operation: UncheckedAccount<'info>,
    #[account(mut, token::mint = mint, token::authority = pool)] pub treasury: Box<Account<'info, TokenAccount>>,
    #[account(mut, token::mint = mint)] pub escrow: Box<Account<'info, TokenAccount>>,
    pub mint: Box<Account<'info, Mint>>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct Project<'info> {
    #[account(seeds = [POOL_DISPATCH_SEED], bump, seeds::program = CORE_ID)] pub dispatcher: Signer<'info>,
    #[account(mut, has_one = source)] pub pool: Box<Account<'info, PoolBook>>,
    /// CHECK: Validated shared Core account reader.
    pub source: UncheckedAccount<'info>,
    /// CHECK: Validated shared Core operation reader.
    pub operation: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct CatchUp<'info> {
    #[account(mut, has_one = source)] pub pool: Box<Account<'info, PoolBook>>,
    /// CHECK: Validated shared Core account reader.
    pub source: UncheckedAccount<'info>,
    /// CHECK: Validated shared Core operation reader.
    pub operation: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct ManagePool<'info> {
    pub manager: Signer<'info>,
    #[account(mut, has_one = manager)] pub pool: Box<Account<'info, PoolBook>>,
}

#[derive(Accounts)]
pub struct ReadBook<'info> {
    #[account(has_one = source)] pub pool: Box<Account<'info, PoolBook>>,
    /// CHECK: Validated shared Core account reader.
    pub source: UncheckedAccount<'info>,
}

#[account]
#[derive(InitSpace)]
pub struct PoolBook {
    pub version: u16,
    pub bump: u8,
    pub pool_id: [u8; 32],
    pub manager: Pubkey,
    pub mint: Pubkey,
    pub treasury: Pubkey,
    pub source: Pubkey,
    pub operating_cash: u64,
    pub funded_capital: u64,
    pub projected: Balances,
    pub applied_sequence: u64,
    pub applied_hash: [u8; 32],
    pub admission_open: bool,
}

#[account]
#[derive(InitSpace)]
pub struct CapitalContribution {
    pub version: u16,
    pub pool: Pubkey,
    pub funder: Pubkey,
    pub operation_id: [u8; 32],
    pub from: Pubkey,
    pub amount: u64,
}

#[error_code]
pub enum PoolError {
    #[msg("Economic operation identity conflicts with recorded contribution")] OperationConflict,
    #[msg("Amount must be positive")] ZeroAmount,
    #[msg("Token accounts do not match the accepted source route")] TokenRouting,
    #[msg("Actual token movement differs from the authorized exact amount")] TokenDelta,
    #[msg("Accounted source cash is unavailable or actual custody is deficient")] CashDeficit,
    #[msg("Source belongs to a different book or asset")] WrongSource,
    #[msg("Pool projection must catch up to the complete Core head")] StaleBook,
    #[msg("Core effect has the wrong identity, transition or head")] WrongEffect,
    #[msg("Effect is skipped, replayed or has the wrong predecessor")] EffectOrder,
    #[msg("Issuer admission is closed")] AdmissionClosed,
    #[msg("Projection accounts must contain the exact ordered complete receipt effect set")] ProjectionAccounts,
}
