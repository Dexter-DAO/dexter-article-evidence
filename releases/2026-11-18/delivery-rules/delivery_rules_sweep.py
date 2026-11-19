#!/usr/bin/env python3
"""Seeded sweep of the delivery-rules signers' outcome against an independent brute force.

Adapted from the review's probe3_group_sweep.py. For every signer count n = 1..5,
group size k = 1..n and both silence values, two orders with random attestations
(values from a small set so prices tie often; about 30% of signers silent) are
settled after objections close, on delivery_rules.py's fixture with the retained
binary. Each case's outcome, group mask and units, and the bill's report, are
compared with the brute force below, written from the rule's statement:

    pool   = the attesting signers when at least k attested, otherwise all n
             (a silent member then supports the silence value)
    group  = every k-subset of the pool; per metric the smallest member amount
    price  = per component ceil(units * numerator / denominator), summed
    report = the highest price; among equal prices the lowest mask (signer i = bit i)

Run in a prepared checkout (after ./dev client build):
  ./dev exec python foundation/onchain/tests/delivery_rules_sweep.py [--record FILE]
"""
import itertools
import random

from delivery_rules import *

RATES = ((3, 7, 600), (5, 3, 300), (1, 1, 900))   # rounding on the first two metrics
SEED = 20261118


def price(units):
    total = 0
    for quantity, (num, den, _max) in zip(units, RATES):
        product = quantity * num
        total += product // den + (1 if product % den else 0)
    return total


def brute(reports, n, k, silent, *, silence_always=False):
    """`silence_always` is the rule of commit ee0fc3b, kept to count the orders whose answer the change moves."""
    attested = [i for i in range(n) if reports[i] is not None]
    pool = attested if len(attested) >= k and not silence_always else list(range(n))
    best = None
    for members in itertools.combinations(pool, k):
        mask = sum(1 << i for i in members)
        vectors = [reports[i] if reports[i] is not None else silent for i in members]
        units = tuple(min(v[m] for v in vectors) for m in range(len(RATES)))
        p = price(units)
        if best is None or p > best[0] or (p == best[0] and mask < best[1]):
            best = (p, mask, units)
    return best


def main():
    rng = random.Random(SEED)
    f = DeliveryRules()
    setup(f)
    f.mandates[0] = f.permit("agent-1", 0)
    keys = [f.carrier, f.carrier_b, f.carrier_c, key(103), key(104)]
    for holder in keys[3:]:
        f.h.svm.airdrop(holder.pubkey(), 100_000_000_000)
    buyer, inspector = f.buyer.pubkey(), f.inspector.pubkey()
    maxima = tuple(r[2] for r in RATES)
    plans = []
    for n in range(1, 6):
        for k in range(1, n + 1):
            for silence in (NOTHING, PAID):
                terms = rule_terms([h.pubkey() for h in keys[:n]], k, signers_silent=silence, objectors=[buyer], decider=inspector)
                rule = f.create_rule(terms, f"sweep: rule n={n} k={k} silence={silence}", case="S")
                for copy in range(2):
                    reports = [None if rng.random() < 0.3 else tuple(rng.choice((0, maxima[m] // 3, maxima[m] // 2, maxima[m]))
                        for m in range(len(RATES))) for _ in range(n)]
                    plans.append(dict(n=n, k=k, silence=silence, rule=rule, reports=reports, name=f"sw-{n}-{k}-{silence}-{copy}"))
    for plan in plans:
        plan["tab"] = f.rule_order(plan["name"], plan["rule"], rates=RATES)
        f.place_rule(plan["tab"], f"sweep: order {plan['name']}", case="S")
        f.open_case(plan["tab"], f"sweep: open {plan['name']}", case="S")
        for holder, units in zip(keys, plan["reports"]):
            if units is not None:
                f.attest(plan["tab"], holder, units, f"sweep: attest {plan['name']}", case="S")
    f.at(OBJECTIONS_CLOSE + 1)
    mismatches, ties, cus, pools, moved = [], 0, [], dict(attesting=0, all_signers=0), []
    for plan in plans:
        silent = maxima if plan["silence"] == PAID else (0,) * len(RATES)
        expected = brute(plan["reports"], plan["n"], plan["k"], silent)
        previous = brute(plan["reports"], plan["n"], plan["k"], silent, silence_always=True)
        if previous[0] != expected[0]:
            moved.append(dict(order=plan["name"], silence="paid" if plan["silence"] == PAID else "nothing",
                previous_price=previous[0], price=expected[0]))
        attested = sum(r is not None for r in plan["reports"])
        pools["attesting" if attested >= plan["k"] else "all_signers"] += 1
        pool = [i for i in range(plan["n"]) if plan["reports"][i] is not None] if attested >= plan["k"] else range(plan["n"])
        tied = [m for m in itertools.combinations(pool, plan["k"])
            if price(tuple(min((plan["reports"][i] if plan["reports"][i] is not None else silent)[x] for i in m)
                for x in range(len(RATES)))) == expected[0]]
        ties += len(tied) > 1
        try:
            row = f.settle_rule(plan["tab"], f"sweep: settle {plan['name']}", case="S", expect=(SIGNERS, expected[1], expected[2]))
            cus.append(row["result"]["compute_units_consumed"])
        except AssertionError as error:
            mismatches.append(dict(plan=plan["name"], reports=plan["reports"], expected=expected, error=str(error)[:300]))
    summary = dict(seed=SEED, retained_sha256=f.rule_sha256, orders=len(plans),
        configurations=len({(p["n"], p["k"], p["silence"]) for p in plans}), orders_by_pool=pools,
        orders_with_tied_best_price=ties, orders_whose_price_the_silence_change_moves=moved,
        mismatches=mismatches, settle_cu_min=min(cus), settle_cu_max=max(cus))
    text = json.dumps(summary, indent=1, default=str)
    print(text)
    if "--record" in sys.argv:
        Path(sys.argv[sys.argv.index("--record") + 1]).write_text(text + "\n")
    assert not mismatches, "the program and the brute force disagree"


if __name__ == "__main__":
    main()
