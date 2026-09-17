"""Source-mapped resident one-shots; long death layers stay outside the EXE."""
import hashlib
import io
import json
import math
import struct
import wave
from pathlib import Path
import fmod_nosound  # noqa: F401  offline FSB decoding without an audio device

SPU_BASE = 0x1010
# Root layout reserves 0x14000..0x18000 for Geo; ambience begins at0x18000.
BANK_LIMIT = 0x14000 - SPU_BASE
EVENTS = ('door', 'jump', 'land', 'nail', 'hurt', 'enemy_hit', 'hard_land', 'footsteps_run')
# The serialized HeroController tail is not parsed by the cooker. This hash
# binds its environment0 Dust mapping to the actual-original reflection audit:
# footstepsRunDust == footStepsRunAudioSource.clip == hero_run_footsteps_stone,
# 97600 stereo frames at48000Hz (audio-movement-1). No retail bytes are embedded.
MOVEMENT_HERO_SHA256 = '8a4a799f36e522e7caccffb0c9c345e9a82eb9b2ef2d73024049439340e4e345'


def great_door_hit_contract(fsm, sample_rate=22050):
    """Validate the exact source action before sharing the resident wall clip."""
    from focus import action_fields
    if fsm['name'] != 'Great Door':
        raise ValueError('Great Door sound FSM changed')
    data = next(st for st in fsm['states'] if st['name'] == 'Hit')['actionData']
    actions = [i for i, name in enumerate(data['actionNames'])
               if name.endswith('.AudioPlayRandom') and data['actionEnabled'][i]]
    if len(actions) != 1:
        raise ValueError('Great Door hit sound action changed')
    fields = action_fields(data, actions[0])
    pitch = [fields[name] for name in ('pitchMin', 'pitchMax')]
    if any(p['useVariable'] for p in pitch):
        raise ValueError('Great Door pitch is dynamic')
    values = [p['value'] for p in pitch]
    if any(abs(a-b)>1e-6 for a,b in zip(values,(.85,1.15))):
        raise ValueError('Great Door pitch bounds changed')
    expected = [{'m_FileID': 2, 'm_PathID': i} for i in (92, 99)]
    if data['unityObjectParams'] != expected:
        raise ValueError('Great Door hit clip selection changed')
    return [round(sample_rate*4096/44100*v) for v in values]


def encode(samples):
    """Predictor-zero PSX ADPCM; preserve the existing door bitstream."""
    out = bytearray()
    for start in range(0, len(samples), 28):
        block = list(samples[start:start + 28])
        block += [0] * (28 - len(block))
        peak = max(abs(x) for x in block)
        shift = 0
        while shift < 12 and peak <= (7 * 4096) >> (shift + 1):
            shift += 1
        unit = 4096 >> shift
        n = [max(-8, min(7, round(x / unit))) & 15 for x in block]
        out.extend((shift, 0))
        out.extend(n[i] | (n[i + 1] << 4) for i in range(0, 28, 2))
    # END jumps to the SDK's reserved silence block; fast release is required.
    out.extend(bytes([12, 1]) + bytes(14))
    return bytes(out)


def convert_wav(data, rate):
    """Mono fold-down and box lowpass, preserving the complete duration.

    Existing integer-factor paths retain their exact bytes. The48000Hz running
    sequence uses11025Hz like the long-SFX category; rational box integration
    avoids changing its duration/pitch to force an integer rate ratio.
    """
    with wave.open(io.BytesIO(data)) as w:
        channels, source_rate, frames = w.getnchannels(), w.getframerate(), w.getnframes()
        if w.getsampwidth() != 2 or channels not in (1, 2):
            raise ValueError('unvalidated source PCM format or rate conversion')
        raw = struct.unpack('<' + 'h' * (frames * channels), w.readframes(frames))
    factor = source_rate // rate
    rational = (source_rate, rate) == (48000, 11025)
    if not rational and (source_rate % rate or factor not in (1, 2, 4)):
        raise ValueError('unvalidated rate-conversion factor')
    mono = [sum(raw[i:i + channels]) / channels for i in range(0, len(raw), channels)]
    # Repeat the last source frame for an incomplete final filter window, as
    # the prior door 2:1 conversion did. One round after channel/time averaging.
    if rational:
        pcm = []
        for out in range((len(mono)*rate+source_rate-1)//source_rate):
            start, end = out*source_rate, (out+1)*source_rate
            total = 0.0
            while start < end:
                index = start//rate
                following = min(end, (index+1)*rate)
                total += mono[min(index,len(mono)-1)]*(following-start)
                start = following
            pcm.append(round(total/source_rate))
    else:
        pcm = [round(sum(mono[min(i + j, len(mono) - 1)] for j in range(factor)) / factor)
               for i in range(0, len(mono), factor)]
    return pcm, {'source_rate': source_rate, 'source_channels': channels,
                 'source_frames': frames, 'sample_rate': rate, 'samples': len(pcm)}


def decode_zero(bank):
    """Independent nibble reconstruction for cooked quality/termination checks."""
    if not bank or len(bank) % 16:
        raise ValueError('ADPCM block alignment')
    pcm = []
    for start in range(0, len(bank), 16):
        header, flags = bank[start:start + 2]
        if header > 12 or flags != (1 if start + 16 == len(bank) else 0):
            raise ValueError('unsupported predictor/shift or unsafe loop flags')
        if flags:
            if bank[start + 2:start + 16] != bytes(14):
                raise ValueError('one-shot terminator must be silent')
            break
        for packed in bank[start + 2:start + 16]:
            for nibble in (packed & 15, packed >> 4):
                signed = nibble if nibble < 8 else nibble - 16
                pcm.append((signed * 4096) >> header)
    return pcm


def pack_bank(items, limit=BANK_LIMIT):
    """No partial admission: a missing event or over-budget bank is an error."""
    if tuple(name for name, _, _ in items) != EVENTS:
        raise ValueError('resident sound event table is incomplete or reordered')
    out = bytearray()
    records = []
    for name, encoded, meta in items:
        decode_zero(encoded)
        records.append(dict(meta, event=name, offset=len(out), bytes=len(encoded)))
        out.extend(encoded)
    if len(out) > limit:
        raise ValueError(f'resident SFX bank {len(out)} exceeds {limit} bytes')
    return bytes(out), records


def death_action_layers(data):
    """Only constant, immediate source AudioPlayerOneShotSingle actions."""
    layers = []
    for action, name in enumerate(data['actionNames']):
        if not name.endswith('.AudioPlayerOneShotSingle'):
            continue
        if not data['actionEnabled'][action]:
            raise ValueError('death audio action disabled in source')
        start = data['actionStartIndex'][action]
        end = data['actionStartIndex'][action + 1] if action + 1 < len(data['actionNames']) else len(data['paramName'])
        fields = {data['paramName'][i]: i for i in range(start, end)}
        def literal(field):
            i = fields[field]
            offset, size = data['paramDataPos'][i], data['paramByteDataSize'][i]
            raw = bytes(data['byteData'][offset:offset + size])
            if data['paramDataType'][i] != 15 or len(raw) != 5 or raw[4] != 0:
                raise ValueError('dynamic or unvalidated death audio scalar')
            value = struct.unpack('<f', raw[:4])[0]
            if not math.isfinite(value):
                raise ValueError('non-finite death audio scalar')
            return value
        index = fields['audioClip']
        if data['paramDataType'][index] != 24:
            raise ValueError('unvalidated death AudioClip parameter')
        obj = data['fsmObjectParams'][data['paramDataPos'][index]]
        if obj['useVariable'] or obj['typeName'] != 'UnityEngine.AudioClip':
            raise ValueError('dynamic death clip reference')
        volume, delay = literal('volume'), literal('delay')
        pitch = [literal('pitchMin'), literal('pitchMax')]
        if not 0 <= volume <= 1 or delay != 0 or pitch != [1.0, 1.0]:
            raise ValueError('changed death voice gain, timing or pitch')
        layers.append({'clip': obj['value'], 'volume': volume, 'pitch': pitch,
                       'delay': delay, 'action_index': action})
    if len(layers) != 2:
        raise ValueError('expected both authored death layers')
    return layers


def main():
    from source import Source, ROOT, dump
    s = Source()
    resources = s.file('resources.assets')
    controller = resources.objects[22332]
    if s.typename(controller) != 'HeroAudioController':
        raise ValueError('source HeroAudioController changed')
    hero_audio = s.read(controller)
    if hashlib.sha256(resources.objects[20602].get_raw_data()).hexdigest()!=MOVEMENT_HERO_SHA256:
        raise ValueError('HeroController movement audio linkage changed; revalidate environment Dust mapping')
    from quality import SCENE_TABLE
    # Footsteps are cooked for the verified Dust environment (type 0). Scenes
    # whose SceneManager selects another surface keep the Dust set and are
    # recorded as an explicit omission instead of stopping the cook.
    environment_omissions=[]
    for info in SCENE_TABLE:
        scene=s.file(info['file'])
        managers=[s.read(o) for o in scene.objects.values()
                  if o.type.name=='MonoBehaviour' and s.typename(o)=='SceneManager']
        if len(managers)!=1:
            raise ValueError(f'{info["scene_name"]}: expected one SceneManager for movement audio')
        if managers[0]['environmentType']!=0:
            environment_omissions.append({'scene':info['scene_name'],'environment_type':managers[0]['environmentType'],
                                          'note':'footsteps use the Dust set; this surface set is not cooked'})
    if environment_omissions:
        print(f'Movement audio: {len(environment_omissions)} scenes use a non-Dust footstep environment (Dust set substituted)',flush=True)
    origins = {}
    selected = {}
    expected = {'jump': 'hero_jump', 'land': 'hero_land_soft',
                'hurt': 'hero_damage_less_harsh', 'hard_land':'hero_land_hard mono test',
                'footsteps_run':'hero_run_footsteps_stone'}
    for event, field in [('jump', 'jump'), ('land', 'softLanding'), ('hurt', 'takeHit'),
                         ('hard_land','hardLanding'), ('footsteps_run','footStepsRun')]:
        source = s.ref(resources, hero_audio[field])
        tree = s.read(source)
        if event in ('hard_land','footsteps_run') and (tree['Loop'] or tree['m_Pitch'] != 1.0):
            raise ValueError('movement audio loop/pitch contract changed')
        # Unity 6 moved the resource reference; the deprecated clip is null.
        clip = s.ref(source.assets_file, tree['m_Resource'])
        selected[event] = (clip, expected[event], tree['m_Volume'])
        origins[event] = {'component': s.sid(controller), 'field': field,
                          'audio_source': s.sid(source), 'resource_field': 'm_Resource',
                          'method': 'HeroAudioController.PlaySound'}
        if event=='footsteps_run':
            origins[event].update(environment_type=0,environment_field='HeroController.footstepsRunDust',
                hero_source_sha256=MOVEMENT_HERO_SHA256,
                selection='Runtime-verified Dust mapping equals default run source; complete nonlooping sequence')
    nail = resources.objects[24289]
    if s.typename(nail) != 'NailSlash':
        raise ValueError('source NailSlash changed')
    go = s.read(s.ref(resources, s.read(nail)['m_GameObject']))
    sources = [s.ref(resources, c['component']) for c in go['m_Component']]
    source = next(o for o in sources if o.type.name == 'AudioSource')
    tree = s.read(source)
    selected['nail'] = (s.ref(resources, tree['m_Resource']), 'sword_3', tree['m_Volume'])
    origins['nail'] = {'component': s.sid(nail), 'audio_source': s.sid(source),
                       'method': 'NailSlash.StartSlash', 'selection': 'authored normal Slash voice'}
    infected = s.file('level6').objects[12567]
    if s.typename(infected) != 'InfectedEnemyEffects':
        raise ValueError('source first Crawler hit effects changed')
    impact = s.read(infected)['impactAudio']
    selected['enemy_hit'] = (s.ref(infected.assets_file, impact['Clip']), 'enemy_damage', impact['Volume'])
    origins['enemy_hit'] = {'component': s.sid(infected), 'field': 'impactAudio',
                            'method': 'InfectedEnemyEffects.RecieveHitEffect',
                            'source_pitch_range': [impact['PitchMin'], impact['PitchMax']]}
    selected['door'] = (s.file('sharedassets6.assets').objects[92], 'breakable_wall_hit_1', 1.0)
    origins['door'] = {'method': 'Breakable.Break', 'selection': 'existing opening-door clip'}
    inputs = {}

    def cook(clip, expected_name, volume, rate, origin):
        audio = clip.read()
        if audio.m_Name != expected_name or not 0 <= volume <= 1:
            raise ValueError(f'changed source sound mapping: {expected_name}')
        decoded = audio.samples
        if len(decoded) != 1:
            raise ValueError('source clip decode did not yield one WAV')
        pcm, meta = convert_wav(next(iter(decoded.values())), rate)
        encoded = encode(pcm)
        reconstructed = decode_zero(encoded)[:len(pcm)]
        mse = sum((a - b) ** 2 for a, b in zip(pcm, reconstructed)) / max(1, len(pcm))
        power = sum(a * a for a in pcm) / max(1, len(pcm))
        for name in (Path(clip.assets_file.name).name,
                     getattr(getattr(audio, 'm_Resource', None), 'm_Source', '')):
            path = s.directory / Path(name).name
            if name and path.is_file():
                inputs[path.name] = {'bytes': path.stat().st_size,
                                    'sha256': hashlib.file_digest(path.open('rb'), 'sha256').hexdigest()}
        meta.update(source_id=s.sid(clip), name=expected_name, origin=origin,
                    source_volume=volume, volume_q14=round(0x3fff * volume / 3),
                    output_sha256=hashlib.sha256(encoded).hexdigest(),
                    pcm_peak=max(map(abs, pcm), default=0),
                    decoded_peak=max(map(abs, reconstructed), default=0),
                    rmse=math.sqrt(mse), snr_db=10 * math.log10(power / mse) if mse and power else None)
        return encoded, meta

    items = []
    for event in EVENTS:
        clip, name, volume = selected[event]
        encoded, meta = cook(clip, name, volume, 11025 if event=='footsteps_run' else 22050, origins[event])
        items.append((event, encoded, meta))
    bank, records = pack_bank(items)
    great_door = s.file('level6').objects[12139]
    great_door_pitch = great_door_hit_contract(s.read(great_door)['fsm'])
    (ROOT / 'data/sfx.adpcm').write_bytes(bank)
    # Preserve the existing artifact/API for diagnostics and old build helpers.
    (ROOT / 'data/door.adpcm').write_bytes(items[0][1])
    rust = f'pub const BANK_BYTES: usize = {len(bank)};\n'
    rust += f'pub const SAMPLES: [(u32,u32,i16);{len(EVENTS)}] = [\n'
    rust += ''.join(f'    ({SPU_BASE + r["offset"]},{r["sample_rate"]},{r["volume_q14"]}), // {r["event"]}\n' for r in records)
    rust += '];\n'
    rust += f'pub const GREAT_DOOR_HIT_PITCH:[u16;2]={great_door_pitch};\n'
    by_event={r['event']:r for r in records}
    # Complete nonlooping sequences restart on the first60Hz tick after end.
    for event,constant in [('land','SOFT_LANDING_TICKS'),('footsteps_run','RUN_SEQUENCE_TICKS')]:
        r=by_event[event]
        rust+=f'pub const {constant}:u16={(r["samples"]*60+r["sample_rate"]-1)//r["sample_rate"]};\n'
    # Read the validated serialized scalar prefix, before the unresolved tail.
    from UnityPy.helpers import TypeTreeHelper
    hero=resources.objects[20602]
    if s.typename(hero)!='HeroController':raise ValueError('HeroController source changed')
    node=hero._get_typetree_node();children=node.m_Children
    node.m_Children=children[:next(i for i,n in enumerate(children) if n.m_Name=='hero_state')]
    old=TypeTreeHelper.read_typetree_boost
    try:
        TypeTreeHelper.read_typetree_boost=None
        constants=hero.read_typetree(nodes=node,check_read=False)
    finally:
        TypeTreeHelper.read_typetree_boost=old;node.m_Children=children
    if abs(constants['BIG_FALL_TIME']-1.1)>1e-6:raise ValueError('hard landing fall duration changed')
    hard_ticks=math.floor(constants['BIG_FALL_TIME']*60)+1
    rust+=f'pub const HARD_FALL_MIN_TICKS:u16={hard_ticks};\n'
    (ROOT / 'data/sfx.rs').write_text(rust)

    # The original death FSM starts both layers together. Preserve full clips
    # separately for future CD->SPU boot loading; neither enters the current EXE.
    death_fsm = resources.objects[24938]
    tree = s.read(death_fsm)
    start = next(st for st in tree['fsm']['states'] if st['name'] == 'Start')
    layers = death_action_layers(start['actionData'])
    death_dir = ROOT / '.hkpsx/audio-extra'
    death_dir.mkdir(exist_ok=True)
    deferred = []
    for layer, name in zip(layers, ('hero_death_v2', 'hero_damage'), strict=True):
        encoded, meta = cook(s.ref(resources, layer['clip']), name, layer['volume'], 22050,
                             {'component': s.sid(death_fsm), 'state': 'Start',
                              'action': 'AudioPlayerOneShotSingle',
                              'action_index': layer['action_index'],
                              'pitch': layer['pitch'], 'delay': layer['delay']})
        path = death_dir / (name + '.adpcm')
        path.write_bytes(encoded)
        deferred.append(dict(meta, path=str(path), bytes=len(encoded), runtime_integrated=False))
    level_path = s.directory / 'level6'
    inputs['level6'] = {'bytes': level_path.stat().st_size,
        'sha256': hashlib.file_digest(level_path.open('rb'), 'sha256').hexdigest()}
    assembly = s.directory / 'Managed/Assembly-CSharp.dll'
    inputs['Managed/Assembly-CSharp.dll'] = {'bytes': assembly.stat().st_size,
        'sha256': hashlib.file_digest(assembly.open('rb'), 'sha256').hexdigest()}
    dump(ROOT / '.hkpsx/audio-provenance.json', {
        'environment_omissions': environment_omissions,
        'source': str(s.directory), 'inputs': inputs,
        'generator_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        'spu_base': SPU_BASE,
        'spu_bytes': len(bank), 'resident_bank_limit': BANK_LIMIT,
        'output_sha256': hashlib.sha256(bank).hexdigest(), 'events': records,
        'deferred_death_layers': deferred,
        'great_door_hit': {'component': s.sid(great_door), 'state': 'Hit',
            'resident_variant': 'sharedassets6.assets:92', 'voice': 15,
            'pitch_register_bounds': great_door_pitch,
            'missing_variants': ['sharedassets6.assets:99', 'sharedassets6.assets:102']},
        'movement_audio': {'hard_fall_min_ticks':hard_ticks,'hard_fall_seconds':constants['BIG_FALL_TIME'],
            'simulation_hz':60,'short_sfx_rate':22050,'long_movement_rate':11025,
            'methods':['HeroController.FallCheck','HeroController.ShouldHardLand',
                       'HeroController.DoHardLanding','HeroAudioController.PlaySound'],
            'limitations':['Hard landing audio only; original0.8s recovery/animation not introduced here',
                           'Running Dust sequence only; walk-zone and other environment sequences remain unsupported',
                           'AudioSource isPlaying resampled to bounded60Hz clip-duration counters',
                           'Pause stops/restarts the footstep sequence rather than resuming its exact sample cursor']},
        'limitations': ['Mono box-filtered22050Hz short SFX;11025Hz long movement; predictor-zero ADPCM',
                       'Ordinary SFX use pitch1.0; Great Door hit uses source pitch bounds with deterministic guest RNG',
                       'Nail uses one authored Slash sound for all supported directions',
                       'Unity mixer/positional effects omitted; source voice volumes scaled1/3 for mix headroom',
                       'Full death layers cooked separately but not runtime-integrated']})
    print('Resident SFX:', len(bank), 'bytes;', len(records), 'events; full death layers deferred')


if __name__ == '__main__':
    main()
