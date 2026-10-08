"""The blackout regression must check scenery, not merely a visible HUD."""
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'tools'))
from validate_streaming import blackout_check,BLACKOUT_TAPE


class BlackoutRegression(unittest.TestCase):
    def test_hud_only_frame_fails_and_scene_requires_route_coverage(self):
        state={'final_ram':{'HK_REGION_ID':8,'HK_PLAYER_X':145,'HK_PLAYER_Y':5,
                            'HK_REVEAL_MASKS_HIDDEN':2}}
        header=b'P6\n320 240\n255\n'
        with tempfile.TemporaryDirectory() as directory:
            image=Path(directory)/'display.ppm'
            image.write_bytes(header+b'\xff'*(320*40*3)+bytes(320*200*3))
            self.assertFalse(blackout_check(BLACKOUT_TAPE,state,image)['passed'])
            image.write_bytes(header+bytes(320*40*3)+b'\x80'*(320*200*3))
            self.assertTrue(blackout_check(BLACKOUT_TAPE,state,image)['passed'])
            state['final_ram']['HK_REGION_ID']=1
            self.assertFalse(blackout_check(BLACKOUT_TAPE,state,image)['passed'])

    def test_other_routes_are_not_subject_to_this_fixture(self):
        self.assertIsNone(blackout_check('other',{},Path('/absent')))


if __name__=='__main__':unittest.main()
