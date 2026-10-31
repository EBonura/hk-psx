"""Compact art banks for ordinary actor families (game/src/actor_bank.rs, docs/ACTOR-BANK.md).

A scene opts in by data: an entry in host/actor_banks.json names the actors whose art leaves the room path and rides a bank
instead, the views that draw them, and how the bank is split. Nothing else in the cook knows about it: with the file's
`scenes` empty, every function here returns an empty answer and the cooked bytes are what they were before it existed.

    {"scenes": {"Crossroads_10": {
        "id": 4,                        # bank id of the first slice; later slices take id + 1, id + 2, ...
        "package": "art_husks",         # the module pool data package that carries the first slice
        "actors": ["Zombie Barger"],    # actor names in the scene (`source` ids also work)
        "reach": 8,                     # optional: widen the default views by this many world units (see below)
        "views": [235, 236],            # chunk ids of the views that draw them; "all": every view of the scene; omitted: the views
                                        # whose region cook lists the actors (those inside the view's interaction bounds)
        "banks": [{"name": "core", "palettes": 4, "exclude_clips": ["Death Air", "Death Land", "Fall"]},
                  {"name": "late", "palettes": 4, "include_clips": ["Death Air", "Death Land", "Fall"]}]}}}

What the cook writes (data/actor_bank/<scene>/): `slice0.bin`, `slice1.bin`, ... (the guest's `Bank` blobs, each exact) and
`bank.json` (the merged tables). The first slice is the scene's module pool package (host/code_modules.py DATA_MODULES) and
the others are linked read-only data (`data/actor_banks.rs`).

In a bank view the actors' frames are ordinary frame records whose texture is the first carrier texture and whose first two
box words carry the sprite's global index in their low bytes; the clips and frames are appended after every other clip, so a
view without the bank keeps the clip indices of the rest. A bank actor is listed (`row['actors']`) only in the views that
carry the bank; the guest does not step or draw an actor its bound view does not list.

Which views carry the bank matters for pop-in: a view draws every actor it lists and no other, and the renderer draws the view
whose camera range holds the camera, which can be a neighbour of the Knight's. So an actor must be listed in every view from
which it can be on screen. The default is the views whose interaction bounds (the camera range and its margin) hold an
actor's placement; `reach` grows those bounds for actors that wander or stand near a view edge, and `views` overrides the
rule (`"all"` lists every view of the scene, which costs the carriers in every one).
"""
import json
import struct
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CONFIG = Path(__file__).with_name('actor_banks.json')
OUT = ROOT / 'data/actor_bank'
COOK = ROOT / 'host/target/release/hk-cook'
CARRIER_MAGIC = b'HKBP'
MAX_SLICES = 2


def scenes():
    """{scene name: entry} of host/actor_banks.json."""
    entries = json.loads(CONFIG.read_text())['scenes']
    for name, e in entries.items():
        if not 1 <= len(e['banks']) <= MAX_SLICES:
            raise ValueError(f'{name}: {len(e["banks"])} banks, the guest reads 1 to {MAX_SLICES}')
        if not e['package'].startswith('art_'):
            raise ValueError(f'{name}: package {e["package"]!r} must be named art_*')
    return entries


def entry(scene_name):
    return scenes().get(scene_name)


def packages():
    """{package name: scene name}, in file order: the data packages the cook adds to host/code_modules.py DATA_MODULES."""
    return {e['package']: name for name, e in scenes().items()}


def late_slices(scene_name):
    """Slices after the first, as the paths the guest links (data/actor_banks.rs)."""
    return [f'actor_bank/{scene_name}/slice{k}.bin' for k in range(1, len(scenes()[scene_name]['banks']))]


def is_bank_actor(e, actor):
    return e is not None and (actor['name'] in e['actors'] or actor['source'] in e['actors'])


def admit(all_actors, e):
    """Make the entry's actors supported ones (an additive scene's actors are refused wholesale until a controller opts in;
    the bank is that opt-in) and return them. They must already have a recognised controller."""
    out = []
    for actor in all_actors:
        if not is_bank_actor(e, actor):
            continue
        if 'movement_control' not in actor:
            raise ValueError(f'actor bank: {actor["name"]} has no recognised controller ({actor.get("pending_movement_error")})')
        kind = actor['movement_control']['kind']
        if kind != 'ZombieSwipeWalker':
            raise ValueError(f'actor bank: {actor["name"]} is a {kind}; only the Zombie family is bound (host/actor_bank.py bindings)')
        actor['movement_supported'] = True
        actor.pop('movement_error', None)
        out.append(actor)
    missing = set(e['actors']) - {a['name'] for a in out} - {a['source'] for a in out}
    if missing:
        raise ValueError(f'actor bank: no actor {sorted(missing)} in the scene')
    return out


def bindings(control, library):
    """clip kind -> clip name for a ZombieSwipeWalker: a copy of the chain in host/cook.py append_actor_art, which owns it."""
    if control['parameters'].get('attack', {}).get('kind') == 'Leap':
        b = {'walk': 'Walk', 'turn': 'Turn', 'idle': 'Idle', 'anticipate': 'Attack', 'lunge': 'Attack', 'cooldown': 'Land'}
    else:
        b = {'walk': 'Walk', 'turn': 'Turn', 'idle': 'Idle', 'anticipate': 'Attack Anticipate',
             'lunge': 'Attack Lunge', 'cooldown': 'Attack Cooldown'}
    if any(c['name'] == 'Fall' for c in library['clips']):
        b['fall'] = 'Fall'
    return b


def _wanted(bank, name):
    if 'include_clips' in bank and name not in bank['include_clips']:
        return False
    return name not in bank.get('exclude_clips', [])


def _ents(source, scene, actors, bank):
    """The ents file of one bank: per actor its library, scale and the clips this bank holds, and its corpse's."""
    from effects import corpse_source
    ents, owners = [], []
    for actor in actors:
        control = actor['movement_control']
        library_o = source.ref(scene.file, actor['tk2dSpriteAnimator']['library'])
        library = source.read(library_o)
        matrix = scene.world(scene.go_transform[actor['game_object']])
        sprite = actor['tk2dSprite']
        scale = [abs(matrix[0][0]), abs(matrix[1][1])]
        names = [n for n in dict.fromkeys(bindings(control, library).values()) if _wanted(bank, n)]
        if names:
            ents.append({'library': source.sid(library_o), 'sprite_scale': [sprite['_scale']['x'], sprite['_scale']['y']],
                         'world_scale': scale, 'alpha': sprite['_color']['a'], 'clips': names})
            owners.append((actor, 'live'))
        record = corpse_source(source, scene, actor)
        if record is not None:
            names = [n for n in dict.fromkeys(c['name'] for c in record['clips']) if _wanted(bank, n)]
            if names:
                ents.append({'library': record['library'], 'sprite_scale': [1.0, 1.0], 'world_scale': record['scale'],
                             'alpha': 1.0, 'clips': names})
                owners.append((actor, 'corpse'))
    return ents, owners


def cook(source, scene, scene_name, e, actors):
    """Cook the entry's banks: write data/actor_bank/<scene>/ and return the merged description (see `describe`)."""
    out = OUT / scene_name
    out.mkdir(parents=True, exist_ok=True)
    slices, sprite_base, clip_base = [], 0, 0
    for k, bank in enumerate(e['banks']):
        ents, owners = _ents(source, scene, actors, bank)
        if not ents:
            raise ValueError(f'{scene_name}: bank {bank["name"]!r} holds no clip of the entry\'s actors')
        with tempfile.TemporaryDirectory() as tmp:
            ents_path, bank_dir = Path(tmp) / 'ents.json', Path(tmp) / 'bank'
            ents_path.write_text(json.dumps({'ents': ents}))
            subprocess.run([str(COOK), 'actor-bank', '--ents', str(ents_path), '--out', str(bank_dir),
                            '--id', str(e['id'] + k), '--palettes', str(bank.get('palettes', 8))], check=True, cwd=ROOT)
            blob = (bank_dir / 'bank.bin').read_bytes()
            doc = json.loads((bank_dir / 'bank.json').read_text())
        (out / f'slice{k}.bin').write_bytes(blob + bytes(-len(blob) % 4))
        slices.append({'name': bank['name'], 'id': e['id'] + k, 'doc': doc, 'owners': owners,
                       'sprite_base': sprite_base, 'clip_base': clip_base, 'bytes': len(blob)})
        sprite_base += len(doc['sprites'])
        clip_base += len(doc['clips'])
    if sprite_base > 0xffff:
        raise ValueError(f'{scene_name}: {sprite_base} bank sprites, a frame names one in 16 bits')
    summary = {'scene': scene_name, 'package': e['package'], 'sprites': sprite_base, 'clips': clip_base,
               'slices': [{k: s[k] for k in ('name', 'id', 'bytes', 'sprite_base', 'clip_base')} | {'stats': s['doc']['stats']}
                          for s in slices]}
    (out / 'bank.json').write_text(json.dumps(summary, indent=1))
    return slices


def room_tables(slices):
    """(atlas, frames, clips, carrier count) to append to a bank view, with frame textures and clip starts as
    append_room_bank wants them (the pack offsets them): carriers first, every frame on carrier 0."""
    from cook import Atlas
    atlas = Atlas()
    carriers = 0
    for s in slices:
        for k, palette in enumerate(s['doc']['palettes']):
            pixels = CARRIER_MAGIC + struct.pack('<2H', s['id'], k)
            atlas.add_quantized(4, 4, b''.join(struct.pack('<H', w) for w in palette), pixels, streamed=True, unique=True)
            carriers += 1
    frames, clips = [], []
    for s in slices:
        doc = s['doc']
        for clip in doc['clips']:
            clips.append({'name': f'{clip["library"]}/{clip["name"]}', 'start': len(frames), 'count': len(clip['frames']),
                          'fps': clip['fps'], 'wrap': clip['wrap'], 'loopStart': clip['loop_start']})
            for local in clip['frames']:
                sprite = s['sprite_base'] + local
                box = [round(v * 65536) for v in doc['sprites'][local]['box']]
                # The sprite index rides in the low byte of the first two words (actor_bank::frame_sprite).
                box[0] = (box[0] & ~0xff) | (sprite & 0xff)
                box[1] = (box[1] & ~0xff) | (sprite >> 8)
                frames.append({'texture': 0, 'box': [v / 65536 for v in box], 'sprite': f'bank:{s["id"]}:{local}', 'event': {}})
    return atlas, frames, clips, carriers


def _clip_index(slices, actor, which, name):
    """The scene-wide bank clip (all slices numbered in order) that `actor`'s `which` ('live' or 'corpse') ent holds as `name`."""
    for s in slices:
        for i, (owner, kind) in enumerate(s['owners']):
            if owner is actor and kind == which and name in s['doc']['ent_clips'][i]:
                return s['clip_base'] + s['doc']['ent_clips'][i][name]
    raise ValueError(f'actor bank: {actor["name"]} has no {which} clip {name!r} in any slice')


def bind(source, scene, actor, slices, first_clip):
    """Point `actor`'s clip bindings (and its corpse's) at its bank clips, `first_clip` being the index of the first bank clip
    in a bank view before the scene's own clip base is added (regions.remap_actor_clips adds that)."""
    from effects import corpse_source
    library_o = source.ref(scene.file, actor['tk2dSpriteAnimator']['library'])
    library = source.read(library_o)
    matrix = scene.world(scene.go_transform[actor['game_object']])
    sprite = actor['tk2dSprite']
    for kind, name in bindings(actor['movement_control'], library).items():
        actor[kind + '_clip'] = first_clip + _clip_index(slices, actor, 'live', name)
    actor['visual_scale'] = [abs(matrix[0][0] * sprite['_scale']['x']), abs(matrix[1][1] * sprite['_scale']['y'])]
    record = corpse_source(source, scene, actor)
    if record is not None:
        record.pop('library_object')
        selected = record.pop('clips')
        record.pop('tiled', None)
        for kind, clip in zip(('air', 'land'), selected):
            record[kind + '_clip'] = first_clip + _clip_index(slices, actor, 'corpse', clip['name'])
        actor['corpse'] = record


def append_to_pack(pack, slices, clip_base, what):
    """The cooked view `pack` with the bank appended: (new pack, carriers added, first bank clip index in the pack)."""
    from regions import append_room_bank
    atlas, frames, clips, carriers = room_tables(slices)
    atlas.pack()
    nc = struct.unpack_from('<6I', pack, 8)[4]
    new, _ = append_room_bank(pack, atlas, frames, clips, nc, what)
    return bytes(new), carriers, nc


def write_tables(manifest_index):
    """data/actor_banks.rs: the scenes that draw from a bank, as `actor_bank::SceneBank` rows. `manifest_index(scene name)` is
    the scene's index in the packed manifest; the package id is the generated `modules::ART_*` constant of its name. Written whenever the contents change, empty for an empty config."""
    rows = []
    for name, e in scenes().items():
        late = late_slices(name)
        slot = f'include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/{late[0]}"))' if late else '&[]'
        rows.append(f'SceneBank{{scene:{manifest_index(name)},package:crate::modules::{e["package"].upper()},late:{slot}}}')
    text = ('// Generated by host/actor_bank.py from host/actor_banks.json; do not edit.\n'
            f'pub static SCENE_BANKS:[SceneBank;{len(rows)}]=[{",".join(rows)}];\n')
    path = ROOT / 'data/actor_banks.rs'
    if not path.is_file() or path.read_text() != text:
        path.write_text(text)
