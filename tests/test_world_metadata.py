"""Host HKWMTA01 cooker checks."""
import json
import struct
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
from world_metadata import encode_scene


class WorldMetadataTests(unittest.TestCase):
    def test_scene_bank_has_fixed_wire_sections_and_q16_values(self):
        scene = {"scene_id": 4, "scene_name": "Synthetic", "file": "level99"}
        rows = [{"chunk_id": 7, "activation_bounds": [0, 0, 2, 2],
                 "collision_bounds": [0, 0, 2, 2], "camera_bounds": [0, 0, 2, 2],
                 "neighbour_chunks": [8], "breakables": [{"source": "level99:12",
                 "state_index": 3, "box": [0, 0, 1, 1], "hit_polygons": [[[0, 0], [1, 0], [0, 1]]]}]}]
        payload, report = encode_scene(scene, rows)
        self.assertEqual(payload[:8], b"HKWMTA01")
        self.assertEqual(struct.unpack_from("<I", payload, 52)[0], len(payload))
        self.assertEqual(report["object_count"], 1)
        section = struct.unpack_from("<3I", payload, 64)
        self.assertEqual(section[2], 76)
        object_section = struct.unpack_from("<3I", payload, 76)
        self.assertEqual(struct.unpack_from("<i", payload, object_section[0] + 12)[0], 0)

    def test_hazard_and_checkpoint_payload_words(self):
        scene = {"scene_id": 1, "scene_name": "Synthetic", "file": "level1"}
        tri = [[[0, 0], [1, 0], [0, 1]]]
        rows = [{"chunk_id": 3, "activation_bounds": [0, 0, 2, 2], "camera_bounds": [0, 0, 2, 2],
                 "hazards": [{"source": "level1:5", "bounds": [0, 0, 1, 1], "world_polygons": tri,
                              "position": [0.5, 0], "damage": 2, "hazard_type": 2}],
                 "checkpoints": [{"source": "level1:6", "bounds": [0, 0, 1, 1], "world_polygons": tri,
                                  "spawn": [1, -1], "respawn_facing_right": False, "fire_once": False}]}]
        payload, report = encode_scene(scene, rows)
        self.assertEqual(report["object_count"], 2)
        offset, count, stride = struct.unpack_from("<3I", payload, 76)
        self.assertEqual(stride, 48)
        hazard = struct.unpack_from("<IIHH4i2I3i", payload, offset)
        self.assertEqual((hazard[2], hazard[3], hazard[10:]), (3, 8, (32768, 2 | 1 << 16, 0)))
        checkpoint = struct.unpack_from("<IIHH4i2I3i", payload, offset + stride)
        self.assertEqual((checkpoint[2], checkpoint[10:]), (4, (65536, -65536, -1)))
        rows[0]["checkpoints"][0]["fire_once"] = True
        with self.assertRaises(ValueError):
            encode_scene(scene, rows)

    def test_camera_lock_carries_two_limit_points_and_look_flags(self):
        # A scene's locks ride once, in its first region, from the scene record.
        lock = {"source": "level1:9", "bounds": [2, 1, 10, 8], "limit_points": [[(14.6, 8.3), (30.5, 8.3)]],
                "prevent_look_down": True, "prevent_look_up": False, "max_priority": True, "battle": True,
                "expires_ticks": 120}
        scene = {"scene_id": 1, "scene_name": "Synthetic", "file": "level1", "camera_lock_objects": [lock]}
        rows = [{"chunk_id": 3, "activation_bounds": [0, 0, 24, 16], "camera_bounds": [0, 0, 24, 16]},
                {"chunk_id": 4, "activation_bounds": [24, 0, 48, 16], "camera_bounds": [24, 0, 48, 16]}]
        payload, report = encode_scene(scene, rows)
        self.assertEqual(report["object_count"], 1)
        offset, count, stride = struct.unpack_from("<3I", payload, 76)
        lock_row = struct.unpack_from("<IIHH4i2I3i", payload, offset)
        self.assertEqual((lock_row[1], lock_row[2], lock_row[3]), (0xFFFFFFFF, 11, 1 | 4 | 8))
        self.assertEqual(lock_row[4:8], (2 * 65536, 65536, 10 * 65536, 8 * 65536))
        self.assertEqual(lock_row[9], 1)  # one polygon: the two limit points
        self.assertEqual(lock_row[10], 120)  # the lifetime in ticks
        points_offset, points_count, points_stride = struct.unpack_from("<3I", payload, 64 + 12 * 3)
        self.assertEqual((points_count, points_stride), (2, 8))
        self.assertEqual(struct.unpack_from("<4i", payload, points_offset), (956826, 543949, 1998848, 543949))
        lock["limit_points"] = [[(0, 0)]]
        with self.assertRaises(ValueError):
            encode_scene(scene, rows)

    def test_multiple_scene_rows_are_sorted_by_global_chunk(self):
        scene = {"scene_id": 0, "scene_name": "Synthetic", "file": "level6"}
        def row(chunk):
            return {"chunk_id": chunk, "activation_bounds": [0, 0, 1, 1],
                    "collision_bounds": [0, 0, 1, 1], "camera_bounds": [0, 0, 1, 1],
                    "neighbour_chunks": [], "breakables": []}
        payload, _ = encode_scene(scene, [row(9), row(2)])
        offset = struct.unpack_from("<I", payload, 64)[0]
        self.assertEqual(struct.unpack_from("<I", payload, offset)[0], 2)


if __name__ == "__main__":
    unittest.main()
