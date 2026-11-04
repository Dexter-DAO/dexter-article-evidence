//! Reconstructed contract checks. No wallet, token CPI or transaction is executed.
use anchor_lang::prelude::*;
use dexter_contracts::{commercial::*, financing::{Asset, FinanceError}, CORE_ID};
use solana_sha256_hasher::hashv;

fn key(n: u8) -> Pubkey { Pubkey::new_from_array([n; 32]) }
fn asset() -> Asset { Asset { mint: key(2), token_program: key(3) } }
fn seller() -> SellerFeeTerms {
    SellerFeeTerms { version: COMMERCIAL_VERSION, agreement_hash: [1; 32], supplier: key(4), asset: asset(),
        gross_cap: 100_000, rate_bps: 137, fixed_fee: 7, aggregate_fee_cap: 1_000,
        maximum_deduction_bps: 10_000, beneficiary: key(5), beneficiary_wallet: key(6),
        destination: key(7), failure_policy: FeeFailurePolicy::WaiveUnavailable }
}
fn interest() -> InterestTerms {
    InterestTerms { version: COMMERCIAL_VERSION, annual_rate_bps: 1_000, seconds_per_year: 100,
        maximum_interest: 10_000, accrual_cutoff: 1_000_000, protocol_share_bps: 3_700,
        protocol_beneficiary: key(8), protocol_destination: key(9),
        repayment_priority: RepaymentPriority::InterestFirst, stop_on_shortfall: false }
}
fn quote() -> SponsorQuote {
    SponsorQuote { version: COMMERCIAL_VERSION, operation_id: [10; 32], funding_source: SponsorFundingSource::OwnerCash,
        payer_principal: key(11), payer_authority: key(12), authority_generation: 3, asset: asset(),
        from: key(13), destination: key(14), sponsor_principal: key(15), rent_owner: key(16), approved_at: 100,
        expires_at: 200, native_cost_lamports: 101, conversion_numerator: 3, conversion_denominator: 2,
        reimbursement: 152, margin: 7, maximum_total_charge: 159, maximum_margin: 7, price_quote_hash: [17; 32] }
}
fn productive() -> ProductiveBackingPolicy {
    ProductiveBackingPolicy { version: COMMERCIAL_VERSION, asset: asset(), beneficial_owner: key(20), income_owner: key(21),
        loss_bearer: key(22), adapter_program: key(23), withdrawal_destination: key(24), ownership_policy_hash: [25; 32],
        liquidity_policy_hash: [26; 32], loss_policy_hash: [27; 32], minimum_liquid_cash: 50,
        redemption_window_seconds: 10, maximum_valuation_age_seconds: 30, valuation_haircut_bps: 1_000 }
}

#[test]
fn split_supplier_settlement_has_one_fixed_fee_and_cumulative_rounding() {
    let terms = seller();
    let (single, all) = SellerFeeState::default().settle(60_000, &terms, true).unwrap();
    let (first, a) = SellerFeeState::default().settle(30_000, &terms, true).unwrap();
    let (split, b) = first.settle(30_000, &terms, true).unwrap();
    assert_eq!((a.fee, b.fee, all.fee), (418, 411, 829));
    assert_eq!(split, single);
    assert_eq!(a.supplier_net + b.supplier_net, 59_171);
    let mut fine = SellerFeeState::default();
    for _ in 0..600 { fine = fine.settle(100, &terms, true).unwrap().0; }
    assert_eq!(fine, single);
}

#[test]
fn every_partial_collection_preserves_gross_and_accepted_deduction_bound() {
    for rate in [0, 1, 137, 2_500, 10_000] {
        for maximum in [0, 1, 100, 5_000, 10_000] {
            let mut terms = seller(); terms.rate_bps = rate; terms.maximum_deduction_bps = maximum;
            let mut state = SellerFeeState::default();
            for gross in [1, 2, 17, 999, 3, 5_001, 27_977] {
                let (next, split) = state.settle(gross, &terms, true).unwrap();
                assert_eq!(split.supplier_net + split.fee, gross);
                let maximum_deduction = (gross as u128 * maximum as u128 + 9_999) / 10_000;
                assert!(split.fee as u128 <= maximum_deduction);
                assert!(next.collected <= terms.aggregate_fee_cap);
                state = next;
            }
            let once = SellerFeeState::default().settle(state.gross_settled, &terms, true).unwrap().0;
            assert_eq!(state, once);
        }
    }
}

#[test]
fn unavailable_fee_is_waived_permanently_and_does_not_block_supplier() {
    let terms = seller();
    let (first, a) = SellerFeeState::default().settle(30_000, &terms, false).unwrap();
    assert_eq!((a.supplier_net, a.fee, a.waived), (30_000, 0, 418));
    let (final_state, b) = first.settle(30_000, &terms, true).unwrap();
    assert_eq!((b.fee, b.waived), (411, 0));
    assert_eq!((final_state.collected, final_state.waived), (411, 418));
}

#[test]
fn seller_fee_returns_are_bounded_and_never_refill_gross_cap() {
    let mut terms = seller(); terms.aggregate_fee_cap = 829;
    let state = SellerFeeState::default().settle(60_000, &terms, true).unwrap().0;
    let returned = state.credit(200, &terms).unwrap().refund(218, &terms).unwrap().credit(411, &terms).unwrap();
    assert_eq!((returned.refunded, returned.credited), (218, 611));
    assert!(returned.credit(1, &terms).is_err());
    let (later, settlement) = returned.settle(40_000, &terms, true).unwrap();
    assert_eq!(settlement.fee, 0);
    assert_eq!(later.collected, 829);
    assert_eq!(later.gross_settled, 100_000);
    assert!(later.settle(1, &terms, true).is_err());
}

#[test]
fn seller_parent_allowance_covers_multiple_entitlements() {
    let first = SellerFeeCapUsage::default().allocate(3, 5).unwrap();
    assert_eq!(first.allocate(3, 5), Err(FinanceError::LimitExceeded));
    let full = first.allocate(2, 5).unwrap();
    assert_eq!(full.allocated, 5);
    assert_eq!(full.allocate(1, 5), Err(FinanceError::LimitExceeded));
    assert_eq!(SellerFeeCapUsage { allocated: u64::MAX }.allocate(1, u64::MAX), Err(FinanceError::Arithmetic));
}

#[test]
fn seller_zero_cap_and_extreme_values_have_explicit_bounded_meaning() {
    let mut terms = seller(); terms.aggregate_fee_cap = 0;
    assert_eq!(SellerFeeState::default().settle(1, &terms, true).unwrap().1.fee, 0);
    terms.gross_cap = u64::MAX; terms.aggregate_fee_cap = u64::MAX; terms.fixed_fee = u64::MAX;
    terms.rate_bps = 10_000;
    let (state, settlement) = SellerFeeState::default().settle(u64::MAX, &terms, true).unwrap();
    assert_eq!(state.collected, u64::MAX); assert_eq!(settlement.supplier_net, 0);
    assert!(state.settle(1, &terms, true).is_err());
}

#[test]
fn seller_hash_binds_accepted_destination_supplier_asset_and_caps() {
    let terms = seller(); let original = terms.digest().unwrap();
    let mut changed = terms.clone(); changed.destination = key(90); assert_ne!(original, changed.digest().unwrap());
    changed = terms.clone(); changed.supplier = key(91); assert_ne!(original, changed.digest().unwrap());
    changed = terms.clone(); changed.asset.mint = key(92); assert_ne!(original, changed.digest().unwrap());
    changed = terms.clone(); changed.aggregate_fee_cap += 1; assert_ne!(original, changed.digest().unwrap());
    changed = terms; changed.maximum_deduction_bps = 10_001; assert!(changed.validate().is_err());
}

#[test]
fn interest_fraction_carry_matches_one_accrual_for_the_same_time_and_principal() {
    let terms = interest();
    let (single, _) = InterestState::new(0, &terms).unwrap().accrue(123, 99, &terms).unwrap();
    let mut partitioned = InterestState::new(0, &terms).unwrap();
    for now in 1..=99 { partitioned = partitioned.accrue(123, now, &terms).unwrap().0; }
    assert_eq!(single, partitioned);
    assert_eq!(single.accrued, 12);
    assert_eq!(single.fractional_numerator, 177_000);
}

#[test]
fn accrued_interest_does_not_compound_and_principal_changes_keep_prior_fraction() {
    let terms = interest();
    let (first, _) = InterestState::new(0, &terms).unwrap().accrue(123, 17, &terms).unwrap();
    let (second, _) = first.accrue(67, 100, &terms).unwrap();
    // Independent rational numerator: 123*17 + 67*83, times 1000 / (10000*100).
    assert_eq!(second.accrued, 7);
    assert_eq!(second.fractional_numerator, 652_000);
    assert_eq!(second.collected, 0);
}

#[test]
fn finite_interest_cap_handles_maximum_principal_rate_and_clock_without_overflow() {
    let mut terms = interest(); terms.annual_rate_bps = u32::MAX; terms.seconds_per_year = u32::MAX;
    terms.maximum_interest = u64::MAX; terms.accrual_cutoff = i64::MAX;
    let (state, delta) = InterestState::new(i64::MIN, &terms).unwrap().accrue(u64::MAX, i64::MAX, &terms).unwrap();
    assert_eq!(delta, u64::MAX); assert_eq!(state.accrued, u64::MAX); assert_eq!(state.fractional_numerator, 0);
    let (collected, _) = state.collect(u64::MAX, &terms, true).unwrap();
    let (later, extra) = collected.accrue(u64::MAX, i64::MAX, &terms).unwrap();
    assert_eq!(extra, 0); assert_eq!(later.accrued, u64::MAX);
}

#[test]
fn cutoff_and_clock_guards_do_not_strand_work_earned_after_cutoff() {
    let mut terms = interest(); terms.accrual_cutoff = 50;
    let (state, delta) = InterestState::new(0, &terms).unwrap().accrue(1_000, 100, &terms).unwrap();
    assert_eq!(delta, 50); assert_eq!(state.last_accrued_at, 100);
    assert!(state.accrue(1_000, 99, &terms).is_err());
    let late = InterestState::new(101, &terms).unwrap().accrue(1_000, 200, &terms).unwrap();
    assert_eq!(late.1, 0); assert_eq!(late.0.accrued, 0);
}

#[test]
fn protocol_take_only_allocates_actual_collected_interest_with_cumulative_rounding() {
    let terms = interest();
    let accrued = InterestState::new(0, &terms).unwrap().accrue(60_000, 100, &terms).unwrap().0;
    assert_eq!((accrued.accrued, accrued.protocol_collected), (6_000, 0));
    let mut state = accrued; let mut protocol = 0; let mut lender = 0;
    for amount in [1, 2, 997, 2_000, 3_000] {
        let (next, allocation) = state.collect(amount, &terms, true).unwrap();
        assert_eq!(allocation.protocol + allocation.lender, amount);
        protocol += allocation.protocol; lender += allocation.lender; state = next;
    }
    assert_eq!((protocol, lender, state.outstanding().unwrap()), (2_220, 3_780, 0));
    assert!(state.collect(1, &terms, true).is_err());
}

#[test]
fn unavailable_protocol_take_stays_with_lender_and_cannot_reappear() {
    let terms = interest();
    let accrued = InterestState::new(0, &terms).unwrap().accrue(10_000, 100, &terms).unwrap().0;
    let (state, first) = accrued.collect(400, &terms, false).unwrap();
    assert_eq!((first.protocol, first.protocol_waived, first.lender), (0, 148, 400));
    let (state, second) = state.collect(600, &terms, true).unwrap();
    assert_eq!((second.protocol, second.lender), (222, 378));
    assert_eq!((state.protocol_collected, state.protocol_waived), (222, 148));
}

#[test]
fn repayment_priority_is_explicit_and_principal_is_not_protocol_income() {
    let a = split_repayment(105, 100, 10, RepaymentPriority::InterestFirst).unwrap();
    let b = split_repayment(105, 100, 10, RepaymentPriority::PrincipalFirst).unwrap();
    assert_eq!((a.principal, a.interest), (95, 10));
    assert_eq!((b.principal, b.interest), (100, 5));
    assert!(split_repayment(111, 100, 10, RepaymentPriority::PrincipalFirst).is_err());
    assert_eq!(split_repayment(u64::MAX, u64::MAX, u64::MAX, RepaymentPriority::InterestFirst).unwrap().interest, u64::MAX);
}

#[test]
fn interest_waiver_never_refills_authorized_lifetime_cap() {
    let mut terms = interest(); terms.maximum_interest = 100;
    let state = InterestState::new(0, &terms).unwrap().accrue(10_000, 100, &terms).unwrap().0;
    let waived = state.waive(100, &terms).unwrap();
    assert_eq!(waived.outstanding().unwrap(), 0);
    assert_eq!(waived.accrue(10_000, 200, &terms).unwrap().1, 0);
    assert!(waived.waive(1, &terms).is_err());
}

#[test]
fn unsupported_shortfall_policy_is_rejected_and_rates_are_not_defaulted() {
    let mut terms = interest(); terms.stop_on_shortfall = true;
    assert_eq!(InterestState::new(0, &terms), Err(FinanceError::WrongTerms));
    terms.stop_on_shortfall = false; terms.seconds_per_year = 0; assert!(terms.validate().is_err());
    terms = interest(); terms.protocol_share_bps = 10_001; assert!(terms.validate().is_err());
    terms = interest(); terms.annual_rate_bps = 0;
    assert_eq!(InterestState::new(0, &terms).unwrap().accrue(u64::MAX, 100, &terms).unwrap().1, 0);
}

#[test]
fn sponsorship_quotes_bind_conversion_total_margin_and_expiry() {
    let q = quote(); q.validate_at(100).unwrap(); q.validate_at(200).unwrap();
    assert_eq!(q.total().unwrap(), 159); assert!(q.validate_at(99).is_err()); assert!(q.validate_at(201).is_err());
    let mut bad = q.clone(); bad.reimbursement = 151; assert!(bad.validate_shape().is_err());
    bad = q.clone(); bad.margin = 8; assert!(bad.validate_shape().is_err());
    bad = q.clone(); bad.maximum_total_charge = 158; assert!(bad.validate_shape().is_err());
    bad = q; bad.conversion_denominator = 0; assert!(bad.validate_shape().is_err());
}

#[test]
fn sponsorship_hash_separates_operation_authority_route_and_funding_source() {
    let q = quote(); let expected = q.digest().unwrap();
    let mut changed = q.clone(); changed.operation_id[0] ^= 1; assert_ne!(expected, changed.digest().unwrap());
    changed = q.clone(); changed.authority_generation += 1; assert_ne!(expected, changed.digest().unwrap());
    changed = q.clone(); changed.payer_authority = key(91); assert_ne!(expected, changed.digest().unwrap());
    changed = q.clone(); changed.from = key(92); assert_ne!(expected, changed.digest().unwrap());
    changed = q.clone(); changed.rent_owner = key(93); assert_ne!(expected, changed.digest().unwrap());
    changed = q; changed.funding_source = SponsorFundingSource::EarnedSupplierProceeds;
    assert_ne!(expected, changed.digest().unwrap());
    let mut cash_bytes = Vec::new(); SponsorFundingSource::OwnerCash.serialize(&mut cash_bytes).unwrap();
    let mut proceeds_bytes = Vec::new(); SponsorFundingSource::EarnedSupplierProceeds.serialize(&mut proceeds_bytes).unwrap();
    assert_eq!(cash_bytes, vec![0]);
    assert_eq!(proceeds_bytes, vec![1]);
}

#[test]
fn sponsorship_overflow_is_refused_instead_of_capping_a_charge_silently() {
    let mut q = quote(); q.native_cost_lamports = u64::MAX; q.conversion_numerator = u64::MAX;
    q.conversion_denominator = 1; assert_eq!(q.validate_shape(), Err(FinanceError::Arithmetic));
    q = quote(); q.reimbursement = u64::MAX; q.margin = 1; assert_eq!(q.total(), Err(FinanceError::Arithmetic));
}

#[test]
fn productive_backing_has_named_ownership_loss_and_staleness_rules() {
    let policy = productive(); policy.validate().unwrap();
    assert_eq!(policy.eligible_redemption_value(1_000, 100, 130).unwrap(), 900);
    assert!(policy.eligible_redemption_value(1_000, 100, 131).is_err());
    assert!(policy.eligible_redemption_value(1_000, 101, 100).is_err());
    let mut changed = policy.clone(); changed.loss_bearer = key(95);
    assert_ne!(policy.digest().unwrap(), changed.digest().unwrap());
    changed.loss_policy_hash = [0; 32]; assert!(changed.validate().is_err());
}

#[test]
fn productive_income_separates_realized_loss_owner_income_and_protocol_take() {
    let positive = allocate_realized_backing_income(1_000, 200, 2_500).unwrap();
    assert_eq!((positive.net_income, positive.uncovered_loss, positive.protocol, positive.owner), (800, 0, 200, 600));
    let negative = allocate_realized_backing_income(200, 1_000, 2_500).unwrap();
    assert_eq!((negative.net_income, negative.uncovered_loss, negative.protocol, negative.owner), (0, 800, 0, 0));
    assert!(allocate_realized_backing_income(1, 0, 10_001).is_err());
}

#[test]
fn assignment_policy_has_one_exact_core_and_domain_bound_meaning() {
    let domain = [99; 32];
    assert_eq!(within_domain_assignment_policy(&domain), hashv(&[
        b"DEXTER:WITHIN-DOMAIN-WHOLE-RESIDUAL:1", CORE_ID.as_ref(), &domain]).to_bytes());
    assert_ne!(within_domain_assignment_policy(&domain), within_domain_assignment_policy(&[100; 32]));
    assert_ne!(within_domain_assignment_policy(&domain), [0; 32]);
}
