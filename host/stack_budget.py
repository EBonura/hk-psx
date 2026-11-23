"""Project stack reservation layered on the unchanged pinned SDK linker script."""
import hashlib
import json
import os
from pathlib import Path
import struct

STACK_RESERVE = 48 * 1024
# The shared resident scene core's window, 0x801FFF00 - CORE_WINDOW_BYTES .. 0x801FFF00, with the handoff block above it
# (scene-system-1123 PLAN.md 3.2: cap 10,240 B including the core's stack, target 8,192 B). The program links for
# RAM_SIZE = 2 MiB - the window, so STACK_INIT, the stack reserve and the module pool under it all move down by the window.
# HK_CORE_WINDOW_BYTES measures another size; 0 links the whole 2 MiB as before the core.
CORE_WINDOW_BYTES = int(os.environ.get('HK_CORE_WINDOW_BYTES', '10240'), 0)
if CORE_WINDOW_BYTES % 0x100 or not 0 <= CORE_WINDOW_BYTES < 0x10000:
    raise ValueError(f'HK_CORE_WINDOW_BYTES {CORE_WINDOW_BYTES} must be whole 256-byte lines under 64 KiB')
RAM_SIZE = 0x200000 - CORE_WINDOW_BYTES
RAM_TOP = 0x80000000 + RAM_SIZE
STACK_INIT = RAM_TOP - 0x100


def prepare_linker(root, output):
    """The pinned SDK linker script, unchanged, behind one line: the SDK's own `STACK_RESERVE_OVERRIDE` hook.

    The SDK script reads the override when the link defines it before the script's own use of the reserve (a script
    placed first, or, as here, the first line of the one script handed to the linker), and the link map then
    carries the line. Nothing in the SDK text is rewritten."""
    source = root / '.psoxide/sdk/psoxide.ld'
    original = source.read_text()
    if 'DEFINED(STACK_RESERVE_OVERRIDE)' not in original:
        raise ValueError('Pinned SDK has no STACK_RESERVE_OVERRIDE hook; review project reservation')
    if 'DEFINED(RAM_SIZE_OVERRIDE)' not in original:
        raise ValueError('Pinned SDK has no RAM_SIZE_OVERRIDE hook; review the core window')
    generated = f'STACK_RESERVE_OVERRIDE = {STACK_RESERVE:#x}; /* hk-psx: 48 KiB stack reservation */\n' + original
    if CORE_WINDOW_BYTES:
        generated = (f'RAM_SIZE_OVERRIDE = {RAM_SIZE:#x}; /* hk-psx: {CORE_WINDOW_BYTES} B scene-core window under 0x801FFF00 */\n'
                     + generated)
    # Derive the filename from its contents: Cargo must relink when the budget
    # changes, even though it does not track external linker-script contents.
    digest = hashlib.sha256(generated.encode()).hexdigest()
    path = output / f'hk-psx-{digest[:16]}.ld'
    path.write_text(generated)
    (output / 'stack-linker.json').write_text(json.dumps({
        'source': str(source), 'source_sha256': hashlib.sha256(original.encode()).hexdigest(),
        'generated': str(path), 'generated_sha256': digest,
        'stack_reserved_bytes': STACK_RESERVE, 'core_window_bytes': CORE_WINDOW_BYTES, 'stack_init': STACK_INIT,
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
