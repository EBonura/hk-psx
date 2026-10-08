"""The SDK's shared SPU-ADPCM encoder (psx-audio-cook) for every HK cooker.

One encoder for the resident one-shots, the scene and Geo banks, the ambience,
Focus and Runner loops and the area-music ring: pre-compensation for the SPU's
Gaussian interpolation and a trellis search against the exact SPU decoder
(the previous cookers ran a predictor-zero Python encoder and a greedy C one).
The input is PCM already at the playback rate, so every length contract the
cookers check (resampled by ffmpeg, as before) is unchanged. The command line
is built from the pinned `.psoxide` by tools/psx-audio-cook.
"""
import os
import struct
import subprocess
import tempfile
import wave
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CRATE = ROOT / 'tools/psx-audio-cook'
FILTERS = ((0, 0), (60, 0), (115, -52), (98, -55), (122, -60))
# The encoder never resamples here: the PCM goes in and out at one nominal
# rate, and nothing it does depends on which.
NOMINAL_RATE = 22050
_BINARY = None


def binary():
    """The psx-audio-cook command line, built once per process."""
    global _BINARY
    if _BINARY is None:
        subprocess.run(['cargo', 'build', '-q', '--release', '--manifest-path', str(CRATE / 'Cargo.toml')],
                       check=True)
        _BINARY = CRATE / 'target/release/psx-audio-cook'
    return _BINARY


def encode_pcm(samples, loop='none'):
    """Signed 16-bit mono samples to ADPCM blocks with every flag zero.

    `loop` is 'none' for a one-shot, 'restart' for a stream its transport
    restarts (same length, history-free first block), or 'ring' for a loop
    that is already whole ADPCM blocks (history-free first block, and the
    pre-compensation wraps across the seam).
    """
    samples = list(samples)
    if not samples:
        return b''
    if loop == 'ring' and len(samples) % 28:
        raise ValueError('a ring loop must be whole ADPCM blocks')
    mode = {'none': 'none', 'restart': 'restart', 'ring': 'whole'}[loop]
    with tempfile.TemporaryDirectory() as tmp:
        source, out = Path(tmp) / 'in.wav', Path(tmp) / 'out.adpcm'
        with wave.open(str(source), 'wb') as w:
            w.setnchannels(1)
            w.setsampwidth(2)
            w.setframerate(NOMINAL_RATE)
            w.writeframes(struct.pack('<%dh' % len(samples), *samples))
        subprocess.run([str(binary()), 'encode', str(source), str(out), '--rate', str(NOMINAL_RATE),
                        '--format', 'raw', '--no-normalize', '--no-flags', '--loop', mode],
                       check=True, stdout=subprocess.DEVNULL)
        data = out.read_bytes()
    if len(data) != -(-len(samples) // 28) * 16:
        raise ValueError(f'encoded {len(data)} bytes for {len(samples)} samples')
    return data


def decode(adpcm):
    """ADPCM blocks decoded as the SPU decodes them (clamped history, shift
    13-15 read as 9), flags ignored."""
    if len(adpcm) % 16:
        raise ValueError('ADPCM block alignment')
    out = []
    s1 = s2 = 0
    for start in range(0, len(adpcm), 16):
        header = adpcm[start]
        f1, f2 = FILTERS[min(header >> 4, 4)]
        shift = header & 15
        shift = 9 if shift > 12 else shift
        for packed in adpcm[start + 2:start + 16]:
            for nibble in (packed & 15, packed >> 4):
                signed = nibble - 16 if nibble > 7 else nibble
                value = ((signed << 12) >> shift) + ((s1 * f1) >> 6) + ((s2 * f2) >> 6)
                value = max(-32768, min(32767, value))
                out.append(value)
                s2, s1 = s1, value
    return out


def resample(wav_bytes, rate):
    """Mono 16-bit PCM at `rate` through the SDK's shared resampler.

    `psx_audio_cook::resample::Sinc` (Kaiser-windowed sinc, low-passed before
    it downsamples) via this repository's CLI (`resample`), the same filter
    every game's Rust cooker uses. Returns `round(len * rate / source)` samples.
    """
    with tempfile.TemporaryDirectory() as tmp:
        source, out = Path(tmp) / 'in.wav', Path(tmp) / 'out.wav'
        source.write_bytes(wav_bytes)
        subprocess.run([str(binary()), 'resample', str(source), str(out), '--rate', str(rate)],
                       check=True, stdout=subprocess.DEVNULL)
        with wave.open(str(out)) as w:
            if w.getnchannels() != 1 or w.getsampwidth() != 2 or w.getframerate() != rate:
                raise ValueError('SDK resampler returned an unexpected WAV')
            frames = w.readframes(w.getnframes())
    return list(struct.unpack('<%dh' % (len(frames) // 2), frames))
