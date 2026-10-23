"""Source-free tests of how a rigid Breakable part becomes a debris piece.

host/effects.py `fragment_geometry` is the classification the cook used to refuse
two thirds of the parts on: it now reads each part's own components (SpinSelf,
SpinSelfSimple, SimpleRock, FinishingRigidBody) and says what the guest solver
needs, and refuses only what the solver cannot hold, by name.
"""
import math
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'host'))
import effects

WHITE = {'r': 1.0, 'g': 1.0, 'b': 1.0, 'a': 1.0}
ROCK_TINT = {'r': 0.489, 'g': 0.51, 'b': 0.559, 'a': 1.0}
BOX = {'m_Size': {'x': 1.0, 'y': 2.0}, 'm_Offset': {'x': 0.0, 'y': 0.0}}
FINISHING = {'waitDuration': 8.0, 'shrinkDuration': 1.0, 'conclusion': 0, 'persistOffScreen': 0}
UPRIGHT = [[1.0, 0.0], [0.0, 1.0]]


def parts(*extra, color=WHITE, collider=('BoxCollider2D', BOX)):
    found = {'SpriteRenderer': (1, {'m_Color': color, 'm_FlipX': 0, 'm_FlipY': 0}),
             collider[0]: (2, collider[1])}
    for name, data in extra:
        found[name] = (3, data)
    return found


def turned(sx, sy, degrees):
    c, s = math.cos(math.radians(degrees)), math.sin(math.radians(degrees))
    return [[c * sx, -s * sy], [s * sx, c * sy]]


class ClassTests(unittest.TestCase):
    def test_spin_self_keeps_its_factor_and_starts_at_random(self):
        g = effects.fragment_geometry(parts(('SpinSelf', {'spinFactor': 8.0})), UPRIGHT, 0.0)
        self.assertEqual((g['spin_half'], g['random_start'], g['finishing'], g['tint']), (16, True, False, False))

    def test_a_turned_spin_self_part_cooks_with_scale_from_its_column_lengths(self):
        # Red before: 'unsupported fragment world scale/rotation' for every turned matrix.
        g = effects.fragment_geometry(parts(('SpinSelf', {'spinFactor': 4.0})), turned(1.5, 0.75, 30.0), 0.0)
        self.assertEqual(g['scale'], [round(1.5 * 65536), round(0.75 * 65536)])

    def test_a_mirrored_parent_flips_x_only(self):
        mirrored = [[-1.0, 0.0], [0.0, 2.0]]
        g = effects.fragment_geometry(parts(('SpinSelf', {'spinFactor': 4.0})), mirrored, 0.0)
        self.assertEqual(g['scale'], [-65536, 2 * 65536])

    def test_spin_self_simple_turns_the_other_way_and_starts_where_it_was_authored(self):
        g = effects.fragment_geometry(
            parts(('SpinSelfSimple', {'spinFactor': 5.0, 'randomStartRotation': 0, 'waitForCall': 0})),
            UPRIGHT, 123.5)
        self.assertEqual((g['spin_half'], g['random_start']), (-10, False))
        self.assertEqual(g['start_pose'], round(123.5 * 64))

    def test_simple_rock_is_seven_and_a_half_against_the_vx_with_its_one_tint_and_finishing(self):
        g = effects.fragment_geometry(
            parts(('SimpleRock', {}), ('FinishingRigidBody', FINISHING), color=ROCK_TINT), UPRIGHT, 0.0)
        self.assertEqual((g['spin_half'], g['random_start'], g['finishing'], g['tint']), (-15, True, True, True))

    def test_a_spinless_part_keeps_its_pose_and_has_an_inertia_all_the_same(self):
        g = effects.fragment_geometry(parts(), UPRIGHT, 270.0)
        self.assertEqual((g['spin_half'], g['random_start'], g['finishing']), (0, False, False))
        self.assertEqual(g['start_pose'], 270 * 64)
        self.assertGreater(g['inv_inertia'], 0)

    def test_finishing_alone_is_a_flag_not_a_spin(self):
        g = effects.fragment_geometry(parts(('FinishingRigidBody', FINISHING)), UPRIGHT, 0.0)
        self.assertEqual((g['spin_half'], g['finishing']), (0, True))

    def test_inverse_inertia_belongs_to_the_shape_not_to_the_spin(self):
        # The guest used to recover it from the torque, right only at factor 4.
        four = effects.fragment_geometry(parts(('SpinSelf', {'spinFactor': 4.0})), UPRIGHT, 0.0)
        ten = effects.fragment_geometry(parts(('SpinSelf', {'spinFactor': 10.0})), UPRIGHT, 0.0)
        self.assertEqual(four['inv_inertia'], ten['inv_inertia'])
        # 1 x 2 box: I = (1 + 4) / 12, so 1/I = 2.4.
        self.assertEqual(four['inv_inertia'], round(65536 / (5 / 12)))

    def test_the_polygon_comes_out_in_q16_with_its_centroid(self):
        g = effects.fragment_geometry(parts(), UPRIGHT, 0.0)
        self.assertEqual(g['polygon'], [[-32768, -65536], [32768, -65536], [32768, 65536], [-32768, 65536]])
        self.assertEqual(g['centroid'], [0, 0])


class RefusalTests(unittest.TestCase):
    def refused(self, found, matrix=UPRIGHT):
        with self.assertRaises(ValueError) as error:
            effects.fragment_geometry(found, matrix, 0.0)
        return str(error.exception)

    def test_a_round_collider_keeps_a_named_refusal(self):
        found = parts(('SpinSelf', {'spinFactor': 4.0}),
                      collider=('CircleCollider2D', {'m_Radius': 0.5, 'm_Offset': {'x': 0.0, 'y': 0.0}}))
        self.assertIn('unsupported rigid fragment collider CircleCollider2D', self.refused(found))

    def test_a_tint_is_only_the_simple_rocks(self):
        found = parts(('SpinSelf', {'spinFactor': 4.0}), color=ROCK_TINT)
        self.assertIn('unsupported fragment sprite color/reflection', self.refused(found))

    def test_a_changed_finishing_timeline_fails_loudly(self):
        found = parts(('FinishingRigidBody', dict(FINISHING, waitDuration=4.0)))
        self.assertIn('finishing timeline', self.refused(found))

    def test_a_scale_outside_the_guest_range_is_refused_on_the_column_lengths(self):
        self.assertIn('scale', self.refused(parts(), turned(3.0, 1.0, 45.0)))

    def test_more_than_eight_points_is_refused(self):
        ring = [{'x': math.cos(i), 'y': math.sin(i)} for i in range(9)]
        found = parts(collider=('PolygonCollider2D',
                                {'m_Points': {'m_Paths': [ring]}, 'm_Offset': {'x': 0.0, 'y': 0.0}}))
        self.assertIn('polygon', self.refused(found))

    def test_a_spin_the_flag_bits_cannot_hold_is_refused(self):
        self.assertIn('spin', self.refused(parts(('SpinSelf', {'spinFactor': 16.0}))))
        self.assertIn('spin', self.refused(parts(('SpinSelf', {'spinFactor': 3.3}))))


class RegionFilterTests(unittest.TestCase):
    def test_only_the_owners_in_the_region_cook_art(self):
        import breakables
        seen = []
        original = breakables.breakable_sources
        breakables.breakable_sources = lambda scene, bounds=None, errors=None, contract=True: (
            seen.append(bounds) or [])
        try:
            from types import SimpleNamespace
            scene = SimpleNamespace(file=SimpleNamespace(name='/x/level6'))
            effects.fragment_records(None, scene, None, [], bounds=[1, 2, 3, 4])
        finally:
            breakables.breakable_sources = original
        self.assertEqual(seen, [[1, 2, 3, 4]])


if __name__ == '__main__':
    unittest.main()
