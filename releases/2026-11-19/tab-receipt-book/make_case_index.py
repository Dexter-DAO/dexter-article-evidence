#!/usr/bin/env python3
"""Write case-index.json from the two traces this record's results rest on.

usage: make_case_index.py [RECORD_DIR]

One entry per transaction of traces/branch/tab-book-replay-trace.json.xz and
traces/branch/double-charge-trace.json.xz, in trace order. Every field is read
from the trace; nothing is computed except the error name, number, program and
source line, which are taken from the transaction's own program log.
"""
import json
import lzma
import re
import sys
from pathlib import Path

SUITES = ["tab-book-replay", "double-charge"]
PROGRAMS = {"727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz": "claims_core",
            "9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G": "principal_registry",
            "3yceApSVxLYihHjLLrCycV8T4Bg1MYggbxQhdzaqB2Sm": "pool_issuer",
            "EsR1knVgJG8NAYFjeGCYpTuvjf1DtwAU1ne5XBdcotq3": "delivery_rules",
            "swigypWHEksbC64pWKwah1WTeh9JXwx8H1rJHLdbQMB": "swig"}
ANCHOR = re.compile(r"Error Code: (\w+)\. Error Number: (\d+)\. Error Message: (.*)")
THROWN = re.compile(r"AnchorError thrown in (\S+?)\.? Error Code")
FAILED = re.compile(r"Program (\S+) failed")


def entry(suite, index, row):
    result = row["result"]
    logs = (result.get("meta") or result).get("logs") or []
    err = result.get("err")
    name = code = message = source = program_id = None
    for line in logs:
        found = ANCHOR.search(line)
        if found and name is None:
            name, code, message = found.group(1), int(found.group(2)), found.group(3)
            thrown = THROWN.search(line)
            source = thrown.group(1) if thrown else None
        failed = FAILED.search(line)
        if failed and program_id is None:
            program_id = failed.group(1)
    if err is not None and name is None:
        name = err if isinstance(err, str) else None
    if err is None:
        outcome = "accepted"
        kind = None
    else:
        outcome = "refused"
        kind = "program" if program_id else "runtime, before any program runs"
    return {
        "suite": suite,
        "index": index,
        "name": row["name"],
        "expected": row["expected"],
        "outcome": outcome,
        "passed": row["passed"],
        "refusal_kind": kind,
        "error": err,
        "error_name": name,
        "error_code": code,
        "raising_program": PROGRAMS.get(program_id, program_id),
        "raising_program_id": program_id,
        "error_source": source,
        "error_message": message,
        "signature_verified": row.get("signature_verified"),
        "wire_bytes": row.get("wire_bytes"),
        "compute_units_consumed": result.get("compute_units_consumed"),
    }


def main():
    root = Path(sys.argv[1] if len(sys.argv) > 1 else Path(__file__).parent)
    index = []
    for suite in SUITES:
        trace = json.loads(lzma.open(root / "traces" / "branch" / (suite + "-trace.json.xz")).read())
        rows = trace["transactions"]
        index += [entry(suite, i, row) for i, row in enumerate(rows)]
        refused = sum(1 for row in rows if row["result"].get("err") is not None)
        print("%s: %d transactions, %d refused, %d assertions" % (suite, len(rows), refused, len(trace["assertions"])))
    (root / "case-index.json").write_text(json.dumps(index, indent=1) + "\n")


if __name__ == "__main__":
    sys.exit(main())
