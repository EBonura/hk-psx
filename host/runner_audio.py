"""Exact full Runner loop/calls, prepared in ignored storage for later guest binding."""
import argparse
import hashlib
import json
from pathlib import Path
from source import ROOT, Source, dump, rel
from cook_music import compile_encoder, cook_clip, sha
from ambience import fnv, loop_payload, validate_blocks, validate_loop
from focus import action_fields


def oneshot_payload(data):
    """Flagless ADPCM blocks and the silent END block (the Rust Focus cook has its own)."""
    validate_blocks(data)
    require(not any(data[i+1] for i in range(0, len(data), 16)), 'one-shot input contains transport flags')
    return data + bytes([12, 1]) + bytes(14)


def fields(data, index):
    """The compact scalar fields of one action, plus its game-object, object and unity-object parameters."""
    result = action_fields(data, index); start = data['actionStartIndex'][index]
    end = data['actionStartIndex'][index+1] if index+1 < len(data['actionNames']) else len(data['paramName'])
    for i in range(start, end):
        kind = data['paramDataType'][i]
        if kind in (19, 24, 11):
            result[data['paramName'][i] or str(i)] = data[{19: 'fsmGameObjectParams', 24: 'fsmObjectParams', 11: 'unityObjectParams'}[kind]][data['paramDataPos'][i]]
    return result

# Immediately above the Focus bank and ending at 0x7FFF0, below the 16 bytes
# psx_spu::init parks the disabled reverb work area on. It was 487440, which
# left 16,368 bytes above it unused.
SPU_BASE = 503808
SPU_END = 524288
GAIN = 5461
SOURCES = (('walk_loop', 'sharedassets32.assets:63', 8000, True),
           ('chase_1', 'sharedassets37.assets:27', 11025, False),
           ('chase_2', 'sharedassets37.assets:26', 11025, False))


def require(ok, message):
    if not ok:
        raise ValueError(message)


def object_identity(source, obj):
    tree = source.read(obj)
    resource = tree['m_Resource']
    path = (source.directory / resource['m_Source']).resolve()
    require(path.is_relative_to(source.directory.resolve()), 'audio resource escapes source')
    with path.open('rb') as stream:
        stream.seek(resource['m_Offset']); raw = stream.read(resource['m_Size'])
    require(len(raw) == resource['m_Size'], 'truncated source audio')
    return {'clip_metadata_sha256': hashlib.sha256(json.dumps(tree, sort_keys=True).encode()).hexdigest(),
            'encoded_resource_sha256': hashlib.sha256(raw).hexdigest()}


def source_contract(source):
    from actors import actor_sources, _component_records
    from scene import Scene
    scene = Scene(source, 'level37')
    def runner_control(actor):
        control = actor.get('movement_control') or actor.get('pending_movement_control') or {}
        return control if control.get('kind') == 'ZombieSwipeWalker' else None
    actors = [a for a in actor_sources(scene) if runner_control(a)]
    require(len(actors) == 2, 'expected both strictly recognized Runners')
    bindings = [runner_control(a)['audio_sources'] for a in actors]
    expected = {'walk_loop': SOURCES[0][1], 'chase': [SOURCES[1][1], SOURCES[2][1]],
                'loop': True, 'volume': 1.0, 'initial_pitch': 0.9973541498184204, 'play_on_awake': False}
    require(all(b == expected for b in bindings), 'Runner AudioSource contract changed')
    actor_records = []
    for actor in actors:
        records = _component_records(scene, actor['game_object'])
        fsm = next(d['fsm'] for _, kind, d in records if kind == 'PlayMakerFSM')
        data = next(st['actionData'] for st in fsm['states'] if st['name'] == 'Anticipate')
        actions = [i for i, name in enumerate(data['actionNames']) if name.endswith('.AudioPlayRandom') and data['actionEnabled'][i]]
        require(len(actions) == 1, 'Runner chase action changed')
        action = fields(data, actions[0])
        pitch = [action[k] for k in ('pitchMin', 'pitchMax')]
        require(all(not p['useVariable'] for p in pitch), 'dynamic Runner chase pitch')
        bounds = [p['value'] for p in pitch]
        require(all(abs(a-b)<1e-6 for a,b in zip(bounds, (.85, 1.15))), 'Runner chase pitch bounds changed')
        audio_id = next(sid for sid, kind, _ in records if kind == 'AudioSource')
        actor_records.append({'actor': actor['source'], 'audio_source': f'level37:{audio_id}',
                              'source_volume': expected['volume'], 'initial_pitch': expected['initial_pitch'],
                              'chase_pitch_bounds': bounds, 'chase_weights': [1.0, 1.0]})
    clips = []
    for event, sid, rate, loop in SOURCES:
        file, pid = sid.split(':'); obj = source.file(file).objects[int(pid)]
        require(obj.type.name == 'AudioClip', 'Runner source is not AudioClip')
        clips.append({'event': event, 'source': sid, 'rate': rate, 'loop': loop,
                      'name': source.read(obj)['m_Name'], 'source_identity': object_identity(source, obj),
                      'source_length_seconds': source.read(obj)['m_Length'],
                      'source_volume': expected['volume'], 'gain': GAIN,
                      'pitch_multipliers': [expected['initial_pitch']]*2 if loop else actor_records[0]['chase_pitch_bounds']})
    return {'actors': actor_records, 'clips': clips, 'scope': 'Bank specification only; no guest playback or voice allocation is installed'}


def assemble(profiles, root, contract, spu_base=SPU_BASE):
    """Pure bank specification: verify source-bound profiles; never truncate to fit."""
    require(spu_base == SPU_BASE and spu_base % 16 == 0, 'Runner SPU tail changed; re-audit layout')
    require(len(profiles) == len(SOURCES) == len(contract['clips']), 'Runner bank inventory incomplete')
    bank = bytearray(); records = []
    for profile, spec, (event, sid, rate, loop) in zip(profiles, contract['clips'], SOURCES):
        require((spec['event'], spec['source'], spec['rate'], spec['loop']) == (event, sid, rate, loop), 'Runner source specification reordered or changed')
        require(profile['source'] == sid and profile['rate'] == rate and profile['channels'] == 1, 'Runner profile source/category rate changed')
        require(profile['name'] == spec['name'] and profile['source_identity'] == spec['source_identity'], 'stale Runner source identity')
        require(profile['spu_pitch'] == round(rate*4096/44100), 'Runner profile pitch mismatch')
        require(len(profile['planes']) == 1 and type(profile['frames']) is int and profile['frames'] > 0, 'invalid Runner mono profile')
        # ffmpeg may round the final rational resampling interval by one sample.
        require(profile['source_pcm_frames'] > 0 and profile['source_pcm_rate'] > 0
                and abs(profile['source_pcm_frames']/profile['source_pcm_rate'] - spec['source_length_seconds']) <= 1/profile['source_pcm_rate']+1e-6
                and abs(profile['frames'] - profile['source_pcm_frames']*rate/profile['source_pcm_rate']) <= 1,
                'Runner profile duration was trimmed or changed')
        plane = profile['planes'][0]; path = root / plane['path']
        require(path.resolve().is_relative_to((root / '.hkpsx').resolve()), 'Runner profile escapes ignored storage')
        raw = path.read_bytes()
        require(len(raw) == plane['bytes'] == profile['bytes'] and sha(path) == plane['sha256'], 'Runner profile payload integrity mismatch')
        require(len(raw) == ((profile['frames']+27)//28)*16 and profile['padding_samples'] == (-profile['frames'])%28,
                'Runner encoded length/padding mismatch')
        payload = loop_payload(raw) if loop else oneshot_payload(raw)
        offset = len(bank)
        records.append(dict(spec, offset=offset, spu_address=spu_base+offset, byte_len=len(payload),
                            checksum=fnv(payload), sha256=hashlib.sha256(payload).hexdigest(),
                            pitch=profile['spu_pitch'], pitch_bounds=[round(rate*4096/44100*p) for p in spec['pitch_multipliers']],
                            valid_frames=profile['frames'], padding_samples=profile['padding_samples']))
        bank.extend(payload)
    require(spu_base+len(bank) <= SPU_END, f'complete Runner bank {len(bank)} exceeds available tail {SPU_END-spu_base}; no samples trimmed')
    descriptor = {'spu_base': spu_base, 'spu_end': spu_base+len(bank), 'spu_free_bytes': SPU_END-spu_base-len(bank),
                  'byte_len': len(bank), 'checksum': fnv(bank), 'sha256': hashlib.sha256(bank).hexdigest(),
                  'clips': records, 'proposed_voices': {'movement_loops': [21, 22], 'creature_calls': [23]}}
    validate_bank(bytes(bank), descriptor)
    return bytes(bank), descriptor


def validate_bank(bank, descriptor):
    require(descriptor['spu_base'] == SPU_BASE and descriptor['spu_end'] == SPU_BASE+len(bank)
            and descriptor['spu_end'] <= SPU_END and descriptor['spu_free_bytes'] == SPU_END-descriptor['spu_end'], 'Runner bank SPU layout mismatch')
    require(len(bank) == descriptor['byte_len'] and fnv(bank) == descriptor['checksum']
            and hashlib.sha256(bank).hexdigest() == descriptor['sha256'], 'Runner bank checksum mismatch')
    require(len(descriptor['clips']) == len(SOURCES), 'Runner bank clip count mismatch')
    offset = 0
    for record, (event, sid, rate, loop) in zip(descriptor['clips'], SOURCES):
        require((record['event'],record['source'],record['rate'],record['loop']) == (event,sid,rate,loop), 'Runner descriptor source/rate mismatch')
        require(record['offset'] == offset and record['spu_address'] == SPU_BASE+offset and offset%16 == 0, 'Runner clip layout mismatch')
        multipliers = [0.9973541498184204]*2 if loop else [.8500000238418579,1.149999976158142]
        require(record['source_volume'] == 1 and record['gain'] == GAIN
                and record['pitch_multipliers'] == multipliers, 'Runner descriptor source gain/pitch changed')
        require(record['pitch'] == round(rate*4096/44100)
                and record['pitch_bounds'] == [round(rate*4096/44100*p) for p in multipliers], 'Runner descriptor pitch mismatch')
        data = bank[offset:offset+record['byte_len']]
        require(len(data) == record['byte_len'] and fnv(data) == record['checksum']
                and hashlib.sha256(data).hexdigest() == record['sha256'], 'Runner clip checksum mismatch')
        validate_blocks(data)
        if loop:
            validate_loop(data)
        else:
            require(data[-16:] == bytes([12,1])+bytes(14) and not any(data[i+1] for i in range(0,len(data)-16,16)), 'Runner terminal ADPCM flags mismatch')
        expected = ((record['valid_frames']+27)//28)*16 + (0 if loop else 16)
        require(record['valid_frames'] > 0 and len(data) == expected and record['padding_samples'] == (-record['valid_frames'])%28,
                'Runner valid sample layout mismatch')
        offset += len(data)
    require(offset == len(bank), 'Runner bank trailing bytes')


def verify_outputs(report_path):
    """Reject stale inputs/code and altered packed output without rewriting evidence."""
    report = json.loads(Path(report_path).read_text())
    for path, expected in report['identity'].items():
        require((ROOT/path).is_file() and sha(ROOT/path) == expected, 'stale Runner input/code: '+path)
    payload_path = (ROOT/report['path']).resolve()
    require(payload_path.is_relative_to(Path(report_path).resolve().parent), 'Runner output path escapes report directory')
    bank = payload_path.read_bytes(); validate_bank(bank, report['bank'])
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, default=ROOT/'.hkpsx/runner71/audio')
    parser.add_argument('--verify', action='store_true')
    args = parser.parse_args(); out = args.out
    require(out.resolve().is_relative_to((ROOT/'.hkpsx').resolve()), 'Runner output must remain in ignored storage')
    report_path = out/'report.json'
    if args.verify:
        report = verify_outputs(report_path); print('Verified complete Runner bank:', report['bank']['byte_len'], 'bytes'); return
    source = Source(); contract = source_contract(source)
    focus_path = ROOT/'.hkpsx/focus-audio.json'; focus = json.loads(focus_path.read_text())
    # The Focus bank (with the ability sounds above it) ends at or below this
    # bank's base; the Focus cook records the free bytes between them.
    require(focus['spu_base']+focus['byte_len'] == focus['spu_end'] <= SPU_BASE
            and sha(ROOT/focus['path']) == focus['sha256'] and sha(ROOT/'data/focus-audio.rs') == focus['manifest_sha256'], 'Focus bank/tail integrity mismatch')
    paths = {source.directory/'globalgamemanagers',source.directory/'level37',focus_path,ROOT/focus['path'],ROOT/'data/focus-audio.rs'}
    from runner import ASSEMBLIES
    paths.update(source.directory/'Managed'/name for name in ASSEMBLIES)
    for spec in contract['clips']:
        file,pid = spec['source'].split(':'); obj=source.file(file).objects[int(pid)]
        paths.update((source.directory/file,source.directory/source.read(obj)['m_Resource']['m_Source']))
    paths.update(ROOT/'host'/name for name in ('runner_audio.py','runner.py','actors.py','source.py','cook_music.py','spu_encode.py','spu_cook.py','ambience.py'))
    identity = {rel(path):sha(path) for path in sorted(paths)}
    out.mkdir(parents=True, exist_ok=True); encoder = compile_encoder(out); profiles=[]
    for spec in contract['clips']:
        file,pid = spec['source'].split(':')
        profiles.append(cook_clip(source,source.file(file).objects[int(pid)],out,encoder,spec['rate'],1))
    bank, descriptor = assemble(profiles,ROOT,contract)
    require(all(sha(path)==value for path,value in identity.items()), 'Runner inputs/code changed during conversion')
    payload_path = out/'runner-audio.adpcm'; payload_path.write_bytes(bank)
    dump(report_path, {'path':rel(payload_path),'identity':identity,'contract':contract,'profiles':profiles,'bank':descriptor,
                      'limitations':['Full clips retained; category downsampling and mono fold-down reduce fidelity.',
                                     'Loop padding is preserved; source AudioSource pitch can persist across chase and walk callbacks.',
                                     'SPU voice assignment, gain mixing and playback scheduling require guest integration and validation.']})
    verify_outputs(report_path)
    # Guest bank: resident ADPCM above the focus bank plus its clip table.
    (ROOT/'data/runner-audio.adpcm').write_bytes(bank)
    rows = ''.join(f"    ({c['spu_address']},{c['byte_len']},{c['pitch']},{c['pitch_bounds'][0]},{c['pitch_bounds'][1]},{str(c['loop']).lower()}), // {c['event']}\n" for c in descriptor['clips'])
    (ROOT/'data/runner-audio.rs').write_text('// Generated from complete Windows source clips; see ignored Runner audio provenance.\n'
        # The guest streams this bank from the disc, so it carries the pack
        # chunk's expected length and FNV-1a checksum, not the bytes themselves.
        f'pub const BANK_BASE: u32 = {SPU_BASE};\npub const BANK_BYTES: usize = {len(bank)};\n'
        f'pub const BANK_CHECKSUM: u32 = {fnv(bank)};\n'
        '/// (SPU address, bytes, nominal pitch, min pitch, max pitch, loops): walk loop, chase 1, chase 2.\n'
        f'pub const CLIPS: [(u32,usize,u16,u16,u16,bool);{len(descriptor["clips"])}] = [\n{rows}];\n')
    print('Runner audio:',len(bank),'bytes; SPU end',descriptor['spu_end'],'remaining',descriptor['spu_free_bytes'])


if __name__ == '__main__':
    main()
