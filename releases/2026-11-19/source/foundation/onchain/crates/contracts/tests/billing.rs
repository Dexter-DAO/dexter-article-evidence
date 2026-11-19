//! Pure contract checks. Signature authentication, immutable record admission,
//! revocation, release ordering and token effects require the native Core tests.
use anchor_lang::prelude::*;
use dexter_contracts::{billing::*, financing::FinanceError, CORE_ID};
use solana_sha256_hasher::hashv;

fn key(n: u8) -> Pubkey { Pubkey::new_from_array([n; 32]) }

fn policy() -> BillingPolicy {
    BillingPolicy {
        schema_version: BILLING_POLICY_VERSION,
        source: key(2), source_identity: [5; 32], work_id: [6; 32], request_hash: [7; 32],
        work_until: 200, evidence_deadline: 300, claim_deadline: 350, metric_count: 3,
        metric_ids: [[11; 32], [12; 32], [13; 32], [0; 32], [0; 32], [0; 32], [0; 32], [0; 32]],
        price_numerators: [1, 3, 0, 0, 0, 0, 0, 0],
        price_denominators: [3, 2, 1, 0, 0, 0, 0, 0],
        max_units: [10, 100, 5, 0, 0, 0, 0, 0],
    }
}

fn one_metric(numerator: u64, denominator: u64, maximum: u64) -> BillingPolicy {
    let mut value = policy();
    value.metric_count = 1;
    value.metric_ids = [[0; 32]; MAX_BILLING_METRICS];
    value.metric_ids[0] = [11; 32];
    value.price_numerators = [0; MAX_BILLING_METRICS];
    value.price_denominators = [0; MAX_BILLING_METRICS];
    value.max_units = [0; MAX_BILLING_METRICS];
    value.price_numerators[0] = numerator;
    value.price_denominators[0] = denominator;
    value.max_units[0] = maximum;
    value
}

fn report() -> SourceReport {
    SourceReport {
        record_id: [21; 32], request_hash: [7; 32], started_at: 110, completed_at: 180,
        units: [2, 3, 4, 0, 0, 0, 0, 0], result_hash: [22; 32],
        finality: FINAL_FOR_BILLING, outcome: COMPLETED_WORK,
    }
}

fn claim() -> SellerClaim { SellerClaim { claim_id: [23; 32], amount: 5 } }

fn hex(value: [u8; 32]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn encoded<T: AnchorSerialize>(value: &T) -> Vec<u8> {
    let mut bytes = Vec::new(); value.serialize(&mut bytes).unwrap(); bytes
}

#[test]
fn policy_admission_requires_named_work_source_and_a_finite_time_window() {
    let valid = policy();
    assert_eq!(valid.validate(100), Ok(()));
    assert_eq!(valid.validate(199), Ok(()));
    assert_eq!(valid.validate(200), Err(FinanceError::WrongTerms));
    assert_eq!(valid.validate(-1), Err(FinanceError::WrongTerms));

    let mut changed = valid.clone(); changed.schema_version = 2;
    assert_eq!(changed.validate(100), Err(FinanceError::SchemaVersion));
    changed = valid.clone(); changed.source = Pubkey::default();
    assert_eq!(changed.validate(100), Err(FinanceError::WrongBinding));
    changed = valid.clone(); changed.source_identity = [0; 32];
    assert_eq!(changed.validate(100), Err(FinanceError::WrongBinding));
    changed = valid.clone(); changed.work_id = [0; 32];
    assert_eq!(changed.validate(100), Err(FinanceError::WrongBinding));
    changed = valid.clone(); changed.request_hash = [0; 32];
    assert_eq!(changed.validate(100), Err(FinanceError::WrongBinding));

    changed = valid.clone(); changed.evidence_deadline = 199;
    assert_eq!(changed.validate(100), Err(FinanceError::WrongTerms));
    changed = valid.clone(); changed.claim_deadline = 199;
    assert_eq!(changed.validate(100), Err(FinanceError::WrongTerms));
    changed = valid.clone(); changed.evidence_deadline = i64::MAX;
    assert_eq!(changed.validate(100), Err(FinanceError::WrongTerms));
    changed = valid; changed.claim_deadline = i64::MAX;
    assert_eq!(changed.validate(100), Err(FinanceError::WrongTerms));

    // The latest finite window still has a representable finalization instant.
    changed.work_until = i64::MAX - 2;
    changed.evidence_deadline = i64::MAX - 1;
    changed.claim_deadline = i64::MAX - 1;
    assert_eq!(changed.validate(i64::MAX - 3), Ok(()));
    assert!(changed.finalization_ready(i64::MAX));
}

#[test]
fn tariff_rejects_ambiguous_metrics_and_noncanonical_unused_slots() {
    let valid = policy();
    for count in [0, 9, u8::MAX] {
        let mut changed = valid.clone(); changed.metric_count = count;
        assert_eq!(changed.validate(100), Err(FinanceError::WrongTerms));
    }
    let mut changed = valid.clone(); changed.metric_ids[1] = changed.metric_ids[0];
    assert_eq!(changed.validate(100), Err(FinanceError::WrongTerms));
    changed = valid.clone(); changed.metric_ids[0] = [0; 32];
    assert_eq!(changed.validate(100), Err(FinanceError::WrongTerms));
    changed = valid.clone(); changed.price_denominators[1] = 0;
    assert_eq!(changed.validate(100), Err(FinanceError::WrongTerms));
    changed = valid.clone(); changed.max_units[1] = 0;
    assert_eq!(changed.validate(100), Err(FinanceError::WrongTerms));
    changed = valid.clone(); changed.price_numerators.fill(0);
    assert_eq!(changed.validate(100), Err(FinanceError::WrongTerms));

    changed = valid.clone(); changed.metric_ids[3] = [14; 32];
    assert_eq!(changed.validate(100), Err(FinanceError::WrongTerms));
    changed = valid.clone(); changed.price_numerators[3] = 1;
    assert_eq!(changed.validate(100), Err(FinanceError::WrongTerms));
    changed = valid.clone(); changed.price_denominators[3] = 1;
    assert_eq!(changed.validate(100), Err(FinanceError::WrongTerms));
    changed = valid; changed.max_units[3] = 1;
    assert_eq!(changed.validate(100), Err(FinanceError::WrongTerms));
}

#[test]
fn every_metric_rounds_up_separately_with_no_charge_for_zero_or_free_units() {
    let terms = policy();
    // ceil(2/3) + ceil(3*3/2) + 4*0 = 1 + 5 + 0.
    assert_eq!(terms.supported_amount(&report().units), Ok(6));
    assert_eq!(terms.supported_amount(&[0; 8]), Ok(0));
    assert_eq!(terms.supported_amount(&[0, 0, 5, 0, 0, 0, 0, 0]), Ok(0));
    assert_eq!(terms.supported_amount(&[10, 100, 5, 0, 0, 0, 0, 0]), Ok(154));

    let mut thirds = terms;
    thirds.price_numerators = [1, 1, 0, 0, 0, 0, 0, 0];
    thirds.price_denominators = [3, 3, 1, 0, 0, 0, 0, 0];
    // Rounding the combined 2/3 once would incorrectly establish one atom.
    assert_eq!(thirds.supported_amount(&[1, 1, 0, 0, 0, 0, 0, 0]), Ok(2));
    assert_eq!(thirds.supported_amount(&[3, 3, 0, 0, 0, 0, 0, 0]), Ok(2));
}

#[test]
fn small_rational_tariffs_match_independent_integer_ceiling_and_limits() {
    for numerator in [1, 2, 7, 31] {
        for denominator in [1, 2, 3, 17, 100] {
            let terms = one_metric(numerator, denominator, 100);
            for quantity in 0..=100 {
                let mut units = [0; 8]; units[0] = quantity;
                let expected = (quantity * numerator + denominator - 1) / denominator;
                assert_eq!(terms.supported_amount(&units), Ok(expected as u128));
            }
            assert_eq!(terms.supported_amount(&[101, 0, 0, 0, 0, 0, 0, 0]), Err(FinanceError::LimitExceeded));
            assert_eq!(terms.supported_amount(&[1, 1, 0, 0, 0, 0, 0, 0]), Err(FinanceError::WrongTerms));
        }
    }
}

#[test]
fn admitted_maxima_cannot_overflow_the_full_u128_aggregate() {
    let maximum = one_metric(u64::MAX, 1, u64::MAX);
    assert_eq!(maximum.validate(100), Ok(()));
    let expected = (u64::MAX as u128) * (u64::MAX as u128);
    assert_eq!(maximum.supported_amount(&[u64::MAX, 0, 0, 0, 0, 0, 0, 0]), Ok(expected));

    let mut overflowing = maximum;
    overflowing.metric_count = 2;
    overflowing.metric_ids[1] = [12; 32];
    overflowing.price_numerators[1] = u64::MAX;
    overflowing.price_denominators[1] = 1;
    overflowing.max_units[1] = u64::MAX;
    // Refuse before admitting work, even when an eventual cap would be small.
    assert_eq!(overflowing.validate(100), Err(FinanceError::Arithmetic));
    assert_eq!(overflowing.supported_amount(&[1, 1, 0, 0, 0, 0, 0, 0]), Err(FinanceError::Arithmetic));

    let mut eight = policy();
    eight.metric_count = 8;
    for i in 0..8 {
        eight.metric_ids[i] = [i as u8 + 1; 32];
        eight.price_numerators[i] = 1;
        eight.price_denominators[i] = 1;
        eight.max_units[i] = u64::MAX;
    }
    assert_eq!(eight.validate(100), Ok(()));
    assert_eq!(eight.supported_amount(&[u64::MAX; 8]), Ok(8 * u64::MAX as u128));
}

#[test]
fn order_and_claim_bounds_apply_before_narrowing_large_supported_values() {
    let terms = one_metric(u64::MAX, 1, 2);
    let support = terms.supported_amount(&[2, 0, 0, 0, 0, 0, 0, 0]).unwrap();
    assert!(support > u64::MAX as u128);
    assert_eq!(deterministic_bill_amount(123, 456, support), 123);
    assert_eq!(deterministic_bill_amount(456, 123, support), 123);
    assert_eq!(deterministic_bill_amount(u64::MAX, u64::MAX, support), u64::MAX);
    assert_eq!(deterministic_bill_amount(2_000, 1_400, 900), 900);
    assert_eq!(deterministic_bill_amount(2_000, 400, 900), 400);
    assert_eq!(deterministic_bill_amount(2_000, 2_400, 3_000), 2_000);
    assert_eq!(deterministic_bill_amount(2_000, 1_400, 0), 0);
    assert_eq!(deterministic_bill_amount(2_000, 0, 900), 0);
    assert_eq!(deterministic_bill_amount(0, u64::MAX, u128::MAX), 0);
}

#[test]
fn report_requires_final_completed_bound_work_and_record_identity() {
    let terms = policy();
    let valid = report();
    assert_eq!(terms.validate_report(&valid, 100, 180), Ok(6));
    let mut changed = valid.clone(); changed.finality = 0;
    assert_eq!(terms.validate_report(&changed, 100, 180), Err(FinanceError::WrongTerms));
    changed = valid.clone(); changed.finality = 2;
    assert_eq!(terms.validate_report(&changed, 100, 180), Err(FinanceError::WrongTerms));
    changed = valid.clone(); changed.outcome = 0;
    assert_eq!(terms.validate_report(&changed, 100, 180), Err(FinanceError::WrongTerms));
    changed = valid.clone(); changed.outcome = 2;
    assert_eq!(terms.validate_report(&changed, 100, 180), Err(FinanceError::WrongTerms));
    changed = valid.clone(); changed.request_hash = [8; 32];
    assert_eq!(terms.validate_report(&changed, 100, 180), Err(FinanceError::WrongBinding));
    changed = valid.clone(); changed.record_id = [0; 32];
    assert_eq!(terms.validate_report(&changed, 100, 180), Err(FinanceError::WrongBinding));
    changed = valid.clone(); changed.result_hash = [0; 32];
    assert_eq!(terms.validate_report(&changed, 100, 180), Err(FinanceError::WrongBinding));
    changed = valid.clone(); changed.units[0] = 11;
    assert_eq!(terms.validate_report(&changed, 100, 180), Err(FinanceError::LimitExceeded));
    changed = valid; changed.units[3] = 1;
    assert_eq!(terms.validate_report(&changed, 100, 180), Err(FinanceError::WrongTerms));
}

#[test]
fn onchain_admission_time_cannot_be_replaced_by_a_backdated_source_timestamp() {
    let terms = policy();
    let valid = report();
    assert_eq!(terms.validate_report(&valid, 100, 300), Ok(6));
    // The unchanged signed report remains within its work window, but arrived late.
    assert_eq!(terms.validate_report(&valid, 100, 301), Err(FinanceError::WrongTerms));
    assert_eq!(terms.validate_report(&valid, 100, 179), Err(FinanceError::WrongTerms));
    assert_eq!(terms.validate_report(&valid, 100, 99), Err(FinanceError::WrongTerms));

    let mut changed = valid.clone(); changed.started_at = 99;
    assert_eq!(terms.validate_report(&changed, 100, 200), Err(FinanceError::WrongTerms));
    changed = valid.clone(); changed.completed_at = 201;
    assert_eq!(terms.validate_report(&changed, 100, 201), Err(FinanceError::WrongTerms));
    changed = valid.clone(); changed.completed_at = 109;
    assert_eq!(terms.validate_report(&changed, 100, 200), Err(FinanceError::WrongTerms));
    changed = valid; changed.started_at = 100; changed.completed_at = 200;
    assert_eq!(terms.validate_report(&changed, 100, 200), Ok(6));
}

#[test]
fn seller_claim_has_its_own_inclusive_deadline_and_may_claim_zero() {
    let terms = policy();
    let mut value = claim();
    assert_eq!(terms.validate_claim(&value, 100, 350), Ok(()));
    assert_eq!(terms.validate_claim(&value, 100, 351), Err(FinanceError::WrongTerms));
    assert_eq!(terms.validate_claim(&value, 100, 99), Err(FinanceError::WrongTerms));
    value.amount = 0;
    assert_eq!(terms.validate_claim(&value, 100, 100), Ok(()));
    value.amount = u64::MAX;
    assert_eq!(terms.validate_claim(&value, 100, 350), Ok(()));
    value.claim_id = [0; 32];
    assert_eq!(terms.validate_claim(&value, 100, 350), Err(FinanceError::WrongBinding));
}

#[test]
fn finalization_waits_for_both_windows_and_never_expires_timely_inputs() {
    let mut terms = policy();
    let support = terms.validate_report(&report(), 100, 300).unwrap();
    let seller = claim();
    terms.validate_claim(&seller, 100, 350).unwrap();
    assert_eq!(terms.finalization_deadline(), 350);
    assert!(!terms.finalization_ready(300));
    assert!(!terms.finalization_ready(350));
    for now in [351, 1_000_000, i64::MAX] {
        assert!(terms.finalization_ready(now));
        assert_eq!(deterministic_bill_amount(20, seller.amount, support), 5);
    }
    terms.evidence_deadline = 400;
    assert_eq!(terms.finalization_deadline(), 400);
    assert!(!terms.finalization_ready(400));
    assert!(terms.finalization_ready(401));
}

#[test]
fn canonical_hashes_match_independent_python_borsh_and_sha256_vectors() {
    // These constants were independently produced with Python struct.pack and
    // hashlib, using raw decoded CORE_ID bytes and no account discriminator.
    let terms = policy(); let source = report(); let seller = claim();
    assert_eq!(encoded(&terms).len(), 602);
    assert_eq!(BillingPolicy::INIT_SPACE, 602);
    assert_eq!(encoded(&source).len(), 178);
    assert_eq!(SourceReport::INIT_SPACE, 178);
    assert_eq!(encoded(&seller).len(), 40);
    assert_eq!(SellerClaim::INIT_SPACE, 40);
    let policy_hash = billing_policy_digest(&[1; 32], &key(3), &key(4), &terms).unwrap();
    assert_eq!(hex(policy_hash), "f04e772ed07571e8c18cc73f73671faeab26824dbfd628d0bc5df2dbb1afd88b");
    assert_eq!(hex(source_report_digest(&[1; 32], &key(3), &key(4), &policy_hash, &source).unwrap()),
        "2afc1604790fc991369fb4df0c9034cf8b8784cb10c4e91e5edf084fa8d5039c");
    assert_eq!(hex(seller_claim_digest(&[1; 32], &key(3), &key(4), &policy_hash, &seller).unwrap()),
        "a5d60559955856d4d3f02e5060e12d4125a4fce68108cdd69c95d2e168aba5ae");
}

#[test]
fn policy_hash_commits_all_roles_work_deadlines_and_tariff_fields() {
    let terms = policy();
    let original = billing_policy_digest(&[1; 32], &key(3), &key(4), &terms).unwrap();
    let mut variants = Vec::new();
    let mut value = terms.clone(); value.schema_version += 1; variants.push(value);
    value = terms.clone(); value.source = key(90); variants.push(value);
    value = terms.clone(); value.source_identity = [90; 32]; variants.push(value);
    value = terms.clone(); value.work_id = [90; 32]; variants.push(value);
    value = terms.clone(); value.request_hash = [90; 32]; variants.push(value);
    value = terms.clone(); value.work_until += 1; variants.push(value);
    value = terms.clone(); value.evidence_deadline += 1; variants.push(value);
    value = terms.clone(); value.claim_deadline += 1; variants.push(value);
    value = terms.clone(); value.metric_count += 1; variants.push(value);
    value = terms.clone(); value.metric_ids[0] = [90; 32]; variants.push(value);
    value = terms.clone(); value.price_numerators[0] += 1; variants.push(value);
    value = terms.clone(); value.price_denominators[0] += 1; variants.push(value);
    value = terms; value.max_units[0] += 1; variants.push(value);
    for variant in variants {
        assert_ne!(original, billing_policy_digest(&[1; 32], &key(3), &key(4), &variant).unwrap());
    }
}

#[test]
fn source_and_seller_messages_bind_context_policy_and_every_signed_field() {
    let terms = policy(); let source = report(); let seller = claim();
    let policy_hash = billing_policy_digest(&[1; 32], &key(3), &key(4), &terms).unwrap();
    let original = source_report_digest(&[1; 32], &key(3), &key(4), &policy_hash, &source).unwrap();
    let original_seller = seller_claim_digest(&[1; 32], &key(3), &key(4), &policy_hash, &seller).unwrap();
    assert_ne!(original, original_seller);

    for (domain, bill, financial_claim, hash) in [
        ([9; 32], key(3), key(4), policy_hash),
        ([1; 32], key(9), key(4), policy_hash),
        ([1; 32], key(3), key(9), policy_hash),
        ([1; 32], key(3), key(4), [9; 32]),
    ] {
        assert_ne!(original, source_report_digest(&domain, &bill, &financial_claim, &hash, &source).unwrap());
        assert_ne!(original_seller, seller_claim_digest(&domain, &bill, &financial_claim, &hash, &seller).unwrap());
    }

    let mut variants = Vec::new();
    let mut value = source.clone(); value.record_id = [90; 32]; variants.push(value);
    value = source.clone(); value.request_hash = [90; 32]; variants.push(value);
    value = source.clone(); value.started_at += 1; variants.push(value);
    value = source.clone(); value.completed_at += 1; variants.push(value);
    value = source.clone(); value.units[0] += 1; variants.push(value);
    value = source.clone(); value.result_hash = [90; 32]; variants.push(value);
    value = source.clone(); value.finality += 1; variants.push(value);
    value = source; value.outcome += 1; variants.push(value);
    for variant in variants {
        assert_ne!(original, source_report_digest(&[1; 32], &key(3), &key(4), &policy_hash, &variant).unwrap());
    }
    let mut changed = seller.clone(); changed.claim_id = [90; 32];
    assert_ne!(original_seller, seller_claim_digest(&[1; 32], &key(3), &key(4), &policy_hash, &changed).unwrap());
    changed = seller; changed.amount += 1;
    assert_ne!(original_seller, seller_claim_digest(&[1; 32], &key(3), &key(4), &policy_hash, &changed).unwrap());
}

#[test]
fn message_domains_include_core_identity_and_cannot_cross_roles() {
    let policy_hash = billing_policy_digest(&[1; 32], &key(3), &key(4), &policy()).unwrap();
    let body = encoded(&claim());
    let canonical = seller_claim_digest(&[1; 32], &key(3), &key(4), &policy_hash, &claim()).unwrap();
    let wrong_program = hashv(&[b"DEXTER:BILLING-SELLER-CLAIM:1", &[1; 32], key(99).as_ref(),
        key(3).as_ref(), key(4).as_ref(), &policy_hash, &body]).to_bytes();
    let wrong_role = hashv(&[b"DEXTER:BILLING-SOURCE-REPORT:1", &[1; 32], CORE_ID.as_ref(),
        key(3).as_ref(), key(4).as_ref(), &policy_hash, &body]).to_bytes();
    assert_ne!(canonical, wrong_program);
    assert_ne!(canonical, wrong_role);
    assert_ne!(policy_hash, billing_policy_digest(&[2; 32], &key(3), &key(4), &policy()).unwrap());
    assert_ne!(policy_hash, billing_policy_digest(&[1; 32], &key(4), &key(3), &policy()).unwrap());
}

#[test]
fn reserved_namespace_and_financial_unit_have_distinct_fixed_domains() {
    assert_eq!(BILLING_NAMESPACE, hashv(&[b"DEXTER:EVIDENCE-BILLING-NAMESPACE:1"]).to_bytes());
    assert_eq!(BILLING_SETTLEMENT_UNIT, hashv(&[b"DEXTER:EVIDENCE-BILLING-FINANCIAL-ATOMS:1"]).to_bytes());
    assert_ne!(BILLING_NAMESPACE, BILLING_SETTLEMENT_UNIT);
}
