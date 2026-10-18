"""Refuse a cooked region that outgrows the guest's terrain edge table.

`EDGE_CACHE` in game/src/world.rs bounds the filtered terrain table a region
keeps, and every solver reads its terrain from such a table (a reader with a
second, uncached path made each solver generic over two reader types, 118 KB of
duplicated code, and was itself a cliff: one region once cooked a single edge
over the old 128 limit and walking into it hung the guest, the frame losing two
VBlanks a tick until the pad queue overflowed). A region past the table has no
slow path any more; `State::refresh_edges` asserts, and this test fails the
build first.
"""
import json
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
import sys
sys.path.insert(0, str(ROOT / 'host'))
import rustsrc

def cap():
    try:
        return rustsrc.const_int(ROOT / 'game/src/world.rs', 'EDGE_CACHE')
    except KeyError:
        raise ValueError('cannot read EDGE_CACHE out of game/src/world.rs') from None


class EdgeCacheCapTests(unittest.TestCase):
    def setUp(self):
        report = ROOT / 'data/regions.json'
        if not report.is_file():
            self.skipTest('no cooked catalogue yet; the cook writes it')
        self.catalogue = json.loads(report.read_text())
        if self.catalogue.get('complete') is not True:
            self.skipTest('the cooked catalogue is unfinished; the cook this build runs will finish it')

    def test_every_region_fits_the_edge_table(self):
        limit = cap()
        over = {slot: (region['scene_name'], region['edges'])
                for slot, region in enumerate(self.catalogue['regions'])
                if region.get('edges', 0) > limit}
        self.assertFalse(over, f'{len(over)} region(s) pass the {limit}-edge table: {over}. '
                               f'Raise EDGE_CACHE in game/src/world.rs (it sizes the Knight\'s table '
                               f'and every slot of the actors\' copies) or cook fewer edges.')
