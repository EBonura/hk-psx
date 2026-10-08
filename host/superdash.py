"""Read the Crystal Heart's timings from the Hero's Superdash PlayMaker FSM.

HeroController carries a SUPER_DASH_SPEED field, but the FSM never reads it:
the travel velocity comes from the FSM's own `Superdash Speed` variable, so
that is what is bound here. No retail payload is embedded; the caller appends
the returned scalars to the guest's cooked parameters.
"""
from focus import action_fields, fsm_variables

def _fsm(source, name):
    file = source.file('resources.assets')
    hero = next(o for o in file.objects.values()
                if o.type.name == 'MonoBehaviour' and source.typename(o) == 'HeroController')
    gid = hero.parse_monobehaviour_head().m_GameObject.m_PathID
    for o in file.objects.values():
        if o.type.name != 'MonoBehaviour' or source.typename(o) != 'PlayMakerFSM':
            continue
        if o.parse_monobehaviour_head().m_GameObject.m_PathID != gid:
            continue
        fsm = source.read(o)['fsm']
        if fsm['name'] == name:
            return fsm
    raise LookupError(f'no {name} FSM on the Hero')

def source_superdash_values(source):
    fsm = _fsm(source, 'Superdash')
    states = {s['name']: s for s in fsm['states']}
    variables = fsm_variables(fsm)
    def actions(state, kind):
        d = states[state]['actionData']
        return [action_fields(d, i) for i, n in enumerate(d['actionNames'])
                if n.rsplit('.', 1)[-1] == kind and d['actionEnabled'][i]]
    def scalar(value):
        return variables[value['name']] if value['useVariable'] else value['value']
    # Both charges wait on the same variable, and both directions read the same
    # speed, so bind one of each and assert the other agrees.
    charge = {scalar(a['time']) for state in ('Ground Charge', 'Wall Charge') for a in actions(state, 'Wait')}
    assert len(charge) == 1, f'the two charge states disagree: {charge}'
    speed = actions('Right', 'SetFloatValue')[0]
    assert speed['floatVariable']['name'] == 'Current SD Speed'
    assert speed['floatValue']['name'] == 'Superdash Speed'
    # `Superdash Speed neg` is not serialized with a value: Init copies the
    # speed into it and multiplies by -1, so the two directions are symmetric.
    negative = actions('Left', 'SetFloatValue')[0]
    assert negative['floatValue']['name'] == 'Superdash Speed neg'
    mirror = actions('Init', 'SetFloatValue') + actions('Init', 'FloatMultiply')
    assert any(a.get('floatVariable', {}).get('name') == 'Superdash Speed neg'
               and a.get('floatValue', {}).get('name') == 'Superdash Speed' for a in mirror)
    assert any(a.get('floatVariable', {}).get('name') == 'Superdash Speed neg'
               and scalar(a['multiplyBy']) == -1.0 for a in mirror if 'multiplyBy' in a)
    cancelable = actions('Dashing', 'Wait')
    assert len(cancelable) == 1 and not cancelable[0]['realTime']
    recover = actions('Hit Wall', 'Wait')
    assert len(recover) == 1
    return {'speed': scalar(speed['floatValue']), 'charge': charge.pop(),
            'cancelable': scalar(cancelable[0]['time']), 'recover': scalar(recover[0]['time']),
            'source_states': list(states),
            'limitations': ['Cancelable state NORM CANCEL (jump, dash or attack out of the travel) '
                            'is not wired until the attack integration',
                            'SLOPE CANCEL, the Zero Timer that ends a travel stalled against a slope, '
                            'is not modelled',
                            'Charge, blast, trail and Hit Wall presentation are not cooked']}
