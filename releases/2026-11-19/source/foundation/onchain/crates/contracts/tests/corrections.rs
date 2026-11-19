use dexter_contracts::corrections::{CorrectionAmounts, CorrectionPlan, CollectionCorrectionUsage, RepaymentRefundUsage};
use dexter_contracts::financing::PrincipalAmounts;

fn principal(original: u64, repaid: u64) -> PrincipalAmounts {
    PrincipalAmounts { originated: original, outstanding: original-repaid, repaid, extinguished: 0 }
}

#[test]
fn unpaid_bill_reduction_releases_payable_backing_without_cash_or_repayment() {
    let original = principal(100, 0);
    let (totals, debt, plan) = CorrectionAmounts::default().apply(&original, 0, 30, 0, 0).unwrap();
    assert_eq!(plan, CorrectionPlan { total:30, principal_credit:30, prior_repayment_refund:0,
        creditor_cash:0, required_cash:0, creditor_shortfall:30, backing_released_from_payable:30 });
    assert_eq!(debt, PrincipalAmounts { originated:100,outstanding:70,repaid:0,extinguished:30 });
    assert_eq!(totals, CorrectionAmounts { unpaid_reduced:30,collected_returned:0,
        principal_credited:30,prior_repayment_refunded:0 });
}

#[test]
fn collected_return_satisfies_current_creditor_without_rewriting_repaid_history() {
    let original = principal(100, 0);
    let (totals, debt, plan) = CorrectionAmounts::default().apply(&original, 100, 0, 35, 0).unwrap();
    assert_eq!((plan.creditor_cash,plan.required_cash,plan.creditor_shortfall),(35,35,0));
    assert_eq!((debt.outstanding,debt.repaid,debt.extinguished),(65,0,35));
    assert_eq!((totals.collected_returned,totals.principal_credited),(35,35));
}

#[test]
fn correction_after_repayment_returns_only_the_repaid_slice_to_its_original_payer() {
    let original = principal(100, 40);
    let (totals, debt, plan) = CorrectionAmounts::default().apply(&original, 100, 0, 80, 0).unwrap();
    assert_eq!((plan.principal_credit,plan.prior_repayment_refund),(60,20));
    assert_eq!((plan.creditor_cash,plan.required_cash),(60,80));
    assert_eq!((debt.originated,debt.outstanding,debt.repaid,debt.extinguished),(100,0,40,60));
    assert_eq!(totals.prior_repayment_refunded,20);
    let (closed, final_debt, final_plan) = totals.apply(&debt,100,0,20,0).unwrap();
    assert_eq!((final_plan.principal_credit,final_plan.prior_repayment_refund),(0,20));
    assert_eq!(final_plan.creditor_cash,0);
    assert_eq!(closed.prior_repayment_refunded,40);
    assert_eq!(final_debt.repaid,40);
    assert!(closed.apply(&final_debt,100,0,1,0).is_err());
}

#[test]
fn unpaid_supplier_bill_already_repaid_requires_real_refund_cash() {
    let original = principal(100, 80);
    let (_, debt, plan) = CorrectionAmounts::default().apply(&original,0,50,0,0).unwrap();
    assert_eq!((plan.principal_credit,plan.prior_repayment_refund),(20,30));
    assert_eq!((plan.creditor_cash,plan.required_cash,plan.creditor_shortfall),(0,30,20));
    assert_eq!(plan.backing_released_from_payable,50);
    assert_eq!(debt.repaid,80);
}

#[test]
fn mixed_correction_allocates_collected_cash_before_uncompensated_creditor_reduction() {
    let original = principal(100, 60);
    let (_, debt, plan) = CorrectionAmounts::default().apply(&original,40,60,40,0).unwrap();
    assert_eq!((plan.total,plan.principal_credit,plan.prior_repayment_refund),(100,40,60));
    assert_eq!((plan.creditor_cash,plan.required_cash,plan.creditor_shortfall),(40,100,0));
    assert_eq!((debt.outstanding,debt.repaid,debt.extinguished),(0,60,40));
}

#[test]
fn assignment_compensation_is_explicit_bounded_cash_and_creates_no_new_debt() {
    let original = principal(100, 0);
    let (_, partial, plan) = CorrectionAmounts::default().apply(&original,20,60,20,35).unwrap();
    assert_eq!((plan.principal_credit,plan.creditor_cash,plan.creditor_shortfall),(80,55,25));
    assert_eq!((plan.required_cash,partial.outstanding,partial.repaid),(55,20,0));
    let (_,_,full) = CorrectionAmounts::default().apply(&original,20,60,20,60).unwrap();
    assert_eq!((full.creditor_cash,full.creditor_shortfall,full.required_cash),(80,0,80));
    assert!(CorrectionAmounts::default().apply(&original,20,60,20,61).is_err());
}

#[test]
fn later_collection_and_repayment_preserve_correction_capacity_and_origin() {
    let original = principal(100,40);
    let (first, debt, _) = CorrectionAmounts::default().apply(&original,40,20,10,0).unwrap();
    let later_debt = debt.repay(30).unwrap();
    first.validate(100,70,&later_debt).unwrap();
    let (second, final_debt, plan) = first.apply(&later_debt,70,10,20,0).unwrap();
    assert_eq!((plan.principal_credit,plan.prior_repayment_refund),(0,30));
    assert_eq!((second.unpaid_reduced,second.collected_returned),(30,30));
    assert_eq!((final_debt.originated,final_debt.repaid,final_debt.extinguished),(100,70,30));
    assert_eq!(100-second.unpaid_reduced-second.collected_returned,
        final_debt.outstanding+final_debt.repaid-second.prior_repayment_refunded);
    assert!(second.apply(&final_debt,70,1,0,0).is_err());
}

#[test]
fn economic_invoice_oracle_matches_all_small_admissible_correction_partitions() {
    for original in 1..=12 {
        for gross_paid in 0..=original {
            for paid_by_debtor in 0..=original {
                let p=principal(original,paid_by_debtor);
                for unpaid in 0..=original-gross_paid {
                    for returned in 0..=gross_paid {
                        if unpaid+returned==0 {continue;}
                        let (_,after,plan)=CorrectionAmounts::default().apply(&p,gross_paid,unpaid,returned,0).unwrap();
                        // Independent billing interpretation: credit the original invoice,
                        // retain gross prior payments, refund only the resulting overpayment.
                        let revised_invoice=original-unpaid-returned;
                        let due=revised_invoice.saturating_sub(paid_by_debtor);
                        let overpayment=paid_by_debtor.saturating_sub(revised_invoice);
                        assert_eq!(after.outstanding,due);
                        assert_eq!(plan.prior_repayment_refund,overpayment);
                        assert_eq!(after.repaid,paid_by_debtor);
                        assert_eq!(after.originated,original);
                        assert_eq!(plan.required_cash,plan.creditor_cash+overpayment);
                        assert!(plan.required_cash<=plan.total);
                        assert_eq!(plan.creditor_shortfall+plan.creditor_cash,plan.principal_credit);
                    }
                }
            }
        }
    }
}

#[test]
fn invalid_history_excess_scope_and_prior_writeoffs_cannot_become_refundable_cash() {
    let p=principal(100,20);
    let bad=CorrectionAmounts { unpaid_reduced:1,..CorrectionAmounts::default() };
    assert!(bad.validate(100,0,&p).is_err());
    assert!(CorrectionAmounts::default().plan(&p,101,0,1,0).is_err());
    assert!(CorrectionAmounts::default().plan(&p,60,41,0,0).is_err());
    assert!(CorrectionAmounts::default().plan(&p,60,0,61,0).is_err());
    assert_eq!(CorrectionAmounts::default().plan(&p,0,0,0,0).unwrap(),CorrectionPlan::default());
    assert!(CorrectionAmounts::default().plan(&p,0,0,0,1).is_err());
    let generic=PrincipalAmounts { originated:100,outstanding:30,repaid:20,extinguished:50 };
    assert!(CorrectionAmounts::default().plan(&generic,100,0,51,0).is_err());
}

#[test]
fn complete_u64_amount_and_overflow_boundaries_are_exact() {
    let p=principal(u64::MAX,0);
    let (_,closed,plan)=CorrectionAmounts::default().apply(&p,u64::MAX,0,u64::MAX,0).unwrap();
    assert_eq!((plan.required_cash,closed.extinguished),(u64::MAX,u64::MAX));
    assert_eq!(closed.outstanding,0);
    assert!(CorrectionAmounts::default().apply(&p,1,u64::MAX,1,0).is_err());
    let usage=CollectionCorrectionUsage {collected_returned:u64::MAX};
    assert!(usage.record(1,u64::MAX).is_err());
}

#[test]
fn original_collection_receipt_has_one_lifetime_refund_capacity() {
    let a=CollectionCorrectionUsage::default().record(17,40).unwrap();
    let b=a.record(23,40).unwrap();
    assert_eq!(b.collected_returned,40);
    assert!(b.record(1,40).is_err());
    assert!(a.record(0,40).is_err());
    assert_eq!(a.collected_returned,17);
}

#[test]
fn original_repayment_separates_principal_lender_interest_and_protocol_refunds() {
    let a=RepaymentRefundUsage::default().record(10,3,1,40,10,4).unwrap();
    let b=a.record(30,3,3,40,10,4).unwrap();
    assert_eq!(b,RepaymentRefundUsage {principal_refunded:40,lender_interest_refunded:6,protocol_interest_refunded:4});
    assert!(b.record(1,0,0,40,10,4).is_err());
    assert!(b.record(0,1,0,40,10,4).is_err());
    assert!(b.record(0,0,1,40,10,4).is_err());
    assert!(b.record(0,0,0,40,10,4).is_err());
    assert!(a.validate(40,3,4).is_err());
    assert_eq!(a.principal_refunded,10);
}
