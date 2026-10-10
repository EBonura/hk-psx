"""Notice when a cooked region outgrows the guest's terrain edge cache.

`EDGE_CACHE` in game/src/world.rs bounds the filtered terrain table a region can
keep. A region past it used to lose the table altogether, which turned every
terrain query into a linear rescan of the bank's object table, per edge, per
body, per tick. One region of 699 was over, by one edge, and walking into it was
a guaranteed hang: the frame lost two VBlanks a tick, the catch-up loop could
never drain the pad queue, and the sampler faulted QueueFull.

That is fixed, so being over is now a graceful slowdown rather than a crash, and
refusing a region for it would be wrong. But on this port route positions are a
performance test, and the tail past the cap is still answered uncached, so a
second region crossing it is worth a person's attention rather than silence.
Nothing else looks: the cook does not warn and the build report does not count.
"""
import json
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

# Slot, scene and why this one is tolerated. Crossroads_10's arena floor view
# carries the False Knight's terrain plus, since every BG Control gate is baked,
# five gate colliders' edges. Its tail is answered uncached and the fight is on
# the disc, so the cost is known and paid.
ACCEPTED = {236: 'Crossroads_10'}


def cap():
    text = (ROOT / 'game/src/world.rs').read_text()
    found = re.search(r'const EDGE_CACHE\s*:\s*usize\s*=\s*(\d+);', text)
    if not found:
        raise ValueError('cannot read EDGE_CACHE out of game/src/world.rs')
    return int(found.group(1))


class EdgeCacheCapTests(unittest.TestCase):
    def setUp(self):
        report = ROOT / 'data/regions.json'
        if not report.is_file():
            self.skipTest('no cooked catalogue yet; the cook writes it')
        self.catalogue = json.loads(report.read_text())
        if self.catalogue.get('complete') is not True:
            self.skipTest('the cooked catalogue is unfinished; the cook this build runs will finish it')

    def test_only_the_known_region_outgrows_the_edge_cache(self):
        limit = cap()
        over = {slot: region['scene_name']
                for slot, region in enumerate(self.catalogue['regions'])
                if region.get('edges', 0) > limit}
        new = {slot: scene for slot, scene in over.items() if slot not in ACCEPTED}
        self.assertFalse(new, f'{len(new)} region(s) now pass the {limit}-edge cache and answer '
                               f'their tail uncached, which is a per-tick cost cliff in that view: '
                               f'{new}. Accept them here with a reason, or cook fewer edges.')
        self.assertEqual({slot: over[slot] for slot in over if slot in ACCEPTED},
                         {slot: scene for slot, scene in ACCEPTED.items() if slot in over},
                         'an accepted region moved scene')
