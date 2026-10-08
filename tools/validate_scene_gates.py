#!/usr/bin/env python3
"""Verify a completed scene-gate replay against its exact resident source bank.

Consumes tools/replay_cue.py output; never builds, launches, or patches RAM.
"""
import argparse,csv,hashlib,json,re
from pathlib import Path

ROOT=Path(__file__).resolve().parents[1]
# The queue's own capacity, from the module that enforces it. Written here as a
# bare 16 with a `>` test, which no peak can satisfy: the queue never holds
# more than CAPACITY (a full one drops its oldest sample, counted in
# HK_INPUT_DROPPED_SAMPLES), so the gate below never fired.
INPUT_QUEUE_CAPACITY=int(re.search(r'pub const CAPACITY: usize = (\d+);',
                                   (ROOT/'game/src/input_queue.rs').read_text()).group(1))
def source_path(value):
    p=Path(value);return p if p.is_absolute() else ROOT/p

def verify_coverage(build,state,scene,map_text,ram):
    """Check the replaceable proof bank against this replay's bound build."""
    if 'scene_coverage' not in build:return None # Historical builds predate this bank.
    coverage=build['scene_coverage']
    owners=[b for b in coverage['bundles']if b['scene_id']==scene['scene_id']]
    if len(owners)!=1:raise ValueError('Unmapped coverage owner')
    owner=owners[0]
    if (state.get('HK_COVERAGE_SCENE')!=owner['scene_index']+1 or
        state.get('HK_COVERAGE_BYTES')!=owner['raw_len'] or
        state.get('HK_COVERAGE_LOADS')!=state['HK_SCENE_LOADS']):
        raise ValueError('Coverage owner, length or admission count mismatch')
    raw=source_path(owner['raw_path']).read_bytes()
    if len(raw)!=owner['raw_len'] or hashlib.sha256(raw).hexdigest()!=owner['raw_sha256']:
        raise ValueError('Coverage source changed')
    m=re.search(r'^([0-9a-f]+)\s+[0-9a-f]+\s+([0-9a-f]+)\s+1\s+hk_psx::disc::COVERAGE_BUFFER$',map_text,re.M)
    if not m or int(m[2],16)!=coverage['arena_bytes'] or len(raw)>coverage['arena_bytes']:
        raise ValueError('Unverified coverage allocation')
    address=int(m[1],16)&0x1fffff
    if ram[address:address+len(raw)]!=raw:raise ValueError('Resident coverage differs from cooked bytes')
    return len(raw)

def verify(directory,minimum_loads=1,town_x=None):
    directory=Path(directory)
    command=json.loads((directory/'command.json').read_text())
    build=json.loads((directory/'build-report.json').read_text())
    if not command.get('completed') or command.get('exit_code')!=0 or command.get('faults') or not command.get('inputs_unchanged'):
        raise ValueError('Replay incomplete, changed, or faulted')
    pack=build['packed_scenes']
    if pack.get('residency')!='scene_gate':raise ValueError('Not exclusive scene residency')
    state=command['final_ram'];chunk=state['HK_REGION_ID']
    if state['HK_GAME_MODE']!=1:raise ValueError('Replay ended before gameplay resumed')
    candidates=[s for s in pack['scenes']if chunk in [r['chunk_id']for r in s['source_rooms']]]
    if len(candidates)!=1:raise ValueError('Unmapped final region')
    scene=candidates[0];path=source_path(scene['resident_raw_path']);raw=path.read_bytes()
    if hashlib.sha256(raw).hexdigest()!=scene['resident_raw_sha256']:raise ValueError('Source scene changed')
    map_text=(directory/'game.map').read_text()
    m=re.search(r'^([0-9a-f]+)\s+[0-9a-f]+\s+([0-9a-f]+)\s+1\s+hk_psx::disc::BUFFERS$',map_text,re.M)
    if not m or int(m[2],16)!=pack['arena_bytes']:raise ValueError('Unverified arena allocation')
    address=(int(m[1],16)&0x1fffff)+scene['ram_offset'];ram=(directory/'ram.bin').read_bytes()
    if ram[address:address+len(raw)]!=raw:raise ValueError('Final resident scene differs from cooked bytes')
    coverage_bytes=verify_coverage(build,state,scene,map_text,ram)
    if state['HK_SCENE_GATE_LOADS']<minimum_loads:raise ValueError('Required scene replacements not observed')
    if state['HK_SCENE_LOADS']!=state['HK_SCENE_GATE_LOADS']+1:raise ValueError('Unexpected scene admission count')
    if state['HK_INPUT_LOADING_TICKS']==0 or state['HK_INPUT_LOADING_SAMPLES']==0:raise ValueError('Loading input service not observed')
    if state['HK_INPUT_QUEUE_PEAK']>=INPUT_QUEUE_CAPACITY:raise ValueError('Input queue reached capacity')
    rows=list(csv.DictReader((directory/'route.csv').open()))
    def value(row,name):return int(row['ram_'+command['watches'][name][2:]])
    regions={r['chunk_id']:s['scene_id']for s in pack['scenes']for r in s['source_rooms']}
    max_town_x=max((value(r,'HK_PLAYER_X')/65536 for r in rows
                    if value(r,'HK_GAME_MODE')==1 and regions.get(value(r,'HK_REGION_ID'))==1),default=None)
    if town_x is not None and (max_town_x is None or max_town_x<town_x):raise ValueError('Town traversal extent not observed')
    # Within a scene, changing spatial views must not trigger additional static
    # reads/uploads. Loading samples are explicitly excluded and separately counted.
    # Gameplay reads the disc on purpose now (area music refills, the room and
    # ambience clip prefetches), all counted in HK_CD_SECTORS_READ, so the
    # scene loader's own count is the one to hold still; builds from before it
    # existed only have the total.
    sectors='HK_SCENE_SECTORS_READ' if 'HK_SCENE_SECTORS_READ' in command['watches'] else 'HK_CD_SECTORS_READ'
    last=None;checked=0
    for row in rows:
        if value(row,'HK_GAME_MODE')!=1:last=None;continue
        snapshot=(value(row,'HK_SCENE_GATE_LOADS'),regions.get(value(row,'HK_REGION_ID')),
                  value(row,sectors),value(row,'HK_VRAM_UPLOAD_BYTES'))
        if coverage_bytes is not None:
            snapshot+=(value(row,'HK_COVERAGE_LOADS'),value(row,'HK_COVERAGE_SCENE'),value(row,'HK_COVERAGE_BYTES'))
        if last and snapshot[:2]==last[:2]:
            if snapshot[2:]!=last[2:]:raise ValueError('Static scene transfer during gameplay')
            checked+=1
        last=snapshot
    result={'status':'PASS','final_scene':scene['scene_id'],'resident_bytes_exact':len(raw),
            'coverage_bytes_exact':coverage_bytes,
            'gate_loads':state['HK_SCENE_GATE_LOADS'],'loading_ticks':state['HK_INPUT_LOADING_TICKS'],
            'loading_samples':state['HK_INPUT_LOADING_SAMPLES'],'queue_peak':state['HK_INPUT_QUEUE_PEAK'],
            'max_town_x':max_town_x,'stable_gameplay_intervals':checked,
            'limitations':'Emulator replay and memory evidence; no physical-console timing or original scripting parity.'}
    (directory/'scene-gate-validation.json').write_text(json.dumps(result,indent=2)+'\n')
    return result

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('replay',type=Path)
    p.add_argument('--minimum-loads',type=int,default=1);p.add_argument('--town-x',type=float)
    a=p.parse_args();print(json.dumps(verify(a.replay,a.minimum_loads,a.town_x),indent=2))
if __name__=='__main__':main()
