"""Read-only source music/ambience extraction and bounded SPU capacity experiment.

No guest build integration: complete clips remain ignored host assets. The
transport must set ADPCM loop/chunk flags and arbitrate the single CD drive.
"""
import argparse,array,hashlib,io,json,math,shutil,struct,subprocess,sys,wave
from pathlib import Path
from source import ROOT,Source,dump
from scene import Scene
from breakables import _components,collider_polygons


# Atmos channels with a resident guest loop. This tuple is the whole decision:
# host/ambience.py derives the clip table length, the cue gain arrays, the SPU
# voices and the streamed clip's index from it, and the guest derives its own
# half from the cooked table.
#
# Eight of the sixteen, one short of covering the game on purpose. The
# AudioManager declares 16 atmos channels and the 456 catalogue scenes that
# carry an atmos cue name all 16 between them, but 401 of those scenes reduce
# to only 18 distinct sets of audibly enabled channels, and the smallest set
# meeting all 18 is eight. There are exactly two such covering sets,
# {0,1,5,7,9,10,14,15} and {0,3,5,7,9,10,14,15}, and this is neither of them;
# see the trade recorded below. What this set leaves silent is Deepnest's 18
# scenes, against the 78 the six-channel set it replaces left silent: 33 City
# of Tears rain, 18 Waterways, 18 Deepnest, 9 Fog Canyon.
#
# Moving or widening the set still costs three things worth checking before
# editing this line:
#   - SPU RAM, against the ceiling host/ambience.py computes (the Focus and
#     Runner banks sit above ambience and move with it),
#   - one of the five pooled voices, for as long as the stem is audible. A long
#     set is no longer what host/ambience.py refuses; what it refuses is a set
#     where more stems than that can be audible at the same moment.
#   - WORLD.PAK chunk ids, because host/pack_scenes.py numbers the atlas and
#     metadata chunks after the ambience clips.
# A channel left out is not an error: the scene records it in
# ambience_omitted_channels and cooks a zero mask, which the guest handles by
# fading the previous stems out and keying nothing on.
#
# Channel 4 rather than 14, which is the whole difference between this set and
# a covering one, and it is not a budget concession: both price at exactly
# 280,928 bytes of SPU against 297,312 available, because whichever channel
# streams costs the ring's 16,384 and nothing more. What 14 would buy is
# Deepnest's 18 catalogue scenes. What it costs is two things the disc has now:
#   - channel 4, cave_noises, plays at 0.0 dB beside cave_wind_loop in the Cave
#     cue, which is Tutorial_01 and 40 admitted Crossroads rooms. Dropping it
#     takes a full-gain layer out of 41 scenes a player can walk through today
#     to prepare 18 that are not on the disc and will not be for a long time.
#   - no admitted scene enables 14, so with 14 streamed the SPU ring never
#     starts on this disc and the streaming transport, the most hardware
#     sensitive part of the audio system, goes unexercised. Keeping 4 keeps it
#     running in every Cave scene.
# This is a cook-time knob so that it can be revisited: when Deepnest is
# admitted its own scenes enable 14, and the trade inverts.
#
# Channel 1 rather than 3, the same question the covering sets pose. Dirtmouth
# is admitted and reachable and its Surface cue plays both, channel 1 at 0.0 dB
# and channel 3 at -2.29 dB. Only one fits, and 1 is both the louder and the
# one that survives the 4 kHz cook: 25.34 dB at 8 kHz falling to 22.03 at 4,
# against 33.58 falling to 21.07 for 3, which is the worst rate penalty of any
# of the sixteen. Room_shop, the only other admitted scene on a Dirtmouth cue,
# plays both at exactly -10.0 dB and cannot tell them apart.
#
# What the raise costs over the admitted 60, measured by name rather than
# inferred: two scenes, Town and Room_shop, each losing the Dirtmouth loop
# above. The other 58 keep every stem they had. What every admitted scene pays
# instead is rate, priced beside RESIDENT_ATMOS_RATES below.
#
# Two earlier swaps this supersedes, kept for the measurement in them. Channel
# 6 gave way to 7 so Greenpath could be heard, and channel 5 then gave way to
# 15 for the 113 MiscWind scenes; both were priced on the same reading, that
# the rain layers' highest enabled gain across every admitted scene is 16 of
# 16,383, about -60.2 dB. Channel 5 is resident again here and 6 is still out,
# and that reading is why 6 costs nothing: no admitted scene can hear it.
#
# Two measured places to get further margin back, neither taken here:
#   - the stream's SPU ring is still sized for the 8 kHz era: two 8,192-byte
#     halves last 3.58 s at the streamed clip's 4 kHz, not the 1.79 s the
#     transport was written and commented for, so halving it returns 8,192
#     bytes of SPU and 4,096 of main RAM at the cadence the design was
#     validated at,
#   - ambience starts at 0x18000 while the Geo bank above 0x14000 ends at
#     0x17640, so 2,496 bytes sit unused between them; taking them couples the
#     ambience base to Geo's exact size, which is why they are still there.
RESIDENT_ATMOS_CHANNELS=(0,1,4,5,7,9,10,15)

# Cook rate per resident channel. Eight loops do not fit SPU at 8 kHz, so 4 kHz
# is the whole set's rate and the default below is a budget decision rather than
# a per-clip one. RESIDENT_ATMOS_RATES is where a channel that earns a rate of
# its own goes; nothing does today.
#
# Lower is not always worse: PSX ADPCM fits a 4-bit residual per sample whatever
# the rate, so a clip whose energy is already below 2 kHz spends half its blocks
# describing an empty band at 8 kHz. What each resident channel pays for 4 kHz,
# as the SNR of the cooked payload against its own resampled source, 8 kHz then
# 4 kHz:
#    0 cave_wind_loop           24.52 -> 21.59      7 green_path_atmos_loop 25.77 -> 20.84
#    1 dirtmouth_wind_loop_a    25.34 -> 22.03      9 fog_canyon_atmos_loop 23.21 -> 22.76
#    4 cave_noises              30.28 -> 31.05     10 waterways_atmos_loop  26.24 -> 25.23
#    5 ruins_rain_indoor_loop   23.41 -> 19.80     15 cave_atmos_misc_3     16.70 -> 24.47
# Two channels gain rather than pay. Channel 15 is 99.999% below 2 kHz and is
# better at 4 kHz by 7.77 dB, the largest gap of any of the sixteen and the
# reason it is affordable at all; channel 4 is 99.94% below 2 kHz and better by
# 0.77. Channel 4's rate is nearly free in SPU either way, because a streamed
# clip only ever occupies the ring, but 8 kHz would double its RAM cache to
# 199,872 bytes to make it measurably worse, so 4 kHz wins twice.
#
# Channel 7 pays the most, 4.93 dB, and it is the only audible stem in all 31
# Greenpath scenes including the 14 admitted ones, which makes it the largest
# remaining audible regression in the playable world and the first candidate if
# margin is ever spent on rate. It does not fit: 8 kHz costs 22,864 more bytes
# against 16,384 free, over the ceiling by 6,480. The only resident channel
# that does fit at 8 kHz is 5, at 15,920, and it would leave 464 bytes and buy
# 3.61 dB for the 33 City of Tears rain scenes, none of which is admitted and
# in all of which channel 5 is the only audible stem, while in every admitted
# scene it is enabled at 16 of 16,383. Spending the whole margin on scenes a
# player cannot reach is the wrong way round, so neither is taken.
#
# The default used to read "4 kHz if it is the streamed channel", which was true
# by accident: the cave bed was moved to 4 kHz because its 8 kHz copy would not
# fit SPU, and only afterwards measured as the better of the two. Rate and
# streaming are separate decisions and the code does not derive one from the
# other.
RESIDENT_ATMOS_RATES={}
DEFAULT_ATMOS_RATE=4000

def atmos_rate(channel):
    """The cook rate for one resident atmos channel."""
    return RESIDENT_ATMOS_RATES.get(channel,DEFAULT_ATMOS_RATE)

def sha(path):
    with Path(path).open('rb')as stream:return hashlib.file_digest(stream,'sha256').hexdigest()
def source_audio_ref(tree):
    """Unity6 uses AudioResource while legacy m_audioClip is often null."""
    for name in ('m_Resource','m_audioClip'):
        value=tree.get(name)
        if isinstance(value,dict)and value.get('m_PathID'):return value
    raise ValueError('AudioSource has no direct audio resource')
def source_snapshot(source,snapshot_obj,group_obj):
    snap=source.read(snapshot_obj);group=source.read(group_obj)
    mixer_obj=source.ref(group_obj.assets_file,group['m_AudioMixer']);mixer=source.read(mixer_obj);constant=mixer['m_MixerConstant']
    if source.sid(source.ref(snapshot_obj.assets_file,snap['m_AudioMixer']))!=source.sid(mixer_obj):
        raise ValueError('snapshot and group belong to different mixers')
    index=constant['snapshotGUIDs'].index(snap['m_SnapshotID']);values=constant['snapshots'][index]['values']
    group_index=constant['groupGUIDs'].index(group['m_GroupID']);chain=[]
    while group_index>=0:
        g=constant['groups'][group_index]
        chain.append({'group_index':group_index,'volume_db':values[g['volumeIndex']],
                      'pitch':values[g['pitchIndex']],'mute':g['mute'],'solo':g['solo']})
        group_index=g['parentConstantIndex']
        if len(chain)>64:raise ValueError('cyclic mixer group parents')
    return {'snapshot_source':source.sid(snapshot_obj),'snapshot_name':snap['m_Name'],
        'mixer_source':source.sid(mixer_obj),'group_source':source.sid(group_obj),'group_name':group['m_Name'],
        'internal_volume_db':sum(g['volume_db']for g in chain),'chain':chain,
        'effects':constant['effects'],'output_group':mixer['m_OutputGroup'],
        'scope':'Selected mixer snapshot only; output mixers/player settings remain separate'}


def inventory(source):
    resource=source.file('resources.assets')
    managers=[o for o in resource.objects.values()if o.type.name=='MonoBehaviour'and source.typename(o)=='AudioManager']
    if len(managers)!=1:raise ValueError('expected one persistent AudioManager')
    manager=source.read(managers[0]);clips={};cues={};scenes=[]
    def clip(ref_file,ref):
        obj=source.ref(ref_file,ref)
        if obj.type.name!='AudioClip':raise ValueError('audio resource is not an AudioClip')
        clips[source.sid(obj)]=obj;return source.sid(obj)
    def cue(ref_file,ref):
        obj=source.ref(ref_file,ref);sid=source.sid(obj)
        if sid in cues:return sid
        if source.typename(obj)!='MusicCue':raise ValueError('music reference is not a MusicCue')
        tree=source.read(obj);record={'source':sid,'name':tree['m_Name'],'channels':[],
            'event':tree['originalMusicEventName'],'track':tree['originalMusicTrackNumber'],'alternatives':[]}
        cues[sid]=record
        for i,c in enumerate(tree['channelInfos']):
            record['channels'].append({'channel':i,'sync':c['sync'],
                'clip':clip(obj.assets_file,c['clip'])if c['clip']['m_PathID']else None})
        for alternative in tree['alternatives']:
            record['alternatives'].append({'player_data_bool':alternative['PlayerDataBoolKey'],
                'cue':cue(obj.assets_file,alternative['Cue'])})
        return sid
    # Which clip each resident channel plays, read once off the persistent
    # AudioManager rather than off whichever admitted scene happens to enable
    # the channel. The two are the same answer, but only this one exists for a
    # channel no admitted scene enables, and the current set has two of those:
    # nothing in the 60 is in Fog Canyon or the Waterways, so channels 9 and 10
    # are resident for scenes not yet admitted. Reading the table from the
    # scenes made the resident set silently uncookable the moment it named a
    # channel ahead of the world.
    resident=[]
    for channel in RESIDENT_ATMOS_CHANNELS:
        audio_obj=source.ref(resource,manager['atmosSources'][channel]);audio=source.read(audio_obj)
        resident.append({'channel':channel,'audio_source':source.sid(audio_obj),
            'clip':clip(audio_obj.assets_file,source_audio_ref(audio)),
            'loop':audio['Loop'],'pitch':audio['m_Pitch'],'volume':audio['m_Volume'],
            'play_on_awake':audio['m_PlayOnAwake']})
    from quality import SCENE_FILES
    for file in SCENE_FILES:
        scene=Scene(source,file);row={'scene_file':file,'managers':[],'music_regions':[],'music_fsm_actions':[]}
        for index,(kind,tree)in scene.objects.items():
            if kind=='SceneManager':
                record={'source':f'{file}:{index}','music_cue':cue(scene.file,tree['musicCue'])if tree['musicCue']['m_PathID']else None,
                    'music_delay':tree['musicDelayTime'],'music_transition':tree['musicTransitionTime'],'ambience':[]}
                if tree['musicSnapshot']['m_PathID']:
                    snap=source.ref(scene.file,tree['musicSnapshot']);record['music_snapshot']={'source':source.sid(snap),'name':source.read(snap)['m_Name']}
                atmos_obj=source.ref(scene.file,tree['atmosCue']);atmos=source.read(atmos_obj)
                record['atmos_cue']={'source':source.sid(atmos_obj),'name':atmos['m_Name']}
                snapshot=source.ref(atmos_obj.assets_file,atmos['snapshot'])
                record['ambience_omitted_channels']=[]
                for channel,enabled in enumerate(atmos['isChannelEnabled']):
                    if not enabled:continue
                    if channel not in RESIDENT_ATMOS_CHANNELS:
                        # The guest keeps one loop per resident channel; a stem
                        # outside that set is an explicit omission for the scene.
                        record['ambience_omitted_channels'].append(channel);continue
                    audio_obj=source.ref(resource,manager['atmosSources'][channel]);audio=source.read(audio_obj)
                    group=source.ref(audio_obj.assets_file,audio['OutputAudioMixerGroup'])
                    record['ambience'].append({'channel':channel,'audio_source':source.sid(audio_obj),
                        'clip':clip(audio_obj.assets_file,source_audio_ref(audio)),
                        'loop':audio['Loop'],'pitch':audio['m_Pitch'],'volume':audio['m_Volume'],
                        'play_on_awake':audio['m_PlayOnAwake'],'snapshot':source_snapshot(source,snapshot,group)})
                row['managers'].append(record)
            elif kind=='MusicRegion':
                gid=tree['m_GameObject']['m_PathID'];record={'source':f'{file}:{index}','active':scene.active(gid),'enabled':bool(tree['m_Enabled']),
                    'name':scene.gos[gid]['m_Name'],'dirtmouth_condition':bool(tree['dirtmouth']),'mines_delay':bool(tree['minesDelay']),
                    'enter_seconds':tree['enterTransitionTime'],'dirtmouth_first_cue_fade_seconds':1.0 if tree['dirtmouth'] else None,'exit_seconds':tree['exitTransitionTime'],'polygons':[]}
                for field in ('enterMusicCue','exitMusicCue'):
                    record[field]=cue(scene.file,tree[field])if tree[field]['m_PathID']else None
                for field in ('enterMusicSnapshot','exitMusicSnapshot'):
                    if not tree[field]['m_PathID']:record[field]=None;continue  # authored regions may leave a snapshot unset
                    obj=source.ref(scene.file,tree[field]);record[field]={'source':source.sid(obj),'name':source.read(obj)['m_Name']}
                for _,typ,col in _components(scene,gid):
                    if typ.endswith('Collider2D')and col['m_Enabled']:
                        record['polygons'].extend(collider_polygons(scene,gid,typ,col))
                row['music_regions'].append(record)
            elif kind=='PlayMakerFSM':
                for state in tree['fsm']['states']:
                    for action in state['actionData']['actionNames']:
                        if 'Music'in action:row['music_fsm_actions'].append({'source':f'{file}:{index}','state':state['name'],'action':action})
        scenes.append(row)
    return {'scenes':scenes,'resident_atmos':resident,'music_cues':list(cues.values()),
        'audio_manager':source.sid(managers[0])},clips


def compile_encoder(out):
    """The ADPCM encoder command (host/spu_encode.py over the SDK's
    psx-audio-cook). `out` is kept for callers; nothing is written there."""
    import spu_cook
    spu_cook.binary()
    return ROOT/'host/spu_encode.py'

def decoded_source(source,obj,folder):
    tree=source.read(obj);ref=tree['m_Resource'];resource=(source.directory/ref['m_Source']).resolve()
    if not resource.is_relative_to(source.directory.resolve()):raise ValueError('audio resource escapes Windows source')
    with resource.open('rb')as reader:
        reader.seek(ref['m_Offset']);encoded=reader.read(ref['m_Size'])
    if len(encoded)!=ref['m_Size']:raise ValueError('truncated source audio resource')
    identity={'clip_metadata_sha256':hashlib.sha256(json.dumps(tree,sort_keys=True).encode()).hexdigest(),
              'encoded_resource_sha256':hashlib.sha256(encoded).hexdigest()}
    wav=folder/'source.wav';stamp=folder/'source-wav.json';valid=False
    if wav.exists()and stamp.exists():
        previous=json.loads(stamp.read_text())
        valid=all(previous.get(k)==v for k,v in identity.items())and previous.get('wav_sha256')==sha(wav)
    if not valid:
        values=obj.read().samples
        if len(values)!=1:raise ValueError('expected one decoded sample resource')
        wav.write_bytes(next(iter(values.values())))
        dump(stamp,dict(identity,wav_sha256=sha(wav)))
    return wav,identity


def cook_clip(source,obj,out,encoder,rate,channels,resampler='ffmpeg'):
    sid=source.sid(obj);tree=source.read(obj);folder=out/sid.replace(':','-');folder.mkdir(exist_ok=True)
    wav,source_identity=decoded_source(source,obj,folder)
    with wave.open(str(wav),'rb')as reader:
        source_frames=reader.getnframes();source_rate=reader.getframerate();source_channels=reader.getnchannels()
        if reader.getsampwidth()!=2:raise ValueError('expected source PCM16')
    version=subprocess.check_output(['ffmpeg','-version'],text=True).splitlines()[0]
    raw=folder/f'{rate}-{channels}.s16le'
    if resampler=='sdk':
        # The SDK's shared resampler (host/spu_cook.py resample); mono only.
        import spu_cook
        if channels!=1:raise ValueError('the SDK resampler path is mono')
        raw.write_bytes(array.array('h',spu_cook.resample(wav.read_bytes(),rate)).tobytes())
        version='psx_audio_cook::resample::Sinc (SDK shared resampler)'
    else:
        subprocess.run(['ffmpeg','-v','error','-y','-i',str(wav),'-ar',str(rate),'-ac',str(channels),'-f','s16le',str(raw)],check=True)
    pcm=array.array('h');pcm.frombytes(raw.read_bytes())
    if sys.byteorder!='little':pcm.byteswap()
    if len(pcm)%channels:raise ValueError('partial PCM frame')
    frames=len(pcm)//channels;planes=[]
    for channel in range(channels):
        mono=pcm[channel::channels];mono_path=folder/f'{rate}-{channels}-ch{channel}.s16le'
        if sys.byteorder!='little':mono.byteswap()
        mono_path.write_bytes(mono.tobytes());encoded=mono_path.with_suffix('.adpcm')
        metric=json.loads(subprocess.check_output([str(encoder),str(mono_path),str(encoded)],text=True))
        data=encoded.read_bytes()
        if len(data)!=math.ceil(frames/28)*16 or any(data[i+1]for i in range(0,len(data),16)):raise ValueError('invalid encoded payload')
        # Independent FFmpeg decode confirms valid framing and records an external
        # quality metric. Its history rounding differs slightly from the SPU's,
        # so sample equality is not claimed.
        vag=encoded.with_suffix('.vag');vag.write_bytes(b'VAGp'+struct.pack('>4I',0x20,0,len(data),rate)+bytes(28)+data)
        decoded=encoded.with_suffix('.decoded.s16le')
        subprocess.run(['ffmpeg','-v','error','-y','-i',str(vag),'-f','s16le',str(decoded)],check=True)
        recon=array.array('h');recon.frombytes(decoded.read_bytes())
        if sys.byteorder!='little':recon.byteswap();mono.byteswap()
        if len(recon)!=math.ceil(frames/28)*28:raise ValueError('external decode frame mismatch')
        error=sum((int(a)-b)**2 for a,b in zip(mono,recon));signal=sum(int(a)**2 for a in mono)
        planes.append({'path':str(encoded.relative_to(ROOT)),'bytes':len(data),'sha256':sha(encoded),
            'encoder_metric':metric,'ffmpeg_snr_db':10*math.log10(signal/error)if signal and error else None})
    return {'source':sid,'name':tree['m_Name'],'source_metadata':tree,'source_identity':source_identity,'source_wav_sha256':sha(wav),
        'source_pcm_frames':source_frames,'source_pcm_rate':source_rate,'source_pcm_channels':source_channels,
        'rate':rate,'spu_pitch':round(rate*4096/44100),'spu_actual_rate':round(rate*4096/44100)*44100/4096,'channels':channels,'frames':frames,'seconds':frames/rate,'padding_samples':(-frames)%28,
        'bytes':sum(p['bytes']for p in planes),'bytes_per_second':rate*channels*16/28,'planes':planes,
        'conversion':version,'flags':'All payload flags zero; runtime transport must install loop/chunk boundaries',
        'loop_boundary':'Full-clip sample count preserved in metadata; final ADPCM block has at most27 zero samples'}

def capacity(report,clips):
    candidates=[]
    for rate,channels in ((22050,2),(22050,1),(11025,1)):
        bps=rate*channels*16/28
        candidates.append({'rate':rate,'channels':channels,'bytes_per_second':bps,
            'fraction_of_double_speed_2048_sector_bandwidth':bps/(150*2048),
            'spu_ring_256k_seconds':256*1024/bps,'spu_half_128k_seconds':128*1024/bps,
            'spu_ring_384k_seconds':384*1024/bps,'spu_half_192k_seconds':192*1024/bps})
    ambient=[]
    by={(x['source'],x['rate'],x['channels']):x for x in clips}
    for scene in report['scenes']:
        for manager in scene['managers']:
            ids=[x['clip']for x in manager['ambience']]
            ambient.append({'scene':scene['scene_file'],'clips':ids,
                'source_snapshot_internal_db':[x['snapshot']['internal_volume_db']for x in manager['ambience']],
                'mono10000_complete_loop_bytes':sum(by[(sid,10000,1)]['bytes']for sid in ids),
                'mono8000_complete_loop_bytes':sum(by[(sid,8000,1)]['bytes']for sid in ids),
                'stereo22050_combined_bps':len(ids)*25200,'mono11025_complete_loop_bytes':sum(by[(sid,11025,1)]['bytes']for sid in ids),
                'note':'Each enabled stem retained; very quiet rain is not silently removed. Output mixer/player gains unresolved.'})
    # Every resident loop, not just the ones an admitted scene enables, which
    # is what the bank actually pays for.
    all_ambient={r['clip'] for r in report['resident_atmos']}
    return {'stream_profiles':candidates,'ambience':ambient,
        'all_resident_ambient_loops_8000_mono_bytes':sum(by[(sid,8000,1)]['bytes']for sid in all_ambient),
        'all_resident_ambient_loops_10000_mono_bytes':sum(by[(sid,10000,1)]['bytes']for sid in all_ambient),
        'ram_staging_proposal_bytes':8192,'cd_sector_bytes':2048,'double_speed_sectors_per_second':150,
        'staging_4_sector_fill_seconds':4/150,'spu_capacity_bytes':512*1024,
        'reserved_sdk_low_spu_bytes':0x1010,'separate_sfx_budget_bytes':32*1024,
        'remaining_spu_after_384k_ring_and_sfx':512*1024-0x1010-32*1024-384*1024,
        'integration_status':'Host groundwork only; no music or ambience enabled in guest, no CD audio arbitration or SPU refill implementation',
        'blockers':['SPU IRQ/refill state machine is absent from pinned high-level SDK',
            'Single CD command owner must schedule room and audio extents with refill deadlines',
            '8KiB staging plus code/state needs rechecking against final SFX-linked main RAM map',
            'Keep enabled ambience playing across cue changes; exact phase/mixer transitions/player settings not yet reproduced',
            'Dirtmouth music trigger is outside current Town coverage']}

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,default=ROOT/'.hkpsx/music');a=p.parse_args();out=a.output if a.output.is_absolute() else ROOT/a.output
    # Compare resolved paths so a symlinked .hkpsx (worktree setups) passes, but
    # keep the unresolved path so report entries stay relative to ROOT.
    if not out.resolve().is_relative_to((ROOT/'.hkpsx').resolve()):raise ValueError('music assets must remain ignored under .hkpsx')
    out.mkdir(parents=True,exist_ok=True);s=Source();report,objects=inventory(s);encoder=compile_encoder(out)
    cooked=[]
    ambience={r['clip'] for r in report['resident_atmos']}
    for sid,obj in sorted(objects.items()):
        print('AUDIO',sid,s.read(obj)['m_Name'],flush=True)
        # Every resident clip is cooked at both 8000 and 4000 whatever
        # RESIDENT_ATMOS_RATES currently says, so re-rating a channel is a
        # host/ambience.py run rather than a full source re-conversion. 10000
        # feeds the capacity table below and nothing in the bank.
        for rate,channels in ((22050,2),(11025,1))+(((10000,1),(8000,1),(4000,1))if sid in ambience else ()):
            cooked.append(cook_clip(s,obj,out,encoder,rate,channels))
    report['clips']=cooked;report['capacity']=capacity(report,cooked)
    from inspect_il import inspect
    types=['AudioManager','SceneManager','MusicRegion','MusicCue','AudioLoopMaster','<BeginApplyAtmosCue>d__12','<BeginApplyMusicCue>d__14','<FadeIn>d__14']
    methods=out/'source-methods.il';methods.write_text(inspect(s.directory/'Managed/Assembly-CSharp.dll',types))
    report['source_methods']={'path':str(methods.relative_to(ROOT)),'sha256':sha(methods),'types':types}
    report['verified_behaviors']=['MusicRegion accepts Hero layer9; Dirtmouth first-cue fade1s, already-Dirtmouth fade3s, exit6s', 'ApplyAtmosCue starts enabled nonplaying channels and stops disabled channels after snapshot transition; already-playing channel phase is retained', 'MusicCue resolves conditional nymmInTown alternative before comparing current cue identity']
    inputs=set(s.files)|{str(p.relative_to(s.directory))for p in (s.directory/'Managed').glob('*.dll')}
    for obj in objects.values():inputs.add(s.read(obj)['m_Resource']['m_Source'])
    report['source_directory']=str(s.directory);report['inputs']={name:{'sha256':sha(s.directory/name),'bytes':(s.directory/name).stat().st_size}for name in sorted(inputs)}
    report['tool_hashes']={name:sha(ROOT/'host'/name)for name in ('source.py','cook_music.py','spu_encode.py','spu_cook.py')}
    dump(out/'provenance.json',report);dump(out/'capacity.json',report['capacity'])
    print(json.dumps(report['capacity'],indent=2),flush=True)
if __name__=='__main__':main()
