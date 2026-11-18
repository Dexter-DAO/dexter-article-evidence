#!/usr/bin/env python3
"""One reserve, many claims: two agents and two sellers drawing on one pot.

The cast is the same in both runs: one buyer with two enrolled agents, two
sellers, one credit issuer and one lender with a single governed wallet. Agent A
may order from seller 1 and agent B from seller 2, each up to 100,000 per order.
The two runs differ only in which pot is the tight one, so each refusal has one
cause:

  shared-reserve-budget   buyer's budget 150,000, lender wallet 1,000,000
  shared-reserve-wallet   buyer's budget 500,000, lender wallet 150,000

Part 1 (budget run): both agents draw on the one budget.
Part 2 (wallet run): orders for both sellers are promised from the one wallet.
Part 3 (wallet run): each seller's claim is collected and released on its own.
Part 4 (wallet run): agent A is revoked before seller 1 is paid.

Seller 2 is the shared fixture's party named "assignee"; here it is only a
second registered seller. Refusals are checked exactly as in commit_legs.py:
transaction error, first failing program frame, Anchor error name and number
from that frame's log, and byte-identical state for every writable account
except the fee payer. Nothing here writes account bytes from the harness and
the clock never moves.

Record: <python> foundation/onchain/tests/shared_reserve.py --record docs/evidence/<directory>
"""
from commit_legs import *

PER_ORDER = 100_000
AGENTS = {"A": (0, "supplier", "seller 1"), "B": (1, "assignee", "seller 2")}
RUNS = ("shared-reserve-budget", "shared-reserve-wallet")


class SharedReserve(CommitLegs):
    @contextlib.contextmanager
    def selling(self, role):
        """Let the shared fixture's seller-bound helpers act for another registered seller."""
        original, self.parties["supplier"] = self.parties["supplier"], self.parties[role]
        try:
            yield
        finally:
            self.parties["supplier"] = original

    def relabel(self, label):
        self.h.cases[-1]["name"] = label

    def cast(self, *, budget, lender_cash):
        self.setup_principals()
        for role, amount in (("protocol", 0), ("fee", 0), ("issuer", 200_000)):
            self.create_custody(role, amount)
        self.create_custody("financier", lender_cash, wallet=True)
        self.open_fee_agreement("main", 2_100)
        self.facility = self.open_facility()
        self.line, self.assignment_policy = self.open_line(self.facility)
        if budget != BUDGET:
            self.set_budget(f"buyer's owner sets the shared budget to {budget:,}", budget)
        self.permissions = []
        for agent, (index, role, seller) in AGENTS.items():
            with self.selling(role):
                self.permissions.append(self.permit("agent-" + agent, index))
            self.relabel(f"buyer's owner permits agent {agent} to order from {seller} on the lender's facility, "
                f"per-order limit {PER_ORDER:,}")
            mandate = self.registry.read(self.h.svm, self.permissions[index], "Mandate")
            self.h.check(f"agent {agent} per-order limit", mandate["per_operation_limit"], PER_ORDER)
            self.h.check(f"agent {agent} draws on the one budget account", mandate["budget"], self.budget)
            self.h.check(f"agent {agent} is permitted for {seller} only", mandate["supplier"],
                self.parties[role]["owner"].pubkey())
        self.h.check("the two agents are different keys", self.agent.pubkey() == self.agent2.pubkey(), False)
        self.h.check("the two sellers are different principals",
            self.parties["supplier"]["principal"] == self.parties["assignee"]["principal"], False)

    def order_for(self, name, agent, cap):
        index, role, _ = AGENTS[agent]
        with self.selling(role):
            tab = self.acceptance(name, cap=cap, maximum_interest=0, index=index, fees=False,
                mandate=self.permissions[index])
        tab["roles"] = ["company", "issuer", role, "financier"]
        tab["seller_role"] = role
        return tab

    def place(self, tab, label, *, part, leg, changed, refuse=None):
        self.send(label, self.commit_instruction(tab, tab["roles"]), tab["signers"],
            reject=refuse is not None, receipt=tab["receipt"])
        return self.note(section=part, leg=leg, changed=changed, refuse=refuse)

    def collect_as(self, request, label, *, part, changed, refuse=None):
        lender = self.parties["financier"]
        self.send(label, self.envelope(compact_sign(lender["config"], lender["wallet"], request["instruction"])),
            request["signers"], reject=refuse is not None, receipt=request["receipt"])
        return self.note(section=part, leg="claims", changed=changed, refuse=refuse)

    def withdraw_case(self, name, amount, label, *, part, changed, refuse=False):
        self.withdraw(self.withdrawal(name, amount), reject=refuse, label=label)
        return self.note(section=part, leg="wallet", changed=changed,
            refuse=dict(program=CORE, name="FundingCashShortfall", code=6033, index=1) if refuse else None)

    def check_budget(self, moment, *, held, consumed=0, ceiling):
        budget = self.registry.read(self.h.svm, self.budget, "Budget")
        self.h.check(f"budget ceiling {moment}", budget["ceiling"], ceiling)
        self.h.check(f"budget held {moment}", budget["reserved"], held)
        self.h.check(f"budget consumed {moment}", budget["consumed"], consumed)

    def check_wallet(self, moment, *, wallet, promised, free):
        cash = self.lender_cash()
        self.h.check(f"lender wallet token balance {moment}", cash["wallet"], wallet)
        self.h.check(f"lender promised total {moment}", cash["promised"], promised)
        self.h.check(f"lender free cash {moment}", cash["free"], free)

    def other_pots_cover(self, moment, amount, *, budget=False, cash=False):
        """Recorded proof that the pots a refusal is not about still cover the order."""
        facility = self.state("FundingFacility", self.facility)
        line = self.state("CustomerCreditLine", self.line)
        pots = [("facility limit", facility["commitment_limit"] - facility["reserved"] - facility["drawn"]),
            ("credit line", line["limit"] - line["reserved"] - line["principal_outstanding"] - line["fee_exposure"])]
        if budget:
            state = self.registry.read(self.h.svm, self.budget, "Budget")
            pots.append(("budget", state["ceiling"] - state["reserved"] - state["consumed"]))
        if cash:
            pots.append(("lender free cash", self.lender_cash()["free"]))
        for name, value in pots:
            self.h.check(f"{moment}: remaining {name} covers {amount:,}", value >= amount, True)


def shared_budget(f):
    """Part 1. Budget 150,000 is the tight pot; the lender wallet holds 1,000,000."""
    f.cast(budget=150_000, lender_cash=LENDER_CASH)
    exhausted = dict(program=REGISTRY, name="BudgetExhausted", code=6012)
    f.check_budget("at the start", held=0, ceiling=150_000)

    a1 = f.order_for("agent-A-order-1", "A", 100_000)
    f.place(a1, "ACCEPTED: agent A orders 100,000 from seller 1; shared budget 150,000 with nothing held",
        part="1", leg="budget", changed="nothing")
    f.check_budget("after agent A's 100,000", held=100_000, ceiling=150_000)

    f.other_pots_cover("before agent B's 100,000", 100_000, cash=True)
    f.place(f.order_for("agent-B-order-1", "B", 100_000),
        "budget refused: agent B orders 100,000 from seller 2; the shared budget has 50,000 left after agent A's order",
        part="1", leg="budget", changed="amount, 50,000 above what the shared budget has left", refuse=exhausted)
    f.check_budget("after agent B's refused 100,000", held=100_000, ceiling=150_000)
    f.place(f.order_for("agent-B-order-1", "B", 50_000),
        "ACCEPTED: agent B orders 50,000 from seller 2; exactly what the shared budget has left",
        part="1", leg="budget", changed="nothing")
    f.check_budget("after agent B's 50,000", held=150_000, ceiling=150_000)

    a2 = f.order_for("agent-A-order-2", "A", 1)
    f.other_pots_cover("before agent A's 1 unit", 1, cash=True)
    f.place(a2, "budget refused: agent A orders 1 unit from seller 1; the shared budget of 150,000 is fully held by the two agents' orders",
        part="1", leg="budget", changed="budget, 1 unit below what the order needs", refuse=exhausted)
    f.check_budget("after agent A's refused 1 unit", held=150_000, ceiling=150_000)
    f.set_budget("buyer's owner raises the shared budget by 1 unit to 150,001", 150_001)
    f.place(a2, "ACCEPTED: agent A orders 1 unit from seller 1 after the owner added 1 unit to the shared budget",
        part="1", leg="budget", changed="nothing")
    f.check_budget("after agent A's 1 unit", held=150_001, ceiling=150_001)
    f.h.check("the lender wallet still holds its full 1,000,000", f.lender_cash()["wallet"], LENDER_CASH)


def shared_wallet(f):
    """Parts 2 to 4. The lender wallet's 150,000 is the tight pot; the budget is 500,000."""
    f.cast(budget=BUDGET, lender_cash=150_000)
    buyer, lender, seller1, seller2 = [f.parties[x] for x in ("company", "financier", "supplier", "assignee")]
    core = lambda name, code: dict(program=CORE, name=name, code=code, index=1)
    seller1_ata, seller2_ata = seller1["owner_ata"], seller2["owner_ata"]
    f.check_wallet("at the start", wallet=150_000, promised=0, free=150_000)

    # Part 2: two sellers, one lender wallet.
    a = f.order_for("agent-A-order-1", "A", 100_000)
    f.place(a, "ACCEPTED: agent A orders 100,000 from seller 1; lender wallet holds 150,000 with nothing promised",
        part="2", leg="wallet", changed="nothing")
    f.check_wallet("after seller 1's 100,000", wallet=150_000, promised=100_000, free=50_000)
    f.other_pots_cover("before seller 2's 100,000", 100_000, budget=True)
    f.place(f.order_for("agent-B-order-1", "B", 100_000),
        "money refused: agent B orders 100,000 from seller 2; 50,000 of the lender wallet's 150,000 is free after seller 1's order",
        part="2", leg="wallet", changed="amount, 50,000 above the lender's free cash",
        refuse=dict(program=CORE, name="FundingCashShortfall", code=6033))
    f.check_wallet("after seller 2's refused 100,000", wallet=150_000, promised=100_000, free=50_000)
    b = f.order_for("agent-B-order-1", "B", 50_000)
    f.place(b, "ACCEPTED: agent B orders 50,000 from seller 2; exactly the lender wallet's free cash",
        part="2", leg="wallet", changed="nothing")
    f.check_wallet("after seller 2's 50,000", wallet=150_000, promised=150_000, free=0)
    f.check_budget("after both orders", held=150_000, ceiling=BUDGET)
    custody = f.state("FinancierCustody", lender["custody"])
    f.h.check("the lender's cash record names one token account", custody["token_account"], lender["custody_token"])
    for seller, tab in (("seller 1", a), ("seller 2", b)):
        claim = f.state("FundedClaim", tab["claim"])
        f.h.check(f"{seller}'s Tab record is backed by that same lender cash record", claim["custody"], lender["custody"])
        f.h.check(f"{seller}'s Tab record names its own payout key", claim["terms"]["supplier"],
            f.parties[tab["seller_role"]]["owner"].pubkey())
    f.h.check("seller 1 has received nothing yet", f.h.amount(seller1_ata), 0)
    f.h.check("seller 2's balance is its 200,000 of test funds", f.h.amount(seller2_ata), 200_000)
    f.withdraw_case("one-unit-while-all-promised", 1,
        "money refused: lender's owner withdraws 1 unit while all 150,000 is promised to the two sellers",
        part="2", changed="amount, 1 unit above free cash of 0", refuse=True)

    # Parts 3 and 4: each claim stands on its own; agent A is revoked first.
    f.send("buyer's owner revokes agent A after its order was accepted", f.ix("principal_registry", "revoke_actor",
        dict(owner=buyer["owner"].pubkey(), principal=f.company, actor=f.actors[0]), expected_generation=1),
        [buyer["owner"]])
    f.h.check("agent A is revoked", f.registry.read(f.h.svm, f.actors[0], "Actor")["active"], False)
    run_a = f.earn(a, "seller-1-work", 100_000,
        label="seller 1 reports 100,000 of completed work on agent A's order; agent A is already revoked")
    untouched = f.h.snapshot([b["claim"], b["reservation"]])
    with f.paid_by(f.collector):
        with f.selling("assignee"):
            f.collect_as(f.collection(b, run_a, 50_000),
                "claims refused: collection of 50,000 against seller 2's order using seller 1's work record; seller 2 has reported no work",
                part="3", changed="work record belongs to another seller's order", refuse=core("FundingRun", 6028))
            absent = pda(CORE, b"usage", seller2["principal"], b["namespace"], ident("completed-run-seller-2-none"))
            f.h.check("seller 2 has no work record at the address tried next", f.h.svm.get_account(absent), None)
            f.collect_as(f.with_accounts(f.collection(b, run_a, 50_000), run=absent),
                "claims refused: collection of 50,000 against seller 2's order naming a work record that does not exist",
                part="3", changed="work record does not exist", refuse=core("AccountNotInitialized", 3012))
        wallet_before = f.h.amount(lender["custody_token"])
        row = f.collect_as(f.collection(a, run_a, 100_000),
            "ACCEPTED: an outside key collects seller 1's earned 100,000 from the shared wallet; agent A is revoked",
            part="3", changed="nothing")
        tx = VersionedTransaction.from_bytes(__import__("base64").b64decode(row["transaction_base64"]))
        f.h.check("seller 1's collection carries one signature, the outside key's",
            [str(k) for k in tx.message.account_keys[:len(tx.signatures)]], [str(f.collector.pubkey())])
        f.h.check("revoked agent A is not among the collection's account keys",
            str(f.agent.pubkey()) in row["account_keys"], False)
        f.h.check("seller 1 receives exactly its 100,000", f.h.amount(seller1_ata), 100_000)
        f.h.check("the shared wallet pays exactly 100,000", wallet_before - f.h.amount(lender["custody_token"]), 100_000)
        f.h.check("seller 2's Tab record and budget reservation are byte-identical after seller 1 is paid",
            f.h.snapshot([b["claim"], b["reservation"]]), untouched)
        f.check_wallet("after seller 1 is paid", wallet=50_000, promised=50_000, free=0)
        f.collect_as(f.collection(a, run_a, 1),
            "claims refused: seller 1's order, paid in full, is used to collect 1 more unit from the 50,000 promised to seller 2",
            part="3", changed="amount, 1 unit above what seller 1 earned", refuse=core("FundingTerms", 6031))
    f.withdraw_case("one-unit-of-seller-2-promise", 1,
        "money refused: lender's owner withdraws 1 unit of the 50,000 still promised to seller 2",
        part="3", changed="amount, 1 unit above free cash of 0", refuse=True)

    with f.selling("assignee"):
        run_b = f.earn(b, "seller-2-work", 20_000, label="seller 2 reports 20,000 of completed work on agent B's order")
        with f.paid_by(f.collector):
            f.collect_as(f.collection(b, run_b, 20_000),
                "ACCEPTED: an outside key collects seller 2's earned 20,000 from the shared wallet",
                part="3", changed="nothing")
    f.h.check("seller 2 receives exactly its 20,000", f.h.amount(seller2_ata), 220_000)
    f.h.check("seller 1's balance is unchanged by seller 2's collection", f.h.amount(seller1_ata), 100_000)
    f.check_wallet("after seller 2 is paid 20,000", wallet=30_000, promised=30_000, free=0)
    f.withdraw_case("released-30000", 30_000,
        "money refused: lender's owner withdraws 30,000 while it is still promised to seller 2's order",
        part="3", changed="seller 2 has not released its unused 30,000", refuse=True)
    with f.selling("assignee"):
        f.release(b, "seller-2-unused-30000", 30_000)
    f.relabel("seller 2 releases the unused 30,000 of its order")
    f.check_wallet("after seller 2's release", wallet=30_000, promised=0, free=30_000)
    f.withdraw_case("released-30000-plus-one", 30_001,
        "money refused: lender's owner withdraws 30,001 when 30,000 is free",
        part="3", changed="amount, 1 unit above free cash", refuse=True)
    f.withdraw_case("released-30000", 30_000,
        "ACCEPTED: lender's owner withdraws the released 30,000", part="3", changed="nothing")
    f.check_wallet("at the end", wallet=0, promised=0, free=0)
    f.h.check("lender's owner holds the 30,000 it withdrew", f.h.amount(lender["owner_ata"]), 30_000)
    f.check_budget("at the end", held=0, consumed=120_000, ceiling=BUDGET)
    f.h.check("every unit of the 150,000 is accounted for: 100,000 to seller 1, 20,000 to seller 2, 30,000 back to the lender",
        f.h.amount(seller1_ata) + (f.h.amount(seller2_ata) - 200_000) + f.h.amount(lender["owner_ata"]), 150_000)


def run(suite, exercise):
    f = SharedReserve()
    try:
        exercise(f)
    except Exception as error:
        f.finish(suite, error=error)
        raise
    index = case_index(f, suite)
    for row in f.h.cases:
        if "commit_legs" in row:
            row["shared_reserve"] = row.pop("commit_legs")
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
            annotated_acceptances=sum(row["outcome"] == "accepted" for row in annotated))
        (directory / (suite + "-receipt.json")).write_text(json.dumps(receipts[suite], indent=2) + "\n")
        rows.extend(index)
    (directory / "case-index.json").write_text(json.dumps(rows, indent=1, default=json_value) + "\n")
    f = results[0][0]
    for name in ("build-receipt.json", "idl-receipt.json"):
        (directory / name).write_bytes((OUT / name).read_bytes())
    manifest = dict(runs=receipts, run_seconds=round(seconds, 2), engine="solders 0.29.0 LiteSVM",
        program_sha256={name: value["sha256"] for name, value in f.build["artifacts"].items()},
        swig_sha256=SWIG_SHA256, build_receipt_sha256=digest(OUT / "build-receipt.json"),
        idl_receipt_sha256=digest(OUT / "idl-receipt.json"))
    (directory / "shared-reserve-receipt.json").write_text(json.dumps(manifest, indent=2) + "\n")
    return manifest


def main():
    started = time.monotonic()
    results = [run(suite, exercise) for suite, exercise in zip(RUNS, (shared_budget, shared_wallet))]
    if "--record" in sys.argv:
        print(json.dumps(record(results, sys.argv[sys.argv.index("--record") + 1], time.monotonic() - started)))


if __name__ == "__main__":
    main()
