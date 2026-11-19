#!/usr/bin/env python3
"""Measure the accounts a flow creates and the lamports they still hold after it.

usage: flow-rent.py SPEC.json [--side NAME=TRACE_DIR:IDL_DIR ...] [--out PREFIX]

SPEC: {"sides": {"main": {"traces": DIR, "idl": DIR}, "branch": {...}},
       "flows": [{"flow": "...", "main": SELECTOR, "branch": SELECTOR}, ...]}
SELECTOR: {"trace": "<suite>", "names": [exact names, in order; each the next successful one]}
          or {"trace": "<suite>", "before": "<name>"}: every successful transaction before it (setup)
          or {"trace": "<suite>", "tab_from": "<commit transaction name>"}: every
          transaction, from that commit on, that names the Tab the commit created.
Traces must carry `account_lamports` (per transaction, every account key:
[lamports, owner, data length, first 8 data bytes] before and after).

An account counts as created by the flow when a flow transaction finds it
absent and leaves it present. Its rent is the lamports it held when created.
Locked afterwards is what flow-created accounts hold after the flow's last
transaction (an account a later flow transaction closes holds nothing). Fee
payers and token-balance changes are not rent and are not counted.
"""
import argparse
import hashlib
import json
import sys
from pathlib import Path

TOKEN_PROGRAMS = {"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA": "SPL token account",
                  "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb": "Token-2022 account"}
OWNERS = {"727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz": "claims_core",
          "9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G": "principal_registry",
          "3yceApSVxLYihHjLLrCycV8T4Bg1MYggbxQhdzaqB2Sm": "pool_issuer",
          "EsR1knVgJG8NAYFjeGCYpTuvjf1DtwAU1ne5XBdcotq3": "delivery_rules",
          "swigypWHEksbC64pWKwah1WTeh9JXwx8H1rJHLdbQMB": "swig",
          "11111111111111111111111111111111": "system"}
LAMPORTS_PER_SOL = 1_000_000_000


def discriminators(idl_dir):
    names = {}
    for path in sorted(Path(idl_dir).glob("*.json")):
        idl = json.loads(path.read_text())
        for account in idl.get("accounts", []):
            names[hashlib.sha256(b"account:" + account["name"].encode()).digest()[:8].hex()] = \
                path.stem + "." + account["name"]
    return names


def kind(state, names):
    lamports, owner, length, head = state
    if owner in TOKEN_PROGRAMS:
        return TOKEN_PROGRAMS[owner]
    if owner == "11111111111111111111111111111111" and length == 0:
        return "system account"
    if head in names:
        return names[head]
    return "%s account (%d bytes)" % (OWNERS.get(owner, owner), length)


def rows_of(trace):
    return trace.get("transactions") or trace.get("transaction_cases") or []


def select(trace, selector):
    rows = rows_of(trace)
    if "names" in selector:
        wanted, picked = list(selector["names"]), []
        index = 0
        for name in wanted:
            while index < len(rows) and (rows[index]["name"] != name or rows[index]["result"].get("err") is not None):
                index += 1
            if index == len(rows):
                raise SystemExit("transaction not found in order: " + name)
            picked.append(index)
            index += 1
        return picked
    if "before" in selector:  # one-time setup: every successful transaction before the named one
        end = next((i for i, row in enumerate(rows) if row["name"] == selector["before"]), None)
        if end is None:
            raise SystemExit("transaction not found: " + selector["before"])
        return [i for i in range(end) if rows[i]["result"].get("err") is None]
    start = next((i for i, row in enumerate(rows) if row["name"] == selector["tab_from"]), None)
    if start is None:
        raise SystemExit("commit not found: " + selector["tab_from"])
    created = [key for key, (before, after) in rows[start]["account_lamports"].items()
               if before is None and after is not None and after[3] == hashlib.sha256(b"account:FundedClaim").digest()[:8].hex()]
    if len(created) != 1:
        raise SystemExit("commit did not create exactly one Tab: %s" % selector["tab_from"])
    tab = created[0]
    picked = [i for i in range(start, len(rows)) if tab in rows[i]["account_keys"] and rows[i]["result"].get("err") is None]
    return picked


def measure(trace, picked, names):
    rows = rows_of(trace)
    created, order = {}, []
    for index in picked:
        row = rows[index]
        if row["result"].get("err") is not None:
            continue
        fee_payer = row["account_keys"][0]
        for key, (before, after) in row["account_lamports"].items():
            if key == fee_payer:
                continue
            if before is None and after is not None and key not in created:
                created[key] = dict(type=kind(after, names), rent=after[0], created_by=row["name"])
                order.append(key)
    last_state = {}
    for index in range(0, picked[-1] + 1):
        row = rows[index]
        if row["result"].get("err") is not None:
            continue
        for key, (_, after) in row["account_lamports"].items():
            last_state[key] = after
    accounts = []
    for key in order:
        state = last_state.get(key)
        accounts.append(dict(address=key, **created[key], locked_after=0 if state is None else state[0],
                             closed=state is None))
    return dict(transactions=[rows[i]["name"] for i in picked], accounts=accounts,
                created=len(accounts), rent=sum(a["rent"] for a in accounts),
                locked_after=sum(a["locked_after"] for a in accounts),
                locked_accounts=sum(1 for a in accounts if not a["closed"]))


def sol(lamports):
    return "%.9f" % (lamports / LAMPORTS_PER_SOL)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("spec")
    parser.add_argument("--out", default="flow-rent")
    parser.add_argument("--side", action="append", default=[],
        help="NAME=TRACE_DIR:IDL_DIR, overriding or adding a side of the spec")
    options = parser.parse_args()
    spec = json.loads(Path(options.spec).read_text())
    for item in options.side:
        side, paths = item.split("=", 1)
        traces, idl = paths.split(":", 1)
        spec.setdefault("sides", {})[side] = dict(traces=traces, idl=idl)
    names = {side: discriminators(value["idl"]) for side, value in spec["sides"].items()}
    result = []
    for flow in spec["flows"]:
        entry = dict(flow=flow["flow"], note=flow.get("note"))
        for side, value in spec["sides"].items():
            if side not in flow:
                continue
            selector = flow[side]
            trace = json.loads((Path(value["traces"]) / (selector["trace"] + "-trace.json")).read_text())
            entry[side] = dict(trace=selector["trace"], **measure(trace, select(trace, selector), names[side]))
        result.append(entry)
    Path(options.out + ".json").write_text(json.dumps(result, indent=1) + "\n")
    lines = ["| flow | side | transactions | accounts created | rent paid (SOL) | accounts left | SOL locked after |",
             "|---|---|---|---|---|---|---|"]
    for entry in result:
        for side in spec["sides"]:
            if side in entry:
                m = entry[side]
                lines.append("| %s | %s | %d | %d | %s | %d | %s |" % (entry["flow"], side, len(m["transactions"]),
                    m["created"], sol(m["rent"]), m["locked_accounts"], sol(m["locked_after"])))
    lines.append("")
    for entry in result:
        lines.append("## " + entry["flow"])
        if entry.get("note"):
            lines += ["", entry["note"]]
        for side in spec["sides"]:
            if side not in entry:
                continue
            m = entry[side]
            lines.append("")
            lines.append("%s (%s): %s" % (side, m["trace"], "; ".join("`%s`" % n for n in m["transactions"])))
            for a in m["accounts"]:
                lines.append("- %s: rent %s SOL, %s (created by `%s`)" % (a["type"], sol(a["rent"]),
                    "closed, 0 locked" if a["closed"] else "still holds %s SOL" % sol(a["locked_after"]), a["created_by"]))
        lines.append("")
    Path(options.out + ".md").write_text("\n".join(lines) + "\n")
    print("\n".join(lines[:2 + 2 * len(result)]))


if __name__ == "__main__":
    sys.exit(main())
