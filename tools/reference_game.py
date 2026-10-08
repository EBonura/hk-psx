#!/usr/bin/env python3
"""Build and run an isolated, instrumented Windows original in Unity headless mode.

Only the ignored clone is patched. No desktop input, Steam launch, user-save
selection, PS1 build or disc writes. See docs/ORIGINAL_REFERENCE.md.
"""
import argparse
import csv
import hashlib
import json
import math
import os
from pathlib import Path
import shutil
import shlex
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]
WORK = ROOT / '.hkpsx/og-reference'
AUDIO_HEADER = ('sequence,queued_test_frame,unity_frame,time,fixed_time,input_tick,input_updates,'
                'operation,callsite,scene,hierarchy,source_id,clip_or_snapshot,asset_id,pitch,'
                'source_volume,volume_scale,x,y,z,parameter,value,phase').split(',')
AUDIO_OPERATIONS = {'Play', 'PlayOneShot', 'Stop', 'Pause', 'UnPause', 'PlayClipAtPoint', 'TransitionTo', 'SetFloat'}


def digest(path):
    with Path(path).open('rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()


def windows(path):
    return 'Z:' + str(Path(path).resolve()).replace('/', '\\')


def crossover_bin(explicit):
    if explicit:
        result = explicit.resolve()
    else:
        result = None
        ps = subprocess.run(['pgrep', '-fl', 'wineserver'], capture_output=True, text=True)
        for line in ps.stdout.splitlines():
            name = line.partition(' ')[2]
            if '/CrossOver.app/' in name and name.endswith('/bin/wineserver'):
                candidate = Path(name).parent
                if (candidate / 'cxbottle').is_file():
                    result = candidate
                    break
        if result is None:
            candidate = Path('/Applications/CrossOver.app/Contents/SharedSupport/CrossOver/bin')
            if (candidate / 'cxbottle').is_file():
                result = candidate
    if result is None or not (result / 'wine').is_file():
        raise ValueError('Pass --crossover-bin for the installed CrossOver bin directory')
    return result


def inputs(install):
    # Bind the entire installed game, including shared assets and managed
    # dependencies. A retained clone must never mix versions after a Steam update.
    result = {str(path): digest(path) for path in sorted(install.rglob('*')) if path.is_file()}
    drive = next((p for p in install.parents if p.name == 'drive_c'), None)
    if drive:
        for directory in (drive / 'users').glob('*/AppData/LocalLow/Team Cherry/Hollow Knight'):
            for path in directory.glob('user*.dat*'):
                if path.is_file():
                    result[str(path)] = digest(path)
    return result


def prepare(args):
    doctor = json.loads((ROOT / '.hkpsx/doctor.json').read_text())
    install = Path(doctor['installs'][0]['install']).resolve()
    if doctor.get('source_platform') != 'Windows (CrossOver)' or not (install / 'hollow_knight.exe').is_file():
        raise ValueError('Windows installation required; rerun tools/doctor.py')
    original = inputs(install)
    WORK.mkdir(parents=True, exist_ok=True)
    game = WORK / 'game'
    if not game.exists():
        subprocess.run(['cp', '-cR', str(install), str(game)], check=True)
    if game.resolve() != game or game.resolve() == install:
        raise ValueError('The reference game must be an isolated real directory')
    # Verify every cloned retail file except the deliberately replaced assembly.
    for path, expected in original.items():
        source = Path(path)
        if not source.is_relative_to(install):
            continue  # Original save fingerprints are not cloned into the runner.
        relative = source.relative_to(install)
        if str(relative) == 'hollow_knight_Data/Managed/Assembly-CSharp.dll':
            continue
        copied = game / relative
        if not copied.is_file() or copied.is_symlink() or digest(copied) != expected:
            raise ValueError('Reference copy is stale or not isolated: ' + str(relative))
    cx = crossover_bin(args.crossover_bin)
    bottle = WORK / 'bottle'
    if not bottle.exists():
        subprocess.run([str(cx / 'cxbottle'), '--bottle', str(bottle), '--create',
                        '--template', 'win10_64', '--description', 'HK headless reference'], check=True)
    dotnet = args.dotnet or WORK / 'deps/dotnet/dotnet'
    dotnet = dotnet.resolve()
    sdk = subprocess.check_output([str(dotnet), '--list-sdks'], text=True).splitlines()[-1]
    version, directory = sdk.split(' ', 1)
    csc = Path(directory.strip('[]')) / version / 'Roslyn/bincore/csc.dll'
    managed = game / 'hollow_knight_Data/Managed'
    if managed.resolve() != managed:
        raise ValueError('Managed destination must not resolve through a symlink')
    source_dll = install / 'hollow_knight_Data/Managed/Assembly-CSharp.dll'
    if os.path.samefile(source_dll, managed / 'Assembly-CSharp.dll'):
        raise ValueError('Managed destination must not alias the original file')
    driver = WORK / 'HKReference.dll'
    references = ['mscorlib.dll', 'netstandard.dll', 'System.dll', 'System.Core.dll', 'Assembly-CSharp.dll',
                  'UnityEngine.dll', 'UnityEngine.CoreModule.dll', 'UnityEngine.Physics2DModule.dll',
                  'UnityEngine.AudioModule.dll', 'UnityEngine.AnimationModule.dll', 'UnityEngine.ParticleSystemModule.dll',
                  'UnityEngine.InputLegacyModule.dll', 'UnityEngine.UI.dll', 'PlayMaker.dll',
                  'UnityEngine.ImageConversionModule.dll']
    # Compile against the ORIGINAL APIs, never a previously patched assembly.
    command = [str(dotnet), str(csc), '-nologo', '-noconfig', '-nostdlib+',
               '-target:library', '-out:' + str(driver)]
    command += ['-r:' + str(install / 'hollow_knight_Data/Managed' / n) for n in references]
    command += [str(p) for p in sorted((ROOT / 'tools/reference').glob('*.cs'))]
    subprocess.run(command, check=True)
    subprocess.run([str(dotnet), 'build', str(ROOT / 'tools/reference/Patcher/Patcher.csproj'),
                    '-o', str(WORK / 'patcher')], check=True)
    saves = WORK / 'saves'
    saves.mkdir(exist_ok=True)
    patch = subprocess.check_output([str(dotnet), str(WORK / 'patcher/Patcher.dll'),
        str(install / 'hollow_knight_Data/Managed/Assembly-CSharp.dll'), str(managed),
        str(driver), windows(saves)], text=True)
    report = {'install': str(install), 'original_inputs': original, 'inputs_unchanged': inputs(install) == original,
              'game': str(game), 'bottle': str(bottle), 'crossover_bin': str(cx),
              'dotnet': str(dotnet), 'sdk': version, 'patch': json.loads(patch),
              'driver_sha256': digest(driver),
              'driver_sources': {str(p): digest(p) for p in (ROOT / 'tools/reference').glob('*.cs')},
              'prepared_at': time.time()}
    (WORK / 'prepare.json').write_text(json.dumps(report, indent=2) + '\n')
    if not report['inputs_unchanged']:
        raise ValueError('Original inputs changed during preparation')
    print('Prepared isolated reference:', game)



def validate_audio_calls(output, unity_log):
    """Validate recorded requests, not audible output or exhaustive native coverage."""
    errors, operations, sites, previous = [], {}, set(), None
    count = 0
    fixed_clock_context_switches = 0
    if 'HKReference AudioTrace logging failed:' in unity_log:
        errors.append('AudioTrace reported a logging failure')
    try:
        with (output / 'audio-calls.csv').open(newline='') as stream:
            reader = csv.DictReader(stream, strict=True)
            if reader.fieldnames != AUDIO_HEADER:
                raise ValueError('unexpected or missing audio-calls.csv header')
            for row in reader:
                if None in row or any(value is None for value in row.values()):
                    raise ValueError(f'incomplete audio row {count}')
                if int(row['sequence']) != count:
                    raise ValueError(f'audio sequence is not consecutive from zero at row {count}')
                if row['phase'] != 'request' or row['operation'] not in AUDIO_OPERATIONS:
                    raise ValueError(f'unsupported audio operation/phase at row {count}')
                caller, separator, offset = row['callsite'].rpartition('@IL_')
                if not caller or not separator or not offset or any(c not in '0123456789abcdefABCDEF' for c in offset):
                    raise ValueError(f'missing original audio callsite at row {count}')
                if int(row['queued_test_frame']) < -1:
                    raise ValueError(f'invalid queued audio frame at row {count}')
                counters = tuple(int(row[key]) for key in ('unity_frame', 'input_tick', 'input_updates'))
                times = tuple(float(row[key]) for key in ('time', 'fixed_time'))
                if any(value < 0 for value in counters + times) or not all(math.isfinite(value) for value in times):
                    raise ValueError(f'invalid audio clock at row {count}')
                if previous:
                    old_counters, old_times = previous
                    if (any(a < b for a, b in zip(counters, old_counters))
                            or times[1] + 1e-6 < old_times[1]):
                        raise ValueError(f'audio clocks moved backwards at row {count}')
                    if times[0] + 1e-6 < old_times[0]:
                        # Time.time is context-dependent: Unity returns fixedTime
                        # in a physics callback. Scene Start can emit audio before
                        # that frame's physics, so its frame time may be ahead of
                        # the next request's advancing fixed clock. Accept only
                        # this evidenced same-frame switch, never arbitrary drift.
                        # https://docs.unity3d.com/6000.0/Documentation/ScriptReference/Time-time.html
                        fixed_context_switch = (counters[0] == old_counters[0]
                                                and times[0] == times[1]
                                                and times[1] > old_times[1])
                        if not fixed_context_switch:
                            raise ValueError(f'audio clocks moved backwards at row {count}')
                        fixed_clock_context_switches += 1
                for key in ('pitch', 'source_volume', 'volume_scale', 'x', 'y', 'z', 'value'):
                    if row[key] and not math.isfinite(float(row[key])):
                        raise ValueError(f'non-finite audio {key} at row {count}')
                if not row['volume_scale']:
                    raise ValueError(f'missing audio volume scale at row {count}')
                previous = counters, times
                operations[row['operation']] = operations.get(row['operation'], 0) + 1
                sites.add(row['callsite'])
                count += 1
    except (OSError, ValueError, TypeError, csv.Error) as error:
        errors.append('Invalid audio-call evidence: ' + str(error))
    return {'passed': not errors, 'errors': errors, 'requests': count,
            'fixed_clock_context_switches': fixed_clock_context_switches,
            'clock_semantics': 'Time.time may switch to an advancing fixed clock within one Unity frame; request sequence remains authoritative',
            'observed_call_sites': len(sites), 'operations': dict(sorted(operations.items())),
            'coverage': 'Recorded managed requests only; excludes uninstrumented calls and does not prove audible output'}


def validate_run(output, expected_frames, allow_command_stop=False, scene='Tutorial_01', graphics=False):
    try:
        return _validate_run(output, expected_frames, allow_command_stop, scene, graphics)
    except (OSError, ValueError, TypeError, KeyError, csv.Error) as error:
        return {'passed': False, 'errors': ['Incomplete or malformed reference log: ' + str(error)]}


def _validate_run(output, expected_frames, allow_command_stop=False, scene='Tutorial_01', graphics=False):
    """A clean process exit alone is not evidence that gameplay ran."""
    errors = []
    path = output / 'state.csv'
    rows = []
    if path.exists():
        with path.open() as f:
            rows = list(csv.DictReader(f))
    played = [r for r in rows if int(r.get('test_frame', '-1')) >= 0]
    indices = [int(r['test_frame']) for r in played]
    driver = (output / 'driver.log').read_text() if (output / 'driver.log').exists() else ''
    command_stop = allow_command_stop and 'STOP command code=0' in driver
    if command_stop:
        expected_frames = len(played)
    if indices != list(range(expected_frames)):
        errors.append(f'Expected {expected_frames} unique consecutive input frames; got {len(indices)}')
    if not played or not any(r['scene'] == scene for r in played):
        errors.append(f'No controlled {scene} gameplay observed')
    if any(r['input_attached'] != 'True' for r in played):
        errors.append('Virtual controller detached during test')
    if ('STOP completed input frames code=0' not in driver and not command_stop) or 'ERROR ' in driver:
        errors.append('Driver did not report successful completion')
    unity = (output / 'unity.log').read_text(errors='replace') if (output / 'unity.log').exists() else ''
    audio = validate_audio_calls(output, unity)
    errors.extend(audio['errors'])
    if 'NullGfxDevice' not in unity and not graphics:
        errors.append('Null graphics device was not confirmed')
    # Runtime exceptions invalidate reference evidence. Shader/render-texture
    # warnings are expected in NullGfx and remain available in the raw log.
    gameplay_log, _, shutdown_log = unity.partition('HK_REFERENCE_STOP code=0 reason=')
    exceptions = [line for line in gameplay_log.splitlines() if 'Exception:' in line]
    shutdown_exceptions = [line for line in shutdown_log.splitlines() if 'Exception:' in line]
    if exceptions:
        errors.append('Original runtime logged exceptions: ' + '; '.join(exceptions[:5]))
    input_events = []
    event_path = output / 'input-events.csv'
    if event_path.exists():
        with event_path.open() as f:
            input_events = [r for r in csv.DictReader(f) if int(r['test_frame']) >= 0]
    if not input_events:
        errors.append('No original input-device events recorded')
    elif not set(indices).issubset({int(r['test_frame']) for r in input_events}):
        errors.append('Gameplay samples lack corresponding input-device events')
    tape_path = output / 'input.csv'
    if tape_path.exists():
        with tape_path.open() as f:
            events = [(int(r['test_frame']), int(r['buttons'], 16 if r['buttons'].lower().startswith('0x') else 10)) for r in csv.DictReader(
                line for line in f if line.strip() and not line.lstrip().startswith('#'))]
        mismatches = []
        for row in played + input_events:
            frame = int(row['test_frame'])
            mask = 0
            for event_frame, event_mask in events:
                if event_frame > frame:
                    break
                mask = event_mask
            if int(row['buttons']) != mask:
                mismatches.append(frame)
        if mismatches:
            errors.append(f'Consumed controller differs from tape at {len(mismatches)} frames: {mismatches[:8]}')
    return {'passed': not errors, 'errors': errors, 'gameplay_frames': len(played), 'input_device_updates': len(input_events), 'command_stop': command_stop,
            'scenes': sorted({r['scene'] for r in played}),
            'x_range': [min(float(r['x']) for r in played), max(float(r['x']) for r in played)] if played else None,
            'y_range': [min(float(r['y']) for r in played), max(float(r['y']) for r in played)] if played else None,
            'shutdown_exceptions': shutdown_exceptions,
            'audio_calls': audio,
            'rendering': 'graphics device, frames captured' if graphics else 'NullGfx; no image/audio-output parity claim'}


def run(args):
    if args.frames <= 0 or args.timeout <= 0:
        raise ValueError('Frames and timeout must be positive')
    report = json.loads((WORK / 'prepare.json').read_text())
    original = report['original_inputs']
    if inputs(Path(report['install'])) != original:
        raise ValueError('Original inputs changed; prepare and bind the new version first')
    if any(digest(p) != value for p, value in report['driver_sources'].items()):
        raise ValueError('Reference driver sources changed; run prepare again')
    managed = Path(report['game']) / 'hollow_knight_Data/Managed'
    if digest(managed / 'HKReference.dll') != report['driver_sha256'] or digest(managed / 'Assembly-CSharp.dll') != report['patch']['patched_sha256']:
        raise ValueError('Prepared managed binaries changed; run prepare again')
    output = (WORK / 'runs' / args.name).resolve()
    if output.parent != (WORK / 'runs').resolve():
        raise ValueError('Run name must be one directory name')
    output.mkdir(parents=True, exist_ok=False)
    if args.tape:
        shutil.copyfile(args.tape, output / 'input.csv')
    # Silent on the host, always: wine's CoreAudio driver is disabled for the
    # game process (mmdevapi then has no endpoint, so Unity's FMOD falls back
    # to its no-sound output), and the driver also sets AudioListener.volume
    # to 0. Audio logic and the managed audio-call trace are unaffected.
    extra = ['HK_REFERENCE_OUTPUT=' + windows(output), 'HK_REFERENCE_MAX_FRAMES=' + str(args.frames),
             'HK_REFERENCE_MAX_SECONDS=' + str(max(1, args.timeout - 5)),
             'WINEDLLOVERRIDES=winecoreaudio.drv=d', 'HK_REFERENCE_MUTE=1']
    if args.scene: extra.append('HK_REFERENCE_SCENE=' + args.scene)
    if args.teleport: extra.append('HK_REFERENCE_TELEPORT=' + args.teleport)
    if args.pd: extra.append('HK_REFERENCE_PD=' + args.pd)
    if args.shot_every: extra.append('HK_REFERENCE_SHOT_EVERY=' + str(args.shot_every))
    if args.shot_size: extra.append('HK_REFERENCE_SHOT_SIZE=' + args.shot_size)
    if args.face: extra.append('HK_REFERENCE_FACE=' + str(args.face))
    if args.fx_frames: extra.append('HK_REFERENCE_FX_FRAMES=' + args.fx_frames)
    if args.hide: extra.append('HK_REFERENCE_HIDE=' + args.hide)
    if args.census_frames: extra.append('HK_REFERENCE_CENSUS_FRAMES=' + args.census_frames)
    env = dict(os.environ, **dict(e.split('=', 1) for e in extra))
    # --graphics keeps Unity's renderer (frame capture); --window also drops
    # batch mode, which opens a game window.
    mode = [] if args.window else ['-batchmode']
    if not (args.graphics or args.window): mode.append('-nographics')
    else: mode += ['-screen-width', '640', '-screen-height', '360', '-screen-fullscreen', '0']
    command = [str(Path(report['crossover_bin']) / 'wine'), '--bottle', report['bottle'],
               '--no-gui', '--workdir', windows(Path(report['game'])), '--env', shlex.join(extra), '--cx-app', windows(Path(report['game']) / 'hollow_knight.exe'),
               *mode, '-logFile', windows(output / 'unity.log')]
    result = {'command': command, 'prepare': report, 'requested_frames': args.frames}
    with (output / 'launcher.log').open('w') as log:
        process = subprocess.Popen(command, env=env, stdout=log, stderr=subprocess.STDOUT)
        if args.pid_file:
            with open(args.pid_file, 'a') as pids:
                pids.write(f'{process.pid} reference_game run {args.name}\n')
        try:
            result['exit_code'] = process.wait(timeout=args.timeout)
        except subprocess.TimeoutExpired:
            result['timed_out'] = True
            # This server belongs exclusively to the isolated test bottle.
            subprocess.run([str(Path(report['crossover_bin']) / 'wineserver'), '-k'],
                env=dict(env, CX_BOTTLE=report['bottle'], WINEPREFIX=report['bottle']),
                stdout=log, stderr=subprocess.STDOUT, timeout=15)
            process.kill()
            result['exit_code'] = process.wait()
    result['validation'] = validate_run(output, args.frames, allow_command_stop=not args.tape,
                                        scene=args.scene.split(':')[0] if args.scene else 'Tutorial_01',
                                        graphics=bool(args.graphics or args.window))
    result['inputs_unchanged'] = inputs(Path(report['install'])) == original
    result['files'] = {p.name: {'bytes': p.stat().st_size, 'sha256': digest(p)}
                       for p in output.iterdir() if p.is_file()}
    (output / 'run.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({'output': str(output), 'exit_code': result['exit_code'],
                      'timed_out': result.get('timed_out', False),
                      'inputs_unchanged': result['inputs_unchanged'],
                      'validation': result['validation']}, indent=2))
    if result.get('timed_out') or result['exit_code'] or not result['inputs_unchanged'] or not result['validation']['passed']:
        raise SystemExit(1)



def command(args):
    output = (WORK / 'runs' / args.name).resolve()
    if output.parent != (WORK / 'runs').resolve() or not output.is_dir():
        raise ValueError('Unknown run name')
    if (output / 'run.json').exists():
        raise ValueError('This run has already finished')
    words = args.text.split()
    if len(words) == 2 and words[0] == 'buttons':
        mask = int(words[1], 0)
        if mask < 0 or mask & ~0xE0F8:
            raise ValueError('Unsupported PS1 button bits')
        if (output / 'input.csv').exists():
            raise ValueError('Live buttons cannot override a loaded tape')
    elif len(words) == 3 and words[0] == 'teleport':
        if not all(math.isfinite(float(x)) for x in words[1:]):
            raise ValueError('Teleport coordinates must be finite')
    elif len(words) == 3 and words[0] == 'scene':
        if not all(word.replace('_', '').isalnum() for word in words[1:]):
            raise ValueError('Expected original scene and gate names')
    elif words not in (['quit'], ['particle-probe']):
        raise ValueError('Expected buttons MASK, teleport X Y, scene ROOM GATE, particle-probe, or quit')
    target = output / 'command.txt'
    temporary = output / 'command.pending'
    temporary.write_text(str(time.time_ns()) + ' ' + ' '.join(words) + '\n')
    temporary.replace(target)
    print('Sent:', ' '.join(words))


def compare(args):
    if not math.isfinite(args.tolerance) or args.tolerance < 0:
        raise ValueError('Tolerance must be finite and nonnegative')
    traces = []
    starts = []
    for name in (args.left, args.right):
        directory = (WORK / 'runs' / name).resolve()
        if directory.parent != (WORK / 'runs').resolve():
            raise ValueError('Run name must be one directory name')
        run_report = json.loads((directory / 'run.json').read_text())
        if not run_report.get('validation', {}).get('passed'):
            raise ValueError('Cannot compare an unvalidated run: ' + name)
        with (directory / 'state.csv').open() as f:
            trace = [r for r in csv.DictReader(f) if int(r['test_frame']) >= 0]
        if not trace:
            raise ValueError('Empty gameplay trace: ' + name)
        traces.append(trace)
        starts.append(int(trace[0]['unity_frame']))
    left, right = traces
    exact = ['test_frame', 'buttons', 'health', 'scene', 'game_state', 'transitioning',
             'on_ground', 'accepting_input', 'input_attached']
    numeric = ['x', 'y', 'vx', 'vy']
    mismatches = sum(any(a[k] != b[k] for k in exact) for a, b in zip(left, right))
    errors = {k: max(abs(float(a[k]) - float(b[k])) for a, b in zip(left, right)) for k in numeric}
    physics_mismatches = sum(int(a['physics_steps']) - int(left[0]['physics_steps']) !=
                             int(b['physics_steps']) - int(right[0]['physics_steps']) for a, b in zip(left, right))
    report = {'runs': [args.left, args.right], 'frames': [len(left), len(right)],
              'start_unity_frames': starts, 'exact_fields': exact, 'exact_mismatched_samples': mismatches,
              'max_numeric_error': errors, 'numeric_tolerance': args.tolerance,
              'relative_physics_step_mismatches': physics_mismatches,
              'passed': len(left) == len(right) and mismatches == 0 and physics_mismatches == 0
                        and all(error <= args.tolerance for error in errors.values())}
    directory = WORK / 'comparisons'
    directory.mkdir(exist_ok=True)
    (directory / (args.left + '--' + args.right + '.json')).write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))
    if not report['passed']:
        raise SystemExit(1)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='mode', required=True)
    p = sub.add_parser('prepare')
    p.add_argument('--dotnet', type=Path)
    p.add_argument('--crossover-bin', type=Path)
    p = sub.add_parser('run')
    p.add_argument('--scene', help='SCENE:GATE start through an original entry gate (test setup)')
    p.add_argument('--teleport', help='X,Y after arrival (test setup)')
    p.add_argument('--pd', help='PlayerData field=value;field=value applied before the scene load')
    p.add_argument('--shot-every', type=int, default=0, help='write a PNG every N test frames (needs --graphics)')
    p.add_argument('--shot-size', help='WxH of captured frames, default 640x360')
    p.add_argument('--face', type=int, default=0, help='with --teleport: 1 face right, -1 face left')
    p.add_argument('--fx-frames', help='test frames that also write per-effect variant captures (needs --graphics)')
    p.add_argument('--hide', help='with --teleport: comma-separated GameObject names to deactivate (test setup)')
    p.add_argument('--census-frames', help='test frames that also write census-fNNNNN.csv, every renderer and the cameras')
    p.add_argument('--graphics', action='store_true', help='keep the renderer (batch mode, no -nographics)')
    p.add_argument('--window', action='store_true', help='run windowed, not in batch mode')
    p.add_argument('--pid-file', help='append the launcher PID here')
    p.add_argument('--name', required=True)
    p.add_argument('--frames', type=int, default=600)
    p.add_argument('--timeout', type=int, default=90)
    p.add_argument('--tape', type=Path)
    p = sub.add_parser('command', help='Atomically send a command to a running headless test')
    p.add_argument('--name', required=True)
    p.add_argument('text', help='buttons MASK, teleport X Y, scene ROOM GATE (explicit setup), or quit')
    p = sub.add_parser('compare', help='Check repeated original-game player traces')
    p.add_argument('--left', required=True)
    p.add_argument('--right', required=True)
    p.add_argument('--tolerance', type=float, default=0.0001)
    args = parser.parse_args()
    {'prepare': prepare, 'run': run, 'command': command, 'compare': compare}[args.mode](args)


if __name__ == '__main__':
    main()
