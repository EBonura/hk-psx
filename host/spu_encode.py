#!/usr/bin/env python3
"""spu_encode.py input.s16le output.adpcm [--ring]

Mono PCM16 (little-endian) to flagless PSX ADPCM through the SDK's shared
encoder (host/spu_cook.py). The first block ignores history, so a loop or a
restarted stream has defined history; the transport that plays the payload
owns every flag. `--ring` is for a loop already whole ADPCM blocks (the area
music), whose pre-compensation then wraps across the seam. Prints one line of
JSON: samples, signal and error energy against the exact SPU decode, and SNR.
"""
import json
import math
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import spu_cook  # noqa: E402


def main(argv):
    if len(argv) not in (2, 3) or (len(argv) == 3 and argv[2] != '--ring'):
        sys.exit('usage: spu_encode.py input.s16le output.adpcm [--ring]')
    raw = Path(argv[0]).read_bytes()
    if len(raw) % 2:
        sys.exit('odd-length PCM16 input')
    samples = list(struct.unpack('<%dh' % (len(raw) // 2), raw))
    data = spu_cook.encode_pcm(samples, 'ring' if len(argv) == 3 else 'restart')
    Path(argv[1]).write_bytes(data)
    decoded = spu_cook.decode(data)[:len(samples)]
    signal = float(sum(s * s for s in samples))
    error = float(sum((s - d) ** 2 for s, d in zip(samples, decoded)))
    print(json.dumps({'samples': len(samples), 'signal_energy': signal, 'error_energy': error,
                      'snr_db': 10 * math.log10(signal / error) if error > 0 and signal > 0 else 999.0}))


if __name__ == '__main__':
    main(sys.argv[1:])
