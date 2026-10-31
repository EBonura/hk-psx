"""Actors draw in whatever view the camera shows; NPCs need the drawn view.

The renderer draws the view whose cooked camera range holds the camera, which
can be a neighbour of the Knight's (game/src/disc.rs camera_view). Enemies need
nothing for that: the guest seats a scene's actors from the Knight's view into
one scene-wide pool and draws every seated actor the screen shows, which is
only right if every view of a scene lists all of its supported actors. An NPC
is cooked into the one view holding it, with its clips in that view's room, so
the frame draws the drawn view's NPC from the drawn view's room (frame.rs).
"""
import json, unittest
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REGIONS = ROOT / 'data/regions.json'


@unittest.skipUnless(REGIONS.is_file(), 'run a cook first')
class ViewActorTests(unittest.TestCase):
    def setUp(self):
        self.regions = json.loads(REGIONS.read_text())['regions']

    def test_every_view_of_a_scene_lists_all_its_supported_actors(self):
        # Except the actors of a compact art bank (host/actor_banks.json), which are listed in the views that carry it.
        import sys
        sys.path.insert(0, str(ROOT / 'host'))
        import actor_bank
        banked = {name: set(e['actors']) for name, e in actor_bank.scenes().items()}
        by_scene = defaultdict(list)
        for g in self.regions:
            mine = banked.get(g['scene_name'], set())
            by_scene[g['scene_name']].append(
                (g['chunk_id'], frozenset(a['source'] for a in g['actors']
                                          if a['movement_supported'] and a['name'] not in mine and a['source'] not in mine)))
        for scene, views in by_scene.items():
            everyone = frozenset().union(*(s for _, s in views))
            for chunk, sources in views:
                with self.subTest(scene=scene, view=chunk):
                    self.assertEqual(sources, everyone)

    def test_a_bank_actor_is_listed_in_exactly_the_views_that_carry_its_bank(self):
        import sys
        sys.path.insert(0, str(ROOT / 'host'))
        import actor_bank
        for scene, entry in actor_bank.scenes().items():
            mine = set(entry['actors'])
            for g in (g for g in self.regions if g['scene_name'] == scene):
                with self.subTest(scene=scene, view=g['chunk_id']):
                    listed = {a['source'] for a in g['actors'] if a['movement_supported'] and (a['name'] in mine or a['source'] in mine)}
                    carries = bool(g.get('actor_art_bank_carriers'))
                    self.assertEqual(bool(listed), carries)
                    if carries:
                        self.assertEqual(len(listed), len(mine))

    def test_each_npc_lives_in_exactly_one_view(self):
        seen = defaultdict(list)
        for g in self.regions:
            for n in g.get('npcs', []):
                seen[n['source']].append(g['chunk_id'])
        self.assertTrue(seen)
        for source, views in seen.items():
            with self.subTest(npc=source):
                self.assertEqual(len(views), 1, views)


if __name__ == '__main__':
    unittest.main()
