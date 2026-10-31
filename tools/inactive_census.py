#!/usr/bin/env python3
"""Every class of inactive solid collider the cook skips, over the whole source.

    .venv/bin/python tools/inactive_census.py [SCENE ...]

`Scene.active` is false for an object a PlayMaker FSM or a component switches on later, so the cook
never bakes its collider. Most are harmless (a corpse, an effect). The ones a persistent bit can enable
(the `Activated Plat` of a vine platform, a gate that closes behind the Knight) are missing terrain.
This prints each class (name family, components, FSM names) with its scene count and an example, and
marks the classes whose object or whose parent carries a PersistentBoolItem or is the `activatedSprite`
of a `VinePlatform`, so the next one is found before a walk finds it.
"""
import re
import sys
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))

from breakables import _components, TERRAIN_LAYER  # noqa: E402
from scene import Scene, _build_settings  # noqa: E402
from source import Source  # noqa: E402

BORING = {'Transform', 'MeshRenderer', 'MeshFilter', 'SpriteRenderer', 'tk2dSprite', 'AudioSource'}


def family(name):
    return re.sub(r'[\s_]*(\(\d+\)|\d+)$', '', name).strip()


def main():
    source = Source()
    files = _build_settings(source)
    classes = defaultdict(lambda: {'scenes': set(), 'objects': 0, 'example': None, 'persistent': 0})
    for name in sys.argv[1:] or sorted(files):
        try:
            sc = Scene(source, files[name])
        except Exception:  # unreadable scenes are the census's blind spot, not its subject
            continue
        activated = set()
        for _, kind, tree in (c for gid in sc.gos for c in _components(sc, gid)):
            if kind == 'VinePlatform':
                activated.add(tree['activatedSprite']['m_PathID'])
        for gid, go in sc.gos.items():
            if go['m_Layer'] != TERRAIN_LAYER or gid not in sc.go_transform or sc.active(gid):
                continue
            parts = list(_components(sc, gid))
            if not any(k.endswith('Collider2D') and t['m_Enabled'] and not t['m_IsTrigger'] for _, k, t in parts):
                continue
            kinds = sorted({k for _, k, _ in parts if k not in BORING and not k.endswith('Collider2D')})
            key = (family(go['m_Name']), tuple(kinds), gid in activated)
            entry = classes[key]
            entry['scenes'].add(name)
            entry['objects'] += 1
            entry['persistent'] += int('PersistentBoolItem' in kinds)
            entry['example'] = entry['example'] or f'{name}:{gid}'
    print('inactive solid layer-8 colliders by class (name family, components, is a vine platform\'s Activated Plat):')
    for key, entry in sorted(classes.items(), key=lambda kv: -kv[1]['objects']):
        print(f"{entry['objects']:4} in {len(entry['scenes']):3} scenes  {key[0]!r:34} {list(key[1])}"
              f"{'  VINE-ACTIVATED' if key[2] else ''}{'  PERSISTENT' if entry['persistent'] else ''}  e.g. {entry['example']}")


if __name__ == '__main__':
    main()
