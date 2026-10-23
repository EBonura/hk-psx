"""Source-free tests of tools/fragment_audit.py's blocker census."""
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))
sys.path.insert(0, str(ROOT / 'tools'))
import fragment_audit as audit

UPRIGHT = [[1.0, 0.0], [0.0, 1.0]]
TURNED = [[0.0, -1.0], [1.0, 0.0]]
WHITE = {'r': 1.0, 'g': 1.0, 'b': 1.0, 'a': 1.0}
BOX = {'m_Size': {'x': 1.0, 'y': 1.0}, 'm_Offset': {'x': 0.0, 'y': 0.0}}


def parts(*extra, color=WHITE, collider='BoxCollider2D'):
    found = {'SpriteRenderer': (1, {'m_Color': color, 'm_FlipX': 0, 'm_FlipY': 0}), collider: (2, BOX)}
    for name, data in extra:
        found[name] = (3, data)
    return found


class BlockerTests(unittest.TestCase):
    def test_a_speed_driven_upright_piece_has_no_blocker(self):
        self.assertEqual(audit.blockers(parts(('SpinSelf', {'spinFactor': 4.0})), UPRIGHT), [])

    def test_every_independent_reason_is_listed_not_only_the_first(self):
        found = audit.blockers(parts(('SpinSelfSimple', {'spinFactor': 5.0}), color={'r': .5, 'g': .5, 'b': .5, 'a': 1.0}), TURNED)
        self.assertEqual(found, ['spin_simple', 'rotated', 'tint'])

    def test_a_spinless_piece_and_a_negative_factor_are_the_same_reason(self):
        self.assertEqual(audit.blockers(parts(), UPRIGHT), ['no_positive_spin'])
        self.assertEqual(audit.blockers(parts(('SpinSelf', {'spinFactor': -7.5})), UPRIGHT), ['no_positive_spin'])

    def test_scale_is_judged_on_the_column_lengths_of_a_turned_matrix(self):
        turned_big = [[0.0, -3.0], [3.0, 0.0]]
        self.assertEqual(audit.blockers(parts(('SpinSelf', {'spinFactor': 4.0})), turned_big), ['rotated', 'scale'])

    def test_a_round_collider_is_its_own_reason(self):
        self.assertEqual(audit.blockers(parts(('SpinSelf', {'spinFactor': 4.0}), collider='CircleCollider2D'), UPRIGHT),
                         ['circle_collider'])


if __name__ == '__main__':
    unittest.main()
