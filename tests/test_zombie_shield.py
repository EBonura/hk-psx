"""Checks for the Zombie Shield's clip order and generated spec.

The recognizer is a structural read of the installed source and is covered by
running the cook. What is worth pinning here is the clip array, because it is
the part with a wrong answer that looks right: `ActorController::ZombieShield`
carries twenty clip ids in one array and `zombie_shield::Clip::slot` indexes
them by position, so swapping two names in the cooker would make every Shield in
the game play the wrong art at the right moment, with nothing anywhere to
complain about it. Naming both ends and comparing them is the only check that
catches that.
"""
import re
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))
import zombie_shield
from actors import actor_placements, generated_actor_specs

CONTROLLER = ROOT / 'shared/hk-sim/src/zombie_shield.rs'


def rust_clips():
    """The `Clip` variants of the controller, in declaration order."""
    text = CONTROLLER.read_text()
    body = re.search(r'pub enum Clip \{(.*?)\n\}', text, re.S)
    if body is None:
        raise AssertionError('zombie_shield.rs no longer declares a Clip enum')
    return [name for name in re.findall(r'^\s{4}(\w+),$', body.group(1), re.M)]


class ClipOrderTests(unittest.TestCase):
    def test_the_cooked_slots_are_the_controller_slots_in_the_same_order(self):
        variants = rust_clips()
        # Walk and Turn are the shared ActorSpec slots; `Clip::slot` returns
        # None for those two and `index - 2` for the rest.
        self.assertEqual(variants[:2], ['Walk', 'Turn'])
        expected = ['ShieldFront', 'ShieldTop', 'BumpFront', 'BumpTop', 'UnshieldFront',
                    'UnshieldTop', 'A1Antic', 'A1Lunge', 'A1Slash', 'A1Cooldown', 'A3Antic',
                    'A3Lunge1', 'A3Slash1', 'A3Cooldown1', 'A3Lunge2', 'A3Cooldown2',
                    'A3Lunge3', 'A3Slash3', 'A3Cooldown3']
        self.assertEqual(variants[2:], ['Idle'] + expected)
        self.assertEqual(len(zombie_shield.CLIP_SLOTS), len(variants) - 2)
        # Each cooked slot name is the snake case of the controller's variant,
        # so the two lists cannot drift apart silently.
        snake = [re.sub(r'(?<!^)(?=[A-Z])', '_', name).lower() for name in variants[2:]]
        self.assertEqual(list(zombie_shield.CLIP_SLOTS), snake)

    def test_the_controller_counts_exactly_the_slots_the_cooker_fills(self):
        count = re.search(r'pub const COUNT: usize = (\d+);', CONTROLLER.read_text())
        self.assertEqual(int(count.group(1)), len(zombie_shield.CLIP_SLOTS))

    def test_every_slot_names_a_clip_the_recognizer_requires(self):
        self.assertEqual(sorted(zombie_shield.SLOT_CLIPS), sorted(zombie_shield.CLIP_SLOTS))
        for slot, name in zombie_shield.SLOT_CLIPS.items():
            self.assertIn(name, zombie_shield.CLIPS, slot)
        # Walk and Turn are the Walker's own and are bound to the shared slots,
        # so the clip contract is exactly the twenty plus those two.
        self.assertEqual(len(zombie_shield.CLIPS), len(zombie_shield.CLIP_SLOTS) + 2)


def actor(**extra):
    control = {'kind': 'ZombieShield', 'walk_speed': 2., 'turn_ticks': 12,
               'turn_cooldown_ticks': 60, 'initial_direction': -1,
               'attack_bounds_q16': [-278200, 62259, 278200, 608174]}
    record = {'source': 'level54:2071', 'spec_source_id': 2071, 'movement_supported': True,
              'position': (32.54, 7.57), 'health': 15,
              'health_manager': {'invincible': 0, 'hasSpecialDeath': 0, 'hasAlternateHitAnimation': 0,
                                 'invincibleFromDirection': 0, 'damageOverride': 0},
              'colliders': [{'bounds': [31.6, 6.3, 32.8, 8.0]}],
              'walk_clip': 3, 'turn_clip': 4, 'movement_control': control,
              'DamageHero': {'damageDealt': 1},
              'Recoil': {'recoilSpeedBase': 10., 'recoilDuration': .15}}
    record.update({slot + '_clip': 10 + index for index, slot in enumerate(zombie_shield.CLIP_SLOTS)})
    record.update(extra)
    return record


class ZombieShieldSpecTests(unittest.TestCase):
    def test_the_spec_carries_the_clip_array_and_the_attack_trigger(self):
        region = {'actors': [actor()]}
        text = generated_actor_specs(region)[0]
        clips = ','.join(str(10 + index) for index in range(len(zombie_shield.CLIP_SLOTS)))
        self.assertIn('hk_sim::ActorController::ZombieShield {clips:[' + clips + ']', text)
        self.assertIn('attack:[-278200,62259,278200,608174]}', text)
        # No corpse is presented, and the walk fields ride along unread.
        self.assertIn('corpse:None', text)
        placement = actor_placements(region)[0]
        self.assertEqual((placement['initial_direction'], placement['random_start_direction']),
                         (-1, False))

    def test_a_shield_missing_one_cooked_clip_is_refused_rather_than_shifted(self):
        record = actor()
        del record['a3_cooldown2_clip']
        with self.assertRaisesRegex(ValueError, 'Zombie Shield is missing cooked clips'):
            generated_actor_specs({'actors': [record]})

    def test_the_art_bindings_name_a_clip_for_every_slot_and_the_two_shared_ones(self):
        bindings = {'walk': 'Walk', 'turn': 'Turn'}
        bindings.update({slot: zombie_shield.SLOT_CLIPS[slot] for slot in zombie_shield.CLIP_SLOTS})
        # cook.py fills `actor[kind + '_clip']` for each binding key, which is
        # what generated_actor_specs then reads back by the same name.
        self.assertEqual(sorted(bindings), sorted(['walk', 'turn'] + list(zombie_shield.CLIP_SLOTS)))
        self.assertEqual(sorted(set(bindings.values())), sorted(zombie_shield.CLIPS))


if __name__ == '__main__':
    unittest.main()
