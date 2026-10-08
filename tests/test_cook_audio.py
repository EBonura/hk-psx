"""Independent sample reconstruction and fail-closed resident admission."""
import math
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
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))
from cook_audio import (BANK_LIMIT, BOSS_EVENTS, EVENTS, convert_wav, decode_oneshot,
                        death_action_layers, encode, encoded_bytes,
                        false_knight_audio_contract, pack_bank)

# The two clips BOSS_EVENTS admits, keyed by the path id the fixture FSM cites.
BOSS_CLIP_IDS = {1: 'sharedassets32.assets:131', 2: 'sharedassets48.assets:39'}


def audio_state(name, path_id):
    """One FalseyControl state whose single enabled AudioPlaySimple plays a clip."""
    data = {'actionNames': ['AudioPlaySimple.AudioPlaySimple'], 'actionEnabled': [1],
            'actionStartIndex': [0], 'paramName': ['oneShotClip', 'volume'],
            'paramDataPos': [0, 0], 'paramDataType': [24, 15], 'paramByteDataSize': [0, 5],
            'byteData': list(struct.pack('<fB', 1.0, 0)),
            'fsmObjectParams': [{'typeName': 'UnityEngine.AudioClip', 'useVariable': 0,
                                 'value': {'m_FileID': 10, 'm_PathID': path_id}}]}
    return {'name': name, 'actionData': data}


def false_knight_fsm():
    return {'name': 'FalseyControl',
            'states': [audio_state(name, 1) for name in ('S Land', 'State 2', 'Land Noise')]
                      + [audio_state(name, 2) for name in ('S Attack', 'JA Hit 2')]}


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
        self.assertEqual(len(decode_oneshot(encoded)), 56)
        self.assertEqual(decode_oneshot(encoded)[29:], [0] * 27)

    def test_signed_extrema_decode_without_overflow(self):
        # Full-scale content at the Nyquist rate: the shared encoder band-
        # limits it (nothing at exactly Nyquist survives), so only the decode's
        # clamping is pinned here, not the samples.
        samples = [-32768, 32767, -4096, 4096] * 7
        decoded = decode_oneshot(encode(samples))
        self.assertEqual(len(decoded), 28)
        self.assertTrue(all(-32768 <= x <= 32767 for x in decoded))

    def test_a_full_scale_tone_codes_closely(self):
        samples = [round(32000 * math.sin(i * 0.3)) for i in range(280)]
        decoded = decode_oneshot(encode(samples))
        error = sum((a - b) ** 2 for a, b in zip(samples, decoded))
        signal = sum(a * a for a in samples)
        self.assertGreater(10 * math.log10(signal / error), 20)

    def test_downmix_and_resample_preserve_duration_and_level(self):
        # Real clip lengths, since the resampler primes its filter.
        pcm, meta = convert_wav(wav([1000, -1000, 3000, 1000, -1000, -3000] * 4000, 2), 22050)
        self.assertEqual(meta['source_frames'], 12000)
        self.assertEqual(meta['source_channels'], 2)
        self.assertEqual(len(pcm), 6000)
        self.assertEqual(meta['samples'], 6000)
        # A constant survives the conversion exactly: no droop, no DC shift.
        flat, _ = convert_wav(wav([12345] * 97600, rate=48000), 11025)
        self.assertEqual(len(flat), 22418)
        self.assertEqual(set(flat), {12345})

    def test_resample_rejects_the_signal_above_the_new_nyquist(self):
        pcm, _ = convert_wav(wav([10000, -10000] * 2000), 11025)
        # A real filter leaves a finite stopband residue where the old box
        # average cancelled exactly; 40 dB down is the property that matters.
        self.assertLess(max(abs(x) for x in pcm), 1000)

    def test_non_integer_rate_conversion_keeps_duration_and_pitch(self):
        pcm, meta = convert_wav(wav([1234] * 97600, rate=48000), 11025)
        self.assertEqual(len(pcm), 22418)
        self.assertEqual(meta['samples'], 22418)

    def test_empty_clip_has_no_invented_sample(self):
        pcm, meta = convert_wav(wav([], rate=48000), 11025)
        self.assertEqual(pcm, [])
        self.assertEqual(meta['samples'], 0)

    def test_a_truncated_resample_is_rejected_rather_than_shipped(self):
        # Shorter than the filter's priming window: must fail, not lose audio.
        with self.assertRaises(ValueError):
            convert_wav(wav([1000, -1000, 3000], 1), 22050)

    def test_bad_loop_flags_are_rejected(self):
        bank = bytearray(encode([1000] * 28))
        bank[1] = 4
        with self.assertRaises(ValueError):
            decode_oneshot(bank)

    def test_budget_is_atomic_and_sample_starts_aligned(self):
        items = [(name, encode([1000] * (i + 1)), {}) for i, name in enumerate(EVENTS)]
        boss = [(event[0], encode([1000] * (i + 1)), {}) for i, event in enumerate(BOSS_EVENTS)]
        bank, records = pack_bank(items, boss)
        self.assertTrue(all(r['offset'] % 16 == 0 for r in records))
        self.assertEqual(sum(r['bytes'] for r in records), len(bank))
        # The hero events keep the first eight indices, which audio.rs binds
        # one-to-one to its dedicated voices.
        self.assertEqual(tuple(r['event'] for r in records), EVENTS + tuple(e[0] for e in BOSS_EVENTS))
        with self.assertRaises(ValueError):
            pack_bank(items, boss, limit=len(bank) - 1)
        with self.assertRaises(ValueError):
            pack_bank(items[:-1], boss)
        with self.assertRaises(ValueError):
            pack_bank(items, boss[:-1])
        self.assertLessEqual(len(bank), BANK_LIMIT)

    def test_encoded_size_is_predicted_exactly_before_a_clip_is_cooked(self):
        # The refusal table is sized rather than cooked, so the estimator has
        # to agree with the real path on every shape, including the exact
        # multiple of 28 and the one sample the resampler may round away.
        for frames, source_rate, rate in [(28, 44100, 44100), (29, 44100, 44100),
                                          (100000, 44100, 22050), (60543, 44100, 11025),
                                          (10884, 44100, 22050), (1, 44100, 8000)]:
            samples = round(frames * rate / source_rate)
            self.assertEqual(encoded_bytes(frames, source_rate, rate), len(encode([0] * samples)))
        with self.assertRaises(ValueError):
            encoded_bytes(0, 44100, 22050)

    def test_boss_contract_refuses_a_state_that_stopped_playing_its_clip(self):
        fsm = false_knight_fsm()
        bindings = false_knight_audio_contract(fsm, lambda ref: BOSS_CLIP_IDS[ref['m_PathID']])
        self.assertEqual([b['event'] for b in bindings], [e[0] for e in BOSS_EVENTS])
        self.assertEqual(bindings[0]['states'], ['S Land', 'State 2', 'Land Noise'])
        for break_it in (lambda f: f['states'].pop(0),
                         lambda f: f.__setitem__('name', 'Something Else'),
                         lambda f: f['states'][0]['actionData']['actionEnabled'].__setitem__(0, 0)):
            broken = copy.deepcopy(fsm)
            break_it(broken)
            with self.assertRaises(ValueError):
                false_knight_audio_contract(broken, lambda ref: BOSS_CLIP_IDS[ref['m_PathID']])


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
        from ambience import fnv
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
