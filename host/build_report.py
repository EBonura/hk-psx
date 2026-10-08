#!/usr/bin/env python3
"""Bind the built EXE/BIN/CUE, link map and every cooked provenance into one build report."""
import argparse,hashlib,json,re,shutil,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'host'))
from paths import build_report_path

def sha(p):
    with Path(p).open('rb') as stream:return hashlib.file_digest(stream,'sha256').hexdigest()
def write_build_report(report,telemetry,candidate=False,latest=False,pgo=False):
    """Publish a variant's evidence without touching stable reports for candidates.

    A pgo build keeps its own variant file, so the ordinary build's figures,
    which docs/BUDGET.md pins, survive it. build.json is always the image on
    disc, whichever kind built it."""
    from paths import build_report_path
    payload=json.dumps(report,indent=2)
    build_report_path(telemetry,candidate,pgo).write_text(payload)
    if latest and not candidate:
        (ROOT/'.hkpsx/build.json').write_text(payload)

def memory_budget(link_map,exe,arena_bytes,coverage_bytes=0):
    """Bind static RAM allocation to the exact final map and EXE header."""
    text=link_map.read_text()
    def section(name):
        m=re.search(r'^([0-9a-f]+)\s+[0-9a-f]+\s+([0-9a-f]+)\s+\d+\s+'+re.escape(name)+r'$',text,re.M)
        if not m:raise ValueError('Missing final map section: '+name)
        return int(m[1],16),int(m[2],16)
    code,code_bytes=section('.text');data,data_bytes=section('.data');bss,bss_bytes=section('.bss')
    reserve=re.search(r'STACK_RESERVE = (0x[0-9a-fA-F]+)',text)
    if not reserve:raise ValueError('Cannot verify linker stack reservation')
    stack_bytes=int(reserve[1],16);initial_sp=int.from_bytes(exe.read_bytes()[0x30:0x34],'little')
    static_end=bss+bss_bytes;stack_floor=initial_sp-stack_bytes
    gap=stack_floor-static_end
    if gap<0:raise ValueError('Static RAM overlaps reserved stack')
    # The room-module pool (host/code_modules.py) is carved from the top of
    # that gap, just below the stack: what is free is the rest.
    pool=re.search(r'POOL_BYTES = (0x[0-9a-fA-F]+)',text)
    pool_bytes=int(pool[1],16) if pool else 0
    if pool_bytes>gap:raise ValueError('Static RAM overlaps the module pool')
    from stack_budget import main_frame_bytes
    main_frame=main_frame_bytes(exe,text)
    if main_frame>=stack_bytes:raise ValueError('main frame alone exceeds stack reservation')
    arenas=re.search(r'^([0-9a-f]+)\s+[0-9a-f]+\s+([0-9a-f]+)\s+1\s+hk_psx::disc::BUFFERS$',text,re.M)
    if not arenas:raise ValueError('Cannot verify linked room arenas')
    arena_start,arena_total=int(arenas[1],16),int(arenas[2],16)
    if arena_bytes<=0 or not arena_total or arena_total%arena_bytes or not bss<=arena_start<arena_start+arena_total<=static_end:
        raise ValueError('Invalid linked room arena allocation')
    if coverage_bytes:
        proof_start,proof_size=section('hk_psx::disc::COVERAGE_BUFFER')
        if proof_size!=coverage_bytes or not bss<=proof_start<proof_start+proof_size<=static_end:
            raise ValueError('Invalid linked coverage arena allocation')
        if proof_start<arena_start+arena_total and proof_start+proof_size>arena_start:
            raise ValueError('Scene and coverage arenas overlap')
    # The largest function against what a MIPS PC16 branch reaches. A branch
    # inside one function spans at most +/-128 KB, and the assembler only
    # refuses when a specific branch pair actually spans too far, so being over
    # is necessary for the failure and not sufficient. When it does refuse it
    # says "out of range PC16 fixup" and names nothing, which cost two builds to
    # diagnose, so the number and its margin are reported every build.
    #
    # Deliberately not `main`. It was `main` until that 615-line frame loop was
    # split into game/src/frame.rs, and a check pinned to one name would have
    # gone on reporting a healthy 30 KB while frame::simulate grew unwatched:
    # the hazard moves to whichever function is biggest.
    #
    # Columns are VMA, LMA, Size, Align, so the size is the third. Symbols are
    # kept to the code span, because a BSS array dwarfs any function and says
    # nothing about branch range.
    functions = []
    for vma, _lma, size, _align, name in re.findall(
            r'^([0-9a-f]+)\s+([0-9a-f]+)\s+([0-9a-f]+)\s+(\d+)\s+(\S+)$', text, re.M):
        at, span = int(vma, 16), int(size, 16)
        # Skip the section rows (`.text`) and the object-file rows; only
        # named symbols are functions.
        if span and code <= at < data and not name.startswith(('/', '.')):
            functions.append((span, name))
    biggest_size, biggest_name = max(functions, default=(None, None))
    pc16 = 1 << 17
    return {'largest_text_bytes':biggest_size,'largest_text_symbol':biggest_name,
            'pc16_branch_range_bytes':pc16,
            'pc16_margin_bytes':None if biggest_size is None else pc16-biggest_size,
            'largest_over_pc16_range':bool(biggest_size and biggest_size > pc16),
            'code_bytes':code_bytes,'data_bytes':data_bytes,'bss_bytes':bss_bytes,
            'code_data_alignment_bytes':data-code-code_bytes,'static_span_bytes':static_end-code,
            'static_end':hex(static_end),'stack_reserved_bytes':stack_bytes,'stack_floor':hex(stack_floor),
            'unallocated_before_stack_bytes':gap,'module_pool_bytes':pool_bytes,
            'free_below_pool_bytes':gap-pool_bytes,'main_frame_bytes':main_frame,
            'reserved_bytes_beyond_main_frame':stack_bytes-main_frame,
            'scene_arena_bytes':arena_bytes,'scene_arena_count':arena_total//arena_bytes,
            'scene_arenas_total_bytes':arena_total,'coverage_arena_bytes':coverage_bytes,
            'limitations':'Linked reservations only; no main-stack high-water or hardware timing claim.'}


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--telemetry',action='store_true')
    p.add_argument('--pgo',action='store_true',help='the guest was built by `pgo`: write build-pgo-*.json, not build-normal.json')
    args=p.parse_args()
    telemetry=args.telemetry
    from paths import link_map_path,hazard_report_path,artifacts
    outputs=artifacts(telemetry)
    report={'candidate':False,'build_kind':'pgo' if args.pgo else 'plain','delivery_policy':'Single canonical disc',
            'sdk':json.load(open(ROOT/'sdk.lock.json')),'telemetry':telemetry,
            'artifacts':{k:str(f) for k,f in outputs.items()},
            'outputs':{str(f):{'bytes':f.stat().st_size,'sha256':sha(f)} for f in outputs.values()},
            'room':json.load(open(ROOT/'.hkpsx/regions-provenance.json')),
            'packed_scenes':json.load(open(ROOT/'.hkpsx/packed-scenes.json')),
            'opaque_tiles':json.load(open(ROOT/'.hkpsx/opaque-tiles/report.json')),
            'opaque_groups':json.load(open(ROOT/'.hkpsx/opaque-groups/report.json')),
            'scene_coverage':json.load(open(ROOT/'.hkpsx/scene-certificates.json')),
            'audio':json.load(open(ROOT/'.hkpsx/audio-provenance.json')),
            'hud':json.load(open(ROOT/'.hkpsx/hud-provenance.json')),
            'geo':json.load(open(ROOT/'.hkpsx/geo-provenance.json')),
            'geo_audio':json.load(open(ROOT/'.hkpsx/geo-audio-provenance.json')),
            'lifeblood':json.load(open(ROOT/'.hkpsx/lifeblood-provenance.json')),
            'great_door':json.load(open(ROOT/'.hkpsx/great-door.json')),
            'battle_gates':json.load(open(ROOT/'.hkpsx/battle-gates.json')),
            'break_effects':json.load(open(ROOT/'.hkpsx/break-effects/report.json')),
            'menu':json.load(open(ROOT/'.hkpsx/menu-provenance.json')),
            'hazards':json.load(open(hazard_report_path(telemetry)))}
    link_map=link_map_path(telemetry)
    report['link_map']={'path':str(link_map),'sha256':sha(link_map)}
    report['memory']=memory_budget(link_map,outputs['exe'],report['packed_scenes']['scene_arena_bytes'],report['scene_coverage']['arena_bytes'])
    report['ambience']=json.load(open(ROOT/'.hkpsx/ambience.json'))
    report['focus_audio']=json.load(open(ROOT/'.hkpsx/focus-audio.json'))
    report['disc_chunk_count']=len(report['packed_scenes']['scenes'])+len(report['ambience']['clips'])+len(report['packed_scenes']['atlases'])+1+len(report['scene_coverage']['bundles'])+len(report['packed_scenes'].get('world_metadata',[]))
    write_build_report(report,telemetry,latest=True,pgo=args.pgo)
    shutil.copy2(link_map,ROOT/'build/hk-psx.map')
    memory=report['memory']
    if memory.get('largest_over_pc16_range'):
        # Said out loud every build, because the failure mode when it finally
        # bites is an assembler error that names nothing. Several callees carry
        # inline(never) purely to hold this down; adding a feature to `main`
        # means pinning another one, or splitting the function properly.
        print(f"WARNING: {memory['largest_text_symbol']} is {memory['largest_text_bytes']:,} bytes, past the "
              f"{memory['pc16_branch_range_bytes']:,} a MIPS PC16 branch reaches. It links only "
              'while no branch pair inside it spans too far.',flush=True)
    print('Build report:',build_report_path(telemetry,pgo=args.pgo),flush=True)

if __name__=='__main__':main()
