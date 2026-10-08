"""Area music cook: sector-exact loops, premix reachability and the manifest; no retail audio."""
import pathlib,sys,unittest
from unittest.mock import patch
import numpy as np
sys.path.insert(0,str(pathlib.Path(__file__).resolve().parents[1]/'host'))
import area_music as music

KEEP=music.KEEP

class AreaMusicTests(unittest.TestCase):
    def test_loops_are_whole_sectors_within_a_cent(self):
        for seconds,rate in ((153.6,44100),(152.47061224489795,44100),(103.74512471655329,44100),(5.36,44100),(0.01,48000)):
            frames=round(seconds*rate);out=music.sector_samples(frames,rate)
            self.assertEqual(out%music.SECTOR_SAMPLES,0)
            self.assertEqual(out//28*16%music.SECTOR,0)
            # Half a step of 3,584 samples: under a cent for any loop past 6 s.
            if seconds>6:self.assertLess(abs(out/(frames*music.RATE/rate)-1),0.00058)
            if seconds>1:self.assertLessEqual(abs(out-frames*music.RATE/rate),music.SECTOR_SAMPLES/2)
        # 153.6 s is already a whole number of sectors at 22,050 Hz.
        self.assertEqual(music.sector_samples(round(153.6*44100),44100),round(153.6*22050))

    def test_resample_hits_the_exact_length_and_keeps_a_tone(self):
        n=10000;t=np.arange(n);x=np.sin(2*np.pi*t/50)*1000+300
        y=music.lanczos_resample(x,n+7)
        self.assertEqual(len(y),n+7)
        self.assertAlmostEqual(float(y.mean()),300,delta=2)
        self.assertAlmostEqual(float(np.abs(y-300).max()),1000,delta=15)

    def test_reachable_follows_gates_scene_states_and_regions(self):
        # Scene 0 plays family 0 Normal; scene 1 keeps the cue under Sub Area;
        # scene 2 keeps everything and has a region entering family 1 Normal.
        scenes=[{'scene':0,'family':0,'snapshot':1},{'scene':1,'family':KEEP,'snapshot':2},
                {'scene':2,'family':KEEP,'snapshot':KEEP},{'scene':3,'family':KEEP,'snapshot':0}]
        regions=[{'scene':2,'enter_family':1,'enter_snapshot':1,'exit_family':KEEP,'exit_snapshot':0}]
        with patch('ambience.gate_edges',return_value={(0,1),(1,2)}):
            heard=music.reachable(scenes,regions,['Silent','Normal','Sub Area'])
        self.assertIn((0,1),heard);self.assertIn((0,2),heard)
        self.assertIn((1,1),heard);self.assertIn((1,0),heard)
        # The region's cue carries back through the gate into Sub Area.
        self.assertIn((1,2),heard)
        # Scene 3 has no gate and no cue: a fresh arrival there hears nothing.
        self.assertEqual(len(heard),5)
        self.assertTrue(all(KEEP not in pair for pair in heard))

    def test_manifest_states_tracks_and_regions(self):
        tracks=[{'sectors':945,'byte_len':945*2048,'checksum':7}]
        mixes=[[{'track':KEEP,'volume':0},{'track':0,'volume':16383}]]
        scenes=[{'family':0,'snapshot':1,'fade_ticks':300,'delay_ticks':60}]
        regions=[{'scene':0,'box':[1,2,3,4],'enter_family':KEEP,'enter_snapshot':0,'enter_fade_ticks':60,
                  'exit_family':KEEP,'exit_snapshot':1,'exit_fade_ticks':120}]
        text=music.rust_manifest(tracks,['Silent','Normal'],mixes,scenes,regions)
        self.assertIn('pub const MUSIC_TRACKS:[MusicTrack;1]=[',text)
        self.assertIn('MusicTrack{sectors:945,byte_len:1935360,checksum:7},',text)
        self.assertIn('pub const MUSIC_MIXES:[[MusicMix;2];1]=[',text)
        self.assertIn(f'MusicScene{{state:MusicState{{family:0,snapshot:1,fade_ticks:300}},delay_ticks:60}},',text)
        self.assertIn(f'MusicRegion{{scene:0,box_:[1, 2, 3, 4],enter:MusicState{{family:{KEEP},snapshot:0,fade_ticks:60}}',text)
        self.assertIn(f'pub const MUSIC_PITCH:u16={music.PITCH};',text)

if __name__=='__main__':unittest.main()
