"""The compact actor art bank's host side (host/actor_bank.py) without retail files or a cooker run."""
import json
import struct
import sys
import tempfile
import unittest
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'host'))
import actor_bank


def slice_of(id, sprites, sprite_base, clip_base, frames):
    """A bank slice as `actor_bank.cook` describes one: `sprites` unit-box sprites and one clip over `frames`."""
    palette = [0] + [0x8000 | i for i in range(1, 16)]
    return {'id': id, 'sprite_base': sprite_base, 'clip_base': clip_base, 'owners': [],
            'doc': {'palettes': [palette], 'sprites': [{'box': [-1.0, -2.0, 3.0, 4.0], 'w': 8, 'h': 8}] * sprites,
                    'clips': [{'name': 'Walk', 'library': 'lib', 'fps': 12.0, 'wrap': 0, 'loop_start': 0, 'frames': frames}]}}


class ActorBankTests(unittest.TestCase):
    def test_the_shipped_config_is_readable_and_its_packages_are_data_modules(self):
        for name, e in actor_bank.scenes().items():
            self.assertTrue(e['package'].startswith('art_'), name)
        self.assertEqual(list(actor_bank.packages()), [e['package'] for e in actor_bank.scenes().values()])

    def test_an_empty_config_writes_an_empty_table_and_changes_nothing_else(self):
        with tempfile.TemporaryDirectory() as tmp:
            config = Path(tmp) / 'actor_banks.json'
            config.write_text('{"scenes": {}}')
            old, old_root = actor_bank.CONFIG, actor_bank.ROOT
            try:
                actor_bank.CONFIG, actor_bank.ROOT = config, Path(tmp)
                (Path(tmp) / 'data').mkdir()
                self.assertEqual(actor_bank.scenes(), {})
                self.assertEqual(actor_bank.packages(), {})
                actor_bank.write_tables(lambda scene: 0)
                self.assertIn('SCENE_BANKS:[SceneBank;0]=[]', (Path(tmp) / 'data/actor_banks.rs').read_text())
            finally:
                actor_bank.CONFIG, actor_bank.ROOT = old, old_root

    def test_a_bank_entry_must_be_one_or_two_slices_with_an_art_package(self):
        with tempfile.TemporaryDirectory() as tmp:
            config = Path(tmp) / 'c.json'
            old = actor_bank.CONFIG
            try:
                actor_bank.CONFIG = config
                for banks, package in (([], 'art_x'), ([{}, {}, {}], 'art_x'), ([{}], 'husks')):
                    config.write_text(json.dumps({'scenes': {'S': {'id': 1, 'package': package, 'actors': [], 'banks': banks}}}))
                    with self.assertRaises(ValueError):
                        actor_bank.scenes()
            finally:
                actor_bank.CONFIG = old

    def test_frames_carry_their_global_sprite_index_in_the_low_box_bytes(self):
        from cook import Atlas
        slices = [slice_of(4, 300, 0, 0, [0, 299]), slice_of(5, 2, 300, 1, [1])]
        atlas, frames, clips, carriers = actor_bank.room_tables(slices)
        self.assertEqual(carriers, 2)
        self.assertEqual([(c['start'], c['count']) for c in clips], [(0, 2), (2, 1)])
        got = []
        for f in frames:
            box = [round(v * 65536) for v in f['box']]
            got.append(((box[0] & 0xff) | (box[1] & 0xff) << 8, box[0] & ~0xff, box[1] & ~0xff, box[2], box[3]))
            self.assertEqual(f['texture'], 0)
        self.assertEqual([g[0] for g in got], [0, 299, 301])
        # The rest of the box is the sprite's own to within the low byte.
        self.assertEqual(got[0][1:], (round(-1.0 * 65536) & ~0xff, round(-2.0 * 65536) & ~0xff, 3 * 65536, 4 * 65536))
        atlas.pack()
        self.assertEqual(len(atlas.entries), 2)
        # Carrier 0 of a slice is a 4x4 streamed texture whose stream is HKBP, the bank id and the palette index.
        w, h, palette, pixels = atlas.quantized[1]
        self.assertEqual((w, h, pixels), (4, 4, b'HKBP' + struct.pack('<2H', 5, 0)))


if __name__ == '__main__':
    unittest.main()
