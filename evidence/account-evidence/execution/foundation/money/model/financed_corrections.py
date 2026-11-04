"""Independent economic oracle for the explicitly accepted correction profile.

The existing pool_model remains authoritative for its covered reserve, capture,
payout, repayment, release, title and facility transitions. It has correction
counters and conservation equations, but no correction/refund transition handler.
This additive oracle supplies the declared voluntary correction rules. It does
not implement investor valuation, impairment cohorts, a legal dispute oracle or
signer authentication. Actual programs must establish authority and cash routes.

An invoice credit reduces the net bill. Historical principal payments stay gross;
only the resulting overpayment returns to that actual payer. Collected supplier
proceeds satisfy residual creditor face first. Explicit additional compensation
can cover the remaining creditor reduction. No facility debt, fee income, spent
authority or historical acquisition cost is implicitly changed.
"""
from copy import deepcopy
from pool_model import checked_amount, require, Rejected


COUNTERS = ("unpaid_reduced", "collected_returned", "principal_credited", "prior_repayment_refunded")


def preview(principal: dict, gross_collected: int, previous: dict,
            unpaid_reduction: int, collected_return: int,
            creditor_compensation: int = 0) -> dict:
    """Derive the result from an adjusted invoice and net prior payments.

    principal uses the on-chain meanings originated/outstanding/repaid/extinguished.
    previous uses CorrectionAmounts' four monotonic counters. Returned keys match
    the public CorrectionPlan for native comparisons, but the debt/refund derivation
    starts from independent billing balances instead of applying Rust book deltas.
    A zero principal plan can accompany an explicitly authorized interest waiver;
    the runtime must reject a completely empty financial operation.
    """
    values = list(principal.values()) + list(previous.values()) + [gross_collected,
        unpaid_reduction, collected_return, creditor_compensation]
    for value in values:
        checked_amount(value)
    p = principal
    c = {key: previous.get(key, 0) for key in COUNTERS}
    require(p["originated"] == p["outstanding"] + p["repaid"] + p["extinguished"],
            "LEGAL_DEBT_CONSERVATION")
    require(c["principal_credited"] <= p["extinguished"], "CORRECTION_DEBT_LINEAGE")
    require(c["prior_repayment_refunded"] <= p["repaid"], "REFUND_EXCEEDS_PAYMENT")
    require(c["unpaid_reduced"] + c["collected_returned"]
            == c["principal_credited"] + c["prior_repayment_refunded"],
            "CORRECTION_CONSERVATION")
    supplier_unpaid = p["originated"] - gross_collected - c["unpaid_reduced"]
    supplier_cash_unreturned = gross_collected - c["collected_returned"]
    require(supplier_unpaid >= 0 and supplier_cash_unreturned >= 0,
            "SUPPLIER_HISTORY_CONSERVATION")
    require(unpaid_reduction <= supplier_unpaid and collected_return <= supplier_cash_unreturned,
            "CORRECTION_SCOPE")

    total = checked_amount(unpaid_reduction + collected_return)
    other_extinction = p["extinguished"] - c["principal_credited"]
    previous_bill = p["originated"] - c["unpaid_reduced"] - c["collected_returned"] - other_extinction
    prior_net_payments = p["repaid"] - c["prior_repayment_refunded"]
    require(previous_bill == p["outstanding"] + prior_net_payments, "NET_BILL_CONSERVATION")
    revised_bill = previous_bill - total
    require(revised_bill >= 0, "OTHER_WRITEOFF_IS_NOT_REFUNDABLE")
    new_due = max(0, revised_bill - prior_net_payments)
    payer_return = max(0, prior_net_payments - revised_bill)
    debt_credit = p["outstanding"] - new_due

    collected_applied_to_debt = min(collected_return, p["outstanding"])
    uncompensated_face = debt_credit - collected_applied_to_debt
    require(0 <= creditor_compensation <= uncompensated_face, "COMPENSATION_EXCEEDS_CREDIT")
    creditor_cash = collected_applied_to_debt + creditor_compensation
    return dict(total=total, principal_credit=debt_credit,
                prior_repayment_refund=payer_return, creditor_cash=creditor_cash,
                required_cash=checked_amount(creditor_cash+payer_return),
                creditor_shortfall=debt_credit-creditor_cash,
                backing_released_from_payable=unpaid_reduction)


def apply(principal: dict, gross_collected: int, previous: dict,
          unpaid_reduction: int, collected_return: int,
          creditor_compensation: int = 0) -> tuple[dict, dict, dict]:
    """Return new monotonic totals, legal debt history and the economic plan."""
    plan = preview(principal, gross_collected, previous, unpaid_reduction,
                   collected_return, creditor_compensation)
    p = deepcopy(principal)
    c = {key: previous.get(key, 0) for key in COUNTERS}
    p["outstanding"] -= plan["principal_credit"]
    p["extinguished"] += plan["principal_credit"]
    c["unpaid_reduced"] += unpaid_reduction
    c["collected_returned"] += collected_return
    c["principal_credited"] += plan["principal_credit"]
    c["prior_repayment_refunded"] += plan["prior_repayment_refund"]
    # The same conservation equations underlie CoreClaim.credited and
    # Payable.corrected in the existing monetary model.
    require(p["originated"] == p["outstanding"]+p["repaid"]+p["extinguished"],
            "LEGAL_DEBT_CONSERVATION")
    require(p["originated"] == (p["originated"]-gross_collected-c["unpaid_reduced"])
            + gross_collected+c["unpaid_reduced"], "PAYABLE_CONSERVATION")
    return c, p, plan


def record_repayment_refund(previous: dict, original: dict, principal: int = 0,
                            lender_interest: int = 0, protocol_interest: int = 0) -> dict:
    """One original receipt owns separate principal/lender/protocol capacities."""
    original_principal = original["principal_paid"]
    original_interest = original["interest_paid"]
    original_protocol = original["protocol_interest"]
    for value in (original_principal,original_interest,original_protocol,principal,
                  lender_interest,protocol_interest,*previous.values()):
        checked_amount(value)
    require(original_protocol <= original_interest, "PROTOCOL_ALLOCATION_EXCEEDS_INTEREST")
    require(principal+lender_interest+protocol_interest > 0, "ZERO_REFUND")
    limits = dict(principal_refunded=original_principal,
                  lender_interest_refunded=original_interest-original_protocol,
                  protocol_interest_refunded=original_protocol)
    additions = dict(principal_refunded=principal, lender_interest_refunded=lender_interest,
                     protocol_interest_refunded=protocol_interest)
    result = {}
    for key,limit in limits.items():
        result[key] = checked_amount(previous.get(key,0)+additions[key])
        require(result[key] <= limit, "ORIGINAL_RECEIPT_REFUND_EXHAUSTED")
    return result
