#!/usr/bin/env python3
"""Three-way commit and collection weld on one scenario.

One buyer, one agent, one seller, one credit issuer and one lender. Every order
has a maximum of 100,000 units of a synthetic six-decimal token, no interest
and no seller fee. The agent's per-order limit is 100,000, the buyer's budget
is 500,000, the lender starts with 1,000,000 in its governed wallet, and the
facility and credit line limits are 500,000.

A. `accept_financed_tab` (Core, with nested Registry and Pool calls) is sent
   with one fact changed per refusal, then unchanged.
B. `authorize_funding_wallet` + Swig SignV2 + `collect_financier` is sent by a
   key that has no role in the scenario, with one fact changed per refusal,
   then unchanged.
C. The lender withdraws one unit more than its free cash, then exactly its
   free cash.

Each refusal is checked for the transaction error, the first program frame
that failed, the Anchor error name and number in that frame's log, and
byte-identical state for every writable account except the fee payer.

Three cases write account bytes from the harness instead of reaching the state
by a transaction: a delegate on the lender wallet's token account (commit) and
a different controlling program in the wallet configuration (commit and
collection). Whether any transaction could produce those states was not
examined. Their names say "harness-written state", their trace rows carry the
exact change, and the bytes are restored and checked afterwards. One case
replays a signed transaction with the amount changed by one unit; the runtime
rejects it before any program runs, and its name says so.

Run in a prepared checkout: ./dev anchor-smoke commit-legs
Record: <python> foundation/onchain/tests/commit_legs.py --record docs/evidence/<directory>
"""
import contextlib
import hashlib
import lzma
import re
import struct
import sys
import time

from solders.account import Account

from financing_fixture import *

ORDER = 100_000
PER_ORDER_LIMIT = 100_000
BUDGET = 500_000
LENDER_CASH = 1_000_000
FACILITY_LIMIT = 500_000
EARNED = 60_000
ENROLLMENT_SECONDS = 3_600
PERMISSION_SECONDS = 7_200
PROGRAM_NAMES = {str(CORE): "claims_core", str(REGISTRY): "principal_registry",
    str(POOL): "pool_issuer", str(SWIG): "swig", str(TOKEN): "spl_token", str(SYSTEM): "system"}
ANCHOR_ERROR = re.compile(r"^Program log: AnchorError (.*?)\. Error Code: (\w+)\. Error Number: (\d+)\. Error Message: (.*)$")
FAILED = re.compile(r"^Program (\w+) failed: (.*)$")
# Swig logs no error name. 3033 is read from the retained source enum
# foundation/swig/sources/swig-wallet/state/src/lib.rs (SwigAuthenticateError, base 3000).
SWIG_3033 = "PermissionDeniedProgramExecInvalidInstruction"


def variant(tab):
    """Independent copy of one prepared order; signer objects are shared."""
    terms = dict(tab["terms"])
    args = dict(tab["args"], terms=terms, seller_fee_terms=dict(tab["args"]["seller_fee_terms"]))
    return dict(tab, terms=terms, args=args, accounts=dict(tab["accounts"]), signers=list(tab["signers"]))


def classify(row):
    """Read the failure from the recorded result; nothing here is inferred from the case name."""
    raw = row["result"]
    logs = ((raw.get("meta") or {}).get("logs")) or []
    failed = [FAILED.match(line).groups() for line in logs if FAILED.match(line)]
    anchors = [ANCHOR_ERROR.match(line).groups() for line in logs if ANCHOR_ERROR.match(line)]
    error = raw.get("err")
    index = code = None
    if isinstance(error, dict) and "InstructionError" in error:
        index, detail = error["InstructionError"]
        code = detail.get("Custom") if isinstance(detail, dict) else None
    anchor = None
    if anchors:
        origin, name, number, message = anchors[-1]
        anchor = dict(origin=origin, name=name, number=int(number), message=message)
    first = failed[0][0] if failed else None
    return dict(err=error, instruction_index=index, custom_code=code,
        raising_program_id=first, raising_program=PROGRAM_NAMES.get(first, first),
        failed_frames=[PROGRAM_NAMES.get(program, program) for program, _ in failed],
        anchor_error=anchor, log_lines=len(logs))


def compact_sign_many(config, wallet, inners):
    """Swig SignV2 compact payload carrying several inner instructions."""
    metas = [M(config, w=True), M(wallet, w=True), M(SYSVAR_INSTRUCTIONS)]
    indexes = {str(m.pubkey): i for i, m in enumerate(metas)}
    compact = bytes([len(inners)])
    for inner in inners:
        if str(inner.program_id) not in indexes:
            indexes[str(inner.program_id)] = len(metas)
            metas.append(M(inner.program_id))
        selected = []
        for meta in inner.accounts:
            signer = meta.is_signer and meta.pubkey != wallet
            index = indexes.get(str(meta.pubkey))
            if index is None:
                index = len(metas)
                indexes[str(meta.pubkey)] = index
                metas.append(M(meta.pubkey, signer, meta.is_writable))
            else:
                old = metas[index]
                metas[index] = M(old.pubkey, old.is_signer or signer, old.is_writable or meta.is_writable)
            selected.append(index)
        compact += bytes([indexes[str(inner.program_id)], len(selected)]) + bytes(selected)
        compact += struct.pack("<H", len(inner.data)) + bytes(inner.data)
    return Instruction(SWIG, struct.pack("<HHI", 11, len(compact), 0) + compact + bytes([2]), metas)


class CommitLegs(FinancingFixture):
    def __init__(self):
        super().__init__()
        self.collector, self.impostor, self.unenrolled = key(60), key(61), key(62)
        self.replacement_agent, self.replacement_buyer_owner = key(63), key(64)
        self.replacement_lender_owner = key(65)
        for signer in (self.collector, self.replacement_buyer_owner, self.replacement_lender_owner):
            self.h.svm.airdrop(signer.pubkey(), 100_000_000_000)

    # -- recording ---------------------------------------------------------
    def send(self, label, instruction, signers=(), **options):
        instructions = list(instruction) if isinstance(instruction, (list, tuple)) else [instruction]
        payer = self.h.payer.pubkey()
        # Every writable account except the fee payer is observed before the
        # transaction, so a refusal is compared byte for byte afterwards.
        self.observe(*[meta.pubkey for item in instructions for meta in item.accounts
            if meta.is_writable and meta.pubkey != payer])
        return super().send(label, instruction, signers, **options)

    def note(self, *, section, leg, changed, refuse=None, **extra):
        row = self.h.cases[-1]
        row["commit_legs"] = dict(section=section, leg=leg, changed=changed,
            seconds_since_start=self.now - NOW, outcome="accepted", **extra)
        if refuse is not None:
            self.refused(row, **refuse)
        return row

    def refused(self, row, *, program=None, name=None, code=None, index=0, runtime=None, decoded=None):
        found = classify(row)
        row["passed"] = False
        assert row["expected"] == "reject", row["name"]
        assert row["observed_after"] == row["observed_before"], (row["name"], "refusal changed observed state")
        payer = row["account_keys"][0]
        writable = {meta["pubkey"] for item in row["instructions"] for meta in item["ordered_metas"]
            if meta["is_writable"]} - {payer}
        assert writable <= set(row["rollback_accounts"]), (row["name"], "writable account was not compared")
        problems = []
        if runtime is not None:
            if found["err"] != runtime: problems.append(("err", found["err"], runtime))
            if found["log_lines"]: problems.append(("program ran", found["log_lines"], 0))
            if row["signature_verified"]: problems.append(("signatures verified", True, False))
        else:
            expected = {"InstructionError": [index, {"Custom": code}]}
            if found["err"] != expected: problems.append(("err", found["err"], expected))
            if found["raising_program_id"] != str(program):
                problems.append(("raising program", found["raising_program"], PROGRAM_NAMES.get(str(program))))
            anchor = found["anchor_error"]
            if name is not None and (anchor is None or anchor["name"] != name or anchor["number"] != code):
                problems.append(("anchor error", anchor, (name, code)))
            if name is None and anchor is not None:
                problems.append(("anchor error", anchor, None))
        if problems:
            raise AssertionError((row["name"], problems, (row["result"].get("meta") or {}).get("logs")))
        row["commit_legs"].update(outcome="refused", refusal=dict(found,
            kind="runtime" if runtime is not None else "program", name_decoded_from_retained_source=decoded,
            writable_accounts_unchanged=len(writable)))
        row["passed"] = True

    @contextlib.contextmanager
    def paid_by(self, signer):
        previous, self.h.payer = self.h.payer, signer
        try:
            yield
        finally:
            self.h.payer = previous

    def clock(self, seconds):
        self.now = NOW + seconds
        self.h.svm.set_clock(Clock(100 + seconds, NOW, 0, 0, self.now))
        self.h.check(f"harness clock is {seconds:,} seconds after the scenario start",
            self.h.svm.get_clock().unix_timestamp, NOW + seconds)

    def write_state(self, address, mutate, description):
        """Harness-written account bytes. Returns what is needed to put them back."""
        before = self.h.svm.get_account(address)
        data = bytearray(before.data)
        mutate(data)
        self.h.svm.set_account(address, Account(before.lamports, bytes(data), before.owner,
            before.executable, before.rent_epoch))
        return dict(address=address, account=before, record=dict(account=str(address), description=description,
            changed_offsets=[i for i, (a, b) in enumerate(zip(before.data, data)) if a != b],
            sha256_before=hashlib.sha256(bytes(before.data)).hexdigest(),
            sha256_written=hashlib.sha256(bytes(data)).hexdigest()))

    def restore_state(self, written):
        self.h.svm.set_account(written["address"], written["account"])
        self.h.check("harness-written account restored to its recorded bytes: " + written["record"]["description"],
            hashlib.sha256(bytes(self.h.svm.get_account(written["address"]).data)).hexdigest(),
            written["record"]["sha256_before"])

    # -- scenario ----------------------------------------------------------
    def scenario(self):
        self.setup_principals()
        for role, amount in (("protocol", 0), ("fee", 0), ("issuer", 200_000)):
            self.create_custody(role, amount)
        self.create_custody("financier", LENDER_CASH, wallet=True)
        self.open_fee_agreement("main", 2_100)
        self.facility = self.open_facility()
        self.line, self.assignment_policy = self.open_line(self.facility)
        self.permission = self.permit("agent-1", 0)

    def permit(self, name, index):
        company = self.parties["company"]
        mandate_id = ident("mandate-" + name)
        mandate = pda(REGISTRY, b"mandate", self.company, mandate_id)
        self.send(f"buyer's owner permits agent {index + 1}: this seller, this lender facility, "
            f"per-order limit {PER_ORDER_LIMIT:,}, for {PERMISSION_SECONDS:,} seconds",
            self.ix("principal_registry", "create_mandate",
                dict(owner=company["owner"].pubkey(), principal=self.company, actor=self.actors[index],
                    budget=self.budget, mandate=mandate),
                args=dict(mandate_id=mandate_id, supplier=self.parties["supplier"]["owner"].pubkey(),
                    source=self.facility, per_operation_limit=PER_ORDER_LIMIT,
                    expires_at=NOW + PERMISSION_SECONDS)), [company["owner"]])
        return mandate

    def open_line_limit(self, name, limit):
        company, issuer = self.parties["company"], self.parties["issuer"]
        terms = self.interest_terms()
        line_id = ident("credit-line-" + name)
        line = pda(CORE, b"credit-line", self.company, line_id)
        self.send(f"buyer's owner and issuer's owner open a second credit line with limit {limit:,}",
            self.ix("claims_core", "open_customer_credit_line",
                dict(owner=company["owner"].pubkey(), company=self.company, issuer_owner=issuer["owner"].pubkey(),
                    issuer=issuer["principal"], issuer_custody=issuer["custody"], facility=self.facility,
                    credit_line=line, protocol_custody=self.parties["protocol"]["custody"]),
                line_id=line_id, limit=limit, interest_terms=terms, assignment_policy_hash=self.assignment_policy,
                owner_generation=company["generation"], issuer_generation=issuer["generation"]),
            [company["owner"], issuer["owner"]])
        return line

    def order(self, name, *, agent=None, generation=1):
        tab = self.acceptance(name, cap=ORDER, maximum_interest=0, fees=False, mandate=self.permission)
        tab["terms"]["expires_at"] = NOW + PERMISSION_SECONDS
        tab["terms"]["credential_generation"] = generation
        return tab if agent is None else self.signed_by(tab, agent)

    def signed_by(self, tab, agent):
        operation_id = self.operation(b"accept", agent.pubkey(), tab["args"]["request_id"])
        receipt = self.receipt(self.company, operation_id)
        tab["accounts"].update(credential=agent.pubkey(), receipt=receipt,
            registry_operation=pda(REGISTRY, b"operation", self.company, operation_id))
        tab["receipt"] = receipt
        tab["signers"][0] = agent
        return tab

    def commit_instruction(self, tab, roles=None):
        return self.ix("claims_core", "accept_financed_tab", tab["accounts"],
            remaining=self.remaining(roles or ["company", "issuer", "supplier", tab["financier"]]), **tab["args"])

    def commit(self, tab, label, *, leg, changed, signers=None, roles=None, refuse=None, **extra):
        self.send(label, self.commit_instruction(tab, roles), tab["signers"] if signers is None else signers,
            reject=refuse is not None, receipt=tab["receipt"])
        return self.note(section="A", leg=leg, changed=changed, refuse=refuse, **extra)

    def replay_with_changed_amount(self, tab, label):
        """Sign the exact order, then change one byte of its amount and keep the signatures."""
        instruction = self.commit_instruction(tab)
        supplied = {signer.pubkey() for signer in tab["signers"]}
        metas = [M(m.pubkey, m.is_signer or m.pubkey in supplied, m.is_writable) for m in instruction.accounts]
        signed = self.h.transaction([Instruction(CORE, bytes(instruction.data), metas)], tab["signers"])
        needle = bytes(tab["terms"]["source"]) + u64(ORDER)
        wire, data = bytes(signed), bytes(instruction.data)
        assert wire.count(needle) == 1 and data.count(needle) == 1
        at, inner_at = wire.index(needle) + 32, data.index(needle) + 32
        changed = VersionedTransaction.from_bytes(wire[:at] + u64(ORDER + 1) + wire[at + 8:])
        assert changed.signatures == signed.signatures and bytes(changed.message) != bytes(signed.message)
        altered = Instruction(CORE, data[:inner_at] + u64(ORDER + 1) + data[inner_at + 8:], metas)
        self.observe(*[m.pubkey for m in metas if m.is_writable and m.pubkey != self.h.payer.pubkey()], tab["receipt"])
        self.h.send(label, [altered], tab["signers"], fail="any", unchanged=self.h.observe, transaction=changed)
        return dict(wire_offset=at, signed_amount=ORDER, sent_amount=ORDER + 1,
            signatures_kept=len(changed.signatures))

    def set_budget(self, label, ceiling):
        company = self.parties["company"]
        operation_id = ident("budget-" + label)
        amendment = pda(REGISTRY, b"budget-amendment", self.company, operation_id)
        self.send(label, self.ix("principal_registry", "amend_allowance",
            dict(owner=company["owner"].pubkey(), principal=self.company, budget=self.budget, amendment=amendment),
            args=dict(operation_id=operation_id, authority_generation=company["generation"],
                expected_revision=self.registry.read(self.h.svm, self.budget, "Budget")["revision"],
                ceiling=ceiling)), [company["owner"]])

    def set_facility(self, label, *, limit, enabled=True):
        lender = self.parties["financier"]
        raw = ident("facility-policy-" + label)
        receipt = pda(CORE, b"facility-policy", self.facility, self.operation(b"facility-policy", lender["principal"], raw))
        self.send(label, self.ix("claims_core", "manage_funding_facility",
            dict(owner=lender["owner"].pubkey(), financier=lender["principal"], facility=self.facility,
                facility_authority=pda(CORE, b"facility-authority", self.facility), receipt=receipt),
            request_id=raw, expected_revision=self.state("FundingFacility", self.facility)["revision"],
            commitment_limit=limit, enabled=enabled, authority_generation=lender["generation"]), [lender["owner"]])

    def add_cash(self, label, amount):
        lender = self.parties["financier"]
        raw = ident("contribute-" + label)
        receipt = self.receipt(lender["principal"], self.operation(b"capital-contribute", lender["owner"].pubkey(), raw))
        accounts = dict(owner=lender["owner"].pubkey(), principal=lender["principal"], custody=lender["custody"],
            custody_token=lender["custody_token"], receipt=receipt)
        accounts["from"] = lender["owner_ata"]
        self.send(label, self.ix("claims_core", "contribute_financier_capital", accounts,
            remaining=self.remaining(["financier"]), args=dict(request_id=raw, amount=amount,
                expected_revision=self.state("FinancierCustody", lender["custody"])["revision"],
                authority_generation=lender["generation"])), [lender["owner"]], receipt=receipt)

    def lender_cash(self):
        custody = self.state("FinancierCustody", self.parties["financier"]["custody"])
        promised = custody["unused"] + custody["payable"]
        return dict(wallet=self.h.amount(self.parties["financier"]["custody_token"]),
            accounted=custody["accounted_cash"], promised=promised, free=custody["accounted_cash"] - promised)

    def rekey_agent(self, label, new_key, *, expected_generation, valid_until):
        company = self.parties["company"]
        self.send(label, self.ix("principal_registry", "rotate_actor",
            dict(owner=company["owner"].pubkey(), principal=self.company, actor=self.actors[0]),
            args=dict(expected_generation=expected_generation, new_key=new_key.pubkey(),
                valid_from=NOW - 100, valid_until=valid_until)), [company["owner"]])

    def rotate_owner(self, role, replacement, label):
        party = self.parties[role]
        previous = party["owner"]
        operation_id = ident("rotate-owner-" + role)
        receipt = pda(REGISTRY, b"owner-rotation", party["principal"], operation_id)
        self.send(label, self.ix("principal_registry", "rotate_principal_owner",
            dict(principal=party["principal"], current_owner=previous.pubkey(), new_owner=replacement.pubkey(),
                receipt=receipt, recovery_policy=pda(REGISTRY, b"recovery-policy", party["principal"])),
            args=dict(operation_id=operation_id, expected_authority_generation=party["generation"],
                new_owner=replacement.pubkey())), [previous, replacement])
        party["owner"], party["generation"] = replacement, party["generation"] + 1
        self.h.check(role + " owner generation after rotation",
            self.registry.read(self.h.svm, party["principal"], "Principal")["authority_generation"], party["generation"])
        return previous

    def slack(self, label):
        """Recorded headroom before a refusal that has no restoring control."""
        budget = self.registry.read(self.h.svm, self.budget, "Budget")
        facility = self.state("FundingFacility", self.facility)
        line = self.state("CustomerCreditLine", self.line)
        for name, value in (
                ("remaining budget", budget["ceiling"] - budget["reserved"] - budget["consumed"]),
                ("remaining facility limit", facility["commitment_limit"] - facility["reserved"] - facility["drawn"]),
                ("remaining credit line", line["limit"] - line["reserved"] - line["principal_outstanding"] - line["fee_exposure"]),
                ("lender free cash", self.lender_cash()["free"])):
            self.h.check(f"{label}: {name} covers one more order of {ORDER:,}", value >= ORDER, True)

    # -- collection --------------------------------------------------------
    def envelope(self, tail):
        """Registry authorization for an arbitrary Swig instruction, hashed as actually sent."""
        lender = self.parties["financier"]
        binding = self.registry.read(self.h.svm, lender["binding"], "FundingWalletBinding")
        args = dict(binding=lender["binding"], nonce=binding["nonce"], expires_at=self.now + 300,
            authority_generation=0, action=1, bundle_hash=bytes(32))
        accounts = dict(config=lender["config"], wallet=lender["wallet"], binding=lender["binding"],
            principal=lender["principal"], caller=self.h.payer.pubkey(), instructions=SYSVAR_INSTRUCTIONS)
        authorize = self.ix("principal_registry", "authorize_funding_wallet", accounts, args=args)
        raw = canonical_instruction(tail, [authorize, tail], self.h.payer.pubkey())
        args["bundle_hash"] = hashlib.sha256(b"DEXTER:FUNDING-SWIG-BUNDLE:1" + bytes(REGISTRY) + bytes(CORE)
            + bytes(SWIG) + self.domain + bytes(lender["principal"]) + bytes(lender["binding"]) + u64(args["nonce"])
            + struct.pack("<q", args["expires_at"]) + u64(0) + bytes([1]) + raw).digest()
        return [self.ix("principal_registry", "authorize_funding_wallet", accounts, args=args), tail]

    def with_accounts(self, request, **accounts):
        changed = dict(request, accounts=dict(request["accounts"], **accounts))
        changed["instruction"] = self.ix("claims_core", "collect_financier", changed["accounts"],
            remaining=self.remaining(request["roles"]), **request["args"])
        return changed

    def collect_case(self, request, label, *, changed, refuse=None, instructions=None, **extra):
        lender = self.parties["financier"]
        if instructions is None:
            instructions = self.envelope(compact_sign(lender["config"], lender["wallet"], request["instruction"]))
        self.send(label, instructions, request["signers"], reject=refuse is not None, receipt=request["receipt"])
        return self.note(section="B", leg="weld", changed=changed, refuse=refuse, **extra)


def commit_refusals_before_acceptance(f, o1):
    buyer, issuer, lender, seller, other = [f.parties[x] for x in ("company", "issuer", "financier", "supplier", "assignee")]
    core = lambda name, code: dict(program=CORE, name=name, code=code)
    registry = lambda name, code: dict(program=REGISTRY, name=name, code=code)

    # Q3: the deal. Seller and the credit issuer's owner sign these exact terms.
    f.commit(o1, "Q3 refused: seller does not sign order 1 (maximum 100,000)",
        signers=[f.agent, issuer["owner"]], refuse=core("OwnerAuthority", 6001),
        leg="Q3", changed="seller's signature absent")
    f.commit(o1, "Q3 refused: credit issuer's owner does not sign order 1 (maximum 100,000)",
        signers=[f.agent, seller["owner"]], refuse=core("OwnerAuthority", 6001),
        leg="Q3", changed="credit issuer's owner signature absent")
    v = variant(o1)
    v["accounts"]["supplier"] = f.impostor.pubkey()
    v["signers"][2] = f.impostor
    f.commit(v, "Q3 refused: a key that is not the seller signs order 1 in the seller's place",
        refuse=core("FundingOwner", 6023), leg="Q3", changed="seller slot signed by another key")
    v = variant(o1)
    v["accounts"]["issuer_owner"] = f.impostor.pubkey()
    v["signers"][1] = f.impostor
    f.commit(v, "Q3 refused: a key that is not the credit issuer's owner signs order 1 in its place",
        refuse=core("FundingOwner", 6023), leg="Q3", changed="issuer-owner slot signed by another key")
    v = variant(o1)
    v["terms"]["supplier"] = other["owner"].pubkey()
    f.commit(v, "Q3 refused: order 1 names a payout key other than the signing seller's",
        refuse=core("FundingTerms", 6031), leg="Q3", changed="payee in the terms")
    v = variant(o1)
    v["args"]["seller_fee_terms"]["gross_cap"] = ORDER - 1
    f.commit(v, "Q3 refused: order 1 says 100,000 but the seller-side terms say 99,999",
        refuse=core("FundingTerms", 6031), leg="Q3", changed="amount in the seller-side terms")
    v = variant(o1)
    v["terms"]["expires_at"] = NOW
    f.commit(v, "Q3 refused: order 1 carries an expiry that has already arrived",
        refuse=core("FundingTerms", 6031), leg="Q3", changed="expiry in the terms")
    v = variant(o1)
    v["args"]["unit_price"] = 2
    f.commit(v, "Q3 refused: order 1 is sent with unit price 2; the terms commit to unit price 1",
        refuse=core("FundingUsage", 6044), leg="Q3", changed="price")
    v = variant(o1)
    v["args"]["assignment_policy_hash"] = ident("another-assignment-policy")
    f.commit(v, "Q3 refused: order 1 carries assignment terms the issuer's credit line did not agree",
        refuse=core("FundingAssignment", 6046), leg="Q3", changed="assignment terms")
    tamper = f.replay_with_changed_amount(o1,
        "Q3 refused by the runtime signature check, before any program runs: order 1 signed at 100,000 is sent with the amount changed to 100,001")
    f.note(section="A", leg="Q3", changed="amount, after all parties signed", tamper=tamper,
        refuse=dict(runtime="SignatureFailure"))
    v = variant(o1)
    v["terms"]["supplier"] = other["owner"].pubkey()
    v["args"]["seller_fee_terms"]["supplier"] = other["owner"].pubkey()
    v["accounts"].update(supplier=other["owner"].pubkey(), supplier_principal=other["principal"])
    v["signers"][2] = other["owner"]
    f.commit(v, "Q1+Q3 refused: a different registered seller signs order 1; the owner's permission names the first seller",
        roles=["company", "issuer", "assignee", "financier"], refuse=registry("IntentMismatch", 6010),
        leg="Q1+Q3", changed="seller (principal, payout key and signature)")

    # Q1: permission. Agent enrolled, current, inside the owner's limits and budget.
    f.commit(f.signed_by(variant(o1), f.unenrolled),
        "Q1 refused: a key the owner never enrolled signs order 1 as the agent",
        refuse=registry("CredentialKey", 6005), leg="Q1", changed="agent key")
    v = variant(o1)
    v["terms"]["cap"] = ORDER + 1
    v["args"]["seller_fee_terms"]["gross_cap"] = ORDER + 1
    f.commit(v, "Q1 refused: order of 100,001 against a per-order limit of 100,000",
        refuse=registry("PerOperationLimit", 6011), leg="Q1", changed="amount, one unit above the per-order limit")
    v = variant(o1)
    v["terms"]["expires_at"] = NOW + PERMISSION_SECONDS + 1
    f.commit(v, "Q1 refused: order 1 would stay open one second longer than the agent's permission",
        refuse=registry("Expired", 6009), leg="Q1", changed="expiry, one second past the permission")
    f.set_budget("buyer's owner lowers the budget from 500,000 to 99,999", ORDER - 1)
    f.commit(o1, "Q1 refused: order 1 (100,000) against a remaining budget of 99,999",
        refuse=registry("BudgetExhausted", 6012), leg="Q1", changed="budget, one unit below the order")
    f.set_budget("buyer's owner restores the budget to 500,000", BUDGET)

    # Q2: money. Lender's free cash and facility cover the order's maximum.
    f.set_facility("lender lowers its facility limit from 500,000 to 99,999", limit=ORDER - 1)
    f.commit(o1, "Q2 refused: order 1 (100,000) against a lender facility limit of 99,999",
        refuse=core("FundingFacility", 6025), leg="Q2", changed="facility limit, one unit below the order")
    f.set_facility("lender restores its facility limit to 500,000", limit=FACILITY_LIMIT)
    f.set_facility("lender switches its facility off for new orders", limit=FACILITY_LIMIT, enabled=False)
    f.commit(o1, "Q2 refused: order 1 while the lender's facility is switched off for new orders",
        refuse=core("FundingAdmission", 6040), leg="Q2", changed="lender approval switched off")
    f.set_facility("lender switches its facility back on", limit=FACILITY_LIMIT, enabled=True)
    tight = f.open_line_limit("limit-99999", ORDER - 1)
    v = variant(o1)
    v["accounts"]["credit_line"] = tight
    f.commit(v, "Q2 refused: order 1 (100,000) drawn on a credit line whose limit is 99,999",
        refuse=core("FundingCreditLine", 6026), leg="Q2", changed="credit line limit, one unit below the order")
    f.withdraw(f.withdrawal("leave-99999-free", LENDER_CASH - (ORDER - 1)),
        label="lender withdraws 900,001 of its 1,000,000, leaving 99,999 free")
    f.h.check("lender free cash is one unit below the order", f.lender_cash()["free"], ORDER - 1)
    f.commit(o1, "Q2 refused: order 1 (100,000) against lender free cash of 99,999",
        refuse=core("FundingCashShortfall", 6033), leg="Q2", changed="lender free cash, one unit below the order")
    f.add_cash("lender adds 1 unit: free cash is 100,000", 1)
    f.h.check("lender free cash equals the order", f.lender_cash()["free"], ORDER)

    def delegate(data):
        data[72:76] = struct.pack("<I", 1)
        data[76:108] = bytes(f.impostor.pubkey())
        data[121:129] = u64(1)
    written = f.write_state(lender["custody_token"], delegate,
        "SPL delegate set to an outside key for 1 unit on the lender wallet's token account")
    f.commit(o1, "Q2 refused (harness-written state): order 1 while a second spender is set on the lender wallet's token account",
        refuse=core("FundingCashControl", 6034), leg="Q2", changed="delegate on the lender's token account",
        harness_written_state=written["record"])
    f.restore_state(written)
    written = f.write_state(lender["config"], lambda data: data.__setitem__(slice(64, 96), bytes(f.impostor.pubkey())),
        "controlling program in the lender's wallet configuration replaced by an outside key")
    f.commit(o1, "Q2 refused (harness-written state): order 1 after the lender wallet's configuration differs from what was bound",
        refuse=core("WalletGraph", 6019), leg="Q2", changed="wallet configuration since binding",
        harness_written_state=written["record"])
    f.restore_state(written)


def accepted_commit(f, o1):
    buyer, issuer, lender, seller = [f.parties[x] for x in ("company", "issuer", "financier", "supplier")]
    budget_before = f.registry.read(f.h.svm, f.budget, "Budget")
    cash_before = f.lender_cash()
    row = f.commit(o1, "ACCEPTED: order 1, maximum 100,000; agent enrolled and inside its 100,000 limit and the 500,000 budget, "
        "lender free cash 100,000, seller and credit issuer's owner signed",
        leg="Q1+Q2+Q3", changed="nothing")
    logs = row["result"]["logs"]
    f.h.check("the commit is one top-level instruction", len(row["instructions"]), 1)
    f.h.check("Core calls Registry inside the commit", f"Program {REGISTRY} invoke [2]" in logs, True)
    f.h.check("Core calls Pool inside the commit", f"Program {POOL} invoke [2]" in logs, True)
    tx = VersionedTransaction.from_bytes(__import__("base64").b64decode(row["transaction_base64"]))
    signers = [str(k) for k in tx.message.account_keys[:len(tx.signatures)]]
    f.h.check("commit signers are fee payer, agent, credit issuer's owner and seller",
        sorted(signers), sorted(str(k.pubkey()) for k in (f.h.payer, f.agent, issuer["owner"], seller["owner"])))
    f.h.check("buyer's owner does not sign the commit", str(buyer["owner"].pubkey()) in signers, False)
    f.h.check("lender's owner does not sign the commit", str(lender["owner"].pubkey()) in signers, False)
    budget = f.registry.read(f.h.svm, f.budget, "Budget")
    f.h.check("budget hold before the commit", budget_before["reserved"], 0)
    f.h.check("budget hold after the commit", budget["reserved"], ORDER)
    f.h.check("budget ceiling unchanged", budget["ceiling"], BUDGET)
    reservation = f.registry.read(f.h.svm, o1["reservation"], "Reservation")
    f.h.check("Registry reservation holds the order maximum", reservation["remaining"], ORDER)
    f.h.check("Registry reservation consumed nothing yet", reservation["consumed"], 0)
    cash = f.lender_cash()
    f.h.check("lender promised total before the commit", cash_before["promised"], 0)
    f.h.check("lender promised total after the commit", cash["promised"], ORDER)
    f.h.check("lender free cash after the commit", cash["free"], 0)
    f.h.check("the commit moves no tokens out of the lender wallet", cash["wallet"], cash_before["wallet"])
    f.h.check("facility records the promise", f.state("FundingFacility", f.facility)["reserved"], ORDER)
    f.h.check("credit line records the promise", f.state("CustomerCreditLine", f.line)["reserved"], ORDER)
    claim = f.state("FundedClaim", o1["claim"])
    for field, expected in (("remaining", ORDER), ("payable", 0), ("debt", 0), ("paid", 0), ("revision", 1),
            ("custody", lender["custody"]), ("facility", f.facility), ("credit_line", f.line),
            ("supplier_principal", seller["principal"]), ("unit_price", 1)):
        f.h.check("Tab record " + field, claim[field], expected)
    for field, expected in (("cap", ORDER), ("supplier", seller["owner"].pubkey()), ("principal", f.company),
            ("actor", f.actors[0]), ("mandate", f.permission), ("source", f.facility),
            ("expires_at", NOW + PERMISSION_SECONDS)):
        f.h.check("Tab record terms " + field, claim["terms"][field], expected)
    f.positions("company", customer_reserved=ORDER, customer_principal=0)
    f.positions("issuer", unused_supplier=ORDER)
    f.positions("supplier", supplier_committed=ORDER, supplier_receivable=0)
    f.positions("financier", funding_reserved=ORDER)


def collection(f, o1):
    buyer, issuer, lender, seller, other = [f.parties[x] for x in ("company", "issuer", "financier", "supplier", "assignee")]
    registry = lambda name, code: dict(program=REGISTRY, name=name, code=code, index=0)
    core = lambda name, code: dict(program=CORE, name=name, code=code, index=1)
    run = f.earn(o1, "order-1-work", EARNED, label="seller reports 60,000 of completed work on order 1")
    usual_payer = f.h.payer
    with f.paid_by(f.collector):
        f.send("outside key prepares its own token account", f.h.ata_ix(f.collector.pubkey(), f.mint))
        outside_ata = f.h.ata(f.collector.pubkey(), f.mint)
        legit = f.collection(o1, run, EARNED)
        config, wallet = lender["config"], lender["wallet"]
        f.collect_case(f.with_accounts(legit, destination=other["owner_ata"]),
            "weld refused: collection of 60,000 with the payee swapped to another principal's token account",
            refuse=core("FundingTokenRouting", 6035), changed="payee token account")
        f.collect_case(f.with_accounts(legit, destination=outside_ata),
            "weld refused: collection of 60,000 with the payee swapped to the collector's own token account",
            refuse=core("FundingTokenRouting", 6035), changed="payee token account")
        f.collect_case(f.collection(o1, run, EARNED + 1),
            "weld refused: collection of 60,001 when the seller earned 60,000",
            refuse=core("FundingTerms", 6031), changed="amount, one unit above what was earned")
        extra_transfer = Instruction(TOKEN, bytes([12]) + u64(1) + bytes([6]),
            [M(lender["custody_token"], w=True), M(f.mint), M(outside_ata, w=True), M(wallet, True)])
        f.collect_case(legit, "weld refused: the wallet envelope carries the collection plus a second transfer of 1 unit to the collector",
            instructions=f.envelope(compact_sign_many(config, wallet, [legit["instruction"], extra_transfer])),
            refuse=registry("FundingAction", 6030), changed="second inner instruction in the wallet envelope")
        envelope = f.envelope(compact_sign(config, wallet, legit["instruction"]))
        unrelated = Instruction(SYSTEM, struct.pack("<IQ", 2, 1),
            [M(f.collector.pubkey(), True, True), M(f.impostor.pubkey(), w=True)])
        f.collect_case(legit, "weld refused: an extra instruction follows the wallet envelope",
            instructions=[*envelope, unrelated], refuse=registry("WalletEnvelope", 6023),
            changed="third top-level instruction")
        f.collect_case(legit, "weld refused: the wallet is asked to sign the collection without the Registry authorization",
            instructions=[compact_sign(config, wallet, legit["instruction"])],
            refuse=dict(program=SWIG, name=None, code=3033, index=0, decoded=SWIG_3033),
            changed="Registry authorization absent")
        f.collect_case(f.with_accounts(legit, custody_token=issuer["custody_token"]),
            "weld refused: collection names another party's token account as the source",
            refuse=registry("FundingAction", 6030), changed="source token account")
        f.collect_case(f.with_accounts(legit, custody=issuer["custody"], custody_token=issuer["custody_token"]),
            "weld refused: collection names another party's cash record and token account as the source",
            refuse=core("FundingTerms", 6031), changed="source cash record and token account")
        written = f.write_state(config, lambda data: data.__setitem__(slice(64, 96), bytes(f.impostor.pubkey())),
            "controlling program in the lender's wallet configuration replaced by an outside key")
        f.collect_case(legit, "weld refused (harness-written state): collection after the lender wallet's configuration differs from what was bound",
            refuse=registry("WalletGraph", 6019), changed="wallet configuration since binding",
            harness_written_state=written["record"])
        f.restore_state(written)

        seller_before = f.h.amount(f.h.ata(seller["owner"].pubkey(), f.mint))
        wallet_before = f.h.amount(lender["custody_token"])
        outside_before = f.h.amount(outside_ata)
        row = f.collect_case(legit, "ACCEPTED: a key with no role in the order collects the seller's earned 60,000 from the lender wallet to the seller",
            changed="nothing")
    tx = VersionedTransaction.from_bytes(__import__("base64").b64decode(row["transaction_base64"]))
    f.h.check("collection carries exactly one signature", len(tx.signatures), 1)
    f.h.check("the only signer and fee payer is the outside key", row["account_keys"][0], str(f.collector.pubkey()))
    for name, signer in (("buyer's owner", buyer["owner"]), ("agent", f.agent), ("lender's owner", lender["owner"]),
            ("credit issuer's owner", issuer["owner"]), ("seller", seller["owner"]),
            ("fee payer of every other transaction", usual_payer)):
        f.h.check(name + " is not among the collection's account keys", str(signer.pubkey()) in row["account_keys"], False)
    f.h.check("seller receives exactly what it earned", f.h.amount(f.h.ata(seller["owner"].pubkey(), f.mint)) - seller_before, EARNED)
    f.h.check("lender wallet pays exactly what the seller earned", wallet_before - f.h.amount(lender["custody_token"]), EARNED)
    f.h.check("the collector receives nothing", f.h.amount(outside_ata) - outside_before, 0)
    claim = f.state("FundedClaim", o1["claim"])
    f.h.check("Tab record paid", claim["paid"], EARNED)
    f.h.check("Tab record still payable", claim["payable"], 0)
    f.h.check("Tab record unused remainder", claim["remaining"], ORDER - EARNED)
    f.h.check("buyer's debt equals the completed work", claim["debt"], EARNED)
    return run


def lender_withdrawal(f):
    lender = f.parties["financier"]
    f.add_cash("lender adds 50,000: 90,000 in the wallet, 40,000 still promised to order 1", 50_000)
    cash = f.lender_cash()
    f.h.check("lender free cash before withdrawal", cash["free"], 50_000)
    f.h.check("lender promised total before withdrawal", cash["promised"], ORDER - EARNED)
    f.withdraw(f.withdrawal("free-plus-one", 50_001), reject=True,
        label="money refused: lender withdraws 50,001 when 50,000 is free and 40,000 is promised")
    f.note(section="C", leg="Q2", changed="amount, one unit above free cash",
        refuse=dict(program=CORE, name="FundingCashShortfall", code=6033, index=1))
    owner_before = f.h.amount(lender["owner_ata"])
    f.withdraw(f.withdrawal("exactly-free", 50_000),
        label="ACCEPTED: lender withdraws exactly its free 50,000")
    f.note(section="C", leg="Q2", changed="nothing")
    f.h.check("lender owner receives its free cash", f.h.amount(lender["owner_ata"]) - owner_before, 50_000)
    f.h.check("promised cash stays in the wallet", f.h.amount(lender["custody_token"]), ORDER - EARNED)


def later_permission_and_approval_changes(f):
    buyer, lender = f.parties["company"], f.parties["financier"]
    registry = lambda name, code: dict(program=REGISTRY, name=name, code=code)
    f.add_cash("lender adds 400,000 so four more orders of 100,000 have cover", 400_000)

    f.rotate_owner("financier", f.replacement_lender_owner, "lender's owner hands control to a new owner key")
    o2 = f.order("order-2")
    f.slack("before order 2")
    f.commit(o2, "Q2 refused: order 2 (100,000) after the lender's owner changed and before the new owner approved the facility",
        refuse=dict(program=CORE, name="FundingAuthorityGeneration", code=6068),
        leg="Q2", changed="lender's facility approval is from the previous owner")
    f.set_facility("lender's new owner approves the facility again (limit 500,000)", limit=FACILITY_LIMIT)
    f.commit(o2, "ACCEPTED: order 2 (100,000) after the lender's new owner approved the facility",
        leg="Q1+Q2+Q3", changed="nothing")

    f.rekey_agent("buyer's owner replaces agent 1's key", f.replacement_agent,
        expected_generation=1, valid_until=NOW + ENROLLMENT_SECONDS)
    f.commit(f.order("order-3"), "Q1 refused: the agent's old key signs order 3 (100,000) after the owner replaced it",
        refuse=registry("CredentialKey", 6005), leg="Q1", changed="agent key replaced by the owner")
    f.commit(f.order("order-3", agent=f.replacement_agent, generation=2),
        "ACCEPTED: order 3 (100,000) signed by the agent's replacement key", leg="Q1+Q2+Q3", changed="nothing")

    f.send("buyer's owner revokes agent 1", f.ix("principal_registry", "revoke_actor",
        dict(owner=buyer["owner"].pubkey(), principal=f.company, actor=f.actors[0]), expected_generation=2),
        [buyer["owner"]])
    f.slack("before order 4")
    f.commit(f.order("order-4", agent=f.replacement_agent, generation=2),
        "Q1 refused: the revoked agent signs order 4 (100,000)",
        refuse=registry("CredentialInactive", 6004), leg="Q1", changed="agent revoked by the owner")

    f.rekey_agent("buyer's owner enrolls agent 1 again until 3,600 seconds after the scenario start",
        f.replacement_agent, expected_generation=3, valid_until=NOW + ENROLLMENT_SECONDS)
    f.clock(ENROLLMENT_SECONDS - 1)
    f.commit(f.order("order-4", agent=f.replacement_agent, generation=4),
        "ACCEPTED: order 4 (100,000) at 3,599 seconds, the last second of the agent's enrollment",
        leg="Q1+Q2+Q3", changed="nothing")
    f.clock(ENROLLMENT_SECONDS)
    f.slack("before order 5")
    f.commit(f.order("order-5", agent=f.replacement_agent, generation=4),
        "Q1 refused: order 5 (100,000) at 3,600 seconds, when the agent's enrollment has ended",
        refuse=registry("Expired", 6009), leg="Q1", changed="clock, one second past the agent's enrollment")

    f.rekey_agent("buyer's owner extends agent 1's enrollment to 7,200 seconds", f.replacement_agent,
        expected_generation=4, valid_until=NOW + PERMISSION_SECONDS)
    f.rotate_owner("company", f.replacement_buyer_owner, "buyer's owner hands control to a new owner key")
    f.slack("before order 5 under the new owner")
    f.commit(f.order("order-5", agent=f.replacement_agent, generation=5),
        "Q1 refused: order 5 (100,000) by an agent the previous buyer owner enrolled, after the buyer's owner changed",
        refuse=registry("AuthorityEpoch", 6034), leg="Q1", changed="buyer's owner changed since the agent was enrolled")


def exercise(f):
    f.scenario()
    o1 = f.order("order-1")
    commit_refusals_before_acceptance(f, o1)
    accepted_commit(f, o1)
    collection(f, o1)
    lender_withdrawal(f)
    later_permission_and_approval_changes(f)


def case_index(f, suite):
    rows = []
    for index, row in enumerate(f.h.cases):
        note = row.get("commit_legs") or {}
        refusal = note.get("refusal") or {}
        anchor = refusal.get("anchor_error") or {}
        meta = row["result"].get("meta") or row["result"]
        rows.append(dict(suite=suite, index=index, name=row["name"],
            section=note.get("section", "setup"), leg=note.get("leg"), changed=note.get("changed"),
            expected=row["expected"], outcome=note.get("outcome", "refused" if row["expected"] == "reject" else "accepted"),
            passed=row["passed"], refusal_kind=refusal.get("kind"), error=row["result"].get("err"),
            error_name=anchor.get("name") or refusal.get("name_decoded_from_retained_source"),
            error_name_source=("program log" if anchor else "retained Swig source enum"
                if refusal.get("name_decoded_from_retained_source") else None),
            error_code=refusal.get("custom_code"),
            raising_program=refusal.get("raising_program"), raising_program_id=refusal.get("raising_program_id"),
            failed_frames=refusal.get("failed_frames"), error_source=anchor.get("origin"),
            error_message=anchor.get("message"), instruction_index=refusal.get("instruction_index"),
            harness_written_state=note.get("harness_written_state"), tamper=note.get("tamper"),
            seconds_since_start=note.get("seconds_since_start"),
            signature_verified=row["signature_verified"], wire_bytes=row["wire_bytes"],
            compute_units_consumed=meta.get("compute_units_consumed"),
            observed_accounts=len(row["observed_before"]), rollback_accounts=len(row["rollback_accounts"]),
            writable_accounts_unchanged=refusal.get("writable_accounts_unchanged")))
    return rows


def record(f, summary, directory, seconds):
    """Copy the finished run into a publication directory; no second execution."""
    directory = Path(directory).resolve()
    directory.mkdir(parents=True, exist_ok=True)
    raw = Path(summary["trace"]).read_bytes()
    packed = directory / "commit-legs-trace.json.xz"
    packed.write_bytes(lzma.compress(raw))
    assert lzma.decompress(packed.read_bytes()) == raw
    index = case_index(f, "commit-legs")
    (directory / "case-index.json").write_text(json.dumps(index, indent=1, default=json_value) + "\n")
    annotated = [row for row in index if row["section"] != "setup"]
    receipt = dict(summary, trace="commit-legs-trace.json.xz", trace_xz_sha256=digest(packed),
        annotated_cases=len(annotated),
        annotated_refusals=sum(row["outcome"] == "refused" for row in annotated),
        annotated_acceptances=sum(row["outcome"] == "accepted" for row in annotated),
        run_seconds=round(seconds, 2), engine="solders 0.29.0 LiteSVM",
        program_sha256={name: value["sha256"] for name, value in f.build["artifacts"].items()},
        swig_sha256=SWIG_SHA256)
    for name in ("build-receipt.json", "idl-receipt.json"):
        receipt[name.replace("-", "_").replace(".json", "_sha256")] = digest(OUT / name)
        (directory / name).write_bytes((OUT / name).read_bytes())
    (directory / "commit-legs-receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    return receipt


def main():
    started = time.monotonic()
    f = CommitLegs()
    try:
        exercise(f)
    except Exception as error:
        f.finish("commit-legs", error=error)
        raise
    summary = f.finish("commit-legs")
    if "--record" in sys.argv:
        print(json.dumps(record(f, summary, sys.argv[sys.argv.index("--record") + 1], time.monotonic() - started)))


if __name__ == "__main__":
    main()
