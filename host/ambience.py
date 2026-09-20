"""Package the resident source ambience as raw disc chunks, never EXE samples.

How many loops there are, and which source atmos channels they are, is
`cook_music.RESIDENT_ATMOS_CHANNELS` and nothing else: the clip table, the cue
gain arrays, the SPU voices and the streamed clip's index all follow from it.
The catalogue's 456 scenes with an atmos cue between them name all 16 channels,
so no resident set covers the game; the set is a budget decision per disc,
priced by the voice budget in `assemble` and the SPU ceiling in `spu_ceiling`.
"""
import argparse,array,hashlib,json,math,re,struct,subprocess,sys
from pathlib import Path
from source import ROOT,Source,dump
from cook_music import sha,source_snapshot
from quality import SCENE_FILES

from cook_music import RESIDENT_ATMOS_CHANNELS as CHANNELS
SPU_START=0x18000
STREAM_CHANNEL=4
# The one clip cached in RAM rather than held whole in SPU. Its position in the
# table follows the channel set, and the guest derives the same index from the
# cooked clips, so re-membering the set cannot point the stream at another
# loop's voice and RAM buffer.
STREAM_INDEX=CHANNELS.index(STREAM_CHANNEL)
STREAM_BYTES=16384
STREAM_RAM_LIMIT=200000
SPU_END=0x80000
SFX_END=0x14000 # Geo occupies 0x14000..0x18000, below ambience.
# The SPU voices ambience owns. The other 18 are spoken for: 0..5 and 16..17
# player SFX, 12..14 Geo, 15 shared by the Great Door and the False Knight,
# 18..20 Focus, 21..23 Runner. A longer resident set has nowhere to key on, so
# the cook refuses rather than taking a voice another bank is already driving.
VOICES=tuple(range(6,12))
# The banks stacked above ambience in SPU RAM, which move up by whatever
# ambience grows. Their sizes do not depend on ambience, so the last build's
# generated manifests are the right source for what the tail costs. Their base
# addresses do depend on it, and are hardcoded in focus_audio.py and
# runner_audio.py, which both have to move in the same change.
TAIL_BANKS=('data/focus-audio.rs','data/runner-audio.rs')
RATE=8000
# The cave bed is the one clip whose 8 kHz copy will not fit SPU; halving its
# rate halves the RAM cache it streams from. 99% of its energy is below 637 Hz.
STREAM_RATE=4000

def fnv(data):
    value=0x811c9dc5
    for byte in data:value=((value^byte)*0x01000193)&0xffffffff
    return value

def validate_blocks(data):
    if not data or len(data)%16:raise ValueError('partial or empty ADPCM blocks')
    if data[0]>>4:raise ValueError('initial ADPCM predictor must be zero')
    for i in range(0,len(data),16):
        if data[i]>>4>4 or data[i]&15>12:raise ValueError('invalid ADPCM header')

def loop_payload(data):
    validate_blocks(data)
    if any(data[i+1] for i in range(0,len(data),16)):raise ValueError('input contains transport flags')
    result=bytearray(data);result[1]=4;result[-15]|=3
    validate_loop(result)
    return bytes(result)

def validate_loop(data):
    validate_blocks(data)
    for i in range(0,len(data),16):
        expected=(4 if i==0 else 0)|(3 if i==len(data)-16 else 0)
        if data[i+1]!=expected:raise ValueError('invalid loop start/end flags')

def volume(db):
    if not math.isfinite(db) or db>0:raise ValueError('unsupported mixer gain')
    return round(16383*10**(db/20))

def bank_bytes(path):
    """BANK_BYTES out of one generated Rust audio manifest.

    The generators do not agree on spacing, so the pattern tolerates it rather
    than each caller carrying its own copy.
    """
    match=re.search(r'pub\s+const\s+BANK_BYTES\s*:\s*usize\s*=\s*(\d+)\s*;',Path(path).read_text())
    if not match:raise ValueError('no BANK_BYTES in '+str(path))
    return int(match[1])

def spu_ceiling(root):
    """Highest SPU address ambience may reach, and what the tail above costs.

    This used to be SPU_END, which counted the Focus and Runner banks above
    ambience as free space and reported a figure 128KiB too generous. Nothing
    raised on it, because focus_audio.py catches the overlap later from its own
    side; what it cost was every budget note that quoted the free figure.
    """
    banks={name:bank_bytes(root/name) for name in TAIL_BANKS}
    reserved=sum(banks.values())
    return {'ceiling':SPU_END-reserved,'reserved_bytes':reserved,'banks':banks}

def assemble(report,root,transitions,snapshots,sfx_end,ceiling):
    if sfx_end>SFX_END:raise ValueError("ambience overlaps resident SFX bank")
    if len(CHANNELS)>len(VOICES):
        raise ValueError(f'{len(CHANNELS)} resident atmos channels need {len(CHANNELS)} SPU voices; '
                         f'ambience owns {len(VOICES)} ({VOICES[0]}..{VOICES[-1]}) and every other voice is allocated')
    channels={};cues=[]
    for scene in report['scenes']:
        if scene['scene_file'] not in SCENE_FILES:continue
        if len(scene['managers'])!=1:raise ValueError('ambiguous scene audio manager')
        manager=scene['managers'][0]
        all_snapshot=snapshots[scene['scene_file']]
        gains=[volume(x['internal_volume_db']) for x in all_snapshot];mask=0
        for entry in manager['ambience']:
            ch=entry['channel'];idx=CHANNELS.index(ch)
            if ch in channels and channels[ch]!=entry['clip']:raise ValueError('shared channel changes clip')
            channels[ch]=entry['clip']
            if not entry['loop'] or entry['pitch']!=1 or entry['volume']!=1:raise ValueError('unsupported AudioSource settings')
            snapshot=entry['snapshot']
            if snapshot['effects'] or any(x['mute'] or x['solo'] or x['pitch']!=1 for x in snapshot['chain']):
                raise ValueError('unsupported mixer processing')
            gains[idx]=volume(snapshot['internal_volume_db']);mask|=1<<idx
        seconds=transitions[scene['scene_file']]
        if not math.isfinite(seconds) or not 0<=seconds<=60:raise ValueError('invalid atmosphere transition')
        cues.append({'scene':SCENE_FILES.index(scene['scene_file']),'source_scene':scene['scene_file'],
            'name':manager['atmos_cue']['name'],'source':manager['source'],'mask':mask,'gains':gains,
            'fade_ticks':round(seconds*60),'source_fade_seconds':seconds,'source_ambience':manager['ambience'],
            'all_channel_snapshot_gains':all_snapshot})
    if set(channels)!=set(CHANNELS) or {c['scene']for c in cues}!=set(range(len(SCENE_FILES))):raise ValueError('missing source ambience channels/scenes')
    clips=[];address=SPU_START
    for index,ch in enumerate(CHANNELS):
        rate=STREAM_RATE if ch==STREAM_CHANNEL else RATE
        matches=[c for c in report['clips']if c['source']==channels[ch] and c['rate']==rate and c['channels']==1]
        if len(matches)!=1:raise ValueError(f'missing or duplicate {rate}Hz mono profile')
        c=matches[0]
        if len(c['planes'])!=1 or c['frames']<=0:raise ValueError('invalid mono source profile')
        plane=c['planes'][0];path=(root/plane['path']).resolve()
        if not path.is_relative_to((root/'.hkpsx').resolve()):raise ValueError('converted payload outside ignored source cache')
        data=path.read_bytes()
        if len(data)!=plane['bytes'] or sha(path)!=plane['sha256']:raise ValueError('converted payload hash mismatch')
        if len(data)!=((c['frames']+27)//28)*16 or c['padding_samples']!=(-c['frames'])%28:
            raise ValueError('encoded length does not preserve valid sample count')
        payload=loop_payload(data)
        spu_bytes=min(len(payload),STREAM_BYTES) if ch==STREAM_CHANNEL else len(payload)
        if spu_bytes<len(payload) and len(payload)>STREAM_RAM_LIMIT:raise ValueError('ambient stream exceeds RAM cache')
        if address%16 or address<SPU_START:raise ValueError('ambience SPU capacity overflow')
        if address+spu_bytes>ceiling:
            raise ValueError(f'ambience SPU capacity overflow: {c["name"]} ends {address+spu_bytes-ceiling} bytes '
                             f'past the {ceiling:#x} ceiling')
        pitch=round(rate*4096/44100)
        if pitch!=c['spu_pitch']:raise ValueError('pitch/profile mismatch')
        clips.append({'source':c['source'],'name':c['name'],'source_channel':ch,'voice':VOICES[index],
            'rate':rate,'pitch':pitch,'actual_rate':pitch*44100/4096,'spu_address':address,
            'byte_len':len(payload),'spu_bytes':spu_bytes,'checksum':fnv(payload),'sha256':hashlib.sha256(payload).hexdigest(),
            'valid_frames':c['frames'],'padding_samples':c['padding_samples'],
            'converted_payload_sha256':plane['sha256'],'profile':c,'payload':payload})
        address+=spu_bytes
    return clips,sorted(cues,key=lambda c:c['scene'])

def decoder_quality(clip,path):
    payload=path.read_bytes();vag=path.with_suffix('.vag');decoded=path.with_suffix('.decoded.s16le')
    vag.write_bytes(b'VAGp'+struct.pack('>4I',0x20,0,len(payload),RATE)+bytes(28)+payload)
    subprocess.run(['ffmpeg','-v','error','-y','-i',str(vag),'-f','s16le',str(decoded)],check=True)
    pcm=array.array('h');pcm.frombytes(decoded.read_bytes())
    source=array.array('h');source.frombytes((ROOT/clip['profile']['planes'][0]['path']).with_suffix('.s16le').read_bytes())
    if sys.byteorder!='little':pcm.byteswap();source.byteswap()
    if len(pcm)!=(len(payload)//16)*28 or len(source)!=clip['valid_frames']:raise ValueError('decoder length mismatch')
    error=sum((a-b)**2 for a,b in zip(source,pcm));signal=sum(x*x for x in source)
    snr=10*math.log10(signal/error)if signal and error else None
    expected=clip['profile']['planes'][0]['ffmpeg_snr_db']
    if (snr is None)!=(expected is None) or (snr is not None and abs(snr-expected)>0.000001):
        raise ValueError('loop flags changed decoded sample quality')
    return {'ffmpeg_snr_db':snr,'decoded_sha256':sha(decoded),
        'decoded_boundary_delta':int(pcm[0])-pcm[-1],
        'source_boundary_delta':int(source[0])-source[-1],
        'zero_tail_samples':clip['padding_samples'],
        'assessment':'Full-loop boundary retained plus final-block padding. Filter0 resets history; no click-free claim or waveform alteration.'}

def rust_manifest(clips,cues):
    lines=['// Generated by host/ambience.py; descriptors only, no embedded audio.',
        '#[derive(Clone,Copy)] pub struct AmbienceClip {pub byte_len:usize,pub spu_bytes:usize,pub checksum:u32,pub spu_address:u32,pub pitch:u16,pub voice:u8,pub source_channel:u8}',
        f'#[derive(Clone,Copy)] pub struct AmbienceCue {{pub mask:u8,pub gains:[i16;{len(clips)}],pub fade_ticks:u16}}',
        f'pub const AMBIENCE_CLIPS:[AmbienceClip;{len(clips)}]=[']
    for c in clips:lines.append('AmbienceClip{byte_len:%d,spu_bytes:%d,checksum:%d,spu_address:%d,pitch:%d,voice:%d,source_channel:%d},'%tuple(c[k]for k in ('byte_len','spu_bytes','checksum','spu_address','pitch','voice','source_channel')))
    lines.extend(['];',f'pub const AMBIENCE_SPU_START:u32={SPU_START};',f'pub const AMBIENCE_SPU_END:u32={SPU_END};',f'pub const AMBIENCE_STREAM_BYTES:usize={STREAM_BYTES};',f'pub const AMBIENCE_STREAM_SOURCE_BYTES:usize={clips[STREAM_INDEX]["byte_len"]};',f'pub const AMBIENCE_SCENES:[AmbienceCue;{len(cues)}]=['])
    for c in cues:lines.append('AmbienceCue{mask:%d,gains:%s,fade_ticks:%d},'%(c['mask'],str(c['gains']),c['fade_ticks']))
    lines.append('];');return '\n'.join(lines)+'\n'

def cached_report(path):
    report=json.loads(path.read_text())
    current=json.loads((ROOT/'.hkpsx/doctor.json').read_text())['installs'][0]['data_directory']
    if Path(report['source_directory']).resolve()!=Path(current).resolve():
        raise ValueError('music cache belongs to a different selected Windows install')
    # Recheck bytes, not modification times: same-size replacements invalidate.
    for name,value in report['inputs'].items():
        if sha(Path(report['source_directory'])/name)!=value['sha256']:raise ValueError('music source changed: '+name)
    for name,value in report['tool_hashes'].items():
        if sha(ROOT/'host'/name)!=value:raise ValueError('music conversion code changed: '+name)
    if [scene['scene_file'] for scene in report['scenes']]!=list(SCENE_FILES):raise ValueError('music report covers a different scene catalog')
    if sha(ROOT/report['source_methods']['path'])!=report['source_methods']['sha256']:raise ValueError('source methods hash mismatch')
    for c in report['clips']:
        if c['rate']==RATE and c['channels']==1:
            for plane in c['planes']:
                path=ROOT/plane['path']
                if sha(path)!=plane['sha256'] or not path.with_suffix('.s16le').is_file():raise ValueError('missing/stale converted profile')
    if len([c for c in report['clips']if c['rate']==RATE and c['channels']==1])!=len(CHANNELS):raise ValueError('missing ambient profile')
    return report

def ensure_music(path):
    try:return cached_report(path)
    except (OSError,ValueError,KeyError,TypeError):
        print('Preparing source music/ambience conversions...',flush=True)
        subprocess.run([sys.executable,str(ROOT/'host/cook_music.py'),'--output',str(path.parent)],check=True)
        return cached_report(path)

def sfx_reservation(root):
    data=(root/'data/sfx.adpcm').read_bytes()
    if bank_bytes(root/'data/sfx.rs')!=len(data) or len(data)%16:raise ValueError('SFX bank/manifest mismatch')
    end=0x1010+len(data)
    if end>SFX_END:raise ValueError('ambience overlaps resident SFX bank')
    return {'start':0x1010,'end':end,'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest(),
        'manifest_sha256':sha(root/'data/sfx.rs')}

def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--source-report',type=Path,default=ROOT/'.hkpsx/music/provenance.json');a=parser.parse_args()
    report=ensure_music(a.source_report)
    methods=ROOT/report['source_methods']['path']
    s=Source(report['source_directory']);transitions={};snapshots={}
    resource=s.file('resources.assets');manager=s.read(resource.objects[26261])
    for file in SCENE_FILES:
        scene_file=s.file(file)
        managers=[o for o in scene_file.objects.values() if o.type.name=='MonoBehaviour' and s.typename(o)=='SceneManager']
        if len(managers)!=1:raise ValueError('ambiguous SceneManager in '+file)
        scene_manager=s.read(managers[0]);snapshot=s.ref(scene_file,scene_manager['atmosSnapshot'])
        transitions[file]=scene_manager['transitionTime'];snapshots[file]=[]
        for channel in CHANNELS:
            audio=s.ref(resource,manager['atmosSources'][channel]);tree=s.read(audio)
            group=s.ref(audio.assets_file,tree['OutputAudioMixerGroup'])
            snapshots[file].append(source_snapshot(s,snapshot,group))
    sfx=sfx_reservation(ROOT);tail=spu_ceiling(ROOT)
    clips,cues=assemble(report,ROOT,transitions,snapshots,sfx['end'],tail['ceiling'])
    out=ROOT/'data/ambience';out.mkdir(parents=True,exist_ok=True)
    work=ROOT/'.hkpsx/ambience';work.mkdir(parents=True,exist_ok=True)
    for i,c in enumerate(clips):
        path=out/f'clip_{i}.adpcm';payload=c.pop('payload');path.write_bytes(payload)
        c['path']=str(path);quality_path=work/f'clip_{i}.adpcm';quality_path.write_bytes(payload)
        c['quality']=decoder_quality(c,quality_path)
    manifest=ROOT/'data/ambience.rs';manifest.write_text(rust_manifest(clips,cues))
    end=clips[-1]['spu_address']+clips[-1]['spu_bytes']
    result={'format':'raw-psx-adpcm-loops-v1','clips':clips,'cues':cues,
        'sfx_reservation':sfx,'total_bytes':sum(c['byte_len']for c in clips),'spu_bytes':sum(c['spu_bytes']for c in clips),'ram_cache_bytes':sum(c['byte_len']for c in clips if c['spu_bytes']<c['byte_len']),'spu_start':SPU_START,'spu_end':end,
        # Free below the banks stacked above ambience, not below SPU RAM's end.
        'spu_free_bytes':tail['ceiling']-end,'spu_ceiling':tail['ceiling'],
        'spu_tail_reserved_bytes':tail['reserved_bytes'],'spu_tail_banks':tail['banks'],
        'resident_atmos_channels':list(CHANNELS),'voices':list(VOICES[:len(clips)]),
        'source_report_sha256':sha(a.source_report),'source_method_sha256':sha(methods),
        'tool_sha256':sha(Path(__file__)),'rust_manifest_sha256':sha(manifest),
        'mixer_scope':'Source internal Atmos group and parent gains; downstream output mixers/player settings separate.',
        'transport':f'{len(clips)} complete source chunks loaded at boot; channel{STREAM_CHANNEL} cached in RAM and fed to a {STREAM_BYTES//1024}KiB SPU ring. The other {len(clips)-1} loops are resident in SPU. No event-time CD reads. Sector padding is outside checksum/byte_len.',
        'omitted_atmos_channels':'Every source atmos channel outside resident_atmos_channels; cook_music records them per scene in ambience_omitted_channels and the scene cooks a zero mask.'}
    dump(ROOT/'.hkpsx/ambience.json',result)
    print(f'{len(clips)} loops:',result['total_bytes'],'bytes, SPU end',hex(end),'free',result['spu_free_bytes'],
          f"below the {tail['ceiling']:#x} ceiling ({tail['reserved_bytes']} bytes of Focus and Runner bank sit above)")
if __name__=='__main__':main()
