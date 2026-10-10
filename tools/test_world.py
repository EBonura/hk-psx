#!/usr/bin/env python3
"""Run native world integration tests against the currently generated region tables.

Uses Cargo's artifact messages to locate exact host rlibs rather than assuming
unstable hashed filenames. Requires data/regions.rs from the local asset build;
never recooks assets, writes a disc, or invokes the guest target.
"""
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT/'tools'))

Q = 65536


def world_banks(output):
    """HKWMTA01 banks the tests read through world::TEST_BANK: the cooked scene 0
    bank and a synthetic one whose values match the test constants in raw Q16."""
    from world_metadata import encode_scene
    report = json.loads((ROOT/'data/regions.json').read_text())
    scene0 = next(s for s in report['scenes'] if s['scene_id'] == 0)
    # host/pack_scenes.py hands the encoder the scene's reveal-mask controllers;
    # the tests read that catalogue out of this bank, so supply it the same way.
    controllers = report.get('reveal_mask_scenes', {}).get('0', {}).get('controllers', [])
    scene0 = dict(scene0, reveal_mask_controllers=controllers, reveal_controllers=len(controllers))
    payload, _ = encode_scene(scene0, [r for r in report['regions'] if r['scene_id'] == 0])
    (output/'scene0.hkwm').write_bytes(payload)
    triangle = [[100, 0], [200, 0], [100, 100]]
    square = [[100, 0], [200, 0], [200, 100], [100, 100]]
    def prop(state, polygon, fades):
        return {'source': f'test:{10 + state}', 'state_index': state, 'hit_points': 1,
                'box': [v / Q for v in [100, 0, 200, 100]],
                'hit_polygons': [[[x / Q, y / Q] for x, y in polygon]],
                'off_draws': [1], 'on_draws': [2], 'edge_indices': [1],
                'persistence': [{'dont_save': False, 'semi_persistent': False}],
                'audio': {'options': [{'name': 'breakable_wall_hit_1'}]},
                'mask_fades': [{'ticks_60hz': 60, 'target_alpha': 0, 'ease': 'linear',
                                'renderers': [{'initial_alpha': 1}], 'draw_indices': [5]}] if fades else []}
    def row(chunk, breakables):
        bounds = [v / Q for v in [0, 0, 1000, 1000]]
        return {'chunk_id': chunk, 'scene_id': 0, 'activation_bounds': bounds, 'camera_bounds': bounds,
                'draws': 8, 'edges': 8, 'breakables': breakables}
    payload, _ = encode_scene({'scene_id': 0, 'scene_name': 'Fixture', 'file': 'fixture'},
                              [row(1, [prop(0, triangle, True), prop(1, square, False)]), row(2, [])])
    (output/'fixture.hkwm').write_bytes(payload)
    return {'HK_TEST_SCENE0_BANK': str(output/'scene0.hkwm'), 'HK_TEST_FIXTURE_BANK': str(output/'fixture.hkwm')}


def main():
    if not (ROOT/'data/regions.rs').is_file():
        raise SystemExit('Missing data/regions.rs; run the local asset build before world integration tests')
    output = ROOT/'.hkpsx/world-tests'
    output.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, CARGO_TARGET_DIR=str(output/'cargo'))
    result = subprocess.run(
        ['cargo', 'build', '--locked', '--manifest-path', str(ROOT/'shared/hk-sim/Cargo.toml'),
         '--message-format=json'], cwd=ROOT, env=env, check=True, capture_output=True, text=True)
    libraries = {}
    for line in result.stdout.splitlines():
        record = json.loads(line)
        if record.get('reason') != 'compiler-artifact':
            continue
        name = record['target']['name']
        if name in ('hk_sim', 'hk_format', 'psx_math'):
            candidates = [Path(path) for path in record['filenames'] if path.endswith('.rlib')]
            if len(candidates) != 1:
                raise SystemExit(f'Expected one native rlib artifact for {name}')
            libraries[name] = candidates[0]
    if set(libraries) != {'hk_sim', 'hk_format', 'psx_math'}:
        raise SystemExit('Cargo did not report every native shared-library artifact')
    executable = output/'world-runtime-tests'
    command = ['rustc', '--edition=2021', '-Awarnings', '-Zcrate-attr=feature(optimize_attribute)', '--test', str(ROOT/'tests/world_runtime.rs')]
    for name, path in sorted(libraries.items()):
        command += ['--extern', f'{name}={path}']
    # hk_sim is uplifted to the profile directory while psx_math stays in
    # deps/, so rustc needs both to resolve hk_sim's dependency.
    for directory in sorted({str(path.parent) for path in libraries.values()}):
        command += ['-L', f'dependency={directory}']
    command += ['-o', str(executable)]
    subprocess.run(command, cwd=ROOT, env=dict(env, CARGO_MANIFEST_DIR=str(ROOT/'game'), **world_banks(output)), check=True)
    subprocess.run([str(executable)], cwd=ROOT, check=True)


if __name__ == '__main__':
    main()
