"""rustc arguments that give a standalone `rustc --test` build of game sources the crates they name.

The runtime harnesses include game modules by `#[path]` and compile them with plain rustc. Game code
reaches the SDK's `psx_math` (the one sanctioned wide arithmetic), `psx_pack` (the shared FNV), `psx_fx` (the seed step) and the
shared simulation crate's `hk_sim::fixed`, so their rlibs have to be on the command line.
"""
import json, os, subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
_cache = []


def externs():
    if not _cache:
        build = subprocess.run(['cargo', 'build', '--locked', '--manifest-path', str(ROOT / 'shared/hk-sim/Cargo.toml'),
                                '--message-format=json'], check=True, capture_output=True, text=True)
        libraries = {}
        for line in build.stdout.splitlines():
            artifact = json.loads(line)
            if artifact.get('reason') != 'compiler-artifact':
                continue
            name = artifact['target']['name']
            if name in ('hk_sim', 'psx_math', 'psx_pack'):
                libraries[name] = [Path(f) for f in artifact['filenames'] if f.endswith('.rlib')][0]
        assert set(libraries) == {'hk_sim', 'psx_math', 'psx_pack'}, libraries
        args = []
        for name, library in sorted(libraries.items()):
            args += ['--extern', f'{name}={library}']
        _cache.extend(args + ['-L', f'dependency={libraries["hk_sim"].parent / "deps"}'])
    return list(_cache) + psx_fx_args()


_fx = []


def psx_fx_args():
    """`--extern psx_fx=...` and the `-L` for its own dependencies: the seed step the game modules call. It is not under
    hk-sim, so it is built on its own (it needs the SDK's psx-gpu beside it)."""
    if not _fx:
        fx = subprocess.run(['cargo', 'build', '--locked', '--manifest-path', str(ROOT / '.psoxide/sdk/crates/psx-fx/Cargo.toml'),
                             '--message-format=json'], check=True, capture_output=True, text=True,
                            env=dict(os.environ, CARGO_TARGET_DIR=str(ROOT / '.hkpsx/psx-fx-target')))
        lib = [Path(f) for line in fx.stdout.splitlines() for a in [json.loads(line)]
               if a.get('reason') == 'compiler-artifact' and a['target']['name'] == 'psx_fx'
               for f in a['filenames'] if f.endswith('.rlib')][0]
        _fx.extend(['--extern', f'psx_fx={lib}', '-L', f'dependency={lib.parent / "deps"}'])
    return list(_fx)
