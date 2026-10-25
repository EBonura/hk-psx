"""Build using the pinned SDK snapshot and scan the final binary. The disc is
packed by host/hk-build/disc.rs."""
import os, re, subprocess, shutil, json, hashlib
from pathlib import Path
from paths import DISC_NAME, artifacts, build_directory, link_map_path, hazard_report_path
ROOT=Path(__file__).resolve().parents[1]
# The ADPCM banks the guest streams from the disc, in the order it uploads
# them (the world bank is read before the title screen, the rest at bootstrap).
# Each payload pairs with a generated manifest of the same stem that carries
# the length and checksum the guest validates the chunk against.
# Streamed banks in the guest's disc.rs AUDIO_BANKS order: the three boot
# ADPCM banks, the world one-shots and the quick map's room art, which is not
# audio but rides the same staged read (host/game_map.py).
AUDIO_BANKS=('sfx.adpcm','geo-audio.adpcm','runner-audio.adpcm','world-sfx.adpcm','game-map.bin')
def bank_manifest(name):
    """The generated .rs carrying a streamed bank's BANK_BYTES and BANK_CHECKSUM."""
    return re.sub(r'\.(adpcm|bin)$','.rs',name)
def fnv(data):
    value=0x811c9dc5
    for byte in data:value=((value^byte)*0x01000193)&0xffffffff
    return value
def check_audio_banks():
    """Each streamed bank's payload against the constants the guest links."""
    for name in AUDIO_BANKS:
        payload=(ROOT/'data'/name).read_bytes()
        manifest=(ROOT/'data'/bank_manifest(name)).read_text()
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

# Emulator-driven PGO (the SDK's tools/psoxide-pgo): line tables and
# discriminators for sample profiling. The flat image drops them at link time.
PROFILE_FLAGS=' -Cdebuginfo=1 -Zdebug-info-for-profiling -Cstrip=none'
def delay_slot_flags():
    """Every MIPS delay-slot filler search on, as the pinned SDK defines it
    (tools/sdk-examples.mk PSX_DELAY_SLOT_FLAGS). Slots get useful work instead
    of the nops -disable-mips-df-backward-search left; hazards.patch then
    trampolines the few loads whose consumer lands inside their delay.
    The SDK writes them as a TOML list body ("-Cflag","-Cflag") for its
    --config rustflags; plain space-separated flags are accepted too."""
    text=(ROOT/'.psoxide/tools/sdk-examples.mk').read_text()
    m=re.search(r'^PSX_DELAY_SLOT_FLAGS\s*:=\s*(.+)$',text,re.M)
    if not m:raise ValueError('The pinned SDK does not define PSX_DELAY_SLOT_FLAGS')
    flags=re.findall(r'"([^"]+)"',m[1]) or m[1].split()
    if not flags or any(re.search(r'[\s",]',f) for f in flags):raise ValueError('Unreadable PSX_DELAY_SLOT_FLAGS: '+m[1])
    return ' '.join(flags)
# The boot art chunk: the title art (data/menu.hk), then the art the title
# uploads for the game that follows it and nothing reads again, each padded to
# a word: the Perpetua glyph sheet (data/read_font.hk), the Geo art
# (data/geo.hk), the Lifeblood art (data/lifeblood.hk) and the title's ornaments
# (data/menu-ornaments.hk: the flourishes and the menu pointer) and the area cards' big glyphs
# (data/card-font.hk, 7,680 bytes for VRAM rows 500..511), then the default title style's extras
# (data/menu-x-9.hk: its particles and animated sprites) and the logos (data/menu-logos.hk). Read into the
# scene arena before the title, it costs no RAM once uploaded; linked into the
# guest the last three held 17,774 bytes for good.
#
# The other nine title styles follow, each from a sector boundary: its background (data/menu-bg-N.hk, the
# layout of menu.hk) and its extras (data/menu-x-N.hk). The boot reads only the first part
# (`BOOT_COMMON_BYTES`); the options page reads a style's sectors when the player picks it
# (disc::Cache::style_art), because the arena holds one style beside the common part.
BOOT_ART_PARTS=('menu.hk','read_font.hk','geo.hk','lifeblood.hk','menu-ornaments.hk','card-font.hk','menu-x-9.hk','menu-logos.hk')
STYLE_COUNT=10
DEFAULT_STYLE=9

def module_tables():
    """Write the generated module tables the guest includes (data/modules.rs,
    data/props_art.rs) and return (scene module table, room art packages).
    hk-build calls it before the host suite, whose guest-target check compiles
    the crate: a fresh clone has neither file until then."""
    import code_modules
    packed=json.loads((ROOT/'.hkpsx/packed-scenes.json').read_text())
    regions_rs=(ROOT/'data/regions.rs').read_text()
    props_rs=(ROOT/'data/props.rs').read_text()
    # Room art travels with its rooms too: the prop art split by kind.
    art,art_rs=code_modules.props_art(props_rs,(ROOT/'data/props.hk').read_bytes())
    if code_modules.HORNET:
        # Her room's state block: the fight lives in these zeroed pool bytes (shared/hk-sim hornet_fight.rs STATE_BYTES).
        import re
        size=int(re.search(r'pub const STATE_BYTES: usize = (\d+);',(ROOT/'shared/hk-sim/src/hornet_fight.rs').read_text())[1])
        art['art_hornet']=bytes(size)
    for name,text in (('modules.rs',code_modules.manifest_rust(packed,regions_rs,props_rs)),('props_art.rs',art_rs)):
        if not (ROOT/'data'/name).is_file() or (ROOT/'data'/name).read_text()!=text:(ROOT/'data'/name).write_text(text)
    return code_modules.scene_table(packed,regions_rs,props_rs),art

def boot_art():
    """Write data/boot-art.hk and the offsets the guest links (data/boot_art.rs)."""
    blob=bytearray();at={}
    for name in BOOT_ART_PARTS:
        data=(ROOT/'data'/name).read_bytes()
        at[name]=(len(blob),len(data))
        blob+=data+bytes(-len(data)%4)
    # The opening-movie player (intro/, built by hk-build into data/intro.exe) rides at the end, for builds with
    # the `intro` feature only: the chunk is read from disc and dropped after the title, so it costs no RAM.
    intro=ROOT/'data/intro.exe'
    intro_at=(0,0)
    if os.environ.get('HK_INTRO')=='1' and intro.is_file():
        import struct
        exe=intro.read_bytes()
        pc0,gp0,load,size=struct.unpack_from('<4I',exe,0x10);sp=sum(struct.unpack_from('<2I',exe,0x30))&0xffffffff
        # game/src/intro.rs launch trusts this: the image lies inside the room-module pool (empty at the title) and
        # its stack starts under the mailbox.
        if exe[:8]!=b'PS-X EXE' or load<0x801DB000 or load+size>0x801F3F00 or sp>0x801FFFE0 or len(exe)<0x800+size:
            raise ValueError(f'data/intro.exe does not fit the pool: load {load:#x} size {size:#x} sp {sp:#x}')
        data=struct.pack('<5I',load,size,pc0,gp0,sp)+exe[0x800:0x800+size];intro_at=(len(blob),len(data));blob+=data+bytes(-len(data)%4)
    def fnv32(data):
        h=0x811c9dc5
        for byte in data:h=((h^byte)*0x01000193)&0xffffffff
        return h
    # The common part ends on a sector boundary; the styles follow, each from one.
    blob+=bytes(-len(blob)%2048)
    common_len=len(blob);common_fnv=fnv32(blob)
    styles=[]
    for i in range(STYLE_COUNT):
        if i==DEFAULT_STYLE:
            styles.append((0,at['menu-x-9.hk'][0],at['menu-x-9.hk'][1],0));continue
        bg=(ROOT/f'data/menu-bg-{i}.hk').read_bytes();x=(ROOT/f'data/menu-x-{i}.hk').read_bytes()
        start=len(blob);body=bg+bytes(-len(bg)%4)+x
        styles.append((start,start+len(bg)+(-len(bg)%4),len(x),fnv32(body)))
        blob+=body+bytes((-(len(blob)+len(body)))%2048)
    fnv=fnv32(blob)
    (ROOT/'data/boot-art.hk').write_bytes(bytes(blob))
    text=('// Generated by host/build_guest.py boot_art() from '+', '.join('data/'+n for n in BOOT_ART_PARTS)+'.\n'
          f'pub const BOOT_ART_BYTES:usize={len(blob)};\npub const BOOT_ART_CHECKSUM:u32={fnv};\n'
          f'pub const FONT_AT:usize={at["read_font.hk"][0]};\n'
          f'pub const GEO_ART:(usize,usize)={at["geo.hk"]};\npub const LIFE_ART:(usize,usize)={at["lifeblood.hk"]};\n'
          f'pub const TITLE_ORNAMENTS:(usize,usize)={at["menu-ornaments.hk"]};\n'
          f'pub const CARD_FONT:(usize,usize)={at["card-font.hk"]};\n'
          f'pub const BOOT_COMMON_BYTES:usize={common_len};\npub const BOOT_COMMON_CHECKSUM:u32={common_fnv};\n'
          f'/// Per style: where its background starts, where its extras start and their length, and the checksum of both (0 for the default style, which rides the common part).\n'
          f'pub const STYLE_ART:[(u32,u32,u32,u32);{STYLE_COUNT}]={[tuple(s) for s in styles]};\n'
          f'pub const LOGO_ART:(usize,usize)={at["menu-logos.hk"]};\n'
          f'pub const INTRO_ART:(usize,usize)={intro_at};\n')
    path=ROOT/'data/boot_art.rs'
    if not path.is_file() or path.read_text()!=text:path.write_text(text)
    if at['read_font.hk'][0]!=len((ROOT/'data/menu.hk').read_bytes()):raise ValueError('the glyphs must follow the title art')

def build(telemetry=False, candidate=False, residency="scene_gate", run_prepass=True, profile=None, work=None, elf_only=False):
    """profile: None for the ordinary build, 'collect' for the profiling flat
    image, 'elf' for the same code linked as an ELF that keeps its DWARF
    (work/hk-psx.elf, no disc or scans), or the path of an LLVM sample profile
    to optimise with. work: put the map, hazard report, EXE and disc in this
    directory instead of the canonical ones."""
    # No 64-bit integer or float type in guest source: the source half of the symbol gate.
    run(['sh',ROOT/'tools/check-no64.sh'])
    if profile is None and not elf_only:
        from shared_publish import tool_source
        tool_source()  # Fail before any guest compilation when the publisher is unavailable.
    if residency!="scene_gate":raise ValueError('Guest coverage residency requires scene_gate')
    if elf_only and (work is None or profile is not None):
        raise ValueError('ELF-only measurement requires --work and canonical production flags without --profile')
    spaced=[str(p) for p in (ROOT,work or '',profile or '') if re.search(r'\s',str(p))]
    if profile is not None and spaced:
        # A profile binds to the checkout path that built it, and the pgo
        # driver has only ever run from paths without spaces.
        raise ValueError('PGO needs paths without spaces: '+', '.join(spaced))
    # The driver skips the prepass when its inputs and outputs are unchanged.
    if run_prepass:prepass(residency)
    boot_art()
    ambience=json.loads((ROOT/'.hkpsx/ambience.json').read_text())
    clips=ambience['clips']
    for path,expected in (
        (ROOT/'data/ambience.rs',ambience['rust_manifest_sha256']),
        (ROOT/'data/sfx.adpcm',ambience['sfx_reservation']['sha256']),
        (ROOT/'data/sfx.rs',ambience['sfx_reservation']['manifest_sha256']),
    ):
        if hashlib.sha256(path.read_bytes()).hexdigest()!=expected:
            raise ValueError('Ambience descriptor/SFX reservation changed: '+str(path))
    area_music=json.loads((ROOT/'.hkpsx/area-music.json').read_text())
    if hashlib.sha256((ROOT/'data/area_music.rs').read_bytes()).hexdigest()!=area_music['manifest_sha256']:raise ValueError('Area music descriptor changed after cooking')
    for track in area_music['tracks']:
        if hashlib.sha256((ROOT/track['path']).read_bytes()).hexdigest()!=track['sha256']:raise ValueError('Area music payload changed after cooking: '+track['path'])
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
    out=Path(work).resolve() if work else build_directory(telemetry,candidate);out.mkdir(parents=True,exist_ok=True)
    # Keep each output variant's guest cache private when the orchestrator
    # supplies a target directory, including builds from a managed worktree.
    base_target=os.environ.get('CARGO_TARGET_DIR')
    guest_target=(Path(base_target)/'hk-guest'/hashlib.sha256(str(out).encode()).hexdigest()[:12]) if base_target else out/'guest'
    env=dict(os.environ,CARGO_TARGET_DIR=str(guest_target))
    link_map=out/'hk-psx-link.map' if work else link_map_path(telemetry,candidate)
    from stack_budget import prepare_linker, check_main_frame
    import code_modules
    scene_code,art=module_tables()
    base_linker=prepare_linker(ROOT,out)
    # Ordinary publication uses the shared SDK tool; profiling retains its own path.
    if profile is None:os.environ["HK_SHARED_PUBLISHER"]="1"
    layered=code_modules.layer(base_linker.read_text())
    linker=out/f'hk-psx-ovl-{hashlib.sha256(layered.encode()).hexdigest()[:16]}.ld'
    linker.write_text(layered)
    # A list, handed to cargo as CARGO_ENCODED_RUSTFLAGS (0x1f-separated):
    # RUSTFLAGS is split on whitespace, so a checkout under a path with a
    # space (say ~/Library/Application Support) handed rustc half a linker
    # script path and failed before compiling anything.
    # The linker writes an ELF, not a flat binary: LLD's binary output puts
    # every code module in the output at its link address. code_modules
    # cuts the PS-X EXE (header, .text, .data) and the module images out of
    # it; --emit-relocs keeps the relocations its fixups and checks read.
    flags=delay_slot_flags().split()+[f'-Clink-arg=-T{linker}','-Clink-arg=--emit-relocs',f'-Clink-arg=-Map={link_map}']
    # A panic is a BREAK at the faulting instruction (-Cpanic=immediate-abort), not a formatted message: core::fmt, the panic
    # Location records and their file names (about 23 KB of the resident image) stay out of RAM. psx-rt's exception handler
    # records Cause/EPC/BadVAddr (__psx_rt_fault_*), which the gates already read (__psx_rt_fault_count) and
    # `psoxide-size faults` names. PSOXIDE_PANIC_MESSAGES=1 restores the formatted panic. Modules need the SDK overlay tools
    # of SDK_TOOL_REVISION (HI16 behind a loop back edge, TRAMPOLINE_WORDS) and code_modules.TRAMP_WORDS to match.
    if os.environ.get('PSOXIDE_PANIC_MESSAGES','0') in ('','0'):flags=['-Zunstable-options','-Cpanic=immediate-abort']+flags
    if profile=='elf':
        flags=flags+PROFILE_FLAGS.split()
    elif profile=='collect':
        flags+=PROFILE_FLAGS.split()
    elif profile is not None:
        # -profile-sample-accurate treats code the tapes never reached as cold
        # rather than unknown, which keeps the optimised image from growing
        # into the little RAM this game has left.
        # -pgso=false was measured on SDK 6da88d92d with the same profile
        # (2026-09-23): gameplay fps moved by under 0.1% on both routes while
        # code grew 26,344 bytes and free RAM fell from 52,576 to 25,952.
        # Profile-guided size optimisation of cold code stays on here, though
        # turning it off was Cortex's win.
        # -hot-callsite-threshold=1000 (LLVM's default is 3000) was measured
        # on perf/sim-caches (2026-09-23) with one profile against the default:
        # code 733,160 -> 681,840 bytes and free RAM 48,476 -> 101,724, gameplay
        # fps 24.430 -> 24.492 on kings and 21.804 -> 21.903 on crossroads, work
        # per frame within 0.3% either way. -pgso=false on top of it kept the
        # fps but spent 30 KB of that RAM (free 71,004).
        flags+=PROFILE_FLAGS.split()+[f'-Zprofile-sample-use={Path(profile).resolve()}','-Cllvm-args=-profile-sample-accurate','-Cllvm-args=-hot-callsite-threshold=1000']
    env.pop('RUSTFLAGS',None);env['CARGO_ENCODED_RUSTFLAGS']='\x1f'.join(flags)
    # Cargo restores cached EXEs without rerunning the linker. Each feature
    # variant must retain its own map; a missing map requires a fresh guest link.
    if not link_map.exists():
        run(['cargo','clean','--release','--target','mipsel-sony-psx','-p','hk-psx'],cwd=ROOT/'game',env=env)
    cmd=['cargo','build','--locked','--release']
    features=(['emulator-telemetry'] if telemetry else [])+[f for f in os.environ.get('HK_GUEST_FEATURES','').split(',') if f]+(['audio-probe'] if os.environ.get('HK_AUDIO_PROBE') else [])+(['intro'] if os.environ.get('HK_INTRO')=='1' else [])+(['hornet'] if code_modules.HORNET else [])
    if features:cmd+=['--features',','.join(features)]
    run(cmd,cwd=ROOT/'game',env=env)
    linked=guest_target/'mipsel-sony-psx/release/hk-psx.exe'
    # No 64-bit integer, 128-bit or soft-float helper may be linked, with no opt-out.
    run(['sh',ROOT/'.psoxide/tools/guest_symbol_gate.sh',link_map],env={k:v for k,v in os.environ.items() if k!='GUEST_GATE_ALLOW'})
    if profile=='elf' or elf_only:
        shutil.copy2(linked,out/'hk-psx.elf')
        return {'elf':out/'hk-psx.elf'}
    # Split and patch from the linker's own bytes, so a cached link stays clean.
    exe=out/'hk-psx-patched.exe'
    from hazards import scan, stack_guard
    report=out/'hazards.json' if work else hazard_report_path(telemetry,candidate)
    if profile is None:
        from shared_publish import publish
        publish(ROOT, linked, link_map, out, scene_code, art)
        shutil.copy2(out/'runtime.exe',exe)
        resident_map=out/'resident.map'
        scan(exe,resident_map,report_path=report)
        stack_guard(exe,resident_map)
        check_main_frame(exe,link_map.read_text())
        if link_map.resolve()!=(out/'hk-psx.map').resolve():shutil.copy2(link_map,out/'hk-psx.map')
        outputs={'exe':out/'hk-psx.exe','bin':out/f'disc/{DISC_NAME}.bin','cue':out/f'disc/{DISC_NAME}.cue'} if work else artifacts(telemetry,candidate)
        outputs['exe'].parent.mkdir(parents=True,exist_ok=True)
        shutil.copy2(exe,outputs['exe'])
        return outputs
    module_report=code_modules.harden(linked,exe,link_map,out/'module-work',report.parent,scene_code,{**art,**code_modules.carried_blobs(ROOT)})
    (out/'modules.json').write_text(json.dumps(module_report,indent=1))
    # The EXE alone holds no module, so its checks read the map without them.
    resident_map=Path(module_report['resident_map'])
    scan(exe,resident_map,report_path=report,patched=module_report['resident_patch'])
    # After the patch: the guard follows its trampolines to the real callees.
    stack_guard(exe,resident_map)
    check_main_frame(exe,link_map.read_text())
    shutil.copy2(link_map,out/'hk-psx.map')
    outputs = {'exe':out/'hk-psx.exe','bin':out/f'disc/{DISC_NAME}.bin','cue':out/f'disc/{DISC_NAME}.cue'} if work else artifacts(telemetry,candidate)
    outputs['exe'].parent.mkdir(parents=True,exist_ok=True)
    shutil.copy2(exe,outputs['exe'])
    return outputs
if __name__=='__main__':
    import argparse
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--telemetry',action='store_true')
    parser.add_argument('--candidate',action='store_true')
    parser.add_argument('--residency',choices=('joint','scene_gate'),default='scene_gate')
    parser.add_argument('--no-prepass',action='store_true',help='reuse the packed scenes, tile certificates and coverage bundles already on disk')
    parser.add_argument('--profile',help="PGO stage: 'collect', 'elf', or the sample profile to optimise with")
    parser.add_argument('--work',type=Path,help='write the map, hazards, EXE and disc here instead of the canonical outputs')
    parser.add_argument('--elf-only',action='store_true',help='stop after linking with canonical production flags; writes ELF/map only, no qualification or disc')
    args=parser.parse_args()
    build(args.telemetry,candidate=args.candidate,residency=args.residency,run_prepass=not args.no_prepass,profile=args.profile,work=args.work,elf_only=args.elf_only)
