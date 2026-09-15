"""Cook complete Focus sounds into one startup-loaded SPU bank.

Windows retail inputs are read-only. All extracted payloads/provenance remain
ignored. Charge uses the sustained-sound 8 kHz category; heal uses 22.05 kHz.
"""
import hashlib,json,subprocess
from source import ROOT,Source,dump
from cook_music import compile_encoder,cook_clip,sha
from ambience import fnv,loop_payload,validate_blocks
from focus import action_fields

SPU_BASE=0x5EA00
SPU_END=0x80000
CHARGE_RATE=8000
HEAL_RATE=22050
GAIN=5461
# These assemblies were inspected for AudioPlay, FadeAudio.OnExit and pooled
# PlayAudioAndRecycle semantics. A changed executable requires a fresh audit.
ASSEMBLIES={
    'Managed/Assembly-CSharp.dll':'e9048ef6a633970f735e01ec166d3959f610eaea7a88d827d48d67b1e5fb87bd',
    'Managed/PlayMaker.dll':'0ef0e7829d125e1f632c8a189260ec6c6882630be6932c8c7ae032efbc53469a',
}

def fields(data,index):
    result=action_fields(data,index);start=data['actionStartIndex'][index]
    end=data['actionStartIndex'][index+1] if index+1<len(data['actionNames']) else len(data['paramName'])
    for i in range(start,end):
        kind=data['paramDataType'][i]
        if kind in (19,24,11):
            result[data['paramName'][i] or str(i)]=data[{19:'fsmGameObjectParams',24:'fsmObjectParams',11:'unityObjectParams'}[kind]][data['paramDataPos'][i]]
    return result

def require(condition,message):
    if not condition:raise ValueError(message)

def literal(value):
    require(not value['useVariable'],'variable-valued Focus audio parameter')
    return value['value']

def charge_target(action):
    target=action.get('gameObject',{})
    return target.get('ownerOption')==1 and target.get('gameObject',{}).get('useVariable') and target['gameObject']['name']=='Charge Audio'

def validate_contract(states,globals_):
    """Validate decoded source actions, preserving start/repeat/fade/stop intent."""
    def actions(state,kind):return [a['fields'] for a in states[state] if a['enabled'] and a['action'].rsplit('.',1)[-1]==kind]
    starts=actions('Focus Start','AudioPlay')
    require(len(starts)==1 and charge_target(starts[0]),'Focus charge start changed')
    require(literal(starts[0]['volume'])==1 and literal(starts[0]['oneShotClip'])=={'m_FileID':0,'m_PathID':0},'Focus charge is no longer normal Play at unit gain')
    heals=actions('Focus Heal','AudioPlayerOneShotSingle')
    require(len(heals)==1,'Focus heal action changed');heal=heals[0]
    for key in ('volume','pitchMin','pitchMax'):require(literal(heal[key])==1,'Focus heal gain or pitch changed')
    require(literal(heal['delay'])==0,'Focus heal delay changed')
    require(literal(heal['audioClip'])=={'m_FileID':0,'m_PathID':1260} and literal(heal['audioPlayer'])=={'m_FileID':0,'m_PathID':4126},'Focus heal clip/prefab changed')
    fades=[]
    for state in ('Focus Cancel','Focus Get Finish'):
        found=actions(state,'FadeAudio');require(len(found)==1 and charge_target(found[0]),'Focus fade target changed')
        fade=found[0];require(literal(fade['startVolume'])==1 and literal(fade['endVolume'])==0,'Focus fade range changed')
        seconds=literal(fade['time']);require(abs(seconds-0.33)<1e-6,'Focus fade duration changed');fades.append(seconds)
    for state in ('Regain Control','Cancel Some','FSM Cancel','Cancel All'):
        require(any(charge_target(a) for a in actions(state,'AudioStop')),'Focus stop missing from '+state)
    for state in ('Focus','Full HP?','Focus Heal'):
        for kind in ('AudioPlay','AudioStop','FadeAudio'):
            require(not any(charge_target(a) for a in actions(state,kind)),'Focus repeat now changes charging voice')
    for event,state in {'LEAVING SCENE':'Cancel Some','FSM CANCEL':'FSM Cancel','HERO DAMAGED':'Reset Cam Zoom'}.items():
        require(globals_.get(event)==state,'Focus interruption routing changed')
    return round(fades[0]*60)

def source_contract(source):
    file=source.file('resources.assets');obj=file.objects[21207]
    require(source.typename(obj)=='PlayMakerFSM','Focus FSM type changed')
    fsm=source.read(obj)['fsm'];require(fsm['name']=='Spell Control','Focus FSM identity changed')
    states={s['name']:[{'action':name,'enabled':s['actionData']['actionEnabled'][i],'fields':fields(s['actionData'],i)} for i,name in enumerate(s['actionData']['actionNames'])] for s in fsm['states']}
    globals_={t['fsmEvent']['name']:t['toState'] for t in fsm['globalTransitions']}
    fade_ticks=validate_contract(states,globals_)
    transitions={s['name']:{t['fsmEvent']['name']:t['toState'] for t in s['transitions']} for s in fsm['states']}
    for state,event,target in [('Reset Cam Zoom','FINISHED','Cancel All'),('Focus Heal','WAIT','Full HP?'),('Full HP?','FINISHED','Focus')]:
        require(transitions[state].get(event)==target,'Focus continuation changed')
    finds=[a['fields'] for a in states['Init'] if a['enabled'] and a['action'].endswith('.FindChild')]
    require(any(literal(a['childName'])=='Charge Audio' and a['storeResult']['name']=='Charge Audio' and a['gameObject']['gameObject']['name']=='Focus Effects' for a in finds),'Charge Audio child binding changed')
    charge=source.read(file.objects[13927]);heal=source.read(file.objects[13920]);recycle=source.read(file.objects[25641])
    require(charge['m_GameObject']=={'m_FileID':0,'m_PathID':5518} and charge['m_Resource']=={'m_FileID':0,'m_PathID':1160},'charge AudioSource binding changed')
    require(heal['m_GameObject']=={'m_FileID':0,'m_PathID':4126} and recycle['audioSource']=={'m_FileID':0,'m_PathID':13920},'heal pool binding changed')
    for audio,loop in ((charge,True),(heal,False)):
        require(bool(audio['Loop'])==loop and audio['m_Pitch']==1 and audio['m_Volume']==1 and not audio['Mute'],'Focus AudioSource settings changed')
        require(audio['OutputAudioMixerGroup']=={'m_FileID':0,'m_PathID':3829},'Focus Actors mixer routing changed')
    return {'fsm':source.sid(obj),'fade_ticks':fade_ticks,'source_fade_seconds':0.33000001311302185,
        'charge_audio_source':'resources.assets:13927','heal_audio_source':'resources.assets:13920',
        'charge_clip':'resources.assets:1160','heal_clip':'resources.assets:1260',
        'states':{k:states[k] for k in ('Focus Start','Focus Heal','Focus Cancel','Focus Get Finish','Regain Control','Cancel Some','FSM Cancel','Cancel All')},
        'global_transitions':globals_,'charge_source':charge,'heal_source':heal}

def oneshot_payload(data):
    validate_blocks(data)
    require(not any(data[i+1] for i in range(0,len(data),16)),'one-shot input contains transport flags')
    return data+bytes([12,1])+bytes(14)

def assemble(charge,heal,ambience_end):
    require(0<=ambience_end<=SPU_BASE and SPU_BASE%16==0,'Focus bank overlaps ambience')
    charge=loop_payload(charge);heal=oneshot_payload(heal)
    require(SPU_BASE+len(charge)+len(heal)<=SPU_END,'Focus bank exceeds SPU RAM')
    return charge+heal,len(charge)

def main():
    out=ROOT/'.hkpsx/focus-audio';out.mkdir(parents=True,exist_ok=True)
    source=Source();contract=source_contract(source)
    inputs={name:sha(source.directory/name) for name in ('globalgamemanagers','resources.assets','resources.resource',*ASSEMBLIES)}
    require(all(inputs[name]==digest for name,digest in ASSEMBLIES.items()),'Focus action implementation changed; re-audit assembly semantics')
    ambience_path=ROOT/'.hkpsx/ambience.json';ambience=json.loads(ambience_path.read_text())
    require(sha(ROOT/'data/ambience.rs')==ambience['rust_manifest_sha256'],'ambience manifest/provenance mismatch')
    ambience_end=ambience['spu_end'];require(ambience_end<=SPU_BASE,'Focus SPU reservation overlaps ambience')
    code={name:sha(ROOT/'host'/name) for name in ('focus_audio.py','focus.py','source.py','cook_music.py','ambience.py','spu_encode.c')}
    version=subprocess.check_output(['ffmpeg','-version'],text=True).splitlines()[0]
    identity={'inputs':inputs,'code':code,'ambience_sha256':sha(ambience_path),'conversion':version}
    report_path=ROOT/'.hkpsx/focus-audio.json';payload_path=ROOT/'data/focus-audio.adpcm';manifest=ROOT/'data/focus-audio.rs'
    if report_path.exists() and payload_path.exists() and manifest.exists():
        old=json.loads(report_path.read_text())
        if old.get('identity')==identity and old.get('sha256')==sha(payload_path) and old.get('manifest_sha256')==sha(manifest):
            print('Focus audio cache verified');return
    encoder=compile_encoder(out);file=source.file('resources.assets');profiles=[]
    for pid,rate,name in ((1160,CHARGE_RATE,'focus_health_charging'),(1260,HEAL_RATE,'focus_health_heal')):
        clip=file.objects[pid];require(source.read(clip)['m_Name']==name,'Focus clip identity changed')
        profiles.append(cook_clip(source,clip,out,encoder,rate,1))
    raw=[(ROOT/p['planes'][0]['path']).read_bytes() for p in profiles]
    bank,charge_bytes=assemble(*raw,ambience_end)
    require(charge_bytes==88448 and len(bank)==108208,'Unexpected full Focus payload size; review SPU layout')
    checksum=fnv(bank);payload_path.write_bytes(bank)
    constants={'SPU_BASE':('u32',SPU_BASE),'BANK_BYTES':('usize',len(bank)),'BANK_CHECKSUM':('u32',checksum),'CHARGE_BYTES':('usize',charge_bytes),
        'CHARGE_RATE':('u32',CHARGE_RATE),'HEAL_RATE':('u32',HEAL_RATE),'FADE_TICKS':('u32',contract['fade_ticks']),'GAIN':('i16',GAIN)}
    manifest.write_text('// Generated complete Focus audio descriptor. Samples are loaded from CD.\n'+''.join(f'pub const {name}:{kind}={value};\n' for name,(kind,value) in constants.items()))
    dump(report_path,{'identity':identity,'path':str(payload_path),'byte_len':len(bank),'sha256':sha(payload_path),'manifest_sha256':sha(manifest),
        'checksum':checksum,'spu_base':SPU_BASE,'spu_end':SPU_BASE+len(bank),'spu_free_bytes':SPU_END-SPU_BASE-len(bank),'charge_bytes':charge_bytes,'heal_bytes':len(bank)-charge_bytes,
        'contract':contract,'profiles':profiles,'gain':GAIN,'voices':{'charge':18,'heal':[19,20]},
        'limitations':['Mono and category downsampling reduce fidelity; complete clips are retained.','SNR measures ADPCM reconstruction against resampled PCM, not retail fidelity.',
            'Actors mixer DSP and downstream user mix are not replicated; gain uses the existing SFX headroom convention.','Charge loops have block padding; original FadeAudio exit clamps to zero before the nominal fade can complete.',
            'Guest playback and hardware timing require separate validation.']})
    print('Focus audio:',len(bank),'bytes; SPU end',hex(SPU_BASE+len(bank)),'remaining',SPU_END-SPU_BASE-len(bank))

if __name__=='__main__':main()
