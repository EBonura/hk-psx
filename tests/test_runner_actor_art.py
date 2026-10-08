"""Explicit actor admission, shared clip cooking and appended-bank bindings."""
import copy
from pathlib import Path
import sys
from types import SimpleNamespace
import unittest
from unittest.mock import patch
from PIL import Image
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'host'))
from actors import actor_placements, generated_actor_region
from cook import append_actor_art
from regions import remap_actor_clips


LIVE = {'walk_clip': 'Walk', 'turn_clip': 'Turn', 'idle_clip': 'Idle',
        'anticipate_clip': 'Attack Anticipate', 'lunge_clip': 'Attack Lunge',
        'cooldown_clip': 'Attack Cooldown', 'fall_clip': 'Fall'}


def actor_fixture():
    actor = dict(source='fixture:42', movement_supported=True, game_object=1,
                 movement_control={'kind': 'ZombieSwipeWalker', 'library_source': 'fixture:9',
                                   'alert_bounds_q16': [-365036, -144507, 365036, 12730],
                                   'parameters': {'walk_speed': 1.5, 'initial_direction': -1, 'walk_velocity_q16': [-98304, 98304], 'lunge_velocity_q16': [-393216, 393216],
                                                  'walking_wait_endpoints_ticks': [240, 90], 'pause_endpoints_ticks': [150, 90]}},
                 position=[2., 3., .001],
                 colliders=[dict(bounds=[1.5, 2., 2.5, 3.5])], health=15,
                 health_manager=dict(hasSpecialDeath=False, hasAlternateHitAnimation=False,
                                     invincibleFromDirection=0, invincible=False, damageOverride=False),
                 DamageHero={'damageDealt': 1}, Recoil={'recoilSpeedBase': 10., 'recoilDuration': .15},
                 corpse=dict(air_clip=7, land_clip=8, bounds=[-1, -2, 3, 4],
                             spawn_offset=[0, 0], bounce_factor=13107))
    actor.update({key: index for index, key in enumerate(LIVE)})
    return actor


class RunnerActorArtTests(unittest.TestCase):
    def test_pending_actors_do_not_enter_art_or_generated_metadata(self):
        pending = {'movement_supported': False, 'pending_movement_control': {'kind': 'ZombieSwipeWalker'}}
        frames, clips = [], []
        with patch('effects.append_corpse_art') as corpses:
            append_actor_art(None, None, [pending], None, frames, clips)
        self.assertEqual((frames, clips), ([], []))
        self.assertEqual(generated_actor_region({'actors': [pending]}), '&[]')
        self.assertFalse(corpses.call_args.args[2][0]['movement_supported'])

    def test_two_instances_share_all_live_clips_and_sprite_frames(self):
        collection = SimpleNamespace(sid='collection:1', assets_file='collectionfile', data={})
        library = SimpleNamespace(sid='library:1', assets_file='libraryfile', data={'clips': [
            dict(name=name, frames=[{'spriteCollection': collection, 'spriteId': 0},
                                    {'spriteCollection': collection, 'spriteId': 1}],
                 fps=12, wrapMode=0, loopStart=0) for name in LIVE.values()]})
        source = SimpleNamespace(ref=lambda file, ref: ref, read=lambda obj: obj.data, sid=lambda obj: obj.sid)
        scene = SimpleNamespace(file='scene', go_transform={1: 1, 2: 2},
                                world=lambda tid: [[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]])
        first = actor_fixture()
        first.update(tk2dSpriteAnimator={'library': library},
                     tk2dSprite={'_scale': {'x': 1, 'y': 1}, '_color': {'a': 1}})
        second = dict(first, game_object=2)
        allocated = []
        def add(*args, **kwargs):
            allocated.append((args, kwargs)); return len(allocated)-1
        frames, clips = [], []
        with patch('cook.tk_sprite', return_value=(Image.new('RGBA', (4, 4)), (0, 0, 1, 1))), \
             patch('effects.append_corpse_art') as corpses:
            append_actor_art(source, scene, [first, second], SimpleNamespace(add_tiled=add), frames, clips)
        self.assertEqual(len(allocated), 2)
        self.assertEqual(len(clips), 7)
        self.assertEqual(len(frames), 14)
        for key, name in LIVE.items():
            self.assertEqual(first[key], second[key])
            self.assertEqual(clips[first[key]]['name'], 'library:1/' + name)
        corpses.assert_called_once()

    def _cook_one_actor(self, atlas, box):
        """Run the actor art path over a single fixture sprite of this size."""
        collection = SimpleNamespace(sid='collection:1', assets_file='collectionfile', data={})
        library = SimpleNamespace(sid='library:1', assets_file='libraryfile', data={'clips': [
            dict(name=name, frames=[{'spriteCollection': collection, 'spriteId': 0}],
                 fps=12, wrapMode=0, loopStart=0) for name in LIVE.values()]})
        source = SimpleNamespace(ref=lambda file, ref: ref, read=lambda obj: obj.data,
                                 sid=lambda obj: obj.sid)
        scene = SimpleNamespace(file='scene', go_transform={1: 1},
                                world=lambda tid: [[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]])
        actor = actor_fixture()
        actor.update(tk2dSpriteAnimator={'library': library},
                     tk2dSprite={'_scale': {'x': 1, 'y': 1}, '_color': {'a': 1}})
        frames, clips = [], []
        image = Image.new('RGBA', (8, 8), (200, 100, 30, 255))
        with patch('cook.tk_sprite', return_value=(image, box)), \
             patch('effects.append_corpse_art'):
            append_actor_art(source, scene, [actor], atlas, frames, clips)
        return actor, frames

    def test_an_oversized_actor_frame_binds_a_rectangle_of_slots(self):
        # Crossroads_47's Stag is the smallest real case: its Idle frames
        # measure 91x89 through this same path. Divided by the FOCAL / -CAM_Z
        # projection, that is the source box below.
        from cook import Atlas, FOCAL, CAM_Z
        atlas = Atlas(max_pages=0)
        actor, frames = self._cook_one_actor(atlas, (0, 0, 91 / (FOCAL / -CAM_Z), 89 / (FOCAL / -CAM_Z)))
        self.assertTrue(actor['movement_supported'])
        self.assertNotIn('limitations', actor)
        # One sprite shared by every clip, so one frame and one 2x2 grid.
        self.assertEqual(atlas.grids, {0: (2, 2)})
        self.assertEqual({frame['texture'] for frame in frames}, {0})
        atlas.pack()
        self.assertEqual([entry[3:5] for entry in atlas.entries],
                         [(64, 64), (27, 64), (64, 25), (27, 25)])
        # Only the frame's first tile carries the grid, which is what
        # hk_format::Room::frame_grid reads back.
        self.assertEqual([entry[1:3] for entry in atlas.entries],
                         [(2, 2), (0, 0), (0, 0), (0, 0)])

    def test_the_actor_refusal_is_the_axis_clamp_not_a_single_slot(self):
        # Scenery is resampled to fit a cap; actor sampling is not, so a frame
        # the 252-pixel clamp would resize is refused instead of squashed. The
        # tile-budget branch cannot fire under that clamp: four tiles by four is
        # 16 of the 20 a frame may bind. tests/test_cook.py holds that bound.
        from cook import Atlas, FOCAL, CAM_Z, MAX_TEXTURE_AXIS
        atlas = Atlas(max_pages=0)
        actor, frames = self._cook_one_actor(
            atlas, (0, 0, (MAX_TEXTURE_AXIS + 2) / (FOCAL / -CAM_Z), 1))
        self.assertFalse(actor['movement_supported'])
        self.assertIn('exceeds the 252-pixel texture axis', actor['limitations'][0])
        # Refused as a whole actor, before any frame reaches the atlas.
        self.assertEqual((frames, atlas.images), ([], []))

    def test_runner_metadata_requires_every_bound_clip_corpse_and_depth(self):
        actor = actor_fixture()
        text = generated_actor_region({'actors': [actor]})
        self.assertIn('controller:hk_sim::ActorController::Runner {idle_clip:2,anticipate_clip:3,lunge_clip:4,cooldown_clip:5,'
                      'params:hk_sim::runner::Params {walk_speed:98304,lunge_speed:393216,walking_wait:[240,90],paused_wait:[150,90],'
                      'attack:hk_sim::runner::Attack::Swipe,gravity:3932160},'
                      'alert:[-365036,-144507,365036,12730]}', text)
        self.assertIn('speed:98304,turn_ticks:10,turn_cooldown_ticks:60', text)
        # Facing and the random-start switch are the placement's, not the type's.
        placement = actor_placements({'actors': [actor]})[0]
        self.assertEqual((placement['initial_direction'], placement['random_start_direction']),
                         (-1, False))
        self.assertIn('health:15,contact_damage:1', text)
        for key in ['idle_clip', 'anticipate_clip', 'lunge_clip', 'cooldown_clip', 'corpse']:
            with self.subTest(key=key):
                bad = copy.deepcopy(actor); del bad[key]
                with self.assertRaisesRegex(ValueError, 'missing'):
                    generated_actor_region({'actors': [bad]})
        for change in [{'idle_clip': 65536},
                       {'health_manager': dict(actor['health_manager'], hasSpecialDeath=True)}]:
            with self.subTest(change=change), self.assertRaises(ValueError):
                generated_actor_region({'actors': [dict(actor, **change)]})

    def test_crawler_emits_explicit_default_controller(self):
        actor = actor_fixture()
        actor['movement_control'] = dict(kind='WalkLeftRight', speed=3.5, turn_ticks=12,
                                        turn_cooldown_ticks=60, initial_direction=1, random_start_direction=True)
        text = generated_actor_region({'actors': [actor]})
        self.assertIn('controller:hk_sim::ActorController::Crawler', text)
        self.assertNotIn('ActorController::Runner', text)
        self.assertIn('speed:229376,turn_ticks:12', text)

    def test_scene_bank_remaps_all_bindings_without_mutating_shared_source(self):
        actor = actor_fixture(); original = copy.deepcopy(actor)
        left, right = remap_actor_clips(actor, 3), remap_actor_clips(actor, 21)
        self.assertEqual(actor, original)
        for key in LIVE:
            self.assertEqual(left[key], original[key]+3)
            self.assertEqual(right[key], original[key]+21)
        for key in ['air_clip', 'land_clip']:
            self.assertEqual(left['corpse'][key], original['corpse'][key]+3)
            self.assertEqual(right['corpse'][key], original['corpse'][key]+21)
        self.assertEqual(left['corpse']['bounce_factor'], 13107)
        with self.assertRaisesRegex(ValueError, 'exceeds u16'):
            remap_actor_clips(actor, 65535)
        del actor['lunge_clip']
        with self.assertRaisesRegex(ValueError, 'missing cooked binding'):
            remap_actor_clips(actor, 0)

    def test_scene_bank_remaps_controller_specific_bindings(self):
        actor = actor_fixture()
        actor['movement_control'] = {'kind': 'Vengefly'}
        actor.update(startle_clip=4, chase_clip=5, turn_fly_clip=6)
        remapped = remap_actor_clips(actor, 30)
        self.assertEqual((remapped['startle_clip'], remapped['chase_clip'], remapped['turn_fly_clip']), (34, 35, 36))
        climber = actor_fixture(); climber['movement_control'] = {'kind': 'Climber'}; climber['stun_clip'] = 2
        self.assertEqual(remap_actor_clips(climber, 10)['stun_clip'], 12)


if __name__ == '__main__':
    unittest.main()
