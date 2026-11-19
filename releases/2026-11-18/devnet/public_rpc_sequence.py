#!/usr/bin/env python3
"""One lender-backed order, end to end, sent to a validator over JSON-RPC.

One buyer with one agent, one seller, one credit issuer and one lender whose
cash sits in a Swig wallet bound to its principal. A synthetic six-decimal
token is created by the run. No interest and no seller fee.

  per-order limit 100,000 | budget 500,000 | facility and credit line 500,000
  lender cash 99,999, then 150,000 | order 100,000 | earned 60,000

  1. setup
  2. the commit (ClaimsCore accept_financed_tab), the same order sent four times:
       money       lender free cash 99,999, one unit below the order   refused
       permission  amount 100,001, one unit above the agent's limit    refused
       deal        unit price 2 where the agreed terms commit to 1      refused
       unchanged                                                       accepted
  3. the seller reports 60,000 of completed work
  4. collection (Registry authorize_funding_wallet + Swig SignV2 + ClaimsCore
     collect_financier) sent and paid for by a key that is none of the parties:
       payee swapped to the sender's own token account                 refused
       an extra instruction after SignV2                               refused
       unchanged                                                       accepted
  5. the lender asks for one unit more than its free cash             refused
     the seller releases the order's unused 40,000                    accepted
  6. owner-pinned terms, on a second agent (order 2, 50,000, fully earned):
       the buyer's owner creates agent 2's permission already pinned to
       size = 10 and unit_price = 1 per completed-run unit of the order's
       service (PrincipalRegistry create_pinned_mandate)                accepted
       order 2 says size = 11, through accept_pinned_financed_tab       refused
       the same size = 11 order through plain accept_financed_tab       refused
       order 2 at 2 per unit, listing its own unit_price               refused
       order 2 says size = 10 at 1 per unit, through the pinned call    accepted
       the seller reports all 50,000 as completed work                  accepted
       a key with no role collects the 50,000 for the seller            accepted

The refusals come before the acceptance they are compared with because they
are the same order: once it is accepted its record exists and it cannot be
offered again. The money refusal needs free cash below the order and the
permission refusal needs free cash above it (ClaimsCore checks cash before it
asks the Registry), so the lender adds cash between the two. Order 2 is 50,000
so that the 90,000 the lender holds after order 1 covers it and both of its
refusals reach the Registry's terms check.

An order's terms are (name, value) fingerprint pairs and its
`external_contract_hash` is the hash of that list; the helpers and the shared
vectors are those of pinned_terms.py. The reserved term `unit_price` carries
sha256("DEXTER:UNIT-PRICE:1" || namespace || unit || u64le(price)) of the
order's own price (pinned_price.py); every order 2 lists it.

Every refusal is simulated, then sent with preflight skipped so the failed
transaction is recorded on chain, then read back and compared with the error
named here in advance.

Nothing here selects an endpoint. `--rpc-url`, `--cluster`, `--payer` and
`--salt-file` are required; the genesis hash must agree with `--cluster`; a
mainnet genesis hash is refused outright; the deployed programs are hashed and
compared with the build the IDLs came from before anything is sent. That build
must be dexter-account main 5df2ff1: the three program files under
`--artifact-root` and the deployed programs are compared with the hashes in
`PINNED_BUILD`, and the Swig program with the devnet dump's hash.

  <python> public_rpc_sequence.py --rpc-url <url> --cluster <local|devnet|testnet> \\
      --payer <keypair.json> --salt-file <run.salt.json> --new \\
      --artifact-root <checkout holding .local/anchor/{idl,deploy}> [--swig-binary <swig.so>]

Add `--check-only` to read the cluster, the programs and the payer and send
nothing. `--new` creates the salt file (mode 600); every key and identifier of
the run is derived from it, and it records the cluster, the payer and the
cluster time the run's windows start from. The same command without `--new`
resumes: a case the journal records as landed is not sent again, and if the
journal is lost the run is rebuilt from the chain. The journal is
`<salt-file>.journal.json`; the result is `<salt-file>.record.json` unless
`--out` names another path.
"""
from __future__ import annotations

import argparse
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

import financing_fixture as ff
import pinned_terms as pt
import pinned_price as pp
from financing_fixture import FinancingFixture, YEAR
from financing_support import (CORE, POOL, PROGRAMS, REGISTRY, ROOT, SWIG, SYSTEM, TOKEN, Idl, M,
    check_receipt, digest, json_value, pda)
from public_rpc_transport import (GENESIS, ChainAccounts, Journal, Rpc, RpcHarness, StepFailed, classify,
    is_loopback, load_keypair, program_binary, refusal_problems)

ORDER = 100_000
PER_ORDER_LIMIT = 100_000
BUDGET = 500_000
CREDIT_LIMIT = 500_000
LENDER_START = ORDER - 1
LENDER_TOP_UP = 50_001
LENDER_CASH = LENDER_START + LENDER_TOP_UP
EARNED = 60_000
MINTED = 1_000_000
SIGNATURE_FEE = 5_000
PINNED_ORDER = 50_000
PINNED_EARNED = PINNED_ORDER
PINNED_UNIT_PRICE = 1
REFUSED_UNIT_PRICE = 2
PERMISSION_BYTES = 517
# dexter-account main 5df2ff1 (owner-pinned terms, delivery signer and price, early settlement,
# program-sourced delivery signer), and Swig as dumped from devnet.
PINNED_BUILD = dict(
    principal_registry="f6f4052ceb824561f98718176bb4d32d7851a603a949b750aecf2b9465587659",
    claims_core="a96472c1e26f35fcabe9a5af9122c2d807c1048ba8a7a511dc0b2ea5473b69f2",
    pool_issuer="6804757b94ce2da8854388a1f748b614296207682db27cde85958f868b726e65",
    swig="0418f037fe1c9f6c0cd9d3ee57d684f69aca2ed9b3388f5d0c45020b01cce1d9")
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


def utc():
    return datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def variant(tab):
    """Independent copy of one prepared order; signer objects are shared."""
    terms = dict(tab["terms"])
    args = dict(tab["args"], terms=terms, seller_fee_terms=dict(tab["args"]["seller_fee_terms"]))
    return dict(tab, terms=terms, args=args, accounts=dict(tab["accounts"]), signers=list(tab["signers"]))


class PublicSequence(FinancingFixture):
    """The suites' builders and checks over `RpcHarness`.

    Replaced here, and only here: fixed keys and identifiers (salted), airdrops
    (one payer funds what a step needs), the fixed clock and 600-second windows
    (cluster clock, configurable window), and parties this order does not use.
    """
    ROLES = ("company", "issuer", "financier", "supplier")

    def __init__(self, harness, run, idls, *, t0, window, bundle_seconds):
        # The parent constructor builds a LiteSVM, loads programs into it and
        # airdrops to fixed keys; none of that applies to a validator.
        self.h, self.run, self.idls = harness, run, idls
        self.registry, self.core, self.pool = [idls[name] for name in PROGRAMS]
        self.t0, self.window, self.bundle_seconds, self.now = t0, window, bundle_seconds, t0
        self.domain = ff.ident("domain")
        self.parties, self.protected, self.receipts, self.tabs = {}, set(), [], {}
        for name in self.ROLES:
            owner = run.key("owner-" + name)
            principal_id = ff.ident("principal-" + name)
            self.parties[name] = dict(name=name, owner=owner, principal_id=principal_id, generation=1,
                principal=pda(REGISTRY, b"principal", owner.pubkey(), principal_id))
        # The inherited collection builder looks this entry up before it knows
        # the order carries no seller fee. It is never registered or sent.
        self.parties["fee"] = dict(name="fee")
        self.fee_agreements = dict(main=dict(address=None, agreement_hash=ff.ident("no-seller-fee"), cap=0))
        self.agent, self.agent2 = run.key("agent-1"), run.key("agent-2")
        self.collector = run.key("collector")
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

    def observe(self, *addresses):
        """The in-process suites snapshot every observed account around each
        transaction. Over RPC each case reads the figures it asserts on."""

    # -- sending -----------------------------------------------------------
    def send(self, label, instruction, signers=(), *, reject=False, error=None, receipt=None, effect=None):
        assert error is None, "name the expected refusal with expect_refusal"
        label = self.h.titles.get(label, label)
        instructions = list(instruction) if isinstance(instruction, (list, tuple)) else [instruction]
        # Same rule as the parent: owner fields accept native signatures or
        # passkey proofs, so the supplied native signers are marked explicitly.
        supplied = {signer.pubkey() for signer in signers}
        instructions = [Instruction(item.program_id, bytes(item.data),
            [M(meta.pubkey, meta.is_signer or meta.pubkey in supplied, meta.is_writable) for meta in item.accounts])
            for item in instructions]
        if reject:
            assert receipt is not None, ("a refusal names an account whose history will hold it", label)
            self.h.witnesses[label] = receipt
        else:
            effect = effect if effect is not None else receipt
            assert effect is not None or label in self.h.effects, ("a sent step states its on-chain effect", label)
            if effect is not None:
                self.h.effects[label] = effect
        row = self.h.send(label, instructions, signers, fail="any" if reject else None)
        if receipt is not None and not reject and not self.h.replaying:
            data = check_receipt(self.core, self.h.svm, receipt)
            for change in data["effects"]:
                book = self.core.read(self.h.svm, change["book"], "FinanceBook")["data"]
                self.h.check(label + ": book frontier", book["sequence"], change["sequence"])
                self.h.check(label + ": book amounts", book["positions"], change["after"])
                self.h.check(label + ": book hash", book["head"], change["hash"])
            self.receipts.append(dict(address=str(receipt), data=data))
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
        tab = super().acceptance(name, **options)
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
        for role in ("supplier", "financier"):
            party = self.parties[role]
            self.send(f"create the {ROLE[role]} owner's token account",
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
            # Later steps move this balance, so the cash record that follows also counts.
            effects[titles[f"prefund {role} token account before explicit allocation"]] = lambda: (
                self.h.svm.get_account(where["custody"]) is not None or self.h.amount(where["token"]) >= amount)
        titles[f"owner allocates {role} capital without inventing token inflow"] = \
            f"{who}'s owner opens the cash record for its wallet ({amount:,} allocated)"
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
                args=dict(mandate_id=mandate_id, supplier=self.parties["supplier"]["owner"].pubkey(),
                    source=self.facility, per_operation_limit=PER_ORDER_LIMIT,
                    expires_at=self.t0 + self.window)), [buyer["owner"]], effect=mandate)
        return mandate

    # -- owner-pinned terms ------------------------------------------------
    def enroll_second_agent(self):
        buyer = self.parties["company"]
        self.send("buyer's owner enrolls agent 2", self.ix("principal_registry", "create_actor",
            dict(owner=buyer["owner"].pubkey(), principal=self.company, actor=self.actors[1]),
            args=dict(actor_id=ff.ident("actor-2"), current_key=self.agent2.pubkey(),
                valid_from=self.t0 - 100, valid_until=self.t0 + self.window + 86_400)), [buyer["owner"]],
            effect=self.actors[1])

    def pinned_mandate_address(self):
        return pda(REGISTRY, b"mandate", self.company, ff.ident("mandate-agent-2"))

    def permit_pinned(self, label, required):
        """create_pinned_mandate: agent 2's permission and its pinned terms in one instruction."""
        buyer = self.parties["company"]
        mandate_id = ff.ident("mandate-agent-2")
        mandate = self.pinned_mandate_address()
        self.send(label, self.ix("principal_registry", "create_pinned_mandate",
            dict(owner=buyer["owner"].pubkey(), principal=self.company, actor=self.actors[1],
                budget=self.budget, mandate=mandate),
            args=dict(mandate_id=mandate_id, supplier=self.parties["supplier"]["owner"].pubkey(),
                source=self.facility, per_operation_limit=PER_ORDER_LIMIT, expires_at=self.t0 + self.window),
            required_terms=required), [buyer["owner"]], effect=mandate)
        return mandate

    def pinned_permission(self, mandate):
        state = self.registry.read(self.h.svm, mandate, "Mandate")
        count = state["required_terms_count"]
        return dict(revision=state["revision"], active=state["active"], pinned=state["required_terms"][:count],
            unused_slots_zero=all(item == dict(name=bytes(32), value=bytes(32)) for item in state["required_terms"][count:]))

    def with_terms(self, tab, pairs, *, price=PINNED_UNIT_PRICE):
        """The order at `price` per unit; its list is `pairs` and its own unit_price term, ascending.

        The signed terms hash and the shown list are built by pinned_terms.py,
        the unit_price term by pinned_price.py.
        """
        tab["unit_price"] = tab["args"]["unit_price"] = price
        listed = pp.ascending(*(pt.term(name, value) for name, value in pairs),
            pp.unit_price_term(tab["namespace"], tab["service_unit"], price))
        return pt.PinnedTerms.set_terms(self, tab, listed)

    def accept_pinned(self, tab, *, reject=False, label=None):
        instruction = self.ix("claims_core", "accept_pinned_financed_tab", tab["accounts"],
            remaining=self.remaining(["company", "issuer", "supplier", tab["financier"]]),
            order_terms=tab["order_terms"], **tab["args"])
        self.send(label, instruction, tab["signers"], reject=reject, receipt=tab["receipt"])
        return instruction

    def setup(self):
        lender = self.parties["financier"]
        self.h.effects["initialize synthetic SPL token and disposable freeze authority"] = self.mint
        self.h.titles["initialize synthetic SPL token and disposable freeze authority"] = \
            f"create the synthetic six-decimal token and mint {MINTED:,} to the payer"
        self.h.effects[self.h.titles["initialize synthetic SPL token and disposable freeze authority"]] = self.mint
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
        top_up = f"give the lender's owner {LENDER_TOP_UP:,} tokens to add later"
        self.h.effects[top_up] = lambda: (self.h.amount(lender["owner_ata"]) >= LENDER_TOP_UP
            or self.h.svm.get_account(self.contribution_receipt("top-up")) is not None)
        self.transfer_tokens(top_up, lender["owner_ata"], LENDER_TOP_UP)

    # -- figures -----------------------------------------------------------
    def lender_cash(self):
        lender = self.parties["financier"]
        custody = self.state("FinancierCustody", lender["custody"])
        promised = custody["unused"] + custody["payable"]
        return dict(wallet=self.h.amount(lender["custody_token"]), accounted=custody["accounted_cash"],
            promised=promised, free=custody["accounted_cash"] - promised)

    def figures(self, tab=None):
        """The numbers every case is judged on, read from the chain."""
        lender, seller = self.parties["financier"], self.parties["supplier"]
        cash = self.lender_cash()
        budget = self.registry.read(self.h.svm, self.budget, "Budget")
        out = dict(lender_wallet_tokens=cash["wallet"], lender_cash_on_record=cash["accounted"],
            lender_promised=cash["promised"], lender_free_cash=cash["free"],
            lender_owner_tokens=self.h.amount(lender["owner_ata"]), seller_tokens=self.h.amount(seller["owner_ata"]),
            collector_tokens=self.h.amount(self.collector_ata),
            budget_held=budget["reserved"], budget_used=budget["consumed"], order=None)
        if tab is not None and self.h.svm.get_account(tab["claim"]) is not None:
            claim = self.state("FundedClaim", tab["claim"])
            out["order"] = {field: claim[field] for field in ("remaining", "payable", "paid", "debt", "released")}
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
    def contribution_receipt(self, name):
        lender = self.parties["financier"]
        return self.receipt(lender["principal"],
            self.operation(b"capital-contribute", lender["owner"].pubkey(), ff.ident("contribute-" + name)))

    def add_cash(self, label, name, amount):
        lender = self.parties["financier"]
        receipt = self.contribution_receipt(name)
        accounts = dict(owner=lender["owner"].pubkey(), principal=lender["principal"], custody=lender["custody"],
            custody_token=lender["custody_token"], receipt=receipt)
        accounts["from"] = lender["owner_ata"]
        self.send(label, self.ix("claims_core", "contribute_financier_capital", accounts,
            remaining=self.remaining(["financier"]), args=dict(request_id=ff.ident("contribute-" + name), amount=amount,
                expected_revision=self.state("FinancierCustody", lender["custody"])["revision"],
                authority_generation=lender["generation"])), [lender["owner"]], receipt=receipt)

    def with_destination(self, request, destination):
        changed = dict(request, accounts=dict(request["accounts"], destination=destination))
        changed["instruction"] = self.ix("claims_core", "collect_financier", changed["accounts"],
            remaining=self.remaining(request["roles"]), **request["args"])
        return changed

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
            lamports=lamports)), [signer], effect=lambda: lamports == 0)


def sequence(f):
    buyer, issuer, lender, seller = [f.parties[role] for role in ("company", "issuer", "financier", "supplier")]
    core = lambda name, index=0: dict(program=CORE, name=name, index=index)
    registry = lambda name, index=0: dict(program=REGISTRY, name=name, index=index)
    usual_payer = f.h.payer

    with f.phase("setup"):
        f.setup()
    order = f.acceptance("order-1", cap=ORDER, maximum_interest=0, fees=False, mandate=f.permission)

    with f.phase("commit"):
        label = f"commit refused (money): order of {ORDER:,} when the lender's free cash is {LENDER_START:,}"
        with f.case(label, order, refuse=core("FundingCashShortfall")):
            f.accept(order, reject=True, label=label)

        label = f"lender adds {LENDER_TOP_UP:,} to its wallet: free cash becomes {LENDER_CASH:,}"
        with f.case(label, order, expected=dict(lender_free_cash=LENDER_CASH, lender_wallet_tokens=LENDER_CASH,
                lender_owner_tokens=0)):
            f.add_cash(label, "top-up", LENDER_TOP_UP)

        changed = variant(order)
        changed["terms"]["cap"] = ORDER + 1
        changed["args"]["seller_fee_terms"]["gross_cap"] = ORDER + 1
        label = f"commit refused (permission): order of {ORDER + 1:,} when the agent's per-order limit is {PER_ORDER_LIMIT:,}"
        with f.case(label, order, refuse=registry("PerOperationLimit")):
            f.accept(changed, reject=True, label=label)

        changed = variant(order)
        changed["args"]["unit_price"] = 2
        label = "commit refused (deal): order sent at unit price 2 when the agreed terms commit to unit price 1"
        with f.case(label, order, refuse=core("FundingUsage")):
            f.accept(changed, reject=True, label=label)

        label = (f"COMMIT ACCEPTED: order of {ORDER:,}, inside the agent's {PER_ORDER_LIMIT:,} limit and the "
            f"{BUDGET:,} budget, lender free cash {LENDER_CASH:,}, seller and credit issuer's owner signed")
        with f.case(label, order, expected=dict(budget_held=ORDER, budget_used=0, lender_promised=ORDER,
                lender_free_cash=LENDER_CASH - ORDER, lender_wallet_tokens=LENDER_CASH,
                order_remaining=ORDER, order_payable=0, order_paid=0, order_debt=0)):
            f.accept(order, label=label)
        if not f.h.replaying:
            row = f.h.cases[-1]
            f.h.check("the commit is one top-level instruction", row["top_level_instructions"], ["claims_core"])
            f.h.check("ClaimsCore calls the Registry inside the commit",
                f"Program {REGISTRY} invoke [2]" in row["logs"], True)
            f.h.check("ClaimsCore calls the Pool inside the commit", f"Program {POOL} invoke [2]" in row["logs"], True)
            f.h.check("commit signers are the fee payer, the agent, the credit issuer's owner and the seller",
                sorted(row["signer_keys"]),
                sorted(str(k.pubkey()) for k in (usual_payer, f.agent, issuer["owner"], seller["owner"])))
            reservation = f.registry.read(f.h.svm, order["reservation"], "Reservation")
            f.h.check("Registry reservation holds the order maximum", reservation["remaining"], ORDER)

    with f.phase("earn"):
        label = f"seller reports {EARNED:,} of completed work on the order"
        with f.case(label, order, expected=dict(order_remaining=ORDER - EARNED, order_payable=EARNED,
                order_debt=EARNED, order_paid=0, budget_used=EARNED, seller_tokens=0)):
            run = f.earn(order, "order-1-work", EARNED, label=label)

    with f.phase("collection"):
        accepted = f"COLLECTION ACCEPTED: a key with no role in the order pays the seller's earned {EARNED:,} from the lender's wallet"
        receipt_rent = f.h.svm.minimum_balance_for_rent_exemption(len(f.h.svm.get_account(order["receipt"]).data))
        floor = f.h.svm.minimum_balance_for_rent_exemption(0)
        # Three transactions' fees and one receipt, above the balance a paying
        # account must keep. Whatever is left is returned in the last phase.
        collector_needs = receipt_rent + floor + 20 * (SIGNATURE_FEE + f.h.priority_fee)
        with f.paid_by(f.collector):
            legit = f.collection(order, run, EARNED, paid_before=0)
        f.fund("payer gives the collecting key lamports for one receipt and its fees", f.collector.pubkey(),
            collector_needs, done=lambda: f.h.svm.get_account(legit["receipt"]) is not None)
        f.send("create the collecting key's own token account", f.h.ata_ix(f.collector.pubkey(), f.mint),
            effect=f.collector_ata)
        with f.paid_by(f.collector):

            label = f"collection refused: payee swapped to the collecting key's own token account"
            with f.case(label, order, refuse=core("FundingTokenRouting", 1)):
                f.collect(f.with_destination(legit, f.collector_ata), reject=True, label=label)

            label = "collection refused: an extra instruction follows the wallet's SignV2"
            with f.case(label, order, refuse=registry("WalletEnvelope", 0)):
                envelope = f.wallet_bundle(legit["instruction"], action=1, caller=f.collector, generation=0)
                extra = Instruction(SYSTEM, struct.pack("<IQ", 2, 1),
                    [M(f.collector.pubkey(), True, True), M(usual_payer.pubkey(), w=True)])
                f.send(label, [*envelope, extra], reject=True, receipt=legit["receipt"])

            with f.case(accepted, order, expected=dict(seller_tokens=EARNED, lender_wallet_tokens=LENDER_CASH - EARNED,
                    collector_tokens=0, lender_promised=ORDER - EARNED, lender_free_cash=LENDER_CASH - ORDER,
                    order_paid=EARNED, order_payable=0, order_remaining=ORDER - EARNED, order_debt=EARNED)):
                f.collect(legit, label=accepted)
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
            f"{LENDER_CASH - ORDER:,} is free and {ORDER - EARNED:,} is promised")
        # The withdrawal names the lender's owner as the one who pays for its
        # receipt, so the owner must hold that rent for the program to reach
        # its cash check. A refusal spends none of it; it is returned below.
        released = f.receipt(f.company, f.operation(b"release", usual_payer.pubkey(), ff.ident("release-order-1-unused")))
        f.fund("payer gives the lender's owner lamports for one receipt", lender["owner"].pubkey(), receipt_rent,
            done=lambda: f.h.journal.landed(refused) or f.h.svm.get_account(released) is not None)
        with f.case(refused, order, refuse=core("FundingCashShortfall", 1)):
            f.withdraw(f.withdrawal("free-plus-one", LENDER_CASH - ORDER + 1), reject=True, label=refused)

        label = f"RELEASE ACCEPTED: seller releases the order's unused {ORDER - EARNED:,}"
        f.h.titles["release unused accepted work order-1-unused"] = label
        with f.case(label, order, expected=dict(lender_promised=0, lender_free_cash=LENDER_CASH - EARNED,
                lender_wallet_tokens=LENDER_CASH - EARNED, budget_held=0, budget_used=EARNED,
                order_remaining=0, order_released=ORDER - EARNED, order_debt=EARNED, order_paid=EARNED)):
            f.release(order, "order-1-unused", ORDER - EARNED)

    with f.phase("owner-pinned terms"):
        size_10 = pt.term("size", "10")
        order2 = f.acceptance("order-2", cap=PINNED_ORDER, maximum_interest=0, fees=False, index=1,
            mandate=f.pinned_mandate_address())
        price_1 = pp.unit_price_term(order2["namespace"], order2["service_unit"], PINNED_UNIT_PRICE)
        f.pin2 = pin = pp.ascending(size_10, price_1)
        f.enroll_second_agent()
        label = ("PERMISSION CREATED PINNED: buyer's owner permits agent 2 with size = 10 and a unit price of "
            f"{PINNED_UNIT_PRICE} already pinned, in one instruction (create_pinned_mandate): this seller, this lender "
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
            f.h.check("the Registry ran CreatePinnedMandate",
                "Program log: Instruction: CreatePinnedMandate" in row["logs"], True)
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
            f.accept_pinned(size_11, reject=True, label=label)

        label = ("pinned order refused (no terms shown): the same size = 11 order sent through plain accept_financed_tab, "
            "which shows no terms")
        with f.case(label, order2, refuse=registry("TermsRequired")):
            f.accept(size_11, reject=True, label=label)

        label = (f"pinned order refused (price): agent 2's order of {PINNED_ORDER:,} with size = 10 at "
            f"{REFUSED_UNIT_PRICE} per unit, listing its own unit_price, where the owner pinned {PINNED_UNIT_PRICE} per unit")
        with f.case(label, order2, refuse=registry("TermsNotPermitted")):
            f.accept_pinned(price_2, reject=True, label=label)

        label = (f"PINNED ORDER ACCEPTED: agent 2's order of {PINNED_ORDER:,} says size = 10 at {PINNED_UNIT_PRICE} per unit, "
            "the pinned values, sent through accept_pinned_financed_tab")
        with f.case(label, order2, expected=dict(budget_held=PINNED_ORDER, budget_used=EARNED, lender_promised=PINNED_ORDER,
                lender_free_cash=LENDER_CASH - EARNED - PINNED_ORDER, lender_wallet_tokens=LENDER_CASH - EARNED,
                order_remaining=PINNED_ORDER, order_payable=0, order_paid=0, order_debt=0)):
            f.accept_pinned(order2, label=label)
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
            f.h.check("the Registry reservation commits to the same terms",
                reservation["terms"]["external_terms_hash"], order2["terms"]["external_terms_hash"])
            f.h.check(f"order 2's record prices {PINNED_UNIT_PRICE} per unit of the pinned service",
                (claim["unit_price"], claim["usage_namespace"], claim["service_unit"]),
                (PINNED_UNIT_PRICE, order2["namespace"], order2["service_unit"]))

        label = f"seller reports all {PINNED_EARNED:,} of order 2 as completed work"
        with f.case(label, order2, expected=dict(order_remaining=0, order_payable=PINNED_EARNED, order_debt=PINNED_EARNED,
                order_paid=0, budget_held=0, budget_used=EARNED + PINNED_EARNED, seller_tokens=EARNED)):
            run2 = f.earn(order2, "order-2-work", PINNED_EARNED, label=label)

        pinned_collected = (f"PINNED ORDER COLLECTED: a key with no role in the order pays the seller's earned "
            f"{PINNED_EARNED:,} on order 2 from the lender's wallet")
        with f.paid_by(f.collector):
            legit2 = f.collection(order2, run2, PINNED_EARNED, paid_before=0)
        topped_up = receipt_rent + floor
        f.send("payer gives the collecting key lamports for order 2's collection receipt and its fee",
            transfer(TransferParams(from_pubkey=usual_payer.pubkey(), to_pubkey=f.collector.pubkey(),
                lamports=receipt_rent + 4 * (SIGNATURE_FEE + f.h.priority_fee))),
            effect=lambda: f.h.svm.balance(f.collector.pubkey()) >= topped_up
                or f.h.svm.get_account(legit2["receipt"]) is not None)
        with f.paid_by(f.collector):
            with f.case(pinned_collected, order2, expected=dict(seller_tokens=EARNED + PINNED_EARNED,
                    lender_wallet_tokens=LENDER_CASH - EARNED - PINNED_EARNED, lender_promised=0,
                    lender_free_cash=LENDER_CASH - EARNED - PINNED_EARNED, collector_tokens=0,
                    order_paid=PINNED_EARNED, order_payable=0, order_remaining=0, order_debt=PINNED_EARNED)):
                f.collect(legit2, label=pinned_collected)
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

    with f.phase("return lamports"):
        f.sweep("collecting key returns its remaining lamports to the payer", f.collector, after=accepted)
        f.sweep("lender's owner returns its lamports to the payer", lender["owner"], after=refused)
    return order, run


def final_state(f, order):
    """Checked against the chain on every invocation, including a resumed one."""
    journal = f.h.journal.value
    journal["assertions"] = [row for row in journal["assertions"] if row["step"] != "final state"]
    f.h.replaying, f.h.current = False, "final state"
    figures = f.figures(order)
    figures["order_2"] = f.figures(f.order2)["order"]
    paid = EARNED + PINNED_EARNED
    expected = dict(lender_wallet_tokens=LENDER_CASH - paid, lender_cash_on_record=LENDER_CASH - paid,
        lender_promised=0, lender_free_cash=LENDER_CASH - paid, lender_owner_tokens=0, seller_tokens=paid,
        collector_tokens=0, budget_held=0, budget_used=paid,
        order=dict(remaining=0, payable=0, paid=EARNED, debt=EARNED, released=ORDER - EARNED),
        order_2=dict(remaining=0, payable=0, paid=PINNED_EARNED, debt=PINNED_EARNED, released=0))
    for field, value in expected.items():
        f.h.check("final " + field, figures[field], value)
    f.h.check("final: agent 2's permission still pins size = 10 and unit_price = 1 at revision 1",
        f.pinned_permission(f.permission2), dict(revision=1, active=True, pinned=f.pin2, unused_slots_zero=True))
    f.positions("company", customer_reserved=0, customer_principal=paid)
    f.positions("supplier", supplier_committed=0, supplier_receivable=0, cash_received=paid)
    f.positions("financier", funding_reserved=0, facility_assets=paid)
    for name, key in (("collecting key", f.collector), ("lender's owner", f.parties["financier"]["owner"])):
        f.h.check(f"{name} holds no lamports after returning them", f.h.svm.balance(key.pubkey()), 0)
    return figures


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
    return idls, binaries, build


def local_binary(path):
    raw = Path(path).read_bytes()
    return dict(path=str(path), bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest(),
        sha256_without_trailing_zeros=hashlib.sha256(raw.rstrip(b"\0")).hexdigest())


def check_programs(rpc, binaries, swig_binary):
    """Hash what is deployed and compare it with the pinned build.

    The local files must hash to `PINNED_BUILD`; the chain must hold those
    bytes, allowing trailing zero padding. Without `--swig-binary` the Swig
    program is compared with the pinned hash directly.
    """
    result, mismatched = {}, []
    expected = dict(binaries)
    if swig_binary:
        expected["swig"] = swig_binary
    for name, program in (*PROGRAMS.items(), ("swig", SWIG)):
        chain = program_binary(rpc, program)
        entry = dict(chain=chain, expected=None, pinned_sha256=PINNED_BUILD[name], matches=None)
        if name in expected:
            local = local_binary(expected[name])
            entry["expected"] = local
            entry["local_file_is_pinned_build"] = local["sha256"] == PINNED_BUILD[name]
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
    parser.add_argument("--min-payer-lamports", type=int, default=600_000_000,
        help="Refuse to start a new run with less than this in the payer.")
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
    programs, mismatched = check_programs(rpc, binaries, args.swig_binary)
    for name, entry in programs.items():
        log(f"program {name:19} {entry['chain']['program']}  sha256 {entry['chain']['sha256']}  matches build: {entry['matches']}")
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
    # The fixtures read these two module names for identifiers and time.
    ff.ident, ff.NOW = run.ident, header["cluster_time_start"]

    f = PublicSequence(harness, run, idls, t0=header["cluster_time_start"], window=header["window_seconds"],
        bundle_seconds=args.bundle_seconds)
    status, failure, order = "passed", None, None
    try:
        order, _ = sequence(f)
        figures = final_state(f, order)
        cases = finalize(harness, journal.rows(), log)
    except (StepFailed, AssertionError) as error:
        status, failure, figures = "failed", str(error), None
        cases = journal.rows()
        log("FAILED: " + failure[:6000])

    after = harness.svm.balance(payer.pubkey())
    spent = None if header["payer_start_lamports"] is None else header["payer_start_lamports"] - after
    funded = {str(f.collector.pubkey()), str(f.parties["financier"]["owner"].pubkey())}
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
        "payer_lamports_delta", "figures_before", "figures_after", "adopted", "note", "problems", "logs")
    record = dict(
        schema="dexter-public-rpc-sequence/v2", status=status, failure=failure,
        cluster=dict(name=args.cluster, genesis_hash=genesis, endpoint=endpoint_label(args.rpc_url),
            node_version=version.get("solana-core"), feature_set=version.get("feature-set"),
            slot_start=header["slot_start"], slot_end=rpc.call("getSlot", [{"commitment": "finalized"}])),
        scenario=dict(order=ORDER, per_order_limit=PER_ORDER_LIMIT, budget=BUDGET, facility_limit=CREDIT_LIMIT,
            credit_line_limit=CREDIT_LIMIT, lender_cash_at_first=LENDER_START, lender_cash_added=LENDER_TOP_UP,
            earned=EARNED, released=ORDER - EARNED, token_decimals=6, interest=0, seller_fee=0,
            window_seconds=header["window_seconds"],
            pinned=dict(order=PINNED_ORDER, earned=PINNED_EARNED, pinned_terms=[["size", "10"], ["unit_price", PINNED_UNIT_PRICE]],
                refused_unit_price=REFUSED_UNIT_PRICE, pinned_list=[dict(name=t["name"].hex(), value=t["value"].hex())
                    for t in getattr(f, "pin2", [])],
                seller_terms=[list(pair) for pair in pt.SELLER_TERMS],
                refused_terms=[list(pair) for pair in pt.changed(pt.SELLER_TERMS, size="11")],
                name_fingerprint_size=pt.name_print("size").hex(),
                value_fingerprints={value: pt.value_print(value).hex() for value in ("10", "11")},
                order_terms_hash_shown={name: pt.order_terms_hash(tab["order_terms"]).hex() for name, tab in (
                    getattr(f, "order2_variants", {}).items())})),
        times=dict(started_at=header["started_at"], ended_at=utc(), cluster_time_start=header["cluster_time_start"],
            cluster_time_end=harness.svm.clock(), invocations=journal.value["invocations"],
            wall_seconds_all_invocations=round(sum(i["wall_seconds"] for i in journal.value["invocations"]), 1)),
        programs={name: dict(program_id=entry["chain"]["program"], sha256_on_chain=entry["chain"]["sha256"],
            bytes_on_chain=entry["chain"]["bytes"], loader=entry["chain"]["loader"],
            last_deployed_slot=entry["chain"].get("last_deployed_slot"),
            upgrade_authority=entry["chain"].get("upgrade_authority"), read_at_slot=entry["chain"]["read_at_slot"],
            expected_sha256=entry["expected"]["sha256"] if entry["expected"] else None, pinned_sha256=entry["pinned_sha256"],
            local_file_is_pinned_build=entry.get("local_file_is_pinned_build"), matches_expected=entry["matches"])
            for name, entry in programs.items()},
        build=dict(artifact_root=str(args.artifact_root), build_inputs_sha256=hashlib.sha256(
            json.dumps(build["inputs"], sort_keys=True).encode()).hexdigest(),
            driver_sources={name: digest(Path(__file__).resolve().parent / name) for name in (
                "public_rpc_sequence.py", "public_rpc_transport.py", "financing_fixture.py", "financing_support.py",
                "pinned_terms.py", "pinned_price.py")}),
        run=dict(salt_file=str(Path(args.salt_file).resolve()), salt_sha256=header["salt_sha256"],
            journal=str(journal.path.resolve()),
            public_keys=dict(payer=str(payer.pubkey()), agent=str(f.agent.pubkey()), agent_2=str(f.agent2.pubkey()),
                collecting_key=str(f.collector.pubkey()), token_mint=str(f.mint),
                **{ROLE[role].replace(" ", "_") + "_owner": str(f.parties[role]["owner"].pubkey()) for role in f.ROLES}),
            addresses=dict(budget=str(f.budget), agent_record=str(f.actors[0]), agent_2_record=str(f.actors[1]),
                **{ROLE[role].replace(" ", "_") + "_principal": str(f.parties[role]["principal"]) for role in f.ROLES},
                **({} if order is None else dict(permission=str(f.permission), facility=str(f.facility),
                    credit_line=str(f.line), lender_wallet=str(f.parties["financier"]["wallet"]),
                    lender_wallet_configuration=str(f.parties["financier"]["config"]),
                    lender_wallet_binding=str(f.parties["financier"]["binding"]),
                    lender_wallet_token_account=str(f.parties["financier"]["custody_token"]),
                    lender_cash_record=str(f.parties["financier"]["custody"]),
                    seller_token_account=str(f.parties["supplier"]["owner_ata"]),
                    order_record=str(order["claim"]), order_receipt=str(order["receipt"]),
                    order_reservation=str(order["reservation"]))),
                **({} if getattr(f, "order2", None) is None else dict(pinned_permission=str(f.permission2),
                    order_2_record=str(f.order2["claim"]), order_2_receipt=str(f.order2["receipt"]),
                    order_2_reservation=str(f.order2["reservation"]))))),
        transport=harness.transport,
        payer=dict(address=str(payer.pubkey()), start_lamports=header["payer_start_lamports"], end_lamports=after,
            spent_lamports=spent, spent_sol=None if spent is None else spent / 1e9,
            of_which_transaction_fees_lamports=fees,
            of_which_account_deposits_lamports=None if spent is None else spent - fees,
            sum_of_per_transaction_payer_changes=sum(row.get("payer_lamports_delta") or 0 for row in cases)),
        rpc=dict(this_invocation=usage, http_requests_all_invocations=sum(
            i["rpc"]["http_requests"] for i in journal.value["invocations"])),
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
        wall_seconds=journal.value["invocations"][-1]["wall_seconds"])))
    if status != "passed":
        sys.exit(1)


if __name__ == "__main__":
    main()
