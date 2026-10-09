"""The resident audio manifests against the guest's audio runtime.

The cooker's own checks (event tables, boss and death FSM contracts, bank
packing, resampling, the encoder's framing) are unit tests of
host/hk-cook/src/cook_audio.rs.
"""
from pathlib import Path
import os
import subprocess
import sys
import tempfile
import unittest
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))


class AudioTests(unittest.TestCase):
    def test_actual_audio_runtime_with_native_spu_trace(self):
        with tempfile.TemporaryDirectory(prefix='hk-audio-runtime-') as tmp:
            root=Path(tmp);(root/'game').mkdir();(root/'data').mkdir()
            # Distinct ordered payload markers and gains expose wrong event
            # indices/voices without depending on generated assets or real audio.
            (root/'data/sfx.adpcm').write_bytes(b''.join(bytes([i])+bytes(31) for i in range(10)))
            (root/'data/sfx.rs').write_text('const BANK_BYTES:usize=320;const SAMPLES:[(u32,u32,i16);8]=['+
                ','.join(f'({0x1010+i*32},{11025 if i==7 else 22050},{(i+1)*1000})' for i in range(8))+
                # Both boss voices carry the source's own gain, which is the
                # door's: set_volume drives the shared voice from that one row.
                '];const BOSS_SAMPLES:[(u32,u32,i16);2]=['+
                f'({0x1010+8*32},11025,1000),({0x1010+9*32},22050,1000)'+
                '];const EXTRA_SAMPLES:[(u32,u32,i16);7]=['+
                ','.join(f'({0x1010+(i%10)*32},11025,1000)' for i in range(7))+
                '];const GREAT_DOOR_HIT_PITCH:[u16;2]=[1741,2355];'+
                'const HARD_FALL_MIN_TICKS:u16=67;const SOFT_LANDING_TICKS:u16=2;const RUN_SEQUENCE_TICKS:u16=4;'+
                # The menu's two clips sit right above the bank.
                f'const UI_BYTES:usize=64;const UI_SAMPLES:[(u32,u32,i16);2]=[({0x1010+320},22050,500),({0x1010+352},11025,500)];')
            (root/'data/ui_sfx.adpcm').write_bytes(bytes([20])+bytes(31)+bytes([21])+bytes(31))
            # The world bank: five markers at its own base, every row at the
            # shared voice's gain, only the first with a pitch range.
            (root/'data/world-sfx.adpcm').write_bytes(b''.join(bytes([40+i])+bytes(31) for i in range(5)))
            (root/'data/world-sfx.rs').write_text('pub const SPU_BASE: u32 = 393216;pub const BANK_BYTES: usize = 160;'
                'pub const BANK_CHECKSUM: u32 = 0;pub const SAMPLES: [(u32,u32,i16,u16,u16);5] = ['
                '(393216,22050,1000,1536,2560),(393248,4000,1000,372,372),(393280,5512,1000,512,512),'
                '(393312,8000,1000,743,743),(393344,11025,1000,1024,1024)];')
            binary=root/'test';env=dict(os.environ,CARGO_MANIFEST_DIR=str(root/'game'))
            source=Path(__file__).resolve().parents[1]/'tests/audio_runtime.rs'
            compiled=subprocess.run(['rustc','--edition=2021','-Awarnings','--test',str(source),'-o',str(binary)],
                env=env,capture_output=True,text=True)
            self.assertEqual(compiled.returncode,0,compiled.stdout+compiled.stderr)
            result=subprocess.run([str(binary)],capture_output=True,text=True)
            self.assertEqual(result.returncode,0,result.stdout+result.stderr)


class StreamedBankManifestTests(unittest.TestCase):
    """The three ADPCM banks the guest streams instead of linking.

    The guest no longer carries the samples, only each bank's length and FNV-1a
    checksum, and it refuses a chunk that does not match them. That makes the
    generated manifest the only thing standing between a recooked payload and a
    boot with silence (or garbage) in SPU RAM, so the pair is checked here as
    well as at build time.
    """

    def setUp(self):
        import build_guest
        self.build_guest = build_guest

    def test_every_streamed_bank_matches_its_generated_manifest(self):
        for name in self.build_guest.AUDIO_BANKS:
            with self.subTest(bank=name):
                self.assertTrue((ROOT / 'data' / name).is_file(), f'Missing data/{name}')
                self.assertTrue((ROOT / 'data' / self.build_guest.bank_manifest(name)).is_file())
        self.build_guest.check_audio_banks()

    def test_a_recooked_payload_without_its_manifest_is_refused(self):
        fnv = self.build_guest.fnv
        for name in self.build_guest.AUDIO_BANKS:
            payload = (ROOT / 'data' / name).read_bytes()
            manifest = (ROOT / 'data' / self.build_guest.bank_manifest(name)).read_text()
            with self.subTest(bank=name):
                # One flipped bit in the payload has to break the pairing, or
                # the checksum the guest validates the chunk against is inert.
                damaged = bytearray(payload)
                damaged[32] ^= 1
                self.assertNotEqual(fnv(bytes(damaged)), fnv(payload))
                self.assertIn(f'pub const BANK_CHECKSUM: u32 = {fnv(payload)};', manifest)
                self.assertIn(f'pub const BANK_BYTES: usize = {len(payload)};', manifest)


if __name__ == '__main__':
    unittest.main()
