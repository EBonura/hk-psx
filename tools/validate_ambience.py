#!/usr/bin/env python3
"""Verify final-disc ambient residency, sustained output and corrupt-bank gating."""
import argparse
import array
import csv
import hashlib
import json
import math
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import wave
from validate import poll_tape

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'host'))
from paths import DISC_LIBRARY


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--build-report',type=Path,default=ROOT/'.hkpsx/build-normal.json')
    p.add_argument('--emulator',type=Path,default=ROOT.parent/'PSoXide-emulator/target/release/frontend')
    p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();build=json.loads(a.build_report.read_text());out=a.output.resolve()
    out.mkdir(parents=True,exist_ok=False)
    link=Path(build['link_map']['path']);map_text=link.read_text();clips=build['ambience']['clips']
    inputs={path:meta['sha256'] for path,meta in build['outputs'].items()}
    inputs[str(link)]=build['link_map']['sha256']
    inputs[str(a.emulator.resolve())]=sha(a.emulator)
    for clip in clips:inputs[clip['path']]=clip['sha256']
    for path,digest in inputs.items():
        if sha(path)!=digest:raise ValueError('Input changed: '+path)
    names=('HK_AMBIENCE_READY','HK_AMBIENCE_LOADED_MASK','HK_AMBIENCE_PLAYING_MASK',
           'HK_AMBIENCE_START_COUNT','HK_AMBIENCE_TRANSITIONS','HK_AMBIENCE_SCENE',
           'HK_AMBIENCE_FADE_TICKS','HK_GAME_MODE','HK_ROOM_LOAD_ERROR','HK_REGION_LOADS',
           'HK_CD_SECTORS_READ','HK_BOUNDARY_WAIT_TICKS','HK_PAD_POLL_MAX_VBLANK_GAP',
           '__psx_rt_fault_count')
    addresses={}
    for name in names+('HK_AMBIENCE_GAINS',):
        m=re.search(r'^([0-9a-f]+)\s+.*\s'+name+r'$',map_text,re.M)
        if not m:raise ValueError('Missing marker: '+name)
        addresses[name]=int(m[1],16)
    def state(path):
        ram=path.read_bytes()
        def word(name,index=0):
            offset=addresses[name]-0x80000000+index*4
            return struct.unpack_from('<I',ram,offset)[0]
        return dict({name:word(name) for name in names},gains=[word('HK_AMBIENCE_GAINS',i) for i in range(6)])
    commands=[]
    def launch(label,cue,polls,audio=False):
        tape=out/(label+'.pxtape');poll_tape(tape,'9:start:1',polls+1)
        command=[str(a.emulator.resolve()),'launch','--path',str(cue),'--embedded-playtest',
                 '--config-dir',str(out/'emulator'),'--steps','6000000000','--input-tape',str(tape),
                 '--stop-at-poll',str(polls),'--dump-ram',str(out/(label+'-ram.bin')),
                 '--dump-spu-ram',str(out/(label+'-spu.bin')),'--dump-display',str(out/(label+'.ppm'))]
        if audio:
            command+=['--dump-audio',str(out/(label+'.wav')),'--route-log',str(out/(label+'-route.csv'))]
            for name in names:command+=['--route-watch-u32',hex(addresses[name])]
        commands.append(command)
        with (out/(label+'.log')).open('w') as log:
            subprocess.run(command,cwd=ROOT,stdout=log,stderr=subprocess.STDOUT,check=True)
        log=(out/(label+'.log')).read_text()
        stop=re.search(r'route-ticks=(\d+)\s+port1-polls=(\d+)',log)
        if not stop or int(stop[2])<polls:raise ValueError('Replay ended before requested input poll')
        return state(out/(label+'-ram.bin'))
    # More than two complete iterations of the longest Cave loop at actual SPU pitch.
    longest=max(c['byte_len']//16*28/(c['pitch']*44100/4096) for c in clips)
    polls=max(5400,math.ceil(longest*2*60)+120)
    normal=launch('loops',build['artifacts']['cue'],polls,True)
    if any(normal[n] for n in ('HK_ROOM_LOAD_ERROR','HK_BOUNDARY_WAIT_TICKS','__psx_rt_fault_count')):
        raise ValueError('Fault or loading hold during ambience playback: '+str(normal))
    expected={'HK_AMBIENCE_READY':1,'HK_AMBIENCE_LOADED_MASK':63,'HK_AMBIENCE_PLAYING_MASK':57,
              'HK_AMBIENCE_START_COUNT':4,'HK_AMBIENCE_TRANSITIONS':1,'HK_AMBIENCE_SCENE':0,
              'HK_AMBIENCE_FADE_TICKS':30,'HK_GAME_MODE':1,'HK_PAD_POLL_MAX_VBLANK_GAP':1}
    if any(normal[k]!=v for k,v in expected.items()):raise ValueError('Ambience lifecycle mismatch: '+str(normal))
    expected_gains=[g if 57&(1<<i) else 0 for i,g in enumerate(build['ambience']['cues'][0]['gains'])]
    if normal['gains']!=expected_gains:raise ValueError('Source mixer endpoint mismatch')
    spu=(out/'loops-spu.bin').read_bytes()
    for clip in clips:
        start=clip['spu_address']
        if spu[start:start+clip['byte_len']]!=Path(clip['path']).read_bytes():
            raise ValueError('Actual SPU payload mismatch: '+clip['name'])
    # Ongoing output after both long-loop boundaries must remain audible data.
    with wave.open(str(out/'loops.wav'),'rb') as wav:
        if (wav.getnchannels(),wav.getsampwidth(),wav.getframerate())!=(2,2,44100):
            raise ValueError('Unexpected emulator audio format')
        pcm=array.array('h');pcm.frombytes(wav.readframes(wav.getnframes()))
    if sys.byteorder!='little':pcm.byteswap()
    seconds=len(pcm)//88200
    rms=[math.sqrt(sum(v*v for v in pcm[s*88200:(s+1)*88200])/88200) for s in range(seconds)]
    if seconds<longest*2 or not rms or min(rms[-5:])==0:
        raise ValueError('Ambient output stopped before repeated loop playback')
    with (out/'loops-route.csv').open() as source:rows=list(csv.DictReader(source))
    for row in rows:
        if int(row['ram_'+f'{addresses["HK_GAME_MODE"]:08x}'])==1:
            if int(row['ram_'+f'{addresses["HK_AMBIENCE_START_COUNT"]:08x}'])!=4:
                raise ValueError('Cave loops were restarted during stationary gameplay')
    # Corrupt the LAST clip: even five uploaded clips must never start a partial bank.
    with tempfile.TemporaryDirectory(prefix='.hk-psx-bad-ambience-',dir=DISC_LIBRARY) as temp:
        bad=Path(temp)/'hk-psx-bad-ambience.bin';shutil.copyfile(build['artifacts']['bin'],bad)
        count=build['disc_chunk_count'];header_sectors=(28+count*24+2047)//2048
        table=bytearray()
        with bad.open('r+b') as disc:
            for i in range(header_sectors):
                disc.seek((1024+i)*2352+24);table+=disc.read(2048)
            sector=struct.unpack_from('<I',table,28+(count-1)*24+4)[0]
            offset=(1024+sector)*2352+24+128;disc.seek(offset);byte=disc.read(1)
            disc.seek(offset);disc.write(bytes([byte[0]^1]))
        cue=bad.with_suffix('.cue');cue.write_text('FILE "hk-psx-bad-ambience.bin" BINARY\n  TRACK 01 MODE2/2352\n    INDEX 01 00:00:00\n')
        corrupt=launch('corrupt',cue,100)
    for key,value in {'HK_GAME_MODE':3,'HK_ROOM_LOAD_ERROR':9,'HK_AMBIENCE_READY':0,
                      'HK_AMBIENCE_LOADED_MASK':31,'HK_AMBIENCE_PLAYING_MASK':0,
                      'HK_AMBIENCE_START_COUNT':0,'HK_REGION_LOADS':0,'__psx_rt_fault_count':0}.items():
        if corrupt[key]!=value:raise ValueError('Corrupt ambience was published: '+str(corrupt))
    for path,digest in inputs.items():
        if sha(path)!=digest:raise ValueError('Input changed during validation: '+path)
    report={'inputs':inputs,'commands':commands,'normal':normal,'corrupt':corrupt,
            'spu_verified_bytes':sum(c['byte_len'] for c in clips),'audio_seconds':seconds,
            'audio_peak':max(abs(v) for v in pcm),'clipped_samples':sum(abs(v)>=32767 for v in pcm),
            'audio_rms_per_second':rms,'longest_loop_seconds':longest,'hardware_validated':False,
            'limitations':'Actual emulator output and SPU residency; no physical-console, stereo-fidelity or click-free-loop claim. Surface transition lifecycle has native tests; natural Cave-to-Town traversal remains unverified.'}
    (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    print('Verified',report['spu_verified_bytes'],'SPU bytes,',seconds,'seconds of loop output and corrupt-bank rejection')


if __name__=='__main__':main()
