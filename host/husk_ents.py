"""Write `.hkpsx/husk/husk_ents.json`, the input of `hk-cook husk-measure`: the Crossroads_10_boss husks (Zombie Barger,
Runner, Hornhead) with their sprite libraries, scales and the clips their controllers admit. Run from the repo root:
`.venv/bin/python host/husk_ents.py`."""
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from source import Source
from scene import Scene
import actors
import runner
from actors import _component_records


def main():
    src = Source()
    boss = Scene(src, 'level48')
    ents = []
    for i, a in enumerate(a for a in actors.actor_sources(boss) if a['name'].startswith('Zombie')):
        c = runner.recognize(boss, a)
        g = [g for g, go in boss.gos.items() if go['m_Name'] == a['name']][0]
        sprite = [t for _, k, t in _component_records(boss, g) if k == 'tk2dSprite'][0]
        m = boss.world(boss.go_transform[g])
        ents.append(dict(ent=i, name=a['name'], library=c['library_source'],
                         sprite_scale=[sprite['_scale']['x'], sprite['_scale']['y']], world_scale=[m[0][0], m[1][1]],
                         alpha=1.0, clips=[dict(name=x['name'], id=n) for n, x in enumerate(c['clips'])]))
    out = Path('.hkpsx/husk/husk_ents.json')
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(dict(ents=ents), indent=1))
    print(f'{len(ents)} husks written to {out}')


if __name__ == '__main__':
    main()
