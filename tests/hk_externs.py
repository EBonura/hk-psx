"""rustc arguments that give a standalone `rustc --test` build of game sources the crates they name.

The runtime harnesses include game modules by `#[path]` and compile them with plain rustc. Game code
reaches the SDK's `psx_math` (the one sanctioned wide arithmetic) and the shared simulation crate's
`hk_sim::fixed`, so both rlibs have to be on the command line.
"""
import json, subprocess
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
            if name in ('hk_sim', 'psx_math'):
                libraries[name] = [Path(f) for f in artifact['filenames'] if f.endswith('.rlib')][0]
        assert set(libraries) == {'hk_sim', 'psx_math'}, libraries
        args = []
        for name, library in sorted(libraries.items()):
            args += ['--extern', f'{name}={library}']
        _cache.extend(args + ['-L', f'dependency={libraries["hk_sim"].parent / "deps"}'])
    return list(_cache)
