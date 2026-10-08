"""Desolate Dive and Howling Wraiths, from the Hero's `Spell Control` FSM.

P15 step 3 catalogues the two spells host/spells.py left as names. They share
Vengeful Spirit's cast gate and its `MP Cost`, and then diverge: Quake drops the
Knight through a looping fall clip until the ground stops it, Scream is a fixed
sequence of four states. Neither has a projectile, so the damage lives on child
hitboxes of the Knight (`Q Fall Damage`, `Q Slam`, `Scr Heads`) whose windows
come from tk2d animation trigger frames rather than from any serialized time.

Nothing here is implemented in the guest. This is extraction and a report only;
no retail payload is embedded and the report goes to .hkpsx.
"""
from superdash import _fsm
from focus import action_fields, fsm_variables
from dream_nail import _knight_animation
from spells import _named_child, _fsm_on

# tk2dSpriteAnimationClip.WrapMode. A looping clip is not a state length.
ONCE, LOOP, LOOP_SECTION = 2, 0, 1
CHARM_SHAMAN_STONE = 'equippedCharm_19'


def _reader(fsm):
    states = {s['name']: s for s in fsm['states']}
    variables = fsm_variables(fsm)
    def scalar(value):
        if isinstance(value, dict) and 'value' in value and 'useVariable' in value:
            return variables.get(value['name']) if value['useVariable'] else value['value']
        return value
    def actions(state, kind):
        d = states[state]['actionData']
        return [action_fields(d, i) for i, n in enumerate(d['actionNames'])
                if n.rsplit('.', 1)[-1] == kind and d['actionEnabled'][i]]
    return states, variables, scalar, actions


def _transitions(state):
    return {t['fsmEvent']['name']: t['toState'] for t in state['transitions']}


def _hero_gid(source, file):
    hero = next(o for o in file.objects.values()
                if o.type.name == 'MonoBehaviour' and source.typename(o) == 'HeroController')
    return hero.parse_monobehaviour_head().m_GameObject.m_PathID


def _transform(source, file, gid):
    for component in source.read(file.objects[gid])['m_Component']:
        obj = source.ref(file, component['component'])
        if obj.type.name == 'Transform':
            return source.read(obj)
    raise LookupError(f'{gid} carries no Transform')


def _placement(source, file, gid, hero_gid):
    """One child's offset and scale in Knight-local units.

    Every spell hitbox hangs off the Knight's `Spells` folder, so the chain is
    short; a rotated or non-identity intermediate would silently move a polygon
    and is refused rather than approximated.
    """
    offset, scale, chain = [0.0, 0.0], [1.0, 1.0], []
    transform = _transform(source, file, gid)
    while True:
        if transform['m_LocalRotation'] != {'x': 0.0, 'y': 0.0, 'z': 0.0, 'w': 1.0}:
            raise ValueError('rotated spell hitbox ancestor unsupported')
        owner = source.read(file.objects[transform['m_GameObject']['m_PathID']])['m_Name']
        chain.append(owner)
        if transform['m_GameObject']['m_PathID'] == hero_gid:
            return offset, scale, chain[::-1]
        for i, axis in enumerate('xy'):
            offset[i] = transform['m_LocalPosition'][axis] + offset[i] * transform['m_LocalScale'][axis]
            scale[i] *= transform['m_LocalScale'][axis]
        if not transform['m_Father']['m_PathID']:
            raise LookupError(f'{chain[0]} does not hang off the Knight')
        transform = source.read(source.ref(file, transform['m_Father']))


def _shape(source, file, gid, hero_gid):
    offset, scale, chain = _placement(source, file, gid, hero_gid)
    for component in source.read(file.objects[gid])['m_Component']:
        obj = source.ref(file, component['component'])
        if obj.type.name == 'BoxCollider2D':
            box = source.read(obj)
            half = [box['m_Size'][a] * abs(scale[i]) / 2 for i, a in enumerate('xy')]
            centre = [offset[i] + box['m_Offset'][a] * scale[i] for i, a in enumerate('xy')]
            return {'path': '/'.join(chain), 'kind': 'box',
                    'bounds': [centre[0] - half[0], centre[1] - half[1],
                               centre[0] + half[0], centre[1] + half[1]]}
        if obj.type.name == 'PolygonCollider2D':
            collider = source.read(obj)
            paths = collider['m_Points']['m_Paths']
            if len(paths) != 1 or not 3 <= len(paths[0]) <= 16:
                raise ValueError(f'{chain[-1]} polygon budget')
            poly = [(offset[0] + (p['x'] + collider['m_Offset']['x']) * scale[0],
                     offset[1] + (p['y'] + collider['m_Offset']['y']) * scale[1]) for p in paths[0]]
            return {'path': '/'.join(chain), 'kind': 'polygon', 'polygon': poly}
    raise LookupError(f'{chain[-1]} carries no 2D collider')


def _damage(source, file, gid):
    """What a spell hitbox deals, from its `Set Damage` and `damages_enemy` pair.

    `damages_enemy` carries a serialized default, but `Set Damage` overwrites it
    on enable: the first write is the no-charm damage, the second is taken only
    behind a Shaman Stone test, exactly as the Fireball's is.
    """
    prefab = source.read(file.objects[gid])
    setter = _fsm_on(source, file, prefab, 'Set Damage')
    _, _, scalar, actions = _reader(setter)
    writes = actions('Set Damage', 'SetFsmInt')
    assert len(writes) == 2, f'{prefab["m_Name"]} Set Damage no longer has a plain and a charmed value'
    assert all(scalar(w['fsmName']) == 'damages_enemy' and scalar(w['variableName']) == 'damageDealt'
               for w in writes), f'{prefab["m_Name"]} Set Damage no longer targets damages_enemy'
    gate = actions('Set Damage', 'PlayerDataBoolTest')
    assert len(gate) == 1 and scalar(gate[0]['boolName']) == CHARM_SHAMAN_STONE, \
        f'{prefab["m_Name"]} charm branch is no longer Shaman Stone'
    assert gate[0]['isFalse'] == 'FINISHED', f'{prefab["m_Name"]} charm branch no longer skips the second write'
    hitter = _fsm_on(source, file, prefab, 'damages_enemy')
    _, hit_vars, _, _ = _reader(hitter)
    return {'damage': scalar(writes[0]['setValue']), 'shaman_stone': scalar(writes[1]['setValue']),
            'attack_type': hit_vars['attackType'], 'direction': hit_vars['direction'],
            'serialized_default': hit_vars['damageDealt']}


def _child(source, file, gid, name):
    transform = _transform(source, file, gid)
    found = []
    for ref in transform['m_Children']:
        child = source.read(source.ref(file, ref))['m_GameObject']['m_PathID']
        if source.read(file.objects[child])['m_Name'] == name:
            found.append(child)
    assert len(found) == 1, f'{name} is no longer a single child'
    return found[0]


def _hit_window(source, file, gid):
    """How long a hitbox stays on, from its own tk2d clip's trigger frames.

    `Hit Box Control` plays one clip, turns the children on at its first trigger
    frame and off at the second, so the window is the gap between them.
    """
    prefab = source.read(file.objects[gid])
    control = _fsm_on(source, file, prefab, 'Hit Box Control')
    states, _, scalar, actions = _reader(control)
    played = actions('Init', 'Tk2dPlayAnimationWithEvents')
    assert len(played) == 1 and played[0]['animationTriggerEvent'] == 'FINISHED', \
        f'{prefab["m_Name"]} no longer drives its hitbox from animation triggers'
    clip_name = scalar(played[0]['clipName'])
    on = actions('Activate', 'ActivateAllChildren')
    off = actions('Deactivate Hit', 'ActivateAllChildren')
    assert len(on) == 1 and scalar(on[0]['activate']), f'{prefab["m_Name"]} Activate no longer enables its hits'
    assert len(off) == 1 and not scalar(off[0]['activate']), f'{prefab["m_Name"]} Deactivate Hit no longer disables them'
    assert _transitions(states['Activate'])['FINISHED'] == 'Deactivate Hit'

    animator = next(source.read(source.ref(file, c['component'])) for c in prefab['m_Component']
                    if source.typename(source.ref(file, c['component'])) == 'tk2dSpriteAnimator')
    table = source.read(source.ref(file, animator['library']))
    clip = next(c for c in table['clips'] if c['name'] == clip_name)
    triggers = [i for i, frame in enumerate(clip['frames']) if frame['triggerEvent']]
    assert len(triggers) == 2, f'{clip_name} no longer carries exactly two trigger frames: {triggers}'
    return {'clip': clip_name, 'clip_seconds': len(clip['frames']) / clip['fps'],
            'starts_at': triggers[0] / clip['fps'], 'ends_at': triggers[1] / clip['fps'],
            'active_seconds': (triggers[1] - triggers[0]) / clip['fps']}


def _hitbox(source, file, gid, hero_gid, children=()):
    """One spell hitbox: where it is, what it deals and when it is on."""
    out = {'name': source.read(file.objects[gid])['m_Name']}
    if children:
        out['window'] = _hit_window(source, file, gid)
        out['hits'] = []
        for name in children:
            child = _child(source, file, gid, name)
            out['hits'].append(dict(_shape(source, file, child, hero_gid), name=name,
                                    **_damage(source, file, child)))
    else:
        out.update(_shape(source, file, gid, hero_gid))
        out.update(_damage(source, file, gid))
    return out


def _knight_child(source, file, name, hero_gid):
    found = []
    for o in file.objects.values():
        if o.type.name != 'GameObject' or source.read(o)['m_Name'] != name:
            continue
        try:
            _placement(source, file, o.path_id, hero_gid)
        except (LookupError, ValueError):
            continue
        found.append(o.path_id)
    assert len(found) == 1, f'{name} is no longer a single object under the Knight'
    return found[0]


def source_quake_scream_values(source):
    file = source.file('resources.assets')
    hero_gid = _hero_gid(source, file)
    control = _fsm(source, 'Spell Control')
    states, variables, scalar, actions = _reader(control)
    cost = variables['MP Cost']

    # Held direction picks the spell: up is Scream, down is Quake, neither is
    # the Fireball host/spells.py already binds.
    choice = actions('Spell Choice', 'BoolTest')
    assert len(choice) == 2, 'Spell Choice no longer tests two held directions'
    assert choice[0]['boolVariable']['name'] == 'Pressed Up' and choice[0]['isTrue'] == 'SCREAM'
    assert choice[1]['boolVariable']['name'] == 'Pressed Down' and choice[1]['isTrue'] == 'QUAKE'
    assert choice[1]['isFalse'] == 'FIREBALL', 'the fallback is no longer the Fireball'
    branch = _transitions(states['Spell Choice'])
    assert branch['QUAKE'] == 'Has Quake?' and branch['SCREAM'] == 'Has Scream?'

    clips = {c['name']: c for c in _knight_animation(source)['clips']}
    def clip_seconds(name, wrap):
        clip = clips.get(name)
        assert clip is not None, f'the Knight lost the {name} clip'
        assert clip['wrapMode'] == wrap, f'{name} changed wrap mode to {clip["wrapMode"]}'
        return len(clip['frames']) / clip['fps']

    def played(state, name, complete):
        actual = actions(state, 'Tk2dPlayAnimationWithEvents')
        assert len(actual) == 1 and scalar(actual[0]['clipName']) == name, f'{state} clip moved'
        assert actual[0]['animationCompleteEvent'] == complete, f'{state} no longer ends on {complete or "its own clip"}'

    def gate(state, field, cast_to):
        level = actions(state, 'GetPlayerDataInt')
        assert len(level) == 1 and scalar(level[0]['intName']) == field, f'{state} no longer reads {field}'
        assert level[0]['storeValue']['name'] == 'Spell Level'
        compare = actions(state, 'IntCompare')
        assert len(compare) == 1 and scalar(compare[0]['integer2']) == 0, f'{state} gate is no longer > 0'
        assert compare[0]['greaterThan'] == 'CAST' and compare[0]['lessThan'] == compare[0]['equal'] == 'CANCEL'
        assert _transitions(states[state])['CAST'] == cast_to
        return f'{field} > 0'

    def takes_mp(state):
        paid = [a for a in actions(state, 'SendMessage')
                if a['functionCall']['FunctionName'] == 'TakeMP']
        assert len(paid) == 1, f'{state} no longer pays for the spell'
        call = paid[0]['functionCall']
        assert call['parameterType'] == 'int' and call['IntParameter']['name'] == 'MP Cost', \
            f'{state} no longer pays MP Cost'

    def level_one(state, to_state):
        compare = actions(state, 'IntCompare')
        assert len(compare) == 1 and scalar(compare[0]['integer2']) == 1, f'{state} no longer splits at level 1'
        assert compare[0]['lessThan'] == compare[0]['equal'] == 'LEVEL 1'
        assert _transitions(states[state])['LEVEL 1'] == to_state

    # ---- Desolate Dive ------------------------------------------------------
    quake_gate = gate('Has Quake?', 'quakeLevel', 'On Ground?')
    launch = {}
    for state, grounded in (('Q On Ground', True), ('Q Off Ground', False)):
        speed = actions(state, 'SetFloatValue')
        assert len(speed) == 1 and speed[0]['floatVariable']['name'] == 'Quake Antic Speed', f'{state} moved'
        launch['grounded' if grounded else 'airborne'] = scalar(speed[0]['floatValue'])
    on_ground = actions('On Ground?', 'BoolTest')
    assert len(on_ground) == 1 and on_ground[0]['boolVariable']['name'] == 'On Ground'
    assert on_ground[0]['isTrue'] == 'ON GROUND' and on_ground[0]['isFalse'] == 'OFF GROUND'

    played('Quake Antic', 'Quake Antic', 'ANIM END')
    assert _transitions(states['Quake Antic'])['ANIM END'] == 'Level Check 2'
    antic_velocity = actions('Quake Antic', 'SetVelocity2d')
    assert len(antic_velocity) == 1 and antic_velocity[0]['y']['name'] == 'Quake Antic Speed'
    decelerate = actions('Quake Antic', 'DecelerateV2')
    assert len(decelerate) == 1, 'the antic no longer bleeds off horizontal speed'
    takes_mp('Level Check 2')
    level_one('Level Check 2', 'Q1 Effect')
    assert _transitions(states['Q1 Effect'])['FINISHED'] == 'Quake1 Down'

    # The fall clip loops, so the state ends on the ground, not on the clip:
    # a FloatCompare watches the Y speed settle inside its own tolerance.
    played('Quake1 Down', 'Quake Fall', '')
    fall = actions('Quake1 Down', 'SetVelocity2d')
    assert len(fall) == 1 and scalar(fall[0]['x']) == 0 and fall[0]['everyFrame'], 'the fall no longer holds a velocity'
    fall_speed = scalar(fall[0]['y'])
    assert fall_speed < 0, f'the dive no longer falls: {fall_speed}'
    landed = actions('Quake1 Down', 'FloatCompare')
    assert len(landed) == 1 and landed[0]['float1']['name'] == 'Y Speed' and landed[0]['equal'] == 'HERO LANDED'
    assert _transitions(states['Quake1 Down'])['HERO LANDED'] == 'Quake1 Land'

    played('Quake1 Land', 'Quake Land', 'FINISHED')
    land_wait = actions('Quake1 Land', 'Wait')
    assert len(land_wait) == 1 and not land_wait[0]['realTime']
    land_clip = clip_seconds('Quake Land', ONCE)
    assert abs(scalar(land_wait[0]['time']) - land_clip) < 1e-6, \
        f'the land clip {land_clip} and its Wait {scalar(land_wait[0]["time"])} no longer agree'

    quake_objects = {}
    for state in ('Quake1 Down', 'Quake1 Land'):
        for a in actions(state, 'ActivateGameObject'):
            quake_objects.setdefault(a['gameObject']['gameObject']['name'], []).append(
                (state, bool(scalar(a['activate']))))

    fall_box = _knight_child(source, file, 'Q Fall Damage', hero_gid)
    slam = _knight_child(source, file, 'Q Slam', hero_gid)

    # ---- Howling Wraiths ----------------------------------------------------
    scream_gate = gate('Has Scream?', 'screamLevel', 'Scream Get?')
    assert _transitions(states['Scream Get?'])['FINISHED'] == 'Level Check 3', 'the Scream pickup branch moved'
    level_one('Level Check 3', 'Scream Antic1')
    played('Scream Antic1', 'Scream Start', 'FINISHED')
    assert _transitions(states['Scream Antic1'])['FINISHED'] == 'Scream Burst 1'

    # The burst clip loops its section, so the Wait is the burst length.
    played('Scream Burst 1', 'Scream', '')
    burst_wait = actions('Scream Burst 1', 'Wait')
    assert len(burst_wait) == 1 and not burst_wait[0]['realTime'], 'the burst no longer ends on a Wait'
    takes_mp('Scream Burst 1')
    roar_wait = actions('End Roar', 'Wait')
    assert len(roar_wait) == 1, 'End Roar no longer waits'
    played('Scream End', 'Scream End', 'FINISHED')
    assert _transitions(states['Scream End'])['FINISHED'] == 'Spell End'

    wave = _named_child(source, file, states['Scream Burst 1'], 'Roar Wave Emitter Scream')
    emitter = _fsm_on(source, file, wave, 'emitter')
    emitter_states, _, emitter_scalar, emitter_actions = _reader(emitter)
    end_wait = emitter_actions('End', 'Wait')
    assert len(end_wait) == 1 and _transitions(emitter_states['End'])['FINISHED'] == 'Destroy', \
        'the roar wave no longer destroys itself after a wait'
    wave_gid = source.read(source.ref(file, wave['m_Component'][0]['component']))['m_GameObject']['m_PathID']
    heads = _knight_child(source, file, 'Scr Heads', hero_gid)

    antic = clip_seconds('Quake Antic', ONCE)
    scream_antic = clip_seconds('Scream Start', ONCE)
    scream_burst = scalar(burst_wait[0]['time'])
    scream_roar = scalar(roar_wait[0]['time'])
    scream_tail = clip_seconds('Scream End', ONCE)
    return {
        'cost': cost,
        'choice': {'up': 'scream', 'down': 'quake', 'neither': 'fireball'},
        'quake': {
            'gate': quake_gate,
            'antic_seconds': antic,
            'antic_launch': launch,
            'antic_decelerate': scalar(decelerate[0]['deceleration']),
            'fall_clip_seconds': clip_seconds('Quake Fall', LOOP),
            'fall_speed': -fall_speed,
            'land_condition': f'|Y Speed| <= {scalar(landed[0]["tolerance"])}',
            'land_seconds': land_clip,
            'paid_in': 'Level Check 2, after the antic',
            'hitboxes': [_hitbox(source, file, fall_box, hero_gid),
                         _hitbox(source, file, slam, hero_gid, ('Hit L', 'Hit R'))],
            'activates': quake_objects,
        },
        'scream': {
            'gate': scream_gate,
            'antic_seconds': scream_antic,
            'burst_seconds': scream_burst,
            'roar_seconds': scream_roar,
            'end_seconds': scream_tail,
            'total_seconds': scream_antic + scream_burst + scream_roar + scream_tail,
            'paid_in': 'Scream Burst 1, on the burst',
            'hitboxes': [_hitbox(source, file, heads, hero_gid, ('Hit L', 'Hit R', 'Hit U'))],
            'spawns': {'prefab': wave['m_Name'], 'source': source.sid(file.objects[wave_gid]),
                       'ended_by': 'END, sent from End Roar and Scream End',
                       'destroyed_after': emitter_scalar(end_wait[0]['time'])},
        },
        'limitations': [
            'Neither spell is implemented in the guest: this is the catalog P15 step 3 asks '
            'for, not a binding.',
            'Only level 1 of each is read. Descending Dark (quakeLevel 2) and Abyss Shriek '
            '(screamLevel 2) run through the Quake2/Scream Antic2 states and are not extracted.',
            'The Quake fall has no fixed length: it ends when the ground stops the Knight, so '
            'the dive duration depends on the drop.',
            'Shaman Stone damage is read but not applied, since charms do not exist yet.',
            'Every effect object (Q Charge, Q Trail, Q Orbs, the pillar, the flashes, the roar '
            'wave sprites) is named but none of it is cooked.',
            'The ENTER QUAKE global transition, which drops the Knight from a scene script '
            'rather than from a cast, is not read.',
        ],
    }


if __name__ == '__main__':
    from source import ROOT, Source, dump
    s = Source()
    out = source_quake_scream_values(s)
    dump(ROOT / '.hkpsx/quake-scream.json', out)
    q, sc = out['quake'], out['scream']
    print(f"both cost {out['cost']} MP; up casts Scream, down casts Quake")
    print(f"Quake: gate {q['gate']}, antic {q['antic_seconds']}s "
          f"(launch {q['antic_launch']['grounded']} grounded / {q['antic_launch']['airborne']} airborne, "
          f"decelerate {q['antic_decelerate']}), fall {q['fall_speed']}/s until {q['land_condition']}, "
          f"land {q['land_seconds']}s")
    for box in q['hitboxes']:
        for hit in box.get('hits', [box]):
            where = box['name'] if hit is box else f"{box['name']}/{hit['name']}"
            print(f"  {where} damage {hit['damage']} (Shaman Stone {hit['shaman_stone']})" +
                  (f", on for {box['window']['active_seconds']}s" if 'window' in box else ''))
    print(f"Scream: gate {sc['gate']}, antic {sc['antic_seconds']}s, burst {sc['burst_seconds']}s, "
          f"roar {sc['roar_seconds']}s, end {sc['end_seconds']}s, total {sc['total_seconds']}s")
    for box in sc['hitboxes']:
        for hit in box['hits']:
            print(f"  {box['name']}/{hit['name']} damage {hit['damage']} "
                  f"(Shaman Stone {hit['shaman_stone']}), on for {box['window']['active_seconds']}s")
    print(f"  spawns {sc['spawns']['prefab']}, destroyed {sc['spawns']['destroyed_after']}s after its end")
