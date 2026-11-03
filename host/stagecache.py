"""Whole-stage receipts: run a host script under the dependency tracker and judge it later.

The region cook is a stage made of units (host/region_units.py) and a main process that runs the
passes over their results (the actor bank, the links, similarity, the world tables). The build
driver needs one answer for the stage: is everything it produced still what a fresh run would
produce? This module gives it.

    stagecache.py run STAGE SCRIPT [ARGS...]   run SCRIPT as __main__ under the tracker; on success write
                                               .hkpsx/stagecache/STAGE.json
    stagecache.py status STAGE                 exit 0 and print the file's digest when the record is fresh,
                                               else exit 1 and print why
    stagecache.py forget STAGE                 delete the record (a stage that starts rewriting its outputs
                                               must not leave a record that still vouches for them)

The record names two kinds of dependency record (host/depcache.py): the main process's own, and the
records of the units whose results it used (a script says which with `depcache.note_units`). It is
fresh when every one of them still describes the code and files on disk. It says nothing about the
stage's outputs: the build driver checks those by hash itself.
"""
from __future__ import annotations

import json
import os
import sys
from pathlib import Path

import depcache

ROOT = depcache.ROOT
STAGE_DIR = ROOT / '.hkpsx/stagecache'
SCHEMA = 1


def chain_path(stage: str) -> Path:
    return STAGE_DIR / f'{stage}.json'


def forget(stage: str):
    chain_path(stage).unlink(missing_ok=True)


def run(stage: str, script: str, args: list[str]) -> int:
    import runpy
    retail = depcache.Retail.checked()
    forget(stage)
    path = Path(script)
    if not path.is_absolute():
        path = ROOT / path
    sys.argv = [str(path)] + args
    if retail is None:
        # Nothing the build driver vouched for: run, record nothing.
        runpy.run_path(str(path), run_name='__main__')
        return 0
    rel = os.path.relpath(path, ROOT)
    with depcache.Tracker(retail=retail, extra_skeletons=[rel, 'host/stagecache.py']) as tracker:
        runpy.run_path(str(path), run_name='__main__')
    main = depcache.save(tracker.record())
    write(stage, main, sorted(depcache.STAGE_UNITS))
    return 0


def write(stage: str, main: str, units: list[str]):
    STAGE_DIR.mkdir(parents=True, exist_ok=True)
    temp = chain_path(stage).with_suffix(f'.{os.getpid()}.tmp')
    temp.write_text(json.dumps({'schema': SCHEMA, 'stage': stage, 'main': main, 'units': units}, sort_keys=True))
    temp.replace(chain_path(stage))


def status(stage: str) -> tuple[bool, list[str], str]:
    """(fresh, reasons, digest of the record file)."""
    path = chain_path(stage)
    if depcache.force_cook():
        return False, ['full recook requested'], ''
    retail = depcache.Retail.checked()
    if retail is None:
        return False, ['retail source not checked by the build driver'], ''
    try:
        raw = path.read_bytes()
        record = json.loads(raw)
    except (OSError, ValueError):
        return False, [f'no stage record {path.relative_to(ROOT)}'], ''
    if record.get('schema') != SCHEMA:
        return False, ['stage record schema changed'], ''
    reasons: list[str] = []
    for name in [record['main']] + list(record['units']):
        reasons += depcache.stale_by_name(name, retail)
        if len(reasons) >= 8:
            break
    import hashlib
    return not reasons, reasons, hashlib.sha256(raw).hexdigest()


def main(argv: list[str]) -> int:
    if len(argv) >= 3 and argv[0] == 'run':
        return run(argv[1], argv[2], argv[3:])
    if len(argv) == 2 and argv[0] == 'status':
        fresh, reasons, digest = status(argv[1])
        if fresh:
            print(digest)
            return 0
        print('stale: ' + '; '.join(reasons[:6]))
        return 1
    if len(argv) == 2 and argv[0] == 'forget':
        forget(argv[1])
        return 0
    print(__doc__)
    return 2


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
