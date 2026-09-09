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

ROOT = Path(__file__).resolve().parents[1]


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
        if name in ('hk_sim', 'hk_format'):
            candidates = [Path(path) for path in record['filenames'] if path.endswith('.rlib')]
            if len(candidates) != 1:
                raise SystemExit(f'Expected one native rlib artifact for {name}')
            libraries[name] = candidates[0]
    if set(libraries) != {'hk_sim', 'hk_format'}:
        raise SystemExit('Cargo did not report both native shared-library artifacts')
    executable = output/'world-runtime-tests'
    command = ['rustc', '--edition=2021', '-Awarnings', '--test', str(ROOT/'tests/world_runtime.rs')]
    for name, path in sorted(libraries.items()):
        command += ['--extern', f'{name}={path}']
    command += ['-L', f'dependency={libraries["hk_sim"].parent}', '-o', str(executable)]
    subprocess.run(command, cwd=ROOT, env=dict(env, CARGO_MANIFEST_DIR=str(ROOT/'game')), check=True)
    subprocess.run([str(executable)], cwd=ROOT, check=True)


if __name__ == '__main__':
    main()
