import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('lifeblood_coverage', Path(__file__).resolve().parents[1] / 'tools/validate_lifeblood.py')
coverage = importlib.util.module_from_spec(spec)
spec.loader.exec_module(coverage)


def state(active=0, struck=0, granted=0, blue=0, health=4, deaths=0):
    return dict(zip(coverage.FIELDS, (1, active, struck, granted, blue, health, deaths)))


class LifebloodCoverageTests(unittest.TestCase):
    def test_requires_earned_masks_and_both_damage_pools(self):
        route = [state(active=2), state(struck=2),
                 state(struck=2, granted=2, blue=2),
                 state(struck=2, granted=2, blue=1),
                 state(struck=2, granted=2),
                 state(struck=2, granted=2, health=3)]
        self.assertTrue(coverage.check_states(route)['passed'])
        self.assertFalse(coverage.check_states(route[:-1])['passed'])
        self.assertFalse(coverage.check_states(route[2:])['passed'])

    def test_rejects_white_damage_while_checking_blue_absorption(self):
        with self.assertRaisesRegex(ValueError, 'Ordinary health'):
            coverage.check_states([state(struck=2, granted=2, blue=2),
                                   state(struck=2, granted=2, blue=1, health=3)])

    def test_death_reset_does_not_count_as_absorption(self):
        with self.assertRaisesRegex(ValueError, 'Death/reset'):
            coverage.check_states([state(struck=2, granted=2, blue=2),
                                   state(health=5, deaths=1)])
