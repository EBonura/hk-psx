"""Verify and cook the original Hollow Shade against .hkpsx/shade/CONTRACT.md.

The Shade can appear in any scene the Knight died in, so its art cannot live
in a per-view atlas. Frames stay in linked RAM and reach VRAM through the
existing 64x64 animation slots; only a single 16-colour CLUT is resident.
"""
import hashlib, json, math
from pathlib import Path
from PIL import Image
from source import Source, ROOT, dump
from cook import tk_sprite, FOCAL, CAM_Z
from geo import q16, rust_array
from materials import quantize_alpha_coverage
from focus import action_fields
from combat import ticks

SHADE = 4541
SHADE_FSM = 21414
DEATH = 6109
DEATH_FSM = 20803
HERO_DEATH_FSM = 21158
LIBRARY = 22801
COLLECTION = 21382
CLUT = (320, 482, 16, 1)
# Free CLUT rows above the dialogue palette at x320.
CLUT_ROWS = 14
# The animation slot caps a single upload; the largest Shade frame is far under.
SLOT_BYTES = 2048
CLIPS = {'Idle': (12, 12., 0), 'Startle': (7, 12., 2), 'Fly': (5, 12., 0), 'TurnToFly': (9, 12., 1),
         'Slash Antic': (6, 12., 2), 'Slash': (2, 24., 2), 'Slash CD': (2, 12., 2),
         'Retreat Start': (7, 16., 2), 'Retreat End': (7, 16., 2),
         'Death Start': (1, 12., 6), 'Death': (12, 16., 2), 'Slash Effect': (2, 24., 2)}
# Clip order the guest indexes; Slash Effect is the child's, and stays last.
ORDER = ['Idle', 'Startle', 'Fly', 'TurnToFly', 'Slash Antic', 'Slash', 'Slash CD',
         'Retreat Start', 'Retreat End', 'Death Start', 'Death', 'Slash Effect']
BODY_SIZE = (0.5, 1.14)
BODY_OFFSET = (0.0, -0.37)
ALERT_RADIUS = 7.27
SLASH_SCALE = 1.2700315713882446
SLASH_LOCAL = (-0.0486297607421875, -0.0116996765136718)
# Fly and Position, the two states the controller reproduces.
FLY_ACTIONS = {
    'ChaseObject': {'speedMax': 4., 'acceleration': .2, 'targetSpread': 0.},
    'ChaseObjectV2': {'speedMax': 4., 'accelerationForce': 8.},
    'WaitRandom': {'timeMin': 1., 'timeMax': 2., 'finishEvent': 'ATTACK'},
    'FaceObject': {'spriteFacesRight': False, 'playNewAnimation': True, 'newAnimationClip': 'TurnToFly'},
    'Tk2dPlayAnimation': {'clipName': 'Fly'},
}
POSITION_ACTIONS = {
    'DistanceFly': {'distance': 3., 'speedMax': 4., 'acceleration': .2, 'targetsHeight': True, 'height': 0.},
    'Wait': {'time': 6., 'finishEvent': 'END'},
}
TRANSITIONS = {
    'Idle': [('ALERT', 'Startle'), ('TOOK DAMAGE', 'Startle'), ('FRIENDLY', 'Friendly Idle')],
    'Startle': [('FINISHED', 'Fly'), ('FRIENDLY', 'Friendly Idle')],
    'Fly': [('ATTACK', 'Quake?'), ('RETREAT', 'Retreat Start')],
    'Position': [('ATTACK', 'Slash Antic'), ('RETREAT', 'Retreat Start'), ('QUAKE', 'Quake Antic'),
                 ('SCREAM', 'Scream Antic'), ('END', 'Quake?')],
    'Slash Antic': [('FINISHED', 'Check Dir')],
    'Check Dir': [('RIGHT', 'Right'), ('LEFT', 'Left')],
    'Right': [('FINISHED', 'Slash')],
    'Left': [('FINISHED', 'Slash')],
    'Slash': [('SLASH', 'Slash Box')],
    'Slash Box': [('FINISHED', 'Slash CD')],
    'Slash CD': [('FINISHED', 'Fly')],
    'Retreat Start': [('FINISHED', 'Retreat')],
    'Retreat': [('FINISHED', 'Retreat End')],
    'Retreat End': [('FINISHED', 'Retreat Reset')],
    'Retreat Reset': [('FINISHED', 'Idle')],
}
# The spell branches hk_sim::shade reproduces: each state's transitions, then the
# audited actions (by name, the fields that select the one meant). The gates are
# `Init`'s PlayerData levels and `shadeMP`; hk_sim::shade holds the timings in ticks.
SPELL_TRANSITIONS = {
    'Attack Choice': [('SLASH', 'Q Other?'), ('FIREBALL', 'Sp Check')],
    'Sp Check': [('CAST', 'Fireball Pos'), ('RETURN', 'Attack Choice')],
    'Fireball Pos': [('ATTACK', 'Cast Antic'), ('RETREAT', 'Retreat Start')],
    'Cast Antic': [('WAIT', 'Cast Charge')],
    'Cast Charge': [('FINISHED', 'Check Dir 2')],
    'Check Dir 2': [('RIGHT', 'Right 2'), ('LEFT', 'Left 2')],
    'Left 2': [('FINISHED', 'Charge Wait')],
    'Right 2': [('FINISHED', 'Charge Wait')],
    'Charge Wait': [('WAIT', 'Check Dir 3')],
    'Check Dir 3': [('RIGHT', 'Shoot R'), ('LEFT', 'Shoot L')],
    'Shoot R': [('FINISHED', 'Cast')],
    'Shoot L': [('FINISHED', 'Cast')],
    'Cast': [('FINISHED', 'Decel')],
    'Decel': [('FINISHED', 'Cooldown')],
    'Cooldown': [('FINISHED', 'Fly')],
    'Quake?': [('FINISHED', 'Scream?'), ('QUAKE', 'Quake Antic')],
    'Scream?': [('FINISHED', 'Attack Choice'), ('SCREAM', 'Scream Antic')],
    'Q Other?': [('QUAKE', 'Will Quake'), ('SCREAM', 'Will Scream'), ('FINISHED', 'Position')],
    'Will Quake': [('FINISHED', 'Position')],
    'Will Scream': [('FINISHED', 'Position')],
    'Quake Antic': [('FINISHED', 'Quake Start')],
    'Quake Start': [('FINISHED', 'Quake')],
    'Quake': [('LAND', 'Land')],
    'Land': [('FINISHED', 'Return Pause')],
    'Return Pause': [('FINISHED', 'Quake Return')],
    'Quake Return': [('FINISHED', 'Reform')],
    'Reform': [('FINISHED', 'Collider On')],
    'Collider On': [('FINISHED', 'Fly')],
    'Scream Antic': [('FINISHED', 'Scream')],
    'Scream': [('FINISHED', 'Scream Recover')],
    'Scream Recover': [('FINISHED', 'Fly')],
}
SPELL_ACTIONS = {
    'Attack Choice': {'SendRandomEvent': {'1': 'FIREBALL', '2': 'SLASH', '4': .2, '5': .8}},
    'Sp Check': {'IntCompare': {'integer1': 'Fireball Level', 'integer2': 1, 'lessThan': 'RETURN'}},
    'Fireball Pos': {'IntOperator': {'integer1': 'SP', 'integer2': 1},
                     'DistanceFly': {'distance': 12., 'speedMax': 4., 'acceleration': .2, 'targetsHeight': True, 'height': 0.},
                     'FloatTestToBool': {'float1': 'Distance', 'float2': 12., 'tolerance': 1.},
                     'Wait': {'time': 3., 'finishEvent': 'ATTACK'}},
    'Cast Antic': {'Wait': {'time': .5, 'finishEvent': 'WAIT'}, 'FloatMultiplyV2': {'floatVariable': 'X Speed', 'multiplyBy': .8}},
    'Left 2': {'SetFloatValue': {'floatVariable': 'X Speed', 'floatValue': 10.}},
    'Right 2': {'SetFloatValue': {'floatVariable': 'X Speed', 'floatValue': -10.}},
    'Charge Wait': {'Wait': {'time': .25, 'finishEvent': 'WAIT'}},
    'Shoot R': {'SetVelocity2d': {'x': 20.}},
    'Shoot L': {'SetVelocity2d': {'x': -20.}, 'SetScale': {'x': -1.}},
    'Cast': {'Tk2dPlayAnimationWithEvents': {'clipName': 'Cast'}, 'SetVelocity2d': {'x': 'X Speed', 'y': ''}},
    'Decel': {'FloatMultiplyV2': {'floatVariable': 'X Speed', 'multiplyBy': .9}},
    'Cooldown': {'FloatMultiplyV2': {'floatVariable': 'X Speed', 'multiplyBy': .9}, 'Wait': {'time': .25}},
    'Quake?': {'IntCompare': {'integer1': 'Quake Level', 'integer2': 0}, 'BoolTest': {'boolVariable': 'Quake Range'},
               'SendRandomEvent': {'17': 'FINISHED', '18': 'QUAKE', '20': .5, '21': .5}},
    'Scream?': {'IntCompare': {'integer1': 'Scream Level', 'integer2': 0}, 'BoolTest': {'boolVariable': 'Scream Range'},
                'SendRandomEvent': {'17': 'FINISHED', '18': 'SCREAM', '20': .5, '21': .5}},
    'Q Other?': {'IntCompare': {'integer1': 'SP', 'integer2': 0, 'equal': 'FINISHED'},
                 'SendRandomEvent': {'13': 'SCREAM', '14': 'QUAKE', '15': 'FINISHED', '17': 1., '18': 1., '19': 1.}},
    'Quake Antic': {'IntOperator': {'integer1': 'SP', 'integer2': 1}, 'SetVelocity2d': {'x': 0., 'y': 10.},
                    'DecelerateXY': {'decelerationY': .85}, 'Wait': {'time': .8}},
    'Quake Start': {'Tk2dPlayAnimationWithEvents': {'clipName': 'Quake Start'}},
    'Quake': {'SetVelocity2d': {'x': '', 'y': -30.}, 'Wait': {'time': 1., 'finishEvent': 'LAND'}},
    'Land': {'SetVelocity2d': {'x': 0., 'y': 0.}, 'Tk2dPlayAnimationWithEvents': {'clipName': 'Quake Land'}},
    'Return Pause': {'Wait': {'time': 1.}},
    'Reform': {'Tk2dPlayAnimationWithEvents': {'clipName': 'Retreat End'}},
    'Scream Antic': {'IntOperator': {'integer1': 'SP', 'integer2': 1}, 'DecelerateV2': {'deceleration': .85},
                     'Wait': {'time': 1.}},
    'Scream': {'Wait': {'time': 1.}},
    'Scream Recover': {'Tk2dPlayAnimationWithEvents': {'clipName': 'Scream End'}},
}
# The spell clips' frame counts, rates and wraps set hk_sim::shade's tick counts.
# Their art is not cooked: the carried pack has no room (see `limitations`).
SPELL_CLIPS = {'Cast Antic': (2, 12., 2), 'Cast Charge': (3, 12., 3), 'Cast': (8, 12., 2),
               'Quake Antic': (5, 12., 1), 'Quake Start': (1, 12., 2), 'Quake': (3, 20., 0),
               'Quake Land': (3, 15., 2), 'Scream Antic': (3, 12., 0), 'Scream': (4, 12., 1),
               'Scream End': (2, 12., 2), 'Fireball': (6, 20., 1), 'Fireball End': (4, 24., 2)}
SHADOW_BALL = 5351
SHADOW_BALL_SPEED = 20.


def _near(a, b):
    return abs(float(a) - float(b)) <= 1e-6


def _scalar(value):
    if isinstance(value, dict):
        return value['name'] if value.get('useVariable') else value['value']
    return value


def _components(source, file, gid):
    go = source.read(file.objects[gid])
    out = []
    for c in go['m_Component']:
        obj = source.ref(file, c['component'])
        out.append((source.typename(obj), obj))
    return go, out


def _one(components, name):
    matches = [obj for typename, obj in components if typename == name]
    if len(matches) != 1:
        raise ValueError('expected one ' + name + ' on the Hollow Shade')
    return matches[0]


def _child(source, file, gid, name):
    go = source.read(file.objects[gid])
    transform = source.ref(file, [c['component'] for c in go['m_Component']][0])
    for ref in source.read(transform)['m_Children']:
        child = source.read(source.ref(file, ref))
        target = source.read(file.objects[child['m_GameObject']['m_PathID']])
        if target['m_Name'] == name:
            return child['m_GameObject']['m_PathID'], child
    raise ValueError('missing Hollow Shade child: ' + name)


def _state_actions(fsm, name):
    state = next((s for s in fsm['states'] if s['name'] == name), None)
    if state is None:
        raise ValueError('missing Shade Control state: ' + name)
    return state


def _matches(actual, fields):
    for key, value in fields.items():
        got = _scalar(actual.get(key))
        if isinstance(value, float) and isinstance(got, (int, float)) and not isinstance(got, bool):
            if not _near(got, value):
                return False
        elif got != value:
            return False
    return True


def _check_actions(fsm, state_name, expected):
    """Exactly one enabled action of each name must carry the audited fields.

    A state may hold several actions of one name (Remove Geo writes both
    `geoPool` and `geo`), so the fields select which one is meant.
    """
    data = _state_actions(fsm, state_name)['actionData']
    for action, fields in expected.items():
        hits = [i for i, n in enumerate(data['actionNames'])
                if n.rsplit('.', 1)[-1] == action and data['actionEnabled'][i]
                and _matches(action_fields(data, i), fields)]
        if len(hits) != 1:
            raise ValueError(f'unsupported Shade action set: {state_name}/{action}')


def _box(collider, scale=(1., 1.), position=(0., 0.)):
    """A BoxCollider2D on a child at `position` with `scale`, as Shade-local bounds."""
    w, h = collider['m_Size']['x'] / 2, collider['m_Size']['y'] / 2
    ox, oy = collider['m_Offset']['x'], collider['m_Offset']['y']
    xs = sorted(position[0] + (ox + d) * scale[0] for d in (-w, w))
    ys = sorted(position[1] + (oy + d) * scale[1] for d in (-h, h))
    return [xs[0], ys[0], xs[1], ys[1]]


def _hit_child(source, resources, name):
    """A spell hit child: inactive, its trigger, DamageHero 1 and a .2 s Deactivate."""
    gid, transform = _child(source, resources, SHADE, name)
    go, components = _components(source, resources, gid)
    damage = source.read(_one(components, 'DamageHero'))
    if go['m_IsActive'] or damage['damageDealt'] != 1 or damage['hazardType'] != 1:
        raise ValueError('unsupported Hollow Shade spell hit: ' + name)
    fsms = [source.read(obj)['fsm'] for typename, obj in components if typename == 'PlayMakerFSM']
    if len(fsms) != 1 or fsms[0]['name'] != 'Deactivate':
        raise ValueError('unsupported Hollow Shade spell hit lifetime: ' + name)
    _check_actions(fsms[0], 'Wait', {'Wait': {'time': .2, 'finishEvent': 'FINISHED'}})
    if transform['m_LocalRotation'] != {'x': 0.0, 'y': 0.0, 'z': 0.0, 'w': 1.0}:
        raise ValueError('rotated Hollow Shade spell hit: ' + name)
    return components, transform


def _range_child(source, resources, name, variable):
    """Quake Range / Scream Range: a trigger box whose FSM sets the Shade's bool."""
    gid, transform = _child(source, resources, SHADE, name)
    go, components = _components(source, resources, gid)
    box = source.read(_one(components, 'BoxCollider2D'))
    fsm = source.read(_one(components, 'PlayMakerFSM'))['fsm']
    names = {v['name']: v['value'] for v in fsm['variables']['stringVariables']}
    if not go['m_IsActive'] or not box['m_IsTrigger'] or names != {'Bool Name': variable, 'FSM Name': 'Shade Control'} \
            or transform['m_LocalPosition']['x'] != 0 or transform['m_LocalPosition']['y'] != 0 \
            or transform['m_LocalScale']['x'] != 1 or transform['m_LocalScale']['y'] != 1:
        raise ValueError('unsupported Hollow Shade range trigger: ' + name)
    return _box(box)


def _spell_geometry(source, resources, fsm):
    """The spells' shapes and the Shadow Ball, Shade-local (ball-local for the ball)."""
    quake_components, quake_transform = _hit_child(source, resources, 'Quake Hit')
    quake_box = source.read(_one(quake_components, 'BoxCollider2D'))
    if not quake_box['m_IsTrigger']:
        raise ValueError('unsupported Hollow Shade Quake Hit collider')
    position = (quake_transform['m_LocalPosition']['x'], quake_transform['m_LocalPosition']['y'])
    scale = (quake_transform['m_LocalScale']['x'], quake_transform['m_LocalScale']['y'])
    quake_hit = _box(quake_box, scale, position)
    scream_components, scream_transform = _hit_child(source, resources, 'Scream Hit')
    circle = source.read(_one(scream_components, 'CircleCollider2D'))
    if not circle['m_IsTrigger']:
        raise ValueError('unsupported Hollow Shade Scream Hit collider')
    s = scream_transform['m_LocalScale']
    # CircleCollider2D scales its radius by the larger of the two scale axes.
    k = max(abs(s['x']), abs(s['y']))
    scream_hit = [scream_transform['m_LocalPosition']['x'] + circle['m_Offset']['x'] * s['x'],
                  scream_transform['m_LocalPosition']['y'] + circle['m_Offset']['y'] * s['y'], circle['m_Radius'] * k]
    # Shoot R and Shoot L spawn the Shadow Ball; its own FSM waits 1 s, then Fade
    # plays Fireball End without touching the velocity, and the ball goes.
    data = _state_actions(fsm, 'Shoot R')['actionData']
    spawned = [action_fields(data, i, objects=True) for i, n in enumerate(data['actionNames'])
               if n.rsplit('.', 1)[-1] == 'CreateObject']
    if len(spawned) != 1 or spawned[0]['gameObject']['value']['m_PathID'] != SHADOW_BALL:
        raise ValueError('unsupported Hollow Shade Shoot R spawn')
    go, components = _components(source, resources, SHADOW_BALL)
    ball_box = source.read(_one(components, 'BoxCollider2D'))
    body = source.read(_one(components, 'Rigidbody2D'))
    if go['m_Name'] != 'Shadow Ball' or not ball_box['m_IsTrigger'] or body['m_GravityScale'] != 0:
        raise ValueError('unsupported Shadow Ball')
    fsms = {f['name']: f for f in (source.read(obj)['fsm'] for typename, obj in components if typename == 'PlayMakerFSM')}
    damage = {v['name']: v['value'] for v in fsms['damages_hero']['variables']['intVariables']}
    if damage.get('damageDealt') != 1 or damage.get('hazardType') != 1:
        raise ValueError('unsupported Shadow Ball damage')
    ball = fsms['Shadow ball']
    if [(s['name'], [(t['fsmEvent']['name'], t['toState']) for t in s['transitions']]) for s in ball['states']] \
            != [('Wait', [('WAIT', 'Fade')]), ('Fade', [('WAIT', 'Destroy')]), ('Destroy', [])]:
        raise ValueError('unsupported Shadow ball FSM')
    _check_actions(ball, 'Wait', {'Wait': {'time': 1., 'finishEvent': 'WAIT'}})
    _check_actions(ball, 'Fade', {'Tk2dPlayAnimationWithEvents': {'clipName': 'Fireball End', 'animationCompleteEvent': 'WAIT'}})
    if any(n.rsplit('.', 1)[-1].startswith('SetVelocity') for n in _state_actions(ball, 'Fade')['actionData']['actionNames']):
        raise ValueError('Shadow ball Fade now changes its speed')
    from charms import hero_constants
    _, hero = hero_constants(source)
    return {'quake_range': _range_child(source, resources, 'Quake Range', 'Quake Range'),
            'scream_range': _range_child(source, resources, 'Scream Range', 'Scream Range'),
            'quake_hit': quake_hit, 'scream_hit': scream_hit, 'ball_bounds': _box(ball_box),
            'parry_invuln_seconds': hero['INVUL_TIME_PARRY']}


def audit(source):
    """Raise unless the live prefab still matches the audited contract."""
    resources = source.file('resources.assets')
    go, components = _components(source, resources, SHADE)
    if go['m_Name'] != 'Hollow Shade' or go.get('m_TagString', '') not in ('', 'Untagged'):
        raise ValueError('changed Hollow Shade identity')
    body = source.read(_one(components, 'BoxCollider2D'))
    if body['m_IsTrigger'] or body['m_EdgeRadius'] != 0 \
            or not all(_near(body['m_Size'][k], v) for k, v in zip('xy', BODY_SIZE)) \
            or not all(_near(body['m_Offset'][k], v) for k, v in zip('xy', BODY_OFFSET)):
        raise ValueError('unsupported Hollow Shade body collider')
    rigid = source.read(_one(components, 'Rigidbody2D'))
    if rigid['m_BodyType'] != 0 or rigid['m_GravityScale'] != 0 or rigid['m_LinearDamping'] != 0 \
            or rigid['m_Constraints'] != 4:
        raise ValueError('unsupported Hollow Shade rigid body')
    recoil = source.read(_one(components, 'Recoil'))
    if recoil['freezeInPlace'] or recoil['recoilSpeedBase'] != 15 or not _near(recoil['recoilDuration'], .15) \
            or recoil['preventRecoilUp']:
        raise ValueError('unsupported Hollow Shade recoil variant')
    damage = source.read(_one(components, 'DamageHero'))
    if damage['damageDealt'] != 1 or damage['hazardType'] != 1:
        raise ValueError('unsupported Hollow Shade contact damage')
    health = source.read(_one(components, 'HealthManager'))
    if health['hp'] != 10 or health['smallGeoDrops'] or health['mediumGeoDrops'] or health['largeGeoDrops']:
        raise ValueError('unsupported Hollow Shade health manager')
    alert_gid, _ = _child(source, resources, SHADE, 'Alert Range New')
    alert_go, alert_components = _components(source, resources, alert_gid)
    circle = source.read(_one(alert_components, 'CircleCollider2D'))
    if not alert_go['m_IsActive'] or not circle['m_IsTrigger'] or not _near(circle['m_Radius'], ALERT_RADIUS) \
            or circle['m_Offset'] != {'x': 0., 'y': 0.}:
        raise ValueError('unsupported Hollow Shade alert range')
    slash_gid, slash_transform = _child(source, resources, SHADE, 'Slash')
    slash_go, slash_components = _components(source, resources, slash_gid)
    if slash_go['m_IsActive'] or not _near(slash_transform['m_LocalScale']['x'], SLASH_SCALE) \
            or not all(_near(slash_transform['m_LocalPosition'][k], v) for k, v in zip('xy', SLASH_LOCAL)):
        raise ValueError('unsupported Hollow Shade Slash child placement')
    polygon = source.read(_one(slash_components, 'PolygonCollider2D'))
    if not polygon['m_IsTrigger'] or len(polygon['m_Points']['m_Paths']) != 1:
        raise ValueError('unsupported Hollow Shade Slash collider')
    slash_damage = source.read(_one(slash_components, 'DamageHero'))
    if slash_damage['damageDealt'] != 1 or slash_damage['hazardType'] != 1:
        raise ValueError('unsupported Hollow Shade Slash damage')
    path = [(p['x'] * SLASH_SCALE + SLASH_LOCAL[0], p['y'] + SLASH_LOCAL[1]) for p in polygon['m_Points']['m_Paths'][0]]
    slash_bounds = [min(p[0] for p in path), min(p[1] for p in path),
                    max(p[0] for p in path), max(p[1] for p in path)]
    fsm = source.read(resources.objects[SHADE_FSM])['fsm']
    if fsm['name'] != 'Shade Control' or fsm['startState'] != 'Pause':
        raise ValueError('unsupported Shade Control entry')
    states = {s['name']: s for s in fsm['states']}
    for name, transitions in TRANSITIONS.items():
        if name not in states or [(t['fsmEvent']['name'], t['toState']) for t in states[name]['transitions']] != transitions:
            raise ValueError('unsupported Shade Control transitions: ' + name)
    _check_actions(fsm, 'Fly', FLY_ACTIONS)
    _check_actions(fsm, 'Position', POSITION_ACTIONS)
    _check_actions(fsm, 'Slash Antic', {'Tk2dPlayAnimationWithEvents': {'clipName': 'Slash Antic'},
                                        'DistanceFly': {'distance': 3., 'speedMax': 4., 'acceleration': .2}})
    _check_actions(fsm, 'Slash', {'Wait': {'time': .0829999968409538, 'finishEvent': 'SLASH'}})
    _check_actions(fsm, 'Right', {'SetVelocity2d': {'x': 8.}})
    _check_actions(fsm, 'Left', {'SetVelocity2d': {'x': -8.}})
    variables = {v['name']: v['value'] for group in fsm['variables'].values() if isinstance(group, list)
                 for v in group if isinstance(v, dict) and 'name' in v and 'value' in v}
    if not _near(variables.get('Max Roam', 0), 25.):
        raise ValueError('unsupported Shade Max Roam')
    for name, transitions in SPELL_TRANSITIONS.items():
        if name not in states or [(t['fsmEvent']['name'], t['toState']) for t in states[name]['transitions']] != transitions:
            raise ValueError('unsupported Shade Control spell transitions: ' + name)
    for name, actions in SPELL_ACTIONS.items():
        _check_actions(fsm, name, actions)
    spells = _spell_geometry(source, resources, fsm)
    death = source.read(resources.objects[DEATH_FSM])['fsm']
    if death['name'] != 'Shade Control' or death['startState'] != 'Death Start':
        raise ValueError('unsupported Hollow Shade Death entry')
    _check_actions(death, 'Death Start', {'SetPlayerDataInt': {'intName': 'geoPool', 'value': 0},
                                          'Wait': {'time': .5, 'finishEvent': 'FINISHED'}})
    hero = source.read(resources.objects[HERO_DEATH_FSM])['fsm']
    _check_actions(hero, 'Remove Geo', {'SetPlayerDataInt': {'intName': 'geo', 'value': 0}})
    _check_actions(hero, 'Drain Soul', {'SetPlayerDataInt': {'intName': 'MPReserve', 'value': 0}})
    library = source.read(resources.objects[LIBRARY])
    by_name = {c['name']: c for c in library['clips'] if c['name']}
    for name, (frames, fps, wrap) in list(CLIPS.items()) + list(SPELL_CLIPS.items()):
        clip = by_name.get(name)
        if clip is None or (len(clip['frames']), clip['fps'], clip['wrapMode']) != (frames, fps, wrap):
            raise ValueError('unsupported Hollow Shade animation: ' + name)
    return by_name, slash_bounds, {'body': list(BODY_SIZE), 'body_offset': list(BODY_OFFSET),
                                   'alert_radius': ALERT_RADIUS, 'hp': health['hp'], 'slash_bounds': slash_bounds,
                                   **spells}


def cook():
    source = Source()
    by_name, slash_bounds, facts = audit(source)
    resources = source.file('resources.assets')
    scale = FOCAL / -CAM_Z
    textures = {}
    images = []
    boxes = []
    art_sources = []

    def sprite(collection_ref, index):
        obj = source.ref(resources, collection_ref)
        im, box = tk_sprite(source, obj.assets_file, source.read(obj), index, textures)
        dims = tuple(max(1, math.ceil((box[i + 2] - box[i]) * scale)) for i in (0, 1))
        images.append(im.resize(dims, Image.Resampling.LANCZOS))
        boxes.append(list(box))
        art_sources.append(f'{source.sid(obj)}:{index}')
        return len(images) - 1

    clips = []
    for name in ORDER:
        clip = by_name[name]
        start = len(images)
        for frame in clip['frames']:
            sprite(frame['spriteCollection'], frame['spriteId'])
        clips.append(dict(name=name, start=start, count=len(clip['frames']),
                          fps=int(clip['fps']), wrap=clip['wrapMode']))
    # 53 frames exceed one 256x256 quantizer sheet, so clips are grouped into
    # sheets of at most that size, each with its own CLUT. Grouping by clip
    # keeps every frame of one animation on a single palette.
    def shelf(sizes, limit=256):
        """Shelf packing; None when this set cannot fit one sheet."""
        order = sorted(range(len(sizes)), key=lambda i: -sizes[i][1])
        positions = [None] * len(sizes)
        x = y = row = 0
        for i in order:
            w, h = sizes[i]
            if w > limit or h > limit:
                return None
            if x + w > limit:
                x = 0
                y += row
                row = 0
            if y + h > limit:
                return None
            positions[i] = (x, y)
            x += w
            row = max(row, h)
        return positions

    groups = []
    current = []
    for clip in clips:
        candidate = current + list(range(clip['start'], clip['start'] + clip['count']))
        if shelf([images[i].size for i in candidate]) is None:
            if not current:
                raise ValueError('a single Hollow Shade clip does not fit one sheet')
            groups.append(current)
            current = list(range(clip['start'], clip['start'] + clip['count']))
        else:
            current = candidate
    if current:
        groups.append(current)
    palettes = []
    blob = bytearray()
    frames = [None] * len(images)
    sheets = []
    for clut_index, group in enumerate(groups):
        sizes = [images[i].size for i in group]
        positions = shelf(sizes)
        sheet = Image.new('RGBA', (256, 256))
        for i, origin in zip(group, positions):
            sheet.paste(images[i], origin)
        sw, sh, palette, packed = quantize_alpha_coverage(sheet, 128)
        palettes.append(palette)
        sheets.append(sheet)
        for i, (x0, y0) in zip(group, positions):
            im = images[i]
            stride = (im.width + 3) // 4 * 2
            texels = bytearray(stride * im.height)
            for y in range(im.height):
                for x in range(im.width):
                    b = packed[(y + y0) * ((sw + 1) // 2) + (x + x0) // 2]
                    n = (b >> (((x + x0) & 1) * 4)) & 15
                    texels[y * stride + x // 2] |= n << ((x & 1) * 4)
            if len(texels) > SLOT_BYTES:
                raise ValueError('Hollow Shade frame exceeds one animation slot')
            frames[i] = dict(offset=len(blob), width=im.width, height=im.height,
                             bounds=boxes[i], clut=clut_index)
            blob.extend(texels)
    if len(palettes) > CLUT_ROWS:
        raise ValueError('Hollow Shade palettes exceed the free CLUT rows')
    palette = b''.join(palettes)
    payload = bytes(palette) + bytes(blob)
    rust = ['// Generated source Hollow Shade art; no embedded retail source dump.',
            f'pub const CLUT_RECT:(u16,u16,u16,u16)=({CLUT[0]},{CLUT[1]},{CLUT[2]},{CLUT[3]});',
            f'pub const PALETTE_BYTES:usize={len(palette)};',
            f'pub const PALETTE_COUNT:usize={len(palettes)};',
            f'pub const SHADE_HP:u16={facts["hp"]};',
            'pub const SLASH_BOUNDS:[i32;4]=' + rust_array([q16(v) for v in slash_bounds]) + ';',
            'pub const BODY_BOUNDS:[i32;4]=' + rust_array([q16(v) for v in (
                BODY_OFFSET[0] - BODY_SIZE[0] / 2, BODY_OFFSET[1] - BODY_SIZE[1] / 2,
                BODY_OFFSET[0] + BODY_SIZE[0] / 2, BODY_OFFSET[1] + BODY_SIZE[1] / 2)]) + ';',
            f'pub const ALERT_RADIUS:i32={q16(ALERT_RADIUS)};',
            '/// The Quake Range and Scream Range triggers, Shade-local (x is symmetric).',
            'pub const QUAKE_RANGE:[i32;4]=' + rust_array([q16(v) for v in facts['quake_range']]) + ';',
            'pub const SCREAM_RANGE:[i32;4]=' + rust_array([q16(v) for v in facts['scream_range']]) + ';',
            '/// Quake Hit as authored (the Shade facing left), and Scream Hit as centre and radius.',
            'pub const QUAKE_HIT:[i32;4]=' + rust_array([q16(v) for v in facts['quake_hit']]) + ';',
            'pub const SCREAM_HIT:[i32;3]=' + rust_array([q16(v) for v in facts['scream_hit']]) + ';',
            '/// The Shadow Ball\'s box, ball-local as authored (flying right).',
            'pub const BALL_BOUNDS:[i32;4]=' + rust_array([q16(v) for v in facts['ball_bounds']]) + ';',
            '/// `NailParry`: parryInvulnTimer = INVUL_TIME_PARRY, in 60 Hz ticks.',
            f'pub const PARRY_INVULN_TICKS:u16={ticks(facts["parry_invuln_seconds"])};',
            'pub const SHADE_CLIPS:&[Clip]=&[' + ','.join(
                'Clip{' + ','.join(f'{k}:{c[k]}' for k in ('start', 'count', 'fps', 'wrap')) + '}'
                for c in clips) + '];',
            'pub const SHADE_FRAMES:&[Frame]=&[']
    for f in frames:
        rust.append('Frame{' + f'offset:{f["offset"]},width:{f["width"]},height:{f["height"]},clut:{f["clut"]},'
                    + 'bounds:' + rust_array([q16(v) for v in f['bounds']]) + '},')
    rust.append('];')
    for i, name in enumerate(ORDER):
        rust.append(f'pub const CLIP_{name.upper().replace(" ", "_")}:usize={i};')
    text = '\n'.join(rust) + '\n'
    (ROOT / 'data/shade.rs').write_text(text)
    (ROOT / 'data/shade.hk').write_bytes(payload)
    report = {'frames': len(frames), 'bytes': len(payload), 'palette_bytes': len(palette),
              'palettes': len(palettes), 'clip_groups': [[clips[c]['name'] for c in range(len(clips))
                  if clips[c]['start'] in g] for g in groups],
              'clut_rect': list(CLUT), 'clips': clips, 'source_art': art_sources,
              'largest_frame_bytes': max((f['width'] + 3) // 4 * 2 * f['height'] for f in frames),
              'facts': facts, 'image_dimensions': [im.size for im in images],
              'limitations': [
                  'Frames reach VRAM through the existing 64x64 animation slots; only the 16-colour CLUT is resident.',
                  'Every particle system, the death orbs, the Soul Orb HUD event, the light effect and all Shade audio are not presented.',
                  'The Slash polygon becomes its axis-aligned bounds and the retreat iTween path a straight interpolation.',
                  'The Fireball (Shadow Ball), Quake and Scream branches run from the levels and shadeMP the death recorded, but '
                  'their art is not cooked: the spell body clips (15,620 bytes), the Shadow Ball (7,748), Cast Ring, Shade Quake, '
                  'Quake 2 Pillar and Scream Blast do not fit the carried pack (the tightest room has about 500 bytes spare), '
                  'so the Shade holds the Fly clip while it casts and the ball and blasts are not drawn.',
                  'The Quake Return iTween is a straight line on easeInOutSine; the retreat stays a straight interpolation.',
                  'Parrying the Slash (nail_clash_tink) recoils the Knight and gives the parry invulnerability; FreezeMoment(3), '
                  'NailParryRecover and the Nail Clash Effect are not reproduced, as for the Mawlek.',
                  'The Friendly, Lake, Jar and Colosseum branches are outside the slice and not implemented.'],
              'payload_sha256': hashlib.sha256(payload).hexdigest(),
              'rust_sha256': hashlib.sha256(text.encode()).hexdigest(),
              'source_sha256': {name: hashlib.sha256((source.directory / name).read_bytes()).hexdigest()
                                for name in list(source.files) + ['Managed/Assembly-CSharp.dll']},
              'tool_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
    dump(ROOT / '.hkpsx/shade-provenance.json', report)
    Path(ROOT / '.hkpsx/shade').mkdir(parents=True, exist_ok=True)
    for i, sheet in enumerate(sheets):
        sheet.save(ROOT / f'.hkpsx/shade/art{i}.png')
    print(f'Hollow Shade: {len(frames)} frames, {len(palettes)} palettes, {len(payload)} bytes, '
          f'largest {report["largest_frame_bytes"]}')
    return report


if __name__ == '__main__':
    cook()
