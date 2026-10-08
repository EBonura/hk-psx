#!/usr/bin/env python3
"""Check earned-SOUL healing and capture the actual final CUE in both renderers."""
import argparse,hashlib,json,re,subprocess
from pathlib import Path
from validate import poll_tape,ENEMY_ROUTE,EARNED_COMBAT_ROUTE
ROOT=Path(__file__).resolve().parents[1]
FOCUS_ROUTE=(ENEMY_ROUTE+',1200:right:335,1270:cross:24,'+
             ','.join(f'{n}:square:2' for n in range(1208,2050,26))+
             ',1550:left:40,2110:circle:200')
# Preserve FOCUS_ROUTE for historical streaming comparisons. This route stops
# in the third Crawler's collision window to earn the second kill before Focus.
EARNED_FOCUS_ROUTE=(EARNED_COMBAT_ROUTE+',1200:right:200,1270:cross:24,'+
                    ','.join(f'{n}:square:2' for n in range(1208,2050,26))+
                    ',2110:circle:200')
def sha(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()
def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--emulator',type=Path,default=ROOT.parent/'PSoXide-emulator/target/release/frontend')
    p.add_argument('--build-report',type=Path,default=ROOT/'.hkpsx/build.json')
    p.add_argument('--output',type=Path,default=ROOT/'captures/focus-final')
    a=p.parse_args();out=a.output.resolve();out.mkdir(parents=True,exist_ok=True)
    build=json.loads(a.build_report.read_text());emulator=a.emulator.resolve()
    for path,meta in build['outputs'].items():
        if sha(path)!=meta['sha256']:raise ValueError('Build artifact changed: '+path)
    link=Path(build['link_map']['path'])
    if sha(link)!=build['link_map']['sha256']:raise ValueError('Build map changed')
    map_text=link.read_text()
    def word(raw,name):
        m=re.search(r'^([0-9a-f]+)\s+.*\s'+re.escape(name)+r'$',map_text,re.M)
        if not m:raise ValueError('Missing guest marker: '+name)
        at=int(m[1],16)-0x80000000
        if not 0<=at<=len(raw)-4:raise ValueError('Guest marker outside RAM')
        return int.from_bytes(raw[at:at+4],'little')
    tape=out/'input.pxtape';poll_tape(tape,EARNED_FOCUS_ROUTE,2400)
    names=('HK_HEALTH','HK_SOUL','HK_ENEMY_HITS','HK_ENEMY_KILLS','HK_FOCUS_STARTED',
           'HK_FOCUS_COMPLETED','HK_FOCUS_HEALED','HK_FOCUS_DRAINED','HK_FOCUS_REFUNDED',
           'HK_FOCUS_LOCKED','HK_GAME_MODE','__psx_rt_fault_count','HK_ROOM_LOAD_ERROR')
    captures={};commands=[]
    for label,poll in [('charging',2160),('healed',2400)]:
        cmd=[str(emulator),'launch','--path',build['artifacts']['cue'],'--embedded-playtest',
             '--config-dir',str(out/'emulator'),'--steps','6000000000','--input-tape',str(tape),
             '--stop-at-poll',str(poll),'--dump-display',str(out/(label+'.ppm')),
             '--dump-hw',str(out/(label+'-hardware.ppm')),'--dump-ram',str(out/(label+'-ram.bin'))]
        commands.append(cmd)
        with (out/(label+'.log')).open('w') as log:subprocess.run(cmd,cwd=ROOT,stdout=log,stderr=subprocess.STDOUT,check=True)
        raw=(out/(label+'-ram.bin')).read_bytes();state={name:word(raw,name) for name in names}
        if state['HK_GAME_MODE']!=1 or state['__psx_rt_fault_count'] or state['HK_ROOM_LOAD_ERROR']:
            raise ValueError('Invalid gameplay state: '+str(state))
        if state['HK_ENEMY_HITS']!=4 or state['HK_ENEMY_KILLS']!=2 or state['HK_FOCUS_STARTED']!=1:
            raise ValueError('Route did not earn source SOUL through two kills: '+str(state))
        if label=='charging':
            if not (state['HK_HEALTH']==4 and 11<state['HK_SOUL']<44 and state['HK_FOCUS_LOCKED']==1
                    and state['HK_FOCUS_COMPLETED']==0 and state['HK_FOCUS_DRAINED']==44-state['HK_SOUL']):
                raise ValueError('Progressive charge did not retain missing health: '+str(state))
        elif not (state['HK_HEALTH']==5 and state['HK_SOUL']==11 and state['HK_FOCUS_DRAINED']==33
                  and state['HK_FOCUS_COMPLETED']==1 and state['HK_FOCUS_HEALED']==1
                  and state['HK_FOCUS_LOCKED']==0 and state['HK_FOCUS_REFUNDED']==0):
            raise ValueError('Focus did not heal exactly one mask for33 SOUL: '+str(state))
        captures[label]=state
    report={'build_report':str(a.build_report.resolve()),'artifacts':build['outputs'],'link_map':build['link_map'],
            'emulator':str(emulator),'emulator_sha256':sha(emulator),'tape_sha256':sha(tape),
            'source':'Unmodified final game; SOUL earned through four successful nail hits, no state injection.',
            'states':captures,'commands':commands,'hardware_validated':False}
    (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(captures,indent=2))
if __name__=='__main__':main()
