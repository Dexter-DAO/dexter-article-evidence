//! Host contract tests only: no Swig authorization or token transfer is executed.
use anchor_lang::prelude::Pubkey;
use dexter_contracts::{completed_run_policy, terms_digest, Balances, TabTerms, ACCEPT, CAPTURE, PAYOUT, RELEASE, REPAY};
use dexter_contracts::financing::*;

fn key(n: u8) -> Pubkey { Pubkey::new_from_array([n; 32]) }
fn asset() -> Asset { Asset { mint: key(4), token_program: key(5) } }
fn binding() -> CustodyBinding {
    CustodyBinding { source: key(6), asset: asset(), token_account: key(7), reserve_ledger: custody_reserve_address(&key(7)),
        beneficial_cash_owner: key(8), settlement_authority: key(9), profile: BackingProfile::GovernedWalletCash,
        controller_program: key(10), wallet_binding: key(11), policy_revision: 1, governance_profile_hash: [12; 32] }
}
fn tab() -> TabTerms {
    TabTerms { domain: [1; 32], tab_id: [2; 32], principal: key(21), actor: key(22), mandate: key(23),
        asset: asset().mint, supplier: key(24), source: key(6), cap: 300, expires_at: 1000,
        credential_generation: 1, mandate_revision: 1,
        external_terms_hash: completed_run_policy(&[25; 32], &[26; 32], 10, &[27; 32]) }
}
fn own_capital() -> (TabTerms, FinancingAcceptance) {
    let tab = tab();
    let funding = FundingCertificate { version: FINANCING_VERSION, domain: tab.domain, guarantee: key(30), custody: binding(),
        arrangement: FundingArrangement::IssuerCapital, original_tab_terms_hash: terms_digest(&tab).unwrap(),
        supplier: tab.supplier, finite_amount: tab.cap, release_policy_hash: [31; 32], service_grant_hash: [32; 32],
        admitted_deployment_hash: [33; 32], accepted_facility_terms_hash: [0; 32] };
    let issuer = IssuerAdmission { issuer_principal: key(8), controlling_program: key(34), controlling_version: 1,
        eligibility_risk_decision_hash: [35; 32], pricing_terms_hash: [36; 32], accepted_credit_amount: 300,
        funding_certificate_hash: funding.digest().unwrap() };
    (tab, FinancingAcceptance { funding, issuer, customer_credit_terms_hash: [37; 32], buyer_fee_schedule_hash: [38; 32],
        buyer_fee_cap: 5, credit_fee_cap: 0, assignment_and_disclosure_policy_hash: [39; 32] })
}
fn external_facility() -> (TabTerms, FinancingAcceptance, FinancierFacilityTerms) {
    let (tab, mut accepted) = own_capital();
    let facility = FinancierFacilityTerms { version: FINANCING_VERSION, domain: tab.domain, facility: key(40),
        financier_principal: key(8), issuer_obligor: key(41), asset: asset(), reserve_ledger: binding().reserve_ledger,
        principal_commitment_limit: 500, new_commitments_from: 0, new_commitments_until: 1000,
        terms_hash: [42; 32], fee_schedule_hash: [43; 32], revision: 1 };
    accepted.funding.arrangement = FundingArrangement::IssuerFacility { facility: facility.facility };
    accepted.funding.accepted_facility_terms_hash = facility.digest().unwrap();
    accepted.issuer.issuer_principal = facility.issuer_obligor;
    accepted.issuer.funding_certificate_hash = accepted.funding.digest().unwrap();
    (tab, accepted, facility)
}
fn title() -> ReceivableTitle {
    ReceivableTitle { lot: key(50), economic_origin: [51; 32], accepted_guarantee: key(30), debtor_principal: key(21),
        asset: asset(), current_creditor: key(8), repayment_destination: key(52), creditor_book: key(53),
        ownership_revision: 0, acquisition_basis: 200, provenance_hash: [54; 32], assignment_and_disclosure_policy_hash: [39; 32] }
}
fn principal() -> PrincipalAmounts { PrincipalAmounts { originated: 200, outstanding: 200, repaid: 0, extinguished: 0 } }
fn assignment(old: &ReceivableTitle) -> ResidualAssignment {
    ResidualAssignment { lot: old.lot, expected_ownership_revision: old.ownership_revision, seller: old.current_creditor,
        buyer: key(60), buyer_destination: key(61), buyer_book: key(62), residual_principal: 200,
        consideration: 190, capitalized_acquisition_costs: 2, acquisition_cost_evidence_hash: [63; 32],
        acquisition_basis: 192, fee_transfer_set_hash: [64; 32],
        accepted_assignment_and_disclosure_policy_hash: old.assignment_and_disclosure_policy_hash }
}
fn collection() -> EarnedCollection {
    EarnedCollection { version: FINANCING_VERSION, domain: [1; 32], claim: key(30), payable_lot: key(70),
        accepted_service_grant_hash: [32; 32], reserve_ledger: binding().reserve_ledger, token_account: key(7),
        asset: asset(), supplier: key(24), destination: key(71), paid_before: 0, amount: 200 }
}

#[test]
fn wallet_acceptance_amounts_keep_free_cash_and_earned_rights_separate() {
    let mut cash = 1000;
    let mut custody = CustodyAmounts::default().reserve(300, cash).unwrap().earn(200).unwrap();
    let company = CustomerCreditUsage { unused_commitments: 100, principal_outstanding: 200, agreed_charge_exposure: 0 };
    assert_eq!(custody.protected().unwrap(), 300);
    assert_eq!(custody.uncommitted_cash(cash).unwrap(), 700);
    assert_eq!(custody.uncommitted_cash(cash - 701), Err(FinanceError::CashShortfall));
    // Amount contract for the required owner withdrawal followed by collection.
    // Agent revocation/backend outage/signers require separate runtime evidence.
    cash -= 700;
    assert_eq!(custody.uncommitted_cash(cash).unwrap(), 0);
    custody = custody.collect(200).unwrap(); cash -= 200;
    assert_eq!((cash, custody.unused, custody.earned_unpaid), (100, 100, 0));
    assert_eq!(company.principal_outstanding, 200);
    custody = custody.release(100).unwrap();
    assert_eq!(cash, 100); // Releasing wallet backing does not move tokens.
    assert_eq!(custody.uncommitted_cash(cash).unwrap(), 100);
}

#[test]
fn all_facilities_share_the_custody_floor_across_policy_rotations() {
    let first = binding(); let mut second = first.clone();
    second.source = key(80); second.policy_revision = 2; second.wallet_binding = key(81);
    second.validate_shape().unwrap();
    assert_eq!(first.reserve_ledger, second.reserve_ledger);
    let custody = CustodyAmounts::default().reserve(300, 1000).unwrap().reserve(250, 1000).unwrap();
    assert_eq!(custody.uncommitted_cash(1000).unwrap(), 450);
    assert_eq!(custody.uncommitted_cash(549), Err(FinanceError::CashShortfall));
    assert_eq!(custody.reserve(451, 1000), Err(FinanceError::CashShortfall));
    second.reserve_ledger = custody_reserve_address(&key(82));
    assert_eq!(second.validate_shape(), Err(FinanceError::WrongBinding));
}

#[test]
fn external_facility_draw_and_company_repayment_are_different_debts() {
    let mut provider_cash = 1000;
    let mut custody = CustodyAmounts::default().reserve(300, provider_cash).unwrap().earn(200).unwrap();
    let mut facility = IssuerFacilityUsage { cash_reserved: 300, funded_principal: 0 };
    let mut company = principal();
    assert_eq!(facility.funded_principal, 0); // Earning alone did not advance cash.
    custody = custody.collect(200).unwrap(); provider_cash -= 200;
    facility = facility.draw_for_supplier(200).unwrap();
    assert_eq!((provider_cash, custody.protected().unwrap(), facility.funded_principal), (800, 100, 200));
    assert_eq!(facility.committed().unwrap(), 300);
    company = company.repay(50).unwrap();
    assert_eq!((company.outstanding, facility.funded_principal, provider_cash), (150, 200, 800));
    facility = facility.repay_financier(50).unwrap(); provider_cash += 50;
    assert_eq!((company.outstanding, facility.funded_principal, provider_cash), (150, 150, 850));
    assert_eq!(custody.protected().unwrap(), 100);
}

#[test]
fn existing_cash_backing_equations_match_the_new_custody_amounts() {
    let mut old = Balances::default(); let mut next = CustodyAmounts::default();
    for (kind, amount) in [(ACCEPT, 300), (CAPTURE, 200), (REPAY, 50), (PAYOUT, 200), (RELEASE, 100)] {
        old = old.transition(kind, amount).unwrap();
        next = match kind {
            ACCEPT => next.reserve(amount, 1000).unwrap(), CAPTURE => next.earn(amount).unwrap(),
            PAYOUT => next.collect(amount).unwrap(), RELEASE => next.release(amount).unwrap(), _ => next,
        };
        assert_eq!((next.unused, next.earned_unpaid, next.protected().unwrap()), (old.reserved, old.payable, old.protected_cash));
    }
    assert_eq!(old.debt, 150);
}

#[test]
fn lowered_limits_fence_new_risk_without_erasing_existing_obligations() {
    let company = CustomerCreditUsage { unused_commitments: 100, principal_outstanding: 200, agreed_charge_exposure: 5 };
    let facility = IssuerFacilityUsage { cash_reserved: 100, funded_principal: 200 };
    assert_eq!(company.headroom(500).unwrap(), 195);
    assert_eq!(company.headroom(200).unwrap(), 0);
    assert_eq!(facility.headroom(200).unwrap(), 0);
    assert_eq!((company.committed().unwrap(), facility.committed().unwrap()), (305, 300));
}

#[test]
fn financing_binds_original_metered_policy_and_separate_credit_fee_cap() {
    let (tab, mut accepted) = own_capital();
    let original_policy = tab.external_terms_hash;
    accepted.validate_against(&tab, None).unwrap(); // 300 credit + 5 separately cash-paid fees.
    assert_eq!(accepted.authorized_charge_cap().unwrap(), 305);
    let first_digest = accepted.digest(&tab, None).unwrap();
    accepted.credit_fee_cap = 5;
    assert_eq!(accepted.validate_against(&tab, None), Err(FinanceError::LimitExceeded));
    accepted.issuer.accepted_credit_amount = 305;
    assert_ne!(first_digest, accepted.digest(&tab, None).unwrap());
    assert_eq!(tab.external_terms_hash, original_policy);
    let mut different_tab = tab.clone(); different_tab.external_terms_hash = [99; 32];
    assert_eq!(accepted.validate_against(&different_tab, None), Err(FinanceError::WrongTerms));
    accepted.credit_fee_cap = 6;
    assert_eq!(accepted.validate_against(&tab, None), Err(FinanceError::LimitExceeded));
}

#[test]
fn external_funding_binds_actual_provider_obligor_asset_and_accepted_terms() {
    let (tab, accepted, facility) = external_facility();
    accepted.validate_against(&tab, Some(&facility)).unwrap();
    assert_eq!(accepted.validate_against(&tab, None), Err(FinanceError::WrongBinding));
    for mutation in 0..5 {
        let mut other = facility.clone();
        match mutation { 0 => other.financier_principal = key(90), 1 => other.issuer_obligor = key(90),
            2 => other.asset.token_program = key(90), 3 => other.reserve_ledger = key(90), _ => other.fee_schedule_hash = [90; 32] }
        assert_eq!(accepted.validate_against(&tab, Some(&other)), Err(FinanceError::WrongBinding));
    }
    let mut self_loan = facility.clone(); self_loan.issuer_obligor = self_loan.financier_principal;
    assert_eq!(self_loan.digest(), Err(FinanceError::WrongTerms));
    // Mutable admission does not participate in the already-accepted certificate.
    let admission = FacilityAdmission { facility_terms_hash: facility.digest().unwrap(), revision: 2,
        accepts_new_commitments: false, usage: IssuerFacilityUsage { cash_reserved: 300, funded_principal: 0 } };
    assert!(!admission.accepts_new_commitments);
    accepted.validate_against(&tab, Some(&facility)).unwrap();
}

#[test]
fn backing_profile_changes_are_explicit_commitments() {
    let (tab, mut accepted) = own_capital();
    let wallet_digest = accepted.digest(&tab, None).unwrap();
    accepted.funding.custody.profile = BackingProfile::CoreEscrowCash;
    assert_eq!(accepted.validate_against(&tab, None), Err(FinanceError::WrongTerms));
    accepted.issuer.funding_certificate_hash = accepted.funding.digest().unwrap();
    assert_ne!(wallet_digest, accepted.digest(&tab, None).unwrap());
}

#[test]
fn assignment_preserves_customer_debt_supplier_backing_and_fee_ownership() {
    let old = title(); let amounts = principal();
    let company = CustomerCreditUsage { unused_commitments: 100, principal_outstanding: 200, agreed_charge_exposure: 0 };
    let custody = CustodyAmounts { unused: 100, earned_unpaid: 200 };
    let fee_owner = FeeTitle { entitlement: key(91), beneficiary: old.current_creditor, destination: key(92), book: old.creditor_book, ownership_revision: 0 };
    let new = assignment(&old).apply_title(&old, &amounts).unwrap();
    assert_eq!(new.current_creditor, key(60)); assert_eq!(new.acquisition_basis, 192);
    assert_eq!(new.ownership_revision, 1); assert_ne!(new.provenance_hash, old.provenance_hash);
    assert_eq!(new.accepted_guarantee, old.accepted_guarantee);
    assert_eq!(new.economic_origin, old.economic_origin);
    assert_eq!((company.committed().unwrap(), custody.protected().unwrap(), fee_owner.beneficiary), (300, 300, old.current_creditor));
    let repaid = amounts.repay(50).unwrap();
    assert_eq!((repaid.outstanding, custody.earned_unpaid, custody.protected().unwrap()), (150, 200, 300));
    let paid_supplier = custody.collect(200).unwrap();
    assert_eq!((paid_supplier.protected().unwrap(), repaid.outstanding), (100, 150));
}

#[test]
fn assignment_rejects_stale_title_partial_transfer_and_invented_basis() {
    let old = title(); let amounts = principal(); let transfer = assignment(&old);
    let mut bad = transfer.clone(); bad.expected_ownership_revision += 1;
    assert_eq!(bad.apply_title(&old, &amounts), Err(FinanceError::WrongRevision));
    bad = transfer.clone(); bad.seller = key(99);
    assert_eq!(bad.apply_title(&old, &amounts), Err(FinanceError::WrongCreditor));
    bad = transfer.clone(); bad.residual_principal = 100;
    assert_eq!(bad.apply_title(&old, &amounts), Err(FinanceError::WrongTerms));
    bad = transfer; bad.acquisition_basis = 201;
    assert_eq!(bad.apply_title(&old, &amounts), Err(FinanceError::InvalidAmounts));
}

#[test]
fn fee_caps_are_aggregate_and_cash_paid_fees_do_not_require_credit() {
    let (tab, mut accepted) = own_capital();
    accepted.validate_against(&tab, None).unwrap();
    let used = FeeCapUsage::default().allocate(3, 0, &accepted).unwrap();
    assert_eq!(used.allocate(3, 0, &accepted), Err(FinanceError::LimitExceeded));
    let full = used.allocate(2, 0, &accepted).unwrap();
    assert_eq!(full.allocated_buyer_cap, 5);
    assert_eq!(FeeCapUsage::default().allocate(1, 1, &accepted), Err(FinanceError::LimitExceeded));
    accepted.credit_fee_cap = 2;
    accepted.issuer.accepted_credit_amount = 302;
    accepted.validate_against(&tab, None).unwrap();
    assert_eq!(FeeCapUsage::default().allocate(5, 2, &accepted).unwrap().allocated_credit_cap, 2);
    assert_eq!(FeeCapUsage::default().allocate(1, 2, &accepted), Err(FinanceError::InvalidAmounts));
}

#[test]
fn fees_require_earning_and_real_collected_proceeds() {
    let accrued = FeeAmounts::default().accrue(5, 5).unwrap();
    assert_eq!(accrued.held_proceeds().unwrap(), 0);
    assert!(accrued.distribute(1).is_err());
    assert_eq!(accrued.accrue(1, 5), Err(FinanceError::LimitExceeded));
    assert!(accrued.collect(6).is_err());
    let collected = accrued.collect(2).unwrap();
    assert_eq!((collected.outstanding().unwrap(), collected.held_proceeds().unwrap()), (3, 2));
    assert!(collected.distribute(3).is_err());
    let distributed = collected.distribute(2).unwrap();
    assert_eq!((distributed.outstanding().unwrap(), distributed.held_proceeds().unwrap()), (3, 0));
}

#[test]
fn fee_refunds_and_distribution_recoveries_do_not_reopen_paid_debt() {
    let paid = FeeAmounts::default().accrue(5, 5).unwrap().collect(5).unwrap().distribute(5).unwrap();
    assert!(FeeAmounts { refunded: 1, ..paid }.validate(5).is_err());
    let recovered_and_refunded = FeeAmounts { recovered_distributions: 2, refunded: 2, ..paid };
    recovered_and_refunded.validate(5).unwrap();
    assert_eq!((recovered_and_refunded.outstanding().unwrap(), recovered_and_refunded.held_proceeds().unwrap()), (0, 0));
    assert!(FeeAmounts { recovered_distributions: 6, ..paid }.validate(5).is_err());
    assert!(FeeAmounts { waived: 1, ..paid }.validate(5).is_err());
}

#[test]
fn partial_repayment_names_exact_title_fee_entitlements_and_debit() {
    let old = title(); let amounts = principal();
    let fee = FeePayment { entitlement: key(91), expected_ownership_revision: 0, beneficiary: key(8), destination: key(92), amount: 2 };
    let allocation = RepaymentAllocation { lot: old.lot, expected_ownership_revision: 0, current_creditor: old.current_creditor,
        principal_destination: old.repayment_destination, principal: 50, fees: vec![fee.clone()], actual_debit: 52 };
    allocation.validate_shape(&old, &amounts).unwrap();
    let mut wrong = allocation.clone(); wrong.actual_debit = 53;
    assert_eq!(wrong.validate_shape(&old, &amounts), Err(FinanceError::InvalidAmounts));
    wrong = allocation.clone(); wrong.fees.push(fee); wrong.actual_debit = 54;
    assert_eq!(wrong.validate_shape(&old, &amounts), Err(FinanceError::DuplicateFee));
    let new_title = assignment(&old).apply_title(&old, &amounts).unwrap();
    assert!(allocation.validate_shape(&new_title, &amounts).is_err());
    // An uncertain persisted repayment must be reconciled, never silently rerouted.
    assert_eq!(allocation.principal_destination, old.repayment_destination);
}

#[test]
fn collection_identity_survives_relayer_and_projection_attempt_changes() {
    let earned = collection(); let economic_id = earned.operation_id().unwrap();
    let first = CollectionAttempt { economic_operation_id: economic_id, relayer: key(95), nonce: [96; 32], expires_at: 100,
        expected_claim_revision: 1, wallet_binding_revision: 1, service_without_pool: false, execution_bundle_hash: [97; 32] };
    let second = CollectionAttempt { relayer: key(98), nonce: [99; 32], expires_at: 200,
        expected_claim_revision: 2, wallet_binding_revision: 2, service_without_pool: true, ..first.clone() };
    assert_eq!(first.economic_operation_id, second.economic_operation_id);
    assert_ne!(first.authorization_digest().unwrap(), second.authorization_digest().unwrap());
    for mutation in 0..5 {
        let mut changed = earned.clone();
        match mutation { 0 => changed.amount = 199, 1 => changed.destination = key(99),
            2 => changed.accepted_service_grant_hash = [99; 32], 3 => changed.domain = [99; 32], _ => changed.paid_before = 1 }
        assert_ne!(economic_id, changed.operation_id().unwrap());
    }
}

#[test]
fn collection_ranges_prevent_overlapping_consumption_and_overpayment() {
    let earned = collection(); earned.validate_unpaid_range(200, 0).unwrap();
    assert_eq!(earned.validate_unpaid_range(199, 0), Err(FinanceError::LimitExceeded));
    assert_eq!(earned.validate_unpaid_range(0, 200), Err(FinanceError::WrongRevision));
    let partial = EarnedCollection { amount: 50, ..earned.clone() };
    partial.validate_unpaid_range(200, 0).unwrap();
    let remainder = EarnedCollection { paid_before: 50, amount: 150, ..earned };
    remainder.validate_unpaid_range(150, 50).unwrap();
    assert_ne!(partial.operation_id().unwrap(), remainder.operation_id().unwrap());
    assert_eq!(partial.validate_unpaid_range(150, 50), Err(FinanceError::WrongRevision));
    // Gross earning was 200, but a valid correction/offset leaves only 100 owed.
    assert_eq!(collection().validate_unpaid_range(100, 0), Err(FinanceError::LimitExceeded));
    EarnedCollection { amount: 100, ..collection() }.validate_unpaid_range(100, 0).unwrap();
    // Actual handler must return an existing exact receipt BEFORE this state guard.
}

#[test]
fn invalid_and_overflowed_states_are_rejected_without_healing_them() {
    assert_eq!(CustodyAmounts { unused: u64::MAX, earned_unpaid: 1 }.collect(1), Err(FinanceError::Arithmetic));
    assert_eq!(CustodyAmounts { unused: u64::MAX, earned_unpaid: 1 }.release(1), Err(FinanceError::Arithmetic));
    assert_eq!(CustodyAmounts::default().reserve(0, 100), Err(FinanceError::ZeroAmount));
    assert!(CustodyAmounts::default().collect(1).is_err());
    assert!(IssuerFacilityUsage { cash_reserved: u64::MAX, funded_principal: 1 }.repay_financier(1).is_err());
    assert!(CustomerCreditUsage { unused_commitments: u64::MAX, principal_outstanding: 1, agreed_charge_exposure: 0 }.headroom(u64::MAX).is_err());
    assert!(PrincipalAmounts { originated: 100, outstanding: 99, repaid: 0, extinguished: 0 }.repay(1).is_err());
    let mut bad = collection(); bad.paid_before = u64::MAX;
    assert!(bad.operation_id().is_err());
    bad = collection(); bad.version += 1;
    assert_eq!(bad.operation_id(), Err(FinanceError::SchemaVersion));
}
