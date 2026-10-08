"""Source action admission, complete sample bounds, and actual guest event hooks."""
import copy
import io
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest
import wave
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT/'host'))
import geo_audio as geo
from cook_audio import encode, decode_oneshot, convert_wav


def action():
    return dict(actionNames=['HutongGames.PlayMaker.Actions.AudioPlayRandom'], actionEnabled=[1],
        actionStartIndex=[0], paramName=['gameObject','audioClips','','','weights','','','pitchMin','pitchMax'],
        paramDataType=[19,12,5,5,12,15,15,15,15], paramDataPos=[0,0,0,1,1,0,5,10,15],
        paramByteDataSize=[0,0,0,0,0,5,5,5,5], byteData=list((struct.pack('<f',1)+b'\0')*4),
        arrayParamSizes=[2,2], arrayParamTypes=['UnityEngine.AudioClip','HutongGames.PlayMaker.FsmFloat'],
        fsmGameObjectParams=[dict(useVariable=1,name='Self')],
        unityObjectParams=[dict(m_FileID=2,m_PathID=92),dict(m_FileID=2,m_PathID=99)])


class GeoAudioTests(unittest.TestCase):
    def test_source_action_only_constant_equal_weight_self(self):
        data=action();self.assertEqual(geo.random_audio_action(data),data['unityObjectParams'])
        mutations=[lambda d:d['actionEnabled'].__setitem__(0,0),
            lambda d:d['byteData'].__setitem__(4,1),
            lambda d:d['byteData'].__setitem__(13,64),
            lambda d:d['fsmGameObjectParams'][0].__setitem__('name','Other'),
            lambda d:d['unityObjectParams'][0].__setitem__('m_PathID',0),
            lambda d:d['arrayParamSizes'].__setitem__(0,3),
            lambda d:d['paramDataType'].__setitem__(2,24)]
        for mutate in mutations:
            bad=copy.deepcopy(data);mutate(bad)
            with self.assertRaises(ValueError):geo.random_audio_action(bad)

    def test_no_truncation_final_pcm_window_and_safe_terminator(self):
        # Long enough for the resampler to prime; the tail stays loud so a
        # truncated conversion or a dropped final window would show up.
        frames=[(1000,-500)]*4000+[(24000,24000)]*100
        wav=io.BytesIO()
        with wave.open(wav,'wb') as out:
            out.setnchannels(2);out.setsampwidth(2);out.setframerate(44100)
            out.writeframes(b''.join(struct.pack('<hh',*p) for p in frames))
        pcm,meta=convert_wav(wav.getvalue(),11025)
        self.assertEqual(meta['source_frames'],4100)
        self.assertEqual(len(pcm),1025)
        self.assertGreater(pcm[-1],20000)
        bank=encode(pcm);decoded=decode_oneshot(bank)
        self.assertEqual(len(decoded),(len(pcm)+27)//28*28)
        self.assertNotEqual(decoded[len(pcm)-1],0)
        self.assertEqual(bank[-16:],bytes([12,1])+bytes(14))
        unsafe=bytearray(bank);unsafe[1]=3
        with self.assertRaises(ValueError):decode_oneshot(unsafe)

    def test_complete_ordered_bank_exact_limit_and_distinct_same_names(self):
        items=[(f'{file}:{pid}',encode([i*1000]*28),{}) for i,(file,pid,_) in enumerate(geo.SOURCES)]
        bank,records=geo.pack_bank(items,192)
        self.assertEqual(len(bank),192);self.assertEqual(records[-1]['spu_address'],geo.SPU_BASE+160)
        self.assertNotEqual(bank[32:64],bank[64:96])
        with self.assertRaises(ValueError):geo.pack_bank(items,191)
        with self.assertRaises(ValueError):geo.pack_bank(items[:-1])
        with self.assertRaises(ValueError):geo.pack_bank(items[::-1])

    def test_real_runtime_with_native_spu_trace(self):
        with tempfile.TemporaryDirectory(prefix='hk-geo-audio-') as tmp:
            root=Path(tmp);(root/'game').mkdir();(root/'data').mkdir()
            (root/'data/geo-audio.adpcm').write_bytes(encode([1000]*28)*6)
            (root/'data/geo-audio.rs').write_text('const BANK_BYTES:usize=192;const SAMPLES:[(u32,u32,i16);6]=['+
                ','.join(f'({geo.SPU_BASE+i*32},11025,5461)' for i in range(6))+'];')
            (root/'data/sfx.rs').write_text('const SAMPLES:[(u32,u32,i16);1]=[(0x1010,22050,5461)];')
            binary=root/'test';env=dict(os.environ,CARGO_MANIFEST_DIR=str(root/'game'))
            compiled=subprocess.run(['rustc','--edition=2021','-Awarnings','--test',str(ROOT/'tests/geo_audio_runtime.rs'),'-o',str(binary)],env=env,capture_output=True,text=True)
            self.assertEqual(compiled.returncode,0,compiled.stdout+compiled.stderr)
            result=subprocess.run([str(binary)],capture_output=True,text=True)
            self.assertEqual(result.returncode,0,result.stdout+result.stderr)

if __name__=='__main__':unittest.main()
