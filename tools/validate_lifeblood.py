#!/usr/bin/env python3
"""Check earned Lifeblood and blue-first damage in an actual-CUE replay.

Consumes tools/replay_cue.py evidence; never builds, patches RAM or writes a disc.
This is a coverage gate, not a claim of source animation or hardware parity.
"""
import argparse
import csv
import hashlib
import json
from pathlib import Path

FIELDS = ('HK_LIFEBLOOD_OPENED', 'HK_LIFEBLOOD_ACTIVE',
          'HK_LIFEBLOOD_STRUCK', 'HK_LIFEBLOOD_GRANTED', 'HK_BLUE_HEALTH',
          'HK_HEALTH', 'HK_DEATHS')


def check_states(states):
    if not states:
        raise ValueError('No gameplay observations')
    opened = any(s['HK_LIFEBLOOD_OPENED'] == 1 and
                 s['HK_LIFEBLOOD_ACTIVE'] == 2 for s in states)
    earned = any(s['HK_LIFEBLOOD_STRUCK'] == 2 and
                 s['HK_LIFEBLOOD_GRANTED'] == 2 and
                 s['HK_BLUE_HEALTH'] == 2 for s in states)
    absorbed = 0
    ordinary_after_blue = False
    for before, after in zip(states, states[1:]):
        if before['HK_LIFEBLOOD_GRANTED'] != 2:
            continue
        if after['HK_DEATHS'] != before['HK_DEATHS']:
            raise ValueError('Death/reset cannot prove blue-health absorption')
        lost = before['HK_BLUE_HEALTH'] - after['HK_BLUE_HEALTH']
        if lost > 0:
            if after['HK_HEALTH'] != before['HK_HEALTH']:
                raise ValueError('Ordinary health changed during the blue-only damage check')
            absorbed += lost
        if absorbed >= 2 and before['HK_BLUE_HEALTH'] == 0 and after['HK_HEALTH'] < before['HK_HEALTH']:
            ordinary_after_blue = True
    checks = {'cocoon_released_two': opened, 'two_masks_earned': earned,
              'two_blue_masks_absorbed': absorbed == 2,
              'ordinary_damage_after_blue_exhausted': ordinary_after_blue}
    return {'passed': all(checks.values()), 'checks': checks}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--replay', type=Path, required=True)
    args = parser.parse_args()
    directory = args.replay.resolve()
    report_path = directory / 'command.json'
    report = json.loads(report_path.read_text())
    if (not report['completed'] or report['exit_code'] or report['faults']
            or not report['inputs_unchanged']):
        raise ValueError('Replay did not complete cleanly with unchanged inputs')
    columns = {name: 'ram_' + report['watches'][name][2:] for name in FIELDS}
    route_path = directory / 'route.csv'
    with route_path.open() as source:
        states = [{name: int(row[column]) for name, column in columns.items()}
                  for row in csv.DictReader(source)]
    states.append({name: report['final_ram'][name] for name in FIELDS})
    result = check_states(states)
    result['evidence_sha256'] = {p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                                 for p in (report_path, route_path, directory / 'ram.bin')}
    (directory / 'lifeblood-coverage.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))
    if not result['passed']:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
