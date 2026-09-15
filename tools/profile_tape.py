#!/usr/bin/env python3
"""Replay a recorded poll-bound tape with out-of-band performance diagnostics.

Writes no disc or guest artifacts. Profiles are bound to the supplied EXE/map,
mounted BIN/CUE, tape, and emulator hashes. A different build may take a different
route even with the same inputs; compare the reported state and transitions too.
"""
import argparse
import collections
import csv
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess
import time

WATCH = ('HK_PLAYER_X', 'HK_PLAYER_Y', 'HK_REGION_ID',
         'HK_CD_SECTORS_READ', 'HK_ROOM_LOAD_STATE', 'HK_ROOM_LOAD_ERROR',
         'HK_HEALTH', 'HK_GAME_MODE', '__psx_rt_fault_count')
OPTIONAL_WATCH = ('HK_LIFEBLOOD_OPENED','HK_LIFEBLOOD_ACTIVE','HK_LIFEBLOOD_STRUCK','HK_LIFEBLOOD_GRANTED','HK_BLUE_HEALTH','HK_GEO_WALLET','HK_GEO_HITS','HK_GEO_ROCKS_DEPLETED','HK_GEO_SPAWNED_VALUE','HK_GEO_COLLECTED_VALUE','HK_GEO_ACTIVE','HK_GEO_PENDING_VALUE','HK_GEO_LOST','HK_GEO_DRAWN','HK_PAUSED','HK_SFX_LEVEL','HK_AMBIENCE_LEVEL','HK_REGION_LOADS','HK_ROOM_DECODE_PHASE','HK_ROOM_READ_REGION','HK_ROOM_DECODE_REGION','HK_BOUNDARY_WAIT_TICKS', 'HK_BOUNDARY_WAIT_MAX',
                  'HK_SCENE_LOADS','HK_SCENE_READ_ID','HK_SCENE_DECODE_ID','HK_REGION_ACTIVATIONS',
                  'HK_ANIM_CACHE_HITS','HK_ANIM_CACHE_MISSES','HK_ANIM_UPLOAD_BYTES','HK_ANIM_UPLOAD_MAX_FRAME',
                  'HK_PAD_POLL_MAX_VBLANK_GAP', 'HK_CD_IRQ_MAX_TIMER2_TICKS',
                  'HK_CD_STREAM_ERROR', 'HK_CD_DISCARDED_SECTORS',
                  'HK_VRAM_UPLOAD_BYTES', 'HK_VRAM_UPLOAD_MAX_FRAME',
                  'HK_VRAM_BANK_HITS', 'HK_VRAM_BANK_MISSES',
                  'HK_VRAM_ACTIVE_BANK', 'HK_VRAM_PENDING_REGION',
                  'HK_SCENERY_REPAIRS', 'HK_SCENERY_REPAIR_FAILURES',
                  'HK_REPAIR_OCCLUSION_QUADS', 'HK_REPAIR_OCCLUSION_SAVED_PIXELS',
                  'HK_SCISSOR_QUADS', 'HK_SCISSOR_SAVED_PIXELS',
                  'HK_SCISSOR_CAPACITY_FALLBACKS',
                  'HK_REVEAL_MASKS_HIDDEN', 'HK_REVEAL_MASKS_PARTIAL',
                  'HK_REVEAL_MASKS_VISIBLE',
                  'HK_READ_SOURCE','HK_READ_PAGE','HK_READ_OPENED','HK_READ_CLOSED',
                  'HK_DEBRIS_ACTIVE','HK_DEBRIS_DRAWN','HK_BREAK_COUNT','HK_ENEMY_KILLS',
                  'HK_DEBRIS_BOUNCES','HK_DEBRIS_ASLEEP','HK_PARTICLES_ACTIVE',
                  'HK_PARTICLES_SPAWNED','HK_PARTICLES_DROPPED','HK_PARTICLES_DRAWN',
    'HK_ROOM_STORED_COMPLETIONS','HK_ROOM_STORED_HITS','HK_INPUT_POLLS', 'HK_INPUT_MISSED_VBLANKS',
                  'HK_INPUT_QUEUE_PEAK', 'HK_INPUT_FAULT',
                  'HK_SCISSOR_CACHE_HITS', 'HK_SCISSOR_CACHE_MISSES',
                  'HK_SCISSOR_SECONDARY_HITS', 'HK_SCISSOR_SECONDARY_MISSES',
                  'HK_CORE_MAP_CACHE_HITS', 'HK_CORE_MAP_CACHE_MISSES',
                  'HK_CORE_QUADS', 'HK_OCCLUSION_QUADS', 'HK_OCCLUSION_SAVED_PIXELS', 'HK_HIDDEN_QUADS', 'HK_HIDDEN_PIXELS', 'HK_ROTATED_OCCLUSION_QUADS',
                  'HK_ROTATED_CORE_CACHE_HITS', 'HK_ROTATED_CORE_CACHE_MISSES', 'HK_ROTATED_CORE_OCCLUDERS',
                  'HK_SOLID_CORE_CACHE_HITS', 'HK_SOLID_CORE_CACHE_MISSES', 'HK_SOLID_CORE_OCCLUDERS',
                  'HK_DYNAMIC_HIDDEN_QUADS',
                  'HK_TILE_PREPARES', 'HK_TILE_PREPARE_HITS', 'HK_TILE_PREPARE_MISSES',
                  'HK_BACK_PREBUILDS', 'HK_BACK_PREBUILD_HITS', 'HK_BACK_PREBUILD_MISSES',
                  'HK_BACK_PREBUILD_PACKETS', 'HK_BACK_PREBUILD_LINES',
                  'HK_TILE_COVERAGE_CANDIDATES', 'HK_TILE_COVERAGE_LOOKUPS', 'HK_TILE_COVERAGE_TILES',
                  'HK_TILE_OCCLUSION_QUADS', 'HK_TILE_SAVED_PIXELS', 'HK_TILE_FALLBACKS',
                  'HK_FRAME_FIRST_KICK_LINES', 'HK_FRAME_LAST_KICK_LINES', 'HK_FRAME_FLIP_LINES',
                  '__psx_rt_vblank_count')


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def analyze(output, watches, map_text):
    def rows(name):
        with (output / name).open() as source:
            return list(csv.DictReader(source))
    route = rows('route.csv')
    if not route:
        raise ValueError('No route rows were recorded')
    def value(row, name):
        return int(row['ram_' + watches[name][2:]])
    gameplay = [r for r in route if value(r, 'HK_GAME_MODE') == 1]
    # Include loading/boundary states after entering gameplay: excluding them
    # would hide precisely the transition delays this report is meant to catch.
    lo = int(gameplay[0]['route_tick']) if gameplay else len(route)
    flips = [r for r in route if int(r['route_tick']) >= lo and int(r['display_start_changed'])]
    intervals = []
    for before, after in zip(flips, flips[1:]):
        intervals.append({
            'route_ticks': int(after['route_tick']) - int(before['route_tick']),
            'bus_cycles': int(after['bus_cycles']) - int(before['bus_cycles']),
            'from_poll': int(before['port1_polls']),
            'to_poll': int(after['port1_polls']),
            'sectors_read': value(after, 'HK_CD_SECTORS_READ') - value(before, 'HK_CD_SECTORS_READ'),
            'loads': value(after, 'HK_REGION_LOADS') - value(before, 'HK_REGION_LOADS') if 'HK_REGION_LOADS' in watches else 0,
            'from_region': value(before, 'HK_REGION_ID'),
            'to_region': value(after, 'HK_REGION_ID'),
        })
        for name in ('HK_BOUNDARY_WAIT_TICKS', 'HK_CD_DISCARDED_SECTORS'):
            if name in watches:
                intervals[-1][name.lower() + '_delta'] = value(after, name) - value(before, name)
    no_cd = [r for r in intervals if not r['sectors_read'] and not r['loads']]
    with_cd = [r for r in intervals if r['sectors_read'] or r['loads']]
    with_boundary_wait = [r for r in intervals if r.get('hk_boundary_wait_ticks_delta', 0) > 0]
    def distribution(items):
        durations = sorted(r['route_ticks'] for r in items)
        def percentile(percent):
            return durations[max(0, (len(durations) * percent + 99) // 100 - 1)] if durations else None
        return {'count': len(items),
                'total_route_ticks': sum(r['route_ticks'] for r in items),
                'total_bus_cycles': sum(r['bus_cycles'] for r in items),
                'max_route_ticks': max(durations) if durations else None,
                'max_bus_cycles': max((r['bus_cycles'] for r in items), default=None),
                'p50_route_ticks': percentile(50), 'p95_route_ticks': percentile(95),
                'mean_route_ticks': sum(r['route_ticks'] for r in items) / len(items) if items else None,
                'histogram': dict(sorted(collections.Counter(r['route_ticks'] for r in items).items()))}
    poll_gaps = []
    last_poll_row = None
    for row in route:
        if int(row['route_tick']) < lo:
            continue
        if last_poll_row is None:
            last_poll_row = row
        elif row['port1_polls'] != last_poll_row['port1_polls']:
            poll_gaps.append({'route_ticks': int(row['route_tick']) - int(last_poll_row['route_tick']),
                              'from_poll': int(last_poll_row['port1_polls']),
                              'to_poll': int(row['port1_polls']),
                              'from_region': value(last_poll_row, 'HK_REGION_ID'),
                              'to_region': value(row, 'HK_REGION_ID')})
            last_poll_row = row
    poll_phase = None
    if '__psx_rt_vblank_count' in watches:
        # Rows sample before or after the poll within a VBlank. A one-count
        # lag change is sampling phase, not a missed VBlank. Compare both clocks
        # instead of interpreting a two-row span as a two-VBlank input stall.
        lags = [((value(row, '__psx_rt_vblank_count') - int(row['port1_polls']) + (1 << 31))
                 & 0xffffffff) - (1 << 31) for row in route if int(row['route_tick']) >= lo]
        if lags:
            poll_phase = {'minimum_vblank_minus_polls': min(lags),
                          'maximum_vblank_minus_polls': max(lags),
                          'lag_variation': max(lags) - min(lags),
                          'histogram': dict(sorted(collections.Counter(lags).items()))}
    transitions = []
    previous = None
    for row in route:
        region = value(row, 'HK_REGION_ID')
        if region != previous:
            transitions.append({'region': region, 'route_tick': int(row['route_tick']),
                                'poll': int(row['port1_polls']),
                                'x': value(row, 'HK_PLAYER_X') / 65536,
                                'y': value(row, 'HK_PLAYER_Y') / 65536})
            previous = region
    symbols = []
    for line in map_text.splitlines():
        match = re.match(r'^([a-f0-9]+)\s+[a-f0-9]+\s+([a-f0-9]+)\s+1\s{10,}(\S.*)$', line)
        if match and int(match[2], 16) > 0:
            symbols.append((int(match[1], 16), int(match[2], 16), match[3]))
    def symbol(address):
        pc = int(address, 16)
        return next((name for start, size, name in symbols if start <= pc < start + size), address)
    def aggregate(name, address, metric):
        counts = collections.Counter()
        for row in rows(name):
            counts[symbol(row[address])] += int(row[metric])
        return {'total': counts.total(), 'functions': [{'symbol': name, 'count': n,
                'percent': n * 100 / counts.total()} for name, n in counts.most_common()]}
    ram = (output / 'ram.bin').read_bytes()
    final = {}
    for name, address in watches.items():
        offset = int(address, 16) - 0x80000000
        if not 0 <= offset <= len(ram) - 4:
            raise ValueError(f'{name} lies outside captured RAM')
        n = int.from_bytes(ram[offset:offset + 4], 'little', signed=name in ('HK_PLAYER_X', 'HK_PLAYER_Y'))
        final[name] = n / 65536 if name in ('HK_PLAYER_X', 'HK_PLAYER_Y') else n
    diagnostics = {name: {'final': final[name],
                          'maximum_observed': max([value(row, name) for row in route] + [final[name]])}
                   for name in OPTIONAL_WATCH if name in watches}
    cpu = rows('cpu-cycles.csv')
    cycles = {key: sum(int(row[key]) for row in cpu if int(row['route_tick']) >= lo)
              for key in cpu[0] if key.endswith('cycles') and key != 'bus_cycles'} if cpu else {}
    gpu = rows('gpu.csv')
    gpu_counts = {key: sum(int(row[key]) for row in gpu if int(row['route_tick']) >= lo)
                  for key in ('display_start_changed', 'draws', 'textured_quads', 'gpu_cycles',
                              'dma_gpu_cycles', 'textured_quad_cycles', 'fill_cycles')}
    resident_activity = {}
    if gameplay:
        for name in ('HK_CD_SECTORS_READ','HK_SCENE_LOADS','HK_VRAM_UPLOAD_BYTES','HK_REGION_ACTIVATIONS'):
            if name in watches:
                values = [value(r,name) for r in route if int(r['route_tick']) >= lo] + [final[name]]
                resident_activity[name] = {'first':values[0], 'final':values[-1],
                                           'delta':values[-1]-values[0], 'span':max(values)-min(values)}
    return {'final_ram': final, 'route_ticks': int(route[-1]['route_tick']),
            'gameplay_resident_activity': resident_activity,
            'bus_cycles': int(route[-1]['bus_cycles']), 'transitions': transitions,
            'all_frame_intervals': distribution(intervals),
            'no_cd_activity_frame_intervals': distribution(no_cd),
            'with_cd_activity_frame_intervals': distribution(with_cd),
            'boundary_wait_frame_intervals': distribution(with_boundary_wait) if 'HK_BOUNDARY_WAIT_TICKS' in watches else None,
            'optional_diagnostics': diagnostics,
            'observed_error_maxima': {name: max([value(row, name) for row in route] + [final[name]])
                                      for name in ('__psx_rt_fault_count', 'HK_ROOM_LOAD_ERROR', 'HK_CD_STREAM_ERROR')
                                      if name in watches},
            'poll_vblank_phase': poll_phase,
            'observed_poll_gaps': {'max_route_ticks': max((r['route_ticks'] for r in poll_gaps), default=None),
                                   'over_2_route_ticks': [r for r in poll_gaps if r['route_ticks'] > 2]},
            'slow_frame_threshold_route_ticks': 6,
            'frames_over_6_route_ticks': [r for r in intervals if r['route_ticks'] > 6],
            'frame_intervals': intervals, 'gameplay_cpu_cycles': cycles, 'gameplay_gpu': gpu_counts,
            'retired_pc_samples': aggregate('pc.csv', 'pc', 'samples'),
            'mmio_stalls': aggregate('mmio.csv', 'line_pc', 'mmio_stall_cycles'),
            'ram_load_stalls': aggregate('ram-loads.csv', 'line_pc', 'ram_load_stall_cycles'),
            'limitations': ['CPU samples count retired instructions, not weighted cycles.',
                           'GPU timing is an emulator estimate; this is not hardware performance proof.',
                           'CD-active intervals only indicate reads/load completions; asynchronous streaming does not imply a freeze.',
                           'No-CD intervals may contain other work; all-frame maxima and boundary counters quantify stalls separately.',
                           'Raw poll spans are sampled on route ticks: two ticks may contain two polls; use independent VBlank/poll lag and the global per-poll VBlank metric.',
                           'Identical poll inputs can follow different positions after pacing or region changes.']}


def seamless_failures(result, max_frame_route_ticks=2):
    """Strict gameplay continuity gate; CD activity itself is not a failure.

    Initial menu/loading is excluded by analyze(). Bank loading must preserve
    polling and ordinary rendering, including every observed region activation.
    """
    if max_frame_route_ticks < 1:
        raise ValueError('Frame budget must be positive')
    failures = []
    diagnostics = result.get('optional_diagnostics', {})
    for name in ('HK_INPUT_MISSED_VBLANKS','HK_INPUT_FAULT'):
        if diagnostics.get(name,{}).get('maximum_observed',0):
            failures.append(f'{name} was nonzero during replay')
    for name, maximum in (('HK_BOUNDARY_WAIT_TICKS', 0), ('HK_BOUNDARY_WAIT_MAX', 0),
                          ('HK_PAD_POLL_MAX_VBLANK_GAP', 1), ('HK_CD_STREAM_ERROR', 0),
                          ('HK_CD_DISCARDED_SECTORS', 0)):
        observed = diagnostics.get(name, {}).get('maximum_observed')
        if observed is None:
            failures.append(f'Missing continuity diagnostic {name}')
        elif observed > maximum:
            failures.append(f'{name}={observed} exceeds {maximum}')
    for name in ('__psx_rt_fault_count', 'HK_ROOM_LOAD_ERROR', 'HK_CD_STREAM_ERROR'):
        observed = result.get('observed_error_maxima', {}).get(name)
        if observed is None:
            failures.append(f'Missing observed error diagnostic {name}')
        elif observed:
            failures.append(f'{name} was nonzero during replay: {observed}')
    if result.get('final_ram', {}).get('HK_GAME_MODE') != 1:
        failures.append('Replay did not finish in gameplay')
    frames = result.get('frame_intervals', [])
    if not frames:
        failures.append('No gameplay frame intervals observed')
    elif max(row['route_ticks'] for row in frames) > max_frame_route_ticks:
        failures.append(f'Frame interval exceeded {max_frame_route_ticks} route ticks')
    activation = [row for row in frames if row['from_region'] != row['to_region']]
    ordinary = [row for row in frames if row['from_region'] == row['to_region']]
    if not activation:
        failures.append('No region activation observed; this route cannot establish seamless traversal')
    if activation and ordinary and max(row['route_ticks'] for row in activation) > max(row['route_ticks'] for row in ordinary):
        failures.append('Activation frame exceeded every ordinary frame interval')
    gap = result.get('observed_poll_gaps', {}).get('max_route_ticks')
    if gap is None:
        failures.append('No raw gameplay poll-gap observations')
    elif gap > 2:
        failures.append(f'Raw observed poll gap={gap} route ticks exceeds sampling-phase bound2')
    phase = result.get('poll_vblank_phase')
    if not phase or phase.get('lag_variation') is None:
        failures.append('Missing independent VBlank/poll phase observations')
    elif phase['lag_variation'] > 1:
        failures.append(f'VBlank/poll lag variation={phase["lag_variation"]} exceeds sampling-phase bound1')
    return failures


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('exe', 'map', 'cue', 'tape', 'output'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--frontend', type=Path, default=Path(__file__).resolve().parents[2] / 'PSoXide-emulator/target/release/frontend')
    parser.add_argument('--require-seamless', action='store_true',
                        help='Fail on any boundary wait, missed gameplay poll, or activation/frame budget spike')
    parser.add_argument('--max-frame-route-ticks', type=int, default=2,
                        help='Maximum gameplay frame interval for --require-seamless (default2: 30 fps at 60 Hz)')
    parser.add_argument('--screenshot-interval',type=int,default=600,
                        help='Route ticks between software display captures (default600)')
    parser.add_argument('--pc-window-ticks',type=int,default=120,
                        help='PC attribution window in route ticks; use1 to isolate a burst VBlank')
    args = parser.parse_args()
    if args.max_frame_route_ticks < 1:
        parser.error('--max-frame-route-ticks must be positive')
    if args.screenshot_interval < 1:
        parser.error('--screenshot-interval must be positive')
    if args.pc_window_ticks < 1:
        parser.error('--pc-window-ticks must be positive')
    args.exe, args.map, args.cue, args.tape, args.output, args.frontend = (
        p.resolve() for p in (args.exe, args.map, args.cue, args.tape, args.output, args.frontend))
    tape = args.tape.read_bytes()
    if tape[:8] != b'PXITAPE2' or len(tape) < 16:
        parser.error('This runner requires the recorded PXITAPE2 pad-poll format')
    samples, start_poll = struct.unpack_from('<II', tape, 8)
    if not samples or len(tape) != 16 + samples * 6:
        parser.error('Malformed poll tape length')
    map_text = args.map.read_text()
    watches = {}
    for name in WATCH + OPTIONAL_WATCH:
        match = re.search(r'^([0-9a-f]+)\s+.*\s' + re.escape(name) + r'$', map_text, re.M)
        if not match:
            if name in OPTIONAL_WATCH:
                continue
            parser.error(f'Missing required map symbol {name}')
        watches[name] = f'0x{match[1]}'
    cue_files = re.findall(r'^\s*FILE\s+"([^"]+)"\s+', args.cue.read_text(), re.M | re.I)
    if not cue_files:
        parser.error('No quoted FILE records in CUE')
    inputs = [args.exe, args.map, args.cue, args.tape, args.frontend] + [args.cue.parent / name for name in cue_files]
    hashes = {str(path): {'sha256': digest(path), 'bytes': path.stat().st_size} for path in inputs}
    args.output.mkdir(parents=True, exist_ok=False)
    artifact_dir = args.output / 'artifacts'
    artifact_dir.mkdir()
    for source in (args.exe, args.map):
        saved = source.read_bytes()
        if hashlib.sha256(saved).hexdigest() != hashes[str(source)]['sha256']:
            raise RuntimeError('Input changed before snapshot')
        (artifact_dir / source.name).write_bytes(saved)
    (args.output / 'frontend-launch-help.txt').write_text(subprocess.check_output([str(args.frontend), 'launch', '--help'], text=True))
    cmd = [str(args.frontend), 'launch', '--path', str(artifact_dir / args.exe.name), '--disc', str(args.cue),
           '--embedded-playtest', '--config-dir', str(args.output / 'emulator'), '--steps', '6000000000',
           '--input-tape', str(args.tape), '--stop-at-poll', str(start_poll + samples)]
    outputs = {'route-log': 'route.csv', 'profile-log': 'profile.csv', 'pc-sample-log': 'pc.csv',
               'pc-sample-callsite-log': 'pc-callsite.csv', 'pc-sample-window-log': 'pc-window.csv',
               'cpu-cycle-profile-log': 'cpu-cycles.csv', 'mmio-stall-line-log': 'mmio.csv',
               'ram-load-stall-line-log': 'ram-loads.csv', 'gpu-frame-stats-log': 'gpu.csv',
               'cd-command-log': 'cd.csv', 'dump-display': 'display.ppm', 'dump-ram': 'ram.bin',
               'route-screenshot-dir': 'screenshots'}
    for flag, name in outputs.items():
        cmd += ['--' + flag, str(args.output / name)]
    cmd += ['--pc-sample-window-ticks', str(args.pc_window_ticks), '--pc-sample-instructions', '4096',
            '--route-screenshot-interval', str(args.screenshot_interval), '--dump-guest-profile']
    for address in watches.values():
        cmd += ['--route-watch-u32', address]
    report = {'command': cmd, 'inputs': hashes, 'watches': watches, 'samples': samples,
              'start_poll': start_poll, 'started_unix': time.time()}
    (args.output / 'replay.json').write_text(json.dumps(report, indent=2) + '\n')
    with (args.output / 'replay.log').open('w') as log:
        proc = subprocess.run(cmd, stdout=log, stderr=subprocess.STDOUT)
    stop = re.search(r'route-ticks=(\d+)\s+port1-polls=(\d+)', (args.output / 'replay.log').read_text())
    reached_poll = int(stop[2]) if stop else None
    report.update(returncode=proc.returncode, ended_unix=time.time(), reached_poll=reached_poll,
                  inputs_unchanged=all(digest(Path(path)) == entry['sha256'] for path, entry in hashes.items()))
    (args.output / 'replay.json').write_text(json.dumps(report, indent=2) + '\n')
    if proc.returncode or not report['inputs_unchanged'] or reached_poll is None or reached_poll < start_poll + samples:
        raise RuntimeError('Replay failed, ended early, or an input changed while replaying; see replay.json/log')
    result = analyze(args.output, watches, map_text)
    if args.require_seamless:
        result['seamless_check'] = {'max_frame_route_ticks': args.max_frame_route_ticks,
                                    'failures': seamless_failures(result, args.max_frame_route_ticks)}
        result['seamless_check']['passed'] = not result['seamless_check']['failures']
    (args.output / 'analysis.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({key: result[key] for key in ('final_ram', 'all_frame_intervals', 'no_cd_activity_frame_intervals', 'with_cd_activity_frame_intervals', 'optional_diagnostics')}, indent=2))
    if args.require_seamless and not result['seamless_check']['passed']:
        raise RuntimeError('Seamless traversal gate failed: ' + '; '.join(result['seamless_check']['failures']))
    if result['final_ram']['__psx_rt_fault_count'] or result['final_ram']['HK_ROOM_LOAD_ERROR']:
        raise RuntimeError('Guest fault/load error observed; profile retained for diagnosis')


if __name__ == '__main__':
    main()
