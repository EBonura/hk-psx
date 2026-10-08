#!/usr/bin/env python3
"""Prepare poll-bound cheat menu routes, or verify their actual-CUE replay reports.

Run --prepare PATH, then tools/replay_cue.py for each tape into PATH/title,
PATH/pause and PATH/reset. --replay PATH checks gameplay values and input hashes.
No guest RAM edits, disc copies or asset changes.
"""
import argparse
import hashlib
import json
from pathlib import Path
from validate import poll_tape

TITLE = '9:up:1,12:cross:1,15:cross:1,18:down:1,21:cross:1,24:down:1,27:cross:1,30:down:1,33:cross:1,36:circle:1,39:down:1,42:start:1,200:start:1,203:up:1,206:cross:1'
PAUSE = '9:start:1,120:start:1,123:up:1,126:cross:1,129:cross:1,132:down:1,135:cross:1,138:down:1,141:cross:1,144:down:1,147:cross:1,150:down:1,153:cross:1,156:down:1,159:cross:8,170:cross:1,174:cross:1,178:cross:1,182:circle:1,186:circle:1'
RESET = PAUSE + ',220:select:1,280:start:1,283:up:1,286:cross:1,289:up:1,292:up:1,295:cross:1,298:circle:1,301:circle:1'
ROUTES = {'title': (TITLE, 260), 'pause': (PAUSE, 210), 'reset': (RESET, 360)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument('--prepare', type=Path)
    mode.add_argument('--replay', type=Path)
    args = parser.parse_args()
    if args.prepare:
        args.prepare.mkdir(parents=True, exist_ok=True)
        for name, (events, count) in ROUTES.items():
            poll_tape(args.prepare / (name + '.pxtape'), events, count)
        return
    checks, hashes = {}, {}
    for name in ROUTES:
        path = args.replay / name / 'command.json'
        data = path.read_bytes()
        report = json.loads(data)
        ram = report['final_ram']
        enabled = name != 'reset'
        expected = {'HK_CHEATS': 15 if enabled else 0,
                    'HK_CHEAT_NAIL_DAMAGE': 21 if enabled else 5,
                    'HK_CHEAT_MASK_CAP': 9 if enabled else 5,
                    'HK_HEALTH': 9 if enabled else 5,
                    'HK_SOUL': 99, 'HK_BLUE_HEALTH': 20 if name == 'pause' else 0,
                    'HK_PAUSED': int(name == 'title'), 'HK_GAME_MODE': 1}
        checks[name] = {'completed': report['completed'] and report['exit_code'] == 0,
                        'stable_inputs': report['inputs_unchanged'], 'no_faults': not report['faults'],
                        **{key: ram.get(key) == value for key, value in expected.items()}}
        if name == 'reset':
            checks[name]['cheats_were_enabled'] = report['maximum_observed']['HK_CHEATS'] == 15
        hashes[name] = hashlib.sha256(data).hexdigest()
    result = {'passed': all(all(row.values()) for row in checks.values()),
              'checks': checks, 'command_sha256': hashes}
    (args.replay / 'validation.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))
    if not result['passed']:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
