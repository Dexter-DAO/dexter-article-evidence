"""Executable monetary reference model for a separately owned PoolIssuer.

This is an integer state machine, not a Solana implementation or a legal oracle.
Core owns accepted rights and its ordered effect log. Pool owns its mirror,
capital, cost basis and investor entitlements. Only actual token movements change
cash. An escaped servicing effect changes Core and tokens; investor actions are
blocked until the Pool consumes the complete canonical log.
"""
from __future__ import annotations

from copy import deepcopy
from dataclasses import dataclass, field, asdict
from functools import wraps
from hashlib import sha256
import json
from typing import Any

U64_MAX = (1 << 64) - 1
UNIT_SCALE = 1_000_000
CORE_MODES = {"CASH": 0, "SUPPLIER_CREDIT": 1, "ISSUER_CREDIT": 2, "MIXED": 3}

# THE sole canonical equation definition. Documentation and Rust are generated
# from this tuple, never maintained as separate equation strings.
NAV_TERMS = (
    ("operating_cash", 1),
    ("beneficial_guarantee_cash", 1),
    ("gross_debt", 1),
    ("acquisition_discount", -1),
    ("principal_allowance", -1),
    ("recourse_receivable", 1),
    ("recourse_allowance", -1),
    ("earned_fee_receivable", 1),
    ("supplier_payable", -1),
    ("refund_liability", -1),
    ("accrued_expenses", -1),
    ("facility_principal", -1),
    ("commitment_provision", -1),
    ("income_proceeds_liability", -1),
    ("unallocated_recovery_liability", -1),
)


def nav_expression() -> str:
    return " ".join(("" if i == 0 else "+ " if sign > 0 else "- ") + name
                    for i, (name, sign) in enumerate(NAV_TERMS))


def net_assets(balances: dict[str, int]) -> int:
    for name, _ in NAV_TERMS:
        checked_amount(balances[name])
    result = sum(sign * balances[name] for name, sign in NAV_TERMS)
    require(-(1 << 127) <= result < (1 << 127), "SIGNED_OVERFLOW")
    return result


class Rejected(Exception):
    pass


def require(condition: bool, reason: str) -> None:
    if not condition:
        raise Rejected(reason)


def checked_amount(value: int) -> int:
    require(isinstance(value, int) and 0 <= value <= U64_MAX, "AMOUNT_RANGE")
    return value


def positive(value: int) -> int:
    checked_amount(value)
    require(value > 0, "ZERO_AMOUNT")
    return value


def exact_allocation(total: int, weights: dict[str, int]) -> dict[str, int]:
    """Fixed-total cumulative intervals: exact sum, deterministic residual atoms."""
    denominator = sum(weights.values())
    require(total == 0 or denominator > 0, "EMPTY_OWNERSHIP")
    if denominator == 0:
        return {}
    out, prefix = {}, 0
    for owner, weight in sorted(weights.items()):
        out[owner] = ((prefix + weight) * total // denominator
                      - prefix * total // denominator)
        prefix += weight
    assert sum(out.values()) == total
    return out


def atomic(method):
    @wraps(method)
    def wrapped(self: "World", *args, **kwargs):
        before = deepcopy(self.__dict__)
        try:
            result = method(self, *args, **kwargs)
            self.assert_invariants()
            return result
        except Exception:
            self.__dict__.clear()
            self.__dict__.update(before)
            raise
    return wrapped


@dataclass(frozen=True)
class Policy:
    """Explicit synthetic inputs in fixtures; not market/calibrated defaults."""
    max_exposure: int
    loss_capital_ppm: int
    junior_exposure_ppm: int
    minimum_junior: int
    operating_buffer: int


@dataclass
class Holding:
    face: int
    basis: int
    allowance: int = 0


@dataclass
class CoreClaim:
    debtor: str
    creditor: str
    original: int
    remaining: int
    repaid: int = 0
    offset: int = 0
    credited: int = 0
    origin_mode: int = 2  #0CASH,1SUPPLIER_CREDIT,2ISSUER_CREDIT; mixed is per-slice
    original_supplier: str = ""


@dataclass
class Guarantee:
    pool: str
    debtor: str
    supplier: str
    accepted: int
    remaining: int
    backing: str  # cash escrow, or exact committed-wallet facility id
    captured: int = 0
    released: int = 0


@dataclass
class Payable:
    pool: str
    supplier: str
    original: int
    remaining: int
    backing: str
    paid: int = 0
    offset: int = 0
    corrected: int = 0


@dataclass
class Facility:
    provider: str
    pool: str
    maximum: int
    new_enabled: bool = True
    reserved: int = 0
    drawn: int = 0
    repaid: int = 0


@dataclass
class LossCohort:
    source: str
    rank: str
    owner_losses: dict[str, int]
    funded: int = 0
    paid: dict[str, int] = field(default_factory=dict)
    resolved_unpaid: int = 0

    @property
    def cap(self):
        return sum(self.owner_losses.values())


@dataclass
class Effect:
    seq: int
    kind: str
    data: dict[str, Any]
    previous_root: str
    root: str


@dataclass
class Book:
    operating_cash: int = 0
    beneficial_guarantee_cash: int = 0
    holdings: dict[str, Holding] = field(default_factory=dict)
    supplier_payable: int = 0
    reservation_remaining: int = 0
    cash_backed_reservation: int = 0
    cash_backed_payable: int = 0
    external_backed_reservation: int = 0
    external_backed_payable: int = 0
    recourse_receivable: int = 0
    recourse_allowance: int = 0
    earned_fee_receivable: int = 0
    refund_liability: int = 0
    accrued_expenses: int = 0
    facility_principal: int = 0
    commitment_provision: int = 0
    income_proceeds_liability: int = 0
    unallocated_recovery_liability: int = 0
    senior_nav: int = 0
    junior_nav: int = 0
    insolvency_deficit: int = 0
    senior_units: dict[str, int] = field(default_factory=dict)
    junior_units: dict[str, int] = field(default_factory=dict)
    cohorts: list[LossCohort] = field(default_factory=list)
    # Actual recovery benefits temporarily used for prior-creditor restoration.
    # These are contingent original-holder restoration memoranda, NOT liabilities
    # or NAV assets. They are checked before later residual gains become capital.
    deferred_recovery: dict[str, int] = field(default_factory=dict)
    source_loss_unrecovered: dict[str, int] = field(default_factory=dict)
    source_deficit_loss: dict[str, int] = field(default_factory=dict)
    recovery_reserved: dict[str, int] = field(default_factory=dict)
    realized_acquisition_returns: int = 0  # reporting counter, never another asset
    provision_by_guarantee: dict[str, int] = field(default_factory=dict)
    income_funded: int = 0
    recovery_funded: int = 0
    mirror_seq: int = 0
    mirror_root: str = "0" * 64

    def balances(self) -> dict[str, int]:
        b = {name: getattr(self, name) for name, _ in NAV_TERMS
             if hasattr(self, name)}
        b["gross_debt"] = sum(h.face for h in self.holdings.values())
        b["acquisition_discount"] = sum(h.face - h.basis for h in self.holdings.values())
        b["principal_allowance"] = sum(h.allowance for h in self.holdings.values())
        return b

    def nav(self):
        return net_assets(self.balances())

    def exposure(self):
        return sum(h.face for h in self.holdings.values()) + self.reservation_remaining

    def absorb_loss(self, source: str, amount: int) -> None:
        """Loss order: junior, senior, then an explicit signed deficit."""
        left = amount
        for rank in ("junior", "senior"):
            old_nav = getattr(self, rank + "_nav")
            applied = min(old_nav, left)
            if applied:
                owners = exact_allocation(applied, getattr(self, rank + "_units"))
                self.cohorts.append(LossCohort(source, rank, owners))
                setattr(self, rank + "_nav", old_nav - applied)
                left -= applied
        self.insolvency_deficit += left
        self.source_deficit_loss[source] = self.source_deficit_loss.get(source, 0) + left
        self.source_loss_unrecovered[source] = self.source_loss_unrecovered.get(source, 0) + amount

    def eligible_historic(self, source: str) -> int:
        cap = sum(c.cap - c.funded - c.resolved_unpaid for c in self.cohorts
                  if c.source == source)
        return max(0, cap - self.recovery_reserved.get(source, 0)
                   - self.deferred_recovery.get(source, 0))

    def restore_gain(self, source: str, amount: int) -> None:
        """Account for benefit from reversing an original loss, not a new fee.

        Source deficit loss is restored before source LP tails. A current global
        deficit can nevertheless consume another source's eligible LP recovery;
        retain that exact source memorandum for prior-creditor-first restoration.
        No memorandum is spendable or a fixed debt to LPs.
        """
        require(amount <= self.source_loss_unrecovered.get(source, 0), "RECOVERY_EXCEEDS_LOSS")
        self.source_loss_unrecovered[source] -= amount
        source_deficit = min(amount, self.source_deficit_loss.get(source, 0))
        self.source_deficit_loss[source] = self.source_deficit_loss.get(source, 0) - source_deficit
        eligible = min(amount - source_deficit, self.eligible_historic(source))
        repair = min(amount, self.insolvency_deficit)
        self.insolvency_deficit -= repair
        residual = amount - repair
        # Record the whole actual old-owner benefit before allocating the part
        # that remains after creditor restoration. Whatever cannot be reserved
        # stays a source-linked contingent memorandum, including cross-source
        # restoration. It cannot disappear when another source later recovers.
        self.deferred_recovery[source] = self.deferred_recovery.get(source, 0) + eligible
        # Restore previously diverted actual source proceeds before any ordinary
        # gain becomes current junior NAV. Original source ownership is retained.
        for old_source in sorted(self.deferred_recovery):
            take = min(residual, self.deferred_recovery[old_source])
            if take:
                self.deferred_recovery[old_source] -= take
                self.recovery_reserved[old_source] = self.recovery_reserved.get(old_source, 0) + take
                self.unallocated_recovery_liability += take
                residual -= take
        if residual:
            # This branch is a real creditor restoration surplus after all actual
            # deferred old-owner rights; it is not a guessed future recovery.
            self.junior_nav += residual

    def book_principal_settlement(self, claim_id: str, amount: int) -> dict[str, int]:
        h = self.holdings[claim_id]
        require(0 < amount <= h.face, "PRINCIPAL_OVERPAYMENT")
        basis_return = min(amount, h.basis)
        carrying_return = min(basis_return, h.basis - h.allowance)
        historic_recovery = basis_return - carrying_return
        acquisition_return = amount - basis_return
        h.face -= amount
        h.basis -= basis_return
        h.allowance -= historic_recovery
        if historic_recovery:
            self.restore_gain(claim_id, historic_recovery)
        if acquisition_return:
            self.realized_acquisition_returns += acquisition_return
            self.income_proceeds_liability += acquisition_return
        return {"carrying_return": carrying_return,
                "historic_recovery": historic_recovery,
                "acquisition_return": acquisition_return}

    def apply(self, e: Effect) -> None:
        require(e.seq == self.mirror_seq + 1 and e.previous_root == self.mirror_root,
                "INCOMPLETE_EFFECT_PREFIX")
        d, k = e.data, e.kind
        q = d.get("amount", 0)
        if k == "RESERVE":
            self.reservation_remaining += q
            if d["backing"] == "cash":
                self.operating_cash -= q
                self.beneficial_guarantee_cash += q
                self.cash_backed_reservation += q
            else:
                self.external_backed_reservation += q
        elif k == "CAPTURE":
            self.reservation_remaining -= q
            self.supplier_payable += q
            h = self.holdings.setdefault(d["claim"], Holding(0, 0))
            h.face += q
            h.basis += q
            if d["backing"] == "cash":
                self.cash_backed_reservation -= q
                self.cash_backed_payable += q
            else:
                self.external_backed_reservation -= q
                self.external_backed_payable += q
            old_provision = self.provision_by_guarantee.get(d["guarantee"], 0)
            transfer = (old_provision if q == d["remaining_before"] else
                        old_provision * q // d["remaining_before"])
            if transfer:
                self.commitment_provision -= transfer
                self.provision_by_guarantee[d["guarantee"]] -= transfer
                h.allowance += transfer
                # Exact source lineage moves with the transferred historical loss.
                old = d["guarantee"]
                self.source_loss_unrecovered[old] -= transfer
                self.source_loss_unrecovered[d["claim"]] = self.source_loss_unrecovered.get(d["claim"], 0) + transfer
                left = transfer
                for c in list(self.cohorts):
                    if c.source == old and left:
                        part = min(left, c.cap)
                        require(c.funded == 0, "FUNDED_PROVISION_SPLIT")
                        split = exact_allocation(part, c.owner_losses)
                        c.owner_losses = {o: v - split.get(o, 0) for o, v in c.owner_losses.items()}
                        self.cohorts.append(LossCohort(d["claim"], c.rank, split))
                        left -= part
                deficit = min(left, self.source_deficit_loss.get(old, 0))
                self.source_deficit_loss[old] = self.source_deficit_loss.get(old, 0) - deficit
                self.source_deficit_loss[d["claim"]] = self.source_deficit_loss.get(d["claim"], 0) + deficit
                require(left == deficit, "PROVISION_LINEAGE")
        elif k == "PAYOUT":
            self.supplier_payable -= q
            if d["backing"] == "cash":
                self.cash_backed_payable -= q
                self.beneficial_guarantee_cash -= q
            else:
                self.external_backed_payable -= q
                self.facility_principal += q
        elif k == "REPAY":
            fee = d["fee"]
            self.operating_cash += q + fee
            if q:
                self.book_principal_settlement(d["claim"], q)
            require(fee <= self.earned_fee_receivable, "FEE_OVERPAYMENT")
            self.earned_fee_receivable -= fee
        elif k == "RELEASE":
            self.reservation_remaining -= q
            if d["backing"] == "cash":
                self.cash_backed_reservation -= q
                self.beneficial_guarantee_cash -= q
                self.operating_cash += q
            else:
                self.external_backed_reservation -= q
            old_provision = self.provision_by_guarantee.get(d["guarantee"], 0)
            reverse = (old_provision if q == d["remaining_before"] else
                       old_provision * q // d["remaining_before"])
            if reverse:
                self.commitment_provision -= reverse
                self.provision_by_guarantee[d["guarantee"]] -= reverse
                self.restore_gain(d["guarantee"], reverse)
        elif k == "OFFSET":
            self.supplier_payable -= q
            if d["backing"] == "cash":
                self.cash_backed_payable -= q
                self.beneficial_guarantee_cash -= q
                self.operating_cash += q
            else:
                self.external_backed_payable -= q
            self.book_principal_settlement(d["claim"], q)
        elif k == "ACQUIRE":
            require(d["claim"] not in self.holdings, "DUPLICATE_HOLDING")
            self.operating_cash -= d["basis"]
            self.holdings[d["claim"]] = Holding(q, d["basis"])
        elif k == "DISPOSE":
            h = self.holdings[d["claim"]]
            self.operating_cash += d["price"]
            carrying = h.basis - h.allowance
            if d["price"] < carrying:
                self.absorb_loss(d["claim"], carrying - d["price"])
            benefit = max(0, min(d["price"], h.basis) - carrying)
            if benefit:
                self.restore_gain(d["claim"], benefit)
            gain = max(0, d["price"] - h.basis)
            if gain:
                self.income_proceeds_liability += gain
                self.realized_acquisition_returns += gain
            # Sale under originally accepted disposition terms ends future
            # asset proceeds. Already funded/reserved historic rights survive.
            preserve_remaining = (self.recovery_reserved.get(d["claim"], 0)
                                  + self.deferred_recovery.get(d["claim"], 0))
            for c in sorted(self.cohorts, key=lambda c: 0 if c.rank == "senior" else 1):
                if c.source == d["claim"]:
                    unclosed = max(0, c.cap - c.funded - c.resolved_unpaid)
                    preserve = min(preserve_remaining, unclosed)
                    preserve_remaining -= preserve
                    c.resolved_unpaid += unclosed - preserve
            self.source_loss_unrecovered[d["claim"]] = 0
            self.source_deficit_loss[d["claim"]] = 0
            del self.holdings[d["claim"]]
        elif k == "FACILITY_REPAY":
            self.operating_cash -= q
            self.facility_principal -= q
        else:
            raise Rejected("UNKNOWN_EFFECT")
        self.mirror_seq, self.mirror_root = e.seq, e.root

    def assert_accounting(self):
        b = self.balances()
        for v in b.values():
            checked_amount(v)
        for h in self.holdings.values():
            require(0 <= h.allowance <= h.basis <= h.face, "HOLDING_RANGE")
        for v in [self.senior_nav, self.junior_nav, self.insolvency_deficit]:
            checked_amount(v)
        require(not (self.insolvency_deficit and (self.senior_nav or self.junior_nav)),
                "DEFICIT_AND_POSITIVE_NAV")
        require(self.nav() == self.senior_nav + self.junior_nav - self.insolvency_deficit,
                "NAV_IDENTITY")
        require(self.reservation_remaining == self.cash_backed_reservation + self.external_backed_reservation,
                "RESERVATION_PARTITION")
        require(self.supplier_payable == self.cash_backed_payable + self.external_backed_payable,
                "PAYABLE_PARTITION")
        require(self.beneficial_guarantee_cash == self.cash_backed_reservation + self.cash_backed_payable,
                "ESCROW_COVERAGE")
        require(self.unallocated_recovery_liability == sum(self.recovery_reserved.values()),
                "HISTORICAL_RESERVE_IDENTITY")


@dataclass
class Pool:
    name: str
    policy: Policy
    book: Book = field(default_factory=Book)
    effects: list[Effect] = field(default_factory=list)
    hook_available: bool = True
    new_enabled: bool = True
    valuation_fresh: bool = True

    @property
    def head(self):
        return self.effects[-1].root if self.effects else "0" * 64

    def projected(self) -> Book:
        b = deepcopy(self.book)
        for effect in self.effects[b.mirror_seq:]:
            b.apply(effect)
        b.assert_accounting()
        return b

    def require_current(self, pricing=False):
        require(self.book.mirror_seq == len(self.effects) and self.book.mirror_root == self.head,
                "UNRECONCILED_SOURCE_EFFECT")
        if pricing:
            require(self.valuation_fresh, "STALE_VALUATION")

    def require_new_risk(self, additional=0):
        self.require_current(pricing=True)
        require(self.new_enabled, "NEW_RISK_PAUSED")
        b, p = self.book, self.policy
        exposure = b.exposure() + additional
        require(not b.insolvency_deficit and exposure <= p.max_exposure, "CAPITAL_OR_EXPOSURE")
        require(b.senior_nav + b.junior_nav >= (exposure * p.loss_capital_ppm + 999_999) // 1_000_000,
                "LOSS_CAPITAL")
        require(b.junior_nav >= max(p.minimum_junior,
                    (exposure * p.junior_exposure_ppm + 999_999) // 1_000_000), "JUNIOR_CAPITAL")

    def free_operating_cash(self):
        b = self.book
        # This reference profile conservatively protects all existing facility
        # principal, not only currently due installments. Production may use a
        # fully specified maturity-aware requirement; that changes liquidity
        # policy, never the NAV equation or contractual debt.
        protected = (b.refund_liability + b.accrued_expenses + b.facility_principal
                     + b.income_proceeds_liability - b.earned_fee_receivable
                     + b.unallocated_recovery_liability + self.policy.operating_buffer)
        return b.operating_cash - protected


class World:
    def __init__(self, wallets: dict[str, int]):
        self.tokens = dict(wallets)
        self.total_tokens = sum(wallets.values())
        self.unallocated_surplus: dict[str, int] = {}
        self.pools: dict[str, Pool] = {}
        self.claims: dict[str, CoreClaim] = {}
        self.guarantees: dict[str, Guarantee] = {}
        self.payables: dict[str, Payable] = {}
        self.facilities: dict[str, Facility] = {}
        self.consumed: set[str] = set()
        self.direct_effects: list[dict[str, Any]] = []

    @staticmethod
    def account(pool, kind):
        return f"{pool}:{kind}"

    def add_pool(self, name: str, policy: Policy):
        require(name not in self.pools, "DUPLICATE_POOL")
        self.pools[name] = Pool(name, policy)
        for kind in ("operating", "guarantee", "income", "recovery", "redemption", "pending"):
            self.tokens[self.account(name, kind)] = 0
        self.assert_invariants()

    def move(self, source, destination, amount):
        checked_amount(amount)
        require(source != destination or amount == 0, "SAME_ACCOUNT")
        require(self.tokens.get(source, 0) >= amount, "INSUFFICIENT_TOKENS")
        self.tokens[source] -= amount
        self.tokens[destination] = self.tokens.get(destination, 0) + amount

    def once(self, operation):
        require(operation not in self.consumed, "ECONOMIC_REPLAY")
        self.consumed.add(operation)

    def emit(self, pool_name, kind, data, mode):
        p = self.pools[pool_name]
        require(mode in ("normal", "escape"), "BAD_MODE")
        payload = {"seq": len(p.effects) + 1, "kind": kind, "data": data, "previous_root": p.head}
        root = sha256(json.dumps(payload, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
        e = Effect(**payload, root=root)
        p.effects.append(e)
        if mode == "normal":
            require(p.hook_available, "POOL_HOOK_UNAVAILABLE")
            require(p.book.mirror_seq == e.seq - 1, "UNRECONCILED_SOURCE_EFFECT")
            p.book.apply(e)
        else:
            require(kind in ("CAPTURE", "PAYOUT", "REPAY", "RELEASE", "OFFSET"),
                    "ESCAPE_CANNOT_CREATE_NEW_RISK")

    @atomic
    def reconcile(self, pool, through_seq=None):
        p = self.pools[pool]
        require(p.hook_available, "POOL_HOOK_UNAVAILABLE")
        end = len(p.effects) if through_seq is None else through_seq
        require(p.book.mirror_seq <= end <= len(p.effects), "BAD_REPLAY_BOUND")
        for e in p.effects[p.book.mirror_seq:end]:
            p.book.apply(e)

    @atomic
    def receive_unallocated(self, operation, donor, pool, location, amount):
        """Unsolicited valid-token receipt is neither repayment nor NAV income."""
        self.once(operation)
        require(location in ("operating", "guarantee"), "SURPLUS_LOCATION")
        require(pool in self.pools, "UNKNOWN_POOL")
        destination = self.account(pool, location)
        self.move(donor, destination, positive(amount))
        self.unallocated_surplus[destination] = self.unallocated_surplus.get(destination, 0) + amount

    @atomic
    def subscribe(self, pool, owner, rank, amount):
        positive(amount)
        require(rank in ("senior", "junior"), "BAD_RANK")
        p = self.pools[pool]
        p.require_current(pricing=True)
        b = p.book
        require(b.insolvency_deficit == 0, "DEFICIT_REQUIRES_EXPLICIT_RECAP")
        old_nav = getattr(b, rank + "_nav")
        units = getattr(b, rank + "_units")
        supply = sum(units.values())
        require(old_nav > 0 or supply == 0, "WIPED_SERIES_REQUIRES_SUCCESSOR")
        minted = amount * UNIT_SCALE if supply == 0 else amount * supply // old_nav
        require(minted > 0, "ZERO_SHARES")
        self.move(owner, self.account(pool, "operating"), amount)
        b.operating_cash += amount
        setattr(b, rank + "_nav", old_nav + amount)
        units[owner] = units.get(owner, 0) + minted

    def register_facility(self, facility, provider, pool, maximum):
        require(facility not in self.facilities and pool in self.pools, "BAD_FACILITY")
        self.facilities[facility] = Facility(provider, pool, positive(maximum))
        self.assert_invariants()

    @atomic
    def capture_direct(self, operation, claim, debtor, supplier, amount, mode,
                       accepted_scope_cap, cash_payer=None):
        """Core direct modes: no issuer, source reservation or invented payable.

        accepted_scope_cap is an authenticated input in the production contract;
        this model tests its monetary bound, not the signature/evidence producer.
        Mode3 MIXED is a container whose concrete slices use0/1/2 separately.
        """
        self.once(operation)
        positive(amount)
        require(mode in (0, 1), "NOT_A_DIRECT_MODE")
        require(amount <= checked_amount(accepted_scope_cap), "DIRECT_SCOPE_EXCEEDED")
        require(claim not in self.claims and supplier not in self.pools,
                "DUPLICATE_OR_WRONG_DIRECT_CREDITOR")
        if mode == 0:
            require(cash_payer is not None, "CASH_REQUIRES_ACTUAL_PAYMENT")
            require(not any(cash_payer == self.account(p, k) for p in self.pools
                            for k in ("operating", "guarantee", "income", "recovery", "redemption", "pending")),
                    "DIRECT_CASH_IS_NOT_POOL_AUTHORITY")
            locked = sum(f.reserved for f in self.facilities.values() if f.provider == cash_payer)
            require(self.tokens.get(cash_payer, 0) >= amount, "INSUFFICIENT_TOKENS")
            require(self.tokens.get(cash_payer, 0) - amount >= locked, "COMMITTED_WALLET_FLOOR")
            self.move(cash_payer, supplier, amount)
            self.claims[claim] = CoreClaim(debtor, supplier, amount, 0,
                                           repaid=amount, origin_mode=0, original_supplier=supplier)
        else:
            require(cash_payer is None, "SUPPLIER_CREDIT_HAS_NO_CAPTURE_DEBIT")
            self.claims[claim] = CoreClaim(debtor, supplier, amount, amount,
                                           origin_mode=1, original_supplier=supplier)
        self.direct_effects.append({"operation": operation, "kind": "DIRECT_CAPTURE", "claim": claim,
                                    "mode": mode, "principal": amount,
                                    "actual_cash_paid": amount if mode == 0 else 0,
                                    "issuer": None, "source": None, "payable": None})

    @atomic
    def repay_direct(self, operation, claim, principal, payer):
        self.once(operation)
        positive(principal)
        c = self.claims[claim]
        require(c.creditor not in self.pools and principal <= c.remaining,
                "WRONG_CREDITOR_OR_OVERPAYMENT")
        self.move(payer, c.creditor, principal)
        c.remaining -= principal
        c.repaid += principal
        self.direct_effects.append({"operation": operation, "kind": "DIRECT_REPAYMENT",
                                    "claim": claim, "principal": principal,
                                    "current_creditor": c.creditor, "actual_cash_paid": principal})

    def repay_current_creditor(self, operation, claim, principal, payer, fee=0, mode="normal"):
        """Route by current title, never assume an original direct claim stays unpooled."""
        if self.claims[claim].creditor in self.pools:
            return self.repay(operation, claim, principal, payer, fee=fee, mode=mode)
        require(fee == 0, "DIRECT_FEE_STREAM_OUTSIDE_REFERENCE_COVERAGE")
        return self.repay_direct(operation, claim, principal, payer)

    @atomic
    def recapitalize_deficit(self, pool, owner, amount):
        """Explicit loss-absorbing contribution; no par shares for deficit repair."""
        positive(amount)
        p = self.pools[pool]
        p.require_current(pricing=True)
        b = p.book
        deficit = b.insolvency_deficit
        require(deficit > 0 and amount >= deficit, "RECAP_MUST_COVER_ENTIRE_DEFICIT")
        require(b.senior_nav == b.junior_nav == 0, "INCONSISTENT_DEFICIT")
        self.move(owner, self.account(pool, "operating"), amount)
        b.operating_cash += amount
        b.insolvency_deficit = 0
        # Actual new junior generation. Old owners keep their immutable loss
        # cohort rights; none is granted to the rescuing investor automatically.
        b.junior_units = {}
        b.junior_nav = amount - deficit
        if b.junior_nav:
            b.junior_units[owner] = b.junior_nav * UNIT_SCALE

    @atomic
    def provider_withdraw(self, provider, destination, amount):
        locked = sum(f.reserved for f in self.facilities.values() if f.provider == provider)
        require(self.tokens[provider] - amount >= locked, "COMMITTED_WALLET_FLOOR")
        self.move(provider, destination, positive(amount))

    @atomic
    def reserve(self, operation, pool, guarantee, debtor, supplier, amount, backing="cash"):
        self.once(operation)
        positive(amount)
        require(guarantee not in self.guarantees, "DUPLICATE_GUARANTEE")
        p = self.pools[pool]
        p.require_new_risk(amount)
        if backing == "cash":
            require(p.free_operating_cash() >= amount, "PROTECTED_CASH")
            self.move(self.account(pool, "operating"), self.account(pool, "guarantee"), amount)
        else:
            f = self.facilities[backing]
            require(f.pool == pool and f.new_enabled, "WRONG_FACILITY")
            require(f.reserved + f.drawn - f.repaid + amount <= f.maximum, "FACILITY_LIMIT")
            locked = sum(x.reserved for x in self.facilities.values() if x.provider == f.provider)
            require(self.tokens[f.provider] >= locked + amount, "FACILITY_NOT_EXECUTABLE")
            f.reserved += amount
        self.guarantees[guarantee] = Guarantee(pool, debtor, supplier, amount, amount, backing)
        self.emit(pool, "RESERVE", {"amount": amount, "backing": backing}, "normal")

    @atomic
    def capture(self, operation, guarantee, claim, amount, mode="normal"):
        self.once(operation)
        positive(amount)
        g = self.guarantees[guarantee]
        require(amount <= g.remaining and claim not in self.claims, "CAPTURE_SCOPE")
        before = g.remaining
        g.remaining -= amount
        g.captured += amount
        self.claims[claim] = CoreClaim(g.debtor, g.pool, amount, amount,
                                       origin_mode=2, original_supplier=g.supplier)
        self.payables[claim] = Payable(g.pool, g.supplier, amount, amount, g.backing)
        self.emit(g.pool, "CAPTURE", {"amount": amount, "claim": claim,
                  "guarantee": guarantee, "remaining_before": before, "backing": g.backing}, mode)

    @atomic
    def fund_existing_payable(self, operation, claim, amount, mode="normal"):
        self.once(operation)
        positive(amount)
        p = self.payables[claim]
        require(amount <= p.remaining, "PAYABLE_OVERPAYMENT")
        if p.backing == "cash":
            self.move(self.account(p.pool, "guarantee"), p.supplier, amount)
        else:
            f = self.facilities[p.backing]
            require(f.pool == p.pool and amount <= f.reserved, "FACILITY_SCOPE")
            self.move(f.provider, p.supplier, amount)
            f.reserved -= amount
            f.drawn += amount
        p.remaining -= amount
        p.paid += amount
        self.emit(p.pool, "PAYOUT", {"amount": amount, "claim": claim, "backing": p.backing}, mode)

    @atomic
    def repay(self, operation, claim, principal, payer, fee=0, mode="normal"):
        self.once(operation)
        require(principal + fee > 0 and principal >= 0 and fee >= 0, "ZERO_PAYMENT")
        c = self.claims[claim]
        require(c.creditor in self.pools and principal <= c.remaining, "WRONG_CREDITOR_OR_OVERPAYMENT")
        self.move(payer, self.account(c.creditor, "operating"), principal + fee)
        c.remaining -= principal
        c.repaid += principal
        self.emit(c.creditor, "REPAY", {"amount": principal, "fee": fee, "claim": claim}, mode)

    @atomic
    def release_unused(self, operation, guarantee, amount, mode="normal"):
        self.once(operation)
        positive(amount)
        g = self.guarantees[guarantee]
        require(amount <= g.remaining, "RELEASE_EARNED_OR_OVER_SCOPE")
        before = g.remaining
        g.remaining -= amount
        g.released += amount
        if g.backing == "cash":
            self.move(self.account(g.pool, "guarantee"), self.account(g.pool, "operating"), amount)
        else:
            self.facilities[g.backing].reserved -= amount
        self.emit(g.pool, "RELEASE", {"amount": amount, "guarantee": guarantee,
                  "backing": g.backing, "remaining_before": before}, mode)

    @atomic
    def offset(self, operation, debt_claim, payable_claim, amount, mode="normal"):
        self.once(operation)
        positive(amount)
        c, p = self.claims[debt_claim], self.payables[payable_claim]
        require(c.creditor == p.pool and c.debtor == p.supplier, "NO_LEGAL_MUTUALITY")
        require(amount <= min(c.remaining, p.remaining), "OFFSET_SCOPE")
        if p.backing == "cash":
            self.move(self.account(p.pool, "guarantee"), self.account(p.pool, "operating"), amount)
        else:
            self.facilities[p.backing].reserved -= amount
        c.remaining -= amount
        c.offset += amount
        p.remaining -= amount
        p.offset += amount
        self.emit(p.pool, "OFFSET", {"amount": amount, "claim": debt_claim,
                  "payable": payable_claim, "backing": p.backing}, mode)

    @atomic
    def recognize_loss(self, pool, claim, amount):
        positive(amount)
        p = self.pools[pool]
        p.require_current()
        h = p.book.holdings[claim]
        require(amount <= h.basis - h.allowance, "LOSS_EXCEEDS_CARRYING")
        h.allowance += amount
        p.book.absorb_loss(claim, amount)
        p.valuation_fresh = False

    @atomic
    def provision_commitment(self, pool, guarantee, amount):
        positive(amount)
        p = self.pools[pool]
        p.require_current()
        g = self.guarantees[guarantee]
        old = p.book.provision_by_guarantee.get(guarantee, 0)
        require(g.pool == pool and amount <= g.remaining - old, "PROVISION_SCOPE")
        p.book.commitment_provision += amount
        p.book.provision_by_guarantee[guarantee] = old + amount
        p.book.absorb_loss(guarantee, amount)
        p.valuation_fresh = False

    @atomic
    def complete_valuation(self, pool):
        self.pools[pool].require_current()
        # In this model all assets are explicitly enumerated. This is not a
        # production pricing oracle: external evidence and aging marks are inputs.
        self.pools[pool].valuation_fresh = True

    def import_existing_claim(self, claim, debtor, creditor, face):
        require(claim not in self.claims and creditor not in self.pools, "BAD_IMPORTED_CLAIM")
        self.claims[claim] = CoreClaim(debtor, creditor, positive(face), face,
                                       origin_mode=1, original_supplier=creditor)
        self.assert_invariants()

    @atomic
    def acquire(self, operation, pool, claim, price, paid_old_fee=0, old_fee_owner="fee_owner"):
        self.once(operation)
        c = self.claims[claim]
        require(c.creditor not in self.pools, "USE_TWO_POOL_SALE")
        p = self.pools[pool]
        p.require_new_risk(c.remaining)
        basis = price + paid_old_fee
        require(0 < basis <= c.remaining and p.free_operating_cash() >= basis, "ACQUISITION_BASIS")
        self.move(self.account(pool, "operating"), c.creditor, price)
        if paid_old_fee:
            self.move(self.account(pool, "operating"), old_fee_owner, paid_old_fee)
        c.creditor = pool
        self.emit(pool, "ACQUIRE", {"amount": c.remaining, "claim": claim, "basis": basis}, "normal")

    @atomic
    def sell(self, operation, seller_pool, claim, buyer, price):
        self.once(operation)
        c = self.claims[claim]
        require(c.creditor == seller_pool, "NOT_ASSET_OWNER")
        old = self.pools[seller_pool]
        old.require_current(pricing=True)
        require(0 <= price <= c.remaining, "SALE_PRICE")
        if buyer in self.pools:
            new = self.pools[buyer]
            new.require_new_risk(c.remaining)
            require(new.free_operating_cash() >= price and price > 0, "BUYER_CASH")
            source = self.account(buyer, "operating")
        else:
            source = buyer
        self.move(source, self.account(seller_pool, "operating"), price)
        c.creditor = buyer
        self.emit(seller_pool, "DISPOSE", {"claim": claim, "price": price}, "normal")
        if buyer in self.pools:
            self.emit(buyer, "ACQUIRE", {"amount": c.remaining, "claim": claim, "basis": price}, "normal")

    @atomic
    def accrue_assigned_fee(self, pool, amount):
        positive(amount)
        p = self.pools[pool]
        p.require_current()
        p.book.earned_fee_receivable += amount
        p.book.income_proceeds_liability += amount

    @atomic
    def write_off_assigned_fee(self, pool, amount):
        positive(amount)
        p = self.pools[pool]
        p.require_current()
        require(amount <= p.book.earned_fee_receivable, "FEE_WRITEOFF_SCOPE")
        p.book.earned_fee_receivable -= amount
        p.book.income_proceeds_liability -= amount

    @atomic
    def fund_income(self, pool, amount):
        positive(amount)
        p = self.pools[pool]
        p.require_current()
        b = p.book
        require(amount <= b.income_proceeds_liability - b.earned_fee_receivable,
                "UNCOLLECTED_INCOME")
        # Release only this exact source-owned amount from the protected floor.
        require(p.free_operating_cash() + amount >= amount and not b.insolvency_deficit,
                "PRIOR_CREDITORS_OR_CASH")
        self.move(self.account(pool, "operating"), self.account(pool, "income"), amount)
        b.operating_cash -= amount
        b.income_proceeds_liability -= amount
        b.income_funded += amount

    @atomic
    def fund_recovery(self, pool, source, amount):
        positive(amount)
        p = self.pools[pool]
        p.require_current()
        b = p.book
        require(amount <= b.recovery_reserved.get(source, 0), "RECOVERY_NOT_RESERVED")
        require(p.free_operating_cash() >= 0 and not b.insolvency_deficit, "PRIOR_CREDITORS_OR_CASH")
        left = amount
        # All original impaired senior for this source precede original junior.
        for rank in ("senior", "junior"):
            for c in b.cohorts:
                if c.source == source and c.rank == rank and left:
                    take = min(left, c.cap - c.funded - c.resolved_unpaid)
                    c.funded += take
                    left -= take
        require(left == 0, "HISTORICAL_ENTITLEMENT_SCOPE")
        self.move(self.account(pool, "operating"), self.account(pool, "recovery"), amount)
        b.operating_cash -= amount
        b.unallocated_recovery_liability -= amount
        b.recovery_reserved[source] -= amount
        b.recovery_funded += amount

    @atomic
    def claim_recovery(self, pool, cohort_index, owner, final=False):
        p = self.pools[pool]
        # Already-funded fixed escrow claims remain live even while mirror/NAV
        # are stale; they do not use operating money or current LP share rights.
        c = p.book.cohorts[cohort_index]
        weight = c.owner_losses.get(owner, 0)
        if final:
            require(c.funded + c.resolved_unpaid == c.cap, "SOURCE_NOT_FINAL")
            entitlement = exact_allocation(c.funded, c.owner_losses).get(owner, 0)
        else:
            entitlement = weight * c.funded // c.cap if c.cap else 0
        pay = entitlement - c.paid.get(owner, 0)
        require(pay > 0, "NO_FUNDED_CLAIM")
        self.move(self.account(pool, "recovery"), owner, pay)
        c.paid[owner] = c.paid.get(owner, 0) + pay
        p.book.recovery_funded -= pay
        return pay

    @atomic
    def pay_facility(self, operation, facility, amount):
        self.once(operation)
        positive(amount)
        f = self.facilities[facility]
        p = self.pools[f.pool]
        p.require_current()
        require(amount <= f.drawn - f.repaid, "FACILITY_OVERPAYMENT")
        # Actual debt service can precede residual LP distributions; no new NAV
        # or share-pricing gate. It cannot use the Core guarantee escrow.
        self.move(self.account(f.pool, "operating"), f.provider, amount)
        f.repaid += amount
        self.emit(f.pool, "FACILITY_REPAY", {"amount": amount}, "normal")

    @atomic
    def redeem(self, pool, owner, rank, units):
        positive(units)
        p = self.pools[pool]
        p.require_current(pricing=True)
        b = p.book
        ownership = getattr(b, rank + "_units")
        require(units <= ownership.get(owner, 0), "SHARE_SCOPE")
        amount = units * getattr(b, rank + "_nav") // sum(ownership.values())
        require(amount > 0 and p.free_operating_cash() >= amount, "PROTECTED_CASH")
        ownership[owner] -= units
        setattr(b, rank + "_nav", getattr(b, rank + "_nav") - amount)
        self.move(self.account(pool, "operating"), owner, amount)
        b.operating_cash -= amount
        p.require_new_risk(0)
        return amount

    def snapshot(self, pool):
        p = self.pools[pool]
        b = p.projected()
        return {**b.balances(), "signed_net_assets": b.nav(),
                "senior_nav": b.senior_nav, "junior_nav": b.junior_nav,
                "insolvency_deficit": b.insolvency_deficit,
                "remaining_guarantees": b.reservation_remaining,
                "credit_exposure": b.exposure(),
                "mirror_seq": p.book.mirror_seq, "canonical_effect_seq": len(p.effects),
                "mirror_current": p.book.mirror_seq == len(p.effects),
                "historical_escrow": self.tokens[self.account(pool, "recovery")],
                "income_escrow": self.tokens[self.account(pool, "income")],
                "tokens_system_total": sum(self.tokens.values())}

    def snapshot_direct(self, claim, parties):
        c = self.claims.get(claim)
        p = self.payables.get(claim)
        return {"origin_mode": c.origin_mode if c else None,
                "debtor": c.debtor if c else None,
                "current_creditor": c.creditor if c else None,
                "original_principal": c.original if c else 0,
                "debt_remaining": c.remaining if c else 0,
                "principal_repaid": c.repaid if c else 0,
                "issuer": p.pool if p else None,
                "payable_exists": p is not None,
                "payable_remaining": p.remaining if p else 0,
                "source": p.backing if p else None,
                "tokens": {party:self.tokens.get(party,0) for party in parties},
                "tokens_system_total":sum(self.tokens.values()),
                "pool_effect_heads":{n:len(p.effects) for n,p in self.pools.items()}}

    def assert_invariants(self):
        require(sum(self.tokens.values()) == self.total_tokens, "TOKEN_CONSERVATION")
        for amount in self.tokens.values():
            checked_amount(amount)
        for c in self.claims.values():
            require(c.original == c.remaining + c.repaid + c.offset + c.credited, "LEGAL_DEBT_CONSERVATION")
        for claim, c in self.claims.items():
            if c.origin_mode in (0,1):
                require(claim not in self.payables, "DIRECT_MODE_CANNOT_HAVE_ISSUER_PAYABLE")
        for g in self.guarantees.values():
            require(g.accepted == g.remaining + g.captured + g.released, "GUARANTEE_CONSERVATION")
        for q in self.payables.values():
            require(q.original == q.remaining + q.paid + q.offset + q.corrected, "PAYABLE_CONSERVATION")
        for name, p in self.pools.items():
            b = p.projected()
            require(b.operating_cash + self.unallocated_surplus.get(self.account(name, "operating"), 0) == self.tokens[self.account(name, "operating")], "OPERATING_CASH_RECONCILIATION")
            require(b.beneficial_guarantee_cash + self.unallocated_surplus.get(self.account(name, "guarantee"), 0) == self.tokens[self.account(name, "guarantee")], "GUARANTEE_CASH_RECONCILIATION")
            require(b.balances()["gross_debt"] == sum(c.remaining for c in self.claims.values() if c.creditor == name), "CORE_MIRROR_DEBT")
            require(b.supplier_payable == sum(x.remaining for x in self.payables.values() if x.pool == name), "CORE_MIRROR_PAYABLE")
            require(b.reservation_remaining == sum(x.remaining for x in self.guarantees.values() if x.pool == name), "CORE_MIRROR_RESERVATION")
            require(b.income_funded == self.tokens[self.account(name, "income")], "INCOME_ESCROW")
            require(b.recovery_funded == self.tokens[self.account(name, "recovery")], "RECOVERY_ESCROW")
            require(b.facility_principal == sum(f.drawn - f.repaid for f in self.facilities.values() if f.pool == name), "FACILITY_DEBT")
        for provider in {f.provider for f in self.facilities.values()}:
            locked = sum(f.reserved for f in self.facilities.values() if f.provider == provider)
            require(self.tokens[provider] >= locked, "GLOBAL_PROVIDER_COMMITMENT")
