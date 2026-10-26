"""The stored hash of every scene, atlas and world-metadata chunk (host/scene_bank.py `stored_hash`, which the
guest's `room_decode::word_hash` computes) matches the chunk file the pack report names.

The pack directory's own FNV-1a checksum is not compared for these chunks, so a postpass that rewrites a chunk
and records the old hash style (crossroads22_wave did, for scene 21's metadata) would otherwise reach the console
as a checksum failure at one gate.
"""
import json
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))
import scene_bank

REPORT = ROOT / '.hkpsx/packed-scenes.json'


@unittest.skipUnless(REPORT.is_file(), 'run a build first')
class StoredHashManifestTests(unittest.TestCase):
    def test_every_chunk_file_matches_its_recorded_hash(self):
        report = json.loads(REPORT.read_text())
        rows = [(f"scene {e['scene_id']}", e['resident_stored_path'], e['resident_stored_fnv']) for e in report['scenes']]
        rows += [(f'atlas {i}', a['stored_path'], a['stored_fnv']) for i, a in enumerate(report['atlases'])]
        rows += [(f"metadata {m['scene_id']}", m['stored_path'], m['stored_fnv']) for m in report['world_metadata']]
        self.assertTrue(rows)
        for name, path, recorded in rows:
            with self.subTest(chunk=name):
                self.assertEqual(scene_bank.stored_hash((ROOT / path).read_bytes()), recorded, path)


if __name__ == '__main__':
    unittest.main()
