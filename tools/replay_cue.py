#!/usr/bin/env python3
"""Replay the single current CUE, with hash-bound input and gameplay diagnostics.

No build, disc copy, GUI or RAM patch. Captures both software and hardware views.
"""
import argparse,csv,hashlib,json,re,struct,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
WATCHES='''HK_PLAYER_X HK_PLAYER_Y HK_REGION_ID HK_GAME_MODE HK_HEALTH HK_SOUL
HK_ROOM_LOAD_ERROR HK_CD_STREAM_ERROR __psx_rt_fault_count HK_SCENERY_REPAIR_FAILURES
HK_BOUNDARY_WAIT_TICKS HK_BOUNDARY_WAIT_MAX HK_INPUT_FAULT HK_INPUT_MISSED_VBLANKS
HK_SCENE_LOADS HK_CD_SECTORS_READ HK_VRAM_UPLOAD_BYTES HK_REGION_ACTIVATIONS
HK_BREAK_COUNT HK_ENEMY_KILLS HK_DEBRIS_ACTIVE HK_DEBRIS_DRAWN HK_PARTICLES_SPAWNED
HK_PARTICLES_DROPPED HK_READ_SOURCE HK_READ_PAGE HK_READ_OPENED HK_READ_CLOSED
HK_PAUSED HK_PAUSE_CONTROLS HK_SFX_LEVEL HK_AMBIENCE_LEVEL HK_DEATHS
HK_MENU_PAGE HK_MENU_ROW HK_MENU_SFX HK_MENU_AMBIENCE
HK_GEO_WALLET HK_GEO_HITS HK_GEO_ROCKS_DEPLETED HK_GEO_SPAWNED_VALUE HK_GEO_COLLECTED_VALUE
HK_GEO_ACTIVE HK_GEO_PENDING_VALUE HK_GEO_LOST HK_GEO_DRAWN'''.split()
WATCHES+='HK_LIFEBLOOD_OPENED HK_LIFEBLOOD_ACTIVE HK_LIFEBLOOD_STRUCK HK_LIFEBLOOD_GRANTED HK_BLUE_HEALTH HK_GEO_AUDIO_READY HK_GEO_PICKUP_SFX HK_GEO_ROCK_HIT_SFX HK_GEO_ROCK_BREAK_SFX'.split()
WATCHES+='HK_AMBIENCE_READY HK_AMBIENCE_LOADED_MASK HK_AMBIENCE_PLAYING_MASK HK_AMBIENCE_START_COUNT HK_AMBIENCE_TRANSITIONS HK_AMBIENCE_SCENE HK_AMBIENCE_FADE_TICKS'.split()
WATCHES+='HK_GREAT_DOOR_HITS HK_GREAT_DOOR_STAGE HK_GREAT_DOOR_OPENED HK_GREAT_DOOR_WAIT HK_GREAT_DOOR_TRANSITIONS'.split()

def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--tape',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    p.add_argument('--screenshot-interval',type=int,default=60)
    p.add_argument('--frontend',type=Path,default=ROOT.parent/'PSoXide-emulator/target/release/frontend')
    a=p.parse_args();r=json.loads((ROOT/'.hkpsx/build.json').read_text())
    cue=Path(r['artifacts']['cue']);exe=Path(r['artifacts']['exe']);mp=Path(r['link_map']['path'])
    if cue.resolve()!=(Path.home()/'Downloads/ps1 games/hk-psx.cue').resolve():raise ValueError('Not the canonical disc')
    for path,info in r['outputs'].items():
        if digest(Path(path))!=info['sha256']:raise ValueError('Build artifact changed: '+path)
    if digest(mp)!=r['link_map']['sha256']:raise ValueError('Build map changed')
    tape=a.tape.resolve();data=tape.read_bytes()
    if data[:8]!=b'PXITAPE2':raise ValueError('Not a poll-bound tape')
    count,start=struct.unpack_from('<II',data,8)
    if len(data)!=16+count*6 or a.screenshot_interval<=0:raise ValueError('Invalid tape/capture interval')
    out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
    help_text=subprocess.check_output([str(a.frontend),'launch','--help'],text=True)
    for flag in ('--embedded-playtest','--stop-at-poll','--dump-hw','--route-watch-u32'):
        if flag not in help_text:raise ValueError('Unsupported frontend flag '+flag)
    (out/'frontend-help.txt').write_text(help_text)
    command=[str(a.frontend),'launch','--path',str(cue),'--embedded-playtest',
        '--config-dir',str(out/'emulator'),'--steps','6000000000','--input-tape',str(tape),
        '--stop-at-poll',str(start+count),'--route-screenshot-interval',str(a.screenshot_interval)]
    for flag,name in {'route-log':'route.csv','route-screenshot-dir':'screenshots','dump-display':'display.ppm',
        'dump-hw':'hw.ppm','dump-vram':'vram.ppm','dump-ram':'ram.bin','dump-audio':'audio.wav','cd-command-log':'cd.csv'}.items():
        command+=['--'+flag,str(out/name)]
    symbols={s[-1]:'0x'+s[0] for line in mp.read_text().splitlines()if len(s:=line.split())==5}
    watches={n:symbols[n] for n in WATCHES if n in symbols}
    for addr in watches.values():command+=['--route-watch-u32',addr]
    paths=[cue,Path(r['artifacts']['bin']),exe,mp,tape,a.frontend]
    def inputs():return {str(path):{'bytes':path.stat().st_size,'sha256':digest(path)}for path in paths}
    before=inputs();report={'command':command,'inputs':before,'watches':watches,'samples':count,'start_poll':start}
    (out/'command.json').write_text(json.dumps(report,indent=2)+'\n');(out/'build-report.json').write_text(json.dumps(r,indent=2)+'\n')
    (out/'game.map').write_bytes(mp.read_bytes())
    with (out/'replay.log').open('w')as log:result=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT)
    report['inputs_unchanged']=inputs()==before;report['exit_code']=result.returncode
    ram=(out/'ram.bin').read_bytes()
    report['final_ram']={n:struct.unpack_from('<I',ram,int(addr,16)&0x1fffff)[0]for n,addr in watches.items()}
    rows=list(csv.DictReader((out/'route.csv').open()))
    report['maximum_observed']={n:max([int(row['ram_'+addr[2:]])for row in rows]+[report['final_ram'][n]])for n,addr in watches.items()}
    report['stop']=re.findall(r'route-ticks=(\d+)\s+port1-polls=(\d+)',(out/'replay.log').read_text())
    report['completed']=bool(report['stop'])and int(report['stop'][-1][1])==start+count
    faults=[n for n in ('HK_ROOM_LOAD_ERROR','HK_CD_STREAM_ERROR','__psx_rt_fault_count','HK_SCENERY_REPAIR_FAILURES','HK_INPUT_FAULT','HK_INPUT_MISSED_VBLANKS')if report['maximum_observed'].get(n)!=0]
    report['faults']=faults
    (out/'command.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({k:report[k]for k in ('completed','exit_code','faults','final_ram')},indent=2))
    if result.returncode or not report['completed']or not report['inputs_unchanged']or faults:raise SystemExit(1)
if __name__=='__main__':main()
