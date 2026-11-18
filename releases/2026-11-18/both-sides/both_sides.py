#!/usr/bin/env python3
"""One account that buys on one order and sells on another.

Three companies in a chain, each a registered principal with its own owner key:

  customer  --order B, maximum 150,000-->  middle company  --order A, maximum 100,000-->  supplier

The middle company is the buyer on order A and the seller on order B. Order A
is backed by lender 1's governed wallet and order B by lender 2's. One credit
issuer stands behind both orders. Orders carry no interest and no seller fee.

  1. Order A accepted: the middle company's agent orders from the supplier.
  2. Order B accepted: the customer's agent orders from the middle company.
  3. The middle company's records are read: one financial book holds both sides.
  7. The middle company's owner revokes its agent and enrolls a new one.
  4. The middle company earns 150,000 on order B; an outside key collects it.
  8. The middle company's owner hands control to a new owner key.
  5. The supplier earns 100,000 on order A; an outside key collects it.
  6. The middle company's records are read again.

The shared fixture names its parties differently. Here "company" is the middle
company, "assignee" is the customer, "sponsor" is lender 2 and "financier" is
lender 1. The fixture's seller-side and buyer-side helpers are pointed at the
right parties for order B by `order_b()`; no instruction or account is altered.

Every transaction is executed. The scenario expects no refusal. No account
bytes are written by the harness and the clock never moves.

Record: <python> foundation/onchain/tests/both_sides.py --record docs/evidence/<directory>
"""
from commit_legs import *

ORDER_A = 100_000
ORDER_B = 150_000


class BothSides(CommitLegs):
    def __init__(self):
        super().__init__()
        self.customer_agent = key(66)

    @contextlib.contextmanager
    def order_b(self):
        """Buyer is the customer, seller is the middle company, lender is lender 2."""
        names = ("company", "budget", "actors", "agent", "facility", "line", "mandates")
        saved = {name: getattr(self, name) for name in names}
        parties = {role: self.parties[role] for role in ("company", "supplier", "financier")}
        self.company, self.budget = self.customer["principal"], self.customer_budget
        self.actors, self.agent = [self.customer_actor], self.customer_agent
        self.facility, self.line = getattr(self, "facility_b", None), getattr(self, "line_b", None)
        self.mandates = [getattr(self, "mandate_b", None)]
        self.parties.update(company=self.customer, supplier=self.middle, financier=self.lender2)
        try:
            yield
        finally:
            for name, value in saved.items():
                setattr(self, name, value)
            self.parties.update(parties)

    def relabel(self, label):
        self.h.cases[-1]["name"] = label

    def chain(self):
        self.setup_principals()
        self.middle, self.customer, self.supplier = [self.parties[x] for x in ("company", "assignee", "supplier")]
        self.lender1, self.lender2, self.issuer = [self.parties[x] for x in ("financier", "sponsor", "issuer")]
        for role, amount in (("protocol", 0), ("fee", 0), ("issuer", 200_000)):
            self.create_custody(role, amount)
        self.create_custody("financier", LENDER_CASH, wallet=True)
        self.relabel("lender 1's owner allocates 1,000,000 in its governed wallet")
        self.create_custody("sponsor", LENDER_CASH, wallet=True)
        self.relabel("lender 2's owner allocates 1,000,000 in its governed wallet")
        self.open_fee_agreement("main", 2_100)
        self.facility = self.open_facility()
        self.relabel("lender 1 and the credit issuer approve lender 1's facility")
        self.line, self.assignment_policy = self.open_line(self.facility)
        self.relabel("middle company's owner and the credit issuer open the middle company's credit line")
        self.grant(self.facility)
        self.relabel("middle company's owner permits its agent to order from the supplier on lender 1's facility")

        customer = self.customer
        self.customer_budget = pda(REGISTRY, b"budget", customer["principal"], self.mint)
        self.customer_actor = pda(REGISTRY, b"actor", customer["principal"], ident("actor-1"))
        self.send("customer's owner sets the customer's budget to 500,000", self.ix("principal_registry", "create_budget",
            dict(owner=customer["owner"].pubkey(), principal=customer["principal"], budget=self.customer_budget),
            args=dict(asset=self.mint, ceiling=BUDGET)), [customer["owner"]])
        self.send("customer's owner enrolls the customer's agent", self.ix("principal_registry", "create_actor",
            dict(owner=customer["owner"].pubkey(), principal=customer["principal"], actor=self.customer_actor),
            args=dict(actor_id=ident("actor-1"), current_key=self.customer_agent.pubkey(),
                valid_from=NOW - 100, valid_until=NOW + ENROLLMENT_SECONDS)), [customer["owner"]])
        with self.order_b():
            self.facility = self.facility_b = self.open_facility(name="lender-2")
            self.relabel("lender 2 and the credit issuer approve lender 2's facility")
            self.line = self.line_b = self.open_line(self.facility_b, "customer")[0]
            self.relabel("customer's owner and the credit issuer open the customer's credit line")
            self.mandate_b = self.grant(self.facility_b, 0, name="customer-agent")
            self.relabel("customer's owner permits its agent to order from the middle company on lender 2's facility")
        self.roles = {
            "customer's owner": self.customer["owner"], "customer's agent": self.customer_agent,
            "middle company's owner (first)": self.middle["owner"], "middle company's agent (first)": self.agent,
            "middle company's agent (second)": self.replacement_agent,
            "middle company's owner (second)": self.replacement_buyer_owner,
            "supplier's owner": self.supplier["owner"], "lender 1's owner": self.lender1["owner"],
            "lender 2's owner": self.lender2["owner"], "credit issuer's owner": self.issuer["owner"],
            "fee payer of setup and order transactions": self.h.payer, "key that collects": self.collector}
        keys = [str(holder.pubkey()) for holder in self.roles.values()]
        self.h.check("all twelve roles are held by different keys", len(set(keys)), len(keys))
        principals = [self.customer["principal"], self.middle["principal"], self.supplier["principal"],
            self.lender1["principal"], self.lender2["principal"], self.issuer["principal"]]
        self.h.check("customer, middle company, supplier, both lenders and the credit issuer are six different principals",
            len(set(map(str, principals))), 6)
        self.h.check("the two lenders use two different wallets", self.lender1["wallet"] == self.lender2["wallet"], False)
        self.h.check("the two lenders' wallets use two different token accounts",
            self.lender1["custody_token"] == self.lender2["custody_token"], False)

    def place(self, tab, label, *, part, changed="nothing"):
        self.send(label, self.commit_instruction(tab), tab["signers"], receipt=tab["receipt"])
        return self.note(section=part, leg="both sides", changed=changed)

    def collect_as(self, request, label, *, part):
        lender = self.parties["financier"]
        self.send(label, self.envelope(compact_sign(lender["config"], lender["wallet"], request["instruction"])),
            request["signers"], receipt=request["receipt"])
        return self.note(section=part, leg="both sides", changed="nothing")

    def book(self, party):
        return self.state("FinanceBook", party["book"])["data"]["positions"]

    def check_book(self, moment, who, party, **nonzero):
        positions = self.book(party)
        self.h.check(f"{who}'s book {moment}: every non-zero position",
            {field: value for field, value in positions.items() if value}, nonzero)
        projection = self.pool.read(self.h.svm, party["projection"], "FinancialProjection")
        self.h.check(f"{who}'s Pool projection {moment} mirrors its book", projection["positions"], positions)
        return positions

    def token_transfers(self, row):
        """Every SPL TransferChecked executed inside the transaction, from its recorded inner instructions."""
        keys, found = row["account_keys"], []
        for group in row["result"]["inner_instructions"]:
            for inner in group:
                instruction = inner["instruction"]
                if keys[instruction["programIdIndex"]] != str(TOKEN):
                    continue
                data, accounts = bytes(instruction["data"][1:]), [keys[i] for i in instruction["accounts"][1:]]
                assert data[0] == 12, "unexpected token instruction"
                found.append(dict(source=accounts[0], mint=accounts[1], destination=accounts[2],
                    authority=accounts[3], amount=int.from_bytes(data[1:9], "little")))
        return found

    def signers_of(self, row):
        tx = VersionedTransaction.from_bytes(__import__("base64").b64decode(row["transaction_base64"]))
        return [str(k) for k in tx.message.account_keys[:len(tx.signatures)]]


def exercise(f):
    f.chain()
    middle, customer, supplier, lender1, lender2, issuer = f.middle, f.customer, f.supplier, f.lender1, f.lender2, f.issuer
    middle_payout = middle["owner_ata"]
    supplier_payout = supplier["owner_ata"]
    customer_token = customer["owner_ata"]
    f.check_book("before any order", "middle company", middle)

    # 1. Order A: the middle company buys from the supplier, backed by lender 1.
    a = f.acceptance("order-A", cap=ORDER_A, maximum_interest=0, fees=False)
    f.place(a, "ACCEPTED: order A, the middle company's agent orders from the supplier, maximum 100,000, backed by lender 1", part="1")
    f.check_book("after order A", "middle company", middle, customer_reserved=ORDER_A)

    # 2. Order B: the customer buys from the middle company, backed by lender 2.
    with f.order_b():
        b = f.acceptance("order-B", cap=ORDER_B, maximum_interest=0, fees=False)
        row = f.place(b, "ACCEPTED: order B, the customer's agent orders from the middle company, maximum 150,000, backed by lender 2", part="2")
    f.h.check("order B is signed by the fee payer, the customer's agent, the credit issuer's owner and the middle company's owner as seller",
        sorted(f.signers_of(row)), sorted(str(k.pubkey()) for k in (f.h.payer, f.customer_agent, issuer["owner"], middle["owner"])))

    # 3. Where the middle company's two positions are recorded.
    f.h.check("the middle company's book address is derived from its principal and the token",
        middle["book"], pda(CORE, b"financial-book", middle["principal"], f.mint))
    before = f.check_book("after both orders", "middle company", middle,
        customer_reserved=ORDER_A, supplier_committed=ORDER_B)
    f.h.check("one account holds the middle company's buyer side", before["customer_reserved"], ORDER_A)
    f.h.check("the same account holds the middle company's seller side", before["supplier_committed"], ORDER_B)
    f.check_book("after both orders", "customer", customer, customer_reserved=ORDER_B)
    f.check_book("after both orders", "supplier", supplier, supplier_committed=ORDER_A)
    f.check_book("after both orders", "lender 1", lender1, funding_reserved=ORDER_A, capital_allocated=LENDER_CASH)
    f.check_book("after both orders", "lender 2", lender2, funding_reserved=ORDER_B, capital_allocated=LENDER_CASH)
    claim_a, claim_b = f.state("FundedClaim", a["claim"]), f.state("FundedClaim", b["claim"])
    f.h.check("order A's record is stored under the middle company as buyer",
        a["claim"], pda(CORE, b"claim", middle["principal"], a["terms"]["tab_id"]))
    f.h.check("order A's record names the supplier as seller", claim_a["supplier_principal"], supplier["principal"])
    f.h.check("order B's record is stored under the customer as buyer",
        b["claim"], pda(CORE, b"claim", customer["principal"], b["terms"]["tab_id"]))
    f.h.check("order B's record names the middle company as seller", claim_b["supplier_principal"], middle["principal"])
    f.h.check("order B's record names the middle company owner's key as payout key", claim_b["terms"]["supplier"], middle["owner"].pubkey())
    f.h.check("order A is backed by lender 1's cash record", claim_a["custody"], lender1["custody"])
    f.h.check("order B is backed by lender 2's cash record", claim_b["custody"], lender2["custody"])
    budget = f.registry.read(f.h.svm, f.budget, "Budget")
    f.h.check("the middle company's Registry budget holds only its buying side", budget["reserved"], ORDER_A)
    f.h.check("the customer's Registry budget holds order B",
        f.registry.read(f.h.svm, f.customer_budget, "Budget")["reserved"], ORDER_B)

    # 7. The middle company changes its agent before either collection.
    watched = [a["claim"], a["reservation"], a["receipt"], b["claim"], b["reservation"], b["receipt"]]
    orders_before = f.h.snapshot(watched)
    f.send("middle company's owner revokes its agent", f.ix("principal_registry", "revoke_actor",
        dict(owner=middle["owner"].pubkey(), principal=f.company, actor=f.actors[0]), expected_generation=1),
        [middle["owner"]])
    f.note(section="7", leg="both sides", changed="nothing")
    new_actor = pda(REGISTRY, b"actor", f.company, ident("actor-new"))
    f.send("middle company's owner enrolls a new agent with a new key", f.ix("principal_registry", "create_actor",
        dict(owner=middle["owner"].pubkey(), principal=f.company, actor=new_actor),
        args=dict(actor_id=ident("actor-new"), current_key=f.replacement_agent.pubkey(),
            valid_from=NOW - 100, valid_until=NOW + ENROLLMENT_SECONDS)), [middle["owner"]])
    f.note(section="7", leg="both sides", changed="nothing")
    f.actors.append(new_actor)
    f.grant(f.facility, 2, name="middle-new-agent")
    f.relabel("middle company's owner permits the new agent to order from the supplier on lender 1's facility")
    f.note(section="7", leg="both sides", changed="nothing")
    f.h.check("the middle company's first agent is revoked", f.registry.read(f.h.svm, f.actors[0], "Actor")["active"], False)
    f.h.check("the middle company's new agent is enrolled", f.registry.read(f.h.svm, new_actor, "Actor")["active"], True)
    f.h.check("orders A and B are byte-identical across the middle company's change of agent", f.h.snapshot(watched), orders_before)
    f.check_book("after the change of agent", "middle company", middle, customer_reserved=ORDER_A, supplier_committed=ORDER_B)

    # 4. The middle company earns on order B and is paid from lender 2's wallet.
    with f.order_b():
        run_b = f.earn(b, "order-B-work", ORDER_B,
            label="ACCEPTED: the middle company reports 150,000 of completed work on order B")
        f.note(section="4", leg="both sides", changed="nothing")
        f.check_book("after its work on order B", "middle company", f.middle, customer_reserved=ORDER_A, supplier_receivable=ORDER_B)
        balances = {name: f.h.amount(token) for name, token in (("middle", middle_payout), ("customer", customer_token),
            ("lender1", lender1["custody_token"]), ("lender2", lender2["custody_token"]), ("supplier", supplier_payout))}
        with f.paid_by(f.collector):
            row = f.collect_as(f.collection(b, run_b, ORDER_B),
                "ACCEPTED: an outside key collects order B's 150,000 from lender 2's wallet to the middle company", part="4")
    f.h.check("order B's collection contains exactly one token transfer: lender 2's wallet to the middle company's payout account, 150,000",
        f.token_transfers(row), [dict(source=str(lender2["custody_token"]), mint=str(f.mint),
            destination=str(middle_payout), authority=str(lender2["wallet"]), amount=ORDER_B)])
    f.h.check("order B's collection is signed by the outside key only", f.signers_of(row), [str(f.collector.pubkey())])
    f.h.check("the customer's own token account is not among order B's collection accounts", str(customer_token) in row["account_keys"], False)
    f.h.check("the customer's owner key is not among order B's collection accounts", str(customer["owner"].pubkey()) in row["account_keys"], False)
    f.h.check("the middle company received exactly 150,000", f.h.amount(middle_payout) - balances["middle"], ORDER_B)
    f.h.check("lender 2's wallet paid exactly 150,000", balances["lender2"] - f.h.amount(lender2["custody_token"]), ORDER_B)
    f.h.check("the customer's own token account did not change", f.h.amount(customer_token), balances["customer"])
    f.h.check("lender 1's wallet did not change", f.h.amount(lender1["custody_token"]), balances["lender1"])
    f.check_book("after being paid on order B", "middle company", middle, customer_reserved=ORDER_A, cash_received=ORDER_B)

    # 8. The middle company changes its owner key between the two collections.
    orders_before = f.h.snapshot(watched)
    first_owner = f.rotate_owner("company", f.replacement_buyer_owner, "middle company's owner hands control to a new owner key")
    f.note(section="8", leg="both sides", changed="nothing")
    f.h.check("the middle company's principal now names the new owner",
        f.registry.read(f.h.svm, middle["principal"], "Principal")["owner"], f.replacement_buyer_owner.pubkey())
    f.h.check("orders A and B are byte-identical across the middle company's change of owner", f.h.snapshot(watched), orders_before)
    f.h.check("the middle company's book is unchanged by its change of owner",
        {k: v for k, v in f.book(middle).items() if v}, dict(customer_reserved=ORDER_A, cash_received=ORDER_B))
    f.h.check("the middle company's book address is unchanged by its change of owner",
        middle["book"], pda(CORE, b"financial-book", middle["principal"], f.mint))

    # 5. The supplier earns on order A and is paid from lender 1's wallet.
    run_a = f.earn(a, "order-A-work", ORDER_A, label="ACCEPTED: the supplier reports 100,000 of completed work on order A")
    f.note(section="5", leg="both sides", changed="nothing")
    f.check_book("after the supplier's work on order A", "middle company", middle, customer_principal=ORDER_A, cash_received=ORDER_B)
    balances = {name: f.h.amount(token) for name, token in (("middle", middle_payout), ("supplier", supplier_payout),
        ("lender1", lender1["custody_token"]), ("lender2", lender2["custody_token"]))}
    with f.paid_by(f.collector):
        row = f.collect_as(f.collection(a, run_a, ORDER_A),
            "ACCEPTED: an outside key collects order A's 100,000 from lender 1's wallet to the supplier", part="5")
    f.h.check("order A's collection contains exactly one token transfer: lender 1's wallet to the supplier's payout account, 100,000",
        f.token_transfers(row), [dict(source=str(lender1["custody_token"]), mint=str(f.mint),
            destination=str(supplier_payout), authority=str(lender1["wallet"]), amount=ORDER_A)])
    f.h.check("order A's collection is signed by the outside key only", f.signers_of(row), [str(f.collector.pubkey())])
    for name, holder in (("first owner", first_owner), ("second owner", f.replacement_buyer_owner),
            ("first agent", f.agent), ("second agent", f.replacement_agent)):
        f.h.check(f"the middle company's {name} key is not among order A's collection accounts",
            str(holder.pubkey()) in row["account_keys"], False)
    f.h.check("the middle company's token account is not among order A's collection accounts", str(middle_payout) in row["account_keys"], False)
    f.h.check("the supplier received exactly 100,000", f.h.amount(supplier_payout) - balances["supplier"], ORDER_A)
    f.h.check("lender 1's wallet paid exactly 100,000", balances["lender1"] - f.h.amount(lender1["custody_token"]), ORDER_A)
    f.h.check("the middle company's token account did not change", f.h.amount(middle_payout), balances["middle"])
    f.h.check("lender 2's wallet did not change", f.h.amount(lender2["custody_token"]), balances["lender2"])

    # 6. The middle company's records after both collections.
    after = f.check_book("after both collections", "middle company", middle, customer_principal=ORDER_A, cash_received=ORDER_B)
    f.h.check("the middle company owes 100,000 of principal on order A", after["customer_principal"], ORDER_A)
    f.h.check("the middle company has been paid 150,000 on order B", after["cash_received"], ORDER_B)
    f.h.check("nothing further is owed to the middle company", (after["supplier_receivable"], after["supplier_committed"]), (0, 0))
    f.check_book("after both collections", "customer", customer, customer_principal=ORDER_B)
    f.check_book("after both collections", "supplier", supplier, cash_received=ORDER_A)
    f.check_book("after both collections", "credit issuer", issuer, owned_principal=ORDER_A + ORDER_B,
        facility_debt=ORDER_A + ORDER_B, capital_allocated=200_000)
    f.check_book("after both collections", "lender 1", lender1, facility_assets=ORDER_A, cash_sent=ORDER_A, capital_allocated=LENDER_CASH)
    f.check_book("after both collections", "lender 2", lender2, facility_assets=ORDER_B, cash_sent=ORDER_B, capital_allocated=LENDER_CASH)
    work_a, work_b = f.state("FundedRun", run_a["run"]), f.state("FundedRun", run_b["run"])
    f.h.check("order A's work record: the debtor is the middle company", work_a["debtor"], middle["principal"])
    f.h.check("order A's work record: the debt is owned by the credit issuer", work_a["current_creditor"], issuer["principal"])
    f.h.check("order B's work record: the debtor is the customer", work_b["debtor"], customer["principal"])
    f.h.check("order B's work record: the debt is owned by the credit issuer", work_b["current_creditor"], issuer["principal"])
    f.h.check("order B's work record: the seller paid is the middle company", work_b["supplier_principal"], middle["principal"])
    claim_a, claim_b = f.state("FundedClaim", a["claim"]), f.state("FundedClaim", b["claim"])
    for name, claim, amount in (("A", claim_a, ORDER_A), ("B", claim_b, ORDER_B)):
        for field, value in (("debt", amount), ("paid", amount), ("payable", 0), ("remaining", 0)):
            f.h.check(f"order {name} record {field} at the end", claim[field], value)
    f.h.check("order A's record still carries the terms the revoked first agent signed", claim_a["terms"], a["terms"])
    f.h.check("order B's record still carries the middle company's first owner key as payout key",
        claim_b["terms"]["supplier"], first_owner.pubkey())
    f.h.check("the middle company's Registry budget shows 100,000 consumed and nothing held",
        (lambda x: (x["consumed"], x["reserved"]))(f.registry.read(f.h.svm, f.budget, "Budget")), (ORDER_A, 0))
    f.h.check("no transaction in this scenario was refused", sum(row["expected"] == "reject" for row in f.h.cases), 0)


def record(f, summary, index, directory, seconds):
    """Copy the finished run into a publication directory; no second execution."""
    directory = Path(directory).resolve()
    directory.mkdir(parents=True, exist_ok=True)
    raw = Path(summary["trace"]).read_bytes()
    packed = directory / "both-sides-trace.json.xz"
    packed.write_bytes(lzma.compress(raw))
    assert lzma.decompress(packed.read_bytes()) == raw
    (directory / "case-index.json").write_text(json.dumps(index, indent=1, default=json_value) + "\n")
    annotated = [row for row in index if row["section"] != "setup"]
    middle = f.middle
    receipt = dict(summary, trace=packed.name, trace_xz_sha256=digest(packed),
        annotated_cases=len(annotated),
        annotated_refusals=sum(row["outcome"] == "refused" for row in annotated),
        annotated_acceptances=sum(row["outcome"] == "accepted" for row in annotated),
        run_seconds=round(seconds, 2), engine="solders 0.29.0 LiteSVM",
        roles={role: str(holder.pubkey()) for role, holder in f.roles.items()},
        accounts={"middle company principal": str(middle["principal"]), "middle company financial book": str(middle["book"]),
            "middle company Pool projection": str(middle["projection"]), "middle company payout token account": str(middle["owner_ata"]),
            "customer principal": str(f.customer["principal"]), "supplier principal": str(f.supplier["principal"]),
            "lender 1 wallet token account": str(f.lender1["custody_token"]),
            "lender 2 wallet token account": str(f.lender2["custody_token"])},
        program_sha256={name: value["sha256"] for name, value in f.build["artifacts"].items()},
        swig_sha256=SWIG_SHA256, build_receipt_sha256=digest(OUT / "build-receipt.json"),
        idl_receipt_sha256=digest(OUT / "idl-receipt.json"))
    for name in ("build-receipt.json", "idl-receipt.json"):
        (directory / name).write_bytes((OUT / name).read_bytes())
    (directory / "both-sides-receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    return receipt


def main():
    started = time.monotonic()
    f = BothSides()
    try:
        exercise(f)
    except Exception as error:
        f.finish("both-sides", error=error)
        raise
    index = case_index(f, "both-sides")
    for row in f.h.cases:
        if "commit_legs" in row:
            row["both_sides"] = row.pop("commit_legs")
    summary = f.finish("both-sides")
    if "--record" in sys.argv:
        print(json.dumps(record(f, summary, index, sys.argv[sys.argv.index("--record") + 1], time.monotonic() - started)))


if __name__ == "__main__":
    main()
