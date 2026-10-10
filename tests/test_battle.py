"""The arena pool budget: the largest moment of a fight, not the sum of its placements.

Crossroads_22 fights in four waves and removes what stood in the room first, so
it never has all of its placements alive at once. `membership` reads that off
the scene and `pool_peak` turns it into the slots the guest has to hold.
"""
import sys
import unittest
from pathlib import Path
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'host'))
import battle
from actors import _component_records

WAVE_FSM = {'fsm': {'name': 'Battle Control', 'states': [{'name': n} for n in (
    'Pause', 'Init', 'Wave 1', 'Wave 2', 'Wave 3', 'Wave 4', 'Pause W 1', 'Pause W 2', 'Pause W 3',
    'End Pause', 'Blob Open', 'End')]}}
TWO_WAVE_FSM = {'fsm': {'name': 'Battle Control', 'states': [{'name': n} for n in ('Pause', 'Init', 'Wave 1', 'Wave 2')]}}


def tree(control, extra=None):
    """Battle Scene (1) > Wave 1 (2) > summon (3) > Spitter (4), and a standing Spitter (5)
    carrying `Remove on battle start`, beside a Hatcher Baby (6) that carries it too."""
    names = {1: 'Battle Scene', 2: 'Wave 1', 3: 'Spitter Summon v2', 4: 'Spitter', 5: 'Spitter (1)', 6: 'Hatcher Baby Spawner (3)'}
    father = {1: 0, 2: 1, 3: 2, 4: 3, 5: 0, 6: 0}
    objects = {100: ('PlayMakerFSM', {'m_GameObject': {'m_PathID': 1}, **control}),
               101: ('PlayMakerFSM', {'m_GameObject': {'m_PathID': 5}, 'fsm': {'name': battle.REMOVE_ON_START}}),
               102: ('PlayMakerFSM', {'m_GameObject': {'m_PathID': 6}, 'fsm': {'name': battle.REMOVE_ON_START}})}
    return SimpleNamespace(
        gos={gid: {'m_Name': name} for gid, name in names.items()},
        objects=objects,
        transforms={gid: {'m_GameObject': {'m_PathID': gid}, 'm_Father': {'m_PathID': father[gid]}} for gid in names},
        go_transform={gid: gid for gid in names})


class MembershipTests(unittest.TestCase):
    def test_a_summoned_enemy_belongs_to_its_wave(self):
        sc = tree(WAVE_FSM)
        self.assertEqual(battle.membership(sc, 4, _component_records(sc, 4)), (1, False))

    def test_what_stood_in_the_room_is_removed_unless_it_is_a_cage_member(self):
        sc = tree(WAVE_FSM)
        self.assertEqual(battle.membership(sc, 5, _component_records(sc, 5)), (0, True))
        self.assertEqual(battle.membership(sc, 6, _component_records(sc, 6)), (0, False))

    def test_a_different_battle_control_is_not_an_arena_this_port_drives(self):
        sc = tree(TWO_WAVE_FSM)
        self.assertEqual(battle.membership(sc, 4, _component_records(sc, 4)), (0, False))


class PoolPeakTests(unittest.TestCase):
    def test_crossroads_22_as_measured(self):
        # 23 cage members, the placed Hatcher and four Spitters, then waves of 2, 3, 3 and 4.
        members = [(0, False)] * 23 + [(0, True)] * 5 + [(1, False)] * 2 + [(2, False)] * 3 + [(3, False)] * 3 + [(4, False)] * 4
        self.assertEqual(len(members), 40)
        self.assertEqual(battle.pool_peak(members), 28)

    def test_a_room_without_an_arena_costs_what_stands_in_it(self):
        self.assertEqual(battle.pool_peak([(0, False)] * 7), 7)
        self.assertEqual(battle.pool_peak([]), 0)

    def test_the_battle_can_be_the_busier_moment(self):
        self.assertEqual(battle.pool_peak([(0, False)] * 10 + [(0, True)] * 2 + [(1, False)] * 4), 14)


if __name__ == '__main__':
    unittest.main()
