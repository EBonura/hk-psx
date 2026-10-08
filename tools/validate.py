#!/usr/bin/env python3
"""Smoke-test final CUE/EXE, deterministic input and software display captures."""
import argparse,csv,hashlib,json,re,shutil,statistics,struct,subprocess,sys,tempfile
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'host'))
from paths import DISC_LIBRARY, artifacts


def _hk_cache_const(name):
    """A `hk_cache` budget read from the crate that enforces it, not copied here.

    `MAX_UPLOAD_BYTES` was written into this file as 16384 and stayed there when
    the animation cache tripled to 49152, so the gate below would have refused
    exactly the frames that change was made to admit.
    """
    text=(ROOT/'shared/hk-cache/src/lib.rs').read_text()
    return int(re.search(rf'pub const {name}: u32 = (\d+);',text).group(1))


MAX_UPLOAD_BYTES=_hk_cache_const('MAX_UPLOAD_BYTES')
# The previous route-clock fixtures had a 16-tick boot offset. Express the
# same movement durations on the guest poll clock, which pauses during CD reads.
ROUTE='9:start:1,54:right:120,89:cross:30,214:left:90,229:cross:3'
COMBAT_ROUTE='9:start:1,54:right:12,66:square:1'
DOOR_ROUTE='9:start:1,54:right:446,285:cross:24,410:cross:24,'+','.join(f'{n}:square:2' for n in range(78,490,24))
# Actual final-disc crawler encounter: source cooldown25 requires a longer press cadence.
ENEMY_ROUTE=('9:start:1,54:right:756,'+','.join(f'{n}:square:2' for n in range(78,720,24))+','+
             ','.join(f'{n}:cross:24' for n in (285,410,540,655))+','+
             ','.join(f'{n}:square:2' for n in range(722,1200,26)))
# Earned combat on the five-slot seamless build: stop before passing the first
# source Crawler. ENEMY_ROUTE above remains the historical streaming A/B tape.
EARNED_COMBAT_ROUTE=('9:start:1,54:right:706,'+','.join(f'{n}:square:2' for n in range(78,720,24))+','+
                    ','.join(f'{n}:cross:24' for n in (285,410,540,655))+','+
                    ','.join(f'{n}:square:2' for n in range(722,1200,26)))
# The full pad, so a route can exercise the abilities P14 bound to the shoulders
# and Triangle, not only movement, jump, nail and Focus.
BUTTONS={'select':1,'l3':2,'r3':4,'start':8,'up':16,'right':32,'down':64,'left':128,
         'l2':256,'r2':512,'l1':1024,'r1':2048,
         'triangle':4096,'circle':8192,'cross':16384,'square':32768}
def poll_tape(path,events,count=1024):
    """PXITAPE2: verified frontend format, one input sample per port-1 poll.

    Route-clock --press events expire while CD loading blocks polling. Tapes
    pause with the guest and therefore preserve gameplay input across loaders.
    """
    samples=[0]*count
    for event in events.split(','):
        if not event:continue
        poll,name,hold=event.split(':')
        poll,hold=int(poll),int(hold)
        if poll<0 or hold<1 or poll+hold>=count:raise ValueError('Input event outside tape')
        for n in range(poll,poll+hold):samples[n]|=BUTTONS[name]
    path.write_bytes(b'PXITAPE2'+struct.pack('<II',count,0)+
                     b''.join(struct.pack('<HBBBB',mask,128,128,128,128) for mask in samples))
def digest(path):
    with path.open('rb') as stream:return hashlib.file_digest(stream,'sha256').hexdigest()
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--emulator',required=True);a=p.parse_args();emu=Path(a.emulator).resolve()
    emulator_sha=digest(emu)
    help_text=subprocess.check_output([str(emu),'launch','--help'],text=True)
    for flag in ('--input-tape','--stop-at-poll','--disc','--cd-command-log','--dump-hw'):
        if flag not in help_text:raise ValueError('Missing required emulator option: '+flag)
    out=ROOT/'captures/regions-final';out.mkdir(parents=True,exist_ok=True);state=ROOT/'.hkpsx';build=json.load(open(state/'build.json'));telemetry=build['telemetry']
    # Remove only this validator's interval captures, so a shorter new run does
    # not leave older frames or an old PNG for an empty pre-init PPM.
    for folder in (out,out/'combat',out/'doors'):
        for pattern in ('tick-[0-9]*.ppm','tick-[0-9]*.png'):
            for capture in folder.glob(pattern):capture.unlink()
    paths=artifacts(telemetry)
    for path,meta in build['outputs'].items():
        if digest(ROOT/path)!=meta['sha256']:raise ValueError('Artifact changed since the build report')
    commands=[]
    map_path=Path(build['link_map']['path'])
    if digest(map_path)!=build['link_map']['sha256']:raise ValueError('Link map changed since this build')
    link_map=map_path.read_text()
    packed=json.loads((state/'packed-rooms.json').read_text())
    metadata_path=Path(packed['metadata'])
    if not metadata_path.is_absolute():metadata_path=ROOT/metadata_path
    selected=json.loads(metadata_path.read_text())
    if len(packed['regions'])!=len(selected['regions']) or not selected['regions']:
        raise ValueError('Packed and selected region counts disagree')
    ambient=build['ambience']['clips']
    chunk_count=len(packed['regions'])+len(ambient)
    header_sectors=(28+chunk_count*24+2047)//2048
    expected_room_bytes=packed['regions'][0]['raw_bytes']
    ambience_sectors=sum((clip['byte_len']+2047)//2048 for clip in ambient)
    initial_sectors=header_sectors+ambience_sectors+(packed['regions'][0]['stored_bytes']+2047)//2048
    # Check the actual final disc table against the exact host compression report.
    # The raw sector user area starts at byte24 for this MODE2/2352 writer.
    table=bytearray()
    with paths['bin'].open('rb') as disc:
        for sector in range(header_sectors):
            disc.seek((1024+sector)*2352+24);table+=disc.read(2048)
    if table[:8]!=b'PSOXWPAK' or struct.unpack_from('<I',table,12)[0]!=chunk_count:
        raise ValueError('Final disc region table mismatch')
    next_sector=header_sectors
    for index,(entry,region) in enumerate(zip(packed['regions'],selected['regions'])):
        chunk_id,offset,sectors,size,checksum,_=struct.unpack_from('<6I',table,28+index*24)
        if (chunk_id,offset,sectors,size,checksum)!=(index+1,next_sector,(entry['stored_bytes']+2047)//2048,entry['stored_bytes'],entry['stored_fnv']):
            raise ValueError(f'Final disc chunk {index+1} does not match compression report')
        if region['bytes']!=entry['raw_bytes']:raise ValueError('Selected region raw size mismatch')
        next_sector+=sectors
    ambient_extents=[]
    for index,clip in enumerate(ambient,len(packed['regions'])):
        entry=struct.unpack_from('<6I',table,28+index*24)
        sectors=(clip['byte_len']+2047)//2048
        if entry!=(index+1,next_sector,sectors,clip['byte_len'],clip['checksum'],0):
            raise ValueError('Final disc ambience table mismatch')
        ambient_extents.append((next_sector,sectors))
        next_sector+=sectors
    def word(ram_path,symbol,index=0):
        match=re.search(r'^([0-9a-f]+)\s+.*\s'+re.escape(symbol)+r'$',link_map,re.MULTILINE)
        if not match:raise ValueError('Missing validation symbol: '+symbol)
        if index < 0:raise ValueError('Negative validation symbol index')
        offset=int(match[1],16)-0x80000000+index*4
        ram=ram_path.read_bytes()
        if not 0 <= offset <= len(ram)-4:raise ValueError('Validation symbol outside RAM: '+symbol)
        return int.from_bytes(ram[offset:offset+4],'little')
    def sfx(ram_path):
        names=('door','jump','land','nail','hurt','enemy_hit')
        counts={name:word(ram_path,'HK_SFX_EVENT_COUNTS',index) for index,name in enumerate(names)}
        total=word(ram_path,'HK_SFX_COUNT')
        if total!=sum(counts.values()):raise ValueError(f'SFX counter total mismatch: {total}, {counts}')
        return dict(total=total,**counts)
    def launch(label,path,extra,presses=ROUTE,tape_count=1024):
        cmd=[str(emu),'launch','--path',str(path),'--embedded-playtest','--config-dir',str(state/'emulator'),'--steps','2000000000']
        if presses:
            tape=state/(label+'.pxtape');poll_tape(tape,presses,count=tape_count)
            cmd+=['--input-tape',str(tape)]
        cmd+=list(map(str,extra));commands.append(cmd)
        print('Running',label,flush=True)
        with (state/(label+'.log')).open('w') as log:subprocess.run(cmd,cwd=ROOT,stdout=log,stderr=subprocess.STDOUT,check=True)
        text=(state/(label+'.log')).read_text()
        match=re.search(r'route-ticks=(\d+)  port1-polls=(\d+)',text)
        if not match:raise ValueError('Missing emulator completion counters')
        return {'route_ticks':int(match[1]),'pad_polls':int(match[2])}
    def residency(ram):
        values={name:word(ram,symbol) for name,symbol in {
            'load_state':'HK_ROOM_LOAD_STATE','load_error':'HK_ROOM_LOAD_ERROR',
            'room_bytes':'HK_ROOM_BYTES','cd_sectors':'HK_CD_SECTORS_READ',
            'region_id':'HK_REGION_ID','region_loads':'HK_REGION_LOADS','room_cache_hits':'HK_ROOM_CACHE_HITS',
            'cache_hits':'HK_ANIM_CACHE_HITS','cache_misses':'HK_ANIM_CACHE_MISSES',
            'upload_bytes':'HK_ANIM_UPLOAD_BYTES','peak_frame_upload_bytes':'HK_ANIM_UPLOAD_MAX_FRAME',
        }.items()}
        if values['load_state']!=2 or values['load_error']:
            raise ValueError(f'CD room load did not verify: {values}')
        region_id=values['region_id']
        if not 1<=region_id<=len(packed['regions']):raise ValueError(f'Invalid resident region: {values}')
        if values['room_bytes']!=packed['regions'][region_id-1]['raw_bytes'] or values['cd_sectors']<initial_sectors or not values['region_loads']:
            raise ValueError(f'Unexpected region bytes or incomplete initial CD load: {values}')
        if not values['cache_hits'] or not values['cache_misses']:
            raise ValueError(f'Animation cache route did not exercise hits and misses: {values}')
        if not 0<values['peak_frame_upload_bytes']<=MAX_UPLOAD_BYTES or not values['upload_bytes']:
            raise ValueError(f'Animation upload budget failed: {values}')
        return values
    title=launch('final-title',paths['cue'],['--stop-at-poll',120,'--dump-display',out/'title.ppm','--dump-ram',state/'title-ram.bin'],presses='')
    title['sfx']=sfx(state/'title-ram.bin')
    if title['sfx']['total']:raise ValueError('Gameplay sound fired on the idle title')
    if word(state/'title-ram.bin','HK_GAME_MODE')!=0:raise ValueError('Title started gameplay without input')
    if word(state/'title-ram.bin','__psx_rt_fault_count'):raise ValueError('Title runtime exception')
    if word(state/'title-ram.bin','HK_CD_SECTORS_READ') or word(state/'title-ram.bin','HK_ROOM_LOAD_STATE'):
        raise ValueError('Room read started before title entry')
    extra=['--stop-at-poll',600]
    if telemetry:extra+=['--guest-debug-log','--profile-log',state/'final-profile.csv']
    route=launch('final-route',paths['cue'],extra+['--route-screenshot-dir',out,'--route-screenshot-interval',30,'--dump-display',out/'final.ppm','--dump-hw',out/'hardware.ppm','--stack-profile-log',state/'final-stack.csv','--dump-ram',state/'final-ram.bin','--cd-command-log',state/'final-cd.csv'])
    if route['pad_polls']<600:raise ValueError('Route failed to reach requested input duration')
    if word(state/'final-ram.bin','HK_GAME_MODE')!=1:raise ValueError('START failed to enter gameplay')
    route['residency']=residency(state/'final-ram.bin')
    repeats=[]
    for n in range(2):
        label=f'final-repeat-{n}'
        result=launch(label,paths['cue'],['--stop-at-poll',300,'--dump-display',out/f'repeat-{n}.ppm','--dump-ram',state/f'repeat-{n}.bin'])
        result['residency']=residency(state/f'repeat-{n}.bin')
        repeats.append({'display':digest(out/f'repeat-{n}.ppm'),'ram':digest(state/f'repeat-{n}.bin'),**result})
    if repeats[0]!=repeats[1]:raise ValueError('Deterministic route diverged')
    exe=launch('final-exe',paths['exe'],['--stop-at-poll',200,'--dump-display',out/'exe.ppm','--dump-ram',state/'exe-ram.bin'],presses='25:cross:1,150:cross:1')
    if exe['pad_polls']<200:raise ValueError('Missing-disc EXE did not return to a responsive retry screen')
    if word(state/'exe-ram.bin','HK_GAME_MODE')!=3 or word(state/'exe-ram.bin','HK_ROOM_LOAD_STATE')!=3:
        raise ValueError('Missing-disc EXE did not show a recoverable load error')
    exe['load_error']=word(state/'exe-ram.bin','HK_ROOM_LOAD_ERROR')
    if not exe['load_error'] or word(state/'exe-ram.bin','HK_ROOM_BYTES'):
        raise ValueError('Missing-disc EXE accepted invalid room data')
    if word(state/'exe-ram.bin','__psx_rt_fault_count'):raise ValueError('EXE runtime exception')
    mounted_exe=launch('final-exe-disc',paths['exe'],['--disc',paths['cue'],'--stop-at-poll',200,
                      '--dump-display',out/'exe-disc.ppm','--dump-ram',state/'exe-disc-ram.bin'],presses='25:cross:1')
    if mounted_exe['pad_polls']<200 or word(state/'exe-disc-ram.bin','HK_GAME_MODE')!=1:
        raise ValueError('EXE with mounted disc failed to enter gameplay')
    if word(state/'exe-disc-ram.bin','__psx_rt_fault_count'):raise ValueError('Mounted EXE runtime exception')
    mounted_exe['residency']=residency(state/'exe-disc-ram.bin')
    # A payload read that transfers bytes still must fail its integrity check.
    # This diagnostic disc lives exclusively in the user's PS1 disc library.
    with tempfile.TemporaryDirectory(prefix='.hk-psx-corrupt-',dir=DISC_LIBRARY) as temp:
        corrupt_bin=Path(temp)/'hk-psx-corrupt.bin'
        corrupt_cue=corrupt_bin.with_suffix('.cue')
        shutil.copyfile(paths['bin'],corrupt_bin)
        with corrupt_bin.open('r+b') as disc:
            disc.seek(1024*2352+24)
            pack_header=disc.read(2048)
            if pack_header[:8]!=b'PSOXWPAK':raise ValueError('Diagnostic disc has no SDK world pack')
            room_sector=struct.unpack_from('<I',pack_header,28+4)[0]
            corruption_offset=(1024+room_sector)*2352+24+128
            disc.seek(corruption_offset);original=disc.read(1)
            if len(original)!=1:raise ValueError('Diagnostic corruption outside disc')
            disc.seek(corruption_offset);disc.write(bytes([original[0]^1]))
        corrupt_cue.write_text('FILE "hk-psx-corrupt.bin" BINARY\n  TRACK 01 MODE2/2352\n    INDEX 01 00:00:00\n')
        corrupt=launch('final-corrupt-disc',corrupt_cue,['--stop-at-poll',100,
                       '--dump-display',out/'corrupt.ppm','--dump-ram',state/'corrupt-ram.bin'],presses='9:start:1')
    corrupt_ram=state/'corrupt-ram.bin'
    if corrupt['pad_polls']<100 or word(corrupt_ram,'HK_GAME_MODE')!=3:
        raise ValueError('Corrupt disc did not return to responsive retry screen')
    if word(corrupt_ram,'HK_ROOM_LOAD_STATE')!=3 or word(corrupt_ram,'HK_ROOM_LOAD_ERROR')!=6:
        raise ValueError('Corrupt room payload was not rejected by checksum')
    if word(corrupt_ram,'HK_CD_SECTORS_READ')!=initial_sectors:
        raise ValueError('Corrupt payload check did not read the full room')
    if any(word(corrupt_ram,name) for name in ('HK_ROOM_BYTES','HK_ANIM_CACHE_HITS','HK_ANIM_CACHE_MISSES','__psx_rt_fault_count')):
        raise ValueError('Corrupt payload reached rendering or raised a runtime fault')
    corrupt.update(load_error=6,cd_sectors=initial_sectors,modified_raw_offset=corruption_offset)
    combat_extra=['--stop-at-poll',140,'--route-screenshot-dir',out/'combat',
                  '--route-screenshot-interval',1,'--dump-display',out/'combat.ppm',
                  '--dump-ram',state/'combat-ram.bin','--stack-profile-log',state/'combat-stack.csv']
    if telemetry:combat_extra+=['--profile-log',state/'combat-profile.csv']
    combat=launch('final-combat',paths['cue'],combat_extra,presses=COMBAT_ROUTE)
    combat['residency']=residency(state/'combat-ram.bin')
    cut=word(state/'combat-ram.bin','HK_GRASS_CUT_MASK')
    swings=word(state/'combat-ram.bin','HK_NAIL_ATTACK_COUNT')
    if cut!=512 or swings!=1:raise ValueError(f'Source grass cut route failed: mask={cut}, swings={swings}')
    if word(state/'combat-ram.bin','__psx_rt_fault_count'):raise ValueError('Combat runtime exception')
    combat.update(grass_cut_mask=cut,nail_attacks=swings,sfx=sfx(state/'combat-ram.bin'))
    if combat['sfx']['nail']!=swings:raise ValueError('Accepted nail attack did not play exactly one swing sound')
    reset=launch('final-reset',paths['cue'],['--stop-at-poll',170,'--dump-display',out/'reset.ppm',
                 '--dump-ram',state/'reset-ram.bin'],presses=COMBAT_ROUTE+',104:select:1')
    if word(state/'reset-ram.bin','HK_GRASS_CUT_MASK') or word(state/'reset-ram.bin','HK_NAIL_ATTACK_COUNT'):
        raise ValueError('Select failed to reset grass/nail state')
    if word(state/'reset-ram.bin','__psx_rt_fault_count'):raise ValueError('Reset runtime exception')
    reset['residency']=residency(state/'reset-ram.bin')
    door_extra=['--stop-at-poll',500,'--dump-display',out/'doors.ppm','--dump-hw',out/'doors-hardware.ppm',
                '--dump-ram',state/'doors-ram.bin','--stack-profile-log',state/'doors-stack.csv',
                '--cd-command-log',state/'doors-cd.csv','--route-log',state/'doors-route.csv',
                '--dump-audio',out/'doors.wav','--dump-spu-ram',state/'doors-spu.bin',
                '--route-screenshot-dir',out/'doors','--route-screenshot-interval',30]
    if telemetry:door_extra+=['--guest-debug-log','--profile-log',state/'doors-profile.csv']
    doors=launch('final-doors',paths['cue'],door_extra,presses=DOOR_ROUTE)
    door_ram=state/'doors-ram.bin'
    doors['residency']=residency(door_ram)
    raw_x=word(door_ram,'HK_PLAYER_X');raw_y=word(door_ram,'HK_PLAYER_Y')
    signed=lambda value:value-(1<<32) if value&(1<<31) else value
    doors.update(x=signed(raw_x)/65536,y=signed(raw_y)/65536,broken=word(door_ram,'HK_BREAK_COUNT'),
                 health=word(door_ram,'HK_HEALTH'),deaths=word(door_ram,'HK_DEATHS'),
                 attacks=word(door_ram,'HK_NAIL_ATTACK_COUNT'))
    if doors['pad_polls']<500 or doors['x']<=74 or doors['broken']<2 or doors['attacks']<2:
        raise ValueError(f'First-door continuation route did not pass two barriers: {doors}')
    if not 1<=doors['health']<=5 or doors['residency']['region_loads']<2:
        raise ValueError(f'Door route health or regional loading failed: {doors}')
    if word(door_ram,'__psx_rt_fault_count'):raise ValueError('Door route runtime exception')
    import wave
    with wave.open(str(out/'doors.wav'),'rb') as wav:
        if wav.getsampwidth()!=2:raise ValueError('Unexpected audio capture sample width')
        samples=wav.readframes(wav.getnframes())
    doors['audio_peak']=max((abs(sample[0]) for sample in struct.iter_unpack('<h',samples)),default=0)
    if doors['audio_peak']==0:raise ValueError('Door route sound capture is silent')
    doors['sfx']=sfx(door_ram)
    if any(doors['sfx'][event]==0 for event in ('door','jump','land','nail')):
        raise ValueError(f'Door traversal did not play each source movement/combat sound: {doors["sfx"]}')
    if doors['sfx']['nail']!=doors['attacks']:
        raise ValueError('Nail sound count differs from accepted attack count')
    audio_bank=(ROOT/'data/sfx.adpcm').read_bytes()
    audio_metadata=json.load(open(state/'audio-provenance.json'))
    audio_base=audio_metadata['spu_base']
    if (state/'doors-spu.bin').read_bytes()[audio_base:audio_base+len(audio_bank)]!=audio_bank:
        raise ValueError('Resident source SFX bank differs from actual SPU RAM')
    doors['spu_bank_verified_bytes']=len(audio_bank)
    enemy_extra=['--stop-at-poll',1200,'--dump-display',out/'enemy.ppm','--dump-hw',out/'enemy-hardware.ppm',
                 '--dump-ram',state/'enemy-ram.bin','--stack-profile-log',state/'enemy-stack.csv']
    if telemetry:enemy_extra+=['--guest-debug-log']
    enemy=launch('final-enemy',paths['cue'],enemy_extra,presses=EARNED_COMBAT_ROUTE,tape_count=2048)
    enemy_ram=state/'enemy-ram.bin'
    enemy.update(hits=word(enemy_ram,'HK_ENEMY_HITS'),kills=word(enemy_ram,'HK_ENEMY_KILLS'),
                 soul=word(enemy_ram,'HK_SOUL'),health=word(enemy_ram,'HK_HEALTH'),
                 mode=word(enemy_ram,'HK_GAME_MODE'))
    if enemy['pad_polls']<1200 or enemy['hits']<2 or enemy['kills']<1 or enemy['soul']!=22:
        raise ValueError(f'Crawler route failed two-hit death/source SOUL reward: {enemy}')
    if not 1<=enemy['health']<=5 or enemy['mode']!=1 or word(enemy_ram,'__psx_rt_fault_count'):
        raise ValueError(f'Crawler route did not finish in healthy gameplay: {enemy}')
    enemy['residency']=residency(enemy_ram)
    enemy['sfx']=sfx(enemy_ram)
    if not 2<=enemy['sfx']['enemy_hit']<=enemy['hits']:
        raise ValueError(f'Accepted enemy hits did not play source impact sounds: {enemy["sfx"]}')
    # Convert local captures. The route clock can fire before GPU init; its
    # exact empty 320x0 PPM is not an image. Final captures remain mandatory.
    subprocess.run([str(ROOT/'.venv/bin/python'),'-c',"from PIL import Image;from pathlib import Path;import sys;[(Image.open(p).save(p.with_suffix('.png'))) for p in Path(sys.argv[1]).rglob('*.ppm') if p.read_bytes().splitlines()!=[b'P6',b'320 0',b'255']]",str(out)],check=True)
    image_stats=json.loads(subprocess.check_output([str(ROOT/'.venv/bin/python'),'-c',
        "from PIL import Image;import json,sys;im=Image.open(sys.argv[1]).convert('RGB');print(json.dumps({'size':im.size,'distinct_colors':len(set(im.getdata())),'bright_pixels':sum(max(p)>100 for p in im.getdata())}))",str(out/'hardware.png')]))
    if image_stats['distinct_colors']<64 or image_stats['bright_pixels']<100:
        raise ValueError('Hardware viewport is black or missing scene content; inspect hardware.png')
    stats={}
    if telemetry:
        rows=[x for x in csv.DictReader(open(state/'final-profile.csv')) if int(x['frame_cycles']) and int(x['sim_ticks'])]
        for key in ['frame_cycles','render','update','present','sim_ticks','tri_primitives']:
            vals=[int(x[key]) for x in rows[10:]];stats[key]={'min':min(vals),'mean':statistics.mean(vals),'max':max(vals)}
    stack=[row for name in ('final-stack.csv','combat-stack.csv','doors-stack.csv','enemy-stack.csv') for row in csv.DictReader(open(state/name))]
    raw_stack_span=max(int(x['depth_bytes']) for x in stack)
    # The CD handler deliberately switches to a separate static stack. The
    # emulator's whole-run SP minimum therefore measures the distance between
    # two allocations, not main-stack consumption. Keep that raw diagnostic,
    # and measure the dedicated IRQ stack's initialized sentinel independently.
    irq_match=re.search(r'^([0-9a-f]+)\s+.*\sHK_CD_IRQ_STACK$',link_map,re.MULTILINE)
    irq_stack_high=None
    if irq_match:
        start=int(irq_match[1],16)-0x80000000
        untouched=[]
        for name in ('final-ram.bin','combat-ram.bin','doors-ram.bin','enemy-ram.bin'):
            data=(state/name).read_bytes()[start:start+2048]
            if len(data)!=2048 or data[:256]!=bytes([0xa5])*256:
                raise ValueError('CD IRQ stack guard was overwritten or never initialized')
            untouched.append(next((i for i,b in enumerate(data) if b!=0xa5),2048))
        irq_stack_high=2048-min(untouched)
    high=None if irq_match else raw_stack_span
    faults=word(state/'final-ram.bin','__psx_rt_fault_count')
    if faults:raise ValueError(f'Guest runtime reported {faults} unexpected exceptions')
    if digest(emu)!=emulator_sha:raise ValueError('Emulator changed during validation; rerun against one binary')
    summary={'unexpected_guest_exceptions':faults,'artifact_hashes':build['outputs'],'emulator':str(emu),'emulator_sha256':emulator_sha,'telemetry':telemetry,'input_clock':'pad_poll (PXITAPE2)','region_count':len(selected['regions']),'packed_report':packed,'expected_initial_room_bytes':expected_room_bytes,'expected_initial_cd_sectors':initial_sectors,'title':title,'route':route,'repeats':repeats,'exe_missing_disc':exe,'exe_mounted_disc':mounted_exe,'corrupt_disc':corrupt,'combat':combat,'reset':reset,'doors':doors,'enemy':enemy,'hardware_renderer_image':image_stats,'stack_observed_bytes':high,'irq_stack_observed_bytes':irq_stack_high,'raw_stack_profiler_span_bytes':raw_stack_span,'stack_limitations':'Main-stack high-water unavailable from whole-run SP trace when dedicated CD IRQ stack is active. IRQ measurement is a sentinel lower-bound observation.','profile':stats,'commands':commands,'hardware_validated':False}
    (state/'validation.json').write_text(json.dumps(summary,indent=2));print('Verified CUE, EXE, repeat display/RAM hashes. Captures:',out)
if __name__=='__main__':main()
