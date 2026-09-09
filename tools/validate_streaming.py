#!/usr/bin/env python3
"""Replay reproducible traversal routes with the strict guest continuity gate.

Checks one exact build report; optional user recordings are copied into the
evidence directory before replay. Never builds or copies disc images.
"""
import argparse
import concurrent.futures
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys

from validate import DOOR_ROUTE, poll_tape
from validate_focus import FOCUS_ROUTE

ROOT = Path(__file__).resolve().parents[1]
BLACKOUT_TAPE = '2e8722e2ea55eb3030fe641bffb917a60aa8946c5607bf6c39b03a1fda31e8a1'


def blackout_check(tape_hash, analysis, display):
    """The preserved long recording ends outside inverse_remask_right.

    Check route coverage as well as scene pixels: a fast black frame or a run
    that never reaches the affected chamber must not pass the promotion gate.
    This is a regression fixture, not a general image-fidelity measurement.
    """
    if tape_hash != BLACKOUT_TAPE:
        return None
    failures=[];state=analysis.get('final_ram') or {}
    if not (state.get('HK_REGION_ID') == 8
            and 134 <= state.get('HK_PLAYER_X', -1000) <= 159
            and 3 <= state.get('HK_PLAYER_Y', -1000) <= 9):
        failures.append('Recording did not reach the formerly obscured lower chamber')
    if state.get('HK_REVEAL_MASKS_HIDDEN',0) < 2:
        failures.append('Both inverse masks must be hidden at the final outside-trigger pose')
    bright=0
    if display.is_file():
        # The pinned frontend writes this exact binary PPM header. Exclude
        # the HUD so visible health masks cannot disguise a black scene.
        header=b'P6\n320 240\n255\n';raw=display.read_bytes()
        if raw.startswith(header) and len(raw)==len(header)+320*240*3:
            pixels=raw[len(header)+320*40*3:]
            bright=sum(max(pixels[i:i+3])>64 for i in range(0,len(pixels),3))
        else:failures.append('Missing or malformed final 320x240 scene capture')
    else:failures.append('No final scene capture')
    if bright<3200:
        failures.append('Lower scene remains obscured; fewer than5% of scene pixels exceed64')
    return {'passed':not failures,'failures':failures,'scene_bright_pixels':bright,
            'scope':'Exact preserved blackout recording and its final lower-chamber pose'}


def write_routes(output):
    routes = {
        'return': (DOOR_ROUTE + ',500:left:446,650:cross:24,780:cross:24,910:cross:24', 1024),
        'boundary-jumps': (
            '9:start:1,54:right:386,285:cross:24,410:cross:24,' +
            ','.join(f'{n}:square:2' for n in range(78, 440, 24)) + ',' +
            ','.join(f'{n}:{"left" if i % 2 == 0 else "right"}:60'
                     for i, n in enumerate(range(440, 1160, 60))) + ',' +
            ','.join(f'{n}:cross:24' for n in range(450, 1100, 120)) + ',' +
            ','.join(f'{n}:square:2' for n in range(442, 1160, 26)), 1280),
        'long-combat': (FOCUS_ROUTE, 2400),
    }
    paths = {}
    for name, (events, count) in routes.items():
        path = output / (name + '.pxtape')
        poll_tape(path, events, count)
        paths[name] = path
    return paths


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--build-report', type=Path, default=ROOT / '.hkpsx/build.json')
    parser.add_argument('--emulator', type=Path, default=ROOT.parent / 'PSoXide-emulator/target/release/frontend')
    parser.add_argument('--output', type=Path, required=True, help='New evidence directory')
    parser.add_argument('--recording', type=Path, action='append', default=[])
    parser.add_argument('--jobs', type=int, choices=(1, 2), default=1)
    args = parser.parse_args()
    build = json.loads(args.build_report.read_text())
    for path, meta in build['outputs'].items():
        if digest(ROOT / path) != meta['sha256']:
            raise ValueError('Build artifact changed: ' + path)
    link = Path(build['link_map']['path'])
    if digest(link) != build['link_map']['sha256']:
        raise ValueError('Build map changed')
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    tapes = out / 'inputs'
    tapes.mkdir()
    routes = write_routes(tapes)
    for i, source in enumerate(args.recording):
        name = f'recording-{i + 1}'
        routes[name] = tapes / (name + '.pxtape')
        shutil.copyfile(source, routes[name])
    (out / 'build-report.json').write_text(json.dumps(build, indent=2) + '\n')

    def replay(item):
        name, tape = item
        command = [sys.executable, str(ROOT / 'tools/profile_tape.py'),
                   '--exe', str(build['artifacts']['exe']), '--map', str(link),
                   '--cue', str(build['artifacts']['cue']), '--frontend', str(args.emulator.resolve()),
                   '--tape', str(tape), '--output', str(out / name), '--require-seamless']
        with (out / (name + '.log')).open('w') as log:
            result = subprocess.run(command, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT)
        analysis_path = out / name / 'analysis.json'
        analysis = json.loads(analysis_path.read_text()) if analysis_path.exists() else {}
        tape_hash=digest(tape)
        visual=blackout_check(tape_hash,analysis,out/name/'display.ppm')
        return name, {'returncode': result.returncode or int(visual is not None and not visual['passed']),
                      'tape_sha256': tape_hash, 'blackout_check':visual,
                      'seamless_check': analysis.get('seamless_check'),
                      'final_ram': analysis.get('final_ram'),
                      'transitions': analysis.get('transitions'),
                      'all_frame_intervals': analysis.get('all_frame_intervals')}

    with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
        results = dict(pool.map(replay, routes.items()))
    report = {'build_report': str(args.build_report.resolve()), 'routes': results,
              'passed': all(r['returncode'] == 0 for r in results.values()),
              'limitations': 'Emulator evidence only. Routes do not cover every possible traversal or physical CD timing.'}
    (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    for name, result in results.items():
        print(name, result['seamless_check'] or 'Replay failed before analysis')
    if not report['passed']:
        raise SystemExit('Streaming validation failed; see ' + str(out / 'report.json'))


if __name__ == '__main__':
    main()
