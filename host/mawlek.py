"""Recover Brooding Mawlek (Crossroads_09) and its arena from the Windows source.

The boss controller lives in shared/hk-sim/src/mawlek.rs, the arena lifecycle
is shared/hk-sim/src/boss.rs's `Battle Control`, and this module is the source
side. It has two halves, because of where they sit in host/cook_inputs.txt:

* `recognize_placement` admits the one placed `Mawlek Body` into the guest
  actor pool. host/actors.py calls it during the region cook, so it is above
  the divider and kept small: the object's shape, its FSM set, and the wake
  box the guest watches. Every clip is `Dummy Blank` there, the way the False
  Knight's generic bank is `Blank`.
* host/mawlek_art.py, a postpass below the divider, reads everything else
  (every wait, speed, angle, count and box of `Mawlek Control`, `Mawlek Arm
  Control`, `Mawlek Head`, the Walker, the shot prefab, the corpse and
  `Battle Control`), asserts it against the constants the guest runs in
  shared/hk-sim/src/mawlek.rs, and cooks the art into this scene alone.
  Changing a number there re-runs the postpass and reuses every cooked region.

A state or child that is missing, or a HealthManager variant the guest does
not model, refuses the cook here rather than seating a half-understood boss.
"""
import hashlib

SCENE_NAME = 'Crossroads_09'
SCENE_FILE = 'level45'
BODY_NAME = 'Mawlek Body'
FSM_SET = {'Mawlek Control'}
CHILDREN = ('Dummy', 'Mawlek Arm R', 'Mawlek Arm L', 'Mawlek Head', 'Spit Effect', 'Alert Range New')
# The generic actor bank binds a one-frame clip for the two slots every
# ActorSpec carries; host/mawlek_art.py cooks the real art into its own bank.
ART_BINDINGS = {'walk': 'Dummy Blank', 'turn': 'Dummy Blank'}


def _fsms(sc, gid):
    from actors import _component_records
    return {data['fsm']['name']: data for _, kind, data in _component_records(sc, gid) if kind == 'PlayMakerFSM'}


def _children(sc, gid):
    tid = sc.go_transform[gid]
    return {sc.gos[t['m_GameObject']['m_PathID']]['m_Name']: t['m_GameObject']['m_PathID']
            for t in sc.transforms.values() if t['m_Father']['m_PathID'] == tid}


def _box_world(sc, gid):
    """A child's one BoxCollider2D as a world box [x0, y0, x1, y1]."""
    from actors import _component_records
    boxes = [tree for _, kind, tree in _component_records(sc, gid) if kind == 'BoxCollider2D']
    if len(boxes) != 1:
        raise ValueError(f'expected one BoxCollider2D on {sc.gos[gid]["m_Name"]}')
    b = boxes[0]
    m = sc.world(sc.go_transform[gid])
    if abs(m[0][1]) > 1e-6 or abs(m[1][0]) > 1e-6:
        raise ValueError(f'{sc.gos[gid]["m_Name"]} is rotated')
    cx = m[0][3] + b['m_Offset']['x'] * m[0][0]
    cy = m[1][3] + b['m_Offset']['y'] * m[1][1]
    hw, hh = abs(b['m_Size']['x'] * m[0][0]) / 2, abs(b['m_Size']['y'] * m[1][1]) / 2
    return [cx - hw, cy - hh, cx + hw, cy + hh]


def recognize_placement(sc, actor):
    """Admit the placed Brooding Mawlek, or refuse with the reason."""
    gid = actor['game_object']
    if sc.gos[gid]['m_Name'] != BODY_NAME or sc.gos[gid]['m_Layer'] != 11:
        raise ValueError('not the Mawlek body on the enemy layer')
    fsms = _fsms(sc, gid)
    if set(fsms) != FSM_SET or not fsms['Mawlek Control']['m_Enabled']:
        raise ValueError('unsupported Mawlek FSM set: ' + ', '.join(sorted(fsms)))
    control = fsms['Mawlek Control']['fsm']
    states = {state['name'] for state in control['states']}
    for needed in ('Dormant', 'Wake', 'Start', 'Idle', 'Super Select', 'Shoot', 'Jump', 'Land 2', 'Music'):
        if needed not in states:
            raise ValueError('Mawlek Control lacks ' + needed)
    children = _children(sc, gid)
    missing = [name for name in CHILDREN if name not in children]
    if missing:
        raise ValueError('Mawlek lacks ' + ', '.join(missing))
    health = actor['health_manager']
    # `Start` clears the serialized invincibility; the guest runtime owns that.
    if health['hasSpecialDeath'] or health['damageOverride'] or health['invincibleFromDirection']:
        raise ValueError('unsupported Mawlek HealthManager variant')
    matrix = sc.world(sc.go_transform[gid])
    if abs(matrix[0][1]) > 1e-6 or abs(matrix[1][0]) > 1e-6:
        raise ValueError('Mawlek is rotated')
    x, y = actor['position'][:2]
    wake = _box_world(sc, children['Alert Range New'])
    wake_q16 = [round((wake[0] - x) * 65536), round((wake[1] - y) * 65536),
                round((wake[2] - x) * 65536), round((wake[3] - y) * 65536)]
    return {
        'kind': 'Mawlek', 'guest_enabled': True, 'art_bindings': dict(ART_BINDINGS),
        # `Mawlek Control` starts `Start` with SetInvincible false; the body is
        # serialized invincible so nothing hurts it while it lurks.
        'no_corpse': True,
        'wake_q16': wake_q16,
        'initial_direction': -1,
        'fsm_sha256': hashlib.sha256(repr(sorted(
            (s['name'], tuple(s['actionData']['actionNames'])) for s in control['states'])).encode()).hexdigest(),
        'limitations': [
            'Every clip, the arms, the head, the shots and the corpse are cooked by host/mawlek_art.py '
            'into this scene alone; the ActorSpec clip fields point at Dummy Blank.',
            'The wake roar\'s Roar Lock on the hero, the particles and the blood are not reproduced.',
        ],
    }

