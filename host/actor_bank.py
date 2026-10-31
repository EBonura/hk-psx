"""Compact art banks for ordinary actor families (game/src/actor_bank.rs, docs/ACTOR-BANK.md).

A scene opts in by data: an entry in host/actor_banks.json names the actors whose art leaves the room path and rides a bank
instead, the views that draw them, and how much of the bank lives in the module pool. Nothing else in the cook knows about it:
with the file's `scenes` empty, every function here returns an empty answer and the cooked bytes are what they were before it
existed.

    {"scenes": {"Crossroads_10": {
        "id": 6,                         # the bank's id (carriers name it)
        "package": "art_husks",          # the module pool data package that carries the head of the bank
        "package_bytes": 35000,          # how many bytes of the bank the package takes (a multiple of 4; the rest is linked data)
        "palettes": 8,                   # shared palettes: one carrier texture each in every bank view
        "actors": ["Zombie Barger"],     # actor names in the scene (`source` ids also work)
        "reach": 8,                      # optional: widen the default views by this many world units (see below)
        "views": [235, 236]}}}           # optional chunk ids of the views that draw them; "all": every view of the scene;
                                         # omitted: the views whose region cook lists the actors, grown by `reach`

The bank is one logical byte string (a `hk_format::boss::Bank`: tables, then the parts' coded texels, live clips first and the
corpse clips last). The cook writes data/actor_bank/<scene>/package.bin (its first `package_bytes`), linked.bin (the rest) and
bank.json. The package is the scene's module pool data package (host/code_modules.py DATA_MODULES) and the rest is linked
read-only data (`data/actor_banks.rs`); the guest reads the two as one string. Because the bank is cut by byte, what sits in
the pool and what sits in the image is a number to tune against the room's pool slack and the free RAM below the pool, not a
property of the art.

In a bank view the actors' frames are ordinary frame records whose texture is the first carrier texture and whose first two
box words carry the sprite's index in their low bytes; the clips and frames are appended after every other clip, so a view
without the bank keeps the clip indices of the rest. A bank actor is listed (`row['actors']`) only in the views that carry the
bank; the guest does not step or draw an actor its bound view does not list.

Which views carry the bank matters for pop-in: a view draws every actor it lists and no other, and the renderer draws the view
whose camera range holds the camera, which can be a neighbour of the Knight's. So an actor must be listed in every view from
which it can be on screen. The default is the views whose interaction bounds (the camera range and its margin) hold an
actor's placement; `reach` grows those bounds for actors that wander or stand near a view edge, and `views` overrides the
rule (`"all"` lists every view of the scene, which costs the carriers and the clip and frame records in every one).
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


def scenes():
    """{scene name: entry} of host/actor_banks.json."""
    entries = json.loads(CONFIG.read_text())['scenes']
    for name, e in entries.items():
        if not e['package'].startswith('art_'):
            raise ValueError(f'{name}: package {e["package"]!r} must be named art_*')
        if e['package_bytes'] % 4 or e['package_bytes'] <= 0:
            raise ValueError(f'{name}: package_bytes must be a positive multiple of 4')
        if not 1 <= e.get('palettes', 8) <= 16:
            raise ValueError(f'{name}: palettes must be 1 to 16')
    return entries


def entry(scene_name):
    return scenes().get(scene_name)


def packages():
    """{package name: scene name}, in file order: the data packages the cook adds to host/code_modules.py DATA_MODULES."""
    return {e['package']: name for name, e in scenes().items()}


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


def _ents(source, scene, actors):
    """The ents file of the bank: every actor's live clips, then every actor's corpse clips. The cooker numbers sprites in
    the order it meets them, so the corpse art (seen only after a kill) is the tail of the bank, which is the end of the blob:
    what spills out of the pool package into the image."""
    from effects import corpse_source
    live, corpse = [], []
    for actor in actors:
        control = actor['movement_control']
        library_o = source.ref(scene.file, actor['tk2dSpriteAnimator']['library'])
        library = source.read(library_o)
        matrix = scene.world(scene.go_transform[actor['game_object']])
        sprite = actor['tk2dSprite']
        live.append(({'library': source.sid(library_o), 'sprite_scale': [sprite['_scale']['x'], sprite['_scale']['y']],
                      'world_scale': [abs(matrix[0][0]), abs(matrix[1][1])], 'alpha': sprite['_color']['a'],
                      'clips': list(dict.fromkeys(bindings(control, library).values()))}, (actor, 'live')))
        record = corpse_source(source, scene, actor)
        if record is not None:
            corpse.append(({'library': record['library'], 'sprite_scale': [1.0, 1.0], 'world_scale': record['scale'],
                            'alpha': 1.0, 'clips': list(dict.fromkeys(c['name'] for c in record['clips']))}, (actor, 'corpse')))
    both = live + corpse
    return [e for e, _ in both], [o for _, o in both]


def cook(source, scene, scene_name, e, actors):
    """Cook the entry's bank: write data/actor_bank/<scene>/ and return its description."""
    out = OUT / scene_name
    out.mkdir(parents=True, exist_ok=True)
    ents, owners = _ents(source, scene, actors)
    with tempfile.TemporaryDirectory() as tmp:
        ents_path, bank_dir = Path(tmp) / 'ents.json', Path(tmp) / 'bank'
        ents_path.write_text(json.dumps({'ents': ents}))
        subprocess.run([str(COOK), 'actor-bank', '--ents', str(ents_path), '--out', str(bank_dir),
                        '--id', str(e['id']), '--palettes', str(e.get('palettes', 8))], check=True, cwd=ROOT)
        blob = (bank_dir / 'bank.bin').read_bytes()
        doc = json.loads((bank_dir / 'bank.json').read_text())
    blob += bytes(-len(blob) % 4)
    cut = min(e['package_bytes'], len(blob))
    (out / 'package.bin').write_bytes(blob[:cut])
    (out / 'linked.bin').write_bytes(blob[cut:])
    if len(doc['sprites']) > 0xffff:
        raise ValueError(f'{scene_name}: {len(doc["sprites"])} bank sprites, a frame names one in 16 bits')
    (out / 'bank.json').write_text(json.dumps({
        'scene': scene_name, 'package': e['package'], 'id': e['id'], 'bytes': len(blob), 'package_bytes': cut,
        'linked_bytes': len(blob) - cut, 'sprites': len(doc['sprites']), 'clips': len(doc['clips']),
        'palettes': len(doc['palettes']), 'stats': doc['stats']}, indent=1))
    return {'id': e['id'], 'doc': doc, 'owners': owners, 'bytes': len(blob), 'package_bytes': cut}


def room_tables(bank):
    """(atlas, frames, clips, carrier count) to append to a bank view, with frame textures and clip starts as
    append_room_bank wants them (the pack offsets them): carriers first, every frame on carrier 0."""
    from cook import Atlas
    atlas = Atlas()
    doc = bank['doc']
    for k, palette in enumerate(doc['palettes']):
        pixels = CARRIER_MAGIC + struct.pack('<2H', bank['id'], k)
        atlas.add_quantized(4, 4, b''.join(struct.pack('<H', w) for w in palette), pixels, streamed=True, unique=True)
    frames, clips = [], []
    for clip in doc['clips']:
        clips.append({'name': f'{clip["library"]}/{clip["name"]}', 'start': len(frames), 'count': len(clip['frames']),
                      'fps': clip['fps'], 'wrap': clip['wrap'], 'loopStart': clip['loop_start']})
        for sprite in clip['frames']:
            box = [round(v * 65536) for v in doc['sprites'][sprite]['box']]
            # The sprite index rides in the low byte of the first two words (actor_bank::frame_sprite).
            box[0] = (box[0] & ~0xff) | (sprite & 0xff)
            box[1] = (box[1] & ~0xff) | (sprite >> 8)
            frames.append({'texture': 0, 'box': [v / 65536 for v in box], 'sprite': f'bank:{bank["id"]}:{sprite}', 'event': {}})
    return atlas, frames, clips, len(doc['palettes'])


def _clip_index(bank, actor, which, name):
    """The bank clip that `actor`'s `which` ('live' or 'corpse') ent holds as `name`."""
    for i, (owner, kind) in enumerate(bank['owners']):
        if owner is actor and kind == which and name in bank['doc']['ent_clips'][i]:
            return bank['doc']['ent_clips'][i][name]
    raise ValueError(f'actor bank: {actor["name"]} has no {which} clip {name!r}')


def bind(source, scene, actor, bank, first_clip):
    """Point `actor`'s clip bindings (and its corpse's) at its bank clips, `first_clip` being the index of the first bank clip
    in a bank view before the scene's own clip base is added (regions.remap_actor_clips adds that)."""
    from effects import corpse_source
    library_o = source.ref(scene.file, actor['tk2dSpriteAnimator']['library'])
    library = source.read(library_o)
    matrix = scene.world(scene.go_transform[actor['game_object']])
    sprite = actor['tk2dSprite']
    for kind, name in bindings(actor['movement_control'], library).items():
        actor[kind + '_clip'] = first_clip + _clip_index(bank, actor, 'live', name)
    actor['visual_scale'] = [abs(matrix[0][0] * sprite['_scale']['x']), abs(matrix[1][1] * sprite['_scale']['y'])]
    record = corpse_source(source, scene, actor)
    if record is not None:
        record.pop('library_object')
        selected = record.pop('clips')
        record.pop('tiled', None)
        for kind, clip in zip(('air', 'land'), selected):
            record[kind + '_clip'] = first_clip + _clip_index(bank, actor, 'corpse', clip['name'])
        actor['corpse'] = record


def append_to_pack(pack, bank, what):
    """The cooked view `pack` with the bank appended: (new pack, carriers added, first bank clip index in the pack)."""
    from regions import append_room_bank
    atlas, frames, clips, carriers = room_tables(bank)
    atlas.pack()
    nc = struct.unpack_from('<6I', pack, 8)[4]
    new, _ = append_room_bank(pack, atlas, frames, clips, nc, what)
    return bytes(new), carriers, nc


def write_tables(manifest_index):
    """data/actor_banks.rs: the scenes that draw from a bank, as `actor_bank::SceneBank` rows. `manifest_index(scene name)` is
    the scene's index in the packed manifest; the package id is the generated `modules::ART_*` constant of its name. Written
    whenever the contents change; empty for an empty config."""
    rows = []
    for name, e in scenes().items():
        rest = f'include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/actor_bank/{name}/linked.bin"))'
        rows.append(f'SceneBank{{scene:{manifest_index(name)},package:crate::modules::{e["package"].upper()},rest:{rest}}}')
    text = ('// Generated by host/actor_bank.py from host/actor_banks.json; do not edit.\n'
            f'pub static SCENE_BANKS:[SceneBank;{len(rows)}]=[{",".join(rows)}];\n')
    path = ROOT / 'data/actor_banks.rs'
    if not path.is_file() or path.read_text() != text:
        path.write_text(text)
