"""Pin the extracted False Knight numbers and keep host and guest in step.

These checks need no retail fixture: they compare host/false_knight.py's source
contract against the literals below and against the constants the guest actually
runs in shared/hk-sim/src/false_knight.rs. Re-extracting from a changed source
is host/false_knight.py's own job, which raises rather than rewrites.
"""
import re
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))
import false_knight as fk


def rust_constants(path):
    """Every `pub const NAME: type = value;` in a module, as ints or int lists."""
    text = path.read_text()
    found = {}
    for match in re.finditer(r'pub const (\w+)\s*:\s*[^=]+=\s*([^;]+);', text):
        name, raw = match.group(1), match.group(2).strip()
        raw = re.sub(r'//.*', '', raw)
        try:
            found[name] = eval(raw.replace('[', '[').replace('ONE', '65536'), {'__builtins__': {}}, {})
        except Exception:
            continue
    return found


class FalseKnightSourceContract(unittest.TestCase):
    """The numbers the extractor asserts against the installed source."""

    def test_health_and_phase_structure(self):
        self.assertEqual(fk.BODY_HEALTH, 65)
        self.assertEqual(fk.HEAD_HEALTH, 40)
        self.assertEqual(fk.BODY_INVULNERABLE_TIME, .25)
        self.assertEqual(fk.HEAD_INVULNERABLE_TIME, .15)
        self.assertEqual(fk.CONTACT_DAMAGE, 1)
        self.assertEqual(fk.STAGGERS_TO_DEATH, 3)
        self.assertEqual(fk.RAGE_SLAMS, 8)
        # Neither HealthManager pays Geo; the reward is the PlayerData set.
        self.assertEqual(fk.GEO_DROPS, {'small': 0, 'medium': 0, 'large': 0})
        self.assertEqual(fk.PLAYER_DATA['falseKnightDefeated'], True)
        self.assertEqual(fk.PLAYER_DATA['killsFalseKnight'], 0)
        self.assertIn('openedMapperShop', fk.PLAYER_DATA)

    def test_phase_table_is_the_three_source_stages(self):
        self.assertEqual(len(fk.PHASES), 3)
        self.assertEqual(fk.PHASES[0]['idle'], (1., 1.))
        self.assertEqual(fk.PHASES[0]['jump_barrels'], (0, 0))
        self.assertEqual(fk.PHASES[0]['slam_barrels'], (0, 0))
        self.assertEqual(fk.PHASES[1]['jump_barrels'], (2, 3))
        self.assertEqual(fk.PHASES[1]['slam_barrels'], (2, 3))
        self.assertEqual(fk.PHASES[2]['jump_barrels'], (2, 2))
        self.assertEqual(fk.PHASES[2]['slam_barrels'], (3, 4))

    def test_attack_budgets_and_distances(self):
        self.assertEqual(fk.TURNS_BEFORE_ATTACK, 3)
        self.assertEqual(fk.JUMP_ATTACKS_IN_A_ROW, 3)
        self.assertEqual(fk.SLAMS_IN_A_ROW, 2)
        self.assertEqual(fk.RUN_TRIGGER_DISTANCE, 21.)
        self.assertEqual(fk.RUN_STOP_DISTANCE, 14.)
        self.assertEqual(fk.SLAM_SKIP_JUMP_DISTANCE, 12.)
        self.assertEqual(fk.WALL_RAY_DISTANCE, 8.)
        self.assertEqual(fk.FALL_RAY_DISTANCE, 9.5)
        self.assertEqual(fk.MOVE_WEIGHTS, {'SMASH': 1., 'JUMP ATTACK': 1., 'JUMP': 1.})

    def test_clip_inventory_matches_the_shared_library(self):
        self.assertEqual(len(fk.CLIPS), 34)
        self.assertEqual(fk.CLIPS['Idle'], (5, 12., 0, 0))
        self.assertEqual(fk.CLIPS['Jump Antic'], (3, 10., 2, 0))
        self.assertEqual(fk.CLIPS['Stun Roll'], (5, 12., 1, 2))
        self.assertEqual(fk.CLIPS['Death Head 1'], (10, 10., 2, 0))

    def test_arena_contract(self):
        self.assertEqual(fk.ARENA_END_WAIT, 2.)
        self.assertEqual(fk.ADDITIVE, {'scene': 'Crossroads_10_boss',
                                       'alt_scene': 'Crossroads_10_boss_defeated',
                                       'gate_flag': 'falseKnightDefeated', 'gate_value': 0})
        self.assertEqual(fk.SCENE_FILE, 'level48')
        self.assertEqual(fk.ROOM_FILE, 'level46')


class GuestAgreesWithTheExtractor(unittest.TestCase):
    """The tick and Q16 constants the guest runs must be the derived ones."""

    def setUp(self):
        self.derived = fk.generated_false_knight_params()
        self.rust = rust_constants(ROOT / 'shared/hk-sim/src/false_knight.rs')
        self.arena = rust_constants(ROOT / 'shared/hk-sim/src/boss.rs')

    def test_every_derived_constant_reaches_the_guest_unchanged(self):
        missing = [key for key in self.derived if key != 'ARENA_END_WAIT_TICKS' and key not in self.rust]
        self.assertEqual(missing, [], 'guest module is missing derived constants')
        for key, value in self.derived.items():
            if key == 'ARENA_END_WAIT_TICKS':
                continue
            self.assertEqual(self.rust[key], value, f'{key} drifted from the source contract')

    def test_the_guest_adds_no_unbacked_numbers(self):
        extra = sorted(set(self.rust) - set(self.derived))
        self.assertEqual(extra, [], 'guest constants with no source derivation')

    def test_arena_wait_is_shared_between_the_two_modules(self):
        self.assertEqual(self.arena['END_WAIT_TICKS'], self.derived['ARENA_END_WAIT_TICKS'])


class DigestRefusesChangedStateMachines(unittest.TestCase):
    """A recognizer that cannot see a change is not a recognizer."""

    @staticmethod
    def fixture():
        return {
            'name': 'FalseyControl', 'startState': 'State 4',
            'variables': {'floatVariables': [{'name': 'Idle Min', 'value': 1.}],
                          'gameObjectVariables': [{'name': 'Head', 'value': {'m_FileID': 0, 'm_PathID': 48}}]},
            'globalTransitions': [{'fsmEvent': {'name': 'STUN'}, 'toState': 'Check Direction'}],
            'states': [{'name': 'Idle',
                        'transitions': [{'fsmEvent': {'name': 'FINISHED'}, 'toState': 'Move Choice'}],
                        'actionData': {'actionNames': ['Tk2dPlayAnimation'], 'actionEnabled': [1],
                                       'actionStartIndex': [0], 'paramName': ['clipName'],
                                       'paramDataType': [18], 'paramByteDataSize': [0], 'byteData': [],
                                       'fsmStringParams': [{'useVariable': 0, 'value': 'Idle'}]}}],
        }

    def test_object_references_do_not_change_the_digest(self):
        base = self.fixture()
        moved = self.fixture()
        moved['variables']['gameObjectVariables'][0]['value']['m_PathID'] = 999
        self.assertEqual(fk.fsm_digest(base), fk.fsm_digest(moved))

    def test_a_changed_parameter_changes_the_digest(self):
        base = self.fixture()
        for mutate in (
                lambda f: f['variables']['floatVariables'][0].__setitem__('value', .5),
                lambda f: f['states'][0]['actionData']['fsmStringParams'][0].__setitem__('value', 'Run'),
                lambda f: f['states'][0]['actionData'].__setitem__('actionEnabled', [0]),
                lambda f: f['states'][0]['transitions'][0].__setitem__('toState', 'Run'),
                lambda f: f['states'][0]['actionData'].__setitem__('byteData', [1, 2, 3]),
                lambda f: f.__setitem__('startState', 'Idle'),
        ):
            changed = self.fixture()
            mutate(changed)
            self.assertNotEqual(fk.fsm_digest(base), fk.fsm_digest(changed))

    def test_ignored_placement_variables_are_excluded(self):
        base = self.fixture()
        base['variables']['boolVariables'] = [{'name': 'Start Closed', 'value': 0}]
        other = self.fixture()
        other['variables']['boolVariables'] = [{'name': 'Start Closed', 'value': 1}]
        self.assertNotEqual(fk.fsm_digest(base), fk.fsm_digest(other))
        self.assertEqual(fk.fsm_digest(base, ('Start Closed',)),
                         fk.fsm_digest(other, ('Start Closed',)))


class GeneratedConstantsStayBounded(unittest.TestCase):
    def test_a_tick_beyond_u16_is_refused(self):
        original = fk.WAITS['Opened']
        try:
            fk.WAITS['Opened'] = 20000.
            with self.assertRaisesRegex(ValueError, 'exceeds its guest representation'):
                fk.generated_false_knight_params()
        finally:
            fk.WAITS['Opened'] = original


if __name__ == '__main__':
    unittest.main()
