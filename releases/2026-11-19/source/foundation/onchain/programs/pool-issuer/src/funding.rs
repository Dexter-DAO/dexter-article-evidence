//! Ordered projections of complete Core-authoritative financial journals.
//! Projection does not transfer cash, alter a claim or require a backend signer.
//!
//! Change 5: no receipt. ClaimsCore passes a step's origin id, mint and book
//! changes as CPI data. For each book, Pool derives the effect from its own
//! frontier and the changes, and accepts it only if its hash, sequence and
//! positions equal the book Core has just written. The book's head commits to
//! its previous head and positions, so no caller can forge another transition.
use anchor_lang::prelude::*;
use anchor_lang::Discriminator;
use dexter_contracts::plus;
use dexter_contracts::funded::{self, BookChange, FinanceBookData, FinanceBookEffect,
    FinancialPositions, FUNDED_VERSION, MAX_BOOK_CHANGES, MAX_FINANCIAL_BOOKS};
use crate::PoolError;

#[account]
#[derive(InitSpace, Default)]
pub struct FinancialProjection {
    pub deposit_payer: Pubkey,
    pub version: u16,
    pub bump: u8,
    pub book: Pubkey,
    pub principal: Pubkey,
    pub mint: Pubkey,
    pub applied_sequence: u64,
    pub applied_hash: [u8; 32],
    pub positions: FinancialPositions,
}

#[derive(Accounts)]
pub struct InitializeFinancialProjection<'info> {
    #[account(mut)] pub payer: Signer<'info>,
    /// CHECK: Strict Core ownership, schema, discriminator, address and bump reader.
    pub book: UncheckedAccount<'info>,
    #[account(init_if_needed, payer = payer, space = 8 + FinancialProjection::INIT_SPACE,
        seeds = [b"financial-projection", book.key().as_ref()], bump)]
    pub projection: Box<Account<'info, FinancialProjection>>,
    pub system_program: Program<'info, System>,
}

/// Copy a book's current state into its projection: catch-up after servicing
/// that skipped Pool. Permissionless; the book is Core-owned and authoritative.
#[derive(Accounts)]
pub struct SyncFinancialProjection<'info> {
    /// CHECK: Strict Core financial-book reader validates the complete binding.
    pub book: UncheckedAccount<'info>,
    #[account(mut, has_one = book, seeds = [b"financial-projection", book.key().as_ref()], bump = projection.bump)]
    pub projection: Box<Account<'info, FinancialProjection>>,
}

/// The remaining accounts are exactly [Core book, Pool projection] for every
/// affected principal, sorted by principal.
#[derive(Accounts)]
pub struct ProjectFinancingBatch {}

pub fn initialize(ctx: Context<InitializeFinancialProjection>) -> Result<()> {
    let book = funded::read_finance_book(&ctx.accounts.book)?;
    let projection = &mut ctx.accounts.projection;
    if projection.version != 0 {
        validate_identity(projection, ctx.accounts.book.key(), &book)?;
        require!(projection.applied_sequence <= book.sequence, PoolError::EffectOrder);
        if projection.applied_sequence == book.sequence { require_financing_caught_up(projection, &book)?; }
        return Ok(()); // Reopening never erases a later projection frontier.
    }
    require!(book.sequence == 0 && book.head == [0; 32] && book.positions == FinancialPositions::default(),
        PoolError::StaleBook);
    projection.set_inner(FinancialProjection { deposit_payer: ctx.accounts.payer.key(),
        version: FUNDED_VERSION, bump: ctx.bumps.projection,
        book: ctx.accounts.book.key(), principal: book.principal, mint: book.mint,
        applied_sequence: 0, applied_hash: [0; 32], positions: FinancialPositions::default() });
    Ok(())
}

/// Catch-up copies the book's state; it never replays missed effects (their
/// receipts no longer exist). The projection never moves backward.
pub fn sync(ctx: Context<SyncFinancialProjection>) -> Result<()> {
    let book = funded::read_finance_book(&ctx.accounts.book)?;
    let projection = &mut ctx.accounts.projection;
    validate_identity(projection, ctx.accounts.book.key(), &book)?;
    require!(projection.applied_sequence <= book.sequence, PoolError::EffectOrder);
    projection.applied_sequence = book.sequence;
    projection.applied_hash = book.head;
    projection.positions = book.positions;
    Ok(())
}

/// Apply one step's changes to every affected projection, atomically. Failure
/// in the last pair rolls back all prior writes and the caller's operation.
pub fn project_batch(ctx: Context<ProjectFinancingBatch>, origin: [u8; 32], mint: Pubkey,
    changes: Vec<BookChange>) -> Result<()> {
    require!(!changes.is_empty() && changes.len() <= MAX_BOOK_CHANGES, PoolError::ProjectionAccounts);
    let mut principals: Vec<Pubkey> = changes.iter().map(|c| c.principal).collect();
    principals.sort_unstable(); principals.dedup();
    require!(principals.len() <= MAX_FINANCIAL_BOOKS
        && ctx.remaining_accounts.len() == principals.len() * 2, PoolError::ProjectionAccounts);
    for (i, principal) in principals.iter().enumerate() {
        let book_info = &ctx.remaining_accounts[2 * i];
        let projection_info = &ctx.remaining_accounts[2 * i + 1];
        require_keys_eq!(book_info.key(), funded::book_address(principal, &mint), PoolError::ProjectionAccounts);
        require!(projection_info.is_writable && !projection_info.executable, PoolError::ProjectionAccounts);
        let book = funded::read_finance_book(book_info)?;
        let mut projection = read_financial_projection(projection_info)?;
        let mut after = projection.positions;
        for change in changes.iter().filter(|c| c.principal == *principal) {
            after.apply(change.field, change.increase, change.amount)?;
        }
        let mut effect = FinanceBookEffect { book: book_info.key(), principal: *principal,
            sequence: plus(projection.applied_sequence, 1)?, previous_hash: projection.applied_hash,
            before: projection.positions, after, hash: [0; 32] };
        effect.hash = funded::book_effect_hash(&origin, &mint, &effect)?;
        require!(effect.sequence == book.sequence, PoolError::EffectOrder);
        apply_projection(&mut projection, book_info.key(), &book, &effect)?;
        projection.try_serialize(&mut &mut projection_info.try_borrow_mut_data()?[..])?;
    }
    Ok(())
}

/// Public strict reader used by Core before taking new financial risk.
pub fn read_financial_projection(info: &AccountInfo) -> Result<FinancialProjection> {
    require_keys_eq!(*info.owner, crate::ID, PoolError::WrongSource);
    require!(!info.executable, PoolError::WrongSource);
    let bytes = info.try_borrow_data()?;
    require!(bytes.len() == 8 + FinancialProjection::INIT_SPACE, PoolError::WrongSource);
    let projection = FinancialProjection::try_deserialize(&mut &bytes[..])
        .map_err(|_| error!(PoolError::WrongSource))?;
    let (expected, bump) = Pubkey::find_program_address(&[b"financial-projection", projection.book.as_ref()], &crate::ID);
    require!(projection.version == FUNDED_VERSION && projection.bump == bump && info.key() == expected
        && projection.book == funded::book_address(&projection.principal, &projection.mint), PoolError::WrongSource);
    Ok(projection)
}

/// Return lamports above a projection's rent minimum to the payer it recorded.
pub fn return_excess<'info>(record: &AccountInfo<'info>, payer: &AccountInfo<'info>) -> Result<()> {
    require_keys_eq!(*record.owner, crate::ID, PoolError::NoExcessRent);
    require!(record.is_writable && payer.is_writable && record.key() != payer.key(), PoolError::NoExcessRent);
    let stored = {
        let data = record.try_borrow_data()?;
        require!(data.len() == 8 + FinancialProjection::INIT_SPACE
            && data[..8] == *FinancialProjection::DISCRIMINATOR, PoolError::NoExcessRent);
        Pubkey::new_from_array(data[8..40].try_into().unwrap())
    };
    require_keys_eq!(stored, payer.key(), PoolError::DepositPayer);
    let minimum = Rent::get()?.minimum_balance(record.data_len());
    let excess = record.lamports().checked_sub(minimum).unwrap_or(0);
    require!(excess > 0, PoolError::NoExcessRent);
    **record.try_borrow_mut_lamports()? -= excess;
    **payer.try_borrow_mut_lamports()? = payer.lamports().checked_add(excess)
        .ok_or_else(|| error!(PoolError::NoExcessRent))?;
    Ok(())
}

pub fn require_financing_caught_up(projection: &FinancialProjection, book: &FinanceBookData) -> Result<()> {
    validate_identity(projection, funded::book_address(&book.principal, &book.mint), book)?;
    require!(projection.applied_sequence == book.sequence && projection.applied_hash == book.head
        && projection.positions == book.positions, PoolError::StaleBook);
    Ok(())
}

fn validate_identity(projection: &FinancialProjection, book_key: Pubkey, book: &FinanceBookData) -> Result<()> {
    require!(projection.version == FUNDED_VERSION && book.version == FUNDED_VERSION
        && projection.book == book_key && projection.principal == book.principal
        && projection.mint == book.mint, PoolError::WrongSource);
    Ok(())
}

fn apply_projection(projection: &mut FinancialProjection, book_key: Pubkey,
    book: &FinanceBookData, effect: &FinanceBookEffect) -> Result<()> {
    validate_identity(projection, book_key, book)?;
    require!(effect.book == book_key && effect.principal == book.principal && effect.sequence > 0
        && effect.sequence <= book.sequence, PoolError::WrongEffect);
    if effect.sequence == book.sequence {
        require!(effect.hash == book.head && effect.after == book.positions, PoolError::WrongEffect);
    }
    if projection.applied_sequence == effect.sequence {
        require!(projection.applied_hash == effect.hash && projection.positions == effect.after,
            PoolError::EffectOrder);
        return Ok(()); // The same frontier can be observed again, without cash movement.
    }
    require!(effect.sequence == plus(projection.applied_sequence, 1)?
        && effect.previous_hash == projection.applied_hash && effect.before == projection.positions,
        PoolError::EffectOrder);
    projection.applied_sequence = effect.sequence;
    projection.applied_hash = effect.hash;
    projection.positions = effect.after;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (FinanceBookData, FinancialProjection, FinanceBookEffect) {
        let principal = Pubkey::new_unique(); let mint = Pubkey::new_unique();
        let book_key = funded::book_address(&principal, &mint);
        let mut after = FinancialPositions::default(); after.unpaid_supplier = 60; after.customer_principal = 60;
        let book = FinanceBookData { version: FUNDED_VERSION, bump: 0, principal, mint,
            sequence: 1, head: [9; 32], positions: after };
        let projection = FinancialProjection { version: FUNDED_VERSION, book: book_key, principal, mint,
            ..FinancialProjection::default() };
        let effect = FinanceBookEffect { book: book_key, principal, sequence: 1, previous_hash: [0; 32],
            before: FinancialPositions::default(), after, hash: [9; 32] };
        (book, projection, effect)
    }

    #[test]
    fn projection_catches_up_once_and_replays_without_reapplying() {
        let (book, mut projection, effect) = fixture();
        assert!(require_financing_caught_up(&projection, &book).is_err());
        apply_projection(&mut projection, effect.book, &book, &effect).unwrap();
        assert!(require_financing_caught_up(&projection, &book).is_ok());
        apply_projection(&mut projection, effect.book, &book, &effect).unwrap();
        assert_eq!(projection.positions.customer_principal, 60);
        assert_eq!(projection.applied_sequence, 1);
    }

    #[test]
    fn projection_rejects_a_gap_wrong_predecessor_or_wrong_effect() {
        let (mut book, mut projection, mut effect) = fixture();
        effect.previous_hash = [1; 32];
        assert!(apply_projection(&mut projection, effect.book, &book, &effect).is_err());
        effect.previous_hash = [0; 32]; effect.sequence = 2; book.sequence = 2;
        assert!(apply_projection(&mut projection, effect.book, &book, &effect).is_err());
        effect.sequence = 1; book.sequence = 1; effect.after.customer_principal = 59;
        assert!(apply_projection(&mut projection, effect.book, &book, &effect).is_err());
        assert_eq!(projection.applied_sequence, 0);
    }

    #[test]
    fn ordered_historical_catchup_cannot_roll_back_a_later_frontier() {
        let (mut book, mut projection, first) = fixture();
        book.sequence = 2; book.head = [10; 32]; book.positions.customer_principal = 80;
        apply_projection(&mut projection, first.book, &book, &first).unwrap();
        let second = FinanceBookEffect { sequence: 2, previous_hash: first.hash, before: first.after,
            after: book.positions, hash: book.head, ..first.clone() };
        apply_projection(&mut projection, second.book, &book, &second).unwrap();
        assert!(apply_projection(&mut projection, first.book, &book, &first).is_err());
        assert_eq!(projection.positions.customer_principal, 80);
    }

    #[test]
    fn strict_projection_reader_rejects_wrong_owner_bump_and_account_shape() {
        let (_, mut projection, _) = fixture();
        let (key, bump) = Pubkey::find_program_address(&[b"financial-projection", projection.book.as_ref()], &crate::ID);
        projection.bump = bump;
        let check = |value: &FinancialProjection, owner: &Pubkey, extra: bool| {
            let mut data = Vec::new(); value.try_serialize(&mut data).unwrap(); if extra { data.push(0); }
            let mut lamports = 0;
            let info = AccountInfo::new(&key, false, true, &mut lamports, &mut data, owner, false);
            read_financial_projection(&info).is_ok()
        };
        assert!(check(&projection, &crate::ID, false));
        assert!(!check(&projection, &dexter_contracts::CORE_ID, false));
        assert!(!check(&projection, &crate::ID, true));
        projection.bump ^= 1; assert!(!check(&projection, &crate::ID, false)); projection.bump ^= 1;
        projection.principal = Pubkey::new_unique(); assert!(!check(&projection, &crate::ID, false));
    }
}
