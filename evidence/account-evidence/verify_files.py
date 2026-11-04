#!/usr/bin/env python3
"""Verify every manifest-listed file without third-party dependencies."""
import hashlib
import json
from pathlib import Path

ROOT=Path(__file__).resolve().parent
manifest=json.loads((ROOT/'manifest.json').read_text())
for row in manifest['files']:
    path=ROOT/row['path']
    assert path.is_file(), ('Missing file',row['path'])
    data=path.read_bytes()
    assert len(data)==row['bytes'], ('Length mismatch',row['path'])
    assert hashlib.sha256(data).hexdigest()==row['sha256'], ('Hash mismatch',row['path'])
print(json.dumps({'status':'passed','files_verified':len(manifest['files'])}))
