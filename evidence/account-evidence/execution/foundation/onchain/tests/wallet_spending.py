#!/usr/bin/env python3
"""Signed native-v1 governed token sends and actual SPL Token Swap execution.

Independent fixture construction uses current generated IDLs and the admitted
composition contract. Synthetic local tokens and disposable signers exercise
real compiled Registry/Core/Pool/Swig/SPL programs. Customer SDK/RPC execution
and validator admission remain separate qualification work.
"""
from financing_fixture import *
from solders.compute_budget import set_compute_unit_limit
from spl_token_swap_fixture import SPL_SWAP, create_pool, load_adapter


def wallet_operation(principal, raw):
    operation = hashlib.sha256(b"DEXTER:WALLET-SPEND-ID:1" + bytes(CORE) + bytes(principal) + raw).digest()
    return operation, pda(CORE, b"wallet-operation", principal, operation)


def wallet_leg(operation, principal, leg):
    identifier = hashlib.sha256(b"DEXTER:WALLET-SPEND-LEG-ID:1" + bytes(CORE) + operation + bytes([leg])).digest()
    return pda(CORE, b"finance-operation", principal, identifier)


def wallet_digest(request):
    fields = ("principal", "binding", "wallet_authority", "source_custody", "source",
        "input_mint", "destination", "destination_authority", "output_mint",
        "output_custody", "adapter_program", "adapter_pool")
    raw = request["request_id"] + bytes([request["kind"]])
    raw += b"".join(bytes(request[field]) for field in fields)
    raw += u64(request["amount"]) + u64(request["minimum_output"]) + u64(request["authority_generation"])
    return hashlib.sha256(b"DEXTER:WALLET-SPEND:1" + bytes(CORE) + raw).digest()


def check_spending_receipt(core, svm, address):
    value = core.read(svm, address, "WalletSpendReceipt")["data"]
    request = value["request"]
    operation, expected = wallet_operation(request["principal"], request["request_id"])
    assert value["version"] == 1 and address == expected
    assert value["request_digest"] == wallet_digest(request)
    assert value["input_receipt"] == wallet_leg(operation, request["principal"], 0)
    raw = core.encode({"defined": {"name": "WalletSpendReceiptData"}}, value)
    expected_hash = hashlib.sha256(b"DEXTER:WALLET-SPEND-RESULT:1" + bytes(CORE) + raw[:-32] + bytes(32)).digest()
    assert value["receipt_hash"] == expected_hash
    input_leg = check_receipt(core, svm, value["input_receipt"])
    assert input_leg["effects_hash"] == value["input_effects_hash"]
    assert input_leg["kind"] == (24 if request["kind"] == 5 else 25)
    assert input_leg["amount"] == request["amount"] and input_leg["mint"] == request["input_mint"]
    assert input_leg["related_receipt"] == address
    assert input_leg["request_digest"] == hashlib.sha256(
        b"DEXTER:WALLET-SPEND-LEG:1" + bytes(CORE) + value["request_digest"] + bytes([0])).digest()
    if request["kind"] == 5:
        assert value["amount_out"] == request["amount"]
        assert value["output_receipt"] == Pubkey.default() and value["output_effects_hash"] == bytes(32)
    else:
        assert request["kind"] == 6 and value["amount_out"] >= request["minimum_output"]
        assert value["output_receipt"] == wallet_leg(operation, request["principal"], 1)
        output_leg = check_receipt(core, svm, value["output_receipt"])
        assert output_leg["kind"] == 26 and output_leg["effects_hash"] == value["output_effects_hash"]
        assert output_leg["amount"] == value["amount_out"] and output_leg["mint"] == request["output_mint"]
        assert output_leg["related_receipt"] == address
        assert output_leg["request_digest"] == hashlib.sha256(
            b"DEXTER:WALLET-SPEND-LEG:1" + bytes(CORE) + value["request_digest"] + bytes([1])).digest()
    return value


class WalletSpendingFixture(FinancingFixture):
    def __init__(self):
        super().__init__()
        self.asset_books = {self.mint: {
            name: {"book": party["book"], "projection": party["projection"]}
            for name, party in self.parties.items()}}
        self.spending_receipts = []

    def remaining_asset(self, mint, roles=("financier",), outage=False):
        principals = sorted({self.parties[role]["principal"] if isinstance(role, str)
            else role for role in roles}, key=bytes)
        remaining = []
        for principal in principals:
            book = pda(CORE, b"financial-book", principal, mint)
            remaining.append(M(book, w=True))
            if not outage:
                remaining.append(M(pda(POOL, b"financial-projection", book), w=True))
        return remaining

    def prepare_output_custody(self, mint, *, role="financier"):
        """Allocate an empty output ledger without relabeling proceeds as capital."""
        party = self.parties[role]
        book = pda(CORE, b"financial-book", party["principal"], mint)
        projection = pda(POOL, b"financial-projection", book)
        destination = self.h.ata(party["wallet"], mint)
        custody = pda(CORE, b"custody-reserve", destination)
        self.observe(book, projection, destination, custody)
        self.send("initialize separate output-asset financial book",
            self.ix("claims_core", "initialize_finance_book", dict(book=book, mint=mint),
                principal=party["principal"]))
        self.send("initialize output-asset Pool projection",
            self.ix("pool_issuer", "initialize_financial_projection",
                dict(book=book, projection=projection)))
        self.send("create exact governed output ATA", self.h.ata_ix(party["wallet"], mint))
        raw = ident("allocate-empty-output-custody-" + str(mint))
        receipt = self.receipt(party["principal"], self.operation(b"custody", party["principal"], raw))
        self.send("owner establishes zero-capital output custody",
            self.ix("claims_core", "create_financier_custody",
                dict(owner=party["owner"].pubkey(), financier=party["principal"],
                    wallet_binding=party["binding"], wallet_config=party["config"],
                    wallet_authority=party["wallet"], custody_token=destination,
                    custody=custody, receipt=receipt, mint=mint),
                remaining=self.remaining_asset(mint, (role,)), request_id=raw, profile=1,
                allocated_cash=0, authority_generation=party["generation"],
                provenance_hash=ident("empty-output-custody-owner-approval")),
            [party["owner"]], receipt=receipt)
        self.asset_books.setdefault(mint, {})[role] = dict(book=book, projection=projection)
        return dict(mint=mint, custody=custody, token=destination, book=book,
            projection=projection, wallet=party["wallet"], principal=party["principal"])

    def all_books(self):
        addresses = [entry["book"] for roles in self.asset_books.values() for entry in roles.values()]
        return self.h.snapshot(addresses)

    def funding_state(self):
        wallet = self.parties["financier"]
        return self.state("FinancierCustody", wallet["custody"])

    def check_reserve(self, label, *, accounted, unused, payable):
        wallet = self.parties["financier"]
        state = self.funding_state()
        for name, expected in (("accounted_cash", accounted), ("unused", unused), ("payable", payable)):
            self.h.check(label + " " + name, state[name], expected)
        self.h.check(label + " actual covers accounted",
            self.h.amount(wallet["custody_token"]) >= state["accounted_cash"], True)
        self.h.check(label + " canonical shared backing",
            state["accounted_cash"] >= state["unused"] + state["payable"], True)

    def spending_permission(self, name, role="financier"):
        return pda(REGISTRY, b"spending-permission", self.parties[role]["principal"], ident(name))

    def permission_state(self, permission):
        return self.registry.read(self.h.svm, permission, "SpendingPermission")

    def grant_spending(self, request, *, exact=False, overrides=None, owner=None, reject=False):
        party = self.parties[request.get("role", "financier")]
        kind, amount = request["kind"], request["amount"]
        terms = dict(binding=party["binding"], kind=kind, mint=request["wire"]["input_mint"],
            debit_from=request["wire"]["source"], output_mint=request["output_mint"],
            destination=request["destination"], destination_authority=request["destination_authority"],
            adapter_program=request.get("adapter_program", Pubkey.default()),
            adapter_pool=request.get("adapter_pool", Pubkey.default()),
            executor=self.h.payer.pubkey(), authority_generation=party["generation"],
            not_before=self.now, expires_at=self.now + 300, minimum_interval=0,
            per_operation_amount=amount, total_amount=amount,
            maximum_operations=1 if exact else 10,
            minimum_output_numerator=1 if kind == 6 else 0,
            minimum_output_denominator=2 if kind == 6 else 0,
            exact_request_digest=request["digest"] if exact else bytes(32),
            exact_receipt=request["receipt"] if exact else Pubkey.default(),
            exact_amount=amount if exact else 0)
        terms.update(overrides or {})
        signer = owner or party["owner"]
        permission = request["permission"]
        self.observe(permission)
        args = dict(permission_id=ident(request["permission_name"]), terms=terms)
        instruction = self.ix("principal_registry", "grant_spending_permission",
            dict(principal=party["principal"], owner=signer.pubkey(),
                binding=party["binding"], permission=permission), args=args)
        self.send("owner grants " + ("exact" if exact else "bounded delegated")
            + " spending permission " + request["permission_name"], instruction,
            [signer], reject=reject)
        if not reject:
            state = self.permission_state(permission)
            raw = bytes(party["principal"]) + ident(request["permission_name"]) + bytes(signer.pubkey())
            raw += self.registry.encode({"defined": {"name": "SpendingPermissionTerms"}}, terms)
            self.h.check("independent complete spending terms digest", state["terms_digest"],
                hashlib.sha256(b"DEXTER:SPENDING-PERMISSION:1" + bytes(REGISTRY) + raw).digest())
        return dict(instruction=instruction, permission=permission, terms=terms,
            role=request.get("role", "financier"), owner=signer)

    def revoke_spending(self, grant):
        party = self.parties[grant["role"]]
        self.send("current owner revokes delegated future spending",
            self.ix("principal_registry", "revoke_spending_permission",
                dict(principal=party["principal"], owner=party["owner"].pubkey(),
                    permission=grant["permission"]),
                args=dict(authority_generation=party["generation"])), [party["owner"]])

    def spending_bundle(self, request, *, caller=None, bad_hash=False, nonce=None,
            expires_at=None, inner=None, action=None, permission=None):
        wallet = self.parties[request.get("role", "financier")]
        caller = caller or self.h.payer
        binding = self.registry.read(self.h.svm, wallet["binding"], "FundingWalletBinding")
        kind = request["kind"] if action is None else action
        args = dict(binding=wallet["binding"], nonce=binding["nonce"] if nonce is None else nonce,
            expires_at=self.now + 300 if expires_at is None else expires_at,
            authority_generation=request["generation"], action=kind, bundle_hash=bytes(32))
        tail = compact_sign(wallet["config"], wallet["wallet"], inner or request["instruction"])
        accounts = dict(config=wallet["config"], wallet=wallet["wallet"], binding=wallet["binding"],
            principal=wallet["principal"], caller=caller.pubkey(), instructions=SYSVAR_INSTRUCTIONS)
        remaining = [M(permission or request["permission"]), M(request["receipt"])]
        authorize = self.ix("principal_registry", "authorize_funding_wallet", accounts,
            remaining=remaining, args=args)
        raw = canonical_instruction(tail, [authorize, tail], self.h.payer.pubkey())
        args["bundle_hash"] = bytes([99]) * 32 if bad_hash else hashlib.sha256(
            b"DEXTER:FUNDING-SWIG-BUNDLE:1" + bytes(REGISTRY) + bytes(CORE) + bytes(SWIG)
            + self.domain + bytes(wallet["principal"]) + bytes(wallet["binding"])
            + u64(args["nonce"]) + struct.pack("<q", args["expires_at"])
            + u64(request["generation"]) + bytes([kind]) + raw).digest()
        return [self.ix("principal_registry", "authorize_funding_wallet", accounts,
            remaining=remaining, args=args), tail]

    def spending_request(self, name, amount, permission, *, swap=False, minimum_output=0,
            destination=None, destination_authority=None, role="financier", generation=None,
            account_overrides=None, arg_overrides=None, remaining=None, reverse=False):
        party = self.parties[role]
        kind = 6 if swap else 5
        raw = ident(name)
        generation = party["generation"] if generation is None else generation
        identifier, operation = wallet_operation(party["principal"], raw)
        input_receipt = wallet_leg(identifier, party["principal"], 0)
        grant = self.spending_permission(permission, role)
        args = dict(request_id=raw, amount=amount, authority_generation=generation)
        accounts = dict(executor=self.h.payer.pubkey(), wallet_authority=party["wallet"],
            principal=party["principal"], binding=party["binding"], permission=grant,
            source_custody=party["custody"], source=party["custody_token"], input_mint=self.mint,
            operation=operation, input_receipt=input_receipt)
        if swap:
            pool_source, pool_destination, mint_in, mint_out = self.swap_pool.oriented(reverse)
            receiving = dict(custody=party["custody"], token=party["custody_token"]) if reverse else self.output
            if reverse:
                accounts.update(source_custody=self.output["custody"], source=self.output["token"], input_mint=mint_in)
            accounts.update(output_custody=receiving["custody"],
                destination=destination or receiving["token"], output_mint=mint_out,
                output_receipt=wallet_leg(identifier, party["principal"], 1), swap_program=SPL_SWAP,
                swap_state=self.swap_pool.state, swap_authority=self.swap_pool.authority,
                pool_source=pool_source, pool_destination=pool_destination,
                pool_mint=self.swap_pool.pool_mint, pool_fee=self.swap_pool.fee_account)
            args["minimum_output"] = minimum_output
            books = self.remaining_asset(mint_in, (role,)) + self.remaining_asset(mint_out, (role,))
        else:
            accounts["destination"] = destination or self.send_destination
            args["destination_authority"] = destination_authority or self.send_owner.pubkey()
            books = self.remaining_asset(self.mint, (role,))
        accounts.update(account_overrides or {})
        args.update(arg_overrides or {})
        wire = dict(request_id=args["request_id"], kind=kind, principal=accounts["principal"],
            binding=accounts["binding"], wallet_authority=accounts["wallet_authority"],
            source_custody=accounts["source_custody"], source=accounts["source"],
            input_mint=accounts["input_mint"], destination=accounts["destination"],
            destination_authority=party["wallet"] if swap else args["destination_authority"],
            output_mint=accounts["output_mint"] if swap else accounts["input_mint"],
            output_custody=accounts["output_custody"] if swap else Pubkey.default(),
            adapter_program=accounts["swap_program"] if swap else Pubkey.default(),
            adapter_pool=accounts["swap_state"] if swap else Pubkey.default(),
            amount=args["amount"], minimum_output=args.get("minimum_output", 0),
            authority_generation=args["authority_generation"])
        self.observe(operation, input_receipt, grant)
        if swap:
            self.observe(accounts["output_receipt"])
        instruction = self.ix("claims_core", "wallet_swap_exact_input" if swap else "wallet_send",
            accounts, remaining=books if remaining is None else remaining, args=args)
        return dict(instruction=instruction, accounts=accounts, args=args, wire=wire,
            kind=kind, amount=args["amount"], minimum_output=wire["minimum_output"], role=role,
            generation=args["authority_generation"], permission_name=permission, permission=grant,
            receipt=operation, input_receipt=input_receipt, digest=wallet_digest(wire),
            output_mint=wire["output_mint"], destination=wire["destination"],
            destination_authority=wire["destination_authority"],
            adapter_program=wire["adapter_program"], adapter_pool=wire["adapter_pool"])

    def record_spending_receipts(self, request):
        data = check_spending_receipt(self.core, self.h.svm, request["receipt"])
        for address in (data["input_receipt"], data["output_receipt"]):
            if address == Pubkey.default():
                continue
            receipt = check_receipt(self.core, self.h.svm, address)
            if str(address) not in [entry["address"] for entry in self.receipts]:
                for effect in receipt["effects"]:
                    current = self.state("FinanceBook", effect["book"])["data"]
                    self.h.check("wallet leg complete book positions", current["positions"], effect["after"])
                    self.h.check("wallet leg exact book hash", current["head"], effect["hash"])
                self.receipts.append(dict(address=str(address), data=copy.deepcopy(receipt)))
        if str(request["receipt"]) not in [entry["address"] for entry in self.spending_receipts]:
            self.spending_receipts.append(dict(address=str(request["receipt"]), data=copy.deepcopy(data)))
        return data

    def spend(self, label, request, *, reject=False, replay=False, error=None, **envelope):
        wallet = self.parties[request["role"]]
        source_custody, source = request["accounts"]["source_custody"], request["accounts"]["source"]
        output_custody = request["accounts"].get("output_custody")
        before_permission = self.permission_state(request["permission"])
        before_binding = self.registry.read(self.h.svm, wallet["binding"], "FundingWalletBinding")
        before_custody = self.state("FinancierCustody", source_custody)
        before_source = self.h.amount(source)
        before_destination = self.h.amount(request["destination"])
        before_books = self.all_books()
        before_output = self.state("FinancierCustody", output_custody) if request["kind"] == 6 else None
        bundle = self.spending_bundle(request, **envelope)
        caller = envelope.get("caller", self.h.payer)
        self.send(label, bundle, [] if caller.pubkey() == self.h.payer.pubkey() else [caller],
            reject=reject, error=error)
        after_permission = self.permission_state(request["permission"])
        if reject or replay:
            self.h.check(label + " preserves permission", after_permission, before_permission)
            self.h.check(label + " preserves funding nonce",
                self.registry.read(self.h.svm, wallet["binding"], "FundingWalletBinding")["nonce"], before_binding["nonce"])
            self.h.check(label + " preserves source custody", self.state("FinancierCustody", source_custody), before_custody)
            self.h.check(label + " preserves source tokens", self.h.amount(source), before_source)
            self.h.check(label + " preserves receiving tokens", self.h.amount(request["destination"]), before_destination)
            self.h.check(label + " preserves every asset book", self.all_books(), before_books)
            if before_output is not None:
                self.h.check(label + " preserves output custody", self.state("FinancierCustody", output_custody), before_output)
        else:
            receipt = self.record_spending_receipts(request)
            self.h.check(label + " exact independently encoded request", receipt["request_digest"], request["digest"])
            self.h.check(label + " exact input debit", before_source - self.h.amount(source), request["amount"])
            self.h.check(label + " actual recipient credit", self.h.amount(request["destination"]) - before_destination, receipt["amount_out"])
            current = self.state("FinancierCustody", source_custody)
            self.h.check(label + " exact source accounted debit", before_custody["accounted_cash"] - current["accounted_cash"], request["amount"])
            for field in ("unused", "payable", "contributed_capital"):
                self.h.check(label + " preserves source " + field, current[field], before_custody[field])
            self.h.check(label + " consumes bounded amount", after_permission["consumed"]["amount"],
                before_permission["consumed"]["amount"] + request["amount"])
            self.h.check(label + " consumes one operation", after_permission["consumed"]["operations"],
                before_permission["consumed"]["operations"] + 1)
            if before_output is not None:
                current_output = self.state("FinancierCustody", output_custody)
                self.h.check(label + " exact output custody credit", current_output["accounted_cash"] - before_output["accounted_cash"], receipt["amount_out"])
                for field in ("unused", "payable", "contributed_capital", "withdrawn_cash"):
                    self.h.check(label + " preserves output " + field, current_output[field], before_output[field])
        if replay:
            self.record_spending_receipts(request)
        return bundle

    def prepare_send_destination(self):
        self.send_owner = key(101)
        self.send_destination = self.h.ata(self.send_owner.pubkey(), self.mint)
        self.observe(self.send_destination)
        self.send("create independently owned send recipient ATA", self.h.ata_ix(self.send_owner.pubkey(), self.mint))

    def prepare_swap(self):
        self.adapter_pin, self.adapter_build = load_adapter(self.h)
        output_mint, _ = self.h.make_mint(key(102), initial=20_000_000)
        self.output = self.prepare_output_custody(output_mint)
        self.swap_pool = create_pool(self.h, self.mint, output_mint, label="wallet-spending")
        self.observe(*self.swap_pool.accounts(), self.swap_pool.authority)

    def finish(self, name, *, error=None):
        summary = super().finish(name, error=error)
        path = Path(summary["trace"])
        trace = json.loads(path.read_text())
        trace["spending_receipts"] = self.spending_receipts
        trace["scope"] = "Signed native-v1 compiled Registry/Core/Pool/Swig/SPL sends and source-built official SPL Token Swap"
        if hasattr(self, "adapter_pin"):
            trace["adapter_pin"] = self.adapter_pin
            trace["adapter_build"] = self.adapter_build
        path.write_text(json.dumps(trace, indent=2, default=json_value) + "\n")
        summary["trace_sha256"] = digest(path)
        summary["spending_receipts"] = len(self.spending_receipts)
        (OUT / (name + "-receipt.json")).write_text(json.dumps(summary, indent=2) + "\n")
        print(json.dumps(summary))
        return summary


def send_authority(f):
    f.setup()
    f.prepare_send_destination()
    wallet = f.parties["financier"]
    exact = f.spending_request("exact-owner-send", 20_000, "exact-send-approval")
    approval = f.grant_spending(exact, exact=True)
    f.spend("unapproved executor cannot use exact owner approval", exact, caller=f.agent2, reject=True)
    f.spend("changed signed bundle hash cannot authorize an ordinary send", exact, bad_hash=True, reject=True)
    f.send("bare Swig token-send composition cannot bypass Registry approval",
        compact_sign(wallet["config"], wallet["wallet"], exact["instruction"]), reject=True)
    f.send("native-v1 spending rejects a provisional ComputeBudget prefix",
        [set_compute_unit_limit(1_400_000), *f.spending_bundle(exact)], reject=True)
    f.spend("repayment action cannot turn a send into repayment authority", exact, action=3, reject=True)
    changed = f.spending_request("exact-owner-send", 20_001, "exact-send-approval")
    f.spend("exact owner approval refuses a changed debit before execution", changed, reject=True)

    # A token account's public key alone does not authenticate its current owner.
    temporary_owner = key(103)
    set_owner = lambda owner: bytes([6, 2, 1]) + bytes(owner)
    f.send("recipient cooperatively changes the existing token account authority",
        Instruction(TOKEN, set_owner(temporary_owner.pubkey()),
            [M(f.send_destination, w=True), M(f.send_owner.pubkey(), True)]), [f.send_owner])
    f.spend("same receiving address with changed token authority is refused", exact, reject=True)
    f.send("recipient restores the explicitly approved token account authority",
        Instruction(TOKEN, set_owner(f.send_owner.pubkey()),
            [M(f.send_destination, w=True), M(temporary_owner.pubkey(), True)]), [temporary_owner])

    nonce = f.registry.read(f.h.svm, wallet["binding"], "FundingWalletBinding")["nonce"]
    f.spend("explicit owner approval transfers exactly the approved asset and amount", exact)
    consumed = f.permission_state(approval["permission"])
    f.send("replaying approval enrollment cannot replenish exact authority", approval["instruction"], [approval["owner"]])
    f.h.check("enrollment retry retains consumed exact approval", f.permission_state(approval["permission"]), consumed)
    f.spend("a populated operation identity refuses conflicting request bytes", changed, reject=True)
    f.spend("exact approval cannot authorize a second economic identity",
        f.spending_request("new-id-under-used-exact-approval", 20_000, "exact-send-approval"), reject=True)
    f.revoke_spending(approval)
    f.spend("immutable send receipt recovers after revocation and envelope expiry",
        exact, replay=True, nonce=nonce, expires_at=f.now - 1)

    first = f.spending_request("delegated-send-one", 30_000, "bounded-send-authority")
    grant = f.grant_spending(first, overrides=dict(total_amount=50_000, maximum_operations=2, minimum_interval=10))
    f.spend("per-operation ceiling refuses one excess token",
        f.spending_request("delegated-too-large", 30_001, "bounded-send-authority"), reject=True)
    f.spend("bounded executor sends the first permitted amount", first)
    second = f.spending_request("delegated-send-two", 20_000, "bounded-send-authority")
    f.spend("standing permission enforces minimum time between operations", second, reject=True)
    f.advance(10)
    f.spend("standing permission admits the second amount at the time boundary", second)
    f.spend("cumulative authority cannot be refilled by a new request id",
        f.spending_request("delegated-send-three", 1, "bounded-send-authority"), reject=True)
    f.h.check("bounded standing permission consumed the complete amount", f.permission_state(grant["permission"])["consumed"]["amount"], 50_000)

    counted = f.spending_request("count-limited-first", 1, "count-limited-permission")
    f.grant_spending(counted, overrides=dict(per_operation_amount=100, total_amount=1_000, maximum_operations=1))
    f.spend("standing permission consumes a single operation below its amount ceiling", counted)
    f.spend("operation count independently bounds unused monetary allowance",
        f.spending_request("count-limited-second", 1, "count-limited-permission"), reject=True)

    future = f.spending_request("time-window-first", 1, "time-window-permission")
    f.grant_spending(future, overrides=dict(not_before=f.now + 1, expires_at=f.now + 2, total_amount=2))
    f.spend("standing permission cannot start before its not-before bound", future, reject=True)
    f.advance(1)
    f.spend("standing permission begins at its exact not-before bound", future)
    f.advance(1)
    f.spend("standing permission expires at its exclusive time boundary",
        f.spending_request("time-window-second", 1, "time-window-permission"), reject=True)

    revoked = f.spending_request("unused-revoked-send", 1, "unused-revoked-permission")
    revoked_grant = f.grant_spending(revoked)
    f.revoke_spending(revoked_grant)
    f.spend("revocation prevents a previously unused delegated send", revoked, reject=True)

    # Real, valid repayment permission accounts cannot substitute for this domain.
    tab = f.acceptance("repayment-domain-separation", cap=1_000, maximum_interest=0, fees=False)
    f.accept(tab)
    run = f.earn(tab, "repayment-domain-separation", 1_000)
    issuer = f.parties["issuer"]
    repayment_id = ident("actual-repayment-permission-cannot-send")
    repayment_permission = pda(REGISTRY, b"repayment-permission", wallet["principal"], repayment_id)
    f.observe(repayment_permission)
    terms = dict(binding=wallet["binding"], kind=3, obligation=run["run"], obligor=f.company,
        mint=f.mint, debit_from=wallet["custody_token"], creditor=issuer["principal"],
        creditor_custody=issuer["custody"], creditor_destination=issuer["custody_token"],
        protocol_beneficiary=f.parties["protocol"]["principal"],
        protocol_destination=f.parties["protocol"]["custody_token"],
        executor=f.h.payer.pubkey(), authority_generation=wallet["generation"],
        not_before=f.now, expires_at=f.now + 300, minimum_interval=0,
        per_payment_amount=1_000, per_payment_principal=1_000, per_payment_interest=0,
        total_amount=1_000, total_principal=1_000, total_interest=0, maximum_payments=1,
        exact_request_digest=bytes(32), exact_amount=0, exact_receipt=Pubkey.default())
    f.send("owner grants an actual repayment-only permission",
        f.ix("principal_registry", "grant_repayment_permission",
            dict(principal=wallet["principal"], owner=wallet["owner"].pubkey(),
                binding=wallet["binding"], permission=repayment_permission),
            args=dict(permission_id=repayment_id, terms=terms)), [wallet["owner"]])
    isolated = f.spending_request("repayment-grant-as-send", 1, "separate-send-domain")
    f.grant_spending(isolated)
    inner = f.ix("claims_core", "wallet_send",
        {**isolated["accounts"], "permission": repayment_permission},
        remaining=f.remaining_asset(f.mint), args=isolated["args"])
    f.spend("repayment permission account cannot implicitly authorize an ordinary send",
        isolated, inner=inner, permission=repayment_permission, reject=True)

    foreign = f.spending_request("foreign-principal-capital", 1, "foreign-capital-attempt",
        account_overrides=dict(source_custody=issuer["custody"], source=issuer["custody_token"]))
    f.grant_spending(foreign, overrides=dict(debit_from=wallet["custody_token"]))
    f.spend("spending approval cannot use another principal's capital", foreign, reject=True)

    stale = f.spending_request("stale-owner-send", 1, "pre-rotation-unused-permission")
    f.grant_spending(stale)
    old_owner = f.rotate_financier()
    f.spend("owner rotation invalidates an old-generation unused permission", stale, reject=True)
    fresh_generation = f.spending_request("same-grant-new-generation", 1, "pre-rotation-unused-permission")
    f.spend("updating request generation cannot revive an old owner's grant", fresh_generation, reject=True)
    f.spend("historical send remains read-only after cooperative owner rotation", exact, replay=True, nonce=nonce, expires_at=f.now - 1)
    rejected_owner = f.spending_request("old-owner-new-grant", 1, "old-owner-new-grant")
    f.grant_spending(rejected_owner, owner=old_owner, reject=True)
    current = f.spending_request("current-owner-send", 1, "current-owner-approval")
    f.grant_spending(current, exact=True)
    f.spend("replacement owner explicitly approves a fresh send", current)


def shared_custody_spending(f):
    f.setup()
    f.prepare_send_destination()
    wallet = f.parties["financier"]
    second_facility = f.open_facility(name="spending-shared-second")
    second_line, second_policy = f.open_line(second_facility, "spending-shared-second")
    second_mandate = f.grant(second_facility, name="spending-shared-second")
    first_tab = f.acceptance("spending-shared-first", cap=120_000, maximum_interest=0, fees=False)
    second_tab = f.acceptance("spending-shared-second", cap=180_000, maximum_interest=0, fees=False,
        facility=second_facility, line=second_line, policy=second_policy, mandate=second_mandate)
    f.accept(first_tab)
    f.accept(second_tab)
    f.check_reserve("two independently accepted facilities", accounted=1_000_000, unused=300_000, payable=0)
    run = f.earn(first_tab, "spending-shared-earned", 100_000)
    f.check_reserve("earned work changes reserve category without freeing cash", accounted=1_000_000, unused=200_000, payable=100_000)

    first = f.spending_request("competing-spend-one", 400_000, "shared-bounded-spend")
    second = f.spending_request("competing-spend-two", 400_000, "shared-bounded-spend")
    f.grant_spending(first, overrides=dict(total_amount=800_000, maximum_operations=2))
    # These are competing prepared transactions executed serially under account
    # locks, not a claim of validator throughput or simultaneous RPC admission.
    prepared_second = f.spending_bundle(second)
    prepared_tx = f.h.transaction(prepared_second)
    f.spend("first prepared operation spends eligible shared-custody cash", first)
    f.h.send("competing signed operation cannot reuse the old wallet nonce", prepared_second,
        fail="any", unchanged=f.h.observe, transaction=prepared_tx)
    f.check_reserve("first spend preserves both facilities", accounted=600_000, unused=200_000, payable=100_000)
    f.spend("fresh authorization still refuses spending another facility's backing", second, reject=True)
    last_free = f.spending_request("spend-exactly-shared-free-balance", 300_000, "shared-bounded-spend")
    f.spend("bounded permission spends exactly the remaining uncommitted balance", last_free)
    f.check_reserve("all remaining wallet cash is canonical accepted backing", accounted=300_000, unused=200_000, payable=100_000)
    extra = f.spending_request("new-grant-cannot-consume-backing", 1, "extra-reserve-permission")
    f.grant_spending(extra, exact=True)
    f.spend("a separate exact owner approval cannot spend one committed token", extra, reject=True)
    f.withdraw(f.withdrawal("withdrawal-shares-spending-floor", 1), reject=True,
        label="ordinary withdrawal preserves the same canonical reserve floor")

    company = f.parties["company"]
    f.send("owner revokes the initiating agent after work was earned",
        f.ix("principal_registry", "revoke_actor",
            dict(owner=company["owner"].pubkey(), principal=f.company, actor=f.actors[0]),
            expected_generation=1), [company["owner"]])
    f.manage_facility("close-new-admission-after-spending", limit=0, enabled=False)
    f.manage_facility("close-second-facility-admission", limit=0, enabled=False, facility=second_facility)
    f.rotate_financier()
    collect = f.collection(first_tab, run, 100_000, outage=True)
    bundle = f.collect(collect, label="independent collector services accepted rights without initiating agent or financier backend")
    tx = VersionedTransaction.from_bytes(base64.b64decode(f.h.cases[-1]["transaction_base64"]))
    f.h.check("accepted collection needs only the independent transaction sponsor", len(tx.signatures), 1)
    f.h.check("collector has no fresh financier approval", any(m.pubkey == wallet["owner"].pubkey() and m.is_signer
        for ix in bundle for m in ix.accounts), False)
    f.check_reserve("earned collection reduces payable and backing together", accounted=200_000, unused=200_000, payable=0)
    f.h.check("supplier payment preserves company debt", f.state("FundedClaim", first_tab["claim"])["debt"], 100_000)
    f.catch_up()
    f.release(first_tab, "remaining-first-accepted-work", 20_000)
    f.release(second_tab, "remaining-second-accepted-work", 180_000)
    f.check_reserve("accepted release frees only the corresponding commitments", accounted=200_000, unused=0, payable=0)
    current_destination = f.h.ata(wallet["owner"].pubkey(), f.mint)
    f.observe(current_destination)
    f.send("replacement owner prepares its ordinary withdrawal destination", f.h.ata_ix(wallet["owner"].pubkey(), f.mint))
    f.withdraw(f.withdrawal("current-owner-free-after-release", 200_000),
        label="current owner retains ordinary withdrawal of released uncommitted funds")
    f.check_reserve("ordinary withdrawal and sends share one custody accounting", accounted=0, unused=0, payable=0)


def actual_adapter_swaps(f):
    f.setup()
    f.prepare_send_destination()
    f.prepare_swap()
    wallet = f.parties["financier"]
    amount, output = f.swap_pool.exact_input(f.h, 10_000)
    exact = f.spending_request("exact-owner-swap", amount, "exact-swap-approval", swap=True, minimum_output=output)
    approval = f.grant_spending(exact, exact=True)
    f.spend("owner-approved exact-input swap executes the actual source-built adapter", exact)
    f.h.check("independent constant-product quote equals measured output",
        check_spending_receipt(f.core, f.h.svm, exact["receipt"])["amount_out"], output)
    f.h.check("actual SPL Token Swap was invoked inside governed Core",
        "Program " + str(SPL_SWAP) + " invoke [3]" in json.dumps(f.h.cases[-1]["result"]), True)
    f.h.check("actual adapter used SPL Token at invocation depth four",
        "Program " + str(TOKEN) + " invoke [4]" in json.dumps(f.h.cases[-1]["result"]), True)

    reverse_amount, reverse_output = f.swap_pool.exact_input(f.h, 1_000, reverse=True)
    reverse = f.spending_request("exact-owner-reverse-swap", reverse_amount, "reverse-swap-approval",
        swap=True, reverse=True, minimum_output=reverse_output)
    f.grant_spending(reverse, exact=True)
    f.spend("governed reverse swap spends owned proceeds and credits the original asset custody", reverse)

    amount, output = f.swap_pool.exact_input(f.h, 12_000)
    delegated = f.spending_request("bounded-swap-one", amount, "bounded-swap-permission", swap=True, minimum_output=output)
    delegated_grant = f.grant_spending(delegated,
        overrides=dict(per_operation_amount=20_000, total_amount=50_000, maximum_operations=4,
            minimum_output_numerator=9, minimum_output_denominator=10))
    low = f.spending_request("caller-lowered-owner-rate", amount, "bounded-swap-permission", swap=True,
        minimum_output=(amount * 9 + 9) // 10 - 1)
    f.spend("executor cannot weaken the owner's minimum output rate", low, reject=True)
    high = f.spending_request("adapter-minimum-output-refusal", amount, "bounded-swap-permission", swap=True, minimum_output=output + 1)
    f.spend("actual adapter refuses an unattainable minimum output", high, reject=True,
        error="Swap instruction exceeds desired slippage limit")
    f.spend("bounded delegated swap commits the actual debit and output", delegated)

    next_amount, next_output = f.swap_pool.exact_input(f.h, 10_000)
    diverted = f.spending_request("swap-output-diversion", next_amount, "bounded-swap-permission", swap=True,
        minimum_output=next_output, destination=f.h.ata(f.send_owner.pubkey(), f.output["mint"]))
    f.observe(diverted["destination"])
    f.send("prepare another owner's output account for routing refusal", f.h.ata_ix(f.send_owner.pubkey(), f.output["mint"]))
    f.spend("approved adapter identity cannot authorize a different receiving account", diverted, reject=True)
    wrong_pool = f.spending_request("swap-wrong-pool", next_amount, "bounded-swap-permission", swap=True,
        minimum_output=next_output, account_overrides=dict(swap_state=f.parties["issuer"]["custody"]))
    f.spend("standing swap permission binds the actual pool identity", wrong_pool, reject=True)
    wrong_vault = f.spending_request("swap-wrong-input-vault", next_amount, "bounded-swap-permission", swap=True,
        minimum_output=next_output, account_overrides=dict(pool_source=f.parties["issuer"]["custody_token"]))
    f.spend("a correct router and pool cannot conceal a different input vault", wrong_vault, reject=True)
    f.spend("a send action cannot use bounded swap permission", delegated, action=5, reject=True)

    rounded_amount = next(n for n in range(10_000, 20_000)
        if (f.swap_pool.quote(f.h, n) or {}).get("actual_input", n) < n)
    quote = f.swap_pool.quote(f.h, rounded_amount)
    rounded = f.spending_request("actual-adapter-rounded-underdebit", rounded_amount, "rounded-underdebit-permission",
        swap=True, minimum_output=quote["output"])
    f.grant_spending(rounded, exact=True)
    f.spend("successful adapter CPI with smaller actual input debit is rolled back",
        rounded, reject=True, error="WalletSwapOutcome")
    logs = json.dumps(f.h.cases[-1]["result"])
    f.h.check("real rounding refusal occurs after successful actual adapter execution", "Program " + str(SPL_SWAP) + " success" in logs, True)
    f.h.check("actual adapter rounding differs from approved exact input", quote["actual_input"] < rounded_amount, True)

    # Accepted servicing may advance authoritative books with Pool unavailable.
    # New spending must roll back when that exact, correctly addressed projection
    # is stale, even after token movement and permission consumption have run.
    tab = f.acceptance("swap-late-booking", cap=50_000, maximum_interest=0, fees=False)
    f.accept(tab)
    second_facility = f.open_facility(name="swap-shared-second")
    second_line, second_policy = f.open_line(second_facility, "swap-shared-second")
    second_mandate = f.grant(second_facility, name="swap-shared-second")
    second_tab = f.acceptance("swap-shared-second", cap=20_000, maximum_interest=0, fees=False,
        facility=second_facility, line=second_line, policy=second_policy, mandate=second_mandate)
    f.accept(second_tab)
    f.earn(tab, "swap-late-booking-delivery", 20_000, outage=True)
    late_amount, late_output = f.swap_pool.exact_input(f.h, 10_000)
    late = f.spending_request("swap-late-projection-rollback", late_amount, "late-booking-approval", swap=True, minimum_output=late_output)
    f.grant_spending(late, exact=True)
    f.spend("stale exact Pool projection rolls back adapter effects and permission consumption", late, reject=True)
    logs = json.dumps(f.h.cases[-1]["result"])
    f.h.check("late refusal follows successful actual adapter CPI", "Program " + str(SPL_SWAP) + " success" in logs, True)
    f.h.check("late refusal follows Registry permission consumption", "Instruction: ConsumeSpendingPermission" in logs, True)
    f.catch_up()
    f.spend("same operation succeeds after authenticated projection catch-up", late)

    # An owner-approved swap may not use committed input cash, irrespective of
    # how much valuable output the adapter would produce for the same wallet.
    state = f.funding_state()
    all_free = state["accounted_cash"] - state["unused"] - state["payable"]
    f.withdraw(f.withdrawal("leave-only-swap-committed-cash", all_free))
    protected_amount, protected_output = f.swap_pool.exact_input(f.h, 1_000)
    protected = f.spending_request("swap-committed-input-refusal", protected_amount, "committed-input-approval",
        swap=True, minimum_output=protected_output)
    f.grant_spending(protected, exact=True)
    f.spend("owner-approved swap cannot convert accepted supplier backing into another asset", protected, reject=True)
    f.check_reserve("swap leaves both facilities' accepted input-asset promises intact", accounted=70_000, unused=50_000, payable=20_000)
    f.revoke_spending(approval)
    f.rotate_financier()
    f.spend("historical swap result recovers after revocation rotation and changed pool price",
        exact, replay=True, nonce=0, expires_at=f.now - 1)
    f.h.check("swapped proceeds are cash receipts without invented capital contribution",
        f.state("FinancierCustody", f.output["custody"])["contributed_capital"], 0)


def main():
    for suffix, scenario in (("send", send_authority), ("shared", shared_custody_spending),
            ("swap", actual_adapter_swaps)):
        f = WalletSpendingFixture()
        try:
            scenario(f)
            f.finish("wallet-spending-" + suffix)
        except Exception as error:
            f.finish("wallet-spending-" + suffix, error=error)
            raise


if __name__ == "__main__":
    main()

