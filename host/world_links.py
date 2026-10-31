"""Switches, the gates they open, and the PlayerData observers that follow them.

One pass over the source scenes that turns every lever-and-gate pair into data the
engine already knows how to run: a scene state bit, the terrain edges that bit
lifts, and the draws it hides. Nothing here is keyed on a scene or object name.

The original
------------
A `Switch Control` FSM sits on an object with a nail-hit trigger. A hit plays its
`Hit` state (a wait), enters `Open`, and sends `ACTIVATE` to the GameObject its
`Target` variable points at; a switch that was hit before (its own
`PersistentBoolItem`, or the PlayerData bool named by its `Player Data` variable
when `SetPlayerData` is set) starts in `Activated` and sends `ACTIVATE AGAIN`
instead, so the gate is open on load. The target carries a gate FSM (`Pillar
Control`, `Gate`, `Gate Control`, `Mantis Gate`, `Toll Gate`) whose `ACTIVATE`
path ends in `SetCollider` false or `DestroySelf`. Separately, a collider object
may carry `DeactivateIfPlayerdataTrue(bool)`, which removes it on load once the
bool is set; Crossroads_33's `sliding_wall` is open for the Knight exactly when
Crossroads_06's switch has set `shamanPillar`.

The port
--------
`world::State` already keeps one persistent bit per scene state (`scene * 128 +
index`), lifts the terrain edges and hides the draws bound to a set bit
(`KIND_REMOTE_ART` carriers), and saves the bit as a `Kind::Breakable` item. So:

  * a switch is a bank breakable with flag 16 (`LEVER`): the nail sets its bit and
    nothing flies off it. Its index is taken from the top of the scene's range
    below the secrets (`LEVER_TOP`);
  * every gate collider and renderer it opens is a remote-art binding on that bit,
    in every view that holds an edge or a draw of it;
  * every `DeactivateIfPlayerdataTrue` collider object whose bool exactly one
    cooked switch sets is bound to that switch's bit, in whatever scene it is
    (the carrier's state base is the switch's scene).

A `VinePlatform` is the same pair folded into one object: a nail cut on its `Vine` child
(`VinePlatformCut`) sets a persistent bit; the hanging platform's collider and art go, and
the `Activated Plat` the platform falls onto, an inactive object the cook skips otherwise,
becomes solid and visible (`bit_enabled` tells the cook which inactive objects to bake). Its
edges are bound with `inverted`: present while the bit is SET.

A gate target whose FSM is not one of the gate families (slide floors, doors,
lifts, bells, stompers) is not opened: it is counted in the report by family so
what is left for later areas is known. The cook never guesses: a target is bound
only after its `ACTIVATE` path is walked and found to remove its own collider.

Run by `host/regions.py` as a postpass over the finished report; it reads source
scenes but changes no region's cooked bytes, so it is below the divider in
`cook_inputs.txt`.
"""
import heapq
from collections import Counter
from pathlib import Path

from breakables import _components, _state_actions as _authored_actions, collider_polygons, TERRAIN_LAYER

SWITCH_FSM = 'Switch Control'
GATE_FAMILIES = ('Pillar Control', 'Gate', 'Gate Control', 'Mantis Gate', 'Toll Gate')
OBSERVER_COMPONENT = 'DeactivateIfPlayerdataTrue'
HIT_EVENT = 'NAIL HIT'
LEVER_FLAG = 16
# Secrets number their states from 127 down and hold at most 8 (game/src/secret_breaks.rs).
LEVER_TOP = 119
STATES_PER_SCENE = 128
TICKS_PER_SECOND = 60
# A tk2dSprite is drawn by the cook as one scenery draw whose source is the sprite component
# (`tk2d_art`); a MeshRenderer or SpriteRenderer is its own draw source.
RENDERER_TYPES = ('MeshRenderer', 'SpriteRenderer', 'tk2dSprite')


class Refused(Exception):
    """A switch, target or observer this pass does not run, with the reason."""


def _file(sc):
    return Path(sc.file.name).name


def _state_actions(fsm, name):
    """A state's enabled actions, or none for a name the FSM does not hold exactly once."""
    try:
        return _authored_actions(fsm, name)
    except ValueError:
        return []


def _transitions(fsm):
    """(state name or None for global, event, target state) for every transition."""
    for transition in fsm.get('globalTransitions', []):
        yield None, transition['fsmEvent']['name'], transition['toState']
    for state in fsm['states']:
        for transition in state.get('transitions', []):
            yield state['name'], transition['fsmEvent']['name'], transition['toState']


def _state_wait(fsm, name):
    """Seconds the longest literal `Wait` in one state holds it."""
    longest = 0.0
    for action, fields in _state_actions(fsm, name):
        if action == 'Wait':
            time = fields.get('time')
            if isinstance(time, dict) and not time.get('useVariable'):
                longest = max(longest, float(time.get('value', 0.0)))
    return longest


def _removes_collider(fsm, name):
    """Whether a state turns its own collider off or destroys its own object."""
    for action, fields in _state_actions(fsm, name):
        if action == 'DestroySelf':
            return True
        if action == 'SetCollider':
            target = fields.get('gameObject')
            active = fields.get('active')
            if (isinstance(target, dict) and target.get('ownerOption') == 0
                    and isinstance(active, dict) and not active.get('useVariable')
                    and not active.get('value')):
                return True
    return False


def delay_to(fsm, events, reached):
    """Seconds of `Wait` between receiving one of `events` and a state `reached` accepts.

    Each transition on an event is an entry point (a gate FSM takes `ACTIVATE` from its
    resting state to play the opening and again from `Init` to open at once on a reload);
    the answer is the longest entry's, which is the live one, and None when no entry
    reaches such a state. The walk follows every authored transition (an FSM's whole
    control flow once globals are included) and charges a state the longest wait it
    holds before it leaves.
    """
    names = {state['name'] for state in fsm['states']}
    edges = {}
    entries = []
    for origin, event, target in _transitions(fsm):
        if target in names:
            edges.setdefault(origin, []).append(target)
            if event in events:
                entries.append(target)
    answers = []
    for entry in entries:
        queue = [(0.0, entry)]
        seen = set()
        while queue:
            cost, name = heapq.heappop(queue)
            if name in seen:
                continue
            seen.add(name)
            if reached(name):
                answers.append(cost)
                break
            cost += _state_wait(fsm, name)
            for target in edges.get(name, []):
                heapq.heappush(queue, (cost, target))
    return max(answers) if answers else None


def _fsm_object_variables(fsm):
    """FSM GameObject variables that point at an object of the same scene file."""
    variables = {}
    for variable in fsm['variables'].get('gameObjectVariables', []):
        pointer = variable.get('value')
        if isinstance(pointer, dict) and pointer.get('m_PathID') and not pointer.get('m_FileID'):
            variables[variable['name']] = pointer['m_PathID']
    return variables


def _fsm_scalars(fsm):
    values = {}
    for group in fsm['variables'].values():
        if isinstance(group, list):
            for variable in group:
                if isinstance(variable, dict) and 'name' in variable and 'value' in variable \
                        and not isinstance(variable['value'], dict):
                    values[variable['name']] = variable['value']
    return values


def _fsms(sc, gid):
    return [(index, tree['fsm']) for index, kind, tree in _components(sc, gid)
            if kind == 'PlayMakerFSM']


def _persistence(sc, gid):
    return [{'dont_save': bool(tree['dontSave']), 'semi_persistent': bool(tree['semiPersistent'])}
            for _, kind, tree in _components(sc, gid) if kind == 'PersistentBoolItem']


def _colliders(sc, gid, solid):
    """(source id, type, tree) of the enabled colliders, solid ones or triggers."""
    return [(sc.sid(index), kind, tree) for index, kind, tree in _components(sc, gid)
            if kind.endswith('Collider2D') and tree['m_Enabled'] and bool(tree['m_IsTrigger']) != solid]


def _box(polygons):
    points = [p for polygon in polygons for p in polygon]
    return [min(p[0] for p in points), min(p[1] for p in points),
            max(p[0] for p in points), max(p[1] for p in points)]


def gate_target(sc, gid, live_events, reload_events, sets_playerdata):
    """One switch target as a gate: its solid colliders, renderers and opening delay.

    Raises Refused with the family name when the object is not a collider gate.
    """
    gate_fsms = [(index, fsm) for index, fsm in _fsms(sc, gid) if fsm['name'] in GATE_FAMILIES]
    if len(gate_fsms) != 1:
        names = sorted({fsm['name'] for _, fsm in _fsms(sc, gid)}) or ['(no FSM)']
        raise Refused('family:' + '+'.join(names))
    _, fsm = gate_fsms[0]
    seconds = delay_to(fsm, live_events, lambda name: _removes_collider(fsm, name))
    # On a reload the switch re-sends `ACTIVATE AGAIN`, which the gate answers by opening at once
    # or, for a gate that keeps its own PersistentBoolItem (`Pillar Control`), ignores because it
    # reads its own bool in `Init`; either way it ends open.
    # (or by `DeactivateIfPlayerdataTrue` on the switch's own PlayerData bool).
    kept = any(kind == 'PersistentBoolItem' or (
        kind == OBSERVER_COMPONENT and sets_playerdata and tree['boolName'] == sets_playerdata)
        for _, kind, tree in _components(sc, gid))
    if seconds is None or not (kept or delay_to(
            fsm, reload_events, lambda name: _removes_collider(fsm, name)) is not None):
        raise Refused(f'family:{fsm["name"]} (its {sorted(live_events)} / {sorted(reload_events)} '
                      'do not remove its collider)')
    colliders = [source for source, _, _ in _colliders(sc, gid, solid=True)]
    renderers = [sc.sid(index) for index, kind, tree in _components(sc, gid)
                 if kind in RENDERER_TYPES and tree['m_Enabled']]
    if not colliders:
        raise Refused(f'family:{fsm["name"]} (no solid collider)')
    return {'game_object': f'{_file(sc)}:{gid}', 'name': sc.gos[gid]['m_Name'],
            'family': fsm['name'], 'collider_sources': colliders,
            'renderer_sources': renderers, 'delay_ticks': round(seconds * TICKS_PER_SECOND)}


def open_targets(fsm):
    """{object pointer: (events `Open` sends it on a hit, events `Activated` sends on a reload)}."""
    variables = _fsm_object_variables(fsm)
    targets = {}
    for state, slot in (('Open', 0), ('Activated', 1)):
        for action, fields in _state_actions(fsm, state):
            if action != 'SendEventByName':
                continue
            event = fields.get('sendEvent')
            owner = fields.get('eventTarget', {}).get('gameObject', {}).get('gameObject', {})
            pointer = variables.get(owner.get('name')) if isinstance(owner, dict) else None
            if pointer is not None and isinstance(event, dict) and event.get('value'):
                targets.setdefault(pointer, (set(), set()))[slot].add(event['value'])
    return {p: events for p, events in targets.items() if events[0] and events[1]}


def tk2d_art(sc):
    """Draw sources (tk2dSprite ids) of the idle art of every switch and gate the scene binds.

    The cook draws each as one scenery draw (host/cook.py `tk2d_wall_draw`). A gate's draw is
    bound off by its switch's bit; the switch's own stays. Neither plays its animation: the
    pillar's shake and sink and the switch's activated frame are not drawn.
    """
    found = []
    for index, (kind, tree) in sorted(sc.objects.items()):
        if kind != 'PlayMakerFSM' or tree['fsm']['name'] != SWITCH_FSM:
            continue
        gid = tree['m_GameObject']['m_PathID']
        if gid not in sc.gos or not sc.active(gid):
            continue
        scalars = _fsm_scalars(tree['fsm'])
        sets = scalars.get('Player Data') if scalars.get('SetPlayerData') else None
        gates = []
        for pointer, (live, reload) in open_targets(tree['fsm']).items():
            if pointer in sc.gos and sc.active(pointer):
                try:
                    gate_target(sc, pointer, live, reload, sets)
                except Refused:
                    continue
                gates.append(pointer)
        for under in [gid, *gates]:
            found.extend(sc.sid(i) for i, k, t in _components(sc, under) if k == 'tk2dSprite')
    return sorted(set(found))


def switch_record(sc, index, fsm, refusals):
    """The record of one `Switch Control`, or a Refused."""
    gid = _components_gid(sc, index)
    states = {state['name'] for state in fsm['states']}
    if not {'Initiate', 'Idle', 'Hit', 'Open'} <= states:
        raise Refused('switch states are not Initiate, Idle, Hit, Open')
    targets = open_targets(fsm)
    if not targets:
        raise Refused('switch sends no event to an object of its scene')
    scalars = _fsm_scalars(fsm)
    sets = scalars.get('Player Data') if scalars.get('SetPlayerData') else None
    # The targets are judged first so a switch this pass cannot run still counts its
    # gates' families in the report.
    gates = []
    for pointer, (live, reload) in sorted(targets.items()):
        if pointer not in sc.gos or not sc.active(pointer):
            refusals.append({'switch': f'{_file(sc)}:{index}', 'reason': 'target is not an active object'})
            continue
        try:
            gates.append(gate_target(sc, pointer, live, reload, sets))
        except Refused as error:
            refusals.append({'switch': f'{_file(sc)}:{index}', 'reason': str(error),
                             'target': sc.gos[pointer]['m_Name']})
    tags = [fields.get('collideTag') for action, fields in _state_actions(fsm, 'Idle')
            if action == 'Trigger2dEvent']
    if not any(isinstance(t, dict) and t.get('value') == 'Nail Attack' for t in tags):
        raise Refused('switch variant: no Nail Attack trigger (ReceivedDamage lever)')
    triggers = _colliders(sc, gid, solid=False)
    if not triggers:
        raise Refused('switch has no trigger collider')
    polygons = [polygon for _, kind, tree in triggers
                for polygon in collider_polygons(sc, gid, kind, tree)]
    hit_seconds = delay_to(fsm, (HIT_EVENT,), lambda name: name == 'Open')
    return {'source': f'{_file(sc)}:{index}', 'game_object': f'{_file(sc)}:{gid}',
            'name': sc.gos[gid]['m_Name'], 'box': _box(polygons),
            'hit_polygons': polygons, 'persistence': _persistence(sc, gid) or [
                {'dont_save': True, 'semi_persistent': False}],
            'sets_playerdata': sets, 'hit_delay_ticks': round((hit_seconds or 0.0) * TICKS_PER_SECOND),
            'gates': gates}


def _components_gid(sc, index):
    return sc.objects[index][1]['m_GameObject']['m_PathID']


VINE_PLATFORM = 'VinePlatform'
VINE_CUT = 'VinePlatformCut'


def _descendants(sc, root):
    """Every GameObject under `root`, itself included."""
    children = {}
    for tid, tree in sc.transforms.items():
        children.setdefault(tree['m_Father']['m_PathID'], []).append(tid)
    stack = [sc.go_transform[root]]
    found = []
    while stack:
        tid = stack.pop()
        found.append(sc.transforms[tid]['m_GameObject']['m_PathID'])
        stack.extend(children.get(tid, []))
    return found


def _renderer_sources(sc, gids):
    return [sc.sid(index) for gid in gids for index, kind, tree in _components(sc, gid)
            if kind in RENDERER_TYPES and tree['m_Enabled']]


def vine_records(sc):
    """One record per `VinePlatform` of the scene, shaped like a switch with one gate.

    The cut is the `Vine` child's trigger (`VinePlatformCut`). While the bit is clear the
    hanging platform stands (its collider, its `Plat Sprite` and the vine sprites); once set
    it is gone, and the `Activated Plat` collider and sprite are there instead.
    """
    records = []
    for index, (kind, tree) in sorted(sc.objects.items()):
        if kind != VINE_PLATFORM:
            continue
        gid = tree['m_GameObject']['m_PathID']
        if gid not in sc.gos or not sc.active(gid):
            continue
        activated = tree['activatedSprite']['m_PathID']
        below = _descendants(sc, gid)
        cuts = [cut for under in below for _, cut_kind, cut in _components(sc, under) if cut_kind == VINE_CUT]
        if activated not in sc.gos or len(cuts) != 1:
            raise Refused('vine platform without exactly one cut and an Activated Plat')
        cut = cuts[0]
        cut_gid = cut['m_GameObject']['m_PathID']
        triggers = _colliders(sc, cut_gid, solid=False)
        if not triggers:
            raise Refused('vine cut has no trigger collider')
        polygons = [polygon for _, k, t in triggers for polygon in collider_polygons(sc, cut_gid, k, t)]
        sprites = cut['sprites']['m_PathID']
        vine_art = _descendants(sc, sprites) if sprites in sc.gos else []
        plat_art = [under for under in below if sc.gos[under]['m_Name'] == 'Plat Sprite']
        records.append({
            'source': f'{_file(sc)}:{index}', 'game_object': f'{_file(sc)}:{gid}',
            'name': sc.gos[gid]['m_Name'], 'box': _box(polygons), 'hit_polygons': polygons,
            'persistence': _persistence(sc, gid) or [{'dont_save': True, 'semi_persistent': False}],
            'sets_playerdata': None, 'hit_delay_ticks': 0, 'family': VINE_PLATFORM,
            'gates': [{
                'game_object': f'{_file(sc)}:{gid}', 'name': sc.gos[gid]['m_Name'], 'family': VINE_PLATFORM,
                'collider_sources': [source for source, _, _ in _colliders(sc, gid, solid=True)],
                # The hanging platform's art and the cut vine (not the stub the vine leaves).
                'renderer_sources': _renderer_sources(sc, plat_art + vine_art),
                'on_collider_sources': [source for source, _, _ in _colliders(sc, activated, solid=True)],
                'on_renderer_sources': _renderer_sources(sc, [activated]),
                'delay_ticks': 0}]})
    return records


def bit_enabled(sc):
    """(GameObject ids, renderer ids) the scene leaves inactive that a persistent bit turns on.

    The cook bakes these colliders as terrain and draws these renderers although the object
    is inactive on load: the `Activated Plat` of a vine platform. Their edges are bound as
    `inverted`, so they are blank until the bit is set.
    """
    gids = set()
    renderers = set()
    for kind, tree in sc.objects.values():
        if kind == VINE_PLATFORM and tree['activatedSprite']['m_PathID'] in sc.gos:
            gid = tree['activatedSprite']['m_PathID']
            gids.add(gid)
            renderers.update(i for i, k, t in _components(sc, gid) if k in RENDERER_TYPES)
    return gids, renderers


def observers(sc):
    """Solid collider objects that a PlayerData bool removes: (bool, record)."""
    found = []
    for index, (kind, tree) in sorted(sc.objects.items()):
        if kind != OBSERVER_COMPONENT:
            continue
        gid = tree['m_GameObject']['m_PathID']
        if gid not in sc.gos or not sc.active(gid) or sc.gos[gid]['m_Layer'] != TERRAIN_LAYER:
            continue
        colliders = [source for source, _, _ in _colliders(sc, gid, solid=True)]
        if colliders:
            found.append((tree['boolName'], {
                'game_object': f'{_file(sc)}:{gid}', 'name': sc.gos[gid]['m_Name'],
                'collider_sources': colliders,
                'renderer_sources': [sc.sid(i) for i, k, t in _components(sc, gid)
                                     if k in RENDERER_TYPES and t['m_Enabled']]}))
    return found


def _free_states(report, scene_id):
    used = set()
    for row in report['regions']:
        if row['scene_id'] == scene_id:
            used.update(b['state_index'] for b in row.get('breakables', []))
            used.update(b['state_index'] for b in row.get('secrets', []))
    return [i for i in range(LEVER_TOP, -1, -1) if i not in used]


def postpack_links(report, source, scenes=None):
    """Find every switch, gate and PlayerData observer of the cooked scenes into `report['links']`."""
    from scene import Scene
    scenes = scenes if scenes is not None else {}
    links = []
    refusals = []
    pending = []
    for info in report['scenes']:
        sid = info['scene_id']
        if sid not in scenes:
            scenes[sid] = Scene(source, info['file'])
        sc = scenes[sid]
        for index, (kind, tree) in sorted(sc.objects.items()):
            if kind != 'PlayMakerFSM' or tree['fsm']['name'] != SWITCH_FSM:
                continue
            gid = tree['m_GameObject']['m_PathID']
            if gid not in sc.gos or not sc.active(gid):
                continue
            try:
                record = switch_record(sc, index, tree['fsm'], refusals)
            except Refused as error:
                refusals.append({'switch': f'{_file(sc)}:{index}', 'scene': info['scene_name'],
                                 'reason': str(error)})
                continue
            if not record['gates']:
                continue
            record.update(scene_id=sid, scene_name=info['scene_name'])
            for gate in record['gates']:
                gate['scene_id'] = sid
            pending.append(record)
        try:
            vines = vine_records(sc)
        except Refused as error:
            vines = []
            refusals.append({'switch': f'{_file(sc)}:vine', 'scene': info['scene_name'], 'reason': str(error)})
        for record in vines:
            record.update(scene_id=sid, scene_name=info['scene_name'])
            record['gates'][0]['scene_id'] = sid
            pending.append(record)
    by_scene = Counter()
    for record in pending:
        free = _free_states(report, record['scene_id'])
        taken = by_scene[record['scene_id']]
        if taken >= len(free):
            raise ValueError(f'scene {record["scene_name"]} has no free state for a switch')
        record['state_index'] = free[taken]
        by_scene[record['scene_id']] += 1
        links.append(record)
    setters = {}
    for record in links:
        if record['sets_playerdata']:
            setters.setdefault(record['sets_playerdata'], []).append(record)
    observed = []
    unbound = Counter()
    for info in report['scenes']:
        for name, observer in observers(scenes[info['scene_id']]):
            owners = setters.get(name, [])
            if len(owners) == 1:
                observer.update(scene_id=info['scene_id'], bool=name,
                                state_scene=owners[0]['scene_id'],
                                state_index=owners[0]['state_index'], owner=owners[0]['source'])
                observed.append(observer)
            else:
                unbound[name] += 1
    report['links'] = links
    report['link_observers'] = observed
    report['link_policy'] = {
        'switches': sum(1 for r in links if r.get('family') != VINE_PLATFORM),
        'vine_platforms': sum(1 for r in links if r.get('family') == VINE_PLATFORM),
        'gates': sum(len(r['gates']) for r in links if r.get('family') != VINE_PLATFORM),
        'observers': len(observed),
        'refused_targets_by_family': dict(sorted(Counter(
            r['reason'] for r in refusals if 'target' in r).items())),
        'refused_switches': [r for r in refusals if 'target' not in r],
        'observers_without_a_cooked_setter': dict(sorted(unbound.items())),
        'scope': 'Switch Control + collider gates (Pillar Control, Gate, Gate Control, Mantis Gate, '
                 'Toll Gate) and DeactivateIfPlayerdataTrue colliders whose bool one switch sets',
    }


def bindings_for(report, region, draw_sources, edge_sources):
    """(lever objects, remote-art bindings) one view carries for the report's links."""
    scene = region['scene_id']
    x0, y0, x1, y1 = region['activation_bounds']
    levers = []
    bindings = []

    def bind(owner, state_scene, state_index, target):
        def draws(key):
            return [i for i, source in enumerate(draw_sources) if source in target.get(key, [])]

        def edges(key):
            return [i for i, source in enumerate(edge_sources) if source in target.get(key, [])]
        off, on, gone, back = (draws('renderer_sources'), draws('on_renderer_sources'),
                               edges('collider_sources'), edges('on_collider_sources'))
        base = {'kind': 'breakable', 'owner_source': owner, 'state_index': state_index,
                'state_scene': state_scene}
        if off or on or gone:
            bindings.append(dict(base, off_draws=off, on_draws=on, edge_indices=gone))
        if back:
            # Present while the bit is SET: the carrier of these entries is inverted.
            bindings.append(dict(base, off_draws=[], on_draws=[], edge_indices=back, inverted=True))

    for link in report.get('links', []):
        if link['scene_id'] == scene:
            box = link['box']
            if not (box[2] < x0 - 2 or box[0] > x1 + 2 or box[3] < y0 - 2 or box[1] > y1 + 2):
                levers.append(dict({key: link[key] for key in (
                    'source', 'name', 'box', 'hit_polygons', 'persistence', 'state_index')},
                    # The source's `Hit` wait, then the longest of its gates' own opening.
                    open_ticks=link['hit_delay_ticks'] + max(g['delay_ticks'] for g in link['gates'])))
        for gate in link['gates']:
            if gate['scene_id'] == scene:
                bind(link['source'], link['scene_id'], link['state_index'], gate)
    for observer in report.get('link_observers', []):
        if observer['scene_id'] == scene:
            bind(observer['owner'], observer['state_scene'], observer['state_index'], observer)
    return levers, bindings
