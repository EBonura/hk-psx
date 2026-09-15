"""Import every Windows BuildSettings scene into resumable host-side source records.

This format preserves source geometry and components independently of gameplay
admission. It is not a PS1 pack; texture cooking and runtime budgets remain separate.
All outputs stay below .hkpsx; this tool never builds or replaces a disc.
"""
import argparse
import base64
import math
import gzip
import hashlib
import json
import multiprocessing
import os
from pathlib import Path
import signal
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
FORMAT = 'HKWORLDIMPORT01'


def sha(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def atomic_json(path, value):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    temp = path.with_suffix(path.suffix + '.tmp')
    temp.write_text(json.dumps(value, separators=(',', ':'), allow_nan=False) + '\n')
    temp.replace(path)


def source_json(value):
    """Preserve binary and nonfinite serialized values without invalid JSON."""
    if isinstance(value, bytes):
        return {'$source_bytes_base64': base64.b64encode(value).decode('ascii')}
    if isinstance(value, float) and not math.isfinite(value):
        return {'$source_float': repr(value)}
    if isinstance(value, dict):
        return {key: source_json(item) for key, item in value.items()}
    if isinstance(value, (list, tuple)):
        return [source_json(item) for item in value]
    return value


def write_gzip(path, value):
    data = json.dumps(source_json(value), separators=(',', ':'), allow_nan=False).encode()
    temp = path.with_suffix(path.suffix + '.tmp')
    temp.write_bytes(gzip.compress(data, mtime=0))
    temp.replace(path)
    return {'path': path.name, 'sha256': sha(path), 'bytes': path.stat().st_size,
            'uncompressed_bytes': len(data)}


def catalog(source):
    build = next(o for o in source.file('globalgamemanagers').objects.values()
                 if o.type.name == 'BuildSettings')
    return [{'index': i, 'file': f'level{i}', 'path': path,
             'scene_name': Path(path.replace('\\', '/')).stem}
            for i, path in enumerate(source.read(build)['scenes'])]


def verify_inputs(directory, recorded):
    failures = []
    for name, expected in recorded.items():
        path = (directory / name).resolve()
        if not path.is_relative_to(directory.resolve()):
            failures.append(name)
        elif not path.is_file() or path.stat().st_size != expected['bytes'] or sha(path) != expected['sha256']:
            failures.append(name)
    if failures:
        raise ValueError(f'Source fingerprint changed or missing: {failures[:8]}')


def read_checkpoint(path, fingerprint, allow_failed=False, attempt_id=None):
    try:
        row = json.loads(path.read_text())
        if row['fingerprint'] != fingerprint or (row['status'] == 'failed' and not allow_failed):
            return None
        if attempt_id is not None and row.get('worker_attempt') != attempt_id:
            return None
        for item in row['outputs'].values():
            output = (path.parent / item['path']).resolve()
            if not output.is_relative_to(path.parent.resolve()):
                return None
            if output.stat().st_size != item['bytes'] or sha(output) != item['sha256']:
                return None
        return row
    except (OSError, ValueError, KeyError, TypeError):
        return None


def valid_checkpoint(path, fingerprint):
    return read_checkpoint(path, fingerprint)


def worker_outcome(exit_code, checkpoint, stopped=False, timed_out=False):
    """Classify one isolated worker without attributing pool loss to other scenes."""
    if stopped:
        return 'interrupted'
    if timed_out:
        return 'timeout'
    if exit_code is None:
        return 'running'
    if exit_code < 0:
        return f'signal:{-exit_code}'
    if exit_code > 0:
        return f'exit:{exit_code}'
    if checkpoint is None:
        return 'missing_result'
    return 'completed'


def summarize(rows, total):
    counts = {}
    systems = {}
    for row in rows:
        for key, value in row.get('counts', {}).items():
            if type(value) is int:
                counts[key] = counts.get(key, 0) + value
        for name, amount in row.get('component_types', {}).items():
            entry = systems.setdefault(name, {'instances': 0, 'scene_indices': [],
                                              'gameplay_support': 'not_evaluated'})
            entry['instances'] += amount
            entry['scene_indices'].append(row['index'])
    return {'catalog_scenes': total, 'processed_scenes': len(rows),
            'imported_scenes': sum(r['status'] == 'imported' for r in rows),
            'partial_scenes': sum(r['status'] == 'partial' for r in rows),
            'failed_scenes': sum(r['status'] == 'failed' for r in rows),
            'all_scenes_processed': len(rows) == total,
            'all_geometry_resolved': len(rows) == total and all(r['status'] == 'imported' for r in rows),
            'ps1_packing': 'not_run', 'gameplay_validation': 'not_run',
            'counts': counts, 'systems': dict(sorted(systems.items()))}


def import_one(directory, info, output, fingerprint, attempt_id=None):
    # A fresh process per scene releases Unity buffers and Scene's bound-method
    # caches. No global source cache grows with the number of imported scenes.
    sys.dont_write_bytecode = True
    from source import Source
    from scene import Scene
    from world_geometry import extract_scene
    folder = Path(output) / info['file']
    folder.mkdir(parents=True, exist_ok=True)
    started = time.monotonic()
    row = dict(info, fingerprint=fingerprint, status='failed', outputs={},
               worker_attempt=attempt_id, worker_pid=os.getpid())
    try:
        source = Source(directory)
        scene = Scene(source, info['file'])
        geometry = extract_scene(source, scene, info)
        row['outputs']['geometry'] = write_gzip(folder/'geometry.json.gz', geometry)
        # Keep original payloads and stable component IDs for later system passes;
        # unsupported scripts must not prevent importing surrounding scenery.
        objects = [{'source': f"{info['file']}:{ident}", 'type': kind, 'data': tree}
                   for ident, (kind, tree) in scene.objects.items()]
        row['outputs']['components'] = write_gzip(folder/'components.json.gz',
            {'format': 'HKWORLDCOMP01', 'scene': info, 'objects': objects, 'errors': scene.errors,
             'omitted_native_types': ['Mesh', 'ParticleSystem', 'ParticleSystemRenderer']})
        from collections import Counter
        row['component_types'] = dict(Counter(kind for kind, _ in scene.objects.values()))
        row['counts'] = geometry.get('counts', {})
        row['errors'] = geometry.get('errors', [])
        row['unsupported'] = geometry.get('unsupported', [])
        row['status'] = 'partial' if row['errors'] or row['unsupported'] else 'imported'
        row['source_files'] = sorted(source.files)
        row['graph'] = {'transitions': [
            {'source_id':gate['source'], 'gate_name':gate['name'],
             'target_scene':gate['target_scene'], 'entry_point':gate['entry_point'],
             'entry_offset':gate['entry_offset'], 'world_position':gate['position'],
             'component_enabled':gate['enabled'], 'active_in_hierarchy':gate['active_hierarchy']}
            for gate in geometry['gates']],
            'errors':[e for e in geometry['errors'] if e.get('type') == 'TransitionPoint'],
            'playmaker_fsm_count':row['component_types'].get('PlayMakerFSM',0)}
    except Exception as error:
        import traceback
        row['failure'] = {'error': str(error), 'traceback': traceback.format_exc()}
    row['seconds'] = round(time.monotonic()-started, 3)
    atomic_json(folder/'result.json', row)
    return row


def failed_worker_row(info, fingerprint, attempt_id, outcome, exit_code):
    failure = {'stage': 'isolated_worker', 'outcome': outcome, 'exit_code': exit_code}
    if exit_code is not None and exit_code < 0:
        failure['signal'] = -exit_code
    return dict(info, fingerprint=fingerprint, status='failed', outputs={},
                worker_attempt=attempt_id, failure=failure, seconds=0)


def stop_process(process, grace_seconds=5):
    """Stop one owned worker and do not leave an unobserved child behind."""
    if process.is_alive():
        process.terminate()
        process.join(timeout=grace_seconds)
    if process.is_alive():
        process.kill()
        process.join(timeout=grace_seconds)
    return process.exitcode


def next_attempt_number(folder):
    attempts = []
    for path in Path(folder).glob('attempt-*.json'):
        try:
            attempts.append(int(path.stem.split('-')[-1]))
        except ValueError:
            continue
    return max(attempts, default=0) + 1


def can_retry(run_try, worker_retries):
    """Retry budget is per run; persistent diagnostic numbering is unrelated."""
    return run_try <= worker_retries


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=ROOT/'.hkpsx/world-import')
    parser.add_argument('--jobs', type=int, default=2, choices=range(1, 5))
    parser.add_argument('--limit', type=int, help='Diagnostic subset, never whole-world completion')
    parser.add_argument('--resume', action='store_true')
    parser.add_argument('--scene-timeout', type=float, default=180.0,
                        help='Maximum seconds for one isolated scene worker')
    parser.add_argument('--worker-retries', type=int, default=1,
                        help='Retries after an abnormal worker exit or timeout')
    args = parser.parse_args()
    output = args.output.resolve()
    if not output.is_relative_to((ROOT/'.hkpsx').resolve()):
        parser.error('Output must be inside the ignored .hkpsx directory')
    if args.limit is not None and args.limit < 1:
        parser.error('--limit must be positive')
    if not math.isfinite(args.scene_timeout) or args.scene_timeout <= 0:
        parser.error('--scene-timeout must be positive and finite')
    if not 0 <= args.worker_retries <= 8:
        parser.error('--worker-retries must be between zero and eight')
    from source import Source
    source = Source()
    directory = source.directory
    scenes = catalog(source)
    # Full source hashes bind external assets even if a reader loads them through
    # UnityPy rather than Source.file. Stat-only reuse is intentionally avoided.
    print('Hashing Windows source inputs...', flush=True)
    inputs = {}
    for path in sorted(directory.rglob('*')):
        if path.is_file():
            inputs[str(path.relative_to(directory))] = {'bytes': path.stat().st_size, 'sha256': sha(path)}
    code = {str(p.relative_to(ROOT)): sha(p) for p in sorted((ROOT/'host').glob('*.py'))}
    code['host/requirements.lock'] = sha(ROOT/'host/requirements.lock')
    identity = {'format': FORMAT, 'source': str(directory), 'inputs': inputs, 'code': code}
    fingerprint = hashlib.sha256(json.dumps(identity, sort_keys=True).encode()).hexdigest()
    inventory_path = ROOT/'.hkpsx/room-inventory.json'
    dependency_report = None
    fsm_actions = {}
    if inventory_path.is_file():
        inventory = json.loads(inventory_path.read_text())
        # Reuse dependency metadata only when every recorded source byte matches.
        compatible = (inventory.get('complete') and inventory.get('source_directory') == str(directory)
                      and inventory.get('source_hashes') and all(inputs.get(name) == expected
                          for name, expected in inventory['source_hashes'].items())
                      and [(r['index'], r['file'], r['path']) for r in inventory['rooms']]
                          == [(r['index'], r['file'], r['path']) for r in scenes])
        if compatible:
            for room in inventory['rooms']:
                seen = set()
                for fsm in room.get('playmaker', {}).get('parsed', []):
                    for name, count in fsm.get('actions', {}).items():
                        entry = fsm_actions.setdefault(name, {'instances':0, 'scene_indices':[]})
                        entry['instances'] += count
                        seen.add(name)
                for name in seen:
                    fsm_actions[name]['scene_indices'].append(room['index'])
            dependency_report = {'path':str(inventory_path), 'sha256':sha(inventory_path),
                'source_fingerprint':inventory['fingerprint'],
                'source_hashes_verified':True,
                'coverage':'Existing serialized dependencies; dynamic prefab closure unresolved'}
        else:
            print('Existing dependency inventory is stale/incomplete; not binding it.', flush=True)
    output.mkdir(parents=True, exist_ok=True)
    atomic_json(output/'provenance.json', dict(identity, fingerprint=fingerprint))
    atomic_json(output/'catalog.json', scenes)
    selected = scenes if args.limit is None else scenes[:args.limit]
    rows = []
    pending = []
    for info in selected:
        saved = valid_checkpoint(output/info['file']/'result.json', fingerprint) if args.resume else None
        if saved:
            rows.append(saved)
        else:
            pending.append(info)
    started = time.monotonic()
    run = {'status': 'running', 'started_unix': time.time(), 'jobs': args.jobs,
           'scene_timeout': args.scene_timeout, 'worker_retries': args.worker_retries,
           'attempts_started': 0, 'attempts_completed': 0, 'retries': 0,
           'timeouts': 0, 'abnormal_exits': 0}
    def checkpoint():
        report = {'format': FORMAT, 'fingerprint': fingerprint,
                  'dependency_inventory': dependency_report, 'serialized_fsm_actions': fsm_actions,
                  'run': run,
                  'coverage': summarize(rows, len(scenes)), 'scenes': sorted(rows, key=lambda r:r['index']),
                  'limitations': [
                      'Host source records, not PS1-ready packed rooms or a playable world.',
                      'All BuildSettings scenes are included, including menus, cinematics and variants; no name-based exclusions.',
                      'Serialized active/enabled states do not resolve progression-dependent activation.',
                      'Source geometry imports do not establish full material, animation or collision-solver parity.',
                      'Textures, audio, particles and dynamically spawned prefab closure require further cooking/system passes.',
                  ]}
        atomic_json(output/'report.json', report)
        return report
    checkpoint()
    del source
    stop_requested = False

    def request_stop(signum, _frame):
        nonlocal stop_requested
        stop_requested = True
        run['stop_signal'] = signum

    previous_handlers = {signum: signal.getsignal(signum) for signum in (signal.SIGINT, signal.SIGTERM)}
    for signum in previous_handlers:
        signal.signal(signum, request_stop)
    context = multiprocessing.get_context('spawn')
    queue = list(pending)
    active = {}
    attempt_counts = {}
    run_try_counts = {}
    try:
        while queue or active:
            while queue and len(active) < args.jobs and not stop_requested:
                info = queue.pop(0)
                attempt = attempt_counts.get(info['index'])
                if attempt is None:
                    attempt = next_attempt_number(output/info['file'])
                else:
                    attempt += 1
                attempt_counts[info['index']] = attempt
                run_try = run_try_counts.get(info['index'], 0) + 1
                run_try_counts[info['index']] = run_try
                attempt_id = f"{fingerprint[:12]}-{info['file']}-{attempt}"
                process = context.Process(target=import_one,
                    args=(str(directory), info, str(output), fingerprint, attempt_id),
                    name=f"world-import-{info['file']}")
                process.start()
                active[process.pid] = {'process': process, 'info': info, 'attempt': attempt,
                    'run_try': run_try, 'attempt_id': attempt_id,
                    'started': time.monotonic(), 'started_unix': time.time()}
                run['attempts_started'] += 1
                atomic_json(output/info['file']/f'attempt-{attempt}.json', {
                    'scene': info, 'attempt': attempt, 'attempt_id': attempt_id,
                    'run_try': run_try,
                    'pid': process.pid, 'started_unix': active[process.pid]['started_unix'],
                    'status': 'running'})
            if stop_requested:
                unfinished = len(queue) + len(active)
                for worker in active.values():
                    stop_process(worker['process'])
                for worker in active.values():
                    atomic_json(output/worker['info']['file']/f"attempt-{worker['attempt']}.json", {
                        'scene': worker['info'], 'attempt': worker['attempt'],
                        'run_try': worker['run_try'],
                        'attempt_id': worker['attempt_id'], 'pid': worker['process'].pid,
                        'started_unix': worker['started_unix'], 'ended_unix': time.time(),
                        'status': 'interrupted', 'exit_code': worker['process'].exitcode})
                active.clear()
                run['status'] = 'interrupted'
                run['pending_scenes'] = unfinished
                run['completed_scenes'] = len(rows)
                checkpoint()
                break
            now = time.monotonic()
            finished = []
            for pid, worker in active.items():
                process = worker['process']
                timed_out = process.is_alive() and now - worker['started'] > args.scene_timeout
                if timed_out:
                    stop_process(process)
                elif process.is_alive():
                    continue
                else:
                    process.join()
                result_path = output/worker['info']['file']/'result.json'
                row = read_checkpoint(result_path, fingerprint, allow_failed=True,
                                      attempt_id=worker['attempt_id']) if process.exitcode == 0 else None
                outcome = worker_outcome(process.exitcode, row, timed_out=timed_out)
                attempt_record = {'scene': worker['info'], 'attempt': worker['attempt'],
                    'run_try': worker['run_try'],
                    'attempt_id': worker['attempt_id'], 'pid': pid,
                    'started_unix': worker['started_unix'], 'ended_unix': time.time(),
                    'status': outcome, 'exit_code': process.exitcode,
                    'seconds': round(now-worker['started'], 3)}
                atomic_json(output/worker['info']['file']/f"attempt-{worker['attempt']}.json",
                            attempt_record)
                run['attempts_completed'] += 1
                if outcome == 'completed':
                    rows.append(row)
                    print(f"{len(rows)}/{len(scenes)} {row['scene_name']}: {row['status']} ({row['seconds']}s)", flush=True)
                else:
                    run['timeouts'] += int(timed_out)
                    run['abnormal_exits'] += int(not timed_out)
                    if can_retry(worker['run_try'], args.worker_retries):
                        run['retries'] += 1
                        queue.insert(0, worker['info'])
                        print(f"RETRY {worker['info']['scene_name']}: {outcome}", flush=True)
                    else:
                        row = failed_worker_row(worker['info'], fingerprint, worker['attempt_id'],
                                                outcome, process.exitcode)
                        atomic_json(result_path, row)
                        rows.append(row)
                        print(f"{len(rows)}/{len(scenes)} {row['scene_name']}: failed ({outcome})", flush=True)
                finished.append(pid)
            for pid in finished:
                active.pop(pid)
            if finished:
                checkpoint()
            elif active:
                time.sleep(0.05)
        if run['status'] == 'running':
            run['status'] = 'processed'
    finally:
        for signum, handler in previous_handlers.items():
            signal.signal(signum, handler)
        for worker in active.values():
            stop_process(worker['process'])
    if stop_requested:
        print(json.dumps({'run_status':'interrupted', 'completed_scenes':len(rows),
                          'pending_scenes':run['pending_scenes']}), flush=True)
        return 130
    verify_inputs(directory, inputs)
    if any(sha(ROOT/name) != digest for name,digest in code.items()):
        raise RuntimeError('Importer source code changed during run; rerun before trusting outputs')
    if dependency_report and sha(inventory_path) != dependency_report['sha256']:
        raise RuntimeError('Dependency inventory changed during run')
    report = checkpoint()
    from room_graph import finalize
    graph = finalize(rows)
    atomic_json(output/'room-graph.json', graph)
    report['room_graph'] = {'path':'room-graph.json', 'sha256':sha(output/'room-graph.json'),
        'edges':graph['edge_count'], 'mapped_gate_edges':graph['mapped_gate_edges'],
        'mapped_scene_edges':graph['mapped_scene_edges']}
    report['inputs_unchanged'] = True
    report['elapsed_seconds'] = round(time.monotonic()-started, 3)
    run['status'] = 'verified'
    run['completed_scenes'] = len(rows)
    run['pending_scenes'] = 0
    run['ended_unix'] = time.time()
    atomic_json(output/'report.json', report)
    print(json.dumps({k:v for k,v in report['coverage'].items() if k not in ('systems', 'counts')}), flush=True)
    return 1 if report['coverage']['failed_scenes'] else 0


if __name__ == '__main__':
    raise SystemExit(main())
