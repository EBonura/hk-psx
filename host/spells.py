"""Spell definitions from the Hero's `Spell Control` FSM and the spell prefabs.

P15 step 1 wants the spell catalog taken from the source rather than assumed,
and the source spreads one spell across three FSMs: `Spell Control` on the Hero
decides what is cast and pays for it, the spawned `Fireball Cast` object aims
and launches, and the `Fireball`/`damages_enemy` pair on the projectile carries
its damage, lifetime and wall behaviour. All three are read here.

Vengeful Spirit is bound from those three. Desolate Dive and Howling Wraiths
(level 1) come from `Spell Control` and the hit children of the Knight's own
`Spells` folder (`source_quake_scream_values`); the older report's
`catalogued_only` entry predates them and is kept for the Rust parity oracle.
No retail payload is embedded; the report goes to .hkpsx.
"""
from superdash import _fsm
from focus import action_fields, fsm_variables

def _variables(fsm):
    return fsm_variables(fsm)

def _reader(fsm):
    states = {s['name']: s for s in fsm['states']}
    variables = _variables(fsm)
    def scalar(value):
        if isinstance(value, dict) and 'value' in value and 'useVariable' in value:
            return variables.get(value['name']) if value['useVariable'] else value['value']
        return value
    def actions(state, kind):
        d = states[state]['actionData']
        return [action_fields(d, i) for i, n in enumerate(d['actionNames'])
                if n.rsplit('.', 1)[-1] == kind and d['actionEnabled'][i]]
    return states, variables, scalar, actions

def _named_child(source, file, state, name):
    """The prefab a state's SpawnObjectFromGlobalPool points at, by name."""
    for param in state['actionData']['fsmGameObjectParams']:
        path_id = param['value']['m_PathID']
        if not path_id:
            continue
        obj = source.ref(file, {'m_FileID': 0, 'm_PathID': path_id})
        tree = source.read(obj)
        if tree.get('m_Name') == name:
            return tree
    raise LookupError(f'no {name} prefab spawned by {state["name"]}')

def _fsm_on(source, file, prefab, name):
    for component in prefab['m_Component']:
        obj = source.ref(file, component['component'])
        if source.typename(obj) != 'PlayMakerFSM':
            continue
        fsm = source.read(obj)['fsm']
        if fsm['name'] == name:
            return fsm
    raise LookupError(f'{prefab["m_Name"]} carries no {name} FSM')

def _collider(source, file, prefab):
    for component in prefab['m_Component']:
        obj = source.ref(file, component['component'])
        if obj.type.name == 'BoxCollider2D':
            box = source.read(obj)
            return {'offset': box['m_Offset'], 'size': box['m_Size']}
    raise LookupError(f'{prefab["m_Name"]} carries no BoxCollider2D')

def source_spell_values(source):
    file = source.file('resources.assets')
    control = _fsm(source, 'Spell Control')
    states, variables, scalar, actions = _reader(control)
    # A tap casts, a hold focuses: Button Down waits this long for the release.
    tap = variables['Button Down Time']
    cost = variables['MP Cost']
    gate = actions('Can Cast?', 'IntCompare')
    assert len(gate) == 1 and scalar(gate[0]['integer2']) == cost, 'the cast gate no longer reads MP Cost'
    assert scalar(actions('Can Cast?', 'GetPlayerDataInt')[0]['intName']) == 'MPCharge'
    level = actions('Has Fireball?', 'GetPlayerDataInt')
    assert len(level) == 1 and scalar(level[0]['intName']) == 'fireballLevel'
    # Every cast phase ends on its own clip, as the Dream Nail's do.
    for state, clip in (('Fireball Antic', 'Fireball Antic'), ('Fireball 1', 'Fireball1 Cast')):
        played = actions(state, 'Tk2dPlayAnimationWithEvents')
        assert len(played) == 1 and scalar(played[0]['clipName']) == clip, f'{state} clip moved'

    # Fireball 1 spawns the ball and sends FINISHED in the same frame, and
    # Fireball Recoil then watches that same clip finish, so the cast clip is
    # the recoil's length rather than a delay before the ball leaves.
    from dream_nail import _knight_animation
    table = _knight_animation(source)
    clips = {c['name']: len(c['frames']) / c['fps'] for c in table['clips']
             if c['name'] in ('Fireball Antic', 'Fireball1 Cast')}
    assert len(clips) == 2, 'the Knight lost a Fireball clip'
    caster = _named_child(source, file, states['Fireball 1'], 'Fireball Top')
    cast = _fsm_on(source, file, caster, 'Fireball Cast')
    _, cast_vars, cast_scalar, cast_actions = _reader(cast)
    speed = cast_scalar(cast_actions('Cast Right', 'SetVelocityAsAngle')[0]['speed'])
    left = cast_scalar(cast_actions('Cast Left', 'SetVelocityAsAngle')[0]['speed'])
    assert speed == left, 'the two directions no longer share a speed'
    recycle = cast_scalar(cast_actions('Wait', 'Wait')[0]['time'])

    cast_states = {s['name']: s for s in cast['states']}
    ball = _named_child(source, file, cast_states['Cast Right'], 'Fireball')
    ball_control = _fsm_on(source, file, ball, 'Fireball Control')
    _, _, ball_scalar, ball_actions = _reader(ball_control)
    lifetime = ball_scalar(ball_actions('Idle', 'Wait')[0]['time'])
    # Set Damage writes the no-charm damage into damages_enemy; the second
    # write is Shaman Stone (charm 19) and is not taken without charms.
    damages = ball_actions('Set Damage', 'SetFsmInt')
    assert len(damages) == 2, 'Set Damage no longer has a plain and a charmed value'
    damage = ball_scalar(damages[0]['setValue'])
    walls = ball_actions('Idle', 'Collision2dEventLayer')
    assert walls and all(a['sendEvent'] == 'WALL' for a in walls), 'the wall stop moved'
    return {
        'fireball': {
            'tap_seconds': tap,
            'antic_seconds': clips['Fireball Antic'],
            'cast_seconds': clips['Fireball1 Cast'],
            'cost': cost,
            'speed': speed,
            'lifetime': lifetime,
            'recycle': recycle,
            'damage': damage,
            'collider': _collider(source, file, ball),
            'gate': 'fireballLevel > 0',
        },
        'catalogued_only': {
            'quake': 'Desolate Dive, gated on quakeLevel; no admitted scene grants or needs it',
            'scream': 'Howling Wraiths, gated on screamLevel; likewise',
        },
        'limitations': [
            'Only Vengeful Spirit is implemented. Desolate Dive and Howling Wraiths are '
            'catalogued with their PlayerData gates and nothing more.',
            'Charm variants (Shaman Stone damage, Flukenest, Defenders Crest, Spell Twister) '
            'are read but not applied, since charms do not exist yet.',
            'The Fireball Antic and cast clips, the ball sprite and every impact effect '
            'are not cooked.',
        ],
    }

def generated_spell_params(values):
    def ticks(seconds):
        return max(1, round(seconds * 60))
    f = values['fireball']
    out = (f"pub const FIREBALL_ANTIC_TICKS: u16 = {ticks(f['antic_seconds'])};\n"
           f"pub const FIREBALL_CAST_TICKS: u16 = {ticks(f['cast_seconds'])};\n")
    box = f['collider']
    half_w = box['size']['x'] / 2
    half_h = box['size']['y'] / 2
    bounds = (box['offset']['x'] - half_w, box['offset']['y'] - half_h,
              box['offset']['x'] + half_w, box['offset']['y'] + half_h)
    out += 'pub const FIREBALL_PARAMS: hk_sim::FireballParams = hk_sim::FireballParams {'
    out += ','.join(f'{k}:{v}' for k, v in (
        ('tap_ticks', ticks(f['tap_seconds'])),
        ('cost', f['cost']),
        ('speed', round(f['speed'] * 65536)),
        ('life_ticks', ticks(f['lifetime'])),
        ('damage', f['damage']),
        ('bounds', '[' + ','.join(str(round(v * 65536)) for v in bounds) + ']'),
    )) + '};\n'
    return out


# Desolate Dive and Howling Wraiths (level 1). Their hits are not spawned prefabs:
# they are children of the Knight's `Spells` folder that Spell Control switches on
# (`Q Fall Damage`, `Q Slam`, `Scr Heads`), each a parent whose `Hit L`, `Hit R` (and
# `Hit U`) children carry a PolygonCollider2D and a `damages_enemy`. The levels above
# 1 (Descending Dark's `Q Slam 2`, `Q Mega` and pillar phase; Abyss Shriek's
# `Scr Heads 2`) are not read: nothing in the port grants them.
QUAKE_SCREAM_LIMITATIONS = [
    'Level 1 only: quakeLevel or screamLevel 2 plays as level 1.',
    'Shaman Stone and Spell Twister read only through MP Cost; the charm damages of '
    'Q Fall Damage, Q Slam and Scr Heads are not applied.',
    'The Knight\'s Quake and Scream clips and the Q Slam, Q Trail, Q Pillar and Scr Heads '
    'art and sounds are not cooked; the guest stands in existing clips and effects.',
]

def _knight_child(source, file, knight, path):
    """The GameObject at `path` (names) under the Knight, with its Transform."""
    go = knight
    for name in path:
        transform = next(source.read(source.ref(file, c['component'])) for c in go['m_Component']
                         if source.ref(file, c['component']).type.name == 'Transform')
        for child in transform['m_Children']:
            t = source.read(source.ref(file, child))
            g = source.read(source.ref(file, t['m_GameObject']))
            if g['m_Name'] == name:
                go = g
                break
        else:
            raise LookupError(f'no {name} under {go["m_Name"]}')
    transform = next(source.read(source.ref(file, c['component'])) for c in go['m_Component']
                     if source.ref(file, c['component']).type.name == 'Transform')
    if transform['m_LocalRotation'] != {'x': 0.0, 'y': 0.0, 'z': 0.0, 'w': 1.0}:
        raise ValueError(f'rotated {path[-1]} unsupported')
    return go, transform

def _component(source, file, go, kind):
    for c in go['m_Component']:
        obj = source.ref(file, c['component'])
        if obj.type.name == kind or (obj.type.name == 'MonoBehaviour' and source.typename(obj) == kind):
            return obj
    raise LookupError(f'{go["m_Name"]} carries no {kind}')

def _damage(source, file, go):
    """`Set Damage` writes the no-charm damage into `damages_enemy` (its second write,
    behind the equippedCharm_19 test, is Shaman Stone's); without that FSM the
    serialized `damageDealt` stands."""
    fsm = None
    for c in go['m_Component']:
        obj = source.ref(file, c['component'])
        if obj.type.name == 'MonoBehaviour' and source.typename(obj) == 'PlayMakerFSM':
            f = source.read(obj)['fsm']
            if f['name'] == 'Set Damage':
                fsm = f
            elif f['name'] == 'damages_enemy':
                base = fsm_variables(f)
                assert base['attackType'] == 2, f'{go["m_Name"]} is no spell'
    _, _, scalar, actions = _reader(fsm)
    writes = actions('Set Damage', 'SetFsmInt')
    assert len(writes) == 2 and all(scalar(w['variableName']) == 'damageDealt' for w in writes), writes
    return scalar(writes[0]['setValue']), base['direction']

def _polygon(source, file, go):
    c = source.read(_component(source, file, go, 'PolygonCollider2D'))
    paths = c['m_Points']['m_Paths']
    assert len(paths) == 1 and 3 <= len(paths[0]) <= 16, f'{go["m_Name"]} polygon budget'
    off = c['m_Offset']
    return [(p['x'] + off['x'], p['y'] + off['y']) for p in paths[0]]

def _hit_parent(source, file, knight, name, children):
    """A `Spells` child whose `Hit Box Control` faces it the same way whichever way the
    Knight faces (`Init Scale`/`Neg Scale`): its hits are world-fixed. Returns the
    parent's Knight-local position, each child's polygon in the parent's world
    orientation (scale applied), damage and direction, and the clip the FSM times
    the hits by."""
    go, t = _knight_child(source, file, knight, ('Spells', name))
    control = next(source.read(source.ref(file, c['component']))['fsm'] for c in go['m_Component']
                   if source.ref(file, c['component']).type.name == 'MonoBehaviour'
                   and source.typename(source.ref(file, c['component'])) == 'PlayMakerFSM'
                   and source.read(source.ref(file, c['component']))['fsm']['name'] == 'Hit Box Control')
    states, variables, scalar, actions = _reader(control)
    assert {'Neg', 'Pos', 'Activate', 'Deactivate Hit'} <= set(states) and 'Neg Scale' in variables
    played = actions('Init', 'Tk2dPlayAnimationWithEvents')
    assert len(played) == 1 and played[0]['animationTriggerEvent'] == 'FINISHED'
    # Init waits for the clip's first trigger, Activate switches the hits on until the next.
    assert [tr['toState'] for tr in states['Activate']['transitions']] == ['Deactivate Hit']
    animator = source.read(_component(source, file, go, 'tk2dSpriteAnimator'))
    library = source.read(source.ref(file, animator['library']))
    clip = next(c for c in library['clips'] if c['name'] == scalar(played[0]['clipName']))
    triggers = [i for i, f in enumerate(clip['frames']) if f.get('triggerEvent')]
    s = t['m_LocalScale']
    assert s['x'] == s['y'] > 0, f'{name} scale'
    hits = {}
    for child in children:
        c, ct = _knight_child(source, file, knight, ('Spells', name, child))
        damage, direction = _damage(source, file, c)
        k = ct['m_LocalScale']
        mirror = k['x'] < 0
        assert abs(abs(k['x']) - k['y']) < 1e-6
        pos = ct['m_LocalPosition']
        poly = [((pos['x'] + abs(k['x']) * x) * s['x'], (pos['y'] + k['y'] * y) * s['y'])
                for x, y in _polygon(source, file, c)]
        hits[child] = {'polygon': poly, 'mirror': mirror, 'damage': damage, 'direction': direction}
    return {'at': (t['m_LocalPosition']['x'], t['m_LocalPosition']['y']), 'hits': hits,
            'from_seconds': triggers[0] / clip['fps'], 'to_seconds': triggers[1] / clip['fps'],
            'clip': clip['name']}

def source_quake_scream_values(source):
    from dream_nail import _knight_animation
    file = source.file('resources.assets')
    control = _fsm(source, 'Spell Control')
    states, variables, scalar, actions = _reader(control)
    hero_obj = next(o for o in file.objects.values()
                    if o.type.name == 'MonoBehaviour' and source.typename(o) == 'HeroController')
    hero = source.read(hero_obj)
    knight = source.read(file.objects[hero_obj.parse_monobehaviour_head().m_GameObject.m_PathID])
    # Spell Choice: up casts Wraiths, down casts Dive, either falling back to the fireball.
    choice = actions('Spell Choice', 'BoolTest')
    assert [(c['boolVariable']['name'], c.get('isTrue')) for c in choice] == \
        [('Pressed Up', 'SCREAM'), ('Pressed Down', 'QUAKE')]
    for state, level in (('Has Quake?', 'quakeLevel'), ('Has Scream?', 'screamLevel')):
        read = actions(state, 'GetPlayerDataInt')
        assert len(read) == 1 and scalar(read[0]['intName']) == level
        assert {t['fsmEvent']['name']: t['toState'] for t in states[state]['transitions']}['CANCEL'] == 'Has Fireball?'
    clips = {c['name']: len(c['frames']) / c['fps'] for c in _knight_animation(source)['clips']}
    def clip(state, name):
        played = actions(state, 'Tk2dPlayAnimationWithEvents')
        assert len(played) == 1 and scalar(played[0]['clipName']) == name, f'{state} clip moved'
        return clips[name]
    def wait(state):
        w = actions(state, 'Wait')
        assert len(w) == 1
        return scalar(w[0]['time'])
    # Quake Antic holds `Quake Antic Speed` (Q On Ground's 11, Q Off Ground's 0) every
    # frame and DecelerateV2 scales it by its factor every physics step after.
    rise = actions('Q On Ground', 'SetFloatValue')
    assert len(rise) == 1 and rise[0]['floatVariable']['name'] == 'Quake Antic Speed'
    decel = actions('Quake Antic', 'DecelerateV2')
    assert len(decel) == 1
    down = actions('Quake1 Down', 'SetVelocity2d')
    assert len(down) == 1 and down[0]['everyFrame']
    # Quake Finish ends the dive's HAZARD_ONLY damage mode and starts parryInvulnTimer.
    finish = [scalar(a['functionCall']['FunctionName']) for a in actions('Quake Finish', 'SendMessage')]
    assert finish == ['QuakeInvuln'], finish
    fall_go, fall_t = _knight_child(source, file, knight, ('Spells', 'Q Fall Damage'))
    box = source.read(_component(source, file, fall_go, 'BoxCollider2D'))
    # Centred on the Knight, so the box needs no mirroring with his facing.
    assert fall_t['m_LocalPosition']['x'] == 0 and fall_t['m_LocalScale']['x'] == 1 and box['m_Offset']['x'] == 0
    fall_damage, _ = _damage(source, file, fall_go)
    quake = {
        'antic_seconds': clip('Quake Antic', 'Quake Antic'),
        'antic_rise': scalar(rise[0]['floatValue']) * scalar(decel[0]['deceleration']),
        'fall_speed': -scalar(down[0]['y']),
        # Quake1 Land ends on whichever comes first, its clip or its Wait.
        'land_seconds': min(clip('Quake1 Land', 'Quake Land'), wait('Quake1 Land')),
        'invuln_seconds': hero['INVUL_TIME_QUAKE'],
        'fall_damage': fall_damage,
        'fall_box': {'offset': box['m_Offset'], 'size': box['m_Size']},
        'slam': _hit_parent(source, file, knight, 'Q Slam', ('Hit L', 'Hit R')),
    }
    scream = {
        'antic_seconds': clip('Scream Antic1', 'Scream Start'),
        'burst_seconds': wait('Scream Burst 1'),
        'roar_seconds': wait('End Roar'),
        'end_seconds': clip('Scream End', 'Scream End'),
        'heads': _hit_parent(source, file, knight, 'Scr Heads', ('Hit L', 'Hit R', 'Hit U')),
    }
    for hits in (quake['slam']['hits'], scream['heads']['hits']):
        # Hit R is Hit L at scale x -1, so one polygon serves both sides (to the Q16 unit:
        # Q Slam's two children sit 1e-7 apart in y).
        same = all(abs(a - b) < 1 / 65536 for p, q in zip(hits['Hit R']['polygon'], hits['Hit L']['polygon'])
                   for a, b in zip(p, q))
        assert same and hits['Hit R']['mirror'] and not hits['Hit L']['mirror']
        assert hits['Hit R']['damage'] == hits['Hit L']['damage']
    assert scream['heads']['hits']['Hit U']['damage'] == scream['heads']['hits']['Hit L']['damage']
    return {'quake': quake, 'scream': scream, 'limitations': QUAKE_SCREAM_LIMITATIONS}

def generated_quake_scream_params(values):
    def ticks(seconds):
        return max(1, round(seconds * 60))
    def q16(v):
        return round(v * 65536)
    def poly(points):
        return '&[' + ','.join(f'[{q16(x)},{q16(y)}]' for x, y in points) + ']'
    q, s = values['quake'], values['scream']
    slam, heads = q['slam'], s['heads']
    box = q['fall_box']
    hw, hh = box['size']['x'] / 2, box['size']['y'] / 2
    fall = (box['offset']['x'] - hw, box['offset']['y'] - hh, box['offset']['x'] + hw, box['offset']['y'] + hh)
    out = 'pub const QUAKE_PARAMS: hk_sim::QuakeParams = hk_sim::QuakeParams {'
    out += ','.join(f'{k}:{v}' for k, v in (
        ('antic_ticks', ticks(q['antic_seconds'])),
        ('antic_rise', q16(q['antic_rise'])),
        ('fall_speed', q16(q['fall_speed'])),
        ('land_ticks', ticks(q['land_seconds'])),
        ('invuln_ticks', ticks(q['invuln_seconds'])),
        ('fall_damage', q['fall_damage']),
        ('fall_bounds', '[' + ','.join(str(q16(v)) for v in fall) + ']'),
        ('slam_damage', slam['hits']['Hit L']['damage']),
        ('slam_from', ticks(slam['from_seconds'])),
        ('slam_to', ticks(slam['to_seconds'])),
        ('slam_at', f"[{q16(slam['at'][0])},{q16(slam['at'][1])}]"),
        ('slam', poly(slam['hits']['Hit L']['polygon'])),
    )) + '};\n'
    out += 'pub const SCREAM_PARAMS: hk_sim::ScreamParams = hk_sim::ScreamParams {'
    out += ','.join(f'{k}:{v}' for k, v in (
        ('antic_ticks', ticks(s['antic_seconds'])),
        ('cast_ticks', ticks(s['burst_seconds']) + ticks(s['roar_seconds']) + ticks(s['end_seconds'])),
        ('hit_from', ticks(heads['from_seconds'])),
        ('hit_to', ticks(heads['to_seconds'])),
        ('damage', heads['hits']['Hit L']['damage']),
        ('at', f"[{q16(heads['at'][0])},{q16(heads['at'][1])}]"),
        ('sides', poly(heads['hits']['Hit L']['polygon'])),
        ('up', poly(heads['hits']['Hit U']['polygon'])),
    )) + '};\n'
    return out
