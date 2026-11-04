"""Shared scenario setup for the newly executed financing acceptance suites."""
from __future__ import annotations
import copy
import hashlib
import json
import struct
from pathlib import Path

from solders.clock import Clock
from solders.instruction import Instruction
from solders.pubkey import Pubkey
from solders.system_program import transfer, TransferParams

from financing_support import *

YEAR = 31_536_000


class FinancingFixture:
    def __init__(self, *, artifact_root=ROOT):
        fixture_paths = {Path(module.__file__).resolve() for module in tuple(sys.modules.values())
            if getattr(module, "__file__", None) and str(module.__file__).endswith(".py")
            and Path(module.__file__).resolve().is_relative_to(ROOT)
            and ".local" not in Path(module.__file__).resolve().relative_to(ROOT).parts}
        self.fixture_inputs = {str(path.relative_to(ROOT)): digest(path) for path in sorted(fixture_paths)}
        self.h = V1Harness("financing-reconstruction")
        self.artifact_root = Path(artifact_root)
        self.idls, self.build, self.idl_receipt = load_programs(self.h, self.artifact_root)
        self.registry, self.core, self.pool = [self.idls[name] for name in PROGRAMS]
        self.domain = ident("financing-reconstruction-domain")
        self.parties = {}
        self.protected = set()
        self.receipts = []
        self.now = NOW
        for name, seed in zip(("company", "issuer", "financier", "supplier", "fee", "protocol", "assignee", "sponsor"), range(20, 28)):
            owner = key(seed)
            self.h.svm.airdrop(owner.pubkey(), 100_000_000_000)
            principal_id = ident("principal-" + name)
            principal = pda(REGISTRY, b"principal", owner.pubkey(), principal_id)
            self.parties[name] = dict(name=name, owner=owner, principal_id=principal_id, principal=principal, generation=1)
            self.observe(principal)
        self.agent, self.agent2, self.replacement_owner = key(30), key(31), key(32)
        self.h.svm.airdrop(self.replacement_owner.pubkey(), 100_000_000_000)
        self.mint, self.funder = self.h.make_mint(key(33), initial=20_000_000)
        self.asset = dict(mint=self.mint, token_program=TOKEN)
        self.company = self.parties["company"]["principal"]
        self.budget = pda(REGISTRY, b"budget", self.company, self.mint)
        self.registry_authority = pda(CORE, b"registry-consumer")
        self.actors = [pda(REGISTRY, b"actor", self.company, ident("actor-" + str(i))) for i in (1, 2)]
        self.mandates = [pda(REGISTRY, b"mandate", self.company, ident("mandate-" + str(i))) for i in (1, 2)]
        self.tabs = {}
        self.fee_agreements = {}
        self.observe(self.funder, self.budget, *self.actors, *self.mandates)
        for party in self.parties.values():
            party["book"] = pda(CORE, b"financial-book", party["principal"], self.mint)
            party["projection"] = pda(POOL, b"financial-projection", party["book"])
            party["owner_ata"] = self.h.ata(party["owner"].pubkey(), self.mint)
            self.observe(party["book"], party["projection"], party["owner_ata"])

    def observe(self, *addresses):
        self.protected.update(addresses)
        self.h.observe = sorted(self.protected, key=bytes)

    def common(self):
        return dict(payer=self.h.payer.pubkey(), mint=self.mint, token_program=TOKEN,
            associated_token_program=ATA, system_program=SYSTEM, pool_program=POOL,
            registry_program=REGISTRY, registry_authority=self.registry_authority,
            repayment_binding=None, repayment_custody=None, repayment_permission=None,
            refund_custody=None)

    def ix(self, program, name, accounts, *, remaining=(), **args):
        return self.idls[program].instruction(name, {**self.common(), **accounts}, remaining=remaining, **args)

    def send(self, label, instruction, signers=(), *, reject=False, error=None, receipt=None):
        instructions = list(instruction) if isinstance(instruction, (list, tuple)) else [instruction]
        if receipt is not None:
            self.observe(receipt)
        result = self.h.send(label, instructions, signers, fail=error or ("any" if reject else None),
            unchanged=self.h.observe if reject or error else ())
        if receipt is not None and not reject and not error:
            data = check_receipt(self.core, self.h.svm, receipt)
            if str(receipt) not in [entry["address"] for entry in self.receipts]:
                for effect in data["effects"]:
                    book = self.core.read(self.h.svm, effect["book"], "FinanceBook")["data"]
                    self.h.check(label + " complete book frontier", book["sequence"], effect["sequence"])
                    self.h.check(label + " complete book amounts", book["positions"], effect["after"])
                    self.h.check(label + " complete book hash", book["head"], effect["hash"])
                self.receipts.append({"address": str(receipt), "data": copy.deepcopy(data)})
        return result

    def remaining(self, roles, outage=False):
        principals = sorted({self.parties[r]["principal"] if isinstance(r, str) else r for r in roles}, key=bytes)
        result = []
        for principal in principals:
            book = pda(CORE, b"financial-book", principal, self.mint)
            result.append(M(book, w=True))
            if not outage:
                result.append(M(pda(POOL, b"financial-projection", book), w=True))
        return result

    def operation(self, tag, scope, raw):
        operation_id = hashlib.sha256(b"DEXTER:FUNDING-OPERATION:1" + bytes(CORE) + tag + bytes(scope) + raw).digest()
        return operation_id

    def receipt(self, principal, operation_id):
        return pda(CORE, b"finance-operation", principal, operation_id)

    def transfer_tokens(self, label, destination, amount, *, source=None, authority=None):
        authority = authority or self.h.payer
        source = source or self.funder
        instruction = Instruction(TOKEN, bytes([12]) + u64(amount) + bytes([6]),
            [M(source, w=True), M(self.mint), M(destination, w=True), M(authority.pubkey(), True)])
        return self.send(label, instruction, [authority])

    def state(self, kind, address):
        return self.core.read(self.h.svm, address, kind)

    def positions(self, role, **expected):
        actual = self.state("FinanceBook", self.parties[role]["book"])["data"]["positions"]
        for field, amount in expected.items():
            self.h.check(role + " financial position " + field, actual[field], amount)

    def freeze(self, token, frozen=True):
        self.send("freeze exact accepted fee recipient" if frozen else "restore accepted fee recipient",
            Instruction(TOKEN, bytes([10 if frozen else 11]),
                [M(token, w=True), M(self.mint), M(self.h.payer.pubkey(), True)]))

    def setup_principals(self):
        for party in self.parties.values():
            self.send("register " + party["name"] + " continuing principal",
                self.ix("principal_registry", "create_principal",
                    dict(owner=party["owner"].pubkey(), principal=party["principal"]),
                    args=dict(principal_id=party["principal_id"], domain=self.domain, kind=1,
                        governing_policy=ident("single-owner-ed25519-profile"))), [party["owner"]])
            self.send("initialize " + party["name"] + " canonical financial book",
                self.ix("claims_core", "initialize_finance_book", dict(book=party["book"]), principal=party["principal"]))
            self.send("initialize " + party["name"] + " zero-frontier Pool projection",
                self.ix("pool_issuer", "initialize_financial_projection",
                    dict(book=party["book"], projection=party["projection"])))
            self.send("create " + party["name"] + " owner token account",
                self.h.ata_ix(party["owner"].pubkey(), self.mint))
        for role, amount in (("company", 200_000), ("issuer", 200_000), ("assignee", 200_000), ("fee", 10_000)):
            self.transfer_tokens("fund only synthetic " + role + " owner cash", self.parties[role]["owner_ata"], amount)
        company = self.parties["company"]
        self.send("owner initializes company allowance", self.ix("principal_registry", "create_budget",
            dict(owner=company["owner"].pubkey(), principal=self.company, budget=self.budget),
            args=dict(asset=self.mint, ceiling=500_000)), [company["owner"]])
        for index, agent in enumerate((self.agent, self.agent2)):
            self.send("owner creates actor " + str(index + 1), self.ix("principal_registry", "create_actor",
                dict(owner=company["owner"].pubkey(), principal=self.company, actor=self.actors[index]),
                args=dict(actor_id=ident("actor-" + str(index + 1)), current_key=agent.pubkey(),
                    valid_from=NOW - 100, valid_until=NOW + 3_600)), [company["owner"]])

    def create_custody(self, role, amount=0, *, wallet=False, name=None):
        party = self.parties[role]
        identity = name or role
        if wallet:
            wallet_id = ident("funding-wallet-" + identity)
            config = pda(SWIG, b"swig", wallet_id)
            authority = pda(SWIG, b"swig-wallet-address", config)
            binding = pda(REGISTRY, b"funding-wallet", party["principal"], config)
            self.observe(config, authority, binding)
            bump = Pubkey.find_program_address([b"swig", wallet_id], SWIG)[1]
            wallet_bump = Pubkey.find_program_address([b"swig-wallet-address", bytes(config)], SWIG)[1]
            auth = bytes(REGISTRY) + bytes([40]) + bytes(7) + bytes(self.registry.instructions["authorize_funding_wallet"]["discriminator"]) + bytes(binding)
            assert len(auth) == 80
            create = Instruction(SWIG, struct.pack("<HHHBB", 0, 7, len(auth), bump, wallet_bump)
                + wallet_id + auth + struct.pack("<HHI", 7, 0, 8),
                [M(config, w=True), M(self.h.payer.pubkey(), True, True), M(authority, w=True), M(SYSTEM)])
            bind = self.ix("principal_registry", "bind_funding_wallet",
                dict(owner=party["owner"].pubkey(), principal=party["principal"], config=config,
                    wallet=authority, binding=binding, swig_program=SWIG),
                args=dict(wallet_id=wallet_id, authority_generation=party["generation"]))
            self.send("create and bind exact governed financier wallet", [create, bind], [party["owner"]])
            self.h.check("complete single ProgramExec graph bytes", len(self.h.svm.get_account(config).data), 152)
            party.update(wallet_id=wallet_id, config=config, wallet=authority, binding=binding, profile=1)
        else:
            authority = pda(CORE, b"escrow-authority", party["principal"], self.mint)
            party.update(config=CORE, wallet=authority, binding=CORE, profile=2)
        custody_token = self.h.ata(authority, self.mint)
        custody = pda(CORE, b"custody-reserve", custody_token)
        party.update(custody=custody, custody_token=custody_token)
        self.observe(custody, custody_token)
        self.send("create " + role + " custody token account", self.h.ata_ix(authority, self.mint))
        if amount:
            self.transfer_tokens("prefund " + role + " token account before explicit allocation", custody_token, amount)
        request_id = ident("allocate-initial-custody-" + identity)
        receipt = self.receipt(party["principal"], self.operation(b"custody", party["principal"], request_id))
        instruction = self.ix("claims_core", "create_financier_custody",
            dict(owner=party["owner"].pubkey(), financier=party["principal"], wallet_binding=party["binding"],
                wallet_config=party["config"], wallet_authority=authority, custody_token=custody_token,
                custody=custody, receipt=receipt), remaining=self.remaining([role]),
            request_id=request_id, profile=party["profile"], allocated_cash=amount,
            authority_generation=party["generation"], provenance_hash=ident("owner-allocated-fixture-capital-" + identity))
        self.send("owner allocates " + role + " capital without inventing token inflow", instruction, [party["owner"]], receipt=receipt)
        data = check_receipt(self.core, self.h.svm, receipt)
        self.h.check(role + " initial allocation has no invented transfer", data["capital_token_inflow"], 0)
        self.h.check(role + " initial allocation preserves actual approving owner", data["capital_authority"], party["owner"].pubkey())
        self.h.check(role + " initial allocation preserves current owner generation", data["capital_owner_generation"], party["generation"])
        self.h.check(role + " allocated custody cash", self.state("FinancierCustody", custody)["accounted_cash"], amount)

    def interest_terms(self, *, rate=1000, cap=10_000):
        protocol = self.parties["protocol"]
        return dict(version=1, annual_rate_bps=rate, seconds_per_year=YEAR,
            maximum_interest=cap, accrual_cutoff=NOW + 2 * YEAR, protocol_share_bps=3700,
            protocol_beneficiary=protocol["principal"], protocol_destination=protocol["custody_token"],
            repayment_priority="InterestFirst", stop_on_shortfall=False)

    def open_facility(self, financier="financier", name="external", *, external=True):
        f, issuer = self.parties[financier], self.parties["issuer"]
        facility_id = ident("facility-" + name)
        facility = pda(CORE, b"funding-facility", f["principal"], facility_id)
        self.observe(facility)
        self.send("approve " + name + " financing arrangement", self.ix("claims_core", "open_funding_facility",
            dict(owner=f["owner"].pubkey(), financier=f["principal"], issuer_owner=issuer["owner"].pubkey(),
                issuer=issuer["principal"], custody=f["custody"], facility=facility,
                protocol_custody=self.parties["protocol"]["custody"]),
            facility_id=facility_id, commitment_limit=500_000, accept_until=NOW + 86_400,
            external=external, terms_hash=ident("facility-contract-" + name),
            interest_terms=self.interest_terms(rate=300 if external else 0, cap=10_000 if external else 0),
            financier_generation=f["generation"], issuer_generation=issuer["generation"]),
            [f["owner"], issuer["owner"]])
        return facility

    def open_line(self, facility, name="main", *, interest_terms=None):
        company, issuer = self.parties["company"], self.parties["issuer"]
        interest_terms = self.interest_terms() if interest_terms is None else interest_terms
        protocol_custody = next((party["custody"] for party in self.parties.values()
            if party.get("custody_token") == interest_terms["protocol_destination"]), None) \
            if interest_terms["protocol_share_bps"] else None
        line_id = ident("credit-line-" + name)
        line = pda(CORE, b"credit-line", self.company, line_id)
        self.observe(line)
        assignment_policy = hashlib.sha256(b"DEXTER:WITHIN-DOMAIN-WHOLE-RESIDUAL:1" + bytes(CORE) + self.domain).digest()
        self.send("owner and issuer approve " + name + " customer credit line", self.ix("claims_core", "open_customer_credit_line",
            dict(owner=company["owner"].pubkey(), company=self.company, issuer_owner=issuer["owner"].pubkey(),
                issuer=issuer["principal"], issuer_custody=issuer["custody"], facility=facility,
                credit_line=line, protocol_custody=protocol_custody),
            line_id=line_id, limit=500_000, interest_terms=interest_terms,
            assignment_policy_hash=assignment_policy,
            owner_generation=company["generation"], issuer_generation=issuer["generation"]),
            [company["owner"], issuer["owner"]])
        return line, assignment_policy

    def manage_facility(self, name, *, limit, enabled, facility=None, role="financier"):
        party = self.parties[role]
        facility = facility or self.facility
        raw = ident("facility-policy-" + name)
        operation = self.operation(b"facility-policy", party["principal"], raw)
        receipt = pda(CORE, b"facility-policy", facility, operation)
        self.observe(receipt)
        before = self.state("FundingFacility", facility)
        instruction = self.ix("claims_core", "manage_funding_facility",
            dict(owner=party["owner"].pubkey(), financier=party["principal"], facility=facility, receipt=receipt),
            request_id=raw, expected_revision=before["revision"], commitment_limit=limit,
            enabled=enabled, authority_generation=party["generation"])
        self.send("owner changes only new facility admission " + name, instruction, [party["owner"]])
        self.send("facility policy retry preserves original outcome " + name, instruction, [party["owner"]])
        after = self.state("FundingFacility", facility)
        for field in ("reserved", "drawn", "repaid", "terms_hash", "interest_terms", "interest", "opening_hash"):
            self.h.check("facility policy preserves " + field, after[field], before[field])
        historical = self.state("FacilityPolicyReceipt", receipt)
        self.h.check("facility policy receipt names original limit", historical["previous_limit"], before["commitment_limit"])
        self.h.check("facility policy receipt names resulting limit", historical["resulting_limit"], limit)
        return receipt

    def grant(self, facility, index=0, *, name=None):
        company = self.parties["company"]
        mandate_id = ident("mandate-" + (name if name is not None else str(index + 1)))
        mandate = pda(REGISTRY, b"mandate", self.company, mandate_id)
        self.observe(mandate)
        self.send("owner grants finite mandate " + str(index + 1), self.ix("principal_registry", "create_mandate",
            dict(owner=company["owner"].pubkey(), principal=self.company, actor=self.actors[index],
                budget=self.budget, mandate=mandate),
            args=dict(mandate_id=mandate_id,
                supplier=self.parties["supplier"]["owner"].pubkey(), source=facility,
                per_operation_limit=200_000, expires_at=NOW + 600)), [company["owner"]])
        return mandate

    def setup(self, *, financier_wallet=True):
        self.setup_principals()
        for role, amount in (("protocol", 0), ("fee", 0), ("issuer", 200_000), ("assignee", 0), ("sponsor", 0)):
            self.create_custody(role, amount)
        self.create_custody("financier", 1_000_000, wallet=financier_wallet)
        self.open_fee_agreement("main", 2_100)
        self.facility = self.open_facility()
        self.line, self.assignment_policy = self.open_line(self.facility)
        self.grant(self.facility)

    def open_fee_agreement(self, name, cap, *, reject=False, owner=None, generation=None,
            supplied_principal=None, supplied_mint=None, supplied_hash=None):
        supplier = self.parties["supplier"]
        agreement_hash = ident("accepted-seller-fee-agreement-" + name)
        agreement = pda(CORE, b"seller-fee-agreement", supplier["principal"], self.mint, agreement_hash)
        owner = owner or supplier["owner"]
        self.observe(agreement)
        instruction = self.ix("claims_core", "open_seller_fee_agreement",
            dict(owner=owner.pubkey(), supplier_principal=supplied_principal or supplier["principal"],
                mint=supplied_mint or self.mint, agreement=agreement),
            agreement_hash=supplied_hash or agreement_hash, aggregate_cap=cap,
            authority_generation=supplier["generation"] if generation is None else generation)
        self.send("supplier approves immutable aggregate fee agreement " + name, instruction, [owner], reject=reject)
        if not reject:
            self.fee_agreements[name] = dict(address=agreement, agreement_hash=agreement_hash, cap=cap)
        return agreement

    def fee_allocated(self, name="main"):
        return self.state("SellerFeeAgreement", self.fee_agreements[name]["address"])["usage"]["allocated"]

    def exercise_fee_parent(self, first_tab):
        self.h.check("first accepted Tab allocates its maximum realizable fee once", self.fee_allocated(), 1_377)
        second = self.acceptance("second-fee-child", cap=50_000, maximum_interest=0)
        self.accept(second, label="second finite Tab shares the supplier-approved parent fee cap")
        self.h.check("two child authorizations consume one aggregate", self.fee_allocated(), 2_069)
        third = self.acceptance("over-parent-fee-cap", cap=50_000, maximum_interest=0)
        self.accept(third, reject=True, label="third otherwise eligible Tab cannot exceed the standing seller fee aggregate")
        self.open_fee_agreement("main", 2_100)
        self.h.check("agreement reopen preserves consumed authorization", self.fee_allocated(), 2_069)
        self.open_fee_agreement("main", 2_101, reject=True)
        self.open_fee_agreement("main", 2_100, supplied_hash=ident("other-accepted-agreement"), reject=True)
        self.open_fee_agreement("main", 2_100, supplied_principal=self.company, reject=True)
        self.open_fee_agreement("main", 2_100, generation=2, reject=True)
        alternative_mint, alternative_source = self.h.make_mint(key(34), initial=1)
        self.observe(alternative_mint, alternative_source)
        self.open_fee_agreement("main", 2_100, supplied_mint=alternative_mint, reject=True)
        self.release(second, "release-second-fee-child", 50_000)
        self.h.check("unused child release cannot refill the standing fee authorization", self.fee_allocated(), 2_069)
        for tab, slice_ in ((first_tab, 1_377), (second, 692)):
            claim = self.state("FundedClaim", tab["claim"])
            self.h.check("claim records exact parent fee agreement", claim["seller_fee_agreement"], self.fee_agreements["main"]["address"])
            self.h.check("claim records allocated seller fee slice", claim["seller_fee_allocation"], slice_)

    def fee_parent_owner_change(self):
        supplier = self.parties["supplier"]
        previous = supplier["owner"]
        replacement = key(35)
        self.h.svm.airdrop(replacement.pubkey(), 100_000_000_000)
        raw = ident("supplier-owner-rotation")
        receipt = pda(REGISTRY, b"owner-rotation", supplier["principal"], raw)
        self.observe(receipt)
        self.send("supplier rotates its continuing-principal owner",
            self.ix("principal_registry", "rotate_principal_owner",
                dict(principal=supplier["principal"], current_owner=previous.pubkey(), new_owner=replacement.pubkey(), receipt=receipt),
                args=dict(operation_id=raw, expected_authority_generation=supplier["generation"], new_owner=replacement.pubkey())),
            [previous, replacement])
        supplier["owner"], supplier["generation"] = replacement, supplier["generation"] + 1
        self.open_fee_agreement("main", 2_100, owner=previous, generation=1, reject=True)
        self.open_fee_agreement("main", 2_100)
        self.h.check("current owner can reconcile original agreement without resetting usage", self.fee_allocated(), 2_069)
        original = self.state("SellerFeeAgreement", self.fee_agreements["main"]["address"])
        self.h.check("agreement preserves original supplier approval identity", original["approved_by"], previous.pubkey())
        self.h.check("agreement preserves original approval generation", original["authority_generation"], 1)

    def seller_fees(self, cap, *, enabled=True, aggregate_cap=2_000, agreement="main"):
        fee = self.parties["fee"]
        return dict(version=1, agreement_hash=self.fee_agreements[agreement]["agreement_hash"],
            supplier=self.parties["supplier"]["owner"].pubkey(), asset=self.asset, gross_cap=cap,
            rate_bps=137 if enabled else 0, fixed_fee=7 if enabled else 0,
            aggregate_fee_cap=aggregate_cap if enabled else 0, maximum_deduction_bps=10_000,
            beneficiary=fee["principal"], beneficiary_wallet=fee["wallet"], destination=fee["custody_token"],
            failure_policy="WaiveUnavailable")

    def acceptance(self, name, *, cap=100_000, maximum_interest=10_000, index=0,
            financier="financier", facility=None, line=None, policy=None, fees=True, fee_agreement="main",
            mandate=None):
        financier_party, supplier, issuer = [self.parties[x] for x in (financier, "supplier", "issuer")]
        facility, line, policy = facility or self.facility, line or self.line, policy or self.assignment_policy
        mandate = self.mandates[index] if mandate is None else mandate
        agent = (self.agent, self.agent2)[index]
        tab_id, raw = ident("tab-" + name), ident("accept-" + name)
        claim = pda(CORE, b"claim", self.company, tab_id)
        reservation = pda(REGISTRY, b"reservation", self.company, tab_id)
        operation_id = self.operation(b"accept", agent.pubkey(), raw)
        registry_operation = pda(REGISTRY, b"operation", self.company, operation_id)
        receipt = self.receipt(self.company, operation_id)
        namespace, unit, contract, unit_price = ident("provider-namespace"), ident("completed-inference-unit"), ident("provider-contract"), 1
        terms = dict(domain=self.domain, tab_id=tab_id, principal=self.company, actor=self.actors[index],
            mandate=mandate, asset=self.mint, supplier=supplier["owner"].pubkey(),
            source=facility, cap=cap, expires_at=NOW + 600, credential_generation=1, mandate_revision=1,
            external_terms_hash=hashlib.sha256(b"DEXTER:COMPLETED-RUN-POLICY:1" + namespace + unit
                + u64(unit_price) + contract).digest())
        data = dict(name=name, claim=claim, reservation=reservation, facility=facility, line=line,
            financier=financier, supplier_principal=supplier["principal"],
            namespace=namespace, service_unit=unit, unit_price=unit_price,
            external_contract_hash=contract, terms=terms)
        args = dict(request_id=raw, terms=terms, maximum_interest=maximum_interest,
            seller_fee_terms=self.seller_fees(cap, enabled=fees, agreement=fee_agreement), assignment_policy_hash=policy,
            namespace=namespace, service_unit=unit, unit_price=unit_price, external_contract_hash=contract)
        accounts = dict(principal=self.company, actor=self.actors[index], credential=agent.pubkey(),
            mandate=mandate, budget=self.budget, reservation=reservation,
            registry_operation=registry_operation, issuer_owner=issuer["owner"].pubkey(), issuer=issuer["principal"],
            supplier=supplier["owner"].pubkey(), supplier_principal=supplier["principal"],
            fee_custody=self.parties["fee"]["custody"] if fees else None, credit_line=line,
            seller_fee_agreement=self.fee_agreements[fee_agreement]["address"] if fees else None,
            facility=facility, custody=financier_party["custody"], custody_token=financier_party["custody_token"],
            wallet_binding=financier_party["binding"], wallet_config=financier_party["config"],
            wallet_authority=financier_party["wallet"], claim=claim, receipt=receipt)
        self.observe(claim, reservation, registry_operation, receipt)
        data.update(args=args, accounts=accounts, receipt=receipt, signers=[agent, issuer["owner"], supplier["owner"]])
        self.tabs[name] = data
        return data

    def accept(self, tab, *, reject=False, label=None):
        instruction = self.ix("claims_core", "accept_financed_tab", tab["accounts"],
            remaining=self.remaining(["company", "issuer", "supplier", tab["financier"]]), **tab["args"])
        self.send(label or "accept finite supplier commitment " + tab["name"], instruction,
            tab["signers"], reject=reject, receipt=tab["receipt"])
        return instruction

    def earn(self, tab, name, units, *, payment_name=None, outage=False, reject=False,
            expected_revision=None, label=None):
        supplier, financier = self.parties["supplier"], self.parties[tab["financier"]]
        run_id = ident("completed-run-" + name)
        payment_id = ident("payment-attempt-" + (payment_name or name))
        run = pda(CORE, b"usage", tab["supplier_principal"], tab["namespace"], run_id)
        operation_id = hashlib.sha256(b"DEXTER:COMPLETED-RUN-OPERATION:1" + bytes(tab["supplier_principal"])
            + tab["namespace"] + run_id).digest()
        registry_operation = pda(REGISTRY, b"run-operation", self.company, operation_id)
        receipt = self.receipt(self.company, operation_id)
        payment = pda(CORE, b"run-payment", self.company, payment_id)
        self.observe(run, registry_operation, receipt, payment)
        args = dict(payment_id=payment_id, namespace=tab["namespace"], run_id=run_id,
            service_unit=tab["service_unit"], external_contract_hash=tab["external_contract_hash"],
            units=units, unit_price=tab["unit_price"],
            expected_revision=self.state("FundedClaim", tab["claim"])["revision"] if expected_revision is None else expected_revision,
            service_without_pool=outage)
        accounts = dict(supplier=supplier["owner"].pubkey(), supplier_principal=supplier["principal"],
            principal=self.company, budget=self.budget, reservation=tab["reservation"],
            registry_operation=registry_operation, line=tab["line"], claim=tab["claim"],
            custody=financier["custody"], custody_token=financier["custody_token"], run=run, payment=payment, receipt=receipt)
        instruction = self.ix("claims_core", "earn_financed_run", accounts,
            remaining=self.remaining(["company", "issuer", "supplier", tab["financier"]], outage), **args)
        self.send(label or "supplier attests completed run " + name, instruction, [supplier["owner"]], reject=reject, receipt=receipt)
        if not reject:
            recorded = self.state("FundedRun", run)
            self.h.check("run retains continuing supplier identity", recorded["supplier_principal"], tab["supplier_principal"])
            self.h.check("run retains its accepted supplier payment wallet", recorded["supplier"], tab["terms"]["supplier"])
            self.h.check("run stores the Principal-scoped financial identity", recorded["operation_id"], operation_id)
        return dict(run=run, receipt=receipt, payment=payment, operation_id=operation_id,
            instruction=instruction, args=args, accounts=accounts)

    def withdrawal(self, name, amount, *, role="financier", outage=False, owner=None, generation=None,
            expected_revision=None, destination=None):
        party = self.parties[role]
        owner = owner or party["owner"]
        generation = party["generation"] if generation is None else generation
        raw = ident("withdrawal-" + name)
        receipt = self.receipt(party["principal"], self.operation(b"withdraw", party["principal"], raw))
        destination = destination or self.h.ata(owner.pubkey(), self.mint)
        self.observe(receipt, destination)
        instruction = self.ix("claims_core", "withdraw_financier_free", dict(payer=owner.pubkey(),
            financier_principal=party["principal"], wallet_binding=party["binding"], custody=party["custody"],
            custody_token=party["custody_token"], wallet_authority=party["wallet"], destination=destination, receipt=receipt),
            remaining=self.remaining([role], outage), request_id=raw, amount=amount,
            expected_revision=self.state("FinancierCustody", party["custody"])["revision"] if expected_revision is None else expected_revision,
            authority_generation=generation, service_without_pool=outage)
        return dict(instruction=instruction, receipt=receipt, signers=[owner], owner=owner, generation=generation, role=role)

    def withdraw(self, request, *, reject=False, label=None, **wrapper):
        instructions = request["instruction"]
        if self.parties[request["role"]]["profile"] == 1:
            instructions = self.wallet_bundle(instructions, action=2, caller=request["owner"],
                generation=request["generation"], role=request["role"], **wrapper)
        self.send(label or "owner withdraws eligible uncommitted cash", instructions,
            request["signers"], reject=reject, receipt=request["receipt"])

    def release(self, tab, name, amount, *, outage=False, supplier=True, reject=False):
        party, financier = self.parties["supplier"], self.parties[tab["financier"]]
        raw = ident("release-" + name)
        operation_id = self.operation(b"release", self.h.payer.pubkey(), raw)
        receipt = self.receipt(self.company, operation_id)
        registry_operation = pda(REGISTRY, b"operation", self.company, operation_id)
        self.observe(receipt, registry_operation)
        instruction = self.ix("claims_core", "release_financier_unused",
            dict(principal=self.company, supplier_principal=party["principal"], supplier=party["owner"].pubkey() if supplier else None,
                budget=self.budget, reservation=tab["reservation"], registry_operation=registry_operation,
                line=tab["line"], claim=tab["claim"], facility=tab["facility"], custody=financier["custody"],
                custody_token=financier["custody_token"], receipt=receipt),
            remaining=self.remaining(["company", "issuer", "supplier", tab["financier"]], outage),
            request_id=raw, amount=amount, expected_revision=self.state("FundedClaim", tab["claim"])["revision"],
            service_without_pool=outage)
        self.send("release unused accepted work " + name, instruction, [party["owner"]] if supplier else (),
            reject=reject, receipt=receipt)
        return receipt

    def collection(self, tab, run, amount, *, paid_before=None, outage=False, sponsor=None,
            fee_available=True, destination=None, revision=None):
        financier, supplier, fee = [self.parties[x] for x in (tab["financier"], "supplier", "fee")]
        claim = self.state("FundedClaim", tab["claim"])
        actual_run = self.state("FundedRun", run["run"])
        paid_before = actual_run["gross_paid"] if paid_before is None else paid_before
        destination = destination or self.h.ata(tab["terms"]["supplier"], self.mint)
        encoded = struct.pack("<H", 1) + self.domain + bytes(tab["claim"]) + bytes(run["run"])
        encoded += claim["acceptance_hash"] + bytes(financier["custody"]) + bytes(financier["custody_token"])
        encoded += bytes(self.mint) + bytes(TOKEN) + bytes(tab["terms"]["supplier"]) + bytes(destination)
        encoded += u64(paid_before) + u64(amount)
        operation_id = hashlib.sha256(b"DEXTER:EARNED-COLLECTION:1" + bytes(CORE) + encoded).digest()
        receipt = self.receipt(self.company, operation_id)
        self.observe(receipt, destination)
        before = claim["seller_fee_state"]["gross_settled"]
        terms = claim["seller_fee_terms"]
        def cumulative(gross):
            return min(gross * terms["rate_bps"] // 10_000 + (terms["fixed_fee"] if gross else 0),
                terms["aggregate_fee_cap"], gross * terms["maximum_deduction_bps"] // 10_000)
        # Account membership is fixed by accepted possible fees, not today's
        # rounding, cap consumption, or destination availability.
        fees_enabled = cumulative(terms["gross_cap"]) > 0
        expected_fee = (cumulative(before + amount) - cumulative(before)) if fees_enabled and fee_available else 0
        roles = ["company", "issuer", "supplier", tab["financier"]]
        if fees_enabled:
            roles.append("fee")
        accounts = dict(financier_principal=financier["principal"], wallet_binding=financier["binding"],
            custody=financier["custody"], custody_token=financier["custody_token"], wallet_authority=financier["wallet"],
            claim=tab["claim"], run=run["run"], facility=tab["facility"], destination=destination,
            fee_destination=fee["custody_token"] if fees_enabled else None,
            fee_custody=fee["custody"] if fees_enabled else None, receipt=receipt,
            reimbursement_principal=None, reimbursement_authority=None, reimbursement_from=None,
            sponsor_destination=None, sponsor_custody=None)
        signers = []
        quote = None
        if sponsor:
            payer = self.parties[sponsor["payer"]]
            sponsor_party = self.parties["sponsor"]
            quote = dict(version=1, operation_id=operation_id, funding_source=sponsor["funding_source"],
                payer_principal=payer["principal"], payer_authority=payer["owner"].pubkey(),
                authority_generation=payer["generation"], asset=self.asset, from_=payer["owner_ata"])
            quote["from"] = quote.pop("from_")
            quote.update(destination=sponsor_party["custody_token"], sponsor_principal=sponsor_party["principal"],
                rent_owner=self.h.payer.pubkey(), approved_at=self.now, expires_at=self.now + 100,
                native_cost_lamports=5_000, conversion_numerator=1, conversion_denominator=10,
                reimbursement=500, margin=50, maximum_total_charge=550, maximum_margin=50,
                price_quote_hash=ident("approved-synthetic-native-to-token-conversion"))
            quote.update(sponsor.get("overrides", {}))
            accounts.update(reimbursement_principal=payer["principal"], reimbursement_authority=payer["owner"].pubkey(),
                reimbursement_from=quote["from"], sponsor_destination=sponsor_party["custody_token"],
                sponsor_custody=sponsor_party["custody"])
            signers.append(payer["owner"])
            roles.extend([sponsor["payer"], "sponsor"])
        args = dict(operation_id=operation_id, paid_before=paid_before, amount=amount,
            expected_revision=claim["revision"] if revision is None else revision,
            service_without_pool=outage, sponsor_quote=quote)
        instruction = self.ix("claims_core", "collect_financier", accounts,
            remaining=self.remaining(roles, outage), **args)
        return dict(instruction=instruction, args=args, accounts=accounts, receipt=receipt,
            roles=roles, signers=signers, financier=tab["financier"], expected_fee=expected_fee)

    def collect(self, request, *, reject=False, label=None, **wrapper):
        instructions = request["instruction"]
        if self.parties[request["financier"]]["profile"] == 1:
            instructions = self.wallet_bundle(instructions, action=1, caller=self.h.payer,
                generation=0, role=request["financier"], **wrapper)
        self.send(label or "independent supplier collection of earned right", instructions,
            request["signers"], reject=reject, receipt=request["receipt"])
        return instructions

    def wallet_bundle(self, inner, *, action, caller, generation, role="financier", nonce=None, expires_at=None, bad_hash=False):
        party = self.parties[role]
        binding = self.registry.read(self.h.svm, party["binding"], "FundingWalletBinding")
        args = dict(binding=party["binding"], nonce=binding["nonce"] if nonce is None else nonce,
            expires_at=self.now + 300 if expires_at is None else expires_at,
            authority_generation=generation, action=action, bundle_hash=bytes(32))
        tail = compact_sign(party["config"], party["wallet"], inner)
        accounts = dict(config=party["config"], wallet=party["wallet"], binding=party["binding"],
            principal=party["principal"], caller=caller.pubkey(), instructions=SYSVAR_INSTRUCTIONS)
        authorize = self.ix("principal_registry", "authorize_funding_wallet", accounts, args=args)
        raw = canonical_instruction(tail, [authorize, tail], self.h.payer.pubkey())
        args["bundle_hash"] = bytes([99]) * 32 if bad_hash else hashlib.sha256(
            b"DEXTER:FUNDING-SWIG-BUNDLE:1" + bytes(REGISTRY) + bytes(CORE) + bytes(SWIG)
            + self.domain + bytes(party["principal"]) + bytes(party["binding"]) + u64(args["nonce"])
            + struct.pack("<q", args["expires_at"]) + u64(generation) + bytes([action]) + raw).digest()
        return [self.ix("principal_registry", "authorize_funding_wallet", accounts, args=args), tail]

    def rotate_financier(self):
        party = self.parties["financier"]
        old_owner = party["owner"]
        operation_id = ident("rotate-financier-owner")
        receipt = pda(REGISTRY, b"owner-rotation", party["principal"], operation_id)
        self.observe(receipt)
        instruction = self.ix("principal_registry", "rotate_principal_owner",
            dict(principal=party["principal"], current_owner=old_owner.pubkey(),
                new_owner=self.replacement_owner.pubkey(), receipt=receipt),
            args=dict(operation_id=operation_id, expected_authority_generation=party["generation"],
                new_owner=self.replacement_owner.pubkey()))
        self.send("both owners authorize financier control rotation", instruction, [old_owner, self.replacement_owner])
        self.send("owner rotation retry preserves original receipt and generation", instruction, [old_owner, self.replacement_owner])
        party["owner"] = self.replacement_owner
        party["generation"] += 1
        self.h.check("owner generation increments once",
            self.registry.read(self.h.svm, party["principal"], "Principal")["authority_generation"], party["generation"])
        return old_owner

    def advance(self, seconds):
        self.now += seconds
        self.h.svm.set_clock(Clock(100 + seconds, NOW, 0, 0, self.now))

    def catch_up(self):
        for entry in self.receipts:
            receipt = Pubkey.from_string(entry["address"])
            for effect in entry["data"]["effects"]:
                projection = pda(POOL, b"financial-projection", effect["book"])
                state = self.pool.read(self.h.svm, projection, "FinancialProjection")
                if state["applied_sequence"] < effect["sequence"]:
                    self.send("public ordered projection catch-up", self.ix("pool_issuer", "project_financing",
                        dict(book=effect["book"], projection=projection, receipt=receipt)))
        for party in self.parties.values():
            book = self.state("FinanceBook", party["book"])["data"]
            projection = self.pool.read(self.h.svm, party["projection"], "FinancialProjection")
            self.h.check("projection reconciled " + party["name"], projection["positions"], book["positions"])
            self.h.check("projection frontier reconciled " + party["name"], projection["applied_sequence"], book["sequence"])

    def finish(self, name, *, error=None):
        source_inputs = {path: digest(self.artifact_root / path) for path in self.build["inputs"]}
        assert source_inputs == self.build["inputs"], "Program source changed during native execution"
        assert self.fixture_inputs == {path: digest(ROOT/path) for path in self.fixture_inputs}, \
            "Native fixture or independent oracle changed during execution"
        trace = dict(schema="dexter-financing-reconstruction-native/v1", status="failed" if error else "passed",
            reconstructed=True, scope="New native-v1 compiled Registry/Core/Pool/Swig execution; no live provider",
            transactions=self.h.cases, assertions=self.h.assertions, receipts=self.receipts,
            error=repr(error) if error else None, build_inputs=self.build["inputs"],
            fixture_inputs=self.fixture_inputs,
            program_artifacts=self.build["artifacts"], swig_sha256=SWIG_SHA256,
            artifact_stages=getattr(self, "artifact_stages", []),
            transport=self.h.transport, public_transactions_sent=0,
            boundary="Local LiteSVM execution does not qualify these messages for customer RPC admission, priority fees or validator finality; older Agave/Kit probes retain their own separate scope")
        path = OUT / (name + "-trace.json")
        path.write_text(json.dumps(trace, indent=2, default=json_value) + "\n")
        summary = dict(status=trace["status"], trace=str(path), trace_sha256=digest(path),
            transaction_count=len(self.h.cases), refused=sum(x["expected"] == "reject" for x in self.h.cases),
            assertions=len(self.h.assertions), receipts=len(self.receipts),
            maximum_signed_packet=max((x["wire_bytes"] for x in self.h.cases), default=0),
            build_receipt_sha256=digest(OUT / "build-receipt.json"), idl_receipt_sha256=digest(OUT / "idl-receipt.json"))
        (OUT / (name + "-receipt.json")).write_text(json.dumps(summary, indent=2) + "\n")
        print(json.dumps(summary))
        return summary
