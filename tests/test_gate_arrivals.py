"""Where the Knight stands after a gate: on the ground under the destination gate, never inside terrain.

`HeroController.EnterScene` places a Knight entering by a left or right gate at `FindGroundPointY`
below the gate's height, and one entering by a door at `FindGroundPoint(gate position)`: a ray of up to
10 units straight down onto layer 8, plus the hero collider's half height. The cook's spawn used the
door's own height, 0.73 inside Crossroads_06's top platform, so a Knight leaving the Ancestral Mound
dropped into a sealed pocket under it. These tests resolve every gate of the cooked report and check
the arrival of each standing entry against the terrain of its scene.
"""
import json
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))

REPORT = ROOT / 'data/regions.json'
STANDING = ('left', 'right', 'door')


@unittest.skipUnless(REPORT.exists(), 'needs a cooked data/regions.json')
class GateArrivalTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        import world
        cls.world = world
        cls.report = json.loads(REPORT.read_text())
        cls.by_id = {s['scene_id']: s for s in cls.report['scenes']}

    def arrivals(self):
        """(source scene, resolved gate, destination gate) for every gate that resolves."""
        for scene in self.report['scenes']:
            gates = {g['source']: g for g in scene['gates']}
            for resolved in self.world.resolve_gates(self.report, self.report['regions'], scene):
                gate = gates[resolved['source']]
                target = self.by_id[resolved['target_scene']]
                destination = next(g for g in target['gates'] if g['name'] == gate['entry_point'])
                yield scene, resolved, destination, target

    def test_every_standing_entry_stands_on_the_ground(self):
        checked = 0
        for scene, resolved, destination, target in self.arrivals():
            if not destination['name'].startswith(STANDING):
                continue
            x, y = resolved['spawn']
            ground = self.world.entry_ground_y(target['file'], x, y)
            where = f"{scene['scene_name']} -> {target['scene_name']} {destination['name']}"
            self.assertIsNotNone(ground, where)
            self.assertAlmostEqual(y, ground, delta=0.02, msg=where)
            checked += 1
        self.assertGreater(checked, 100)

    def test_the_mound_exit_stands_on_the_top_platform(self):
        for scene, resolved, destination, target in self.arrivals():
            if scene['scene_name'] == 'Crossroads_ShamanTemple' and target['scene_name'] == 'Crossroads_06':
                self.assertEqual(destination['name'], 'door1')
                self.assertGreater(resolved['spawn'][1], 30.3)
                return
        self.fail('the Mound has no gate to Crossroads_06')


if __name__ == '__main__':
    unittest.main()
