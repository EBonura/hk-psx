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
sys.path.insert(0, str(ROOT / 'tools'))
# The record's tag and length come from game/src/save.rs rather than being
# repeated here. They were hardcoded once and went stale the moment the charm
# board joined the record: this file would then have found no save on any card
# and reported a healthy result for a check that never ran.
from migrate_cards import loadable_records

# game/src/save.rs names each file BASLUS-00000HKPSX<profile><copy>.
PREFIX = b'BASLUS-00000HKPSX'


def findable_saves(path):
    """Every save the guest could actually load: in-use directory entry first."""
    card = path.read_bytes()
    out = []
    for block in range(1, 16):
        entry = card[block * 128:(block + 1) * 128]
        name = entry[10:30].split(b'\0')[0]
        if entry[0] != 0x51 or not name.startswith(PREFIX) or len(name) != len(PREFIX) + 2:
            continue
        start = 8192 * block
        # Any format the guest loads counts: the fixture this seeds from is
        # HKS4 and the write under test is HKS5, and both are a save to lose.
        for at, length, magic in loadable_records(card, start, start + 8192)[:1]:
            out.append({'name': name.decode(), 'format': magic.decode(), 'bytes': length,
                        'sequence': struct.unpack_from('<I', card, at + 48)[0],
                        'geo': struct.unpack_from('<I', card, at + 24)[0]})
    return out


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--route', default='bench-save', help='a route whose run writes the card')
    # Resumes at the Dirtmouth bench, so the route sits and accepts the prompt.
    p.add_argument('--card', type=Path, default=ROOT / 'tools/cards/town-continue.mcd')
    p.add_argument('--output', type=Path, required=True)
    a = p.parse_args()
    report = json.loads((ROOT / '.hkpsx/validate' / a.route / 'command.json').read_text())
    a.output.mkdir(parents=True, exist_ok=True)

    def replay(steps, tag, card):
        """One run of the route at `steps`, against `card`, into its own dir."""
        run = a.output / tag
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
        return result, run

    # Seed the profile's second copy first, so the write under test is one that
    # frees an existing directory entry while the other copy carries the save.
    # That is the case a single-file save lost entirely.
    seeded = a.output / 'seeded.mcd'
    shutil.copyfile(a.card, seeded)
    result, _ = replay(6_000_000_000, 'seed', seeded)
    if result.returncode != 0:
        raise SystemExit(f'seed run failed: {result.stderr[:200]}')
    before = findable_saves(seeded)
    if len(before) < 2:
        raise SystemExit('seed run did not leave both copies; the cuts would not test an overwrite')
    # Calibrate against the very card the cuts will use: boot timing depends on
    # what the card holds, so the route's own cycle numbers would put every cut
    # on the wrong side of the write.
    calibration = a.output / 'calibration.mcd'
    shutil.copyfile(seeded, calibration)
    result, run = replay(6_000_000_000, 'calibrate', calibration)
    if result.returncode != 0:
        raise SystemExit(f'calibration run failed: {result.stderr[:200]}')
    rows = list(__import__('csv').DictReader((run / 'route.csv').open()))
    watches = report['watches']
    blocked = 'ram_' + watches['HK_INPUT_BLOCKED_VBLANKS'][2:]
    writes = 'ram_' + watches['HK_SAVE_WRITES'][2:]
    window = [int(r['cpu_tick']) for r in rows if int(r[blocked]) > 0 and int(r[writes]) == 0]
    done = next((int(r['cpu_tick']) for r in rows if int(r[writes]) > 0), None)
    if not window or done is None:
        raise SystemExit('calibration run never completed a card write')
    if findable_saves(calibration) == before:
        raise SystemExit('calibration run did not change the card; the cuts would prove nothing')
    results = []
    # Early, middle and late in the transfer, plus just past its completion.
    span = done - window[0]
    for name, steps in (('early', window[0] + span // 8), ('middle', window[0] + span // 2),
                        ('late', done - span // 8), ('after', done + span)):
        card = a.output / f'{name}.mcd'
        shutil.copyfile(seeded, card)
        result, _ = replay(steps, name, card)
        saves = findable_saves(card)
        results.append({'cut': name, 'steps': steps, 'exit': result.returncode, 'saves': saves,
                        'recoverable': bool(saves)})
        print(f'{name:7s} at {steps}: ' + ('OK   ' if saves else 'LOST ') + json.dumps(saves))
    if results[-1]['saves'] == before:
        raise SystemExit('the cut past the write did not land the new save; the window is wrong')
    (a.output / 'power-cut.json').write_text(json.dumps(
        {'route': a.route, 'card_before': before, 'results': results}, indent=2) + '\n')
    lost = [r for r in results if not r['recoverable']]
    if lost:
        raise SystemExit(f'{len(lost)} cut(s) left no loadable save')
    print(f'All {len(results)} interruption points leave a loadable save.')


if __name__ == '__main__':
    main()
