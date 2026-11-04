"""Native correction helpers with explicit original-lineage and whole-book checks.

Instruction encoders come from the freshly generated program IDLs. Economic
expectations are supplied by the declared correction rules, not inferred from
the observed program result.
"""
from __future__ import annotations
import copy

from financing_commercial import CommercialFixture
from financing_support import *

sys.path.insert(0, str(ROOT / "foundation/money/model"))
from financed_corrections import apply as correction_oracle, preview as correction_preview, record_repayment_refund


class CorrectionFixture(CommercialFixture):
    CLAIM_ORIGIN_FIELDS = (
        "terms", "acceptance_hash", "supplier_principal", "usage_namespace",
        "service_unit", "unit_price", "external_contract_hash", "credit_line",
        "facility", "custody", "issuer", "issuer_custody", "assignment_policy_hash",
        "seller_fee_agreement", "seller_fee_allocation", "seller_fee_terms", "interest_terms",
    )
    RUN_ORIGIN_FIELDS = (
        "operation_id", "claim", "namespace", "run_id", "service_unit", "units",
        "unit_price", "supplier", "supplier_principal", "debtor", "mint",
        "gross_earned", "capture_receipt", "capture_digest", "interest_terms",
    )

    def register_external_contributor(self, role, seed, amount):
        """Register a real unrelated Principal whose current owner volunteers cash."""
        owner = key(seed)
        self.h.svm.airdrop(owner.pubkey(), 100_000_000_000)
        principal_id = ident("independent-cash-contributor-" + role)
        principal = pda(REGISTRY, b"principal", owner.pubkey(), principal_id)
        domain = ident("unrelated-contributor-domain-" + role)
        book = pda(CORE, b"financial-book", principal, self.mint)
        projection = pda(POOL, b"financial-projection", book)
        owner_ata = self.h.ata(owner.pubkey(), self.mint)
        self.parties[role] = dict(name=role, owner=owner, principal_id=principal_id,
            principal=principal, generation=1, book=book, projection=projection, owner_ata=owner_ata)
        self.observe(principal, book, projection, owner_ata)
        self.send("independent owner registers a contributor in another domain",
            self.ix("principal_registry", "create_principal",
                dict(owner=owner.pubkey(), principal=principal),
                args=dict(principal_id=principal_id, domain=domain, kind=1,
                    governing_policy=ident("single-owner-ed25519-profile"))), [owner])
        self.send("initialize unrelated contributor's authenticated financial book",
            self.ix("claims_core", "initialize_finance_book", dict(book=book), principal=principal))
        self.send("initialize unrelated contributor's Pool projection",
            self.ix("pool_issuer", "initialize_financial_projection", dict(book=book, projection=projection)))
        self.send("create unrelated contributor's canonical owner token account", self.h.ata_ix(owner.pubkey(), self.mint))
        self.transfer_tokens("fund only the independent owner's volunteered synthetic cash", owner_ata, amount)
        actual = self.registry.read(self.h.svm, principal, "Principal")
        self.h.check("voluntary contributor is genuinely outside the bill's domain", actual["domain"] != self.domain, True)
        self.h.check("voluntary contribution retains authenticated current owner", actual["owner"], owner.pubkey())

    def remember_origin(self, tab, run):
        claim = self.state("FundedClaim", tab["claim"])
        billed = self.state("FundedRun", run["run"])
        immutable_accounts = (run["receipt"], run["payment"])
        return dict(
            claim={name: copy.deepcopy(claim[name]) for name in self.CLAIM_ORIGIN_FIELDS},
            run={name: copy.deepcopy(billed[name]) for name in self.RUN_ORIGIN_FIELDS},
            principal_originated=billed["principal"]["originated"],
            immutable_data={str(address): bytes(self.h.svm.get_account(address).data)
                for address in immutable_accounts},
        )

    def assert_origin(self, label, tab, run, original):
        claim = self.state("FundedClaim", tab["claim"])
        billed = self.state("FundedRun", run["run"])
        self.h.check(label + " frozen accepted claim",
            {name: claim[name] for name in self.CLAIM_ORIGIN_FIELDS}, original["claim"])
        self.h.check(label + " original delivered run",
            {name: billed[name] for name in self.RUN_ORIGIN_FIELDS}, original["run"])
        self.h.check(label + " original principal remains attributed",
            billed["principal"]["originated"], original["principal_originated"])
        for address, data in original["immutable_data"].items():
            self.h.check(label + " immutable capture/payment record " + address,
                bytes(self.h.svm.get_account(Pubkey.from_string(address)).data), data)
        expected_run = pda(CORE, b"usage", billed["supplier_principal"],
            billed["namespace"], billed["run_id"])
        self.h.check(label + " continuing supplier determines run address", run["run"], expected_run)
        self.h.check(label + " principal conservation",
            billed["principal"]["originated"],
            sum(billed["principal"][field] for field in ("outstanding", "repaid", "extinguished")))

    def book_snapshot(self):
        return {role: copy.deepcopy(self.state("FinanceBook", party["book"])["data"]["positions"])
            for role, party in self.parties.items()}

    def assert_book_deltas(self, label, before, changes):
        """Assert every field of every known book, including unchanged parties."""
        for role, original in before.items():
            expected = copy.deepcopy(original)
            for field, delta in changes.get(role, {}).items():
                assert field in expected, ("Unknown independent book expectation", field)
                expected[field] += delta
                assert expected[field] >= 0, ("Invalid economic expectation", role, field)
            actual = self.state("FinanceBook", self.parties[role]["book"])["data"]["positions"]
            self.h.check(label + " complete financial book " + role, actual, expected)

    def assert_protected(self, label, role="financier"):
        party = self.parties[role]
        custody = self.state("FinancierCustody", party["custody"])
        actual = self.h.amount(party["custody_token"])
        self.h.check(label + " actual cash covers allocated backing", actual >= custody["accounted_cash"], True)
        self.h.check(label + " allocated cash covers all accepted rights",
            custody["accounted_cash"] >= custody["unused"] + custody["payable"], True)

    def role_of(self, principal):
        return next(role for role, party in self.parties.items() if party["principal"] == principal)

    @staticmethod
    def interest_preview(run, now):
        """Independent integer simple-interest calculation for the fixture clock."""
        terms, old = run["interest_terms"], run["interest"]
        elapsed = max(0, min(now, terms["accrual_cutoff"]) - old["last_accrued_at"])
        denominator = 10_000 * terms["seconds_per_year"]
        numerator = (run["principal"]["outstanding"] * terms["annual_rate_bps"] * elapsed
            + old["fractional_numerator"])
        return min(terms["maximum_interest"] - old["accrued"], numerator // denominator)

    def correction(self, tab, run, name, unpaid=0, returned=0, *, compensation=0,
            waiver=0, maximum_interest=0, collection=None, repayment=None,
            funder="supplier", creditor=None, outage=False, reason=None,
            plan_override=None, account_overrides=None, arg_overrides=None,
            remaining_roles=None, omit_signers=()):
        claim = self.state("FundedClaim", tab["claim"])
        delivered = self.state("FundedRun", run["run"])
        plan = plan_override or correction_preview(delivered["principal"], delivered["gross_paid"],
            delivered["corrections"], unpaid, returned, compensation)
        company, supplier = [self.parties[role] for role in ("company", "supplier")]
        creditor = creditor or self.role_of(delivered["current_creditor"])
        current, backing = self.parties[creditor], self.parties[tab["financier"]]
        funding = self.parties[funder]
        consent = plan["creditor_shortfall"] > 0 or waiver > 0
        raw = ident(name)
        op = self.operation(b"correct-run", run["run"], raw)
        receipt = self.receipt(self.company, op)
        result = pda(CORE, b"correction-result", run["run"], op)
        collection = collection["receipt"] if isinstance(collection, dict) else collection
        repayment = repayment["receipt"] if isinstance(repayment, dict) else repayment
        collection_usage = pda(CORE, b"correction-collection", collection) if collection else None
        repayment_usage = pda(CORE, b"correction-repayment", repayment) if repayment else None
        original = self.state("FinanceReceipt", repayment)["data"] if repayment else None
        args = dict(request_id=raw, reason_hash=reason or ident("authorized-provider-adjustment-" + name),
            unpaid_reduction=unpaid, collected_return=returned, creditor_compensation=compensation,
            interest_waiver=waiver, maximum_new_interest=maximum_interest,
            expected_claim_revision=claim["revision"], expected_ownership_revision=delivered["ownership_revision"],
            company_owner_generation=company["generation"],
            supplier_owner_generation=supplier["generation"] if plan["total"] else 0,
            creditor_owner_generation=current["generation"] if consent else 0,
            funding_owner_generation=funding["generation"] if plan["required_cash"] else 0,
            service_without_pool=outage)
        accounts = dict(company_owner=company["owner"].pubkey(),
            supplier_owner=supplier["owner"].pubkey() if plan["total"] else None,
            principal=self.company, supplier_principal=supplier["principal"], creditor_principal=current["principal"],
            creditor_owner=current["owner"].pubkey() if consent else None,
            funding_principal=funding["principal"] if plan["required_cash"] else None,
            funding_authority=funding["owner"].pubkey() if plan["required_cash"] else None,
            funding_from=funding["owner_ata"] if plan["required_cash"] else None,
            budget=self.budget, reservation=tab["reservation"],
            registry_accrual_operation=self.registry_interest_operation(op, run["run"]),
            registry_release_operation=self.registry_interest_operation(op, run["run"], release=True),
            line=tab["line"], claim=tab["claim"], run=run["run"], custody=backing["custody"],
            custody_token=backing["custody_token"], facility=tab["facility"],
            creditor_custody=current["custody"] if plan["creditor_cash"] else None,
            creditor_destination=current["custody_token"] if plan["creditor_cash"] else None,
            original_collection=collection, collection_usage=collection_usage,
            original_repayment=repayment, repayment_usage=repayment_usage,
            refund_destination=original["debit_from"] if original else None,
            receipt=receipt, correction=result)
        roles = ["company", creditor]
        if plan["total"]:
            roles += ["supplier"]
        if unpaid:
            roles += ["issuer", tab["financier"]]
        if plan["required_cash"]:
            roles += [funder]
        if plan["prior_repayment_refund"] and original:
            roles += [self.role_of(original["debit_principal"])]
        signers = [company["owner"]]
        if plan["total"]:
            signers += [supplier["owner"]]
        if consent:
            signers += [current["owner"]]
        if plan["required_cash"]:
            signers += [funding["owner"]]
        accounts.update(account_overrides or {})
        args.update(arg_overrides or {})
        self.observe(receipt, result, *(address for address in (collection_usage, repayment_usage) if address))
        instruction = self.ix("claims_core", "correct_financed_run", accounts,
            remaining=self.remaining(roles if remaining_roles is None else remaining_roles, outage), args=args)
        return dict(instruction=instruction, receipt=receipt, result=result,
            signers=[keypair for keypair in signers if keypair.pubkey() not in omit_signers],
            args=args, accounts=accounts, plan=plan, tab=tab, run=run,
            creditor=creditor, funder=funder, original_repayment=original)

    def assert_result(self, request):
        result = self.state("CorrectionReceipt", request["result"])
        unsigned = {**result, "result_hash": bytes(32)}
        expected_hash = hashlib.sha256(b"DEXTER:FUNDED-CORRECTION-RESULT:1" + bytes(CORE)
            + self.core.encode({"defined": {"name": "CorrectionReceipt"}}, unsigned)).digest()
        self.h.check("correction result hash covers exact immutable response", result["result_hash"], expected_hash)
        receipt = self.state("FinanceReceipt", request["receipt"])["data"]
        self.h.check("financial journal names its precise correction result", receipt["related_receipt"], request["result"])
        self.h.check("correction result commits complete affected-book journal", result["financial_effect_hash"], receipt["effects_hash"])
        self.h.check("correction result retains exact accepted request", result["request"], request["args"])
        return result

    def correct(self, label, request, *, reject=False, replay=False):
        if reject or replay:
            before = self.h.snapshot(self.h.observe)
            self.execute(label, request, reject)
            if replay:
                self.h.check(label + " no protected state changes", self.h.snapshot(self.h.observe), before)
                self.assert_result(request)
            return
        tab, run, args = request["tab"], request["run"], request["args"]
        old_run = self.state("FundedRun", run["run"])
        old_claim = self.state("FundedClaim", tab["claim"])
        old_facility = self.state("FundingFacility", tab["facility"])
        old_budget = self.registry.read(self.h.svm, self.budget, "Budget")
        old_reservation = self.registry.read(self.h.svm, tab["reservation"], "Reservation")
        books = self.book_snapshot()
        before_tokens = {str(address): self.h.amount(address) for party in self.parties.values()
            for address in (party["owner_ata"], party.get("custody_token")) if address}
        for field in ("funding_from", "refund_destination", "creditor_destination"):
            address = request["accounts"].get(field)
            if address:
                before_tokens[str(address)] = self.h.amount(address)
        after_corrections, after_principal, plan = correction_oracle(old_run["principal"], old_run["gross_paid"],
            old_run["corrections"], args["unpaid_reduction"], args["collected_return"], args["creditor_compensation"])
        new_interest = self.interest_preview(old_run, self.now)
        outstanding_interest = old_run["interest"]["accrued"] + new_interest - old_run["interest"]["collected"] \
            - old_run["interest"]["waived"] - args["interest_waiver"]
        released = (old_run["interest_terms"]["maximum_interest"] - old_run["interest"]["accrued"]
            - new_interest - old_run["fee_authority_released"]) if after_principal["outstanding"] == 0 and outstanding_interest == 0 else 0
        self.execute(label, request)
        result = self.assert_result(request)
        self.h.check(label + " independent economic plan", result["plan"], plan)
        self.h.check(label + " independent principal evolution", result["principal_after"], after_principal)
        self.h.check(label + " independent monotonic bill adjustments", result["corrections_after"], after_corrections)
        self.h.check(label + " interest before changing principal", result["newly_accrued_interest"], new_interest)
        self.h.check(label + " only unused charge authority is released", result["unused_charge_authority_released"], released)
        current = self.state("FundedRun", run["run"])
        self.h.check(label + " durable run principal", current["principal"], after_principal)
        self.h.check(label + " durable run correction counters", current["corrections"], after_corrections)
        for field in ("gross_paid", "gross_earned", "ownership_revision", "current_creditor", "acquisition_basis", "capture_receipt"):
            self.h.check(label + " historical " + field, current[field], old_run[field])
        for field in ("collected", "protocol_collected", "protocol_waived"):
            self.h.check(label + " no implicit collected-interest refund " + field,
                current["interest"][field], old_run["interest"][field])
        facility = self.state("FundingFacility", tab["facility"])
        expected_facility = {**old_facility}
        if args["unpaid_reduction"]:
            expected_facility["reserved"] -= args["unpaid_reduction"]
            expected_facility["revision"] += 1
        self.h.check(label + " separate financier facility debt and terms", facility, expected_facility)
        claim = self.state("FundedClaim", tab["claim"])
        for field in ("paid", "repaid", "released", "seller_fee_state", "seller_fee_allocation"):
            self.h.check(label + " no implicit reset of claim " + field, claim[field], old_claim[field])
        self.h.check(label + " unpaid supplier right reduced only by surrender", claim["payable"], old_claim["payable"]-args["unpaid_reduction"])
        budget = self.registry.read(self.h.svm, self.budget, "Budget")
        reservation = self.registry.read(self.h.svm, tab["reservation"], "Reservation")
        self.h.check(label + " lifetime expenditure authority never replenished", budget["consumed"], old_budget["consumed"]+new_interest)
        self.h.check(label + " reservation keeps historical consumed authority", reservation["consumed"], old_reservation["consumed"]+new_interest)
        self.h.check(label + " correction is distinct from unused principal release", reservation["released"], old_reservation["released"]+released)
        changes = {}
        def change(role, field, amount):
            target = changes.setdefault(role, {})
            target[field] = target.get(field, 0) + amount
        change("company", "customer_principal", -plan["principal_credit"])
        change(request["creditor"], "owned_principal", -plan["principal_credit"])
        change("company", "interest_owed", new_interest-args["interest_waiver"])
        change(request["creditor"], "interest_owned", new_interest-args["interest_waiver"])
        change("company", "credit_charge_exposure", -args["interest_waiver"]-released)
        if args["unpaid_reduction"]:
            change("issuer", "unpaid_supplier", -args["unpaid_reduction"])
            change("supplier", "supplier_receivable", -args["unpaid_reduction"])
            change(tab["financier"], "funding_reserved", -args["unpaid_reduction"])
        if plan["required_cash"]:
            change(request["funder"], "cash_sent", plan["required_cash"])
        if plan["creditor_cash"]:
            change(request["creditor"], "cash_received", plan["creditor_cash"])
        if plan["prior_repayment_refund"]:
            change(self.role_of(request["original_repayment"]["debit_principal"]), "cash_received", plan["prior_repayment_refund"])
        self.assert_book_deltas(label, books, changes)
        expected_tokens = dict(before_tokens)
        if plan["required_cash"]:
            expected_tokens[str(request["accounts"]["funding_from"])] -= plan["required_cash"]
        if plan["creditor_cash"]:
            expected_tokens[str(request["accounts"]["creditor_destination"])] += plan["creditor_cash"]
        if plan["prior_repayment_refund"]:
            expected_tokens[str(request["accounts"]["refund_destination"])] += plan["prior_repayment_refund"]
        for address, expected in expected_tokens.items():
            self.h.check(label + " exact token balance " + address, self.h.amount(Pubkey.from_string(address)), expected)
        self.assert_protected(label, tab["financier"])
        return result

    def interest_refund(self, tab, run, original, name, lender_amount=0, protocol_amount=0,
            *, lender=None, protocol="protocol", outage=False, account_overrides=None, arg_overrides=None):
        original = original["receipt"] if isinstance(original, dict) else original
        source = self.state("FinanceReceipt", original)["data"]
        lender = lender or self.role_of(source["current_creditor"])
        previous, take = self.parties[lender], self.parties[protocol]
        raw = ident(name)
        operation = self.operation(b"refund-run-interest", run["run"], raw)
        receipt = self.receipt(self.company, operation)
        result = pda(CORE, b"interest-refund-result", run["run"], operation)
        usage = pda(CORE, b"correction-repayment", original)
        accounts = dict(principal=self.company, claim=tab["claim"], run=run["run"],
            original_repayment=original, repayment_usage=usage,
            lender_principal=previous["principal"] if lender_amount else None,
            lender_authority=previous["owner"].pubkey() if lender_amount else None,
            lender_from=previous["owner_ata"] if lender_amount else None,
            protocol_principal=take["principal"] if protocol_amount else None,
            protocol_authority=take["owner"].pubkey() if protocol_amount else None,
            protocol_from=take["owner_ata"] if protocol_amount else None,
            refund_destination=source["debit_from"], receipt=receipt, interest_refund=result)
        args = dict(request_id=raw, reason_hash=ident("voluntary-collected-interest-return-"+name),
            lender_interest_refund=lender_amount, protocol_interest_refund=protocol_amount,
            lender_owner_generation=previous["generation"] if lender_amount else 0,
            protocol_owner_generation=take["generation"] if protocol_amount else 0,
            service_without_pool=outage)
        roles = [self.role_of(source["debit_principal"])]
        signers = []
        if lender_amount:
            roles.append(lender); signers.append(previous["owner"])
        if protocol_amount:
            roles.append(protocol); signers.append(take["owner"])
        accounts.update(account_overrides or {}); args.update(arg_overrides or {})
        self.observe(receipt, result, usage)
        return dict(instruction=self.ix("claims_core", "refund_funded_interest", accounts,
            remaining=self.remaining(roles, outage), args=args), receipt=receipt, result=result,
            usage=usage, signers=signers, args=args, accounts=accounts,
            tab=tab, run=run, source=source, lender=lender, protocol=protocol)

    def refund_interest(self, label, request, *, reject=False, replay=False):
        if reject or replay:
            before = self.h.snapshot(self.h.observe)
            self.execute(label, request, reject)
            if replay:
                self.h.check(label + " replay has no protected effect", self.h.snapshot(self.h.observe), before)
            return
        tab, run, args = request["tab"], request["run"], request["args"]
        protected_rights = [tab["claim"], run["run"], run["receipt"], request["accounts"]["original_repayment"],
            tab["facility"], self.budget, tab["reservation"]]
        protected_rights += [party["custody"] for party in self.parties.values() if "custody" in party]
        before_rights = self.h.snapshot(protected_rights)
        books = self.book_snapshot()
        before_usage = self.state("RepaymentRefundRecord", request["usage"])["usage"] \
            if self.h.svm.get_account(request["usage"]) else dict(principal_refunded=0, lender_interest_refunded=0, protocol_interest_refunded=0)
        expected_usage = record_repayment_refund(before_usage, request["source"],
            lender_interest=args["lender_interest_refund"], protocol_interest=args["protocol_interest_refund"])
        old_cash = {str(address): self.h.amount(address) for address in
            (request["accounts"]["lender_from"], request["accounts"]["protocol_from"], request["accounts"]["refund_destination"]) if address}
        self.execute(label, request)
        result = self.state("InterestRefundReceipt", request["result"])
        unsigned = {**result, "result_hash": bytes(32)}
        hashed = hashlib.sha256(b"DEXTER:FUNDED-INTEREST-REFUND-RESULT:1" + bytes(CORE)
            + self.core.encode({"defined": {"name": "InterestRefundReceipt"}}, unsigned)).digest()
        self.h.check(label + " immutable interest refund result hash", result["result_hash"], hashed)
        self.h.check(label + " independent original receipt capacities", result["usage_after"], expected_usage)
        self.h.check(label + " principal refund capacity cannot refill", result["usage_after"]["principal_refunded"], before_usage["principal_refunded"])
        self.h.check(label + " durable shared original repayment usage", self.state("RepaymentRefundRecord", request["usage"])["usage"], expected_usage)
        self.h.check(label + " original debts, backing, rights and interest history stay intact", self.h.snapshot(protected_rights), before_rights)
        changes = {}
        def change(role, field, amount):
            current = changes.setdefault(role, {})
            current[field] = current.get(field, 0) + amount
        total = args["lender_interest_refund"] + args["protocol_interest_refund"]
        change(self.role_of(request["source"]["debit_principal"]), "cash_received", total)
        for role, field, source in ((request["lender"], "lender_interest_refund", "lender_from"),
                (request["protocol"], "protocol_interest_refund", "protocol_from")):
            if args[field]:
                change(role, "cash_sent", args[field])
                old_cash[str(request["accounts"][source])] -= args[field]
        old_cash[str(request["accounts"]["refund_destination"])] += total
        self.assert_book_deltas(label, books, changes)
        for address, amount in old_cash.items():
            self.h.check(label + " exact voluntary refund cash " + address, self.h.amount(Pubkey.from_string(address)), amount)
        return result

    def rotate(self, role, seed, name):
        party = self.parties[role]
        old_owner, generation = party["owner"], party["generation"]
        replacement = key(seed)
        self.h.svm.airdrop(replacement.pubkey(), 100_000_000_000)
        operation_id = ident("correction-owner-rotation-" + name)
        receipt = pda(REGISTRY, b"owner-rotation", party["principal"], operation_id)
        self.observe(receipt)
        instruction = self.ix("principal_registry", "rotate_principal_owner",
            dict(principal=party["principal"], current_owner=old_owner.pubkey(),
                new_owner=replacement.pubkey(), receipt=receipt),
            args=dict(operation_id=operation_id, expected_authority_generation=generation,
                new_owner=replacement.pubkey()))
        self.send("both current and replacement owners approve " + role + " rotation",
            instruction, [old_owner, replacement])
        party.update(owner=replacement, generation=generation + 1,
            owner_ata=self.h.ata(replacement.pubkey(), self.mint))
        self.observe(party["owner_ata"])
        self.send("create replacement " + role + " owner cash account",
            self.h.ata_ix(replacement.pubkey(), self.mint))
        return old_owner
