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

SPU residency follows the area rather than the game. The guest loads a clip at
the scene gate that first needs it, into an address `allocate` gives it here,
and reuses the bytes of clips the new area does not play. Two clips share SPU
only when no scene cue plays both and no scene gate joins a scene that plays
one to a scene that plays the other, so a gate never has to cut a stem that is
still fading out. The same holds for the clips of every scene one gate away,
so the guest can load the next area's clips in the background while the drive
is idle, into bytes nothing around the Knight is using, and an area-change
gate finds them resident. Transitions no gate describes (a respawn at a distant bench,
the debug reset) are the guest's to arbitrate: it stops an outgoing stem whose
bytes the incoming cue needs, during the black load, and counts it.
"""
import argparse,array,hashlib,json,math,re,struct,subprocess,sys
from pathlib import Path
from source import ROOT,Source,dump,rel
from cook_music import sha,source_snapshot,atmos_rate,DEFAULT_ATMOS_RATE
from quality import SCENE_FILES

from cook_music import RESIDENT_ATMOS_CHANNELS as CHANNELS
SPU_START=0x18000
# Every loop is held whole in SPU now, cave_noises' 99,936 bytes included: it
# used to stream from a RAM copy through a 16 KiB ring because all eight loops
# had to fit at once, and per-area residency made room for it. The ring, its
# RAM buffer and its voice belong to area music (game/src/audio_stream.rs).
# The ring sits directly below the world one-shot bank (host/hk-cook/src/cook_audio.rs), which
# sits directly below Focus; ambience must end at or below the ring.
MUSIC_RING_BYTES=16384
SPU_END=0x80000
SFX_END=0x14000 # Geo occupies 0x14000..0x18000, below ambience.
# The SPU voices ambience owns. The other 19 are spoken for: 0..5 and 16..17
# player SFX, 11 the per-scene one-shots (host/scene_sfx.py), 12..14 Geo, 15
# shared by the Great Door, the False Knight, the menu and the world bank,
# 18..20 Focus, 21..23 Runner. The cook never takes a voice another bank is
# already driving, so these five are the whole budget however long the set is.
# Voice 11 left the pool when the scene banks arrived: the admitted cues keep
# at most four stems audible across a transition (`pool_pressure`), so the
# pool is four; a catalogue that needs five is refused below, by name.
VOICES=tuple(range(6,11))
SCENE_SFX_VOICE=11
# The first of them is area music's. audio_stream.rs owns the SPU IRQ address
# register exclusively while the music ring runs and there is one of those on
# this hardware, so the ring keeps one voice for the life of the disc. Before
# area music it carried cave_noises' ring; that loop is pooled like the rest.
MUSIC_VOICE=VOICES[0]
# The rest are a pool: a stem takes one when it keys on and returns it once it
# has finished fading out. This is what lets more loops be resident than there
# are voices, and it is why `assemble` prices the set on how many stems can be
# audible at once rather than on how many clips there are.
POOL_VOICES=VOICES[1:]
# The banks stacked above ambience in SPU RAM, lowest first. Their sizes do not
# depend on ambience, so the last build's generated manifests are the right
# source for what the tail costs. Focus and Runner are pinned to the top of SPU
# RAM (host/hk-cook/src/focus_audio.rs, runner_audio.py) and the world bank is cooked to end
# where Focus begins, so the ring and ambience's ceiling are what move.
TAIL_BANKS=('data/world-sfx.rs','data/focus-audio.rs','data/runner-audio.rs')
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

# Stems a scene's cue enables but which the scene does not keep resident.
# Crossroads_04 (Gruz Mother's arena): its cue plays `Rain Indoor`
# (ruins_rain_indoor_loop, 15,936 SPU bytes) through the `at Cave` snapshot at
# -60.4 dB, voice gain 16 of 16383. Its bytes carry the Gruz Mother one-shots
# instead (Manny's call, 2026-10-01). Only a stem at -60 dB or quieter (gain 16
# or less) may be listed; the cook refuses anything louder.
UNLOADED_STEMS={'level40':('ruins_rain_indoor_loop',)}
INAUDIBLE_GAIN=16

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
    """Tail banks whose declared base now sits inside the bank below it.

    Ambience growing moves every bank above it, and those bases are hardcoded
    in host/hk-cook/src/focus_audio.rs and runner_audio.py. Their own cooks refuse an overlap
    and neither runs unless someone remembers to run it, so the ambience cook
    says which base is now wrong and the lowest it may be. A gap is not drift:
    per-area residency shrank ambience below the bases the tail was cooked at,
    and the gap is free SPU (`spu_gap_bytes` in the report).
    """
    drift={};expected=end
    for name in TAIL_BANKS:
        declared=tail_base(root,name)
        if declared<expected:drift[name]={'declared':declared,'required':expected}
        expected=max(expected,declared)+bank_bytes(root/name)
    return drift

def spu_ceiling(root):
    """Highest SPU address ambience may reach, and what the tail above costs.

    This used to be SPU_END, which counted the Focus and Runner banks above
    ambience as free space and reported a figure 128KiB too generous. Nothing
    raised on it, because the Focus cook catches the overlap later from its own
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
    follow any scene, so the bound is over every pair of them.

    Over all 456 catalogue scenes with an atmos cue this is 5 for either of the
    two eight-channel covering sets, against 3 for the largest single cue. The
    admitted scenes are a subset, so what this returns is usually smaller.
    """
    masks={c['mask'] for c in cues}
    return max(bin(a|b).count('1') for a in masks for b in masks)

def neighbour_masks(cues,edges):
    """Per scene, the stems of every scene one resolved gate away."""
    mask={c['scene']:c['mask'] for c in cues};out={s:0 for s in mask}
    for a,b in edges:
        if a in mask and b in mask:out[a]|=mask[b];out[b]|=mask[a]
    return out

def conflicts(cues,edges):
    """Stem pairs that may be in SPU at the same time.

    Both stems of one cue play together, and a gate between two scenes plays
    the outgoing cue's stems out while the incoming cue's rise, during and
    after the load. The guest also loads the stems of every scene one gate away
    in the background, so a scene's own stems and all of its neighbours' are
    resident together. `edges` are scene-id pairs joined by a resolved gate.
    """
    mask={c['scene']:c['mask'] for c in cues};together={c['mask'] for c in cues}
    together|={mask[a]|mask[b] for a,b in edges if a in mask and b in mask}
    together|={mask[s]|n for s,n in neighbour_masks(cues,edges).items()}
    pairs=set()
    for m in together:
        stems=[s for s in range(len(CHANNELS)) if m>>s&1]
        pairs|={(a,b) for a in stems for b in stems if a!=b}
    return pairs

def allocate(sizes,pairs,start):
    """First-fit SPU addresses, largest clip first, sharing bytes between clips
    that can never be resident together. A clip no cue plays conflicts with
    nothing and never loads, so it takes `start` and costs nothing."""
    address={}
    for stem in sorted(range(len(sizes)),key=lambda s:(-sizes[s],s)):
        taken=sorted((address[o],address[o]+sizes[o]) for o in address if (stem,o) in pairs)
        at=start
        for lo,hi in taken:
            if at+sizes[stem]<=lo:break
            at=max(at,(hi+15)&~15)
        address[stem]=at
    return [address[s] for s in range(len(sizes))]

def assemble(report,root,transitions,snapshots,sfx_end,ceiling,edges=()):
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
    clips=[]
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
        spu_bytes=len(payload)
        pitch=round(rate*4096/44100)
        if pitch!=c['spu_pitch']:raise ValueError('pitch/profile mismatch')
        clips.append({'source':c['source'],'name':c['name'],'source_channel':ch,
            'rate':rate,'pitch':pitch,'actual_rate':pitch*44100/4096,
            'byte_len':len(payload),'spu_bytes':spu_bytes,'checksum':fnv(payload),'sha256':hashlib.sha256(payload).hexdigest(),
            'valid_frames':c['frames'],'padding_samples':c['padding_samples'],
            'converted_payload_sha256':plane['sha256'],'profile':c,'payload':payload})
    pairs=conflicts(cues,edges)
    near=neighbour_masks(cues,edges)
    for c in cues:c['prefetch']=near[c['scene']]&~c['mask']
    # Scoped residency drops, applied after the conflict pairs above so every
    # stem keeps the address it had. The scene's one-shot bank (scene_sfx.py)
    # then sees the stem's bytes as a gap, and ambience::forget cuts the stem
    # if it is still fading in from the previous scene when the bank lands.
    for c in cues:
        for name in UNLOADED_STEMS.get(c['source_scene'],()):
            stem=next(i for i,ch in enumerate(CHANNELS) if clips[i]['name']==name)
            if c['gains'][stem]>INAUDIBLE_GAIN:
                raise ValueError(f'{name} is audible in {c["source_scene"]}; it may not be dropped from residency')
            c['mask']&=~(1<<stem);c['prefetch']&=~(1<<stem)
            # scene_sfx.py fits the scene's existing clips to the layout they
            # had, and only the scene's late rows to these bytes.
            c['unloaded']=c.get('unloaded',0)|(1<<stem)
    for clip,address in zip(clips,allocate([c['spu_bytes'] for c in clips],pairs,SPU_START)):
        if address%16 or address<SPU_START:raise ValueError('ambience SPU capacity overflow')
        if address+clip['spu_bytes']>ceiling:
            raise ValueError(f'ambience SPU capacity overflow: {clip["name"]} ends {address+clip["spu_bytes"]-ceiling} bytes '
                             f'past the {ceiling:#x} ceiling')
        clip['spu_address']=address
    for clip in clips:
        clip['shares_spu_with']=[o['source_channel'] for o in clips if o is not clip
            and o['spu_address']<clip['spu_address']+clip['spu_bytes'] and clip['spu_address']<o['spu_address']+o['spu_bytes']]
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

def rust_manifest(clips,cues,ring_base=SPU_END-MUSIC_RING_BYTES):
    lines=['// Generated by host/ambience.py; descriptors only, no embedded audio.',
        '#[derive(Clone,Copy)] pub struct AmbienceClip {pub byte_len:usize,pub spu_bytes:usize,pub checksum:u32,pub spu_address:u32,pub pitch:u16,pub source_channel:u8}',
        f'#[derive(Clone,Copy)] pub struct AmbienceCue {{pub mask:u8,pub gains:[i16;{len(clips)}],pub fade_ticks:u16}}',
        f'pub const AMBIENCE_CLIPS:[AmbienceClip;{len(clips)}]=[']
    for c in clips:lines.append('AmbienceClip{byte_len:%d,spu_bytes:%d,checksum:%d,spu_address:%d,pitch:%d,source_channel:%d},'%tuple(c[k]for k in ('byte_len','spu_bytes','checksum','spu_address','pitch','source_channel')))
    end=max(c['spu_address']+c['spu_bytes'] for c in clips)
    lines.extend(['];',f'pub const AMBIENCE_SPU_START:u32={SPU_START};',
        '// The end of the widest set any cue or gate keeps resident, not of every clip at once.',
        f'pub const AMBIENCE_SPU_END:u32={end};',
        '// Area music keeps this voice and this ring for the life of the disc; the',
        '// pool is handed out when a stem keys on and returned when it finishes fading.',
        f'pub const MUSIC_VOICE:u8={MUSIC_VOICE};',f'pub const MUSIC_RING_BASE:u32={ring_base};',
        f'pub const MUSIC_RING_BYTES:usize={MUSIC_RING_BYTES};',
        f'pub const AMBIENCE_POOL_VOICES:[u8;{len(POOL_VOICES)}]={list(POOL_VOICES)};',
        '// The per-scene one-shot banks\' voice (host/scene_sfx.py), outside the pool.',
        f'pub const SCENE_SFX_VOICE:u8={SCENE_SFX_VOICE};',
        f'pub const AMBIENCE_SCENES:[AmbienceCue;{len(cues)}]=['])
    for c in cues:lines.append('AmbienceCue{mask:%d,gains:%s,fade_ticks:%d},'%(c['mask'],str(c['gains']),c['fade_ticks']))
    lines.append('];')
    lines.append('/// Per scene: stems of the scenes one gate away that its own cue does not play,')
    lines.append('/// which the drive loads in the background while it is idle.')
    lines.append(f'pub const AMBIENCE_PREFETCH:[u8;{len(cues)}]={[c.get("prefetch",0) for c in cues]};')
    return '\n'.join(lines)+'\n'

def gate_edges(root):
    """Scene-id pairs joined by a resolved gate, from the region cook's report."""
    scenes=json.loads((root/'data/regions.json').read_text())['scenes']
    if [s['file'] for s in scenes]!=list(SCENE_FILES) or any(s['scene_id']!=i for i,s in enumerate(scenes)):
        raise ValueError('region report covers a different scene catalog')
    return {tuple(sorted((s['scene_id'],g['target_scene']))) for s in scenes for g in s['resolved_gates']
            if g['target_scene']!=s['scene_id']}

def resident_by_cue(clips,cues):
    """Which loops each distinct cue keeps in SPU, and what that costs."""
    out={}
    for c in cues:
        key=f"{c['name']} (mask {c['mask']})"
        stems=[i for i in range(len(clips)) if c['mask']>>i&1]
        out.setdefault(key,{'channels':[clips[i]['source_channel'] for i in stems],
            'spu_bytes':sum(clips[i]['spu_bytes'] for i in stems),'scenes':[]})['scenes'].append(c['source_scene'])
    return out

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
    edges=gate_edges(ROOT)
    ring_base=tail_base(ROOT,TAIL_BANKS[0])-MUSIC_RING_BYTES
    clips,cues=assemble(report,ROOT,transitions,snapshots,sfx['end'],ring_base,edges)
    out=ROOT/'data/ambience';out.mkdir(parents=True,exist_ok=True)
    work=ROOT/'.hkpsx/ambience';work.mkdir(parents=True,exist_ok=True)
    for i,c in enumerate(clips):
        path=out/f'clip_{i}.adpcm';payload=c.pop('payload');path.write_bytes(payload)
        c['path']=rel(path);quality_path=work/f'clip_{i}.adpcm';quality_path.write_bytes(payload)
        c['quality']=decoder_quality(c,quality_path)
    manifest=ROOT/'data/ambience.rs';manifest.write_text(rust_manifest(clips,cues,ring_base))
    end=max(c['spu_address']+c['spu_bytes'] for c in clips)
    result={'format':'raw-psx-adpcm-loops-v1','clips':clips,'cues':cues,
        'sfx_reservation':sfx,'total_bytes':sum(c['byte_len']for c in clips),'spu_bytes':sum(c['spu_bytes']for c in clips),'ram_cache_bytes':sum(c['byte_len']for c in clips if c['spu_bytes']<c['byte_len']),'spu_start':SPU_START,'spu_end':end,
        # Free between ambience's widest resident set and the first bank
        # stacked above it, which is where those banks are actually cooked.
        'spu_free_bytes':ring_base-end,'spu_ceiling':ring_base,
        'music_ring':{'base':ring_base,'bytes':MUSIC_RING_BYTES,'voice':MUSIC_VOICE},
        'gate_edges':sorted(edges),'resident_by_cue':resident_by_cue(clips,cues),
        'spu_tail_reserved_bytes':tail['reserved_bytes'],'spu_tail_banks':tail['banks'],
        'spu_tail_bases':{name:tail_base(ROOT,name)for name in TAIL_BANKS},
        'resident_atmos_channels':list(CHANNELS),'voices':list(VOICES),
        'music_voice':MUSIC_VOICE,'pool_voices':list(POOL_VOICES),
        'stems_live_across_a_transition':pool_pressure(cues),
        'voice_pool_scope':f'Ambience owns voices {VOICES[1]}..{VOICES[-1]} as a pool: one is taken when a stem keys on '
            'and returned once it has finished fading out, so the resident set is bounded by how many stems '
            f'can be audible at once rather than by how many clips there are. Voice {MUSIC_VOICE} is area music\'s.',
        'resident_atmos_rates':{str(ch):atmos_rate(ch)for ch in CHANNELS},
        'clamped_boosts':{c['source_scene']:c['clamped_boost_db']for c in cues if c['clamped_boost_db']},
        'clamped_boost_scope':f'Source Atmos gains above unity play at full scale instead. Refused above {BOOST_CEILING_DB} dB.',
        'source_report_sha256':sha(a.source_report),'source_method_sha256':sha(methods),
        'tool_sha256':sha(Path(__file__)),'rust_manifest_sha256':sha(manifest),
        'mixer_scope':'Source internal Atmos group and parent gains; downstream output mixers/player settings separate.',
        'transport':f'All {len(clips)} loops load whole into SPU at the scene gate that first needs them, into addresses shared by clips no cue or gate keeps resident together; a clip no admitted scene plays never loads. Gate loads only. Sector padding is outside checksum/byte_len.',
        'omitted_atmos_channels':'Every source atmos channel outside resident_atmos_channels; cook_music records them per scene in ambience_omitted_channels and the scene cooks a zero mask.'}
    drift=tail_drift(ROOT,end);result['spu_tail_drift']=drift
    dump(ROOT/'.hkpsx/ambience.json',result)
    print(f'{len(clips)} loops:',result['total_bytes'],'bytes, widest resident set ends',hex(end),'free',result['spu_free_bytes'],
          f"below the music ring at {result['spu_ceiling']:#x}")
    for name,cue in result['resident_by_cue'].items():print(f"  {name}: channels {cue['channels']}, {cue['spu_bytes']} SPU bytes")
    if drift:
        # The bank is written either way; what is stale is the tail above it,
        # and saying so beats leaving the next cook to find the overlap.
        for name,move in drift.items():
            print(f'  {name} declares base {move["declared"]:#x} and must be {move["required"]:#x}; re-run its cook')
        raise SystemExit('SPU tail banks no longer abut ambience')
if __name__=='__main__':main()
