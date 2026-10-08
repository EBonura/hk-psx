"""The script bank cooker.

`host/script_ir.py` decides what an FSM means; this decides what ships. The two
things that can be silently wrong here are the trigger rectangle, which decides
where a script fires, and the start state, which decides what it does first.
Both are pinned. So is the rule that keeps the shipped count honest: an FSM that
compiles to nothing is not cooked, and one that can never take a transition is
cooked but never counted as behaviour.
"""
import sys, unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'host'))
sys.path.insert(0, str(Path(__file__).resolve().parent))

from cook_scripts import (initial_variables, sent_events, start_state, volume_bounds, writes)
from script_ir import (BOOL_TEST, HERO_TRIGGER, NEXT_FRAME, PD_SET, SET_INT, WAIT, REPEAT,
                       TRIGGER_ENTER)

ONE = 65536


class Fake:
    """One GameObject carrying colliders, with a translation and a scale.

    Enough of `scene.Scene` for `volume_bounds`: the components it walks, the
    world transform it places them with, and the rotation test the circle needs.
    """

    def __init__(self, colliders, offset=(0, 0), scale=(1, 1), rotated=False):
        self.gos = {1: {'m_Name': 'Trigger', 'm_Component': [
            {'component': {'m_FileID': 0, 'm_PathID': 10 + i}} for i in range(len(colliders))]}}
        self.objects = {10 + i: c for i, c in enumerate(colliders)}
        self.go_transform = {1: 100}
        self.offset, self.scale, self.rotated = offset, scale, rotated

    def world(self, _tid):
        skew = 0.5 if self.rotated else 0.0
        return [[self.scale[0], skew, 0, self.offset[0]],
                [skew, self.scale[1], 0, self.offset[1]],
                [0, 0, 1, 0], [0, 0, 0, 1]]

    def point(self, _gid, x=0, y=0, z=0):
        m = self.world(100)
        return tuple(sum(m[i][j] * v for j, v in enumerate([x, y, z, 1])) for i in range(3))


def box(w, h, offset=(0, 0), trigger=True, enabled=True):
    return ('BoxCollider2D', {'m_Enabled': enabled, 'm_IsTrigger': trigger,
                              'm_Offset': {'x': offset[0], 'y': offset[1]},
                              'm_Size': {'x': w, 'y': h}})


class VolumeTests(unittest.TestCase):
    def test_a_box_becomes_its_world_rectangle(self):
        scene = Fake([box(4, 2)], offset=(10, 3))
        self.assertEqual(volume_bounds(scene, 1), [8 * ONE, 2 * ONE, 12 * ONE, 4 * ONE])

    def test_the_transform_scale_reaches_the_rectangle(self):
        # A trigger under a scaled parent is the scaled box, not the authored
        # one. Getting this wrong moves where every script in that tree fires.
        scene = Fake([box(4, 2)], offset=(0, 0), scale=(2, 3))
        self.assertEqual(volume_bounds(scene, 1), [-4 * ONE, -3 * ONE, 4 * ONE, 3 * ONE])

    def test_two_boxes_become_their_union(self):
        # Three of the admitted owners carry two, and Unity raises the callback
        # for whichever one the Knight crosses.
        scene = Fake([box(2, 2), box(2, 2, offset=(6, 0))])
        self.assertEqual(volume_bounds(scene, 1), [-ONE, -ONE, 7 * ONE, ONE])

    def test_a_disabled_or_non_trigger_collider_is_not_a_volume(self):
        for collider in (box(2, 2, trigger=False), box(2, 2, enabled=False)):
            with self.assertRaises(ValueError):
                volume_bounds(Fake([collider]), 1)

    def test_a_rotated_circle_refuses_rather_than_boxing_its_cardinal_points(self):
        # A box or polygon is the AABB of its transformed corners whatever the
        # rotation; a circle's silhouette is not.
        circle = ('CircleCollider2D', {'m_Enabled': True, 'm_IsTrigger': True,
                                       'm_Offset': {'x': 0, 'y': 0}, 'm_Radius': 2})
        self.assertEqual(volume_bounds(Fake([circle]), 1),
                         [-2 * ONE, -2 * ONE, 2 * ONE, 2 * ONE])
        with self.assertRaises(ValueError):
            volume_bounds(Fake([circle], rotated=True), 1)

    def test_a_collider_shape_the_cooker_cannot_reduce_refuses(self):
        edge = ('EdgeCollider2D', {'m_Enabled': True, 'm_IsTrigger': True,
                                   'm_Offset': {'x': 0, 'y': 0}})
        with self.assertRaises(ValueError):
            volume_bounds(Fake([edge]), 1)


class BindingTests(unittest.TestCase):
    def test_the_start_state_is_the_authored_one_not_the_first(self):
        program = {'state_names': ['Pause', 'Idle', 'Set']}
        self.assertEqual(start_state({'startState': 'Set'}, program), 2)

    def test_a_start_state_that_is_not_a_state_refuses(self):
        with self.assertRaises(ValueError):
            start_state({'startState': 'Nowhere'}, {'state_names': ['Idle']})

    def test_only_integer_initial_values_survive(self):
        # Nothing compiled can read a slot as a float or a string, and zero is
        # NO_OBJECT, so the rest start at zero without that being a guess.
        fsm = {'variables': {'a': [{'name': 'flag', 'value': True},
                                   {'name': 'count', 'value': 7},
                                   {'name': 'speed', 'value': 1.5},
                                   {'name': 'who', 'value': {'m_PathID': 0}}]}}
        program = {'var_count': 5, 'variables': {'flag': 0, 'count': 1, 'speed': 2, 'who': 3}}
        self.assertEqual(initial_variables(fsm, program), [1, 7, 0, 0, 0])


class ActivityTests(unittest.TestCase):
    """Which ops can do something, which is what separates the three cooked
    instances that act from the seven that reproduce a dead authored action."""

    def test_an_op_that_sends_nowhere_sends_nothing(self):
        self.assertEqual(sent_events((HERO_TRIGGER, TRIGGER_ENTER | REPEAT, 0, 0, 4)), (4,))
        self.assertEqual(sent_events((HERO_TRIGGER, TRIGGER_ENTER | REPEAT, 0, 0, 0)), ())
        self.assertEqual(sent_events((BOOL_TEST, 0, 0, 2, 3)), (2, 3))
        self.assertEqual(sent_events((WAIT, 0, 60, 5, 0)), (5,))
        self.assertEqual(sent_events((NEXT_FRAME, 0, 6, 0, 0)), (6,))
        self.assertEqual(sent_events((SET_INT, 0, 0, 1, 0)), ())

    def test_only_a_playerdata_write_or_a_native_call_counts_as_a_write(self):
        self.assertTrue(writes((PD_SET, 0, 0, 0, 0)))
        self.assertFalse(writes((SET_INT, 0, 0, 1, 0)))
        self.assertFalse(writes((HERO_TRIGGER, TRIGGER_ENTER | REPEAT, 0, 0, 1)))


if __name__ == '__main__':
    unittest.main()
