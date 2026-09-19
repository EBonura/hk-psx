"""What the admitted scenes actually ask of the physics runtime (P11 step 1).

The plan wants collision layers, terrain shape families, one-way behaviour and
gameplay triggers catalogued from the installed source rather than guessed, and
the answer decides how much of steps 2 to 4 there is to write. This walks every
admitted scene and fails loudly if a family appears that the guest's
segment-based collision cannot represent, so the catalog cannot silently go
stale as more of the map is admitted.

No retail payload is embedded; the report goes to .hkpsx.
"""
import sys
from pathlib import Path as _Path
sys.path.insert(0, str(_Path(__file__).resolve().parents[1] / 'host'))
import collections, json
from pathlib import Path
from source import ROOT, dump

# Unity layers this port cares about, named from the source's own usage.
TERRAIN = 8
SHAPES = ('BoxCollider2D', 'PolygonCollider2D', 'EdgeCollider2D', 'CircleCollider2D')
# Shapes the guest's cooked edge model represents exactly. A circle has no
# segment form, so one on Terrain would be an approximation and is refused.
REPRESENTABLE = {'BoxCollider2D', 'PolygonCollider2D', 'EdgeCollider2D'}
# Components that move, bounce or otherwise change the ground under the Knight.
MOVERS = ('LiftPlatform', 'ConveyorBelt', 'ConveyorMovementHero', 'PlatformStickAgent',
          'MovingPlatform', 'PlatformMoving', 'Crumbler', 'BounceShroom', 'BigBouncer')
# Contact damage, hazard recovery and the nail's tink surfaces.
INTERACTIONS = ('DamageHero', 'DamageEnemies', 'HazardRespawnTrigger', 'HazardRespawnMarker',
                'TinkEffect', 'Breakable', 'AcidCorpseSplash')

def survey(source, scene_files):
    terrain = collections.Counter()
    triggers = collections.Counter()
    solids = collections.Counter()
    movers = collections.Counter()
    mover_scenes = collections.defaultdict(collections.Counter)
    interactions = collections.Counter()
    effectors = collections.Counter()
    layers = collections.Counter()
    unrepresentable = []
    for name, scene in scene_files.items():
        file = source.file(name)
        for obj in file.objects.values():
            kind = obj.type.name
            if kind == 'PlatformEffector2D':
                effectors['PlatformEffector2D'] += 1
                continue
            if kind == 'GameObject':
                try:
                    layers[source.read(obj)['m_Layer']] += 1
                except Exception:
                    pass
                continue
            if kind in SHAPES:
                try:
                    tree = source.read(obj)
                except Exception:
                    continue
                if tree.get('m_UsedByEffector'):
                    effectors['usedByEffector'] += 1
                if tree.get('m_IsTrigger'):
                    triggers[kind] += 1
                    continue
                solids[kind] += 1
                try:
                    owner = source.read(source.ref(file, tree['m_GameObject']))
                except Exception:
                    continue
                if owner['m_Layer'] != TERRAIN:
                    continue
                terrain[kind] += 1
                if kind not in REPRESENTABLE:
                    unrepresentable.append({'scene': scene, 'name': owner['m_Name'], 'shape': kind})
                continue
            try:
                typename = source.typename(obj)
            except Exception:
                continue
            if typename in MOVERS:
                movers[typename] += 1
                mover_scenes[typename][scene] += 1
            if typename in INTERACTIONS:
                interactions[typename] += 1
    return {
        'scene_count': len(scene_files),
        'terrain_shapes': dict(terrain),
        'solid_shapes': dict(solids),
        'trigger_shapes': dict(triggers),
        'movers': dict(movers),
        'mover_scenes': {k: dict(v) for k, v in mover_scenes.items()},
        'interactions': dict(interactions),
        'one_way': dict(effectors),
        'game_object_layers': dict(sorted(layers.items())),
        'unrepresentable_terrain': unrepresentable,
    }

def tink_coverage(source, scene_files):
    """How many TinkEffect surfaces the static pogo pool already covers, and why
    the rest do not qualify. `pogo_sources` owns that policy; this only measures
    it, so tightening or widening the policy stays one decision in one place."""
    from scene import Scene
    from actors import pogo_sources, _colliders
    counts = collections.Counter()
    for name in scene_files:
        sc = Scene(source, name)
        tink = {t['m_GameObject']['m_PathID'] for typ, t in sc.objects.values() if typ == 'TinkEffect'}
        if not tink:
            continue
        targets = {int(t['game_object'].split(':')[1]) for t in pogo_sources(sc)['targets']}
        counts['total'] += len(tink)
        counts['covered'] += len(tink & targets)
        by_go = {}
        for sid, (typ, tree) in sc.objects.items():
            gid = tree.get('m_GameObject', {}).get('m_PathID')
            if gid:
                by_go.setdefault(gid, []).append((sid, typ, tree))
        for gid in tink - targets:
            records = by_go.get(gid, [])
            types = {t for _, t, _ in records}
            if sc.gos[gid]['m_Layer'] not in (11, 17, 19):
                counts['excluded: on the Terrain layer, outside the pogo layer policy'] += 1
            elif types & {'BigBouncer', 'BounceShroom', 'PlayMakerFSM'}:
                counts['excluded: an FSM owns the object, so it may move or vanish'] += 1
            elif not _colliders(sc, gid, records):
                counts['excluded: the collider is on a child, not the tink owner'] += 1
            else:
                counts['excluded: other'] += 1
    return dict(counts)

def report(source, scene_files):
    out = survey(source, scene_files)
    assert not out['unrepresentable_terrain'], (
        'terrain shape the cooked edge model cannot represent: '
        f"{out['unrepresentable_terrain'][:3]}")
    assert not out['one_way'], (
        'a one-way platform appeared; the collision solver has no directional rule yet: '
        f"{out['one_way']}")
    out['findings'] = [
        'Terrain is boxes, polygons and edge colliders only, all of which the cooked '
        'segment model represents exactly. The only solid circles in the admitted '
        'scenes are Baldur bodies on the enemy layer.',
        'No PlatformEffector2D and no collider marked usedByEffector exists in any '
        'admitted scene, so there is no one-way platform behaviour to preserve here.',
        'The only mover is LiftPlatform, which is not a moving platform: its Update '
        'sinks its two parts by 0.75 a second for 0.12 s, to a floor of 0.09 units, '
        'and holds. It cannot carry, block or tunnel a rider, so P11 step 3 has '
        'nothing to implement in the admitted world.',
        'Contact damage and hazard recovery already run through the cooked DamageHero '
        'volumes and HazardRespawnTriggers, which is how acid gets its behaviour too.',
    ]
    # The nail's tink surfaces are the one family that is only partly cooked, so
    # the split is measured rather than described.
    out['tink_coverage'] = tink_coverage(source, scene_files)
    out['owed'] = [
        'BounceShroom: the one bouncy target in the admitted world. Its hero-side '
        'response is SHROOM_BOUNCE_VELOCITY and BOUNCE_SHROOM_TIME on HeroController, '
        'next to the pogo it shares machinery with.',
        'TinkEffect surfaces: only the ones that already qualify as pogo targets '
        'recoil the nail. See tink_coverage for the measured split and why each '
        'excluded group is excluded.',
    ]
    dump(ROOT / '.hkpsx/physics-catalog.json', out)
    return out

if __name__ == '__main__':
    from source import Source
    s = Source()
    scenes = {e['file']: e['scene_name']
              for e in json.load(open(ROOT / '.hkpsx/selected-regions.json'))['scenes']}
    out = report(s, scenes)
    print(f"Physics catalog over {out['scene_count']} scenes: "
          f"terrain {out['terrain_shapes']}, movers {out['movers']}, "
          f"one-way {out['one_way'] or 'none'}")
    for key in ('interactions',):
        print(f'  {key}: {out[key]}')
