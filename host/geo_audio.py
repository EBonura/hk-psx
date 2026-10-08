"""Complete source Geo one-shots in the reserved 16KiB SPU range below ambience."""
import hashlib
import json
import math
import struct
from pathlib import Path
from cook_audio import convert_wav, encode, decode_oneshot
from ambience import fnv

SPU_BASE = 0x14000
BANK_LIMIT = 0x18000 - SPU_BASE
SOURCES = [('resources.assets', 1179, 'geo_small_collect_1'),
           ('resources.assets', 1144, 'geo_small_collect_2'),
           ('resources.assets', 1352, 'geo_small_collect_2'),
           ('sharedassets6.assets', 149, 'geo_rock_hit_1'),
           ('sharedassets6.assets', 173, 'geo_rock_hit_2'),
           ('sharedassets6.assets', 103, 'geo_rock_hit_3')]


def random_audio_action(data):
    """Admit only the observed Self, equal-weight, pitch-one source action."""
    actions = [i for i, n in enumerate(data['actionNames'])
               if n == 'HutongGames.PlayMaker.Actions.AudioPlayRandom']
    if len(actions) != 1:
        raise ValueError('expected one AudioPlayRandom')
    a = actions[0]
    if not data['actionEnabled'][a]:
        raise ValueError('disabled audio action')
    start = data['actionStartIndex'][a]
    end = data['actionStartIndex'][a + 1] if a + 1 < len(data['actionNames']) else len(data['paramName'])
    fields = {data['paramName'][i]: i for i in range(start, end) if data['paramName'][i]}
    if set(fields) != {'gameObject', 'audioClips', 'weights', 'pitchMin', 'pitchMax'}:
        raise ValueError('changed audio action fields')
    def scalar(i):
        pos, size = data['paramDataPos'][i], data['paramByteDataSize'][i]
        raw = bytes(data['byteData'][pos:pos + size])
        if data['paramDataType'][i] != 15 or len(raw) != 5 or raw[4] != 0:
            raise ValueError('dynamic audio scalar')
        value = struct.unpack('<f', raw[:4])[0]
        if value != 1.0:
            raise ValueError('changed audio weight or pitch')
        return value
    def array(name, typename, element):
        i = fields[name]
        if data['paramDataType'][i] != 12:
            raise ValueError('changed audio array type')
        array_index = data['paramDataPos'][i]
        count = data['arrayParamSizes'][array_index]
        if data['arrayParamTypes'][array_index] != typename or not 1 <= count <= 3 or i + count >= end:
            raise ValueError('changed audio array')
        for j in range(i + 1, i + count + 1):
            if data['paramName'][j]:
                raise ValueError('named array element')
        return [element(j) for j in range(i + 1, i + count + 1)]
    def clip(i):
        if data['paramDataType'][i] != 5:
            raise ValueError('dynamic clip reference')
        ref = data['unityObjectParams'][data['paramDataPos'][i]]
        if not ref['m_PathID']:
            raise ValueError('null clip')
        return ref
    i = fields['gameObject']
    if data['paramDataType'][i] != 19:
        raise ValueError('changed audio owner type')
    owner = data['fsmGameObjectParams'][data['paramDataPos'][i]]
    if owner['useVariable'] != 1 or owner['name'] != 'Self':
        raise ValueError('changed audio owner')
    clips = array('audioClips', 'UnityEngine.AudioClip', clip)
    weights = array('weights', 'HutongGames.PlayMaker.FsmFloat', scalar)
    if len(clips) != len(weights):
        raise ValueError('clip/weight count mismatch')
    scalar(fields['pitchMin']); scalar(fields['pitchMax'])
    return clips


def pack_bank(items, limit=BANK_LIMIT):
    if len(items) != len(SOURCES):
        raise ValueError('incomplete Geo bank')
    bank = bytearray(); records = []
    for (source, encoded, meta), (file, pid, _) in zip(items, SOURCES):
        if source != f'{file}:{pid}':
            raise ValueError('reordered Geo source IDs')
        decode_oneshot(encoded)
        records.append(dict(meta, source=source, offset=len(bank), bytes=len(encoded),
                            spu_address=SPU_BASE + len(bank)))
        bank.extend(encoded)
    if len(bank) > limit:
        raise ValueError('Geo bank exceeds remaining SPU capacity')
    return bytes(bank), records


def main():
    from source import Source, ROOT, dump
    s = Source(); resources = s.file('resources.assets'); level = s.file('level6')
    origins = {'coins': [], 'rocks': []}
    def audio_source(obj):
        go = s.read(s.ref(obj.assets_file, s.read(obj)['m_GameObject']))
        audio = [s.ref(obj.assets_file, c['component']) for c in go['m_Component']]
        audio = [o for o in audio if o.type.name == 'AudioSource']
        if len(audio) != 1:
            raise ValueError('expected one event AudioSource')
        tree = s.read(audio[0])
        if tree['m_Volume'] != 1 or tree['m_Pitch'] != 1 or tree['Mute']:
            raise ValueError('changed event AudioSource gain/pitch/mute')
        return s.sid(audio[0])
    for pid, ids in [(25168, [1179, 1144]), (26170, [1179, 1144]), (27009, [1179, 1352])]:
        obj = resources.objects[pid]
        if s.typename(obj) != 'GeoControl':
            raise ValueError('changed GeoControl')
        refs = [s.sid(s.ref(resources, r)) for r in s.read(obj)['pickupSounds']]
        if refs != [f'resources.assets:{i}' for i in ids]:
            raise ValueError('changed pickup sound mapping')
        origins['coins'].append({'component': s.sid(obj), 'clips': refs, 'audio_source': audio_source(obj)})
    for pid in [12121, 12126, 12186, 12227, 12275]:
        obj = level.objects[pid]; tree = s.read(obj)
        states = {v['name']: v for v in tree['fsm']['states']}
        result = {'fsm': s.sid(obj), 'audio_source': audio_source(obj)}
        for state, ids in [('Check Direction', [149, 173, 103]), ('Destroy', [92, 99])]:
            refs = [s.sid(s.ref(level, r)) for r in random_audio_action(states[state]['actionData'])]
            if refs != [f'sharedassets6.assets:{i}' for i in ids]:
                raise ValueError('changed rock sound mapping')
            result[state] = refs
        origins['rocks'].append(result)
    def cook(file, pid, name, rate, resampler='ffmpeg'):
        obj = s.file(file).objects[pid]; audio = obj.read()
        if audio.m_Name != name or len(audio.samples) != 1:
            raise ValueError('changed source clip')
        wav = next(iter(audio.samples.values()))
        pcm, meta = convert_wav(wav, rate, resampler); encoded = encode(pcm)
        decoded = decode_oneshot(encoded)[:len(pcm)]
        mse = sum((a-b)**2 for a,b in zip(pcm, decoded)) / max(1,len(pcm))
        power = sum(a*a for a in pcm) / max(1,len(pcm))
        return s.sid(obj), encoded, dict(meta, name=name, source_wav_sha256=hashlib.sha256(wav).hexdigest(),
            encoded_sha256=hashlib.sha256(encoded).hexdigest(), snr_db=10*math.log10(power/mse) if mse and power else None,
            source_duration_seconds=meta['source_frames']/meta['source_rate'])
    bank, records = pack_bank([cook(*item, 11025) for item in SOURCES])
    # The door clip is the resident hero bank's first sample, cooked by
    # cook_audio.py at the rate its allocator chose and with its resampler:
    # re-cook it the same way to prove the bytes are that source clip.
    hero = json.loads((ROOT/'.hkpsx/audio-provenance.json').read_text())
    door_record = next(e for e in hero['events'] if e.get('event') == 'door')
    if door_record['source_id'] != 'sharedassets6.assets:92' or door_record['offset'] != 0:
        raise ValueError('resident door sample moved')
    _, door, door_meta = cook('sharedassets6.assets', 92, 'breakable_wall_hit_1',
                              door_record['sample_rate'], door_record.get('resampler', 'ffmpeg'))
    resident = (ROOT/'data/sfx.adpcm').read_bytes()
    if resident[:len(door)] != door:
        raise ValueError('resident door sample no longer equals source rock-break clip')
    (ROOT/'data/geo-audio.adpcm').write_bytes(bank)
    manifest = '// Generated from complete Windows source clips; see ignored Geo audio provenance.\n'
    # The guest streams this bank from the disc, so it carries the pack chunk's
    # expected length and FNV-1a checksum rather than the bytes themselves.
    manifest += f'pub const BANK_BYTES: usize = {len(bank)};\n'
    manifest += f'pub const BANK_CHECKSUM: u32 = {fnv(bank)};\n'
    manifest += 'const SAMPLES: [(u32,u32,i16);6] = [\n'
    manifest += ''.join(f'    ({r["spu_address"]},11025,5461),\n' for r in records) + '];\n'
    (ROOT/'data/geo-audio.rs').write_text(manifest)
    inputs = {}
    for file in ['resources.assets', 'resources.resource', 'sharedassets6.assets', 'sharedassets6.resource', 'level6', 'Managed/Assembly-CSharp.dll']:
        p = s.directory/file
        if p.is_file(): inputs[file] = hashlib.sha256(p.read_bytes()).hexdigest()
    dump(ROOT/'.hkpsx/geo-audio-provenance.json', {
        'version': 1, 'source_inputs': inputs, 'origins': origins, 'clips': records,
        'bytes':len(bank), 'spu_base':SPU_BASE, 'spu_end':SPU_BASE+len(bank), 'remaining_spu_bytes':BANK_LIMIT-len(bank),
        'bank_sha256':hashlib.sha256(bank).hexdigest(), 'voices':{'pickup':12,'hit':13,'break':14},
        'source_gain':1.0, 'mix_headroom_gain':1/3,
        'break_reuse':dict(door_meta,source='sharedassets6.assets:92',address=0x1010,
                          resident_bank_sha256=hashlib.sha256(resident).hexdigest()),
        'limitations':['Second rock-destruction variant sharedassets6.assets:99 omitted for SPU budget.',
            'Complete clips folded to mono and polyphase-resampled to 11025 Hz before SDK psx-audio-cook ADPCM.',
            'Equal-weight deterministic event PRNG differs from original Unity RNG.',
            'One voice per event class; rapid same-class retriggers replace that class.',
            'Initial source pitch 1 used; inherited Geo bounce pitch, spatial attenuation and source mixer are not reproduced.'],
        'source_methods':['GeoControl.PlayCollectSound: Random.Range and AudioSource.PlayOneShot',
                          'AudioPlayRandom.DoPlayRandomClip: weighted choice, pitch and PlayOneShot'],
        'tool_sha256':{p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in [Path(__file__),ROOT/'host/cook_audio.py',ROOT/'host/spu_cook.py']}})
    print(f'Geo audio: {len(bank)}/{BANK_LIMIT} bytes, complete six clips, end {SPU_BASE+len(bank):#x}')

if __name__ == '__main__':
    main()
