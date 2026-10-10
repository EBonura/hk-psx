"""Project stack reservation layered on the unchanged pinned SDK linker script."""
import hashlib
import json
from pathlib import Path
import re
import struct

STACK_RESERVE = 48 * 1024


def prepare_linker(root, output):
    source = root / '.psoxide/sdk/psoxide.ld'
    original = source.read_text()
    generated, count = re.subn(r'^STACK_RESERVE = 0x8000;[^\n]*$',
                              'STACK_RESERVE = 0xc000; /* hk-psx: 48 KiB stack reservation */',
                              original, flags=re.M)
    if count != 1:
        raise ValueError('Pinned SDK stack declaration changed; review project reservation')
    # STACK_INIT leaves 256 bytes at the top of RAM. Exclude those too so
    # the linker's static limit agrees with the reported stack floor.
    generated, count = re.subn(r'LENGTH = EXE_HEAD_BYTES \+ RAM_SIZE - BIOS_SIZE - STACK_RESERVE',
                              'LENGTH = EXE_HEAD_BYTES + STACK_INIT - LOAD_ADDR - STACK_RESERVE', generated)
    if count != 1:
        raise ValueError('Pinned SDK RAM region changed; review project reservation')
    # Derive the filename from its contents: Cargo must relink when the budget
    # changes, even though it does not track external linker-script contents.
    digest = hashlib.sha256(generated.encode()).hexdigest()
    path = output / f'hk-psx-{digest[:16]}.ld'
    path.write_text(generated)
    (output / 'stack-linker.json').write_text(json.dumps({
        'source': str(source), 'source_sha256': hashlib.sha256(original.encode()).hexdigest(),
        'generated': str(path), 'generated_sha256': digest,
        'stack_reserved_bytes': STACK_RESERVE,
    }, indent=2) + '\n')
    return path


def main_frame_bytes(exe, map_text):
    """Read the actual main prologue; this is a lower bound, not high-water."""
    address = next((int(fields[0], 16) for line in map_text.splitlines()
                    if len(fields := line.split()) == 5 and fields[4] == 'main'), None)
    if address is None:
        raise ValueError('Missing main symbol for stack reservation check')
    data = Path(exe).read_bytes()
    base = struct.unpack_from('<I', data, 0x18)[0]
    offset = 2048 + address - base
    if not 2048 <= offset <= len(data) - 8:
        raise ValueError('main symbol outside EXE payload')
    first, second = struct.unpack_from('<II', data, offset)
    if first >> 16 == 0x27bd and first & 0x8000:  # addiu sp, sp, negative immediate
        return 0x10000 - (first & 0xffff)
    if first >> 16 == 0x3401 and second == 0x03a1e823:  # ori at, zero, size; subu sp, sp, at
        return first & 0xffff
    raise ValueError('Unknown main stack prologue; inspect final binary before admitting build')


def check_main_frame(exe, map_text):
    frame = main_frame_bytes(exe, map_text)
    if frame >= STACK_RESERVE:
        raise ValueError(f'main alone uses {frame} bytes of {STACK_RESERVE}-byte stack reserve')
    return frame
