#!/usr/bin/env python3
"""The owner pins the price; a retried order shows the terms it was accepted with.

Two more reserved term names, checked by the Registry the way delivery_signer
is checked (pinned_delivery.py):

    billing_tariff  value = sha256("DEXTER:BILLING-TARIFF:1" || u8(metric_count) || metric_ids[8]
                            || price_numerators[8] || price_denominators[8] || max_units[8])
                    of an evidence or recoverable order's billing policy, integers u64 little-endian
    unit_price      value = sha256("DEXTER:UNIT-PRICE:1" || namespace || unit || u64le(unit_price))
                    of an order through accept_pinned_financed_tab

A listed reserved term must carry the order's own value (`PriceMismatch` for
the two price names); an order that has no such value cannot list the name.
A pinned price the order leaves out fails the pin check (`TermsNotPermitted`).

A retry of an accepted order through a pinned instruction returns the original
outcome only when the list it shows hashes to what the order committed to: its
external contract hash, or its billing policy's request_hash. Otherwise
ClaimsCore refuses it with `OrderTermsMismatch`. Plain retries are unchanged.

The Registry's own recompute of the billing policy hash is not reachable on
its own: ClaimsCore computes the same hash from the same domain, bill, claim
and policy and refuses first (`BillingPolicy`), and `reserve_pinned_billing`
requires ClaimsCore's authority signature. The list-to-request_hash part of
that check is reached and recorded (TermsMismatch rows in pinned_delivery.py).

Main run on the setup of pinned_delivery.py. A recoverable run on recovery.py
takes a pinned order through the source's report, the seller's claim,
finalization and payment, with an owner rotation of the source principal in
between. Refusals are checked as in pinned_terms.py.

Run in a prepared checkout: ./dev anchor-smoke pinned-price
Record: <python> foundation/onchain/tests/pinned_price.py --record docs/evidence/<directory>
"""
import struct

from pinned_delivery import *
from pinned_delivery import refusal as delivery_refusal, REGISTRY_ERRORS as DELIVERY_ERRORS

BILLING_TARIFF = name_print("billing_tariff")
UNIT_PRICE = name_print("unit_price")
REGISTRY_ERRORS = dict(DELIVERY_ERRORS, PriceMismatch=6055)
CORE_ERRORS = dict(OrderTermsMismatch=6077, BillingAuthorityGeneration=6070)
RAISED.update({
    "price": REGISTRY_SOURCE + "lib.rs:368",
    "retried list": CORE_SOURCE + "funding.rs:517",
    "source generation": CORE_SOURCE + "recoverable_billing.rs:115",
})
AT_1 = ((1, 1, CAP),)    # 1 per unit, at most 2,000 units
AT_2 = ((2, 1, CAP),)    # 2 per unit


def refusal(name, where, *, index=0):
    if name in REGISTRY_ERRORS:
        return dict(program=REGISTRY, name=name, code=REGISTRY_ERRORS[name], index=index, source=RAISED[where])
    return dict(program=CORE, name=name, code=CORE_ERRORS[name], index=index, source=RAISED[where])


def metric_ids(count):
    """evidence_billing.py's metric ids for `count` metrics, all eight slots."""
    return [ident("metric-" + str(i)) for i in range(count)] + [ZERO] * (8 - count)


def tariff_fields(rates, ids=None):
    """The tariff of evidence_billing.py's policy for `rates`, all eight slots."""
    count = len(rates)
    return dict(metric_count=count, metric_ids=metric_ids(count) if ids is None else ids,
        price_numerators=[r[0] for r in rates] + [0] * (8 - count), price_denominators=[r[1] for r in rates] + [0] * (8 - count),
        max_units=[r[2] for r in rates] + [0] * (8 - count))


def tariff_hash(policy):
    return hashlib.sha256(b"DEXTER:BILLING-TARIFF:1" + bytes([policy["metric_count"]]) + b"".join(policy["metric_ids"])
        + struct.pack("<8Q", *policy["price_numerators"]) + struct.pack("<8Q", *policy["price_denominators"])
        + struct.pack("<8Q", *policy["max_units"])).digest()


def tariff_term(rates, ids=None):
    return dict(name=BILLING_TARIFF, value=tariff_hash(tariff_fields(rates, ids)))


def unit_price_term(namespace, unit, price):
    return dict(name=UNIT_PRICE, value=hashlib.sha256(b"DEXTER:UNIT-PRICE:1" + namespace + unit + u64(price)).digest())


def ascending(*terms):
    return sorted(terms, key=lambda item: item["name"])


class Priced:
    """Orders built at a chosen tariff and under a chosen permission."""
    rates = None
    order_mandate = None

    def policy(self, name, **options):
        if self.rates is not None:
            options["rates"] = self.rates
        return super().policy(name, **options)

    def acceptance(self, name, **options):
        if self.order_mandate is not None:
            options.setdefault("mandate", self.order_mandate)
        return super().acceptance(name, **options)

    def check_tariff(self, tab, rates, ids=None):
        self.h.check(f"{tab['name']}: the policy's tariff hashes as the listed tariff of these rates",
            tariff_hash(tab["billing_policy"]), tariff_hash(tariff_fields(rates, ids)))

    def retry(self, label, instruction, tab, *, case, what_changed, refuse=None):
        """A retry of an accepted order: refused or not, its records stay byte-identical."""
        records = [tab["claim"], tab["reservation"], tab["receipt"]] + ([tab["bill"]] if "bill" in tab else [])
        before = self.h.snapshot(records)
        self.send(label, instruction, tab["signers"], reject=refuse is not None, receipt=tab["receipt"])
        row = self.mark(case, "retry", what_changed, refuse=refuse)
        self.h.check("the retry left the accepted order's records byte-identical: " + label, self.h.snapshot(records), before)
        self.h.check("the retry did not reach the Registry: " + label, f"Program {REGISTRY} invoke [2]" in ((row["result"].get("meta") or row["result"]).get("logs") or []), False)
        return row


class PinnedPrice(Priced, PinnedDelivery):
    def priced_order(self, name, *, revision, mandate, index=0, pairs=SELLER_TERMS, signer=None, rates=AT_1, reserved=(),
            listed=None, ids=None):
        """An evidence order of 2,000 at `rates` (metric ids `ids`), naming `signer` (the carrier by default) for delivery."""
        signer = self.meter.pubkey() if signer is None else signer
        overrides = dict(source=signer, **({} if ids is None else dict(metric_ids=ids)))
        listed = ascending(*(term(n, v) for n, v in pairs), *reserved) if listed is None else listed
        self.rates, self.order_mandate, self.agent_index = rates, mandate, index
        try:
            tab = self.evidence_order(lambda: self.evidence_tab(name, cap=CAP, rates=rates,
                policy_overrides=overrides), listed, None)
        finally:
            self.rates, self.order_mandate, self.agent_index = None, None, 0
        tab["terms"]["mandate_revision"] = revision
        self.check_tariff(tab, rates, ids)
        return tab

    def run_order(self, name, *, price, revision, mandate, index=0, pairs=SELLER_TERMS, reserved=(), namespace=None, unit=None):
        """An order through accept_pinned_financed_tab of 100,000 at `price` per completed-run unit."""
        tab = self.acceptance(name, cap=ORDER, maximum_interest=0, fees=False, index=index, mandate=mandate)
        tab["terms"].update(expires_at=NOW + PERMISSION_SECONDS, mandate_revision=revision)
        tab["unit_price"] = tab["args"]["unit_price"] = price
        if namespace is not None:
            tab["namespace"] = tab["args"]["namespace"] = namespace
        if unit is not None:
            tab["service_unit"] = tab["args"]["service_unit"] = unit
        return self.set_terms(tab, ascending(*(term(n, v) for n, v in pairs), *reserved))

    def evidence_instruction(self, tab, order_terms=None):
        shown = tab["order_terms"] if order_terms is None else order_terms
        return self.ix("claims_core", "accept_pinned_evidence_financed_tab", tab["evidence_accounts"],
            remaining=self.roles_of(tab), order_terms=shown, **tab["evidence_args"])

    def run_instruction(self, tab, order_terms=None):
        shown = tab["order_terms"] if order_terms is None else order_terms
        args = dict(tab["args"])
        return self.ix("claims_core", "accept_pinned_financed_tab", tab["accounts"], remaining=self.roles_of(tab),
            order_terms=shown, **args)


def interface(f):
    names = {error["name"]: error["code"] for error in f.registry.value["errors"]}
    f.h.check("error numbers written in this module equal the built Registry interface",
        {name: names.get(name) for name in REGISTRY_ERRORS}, REGISTRY_ERRORS)
    names = {error["name"]: error["code"] for error in f.core.value["errors"]}
    f.h.check("error numbers written in this module equal the built ClaimsCore interface",
        {name: names.get(name) for name in CORE_ERRORS}, CORE_ERRORS)
    shared = json.loads(SHARED_VECTORS.read_text())
    for key, fingerprint in (("billing_tariff", BILLING_TARIFF), ("unit_price", UNIT_PRICE)):
        f.h.check(f"the reserved name {key} equals the shared vectors", (shared[key]["name"], shared[key]["name_fingerprint"]),
            (key, fingerprint.hex()))
    for vector in shared["billing_tariff"]["vectors"]:
        policy = dict(vector, metric_ids=[bytes.fromhex(x) for x in vector["metric_ids"]],
            **{k: [int(x) for x in vector[k]] for k in ("price_numerators", "price_denominators", "max_units")})
        f.h.check("shared billing tariff vector: " + vector["name"], tariff_hash(policy).hex(), vector["value"])
    for vector in shared["unit_price"]["vectors"]:
        f.h.check("shared unit price vector: " + vector["name"], unit_price_term(bytes.fromhex(vector["namespace"]),
            bytes.fromhex(vector["unit"]), int(vector["unit_price"]))["value"].hex(), vector["value"])
    f.h.check("a tariff at 2 per unit hashes differently from 1 per unit",
        tariff_term(AT_1)["value"] == tariff_term(AT_2)["value"], False)


def cast(f):
    parties = f.parties
    f.roles = {"buyer's owner": parties["company"]["owner"], "agent 1": f.agent, "agent 2": f.agent2,
        "seller": parties["supplier"]["owner"], "carrier": f.meter,
        "lender's owner": parties["financier"]["owner"], "credit issuer's owner": parties["issuer"]["owner"],
        "fee payer of setup and order transactions": f.h.payer}
    keys = [str(holder.pubkey()) for holder in f.roles.values()]
    f.h.check("all eight roles are held by different keys", len(set(keys)), len(keys))


def evidence_price(f):
    """The owner pins the carrier, size = 10 and the tariff of 1 per unit on agent 1's permission."""
    carrier = f.meter.pubkey()
    pin = ascending(term("size", "10"), signer_term(carrier), tariff_term(AT_1))
    f.permission = f.create_pinned("agent-1", 0, pin, "ACCEPTED: the buyer's owner creates agent 1's permission pinned to size = 10, "
        "delivery_signer = the carrier and billing_tariff = 1 per unit, at most 2,000 units", case="price")
    f.h.check("agent 1's permission holds the three terms at revision 1", f.pinned(f.permission), dict(revision=1, pinned=pin, active=True))

    def order(rates=AT_1, listed_rates=None, ids=None, **options):
        listed_rates = rates if listed_rates is None else listed_rates
        return f.priced_order("order-1", revision=1, mandate=f.permission, rates=rates, ids=ids,
            reserved=options.pop("reserved", (signer_term(carrier), tariff_term(listed_rates, ids))), **options)
    f.place_delivery(order(rates=AT_2), "refused: evidence order 1 at 2 per unit, size = 10, naming the carrier; "
        "the owner pinned 1 per unit", case="price", what_changed="tariff: 2 per unit instead of 1",
        refuse=refusal("TermsNotPermitted", "pinned term"))
    f.place_delivery(order(rates=AT_2, listed_rates=AT_1), "refused: evidence order 1 at 2 per unit whose list says the pinned "
        "tariff of 1 per unit", case="price", what_changed="the policy's tariff: 2 per unit, while the list says 1",
        refuse=refusal("PriceMismatch", "price"))
    other_metric = [ident("metric-other")] + [ZERO] * 7
    for options, what, changed_part in (
            (dict(rates=((1, 2, CAP),)), "priced 1/2 per unit (denominator 2)", "tariff denominator: 2 instead of 1"),
            (dict(rates=((1, 1, CAP - 1),)), "for at most 1,999 units", "tariff maximum units: 1,999 instead of 2,000"),
            (dict(ids=other_metric), "metering a different metric id", "tariff metric id"),
            (dict(rates=((1, 1, CAP), (1, 1, 1))), "with a second metric", "tariff metric count: 2 instead of 1")):
        f.place_delivery(order(**options), f"refused: evidence order 1 {what}, listing its own tariff; the owner pinned "
            "1 per unit for at most 2,000 units of one metric", case="price", what_changed=changed_part,
            refuse=refusal("TermsNotPermitted", "pinned term"))
    f.place_delivery(order(reserved=(signer_term(carrier),)), "refused: evidence order 1 at 1 per unit whose list leaves out "
        "the billing_tariff term the owner pinned", case="price", what_changed="the list does not carry the billing_tariff term",
        refuse=refusal("TermsNotPermitted", "pinned term"))
    f.place_delivery(order(reserved=(signer_term(carrier), tariff_term(AT_1), unit_price_term(f.tabs_namespace(), f.tabs_unit(), 1))),
        "refused: evidence order 1 whose list also carries a unit_price term; an evidence order has a tariff, not a unit price",
        case="price", what_changed="the list carries a unit_price term", refuse=refusal("PriceMismatch", "price"))
    o1 = order()
    row = f.place_delivery(o1, "ACCEPTED: evidence order 1, maximum 2,000, at 1 per unit, size = 10, naming the carrier", case="price")
    f.ran(row, 1, "AcceptPinnedEvidenceFinancedTab", "ReservePinnedBilling")
    f.check_accepted_bill(o1, 1, carrier)
    f.h.check("order 1: the bill's policy prices 1 per unit for at most 2,000 units",
        [f.bill_state(o1)["policy"][k][0] for k in ("price_numerators", "price_denominators", "max_units")], [1, 1, CAP])
    return o1


def evidence_retries(f, o1):
    """Case: a retry of accepted evidence order 1 shows the list it was accepted with, or is refused."""
    carrier = f.meter.pubkey()
    row = f.retry("no effect: accepted evidence order 1 is sent again with the list it was accepted with",
        f.evidence_instruction(o1), o1, case="retry", what_changed="an accepted order is sent again")
    f.h.check("the retry with the accepted list changes no observed account", row["observed_after"], row["observed_before"])
    other = ascending(*(term(n, v) for n, v in SELLER_TERMS + (("note", "gift"),)), signer_term(carrier), tariff_term(AT_1))
    f.retry("refused: accepted evidence order 1 is sent again with a list that adds note = gift", f.evidence_instruction(o1, other),
        o1, case="retry", what_changed="list shown: a field added", refuse=refusal("OrderTermsMismatch", "retried list"))
    breaking = ascending(*(term(n, v) for n, v in changed(SELLER_TERMS, size="11")), signer_term(carrier), tariff_term(AT_1))
    f.retry("refused: accepted evidence order 1 is sent again with a list that says size = 11, which the owner's pin refuses",
        f.evidence_instruction(o1, breaking), o1, case="retry", what_changed="list shown: size = 11",
        refuse=refusal("OrderTermsMismatch", "retried list"))
    plain = f.ix("claims_core", "accept_evidence_financed_tab", o1["evidence_accounts"], remaining=f.roles_of(o1), **o1["evidence_args"])
    f.retry("no effect: accepted evidence order 1 is sent again through plain accept_evidence_financed_tab", plain, o1,
        case="retry", what_changed="plain instruction, no list shown")


def run_price(f):
    """The owner pins size = 10 and 1 per completed-run unit on agent 2's permission."""
    namespace, unit = f.tabs_namespace(), f.tabs_unit()
    pin = ascending(term("size", "10"), unit_price_term(namespace, unit, 1))
    f.second = f.create_pinned("agent-2", 1, pin, "ACCEPTED: the buyer's owner creates agent 2's permission pinned to size = 10 "
        "and unit_price = 1 per completed-run unit", case="unit price")

    def order(price=1, listed_price=None, **options):
        listed_price = price if listed_price is None else listed_price
        own = unit_price_term(options.get("namespace") or namespace, options.get("unit") or unit, listed_price)
        return f.run_order("order-2", price=price, revision=1, index=1, mandate=f.second,
            reserved=options.pop("reserved", (own,)), **options)
    f.place(order(price=2), "refused: agent 2 submits order 2 (accept_pinned_financed_tab) at 2 per unit; the owner pinned 1",
        case="unit price", what_changed="unit price: 2 instead of 1", refuse=refusal("TermsNotPermitted", "pinned term"))
    f.place(order(price=2, listed_price=1), "refused: order 2 at 2 per unit whose list says the pinned unit_price of 1",
        case="unit price", what_changed="the order's unit price: 2, while the list says 1", refuse=refusal("PriceMismatch", "price"))
    f.place(order(namespace=ident("another-service")), "refused: order 2 at 1 per completed-run unit of another of the "
        "seller's services (namespace), listing its own unit_price", case="unit price",
        what_changed="namespace: another service", refuse=refusal("TermsNotPermitted", "pinned term"))
    f.place(order(unit=ident("another-unit")), "refused: order 2 at 1 per unit of a different unit, listing its own unit_price",
        case="unit price", what_changed="unit: another unit", refuse=refusal("TermsNotPermitted", "pinned term"))
    f.place(order(reserved=(unit_price_term(namespace, unit, 1), tariff_term(AT_1))), "refused: order 2 whose list also carries a "
        "billing_tariff term; an order through accept_pinned_financed_tab has a unit price, not a tariff",
        case="unit price", what_changed="the list carries a billing_tariff term", refuse=refusal("PriceMismatch", "price"))
    o2 = order()
    row = f.place(o2, "ACCEPTED: order 2, maximum 100,000, at 1 per completed-run unit, size = 10", case="unit price")
    f.accepted(o2, row, 2, shown=True, held=CAP + ORDER)
    claim = f.state("FundedClaim", o2["claim"])
    f.h.check("order 2: the order record prices 1 per unit of the pinned unit", (claim["unit_price"], claim["service_unit"]), (1, unit))

    row = f.retry("no effect: accepted order 2 is sent again with the list it was accepted with", f.run_instruction(o2), o2,
        case="retry", what_changed="an accepted order is sent again")
    f.h.check("the retry with the accepted list changes no observed account", row["observed_after"], row["observed_before"])
    other = ascending(*(term(n, v) for n, v in SELLER_TERMS + (("note", "gift"),)), unit_price_term(namespace, unit, 1))
    f.retry("refused: accepted order 2 is sent again with a list that adds note = gift", f.run_instruction(o2, other), o2,
        case="retry", what_changed="list shown: a field added", refuse=refusal("OrderTermsMismatch", "retried list"))
    breaking = ascending(*(term(n, v) for n, v in changed(SELLER_TERMS, size="11")), unit_price_term(namespace, unit, 1))
    f.retry("refused: accepted order 2 is sent again with a list that says size = 11, which the owner's pin refuses",
        f.run_instruction(o2, breaking), o2, case="retry", what_changed="list shown: size = 11",
        refuse=refusal("OrderTermsMismatch", "retried list"))
    f.retry("no effect: accepted order 2 is sent again through plain accept_financed_tab", f.commit_instruction(o2), o2,
        case="retry", what_changed="plain instruction, no list shown")
    return o2


def without_signer_term(f):
    """Evidence orders through the pinned instruction whose list has no delivery_signer term."""
    size_only = f.create_pinned("agent-1-size", 0, [term("size", "10")],
        "ACCEPTED: the buyer's owner creates a second permission for agent 1 pinned to size = 10 only", case="no signer term")
    o3 = f.priced_order("order-3", revision=1, mandate=size_only)
    f.h.check("order 3's list has no delivery_signer term", any(t["name"] == DELIVERY_SIGNER for t in o3["order_terms"]), False)
    row = f.place_delivery(o3, "ACCEPTED: evidence order 3 through the pinned evidence instruction without a delivery_signer term; "
        "the permission pins size = 10 only", case="no signer term")
    f.check_accepted_bill(o3, 3, f.meter.pubkey())
    unpinned = f.permit("agent-2-unpinned", 1)
    o4 = f.priced_order("order-4", revision=1, mandate=unpinned, index=1)
    f.place_delivery(o4, "ACCEPTED: agent 2's evidence order 4 through the pinned evidence instruction without a delivery_signer "
        "term; the permission pins nothing", case="no signer term")
    f.check_accepted_bill(o4, 4, f.meter.pubkey())
    return unpinned


def existing_bill(f, unpinned):
    """A pinned instruction against a bill that a plain-path order already wrote."""
    f.rates, f.order_mandate, f.agent_index = AT_1, unpinned, 1
    try:
        o5 = f.evidence_tab("order-5", cap=CAP, rates=AT_1)
    finally:
        f.rates, f.order_mandate, f.agent_index = None, None, 0
    f.place_delivery(o5, "ACCEPTED: agent 2's evidence order 5 through plain accept_evidence_financed_tab; its policy's "
        "request_hash is the evidence fixture's request identity", case="existing bill", shown=False)
    listed = ascending(*(term(n, v) for n, v in SELLER_TERMS))
    f.h.check("order 5's request_hash is not the hash of the list shown next", order_terms_hash(listed) == o5["billing_policy"]["request_hash"], False)
    f.retry("refused: order 5 is sent again through the pinned evidence instruction with a list; its bill's request_hash "
        "is not that list's hash", f.evidence_instruction(o5, listed), o5, case="existing bill",
        what_changed="pinned instruction against a bill written by the plain instruction",
        refuse=refusal("OrderTermsMismatch", "retried list"))
    o6 = f.priced_order("order-6", revision=1, mandate=unpinned, index=1)
    f.place_delivery(o6, "ACCEPTED: agent 2's evidence order 6 through plain accept_evidence_financed_tab; its policy's "
        "request_hash is the hash of its list", case="existing bill", shown=False)
    row = f.retry("no effect: order 6 is sent again through the pinned evidence instruction with that list",
        f.evidence_instruction(o6), o6, case="existing bill", what_changed="pinned instruction against a bill written by the plain instruction")
    f.h.check("the pinned retry of order 6 changes no observed account", row["observed_after"], row["observed_before"])


def plain_run_order(f, unpinned):
    """A pinned retry of an order that was accepted through plain accept_financed_tab."""
    own = unit_price_term(f.tabs_namespace(), f.tabs_unit(), 1)
    o7 = f.run_order("order-7", price=1, revision=1, index=1, mandate=unpinned, reserved=(own,))
    row = f.place(o7, "ACCEPTED: agent 2's order 7, maximum 100,000, through plain accept_financed_tab; its external contract "
        "hash is the hash of its list", case="plain then pinned", shown=False)
    f.accepted(o7, row, 7, shown=False, held=5 * CAP + 2 * ORDER)
    other = ascending(*(term(n, v) for n, v in changed(SELLER_TERMS, size="11")), own)
    f.retry("refused: order 7 is sent again through accept_pinned_financed_tab with a list that says size = 11",
        f.run_instruction(o7, other), o7, case="plain then pinned", what_changed="list shown: not the order's own",
        refuse=refusal("OrderTermsMismatch", "retried list"))
    row = f.retry("no effect: order 7 is sent again through accept_pinned_financed_tab with its own list",
        f.run_instruction(o7), o7, case="plain then pinned", what_changed="pinned instruction against an order accepted plain")
    f.h.check("the pinned retry of order 7 changes no observed account", row["observed_after"], row["observed_before"])


def end_state(f):
    budget = f.registry.read(f.h.svm, f.budget, "Budget")
    f.h.check("budget at the end: evidence orders 1, 3, 4, 5 and 6 of 2,000 and orders 2 and 7 of 100,000 held",
        (budget["reserved"], budget["consumed"]), (5 * CAP + 2 * ORDER, 0))
    refused = [row for row in f.h.cases if row["expected"] == "reject"]
    f.h.check("every refusal in this run was annotated and checked",
        all(row.get("commit_legs", {}).get("outcome") == "refused" for row in refused), True)


def exercise(f):
    setup(f)
    interface(f)
    cast(f)
    o1 = evidence_price(f)
    evidence_retries(f, o1)
    run_price(f)
    unpinned = without_signer_term(f)
    existing_bill(f, unpinned)
    plain_run_order(f, unpinned)
    end_state(f)


PinnedPrice.tabs_unit = lambda self: ident("completed-inference-unit")
PinnedPrice.tabs_namespace = lambda self: ident("provider-namespace")


class RecoverablePrice(Priced, RecoverableDelivery):
    """A pinned recoverable order through report, claim, finalization and payment, with a source owner rotation."""

    def recoverable_priced(self, name, *, mandate, revision, rates=AT_1, pairs=SELLER_TERMS, listed_signer=None, reserved=None):
        source = self.parties["assignee"]["principal"]
        signer = source if listed_signer is None else listed_signer
        listed = ascending(*(term(n, v) for n, v in pairs), signer_term(signer), *(reserved or ()))
        self.rates = rates
        try:
            tab = self.evidence_order(lambda: self.recoverable_tab(name, mandate=mandate, cap=CAP), listed, None)
        finally:
            self.rates = None
        tab["terms"]["mandate_revision"] = revision
        self.check_tariff(tab, rates)
        return tab

    def recoverable_instruction(self, tab, order_terms):
        """accept_pinned_recoverable_financed_tab in its exact generated account order, showing `order_terms`."""
        definition = self.core.instructions["accept_pinned_recoverable_financed_tab"]
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
        args = dict(tab["evidence_args"], order_terms=order_terms)
        data = bytes(definition["discriminator"]) + b"".join(self.core.encode(a["type"], args[a["name"]]) for a in definition["args"])
        return Instruction(CORE, data, [*metas, *self.remaining(["company", "issuer", "supplier", tab["financier"]])])


def recoverable(f):
    f.setup(financier_wallet=True)
    buyer, seller, source = f.parties["company"], f.parties["supplier"], f.parties["assignee"]
    f.receiving_open("supplier")
    principal = source["principal"]

    # An unpinned permission: a delivery_signer term other than the source principal is refused.
    unpinned = f.controlled_mandate("price-unpinned")
    f.place_recoverable(f.recoverable_priced("order-1-recoverable", mandate=unpinned, revision=1,
        listed_signer=source["owner"].pubkey()), "refused: recoverable order 1 on an unpinned permission whose delivery_signer "
        "term is the source principal's owner key, not the principal", case="recoverable",
        what_changed="delivery_signer value: the owner's key instead of the source principal",
        refuse=refusal("DeliverySignerMismatch", "delivery signer"))
    o1 = f.recoverable_priced("order-1-recoverable", mandate=unpinned, revision=1)
    f.place_recoverable(o1, "ACCEPTED: recoverable order 1, maximum 2,000, naming the source principal, on the unpinned permission",
        case="recoverable")

    # A pinned permission: size = 10, the source principal and 1 per unit.
    pinned = f.controlled_mandate("price-pinned")
    pin = ascending(term("size", "10"), signer_term(principal), tariff_term(AT_1))
    f.send("ACCEPTED: the buyer's owner pins size = 10, delivery_signer = the source principal and billing_tariff = 1 per unit",
        pin_instruction(f.registry, buyer["owner"].pubkey(), f.company, pinned, pin, 1), [buyer["owner"]])
    f.mark("recoverable", "pin", "nothing")
    f.place_recoverable(f.recoverable_priced("order-2-recoverable", mandate=pinned, revision=2, rates=AT_2,
        reserved=(tariff_term(AT_2),)), "refused: recoverable order 2 at 2 per unit; the owner pinned 1 per unit",
        case="recoverable", what_changed="tariff: 2 per unit instead of 1", refuse=refusal("TermsNotPermitted", "pinned term"))
    f.place_recoverable(f.recoverable_priced("order-2-recoverable", mandate=pinned, revision=2, rates=AT_2,
        reserved=(tariff_term(AT_1),)), "refused: recoverable order 2 at 2 per unit whose list says the pinned tariff of 1 per unit",
        case="recoverable", what_changed="the policy's tariff: 2 per unit, while the list says 1", refuse=refusal("PriceMismatch", "price"))
    o2 = f.recoverable_priced("order-2-recoverable", mandate=pinned, revision=2, reserved=(tariff_term(AT_1),))
    f.place_recoverable(o2, "ACCEPTED: recoverable order 2, maximum 2,000, at 1 per unit, size = 10, naming the source principal",
        case="recoverable")
    f.check_accepted_bill(o2, 2, principal)
    breaking = ascending(*(term(n, v) for n, v in changed(SELLER_TERMS, size="11")), signer_term(principal), tariff_term(AT_1))
    f.retry("refused: accepted recoverable order 2 is sent again with a list that says size = 11",
        f.recoverable_instruction(o2, breaking), o2, case="recoverable", what_changed="list shown: not the order's own",
        refuse=refusal("OrderTermsMismatch", "retried list"))
    row = f.retry("no effect: accepted recoverable order 2 is sent again with its own list",
        f.recoverable_instruction(o2, o2["order_terms"]), o2, case="recoverable", what_changed="an accepted order is sent again")
    f.h.check("the retry with its own list changes no observed account", row["observed_after"], row["observed_before"])

    # The source principal's owner rotates its key; the order still names the principal.
    old_owner, replacement = source["owner"], key(170)
    f.h.svm.airdrop(replacement.pubkey(), 100_000_000_000)
    f.rotate("assignee", replacement, "price-run")
    f.h.cases[-1]["name"] = "ACCEPTED: the source principal's owner rotates to a new key"
    f.mark("recoverable", "owner rotation", "nothing")
    f.h.check("the source principal is now owned by the new key at generation 2",
        (f.principal("assignee")["owner"], f.principal("assignee")["authority_generation"]), (replacement.pubkey(), 2))
    report = f.report(o2, units=(SIGNED,))
    f.h.check("the report carries order 2's terms hash as its request_hash", report["request_hash"], order_terms_hash(o2["order_terms"]))
    f.principal_record(o2, report, source=True, owner=old_owner, generation=1, error="BillingAuthorityGeneration")
    f.h.cases[-1]["name"] = "refused: the source principal's former owner key signs the report of 900 on order 2"
    f.mark("recoverable", "report", "signed by the principal's former owner key",
        refuse=refusal("BillingAuthorityGeneration", "source generation", index=1))
    f.principal_record(o2, report, source=True)
    f.h.cases[-1]["name"] = "ACCEPTED: the source principal's current owner signs 900 units delivered on order 2"
    f.mark("recoverable", "report", "nothing")
    f.principal_record(o2, f.monetary_claim(o2, CLAIMED))
    f.h.cases[-1]["name"] = "ACCEPTED: the seller signs a claim of 1,400 on order 2"
    f.mark("recoverable", "claim", "nothing")
    f.at(46)
    f.finalize(o2, label="ACCEPTED: an outside key finalizes order 2 at 46 seconds: the source signed 900, the seller claimed 1,400")
    f.mark("recoverable", "finalization", "nothing")
    f.assert_final_amount(o2, SIGNED)
    before = f.h.amount(seller["receiving_token"])
    f.collect_from(o2, SIGNED, label="ACCEPTED: an outside key collects order 2's bill of 900 to the seller's receiving custody")
    f.mark("recoverable", "collection", "nothing")
    f.h.check("the seller's receiving custody receives exactly 900", f.h.amount(seller["receiving_token"]) - before, SIGNED)
    claim = f.state("FundedClaim", o2["claim"])
    for field, value in (("paid", SIGNED), ("payable", 0), ("debt", SIGNED), ("released", CAP - SIGNED), ("remaining", 0)):
        f.h.check("order 2 record " + field + " after collection", claim[field], value)
    f.roles = {"buyer's owner": buyer["owner"], "agent": f.agent, "seller's owner": seller["owner"],
        "source principal's first owner": old_owner, "source principal's owner after rotation": replacement}


RUNS = (("pinned-price", PinnedPrice, exercise), ("pinned-price-recoverable", RecoverablePrice, recoverable))


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
            row["pinned_price"] = row.pop("commit_legs")
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
        name_fingerprints={name: name_print(name).hex() for name in ("size", "delivery_signer", "billing_tariff", "unit_price")},
        tariff_values={"1 per unit": tariff_term(AT_1)["value"].hex(), "2 per unit": tariff_term(AT_2)["value"].hex()},
        unit_price_values={str(p): unit_price_term(ident("provider-namespace"), ident("completed-inference-unit"), p)["value"].hex()
            for p in (1, 2)},
        program_sha256={name: value["sha256"] for name, value in f.build["artifacts"].items()},
        swig_sha256=SWIG_SHA256, build_receipt_sha256=digest(OUT / "build-receipt.json"),
        idl_receipt_sha256=digest(OUT / "idl-receipt.json"))
    (directory / "pinned-price-receipt.json").write_text(json.dumps(manifest, indent=2) + "\n")
    return manifest


def main():
    started = time.monotonic()
    results = [run(*item) for item in RUNS]
    if "--record" in sys.argv:
        print(json.dumps(record(results, sys.argv[sys.argv.index("--record") + 1], time.monotonic() - started)))


if __name__ == "__main__":
    main()
