#!/usr/bin/env python3
"""Prove cooked particle tracks against the scalar native runtime.

Run explicitly after the local asset cook (not part of the lightweight test suite):
    .venv/bin/python tools/test_particle_tracks.py
Requires the Windows BYOA break-effects report and host extraction dependencies.
Only writes ignored native proof artifacts; never recooks rooms, builds the guest,
or reads/writes a disc. Cargo artifact messages identify the exact host libraries.
"""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / '.hkpsx/particle-track-audit/portable'


def main():
    report = ROOT / '.hkpsx/break-effects/report.json'
    if not report.is_file():
        raise SystemExit('Missing .hkpsx/break-effects/report.json; run the local Windows asset build first')
    sys.path.insert(0, str(ROOT / 'host'))
    import break_effects

    OUTPUT.mkdir(parents=True, exist_ok=True)
    # Snapshot once: all fixture generation and provenance use these same bytes.
    report_bytes = report.read_bytes()
    (OUTPUT / 'report.json').write_bytes(report_bytes)
    source = json.loads(report_bytes)
    tracks = break_effects.particle_tracks(source['styles'], source['emitters'])
    fixture = OUTPUT / 'cooked.rs'
    fixture.write_text(break_effects.generate(source['styles'], source['emitters'],
        source['uploads'], source['art'], source['frames'], tracks))

    runtime = (ROOT / 'game/src/break_effects.rs').read_text()
    if '#[cfg(test)]\nmod tests {' not in runtime or not runtime.rstrip().endswith('}'):
        raise SystemExit('Break-effects native test module changed; update the proof inclusion point')
    # Tests need private scalar reference helpers, so include them in the existing
    # final cfg(test) module of an ignored copy, leaving production untouched.
    end = runtime.rfind('}')
    runtime = runtime[:end] + '\ninclude!(env!("HK_TRACK_TESTS"));\n' + runtime[end:]
    (OUTPUT / 'break_effects.rs').write_text(runtime)
    particles = (ROOT / 'game/src/particles.rs').read_text()
    for name in ('break_effects.rs', 'particle_perspective.rs', 'particle_division.rs'):
        path = OUTPUT / name if name == 'break_effects.rs' else ROOT / 'game/src' / name
        old = f'#[path="{name}"]'
        if particles.count(old) != 1:
            raise SystemExit(f'Particle module path changed: {name}')
        particles = particles.replace(old, f'#[path={json.dumps(str(path))}]')
    (OUTPUT / 'particles.rs').write_text(particles)
    harness = '''const KNIGHT_SCALE:i32=60693;
mod render {
    pub const SOURCE_ALPHA_COVERAGE:u8=1;
    pub fn texture(_:usize,_:[(i16,i16);4],_:(u8,u8,u8)) {}
    pub fn texture_material_alpha(_:usize,_:[(i16,i16);4],_:(u8,u8,u8),_:u8) {}
}
#[path=DEBRIS] pub mod debris;
mod world {pub use crate::debris;}
#[path=PARTICLES] mod particles;
'''.replace('DEBRIS', json.dumps(str(ROOT / 'game/src/debris.rs'))).replace(
        'PARTICLES', json.dumps(str(OUTPUT / 'particles.rs')))
    (OUTPUT / 'probe.rs').write_text(harness)

    env = dict(os.environ, CARGO_TARGET_DIR=str(OUTPUT / 'cargo'),
        CARGO_MANIFEST_DIR=str(ROOT / 'game'), HK_TRACK_FIXTURE=str(fixture),
        HK_TRACK_TESTS=str(ROOT / 'tests/particle_tracks_source.rs'))
    cargo_command = ['cargo', 'build', '--locked', '--manifest-path',
        str(ROOT / 'shared/hk-sim/Cargo.toml'), '--message-format=json']
    result = subprocess.run(cargo_command, cwd=ROOT, env=env, check=True,
        capture_output=True, text=True)
    (OUTPUT / 'cargo.log').write_text(result.stdout + result.stderr)
    libraries = {}
    for line in result.stdout.splitlines():
        record = json.loads(line)
        if record.get('reason') != 'compiler-artifact':
            continue
        name = record['target']['name']
        if name in ('hk_sim', 'hk_format'):
            candidates = [Path(p) for p in record['filenames'] if p.endswith('.rlib')]
            if len(candidates) != 1:
                raise SystemExit(f'Expected one native rlib artifact for {name}')
            libraries[name] = candidates[0]
    if set(libraries) != {'hk_sim', 'hk_format'}:
        raise SystemExit('Cargo did not report both native shared-library artifacts')
    command = ['rustc', '--edition=2021', '-Awarnings', '--test', '-O',
        str(OUTPUT / 'probe.rs'), '-o', str(OUTPUT / 'probe')]
    for name, path in sorted(libraries.items()):
        command += ['--extern', f'{name}={path}']
    command += ['-L', f'dependency={libraries["hk_sim"].parent}']
    subprocess.run(command, cwd=ROOT, env=env, check=True)
    run_command = [str(OUTPUT / 'probe'), 'actual_', '--nocapture']
    result = subprocess.run(run_command, cwd=ROOT, env=env, capture_output=True, text=True)
    (OUTPUT / 'native.log').write_text(result.stdout + result.stderr)
    inputs = [ROOT / p for p in ('tools/test_particle_tracks.py',
        'tests/particle_tracks_source.rs', 'host/break_effects.py',
        'game/src/break_effects.rs', 'game/src/particles.rs', 'game/src/debris.rs',
        'game/src/particle_perspective.rs', 'game/src/particle_division.rs')]
    inputs += [OUTPUT / p for p in ('report.json', 'cooked.rs', 'break_effects.rs', 'particles.rs', 'probe.rs')]
    inputs += list(libraries.values())
    hashes = {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in inputs}
    (OUTPUT / 'proof.json').write_text(json.dumps(dict(returncode=result.returncode,
        commands=[cargo_command, command, run_command], sha256=hashes,
        stdout=result.stdout, stderr=result.stderr), indent=2) + '\n')
    print(result.stdout + result.stderr, end='')
    print(f'Proof artifacts: {OUTPUT}')
    raise SystemExit(result.returncode)


if __name__ == '__main__':
    main()
