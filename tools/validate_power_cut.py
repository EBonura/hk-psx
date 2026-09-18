#!/usr/bin/env python3
"""Cut power inside a memory card write; a findable valid save must remain.

`Card::write` frees a file's directory entry before writing its data and only
restores it at the end, so a single-file save has a window of roughly 190 ms in
which an interruption leaves nothing loadable. game/src/save.rs alternates
between two files so the copy being relied on is never the one being written.
This replays the death route, stops the emulator mid-write, and checks the card.
"""
import argparse, json, shutil, struct, subprocess, sys
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]
NAMES = (b'BASLUS-00000HKPSX000', b'BASLUS-00000HKPSX001')


def fnv(data):
    value = 0x811c9dc5
    for byte in data:
        value = ((value ^ byte) * 16777619) & 0xffffffff
    return value


def findable_saves(path):
    """Every save the guest could actually load: in-use directory entry first."""
    card = path.read_bytes()
    out = []
    for block in range(1, 16):
        entry = card[block * 128:(block + 1) * 128]
        if entry[0] != 0x51 or entry[10:30].split(b'\0')[0] not in NAMES:
            continue
        start = 8192 * block
        at = card.find(b'HKS1', start, start + 8192)
        if at < 0:
            continue
        if fnv(card[at:at + 52]) == struct.unpack_from('<I', card, at + 52)[0]:
            out.append({'name': entry[10:30].split(b'\0')[0].decode(),
                        'sequence': struct.unpack_from('<I', card, at + 48)[0],
                        'geo': struct.unpack_from('<I', card, at + 24)[0]})
    return out


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--route', default='kings-death', help='a route whose run writes the card')
    p.add_argument('--card', type=Path, default=ROOT / 'tools/cards/town-continue.mcd')
    p.add_argument('--output', type=Path, required=True)
    a = p.parse_args()
    report = json.loads((ROOT / '.hkpsx/validate' / a.route / 'command.json').read_text())
    rows = list(__import__('csv').DictReader((ROOT / '.hkpsx/validate' / a.route / 'route.csv').open()))
    watches = report['watches']
    blocked = 'ram_' + watches['HK_INPUT_BLOCKED_VBLANKS'][2:]
    writes = 'ram_' + watches['HK_SAVE_WRITES'][2:]
    window = [int(r['cpu_tick']) for r in rows if int(r[blocked]) > 0 and int(r[writes]) == 0]
    done = next(int(r['cpu_tick']) for r in rows if int(r[writes]) > 0)
    if not window:
        raise SystemExit('route never blocks on a card write')
    before = findable_saves(a.card)
    if not before:
        raise SystemExit('the starting card holds no loadable save')
    a.output.mkdir(parents=True, exist_ok=True)
    results = []
    # Early, middle and late in the transfer, plus just past its completion.
    span = done - window[0]
    for name, steps in (('early', window[0] + span // 8), ('middle', window[0] + span // 2),
                        ('late', done - span // 8), ('after', done + span)):
        card = a.output / f'{name}.mcd'
        shutil.copyfile(a.card, card)
        run = a.output / name
        shutil.rmtree(run, ignore_errors=True)
        run.mkdir(parents=True)
        command = list(report['command'])
        command[command.index('--steps') + 1] = str(steps)
        command[command.index('--memcard') + 1] = str(card)
        command[command.index('--config-dir') + 1] = str(run / 'emulator')
        for flag in ('--route-log', '--route-screenshot-dir', '--dump-display', '--dump-hw',
                     '--dump-vram', '--dump-spu-ram', '--dump-ram', '--dump-audio', '--cd-command-log'):
            if flag in command:
                command[command.index(flag) + 1] = str(run / Path(command[command.index(flag) + 1]).name)
        result = subprocess.run(command, capture_output=True, text=True)
        saves = findable_saves(card)
        results.append({'cut': name, 'steps': steps, 'exit': result.returncode, 'saves': saves,
                        'recoverable': bool(saves)})
        print(f'{name:7s} at {steps}: ' + ('OK   ' if saves else 'LOST ') + json.dumps(saves))
    (a.output / 'power-cut.json').write_text(json.dumps(
        {'route': a.route, 'card_before': before, 'results': results}, indent=2) + '\n')
    lost = [r for r in results if not r['recoverable']]
    if lost:
        raise SystemExit(f'{len(lost)} cut(s) left no loadable save')
    print(f'All {len(results)} interruption points leave a loadable save.')


if __name__ == '__main__':
    main()
