"""Table-driven recognizer for enemies that run their original PlayMaker trees on the compact actor tier.

A family is a row of host/enemy_families.json: the name prefixes and components that select it, the FSMs it
carries (by name, with the host/false_knight.py digest of each, so retail data that drifts fails the cook),
which of them the port drops as inert plumbing (and why), the child object whose trigger box the tree
listens on, and the sprite clips the guest binds. The guest side is `hk_sim::fsm_actor`; the banks and the
geometry tables come from host/hk-fsm `enemy_family.rs`. Rust twin of this file: host/hk-cook/src/enemy_family.rs
(the parity gate compares the control records of the two).
"""
import json
import math
from pathlib import Path

from false_knight import fsm_digest

MANIFEST = json.loads(Path(__file__).with_name('enemy_families.json').read_text())['families']
BASIS_TOLERANCE = 1e-5


def _near(a, b, tolerance=1e-6):
    return abs(float(a) - float(b)) <= tolerance


def row_for(name):
    """The manifest row whose name prefix starts `name`, or None."""
    return next((row for row in MANIFEST if any(name.startswith(p) for p in row['name_prefixes'])), None)


def _children(sc, gid):
    result = {}
    for child in sc.transforms[sc.go_transform[gid]]['m_Children']:
        tid = child['m_PathID']
        kid = sc.transforms[tid]['m_GameObject']['m_PathID']
        result[sc.gos[kid]['m_Name']] = (kid, tid)
    return result


def _quarter(sc, gid):
    """The quarter turn of the actor's world basis: a proper rotation of unit scale, or a refusal."""
    matrix = sc.world(sc.go_transform[gid])
    quarter = round(math.degrees(math.atan2(matrix[1][0], matrix[0][0])) / 90) % 4
    cos, sin = [(1, 0), (0, 1), (-1, 0), (0, -1)][quarter]
    if any(abs(matrix[i][j] - value) > BASIS_TOLERANCE
           for i, row in enumerate([[cos, -sin], [sin, cos]]) for j, value in enumerate(row)):
        raise ValueError('unsupported enemy family rotation or scale')
    return quarter


def recognize(sc, actor):
    """The control record of a placement of a manifest family, or a ValueError naming what differs."""
    from actors import _component_records
    name = sc.gos[actor['game_object']]['m_Name']
    row = row_for(name)
    if row is None:
        raise ValueError('no enemy family row names ' + name)
    gid = actor['game_object']
    records = _component_records(sc, gid)
    kinds = {kind for _, kind, _ in records}
    if any(component not in kinds for component in row['components']):
        raise ValueError(f'{row["name"]}: a required component is missing')
    wanted = {(f['on'], f['name']): f for f in row['fsms']}
    found = {}
    for _, kind, data in records:
        if kind == 'PlayMakerFSM':
            found[('', data['fsm']['name'])] = data
    children = _children(sc, gid)
    for (on, fsm_name) in wanted:
        if on and on not in children:
            raise ValueError(f'{row["name"]}: missing child {on}')
    for on, (kid, _) in children.items():
        for _, kind, data in _component_records(sc, kid):
            if kind == 'PlayMakerFSM':
                found[(on, data['fsm']['name'])] = data
    if set(found) != set(wanted):
        raise ValueError(f'{row["name"]}: unexpected PlayMaker FSM set {sorted(found)}')
    for key, data in found.items():
        if not data['m_Enabled'] or fsm_digest(data['fsm']) != wanted[key]['digest']:
            raise ValueError(f'{row["name"]}: FSM {key[1]} differs from the pinned original')
    trigger = row['trigger_child']
    kid, tid = children[trigger]
    local = sc.transforms[tid]
    if not sc.active(kid) or local['m_LocalPosition'] != {'x': 0., 'y': 0., 'z': 0.} \
            or local['m_LocalScale'] != {'x': 1., 'y': 1., 'z': 1.} \
            or local['m_LocalRotation'] != {'x': 0., 'y': 0., 'z': 0., 'w': 1.}:
        raise ValueError(f'{row["name"]}: the {trigger} child is not seated on the actor')
    boxes = [d for _, k, d in _component_records(sc, kid) if k == 'BoxCollider2D']
    if len(boxes) != 1 or not boxes[0]['m_Enabled'] or not boxes[0]['m_IsTrigger']:
        raise ValueError(f'{row["name"]}: unsupported {trigger} trigger')
    box = boxes[0]
    offset, size = box['m_Offset'], box['m_Size']
    # The trigger's box in the actor's own frame (the guest turns it with the placement), Q16.
    trigger_q16 = [round((offset['x'] - size['x'] / 2) * 65536), round((offset['y'] - size['y'] / 2) * 65536),
                   round((offset['x'] + size['x'] / 2) * 65536), round((offset['y'] + size['y'] / 2) * 65536)]
    quarter = _quarter(sc, gid)
    animator = actor.get('tk2dSpriteAnimator')
    sprite = actor.get('tk2dSprite')
    if animator is None or sprite is None or sprite['_scale'] != {'x': 1., 'y': 1., 'z': 1.}:
        raise ValueError(f'{row["name"]}: unsupported sprite scale')
    library = sc.source.read(sc.source.ref(sc.file, animator['library']))
    have = {c['name'] for c in library['clips'] if c['name']}
    if any(clip not in have for clip in row['clips']):
        raise ValueError(f'{row["name"]}: animation library lacks a clip')
    # The prefab's sprite is the last frame of the rest clip, which the tree never plays at rest: the guest shows that frame
    # until the first trigger.
    rest = next(c for c in library['clips'] if c['name'] == row['rest'])
    if rest['frames'][-1]['spriteId'] != sprite['_spriteId']:
        raise ValueError(f'{row["name"]}: the authored sprite is not the last frame of {row["rest"]}')
    bindings = {'walk': row['rest'], 'turn': row['rest']}
    bindings.update({f'f{i}': clip for i, clip in enumerate(row['clips'])})
    return {'kind': 'FsmActor', 'guest_enabled': True, 'family': row['name'], 'family_id': row['id'],
            'rotation_q16': quarter * 90 * 65536, 'trigger_q16': trigger_q16, 'no_corpse': True, 'art_axes': 'basis',
            'art_bindings': bindings, 'limitations': list(row['limitations'])}
