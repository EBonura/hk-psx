"""Talkable NPC placements: the talk trigger, the standing art and the prompt.

`npc_control` is one template FSM every talkable NPC carries. It owns the range
trigger, which is the NPC object's own enabled trigger collider, raises the
prompt marker while the hero body is inside it and starts the conversation on
UP. `Conversation Control` then picks the line and plays `Talk Left` or
`Talk Right` on the NPC: `Hero Is Left` and `Hero Is Right` both compare the
NPC's own localScale.x against zero, and Elderbug's is +1.25, so the hero's side
is the clip's own direction and neither state can mirror the choice.

Admission is an explicit name list rather than the recognizer in npcs.py,
because only one NPC is implemented: an NPC gated on PlayerData, or one whose
conversation needs a branch the port cannot pick, must not be cooked into a
region by accident. The turn, reach, slump, flower and shiver clips, the prompt
marker art, the hero alignment walk and the NPC name plate are not cooked.
"""
from cook import FOCAL, CAM_Z, guest_wrap, tk_sprite
from focus import fsm_variables
# The parent walk and the deactivator component names are the NPC extractor's,
# and this cooker must reject exactly what that extractor calls a gate.
from npcs import DEACTIVATORS, _ancestors

# The clip order the guest's cooked clip base indexes: Idle, then the two talk
# clips, so one payload word binds all three.
NPC_CLIPS = ('Idle', 'Talk Left', 'Talk Right')
# Ungated on a fresh save, reachable from the opening route, and its opening
# conversation needs no player choice. .hkpsx/npc-catalog.json is the survey.
ADMITTED = ('Elderbug',)
CONTROL_FSM = 'npc_control'
CONVO_FSM = 'Conversation Control'
# tk2d frames reach VRAM through the shared 64x64 animation slots.
FRAME_LIMIT = 64


def _component(sc, gid, kind):
    """The one component of this kind on an object, or None."""
    found = [tree for _cid, (typ, tree) in sc.objects.items()
             if typ == kind and tree.get('m_GameObject', {}).get('m_PathID') == gid]
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


def _refuse_if_gated(sc, gid):
    """A Deactivate component anywhere above the NPC removes it on some saves."""
    for owner in [gid] + _ancestors(sc, gid):
        for kind in DEACTIVATORS:
            if _component(sc, owner, kind) is not None:
                raise ValueError(f'{sc.gos[gid]["m_Name"]} is gated by {kind} on '
                                 f'{sc.gos[owner]["m_Name"]}; only ungated NPCs are cooked')


def npc_sources(sc, bounds):
    """Every admitted NPC whose talk trigger touches this region's envelope."""
    scene_file = sc.file.name.split('/')[-1]
    result = []
    for gid, go in sorted(sc.gos.items()):
        if go['m_Name'] not in ADMITTED or not sc.active(gid):
            continue
        control = _fsm(sc, gid, CONTROL_FSM)
        if control is None or _fsm(sc, gid, CONVO_FSM) is None:
            continue
        _refuse_if_gated(sc, gid)
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
        result.append({
            'source': f'{scene_file}:{gid}', 'game_object': gid, 'name': go['m_Name'],
            'position': [position[0], position[1]], 'bounds': trigger,
            'marker': [marker[0], marker[1]],
            'prompt': prompt,
            'default_clip': NPC_CLIPS[0],
            'plays_automatically': bool(animator['playAutomatically']),
            'clips': list(NPC_CLIPS),
            'limitations': ['Idle, Talk Left and Talk Right only: the turn, reach, slump, '
                            'flower and shiver clips, the prompt marker art, the hero '
                            'alignment walk and the name plate are not cooked'],
        })
    return result


def append_npc_art(source, sc, npcs, atlas, frames, clips):
    """Append each NPC's three clips to one room bank, in NPC_CLIPS order."""
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
        # so a mirrored NPC would invert the guest's own side test.
        if matrix[0][0] <= 0:
            raise ValueError(f'{npc["name"]} is mirrored; the talk clip choice is unmodelled')
        colour = sprite['_color']
        npc['clip_base'] = len(clips)
        npc['visual_scale'] = [sx, sy]
        npc['library'] = source.sid(library_o)
        for name in npc['clips']:
            clip = next(c for c in library['clips'] if c['name'] == name)
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
                    if width > FRAME_LIMIT or height > FRAME_LIMIT:
                        raise ValueError(f'{npc["name"]} frame {width:.0f}x{height:.0f} exceeds '
                                         f'the {FRAME_LIMIT}x{FRAME_LIMIT} animation cache')
                    cache[key] = (atlas.add(image, width, height, streamed=True), box)
                texture, box = cache[key]
                frames.append({'texture': texture, 'box': box, 'sprite': f'{sid}:{index}', 'event': frame})
            clips.append({'name': f'{source.sid(library_o)}/{name}', 'start': start,
                          'count': len(clip['frames']), 'fps': clip['fps'],
                          'wrap': guest_wrap(clip), 'loopStart': clip.get('loopStart', 0)})
    return npcs
