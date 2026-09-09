#!/usr/bin/env python3
"""Reproducible local Windows-source cook, guest build and BIN/CUE packing."""
import argparse,hashlib,json,os,subprocess,sys,tarfile,io,shutil,re
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'host'))

def run(cmd,**kw):
    print('+',' '.join(map(str,cmd)),flush=True)
    subprocess.run(list(map(str,cmd)),check=True,**kw)
def sha(p):
    with Path(p).open('rb') as stream:return hashlib.file_digest(stream,'sha256').hexdigest()
def write_build_report(report,telemetry,candidate=False,latest=False):
    """Publish a variant's evidence without touching stable reports for candidates."""
    from paths import build_report_path
    payload=json.dumps(report,indent=2)
    build_report_path(telemetry,candidate).write_text(payload)
    if latest and not candidate:
        (ROOT/'.hkpsx/build.json').write_text(payload)

def memory_budget(link_map,exe,arena_bytes):
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
    arenas=re.search(r'^([0-9a-f]+)\s+[0-9a-f]+\s+([0-9a-f]+)\s+1\s+hk_psx::disc::BUFFERS$',text,re.M)
    if not arenas:raise ValueError('Cannot verify linked room arenas')
    arena_start,arena_total=int(arenas[1],16),int(arenas[2],16)
    if arena_bytes<=0 or not arena_total or arena_total%arena_bytes or not bss<=arena_start<arena_start+arena_total<=static_end:
        raise ValueError('Invalid linked room arena allocation')
    return {'code_bytes':code_bytes,'data_bytes':data_bytes,'bss_bytes':bss_bytes,
            'code_data_alignment_bytes':data-code-code_bytes,'static_span_bytes':static_end-code,
            'static_end':hex(static_end),'stack_reserved_bytes':stack_bytes,'stack_floor':hex(stack_floor),
            'unallocated_before_stack_bytes':gap,'scene_arena_bytes':arena_bytes,'scene_arena_count':arena_total//arena_bytes,
            'scene_arenas_total_bytes':arena_total,
            'limitations':'Linked reservations only; no main-stack high-water or hardware timing claim.'}

def sdk(source):
    pin=json.loads((ROOT/'sdk.lock.json').read_text());revision=pin['revision'];destination=ROOT/'.psoxide'
    state=ROOT/'.hkpsx/sdk.json'
    if state.exists():
        record=json.loads(state.read_text())
        if record['revision']==revision and all((destination/p).is_file() and sha(destination/p)==digest for p,digest in record['files'].items()):return
        raise RuntimeError('Pinned SDK cache changed. Move .psoxide and .hkpsx/sdk.json aside before rehydrating.')
    source=Path(source).resolve()
    # git archive selects committed bytes even if a sibling has local changes.
    raw=subprocess.check_output(['git','-C',str(source),'archive',revision])
    destination.mkdir(exist_ok=True)
    with tarfile.open(fileobj=io.BytesIO(raw)) as archive:
        archive.extractall(destination,filter='data')
        members=[m.name for m in archive.getmembers() if m.isfile()]
    state.write_text(json.dumps({'revision':revision,'files':{p:sha(destination/p) for p in members}},indent=2))

def cook_cached(force=False):
    provenance=ROOT/'.hkpsx/regions-provenance.json';cache=ROOT/'.hkpsx/regions-cook-cache.json'
    code={str(p.relative_to(ROOT)):sha(p) for p in [ROOT/'host'/name for name in ('cook.py','regions.py','actors.py','particles.py','effects.py','breakables.py','combat.py','scene.py','source.py','world.py','flat_support.py','reveal_masks.py','texture_dedup.py','similarity_dedup.py','constant_textures.py','region_delta.py','quality.py','focus.py','alpha_covers.py','materials.py','tilemap_fill.py','break_effects.py')]}
    for name in ('audit_texture_similarity.py','texture_match_groups.py','audit_resident_bank.py'):
        code['tools/'+name]=sha(ROOT/'tools'/name)
    code['requirements']=sha(ROOT/'host/requirements.lock')
    current=json.load(open(ROOT/'.hkpsx/doctor.json'))['installs'][0]['data_directory']
    valid=False
    if not force and provenance.exists() and cache.exists():
        prov=json.load(provenance.open());old=json.load(cache.open())
        valid=prov['source']==current and old['code']==code
        if valid:
            valid=all((Path(current)/p).is_file() and sha(Path(current)/p)==meta['sha256'] for p,meta in prov['inputs'].items())
            valid=valid and all((ROOT/p).is_file() and sha(ROOT/p)==h for p,h in old['outputs'].items())
    if valid:print('Cook cache hit: source hashes and generated outputs match.',flush=True);return
    run([ROOT/'.venv/bin/python',ROOT/'host/regions.py'],cwd=ROOT)
    outputs={str(p.relative_to(ROOT)):sha(p) for p in [ROOT/'data/room.hk',ROOT/'data/params.rs',ROOT/'data/regions.json',ROOT/'data/pogo.rs']+list((ROOT/'data/regions').glob('chunk_*.hk'))}
    cache.write_text(json.dumps({'code':code,'outputs':outputs},indent=2))

def menu_cached(force=False):
    provenance=ROOT/'.hkpsx/menu-provenance.json';cache=ROOT/'.hkpsx/menu-cache.json'
    code={name:sha(ROOT/'host'/name) for name in ('cook_menu.py','source.py','requirements.lock')}
    current=json.load(open(ROOT/'.hkpsx/doctor.json'))['installs'][0]['data_directory']
    if not force and provenance.exists() and cache.exists():
        prov=json.loads(provenance.read_text());old=json.loads(cache.read_text())
        output=prov['output'];path=ROOT/output['path']
        if (old['code']==code and prov['source']==current
            and path.is_file() and sha(path)==output['sha256']
            and all((Path(current)/p).is_file() and sha(Path(current)/p)==m['sha256']
                    for p,m in prov['inputs'].items())):
            print('Menu cache hit: source hashes and generated output match.',flush=True)
            return
    run([ROOT/'.venv/bin/python',ROOT/'host/cook_menu.py'],cwd=ROOT)
    cache.write_text(json.dumps({'code':code},indent=2))

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--hollow-knight');p.add_argument('--sdk-source',default=str(ROOT.parent/'PSoXide-sdk'));p.add_argument('--telemetry',action='store_true');p.add_argument('--candidate',action='store_true',help='Deprecated alias for a normal build; always updates the single hk-psx.cue');p.add_argument('--recook',action='store_true');args=p.parse_args()
    os.chdir(ROOT);(ROOT/'.hkpsx').mkdir(exist_ok=True)
    cmd=[sys.executable,ROOT/'tools/doctor.py']
    if args.hollow_knight:cmd+=['--hollow-knight',args.hollow_knight]
    run(cmd)
    venv=ROOT/'.venv/bin/python';req=ROOT/'host/requirements.lock';stamp=ROOT/'.hkpsx/python-lock.sha256'
    if not venv.exists():run([sys.executable,'-m','venv',ROOT/'.venv'])
    if not stamp.exists() or stamp.read_text()!=sha(req):
        run([venv,'-m','pip','install','--requirement',req]);stamp.write_text(sha(req))
    sdk(args.sdk_source);cook_cached(args.recook);menu_cached(args.recook)
    for script in ('cook_audio.py','cook_hud.py','ambience.py','read_points.py','geo.py','geo_audio.py','lifeblood.py','break_effects.py'):
        run([venv,ROOT/'host'/script],cwd=ROOT)
    run(['cargo','run','--quiet','--manifest-path',ROOT/'shared/hk-format/Cargo.toml','--example','check','--',ROOT/'data/room.hk'])
    from build_guest import build
    from paths import link_map_path,hazard_report_path
    # One requested variant, one playable disc. Replays report quality failures
    # independently; they must not leave an older build behind the main name.
    if args.candidate:
        print('--candidate is deprecated; updating the single hk-psx.cue.',flush=True)
    telemetry=args.telemetry
    outputs=build(telemetry,candidate=False)
    report={'candidate':False,'delivery_policy':'Single canonical disc',
            'sdk':json.load(open(ROOT/'sdk.lock.json')),'telemetry':telemetry,
            'artifacts':{k:str(f) for k,f in outputs.items()},
            'outputs':{str(f):{'bytes':f.stat().st_size,'sha256':sha(f)} for f in outputs.values()},
            'room':json.load(open(ROOT/'.hkpsx/regions-provenance.json')),
            'packed_scenes':json.load(open(ROOT/'.hkpsx/packed-scenes.json')),
            'audio':json.load(open(ROOT/'.hkpsx/audio-provenance.json')),
            'hud':json.load(open(ROOT/'.hkpsx/hud-provenance.json')),
            'geo':json.load(open(ROOT/'.hkpsx/geo-provenance.json')),
            'geo_audio':json.load(open(ROOT/'.hkpsx/geo-audio-provenance.json')),
            'lifeblood':json.load(open(ROOT/'.hkpsx/lifeblood-provenance.json')),
            'break_effects':json.load(open(ROOT/'.hkpsx/break-effects/report.json')),
            'menu':json.load(open(ROOT/'.hkpsx/menu-provenance.json')),
            'hazards':json.load(open(hazard_report_path(telemetry)))}
    link_map=link_map_path(telemetry)
    report['link_map']={'path':str(link_map),'sha256':sha(link_map)}
    report['memory']=memory_budget(link_map,outputs['exe'],report['packed_scenes']['scene_arena_bytes'])
    report['ambience']=json.load(open(ROOT/'.hkpsx/ambience.json'))
    report['disc_chunk_count']=len(report['packed_scenes']['scenes'])+len(report['ambience']['clips'])
    write_build_report(report,telemetry,latest=True)
    shutil.copy2(link_map,ROOT/'build/hk-psx.map')
    print('Updated:',outputs['cue'],flush=True)
    print('Run traversal and gameplay validation separately; building does not claim they pass.',flush=True)
if __name__=='__main__':main()
