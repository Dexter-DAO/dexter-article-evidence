#!/usr/bin/env python3
"""Settle when both records are in; a program can be the delivery signer.

An evidence order's bill is min(maximum, seller's claim, what the delivery
signer's report supports). `finalize_evidence_bill` runs as soon as the report
and the seller's claim are both admitted, and otherwise after both admission
windows close, when a missing record counts as zero. Both records are
immutable once admitted, so the amount is the same whenever it is finalized.

`accept_program_sourced_financed_tab` accepts an evidence order whose delivery
signer (`policy.source`) is a rule program's address
["delivery-source", policy.source_identity]. ClaimsCore derives that address
from the rule program shown and reads the program's loader records: the program
is executable, owned by the upgradeable loader, and its ProgramData has no
upgrade authority. `order_terms`, when shown, is checked by the Registry as on
the other pinned evidence instructions, so an owner can pin delivery_signer to
the rule program's address. `submit_program_bill_evidence` admits the report
when that address signs, which only the rule program can do (by CPI). The
report digest, deadline and replay rules are the ones `submit_bill_evidence`
uses.

The rule program here is foundation/test-programs/delivery-rule: it signs any report its caller
passes, at its own delivery-source address. The same ELF is loaded twice in
LiteSVM: once with no upgrade authority, once with one.

Runs: the main run (pinned_delivery.py's fixture: one buyer, two agents, one
seller, the carrier, one credit issuer, a lender wallet), and a recoverable
order on recovery.py's fixture. Evidence orders have a maximum of 2,000 at a
price of 1 per unit; work ends at 20 seconds, the report window at 40 and the
claim window at 45.

Refusals are checked as in pinned_terms.py: transaction error, first failing
program frame, Anchor error name and number, the source location in the failing
frame's log line (table `RAISED`), and byte-identical state for every writable
account except the fee payer.

Build the rule program once:
  ./dev exec cargo-build-sbf --no-rustup-override --skip-tools-install --tools-version v1.57 \\
    --manifest-path foundation/test-programs/delivery-rule/Cargo.toml --arch v3 \\
    --sbf-out-dir .local/anchor/fixtures -- --locked
Run in a prepared checkout: ./dev anchor-smoke early-settlement
Record: <python> foundation/onchain/tests/early_settlement.py --record docs/evidence/<directory>
"""
from pinned_delivery import *
from recovery import RecoveryFixture

CAP = 2_000
SIGNED = 900
CLAIMED = 1_400
RULE_SO = OUT / "fixtures/delivery_rule.so"
RULE_SOURCE = ROOT / "foundation/test-programs/delivery-rule"
RULE = key(90).pubkey()                 # immutable copy
UPGRADEABLE_RULE = key(91).pubkey()     # same ELF, with an upgrade authority
UPGRADE_AUTHORITY = key(92).pubkey()
LOADER = Pubkey.from_string("BPFLoaderUpgradeab1e11111111111111111111111")
SOURCE_IDENTITY = ident("synthetic-source-control-disclosure")   # evidence_billing.py's policy default
OTHER_IDENTITY = ident("another-source-control-disclosure")
CORE_ERRORS = dict(BillingPolicy=6060, BillingSignature=6063, BillingReplay=6064, BillingFinality=6066,
    BillingSourceProgram=6078)
ANCHOR_ERRORS = dict(AccountNotSigner=3010, AccountNotInitialized=3012)
REGISTRY_ERRORS = dict(TermsRequired=6051, TermsNotPermitted=6053)
CORE_SOURCE = "thrown in programs/claims-core/src/"
# Written here from the source at this commit and asserted for every refusal.
RAISED = {
    "finalization": CORE_SOURCE + "evidence_billing.rs:366",
    "report key": CORE_SOURCE + "evidence_billing.rs:281",
    "program signer": CORE_SOURCE + "evidence_billing.rs:324",
    "report replay": CORE_SOURCE + "evidence_billing.rs:297",
    "rule address": CORE_SOURCE + "evidence_billing.rs:162",
    "rule program": CORE_SOURCE + "evidence_billing.rs:164",
    "rule loader records": CORE_SOURCE + "evidence_billing.rs:168",
    "key source": CORE_SOURCE + "evidence_billing.rs:223",
    "source account": "caused by account: source",
    "binding account": "caused by account: binding",
    "no list shown": "thrown in programs/principal-registry/src/lib.rs:357",
    "pinned term": "thrown in programs/principal-registry/src/lib.rs:363",
}


def refusal(name, where, *, index=0):
    if name in REGISTRY_ERRORS:
        return dict(program=REGISTRY, name=name, code=REGISTRY_ERRORS[name], index=index, source=RAISED[where])
    return dict(program=CORE, name=name, code=CORE_ERRORS.get(name) or ANCHOR_ERRORS[name], index=index, source=RAISED[where])


def delivery_source(program, identity=SOURCE_IDENTITY):
    return pda(program, b"delivery-source", identity)


def program_data(program):
    return pda(LOADER, bytes(program))


class RuleProgram:
    """The delivery-rule ELF at RULE (no upgrade authority) and UPGRADEABLE_RULE (with one)."""

    def load_rules(self):
        assert RULE_SO.is_file(), "Build foundation/test-programs/delivery-rule first; see this module's docstring"
        self.rule_sha256 = self.h.load(RULE, RULE_SO)
        assert self.h.load(UPGRADEABLE_RULE, RULE_SO, upgrade_authority=UPGRADE_AUTHORITY) == self.rule_sha256
        for program, authority in ((RULE, None), (UPGRADEABLE_RULE, UPGRADE_AUTHORITY)):
            account, data = self.h.svm.get_account(program), self.h.svm.get_account(program_data(program))
            self.h.check(f"rule program {program} is an executable upgradeable-loader program",
                (account.executable, account.owner, bytes(account.data)),
                (True, LOADER, (2).to_bytes(4, "little") + bytes(program_data(program))))
            self.h.check(f"rule program {program}: ProgramData upgrade authority",
                (bytes(data.data[:4]), data.data[12], bytes(data.data[13:45]) if data.data[12] else None),
                ((3).to_bytes(4, "little"), 0 if authority is None else 1, None if authority is None else bytes(authority)))

    def relay(self, tab, report, *, rule=RULE, rule_account=None):
        """The rule program at `rule` calls submit_program_bill_evidence, signing as its delivery-source address."""
        source = delivery_source(rule, tab["billing_policy"]["source_identity"])
        inner = self.ix("claims_core", "submit_program_bill_evidence",
            dict(bill=tab["bill"], source=source, rule_program=rule_account or rule), report=report)
        return Instruction(rule, tab["billing_policy"]["source_identity"] + bytes(inner.data),
            [M(CORE), M(tab["bill"], w=True), M(source), M(rule_account or rule)])

    def program_report(self, tab, report, label, *, case, what_changed="nothing", refuse=None, **options):
        before = self.bill_state(tab)
        with self.fee_payer(self.submitter):
            self.send(label, self.relay(tab, report, **options), reject=refuse is not None)
        row = self.mark(case, "program report", what_changed, refuse=refuse)
        if refuse is None:
            digest = phase_digest(REPORT_DOMAIN, self.domain, tab["bill"], tab["claim"], report_bytes(report), tab["policy_hash"])
            bill = self.bill_state(tab)
            self.h.check(label + ": the bill keeps the report", bill["source_report"], report)
            self.h.check(label + ": the report digest is the one an ed25519 signer signs", bill["source_digest"], digest)
            self.h.check(label + ": admitted at", bill["evidence_admitted_at"],
                self.now if before["source_report"] is None else before["evidence_admitted_at"])
            self.h.check(label + ": supported amount", bill["supported_amount"], supported_amount(tab["billing_policy"], report["units"]))
            self.h.check(label + ": the only transaction signer is the submitting key", self.signers_of(row), [str(self.submitter.pubkey())])
        return row


class EarlySettlement(RuleProgram, PinnedDelivery):
    def __init__(self):
        super().__init__()
        self.load_rules()

    def order(self, name, *, source=None, index=0, overrides=None):
        self.agent_index = index
        try:
            return self.evidence_tab(name, cap=CAP, rates=((1, 1, CAP),),
                policy_overrides=dict(source=self.meter.pubkey() if source is None else source, **(overrides or {})))
        finally:
            self.agent_index = 0

    def place_program(self, tab, label, *, case, rule=RULE, rule_data=None, what_changed="nothing", shown=None, refuse=None):
        """accept_program_sourced_financed_tab; `shown` is the list of terms, or None."""
        accounts = dict(tab["evidence_accounts"], rule_program=rule, rule_program_data=rule_data or program_data(rule))
        instruction = self.ix("claims_core", "accept_program_sourced_financed_tab", accounts,
            remaining=self.roles_of(tab), order_terms=shown, **tab["evidence_args"])
        before = self.money()
        self.send(label, instruction, tab["signers"], reject=refuse is not None, receipt=tab["receipt"])
        row = self.mark(case, "program-sourced order" + (", list shown" if shown else ""), what_changed, refuse=refuse)
        if refuse is not None:
            self.nothing_moved(tab, label, before)
            self.h.check("no bill exists: " + label, absent(self.h.svm, tab["bill"]), True)
        return row

    def loader_buffer(self):
        """A real upgradeable-loader buffer account (tag 1), created and initialized by the loader."""
        buffer, authority = key(93), key(94)
        space = 37
        create = create_account(CreateAccountParams(from_pubkey=self.h.payer.pubkey(), to_pubkey=buffer.pubkey(),
            lamports=self.h.svm.minimum_balance_for_rent_exemption(space), space=space, owner=LOADER))
        initialize = Instruction(LOADER, (0).to_bytes(4, "little"), [M(buffer.pubkey(), w=True), M(authority.pubkey())])
        self.send("ACCEPTED: a loader buffer account is created and initialized", [create, initialize], [buffer])
        self.mark("4", "loader buffer", "nothing")
        account = self.h.svm.get_account(buffer.pubkey())
        self.h.check("the buffer is a loader-owned account with tag 1 and its authority",
            (account.owner, account.executable, bytes(account.data)),
            (LOADER, False, (1).to_bytes(4, "little") + b"\x01" + bytes(authority.pubkey())))
        return buffer.pubkey()

    def place_plain(self, tab, label, *, case, what_changed="nothing", refuse=None):
        return self.place_delivery(tab, label, case=case, what_changed=what_changed, shown=False, refuse=refuse)

    def settle(self, tab, amount, label, *, case, early):
        self.finalize(tab, label=label)
        row = self.mark(case, "finalization", "nothing")
        self.assert_final_amount(tab, amount, early=early)
        return row

    def refuse_finalize(self, tab, label, *, case, what_changed):
        self.finalize(tab, error="BillingFinality", label=label)
        return self.mark(case, "finalization", what_changed, refuse=refusal("BillingFinality", "finalization"))

    def pay(self, tab, amount, label, *, case):
        seller = self.h.ata(self.parties["supplier"]["owner"].pubkey(), self.mint)
        wallet = self.parties["financier"]["custody_token"]
        before = self.h.amount(seller), self.h.amount(wallet)
        self.collect_from(tab, amount, payer=self.collector, label=label)
        row = self.mark(case, "collection", "nothing")
        self.h.check(label + ": the seller receives", self.h.amount(seller) - before[0], amount)
        self.h.check(label + ": the lender wallet pays", before[1] - self.h.amount(wallet), amount)
        return row


def interface(f):
    for program, written in (("claims_core", CORE_ERRORS), ("principal_registry", REGISTRY_ERRORS)):
        names = {error["name"]: error["code"] for error in f.idls[program].value["errors"]}
        f.h.check(f"error numbers written in this module equal the built {program} interface",
            {name: names.get(name) for name in written}, written)
    plain, program = f.core.instructions["accept_pinned_evidence_financed_tab"], f.core.instructions["accept_program_sourced_financed_tab"]
    f.h.check("accept_program_sourced_financed_tab takes the pinned evidence instruction's arguments, the list optional",
        [(a["name"], a["type"]) for a in program["args"]],
        [(a["name"], a["type"]) for a in plain["args"][:-1]] + [("order_terms", {"option": plain["args"][-1]["type"]})])
    f.h.check("accept_program_sourced_financed_tab takes the evidence order's accounts, then the rule program and its ProgramData",
        (program["accounts"][0]["accounts"], [a["name"] for a in program["accounts"][1:]]),
        (plain["accounts"], ["rule_program", "rule_program_data"]))
    submit = f.core.instructions["submit_program_bill_evidence"]
    f.h.check("submit_program_bill_evidence: the bill, the signing source and the rule program; one argument, the report",
        ([(a["name"], a.get("writable", False), a.get("signer", False)) for a in submit["accounts"]], [a["name"] for a in submit["args"]]),
        ([("bill", True, False), ("source", False, True), ("rule_program", False, False)], ["report"]))


def cast(f):
    parties = f.parties
    f.roles = {"buyer's owner": parties["company"]["owner"], "agent 1": f.agent, "agent 2": f.agent2,
        "seller": parties["supplier"]["owner"], "carrier (ed25519 delivery signer)": f.meter,
        "lender's owner": parties["financier"]["owner"], "credit issuer's owner": parties["issuer"]["owner"],
        "fee payer of setup and order transactions": f.h.payer, "key that submits signed records": f.submitter,
        "key that finalizes": f.finalizer, "key that collects": f.collector}
    keys = [str(holder.pubkey()) for holder in f.roles.values()]
    f.h.check("all roles are held by different keys", len(set(keys)), len(keys))
    f.h.check("the rule program's delivery-source address is not a signing key", delivery_source(RULE).is_on_curve(), False)


def both_in(f):
    """Section 1: report and claim in at 0 seconds; the bill settles and the seller is paid at once."""
    f.mandates[0] = f.permit("agent-1", 0)
    orders = {}
    for number in (1, 2, 3, 4):
        orders[number] = f.order(f"order-{number}")
        f.place_plain(orders[number], f"ACCEPTED: evidence order {number}, maximum 2,000, naming the carrier", case="1")
    o1 = orders[1]
    f.submit_report(o1, f.report(o1, units=(SIGNED,)), label="ACCEPTED: the carrier signs 900 units delivered on order 1 at 0 seconds")
    f.mark("1", "report", "nothing")
    f.submit_claim(o1, f.monetary_claim(o1, CLAIMED), label="ACCEPTED: the seller signs a claim of 1,400 on order 1 at 0 seconds")
    f.mark("1", "claim", "nothing")
    f.h.check("bill expected from the smallest of maximum 2,000, claim 1,400 and signed 900", min(CAP, CLAIMED, SIGNED), SIGNED)
    f.settle(o1, SIGNED, "ACCEPTED: an outside key finalizes order 1 at 0 seconds, with both windows open: bill 900, 1,100 released",
        case="1", early=True)
    f.pay(o1, SIGNED, "ACCEPTED: an outside key collects order 1's 900 from the lender wallet to the seller at 0 seconds", case="1")
    retained = f.h.snapshot([o1["bill"], o1["claim"]])
    f.finalize(o1, label="no effect: order 1 is finalized again; the program returns the original outcome")
    f.mark("1", "finalization", "an already finalized bill is finalized again")
    f.submit_report(o1, f.report(o1, units=(SIGNED,)), payer=f.finalizer,
        label="no effect: the carrier's exact report on order 1 is sent again after settlement")
    f.mark("1", "report", "an admitted report is sent again")
    f.h.check("order 1: finalizing and reporting again change nothing", f.h.snapshot([o1["bill"], o1["claim"]]), retained)
    f.submit_report(o1, f.report(o1, units=(SIGNED + 100,)), error="BillingReplay",
        label="refused: the carrier signs 1,000 units on settled order 1, which already holds its report of 900")
    f.mark("1", "report", "units in the report: 1,000 instead of the admitted 900",
        refuse=refusal("BillingReplay", "report replay", index=1))
    return orders


def one_missing(f, orders):
    """Section 2: before the windows close, a bill with only one record does not finalize."""
    o2, o3 = orders[2], orders[3]
    f.submit_report(o2, f.report(o2, units=(SIGNED,)), label="ACCEPTED: the carrier signs 900 units delivered on order 2")
    f.mark("2", "report", "nothing")
    f.refuse_finalize(o2, "refused: finalization of order 2 at 0 seconds: the carrier's report is in, the seller has not claimed",
        case="2", what_changed="the seller's claim is not in")
    f.submit_claim(o3, f.monetary_claim(o3, CLAIMED), label="ACCEPTED: the seller signs a claim of 1,400 on order 3")
    f.mark("2", "claim", "nothing")
    f.refuse_finalize(o3, "refused: finalization of order 3 at 0 seconds: the seller's claim is in, the carrier has not signed",
        case="2", what_changed="the carrier's report is not in")
    o4 = orders[4]
    f.submit_report(o4, f.report(o4, units=(SIGNED,)), label="ACCEPTED: the carrier signs 900 units delivered on order 4")
    f.mark("3", "report", "nothing")
    f.at(45)
    f.submit_claim(o4, f.monetary_claim(o4, CLAIMED), label="ACCEPTED: the seller signs a claim of 1,400 on order 4 at 45 seconds")
    f.mark("3", "claim", "nothing")
    f.refuse_finalize(o2, "refused: finalization of order 2 at 45 seconds, the last second of the seller's claim window",
        case="2", what_changed="clock, one second before finalization opens; the seller's claim is not in")


def after_deadlines(f, orders):
    """Section 3: after both windows close, finalization is as before; a missing record counts as zero."""
    f.at(46)
    for number, amount, why in ((2, 0, "the carrier signed 900 and the seller never claimed: bill 0, all 2,000 released"),
            (3, 0, "the seller claimed 1,400 and the carrier never signed: bill 0, all 2,000 released"),
            (4, SIGNED, "the carrier signed 900 and the seller claimed 1,400: bill 900, 1,100 released")):
        f.settle(orders[number], amount, f"ACCEPTED: an outside key finalizes order {number} at 46 seconds; {why}", case="3", early=False)
    f.pay(orders[4], SIGNED, "ACCEPTED: an outside key collects order 4's 900 from the lender wallet to the seller", case="3")


def program_order(f):
    """Section 4: an order whose delivery signer is the rule program's address."""
    rule_source = delivery_source(RULE)

    def p1(**options):
        return f.order("program-order-1", **options)
    address = refusal("BillingSourceProgram", "rule address")
    f.place_program(p1(source=delivery_source(UPGRADEABLE_RULE)), "refused: program order 1 names the delivery-source address "
        "of the rule program with an upgrade authority, and shows that program", case="4", rule=UPGRADEABLE_RULE,
        what_changed="rule program: the same code with an upgrade authority", refuse=refusal("BillingSourceProgram", "rule loader records"))
    f.place_program(p1(source=delivery_source(UPGRADEABLE_RULE)), "refused: program order 1 names the upgradeable rule program's "
        "address but shows the immutable rule program", case="4",
        what_changed="delivery signer: another program's delivery-source address", refuse=address)
    f.place_program(p1(source=delivery_source(RULE, OTHER_IDENTITY)), "refused: program order 1 names the rule program's address "
        "for another source identity than the policy's", case="4",
        what_changed="delivery signer: the rule program's address for another source identity", refuse=address)
    f.place_program(p1(source=f.meter.pubkey()), "refused: program order 1 names the carrier's key as delivery signer "
        "through the program instruction", case="4", what_changed="delivery signer: an ed25519 key", refuse=address)
    wallet = f.parties["supplier"]["owner"].pubkey()
    f.place_program(p1(source=delivery_source(wallet)), "refused: program order 1 shows the seller's wallet as the rule program, "
        "naming that wallet's delivery-source address", case="4", rule=wallet, rule_data=program_data(RULE),
        what_changed="rule program: a wallet, not a program", refuse=refusal("BillingSourceProgram", "rule program"))
    f.place_program(p1(source=rule_source), "refused: program order 1 shows the immutable rule program with the upgradeable "
        "program's ProgramData", case="4", rule_data=program_data(UPGRADEABLE_RULE),
        what_changed="ProgramData: another program's", refuse=refusal("BillingSourceProgram", "rule loader records"))
    buffer = f.loader_buffer()
    f.place_program(p1(source=rule_source), "refused: program order 1 shows the immutable rule program with a loader buffer "
        "account in place of its ProgramData", case="4", rule_data=buffer,
        what_changed="ProgramData: a loader buffer account", refuse=refusal("BillingSourceProgram", "rule loader records"))
    f.place_plain(p1(source=rule_source), "refused: program order 1 naming the rule program's address, sent through plain "
        "accept_evidence_financed_tab", case="4", what_changed="instruction: the ed25519 evidence instruction",
        refuse=refusal("BillingPolicy", "key source"))
    p = p1(source=rule_source)
    row = f.place_program(p, "ACCEPTED: program order 1, maximum 2,000, names the delivery-source address of the rule program, "
        "which has no upgrade authority", case="4")
    f.ran(row, "program 1", "AcceptProgramSourcedFinancedTab", "Reserve")
    bill = f.bill_state(p)
    f.h.check("program order 1: the bill names the rule program's address as delivery signer", bill["policy"]["source"], rule_source)
    f.h.check("program order 1: the order record commits to the policy hash", f.state("FundedClaim", p["claim"])["external_contract_hash"], p["policy_hash"])
    f.h.check("program order 1: the rule program does not sign the order", str(rule_source) in f.signers_of(row), False)
    held = f.money()
    f.place_program(p, "no effect: accepted program order 1 is sent again; the program returns the original outcome", case="4",
        what_changed="an already accepted order is sent again")
    f.h.check("re-sending accepted program order 1 holds nothing more", f.money(), held)
    return p


def program_reports(f, p, key_order):
    """Section 5: only the rule program's report is admitted on a program order; it cannot report on a key's order."""
    report = f.report(p, units=(SIGNED,))
    f.submit_report(p, report, label="refused: the carrier signs the report of 900 on program order 1 with its ed25519 key",
        error="BillingSignature")
    f.mark("5", "report", "signer: an ed25519 key instead of the rule program", refuse=refusal("BillingSignature", "report key", index=1))
    principal = f.parties["assignee"]
    digest = phase_digest(REPORT_DOMAIN, f.domain, p["bill"], p["claim"], report_bytes(report), p["policy_hash"])
    with f.fee_payer(f.submitter):
        ix = f.ix("claims_core", "submit_principal_bill_evidence", dict(bill=p["bill"], binding=pda(CORE, b"bill-authority", p["bill"]),
            authority_principal=principal["principal"], instructions=SYSVAR_INSTRUCTIONS),
            report=report, authority_generation=1, signature_ix_index=0)
        f.send("refused: a principal's owner submits the report of 900 on program order 1 through submit_principal_bill_evidence",
            [ed25519_ix(principal["owner"], digest), ix], reject=True)
    f.mark("5", "report", "instruction: the principal report instruction", refuse=refusal("AccountNotInitialized", "binding account", index=1))
    with f.fee_payer(f.submitter):
        direct = f.ix("claims_core", "submit_program_bill_evidence", dict(bill=p["bill"], source=delivery_source(RULE), rule_program=RULE),
            report=report)
        f.send("refused: the report of 900 on program order 1 is sent to ClaimsCore directly, with the rule program's address not signing",
            Instruction(CORE, bytes(direct.data), [M(p["bill"], w=True), M(delivery_source(RULE)), M(RULE)]), reject=True)
    f.mark("5", "program report", "the rule program does not sign", refuse=refusal("AccountNotSigner", "source account"))
    f.program_report(p, report, "refused: the upgradeable copy of the rule program signs the report of 900 on program order 1 "
        "at its own delivery-source address", case="5", rule=UPGRADEABLE_RULE,
        what_changed="signing program: another program than the one the order names", refuse=refusal("BillingSignature", "program signer"))
    f.program_report(p, report, "refused: the rule program signs, but the instruction shows the upgradeable program as rule program",
        case="5", rule_account=UPGRADEABLE_RULE, what_changed="rule program account: another program",
        refuse=refusal("BillingSignature", "program signer"))
    f.program_report(key_order, f.report(key_order, units=(SIGNED,)), "refused: the rule program signs a report of 900 "
        "on key order 5, which names the carrier", case="5", what_changed="order: one that names an ed25519 key",
        refuse=refusal("BillingSignature", "program signer"))
    f.program_report(p, report, "ACCEPTED: the rule program signs 900 units delivered on program order 1, by CPI", case="5")
    f.program_report(p, report, "no effect: the rule program sends its exact report on program order 1 again", case="5",
        what_changed="an admitted report is sent again")
    f.program_report(p, f.report(p, units=(SIGNED + 100,)), "refused: the rule program signs a second report of 1,000 on program order 1",
        case="5", what_changed="units in the report: 1,000 instead of the admitted 900", refuse=refusal("BillingReplay", "report replay"))
    f.submit_claim(p, f.monetary_claim(p, CLAIMED), label="ACCEPTED: the seller signs a claim of 1,400 on program order 1")
    f.mark("5", "claim", "nothing")
    f.settle(p, SIGNED, "ACCEPTED: an outside key finalizes program order 1 at 0 seconds: the rule program signed 900, "
        "the seller claimed 1,400; bill 900", case="5", early=True)
    f.pay(p, SIGNED, "ACCEPTED: an outside key collects program order 1's 900 from the lender wallet to the seller", case="5")


def pinned_rule(f):
    """Section 6: the owner pins delivery_signer = the rule program's address."""
    rule_source, other = delivery_source(RULE), delivery_source(RULE, OTHER_IDENTITY)
    pin = [signer_term(rule_source)]
    f.mandates[1] = f.create_pinned("agent-2", 1, pin,
        "ACCEPTED: the buyer's owner creates agent 2's permission pinned to delivery_signer = the rule program's address", case="6")
    f.h.check("agent 2's permission pins the rule program's address", f.pinned(f.mandates[1]), dict(revision=1, pinned=pin, active=True))

    def order(source, identity=SOURCE_IDENTITY):
        return f.evidence_order(lambda: f.order("pinned-program-order", source=source, index=1,
            overrides=dict(source_identity=identity)), delivery_list(SELLER_TERMS, source), None)
    tab = order(other, OTHER_IDENTITY)
    f.place_program(tab, "refused: agent 2's program order names the rule program's address for another source identity; "
        "the owner pinned the address for the policy's source identity", case="6", shown=tab["order_terms"],
        what_changed="delivery signer: another delivery-source address of the rule program",
        refuse=refusal("TermsNotPermitted", "pinned term"))
    tab = order(rule_source)
    f.place_program(tab, "refused: agent 2's program order naming the pinned address shows no list", case="6",
        what_changed="no list shown", refuse=refusal("TermsRequired", "no list shown"))
    row = f.place_program(tab, "ACCEPTED: agent 2's program order, maximum 2,000, names the pinned rule program's address "
        "and shows its list", case="6", shown=tab["order_terms"])
    f.ran(row, "pinned program", "AcceptProgramSourcedFinancedTab", "ReservePinnedBilling")
    f.check_accepted_bill(tab, "pinned program", rule_source)
    f.h.check("the list shown carries delivery_signer = the rule program's address", signer_term(rule_source) in tab["order_terms"], True)
    return tab


def end_state(f):
    refused = [row for row in f.h.cases if row["expected"] == "reject"]
    f.h.check("every refusal in this run was annotated and checked",
        all(row.get("commit_legs", {}).get("outcome") == "refused" for row in refused), True)


def exercise(f):
    setup(f)
    interface(f)
    cast(f)
    orders = both_in(f)
    p = program_order(f)
    key_order = f.order("order-5")
    f.place_plain(key_order, "ACCEPTED: evidence order 5, maximum 2,000, naming the carrier", case="5")
    program_reports(f, p, key_order)
    pinned_rule(f)
    one_missing(f, orders)
    after_deadlines(f, orders)
    end_state(f)


class RecoverableEarly(RuleProgram, SourceChecked, CommitLegs, RecoveryFixture):
    """Section 7: a recoverable order (delivery signer: a Registry principal) settles once both records are in."""

    def __init__(self):
        super().__init__()
        self.load_rules()

    def mark(self, case, kind, what_changed, *, refuse=None):
        return self.note(section=case, leg=kind, changed=what_changed, refuse=refuse)

    def relabel(self, case, kind, label):
        """recovery.py's helpers send under generic labels; name the accepted row for this record."""
        self.h.cases[-1]["name"] = label
        return self.mark(case, kind, "nothing")

    signers_of = PinnedTerms.signers_of


def recoverable(f):
    f.setup(financier_wallet=True)
    f.receiving_open("supplier")
    tab = f.recoverable_tab("recoverable-1", mandate=f.controlled_mandate("early-settlement"))
    f.accept_recoverable(tab)
    f.relabel("7", "recoverable order", "ACCEPTED: recoverable order 1, maximum 2,000, naming the source principal as delivery signer")
    report, claim = f.report(tab), f.monetary_claim(tab, CLAIMED)
    f.program_report(tab, report, "refused: the rule program signs the report on recoverable order 1, whose delivery signer is "
        "the source principal", case="7", what_changed="order: one that names a Registry principal",
        refuse=refusal("BillingSignature", "program signer"))
    f.principal_record(tab, report, source=True)
    f.relabel("7", "principal report", "ACCEPTED: the source principal's owner signs 900 units delivered on recoverable order 1")
    f.finalize(tab, error="BillingFinality", label="refused: finalization of recoverable order 1 at 0 seconds: the source's "
        "report is in, the seller has not claimed")
    f.mark("7", "finalization", "the seller's claim is not in", refuse=refusal("BillingFinality", "finalization"))
    f.principal_record(tab, claim)
    f.relabel("7", "principal claim", "ACCEPTED: the seller principal's owner signs a claim of 1,400 on recoverable order 1")
    f.finalize(tab, label="ACCEPTED: an outside key finalizes recoverable order 1 at 0 seconds: the source signed 900, "
        "the seller claimed 1,400; bill 900")
    f.mark("7", "finalization", "nothing")
    f.assert_final_amount(tab, SIGNED, early=True)
    f.roles = {"buyer's owner": f.parties["company"]["owner"], "agent": f.agent, "seller's owner": f.parties["supplier"]["owner"],
        "source principal's owner": f.parties["assignee"]["owner"]}


RUNS = (("early-settlement", EarlySettlement, exercise), ("early-settlement-recoverable", RecoverableEarly, recoverable))


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
        program_sha256={name: value["sha256"] for name, value in f.build["artifacts"].items()},
        rule_program=dict(source="foundation/test-programs/delivery-rule", sha256=f.rule_sha256,
            source_sha256={name: digest(RULE_SOURCE / name) for name in ("Cargo.toml", "Cargo.lock", "src/lib.rs")},
            immutable_address=str(RULE), upgradeable_address=str(UPGRADEABLE_RULE), upgrade_authority=str(UPGRADE_AUTHORITY),
            delivery_source=str(delivery_source(RULE)), source_identity=SOURCE_IDENTITY.hex()),
        swig_sha256=SWIG_SHA256, build_receipt_sha256=digest(OUT / "build-receipt.json"),
        idl_receipt_sha256=digest(OUT / "idl-receipt.json"))
    (directory / "early-settlement-receipt.json").write_text(json.dumps(manifest, indent=2) + "\n")
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
            row["early_settlement"] = row.pop("commit_legs")
    return f, f.finish(suite), index


def main():
    started = time.monotonic()
    results = [run(*item) for item in RUNS]
    if "--record" in sys.argv:
        print(json.dumps(record(results, sys.argv[sys.argv.index("--record") + 1], time.monotonic() - started)))


if __name__ == "__main__":
    main()
