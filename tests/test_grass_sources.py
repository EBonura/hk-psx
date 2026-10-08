"""Unsupported GrassCut instances are explicit exceptions, not region failures."""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'host'))
from combat import grass_sources


class StubFile:
    name = 'level99'


class StubScene:
    file = StubFile()

    def __init__(self):
        self.objects = {}

    def grass(self, sid, x, disable_count=1):
        gid = sid * 10
        self.objects[gid] = ('GameObject', {'m_Name': f'grass {sid}', 'm_IsActive': 1})
        self.objects[gid + 1] = ('SpriteRenderer', {})
        self.objects[gid + 2] = ('SpriteRenderer', {})
        self.objects[gid + 3] = ('BoxCollider2D', {'m_GameObject': {'m_PathID': gid}, 'm_Enabled': 1, 'm_IsTrigger': 1,
                                                   'm_Size': {'x': 1, 'y': 1}, 'm_Offset': {'x': 0, 'y': 0}})
        self.objects[sid] = ('GrassCut', {'m_GameObject': {'m_PathID': gid}, 'm_Enabled': 1,
                                          'disable': [{'m_FileID': 0, 'm_PathID': gid + 1}] * disable_count,
                                          'enable': [{'m_FileID': 0, 'm_PathID': gid + 2}],
                                          'disableColliders': [], 'enableColliders': []})
        self.positions = getattr(self, 'positions', {})
        self.positions[gid] = (x, 5.0, 0.004)

    def point(self, gid, x=0, y=0, z=0):
        px, py, pz = self.positions[gid]
        return (px + x, py + y, pz + z)

    def active(self, gid):
        return True


class GrassSourceTests(unittest.TestCase):
    def scene(self):
        sc = StubScene()
        sc.grass(1, 30.0)
        sc.grass(2, 40.0, disable_count=2)
        return sc

    def test_unsupported_topology_is_recorded_when_errors_are_collected(self):
        errors = []
        grass = grass_sources(self.scene(), (20, 0, 70, 25), errors=errors)
        self.assertEqual([g['source'] for g in grass], ['level99:1'])
        self.assertEqual(errors, [{'id': 'level99:2', 'type': 'unsupported GrassCut',
                                   'error': 'unsupported GrassCut renderer/collider topology'}])

    def test_unsupported_topology_still_raises_without_error_collection(self):
        with self.assertRaisesRegex(ValueError, 'topology'):
            grass_sources(self.scene(), (20, 0, 70, 25))


if __name__ == '__main__':
    unittest.main()
