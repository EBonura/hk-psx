"""The Geo audio manifest against the guest's Geo audio runtime.

The cooker's own checks (the observed AudioPlayRandom action, bank packing)
are unit tests of host/hk-cook/src/geo_audio.rs.
"""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
ROOT = Path(__file__).resolve().parents[1]
SPU_BASE = 0x14000
# One cooked 28-sample one-shot and its silent terminator block.
ONE_SHOT = bytes.fromhex('040045444444444444444444444444540c010000000000000000000000000000')


class GeoAudioTests(unittest.TestCase):
    def test_real_runtime_with_native_spu_trace(self):
        with tempfile.TemporaryDirectory(prefix='hk-geo-audio-') as tmp:
            root=Path(tmp);(root/'game').mkdir();(root/'data').mkdir()
            (root/'data/geo-audio.adpcm').write_bytes(ONE_SHOT*6)
            (root/'data/geo-audio.rs').write_text('const BANK_BYTES:usize=192;const SAMPLES:[(u32,u32,i16);6]=['+
                ','.join(f'({SPU_BASE+i*32},11025,5461)' for i in range(6))+'];')
            (root/'data/sfx.rs').write_text('const SAMPLES:[(u32,u32,i16);1]=[(0x1010,22050,5461)];')
            binary=root/'test';env=dict(os.environ,CARGO_MANIFEST_DIR=str(root/'game'))
            compiled=subprocess.run(['rustc','--edition=2021','-Awarnings','--test',str(ROOT/'tests/geo_audio_runtime.rs'),'-o',str(binary)],env=env,capture_output=True,text=True)
            self.assertEqual(compiled.returncode,0,compiled.stdout+compiled.stderr)
            result=subprocess.run([str(binary)],capture_output=True,text=True)
            self.assertEqual(result.returncode,0,result.stdout+result.stderr)

if __name__=='__main__':unittest.main()
