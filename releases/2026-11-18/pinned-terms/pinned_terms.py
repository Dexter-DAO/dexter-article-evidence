#!/usr/bin/env python3
"""An order the owner did not allow never commits: owner-pinned terms.

The seller's terms are a list of fields. Each field becomes a term, a pair of
fingerprints:

    name fingerprint  = sha256("DEXTER:ORDER-TERM-NAME:1" || name)
    value fingerprint = sha256("DEXTER:ORDER-TERM-VALUE:1" || value)

An order lists its terms in ascending order of name fingerprint, so each name
appears once. The order's terms hash is over that list:

    terms hash = sha256("DEXTER:ORDER-TERMS:2" || u8(count) || name_1 || value_1 || ... || name_n || value_n)

That hash is the order's `external_contract_hash`. The agent and the seller
sign the transaction that carries it. The buyer's owner can pin up to four
terms on an agent's permission (PrincipalRegistry `pin_mandate_terms`). An
order under a pinned permission is accepted only through ClaimsCore
`accept_pinned_financed_tab`, which shows the order's full list. The Registry
checks that the names ascend strictly, that the list hashes to what was signed,
and that every pinned name is in it with the pinned value. The programs compare
32-byte values only; they never read a field name or value.

Main run: one buyer, two agents, one seller, one credit issuer and one lender.
Every order has a maximum of 100,000 at unit price 1, no interest and no seller
fee. Numbers are those of commit_legs.py: per-order limit 100,000, budget
500,000, facility and credit line 500,000, lender wallet 1,000,000.

Refusals are checked the same way as in commit_legs.py: transaction error,
first failing program frame, Anchor error name and number from that frame's
log, and byte-identical state for every writable account except the fee payer.
This module also asserts the source location in that log line against the
table `RAISED` below. Each refused order is also checked for no budget hold,
no lender promise and no order record. In the main run no account bytes are
written by the harness and the clock is never moved.

A second run on the same setup creates a permission already pinned, in one
instruction (`create_pinned_mandate`), and follows one order through later pin
changes and a revocation.

Three short runs follow, each on an existing fixture:
- passkey owners, on the setup of passkey_authority.py. That setup writes a
  disposable upgrade authority into the Registry's loader metadata from the
  harness; it writes no other account bytes and does not move the clock.
- a recoverable order (`accept_recoverable_financed_tab`), on recovery.py.
- pool-funded orders (`accept_tab` and `accept_metered_tab`), on
  integration.py. That fixture sends v0 transactions through a lookup table
  the harness writes, and its transaction builder is given the signer flags of
  the keys that sign.

The fingerprints, lists and hashes computed here are compared with
order_terms_vectors.json, which the SDK's order-terms test reads as well.

Run in a prepared checkout: ./dev anchor-smoke pinned-terms
Record: <python> foundation/onchain/tests/pinned_terms.py --record docs/evidence/<directory>
"""
from commit_legs import *
from evidence_billing import *
from passkey_authority import PasskeyFixture
from recovery import RecoveryFixture
from integration import Journey

NAME_DOMAIN = b"DEXTER:ORDER-TERM-NAME:1"
VALUE_DOMAIN = b"DEXTER:ORDER-TERM-VALUE:1"
TERMS_DOMAIN = b"DEXTER:ORDER-TERMS:2"
POLICY_PREFIX = b"DEXTER:COMPLETED-RUN-POLICY:1"
# Written here from the Registry source enum, then compared with the built interface.
REGISTRY_ERRORS = dict(OwnerAuthority=6001, ConfigurationConflict=6002, InvalidPolicy=6003,
    MandateRevoked=6007, MandateRevision=6008, TermsRequired=6051, TermsMismatch=6052, TermsNotPermitted=6053)
ANCHOR_ERRORS = dict(ConstraintSeeds=2006, AccountNotSigner=3010)
# Where each refusal is raised: the origin in the Anchor log line, written here
# from the Registry source at this commit and asserted for every refusal.
SOURCE = "thrown in programs/principal-registry/src/"
RAISED = {
    "owner account": "caused by account: owner",
    "owner signature": SOURCE + "passkey.rs:415",
    "pin list": SOURCE + "lib.rs:225",
    "pin on revoked": SOURCE + "lib.rs:230",
    "pin revision": SOURCE + "lib.rs:231",
    "created list": SOURCE + "lib.rs:275",
    "create sent again": SOURCE + "lib.rs:283",
    "order revision": SOURCE + "lib.rs:336",
    "no list shown": SOURCE + "lib.rs:352",
    "list length": SOURCE + "lib.rs:354",
    "list names": SOURCE + "lib.rs:355",
    "list hash": SOURCE + "lib.rs:356",
    "pinned term": SOURCE + "lib.rs:358",
    "Core authority": "caused by account: core_authority",
}
SHARED_VECTORS = Path(__file__).resolve().parent / "order_terms_vectors.json"


def refusal(name, where, **extra):
    """What a refusal must show: raising program, error name and number, and where it is raised."""
    return dict(program=REGISTRY, name=name, code=REGISTRY_ERRORS.get(name) or ANCHOR_ERRORS[name],
        source=RAISED[where], **extra)


class SourceChecked:
    """commit_legs.py's refusal check, plus the source location in the failing frame's log line."""
    def refused(self, row, *, source, **expected):
        CommitLegs.refused(self, row, **expected)
        row["passed"] = False
        found = row["commit_legs"]["refusal"]["anchor_error"]
        assert found is not None and found["origin"] == source, (row["name"], "raised at", found, "expected", source)
        row["passed"] = True
SELLER_TERMS = (("item", "trail shoe"), ("size", "10"), ("colour", "red"), ("ship_to", "warehouse 4"))
SIXTEEN_TERMS = SELLER_TERMS + (("quantity", "1 pair"), ("width", "regular"), ("brand", "example"),
    ("material", "mesh"), ("sole", "rubber"), ("laces", "black"), ("box", "recycled"), ("carrier", "ground"),
    ("deliver_by", "2026-12-01"), ("returns", "30 days"), ("warranty", "1 year"), ("invoice", "A-1001"))


def name_print(name):
    return hashlib.sha256(NAME_DOMAIN + name.encode()).digest()


def value_print(value):
    return hashlib.sha256(VALUE_DOMAIN + value.encode()).digest()


def term(name, value):
    """One term of the seller's terms: the fingerprints of a field's name and value."""
    return dict(name=name_print(name), value=value_print(value))


def listing(pairs):
    """The terms of (name, value) pairs in ascending order of name fingerprint."""
    return sorted((term(name, value) for name, value in pairs), key=lambda item: item["name"])


def as_terms(given):
    """Pairs are listed in ascending order; a prepared list of terms is used exactly as given."""
    given = list(given)
    return given if given and isinstance(given[0], dict) else listing(given)


def order_terms_hash(terms):
    """Hash of a list of terms, whatever its length or order."""
    return hashlib.sha256(TERMS_DOMAIN + bytes([len(terms)])
        + b"".join(item["name"] + item["value"] for item in terms)).digest()


def changed(pairs, **replacements):
    return tuple((name, replacements.get(name, value)) for name, value in pairs)


def without(pairs, name):
    return tuple(pair for pair in pairs if pair[0] != name)


def pin_instruction(registry, owner, principal, permission, required, expected_revision):
    return registry.instruction("pin_mandate_terms", dict(owner=owner, principal=principal, mandate=permission),
        expected_revision=expected_revision, required_terms=required)


def stored_pin(registry, svm, permission):
    state = registry.read(svm, permission, "Mandate")
    count = state["required_terms_count"]
    assert all(item == dict(name=bytes(32), value=bytes(32)) for item in state["required_terms"][count:]), \
        "unused pin slots must be zero"
    return dict(revision=state["revision"], pinned=state["required_terms"][:count], active=state["active"])


def absent(svm, address):
    account = svm.get_account(address)
    return account is None or (account.lamports == 0 and not account.data)


class PinnedTerms(SourceChecked, CommitLegs, EvidenceBillingFixture):
    def __init__(self):
        super().__init__()
        self.outsider = self.impostor
        for signer in (self.agent, self.outsider):
            self.h.svm.airdrop(signer.pubkey(), 100_000_000_000)

    def mark(self, case, kind, what_changed, *, refuse=None, **extra):
        return self.note(section=case, leg=kind, changed=what_changed, refuse=refuse, **extra)

    # -- permission --------------------------------------------------------
    def permission_state(self, permission):
        return self.registry.read(self.h.svm, permission, "Mandate")

    def pinned(self, permission):
        return stored_pin(self.registry, self.h.svm, permission)

    def pin_instruction(self, permission, required, expected_revision, *, owner=None):
        return pin_instruction(self.registry, self.parties["company"]["owner"].pubkey() if owner is None else owner,
            self.company, permission, required, expected_revision)

    def pin(self, permission, required, label, *, case, what_changed="nothing", expected_revision=None,
            owner=None, signers=None, refuse=None):
        if expected_revision is None:
            expected_revision = self.permission_state(permission)["revision"]
        self.send(label, self.pin_instruction(permission, required, expected_revision, owner=owner),
            [self.parties["company"]["owner"]] if signers is None else signers, reject=refuse is not None)
        return self.mark(case, "pin", what_changed, refuse=refuse)

    def create_pinned(self, name, index, required, label, *, case, what_changed="nothing", refuse=None):
        """create_pinned_mandate: the permission and its pinned terms in one instruction."""
        buyer = self.parties["company"]
        mandate_id = ident("mandate-" + name)
        permission = pda(REGISTRY, b"mandate", self.company, mandate_id)
        self.send(label, self.ix("principal_registry", "create_pinned_mandate",
            dict(owner=buyer["owner"].pubkey(), principal=self.company, actor=self.actors[index],
                budget=self.budget, mandate=permission),
            args=dict(mandate_id=mandate_id, supplier=self.parties["supplier"]["owner"].pubkey(),
                source=self.facility, per_operation_limit=PER_ORDER_LIMIT, expires_at=NOW + PERMISSION_SECONDS),
            required_terms=required), [buyer["owner"]], reject=refuse is not None)
        self.mark(case, "create pinned", what_changed, refuse=refuse)
        return permission

    def recreate(self, name, index, permission, label, *, case, what_changed, refuse=None):
        """The permission's original create_mandate, sent again unchanged."""
        buyer = self.parties["company"]
        self.send(label, self.ix("principal_registry", "create_mandate",
            dict(owner=buyer["owner"].pubkey(), principal=self.company, actor=self.actors[index],
                budget=self.budget, mandate=permission),
            args=dict(mandate_id=ident("mandate-" + name), supplier=self.parties["supplier"]["owner"].pubkey(),
                source=self.facility, per_operation_limit=PER_ORDER_LIMIT, expires_at=NOW + PERMISSION_SECONDS)),
            [buyer["owner"]], reject=refuse is not None)
        return self.mark(case, "setup sent again", what_changed, refuse=refuse)

    # -- orders ------------------------------------------------------------
    def set_terms(self, tab, signed, shown=None):
        """`signed` is the list the order's hash commits to; `shown` is the list sent with the order."""
        contract = order_terms_hash(as_terms(signed))
        tab["external_contract_hash"] = tab["args"]["external_contract_hash"] = contract
        tab["terms"]["external_terms_hash"] = hashlib.sha256(POLICY_PREFIX + tab["namespace"]
            + tab["service_unit"] + u64(tab["unit_price"]) + contract).digest()
        tab["order_terms"] = as_terms(signed if shown is None else shown)
        return tab

    def terms_order(self, name, pairs, *, revision, index=0, permission=None):
        tab = self.acceptance(name, cap=ORDER, maximum_interest=0, fees=False, index=index,
            mandate=self.permission if permission is None else permission)
        tab["terms"].update(expires_at=NOW + PERMISSION_SECONDS, mandate_revision=revision)
        return self.set_terms(tab, pairs)

    def roles_of(self, tab):
        return self.remaining(["company", "issuer", "supplier", tab["financier"]])

    def place(self, tab, label, *, case, what_changed="nothing", shown=True, refuse=None):
        """`shown=True` sends accept_pinned_financed_tab with the list; `shown=False` sends accept_financed_tab."""
        if shown:
            instruction = self.ix("claims_core", "accept_pinned_financed_tab", tab["accounts"],
                remaining=self.roles_of(tab), order_terms=tab["order_terms"], **tab["args"])
        else:
            instruction = self.commit_instruction(tab)
        before = self.money()
        self.send(label, instruction, tab["signers"], reject=refuse is not None, receipt=tab["receipt"])
        row = self.mark(case, "order, list shown" if shown else "order, no list shown", what_changed, refuse=refuse)
        if refuse is not None:
            self.nothing_moved(tab, label, before)
        return row

    def place_evidence(self, tab, label, *, case, what_changed, refuse):
        instruction = self.ix("claims_core", "accept_evidence_financed_tab", tab["evidence_accounts"],
            remaining=self.roles_of(tab), **tab["evidence_args"])
        before = self.money()
        self.send(label, instruction, tab["signers"], reject=True, receipt=tab["receipt"])
        row = self.mark(case, "evidence order, no list shown", what_changed, refuse=refuse)
        self.nothing_moved(tab, label, before)
        return row

    def money(self):
        budget = self.registry.read(self.h.svm, self.budget, "Budget")
        cash = self.lender_cash()
        return dict(budget_held=budget["reserved"], budget_consumed=budget["consumed"],
            lender_promised=cash["promised"], lender_free=cash["free"], lender_wallet=cash["wallet"],
            facility_promised=self.state("FundingFacility", self.facility)["reserved"],
            credit_line_promised=self.state("CustomerCreditLine", self.line)["reserved"])

    def nothing_moved(self, tab, label, before):
        self.h.check("nothing moved (budget hold, lender promise, facility, credit line, wallet): " + label,
            self.money(), before)
        for name, address in (("order record", tab["claim"]), ("Registry reservation", tab["reservation"]),
                ("Registry operation record", tab["accounts"]["registry_operation"]), ("receipt", tab["receipt"])):
            self.h.check(f"no {name} exists: " + label, absent(self.h.svm, address), True)

    def accepted(self, tab, row, number, *, shown, held):
        """State after an accepted order; `held` is the budget hold expected afterwards."""
        logs = row["result"]["logs"]
        core_name = "AcceptPinnedFinancedTab" if shown else "AcceptFinancedTab"
        registry_name = "ReservePinned" if shown else "Reserve"
        self.h.check(f"order {number}: Core ran {core_name}", "Program log: Instruction: " + core_name in logs, True)
        self.h.check(f"order {number}: the Registry ran {registry_name} inside the commit",
            "Program log: Instruction: " + registry_name in logs, True)
        claim = self.state("FundedClaim", tab["claim"])
        self.h.check(f"order {number}: the order record stores the terms hash both sides signed",
            claim["external_contract_hash"], tab["external_contract_hash"])
        self.h.check(f"order {number}: order record maximum", claim["remaining"], ORDER)
        reservation = self.registry.read(self.h.svm, tab["reservation"], "Reservation")
        self.h.check(f"order {number}: the Registry reservation commits to the same terms",
            reservation["terms"]["external_terms_hash"], tab["terms"]["external_terms_hash"])
        self.h.check(f"order {number}: the Registry reservation holds the order maximum", reservation["remaining"], ORDER)
        money = self.money()
        self.h.check(f"order {number}: budget hold afterwards", money["budget_held"], held)
        self.h.check(f"order {number}: lender promise afterwards", money["lender_promised"], held)
        self.h.check(f"order {number}: no tokens left the lender wallet", money["lender_wallet"], LENDER_CASH)
        signers = self.signers_of(row)
        self.h.check(f"order {number}: the buyer's owner does not sign the order",
            str(self.parties["company"]["owner"].pubkey()) in signers, False)

    def signers_of(self, row):
        tx = VersionedTransaction.from_bytes(__import__("base64").b64decode(row["transaction_base64"]))
        return [str(k) for k in tx.message.account_keys[:len(tx.signatures)]]


def interface(f):
    names = {error["name"]: error["code"] for error in f.registry.value["errors"]}
    f.h.check("error numbers written in this module equal the built Registry interface",
        {name: names.get(name) for name in REGISTRY_ERRORS}, REGISTRY_ERRORS)
    fields = [field["name"] for field in f.registry.types["Mandate"]["fields"]]
    f.h.check("the permission record ends with the pinned-terms count and list",
        fields[-2:], ["required_terms_count", "required_terms"])
    f.h.check("a term is a name fingerprint and a value fingerprint",
        [(field["name"], field["type"]) for field in f.registry.types["Term"]["fields"]],
        [("name", {"array": ["u8", 32]}), ("value", {"array": ["u8", 32]})])
    f.h.check("Core's pinned order takes the plain order's arguments plus the list",
        [a["name"] for a in f.core.instructions["accept_pinned_financed_tab"]["args"]],
        [a["name"] for a in f.core.instructions["accept_financed_tab"]["args"]] + ["order_terms"])
    f.h.check("Core's pinned order takes the plain order's accounts",
        f.core.instructions["accept_pinned_financed_tab"]["accounts"], f.core.instructions["accept_financed_tab"]["accounts"])
    f.h.check("the Registry's pinned reservation takes the plain reservation's accounts",
        f.registry.instructions["reserve_pinned"]["accounts"], f.registry.instructions["reserve"]["accounts"])
    f.h.check("pinning takes the accounts of revoking a permission",
        f.registry.instructions["pin_mandate_terms"]["accounts"], f.registry.instructions["revoke_mandate"]["accounts"])
    f.h.check("size = 10 and size = 11 have the same name fingerprint and different value fingerprints",
        (term("size", "10")["name"] == term("size", "11")["name"], term("size", "10")["value"] == term("size", "11")["value"]),
        (True, False))
    f.h.check("a name and a value of the same text have different fingerprints", name_print("10") == value_print("10"), False)
    names = [item["name"] for item in listing(SIXTEEN_TERMS)]
    f.h.check("a listing ascends strictly by name fingerprint", all(a < b for a, b in zip(names, names[1:])), True)
    shared = json.loads(SHARED_VECTORS.read_text())
    f.h.check("the shared vectors name the domains this module uses",
        [shared[key].encode() for key in ("name_domain", "value_domain", "terms_domain")],
        [NAME_DOMAIN, VALUE_DOMAIN, TERMS_DOMAIN])
    for vector in shared["vectors"]:
        terms = listing(tuple(tuple(entry) for entry in vector["entries"]))
        f.h.check("shared vector, list of terms: " + vector["name"],
            [dict(name=item["name"].hex(), value=item["value"].hex()) for item in terms], vector["terms"])
        f.h.check("shared vector, order terms hash: " + vector["name"], order_terms_hash(terms).hex(), vector["hash"])
    repeated = [item["name"] for item in listing(tuple(tuple(entry) for entry in shared["repeated_name"]["entries"]))]
    f.h.check("the shared repeated-name entries do not ascend strictly",
        all(a < b for a, b in zip(repeated, repeated[1:])), False)


def cast(f):
    parties = f.parties
    f.roles = {"buyer's owner": parties["company"]["owner"], "agent 1": f.agent, "agent 2": f.agent2,
        "seller": parties["supplier"]["owner"], "lender's owner": parties["financier"]["owner"],
        "credit issuer's owner": parties["issuer"]["owner"], "fee payer of setup and order transactions": f.h.payer,
        "outside key (refusals)": f.outsider}
    keys = [str(holder.pubkey()) for holder in f.roles.values()]
    f.h.check("all eight roles are held by different keys", len(set(keys)), len(keys))


def pin_size(f, order_1):
    """Case 1 and the refusals that share its instruction."""
    size_10, size_11, red = term("size", "10"), term("size", "11"), term("colour", "red")
    before = f.permission_state(f.permission)
    f.h.check("agent 1's permission before the pin: nothing pinned, revision 1",
        f.pinned(f.permission), dict(revision=1, pinned=[], active=True))
    row = f.pin(f.permission, [size_10], "ACCEPTED: the buyer's owner pins size = 10 on agent 1's permission", case="1")
    after = f.permission_state(f.permission)
    f.h.check("agent 1's permission after the pin: the stored list is the term size = 10, revision 2",
        f.pinned(f.permission), dict(revision=2, pinned=[size_10], active=True))
    f.h.check("the pin changed nothing else on the permission",
        {k: v for k, v in after.items() if k not in ("revision", "required_terms_count", "required_terms")},
        {k: v for k, v in before.items() if k not in ("revision", "required_terms_count", "required_terms")})
    f.h.check("the pin is signed by the fee payer and the buyer's owner only", sorted(f.signers_of(row)),
        sorted([str(f.h.payer.pubkey()), str(f.parties["company"]["owner"].pubkey())]))
    row = f.pin(f.permission, [size_10], "no effect: the same pin (expected revision 1, size = 10) is sent again",
        case="1", what_changed="the pin is sent a second time", expected_revision=1)
    f.h.check("sending the pin again changes no observed account", row["observed_after"], row["observed_before"])
    f.h.check("agent 1's permission is still at revision 2", f.pinned(f.permission)["revision"], 2)

    # A replayed create never reports a pinned permission as freshly created, and never clears its pins.
    f.recreate("agent-1", 0, f.permission, "refused: the original setup of agent 1's permission is sent again after the pin",
        case="1", what_changed="the permission was pinned since it was created",
        refuse=refusal("ConfigurationConflict", "create sent again"))
    f.h.check("agent 1's permission keeps its pin after the refused setup",
        f.pinned(f.permission), dict(revision=2, pinned=[size_10], active=True))
    row = f.recreate("agent-2", 1, f.second, "no effect: the original setup of agent 2's unpinned permission is sent again",
        case="1", what_changed="nothing")
    f.h.check("sending agent 2's setup again changes no observed account", row["observed_after"], row["observed_before"])

    # Case 9: only the buyer's owner can pin or unpin.
    owner = f.parties["company"]["owner"].pubkey()
    constraint, handler = refusal("OwnerAuthority", "owner account"), refusal("OwnerAuthority", "owner signature")
    for holder, who in ((f.agent, "agent 1"), (f.outsider, "a key that is not the owner")):
        with f.paid_by(holder):
            f.pin(f.permission, [], f"refused: {who} unpins agent 1's permission, naming itself as the owner",
                case="9", what_changed="pin instruction signed by a key that is not the buyer's owner, in the owner's place",
                owner=holder.pubkey(), signers=[holder], refuse=constraint)
            f.pin(f.permission, [size_11], f"refused: {who} changes the pin to size = 11, naming the real owner without its signature",
                case="9", what_changed="buyer's owner signature absent", signers=[], refuse=handler)
            f.pin(f.permission, [], f"refused: {who} unpins agent 1's permission, naming the real owner without its signature",
                case="9", what_changed="buyer's owner signature absent", signers=[], refuse=handler)
    f.h.check("agent 1's permission is unchanged after the six attempts",
        f.pinned(f.permission), dict(revision=2, pinned=[size_10], active=True))

    # What a pin may contain. The spare permission belongs to agent 2 and is never used for an order.
    four = listing(SELLER_TERMS)
    f.pin(f.spare, four, "ACCEPTED: the buyer's owner pins four terms on a spare permission, the most a permission can hold",
        case="what a pin holds")
    f.h.check("the spare permission stores the four terms in ascending order of name",
        f.pinned(f.spare), dict(revision=2, pinned=four, active=True))
    two = listing((("size", "10"), ("colour", "red")))
    for required, what in ((listing(SELLER_TERMS + (("note", "gift"),)), "five terms"),
            ([size_10, size_11], "the field size twice, as 10 and as 11"),
            ([size_10, size_10], "the same term twice"),
            (two[::-1], "two terms with their names out of ascending order"),
            ([dict(name=bytes(32), value=size_10["value"])], "a term whose name is all zeros")):
        f.pin(f.spare, required, f"refused: the buyer's owner pins {what} on the spare permission",
            case="what a pin holds", what_changed="pinned list: " + what, refuse=refusal("InvalidPolicy", "pin list"))
    f.pin(f.spare, [size_10], "refused: the buyer's owner pins size = 10 on the spare permission against revision 1; it is at revision 2",
        case="what a pin holds", what_changed="expected revision is one behind", expected_revision=1,
        refuse=refusal("MandateRevision", "pin revision"))
    f.h.check("the spare permission is unchanged after the six refusals",
        f.pinned(f.spare), dict(revision=2, pinned=four, active=True))
    revoke = f.ix("principal_registry", "revoke_mandate",
        dict(owner=owner, principal=f.company, mandate=f.spare), expected_revision=2)
    f.send("the buyer's owner revokes the spare permission", revoke, [f.parties["company"]["owner"]])
    f.mark("what a pin holds", "revoke", "nothing")
    f.pin(f.spare, [size_10], "refused: the buyer's owner pins size = 10 on the revoked spare permission",
        case="what a pin holds", what_changed="the permission was revoked", refuse=refusal("MandateRevoked", "pin on revoked"))
    f.send("no effect: the revocation of the spare permission is sent again", revoke, [f.parties["company"]["owner"]])
    row = f.mark("what a pin holds", "revoke", "the revocation is sent a second time")
    f.h.check("sending the revocation again changes no observed account", row["observed_after"], row["observed_before"])
    f.h.check("the revoked spare permission keeps its revision and its list",
        f.pinned(f.spare), dict(revision=3, pinned=four, active=False))


def size_refusals_then_acceptance(f, order_1, prepared_before_pin):
    not_permitted, mismatch = refusal("TermsNotPermitted", "pinned term"), refusal("TermsMismatch", "list hash")
    names = refusal("TermsMismatch", "list names")

    v = f.set_terms(variant(order_1), changed(SELLER_TERMS, size="11"))
    f.place(v, "refused: agent 1 and the seller submit order 1 with terms that say size = 11; the owner pinned size = 10",
        case="2", what_changed="size in the seller's terms: 11 instead of 10", refuse=not_permitted)
    v = f.set_terms(variant(order_1), without(SELLER_TERMS, "size"))
    f.place(v, "refused: order 1 with terms that have no size field at all",
        case="4", what_changed="size field absent from the seller's terms", refuse=not_permitted)
    v = f.set_terms(variant(order_1), changed(SELLER_TERMS, size="11"), shown=SELLER_TERMS)
    f.place(v, "refused: order 1's signed terms say size = 11; the agent shows a list that says size = 10",
        case="5", what_changed="list shown is not the list the signed terms hash to", refuse=mismatch)
    v = f.set_terms(variant(order_1), SELLER_TERMS, shown=())
    f.place(v, "refused: order 1 with size = 10 is sent through the pinned instruction with an empty list",
        case="5", what_changed="list shown is empty", refuse=refusal("TermsMismatch", "list length"))
    twice = listing(SELLER_TERMS + (("size", "11"),))
    f.h.check("the list that names size twice holds size = 10, which the owner pinned, and size = 11",
        (term("size", "10") in twice, term("size", "11") in twice, len(twice)), (True, True, 5))
    v = f.set_terms(variant(order_1), twice)
    f.place(v, "refused: order 1 with a list that names size twice, as size = 10 and as size = 11; a list may name a field once",
        case="13", what_changed="size named twice in the seller's terms: 10 and 11", refuse=names)
    v = f.set_terms(variant(order_1), listing(SELLER_TERMS)[::-1])
    f.place(v, "refused: order 1 with size = 10 and its four names in descending order; names must ascend",
        case="14", what_changed="order of the names in the list", refuse=names)
    f.place(order_1, "refused: order 1 with size = 10 is sent through plain accept_financed_tab, which shows no list",
        case="6", what_changed="plain instruction, no list shown", shown=False, refuse=refusal("TermsRequired", "no list shown"))
    f.h.check("the order prepared before the pin names revision 1", prepared_before_pin["terms"]["mandate_revision"], 1)
    f.place(prepared_before_pin, "refused: order 1 with size = 10, prepared against the permission's revision 1 from before the pin",
        case="8", what_changed="permission revision named by the order: 1 instead of 2",
        refuse=refusal("MandateRevision", "order revision"))

    row = f.place(order_1, "ACCEPTED: agent 1 and the seller submit order 1, maximum 100,000, with terms that say size = 10",
        case="3")
    f.accepted(order_1, row, 1, shown=True, held=ORDER)
    f.h.check("order 1: the list shown hashes to the stored terms hash",
        order_terms_hash(order_1["order_terms"]), f.state("FundedClaim", order_1["claim"])["external_contract_hash"])
    f.h.check("order 1: the list shown contains the pinned term", term("size", "10") in order_1["order_terms"], True)
    row = f.place(order_1, "no effect: accepted order 1 is sent again; the program returns the original outcome and changes nothing",
        case="3", what_changed="an already accepted order is sent again")
    f.h.check("re-sending accepted order 1 changes no observed account", row["observed_after"], row["observed_before"])


def unpinned_agent(f):
    seller = f.parties["supplier"]
    f.h.check("agent 2's permission: nothing pinned, revision 1",
        f.pinned(f.second), dict(revision=1, pinned=[], active=True))
    size_11 = changed(SELLER_TERMS, size="11")
    order_2 = f.terms_order("order-2", size_11, revision=1, index=1, permission=f.second)
    row = f.place(order_2, "ACCEPTED: agent 2, whose permission the owner did not pin, submits order 2 with size = 11 through plain accept_financed_tab",
        case="7", shown=False)
    f.accepted(order_2, row, 2, shown=False, held=2 * ORDER)
    order_5 = f.terms_order("order-5", size_11, revision=1, index=1, permission=f.second)
    v = f.set_terms(variant(order_5), size_11, shown=SELLER_TERMS)
    f.place(v, "refused: agent 2 sends order 5 (size = 11) through the pinned instruction with a list that says size = 10",
        case="7", what_changed="list shown is not the list the signed terms hash to",
        refuse=refusal("TermsMismatch", "list hash"))
    row = f.place(order_5, "ACCEPTED: agent 2 submits order 5 with size = 11 through the pinned instruction; its permission pins nothing",
        case="7")
    f.accepted(order_5, row, 5, shown=True, held=3 * ORDER)
    f.release(order_5, "order-5-unused", ORDER)
    f.h.cases[-1]["name"] = "the seller releases order 5's unused 100,000, so the scenario stays inside the 500,000 budget"
    f.mark("7", "release", "nothing")
    f.h.check("budget hold after order 5's release", f.money()["budget_held"], 2 * ORDER)
    f.h.check("the release was signed by the fee payer and the seller only", sorted(f.signers_of(f.h.cases[-1])),
        sorted([str(f.h.payer.pubkey()), str(seller["owner"].pubkey())]))


def two_pins(f):
    both = listing((("size", "10"), ("colour", "red")))
    f.pin(f.permission, both, "ACCEPTED: the buyer's owner pins size = 10 and colour = red on agent 1's permission",
        case="10")
    f.h.check("agent 1's permission stores both terms, revision 3",
        f.pinned(f.permission), dict(revision=3, pinned=both, active=True))
    order_3 = f.terms_order("order-3", SIXTEEN_TERMS, revision=3)
    f.h.check("order 3 carries sixteen terms, the most an order can show", len(order_3["order_terms"]), 16)
    v = f.set_terms(variant(order_3), changed(SIXTEEN_TERMS, colour="blue"))
    f.place(v, "refused: order 3 with size = 10 and colour = blue; the owner pinned size = 10 and colour = red",
        case="10", what_changed="colour in the seller's terms: blue instead of red",
        refuse=refusal("TermsNotPermitted", "pinned term"))
    v = f.set_terms(variant(order_3), SIXTEEN_TERMS + (("note", "gift"),))
    f.h.check("the oversized variant carries seventeen terms", len(v["order_terms"]), 17)
    f.place(v, "refused: order 3 with a seventeenth term added to its sixteen",
        case="10", what_changed="seventeen terms in the list, one more than an order can show",
        refuse=refusal("TermsMismatch", "list length"))
    row = f.place(order_3, "ACCEPTED: order 3, maximum 100,000, with size = 10 and colour = red among its sixteen terms",
        case="10")
    f.accepted(order_3, row, 3, shown=True, held=3 * ORDER)


def paths_without_a_list(f):
    """Cases 12 and 6 again while two terms are pinned; both are accepted after the unpin."""
    required = refusal("TermsRequired", "no list shown")
    f.mandates[0] = f.permission
    evidence = f.evidence_tab("order-6-evidence", cap=ORDER, rates=((1, 1, ORDER),))
    evidence["terms"]["mandate_revision"] = 3
    f.h.check("the evidence order names agent 1's pinned permission", evidence["terms"]["mandate"], f.permission)
    f.place_evidence(evidence, "refused: order 6, an evidence order (accept_evidence_financed_tab) of 100,000, on agent 1's pinned permission",
        case="12", what_changed="the permission is pinned and this instruction shows no list", refuse=required)
    order_4 = f.terms_order("order-4", changed(SELLER_TERMS, size="11"), revision=3)
    f.place(order_4, "refused: order 4 with size = 11 through plain accept_financed_tab while agent 1's permission is pinned",
        case="6", what_changed="the permission is pinned and this instruction shows no list", shown=False, refuse=required)

    f.pin(f.permission, [], "ACCEPTED: the buyer's owner unpins agent 1's permission (empty list)", case="11")
    f.h.check("agent 1's permission after the unpin: nothing pinned, revision 4",
        f.pinned(f.permission), dict(revision=4, pinned=[], active=True))
    order_4["terms"]["mandate_revision"] = 4
    row = f.place(order_4, "ACCEPTED: after the unpin, order 4 with size = 11 through plain accept_financed_tab",
        case="11", shown=False)
    f.accepted(order_4, row, 4, shown=False, held=4 * ORDER)
    evidence["terms"]["mandate_revision"] = 4
    f.accept_evidence(evidence, label="ACCEPTED: after the unpin, order 6, the evidence order of 100,000")
    f.mark("12", "evidence order, no list shown", "nothing")
    return evidence


def end_state(f):
    money = f.money()
    f.h.check("budget at the end: five orders of 100,000 held, nothing consumed",
        (money["budget_held"], money["budget_consumed"]), (5 * ORDER, 0))
    f.h.check("lender promise at the end", money["lender_promised"], 5 * ORDER)
    f.h.check("no tokens left the lender wallet in this scenario", money["lender_wallet"], LENDER_CASH)
    f.h.check("agent 2's permission was never pinned", f.pinned(f.second), dict(revision=1, pinned=[], active=True))
    refused = [row for row in f.h.cases if row["expected"] == "reject"]
    f.h.check("every refusal in this run was annotated and checked",
        all(row.get("commit_legs", {}).get("outcome") == "refused" for row in refused), True)


def exercise(f):
    f.scenario()
    interface(f)
    cast(f)
    f.second = f.permit("agent-2", 1)
    f.spare = f.permit("spare", 1)
    f.h.cases[-1]["name"] = "buyer's owner creates a spare permission for agent 2, used only to show what a pin may contain"
    order_1 = f.terms_order("order-1", SELLER_TERMS, revision=1)
    prepared_before_pin = variant(order_1)
    pin_size(f, order_1)
    order_1["terms"]["mandate_revision"] = 2
    size_refusals_then_acceptance(f, order_1, prepared_before_pin)
    unpinned_agent(f)
    two_pins(f)
    paths_without_a_list(f)
    end_state(f)


class CreatedPinnedTerms(PinnedTerms):
    """A permission created already pinned, and one order followed through later changes to the pin."""


def created_pinned(f):
    buyer, seller, lender = [f.parties[x] for x in ("company", "supplier", "financier")]
    f.setup_principals()
    for role, amount in (("protocol", 0), ("fee", 0), ("issuer", 200_000)):
        f.create_custody(role, amount)
    f.create_custody("financier", LENDER_CASH, wallet=True)
    f.open_fee_agreement("main", 2_100)
    f.facility = f.open_facility()
    f.line, f.assignment_policy = f.open_line(f.facility)
    cast(f)
    size_10, size_11, size_12 = term("size", "10"), term("size", "11"), term("size", "12")
    conflict = refusal("ConfigurationConflict", "create sent again")
    invalid = refusal("InvalidPolicy", "created list")
    not_permitted = refusal("TermsNotPermitted", "pinned term")

    # Case 17: the permission is created with its pin, so it is never usable without it.
    f.permission = f.create_pinned("agent-1", 0, [size_10],
        "ACCEPTED: the buyer's owner creates agent 1's permission already pinned to size = 10, in one instruction", case="17")
    row = f.h.cases[-1]
    f.h.check("the permission exists at revision 1 with the term size = 10",
        f.pinned(f.permission), dict(revision=1, pinned=[size_10], active=True))
    f.h.check("creating it pinned is one instruction", len(row["instructions"]), 1)
    f.h.check("the Registry ran CreatePinnedMandate", "Program log: Instruction: CreatePinnedMandate" in row["result"]["logs"], True)
    f.h.check("it is signed by the fee payer and the buyer's owner only", sorted(f.signers_of(row)),
        sorted([str(f.h.payer.pubkey()), str(buyer["owner"].pubkey())]))
    order_1 = f.terms_order("order-1", SELLER_TERMS, revision=1)
    wrong = f.set_terms(variant(order_1), changed(SELLER_TERMS, size="11"))
    f.place(wrong, "refused: straight after the permission is created, agent 1 and the seller submit order 1 with size = 11 "
        "through plain accept_financed_tab", case="17", what_changed="size = 11, plain instruction, no list shown",
        shown=False, refuse=refusal("TermsRequired", "no list shown"))
    f.place(wrong, "refused: order 1 with size = 11 through the pinned instruction",
        case="17", what_changed="size in the seller's terms: 11 instead of 10", refuse=not_permitted)
    row = f.place(order_1, "ACCEPTED: order 1, maximum 100,000, with size = 10 through the pinned instruction", case="17")
    f.accepted(order_1, row, 1, shown=True, held=ORDER)

    # Case 18: sending the creation again.
    row_before = f.pinned(f.permission)
    f.create_pinned("agent-1", 0, [size_10], "no effect: the same creation (size = 10) is sent again",
        case="18", what_changed="the creation is sent a second time")
    row = f.h.cases[-1]
    f.h.check("sending the creation again changes no observed account", row["observed_after"], row["observed_before"])
    f.recreate("agent-1", 0, f.permission, "refused: plain create_mandate is sent for the permission that was created pinned",
        case="18", what_changed="the creation is sent without its pinned terms", refuse=conflict)
    f.create_pinned("agent-1", 0, [size_11], "refused: the creation is sent again with size = 11 in place of size = 10",
        case="18", what_changed="pinned terms in the creation: size = 11", refuse=conflict)
    f.h.check("the permission is unchanged after the three re-sends", f.pinned(f.permission), row_before)
    for required, what in (([], "no terms"), ([size_10, size_11], "the field size twice, as 10 and as 11"),
            (listing(SELLER_TERMS + (("note", "gift"),)), "five terms")):
        name = "never-created-" + str(len(required))
        f.create_pinned(name, 0, required, f"refused: the buyer's owner creates a pinned permission with {what}",
            case="18", what_changed="pinned list at creation: " + what, refuse=invalid)
        f.h.check(f"no permission exists after the creation with {what}",
            absent(f.h.svm, pda(REGISTRY, b"mandate", f.company, ident("mandate-" + name))), True)

    # Case 21: a pinned value of all zeros is a value like any other.
    zero = dict(name=name_print("size"), value=bytes(32))
    f.second = f.create_pinned("agent-2", 1, [zero],
        "ACCEPTED: the buyer's owner creates agent 2's permission pinned to size with an all-zero value fingerprint", case="21")
    f.h.check("agent 2's permission stores the all-zero value", f.pinned(f.second), dict(revision=1, pinned=[zero], active=True))
    order_2 = f.terms_order("order-2", SELLER_TERMS, revision=1, index=1, permission=f.second)
    f.place(order_2, "refused: agent 2 submits order 2 with size = 10; its permission pins the all-zero value",
        case="21", what_changed="size value in the seller's terms: 10, not the pinned all-zero value", refuse=not_permitted)
    with_zero = [zero if item["name"] == zero["name"] else item for item in listing(SELLER_TERMS)]
    row = f.place(f.set_terms(order_2, with_zero), "ACCEPTED: order 2, maximum 100,000, whose size term carries the all-zero value",
        case="21")
    f.accepted(order_2, row, 2, shown=True, held=2 * ORDER)

    # Case 22: the Registry's pinned reservation called directly, without ClaimsCore.
    order_3 = f.terms_order("order-3", SELLER_TERMS, revision=1)
    info = f.registry.instructions["reserve_pinned"]
    data = bytes(info["discriminator"]) + b"".join(f.registry.encode(a["type"], value) for a, value in zip(info["args"], (
        dict(operation_id=f.operation(b"accept", f.agent.pubkey(), order_3["args"]["request_id"]), terms=order_3["terms"]),
        order_3["namespace"], order_3["service_unit"], order_3["unit_price"], order_3["order_terms"])))
    f.h.check("the direct call carries the five arguments of reserve_pinned",
        [a["name"] for a in info["args"]], ["args", "namespace", "unit", "unit_price", "order_terms"])
    for authority, signed, name, what in ((f.registry_authority, False, "AccountNotSigner", "ClaimsCore's authority address, which no key can sign for"),
            (f.outsider.pubkey(), True, "ConstraintSeeds", "its own key in place of ClaimsCore's authority")):
        accounts = order_3["accounts"]
        metas = [M(authority, signed), M(f.h.payer.pubkey(), True, True), M(f.company), M(accounts["actor"]),
            M(f.agent.pubkey(), True), M(accounts["mandate"]), M(f.budget, w=True), M(order_3["reservation"], w=True),
            M(accounts["registry_operation"], w=True), M(SYSTEM)]
        before = f.money()
        f.send(f"refused: a direct call to the Registry's reserve_pinned for order 3 (size = 10), naming {what}",
            Instruction(REGISTRY, data, metas), [f.agent, f.outsider] if signed else [f.agent], reject=True)
        f.mark("22", "direct Registry call", "the caller is not ClaimsCore", refuse=refusal(name, "Core authority"))
        f.nothing_moved(order_3, "direct call naming " + what, before)

    # Cases 19 and 20: order 1 was accepted under size = 10. The owner then changes the pin, unpins and revokes.
    accepted = f.h.snapshot([order_1["claim"], order_1["reservation"], order_1["receipt"]])
    f.pin(f.permission, [size_12], "ACCEPTED: the buyer's owner changes the pin on agent 1's permission to size = 12", case="19")
    f.h.check("agent 1's permission now pins size = 12 at revision 2", f.pinned(f.permission), dict(revision=2, pinned=[size_12], active=True))
    resent = f.set_terms(variant(order_1), SELLER_TERMS, shown=changed(SELLER_TERMS, size="11"))
    f.h.check("the re-sent order shows a list that does not hash to its signed terms",
        order_terms_hash(resent["order_terms"]) == resent["external_contract_hash"], False)
    for shown, how in ((True, "through the pinned instruction with a list that says size = 11"),
            (False, "through plain accept_financed_tab")):
        row = f.place(resent, f"no effect: accepted order 1 is sent again after the pin changed, {how}", case="20",
            what_changed="an accepted order is sent again after the pin changed", shown=shown)
        f.h.check("the re-sent order changes no observed account: " + how, row["observed_after"], row["observed_before"])
        f.h.check("the re-sent order does not reach the Registry: " + how, f"Program {REGISTRY} invoke [2]" in row["result"]["logs"], False)
    f.pin(f.permission, [], "ACCEPTED: the buyer's owner unpins agent 1's permission (empty list)", case="19")
    f.send("the buyer's owner revokes agent 1's permission", f.ix("principal_registry", "revoke_mandate",
        dict(owner=buyer["owner"].pubkey(), principal=f.company, mandate=f.permission), expected_revision=3), [buyer["owner"]])
    f.mark("19", "revoke", "nothing")
    f.h.check("agent 1's permission is revoked at revision 4", f.pinned(f.permission), dict(revision=4, pinned=[], active=False))
    f.h.check("the pin change, the unpin and the revocation left order 1's records byte-identical",
        f.h.snapshot([order_1["claim"], order_1["reservation"], order_1["receipt"]]), accepted)
    run = f.earn(order_1, "order-1-work", EARNED,
        label="ACCEPTED: after the pin changed and the permission was revoked, the seller reports 60,000 of completed work on order 1")
    f.mark("19", "seller's work report", "nothing")
    seller_ata = f.h.ata(seller["owner"].pubkey(), f.mint)
    seller_before, wallet_before = f.h.amount(seller_ata), f.h.amount(lender["custody_token"])
    with f.paid_by(f.collector):
        request = f.collection(order_1, run, EARNED)
        f.send("ACCEPTED: an outside key collects order 1's 60,000 from the lender wallet to the seller",
            f.envelope(compact_sign(lender["config"], lender["wallet"], request["instruction"])),
            request["signers"], receipt=request["receipt"])
        row = f.mark("19", "collection", "nothing")
    f.h.check("the collection's only signer is the outside key", f.signers_of(row), [str(f.collector.pubkey())])
    f.h.check("the seller receives exactly 60,000", f.h.amount(seller_ata) - seller_before, EARNED)
    f.h.check("the lender wallet pays exactly 60,000", wallet_before - f.h.amount(lender["custody_token"]), EARNED)
    claim = f.state("FundedClaim", order_1["claim"])
    for field, value in (("paid", EARNED), ("payable", 0), ("debt", EARNED), ("remaining", ORDER - EARNED)):
        f.h.check("order 1 record " + field + " after collection", claim[field], value)
    f.h.check("order 1's record still carries the terms hash it was accepted with",
        claim["external_contract_hash"], order_1["external_contract_hash"])


class PasskeyPinnedTerms(SourceChecked, CommitLegs, PasskeyFixture):
    """A permission created pinned, a pin change and pinned orders when the owners are passkeys."""


def passkey_owners(f):
    f.deployment_setup()
    f.first_passkey_principal()
    f.setup(financier_wallet=True)
    credentials = {}
    for role in ("company", "issuer"):
        credentials[role] = f.initialize_passkey(role, role)
        f.adopt(role, credentials[role], role)
    buyer, issuer, seller = [f.parties[x] for x in ("company", "issuer", "supplier")]
    f.h.check("the buyer's owner and the credit issuer's owner are passkey aliases, not signing keys",
        [x["owner"].pubkey().is_on_curve() for x in (buyer, issuer)], [False, False])
    f.actors[0] = f.epoch_actor("pinned-terms")
    mandate_id = ident("pinned-terms-passkey-owner")
    permission = pda(REGISTRY, b"mandate", f.company, mandate_id, u64(buyer["generation"]))
    f.mandates[0] = permission
    size_10 = term("size", "10")
    f.send("ACCEPTED: the buyer's owner, a passkey, creates the agent's permission already pinned to size = 10, in one approved instruction",
        f.ix("principal_registry", "create_pinned_mandate",
            dict(owner=buyer["owner"].pubkey(), principal=f.company, actor=f.actors[0], budget=f.budget, mandate=permission),
            args=dict(mandate_id=mandate_id, supplier=seller["owner"].pubkey(), source=f.facility,
                per_operation_limit=200_000, expires_at=NOW + 600), required_terms=[size_10]), [buyer["owner"]])
    row = f.note(section="passkey", leg="create pinned", changed="nothing")
    f.h.check("the creation was approved by one passkey proof", len(row["passkey_authorizations"]), 1)
    f.h.check("the permission exists at revision 1 with the term size = 10",
        stored_pin(f.registry, f.h.svm, permission), dict(revision=1, pinned=[size_10], active=True))

    def order(name, pairs, revision, *, shown=True, **options):
        tab = f.acceptance(name, cap=ORDER, **options)
        tab["terms"]["mandate_revision"] = revision
        PinnedTerms.set_terms(f, tab, pairs)
        remaining = f.remaining(["company", "issuer", "supplier", tab["financier"]])
        if shown:
            return tab, f.ix("claims_core", "accept_pinned_financed_tab", tab["accounts"], remaining=remaining,
                order_terms=tab["order_terms"], **tab["args"])
        return tab, f.ix("claims_core", "accept_financed_tab", tab["accounts"], remaining=remaining, **tab["args"])
    plain = dict(maximum_interest=0, fees=False)
    tab, instruction = order("order-1", changed(SIXTEEN_TERMS, size="11"), 1, shown=False, **plain)
    f.send("refused: straight after the permission is created, order 1 with size = 11 through plain accept_financed_tab, "
        "signed by the agent, the seller and the credit issuer's passkey", instruction, tab["signers"], reject=True, receipt=tab["receipt"])
    f.note(section="passkey", leg="order, no list shown", changed="size = 11, plain instruction, no list shown",
        refuse=refusal("TermsRequired", "no list shown", index=2))
    tab, instruction = order("order-1", SIXTEEN_TERMS, 1, **plain)
    f.send("ACCEPTED: order 1, maximum 100,000, with size = 10 among its sixteen terms, signed by the agent, the seller and the credit issuer's passkey",
        instruction, tab["signers"], receipt=tab["receipt"])
    row = f.note(section="passkey", leg="order, list shown", changed="nothing")
    f.h.check("the order was approved by one passkey proof, the credit issuer's", len(row["passkey_authorizations"]), 1)
    f.h.check("the order record stores the terms hash", f.state("FundedClaim", tab["claim"])["external_contract_hash"],
        tab["external_contract_hash"])
    f.h.check("budget hold after order 1", f.registry.read(f.h.svm, f.budget, "Budget")["reserved"], ORDER)

    both = listing((("size", "10"), ("colour", "red")))
    f.send("ACCEPTED: the buyer's owner, a passkey, pins size = 10 and colour = red on the agent's permission",
        pin_instruction(f.registry, buyer["owner"].pubkey(), f.company, permission, both, 1), [buyer["owner"]])
    row = f.note(section="passkey", leg="pin", changed="nothing")
    f.h.check("the pin was approved by one passkey proof", len(row["passkey_authorizations"]), 1)
    f.h.check("the permission stores both terms at revision 2",
        stored_pin(f.registry, f.h.svm, permission), dict(revision=2, pinned=both, active=True))
    costly = dict(maximum_interest=1_000, fees=True)
    tab, instruction = order("order-2", changed(SIXTEEN_TERMS, colour="blue"), 2, **costly)
    f.send("refused: order 2 with colour = blue among its sixteen terms, with a seller fee and interest",
        instruction, tab["signers"], reject=True, receipt=tab["receipt"])
    f.note(section="passkey", leg="order, list shown", changed="colour in the seller's terms: blue instead of red",
        refuse=refusal("TermsNotPermitted", "pinned term", index=2))
    tab, instruction = order("order-2", SIXTEEN_TERMS, 2, **costly)
    f.send("ACCEPTED: order 2, maximum 100,000 with a seller fee and interest of up to 1,000, sixteen terms, "
        "signed by the agent, the seller and the credit issuer's passkey", instruction, tab["signers"], receipt=tab["receipt"])
    row = f.note(section="passkey", leg="order, list shown", changed="nothing")
    f.h.check("order 2 is the largest signed transaction of this run", row["wire_bytes"], max(x["wire_bytes"] for x in f.h.cases))
    f.h.check("order 2 fits the 4,096-byte transaction limit", row["wire_bytes"] <= 4096, True)
    f.h.check("budget hold after order 2 includes its interest", f.registry.read(f.h.svm, f.budget, "Budget")["reserved"], 2 * ORDER + 1_000)
    f.roles = {"buyer's owner (passkey alias)": buyer["owner"], "credit issuer's owner (passkey alias)": issuer["owner"],
        "agent": f.agent, "seller": seller["owner"]}


class RecoverablePinnedTerms(SourceChecked, CommitLegs, RecoveryFixture):
    """A pinned permission on a recoverable order, which shows no list."""


def recoverable_order(f):
    f.setup(financier_wallet=True)
    buyer, seller = f.parties["company"], f.parties["supplier"]
    f.receiving_open("supplier")
    permission = f.controlled_mandate("pinned-terms")
    size_10 = term("size", "10")

    def pin(required_terms, revision, label):
        f.send(label, pin_instruction(f.registry, buyer["owner"].pubkey(), f.company, permission, required_terms, revision),
            [buyer["owner"]])
        f.note(section="15", leg="pin", changed="nothing")
    pin([size_10], 1, "ACCEPTED: the buyer's owner pins size = 10 on the agent's permission")
    f.h.check("the permission stores the term size = 10 at revision 2",
        stored_pin(f.registry, f.h.svm, permission), dict(revision=2, pinned=[size_10], active=True))
    tab = f.recoverable_tab("order-1-recoverable", mandate=permission, cap=ORDER)
    tab["terms"]["mandate_revision"] = 2
    budget = f.registry.read(f.h.svm, f.budget, "Budget")
    f.accept_recoverable(tab, error="TermsRequired", error_program="principal_registry")
    f.h.cases[-1]["name"] = ("refused: order 1, a recoverable order (accept_recoverable_financed_tab) of 100,000, "
        "on the agent's pinned permission")
    f.note(section="15", leg="recoverable order, no list shown",
        changed="the permission is pinned and this instruction shows no list", refuse=refusal("TermsRequired", "no list shown"))
    f.h.check("the refused recoverable order left the budget as it was", f.registry.read(f.h.svm, f.budget, "Budget"), budget)
    for name, address in (("order record", tab["claim"]), ("Registry reservation", tab["reservation"]), ("bill", tab["bill"])):
        f.h.check(f"no {name} exists after the refused recoverable order", absent(f.h.svm, address), True)
    pin([], 2, "ACCEPTED: the buyer's owner unpins the agent's permission (empty list)")
    tab["terms"]["mandate_revision"] = 3
    f.accept_recoverable(tab)
    f.h.cases[-1]["name"] = "ACCEPTED: after the unpin, order 1, the recoverable order of 100,000"
    f.note(section="15", leg="recoverable order, no list shown", changed="nothing")
    f.h.check("budget hold after the accepted recoverable order", f.registry.read(f.h.svm, f.budget, "Budget")["reserved"], ORDER)
    f.roles = {"buyer's owner": buyer["owner"], "agent": f.agent, "seller's owner": seller["owner"]}


class PoolFundedPinnedTerms(Journey):
    """A pinned permission on pool-funded orders (`accept_tab`, `accept_metered_tab`), on integration.py's fixture.

    That fixture builds v0 transactions from interface signer flags. Owner
    accounts are not flagged there, so the keys that sign are flagged here.
    """
    now = NOW
    note = CommitLegs.note
    refused = SourceChecked.refused

    def tx(self, instructions, signers):
        supplied = {signer.pubkey() for signer in signers}
        return super().tx([Instruction(item.program_id, bytes(item.data),
            [M(meta.pubkey, meta.is_signer or meta.pubkey in supplied, meta.is_writable) for meta in item.accounts])
            for item in instructions], signers)

    def finish(self, suite, *, error=None):
        trace = dict(schema="dexter-pinned-terms-pool-funded/v1", status="failed" if error else "passed",
            scope="Compiled Registry/Core/Pool on integration.py's fixture: v0 transactions through a lookup table written by the harness",
            transactions=self.h.cases, assertions=self.h.assertions, error=repr(error) if error else None,
            program_sha256=self.hashes, public_transactions_sent=0)
        path = OUT / (suite + "-trace.json")
        path.write_text(json.dumps(trace, indent=2, default=json_value) + "\n")
        summary = dict(status=trace["status"], trace=str(path), trace_sha256=digest(path),
            transaction_count=len(self.h.cases), refused=sum(x["expected"] == "reject" for x in self.h.cases),
            assertions=len(self.h.assertions), receipts=0,
            maximum_signed_packet=max((x["wire_bytes"] for x in self.h.cases), default=0),
            build_receipt_sha256=digest(OUT / "build-receipt.json"), idl_receipt_sha256=digest(OUT / "idl-receipt.json"))
        print(json.dumps(summary))
        return summary


def pool_funded_order(j):
    j.setup()
    registry = Idl(OUT / "idl/principal_registry.json")
    permission = j.mandates[0]
    size_10 = term("size", "10")
    required = refusal("TermsRequired", "no list shown", index=1)

    def pin(required_terms, revision, label):
        j.send(label, pin_instruction(registry, j.owner.pubkey(), j.principal, permission, required_terms, revision), [j.owner])
        j.note(section="16", leg="pin", changed="nothing")

    def held():
        return registry.read(j.h.svm, j.budget, "Budget")["reserved"]

    def metered(tab, label, request_id, error=None):
        """accept_metered_tab with the accounts of accept_tab and the unit-price policy the terms commit to."""
        operation_id, core_op, registry_op = j.operations(request_id, j.agent.pubkey())
        accounts = dict(j.base(), actor=j.actors[0], mandate=permission, credential=j.agent.pubkey(),
            manager=j.h.payer.pubkey(), claim=tab["claim"], reservation=tab["reservation"],
            operation=core_op, registry_operation=registry_op)
        j.send(label, j.ix("claims_core", "accept_metered_tab", accounts,
            args=dict(operation_id=request_id, terms=tab["terms"]), **tab["policy"]), [j.h.payer, j.agent], error)
    pin([size_10], 1, "ACCEPTED: the buyer's owner pins size = 10 on agent 1's permission")
    j.h.check("the permission stores the term size = 10 at revision 2",
        stored_pin(registry, j.h.svm, permission), dict(revision=2, pinned=[size_10], active=True))
    order_1 = j.tab("order-1-pool-funded", ORDER)
    order_2 = j.tab("order-2-metered", ORDER)
    order_2["policy"] = dict(namespace=ident("provider-namespace"), service_unit=ident("completed-inference-unit"),
        unit_price=1, external_contract_hash=ident("provider-contract"))
    order_2["terms"]["external_terms_hash"] = hashlib.sha256(POLICY_PREFIX + order_2["policy"]["namespace"]
        + order_2["policy"]["service_unit"] + u64(1) + order_2["policy"]["external_contract_hash"]).digest()
    requests = ident("accept-order-1-pool-funded"), ident("accept-order-2-metered")
    for tab in (order_1, order_2):
        tab["terms"]["mandate_revision"] = 2
    j.accept(order_1, "refused: order 1, a pool-funded order (accept_tab) of 100,000, on agent 1's pinned permission",
        error="TermsRequired", request_id=requests[0])
    j.note(section="16", leg="pool-funded order, no list shown",
        changed="the permission is pinned and this instruction shows no list", refuse=required)
    metered(order_2, "refused: order 2, a metered pool-funded order (accept_metered_tab) of 100,000, on agent 1's pinned permission",
        requests[1], error="TermsRequired")
    j.note(section="23", leg="metered pool-funded order, no list shown",
        changed="the permission is pinned and this instruction shows no list", refuse=required)
    j.h.check("the refused pool-funded orders left no budget hold", held(), 0)
    for tab in (order_1, order_2):
        for name, address in (("order record", tab["claim"]), ("Registry reservation", tab["reservation"])):
            j.h.check(f"no {name} exists after the refused {tab['name']}", absent(j.h.svm, address), True)
    pin([], 2, "ACCEPTED: the buyer's owner unpins agent 1's permission (empty list)")
    for tab in (order_1, order_2):
        tab["terms"]["mandate_revision"] = 3
    j.accept(order_1, "ACCEPTED: after the unpin, order 1, the pool-funded order of 100,000", request_id=requests[0])
    j.note(section="16", leg="pool-funded order, no list shown", changed="nothing")
    j.h.check("budget hold after the accepted pool-funded order", held(), ORDER)
    metered(order_2, "ACCEPTED: after the unpin, order 2, the metered pool-funded order of 100,000", requests[1])
    j.note(section="23", leg="metered pool-funded order, no list shown", changed="nothing")
    j.h.check("budget hold after the accepted metered order", held(), 2 * ORDER)
    j.h.check("order 2's record is a metered one", j.core.read(j.h.svm, order_2["claim"], "Claim")["version"], 2)
    j.roles = {"buyer's owner": j.owner, "agent 1": j.agent, "seller": j.supplier, "pool manager and fee payer": j.h.payer}


RUNS = (("pinned-terms", PinnedTerms, exercise), ("pinned-terms-created-pinned", CreatedPinnedTerms, created_pinned),
    ("pinned-terms-passkey", PasskeyPinnedTerms, passkey_owners),
    ("pinned-terms-recoverable", RecoverablePinnedTerms, recoverable_order),
    ("pinned-terms-pool-funded", PoolFundedPinnedTerms, pool_funded_order))


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
            row["pinned_terms"] = row.pop("commit_legs")
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
            annotated_acceptances=sum(row["outcome"] == "accepted" for row in annotated),
            roles={role: str(holder.pubkey()) for role, holder in f.roles.items()})
        rows.extend(index)
    (directory / "case-index.json").write_text(json.dumps(rows, indent=1, default=json_value) + "\n")
    f = results[0][0]
    for name in ("build-receipt.json", "idl-receipt.json"):
        (directory / name).write_bytes((OUT / name).read_bytes())
    manifest = dict(runs=receipts, run_seconds=round(seconds, 2), engine="solders 0.29.0 LiteSVM",
        name_fingerprints={name: name_print(name).hex() for name in ("size", "colour")},
        value_fingerprints={value: value_print(value).hex() for value in ("10", "11", "red", "blue")},
        program_sha256={name: value["sha256"] for name, value in f.build["artifacts"].items()},
        swig_sha256=SWIG_SHA256, build_receipt_sha256=digest(OUT / "build-receipt.json"),
        idl_receipt_sha256=digest(OUT / "idl-receipt.json"))
    (directory / "pinned-terms-receipt.json").write_text(json.dumps(manifest, indent=2) + "\n")
    return manifest


def main():
    started = time.monotonic()
    results = [run(*item) for item in RUNS]
    if "--record" in sys.argv:
        print(json.dumps(record(results, sys.argv[sys.argv.index("--record") + 1], time.monotonic() - started)))


if __name__ == "__main__":
    main()
