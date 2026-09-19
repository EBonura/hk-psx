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
#                     in host/cook_audio.py, so no cook can satisfy them.
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
                raise ValueError('vine is outside the authored depth band and Start disables it: '
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
            errors.append({'id': f'{file}:{index}', 'type': VINE_COMPONENT, 'error': str(error)})
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
