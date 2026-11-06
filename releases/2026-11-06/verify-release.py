#!/usr/bin/env python3
"""Check the dated evidence files' hashes and recorded recovery outcomes, offline."""
import hashlib
import json
import lzma
from pathlib import Path

root = Path(__file__).resolve().parent
manifest = json.loads((root / 'manifest.json').read_text())
for name, expected in manifest['files'].items():
    path = (root / name).resolve()
    assert path.is_relative_to(root), f'Invalid manifest path: {name}'
    data = path.read_bytes()
    assert len(data) == expected['bytes'], f'Size mismatch: {name}'
    assert hashlib.sha256(data).hexdigest() == expected['sha256'], f'Hash mismatch: {name}'

trace = json.loads(lzma.decompress((root / 'recovery/recovery-trace.json.xz').read_bytes()))
receipt = json.loads((root / 'recovery/recovery-receipt.json').read_text())
scenarios = json.loads((root / 'recovery/recovery-scenarios.json').read_text())
assert trace['status'] == receipt['status'] == scenarios['status'] == 'passed'
assert trace['public_transactions_sent'] == 0
assert len(trace['transactions']) == receipt['transaction_count'] == 168
assert len(trace['assertions']) == receipt['assertions'] == 674
assert len(trace['receipts']) == receipt['receipts'] == 22
assert all(x['passed'] for x in trace['transactions'])
assert all(x['passed'] for x in trace['assertions'])
assert len(scenarios['recoveries']) == 8
assert scenarios['scenarios']['before_work']['no_old_signature'] is True
assert scenarios['scenarios']['before_work']['request_survived_hostile_rotation'] is True
accepted = scenarios['scenarios']['accepted_work']
assert accepted['cap'] == 2000 and accepted['bill'] == 900
assert accepted['unused_release'] == 1100
assert sum(accepted['partial_collections']) == 900
assert sum(accepted['receiving_withdrawals']) == 900
merge = json.loads((root / 'recovery/account82-84-85-merge-receipt.json').read_text())
assert merge['verifiedMain'] == '133c2286e3c1b52aea952a1a4ddc79acdb9f59ae'
assert merge['treeEqualsReviewed1489470'] is True
print(json.dumps({
    'status': 'passed', 'release': '2026-11-06',
    'files_hashed': len(manifest['files']),
    'retained_native_transactions': len(trace['transactions']),
    'retained_native_assertions': len(trace['assertions']),
    'recorded_owner_recoveries': len(scenarios['recoveries']),
    'recorded_collection_and_withdrawals_atomic': 900,
    'scope': 'File hashes and recorded recovery outcomes.',
    'api_signature_check': 'Run node releases/2026-11-06/continuing-api/verify-records.mjs separately.'
}, indent=2))
