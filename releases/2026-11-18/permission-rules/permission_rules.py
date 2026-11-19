#!/usr/bin/env python3
"""One standing permission with lists, ranges, a price ceiling and several sellers.

The buyer's owner gives agent 1 one permission, created with its rules in one
instruction (PrincipalRegistry `create_mandate`): sellers 1, 2 or 3; size 10
or 10.5; black or white; one to three pairs; at most 185 per completed-run
unit of the seller's inference service; at most 100,000 per order. The agent
then places many orders inside it through ClaimsCore
`accept_pinned_financed_tab`, which shows the order's terms.

A permission names 1 to 4 sellers and holds up to 4 rules, one per term name:

    list   the order's value fingerprint is one of 1 to 4 listed values
    range  the order's number is from min to max; the order shows the number
           beside its term, and its value is the number's fingerprint
           sha256("DEXTER:ORDER-TERM-NUMBER:1" || i64le(n))
    unit_price range   a price ceiling: the order's own service and
           completed-run unit are one of 1 to 4 listed, each
           sha256("DEXTER:PRICE-UNIT:1" || namespace || unit), and its own
           price per unit is from min to max

A decimal is a whole number of its smallest step and its name says the step:
size 10.5 is size_tenths = 105. A range bounds what the order shows; what the
seller bills is bounded by the order's maximum (and the per-order limit).

Orders outside the permission are refused at the commit. Each refusal is
checked as in pinned_terms.py: transaction error, first failing program frame,
Anchor error name and number, the source location in the failing frame's log
line (table `RAISED`, checked against the source text), byte-identical state
for every writable account except the fee payer, and no budget hold, lender
promise, order record, Registry reservation, operation record or receipt.
Every refusal has an accepted twin that differs from it in one respect. The
orders a reviewer sent against an earlier build (a price per a thousandth of
the unit, another service, a name shown twice, numbers beside text and price
terms, the billing namespace, a one-pair order billed for six units) are
recorded here with their twins.

Agent 2 holds a second permission whose delivery_signer is a list of two
carriers, and a spare permission that shows which rules the Registry refuses
to store. The owner later updates the standing permission's rules.

The harness writes no account bytes and never moves the clock.

Run in a prepared checkout: ./dev anchor-smoke permission-rules
Record: <python> foundation/onchain/tests/permission_rules.py --record docs/evidence/<directory>
"""
from pinned_price import *

NUMBER_DOMAIN = b"DEXTER:ORDER-TERM-NUMBER:1"
PRICE_UNIT_DOMAIN = b"DEXTER:PRICE-UNIT:1"
SMALL = 1_000            # the maximum of every order outside the sequence
BUDGET = 400_000         # the buyer's budget for all its agents; the facility and the credit line allow 500,000
CEILING = 185            # at most 185 per completed-run unit
SERVICE = (ident("provider-namespace"), ident("completed-inference-unit"))   # financing_fixture.py's service and unit
MILLI_UNIT = ident("one-thousandth-of-a-completed-inference-unit")
CONSULTING = (ident("consulting"), ident("second"))
REGISTRY_ERRORS = dict(ConfigurationConflict=6002, InvalidPolicy=6003, MandateRevision=6008, IntentMismatch=6010,
    PerOperationLimit=6011, BudgetExhausted=6012, TermsMismatch=6052, TermsNotPermitted=6053, NumberMismatch=6056,
    NumberOutOfRange=6057)
SOURCE = "thrown in programs/principal-registry/src/"
# Written here from the Registry source at this commit; `interface` checks each
# line names its error, and every refusal asserts its origin.
RAISED = {
    "set rules": ("lib.rs", 220, "InvalidPolicy"),
    "set revision": ("lib.rs", 226, "MandateRevision"),
    "created rules": ("lib.rs", 273, "InvalidPolicy"),
    "create sent again": ("lib.rs", 282, "ConfigurationConflict"),
    "order revision": ("lib.rs", 327, "MandateRevision"),
    "seller": ("lib.rs", 336, "IntentMismatch"),
    "per-order limit": ("lib.rs", 338, "PerOperationLimit"),
    "list names": ("lib.rs", 346, "TermsMismatch"),
    "number shown": ("lib.rs", 349, "NumberMismatch"),
    "name not listed": ("state.rs", 383, "TermsNotPermitted"),
    "no unit price": ("state.rs", 388, "TermsNotPermitted"),
    "billing or zero price": ("state.rs", 390, "TermsNotPermitted"),
    "service not named": ("state.rs", 392, "TermsNotPermitted"),
    "value not listed": ("state.rs", 396, "TermsNotPermitted"),
    "no number": ("state.rs", 398, "TermsNotPermitted"),
    "out of range": ("state.rs", 399, "NumberOutOfRange"),
    "budget": ("state.rs", 95, "BudgetExhausted"),
}
# Where the macro starts when its condition spans lines.
ORIGIN = {"create sent again": 277, "service not named": 391}
REGISTRY_SRC = ROOT / "foundation/onchain/programs/principal-registry/src"
CORE_BILLING_PROFILE = ("funding.rs", 471, "BillingProfile")


def refusal(name, where):
    file, line, _ = RAISED[where]
    return dict(program=REGISTRY, name=name, code=REGISTRY_ERRORS[name], source=f"{SOURCE}{file}:{ORIGIN.get(where, line)}")


def number_print(n):
    return hashlib.sha256(NUMBER_DOMAIN + struct.pack("<q", n)).digest()


def price_unit(namespace, unit):
    return hashlib.sha256(PRICE_UNIT_DOMAIN + namespace + unit).digest()


def number_term(name, n):
    """A whole-number term: the number's fingerprint, with the number shown beside it."""
    return dict(name=name_print(name), value=number_print(n), number=n)


def list_rule(name, values, *, kind="List", low=0, high=0):
    """A list rule: 1 to 4 value fingerprints, stored ascending, unused slots zero."""
    values = sorted(values)
    return dict(name=name_print(name) if isinstance(name, str) else name, kind=kind, value_count=len(values),
        values=values + [bytes(32)] * (4 - len(values)), min=low, max=high)


def range_rule(name, low, high):
    return dict(name=name_print(name) if isinstance(name, str) else name, kind="Range", value_count=0,
        values=[bytes(32)] * 4, min=low, max=high)


def ceiling_rule(services, low, high):
    """A price ceiling: the services it bounds, as their price-unit fingerprints, and the bounds on the price per unit."""
    return list_rule(UNIT_PRICE, [price_unit(*service) for service in services], kind="Range", low=low, high=high)


def rules(sellers, terms):
    """What create_mandate and set_mandate_rules take: sellers and rules, each ascending."""
    return dict(sellers=sorted(sellers, key=bytes), terms=sorted(terms, key=lambda item: item["name"]))


SIZES = list_rule("size_tenths", [number_print(100), number_print(105)])
COLOURS = list_rule("colour", [value_print("black"), value_print("white")])
QUANTITY = range_rule("quantity", 1, 3)
PRICE = ceiling_rule([SERVICE], 0, CEILING)
BLACK_ONLY = list_rule("colour", [value_print("black")])


class PermissionRules(PinnedPrice):
    def __init__(self):
        super().__init__()
        for name, seed in (("seller-2", 200), ("seller-3", 201), ("seller-4", 202)):
            self.add_party(name, seed)
        self.carrier_b, self.carrier_c = self.other_meter, key(203)
        # What the budget holds and has consumed, and what the lender has promised, written here as orders land.
        self.expected = dict(held=0, consumed=0, promised=0)

    def add_party(self, name, seed):
        """A further registered seller, set up as financing_fixture.py sets up its parties."""
        owner = key(seed)
        self.h.svm.airdrop(owner.pubkey(), 100_000_000_000)
        principal_id = ident("principal-" + name)
        principal = pda(REGISTRY, b"principal", owner.pubkey(), principal_id)
        party = dict(name=name, owner=owner, principal_id=principal_id, principal=principal, generation=1)
        party["book"] = pda(CORE, b"financial-book", principal, self.mint)
        party["projection"] = pda(POOL, b"financial-projection", party["book"])
        party["owner_ata"] = self.h.ata(owner.pubkey(), self.mint)
        self.parties[name] = party
        self.observe(principal, party["book"], party["projection"], party["owner_ata"])

    def seller(self, role):
        return self.parties[role]["owner"].pubkey()

    def roles_of(self, tab):
        return self.remaining(["company", "issuer", tab.get("seller", "supplier"), tab["financier"]])

    # -- permissions -------------------------------------------------------
    def create(self, name, index, given, label, *, case, what_changed="nothing", refuse=None):
        buyer = self.parties["company"]
        mandate_id = ident("mandate-" + name)
        permission = pda(REGISTRY, b"mandate", self.company, mandate_id)
        self.send(label, self.ix("principal_registry", "create_mandate",
            dict(owner=buyer["owner"].pubkey(), principal=self.company, actor=self.actors[index],
                budget=self.budget, mandate=permission),
            args=dict(mandate_id=mandate_id, source=self.facility, per_operation_limit=PER_ORDER_LIMIT,
                expires_at=NOW + PERMISSION_SECONDS), rules=given), [buyer["owner"]], reject=refuse is not None)
        self.mark(case, "create", what_changed, refuse=refuse)
        return permission

    def set_rules(self, permission, given, label, *, case, expected_revision, what_changed="nothing", refuse=None):
        buyer = self.parties["company"]
        self.send(label, self.registry.instruction("set_mandate_rules",
            dict(owner=buyer["owner"].pubkey(), principal=self.company, mandate=permission),
            expected_revision=expected_revision, rules=given), [buyer["owner"]], reject=refuse is not None)
        return self.mark(case, "set rules", what_changed, refuse=refuse)

    def stored(self, permission):
        """The permission's revision, state, sellers and rules; unused slots must be zero."""
        state = self.registry.read(self.h.svm, permission, "Mandate")
        sellers, count = state["seller_count"], state["rule_count"]
        self.h.check("unused seller and rule slots are zero",
            ([str(item) for item in state["sellers"][sellers:]], state["rules"][count:]),
            ([str(Pubkey.default())] * (4 - sellers), [UNUSED_RULE] * (4 - count)))
        return dict(revision=state["revision"], active=state["active"], sellers=state["sellers"][:sellers],
            terms=state["rules"][:count])

    # -- orders ------------------------------------------------------------
    def sold_by(self, tab, role):
        """The same order with another registered seller: principal, payout key and signature."""
        party = self.parties[role]
        tab["terms"]["supplier"] = tab["args"]["seller_fee_terms"]["supplier"] = party["owner"].pubkey()
        tab["accounts"].update(supplier=party["owner"].pubkey(), supplier_principal=party["principal"])
        tab["supplier_principal"] = party["principal"]
        tab["signers"][2] = party["owner"]
        tab["seller"] = role
        return tab

    def shoe(self, name, *, revision, seller="supplier", size=100, colour="black", quantity=1, price=CEILING, cap=SMALL,
            mandate=None, drop=(), replace=None, service=SERVICE):
        """An order through accept_pinned_financed_tab: a trail shoe, its size in tenths and quantity as numbers,
        at `price` per completed-run unit of `service` (namespace and unit)."""
        tab = self.acceptance(name, cap=cap, maximum_interest=0, fees=False, mandate=mandate or self.shoes)
        tab["terms"].update(expires_at=NOW + PERMISSION_SECONDS, mandate_revision=revision)
        tab["namespace"], tab["service_unit"] = tab["args"]["namespace"], tab["args"]["service_unit"] = service
        tab["unit_price"] = tab["args"]["unit_price"] = price
        self.sold_by(tab, seller)
        listed = {"item": term("item", "trail shoe"), "size_tenths": number_term("size_tenths", size),
            "colour": term("colour", colour), "quantity": number_term("quantity", quantity),
            "unit_price": unit_price_term(tab["namespace"], tab["service_unit"], price), "ship_to": term("ship_to", "warehouse 4")}
        listed.update(replace or {})
        return self.set_terms(tab, ascending(*(item for key, item in listed.items() if key not in drop)))

    def accepted_order(self, tab, row):
        logs = row["result"]["logs"]
        self.h.check(f"{tab['name']}: Core ran AcceptPinnedFinancedTab and the Registry ran ReservePinned inside it",
            ["Program log: Instruction: " + name in logs for name in ("AcceptPinnedFinancedTab", "ReservePinned")], [True, True])
        claim = self.state("FundedClaim", tab["claim"])
        self.h.check(f"{tab['name']}: the order record stores the terms hash both sides signed and the order maximum",
            (claim["external_contract_hash"], claim["remaining"], claim["unit_price"]),
            (tab["external_contract_hash"], tab["terms"]["cap"], tab["unit_price"]))
        reservation = self.registry.read(self.h.svm, tab["reservation"], "Reservation")
        self.h.check(f"{tab['name']}: the Registry reservation holds the order maximum for the seller who signed",
            (reservation["remaining"], reservation["terms"]["supplier"]), (tab["terms"]["cap"], tab["terms"]["supplier"]))
        self.expected["held"] += tab["terms"]["cap"]
        self.expected["promised"] += tab["terms"]["cap"]
        self.check_money(tab["name"])
        self.h.check(f"{tab['name']}: no tokens left the lender wallet", self.money()["lender_wallet"], LENDER_CASH)
        self.h.check(f"{tab['name']}: the buyer's owner does not sign the order",
            str(self.parties["company"]["owner"].pubkey()) in self.signers_of(row), False)

    def check_money(self, label):
        money = self.money()
        self.h.check(f"{label}: budget hold, budget consumed and lender promise afterwards",
            (money["budget_held"], money["budget_consumed"], money["lender_promised"]),
            (self.expected["held"], self.expected["consumed"], self.expected["promised"]))

    def order(self, tab, label, *, case, what_changed="nothing", refuse=None):
        row = self.place(tab, label, case=case, what_changed=what_changed, refuse=refuse)
        if refuse is None:
            self.accepted_order(tab, row)
        return row

    def bill(self, tab, units, label, *, case):
        """The seller reports `units` of completed work on an accepted order; its bill moves from held to consumed."""
        self.earn(tab, tab["name"] + "-work", units, label=label)
        row = self.mark(case, "seller's work report", "nothing")
        billed = units * tab["unit_price"]
        self.expected["held"] -= billed
        self.expected["consumed"] += billed
        self.check_money(tab["name"] + " after the work report")
        claim = self.state("FundedClaim", tab["claim"])
        self.h.check(f"{tab['name']}: the seller earned {billed:,} and {tab['terms']['cap'] - billed:,} of the maximum is unused",
            (claim["payable"], claim["remaining"]), (billed, tab["terms"]["cap"] - billed))
        return row


def interface(f):
    names = {error["name"]: error["code"] for error in f.registry.value["errors"]}
    f.h.check("error numbers written in this module equal the built Registry interface",
        {name: names.get(name) for name in REGISTRY_ERRORS}, REGISTRY_ERRORS)
    for where, (file, line, error) in RAISED.items():
        text = (REGISTRY_SRC / file).read_text().splitlines()[line - 1]
        f.h.check(f"source line {file}:{line} raises {error} ({where})", f"RegistryError::{error}" in text, True)
    f.h.check("create_mandate takes the permission's arguments and its rules",
        [a["name"] for a in f.registry.instructions["create_mandate"]["args"]], ["args", "rules"])
    f.h.check("set_mandate_rules takes the revision it replaces and the rules",
        [a["name"] for a in f.registry.instructions["set_mandate_rules"]["args"]], ["expected_revision", "rules"])
    f.h.check("the rules are sellers and term rules", [x["name"] for x in f.registry.types["MandateRules"]["fields"]],
        ["sellers", "terms"])
    f.h.check("a term rule is a name, a kind, a value count, four values and two bounds",
        [(x["name"], x["type"]) for x in f.registry.types["TermRule"]["fields"]],
        [("name", {"array": ["u8", 32]}), ("kind", {"defined": {"name": "RuleKind"}}), ("value_count", "u8"),
         ("values", {"array": [{"array": ["u8", 32]}, 4]}), ("min", "i64"), ("max", "i64")])
    f.h.check("an order term carries the number its value may fingerprint",
        [x["name"] for x in f.registry.types["Term"]["fields"]], ["name", "value", "number"])
    f.h.check("ClaimsCore's pinned order takes the plain order's arguments plus the shown list",
        [a["name"] for a in f.core.instructions["accept_pinned_financed_tab"]["args"]],
        [a["name"] for a in f.core.instructions["accept_financed_tab"]["args"]] + ["order_terms"])
    shared = json.loads(SHARED_VECTORS.read_text())
    f.h.check("the shared vectors name the number domain this module uses", shared["number"]["domain"].encode(), NUMBER_DOMAIN)
    for vector in shared["number"]["vectors"]:
        f.h.check("shared number vector: " + vector["number"], number_print(int(vector["number"])).hex(), vector["value"])
    f.h.check("size 10 as a number and as text have different fingerprints", number_print(100) == value_print("10"), False)
    f.h.check("the shared vectors name the price-unit domain this module uses", shared["price_unit"]["domain"].encode(),
        PRICE_UNIT_DOMAIN)
    for vector in shared["price_unit"]["vectors"]:
        f.h.check("shared price-unit vector: " + vector["name"],
            price_unit(bytes.fromhex(vector["namespace"]), bytes.fromhex(vector["unit"])).hex(), vector["value"])
    f.h.check("the ceiling's service and the two the review ordered are the first three price-unit vectors",
        [(bytes.fromhex(v["namespace"]), bytes.fromhex(v["unit"])) for v in shared["price_unit"]["vectors"][:3]],
        [SERVICE, (SERVICE[0], MILLI_UNIT), CONSULTING])


def cast(f):
    p = f.parties
    f.roles = {"buyer's owner": p["company"]["owner"], "agent 1": f.agent, "agent 2": f.agent2,
        "seller 1": p["supplier"]["owner"], "seller 2": p["seller-2"]["owner"], "seller 3": p["seller-3"]["owner"],
        "seller 4": p["seller-4"]["owner"], "carrier A": f.meter, "carrier B": f.carrier_b, "carrier C": f.carrier_c,
        "lender's owner": p["financier"]["owner"], "credit issuer's owner": p["issuer"]["owner"],
        "fee payer of setup and order transactions": f.h.payer}
    keys = [str(holder.pubkey()) for holder in f.roles.values()]
    f.h.check("all thirteen roles are held by different keys", len(set(keys)), len(keys))


def setup(f):
    f.setup_principals()
    for role, amount in (("protocol", 0), ("fee", 0), ("issuer", 200_000)):
        f.create_custody(role, amount)
    f.create_custody("financier", LENDER_CASH, wallet=True)
    f.open_fee_agreement("main", 2_100)
    f.facility = f.open_facility()
    f.line, f.assignment_policy = f.open_line(f.facility)
    f.set_budget(f"the buyer's owner sets the budget all its agents share to {BUDGET:,}", BUDGET)
    f.h.check("the shared budget is 400,000; the facility and the credit line allow 500,000",
        (f.registry.read(f.h.svm, f.budget, "Budget")["ceiling"], f.state("FundingFacility", f.facility)["commitment_limit"],
         f.state("CustomerCreditLine", f.line)["limit"]), (BUDGET, 500_000, 500_000))


def standing_permission(f):
    """The permission is created with its rules, in one instruction."""
    sellers = [f.seller(role) for role in ("supplier", "seller-2", "seller-3")]
    f.r1 = rules(sellers, [SIZES, COLOURS, QUANTITY, PRICE])
    f.shoes = f.create("shoes", 0, f.r1, "ACCEPTED: the buyer's owner gives agent 1 one standing permission, in one "
        "instruction: sellers 1, 2 or 3; size 10 or 10.5; black or white; 1 to 3 pairs; at most 185 per unit of the "
        "inference service; "
        "at most 100,000 per order", case="permission")
    row = f.h.cases[-1]
    f.h.check("the permission is created with its sellers and rules at revision 1",
        f.stored(f.shoes), dict(revision=1, active=True, sellers=f.r1["sellers"], terms=f.r1["terms"]))
    account = f.h.svm.get_account(f.shoes)
    f.h.check("the permission record is 1,070 bytes and holds the runtime's rent-exempt minimum for its size",
        (len(account.data), account.lamports), (1_070, f.h.svm.minimum_balance_for_rent_exemption(1_070)))
    f.h.check("creating it is one instruction, signed by the fee payer and the buyer's owner only",
        (len(row["instructions"]), sorted(f.signers_of(row))),
        (1, sorted([str(f.h.payer.pubkey()), str(f.parties["company"]["owner"].pubkey())])))
    f.permission_bytes = len(account.data)
    f.create_wire_bytes = row["wire_bytes"]


def lists(f):
    f.order(f.shoe("list-size-10", revision=1, size=110), "refused: agent 1 orders size 11 from seller 1; the permission "
        "allows size 10 or 10.5", case="list", what_changed="size: 110 tenths instead of 100",
        refuse=refusal("TermsNotPermitted", "value not listed"))
    f.order(f.shoe("list-size-10", revision=1), "ACCEPTED: agent 1 orders size 10, black, 1 pair at 185 per unit from "
        "seller 1", case="list")
    f.order(f.shoe("list-white", revision=1, size=105, colour="red"), "refused: agent 1 orders size 10.5 in red; the "
        "permission allows black or white", case="list", what_changed="colour: red instead of white",
        refuse=refusal("TermsNotPermitted", "value not listed"))
    tab = f.shoe("list-white", revision=1, size=105, colour="white")
    f.order(tab, "ACCEPTED: agent 1 orders size 10.5, white, 1 pair at 185 per unit from seller 1", case="list")
    f.h.check("the size 10.5 order shows 105 tenths with its number", number_term("size_tenths", 105) in tab["order_terms"], True)


def ranges(f):
    def qty(name, n, **options):
        return f.shoe(name, revision=1, quantity=n, **options)
    f.order(qty("range-min", 0), "refused: agent 1 orders 0 pairs; the permission allows 1 to 3",
        case="range", what_changed="quantity: 0 instead of 1", refuse=refusal("NumberOutOfRange", "out of range"))
    shown_two = qty("range-min", 1, replace={"quantity": dict(number_term("quantity", 1), number=2)})
    f.h.check("the order that shows 2 beside the fingerprint of 1 hashes as the order of 1 pair",
        shown_two["external_contract_hash"], qty("range-min", 1)["external_contract_hash"])
    f.order(shown_two, "refused: agent 1's order carries the fingerprint of 1 pair and shows the number 2 beside it",
        case="range", what_changed="number shown beside quantity: 2, while its value is the fingerprint of 1",
        refuse=refusal("NumberMismatch", "number shown"))
    f.order(qty("range-min", 1, replace={"quantity": dict(number_term("quantity", 1), number=None)}),
        "refused: agent 1's order carries the fingerprint of 1 pair and shows no number beside it",
        case="range", what_changed="number not shown beside quantity",
        refuse=refusal("TermsNotPermitted", "no number"))
    f.order(qty("range-min", 1, drop=("quantity",)), "refused: agent 1's order leaves quantity out of its terms",
        case="range", what_changed="quantity absent from the terms", refuse=refusal("TermsNotPermitted", "name not listed"))
    f.order(qty("range-min", 1), "ACCEPTED: agent 1 orders 1 pair, the least the permission allows", case="range")
    f.order(qty("range-max", 4), "refused: agent 1 orders 4 pairs; the permission allows 1 to 3",
        case="range", what_changed="quantity: 4 instead of 3", refuse=refusal("NumberOutOfRange", "out of range"))
    f.order(qty("range-max", 3), "ACCEPTED: agent 1 orders 3 pairs, the most the permission allows", case="range")


def shown_lists(f):
    """What an order may show beside its terms: each name once, a number only where the value is that number's."""
    twice = f.shoe("shown", revision=1)
    twice["order_terms"] = sorted(twice["order_terms"] + [term("quantity", "1")], key=lambda item: item["name"])
    f.order(twice, "refused: agent 1's order shows quantity twice, as the number 1 and as the text 1",
        case="shown list", what_changed="quantity shown a second time, as text", refuse=refusal("TermsMismatch", "list names"))

    def with_number(name, number):
        tab = f.shoe("shown", revision=1)
        tab["order_terms"] = [dict(item, number=number) if item["name"] == name else item for item in tab["order_terms"]]
        return tab
    f.order(with_number(UNIT_PRICE, CEILING), "refused: agent 1's order shows the number 185 beside its unit_price term",
        case="shown list", what_changed="number shown beside unit_price: 185", refuse=refusal("NumberMismatch", "number shown"))
    f.order(with_number(name_print("colour"), 0), "refused: agent 1's order shows the number 0 beside its text term colour",
        case="shown list", what_changed="number shown beside colour: 0", refuse=refusal("NumberMismatch", "number shown"))
    f.order(f.shoe("shown", revision=1), "ACCEPTED: agent 1's order shows each term once, numbers beside size and "
        "quantity only", case="shown list")


def price_ceiling(f):
    f.order(f.shoe("price", revision=1, price=CEILING + 1), "refused: agent 1 orders at 186 per unit; the permission "
        "allows at most 185", case="price ceiling", what_changed="unit price: 186 instead of 185",
        refuse=refusal("NumberOutOfRange", "out of range"))
    f.order(f.shoe("price", revision=1, drop=("unit_price",)), "refused: agent 1's order at 185 per unit leaves its "
        "unit_price term out of its terms", case="price ceiling", what_changed="unit_price absent from the terms",
        refuse=refusal("TermsNotPermitted", "name not listed"))
    f.order(f.shoe("price", revision=1, service=(SERVICE[0], MILLI_UNIT)), "refused: agent 1 orders at 185 per "
        "thousandth of a completed-run unit of the same service; the ceiling names the whole unit",
        case="price ceiling", what_changed="completed-run unit: a thousandth of the unit the ceiling names",
        refuse=refusal("TermsNotPermitted", "service not named"))
    f.order(f.shoe("price", revision=1, service=CONSULTING), "refused: agent 1 orders consulting, per second, at 185; "
        "the ceiling names the inference service only", case="price ceiling",
        what_changed="service and unit: consulting, per second", refuse=refusal("TermsNotPermitted", "service not named"))
    tab = f.shoe("price", revision=1)
    f.order(tab, "ACCEPTED: agent 1 orders at 185 per unit of the inference service, the ceiling", case="price ceiling")
    claim = f.state("FundedClaim", tab["claim"])
    f.h.check("the order record prices 185 per completed-run unit of the named service",
        (claim["unit_price"], claim["service_unit"]), (CEILING, SERVICE[1]))
    billing = dict(program=CORE, name="BillingProfile", code={e["name"]: e["code"] for e in f.core.value["errors"]}["BillingProfile"],
        source=f"thrown in programs/claims-core/src/{CORE_BILLING_PROFILE[0]}:{CORE_BILLING_PROFILE[1]}")
    f.order(f.shoe("price-1", revision=1, price=1, service=(BILLING_NAMESPACE, BILLING_SETTLEMENT_UNIT)),
        "refused: agent 1 orders at 1 per unit in the billing namespace and unit through the completed-run path",
        case="price ceiling", what_changed="service and unit: the billing namespace and settlement unit", refuse=billing)
    f.order(f.shoe("price-1", revision=1, price=1), "ACCEPTED: agent 1 orders at 1 per unit of the inference service",
        case="price ceiling")


def sellers(f):
    f.order(f.shoe("seller-2", revision=1, seller="seller-2"), "ACCEPTED: agent 1 orders size 10, black, 1 pair at 185 "
        "per unit from seller 2", case="sellers")
    f.order(f.shoe("seller-3", revision=1, seller="seller-4"), "refused: agent 1 orders the same shoe from seller 4, "
        "whom the permission does not name", case="sellers", what_changed="seller: seller 4 (principal, payout key and "
        "signature) instead of seller 3", refuse=refusal("IntentMismatch", "seller"))
    f.order(f.shoe("seller-3", revision=1, seller="seller-3"), "ACCEPTED: agent 1 orders the shoe from seller 3",
        case="sellers")


def delivery_signers(f):
    """Agent 2's permission: delivery_signer is one of two carriers. A price ceiling refuses evidence orders."""
    carrier_a, carrier_b, carrier_c = f.meter.pubkey(), f.carrier_b.pubkey(), f.carrier_c.pubkey()
    signer_rule = list_rule(DELIVERY_SIGNER, [bytes(carrier_a), bytes(carrier_b)])
    carriers = rules([f.seller("supplier")], [signer_rule])
    f.carriers = f.create("carriers", 1, carriers, "ACCEPTED: the buyer's owner gives agent 2 a permission whose "
        "delivery signer is carrier A or carrier B", case="delivery signer")

    def evidence(name, signer, revision, reserved=None):
        return f.priced_order(name, revision=revision, mandate=f.carriers, index=1, signer=signer,
            reserved=(signer_term(signer),) if reserved is None else reserved)

    def accept(name, signer, label, revision=1):
        tab = evidence(name, signer, revision)
        f.place_delivery(tab, label, case="delivery signer" if revision == 1 else "price ceiling, evidence order")
        f.check_accepted_bill(tab, name, signer)
        f.expected["held"] += CAP
        f.expected["promised"] += CAP
        f.check_money(name)
    accept("evidence-a", carrier_a, "ACCEPTED: agent 2's evidence order of 2,000 names carrier A to sign for delivery")
    f.place_delivery(evidence("evidence-b", carrier_c, 1), "refused: agent 2's evidence order names carrier C to sign "
        "for delivery", case="delivery signer", what_changed="delivery signer: carrier C instead of carrier B",
        refuse=refusal("TermsNotPermitted", "value not listed"))
    accept("evidence-b", carrier_b, "ACCEPTED: agent 2's evidence order of 2,000 names carrier B to sign for delivery")

    # A price ceiling bounds a price per completed-run unit; an evidence order has a tariff instead.
    ceiling = rules([f.seller("supplier")], [signer_rule, PRICE])
    f.set_rules(f.carriers, ceiling, "ACCEPTED: the buyer's owner adds a price ceiling of 185 per unit of the inference "
        "service to agent 2's permission", case="price ceiling, evidence order", expected_revision=1)
    f.place_delivery(evidence("evidence-ceiling", carrier_a, 2), "refused: agent 2's evidence order naming carrier A, "
        "under the price ceiling; it has no unit_price term", case="price ceiling, evidence order",
        what_changed="the permission has a price ceiling", refuse=refusal("TermsNotPermitted", "name not listed"))
    listed = (signer_term(carrier_a), unit_price_term(*SERVICE, 1))
    f.place_delivery(evidence("evidence-ceiling", carrier_a, 2, listed), "refused: agent 2's evidence order naming "
        "carrier A lists a unit_price term under the price ceiling; an evidence order has no unit price",
        case="price ceiling, evidence order", what_changed="the permission has a price ceiling; the list carries a unit_price term",
        refuse=refusal("TermsNotPermitted", "no unit price"))
    f.set_rules(f.carriers, carriers, "ACCEPTED: the buyer's owner removes the price ceiling from agent 2's permission",
        case="price ceiling, evidence order", expected_revision=2)
    accept("evidence-ceiling", carrier_a, "ACCEPTED: agent 2's evidence order of 2,000 naming carrier A, without the "
        "price ceiling", revision=3)
    f.h.check("agent 2's permission is at revision 3 with its carrier rule", f.stored(f.carriers),
        dict(revision=3, active=True, sellers=carriers["sellers"], terms=carriers["terms"]))


def invalid_rules(f):
    """The Registry refuses rules it cannot store in their one form; each twin is the valid set accepted after them."""
    one, two = f.seller("supplier"), f.seller("seller-2")
    valid = rules([one, two], [SIZES, QUANTITY])
    invalid = refusal("InvalidPolicy", "set rules")
    duplicate = rules([one, one], [SIZES, QUANTITY])
    f.create("spare", 1, duplicate, "refused: the buyer's owner creates a permission naming seller 1 twice",
        case="invalid rules", what_changed="sellers: seller 1 twice instead of sellers 1 and 2",
        refuse=refusal("InvalidPolicy", "created rules"))
    f.h.check("no permission exists after the refused creation",
        absent(f.h.svm, pda(REGISTRY, b"mandate", f.company, ident("mandate-spare"))), True)
    f.spare = f.create("spare", 1, valid, "ACCEPTED: the buyer's owner creates a spare permission for agent 2: sellers "
        "1 and 2; size 10 or 10.5; 1 to 3 pairs", case="invalid rules")
    f.create("spare", 1, valid, "no effect: the same creation is sent again", case="invalid rules",
        what_changed="the creation is sent a second time")
    row = f.h.cases[-1]
    f.h.check("sending the creation again changes no observed account", row["observed_after"], row["observed_before"])
    f.create("spare", 1, rules([one], valid["terms"]), "refused: the creation is sent again naming seller 1 only",
        case="invalid rules", what_changed="sellers: seller 1 instead of sellers 1 and 2",
        refuse=refusal("ConfigurationConflict", "create sent again"))
    # Each refused set differs in one respect from `target`, which the owner sets after them.
    size, quantity = SIZES, range_rule("quantity", 1, 2)
    target = rules([one, two], [size, quantity])

    def terms(size_rule=size, quantity_rule=quantity):
        return dict(target, terms=sorted([size_rule, quantity_rule], key=lambda item: item["name"]))
    many = [range_rule(bytes([index]) * 32, 0, 1) for index in range(1, 6)]
    cases = (
        (dict(target, terms=sorted([size, quantity], key=lambda item: item["name"], reverse=True)),
            "the two rules with their names out of ascending order", "order of the rule names"),
        (dict(target, terms=sorted([size, list_rule("size_tenths", [number_print(110)]), quantity], key=lambda item: item["name"])),
            "size twice, as 10 or 10.5 and as 11", "a second rule for size"),
        (terms(dict(size, value_count=0, values=[bytes(32)] * 4)), "a size list with no values", "size list: empty"),
        (terms(dict(size, value_count=5)), "a size list that counts five values; a list holds at most four",
            "size list: count 5 instead of 2"),
        (terms(dict(size, values=size["values"][:3] + [value_print("stray")])),
            "a size list of two values with a value in an unused slot", "size list: a value past its count"),
        (terms(dict(size, values=[size["values"][1], size["values"][0]] + [bytes(32)] * 2)),
            "the size list's two values in descending order", "order of the size values"),
        (terms(quantity_rule=range_rule("quantity", 2, 1)), "quantity from 2 to 1", "quantity bounds: 2 to 1 instead of 1 to 2"),
        (rules([one, two], [size, quantity, range_rule(DELIVERY_SIGNER, 0, 1)]), "a range on delivery_signer",
            "a range rule on delivery_signer added"),
        (rules([one, two], [size, quantity, range_rule(BILLING_TARIFF, 0, 1)]), "a range on billing_tariff",
            "a range rule on billing_tariff added"),
        (rules([one, two], [size, quantity, dict(PRICE, value_count=0, values=[bytes(32)] * 4)]),
            "a price ceiling that names no service", "a price ceiling with no service added"),
        (dict(target, terms=many), "five rules; a permission holds at most four", "rules: five instead of two"),
        (dict(target, sellers=sorted([one, two, f.seller("seller-3"), f.seller("seller-4"), f.agent.pubkey()], key=bytes)),
            "five sellers; a permission names at most four", "sellers: five instead of two"),
        (dict(target, sellers=[one, one]), "seller 1 twice", "sellers: seller 1 twice instead of sellers 1 and 2"),
        (dict(target, sellers=[Pubkey.default(), one]), "the all-zero key as a seller", "sellers: the all-zero key instead of seller 2"),
        (dict(target, sellers=sorted([one, two], key=bytes, reverse=True)), "sellers 1 and 2 out of ascending order", "order of the sellers"),
        (dict(target, sellers=[]), "no seller", "sellers: none"),
    )
    for given, what, what_changed in cases:
        f.set_rules(f.spare, given, f"refused: the buyer's owner sets rules on the spare permission with {what}",
            case="invalid rules", expected_revision=1, what_changed=what_changed, refuse=invalid)
    f.h.check("the spare permission is unchanged after the refused rules",
        f.stored(f.spare), dict(revision=1, active=True, sellers=valid["sellers"], terms=valid["terms"]))
    f.set_rules(f.spare, target, "ACCEPTED: the buyer's owner sets rules on the spare permission: sellers 1 and 2; "
        "size 10 or 10.5; 1 to 2 pairs", case="invalid rules", expected_revision=1)
    f.h.check("the spare permission holds those rules at revision 2",
        f.stored(f.spare), dict(revision=2, active=True, sellers=target["sellers"], terms=target["terms"]))


def update(f):
    """The owner adds seller 4 and narrows the colour to black; a list of one requires exactly that value."""
    stale = f.shoe("update-stale", revision=1, seller="seller-2")
    f.r2 = rules([f.seller(role) for role in ("supplier", "seller-2", "seller-3", "seller-4")],
        [SIZES, BLACK_ONLY, QUANTITY, PRICE])
    f.set_rules(f.shoes, f.r2, "ACCEPTED: the buyer's owner adds seller 4 to agent 1's permission and allows black only",
        case="update", expected_revision=1)
    f.h.check("the permission moves to revision 2 with four sellers and the colour list of one",
        f.stored(f.shoes), dict(revision=2, active=True, sellers=f.r2["sellers"], terms=f.r2["terms"]))
    row = f.set_rules(f.shoes, f.r2, "no effect: the same rules are sent again against revision 1", case="update",
        expected_revision=1, what_changed="the update is sent a second time")
    f.h.check("sending the update again changes no observed account", row["observed_after"], row["observed_before"])
    f.set_rules(f.shoes, f.r1, "refused: the original rules are sent again against revision 1; the permission is at "
        "revision 2", case="update", expected_revision=1, what_changed="rules: the original set, against a stale revision",
        refuse=refusal("MandateRevision", "set revision"))
    f.h.check("agent 1's permission is still at revision 2", f.stored(f.shoes)["revision"], 2)
    f.order(stale, "refused: agent 1's order from seller 2, signed against revision 1 before the update",
        case="update", what_changed="permission revision named by the order: 1 instead of 2",
        refuse=refusal("MandateRevision", "order revision"))
    f.order(f.shoe("update-stale", revision=2, seller="seller-2"), "ACCEPTED: the same order from seller 2 signed "
        "against revision 2", case="update")
    f.order(f.shoe("update-white", revision=2, colour="white"), "refused: agent 1 orders white; the permission now "
        "allows black only", case="update", what_changed="colour: white instead of black",
        refuse=refusal("TermsNotPermitted", "value not listed"))
    f.order(f.shoe("update-white", revision=2), "ACCEPTED: agent 1 orders black", case="update")
    f.order(f.shoe("seller-4", revision=2, seller="seller-4"), "ACCEPTED: agent 1 orders from seller 4, whom the owner "
        "added", case="update")


def billed(f):
    """A range bounds what the order shows. The bill is bounded by the order's maximum."""
    tab = f.shoe("one-pair", revision=2, price=150)
    f.order(tab, "ACCEPTED: agent 1 orders 1 pair at 150 per unit, maximum 1,000", case="what a range bounds")
    f.bill(tab, 6, "ACCEPTED: seller 1 reports 6 completed-run units at 150 on the order that shows 1 pair: 900 earned",
        case="what a range bounds")
    f.h.check("the 1-pair order is billed 900 for 6 units, inside its maximum of 1,000",
        (f.state("FundedClaim", tab["claim"])["payable"], tab["terms"]["cap"]), (900, SMALL))


def sequence(f):
    """One permission, order after order: sizes and sellers change; the per-order limit and the budget still hold."""
    def shoe(name, seller, size, quantity, price, cap):
        return f.shoe(name, revision=2, seller=seller, size=size, quantity=quantity, price=price, cap=cap)
    for name, seller, number, size, quantity, price in (("seq-1", "supplier", 1, 100, 1, 150), ("seq-2", "seller-2", 2, 105, 2, 185)):
        f.order(shoe(name, seller, size, quantity, price, PER_ORDER_LIMIT), f"ACCEPTED: agent 1 orders size {size / 10:g}, "
            f"{quantity} pair{'s' if quantity > 1 else ''} at {price} per unit from seller {number}, maximum 100,000",
            case="sequence")
    f.order(shoe("seq-3", "seller-3", 100, 3, 120, PER_ORDER_LIMIT + 1), "refused: agent 1 orders size 10, 3 pairs at "
        "120 per unit from seller 3 with a maximum of 100,001; the per-order limit is 100,000", case="sequence",
        what_changed="maximum: 100,001 instead of 100,000", refuse=refusal("PerOperationLimit", "per-order limit"))
    f.order(shoe("seq-3", "seller-3", 100, 3, 120, PER_ORDER_LIMIT), "ACCEPTED: the same order with a maximum of 100,000",
        case="sequence")
    left = BUDGET - f.expected["held"] - f.expected["consumed"]
    budget = f.registry.read(f.h.svm, f.budget, "Budget")
    f.h.check("before order seq-4 the budget has less than 100,000 left",
        (budget["ceiling"] - budget["reserved"] - budget["consumed"], 0 < left < PER_ORDER_LIMIT), (left, True))
    f.order(shoe("seq-4", "seller-4", 105, 1, 185, PER_ORDER_LIMIT), f"refused: agent 1 orders size 10.5, 1 pair at 185 per "
        f"unit from seller 4 with a maximum of 100,000; the budget all agents share has {left:,} left", case="sequence",
        what_changed=f"maximum: 100,000 instead of {left:,}", refuse=refusal("BudgetExhausted", "budget"))
    f.order(shoe("seq-4", "seller-4", 105, 1, 185, left), f"ACCEPTED: the same order with a maximum of {left:,}, "
        "what the budget has left", case="sequence")
    budget = f.registry.read(f.h.svm, f.budget, "Budget")
    f.h.check("the budget is fully used: 400,000 reserved or consumed", budget["reserved"] + budget["consumed"], BUDGET)
    f.sequence_left = left


def end_state(f):
    money, line = f.money(), f.state("CustomerCreditLine", f.line)
    f.h.check("at the end the budget holds and has consumed what the run accepted and billed; the lender's promise and the "
        "credit line, unused plus owed, hold the accepted maximums",
        (money["budget_held"], money["budget_consumed"], money["lender_promised"], line["reserved"] + line["principal_outstanding"]),
        (f.expected["held"], f.expected["consumed"], f.expected["promised"], f.expected["promised"]))
    f.h.check("no tokens left the lender wallet in this run", money["lender_wallet"], LENDER_CASH)
    refused = [row for row in f.h.cases if row["expected"] == "reject"]
    f.h.check("every refusal in this run was annotated and checked",
        all(row.get("commit_legs", {}).get("outcome") == "refused" for row in refused), True)


def exercise(f):
    interface(f)
    setup(f)
    cast(f)
    standing_permission(f)
    lists(f)
    ranges(f)
    shown_lists(f)
    price_ceiling(f)
    sellers(f)
    delivery_signers(f)
    invalid_rules(f)
    update(f)
    billed(f)
    sequence(f)
    end_state(f)


def record(results, directory, seconds):
    """Copy the finished run into a publication directory; no second execution."""
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
        name_fingerprints={name: name_print(name).hex() for name in ("size_tenths", "colour", "quantity", "unit_price",
            "delivery_signer")},
        value_fingerprints={value: value_print(value).hex() for value in ("black", "white", "red")},
        number_fingerprints={str(n): number_print(n).hex() for n in (0, 1, 2, 3, 4, 100, 105, 110)},
        permission_bytes=f.permission_bytes, create_wire_bytes=f.create_wire_bytes,
        program_sha256={name: value["sha256"] for name, value in f.build["artifacts"].items()},
        swig_sha256=SWIG_SHA256, build_receipt_sha256=digest(OUT / "build-receipt.json"),
        idl_receipt_sha256=digest(OUT / "idl-receipt.json"))
    (directory / "permission-rules-receipt.json").write_text(json.dumps(manifest, indent=2) + "\n")
    return manifest


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
            row["permission_rules"] = row.pop("commit_legs")
    return f, f.finish(suite), index


def main():
    started = time.monotonic()
    results = [run("permission-rules", PermissionRules, exercise)]
    if "--record" in sys.argv:
        print(json.dumps(record(results, sys.argv[sys.argv.index("--record") + 1], time.monotonic() - started)))


if __name__ == "__main__":
    main()
