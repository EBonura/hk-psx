#!/usr/bin/env python3
"""What host/world_links.py would bind or refuse over every scene of the source, cooked or not.

    .venv/bin/python tools/world_links_census.py [SCENE ...]

Prints, per scene with a switch, its switches and their gates by family, then totals: the
switch targets the system opens (collider gates), the ones it leaves out and why, and the
PlayerData observers it could bind. This is the list of what later areas still need.
"""
import sys
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))

import world_links  # noqa: E402
from scene import Scene, _build_settings  # noqa: E402
from source import Source  # noqa: E402


def main():
    source = Source()
    files = _build_settings(source)
    wanted = sys.argv[1:] or sorted(files)
    opened = Counter()
    refused = Counter()
    variants = Counter()
    observed = Counter()
    for name in wanted:
        try:
            sc = Scene(source, files[name])
        except Exception as error:  # a scene the reader cannot open is reported, not guessed at
            print(f'{name}: unreadable ({error})')
            continue
        refusals = []
        for index, (kind, tree) in sorted(sc.objects.items()):
            if kind != 'PlayMakerFSM' or tree['fsm']['name'] != world_links.SWITCH_FSM:
                continue
            gid = tree['m_GameObject']['m_PathID']
            if gid not in sc.gos or not sc.active(gid):
                continue
            try:
                record = world_links.switch_record(sc, index, tree['fsm'], refusals)
            except world_links.Refused as error:
                variants[str(error)] += 1
                continue
            for gate in record['gates']:
                opened[gate['family']] += 1
        for entry in refusals:
            if 'target' in entry:
                refused[entry['reason']] += 1
        for boolean, _ in world_links.observers(sc):
            observed[boolean] += 1
    print('collider gates opened by a nail switch:', dict(opened))
    print('switch targets left out (family):', dict(refused))
    print('switches left out (variant):', dict(variants))
    print('DeactivateIfPlayerdataTrue colliders by bool:', len(observed), 'bools,', sum(observed.values()), 'objects')


if __name__ == '__main__':
    main()
