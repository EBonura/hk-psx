#!/usr/bin/env python3
"""Build and run tools/route_sim/main.rs against the shared host rlibs.

usage: route_sim.py X Y SCRIPT REGION_CHUNK...   (chunk ids select data/regions/region-NNN/room.hk)
"""
import json
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def main():
    x, y, script, *chunks = sys.argv[1:]
    output = ROOT/'.hkpsx/world-tests'
    output.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, CARGO_TARGET_DIR=str(output/'cargo'))
    result = subprocess.run(['cargo', 'build', '--locked', '--manifest-path', str(ROOT/'shared/hk-sim/Cargo.toml'),
                             '--message-format=json'], cwd=ROOT, env=env, check=True, capture_output=True, text=True)
    libraries = {}
    for line in result.stdout.splitlines():
        record = json.loads(line)
        if record.get('reason') == 'compiler-artifact' and record['target']['name'] in ('hk_sim', 'hk_format'):
            libraries[record['target']['name']] = next(p for p in record['filenames'] if p.endswith('.rlib'))
    executable = output/'route-sim'
    command = ['rustc', '--edition=2021', '-Awarnings', '-O', str(ROOT/'tools/route_sim/main.rs'), '-o', str(executable)]
    for name, path in sorted(libraries.items()):
        command += ['--extern', f'{name}={path}']
    command += ['-L', f'dependency={Path(libraries["hk_sim"]).parent}']
    subprocess.run(command, cwd=ROOT, env=dict(env, CARGO_MANIFEST_DIR=str(ROOT/'game')), check=True)
    rooms = [str(ROOT/f'data/regions/region-{int(c):03}/room.hk') for c in chunks]
    subprocess.run([str(executable), x, y, script, *rooms], check=True)


if __name__ == '__main__':
    main()
