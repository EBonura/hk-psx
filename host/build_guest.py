"""Build using the pinned SDK snapshot, scan the final binary and pack a disc."""
import os, subprocess, shutil, tempfile, json, hashlib
from pathlib import Path
from paths import DISC_LIBRARY, artifacts, build_directory, link_map_path, hazard_report_path
ROOT=Path(__file__).resolve().parents[1]
def scene_manifest():
    import json
    metadata=ROOT/'data/regions.json'
    report=json.loads(metadata.read_text())
    if not report.get('complete'):raise ValueError('Region cook is incomplete')
    snapshot=ROOT/'.hkpsx/selected-regions.json'
    snapshot.write_text(metadata.read_text())
    run([ROOT/'.venv/bin/python',ROOT/'host/pack_scenes.py',snapshot],cwd=ROOT)
    return json.loads((ROOT/'.hkpsx/packed-scenes.json').read_text())['scenes']
def run(args,**kw):
    print('+', ' '.join(map(str,args)),flush=True)
    subprocess.run(list(map(str,args)),check=True,**kw)
def build(telemetry=False, candidate=False):
    # Derive reservations from the exact final packs, including direct builds.
    from scenery_geometry import generate
    generate(ROOT)
    rooms = scene_manifest()
    # Certificates bind to the final deduplicated bank geometry/palette words.
    # Keep direct guest builds on the same reproducible path as tools/build.py.
    from opaque_tiles import cook as cook_opaque_tiles
    cook_opaque_tiles()
    from opaque_groups import cook as cook_opaque_groups
    cook_opaque_groups()
    ambience=json.loads((ROOT/'.hkpsx/ambience.json').read_text())
    clips=ambience['clips']
    for path,expected in (
        (ROOT/'data/ambience.rs',ambience['rust_manifest_sha256']),
        (ROOT/'data/sfx.adpcm',ambience['sfx_reservation']['sha256']),
        (ROOT/'data/sfx.rs',ambience['sfx_reservation']['manifest_sha256']),
    ):
        if hashlib.sha256(path.read_bytes()).hexdigest()!=expected:
            raise ValueError('Ambience descriptor/SFX reservation changed: '+str(path))
    arena=json.loads((ROOT/'.hkpsx/packed-scenes.json').read_text())['scene_arena_bytes']
    for clip in clips:
        path=ROOT/clip['path'];payload=path.read_bytes()
        if len(payload)!=clip['byte_len'] or hashlib.sha256(payload).hexdigest()!=clip['sha256']:
            raise ValueError('Ambience payload changed after cooking: '+str(path))
        if (len(payload)+2047)//2048*2048>arena:
            raise ValueError('Ambience clip does not fit the startup room arena: '+str(path))
    out=build_directory(telemetry,candidate);out.mkdir(parents=True,exist_ok=True)
    env=dict(os.environ,CARGO_TARGET_DIR=str(out/'guest'))
    link_map=link_map_path(telemetry,candidate)
    from stack_budget import prepare_linker, check_main_frame
    linker=prepare_linker(ROOT,out)
    env['RUSTFLAGS']=f'-Cllvm-args=-disable-mips-df-backward-search -Clink-arg=-T{linker} -Clink-arg=--oformat=binary -Clink-arg=-Map={link_map}'
    # Cargo restores cached EXEs without rerunning the linker. Each feature
    # variant must retain its own map; a missing map requires a fresh guest link.
    if not link_map.exists():
        run(['cargo','clean','--release','--target','mipsel-sony-psx','-p','hk-psx'],cwd=ROOT/'game',env=env)
    cmd=['cargo','build','--locked','--release']
    if telemetry:cmd+=['--features','emulator-telemetry']
    run(cmd,cwd=ROOT/'game',env=env)
    exe=out/'guest/mipsel-sony-psx/release/hk-psx.exe'
    from hazards import scan
    scan(exe,link_map,report_path=hazard_report_path(telemetry,candidate))
    check_main_frame(exe,link_map.read_text())
    shutil.copy2(link_map,out/'hk-psx.map')
    outputs = artifacts(telemetry,candidate)
    outputs['exe'].parent.mkdir(parents=True,exist_ok=True)
    DISC_LIBRARY.mkdir(parents=True,exist_ok=True)
    # Build the complete pair before replacing the library's last good image.
    # Even temporary BIN/CUE files remain inside the one disc library.
    with tempfile.TemporaryDirectory(prefix='.hk-psx-',dir=DISC_LIBRARY) as temp, \
         tempfile.TemporaryDirectory(prefix='disc-chunks-',dir=out) as chunk_temp:
        staged=Path(temp)/outputs['bin'].name
        chunks = Path(chunk_temp)
        for room in rooms:
            shutil.copy2(room['path'],chunks/f"chunk_{room['chunk_id']}.hk")
        for index,clip in enumerate(clips):
            shutil.copy2(ROOT/clip['path'],chunks/f"chunk_{len(rooms)+index+1}.adpcm")
        run(['cargo','run','--locked','--release','--manifest-path',ROOT/'.psoxide/tools/mkisopsx/Cargo.toml','--','--exe',exe,'--out',staged,'--volume','HKPSX','--world-pack-extra-dir',chunks],cwd=ROOT)
        staged.replace(outputs['bin'])
        staged.with_suffix('.cue').replace(outputs['cue'])
    shutil.copy2(exe,outputs['exe'])
    return outputs
if __name__=='__main__':
    import sys
    build('--telemetry' in sys.argv, candidate='--candidate' in sys.argv)
