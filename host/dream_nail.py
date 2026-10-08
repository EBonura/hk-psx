"""The Dream Nail's own timing, hitbox and reward, from the Hero's FSM.

P14 step 5 asks for the Dream Nail's separate target, resource and state-change
rules rather than another nail swing, and the source keeps them in three
places: the Hero's `Dream Nail` PlayMaker FSM drives the states, each state's
length is its tk2d clip rather than a serialized time, and the reward lives on
the target's `EnemyDreamnailReaction`. No retail payload is embedded; the
report goes to .hkpsx.

Dream Gate (setting a gate, warping to it, the essence cost and the Godhome
branches) is deliberately not read here: none of it is reachable in the
admitted scenes, and it would need essence and scene warping the port has not
built.
"""
import math
from superdash import _fsm
from focus import action_fields

CLIPS = ('DN Start', 'DN Charge', 'DN Slash Antic', 'DN Slash')

def _knight_animation(source):
    file = source.file('resources.assets')
    for o in file.objects.values():
        if o.type.name != 'MonoBehaviour' or source.typename(o) != 'tk2dSpriteAnimation':
            continue
        table = source.read(o)
        names = {c['name'] for c in table['clips']}
        if {'Idle', 'Run', 'Slash', 'Airborne', 'Focus'}.issubset(names):
            return table
    raise LookupError('no Knight animation table')

def _hitbox(source, hero_gid):
    """The `Dream Effects > Hitbox` polygon, in Knight-local units."""
    file = source.file('resources.assets')
    for o in file.objects.values():
        if o.type.name != 'GameObject':
            continue
        go = source.read(o)
        if go['m_Name'] != 'Hitbox':
            continue
        components = [source.ref(file, c['component']) for c in go['m_Component']]
        collider = next((c for c in components if c.type.name == 'PolygonCollider2D'), None)
        if collider is None:
            continue
        transform = next(c for c in components if c.type.name == 'Transform')
        t = source.read(transform)
        # Walk up to the Knight, refusing any ancestor that moves or scales the
        # polygon, exactly as the nail extraction does.
        chain, parent = [], t
        while parent['m_Father']['m_PathID']:
            parent = source.read(source.ref(file, parent['m_Father']))
            chain.append(parent)
            if parent['m_GameObject']['m_PathID'] == hero_gid:
                break
        if not chain or chain[-1]['m_GameObject']['m_PathID'] != hero_gid:
            continue
        for ancestor in chain[:-1]:
            if any(ancestor['m_LocalPosition'][k] != 0 or ancestor['m_LocalScale'][k] != 1 for k in 'xyz'):
                raise ValueError('non-identity Dream Nail ancestor unsupported')
        if t['m_LocalRotation'] != {'x': 0.0, 'y': 0.0, 'z': 0.0, 'w': 1.0}:
            raise ValueError('rotated Dream Nail hitbox unsupported')
        c = source.read(collider)
        paths = c['m_Points']['m_Paths']
        if len(paths) != 1 or not 3 <= len(paths[0]) <= 16:
            raise ValueError('Dream Nail polygon budget')
        pos, off, scale = t['m_LocalPosition'], c['m_Offset'], t['m_LocalScale']
        poly = [(pos['x'] + (p['x'] + off['x']) * scale['x'],
                 pos['y'] + (p['y'] + off['y']) * scale['y']) for p in paths[0]]
        if any(not math.isfinite(v) or abs(v) > 16 for point in poly for v in point):
            raise ValueError('Dream Nail local polygon exceeds Q16 bound')
        return {'collider': source.sid(collider), 'transform': source.sid(transform), 'polygon': poly}
    raise LookupError('no Dream Nail Hitbox under the Knight')

def source_dream_nail_values(source):
    file = source.file('resources.assets')
    hero = next(o for o in file.objects.values()
                if o.type.name == 'MonoBehaviour' and source.typename(o) == 'HeroController')
    hero_gid = hero.parse_monobehaviour_head().m_GameObject.m_PathID
    fsm = _fsm(source, 'Dream Nail')
    states = {s['name']: s for s in fsm['states']}
    # Every phase but the slash ends on its own clip finishing, through
    # Tk2dPlayAnimationWithEvents, so the clip lengths are the timings.
    for state, clip in (('Start', 'DN Start'), ('Charge', 'DN Charge'),
                        ('Slash Antic', 'DN Slash Antic'), ('Slash', 'DN Slash')):
        names = [n.rsplit('.', 1)[-1] for i, n in enumerate(states[state]['actionData']['actionNames'])
                 if states[state]['actionData']['actionEnabled'][i]]
        assert 'Tk2dPlayAnimationWithEvents' in names, f'{state} no longer ends on its clip'
    def actions(state, kind):
        d = states[state]['actionData']
        return [action_fields(d, i) for i, n in enumerate(d['actionNames'])
                if n.rsplit('.', 1)[-1] == kind and d['actionEnabled'][i]]
    # Releasing the button during the charge cancels it (ListenForDreamNail).
    assert actions('Charge', 'ListenForDreamNail'), 'the charge no longer watches the button'
    wait = actions('Slash', 'Wait')
    assert len(wait) == 1
    table = _knight_animation(source)
    clips = {c['name']: len(c['frames']) / c['fps'] for c in table['clips'] if c['name'] in CLIPS}
    missing = [c for c in CLIPS if c not in clips]
    assert not missing, f'Knight animation lost {missing}'
    return {
        'start': clips['DN Start'],
        'charge': clips['DN Charge'],
        'antic': clips['DN Slash Antic'],
        'slash': clips['DN Slash'],
        'cancelable_after': wait[0]['time']['value'],
        'hitbox': _hitbox(source, hero_gid),
        # EnemyDreamnailReaction::RecieveDreamImpact adds 33 MP charge, or 66
        # with Dream Wielder (charm 30). No charms, so 33.
        'soul': 33,
        'limitations': [
            'Dream Gate: setting, warping, the essence cost and the Godhome branches '
            'are not read or implemented.',
            'Dream dialogue is a target-supplied convo title; only the GENERIC set the '
            'admitted enemies carry is bound.',
            'The charge, slash and impact presentation is not cooked.',
        ],
    }

def generated_dream_nail_params(values):
    def ticks(seconds):
        return max(1, round(seconds * 60))
    out = 'pub const DREAM_NAIL_PARAMS: hk_sim::DreamNailParams = hk_sim::DreamNailParams {'
    out += ','.join(f'{k}:{v}' for k, v in (
        ('start_ticks', ticks(values['start'])),
        ('charge_ticks', ticks(values['charge'])),
        ('antic_ticks', ticks(values['antic'])),
        ('slash_ticks', ticks(values['slash'])),
        ('cancelable_ticks', ticks(values['cancelable_after'])),
        ('soul', values['soul']),
    )) + '};\n'
    poly = values['hitbox']['polygon']
    out += 'pub const DREAM_NAIL_POLYGON: &[[i32;2]] = &['
    out += ','.join(f'[{round(x * 65536)},{round(y * 65536)}]' for x, y in poly) + '];\n'
    return out
