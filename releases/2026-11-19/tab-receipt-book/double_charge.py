#!/usr/bin/env python3
"""Double-charge attacks on Change 5 money moves, refused by numbers and windows.

Every chosen-amount money move carries a number its record's window admits
once: an ordinary repayment its route record's, a facility repayment the
facility's, a withdrawal or contribution its custody's, and an interest refund,
seller-fee credit or correction cash leg its payer's book. Each attack below
is refused with its exact error, changes nothing the harness observes, and is
followed by the legitimate path landing.

It also states the SDK's recovery rule after a lost response: read the
record's window and the history for the intent's event. Bit set and event
found: landed. Bit clear and no event: not landed. Bit set and no matching
event: unknown, never rebuilt.

Uses the compiled Registry/Core/Pool/Swig/SPL programs, signed native v1 and
synthetic tokens; no account bytes are written by the harness.
"""
from financing_governed_repayment import GovernedFixture
from financing_fixture import YEAR
from verify_financing_corrections import signed_request_digest
from financing_support import *

WINDOW_BITS, JUMP_CAP = 1024, 65_536


def window_state(window, number):
    """Where `number` stands in an OrderWindow (contracts window.rs): bit i of
    the map stands for number `highest - i`."""
    if number == 0:
        return "refused"
    highest = window["highest"]
    if number > highest:
        return "open" if number - highest <= JUMP_CAP else "refused"
    distance = highest - number
    if distance >= WINDOW_BITS:
        return "below"
    return "used" if bytes(window["bits"])[distance // 8] & (1 << (distance % 8)) else "open"


def sdk_outcome(window, number, history, matches):
    """The SDK's rule after a lost response, from state first and history second."""
    state = window_state(window, number)
    found = any(matches(name, value) for name, value in history)
    if state == "used":
        return "landed" if found else "unknown"
    if state == "open":
        assert not found, "history shows a number its window never admitted"
        return "not landed"
    if state == "below":
        return "landed" if found else "unknown"
    return "never"


class DoubleChargeFixture(GovernedFixture):
    def __init__(self):
        super().__init__()
        self.attacks = []
        self.sibling_payer = key(142)
        self.h.svm.airdrop(self.sibling_payer.pubkey(), 10_000_000_000)

    def history(self):
        """The events a client can read back (getTransaction for each signature)."""
        return [(event, value) for _, event, value in self.h.ledger.events]

    def attack(self, name, refused, error, landed, **facts):
        rows = {row["name"]: row for row in self.h.cases}
        for label in refused:
            row = rows[label]
            assert row["expected"] == "reject" and row["passed"], label
        self.attacks.append(dict(attack=name, refused=list(refused), error=error, legitimate_path=list(landed),
            refused_changes_nothing=all(rows[label]["observed_after"] == rows[label]["observed_before"] for label in refused),
            **facts))

    def as_payer(self, payer, action):
        original = self.h.payer
        self.h.payer = payer
        try:
            return action()
        finally:
            self.h.payer = original

    def window(self, kind, address):
        state = self.state(kind, address)
        return state["window"]

    # Independent request digests, as the SDK records them at prepare time.
    def run_digest(self, request):
        return self.request_digest(dict(request, kind=3))

    def facility_digest(self, request):
        return self.request_digest(dict(request, kind=4))

    def withdrawal_digest(self, role, name, amount, *, destination=None):
        party = self.parties[role]
        owner = party["owner"].pubkey()
        operation = self.operation(b"withdraw", party["principal"], ident("withdrawal-" + name))
        destination = destination or self.h.ata(owner, self.mint)
        return hashlib.sha256(b"DEXTER:FINANCIER-FREE-WITHDRAWAL:1" + bytes(CORE) + operation + bytes(party["principal"])
            + bytes(owner) + bytes(party["custody"]) + bytes(destination) + u64(amount) + u64(party["generation"])).digest()

    def contribution_digest(self, name, amount):
        party = self.parties["financier"]
        owner = party["owner"].pubkey()
        operation = self.operation(b"capital-contribute", owner, ident(name))
        return hashlib.sha256(b"DEXTER:CONTRIBUTE-FINANCIER-CAPITAL:1" + bytes(CORE) + bytes(party["principal"])
            + bytes(owner) + u64(party["generation"]) + operation + bytes(party["custody"]) + bytes(party["owner_ata"])
            + bytes(party["custody_token"]) + bytes(self.mint) + u64(amount)).digest()

    def credit_digest(self, request):
        a, x = request["instruction"].accounts, request["args"]
        fee, supplier = self.parties["fee"], self.parties["supplier"]
        return hashlib.sha256(b"DEXTER:SELLER-FEE-CREDIT:3" + bytes(CORE) + bytes(fee["principal"])
            + bytes(fee["owner"].pubkey()) + u64(x["owner_generation"]) + bytes(request["tab"]["claim"])
            + bytes(request["run"]["run"]) + bytes(fee["owner_ata"]) + bytes(request["destination"] or supplier["owner_ata"])
            + bytes(self.mint) + u64(x["amount"]) + u64(x["number"])).digest()

    def correction_digest(self, request):
        return signed_request_digest(self.core, "correct_financed_run", request["args"],
            {**request["accounts"], "mint": self.mint})

    def refund_digest(self, request):
        return signed_request_digest(self.core, "refund_funded_interest", request["args"],
            {**request["accounts"], "mint": self.mint})

    def book_window(self, role):
        return self.state("FinanceBook", self.parties[role]["book"])["window"]

    def facility_repayment_by(self, role, name, amount, *, number, maximum_interest=0):
        """An ordinary facility repayment of the issuer's facility paid by `role`;
        its number belongs to the payer's own book window."""
        paying, financier, issuer = self.parties[role], self.parties["financier"], self.parties["issuer"]
        raw = ident(name)
        operation = self.operation(b"repay-facility", paying["owner"].pubkey(), raw)
        accounts = dict(repayment_authority=paying["owner"].pubkey(), repayment_principal=paying["principal"],
            issuer=issuer["principal"], facility=self.facility, financier_custody=financier["custody"],
            destination=financier["custody_token"])
        accounts["from"] = paying["owner_ata"]
        terms = self.state("FundingFacility", self.facility)["interest_terms"]
        accounts["protocol_custody"] = next((party["custody"] for party in self.parties.values()
            if party.get("custody_token") == terms["protocol_destination"]), None) if terms["protocol_share_bps"] else None
        accounts["protocol_destination"] = terms["protocol_destination"] if terms["protocol_share_bps"] else None
        args = dict(request_id=raw, amount=amount, maximum_interest=maximum_interest,
            expected_facility_revision=self.state("FundingFacility", self.facility)["revision"],
            repayment_owner_generation=paying["generation"], number=number, service_without_pool=False)
        roles = ["issuer", "financier", role] + ([terms["protocol_beneficiary"]] if terms["protocol_share_bps"] else [])
        instruction = self.ix("claims_core", "repay_funding_facility", accounts, remaining=self.remaining(roles), args=args)
        # The facility repayment's detail names the issuer, whose facility it pays.
        return dict(instruction=instruction, receipt=self.receipt(issuer["principal"], operation),
            signers=[paying["owner"]], args=args, accounts=accounts)

    def release_request(self, tab, name, amount, *, number=None, expected_steps=None):
        """The supplier's surrender before expiry (FinancingFixture.release), unsent."""
        party, financier = self.parties["supplier"], self.parties[tab["financier"]]
        raw = ident("release-" + name)
        operation = hashlib.sha256(b"DEXTER:FUNDING-OPERATION:1" + bytes(CORE) + b"release"
            + bytes(tab["claim"]) + bytes(tab["supplier_principal"]) + raw).digest()
        args = dict(request_id=raw, amount=amount,
            expected_steps=self.steps(tab["claim"]) if expected_steps is None else expected_steps,
            service_without_pool=False, number=self.number(party["book"]) if number is None else number)
        instruction = self.ix("claims_core", "release_financier_unused",
            dict(principal=self.company, supplier_principal=party["principal"], supplier=party["owner"].pubkey(),
                budget=self.budget, reservation=tab["reservation"], line=tab["line"], claim=tab["claim"],
                facility=tab["facility"], custody=financier["custody"], custody_token=financier["custody_token"]),
            remaining=self.remaining(["company", "issuer", "supplier", tab["financier"]]), **args)
        return dict(instruction=instruction, receipt=self.receipt(self.company, operation), signers=[party["owner"]],
            args=args)

    def exact_grant(self, request, label, *, number, approval, error=None):
        """grant_exact_repayment at a chosen number and approval account."""
        role, wallet = request["role"], request["wallet"]
        party = self.parties[role]
        a, args = request["accounts"], request["args"]
        terms = dict(binding=wallet["binding"], kind=request["kind"], obligation=request["obligation"],
            obligor=request["obligor"], mint=self.mint, debit_from=wallet["custody_token"],
            creditor=request["creditor"], creditor_custody=a["creditor_custody"], creditor_destination=a["destination"],
            protocol_beneficiary=request["terms"]["protocol_beneficiary"] if request["terms"]["protocol_share_bps"] else Pubkey.default(),
            protocol_destination=request["terms"]["protocol_destination"] if request["terms"]["protocol_share_bps"] else Pubkey.default(),
            executor=self.h.payer.pubkey(), authority_generation=party["generation"],
            not_before=self.now, expires_at=self.now + 300, minimum_interval=0,
            per_payment_amount=args["amount"], per_payment_principal=args["amount"],
            per_payment_interest=args["maximum_interest"], total_amount=args["amount"],
            total_principal=args["amount"], total_interest=args["maximum_interest"], maximum_payments=1)
        self.observe(approval)
        instruction = self.ix("principal_registry", "grant_exact_repayment",
            dict(principal=party["principal"], owner=party["owner"].pubkey(), binding=wallet["binding"], approval=approval),
            args=dict(number=number, terms=terms, exact=dict(request_digest=request["digest"], amount=args["amount"])))
        self.send(label, instruction, [party["owner"]], reject=error is not None, error=error)


def lost_response_repayment(f, tab, run):
    """1. Ordinary repayment: the response is lost; state and history decide."""
    intent = f.repayment(tab, run, "lost-response-payment", 10_000, creditor="issuer", maximum_interest=0)
    record, number, digest = intent["record"], intent["args"]["number"], f.run_digest(intent)
    f.h.check("lost-response payment is the route's first number", number, 1)
    f.h.check("before sending, the SDK sees the number as not landed",
        sdk_outcome({"highest": 0, "bits": bytes(128)}, number, f.history(), lambda *_: False), "not landed")
    f.execute("ordinary repayment lands while its response is lost", intent)
    # The client never saw a response. It reads the route record's window and
    # looks for the Repaid event with this number and the intent's digest.
    matches = lambda name, value: (name == "Repaid" and value["repayment_record"] == record
        and value["number"] == number and value["detail"]["request_digest"] == digest)
    window = f.window("RepaymentRecord", record)
    f.h.check("SDK rule: window bit set and the intent's Repaid event found: landed",
        sdk_outcome(window, number, f.history(), matches), "landed")
    f.h.check("SDK rule: the next number's bit is clear and no event exists: not landed",
        sdk_outcome(window, number + 1, f.history(), lambda name, value: name == "Repaid"
            and value["repayment_record"] == record and value["number"] == number + 1), "not landed")
    f.h.check("SDK rule: bit set but the history does not show this intent: unknown, never rebuilt",
        sdk_outcome(window, number, [], matches), "unknown")
    rebuilt = f.repayment(tab, run, "lost-response-payment", 10_000, creditor="issuer", maximum_interest=0, number=number)
    f.h.check("a naive rebuild names the new step count", rebuilt["args"]["expected_steps"], f.steps(tab["claim"]))
    f.execute("naive rebuild of a landed ordinary repayment is refused", rebuilt, True, "NumberUsed")
    f.refused_with("NumberUsed")
    fresh = f.repayment(tab, run, "lost-response-next-payment", 10_000, creditor="issuer", maximum_interest=0)
    f.h.check("the route's next payment takes the next number", fresh["args"]["number"], number + 1)
    f.execute("the route's next payment lands with a fresh number", fresh)
    f.attack("lost response, ordinary repayment (route record window)",
        ["naive rebuild of a landed ordinary repayment is refused"], "NumberUsed",
        ["ordinary repayment lands while its response is lost", "the route's next payment lands with a fresh number"],
        window=str(record), number=number, sdk_rule=dict(landed="bit set and event found", not_landed="bit clear, no event",
            unknown="bit set, no matching event: never rebuilt"))


def sibling_attempts(f, tab, run):
    """3. Two signed attempts of one intent, differing only in fee payer and blockhash."""
    first = f.repayment(tab, run, "sibling-payment", 5_000, creditor="issuer", maximum_interest=0)
    second = f.as_payer(f.sibling_payer, lambda: f.repayment(tab, run, "sibling-payment", 5_000, creditor="issuer",
        maximum_interest=0, number=first["args"]["number"], expected_steps=first["args"]["expected_steps"]))
    f.h.check("sibling repayment attempts are one intent: same request digest",
        f.run_digest(second), f.run_digest(first))
    f.h.check("sibling repayment attempts carry the same number", second["args"]["number"], first["args"]["number"])
    f.execute("first signed attempt of a repayment lands", first)
    f.as_payer(f.sibling_payer, lambda: f.execute(
        "sibling attempt of the same repayment from another fee payer is refused", second, True, "NumberUsed"))
    f.refused_with("NumberUsed")
    f.attack("sibling attempts, ordinary repayment", ["sibling attempt of the same repayment from another fee payer is refused"],
        "NumberUsed", ["first signed attempt of a repayment lands"])

    issuer = f.parties["issuer"]
    one = f.withdrawal("sibling-withdrawal", 1_000, role="issuer")
    other = dict(one)  # The same signed instruction; only the fee payer and blockhash differ.
    before = f.h.amount(issuer["owner_ata"])
    f.withdraw(one, label="first signed attempt of a withdrawal lands")
    f.as_payer(f.sibling_payer, lambda: f.withdraw(other, reject=True,
        label="sibling attempt of the same withdrawal from another fee payer is refused"))
    f.refused_with("NumberUsed")
    f.h.check("the owner receives the withdrawal once", f.h.amount(issuer["owner_ata"]) - before, 1_000)
    f.attack("sibling attempts, withdrawal", ["sibling attempt of the same withdrawal from another fee payer is refused"],
        "NumberUsed", ["first signed attempt of a withdrawal lands"])

    # Two different intents that wrongly share one number: two allocators.
    shared = f.number(first["record"])
    a = f.repayment(tab, run, "shared-number-intent-a", 3_000, creditor="issuer", maximum_interest=0, number=shared)
    b = f.repayment(tab, run, "shared-number-intent-b", 2_000, creditor="issuer", maximum_interest=0, number=shared)
    f.h.check("two different intents", f.run_digest(a) != f.run_digest(b), True)
    f.execute("first intent under a shared number lands", a)
    b_digest = f.run_digest(b)
    f.h.check("SDK rule for intent b without its response: the bit is set by another intent, so unknown",
        sdk_outcome(f.window("RepaymentRecord", a["record"]), shared, f.history(),
            lambda name, value: name == "Repaid" and value["number"] == shared
                and value["detail"]["request_digest"] == b_digest), "unknown")
    f.execute("a different intent reusing that number is refused", b, True, "NumberUsed")
    f.refused_with("NumberUsed")
    # Its refusal is a recorded failure, so intent b did not land and may take
    # its own number from the record's one allocator.
    b_fresh = f.repayment(tab, run, "shared-number-intent-b", 2_000, creditor="issuer", maximum_interest=0)
    f.h.check("one allocator per record gives the second intent its own number", b_fresh["args"]["number"], shared + 1)
    f.execute("the second intent lands under its own number", b_fresh)
    f.attack("two intents sharing one number (why one allocator per record)",
        ["a different intent reusing that number is refused"], "NumberUsed",
        ["first intent under a shared number lands", "the second intent lands under its own number"])


def rebuilt_moves(f, tab, run):
    """2. Facility repayment, withdrawal, contribution, seller-fee credit and
    correction cash leg: each landed, then rebuilt with its number."""
    history = f.history
    # Facility repayment: the paying issuer's book window.
    facility = f.facility_repayment("facility-lost-response", 5_000, maximum_interest=0, protocol=False)
    f.execute("issuer facility repayment lands while its response is lost", facility)
    number, digest = facility["args"]["number"], f.facility_digest(facility)
    f.h.check("SDK rule finds the landed facility repayment", sdk_outcome(f.book_window("issuer"),
        number, history(), lambda name, value: name == "RecordOperation" and value["record"] == f.facility
        and value["number"] == number and value["detail"]["request_digest"] == digest), "landed")
    f.execute("naive rebuild of a landed facility repayment is refused",
        f.facility_repayment("facility-lost-response", 5_000, maximum_interest=0, protocol=False, number=number),
        True, "NumberUsed")
    f.refused_with("NumberUsed")
    fresh = f.facility_repayment("facility-next-payment", 5_000, maximum_interest=0, protocol=False)
    f.h.check("the issuer's next facility repayment takes its next number", fresh["args"]["number"], number + 1)
    f.execute("the facility's next repayment lands with a fresh number", fresh)
    f.attack("lost response, facility repayment (payer's book window)",
        ["naive rebuild of a landed facility repayment is refused"], "NumberUsed",
        ["issuer facility repayment lands while its response is lost", "the facility's next repayment lands with a fresh number"])

    # Withdrawal: the custody's window.
    issuer = f.parties["issuer"]
    withdrawal = f.withdrawal("withdrawal-lost-response", 2_000, role="issuer")
    f.withdraw(withdrawal, label="escrow withdrawal lands while its response is lost")
    number = f.numbers[str(issuer["custody"])]
    digest = f.withdrawal_digest("issuer", "withdrawal-lost-response", 2_000)
    f.h.check("SDK rule finds the landed withdrawal", sdk_outcome(f.window("FinancierCustody", issuer["custody"]),
        number, history(), lambda name, value: name == "RecordOperation" and value["record"] == issuer["custody"]
        and value["number"] == number and value["detail"]["request_digest"] == digest), "landed")
    f.withdraw(f.withdrawal("withdrawal-lost-response", 2_000, role="issuer", number=number), reject=True,
        label="naive rebuild of a landed withdrawal is refused")
    f.refused_with("NumberUsed")
    f.withdraw(f.withdrawal("withdrawal-next", 2_000, role="issuer"),
        label="the custody's next withdrawal lands with a fresh number")
    f.attack("lost response, withdrawal (custody window)", ["naive rebuild of a landed withdrawal is refused"],
        "NumberUsed", ["escrow withdrawal lands while its response is lost", "the custody's next withdrawal lands with a fresh number"])

    # Contribution: the custody's window.
    financier = f.parties["financier"]
    f.transfer_tokens("fund the financier owner's contributions", financier["owner_ata"], 10_000)
    contribution = f.capital_request("contribution-lost-response", 3_000)
    f.execute("capital contribution lands while its response is lost", contribution)
    number = f.numbers[str(financier["custody"])]  # The custody's number the request was just given.
    digest = f.contribution_digest("contribution-lost-response", 3_000)
    f.h.check("SDK rule finds the landed contribution", sdk_outcome(f.window("FinancierCustody", financier["custody"]),
        number, history(), lambda name, value: name == "RecordOperation" and value["record"] == financier["custody"]
        and value["number"] == number and value["detail"]["request_digest"] == digest), "landed")
    f.execute("naive rebuild of a landed contribution is refused",
        f.capital_request("contribution-lost-response", 3_000, number=number), True, "NumberUsed")
    f.refused_with("NumberUsed")
    f.execute("the custody's next contribution lands with a fresh number", f.capital_request("contribution-next", 3_000))
    f.attack("lost response, contribution (custody window)", ["naive rebuild of a landed contribution is refused"],
        "NumberUsed", ["capital contribution lands while its response is lost", "the custody's next contribution lands with a fresh number"])

    # Seller-fee credit: the fee beneficiary's book window.
    fee = f.parties["fee"]
    credit = f.fee_credit(tab, run, "credit-lost-response", 100)
    f.execute("seller-fee credit lands while its response is lost", credit)
    number, digest = credit["args"]["number"], f.credit_digest(credit)
    f.h.check("SDK rule finds the landed seller-fee credit", sdk_outcome(f.state("FinanceBook", fee["book"])["window"],
        number, history(), lambda name, value: name == "FeeCredited" and value["number"] == number
        and value["detail"]["request_digest"] == digest), "landed")
    f.execute("an exact retry of a landed seller-fee credit is refused", credit, True, "TabStale")
    f.refused_with("TabStale")
    f.execute("naive rebuild of a landed seller-fee credit is refused",
        f.fee_credit(tab, run, "credit-lost-response", 100, number=number), True, "NumberUsed")
    f.refused_with("NumberUsed")
    f.execute("the beneficiary's next credit lands with a fresh number", f.fee_credit(tab, run, "credit-next", 100))
    f.attack("lost response, seller-fee credit (beneficiary's book window)",
        ["an exact retry of a landed seller-fee credit is refused", "naive rebuild of a landed seller-fee credit is refused"],
        "TabStale (exact retry), NumberUsed (rebuild)",
        ["seller-fee credit lands while its response is lost", "the beneficiary's next credit lands with a fresh number"])

    # Correction cash leg: the funder's book window (the supplier returns collected cash).
    supplier = f.parties["supplier"]
    correction = f.correction(tab, run, "correction-lost-response", returned=1_000)
    f.h.check("the correction moves cash, so it carries the supplier's number", correction["args"]["number"] > 0, True)
    f.correct("supplier-funded correction cash leg lands while its response is lost", correction)
    number, digest = correction["args"]["number"], f.correction_digest(correction)
    f.h.check("SDK rule finds the landed correction", sdk_outcome(f.state("FinanceBook", supplier["book"])["window"],
        number, history(), lambda name, value: name == "RunCorrected" and value["correction"]["request"]["number"] == number
        and value["detail"]["request_digest"] == digest), "landed")
    f.correct("an exact retry of a landed correction cash leg is refused", correction, replay=True)
    rebuilt = f.correction(tab, run, "correction-lost-response", returned=1_000, number=number)
    f.h.check("the supplier could fund the rebuilt cash leg again", f.h.amount(supplier["owner_ata"]) >= 1_000, True)
    f.correct("naive rebuild of a landed correction cash leg is refused", rebuilt, reject=True, error="NumberUsed",
        token_outcomes=("success",))
    f.correct("the supplier's next correction lands with a fresh number", f.correction(tab, run, "correction-next", returned=1_000))
    f.attack("lost response, correction cash leg (funder's book window)",
        ["an exact retry of a landed correction cash leg is refused", "naive rebuild of a landed correction cash leg is refused"],
        "TabStale (exact retry), NumberUsed (rebuild, after its token leg rolls back)",
        ["supplier-funded correction cash leg lands while its response is lost", "the supplier's next correction lands with a fresh number"])


def third_party_facility(f):
    """Review item 1: one writer per window. A third party repaying the issuer's
    facility numbers only its own book; the issuer's in-flight number keeps its
    standing in the issuer's window and still lands."""
    issuer, third = f.parties["issuer"], f.parties["assignee"]
    in_flight = f.number(issuer["book"])
    issuer_window = f.book_window("issuer")
    f.h.check("the issuer's in-flight number is open in its window", window_state(issuer_window, in_flight), "open")
    first = f.facility_repayment_by("assignee", "third-party-facility-payment-at-issuer-number", 1_000, number=in_flight)
    f.execute("a third party repays the issuer's facility with the issuer's in-flight number", first)
    f.h.check("the third party's own window admitted that number", window_state(f.book_window("assignee"), in_flight), "used")
    f.h.check("the issuer's window is untouched by the third party", f.book_window("issuer"), issuer_window)
    ahead = in_flight + 60_000
    second = f.facility_repayment_by("assignee", "third-party-facility-payment-far-ahead", 1_000, number=ahead)
    f.execute("a third party repays the issuer's facility with a number 60,000 ahead", second)
    f.h.check("the third party's window moved to that number", f.book_window("assignee")["highest"], ahead)
    f.h.check("the issuer's window is still untouched", f.book_window("issuer"), issuer_window)
    f.h.check("the issuer's in-flight number keeps its standing", window_state(f.book_window("issuer"), in_flight), "open")
    own = f.facility_repayment("issuer-in-flight-facility-payment", 1_000, maximum_interest=0, protocol=False,
        number=in_flight)
    f.execute("the issuer's own facility repayment lands with its in-flight number", own)
    f.h.check("the issuer's window admitted its own number", window_state(f.book_window("issuer"), in_flight), "used")
    f.attack("third party repays the issuer's facility (one writer per window)", [], "none: every payment lands",
        ["a third party repays the issuer's facility with the issuer's in-flight number",
            "a third party repays the issuer's facility with a number 60,000 ahead",
            "the issuer's own facility repayment lands with its in-flight number"],
        issuer_number=in_flight, third_party_numbers=[in_flight, ahead])


def numbered_owed_moves(f, tab):
    """Review item 3: a no-cash correction (the company's book) and a surrender
    release (the supplier's book) are numbered; a rebuild of each is refused."""
    company, supplier = f.parties["company"], f.parties["supplier"]
    unpaid = f.earn(tab, "double-charge-unpaid-delivery", 10_000)
    correction = f.correction(tab, unpaid, "no-cash-correction", unpaid=2_000)
    f.h.check("a correction without cash legs is numbered by the company's book",
        (correction["plan"]["required_cash"], correction["args"]["number"] > 0), (0, True))
    f.correct("company, supplier and creditor agree a no-cash unpaid reduction", correction)
    number = correction["args"]["number"]
    f.h.check("the company's window admitted the no-cash correction's number",
        window_state(f.book_window("company"), number), "used")
    f.correct("an exact retry of a landed no-cash correction is refused", correction, replay=True)
    rebuilt = f.correction(tab, unpaid, "no-cash-correction", unpaid=2_000, number=number)
    f.correct("naive rebuild of a landed no-cash correction is refused", rebuilt, reject=True, error="NumberUsed")
    fresh = f.correction(tab, unpaid, "no-cash-correction-next", unpaid=2_000)
    f.h.check("the company's next correction takes a fresh number", fresh["args"]["number"] > number, True)
    f.correct("the company's next no-cash correction lands with a fresh number", fresh)
    f.attack("rebuilt no-cash correction (company's book window)",
        ["an exact retry of a landed no-cash correction is refused", "naive rebuild of a landed no-cash correction is refused"],
        "TabStale (exact retry), NumberUsed (rebuild)",
        ["company, supplier and creditor agree a no-cash unpaid reduction", "the company's next no-cash correction lands with a fresh number"])

    surrender = f.release_request(tab, "surrender-before-expiry", 3_000)
    f.h.check("the surrender happens before the Tab's expiry", f.now <= tab["terms"]["expires_at"], True)
    f.send("supplier surrenders unused capacity before expiry", surrender["instruction"], surrender["signers"],
        receipt=surrender["receipt"])
    number = surrender["args"]["number"]
    f.h.check("the supplier's window admitted the surrender's number", window_state(f.book_window("supplier"), number), "used")
    f.send("an exact retry of a landed surrender is refused", surrender["instruction"], surrender["signers"],
        reject=True, error="TabStale")
    f.refused_with("TabStale")
    rebuilt = f.release_request(tab, "surrender-before-expiry", 3_000, number=number)
    f.send("naive rebuild of a landed surrender is refused", rebuilt["instruction"], rebuilt["signers"],
        reject=True, error="NumberUsed")
    f.refused_with("NumberUsed")
    fresh = f.release_request(tab, "surrender-next", 3_000)
    f.h.check("the supplier's next surrender takes the next number", fresh["args"]["number"], number + 1)
    f.send("the supplier's next surrender lands with a fresh number", fresh["instruction"], fresh["signers"],
        receipt=fresh["receipt"])
    f.attack("rebuilt surrender release before expiry (supplier's book window)",
        ["an exact retry of a landed surrender is refused", "naive rebuild of a landed surrender is refused"],
        "TabStale (exact retry), NumberUsed (rebuild)",
        ["supplier surrenders unused capacity before expiry", "the supplier's next surrender lands with a fresh number"])
    return unpaid


def close_order(f):
    """Review item 3: a repayment record closes only after its run, and the Tab
    only after every run and record."""
    tab = f.acceptance("double-charge-close-order", cap=1_000, maximum_interest=0, index=1, fees=False)
    f.accept(tab)
    run = f.earn(tab, "double-charge-close-order-run", 1_000)
    f.collect(f.collection(tab, run, 1_000), label="supplier collects the close-order Tab's run")
    paid = f.repayment(tab, run, "close-order-payment", 1_000, creditor="issuer", maximum_interest=0)
    f.execute("company repays the close-order Tab in full without closes", paid)
    close_run, close_record, close_tab = f.closes(tab, [run["run"]], [(run["run"], paid["record"])])
    keeper = f.keeper()
    payer = f.state("FundedClaim", tab["claim"])["deposit_payer"]
    def step(label, instruction, *, closes_event=None, gone=()):
        before = f.lamports(payer)
        lamports = sum(f.lamports(address) for address in gone)
        f.as_payer(keeper, lambda: f.send(label, instruction))
        events = [event for case, event, _ in f.h.ledger.events if case == label]
        f.h.check(label + ": close events", events, [closes_event] if closes_event else [])
        for address in gone:
            f.h.check(label + ": closed " + str(address), f.h.svm.get_account(address) is None or f.lamports(address) == 0, True)
        f.h.check(label + ": deposits returned to the recorded payer", f.lamports(payer) - before, lamports)
    step("close_repayment_record before its run is closed is a no-op", close_record)
    f.h.check("the record waits for its run", f.h.svm.get_account(paid["record"]) is not None, True)
    step("close_tab with its run and record open is a no-op", close_tab)
    step("close_run closes the settled run", close_run, closes_event="RunClosed", gone=[run["run"]])
    step("close_tab still waits for the open record", close_tab)
    f.h.check("the Tab counts its open record", f.state("FundedClaim", tab["claim"])["open_records"], 1)
    step("close_repayment_record closes after its run", close_record, closes_event="RepaymentRecordClosed",
        gone=[paid["record"]])
    step("close_tab closes once every run and record is closed", close_tab, closes_event="TabClosed",
        gone=[tab["claim"], tab["reservation"]])
    f.attack("close order: runs, then records, then the Tab", [], "none: early closes are no-ops",
        ["close_repayment_record before its run is closed is a no-op", "close_tab with its run and record open is a no-op",
            "close_run closes the settled run", "close_tab still waits for the open record",
            "close_repayment_record closes after its run", "close_tab closes once every run and record is closed"])


def exact_regrant(f, tab, run):
    """4. Exact-approval re-grant for a governed repayment."""
    first = f.governed_run(tab, run, "regrant-intent", 1_000, "approval-a")
    approval_a, number_a = f.permission_address("company", "approval-a")
    f.grant_permission(first, exact=True)
    f.pay("governed payment lands under exact approval a", first)
    f.h.check("approval a is used", f.permission_state(approval_a)["consumed"]["payments"], 1)
    f.pay("an exact retry under used approval a is refused", first, reject=True, error="TabStale")
    rebuilt = f.governed_run(tab, run, "regrant-intent", 1_000, "approval-a")
    f.pay("a rebuild under used approval a is refused", rebuilt, reject=True, error="RepaymentPermission")
    # The SDK never re-grants to retry: a new grant takes the next number b.
    second = f.governed_run(tab, run, "regrant-intent", 1_000, "approval-b")
    approval_b, number_b = f.permission_address("company", "approval-b")
    f.h.check("a re-grant takes the principal's next number", number_b, number_a + 1)
    f.h.check("the same intent under number b is a different request", second["digest"] != first["digest"], True)
    f.grant_permission(second, exact=True)
    stale = dict(first, accounts={**first["accounts"], "repayment_permission": approval_b})
    stale["instruction"] = f.ix("claims_core", "repay_governed_funded_run", stale["accounts"],
        remaining=f.remaining(first["roles"]), args=first["args"])
    f.pay("approval a's attempt cannot run under re-granted approval b", stale, reject=True, error="RepaymentPermission")
    other = f.governed_run(tab, run, "other-intent-at-used-number", 900, "approval-a")
    f.exact_grant(other, "a grant at existing approval number a is refused", number=number_a, approval=approval_a,
        error="RepaymentPermission")
    skipped = number_b + 2
    f.exact_grant(other, "a grant that skips the principal's next number is refused", number=skipped,
        approval=approval_address(f.parties["company"]["principal"], skipped), error="ApprovalNumber")
    f.pay("approval b authorizes only its own new payment, which lands once", second)
    # Past expiry anyone closes approval a; its rent returns to its payer, and
    # the stale attempt under it is refused.
    f.advance(301)
    state = f.permission_state(approval_a)
    payer = state["deposit_payer"]
    rent = f.lamports(approval_a)
    payer_before = f.lamports(payer)
    f.as_payer(f.keeper(), lambda: f.send("anyone closes used approval a after its expiry",
        f.ix("claims_core", "close_approval", dict(approval=approval_a, deposit_payer=payer))))
    f.h.check("approval a is closed", f.h.svm.get_account(approval_a) is None or f.lamports(approval_a) == 0, True)
    f.h.check("approval a's rent returns to its payer", f.lamports(payer) - payer_before, rent)
    closed = [value for case, event, value in f.h.ledger.events if case == "anyone closes used approval a after its expiry"
        and event == "ApprovalClosed"]
    f.h.check("ApprovalClosed says approval a was used", (closed[0]["number"], closed[0]["used"]), (number_a, True))
    f.pay("a stale attempt after approval a closed is refused", first, reject=True, error="RepaymentPermission")
    f.attack("exact-approval re-grant, governed repayment",
        ["an exact retry under used approval a is refused", "a rebuild under used approval a is refused",
            "approval a's attempt cannot run under re-granted approval b", "a grant at existing approval number a is refused",
            "a grant that skips the principal's next number is refused", "a stale attempt after approval a closed is refused"],
        "TabStale, RepaymentPermission, RepaymentPermission, RepaymentPermission, ApprovalNumber, RepaymentPermission",
        ["governed payment lands under exact approval a", "approval b authorizes only its own new payment, which lands once",
            "anyone closes used approval a after its expiry"],
        approval_a=str(approval_a), number_a=number_a, number_b=number_b, closed_rent=rent)


def standing_replay(f, tab, run):
    """5. A standing grant's window refuses a reused number."""
    first = f.governed_run(tab, run, "standing-payment-one", 1_000, "standing-grant")
    f.grant_permission(first, overrides=dict(total_amount=5_000, total_principal=5_000, maximum_payments=5))
    f.pay("standing grant pays its first number", first)
    f.pay("a replay of the standing grant's landed number is refused", first, reject=True, error="OrderUsed")
    second = f.governed_run(tab, run, "standing-payment-two", 1_000, "standing-grant")
    f.h.check("the grant's next payment takes the next number", second["args"]["number"], first["args"]["number"] + 1)
    f.pay("the standing grant's next number lands", second)
    f.attack("standing-grant governed repayment replay", ["a replay of the standing grant's landed number is refused"],
        "OrderUsed", ["standing grant pays its first number", "the standing grant's next number lands"])


def settled_retry(f):
    """6. A settling repayment with appended closes, retried after it landed."""
    f.grant(f.facility, 1)
    tab = f.acceptance("double-charge-settle", cap=1_000, maximum_interest=0, index=1, fees=False)
    f.accept(tab)
    run = f.earn(tab, "double-charge-settle-run", 1_000)
    f.collect(f.collection(tab, run, 1_000), label="supplier collects the small order")
    settling = f.repayment(tab, run, "settling-payment", 1_000, creditor="issuer", maximum_interest=0)
    closes = f.closes(tab, [run["run"]], [(run["run"], settling["record"])])
    f.settle_and_close("settling repayment lands with its closes appended", tab, [run["run"]],
        [(run["run"], settling["record"])], settling=settling)
    payer = f.h.payer
    deposit_before = f.lamports(payer.pubkey())
    # The same settling transaction, resent: it fails as a whole on the closed records.
    f.as_payer(f.keeper(), lambda: f.send("settling repayment with its closes, retried after landing",
        [settling["instruction"], *closes], [payer, *settling["signers"]], reject=True, error="AccountNotInitialized"))
    row = f.h.cases[-1]
    f.h.check("the retried settlement fails on a closed record",
        any("Error Code: AccountNotInitialized." in line for line in row["result"]["meta"]["logs"]), True)
    f.as_payer(f.keeper(), lambda: f.send("resending only the closes after settlement", closes, [],
        reject=True, error="AccountNotInitialized"))
    f.h.check("no deposit is paid twice", f.lamports(payer.pubkey()), deposit_before)
    for gone in (tab["claim"], tab["reservation"], run["run"], settling["record"]):
        f.h.check("settled records stay closed", f.h.svm.get_account(gone) is None or f.lamports(gone) == 0, True)
    f.attack("settling repayment with appended closes retried after landing",
        ["settling repayment with its closes, retried after landing", "resending only the closes after settlement"],
        "AccountNotInitialized (both)", ["settling repayment lands with its closes appended"])


def interest_refund_rebuild(f, tab, run):
    """2 (continued). Interest refund: the lender's book window."""
    issuer = f.parties["issuer"]
    f.advance(YEAR)
    paid = f.repayment(tab, run, "interest-bearing-payment", 5_000, creditor="issuer", maximum_interest=10_000)
    f.execute("company pays accrued interest to the issuer", paid)
    collected = f.view(paid["receipt"])["interest_paid"]
    f.h.check("the payment collected interest to refund", collected > 0, True)
    refund = f.interest_refund(tab, run, paid, "refund-lost-response", 100, 0)
    f.execute("interest refund lands while its response is lost", refund)
    number, digest = refund["args"]["number"], f.refund_digest(refund)
    f.h.check("SDK rule finds the landed interest refund", sdk_outcome(f.state("FinanceBook", issuer["book"])["window"],
        number, f.history(), lambda name, value: name == "InterestRefunded" and value["refund"]["number"] == number
        and value["detail"]["request_digest"] == digest), "landed")
    f.refund_interest("an exact retry of a landed interest refund is refused", refund, replay=True)
    rebuilt = f.interest_refund(tab, run, paid, "refund-lost-response", 100, 0, number=number)
    f.h.check("the lender could fund the rebuilt refund again", f.h.amount(issuer["owner_ata"]) >= 100, True)
    f.refund_interest("naive rebuild of a landed interest refund is refused", rebuilt, reject=True, error="NumberUsed",
        token_outcomes=("success",))
    f.refund_interest("the lender's next refund lands with a fresh number",
        f.interest_refund(tab, run, paid, "refund-next", 100, 0))
    f.attack("lost response, interest refund (lender's book window)",
        ["an exact retry of a landed interest refund is refused", "naive rebuild of a landed interest refund is refused"],
        "TabStale (exact retry), NumberUsed (rebuild, after its token leg rolls back)",
        ["interest refund lands while its response is lost", "the lender's next refund lands with a fresh number"])


def exercise(f):
    f.exact_permissions = {"approval-a", "approval-b"}
    f.setup()
    f.repayment_wallet("company", 100_000)
    tab = f.acceptance("double-charge-ledger")
    f.accept(tab)
    run = f.earn(tab, "double-charge-delivery", 80_000)
    f.collect(f.collection(tab, run, 80_000), label="supplier collects the delivered 80,000")
    for party in f.parties.values():
        f.observe(*(party[k] for k in ("book", "owner_ata", "custody", "custody_token") if k in party))
    f.observe(tab["claim"], tab["reservation"], run["run"], f.facility, f.line)
    lost_response_repayment(f, tab, run)
    sibling_attempts(f, tab, run)
    rebuilt_moves(f, tab, run)
    third_party_facility(f)
    numbered_owed_moves(f, tab)  # Before the Tab's expiry: a surrender is a pre-expiry release.
    exact_regrant(f, tab, run)
    standing_replay(f, tab, run)
    settled_retry(f)  # Before the year of interest below: its agent and order expire.
    close_order(f)
    interest_refund_rebuild(f, tab, run)


def main():
    f = DoubleChargeFixture()
    try:
        exercise(f)
    except Exception as error:
        f.finish("double-charge", error=error)
        raise
    summary = f.finish("double-charge")
    path = OUT / "double-charge-scenarios.json"
    path.write_text(json.dumps(dict(schema="dexter-double-charge-attacks/v1", status="passed",
        transport="native-v1 LiteSVM", public_transactions_sent=0, trace=summary["trace"],
        trace_sha256=summary["trace_sha256"], attacks=f.attacks,
        boundary="Compiled-program local execution with synthetic tokens; no deployment or public RPC"),
        indent=2, default=json_value) + "\n")
    print(json.dumps(dict(scenarios=str(path), sha256=digest(path), attacks=len(f.attacks))))


if __name__ == "__main__":
    main()
