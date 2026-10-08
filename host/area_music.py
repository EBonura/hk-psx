"""Cook area music as mono ADPCM streams.

Hollow Knight's area music is a MusicCue of up to six looping layers, and a
scene's music snapshot decides which of them are audible: Crossroads' Normal
plays its bass and main layers, its Sub Area only the main one. The guest has
one voice and one SPU ring for music, fed from main RAM and refilled from CD
between room loads, so each (cue, audible layer set) the admitted scenes and
their music regions can reach is premixed into one mono stream here. What a
snapshot changes on top of that is the stream's volume.

Streams are 22,050 Hz mono PSX ADPCM, resampled to a whole number of 2,048-byte
sectors (a multiple of 3,584 samples) so the refill never reads past a loop:
the loop keeps every source sample and plays up to half a 3,584-sample step
fast or slow, under a cent (0.058%) for every loop past 6 s; the 5.36 s drone
layer is the exception at 0.071%. Payload flags are zero; the
guest's ring owns loop and boundary flags.

The title and the fights' songs (Boss1, Enemy Battle, Boss Defeat) are XA-ADPCM
on the disc, cooked by host/hk-cook/src/xa_music.rs.

Source: the persistent AudioManager's music channels and mixer, each admitted
scene's SceneManager and its MusicRegion colliders (as bounding boxes). Audio
bytes stay in ignored data/ and .hkpsx/ outputs of this cook.
"""
import hashlib,json,math,subprocess,sys,wave
from pathlib import Path
import numpy as np
from source import ROOT,Source,dump,rel
from cook_music import decoded_source,sha,source_snapshot,compile_encoder
from quality import SCENE_FILES

RATE=22050
PITCH=round(RATE*4096/44100)
SECTOR=2048
SECTOR_SAMPLES=SECTOR//16*28
# A layer quieter than this under a snapshot is off: the source mutes layers at
# about -60 dB rather than stopping them.
AUDIBLE_DB=-40.0
FULL_SCALE=16383
TICKS=60
KEEP=255
OUT=ROOT/'.hkpsx/area-music'
REPORT=ROOT/'.hkpsx/area-music.json'
MANIFEST=ROOT/'data/area_music.rs'

def fnv(data):
    value=0x811c9dc5
    for byte in data:value=((value^byte)*0x01000193)&0xffffffff
    return value

def sector_samples(frames,source_rate):
    """Samples at RATE, rounded to whole sectors of ADPCM."""
    exact=frames*RATE/source_rate
    return max(1,round(exact/SECTOR_SAMPLES))*SECTOR_SAMPLES

def lanczos_resample(x,count,taps=16):
    """Resample `x` to exactly `count` samples over the same span. The ratio is
    within 0.1% of one, so no anti-alias band is lost; the loop wraps."""
    n=len(x);out=np.empty(count,np.float64);step=n/count;half=taps//2
    for start in range(0,count,1<<16):
        t=(np.arange(start,min(count,start+(1<<16)))*step)
        base=np.floor(t).astype(np.int64);frac=t-base
        acc=np.zeros(len(t));norm=np.zeros(len(t))
        for k in range(-half+1,half+1):
            d=frac-k;w=np.sinc(d)*np.sinc(d/half)
            acc+=w*x[(base+k)%n];norm+=w
        out[start:start+len(t)]=acc/norm
    return out

def layer_pcm(source,obj,folder,frames):
    """One source layer as mono float PCM at RATE, `frames` long."""
    wav,identity=decoded_source(source,obj,folder)
    raw=folder/f'{RATE}-1.s16le'
    subprocess.run(['ffmpeg','-v','error','-y','-i',str(wav),'-ar',str(RATE),'-ac','1','-f','s16le',str(raw)],check=True)
    pcm=np.frombuffer(raw.read_bytes(),'<i2').astype(np.float64)
    with wave.open(str(wav),'rb')as reader:source_frames,source_rate=reader.getnframes(),reader.getframerate()
    return lanczos_resample(pcm,frames),identity,source_frames,source_rate

def music_groups(s):
    resource=s.file('resources.assets')
    managers=[o for o in resource.objects.values() if o.type.name=='MonoBehaviour' and s.typename(o)=='AudioManager']
    if len(managers)!=1:raise ValueError('expected one persistent AudioManager')
    groups=[]
    for ref in s.read(managers[0])['musicSources']:
        audio=s.ref(resource,ref);tree=s.read(audio)
        if not tree['Loop'] or tree['m_Pitch']!=1:raise ValueError('unsupported music AudioSource')
        groups.append(s.ref(audio.assets_file,tree['OutputAudioMixerGroup']))
    return groups

def by_sid(s,sid):
    name,path=sid.rsplit(':',1);return s.file(name).objects[int(path)]

def plan(report,s,groups):
    """Families (cues), the snapshot table and per-scene/region music states."""
    cues={c['source']:c for c in report['music_cues']}
    families=[];snapshots=[];snapshot_objects={}
    def family(sid):
        if sid is None:return KEEP
        if sid not in families:families.append(sid)
        return families.index(sid)
    def snapshot(ref):
        if ref is None:return KEEP
        if ref['name'] not in snapshots:snapshots.append(ref['name']);snapshot_objects[ref['name']]=ref['source']
        elif snapshot_objects[ref['name']]!=ref['source']:raise ValueError('two snapshots share a name: '+ref['name'])
        return snapshots.index(ref['name'])
    scenes=[];regions=[]
    if [x['scene_file'] for x in report['scenes']]!=list(SCENE_FILES):raise ValueError('music report covers a different catalogue')
    for index,row in enumerate(report['scenes']):
        if len(row['managers'])!=1:raise ValueError('ambiguous SceneManager')
        m=row['managers'][0]
        scenes.append({'scene':index,'source_scene':row['scene_file'],'family':family(m['music_cue']),
            'snapshot':snapshot(m.get('music_snapshot')),'delay_ticks':round(m['music_delay']*TICKS),
            'fade_ticks':round(m['music_transition']*TICKS)})
        for region in row['music_regions']:
            if not(region['active'] and region['enabled']):continue
            points=[p for poly in region['polygons'] for p in poly]
            if not points:continue
            box=[min(p[0]for p in points),min(p[1]for p in points),max(p[0]for p in points),max(p[1]for p in points)]
            regions.append({'scene':index,'source':region['source'],'box':[round(v*65536) for v in box],
                'enter_family':family(region['enterMusicCue']),'enter_snapshot':snapshot(region['enterMusicSnapshot']),
                'enter_fade_ticks':round(region['enter_seconds']*TICKS),
                'exit_family':family(region['exitMusicCue']),'exit_snapshot':snapshot(region['exitMusicSnapshot']),
                'exit_fade_ticks':round(region['exit_seconds']*TICKS),
                'polygon_is_box':all(len(poly)==4 for poly in region['polygons']) and len(region['polygons'])==1})
    gains={}
    for name in snapshots:
        obj=by_sid(s,snapshot_objects[name])
        gains[name]=[source_snapshot(s,obj,g)['internal_volume_db'] for g in groups]
    live=reachable(scenes,regions,snapshots)
    table=[]
    for f,sid in enumerate(families):
        # nymmInTown would select Dirtmouth's accordion alternative; nothing
        # admitted sets it, so the first cue is the one cooked.
        cue=cues[sid];row=[]
        for sn,name in enumerate(snapshots):
            layers=[(c['channel'],c['clip'],gains[name][c['channel']]) for c in cue['channels']
                    if c['clip'] and gains[name][c['channel']]>AUDIBLE_DB] if (f,sn) in live else []
            row.append({'snapshot':name,'layers':layers,'reachable':(f,sn) in live})
        table.append({'cue':sid,'name':cue['name'],'mixes':row})
    return families,snapshots,gains,scenes,regions,table

def reachable(scenes,regions,snapshots):
    """(family, snapshot) pairs a player can hear. Every scene may be arrived at
    fresh (a Continue boots at its bench); gates carry the state across, a
    scene applies its own state on arrival and a region its enter and exit."""
    from ambience import gate_edges
    edges=gate_edges(ROOT);neighbours={}
    for a,b in edges:neighbours.setdefault(a,set()).add(b);neighbours.setdefault(b,set()).add(a)
    start=snapshots.index('Normal') if 'Normal' in snapshots else KEEP
    def apply(state,family,snapshot):
        return (state[0] if family==KEEP else family,state[1] if snapshot==KEEP else snapshot)
    seen=set();queue=[(x['scene'],(KEEP,start)) for x in scenes];heard=set()
    while queue:
        scene,state=queue.pop()
        if (scene,state) in seen:continue
        seen.add((scene,state));x=scenes[scene]
        inside={apply(state,x['family'],x['snapshot'])}
        for _ in range(2):
            for r in regions:
                if r['scene']!=scene:continue
                for y in list(inside):
                    e=apply(y,r['enter_family'],r['enter_snapshot']);inside|={e,apply(e,r['exit_family'],r['exit_snapshot'])}
        heard|={y for y in inside if KEEP not in y}
        queue+=[(n,y) for n in neighbours.get(scene,()) for y in inside]
    return heard

def cook():
    report=json.loads((ROOT/'.hkpsx/music/provenance.json').read_text())
    s=Source(report['source_directory']);groups=music_groups(s)
    families,snapshots,gains,scenes,regions,table=plan(report,s,groups)
    OUT.mkdir(parents=True,exist_ok=True);encoder=compile_encoder(OUT)
    clip_objects={sid:by_sid(s,sid) for f in table for m in f['mixes'] for _,sid,_ in m['layers']}
    tracks=[];mix_table=[]
    for f in table:
        row=[]
        for m in f['mixes']:
            if not m['layers']:row.append({'track':KEEP,'volume':0});continue
            top=max(g for _,_,g in m['layers'])
            # One stream per audible layer set: Normal Soft is Normal 10 dB
            # down with the layers half a dB apart, and plays Normal's stream.
            key=(f['cue'],tuple(ch for ch,_,_ in m['layers']))
            found=[i for i,t in enumerate(tracks) if t['key']==key]
            if not found:
                tracks.append({'key':key,'family':f['name'],'layers':[{'channel':ch,'clip':sid,'relative_db':round(g-top,3)} for ch,sid,g in m['layers']]})
                found=[len(tracks)-1]
            row.append({'track':found[0],'volume':round(FULL_SCALE*10**(min(top,0.0)/20))})
        mix_table.append(row)
    for index,t in enumerate(tracks):
        lengths={}
        for layer in t['layers']:
            obj=clip_objects[layer['clip']]
            tree=s.read(obj);lengths[layer['clip']]=(tree['m_Length'],tree['m_Frequency'])
        seconds={round(v[0],3) for v in lengths.values()}
        if len(seconds)!=1:raise ValueError('layers of one cue differ in length: '+t['family'])
        first=next(iter(lengths.values()));frames=sector_samples(round(first[0]*first[1]),first[1])
        mix=np.zeros(frames)
        for layer in t['layers']:
            folder=OUT/layer['clip'].replace(':','-');folder.mkdir(exist_ok=True)
            pcm,identity,source_frames,source_rate=layer_pcm(s,clip_objects[layer['clip']],folder,frames)
            mix+=pcm*10**(layer['relative_db']/20)
            layer.update(source_identity=identity,source_frames=source_frames,source_rate=source_rate)
        peak=float(np.max(np.abs(mix)))
        headroom=min(1.0,32767/peak) if peak else 1.0
        pcm=np.clip(np.round(mix*headroom),-32768,32767).astype('<i2')
        mono=OUT/f'track_{index}.s16le';mono.write_bytes(pcm.tobytes())
        encoded=OUT/f'track_{index}.adpcm'
        metric=json.loads(subprocess.check_output([str(encoder),str(mono),str(encoded),'--ring'],text=True))
        data=encoded.read_bytes()
        if len(data)!=frames//28*16 or len(data)%SECTOR or any(data[i+1] for i in range(0,len(data),16)):
            raise ValueError('music stream is not whole sectors of flagless ADPCM')
        path=ROOT/'data/music'/f'track_{index}.adpcm';path.parent.mkdir(parents=True,exist_ok=True);path.write_bytes(data)
        t.update(path=rel(path),frames=frames,seconds=frames/RATE,byte_len=len(data),sectors=len(data)//SECTOR,
            checksum=fnv(data),sha256=hashlib.sha256(data).hexdigest(),headroom_db=20*math.log10(headroom),
            rate_error=frames/(round(first[0]*first[1])*RATE/first[1])-1,encoder_metric=metric)
        del t['key']
    MANIFEST.write_text(rust_manifest(tracks,snapshots,mix_table,scenes,regions))
    return {'format':'hk-area-music-v1','rate':RATE,'pitch':PITCH,'families':[f['name'] for f in table],
        'snapshots':snapshots,'snapshot_layer_db':gains,'mixes':mix_table,'tracks':tracks,'scenes':scenes,'regions':regions,
        'manifest_sha256':sha(MANIFEST),
        'limitations':['Layer sets are premixed per (cue, audible layers): a Normal to Sub Area change switches '
            'premix at the same offset once the buffered audio ahead of it has played, instead of fading the bass layer.',
            'MusicRegion colliders are tested as their bounding boxes.',
            'nymmInTown (the Dirtmouth accordion) is not tracked; Dirtmouth plays its first cue.',
            'Source mixer effects and exact transition curves are not reproduced; volume ramps are linear.']}

def rust_manifest(tracks,snapshots,mixes,scenes,regions):
    lines=['// Generated by host/area_music.py; descriptors only, no embedded audio.',
        '#[derive(Clone,Copy)] pub struct MusicTrack {pub sectors:u32,pub byte_len:usize,pub checksum:u32}',
        '#[derive(Clone,Copy)] pub struct MusicMix {pub track:u8,pub volume:i16}',
        '#[derive(Clone,Copy)] pub struct MusicState {pub family:u8,pub snapshot:u8,pub fade_ticks:u16}',
        '#[derive(Clone,Copy)] pub struct MusicScene {pub state:MusicState,pub delay_ticks:u16}',
        '#[derive(Clone,Copy)] pub struct MusicRegion {pub scene:u16,pub box_:[i32;4],pub enter:MusicState,pub exit:MusicState}',
        f'/// No change: keep the playing cue or snapshot.\npub const MUSIC_KEEP:u8={KEEP};',
        f'pub const MUSIC_PITCH:u16={PITCH};',f'pub const MUSIC_SNAPSHOTS:usize={len(snapshots)};',
        f'pub const MUSIC_TRACKS:[MusicTrack;{len(tracks)}]=[']
    lines+=['MusicTrack{sectors:%d,byte_len:%d,checksum:%d},'%(t['sectors'],t['byte_len'],t['checksum']) for t in tracks]
    lines+=['];',f'/// [family][snapshot]: which premix plays and how loud; track {KEEP} is silence.',
        f'pub const MUSIC_MIXES:[[MusicMix;{len(snapshots)}];{len(mixes)}]=[']
    lines+=['['+','.join('MusicMix{track:%d,volume:%d}'%(m['track'],m['volume']) for m in row)+'],' for row in mixes]
    state=lambda f,s,t:'MusicState{family:%d,snapshot:%d,fade_ticks:%d}'%(f,s,t)
    lines+=['];',f'pub const MUSIC_SCENES:[MusicScene;{len(scenes)}]=[']
    lines+=['MusicScene{state:%s,delay_ticks:%d},'%(state(x['family'],x['snapshot'],x['fade_ticks']),x['delay_ticks']) for x in scenes]
    lines+=['];',f'pub const MUSIC_REGIONS:[MusicRegion;{len(regions)}]=[']
    lines+=['MusicRegion{scene:%d,box_:%s,enter:%s,exit:%s},'%(r['scene'],str(r['box']),state(r['enter_family'],r['enter_snapshot'],r['enter_fade_ticks']),
        state(r['exit_family'],r['exit_snapshot'],r['exit_fade_ticks'])) for r in regions]
    lines.append('];')
    return '\n'.join(lines)+'\n'

def main():
    inputs={'provenance':sha(ROOT/'.hkpsx/music/provenance.json')}
    code={n:sha(ROOT/'host'/n) for n in ('area_music.py','cook_music.py','source.py','spu_encode.py','spu_cook.py')}
    if REPORT.exists():
        old=json.loads(REPORT.read_text())
        outputs=[ROOT/t['path'] for t in old.get('tracks',[])]+[MANIFEST]
        if old.get('inputs')==inputs and old.get('code')==code and all(p.exists() for p in outputs) \
                and sha(MANIFEST)==old['manifest_sha256'] \
                and all(sha(ROOT/t['path'])==t['sha256'] for t in old['tracks']) \
                and not any(Path(t['path']).is_absolute() for t in old['tracks']):
            print('Area music cache verified');return
    result=cook();result.update(inputs=inputs,code=code)
    dump(REPORT,result)
    for i,t in enumerate(result['tracks']):
        print(f"track {i}: {t['family']} layers {[l['channel'] for l in t['layers']]} {t['seconds']:.2f}s {t['byte_len']} bytes "
              f"headroom {t['headroom_db']:.2f} dB rate error {t['rate_error']*100:+.4f}%")
if __name__=='__main__':main()
