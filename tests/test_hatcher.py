"""Synthetic checks for the Hatcher cage reservation, without retail fixtures.

The recognizers themselves are structural reads of the installed source and are
covered by running the cook; what is worth pinning here is the arithmetic that
decides whether a scene's family is admitted at all, because that is the part
with a wrong answer that looks right: a cage that does not fit has to refuse the
whole family rather than quietly carry fewer babies than the source parks.
"""
import sys
import unittest
from pathlib import Path
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'host'))
import hatcher
from actors import actor_placements, generated_actor_specs


def scene(babies, hatchers, others, file='level57', tag=hatcher.CAGE_TAG, cages=1):
    """A scene with one tagged cage, `babies` parked in it and `hatchers` placed.

    Positions follow the source: the cage and everything in it sits at (100,
    100), outside every room, and the Hatchers stand inside level57's bounds.
    """
    gos, objects, transforms, go_transform, points = {}, {}, {}, {}, {}
    for index in range(cages):
        gid = 1 + index
        gos[gid] = {'m_Name': f'Hatcher Cage ({index})', 'm_Layer': 8, 'm_Tag': tag}
        transforms[gid] = {'m_GameObject': {'m_PathID': gid}, 'm_Children': [], 'm_Father': {'m_PathID': 0}}
        go_transform[gid] = gid
        points[gid] = (100., 100.)
    for index in range(babies):
        gid = 100 + index
        gos[gid] = {'m_Name': f'Hatcher Baby Spawner ({index})', 'm_Layer': 11, 'm_Tag': 0}
        transforms[gid] = {'m_GameObject': {'m_PathID': gid}, 'm_Children': [], 'm_Father': {'m_PathID': 1}}
        go_transform[gid] = gid
        points[gid] = (100., 100.)
        transforms[1]['m_Children'].append({'m_PathID': gid})
    for index in range(hatchers):
        gid = 200 + index
        gos[gid] = {'m_Name': f'Hatcher {index}', 'm_Layer': 11, 'm_Tag': 0}
        transforms[gid] = {'m_GameObject': {'m_PathID': gid}, 'm_Children': [], 'm_Father': {'m_PathID': 0}}
        go_transform[gid] = gid
        points[gid] = (10. + index, 20.)
        objects[1000 + index] = ('HealthManager', {'m_GameObject': {'m_PathID': gid}, 'm_Enabled': 1})
    stub = SimpleNamespace(gos=gos, objects=objects, transforms=transforms, go_transform=go_transform,
                           file=SimpleNamespace(name=file), active=lambda gid: True,
                           point=lambda gid: points[gid])
    # The probe that counts the scene's other supported actors is answered here
    # rather than run, so the arithmetic is the only thing under test.
    stub.hatcher_others = others
    return stub


class HatcherBudgetTests(unittest.TestCase):
    def test_a_cage_inside_both_bounds_is_admitted_whole(self):
        # Crossroads_19 as measured: two admitted actors, one Hatcher, fifteen
        # parked babies, which is 18 of the 20 animation slots a frame binds.
        self.assertEqual(hatcher.family_budget(scene(babies=15, hatchers=1, others=2)), 15)
        # Crossroads_27: three Hatchers over one shared cage of fifteen.
        self.assertEqual(hatcher.family_budget(scene(babies=15, hatchers=3, others=0)), 15)

    def test_a_cage_past_the_guest_pool_refuses_the_family_instead_of_shrinking(self):
        # Crossroads_22 as measured: twelve admitted Aspids, one Hatcher and a
        # cage of twenty-three is 36 of the 32 slots the scene has.
        with self.assertRaisesRegex(ValueError, 'cage of 23 needs 36 of the 32 guest actor slots'):
            hatcher.family_budget(scene(babies=23, hatchers=1, others=12))

    def test_a_cage_past_the_frame_budget_refuses_before_it_can_panic(self):
        # Inside the 32-slot pool but past the 20 animation slots prepare_draws
        # asserts on, which a cage is the only thing that gets a scene near.
        with self.assertRaisesRegex(ValueError, 'cage of 15 needs 21 of the 20 animation slots'):
            hatcher.family_budget(scene(babies=15, hatchers=1, others=5))

    def test_more_hatchers_than_the_runtime_releases_on_a_frame_are_refused(self):
        with self.assertRaisesRegex(ValueError, '5 Hatchers in one scene exceeds the 4 releases'):
            hatcher.family_budget(scene(babies=5, hatchers=5, others=0))

    def test_the_scene_needs_exactly_one_tagged_cage(self):
        with self.assertRaisesRegex(ValueError, 'exactly one Extra Tag cage, found 0'):
            hatcher.family_budget(scene(babies=0, hatchers=1, others=0, tag=0))
        with self.assertRaisesRegex(ValueError, 'exactly one Extra Tag cage, found 2'):
            hatcher.family_budget(scene(babies=1, hatchers=1, others=0, cages=2))

    def test_a_hatcher_outside_the_room_is_not_counted_as_a_placement(self):
        stub = scene(babies=15, hatchers=2, others=2)
        # Crossroads_22 parks its `Hatcher NP` copies at x 155, well past the
        # room. One of the two here joins them and stops being a placement.
        moved = dict({gid: stub.point(gid) for gid in stub.gos})
        moved[201] = (155.644, 18.691)
        stub.point = lambda gid: moved[gid]
        self.assertEqual(hatcher.family_budget(stub), 15)
        self.assertEqual(hatcher.placed_hatchers(stub), [200])

    def test_an_unpacked_scene_has_no_bounds_to_place_a_hatcher_against(self):
        with self.assertRaisesRegex(ValueError, 'outside the admitted scene table'):
            hatcher.scene_bounds(scene(babies=0, hatchers=0, others=0, file='level999'))

    def test_the_probe_excludes_the_family_it_is_measuring(self):
        stub = scene(babies=15, hatchers=1, others=0)
        stub.hatcher_others = -1
        with self.assertRaisesRegex(ValueError, 'excluded while its own scene budget is measured'):
            hatcher.family_budget(stub)


def actor(kind, **extra):
    record = {'source': f'level57:{5007 if kind == "Hatcher" else 5010}', 'spec_source_id': 5007,
              'movement_supported': True, 'position': (42., 34.), 'health': 20,
              'health_manager': {'invincible': 0, 'hasSpecialDeath': 0, 'hasAlternateHitAnimation': 0,
                                 'invincibleFromDirection': 0, 'damageOverride': 0},
              'colliders': [{'bounds': [41., 33., 43., 35.]}],
              'walk_clip': 8, 'turn_clip': 8, 'fire_clip': 9,
              'movement_control': {'kind': kind, 'start_alert': False},
              'DamageHero': {'damageDealt': 1}}
    record.update(extra)
    return record


class HatcherSpecTests(unittest.TestCase):
    def test_the_hatcher_spec_carries_its_fire_clip_and_the_placement_start_alert(self):
        region = {'actors': [actor('Hatcher')]}
        text = generated_actor_specs(region)[0]
        self.assertIn('hk_sim::ActorController::Hatcher {fire_clip:9}', text)
        self.assertIn('corpse:None', text)
        # The controller owns every velocity the body has.
        self.assertIn('hk_sim::WalkParams {speed:0,turn_ticks:0,turn_cooldown_ticks:0}', text)
        # `startAlert` is authored per placement, so it rides in the scene bank
        # and two Hatchers that differ only in it are still one linked type.
        self.assertIs(actor_placements(region)[0]['start_alert'], False)
        alert = {'actors': [actor('Hatcher', movement_control={'kind': 'Hatcher', 'start_alert': True})]}
        self.assertEqual(generated_actor_specs(alert), [text])
        self.assertIs(actor_placements(alert)[0]['start_alert'], True)

    def test_a_hatcher_without_a_cooked_fire_clip_is_refused(self):
        record = actor('Hatcher')
        del record['fire_clip']
        with self.assertRaisesRegex(ValueError, 'Hatcher is missing cooked clips'):
            generated_actor_specs({'actors': [record]})

    def test_a_cage_member_is_the_same_object_wherever_it_is_reserved(self):
        text = generated_actor_specs({'actors': [actor('HatcherBaby')]})[0]
        self.assertIn('controller:hk_sim::ActorController::HatcherBaby,', text)
        self.assertIn('corpse:None', text)


if __name__ == '__main__':
    unittest.main()
