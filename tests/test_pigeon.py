"""Checks for the Pigeon's clip order, its generated spec and its constants.

The recognizer is a structural read of the installed source and is covered by
running the cook. Three things are worth pinning here instead.

The clip array, for the reason `tests/test_zombie_shield.py` pins the Shield's:
`ActorController::Pigeon` carries its extra idle loops in one array that
`pigeon::Clip::slot` indexes by position, so swapping two names in the cooker
would play the wrong loop with nothing anywhere to complain about it.

The constants, because `host/pigeon.py` proves the placement's own geometry and
its FSM's own numbers against a copy of what `shared/hk-sim/src/pigeon.rs`
carries. Two copies of a number are only evidence while they agree, so the
agreement is checked rather than assumed.

And the trigger body, because the Pigeon is the first family whose whole hurt
surface is a trigger and the collider filter in `generated_actor_records` used
to drop those unconditionally.
"""
import re
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))
import rustsrc
import pigeon
from actors import actor_placements, generated_actor_specs

CONTROLLER = ROOT / 'shared/hk-sim/src/pigeon.rs'
ONE = 65536


def rust_clips():
    """The `Clip` variants of the controller, in declaration order."""
    try:
        return rustsrc.enum_variants(CONTROLLER, 'Clip')
    except KeyError:
        raise AssertionError('pigeon.rs no longer declares a Clip enum') from None


def rust_const(name):
    """One `pub const` of the controller, as a list of evaluated integers."""
    found = rustsrc.consts(CONTROLLER, one=ONE).get(name)
    if found is None:
        raise AssertionError(f'pigeon.rs no longer declares {name}')
    return list(found) if isinstance(found, (list, tuple)) else [found]


class ClipOrderTests(unittest.TestCase):
    def test_the_cooked_slots_are_the_controller_slots_in_the_same_order(self):
        variants = rust_clips()
        # Idle1 and Fly are the shared ActorSpec slots; `Clip::slot` returns
        # None for those two and `index - 2` for the rest.
        self.assertEqual(variants[:2], ['Idle1', 'Fly'])
        self.assertEqual(variants[2:], ['Idle2', 'Idle3'])
        self.assertEqual(len(pigeon.CLIP_SLOTS), len(variants) - 2)
        snake = [re.sub(r'(?<!^)(?=[A-Z])', '_', name).lower() for name in variants[2:]]
        self.assertEqual(list(pigeon.CLIP_SLOTS), snake)

    def test_the_controller_counts_exactly_the_slots_the_cooker_fills(self):
        self.assertEqual(rustsrc.const_int(CONTROLLER, 'COUNT'), len(pigeon.CLIP_SLOTS))

    def test_every_slot_names_a_clip_the_recognizer_requires(self):
        self.assertEqual(sorted(pigeon.SLOT_CLIPS), sorted(pigeon.CLIP_SLOTS))
        for slot, name in pigeon.SLOT_CLIPS.items():
            self.assertIn(name, pigeon.CLIPS, slot)
        # Idle 01 and Fly are bound to the two shared slots, so the clip
        # contract is exactly the two carried ones plus those.
        self.assertEqual(len(pigeon.CLIPS), len(pigeon.CLIP_SLOTS) + 2)


class ConstantTests(unittest.TestCase):
    def test_the_hero_range_circle_is_the_one_the_controller_reads(self):
        centre = rust_const('HERO_RANGE_CENTER')
        radius = rust_const('HERO_RANGE_RADIUS')
        self.assertEqual(tuple(centre + radius), pigeon.HERO_RANGE_Q16)

    def test_the_flight_numbers_are_the_ones_the_recognizer_proves(self):
        self.assertEqual(rust_const('TAKEOFF_RISE'), [pigeon.TAKEOFF_RISE_Q16])
        self.assertEqual(rust_const('RISE_FORCE'), pigeon.RISE_FORCE_Q16)
        self.assertEqual(rust_const('SIDE_FORCE'), pigeon.SIDE_FORCE_Q16)
        self.assertEqual(rust_const('LIFE_TICKS'), [pigeon.LIFE_TICKS])
        self.assertEqual(rust_const('START_FRAMES'), pigeon.START_FRAMES)

    def test_the_smaller_circles_really_do_lie_inside_the_carried_one(self):
        # This is what licenses the guest reading one range child of the two.
        for inner in (pigeon.ENEMY_RANGE_Q16, pigeon.WAKER_Q16):
            pigeon._containment(inner, pigeon.HERO_RANGE_Q16)
        with self.assertRaisesRegex(ValueError, 'is not inside the admitted'):
            pigeon._containment((0, 0, pigeon.HERO_RANGE_Q16[2] + 1), pigeon.HERO_RANGE_Q16)


def actor(**extra):
    control = {'kind': 'Pigeon', 'initial_direction': -1, 'no_corpse': True,
               'trigger_body': True}
    record = {'source': 'level128:7965', 'spec_source_id': 7965, 'movement_supported': True,
              'position': (143.517, 19.853, 0.0), 'health': 1,
              'health_manager': {'invincible': 0, 'hasSpecialDeath': 0, 'hasAlternateHitAnimation': 0,
                                 'invincibleFromDirection': 0, 'damageOverride': 0},
              # The one collider the family has, and it is a trigger.
              'colliders': [{'trigger': True, 'bounds': [143.34, 20.018, 143.879, 20.743]}],
              'walk_clip': 3, 'turn_clip': 4, 'movement_control': control}
    record.update({slot + '_clip': 10 + index for index, slot in enumerate(pigeon.CLIP_SLOTS)})
    record.update(extra)
    return record


class PigeonSpecTests(unittest.TestCase):
    def test_the_spec_carries_the_two_extra_idle_loops_and_nothing_else(self):
        region = {'actors': [actor()]}
        text = generated_actor_specs(region)[0]
        clips = ','.join(str(10 + index) for index in range(len(pigeon.CLIP_SLOTS)))
        self.assertIn('hk_sim::ActorController::Pigeon {clips:[' + clips + ']}', text)
        # No corpse, no contact damage, no recoil and no Dream Nail soul: the
        # source object carries none of the components any of those come from.
        self.assertIn('corpse:None', text)
        self.assertIn('contact_damage:0', text)
        self.assertIn('recoil_speed:0', text)
        self.assertIn('dream_soul:0', text)
        # The trigger box is the hurt surface, measured off the placement.
        self.assertIn('bounds:[-11600,10813,23724,58327]', text)

    def test_a_solid_body_is_not_silently_preferred_over_the_trigger(self):
        # A Pigeon that grew a solid collider is a different object: the filter
        # takes the class the controller asked for and finds none.
        record = actor(colliders=[{'trigger': False, 'bounds': [0., 0., 1., 1.]}])
        with self.assertRaisesRegex(ValueError, 'unsupported distinct body colliders'):
            generated_actor_specs({'actors': [record]})

    def test_a_pigeon_missing_one_cooked_loop_is_refused_rather_than_shifted(self):
        record = actor()
        del record['idle3_clip']
        with self.assertRaisesRegex(ValueError, 'Pigeon is missing cooked clips'):
            generated_actor_specs({'actors': [record]})

    def test_the_placement_carries_no_facing_the_controller_would_overwrite(self):
        placement = actor_placements({'actors': [actor()]})[0]
        # `Set Frame` flips the mirror on a coin and `Right`/`Left` replace it
        # with the flight direction, so -1 here is the authored pose and not a
        # claim about which way a running bird faces.
        self.assertEqual(placement['initial_direction'], -1)
        self.assertFalse(placement['random_start_direction'])
        self.assertFalse(placement['start_alert'])
        self.assertFalse(placement['start_right'])
        self.assertEqual(placement['rotation_quarter'], 0)

    def test_the_art_bindings_name_a_clip_for_every_slot_and_the_two_shared_ones(self):
        bindings = {'walk': 'Idle 01', 'turn': 'Fly'}
        bindings.update({slot: pigeon.SLOT_CLIPS[slot] for slot in pigeon.CLIP_SLOTS})
        # cook.py fills `actor[kind + '_clip']` for each binding key, which is
        # what generated_actor_specs then reads back by the same name.
        self.assertEqual(sorted(bindings), sorted(['walk', 'turn'] + list(pigeon.CLIP_SLOTS)))
        self.assertEqual(sorted(set(bindings.values())), sorted(pigeon.CLIPS))


if __name__ == '__main__':
    unittest.main()
