//! PoolIssuer keeps one financial projection per ClaimsCore financial book.
//! Change 5, P1: the cash-guarantee pool (treasury, reserve, project, catch-up,
//! investor contributions and book value) is retired; every lender is its own
//! financier principal with wallet or escrow custody in ClaimsCore.
use anchor_lang::prelude::*;

pub mod funding;
pub use funding::*;

declare_id!("3yceApSVxLYihHjLLrCycV8T4Bg1MYggbxQhdzaqB2Sm");

#[program]
pub mod pool_issuer {
    use super::*;

    pub fn initialize_financial_projection(ctx: Context<InitializeFinancialProjection>) -> Result<()> {
        funding::initialize(ctx)
    }

    /// Catch a projection up to its book by copying the book's state.
    pub fn sync_financial_projection(ctx: Context<SyncFinancialProjection>) -> Result<()> {
        funding::sync(ctx)
    }

    /// One financial step's changes, from ClaimsCore (or anyone): accepted only if
    /// they take each projection exactly to the book's current state.
    pub fn project_financing_batch(ctx: Context<ProjectFinancingBatch>, origin: [u8; 32], mint: Pubkey,
        changes: Vec<dexter_contracts::funded::BookChange>) -> Result<()> {
        funding::project_batch(ctx, origin, mint, changes)
    }

    /// Permissionless. Returns lamports above a projection's rent minimum (from
    /// the Rent sysvar now) to the payer recorded when it was created.
    pub fn return_excess_rent(ctx: Context<ReturnExcessRent>) -> Result<()> {
        funding::return_excess(&ctx.accounts.record, &ctx.accounts.deposit_payer)
    }
}

#[derive(Accounts)]
pub struct ReturnExcessRent<'info> {
    /// CHECK: A Pool financial projection, which stores its rent payer first; checked in the handler.
    #[account(mut)]
    pub record: UncheckedAccount<'info>,
    /// CHECK: Must equal the rent payer stored in the record.
    #[account(mut)]
    pub deposit_payer: UncheckedAccount<'info>,
}

#[error_code]
pub enum PoolError {
    // Codes 6000-6004 and 6009 belonged to the retired cash-guarantee pool
    // (P1, Change 5). Retained codes keep their numbers; retired codes are never reused.
    #[msg("Projection belongs to a different book, principal or asset")] WrongSource = 5,
    #[msg("Pool projection must catch up to the complete Core head")] StaleBook,
    #[msg("Core effect has the wrong identity, transition or head")] WrongEffect,
    #[msg("Effect is skipped, replayed or has the wrong predecessor")] EffectOrder,
    #[msg("Projection accounts must be the exact ordered book and projection pairs of the step")] ProjectionAccounts = 10,
    #[msg("Account is not the record's rent payer")] DepositPayer,
    #[msg("Record has no lamports above its rent minimum, or is not a Pool projection")] NoExcessRent,
}
