"""Package the resident source ambience as raw disc chunks, never EXE samples.

How many loops there are, and which source atmos channels they are, is
`cook_music.RESIDENT_ATMOS_CHANNELS` and nothing else: the clip table, the cue
gain arrays, the SPU voices and the streamed clip's index all follow from it.
What rate each one is cooked at is `cook_music.DEFAULT_ATMOS_RATE` and the
per-channel exceptions beside it, which is a separate decision and no longer
inferred from which clip streams. Which clip each channel plays is read off the
persistent AudioManager, so a channel can be resident ahead of any scene that
enables it.

The catalogue's 456 scenes with an atmos cue between them name all 16 channels.
Eight would be enough to leave none of them without an audible stem; the eight
resident today are deliberately not that set, and what they leave silent is
Deepnest. The trade is recorded beside the tuple. What still has to be priced
per disc is the pool in `pool_pressure` and the SPU ceiling in `spu_ceiling`.

Eight used to not fit the voices, because a clip owned one for the life of the
disc and ambience owns six. A stem now takes a voice when it keys on and gives
it back when it has finished fading out, so what the set has to fit is not how
many loops are resident but how many of them can be audible at the same moment.
"""
import argparse,array,hashlib,json,math,re,struct,subprocess,sys
from pathlib import Path
from source import ROOT,Source,dump
from cook_music import sha,source_snapshot,atmos_rate,DEFAULT_ATMOS_RATE
from quality import SCENE_FILES

from cook_music import RESIDENT_ATMOS_CHANNELS as CHANNELS
SPU_START=0x18000
# The longest resident loop, which is what the stream exists for: 43.72 s of
# cave_noises is 99,936 bytes at 4 kHz and the set has 16,384 spare, so it
# cannot be held whole. Which channel streams is also which one stays exercised
# on hardware, because the ring only runs when a cue keys that stem on, and the
# Cave cue keys this one on in 41 admitted scenes. Whichever channel this is
# must be the only clip the cook truncates, because the guest finds it by
# looking for that one row.
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
# 18..20 Focus, 21..23 Runner. The cook never takes a voice another bank is
# already driving, so these six are the whole budget however long the set is.
VOICES=tuple(range(6,12))
# The streamed clip's voice cannot be pooled. audio_stream.rs owns the SPU IRQ
# address register exclusively while it runs and there is one of those on this
# hardware, so the stream keeps one voice for the life of the disc. It is the
# first voice ambience owns rather than whichever slot the streamed channel
# happens to occupy, so re-picking the resident set does not move it.
STREAM_VOICE=VOICES[0]
# The rest are a pool: a stem takes one when it keys on and returns it once it
# has finished fading out. This is what lets more loops be resident than there
# are voices, and it is why `assemble` prices the set on how many stems can be
# audible at once rather than on how many clips there are.
POOL_VOICES=VOICES[1:]
# The banks stacked above ambience in SPU RAM, which move by whatever ambience
# grows or shrinks. Their sizes do not depend on ambience, so the last build's
# generated manifests are the right source for what the tail costs. Their base
# addresses do depend on it, and are hardcoded in focus_audio.py and
# runner_audio.py, which both have to move in the same change.
TAIL_BANKS=('data/focus-audio.rs','data/runner-audio.rs')
# What rate each resident channel is cooked at, and why, is
# `cook_music.DEFAULT_ATMOS_RATE` and the per-channel exceptions beside it. It
# is not the same question as which channel streams: the streamed clip happens
# also to be a 4 kHz one, and reading either from the other put a literal in
# five places.
RATE=DEFAULT_ATMOS_RATE
RATES=tuple(sorted({atmos_rate(ch) for ch in CHANNELS}))

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

# A source Atmos gain above unity clamps to full scale rather than lifting one
# stem's ceiling above the 16,383 every other bank in this port mixes against.
# The largest boost anywhere in the catalogue is channel 1's +5.15 dB in the
# WindTunnel cue, so a gain past this ceiling means the snapshot's parent chain
# was read wrong rather than that a stem is loud, and the cook still refuses it.
BOOST_CEILING_DB=6.0

def volume(db):
    """One source mixer gain as an SPU voice volume; 16,383 is full scale."""
    if not math.isfinite(db) or db>BOOST_CEILING_DB:raise ValueError('unsupported mixer gain')
    return round(16383*10**(min(db,0.0)/20))

def boosts(channels,gains_db):
    """Which channels sit above unity, and by how much, for the report."""
    return {str(ch):round(db,5) for ch,db in zip(channels,gains_db) if db>0}

def bank_bytes(path):
    """BANK_BYTES out of one generated Rust audio manifest.

    The generators do not agree on spacing, so the pattern tolerates it rather
    than each caller carrying its own copy.
    """
    return bank_constant(path,'BANK_BYTES','usize')

def bank_constant(path,name,kind):
    """One `pub const <name>:<kind>=<integer>;` out of a generated manifest."""
    match=re.search(r'pub\s+const\s+%s\s*:\s*%s\s*=\s*(\d+)\s*;'%(name,kind),Path(path).read_text())
    if not match:raise ValueError(f'no {name} in '+str(path))
    return int(match[1])

def tail_base(root,name):
    """Where one bank stacked above ambience currently says it starts.

    The two generators spell it differently, so ask for both rather than
    teaching each caller which is which.
    """
    for constant in ('SPU_BASE','BANK_BASE'):
        try:return bank_constant(root/name,constant,'u32')
        except ValueError:continue
    raise ValueError('no SPU_BASE or BANK_BASE in '+name)

def tail_drift(root,end):
    """Tail banks whose declared base no longer abuts the bank below it.

    Ambience growing moves every bank above it, and those bases are hardcoded
    in focus_audio.py and runner_audio.py. Their own cooks refuse an overlap
    but not a gap, and neither runs unless someone remembers to run it, so the
    ambience cook says which base is now wrong and what it should be.
    """
    drift={};expected=end
    for name in TAIL_BANKS:
        declared=tail_base(root,name)
        if declared!=expected:drift[name]={'declared':declared,'required':expected}
        expected+=bank_bytes(root/name)
    return drift

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

def pool_pressure(cues):
    """Most pooled stems that can be keyed on at once, over the cooked cues.

    A transition fades the outgoing cue's stems out while the incoming cue's
    rise, so both sets hold a voice at the same time and the pool is sized on
    the union of two cues rather than on the largest single cue. Any scene can
    follow any scene, so the bound is over every pair of them. The streamed stem
    is excluded because its voice is never in the pool.

    Over all 456 catalogue scenes with an atmos cue this is 5 for either of the
    two eight-channel covering sets, against 3 for the largest single cue. The
    admitted scenes are a subset, so what this returns is usually smaller.
    """
    masks={c['mask'] for c in cues};stream=1<<STREAM_INDEX
    return max(bin((a|b)&~stream).count('1') for a in masks for b in masks)

def assemble(report,root,transitions,snapshots,sfx_end,ceiling):
    if sfx_end>SFX_END:raise ValueError("ambience overlaps resident SFX bank")
    if len(CHANNELS)>8:
        raise ValueError(f'{len(CHANNELS)} resident atmos channels do not fit the one-bit-per-stem cue mask')
    # Which clip each resident channel plays comes from the persistent
    # AudioManager, not from the admitted scenes, because a resident channel no
    # admitted scene enables still has a loop to load. Two of the current eight
    # are in that position: Fog Canyon and the Waterways are both outside the
    # admitted 60, so channels 9 and 10 are on the disc for scenes not yet in
    # the catalogue and no cue keys them on yet.
    residents={r['channel']:r for r in report['resident_atmos']}
    if set(residents)!=set(CHANNELS):raise ValueError('source report resolves a different resident atmos set')
    channels={ch:residents[ch]['clip'] for ch in CHANNELS};cues=[]
    for resident in residents.values():
        if not resident['loop'] or resident['pitch']!=1 or resident['volume']!=1:raise ValueError('unsupported AudioSource settings')
    for scene in report['scenes']:
        if scene['scene_file'] not in SCENE_FILES:continue
        if len(scene['managers'])!=1:raise ValueError('ambiguous scene audio manager')
        manager=scene['managers'][0]
        all_snapshot=snapshots[scene['scene_file']]
        # The scene's own atmosSnapshot sets every resident channel; an enabled
        # channel is then overridden by the atmos cue's snapshot, which is a
        # different object and can disagree. Keep the dB that wins so the
        # clamp record cannot name a level the bank does not play.
        used=[x['internal_volume_db'] for x in all_snapshot]
        gains=[volume(db) for db in used];mask=0
        for entry in manager['ambience']:
            ch=entry['channel'];idx=CHANNELS.index(ch)
            if channels[ch]!=entry['clip']:raise ValueError('shared channel changes clip')
            if not entry['loop'] or entry['pitch']!=1 or entry['volume']!=1:raise ValueError('unsupported AudioSource settings')
            snapshot=entry['snapshot']
            if snapshot['effects'] or any(x['mute'] or x['solo'] or x['pitch']!=1 for x in snapshot['chain']):
                raise ValueError('unsupported mixer processing')
            used[idx]=snapshot['internal_volume_db']
            gains[idx]=volume(used[idx]);mask|=1<<idx
        seconds=transitions[scene['scene_file']]
        if not math.isfinite(seconds) or not 0<=seconds<=60:raise ValueError('invalid atmosphere transition')
        cues.append({'scene':SCENE_FILES.index(scene['scene_file']),'source_scene':scene['scene_file'],
            'name':manager['atmos_cue']['name'],'source':manager['source'],'mask':mask,'gains':gains,
            'fade_ticks':round(seconds*60),'source_fade_seconds':seconds,'source_ambience':manager['ambience'],
            'clamped_boost_db':boosts(CHANNELS,used),'all_channel_snapshot_gains':all_snapshot})
    if {c['scene']for c in cues}!=set(range(len(SCENE_FILES))):raise ValueError('missing source ambience scenes')
    live=pool_pressure(cues)
    if live>len(POOL_VOICES):
        raise ValueError(f'{live} stems can be audible at once across a transition and ambience pools '
                         f'{len(POOL_VOICES)} SPU voices ({POOL_VOICES}); every other voice is allocated')
    clips=[];address=SPU_START
    for ch in CHANNELS:
        rate=atmos_rate(ch)
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
        clips.append({'source':c['source'],'name':c['name'],'source_channel':ch,
            # Only the streamed clip owns a voice; the rest are handed one out
            # of the pool when they key on, so naming one here would be a guess.
            'voice':STREAM_VOICE if ch==STREAM_CHANNEL else None,
            'rate':rate,'pitch':pitch,'actual_rate':pitch*44100/4096,'spu_address':address,
            'byte_len':len(payload),'spu_bytes':spu_bytes,'checksum':fnv(payload),'sha256':hashlib.sha256(payload).hexdigest(),
            'valid_frames':c['frames'],'padding_samples':c['padding_samples'],
            'converted_payload_sha256':plane['sha256'],'profile':c,'payload':payload})
        address+=spu_bytes
    return clips,sorted(cues,key=lambda c:c['scene'])

def decoder_quality(clip,path):
    payload=path.read_bytes();vag=path.with_suffix('.vag');decoded=path.with_suffix('.decoded.s16le')
    # The clip's own rate, not the default: VAG playback rate does not change
    # the decoded samples this compares, so a wrong one here stayed invisible.
    vag.write_bytes(b'VAGp'+struct.pack('>4I',0x20,0,len(payload),clip['rate'])+bytes(28)+payload)
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
        '#[derive(Clone,Copy)] pub struct AmbienceClip {pub byte_len:usize,pub spu_bytes:usize,pub checksum:u32,pub spu_address:u32,pub pitch:u16,pub source_channel:u8}',
        f'#[derive(Clone,Copy)] pub struct AmbienceCue {{pub mask:u8,pub gains:[i16;{len(clips)}],pub fade_ticks:u16}}',
        f'pub const AMBIENCE_CLIPS:[AmbienceClip;{len(clips)}]=[']
    for c in clips:lines.append('AmbienceClip{byte_len:%d,spu_bytes:%d,checksum:%d,spu_address:%d,pitch:%d,source_channel:%d},'%tuple(c[k]for k in ('byte_len','spu_bytes','checksum','spu_address','pitch','source_channel')))
    lines.extend(['];',f'pub const AMBIENCE_SPU_START:u32={SPU_START};',f'pub const AMBIENCE_SPU_END:u32={SPU_END};',f'pub const AMBIENCE_STREAM_BYTES:usize={STREAM_BYTES};',f'pub const AMBIENCE_STREAM_SOURCE_BYTES:usize={clips[STREAM_INDEX]["byte_len"]};',
        '// The streamed clip keeps this voice for the life of the disc; the pool is',
        '// handed out when a stem keys on and returned when it finishes fading out.',
        f'pub const AMBIENCE_STREAM_VOICE:u8={STREAM_VOICE};',
        f'pub const AMBIENCE_POOL_VOICES:[u8;{len(POOL_VOICES)}]={list(POOL_VOICES)};',
        f'pub const AMBIENCE_SCENES:[AmbienceCue;{len(cues)}]=['])
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
    if [r['channel'] for r in report['resident_atmos']]!=list(CHANNELS):raise ValueError('music report resolves a different resident atmos set')
    if sha(ROOT/report['source_methods']['path'])!=report['source_methods']['sha256']:raise ValueError('source methods hash mismatch')
    # Every rate the resident set actually reads, not just the default one: a
    # channel cooked at 4 kHz used to reach `assemble` unverified and fail there
    # on the payload hash instead of here on the stale cache.
    for c in report['clips']:
        if c['rate']in RATES and c['channels']==1:
            for plane in c['planes']:
                path=ROOT/plane['path']
                if sha(path)!=plane['sha256'] or not path.with_suffix('.s16le').is_file():raise ValueError('missing/stale converted profile')
    for rate in RATES:
        if len([c for c in report['clips']if c['rate']==rate and c['channels']==1])<len([ch for ch in CHANNELS if atmos_rate(ch)==rate]):
            raise ValueError('missing ambient profile')
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
        'spu_tail_bases':{name:tail_base(ROOT,name)for name in TAIL_BANKS},
        'resident_atmos_channels':list(CHANNELS),'voices':list(VOICES),
        'stream_voice':STREAM_VOICE,'pool_voices':list(POOL_VOICES),
        'stems_live_across_a_transition':pool_pressure(cues),
        'voice_pool_scope':f'Ambience owns voices {VOICES[0]}..{VOICES[-1]}. Voice {STREAM_VOICE} is the '
            f'stream\'s for the life of the disc; the other {len(POOL_VOICES)} are taken when a stem keys on '
            'and returned once it has finished fading out, so the resident set is bounded by how many stems '
            'can be audible at once rather than by how many clips there are.',
        'resident_atmos_rates':{str(ch):atmos_rate(ch)for ch in CHANNELS},
        'clamped_boosts':{c['source_scene']:c['clamped_boost_db']for c in cues if c['clamped_boost_db']},
        'clamped_boost_scope':f'Source Atmos gains above unity play at full scale instead. Refused above {BOOST_CEILING_DB} dB.',
        'source_report_sha256':sha(a.source_report),'source_method_sha256':sha(methods),
        'tool_sha256':sha(Path(__file__)),'rust_manifest_sha256':sha(manifest),
        'mixer_scope':'Source internal Atmos group and parent gains; downstream output mixers/player settings separate.',
        'transport':f'{len(clips)} complete source chunks loaded at boot; channel{STREAM_CHANNEL} cached in RAM and fed to a {STREAM_BYTES//1024}KiB SPU ring. The other {len(clips)-1} loops are resident in SPU. No event-time CD reads. Sector padding is outside checksum/byte_len.',
        'omitted_atmos_channels':'Every source atmos channel outside resident_atmos_channels; cook_music records them per scene in ambience_omitted_channels and the scene cooks a zero mask.'}
    drift=tail_drift(ROOT,end);result['spu_tail_drift']=drift
    dump(ROOT/'.hkpsx/ambience.json',result)
    print(f'{len(clips)} loops:',result['total_bytes'],'bytes, SPU end',hex(end),'free',result['spu_free_bytes'],
          f"below the {tail['ceiling']:#x} ceiling ({tail['reserved_bytes']} bytes of Focus and Runner bank sit above)")
    if drift:
        # The bank is written either way; what is stale is the tail above it,
        # and saying so beats leaving the next cook to find the overlap.
        for name,move in drift.items():
            print(f'  {name} declares base {move["declared"]:#x} and must be {move["required"]:#x}; re-run its cook')
        raise SystemExit('SPU tail banks no longer abut ambience')
if __name__=='__main__':main()
