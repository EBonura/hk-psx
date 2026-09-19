"""Build using the pinned SDK snapshot, scan the final binary and pack a disc."""
import os, re, subprocess, shutil, tempfile, json, hashlib
from pathlib import Path
from paths import DISC_LIBRARY, artifacts, build_directory, link_map_path, hazard_report_path
ROOT=Path(__file__).resolve().parents[1]
# The ADPCM banks the guest streams from the disc at bootstrap, in the order it
# uploads them. Each payload pairs with a generated manifest of the same stem
# that carries the length and checksum the guest validates the chunk against.
AUDIO_BANKS=('sfx.adpcm','geo-audio.adpcm','runner-audio.adpcm')
def check_audio_banks():
    """Each streamed bank's payload against the constants the guest links."""
    from ambience import fnv
    for name in AUDIO_BANKS:
        payload=(ROOT/'data'/name).read_bytes()
        manifest=(ROOT/'data'/name.replace('.adpcm','.rs')).read_text()
        fields={key:int(value) for key,value in
                re.findall(r'const (BANK_BYTES|BANK_CHECKSUM)\s*:\s*\w+\s*=\s*(\d+);',manifest)}
        if fields.get('BANK_BYTES')!=len(payload) or fields.get('BANK_CHECKSUM')!=fnv(payload):
            raise ValueError('Streamed audio bank does not match its manifest: data/'+name)
def scene_manifest(residency="scene_gate"):
    import json
    metadata=ROOT/'data/regions.json'
    report=json.loads(metadata.read_text())
    if not report.get('complete'):raise ValueError('Region cook is incomplete')
    snapshot=ROOT/'.hkpsx/selected-regions.json'
    snapshot.write_text(metadata.read_text())
    run([ROOT/'.venv/bin/python',ROOT/'host/pack_scenes.py',snapshot,'--residency',residency],cwd=ROOT)
    return json.loads((ROOT/'.hkpsx/packed-scenes.json').read_text())['scenes']
def run(args,**kw):
    print('+', ' '.join(map(str,args)),flush=True)
    subprocess.run(list(map(str,args)),check=True,**kw)
def prepass(residency="scene_gate"):
    """Cooked-data stages between the region cook and the guest link."""
    # Derive reservations from the exact final packs, including direct builds.
    from scenery_geometry import generate
    generate(ROOT)
    scene_manifest(residency)
    # Certificates bind to the final deduplicated bank geometry/palette words.
    # Keep direct guest builds on the same reproducible path as tools/build.py.
    from opaque_tiles import cook as cook_opaque_tiles
    cook_opaque_tiles()
    from opaque_groups import cook as cook_opaque_groups
    cook_opaque_groups()
    # The compressor uses pinned host dependencies from the build environment.
    run([ROOT/'.venv/bin/python',ROOT/'host/scene_certificates.py'],cwd=ROOT)

def build(telemetry=False, candidate=False, residency="scene_gate", package_disc=True, run_prepass=True):
    if residency!="scene_gate":raise ValueError('Guest coverage residency requires scene_gate')
    # The driver skips the prepass when its inputs and outputs are unchanged.
    if run_prepass:prepass(residency)
    packed=json.loads((ROOT/'.hkpsx/packed-scenes.json').read_text())
    rooms=packed['scenes'];atlases=packed['atlases']
    ambience=json.loads((ROOT/'.hkpsx/ambience.json').read_text())
    clips=ambience['clips']
    coverage=json.loads((ROOT/'.hkpsx/scene-certificates.json').read_text())
    music=json.loads((ROOT/'.hkpsx/title-music.json').read_text())
    if hashlib.sha256((ROOT/music['path']).read_bytes()).hexdigest()!=music['sha256'] or hashlib.sha256((ROOT/'data/title_music.rs').read_bytes()).hexdigest()!=music['manifest_sha256']:raise ValueError('Title music changed after cooking')
    for path,expected in (
        (ROOT/'data/ambience.rs',ambience['rust_manifest_sha256']),
        (ROOT/'data/sfx.adpcm',ambience['sfx_reservation']['sha256']),
        (ROOT/'data/sfx.rs',ambience['sfx_reservation']['manifest_sha256']),
    ):
        if hashlib.sha256(path.read_bytes()).hexdigest()!=expected:
            raise ValueError('Ambience descriptor/SFX reservation changed: '+str(path))
    focus=json.loads((ROOT/'.hkpsx/focus-audio.json').read_text())
    for path,key in ((ROOT/focus['path'],'sha256'),(ROOT/'data/focus-audio.rs','manifest_sha256')):
        if hashlib.sha256(path.read_bytes()).hexdigest()!=focus[key]:raise ValueError('Focus audio changed after cooking')
    check_audio_banks()
    arena=json.loads((ROOT/'.hkpsx/packed-scenes.json').read_text())['scene_arena_bytes']
    if (focus['byte_len']+2047)//2048*2048>arena:raise ValueError('Focus bank exceeds startup arena')
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
    if package_disc:
        DISC_LIBRARY.mkdir(parents=True,exist_ok=True)
        # Build the complete pair before replacing the library's last good
        # image. Even temporary BIN/CUE files remain inside the one disc
        # library.
        with tempfile.TemporaryDirectory(prefix='.hk-psx-',dir=DISC_LIBRARY) as temp, \
             tempfile.TemporaryDirectory(prefix='disc-chunks-',dir=out) as chunk_temp:
            staged=Path(temp)/outputs['bin'].name
            chunks = Path(chunk_temp)
            for room in rooms:
                shutil.copy2(room['path'],chunks/f"chunk_{room['chunk_id']}.hk")
            for index,clip in enumerate(clips):
                shutil.copy2(ROOT/clip['path'],chunks/f"chunk_{len(rooms)+index+1}.adpcm")
            for index,atlas in enumerate(atlases):
                shutil.copy2(atlas['path'],chunks/f"chunk_{len(rooms)+len(clips)+index+1}.atlas")
            shutil.copy2(ROOT/focus['path'],chunks/f"chunk_{len(rooms)+len(clips)+len(atlases)+1}.focus")
            for bundle in coverage['bundles']:
                shutil.copy2(bundle['path'],chunks/f"chunk_{bundle['chunk_id']}.coverage")
            world_banks=json.loads((ROOT/'.hkpsx/packed-scenes.json').read_text()).get('world_metadata',[])
            for bank in world_banks:
                shutil.copy2(bank['path'],chunks/f"chunk_{bank['chunk_id']}.worldmeta")
            # One effect-art chunk per catalog scene id, after the world metadata chunks.
            effect_base=len(rooms)+len(clips)+len(atlases)+1+len(coverage['bundles'])+len(world_banks)
            effect_chunks=json.loads((ROOT/'.hkpsx/break-effects/report.json').read_text())['chunks']
            for entry in effect_chunks:
                shutil.copy2(entry['path'],chunks/f"chunk_{effect_base+entry['scene_id']+1}.effectart")
            # The SFX, Geo and Runner ADPCM banks, last and in the guest's upload
            # order. They used to be linked into the guest; streaming them at
            # bootstrap returned 109,536 bytes of RAM, and the SPU owns the
            # samples from the one upload just as it did before.
            for index,name in enumerate(AUDIO_BANKS):
                payload=ROOT/'data'/name
                if (len(payload.read_bytes())+2047)//2048*2048>arena:
                    raise ValueError('Audio bank does not fit the startup room arena: '+str(payload))
                shutil.copy2(payload,chunks/f"chunk_{effect_base+len(effect_chunks)+index+1}.bank")
            run(['cargo','run','--locked','--release','--manifest-path',ROOT/'.psoxide/tools/mkisopsx/Cargo.toml','--','--exe',exe,'--out',staged,'--volume','HKPSX','--world-pack-extra-dir',chunks,'--cdda-track',ROOT/music['path']],cwd=ROOT)
            staged.replace(outputs['bin'])
            staged.with_suffix('.cue').replace(outputs['cue'])
    shutil.copy2(exe,outputs['exe'])
    return outputs
if __name__=='__main__':
    import argparse
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--telemetry',action='store_true')
    parser.add_argument('--candidate',action='store_true')
    parser.add_argument('--residency',choices=('joint','scene_gate'),default='scene_gate')
    parser.add_argument('--no-disc',action='store_true',help='build and validate the guest EXE without replacing the playable BIN/CUE')
    parser.add_argument('--no-prepass',action='store_true',help='reuse the packed scenes, tile certificates and coverage bundles already on disk')
    args=parser.parse_args()
    build(args.telemetry,candidate=args.candidate,residency=args.residency,package_disc=not args.no_disc,run_prepass=not args.no_prepass)
