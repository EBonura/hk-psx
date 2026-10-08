#!/usr/bin/env python3
"""One-time, guarded metadata migration for region-resident authored mask owners.

No asset pack changes. Reject changed source inputs, outputs, and unrelated cook
code. The accepted previous hashes are the verified cook before this migration;
afterward identical code may repeat the idempotent metadata postpass.
"""
import hashlib
import json
import sys
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT/'host'))
from world import postpack_masks, generate

PREVIOUS = {
    'host/regions.py':'345a757d2c57b41984596fff7cb3aca5750739bba3459f9152796fabcf20781e',
    'host/world.py':'79af8196b06787a902d11924d288f92ac55835965ec97055a88aac2945278b95',
}
def sha(path):
    with Path(path).open('rb') as stream:return hashlib.file_digest(stream,'sha256').hexdigest()
def write(path, value):
    temporary = path.with_suffix('.tmp')
    temporary.write_text(json.dumps(value, indent=2));temporary.replace(path)
def main():
    cache_path = ROOT/'.hkpsx/regions-cook-cache.json'
    cache = json.loads(cache_path.read_text())
    provenance = json.loads((ROOT/'.hkpsx/regions-provenance.json').read_text())
    current = json.loads((ROOT/'.hkpsx/doctor.json').read_text())['installs'][0]['data_directory']
    if provenance['source'] != current:raise ValueError('source installation changed')
    for name, record in provenance['inputs'].items():
        if sha(Path(current)/name) != record['sha256']:raise ValueError(f'source input changed: {name}')
    for name, expected in cache['outputs'].items():
        if sha(ROOT/name) != expected:raise ValueError(f'cooked output changed: {name}')
    for name, expected in cache['code'].items():
        actual = sha(ROOT/('host/requirements.lock' if name=='requirements' else name))
        if actual != expected and PREVIOUS.get(name) != expected:
            raise ValueError(f'cook code outside known metadata migration: {name}')
    report_path = ROOT/'data/regions.json'
    report = json.loads(report_path.read_text())
    if not report['complete']:raise ValueError('complete region cook required')
    # Validate every source-to-draw table against its immutable base pack. Final
    # postpacking changes texture IDs only; instance indices remain identical.
    import struct
    for row in report['regions']:
        base = ROOT/f'data/regions/region-{row["chunk_id"]:03}/room.hk'
        if sha(base) != row.get('base_sha256',row['sha256']):
            raise ValueError(f'base pack changed: {base}')
        raw = base.read_bytes()
        if struct.unpack_from('<I',raw,16)[0] != row['draws']:
            raise ValueError('base draw count mismatch')
    postpack_masks(report)
    generate(report)  # Validate complete metadata before publishing JSON/cache.
    write(report_path,report)
    for name in PREVIOUS:cache['code'][name] = sha(ROOT/name)
    cache['outputs']['data/regions.json'] = sha(report_path)
    write(cache_path,cache)
    print(json.dumps(report['mask_binding_policy'],indent=2))
if __name__=='__main__':main()
