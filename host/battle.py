"""Which enemies an arena brings and takes away, and what that does to the pool.

Crossroads_22 fights in waves of ordinary enemies. Its `Battle Control` FSM
activates the children of `Wave 1` .. `Wave 4` one wave at a time, each only
after the last wave's members are dead, and its `Remove on battle start` FSM
kills what stood in the room before (the world Hatcher and four Spitters). So
no moment of the fight holds every placement at once, and the guest's 32 actor
slots only have to hold the largest moment, not the sum.

`membership` reads which of the two an enemy is; `pool_peak` is the largest
moment. The Rust twin is host/hk-cook/src/battle.rs, and both have to agree
because the cook's pool checks (host/actors.py, host/hatcher.py) call them.
"""
import re

WAVE_NAME = re.compile(r'^Wave (\d+)$')
BATTLE_CONTROL = 'Battle Control'
REMOVE_ON_START = 'Remove on battle start'


def _fsm_names(records):
    return {data['fsm']['name'] for _, kind, data in records if kind == 'PlayMakerFSM'}


# The states only Crossroads_22's four wave `Battle Control` has. Crossroads_08's
# is a different, two wave FSM that this port does not drive, so its enemies are
# not arena members and stand with the scene as they always have.
WAVE_STATES = frozenset(['Wave 1', 'Wave 2', 'Wave 3', 'Wave 4', 'Pause W 1', 'Pause W 2', 'Pause W 3',
                         'End Pause', 'Blob Open'])


def wave_arena(records):
    """Whether these components carry the four wave `Battle Control`."""
    return any(kind == 'PlayMakerFSM' and data['fsm']['name'] == BATTLE_CONTROL
               and WAVE_STATES <= {state['name'] for state in data['fsm']['states']}
               for _, kind, data in records)


def _parent(sc, gid):
    father = sc.transforms[sc.go_transform[gid]]['m_Father']['m_PathID']
    if not father:
        return None
    return sc.transforms[father]['m_GameObject']['m_PathID']


def membership(sc, gid, records):
    """(wave, removable) of the enemy on `gid`: its 1 based wave or 0, and whether
    the arena's `Remove on battle start` kills it when the fight begins.

    A Hatcher Baby carries that FSM too but is not removed by it: its `Inert`
    state has no transition for the event, so it stays parked in its cage.
    """
    removable = (REMOVE_ON_START in _fsm_names(records)
                 and not sc.gos[gid]['m_Name'].startswith('Hatcher Baby'))
    wave = 0
    from actors import _component_records
    parent = _parent(sc, gid)
    while parent is not None:
        found = WAVE_NAME.match(sc.gos[parent]['m_Name'])
        if found:
            above = _parent(sc, parent)
            if above is not None and wave_arena(_component_records(sc, above)):
                wave = int(found.group(1))
                break
        parent = _parent(sc, parent)
    return wave, removable


def pool_peak(members):
    """Slots the guest needs for `members`, an iterable of (wave, removable).

    Before the battle everything outside a wave stands. Once it starts the
    removable ones are gone and one wave at a time stands in their place.
    """
    members = list(members)
    standing = [m for m in members if not m[0]]
    kept = sum(1 for m in standing if not m[1])
    sizes = {}
    for wave, _ in members:
        if wave:
            sizes[wave] = sizes.get(wave, 0) + 1
    return max(len(standing), kept + max(sizes.values(), default=0))


def actor_member(actor):
    """(wave, removable) of a cooked actor row."""
    return actor.get('battle_wave', 0), bool(actor.get('battle_removable', False))
