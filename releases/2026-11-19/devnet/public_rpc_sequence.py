#!/usr/bin/env python3
"""Three lender-backed orders, end to end, sent to a validator over JSON-RPC.

One buyer with two agents, one seller, one credit issuer and one lender whose
cash sits in a Swig wallet bound to its principal. A synthetic six-decimal
token is created by the run. No interest and no seller fee.

  per-order limit 100,000 | budget 500,000 | facility and credit line 500,000
  lender cash 99,999, then 150,000 | order 100,000 | earned 60,000

Change 5: no step writes a receipt account. Each order is a Tab at its
mandate's next order number; every chosen-amount money move takes a number
from the window of the record it writes (the mandate, the lender's custody, the
seller's and the issuer's books, each payment route's repayment record); a
settled order closes, and every deposit goes back to the account that paid it.

  1. setup
  2. the commit (ClaimsCore accept_financed_tab at order number 1 of agent 1's
     mandate), the same order sent four times:
       money       lender free cash 99,999, one unit below the order   refused
       permission  amount 100,001, one unit above the agent's limit    refused
       deal        unit price 2 where the agreed terms commit to 1      refused
       unchanged                                                       accepted
     between the first two, the lender adds 50,001 (custody number 1)
  3. the seller reports 60,000 of completed work
  4. collection (Registry authorize_funding_wallet + Swig SignV2 + ClaimsCore
     collect_financier) sent and paid for by a key that is none of the parties:
       payee swapped to the sender's own token account                 refused
       an extra instruction after SignV2                               refused
       unchanged                                                       accepted
  5. the lender asks for one unit more than its free cash             refused
     the seller surrenders the order's unused 40,000 (seller's book
     number 1)                                                        accepted
  6. the buyer repays order 1 (route number 1, 30,000)                accepted
     the same repayment rebuilt against the new step count, number 1  refused (NumberUsed)
     the buyer's last 30,000 (number 2) with order 1's closes
     appended: the run, then its repayment record, then the Tab with
     its budget hold; every deposit back to its payer                 accepted
  7. owner-pinned terms, on a second agent (order 2, 50,000, fully earned):
       the buyer's owner creates agent 2's permission already pinned to
       size = 10 and unit_price = 1 per completed-run unit of the order's
       service (PrincipalRegistry create_mandate with its rules)        accepted
       order 2 says size = 11, through accept_pinned_financed_tab       refused
       the same size = 11 order through plain accept_financed_tab       refused
       order 2 at 2 per unit, listing its own unit_price               refused
       order 2 says size = 10 at 1 per unit, through the pinned call    accepted
       the seller reports all 50,000 as completed work                  accepted
       a key with no role collects the 50,000 for the seller            accepted
       the buyer repays 50,000; then that key, as a keeper, sends
       order 2's closes alone and pays only the fee                     accepted
  8. a delivery rule, on cluster time (order 3, 2,000 at 1 per unit):
       a rule in the delivery-rules program: the carrier alone signs for
       delivery and nobody may object; signing closes 60 s after an order's
       work deadline and objections 60 s later; a silent carrier supports
       nothing (delivery_rules create_rule)                             accepted
       the buyer's owner gives agent 2 a second permission, pinned to
       delivery_signer = the rule's delivery-source address             accepted
       agent 2's order names that address as its delivery signer and
       shows its list (accept_program_sourced_financed_tab)             accepted
       the payer opens the order's case                                 accepted
       the seller attests 1,400 units; it is not on the rule            refused
       the carrier attests 900 units                                    accepted
       a key with no role settles while objections are open             refused
       once they close on cluster time the same key settles: the rules
       program reports the carrier's 900 into ClaimsCore by CPI         accepted
       the seller claims 1,400; anyone finalizes: bill 900, 1,100
       released; the key with no role collects the 900 for the seller
       and closes the case, whose rent returns to the payer             accepted
       the buyer repays 900 with order 3's closes appended, its bill
       among them                                                       accepted
  9. the credit issuer repays the lender's facility 110,900 (issuer's
     book number 1): the lender is whole                              accepted

The refusals come before the acceptance they are compared with because they
are the same order: once it is accepted its Tab exists and it cannot be
offered again. The money refusal needs free cash below the order and the
permission refusal needs free cash above it (ClaimsCore checks cash before it
asks the Registry), so the lender adds cash between the two. Order 2 is 50,000
so that the 90,000 the lender holds after order 1 covers it and both of its
refusals reach the Registry's terms check.

Order 3's windows follow its work deadline, which the stage sets 60 s after
the cluster time at which it first builds the order and keeps in the journal
(once the order is on chain, its commit event is the record). The order admits
the rule's report and the seller's claim for a day after the rule's decision
deadline; the program requires at least an hour. The stage waits on the
cluster clock, never the local one, and takes a few minutes. A resume after
order 3's signing or objection window has closed cannot complete it: those
steps say so and send nothing.

An order's terms are (name, value) fingerprint pairs, each shown with no
number, and its `external_contract_hash` is the hash of that list; the helpers
and the shared vectors are those of pinned_terms.py. A pin is a list rule of
one value in the permission's rules. The reserved term `unit_price` carries
sha256("DEXTER:UNIT-PRICE:1" || namespace || unit || u64le(price)) of the
order's own price (pinned_price.py); every order 2 lists it.

Every refusal is simulated, then sent with preflight skipped so the failed
transaction is recorded on chain, then read back and compared with the error
named here in advance. Every successful transaction's ClaimsCore events are
read back and replayed: each Tab's step count and fingerprint and each book's
sequence, head and positions must equal the accounts the programs wrote.

Nothing here selects an endpoint. `--rpc-url`, `--cluster`, `--payer` and
`--salt-file` are required; the genesis hash must agree with `--cluster`; a
mainnet genesis hash is refused outright; the deployed programs are hashed and
compared with the build the IDLs came from before anything is sent. That build
must be of this checkout's program source: the three program files under
`--artifact-root` and the deployed programs are compared with the hashes in
`PINNED_BUILD` (the binaries of dexter-account branch
feat/dex-program-addresses-20261119: Change 5's programs at their DEX addresses), and
the Swig program with the devnet dump's hash (on `--cluster local`, also the
repository's retained Swig ELF, which the local validator profile loads). The
delivery-rules program must equal this build's delivery_rules.so, be owned by
the upgradeable loader, and either have no upgrade authority or share
ClaimsCore's: ClaimsCore accepts an order naming it only then.

  <python> public_rpc_sequence.py --rpc-url <url> --cluster <local|devnet|testnet> \\
      --payer <keypair.json> --salt-file <run.salt.json> --new \\
      --artifact-root <checkout holding .local/anchor/{idl,deploy}> [--swig-binary <swig.so>]

Add `--check-only` to read the cluster, the programs (including the
delivery-rules program's bytes and upgrade authority) and the payer and send
nothing. `--new` creates the salt file (mode 600); every key and identifier of
the run is derived from it, and it records the cluster, the payer and the
cluster time the run's windows start from. The same command without `--new`
resumes: a case the journal records as landed is not sent again, and if the
journal is lost the run is rebuilt from the chain. A step already on chain is
never rebuilt, because the accounts it wrote may since have closed. The journal
is `<salt-file>.journal.json`; the result is `<salt-file>.record.json` unless
`--out` names another path. `--stop-after <case>` ends the invocation after
that case lands, as an interruption would.
"""
from __future__ import annotations

import argparse
import base64
import contextlib
import datetime
import hashlib
import json
import os
import secrets
import struct
import sys
import time
from pathlib import Path
from urllib.parse import urlparse

from solders.instruction import Instruction
from solders.keypair import Keypair
from solders.pubkey import Pubkey
from solders.system_program import TransferParams, transfer

import delivery_rules as dr
import evidence_billing as eb
import financing_commercial as fc
import financing_fixture as ff
import pinned_delivery as pd
import pinned_terms as pt
import pinned_price as pp
from financing_commercial import CommercialFixture
from financing_fixture import YEAR
from financing_support import (CORE, POOL, PROGRAMS, REGISTRY, ROOT, SWIG, SYSTEM, SYSVAR_INSTRUCTIONS, TOKEN, Idl, M,
    check_receipt, digest, json_value, pda, repayment_record_address)
from public_rpc_transport import (GENESIS, PROGRAM_NAMES, UPGRADEABLE_LOADER, ChainAccounts, Journal, Rpc, RpcHarness,
    RpcLedger, StepFailed, classify, is_loopback, load_keypair, program_binary, refusal_problems)
from svm_common import ED25519, ed25519_ix
import tab_closes

ORDER = 100_000
PER_ORDER_LIMIT = 100_000
BUDGET = 500_000
CREDIT_LIMIT = 500_000
LENDER_START = ORDER - 1
LENDER_TOP_UP = 50_001
LENDER_CASH = LENDER_START + LENDER_TOP_UP
EARNED = 60_000
FIRST_PAYMENT = 30_000          # order 1's first repayment; the second settles it
MINTED = 1_000_000
SIGNATURE_FEE = 5_000
PINNED_ORDER = 50_000
PINNED_EARNED = PINNED_ORDER
PINNED_UNIT_PRICE = 1
REFUSED_UNIT_PRICE = 2
PERMISSION_BYTES = 1_282
RULE_ORDER = 2_000             # order 3's maximum, at 1 per unit
RULE_SIGNED = 900              # units the carrier attests
RULE_CLAIMED = 1_400           # the seller's claim; also the units it attests, refused
RULE_SIGN_SECONDS = 60         # signing closes this long after order 3's work deadline
RULE_OBJECT_SECONDS = 60       # objections close this long after signing closes
RULE_LEAD_SECONDS = 60         # order 3's work deadline, after the cluster time it is first built at
RULE_SETTLE_SECONDS = 86_400   # reports and claims are admitted this long after the rule's decision deadline
WINDOW_MARGIN_SECONDS = 20     # a refusal that needs an open window is sent only with this much of it left
PAID = EARNED + PINNED_EARNED + RULE_SIGNED
# Every chosen-amount money move names its number. Each window has one writer,
# and the run allocates each number once, so a resumed invocation rebuilds the
# same numbers without a counter.
NUMBER = dict(top_up=1, over_ask=2, surrender=1, order_1_first=1, order_1_last=2, order_2=1, order_3=1, facility=1)
# dexter-account branch feat/dex-program-addresses-20261119: Change 5, each Tab its own receipt book, built
# at the four DEX program addresses. Swig as dumped from devnet; the delivery-rules program is this build's.
PINNED_BUILD = dict(
    principal_registry="64bbaea177e9a982f483e78b28877bb03ee94daaac1dc374c899e4b78105505e",
    claims_core="6144105bd973f843080e9eb15013cc8ab20069608e35ea46627e9ccdfcb98e0a",
    pool_issuer="97d95b58df5c6fc60f92adfb1ca6a0e31be119f50d3a5b2564b50d6a05473119",
    swig="0418f037fe1c9f6c0cd9d3ee57d684f69aca2ed9b3388f5d0c45020b01cce1d9",
    delivery_rules=dr.RETAINED_SHA256)
# The Swig ELF this repository retains and the local validator profile loads; accepted on --cluster local only.
SWIG_RETAINED = ROOT / "foundation/swig/runtime/artifacts/swig-retained.so"
SWIG_RETAINED_SHA256 = "bb744d9811c392e3f343952d1a99bef9c97aa1936248a23b9d7efc1adb8939c1"
PROGRAM_NAMES.update({str(dr.RULES): "delivery_rules", str(ED25519): "ed25519"})
ROLE = dict(company="buyer", issuer="credit issuer", financier="lender", supplier="seller")
SALT_SCHEMA = "dexter-public-rpc-run-salt/v1"


class Run:
    """Everything a run names is derived from one random salt.

    The salt file also fixes what the salt was created for: the cluster, the
    payer, and the cluster time every permission and order window starts from.
    With those, a run can be rebuilt from the chain even if its journal is lost.
    """
    def __init__(self, value):
        assert value["schema"] == SALT_SCHEMA
        self.value, self.salt = value, bytes.fromhex(value["salt_hex"])
        assert len(self.salt) == 32

    def key(self, label):
        return Keypair.from_seed(hashlib.sha256(b"DEXTER:PUBLIC-RUN-KEY:1" + self.salt + label.encode()).digest())

    def ident(self, label):
        return hashlib.sha256(b"DEXTER:PUBLIC-RUN-ID:1" + self.salt + label.encode()).digest()

    @classmethod
    def create(cls, path, **bound):
        path = Path(path)
        if path.exists():
            raise SystemExit(f"{path} already exists. Drop --new to resume that run, or name a new salt file.")
        descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, "w") as handle:
            handle.write(json.dumps(dict(schema=SALT_SCHEMA, salt_hex=secrets.token_hex(32), created_at=utc(),
                **bound), indent=1) + "\n")
            handle.flush()
            os.fsync(handle.fileno())

    @classmethod
    def load(cls, path):
        path = Path(path)
        if not path.exists():
            raise SystemExit(f"{path} does not exist. Pass --new to start a run; without it an existing run is resumed.")
        assert path.stat().st_mode & 0o077 == 0, "salt file must not be readable by group or others"
        return cls(json.loads(path.read_text()))


class Interrupted(Exception):
    """`--stop-after` reached: the invocation ends as an interruption would."""


def utc():
    return datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def variant(tab):
    """Independent copy of one prepared order; signer objects are shared."""
    terms = dict(tab["terms"])
    args = dict(tab["args"], terms=terms, seller_fee_terms=dict(tab["args"]["seller_fee_terms"]))
    return dict(tab, terms=terms, args=args, accounts=dict(tab["accounts"]), signers=list(tab["signers"]))


def window_state(window, number):
    """Where `number` stands in an OrderWindow: bit i of the map stands for `highest - i`."""
    highest = window["highest"]
    if number > highest:
        return "open"
    distance = highest - number
    if distance >= 1024:
        return "below"
    return "used" if bytes(window["bits"])[distance // 8] & (1 << (distance % 8)) else "open"


class PublicSequence(CommercialFixture):
    """The suites' builders and checks over `RpcHarness`.

    Replaced here, and only here: fixed keys and identifiers (salted), airdrops
    (one payer funds what a step needs), the fixed clock and 600-second windows
    (cluster clock, configurable window), and parties this order does not use.
    """
    ROLES = ("company", "issuer", "financier", "supplier")
    # Evidence orders and delivery rules: the suites' own builders and readers.
    policy = eb.EvidenceBillingFixture.policy
    evidence_tab = eb.EvidenceBillingFixture.evidence_tab
    monetary_claim = eb.EvidenceBillingFixture.monetary_claim
    bill_state = eb.EvidenceBillingFixture.bill_state
    finalization = eb.EvidenceBillingFixture.finalization
    finalized_event = eb.EvidenceBillingFixture.finalized_event
    record_finalization_receipts = eb.EvidenceBillingFixture.record_finalization_receipts
    accepted_terms_digest = eb.EvidenceBillingFixture.accepted_terms_digest
    read_rules = dr.DeliveryRules.read_rules

    def __init__(self, harness, run, idls, *, t0, window, bundle_seconds):
        # The parent constructor builds a LiteSVM, loads programs into it and
        # airdrops to fixed keys; none of that applies to a validator.
        self.h, self.run, self.idls = harness, run, idls
        self.registry, self.core, self.pool = [idls[name] for name in PROGRAMS]
        self.rules = idls["delivery_rules"]
        self.t0, self.window, self.bundle_seconds, self.now = t0, window, bundle_seconds, t0
        self.domain = ff.ident("domain")
        self.parties, self.protected, self.receipts, self.tabs = {}, set(), [], {}
        # Change 5: one order-number allocator per mandate (names reuse their number).
        self.orders, self.next_order, self.numbers = {}, {}, {}
        for name in self.ROLES:
            owner = run.key("owner-" + name)
            principal_id = ff.ident("principal-" + name)
            self.parties[name] = dict(name=name, owner=owner, principal_id=principal_id, generation=1,
                principal=pda(REGISTRY, b"principal", owner.pubkey(), principal_id))
        # The inherited collection and repayment builders look these entries up before they know the order
        # carries no seller fee and no protocol interest share. They are never registered or sent.
        self.parties["fee"] = dict(name="fee")
        self.parties["protocol"] = dict(name="protocol", custody=None, custody_token=None)
        self.fee_agreements = dict(main=dict(address=None, agreement_hash=ff.ident("no-seller-fee"), cap=0))
        self.agent, self.agent2 = run.key("agent-1"), run.key("agent-2")
        self.collector = run.key("collector")
        self.carrier = run.key("carrier")
        # evidence_billing.py's policy builder starts from its delivery signer, `meter`; order 3 overrides it.
        self.meter = self.carrier
        # The inherited acceptance builder's agent and permission, for an order built inside evidence_tab.
        self.order_options = {}
        self.mint_key = run.key("mint")
        self.mint = self.mint_key.pubkey()
        self.funder = self.h.ata(self.h.payer.pubkey(), self.mint)
        self.asset = dict(mint=self.mint, token_program=TOKEN)
        self.company = self.parties["company"]["principal"]
        self.budget = pda(REGISTRY, b"budget", self.company, self.mint)
        self.registry_authority = pda(CORE, b"registry-consumer")
        self.actors = [pda(REGISTRY, b"actor", self.company, ff.ident("actor-" + str(n))) for n in (1, 2)]
        self.mandates = []
        for name in self.ROLES:
            party = self.parties[name]
            party["book"] = pda(CORE, b"financial-book", party["principal"], self.mint)
            party["projection"] = pda(POOL, b"financial-projection", party["book"])
            party["owner_ata"] = self.h.ata(party["owner"].pubkey(), self.mint)
        self.collector_ata = self.h.ata(self.collector.pubkey(), self.mint)
        self.closes_done = []

    def observe(self, *addresses):
        """The in-process suites snapshot every observed account around each
        transaction. Over RPC each case reads the figures it asserts on."""

    # -- sending -----------------------------------------------------------
    def send(self, label, instruction, signers=(), *, reject=False, error=None, receipt=None, effect=None, witness=None):
        """Send one case. `instruction` is an instruction, a list of them, or a
        callable returning dict(instructions, signers[, receipt]) that is called
        only when the case must be sent.

        A refusal names `witness`, an account its transaction names, whose
        history will hold it. A sent step states `effect`, how its result is
        found on chain (an account that exists, or a callable).
        """
        assert error is None, "name the expected refusal with expect_refusal"
        label = self.h.titles.get(label, label)
        built = {}

        def build():
            if callable(instruction):
                request = instruction()
                built.update(request)
                items, keys = request["instructions"], request.get("signers", ())
            else:
                items, keys = instruction, signers
            items = list(items) if isinstance(items, (list, tuple)) else [items]
            # Same rule as the parent: owner fields accept native signatures or
            # passkey proofs, so the supplied native signers are marked explicitly.
            supplied = {key.pubkey() for key in keys}
            items = [Instruction(item.program_id, bytes(item.data),
                [M(meta.pubkey, meta.is_signer or meta.pubkey in supplied, meta.is_writable) for meta in item.accounts])
                for item in items]
            return items, list(keys)
        if reject:
            witness = witness if witness is not None else self.h.witnesses.get(label)
            assert witness is not None, ("a refusal names an account whose history will hold it", label)
            self.h.witnesses[label] = witness
        else:
            if effect is not None:
                self.h.effects[label] = effect
            assert label in self.h.effects, ("a sent step states its on-chain effect", label)
        row = self.h.send(label, build, (), fail="any" if reject else None)
        receipt = built.get("receipt", receipt)
        if receipt is not None and not reject and not self.h.replaying:
            data = check_receipt(self.core, self.h.svm, receipt, self.h.ledger)
            for change in data["effects"]:
                book = self.core.read(self.h.svm, change["book"], "FinanceBook")["data"]
                self.h.check(label + ": book frontier", book["sequence"], change["sequence"])
                self.h.check(label + ": book amounts", book["positions"], change["after"])
                self.h.check(label + ": book hash", book["head"], change["hash"])
            self.receipts.append(dict(address=str(receipt), event=data))
        return row

    def expect_refusal(self, label, *, program, name, index=0):
        """The exact failure a case must land with; the code comes from the program's IDL."""
        idl = next(idl for idl in self.idls.values() if idl.program == program)
        code = next(entry["code"] for entry in idl.value["errors"] if entry["name"] == name)
        self.h.refusals[label] = dict(program=program, name=name, code=code, index=index)

    @contextlib.contextmanager
    def paid_by(self, signer):
        previous, self.h.payer = self.h.payer, signer
        try:
            yield
        finally:
            self.h.payer = previous

    @contextlib.contextmanager
    def phase(self, name):
        previous, self.h.phase = self.h.phase, name
        try:
            yield
        finally:
            self.h.phase = previous

    # -- finding a step's effect on chain (used only to rebuild a lost journal) --
    def window_used(self, kind, address, number):
        """Whether `number` is used in the window of the record at `address`."""
        reader = self.registry if kind == "Mandate" else self.core
        if self.h.svm.get_account(address, fresh=True) is None:
            return False
        state = reader.read(self.h.svm, address, kind)
        window = state["window"]
        return window_state(window, number) == "used"

    def order_used(self, tab):
        return self.window_used("Mandate", tab["mandate"], tab["order"])

    def event_found(self, address, name, match=lambda value: True):
        """A successful transaction naming `address` carries a ClaimsCore event `name` for which `match` holds."""
        return self.h.landed_with(address, events=lambda events: any(event == name and match(value)
            for event, value in events))

    def instruction_found(self, address, instruction):
        return self.h.landed_with(address, instruction=instruction)

    # -- terms -------------------------------------------------------------
    def interest_terms(self, **_):
        return dict(version=1, annual_rate_bps=0, seconds_per_year=YEAR, maximum_interest=0,
            accrual_cutoff=self.t0 + 2 * YEAR, protocol_share_bps=0, protocol_beneficiary=Pubkey.default(),
            protocol_destination=Pubkey.default(), repayment_priority="InterestFirst", stop_on_shortfall=False)

    def seller_fees(self, cap, *, enabled=True, aggregate_cap=2_000, agreement="main"):
        assert not enabled, "this sequence carries no seller fee"
        return dict(version=1, agreement_hash=self.fee_agreements[agreement]["agreement_hash"],
            supplier=self.parties["supplier"]["owner"].pubkey(), asset=self.asset, gross_cap=cap,
            rate_bps=0, fixed_fee=0, aggregate_fee_cap=0, maximum_deduction_bps=10_000,
            beneficiary=Pubkey.default(), beneficiary_wallet=Pubkey.default(), destination=Pubkey.default(),
            failure_policy="WaiveUnavailable")

    def acceptance(self, name, **options):
        tab = super().acceptance(name, **{**self.order_options, **options})
        tab["terms"]["expires_at"] = self.t0 + self.window
        return tab

    def wallet_bundle(self, inner, **options):
        self.now = self.h.svm.clock()
        options["expires_at"] = self.now + self.bundle_seconds
        return super().wallet_bundle(inner, **options)

    # -- setup -------------------------------------------------------------
    def setup_principals(self):
        for role in self.ROLES:
            party, who = self.parties[role], ROLE[role]
            self.send(f"register the {who} as a principal",
                self.ix("principal_registry", "create_principal",
                    dict(owner=party["owner"].pubkey(), principal=party["principal"]),
                    args=dict(principal_id=party["principal_id"], domain=self.domain, kind=1,
                        governing_policy=ff.ident("single-owner-ed25519-profile"))), [party["owner"]],
                effect=party["principal"])
            self.send(f"open the {who}'s financial book",
                self.ix("claims_core", "initialize_finance_book", dict(book=party["book"]), principal=party["principal"]),
                effect=party["book"])
            self.send(f"open the {who}'s Pool projection",
                self.ix("pool_issuer", "initialize_financial_projection",
                    dict(book=party["book"], projection=party["projection"])), effect=party["projection"])
            self.send(f"create the {who} owner's token account",
                self.h.ata_ix(party["owner"].pubkey(), self.mint), effect=party["owner_ata"])
        buyer = self.parties["company"]
        self.send(f"buyer's owner sets the budget to {BUDGET:,}", self.ix("principal_registry", "create_budget",
            dict(owner=buyer["owner"].pubkey(), principal=self.company, budget=self.budget),
            args=dict(asset=self.mint, ceiling=BUDGET)), [buyer["owner"]], effect=self.budget)
        self.send("buyer's owner enrolls agent 1", self.ix("principal_registry", "create_actor",
            dict(owner=buyer["owner"].pubkey(), principal=self.company, actor=self.actors[0]),
            args=dict(actor_id=ff.ident("actor-1"), current_key=self.agent.pubkey(),
                valid_from=self.t0 - 100, valid_until=self.t0 + self.window + 86_400)), [buyer["owner"]],
            effect=self.actors[0])

    def custody_addresses(self, role, *, wallet):
        """Where the inherited `create_custody` will put things, to state each step's effect first."""
        party = self.parties[role]
        if wallet:
            config = pda(SWIG, b"swig", ff.ident("funding-wallet-" + role))
            authority = pda(SWIG, b"swig-wallet-address", config)
            binding = pda(REGISTRY, b"funding-wallet", party["principal"], config)
        else:
            config = binding = CORE
            authority = pda(CORE, b"escrow-authority", party["principal"], self.mint)
        token = self.h.ata(authority, self.mint)
        return dict(config=config, authority=authority, binding=binding, token=token,
            custody=pda(CORE, b"custody-reserve", token))

    def create_custody(self, role, amount=0, *, wallet=False, name=None):
        where, who = self.custody_addresses(role, wallet=wallet), ROLE[role]
        titles, effects = self.h.titles, self.h.effects
        if wallet:
            titles["create and bind exact governed financier wallet"] = \
                f"create the {who}'s Swig wallet and bind it to the {who}'s principal"
            effects[titles["create and bind exact governed financier wallet"]] = where["binding"]
        titles[f"create {role} custody token account"] = f"create the {who} wallet's token account"
        effects[titles[f"create {role} custody token account"]] = where["token"]
        if amount:
            titles[f"prefund {role} token account before explicit allocation"] = \
                f"put {amount:,} tokens in the {who}'s wallet"
            effects[titles[f"prefund {role} token account before explicit allocation"]] = \
                lambda: self.h.token_credit_found(where["token"], amount)
        title = f"{who}'s owner opens the cash record for its wallet ({amount:,} allocated)"
        titles[f"owner allocates {role} capital without inventing token inflow"] = title
        effects[title] = where["custody"]
        super().create_custody(role, amount, wallet=wallet, name=name)

    def open_facility(self):
        lender, issuer = self.parties["financier"], self.parties["issuer"]
        facility_id = ff.ident("facility")
        facility = pda(CORE, b"funding-facility", lender["principal"], facility_id)
        authority = pda(CORE, b"facility-authority", facility)
        self.send(f"lender's owner and credit issuer's owner open the lender's facility (limit {CREDIT_LIMIT:,})",
            self.ix("claims_core", "open_funding_facility",
                dict(owner=lender["owner"].pubkey(), financier=lender["principal"],
                    issuer_owner=issuer["owner"].pubkey(), issuer=issuer["principal"], custody=lender["custody"],
                    facility=facility, facility_authority=authority, protocol_custody=None),
                facility_id=facility_id, commitment_limit=CREDIT_LIMIT, accept_until=self.t0 + self.window + 86_400,
                external=True, terms_hash=ff.ident("facility-contract"), interest_terms=self.interest_terms(),
                financier_generation=lender["generation"], issuer_generation=issuer["generation"]),
            [lender["owner"], issuer["owner"]], effect=facility)
        return facility

    def permit(self):
        buyer = self.parties["company"]
        mandate_id = ff.ident("mandate-agent-1")
        mandate = pda(REGISTRY, b"mandate", self.company, mandate_id)
        self.send(f"buyer's owner permits agent 1: this seller, this lender facility, per-order limit {PER_ORDER_LIMIT:,}",
            self.ix("principal_registry", "create_mandate",
                dict(owner=buyer["owner"].pubkey(), principal=self.company, actor=self.actors[0],
                    budget=self.budget, mandate=mandate),
                args=dict(mandate_id=mandate_id, source=self.facility, per_operation_limit=PER_ORDER_LIMIT,
                    expires_at=self.t0 + self.window),
                rules=pt.rules_of([self.parties["supplier"]["owner"].pubkey()], [])), [buyer["owner"]], effect=mandate)
        return mandate

    # -- owner-pinned terms ------------------------------------------------
    def enroll_second_agent(self):
        buyer = self.parties["company"]
        self.send("buyer's owner enrolls agent 2", self.ix("principal_registry", "create_actor",
            dict(owner=buyer["owner"].pubkey(), principal=self.company, actor=self.actors[1]),
            args=dict(actor_id=ff.ident("actor-2"), current_key=self.agent2.pubkey(),
                valid_from=self.t0 - 100, valid_until=self.t0 + self.window + 86_400)), [buyer["owner"]],
            effect=self.actors[1])

    def pinned_mandate_address(self, name="mandate-agent-2"):
        return pda(REGISTRY, b"mandate", self.company, ff.ident(name))

    def permit_pinned(self, label, required, name="mandate-agent-2"):
        """create_mandate with its rules: a permission of agent 2 and its pinned terms in one instruction."""
        buyer = self.parties["company"]
        mandate_id = ff.ident(name)
        mandate = self.pinned_mandate_address(name)
        self.send(label, self.ix("principal_registry", "create_mandate",
            dict(owner=buyer["owner"].pubkey(), principal=self.company, actor=self.actors[1],
                budget=self.budget, mandate=mandate),
            args=dict(mandate_id=mandate_id, source=self.facility, per_operation_limit=PER_ORDER_LIMIT,
                expires_at=self.t0 + self.window),
            rules=pt.rules_of([self.parties["supplier"]["owner"].pubkey()], required)), [buyer["owner"]], effect=mandate)
        return mandate

    def pinned_permission(self, mandate):
        """The permission's pins: each a list rule of one value, read back as (name, value); its one seller."""
        state = self.registry.read(self.h.svm, mandate, "Mandate")
        count = state["rule_count"]
        rules = state["rules"][:count]
        assert all(rule["kind"] == "List" and rule["value_count"] == 1 for rule in rules), "a pin is a list of one"
        assert state["sellers"][:state["seller_count"]] == [self.parties["supplier"]["owner"].pubkey()], "one seller"
        return dict(revision=state["revision"], active=state["active"],
            pinned=[dict(name=rule["name"], value=rule["values"][0]) for rule in rules],
            unused_slots_zero=all(item == pt.UNUSED_RULE for item in state["rules"][count:])
                and all(item == Pubkey.default() for item in state["sellers"][state["seller_count"]:]))

    def with_terms(self, tab, pairs, *, price=PINNED_UNIT_PRICE):
        """The order at `price` per unit; its list is `pairs` and its own unit_price term, ascending.

        The signed terms hash and the shown list are built by pinned_terms.py,
        the unit_price term by pinned_price.py.
        """
        tab["unit_price"] = tab["args"]["unit_price"] = price
        listed = pp.ascending(*(pt.term(name, value) for name, value in pairs),
            pp.unit_price_term(tab["namespace"], tab["service_unit"], price))
        return pt.PinnedTerms.set_terms(self, tab, listed)

    def pinned_instruction(self, tab):
        return self.ix("claims_core", "accept_pinned_financed_tab", tab["accounts"],
            remaining=self.remaining(["company", "issuer", "supplier", tab["financier"]]),
            order_terms=pt.shown_terms(tab["order_terms"]), **tab["args"])

    # -- orders: build-only requests ---------------------------------------
    def commit(self, label, tab, *, pinned=False, reject=False):
        """A commit at the Tab's order number; its effect is that number used in the mandate's window."""
        build = lambda: dict(instructions=self.pinned_instruction(tab) if pinned else self.accept_instruction(tab),
            signers=tab["signers"], receipt=tab["receipt"])
        self.send(label, build, reject=reject, witness=tab["claim"] if reject else None,
            effect=None if reject else (lambda: self.order_used(tab)))

    def earn_step(self, label, tab, name, units):
        """The seller's earn_financed_run, built only when sent. Returns the run's address."""
        run = ff.run_address(tab["claim"], ff.ident("completed-run-" + name))
        def build():
            request = self.build_earn(tab, name, units)
            assert request["run"] == run
            return dict(instructions=request["instruction"], signers=request["signers"], receipt=request["receipt"])
        self.send(label, build, effect=lambda: self.event_found(run, "RunEarned", lambda value: value["detail"]["run"] == run))
        return run

    def collect_step(self, label, tab, run, amount, *, reject=False, tamper=None):
        """A collection of `amount` earned on `run`, wrapped in the lender wallet's Swig envelope, from the
        current fee payer. `tamper(request)` returns the instructions to send instead."""
        def build():
            request = self.collection(tab, dict(run=run), amount, paid_before=0)
            if tamper is not None:
                return dict(instructions=tamper(request), signers=request["signers"])
            instructions = self.wallet_bundle(request["instruction"], action=1, caller=self.h.payer, generation=0,
                role=request["financier"])
            return dict(instructions=instructions, signers=request["signers"], receipt=request["receipt"])
        self.send(label, build, reject=reject, witness=run if reject else None,
            effect=None if reject else (lambda: self.event_found(run, "Collected",
                lambda value: value["detail"]["run"] == run and value["detail"]["amount"] == amount)))

    def repay_step(self, label, tab, run, amount, number, *, reject=False, closes=None):
        """The buyer's repayment of `run` at route `number`; with `closes` (runs, records), the order's closes
        are appended to it. Its effect: a Repaid event with that number on the route's record."""
        record = repayment_record_address(run, self.parties["company"]["owner_ata"], self.parties["issuer"]["principal"])
        def build():
            request = self.repayment(tab, dict(run=run), "repay-" + tab["name"] + "-" + str(number), amount,
                creditor="issuer", maximum_interest=0, number=number)
            assert request["record"] == record
            instructions = [request["instruction"]]
            if closes is not None:
                instructions += self.closes_for(tab, *closes)
            return dict(instructions=instructions, signers=request["signers"],
                receipt=None if reject else request["receipt"])
        self.send(label, build, reject=reject, witness=record if reject else None,
            effect=None if reject else (lambda: self.event_found(record, "Repaid",
                lambda value: value["repayment_record"] == record and value["number"] == number)))
        return record

    def closes_for(self, tab, runs, records):
        """A settled Tab's closes in the order the programs require: each run, then each repayment record
        (whose close waits for its run), then the Tab with its budget hold and, for an evidence order, its bill
        and binding address. A record the settling payment creates in the same transaction is paid for by its
        fee payer."""
        instructions = [tab_closes.close_run_instruction(self, tab, run)[0] for run in runs]
        for run, record in records:
            payer = self.state("RepaymentRecord", record)["deposit_payer"] \
                if self.h.svm.get_account(record) is not None else self.h.payer.pubkey()
            instructions.append(self.ix("claims_core", "close_repayment_record",
                dict(claim=tab["claim"], run=run, record=record, record_payer=payer)))
        instructions.append(tab_closes.close_tab_instruction(self, tab)[0])
        return instructions

    def open_to_close(self, label, tab):
        """Whether this invocation will send the closes of `tab`: not already in the journal, and the Tab still
        open (a rebuilt journal finds a closed Tab's closes on chain instead)."""
        return not self.h.journal.landed(label) and self.h.svm.get_account(tab["claim"], fresh=True) is not None

    def closing(self, tab, runs, records):
        """What each close will return, read before the closes: every record's deposit and its recorded
        payer. A record created by the same transaction returns its deposit to the payer that just funded it."""
        records_by_kind = {}
        claim = self.state("FundedClaim", tab["claim"])
        records_by_kind["tab"] = (tab["claim"], claim["deposit_payer"])
        hold = self.registry.read(self.h.svm, tab["reservation"], "Reservation")
        records_by_kind["hold"] = (tab["reservation"], hold["deposit_payer"])
        for index, run in enumerate(runs, 1):
            records_by_kind[f"run {index}"] = (run, self.state("FundedRun", run)["deposit_payer"])
        for index, (_, record) in enumerate(records, 1):
            if self.h.svm.get_account(record) is not None:
                records_by_kind[f"repayment record {index}"] = (record, self.state("RepaymentRecord", record)["deposit_payer"])
        if "bill" in tab:
            records_by_kind["bill"] = (tab["bill"], self.bill_state(tab)["deposit_payer"])
        deposits = {kind: self.h.svm.get_account(address).lamports for kind, (address, _) in records_by_kind.items()}
        mandate = self.registry.read(self.h.svm, tab["mandate"], "Mandate")
        return dict(records=records_by_kind, deposits=deposits, mandate=mandate, steps=claim["steps"],
            fingerprint=claim["fingerprint"])

    def check_closes(self, label, tab, before):
        """Every record is gone; each deposit reached its recorded payer, read from the transaction's own
        balances; the closer paid only its fee unless it was also a payer; TabClosed ends the Tab's history with
        its step count and fingerprint, names the hold and bill with their payers and deposits, and the mandate's
        closed-Tab chain folds it in."""
        row = self.h.cases[-1]
        records, deposits = before["records"], before["deposits"]
        for kind, (address, _) in records.items():
            self.h.check(f"{label}: closed {kind} is gone", self.h.svm.get_account(address, fresh=True) is None, True)
        refunds = {}
        for kind, (_, payer) in records.items():
            refunds[str(payer)] = refunds.get(str(payer), 0) + deposits[kind]
        fee_payer = row["fee_payer"]
        for payer, amount in sorted(refunds.items()):
            expected = amount - (row["fee_lamports"] if payer == fee_payer else 0)
            self.h.check(f"{label}: deposits return to the recorded payer {payer}",
                row["lamport_deltas"].get(payer, 0), expected)
        if fee_payer not in refunds:
            self.h.check(f"{label}: the closer pays only its fee", row["lamport_deltas"].get(fee_payer, 0),
                -row["fee_lamports"])
        closed = [value for case, event, value in self.h.ledger.events if case == label and event == "TabClosed"]
        self.h.check(f"{label}: one TabClosed ends the Tab's history", len(closed), 1)
        event = closed[0]
        self.h.check(f"{label}: TabClosed carries the Tab's step count and fingerprint before the close",
            (event["header"]["tab"], event["tab_payer"], event["tab_lamports"]),
            (tab["claim"], records["tab"][1], deposits["tab"]))
        none = (Pubkey.default(), Pubkey.default(), 0)
        for kind in ("hold", "bill", "binding"):
            expected = (records[kind][0], records[kind][1], deposits[kind]) if kind in records else none
            self.h.check(f"{label}: TabClosed reports the {kind}, its payer and its deposit",
                (event[kind], event[kind + "_payer"], event[kind + "_lamports"]), expected)
        mandate = self.registry.read(self.h.svm, tab["mandate"], "Mandate")
        link = hashlib.sha256(b"DEXTER:CLOSED-TAB:1" + bytes(before["mandate"]["closed_tabs_head"]) + bytes(tab["claim"])
            + struct.pack("<I", event["steps"]) + bytes(event["prior_fingerprint"])).digest()
        self.h.check(f"{label}: the mandate counts the closed Tab", mandate["closed_tabs"],
            before["mandate"]["closed_tabs"] + 1)
        self.h.check(f"{label}: the mandate's closed-Tab chain folds in the Tab's final fingerprint",
            mandate["closed_tabs_head"], link)
        self.h.check(f"{label}: the Tab's order number stays used", self.order_used(tab), True)
        self.closes_done.append(dict(case=label, order=tab["order"], tab=str(tab["claim"]), fee_payer=fee_payer,
            fee_lamports=row["fee_lamports"], closed={kind: str(address) for kind, (address, _) in records.items()},
            deposits=deposits, refunds_by_payer=refunds, steps=event["steps"],
            final_fingerprint=bytes(event["prior_fingerprint"]).hex()))

    # -- delivery rule -----------------------------------------------------
    def rule_terms(self):
        """The carrier alone signs for delivery; nobody may object, so nobody decides; silence pays nothing."""
        return dict(dr.rule_terms([self.carrier.pubkey()], 1, decide=0),
            sign_seconds=RULE_SIGN_SECONDS, object_seconds=RULE_OBJECT_SECONDS)

    def rule_order(self, rule, work_until):
        """Order 3: an evidence order under agent 2's rule permission whose delivery signer is the rule's
        delivery-source address. Its list is the seller's terms and delivery_signer; its policy commits to it."""
        source = dr.delivery_source(dr.RULES, bytes(rule))
        listed = pd.delivery_list(pt.SELLER_TERMS, source)
        settle_by = work_until + RULE_SIGN_SECONDS + RULE_OBJECT_SECONDS + RULE_SETTLE_SECONDS
        self.order_options = dict(index=1, mandate=self.rule_permission)
        try:
            tab = self.evidence_tab("rule-order", cap=RULE_ORDER, rates=((1, 1, RULE_ORDER),), policy_overrides=dict(
                source=source, source_identity=bytes(rule), request_hash=pt.order_terms_hash(listed),
                work_until=work_until, evidence_deadline=settle_by, claim_deadline=settle_by))
        finally:
            self.order_options = {}
        return dict(tab, rule=rule, source=source, order_terms=listed, case=dr.case_address(tab["bill"]))

    def committed_work_until(self, tab):
        """Order 3's work deadline as its commit recorded it: the Tab's terms expire at the work deadline,
        and TabCommitted carries them. Read from the commit's event, so it survives the Tab's close."""
        found = []
        def match(events):
            for event, value in events:
                if event == "TabCommitted" and value["header"]["tab"] == tab["claim"]:
                    found.append(value["tab_state"]["terms"]["expires_at"])
                    return True
            return False
        return found[0] if self.h.landed_with(tab["claim"], events=match) else None

    def rule_order_at(self, rule, label):
        """Order 3 with its work deadline: RULE_LEAD_SECONDS after the cluster time it is first built at, kept in
        the journal. Once the order is on chain its commit is the record; an order never sent is rebuilt when its
        deadline is too close to land."""
        header = self.h.journal.value["header"]
        work_until = header.get("rule_order_work_until") or self.h.svm.clock() + RULE_LEAD_SECONDS
        tab = self.rule_order(rule, work_until)
        if self.order_used(tab):
            if self.h.svm.get_account(tab["bill"]) is not None:
                work_until = self.bill_state(tab)["policy"]["work_until"]
            elif header.get("rule_order_work_until") is None:
                work_until = self.committed_work_until(tab)
                assert work_until is not None, "order 3's number is used but its commit was not found"
        elif self.h.journal.step(label) is None and work_until - self.h.svm.clock() < RULE_LEAD_SECONDS // 2:
            work_until = self.h.svm.clock() + RULE_LEAD_SECONDS
        if work_until != tab["billing_policy"]["work_until"]:
            tab = self.rule_order(rule, work_until)
        if header.get("rule_order_work_until") != work_until:
            header["rule_order_work_until"] = work_until
            self.h.journal.save()
        return tab

    def wait_for(self, moment, what):
        """Until cluster time passes `moment`."""
        while (now := self.h.svm.clock()) <= moment:
            self.h.log(f"   cluster time {now}: {moment - now + 1} s until {what}")
            time.sleep(min(moment - now + 1, 15))
        return now

    def window_left(self, label, closes):
        """A refusal that needs an open window, sent with less than WINDOW_MARGIN_SECONDS of it left, could land
        after it closes and succeed. Such a case is not sent."""
        if self.h.journal.step(label) is None and not self.h.recovering:
            left = closes - self.h.svm.clock()
            if left < WINDOW_MARGIN_SECONDS:
                raise StepFailed(f"{label}: {left} s of the window are left, fewer than {WINDOW_MARGIN_SECONDS}; "
                    "not sent. This run cannot record the case; start a new run.")

    def setup(self):
        lender, buyer, issuer = self.parties["financier"], self.parties["company"], self.parties["issuer"]
        title = "initialize synthetic SPL token and disposable freeze authority"
        self.h.titles[title] = f"create the synthetic six-decimal token and mint {MINTED:,} to the payer"
        self.h.effects[self.h.titles[title]] = self.mint
        self.h.make_mint(self.mint_key, initial=MINTED)
        self.setup_principals()
        self.create_custody("issuer", 0)
        self.create_custody("financier", LENDER_START, wallet=True)
        self.facility = self.open_facility()
        line_title = f"buyer's owner and credit issuer's owner open the buyer's credit line (limit {CREDIT_LIMIT:,})"
        self.h.titles["owner and issuer approve main customer credit line"] = line_title
        self.h.effects[line_title] = pda(CORE, b"credit-line", self.company, ff.ident("credit-line-main"))
        self.line, self.assignment_policy = self.open_line(self.facility)
        self.permission = self.permit()
        for party, amount, label in (
                (lender, LENDER_TOP_UP, f"give the lender's owner {LENDER_TOP_UP:,} tokens to add later"),
                (buyer, PAID, f"give the buyer's owner {PAID:,} tokens to repay its three orders"),
                (issuer, PAID, f"give the credit issuer's owner {PAID:,} tokens to repay the lender's facility")):
            self.h.effects[label] = (lambda account=party["owner_ata"], amount=amount:
                self.h.token_credit_found(account, amount))
            self.transfer_tokens(label, party["owner_ata"], amount)

    # -- figures -----------------------------------------------------------
    def lender_cash(self):
        lender = self.parties["financier"]
        custody = self.state("FinancierCustody", lender["custody"])
        promised = custody["unused"] + custody["payable"]
        return dict(wallet=self.h.amount(lender["custody_token"]), accounted=custody["accounted_cash"],
            promised=promised, free=custody["accounted_cash"] - promised)

    def figures(self, tab=None):
        """The numbers every case is judged on, read from the chain."""
        lender, seller, buyer, issuer = [self.parties[role] for role in ("financier", "supplier", "company", "issuer")]
        cash = self.lender_cash()
        budget = self.registry.read(self.h.svm, self.budget, "Budget")
        out = dict(lender_wallet_tokens=cash["wallet"], lender_cash_on_record=cash["accounted"],
            lender_promised=cash["promised"], lender_free_cash=cash["free"],
            lender_owner_tokens=self.h.amount(lender["owner_ata"]), seller_tokens=self.h.amount(seller["owner_ata"]),
            buyer_tokens=self.h.amount(buyer["owner_ata"]), issuer_owner_tokens=self.h.amount(issuer["owner_ata"]),
            issuer_cash_tokens=self.h.amount(issuer["custody_token"]),
            collector_tokens=self.h.amount(self.collector_ata),
            budget_held=budget["reserved"], budget_used=budget["consumed"], order=None)
        if tab is not None and self.h.svm.get_account(tab["claim"]) is not None:
            claim = self.state("FundedClaim", tab["claim"])
            out["order"] = {field: claim[field] for field in ("remaining", "payable", "paid", "debt", "repaid", "released")}
        if tab is not None and "bill" in tab and self.h.svm.get_account(tab["bill"]) is not None:
            bill = self.bill_state(tab)
            out["bill"] = {field: bill[field] for field in ("source_report", "supported_amount", "seller_claim",
                "finalized", "established_amount", "released_amount")}
        if tab is not None and "case" in tab and self.h.svm.get_account(tab["case"]) is not None:
            out["case"] = self.read_rules("Case", tab["case"])
        return out

    @contextlib.contextmanager
    def case(self, label, tab, *, refuse=None, expected=None):
        """Read the figures around one case; a refusal must leave every one unchanged."""
        if refuse is not None:
            self.expect_refusal(label, **refuse)
        if self.h.journal.landed(label):
            yield
            return
        before = self.figures(tab)
        yield
        if self.h.replaying:
            return
        after = self.figures(tab)
        self.h.annotate(label, figures_before=before, figures_after=after)
        if refuse is not None:
            self.h.check(label + ": every figure read is unchanged", after, before)
        for field, value in (expected or {}).items():
            actual = after["order"][field[6:]] if field.startswith("order_") else after[field]
            self.h.check(f"{label}: {field}", actual, value)

    # -- operations --------------------------------------------------------
    def add_cash(self, label, name, amount, number):
        lender = self.parties["financier"]
        def build():
            request = self.capital_request("contribute-" + name, amount, number=number)
            return dict(instructions=request["instruction"], signers=request["signers"], receipt=request["receipt"])
        self.send(label, build, effect=lambda: self.window_used("FinancierCustody", lender["custody"], number))

    def fund(self, label, recipient, lamports, *, done):
        """One system transfer from the payer for a key that must hold lamports."""
        self.send(label, transfer(TransferParams(from_pubkey=self.h.payer.pubkey(), to_pubkey=recipient,
            lamports=lamports)), effect=lambda: self.h.svm.balance(recipient) >= lamports or done())

    def sweep(self, label, signer, *, after):
        """Return every lamport a funded key still holds. The payer pays the fee."""
        if not self.h.journal.landed(after):
            return
        lamports = 0 if self.h.journal.landed(label) else self.h.svm.balance(signer.pubkey())
        self.send(label, transfer(TransferParams(from_pubkey=signer.pubkey(), to_pubkey=self.h.payer.pubkey(),
            lamports=lamports)), [signer], effect=lambda: lamports == 0 or self.h.svm.balance(signer.pubkey()) == 0)


def sequence(f):
    buyer, issuer, lender, seller = [f.parties[role] for role in ("company", "issuer", "financier", "supplier")]
    core = lambda name, index=0: dict(program=CORE, name=name, index=index)
    registry = lambda name, index=0: dict(program=REGISTRY, name=name, index=index)
    usual_payer = f.h.payer
    floor = f.h.svm.minimum_balance_for_rent_exemption(0)

    with f.phase("setup"):
        f.setup()
    order = f.acceptance("order-1", cap=ORDER, maximum_interest=0, fees=False, mandate=f.permission)

    with f.phase("commit"):
        f.h.check("order 1 is the first number of agent 1's mandate", order["order"], 1)
        label = f"commit refused (money): order of {ORDER:,} when the lender's free cash is {LENDER_START:,}"
        with f.case(label, order, refuse=core("FundingCashShortfall")):
            f.commit(label, order, reject=True)

        label = f"lender adds {LENDER_TOP_UP:,} to its wallet (custody number {NUMBER['top_up']}): free cash becomes {LENDER_CASH:,}"
        with f.case(label, order, expected=dict(lender_free_cash=LENDER_CASH, lender_wallet_tokens=LENDER_CASH,
                lender_owner_tokens=0)):
            f.add_cash(label, "top-up", LENDER_TOP_UP, NUMBER["top_up"])

        changed = variant(order)
        changed["terms"]["cap"] = ORDER + 1
        changed["args"]["seller_fee_terms"]["gross_cap"] = ORDER + 1
        label = f"commit refused (permission): order of {ORDER + 1:,} when the agent's per-order limit is {PER_ORDER_LIMIT:,}"
        with f.case(label, order, refuse=registry("PerOperationLimit")):
            f.commit(label, changed, reject=True)

        changed = variant(order)
        changed["args"]["unit_price"] = 2
        label = "commit refused (deal): order sent at unit price 2 when the agreed terms commit to unit price 1"
        with f.case(label, order, refuse=core("FundingUsage")):
            f.commit(label, changed, reject=True)

        label = (f"COMMIT ACCEPTED: order 1 of {ORDER:,} at agent 1's order number 1, inside the agent's "
            f"{PER_ORDER_LIMIT:,} limit and the {BUDGET:,} budget, lender free cash {LENDER_CASH:,}, seller and "
            "credit issuer's owner signed")
        with f.case(label, order, expected=dict(budget_held=ORDER, budget_used=0, lender_promised=ORDER,
                lender_free_cash=LENDER_CASH - ORDER, lender_wallet_tokens=LENDER_CASH,
                order_remaining=ORDER, order_payable=0, order_paid=0, order_debt=0)):
            f.commit(label, order)
        if not f.h.replaying:
            row = f.h.cases[-1]
            f.h.check("the commit is one top-level instruction", row["top_level_instructions"], ["claims_core"])
            f.h.check("ClaimsCore calls the Registry inside the commit",
                f"Program {REGISTRY} invoke [2]" in row["logs"], True)
            f.h.check("commit signers are the fee payer, the agent, the credit issuer's owner and the seller",
                sorted(row["signer_keys"]),
                sorted(str(k.pubkey()) for k in (usual_payer, f.agent, issuer["owner"], seller["owner"])))
            reservation = f.registry.read(f.h.svm, order["reservation"], "Reservation")
            f.h.check("the order's budget hold holds the order maximum", (reservation["remaining"], reservation["order"]),
                (ORDER, order["order"]))
            f.h.check("the Tab lives at its mandate and order number and keeps them in its terms",
                (f.state("FundedClaim", order["claim"])["terms"]["order"], order["claim"]),
                (order["order"], ff.tab_address(order["mandate"], order["order"])))
            f.h.check("agent 1's mandate window has order number 1 used", f.order_used(order), True)
            f.h.check("the commit wrote no receipt account", f.h.svm.get_account(order["receipt"], fresh=True), None)

    with f.phase("earn"):
        label = f"seller reports {EARNED:,} of completed work on the order"
        with f.case(label, order, expected=dict(order_remaining=ORDER - EARNED, order_payable=EARNED,
                order_debt=EARNED, order_paid=0, budget_used=EARNED, seller_tokens=0)):
            run = f.earn_step(label, order, "order-1-work", EARNED)

    with f.phase("collection"):
        accepted = f"COLLECTION ACCEPTED: a key with no role in the order pays the seller's earned {EARNED:,} from the lender's wallet"
        # Fees only: Change 5 writes no receipt. Whatever is left is returned later.
        f.fund("payer gives the collecting key lamports for its fees", f.collector.pubkey(),
            floor + 40 * (SIGNATURE_FEE + f.h.priority_fee),
            done=lambda: f.event_found(run, "Collected", lambda value: value["detail"]["run"] == run))
        f.send("create the collecting key's own token account", f.h.ata_ix(f.collector.pubkey(), f.mint),
            effect=f.collector_ata)
        with f.paid_by(f.collector):
            label = f"collection refused: payee swapped to the collecting key's own token account"
            with f.case(label, order, refuse=core("FundingTokenRouting", 1)):
                def swapped(request):
                    changed = dict(request, accounts=dict(request["accounts"], destination=f.collector_ata))
                    instruction = f.ix("claims_core", "collect_financier", changed["accounts"],
                        remaining=f.remaining(request["roles"]), **request["args"])
                    return f.wallet_bundle(instruction, action=1, caller=f.collector, generation=0)
                f.collect_step(label, order, run, EARNED, reject=True, tamper=swapped)

            label = "collection refused: an extra instruction follows the wallet's SignV2"
            with f.case(label, order, refuse=registry("WalletEnvelope", 0)):
                def extra(request):
                    envelope = f.wallet_bundle(request["instruction"], action=1, caller=f.collector, generation=0)
                    return [*envelope, Instruction(SYSTEM, struct.pack("<IQ", 2, 1),
                        [M(f.collector.pubkey(), True, True), M(usual_payer.pubkey(), w=True)])]
                f.collect_step(label, order, run, EARNED, reject=True, tamper=extra)

            with f.case(accepted, order, expected=dict(seller_tokens=EARNED, lender_wallet_tokens=LENDER_CASH - EARNED,
                    collector_tokens=0, lender_promised=ORDER - EARNED, lender_free_cash=LENDER_CASH - ORDER,
                    order_paid=EARNED, order_payable=0, order_remaining=ORDER - EARNED, order_debt=EARNED)):
                f.collect_step(accepted, order, run, EARNED)
            if not f.h.replaying:
                row = f.h.cases[-1]
                f.h.check("collection carries exactly one signature", row["signature_count"], 1)
                f.h.check("the only signer and fee payer is the collecting key", row["signer_keys"],
                    [str(f.collector.pubkey())])
                for name, signer in (("buyer's owner", buyer["owner"]), ("agent", f.agent),
                        ("lender's owner", lender["owner"]), ("credit issuer's owner", issuer["owner"]),
                        ("seller", seller["owner"]), ("payer of every other transaction", usual_payer)):
                    f.h.check(name + " is not among the collection's account keys",
                        str(signer.pubkey()) in row["account_keys"], False)
                f.h.check("collection is Registry authorization then Swig SignV2",
                    row["top_level_instructions"], ["principal_registry", "swig"])

    with f.phase("withdraw and release"):
        refused = (f"withdrawal refused: lender asks for {LENDER_CASH - ORDER + 1:,} when "
            f"{LENDER_CASH - ORDER:,} is free and {ORDER - EARNED:,} is promised (custody number {NUMBER['over_ask']})")
        with f.case(refused, order, refuse=core("FundingCashShortfall", 1)):
            def over_ask():
                request = f.withdrawal("free-plus-one", LENDER_CASH - ORDER + 1, number=NUMBER["over_ask"])
                instructions = f.wallet_bundle(request["instruction"], action=2, caller=request["owner"],
                    generation=request["generation"], role=request["role"])
                return dict(instructions=instructions, signers=request["signers"])
            f.send(refused, over_ask, reject=True, witness=lender["custody"])

        label = (f"RELEASE ACCEPTED: the seller surrenders the order's unused {ORDER - EARNED:,} "
            f"(seller's book number {NUMBER['surrender']})")
        with f.case(label, order, expected=dict(lender_promised=0, lender_free_cash=LENDER_CASH - EARNED,
                lender_wallet_tokens=LENDER_CASH - EARNED, budget_held=0, budget_used=EARNED,
                order_remaining=0, order_released=ORDER - EARNED, order_debt=EARNED, order_paid=EARNED)):
            def release():
                request = f.build_release(order, "order-1-unused", ORDER - EARNED, number=NUMBER["surrender"])
                return dict(instructions=request["instruction"], signers=request["signers"], receipt=request["receipt"])
            f.send(label, release, effect=lambda: f.window_used("FinanceBook", seller["book"], NUMBER["surrender"]))
        if not f.h.replaying:
            f.h.check("the surrender's number is used in the seller's book window",
                f.window_used("FinanceBook", seller["book"], NUMBER["surrender"]), True)

    with f.phase("order 1 repaid and closed"):
        label = f"buyer repays {FIRST_PAYMENT:,} of order 1 (route number {NUMBER['order_1_first']})"
        with f.case(label, order, expected=dict(order_debt=EARNED - FIRST_PAYMENT, order_repaid=FIRST_PAYMENT,
                buyer_tokens=PAID - FIRST_PAYMENT, issuer_cash_tokens=FIRST_PAYMENT)):
            record = f.repay_step(label, order, run, FIRST_PAYMENT, NUMBER["order_1_first"])
        if not f.h.replaying:
            f.h.check("the payment route's record has number 1 used",
                f.window_used("RepaymentRecord", record, NUMBER["order_1_first"]), True)

        label = (f"repayment refused (number used): the same {FIRST_PAYMENT:,} rebuilt against order 1's new step "
            f"count, with its landed route number {NUMBER['order_1_first']}")
        with f.case(label, order, refuse=core("NumberUsed")):
            f.repay_step(label, order, run, FIRST_PAYMENT, NUMBER["order_1_first"], reject=True)

        label = (f"ORDER 1 SETTLED AND CLOSED: the buyer repays its last {EARNED - FIRST_PAYMENT:,} (route number "
            f"{NUMBER['order_1_last']}); the run, its repayment record and the Tab with its budget hold close in the "
            "same transaction")
        before = f.closing(order, [run], [(run, record)]) if f.open_to_close(label, order) else None
        with f.case(label, order, expected=dict(buyer_tokens=PAID - EARNED, issuer_cash_tokens=EARNED, budget_held=0,
                budget_used=EARNED)):
            f.repay_step(label, order, run, EARNED - FIRST_PAYMENT, NUMBER["order_1_last"],
                closes=([run], [(run, record)]))
        if not f.h.replaying:
            f.check_closes(label, order, before)

    with f.phase("owner-pinned terms"):
        size_10 = pt.term("size", "10")
        order2 = f.acceptance("order-2", cap=PINNED_ORDER, maximum_interest=0, fees=False, index=1,
            mandate=f.pinned_mandate_address())
        price_1 = pp.unit_price_term(order2["namespace"], order2["service_unit"], PINNED_UNIT_PRICE)
        f.pin2 = pin = pp.ascending(size_10, price_1)
        f.enroll_second_agent()
        label = ("PERMISSION CREATED PINNED: buyer's owner permits agent 2 with size = 10 and a unit price of "
            f"{PINNED_UNIT_PRICE} already pinned, in one instruction (create_mandate): this seller, this lender "
            f"facility, per-order limit {PER_ORDER_LIMIT:,}")
        f.permission2 = f.permit_pinned(label, pin)
        if not f.h.replaying:
            row = f.h.cases[-1]
            f.h.check(f"agent 2's permission is at revision 1, active, pinning size = 10 and unit_price = {PINNED_UNIT_PRICE}",
                f.pinned_permission(f.permission2), dict(revision=1, active=True, pinned=pin, unused_slots_zero=True))
            f.h.check(f"the permission record is {PERMISSION_BYTES} bytes",
                len(f.h.svm.get_account(f.permission2).data), PERMISSION_BYTES)
            f.h.check("the pinned permission is created by one Registry instruction",
                row["top_level_instructions"], ["principal_registry"])
            f.h.check("the Registry ran CreateMandate",
                "Program log: Instruction: CreateMandate" in row["logs"], True)
            f.h.check("the pinned permission is signed by the fee payer and the buyer's owner only",
                sorted(row["signer_keys"]), sorted(str(k.pubkey()) for k in (usual_payer, buyer["owner"])))

        order2 = f.with_terms(order2, pt.SELLER_TERMS)
        f.order2 = order2
        size_11 = f.with_terms(variant(order2), pt.changed(pt.SELLER_TERMS, size="11"))
        price_2 = f.with_terms(variant(order2), pt.SELLER_TERMS, price=REFUSED_UNIT_PRICE)
        f.order2_variants = {"size = 10 at 1 per unit": order2, "size = 11 at 1 per unit": size_11,
            "size = 10 at 2 per unit": price_2}
        own_price_2 = pp.unit_price_term(order2["namespace"], order2["service_unit"], REFUSED_UNIT_PRICE)
        f.h.check("order 2's list carries size = 10 and its own unit_price of 1; the size = 11 variant differs in size only; "
            "the price variant is at 2 per unit and lists unit_price = 2; all three hash differently",
            (size_10 in order2["order_terms"], price_1 in order2["order_terms"], pt.term("size", "11") in size_11["order_terms"],
                price_1 in size_11["order_terms"], (price_2["unit_price"], own_price_2 in price_2["order_terms"],
                    size_10 in price_2["order_terms"], price_1 in price_2["order_terms"]),
                len({bytes(t["terms"]["external_terms_hash"]) for t in (order2, size_11, price_2)})),
            (True, True, True, True, (REFUSED_UNIT_PRICE, True, True, False), 3))

        label = (f"pinned order refused (terms): agent 2's order of {PINNED_ORDER:,} says size = 11 where the owner "
            "pinned size = 10, sent through accept_pinned_financed_tab")
        with f.case(label, order2, refuse=registry("TermsNotPermitted")):
            f.commit(label, size_11, pinned=True, reject=True)

        label = ("pinned order refused (no terms shown): the same size = 11 order sent through plain accept_financed_tab, "
            "which shows no terms")
        with f.case(label, order2, refuse=registry("TermsRequired")):
            f.commit(label, size_11, reject=True)

        label = (f"pinned order refused (price): agent 2's order of {PINNED_ORDER:,} with size = 10 at "
            f"{REFUSED_UNIT_PRICE} per unit, listing its own unit_price, where the owner pinned {PINNED_UNIT_PRICE} per unit")
        with f.case(label, order2, refuse=registry("TermsNotPermitted")):
            f.commit(label, price_2, pinned=True, reject=True)

        label = (f"PINNED ORDER ACCEPTED: agent 2's order of {PINNED_ORDER:,} says size = 10 at {PINNED_UNIT_PRICE} per unit, "
            "the pinned values, sent through accept_pinned_financed_tab")
        with f.case(label, order2, expected=dict(budget_held=PINNED_ORDER, budget_used=EARNED, lender_promised=PINNED_ORDER,
                lender_free_cash=LENDER_CASH - EARNED - PINNED_ORDER, lender_wallet_tokens=LENDER_CASH - EARNED,
                order_remaining=PINNED_ORDER, order_payable=0, order_paid=0, order_debt=0)):
            f.commit(label, order2, pinned=True)
        if not f.h.replaying:
            row = f.h.cases[-1]
            f.h.check("the pinned order is one top-level instruction", row["top_level_instructions"], ["claims_core"])
            f.h.check("ClaimsCore ran AcceptPinnedFinancedTab",
                "Program log: Instruction: AcceptPinnedFinancedTab" in row["logs"], True)
            f.h.check("the Registry ran ReservePinned inside the order",
                "Program log: Instruction: ReservePinned" in row["logs"], True)
            f.h.check("pinned order signers are the fee payer, agent 2, the credit issuer's owner and the seller",
                sorted(row["signer_keys"]),
                sorted(str(k.pubkey()) for k in (usual_payer, f.agent2, issuer["owner"], seller["owner"])))
            claim = f.state("FundedClaim", order2["claim"])
            f.h.check("order 2's record stores the terms hash agent 2 and the seller signed",
                claim["external_contract_hash"], pt.order_terms_hash(order2["order_terms"]))
            reservation = f.registry.read(f.h.svm, order2["reservation"], "Reservation")
            f.h.check("the budget hold commits to the same terms",
                reservation["terms_digest"], f.accepted_terms_digest(order2))
            f.h.check(f"order 2's record prices {PINNED_UNIT_PRICE} per unit of the pinned service",
                (claim["unit_price"], claim["usage_namespace"], claim["service_unit"]),
                (PINNED_UNIT_PRICE, order2["namespace"], order2["service_unit"]))

        label = f"seller reports all {PINNED_EARNED:,} of order 2 as completed work"
        with f.case(label, order2, expected=dict(order_remaining=0, order_payable=PINNED_EARNED, order_debt=PINNED_EARNED,
                order_paid=0, budget_held=0, budget_used=EARNED + PINNED_EARNED, seller_tokens=EARNED)):
            run2 = f.earn_step(label, order2, "order-2-work", PINNED_EARNED)

        pinned_collected = (f"PINNED ORDER COLLECTED: a key with no role in the order pays the seller's earned "
            f"{PINNED_EARNED:,} on order 2 from the lender's wallet")
        with f.paid_by(f.collector):
            with f.case(pinned_collected, order2, expected=dict(seller_tokens=EARNED + PINNED_EARNED,
                    lender_wallet_tokens=LENDER_CASH - EARNED - PINNED_EARNED, lender_promised=0,
                    lender_free_cash=LENDER_CASH - EARNED - PINNED_EARNED, collector_tokens=0,
                    order_paid=PINNED_EARNED, order_payable=0, order_remaining=0, order_debt=PINNED_EARNED)):
                f.collect_step(pinned_collected, order2, run2, PINNED_EARNED)
            if not f.h.replaying:
                row = f.h.cases[-1]
                f.h.check("order 2's collection carries exactly one signature", row["signature_count"], 1)
                f.h.check("the only signer and fee payer of order 2's collection is the collecting key",
                    row["signer_keys"], [str(f.collector.pubkey())])
                for name, signer in (("buyer's owner", buyer["owner"]), ("agent 2", f.agent2),
                        ("lender's owner", lender["owner"]), ("credit issuer's owner", issuer["owner"]),
                        ("seller", seller["owner"]), ("payer of every other transaction", usual_payer)):
                    f.h.check(name + " is not among order 2's collection's account keys",
                        str(signer.pubkey()) in row["account_keys"], False)
                f.h.check("order 2's collection is Registry authorization then Swig SignV2",
                    row["top_level_instructions"], ["principal_registry", "swig"])

    with f.phase("order 2 repaid and closed"):
        label = f"buyer repays all {PINNED_EARNED:,} of order 2 (route number {NUMBER['order_2']})"
        with f.case(label, order2, expected=dict(order_debt=0, order_repaid=PINNED_EARNED,
                buyer_tokens=PAID - EARNED - PINNED_EARNED, issuer_cash_tokens=EARNED + PINNED_EARNED)):
            record2 = f.repay_step(label, order2, run2, PINNED_EARNED, NUMBER["order_2"])
        label = closed_2 = ("ORDER 2 CLOSED BY A KEEPER: the collecting key, which has no role in the order, sends "
            "order 2's closes alone; every deposit goes to the account that paid it and the keeper pays only the fee")
        before = f.closing(order2, [run2], [(run2, record2)]) if f.open_to_close(label, order2) else None
        with f.paid_by(f.collector):
            with f.case(label, order2, expected=dict(budget_held=0, budget_used=EARNED + PINNED_EARNED)):
                f.send(label, lambda: dict(instructions=f.closes_for(order2, [run2], [(run2, record2)])),
                    effect=lambda: f.h.svm.get_account(order2["claim"], fresh=True) is None and f.order_used(order2))
            if not f.h.replaying:
                f.check_closes(label, order2, before)

    with f.phase("return lamports"):
        f.sweep("collecting key returns its remaining lamports to the payer", f.collector, after=closed_2)
    return order, run


def rule_stage(f):
    """Phase 8: order 3's delivery is settled under a delivery rule, on cluster time, then repaid and closed."""
    seller, issuer = f.parties["supplier"], f.parties["issuer"]
    usual_payer = f.h.payer
    rules = lambda name: dict(program=dr.RULES, name=name)
    paid_before = EARNED + PINNED_EARNED
    free = LENDER_CASH - paid_before
    terms = f.rule_terms()
    rule = dr.rule_address(terms)
    source = dr.delivery_source(dr.RULES, bytes(rule))

    with f.phase("delivery rule"):
        label = (f"RULE CREATED: the carrier alone signs for delivery and nobody may object; signing closes {RULE_SIGN_SECONDS} s "
            f"after an order's work deadline and objections {RULE_OBJECT_SECONDS} s later; a silent carrier supports nothing")
        f.send(label, f.ix("delivery_rules", "create_rule", dict(rule=rule), terms=terms), effect=rule)
        if not f.h.replaying:
            row = f.h.cases[-1]
            f.h.check("the rule's terms encode as the built interface does", f.rules.encode({"defined": {"name": "RuleTerms"}},
                terms), dr.rule_bytes(terms))
            state = f.read_rules("Rule", rule)
            f.h.check("the rule holds its terms at the address of their hash, and the payer that funded it",
                (state["terms"], state["deposit_payer"]), (terms, usual_payer.pubkey()))
            account = f.h.svm.get_account(rule)
            f.h.check("the rule account holds exactly its rent", account.lamports,
                f.h.svm.minimum_balance_for_rent_exemption(len(account.data)))
            f.h.check("the rule is created by one delivery-rules instruction signed by the fee payer only",
                (row["top_level_instructions"], row["signer_keys"]), (["delivery_rules"], [str(usual_payer.pubkey())]))
        pin = [pd.signer_term(source)]
        label = ("PERMISSION CREATED PINNED (delivery rule): buyer's owner gives agent 2 a second permission, pinned to "
            "delivery_signer = the rule's delivery-source address, in one instruction (create_mandate): this seller, "
            f"this lender facility, per-order limit {PER_ORDER_LIMIT:,}")
        f.rule_permission = f.permit_pinned(label, pin, name="mandate-agent-2-rule")
        if not f.h.replaying:
            f.h.check("agent 2's rule permission is at revision 1, active, pinning delivery_signer = the rule's "
                "delivery-source address", f.pinned_permission(f.rule_permission),
                dict(revision=1, active=True, pinned=pin, unused_slots_zero=True))
            f.h.check("the rule's delivery-source address is a program address, not a signing key", source.is_on_curve(), False)

    with f.phase("rule order"):
        label = (f"RULE ORDER ACCEPTED: agent 2's order 3 of {RULE_ORDER:,} at 1 per unit names the rule's delivery-source "
            "address as its delivery signer, the pinned value, and shows its list (accept_program_sourced_financed_tab)")
        tab = f.rule_tab = f.rule_order_at(rule, label)
        policy = tab["billing_policy"]
        signing_closes = policy["work_until"] + RULE_SIGN_SECONDS
        objections_close = signing_closes + RULE_OBJECT_SECONDS
        decision_closes = objections_close + terms["decide_seconds"]
        with f.case(label, tab, expected=dict(budget_held=RULE_ORDER, budget_used=paid_before, lender_promised=RULE_ORDER,
                lender_free_cash=free - RULE_ORDER, lender_wallet_tokens=free,
                order_remaining=RULE_ORDER, order_payable=0, order_paid=0, order_debt=0)):
            f.send(label, lambda: dict(instructions=f.ix("claims_core", "accept_program_sourced_financed_tab",
                dict(tab["evidence_accounts"], rule_program=dr.RULES, rule_program_data=dr.program_data(dr.RULES),
                    core_program_data=dr.program_data(CORE)),
                remaining=f.remaining(["company", "issuer", "supplier", "financier"]), order_terms=pt.shown_terms(tab["order_terms"]),
                **tab["evidence_args"]), signers=tab["signers"], receipt=tab["receipt"]),
                effect=lambda: f.order_used(tab))
        if not f.h.replaying:
            row = f.h.cases[-1]
            f.h.check("order 3 is one top-level instruction", row["top_level_instructions"], ["claims_core"])
            f.h.check("ClaimsCore ran AcceptProgramSourcedFinancedTab and the Registry ReservePinnedBilling inside it",
                ("Program log: Instruction: AcceptProgramSourcedFinancedTab" in row["logs"],
                    "Program log: Instruction: ReservePinnedBilling" in row["logs"]), (True, True))
            f.h.check("order 3's signers are the fee payer, agent 2, the credit issuer's owner and the seller",
                sorted(row["signer_keys"]),
                sorted(str(k.pubkey()) for k in (usual_payer, f.agent2, issuer["owner"], seller["owner"])))
            bill = f.bill_state(tab)
            f.h.check("order 3's bill keeps the policy: the rule as source identity, its delivery-source address as "
                "delivery signer, the list's hash as request_hash", (bill["policy"], bill["policy_hash"]),
                (policy, tab["policy_hash"]))
            f.h.check("the list shown carries delivery_signer = the rule's delivery-source address",
                (pd.signer_term(source) in tab["order_terms"], policy["source"], bytes(policy["source_identity"]),
                    policy["request_hash"]), (True, source, bytes(rule), pt.order_terms_hash(tab["order_terms"])))
            f.h.check("order 3's record commits to the policy hash", f.state("FundedClaim", tab["claim"])["external_contract_hash"],
                tab["policy_hash"])
            f.h.check("order 3 admits the report and the claim a day after the rule's decision deadline; the program "
                "requires an hour", (policy["evidence_deadline"] - decision_closes, policy["claim_deadline"] - decision_closes,
                    policy["evidence_deadline"] - decision_closes >= dr.MIN_SETTLE), (RULE_SETTLE_SECONDS, RULE_SETTLE_SECONDS, True))
            f.h.check("order 3 was admitted before its work deadline", bill["admitted_at"] < policy["work_until"], True)
            f.h.check("order 3's bill records the commit's fee payer as the payer of its deposit", bill["deposit_payer"],
                usual_payer.pubkey())

    with f.phase("rule case"):
        label = "CASE OPENED: the payer opens order 3's case under the rule and pays its rent"
        with f.case(label, tab):
            f.send(label, f.ix("delivery_rules", "open_case", dict(rule=rule, bill=tab["bill"], case=tab["case"])),
                effect=lambda: f.instruction_found(tab["case"], "OpenCase"))
        if not f.h.replaying:
            state = f.read_rules("Case", tab["case"])
            f.h.check("the case records the bill, the rule, the opener and the deadlines that follow order 3's work deadline",
                {k: state[k] for k in ("bill", "rule", "opener", "signing_closes", "objections_close", "decision_closes")},
                dict(bill=tab["bill"], rule=rule, opener=usual_payer.pubkey(), signing_closes=signing_closes,
                    objections_close=objections_close, decision_closes=decision_closes))
            f.h.check("nothing is attested, objected, decided or settled",
                (state["attested"], state["objector"], state["decision"], state["outcome"]), (0, None, None, 0))
            account = f.h.svm.get_account(tab["case"])
            f.h.check("the case holds exactly its rent", account.lamports,
                f.h.svm.minimum_balance_for_rent_exemption(len(account.data)))

        attest = lambda signer, units: f.ix("delivery_rules", "attest", dict(signer=signer.pubkey(), case=tab["case"],
            rule=rule, bill=tab["bill"]), units=dr.units8([units]))
        label = f"attestation refused: the seller signs {RULE_CLAIMED:,} units on order 3's case; the rule names the carrier alone"
        with f.case(label, tab, refuse=rules("NotASigner")):
            f.send(label, attest(seller["owner"], RULE_CLAIMED), [seller["owner"]], reject=True, witness=tab["case"])

        label = f"CARRIER ATTESTS: the rule's carrier signs {RULE_SIGNED:,} units delivered on order 3, before signing closes"
        with f.case(label, tab):
            f.send(label, attest(f.carrier, RULE_SIGNED), [f.carrier], effect=lambda: f.instruction_found(tab["case"], "Attest"))
        if not f.h.replaying:
            row = f.h.cases[-1]
            state = f.read_rules("Case", tab["case"])
            f.h.check("the case keeps the carrier's units and marks it as attested", (state["attestations"][0], state["attested"]),
                (dr.units8([RULE_SIGNED]), 1))
            f.h.check("the attestation is signed by the fee payer and the carrier", sorted(row["signer_keys"]),
                sorted(str(k.pubkey()) for k in (usual_payer, f.carrier)))

        closed = "CASE CLOSED: the collecting key closes order 3's case; its rent returns to the payer, who opened it"
        f.fund("payer gives the collecting key lamports to settle, collect and close order 3's case: fees only",
            f.collector.pubkey(), f.h.svm.minimum_balance_for_rent_exemption(0) + 10 * (SIGNATURE_FEE + f.h.priority_fee),
            done=lambda: f.instruction_found(tab["case"], "Settle"))

        settle = lambda: dict(instructions=f.ix("delivery_rules", "settle", dict(case=tab["case"], rule=rule, bill=tab["bill"],
            delivery_source=source, delivery_rules=dr.RULES, claims_core=CORE, tab=tab["claim"],
            rule_program_data=dr.program_data(dr.RULES), core_program_data=dr.program_data(CORE))))
        label = "settlement refused: the collecting key, which has no role, settles order 3's case while the objection window is open"
        f.window_left(label, objections_close)
        with f.paid_by(f.collector):
            with f.case(label, tab, refuse=rules("ObjectionsOpen")):
                f.send(label, settle, reject=True, witness=tab["case"])

        f.wait_for(objections_close, "order 3's objection window closes")
        label = (f"CASE SETTLED: once objections close, the collecting key settles order 3; the rules program reports the "
            f"carrier's {RULE_SIGNED:,} units into ClaimsCore by CPI")
        with f.paid_by(f.collector):
            with f.case(label, tab, expected=dict(order_remaining=RULE_ORDER, order_payable=0, order_debt=0)):
                f.send(label, settle, effect=lambda: f.instruction_found(tab["case"], "Settle"))
        if not f.h.replaying:
            row = f.h.cases[-1]
            state = f.read_rules("Case", tab["case"])
            f.h.check("the case records the carrier's group and its units as the outcome",
                (state["outcome"], state["group"], state["units"]), (dr.SIGNERS, 0b1, dr.units8([RULE_SIGNED])))
            record = dr.case_bytes(state)
            f.h.check("the case record's bytes match the built interface", f.rules.encode({"defined": {"name": "Case"}}, state),
                record)
            logged = "Program data: " + " ".join(base64.b64encode(part).decode()
                for part in (dr.OUTCOME_DOMAIN, bytes(tab["case"]), record))
            f.h.check("the settle log carries the domain, the case address and the case record", logged in row["logs"], True)
            bill = f.bill_state(tab)
            report = dict(record_id=bytes(tab["case"]), request_hash=policy["request_hash"], started_at=bill["admitted_at"],
                completed_at=policy["work_until"], units=dr.units8([RULE_SIGNED]),
                result_hash=hashlib.sha256(dr.OUTCOME_DOMAIN + bytes(tab["case"]) + record).digest(), finality=1, outcome=1)
            f.h.check("the bill holds the rule's report; its result_hash is the hash of the logged case record",
                bill["source_report"], report)
            f.h.check("the report digest is the one ClaimsCore computes for any delivery signer", bill["source_digest"],
                eb.phase_digest(eb.REPORT_DOMAIN, f.domain, tab["bill"], tab["claim"], eb.report_bytes(report), tab["policy_hash"]))
            f.h.check(f"the report supports {RULE_SIGNED:,} and was admitted after objections closed",
                (bill["supported_amount"], bill["evidence_admitted_at"] > objections_close), (RULE_SIGNED, True))
            f.h.check("the rules program called ClaimsCore's submit_program_bill_evidence",
                (f"Program {CORE} invoke [2]" in row["logs"], "Program log: Instruction: SubmitProgramBillEvidence" in row["logs"]),
                (True, True))
            f.h.check("the settlement carries one signature, the collecting key's", (row["signature_count"], row["signer_keys"]),
                (1, [str(f.collector.pubkey())]))

    with f.phase("rule bill"):
        label = f"seller signs a claim of {RULE_CLAIMED:,} on order 3"
        claim = f.monetary_claim(tab, RULE_CLAIMED)
        message = eb.phase_digest(eb.CLAIM_DOMAIN, f.domain, tab["bill"], tab["claim"], eb.seller_claim_bytes(claim),
            tab["policy_hash"])
        with f.case(label, tab):
            f.send(label, [ed25519_ix(seller["owner"], message), f.ix("claims_core", "submit_bill_claim",
                dict(bill=tab["bill"], claim=tab["claim"], supplier_principal=tab["supplier_principal"],
                    instructions=SYSVAR_INSTRUCTIONS),
                seller_claim=claim, signature_ix_index=0)],
                effect=lambda: f.event_found(tab["bill"], "ClaimAdmitted", lambda value: value["bill"] == tab["bill"]))
        if not f.h.replaying:
            row = f.h.cases[-1]
            bill = f.bill_state(tab)
            f.h.check("the bill keeps the seller's claim and the digest its key signed", (bill["seller_claim"], bill["seller_digest"]),
                (claim, message))
            f.h.check("the claim's transaction is signed by the fee payer only; the seller's key signs inside the ed25519 "
                "instruction", (row["signer_keys"], row["top_level_instructions"]), ([str(usual_payer.pubkey())],
                    ["ed25519", "claims_core"]))

        label = (f"BILL FINALIZED: the rule reported {RULE_SIGNED:,} and the seller claimed {RULE_CLAIMED:,}; the bill is "
            f"{RULE_SIGNED:,} and the unused {RULE_ORDER - RULE_SIGNED:,} is released")
        with f.case(label, tab, expected=dict(budget_held=0, budget_used=paid_before + RULE_SIGNED, lender_promised=RULE_SIGNED,
                lender_free_cash=free - RULE_SIGNED, lender_wallet_tokens=free, order_remaining=0, order_payable=RULE_SIGNED,
                order_debt=RULE_SIGNED, order_paid=0, order_released=RULE_ORDER - RULE_SIGNED)):
            f.send(label, lambda: dict(instructions=f.finalization(tab)["instruction"]),
                effect=lambda: f.event_found(tab["bill"], "BillFinalized", lambda value: value["bill"] == tab["bill"]))
        if not f.h.replaying:
            f.record_finalization_receipts(tab)
            bill = f.bill_state(tab)
            f.h.check("the bill is the smallest of the maximum, the seller's claim and the rule's report",
                (bill["finalized"], bill["established_amount"], bill["released_amount"]),
                (True, min(RULE_ORDER, RULE_CLAIMED, RULE_SIGNED), RULE_ORDER - min(RULE_ORDER, RULE_CLAIMED, RULE_SIGNED)))
            f.h.check("the bill is final with both records in, before either deadline",
                bill["finalized_at"] <= min(policy["evidence_deadline"], policy["claim_deadline"]), True)

        run3 = tab["settlement"]["run"]
        label = (f"RULE ORDER COLLECTED: a key with no role in the order pays the seller's {RULE_SIGNED:,} on order 3 "
            "from the lender's wallet")
        with f.paid_by(f.collector):
            with f.case(label, tab, expected=dict(seller_tokens=paid_before + RULE_SIGNED, lender_wallet_tokens=free - RULE_SIGNED,
                    lender_promised=0, lender_free_cash=free - RULE_SIGNED, collector_tokens=0, order_paid=RULE_SIGNED,
                    order_payable=0, order_debt=RULE_SIGNED)):
                f.collect_step(label, tab, run3, RULE_SIGNED)
            if not f.h.replaying:
                row = f.h.cases[-1]
                f.h.check("order 3's collection carries one signature, the collecting key's",
                    (row["signature_count"], row["signer_keys"]), (1, [str(f.collector.pubkey())]))
                f.h.check("order 3's collection is Registry authorization then Swig SignV2", row["top_level_instructions"],
                    ["principal_registry", "swig"])

            case_account = None if f.h.journal.landed(closed) else f.h.svm.get_account(tab["case"], fresh=True)
            case_rent = None if case_account is None else case_account.lamports
            with f.case(closed, tab):
                f.send(closed, f.ix("delivery_rules", "close_case", dict(case=tab["case"], opener=usual_payer.pubkey(),
                    bill=tab["bill"])), effect=lambda: f.instruction_found(tab["case"], "CloseCase"))
            if not f.h.replaying:
                row = f.h.cases[-1]
                f.h.check("the case is closed and its rent returned to the payer, who opened it",
                    (f.h.svm.get_account(tab["case"], fresh=True), row["payer_lamports_delta"]), (None, case_rent))

    with f.phase("order 3 repaid and closed"):
        record3 = repayment_record_address(run3, f.parties["company"]["owner_ata"], issuer["principal"])
        label = settled_3 = (f"ORDER 3 SETTLED AND CLOSED: the buyer repays its {RULE_SIGNED:,} (route number "
            f"{NUMBER['order_3']}); the run, the new repayment record and the Tab with its budget hold and bill close in "
            "the same transaction")
        before = f.closing(tab, [run3], [(run3, record3)]) if f.open_to_close(label, tab) else None
        with f.case(label, tab, expected=dict(buyer_tokens=0, issuer_cash_tokens=PAID, budget_held=0, budget_used=PAID)):
            f.repay_step(label, tab, run3, RULE_SIGNED, NUMBER["order_3"], closes=([run3], [(run3, record3)]))
        if not f.h.replaying:
            f.check_closes(label, tab, before)

    with f.phase("facility repaid"):
        label = (f"FACILITY REPAID: the credit issuer repays the lender's facility {PAID:,} (issuer's book number "
            f"{NUMBER['facility']}); the lender's wallet is back to {LENDER_CASH:,}")
        with f.case(label, None, expected=dict(lender_wallet_tokens=LENDER_CASH, lender_cash_on_record=LENDER_CASH,
                issuer_owner_tokens=0)):
            def facility():
                request = f.facility_repayment("repay-facility", PAID, maximum_interest=0, number=NUMBER["facility"])
                return dict(instructions=request["instruction"], signers=request["signers"], receipt=request["receipt"])
            f.send(label, facility, effect=lambda: f.window_used("FinanceBook", issuer["book"], NUMBER["facility"]))

    with f.phase("return lamports"):
        f.sweep("collecting key returns its remaining lamports to the payer after order 3", f.collector, after=settled_3)
    return tab


def final_state(f, order):
    """Checked against the chain on every invocation, including a resumed one."""
    journal = f.h.journal.value
    journal["assertions"] = [row for row in journal["assertions"] if row["step"] != "final state"]
    f.h.replaying, f.h.current = False, "final state"
    figures = f.figures()
    expected = dict(lender_wallet_tokens=LENDER_CASH, lender_cash_on_record=LENDER_CASH,
        lender_promised=0, lender_free_cash=LENDER_CASH, lender_owner_tokens=0, seller_tokens=PAID,
        buyer_tokens=0, issuer_owner_tokens=0, issuer_cash_tokens=PAID, collector_tokens=0, budget_held=0, budget_used=PAID)
    for field, value in expected.items():
        f.h.check("final " + field, figures[field], value)
    for name, tab in (("order 1", order), ("order 2", f.order2), ("order 3", f.rule_tab)):
        accounts = [tab["claim"], tab["reservation"]] + ([tab["bill"], tab["case"]] if "bill" in tab else [])
        f.h.check(f"final: {name}'s Tab, budget hold" + (", bill and case" if "bill" in tab else "") + " are closed",
            [f.h.svm.get_account(address, fresh=True) for address in accounts], [None] * len(accounts))
        f.h.check(f"final: {name}'s order number stays used in its mandate's window", f.order_used(tab), True)
        mandate = f.registry.read(f.h.svm, tab["mandate"], "Mandate")
        f.h.check(f"final: {name}'s mandate has closed one Tab", mandate["closed_tabs"], 1)
    f.h.check("final: agent 2's permission still pins size = 10 and unit_price = 1 at revision 1",
        f.pinned_permission(f.permission2), dict(revision=1, active=True, pinned=f.pin2, unused_slots_zero=True))
    f.h.check("final: the rule still holds its terms", f.read_rules("Rule", f.rule_tab["rule"])["terms"], f.rule_terms())
    f.positions("company", customer_reserved=0, customer_principal=0)
    f.positions("supplier", supplier_committed=0, supplier_receivable=0, cash_received=PAID)
    f.positions("financier", funding_reserved=0, facility_assets=0)
    f.positions("issuer", facility_debt=0, owned_principal=0)
    for name, kind, address, number in (
            ("the lender's custody", "FinancierCustody", f.parties["financier"]["custody"], NUMBER["top_up"]),
            ("the seller's book", "FinanceBook", f.parties["supplier"]["book"], NUMBER["surrender"]),
            ("the issuer's book", "FinanceBook", f.parties["issuer"]["book"], NUMBER["facility"])):
        f.h.check(f"final: number {number} is used in {name}'s window", f.window_used(kind, address, number), True)
    f.h.check("final: the refused over-ask's number is still open in the lender's custody window",
        f.window_used("FinancierCustody", f.parties["financier"]["custody"], NUMBER["over_ask"]), False)
    f.h.check("collecting key holds no lamports after returning them", f.h.svm.balance(f.collector.pubkey()), 0)
    return figures


def rule_scenario(f):
    """Order 3's numbers and rule, and its timeline once the stage has built the order."""
    settled = min(RULE_ORDER, RULE_CLAIMED, RULE_SIGNED)
    out = dict(program=str(dr.RULES), order=RULE_ORDER, unit_price=1, carrier_attests=RULE_SIGNED,
        seller_attests_refused=RULE_CLAIMED, seller_claims=RULE_CLAIMED, bill=settled, released=RULE_ORDER - settled,
        rule_terms=f.rule_terms(), work_deadline_lead_seconds=RULE_LEAD_SECONDS,
        seller_terms=[list(pair) for pair in pt.SELLER_TERMS] + [["delivery_signer", "the rule's delivery-source address"]])
    tab = getattr(f, "rule_tab", None)
    if tab is not None:
        policy = tab["billing_policy"]
        signing = policy["work_until"] + RULE_SIGN_SECONDS
        objections = signing + RULE_OBJECT_SECONDS
        decision = objections + out["rule_terms"]["decide_seconds"]
        out.update(work_until=policy["work_until"], signing_closes=signing, objections_close=objections,
            decision_closes=decision, evidence_deadline=policy["evidence_deadline"], claim_deadline=policy["claim_deadline"],
            earliest_evidence_deadline_the_program_accepts=decision + dr.MIN_SETTLE,
            order_terms=[dict(name=t["name"].hex(), value=t["value"].hex()) for t in tab["order_terms"]],
            order_terms_hash=pt.order_terms_hash(tab["order_terms"]).hex(), policy_hash=tab["policy_hash"].hex())
    return out


def load_artifacts(root):
    """IDLs and binaries of one build, tied to the program source in this checkout."""
    output = Path(root) / ".local/anchor"
    build = json.loads((output / "build-receipt.json").read_text())
    idl_receipt = json.loads((output / "idl-receipt.json").read_text())
    assert build["status"] == idl_receipt["status"] == "passed"
    assert build["inputs"] == idl_receipt["inputs"], "programs and IDLs were built from different source"
    for path, checksum in build["inputs"].items():
        assert digest(ROOT / path) == checksum, (path, "this checkout's program source differs from the build's")
    idls, binaries = {}, {}
    for name, program in PROGRAMS.items():
        path = output / "idl" / (name + ".json")
        assert digest(path) == idl_receipt["artifacts"][name + ".json"]["sha256"], (name, "IDL differs from its receipt")
        idls[name] = Idl(path)
        assert idls[name].program == program
        binary = output / "deploy" / (name + ".so")
        assert digest(binary) == build["artifacts"][name + ".so"]["sha256"], (name, "binary differs from its receipt")
        binaries[name] = binary
    path = output / "idl/delivery_rules.json"
    assert digest(path) == idl_receipt["artifacts"]["delivery_rules.json"]["sha256"], "delivery_rules IDL differs from its receipt"
    idls["delivery_rules"] = Idl(path)
    assert idls["delivery_rules"].program == dr.RULES
    binaries["delivery_rules"] = output / "deploy/delivery_rules.so"
    assert build["artifacts"]["delivery_rules.so"]["sha256"] == digest(binaries["delivery_rules"]) \
        == digest(dr.RETAINED) == dr.RETAINED_SHA256, "the build's delivery_rules.so is not the binary this checkout retains"
    return idls, binaries, build


def local_binary(path):
    raw = Path(path).read_bytes()
    return dict(path=str(path), bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest(),
        sha256_without_trailing_zeros=hashlib.sha256(raw.rstrip(b"\0")).hexdigest())


def check_programs(rpc, binaries, swig_binary, cluster):
    """Hash what is deployed and compare it with the pinned build.

    The local files must hash to `PINNED_BUILD`; the chain must hold those
    bytes, allowing trailing zero padding. Without `--swig-binary` the Swig
    program is compared with the pinned hash directly; on `--cluster local` the
    repository's retained Swig ELF is also a known build. The delivery-rules
    program must be owned by the upgradeable loader and have no upgrade authority
    or exactly ClaimsCore's: ClaimsCore accepts an order naming it only then.
    """
    result, mismatched = {}, []
    expected = dict(binaries)
    if swig_binary:
        expected["swig"] = swig_binary
    core_authority = None
    for name, program in (*PROGRAMS.items(), ("swig", SWIG), ("delivery_rules", dr.RULES)):
        chain = program_binary(rpc, program)
        entry = dict(chain=chain, expected=None, pinned_sha256=PINNED_BUILD[name], matches=None)
        if name == "claims_core" and chain is not None:
            core_authority = chain.get("upgrade_authority")
        if chain is None:
            entry["matches"] = False
            mismatched.append(name + " (no program at this address)")
            result[name] = entry
            continue
        if name == "delivery_rules":
            authority = chain.get("upgrade_authority")
            entry["trusted_by_claims_core"] = chain["loader"] == str(UPGRADEABLE_LOADER) and (
                authority is None or authority == core_authority)
            if not entry["trusted_by_claims_core"]:
                mismatched.append(f"delivery_rules (loader {chain['loader']}, upgrade authority {authority}, ClaimsCore's "
                    f"{core_authority}: ClaimsCore accepts orders naming it only when it has none or the same)")
        if name in expected:
            local = local_binary(expected[name])
            entry["expected"] = local
            known = {PINNED_BUILD[name]} | ({SWIG_RETAINED_SHA256} if name == "swig" and cluster == "local" else set())
            entry["local_file_is_pinned_build"] = local["sha256"] in known
            # A program account can be longer than its ELF; the tail is zeros.
            entry["matches"] = False if not entry["local_file_is_pinned_build"] else (
                "exact" if chain["sha256"] == local["sha256"] else
                "after removing trailing zero padding"
                if chain["sha256_without_trailing_zeros"] == local["sha256_without_trailing_zeros"] else False)
        else:
            entry["matches"] = "exact" if chain["sha256"] == PINNED_BUILD[name] else False
        if entry["matches"] is False:
            mismatched.append(name)
        result[name] = entry
    return result, mismatched


def finalize(h, rows, log):
    """Wait until every signature is finalized, then read each transaction back once more.

    The status of every signature is always checked. A node that has pruned an
    old block no longer returns the transaction itself; that is recorded for the
    case, and the status (slot and error) is what was compared.
    """
    signatures = [row["signature"] for row in rows if row.get("signature")]
    deadline = time.monotonic() + 180
    while True:
        statuses = []
        for start in range(0, len(signatures), 256):
            statuses += h.rpc.call("getSignatureStatuses", [signatures[start:start + 256],
                {"searchTransactionHistory": True}])["value"]
        pending = sum(1 for status in statuses if status is None or status.get("confirmationStatus") != "finalized")
        if not pending:
            break
        assert time.monotonic() < deadline, f"{pending} signatures not finalized after 180 seconds"
        log(f"   waiting for finality: {pending} of {len(signatures)} signatures not yet finalized")
        time.sleep(3)
    status_of = dict(zip(signatures, statuses))
    first_available = None
    verified = []
    for row in rows:
        if not row.get("signature"):
            verified.append(dict(row, finalized=None))
            continue
        status = status_of[row["signature"]]
        assert (status["slot"], status["err"]) == (row["slot"], row["err"]), \
            (row["name"], "finalized status differs from the journal", status)
        assert (status["err"] is not None) == (row["expect"] == "refuse"), \
            (row["name"], "REFUSAL DID NOT REFUSE" if row["expect"] == "refuse" else "expected success is a failed transaction")
        try:
            chain = h.describe(row["signature"], h.fetch(row["signature"], "finalized", attempts=6))
        except LookupError:
            if first_available is None:
                first_available = h.rpc.call("getFirstAvailableBlock")
            assert first_available > row["slot"], (row["name"], "node holds the block but not the transaction")
            verified.append(dict(row, finalized=True, read_back_at_finality="not available: the node has pruned this block"))
            continue
        problems = [(field, row.get(field), chain[field]) for field in ("slot", "outcome", "err", "compute_units",
            "fee_lamports", "logs") if row.get(field) != chain[field]]
        if row["expect"] == "refuse":
            problems += refusal_problems(classify(chain["err"], chain["logs"]), row["expected_refusal"])
        assert not problems, (row["name"], "finalized transaction differs from the journal", problems)
        verified.append(dict(row, finalized=True, read_back_at_finality="identical"))
    return verified


def parse():
    parser = argparse.ArgumentParser(description="Send the lender-backed order sequence to a validator over JSON-RPC.")
    parser.add_argument("--rpc-url", required=True, help="JSON-RPC endpoint. There is no default.")
    parser.add_argument("--cluster", required=True, choices=("local", "devnet", "testnet"),
        help="What the endpoint is expected to be; checked against its genesis hash.")
    parser.add_argument("--salt-file", required=True, help="Run salt (mode 600). Every key of the run derives from it.")
    parser.add_argument("--new", action="store_true", help="Create the salt file and start a run. Without it, resume.")
    parser.add_argument("--payer", help="Keypair file of the one funded payer.")
    parser.add_argument("--artifact-root", default=str(ROOT), help="Checkout holding .local/anchor/{idl,deploy} of the deployed build.")
    parser.add_argument("--swig-binary", help="Swig program file expected on chain.")
    parser.add_argument("--allow-binary-mismatch", action="store_true")
    parser.add_argument("--check-only", action="store_true", help="Read the cluster, programs and payer; send nothing.")
    parser.add_argument("--out", help="Record path. Default: <salt-file>.record.json")
    parser.add_argument("--min-interval", type=float, default=0.55, help="Seconds between RPC requests.")
    parser.add_argument("--window-seconds", type=int, default=7 * 86_400, help="Life of the agent's permission and the order.")
    parser.add_argument("--bundle-seconds", type=int, default=900, help="Life of one wallet authorization.")
    parser.add_argument("--priority-fee-lamports", type=int, default=0)
    parser.add_argument("--max-accounts", type=int, default=64, help="Per-transaction account limit to hold to.")
    parser.add_argument("--min-payer-lamports", type=int, default=750_000_000,
        help="Refuse to start a new run with less than this in the payer.")
    parser.add_argument("--stop-after", help="End this invocation once the case of this name (or name prefix) lands.")
    return parser.parse_args()


def endpoint_label(url):
    """Host only: an endpoint's path or query can carry an access key."""
    parsed = urlparse(url)
    return f"{parsed.scheme}://{parsed.hostname}" + (f":{parsed.port}" if parsed.port else "")


def main():
    args = parse()
    started, started_at = time.monotonic(), utc()
    log = lambda text: print(text, flush=True)
    rpc = Rpc(args.rpc_url, min_interval=args.min_interval, log=log)

    genesis = rpc.call("getGenesisHash")
    known = GENESIS.get(genesis)
    if known == "mainnet-beta":
        raise SystemExit("This endpoint is mainnet-beta. This driver does not run there.")
    if args.cluster == "local":
        if known is not None or not is_loopback(args.rpc_url):
            raise SystemExit(f"--cluster local needs a loopback endpoint that is not a public cluster (genesis {genesis}).")
    elif known != args.cluster:
        raise SystemExit(f"--cluster {args.cluster} but the endpoint's genesis hash {genesis} is {known or 'not a known public cluster'}.")
    version = rpc.call("getVersion")
    log(f"cluster {args.cluster}  genesis {genesis}  node {version.get('solana-core')}  endpoint {endpoint_label(args.rpc_url)}")

    idls, binaries, build = load_artifacts(args.artifact_root)
    programs, mismatched = check_programs(rpc, binaries, args.swig_binary, args.cluster)
    for name, entry in programs.items():
        chain = entry["chain"] or dict(program="(no program at this address)", sha256=None)
        log(f"program {name:19} {chain['program']}  sha256 {chain['sha256']}  matches build: {entry['matches']}"
            + (f"  upgrade authority: {chain.get('upgrade_authority') or 'none'}" if entry["chain"] else ""))
    if mismatched and not args.allow_binary_mismatch:
        raise SystemExit(f"Deployed program differs from the expected binary: {', '.join(mismatched)}. Nothing was sent.")

    payer = load_keypair(args.payer) if args.payer else None
    if args.check_only:
        chain = ChainAccounts(rpc)
        log(f"cluster time {chain.clock()}  payer {payer.pubkey() if payer else None}  "
            f"payer lamports {chain.balance(payer.pubkey()) if payer else None}")
        log(f"check only: nothing sent. requests {rpc.usage()['http_requests']}")
        return
    assert payer is not None, "--payer is required to send"

    chain = ChainAccounts(rpc)
    balance_now = chain.balance(payer.pubkey())
    if args.new:
        if balance_now < args.min_payer_lamports:
            raise SystemExit(f"Payer holds {balance_now} lamports; a new run needs at least {args.min_payer_lamports}. Nothing was sent.")
        Run.create(args.salt_file, genesis_hash=genesis, cluster=args.cluster, payer=str(payer.pubkey()),
            cluster_time_start=chain.clock(), window_seconds=args.window_seconds)
    run = Run.load(args.salt_file)
    bound = run.value
    assert bound["genesis_hash"] == genesis, "this run was started on a different cluster"
    assert bound["payer"] == str(payer.pubkey()), "this run was started with a different payer"
    journal = Journal(args.salt_file + ".journal.json")
    harness = RpcHarness("public-rpc-sequence", rpc, payer, journal, max_accounts=args.max_accounts,
        priority_fee=args.priority_fee_lamports, log=log)
    harness.ledger = RpcLedger(idls["claims_core"])
    header = journal.value["header"]
    if not header:
        header.update(genesis_hash=genesis, cluster=args.cluster, payer=str(payer.pubkey()),
            cluster_time_start=bound["cluster_time_start"], window_seconds=bound["window_seconds"],
            salt_sha256=hashlib.sha256(run.salt).hexdigest(), started_at=bound["created_at"],
            # Known only when the journal dates from the run's first moment.
            payer_start_lamports=balance_now if args.new else None,
            slot_start=rpc.call("getSlot", [{"commitment": "confirmed"}]) if args.new else None,
            rebuilt_from_chain=not args.new)
        journal.save()
        if not args.new:
            log("no journal beside the salt file: rebuilding the run from the chain")
    harness.recovering = bool(header.get("rebuilt_from_chain")) and not any(
        entry["status"] == "passed" for entry in journal.value["invocations"])
    # Kept current on every journal write, so an interrupted invocation still counts.
    invocation = dict(started_at=started_at, status="interrupted")
    journal.value["invocations"].append(invocation)
    def account_for_invocation():
        invocation.update(ended_at=utc(), wall_seconds=round(time.monotonic() - started, 1), sent=harness.sent,
            already_on_chain=harness.skipped, adopted_from_chain=harness.adopted, rpc=rpc.usage())
    journal.before_save = account_for_invocation
    assert header["salt_sha256"] == hashlib.sha256(run.salt).hexdigest(), "journal belongs to another salt"
    # The fixtures read these module names for identifiers and time.
    ff.ident, ff.NOW = run.ident, header["cluster_time_start"]
    eb.ident = fc.ident = run.ident
    if args.stop_after:
        send_one = harness.send
        def send_then_stop(name, *rest, **options):
            row = send_one(name, *rest, **options)
            if row.get("name", "").startswith(args.stop_after) and not harness.replaying:
                raise Interrupted(row["name"])
            return row
        harness.send = send_then_stop

    f = PublicSequence(harness, run, idls, t0=header["cluster_time_start"], window=header["window_seconds"],
        bundle_seconds=args.bundle_seconds)
    status, failure, order = "passed", None, None
    try:
        order, _ = sequence(f)
        rule_stage(f)
        figures = final_state(f, order)
        cases = finalize(harness, journal.rows(), log)
    except Interrupted as stop:
        status, failure, figures = "interrupted", f"--stop-after: stopped once '{stop}' landed", None
        cases = journal.rows()
        log("INTERRUPTED: " + failure)
    except (StepFailed, AssertionError) as error:
        status, failure, figures = "failed", str(error), None
        cases = journal.rows()
        log("FAILED: " + failure[:6000])

    after = harness.svm.balance(payer.pubkey())
    spent = None if header["payer_start_lamports"] is None else header["payer_start_lamports"] - after
    funded = {str(f.collector.pubkey())}
    fees = sum(row.get("fee_lamports") or 0 for row in cases
        if row.get("fee_payer") in ({str(payer.pubkey())} | funded))
    usage = rpc.usage()
    invocation["status"] = status
    journal.save()

    explorer = (lambda signature: f"https://explorer.solana.com/tx/{signature}?cluster={args.cluster}") \
        if args.cluster != "local" else (lambda signature: None)
    keep = ("name", "phase", "expect", "outcome", "passed", "finalized", "read_back_at_finality", "signature", "slot", "block_time", "err",
        "error", "expected_refusal", "simulated_before_send", "compute_units", "fee_lamports", "fee_payer",
        "signature_count", "signer_keys", "account_count", "wire_bytes", "top_level_instructions",
        "payer_lamports_delta", "lamport_deltas", "events", "figures_before", "figures_after", "adopted", "note", "problems", "logs")
    tabs = [("order_1", order), ("order_2", getattr(f, "order2", None)), ("order_3", getattr(f, "rule_tab", None))]
    record = dict(
        schema="dexter-public-rpc-sequence/v3", status=status, failure=failure,
        cluster=dict(name=args.cluster, genesis_hash=genesis, endpoint=endpoint_label(args.rpc_url),
            node_version=version.get("solana-core"), feature_set=version.get("feature-set"),
            slot_start=header["slot_start"], slot_end=rpc.call("getSlot", [{"commitment": "finalized"}])),
        scenario=dict(order=ORDER, per_order_limit=PER_ORDER_LIMIT, budget=BUDGET, facility_limit=CREDIT_LIMIT,
            credit_line_limit=CREDIT_LIMIT, lender_cash_at_first=LENDER_START, lender_cash_added=LENDER_TOP_UP,
            earned=EARNED, released=ORDER - EARNED, first_repayment=FIRST_PAYMENT, repaid=PAID, token_decimals=6,
            interest=0, seller_fee=0, numbers=NUMBER, window_seconds=header["window_seconds"],
            pinned=dict(order=PINNED_ORDER, earned=PINNED_EARNED, pinned_terms=[["size", "10"], ["unit_price", PINNED_UNIT_PRICE]],
                refused_unit_price=REFUSED_UNIT_PRICE, pinned_list=[dict(name=t["name"].hex(), value=t["value"].hex())
                    for t in getattr(f, "pin2", [])],
                seller_terms=[list(pair) for pair in pt.SELLER_TERMS],
                refused_terms=[list(pair) for pair in pt.changed(pt.SELLER_TERMS, size="11")],
                name_fingerprint_size=pt.name_print("size").hex(),
                value_fingerprints={value: pt.value_print(value).hex() for value in ("10", "11")},
                order_terms_hash_shown={name: pt.order_terms_hash(tab["order_terms"]).hex() for name, tab in (
                    getattr(f, "order2_variants", {}).items())}),
            delivery_rule=rule_scenario(f)),
        times=dict(started_at=header["started_at"], ended_at=utc(), cluster_time_start=header["cluster_time_start"],
            cluster_time_end=harness.svm.clock(), invocations=journal.value["invocations"],
            wall_seconds_all_invocations=round(sum(i.get("wall_seconds", 0) for i in journal.value["invocations"]), 1)),
        programs={name: dict(program_id=(entry["chain"] or {}).get("program"), sha256_on_chain=(entry["chain"] or {}).get("sha256"),
            bytes_on_chain=(entry["chain"] or {}).get("bytes"), loader=(entry["chain"] or {}).get("loader"),
            last_deployed_slot=(entry["chain"] or {}).get("last_deployed_slot"),
            upgrade_authority=(entry["chain"] or {}).get("upgrade_authority"), read_at_slot=(entry["chain"] or {}).get("read_at_slot"),
            expected_sha256=entry["expected"]["sha256"] if entry["expected"] else None, pinned_sha256=entry["pinned_sha256"],
            local_file_is_pinned_build=entry.get("local_file_is_pinned_build"), matches_expected=entry["matches"],
            **({} if "trusted_by_claims_core" not in entry else dict(trusted_by_claims_core=entry["trusted_by_claims_core"])))
            for name, entry in programs.items()},
        build=dict(artifact_root=str(args.artifact_root), build_inputs_sha256=hashlib.sha256(
            json.dumps(build["inputs"], sort_keys=True).encode()).hexdigest(),
            delivery_rules_binary=dict(path=str(dr.RETAINED.relative_to(ROOT)), sha256=dr.RETAINED_SHA256),
            driver_sources={name: digest(Path(__file__).resolve().parent / name) for name in (
                "public_rpc_sequence.py", "public_rpc_transport.py", "financing_fixture.py", "financing_support.py",
                "financing_commercial.py", "tab_closes.py", "pinned_terms.py", "pinned_price.py", "evidence_billing.py",
                "pinned_delivery.py", "early_settlement.py", "delivery_rules.py")}),
        run=dict(salt_file=str(Path(args.salt_file).resolve()), salt_sha256=header["salt_sha256"],
            journal=str(journal.path.resolve()),
            public_keys=dict(payer=str(payer.pubkey()), agent=str(f.agent.pubkey()), agent_2=str(f.agent2.pubkey()),
                collecting_key=str(f.collector.pubkey()), carrier=str(f.carrier.pubkey()), token_mint=str(f.mint),
                **{ROLE[role].replace(" ", "_") + "_owner": str(f.parties[role]["owner"].pubkey()) for role in f.ROLES}),
            addresses=dict(budget=str(f.budget), agent_record=str(f.actors[0]), agent_2_record=str(f.actors[1]),
                **{ROLE[role].replace(" ", "_") + "_principal": str(f.parties[role]["principal"]) for role in f.ROLES},
                **({} if order is None else dict(permission=str(f.permission), facility=str(f.facility),
                    credit_line=str(f.line), lender_wallet=str(f.parties["financier"]["wallet"]),
                    lender_wallet_configuration=str(f.parties["financier"]["config"]),
                    lender_wallet_binding=str(f.parties["financier"]["binding"]),
                    lender_wallet_token_account=str(f.parties["financier"]["custody_token"]),
                    lender_cash_record=str(f.parties["financier"]["custody"]),
                    seller_token_account=str(f.parties["supplier"]["owner_ata"]))),
                **{name: dict(mandate=str(tab["mandate"]), order_number=tab["order"], tab=str(tab["claim"]),
                    budget_hold=str(tab["reservation"]), **({} if "bill" not in tab else dict(bill=str(tab["bill"]),
                        case=str(tab["case"]))))
                    for name, tab in tabs if tab is not None},
                **({} if getattr(f, "rule_tab", None) is None else dict(rule=str(f.rule_tab["rule"]),
                    rule_delivery_source=str(f.rule_tab["source"]), rule_permission=str(f.rule_permission),
                    pinned_permission=str(f.permission2))))),
        closes=f.closes_done,
        change5_ledger=harness.ledger.counts,
        transport=harness.transport,
        payer=dict(address=str(payer.pubkey()), start_lamports=header["payer_start_lamports"], end_lamports=after,
            spent_lamports=spent, spent_sol=None if spent is None else spent / 1e9,
            of_which_transaction_fees_lamports=fees,
            of_which_account_deposits_lamports=None if spent is None else spent - fees,
            sum_of_per_transaction_payer_changes=sum(row.get("payer_lamports_delta") or 0 for row in cases),
            payer_changes_by_phase={phase: sum(row.get("payer_lamports_delta") or 0 for row in cases if row["phase"] == phase)
                for phase in dict.fromkeys(row["phase"] for row in cases)}),
        rpc=dict(this_invocation=usage, http_requests_all_invocations=sum(
            i["rpc"]["http_requests"] for i in journal.value["invocations"] if "rpc" in i)),
        counts=dict(transactions=sum(1 for row in cases if row.get("signature")),
            succeeded=sum(1 for row in cases if row.get("outcome") == "success"),
            refused_on_chain=sum(1 for row in cases if row.get("outcome") == "failed"),
            sent_this_invocation=harness.sent, already_on_chain_this_invocation=harness.skipped,
            adopted_from_chain_this_invocation=harness.adopted,
            largest_transaction_bytes=max((row.get("wire_bytes") or 0 for row in cases), default=0),
            most_accounts_in_one_transaction=max((row.get("account_count") or 0 for row in cases), default=0),
            assertions=len(journal.value["assertions"])),
        journal_rebuilt_from_chain=bool(header.get("rebuilt_from_chain")),
        final_figures=figures,
        cases=[dict({key: row[key] for key in keep if key in row}, explorer=explorer(row.get("signature")))
            for row in cases],
        assertions=journal.value["assertions"])
    out = Path(args.out or args.salt_file + ".record.json")
    out.write_text(json.dumps(record, indent=1, default=json_value) + "\n")

    log("")
    for row in cases:
        if row["phase"] in ("setup", "return lamports"):
            continue
        error = row.get("error") or {}
        log(f"{row.get('outcome', row['status']):8} {str(row.get('finalized')):5} "
            f"{(error.get('raising_program') or ''):19}{(error.get('error_name') or ''):22}"
            f"{str(error.get('custom_code') or ''):5} {row.get('compute_units') or '':>7}  {row.get('signature')}  {row['name']}")
    log(json.dumps(dict(status=status, record=str(out), transactions=record["counts"]["transactions"],
        succeeded=record["counts"]["succeeded"], refused_on_chain=record["counts"]["refused_on_chain"],
        sent_this_invocation=harness.sent, already_on_chain=harness.skipped, adopted=harness.adopted,
        payer_spent_sol=record["payer"]["spent_sol"], http_requests=usage["http_requests"],
        http_requests_all_invocations=record["rpc"]["http_requests_all_invocations"],
        wall_seconds=journal.value["invocations"][-1].get("wall_seconds"))))
    if status != "passed":
        sys.exit(1 if status == "failed" else 3)


if __name__ == "__main__":
    main()
