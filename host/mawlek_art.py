"""Brooding Mawlek's contract and art, cooked into Crossroads_09 as a postpass.

host/mawlek.py admits the placed `Mawlek Body` during the region cook with a
`Dummy Blank` for every generic clip slot. This module does the rest, below
the cook_inputs divider so a change here reuses every cooked region:

- `CONTRACT` is every number shared/hk-sim/src/mawlek.rs runs, and
  `check_contract` reads each one back out of the installed source (the five
  FSMs, the Walker, the HealthManager, the shot prefab and the corpse prefab)
  and refuses the cook if one moved. tests/test_mawlek.py holds `CONTRACT`
  against the Rust constants, so neither side can drift alone.
- `cook_scene_bank` cooks every clip the fight plays on the body, the Dummy,
  the arms and the head, the `Shot Mawlek NoDrip` projectile and its impact,
  the `Spit Effect` and the corpse's roar into the scene's own bank, through
  host/false_knight_art.py's decomposition and residency plan: static parts in
  the scene's spare texture pages first, streamed cells after.
- data/mawlek_art.rs carries that bank's tables plus the geometry the guest
  needs beside the art: where each child hangs off the body, the head target,
  each arm's `Attack Range` and swipe collider, and the corpse's box.

Every part's box is relative to the object it hangs off, so a part is drawn at
its object's origin and mirrored about it: the arms are one art, and `Mawlek
Arm L` is `Mawlek Arm R`'s art with its transform's -1 x scale.
"""
import hashlib
import json
import math
import re
import struct
from pathlib import Path

from PIL import Image

from source import ROOT
from cook import tk_sprite, FOCAL, CAM_Z, MAX_TEXTURE_AXIS, guest_wrap
import false_knight as fk
import false_knight_art as fka
import mawlek

# `hk_sim::mawlek::Clip` order. `Dummy Blank` is the one clip nothing draws:
# the body, the arms and the head play it to vanish, so it is cooked with no
# frames at all.
CLIPS = ('Body Idle', 'Body Walk', 'Idle Turn', 'Dummy Blank', 'Dummy Lurk', 'Dummy Intro Jump',
         'Dummy Intro Land', 'Dummy Roar', 'Roar Cooldown', 'Dummy Shoot Antic', 'Dummy Shoot',
         'Dummy Jump Antic', 'Dummy Jump', 'Dummy Land', 'Arm Idle', 'Arm Swipe Antic', 'Arm Swipe',
         'Arm Swipe Cooldown', 'Head Idle', 'Head Spit')
BLANK = 'Dummy Blank'
# The art past `Clip`, in the guest's order: the projectile's two clips, the
# mouth splash, and the corpse, which plays `Dummy Roar` at its own scale.
EXTRA = ('Shot', 'Shot Impact', 'Spit Effect', 'Corpse')
ART_CLIPS = CLIPS + EXTRA
# Which child plays a clip, so which transform scale its art is cooked at.
PART = {name: ('Body' if name in ('Body Idle', 'Body Walk', 'Idle Turn') else
               'Arm' if name.startswith('Arm') else 'Head' if name.startswith('Head') else 'Dummy')
        for name in CLIPS}
CHILD = {'Body': mawlek.BODY_NAME, 'Dummy': 'Dummy', 'Arm': 'Mawlek Arm R', 'Head': 'Mawlek Head'}
SHOT_NAME = 'Shot Mawlek NoDrip'
CORPSE_NAME = 'Corpse Egg Guardian'
# What the fight shows most goes to the static pages first; the super attacks
# hide the body, the arms and the head, so their Dummy art streams against
# fewer neighbours, and the corpse plays with nothing else on screen.
PRIORITY = ('Body Idle', 'Body Walk', 'Arm Idle', 'Head Idle', 'Head Spit', 'Shot', 'Shot Impact',
            'Spit Effect', 'Idle Turn', 'Arm Swipe Antic', 'Arm Swipe', 'Arm Swipe Cooldown',
            'Dummy Jump Antic', 'Dummy Jump', 'Dummy Land', 'Dummy Intro Land', 'Dummy Shoot Antic',
            'Dummy Shoot', 'Dummy Lurk', 'Dummy Intro Jump', 'Dummy Roar', 'Roar Cooldown', 'Corpse', BLANK)
# Crossroads_09's own scenery is 8 of the 19 pages (host/pack_scenes.py); the
# same one page of margin as the False Knight's bank keeps the scene bank's own
# check the one that refuses.
SCENE_PAGE_LIMIT = 18
# Streamed cells land in the scene arena. Crossroads_09 is 62,244 resident
# bytes against Crossroads_10's 358,928, and the arena is sized by the
# largest, so this is far inside what the arena already reserves; it is kept
# low anyway because every byte is read at the gate into the room.
STREAM_BYTES_LIMIT = 96 * 1024

# Every number hk_sim::mawlek runs, as the source serializes it. Seconds are
# converted with combat.ticks (ceiling at 60 Hz) when compared with the Rust.
CONTRACT = {
    'HEALTH': 300, 'INVULNERABLE_SECONDS': .15, 'CONTACT_DAMAGE': 1,
    'WAKE_SECONDS': .166, 'WAKE_JUMP_SECONDS': .1, 'WAKE_JUMP_SPEED': 53., 'WAKE_JUMP_ANGLE': 90.,
    'GRAVITY': 3., 'LURK_DEPTH': 3.16, 'WAKE_DEPTH_SECONDS': .5,
    'WAKE_ROAR_SECONDS': 2., 'IDLE_SECONDS': (2., 3.),
    'JUMP_SPEED_Y': 68., 'JUMP_SECONDS': .1, 'JUMP_X_FACTOR': 1.25,
    'LAND_SECONDS': .5, 'LAND_2_SECONDS': .25, 'JUMP_COOLDOWN_SECONDS': .25, 'SPIT_COOLDOWN_SECONDS': 1.75,
    'IN_A_ROW': 3, 'REPEAT_ABOVE': 75., 'SPIT_SHOTS': 25, 'SPIT_SPEED': (32., 35.),
    'SPIT_ANGLES_LEFT': (92., 105.), 'SPIT_ANGLES_RIGHT': (75., 88.),
    'HEAD_IDLE_SECONDS': (.3, .6), 'HEAD_ANTIC_SECONDS': .083, 'HEAD_SHOOT_SECONDS': .25,
    'HEAD_SHOT_SPEED': 27., 'HEAD_ANGLES_LEFT': (95., 105.), 'HEAD_ANGLES_RIGHT': (75., 85.),
    'ARM_PAUSE_SECONDS': .15,
    'WALK_SPEED': 3., 'WALK_SECONDS': (1., 3.), 'PAUSE_SECONDS': (1., 3.),
    'CORPSE_SECONDS': (1.5, 3., 1.), 'DEATH_SILENCE_SECONDS': 2.,
    'BLOW_WAIT_SECONDS': 5.5, 'END_WAIT_SECONDS': 5.,
    'CLIPS': {'Body Idle': (3, 10.), 'Body Walk': (3, 12.), 'Idle Turn': (6, 10.), 'Dummy Blank': (1, 30.),
              'Dummy Lurk': (3, 10.), 'Dummy Intro Jump': (9, 12.), 'Dummy Intro Land': (3, 12.),
              'Dummy Roar': (4, 15.), 'Roar Cooldown': (5, 12.), 'Dummy Shoot Antic': (8, 12.),
              'Dummy Shoot': (3, 10.), 'Dummy Jump Antic': (3, 10.), 'Dummy Jump': (7, 10.),
              'Dummy Land': (6, 12.), 'Arm Idle': (5, 12.), 'Arm Swipe Antic': (13, 16.), 'Arm Swipe': (1, 15.),
              'Arm Swipe Cooldown': (3, 15.), 'Head Idle': (3, 12.), 'Head Spit': (3, 12.)},
}
# Structural digests (host/false_knight.py `fsm_digest`) of every state
# machine the fight runs, so a transition or an action the tables above do not
# name still refuses the cook when it changes.
FSM_SHA256 = {
    'Mawlek Control': 'cef5f4a5b9d286e8ed579cf816306c890258bc5322b0622f0a97343cd6a7a082',
    'Mawlek Arm Control': 'ed06cecf199accdf78d97a6d50b61061294146c63cf7578f16aee309fbe2f93b',
    'Mawlek Arm Control L': '9e56a3438f228eda19f93439e52219c62693a213cb3870d2f9c0878436d45bf3',
    'Mawlek Head': 'b8a923a20552965e5159a14c9adc64d5c10818e601fb5f0313e6199c436686e3',
    'nail_clash_tink': '5e71bc12aae4f613ddd3c20e34f5a291cd2ed977ae334dda6e3c1051f9337b9a',
    'Battle Control': '06096697a5390bc878dd7820765f2822e9f059c152ed63e8bd02fedd6b554280',
    'corpse': '4b81de6397e3941ef8fbd33c89995db012f8cec009e3c6b0cd41521ddf1ad367',
}


def _near(a, b):
    return abs(float(a) - float(b)) <= 1e-5


def _require(what, got, want):
    if isinstance(want, (tuple, list)):
        ok = len(got) == len(want) and all(_near(g, w) for g, w in zip(got, want))
    else:
        ok = _near(got, want)
    if not ok:
        raise ValueError(f'Mawlek source moved: {what} is {got!r}, the guest runs {want!r}')


def _states(fsm):
    return {state['name']: state for state in fsm['states']}


def _value(field):
    return fka._scalar(field) if isinstance(field, dict) else field


def _vector3(state, action_index, name):
    """A literal FsmVector3 parameter of one action."""
    data = state['actionData']
    start = data['actionStartIndex'][action_index]
    end = (data['actionStartIndex'][action_index + 1] if action_index + 1 < len(data['actionNames'])
           else len(data['paramName']))
    for i in range(start, end):
        if data['paramName'][i] == name and data['paramDataType'][i] == 28:
            pos = data['paramDataPos'][i]
            raw = bytes(data['byteData'][pos:pos + 13])
            if raw[12]:
                raise ValueError(f'{state["name"]} {name} is a variable')
            return struct.unpack('<3f', raw[:12])
    raise ValueError(f'{state["name"]} lacks a literal {name}')


def _prefab(s, file, path_id):
    """(GameObject tree, {typename: [(object, tree)]}) of a prefab root."""
    go_o = s.file(file).objects[path_id]
    go = go_o.read_typetree()
    parts = {}
    for c in go['m_Component']:
        o = s.ref(go_o.assets_file, c['component'])
        kind = s.typename(o) if o.type.name == 'MonoBehaviour' else o.type.name
        parts.setdefault(kind, []).append((o, s.read(o) if o.type.name == 'MonoBehaviour' else o.read_typetree()))
    return go_o, go, parts


def _fling(state):
    (fields, _), = fka._actions(state, 'FlingObjectsFromGlobalPool')
    return fields


def sources(s, sc):
    """The objects the contract and the art are read from."""
    body = next(gid for gid, go in sc.gos.items() if go['m_Name'] == mawlek.BODY_NAME)
    children = mawlek._children(sc, body)
    fsms = {'Mawlek Control': mawlek._fsms(sc, body)['Mawlek Control']['fsm']}
    arm_r = mawlek._fsms(sc, children['Mawlek Arm R'])
    arm_l = mawlek._fsms(sc, children['Mawlek Arm L'])
    fsms['Mawlek Arm Control'] = arm_r['Mawlek Arm Control']['fsm']
    # The left arm's `Init` lists the same three actions in another order, so
    # each arm is pinned by its own digest rather than held equal to the other.
    fsms['Mawlek Arm Control L'] = arm_l['Mawlek Arm Control']['fsm']
    fsms['nail_clash_tink'] = arm_r['nail_clash_tink']['fsm']
    if fk.fsm_digest(arm_l['nail_clash_tink']['fsm']) != fk.fsm_digest(fsms['nail_clash_tink']):
        raise ValueError('the two Mawlek arms no longer parry the same way')
    fsms['Mawlek Head'] = mawlek._fsms(sc, children['Mawlek Head'])['Mawlek Head']['fsm']
    battle = next(gid for gid, go in sc.gos.items() if go['m_Name'] == 'Battle Scene')
    fsms['Battle Control'] = mawlek._fsms(sc, battle)['Battle Control']['fsm']
    from actors import _component_records
    components = {kind: tree for _, kind, tree in _component_records(sc, body)}
    shoot = _states(fsms['Mawlek Control'])['Shoot']
    shot_ref = _fling(shoot)['gameObject']['value']
    shot_o = s.ref(sc.file, shot_ref)
    death = components['EnemyDeathEffects']
    corpse_o = s.ref(sc.file, death['corpsePrefab'])
    corpse = _prefab(s, corpse_o.assets_file.name, corpse_o.path_id)
    if corpse[1]['m_Name'] != CORPSE_NAME:
        raise ValueError('Mawlek corpse prefab is now ' + corpse[1]['m_Name'])
    corpse_fsm = [t['fsm'] for _, t in corpse[2]['PlayMakerFSM'] if t['fsm']['name'] == 'corpse']
    if len(corpse_fsm) != 1:
        raise ValueError('Mawlek corpse lacks its corpse FSM')
    fsms['corpse'] = corpse_fsm[0]
    shot = _prefab(s, shot_o.assets_file.name, shot_o.path_id)
    if shot[1]['m_Name'] != SHOT_NAME:
        raise ValueError('Mawlek spits ' + shot[1]['m_Name'])
    return {'body': body, 'children': children, 'fsms': fsms, 'components': components,
            'shot': shot, 'corpse': corpse, 'battle': battle}


def check_contract(s, sc, src):
    """Read every number `CONTRACT` names out of the source and refuse a change."""
    from combat import HIT_EVASION_SECONDS, ticks
    del ticks
    c = CONTRACT
    fsms = src['fsms']
    for name, digest in FSM_SHA256.items():
        got = fk.fsm_digest(fsms[name])
        if got != digest:
            raise ValueError(f'Mawlek FSM {name} changed: {got}')
    comp = src['components']
    health = comp['HealthManager']
    _require('hp', health['hp'], c['HEALTH'])
    _require('invulnerableTime', health['invulnerableTime'], c['INVULNERABLE_SECONDS'])
    _require('DamageHero', comp['DamageHero']['damageDealt'], c['CONTACT_DAMAGE'])
    if any(health[k] for k in ('smallGeoDrops', 'mediumGeoDrops', 'largeGeoDrops')):
        raise ValueError('Mawlek now drops Geo')
    walker = comp['Walker']
    _require('walkSpeedR', walker['walkSpeedR'], c['WALK_SPEED'])
    _require('walkSpeedL', -walker['walkSpeedL'], c['WALK_SPEED'])
    _require('pauseWait', (walker['pauseWaitMin'], walker['pauseWaitMax']), c['WALK_SECONDS'])
    _require('pauseTime', (walker['pauseTimeMin'], walker['pauseTimeMax']), c['PAUSE_SECONDS'])
    for flag, want in (('preventScaleChange', 1), ('preventTurningToFaceHero', 1), ('pauses', 1),
                       ('turnAfterIdlePercentage', 0), ('ignoreHoles', 0), ('startInactive', 1), ('ambush', 0),
                       ('waitForHeroX', 0), ('preventTurn', 0)):
        if walker[flag] != want:
            raise ValueError(f'Mawlek Walker {flag} is {walker[flag]}')
    if (walker['idleClip'], walker['walkClip'], walker['turnClip']) != ('Body Idle', 'Body Walk', 'Idle Turn'):
        raise ValueError('Mawlek Walker clips changed')
    st = _states(fsms['Mawlek Control'])
    wait = lambda name: fk._wait_seconds(st[name])
    _require('Wake', wait('Wake'), c['WAKE_SECONDS'])
    _require('Wake Jump', wait('Wake Jump'), c['WAKE_JUMP_SECONDS'])
    angle = fka._one(st['Wake Jump'], 'SetVelocityAsAngle')
    _require('Wake Jump speed', _value(angle['speed']), c['WAKE_JUMP_SPEED'])
    _require('Wake Jump angle', _value(angle['angle']), c['WAKE_JUMP_ANGLE'])
    _require('Wake In Air gravity', _value(fka._one(st['Wake In Air'], 'SetGravity2dScale')['gravityScale']),
             c['GRAVITY'])
    _require('Init gravity', _value(fka._one(st['Init'], 'SetGravity2dScale')['gravityScale']), 0.)
    (tween, tween_index), = fka._actions(st['Wake In Air'], 'iTweenMoveBy')
    _require('iTween time', _value(tween['time']), c['WAKE_DEPTH_SECONDS'])
    if fka._enum(st['Wake In Air'], tween_index, 'easeType') != 21:
        raise ValueError('Wake In Air no longer tweens linearly')
    _require('iTween vector', _vector3(st['Wake In Air'], tween_index, 'vector'), (0., 0., -c['LURK_DEPTH']))
    z = sc.world(sc.go_transform[src['body']])[2][3]
    _require('Mawlek z', z, c['LURK_DEPTH'])
    _require('Wake Roar', wait('Wake Roar'), c['WAKE_ROAR_SECONDS'])
    idle = fka._one(st['Idle'], 'RandomFloat')
    _require('Idle', (_value(idle['min']), _value(idle['max'])), c['IDLE_SECONDS'])
    for name in ('Jump', 'Jump 2'):
        v = fka._one(st[name], 'SetVelocity2d')
        _require(name + ' y', _value(v['y']), c['JUMP_SPEED_Y'])
        _require(name, wait(name), c['JUMP_SECONDS'])
    for name in ('Detect Hero Pos 3', 'Aim Return'):
        (mul, _), _neg = fka._actions(st[name], 'FloatMultiply')
        _require(name + ' factor', _value(mul['multiplyBy']), c['JUMP_X_FACTOR'])
    _require('Land', wait('Land'), c['LAND_SECONDS'])
    _require('Land 2', wait('Land 2'), c['LAND_2_SECONDS'])
    _require('Land 2 cooldown', _value(fka._one(st['Land 2'], 'SetFloatValue')['floatValue']),
             c['JUMP_COOLDOWN_SECONDS'])
    _require('Shoot cooldown', _value(fka._one(st['Shoot'], 'SetFloatValue')['floatValue']),
             c['SPIT_COOLDOWN_SECONDS'])
    for name in ('Super Jump', 'Detect Hero Pos 2'):
        _require(name + ' in a row', _value(fka._one(st[name], 'IntCompare')['integer2']), c['IN_A_ROW'])
    _require('Repeat Check', _value(fka._one(st['Repeat Check'], 'FloatCompare')['float2']), c['REPEAT_ABOVE'])
    spray = _fling(st['Shoot'])
    _require('spit count', (_value(spray['spawnMin']), _value(spray['spawnMax'])),
             (c['SPIT_SHOTS'], c['SPIT_SHOTS']))
    variables = fk._variables(fsms['Mawlek Control'])
    _require('Shot Speed', (variables['Shot Speed'], variables['Shot Speed Max']), c['SPIT_SPEED'])
    for side, key in (('L', 'SPIT_ANGLES_LEFT'), ('R', 'SPIT_ANGLES_RIGHT')):
        values = [_value(f['floatValue']) for f, _ in fka._actions(st[side], 'SetFloatValue')]
        _require('spit ' + side, values, c[key])
    head = _states(fsms['Mawlek Head'])
    hidle = fka._one(head['Idle'], 'RandomFloat')
    _require('Head Idle', (_value(hidle['min']), _value(hidle['max'])), c['HEAD_IDLE_SECONDS'])
    _require('Head Shoot Antic', fk._wait_seconds(head['Shoot Antic']), c['HEAD_ANTIC_SECONDS'])
    _require('Head Shoot', fk._wait_seconds(head['Shoot']), c['HEAD_SHOOT_SECONDS'])
    _require('Head Shot Speed', fk._variables(fsms['Mawlek Head'])['Shot Speed'], c['HEAD_SHOT_SPEED'])
    hspray = _fling(head['Shoot'])
    _require('head count', (_value(hspray['spawnMin']), _value(hspray['spawnMax'])), (1, 1))
    for side, key in (('L', 'HEAD_ANGLES_LEFT'), ('R', 'HEAD_ANGLES_RIGHT')):
        values = [_value(f['floatValue']) for f, _ in fka._actions(head[side], 'SetFloatValue')]
        _require('head ' + side, values, c[key])
    arm = _states(fsms['Mawlek Arm Control'])
    _require('Re attack Pause', fk._wait_seconds(arm['Re attack Pause']), c['ARM_PAUSE_SECONDS'])
    corpse = _states(fsms['corpse'])
    _require('corpse waits', [fk._wait_seconds(corpse[n]) for n in ('Init', 'Steam', 'Ready')], c['CORPSE_SECONDS'])
    (snap, snap_index), = fka._actions(corpse['Music'], 'TransitionToAudioSnapshot')
    _require('corpse silence', _value(snap['transitionTime']), c['DEATH_SILENCE_SECONDS'])
    battle = _states(fsms['Battle Control'])
    _require('Blow Wait', fk._wait_seconds(battle['Blow Wait']), c['BLOW_WAIT_SECONDS'])
    _require('End Wait', fk._wait_seconds(battle['End Wait']), c['END_WAIT_SECONDS'])
    library = s.read(s.ref(sc.file, fka._component(sc, src['body'], 'tk2dSpriteAnimator')['library']))
    by_name = {clip['name']: clip for clip in library['clips'] if clip['name']}
    for name, (frames, fps) in c['CLIPS'].items():
        clip = by_name[name]
        _require(name, (len(clip['frames']), clip['fps']), (frames, fps))
    return {name: fk.fsm_digest(fsm) for name, fsm in fsms.items()}


def _box(tree, matrix):
    """A BoxCollider2D under a world matrix, as a world box."""
    cx = matrix[0][3] + tree['m_Offset']['x'] * matrix[0][0]
    cy = matrix[1][3] + tree['m_Offset']['y'] * matrix[1][1]
    hw, hh = abs(tree['m_Size']['x'] * matrix[0][0]) / 2, abs(tree['m_Size']['y'] * matrix[1][1]) / 2
    return [cx - hw, cy - hh, cx + hw, cy + hh]


def geometry(s, sc, src):
    """Child offsets and collision boxes, relative to the body's transform."""
    body = src['body']
    children = src['children']
    origin = sc.world(sc.go_transform[body])
    ox, oy = origin[0][3], origin[1][3]

    def unrotated(gid):
        m = sc.world(sc.go_transform[gid])
        if abs(m[0][1]) > 1e-6 or abs(m[1][0]) > 1e-6:
            raise ValueError(f'{sc.gos[gid]["m_Name"]} is rotated')
        return m

    def offset(name, turned=False):
        m = sc.world(sc.go_transform[children[name]]) if turned else unrotated(children[name])
        return [m[0][3] - ox, m[1][3] - oy]

    def rel(box):
        return [box[0] - ox, box[1] - oy, box[2] - ox, box[3] - oy]
    head = children['Mawlek Head']
    head_box = rel(_box(fka._component(sc, head, 'BoxCollider2D'), unrotated(head)))
    ranges, hitboxes = [], []
    for name in ('Mawlek Arm R', 'Mawlek Arm L'):
        gid = children[name]
        attack = mawlek._children(sc, gid)['Attack Range']
        ranges.append(rel(_box(fka._component(sc, attack, 'BoxCollider2D'), unrotated(attack))))
        poly = fka._component(sc, gid, 'PolygonCollider2D')
        m = unrotated(gid)
        points = [(m[0][3] + (p['x'] + poly['m_Offset']['x']) * m[0][0], m[1][3] + (p['y'] + poly['m_Offset']['y']) * m[1][1])
                  for path in poly['m_Points']['m_Paths'] for p in path]
        if not points:
            raise ValueError(f'{name} has an empty swipe collider')
        hitboxes.append(rel([min(p[0] for p in points), min(p[1] for p in points),
                             max(p[0] for p in points), max(p[1] for p in points)]))
    _, _, shot_parts = src['shot']
    shot_box = shot_parts['BoxCollider2D'][0][1]
    if not (_near(shot_box['m_Size']['x'], .640625) and _near(shot_box['m_Size']['y'], .5625)):
        raise ValueError('Shot Mawlek NoDrip box differs from the pooled goop box')
    body_rb = shot_parts['Rigidbody2D'][0][1]
    if not _near(body_rb['m_GravityScale'], .6) or shot_parts['DamageHero'][0][1]['damageDealt'] != 1:
        raise ValueError('Shot Mawlek NoDrip body differs from the pooled goop')
    _, _, corpse_parts = src['corpse']
    corpse_box = corpse_parts['BoxCollider2D'][0][1]
    corpse_scale = corpse_parts['Transform'][0][1]['m_LocalScale']
    if not _near(corpse_parts['Rigidbody2D'][0][1]['m_GravityScale'], 1.):
        raise ValueError('Mawlek corpse gravity changed')
    death = src['components']['EnemyDeathEffects']
    return {
        'dummy': offset('Dummy'), 'arms': [offset('Mawlek Arm R'), offset('Mawlek Arm L')],
        'head': offset('Mawlek Head'), 'spit': offset('Spit Effect', turned=True),
        'head_box': head_box, 'arm_ranges': ranges, 'arm_hitboxes': hitboxes,
        'corpse_box': [corpse_box['m_Offset']['x'] * corpse_scale['x'] - corpse_box['m_Size']['x'] * corpse_scale['x'] / 2,
                       corpse_box['m_Offset']['y'] * corpse_scale['y'] - corpse_box['m_Size']['y'] * corpse_scale['y'] / 2,
                       corpse_box['m_Offset']['x'] * corpse_scale['x'] + corpse_box['m_Size']['x'] * corpse_scale['x'] / 2,
                       corpse_box['m_Offset']['y'] * corpse_scale['y'] + corpse_box['m_Size']['y'] * corpse_scale['y'] / 2],
        'corpse_fling': death['corpseFlingSpeed'],
    }


def source_art(s, sc, src):
    """Every sprite the bank needs, keyed as host/false_knight_art.py keys them.

    Returns (sprites, clips): sprites maps a key to (image, box, w, h), the box
    relative to the object the sprite hangs off; clips maps an art clip name to
    its tk2d record and sprite keys.
    """
    textures, collections, sprites, clips = {}, {}, {}, {}
    project = FOCAL / -CAM_Z

    def add(lib_o, clip, sx, sy, name, turn=0, tint=None):
        # A quarter turn is cooked into the art: the box and the image turn
        # together, scaled first because the transform scales before it turns.
        if turn not in (0, 90, -90):
            raise ValueError(f'Mawlek {name} is turned {turn} degrees')
        keys = []
        for frame in clip['frames']:
            collection_o = s.ref(lib_o.assets_file, frame['spriteCollection'])
            sid = s.sid(collection_o)
            if sid not in collections:
                collections[sid] = s.read(collection_o)
            key = (sid, frame['spriteId'], round(sx, 6), round(sy, 6), turn, repr(tint))
            if key not in sprites:
                image, box = tk_sprite(s, collection_o.assets_file, collections[sid], frame['spriteId'], textures)
                box = (box[0] * sx, box[1] * sy, box[2] * sx, box[3] * sy)
                if tint is not None:
                    # tk2dSprite `_color` multiplies the texel colour.
                    bands = image.convert('RGBA').split()
                    image = Image.merge('RGBA', [b.point(lambda v, k=k: round(v * k)) for b, k in
                                                       zip(bands[:3], (tint['r'], tint['g'], tint['b']))] + [bands[3]])
                if turn == -90:
                    image, box = image.rotate(-90, expand=True), (box[1], -box[2], box[3], -box[0])
                elif turn == 90:
                    image, box = image.rotate(90, expand=True), (-box[3], box[0], -box[1], box[2])
                w = math.ceil((box[2] - box[0]) * project)
                h = math.ceil((box[3] - box[1]) * project)
                if w > MAX_TEXTURE_AXIS or h > MAX_TEXTURE_AXIS:
                    raise ValueError(f'Mawlek {name} frame {w}x{h} exceeds the texture axis')
                sprites[key] = (image, box, max(1, w), max(1, h))
            keys.append(key)
        clips[name] = {'record': clip, 'keys': keys}

    def part_scale(gid):
        m = sc.world(sc.go_transform[gid])
        tk = fka._component(sc, gid, 'tk2dSprite')
        if tk['_color'] != {'r': 1.0, 'g': 1.0, 'b': 1.0, 'a': 1.0}:
            raise ValueError(f'{sc.gos[gid]["m_Name"]} is tinted')
        return abs(m[0][0] * tk['_scale']['x']), abs(m[1][1] * tk['_scale']['y'])

    for name in CLIPS:
        gid = src['body'] if PART[name] == 'Body' else src['children'][CHILD[PART[name]]]
        lib_o = s.ref(sc.file, fka._component(sc, gid, 'tk2dSpriteAnimator')['library'])
        clip = next(c for c in s.read(lib_o)['clips'] if c['name'] == name)
        if name == BLANK:
            clips[name] = {'record': clip, 'keys': []}
            continue
        add(lib_o, clip, *part_scale(gid), name)
    # The projectile, at its prefab scale times EnemyBullet's scaleMin, the
    # size the Blocker's `Shot Mawlek` is cooked at too.
    _, _, parts = src['shot']
    animator = parts['tk2dSpriteAnimator'][0][1]
    lib_o = s.ref(parts['tk2dSpriteAnimator'][0][0].assets_file, animator['library'])
    library = s.read(lib_o)
    scale = parts['Transform'][0][1]['m_LocalScale']['x'] * parts['EnemyBullet'][0][1]['scaleMin']
    tk = parts['tk2dSprite'][0][1]
    for name, clip_name in (('Shot', 'Idle'), ('Shot Impact', 'Impact')):
        clip = next(c for c in library['clips'] if c['name'] == clip_name)
        add(lib_o, clip, scale * abs(tk['_scale']['x']), scale * abs(tk['_scale']['y']), name)
    # `Spit Effect` plays `Enemy Shot` once, turned the way its transform is.
    spit = src['children']['Spit Effect']
    m = sc.world(sc.go_transform[spit])
    spit_tk = fka._component(sc, spit, 'tk2dSprite')
    spit_lib = s.ref(sc.file, fka._component(sc, spit, 'tk2dSpriteAnimator')['library'])
    spit_clip = next(c for c in s.read(spit_lib)['clips'] if c['name'] == 'Enemy Shot')
    sx = math.hypot(m[0][0], m[1][0]) * abs(spit_tk['_scale']['x'])
    sy = math.hypot(m[0][1], m[1][1]) * abs(spit_tk['_scale']['y'])
    turn = round(math.degrees(math.atan2(m[1][0], m[0][0])))
    if spit_tk['_color']['a'] != 1.0:
        raise ValueError('Spit Effect is translucent')
    add(spit_lib, spit_clip, sx, sy, 'Spit Effect', turn, spit_tk['_color'])
    # The corpse's animator plays `Dummy Roar` from the Mawlek's library at
    # the corpse's own scale.
    go_o, go, cparts = src['corpse']
    canim = cparts['tk2dSpriteAnimator'][0][1]
    clib_o = s.ref(cparts['tk2dSpriteAnimator'][0][0].assets_file, canim['library'])
    cclip = s.read(clib_o)['clips'][canim['defaultClipId']]
    if cclip['name'] != 'Dummy Roar' or not canim['playAutomatically']:
        raise ValueError('the Mawlek corpse no longer roars')
    ctk = cparts['tk2dSprite'][0][1]
    cscale = cparts['Transform'][0][1]['m_LocalScale']
    add(clib_o, cclip, abs(cscale['x'] * ctk['_scale']['x']), abs(cscale['y'] * ctk['_scale']['y']), 'Corpse')
    return sprites, clips, {'spit_turn_degrees': turn}


def rust_constants(path=ROOT / 'shared/hk-sim/src/mawlek.rs'):
    """Every `pub const NAME: type = value;` in the Rust module, as ints or lists."""
    found = {}
    for match in re.finditer(r'pub const (\w+)\s*:\s*[^=]+=\s*([^;]+);', path.read_text()):
        raw = re.sub(r'//.*', '', match.group(2)).strip()
        try:
            found[match.group(1)] = eval(raw.replace('ONE', '65536'), {'__builtins__': {}}, {})
        except Exception:
            continue
    return found


def expected_rust():
    """`CONTRACT` in the guest's units, keyed by the Rust constant names."""
    from combat import HIT_EVASION_SECONDS, ticks
    c = CONTRACT
    q = lambda v: round(v * 65536)
    t = lambda pair: [ticks(v) for v in pair]
    clip_ticks = [ticks(c['CLIPS'][name][0] / c['CLIPS'][name][1]) for name in CLIPS]
    return {
        'HEALTH': c['HEALTH'], 'INVULNERABLE_TICKS': ticks(HIT_EVASION_SECONDS),
        'CONTACT_DAMAGE': c['CONTACT_DAMAGE'], 'WAKE_TICKS': ticks(c['WAKE_SECONDS']),
        'WAKE_JUMP_TICKS': ticks(c['WAKE_JUMP_SECONDS']), 'WAKE_JUMP_SPEED': q(c['WAKE_JUMP_SPEED']),
        'GRAVITY': q(c['GRAVITY']), 'LURK_DEPTH': q(c['LURK_DEPTH']),
        'WAKE_DEPTH_TICKS': ticks(c['WAKE_DEPTH_SECONDS']), 'WAKE_ROAR_TICKS': ticks(c['WAKE_ROAR_SECONDS']),
        'IDLE_TICKS': t(c['IDLE_SECONDS']), 'JUMP_SPEED_Y': q(c['JUMP_SPEED_Y']), 'JUMP_TICKS': ticks(c['JUMP_SECONDS']),
        'JUMP_X_FACTOR': q(c['JUMP_X_FACTOR']), 'LAND_TICKS': ticks(c['LAND_SECONDS']),
        'LAND_2_TICKS': ticks(c['LAND_2_SECONDS']), 'JUMP_COOLDOWN_TICKS': ticks(c['JUMP_COOLDOWN_SECONDS']),
        'SPIT_COOLDOWN_TICKS': ticks(c['SPIT_COOLDOWN_SECONDS']), 'IN_A_ROW': c['IN_A_ROW'],
        'REPEAT_ABOVE': round(c['REPEAT_ABOVE']), 'SPIT_SHOTS': c['SPIT_SHOTS'],
        'SPIT_SPEED': [q(v) for v in c['SPIT_SPEED']],
        'SPIT_ANGLES_LEFT': [round(v) for v in c['SPIT_ANGLES_LEFT']],
        'SPIT_ANGLES_RIGHT': [round(v) for v in c['SPIT_ANGLES_RIGHT']],
        'HEAD_IDLE_TICKS': t(c['HEAD_IDLE_SECONDS']), 'HEAD_ANTIC_TICKS': ticks(c['HEAD_ANTIC_SECONDS']),
        'HEAD_SHOOT_TICKS': ticks(c['HEAD_SHOOT_SECONDS']), 'HEAD_SHOT_SPEED': q(c['HEAD_SHOT_SPEED']),
        'HEAD_ANGLES_LEFT': [round(v) for v in c['HEAD_ANGLES_LEFT']],
        'HEAD_ANGLES_RIGHT': [round(v) for v in c['HEAD_ANGLES_RIGHT']],
        'ARM_PAUSE_TICKS': ticks(c['ARM_PAUSE_SECONDS']), 'WALK_SPEED': q(c['WALK_SPEED']),
        'WALK_TICKS': t(c['WALK_SECONDS']), 'PAUSE_TICKS': t(c['PAUSE_SECONDS']),
        'CORPSE_TICKS': t(c['CORPSE_SECONDS']), 'DEATH_SILENCE_TICKS': ticks(c['DEATH_SILENCE_SECONDS']),
        'ARENA_END_TICKS': ticks(c['BLOW_WAIT_SECONDS'] + c['END_WAIT_SECONDS']),
        'HEART_PIECE_TICKS': ticks(c['BLOW_WAIT_SECONDS']), 'CLIP_TICKS': clip_ticks,
    }


def check_rust():
    rust = rust_constants()
    for name, want in expected_rust().items():
        got = rust.get(name)
        if isinstance(want, list):
            got = list(got) if got is not None else None
        if got != want:
            raise ValueError(f'shared/hk-sim/src/mawlek.rs {name} is {got}, the source says {want}')


def neutral_actor(actor):
    """Nothing to neutralize: host/mawlek.py already binds `Dummy Blank`."""
    return actor


def rust_table(anchor_clip, sprite_rows, clip_rows, sequence, geo, scene_id):
    q = lambda v: round(v * 65536)
    pair = lambda v: f'[{q(v[0])}, {q(v[1])}]'
    box = lambda v: '[' + ', '.join(str(q(x)) for x in v) + ']'
    lines = ['// Generated by host/mawlek_art.py from the installed source; do not edit.',
             '// Brooding Mawlek\'s parts and geometry: see docs/MAWLEK.md.',
             f'pub const MW_SCENE: usize = {scene_id};',
             f'pub const MW_ART_ANCHOR_CLIP: u16 = {anchor_clip};',
             '/// Per sprite: first part (frames after the anchor clip\'s first), parts, streamed.',
             f'pub const MW_ART_SPRITES: [(u16, u8, bool); {len(sprite_rows)}] = [']
    lines += [f'    ({a}, {b}, {str(bool(c)).lower()}),' for a, b, c in sprite_rows]
    lines += ['];', '/// Per art clip, in `hk_sim::mawlek::Clip` order then the extras: first sequence entry,',
              '/// frames, fps (Q16), wrap, loop start.',
              f'pub const MW_ART_CLIPS: [(u16, u8, u32, u8, u8); {len(clip_rows)}] = [']
    lines += [f'    ({a}, {b}, {c}, {d}, {e}), // {n}' for a, b, c, d, e, n in clip_rows]
    lines += ['];', f'pub const MW_ART_SEQUENCE: [u16; {len(sequence)}] = [' + ', '.join(map(str, sequence)) + '];',
              '/// Each child\'s transform relative to the body\'s, Q16 world units.',
              f'pub const MW_DUMMY_OFFSET: [i32; 2] = {pair(geo["dummy"])};',
              f'pub const MW_ARM_OFFSET: [[i32; 2]; 2] = [{pair(geo["arms"][0])}, {pair(geo["arms"][1])}];',
              f'pub const MW_HEAD_OFFSET: [i32; 2] = {pair(geo["head"])};',
              f'pub const MW_SPIT_OFFSET: [i32; 2] = {pair(geo["spit"])};',
              '/// The Head\'s BoxCollider2D, each arm\'s `Attack Range` and the bounds of its swipe',
              '/// PolygonCollider2D, relative to the body, Q16 `[x0, y0, x1, y1]`.',
              f'pub const MW_HEAD_BOX: [i32; 4] = {box(geo["head_box"])};',
              f'pub const MW_ARM_RANGE: [[i32; 4]; 2] = [{box(geo["arm_ranges"][0])}, {box(geo["arm_ranges"][1])}];',
              f'pub const MW_ARM_HITBOX: [[i32; 4]; 2] = [{box(geo["arm_hitboxes"][0])}, {box(geo["arm_hitboxes"][1])}];',
              '/// `Corpse Egg Guardian`\'s box relative to its transform, and EnemyDeathEffects\' fling speed.',
              f'pub const MW_CORPSE_BOX: [i32; 4] = {box(geo["corpse_box"])};',
              f'pub const MW_CORPSE_FLING: i32 = {q(geo["corpse_fling"])};']
    return '\n'.join(lines) + '\n'


def cook_scene_bank(s, sc, actor, rows, atlas, frames, clips):
    """Check the contract and append the whole bank to the scene's actor atlas.

    Same shape as host/false_knight_art.py `cook_scene_bank`: returns the
    report entry and a function that writes data/mawlek_art.rs once the bank's
    clip base is known.
    """
    del actor
    src = sources(s, sc)
    digests = check_contract(s, sc, src)
    check_rust()
    geo = geometry(s, sc, src)
    sprites, art_clips, extra = source_art(s, sc, src)
    base = [(ROOT / 'data/regions' / f'region-{r["chunk_id"]:03}' / 'room.hk').read_bytes() for r in rows]
    scenery = fka.scenery_rects(base)
    quantized, cut, decisions, totals = fka.plan(sprites, art_clips, {}, scenery, atlas._quantize,
                                                 priority=PRIORITY, page_limit=SCENE_PAGE_LIMIT,
                                                 stream_limit=STREAM_BYTES_LIMIT, label='Mawlek')
    anchor, sprite_rows, clip_rows, sequence, _ = fka.append_bank(
        atlas, frames, clips, sprites, art_clips, {}, quantized, cut, names=ART_CLIPS, anchor_name='Mawlek parts')
    report = {'decisions': decisions, **totals, 'art_clips': list(ART_CLIPS), 'fsm_sha256': digests,
              'geometry': geo, **extra, 'anchor_clip_in_bank': anchor,
              'code_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}

    def write(clip_base):
        text = rust_table(anchor + clip_base, sprite_rows, clip_rows, sequence, geo, rows[0]['scene_id'])
        path = ROOT / 'data/mawlek_art.rs'
        if not path.is_file() or path.read_text() != text:
            path.write_text(text)
        report['anchor_clip'] = anchor + clip_base
        (ROOT / '.hkpsx/mawlek').mkdir(parents=True, exist_ok=True)
        (ROOT / '.hkpsx/mawlek/art.json').write_text(json.dumps(report, indent=2, default=str) + '\n')
    return report, write


if __name__ == '__main__':
    # Dry run: the contract and the art sizes, without touching any bank.
    from source import Source
    from scene import Scene
    s = Source()
    sc = Scene(s, mawlek.SCENE_FILE)
    src = sources(s, sc)
    print(json.dumps(check_contract(s, sc, src), indent=1))
    check_rust()
    print(json.dumps(geometry(s, sc, src), indent=1))
    sprites, art_clips, extra = source_art(s, sc, src)
    print(extra, len(sprites), 'sprites')
    for name in ART_CLIPS:
        keys = art_clips[name]['keys']
        print(f'{name:20} {len(keys):3} frames, sizes', sorted({sprites[k][2:] for k in keys})[:4])
