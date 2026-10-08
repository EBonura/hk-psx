#!/usr/bin/env python3
"""Verify final-disc ambient residency, sustained output."""
import argparse
import array
import csv
import hashlib
import json
import math
from pathlib import Path
import re
import struct
import subprocess
import sys
import wave
from validate import poll_tape

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'host'))


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--build-report',type=Path,default=ROOT/'.hkpsx/build-normal.json')
    p.add_argument('--emulator',type=Path,default=ROOT.parent/'PSoXide-emulator/target/release/frontend')
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--verify-existing',action='store_true',help='Recheck an existing hash-bound capture without replaying')
    a=p.parse_args();build=json.loads(a.build_report.read_text());out=a.output.resolve()
    out.mkdir(parents=True,exist_ok=a.verify_existing)
    link=Path(build['link_map']['path']);map_text=link.read_text();clips=build['ambience']['clips']
    inputs={path:meta['sha256'] for path,meta in build['outputs'].items()}
    inputs[str(link)]=build['link_map']['sha256']
    inputs[str(a.emulator.resolve())]=sha(a.emulator)
    for clip in clips:inputs[clip['path']]=clip['sha256']
    for path,digest in inputs.items():
        if sha(path)!=digest:raise ValueError('Input changed: '+path)
    previous=json.loads((out/'report.json').read_text()) if a.verify_existing else None
    if previous and previous['inputs']!=inputs:raise ValueError('Existing capture belongs to different inputs')
    names=('HK_AMBIENCE_READY','HK_AMBIENCE_LOADED_MASK','HK_AMBIENCE_PLAYING_MASK',
           'HK_AMBIENCE_START_COUNT','HK_AMBIENCE_TRANSITIONS','HK_AMBIENCE_SCENE',
           'HK_AMBIENCE_FADE_TICKS','HK_AMBIENCE_VOICE_MASK','HK_AMBIENCE_VOICE_DENIALS',
           'HK_GAME_MODE','HK_ROOM_LOAD_ERROR','HK_REGION_ACTIVATIONS',
           'HK_CD_SECTORS_READ','HK_BOUNDARY_WAIT_TICKS','HK_PAD_POLL_MAX_VBLANK_GAP',
           '__psx_rt_fault_count','HK_AUDIO_STREAM_STARTS','HK_AUDIO_STREAM_REFILLS',
           'HK_AUDIO_STREAM_SOURCE_WRAPS','HK_AUDIO_STREAM_MAX_SERVICE_GAP','HK_AUDIO_STREAM_UNDERRUNS')
    addresses={}
    for name in names+('HK_AMBIENCE_GAINS','HK_AMBIENCE_STREAM_SOURCE'):
        m=re.search(r'^([0-9a-f]+)\s+.*\s'+name+r'$',map_text,re.M)
        if not m:raise ValueError('Missing marker: '+name)
        addresses[name]=int(m[1],16)
    def state(path):
        ram=path.read_bytes()
        def word(name,index=0):
            offset=addresses[name]-0x80000000+index*4
            return struct.unpack_from('<I',ram,offset)[0]
        return dict({name:word(name) for name in names},gains=[word('HK_AMBIENCE_GAINS',i) for i in range(len(clips))])
    commands=[]
    def launch(label,cue,polls,audio=False):
        # Two presses: the first dismisses the boot screen and the second
        # starts the game. One at poll 9 was enough until the disc grew to 60
        # scenes and the pack directory moved to a bootstrap read, and it has
        # been reaching a title that is not up yet since. A third press would
        # start the game and then pause it, which the stationary-gameplay check
        # below would read as a fault.
        tape=out/(label+'.pxtape');poll_tape(tape,'9:start:1,200:start:1',polls+1)
        command=[str(a.emulator.resolve()),'launch','--path',str(cue),'--embedded-playtest',
                 '--config-dir',str(out/'emulator'),'--steps','6000000000','--input-tape',str(tape),
                 '--stop-at-poll',str(polls),'--dump-ram',str(out/(label+'-ram.bin')),
                 '--dump-spu-ram',str(out/(label+'-spu.bin')),'--dump-display',str(out/(label+'.ppm'))]
        if audio:
            command+=['--dump-audio',str(out/(label+'.wav')),'--route-log',str(out/(label+'-route.csv'))]
            for name in names:command+=['--route-watch-u32',hex(addresses[name])]
        commands.append(command)
        if previous:
            if command not in previous['commands']:raise ValueError('Existing capture command differs')
        else:
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
    if any(normal[n] for n in ('HK_ROOM_LOAD_ERROR','HK_BOUNDARY_WAIT_TICKS','__psx_rt_fault_count','HK_AUDIO_STREAM_UNDERRUNS')):
        raise ValueError('Fault or loading hold during ambience playback: '+str(normal))
    # The cue the replay settles on, and everything that follows from it. These
    # were 63, 57, 4 and 30: the resident channel set written out a fourth time,
    # in a file that only runs on a disc and so only fails on one.
    #
    # A fifth copy of that same 4 survived below, in the stationary-gameplay
    # loop, and was wrong through two later swaps of the resident set: it read 2
    # under (0,1,3,4,7,15) and 3 under the current set. It is bound here instead
    # of read off `expected`, because `expected` is rebound to a byte buffer in
    # the SPU comparison between the two uses.
    cue=build['ambience']['cues'][0]
    cave_stems=bin(cue['mask']).count('1')
    expected={'HK_AMBIENCE_READY':1,'HK_AMBIENCE_LOADED_MASK':(1<<len(clips))-1,'HK_AMBIENCE_PLAYING_MASK':cue['mask'],
              'HK_AMBIENCE_START_COUNT':cave_stems,'HK_AMBIENCE_TRANSITIONS':1,'HK_AMBIENCE_SCENE':cue['scene'],
              'HK_AMBIENCE_FADE_TICKS':cue['fade_ticks'],'HK_AMBIENCE_VOICE_DENIALS':0,
              'HK_GAME_MODE':1,'HK_PAD_POLL_MAX_VBLANK_GAP':1}
    if any(normal[k]!=v for k,v in expected.items()):raise ValueError('Ambience lifecycle mismatch: '+str(normal))
    # A voice per audible stem, all of them ambience's own, and the stream on
    # the one voice that is never pooled. Which pooled voice a stem took is a
    # runtime decision, so what is checked is the count and the ownership.
    owned=set(build['ambience']['voices']);stream=build['ambience']['stream_voice']
    driven={v for v in owned if normal['HK_AMBIENCE_VOICE_MASK']>>v&1}
    if normal['HK_AMBIENCE_VOICE_MASK']!=sum(1<<v for v in driven):
        raise ValueError('Ambience is driving a voice it does not own: '+str(normal))
    if len(driven)!=bin(cue['mask']).count('1'):raise ValueError('Voices driven do not match audible stems: '+str(normal))
    streamed=[i for i,c in enumerate(clips) if c.get('spu_bytes',c['byte_len'])!=c['byte_len']]
    if (stream in driven)!=bool(cue['mask']>>streamed[0]&1):
        raise ValueError('The streamed stem is not on the voice that is never pooled: '+str(normal))
    # Two wraps of the source, counted in ring halves. This was 48, which is two
    # wraps at the ring size and sample rate of the time; the streamed clip
    # cooks at 4 kHz now and the same two wraps are 24 refills. The wrap count
    # already asserts the loops, so this floor only catches a ring being
    # refilled without advancing, and it has to be in the ring's own units.
    streamed_clip=clips[streamed[0]]
    refills=2*streamed_clip['byte_len']//(streamed_clip['spu_bytes']//2)
    if normal['HK_AUDIO_STREAM_STARTS']!=1 or normal['HK_AUDIO_STREAM_SOURCE_WRAPS']<2 or normal['HK_AUDIO_STREAM_REFILLS']<refills:
        raise ValueError('Stream did not cross two complete source loops: '+str(normal))
    expected_gains=[g if cue['mask']&(1<<i) else 0 for i,g in enumerate(cue['gains'])]
    if normal['gains']!=expected_gains:raise ValueError('Source mixer endpoint mismatch')
    spu=(out/'loops-spu.bin').read_bytes()
    for clip in clips:
        start=clip['spu_address']
        if clip.get('spu_bytes',clip['byte_len']) != clip['byte_len']:
            ram=(out/'loops-ram.bin').read_bytes()
            offset=addresses['HK_AMBIENCE_STREAM_SOURCE']-0x80000000
            source=Path(clip['path']).read_bytes()
            if ram[offset:offset+clip['byte_len']]!=source:
                raise ValueError('RAM stream source differs from complete cooked loop')
            half_bytes=clip['spu_bytes']//2
            newest=normal['HK_AUDIO_STREAM_REFILLS']+1
            for half in range(2):
                sequence=newest-((newest-half)%2)
                begin=(sequence*half_bytes)%len(source)
                expected=bytearray((source+source)[begin:begin+half_bytes])
                for block in range(0,half_bytes,16):
                    expected[block+1]=4 if half==0 and block==0 else 3 if half==1 and block==half_bytes-16 else 0
                address=start+half*half_bytes
                if spu[address:address+half_bytes]!=expected:
                    raise ValueError('Stream half does not match its observed source sequence')
            continue
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
            if int(row['ram_'+f'{addresses["HK_AMBIENCE_START_COUNT"]:08x}'])!=cave_stems:
                raise ValueError('Cave loops were restarted during stationary gameplay')
    for path,digest in inputs.items():
        if sha(path)!=digest:raise ValueError('Input changed during validation: '+path)
    report={'inputs':inputs,'commands':commands,'normal':normal,
            'spu_verified_bytes':sum(c.get('spu_bytes',c['byte_len']) for c in clips),'audio_seconds':seconds,
            'audio_peak':max(abs(v) for v in pcm),'clipped_samples':sum(abs(v)>=32767 for v in pcm),
            'audio_rms_per_second':rms,'longest_loop_seconds':longest,'hardware_validated':False,
            'limitations':'Actual emulator output and SPU residency; no physical-console, stereo-fidelity or click-free-loop claim. Surface transition lifecycle has native tests; natural Cave-to-Town traversal remains unverified.'}
    (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    print('Verified',report['spu_verified_bytes'],'SPU bytes,',seconds,'seconds of loop output')


if __name__=='__main__':main()
