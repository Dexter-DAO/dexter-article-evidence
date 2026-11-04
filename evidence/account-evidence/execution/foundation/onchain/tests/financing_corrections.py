#!/usr/bin/env python3
"""Actual native-v1 bill-credit/refund lifecycle with independent economics.

All money is synthetic SPL USDC-shaped test data. These suites use the freshly
compiled programs; the pure oracle never replaces a program or patches state.
"""
from financing_correction_fixture import *
from financing_fixture import YEAR
import argparse


def lifecycle(f):
    f.setup()
    tab = f.acceptance("correction-lifecycle")
    f.accept(tab)
    original_fee_allocation = f.fee_allocated()
    run = f.earn(tab, "lifecycle-first-delivery", 50_000)
    origin = f.remember_origin(tab, run)
    first = f.correction(tab, run, "first-unpaid-credit", unpaid=10_000)
    f.correct("supplier and company negotiate an unpaid reduction with creditor consent", first)
    f.correct("same correction ID returns its original result without another credit", first, replay=True)
    conflicted = f.correction(tab, run, "first-unpaid-credit", unpaid=9_999)
    f.correct("same correction ID cannot change its accepted amount", conflicted, reject=True)
    f.correct("current creditor cannot lose principal without consent",
        f.correction(tab, run, "missing-creditor-consent", unpaid=1_000,
            account_overrides=dict(creditor_owner=None), omit_signers=(f.parties["issuer"]["owner"].pubkey(),)), reject=True)
    f.correct("stale company owner generation cannot authorize a new correction",
        f.correction(tab, run, "stale-company-generation", unpaid=1_000,
            arg_overrides=dict(company_owner_generation=2)), reject=True)
    collection = f.collection(tab, run, 20_000)
    f.collect(collection, label="supplier collects the reduced bill's earned residual")
    f.correct("a capture receipt cannot substitute for actual collected-cash lineage",
        f.correction(tab, run, "wrong-collection-kind", returned=1, collection=run["receipt"]), reject=True)
    f.correct("unfunded voluntary return rolls back every financial and usage account",
        f.correction(tab, run, "unfunded-return", returned=1, collection=collection, funder="protocol"), reject=True)
    fee = collection["expected_fee"]
    # Collected gross and accepted fees remain separate economic families.
    f.execute("fee beneficiary explicitly credits its collected fee to the frozen supplier route",
        f.fee_credit(tab, collection["receipt"], "explicit-seller-fee-credit", fee))
    f.correct("omitting the supplier's zero-observation book rolls back cash return and original-receipt usage",
        f.correction(tab,run,"missing-observed-supplier-book",returned=1,collection=collection,funder="fee",
            remaining_roles=["company","issuer","fee"]),reject=True)
    f.register_external_contributor("volunteer", 80, 5_000)
    paid = f.correction(tab, run, "first-collected-credit", returned=5_000, collection=collection, funder="volunteer")
    f.correct("independent other-domain owner funds the approved cash return without fresh creditor signature", paid)
    f.h.check("voluntary refund debits only the other-domain owner's approved cash", f.h.amount(f.parties["volunteer"]["owner_ata"]), 0)
    f.positions("volunteer", cash_sent=5_000)
    later = f.earn(tab, "lifecycle-later-independent-delivery", 10_000)
    f.h.check("later delivery retains proportional original expenditure basis",
        f.state("FundedRun", later["run"])["interest_terms"]["maximum_interest"], 1_000)
    mixed = f.correction(tab, run, "mixed-unpaid-paid-credit", unpaid=5_000, returned=5_000,
        collection=collection, outage=True)
    f.correct("mixed credit services accepted rights without consulting Pool projections", mixed)
    f.correct("historical correction remains recoverable after a later delivery", first, replay=True)
    f.correct("caught-up projections are required for the ordinary projection mode",
        f.correction(tab, run, "stale-projection-credit", unpaid=1_000), reject=True)
    f.catch_up()
    f.assert_origin("after partial corrections and later delivery", tab, run, origin)
    f.execute("partly corrected delivered receivable can still be assigned with its remaining obligations",
        f.assignment(tab, run, "assignment-after-partial-corrections", consideration=23_000, costs=0))
    f.correct("historical correction remains recoverable after a later title assignment", first, replay=True)
    over = f.correction(tab, run, "over-remaining-unpaid", unpaid=1,
        arg_overrides=dict(unpaid_reduction=15_001))
    f.correct("different correction IDs cannot over-reduce the remaining supplier bill", over, reject=True)
    full = f.correction(tab, run, "full-original-bill-credit", unpaid=15_000, returned=10_000, collection=collection)
    f.correct("full correction closes the original bill and preserves the later run", full)
    f.h.check("later delivery is still company debt", f.state("FundedClaim", tab["claim"])["debt"], 10_000)
    f.h.check("original delivery remains fully attributable", f.state("FundedRun", run["run"])["gross_earned"], 50_000)
    f.h.check("fee aggregate cap was not replenished by a bill correction", f.fee_allocated(), original_fee_allocation)
    f.assert_origin("after full correction", tab, run, origin)
    f.rotate("supplier", 71, "after-full-bill-correction")
    f.rotate("company", 72, "after-full-bill-correction")
    f.correct("original signed correction recovers after both owner rotations", full, replay=True)
    unchanged_addresses = list(f.h.observe)
    unchanged_state = f.h.snapshot(unchanged_addresses)
    alias = f.earn(tab, "lifecycle-first-delivery", 50_000, payment_name="new-worker-after-full-correction",
        label="replacement supplier owner retries the same fully corrected delivery under a new payment ID")
    f.h.check("new payment ID recovers the immutable original capture", alias["receipt"], run["receipt"])
    f.h.check("new payment ID cannot erase corrections or replenish expenditure authority",
        f.h.snapshot(unchanged_addresses), unchanged_state)
    f.h.check("original capture remains a historical gross bill rather than a new debit",
        f.state("FinanceReceipt", alias["receipt"])["data"]["amount"], 50_000)
    prior_financial = f.book_snapshot()
    preserved = [tab["claim"],run["run"],tab["facility"],
        f.parties["financier"]["custody"],f.parties["financier"]["custody_token"]]
    prior_custody = f.h.snapshot(preserved)
    f.collect(collection,label="original collection retry after full refund recovers its old receipt without paying again")
    f.h.check("original collection recovery leaves all books unchanged",f.book_snapshot(),prior_financial)
    f.h.check("original collection recovery cannot reset bill or backing corrections",f.h.snapshot(preserved),prior_custody)
    f.collect(f.collection(tab, later, 10_000), label="replacement supplier owner services later work to the original accepted wallet")
    f.release(tab, "remaining-work-still-unused", 40_000)
    f.withdraw(f.withdrawal("unpaid-reductions-release-eligible-wallet-cash", 100_000))
    f.h.check("correction never cancelled drawn issuer-to-financier debt", f.state("FundingFacility", f.facility)["drawn"], 30_000)
    f.assert_protected("after corrected backing withdrawal")
    f.execute("issuer later services the still-separate financier debt after supplier bill corrections",
        f.facility_repayment("facility-after-correction",30_000,maximum_interest=0,protocol=False))
    facility = f.state("FundingFacility",f.facility)
    f.h.check("only actual later facility repayment settles outstanding financier principal",facility["drawn"],0)
    f.h.check("facility records the actual cumulative principal repayment",facility["repaid"],30_000)
    f.h.check("facility settlement does not erase remaining company debt",f.state("FundedClaim",tab["claim"])["debt"],10_000)
    f.positions("issuer",facility_debt=0)
    f.positions("financier",facility_assets=0)
    f.assert_origin("after later collection and unused release", tab, run, origin)


def ownership_and_repayments(f):
    f.setup()
    tab = f.acceptance("title-repayment-corrections")
    # A separate line has its own accepted interest allocation; no production rate
    # is selected by these explicit synthetic fixture terms.
    waiver_line, waiver_policy = f.open_line(f.facility, "interest-waiver-line")
    waiver_tab = f.acceptance("interest-waiver", cap=20_000, maximum_interest=2_000,
        line=waiver_line, policy=waiver_policy, fees=False)
    f.accept(tab)
    f.accept(waiver_tab)
    run = f.earn(tab, "partial-paid-and-repaid", 80_000)
    waiver_run = f.earn(waiver_tab, "waivable-interest-run", 20_000)
    origin = f.remember_origin(tab, run)
    wallet = f.parties["financier"]["profile"] == 1
    collection = f.collection(tab, run, 50_000,
        sponsor=dict(payer="supplier", funding_source="EarnedSupplierProceeds") if wallet
            else dict(payer="assignee", funding_source="OwnerCash"))
    f.collect(collection, label="supplier collects gross-to-net with separately accepted sponsor reimbursement")
    f.h.check("source collection includes every separate reimbursement and fee party",
        len(f.state("FinanceReceipt", collection["receipt"])["data"]["effects"]), 6 if wallet else 7)
    f.transfer_tokens("fund explicit third-party principal and interest repayment", f.parties["sponsor"]["owner_ata"], 54_000)
    f.transfer_tokens("fund explicit independent cash contributor for negotiated adjustments", f.parties["fee"]["owner_ata"], 80_000)
    f.advance(YEAR // 2)
    repayment = f.repayment(tab, run, "third-party-pays-before-title-sale", 54_000,
        payer="sponsor", creditor="issuer", maximum_interest=4_000)
    f.execute("actual third-party owner pays interest and principal before title assignment", repayment)
    f.h.check("principal payment stays separate from collected interest",
        f.state("FinanceReceipt", repayment["receipt"])["data"]["principal_paid"], 50_000)
    assignment = f.assignment(tab, run, "sell-thirty-thousand-residual", consideration=28_000)
    f.execute("issuer assigns only the remaining principal and future interest", assignment)
    f.rotate("sponsor", 73, "repayment-payer-before-refund")
    before_mixed = f.correction(tab, run, "mixed-after-repayment-assignment", unpaid=20_000,
        returned=20_000, collection=collection, repayment=repayment, funder="fee")
    f.correct("both source receipts support one atomic seven-book correction", before_mixed)
    f.h.check("mixed correction has seven distinct affected books",
        len(f.state("FinanceReceipt", before_mixed["receipt"])["data"]["effects"]), 7)
    f.h.check("historical payer refund goes to original cash source after owner rotation",
        f.h.amount(repayment["accounts"]["from"]), 10_000)
    f.h.check("replacement payer owner gets no redirected refund", f.h.amount(f.parties["sponsor"]["owner_ata"]), 0)
    f.h.check("current assignee is compensated before prior payer overpayment returns",
        before_mixed["plan"]["creditor_cash"], 20_000)
    f.h.check("assignment consideration remains historical acquisition basis",
        f.state("FundedRun", run["run"])["acquisition_basis"], 28_123)
    f.correct("old operation survives later owner and title changes", before_mixed, replay=True)
    second_collection = f.collection(tab, run, 10_000)
    f.collect(second_collection, label="supplier still collects the uncorrected ten-thousand residual after assignee debt closes")
    f.correct("per-collection return capacity cannot be exceeded by a different ID",
        f.correction(tab, run, "over-original-collection-cap", returned=30_001,
            collection=collection, repayment=repayment, funder="fee"), reject=True)
    f.correct("refund cannot be redirected to a replacement owner",
        f.correction(tab, run, "wrong-historical-refund-route", returned=1,
            collection=collection, repayment=repayment, funder="fee",
            account_overrides=dict(refund_destination=f.parties["sponsor"]["owner_ata"])), reject=True)
    first_return = f.correction(tab, run, "closed-debt-original-collection-return", returned=30_000,
        collection=collection, repayment=repayment, funder="fee")
    f.correct("closed-debt refund needs no new creditor approval and consumes original repayment capacity", first_return)
    final_return = f.correction(tab, run, "closed-debt-second-collection-return", returned=10_000,
        collection=second_collection, repayment=repayment, funder="fee")
    f.correct("separate collection lineage returns the last ten-thousand overpayment", final_return)
    f.h.check("original third-party principal is refunded exactly once", f.h.amount(repayment["accounts"]["from"]), 50_000)
    f.h.check("collected interest remains separate from principal refunds",
        f.state("FundedRun", run["run"])["interest"]["collected"], 4_000)
    f.h.check("protocol take remains collected until separately authorized",
        f.h.amount(f.parties["protocol"]["custody_token"]), 1_480)
    f.h.check("sponsorship remains earned and is not silently refunded",
        f.state("FinanceBook", f.parties["sponsor"]["book"])["data"]["positions"]["sponsor_margin"], 50)
    f.transfer_tokens("fund only part of a protocol recipient's voluntary interest refund",
        f.parties["protocol"]["owner_ata"], 1_000)
    f.refund_interest("second voluntary refund leg failure rolls back the first cash transfer and all usage",
        f.interest_refund(tab, run, repayment, "interest-second-leg-insufficient", 1_000, 1_480), reject=True)
    interest_partial = f.interest_refund(tab, run, repayment, "partial-historical-interest-return", 1_000, 500)
    f.refund_interest("original interest recipients separately return part of actual collected interest", interest_partial)
    f.refund_interest("current assignee cannot debit income collected by the historical issuer",
        f.interest_refund(tab, run, repayment, "wrong-interest-recipient", 100, lender="assignee"), reject=True)
    f.rotate("issuer", 74, "historical-interest-recipient")
    f.transfer_tokens("new issuer owner explicitly funds remaining historic interest return", f.parties["issuer"]["owner_ata"], 1_520)
    f.transfer_tokens("protocol owner adds only remaining approved return cash", f.parties["protocol"]["owner_ata"], 480)
    interest_final = f.interest_refund(tab, run, repayment, "remaining-historical-interest-return", 1_520, 980)
    f.refund_interest("replacement historical lender owner returns its remaining collected interest", interest_final)
    f.refund_interest("original interest refund remains recoverable after its recipient changes owner", interest_partial, replay=True)
    f.refund_interest("a new interest refund ID cannot exceed the original protocol cash", 
        f.interest_refund(tab, run, repayment, "exhausted-protocol-cash", 0, 1), reject=True)
    f.h.check("actual payer receives original principal and both original interest allocations", f.h.amount(repayment["accounts"]["from"]), 54_000)
    f.correct("fully credited run cannot be corrected under yet another ID",
        f.correction(tab, run, "nothing-left-to-refund", plan_override=before_mixed["plan"],
            returned=1, collection=second_collection, repayment=repayment, funder="fee"), reject=True)
    f.assert_origin("fully refunded after title and payer-owner changes", tab, run, origin)
    waive = f.correction(waiver_tab, waiver_run, "interest-only-waiver", waiver=1_000, maximum_interest=1_000)
    f.correct("company and current creditor waive accrued unpaid interest without changing supplier bill", waive)
    f.h.check("interest-only correction preserves supplier principal bill",
        f.state("FundedRun", waiver_run["run"])["principal"]["outstanding"], 20_000)
    f.advance(YEAR//4)
    f.correct("full principal credit retains independently accrued unpaid interest",
        f.correction(waiver_tab,waiver_run,"principal-closed-interest-still-owed",unpaid=20_000,maximum_interest=500))
    f.h.check("closing principal does not erase the remaining interest obligation",
        f.state("FundedClaim",waiver_tab["claim"])["interest_outstanding"],500)
    f.correct("later interest-only waiver closes debt without supplier approval and releases only unearned charge capacity",
        f.correction(waiver_tab,waiver_run,"late-interest-only-closure",waiver=500))
    f.h.check("waived historical accrual still counts as consumed lifetime authority",
        f.registry.read(f.h.svm,waiver_tab["reservation"],"Reservation")["consumed"],21_500)


def repaid_before_collection(f):
    f.setup()
    tab = f.acceptance("repaid-before-supplier-collection", fees=False)
    f.accept(tab)
    run = f.earn(tab, "unpaid-supplier-already-repaid-buyer", 60_000)
    original = f.remember_origin(tab, run)
    paid = f.repayment(tab, run, "buyer-pays-twenty-first", 20_000, creditor="issuer", maximum_interest=0)
    f.execute("buyer can repay before supplier exercises its collection right", paid)
    f.execute("creditor assigns remaining forty-thousand buyer debt",
        f.assignment(tab, run, "assignment-before-unpaid-credit", consideration=38_000, costs=0))
    f.correct("full cash compensation makes an unpaid credit neutral to the assignee without fresh approval",
        f.correction(tab, run, "cash-compensated-unpaid-credit", unpaid=10_000, compensation=10_000, funder="issuer"))
    later_paid = f.repayment(tab, run, "later-third-party-payment-after-correction", 5_000,
        payer="fee", creditor="assignee", maximum_interest=0)
    f.execute("later third-party repayment follows current title after a partial correction", later_paid)
    f.correct("cancelling an unpaid invoice after repayment requires real cash for the actual payer",
        f.correction(tab, run, "unpaid-forty-thousand-credit", unpaid=40_000, repayment=paid, funder="issuer"))
    f.h.check("no facility draw is invented by correcting an uncollected bill", f.state("FundingFacility", f.facility)["drawn"], 0)
    f.correct("first historical payer receives only its remaining original cash capacity",
        f.correction(tab, run, "unpaid-final-first-payer-credit", unpaid=5_000, repayment=paid, funder="issuer"))
    f.correct("second historical payer receives its own separately attributed overpayment",
        f.correction(tab, run, "unpaid-final-second-payer-credit", unpaid=5_000, repayment=later_paid, funder="issuer"))
    f.h.check("actual original payer recovers exactly principal previously debited", f.h.amount(f.parties["company"]["owner_ata"]), 200_000)
    f.h.check("actual later payer independently recovers its principal", f.h.amount(f.parties["fee"]["owner_ata"]), 10_000)
    f.h.check("no correction claws back historical assignment cash", f.h.amount(f.parties["issuer"]["custody_token"]), 258_000)
    f.assert_origin("uncollected bill fully corrected after repayment and assignment", tab, run, original)
    f.release(tab, "unearned-forty-thousand-release", 40_000)
    f.assert_protected("full unpaid correction and remaining release")


class SameBeneficiaryFixture(CorrectionFixture):
    def ix(self, program, name, accounts, **kwargs):
        if program == "claims_core" and name in ("open_funding_facility","open_customer_credit_line"):
            accounts={**accounts,"protocol_custody":self.parties["issuer"]["custody"]}
        return super().ix(program,name,accounts,**kwargs)

    def interest_terms(self, *, rate=1000, cap=10_000):
        terms = super().interest_terms(rate=rate, cap=cap)
        issuer = self.parties["issuer"]
        terms.update(protocol_beneficiary=issuer["principal"], protocol_destination=issuer["custody_token"])
        return terms


class ExternalEscrowFixture(CorrectionFixture):
    def setup(self):
        super().setup(financier_wallet=False)


def same_source_interest(f):
    f.setup()
    tab = f.acceptance("same-lender-protocol-beneficiary", fees=False)
    f.accept(tab)
    run = f.earn(tab, "same-beneficiary-actual-repayment", 10_000)
    f.advance(YEAR)
    paid = f.repayment(tab, run, "same-beneficiary-original-repayment", 11_000, creditor="issuer", maximum_interest=1_000)
    issuer = f.parties["issuer"]
    paid["accounts"].update(protocol_custody=issuer["custody"], protocol_destination=issuer["custody_token"])
    paid["instruction"] = f.ix("claims_core", "repay_funded_run", paid["accounts"],
        remaining=f.remaining(["company", "issuer"]), args=paid["args"])
    f.execute("actual repayment allocates lender and protocol cash to the same Principal custody", paid)
    f.transfer_tokens("owner keeps exactly seven hundred available for voluntary interest return",
        f.funder, 199_300, source=issuer["owner_ata"], authority=issuer["owner"])
    f.refund_interest("same OwnerCash source cannot spend its original balance twice",
        f.interest_refund(tab, run, paid, "alias-source-total-insufficient", 630, 370, protocol="issuer"), reject=True)
    f.transfer_tokens("owner explicitly funds the combined aliased refund amount", issuer["owner_ata"], 300)
    first = f.interest_refund(tab, run, paid, "alias-source-partial", 200, 100, protocol="issuer")
    f.refund_interest("same Principal and same ATA authorize two separately capped refund legs", first)
    f.refund_interest("same-source remainder completes both original allocations",
        f.interest_refund(tab, run, paid, "alias-source-remainder", 430, 270, protocol="issuer"))
    f.h.check("same source returns exactly one thousand once", f.h.amount(issuer["owner_ata"]), 0)
    f.refund_interest("same-source historical retry is independent of depleted cash", first, replay=True)


def escrow_own_capital(f):
    f.setup()
    facility = f.open_facility("issuer", "correction-own-capital", external=False)
    line, policy = f.open_line(facility, "correction-own-capital")
    mandate = f.grant(facility, 1)
    tab = f.acceptance("escrow-correction", cap=40_000, maximum_interest=0, index=1,
        financier="issuer", facility=facility, line=line, policy=policy, mandate=mandate, fees=False)
    f.accept(tab)
    run = f.earn(tab, "own-capital-delivery", 20_000)
    origin = f.remember_origin(tab, run)
    collected = f.collection(tab, run, 10_000)
    f.collect(collected)
    first = f.correction(tab, run, "own-capital-mixed-partial", unpaid=5_000, returned=5_000, collection=collected)
    f.correct("issuer-owned escrow simultaneously releases unpaid backing and receives returned supplier cash", first)
    f.h.check("creditor and backing custody are intentionally the same account",
        first["accounts"]["custody"], first["accounts"]["creditor_custody"])
    f.correct("same-custody residual correction preserves exact token and reserve accounting",
        f.correction(tab, run, "own-capital-mixed-final", unpaid=5_000, returned=5_000, collection=collected))
    f.positions("issuer", facility_debt=0, facility_assets=0)
    f.release(tab, "own-capital-never-earned-work", 20_000)
    f.h.check("returned cash and released reserve preserve all own capital",
        f.state("FinancierCustody", f.parties["issuer"]["custody"])["accounted_cash"], 200_000)
    f.withdraw(f.withdrawal("all-own-capital-free-again", 200_000, role="issuer"))
    f.assert_origin("own-capital correction and withdrawal", tab, run, origin)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--suite", choices=["lifecycle", "ownership", "ownership-escrow", "repaid-uncollected", "own-capital", "interest-alias"])
    selected = parser.parse_args().suite
    summaries = []
    for name, exercise, fixture_type in (("financing-corrections-lifecycle", lifecycle, CorrectionFixture),
            ("financing-corrections-ownership", ownership_and_repayments, CorrectionFixture),
            ("financing-corrections-ownership-escrow", ownership_and_repayments, ExternalEscrowFixture),
            ("financing-corrections-repaid-uncollected", repaid_before_collection, CorrectionFixture),
            ("financing-corrections-own-capital", escrow_own_capital, CorrectionFixture),
            ("financing-corrections-interest-alias", same_source_interest, SameBeneficiaryFixture)):
        if selected and name != "financing-corrections-" + selected:
            continue
        fixture = fixture_type()
        try:
            exercise(fixture)
        except BaseException as error:
            fixture.finish(name, error=error)
            raise
        summaries.append(fixture.finish(name))
    if not selected:
        (OUT / "financing-corrections-receipt.json").write_text(json.dumps(dict(status="passed", suites=summaries), indent=2)+"\n")


if __name__ == "__main__":
    main()
