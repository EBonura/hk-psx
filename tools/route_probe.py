#!/usr/bin/env python3
"""Run native collision/nail route diagnostics against local cooked room packs.

This is not an emulator test: ticks start at gameplay, without title/CD delays,
actors, health/recoil, hazard respawning or gate warping. Hazard and checkpoint
contacts are reported. Retail inputs and generated Rust remain under .hkpsx.

The guest modules are compiled where they live and built with `--cfg test`, the
same shape tests/world_runtime.rs uses, so rustc resolves whatever submodules
world.rs declares and the PS1 presentation paths drop out. Nothing here edits a
guest file; tests/test_route_probe.py is what catches this drifting again.
"""
import argparse
import hashlib
import json
import os
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--route', required=True, help='named route fixture, tick:button:duration CSV, or @route-file')
    parser.add_argument('--ticks', type=int)
    parser.add_argument('--metadata', type=Path, default=ROOT/'.hkpsx/selected-regions.json')
    # The Mantis Claw is off by default because a wall slide and a wall jump
    # need no button: granting it silently changes any route that presses into
    # a wall in mid-air, and such a route cannot reproduce on the guest.
    parser.add_argument('--claw', action='store_true',
                        help='grant the Mantis Claw, for authoring wall routes deliberately')
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
        if int(start) < 0 or int(duration) < 1 or button not in ('start', 'left', 'right', 'cross', 'square', 'up', 'down', 'l1', 'r1', 'triangle'):
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
    # `params.rs` is included by main.rs directly; the generated sources that
    # guest modules reach for as CARGO_MANIFEST_DIR/../data are staged beside
    # the crate so those includes resolve without editing any guest file.
    shutil.copyfile(ROOT/'data/params.rs', src/'params.rs')
    hashes['data/params.rs'] = hashlib.sha256((src/'params.rs').read_bytes()).hexdigest()
    generated = work.parent/'data'
    generated.mkdir(parents=True, exist_ok=True)
    for name in ('regions.rs',):
        shutil.copyfile(ROOT/'data'/name, generated/name)
        hashes['data/'+name] = hashlib.sha256((generated/name).read_bytes()).hexdigest()
    for crate in ('hk-sim', 'hk-format'):
        path = ROOT/'shared'/crate/'src/lib.rs'
        hashes[str(path.relative_to(ROOT))] = hashlib.sha256(path.read_bytes()).hexdigest()
    # The guest modules are compiled where they live, so rustc resolves the
    # submodules world.rs declares (debris, particles and their includes) on its
    # own. Copying world.rs into this crate is what used to break that.
    for name in ('world.rs', 'impact.rs', 'reveal_masks.rs', 'secret_breaks.rs', 'debris.rs', 'particles.rs'):
        hashes['game/src/'+name] = hashlib.sha256((ROOT/'game/src'/name).read_bytes()).hexdigest()
    # Every cooked per-scene world bank, which the probe selects between as it
    # crosses scenes, exactly as the guest admits one bank at a time.
    banks = sorted((int(p.stem.split('_')[1]), p) for p in (ROOT/'.hkpsx/world-metadata-packed').glob('scene_*.hkwm'))
    if not banks:
        raise ValueError('no cooked world metadata banks; run a build first')
    for scene, path in banks:
        hashes[str(path.relative_to(ROOT))] = hashlib.sha256(path.read_bytes()).hexdigest()
    (src/'paths.rs').write_text('const PATHS: &[&str] = &'+json.dumps(paths)+';\n'
        + 'const BANKS: &[(usize, &str)] = &['
        + ','.join(f'({scene},{json.dumps(str(path))})' for scene, path in banks) + '];\n')
    main = (ROOT/'tools/route_probe.rs').read_text()
    for module in ('world', 'impact', 'reveal_masks', 'secret_breaks'):
        old_decl = 'mod '+module+';'
        if old_decl not in main:
            raise ValueError('probe no longer declares mod '+module)
        main = main.replace(old_decl, '#[path='+json.dumps(str(ROOT/'game/src'/(module+'.rs')))+']mod '+module+';', 1)
    (src/'main.rs').write_text(main)
    # The guest modules call psx-math directly (debris.rs, break_effects.rs),
    # so the probe depends on the same SDK crate game/Cargo.toml does.
    crates = {crate: ROOT/'shared'/crate for crate in ('hk-sim', 'hk-format')}
    crates['psx-math'] = ROOT/'.psoxide/sdk/crates/psx-math'
    (work/'Cargo.toml').write_text('[package]\nname="hk-native-route-probe"\nversion="0.1.0"\nedition="2021"\n[workspace]\n[dependencies]\n'+
        ''.join(f'{crate}={{path={json.dumps(str(path))}}}\n' for crate, path in crates.items()))
    (work/'inputs.json').write_text(json.dumps({'metadata':str(args.metadata),'route':route,'ticks':args.ticks,'sha256':hashes}, indent=2)+'\n')
    # The guest already keeps its PS1 presentation behind cfg(test) and reads
    # its world bank from a thread local there, which is exactly the native
    # shape this probe needs; no guest file is edited to suit the probe.
    env = dict(os.environ, RUSTFLAGS=os.environ.get('RUSTFLAGS', '')+' --cfg test')
    subprocess.run(['cargo', 'build', '--offline', '--release', '--manifest-path', str(work/'Cargo.toml')], check=True, env=env)
    probe = [str(work/'target/release/hk-native-route-probe'), route, str(args.ticks)]
    subprocess.run(probe + (['--claw'] if args.claw else []), check=True)


if __name__ == '__main__':
    main()
