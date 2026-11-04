#!/usr/bin/env python3
"""New signed native-v1 wallet/escrow and accepted-servicing acceptance cases."""
import copy
import hashlib
import struct

from financing_fixture import *


def malformed_graphs(f):
    party = f.parties["financier"]
    discriminator = bytes(f.registry.instructions["authorize_funding_wallet"]["discriminator"])
    for name, offset, selector in (("wrong prefix offset", 39, discriminator),
            ("wrong authority instruction", 40, bytes([77]) * 8)):
        wallet_id = ident("rejected-graph-" + name)
        config = pda(SWIG, b"swig", wallet_id)
        wallet = pda(SWIG, b"swig-wallet-address", config)
        binding = pda(REGISTRY, b"funding-wallet", party["principal"], config)
        f.observe(config, wallet, binding)
        bump = Pubkey.find_program_address([b"swig", wallet_id], SWIG)[1]
        wallet_bump = Pubkey.find_program_address([b"swig-wallet-address", bytes(config)], SWIG)[1]
        auth = bytes(REGISTRY) + bytes([offset]) + bytes(7) + selector + bytes(binding)
        create = Instruction(SWIG, struct.pack("<HHHBB", 0, 7, len(auth), bump, wallet_bump)
            + wallet_id + auth + struct.pack("<HHI", 7, 0, 8),
            [M(config, w=True), M(f.h.payer.pubkey(), True, True), M(wallet, w=True), M(SYSTEM)])
        bind = f.ix("principal_registry", "bind_funding_wallet",
            dict(owner=party["owner"].pubkey(), principal=party["principal"], config=config,
                wallet=wallet, binding=binding, swig_program=SWIG),
            args=dict(wallet_id=wallet_id, authority_generation=party["generation"]))
        f.send("complete Swig graph rejects " + name, [create, bind], [party["owner"]], reject=True)


def shared_custody_facilities(f):
    financier = f.parties["financier"]
    second_facility = f.open_facility(name="shared-wallet-second")
    second_line, second_policy = f.open_line(second_facility, "shared-wallet-second")
    second_mandate = f.grant(second_facility, name="shared-wallet-second")
    first = f.acceptance("shared-wallet-first", cap=120_000, maximum_interest=0, fees=False)
    second = f.acceptance("shared-wallet-second", cap=180_000, maximum_interest=0, fees=False,
        facility=second_facility, line=second_line, policy=second_policy, mandate=second_mandate)
    before = f.state("FinancierCustody", financier["custody"])
    f.accept(first, label="first facility accepts work against the financier wallet")
    f.accept(second, label="second facility accepts work against the same financier wallet")
    for facility, tab, amount in ((f.facility, first, 120_000), (second_facility, second, 180_000)):
        facility_state = f.state("FundingFacility", facility)
        f.h.check("both facilities identify the same authoritative custody", facility_state["custody"], financier["custody"])
        f.h.check("each facility retains its own finite commitment", facility_state["reserved"], amount)
        f.h.check("each accepted claim retains the shared custody", f.state("FundedClaim", tab["claim"])["custody"], financier["custody"])
    shared = f.state("FinancierCustody", financier["custody"])
    f.h.check("shared reserve includes both facilities", shared["unused"] + shared["payable"], 300_000)
    f.h.check("eligible owner cash excludes both facilities", shared["accounted_cash"] - shared["unused"] - shared["payable"], 700_000)
    f.h.check("accepting both commitments leaves cash in the same wallet", f.h.amount(financier["custody_token"]), 1_000_000)
    f.release(first, "shared-wallet-first-unused", 120_000)
    f.release(second, "shared-wallet-second-unused", 180_000)
    released = f.state("FinancierCustody", financier["custody"])
    for field in ("accounted_cash", "unused", "payable", "contributed_capital", "withdrawn_cash"):
        f.h.check("valid releases preserve original custody " + field, released[field], before[field])


def supplier_owner_continuity(f, previous_tab, previous_run, facility, line, policy):
    supplier = f.parties["supplier"]
    old_destination = f.h.ata(previous_tab["terms"]["supplier"], f.mint)
    retained = f.h.snapshot([previous_tab["claim"], previous_run["run"], previous_run["receipt"], old_destination])
    principal = f.registry.read(f.h.svm, supplier["principal"], "Principal")
    f.h.check("supplier continues with its replacement owner", principal["owner"], supplier["owner"].pubkey())
    supplier["owner_ata"] = f.h.ata(supplier["owner"].pubkey(), f.mint)
    f.observe(supplier["owner_ata"])
    f.send("replacement supplier owner prepares its payment destination", f.h.ata_ix(supplier["owner"].pubkey(), f.mint))
    mandate = f.grant(facility, 1, name="supplier-after-owner-change")
    tab = f.acceptance("supplier-after-owner-change", cap=6_000, maximum_interest=0, index=1,
        financier="issuer", facility=facility, line=line, policy=policy, fees=False, mandate=mandate)
    f.accept(tab, label="continuing supplier accepts a new Tab with its current payment wallet")
    run_name = "distinct-work-after-supplier-owner-change"
    run = f.earn(tab, run_name, 6_000)
    run_id = ident("completed-run-" + run_name)
    expected_run = pda(CORE, b"usage", supplier["principal"], tab["namespace"], run_id)
    f.h.check("new run address uses the continuing supplier Principal", run["run"], expected_run)
    earlier = f.state("FundedRun", previous_run["run"])
    current = f.state("FundedRun", run["run"])
    f.h.check("old and new work retain the same supplier Principal", current["supplier_principal"], earlier["supplier_principal"])
    f.h.check("new work retains the newly accepted payment wallet", current["supplier"], supplier["owner"].pubkey())
    before_cash = f.h.amount(supplier["owner_ata"])
    collection = f.collection(tab, run, 6_000)
    f.h.check("collection uses the new Tab's accepted destination", collection["accounts"]["destination"], supplier["owner_ata"])
    f.collect(collection, label="independent collection pays the continuing supplier's newly accepted wallet")
    f.h.check("new supplier wallet receives exactly its distinct earned amount", f.h.amount(supplier["owner_ata"]) - before_cash, 6_000)
    f.h.check("new work preserves earlier accepted records and payment destination",
        f.h.snapshot(list(map(Pubkey.from_string, retained))), retained)


def exercise(f):
    f.setup()
    financier, supplier, issuer = [f.parties[x] for x in ("financier", "supplier", "issuer")]
    malformed_graphs(f)
    shared_custody_facilities(f)

    # Prepare the alternative before any projection outage. Later new-risk
    # admission therefore has adequate cash and a valid live agent/mandate.
    escrow_facility = f.open_facility("issuer", "issuer-owned", external=False)
    escrow_line, escrow_policy = f.open_line(escrow_facility, "issuer-owned")
    f.grant(escrow_facility, 1)
    tab = f.acceptance("wallet-core-case")
    accepted = f.accept(tab)
    f.accept(tab, label="exact acceptance retry does not refill limits or allocate capital")
    f.exercise_fee_parent(tab)
    f.h.check("accepted gross cash protection", f.state("FinancierCustody", financier["custody"])["unused"], 100_000)
    f.h.check("authority includes separate buyer finance ceiling", f.registry.read(f.h.svm, f.budget, "Budget")["reserved"], 110_000)
    f.positions("company", customer_reserved=100_000, credit_charge_exposure=10_000, customer_principal=0)
    f.positions("issuer", unused_supplier=100_000, unpaid_supplier=0, owned_principal=0)
    f.positions("supplier", supplier_committed=100_000, supplier_receivable=0)

    run = f.earn(tab, "one-metered-run", 60_000, payment_name="worker-one")
    before = f.h.snapshot([tab["claim"], tab["reservation"], f.budget, financier["custody"], run["receipt"]])
    alias = f.earn(tab, "one-metered-run", 60_000, payment_name="worker-two", expected_revision=0,
        label="independent worker payment ID recovers one already-billed run")
    f.h.check("two worker aliases share one immutable receipt", alias["receipt"], run["receipt"])
    f.h.check("second worker does not change obligations or cash", f.h.snapshot(list(map(Pubkey.from_string, before))), before)
    f.earn(tab, "one-metered-run", 60_001, payment_name="conflicting-worker", reject=True,
        label="same delivered-run identity cannot carry a conflicting quantity")
    f.h.check("one run creates exactly one company debt", f.state("FundedClaim", tab["claim"])["debt"], 60_000)

    company = f.parties["company"]
    f.send("owner revokes initiating agent after the supplier earns payment",
        f.ix("principal_registry", "revoke_actor",
            dict(owner=company["owner"].pubkey(), principal=f.company, actor=f.actors[0]),
            expected_generation=1), [company["owner"]])
    refused = f.acceptance("revoked-agent-new-spend", cap=1, maximum_interest=0, fees=False)
    f.accept(refused, reject=True, label="revoked initiating agent cannot create a new commitment")
    f.manage_facility("disable-and-lower-below-existing-exposure", limit=0, enabled=False)
    f.withdraw(f.withdrawal("one-more-than-free", 900_001), reject=True,
        label="owner cannot withdraw a single protected token")
    free = f.withdrawal("eligible-free-cash", 900_000)
    f.withdraw(free)
    f.withdraw(free, label="withdrawal economic retry never debits twice")
    f.h.check("owner keeps access to uncommitted funds", f.h.amount(financier["owner_ata"]), 900_000)
    f.h.check("all accepted commitments remain backed", f.h.amount(financier["custody_token"]), 100_000)

    old_owner = f.rotate_financier()
    new_ata = f.h.ata(financier["owner"].pubkey(), f.mint)
    f.send("new owner prepares its ordinary cash destination", f.h.ata_ix(financier["owner"].pubkey(), f.mint))
    financier["owner_ata"] = new_ata
    f.withdraw(f.withdrawal("old-owner-after-control-change", 1, owner=old_owner, generation=1), reject=True,
        label="former owner cannot withdraw after a completed owner rotation")
    f.withdraw(f.withdrawal("new-owner-protected-reserve", 1), reject=True,
        label="new owner also cannot spend previously committed reserves")

    collect_one = f.collection(tab, run, 30_000, outage=True)
    f.collect(collect_one, reject=True, bad_hash=True,
        label="changed wallet bundle hash refuses before moving any money")
    bare = compact_sign(financier["config"], financier["wallet"], collect_one["instruction"])
    f.send("Swig alone cannot bypass Registry qualification", bare, reject=True)
    envelope = f.wallet_bundle(collect_one["instruction"], action=1, caller=f.h.payer, generation=0)
    unrelated = Instruction(SYSTEM, struct.pack("<IQ", 2, 1),
        [M(f.h.payer.pubkey(), True, True), M(old_owner.pubkey(), w=True)])
    f.send("wallet authorization refuses an appended unrelated instruction", [*envelope, unrelated], reject=True)
    # Rollback is checked after the Core transfer would have happened: journal
    # validation is deliberately last and receives an incomplete book set.
    bad = copy.copy(collect_one)
    bad["instruction"] = f.ix("claims_core", "collect_financier", collect_one["accounts"],
        remaining=f.remaining(collect_one["roles"], True)[:-1], **collect_one["args"])
    f.collect(bad, reject=True, label="incomplete final accounting rolls back supplier and fee token legs")
    envelope = f.collect(collect_one,
        label="independent supplier collects from rotated financier wallet with backend absent and Pool omitted")
    signed = f.h.cases[-1]
    tx = VersionedTransaction.from_bytes(__import__("base64").b64decode(signed["transaction_base64"]))
    f.h.check("earned collection needs only independent transaction payer signature", len(tx.signatures), 1)
    f.h.check("financier backend owner is not a collection signer", str(financier["owner"].pubkey()) in signed["account_keys"], False)
    f.h.check("former agent is not a collection signer", str(f.agent.pubkey()) in signed["account_keys"], False)
    f.h.check("first collection preserves company debt", f.state("FundedClaim", tab["claim"])["debt"], 60_000)

    escrow_tab = f.acceptance("escrow-alternative", cap=20_000, maximum_interest=2_000, index=1,
        financier="issuer", facility=escrow_facility, line=escrow_line, policy=escrow_policy, fees=False)
    f.accept(escrow_tab, reject=True, label="cash-backed new risk refuses stale company supplier and issuer projections")
    f.collect(collect_one, label="fresh public attempt nonce recovers original supplier receipt without another debit")
    collect_two = f.collection(tab, run, 30_000, outage=True)
    f.collect(collect_two, label="supplier collects the remaining earned range without any fresh owner approval")
    f.collect(f.collection(tab, run, 1, paid_before=29_999, outage=True), reject=True,
        label="overlapping historical range cannot collect another token")
    f.h.check("supplier receives accepted gross minus aggregate fee", f.h.amount(supplier["owner_ata"]), 59_171)
    f.h.check("seller fee is cumulative with one fixed component", f.h.amount(f.parties["fee"]["custody_token"]), 829)
    f.h.check("external facility is drawn only on actual payout", f.state("FundingFacility", f.facility)["drawn"], 60_000)
    f.positions("issuer", owned_principal=60_000, unpaid_supplier=0, unused_supplier=40_000, facility_debt=60_000)
    f.positions("supplier", supplier_receivable=0, supplier_committed=40_000, cash_received=59_171, seller_fees_paid=829)
    f.positions("financier", funding_reserved=40_000, facility_assets=60_000, cash_sent=960_000)
    f.release(tab, "supplier-surrender-required", 40_000, outage=True, supplier=False, reject=True)
    f.release(tab, "unused-finite-work", 40_000, outage=True)
    f.withdraw(f.withdrawal("released-reserve-now-free", 40_000, outage=True))
    f.h.check("unused release frees cash without erasing debt", f.state("FundedClaim", tab["claim"])["debt"], 60_000)
    f.h.check("new owner may withdraw released funds", f.h.amount(new_ata), 40_000)
    f.h.check("company allowance retains only accepted unaccrued interest exposure",
        f.registry.read(f.h.svm, f.budget, "Budget")["reserved"], 6_000)
    f.positions("company", customer_reserved=0, customer_principal=60_000, credit_charge_exposure=6_000)
    f.positions("supplier", supplier_committed=0, supplier_receivable=0)
    f.positions("financier", capital_allocated=1_000_000, funding_reserved=0, facility_assets=60_000, cash_sent=1_000_000)

    latest = f.receipts[-1]
    effect = latest["data"]["effects"][0]
    f.send("projection refuses a later receipt before its missing predecessor",
        f.ix("pool_issuer", "project_financing", dict(book=effect["book"],
            projection=pda(POOL, b"financial-projection", effect["book"]), receipt=Pubkey.from_string(latest["address"]))), reject=True)
    f.catch_up()
    f.accept(escrow_tab, label="new risk proceeds after independent ordered projection repair")
    first = f.earn(escrow_tab, "escrow-run-one", 10_000)
    second = f.earn(escrow_tab, "escrow-run-two", 10_000)
    f.h.check("different completed runs at the same price have distinct receipts", first["receipt"] == second["receipt"], False)
    f.collect(f.collection(escrow_tab, first, 10_000), label="issuer-owned Core escrow pays one earned run")
    f.collect(f.collection(escrow_tab, second, 10_000), label="issuer-owned Core escrow pays a separate equal-price run")
    own = f.state("FundingFacility", escrow_facility)
    f.h.check("issuer-owned capital creates no issuer-to-itself funded debt", own["drawn"], 0)
    f.h.check("escrow payout also preserves company obligation", f.state("FundedClaim", escrow_tab["claim"])["debt"], 20_000)
    f.positions("issuer", owned_principal=80_000, unpaid_supplier=0, facility_debt=60_000)

    # Fee routing is a paid service, not a veto over already-earned rights.
    f.open_fee_agreement("unavailable-service", 500)
    fee_tab = f.acceptance("unavailable-fee-service", cap=10_000, maximum_interest=0, index=1,
        financier="issuer", facility=escrow_facility, line=escrow_line, policy=escrow_policy,
        fee_agreement="unavailable-service")
    f.accept(fee_tab)
    fee_run = f.earn(fee_tab, "fee-service-offline", 10_000)
    f.freeze(f.parties["fee"]["custody_token"])
    fee_collection = f.collection(fee_tab, fee_run, 10_000, fee_available=False)
    f.collect(fee_collection, label="frozen accepted fee recipient cannot block supplier full-net collection")
    data = check_receipt(f.core, f.h.svm, fee_collection["receipt"])
    f.h.check("unavailable fee is explicitly waived", data["seller_fee_waived"], 144)
    f.h.check("supplier receives full gross when paid fee route is unavailable", data["supplier_net"], 10_000)
    f.h.check("fee account cannot receive unavailable collection", f.h.amount(f.parties["fee"]["custody_token"]), 829)
    f.freeze(f.parties["fee"]["custody_token"], False)
    f.collect(fee_collection, label="restoring hosted fee availability cannot retroactively bill a waived fee")
    f.h.check("main agreement still retains every child allocation after collection and release", f.fee_allocated(), 2_069)
    f.h.check("fee waiver cannot refill its own standing authorization", f.fee_allocated("unavailable-service"), 144)
    f.fee_parent_owner_change()
    f.catch_up()
    supplier_owner_continuity(f, tab, run, escrow_facility, escrow_line, escrow_policy)
    return dict(tab=tab, run=run, collections=[collect_one, collect_two], escrow=escrow_tab)


def main():
    f = FinancingFixture()
    try:
        exercise(f)
    except Exception as error:
        f.finish("financing-wallet", error=error)
        raise
    f.finish("financing-wallet")


if __name__ == "__main__":
    main()
