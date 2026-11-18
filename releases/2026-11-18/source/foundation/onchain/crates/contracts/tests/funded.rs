//! Newly executed journal arithmetic, canonical hash and cross-program reader checks.
use anchor_lang::prelude::*;
use dexter_contracts::{commercial::SponsorFundingSource, funded::*, CORE_ID};
use solana_sha256_hasher::hashv;

fn key(n: u8) -> Pubkey { Pubkey::new_from_array([n; 32]) }
fn receipt(effects: usize) -> (Pubkey, FinanceReceiptData) {
    let principal = key(1); let mint = key(2); let operation_id = [3; 32];
    let address = receipt_address(&principal, &operation_id);
    let mut data = FinanceReceiptData { version: FUNDED_VERSION, principal, mint, operation_id,
        kind: FINANCE_EARN, amount: 100, ..FinanceReceiptData::default() };
    for i in 0..effects {
        let principal = key(10 + i as u8); let book = book_address(&principal, &mint);
        let mut effect = FinanceBookEffect { book, principal, sequence: 1, previous_hash: [0; 32],
            before: FinancialPositions::default(), after: FinancialPositions { cash_received: 100, ..FinancialPositions::default() }, hash: [0; 32] };
        effect.hash = book_effect_hash(&address, &mint, &effect).unwrap(); data.effects.push(effect);
    }
    data.effects_hash = financial_receipt_hash(&data).unwrap(); (address, data)
}
fn account_bytes<T: AnchorSerialize>(name: &[u8], data: &T, allocated: Option<usize>) -> Vec<u8> {
    let mut bytes = hashv(&[name]).to_bytes()[..8].to_vec(); data.serialize(&mut bytes).unwrap();
    if let Some(size) = allocated { assert!(bytes.len() <= size); bytes.resize(size, 0); }
    bytes
}
fn read_receipt(address: Pubkey, owner: Pubkey, mut bytes: Vec<u8>) -> Result<FinanceReceiptData> {
    let mut lamports = 1;
    let account = AccountInfo::new(&address, false, false, &mut lamports, &mut bytes, &owner, false);
    read_finance_receipt(&account)
}
fn read_book(address: Pubkey, owner: Pubkey, mut bytes: Vec<u8>) -> Result<FinanceBookData> {
    let mut lamports = 1;
    let account = AccountInfo::new(&address, false, false, &mut lamports, &mut bytes, &owner, false);
    read_finance_book(&account)
}

#[test]
fn receipt_hash_matches_independent_canonical_borsh_for_zero_through_eight_books() {
    for count in 0..=MAX_FINANCIAL_BOOKS {
        let (address, mut data) = receipt(count);
        for source in [None, Some(SponsorFundingSource::OwnerCash), Some(SponsorFundingSource::EarnedSupplierProceeds)] {
            data.sponsor_funding_source = source; data.effects_hash = financial_receipt_hash(&data).unwrap();
            let mut canonical = data.clone(); canonical.effects_hash = [0; 32];
            let mut canonical_bytes = Vec::new(); canonical.serialize(&mut canonical_bytes).unwrap();
            let expected = hashv(&[b"DEXTER:FINANCIAL-RECEIPT:1", CORE_ID.as_ref(), &canonical_bytes]).to_bytes();
            assert_eq!(data.effects_hash, expected);
            let minimal = account_bytes(b"account:FinanceReceipt", &data, None);
            assert_eq!(financial_receipt_hash_from_serialized_body(&minimal[8..]).unwrap(), expected);
            assert_eq!(financial_receipt_hash_from_serialized_body(&canonical_bytes).unwrap(), expected);
            assert_eq!(read_receipt(address, CORE_ID, minimal).unwrap(), data);
            let padded = account_bytes(b"account:FinanceReceipt", &data, Some(8 + FinanceReceiptData::INIT_SPACE));
            assert_eq!(read_receipt(address, CORE_ID, padded).unwrap(), data);
        }
    }
}

#[test]
fn receipt_body_hash_excludes_only_its_hash_suffix_and_never_allocated_padding() {
    let (_, data) = receipt(7);
    let original = financial_receipt_hash(&data).unwrap();
    let mut bytes = account_bytes(b"account:FinanceReceipt", &data, None);
    let hash_offset = bytes.len() - 32;
    bytes[hash_offset..].fill(91);
    assert_eq!(financial_receipt_hash_from_serialized_body(&bytes[8..]).unwrap(), original);
    // The body helper deliberately takes an exact consumed body. Its callers
    // must never include the discriminator or the unused account allocation.
    assert_ne!(financial_receipt_hash_from_serialized_body(&bytes).unwrap(), original);
    let mut padded_body = bytes[8..].to_vec(); padded_body.push(0);
    assert_ne!(financial_receipt_hash_from_serialized_body(&padded_body).unwrap(), original);
    assert!(financial_receipt_hash_from_serialized_body(&[0; 31]).is_err());
    assert!(financial_receipt_hash_from_serialized_body(&vec![0; FinanceReceiptData::INIT_SPACE + 1]).is_err());
}

#[test]
fn fixed_effect_hash_matches_the_original_generic_preimage_for_every_position() {
    let (address, data) = receipt(1);
    for field in 0..=SELLER_FEE_CREDITS_RECEIVED {
        for value in [0, 1, 123_456_789, u64::MAX] {
            let mut effect = data.effects[0].clone();
            effect.sequence = value;
            effect.previous_hash = [field; 32];
            effect.before = FinancialPositions::default();
            effect.after = FinancialPositions::default();
            effect.before.apply(field, true, value).unwrap();
            effect.after.apply(field, true, value.wrapping_add(17)).unwrap();
            let preimage = (address, data.mint, effect.book, effect.principal,
                effect.sequence, effect.previous_hash, effect.before, effect.after);
            let mut bytes = Vec::new(); preimage.serialize(&mut bytes).unwrap();
            assert_eq!(bytes.len(), 552);
            let original = financial_digest(b"DEXTER:FINANCE-BOOK-EFFECT:1", &preimage).unwrap();
            assert_eq!(book_effect_hash(&address, &data.mint, &effect).unwrap(), original);
            effect.hash = [42; 32]; // The effect's own hash is not in its preimage.
            assert_eq!(book_effect_hash(&address, &data.mint, &effect).unwrap(), original);
            assert_ne!(book_effect_hash(&key(70), &data.mint, &effect).unwrap(), original);
            assert_ne!(book_effect_hash(&address, &key(71), &effect).unwrap(), original);
        }
    }
}

#[test]
fn borrowed_receipt_hash_rejects_noncanonical_tags_truncation_and_nonzero_padding() {
    let (address, data) = receipt(1);
    let minimal = account_bytes(b"account:FinanceReceipt", &data, None);
    let mut truncated = minimal.clone(); truncated.pop();
    assert!(read_receipt(address, CORE_ID, truncated).is_err());
    let mut padded = account_bytes(b"account:FinanceReceipt", &data, Some(8 + FinanceReceiptData::INIT_SPACE));
    *padded.last_mut().unwrap() = 1;
    assert!(read_receipt(address, CORE_ID, padded).is_err());

    let mut sponsored = data.clone();
    sponsored.sponsor_funding_source = Some(SponsorFundingSource::OwnerCash);
    sponsored.effects_hash = financial_receipt_hash(&sponsored).unwrap();
    let with_source = account_bytes(b"account:FinanceReceipt", &sponsored, None);
    // Locate the Option discriminant by comparing equivalent None/Some
    // serializations, rather than duplicating an unrelated layout offset.
    let option_offset = minimal.iter().zip(&with_source).position(|(a, b)| a != b).unwrap();
    assert_eq!((minimal[option_offset], with_source[option_offset], with_source[option_offset + 1]), (0, 1, 0));
    let mut wrong_option = with_source.clone(); wrong_option[option_offset] = 2;
    assert!(read_receipt(address, CORE_ID, wrong_option).is_err());
    let mut wrong_enum = with_source; wrong_enum[option_offset + 1] = 2;
    assert!(read_receipt(address, CORE_ID, wrong_enum).is_err());
}

#[test]
fn receipt_reader_authenticates_owner_address_type_outer_hash_and_each_inner_effect() {
    let (address, data) = receipt(2);
    let bytes = account_bytes(b"account:FinanceReceipt", &data, None);
    assert!(read_receipt(address, key(90), bytes.clone()).is_err());
    assert!(read_receipt(key(91), CORE_ID, bytes.clone()).is_err());
    let mut wrong_type = bytes; wrong_type[0] ^= 1; assert!(read_receipt(address, CORE_ID, wrong_type).is_err());
    let mut changed = data.clone(); changed.supplier_net += 1;
    assert!(read_receipt(address, CORE_ID, account_bytes(b"account:FinanceReceipt", &changed, None)).is_err());
    changed = data.clone(); changed.effects[0].after.cash_received += 1;
    changed.effects_hash = financial_receipt_hash(&changed).unwrap();
    assert!(read_receipt(address, CORE_ID, account_bytes(b"account:FinanceReceipt", &changed, None)).is_err());
    changed = data; changed.effects[1] = changed.effects[0].clone();
    changed.effects_hash = financial_receipt_hash(&changed).unwrap();
    assert!(read_receipt(address, CORE_ID, account_bytes(b"account:FinanceReceipt", &changed, None)).is_err());
}

#[test]
fn receipt_capacity_is_a_schema_bound_and_not_a_transaction_packet_limit() {
    let (address, mut data) = receipt(MAX_FINANCIAL_BOOKS);
    data.effects.push(data.effects[0].clone());
    assert!(financial_receipt_hash(&data).is_err());
    assert!(read_receipt(address, CORE_ID, account_bytes(b"account:FinanceReceipt", &data, None)).is_err());
    let (address, data) = receipt(0);
    let mut bytes = account_bytes(b"account:FinanceReceipt", &data, Some(8 + FinanceReceiptData::INIT_SPACE));
    bytes.push(0); assert!(read_receipt(address, CORE_ID, bytes).is_err());
}

#[test]
fn book_reader_checks_canonical_bump_and_exact_fixed_layout() {
    let principal = key(1); let mint = key(2);
    let (address, bump) = Pubkey::find_program_address(&[b"financial-book", principal.as_ref(), mint.as_ref()], &CORE_ID);
    let book = FinanceBookData { version: FUNDED_VERSION, bump, principal, mint, ..FinanceBookData::default() };
    let bytes = account_bytes(b"account:FinanceBook", &book, None);
    assert_eq!(read_book(address, CORE_ID, bytes.clone()).unwrap(), book);
    let mut wrong = book; wrong.bump ^= 1;
    assert!(read_book(address, CORE_ID, account_bytes(b"account:FinanceBook", &wrong, None)).is_err());
    let mut oversized = bytes; oversized.push(0); assert!(read_book(address, CORE_ID, oversized).is_err());
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

#[test]
fn sponsorship_and_capital_token_legs_are_committed_as_separate_facts() {
    let (_, data) = receipt(1); let hash = data.effects_hash;
    let mut changed = data.clone(); changed.capital_allocated = 1_000;
    assert_ne!(hash, financial_receipt_hash(&changed).unwrap());
    let allocation = financial_receipt_hash(&changed).unwrap(); changed.capital_token_inflow = 1_000;
    assert_ne!(allocation, financial_receipt_hash(&changed).unwrap());
    let inflow = financial_receipt_hash(&changed).unwrap(); changed.capital_authority = key(80);
    changed.capital_from = key(81); changed.capital_owner_generation = 2;
    assert_ne!(inflow, financial_receipt_hash(&changed).unwrap());
    changed = data; changed.sponsor_from = key(90); changed.sponsor_payer_authority = key(91);
    assert_ne!(hash, financial_receipt_hash(&changed).unwrap());
}

#[test]
fn title_and_repayment_routes_remain_historical_receipt_facts() {
    let (_, mut data) = receipt(2);
    data.previous_creditor = key(70); data.current_creditor = key(71);
    data.ownership_revision = 4; data.creditor_destination = key(72);
    data.assignment_consideration = 57_000; data.assignment_acquisition_cost = 100;
    data.previous_acquisition_basis = 60_000; data.new_acquisition_basis = 57_100;
    data.debit_principal = key(71); data.debit_authority = key(73); data.debit_from = key(74);
    data.assignment_cost_beneficiary = key(75); data.assignment_cost_destination = key(76);
    let accepted = financial_receipt_hash(&data).unwrap();
    let mut changed = data.clone(); changed.current_creditor = key(77);
    assert_ne!(accepted, financial_receipt_hash(&changed).unwrap());
    changed = data.clone(); changed.ownership_revision += 1;
    assert_ne!(accepted, financial_receipt_hash(&changed).unwrap());
    changed = data.clone(); changed.creditor_destination = key(78);
    assert_ne!(accepted, financial_receipt_hash(&changed).unwrap());
    changed = data.clone(); changed.assignment_acquisition_cost = 101;
    assert_ne!(accepted, financial_receipt_hash(&changed).unwrap());
    changed = data.clone(); changed.debit_from = key(79);
    assert_ne!(accepted, financial_receipt_hash(&changed).unwrap());
    changed = data; changed.previous_acquisition_basis -= 1;
    assert_ne!(accepted, financial_receipt_hash(&changed).unwrap());
}
