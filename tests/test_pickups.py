"""Chests and pickups: the generated table, and the runtime against it."""
import os, subprocess, sys, tempfile, unittest
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))
import pickups


def fake_metadata():
    chest = {'source': 'level46:2036', 'position': [6.7, 12.78], 'body': [5.5, 10.5, 8.0, 12.5],
             'reach': [1.5, 10.75, 11.5, 19.25], 'geo': [50, 10, 4], 'speed': [25.0, 38.0], 'angle': [80.0, 100.0],
             'frames': [0, 1, 2, 3], 'scale': 60692, 'open_fps': 12.0}
    crest = {'source': 'level46:497', 'position': [38.66, 2.94], 'reach': [37.5, 2.75, 40.0, 3.25], 'touch': False,
             'grant': ['city_key', 0], 'chest': None, 'after_false_knight': True, 'name': 'City Crest',
             'frames': list(range(4, 11)), 'fps': 7.5, 'scale': 60692, 'fling_from': None}
    kneel = {'frames': list(range(11, 25)), 'clips': [{'frames': [0, 1, 2], 'fps': 12.0}, {'frames': [3], 'fps': 12.0},
                                                      {'frames': [4, 5], 'fps': 12.0}]}
    record = {'chests': [chest], 'pickups': [crest], 'refused': [], 'kneel': kneel, 'level': 3}
    return {'regions': [{'scene_id': 0}, {'scene_id': 11, 'pickups': dict(record, frame_base=40, hide=[3])},
                        {'scene_id': 11, 'pickups': dict(record, frame_base=52, hide=[])}]}


class TableTests(unittest.TestCase):
    def test_one_row_per_object_and_a_base_per_view(self):
        text = pickups.rust(fake_metadata())
        self.assertIn('Chest{scene:11,local:0,', text)
        self.assertIn('closed:0,opened:1,', text)
        self.assertIn('open_first:2,open_count:2,open_fps:3072', text)
        self.assertIn('pub static KNEELS:&[(u16,u16,bool)]=&[(11,11,false)];', text)
        self.assertIn('geo:[0,20,4]', text, '200 Geo in 24 coins')
        self.assertIn('Pickup{scene:11,local:1,', text)
        self.assertIn('grant:Grant::CityKey,chest:255,after_false_knight:true,after_arena:false,first:4,count:7,fps:1920,scale:60692,fling:None,name:"City Crest"', text)
        self.assertIn('pub static BASES:&[(u16,u16,u16)]=&[(1,1,40),(2,2,52)];', text)
        self.assertIn('pub static HIDE:&[(u16,&[u16])]=&[(1,&[3])];', text)

    def test_the_cooked_table_holds_the_false_knights_200_geo_and_the_crest(self):
        path = ROOT / 'data/pickups.rs'
        if not path.is_file():
            self.skipTest('pickups are not cooked; the build cooks them')
        text = path.read_text()
        self.assertRegex(text, r'Chest\{scene:11,local:0,[^}]*geo:\[0,20,4\]')
        self.assertRegex(text, r'Pickup\{scene:11,[^}]*grant:Grant::CityKey,chest:255,after_false_knight:true')
        self.assertRegex(text, r'Pickup\{scene:0,local:1,[^}]*grant:Grant::Charm\(6\),chest:0,')


class RuntimeTests(unittest.TestCase):
    def test_chests_crest_and_pieces(self):
        # The cooked table when there is one, else the fixture above plus a
        # touch piece, so the logic is tested either way.
        cooked = ROOT / 'data/pickups.rs'
        with tempfile.TemporaryDirectory(prefix='hk-pickups-test-') as temp:
            (Path(temp) / 'game').mkdir()
            (Path(temp) / 'data').mkdir()
            if cooked.is_file():
                text = cooked.read_text()
            else:
                metadata = fake_metadata()
                piece = {'source': 'level67:622', 'position': [15.0, 5.7], 'reach': [14.5, 5.25, 15.5, 6.0], 'touch': True,
                         'grant': ['vessel_fragment', 0], 'chest': None, 'after_false_knight': False, 'name': '',
                         'frames': [25], 'fps': 1.0, 'scale': 60692, 'fling_from': None}
                held = dict(piece, source='level46:1', touch=False, grant=['charm', 6], chest=0, name='Fury of the Fallen',
                            reach=[5.5, 10.5, 8.0, 11.0], frames=[26], fling_from=[6.7, 12.5])
                metadata['regions'][1]['pickups']['pickups'] += [piece, held]
                text = pickups.rust(metadata)
            (Path(temp) / 'data/pickups.rs').write_text(text)
            binary = Path(temp) / 'pickups-tests'
            env = dict(os.environ, CARGO_MANIFEST_DIR=str(Path(temp) / 'game'))
            built = subprocess.run(['rustc', '--edition=2021', '-Awarnings', '--test', str(ROOT / 'tests/pickups_runtime.rs'),
                                    '-o', str(binary)], env=env, capture_output=True, text=True)
            self.assertEqual(built.returncode, 0, built.stdout + built.stderr)
            run = subprocess.run([str(binary)], capture_output=True, text=True)
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)


if __name__ == '__main__':
    unittest.main()
