//! Journal arithmetic, origin-bound effect hashes and the Core book reader.
//! Change 5: no financial receipts. A book effect's hash binds the id of the
//! step that made it (Tab step, wallet leg or custody operation).
use anchor_lang::prelude::*;
use dexter_contracts::{funded::*, CORE_ID};
use solana_sha256_hasher::hashv;

fn key(n: u8) -> Pubkey { Pubkey::new_from_array([n; 32]) }
fn effect(origin: &[u8; 32], mint: &Pubkey) -> FinanceBookEffect {
    let principal = key(10); let book = book_address(&principal, mint);
    let mut effect = FinanceBookEffect { book, principal, sequence: 1, previous_hash: [0; 32],
        before: FinancialPositions::default(), after: FinancialPositions { cash_received: 100, ..FinancialPositions::default() }, hash: [0; 32] };
    effect.hash = book_effect_hash(origin, mint, &effect).unwrap(); effect
}
fn account_bytes<T: AnchorSerialize>(name: &[u8], data: &T, allocated: Option<usize>) -> Vec<u8> {
    let mut bytes = hashv(&[name]).to_bytes()[..8].to_vec(); data.serialize(&mut bytes).unwrap();
    if let Some(size) = allocated { assert!(bytes.len() <= size); bytes.resize(size, 0); }
    bytes
}
fn read_book(address: Pubkey, owner: Pubkey, mut bytes: Vec<u8>) -> Result<(Pubkey, FinanceBookData)> {
    let mut lamports = 1;
    let account = AccountInfo::new(&address, false, false, &mut lamports, &mut bytes, &owner, false);
    read_finance_book_paid(&account)
}

#[test]
fn fixed_effect_hash_matches_the_generic_preimage_for_every_position() {
    let origin = [3u8; 32]; let mint = key(2);
    let base = effect(&origin, &mint);
    for field in 0..=SELLER_FEE_CREDITS_RECEIVED {
        for value in [0, 1, 123_456_789, u64::MAX] {
            let mut effect = base.clone();
            effect.sequence = value;
            effect.previous_hash = [field; 32];
            effect.before = FinancialPositions::default();
            effect.after = FinancialPositions::default();
            effect.before.apply(field, true, value).unwrap();
            effect.after.apply(field, true, value.wrapping_add(17)).unwrap();
            let preimage = (origin, mint, effect.book, effect.principal,
                effect.sequence, effect.previous_hash, effect.before, effect.after);
            let mut bytes = Vec::new(); preimage.serialize(&mut bytes).unwrap();
            assert_eq!(bytes.len(), 552);
            let original = financial_digest(b"DEXTER:FINANCE-BOOK-EFFECT:2", &preimage).unwrap();
            assert_eq!(book_effect_hash(&origin, &mint, &effect).unwrap(), original);
            effect.hash = [42; 32]; // The effect's own hash is not in its preimage.
            assert_eq!(book_effect_hash(&origin, &mint, &effect).unwrap(), original);
            assert_ne!(book_effect_hash(&[70; 32], &mint, &effect).unwrap(), original);
            assert_ne!(book_effect_hash(&origin, &key(71), &effect).unwrap(), original);
        }
    }
}

#[test]
fn replaying_changes_reproduces_a_book_head() {
    // A replayer holding a book at (sequence, head, positions) applies one step's
    // changes and recomputes the head from the step's origin id.
    let origin = [7u8; 32]; let mint = key(2); let principal = key(10);
    let changes = [BookChange { principal, field: CASH_RECEIVED, increase: true, amount: 100 },
        BookChange { principal, field: CAPITAL_ALLOCATED, increase: true, amount: 40 }];
    let before = FinancialPositions::default();
    let mut after = before;
    for c in &changes { after.apply(c.field, c.increase, c.amount).unwrap(); }
    let mut effect = FinanceBookEffect { book: book_address(&principal, &mint), principal, sequence: 1,
        previous_hash: [0; 32], before, after, hash: [0; 32] };
    effect.hash = book_effect_hash(&origin, &mint, &effect).unwrap();
    let mut replayed = before;
    for c in &changes { replayed.apply(c.field, c.increase, c.amount).unwrap(); }
    let replay = FinanceBookEffect { after: replayed, ..effect.clone() };
    assert_eq!(book_effect_hash(&origin, &mint, &replay).unwrap(), effect.hash);
}

#[test]
fn book_reader_checks_canonical_bump_payer_and_exact_fixed_layout() {
    let principal = key(1); let mint = key(2); let payer = key(9);
    let (address, bump) = Pubkey::find_program_address(&[b"financial-book", principal.as_ref(), mint.as_ref()], &CORE_ID);
    let book = FinanceBookData { version: FUNDED_VERSION, bump, principal, mint, ..FinanceBookData::default() };
    // The book's account ends with its number window (payer, data, window).
    let window = dexter_contracts::window::OrderWindow::default();
    let bytes = account_bytes(b"account:FinanceBook", &(payer, &book, &window), None);
    assert_eq!(read_book(address, CORE_ID, bytes.clone()).unwrap(), (payer, book.clone()));
    let mut wrong = book.clone(); wrong.bump ^= 1;
    assert!(read_book(address, CORE_ID, account_bytes(b"account:FinanceBook", &(payer, &wrong, &window), None)).is_err());
    // Without the window the account has the wrong length.
    assert!(read_book(address, CORE_ID, account_bytes(b"account:FinanceBook", &(payer, &book), None)).is_err());
    let mut oversized = bytes.clone(); oversized.push(0); assert!(read_book(address, CORE_ID, oversized).is_err());
    assert!(read_book(address, key(90), bytes).is_err());
}

#[test]
fn every_book_amount_is_checked_and_capital_allocation_is_not_cash_or_income() {
    let mut positions = FinancialPositions::default();
    positions.apply(CAPITAL_ALLOCATED, true, 1_000).unwrap();
    assert_eq!((positions.capital_allocated, positions.cash_received, positions.lender_interest), (1_000, 0, 0));
    for field in 0..=SELLER_FEE_CREDITS_RECEIVED {
        let mut p = FinancialPositions::default();
        assert!(p.apply(field, false, 1).is_err());
        p.apply(field, true, u64::MAX).unwrap();
        assert!(p.apply(field, true, 1).is_err());
        p.apply(field, false, u64::MAX).unwrap(); assert_eq!(p, FinancialPositions::default());
    }
    assert!(positions.apply(255, true, 1).is_err());
}

#[test]
fn supplier_assets_and_fee_expenses_do_not_cancel_an_issuers_liabilities_or_income() {
    let mut positions = FinancialPositions::default();
    positions.apply(UNPAID_SUPPLIER, true, 100).unwrap();
    positions.apply(SUPPLIER_RECEIVABLE, true, 60).unwrap();
    positions.apply(SELLER_FEES, true, 7).unwrap();
    positions.apply(SELLER_FEES_PAID, true, 3).unwrap();
    positions.apply(SELLER_FEE_REFUNDS, true, 2).unwrap();
    positions.apply(SELLER_FEE_CREDITS_RECEIVED, true, 1).unwrap();
    assert_eq!((positions.unpaid_supplier, positions.supplier_receivable), (100, 60));
    assert_eq!((positions.seller_fees, positions.seller_fees_paid), (7, 3));
    assert_eq!((positions.seller_fee_refunds, positions.seller_fee_credits_received), (2, 1));
}
