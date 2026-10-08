"""Spell definitions from the Hero's `Spell Control` FSM and the spell prefabs.

P15 step 1 wants the spell catalog taken from the source rather than assumed,
and the source spreads one spell across three FSMs: `Spell Control` on the Hero
decides what is cast and pays for it, the spawned `Fireball Cast` object aims
and launches, and the `Fireball`/`damages_enemy` pair on the projectile carries
its damage, lifetime and wall behaviour. All three are read here.

Only Vengeful Spirit is bound so far. Desolate Dive and Howling Wraiths are
catalogued with their gates, since neither is reachable in the admitted scenes.
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
