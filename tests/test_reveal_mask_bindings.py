"""Reveal masks without the exact binary CLUT fade by gain, and say so."""
import struct
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'host'))
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tests'))
from region_delta import layout
from reveal_masks import bind_regions
from test_constant_textures import room


def bind(raw):
    region = {'chunk_id': 1, 'scene_id': 0, 'draws': 1}
    report = {'regions': [region]}
    controllers = {'controllers': [{'controller': 0, 'renderer_sources': ['f:1']}]}
    bind_regions(report, {0: controllers}, {1: ['f:1']}, {1: raw})
    return region


class RevealMaskBindingTests(unittest.TestCase):
    def test_binary_black_mask_binds_and_soft_mask_is_recorded(self):
        raw = room()
        self.assertEqual(bind(raw)['reveal_mask_bindings'], [{'controller': 0, 'draw': 0, 'renderer_source': 'f:1'}])
        soft = bytearray(raw)
        _, prefix, _, _ = layout(raw)
        struct.pack_into('<H', soft, prefix + 2 * 2, 0x4210)  # third CLUT word is no longer transparent black
        region = bind(bytes(soft))
        self.assertEqual(region['reveal_mask_bindings'], [{'controller': 0, 'draw': 0, 'renderer_source': 'f:1'}])
        self.assertEqual(region['reveal_mask_gain'][0]['reason'], 'reveal mask requires exact binary black palette')


if __name__ == '__main__':
    unittest.main()
