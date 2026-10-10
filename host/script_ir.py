"""Compile PlayMaker FSMs to the HKSCR01 IR (P10 steps 2 and 4).

Only the first slice is supported: comparisons and branches, booleans and ints,
waits, and state and event sends. Every other action is an explicit refusal
naming the action, because step 7 is clear that an unsupported required action
must block its scene rather than compile to a no-op that lets the scene claim to
be implemented.

The refusals are the useful output right now. Running this over the admitted
scenes says exactly how much of the authored behaviour the current slice covers
and which action to implement next. FSMs the native actor controllers already
replace are counted apart from that, because they are not script work at all.

No retail payload is embedded; the report goes to .hkpsx.
"""
import collections, re, struct
from focus import action_fields

# Opcode numbers, in the declaration order of hk_sim::script::Code. Every
# variant is named even where nothing emits it yet, because a gap here shifts
# every opcode below it: omitting NativeConst silently aimed the three
# PlayerData opcodes one slot low.
(NOP, WAIT, SET_BOOL, SET_INT, BOOL_TEST, INT_COMPARE, SEND_EVENT, NEXT_FRAME, NATIVE,
 NATIVE_CONST, PD_GET, PD_SET, PD_BOOL_TEST, HERO_TRIGGER) = range(14)

CMP_LT, CMP_LE, CMP_EQ, CMP_NE, CMP_GE, CMP_GT = range(6)
# HeroTrigger phases, in the declaration order of PlayMaker's Trigger2DType,
# which is the enum the source action serializes.
TRIGGER_ENTER, TRIGGER_STAY, TRIGGER_EXIT = range(3)
# The flag bit marking an op that re-runs every tick while its state is current.
REPEAT = 0x80
# PlayMaker's FsmEventTarget.EventTarget, in declaration order. Only the three
# the scenes actually use are named. The order is confirmed by the data rather
# than assumed: all 214 instances carrying an `fsmName` are mode 2 and no
# instance of any other mode carries one, which is true of `GameObjectFSM` and
# of no other member.
TARGET_SELF, TARGET_GAME_OBJECT, TARGET_GAME_OBJECT_FSM = 0, 1, 2

def _check_opcodes():
    """Fail the import when these numbers drift from hk_sim::script.

    They already did once: `NativeConst` was added to the Rust enum and not
    here, which silently shifted all three PlayerData opcodes one slot low.
    Nothing had cooked a bank yet, so nothing caught it. Reading the enum is
    cheaper than the next silent miscompile.

    The flag and selector bytes are checked the same way and for the same
    reason: they are cooked into every op's `flags`, and a mismatch there is a
    behaviour change rather than a crash.
    """
    from source import ROOT
    import rustsrc
    text = rustsrc.source(ROOT / 'shared/hk-sim/src/script.rs')
    start = text.index('pub enum Code{') + len('pub enum Code{')
    variants = [v for v in text[start:text.index('}', start)].split(',') if v]
    expected = ['Nop', 'Wait', 'SetBool', 'SetInt', 'BoolTest', 'IntCompare', 'SendEvent',
                'NextFrameEvent', 'Native', 'NativeConst', 'PlayerDataGet', 'PlayerDataSet',
                'PlayerDataBoolTest', 'HeroTrigger']
    if variants != expected:
        raise AssertionError(f'hk_sim::script::Code changed: {variants} != {expected}')
    for name, value in re.findall(r'pub const (\w+):u8=(\w+);', text):
        here = globals().get(name)
        if here is not None and here != int(value, 0):
            raise AssertionError(f'hk_sim::script::{name} is {value}, not {here}')

_check_opcodes()

# The object reference constants, mirroring hk_sim::script.
HERO = 0x7fff_ffff
NO_OBJECT = 0

def source_id(path_id):
    """A scene path id as the stable 31-bit source id the cooked banks carry.

    HERO is the top of that range on purpose, so it survives the mask and cannot
    collide with a real object.
    """
    return path_id & 0x7fff_ffff

def decoded_action(data, index):
    """One action's fields, including the enums `action_fields` leaves out.

    A PlayMaker enum is a raw four-byte parameter rather than one of the compact
    scalar shapes, so the shared decoder skips it: adding it there would change
    the field set the actor recognizers match on. `Trigger2dEvent` carries its
    whole meaning in one, the Trigger2DType that says enter, stay or exit, so
    the compiler reads them here instead.
    """
    fields = action_fields(data, index, objects=True)
    start = data['actionStartIndex'][index]
    end = (data['actionStartIndex'][index + 1] if index + 1 < len(data['actionNames'])
           else len(data['paramName']))
    for i in range(start, end):
        if data['paramDataType'][i] != 7:
            continue
        pos, size = data['paramDataPos'][i], data['paramByteDataSize'][i]
        if size != 4:
            raise ValueError('invalid enum width')
        fields[data['paramName'][i] or str(i)] = struct.unpack(
            '<i', bytes(data['byteData'][pos:pos + size]))[0]
    return fields

class Unsupported(Exception):
    """An action this slice cannot compile. Carries the action for the report."""
    def __init__(self, action, detail=''):
        super().__init__(f'{action}{": " + detail if detail else ""}')
        self.action = action
        self.detail = detail

class Events:
    """Interned event names; zero is reserved for 'no event'."""
    def __init__(self):
        self.ids = {}
    def id(self, name):
        if not name:
            return 0
        if not isinstance(name, str):
            # An FsmString rather than the interned FsmEvent most actions carry.
            # This used to reach the dict lookup and raise TypeError, which the
            # survey read as an ordinary refusal, so every SendEventByName to
            # self was silently counted as uncompilable. Refuse by name instead.
            raise Unsupported('<event name>', 'event name is not a cooked constant')
        return self.ids.setdefault(name, len(self.ids) + 1)

class Fields:
    """PlayerData names a script touches, interned into a per-bank table. The
    guest resolves each to the persistent store's StateId, so a script reads and
    writes the same values the save file holds."""
    def __init__(self):
        self.names = []
    def index(self, name):
        if name not in self.names:
            self.names.append(name)
        return self.names.index(name)

class Constants:
    def __init__(self):
        self.values = []
    def index(self, value):
        value = int(value)
        if value not in self.values:
            self.values.append(value)
        return self.values.index(value)

def physics_layers(source):
    """The project's 2D layer names and collision matrix, read once per source.

    A `Trigger2dEvent` with no tag filter fires for whatever Unity's physics
    delivered to the collider, and what physics delivers is exactly what this
    matrix allows. So "could anything but the Knight be inside this volume" has
    a source-derived answer rather than a feeling about what the room contains.
    """
    if not hasattr(source, '_physics_layers'):
        file = source.file('globalgamemanagers')
        names = next(o for o in file.objects.values()
                     if o.type.name == 'TagManager').read_typetree()['layers']
        matrix = next(o for o in file.objects.values()
                      if o.type.name == 'Physics2DSettings').read_typetree()['m_LayerCollisionMatrix']
        source._physics_layers = (names, matrix)
    return source._physics_layers

class SceneObjects:
    """Cook-time object resolution over one scene's transform tree.

    The IR has no runtime object model: a reference is a stable source id the
    host maps to whatever it actually holds. So every reference a script takes
    has to be answerable here, from the source scene, or refused.

    The source scene is the right authority, not the cooked world. An object the
    port has not cooked yet still existed when the FSM was authored, so
    answering from the cooked set would turn "not built yet" into "not there",
    and a later test would quietly take the branch for absent rather than
    blocking the scene.
    """
    def __init__(self, scene):
        self.scene = scene
        self.named_objects = collections.defaultdict(list)
        for gid, go in scene.gos.items():
            self.named_objects[go['m_Name']].append(gid)
        self.object_fsms = collections.defaultdict(list)
        for typename, tree in scene.objects.values():
            if typename == 'PlayMakerFSM' and 'fsm' in tree:
                self.object_fsms[tree['m_GameObject']['m_PathID']].append(tree['fsm']['name'])

    def fsm_names(self, gid):
        """Every PlayMakerFSM component on one object, by authored name.

        Counted whatever the component's enabled flag says and whatever the port
        does with the FSM, because this answers "who would receive an event sent
        at this object", and an extra name here only ever makes the compiler
        refuse. A native actor's own FSM counts: the port running that behaviour
        in Rust does not stop the component being a second receiver in the
        source, and this question is about the source.
        """
        return self.object_fsms.get(gid, [])

    def child(self, gid, path):
        """`Transform.Find`: a direct child by name, or a slash-separated path.

        Returns 0 where the parent has no such child, which is the null the
        original stores, and None where the parent is not an object of this
        scene and the question therefore has no cook-time answer. Inactive
        children count, because `Transform.Find` finds them.
        """
        current = gid
        for part in path.split('/'):
            tid = self.scene.go_transform.get(current)
            if current not in self.scene.gos or tid is None:
                return None
            current, unread = NO_OBJECT, False
            for ref in self.scene.transforms[tid]['m_Children']:
                cid = ref['m_PathID']
                kid = (self.scene.transforms[cid]['m_GameObject']['m_PathID']
                       if cid in self.scene.transforms else 0)
                if kid not in self.scene.gos:
                    # A child this parse could not read is a child whose name
                    # is unknown, so "no such child" is not an answer here.
                    unread = True
                    continue
                # Unity returns the first match in sibling order, and m_Children
                # is that order, so a duplicated name is not ambiguous.
                if self.scene.gos[kid]['m_Name'] == part:
                    current = kid
                    break
            if not current:
                return None if unread else NO_OBJECT
        return current

    def parent(self, gid):
        """The object above one object: 0 at a root, None where unanswerable."""
        tid = self.scene.go_transform.get(gid)
        if gid not in self.scene.gos or tid is None:
            return None
        father = self.scene.transforms[tid]['m_Father']['m_PathID']
        if not father:
            return NO_OBJECT
        if father not in self.scene.transforms:
            return None
        above = self.scene.transforms[father]['m_GameObject']['m_PathID']
        return above if above in self.scene.gos else None

    def named(self, name):
        """`GameObject.Find`: the one active object with this name, or None.

        Unity searches everything loaded, which includes the persistent objects
        the port does not model at all, so "this scene holds no such object" is
        not the same answer as "there is none" and is refused rather than
        resolved to NO_OBJECT. Two active objects sharing the name are refused
        too, because Unity does not define which one it hands back.
        """
        found = [gid for gid in self.named_objects.get(name, ()) if self.scene.active(gid)]
        return found[0] if len(found) == 1 else None

    def has_trigger(self, gid):
        """Whether this object carries an enabled 2D trigger collider of its own.

        Unity raises OnTriggerEnter2D on the collider's GameObject, so an FSM
        whose own object has none never receives the callback. Six of the
        admitted scenes' `Area Title Controller` instances are exactly that: an
        authored `Trigger2dEvent` with no collider, and no Rigidbody2D to
        forward one from a child either. Compiling those would put an op in the
        bank that can never fire, which reads as an implemented trigger rather
        than an absent one.
        """
        for component in self.scene.gos.get(gid, {}).get('m_Component', ()):
            if component['component']['m_FileID']:
                continue
            typename, tree = self.scene.objects.get(component['component']['m_PathID'],
                                                    (None, None))
            if (typename or '').endswith('Collider2D') and tree['m_Enabled'] and tree['m_IsTrigger']:
                return True
        return False

    def reachable_layers(self, gid):
        """The named layers whose colliders physics can deliver to this object.

        `{'Player'}` is the compilable case: nothing but a Player-layer collider
        can be inside the volume, and in this game that layer holds the Knight's
        body. `Hero Detector` is the layer the level authors used for exactly
        that, and its matrix row admits Player and nothing else named.

        Layers the project never named are dropped. Their matrix rows keep
        Unity's all-on default because the inspector shows no row to edit for a
        layer with no name, and nothing can be assigned to one in the editor
        either: no object in the 45 admitted scenes carries a collider on one.
        """
        go = self.scene.gos.get(gid)
        if go is None:
            return None
        names, matrix = physics_layers(self.scene.source)
        mask = matrix[go['m_Layer']] & 0xffff_ffff
        return {names[i] for i in range(len(names)) if mask >> i & 1 and names[i]}

# The reference actions that store an object, and the field each stores into.
# `SetGameObject` is a reference action like the rest: it copies one object
# field into an object variable, so wherever the source resolves at cook time
# the destination is fixed too. It was the single largest disturber of the
# variables this cook can fix, which is why every one of its 195 enabled
# instances used to poison the variable it writes for the whole FSM.
OBJECT_STORE = {'GetOwner': 'storeGameObject', 'GetHero': 'storeResult',
                'FindChild': 'storeResult', 'GetParent': 'storeResult',
                'FindGameObject': 'store', 'SetGameObject': 'variable'}
# Fields that only read the object they name. PlayMaker's convention is that an
# output field is a `store...`, and the measured actions keep to it, but the
# cost of missing a write is a reference resolved from a value something else
# replaced, so this is the closed list the scenes actually use and any field not
# on it counts as a write. `GetEventSender` is why it is a list and not the
# convention: it stores the sender through `sentByGameObject`, which does not
# read like an output at all.
OBJECT_READS = frozenset({'gameObject', 'spawnPoint', 'parent', 'target', 'Target',
                          'targetObject', 'containerObject', 'objectA', 'objectB',
                          'setValue', 'eventTarget'})

def _owner_default(value):
    """Whether a decoded field is an FsmOwnerDefault, which is always a read.

    This is a rule read off the field's own type rather than another name on the
    list above, and it is the stronger statement. An FsmOwnerDefault exists to
    say *which* object an action operates on: it is `ownerOption` plus a
    GameObject, and an action with something to store uses a plain FsmGameObject
    instead, because there is nothing to store an owner option into.

    Measured over the admitted scenes before trusting it: 23,047 occurrences
    across 133 (action, field) pairs, carried by six field names, and not one of
    them is the store field of a reference action or is even named like an
    output. Three were being counted as writes for want of a name on the list,
    `gameObject1` on `BoxColliderOffset` and `BoundsBoxCollider` among them,
    which is the pair docs/SCRIPT_IR.md flagged as inputs counted as writes.
    """
    return isinstance(value, dict) and 'ownerOption' in value
# Actions that can neither transition nor block, so a run of them at the top of
# a state is guaranteed to have finished whenever that state has run at all.
# That guarantee is what makes a reference set there safe to read elsewhere.
# Anything not on this list is assumed to transition, because an action this
# slice does not model may call `Fsm.Event` in native code, and a reference read
# from a write that never happened is a wrong answer rather than a refusal.
QUIET = set(OBJECT_STORE) | {
    'SetBoolValue', 'SetIntValue', 'GetPlayerDataBool', 'GetPlayerDataInt',
    'SetPlayerDataBool', 'SetPlayerDataInt'}

def _named_variables(value):
    """Every variable a decoded action field names, however it is nested."""
    if isinstance(value, dict):
        if value.get('useVariable') and value.get('name'):
            yield value['name']
        for inner in value.values():
            yield from _named_variables(inner)
    elif isinstance(value, list):
        for inner in value:
            yield from _named_variables(inner)

def _fixed_object_variables(fsm):
    """Object variables no action outside the reference set can disturb.

    A reference action only helps if the next one can read what it stored:
    `FindChild` under `Self` is the common shape, and `Self` is whatever the
    state's opening `GetOwner` put there. So the compiler has to know which
    variables hold a value it fixed.

    Every other action in the FSM gets to disturb the answer, because an action
    this slice does not model may well be writing the variable, and a reference
    resolved from a write that something else replaced is a wrong answer rather
    than a refusal. Only a field on the read list is exempt.
    """
    stores = collections.defaultdict(list)
    disturbed = set()
    for state in fsm['states']:
        data = state['actionData']
        for i, raw in enumerate(data['actionNames']):
            if not data['actionEnabled'][i]:
                continue
            action = raw.rsplit('.', 1)[-1]
            try:
                args = action_fields(data, i, objects=True)
            except Exception:
                # An action this decoder cannot read is a mention we cannot rule
                # out, so nothing in this FSM is fixed.
                return {}
            for key, value in args.items():
                reads = key in OBJECT_READS or _owner_default(value)
                for name in _named_variables(value):
                    if key == OBJECT_STORE.get(action):
                        stores[name].append(action)
                    elif not reads:
                        disturbed.add(name)
    # Two stores disagree unless both are the constants: the owner and the hero
    # answer the same everywhere in one FSM, so repeating them is harmless.
    return {name: acts for name, acts in stores.items()
            if name not in disturbed
            and (len(acts) == 1 or set(acts) <= {'GetOwner', 'GetHero'})}

def _reference(action, args, owner_id, objects, visible, folded, fixed):
    """Note what a reference action just fixed, for the actions that follow it.

    Within one state this is sound wherever the store sits. PlayMaker runs a
    state's actions in declaration order and abandons the rest the moment one
    transitions, so an action that runs at all ran after every action before it.
    """
    store = OBJECT_STORE.get(action)
    if store is None:
        return
    field = args.get(store)
    name = field.get('name') if isinstance(field, dict) and field.get('useVariable') else None
    if not name or name not in fixed:
        return
    try:
        visible[name] = _resolve_object(action, args, owner_id, objects, visible, folded)
    except Exception:
        visible.pop(name, None)

def _state_graph(fsm):
    """The FSM's transition graph, from the state it actually starts in.

    Only an authored transition moves a PlayMaker FSM between states, and
    `compile_fsm` refuses a definition with a global transition before anything
    here runs, so these edges are the whole of the control flow.
    """
    names = [s['name'] for s in fsm['states']]
    index_of = {name: i for i, name in enumerate(names)}
    succ = [[index_of[t['toState']] for t in s['transitions'] if t['toState'] in index_of]
            for s in fsm['states']]
    start = index_of.get(fsm['startState'], 0)
    reachable, stack = set(), [start]
    while stack:
        node = stack.pop()
        if node in reachable:
            continue
        reachable.add(node)
        stack.extend(succ[node])
    return start, succ, reachable

def _dominators(start, succ, reachable):
    """For each reachable state, the states every path from the start goes through.

    A state nothing reaches gets no entry, so a read inside it resolves nothing
    rather than inheriting the whole FSM the way an unreachable node does under
    the usual all-nodes initialisation.
    """
    preds = collections.defaultdict(list)
    for node in reachable:
        for target in succ[node]:
            if target in reachable:
                preds[target].append(node)
    dom = {node: ({start} if node == start else set(reachable)) for node in reachable}
    changing = True
    while changing:
        changing = False
        for node in reachable:
            if node == start:
                continue
            incoming = [dom[p] for p in preds[node]]
            found = {node} | (set.intersection(*incoming) if incoming else set())
            if found != dom[node]:
                dom[node] = found
                changing = True
    return dom

def _entry_writes(fsm, owner_id, objects, folded, fixed, env):
    """What each state has certainly stored by the time it can leave.

    The run of actions at the top of a state, up to the first that could
    transition or block, has run whenever that state has run at all. Each state
    starts from what it can already read on entry, so a `FindChild` under a
    `Self` that a dominating state set resolves here too.
    """
    out = []
    for index, state in enumerate(fsm['states']):
        here = dict(env[index])
        data = state['actionData']
        for i, raw in enumerate(data['actionNames']):
            if not data['actionEnabled'][i]:
                continue
            action = raw.rsplit('.', 1)[-1]
            if action not in QUIET:
                break
            try:
                _reference(action, decoded_action(data, i), owner_id, objects,
                           here, folded, fixed)
            except Exception:
                break
        out.append(here)
    return out

def _object_env(fsm, owner_id, objects, folded):
    """The variables this cook can fix, and what each state can read on entry.

    Reading a reference stored in another state needs more than declaration
    order: the write has to be one every path has already taken. The start
    state's opening run is the simplest case of that, and used to be the only
    one. The general case is any state that *dominates* the reading state, which
    the FSM's own transition graph answers, and it is worth having: measured over
    the admitted scenes, 1,131 object fields are written somewhere the narrow
    rule could not see, against 1,579 the narrow rule already resolved.

    A state does not read its own entry writes from here. An action sitting
    before the store still has to see nothing, and the in-order pass inside the
    state is what gets that right.

    Returns (fixed, env): the variables no other action disturbs, and one entry
    environment per state.
    """
    fixed = _fixed_object_variables(fsm)
    if not fsm['states']:
        return fixed, []
    start, succ, reachable = _state_graph(fsm)
    dom = _dominators(start, succ, reachable)
    env = [{} for _ in fsm['states']]
    # A dominating state's own entry writes may themselves depend on what it
    # could read, so this settles rather than resolving in one pass. It only
    # ever adds, and the state count bounds it.
    for _round in range(len(fsm['states']) + 1):
        writes = _entry_writes(fsm, owner_id, objects, folded, fixed, env)
        changed = False
        for index in range(len(fsm['states'])):
            here = {}
            for other in sorted(dom.get(index, ())):
                if other == index:
                    continue
                for name, found in writes[other].items():
                    if here.setdefault(name, found) != found:
                        # Two dominating writes disagree, so which one reached
                        # this state depends on the path. `_fixed_object_variables`
                        # admits a variable stored only by `GetOwner` and
                        # `GetHero`, and those are different objects, so this is
                        # reachable rather than theoretical.
                        here[name] = None
            here = {name: value for name, value in here.items() if value is not None}
            if here != env[index]:
                env[index] = here
                changed = True
        if not changed:
            break
    return fixed, env

def _variable_values(fsm):
    """Declared variables to their serialized initial values."""
    out = {}
    for group in fsm['variables'].values():
        if not isinstance(group, list):
            continue
        for v in group:
            if isinstance(v, dict) and 'name' in v:
                out.setdefault(v['name'], v.get('value'))
    return out

def _variable_mentions(fsm):
    """How many enabled actions mention each variable by name.

    A variable mentioned exactly once is only read, never written, so its
    serialized initial value is constant for the run and can be folded. That is
    the safety condition: a second mention might be the write.
    """
    counts = collections.Counter()
    for state in fsm['states']:
        data = state['actionData']
        for i in range(len(data['actionNames'])):
            if not data['actionEnabled'][i]:
                continue
            seen = set()
            try:
                decoded = action_fields(data, i, objects=True)
            except Exception:
                # An action this decoder cannot read is a mention we cannot
                # rule out, so nothing in it folds.
                decoded = {}
                counts.clear()
                return counts
            for value in decoded.values():
                if isinstance(value, dict) and value.get('useVariable') and value.get('name'):
                    seen.add(value['name'])
            for name in seen:
                counts[name] += 1
    return counts

def _variables(fsm):
    """Variable name to slot index, in declaration order."""
    return {name: i for i, name in enumerate(_variable_values(fsm))}

def compile_fsm(fsm, owner_id=None, objects=None):
    """One FSM definition to (states, transitions, ops, constants, events).

    `owner_id` is the owning GameObject's source path id and `objects` a
    SceneObjects over the scene it sits in; without them every reference action
    is refused rather than guessed at.

    Variables are a span in the caller's pool, so there is no slot cap here.
    Raises Unsupported on the first action this slice does not implement.
    """
    if fsm['globalTransitions']:
        # A global transition fires from any state, so dropping one turns an
        # event that leaves the FSM into an event that goes nowhere: a silent
        # wrong answer with no symptom at the seam. No FSM that compiles today
        # has one, which is why this refuses rather than models them.
        raise Unsupported('<global transitions>', 'the definition has global transitions')
    events = Events()
    constants = Constants()
    fields = Fields()
    variables = _variables(fsm)
    mentions = _variable_mentions(fsm)
    folded = {name: value for name, value in _variable_values(fsm).items()
              if mentions[name] == 1}
    fixed, env = _object_env(fsm, owner_id, objects, folded)
    names = [s['name'] for s in fsm['states']]
    index_of = {name: i for i, name in enumerate(names)}
    states, transitions, ops = [], [], []
    for index, state in enumerate(fsm['states']):
        first_op, first_transition = len(ops), len(transitions)
        for t in state['transitions']:
            target = t['toState']
            if target not in index_of:
                # PlayMaker allows a transition with no target; it is a no-op.
                continue
            transitions.append((events.id(t['fsmEvent']['name']), index_of[target]))
        # Each state earns its references as it runs, on top of what the states
        # that dominate it pinned on the way in, so an action that reads one
        # before it is set is refused rather than handed a value stored later.
        visible = dict(env[index])
        data = state['actionData']
        for i, raw in enumerate(data['actionNames']):
            if not data['actionEnabled'][i]:
                continue
            action = raw.rsplit('.', 1)[-1]
            args = decoded_action(data, i)
            ops.extend(compile_action(action, args, events, constants, variables, owner_id,
                                      fields, folded, objects, visible, fsm['name']))
            _reference(action, args, owner_id, objects, visible, folded, fixed)
        states.append((first_op, len(ops) - first_op, first_transition,
                       len(transitions) - first_transition))
    return {'states': states, 'transitions': transitions, 'ops': ops,
            'constants': constants.values, 'events': events.ids,
            'state_names': names, 'variables': variables, 'var_count': len(variables),
            'player_data_fields': fields.names}

def _folded_scalar(value, folded):
    """A scalar, reading through a read-only variable's initial value."""
    plain = _scalar(value)
    if plain is not None:
        return plain
    if folded and isinstance(value, dict):
        return folded.get(value.get('name'))
    return None

def _scalar(value):
    """A compact FSM scalar, or None when it is a variable reference."""
    if isinstance(value, dict) and 'value' in value and 'useVariable' in value:
        return None if value['useVariable'] else value['value']
    return value

def _var(value, variables, action):
    """The slot a field refers to, refusing an unbound or literal reference."""
    if not isinstance(value, dict) or not value.get('useVariable'):
        raise Unsupported(action, 'literal where a variable slot is needed')
    slot = variables.get(value['name'])
    if slot is None:
        raise Unsupported(action, f"unknown variable {value['name']!r}")
    return slot

def _object(field, action, owner_id, env):
    """An FsmOwnerDefault or FsmGameObject field as a source path id.

    Refuses rather than defaulting: the owner is the only implied object, and a
    variable is only an answer where this cook fixed what it holds.
    """
    if isinstance(field, dict) and 'ownerOption' in field:
        if not field['ownerOption']:
            if owner_id is None:
                raise Unsupported(action, 'no stable owner id supplied')
            return owner_id
        field = field.get('gameObject')
    if not isinstance(field, dict):
        raise Unsupported(action, 'unreadable object field')
    if field.get('useVariable'):
        name = field.get('name')
        if env and name in env:
            return env[name]
        raise Unsupported(action, 'object from a variable this cook cannot fix')
    pointer = field.get('value')
    if not isinstance(pointer, dict):
        raise Unsupported(action, 'unreadable object field')
    if pointer.get('m_FileID'):
        raise Unsupported(action, 'object in another file')
    if not pointer.get('m_PathID'):
        raise Unsupported(action, 'unset object field')
    return pointer['m_PathID']

def _resolve_object(action, args, owner_id, objects, env, folded):
    """The object one reference action stores, as a source path id.

    This is the whole of the cook-time rule docs/SCRIPT_IR.md states: a
    reference resolves where the target is static and is refused where it is
    not. Nothing here reaches the guest as a lookup; what reaches it is the id.
    """
    if action == 'GetOwner':
        # The owner of an FSM never changes.
        if owner_id is None:
            raise Unsupported(action, 'no stable owner id supplied')
        return owner_id
    if action == 'GetHero':
        # The Knight is the reserved reference, so this is the constant.
        return HERO
    if action == 'FindGameObject':
        name = _folded_scalar(args.get('objectName'), folded)
        tag = _folded_scalar(args.get('withTag'), folded)
        if not isinstance(name, str) or not isinstance(tag, str):
            raise Unsupported(action, 'name or tag from a variable this cook cannot fix')
        if not name:
            # The Knight is the only object in the game with the Player tag, and
            # the port answers for it natively. Every other tag names a
            # persistent object the port has no model of, and the parsed
            # GameObject carries a tag index rather than the string anyway, so
            # there is nothing to match it against.
            if tag == 'Player':
                return HERO
            raise Unsupported(action, f'lookup by tag {tag!r}')
        if tag not in ('', 'Untagged'):
            raise Unsupported(action, f'lookup by name and tag {tag!r}')
        if objects is None:
            raise Unsupported(action, 'no scene resolver supplied')
        found = objects.named(name)
        if found is None:
            raise Unsupported(action, 'name is not one active object of this scene')
        return found
    target = _object(args.get('gameObject'), action, owner_id, env)
    if action == 'SetGameObject':
        # A copy, so the source is the answer. No tree walk and no resolver
        # needed: whatever `_object` could name, this stores.
        return target
    if objects is None:
        raise Unsupported(action, 'no scene resolver supplied')
    if action == 'FindChild':
        child = _folded_scalar(args.get('childName'), folded)
        if not isinstance(child, str) or not child:
            raise Unsupported(action, 'child name from a variable this cook cannot fix')
        found = objects.child(target, child)
    else:
        found = objects.parent(target)
    if found is None:
        # The hero and anything outside this scene land here: the port's object
        # model is regions, region objects and actors, not a Unity scene graph,
        # so there is no tree to walk from them.
        raise Unsupported(action, 'target is not an object of this scene')
    return found

def _filter(field, action, folded, what):
    """A tag or layer filter as a string, reading PlayMaker's None as no filter.

    A NamedVariable with `useVariable` set and an empty name is PlayMaker's
    None: the field is unbound and the action reads the empty string, which for
    these two means "match anything". 816 of the 1,026 `Trigger2dEvent`
    instances carry the tag in that shape, and treating it as a variable the
    cook cannot fix would refuse them all for the wrong reason.
    """
    if isinstance(field, dict) and field.get('useVariable') and not field.get('name'):
        return ''
    value = _folded_scalar(field, folded)
    if not isinstance(value, str):
        raise Unsupported(action, f'{what} from a variable this cook cannot fix')
    return value

def _trigger(action, args, events, constants, owner_id, folded, objects):
    """`Trigger2dEvent` as a HeroTrigger op, or a refusal naming what is in the way.

    The port can answer one question about a trigger volume: is the hero body
    inside it. That is the question npc_control's talk range and the bench
    already put to a cooked object's bounds. Everything else an instance can
    name, an enemy, a nail swing, a spell, a breaker, is a collider the guest
    has no model of, and an op that tested the hero in its place would fire for
    the wrong body rather than not at all.
    """
    if owner_id is None:
        raise Unsupported(action, 'no stable owner id supplied')
    if objects is None:
        raise Unsupported(action, 'no scene resolver supplied')
    phase = args.get('trigger')
    if phase not in (TRIGGER_ENTER, TRIGGER_STAY, TRIGGER_EXIT):
        raise Unsupported(action, 'unreadable trigger type')
    # What the volume watches for is asked before how the instance books the
    # result, so an instance that is out of reach for both reasons is reported
    # by the one that would still be true after the other is implemented.
    layer = _filter(args.get('collideLayer'), action, folded, 'collide layer')
    if layer:
        raise Unsupported(action, f'layer filter {layer!r}')
    tag = _filter(args.get('collideTag'), action, folded, 'collide tag')
    if tag and tag != 'Player':
        # HeroBox, Nail Attack, Dream Attack, Hero Spell and Wall Breaker all
        # name a child collider of the Knight rather than the Knight, and each
        # is a different shape from the body, so answering them with the body
        # would move where the trigger fires rather than refuse to place it.
        raise Unsupported(action, f'collide tag {tag!r}')
    reachable = objects.reachable_layers(owner_id)
    if reachable is None:
        raise Unsupported(action, 'the FSM object is not an object of this scene')
    if 'Player' not in reachable:
        # Said of the body rather than of the hero, because an Attack-layer
        # volume does see the Knight, through the HeroBox child the port has no
        # collider for.
        raise Unsupported(action, 'physics delivers no hero-body collider to this volume')
    if not tag and reachable != {'Player'}:
        raise Unsupported(action, 'any collider on ' + ', '.join(sorted(reachable - {'Player'})))
    if not objects.has_trigger(owner_id):
        raise Unsupported(action, 'the FSM object carries no trigger collider')
    store = args.get('storeCollider')
    if isinstance(store, dict) and store.get('useVariable') and store.get('name'):
        # PlayMaker stores the colliding object before it sends, so the write
        # belongs inside the op rather than beside it: emitted as its own op it
        # would leave the Knight in that variable on every tick the volume is
        # empty.
        #
        # This used to say the op had no operand to carry the slot, which is not
        # true: `HeroTrigger` reads `a` and `c` and leaves `b` at zero, and the
        # guest could take `b` as slot-plus-one the way an event id takes zero
        # for none. What stops it is worth less than the operand: 49 blocked
        # instances name a storeCollider and not one of them is blocked by
        # anything else this cook could answer, so the work moves the action
        # count and no FSM at all. Left refused rather than left misdescribed.
        raise Unsupported(action, 'stores the colliding object')
    event = events.id(args.get('sendEvent'))
    if not event:
        # An authored trigger with no event does nothing in PlayMaker either,
        # but a compiled op that queries a volume and sends nowhere is
        # indistinguishable from an unimplemented one in a trace, so it refuses
        # by name instead.
        raise Unsupported(action, 'no event to send')
    # Always REPEAT: the action listens for as long as its state is current.
    return [(HERO_TRIGGER, phase | REPEAT, constants.index(source_id(owner_id)), 0, event)]

def _sends_to_self(action, args, owner_id, objects, env, folded, fsm_name):
    """Whether this send lands exactly where a send to `Self` lands.

    PlayMaker names the sending FSM in more than one way, and the executor
    already implements the meaning; what was missing is the cook-time step that
    recognises the other spellings. `Self` is one. A send to `GameObject`
    delivers to every PlayMakerFSM on that object, so where the object is the
    sender's own and carries no other FSM, the two name the same single
    receiver. A send to `GameObjectFSM` names one FSM on one object, so it is
    the sender when both halves match.

    Anything else is refused by what is actually in the way, which is worth more
    to the inventory than one blanket reason: a target this cook cannot name is
    a different problem from a target it named and found to be somebody else.
    """
    target = args.get('eventTarget')
    mode = target.get('target') if isinstance(target, dict) else None
    if mode in (None, TARGET_SELF):
        return
    if mode not in (TARGET_GAME_OBJECT, TARGET_GAME_OBJECT_FSM):
        # Broadcast, the host FSM and the sub-FSMs. Each names a receiver set
        # the cook cannot close, so none of them reduces to a send to self.
        raise Unsupported(action, 'send to a target this slice does not model')
    try:
        found = _object(target.get('gameObject'), action, owner_id, env)
    except Unsupported:
        raise Unsupported(action, 'send to an object this cook cannot fix')
    if found != owner_id:
        raise Unsupported(action, 'send to another object')
    named = _folded_scalar(target.get('fsmName'), folded)
    if mode == TARGET_GAME_OBJECT_FSM:
        if not isinstance(named, str) or not named:
            raise Unsupported(action, 'send to an FSM name this cook cannot fix')
        if named != fsm_name:
            raise Unsupported(action, 'send to another FSM on this object')
        return
    if objects is None:
        raise Unsupported(action, 'no scene resolver supplied')
    others = objects.fsm_names(owner_id)
    if len(others) != 1:
        # The event would reach every FSM on the object, and the port has no
        # instance table to reach the rest with. Refusing names the receiver
        # count, because that is the thing that would have to change.
        raise Unsupported(action, f'send to all {len(others)} FSMs on this object')

def compile_action(action, args, events, constants, variables, owner_id=None, fields=None,
                   folded=None, objects=None, env=None, fsm_name=None):
    """Ops for one enabled action, or Unsupported."""
    if action in OBJECT_STORE:
        # A resolved reference is a constant, so an `everyFrame` one would store
        # the same id again; only a write from elsewhere could make that differ,
        # and _fixed_object_variables refuses the variable when one exists.
        slot = _var(args.get(OBJECT_STORE[action]), variables, action)
        found = _resolve_object(action, args, owner_id, objects, env, folded)
        return [(SET_INT, 0, slot, constants.index(source_id(found)), 0)]
    if action in ('PlayerDataBoolTest', 'GetPlayerDataBool', 'SetPlayerDataBool',
                  'GetPlayerDataInt', 'SetPlayerDataInt'):
        name = _folded_scalar(args.get('boolName') if 'Bool' in action else args.get('intName'),
                              folded)
        if not isinstance(name, str) or not name:
            raise Unsupported(action, 'variable field name')
        field = fields.index(name)
        if action == 'PlayerDataBoolTest':
            return [(PD_BOOL_TEST, 0, field, events.id(args.get('isTrue')),
                     events.id(args.get('isFalse')))]
        if action.startswith('Get'):
            slot = _var(args.get('storeValue'), variables, action)
            return [(PD_GET, 0, slot, field, 0)]
        value = args.get('value')
        if _scalar(value) is not None:
            # A literal write goes through a constant and a compiler temporary,
            # which the pooled variables make free.
            temp = variables.setdefault(f'<{action}:{field}>', len(variables))
            return [(SET_INT, 0, temp, constants.index(int(_scalar(value))), 0),
                    (PD_SET, 0, field, temp, 0)]
        return [(PD_SET, 0, field, _var(value, variables, action), 0)]
    if action == 'Trigger2dEvent':
        return _trigger(action, args, events, constants, owner_id, folded, objects)
    if action == 'Wait':
        # Read through a read-only variable, the way the object, tag and event
        # name fields already are. A variable no other action in the FSM
        # mentions cannot have been written, so its serialized value is the
        # whole of its run: 235 blocked instances were refusing a wait whose
        # length the cook could read off the definition.
        time = _folded_scalar(args.get('time'), folded)
        if time is None:
            raise Unsupported(action, 'variable wait time')
        if args.get('realTime'):
            raise Unsupported(action, 'real-time wait')
        return [(WAIT, 0, max(1, round(float(time) * 60)), events.id(args.get('finishEvent')), 0)]
    if action == 'NextFrameEvent':
        return [(NEXT_FRAME, 0, events.id(args.get('sendEvent')), 0, 0)]
    if action == 'SendEventByName':
        # Only a send that reaches the sending FSM and nothing else is in this
        # slice; reaching another FSM needs the instance table that step 5's
        # scope work builds.
        _sends_to_self(action, args, owner_id, objects, env, folded, fsm_name)
        delay = args.get('delay')
        if delay is not None:
            # A delayed send goes on PlayMaker's own queue and arrives later.
            # The executor has a pending queue but no op that schedules into it,
            # so a delay was being dropped and the send fired on the wrong
            # frame: six instances authored 0.5, 1.0 and 4.0 seconds.
            seconds = _folded_scalar(delay, folded)
            if seconds is None:
                raise Unsupported(action, 'delay from a variable this cook cannot fix')
            if seconds:
                raise Unsupported(action, 'delayed send')
        # This action names its event with an FsmString, not the interned
        # FsmEvent the rest carry, so it can be a variable and has to be read as
        # one. Handing the raw field to the event table instead raised TypeError,
        # which the survey counted as an ordinary refusal: every send to self
        # measured as uncompilable and none had ever been emitted.
        name = _folded_scalar(args.get('sendEvent'), folded)
        if not isinstance(name, str) or not name:
            raise Unsupported(action, 'event name from a variable this cook cannot fix')
        # An `everyFrame` send re-sends for as long as the state is current,
        # which is what REPEAT means, and the executor already runs flagged ops
        # that way. One instance is authored so, and it was compiling to a
        # single send.
        return [(SEND_EVENT, REPEAT if args.get('everyFrame') else 0,
                 events.id(name), 0, 0)]
    if action == 'SetBoolValue':
        value = _folded_scalar(args.get('boolValue'), folded)
        if value is None:
            raise Unsupported(action, 'variable source')
        return [(SET_BOOL, int(bool(value)), _var(args.get('boolVariable'), variables, action), 0, 0)]
    if action == 'SetIntValue':
        value = _folded_scalar(args.get('intValue'), folded)
        if value is None:
            raise Unsupported(action, 'variable source')
        return [(SET_INT, 0, _var(args.get('intVariable'), variables, action),
                 constants.index(value), 0)]
    if action == 'BoolTest':
        return [(BOOL_TEST, 0, _var(args.get('boolVariable'), variables, action),
                 events.id(args.get('isTrue')), events.id(args.get('isFalse')))]
    if action == 'IntCompare':
        slot = _var(args.get('integer1'), variables, action)
        right = _folded_scalar(args.get('integer2'), folded)
        if right is None:
            raise Unsupported(action, 'variable right-hand side')
        index = constants.index(right)
        out = []
        for key, selector in (('lessThan', CMP_LT), ('equal', CMP_EQ), ('greaterThan', CMP_GT)):
            event = events.id(args.get(key))
            if event:
                out.append((INT_COMPARE, selector, slot, index, event))
        return out
    raise Unsupported(action)

def enabled_actions(fsm):
    """Every enabled action in the definition, by name, repeats included."""
    for state in fsm['states']:
        data = state['actionData']
        for i, raw in enumerate(data['actionNames']):
            if data['actionEnabled'][i]:
                yield raw.rsplit('.', 1)[-1]

def unsupported_actions(fsm, owner_id=None, objects=None, reasons=None):
    """Every action in this FSM the slice cannot compile, counted per instance.

    Ranking by the first refusal is a treadmill: an FSM compiles only when all
    of its actions do, so what matters is the whole set an FSM is missing. The
    counts are what says whether work on an action moved anything, because a
    definition can go from refusing an action everywhere to refusing it once
    without its instance count changing at all.

    A Counter passed as `reasons` also collects `(action, detail)`, which is
    what separates an action nothing has implemented yet from one this cook
    could not answer for in these particular scenes.
    """
    events, constants, fields = Events(), Constants(), Fields()
    variables = _variables(fsm)
    mentions = _variable_mentions(fsm)
    folded = {name: value for name, value in _variable_values(fsm).items()
              if mentions[name] == 1}
    fixed, env = _object_env(fsm, owner_id, objects, folded)
    missing = collections.Counter()
    for index, state in enumerate(fsm['states']):
        visible = dict(env[index])
        data = state['actionData']
        for i, raw in enumerate(data['actionNames']):
            if not data['actionEnabled'][i]:
                continue
            action = raw.rsplit('.', 1)[-1]
            try:
                args = decoded_action(data, i)
                compile_action(action, args, events, constants, variables, owner_id, fields,
                               folded, objects, visible, fsm['name'])
            except Unsupported as refusal:
                missing[refusal.action] += 1
                if reasons is not None:
                    reasons[(refusal.action, refusal.detail)] += 1
            except Exception:
                missing[action] += 1
                if reasons is not None:
                    reasons[(action, 'the decoder could not read this action')] += 1
                continue
            _reference(action, args, owner_id, objects, visible, folded, fixed)
    return missing

def scene_fsms(source, name):
    """The scene and its FSMs as (path id, owner path id, definition, runs_natively).

    The scene comes back with them because the compiler needs it: a reference
    action resolves against the transform tree, and parsing the scene twice to
    get at it would double the expensive step.

    An FSM whose GameObject already carries a supported native actor controller
    is the PlayMaker side of behaviour the port implements in Rust, not script
    work: the 35 `flyer_receive_direction_msg` instances on driven flyers are
    the recoil those controllers already apply. Counting them as blocked
    overstates what is left to build.

    The test is the actor's own GameObject, not the enemy it talks about. The
    112 `enemy_message` instances look like the damage path `ActorHealth` owns,
    but every one of them sits on a detector or trigger volume with no
    HealthManager, so they stay script work.

    Both halves come out of one Scene, because parsing a scene reads every
    object's typetree and is by far the expensive step here. Reading the FSMs
    back off that same parse is why this does not touch the file twice.
    """
    from scene import Scene
    from actors import actor_sources
    sc = Scene(source, name)
    native = {a['game_object'] for a in actor_sources(sc) if a.get('movement_supported')}
    return sc, [(path_id, tree['m_GameObject']['m_PathID'], tree['fsm'],
                 tree['m_GameObject']['m_PathID'] in native)
                for path_id, (typename, tree) in sc.objects.items()
                if typename == 'PlayMakerFSM' and 'fsm' in tree]

def survey(source, scene_files):
    """How much of the admitted scenes' authored behaviour this slice compiles."""
    compiled = collections.Counter()
    blockers = collections.Counter()
    blocking_fsms = collections.Counter()
    native_fsms = collections.Counter()
    # Instances keyed by the exact set of actions they still need, so the
    # ranking answers "what would make whole FSMs run" rather than "what
    # refuses first".
    missing_sets = collections.Counter()
    action_unlocks = collections.Counter()
    unlock_fsms = collections.defaultdict(collections.Counter)
    # Why each blocked instance refused, which is what says whether an action
    # needs an opcode, a better cook-time answer, or a capability the port has
    # not got.
    reasons = collections.Counter()
    # Per action instance, not per definition. An FSM compiles only when all of
    # its actions do, so the definition count moves in steps and can sit still
    # through work that really did land; this is the measure that does not.
    seen_actions = collections.Counter()
    refused_actions = collections.Counter()
    refusal_details = collections.Counter()
    unparsed = {}
    for name in scene_files:
        try:
            scene, fsms = scene_fsms(source, name)
            objects = SceneObjects(scene)
        except Exception as error:
            # Without a parse there is no native-actor set, so counting this
            # scene's FSMs as script work would put back exactly the
            # overstatement this measurement exists to remove. Name the scene
            # instead, so a silent drop cannot be mistaken for coverage.
            unparsed[name] = str(error)
            continue
        for path_id, owner_id, fsm, runs_natively in fsms:
            if runs_natively:
                compiled['on_native_actors'] += 1
                native_fsms[fsm['name']] += 1
                continue
            compiled['instances'] += 1
            seen_actions.update(enabled_actions(fsm))
            # The owner is the FSM's GameObject, not its component: that is the
            # object a reference action walks from and the id the world metadata
            # bank carries.
            try:
                program = compile_fsm(fsm, owner_id=owner_id, objects=objects)
            except Unsupported as refusal:
                blockers[refusal.action] += 1
                blocking_fsms[fsm['name']] += 1
                reasons[(refusal.action, refusal.detail)] += 1
                missing = unsupported_actions(fsm, owner_id=owner_id, objects=objects,
                                              reasons=refusal_details)
                refused_actions.update(missing)
                missing_sets[tuple(sorted(missing))] += 1
                if len(missing) == 1:
                    action = next(iter(missing))
                    action_unlocks[action] += 1
                    unlock_fsms[action][fsm['name']] += 1
                continue
            compiled['compiled'] += 1
            compiled['ops'] += len(program['ops'])
    return {'instances': compiled['instances'], 'compiled': compiled['compiled'],
            'compiled_ops': compiled['ops'],
            'action_instances': sum(seen_actions.values()),
            'compiled_action_instances': sum(seen_actions.values()) - sum(refused_actions.values()),
            'per_action': [{'action': a, 'instances': n, 'refused': refused_actions[a],
                            'compiled': n - refused_actions[a]}
                           for a, n in seen_actions.most_common()],
            'on_native_actors': compiled['on_native_actors'],
            'native_actor_fsms': dict(native_fsms.most_common(20)),
            'unparsed_scenes': unparsed,
            'first_blocking_action': dict(blockers.most_common(25)),
            'first_refusal_reason': [{'action': a, 'reason': d, 'instances': n}
                                     for (a, d), n in reasons.most_common(25)],
            'refusal_reason_instances': [{'action': a, 'reason': d, 'instances': n}
                                         for (a, d), n in refusal_details.most_common()],
            'blocked_fsms': dict(blocking_fsms.most_common(20)),
            'one_action_from_compiling': [
                {'action': action, 'instances': n, 'fsms': dict(unlock_fsms[action].most_common(8))}
                for action, n in action_unlocks.most_common(20)],
            'missing_sets': [{'actions': list(k), 'instances': n}
                             for k, n in missing_sets.most_common(15)]}

if __name__ == '__main__':
    import json
    from source import Source, ROOT, dump
    s = Source()
    scenes = {e['file']: e['scene_name']
              for e in json.load(open(ROOT / '.hkpsx/selected-regions.json'))['scenes']}
    out = survey(s, scenes)
    out['limitations'] = [
        'Only the first slice compiles: waits, bool and int sets, BoolTest, IntCompare, '
        'SendEventByName to self, NextFrameEvent, the PlayerData accessors, the '
        'cook-time object references (GetOwner, GetHero, FindChild, GetParent, '
        'FindGameObject, SetGameObject) and the hero half of Trigger2dEvent.',
        'A scalar field reads through a variable no other action in the FSM mentions, '
        'because nothing can then have written it and its serialized value is the whole '
        'of its run. That is how an object name, a tag and an event name were already '
        'read, and Wait, SetBoolValue, SetIntValue and IntCompare now read the same way.',
        'SendEventByName compiles every spelling that reaches the sending FSM and nothing '
        'else: PlayMaker\'s Self, a send to a GameObject that resolves to the sender\'s '
        'own and carries no other FSM, and a send to GameObjectFSM naming this FSM on it. '
        'A send that would reach a second FSM is refused with the receiver count, because '
        'the receiver is what would have to change. A non-zero delay is refused rather '
        'than dropped: the executor has a pending queue but no op that schedules into it.',
        'Trigger2dEvent compiles only where the hero body is the one collider that can '
        'be in the volume: either the action filters on the Player tag, or it filters on '
        'nothing and the project Physics2D matrix lets no named layer but Player reach '
        'the owner, which is what the Hero Detector layer means. An instance naming '
        'HeroBox, Nail Attack, Dream Attack, Hero Spell or Wall Breaker is refused '
        'because each is a child collider of the Knight with its own shape, and one '
        'naming an Enemies layer or an Enemy Detector volume is refused because the '
        'guest has no collider for what it is watching for. An owner with no enabled '
        'trigger collider of its own is refused too: Unity would never raise the '
        'callback, so the op could only ever be a trigger that silently never fires.',
        'A reference compiles only where its target is fixed at cook time. FindChild and '
        'GetParent walk the source scene from the owner, a literal pointer or a variable '
        'the start state sets before it can transition; FindGameObject answers only for '
        'the Player tag, which is the Knight, or for a name exactly one active object of '
        'the scene carries. Everything else is refused rather than defaulted.',
        'ActivateGameObject and ActivateAllChildren stay refused on purpose. A cooked '
        'object has no runtime active flag: its state_id is a persistence key and nothing '
        'in the renderer consults one, so emitting these would be a no-op at runtime, '
        'which is exactly the silent implementation claim step 7 forbids.',
        'FSMs on GameObjects a supported native actor controller already runs are counted '
        'under on_native_actors and left out of the instances denominator, because their '
        'behaviour is already implemented in Rust rather than waiting on the script slice.',
        'An actor counts as native only where actors.py reports movement_supported and only '
        'on its own GameObject, so an enemy the port draws but does not yet drive, and a '
        'detector or trigger volume that only messages one, both stay script work.',
        'first_blocking_action and first_refusal_reason count each blocked FSM once, by '
        'the action that refuses first; one_action_from_compiling and missing_sets use '
        'the whole set an FSM is missing and are the ones to rank by.',
        'The reference analysis errs towards refusing. A variable is only fixed where no '
        'action outside the reference set mentions it, so an FSM already blocked by an '
        'unmodelled action can have its references refused as well, and appear in '
        'missing_sets with one more action than it truly needs. That costs nothing on the '
        'compiled count, because no other compiling action takes an object.',
        'compiled counts whole FSM instances, which move only when every action in a '
        'definition compiles; per_action and compiled_action_instances count action '
        'instances and are what show whether work on one action landed. Neither is a '
        'runtime claim: a compiled FSM is one the cooker can emit, not one that has been '
        'replayed against the original.',
        'Nothing is cooked into the disc yet: this measures the compiler, it does not '
        'ship a script bank.',
    ]
    dump(ROOT / '.hkpsx/script-ir-coverage.json', out)
    print(f"{out['compiled']} of {out['instances']} script-side FSM instances compile "
          f"({out['compiled'] / max(1, out['instances']):.1%}), {out['compiled_ops']} ops; "
          f"{out['on_native_actors']} further instances sit on native actor controllers")
    print(f"{out['compiled_action_instances']} of {out['action_instances']} action instances "
          f"compile ({out['compiled_action_instances'] / max(1, out['action_instances']):.1%}); "
          f"an FSM needs all of its own before it runs at all")
    if out['unparsed_scenes']:
        print(f"{len(out['unparsed_scenes'])} scenes did not parse and are not measured: "
              f"{', '.join(out['unparsed_scenes'])}")
    print('one action away from compiling:')
    for row in out['one_action_from_compiling'][:12]:
        print(f"  {row['action']:32} would complete {row['instances']:4} instances  "
              f"({', '.join(row['fsms'])[:60]})")
    print('most common missing sets:')
    for row in out['missing_sets'][:8]:
        print(f"  {row['instances']:4}  {', '.join(row['actions'])[:90]}")
    print('reference actions, by instance:')
    for row in out['per_action']:
        if row['action'] in OBJECT_STORE:
            print(f"  {row['action']:20} {row['compiled']:4} of {row['instances']:4} compile")
            for reason in out['refusal_reason_instances']:
                if reason['action'] == row['action']:
                    print(f"      {reason['instances']:4} {reason['reason']}")
    print('first refusal reasons:')
    for row in out['first_refusal_reason'][:12]:
        print(f"  {row['instances']:4}  {row['action']}: {row['reason'] or 'no rule for this action'}")
