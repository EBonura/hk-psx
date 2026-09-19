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


class AdditiveMergeTests(unittest.TestCase):
    """The id and reference rewrite that lets one scene hold two files.

    Synthetic: both sides of the rewrite are pure functions of a tree, so this
    needs no retail fixture. What it guards is the failure that would be silent,
    which is a reference that still points at the wrong file's object.
    """

    def test_own_file_references_shift_and_external_ones_are_remapped(self):
        from scene import retarget
        tree = {'m_GameObject': {'m_FileID': 0, 'm_PathID': 40},
                'm_Script': {'m_FileID': 3, 'm_PathID': 2049},
                'library': {'m_FileID': 2, 'm_PathID': 9},
                'nothing': {'m_FileID': 0, 'm_PathID': 0},
                'children': [{'component': {'m_FileID': 0, 'm_PathID': 7}}],
                'box': {'x': 1.0, 'y': 2.0}}
        retarget(tree, 100000, {2: 20, 3: 4})
        self.assertEqual(tree['m_GameObject'], {'m_FileID': 0, 'm_PathID': 100040})
        self.assertEqual(tree['children'][0]['component'], {'m_FileID': 0, 'm_PathID': 100007})
        self.assertEqual(tree['m_Script'], {'m_FileID': 4, 'm_PathID': 2049})
        self.assertEqual(tree['library'], {'m_FileID': 20, 'm_PathID': 9})
        # A null reference stays null, and a plain two-field vector is not a PPtr.
        self.assertEqual(tree['nothing'], {'m_FileID': 0, 'm_PathID': 0})
        self.assertEqual(tree['box'], {'x': 1.0, 'y': 2.0})

    def test_a_merged_object_reports_its_own_file(self):
        from scene import Scene, ADDITIVE_ID_BASE
        sc = Scene.__new__(Scene)
        sc._origins = ((ADDITIVE_ID_BASE, 'level48'), (0, 'level46'))
        self.assertEqual(sc.sid(325), 'level46:325')
        self.assertEqual(sc.sid(ADDITIVE_ID_BASE + 325), 'level48:325')


@unittest.skipUnless((ROOT / '.hkpsx/doctor.json').is_file(), 'needs the installed game')
class PlacedFalseKnightTests(unittest.TestCase):
    """Crossroads_10 with its additive boss scene merged in."""

    @classmethod
    def setUpClass(cls):
        from source import Source
        from scene import Scene
        cls.source = Source()
        cls.scene = Scene(cls.source, fk.ROOM_FILE)

    def test_the_room_merges_the_boss_scene_the_loader_selects(self):
        # falseKnightDefeated is false on a fresh save, so BossLoader takes
        # sceneNameToLoad rather than the defeated variant.
        self.assertEqual(self.scene.additive, ('level48',))

    def test_the_boss_is_admitted_and_the_pre_battle_zombies_are_not(self):
        from actors import actor_sources
        actors = {a['name']: a for a in actor_sources(self.scene)}
        boss = actors['False Knight New']
        self.assertTrue(boss['movement_supported'])
        self.assertEqual(boss['source'], 'level48:325')
        control = boss['movement_control']
        self.assertEqual(control['kind'], 'FalseKnight')
        # Six of the thirty-four clips, which is what Crossroads_10's room-pack
        # byte budget holds; the guest shows the rest as the nearest of them.
        self.assertEqual(control['art_bindings'], {
            'walk': 'Idle', 'turn': 'Turn', 'jump_antic': 'Jump Antic',
            'land': 'Land', 'stun_opened': 'Stun Opened', 'attack': 'Attack'})
        # `Turn L` writes the scale negative, so the placement starts facing
        # left, and the cook's absolute scale turns that into +1 for the draw.
        self.assertFalse(control['facing_right'])
        self.assertEqual(control['initial_direction'], 1)
        self.assertEqual(control['turn_ticks'], 10)
        # The body is killable: reaching zero restores it to 65 and staggers it,
        # and the exposed Head is what a phase costs.
        self.assertFalse(control['invincible'])
        self.assertTrue(control['special_death'])
        self.assertEqual(control['head_health'], 40)
        # `Battle Scene`'s trigger: a one-unit curtain across the arena floor,
        # and the only thing the boss tests while it is dormant.
        x0, y0, x1, y1 = control['arena_trigger_world']
        self.assertAlmostEqual(x1 - x0, 1.)
        self.assertAlmostEqual(y1 - y0, 19.592126846313477)
        self.assertAlmostEqual(x0, 13.5)
        for name in ('Zombie Barger', 'Zombie Runner', 'Zombie Hornhead'):
            self.assertFalse(actors[name]['movement_supported'], name)
            self.assertIn('additive scene', actors[name]['movement_error'])
        # The Head carries no FalseyControl, so it stays a record either way.
        self.assertFalse(actors['Head']['movement_supported'])

    def test_the_merge_brings_the_arena_floor_the_room_does_not_have(self):
        """Every layer-8 floor under the boss belongs to the additive scene."""
        from scene import Scene
        def floors(scene):
            out = set()
            for i, (kind, tree) in scene.objects.items():
                if kind != 'BoxCollider2D' or not tree['m_Enabled'] or tree['m_IsTrigger']:
                    continue
                gid = tree['m_GameObject']['m_PathID']
                if not scene.active(gid) or scene.gos[gid]['m_Layer'] != 8:
                    continue
                top = scene.point(gid, tree['m_Offset']['x'], tree['m_Offset']['y'] + tree['m_Size']['y'] / 2)
                half = tree['m_Size']['x'] / 2
                if top[0] - half <= 26.07 <= top[0] + half and 26 <= top[1] <= 27:
                    out.add(scene.sid(i))
            return out
        self.assertEqual(floors(Scene(self.source, fk.ROOM_FILE, merge_additive=False)), set())
        self.assertIn('level48:154', floors(self.scene))
