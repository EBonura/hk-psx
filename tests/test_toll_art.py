"""Every scene with a Toll Machine is listed in quality.TOLL_ART, so its regions are cooked with the machines' art."""
import json, sys, unittest
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))


class TollArtTests(unittest.TestCase):
    @unittest.skipUnless((ROOT / 'data/regions.json').is_file(), 'needs the cooked region report')
    def test_scenes_with_a_toll_machine_are_listed(self):
        from quality import TOLL_ART
        report = json.loads((ROOT / 'data/regions.json').read_text())
        scenes = {link['scene_name'] for link in report['links'] if link.get('family') == 'Toll Machine'}
        self.assertEqual(scenes - TOLL_ART, set(), 'add the scene to host/quality.py TOLL_ART so its regions recook')
        self.assertEqual(TOLL_ART - scenes, set(), 'a listed scene has no Toll Machine record')

    def test_a_listed_scene_marks_its_region_specs(self):
        from regions import region_spec
        from quality import TOLL_ART
        scene = {'scene_id': 1, 'scene_name': next(iter(TOLL_ART)), 'file': 'x', 'camera_global_bounds': [0, 0, 100, 100]}
        self.assertTrue(region_spec(scene, [0, 0, 10, 10]).get('toll_art'))
        other = dict(scene, scene_name='Tutorial_01')
        self.assertNotIn('toll_art', region_spec(other, [0, 0, 10, 10]))


if __name__ == '__main__':
    unittest.main()
