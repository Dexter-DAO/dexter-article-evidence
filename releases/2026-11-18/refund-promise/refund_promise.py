#!/usr/bin/env python3
"""A seller's refund promise, composed from two orders in opposite directions.

Nothing in the programs ties a refund to a purchase. This composes one from
existing parts and records whether each step is accepted:

  purchase        buyer's agent orders 100,000 from the seller, backed by a
                  lender, on the ordinary seller-reported path
  refund promise  the SELLER's agent orders up to 100,000 "from" the original
                  BUYER, backed by the seller's own cash in the seller's own
                  governed wallet, accepted with accept_evidence_financed_tab
                  naming a decider key as the signer. The amount the decider
                  signs for is what the original buyer can collect.

Two pairs are run at once. In pair 1 the decider awards 30,000. In pair 2 the
decider never signs.

  0 s   both purchases and both promises accepted; the seller is paid 100,000
        on each purchase; refusals while the promises are open
  30 s  the decider signs 30,000 on promise 1; the original buyer signs a claim
        of 100,000 on each promise
  46 s  both promises are finalized by an outside key; 30,000 is collected to
        the original buyer; the seller's remaining cash is free again

The shared fixture's buyer-side, seller-side and lender-side helpers are
pointed at the right parties for the promise by `promise()`: the seller's
principal is the buyer of record and the financier, the original buyer's
principal is the seller of record. No instruction or account is altered and no
account bytes are written by the harness. The harness sets the clock to 30 and
46 seconds.

Record: <python> foundation/onchain/tests/refund_promise.py --record docs/evidence/<directory>
"""
from commit_legs import *
from evidence_billing import *

PRICE = 100_000
AWARD = 30_000
SELLER_CASH = 200_000


class RefundPromise(CommitLegs, EvidenceBillingFixture):
    def __init__(self):
        super().__init__()
        self.seller_agent = key(67)

    @contextlib.contextmanager
    def promise(self):
        """Buyer of record and financier: the seller. Seller of record: the original buyer."""
        names = ("company", "budget", "actors", "agent", "facility", "line", "mandates")
        saved = {name: getattr(self, name) for name in names}
        parties = {role: self.parties[role] for role in ("company", "supplier", "financier")}
        self.company, self.budget = self.seller["principal"], self.seller_budget
        self.actors, self.agent = [self.seller_actor], self.seller_agent
        self.facility, self.line = getattr(self, "facility_s", None), getattr(self, "line_s", None)
        self.mandates = [getattr(self, "mandate_s", None)]
        self.parties.update(company=self.seller, supplier=self.buyer, financier=self.seller)
        try:
            yield
        finally:
            for name, value in saved.items():
                setattr(self, name, value)
            self.parties.update(parties)

    def relabel(self, label, back=1):
        self.h.cases[-back]["name"] = label

    def error(self, name, index=0):
        code = next(item["code"] for item in self.core.value["errors"] if item["name"] == name)
        return dict(program=CORE, name=name, code=code, index=index)

    def mark(self, part, changed="nothing", *, refuse=None):
        return self.note(section=part, leg="refund promise", changed=changed, refuse=refuse)

    def positions(self, party):
        return self.state("FinanceBook", party["book"])["data"]["positions"]

    def token_transfers(self, row):
        keys, found = row["account_keys"], []
        for group in row["result"]["inner_instructions"]:
            for inner in group:
                instruction = inner["instruction"]
                if keys[instruction["programIdIndex"]] != str(TOKEN):
                    continue
                data, accounts = bytes(instruction["data"][1:]), [keys[i] for i in instruction["accounts"][1:]]
                assert data[0] == 12, "unexpected token instruction"
                found.append(dict(source=accounts[0], destination=accounts[2], authority=accounts[3],
                    amount=int.from_bytes(data[1:9], "little")))
        return found

    def setup_parties(self):
        self.setup_principals()
        self.buyer, self.seller, self.lender, self.issuer = [self.parties[x] for x in ("company", "supplier", "financier", "issuer")]
        for role, amount in (("protocol", 0), ("fee", 0), ("issuer", 200_000)):
            self.create_custody(role, amount)
        self.create_custody("financier", LENDER_CASH, wallet=True)
        self.open_fee_agreement("main", 2_100)
        self.facility = self.open_facility()
        self.line, self.assignment_policy = self.open_line(self.facility)
        self.permission = self.permit("agent-1", 0)

        seller = self.seller
        self.transfer_tokens("give the seller 200,000 of test funds of its own", seller["owner_ata"], SELLER_CASH)
        self.seller_budget = pda(REGISTRY, b"budget", seller["principal"], self.mint)
        self.seller_actor = pda(REGISTRY, b"actor", seller["principal"], ident("actor-1"))
        self.send("seller's owner sets the seller's budget to 500,000", self.ix("principal_registry", "create_budget",
            dict(owner=seller["owner"].pubkey(), principal=seller["principal"], budget=self.seller_budget),
            args=dict(asset=self.mint, ceiling=BUDGET)), [seller["owner"]])
        self.send("seller's owner enrolls the seller's agent", self.ix("principal_registry", "create_actor",
            dict(owner=seller["owner"].pubkey(), principal=seller["principal"], actor=self.seller_actor),
            args=dict(actor_id=ident("actor-1"), current_key=self.seller_agent.pubkey(),
                valid_from=NOW - 100, valid_until=NOW + ENROLLMENT_SECONDS)), [seller["owner"]])
        with self.promise():
            self.create_custody("financier", 0, wallet=True, name="seller-own")
            self.relabel("seller's owner creates and binds the seller's own governed wallet", 3)
            self.relabel("seller's own wallet gets its token account", 2)
            self.relabel("seller's owner opens the wallet's cash record with nothing in it")
            self.add_cash("seller's owner moves 200,000 of the seller's own cash into the seller's own governed wallet", SELLER_CASH)
            self.mark("2")
            # Funding choice (i): the seller's own principal is the financier, a second principal is the credit issuer.
            self.facility = self.facility_s = self.open_facility(name="seller-own")
            self.relabel("ACCEPTED: seller's owner and the credit issuer's owner approve a facility in which the seller is the financier")
            self.mark("2")
            self.line = self.line_s = self.open_line(self.facility_s, "seller")[0]
            self.relabel("ACCEPTED: seller's owner and the credit issuer's owner open the seller's credit line on that facility")
            self.mark("2")
            self.mandate_s = self.grant(self.facility_s, 0, name="seller-agent")
            self.relabel("seller's owner permits its agent to place refund promises payable to the original buyer")
        facility = self.state("FundingFacility", self.facility_s)
        self.h.check("the promise facility's financier is the seller's principal", facility["financier"], seller["principal"])
        self.h.check("the promise facility's credit issuer is a different principal", facility["issuer"], self.issuer["principal"])
        self.h.check("the seller's wallet holds the seller's 200,000", self.h.amount(seller["custody_token"]), SELLER_CASH)
        self.h.check("the seller's own token account is empty after funding its wallet", self.h.amount(seller["owner_ata"]), 0)
        self.roles = {
            "original buyer's owner": self.buyer["owner"], "original buyer's agent": self.agent,
            "seller's owner": seller["owner"], "seller's agent": self.seller_agent,
            "lender's owner": self.lender["owner"], "credit issuer's owner": self.issuer["owner"],
            "decider": self.meter, "fee payer of setup and order transactions": self.h.payer,
            "key that submits signed records": self.submitter, "key that finalizes": self.finalizer,
            "key that collects": self.collector}
        keys = [str(holder.pubkey()) for holder in self.roles.values()]
        self.h.check("all eleven roles are held by different keys", len(set(keys)), len(keys))

    def promise_order(self, name, purchase):
        """Evidence order in the opposite direction; its request hash commits to the purchase's record address."""
        link = hashlib.sha256(b"refund promise for purchase record " + bytes(purchase["claim"])).digest()
        tab = self.evidence_tab(name, cap=PRICE, rates=((1, 1, PRICE),), policy_overrides=dict(request_hash=link))
        tab["link"] = link
        return tab


def exercise(f):
    f.setup_parties()
    buyer, seller, lender, issuer = f.buyer, f.seller, f.lender, f.issuer
    buyer_payout, seller_payout = buyer["owner_ata"], seller["owner_ata"]
    signature, shortfall = f.error("BillingSignature", 1), f.error("FundingCashShortfall", 1)

    # 0 seconds: purchases and promises accepted before the seller is paid.
    purchases, promises = {}, {}
    for pair in (1, 2):
        purchases[pair] = f.order(f"purchase-{pair}")
        f.send(f"ACCEPTED: purchase {pair}, the buyer's agent orders 100,000 from the seller, backed by the lender",
            f.commit_instruction(purchases[pair]), purchases[pair]["signers"], receipt=purchases[pair]["receipt"])
        f.mark("1")
        with f.promise():
            promises[pair] = f.promise_order(f"refund-promise-{pair}", purchases[pair])
            f.accept_evidence(promises[pair], label=f"ACCEPTED: refund promise {pair}, the seller's agent promises up to 100,000 "
                "to the original buyer, backed by the seller's own wallet, naming the decider")
            row = f.mark("2")
            bill = f.bill_state(promises[pair])
            claim = f.state("FundedClaim", promises[pair]["claim"])
        f.h.check(f"promise {pair}: the buyer of record is the seller", claim["terms"]["principal"], seller["principal"])
        f.h.check(f"promise {pair}: the seller of record is the original buyer", claim["supplier_principal"], buyer["principal"])
        f.h.check(f"promise {pair}: the payout key is the original buyer's owner key", claim["terms"]["supplier"], buyer["owner"].pubkey())
        f.h.check(f"promise {pair}: backed by the seller's own cash record", claim["custody"], seller["custody"])
        f.h.check(f"promise {pair}: the bill account names the decider as signer", bill["policy"]["source"], f.meter.pubkey())
        f.h.check(f"promise {pair}: the bill account names the original buyer as the only claimant", bill["supplier_claimant"], buyer["owner"].pubkey())
        f.h.check(f"promise {pair}: its request hash is the hash of purchase {pair}'s record address",
            bill["policy"]["request_hash"], hashlib.sha256(b"refund promise for purchase record " + bytes(purchases[pair]["claim"])).digest())
        f.h.check(f"promise {pair}: the decider's key is not among the acceptance's accounts", str(f.meter.pubkey()) in row["account_keys"], False)
        f.h.check(f"promise {pair}: the purchase's record is not among the acceptance's accounts",
            str(purchases[pair]["claim"]) in row["account_keys"], False)
        purchase_claim = f.state("FundedClaim", purchases[pair]["claim"])
        f.h.check(f"purchase {pair}: its record's contract hash is the fixture's ordinary one and does not refer to the promise",
            purchase_claim["external_contract_hash"], ident("provider-contract"))
    with f.promise():
        cash = f.lender_cash()
    f.h.check("the seller's wallet still holds its 200,000", cash["wallet"], SELLER_CASH)
    f.h.check("all 200,000 of the seller's wallet is promised", cash["promised"], 2 * PRICE)
    f.h.check("none of the seller's wallet is free", cash["free"], 0)
    f.h.check("the seller's budget holds both promises", f.registry.read(f.h.svm, f.seller_budget, "Budget")["reserved"], 2 * PRICE)

    with f.promise():
        f.withdraw(f.withdrawal("promised-cash", 1), reject=True,
            label="refused: seller's owner withdraws 1 unit from the seller's wallet while both promises are open")
        f.mark("5", "amount, 1 unit above the seller's free cash of 0", refuse=shortfall)

    runs = {}
    for pair in (1, 2):
        runs[pair] = f.earn(purchases[pair], f"purchase-{pair}-delivery", PRICE,
            label=f"ACCEPTED: the seller reports 100,000 delivered on purchase {pair}")
        f.mark("1")
        with f.paid_by(f.collector):
            request = f.collection(purchases[pair], runs[pair], PRICE)
            f.send(f"ACCEPTED: an outside key collects purchase {pair}'s 100,000 from the lender's wallet to the seller",
                f.envelope(compact_sign(lender["config"], lender["wallet"], request["instruction"])),
                request["signers"], receipt=request["receipt"])
            f.mark("1")
    f.h.check("the seller has been paid 200,000 for the two purchases", f.h.amount(seller_payout), 2 * PRICE)
    f.h.check("the seller's wallet still holds the promised 200,000", f.h.amount(seller["custody_token"]), SELLER_CASH)

    with f.promise():
        award = f.report(promises[1], units=(AWARD,))
        for holder, who in ((seller["owner"], "the seller"), (buyer["owner"], "the original buyer")):
            f.submit_report(promises[1], award, source=holder, error="BillingSignature",
                label=f"refused: {who} signs a refund of 30,000 on promise 1 in the decider's place")
            f.mark("5", "report signed by a party instead of the decider", refuse=signature)
        f.finalize(promises[1], error="BillingFinality",
            label="refused: finalization of promise 1 at 0 seconds, before the decider's window has closed")
        f.mark("5", "clock, before the windows close", refuse=f.error("BillingFinality"))

        # 30 seconds: after the seller was paid, the decider and the original buyer sign.
        f.at(30)
        f.submit_report(promises[1], award, label="ACCEPTED: at 30 seconds the decider signs a refund of 30,000 on promise 1")
        f.mark("3")
        for pair in (1, 2):
            f.submit_claim(promises[pair], f.monetary_claim(promises[pair], PRICE),
                label=f"ACCEPTED: the original buyer signs a claim of 100,000 on promise {pair}")
            f.mark("3" if pair == 1 else "4")

        # 46 seconds: finalization and payment.
        f.at(46)
        f.finalize(promises[1], label="ACCEPTED: an outside key finalizes promise 1 at 46 seconds; "
            "the decider signed 30,000 and the original buyer claimed 100,000: refund 30,000, 70,000 released to the seller")
        f.mark("3")
        f.assert_final_amount(promises[1], AWARD)
        f.finalize(promises[2], label="ACCEPTED: an outside key finalizes promise 2 at 46 seconds; "
            "the decider never signed and the original buyer claimed 100,000: refund 0, all 100,000 released to the seller")
        f.mark("4")
        f.assert_final_amount(promises[2], 0)
        f.collect_from(promises[2], PRICE, payer=f.collector, error="FundingRun",
            label="refused: collection to the original buyer of the 100,000 it claimed on promise 2, which the decider never signed for")
        f.mark("5", "no refund: the decider never signed", refuse=f.error("FundingRun", 1))
        f.collect_from(promises[1], AWARD + 1, payer=f.collector, error="FundingTerms",
            label="refused: collection to the original buyer of 30,001 on promise 1, one unit above the refund the decider signed")
        f.mark("5", "amount, one unit above the refund", refuse=f.error("FundingTerms", 1))
        before = {name: f.h.amount(token) for name, token in (("buyer", buyer_payout), ("wallet", seller["custody_token"]),
            ("lender", lender["custody_token"]), ("seller", seller_payout))}
        f.collect_from(promises[1], AWARD, payer=f.collector,
            label="ACCEPTED: an outside key collects the refund of 30,000 from the seller's wallet to the original buyer")
        row = f.mark("3")
        cash = f.lender_cash()
        work = f.state("FundedRun", promises[1]["settlement"]["run"])
        facility = f.state("FundingFacility", f.facility_s)
    f.h.check("the refund is exactly one token transfer: the seller's wallet to the original buyer's payout account, 30,000",
        f.token_transfers(row), [dict(source=str(seller["custody_token"]), destination=str(buyer_payout),
            authority=str(seller["wallet"]), amount=AWARD)])
    f.h.check("the original buyer received exactly 30,000", f.h.amount(buyer_payout) - before["buyer"], AWARD)
    f.h.check("the seller's wallet paid exactly 30,000", before["wallet"] - f.h.amount(seller["custody_token"]), AWARD)
    f.h.check("the lender's wallet did not change", f.h.amount(lender["custody_token"]), before["lender"])
    f.h.check("the seller's own token account did not change", f.h.amount(seller_payout), before["seller"])
    f.h.check("the seller's wallet now holds 170,000", cash["wallet"], SELLER_CASH - AWARD)
    f.h.check("nothing in the seller's wallet is promised any more", cash["promised"], 0)
    f.h.check("the seller's 170,000 is free", cash["free"], SELLER_CASH - AWARD)
    f.h.check("refund work record: the debtor is the seller", work["debtor"], seller["principal"])
    f.h.check("refund work record: the creditor is the credit issuer", work["current_creditor"], issuer["principal"])
    f.h.check("refund work record: the seller owes 30,000", work["principal"]["outstanding"], AWARD)
    f.h.check("promise facility: the credit issuer owes 30,000 to the facility's financier", facility["drawn"], AWARD)
    f.h.check("promise facility: its financier is the seller", facility["financier"], seller["principal"])
    book = f.positions(seller)
    f.h.check("seller's book: principal the seller owes", book["customer_principal"], AWARD)
    f.h.check("seller's book: facility assets the seller is owed", book["facility_assets"], AWARD)
    f.h.check("seller's book: nothing still held for promises", (book["customer_reserved"], book["funding_reserved"]), (0, 0))
    book = f.positions(issuer)
    f.h.check("credit issuer's book: principal it owns (200,000 from the buyer, 30,000 from the seller)", book["owned_principal"], 2 * PRICE + AWARD)
    f.h.check("credit issuer's book: facility debt it owes (200,000 to the lender, 30,000 to the seller)", book["facility_debt"], 2 * PRICE + AWARD)
    with f.promise():
        f.withdraw(f.withdrawal("after-promises", SELLER_CASH - AWARD),
            label="ACCEPTED: seller's owner withdraws the 170,000 left in the seller's wallet after both promises closed")
        f.mark("5")
    f.h.check("the seller ends with 370,000: 200,000 of its own, 200,000 for the two purchases, less the 30,000 refund",
        f.h.amount(seller_payout), 2 * PRICE + SELLER_CASH - AWARD)
    for pair in (1, 2):
        claim = f.state("FundedClaim", purchases[pair]["claim"])
        f.h.check(f"purchase {pair}: the buyer's debt on the purchase is still the full 100,000; the refund did not reduce it",
            (claim["debt"], claim["paid"]), (PRICE, PRICE))
    claim = f.state("FundedClaim", promises[2]["claim"])
    f.h.check("promise 2 at the end: nothing paid, no debt, 100,000 released",
        (claim["paid"], claim["debt"], claim["released"]), (0, 0, PRICE))


def record(f, summary, index, directory, seconds):
    """Copy the finished run into a publication directory; no second execution."""
    directory = Path(directory).resolve()
    directory.mkdir(parents=True, exist_ok=True)
    raw = Path(summary["trace"]).read_bytes()
    packed = directory / "refund-promise-trace.json.xz"
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
        accounts={"seller principal": str(f.seller["principal"]), "seller's governed wallet": str(f.seller["wallet"]),
            "seller's wallet token account": str(f.seller["custody_token"]),
            "seller's own token account": str(f.seller["owner_ata"]),
            "original buyer principal": str(f.buyer["principal"]),
            "original buyer's payout token account": str(f.buyer["owner_ata"]),
            "lender's wallet token account": str(f.lender["custody_token"])},
        program_sha256={name: value["sha256"] for name, value in f.build["artifacts"].items()},
        swig_sha256=SWIG_SHA256, build_receipt_sha256=digest(OUT / "build-receipt.json"),
        idl_receipt_sha256=digest(OUT / "idl-receipt.json"))
    for name in ("build-receipt.json", "idl-receipt.json"):
        (directory / name).write_bytes((OUT / name).read_bytes())
    (directory / "refund-promise-receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    return receipt


def main():
    started = time.monotonic()
    f = RefundPromise()
    try:
        exercise(f)
    except Exception as error:
        f.finish("refund-promise", error=error)
        raise
    index = case_index(f, "refund-promise")
    for row in f.h.cases:
        if "commit_legs" in row:
            row["refund_promise"] = row.pop("commit_legs")
    summary = f.finish("refund-promise")
    if "--record" in sys.argv:
        print(json.dumps(record(f, summary, index, sys.argv[sys.argv.index("--record") + 1], time.monotonic() - started)))


if __name__ == "__main__":
    main()
