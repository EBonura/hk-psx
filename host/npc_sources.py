"""Talkable NPC placements: the talk trigger, the standing art and the prompt.

`npc_control` is one template FSM every talkable NPC carries. It owns the range
trigger, which is the NPC object's own enabled trigger collider, raises the
prompt marker while the hero body is inside it and starts the conversation on
UP. `Conversation Control` then picks the line and plays the talk clip.

Admission is an explicit table rather than the recognizer in npcs.py. The
recognizer finds 27 NPCs across the 45 admitted scenes and almost none of them
has a shape this cooker can hold: see `.hkpsx/npc-catalog.json` for the survey
and `.hkpsx/npc-lines.json` for what each cook actually did.

The table carries the clip binding too, because the libraries disagree on what
the talk clips are called. Elderbug's names `Talk Left` and `Talk Right`, and
his `Hero Is Left`/`Hero Is Right` compare his own localScale.x, which is +1.25,
so the hero's side is the clip's own direction. Everyone else has a single
`Talk`, or a seated pair, and turns by mirroring instead. Reading the names and
guessing would bind the wrong clip in silence, so the binding is written down.

Not cooked: every other clip in the library, the prompt marker art, the hero
alignment walk and the NPC name plate.
"""
from functools import lru_cache

from cook import FOCAL, CAM_Z, guest_wrap, tk_sprite
from focus import fsm_variables
# The parent walk, the deactivator component names and the new-save PlayerData
# are the NPC extractor's, so this cooker reads a gate exactly as it does.
from npcs import DEACTIVATORS, playerdata_defaults, _ancestors

# The three guest animation slots one cooked clip base indexes, in this order,
# so one payload word binds all of them.
NPC_SLOTS = ('idle', 'talk left', 'talk right')
# (scene, object) -> the source clip each slot plays. Present on a fresh save
# with the gates activation.py evaluates, standing in its own default clip, and
# with a conversation chain npcs.py can decide end to end.
#
# Elderbug's `Hero Is Left`/`Hero Is Right` compare his own localScale.x, which
# is +1.25, so the hero's side is the clip's own direction. Myla has a single
# `Talk` and turns by mirroring instead, which the port does not reproduce, so
# both talk slots hold it and she keeps facing the way the scene placed her.
#
# Cornifer idles through his conversation. His view (Crossroads_33 chunk 410)
# is 358 KB before any NPC art against the 384 KB staging budget, and his
# three clips cook to 59,660 texel bytes (29 frames); `Idle` alone is 9 frames
# and fits. His `Talk L` / `Talk R` (chosen by `Check Direction`) are not cooked.
ADMITTED = {
    ('Town', 'Elderbug'): ('Idle', 'Talk Left', 'Talk Right'),
    ('Crossroads_45', 'Miner'): ('Idle', 'Talk', 'Talk'),
    ('Crossroads_33', 'Cornifer'): ('Idle', 'Idle', 'Idle'),
}
# NPCs whose conversation is not a chain npc_dialogue.py can cook, because it
# ends in a yes/no purchase. Their art and trigger cook here as usual; their
# conversation is host/cornifer.py and game/src/mapper.rs.
SCRIPTED = {('Crossroads_33', 'Cornifer')}
CONTROL_FSM = 'npc_control'
CONVO_FSM = 'Conversation Control'
# tk2d frames reach VRAM through the shared 64x64 animation slots. A frame
# larger than one slot binds a rectangle of them, up to the budget the four
# keys main.rs always holds leave free; Atlas.add_tiled enforces that.


def _components(sc, gid, kind):
    """Every component of this kind on an object, in file order."""
    return [tree for _cid, (typ, tree) in sc.objects.items()
            if typ == kind and tree.get('m_GameObject', {}).get('m_PathID') == gid]


def _component(sc, gid, kind):
    """The one component of this kind on an object, or None."""
    found = _components(sc, gid, kind)
    if len(found) > 1:
        raise ValueError(f'{sc.gos[gid]["m_Name"]} carries {len(found)} {kind} components')
    return found[0] if found else None


def _fsm(sc, gid, name):
    for _cid, (typ, tree) in sc.objects.items():
        if typ == 'PlayMakerFSM' and tree['m_GameObject']['m_PathID'] == gid and tree['fsm']['name'] == name:
            return tree['fsm']
    return None


def _trigger(sc, gid):
    """The npc_control range box, in world units."""
    boxes = [tree for _cid, (typ, tree) in sc.objects.items()
             if typ == 'BoxCollider2D' and tree['m_GameObject']['m_PathID'] == gid
             and tree['m_Enabled'] and tree['m_IsTrigger']]
    if len(boxes) != 1:
        raise ValueError(f'{sc.gos[gid]["m_Name"]} has {len(boxes)} talk triggers')
    off = boxes[0]['m_Offset']
    size = boxes[0]['m_Size']
    corners = [sc.point(gid, x + off['x'], y + off['y'])[:2] for x, y in
               [(-size['x'] / 2, -size['y'] / 2), (size['x'] / 2, size['y'] / 2)]]
    return [min(corners[0][0], corners[1][0]), min(corners[0][1], corners[1][1]),
            max(corners[0][0], corners[1][0]), max(corners[0][1], corners[1][1])]


def _child(sc, gid, name):
    transform = sc.transforms[sc.go_transform[gid]]
    for ref in transform['m_Children']:
        kid = sc.transforms[ref['m_PathID']]['m_GameObject']['m_PathID']
        if kid in sc.gos and sc.gos[kid]['m_Name'] == name:
            return kid
    raise ValueError(f'{sc.gos[gid]["m_Name"]} has no {name} child')


@lru_cache(maxsize=1)
def _new_save(assembly):
    """SetupNewPlayerData, decompiled once rather than once per region."""
    return playerdata_defaults(assembly)


def _gates(sc, gid):
    """The Deactivate components on the NPC or above it, and what they read.

    `Scene.active` has already applied the ones that fire on a fresh save, which
    is the answer the whole cooked world is built from: the port has no producer
    for any of these fields, so a gate that does not fire today cannot start
    firing later. Those survivors are recorded rather than refused, because the
    divergence from a save that does set them belongs in the report. A gate whose
    field is not a new-save default is refused instead: nothing here knows
    whether that NPC exists.
    """
    defaults = _new_save(sc.source.directory / 'Managed/Assembly-CSharp.dll')
    out = []
    for owner in [gid] + _ancestors(sc, gid):
        for kind in DEACTIVATORS:
            for component in _components(sc, owner, kind):
                field = component['boolName']
                if field not in defaults:
                    raise ValueError(f'{sc.gos[gid]["m_Name"]} sits under {kind}({field}) on '
                                     f'{sc.gos[owner]["m_Name"]}, and {field} is not a new-save '
                                     'default; whether it exists is unanswered')
                out.append(f'{kind}({field}) on {sc.gos[owner]["m_Name"]}')
    return out


def npc_sources(scene_name, sc, bounds):
    """Every admitted NPC whose talk trigger touches this region's envelope."""
    scene_file = sc.file.name.split('/')[-1]
    result = []
    for gid, go in sorted(sc.gos.items()):
        binding = ADMITTED.get((scene_name, go['m_Name']))
        if binding is None or not sc.active(gid):
            continue
        control = _fsm(sc, gid, CONTROL_FSM)
        if control is None or _fsm(sc, gid, CONVO_FSM) is None:
            continue
        variables = fsm_variables(control)
        if not variables.get('Can Talk'):
            raise ValueError(f'{go["m_Name"]} carries Can Talk clear; it answers no prompt')
        prompt = variables.get('Prompt Name')
        if not isinstance(prompt, str) or not prompt:
            raise ValueError(f'{go["m_Name"]} carries no npc_control Prompt Name')
        trigger = _trigger(sc, gid)
        if not (trigger[0] <= bounds[2] and trigger[2] >= bounds[0]
                and trigger[1] <= bounds[3] and trigger[3] >= bounds[1]):
            continue
        position = sc.point(gid)
        marker = sc.point(_child(sc, gid, 'Prompt Marker'))
        animator = _component(sc, gid, 'tk2dSpriteAnimator')
        gates = _gates(sc, gid)
        limitations = [f'{" / ".join(dict.fromkeys(binding))} only: the other clips, the '
                       'prompt marker art, the hero alignment walk and the name plate '
                       'are not cooked']
        if binding[1] == binding[2]:
            limitations.append('one talk clip for both sides: the source mirrors the NPC '
                               'to face the hero and the port does not')
        if gates:
            limitations.append('present because none of these fires on a fresh save: '
                               + ', '.join(gates))
        result.append({
            'source': f'{scene_file}:{gid}', 'game_object': gid, 'name': go['m_Name'],
            'scripted': (scene_name, go['m_Name']) in SCRIPTED,
            'position': [position[0], position[1]], 'bounds': trigger,
            'marker': [marker[0], marker[1]],
            'prompt': prompt,
            'default_clip': binding[0],
            'plays_automatically': bool(animator['playAutomatically']),
            'clips': list(binding), 'slots': list(NPC_SLOTS), 'gates': gates,
            'limitations': limitations,
        })
    return result


def append_npc_art(source, sc, npcs, atlas, frames, clips):
    """Append each NPC's three clips to one room bank, in NPC_SLOTS order.

    A binding may name the same source clip twice; the atlas keys its images by
    sprite, so the second slot costs frame records and no texture.
    """
    textures, collections, cache = {}, {}, {}
    scale = FOCAL / -CAM_Z
    for npc in npcs:
        gid = npc['game_object']
        animator = _component(sc, gid, 'tk2dSpriteAnimator')
        sprite = _component(sc, gid, 'tk2dSprite')
        library_o = source.ref(sc.file, animator['library'])
        library = source.read(library_o)
        # The guest stands the NPC in its default clip with no FSM behind it,
        # which is only what the scene shows if the animator starts it itself.
        default = library['clips'][animator['defaultClipId']]['name']
        if default != npc['default_clip'] or not animator['playAutomatically']:
            raise ValueError(f'{npc["name"]} no longer idles on {npc["default_clip"]!r}')
        matrix = sc.world(sc.go_transform[gid])
        sx = abs(matrix[0][0] * sprite['_scale']['x'])
        sy = abs(matrix[1][1] * sprite['_scale']['y'])
        # Hero Is Left / Hero Is Right read this scale to choose the talk clip,
        # so a mirrored NPC would invert the guest's own side test, and the
        # cooked frames would face the wrong way besides.
        if matrix[0][0] <= 0:
            raise ValueError(f'{npc["name"]} is mirrored; the talk clip choice is unmodelled')
        colour = sprite['_color']
        npc['clip_base'] = len(clips)
        npc['visual_scale'] = [sx, sy]
        npc['library'] = source.sid(library_o)
        for name in npc['clips']:
            clip = next((c for c in library['clips'] if c['name'] == name), None)
            if clip is None:
                raise ValueError(f'{npc["name"]}\'s library has no {name!r} clip')
            start = len(frames)
            for frame in clip['frames']:
                collection_o = source.ref(library_o.assets_file, frame['spriteCollection'])
                sid = source.sid(collection_o)
                if sid not in collections:
                    collections[sid] = source.read(collection_o)
                index = frame['spriteId']
                key = (sid, index, sx, sy, colour['a'])
                if key not in cache:
                    image, box = tk_sprite(source, collection_o.assets_file, collections[sid], index, textures)
                    image.putalpha(image.getchannel('A').point(lambda a: round(a * colour['a'])))
                    box = (box[0] * sx, box[1] * sy, box[2] * sx, box[3] * sy)
                    width, height = (box[2] - box[0]) * scale, (box[3] - box[1]) * scale
                    try:
                        cache[key] = (atlas.add_tiled(image, width, height), box)
                    except ValueError as error:
                        raise ValueError(f'{npc["name"]} frame '
                                         f'{width:.0f}x{height:.0f}: {error}') from error
                texture, box = cache[key]
                frames.append({'texture': texture, 'box': box, 'sprite': f'{sid}:{index}', 'event': frame})
            clips.append({'name': f'{source.sid(library_o)}/{name}', 'start': start,
                          'count': len(clip['frames']), 'fps': clip['fps'],
                          'wrap': guest_wrap(clip), 'loopStart': clip.get('loopStart', 0)})
    return npcs
