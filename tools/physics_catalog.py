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

def pogo_coverage(source, scene_files):
    """What the down slash can bounce off, what the cooker refuses, and why.

    `NailSlash.OnTriggerEnter2D` reads `other.gameObject.layer`, then
    `GetComponent<NonBouncer>`, `<BigBouncer>` and `<BounceShroom>` on that same
    object. It never reads `TinkEffect`, so a tink surface is not a pogo surface
    and the two populations are counted apart here. `TinkEffect` is the spark:
    its own `OnTriggerEnter2D` answers a collider tagged `Nail Attack` with a
    camera shake, a flash and a sound on a 0.25 s throttle, and never touches
    hero velocity.

    `pogo_sources` owns the policy. This reads that function's own refusals
    rather than re-deriving them, so the measurement cannot drift from the
    policy it measures; an earlier version re-derived them and mis-ordered the
    checks, which reported the wrong reason for six objects.
    """
    from scene import Scene
    from actors import pogo_sources
    counts = collections.Counter()
    refused = collections.Counter()
    peak = 0
    for name in scene_files:
        sc = Scene(source, name)
        records = pogo_sources(sc)
        targets = records['targets']
        # Compare the `file:id` strings pogo_sources emits. Scene.sid names the
        # file an object was serialized in, so parsing an int out of it and
        # matching that against a merged path id silently misses every object
        # an additive scene contributed.
        counts['covered_targets'] += len(targets)
        counts['covered_objects'] += len({t['game_object'] for t in targets})
        peak = max(peak, len(targets))
        for entry in records['unsupported']:
            refused[entry.get('reason') or entry['unsupported']] += 1
        tink = {t['m_GameObject']['m_PathID'] for typ, t in sc.objects.values() if typ == 'TinkEffect'}
        counts['tink_effect_owners'] += len(tink)
        counts['tink_effect_owners_that_pogo'] += len(
            {sc.sid(gid) for gid in tink} & {t['game_object'] for t in targets})
    grouped = collections.Counter()
    for reason, count in refused.items():
        grouped[reason.split(': ', 1)[-1] if reason.startswith('special/dynamic') else reason] += count
    # `pogo_sources` raises past its per-scene bound rather than dropping a
    # target, so a refusal on capacity cannot exist; the peak below is what
    # says how much room is left under it.
    return {**counts, 'refused': dict(grouped), 'refused_total': sum(grouped.values()),
            'peak_targets_in_one_scene': peak, 'cooker_bound_per_scene': 128}

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
    # Pogo is the one family that is only partly cooked, so the split is
    # measured rather than described.
    out['pogo_coverage'] = pogo_coverage(source, scene_files)
    out['findings'].append(
        'TinkEffect is not the pogo population. NailSlash.OnTriggerEnter2D reads '
        'the collider layer and then NonBouncer, BigBouncer and BounceShroom on '
        'that object, and never reads TinkEffect; TinkEffect is the spark, sound '
        'and camera shake. Counting pogo coverage against TinkEffect surfaces '
        'measures the overlap of two unrelated sets.')
    out['owed'] = [
        'BounceShroom: the one bouncy target in the admitted world. Its hero-side '
        'response is SHROOM_BOUNCE_VELOCITY and BOUNCE_SHROOM_TIME on HeroController, '
        'next to the pogo it shares machinery with.',
        'Pogo targets an FSM owns. The source bounces off them regardless of the '
        'FSM; the cooker refuses them because it cannot tell a cosmetic FSM from '
        'one that moves or removes the object. See pogo_coverage.refused.',
        'Pogo targets whose only collider is a circle: CircleCollider2D has no '
        'exact segment form, and the cooked contact model is polygon-only.',
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
    pogo = out['pogo_coverage']
    print(f"  pogo: {pogo['covered_targets']} targets cooked from "
          f"{pogo['covered_objects']} objects, {pogo['refused_total']} refused, "
          f"peak {pogo['peak_targets_in_one_scene']} of {pogo['cooker_bound_per_scene']} in one scene")
    for reason, count in sorted(pogo['refused'].items(), key=lambda pair: -pair[1]):
        print(f'    {count:4d} refused: {reason}')
    print(f"    TinkEffect owners: {pogo['tink_effect_owners']}, of which "
          f"{pogo['tink_effect_owners_that_pogo']} are also pogo targets "
          '(unrelated populations; see findings)')
