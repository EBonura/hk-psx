"""The scene bank's rigid debris pieces (KIND_FRAGMENT, KIND_FRAGMENT_FRAMES).

tools/world_metadata.py writes each piece once at the head of the scene's first
region and a (piece, frame) map per region that can draw it. The golden bank under
tests/golden is parsed by the Rust format crate and decoded by the guest solver
(shared/hk-sim/tests/debris.rs), so the two sides share one byte contract.
"""
import struct
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))
sys.path.insert(0, str(ROOT / 'tools'))
import world_metadata as wm

GOLDEN = ROOT / 'tests/golden/fragment_bank.hkwm'
SCENE = {'scene_id': 3, 'file': 'level9', 'scene_name': 'Crossroads_01', 'raw_fnv': 7}


def piece(source, owner, frame, **changes):
    found = {'source': f'level9:{source}', 'door_state': owner, 'frame': frame,
             'origin': [65536 * 10, -65536 * 3], 'centroid': [1000, -2000], 'scale': [-65536, 65536],
             'polygon': [[-32768, -65536], [32768, -65536], [32768, 65536], [-32768, 65536]],
             'inv_inertia': 157286, 'spin_half': -15, 'random_start': True, 'finishing': True,
             'tint': True, 'start_pose': 64 * 123}
    found.update(changes)
    return found


def region(chunk, pieces):
    box = [0.0, 0.0, 30.0, 20.0]
    return {'chunk_id': chunk, 'activation_bounds': box, 'camera_bounds': box, 'door_debris': pieces,
            'statics': {'door_debris': 0, 'particle_bank': -1, 'grass_impact': -1}}


def sample_bank():
    a, b = piece(5, 3, 40), piece(4, 3, 41, spin_half=8, random_start=False, finishing=False, tint=False, start_pose=0)
    rows = [region(1, [a, b]), region(2, [dict(b, frame=7)]), region(3, [])]
    return wm.encode_scene(SCENE, rows)[0]


def sections(bank):
    return {name: struct.unpack_from('<3I', bank, 64 + 12 * n)
            for n, name in enumerate(('regions', 'objects', 'polygons', 'points', 'indices'))}


def objects(bank):
    offset, count, stride = sections(bank)['objects']
    return [struct.unpack_from('<IIHH4i2I3i', bank, offset + n * stride) for n in range(count)]


class FragmentBankTests(unittest.TestCase):
    def test_each_piece_is_written_once_at_the_head_of_the_first_region_in_owner_order(self):
        found = [o for o in objects(sample_bank()) if o[2] == wm.KINDS['fragment']]
        self.assertEqual([o[0] for o in found], [4, 5], 'sorted by owner state, then source id')
        # The head of the first region: object indexes 0 and 1.
        self.assertEqual([o[2] for o in objects(sample_bank())[:2]], [19, 19])

    def test_a_piece_packs_owner_pose_spin_flags_slots_and_payload(self):
        plain, rock = [o for o in objects(sample_bank()) if o[2] == 19]
        source, state, kind, flags, *rest = rock
        bounds, polygon_first, polygon_count, extra = rest[:4], rest[4], rest[5], rest[6:9]
        self.assertEqual((source, state & 0xFF, state >> 8), (5, 3, 64 * 123))
        self.assertEqual(flags, (-15 & 0x3F) | 64 | 128 | 256)
        self.assertEqual(list(bounds), [65536 * 10, -65536 * 3, 1000, -2000])
        self.assertEqual((polygon_count, tuple(extra)), (1, (-65536, 65536, 157286)))
        self.assertEqual(plain[3], 8, 'authored pose, no finishing, no tint: the spin alone')
        self.assertEqual(plain[1] >> 8, 0)

    def test_a_region_maps_only_the_pieces_it_can_draw_to_its_own_frames(self):
        bank = sample_bank()
        found = objects(bank)
        indices_at, _, stride = sections(bank)['indices']
        maps = []
        for o in found:
            if o[2] == wm.KINDS['fragment_frames']:
                word = o[-3] & 0xFFFFFFFF
                first, count = word & 0xFFFF, word >> 16
                maps.append(list(struct.unpack_from(f'<{count}H', bank, indices_at + first * stride)))
        # Region 1 draws both (piece 4 is index 0, piece 5 index 1), region 2 one, region 3 none.
        self.assertEqual(maps, [[1, 40, 0, 41], [0, 7]])

    def test_the_same_piece_with_a_different_shape_in_two_regions_is_an_error(self):
        a = piece(5, 3, 40)
        with self.assertRaisesRegex(ValueError, 'differs between regions'):
            wm.encode_scene(SCENE, [region(1, [a]), region(2, [dict(a, scale=[65536, 65536])])])

    def test_values_the_fields_cannot_hold_are_errors(self):
        for change, reason in ((dict(spin_half=40), 'spin'), (dict(start_pose=23040), 'start pose'),
                               (dict(door_state=128), 'owner state'), (dict(inv_inertia=0), 'inertia'),
                               (dict(polygon=[[0, 0]] * 9), 'polygon')):
            with self.subTest(reason):
                with self.assertRaisesRegex(ValueError, reason):
                    wm.encode_scene(SCENE, [region(1, [piece(5, 3, 40, **change)])])

    def test_the_golden_bank_is_what_the_encoder_writes(self):
        # Regenerate with: python3 tests/test_world_metadata_fragments.py --regenerate
        self.assertEqual(GOLDEN.read_bytes(), sample_bank())


if __name__ == '__main__':
    if sys.argv[1:] == ['--regenerate']:
        GOLDEN.write_bytes(sample_bank())
    else:
        unittest.main()
