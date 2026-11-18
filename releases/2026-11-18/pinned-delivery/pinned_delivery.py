#!/usr/bin/env python3
"""The owner pins the size and who signs for delivery; payment follows that signer.

An evidence order names one delivery signer: its billing policy's `source`.
Only that key's signed report says what was delivered. The order's terms are
the list of pinned_terms.py, one (name, value) pair of fingerprints per field,
with one reserved name:

    delivery_signer  name  = sha256("DEXTER:ORDER-TERM-NAME:1" || "delivery_signer")
                     value = the 32 bytes of the key the order names to sign for delivery

On an evidence order the list hashes to the policy's `request_hash`. The policy
hash commits to that value and to `source`, and the terms the agent and the
seller sign commit to the policy hash. ClaimsCore
`accept_pinned_evidence_financed_tab` and `accept_pinned_recoverable_financed_tab`
are the plain evidence and recoverable instructions plus the shown list. The
Registry (`reserve_pinned_billing`) checks that the names ascend, that the list
hashes to `request_hash` of a policy the signed terms commit to, that every
pinned term is in the list, and that a listed delivery_signer is the policy's
`source`. An order that names no delivery signer (`accept_pinned_financed_tab`)
cannot list one.

On the recoverable variant the policy's `source` is the source's Registry
principal, so the delivery signer is that principal: whoever is its owner when
the report is signed, through owner rotation and recovery.

Main run: one buyer, two agents, one seller, a carrier, an inspector, one
credit issuer and one lender. Evidence orders have a maximum of 2,000 at a
fixed price of 1 per unit, as in named_signer.py. Orders through
`accept_pinned_financed_tab` have a maximum of 100,000. Further runs: a
recoverable order on recovery.py's fixture, and the same orders approved by
passkey owners on passkey_authority.py's fixture.

Refusals are checked as in pinned_terms.py: transaction error, first failing
program frame, Anchor error name and number, the source location in the
failing frame's log line (table `RAISED`), byte-identical state for every
writable account except the fee payer, and no budget hold, lender promise,
order record, Registry reservation, operation record, receipt or bill.

Run in a prepared checkout: ./dev anchor-smoke pinned-delivery
Record: <python> foundation/onchain/tests/pinned_delivery.py --record docs/evidence/<directory>
"""
from pinned_terms import *

CAP = 2_000
SIGNED = 900
CLAIMED = 1_400
DELIVERY_SIGNER = name_print("delivery_signer")
REGISTRY_ERRORS = dict(TermsRequired=6051, TermsMismatch=6052, TermsNotPermitted=6053, DeliverySignerMismatch=6054)
CORE_ERRORS = dict(BillingSignature=6063)
REGISTRY_SOURCE = "thrown in programs/principal-registry/src/"
CORE_SOURCE = "thrown in programs/claims-core/src/"
# Written here from the source at this commit and asserted for every refusal.
RAISED = {
    "no list shown": REGISTRY_SOURCE + "lib.rs:357",
    "list names": REGISTRY_SOURCE + "lib.rs:360",
    "list hash": REGISTRY_SOURCE + "lib.rs:361",
    "pinned term": REGISTRY_SOURCE + "lib.rs:363",
    "delivery signer": REGISTRY_SOURCE + "lib.rs:366",
    "report signature": CORE_SOURCE + "evidence_billing.rs:231",
}


def refusal(name, where, *, index=0):
    if name in REGISTRY_ERRORS:
        return dict(program=REGISTRY, name=name, code=REGISTRY_ERRORS[name], index=index, source=RAISED[where])
    return dict(program=CORE, name=name, code=CORE_ERRORS[name], index=index, source=RAISED[where])


def signer_term(signer):
    """The reserved term naming `signer` as the key that signs for delivery."""
    return dict(name=DELIVERY_SIGNER, value=bytes(signer))


def delivery_list(pairs, signer=None, *extra):
    """The seller's terms, the delivery signer's term and any further prepared terms, ascending by name."""
    terms = [term(name, value) for name, value in pairs] + ([] if signer is None else [signer_term(signer)]) + list(extra)
    return sorted(terms, key=lambda item: item["name"])


class ShownList:
    """An evidence order's policy carries `request_hash` = the hash of the list it commits to."""
    committed = None

    def policy(self, name, **options):
        policy = super().policy(name, **options)
        if self.committed is not None:
            policy["request_hash"] = order_terms_hash(self.committed)
        return policy

    def evidence_order(self, build, listed, signed):
        self.committed = listed if signed is None else signed
        try:
            tab = build()
        finally:
            self.committed = None
        tab["order_terms"] = listed
        self.h.check(f"{tab['name']}: the policy's request_hash is the hash of the list its terms commit to",
            tab["billing_policy"]["request_hash"], order_terms_hash(tab["order_terms"] if signed is None else signed))
        return tab

    def check_accepted_bill(self, tab, number, signer):
        bill = self.bill_state(tab)
        self.h.check(f"order {number}: the bill records the delivery signer the order names", bill["policy"]["source"], signer)
        self.h.check(f"order {number}: the bill's policy carries the hash of the list shown",
            bill["policy"]["request_hash"], order_terms_hash(tab["order_terms"]))
        claim = self.state("FundedClaim", tab["claim"])
        self.h.check(f"order {number}: the order record commits to the policy hash",
            claim["external_contract_hash"], tab["policy_hash"])
        reservation = self.registry.read(self.h.svm, tab["reservation"], "Reservation")
        self.h.check(f"order {number}: the Registry reservation commits to the same terms",
            reservation["terms"]["external_terms_hash"], tab["terms"]["external_terms_hash"])
        return bill


class PinnedDelivery(ShownList, PinnedTerms):
    agent_index = 0

    def acceptance(self, name, **options):
        options.setdefault("index", self.agent_index)
        return super().acceptance(name, **options)

    def delivery_order(self, name, pairs, signer, *, revision, index=0, listed_signer=None, signed=None, listed=None):
        """An evidence order of 2,000 naming `signer` (the policy's source) to sign for delivery.

        `listed_signer` puts another key in the list's delivery_signer term;
        `signed` is a list the policy commits to in place of the list shown;
        `listed` is a prepared list shown in place of the seller's terms.
        """
        if listed is None:
            listed = delivery_list(pairs, signer if listed_signer is None else listed_signer)
        self.agent_index = index
        try:
            tab = self.evidence_order(lambda: self.evidence_tab(name, cap=CAP, rates=((1, 1, CAP),),
                policy_overrides=dict(source=signer)), listed, signed)
        finally:
            self.agent_index = 0
        tab["terms"]["mandate_revision"] = revision
        return tab

    def place_delivery(self, tab, label, *, case, what_changed="nothing", shown=True, refuse=None):
        """`shown=True` sends accept_pinned_evidence_financed_tab with the list; `shown=False` the plain evidence instruction."""
        name = "accept_pinned_evidence_financed_tab" if shown else "accept_evidence_financed_tab"
        extra = dict(order_terms=tab["order_terms"]) if shown else {}
        instruction = self.ix("claims_core", name, tab["evidence_accounts"], remaining=self.roles_of(tab),
            **tab["evidence_args"], **extra)
        before = self.money()
        self.send(label, instruction, tab["signers"], reject=refuse is not None, receipt=tab["receipt"])
        row = self.mark(case, "evidence order, list shown" if shown else "evidence order, no list shown",
            what_changed, refuse=refuse)
        if refuse is not None:
            self.nothing_moved(tab, label, before)
            self.h.check("no bill exists: " + label, absent(self.h.svm, tab["bill"]), True)
        return row

    def ran(self, row, number, core, registry):
        logs = row["result"]["logs"]
        self.h.check(f"order {number}: Core ran {core}", "Program log: Instruction: " + core in logs, True)
        self.h.check(f"order {number}: the Registry ran {registry} inside the commit",
            "Program log: Instruction: " + registry in logs, True)


def interface(f):
    names = {error["name"]: error["code"] for error in f.registry.value["errors"]}
    f.h.check("error numbers written in this module equal the built Registry interface",
        {name: names.get(name) for name in REGISTRY_ERRORS}, REGISTRY_ERRORS)
    names = {error["name"]: error["code"] for error in f.core.value["errors"]}
    f.h.check("error numbers written in this module equal the built ClaimsCore interface",
        {name: names.get(name) for name in CORE_ERRORS}, CORE_ERRORS)
    for pinned, plain in (("accept_pinned_evidence_financed_tab", "accept_evidence_financed_tab"),
            ("accept_pinned_recoverable_financed_tab", "accept_recoverable_financed_tab")):
        f.h.check(f"{pinned} takes the arguments of {plain} plus the list",
            [a["name"] for a in f.core.instructions[pinned]["args"]],
            [a["name"] for a in f.core.instructions[plain]["args"]] + ["order_terms"])
        f.h.check(f"{pinned} takes the accounts of {plain}",
            f.core.instructions[pinned]["accounts"], f.core.instructions[plain]["accounts"])
    f.h.check("the Registry's billing reservation takes the plain reservation's accounts",
        f.registry.instructions["reserve_pinned_billing"]["accounts"], f.registry.instructions["reserve"]["accounts"])
    f.h.check("the Registry's billing reservation takes the order, the policy's bill and claim, the policy and the list",
        [a["name"] for a in f.registry.instructions["reserve_pinned_billing"]["args"]],
        ["args", "bill", "claim", "policy", "order_terms"])
    shared = json.loads(SHARED_VECTORS.read_text())["delivery_signer"]
    f.h.check("the reserved name's fingerprint equals the shared vectors",
        (shared["name"], shared["name_fingerprint"]), ("delivery_signer", DELIVERY_SIGNER.hex()))
    for vector in shared["vectors"]:
        terms = delivery_list(tuple(tuple(entry) for entry in vector["entries"]), Pubkey.from_string(vector["signer"]))
        f.h.check("shared delivery vector, list of terms: " + vector["name"],
            [dict(name=item["name"].hex(), value=item["value"].hex()) for item in terms], vector["terms"])
        f.h.check("shared delivery vector, order terms hash: " + vector["name"], order_terms_hash(terms).hex(), vector["hash"])


def cast(f):
    parties = f.parties
    f.roles = {"buyer's owner": parties["company"]["owner"], "agent 1": f.agent, "agent 2": f.agent2,
        "seller": parties["supplier"]["owner"], "carrier": f.meter, "inspector": f.other_meter,
        "lender's owner": parties["financier"]["owner"], "credit issuer's owner": parties["issuer"]["owner"],
        "fee payer of setup and order transactions": f.h.payer, "key that submits signed records": f.submitter,
        "key that finalizes": f.finalizer, "key that collects": f.collector}
    keys = [str(holder.pubkey()) for holder in f.roles.values()]
    f.h.check("all twelve roles are held by different keys", len(set(keys)), len(keys))


def setup(f):
    f.setup_principals()
    for role, amount in (("protocol", 0), ("fee", 0), ("issuer", 200_000)):
        f.create_custody(role, amount)
    f.create_custody("financier", LENDER_CASH, wallet=True)
    f.open_fee_agreement("main", 2_100)
    f.facility = f.open_facility()
    f.line, f.assignment_policy = f.open_line(f.facility)


def pinned_carrier(f):
    """Case 1: size = 10 and delivery_signer = the carrier, pinned when the permission is created."""
    carrier, seller, inspector = f.meter.pubkey(), f.parties["supplier"]["owner"].pubkey(), f.other_meter.pubkey()
    pin = sorted([term("size", "10"), signer_term(carrier)], key=lambda item: item["name"])
    f.permission = f.create_pinned("agent-1", 0, pin,
        "ACCEPTED: the buyer's owner creates agent 1's permission already pinned to size = 10 and delivery_signer = the carrier",
        case="1")
    f.h.check("agent 1's permission holds both terms at revision 1", f.pinned(f.permission), dict(revision=1, pinned=pin, active=True))
    f.h.check("the pinned delivery_signer value is the carrier's key", signer_term(carrier) in f.pinned(f.permission)["pinned"], True)
    f.second = f.permit("agent-2", 1)
    f.mandates[0], f.mandates[1] = f.permission, f.second
    not_permitted = refusal("TermsNotPermitted", "pinned term")

    def order_1(**options):
        return f.delivery_order("order-1", options.pop("pairs", SELLER_TERMS), options.pop("signer", carrier), revision=1, **options)
    f.place_delivery(order_1(pairs=changed(SELLER_TERMS, size="11")),
        "refused: agent 1 and the seller submit evidence order 1 for size = 11 naming the carrier; the owner pinned size = 10",
        case="1", what_changed="size in the seller's terms: 11 instead of 10", refuse=not_permitted)
    for key_, who in ((seller, "the seller's own key"), (inspector, "an inspector's key")):
        f.place_delivery(order_1(signer=key_),
            f"refused: evidence order 1 for size = 10 naming {who} to sign for delivery; the owner pinned the carrier",
            case="1", what_changed=f"delivery signer the order names: {who} instead of the carrier", refuse=not_permitted)
    f.place_delivery(order_1(signed=delivery_list(changed(SELLER_TERMS, size="11"), carrier)),
        "refused: evidence order 1 naming the carrier shows size = 10; the terms its policy commits to say size = 11",
        case="1", what_changed="list shown is not the list the policy's request_hash commits to",
        refuse=refusal("TermsMismatch", "list hash"))
    f.place_delivery(order_1(), "refused: evidence order 1 for size = 10 naming the carrier, through plain accept_evidence_financed_tab, "
        "which shows no list", case="6", what_changed="plain evidence instruction, no list shown", shown=False,
        refuse=refusal("TermsRequired", "no list shown"))
    twice = delivery_list(SELLER_TERMS, carrier, signer_term(seller))
    f.h.check("the list naming delivery_signer twice holds the carrier, whom the owner pinned, and the seller",
        (signer_term(carrier) in twice, signer_term(seller) in twice, len(twice)), (True, True, 6))
    f.place_delivery(order_1(listed=twice),
        "refused: evidence order 1 whose list names delivery_signer twice, as the carrier and as the seller; a list names a field once",
        case="7", what_changed="delivery_signer named twice in the list", refuse=refusal("TermsMismatch", "list names"))
    f.place_delivery(order_1(signer=seller, listed_signer=carrier),
        "refused: evidence order 1 lists delivery_signer = the carrier, but its policy names the seller to sign for delivery",
        case="7", what_changed="the policy's source: the seller, while the list says the carrier",
        refuse=refusal("DeliverySignerMismatch", "delivery signer"))
    f.place_delivery(order_1(listed=delivery_list(SELLER_TERMS)),
        "refused: evidence order 1 for size = 10 naming the carrier, whose list leaves out the delivery_signer term the owner pinned",
        case="7", what_changed="the list does not carry the delivery_signer term", refuse=not_permitted)

    o1 = order_1()
    row = f.place_delivery(o1, "ACCEPTED: agent 1 and the seller submit evidence order 1, maximum 2,000, for size = 10 naming the carrier",
        case="1")
    f.ran(row, 1, "AcceptPinnedEvidenceFinancedTab", "ReservePinnedBilling")
    f.check_accepted_bill(o1, 1, carrier)
    f.h.check("order 1: the list shown holds both pinned terms", all(t in o1["order_terms"] for t in pin), True)
    f.h.check("order 1: the carrier neither signs nor appears as an account when the order is accepted",
        str(carrier) in row["account_keys"], False)
    f.h.check("order 1: the buyer's owner does not sign the order", str(f.parties["company"]["owner"].pubkey()) in f.signers_of(row), False)
    f.h.check("budget hold after order 1", f.money()["budget_held"], CAP)
    row = f.place_delivery(o1, "no effect: accepted evidence order 1 is sent again; the program returns the original outcome",
        case="1", what_changed="an already accepted order is sent again")
    f.h.check("re-sending accepted order 1 changes no observed account", row["observed_after"], row["observed_before"])
    return o1


def unpinned_agent(f):
    """Case 4 and the reserved name on an order that names no delivery signer."""
    carrier = f.meter.pubkey()
    f.h.check("agent 2's permission pins nothing", f.pinned(f.second), dict(revision=1, pinned=[], active=True))
    f.agent_index = 1
    try:
        o2 = f.evidence_tab("order-2", cap=CAP, rates=((1, 1, CAP),))
    finally:
        f.agent_index = 0
    f.h.check("order 2: its policy names the carrier and keeps the evidence fixture's request_hash",
        (o2["billing_policy"]["source"], o2["billing_policy"]["request_hash"]), (carrier, ident("request-order-2")))
    row = f.place_delivery(o2, "ACCEPTED: agent 2, whose permission pins nothing, submits evidence order 2 of 2,000 naming the carrier "
        "through plain accept_evidence_financed_tab", case="4", shown=False)
    f.ran(row, 2, "AcceptEvidenceFinancedTab", "Reserve")
    f.h.check("order 2: the Registry did not run a pinned reservation",
        any("Instruction: ReservePinned" in line for line in row["result"]["logs"]), False)
    bill = f.bill_state(o2)
    f.h.check("order 2: the bill records the carrier", bill["policy"]["source"], carrier)
    f.h.check("order 2: the order record commits to the policy hash",
        f.state("FundedClaim", o2["claim"])["external_contract_hash"], o2["policy_hash"])
    o3 = f.delivery_order("order-3", SELLER_TERMS, carrier, revision=1, index=1)
    row = f.place_delivery(o3, "ACCEPTED: agent 2 submits evidence order 3 of 2,000 for size = 10 naming the carrier "
        "through the pinned evidence instruction; its permission pins nothing", case="7")
    f.ran(row, 3, "AcceptPinnedEvidenceFinancedTab", "ReservePinnedBilling")
    f.check_accepted_bill(o3, 3, carrier)

    # An order through accept_pinned_financed_tab names no delivery signer, so its list cannot carry one.
    with_signer = delivery_list(SELLER_TERMS, carrier)
    o4 = f.terms_order("order-4", with_signer, revision=1, index=1, permission=f.second)
    f.place(o4, "refused: agent 2 submits order 4 (accept_pinned_financed_tab, no delivery signer) whose list says "
        "delivery_signer = the carrier", case="7", what_changed="the list carries a delivery_signer term",
        refuse=refusal("DeliverySignerMismatch", "delivery signer"))
    o4 = f.terms_order("order-4", SELLER_TERMS, revision=1, index=1, permission=f.second)
    row = f.place(o4, "ACCEPTED: order 4 with the same seller's terms and no delivery_signer term", case="7")
    f.accepted(o4, row, 4, shown=True, held=3 * CAP + ORDER)


def bypass(f):
    """Agent 1's pinned carrier cannot be met by an order that names no delivery signer."""
    carrier = f.meter.pubkey()
    o5 = f.terms_order("order-5", delivery_list(SELLER_TERMS, carrier), revision=1)
    f.place(o5, "refused: agent 1 sends order 5 for size = 10 through accept_pinned_financed_tab, which names no delivery signer, "
        "with a list that says delivery_signer = the carrier", case="7",
        what_changed="instruction: an order that names no delivery signer, carrying the pinned carrier term",
        refuse=refusal("DeliverySignerMismatch", "delivery signer"))
    o5 = f.terms_order("order-5", SELLER_TERMS, revision=1)
    f.place(o5, "refused: order 5 through accept_pinned_financed_tab without the delivery_signer term; the owner pinned the carrier",
        case="7", what_changed="the list does not carry the pinned delivery_signer term",
        refuse=refusal("TermsNotPermitted", "pinned term"))


def buyer_signs(f):
    """Case 3: the owner pins delivery_signer = the buyer's own key."""
    buyer = f.parties["company"]["owner"].pubkey()
    pin = sorted([term("size", "10"), signer_term(buyer)], key=lambda item: item["name"])
    f.pin(f.permission, pin, "ACCEPTED: the buyer's owner changes agent 1's pin to size = 10 and delivery_signer = the buyer's own key",
        case="3")
    f.h.check("agent 1's permission now pins the buyer's key at revision 2", f.pinned(f.permission), dict(revision=2, pinned=pin, active=True))
    f.place_delivery(f.delivery_order("order-6", SELLER_TERMS, f.meter.pubkey(), revision=2),
        "refused: evidence order 6 for size = 10 naming the carrier; the owner pinned the buyer's own key",
        case="3", what_changed="delivery signer the order names: the carrier instead of the buyer",
        refuse=refusal("TermsNotPermitted", "pinned term"))
    o6 = f.delivery_order("order-6", SELLER_TERMS, buyer, revision=2)
    row = f.place_delivery(o6, "ACCEPTED: evidence order 6, maximum 2,000, for size = 10 naming the buyer's own key", case="3")
    f.ran(row, 6, "AcceptPinnedEvidenceFinancedTab", "ReservePinnedBilling")
    f.check_accepted_bill(o6, 6, buyer)
    return o6


def carrier_signs(f, o1):
    """Case 2: the carrier signs for 900 against the seller's claim of 1,400; the bill is 900 and is paid."""
    seller, lender = f.parties["supplier"], f.parties["financier"]
    report = f.report(o1, units=(SIGNED,))
    f.h.check("the carrier's report carries the order's terms hash as its request_hash",
        report["request_hash"], order_terms_hash(o1["order_terms"]))
    f.submit_report(o1, report, source=seller["owner"], error="BillingSignature",
        label="refused: the seller signs the report of 900 delivered on order 1; the order names the carrier")
    f.mark("2", "report", "report signed by the seller instead of the carrier", refuse=refusal("BillingSignature", "report signature", index=1))
    f.submit_report(o1, report, label="ACCEPTED: the carrier signs 900 units delivered on order 1")
    f.mark("2", "report", "nothing")
    f.submit_claim(o1, f.monetary_claim(o1, CLAIMED), label="ACCEPTED: the seller signs a claim of 1,400 on order 1")
    f.mark("2", "claim", "nothing")
    claim = f.state("FundedClaim", o1["claim"])
    f.h.check("signed records alone create no debt and nothing payable", (claim["debt"], claim["payable"]), (0, 0))
    f.at(46)
    f.finalize(o1, label="ACCEPTED: an outside key finalizes order 1 at 46 seconds: the carrier signed 900, the seller claimed 1,400")
    f.mark("2", "finalization", "nothing")
    f.h.check("bill expected from the smallest of maximum 2,000, claim 1,400 and signed 900", min(CAP, CLAIMED, SIGNED), SIGNED)
    f.assert_final_amount(o1, SIGNED)
    seller_ata = f.h.ata(seller["owner"].pubkey(), f.mint)
    seller_before, wallet_before = f.h.amount(seller_ata), f.h.amount(lender["custody_token"])
    f.collect_from(o1, SIGNED, payer=f.collector,
        label="ACCEPTED: an outside key collects order 1's bill of 900 from the lender wallet to the seller")
    row = f.mark("2", "collection", "nothing")
    f.h.check("the collection's only signer is the outside key", f.signers_of(row), [str(f.collector.pubkey())])
    f.h.check("the seller receives exactly 900", f.h.amount(seller_ata) - seller_before, SIGNED)
    f.h.check("the lender wallet pays exactly 900", wallet_before - f.h.amount(lender["custody_token"]), SIGNED)
    claim = f.state("FundedClaim", o1["claim"])
    for field, value in (("paid", SIGNED), ("payable", 0), ("debt", SIGNED), ("released", CAP - SIGNED), ("remaining", 0)):
        f.h.check("order 1 record " + field + " after collection", claim[field], value)


def end_state(f):
    budget = f.registry.read(f.h.svm, f.budget, "Budget")
    f.h.check("budget at the end: orders 2, 3 and 6 of 2,000 and order 4 of 100,000 held; order 1 consumed 900",
        (budget["reserved"], budget["consumed"]), (3 * CAP + ORDER, SIGNED))
    refused = [row for row in f.h.cases if row["expected"] == "reject"]
    f.h.check("every refusal in this run was annotated and checked",
        all(row.get("commit_legs", {}).get("outcome") == "refused" for row in refused), True)


def exercise(f):
    setup(f)
    interface(f)
    cast(f)
    o1 = pinned_carrier(f)
    unpinned_agent(f)
    bypass(f)
    buyer_signs(f)
    carrier_signs(f, o1)
    end_state(f)


class RecoverableDelivery(ShownList, SourceChecked, CommitLegs, RecoveryFixture):
    """Case 5: the recoverable variant, whose delivery signer is the source's Registry principal."""

    def mark(self, case, kind, what_changed, *, refuse=None):
        return self.note(section=case, leg=kind, changed=what_changed, refuse=refuse)

    def recoverable_order(self, name, pairs, listed_signer, *, mandate, revision):
        source = self.parties["assignee"]["principal"]
        tab = self.evidence_order(lambda: self.recoverable_tab(name, mandate=mandate, cap=CAP),
            delivery_list(pairs, listed_signer), None)
        tab["terms"]["mandate_revision"] = revision
        self.h.check(f"{name}: the policy names the source principal", tab["billing_policy"]["source"], source)
        return tab

    def place_recoverable(self, tab, label, *, case, what_changed="nothing", shown=True, refuse=None):
        """accept_recoverable_financed_tab's exact generated account order, plus the list when shown."""
        name = "accept_pinned_recoverable_financed_tab" if shown else "accept_recoverable_financed_tab"
        definition = self.core.instructions[name]
        values = {**self.common(), **tab["evidence_accounts"]}
        metas = []

        def append(fields):
            for field in fields:
                if "accounts" in field:
                    append(field["accounts"])
                    continue
                public = values[field["name"]]
                metas.append(M(CORE) if public is None else M(public, field.get("signer", False), field.get("writable", False)))
        append(definition["accounts"])
        args = dict(tab["evidence_args"], **(dict(order_terms=tab["order_terms"]) if shown else {}))
        self.h.check(f"{name} arguments", sorted(args), sorted(a["name"] for a in definition["args"]))
        data = bytes(definition["discriminator"]) + b"".join(self.core.encode(a["type"], args[a["name"]]) for a in definition["args"])
        instruction = Instruction(CORE, data, [*metas, *self.remaining(["company", "issuer", "supplier", tab["financier"]])])
        budget = self.registry.read(self.h.svm, self.budget, "Budget")
        self.send(label, instruction, tab["signers"], reject=refuse is not None, receipt=tab["receipt"])
        row = self.mark(case, "recoverable order, list shown" if shown else "recoverable order, no list shown", what_changed, refuse=refuse)
        if refuse is not None:
            self.h.check("the refused recoverable order left the budget as it was: " + label,
                self.registry.read(self.h.svm, self.budget, "Budget"), budget)
            for what, address in (("order record", tab["claim"]), ("Registry reservation", tab["reservation"]),
                    ("bill", tab["bill"]), ("authority binding", tab["authority_binding"])):
                self.h.check(f"no {what} exists: " + label, absent(self.h.svm, address), True)
        return row


def recoverable(f):
    f.setup(financier_wallet=True)
    buyer = f.parties["company"]
    f.receiving_open("supplier")
    permission = f.controlled_mandate("pinned-delivery")
    source = f.parties["assignee"]["principal"]
    source_owner = f.parties["assignee"]["owner"].pubkey()
    pin = sorted([term("size", "10"), signer_term(source)], key=lambda item: item["name"])
    f.send("ACCEPTED: the buyer's owner pins size = 10 and delivery_signer = the source's principal on the agent's permission",
        pin_instruction(f.registry, buyer["owner"].pubkey(), f.company, permission, pin, 1), [buyer["owner"]])
    f.mark("5", "pin", "nothing")
    f.h.check("the permission stores both terms at revision 2", stored_pin(f.registry, f.h.svm, permission),
        dict(revision=2, pinned=pin, active=True))
    f.h.check("the source principal is a Registry address, not a signing key", source.is_on_curve(), False)

    def order(**options):
        return f.recoverable_order("order-1-recoverable", options.pop("pairs", SELLER_TERMS), options.pop("signer", source),
            mandate=permission, revision=2)
    f.place_recoverable(order(), "refused: recoverable order 1 for size = 10 through plain accept_recoverable_financed_tab, "
        "which shows no list", case="5", what_changed="plain recoverable instruction, no list shown", shown=False,
        refuse=refusal("TermsRequired", "no list shown"))
    f.place_recoverable(order(pairs=changed(SELLER_TERMS, size="11")),
        "refused: recoverable order 1 for size = 11; the owner pinned size = 10", case="5",
        what_changed="size in the seller's terms: 11 instead of 10", refuse=refusal("TermsNotPermitted", "pinned term"))
    f.place_recoverable(order(signer=source_owner),
        "refused: recoverable order 1 whose delivery_signer term is the source principal's current owner key, not the principal",
        case="5", what_changed="delivery_signer value: the owner's key instead of the source principal",
        refuse=refusal("TermsNotPermitted", "pinned term"))
    tab = order()
    row = f.place_recoverable(tab, "ACCEPTED: recoverable order 1, maximum 2,000, for size = 10 naming the source principal", case="5")
    logs = row["result"]["logs"]
    for program, name in (("Core", "AcceptPinnedRecoverableFinancedTab"), ("Registry", "ReservePinnedBilling")):
        f.h.check(f"recoverable order 1: {program} ran {name}", "Program log: Instruction: " + name in logs, True)
    f.check_accepted_bill(tab, 1, source)
    f.h.check("budget hold after the accepted recoverable order", f.registry.read(f.h.svm, f.budget, "Budget")["reserved"], CAP)
    f.roles = {"buyer's owner": buyer["owner"], "agent": f.agent, "seller's owner": f.parties["supplier"]["owner"],
        "source principal's owner": f.parties["assignee"]["owner"]}


class PasskeyDelivery(ShownList, SourceChecked, CommitLegs, PasskeyFixture):
    """Pinned evidence and recoverable orders when the buyer's and the credit issuer's owners are passkeys."""

    def mark(self, case, kind, what_changed, *, refuse=None):
        return self.note(section=case, leg=kind, changed=what_changed, refuse=refuse)

    place_recoverable = RecoverableDelivery.place_recoverable
    recoverable_order = RecoverableDelivery.recoverable_order


def passkey_owners(f):
    f.deployment_setup()
    f.first_passkey_principal()
    f.setup(financier_wallet=True)
    for role in ("company", "issuer"):
        f.adopt(role, f.initialize_passkey(role, role), role)
    buyer, issuer, seller = [f.parties[x] for x in ("company", "issuer", "supplier")]
    f.h.check("the buyer's owner and the credit issuer's owner are passkey aliases, not signing keys",
        [x["owner"].pubkey().is_on_curve() for x in (buyer, issuer)], [False, False])
    carrier = f.meter.pubkey()
    f.actors[0] = f.epoch_actor("pinned-delivery")
    f.receiving_open("supplier")

    def create(name, supplier, pin):
        mandate_id = ident(name)
        permission = pda(REGISTRY, b"mandate", f.company, mandate_id, u64(buyer["generation"]))
        f.observe(permission)
        f.send(f"ACCEPTED: the buyer's owner, a passkey, creates the agent's permission already pinned ({name})",
            f.ix("principal_registry", "create_pinned_mandate",
                dict(owner=buyer["owner"].pubkey(), principal=f.company, actor=f.actors[0], budget=f.budget, mandate=permission),
                args=dict(mandate_id=mandate_id, supplier=supplier, source=f.facility, per_operation_limit=200_000,
                    expires_at=NOW + 600), required_terms=pin), [buyer["owner"]])
        row = f.mark("passkey", "create pinned", "nothing")
        f.h.check("the creation was approved by one passkey proof", len(row["passkey_authorizations"]), 1)
        return permission
    pin = sorted([term("size", "10"), signer_term(carrier)], key=lambda item: item["name"])
    f.mandates[0] = create("pinned-delivery-evidence", seller["owner"].pubkey(), pin)

    def order(pairs):
        tab = f.evidence_order(lambda: f.evidence_tab("order-1-passkey", cap=CAP, rates=((1, 1, CAP),)),
            delivery_list(pairs, carrier), None)
        tab["terms"]["mandate_revision"] = 1
        return tab, f.ix("claims_core", "accept_pinned_evidence_financed_tab", tab["evidence_accounts"],
            remaining=f.remaining(["company", "issuer", "supplier", tab["financier"]]), order_terms=tab["order_terms"],
            **tab["evidence_args"])
    tab, instruction = order(changed(SELLER_TERMS, size="11"))
    f.send("refused: evidence order 1 for size = 11 naming the carrier, signed by the agent, the seller and the credit issuer's passkey",
        instruction, tab["signers"], reject=True, receipt=tab["receipt"])
    f.mark("passkey", "evidence order, list shown", "size in the seller's terms: 11 instead of 10",
        refuse=refusal("TermsNotPermitted", "pinned term", index=2))
    tab, instruction = order(SELLER_TERMS)
    f.send("ACCEPTED: evidence order 1, maximum 2,000, for size = 10 naming the carrier, signed by the agent, the seller "
        "and the credit issuer's passkey", instruction, tab["signers"], receipt=tab["receipt"])
    row = f.mark("passkey", "evidence order, list shown", "nothing")
    f.h.check("the evidence order was approved by one passkey proof, the credit issuer's", len(row["passkey_authorizations"]), 1)
    f.check_accepted_bill(tab, 1, carrier)

    source = f.parties["assignee"]["principal"]
    pin = sorted([term("size", "10"), signer_term(source)], key=lambda item: item["name"])
    permission = create("pinned-delivery-recoverable", seller["receiving"], pin)
    tab = f.recoverable_order("order-2-passkey-recoverable", SELLER_TERMS, source, mandate=permission, revision=1)
    f.place_recoverable(tab, "ACCEPTED: recoverable order 2, maximum 2,000, for size = 10 naming the source principal, "
        "signed by the agent, the seller and the credit issuer's passkey", case="passkey")
    row = f.h.cases[-1]
    f.h.check("the recoverable order was approved by one passkey proof, the credit issuer's", len(row["passkey_authorizations"]), 1)
    f.check_accepted_bill(tab, 2, source)
    f.h.check("both orders fit the 4,096-byte transaction limit", max(x["wire_bytes"] for x in f.h.cases) <= 4096, True)
    f.roles = {"buyer's owner (passkey alias)": buyer["owner"], "credit issuer's owner (passkey alias)": issuer["owner"],
        "agent": f.agent, "seller": seller["owner"], "carrier": f.meter}


RUNS = (("pinned-delivery", PinnedDelivery, exercise),
    ("pinned-delivery-recoverable", RecoverableDelivery, recoverable),
    ("pinned-delivery-passkey", PasskeyDelivery, passkey_owners))


def run(suite, fixture, scenario):
    f = fixture()
    try:
        scenario(f)
    except Exception as error:
        f.finish(suite, error=error)
        raise
    index = case_index(f, suite)
    for row in f.h.cases:
        if "commit_legs" in row:
            row["pinned_delivery"] = row.pop("commit_legs")
    return f, f.finish(suite), index


def record(results, directory, seconds):
    """Copy the finished runs into a publication directory; no second execution."""
    directory = Path(directory).resolve()
    directory.mkdir(parents=True, exist_ok=True)
    rows, receipts = [], {}
    for f, summary, index in results:
        suite = index[0]["suite"]
        raw = Path(summary["trace"]).read_bytes()
        packed = directory / (suite + "-trace.json.xz")
        packed.write_bytes(lzma.compress(raw))
        assert lzma.decompress(packed.read_bytes()) == raw
        annotated = [row for row in index if row["section"] != "setup"]
        receipts[suite] = dict(summary, trace=packed.name, trace_xz_sha256=digest(packed),
            annotated_cases=len(annotated),
            annotated_refusals=sum(row["outcome"] == "refused" for row in annotated),
            annotated_acceptances=sum(row["outcome"] == "accepted" for row in annotated),
            roles={role: str(holder.pubkey()) for role, holder in f.roles.items()})
        rows.extend(index)
    (directory / "case-index.json").write_text(json.dumps(rows, indent=1, default=json_value) + "\n")
    f = results[0][0]
    for name in ("build-receipt.json", "idl-receipt.json"):
        (directory / name).write_bytes((OUT / name).read_bytes())
    manifest = dict(runs=receipts, run_seconds=round(seconds, 2), engine="solders 0.29.0 LiteSVM",
        name_fingerprints={name: name_print(name).hex() for name in ("size", "delivery_signer")},
        value_fingerprints={value: value_print(value).hex() for value in ("10", "11")},
        program_sha256={name: value["sha256"] for name, value in f.build["artifacts"].items()},
        swig_sha256=SWIG_SHA256, build_receipt_sha256=digest(OUT / "build-receipt.json"),
        idl_receipt_sha256=digest(OUT / "idl-receipt.json"))
    (directory / "pinned-delivery-receipt.json").write_text(json.dumps(manifest, indent=2) + "\n")
    return manifest


def main():
    started = time.monotonic()
    results = [run(*item) for item in RUNS]
    if "--record" in sys.argv:
        print(json.dumps(record(results, sys.argv[sys.argv.index("--record") + 1], time.monotonic() - started)))


if __name__ == "__main__":
    main()
