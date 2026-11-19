#!/usr/bin/env python3
"""A rule signs for delivery: the delivery-rules program with the real programs.

An evidence order can name a rule program as its delivery signer
(early_settlement.py): `policy.source` is the program's address
["delivery-source", policy.source_identity], and ClaimsCore admits a report only
when that address signs, by CPI. The delivery-rules program
(foundation/onchain/programs/delivery-rules) is such a program. This module loads
its retained binary (foundation/onchain/programs/delivery-rules/artifacts), by
pinned hash, with no upgrade authority. Its rules state:

    who signs for delivery    up to five keys; a group is `threshold` of them
    silence                   with fewer than `threshold` attestations, a silent signer
                              supports zero ("silence pays nothing") or the policy's
                              maximum units ("paid unless someone objects")
    who may object, by when   up to five keys; objections close object_seconds after signing closes
    who decides an objection  one key, after signing closes; the decision closes
                              decide_seconds (at least an hour) after objections close
    if the decider is silent  zero, or the objection lapses

The signers' outcome: each group of `threshold` signers supports, per metric, the
smallest amount its members support. Once `threshold` signers attested, only
attesting signers form groups; with fewer, every signer does and a silent one
supports the silence value. Groups are priced with the order's policy; the
highest-priced group is reported and, among equal prices, the group with the
lowest mask (signer i = bit i).

A rule lives at ["rule", sha256("DEXTER:DELIVERY-RULE:1" || terms)] and no
instruction changes or closes it. An order names a rule with source_identity =
the rule's address. Its case lives at ["case", bill]; anyone opens it (and pays
its rent), which fixes the timeline from the order's work deadline; the order's
evidence deadline must be at least MIN_SETTLE_SECONDS after the decision
deadline. Anyone settles once the outcome is fixed; the program then logs the
case record, hashes it into the report's result_hash, and reports into
ClaimsCore by CPI.

Main run: pinned_delivery.py's fixture (one buyer, two agents, one seller, a
carrier, an inspector, one credit issuer, a lender wallet) plus two more
carriers. Orders have a maximum of 2,000 at a price of 1 per unit. Work ends
20 seconds after the start; reports and claims are admitted until ten days
after that. The rules give one day to sign, three more to object and five more
to decide.

Refusals are checked as in pinned_terms.py: transaction error, first failing
program frame, Anchor error name and number, the source location in the failing
frame's log line (table `RAISED`), and byte-identical state for every writable
account except the fee payer. The SDK's checkDeliveryRuleOrder runs on the run's
accounts through delivery_rules_sdk_check.mjs, which needs ./dev client build.

Run in a prepared checkout: ./dev anchor-smoke delivery-rules
Record: <python> foundation/onchain/tests/delivery_rules.py --record docs/evidence/<directory>
"""
import base64
import shutil
import subprocess

from solders.system_program import transfer, TransferParams

from early_settlement import EarlySettlement, delivery_source, program_data, LOADER
from pinned_delivery import *

RULES = Pubkey.from_string("EsR1knVgJG8NAYFjeGCYpTuvjf1DtwAU1ne5XBdcotq3")
RETAINED = ROOT / "foundation/onchain/programs/delivery-rules/artifacts/delivery-rules-retained.so"
RETAINED_SHA256 = "704831da874812b856d47806083726bd4bfefe9b43fe526b3fba3c70029f9f0f"
SDK_CHECK = Path(__file__).resolve().parent / "delivery_rules_sdk_check.mjs"
UPGRADEABLE_RULES = key(110).pubkey()     # the same ELF, with an upgrade authority
UPGRADE_AUTHORITY = key(111).pubkey()
PROGRAM_NAMES[str(RULES)] = "delivery_rules"
PROGRAM_NAMES[str(SYSTEM)] = "system"
CAP, SIGNED, CLAIMED = 2_000, 900, 1_400
DAY = 86_400
MIN_SETTLE = 3_600
MIN_DECIDE = 3_600
WORK = 20                                   # the order's work deadline, seconds after the start
SIGNING_CLOSES = WORK + DAY
OBJECTIONS_CLOSE = SIGNING_CLOSES + 3 * DAY
DECISION_CLOSES = OBJECTIONS_CLOSE + 5 * DAY
EVIDENCE = WORK + 10 * DAY                  # the orders' report and claim deadlines
RULE_DOMAIN = b"DEXTER:DELIVERY-RULE:1"
OUTCOME_DOMAIN = b"DEXTER:DELIVERY-RULE-OUTCOME:1"
NOTHING, PAID, LAPSE = 0, 1, 1
SIGNERS, DECIDED, DECIDER_SILENT = 1, 2, 3
TWO_METRICS = ((1, 1, 1_500), (2, 1, 250))  # pairs at 1, express deliveries at 2
RULES_ERRORS = dict(InvalidRule=6000, NotABill=6001, RuleNotNamed=6002, BillClosed=6003, RuleOutlastsOrder=6004,
    NotASigner=6005, AlreadyAttested=6006, SigningClosed=6007, UnitsOutsidePolicy=6008, NotAnObjector=6009,
    ObjectionsClosed=6010, AlreadyObjected=6011, NotTheDecider=6012, NoObjection=6013, DecisionClosed=6014,
    AlreadyDecided=6015, ObjectionsOpen=6016, DecisionOpen=6017, AlreadySettled=6018, BillNotFinal=6019,
    SigningOpen=6020)
ANCHOR_ERRORS = dict(ConstraintHasOne=2001, ConstraintSeeds=2006, AccountNotInitialized=3012)
CORE_ERRORS = dict(BillingDeadline=6065, BillingSourceProgram=6078)
REGISTRY_ERRORS = dict(TermsNotPermitted=6053)
RULES_SOURCE = "thrown in programs/delivery-rules/src/lib.rs:"
# Written here from the source at this commit and asserted for every refusal.
RAISED = {
    "rule terms": RULES_SOURCE + "125",
    "bill owner": RULES_SOURCE + "160",
    "bill account": RULES_SOURCE + "162",
    "units": RULES_SOURCE + "167",
    "rule named": RULES_SOURCE + "190",
    "bill closed": RULES_SOURCE + "191",
    "timeline": RULES_SOURCE + "197",
    "settled attest": RULES_SOURCE + "207",
    "signer": RULES_SOURCE + "210",
    "attested": RULES_SOURCE + "211",
    "signing closed": RULES_SOURCE + "212",
    "settled object": RULES_SOURCE + "222",
    "objector": RULES_SOURCE + "224",
    "objections closed": RULES_SOURCE + "225",
    "objected": RULES_SOURCE + "226",
    "settled decide": RULES_SOURCE + "235",
    "decider": RULES_SOURCE + "237",
    "no objection": RULES_SOURCE + "238",
    "signing open": RULES_SOURCE + "240",
    "decision closed": RULES_SOURCE + "241",
    "decided": RULES_SOURCE + "242",
    "settled": RULES_SOURCE + "253",
    "decision open": RULES_SOURCE + "260",
    "objections open": RULES_SOURCE + "266",
    "bill final": RULES_SOURCE + "294",
    "rule account": "caused by account: rule",
    "case account": "caused by account: case",
    "rule program": "thrown in programs/claims-core/src/evidence_billing.rs:168",
    "core deadline": "thrown in programs/claims-core/src/evidence_billing.rs:301",
    "pinned term": "thrown in programs/principal-registry/src/lib.rs:363",
}


def refusal(name, where=None):
    """What a refusal must show: raising program, error name and number, and where it is raised."""
    if name == "AccountAlreadyInUse":   # the system program's allocate, inside the rule's creation
        return dict(program=SYSTEM, name=None, code=0, source=None)
    if name == "InsufficientFundsForRent":   # the runtime's rent check after execution
        return dict(runtime_error={"InsufficientFundsForRent": {"account_index": 1}}, source=None)
    program = REGISTRY if name in REGISTRY_ERRORS else CORE if name in CORE_ERRORS else RULES
    code = {**RULES_ERRORS, **ANCHOR_ERRORS, **CORE_ERRORS, **REGISTRY_ERRORS}[name]
    return dict(program=program, name=name, code=code, source=RAISED[where])


def rule_bytes(t):
    """Independent fixed-width Borsh encoding of RuleTerms."""
    raw = b"".join(bytes(k) for k in t["signers"]) + bytes([t["signer_count"], t["threshold"], t["signers_silent"]])
    raw += struct.pack("<I", t["sign_seconds"]) + b"".join(bytes(k) for k in t["objectors"])
    raw += bytes([t["objector_count"]]) + struct.pack("<I", t["object_seconds"]) + bytes(t["decider"])
    raw += struct.pack("<I", t["decide_seconds"]) + bytes([t["decider_silent"]])
    assert len(raw) == 369
    return raw


def case_bytes(c):
    """Independent Borsh encoding of the case record: what settle logs and hashes."""
    raw = bytes(c["bill"]) + bytes(c["rule"]) + bytes(c["opener"])
    raw += struct.pack("<qqqB", c["signing_closes"], c["objections_close"], c["decision_closes"], c["attested"])
    raw += b"".join(struct.pack("<8Q", *units) for units in c["attestations"])
    raw += b"\0" if c["objector"] is None else b"\1" + bytes(c["objector"])
    raw += b"\0" if c["decision"] is None else b"\1" + struct.pack("<8Q", *c["decision"])
    return raw + bytes([c["outcome"], c["group"]]) + struct.pack("<8Q", *c["units"])


def rule_address(t):
    return pda(RULES, b"rule", hashlib.sha256(RULE_DOMAIN + rule_bytes(t)).digest())


def case_address(bill):
    return pda(RULES, b"case", bill)


def rule_terms(signers, threshold, *, signers_silent=NOTHING, objectors=(), decider=None, decider_silent=NOTHING,
        decide=5 * DAY):
    """A rule on this run's timeline: one day to sign, three more to object, five more (or `decide` seconds) to decide."""
    pad = lambda keys: list(keys) + [Pubkey.default()] * (5 - len(keys))
    return dict(signers=pad(signers), signer_count=len(signers), threshold=threshold, signers_silent=signers_silent,
        sign_seconds=DAY, objectors=pad(objectors), objector_count=len(objectors), object_seconds=3 * DAY,
        decider=decider or Pubkey.default(), decide_seconds=decide, decider_silent=decider_silent)


def units8(units):
    return list(units) + [0] * (8 - len(units))


def clock_text(seconds):
    days, rest = divmod(seconds, DAY)
    return f"{days} d {rest} s" if days else f"{rest} s"


class DeliveryRules(EarlySettlement):
    """pinned_delivery.py's fixture with the retained delivery-rules binary at its declared address."""

    def load_rules(self):
        self.rules = Idl(OUT / "idl/delivery_rules.json")
        assert self.rules.program == RULES
        assert self.idl_receipt["artifacts"]["delivery_rules.json"]["sha256"] == digest(self.rules.path)
        self.idls["delivery_rules"] = self.rules
        self.rule_sha256 = self.h.load(RULES, RETAINED)
        assert self.rule_sha256 == RETAINED_SHA256, ("retained delivery-rules binary", self.rule_sha256)
        self.retained_equals_build = self.build["artifacts"]["delivery_rules.so"]["sha256"] == RETAINED_SHA256
        assert self.h.load(UPGRADEABLE_RULES, RETAINED, upgrade_authority=UPGRADE_AUTHORITY) == self.rule_sha256
        for program, authority in ((RULES, None), (UPGRADEABLE_RULES, UPGRADE_AUTHORITY)):
            account, data = self.h.svm.get_account(program), self.h.svm.get_account(program_data(program))
            self.h.check(f"rules program {program} is an executable upgradeable-loader program",
                (account.executable, account.owner, bytes(account.data)),
                (True, LOADER, (2).to_bytes(4, "little") + bytes(program_data(program))))
            self.h.check(f"rules program {program}: ProgramData upgrade authority",
                (bytes(data.data[:4]), data.data[12], bytes(data.data[13:45]) if data.data[12] else None),
                ((3).to_bytes(4, "little"), 0 if authority is None else 1, None if authority is None else bytes(authority)))
        self.carrier, self.carrier_b, self.carrier_c = self.meter, key(100), key(101)
        self.inspector, self.settler = self.other_meter, key(102)
        self.buyer, self.seller = self.parties["company"]["owner"], self.parties["supplier"]["owner"]
        for holder in (self.carrier, self.carrier_b, self.carrier_c, self.inspector, self.settler):
            self.h.svm.airdrop(holder.pubkey(), 100_000_000_000)
        self.settle_logs = {}

    def refused(self, row, *, source, runtime_error=None, **expected):
        if runtime_error is not None:
            found = classify(row)
            row["passed"] = False
            assert row["expected"] == "reject" and row["observed_after"] == row["observed_before"], row["name"]
            assert found["err"] == runtime_error, (row["name"], found["err"])
            payer = row["account_keys"][0]
            writable = {meta["pubkey"] for item in row["instructions"] for meta in item["ordered_metas"]
                if meta["is_writable"]} - {payer}
            assert writable <= set(row["rollback_accounts"]), row["name"]
            row["commit_legs"].update(outcome="refused", refusal=dict(found, kind="runtime",
                raising_program="runtime", writable_accounts_unchanged=len(writable)))
            row["passed"] = True
            return
        if source is None:
            return CommitLegs.refused(self, row, **expected)
        return SourceChecked.refused(self, row, source=source, **expected)

    def read_rules(self, kind, address):
        account = self.h.svm.get_account(address)
        assert account is not None and account.owner == RULES and not account.executable, (kind, address)
        assert bytes(account.data[:8]) == bytes(self.rules.accounts[kind]["discriminator"])
        state, used = self.rules.decode({"defined": {"name": kind}}, account.data, 8)
        assert not any(account.data[used:]), "noncanonical trailing bytes in " + kind
        return state

    # -- rules ---------------------------------------------------------------
    def create_rule(self, t, label, *, case, what_changed="nothing", address=None, refuse=None):
        address = address or rule_address(t)
        self.h.check("rule terms: canonical bytes match the generated interface",
            self.rules.encode({"defined": {"name": "RuleTerms"}}, t), rule_bytes(t))
        self.send(label, self.ix("delivery_rules", "create_rule", dict(rule=address), terms=t), reject=refuse is not None)
        self.mark(case, "create rule", what_changed, refuse=refuse)
        if refuse is None:
            self.h.check(label + ": the rule holds its terms at the address of their hash", self.read_rules("Rule", address)["terms"], t)
            account = self.h.svm.get_account(address)
            self.h.check(label + ": the rule account holds exactly its rent", account.lamports,
                self.h.svm.minimum_balance_for_rent_exemption(len(account.data)))
        return address

    def prefund(self, address, lamports, label, *, case, what_changed="nothing", refuse=None):
        """A transfer to an address before the program creates an account there."""
        self.observe(address)
        self.send(label, transfer(TransferParams(from_pubkey=self.h.payer.pubkey(), to_pubkey=address, lamports=lamports)),
            reject=refuse is not None)
        self.mark(case, "transfer", what_changed, refuse=refuse)

    # -- orders ----------------------------------------------------------------
    def rule_order(self, name, rule, *, program=RULES, evidence=EVIDENCE, rates=((1, 1, CAP),), source=None, index=0):
        """An evidence order naming `rule`: source_identity = the rule's address, source = its delivery-source address."""
        self.agent_index = index
        try:
            tab = self.evidence_tab(name, cap=CAP, rates=rates, policy_overrides=dict(
                source=source or delivery_source(program, bytes(rule)), source_identity=bytes(rule),
                evidence_deadline=NOW + evidence, claim_deadline=NOW + EVIDENCE))
        finally:
            self.agent_index = 0
        tab.update(rule=rule, case=case_address(tab["bill"]))
        self.observe(tab["case"])
        return tab

    def place_rule(self, tab, label, *, case, **options):
        row = self.place_program(tab, label, case=case, rule=RULES, **options)
        if options.get("refuse") is None:
            bill = self.bill_state(tab)
            self.h.check(label + ": the bill names the rule and its delivery-source address",
                (bytes(bill["policy"]["source_identity"]), bill["policy"]["source"]),
                (bytes(tab["rule"]), delivery_source(RULES, bytes(tab["rule"]))))
            self.h.check(label + ": admitted at the start", bill["admitted_at"], NOW)
        return row

    def sdk_check(self, tab):
        """checkDeliveryRuleOrder from the built SDK, on the rule account as it is in this run."""
        node = shutil.which("node")
        assert node and (ROOT / "foundation/onchain/client/dist/delivery-rules.js").is_file(), \
            "Build the SDK first: ./dev client build"
        account = self.h.svm.get_account(tab["rule"])
        policy = tab["billing_policy"]
        payload = dict(rule=None if account is None else dict(address=str(tab["rule"]), owner=str(account.owner),
                executable=account.executable, lamports=str(account.lamports), dataBase64=base64.b64encode(account.data).decode()),
            policy=dict(source=str(policy["source"]), sourceIdentity=bytes(policy["source_identity"]).hex(),
                workUntil=str(policy["work_until"]), evidenceDeadline=str(policy["evidence_deadline"])))
        result = subprocess.run([node, str(SDK_CHECK)], input=json.dumps(payload), capture_output=True, text=True, timeout=60)
        assert result.returncode == 0, result.stderr
        return json.loads(result.stdout)

    # -- cases -----------------------------------------------------------------
    def open_case(self, tab, label, *, case, what_changed="nothing", opener=None, rule=None, bill=None, refuse=None,
            evidence=EVIDENCE):
        opener = opener or self.seller
        bill = bill or tab["bill"]
        address = case_address(bill)
        self.observe(address)
        before = self.h.svm.get_account(address)
        with self.fee_payer(opener):
            self.send(label, self.ix("delivery_rules", "open_case", dict(rule=rule or tab["rule"], bill=bill, case=address)),
                reject=refuse is not None)
        row = self.mark(case, "open case", what_changed, refuse=refuse)
        if refuse is not None:
            self.h.check("no case exists: " + label, self.h.svm.get_account(address) == before, True)
            return row
        state = self.read_rules("Case", address)
        policy = tab["billing_policy"]
        decide = self.read_rules("Rule", tab["rule"])["terms"]["decide_seconds"]
        self.h.check(label + ": the case records the bill, the rule, the opener and the deadlines",
            {k: state[k] for k in ("bill", "rule", "opener", "signing_closes", "objections_close", "decision_closes")},
            dict(bill=tab["bill"], rule=tab["rule"], opener=opener.pubkey(), signing_closes=NOW + SIGNING_CLOSES,
                objections_close=NOW + OBJECTIONS_CLOSE, decision_closes=NOW + OBJECTIONS_CLOSE + decide))
        self.h.check(label + ": the order's evidence deadline is at least an hour after the decision deadline",
            policy["evidence_deadline"] - state["decision_closes"] >= MIN_SETTLE, True)
        self.h.check(label + ": nothing is attested, objected, decided or settled",
            (state["attested"], state["objector"], state["decision"], state["outcome"]), (0, None, None, 0))
        account = self.h.svm.get_account(address)
        self.h.check(label + ": the case holds exactly its rent", account.lamports,
            self.h.svm.minimum_balance_for_rent_exemption(len(account.data)))
        tab["case_rent"] = account.lamports
        tab["opener"] = opener
        return row

    def act(self, name, holder, accounts, label, case, kind, what_changed, refuse, **args):
        """The settling key sends its own transactions; a signer's, objector's or decider's is paid by the submitting key."""
        payer, signers = (holder, ()) if holder is self.settler else (self.submitter, [holder])
        with self.fee_payer(payer):
            self.send(label, self.ix("delivery_rules", name, accounts, **args), signers, reject=refuse is not None)
        row = self.mark(case, kind, what_changed, refuse=refuse)
        if refuse is None:
            self.h.check(label + ": signed by the fee payer and the acting key only", self.signers_of(row),
                list(dict.fromkeys([str(payer.pubkey()), str(holder.pubkey())])))
        return row

    def attest(self, tab, signer, units, label, *, case, what_changed="nothing", refuse=None, rule=None, case_of=None):
        target = (case_of or tab)["case"]
        before = self.read_rules("Case", target)
        self.act("attest", signer, dict(signer=signer.pubkey(), case=target, rule=rule or tab["rule"], bill=tab["bill"]),
            label, case, "attest", what_changed, refuse, units=units8(units))
        if refuse is None:
            state = self.read_rules("Case", tab["case"])
            index = [str(k) for k in self.read_rules("Rule", tab["rule"])["terms"]["signers"]].index(str(signer.pubkey()))
            self.h.check(label + ": the case keeps the signer's units", state["attestations"][index], units8(units))
            self.h.check(label + ": the signer is marked as attested", state["attested"], before["attested"] | 1 << index)

    def object_(self, tab, holder, label, *, case, what_changed="nothing", refuse=None, rule=None):
        self.act("object", holder, dict(objector=holder.pubkey(), case=tab["case"], rule=rule or tab["rule"]),
            label, case, "object", what_changed, refuse)
        if refuse is None:
            self.h.check(label + ": the case records the objector", self.read_rules("Case", tab["case"])["objector"], holder.pubkey())

    def decide(self, tab, holder, units, label, *, case, what_changed="nothing", refuse=None, rule=None, case_of=None):
        target = (case_of or tab)["case"]
        self.act("decide", holder, dict(decider=holder.pubkey(), case=target, rule=rule or tab["rule"], bill=tab["bill"]),
            label, case, "decide", what_changed, refuse, units=units8(units))
        if refuse is None:
            self.h.check(label + ": the case records the decision", self.read_rules("Case", tab["case"])["decision"], units8(units))

    def settle_rule(self, tab, label, *, case, expect=None, what_changed="nothing", rule=None, bill=None, refuse=None):
        """Anyone settles: the rules program logs the case record, hashes it into the report and reports into ClaimsCore.

        `expect` is (outcome, group mask, units) written in the caller from the signed inputs.
        """
        rule = rule or tab["rule"]
        accounts = dict(case=tab["case"], rule=rule, bill=bill or tab["bill"], delivery_source=delivery_source(RULES, bytes(rule)),
            delivery_rules=RULES, claims_core=CORE)
        row = self.act("settle", self.settler, accounts, label, case, "settle", what_changed, refuse)
        if refuse is not None:
            return row
        outcome, group, units = expect
        units = units8(units)
        state = self.read_rules("Case", tab["case"])
        self.h.check(label + ": the case records the outcome, the group and the units",
            (state["outcome"], state["group"], state["units"]), (outcome, group, units))
        record = case_bytes(state)
        self.h.check(label + ": the case record's Borsh bytes match the generated interface",
            self.rules.encode({"defined": {"name": "Case"}}, state), record)
        logged = "Program data: " + " ".join(base64.b64encode(part).decode() for part in (OUTCOME_DOMAIN, bytes(tab["case"]), record))
        logs = row["result"]["logs"]
        self.h.check(label + ": the settle log carries the domain, the case address and the case record", logged in logs, True)
        self.settle_logs[str(tab["case"])] = logged
        report = dict(record_id=bytes(tab["case"]), request_hash=tab["billing_policy"]["request_hash"], started_at=NOW,
            completed_at=min(self.now, NOW + WORK), units=units,
            result_hash=hashlib.sha256(OUTCOME_DOMAIN + bytes(tab["case"]) + record).digest(), finality=1, outcome=1)
        bill = self.bill_state(tab)
        self.h.check(label + ": the bill holds the rule's report; its result_hash is the hash of the logged record",
            bill["source_report"], report)
        self.h.check(label + ": the report digest is the one ClaimsCore computes for any delivery signer", bill["source_digest"],
            phase_digest(REPORT_DOMAIN, self.domain, tab["bill"], tab["claim"], report_bytes(report), tab["policy_hash"]))
        self.h.check(label + ": admitted at the clock of the settlement", bill["evidence_admitted_at"], self.now)
        self.h.check(label + ": supported amount", bill["supported_amount"], supported_amount(tab["billing_policy"], units))
        self.h.check(label + ": the rules program called ClaimsCore's submit_program_bill_evidence",
            (f"Program {CORE} invoke [2]" in logs, "Program log: Instruction: SubmitProgramBillEvidence" in logs), (True, True))
        return row

    def close_case(self, tab, label, *, case, what_changed="nothing", refuse=None, opener=None, bill=None):
        recipient = opener or tab["opener"].pubkey()
        before = self.h.svm.get_account(tab["opener"].pubkey()).lamports
        self.act("close_case", self.settler, dict(case=tab["case"], opener=recipient, bill=bill or tab["bill"]),
            label, case, "close case", what_changed, refuse)
        if refuse is None:
            self.h.check(label + ": the case is closed", absent(self.h.svm, tab["case"]), True)
            self.h.check(label + ": its rent returned to the opener",
                self.h.svm.get_account(tab["opener"].pubkey()).lamports - before, tab["case_rent"])

    def claim(self, tab, label):
        self.submit_claim(tab, self.monetary_claim(tab, CLAIMED), label=label)
        self.mark("orders", "claim", "nothing")


def interface(f):
    for program, written in (("delivery_rules", RULES_ERRORS), ("claims_core", CORE_ERRORS), ("principal_registry", REGISTRY_ERRORS)):
        names = {error["name"]: error["code"] for error in f.idls[program].value["errors"]}
        f.h.check(f"error numbers written in this module equal the built {program} interface",
            {name: names.get(name) for name in written}, written)
    names = [i["name"] for i in f.rules.value["instructions"]]
    f.h.check("the rules program's instructions",
        names, ["attest", "close_case", "create_rule", "decide", "object", "open_case", "settle"])
    writes_rule = [i["name"] for i in f.rules.value["instructions"]
        for a in i["accounts"] if a["name"] == "rule" and a.get("writable")]
    f.h.check("only create_rule writes a rule account; no instruction changes or closes a rule", writes_rule, ["create_rule"])
    settle = f.rules.instructions["settle"]
    f.h.check("settle: the case, the rule, the bill, the delivery-source address, this program and ClaimsCore; no signer",
        [(a["name"], a.get("writable", False), a.get("signer", False)) for a in settle["accounts"]],
        [("case", True, False), ("rule", False, False), ("bill", True, False), ("delivery_source", False, False),
            ("delivery_rules", False, False), ("claims_core", False, False)])
    submit = f.core.instructions["submit_program_bill_evidence"]
    f.h.check("ClaimsCore's submit_program_bill_evidence: the bill, the signing source and the rule program",
        [(a["name"], a.get("writable", False), a.get("signer", False)) for a in submit["accounts"]],
        [("bill", True, False), ("source", False, True), ("rule_program", False, False)])
    f.h.check("the retained rules binary is the one this commit builds", f.retained_equals_build, True)


def cast(f):
    f.roles = {"buyer's owner (objector)": f.buyer, "agent 1": f.agent, "agent 2": f.agent2,
        "seller (opens cases)": f.seller, "carrier": f.carrier, "second carrier": f.carrier_b, "third carrier": f.carrier_c,
        "inspector (decider)": f.inspector, "lender's owner": f.parties["financier"]["owner"],
        "credit issuer's owner": f.parties["issuer"]["owner"], "fee payer of setup and order transactions": f.h.payer,
        "key that pays for claims, attestations, objections and decisions": f.submitter, "key that settles and closes cases": f.settler,
        "key that finalizes": f.finalizer, "key that collects": f.collector}
    keys = [str(holder.pubkey()) for holder in f.roles.values()]
    f.h.check("all roles are held by different keys", len(set(keys)), len(keys))


def rules(f):
    """Section 1: rules are created at the address of their contents and never change."""
    carrier, b, c = f.carrier.pubkey(), f.carrier_b.pubkey(), f.carrier_c.pubkey()
    buyer, inspector = f.buyer.pubkey(), f.inspector.pubkey()
    shoe = rule_terms([carrier], 1, objectors=[buyer], decider=inspector)
    f.shoe = f.create_rule(shoe, "ACCEPTED: the shoe-store rule is created: the carrier signs for delivery; the buyer may object "
        "until three days after signing closes; the inspector decides within five more; a silent carrier supports nothing, "
        "and a silent inspector pays nothing", case="1")
    f.lapse = f.create_rule(dict(shoe, decider_silent=LAPSE), "ACCEPTED: the same rule, except that if the inspector stays silent "
        "the objection lapses", case="1")
    trio = rule_terms([carrier, b, c], 2, objectors=[buyer], decider=inspector)
    f.trio = f.create_rule(trio, "ACCEPTED: a rule with three carriers in groups of two; a silent carrier supports nothing; "
        "the buyer may object; the inspector decides", case="1")
    f.paid = f.create_rule(dict(trio, signers_silent=PAID), "ACCEPTED: the three-carrier rule, except that while fewer than two "
        "carriers have reported, a silent carrier supports the order's maximum: the seller is paid unless the buyer objects", case="1")
    duo = rule_terms([carrier, b], 2, signers_silent=PAID, objectors=[buyer], decider=inspector)
    duo_address = rule_address(duo)
    f.prefund(duo_address, 1, "refused: one lamport is sent to the address of the two-carrier rule before it exists", case="1",
        what_changed="lamports sent: 1 instead of the 890,880 a new account must hold", refuse=refusal("InsufficientFundsForRent"))
    empty_rent = f.h.svm.minimum_balance_for_rent_exemption(0)
    f.h.check("the least a new account can hold is the rent of an empty account", empty_rent, 890_880)
    f.prefund(duo_address, empty_rent, "ACCEPTED: 890,880 lamports, the least a new account can hold, are sent to the address "
        "of the two-carrier rule before it exists", case="1")
    f.duo = f.create_rule(duo, "ACCEPTED: the two-carrier rule is created at its pre-funded address: both carriers in one "
        "group; while fewer than two have reported, a silent carrier supports the order's maximum", case="1")
    f.create_rule(rule_terms([carrier], 1, objectors=[buyer], decider=inspector, decide=MIN_DECIDE - 1),
        "refused: a shoe-store rule whose inspector has 3,599 seconds to decide after objections close", case="1",
        what_changed="decision window: 3,599 seconds instead of 3,600", refuse=refusal("InvalidRule", "rule terms"))
    hour = rule_terms([carrier], 1, objectors=[buyer], decider=inspector, decide=MIN_DECIDE)
    f.hour = f.create_rule(hour, "ACCEPTED: a shoe-store rule whose inspector has 3,600 seconds to decide after objections "
        "close", case="1")
    f.rule_terms = {f.shoe: shoe, f.lapse: dict(shoe, decider_silent=LAPSE), f.trio: trio, f.paid: dict(trio, signers_silent=PAID),
        f.duo: duo, f.hour: hour}
    f.h.check("the six rules have six addresses", len(set(map(str, f.rule_terms))), 6)
    f.rule_snapshot = f.h.snapshot(list(f.rule_terms))
    f.create_rule(shoe, "refused: the shoe-store rule is created again", case="1",
        what_changed="the rule already exists at the address of these terms", refuse=refusal("AccountAlreadyInUse"))
    f.create_rule(dict(shoe, decider=f.seller.pubkey()), "refused: terms naming the seller as decider are written to the "
        "shoe-store rule's address", case="1", what_changed="decider in the terms: the seller instead of the inspector",
        address=f.shoe, refuse=refusal("ConstraintSeeds", "rule account"))
    f.create_rule(rule_terms([carrier, b, carrier], 2, objectors=[buyer], decider=inspector),
        "refused: a rule naming the carrier twice among its three signers", case="1",
        what_changed="third signer: the carrier again instead of the third carrier", refuse=refusal("InvalidRule", "rule terms"))
    f.create_rule(rule_terms([carrier], 1, objectors=[buyer]), "refused: a rule with the buyer as objector and no decider",
        case="1", what_changed="decider: none instead of the inspector", refuse=refusal("InvalidRule", "rule terms"))
    f.h.check("the shoe-store rule is unchanged after both attempts", f.h.snapshot([f.shoe]), {str(f.shoe): f.rule_snapshot[str(f.shoe)]})


def orders(f):
    """Section 2: orders that name a rule; ClaimsCore requires the program to have no upgrade authority."""
    f.mandates[0] = f.permit("agent-1", 0)
    f.place_program(f.rule_order("shoe-1", f.shoe, program=UPGRADEABLE_RULES), "refused: order shoe-1 names the shoe-store rule "
        "through the copy of the rules program that has an upgrade authority", case="2", rule=UPGRADEABLE_RULES,
        what_changed="rules program: the same code with an upgrade authority", refuse=refusal("BillingSourceProgram", "rule program"))
    o = {}
    for name, rule, what in (("shoe-1", f.shoe, "the shoe-store rule"), ("shoe-2", f.shoe, "the shoe-store rule"),
            ("shoe-3", f.shoe, "the shoe-store rule"), ("shoe-4", f.shoe, "the shoe-store rule"),
            ("lapse-1", f.lapse, "the rule whose objection lapses"), ("trio-1", f.trio, "the three-carrier rule"),
            ("paid-1", f.paid, "the paid-unless-objection rule"), ("paid-2", f.paid, "the paid-unless-objection rule"),
            ("collude", f.paid, "the paid-unless-objection rule"), ("three", f.paid, "the paid-unless-objection rule"),
            ("duo-1", f.duo, "the two-carrier rule"), ("hour-1", f.hour, "the rule with an hour to decide")):
        o[name] = f.rule_order(name, rule)
        f.place_rule(o[name], f"ACCEPTED: order {name}, maximum 2,000, names {what}", case="2")
    for name in ("trio-2", "mix"):
        o[name] = f.rule_order(name, f.trio, rates=TWO_METRICS)
        f.place_rule(o[name], f"ACCEPTED: order {name}, maximum 2,000 over two metrics (up to 1,500 pairs at 1 and up to 250 "
            "express deliveries at 2), names the three-carrier rule", case="2")
    o["short"] = f.rule_order("short", f.shoe, evidence=DECISION_CLOSES + MIN_SETTLE - 1)
    f.place_rule(o["short"], "ACCEPTED: order short names the shoe-store rule; it admits reports until 3,599 seconds after the "
        "rule's decision deadline", case="2")
    for name in ("window", "missed"):
        o[name] = f.rule_order(name, f.shoe, evidence=DECISION_CLOSES + MIN_SETTLE)
        f.place_rule(o[name], f"ACCEPTED: order {name} names the shoe-store rule; it admits reports until 3,600 seconds after "
            "the rule's decision deadline", case="2")
    o["keyed"] = f.rule_order("keyed", f.shoe, source=f.carrier.pubkey())
    f.place_plain(o["keyed"], "ACCEPTED: order keyed carries the shoe-store rule's address as source_identity but names "
        "the carrier's key as delivery signer, through the ed25519 evidence instruction", case="2")
    # An order naming a rule nobody created: the address of the shoe-store terms with one more second to sign.
    ghost = rule_address(dict(f.rule_terms[f.shoe], sign_seconds=DAY + 1))
    o["ghost"] = f.rule_order("ghost", ghost)
    f.sdk_result = dict(ghost=f.sdk_check(o["ghost"]), short=f.sdk_check(o["short"]), shoe=f.sdk_check(o["shoe-1"]))
    f.h.check("the SDK check refuses order ghost before signing: no rule account exists at the address it names",
        f.sdk_result["ghost"], dict(error="DELIVERY_RULE_NOT_FOUND"))
    f.h.check("the SDK check refuses order short before signing: it leaves less than an hour to settle",
        f.sdk_result["short"], dict(error="DELIVERY_RULE_WINDOW"))
    shoe = f.sdk_result["shoe"]
    f.h.check("the SDK check accepts order shoe-1 and returns the shoe-store rule's roles",
        {k: shoe[k] for k in ("rule", "signers", "threshold", "signersSilent", "objectors", "decider", "deciderSilent", "overlaps")},
        dict(rule=str(f.shoe), signers=[str(f.carrier.pubkey())], threshold=1, signersSilent="nothing",
            objectors=[str(f.buyer.pubkey())], decider=str(f.inspector.pubkey()), deciderSilent="nothing", overlaps=[]))
    f.h.check("the SDK check for order shoe-1: the decision deadline, the guaranteed minimum to settle by (an hour later) "
        "and the last moment to settle (the order's evidence deadline)",
        (int(shoe["decisionCloses"]), int(shoe["minimumSettleBy"]), int(shoe["settleBy"])),
        (NOW + DECISION_CLOSES, NOW + DECISION_CLOSES + MIN_SETTLE, NOW + EVIDENCE))
    f.h.check("the rule order ghost names has no account", f.h.svm.get_account(ghost) is None, True)
    f.place_rule(o["ghost"], "ACCEPTED: order ghost names a rule address where no rule exists; ClaimsCore does not read the "
        "rule, and the SDK check refused this order before signing", case="2")
    # The owner pins delivery_signer to the shoe-store rule's delivery-source address.
    pin = [signer_term(delivery_source(RULES, bytes(f.shoe)))]
    f.mandates[1] = f.create_pinned("agent-2", 1, pin, "ACCEPTED: the buyer's owner creates agent 2's permission pinned to "
        "delivery_signer = the shoe-store rule's delivery-source address", case="2")

    def pinned(rule):
        source = delivery_source(RULES, bytes(rule))
        return f.evidence_order(lambda: f.rule_order("pinned", rule, index=1), delivery_list(SELLER_TERMS, source), None)
    tab = pinned(f.lapse)
    f.place_program(tab, "refused: agent 2's order names the rule whose objection lapses; the owner pinned the shoe-store rule",
        case="2", rule=RULES, shown=tab["order_terms"], what_changed="rule named: the lapsing rule's delivery-source address",
        refuse=refusal("TermsNotPermitted", "pinned term"))
    tab = pinned(f.shoe)
    row = f.place_program(tab, "ACCEPTED: agent 2's order, maximum 2,000, names the pinned shoe-store rule and shows its list",
        case="2", rule=RULES, shown=tab["order_terms"])
    f.ran(row, "pinned", "AcceptProgramSourcedFinancedTab", "ReservePinnedBilling")
    f.check_accepted_bill(tab, "pinned", delivery_source(RULES, bytes(f.shoe)))
    return o


def cases(f, o):
    """Section 3: anyone opens a case; its rule must be the bill's and must leave an hour to settle."""
    f.open_case(o["short"], "refused: the seller opens the case of order short, which admits reports until 3,599 seconds "
        "after the rule's decision deadline", case="3", what_changed="the order's evidence deadline: 3,599 seconds after the "
        "decision deadline instead of 3,600", refuse=refusal("RuleOutlastsOrder", "timeline"))
    f.open_case(o["window"], "ACCEPTED: the seller opens the case of order window, which admits reports until 3,600 seconds "
        "after the rule's decision deadline", case="3")
    f.open_case(o["ghost"], "refused: the seller opens the case of order ghost; no rule exists at the address the order names",
        case="3", what_changed="rule account: an address where no rule exists", refuse=refusal("AccountNotInitialized", "rule account"))
    f.open_case(o["lapse-1"], "refused: the seller opens the case of order lapse-1 with the shoe-store rule", case="3",
        rule=f.shoe, what_changed="rule shown: the shoe-store rule, not the one the order names", refuse=refusal("RuleNotNamed", "rule named"))
    f.open_case(o["keyed"], "refused: the seller opens the case of order keyed with the shoe-store rule; the order names the "
        "carrier's key, not the rule's delivery-source address", case="3",
        what_changed="the order's delivery signer: the carrier's key", refuse=refusal("RuleNotNamed", "rule named"))
    f.open_case(o["shoe-1"], "refused: the seller opens a case for order shoe-1's order record instead of its bill", case="3",
        bill=o["shoe-1"]["claim"], what_changed="bill account: the order record, also owned by ClaimsCore",
        refuse=refusal("NotABill", "bill account"))
    f.open_case(o["shoe-1"], "refused: the seller opens a case for the shoe-store rule's own account in place of a bill", case="3",
        bill=f.shoe, what_changed="bill account: an account the rules program owns", refuse=refusal("NotABill", "bill owner"))
    for name in ("shoe-1", "shoe-3", "shoe-4", "lapse-1", "trio-1", "trio-2", "mix", "paid-1", "paid-2", "collude", "three",
            "hour-1", "missed"):
        f.open_case(o[name], f"ACCEPTED: the seller opens the case of order {name}", case="3")
    f.open_case(o["shoe-2"], "ACCEPTED: the buyer's owner opens the case of order shoe-2", case="3", opener=f.buyer)
    f.prefund(o["duo-1"]["case"], f.h.svm.minimum_balance_for_rent_exemption(0), "ACCEPTED: 890,880 lamports are sent to the "
        "case address of order duo-1 before the case exists", case="3")
    f.open_case(o["duo-1"], "ACCEPTED: the seller opens the case of order duo-1 at its pre-funded address", case="3")
    for name in ("shoe-1", "shoe-2", "shoe-3", "shoe-4", "lapse-1", "trio-1", "trio-2", "mix", "paid-1", "paid-2", "collude",
            "three", "duo-1", "hour-1", "window", "missed", "ghost"):
        f.claim(o[name], f"ACCEPTED: the seller signs a claim of 1,400 on order {name}")


def attestations(f, o):
    """At the start: signers attest once, with units the order's policy admits; the buyer objects on shoe-2."""
    s1, s2, s3 = o["shoe-1"], o["shoe-2"], o["shoe-3"]
    f.attest(s1, f.carrier, (CAP + 1,), "refused: the carrier signs 2,001 units on order shoe-1, whose policy admits "
        "up to 2,000", case="4", what_changed="units: 2,001 instead of 900", refuse=refusal("UnitsOutsidePolicy", "units"))
    f.attest(s1, f.carrier, (SIGNED,), "refused: the carrier signs 900 units on order shoe-1 naming the rule whose objection "
        "lapses", case="4", rule=f.lapse, what_changed="rule shown: another rule than the case's",
        refuse=refusal("ConstraintHasOne", "case account"))
    f.attest(s1, f.carrier, (SIGNED,), "refused: the carrier signs 900 units for order shoe-1's bill on order shoe-3's case",
        case="4", case_of=s3, what_changed="case shown: another order's case under the same rule",
        refuse=refusal("ConstraintHasOne", "case account"))
    for name, section in (("shoe-1", "4"), ("shoe-2", "5"), ("shoe-3", "6"), ("lapse-1", "6"), ("hour-1", "5"), ("window", "9"),
            ("missed", "9")):
        f.attest(o[name], f.carrier, (SIGNED,), f"ACCEPTED: the carrier signs 900 units delivered on order {name}", case=section)
    trio = o["trio-1"]
    f.attest(trio, f.inspector, (800,), "refused: the inspector signs 800 units on order trio-1; it is not one of the "
        "rule's carriers", case="7", what_changed="signer: the inspector instead of the third carrier", refuse=refusal("NotASigner", "signer"))
    for holder, units, who in ((f.carrier, 900, "carrier"), (f.carrier_b, 700, "second carrier"), (f.carrier_c, 800, "third carrier")):
        f.attest(trio, holder, (units,), f"ACCEPTED: the {who} signs {units:,} units on order trio-1", case="7")
    f.attest(trio, f.carrier, (950,), "refused: the carrier signs 950 units on order trio-1, where it already signed 900",
        case="7", what_changed="the carrier has already attested on this case", refuse=refusal("AlreadyAttested", "attested"))
    for name, reports in (("trio-2", ((900, 100), (700, 200), (800, 50))), ("mix", ((900, 50), (1_500, 250), (800, 100)))):
        for holder, units, who in zip((f.carrier, f.carrier_b, f.carrier_c), reports, ("carrier", "second carrier", "third carrier")):
            f.attest(o[name], holder, units, f"ACCEPTED: the {who} signs {units[0]:,} pairs and {units[1]:,} express deliveries "
                f"on order {name}", case="7")
    f.attest(o["paid-2"], f.carrier, (500,), "ACCEPTED: the carrier signs 500 units on order paid-2; the other two carriers "
        "stay silent", case="8")
    f.attest(o["collude"], f.carrier, (CAP,), "ACCEPTED: the carrier signs 2,000 units, the order's maximum, on order collude",
        case="8")
    f.attest(o["collude"], f.carrier_b, (500,), "ACCEPTED: the second carrier signs 500 units on order collude; the third "
        "carrier stays silent", case="8")
    for holder, units, who in ((f.carrier, CAP, "carrier"), (f.carrier_b, 500, "second carrier"), (f.carrier_c, CLAIMED, "third carrier")):
        f.attest(o["three"], holder, (units,), f"ACCEPTED: the {who} signs {units:,} units on order three", case="8")
    f.attest(o["duo-1"], f.carrier, (500,), "ACCEPTED: the carrier signs 500 units on order duo-1; the second carrier stays "
        "silent", case="8")
    # Objections are taken any time before objections close; the decider waits for signing to close.
    f.object_(s2, f.buyer, "refused: the buyer's owner objects on order shoe-2 naming the rule whose objection lapses", case="5",
        rule=f.lapse, what_changed="rule shown: another rule than the case's", refuse=refusal("ConstraintHasOne", "case account"))
    f.object_(s2, f.buyer, "ACCEPTED: the buyer's owner objects on order shoe-2 at the start", case="5")
    f.decide(s2, f.inspector, (600,), "refused: the inspector decides 600 units on order shoe-2 while signing is open",
        case="5", what_changed="clock: signing has not closed", refuse=refusal("SigningOpen", "signing open"))
    f.decide(s2, f.inspector, (600,), "refused: the inspector decides 600 units on order shoe-2 naming the rule whose objection "
        "lapses", case="5", rule=f.lapse, what_changed="rule shown: another rule than the case's",
        refuse=refusal("ConstraintHasOne", "case account"))
    f.decide(s2, f.inspector, (600,), "refused: the inspector decides 600 units for order shoe-2's bill on order shoe-3's case",
        case="5", case_of=s3, what_changed="case shown: another order's case under the same rule",
        refuse=refusal("ConstraintHasOne", "case account"))


def signing_closed(f, o):
    """One second after signing closes: no more attestations; the inspector decides shoe-2."""
    f.at(SIGNING_CLOSES + 1)
    s2 = o["shoe-2"]
    f.attest(o["paid-2"], f.carrier_b, (500,), "refused: the second carrier signs 500 units on order paid-2 one second after "
        "signing closed", case="8", what_changed="clock: one second after signing closes", refuse=refusal("SigningClosed", "signing closed"))
    f.decide(o["shoe-1"], f.inspector, (600,), "refused: the inspector decides 600 units on order shoe-1, where nobody objected",
        case="5", what_changed="nobody has objected on this case", refuse=refusal("NoObjection", "no objection"))
    f.decide(s2, f.seller, (1_400,), "refused: the seller decides 1,400 units on order shoe-2", case="5",
        what_changed="decider: the seller instead of the inspector", refuse=refusal("NotTheDecider", "decider"))
    f.decide(s2, f.inspector, (600,), "ACCEPTED: the inspector decides 600 units on order shoe-2 one second after signing closed",
        case="5")
    f.decide(s2, f.inspector, (700,), "refused: the inspector decides 700 units on order shoe-2, which it already decided",
        case="5", what_changed="the inspector has already decided this case", refuse=refusal("AlreadyDecided", "decided"))
    f.settle_rule(s2, "ACCEPTED: an outside key settles order shoe-2 at once: the inspector decided 600", case="5",
        expect=(DECIDED, 0, (600,)))
    f.decide(s2, f.inspector, (700,), "refused: the inspector decides 700 units on order shoe-2 after it settled", case="5",
        what_changed="the case is settled", refuse=refusal("AlreadySettled", "settled decide"))
    f.close_case(s2, "refused: an outside key closes the case of order shoe-2 before its bill is finalized", case="5",
        what_changed="the bill is not finalized", refuse=refusal("BillNotFinal", "bill final"))
    f.settle(s2, 600, "ACCEPTED: an outside key finalizes order shoe-2: the rule reported 600, the seller claimed 1,400; bill 600",
        case="5", early=True)
    f.close_case(s2, "ACCEPTED: an outside key closes the case of order shoe-2; its rent returns to the buyer's owner, who opened it",
        case="5")


def objections_close(f, o):
    """At the last second of the objection window."""
    f.at(OBJECTIONS_CLOSE)
    s3 = o["shoe-3"]
    f.object_(s3, f.seller, "refused: the seller objects on order shoe-3", case="6",
        what_changed="objector: the seller instead of the buyer's owner", refuse=refusal("NotAnObjector", "objector"))
    for name, section in (("shoe-3", "6"), ("lapse-1", "6"), ("hour-1", "5")):
        f.object_(o[name], f.buyer, f"ACCEPTED: the buyer's owner objects on order {name} in the last second of the objection window",
            case=section)
    f.object_(s3, f.buyer, "refused: the buyer's owner objects on order shoe-3 a second time", case="6",
        what_changed="the case already has the buyer's objection", refuse=refusal("AlreadyObjected", "objected"))
    f.settle_rule(o["shoe-1"], "refused: an outside key settles order shoe-1 in the last second of the objection window",
        case="4", what_changed="clock: the objection window is still open", refuse=refusal("ObjectionsOpen", "objections open"))


def no_objection(f, o):
    """One second after the objection window closes."""
    f.at(OBJECTIONS_CLOSE + 1)
    s1 = o["shoe-1"]
    f.object_(s1, f.buyer, "refused: the buyer's owner objects on order shoe-1 one second after the objection window closed",
        case="4", what_changed="clock: one second after objections close", refuse=refusal("ObjectionsClosed", "objections closed"))
    f.settle_rule(s1, "refused: an outside key settles order shoe-1's case naming the rule whose objection lapses", case="4",
        rule=f.lapse, what_changed="rule shown: another rule than the case's", refuse=refusal("ConstraintHasOne", "case account"))
    f.settle_rule(s1, "refused: an outside key settles order shoe-1's case with order trio-1's bill", case="4",
        bill=o["trio-1"]["bill"], what_changed="bill shown: another order's bill", refuse=refusal("ConstraintHasOne", "case account"))
    f.settle_rule(s1, "ACCEPTED: an outside key settles order shoe-1: the carrier signed 900, nobody objected", case="4",
        expect=(SIGNERS, 0b1, (SIGNED,)))
    f.settle_rule(s1, "refused: an outside key settles order shoe-1 again", case="4", what_changed="the case is settled",
        refuse=refusal("AlreadySettled", "settled"))
    f.h.check("bill expected from the smallest of maximum 2,000, claim 1,400 and the carrier's 900", min(CAP, CLAIMED, SIGNED), SIGNED)
    f.settle(s1, SIGNED, "ACCEPTED: an outside key finalizes order shoe-1: bill 900, 1,100 released", case="4", early=True)
    f.pay(s1, SIGNED, "ACCEPTED: an outside key collects order shoe-1's 900 from the lender wallet to the seller", case="4")
    f.close_case(s1, "refused: an outside key closes the case of order shoe-1 with order trio-1's bill", case="4",
        bill=o["trio-1"]["bill"], what_changed="bill shown: another order's bill", refuse=refusal("ConstraintHasOne", "case account"))
    f.close_case(s1, "refused: an outside key closes the case of order shoe-1 naming itself to receive the rent", case="4",
        opener=f.settler.pubkey(), what_changed="rent recipient: the closing key instead of the seller, who opened the case",
        refuse=refusal("ConstraintHasOne", "case account"))
    f.close_case(s1, "ACCEPTED: an outside key closes the case of order shoe-1; its rent returns to the seller, who opened it", case="4")
    f.h.check("order shoe-1's case record survives the closed case in the settle log: its hash is the bill's result_hash",
        hashlib.sha256(b"".join(base64.b64decode(part) for part in
            f.settle_logs[str(s1["case"])].removeprefix("Program data: ").split(" "))).digest(),
        f.bill_state(s1)["source_report"]["result_hash"])
    f.open_case(s1, "refused: an outside key opens the case of order shoe-1 again after it closed", case="4", opener=f.settler,
        what_changed="the bill holds its report and is finalized", refuse=refusal("BillClosed", "bill closed"))

    s4 = o["shoe-4"]
    f.settle_rule(s4, "ACCEPTED: an outside key settles order shoe-4: the carrier never signed and nobody objected; a silent "
        "carrier supports nothing, so the rule reports zero", case="8", expect=(SIGNERS, 0b1, (0,)))
    f.attest(s4, f.carrier, (SIGNED,), "refused: the carrier signs 900 units on order shoe-4 after it settled", case="8",
        what_changed="the case is settled", refuse=refusal("AlreadySettled", "settled attest"))
    f.object_(s4, f.buyer, "refused: the buyer's owner objects on order shoe-4 after it settled", case="8",
        what_changed="the case is settled", refuse=refusal("AlreadySettled", "settled object"))
    f.settle(s4, 0, "ACCEPTED: an outside key finalizes order shoe-4: bill 0, all 2,000 released", case="8", early=True)

    f.h.check("order trio-1's groups of two: carrier and second 700, carrier and third 800, second and third 700",
        [min(900, 700), min(900, 800), min(700, 800)], [700, 800, 700])
    f.settle_rule(o["trio-1"], "ACCEPTED: an outside key settles order trio-1: the carrier and the third carrier both support 800, "
        "the most any two support", case="7", expect=(SIGNERS, 0b101, (800,)))
    f.settle(o["trio-1"], 800, "ACCEPTED: an outside key finalizes order trio-1: bill 800", case="7", early=True)
    f.h.check("order trio-2's groups of two, priced at 1 a pair and 2 an express delivery",
        [(min(900, 700), min(100, 200), 700 + 200), (min(900, 800), min(100, 50), 800 + 100), (min(700, 800), min(200, 50), 700 + 100)],
        [(700, 100, 900), (800, 50, 900), (700, 50, 800)])
    f.settle_rule(o["trio-2"], "ACCEPTED: an outside key settles order trio-2: the carrier and the second carrier both support 700 "
        "pairs and 100 express deliveries (900), tied with the carrier and the third carrier; the lower mask wins",
        case="7", expect=(SIGNERS, 0b011, (700, 100)))
    f.settle(o["trio-2"], 900, "ACCEPTED: an outside key finalizes order trio-2: bill 900", case="7", early=True)
    f.h.check("order mix's groups of two: the second carrier reports the order's whole maximum",
        [(min(900, 1_500), min(50, 250), 900 + 100), (min(900, 800), min(50, 100), 800 + 100), (min(1_500, 800), min(250, 100), 800 + 200)],
        [(900, 50, 1_000), (800, 50, 900), (800, 100, 1_000)])
    f.settle_rule(o["mix"], "ACCEPTED: an outside key settles order mix: no two carriers support more than 1,000; the second "
        "carrier's report of the maximum moves nothing", case="7", expect=(SIGNERS, 0b011, (900, 50)))
    f.settle(o["mix"], 1_000, "ACCEPTED: an outside key finalizes order mix: bill 1,000", case="7", early=True)

    f.settle_rule(o["paid-1"], "ACCEPTED: an outside key settles order paid-1: no carrier signed and nobody objected; silent carriers "
        "support the maximum of 2,000", case="8", expect=(SIGNERS, 0b011, (CAP,)))
    f.settle(o["paid-1"], CLAIMED, "ACCEPTED: an outside key finalizes order paid-1: bill 1,400, the seller's claim", case="8", early=True)
    f.pay(o["paid-1"], CLAIMED, "ACCEPTED: an outside key collects order paid-1's 1,400 from the lender wallet to the seller", case="8")
    f.settle_rule(o["paid-2"], "ACCEPTED: an outside key settles order paid-2: only the carrier signed (500), fewer than two, so "
        "silence counts; the two silent carriers support the maximum of 2,000", case="8", expect=(SIGNERS, 0b110, (CAP,)))
    f.settle(o["paid-2"], CLAIMED, "ACCEPTED: an outside key finalizes order paid-2: bill 1,400", case="8", early=True)
    f.settle_rule(o["duo-1"], "ACCEPTED: an outside key settles order duo-1: only the carrier signed (500), so silence counts; "
        "the carrier and the silent second carrier together support 500", case="8", expect=(SIGNERS, 0b11, (500,)))
    f.settle(o["duo-1"], 500, "ACCEPTED: an outside key finalizes order duo-1: bill 500", case="8", early=True)
    f.h.check("order collude: two carriers reported, so only they form a group; together they support 500",
        min(CAP, 500), 500)
    f.settle_rule(o["collude"], "ACCEPTED: an outside key settles order collude: the carrier signed 2,000 and the second carrier "
        "500; two carriers reported, so the silent third carrier does not count, and the two support 500", case="8",
        expect=(SIGNERS, 0b011, (500,)))
    f.settle(o["collude"], 500, "ACCEPTED: an outside key finalizes order collude: bill 500", case="8", early=True)
    f.h.check("order three's groups of two: carrier and second 500, carrier and third 1,400, second and third 500",
        [min(CAP, 500), min(CAP, CLAIMED), min(500, CLAIMED)], [500, 1_400, 500])
    f.settle_rule(o["three"], "ACCEPTED: an outside key settles order three: the carrier signed 2,000, the second carrier 500 and "
        "the third carrier 1,400; the carrier and the third carrier both support 1,400", case="8", expect=(SIGNERS, 0b101, (CLAIMED,)))
    f.settle(o["three"], CLAIMED, "ACCEPTED: an outside key finalizes order three: bill 1,400", case="8", early=True)


def decided_within_the_hour(f, o):
    """Order hour-1: the buyer objected in the last second of the objection window; the inspector has one hour."""
    f.at(OBJECTIONS_CLOSE + MIN_DECIDE)
    h1 = o["hour-1"]
    f.decide(h1, f.inspector, (700,), "ACCEPTED: the inspector decides 700 units on order hour-1 in the last second of its hour, "
        "3,600 seconds after objections closed", case="5")
    f.settle_rule(h1, "ACCEPTED: an outside key settles order hour-1 at once: the inspector decided 700", case="5",
        expect=(DECIDED, 0, (700,)))
    f.settle(h1, 700, "ACCEPTED: an outside key finalizes order hour-1: bill 700", case="5", early=True)


def decider_silent(f, o):
    f.at(DECISION_CLOSES)
    f.settle_rule(o["shoe-3"], "refused: an outside key settles order shoe-3 in the last second of the decision window; the "
        "buyer objected and the inspector has not decided", case="6", what_changed="clock: the inspector can still decide",
        refuse=refusal("DecisionOpen", "decision open"))
    f.at(DECISION_CLOSES + 1)
    f.decide(o["shoe-3"], f.inspector, (600,), "refused: the inspector decides 600 units on order shoe-3 one second after the "
        "decision window closed", case="6", what_changed="clock: one second after the decision closes",
        refuse=refusal("DecisionClosed", "decision closed"))
    f.settle_rule(o["shoe-3"], "ACCEPTED: an outside key settles order shoe-3: the buyer objected and the inspector stayed "
        "silent; the rule reports zero", case="6", expect=(DECIDER_SILENT, 0, (0,)))
    f.settle(o["shoe-3"], 0, "ACCEPTED: an outside key finalizes order shoe-3: bill 0, all 2,000 released", case="6", early=True)
    f.settle_rule(o["lapse-1"], "ACCEPTED: an outside key settles order lapse-1: the buyer objected, the inspector stayed silent, "
        "the objection lapsed; the rule reports the carrier's 900", case="6", expect=(DECIDER_SILENT, 0b1, (SIGNED,)))
    f.settle(o["lapse-1"], SIGNED, "ACCEPTED: an outside key finalizes order lapse-1: bill 900", case="6", early=True)


def settle_window(f, o):
    """The hour between the decision deadline and the evidence deadline of orders window and missed."""
    f.at(DECISION_CLOSES + MIN_SETTLE)
    f.settle_rule(o["window"], "ACCEPTED: an outside key settles order window in the last second its evidence deadline admits, "
        "3,600 seconds after the rule's decision deadline", case="9", expect=(SIGNERS, 0b1, (SIGNED,)))
    f.settle(o["window"], SIGNED, "ACCEPTED: an outside key finalizes order window: bill 900", case="9", early=True)
    f.at(DECISION_CLOSES + MIN_SETTLE + 1)
    f.settle_rule(o["missed"], "refused: an outside key settles order missed one second after its evidence deadline", case="9",
        what_changed="clock: one second after the order's evidence deadline", refuse=refusal("BillingDeadline", "core deadline"))
    f.at(EVIDENCE + 1)
    for name, why in (("missed", "nobody settled in time"), ("ghost", "its rule does not exist, so no case could open")):
        f.settle(o[name], 0, f"ACCEPTED: an outside key finalizes order {name} after both deadlines: {why}; with no report the "
            "bill is 0", case="9", early=False)


def end_state(f):
    f.h.check("every rule account is byte-identical to its state at creation", f.h.snapshot(list(f.rule_terms)), f.rule_snapshot)
    refused = [row for row in f.h.cases if row["expected"] == "reject"]
    f.h.check("every refusal in this run was annotated and checked",
        all(row.get("commit_legs", {}).get("outcome") == "refused" for row in refused), True)
    size = RETAINED.stat().st_size
    f.cost = dict(program_bytes=size, program_data_bytes=45 + size,
        program_data_rent=f.h.svm.minimum_balance_for_rent_exemption(45 + size),
        program_account_rent=f.h.svm.minimum_balance_for_rent_exemption(36),
        rule_bytes=8 + 369, rule_rent=f.h.svm.minimum_balance_for_rent_exemption(8 + 369),
        case_bytes=len(f.h.svm.get_account(case_address(f.cost_bill)).data),
        case_rent=f.h.svm.minimum_balance_for_rent_exemption(len(f.h.svm.get_account(case_address(f.cost_bill)).data)))


def exercise(f):
    setup(f)
    interface(f)
    cast(f)
    rules(f)
    o = orders(f)
    cases(f, o)
    f.cost_bill = o["trio-1"]["bill"]
    attestations(f, o)
    signing_closed(f, o)
    objections_close(f, o)
    no_objection(f, o)
    decided_within_the_hour(f, o)
    decider_silent(f, o)
    settle_window(f, o)
    end_state(f)


def record(f, summary, index, directory, seconds):
    """Copy the finished run into a publication directory; no second execution."""
    directory = Path(directory).resolve()
    directory.mkdir(parents=True, exist_ok=True)
    raw = Path(summary["trace"]).read_bytes()
    packed = directory / "delivery-rules-trace.json.xz"
    packed.write_bytes(lzma.compress(raw))
    assert lzma.decompress(packed.read_bytes()) == raw
    (directory / "case-index.json").write_text(json.dumps(index, indent=1, default=json_value) + "\n")
    annotated = [row for row in index if row["section"] != "setup"]
    for name in ("build-receipt.json", "idl-receipt.json"):
        (directory / name).write_bytes((OUT / name).read_bytes())
    receipt = dict(summary, trace=packed.name, trace_xz_sha256=digest(packed), annotated_cases=len(annotated),
        annotated_refusals=sum(row["outcome"] == "refused" for row in annotated),
        annotated_acceptances=sum(row["outcome"] == "accepted" for row in annotated),
        roles={role: str(holder.pubkey()) for role, holder in f.roles.items()},
        run_seconds=round(seconds, 2), engine="solders 0.29.0 LiteSVM",
        program_sha256={name: value["sha256"] for name, value in f.build["artifacts"].items()},
        rules_program=dict(address=str(RULES), retained=str(RETAINED.relative_to(ROOT)), retained_sha256=RETAINED_SHA256,
            retained_equals_build=f.retained_equals_build, upgradeable_copy=str(UPGRADEABLE_RULES),
            upgrade_authority=str(UPGRADE_AUTHORITY),
            rules={name: str(address) for name, address in (("shoe-store", f.shoe), ("lapse", f.lapse), ("trio", f.trio),
                ("paid", f.paid), ("duo", f.duo), ("hour", f.hour))},
            delivery_sources={name: str(delivery_source(RULES, bytes(address))) for name, address in
                (("shoe-store", f.shoe), ("lapse", f.lapse), ("trio", f.trio), ("paid", f.paid), ("duo", f.duo), ("hour", f.hour))},
            sdk_check=f.sdk_result, cost=f.cost),
        swig_sha256=SWIG_SHA256, build_receipt_sha256=digest(OUT / "build-receipt.json"),
        idl_receipt_sha256=digest(OUT / "idl-receipt.json"))
    (directory / "delivery-rules-receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    return receipt


def main():
    started = time.monotonic()
    f = DeliveryRules()
    try:
        exercise(f)
    except Exception as error:
        f.finish("delivery-rules", error=error)
        raise
    index = case_index(f, "delivery-rules")
    for row in f.h.cases:
        if "commit_legs" in row:
            row["delivery_rules"] = row.pop("commit_legs")
    summary = f.finish("delivery-rules")
    if "--record" in sys.argv:
        print(json.dumps(record(f, summary, index, sys.argv[sys.argv.index("--record") + 1], time.monotonic() - started)))


if __name__ == "__main__":
    main()
