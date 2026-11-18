#!/usr/bin/env python3
"""Payment waits for the signer both sides named.

Orders here are accepted with `accept_evidence_financed_tab`. Each carries a
billing policy that names one delivery signer (`policy.source`). Only that
key's signed report says what was delivered, the seller signs its own claim,
and after both windows close anyone can finalize the bill.

One scenario. Every order has a maximum of 2,000 at a fixed price of 1 per
unit, for the same goods. Work must finish within 20 seconds of acceptance, the
named signer may report until 40 seconds and the seller may claim until 45.

  order 1  signer signs 900, seller claims 1,400
  order 2  signer never signs, seller claims 1,400
  order 3  signer signs 900, seller claims 500
  order 4  signer signs one second late, seller claims 1,400
  order 5  the order names the buyer's owner as the signer; it signs 900
  order 6  signer signs in the last second of its window, seller claims 1,400

In orders 1 to 4 and 6 the named signer is a key with no other role. The trace
asserts it is not the buyer's owner, the agent, the seller, the lender's owner,
the credit issuer's owner or any fee payer.

This reuses the setup and helpers of evidence_billing.py and the refusal checks
of commit_legs.py: transaction error, first failing program frame, Anchor error
name and number from that frame's log, and byte-identical state for every
writable account except the fee payer. No account bytes are written by the
harness. The harness sets the clock to 40, 41, 45 and 46 seconds.

Record: <python> foundation/onchain/tests/named_signer.py --record docs/evidence/<directory>
"""
from commit_legs import *
from evidence_billing import *

CAP = 2_000
SIGNED = 900
GOODS = ident("one-box-of-shoes-specification")


class NamedSigner(CommitLegs, EvidenceBillingFixture):
    def error(self, name, index=0):
        code = next(item["code"] for item in self.core.value["errors"] if item["name"] == name)
        return dict(program=CORE, name=name, code=code, index=index)

    def mark(self, part, changed, *, refuse=None, **extra):
        return self.note(section=part, leg="delivery", changed=changed, refuse=refuse, **extra)

    def shoe_order(self, name, *, signer=None):
        overrides = dict(request_hash=GOODS)
        if signer is not None:
            overrides["source"] = signer
        return self.evidence_tab(name, cap=CAP, rates=((1, 1, CAP),), policy_overrides=overrides)

    def bill_amounts(self, tab):
        bill = self.bill_state(tab)
        return dict(signed=bill["supported_amount"], bill=bill["established_amount"],
            released=bill["released_amount"], finalized=bill["finalized"])


def cast(f):
    parties = f.parties
    f.roles = {
        "buyer's owner": parties["company"]["owner"], "buyer's agent": f.agent,
        "seller": parties["supplier"]["owner"], "lender's owner": parties["financier"]["owner"],
        "credit issuer's owner": parties["issuer"]["owner"],
        "fee payer of setup and order transactions": f.h.payer,
        "named delivery signer (orders 1, 2, 3, 4, 6)": f.meter,
        "key that submits signed records": f.submitter, "key that finalizes bills": f.finalizer,
        "key that collects": f.collector, "key no order names (refusals)": f.other_meter}
    signer = f.meter.pubkey()
    for role, holder in f.roles.items():
        if holder is not f.meter:
            f.h.check("named delivery signer is not the " + role, signer == holder.pubkey(), False)
    keys = [str(holder.pubkey()) for holder in f.roles.values()]
    f.h.check("all eleven roles are held by different keys", len(set(keys)), len(keys))


def accept_orders(f):
    buyer, seller = f.parties["company"], f.parties["supplier"]
    orders = {}
    for number in (1, 2, 3, 4, 6):
        orders[number] = f.shoe_order(f"order-{number}")
    orders[5] = f.shoe_order("order-5", signer=buyer["owner"].pubkey())
    for number in sorted(orders):
        tab = orders[number]
        named = "the buyer's owner" if number == 5 else "the delivery signer"
        f.accept_evidence(tab, label=f"ACCEPTED: order {number}, maximum 2,000 at price 1 per unit, naming {named}; "
            "signed by the agent, the seller and the credit issuer's owner")
        row = f.mark("1", "nothing")
        bill = f.bill_state(tab)
        expected = buyer["owner"].pubkey() if number == 5 else f.meter.pubkey()
        f.h.check(f"order {number}: the bill account records the named signer's key", bill["policy"]["source"], expected)
        f.h.check(f"order {number}: the bill account records the seller as the only claimant",
            bill["supplier_claimant"], seller["owner"].pubkey())
        claim = f.state("FundedClaim", tab["claim"])
        f.h.check(f"order {number}: the Tab record commits to the policy that names the signer",
            claim["external_contract_hash"], tab["policy_hash"])
        f.h.check(f"order {number}: the Tab record is bound to the bill profile",
            claim["usage_namespace"], BILLING_NAMESPACE)
        reservation = f.registry.read(f.h.svm, tab["reservation"], "Reservation")
        f.h.check(f"order {number}: the Registry reservation commits to the same policy",
            reservation["terms"]["external_terms_hash"], tab["terms"]["external_terms_hash"])
        other = dict(tab["billing_policy"], source=f.other_meter.pubkey())
        f.h.check(f"order {number}: naming a different signer would give a different policy hash",
            phase_digest(POLICY_DOMAIN, f.domain, tab["bill"], tab["claim"], policy_bytes(other)) == tab["policy_hash"], False)
        if number != 5:
            f.h.check(f"order {number}: the named signer neither signs nor appears as an account when the order is accepted",
                str(f.meter.pubkey()) in row["account_keys"], False)
    f.h.check("no order has a bill or a debt at acceptance",
        sum(f.state("FundedClaim", tab["claim"])["debt"] for tab in orders.values()), 0)
    f.h.check("lender has promised six times 2,000", f.lender_cash()["promised"], 6 * CAP)
    return orders


def reports_and_claims(f, orders):
    buyer, seller = f.parties["company"], f.parties["supplier"]
    o1, o3 = orders[1], orders[3]
    signature = f.error("BillingSignature", 1)

    f.earn(o1, "seller-reports-own-delivery", SIGNED, reject=True, error="BillingProfile",
        label="refused: the seller reports 900 delivered on order 1 through the seller-only path (earn_financed_run)")
    f.mark("2", "seller reports its own delivery instead of the named signer", refuse=f.error("BillingProfile"))

    report3 = f.report(o3, units=(SIGNED,))
    signed3 = f.submit_report(o3, report3, label="ACCEPTED: the named signer signs 900 units delivered on order 3")
    f.mark("3", "nothing")
    report1 = f.report(o1, units=(SIGNED,))
    for holder, who in ((f.other_meter, "a key no order names"), (seller["owner"], "the seller"),
            (buyer["owner"], "the buyer's owner, whom order 1 does not name,")):
        f.submit_report(o1, report1, source=holder, error="BillingSignature",
            label=f"refused: {who} signs the report of 900 units delivered on order 1")
        f.mark("3", "report signed by a key other than the one the order names", refuse=signature)
    f.submit_report(o1, report3, proof=signed3[0], error="BillingSignature",
        label="refused: the named signer's signed report for order 3 is presented on order 1")
    f.mark("3", "report was signed for a different order", refuse=signature)
    f.submit_report(o1, report1, label="ACCEPTED: the named signer signs 900 units delivered on order 1")
    f.mark("4", "nothing")
    f.submit_report(orders[5], f.report(orders[5], units=(SIGNED,)), source=buyer["owner"],
        label="ACCEPTED: the buyer's owner, named by order 5, signs 900 units delivered on order 5")
    f.mark("9", "nothing")

    for number, amount in ((1, 1_400), (2, 1_400), (3, 500), (4, 1_400), (5, 1_400), (6, 1_400)):
        f.submit_claim(orders[number], f.monetary_claim(orders[number], amount),
            label=f"ACCEPTED: the seller signs a claim of {amount:,} on order {number}")
        f.mark({1: "4", 2: "6", 3: "7", 4: "8", 5: "9", 6: "8"}[number], "nothing")
    for number in orders:
        claim = f.state("FundedClaim", orders[number]["claim"])
        f.h.check(f"order {number}: signed records alone create no debt and nothing payable",
            (claim["debt"], claim["payable"]), (0, 0))

    f.finalize(o1, error="BillingFinality",
        label="refused: finalization of order 1 at 0 seconds, while the signer's and the seller's windows are open")
    f.mark("5", "clock, 46 seconds before finalization opens", refuse=f.error("BillingFinality"))


def deadlines(f, orders):
    f.at(40)
    f.submit_report(orders[6], f.report(orders[6], units=(SIGNED,)),
        label="ACCEPTED: the named signer signs 900 units on order 6 at 40 seconds, the last second of its window")
    f.mark("8", "nothing")
    f.at(41)
    f.submit_report(orders[4], f.report(orders[4], units=(SIGNED,)), error="BillingDeadline",
        label="refused: the named signer signs 900 units on order 4 at 41 seconds, one second after its window closed")
    f.mark("8", "clock, one second past the signer's deadline", refuse=f.error("BillingDeadline", 1))
    f.at(45)
    f.finalize(orders[1], error="BillingFinality",
        label="refused: finalization of order 1 at 45 seconds, the last second of the seller's claim window")
    f.mark("5", "clock, one second before finalization opens", refuse=f.error("BillingFinality"))
    f.at(46)


def bills(f, orders):
    seller, lender = f.parties["supplier"], f.parties["financier"]
    seller_ata = f.h.ata(seller["owner"].pubkey(), f.mint)
    expected = {1: (SIGNED, 1_400, 900), 2: (0, 1_400, 0), 3: (SIGNED, 500, 500),
        4: (0, 1_400, 0), 5: (SIGNED, 1_400, 900), 6: (SIGNED, 1_400, 900)}
    reasons = {1: "the signer signed 900 and the seller claimed 1,400: bill 900, 1,100 released",
        2: "the signer never signed and the seller claimed 1,400: bill 0, all 2,000 released",
        3: "the signer signed 900 and the seller claimed 500: bill 500, 1,500 released",
        4: "the signer's only report was one second late and the seller claimed 1,400: bill 0, all 2,000 released",
        5: "the buyer's owner signed 900 as the named signer and the seller claimed 1,400: bill 900, 1,100 released",
        6: "the signer signed 900 in its last second and the seller claimed 1,400: bill 900, 1,100 released"}
    parts = {1: "4", 2: "6", 3: "7", 4: "8", 5: "9", 6: "8"}
    for number in sorted(orders):
        tab = orders[number]
        signed, claimed, amount = expected[number]
        f.h.check(f"order {number}: bill expected from the smallest of maximum {CAP:,}, claim {claimed:,} and signed {signed:,}",
            min(CAP, claimed, signed), amount)
        f.finalize(tab, label=f"ACCEPTED: an outside key finalizes order {number} at 46 seconds; {reasons[number]}")
        row = f.mark(parts[number], "nothing")
        f.assert_final_amount(tab, amount)
        state = f.bill_amounts(tab)
        f.h.check(f"order {number}: amount the named signer's report supports", state["signed"], signed)
        f.h.check(f"order {number}: bill", state["bill"], amount)
        f.h.check(f"order {number}: released back to the buyer's budget and the lender's free cash", state["released"], CAP - amount)
        f.h.check(f"order {number}: finalization is signed only by the outside key",
            row["account_keys"][0], str(f.finalizer.pubkey()))

    wallet_before, seller_before = f.h.amount(lender["custody_token"]), f.h.amount(seller_ata)
    f.h.check("the seller has received nothing before collection", seller_before, 0)
    f.collect_from(orders[2], 1_400, payer=f.collector, error="FundingRun",
        label="refused: collection of the 1,400 the seller claimed on order 2, which the named signer never signed for")
    f.mark("6", "no bill: the named signer never signed", refuse=f.error("FundingRun", 1))
    f.collect_from(orders[1], 901, payer=f.collector, error="FundingTerms",
        label="refused: collection of 901 on order 1, one unit above the bill of 900")
    f.mark("4", "amount, one unit above the bill", refuse=f.error("FundingTerms", 1))
    f.collect_from(orders[1], 900, payer=f.collector,
        label="ACCEPTED: an outside key collects order 1's bill of 900 from the lender wallet to the seller")
    f.mark("4", "nothing")
    f.h.check("the seller received exactly 900", f.h.amount(seller_ata) - seller_before, 900)
    f.h.check("the lender wallet paid exactly 900", wallet_before - f.h.amount(lender["custody_token"]), 900)
    claim = f.state("FundedClaim", orders[1]["claim"])
    for field, value in (("paid", 900), ("payable", 0), ("debt", 900), ("released", 1_100), ("remaining", 0)):
        f.h.check("order 1 Tab record " + field, claim[field], value)
    claim = f.state("FundedClaim", orders[2]["claim"])
    for field, value in (("paid", 0), ("payable", 0), ("debt", 0), ("released", CAP), ("remaining", 0)):
        f.h.check("order 2 Tab record " + field, claim[field], value)
    cash = f.lender_cash()
    f.h.check("lender still owes the uncollected bills of orders 3, 5 and 6", cash["promised"], 500 + 900 + 900)
    f.h.check("lender wallet holds its 1,000,000 less the 900 collected", cash["wallet"], 1_000_000 - 900)
    budget = f.registry.read(f.h.svm, f.budget, "Budget")
    f.h.check("buyer's budget consumed equals the four bills", budget["consumed"], 900 + 500 + 900 + 900)
    f.h.check("buyer's budget holds nothing further for these orders", budget["reserved"], 0)


def exercise(f):
    f.setup()
    cast(f)
    orders = accept_orders(f)
    reports_and_claims(f, orders)
    deadlines(f, orders)
    bills(f, orders)


def record(f, summary, index, directory, seconds):
    """Copy the finished run into a publication directory; no second execution."""
    directory = Path(directory).resolve()
    directory.mkdir(parents=True, exist_ok=True)
    raw = Path(summary["trace"]).read_bytes()
    packed = directory / "named-signer-trace.json.xz"
    packed.write_bytes(lzma.compress(raw))
    assert lzma.decompress(packed.read_bytes()) == raw
    (directory / "case-index.json").write_text(json.dumps(index, indent=1, default=json_value) + "\n")
    annotated = [row for row in index if row["section"] != "setup"]
    receipt = dict(summary, trace=packed.name, trace_xz_sha256=digest(packed),
        annotated_cases=len(annotated),
        annotated_refusals=sum(row["outcome"] == "refused" for row in annotated),
        annotated_acceptances=sum(row["outcome"] == "accepted" for row in annotated),
        run_seconds=round(seconds, 2), engine="solders 0.29.0 LiteSVM",
        roles={role: str(holder.pubkey()) for role, holder in f.roles.items()},
        program_sha256={name: value["sha256"] for name, value in f.build["artifacts"].items()},
        swig_sha256=SWIG_SHA256, build_receipt_sha256=digest(OUT / "build-receipt.json"),
        idl_receipt_sha256=digest(OUT / "idl-receipt.json"))
    for name in ("build-receipt.json", "idl-receipt.json"):
        (directory / name).write_bytes((OUT / name).read_bytes())
    (directory / "named-signer-receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    return receipt


def main():
    started = time.monotonic()
    encoding_vectors()
    f = NamedSigner()
    try:
        exercise(f)
    except Exception as error:
        f.finish("named-signer", error=error)
        raise
    index = case_index(f, "named-signer")
    for row in f.h.cases:
        if "commit_legs" in row:
            row["named_signer"] = row.pop("commit_legs")
    summary = f.finish("named-signer")
    if "--record" in sys.argv:
        print(json.dumps(record(f, summary, index, sys.argv[sys.argv.index("--record") + 1], time.monotonic() - started)))


if __name__ == "__main__":
    main()
