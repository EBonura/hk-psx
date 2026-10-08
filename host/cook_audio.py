"""Source-mapped resident one-shots; long death layers stay outside the EXE."""
import hashlib
import io
import json
import math
import struct
import subprocess
import wave
from pathlib import Path
import fmod_nosound  # noqa: F401  offline FSB decoding without an audio device
import spu_cook

SPU_BASE = 0x1010
# Root layout reserves 0x14000..0x18000 for Geo; ambience begins at0x18000.
BANK_LIMIT = 0x14000 - SPU_BASE
EVENTS = ('door', 'jump', 'land', 'nail', 'hurt', 'enemy_hit', 'hard_land', 'footsteps_run')
# Every resident hero one-shot is resampled by the SDK's shared resampler
# (spu_cook.resample), and the SDK's rate allocator (psx_audio_cook::rate::
# allocate, through `psx-audio-cook plan`) picks each clip's rate from
# RATE_LADDER so the old and the new hero sounds fit the SPU window below the
# Geo bank at the least band loss. Manny's rule: halve the rate where room is
# needed, never trim a clip. A clip never goes above the rate it shipped at.
HERO_RATE = 11025
RATE_LADDER = (22050, 11025, 5512)
# The Knight's sounds the port did not play, appended after the boss voices
# so the dedicated-voice indices never move. (event, file, path id, clip,
# source volume, where the source plays it). Volumes are the Knight's own
# AudioSource m_Volume (Knight/Sounds/*, Knight/Attacks/*); the clips
# HeroController plays with PlayOneShot from a field play at unit volume.
HERO_EXTRA = (
    ('nail_alt', 'resources.assets', 1326, 'sword_4', 1.0, 'Knight/Attacks/AltSlash AudioSource'),
    ('nail_down', 'resources.assets', 1195, 'sword_2', 1.0, 'Knight/Attacks/DownSlash AudioSource'),
    ('dash', 'resources.assets', 1186, 'hero_dash', 1.0, 'Knight/Sounds/Dash AudioSource'),
    ('walljump', 'resources.assets', 1302, 'hero_wall_jump', 0.5, 'Knight/Sounds/Walljump AudioSource'),
    ('wings', 'resources.assets', 1347, 'hero_wings', 1.0, 'HeroController.doubleJumpClip'),
    ('claw', 'resources.assets', 1250, 'hero_mantis_claw', 1.0, 'HeroController.mantisClawClip'),
    ('shade_dash', 'resources.assets', 1153, 'hero_shade_dash_1', 1.0, 'HeroController.shadowDashClip'),
)
# The False Knight's own voices, appended after the hero events in the same
# resident bank. `FalseyControl` plays each through `AudioPlaySimple` on the
# states named here, at source volume 1.0 and pitch 1.0. The rate is the
# category rate this file already uses: 22050 Hz for a short one-shot, 11025 Hz
# for a clip over a second, exactly as `footsteps_run` is cooked.
#
# This bank is streamed from the disc at bootstrap (host/build_guest.py
# AUDIO_BANKS), so it costs SPU RAM only; it stopped costing linked RAM when
# the banks left the EXE. What bounds it is the Geo bank at 0x14000.
BOSS_EVENTS = (
    ('boss_land', 'sharedassets32.assets', 131, 'false_knight_land', 11025,
     ('S Land', 'State 2', 'Land Noise')),
    ('boss_swing', 'sharedassets48.assets', 39, 'false_knight_swing', 22050,
     ('S Attack', 'JA Hit 2')),
)
# Every other clip `FalseyControl` reaches, and the state that plays it. None of
# them is admitted, and `boss_refusals` measures each one rather than asserting
# it, so the reason stays a number. `false_knight_strike_ground` is the one that
# hurts: it is the slam's own impact, and the BigShake it shares a state with is
# all the slam has left.
BOSS_REFUSED = (
    ('sharedassets48.assets', 28, 'false_knight_strike_ground', 'Slam, Rage Slam, JA Slam, Floor Break'),
    ('sharedassets19.assets', 31, 'false_knight_ceiling_break', 'Start Fall, Floor Break'),
    ('sharedassets46.assets', 22, 'false_knight_damage_armour_final', 'Stun Start'),
    ('sharedassets48.assets', 45, 'false_knight_jump', 'Jump 2, JA Jump 2'),
    ('sharedassets6.assets', 171, 'false_knight_land_1st_time', 'Rubble End, Death Land'),
    ('sharedassets48.assets', 21, 'false_knight_roll', 'Stun Land'),
    ('sharedassets48.assets', 29, 'zombie_guard_footstep', 'Run'),
    ('sharedassets32.assets', 143, 'zombie_shield_raise', 'Open Uuup, Death Open'),
    ('sharedassets32.assets', 87, 'zombie_shield_move', 'Open Uuup, Death Open'),
    ('sharedassets48.assets', 30, 'FKnight_Rage', 'Jump 2, Esc Jump'),
    ('sharedassets48.assets', 38, 'FKnight_death', 'Steam'),
    ('sharedassets32.assets', 135, 'boss_final_hit', 'Death Anim Start'),
    ('sharedassets32.assets', 62, 'boss_gushing', 'Steam'),
    ('sharedassets32.assets', 99, 'boss_explode', 'Blow'),
    ('sharedassets40.assets', 30, 'Boss Defeat', 'Boss Death Sting'),
    ('sharedassets6.assets', 102, 'breakable_wall_death', 'Floor Break'),
    ('resources.assets', 1308, 'enemy_death_sword', 'Recover'),
    ('resources.assets', 1248, 'enemy_damage', 'Recover'),
)
# The menu's own clips, from resources.assets' one MenuAudioController
# (`UIAudioPlayer`): `select` plays on every cursor move and `slider` on every
# option step. They are embedded in the EXE and uploaded by `audio::init`
# before the title screen, just above the SFX bank, because the bank itself is
# only read from the disc after the title (the drive is playing the title's
# CD-DA until then). `submit`/`cancel` (ui_button_confirm, 2.34 s) and
# `startGame` (spa_heal, 2.74 s) are refused: at any rate this cook uses they
# are several times the room left below the Geo bank at 0x14000.
MENU_AUDIO_CONTROLLER = 23421
UI_EVENTS = (('select', 'ui_change_selection', 11025), ('slider', 'ui_option_click', 11025))
UI_WORLD = (('submit', 'ui_button_confirm'), ('cancel', 'ui_button_confirm'), ('startGame', 'spa_heal'))
# One-shots every scene can play that did not fit below the Geo bank, in a
# second bank, `data/world-sfx.adpcm`. It is not bounded by 0x14000: it sits
# directly below the Focus bank, and host/ambience.py stacks the music ring
# and its own ceiling below it (TAIL_BANKS), so SPU RAM that nothing ever
# wrote (59,584 bytes above ambience's widest set, 16,368 above Runner, both
# all zero in an SPU dump after a 20,000-poll playtest) now holds them. The
# guest reads it from the disc before the title screen, because two of them
# are the title menu's own, and plays all of them on the shared voice 15.
#
# (event, where the source names the clip, clip name, rate). Each rate was
# picked from the share of the clip's source energy above the new Nyquist
# (lower is less lost; tools in the audit notes, not re-measured here):
#   enemy_death_sword    22050: -9.7 dB would go at 11025, the brightest of
#                               the five and the most frequent, so full rate.
#   hero_death_v2         4000: -54.3 dB above 2 kHz (-65.7 at 8000), the rate
#                               every ambience loop already plays at.
#   ui_button_confirm     5512: -14.8 dB at 5512, 8000 and 11025 alike; the
#                               share it loses is above 8 kHz and already gone.
#   spa_heal              8000: -12.5 dB (-13.2 at 11025, -10.7 at 5512).
#   health_cocoon_break  11025: -10.7 dB (-8.5 at 8000).
# Source gain is 1.0 for all five (EnemyDeathEffects.enemyDeathSwordAudio,
# the death FSM's literal, UIAudioPlayer's and Health Cocoon's AudioSource),
# which is the shared voice's gain.
WORLD_EVENTS = (
    ('enemy_death', 'EnemyDeathEffects.enemyDeathSwordAudio', 'enemy_death_sword', 22050),
    ('hero_death', 'Hero Death FSM Start layer 0', 'hero_death_v2', 4000),
    ('ui_confirm', 'MenuAudioController.submit/cancel', 'ui_button_confirm', 5512),
    ('ui_start', 'MenuAudioController.startGame', 'spa_heal', 8000),
    ('cocoon_break', 'HealthCocoon.deathSound', 'health_cocoon_break', 11025),
)
# The Crawler's EnemyDeathEffects in King's Pass: every admitted enemy family
# kills through the same component and the same clip.
ENEMY_DEATH_EFFECTS = 12681
HEALTH_COCOON = 12337
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


def encoded_bytes(frames, source_rate, rate):
    """What `encode(convert_wav(...))` will weigh, without spending the encode.

    `convert_wav` lands on `round(frames*rate/source_rate)` samples and `encode`
    emits one 16-byte block per 28 of them plus the terminator, so a clip's cost
    is known before it is worth decoding. `main` asserts this against every clip
    it actually cooks, so a change to either function fails here rather than
    quietly making the refusal table optimistic.
    """
    if frames <= 0 or source_rate <= 0 or rate <= 0:
        raise ValueError('cannot size a clip without a source duration')
    return -(-round(frames * rate / source_rate) // 28) * 16 + 16


def wav_frames(data):
    """Source frame count and rate from the WAV header alone, no PCM decode."""
    with wave.open(io.BytesIO(data)) as w:
        if w.getsampwidth() != 2 or w.getnchannels() not in (1, 2):
            raise ValueError('unvalidated source PCM format')
        return w.getnframes(), w.getframerate()


def audio_actions(data, resolve):
    """Every enabled immediate one-shot in one FSM state, with its clip.

    `resolve` maps a serialized AudioClip PPtr to a source id, so the walk is a
    pure function of the FSM and stays testable without the Unity install.
    """
    out = []
    for action, name in enumerate(data['actionNames']):
        short = name.rsplit('.', 1)[-1]
        if short not in ('AudioPlaySimple', 'AudioPlayerOneShotSingle') or not data['actionEnabled'][action]:
            continue
        start = data['actionStartIndex'][action]
        end = data['actionStartIndex'][action + 1] if action + 1 < len(data['actionNames']) else len(data['paramName'])
        record = {'action': short, 'index': action, 'clip': None, 'volume': None, 'pitch': [1.0, 1.0]}
        for i in range(start, end):
            field = data['paramName'][i]
            if data['paramDataType'][i] == 24 and field in ('oneShotClip', 'audioClip'):
                obj = data['fsmObjectParams'][data['paramDataPos'][i]]
                if obj['useVariable'] or obj['typeName'] != 'UnityEngine.AudioClip':
                    raise ValueError('dynamic boss clip reference')
                record['clip'] = resolve(obj['value'])
            elif data['paramDataType'][i] == 15 and field in ('volume', 'pitchMin', 'pitchMax'):
                offset, size = data['paramDataPos'][i], data['paramByteDataSize'][i]
                raw = bytes(data['byteData'][offset:offset + size])
                if size < 4:
                    raise ValueError('unvalidated boss audio scalar')
                value = struct.unpack('<f', raw[:4])[0]
                if not math.isfinite(value):
                    raise ValueError('non-finite boss audio scalar')
                if field == 'volume':
                    record['volume'] = value
                else:
                    record['pitch'][field == 'pitchMax'] = value
        out.append(record)
    return out


def false_knight_audio_contract(fsm, resolve):
    """Bind every admitted boss voice to the `FalseyControl` state that plays it.

    A state that stopped playing its clip, gained a second copy of it, or moved
    off unit gain and pitch fails the cook: the guest plays one voice per event
    and has no way to reproduce a changed one.
    """
    if fsm['name'] != 'FalseyControl':
        raise ValueError('False Knight audio FSM changed')
    states = {state['name']: state for state in fsm['states']}
    bindings = []
    for event, file, path_id, name, rate, wanted in BOSS_EVENTS:
        sid = f'{file}:{path_id}'
        for state in wanted:
            if state not in states:
                raise ValueError('missing False Knight audio state: ' + state)
            played = [a for a in audio_actions(states[state]['actionData'], resolve) if a['clip'] == sid]
            if len(played) != 1:
                raise ValueError(f'{state} no longer plays {name} exactly once')
            if played[0]['volume'] != 1.0 or played[0]['pitch'] != [1.0, 1.0]:
                raise ValueError(f'{state} changed the {name} gain or pitch')
        bindings.append({'event': event, 'source': sid, 'name': name, 'sample_rate': rate,
                         'states': list(wanted), 'action': 'AudioPlaySimple',
                         'method': 'FalseyControl.' + '/'.join(wanted)})
    return bindings


def boss_refusals(open_clip, free_bytes):
    """Size every boss clip this bank cannot hold, at each rate the port uses.

    A refusal carries the byte count that produced it rather than a sentence,
    because the only thing that would change the answer is the number.
    """
    out = []
    for file, path_id, name, states in BOSS_REFUSED:
        frames, source_rate = open_clip(file, path_id, name)
        rates = {str(rate): encoded_bytes(frames, source_rate, rate) for rate in (22050, 11025, 8000)}
        out.append({'source': f'{file}:{path_id}', 'name': name, 'states': states,
                    'source_frames': frames, 'source_rate': source_rate,
                    'encoded_bytes': rates, 'free_bytes': free_bytes,
                    'fits_at': [rate for rate, size in rates.items() if size <= free_bytes]})
    return out


def encode(samples):
    """One-shot PSX ADPCM from the SDK's shared encoder (host/spu_cook.py), every
    block's flags zero, then the silent terminator block."""
    out = bytearray(spu_cook.encode_pcm(samples, 'none'))
    # END jumps to the SDK's reserved silence block; fast release is required.
    out.extend(bytes([12, 1]) + bytes(14))
    return bytes(out)


def convert_wav(data, rate, resampler='ffmpeg'):
    """Mono fold-down and resample, preserving the complete duration.

    Uses the same ffmpeg polyphase resampler the music, ambience, Focus and
    Runner cooks already run through `cook_music.cook_clip`, rather than a
    second hand-written filter. The previous box average aliased everything
    above the new Nyquist back into the band and rolled off inside it, which
    was audible on the nail swing and footsteps.
    """
    with wave.open(io.BytesIO(data)) as w:
        channels, source_rate, frames = w.getnchannels(), w.getframerate(), w.getnframes()
        if w.getsampwidth() != 2 or channels not in (1, 2):
            raise ValueError('unvalidated source PCM format or rate conversion')
    if frames == 0:
        return [], {'source_rate': source_rate, 'source_channels': channels,
                    'source_frames': 0, 'sample_rate': rate, 'samples': 0}
    if resampler == 'sdk':
        pcm = spu_cook.resample(data, rate)
    else:
        converted = subprocess.run(
            ['ffmpeg', '-v', 'error', '-i', 'pipe:0', '-ar', str(rate), '-ac', '1', '-f', 's16le', 'pipe:1'],
            input=data, stdout=subprocess.PIPE, check=True).stdout
        pcm = list(struct.unpack('<' + 'h' * (len(converted) // 2), converted))
    # The resampler primes its filter, so a clip shorter than that window can
    # come back truncated. Every real clip here is thousands of frames; fail
    # rather than let a short one ship silently missing its head or tail.
    expected = round(frames * rate / source_rate)
    if abs(len(pcm) - expected) > 1:
        raise ValueError(f'resampled length {len(pcm)} is not the expected {expected}')
    return pcm, {'source_rate': source_rate, 'source_channels': channels,
                 'source_frames': frames, 'sample_rate': rate, 'samples': len(pcm), 'resampler': resampler}


def decode_oneshot(bank):
    """Checks a one-shot's framing (no flags but the silent END terminator) and
    decodes it as the SPU does, for the cooked quality/termination checks."""
    if not bank or len(bank) % 16:
        raise ValueError('ADPCM block alignment')
    for start in range(0, len(bank), 16):
        header, flags = bank[start:start + 2]
        if header >> 4 > 4 or header & 15 > 12 or flags != (1 if start + 16 == len(bank) else 0):
            raise ValueError('unsupported predictor/shift or unsafe loop flags')
    if bank[-14:] != bytes(14):
        raise ValueError('one-shot terminator must be silent')
    return spu_cook.decode(bank[:-16])


def allocate_rates(rows, budget):
    """{key: rate} from the SDK allocator. `rows` are (key, wav bytes, top rate);
    each clip may take its top rate or any lower step of RATE_LADDER."""
    import tempfile
    with tempfile.TemporaryDirectory() as tmp:
        request, ladders = [f'budget\t{budget}'], []
        for key, data, top in rows:
            frames, source_rate = wav_frames(data)
            # At most one halving below the rate a clip shipped at (Manny's
            # rule is to halve, so a clip never drops two steps).
            ladder = [r for r in RATE_LADDER if r <= top][:2]
            sizes = [encoded_bytes(frames, source_rate, r) for r in ladder]
            path = Path(tmp) / f'{len(ladders)}.wav'
            path.write_bytes(data)
            request.append(f'1\t{len(ladder) - 1}\t{path}\t{",".join(map(str, ladder))}\t{",".join(map(str, sizes))}')
            ladders.append((key, ladder))
        plan = Path(tmp) / 'plan.txt'
        plan.write_text('\n'.join(request) + '\n')
        out = subprocess.run([str(spu_cook.binary()), 'plan', str(plan)], capture_output=True, text=True, check=True).stdout
    lines = [l for l in out.splitlines() if l.strip()]
    if not lines or lines[0].strip() == 'none':
        raise ValueError('the hero sounds do not fit the SFX window at any allowed rate')
    return {key: ladder[int(line.split('\t')[0])] for (key, ladder), line in zip(ladders, lines, strict=True)}


def pack_bank(items, boss_items=(), limit=BANK_LIMIT, extra_items=()):
    """No partial admission: a missing event or over-budget bank is an error.

    The boss voices are appended after the hero events so the eight indices
    `game/src/audio.rs` binds to its dedicated voices never move.
    """
    if tuple(name for name, _, _ in items) != EVENTS:
        raise ValueError('resident sound event table is incomplete or reordered')
    if tuple(name for name, _, _ in boss_items) != tuple(event[0] for event in BOSS_EVENTS):
        raise ValueError('False Knight sound table is incomplete or reordered')
    if tuple(name for name, _, _ in extra_items) not in ((), tuple(event[0] for event in HERO_EXTRA)):
        raise ValueError('hero extra sound table is incomplete or reordered')
    out = bytearray()
    records = []
    for name, encoded, meta in list(items) + list(boss_items) + list(extra_items):
        decode_oneshot(encoded)
        records.append(dict(meta, event=name, offset=len(out), bytes=len(encoded)))
        out.extend(encoded)
    if len(out) > limit:
        raise ValueError(f'resident SFX bank {len(out)} exceeds {limit} bytes')
    return bytes(out), records


def pack_world(items):
    """The world bank in WORLD_EVENTS order, every clip whole and 16-byte aligned.

    The guest indexes it by position, so a missing or reordered event is an
    error rather than a shifted table.
    """
    if tuple(name for name, _, _ in items) != tuple(event[0] for event in WORLD_EVENTS):
        raise ValueError('world sound table is incomplete or reordered')
    out = bytearray()
    records = []
    for name, encoded, meta in items:
        decode_oneshot(encoded)
        records.append(dict(meta, event=name, offset=len(out), bytes=len(encoded)))
        out.extend(encoded)
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
    from source import Source, ROOT, dump, rel
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

    def cook(clip, expected_name, volume, rate, origin, resampler='ffmpeg'):
        audio = clip.read()
        if audio.m_Name != expected_name or not 0 <= volume <= 1:
            raise ValueError(f'changed source sound mapping: {expected_name}')
        decoded = audio.samples
        if len(decoded) != 1:
            raise ValueError('source clip decode did not yield one WAV')
        pcm, meta = convert_wav(next(iter(decoded.values())), rate, resampler)
        encoded = encode(pcm)
        reconstructed = decode_oneshot(encoded)[:len(pcm)]
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

    def source_wav(clip):
        decoded = clip.read().samples
        if len(decoded) != 1:
            raise ValueError('source clip decode did not yield one WAV')
        return next(iter(decoded.values()))
    # One allocation over everything the window holds: the hero events at the
    # rate each shipped at as their ceiling, the boss voices, the Knight's
    # extra one-shots (ceiling 22,050 Hz like every short hero sound), with the
    # menu clips' fixed bytes taken off the budget first.
    menu_bytes = 2560
    rows = [(('hero', e), source_wav(selected[e][0]), 11025 if e == 'footsteps_run' else 22050) for e in EVENTS]
    rows += [(('boss', e[0]), source_wav(s.file(e[1]).objects[e[2]]), e[4]) for e in BOSS_EVENTS]
    rows += [(('extra', e[0]), source_wav(s.file(e[1]).objects[e[2]]), 22050) for e in HERO_EXTRA]
    rates = allocate_rates(rows, BANK_LIMIT - menu_bytes)
    print('Hero SFX rates (SDK allocator): ' + ', '.join(f'{k[1]} {r}' for k, r in rates.items()), flush=True)
    items = []
    for event in EVENTS:
        clip, name, volume = selected[event]
        encoded, meta = cook(clip, name, volume, rates[('hero', event)], origins[event], 'sdk')
        items.append((event, encoded, meta))

    # The False Knight lives in level48, the additive boss scene host/scene.py
    # merges into Crossroads_10; its FalseyControl is read here directly rather
    # than through that merge, because the clip PPtrs are the file's own.
    boss_scene = s.file('level48')
    controls = [o for o in boss_scene.objects.values()
                if o.type.name == 'MonoBehaviour' and s.typename(o) == 'PlayMakerFSM'
                and s.read(o)['fsm']['name'] == 'FalseyControl']
    if len(controls) != 1:
        raise ValueError('expected one FalseyControl in the boss scene')
    control = controls[0]
    boss_bindings = false_knight_audio_contract(
        s.read(control)['fsm'], lambda ref: s.sid(s.ref(boss_scene, ref)))
    boss_items = []
    for (event, file, path_id, name, rate, _states), binding in zip(BOSS_EVENTS, boss_bindings, strict=True):
        clip = s.file(file).objects[path_id]
        rate = rates[('boss', event)]
        encoded, meta = cook(clip, name, 1.0, rate,
                             {'component': s.sid(control), 'states': binding['states'],
                              'action': binding['action'], 'method': binding['method']}, 'sdk')
        # The refusal table is sized rather than cooked, so prove the estimator
        # against a clip that went through the real path before trusting it.
        predicted = encoded_bytes(meta['source_frames'], meta['source_rate'], rate)
        if predicted != len(encoded):
            raise ValueError(f'{name}: predicted {predicted} encoded bytes, cooked {len(encoded)}')
        boss_items.append((event, encoded, meta))
    extra_items = []
    for event, file, path_id, name, volume, where in HERO_EXTRA:
        encoded, meta = cook(s.file(file).objects[path_id], name, volume, rates[('extra', event)], {'where': where}, 'sdk')
        extra_items.append((event, encoded, meta))
    bank, records = pack_bank(items, boss_items, extra_items=extra_items)

    # What the fight asked for and this bank could not hold, in bytes. Sized
    # from each clip's own WAV header through the estimator just proven above.
    def open_clip(file, path_id, name):
        audio = s.file(file).objects[path_id].read()
        if audio.m_Name != name:
            raise ValueError(f'changed boss clip mapping: {name}')
        decoded = audio.samples
        if len(decoded) != 1:
            raise ValueError('source clip decode did not yield one WAV')
        return wav_frames(next(iter(decoded.values())))
    refused = boss_refusals(open_clip, BANK_LIMIT - len(bank))
    admitted_names = {name for _, _, name, _ in BOSS_REFUSED} & {e[3] for e in BOSS_EVENTS}
    if admitted_names:
        raise ValueError('a clip is both admitted and refused: ' + ', '.join(sorted(admitted_names)))
    fitting = [r for r in refused if r['fits_at']]
    print(f'False Knight: {len(boss_items)} voices admitted, {len(refused)} refused, '
          f'{BANK_LIMIT - len(bank)} bank bytes free', flush=True)
    for r in fitting:
        print(f'  note: {r["name"]} would now fit at {"/".join(r["fits_at"])} Hz '
              f'({r["encoded_bytes"]}) against {r["free_bytes"]} free', flush=True)

    menu_audio = s.read(resources.objects[MENU_AUDIO_CONTROLLER])
    if s.typename(resources.objects[MENU_AUDIO_CONTROLLER]) != 'MenuAudioController':
        raise ValueError('MenuAudioController moved')
    for field, name in UI_WORLD:
        if s.read(s.ref(resources, menu_audio[field])).get('m_Name') != name:
            raise ValueError(f'changed menu clip mapping: {field}')
    ui_items = []
    for field, name, rate in UI_EVENTS:
        clip = s.ref(resources, menu_audio[field])
        encoded, meta = cook(clip, name, 1.0, rate,
                             {'component': s.sid(resources.objects[MENU_AUDIO_CONTROLLER]), 'field': field})
        ui_items.append((field, encoded, meta))
    ui_blob = b''.join(encoded for _, encoded, _ in ui_items)
    if len(bank) + len(ui_blob) > BANK_LIMIT:
        raise ValueError(f'menu clips {len(ui_blob)} do not fit the {BANK_LIMIT - len(bank)} bytes above the SFX bank')
    (ROOT / 'data/ui_sfx.adpcm').write_bytes(ui_blob)
    print(f'Menu clips: {len(ui_blob)} bytes above the SFX bank, {BANK_LIMIT - len(bank) - len(ui_blob)} left', flush=True)

    great_door = s.file('level6').objects[12139]
    great_door_pitch = great_door_hit_contract(s.read(great_door)['fsm'], rates[('hero', 'door')])
    (ROOT / 'data/sfx.adpcm').write_bytes(bank)
    # Preserve the existing artifact/API for diagnostics and old build helpers.
    (ROOT / 'data/door.adpcm').write_bytes(items[0][1])
    def rows(selected):
        return ''.join(f'    ({SPU_BASE + r["offset"]},{r["sample_rate"]},{r["volume_q14"]}), // {r["event"]}\n'
                       for r in records if r['event'] in selected)
    rust = f'pub const BANK_BYTES: usize = {len(bank)};\n'
    # The guest streams this bank from the disc, so it carries the pack chunk's
    # expected length and FNV-1a checksum rather than the bytes themselves.
    from ambience import fnv
    rust += f'pub const BANK_CHECKSUM: u32 = {fnv(bank)};\n'
    rust += f'pub const SAMPLES: [(u32,u32,i16);{len(EVENTS)}] = [\n{rows(EVENTS)}];\n'
    boss = tuple(event[0] for event in BOSS_EVENTS)
    rust += f'/// False Knight one-shots, on the shared voice: {", ".join(boss)}.\n'
    rust += f'pub const BOSS_SAMPLES: [(u32,u32,i16);{len(boss)}] = [\n{rows(boss)}];\n'
    extra = tuple(event[0] for event in HERO_EXTRA)
    rust += f'/// The Knight\'s other one-shots, played by reconfiguring a hero voice: {", ".join(extra)}.\n'
    rust += f'pub const EXTRA_SAMPLES: [(u32,u32,i16);{len(extra)}] = [\n{rows(extra)}];\n'
    rust += f'pub const GREAT_DOOR_HIT_PITCH:[u16;2]={great_door_pitch};\n'
    ui_rows, offset = [], SPU_BASE + len(bank)
    for field, encoded, meta in ui_items:
        ui_rows.append(f'    ({offset},{meta["sample_rate"]},{meta["volume_q14"]}), // {field}\n')
        offset += len(encoded)
    rust += f'/// Menu clips (data/ui_sfx.adpcm), uploaded at SPU {SPU_BASE + len(bank)} before the title: select, slider.\n'
    rust += f'pub const UI_BYTES: usize = {len(ui_blob)};\n'
    rust += f'pub const UI_SAMPLES: [(u32,u32,i16);{len(ui_items)}] = [\n{"".join(ui_rows)}];\n'
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
        deferred.append(dict(meta, path=rel(path), bytes=len(encoded), runtime_integrated=False))

    # The world bank. Every clip is resolved through the component that plays
    # it, so a patch that rewires one fails here instead of shipping the old one.
    level6 = s.file('level6')
    effects = level6.objects[ENEMY_DEATH_EFFECTS]
    cocoon = level6.objects[HEALTH_COCOON]
    if s.typename(effects) != 'EnemyDeathEffects' or s.typename(cocoon) != 'HealthCocoon':
        raise ValueError('source enemy death or cocoon component moved')
    sword = s.read(effects)['enemyDeathSwordAudio']
    cocoon_tree = s.read(cocoon)
    cocoon_go = s.read(s.ref(level6, cocoon_tree['m_GameObject']))
    cocoon_source = [s.read(o) for o in (s.ref(level6, c['component']) for c in cocoon_go['m_Component'])
                     if o.type.name == 'AudioSource']
    if len(cocoon_source) != 1:
        raise ValueError('Health Cocoon no longer has exactly one AudioSource')
    world_sources = {
        'enemy_death': (s.ref(effects.assets_file, sword['Clip']), sword['Volume'],
                        (sword['PitchMin'], sword['PitchMax']), {'component': s.sid(effects), 'field': 'enemyDeathSwordAudio',
                        'method': 'EnemyDeathEffects.EmitSound'}),
        'hero_death': (s.ref(resources, layers[0]['clip']), layers[0]['volume'], tuple(layers[0]['pitch']),
                       {'component': s.sid(death_fsm), 'state': 'Start', 'action_index': layers[0]['action_index'],
                        'note': 'the second layer, hero_damage, is stood in for by the hurt clip on voice 4'}),
        'ui_confirm': (s.ref(resources, menu_audio['submit']), 1.0, (1.0, 1.0),
                       {'component': s.sid(resources.objects[MENU_AUDIO_CONTROLLER]), 'field': 'submit, cancel'}),
        'ui_start': (s.ref(resources, menu_audio['startGame']), 1.0, (1.0, 1.0),
                     {'component': s.sid(resources.objects[MENU_AUDIO_CONTROLLER]), 'field': 'startGame'}),
        'cocoon_break': (s.ref(level6, cocoon_tree['deathSound']), cocoon_source[0]['m_Volume'],
                         (cocoon_source[0]['m_Pitch'], cocoon_source[0]['m_Pitch']),
                         {'component': s.sid(cocoon), 'field': 'deathSound', 'method': 'HealthCocoon.PlaySound'}),
    }
    world_items = []
    for event, where, name, rate in WORLD_EVENTS:
        clip, volume, pitch, origin = world_sources[event]
        if volume != 1.0:
            raise ValueError(f'{name}: source gain {volume} is not the shared voice gain')
        encoded, meta = cook(clip, name, volume, rate, dict(origin, where=where))
        if encoded_bytes(meta['source_frames'], meta['source_rate'], rate) != len(encoded):
            raise ValueError(f'{name}: encoded size does not match the estimator')
        register = [round(rate * 4096 / 44100 * p) for p in pitch]
        if not all(0 < r < 0x4000 for r in register):
            raise ValueError(f'{name}: pitch register out of range')
        world_items.append((event, encoded, dict(meta, source_pitch=list(pitch), pitch_register=register)))
    world, world_records = pack_world(world_items)
    from focus_audio import SPU_BASE as FOCUS_SPU_BASE
    world_base = FOCUS_SPU_BASE - len(world)
    (ROOT / 'data/world-sfx.adpcm').write_bytes(world)
    rust = '// Generated by host/cook_audio.py: the world one-shots, streamed before the title.\n'
    rust += f'pub const SPU_BASE: u32 = {world_base};\n'
    rust += f'pub const BANK_BYTES: usize = {len(world)};\n'
    rust += f'pub const BANK_CHECKSUM: u32 = {fnv(world)};\n'
    rust += ('/// (SPU address, rate, gain, pitch register min, max): '
             + ', '.join(r['event'] for r in world_records) + '.\n')
    rust += f'pub const SAMPLES: [(u32,u32,i16,u16,u16);{len(world_records)}] = [\n'
    for r in world_records:
        low, high = r['pitch_register']
        rust += f'    ({world_base + r["offset"]},{r["sample_rate"]},{r["volume_q14"]},{low},{high}), // {r["event"]}\n'
    rust += '];\n'
    (ROOT / 'data/world-sfx.rs').write_text(rust)
    print(f'World SFX: {len(world)} bytes at {world_base:#x}..{FOCUS_SPU_BASE:#x}, '
          + ', '.join(f'{r["name"]} {r["bytes"]}B@{r["sample_rate"]}' for r in world_records), flush=True)
    level_path = s.directory / 'level6'
    inputs['level6'] = {'bytes': level_path.stat().st_size,
        'sha256': hashlib.file_digest(level_path.open('rb'), 'sha256').hexdigest()}
    assembly = s.directory / 'Managed/Assembly-CSharp.dll'
    inputs['Managed/Assembly-CSharp.dll'] = {'bytes': assembly.stat().st_size,
        'sha256': hashlib.file_digest(assembly.open('rb'), 'sha256').hexdigest()}
    boss_path = s.directory / 'level48'
    inputs['level48'] = {'bytes': boss_path.stat().st_size,
        'sha256': hashlib.file_digest(boss_path.open('rb'), 'sha256').hexdigest()}
    dump(ROOT / '.hkpsx/audio-provenance.json', {
        'environment_omissions': environment_omissions,
        'source': str(s.directory), 'inputs': inputs,
        'generator_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        'spu_base': SPU_BASE,
        'spu_bytes': len(bank), 'resident_bank_limit': BANK_LIMIT,
        'output_sha256': hashlib.sha256(bank).hexdigest(), 'events': records,
        'deferred_death_layers': deferred,
        'world_sfx': {'spu_base': world_base, 'spu_end': world_base + len(world), 'bytes': len(world),
            'output_sha256': hashlib.sha256(world).hexdigest(), 'voice': 15, 'events': world_records,
            'transport': 'WORLD.PAK chunk read before the title screen; the bootstrap retries it if that read failed',
            'limitations': [
                'All five share voice 15 with the Great Door, the False Knight and the menu clips; a new'
                ' play retriggers over whatever that voice was playing.',
                'The death FSM also starts hero_damage with hero_death_v2; the hurt clip already playing'
                ' on voice 4 (hero_damage_less_harsh) stands in for it.',
                'Rates below the category rates are chosen from measured energy above the new Nyquist;'
                ' see WORLD_EVENTS.']},
        'great_door_hit': {'component': s.sid(great_door), 'state': 'Hit',
            'resident_variant': 'sharedassets6.assets:92', 'voice': 15,
            'pitch_register_bounds': great_door_pitch,
            'missing_variants': ['sharedassets6.assets:99', 'sharedassets6.assets:102']},
        'false_knight': {'component': s.sid(control), 'scene': 'level48',
            'admitted': boss_bindings, 'refused': refused,
            # Voice 15 is shared with the Great Door: every one of the 24 SPU
            # voices is allocated (0..5,16..17 SFX, 6..11 ambience, 12..14 Geo,
            # 18..20 Focus, 21..23 Runner), and no scene holds both the door
            # (Tutorial_01) and the fight (Crossroads_10).
            'voice': 15, 'bank_free_bytes': BANK_LIMIT - len(bank),
            'limitations': [
                'The resident bank is bounded by the Geo bank at 0x14000; clips every scene'
                ' needs go to the world bank instead, and a per-scene bank does not exist yet.',
                'false_knight_strike_ground, the slam impact itself, is refused at every rate'
                ' this port uses; the slam keeps its BigShake and its swing and has no boom.',
                '`Stun Land` plays false_knight_land through AudioPlayerOneShotSingle at pitch'
                ' 1.15 rather than AudioPlaySimple at 1.0; the pitched variant is not admitted.',
                'The boss shares one voice, so a swing retriggers over a landing tail the way'
                ' every other event class here does, rather than mixing as the source does.']},
        'movement_audio': {'hard_fall_min_ticks':hard_ticks,'hard_fall_seconds':constants['BIG_FALL_TIME'],
            'simulation_hz':60,'short_sfx_rate':'SDK allocator per clip (see events)','long_movement_rate':11025,
            'methods':['HeroController.FallCheck','HeroController.ShouldHardLand',
                       'HeroController.DoHardLanding','HeroAudioController.PlaySound'],
            'limitations':['Hard landing audio only; original0.8s recovery/animation not introduced here',
                           'Running Dust sequence only; walk-zone and other environment sequences remain unsupported',
                           'AudioSource isPlaying resampled to bounded60Hz clip-duration counters',
                           'Pause stops/restarts the footstep sequence rather than resuming its exact sample cursor']},
        'limitations': ['Mono 11025 Hz hero SFX resampled by the SDK shared resampler (halved from 22050 Hz); SDK psx-audio-cook ADPCM',
                       'Ordinary SFX use pitch1.0; Great Door hit uses source pitch bounds with deterministic guest RNG',
                       'Nail uses one authored Slash sound for all supported directions',
                       'Unity mixer/positional effects omitted; source voice volumes scaled1/3 for mix headroom',
                       'Full-rate death layers are cooked separately for reference; the guest plays'
                       ' hero_death_v2 from the world bank at 4000 Hz']})
    print('Resident SFX:', len(bank), 'bytes;', len(records), 'events; full death layers deferred')


if __name__ == '__main__':
    main()
