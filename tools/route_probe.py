#!/usr/bin/env python3
"""Run native collision/nail route diagnostics against local cooked room packs.

This is not an emulator test: ticks start at gameplay, without title/CD delays,
actors, health/recoil, hazard respawning or gate warping. Hazard and checkpoint
contacts are reported. Retail inputs and generated Rust remain under .hkpsx.
"""
import argparse
import hashlib
import json
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--route', required=True, help='named route fixture, tick:button:duration CSV, or @route-file')
    parser.add_argument('--ticks', type=int)
    parser.add_argument('--metadata', type=Path, default=ROOT/'.hkpsx/selected-regions.json')
    args = parser.parse_args()
    fixtures = json.loads((ROOT/'tools/route_fixtures.json').read_text())['routes']
    if args.route in fixtures:
        fixture = fixtures[args.route]
        route = fixture['events']
        if args.ticks is None:
            args.ticks = fixture['ticks']
    else:
        route = Path(args.route[1:]).read_text().strip() if args.route.startswith('@') else args.route
    if args.ticks is None:
        args.ticks = 1000
    for event in route.split(','):
        start, button, duration = event.split(':')
        if int(start) < 0 or int(duration) < 1 or button not in ('start', 'left', 'right', 'cross', 'square', 'up', 'down'):
            raise ValueError('Invalid route event: '+event)
    if args.ticks < 1:
        raise ValueError('ticks must be positive')
    metadata = json.loads(args.metadata.read_text())
    if not metadata.get('complete'):
        raise ValueError('Refusing an incomplete/in-progress cook')
    work = ROOT/'.hkpsx/native-route-probe'
    src = work/'src'
    src.mkdir(parents=True, exist_ok=True)
    paths = []
    hashes = {}
    for region in metadata['regions']:
        path = ROOT/region['path']
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        if digest != region['sha256']:
            raise ValueError('Pack changed since selected metadata: '+str(path))
        paths.append(str(path))
        hashes[str(path.relative_to(ROOT))] = digest
    # Snapshot generated metadata, not retail textures. The main cook only
    # updates regions.rs after completion; count checking catches partial sets.
    for name in ('params.rs', 'regions.rs'):
        shutil.copyfile(ROOT/'data'/name, src/name)
        hashes['data/'+name] = hashlib.sha256((src/name).read_bytes()).hexdigest()
    world = (ROOT/'game/src/world.rs').read_text()
    hashes['game/src/world.rs'] = hashlib.sha256(world.encode()).hexdigest()
    for crate in ('hk-sim', 'hk-format'):
        path = ROOT/'shared'/crate/'src/lib.rs'
        hashes[str(path.relative_to(ROOT))] = hashlib.sha256(path.read_bytes()).hexdigest()
    old = 'include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/regions.rs"));'
    if old not in world:
        raise ValueError('world.rs generated-metadata include changed; update probe adapter')
    (src/'world.rs').write_text(world.replace(old, 'include!("regions.rs");'))
    (src/'paths.rs').write_text('const PATHS: &[&str] = &'+json.dumps(paths)+';\n')
    shutil.copyfile(ROOT/'tools/route_probe.rs', src/'main.rs')
    (work/'Cargo.toml').write_text('[package]\nname="hk-native-route-probe"\nversion="0.1.0"\nedition="2021"\n[workspace]\n[dependencies]\n'+
        ''.join(f'{crate}={{path={json.dumps(str(ROOT/"shared"/crate))}}}\n' for crate in ('hk-sim', 'hk-format')))
    (work/'inputs.json').write_text(json.dumps({'metadata':str(args.metadata),'route':route,'ticks':args.ticks,'sha256':hashes}, indent=2)+'\n')
    subprocess.run(['cargo', 'build', '--offline', '--release', '--manifest-path', str(work/'Cargo.toml')], check=True)
    subprocess.run([str(work/'target/release/hk-native-route-probe'), route, str(args.ticks)], check=True)


if __name__ == '__main__':
    main()
