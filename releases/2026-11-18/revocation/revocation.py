#!/usr/bin/env python3
"""After the owner revokes the agent: the next order fails, the seller still collects.

One buyer, one agent, one seller, one credit issuer and one lender, in the
order the events are told:

  1. The agent places three orders, each with a maximum of 100,000. All accepted.
  2. The buyer's owner revokes the agent.
  3. The revoked agent signs a fourth order of 100,000 on the same terms: refused.
     It then tries the two things it can still send about an existing order.
  4. After the revocation the seller reports 60,000 and 100,000 of work on
     orders 1 and 2 and nothing on order 3. An outside key collects both
     amounts to the seller. Unused amounts are released.
  5. End state: debt, order records and the agent's record.
  6. The owner enrolls a new agent, which places a fifth order.

Numbers are those of commit_legs.py: per-order limit 100,000, budget 500,000,
facility and credit line 500,000, lender wallet 1,000,000, no interest and no
seller fee. Refusals are checked the same way: transaction error, first failing
program frame, Anchor error name and number from that frame's log, and
byte-identical state for every writable account except the fee payer.

The shared setup enrolls a second agent key that this scenario never uses. No
account bytes are written by the harness. The harness sets the clock once, to
7,201 seconds, for the last release.

Record: <python> foundation/onchain/tests/revocation.py --record docs/evidence/<directory>
"""
from commit_legs import *

REPORTED = {1: 60_000, 2: 100_000}


class Revocation(CommitLegs):
    def place(self, tab, label, *, part, changed, refuse=None):
        self.send(label, self.commit_instruction(tab), tab["signers"], reject=refuse is not None, receipt=tab["receipt"])
        return self.note(section=part, leg="revocation", changed=changed, refuse=refuse)

    def collect_as(self, request, label, *, part, changed, refuse=None):
        lender = self.parties["financier"]
        self.send(label, self.envelope(compact_sign(lender["config"], lender["wallet"], request["instruction"])),
            request["signers"], reject=refuse is not None, receipt=request["receipt"])
        return self.note(section=part, leg="revocation", changed=changed, refuse=refuse)

    def release_case(self, tab, name, amount, label, *, signer, part, changed, refuse=None):
        """release_financier_unused with an explicit choice of who signs in the seller's slot."""
        seller, lender = self.parties["supplier"], self.parties[tab["financier"]]
        raw = ident("release-" + name)
        operation_id = self.operation(b"release", self.h.payer.pubkey(), raw)
        receipt = self.receipt(self.company, operation_id)
        instruction = self.ix("claims_core", "release_financier_unused",
            dict(principal=self.company, supplier_principal=seller["principal"],
                supplier=None if signer is None else signer.pubkey(), budget=self.budget,
                reservation=tab["reservation"], registry_operation=pda(REGISTRY, b"operation", self.company, operation_id),
                line=tab["line"], claim=tab["claim"], facility=tab["facility"], custody=lender["custody"],
                custody_token=lender["custody_token"], receipt=receipt),
            remaining=self.remaining(["company", "issuer", "supplier", tab["financier"]]),
            request_id=raw, amount=amount, expected_revision=self.state("FundedClaim", tab["claim"])["revision"],
            service_without_pool=False)
        self.send(label, instruction, [] if signer is None else [signer], reject=refuse is not None, receipt=receipt)
        return self.note(section=part, leg="revocation", changed=changed, refuse=refuse)

    def held(self):
        budget = self.registry.read(self.h.svm, self.budget, "Budget")
        return dict(held=budget["reserved"], consumed=budget["consumed"], ceiling=budget["ceiling"])

    def signers_of(self, row):
        tx = VersionedTransaction.from_bytes(__import__("base64").b64decode(row["transaction_base64"]))
        return [str(k) for k in tx.message.account_keys[:len(tx.signatures)]]


def exercise(f):
    f.scenario()
    buyer, issuer, lender, seller = [f.parties[x] for x in ("company", "issuer", "financier", "supplier")]
    agent = str(f.agent.pubkey())
    seller_ata = f.h.ata(seller["owner"].pubkey(), f.mint)
    for name in ("earn_financed_run", "collect_financier", "release_financier_unused"):
        accounts = [account["name"] for account in f.core.instructions[name]["accounts"]]
        f.h.check(f"{name} takes no agent, agent-key or permission account",
            [x for x in accounts if x in ("actor", "credential", "mandate")], [])
    for name in ("consume_completed_run", "release"):
        accounts = [account["name"] for account in f.registry.instructions[name]["accounts"]]
        f.h.check(f"Registry {name} takes no agent, agent-key or permission account",
            [x for x in accounts if x in ("actor", "credential", "mandate")], [])

    # 1. Three orders accepted.
    orders = {}
    for number in (1, 2, 3):
        orders[number] = f.order(f"order-{number}")
        f.place(orders[number], f"ACCEPTED: the agent places order {number}, maximum 100,000, with the seller",
            part="1", changed="nothing")
        f.h.check(f"budget hold after order {number}", f.held()["held"], number * ORDER)
        f.h.check(f"lender promised total after order {number}", f.lender_cash()["promised"], number * ORDER)
    accepted = {number: f.h.snapshot([tab["claim"], tab["reservation"], tab["receipt"]]) for number, tab in orders.items()}
    terms = {number: f.state("FundedClaim", tab["claim"])["terms"] for number, tab in orders.items()}
    f.h.check("budget hold with three orders accepted", f.held(), dict(held=300_000, consumed=0, ceiling=BUDGET))
    cash = f.lender_cash()
    f.h.check("lender promised total with three orders accepted", cash["promised"], 300_000)
    f.h.check("lender free cash with three orders accepted", cash["free"], LENDER_CASH - 300_000)
    f.h.check("no tokens have left the lender wallet", cash["wallet"], LENDER_CASH)
    f.h.check("the seller has received nothing", f.h.amount(seller_ata), 0)

    # 2. The owner revokes the agent.
    f.send("buyer's owner revokes the agent", f.ix("principal_registry", "revoke_actor",
        dict(owner=buyer["owner"].pubkey(), principal=f.company, actor=f.actors[0]), expected_generation=1),
        [buyer["owner"]])
    f.note(section="2", leg="revocation", changed="nothing")
    actor = f.registry.read(f.h.svm, f.actors[0], "Actor")
    f.h.check("the agent's record shows it revoked", actor["active"], False)
    f.h.check("the agent's record generation moved from 1 to 2", actor["generation"], 2)
    f.h.check("revoking the agent leaves the three order records byte-identical",
        {number: f.h.snapshot([tab["claim"], tab["reservation"], tab["receipt"]]) for number, tab in orders.items()}, accepted)
    f.h.check("revoking the agent leaves the budget hold at 300,000", f.held()["held"], 300_000)
    f.h.check("revoking the agent leaves the lender's promise at 300,000", f.lender_cash()["promised"], 300_000)

    # 3. The revoked agent's fourth order, and what it can still send about an existing order.
    f.slack("before the fourth order")
    f.place(f.order("order-4"), "refused: the revoked agent signs a fourth order of 100,000 with the seller on the same terms",
        part="3", changed="the agent was revoked", refuse=dict(program=REGISTRY, name="CredentialInactive", code=6004))
    f.h.check("budget hold after the refused fourth order", f.held()["held"], 300_000)
    f.h.check("lender promised total after the refused fourth order", f.lender_cash()["promised"], 300_000)
    row = f.place(orders[1], "no effect: the revoked agent sends accepted order 1 again; the program returns the original outcome and changes nothing",
        part="3", changed="an already accepted order is sent again by the revoked agent")
    f.h.check("re-sending accepted order 1 changes no observed account", row["observed_after"], row["observed_before"])
    f.h.check("re-sending accepted order 1 does not call Registry", f"Program {REGISTRY} invoke [2]" in row["result"]["logs"], False)
    f.release_case(orders[3], "revoked-agent-tries", ORDER,
        "refused: the revoked agent signs a release of order 3's unused 100,000 in the seller's place",
        signer=f.agent, part="3", changed="release signed by the agent instead of the seller",
        refuse=dict(program=CORE, name="FundingReleaseAuthority", code=6043))

    # 4. After the revocation the seller reports work and is paid.
    runs = {}
    for number, amount in REPORTED.items():
        runs[number] = f.earn(orders[number], f"order-{number}-work", amount,
            label=f"ACCEPTED: after the revocation the seller reports {amount:,} of completed work on order {number}")
        row = f.note(section="4", leg="revocation", changed="nothing")
        f.h.check(f"order {number}: the work report is signed by the fee payer and the seller only",
            sorted(f.signers_of(row)), sorted([str(f.h.payer.pubkey()), str(seller["owner"].pubkey())]))
        f.h.check(f"order {number}: the revoked agent's key is not among the work report's account keys",
            agent in row["account_keys"], False)
    with f.paid_by(f.collector):
        for number, amount in REPORTED.items():
            before = f.h.amount(seller_ata)
            row = f.collect_as(f.collection(orders[number], runs[number], amount),
                f"ACCEPTED: after the revocation an outside key collects order {number}'s {amount:,} from the lender wallet to the seller",
                part="4", changed="nothing")
            f.h.check(f"order {number}: the collection's only signer is the outside key",
                f.signers_of(row), [str(f.collector.pubkey())])
            f.h.check(f"order {number}: the revoked agent's key is not among the collection's account keys",
                agent in row["account_keys"], False)
            f.h.check(f"order {number}: the seller receives exactly {amount:,}", f.h.amount(seller_ata) - before, amount)
        f.release_case(orders[3], "outside-key-before-expiry", ORDER,
            "refused: an outside key releases order 3's unused 100,000 without the seller's signature before the order expires",
            signer=None, part="4", changed="seller's signature absent before the order's expiry",
            refuse=dict(program=CORE, name="FundingReleaseAuthority", code=6043))
    f.h.check("the seller received exactly 160,000", f.h.amount(seller_ata), 160_000)
    f.h.check("the lender wallet paid exactly 160,000", f.h.amount(lender["custody_token"]), LENDER_CASH - 160_000)
    row = f.release_case(orders[1], "order-1-unused", 40_000,
        "ACCEPTED: the seller signs the release of order 1's unused 40,000",
        signer=seller["owner"], part="4", changed="nothing")
    f.h.check("order 1's release is signed by the fee payer and the seller only",
        sorted(f.signers_of(row)), sorted([str(f.h.payer.pubkey()), str(seller["owner"].pubkey())]))
    f.h.check("budget after order 1's release", f.held(), dict(held=100_000, consumed=160_000, ceiling=BUDGET))
    f.h.check("lender promise after order 1's release is order 3's 100,000", f.lender_cash()["promised"], 100_000)

    # 6. A new agent, enrolled after the revocation, places a fifth order.
    new_actor = pda(REGISTRY, b"actor", f.company, ident("actor-new"))
    f.send("buyer's owner enrolls a new agent with a new key", f.ix("principal_registry", "create_actor",
        dict(owner=buyer["owner"].pubkey(), principal=f.company, actor=new_actor),
        args=dict(actor_id=ident("actor-new"), current_key=f.replacement_agent.pubkey(),
            valid_from=NOW - 100, valid_until=NOW + ENROLLMENT_SECONDS)), [buyer["owner"]])
    f.note(section="6", leg="revocation", changed="nothing")
    f.actors.append(new_actor)
    new_permission = f.permit("agent-new", 2)
    f.h.cases[-1]["name"] = ("buyer's owner permits the new agent: this seller, this lender facility, "
        "per-order limit 100,000, for 7,200 seconds")
    fifth = f.order("order-5")
    fifth["terms"].update(actor=new_actor, mandate=new_permission)
    fifth["accounts"].update(actor=new_actor, mandate=new_permission)
    f.signed_by(fifth, f.replacement_agent)
    f.place(fifth, "ACCEPTED: the new agent places a fifth order, maximum 100,000, with the seller",
        part="6", changed="nothing")
    f.h.check("budget after the fifth order", f.held(), dict(held=200_000, consumed=160_000, ceiling=BUDGET))
    f.h.check("the fifth order's record names the new agent", f.state("FundedClaim", fifth["claim"])["terms"]["actor"], new_actor)
    f.h.check("the revoked agent's record is still revoked", f.registry.read(f.h.svm, f.actors[0], "Actor")["active"], False)

    # 4, continued. Order 3's unused amount after the order expires.
    f.clock(PERMISSION_SECONDS + 1)
    with f.paid_by(f.collector):
        row = f.release_case(orders[3], "outside-key-after-expiry", ORDER,
            "ACCEPTED: at 7,201 seconds, after order 3 expired, an outside key releases its unused 100,000 with no other signature",
            signer=None, part="4", changed="nothing")
    f.h.check("order 3's release after expiry is signed by the outside key only", f.signers_of(row), [str(f.collector.pubkey())])
    f.h.check("the revoked agent's key is not among that release's account keys", agent in row["account_keys"], False)

    # 5. End state.
    expected = {1: dict(debt=60_000, paid=60_000, payable=0, remaining=0, released=40_000),
        2: dict(debt=100_000, paid=100_000, payable=0, remaining=0, released=0),
        3: dict(debt=0, paid=0, payable=0, remaining=0, released=100_000)}
    for number, tab in orders.items():
        claim = f.state("FundedClaim", tab["claim"])
        for field, value in expected[number].items():
            f.h.check(f"order {number} record {field} at the end", claim[field], value)
        f.h.check(f"order {number} record still carries the terms the revoked agent signed", claim["terms"], terms[number])
        f.h.check(f"order {number} record still names the revoked agent's record", claim["terms"]["actor"], f.actors[0])
        f.h.check(f"order {number} acceptance receipt is byte-identical to when it was accepted",
            f.h.snapshot([tab["receipt"]]), {k: v for k, v in accepted[number].items() if k == str(tab["receipt"])})
    f.h.check("the buyer's debt on the three orders equals what was collected",
        sum(f.state("FundedClaim", tab["claim"])["debt"] for tab in orders.values()), 160_000)
    for number, amount in REPORTED.items():
        run = f.state("FundedRun", runs[number]["run"])
        f.h.check(f"order {number} work record: the debtor is the buyer", run["debtor"], f.company)
        f.h.check(f"order {number} work record: the debt's first owner is the credit issuer", run["current_creditor"], issuer["principal"])
        f.h.check(f"order {number} work record: ownership has not changed hands", run["ownership_revision"], 1)
        f.h.check(f"order {number} work record: the creditor's cash record is the credit issuer's", run["creditor_custody"], issuer["custody"])
        f.h.check(f"order {number} work record: principal outstanding", run["principal"]["outstanding"], amount)
        f.h.check(f"order {number} work record: paid to the seller", run["gross_paid"], amount)
    f.positions("company", customer_principal=160_000)
    f.positions("issuer", owned_principal=160_000, unpaid_supplier=0)
    f.positions("supplier", cash_received=160_000, supplier_receivable=0)
    f.h.check("budget at the end: 160,000 consumed, 100,000 held for the fifth order",
        f.held(), dict(held=100_000, consumed=160_000, ceiling=BUDGET))
    f.h.check("lender promise at the end is the fifth order's 100,000", f.lender_cash()["promised"], 100_000)
    actor = f.registry.read(f.h.svm, f.actors[0], "Actor")
    f.h.check("the agent's record at the end: revoked", (actor["active"], actor["generation"]), (False, 2))
    f.h.check("the agent's record at the end still holds the revoked key", actor["current_key"], f.agent.pubkey())


def record(f, summary, index, directory, seconds):
    """Copy the finished run into a publication directory; no second execution."""
    directory = Path(directory).resolve()
    directory.mkdir(parents=True, exist_ok=True)
    raw = Path(summary["trace"]).read_bytes()
    packed = directory / "revocation-trace.json.xz"
    packed.write_bytes(lzma.compress(raw))
    assert lzma.decompress(packed.read_bytes()) == raw
    (directory / "case-index.json").write_text(json.dumps(index, indent=1, default=json_value) + "\n")
    annotated = [row for row in index if row["section"] != "setup"]
    receipt = dict(summary, trace=packed.name, trace_xz_sha256=digest(packed),
        annotated_cases=len(annotated),
        annotated_refusals=sum(row["outcome"] == "refused" for row in annotated),
        annotated_acceptances=sum(row["outcome"] == "accepted" for row in annotated),
        run_seconds=round(seconds, 2), engine="solders 0.29.0 LiteSVM",
        program_sha256={name: value["sha256"] for name, value in f.build["artifacts"].items()},
        swig_sha256=SWIG_SHA256, build_receipt_sha256=digest(OUT / "build-receipt.json"),
        idl_receipt_sha256=digest(OUT / "idl-receipt.json"))
    for name in ("build-receipt.json", "idl-receipt.json"):
        (directory / name).write_bytes((OUT / name).read_bytes())
    (directory / "revocation-receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    return receipt


def main():
    started = time.monotonic()
    f = Revocation()
    try:
        exercise(f)
    except Exception as error:
        f.finish("revocation", error=error)
        raise
    index = case_index(f, "revocation")
    for row in f.h.cases:
        if "commit_legs" in row:
            row["revocation"] = row.pop("commit_legs")
    summary = f.finish("revocation")
    if "--record" in sys.argv:
        print(json.dumps(record(f, summary, index, sys.argv[sys.argv.index("--record") + 1], time.monotonic() - started)))


if __name__ == "__main__":
    main()
