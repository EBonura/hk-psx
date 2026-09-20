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
# It cannot be sized once for the game. The AudioManager declares 16 atmos
# channels and the 456 catalogue scenes that carry an atmos cue name all 16
# between them, which at 8 kHz mono is about 1.4 MB against 512 KB of SPU RAM,
# on six voices; every other SPU voice belongs to another bank. So the resident
# set is a per-disc budget decision, and adding a channel costs three things
# worth checking before editing this line:
#   - SPU RAM, against the ceiling host/ambience.py computes (the Focus and
#     Runner banks sit above ambience and move up with it),
#   - one of ambience's six voices, which host/ambience.py refuses to exceed,
#   - WORLD.PAK chunk ids, because host/pack_scenes.py numbers the atlas and
#     metadata chunks after the ambience clips.
# A channel left out is not an error: the scene records it in
# ambience_omitted_channels and cooks a zero mask, which the guest handles by
# fading the previous stems out and keying nothing on. Three admitted scenes
# are silent that way today: Fungus1_01 (Greenpath, channel 7) and
# Crossroads_46b and Crossroads_50 (MiscWind, channel 15).
RESIDENT_ATMOS_CHANNELS=(0,1,3,4,5,6)

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
    return {'scenes':scenes,'music_cues':list(cues.values()),'audio_manager':source.sid(managers[0])},clips


def compile_encoder(out):
    tool=out/'spu_encode';subprocess.run(['clang','-O3','-Wall','-Wextra','-Werror',str(ROOT/'host/spu_encode.c'),'-lm','-o',str(tool)],check=True)
    return tool

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


def cook_clip(source,obj,out,encoder,rate,channels):
    sid=source.sid(obj);tree=source.read(obj);folder=out/sid.replace(':','-');folder.mkdir(exist_ok=True)
    wav,source_identity=decoded_source(source,obj,folder)
    with wave.open(str(wav),'rb')as reader:
        source_frames=reader.getnframes();source_rate=reader.getframerate();source_channels=reader.getnchannels()
        if reader.getsampwidth()!=2:raise ValueError('expected source PCM16')
    version=subprocess.check_output(['ffmpeg','-version'],text=True).splitlines()[0]
    raw=folder/f'{rate}-{channels}.s16le'
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
        # quality metric. Its history rounding differs slightly from SPU-style
        # rounded integer prediction, so sample equality is not claimed.
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
    all_ambient=set(sid for a in ambient for sid in a['clips'])
    return {'stream_profiles':candidates,'ambience':ambient,
        'all_six_ambient_loops_8000_mono_bytes':sum(by[(sid,8000,1)]['bytes']for sid in all_ambient),
        'all_six_ambient_loops_10000_mono_bytes':sum(by[(sid,10000,1)]['bytes']for sid in all_ambient),
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
    ambience={a['clip'] for scene in report['scenes'] for manager in scene['managers'] for a in manager['ambience']}
    for sid,obj in sorted(objects.items()):
        print('AUDIO',sid,s.read(obj)['m_Name'],flush=True)
        # 4000 exists for the long cave bed, whose 8 kHz copy does not fit SPU.
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
    report['tool_hashes']={name:sha(ROOT/'host'/name)for name in ('source.py','cook_music.py','spu_encode.c')}
    dump(out/'provenance.json',report);dump(out/'capacity.json',report['capacity'])
    print(json.dumps(report['capacity'],indent=2),flush=True)
if __name__=='__main__':main()
