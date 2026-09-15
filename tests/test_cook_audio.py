"""Independent sample reconstruction and fail-closed resident admission."""
import io
import copy
import os
from pathlib import Path
import struct
import sys
import subprocess
import tempfile
import unittest
import wave
from fractions import Fraction
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'host'))
from cook_audio import BANK_LIMIT, EVENTS, convert_wav, decode_zero, death_action_layers, encode, pack_bank


def wav(samples, channels=1, rate=44100):
    out = io.BytesIO()
    with wave.open(out, 'wb') as w:
        w.setnchannels(channels)
        w.setsampwidth(2)
        w.setframerate(rate)
        w.writeframes(struct.pack('<' + 'h' * len(samples), *samples))
    return out.getvalue()


def death_actions():
    data = {'actionNames': ['AudioPlayerOneShotSingle.AudioPlayerOneShotSingle'] * 2,
            'actionEnabled': [1, 1], 'actionStartIndex': [0, 5],
            'paramName': [], 'paramDataPos': [], 'paramDataType': [],
            'paramByteDataSize': [], 'byteData': [], 'fsmObjectParams': []}
    for layer in range(2):
        data['paramName'].append('audioClip'); data['paramDataPos'].append(layer)
        data['paramDataType'].append(24); data['paramByteDataSize'].append(0)
        data['fsmObjectParams'].append({'typeName': 'UnityEngine.AudioClip', 'useVariable': 0,
                                        'value': {'m_FileID': 0, 'm_PathID': layer + 1}})
        for name, value in [('pitchMin', 1), ('pitchMax', 1), ('volume', 1), ('delay', 0)]:
            data['paramName'].append(name); data['paramDataPos'].append(len(data['byteData']))
            data['paramDataType'].append(15); data['paramByteDataSize'].append(5)
            data['byteData'].extend(struct.pack('<fB', value, 0))
    return data


class AudioTests(unittest.TestCase):
    def test_actual_audio_runtime_with_native_spu_trace(self):
        with tempfile.TemporaryDirectory(prefix='hk-audio-runtime-') as tmp:
            root=Path(tmp);(root/'game').mkdir();(root/'data').mkdir()
            # Distinct ordered payload markers and gains expose wrong event
            # indices/voices without depending on generated assets or real audio.
            (root/'data/sfx.adpcm').write_bytes(b''.join(bytes([i])+bytes(31) for i in range(8)))
            (root/'data/sfx.rs').write_text('const BANK_BYTES:usize=256;const SAMPLES:[(u32,u32,i16);8]=['+
                ','.join(f'({0x1010+i*32},{11025 if i==7 else 22050},{(i+1)*1000})' for i in range(8))+
                '];const GREAT_DOOR_HIT_PITCH:[u16;2]=[1741,2355];'+
                'const HARD_FALL_MIN_TICKS:u16=67;const SOFT_LANDING_TICKS:u16=2;const RUN_SEQUENCE_TICKS:u16=4;')
            binary=root/'test';env=dict(os.environ,CARGO_MANIFEST_DIR=str(root/'game'))
            source=Path(__file__).resolve().parents[1]/'tests/audio_runtime.rs'
            compiled=subprocess.run(['rustc','--edition=2021','-Awarnings','--test',str(source),'-o',str(binary)],
                env=env,capture_output=True,text=True)
            self.assertEqual(compiled.returncode,0,compiled.stdout+compiled.stderr)
            result=subprocess.run([str(binary)],capture_output=True,text=True)
            self.assertEqual(result.returncode,0,result.stdout+result.stderr)

    def test_death_mapping_retains_both_immediate_source_layers(self):
        layers = death_action_layers(death_actions())
        self.assertEqual([x['clip']['m_PathID'] for x in layers], [1, 2])
        self.assertEqual([x['delay'] for x in layers], [0, 0])

    def test_death_mapping_rejects_dynamic_or_disabled_audio(self):
        data = death_actions()
        dynamic = copy.deepcopy(data)
        dynamic['fsmObjectParams'][0]['useVariable'] = 1
        with self.assertRaises(ValueError):
            death_action_layers(dynamic)
        data['actionEnabled'][1] = 0
        with self.assertRaises(ValueError):
            death_action_layers(data)

    def test_terminal_block_is_silent_and_never_repeats(self):
        encoded = encode([1000] * 29)
        self.assertEqual(len(encoded), 48)
        self.assertEqual(encoded[-16:], bytes([12, 1]) + bytes(14))
        self.assertEqual(len(decode_zero(encoded)), 56)
        self.assertEqual(decode_zero(encoded)[29:], [0] * 27)

    def test_signed_extrema_decode_without_overflow(self):
        samples = [-32768, 32767, -4096, 4096] * 7
        decoded = decode_zero(encode(samples))
        self.assertEqual(decoded[0], -32768)
        self.assertEqual(decoded[1], 28672)
        self.assertTrue(all(-32768 <= x <= 32767 for x in decoded))
        self.assertLessEqual(max(abs(a - b) for a, b in zip(samples, decoded)), 4095)

    def test_stereo_downmix_and_lowpass_preserve_duration(self):
        pcm, meta = convert_wav(wav([1000, -1000, 3000, 1000, -1000, -3000], 2), 22050)
        self.assertEqual(pcm, [1000, -2000])
        self.assertEqual(meta['source_frames'], 3)
        self.assertEqual(meta['source_channels'], 2)
        self.assertEqual(meta['samples'], 2)

    def test_lowpass_removes_alternating_nyquist_signal(self):
        pcm, _ = convert_wav(wav([10000, -10000] * 20), 11025)
        self.assertEqual(pcm, [0] * 10)

    def test_48000_running_sequence_uses_rational_filter_without_pitch_change(self):
        source=[1000,-2000,3000,-4000,5000,-6000,7000,-8000,9000]
        pcm,meta=convert_wav(wav(source,rate=48000),11025)
        # Independent exact rational integration of source sample intervals;
        # the incomplete final output window repeats the last source frame.
        expected=[]
        for j in range((len(source)*11025+47999)//48000):
            lo,hi=Fraction(j*48000,11025),Fraction((j+1)*48000,11025)
            area=Fraction(0)
            for i in range(int(lo),int(hi)+1):
                width=max(Fraction(0),min(hi,i+1)-max(lo,i))
                area+=source[min(i,len(source)-1)]*width
            expected.append(round(area/(hi-lo)))
        self.assertEqual(pcm,expected)
        self.assertEqual(meta['source_frames'],len(source))
        self.assertEqual(meta['samples'],3)
        dc,_=convert_wav(wav([12345]*97600,rate=48000),11025)
        self.assertEqual(len(dc),22418)
        self.assertEqual(set(dc),{12345})

    def test_empty_rational_clip_has_no_invented_sample(self):
        pcm,meta=convert_wav(wav([],rate=48000),11025)
        self.assertEqual(pcm,[])
        self.assertEqual(meta['samples'],0)

    def test_rate_conversion_rejects_unverified_factor(self):
        with self.assertRaises(ValueError):
            convert_wav(wav([0] * 100), 8000)

    def test_bad_loop_flags_are_rejected(self):
        bank = bytearray(encode([1000] * 28))
        bank[1] = 4
        with self.assertRaises(ValueError):
            decode_zero(bank)

    def test_budget_is_atomic_and_sample_starts_aligned(self):
        items = [(name, encode([1000] * (i + 1)), {}) for i, name in enumerate(EVENTS)]
        bank, records = pack_bank(items)
        self.assertTrue(all(r['offset'] % 16 == 0 for r in records))
        self.assertEqual(sum(r['bytes'] for r in records), len(bank))
        with self.assertRaises(ValueError):
            pack_bank(items, limit=len(bank) - 1)
        with self.assertRaises(ValueError):
            pack_bank(items[:-1])
        self.assertLessEqual(len(bank), BANK_LIMIT)


if __name__ == '__main__':
    unittest.main()
