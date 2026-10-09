"""Source-driven Breakable C# subset; source data and CIL evidence stay local.

Breakable.Hit calls Break on the first valid hit, with no health counter.
Break toggles authored whole/remnant objects, disables colliders and sets its
isBroken flag. PersistentBoolItem callbacks expose that flag.

Breakable.Break is one behaviour for the whole catalogue; what differs between
instances is the destruction output the scene hangs off it. So a family here is
the shape of that output, read from the instance's own serialized fields and
components. Nothing is keyed on a scene or object name: the same barrel appears
under a dozen names and the same name appears with different parts.
"""
import math
import re
import struct
from pathlib import Path

MAX_SCENE_BREAKABLES = 128
MAX_HIT_POINTS = 16

# The outputs Breakable.Break produces, each traced to the source member that
# authors it. An instance's contract is the subset it actually authors.
OUTPUT_VISUAL = 'visual'        # SetStaticPartsActivation: wholeParts off, remnantParts on
OUTPUT_COLLIDER = 'collider'    # Break disables the body collider and whole colliders
OUTPUT_AUDIO = 'audio'          # breakAudioEvent / breakAudioClipTable
OUTPUT_PARTICLES = 'particles'  # debrisParts carrying a ParticleSystem
OUTPUT_FRAGMENTS = 'fragments'  # debrisParts flung as rigid bodies at flingSpeed
OUTPUT_MASK = 'mask'            # hitEventReciever forwarding HIT to a mask fade
OUTPUTS = (OUTPUT_VISUAL, OUTPUT_COLLIDER, OUTPUT_AUDIO, OUTPUT_PARTICLES,
           OUTPUT_FRAGMENTS, OUTPUT_MASK)

# The one break sample resident in the SPU bank below 0x14000 (data/sfx.rs
# entry 0). The other four clips the catalogue's tables name need 50,864 bytes
# against 14,432 free there, measured from their serialized lengths, so an
# instance that wants one of them has no reproducible break sound today.
RESIDENT_BREAK_CLIPS = ('breakable_wall_hit_1',)


def _local_id(ref):
    if ref['m_FileID']:
        raise ValueError('external Breakable scene-object reference unsupported')
    return ref['m_PathID']


def _components(sc, gid):
    for component in sc.gos[gid]['m_Component']:
        index = _local_id(component['component'])
        if index in sc.objects:
            typ, tree = sc.objects[index]
            yield index, typ, tree


def _descendants(sc, root):
    children = {}
    for tid, tree in sc.transforms.items():
        children.setdefault(tree['m_Father']['m_PathID'], []).append(tid)
    stack = [sc.go_transform[root]]
    result = set()
    while stack:
        tid = stack.pop()
        if tid in result:
            raise ValueError('cyclic Breakable part hierarchy')
        result.add(tid)
        stack.extend(children.get(tid, []))
    return {sc.transforms[tid]['m_GameObject']['m_PathID'] for tid in result}


def collider_polygons(sc, gid, typ, tree):
    offset = tree['m_Offset']
    if typ == 'BoxCollider2D':
        x, y = tree['m_Size']['x'] / 2, tree['m_Size']['y'] / 2
        paths = [[{'x': a, 'y': b} for a, b in [(-x,-y),(x,-y),(x,y),(-x,y)]]]
    elif typ == 'PolygonCollider2D':
        paths = tree['m_Points']['m_Paths']
    else:
        raise ValueError(f'unsupported Breakable hit collider: {typ}')
    result = []
    for path in paths:
        if not 3 <= len(path) <= MAX_HIT_POINTS:
            raise ValueError('Breakable hit polygon requires 3..16 source vertices')
        points = [sc.point(gid, p['x']+offset['x'], p['y']+offset['y'])[:2] for p in path]
        if any(not math.isfinite(v) or abs(v)>512 for point in points for v in point):
            raise ValueError('Breakable world coordinate exceeds bounded Q16 range')
        result.append(points)
    if not result:
        raise ValueError('Breakable has no hit polygon')
    return result


def _clip(source, file, ref, weight):
    clip = source.ref(file, ref)
    data = source.read(clip)
    return {'source': source.sid(clip), 'weight': weight, 'name': data['m_Name'],
            'channels': data['m_Channels'], 'frequency': data['m_Frequency'],
            'seconds': data['m_Length'], 'compression_format': data['m_CompressionFormat'],
            'resource': data['m_Resource']}


def _audio(sc, tree):
    """Both source break-sound paths, not only the weighted table.

    Breakable.Break plays breakAudioClipTable when one is assigned and
    breakAudioEvent otherwise. Reading only the table left 18 catalogue
    instances looking silent when the scene actually authors a single clip.
    """
    source = sc.source
    out = {'event': tree['breakAudioEvent'], 'table_id': None, 'options': []}
    table_ref = tree['breakAudioClipTable']
    if table_ref['m_PathID']:
        obj = source.ref(sc.file, table_ref)
        table = source.read(obj)
        out.update(table_id=source.sid(obj), pitch_min=table['pitchMin'], pitch_max=table['pitchMax'])
        for option in table['options']:
            if option['Clip']['m_PathID']:
                out['options'].append(_clip(source, obj.assets_file, option['Clip'], option['Weight']))
    elif tree['breakAudioEvent']['Clip']['m_PathID']:
        event = tree['breakAudioEvent']
        out.update(event_clip=True, pitch_min=event['PitchMin'], pitch_max=event['PitchMax'])
        out['options'].append(_clip(source, sc.file, event['Clip'], 1.0))
    return out


# Serialized rigid-fling variants, measured across every debrisPart of every
# Breakable in the 45 admitted scenes (355 parts under 193 owners). Listing the
# observed values rather than pinning one keeps the definition shared without
# admitting a body the debris solver has never been shown.
RIGID_BODIES = ((0, 1.0, 0.05, 1.0, 0), (0, 1.0, 0.05, 0.9, 0), (0, 1.0, 3.0, 1.0, 0))
RIGID_BOUNCE_FACTORS = (0.1, 0.4, 0.5)
# SpinSelf turns with the fragment's own speed; SpinSelfSimple turns at a fixed
# rate. A part with neither keeps the launch rotation.
SPIN_COMPONENTS = ('SpinSelf', 'SpinSelfSimple')


def _close(value, expected, tolerance=1e-5):
    return abs(value - expected) <= tolerance


def rigid_fragment(sc, part_gid):
    """The shared rigid-fling definition, or a reason this part is not one.

    Reproduces Breakable.Break's FlingObject path: the part is detached, given a
    speed inside flingSpeedMin..Max at angleOffset, and left to ObjectBounce.
    Returns None when the part is not a rigid fling at all (a particle emitter,
    say); raises ValueError when it is one the solver has not been shown.
    """
    components = {typ: (index, data) for index, typ, data in _components(sc, part_gid)}
    if 'Rigidbody2D' not in components or 'SpriteRenderer' not in components:
        return None
    colliders = [typ for typ in components if typ.endswith('Collider2D')]
    spins = [name for name in SPIN_COMPONENTS if name in components]
    if len(colliders) != 1:
        raise ValueError(f'rigid fragment needs exactly one collider, has {len(colliders)}')
    if len(spins) > 1:
        raise ValueError('rigid fragment carries two spin behaviours')
    if 'ObjectBounce' not in components:
        raise ValueError('rigid fragment has no ObjectBounce landing behaviour')
    body = components['Rigidbody2D'][1]
    fields = (body['m_BodyType'], body['m_Mass'], round(body['m_AngularDamping'], 4),
              round(body['m_GravityScale'], 4), body['m_Constraints'])
    if body['m_UseAutoMass'] or body['m_LinearDamping']:
        raise ValueError('rigid fragment uses auto mass or linear damping')
    if not any(all(_close(a, b) for a, b in zip(fields, variant)) for variant in RIGID_BODIES):
        raise ValueError(f'unmeasured rigid fragment body {fields}')
    bounce = components['ObjectBounce'][1]
    if not any(_close(bounce['bounceFactor'], v) for v in RIGID_BOUNCE_FACTORS):
        raise ValueError(f"unmeasured fragment bounce factor {bounce['bounceFactor']}")
    if bounce['speedThreshold'] != 1 or any(bounce[k] for k in ('playSound', 'playAnimationOnBounce', 'sendFSMEvent')):
        raise ValueError('fragment bounce drives sound, animation or an FSM event')
    spin = components[spins[0]][1] if spins else None
    if spins and spins[0] == 'SpinSelfSimple' and (spin['randomStartRotation'] or spin['waitForCall']):
        raise ValueError('SpinSelfSimple fragment waits for a call or randomizes its start')
    return {'game_object': f'{Path(sc.file.name).name}:{part_gid}',
            'renderer': components['SpriteRenderer'][0], 'collider_type': colliders[0],
            'spin': spins[0] if spins else None,
            'spin_factor': spin['spinFactor'] if spin else 0.0,
            'bounce_factor': bounce['bounceFactor'], 'body': list(fields)}


def _literal_action(data, name, kind, size):
    index = data['paramName'].index(name)
    if data['paramDataType'][index] != kind or data['paramByteDataSize'][index] != size:
        raise ValueError(f'unsupported serialized action parameter {name}')
    start = data['paramDataPos'][index]
    raw = bytes(data['byteData'])
    if start < 0 or start+size > len(raw):
        raise ValueError('action parameter byte range')
    value = raw[start:start+size]
    # FsmUtility.ByteArrayToFsmFloat/Bool use value bytes followed by a
    # UseVariable byte; variable names would follow and enlarge the record.
    if kind in (15,17) and value[-1] != 0:
        raise ValueError(f'variable action parameter {name}')
    if kind == 15:
        number = struct.unpack('<f', value[:4])[0]
        if not math.isfinite(number):raise ValueError('non-finite action float')
        return number
    if kind == 17:
        if value[0] not in (0,1):raise ValueError('non-boolean action literal')
        return bool(value[0])
    return struct.unpack('<i', value)[0]


def mask_fades(sc, receiver_gid):
    """Strictly recognize an authored HIT -> iTweenFadeTo mask controller.

    This is a compiled behavior subset, not a general PlayMaker interpreter.
    Unrecognized handlers remain explicit and do not block core Breakable.Hit.
    """
    file = Path(sc.file.name).name
    fades, errors = [], []
    for index, kind, tree in _components(sc, receiver_gid):
        if kind != 'PlayMakerFSM':continue
        try:
            fsm = tree['fsm']
            initial = next(st for st in fsm['states'] if st['name'] == fsm['startState'])
            transition = next(t for t in initial['transitions'] if t['fsmEvent']['name'] == 'HIT')
            state = next(st for st in fsm['states'] if st['name'] == transition['toState'])
            data = state['actionData']
            if data['actionNames'] != ['HutongGames.PlayMaker.Actions.iTweenFadeTo'] or data['actionEnabled'] != [1]:
                raise ValueError('HIT destination is not one enabled iTweenFadeTo action')
            owner = data['fsmOwnerDefaultParams']
            if len(owner)!=1 or owner[0]['ownerOption']!=0:
                raise ValueError('iTweenFadeTo target is not owner')
            alpha = _literal_action(data,'alpha',15,5)
            duration = _literal_action(data,'time',15,5)
            delay = _literal_action(data,'delay',15,5)
            children = _literal_action(data,'includeChildren',17,2)
            ease = _literal_action(data,'easeType',7,4)
            loop = _literal_action(data,'loopType',7,4)
            realtime = _literal_action(data,'realTime',17,2)
            if not 0<=alpha<=1 or not 0<duration<=10 or delay!=0 or ease!=21 or loop!=0 or realtime:
                raise ValueError('unsupported iTweenFadeTo timing/ease/loop')
            # Both installed EaseType enums declare linear=21 in metadata.
            gids = _descendants(sc,receiver_gid) if children else {receiver_gid}
            renderers=[]
            for gid in gids:
                for renderer_id, renderer_type, renderer in _components(sc,gid):
                    if renderer_type=='SpriteRenderer' and renderer['m_Enabled']:
                        renderers.append({'source':f'{file}:{renderer_id}','id':renderer_id,
                                          'initial_alpha':renderer['m_Color']['a']})
            if not renderers:raise ValueError('mask controller has no sprite renderers')
            fades.append({'source_fsm':f'{file}:{index}','fsm_name':fsm['name'],
                'from_state':initial['name'],'to_state':state['name'],'event':'HIT',
                'renderers':renderers,'renderer_ids':[r['id']for r in renderers],
                'renderer_sources':[r['source']for r in renderers],
                'target_alpha':alpha,'seconds':duration,'ticks_60hz':math.ceil(duration*60-1e-5),
                'include_children':children,'ease':'linear','ease_enum':ease})
        except Exception as error:
            errors.append({'source':f'{file}:{index}','type':'forwarded Breakable event handler','error':str(error)})
    return fades, errors


# Authored outputs whose absence refuses the instance outright.
#
# Enforcing an output means an instance that authors it and cannot get it stops
# being a breakable: it keeps its collider and its whole art and reads as
# ordinary scenery, which is honest, where a silent break reads as a bug.
#
# Two authored outputs are measured per instance and reported but not enforced,
# because enforcing either is a budget decision rather than a cook decision:
#
#   OUTPUT_AUDIO      164 of 338 catalogue instances name one of four clips that
#                     are not resident. Their serialized lengths need 50,864 SPU
#                     bytes against the 14,432 free below the 0x14000 bank limit
#                     in host/hk-cook/src/cook_audio.rs, so no cook can satisfy them.
#   OUTPUT_FRAGMENTS  193 instances fling 355 rigid parts; only Tutorial_01's 28
#                     are cooked. The definition below recognizes all 355, but
#                     every Spec links, so admitting them is a measured cost
#                     against the linked headroom rather than a free win.
ENFORCED_OUTPUTS = (OUTPUT_VISUAL, OUTPUT_PARTICLES, OUTPUT_MASK)


def destruction_contract(sc, record, gravity=None):
    """The outputs this instance authors, and which of them the port produces.

    Reads the instance's own parts, so the same definition answers a Tutorial
    door and a Crossroads barrel. `gravity` is only needed to evaluate particle
    emitters; omit it to report the authored shape without the port's verdict.
    """
    from break_effects import part_emitter, scene_gravity
    if gravity is None:
        gravity = scene_gravity(sc.source)
    authored, missing = [], []
    if record['off_renderer_ids'] or record['on_renderer_ids']:
        authored.append(OUTPUT_VISUAL)
    authored.append(OUTPUT_COLLIDER)
    clips = [option['name'] for option in record['audio']['options']]
    if clips:
        authored.append(OUTPUT_AUDIO)
        absent = sorted({name for name in clips if name not in RESIDENT_BREAK_CLIPS})
        if absent:
            missing.append({'output': OUTPUT_AUDIO, 'reason': 'break clip not resident: ' + ', '.join(absent)})
    emitters, fragments = [], []
    for part in record['debris']:
        part_gid = int(part['game_object'].split(':')[1])
        try:
            found = part_emitter(sc.source, sc, part_gid, gravity)
        except ValueError as error:
            authored.append(OUTPUT_PARTICLES)
            missing.append({'output': OUTPUT_PARTICLES, 'part': part['game_object'], 'reason': str(error)})
            continue
        if found is not None:
            authored.append(OUTPUT_PARTICLES)
            emitters.append(part['game_object'])
            continue
        try:
            rigid = rigid_fragment(sc, part_gid)
        except ValueError as error:
            authored.append(OUTPUT_FRAGMENTS)
            missing.append({'output': OUTPUT_FRAGMENTS, 'part': part['game_object'], 'reason': str(error)})
            continue
        if rigid is not None:
            authored.append(OUTPUT_FRAGMENTS)
            fragments.append(rigid)
        else:
            missing.append({'output': OUTPUT_PARTICLES, 'part': part['game_object'],
                            'reason': 'debris part is neither a particle system nor a rigid fling'})
            authored.append(OUTPUT_PARTICLES)
    if record['forwarded_events']['hit_receiver']:
        authored.append(OUTPUT_MASK)
        if not record['mask_fades']:
            reasons = '; '.join(error['error'] for error in record['event_errors']) or 'no recognized handler'
            missing.append({'output': OUTPUT_MASK, 'reason': reasons})
    authored = [name for name in OUTPUTS if name in authored]
    enforced = sorted({item['output'] for item in missing} & set(ENFORCED_OUTPUTS))
    return {'authored': authored, 'missing': missing, 'refused_outputs': enforced,
            'particle_parts': emitters, 'rigid_fragments': fragments}


# --------------------------------------------------------------- arena gates
#
# An arena gate is the one family in this package whose serialized state and its
# state on the first drawn frame disagree, which is why it is worth a recognizer
# of its own. host/world_geometry.py makes terrain out of every enabled,
# non-trigger, layer-8 collider it finds, reading the serialized values; a gate
# is serialized closed whatever its placement says, so an open gate arrives here
# as solid terrain. None of the 16 gate tk2dSprites is cooked into a draw, so
# the result is an invisible wall rather than a visible one.
BG_CONTROL_FSM = 'BG Control'
# The same definition, and the same digest, host/false_knight.py pinned for the
# False Knight arena. Excluding the placement variable, every active gate in the
# catalogue hashes to it, so one pin covers the family rather than one per room.
BG_CONTROL_SHA256 = '98a0b55f39c2f01fa2e16c353d581532179c4893a03437b10cbee49b19e8ca9f'
BG_CONTROL_PLACEMENT_VARIABLE = 'Start Closed'
BG_CONTROL_START_STATE = 'Opened'
BG_CONTROL_START_ACTIONS = ('GetOwner', 'BoolTest', 'Tk2dPlayAnimation', 'SetCollider')
# `Opened` sends this to itself when the placement variable is set. PlayMaker
# abandons the rest of a state once a transition is taken, so the `SetCollider`
# that would open the gate never runs and `Quick Close` turns the box on instead.
BG_CONTROL_CLOSE_EVENT = 'BG QUICK CLOSE'
BG_CONTROL_CLOSE_STATE = 'Quick Close'
# A closed gate reopens only on this, broadcast by a `Battle Scene` when its
# arena is won. Nothing in this port broadcasts it.
BG_CONTROL_OPEN_EVENT = 'BG OPEN'
TERRAIN_LAYER = 8


def _state_actions(fsm, name):
    """(action, fields) for one state's enabled actions, in authored order."""
    from focus import action_fields
    states = [state for state in fsm['states'] if state['name'] == name]
    if len(states) != 1:
        raise ValueError(f'expected exactly one {name!r} state, found {len(states)}')
    data = states[0]['actionData']
    return [(raw.rsplit('.', 1)[-1], action_fields(data, index))
            for index, raw in enumerate(data['actionNames']) if data['actionEnabled'][index]]


def _fsm_variables(fsm):
    """Serialized FSM variables by name, object references left out."""
    return {v['name']: v['value'] for group in fsm['variables'].values() if isinstance(group, list)
            for v in group if isinstance(v, dict) and 'name' in v and 'value' in v
            and not (isinstance(v['value'], dict) and 'm_PathID' in v['value'])}


def _sets_own_collider(action, fields, expected):
    """A SetCollider acting on Owner with a literal value, or a refusal."""
    if action != 'SetCollider':
        raise ValueError(f'expected SetCollider, found {action}')
    target = fields.get('gameObject')
    if not isinstance(target, dict) or target.get('ownerOption') != 0:
        raise ValueError('SetCollider does not act on the gate itself')
    active = fields.get('active')
    if not isinstance(active, dict) or active.get('useVariable') or bool(active.get('value')) is not expected:
        raise ValueError(f'SetCollider no longer sets the gate collider to {expected}')


def battle_gate(sc, gid, fsm):
    """The one decision a `BG Control` gate makes on load, or a reason to refuse.

    Reproduces `Opened`, the start state: `GetOwner`, a `BoolTest` of the
    placement variable that sends `BG QUICK CLOSE` when it is set and names no
    event when it is clear, the opened animation, and `SetCollider active=false`
    on the gate itself. Every other authored path waits on an arena event, so in
    this port a gate's whole observable behaviour is the boolean below.
    """
    if fsm['startState'] != BG_CONTROL_START_STATE:
        raise ValueError(f"BG Control starts in {fsm['startState']!r}")
    actions = _state_actions(fsm, BG_CONTROL_START_STATE)
    if tuple(name for name, _ in actions) != BG_CONTROL_START_ACTIONS:
        raise ValueError('BG Control start state is not the authored action sequence')
    test = actions[1][1]
    variable = test.get('boolVariable')
    if not isinstance(variable, dict) or not variable.get('useVariable') \
            or variable.get('name') != BG_CONTROL_PLACEMENT_VARIABLE:
        raise ValueError('BG Control start test does not read the placement variable')
    if test.get('isTrue') != BG_CONTROL_CLOSE_EVENT or test.get('isFalse') or test.get('everyFrame'):
        raise ValueError('BG Control start test no longer only closes the gate')
    _sets_own_collider(*actions[3], expected=False)
    # The other half of the pair has to say so too: a gate that starts closed is
    # only solid because `Quick Close` turns its box back on.
    closing = _state_actions(fsm, BG_CONTROL_CLOSE_STATE)
    if not closing:
        raise ValueError('BG Control quick close does nothing')
    _sets_own_collider(*closing[0], expected=True)
    if not any(event['fsmEvent']['name'] == BG_CONTROL_OPEN_EVENT
               for state in fsm['states'] if state['name'] == BG_CONTROL_CLOSE_STATE
               for event in state['transitions']):
        raise ValueError('BG Control quick close no longer waits for the arena to open it')
    start_closed = _fsm_variables(fsm).get(BG_CONTROL_PLACEMENT_VARIABLE)
    if start_closed not in (0, 1, False, True):
        raise ValueError(f'BG Control {BG_CONTROL_PLACEMENT_VARIABLE!r} is not a serialized boolean')

    colliders = [(i, t, c) for i, t, c in _components(sc, gid) if t.endswith('Collider2D')]
    if len(colliders) != 1:
        raise ValueError(f'arena gate carries {len(colliders)} colliders, not one')
    body_id, collider_type, body = colliders[0]
    if collider_type != 'BoxCollider2D':
        raise ValueError(f'arena gate collider is a {collider_type}')
    if not body['m_Enabled'] or body['m_IsTrigger']:
        raise ValueError('arena gate collider is not a serialized solid')
    if sc.gos[gid]['m_Layer'] != TERRAIN_LAYER:
        raise ValueError(f"arena gate is on layer {sc.gos[gid]['m_Layer']}, not terrain")
    polygons = collider_polygons(sc, gid, collider_type, body)
    points = [p for polygon in polygons for p in polygon]
    file = Path(sc.file.name).name
    return {
        'gid': gid, 'position': sc.point(gid), 'start_closed': bool(start_closed),
        # The verdict the cook needs, and the join key it needs it under:
        # world_geometry keys every terrain edge by its collider's source id.
        'solid_on_load': bool(start_closed), 'collider_source': sc.sid(body_id),
        'box': [min(p[0] for p in points), min(p[1] for p in points),
                max(p[0] for p in points), max(p[1] for p in points)],
        'opens_on': BG_CONTROL_OPEN_EVENT,
        # A closed gate has no way out of `Quick Close` here: no Battle Scene
        # runs, so nothing sends BG OPEN. That is the fresh-save state the
        # original also presents, but it never lifts.
        'opens_in_port': False,
        'limitations': ['Gate animation, dust and slam audio are not reproduced; only the '
                        'collider state the room loads with is answered',
                        'A gate that starts closed stays closed: no Battle Scene arena runs'],
    }


def battle_gates(sc, errors=None):
    """Every active arena gate in the scene, each with its load-time collider.

    The selector is the FSM definition name, never the object name: the same
    `Battle Gate` name appears on an object driven by a different FSM, and the
    same definition appears under other names. Anything carrying the definition
    that does not decode is appended to `errors` rather than guessed at, because
    a wrong answer here either walls a room off or opens one that should be shut.
    """
    from false_knight import fsm_digest
    file = Path(sc.file.name).name
    result = []
    for index, (typ, tree) in sorted(sc.objects.items()):
        if typ != 'PlayMakerFSM' or tree['fsm']['name'] != BG_CONTROL_FSM:
            continue
        gid = _local_id(tree['m_GameObject'])
        if gid not in sc.gos or not sc.active(gid):
            continue
        try:
            digest = fsm_digest(tree['fsm'], (BG_CONTROL_PLACEMENT_VARIABLE,))
            if digest != BG_CONTROL_SHA256:
                raise ValueError(f'unverified BG Control variant: {digest}')
            record = battle_gate(sc, gid, tree['fsm'])
            record.update(source=f'{file}:{index}', game_object=f'{file}:{gid}',
                          name=sc.gos[gid]['m_Name'], fsm_sha256=digest)
            result.append(record)
        except Exception as error:
            if errors is None:
                raise
            errors.append({'id': f'{file}:{index}', 'type': BG_CONTROL_FSM, 'error': str(error)})
    return result


# ------------------------------------------------------- FSM-authored families
#
# Everything below reads a family whose behaviour lives in a PlayMaker FSM
# rather than a C# component. Two rules apply to all of them.
#
# Selection is by FSM *definition*, never by object name. `Breakable Wall`,
# `Breakable Wall_Silhouette`, `Breakable Wall Waterways` and `Break Wall 2` are
# four object names for two authored hidden-wall definitions, and the object
# name also picks up copies the editor suffixed. Definition names are normalised
# first, for the same reason: an editor copy can carry one of those suffixes too.
#
# A definition name is a starting point and not a proof. `Break Wall 2` ships
# under the definition name `FSM`, which names dozens of unrelated behaviours,
# so a candidate is confirmed by its state-name signature and then pinned with
# `false_knight.fsm_digest`, exactly as the secret masks are. Placement
# variables a family legitimately varies are excluded from the digest and
# validated per instance instead, because the digest cannot see them.

_COPY_SUFFIX = re.compile(r'\s*\(\d+\)\s*$')


def definition_name(fsm):
    """One FSM definition's name with an editor copy suffix removed."""
    return _COPY_SUFFIX.sub('', (fsm.get('name') or '').strip())


def _state_names(fsm):
    return frozenset(state['name'] for state in fsm['states'])


def _string_variable(fsm, name):
    value = _fsm_variables(fsm).get(name, '')
    if not isinstance(value, str):
        raise ValueError(f'{name!r} is not a serialized string')
    return value


def _named_object(sc, name):
    """The one active scene object with this name, for a FindGameObject target.

    PlayMaker's `FindGameObject` resolves a name against the whole scene at
    runtime and takes whichever object it finds first. The port has no runtime
    scene graph to ask, so a name that does not resolve to exactly one active
    object is refused rather than guessed: picking the wrong one here wires a
    wall to somebody else's art.
    """
    found = [gid for gid, go in sc.gos.items() if go['m_Name'] == name and sc.active(gid)]
    if len(found) != 1:
        raise ValueError(f'{name!r} names {len(found)} active scene objects, not one')
    return found[0]


def _send_events(fsm, state_name):
    """(event, target, object variable, sendToChildren) per enabled send."""
    from focus import action_fields
    out = []
    for state in fsm['states']:
        if state['name'] != state_name:
            continue
        data = state['actionData']
        for index, raw in enumerate(data['actionNames']):
            if not data['actionEnabled'][index] or not raw.endswith('.SendEventByName'):
                continue
            fields = action_fields(data, index)
            target = fields['eventTarget']
            owner = target['gameObject']
            out.append({'event': fields['sendEvent']['value'],
                        'target': target['target'],
                        'variable': owner['gameObject'].get('name') if owner.get('ownerOption') else None,
                        'owner': not owner.get('ownerOption'),
                        'to_children': bool(target['sendToChildren']['value'])})
    return out


# ---------------------------------------------------------------- hidden walls
#
# A hidden wall is solid drawn terrain that takes four nail hits, or one spell,
# and then takes itself and the black mask in front of the secret away. Measured
# over the 46 admitted scenes: 7 instances of one authored shape under two
# definition names, 6 of them active.
#
#   breakable_wall_v2   4 instances, 4 scenes, 1 structural shape, 2 digests
#                       before `Facing` is excluded and 1 after.
#   FSM (`Break Wall 2`)  3 instances, 3 scenes, 1 digest once the three
#                       placement variables are excluded. Crossroads_27's is
#                       serialized inactive and its `Mask Name` resolves to
#                       nothing in that scene, so 2 are live.
#
# The two shapes differ in how the break reaches the mask, and that difference
# is the whole reason this family and the secret masks are one piece of work:
#
#   breakable_wall_v2  `Break` and `Activated` send UNCOVER to the wall's own
#                      GameObject with `sendToChildren` set, so it reaches the
#                      `Masks` child and everything under it.
#   Break Wall 2       `Initiate` resolves `Mask Name` with `FindGameObject` and
#                      `Break`/`Activated` send UNCOVER straight to that object.
#
# Either way the mask's own `Trigger2dEvent` is not what fires it. Five of the
# twelve reveal-mask refusals in host/reveal_masks.py read "no trigger collider
# on the owner"; those owners are not badly authored, they are waiting on a
# wall. `uncover_drivers` below is what lets that refusal say so.
HIDDEN_WALL_NAIL_HITS = 4
# `Check If Nail` switches on the damager's `attackType`: 0 is the nail and
# takes one off `Hits`, 2 is a spell and goes straight to `Break`.
HIDDEN_WALL_SPELL_ATTACK_TYPE = 2
HIDDEN_WALL_UNCOVER = 'UNCOVER'
# Serialized per placement, so excluded from the digest and checked per
# instance. `Facing` picks which recoil state runs and does not gate the hit
# count; the two name variables are Break Wall 2's scene lookups.
HIDDEN_WALL_PLACEMENT_VARIABLES = ('Facing', 'Mask Name', 'CamLock Name')
HIDDEN_WALL_FACINGS = (0, 1, 2, 3)
HIDDEN_WALL_SHAPES = {
    '5ed4602f92093bd200971a86dec0e40c9af85ca39e4bf40924b864b5ebfb47ea': {
        'definition': 'breakable_wall_v2', 'start': 'Get Refs', 'uncover': 'children',
        'renderer': 'SpriteRenderer',
        'states': frozenset({
            'Idle', 'Check If Nail', 'Hit', 'Initiate', 'Check Direction', 'Break',
            'Hit Right', 'Return Right', 'Hit Left', 'Return Left', 'Hit Down',
            'Return Down', 'Hit Up', 'Pause Frame', 'Destroy', 'Pause', 'Activated',
            'Activated?', 'Ruin Lift?', 'Deactivate', 'Get Refs', 'PD Bool?',
            'Spell Destroy'}),
        # `Activated?` reaches `Ruin Lift?` only because `StringCompare` finds
        # `PlayerData Bool` empty and fires FINISHED before `PlayerDataBoolTest`
        # runs, and `Ruin Lift?` reaches `Initiate` only because `Ruin Lift` is
        # clear. A placement that sets either needs a save this port has not got.
        'clear_booleans': ('Ruin Lift',), 'empty_strings': ('PlayerData Bool',)},
    'e316a8d60e3e4f8269f5d3ac97cfbda4e2cb7715495a24778123019e3d534283': {
        'definition': 'FSM', 'start': 'Pause', 'uncover': 'name',
        'renderer': 'tk2dSprite',
        'states': frozenset({
            'Idle', 'Check If Nail', 'Hit', 'Initiate', 'Check Direction', 'Break',
            'Hit Right', 'Return Right', 'Hit Left', 'Return Left', 'Hit Down',
            'Return Down', 'Hit Up', 'Pause Frame', 'Damage', 'Destroy', 'Pause',
            'Activated', 'Spell Destroy'}),
        # No PlayerData branch on this shape; the camera lock it would look up is
        # unnamed on every catalogue instance and is refused below if it is not.
        'clear_booleans': (), 'empty_strings': ('CamLock Name',)},
}
# The break spawns this prefab. It carries a ParticleSystem, so it is authored
# particle output, and the port has no path that instantiates a prefab at all.
HIDDEN_WALL_DUST_PREFAB = 'Dust Break Wall'


def hidden_wall_shape(fsm):
    """The pinned hidden-wall shape this FSM is, or None.

    The state-name signature is the cheap screen; the digest is the proof. Both
    are needed: the signature alone would admit an edited variant and the digest
    alone would hash every FSM in the scene.
    """
    from false_knight import fsm_digest
    names = _state_names(fsm)
    if not any(names == shape['states'] for shape in HIDDEN_WALL_SHAPES.values()):
        return None
    digest = fsm_digest(fsm, HIDDEN_WALL_PLACEMENT_VARIABLES)
    shape = HIDDEN_WALL_SHAPES.get(digest)
    if shape is None:
        raise ValueError(f'unverified hidden wall variant: {digest}')
    if definition_name(fsm) != shape['definition']:
        raise ValueError(f'hidden wall digest {digest} under definition {definition_name(fsm)!r}')
    if fsm['startState'] != shape['start']:
        raise ValueError(f"hidden wall starts in {fsm['startState']!r}")
    return dict(shape, sha256=digest)


def uncover_drivers(sc):
    """Scene objects a hidden wall uncovers, by the object that receives UNCOVER.

    Maps the receiving GameObject id to the wall record fragment, so a reveal
    controller that carries no trigger collider of its own can say which wall
    fires it instead of reading as broken authoring. Only structure is read
    here; whether either side is admitted is decided by its own recognizer.
    """
    if hasattr(sc, '_uncover_drivers'):
        return sc._uncover_drivers
    drivers = {}
    for index, (typ, tree) in sorted(sc.objects.items()):
        if typ != 'PlayMakerFSM' or not tree['m_Enabled']:
            continue
        gid = _local_id(tree['m_GameObject'])
        if gid not in sc.gos or not sc.active(gid):
            continue
        try:
            shape = hidden_wall_shape(tree['fsm'])
        except ValueError:
            continue
        if shape is None:
            continue
        try:
            targets = _uncover_targets(sc, gid, tree['fsm'], shape)
        except ValueError:
            continue
        driver = {'source': sc.sid(index), 'game_object': sc.sid(gid),
                  'name': sc.gos[gid]['m_Name'], 'definition': shape['definition'],
                  'fsm_sha256': shape['sha256'], 'uncover': shape['uncover']}
        for target in targets:
            drivers.setdefault(target, driver)
    sc._uncover_drivers = drivers
    return drivers


def _uncover_targets(sc, gid, fsm, shape):
    """GameObject ids that receive the wall's UNCOVER, in source terms.

    `sendToChildren` is a field of the serialized `FsmEventTarget`, so whether
    the break reaches the mask is read rather than assumed: the subtree shape
    sets it and the named shape does not.
    """
    sends = [send for state in ('Break', 'Activated') for send in _send_events(fsm, state)
             if send['event'] == HIDDEN_WALL_UNCOVER]
    if not sends:
        raise ValueError('hidden wall no longer broadcasts UNCOVER')
    if shape['uncover'] == 'children':
        if not all(send['to_children'] for send in sends):
            raise ValueError('hidden wall UNCOVER no longer reaches its children')
        # Every active descendant, because the authored receivers sit two levels
        # down under `Masks` and PlayMaker's broadcast is recursive. Inactive
        # ones are left out: the broadcast collects components from the active
        # hierarchy, and an FSM that is not running cannot answer an event.
        # Crossroads_18 ships two of its four mask objects inactive.
        return sorted(g for g in _descendants(sc, gid) - {gid} if sc.active(g))
    if any(send['to_children'] for send in sends):
        raise ValueError('named-target hidden wall UNCOVER also broadcasts to children')
    name = _string_variable(fsm, 'Mask Name')
    if not name:
        raise ValueError('hidden wall names no mask to uncover')
    return [_named_object(sc, name)]


def _particle_outputs(sc, gids, gravity, authored, missing, played=()):
    """Record every emitter in `gids` the bounded particle model cannot run.

    `played` is the emitters this instance has resolved an action for, which is
    what lets a one-shot system with `playOnAwake` off be read as the emitter it
    is rather than as one nothing starts.
    """
    from break_effects import part_emitter
    for gid in gids:
        try:
            found = part_emitter(sc.source, sc, gid, gravity, played=gid in played)
        except ValueError as error:
            authored.append(OUTPUT_PARTICLES)
            missing.append({'output': OUTPUT_PARTICLES, 'part': sc.sid(gid), 'reason': str(error)})
            continue
        if found is not None:
            authored.append(OUTPUT_PARTICLES)


def _child_bindings(sc, gid, fsm):
    """Object variable to scene object, for every literal FindChild on the owner.

    `Transform.Find` takes a direct child by name, so the binding is decidable
    from the serialized hierarchy alone. A lookup aimed at anything but the
    FSM's own object, or one whose name or result is itself a variable, binds
    nothing rather than binding a guess.
    """
    from focus import action_fields
    owner = _owner_variable(fsm)
    children = {}
    for child in sc.transforms[sc.go_transform[gid]]['m_Children']:
        found = sc.transforms[child['m_PathID']]['m_GameObject']['m_PathID']
        children.setdefault(sc.gos[found]['m_Name'], []).append(found)
    bound = {}
    for state in fsm['states']:
        data = state['actionData']
        for index, raw in enumerate(data['actionNames']):
            if not data['actionEnabled'][index] or not raw.endswith('.FindChild'):
                continue
            fields = action_fields(data, index)
            target = fields['gameObject']
            if target['ownerOption'] and target['gameObject'].get('name') != owner:
                continue
            if fields['childName']['useVariable']:
                continue
            slot = _action_slots(data, index).get('storeResult')
            if slot is None or data['paramDataType'][slot] != 19:
                continue
            stored = data['fsmGameObjectParams'][data['paramDataPos'][slot]]
            if not stored.get('useVariable') or not stored.get('name'):
                continue
            found = children.get(fields['childName']['value'], [])
            bound[stored['name']] = found[0] if len(found) == 1 else None
    return bound


def _played_emitters(sc, gid, fsm):
    """Scene objects an enabled PlayParticleEmitter in this FSM starts.

    The action names an FSM object variable rather than an object, so the name
    is traced back to the literal `FindChild` that binds it. A variable nothing
    binds resolves to nothing and its emitter stays refused, which is the point:
    the flag this feeds says an action was resolved, not that one was assumed.
    """
    from focus import action_fields
    bound = _child_bindings(sc, gid, fsm)
    played = set()
    for state in fsm['states']:
        data = state['actionData']
        for index, raw in enumerate(data['actionNames']):
            if not data['actionEnabled'][index] or not raw.endswith('.PlayParticleEmitter'):
                continue
            target = action_fields(data, index)['gameObject']
            if not target['ownerOption']:
                played.add(gid)
                continue
            found = bound.get(target['gameObject'].get('name'))
            if found is not None:
                played.add(found)
    return played


def _prefab_particles(source, file, ref):
    """(name, carries a ParticleSystem) for a CreateObject prefab reference."""
    if not ref['m_PathID']:
        return None
    prefab = source.ref(file, ref)
    tree = source.read(prefab)
    if 'm_Component' not in tree:
        raise ValueError('CreateObject target is not a GameObject')
    parts = []
    for component in tree['m_Component']:
        try:
            parts.append(source.ref(prefab.assets_file, component['component']).type.name)
        except Exception:
            parts.append('?')
    return {'source': source.sid(prefab), 'name': tree['m_Name'],
            'particles': 'ParticleSystem' in parts}


def _action_parameters(fsm, state_name, action, parameter, kind, table):
    """One named typed parameter of every enabled `action` in one state.

    PlayMaker serializes a state's parameters as one flat run split by
    `actionStartIndex`, and the typed tables are indexed by `paramDataPos`
    rather than by parameter order, so reading a parameter means walking that
    action's slice. `focus.action_fields` decodes the compact scalars and skips
    the reference kinds this needs, which is why the slice is walked here.
    """
    out = []
    for state in fsm['states']:
        if state['name'] != state_name:
            continue
        data = state['actionData']
        for index, raw in enumerate(data['actionNames']):
            if not data['actionEnabled'][index] or not raw.endswith('.' + action):
                continue
            start = data['actionStartIndex'][index]
            end = (data['actionStartIndex'][index + 1] if index + 1 < len(data['actionNames'])
                   else len(data['paramName']))
            out += [data[table][data['paramDataPos'][k]] for k in range(start, end)
                    if data['paramName'][k] == parameter and data['paramDataType'][k] == kind]
    return out


# FsmVector3 as PlayMaker byte-serializes it: three floats then the UseVariable
# flag. The record is exactly 13 bytes, so no variable name can follow it, which
# is what makes a set flag readable as IsNone rather than as a named variable.
FSM_VECTOR3 = 28
FSM_VECTOR3_BYTES = 13


def _action_slots(data, index):
    """Parameter name to flat-run index for one action's own slice."""
    start = data['actionStartIndex'][index]
    end = (data['actionStartIndex'][index + 1] if index + 1 < len(data['actionNames'])
           else len(data['paramName']))
    return {data['paramName'][k]: k for k in range(start, end)}


def _vector_parameter(data, slot):
    """One serialized FsmVector3, or None when the action leaves it unset."""
    if data['paramDataType'][slot] != FSM_VECTOR3 or data['paramByteDataSize'][slot] != FSM_VECTOR3_BYTES:
        raise ValueError('unsupported serialized vector parameter')
    raw = bytes(data['byteData'])
    start = data['paramDataPos'][slot]
    if start < 0 or start + FSM_VECTOR3_BYTES > len(raw):
        raise ValueError('action parameter byte range')
    if raw[start + 12]:
        return None
    value = struct.unpack('<3f', raw[start:start + 12])
    if not all(math.isfinite(v) for v in value):
        raise ValueError('non-finite action vector')
    return value


def _owner_variable(fsm):
    """The object variable GetOwner stores, which is the FSM's own GameObject.

    Both hidden-wall shapes and both cracked-floor shapes bind one, and it is
    the variable their CreateObject names as its spawn point. Reading it is what
    turns `Self` into a scene object without trusting the variable's name.
    """
    for state in fsm['states']:
        data = state['actionData']
        for index, raw in enumerate(data['actionNames']):
            if not data['actionEnabled'][index] or not raw.endswith('.GetOwner'):
                continue
            slot = _action_slots(data, index).get('storeGameObject')
            if slot is None or data['paramDataType'][slot] != 19:
                continue
            stored = data['fsmGameObjectParams'][data['paramDataPos'][slot]]
            if stored.get('useVariable') and stored.get('name'):
                return stored['name']
    return None


def _spawn_transform(sc, gid, fsm, data, slots):
    """Where CreateObject puts the object it instantiates, in world terms.

    PlayMaker starts from the spawn point's own transform, adds the action's
    literal `position` as an offset, and takes the literal `rotation` when one
    is authored. Every catalogue instance spawns at its own object, so the only
    spawn point resolved here is the variable `GetOwner` stores; anything else
    is refused rather than placed by guess.
    """
    point = data['fsmGameObjectParams'][data['paramDataPos'][slots['spawnPoint']]]
    offset = _vector_parameter(data, slots['position'])
    rotation = _vector_parameter(data, slots['rotation'])
    if point['value']['m_PathID'] or not point.get('useVariable'):
        raise ValueError('the spawn point is not the FSM object variable this port can resolve')
    owner = _owner_variable(fsm)
    if owner is None or point.get('name') != owner:
        raise ValueError(f"the spawn point {point.get('name')!r} is not the object GetOwner stores")
    origin = sc.point(gid)
    if offset is not None:
        origin = [a + b for a, b in zip(origin, offset)]
    if rotation is None:
        # Unity would take the spawn point's own eulerAngles here. No catalogue
        # instance does, so the decomposition that would need is not written.
        raise ValueError('the spawn takes its rotation from the spawn point, which is not read')
    return list(origin), rotation


def _create_object_prefabs(sc, gid, fsm, state_name):
    """Every enabled CreateObject prefab in one state, with where it lands.

    Raises through `_spawn_transform` when the spawn cannot be resolved, so a
    prefab is never placed by assumption.
    """
    out = []
    for state in fsm['states']:
        if state['name'] != state_name:
            continue
        data = state['actionData']
        for index, raw in enumerate(data['actionNames']):
            if not data['actionEnabled'][index] or not raw.endswith('.CreateObject'):
                continue
            slots = _action_slots(data, index)
            if not {'gameObject', 'spawnPoint', 'position', 'rotation'} <= set(slots):
                raise ValueError('unsupported serialized CreateObject parameters')
            if data['paramDataType'][slots['gameObject']] != 19:
                raise ValueError('unsupported serialized CreateObject target')
            reference = data['fsmGameObjectParams'][data['paramDataPos'][slots['gameObject']]]
            if reference.get('useVariable'):
                continue
            found = _prefab_particles(sc.source, sc.file, reference['value'])
            if found is None or not found['particles']:
                # A prefab with no ParticleSystem is not a particle output, so
                # where it lands never has to be resolved. The break's flung
                # wood and rock pools are this, and asking them for a spawn this
                # port can place would refuse the dust beside them.
                continue
            found['reference'] = reference['value']
            found['origin'], found['rotation'] = _spawn_transform(sc, gid, fsm, data, slots)
            out.append(found)
    return out


def _prefab_particle_outputs(sc, gid, fsm, states, gravity, authored, missing):
    """Record every emitter the break's CreateObject prefabs bring with them.

    The refusal this replaces read "no cooked path spawns a prefab", and that
    stopped being the question once the spawn stopped needing a runtime: the
    prefab reference and its place are both literals in the action, so
    break_effects resolves the whole thing at cook time and runs each emitter
    through the same validator an authored child goes through. What is left is
    the honest one, which is whether those emitters are reproducible at all.
    """
    from break_effects import prefab_emitters
    for state in states:
        try:
            prefabs = _create_object_prefabs(sc, gid, fsm, state)
        except ValueError as error:
            authored.append(OUTPUT_PARTICLES)
            missing.append({'output': OUTPUT_PARTICLES, 'reason': f'the break spawns a prefab and {error}'})
            continue
        for prefab in prefabs:
            authored.append(OUTPUT_PARTICLES)
            try:
                prefab_emitters(sc.source, sc.file, prefab['reference'], gravity,
                                prefab['origin'], prefab['rotation'])
            except ValueError as error:
                missing.append({'output': OUTPUT_PARTICLES, 'part': prefab['source'],
                                'reason': f"the break instantiates the {prefab['name']!r} particle "
                                          f'prefab, and {error}'})


def _break_clips(sc, fsm, state_name):
    """AudioPlayerOneShotSingle clip names played by one state."""
    names = []
    for clip in _action_parameters(fsm, state_name, 'AudioPlayerOneShotSingle',
                                   'audioClip', 24, 'fsmObjectParams'):
        reference = clip['value']
        if reference['m_PathID']:
            names.append(sc.source.read(sc.source.ref(sc.file, reference))['m_Name'])
    return names


def _audio_output(clips, authored, missing):
    """Break audio, measured against the resident bank and never enforced.

    Same rule the Breakable catalogue already runs under: 164 of its instances
    name a clip the 0x14000 SPU bank has no room for, so no cook can satisfy
    them and enforcing it would only delete objects that otherwise work.
    """
    if not clips:
        return
    authored.append(OUTPUT_AUDIO)
    absent = sorted({name for name in clips if name not in RESIDENT_BREAK_CLIPS})
    if absent:
        missing.append({'output': OUTPUT_AUDIO, 'reason': 'break clip not resident: ' + ', '.join(absent)})


def hidden_wall(sc, gid, index, fsm, shape, gravity, drawn_renderers=None):
    """One hidden wall's authored break, and which of it the port can produce."""
    variables = _fsm_variables(fsm)
    for name in shape['clear_booleans']:
        if variables.get(name) not in (0, False):
            raise ValueError(f'{name!r} is set, so the load path is not the plain one')
    for name in shape['empty_strings']:
        if _string_variable(fsm, name):
            raise ValueError(f'{name!r} names a save-backed lookup this port cannot answer')
    # Nothing answers `Activated` here: no save and no scene state, so the cook
    # reproduces the fresh-save answer the serialized value gives.
    if variables.get('Activated') not in (0, False):
        raise ValueError('hidden wall is serialized already broken')
    if variables.get('Facing') not in HIDDEN_WALL_FACINGS:
        raise ValueError(f"hidden wall Facing {variables.get('Facing')!r} is outside the authored switch")
    hits = variables.get('Hits')
    if not isinstance(hits, int) or isinstance(hits, bool) or not 1 <= hits <= MAX_HIT_POINTS:
        raise ValueError(f'hidden wall hit count {hits!r} is not a serialized 1..16 integer')

    colliders = [(i, t, c) for i, t, c in _components(sc, gid) if t.endswith('Collider2D')]
    if len(colliders) != 1:
        raise ValueError(f'hidden wall carries {len(colliders)} colliders, not one')
    body_id, collider_type, body = colliders[0]
    if collider_type != 'BoxCollider2D':
        raise ValueError(f'hidden wall collider is a {collider_type}')
    if not body['m_Enabled'] or body['m_IsTrigger']:
        raise ValueError('hidden wall collider is not a serialized solid')
    if sc.gos[gid]['m_Layer'] != TERRAIN_LAYER:
        raise ValueError(f"hidden wall is on layer {sc.gos[gid]['m_Layer']}, not terrain")
    polygons = collider_polygons(sc, gid, collider_type, body)
    points = [p for polygon in polygons for p in polygon]

    components = {typ: i for i, typ, _ in _components(sc, gid)}
    authored, missing = [OUTPUT_COLLIDER, OUTPUT_VISUAL], []
    renderer = components.get(shape['renderer'])
    if renderer is None:
        raise ValueError(f"hidden wall has no {shape['renderer']} to hide")
    renderer_source = sc.sid(renderer)
    if shape['renderer'] != 'SpriteRenderer':
        # The same reason the arena gates' 16 tk2dSprites are invisible walls:
        # no tk2d sprite in the catalogue is cooked into a draw, so hiding this
        # wall would remove a collider and leave the art standing.
        missing.append({'output': OUTPUT_VISUAL,
                        'reason': f"the wall is drawn by a {shape['renderer']}, which no cooked draw carries"})
    elif drawn_renderers is not None and renderer_source not in drawn_renderers:
        missing.append({'output': OUTPUT_VISUAL, 'reason': 'the wall renderer is not cooked into any draw'})

    _audio_output(_break_clips(sc, fsm, 'Break'), authored, missing)
    emitters = [g for g in sorted(_descendants(sc, gid) - {gid})
                if 'ParticleSystem' in {t for _, t, _ in _components(sc, g)}]
    _particle_outputs(sc, emitters, gravity, authored, missing, _played_emitters(sc, gid, fsm))
    _prefab_particle_outputs(sc, gid, fsm, ('Break',), gravity, authored, missing)

    uncovers = []
    from reveal_masks import reveal_mask_sources
    reveals = reveal_mask_sources(sc)
    admitted = {record['game_object'] for record in reveals['controllers']}
    refused = {record['source']: record['error'] for record in reveals['unsupported']}
    for target in _uncover_targets(sc, gid, fsm, shape):
        for controller, kind, tree in _components(sc, target):
            if kind != 'PlayMakerFSM':
                continue
            source_id = sc.sid(controller)
            entry = {'source': source_id, 'game_object': sc.sid(target),
                     'name': sc.gos[target]['m_Name'], 'definition': definition_name(tree['fsm'])}
            if sc.sid(target) in admitted:
                entry['reveal'] = 'admitted'
            elif source_id in refused:
                entry['reveal'] = 'refused'
                entry['reveal_error'] = refused[source_id]
            else:
                # Crossroads_03's `crossroads_03_mask` is this: a two-state
                # `Idle -UNCOVER-> Fade` definition with no `Trigger2dEvent` at
                # all, so host/reveal_masks.py's candidate screen never sees it.
                entry['reveal'] = 'not a reveal controller'
            uncovers.append(entry)
    if uncovers:
        authored.append(OUTPUT_MASK)
        unusable = [entry for entry in uncovers if entry['reveal'] != 'admitted']
        if unusable:
            missing.append({'output': OUTPUT_MASK,
                            'reason': 'the break uncovers ' + ', '.join(
                                f"{entry['name']} ({entry['reveal']})" for entry in unusable)})

    authored = [name for name in OUTPUTS if name in authored]
    refused_outputs = sorted({item['output'] for item in missing} & set(ENFORCED_OUTPUTS))
    return {
        'gid': gid, 'source': sc.sid(index), 'game_object': sc.sid(gid),
        'name': sc.gos[gid]['m_Name'], 'definition': shape['definition'],
        'fsm_sha256': shape['sha256'], 'position': sc.point(gid),
        'nail_hits': hits, 'spell_attack_type': HIDDEN_WALL_SPELL_ATTACK_TYPE,
        'facing': variables['Facing'],
        # world_geometry keys every terrain edge by its collider's source id, so
        # this is the join key a cook needs to take the wall's wall away.
        'collider_source': sc.sid(body_id), 'hit_polygons': polygons,
        'box': [min(p[0] for p in points), min(p[1] for p in points),
                max(p[0] for p in points), max(p[1] for p in points)],
        'renderer_source': renderer_source, 'renderer_type': shape['renderer'],
        'uncovers': uncovers, 'break_clips': _break_clips(sc, fsm, 'Break'),
        'destruction': {'authored': authored, 'missing': missing, 'refused_outputs': refused_outputs},
        'limitations': [
            f'{hits} nail hits or one spell; the port has no multi-hit break, so this count '
            'is recorded and not yet run',
            'Recoil, hit sparks, the camera shake the break sends to CameraShake and the '
            'PersistentBoolItem that keeps a wall broken across a reload are not reproduced'],
    }


def hidden_walls(sc, errors=None, contract=True, drawn_renderers=None):
    """Every active hidden wall in the scene, with its break contract.

    With `contract`, a wall whose authored destruction output the port cannot
    produce is refused here, so it stays solid drawn terrain instead of opening
    onto a black mask it cannot take with it. That is the step 4 rule read for
    this family, and it is the whole reason the wall and the secret behind it
    are decided together.
    """
    gravity = None
    result = []
    for index, (typ, tree) in sorted(sc.objects.items()):
        if typ != 'PlayMakerFSM' or not tree['m_Enabled']:
            continue
        gid = _local_id(tree['m_GameObject'])
        if gid not in sc.gos or not sc.active(gid):
            continue
        try:
            shape = hidden_wall_shape(tree['fsm'])
            if shape is None:
                continue
        except ValueError as error:
            if errors is None:
                raise
            errors.append({'id': sc.sid(index), 'type': 'hidden wall', 'error': str(error)})
            continue
        try:
            if gravity is None:
                from break_effects import scene_gravity
                gravity = scene_gravity(sc.source)
            record = hidden_wall(sc, gid, index, tree['fsm'], shape, gravity, drawn_renderers)
            if contract and record['destruction']['refused_outputs']:
                raise ValueError('authored destruction output the port cannot produce: '
                                 + '; '.join(f"{item['output']} ({item['reason']})"
                                             for item in record['destruction']['missing']
                                             if item['output'] in ENFORCED_OUTPUTS))
            result.append(record)
        except Exception as error:
            if errors is None:
                raise
            errors.append({'id': sc.sid(index), 'type': 'hidden wall', 'error': str(error)})
    return result


# -------------------------------------------------------------- cracked floors
#
# `break_floor` is the floor version of the same idea and it is authored twice:
# 5 instances over 5 scenes, 2 digests, no placement variable between them.
#
#   b74c4cdb  4 instances. `Idle` receives damage through the C# `ReceivedDamage`
#             action, `Check If Nail` requires the hero inside the `Hero Range`
#             child trigger and attack type 0, and three hits reach `Break`.
#   72a2d729  1 instance, Crossroads_09. `Idle` carries a `Nail Attack`
#             `Trigger2dEvent` straight to `Hit`, so nothing gates the type or
#             the hero's distance, and `Break` runs through `Break Wood` first.
#
# The floor's terrain is not its own collider. The owner carries a trigger on
# layer 8 that is only the nail target; the solid the hero stands on is a
# `Solid` child, and `Break` takes it away with `ActivateGameObject`. That is
# why tools/breakable_catalog.py's terrain screen does not list this family: the
# collider that matters belongs to an object with no FSM on it.
CRACKED_FLOOR_NAIL_HITS = 3
CRACKED_FLOOR_SOLID_CHILD = 'Solid'
CRACKED_FLOOR_SHAPES = {
    'b74c4cdb9150a87be0a398b4d50bab0a1ebf40f99c2a1a746768b8573ed72d55': {
        'definition': 'break_floor', 'start': 'Pause', 'gate': 'Hero Range and attack type',
        'states': frozenset({'Idle', 'Check If Nail', 'Hit', 'Initiate', 'Break',
                             'Hit 1', 'Hit 2', 'Pause', 'Activated'})},
    '72a2d729923a6564f3059f52037143bc90ad25cbf7975787bb5d4f59883d3f12': {
        'definition': 'break_floor', 'start': 'Pause', 'gate': 'any Nail Attack trigger',
        'states': frozenset({'Idle', 'Check If Nail', 'Hit', 'Initiate', 'Break',
                             'Break Wood', 'Hit 1', 'Hit 2', 'Pause', 'Activated'})},
}


def cracked_floor_shape(fsm):
    """The pinned cracked-floor shape this FSM is, or None."""
    from false_knight import fsm_digest
    names = _state_names(fsm)
    if not any(names == shape['states'] for shape in CRACKED_FLOOR_SHAPES.values()):
        return None
    digest = fsm_digest(fsm)
    shape = CRACKED_FLOOR_SHAPES.get(digest)
    if shape is None:
        raise ValueError(f'unverified cracked floor variant: {digest}')
    if definition_name(fsm) != shape['definition']:
        raise ValueError(f'cracked floor digest {digest} under definition {definition_name(fsm)!r}')
    if fsm['startState'] != shape['start']:
        raise ValueError(f"cracked floor starts in {fsm['startState']!r}")
    return dict(shape, sha256=digest)


def cracked_floor(sc, gid, index, fsm, shape, gravity):
    """One cracked floor's authored break, and which of it the port can produce."""
    variables = _fsm_variables(fsm)
    if variables.get('Activated') not in (0, False):
        raise ValueError('cracked floor is serialized already broken')
    if variables.get('Hits') not in (0, False):
        raise ValueError('cracked floor is serialized part way through its hits')
    colliders = [(i, t, c) for i, t, c in _components(sc, gid) if t.endswith('Collider2D')]
    if len(colliders) != 1:
        raise ValueError(f'cracked floor carries {len(colliders)} hit colliders, not one')
    body_id, collider_type, body = colliders[0]
    if not body['m_Enabled'] or not body['m_IsTrigger']:
        raise ValueError('cracked floor hit collider is not an enabled trigger')
    polygons = collider_polygons(sc, gid, collider_type, body)
    points = [p for polygon in polygons for p in polygon]

    solids = []
    for child in sorted(_descendants(sc, gid) - {gid}):
        if sc.gos[child]['m_Name'] != CRACKED_FLOOR_SOLID_CHILD or not sc.active(child):
            continue
        for i, t, c in _components(sc, child):
            if t.endswith('Collider2D') and c['m_Enabled'] and not c['m_IsTrigger']:
                solids.append(sc.sid(i))
    if not solids:
        raise ValueError(f'cracked floor has no active {CRACKED_FLOOR_SOLID_CHILD} collider to remove')

    authored, missing = [OUTPUT_COLLIDER, OUTPUT_VISUAL], []
    _audio_output(_break_clips(sc, fsm, 'Break') + _break_clips(sc, fsm, 'Break Wood'), authored, missing)
    emitters = [g for g in sorted(_descendants(sc, gid) - {gid})
                if 'ParticleSystem' in {t for _, t, _ in _components(sc, g)}]
    _particle_outputs(sc, emitters, gravity, authored, missing, _played_emitters(sc, gid, fsm))
    _prefab_particle_outputs(sc, gid, fsm, ('Break', 'Break Wood'), gravity, authored, missing)
    renderers = [sc.sid(i) for child in sorted(_descendants(sc, gid) - {gid})
                 for i, t, d in _components(sc, child)
                 if t == 'SpriteRenderer' and d['m_Enabled'] and sc.active(child)]

    authored = [name for name in OUTPUTS if name in authored]
    refused_outputs = sorted({item['output'] for item in missing} & set(ENFORCED_OUTPUTS))
    return {
        'gid': gid, 'source': sc.sid(index), 'game_object': sc.sid(gid),
        'name': sc.gos[gid]['m_Name'], 'definition': shape['definition'],
        'fsm_sha256': shape['sha256'], 'position': sc.point(gid),
        'nail_hits': CRACKED_FLOOR_NAIL_HITS, 'hit_gate': shape['gate'],
        'hit_collider': sc.sid(body_id), 'hit_polygons': polygons,
        'box': [min(p[0] for p in points), min(p[1] for p in points),
                max(p[0] for p in points), max(p[1] for p in points)],
        # The colliders the break removes, keyed the way world_geometry keys
        # terrain edges. The hit trigger is not one of them; it is not terrain.
        'solid_collider_sources': solids, 'renderer_sources': renderers,
        'destruction': {'authored': authored, 'missing': missing, 'refused_outputs': refused_outputs},
        'limitations': [
            f'{CRACKED_FLOOR_NAIL_HITS} nail hits with the two staged sag frames between them; the '
            'port has no multi-hit break, so this count is recorded and not yet run',
            'The flung wood and rock pool objects, the camera shake and the PersistentBoolItem '
            'that keeps a floor broken across a reload are not reproduced'],
    }


def cracked_floors(sc, errors=None, contract=True):
    """Every active cracked floor in the scene, with its break contract."""
    gravity = None
    result = []
    for index, (typ, tree) in sorted(sc.objects.items()):
        if typ != 'PlayMakerFSM' or not tree['m_Enabled']:
            continue
        gid = _local_id(tree['m_GameObject'])
        if gid not in sc.gos or not sc.active(gid):
            continue
        try:
            shape = cracked_floor_shape(tree['fsm'])
            if shape is None:
                continue
        except ValueError as error:
            if errors is None:
                raise
            errors.append({'id': sc.sid(index), 'type': 'cracked floor', 'error': str(error)})
            continue
        try:
            if gravity is None:
                from break_effects import scene_gravity
                gravity = scene_gravity(sc.source)
            record = cracked_floor(sc, gid, index, tree['fsm'], shape, gravity)
            if contract and record['destruction']['refused_outputs']:
                raise ValueError('authored destruction output the port cannot produce: '
                                 + '; '.join(f"{item['output']} ({item['reason']})"
                                             for item in record['destruction']['missing']
                                             if item['output'] in ENFORCED_OUTPUTS))
            result.append(record)
        except Exception as error:
            if errors is None:
                raise
            errors.append({'id': sc.sid(index), 'type': 'cracked floor', 'error': str(error)})
    return result


# ------------------------------------------------------------ infected vines
#
# BreakableInfectedVine needs no FSM recognizer, but it is not a Breakable
# either, and reading it as one would be wrong in both directions.
# `OnTriggerEnter2D` keeps the vine's own renderer and collider and instead
# deactivates every `blobs` entry, asks the global blood pool for
# `spatterAmount` spatters at each blob's position between
# `spatterAngleMin..Max` at `spatterSpeedMin..Max`, activates every `effects`
# entry and plays its AudioSource at a pitch in `audioPitchMin..Max`. It sets a
# plain field, not a PersistentBoolItem, so a cut vine returns on scene reload.
VINE_COMPONENT = 'BreakableInfectedVine'
# `Start` disables the AudioSource, the Collider2D and the component itself when
# the object sits further than this from the plane the vines are authored on, so
# a vine outside the band can never be hit. This is the same shape as
# Breakable's inertForegroundThreshold/inertBackgroundThreshold, except that
# these two are literals in `Start` rather than serialized per instance.
VINE_DEPTH_CENTRE = 0.004000000189989805
VINE_DEPTH_RANGE = 1.0
# `Nail Attack`, `Hero Spell`, and `HeroBox` while cState.superDashing.
VINE_HIT_TAGS = ('Nail Attack', 'Hero Spell', 'HeroBox')
# BreakableInfectedVine authors one output Breakable.Break never does: a clip
# played on a separate object, its tk2d `effects` and its Mecanim blobs.
OUTPUT_ANIMATION = 'animation'
VINE_ENFORCED_OUTPUTS = (OUTPUT_VISUAL, OUTPUT_PARTICLES, OUTPUT_ANIMATION)


def _vine_parts(sc, refs, what):
    """The in-scene objects one vine array names, with what each carries."""
    file = Path(sc.file.name).name
    parts = []
    for ref in refs:
        if not ref['m_PathID']:
            continue
        if ref['m_FileID']:
            raise ValueError(f'vine {what} lives outside this scene')
        part_gid = ref['m_PathID']
        if part_gid not in sc.gos:
            raise ValueError(f'vine {what} is not a readable scene object')
        parts.append({'game_object': f'{file}:{part_gid}', 'name': sc.gos[part_gid]['m_Name'],
                      'components': sorted(typ for _, typ, _ in _components(sc, part_gid))})
    return parts


def infected_vine(sc, gid, tree):
    """One vine's authored output, and which of it the port can produce."""
    position = sc.point(gid)
    inert = abs(position[2] - VINE_DEPTH_CENTRE) > VINE_DEPTH_RANGE
    blobs = _vine_parts(sc, tree['blobs'], 'blob')
    effects = _vine_parts(sc, tree['effects'], 'effect')
    colliders = [(i, t, c) for i, t, c in _components(sc, gid) if t.endswith('Collider2D')]
    if len(colliders) != 1:
        raise ValueError(f'vine carries {len(colliders)} colliders, not one')
    body_id, collider_type, body = colliders[0]
    if not body['m_IsTrigger']:
        raise ValueError('vine hit collider is not a trigger')
    spatter = [tree['spatterAmount'], tree['spatterAngleMin'], tree['spatterAngleMax'],
               tree['spatterSpeedMin'], tree['spatterSpeedMax']]
    if not all(isinstance(v, (int, float)) and math.isfinite(v) for v in spatter):
        raise ValueError('vine spatter parameters are not finite numbers')

    authored, missing = [], []
    if blobs:
        authored.append(OUTPUT_VISUAL)
        # The blobs are the vine's visible payload and they only go away; the
        # port can turn a SpriteRenderer off, but only if the cook draws it.
        for blob in blobs:
            if 'SpriteRenderer' not in blob['components']:
                missing.append({'output': OUTPUT_VISUAL, 'part': blob['game_object'],
                                'reason': 'vine blob has no SpriteRenderer to hide'})
        animated = [blob for blob in blobs if 'Animator' in blob['components']]
        if animated:
            authored.append(OUTPUT_ANIMATION)
            missing.append({'output': OUTPUT_ANIMATION,
                            'reason': f'{len(animated)} vine blobs animate through Mecanim, '
                                      'which no cooked animation path reaches'})
    if spatter[0]:
        # GlobalPool.SpawnBlood, not a ParticleSystem on the object, so
        # break_effects.part_emitter has nothing to read and no style to cook.
        authored.append(OUTPUT_PARTICLES)
        missing.append({'output': OUTPUT_PARTICLES,
                        'reason': f'{int(spatter[0])} blood spatters per blob come from the '
                                  'global pool, which this port has no emitter for'})
    if effects:
        authored.append(OUTPUT_ANIMATION)
        missing.append({'output': OUTPUT_ANIMATION,
                        'reason': 'vine effects are tk2d clips on separate objects: '
                                  + ', '.join(effect['name'] for effect in effects)})
    refused = sorted({item['output'] for item in missing} & set(VINE_ENFORCED_OUTPUTS))
    file = Path(sc.file.name).name
    return {'gid': gid, 'name': sc.gos[gid]['m_Name'], 'position': position,
            'game_object': f'{file}:{gid}', 'hit_collider': f'{file}:{body_id}',
            'collider_type': collider_type, 'inert_by_depth': inert,
            'hit_tags': list(VINE_HIT_TAGS), 'spatter': spatter,
            'audio_pitch': [tree['audioPitchMin'], tree['audioPitchMax']],
            'blobs': blobs, 'effects': effects, 'persistent': False,
            'destruction': {'authored': sorted(set(authored)), 'missing': missing,
                            'refused_outputs': refused},
            'limitations': ['Vine state is not persistent in the source either; a cut vine '
                            'returns when the scene reloads']}


class SourceInert(ValueError):
    """The original disables this object too, so no port gap is being reported.

    Counting these next to real refusals overstates the work left. Five of the
    thirteen infected vines are in this state: `Start` turns off their
    AudioSource, their Collider2D and the component itself, so they are
    un-hittable background art in the original as well. They can never be
    admitted and they are not owed to anyone.
    """


def infected_vines(sc, errors=None, contract=True):
    """Every BreakableInfectedVine in the scene that the source leaves hittable.

    A vine outside the authored depth band is reported with `inert_by_depth` and
    not admitted, because `Start` disables the component before any hit can
    reach it. With `contract`, a vine whose authored output the port cannot
    produce is refused here, so it stays whole scenery instead of losing its
    blobs with no spatter, no effect clip and no sound.
    """
    file = Path(sc.file.name).name
    result = []
    for index, (typ, tree) in sorted(sc.objects.items()):
        if typ != VINE_COMPONENT:
            continue
        gid = _local_id(tree['m_GameObject'])
        if gid not in sc.gos or not tree.get('m_Enabled', 1) or not sc.active(gid):
            continue
        try:
            record = infected_vine(sc, gid, tree)
            record['source'] = f'{file}:{index}'
            if record['inert_by_depth']:
                raise SourceInert('vine is outside the authored depth band and Start disables it: '
                                  f"z {record['position'][2]:.3f}")
            if contract and record['destruction']['refused_outputs']:
                raise ValueError('authored destruction output the port cannot produce: '
                                 + '; '.join(f"{item['output']} ({item['reason']})"
                                             for item in record['destruction']['missing']
                                             if item['output'] in VINE_ENFORCED_OUTPUTS))
            result.append(record)
        except Exception as error:
            if errors is None:
                raise
            errors.append({'id': f'{file}:{index}', 'type': VINE_COMPONENT, 'error': str(error),
                           'source_inert': isinstance(error, SourceInert)})
    return result


def breakable_sources(sc, bounds=None, errors=None, contract=True):
    """Return authored records, optionally overlapping (xmin,ymin,xmax,ymax).

    state_index is the sorted ordinal of ALL Breakable components in the source
    scene, independent of region bounds, activation and draw ordering. Keep the
    state bitmap associated with its scene; source is the persistent identity.
    If errors is supplied, unsupported records are appended there; otherwise a
    malformed selected object fails explicitly rather than silently disappearing.

    With `contract`, an instance that authors an enforced destruction output the
    port cannot produce is refused here, so it stays solid drawn scenery instead
    of vanishing without it. Passes that report their own per-part refusals
    (the effect and fragment art cooks) pass contract=False and see everything.
    """
    file = Path(sc.file.name).name
    gravity = None
    all_ids = sorted(o.path_id for o in sc.file.objects.values()
                     if o.type.name == 'MonoBehaviour' and sc.source.typename(o) == 'Breakable')
    if len(all_ids) > MAX_SCENE_BREAKABLES:
        raise ValueError(f'Breakable scene state budget exceeded: {len(all_ids)} > {MAX_SCENE_BREAKABLES}')
    result = []
    for state_index, index in enumerate(all_ids):
        try:
            if index not in sc.objects:
                raise ValueError('Breakable schema was not successfully read')
            typ, tree = sc.objects[index]
            gid = _local_id(tree['m_GameObject'])
            if not tree['m_Enabled'] or not sc.active(gid):
                continue
            position = sc.point(gid)
            if not tree['inertForegroundThreshold'] <= position[2] <= tree['inertBackgroundThreshold']:
                continue
            colliders = [(i, t, c) for i, t, c in _components(sc, gid) if t.endswith('Collider2D')]
            if not colliders:
                raise ValueError('Breakable has no body collider')
            body_id, collider_type, body = colliders[0]  # Awake.GetComponent<Collider2D>
            if not body['m_Enabled']:
                continue
            hit_polygons = collider_polygons(sc, gid, collider_type, body)
            points = [p for poly in hit_polygons for p in poly]
            box = [min(p[0] for p in points), min(p[1] for p in points),
                   max(p[0] for p in points), max(p[1] for p in points)]
            if bounds and (box[2]<bounds[0] or box[0]>bounds[2] or box[3]<bounds[1] or box[1]>bounds[3]):
                continue
            whole_gids = set()
            remnant_gids = set()
            for ref in tree['wholeParts']:
                if ref['m_PathID']:
                    whole_gids.update(_descendants(sc, _local_id(ref)))
            for ref in tree['remnantParts']:
                if ref['m_PathID']:
                    remnant_gids.update(_descendants(sc, _local_id(ref)))
            off = {_local_id(tree['wholeRenderer'])} if tree['wholeRenderer']['m_PathID'] else set()
            on = set()
            disabled = {body_id}
            for part_gid in whole_gids | remnant_gids:
                for part_id, part_type, part in _components(sc, part_gid):
                    if part_type == 'SpriteRenderer' and part['m_Enabled']:
                        (off if part_gid in whole_gids else on).add(part_id)
                    elif part_type.endswith('Collider2D') and part_gid in whole_gids and part['m_Enabled']:
                        disabled.add(part_id)
            for renderer_id in off | on:
                if sc.objects.get(renderer_id, (None,))[0] != 'SpriteRenderer':
                    raise ValueError('non-SpriteRenderer Breakable static part')
            if off & on:
                raise ValueError('Breakable renderer is both whole and remnant')
            persistence = []
            for component_id, component_type, component in _components(sc, gid):
                if component_type == 'PersistentBoolItem':
                    persistence.append({'source': f'{file}:{component_id}',
                        'authored_id': component['persistentBoolData']['id'],
                        'runtime_id_if_empty': sc.gos[gid]['m_Name'],
                        'authored_scene_name': component['persistentBoolData']['sceneName'],
                        'semi_persistent': bool(component['semiPersistent']), 'dont_save': bool(component['dontSave'])})
            debris = []
            for ref in tree['debrisParts']:
                if not ref['m_PathID']:
                    continue
                part_gid = _local_id(ref)
                components = [{'source': f'{file}:{part_id}', 'type': part_type,
                               **({'serialized': part} if part_type in ('Rigidbody2D','SpinSelf','ObjectBounce') else {})}
                              for part_id, part_type, part in _components(sc, part_gid)]
                debris.append({'game_object': f'{file}:{part_gid}', 'name': sc.gos[part_gid]['m_Name'],
                               'position': sc.point(part_gid), 'components': components})
            receiver = tree['hitEventReciever']
            fades, event_errors = mask_fades(sc,_local_id(receiver)) if receiver['m_PathID'] else ([],[])
            record = {'source': f'{file}:{index}', 'game_object': f'{file}:{gid}', 'gid': gid,
                'name': sc.gos[gid]['m_Name'], 'state_index': state_index, 'scene_state_count': len(all_ids),
                'position': position, 'hit_points': 1, 'box': box, 'hit_polygons': hit_polygons,
                'body_collider': f'{file}:{body_id}', 'body_is_trigger': bool(body['m_IsTrigger']),
                'off_renderer_ids': sorted(off), 'on_renderer_ids': sorted(on),
                'disabled_collider_ids': sorted(disabled),
                'off_renderer_sources': [f'{file}:{i}' for i in sorted(off)],
                'on_renderer_sources': [f'{file}:{i}' for i in sorted(on)],
                'disabled_collider_sources': [sc.sid(i) for i in sorted(disabled)],
                'persistence': persistence, 'audio': _audio(sc, tree), 'mask_fades': fades, 'event_errors': event_errors,
                'debris': debris, 'fling_speed': [tree['flingSpeedMin'], tree['flingSpeedMax']],
                'angle_offset': tree['angleOffset'],
                'forwarded_events': {'hit_receiver': sc.source.sid(sc.source.ref(sc.file,receiver)) if receiver['m_PathID'] else None,
                    'receiver_event': 'HIT' if receiver['m_PathID'] else None,
                    'self_event': 'BREAK' if tree['forwardBreakEvent'] else None},
                'limitations': ['Debris physics, dust and impact effects, audio playback and forwarded FSM events require separate runtime support',
                    'PersistentBoolItem identity/state is recorded; no memory-card or retail-save compatibility is implied'],
            }
            if contract:
                if gravity is None:
                    from break_effects import scene_gravity
                    gravity = scene_gravity(sc.source)
                record['destruction'] = destruction_contract(sc, record, gravity)
                if record['destruction']['refused_outputs']:
                    raise ValueError('authored destruction output the port cannot produce: '
                                     + '; '.join(f"{item['output']} ({item['reason']})"
                                                 for item in record['destruction']['missing']
                                                 if item['output'] in ENFORCED_OUTPUTS))
            result.append(record)
        except Exception as error:
            if errors is None:
                raise
            errors.append({'id': f'{file}:{index}', 'type': 'Breakable', 'error': str(error)})
    return result


def bind_breakables(records, draws, edges):
    """Attach current region indices without changing stable scene state indices."""
    draw_ids = {d['source']: i for i, d in enumerate(draws)}
    out = []
    for record in records:
        bound = dict(record)
        bound['off_draws'] = [draw_ids[s] for s in record['off_renderer_sources'] if s in draw_ids]
        bound['on_draws'] = [draw_ids[s] for s in record['on_renderer_sources'] if s in draw_ids]
        bound['edge_indices'] = [i for i, edge in enumerate(edges) if edge['source'] in record['disabled_collider_sources']]
        bound['mask_fades'] = [dict(f,draw_indices=[draw_ids[s] for s in f['renderer_sources'] if s in draw_ids]) for f in record.get('mask_fades',[])]
        bound['unresident_renderer_sources'] = [s for s in record['off_renderer_sources']+record['on_renderer_sources'] if s not in draw_ids]
        out.append(bound)
    return out


if __name__ == '__main__':
    from source import Source, ROOT, dump
    from scene import Scene
    source = Source(); scene = Scene(source, 'level6'); errors = []
    records = breakable_sources(scene, errors=errors)
    dump(ROOT/'.hkpsx/breakables-source.json', {'scene': 'Tutorial_01', 'records': records, 'errors': errors,
         'methods': ['Breakable.Awake','Breakable.Start','Breakable.Hit','Breakable.Break','Breakable.SetStaticPartsActivation','PersistentBoolItem.SetMyID'],
         'source_constraints': 'Original C# first-hit behavior; no generic PlayMaker execution'})
    print(f'{len(records)} active in-depth Breakables, {len(errors)} unsupported records')
    for record in records:
        print(record['state_index'], record['name'], record['box'], record['off_renderer_ids'], record['on_renderer_ids'], record['disabled_collider_ids'])
