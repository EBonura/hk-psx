"""A door the cook does not resolve is listed with its reason, never silently gone (host/world.py resolve_gates)."""
import json
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))
import world


def gate(name, target, entry, **changes):
    found = {'source': f'level1:{name}', 'name': name, 'enabled': True, 'target_scene': target, 'entry_point': entry,
             'trigger_bounds': [0, 0, 1, 1], 'position': [1.0, 1.0, 0.0]}
    found.update(changes)
    return found


class GateCensusTests(unittest.TestCase):
    def test_each_unresolved_door_names_its_reason(self):
        here = {'scene_id': 0, 'scene_name': 'A', 'file': 'level1', 'gates': [
            gate('top1', 'C', 'bot1'),
            gate('left1', 'B', 'right1', enabled=False),
            gate('right1', 'B', 'nowhere'),
            gate('bot1', 'B', 'left1', trigger_bounds=None),
        ]}
        del here['gates'][3]['trigger_bounds']
        there = {'scene_id': 1, 'scene_name': 'B', 'file': 'level2', 'gates': [gate('left1', 'A', 'right1')]}
        report = {'scenes': [here, there], 'regions': []}
        self.assertEqual(world.resolve_gates(report, [], here), [])
        self.assertEqual([(d['name'], d['reason']) for d in here['dropped_gates']], [
            ('top1', 'target scene not admitted'),
            ('left1', 'disabled'),
            ('right1', "entry point 'nowhere' matches 0 gates of the target"),
            ('bot1', 'no trigger bounds'),
        ])
        there['resolved_gates'] = []
        here['resolved_gates'] = []
        there['dropped_gates'] = []
        line = world.census_line([here, there])
        self.assertIn('4 dropped', line)
        self.assertIn('unadmitted targets: C', line)


if __name__ == '__main__':
    unittest.main()
