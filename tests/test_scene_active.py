"""Scene.active treats unreadable or foreign GameObjects as inactive."""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'host'))
from scene import Scene


class SceneActiveTests(unittest.TestCase):
    def test_missing_game_object_is_inactive_not_a_key_error(self):
        sc = Scene.__new__(Scene)
        sc.gos = {1: {'m_IsActive': 1}, 2: {'m_IsActive': 1}}
        sc.transforms = {10: {'m_GameObject': {'m_PathID': 1}, 'm_Father': {'m_PathID': 0}},
                         20: {'m_GameObject': {'m_PathID': 2}, 'm_Father': {'m_PathID': 99}}}
        sc.go_transform = {1: 10, 2: 20}
        self.assertTrue(sc.active(1))
        self.assertFalse(sc.active(2089))  # never read
        self.assertFalse(sc.active(2))     # parent transform missing


if __name__ == '__main__':
    unittest.main()
