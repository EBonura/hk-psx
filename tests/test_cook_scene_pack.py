"""Isolated scene-pack envelopes and view layout derive only from source records."""
import json
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
from cook_scene_pack import envelope, layout, split, read_summaries, mandatory_packets, failing_region, FORMAT
import struct


class StubScene:
    """Minimal host.scene.Scene surface used by regions.scene_metadata."""

    def __init__(self):
        self.objects = {}
        self.gos = {}
        self.positions = {}
        self.inactive = set()
        self.errors = []
        self.next = 1

    def add(self, name, typ, tree, position=(0.0, 0.0, 0.0), collider=None, active=True):
        gid = self.next
        self.next += 1
        self.gos[gid] = {'m_Name': name}
        self.positions[gid] = position
        if not active:
            self.inactive.add(gid)
        self.objects[self.next] = (typ, dict(tree, m_GameObject={'m_PathID': gid}, m_Enabled=1))
        self.next += 1
        if collider:
            w, h = collider
            self.objects[self.next] = ('BoxCollider2D', {'m_GameObject': {'m_PathID': gid},
                                                          'm_Offset': {'x': 0, 'y': 0}, 'm_Size': {'x': w, 'y': h}})
            self.next += 1
        return gid

    def point(self, gid, x=0, y=0, z=0):
        px, py, pz = self.positions[gid]
        return (px + x, py + y, pz + z)

    def active(self, gid):
        return gid not in self.inactive


def crossroads_like():
    sc = StubScene()
    sc.add('TileMap', 'tk2dTileMap', {'width': 100, 'height': 42})
    sc.add('top2', 'TransitionPoint', {'targetScene': 'Town', 'entryPoint': 'bot1', 'entryOffset': {'x': 0.0, 'y': 5.5}},
           position=(52.5, 42.5, 0.0), collider=(1, 1))
    sc.add('left1', 'TransitionPoint', {'targetScene': 'Crossroads_07', 'entryPoint': 'right1',
                                        'entryOffset': {'x': 0.0, 'y': 0.0}}, position=(-0.5, 9.0, 0.0), collider=(1, 3))
    # Authored lock volumes reach far outside the map and must not grow the envelope.
    sc.add('CameraLockArea C', 'CameraLockArea', {'cameraXMin': 0.0, 'cameraYMin': 13.0, 'cameraXMax': 9999.0, 'cameraYMax': 13.0},
           position=(52.65, 4.89, 0.0), collider=(200, 40))
    # Disabled tilemaps are not part of the resident scene.
    sc.add('Hidden', 'tk2dTileMap', {'width': 500, 'height': 500}, position=(-300, -300, 0), active=False)
    return sc


class EnvelopeTests(unittest.TestCase):
    def test_envelope_uses_tilemap_gates_and_spawn_points_only(self):
        env, meta = envelope(crossroads_like(), {'file': 'level37', 'scene_id': 37, 'scene_name': 'Crossroads_01'})
        self.assertEqual(env['tilemaps'], [[0.0, 0.0, 100.0, 42.0]])
        self.assertEqual(env['camera_global_bounds'], [0.0, 0.0, 100.0, 42.0])
        # left1 trigger reaches x-1; top2 spawn is y48; pads are 1/5/1/1.
        self.assertEqual(env['runtime_bounds'], [-2, -5, 101, 49])
        self.assertEqual(len(meta['gates']), 2)
        self.assertEqual(len(meta['camera_locks']), 1)

    def test_scene_without_source_extent_has_no_envelope(self):
        sc = StubScene()
        sc.add('Only sprite', 'SpriteRenderer', {})
        env, meta = envelope(sc, {'file': 'level0', 'scene_id': 0, 'scene_name': 'Menu'})
        self.assertIsNone(env)
        self.assertEqual(meta['gates'], [])

    def test_layout_matches_canonical_view_stepping(self):
        boxes = layout([-2, -5, 101, 49])
        self.assertEqual(len(boxes), 20)
        self.assertEqual(boxes[0], [-2, -5, 22, 11])
        self.assertEqual(boxes[-1], [94, 43, 101, 49])
        self.assertTrue(all(b[2] - b[0] <= 24 and b[3] - b[1] <= 16 for b in boxes))

    def test_pack_errors_name_the_failing_view(self):
        self.assertEqual(failing_region(ValueError('Similarity result exceeds runtime budget in region25: 6pages')), 25)
        self.assertEqual(failing_region('Scene region 17 packet reservation exceeded: 788+264 > 1032'), 17)
        self.assertIsNone(failing_region('Sequential scene decoding failed'))

    def test_split_halves_longer_axis_until_minimum(self):
        self.assertEqual(split([0, 0, 24, 16]), [[0, 0, 12, 16], [12, 0, 24, 16]])
        self.assertEqual(split([0, 0, 4, 16]), [[0, 0, 4, 8], [0, 8, 4, 16]])
        self.assertIsNone(split([0, 0, 3, 3]))

    def test_read_summaries_keeps_only_scene_pack_records(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / 'A').mkdir()
            (root / 'A' / 'summary.json').write_text(json.dumps({'format': FORMAT, 'scene_name': 'A', 'status': 'cooked'}))
            (root / 'B').mkdir()
            (root / 'B' / 'summary.json').write_text(json.dumps({'format': 'OTHER', 'scene_name': 'B'}))
            found = read_summaries(root)
        self.assertEqual(list(found), ['A'])
        self.assertEqual(found['A']['summary_path'], str(root / 'A' / 'summary.json'))


def room(draws, size=256):
    """Minimal HKROOM02 with one size×size texture and `draws` full-quad draws."""
    data = bytearray(b'HKROOM02' + struct.pack('<6I', 0, 1, draws, 0, 0, 0) + struct.pack('<2I', 0, 0))
    data += struct.pack('<6HI', 0, 0, 0, size, size, 0, 0)
    quad = [(-size, -size), (size, -size), (size, size), (-size, size)]
    for _ in range(draws):
        data += struct.pack('<Hh', 0, 0) + struct.pack('<i', 65536) + struct.pack('<8i', *[v for xy in quad for v in xy]) + struct.pack('<I', 0)
    return bytes(data)


class PacketReservationTests(unittest.TestCase):
    def test_reservation_grows_with_draws_and_rejects_other_formats(self):
        one = mandatory_packets(room(1))
        self.assertGreaterEqual(one, 1)
        self.assertEqual(mandatory_packets(room(5)), 5 * one)
        with self.assertRaisesRegex(ValueError, 'room format'):
            mandatory_packets(b'HKROOM01' + bytes(40))


if __name__ == '__main__':
    unittest.main()
