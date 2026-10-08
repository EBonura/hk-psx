#!/usr/bin/env python3
"""Compare aligned PCM captures; does not infer audible or hardware parity."""
import argparse
import hashlib
import json
from pathlib import Path
import wave
import numpy as np


def read(path):
    with wave.open(str(path)) as wav:
        if (wav.getnchannels(), wav.getsampwidth(), wav.getframerate()) != (2, 2, 44100):
            raise ValueError('Expected stereo 44,100Hz signed16 PCM capture')
        return np.frombuffer(wav.readframes(wav.getnframes()), dtype='<i2').reshape(-1, 2)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--before', type=Path, required=True)
    p.add_argument('--after', type=Path, required=True)
    p.add_argument('--offset', type=int, required=True, help='Before-capture sample offset relative to after')
    p.add_argument('--start-seconds', type=int, default=15)
    p.add_argument('--output', type=Path, required=True)
    a = p.parse_args()
    before, after = read(a.before), read(a.after)
    start = a.start_seconds * 44100
    count = min(len(before) - a.offset, len(after)) - start
    if start < 0 or start + a.offset < 0 or count <= 0:
        raise ValueError('Empty or invalid comparison interval')
    delta = before[start+a.offset:start+a.offset+count].astype(np.int32) - after[start:start+count].astype(np.int32)
    result = {
        'inputs': {str(path.resolve()): hashlib.sha256(path.read_bytes()).hexdigest() for path in (a.before, a.after)},
        'alignment_samples': a.offset, 'from_second': a.start_seconds,
        'duration_seconds': count / 44100, 'channels': 2,
        'differing_samples': int(np.count_nonzero(delta)),
        'max_absolute_error': int(np.max(np.abs(delta))),
        'scope': 'Exact aligned emulator PCM comparison; does not establish original-game audio or physical-console parity.',
    }
    a.output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))
    if result['differing_samples']:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
