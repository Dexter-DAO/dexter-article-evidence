#!/usr/bin/env python3
"""Check retained signed records. Does not send or execute transactions."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import platform
from pathlib import Path
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parent
TESTS = ROOT / 'execution/foundation/onchain/tests'
EVIDENCE = ROOT / 'account/docs/evidence/wallet-spending-20261101'
sys.path.insert(0, str(TESTS))
from verify_wallet_spending import verify as verify_wallet
from verify_financing_corrections import verify_corrections


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    rows = []
    for trace in sorted(EVIDENCE.glob('*-trace.json.xz')):
        if trace.name == 'adapter-direct-trace.json.xz':
            continue  # Supplemental adapter fixture has its own retained receipt.
        checker = verify_wallet if trace.name.startswith('wallet-spending-') else verify_corrections
        original = json.loads(trace.with_name(trace.name.replace('-trace.json.xz', '-offline.json')).read_text())
        try:
            result = checker(trace, EVIDENCE / 'idl')
            assert result == original, ('Fresh verifier result differs from the retained verification', trace.name)
            rows.append({'file': str(trace.relative_to(ROOT)), 'compressed_sha256': hashlib.sha256(trace.read_bytes()).hexdigest(),
                         'matches_retained_result': True, 'result': result})
            print(f'PASS {trace.name}: {result["signed_packets"]} signed packets', file=sys.stderr)
        except Exception as exc:
            rows.append({'file': str(trace.relative_to(ROOT)), 'status':'failed',
                         'error_type':type(exc).__name__, 'error':str(exc)})
            print(f'FAIL {trace.name}: {type(exc).__name__}: {exc}', file=sys.stderr)
    assert len(rows) == 15, 'Incomplete trace set'
    ok = all(row.get('matches_retained_result') for row in rows)
    result = {'schema':'dexter-article-retained-evidence-check/v1', 'status':'passed' if ok else 'failed',
              'checked_at_utc':datetime.now(timezone.utc).isoformat(), 'python':platform.python_version(),
              'platform':platform.platform(), 'scope':'Fresh offline validation of retained signed transactions and account snapshots. No new SVM execution and no RPC submission.',
              'checks':rows,
              'totals':{key:sum(row.get('result',{}).get(key,0) for row in rows) for key in ['signed_packets','verified_financial_receipts','verified_spending_operations','verified_actual_adapter_swaps','refusals_with_rollback','recorded_state_assertions']}}
    data=json.dumps(result,indent=2)+'\n'
    if args.output:
        args.output.parent.mkdir(parents=True,exist_ok=True)
        args.output.write_text(data)
    else:
        print(data,end='')
    return 0 if ok else 1

if __name__=='__main__':
    raise SystemExit(main())
