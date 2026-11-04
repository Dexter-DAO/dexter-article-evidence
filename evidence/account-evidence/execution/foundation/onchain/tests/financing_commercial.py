#!/usr/bin/env python3
"""New native-v1 financing title, cash, interest, fee and sponsorship checks."""
from financing_fixture import *


class CommercialFixture(FinancingFixture):
    def capital_request(self, name, amount, *, adoption=False, generation=None, revision=None):
        party = self.parties["financier"]
        raw = ident(name)
        tag = b"capital-adopt" if adoption else b"capital-contribute"
        receipt = self.receipt(party["principal"], self.operation(tag, party["owner"].pubkey(), raw))
        accounts = dict(owner=party["owner"].pubkey(), principal=party["principal"], custody=party["custody"],
            custody_token=party["custody_token"], receipt=receipt)
        args = dict(request_id=raw, amount=amount,
            expected_revision=self.state("FinancierCustody", party["custody"])["revision"] if revision is None else revision,
            authority_generation=party["generation"] if generation is None else generation)
        if adoption:
            args["provenance_hash"] = ident("explicit-owner-attestation-for-surplus")
        else:
            accounts["from"] = party["owner_ata"]
        instruction = self.ix("claims_core", "adopt_financier_surplus" if adoption else "contribute_financier_capital",
            accounts, remaining=self.remaining(["financier"]), args=args)
        self.observe(receipt)
        return dict(instruction=instruction, receipt=receipt, signers=[party["owner"]])

    def execute(self, name, request, reject=False):
        self.send(name, request["instruction"], request["signers"], reject=reject, receipt=request["receipt"])

    def registry_interest_operation(self, operation, run, *, release=False):
        domain = b"DEXTER:FUNDED-INTEREST-RELEASE:1" if release else b"DEXTER:FUNDED-INTEREST-ACCRUAL:1"
        request = hashlib.sha256(domain + bytes(CORE) + operation + bytes(run)).digest()
        account = pda(REGISTRY, b"operation", self.company, request)
        self.observe(account)
        return account

    def assignment(self, tab, run, name, *, consideration=57_000, costs=123, seller="issuer", buyer="assignee"):
        selling, buying, fee = [self.parties[x] for x in (seller, buyer, "fee")]
        raw = ident(name)
        operation = self.operation(b"assign-run", selling["owner"].pubkey(), raw)
        receipt = self.receipt(self.company, operation)
        self.observe(receipt)
        accounts = dict(seller_owner=selling["owner"].pubkey(), buyer_owner=buying["owner"].pubkey(),
            principal=self.company, seller=selling["principal"], buyer=buying["principal"],
            budget=self.budget, reservation=tab["reservation"],
            registry_accrual_operation=self.registry_interest_operation(operation, run["run"]),
            claim=tab["claim"], run=run["run"], seller_custody=selling["custody"], buyer_custody=buying["custody"],
            purchase_from=buying["owner_ata"], seller_destination=selling["custody_token"],
            buyer_destination=buying["custody_token"], cost_principal=fee["principal"] if costs else None,
            cost_custody=fee["custody"] if costs else None, cost_destination=fee["custody_token"] if costs else None,
            receipt=receipt)
        args = dict(request_id=raw, consideration=consideration, acquisition_costs=costs,
            acquisition_cost_evidence_hash=ident("documented-real-acquisition-cost") if costs else bytes(32),
            expected_claim_revision=self.state("FundedClaim", tab["claim"])["revision"],
            expected_ownership_revision=self.state("FundedRun", run["run"])["ownership_revision"],
            seller_owner_generation=selling["generation"], buyer_owner_generation=buying["generation"])
        roles = ["company", seller, buyer] + (["fee"] if costs else [])
        instruction = self.ix("claims_core", "assign_funded_run", accounts, remaining=self.remaining(roles), args=args)
        return dict(instruction=instruction, receipt=receipt, signers=[selling["owner"], buying["owner"]], args=args, accounts=accounts)

    def repayment(self, tab, run, name, amount, *, payer="company", creditor="assignee", maximum_interest=6_000,
            outage=False, expected_ownership_revision=None, generation=None, protocol=True):
        paying, current, take = [self.parties[x] for x in (payer, creditor, "protocol")]
        raw = ident(name)
        operation = self.operation(b"repay-run", paying["owner"].pubkey(), raw)
        receipt = self.receipt(self.company, operation)
        self.observe(receipt)
        accounts = dict(repayment_authority=paying["owner"].pubkey(), repayment_principal=paying["principal"],
            principal=self.company, budget=self.budget, reservation=tab["reservation"],
            registry_accrual_operation=self.registry_interest_operation(operation, run["run"]),
            registry_release_operation=self.registry_interest_operation(operation, run["run"], release=True),
            line=tab["line"], claim=tab["claim"], run=run["run"], creditor_custody=current["custody"],
            destination=current["custody_token"], protocol_custody=take["custody"],
            protocol_destination=take["custody_token"], receipt=receipt)
        accounts["from"] = paying["owner_ata"]
        args = dict(request_id=raw, amount=amount, maximum_interest=maximum_interest,
            expected_claim_revision=self.state("FundedClaim", tab["claim"])["revision"],
            expected_ownership_revision=self.state("FundedRun", run["run"])["ownership_revision"]
                if expected_ownership_revision is None else expected_ownership_revision,
            repayment_owner_generation=paying["generation"] if generation is None else generation,
            service_without_pool=outage)
        # The protocol flag is retained for historical fixture call sites;
        # accepted sharing terms now fix book membership even when the actual
        # allocation is zero or its destination becomes unavailable.
        terms = self.state("FundedRun", run["run"])["interest_terms"]
        accounts["protocol_custody"] = next((party["custody"] for party in self.parties.values()
            if party.get("custody_token") == terms["protocol_destination"]), None) if terms["protocol_share_bps"] else None
        accounts["protocol_destination"] = terms["protocol_destination"] if terms["protocol_share_bps"] else None
        roles = ["company", payer, creditor]
        if terms["protocol_share_bps"] > 0:
            roles.append(terms["protocol_beneficiary"])
        instruction = self.ix("claims_core", "repay_funded_run", accounts, remaining=self.remaining(roles, outage), args=args)
        return dict(instruction=instruction, receipt=receipt, signers=[paying["owner"]], args=args, accounts=accounts)

    def facility_repayment(self, name, amount, *, maximum_interest=1_800, outage=False, protocol=True):
        paying, financier, take = [self.parties[x] for x in ("issuer", "financier", "protocol")]
        raw = ident(name)
        operation = self.operation(b"repay-facility", paying["owner"].pubkey(), raw)
        receipt = self.receipt(paying["principal"], operation)
        self.observe(receipt)
        accounts = dict(repayment_authority=paying["owner"].pubkey(), repayment_principal=paying["principal"],
            issuer=paying["principal"], facility=self.facility, financier_custody=financier["custody"],
            destination=financier["custody_token"], protocol_custody=take["custody"],
            protocol_destination=take["custody_token"], receipt=receipt)
        accounts["from"] = paying["owner_ata"]
        args = dict(request_id=raw, amount=amount, maximum_interest=maximum_interest,
            expected_facility_revision=self.state("FundingFacility", self.facility)["revision"],
            repayment_owner_generation=paying["generation"], service_without_pool=outage)
        terms = self.state("FundingFacility", self.facility)["interest_terms"]
        accounts["protocol_custody"] = next((party["custody"] for party in self.parties.values()
            if party.get("custody_token") == terms["protocol_destination"]), None) if terms["protocol_share_bps"] else None
        accounts["protocol_destination"] = terms["protocol_destination"] if terms["protocol_share_bps"] else None
        roles = ["issuer", "financier"]
        if terms["protocol_share_bps"] > 0:
            roles.append(terms["protocol_beneficiary"])
        instruction = self.ix("claims_core", "repay_funding_facility", accounts,
            remaining=self.remaining(roles, outage), args=args)
        return dict(instruction=instruction, receipt=receipt, signers=[paying["owner"]],
            args=args, accounts=accounts)

    def fee_credit(self, tab, original, name, amount, *, generation=None, destination=None, outage=False):
        beneficiary, supplier = self.parties["fee"], self.parties["supplier"]
        raw = ident(name)
        operation = self.operation(b"fee-credit", beneficiary["owner"].pubkey(), raw)
        receipt = self.receipt(beneficiary["principal"], operation)
        credits = pda(CORE, b"seller-fee-credits", original)
        self.observe(receipt, credits)
        accounts = dict(beneficiary_owner=beneficiary["owner"].pubkey(), beneficiary=beneficiary["principal"],
            claim=tab["claim"], original_receipt=original, credits=credits,
            destination=destination or supplier["owner_ata"], receipt=receipt)
        accounts["from"] = beneficiary["owner_ata"]
        args = dict(request_id=raw, amount=amount,
            owner_generation=beneficiary["generation"] if generation is None else generation,
            expected_claim_revision=self.state("FundedClaim", tab["claim"])["revision"], service_without_pool=outage)
        instruction = self.ix("claims_core", "credit_seller_fee", accounts,
            remaining=self.remaining(["fee", "supplier"], outage), args=args)
        return dict(instruction=instruction, receipt=receipt, signers=[beneficiary["owner"]])


def exercise(f):
    f.setup()
    financier = f.parties["financier"]
    f.transfer_tokens("fund disposable current financier owner contribution", financier["owner_ata"], 5_000)
    f.execute("capital contribution refuses stale owner generation", f.capital_request("capital-stale-owner", 5_000, generation=2), True)
    contributed = f.capital_request("new-authenticated-owner-capital", 5_000)
    f.execute("owner contribution records actual token inflow and allocated own capital", contributed)
    f.execute("contribution retry works even after source cash is exhausted", contributed)
    data = check_receipt(f.core, f.h.svm, contributed["receipt"])
    f.h.check("capital receipt preserves true transfer amount", data["capital_token_inflow"], 5_000)
    f.h.check("capital receipt names actual owner debit", data["capital_from"], financier["owner_ata"])
    f.transfer_tokens("unsolicited surplus is only a token balance change", financier["custody_token"], 77)
    f.h.check("unsolicited cash is not allocated by mere arrival", f.state("FinancierCustody", financier["custody"])["accounted_cash"], 1_005_000)
    f.execute("owner cannot allocate more than actual surplus", f.capital_request("too-much-surplus", 78, adoption=True), True)
    adopted = f.capital_request("owner-adopts-provenance-bound-surplus", 77, adoption=True)
    f.execute("owner explicitly allocates surplus with provenance without inventing an inflow", adopted)
    f.execute("surplus allocation receipt retry does not allocate twice", adopted)
    f.positions("financier", capital_allocated=1_005_077, cash_received=5_000)

    tab = f.acceptance("commercial-delivery-title")
    f.accept(tab)
    f.exercise_fee_parent(tab)
    run = f.earn(tab, "commercial-completed-run", 60_000)
    original_supplier = f.state("FundedRun", run["run"])["supplier"]
    assigned = f.assignment(tab, run, "whole-residual-assignment")
    f.execute("current creditor assigns delivered whole residual for actual consideration and costs", assigned)
    f.execute("assignment historical retry preserves title and cash", assigned)
    title = f.state("FundedRun", run["run"])
    f.h.check("receivable belongs to current financier after assignment", title["current_creditor"], f.parties["assignee"]["principal"])
    f.h.check("assignment keeps supplier entitled to the same unpaid gross", title["gross_earned"] - title["gross_paid"], 60_000)
    f.h.check("assignment never changes original supplier payment destination", title["supplier"], original_supplier)
    f.h.check("acquisition basis includes only actual paid consideration and evidenced costs", title["acquisition_basis"], 57_123)
    f.execute("former creditor cannot reassign someone else's residual", f.assignment(tab, run, "stale-creditor-assignment"), True)
    f.positions("issuer", owned_principal=0, unpaid_supplier=60_000, cash_received=57_000)
    f.positions("assignee", owned_principal=60_000, cash_sent=57_123)

    bad_sponsor = dict(payer="supplier", funding_source="EarnedSupplierProceeds",
        overrides=dict(native_cost_lamports=600_001, reimbursement=60_001, maximum_total_charge=60_051))
    f.collect(f.collection(tab, run, 60_000, sponsor=bad_sponsor), reject=True,
        label="sponsorship greater than newly earned proceeds rolls back every payment leg")
    bad_sponsor = dict(payer="supplier", funding_source="EarnedSupplierProceeds", overrides=dict(authority_generation=2))
    f.collect(f.collection(tab, run, 60_000, sponsor=bad_sponsor), reject=True,
        label="unapproved current-owner sponsorship refuses and rolls back earned transfer")
    f.collect(f.collection(tab, run, 60_000, sponsor=dict(payer="company", funding_source="OwnerCash")), reject=True,
        label="governed wallet bundle refuses an unqualified owner-cash reimbursement source")
    sponsored = f.collection(tab, run, 60_000, sponsor=dict(payer="supplier", funding_source="EarnedSupplierProceeds"))
    f.collect(sponsored, label="successful wallet supplier collection reimburses approved token cost and margin")
    receipt = check_receipt(f.core, f.h.svm, sponsored["receipt"])
    f.h.check("gross-to-net supplier credit is separate from sponsorship debit", receipt["supplier_net"], 59_171)
    f.h.check("approved sponsor reimbursement is exact", receipt["sponsor_reimbursement"], 500)
    f.h.check("approved sponsor margin is exact", receipt["sponsor_margin"], 50)
    f.h.check("supplier receives actual final tokens after both agreed charges", f.h.amount(f.parties["supplier"]["owner_ata"]), 58_621)
    f.h.check("sponsor custody receives exact approved charge", f.h.amount(f.parties["sponsor"]["custody_token"]), 550)
    f.h.check("supplier entitlement fulfilled while company debt survives", f.state("FundedClaim", tab["claim"])["debt"], 60_000)
    replay = f.collection(tab, run, 60_000, paid_before=0)
    f.collect(replay, label="same collection can recover without reapproving or recharging sponsorship")
    f.h.check("sponsorship can only be charged once", f.h.amount(f.parties["sponsor"]["custody_token"]), 550)
    f.positions("sponsor", cash_received=550, sponsor_reimbursement=500, sponsor_margin=50)
    f.positions("supplier", supplier_receivable=0, cash_received=59_171, cash_sent=550, seller_fees_paid=829)
    f.release(tab, "commercial-unused-work", 40_000)

    escrow_facility = f.open_facility("issuer", "sponsor-owner-cash", external=False)
    escrow_line, escrow_policy = f.open_line(escrow_facility, "sponsor-owner-cash")
    f.grant(escrow_facility, 1)
    escrow_tab = f.acceptance("sponsored-escrow", cap=1_000, maximum_interest=0, index=1,
        financier="issuer", facility=escrow_facility, line=escrow_line, policy=escrow_policy, fees=False)
    f.accept(escrow_tab)
    escrow_run = f.earn(escrow_tab, "sponsored-escrow-run", 1_000)
    escrow_collection = f.collection(escrow_tab, escrow_run, 1_000,
        sponsor=dict(payer="company", funding_source="OwnerCash"))
    f.collect(escrow_collection, label="escrow alternative sponsors collection from separately approved owner cash")
    escrow_data = check_receipt(f.core, f.h.svm, escrow_collection["receipt"])
    f.h.check("owner-cash sponsorship leaves supplier credit intact", escrow_data["supplier_net"], 1_000)
    f.h.check("owner-cash sponsorship is not a supplier-proceeds deduction", escrow_data["supplier_proceeds_reimbursement"], 0)
    f.execute("company repays distinct issuer-owned capital without external self-debt",
        f.repayment(escrow_tab, escrow_run, "repay-sponsored-escrow", 1_000, creditor="issuer", maximum_interest=0))

    f.advance(YEAR)
    f.execute("repayment cannot select former creditor after title assignment",
        f.repayment(tab, run, "wrong-creditor-repayment", 1_000, creditor="issuer"), True)
    f.execute("payer accepted interest ceiling remains enforceable",
        f.repayment(tab, run, "too-low-interest-authorization", 1_000, maximum_interest=999), True)
    partial = f.repayment(tab, run, "third-party-interest-service", 1_000, payer="issuer")
    f.execute("ordinary authorized third-party cash services buyer interest without company approval", partial)
    data = check_receipt(f.core, f.h.svm, partial["receipt"])
    f.h.check("repayment receipt distinguishes actual payer from company debtor", data["debit_principal"], f.parties["issuer"]["principal"])
    f.h.check("partial interest is allocated only from cash collected", data["protocol_interest"], 370)
    f.h.check("interest-only payment preserves customer principal", data["principal_paid"], 0)
    f.h.check("one year of accepted synthetic borrower interest", f.state("FundedRun", run["run"])["interest"]["accrued"], 6_000)
    f.h.check("buyer repayment leaves issuer-financier draw untouched", f.state("FundingFacility", f.facility)["drawn"], 60_000)
    final = f.repayment(tab, run, "customer-clears-title-current-owner", 65_000, maximum_interest=5_000)
    f.execute("company repays remaining principal and interest to current creditor", final)
    f.execute("repayment historical receipt is recovered without repeat debit", final)
    f.h.check("current creditor receives principal plus retained collected interest", f.h.amount(f.parties["assignee"]["custody_token"]), 63_780)
    f.h.check("protocol take is allocated from actual borrower interest", f.h.amount(f.parties["protocol"]["custody_token"]), 2_220)
    f.h.check("original supplier remains fully paid after all financing changes", f.state("FundedRun", run["run"])["gross_paid"], 60_000)
    f.positions("company", customer_principal=0, interest_owed=0, credit_charge_exposure=0, customer_reserved=0, cash_sent=66_550)
    f.positions("assignee", owned_principal=0, interest_owned=0, cash_received=63_780, lender_interest=3_780)

    f.freeze(f.parties["protocol"]["custody_token"])
    waived_take = f.facility_repayment("facility-service-with-protocol-unavailable", 100, maximum_interest=100, protocol=False)
    f.execute("frozen protocol take cannot block accepted financing repayment", waived_take)
    waived_receipt = check_receipt(f.core, f.h.svm, waived_take["receipt"])
    f.h.check("unavailable protocol take is permanently waived for that collection", waived_receipt["protocol_waived"], 37)
    f.h.check("unavailable protocol receives no tokens", waived_receipt["protocol_interest"], 0)
    f.freeze(f.parties["protocol"]["custody_token"], False)
    facility = f.facility_repayment("issuer-clears-distinct-facility-debt", 61_700, maximum_interest=1_700)
    f.execute("issuer separately repays external financier principal and accrued funding interest", facility)
    f.execute("facility repayment receipt retry cannot debit twice", facility)
    f.h.check("issuer facility principal is independently extinguished", f.state("FundingFacility", f.facility)["drawn"], 0)
    f.h.check("restoring protocol availability does not revive waived take", f.h.amount(f.parties["protocol"]["custody_token"]), 2_849)
    f.positions("financier", facility_assets=0, interest_owned=0, lender_interest=1_171, cash_received=66_171)
    f.positions("issuer", facility_debt=0, interest_owed=0, cash_sent=63_800)
    f.positions("protocol", cash_received=2_849, protocol_interest=2_849)

    before_debt = f.h.snapshot([run["run"], tab["reservation"], f.budget, f.facility])
    f.execute("fee credit cannot exceed the original actual collection",
        f.fee_credit(tab, sponsored["receipt"], "oversized-fee-credit", 830), True)
    f.execute("fee credit cannot redirect supplier funds to another owner",
        f.fee_credit(tab, sponsored["receipt"], "wrong-fee-credit-route", 829,
            destination=f.parties["issuer"]["owner_ata"]), True)
    first_credit = f.fee_credit(tab, sponsored["receipt"], "seller-fee-credit-first", 400)
    f.execute("fee beneficiary returns a first actual portion to original supplier", first_credit)
    f.execute("fee-credit receipt recovers without a duplicate return", first_credit)
    rest_credit = f.fee_credit(tab, sponsored["receipt"], "seller-fee-credit-remainder", 429)
    f.execute("fee beneficiary returns the remaining actual collected fee", rest_credit)
    f.h.check("fee credits preserve debt, title, budget and facility facts", f.h.snapshot(list(map(Pubkey.from_string, before_debt))), before_debt)
    f.h.check("fee state distinguishes returned fees from new credit capacity", f.state("FundedClaim", tab["claim"])["seller_fee_state"]["credited"], 829)
    f.h.check("original supplier receives exact cumulative fee credit", f.h.amount(f.parties["supplier"]["owner_ata"]), 60_450)
    f.positions("supplier", seller_fees_paid=829, seller_fee_credits_received=829, cash_received=61_000, cash_sent=550)
    f.positions("fee", seller_fees=829, seller_fee_refunds=829, cash_received=952, cash_sent=829)
    f.execute("returned seller fees cannot be returned a second time under a different ID",
        f.fee_credit(tab, sponsored["receipt"], "seller-fee-credit-overrun", 1), True)
    f.h.check("full fee credit never refills the standing seller authorization", f.fee_allocated(), 2_069)
    f.fee_parent_owner_change()
    f.h.check("financing fee lifetime allocation is not refilled by repayment or refunds",
        f.state("CustomerCreditLine", f.line)["allocated_fee_caps"], 10_000)
    f.h.check("Registry retains consumed principal plus earned interest",
        f.registry.read(f.h.svm, f.budget, "Budget")["consumed"], 67_000)
    f.h.check("Registry frees only never-earned authority",
        f.registry.read(f.h.svm, f.budget, "Budget")["reserved"], 0)
    f.catch_up()


def main():
    f = CommercialFixture()
    try:
        exercise(f)
    except Exception as error:
        f.finish("financing-commercial", error=error)
        raise
    f.finish("financing-commercial")


if __name__ == "__main__":
    main()
