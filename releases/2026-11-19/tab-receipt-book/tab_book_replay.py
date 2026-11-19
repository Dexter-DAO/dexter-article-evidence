#!/usr/bin/env python3
"""Close each Tab the moment it settles; then nothing it signed works again.

Two order profiles run their whole life on Change 5's programs, on one buyer
mandate whose order window admits each number once:

  ordinary  order 5: commit (the agent, the credit issuer's owner and the seller
            sign), the seller's capture of 6,000 (earn), collection to the
            seller, the seller's release of the unused 4,000, and the buyer's two
            numbered repayments of 3,000. The second settles the Tab and carries
            every close.
  evidence  order 20: commit naming a meter, the meter's signed report of 900
            units, the seller's signed claim of 1,400, finalization (the bill
            earns 900 and releases 1,100), collection, and the buyer's two
            numbered repayments of 450. The second settles the Tab and carries
            every close.

An outside keeper pays for each settling transaction. Every deposit returns to
the payer recorded when its record was created: the commit's payer for the Tab,
its hold and the bill; the capture's payer (the seller) or the finalizer for the
run; the first repayment's payer (the buyer's owner) for the repayment record.

Then, for each profile:
  1. Every step's transaction is sent again, first as its original bytes and
     then rebuilt with a fresh blockhash and re-signed by the same keys with the
     same instructions. All are refused and nothing changes. The commit is
     refused by the mandate's order window (OrderUsed); the other steps name
     records the close removed.
  2. Every signed record is presented against new Tabs at nearby order numbers
     (n-1, n+1, n+2): the commit's signatures on the commit rebuilt for the
     other number; the seller's signed capture rebuilt for the new Tab; the
     meter's report and the seller's claim, whose Ed25519 digests name the bill
     and the Tab, presented to the new Tab's bill; and the buyer's numbered
     repayment rebuilt for the new Tab's route. Each is refused because what was
     signed names the old Tab or number. Freshly signed records for the new Tab
     land.
  3. Reuse after close: the closed Tab's address cannot be committed again; the
     old run id billed under the new Tab is a distinct run (trade-off A); and
     the old run's address can never hold a run again.

The harness writes no account bytes. The clock never moves: both evidence
records are admitted at the start, so the bill finalizes at once.

Run in a prepared checkout: <python> foundation/onchain/tests/tab_book_replay.py
"""
import base64

from commit_legs import *
from evidence_billing import *
from financing_commercial import CommercialFixture
from tab_closes import settlement_closes, before_close, verify_close, unobserve_payers, lamports

ORDINARY_CAP, CAPTURED, UNUSED, REPAID = 10_000, 6_000, 4_000, 3_000
EVIDENCE_CAP, SIGNED, CLAIMED = 2_000, 900, 1_400
RUNTIME = ("SignatureFailure", "AlreadyProcessed", "BlockhashNotFound")


def rebuild(row):
    """A recorded step's instructions exactly as they were sent."""
    return [Instruction(Pubkey.from_string(ix["program_id"]), bytes.fromhex(ix["data_hex"]),
        [M(Pubkey.from_string(m["pubkey"]), m["is_signer"], m["is_writable"]) for m in ix["ordered_metas"]])
        for ix in row["instructions"]]


def signed_message(row):
    return VersionedTransaction.from_bytes(base64.b64decode(row["transaction_base64"]))


def promote(instruction, signers):
    """Mark the keys that sign as signers, as the fixture's send does for owner
    fields that may also be passkey aliases."""
    supplied = {signer.pubkey() for signer in signers}
    return Instruction(instruction.program_id, bytes(instruction.data),
        [M(m.pubkey, m.is_signer or m.pubkey in supplied, m.is_writable) for m in instruction.accounts])


class TabBookReplay(CommitLegs, EvidenceBillingFixture):
    repayment = CommercialFixture.repayment

    def __init__(self):
        super().__init__()
        self.keeper = key(143)
        self.h.svm.airdrop(self.keeper.pubkey(), 100_000_000_000)
        self.results = dict(profiles={})

    # -- keys and recorded steps -------------------------------------------------
    def keypair(self, address):
        known = [self.h.payer, self.agent, self.agent2, self.meter, self.other_meter, self.submitter, self.finalizer,
            self.collector, self.replacement_collector, self.keeper] + [p["owner"] for p in self.parties.values()]
        return {str(k.pubkey()): k for k in known}[str(address)]

    def order_at(self, name, number, *, index=0):
        """The fixture's allocator names this order `number` on agent `index`'s mandate."""
        self.orders[(str(self.mandates[index]), name)] = number

    def step(self, profile, label):
        """Remember the last transaction as a step of this profile's life."""
        row = self.h.cases[-1]
        assert row["passed"] and row["expected"] == "success", label
        profile["steps"].append(dict(label=label, row=row))
        return row

    def refusal(self, row, expected):
        """The exact refusal of the last send: a runtime error or a program's Anchor error."""
        found = classify(row)
        if expected in RUNTIME:
            assert found["err"] == expected and not found["log_lines"], (row["name"], found)
            return dict(error=expected, kind="runtime, before any program runs",
                signatures_verified=row["signature_verified"])
        anchor = found["anchor_error"]
        assert anchor is not None and anchor["name"] == expected, (row["name"], found)
        return dict(error=expected, kind="program", program=found["raising_program"], origin=anchor["origin"],
            number=anchor["number"], instruction_index=found["instruction_index"])

    def protected_except(self, payer):
        """Every observed account except the fee payer of the attempt."""
        return [address for address in self.h.observe if address != payer]

    def refused(self, label, instructions, signers, payer, expected, *, transaction=None):
        """Send and require refusal with `expected` (one of RUNTIME when None is
        given for a literal resend), every protected account unchanged."""
        with self.fee_payer(payer):
            self.h.send(label, instructions, signers, fail=expected or "any", unchanged=self.protected_except(payer.pubkey()),
                transaction=transaction)
        row = self.h.cases[-1]
        if expected is None:
            expected = classify(row)["err"]
            assert expected in RUNTIME, (label, expected)
        return self.refusal(row, expected)

    def forged(self, original, instructions):
        """`instructions` signed by nobody: the original transaction's signatures on a
        message that differs only in what the instructions name."""
        old = signed_message(original)
        payer = self.keypair(original["account_keys"][0])
        signers = [self.keypair(k) for k in original["account_keys"][1:len(old.signatures)]]
        with self.fee_payer(payer):
            fresh = self.h.transaction(instructions, signers)
        assert [str(k) for k in fresh.message.account_keys[:len(old.signatures)]] \
            == original["account_keys"][:len(old.signatures)], "the same keys sign in the same order"
        assert bytes(fresh.message) != bytes(old.message)
        return VersionedTransaction.populate(fresh.message, old.signatures), payer

    # -- settlement ----------------------------------------------------------------
    def settle(self, profile, tab, request, runs, label, deposit_payers):
        """The settling repayment and every close in one transaction, paid by the keeper."""
        record = request["record"]
        closes, accounts, extra, claim = settlement_closes(self, tab, runs=runs, records=[(runs[0], record)])
        before = before_close(self, tab, accounts, extra)
        observed = set(self.protected)
        with self.fee_payer(self.keeper):
            self.send(label, [request["instruction"], *closes], request["signers"], receipt=request["receipt"])
        row = self.step(profile, "settling repayment 2 with every close")
        unobserve_payers(self, before, observed)
        result = verify_close(self, tab, profile["name"], before, payers=deposit_payers)
        for kind, (address, payer) in extra.items():
            event = "RunClosed" if kind.startswith("run") else "RepaymentRecordClosed"
            field = "run" if event == "RunClosed" else "record"
            closed = [value for _, name, value in self.h.ledger.events if name == event and value[field] == address]
            self.h.check(f"{profile['name']}: {kind} closes once, its deposit to its payer",
                [(value["payer"], value["lamports"]) for value in closed], [(payer, before["deposits"][kind])])
        self.h.check(profile["name"] + ": the settling transaction is one transaction with the repayment and every close",
            [ix["program_id"] for ix in row["instructions"]], [str(CORE)] * (1 + len(closes)))
        state = {kind: str(address) for kind, (address, _) in before["records"].items()}
        payers = {}
        for kind, (_, payer) in before["records"].items():
            payers.setdefault(str(payer), dict(records=[], lamports=0))
            payers[str(payer)]["records"].append(kind)
            payers[str(payer)]["lamports"] += before["deposits"][kind]
        result.update(records=state, deposits_by_payer=payers, keeper=str(self.keeper.pubkey()),
            settling_wire_bytes=row["wire_bytes"], settling_compute_units=row["result"]["compute_units_consumed"])
        profile["close"] = result
        return result


def ordinary_life(f):
    """Order 5: commit, capture, collection, release, two numbered repayments; the second settles and closes."""
    company, supplier = f.parties["company"], f.parties["supplier"]
    profile = dict(name="ordinary order 5", order=5, steps=[])
    f.order_at("ordinary-5", 5)
    tab = f.acceptance("ordinary-5", cap=ORDINARY_CAP, maximum_interest=0, fees=False)
    f.send("ordinary order 5: the agent, the credit issuer's owner and the seller commit 10,000",
        f.commit_instruction(tab), tab["signers"], receipt=tab["receipt"])
    f.step(profile, "commit")
    with f.fee_payer(supplier["owner"]):
        run = f.earn(tab, "ordinary-5-capture", CAPTURED,
            label="ordinary order 5: the seller captures 6,000 of completed work, paying its own fee")
    f.step(profile, "capture")
    with f.fee_payer(f.collector):
        f.collect(f.collection(tab, run, CAPTURED),
            label="ordinary order 5: an outside key collects 6,000 from the lender's escrow to the seller")
    f.step(profile, "collection")
    f.release(tab, "ordinary-5-unused", UNUSED)
    f.step(profile, "release")
    with f.fee_payer(company["owner"]):
        first = f.repayment(tab, run, "ordinary-5-repay-1", REPAID, payer="company", creditor="issuer", maximum_interest=0)
        f.send("ordinary order 5: the buyer repays 3,000, repayment number 1 on its route",
            first["instruction"], first["signers"], receipt=first["receipt"])
    f.step(profile, "repayment 1")
    f.h.check("ordinary order 5: the buyer's first repayment carries number 1", first["args"]["number"], 1)
    with f.fee_payer(f.keeper):
        second = f.repayment(tab, run, "ordinary-5-repay-2", REPAID, payer="company", creditor="issuer", maximum_interest=0)
    f.h.check("ordinary order 5: the settling repayment carries number 2", second["args"]["number"], 2)
    f.settle(profile, tab, second, [run["run"]],
        "ordinary order 5: the buyer repays the last 3,000 (number 2) and the same transaction closes the "
        "repayment record, the run and the Tab with its hold; an outside keeper pays",
        {"tab": f.h.payer.pubkey(), "hold": f.h.payer.pubkey(), "run 1": supplier["owner"].pubkey(),
            "repayment record 1": company["owner"].pubkey()})
    profile.update(tab=tab, run=run, repayments=[first, second])
    return profile


def evidence_life(f):
    """Order 20: commit, meter report, seller claim, finalization, collection, two repayments; the second settles."""
    company = f.parties["company"]
    profile = dict(name="evidence order 20", order=20, steps=[])
    f.order_at("evidence-20", 20)
    tab = f.evidence_tab("evidence-20", cap=EVIDENCE_CAP, rates=((1, 1, EVIDENCE_CAP),))
    f.accept_evidence(tab, label="evidence order 20: the agent, the credit issuer's owner and the seller commit 2,000 "
        "naming the meter")
    f.step(profile, "commit")
    report = f.report(tab, units=(SIGNED,))
    f.submit_report(tab, report, label="evidence order 20: the meter's signed report of 900 units is admitted")
    f.step(profile, "meter report")
    claim = f.monetary_claim(tab, CLAIMED)
    f.submit_claim(tab, claim, label="evidence order 20: the seller's signed claim of 1,400 is admitted")
    f.step(profile, "seller claim")
    f.finalize(tab, label="evidence order 20: an outside key finalizes the bill: 900 earned, 1,100 released")
    f.step(profile, "finalization")
    f.assert_final_amount(tab, SIGNED, early=True)
    f.collect_from(tab, SIGNED, label="evidence order 20: an outside key collects the bill of 900 to the seller")
    f.step(profile, "collection")
    run = dict(run=tab["settlement"]["run"])
    with f.fee_payer(company["owner"]):
        first = f.repayment(tab, run, "evidence-20-repay-1", SIGNED // 2, payer="company", creditor="issuer",
            maximum_interest=0)
        f.send("evidence order 20: the buyer repays 450, repayment number 1 on its route",
            first["instruction"], first["signers"], receipt=first["receipt"])
    f.step(profile, "repayment 1")
    with f.fee_payer(f.keeper):
        second = f.repayment(tab, run, "evidence-20-repay-2", SIGNED // 2, payer="company", creditor="issuer",
            maximum_interest=0)
    f.settle(profile, tab, second, [run["run"]],
        "evidence order 20: the buyer repays the last 450 (number 2) and the same transaction closes the "
        "repayment record, the run and the Tab with its hold and bill; an outside keeper pays",
        {"tab": f.h.payer.pubkey(), "hold": f.h.payer.pubkey(), "bill": f.h.payer.pubkey(),
            "run 1": f.finalizer.pubkey(), "repayment record 1": company["owner"].pubkey()})
    f.h.check("evidence order 20: the empty recoverable-binding address stays empty",
        f.h.svm.get_account(pda(CORE, b"bill-authority", tab["bill"])) is None, True)
    profile.update(tab=tab, run=run, report=report, claim=claim, repayments=[first, second],
        report_proof=profile["steps"][1]["row"], claim_proof=profile["steps"][2]["row"])
    return profile


EXPECTED_STEP_REFUSALS = {
    # step: (error, why) for the step re-signed with a fresh blockhash
    "commit": ("OrderUsed", "the mandate's order window already holds this number"),
}


def replay_steps(f, profile):
    """Send every step again, as its original bytes and re-signed; all refused, nothing changes."""
    attempts = []
    watched = [profile["tab"]["mandate"], f.budget, f.line, f.facility] + [p[k] for p in f.parties.values()
        for k in ("custody", "book", "custody_token", "owner_ata") if k in p]
    before = f.h.snapshot(watched)
    for step in profile["steps"]:
        row = step["row"]
        payer = f.keypair(row["account_keys"][0])
        signers = [f.keypair(k) for k in row["account_keys"][1:len(signed_message(row).signatures)]]
        literal = f.refused(f"replay of {profile['name']} {step['label']}: the original signed bytes",
            rebuild(row), (), payer, None, transaction=signed_message(row))
        expected = EXPECTED_STEP_REFUSALS.get(step["label"], ("AccountNotInitialized", "the close removed its records"))
        resigned = f.refused(f"replay of {profile['name']} {step['label']}: re-signed by the same keys with a fresh blockhash",
            rebuild(row), signers, payer, expected[0])
        if expected[0] == "AccountNotInitialized":
            account = resigned["origin"].removeprefix("caused by account: ")
            resigned["account"] = account
        attempts.append(dict(step=step["label"], original_signature=row["result"]["meta"]["signature"]
            if "meta" in row["result"] else row["result"]["signature"], literal=literal, resigned=resigned,
            why=expected[1]))
    f.h.check(profile["name"] + ": no replay changed the mandate, budget, line, facility, custody, books or token accounts",
        f.h.snapshot(watched), before)
    profile["replays"] = attempts
    return attempts


def commit_variant(f, profile, number, *, evidence=False):
    """The profile's commit rebuilt for order `number`: same request and terms, another number."""
    original = profile["tab"]
    name = f"{'evidence' if evidence else 'ordinary'}-{profile['order']}-as-{number}"
    f.order_at(name, number)
    if evidence:
        tab = f.evidence_tab(name, cap=EVIDENCE_CAP, rates=((1, 1, EVIDENCE_CAP),), policy_overrides=dict(
            work_id=original["billing_policy"]["work_id"], request_hash=original["billing_policy"]["request_hash"]))
        tab["evidence_args"]["request_id"] = original["evidence_args"]["request_id"]
        instruction = f.ix("claims_core", "accept_evidence_financed_tab", tab["evidence_accounts"],
            remaining=f.remaining(["company", "issuer", "supplier", tab["financier"]]), **tab["evidence_args"])
    else:
        tab = f.acceptance(name, cap=ORDINARY_CAP, maximum_interest=0, fees=False)
        tab["args"]["request_id"] = original["args"]["request_id"]
        instruction = f.commit_instruction(tab)
    return tab, promote(instruction, tab["signers"])


def nearby_commits(f, profile, *, evidence=False):
    """The commit's signatures on the commit rebuilt for n-1, n+1 and n+2: refused by the runtime."""
    n, commit_row = profile["order"], profile["steps"][0]["row"]
    attempts = []
    for number in (n - 1, n + 1, n + 2):
        tab, instruction = commit_variant(f, profile, number, evidence=evidence)
        transaction, payer = f.forged(commit_row, [instruction])
        refusal = f.refused(f"{profile['name']}: its commit signatures on the same commit rebuilt for order {number}",
            [instruction], (), payer, "SignatureFailure", transaction=transaction)
        attempts.append(dict(record="buyer's commit (agent, credit issuer's owner and seller signatures)",
            against=f"order {number}", bound=dict(signed_order=n, presented_order=number,
                signed_tab=str(profile["tab"]["claim"]), presented_tab=str(tab["claim"]),
                signed_hold=str(profile["tab"]["reservation"]), presented_hold=str(tab["reservation"])), **refusal))
        f.h.check(f"{profile['name']}: no Tab exists at order {number} after the refused rebuilt commit",
            f.h.svm.get_account(tab["claim"]) is None, True)
    return attempts


def fresh_commit(f, profile, number, *, evidence=False, same_work=False):
    """A new order at `number`, signed afresh by the same parties: it lands."""
    name = f"{'evidence' if evidence else 'ordinary'}-{number}"
    f.order_at(name, number)
    original = profile["tab"]
    if evidence:
        overrides = dict(work_id=original["billing_policy"]["work_id"],
            request_hash=original["billing_policy"]["request_hash"]) if same_work else None
        tab = f.evidence_tab(name, cap=EVIDENCE_CAP, rates=((1, 1, EVIDENCE_CAP),), policy_overrides=overrides)
        f.accept_evidence(tab, label=f"{profile['name']}: a new evidence order {number}, signed afresh, lands")
    else:
        tab = f.acceptance(name, cap=ORDINARY_CAP, maximum_interest=0, fees=False)
        f.send(f"{profile['name']}: a new order {number}, signed afresh by the same parties, lands",
            f.commit_instruction(tab), tab["signers"], receipt=tab["receipt"])
    f.h.check(f"{profile['name']}: order {number} is a new Tab at a new address",
        (tab["claim"] != original["claim"], f.state("FundedClaim", tab["claim"])["terms"]["order"]), (True, number))
    return tab


def ordinary_records(f, profile):
    """Ordinary order 5's signed records against new Tabs at orders 4, 6 and 7."""
    supplier, company = f.parties["supplier"], f.parties["company"]
    attempts = nearby_commits(f, profile)
    landed = []
    lower = fresh_commit(f, profile, 4)
    landed.append(dict(record="new commit", order=4, note="order 4 is below the mandate's highest and was never used"))
    new = fresh_commit(f, profile, 6)
    landed.append(dict(record="new commit", order=6))

    # The seller's capture, signed for Tab 5, rebuilt for Tab 6 with the same run id.
    capture_row = profile["steps"][1]["row"]
    old_run = profile["run"]
    run_id = old_run["args"]["run_id"]
    args = dict(old_run["args"], expected_steps=f.steps(new["claim"]))
    accounts = dict(old_run["accounts"], reservation=new["reservation"], line=new["line"], claim=new["claim"],
        run=run_address(new["claim"], run_id))
    with f.fee_payer(supplier["owner"]):
        instruction = f.ix("claims_core", "earn_financed_run", accounts,
            remaining=f.remaining(["company", "issuer", "supplier", new["financier"]]), **args)
    transaction, payer = f.forged(capture_row, [instruction])
    refusal = f.refused(f"{profile['name']}: the seller's signed capture rebuilt for order 6",
        [instruction], (), payer, "SignatureFailure", transaction=transaction)
    attempts.append(dict(record="seller's capture (seller's signature)", against="order 6",
        bound=dict(signed_tab=str(profile["tab"]["claim"]), presented_tab=str(new["claim"]),
            signed_run=str(old_run["run"]), presented_run=str(run_address(new["claim"], run_id))), **refusal))

    # Fresh: the seller captures the same work id under Tab 6. Trade-off A: a
    # run id is unique per Tab, so the same work bills once more there.
    with f.fee_payer(supplier["owner"]):
        run = f.earn(new, "ordinary-5-capture", CAPTURED,
            label=f"{profile['name']}: the seller signs a new capture of the same run id under order 6; it lands")
    f.h.check("trade-off A: the old run id under order 6 is a distinct run address",
        (run["run"] != old_run["run"], run["run"] == run_address(new["claim"], run_id)), (True, True))
    f.h.check("the old run's address holds no account", f.h.svm.get_account(old_run["run"]) is None, True)
    landed.append(dict(record="new capture of the same run id", order=6, run=str(run["run"]),
        old_run=str(old_run["run"]), trade_off="A: cross-Tab run-id uniqueness is dropped; the supplier SDK binds "
        "each delivered run to one Tab before billing"))
    with f.fee_payer(f.collector):
        f.collect(f.collection(new, run, CAPTURED), label=f"{profile['name']}: an outside key collects order 6's 6,000")

    # The buyer's numbered repayments, signed for Tab 5's route, rebuilt for Tab
    # 6's route with the same request, amount and number (each under the key
    # that paid for it: the buyer's owner, then the keeper).
    for index, original in enumerate(profile["repayments"], 1):
        repay_row = profile["steps"][3 + index]["row"]
        with f.fee_payer(f.keypair(repay_row["account_keys"][0])):
            rebuilt = f.repayment(new, run, f"ordinary-5-repay-{index}", REPAID, payer="company", creditor="issuer",
                maximum_interest=0, number=original["args"]["number"])
        transaction, payer = f.forged(repay_row, [rebuilt["instruction"]])
        refusal = f.refused(f"{profile['name']}: the buyer's signed repayment number {original['args']['number']} "
            "rebuilt for order 6's route", [rebuilt["instruction"]], (), payer, "SignatureFailure", transaction=transaction)
        attempts.append(dict(record=f"buyer's repayment number {original['args']['number']} (buyer owner's signature)",
            against="order 6's repayment route", bound=dict(signed_record=str(original["record"]),
                presented_record=str(rebuilt["record"]), signed_run=str(old_run["run"]), presented_run=str(run["run"]),
                number=original["args"]["number"]), **refusal))
    with f.fee_payer(company["owner"]):
        fresh = f.repayment(new, run, "ordinary-6-repay-1", REPAID, payer="company", creditor="issuer", maximum_interest=0)
        f.send(f"{profile['name']}: the buyer signs a new repayment for order 6, number 1 on that route; it lands",
            fresh["instruction"], fresh["signers"], receipt=fresh["receipt"])
    landed.append(dict(record="new repayment", order=6, number=fresh["args"]["number"], route=str(fresh["record"])))
    profile.update(record_replays=attempts, landed=landed, new_tab=new, lower_tab=lower, new_run=run)
    return attempts


def evidence_records(f, profile):
    """Evidence order 20's signed records against new Tabs at orders 19, 21 and 22."""
    company = f.parties["company"]
    attempts = nearby_commits(f, profile, evidence=True)
    landed = []
    fresh_commit(f, profile, 19, evidence=True)
    landed.append(dict(record="new evidence commit", order=19, note="order 19 is below the mandate's highest and was never used"))
    new = fresh_commit(f, profile, 21, evidence=True, same_work=True)
    landed.append(dict(record="new evidence commit for the same work and request", order=21))
    old = profile["tab"]

    # The meter's report and the seller's claim, signed over order 20's bill and
    # Tab, presented to order 21's bill. Same report and claim bytes, same policy
    # facts; only the bill, the Tab and so the policy hash differ.
    for record, proof_row, body, error_label in (
            ("meter's report (Ed25519 over the report digest)", profile["report_proof"], profile["report"], "report"),
            ("seller's claim (Ed25519 over the claim digest)", profile["claim_proof"], profile["claim"], "claim")):
        proof = rebuild(proof_row)[0]
        domain, report_kind = (REPORT_DOMAIN, True) if error_label == "report" else (CLAIM_DOMAIN, False)
        signed_digest = phase_digest(domain, f.domain, old["bill"], old["claim"],
            report_bytes(body) if report_kind else seller_claim_bytes(body), old["policy_hash"])
        expected_digest = phase_digest(domain, f.domain, new["bill"], new["claim"],
            report_bytes(body) if report_kind else seller_claim_bytes(body), new["policy_hash"])
        f.h.check(f"the presented {error_label} proof signs order 20's digest", bytes(proof.data[112:144]), signed_digest)
        if report_kind:
            f.submit_report(new, body, proof=proof, error="BillingSignature",
                label=f"{profile['name']}: the meter's signed report for order 20 presented to order 21's bill")
        else:
            f.submit_claim(new, body, proof=proof, error="BillingSignature",
                label=f"{profile['name']}: the seller's signed claim for order 20 presented to order 21's bill")
        row = f.h.cases[-1]
        attempts.append(dict(record=record, against="order 21's bill", bound=dict(
            signed_bill=str(old["bill"]), presented_bill=str(new["bill"]), signed_tab=str(old["claim"]),
            presented_tab=str(new["claim"]), signed_digest=signed_digest.hex(), digest_order_21_needs=expected_digest.hex()),
            **f.refusal(row, "BillingSignature")))
    # Fresh: the same report and claim bytes, signed over order 21's bill and Tab, land.
    report, claim = copy.deepcopy(profile["report"]), copy.deepcopy(profile["claim"])
    f.submit_report(new, report, label=f"{profile['name']}: the meter signs the same report for order 21's bill; it lands")
    f.submit_claim(new, claim, label=f"{profile['name']}: the seller signs the same claim for order 21's bill; it lands")
    landed += [dict(record="new meter report", order=21), dict(record="new seller claim", order=21)]
    f.finalize(new, label=f"{profile['name']}: an outside key finalizes order 21: 900 earned, 1,100 released")
    f.assert_final_amount(new, SIGNED, early=True)
    f.h.check("trade-off A: the same work id under order 21 is a distinct run",
        (new["settlement"]["run"] != old["settlement"]["run"], f.h.svm.get_account(old["settlement"]["run"]) is None),
        (True, True))
    landed.append(dict(record="finalization of the same work under order 21", order=21, run=str(new["settlement"]["run"]),
        old_run=str(old["settlement"]["run"]), trade_off="A (spec section 10 items 6-7): a work id is unique per Tab"))
    f.collect_from(new, SIGNED, label=f"{profile['name']}: an outside key collects order 21's bill of 900")
    run = dict(run=new["settlement"]["run"])
    original = profile["repayments"][0]
    with f.fee_payer(company["owner"]):
        rebuilt = f.repayment(new, run, "evidence-20-repay-1", SIGNED // 2, payer="company", creditor="issuer",
            maximum_interest=0, number=original["args"]["number"])
    transaction, payer = f.forged(profile["steps"][5]["row"], [rebuilt["instruction"]])
    refusal = f.refused(f"{profile['name']}: the buyer's signed repayment number 1 rebuilt for order 21's route",
        [rebuilt["instruction"]], (), payer, "SignatureFailure", transaction=transaction)
    attempts.append(dict(record="buyer's repayment number 1 (buyer owner's signature)", against="order 21's repayment route",
        bound=dict(signed_record=str(original["record"]), presented_record=str(rebuilt["record"]),
            signed_run=str(old["settlement"]["run"]), presented_run=str(run["run"]), number=1), **refusal))
    with f.fee_payer(company["owner"]):
        fresh = f.repayment(new, run, "evidence-21-repay-1", SIGNED // 2, payer="company", creditor="issuer",
            maximum_interest=0, number=1)
        f.send(f"{profile['name']}: the buyer signs a new repayment for order 21, number 1 on that route; it lands",
            fresh["instruction"], fresh["signers"], receipt=fresh["receipt"])
    landed.append(dict(record="new repayment", order=21, number=1, route=str(fresh["record"])))
    profile.update(record_replays=attempts, landed=landed, new_tab=new)
    return attempts


def reuse_after_close(f, ordinary, evidence):
    """The closed Tabs' addresses cannot be committed again; their runs' addresses never hold a run again."""
    results = []
    for profile, evidence_order in ((ordinary, False), (evidence, True)):
        n = profile["order"]
        name = f"{'evidence' if evidence_order else 'ordinary'}-{n}-again"
        f.order_at(name, n)
        if evidence_order:
            tab = f.evidence_tab(name, cap=EVIDENCE_CAP, rates=((1, 1, EVIDENCE_CAP),))
            instruction = f.ix("claims_core", "accept_evidence_financed_tab", tab["evidence_accounts"],
                remaining=f.remaining(["company", "issuer", "supplier", tab["financier"]]), **tab["evidence_args"])
        else:
            tab = f.acceptance(name, cap=ORDINARY_CAP, maximum_interest=0, fees=False)
            instruction = f.commit_instruction(tab)
        f.h.check(f"{profile['name']}: the new request names the closed Tab's address", tab["claim"], profile["tab"]["claim"])
        refusal = f.refused(f"{profile['name']}: a new commit, signed afresh with a new request, recreates order {n}'s address",
            [promote(instruction, tab["signers"])], tab["signers"], f.h.payer, "OrderUsed")
        results.append(dict(attempt=f"recreate order {n}'s Tab with a fresh signed commit", **refusal))
    # The old run's address: a capture naming closed Tab 5 and its old run, signed afresh by the seller.
    old_run, tab = ordinary["run"], ordinary["tab"]
    supplier = f.parties["supplier"]["owner"]
    with f.fee_payer(supplier):
        instruction = f.ix("claims_core", "earn_financed_run", old_run["accounts"],
            remaining=f.remaining(["company", "issuer", "supplier", tab["financier"]]), **dict(old_run["args"], expected_steps=0))
    refusal = f.refused("ordinary order 5: the seller signs a fresh capture naming closed Tab 5 and its old run address",
        [instruction], [supplier], supplier, "AccountNotInitialized")
    results.append(dict(attempt="bill the old run id at its old address, under closed Tab 5", **refusal,
        why="the run address derives from Tab 5's address, Tab 5 no longer exists, and order 5 can never be "
            "committed again (OrderUsed, later OrderBelowWindow), so no run can ever be created there"))
    f.h.check("the old run's address still holds no account", f.h.svm.get_account(old_run["run"]) is None, True)
    for profile in (ordinary, evidence):
        f.h.check(f"{profile['name']}: its Tab and hold addresses still hold no account after every replay",
            (f.h.svm.get_account(profile["tab"]["claim"]), f.h.svm.get_account(profile["tab"]["reservation"])), (None, None))
    mandate = f.registry.read(f.h.svm, tab["mandate"], "Mandate")
    f.h.check("the mandate's window keeps orders 5 and 20 used", (mandate["window"]["highest"] >= 21), True)
    return results


def exercise(f):
    f.setup(financier_wallet=False)
    ordinary = ordinary_life(f)
    evidence = evidence_life(f)
    profiles = dict(ordinary=ordinary, evidence=evidence)
    for profile in profiles.values():
        replay_steps(f, profile)
    ordinary_records(f, ordinary)
    evidence_records(f, evidence)
    reuse = reuse_after_close(f, ordinary, evidence)
    f.results = dict(reuse_after_close=reuse, profiles={key_: dict(
        name=p["name"], order=p["order"], tab=str(p["tab"]["claim"]),
        steps=[dict(step=s["label"], name=s["row"]["name"]) for s in p["steps"]],
        close=p["close"], step_replays=p["replays"], record_replays=p["record_replays"], fresh_records=p["landed"])
        for key_, p in profiles.items()})


def main():
    encoding_vectors()
    f = TabBookReplay()
    try:
        exercise(f)
    except Exception as error:
        f.finish("tab-book-replay", error=error)
        raise
    summary = f.finish("tab-book-replay")
    result = dict(schema="dexter-tab-book-replay/v1", status="passed", transport="native-v1 LiteSVM",
        public_transactions_sent=0, native_suite=summary, **f.results,
        boundary="Compiled-program local execution of the dd70820 build; no deployment or public RPC")
    path = OUT / "tab-book-replay-scenarios.json"
    path.write_text(json.dumps(result, indent=2, default=json_value) + "\n")
    print(json.dumps(dict(scenarios=str(path), sha256=digest(path))))


if __name__ == "__main__":
    main()
