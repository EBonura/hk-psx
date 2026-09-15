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
