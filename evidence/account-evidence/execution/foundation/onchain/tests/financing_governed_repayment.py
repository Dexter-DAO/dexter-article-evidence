#!/usr/bin/env python3
"""Signed native-v1 governed financed repayment through the retained Swig.

These fixtures exercise the actual Registry/Core/Pool/Swig/SPL programs. They
use disposable synthetic cash and generated current IDLs. The request hashes
below independently encode the shared contract; no program account is patched.
Customer Kit/RPC preparation and durable coordinator admission are separate.
"""
from financing_correction_fixture import *
from financing_fixture import YEAR


def option_key(value):
    return b"\0" if value is None else b"\1" + bytes(value)


class GovernedFixture(CorrectionFixture):
    def __init__(self):
        super().__init__()
        self.repayment_wallets = {}

    def repayment_wallet(self, role, amount=100_000):
        """A distinct governed source can share a Principal with its creditor.

        Preserve existing issuer/creditor destinations when creating its cash
        source; both wallets still affect the same continuing financial book.
        """
        saved = dict(self.parties[role])
        self.create_custody(role, amount, wallet=True, name="repayment-" + role)
        result = dict(self.parties[role])
        self.repayment_wallets[role] = result
        self.parties[role].clear()
        self.parties[role].update(saved)
        return result

    def protocol_route(self, terms):
        if not terms["protocol_share_bps"]:
            return None, None
        candidates = [*self.parties.values(), *self.repayment_wallets.values()]
        custody = next(p["custody"] for p in candidates
            if p.get("custody_token") == terms["protocol_destination"])
        return custody, terms["protocol_destination"]

    def permission_address(self, role, name):
        return pda(REGISTRY, b"repayment-permission", self.parties[role]["principal"], ident(name))

    def request_digest(self, request):
        a, x = request["accounts"], request["args"]
        if request["kind"] == 3:
            raw = b"".join(bytes(a[k]) for k in ("principal", "claim", "run",
                "repayment_principal", "repayment_authority"))
            raw += u64(x["repayment_owner_generation"])
            raw += b"".join(bytes(a[k]) for k in ("from", "creditor_custody", "destination"))
            raw += bytes(self.mint) + u64(x["amount"]) + u64(x["maximum_interest"])
            raw += u64(x["expected_ownership_revision"])
            domain = b"DEXTER:FUNDED-RUN-REPAY:1"
        else:
            raw = b"".join(bytes(a[k]) for k in ("issuer", "facility",
                "repayment_principal", "repayment_authority"))
            raw += u64(x["repayment_owner_generation"])
            raw += b"".join(bytes(a[k]) for k in ("from", "financier_custody", "destination"))
            raw += bytes(self.mint) + u64(x["amount"]) + u64(x["maximum_interest"])
            domain = b"DEXTER:FUNDING-FACILITY-REPAY:1"
        raw += option_key(a["protocol_custody"]) + option_key(a["protocol_destination"])
        return hashlib.sha256(domain + bytes(CORE) + raw).digest()

    def governed_run(self, tab, run, name, amount, permission, *, role="company",
            maximum_interest=0, outage=False, account_overrides=None, arg_overrides=None):
        state = self.state("FundedRun", run["run"])
        creditor = self.role_of(state["current_creditor"])
        request = super().repayment(tab, run, name, amount, payer=role, creditor=creditor,
            maximum_interest=maximum_interest, outage=outage)
        wallet = self.repayment_wallets[role]
        a, args = request["accounts"], request["args"]
        operation = self.operation(b"repay-run", wallet["wallet"], args["request_id"])
        receipt = self.receipt(self.company, operation)
        a.update(repayment_authority=wallet["wallet"], repayment_binding=wallet["binding"],
            repayment_custody=wallet["custody"],
            repayment_permission=self.permission_address(role, permission),
            receipt=receipt, registry_accrual_operation=self.registry_interest_operation(operation, run["run"]),
            registry_release_operation=self.registry_interest_operation(operation, run["run"], release=True))
        a["from"] = wallet["custody_token"]
        a["protocol_custody"], a["protocol_destination"] = self.protocol_route(state["interest_terms"])
        a.update(account_overrides or {})
        args.update(arg_overrides or {})
        roles = ["company", role, creditor]
        if state["interest_terms"]["protocol_share_bps"]:
            roles.append(state["interest_terms"]["protocol_beneficiary"])
        self.observe(receipt)
        request.update(instruction=self.ix("claims_core", "repay_governed_funded_run", a,
            remaining=self.remaining(roles, outage), args=args), receipt=receipt, kind=3,
            role=role, wallet=wallet, permission=permission, roles=roles, terms=state["interest_terms"],
            creditor=state["current_creditor"], obligation=run["run"], obligor=self.company,
            signers=[], tab=tab, run=run)
        request["digest"] = self.request_digest(request)
        return request

    def governed_facility(self, name, amount, permission, *, role="issuer",
            maximum_interest=0, outage=False, account_overrides=None, arg_overrides=None):
        request = super().facility_repayment(name, amount, maximum_interest=maximum_interest, outage=outage)
        wallet = self.repayment_wallets[role]
        state = self.state("FundingFacility", self.facility)
        a, args = request["accounts"], request["args"]
        operation = self.operation(b"repay-facility", wallet["wallet"], args["request_id"])
        receipt = self.receipt(self.parties["issuer"]["principal"], operation)
        a.update(repayment_principal=self.parties[role]["principal"],
            repayment_authority=wallet["wallet"], repayment_binding=wallet["binding"],
            repayment_custody=wallet["custody"],
            repayment_permission=self.permission_address(role, permission), receipt=receipt)
        a["from"] = wallet["custody_token"]
        args["repayment_owner_generation"] = self.parties[role]["generation"]
        a["protocol_custody"], a["protocol_destination"] = self.protocol_route(state["interest_terms"])
        a.update(account_overrides or {})
        args.update(arg_overrides or {})
        roles = ["issuer", "financier", role]
        if state["interest_terms"]["protocol_share_bps"]:
            roles.append(state["interest_terms"]["protocol_beneficiary"])
        self.observe(receipt)
        request.update(instruction=self.ix("claims_core", "repay_governed_funding_facility", a,
            remaining=self.remaining(roles, outage), args=args), receipt=receipt, kind=4,
            role=role, wallet=wallet, permission=permission, roles=roles, terms=state["interest_terms"],
            creditor=state["financier"], obligation=self.facility, obligor=self.parties["issuer"]["principal"],
            signers=[])
        request["digest"] = self.request_digest(request)
        return request

    def grant_permission(self, request, *, exact=False, overrides=None, owner=None, reject=False):
        role, wallet = request["role"], request["wallet"]
        party = self.parties[role]
        a, args = request["accounts"], request["args"]
        amount = args["amount"]
        terms = dict(binding=wallet["binding"], kind=request["kind"], obligation=request["obligation"],
            obligor=request["obligor"], mint=self.mint, debit_from=wallet["custody_token"],
            creditor=request["creditor"],
            creditor_custody=a["creditor_custody" if request["kind"] == 3 else "financier_custody"],
            creditor_destination=a["destination"],
            protocol_beneficiary=request["terms"]["protocol_beneficiary"] if request["terms"]["protocol_share_bps"] else Pubkey.default(),
            protocol_destination=request["terms"]["protocol_destination"] if request["terms"]["protocol_share_bps"] else Pubkey.default(),
            executor=self.h.payer.pubkey(), authority_generation=party["generation"],
            not_before=self.now, expires_at=self.now + 300, minimum_interval=0,
            per_payment_amount=amount, per_payment_principal=amount,
            per_payment_interest=args["maximum_interest"],
            total_amount=amount, total_principal=amount, total_interest=args["maximum_interest"],
            maximum_payments=1 if exact else 10,
            exact_request_digest=request["digest"] if exact else bytes(32),
            exact_amount=amount if exact else 0,
            exact_receipt=request["receipt"] if exact else Pubkey.default())
        terms.update(overrides or {})
        permission = self.permission_address(role, request["permission"])
        self.observe(permission)
        signer = owner or party["owner"]
        accounts = dict(principal=party["principal"], owner=signer.pubkey(),
            binding=wallet["binding"], permission=permission)
        instruction = self.ix("principal_registry", "grant_repayment_permission", accounts,
            args=dict(permission_id=ident(request["permission"]), terms=terms))
        self.send("owner grants " + ("exact individual" if exact else "bounded standing") + " repayment permission " + request["permission"],
            instruction, [signer], reject=reject)
        return dict(instruction=instruction, terms=terms, permission=permission, role=role, owner=signer)

    def permission_state(self, permission):
        return self.registry.read(self.h.svm, permission, "RepaymentPermission")

    def revoke_permission(self, grant):
        party = self.parties[grant["role"]]
        self.send("current owner revokes future repayment permission",
            self.ix("principal_registry", "revoke_repayment_permission",
                dict(principal=party["principal"], owner=party["owner"].pubkey(), permission=grant["permission"]),
                args=dict(authority_generation=party["generation"])), [party["owner"]])

    def repayment_bundle(self, request, *, caller=None, bad_hash=False, nonce=None, expires_at=None, inner=None):
        wallet = request["wallet"]
        caller = caller or self.h.payer
        binding = self.registry.read(self.h.svm, wallet["binding"], "FundingWalletBinding")
        generation = request["args"]["repayment_owner_generation"]
        args = dict(binding=wallet["binding"], nonce=binding["nonce"] if nonce is None else nonce,
            expires_at=self.now + 300 if expires_at is None else expires_at,
            authority_generation=generation, action=request["kind"], bundle_hash=bytes(32))
        tail = compact_sign(wallet["config"], wallet["wallet"], inner or request["instruction"])
        accounts = dict(config=wallet["config"], wallet=wallet["wallet"], binding=wallet["binding"],
            principal=self.parties[request["role"]]["principal"], caller=caller.pubkey(), instructions=SYSVAR_INSTRUCTIONS)
        remaining = [M(request["accounts"]["repayment_permission"]), M(request["receipt"])]
        authorize = self.ix("principal_registry", "authorize_funding_wallet", accounts, remaining=remaining, args=args)
        raw = canonical_instruction(tail, [authorize, tail], self.h.payer.pubkey())
        args["bundle_hash"] = bytes([99]) * 32 if bad_hash else hashlib.sha256(
            b"DEXTER:FUNDING-SWIG-BUNDLE:1" + bytes(REGISTRY) + bytes(CORE) + bytes(SWIG)
            + self.domain + bytes(self.parties[request["role"]]["principal"])
            + bytes(wallet["binding"]) + u64(args["nonce"]) + struct.pack("<q", args["expires_at"])
            + u64(generation) + bytes([request["kind"]]) + raw).digest()
        return [self.ix("principal_registry", "authorize_funding_wallet", accounts, remaining=remaining, args=args), tail]

    def pay(self, label, request, *, reject=False, replay=False, error=None, **envelope):
        permission = request["accounts"]["repayment_permission"]
        source = request["wallet"]["custody"]
        before = self.permission_state(permission)
        binding_before = self.registry.read(self.h.svm, request["wallet"]["binding"], "FundingWalletBinding")
        source_before = self.state("FinancierCustody", source)
        cash_before = self.h.amount(request["wallet"]["custody_token"])
        books_before = self.book_snapshot()
        bundle = self.repayment_bundle(request, **envelope)
        caller = envelope.get("caller", self.h.payer)
        signers = [] if caller.pubkey() == self.h.payer.pubkey() else [caller]
        self.send(label, bundle, signers, reject=reject, error=error, receipt=request["receipt"])
        after = self.permission_state(permission)
        if reject or replay:
            self.h.check(label + " consumes no additional permission", after, before)
            self.h.check(label + " preserves funding wallet nonce", self.registry.read(
                self.h.svm, request["wallet"]["binding"], "FundingWalletBinding")["nonce"], binding_before["nonce"])
            self.h.check(label + " preserves payer custody", self.state("FinancierCustody", source), source_before)
            self.h.check(label + " does not debit tokens", self.h.amount(request["wallet"]["custody_token"]), cash_before)
            self.h.check(label + " does not change any financial book", self.book_snapshot(), books_before)
        else:
            receipt = check_receipt(self.core, self.h.svm, request["receipt"])
            self.h.check(label + " independent exact request digest", receipt["request_digest"], request["digest"])
            self.h.check(label + " historical actual payer", receipt["debit_principal"], self.parties[request["role"]]["principal"])
            self.h.check(label + " historical wallet authority", receipt["debit_authority"], request["wallet"]["wallet"])
            self.h.check(label + " historical exact debit account", receipt["debit_from"], request["wallet"]["custody_token"])
            self.h.check(label + " source tokens decrease exactly", cash_before - self.h.amount(request["wallet"]["custody_token"]), request["args"]["amount"])
            current = self.state("FinancierCustody", source)
            self.h.check(label + " source accounted cash decreases exactly", source_before["accounted_cash"] - current["accounted_cash"], request["args"]["amount"])
            for field in ("unused", "payable"):
                self.h.check(label + " preserves shared protected " + field, current[field], source_before[field])
            used = after["consumed"]
            old = before["consumed"]
            for field, delta in (("amount", request["args"]["amount"]), ("principal", receipt["principal_paid"]),
                    ("interest", receipt["interest_paid"]), ("payments", 1)):
                self.h.check(label + " consumes actual " + field, used[field], old[field] + delta)
            self.h.check(label + " preserves accepted funding floor", self.h.amount(request["wallet"]["custody_token"]) >= current["unused"] + current["payable"], True)
        return bundle


def run_authority(f):
    f.setup()
    f.repayment_wallet("company")
    tab = f.acceptance("governed-company", cap=50_000, maximum_interest=5_000, fees=False)
    f.accept(tab)
    run = f.earn(tab, "governed-delivery", 40_000)
    f.collect(f.collection(tab, run, 40_000))
    first = f.governed_run(tab, run, "individual-payment", 10_000, "individual-permission")
    grant = f.grant_permission(first, exact=True)
    f.pay("another executor cannot spend the owner's permission", first, reject=True, caller=f.agent)
    f.pay("changed Swig bundle hash cannot authorize a debit", first, reject=True, bad_hash=True)
    f.send("bare Swig call cannot invent Registry authority", compact_sign(first["wallet"]["config"], first["wallet"]["wallet"], first["instruction"]), reject=True)
    changed = f.governed_run(tab, run, "individual-payment", 10_001, "individual-permission")
    f.pay("exact owner approval rejects a changed amount", changed, reject=True)
    changed_id = f.governed_run(tab, run, "different-payment-id", 10_000, "individual-permission")
    f.pay("exact approval binds the original receipt identity", changed_id, reject=True)
    protocol = f.parties["protocol"]
    inner = first["instruction"]
    missing_book = Instruction(inner.program_id, inner.data,
        [meta for meta in inner.accounts if meta.pubkey not in (protocol["book"], protocol["projection"])])
    f.pay("late missing zero-effect protocol book rolls back the entire payment", first,
        reject=True, error="FundingBookAccounts", inner=missing_book)
    row = f.h.cases[-1]
    logs = row["result"]["meta"]["logs"]
    consumed = next(i for i, log in enumerate(logs) if "Instruction: ConsumeRepaymentPermission" in log)
    consume_success = next(i for i, log in enumerate(logs) if i > consumed and log == f"Program {REGISTRY} success")
    transferred = next(i for i, log in enumerate(logs) if log == f"Program {TOKEN} success")
    journal_failure = next(i for i, log in enumerate(logs) if "Error Code: FundingBookAccounts" in log)
    f.h.check("late rollback occurs after successful permission CPI and SPL transfer",
        consumed < consume_success < transferred < journal_failure, True)
    token_call = next(call["instruction"] for group in row["result"]["meta"]["inner_instructions"]
        for call in group if row["account_keys"][call["instruction"]["programIdIndex"]] == str(TOKEN))
    f.h.check("rolled-back SPL instruction attempted the exact approved amount",
        bytes(token_call["data"][1:]), bytes([12]) + u64(10_000) + bytes([6]))
    f.pay("individual owner approval repays delivered principal through native Swig", first)
    f.pay("populated receipt refuses a conflicting economic digest under the same identity", changed,
        reject=True, error="RepaymentPermission")
    f.pay("lost response recovery reuses original economic payment", first, replay=True)
    zero = check_receipt(f.core, f.h.svm, first["receipt"])
    protocol_effect = next(e for e in zero["effects"] if e["principal"] == f.parties["protocol"]["principal"])
    f.h.check("accepted protocol book is retained for zero interest allocation", protocol_effect["before"], protocol_effect["after"])
    principal_limit = f.governed_run(tab, run, "principal-limit-is-independent", 1_000, "principal-limit-permission")
    f.grant_permission(principal_limit, overrides=dict(per_payment_principal=999, total_principal=999))
    f.pay("principal component limit rejects an otherwise permitted total", principal_limit, reject=True)
    expires = f.governed_run(tab, run, "expired-unused-permission", 1_000, "expired-permission")
    f.grant_permission(expires, overrides=dict(expires_at=f.now + 5))

    standing_one = f.governed_run(tab, run, "standing-one", 6_000, "standing-permission")
    standing = f.grant_permission(standing_one, overrides=dict(not_before=f.now + 10, minimum_interval=10,
        total_amount=12_000, total_principal=12_000, maximum_payments=2))
    f.pay("standing permission enforces its not-before time", standing_one, reject=True)
    f.advance(10)
    f.pay("expired unused permission cannot make its first payment", expires, reject=True)
    f.pay("designated executor pays without another owner signature", standing_one)
    f.pay("historical standing result ignores minimum interval", standing_one, replay=True)
    standing_two = f.governed_run(tab, run, "standing-two", 6_000, "standing-permission")
    f.pay("different payment cannot evade minimum interval", standing_two, reject=True)
    f.advance(10)
    f.pay("second eligible payment consumes bounded standing allowance", standing_two)
    third = f.governed_run(tab, run, "standing-over-budget", 1, "standing-permission")
    f.pay("new payment cannot exceed cumulative consumption", third, reject=True)
    before = f.permission_state(standing["permission"])
    f.send("grant retry cannot replenish spent standing limits", standing["instruction"], [standing["owner"]])
    f.h.check("grant retry preserves cumulative consumption", f.permission_state(standing["permission"]), before)

    pending = f.governed_run(tab, run, "old-title-unexecuted", 1_000, "old-title-permission")
    f.grant_permission(pending)
    f.execute("remaining title can be assigned after governed payments", f.assignment(tab, run, "governed-title-transfer", consideration=17_000, costs=0))
    assigned = f.governed_run(tab, run, "assignment-needs-new-permission", 1_000, "old-title-permission")
    f.pay("standing grant does not silently follow creditor assignment", assigned, reject=True)
    f.revoke_permission(grant)
    f.rotate("company", 71, "governed-payment-owner-change")
    f.pay("original payment survives creditor assignment owner rotation and revocation", first, replay=True)
    changed_owner = f.governed_run(tab, run, "new-owner-old-grant", 1_000, "old-title-permission")
    f.pay("new owner generation invalidates unexecuted old authority", changed_owner, reject=True)
    replacement = f.governed_run(tab, run, "replacement-owner-payment", 1_000, "replacement-owner-permission")
    replacement_grant = f.grant_permission(replacement, exact=True)
    f.pay("replacement owner can approve the continuing wallet and current creditor", replacement)
    f.revoke_permission(replacement_grant)
    original_consumed = f.permission_state(grant["permission"])
    f.send("original exact enrollment remains recoverable after owner rotation and revocation",
        grant["instruction"], [grant["owner"]])
    f.h.check("historical grant reconciliation never reactivates consumed authority",
        f.permission_state(grant["permission"]), original_consumed)
    revoked = f.governed_run(tab, run, "revoked-new-payment", 1, "replacement-owner-permission")
    f.pay("revoked permission cannot authorize another economic payment", revoked, reject=True)
    f.advance(400)
    f.pay("immutable receipt recovers after its permission expires", first, replay=True,
        expires_at=f.now - 1, nonce=0)


def facility_authority(f):
    f.setup()
    f.repayment_wallet("issuer", 70_000)
    # Reuse the actual financier source to exercise its aggregate reserve.
    f.repayment_wallets["financier"] = dict(f.parties["financier"])
    tab = f.acceptance("governed-facility", cap=100_000, maximum_interest=0, fees=False)
    f.accept(tab)
    run = f.earn(tab, "governed-facility-draw", 60_000)
    collection = f.collection(tab, run, 60_000)
    f.collect(collection)
    before_debt = f.state("FundedClaim", tab["claim"])["debt"]
    individual = f.governed_facility("facility-individual", 10_000, "facility-individual-permission")
    f.grant_permission(individual, exact=True)
    f.pay("issuer explicitly repays its separate external facility", individual)
    f.h.check("facility payment leaves company debt intact", f.state("FundedClaim", tab["claim"])["debt"], before_debt)
    standing = f.governed_facility("facility-standing-one", 5_000, "facility-standing-permission")
    permission = f.grant_permission(standing, overrides=dict(total_amount=10_000, total_principal=10_000, maximum_payments=2))
    f.pay("bounded issuer standing permission services facility debt", standing)
    f.pay("facility durable receipt is not another payment", standing, replay=True)
    f.revoke_permission(permission)
    blocked = f.governed_facility("facility-after-revocation", 1, "facility-standing-permission")
    f.pay("facility standing revocation stops future servicing", blocked, reject=True)
    f.pay("facility historical result survives revocation", standing, replay=True)

    # Keep the financier's accepted unused commitment and lower its free cash
    # using the existing governed withdrawal path, then attempt a third-party
    # company payment from precisely that protected source.
    available = f.state("FinancierCustody", f.parties["financier"]["custody"])
    free = available["accounted_cash"] - available["unused"] - available["payable"]
    f.withdraw(f.withdrawal("leave-only-one-free-token", free - 1))
    protected = f.governed_run(tab, run, "financier-cannot-spend-reserve", 2, "protected-source-permission", role="financier")
    f.grant_permission(protected, exact=True)
    f.pay("repayment cannot consume any other accepted wallet commitment", protected, reject=True)
    one = f.governed_run(tab, run, "financier-uses-only-free-token", 1, "free-source-permission", role="financier")
    f.grant_permission(one, exact=True)
    f.pay("eligible uncommitted cash remains usable for approved repayment", one)
    f.h.check("accepted unused supplier reservation survives payer repayment", f.state("FinancierCustody", f.parties["financier"]["custody"])["unused"], 40_000)
    # After a title transfer the historical payer can also be today's creditor
    # and the original funding custodian. All roles must update the one actual
    # custody once, without a stale unchecked-account write losing either leg.
    f.execute("historical payer acquires the residual creditor title",
        f.assignment(tab, run, "historical-payer-becomes-creditor", consideration=10_000, costs=0, buyer="financier"))
    f.pay("historical payment recovers when its payer is now the creditor", one, replay=True)
    alias_custody = f.parties["financier"]["custody"]
    correction = f.correction(tab, run, "aliased-historical-governed-refund", returned=60_000,
        collection=collection, repayment=one,
        account_overrides=dict(refund_custody=alias_custody))
    before = f.state("FinancierCustody", alias_custody)
    f.correct("one custody receives current creditor cash and historical payer refund", correction)
    after = f.state("FinancierCustody", alias_custody)
    f.h.check("aliased custody receives both exact cash legs once", after["accounted_cash"], before["accounted_cash"] + 60_000)
    f.h.check("aliased refund preserves other accepted commitments", after["unused"], before["unused"])
    f.correct("aliased custody historical correction cannot apply again", correction, replay=True)
    f.release(tab, "facility-unperformed-work", 40_000)


def stable_accounts_and_refunds(f):
    f.setup()
    f.repayment_wallet("company", 100_000)
    f.repayment_wallet("issuer", 100_000)
    tab = f.acceptance("governed-refunds", cap=50_000, maximum_interest=5_000, fees=False)
    f.accept(tab)
    run = f.earn(tab, "governed-refund-delivery", 50_000)
    collection = f.collection(tab, run, 50_000)
    f.collect(collection)
    f.advance(YEAR)
    interest_limit = f.governed_run(tab, run, "interest-limit-is-independent", 1_000,
        "interest-limit-permission", maximum_interest=1_000)
    f.grant_permission(interest_limit, overrides=dict(per_payment_interest=999, total_interest=999))
    f.pay("interest approval bound rejects a wider requested ceiling", interest_limit, reject=True)
    interest = f.governed_run(tab, run, "governed-interest", 1_000, "interest-only-permission", maximum_interest=1_000)
    grant = f.grant_permission(interest, overrides=dict(per_payment_principal=0, total_principal=0,
        total_amount=2_000, total_interest=2_000, maximum_payments=2))
    f.pay("standing allowance consumes actual accrued interest separately", interest)
    f.h.check("positive protocol allocation has fixed accepted books", check_receipt(f.core, f.h.svm, interest["receipt"])["protocol_interest"], 370)
    next_payment = f.governed_run(tab, run, "governed-interest-frozen-protocol", 1_000, "interest-only-permission", maximum_interest=1_000)
    original_accounts = [m.pubkey for m in next_payment["instruction"].accounts]
    f.freeze(f.parties["protocol"]["custody_token"])
    f.pay("destination freeze after preparation keeps the exact repayment account set", next_payment)
    f.h.check("prepared account set remains unchanged at execution", [m.pubkey for m in next_payment["instruction"].accounts], original_accounts)
    waived = check_receipt(f.core, f.h.svm, next_payment["receipt"])
    f.h.check("frozen protocol allocation stays waived", waived["protocol_interest"], 0)
    f.h.check("waived protocol value is recorded", waived["protocol_waived"], 370)
    f.freeze(f.parties["protocol"]["custody_token"], False)
    f.revoke_permission(grant)
    f.rotate("company", 72, "governed-historical-refund")
    f.pay("interest payment remains recoverable after permission revocation and owner rotation", interest, replay=True)
    wallet = f.repayment_wallets["company"]
    f.transfer_tokens("fund recipient's explicit interest return", f.parties["protocol"]["owner_ata"], 100)
    missing = f.interest_refund(tab, run, interest, "missing-governed-refund-custody", 100, 100)
    f.refund_interest("governed historical refund requires its reserve ledger", missing, reject=True)
    wrong = f.interest_refund(tab, run, interest, "wrong-governed-refund-custody", 100, 100,
        account_overrides=dict(refund_custody=f.parties["issuer"]["custody"]))
    f.refund_interest("historical refund cannot credit an unrelated custody", wrong, reject=True)
    returned = f.interest_refund(tab, run, interest, "governed-collected-interest-return", 100, 100,
        account_overrides=dict(refund_custody=wallet["custody"]))
    before = f.state("FinancierCustody", wallet["custody"])
    consumed = f.permission_state(grant["permission"])
    f.refund_interest("historical income recipients return cash to original governed payer", returned)
    f.h.check("governed interest return is accounted cash", f.state("FinancierCustody", wallet["custody"])["accounted_cash"], before["accounted_cash"] + 200)
    f.h.check("interest refund cannot replenish repayment authority", f.permission_state(grant["permission"]), consumed)
    f.refund_interest("governed interest refund receipt cannot credit twice", returned, replay=True)

    principal = f.governed_run(tab, run, "governed-principal-and-interest", 23_000, "principal-payment-permission", maximum_interest=3_000)
    f.grant_permission(principal, exact=True)
    f.pay("new owner pays the remaining accrued interest and principal", principal)
    f.h.check("payment receipt separates real principal", check_receipt(f.core, f.h.svm, principal["receipt"])["principal_paid"], 20_000)
    correction = f.correction(tab, run, "governed-principal-overpayment-return", returned=35_000,
        collection=collection, repayment=principal,
        account_overrides=dict(refund_custody=wallet["custody"]))
    before = f.state("FinancierCustody", wallet["custody"])
    f.correct("funded supplier correction returns overpaid principal to historical governed source", correction)
    f.h.check("principal return credits exact governed accounted cash", f.state("FinancierCustody", wallet["custody"])["accounted_cash"], before["accounted_cash"] + 5_000)
    f.correct("historical principal return cannot duplicate its cash", correction, replay=True)


def coincident_roles_and_zero_share(f):
    f.setup()
    f.repayment_wallet("issuer", 100_000)
    issuer = f.parties["issuer"]
    alias_terms = dict(f.interest_terms(), protocol_beneficiary=issuer["principal"],
        protocol_destination=issuer["custody_token"])
    alias_line, alias_policy = f.open_line(f.facility, "coincident-roles", interest_terms=alias_terms)
    alias_tab = f.acceptance("coincident-roles", cap=10_000, maximum_interest=1_000, fees=False,
        line=alias_line, policy=alias_policy)
    f.accept(alias_tab)
    alias_run = f.earn(alias_tab, "coincident-roles-delivery", 10_000)
    zero_terms = dict(f.interest_terms(), protocol_share_bps=0)
    zero_line, zero_policy = f.open_line(f.facility, "zero-share-metadata", interest_terms=zero_terms)
    zero_tab = f.acceptance("zero-share-metadata", cap=1_000, maximum_interest=100, fees=False,
        line=zero_line, policy=zero_policy)
    f.accept(zero_tab)
    zero_run = f.earn(zero_tab, "zero-share-delivery", 1_000)
    fee_tab = f.acceptance("stable-fee-membership", cap=1_000, maximum_interest=0)
    # Proportional-only fees exercise a true rounding-to-zero first collection.
    fee_tab["args"]["seller_fee_terms"]["fixed_fee"] = 0
    f.accept(fee_tab)
    fee_run = f.earn(fee_tab, "stable-fee-delivery", 1_000)
    tiny = f.collection(fee_tab, fee_run, 1)
    f.collect(tiny, label="zero-rounded collection fee retains accepted beneficiary book")
    data = check_receipt(f.core, f.h.svm, tiny["receipt"])
    fee_effect = next(e for e in data["effects"] if e["principal"] == f.parties["fee"]["principal"])
    f.h.check("zero fee records unchanged beneficiary positions", fee_effect["before"], fee_effect["after"])
    prepared = f.collection(fee_tab, fee_run, 999)
    f.freeze(f.parties["fee"]["custody_token"])
    f.collect(prepared, label="fee destination freeze after preparation cannot change required books")
    data = check_receipt(f.core, f.h.svm, prepared["receipt"])
    f.h.check("fee waiver leaves supplier paid with actual zero deduction", data["seller_fee"], 0)
    f.h.check("fee waiver records accepted deduction separately", data["seller_fee_waived"], 13)
    f.freeze(f.parties["fee"]["custody_token"], False)
    f.advance(YEAR)
    alias = f.governed_run(alias_tab, alias_run, "coincident-payer-creditor-protocol", 1_000,
        "coincident-permission", role="issuer", maximum_interest=1_000)
    f.grant_permission(alias, exact=True)
    f.pay("one Principal can be payer creditor and protocol through distinct source custody", alias)
    data = check_receipt(f.core, f.h.svm, alias["receipt"])
    f.h.check("coincident roles deduplicate to debtor and common beneficiary books", len(data["effects"]), 2)
    f.h.check("coincident protocol custody receives real interest allocation", data["protocol_interest"], 370)
    zero = f.governed_run(zero_tab, zero_run, "zero-share-preserved-metadata", 100,
        "zero-share-permission", role="issuer", maximum_interest=100)
    f.grant_permission(zero, exact=True)
    f.pay("zero-share accepted metadata needs no protocol token or book account", zero)
    data = check_receipt(f.core, f.h.svm, zero["receipt"])
    f.h.check("zero sharing does not invent protocol income", data["protocol_interest"], 0)
    f.h.check("zero sharing retains exact distinct principal set", len(data["effects"]), 2)


def main():
    for suffix, scenario in (("run", run_authority), ("facility", facility_authority),
            ("refunds", stable_accounts_and_refunds), ("roles", coincident_roles_and_zero_share)):
        f = GovernedFixture()
        try:
            scenario(f)
            f.finish("financing-governed-repayment-" + suffix)
        except Exception as error:
            f.finish("financing-governed-repayment-" + suffix, error=error)
            raise


if __name__ == "__main__":
    main()
