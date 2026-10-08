"""Whole-world matrix keeps source evidence separate from PS1 admission."""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
from world_pack_matrix import build, markdown


def fixture():
    world = {
        "fingerprint": "world",
        "inputs_unchanged": True,
        "run": {"status": "verified"},
        "coverage": {"catalog_scenes": 2, "processed_scenes": 2,
                      "imported_scenes": 1, "partial_scenes": 1,
                      "failed_scenes": 0, "all_scenes_processed": True},
        "scenes": [
            {"index": 0, "file": "level0", "scene_name": "A", "path": "A.unity",
             "status": "imported", "outputs": {"geometry": {"bytes": 10,
             "uncompressed_bytes": 20, "sha256": "ga"}, "components": {"bytes": 3,
             "uncompressed_bytes": 4, "sha256": "ca"}}, "counts": {}, "unsupported": []},
            {"index": 1, "file": "level1", "scene_name": "B", "path": "B.unity",
             "status": "partial", "outputs": {"geometry": {"bytes": 11,
             "uncompressed_bytes": 30, "sha256": "gb"}}, "counts": {},
             "unsupported": [{"type": "TextMesh"}]},
        ],
    }
    inventory = {
        "fingerprint": "inventory", "complete": True,
        "rooms": [
            {"scene_name": "A", "texture_ids": ["shared", "a"],
             "sprite_ids": ["sa"], "animation_ids": ["aa"],
             "source_texture_bytes": 2, "source_atlas_4bpp_estimate_bytes": 1},
            {"scene_name": "B", "texture_ids": ["shared", "b"],
             "sprite_ids": ["sb"], "animation_ids": ["ab"],
             "source_texture_bytes": 3, "source_atlas_4bpp_estimate_bytes": 2},
        ],
    }
    graph = {"rooms": [{"scene_name": "A", "neighbours": ["B"]},
                        {"scene_name": "B", "neighbours": ["A"]}]}
    return world, inventory, graph


class WorldPackMatrixTests(unittest.TestCase):
    def test_matrix_joins_current_and_neighbour_source_sets(self):
        data = build(*fixture())
        self.assertEqual(data["scene_count"], 2)
        row = data["scenes"][0]
        self.assertEqual(row["window"]["scene_names"], ["A", "B"])
        self.assertEqual(row["window"]["texture_count"], 3)
        self.assertEqual(row["window"]["geometry_uncompressed_bytes"], 50)
        self.assertIsNone(row["cooking"]["ps1_ram_bytes"])
        self.assertEqual(row["cooking"]["admission"], "source_matrix_only")

    def test_matrix_rejects_stale_or_incomplete_inputs(self):
        world, inventory, graph = fixture()
        world["inputs_unchanged"] = False
        with self.assertRaisesRegex(ValueError, "verified"):
            build(world, inventory, graph)
        world, inventory, graph = fixture()
        inventory["complete"] = False
        with self.assertRaisesRegex(ValueError, "incomplete"):
            build(world, inventory, graph)

    def test_matrix_rejects_scene_mismatch(self):
        world, inventory, graph = fixture()
        inventory["rooms"].pop()
        with self.assertRaisesRegex(ValueError, "mismatch"):
            build(world, inventory, graph)


if __name__ == "__main__":
    unittest.main()


class ScenePackMergeTests(unittest.TestCase):
    def packs(self):
        return {
            "A": {"status": "cooked", "resident_bytes": 98808, "page_bytes": 229376, "palette_bytes": 16032,
                  "regions": 12, "pages": 7, "palettes": 501, "decoder_status": "PASS",
                  "unsupported_cook_errors": [{"source": "level37:4778"}], "actors": [{}, {}],
                  "summary_path": "/tmp/A/summary.json"},
            "B": {"status": "over_budget", "error": "ValueError('Scene37: 22 pages exceeds 20')",
                  "resident_bytes": 500000, "page_bytes": 720896, "palette_bytes": 1000},
        }

    def test_isolated_pack_costs_fill_ps1_fields_without_claiming_admission(self):
        data = build(*fixture(), scene_packs=self.packs())
        a, b = data["scenes"]
        self.assertEqual(a["cooking"]["ps1_ram_bytes"], 98808)
        self.assertEqual(a["cooking"]["ps1_vram_bytes"], 229376 + 16032)
        self.assertEqual(a["cooking"]["admission"], "isolated_pack_measured")
        self.assertEqual(a["cooking"]["pack"]["decoder_status"], "PASS")
        self.assertEqual(a["cooking"]["pack_unsupported_errors"], 1)
        self.assertEqual(a["cooking"]["pack_actors"], 2)
        # Over-budget packs keep their failure visible and never report a fitting cost.
        self.assertIsNone(b["cooking"]["ps1_ram_bytes"])
        self.assertEqual(b["cooking"]["admission"], "isolated_pack_over_budget")
        self.assertIn("exceeds", b["cooking"]["pack"]["error"])
        self.assertEqual(data["pack_admission"], {"isolated_pack_measured": 1, "isolated_pack_over_budget": 1})
        self.assertEqual(data["pack_failure_causes"],
                         [{"cause": "ValueError('SceneN: N pages exceeds N')", "scenes": 1, "scene_names": ["B"]}])
        self.assertEqual(data["peaks"]["largest_isolated_pack_resident"],
                         {"scene_name": "A", "bytes": 98808, "vram_bytes": 245408})
        text = markdown(data)
        self.assertIn("- Pack failure cause (1 scenes): `ValueError('SceneN: N pages exceeds N')`", text)
        self.assertIn("| A | imported | 1 | 20 | 2 | 1 | 1 | 98808 | 245408 | isolated_pack_measured |", text)

    def test_matrix_without_packs_keeps_source_only_defaults(self):
        data = build(*fixture())
        self.assertEqual(data["pack_admission"], {"source_matrix_only": 2})
        self.assertIsNone(data["peaks"]["largest_isolated_pack_resident"])
        self.assertNotIn("pack", data["scenes"][0]["cooking"])
