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
import collections
from focus import action_fields

# Opcode numbers, in the declaration order of hk_sim::script::Code. Every
# variant is named even where nothing emits it yet, because a gap here shifts
# every opcode below it: omitting NativeConst silently aimed the three
# PlayerData opcodes one slot low.
(NOP, WAIT, SET_BOOL, SET_INT, BOOL_TEST, INT_COMPARE, SEND_EVENT, NEXT_FRAME, NATIVE,
 NATIVE_CONST, PD_GET, PD_SET, PD_BOOL_TEST) = range(13)

def _check_opcodes():
    """Fail the import when these numbers drift from hk_sim::script::Code.

    They already did once: `NativeConst` was added to the Rust enum and not
    here, which silently shifted all three PlayerData opcodes one slot low.
    Nothing had cooked a bank yet, so nothing caught it. Reading the enum is
    cheaper than the next silent miscompile.
    """
    from source import ROOT
    text = (ROOT / 'shared/hk-sim/src/script.rs').read_text()
    body = text[text.index('pub enum Code {'):]
    body = body[:body.index('\n}')]
    variants = [line.strip().rstrip(',') for line in body.splitlines()[1:]
                if line.startswith('    ') and line.strip()[:1].isupper()]
    expected = ['Nop', 'Wait', 'SetBool', 'SetInt', 'BoolTest', 'IntCompare', 'SendEvent',
                'NextFrameEvent', 'Native', 'NativeConst', 'PlayerDataGet', 'PlayerDataSet',
                'PlayerDataBoolTest']
    if variants != expected:
        raise AssertionError(f'hk_sim::script::Code changed: {variants} != {expected}')

_check_opcodes()

CMP_LT, CMP_LE, CMP_EQ, CMP_NE, CMP_GE, CMP_GT = range(6)

# The object reference constants, mirroring hk_sim::script.
HERO = 0x7fff_ffff
NO_OBJECT = 0

def source_id(path_id):
    """A scene path id as the stable 31-bit source id the cooked banks carry.

    HERO is the top of that range on purpose, so it survives the mask and cannot
    collide with a real object.
    """
    return path_id & 0x7fff_ffff

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

# The reference actions that store an object, and the field each stores into.
OBJECT_STORE = {'GetOwner': 'storeGameObject', 'GetHero': 'storeResult',
                'FindChild': 'storeResult', 'GetParent': 'storeResult',
                'FindGameObject': 'store'}
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
# Actions that can neither transition nor block, so a run of them at the top of
# the start state is guaranteed to have finished before the FSM does anything
# else. That guarantee is what makes a reference set there safe to read from
# any other state.
PROLOGUE_SAFE = set(OBJECT_STORE) | {
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
                for name in _named_variables(value):
                    if key == OBJECT_STORE.get(action):
                        stores[name].append(action)
                    elif key not in OBJECT_READS:
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

def _object_env(fsm, owner_id, objects, folded):
    """The variables this cook can fix, and what the start state fixes for all.

    Reading a reference in another state needs more than declaration order: the
    write has to be one every path has already taken. Only the opening run of
    actions in the start state gives that, and only while every action in it is
    one that cannot transition or block. Past that point the FSM may already
    have left, so a later state could read a variable the write never reached.

    Returns (fixed, env): the variables no other action disturbs, and the ids
    the start state's opening run pins for the whole definition.
    """
    fixed = _fixed_object_variables(fsm)
    env = {}
    if not fsm['states']:
        return fixed, env
    data = fsm['states'][0]['actionData']
    for i, raw in enumerate(data['actionNames']):
        if not data['actionEnabled'][i]:
            continue
        action = raw.rsplit('.', 1)[-1]
        if action not in PROLOGUE_SAFE:
            break
        try:
            _reference(action, action_fields(data, i, objects=True), owner_id, objects,
                       env, folded, fixed)
        except Exception:
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
        # Each state earns its references as it runs, on top of what the start
        # state pinned for everyone, so an action that reads one before it is
        # set is refused rather than handed a value stored later.
        visible = dict(env) if index else {}
        data = state['actionData']
        for i, raw in enumerate(data['actionNames']):
            if not data['actionEnabled'][i]:
                continue
            action = raw.rsplit('.', 1)[-1]
            args = action_fields(data, i, objects=True)
            ops.extend(compile_action(action, args, events, constants, variables, owner_id,
                                      fields, folded, objects, visible))
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

def compile_action(action, args, events, constants, variables, owner_id=None, fields=None,
                   folded=None, objects=None, env=None):
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
    if action == 'Wait':
        time = _scalar(args.get('time'))
        if time is None:
            raise Unsupported(action, 'variable wait time')
        if args.get('realTime'):
            raise Unsupported(action, 'real-time wait')
        return [(WAIT, 0, max(1, round(float(time) * 60)), events.id(args.get('finishEvent')), 0)]
    if action == 'NextFrameEvent':
        return [(NEXT_FRAME, 0, events.id(args.get('sendEvent')), 0, 0)]
    if action == 'SendEventByName':
        target = args.get('eventTarget')
        # Only a send to self is in this slice; a send to another FSM needs the
        # instance table that step 5's scope work builds.
        if isinstance(target, dict) and target.get('target') not in (None, 0):
            raise Unsupported(action, 'send to another FSM')
        # This action names its event with an FsmString, not the interned
        # FsmEvent the rest carry, so it can be a variable and has to be read as
        # one. Handing the raw field to the event table instead raised TypeError,
        # which the survey counted as an ordinary refusal: every send to self
        # measured as uncompilable and none had ever been emitted.
        name = _folded_scalar(args.get('sendEvent'), folded)
        if not isinstance(name, str) or not name:
            raise Unsupported(action, 'event name from a variable this cook cannot fix')
        return [(SEND_EVENT, 0, events.id(name), 0, 0)]
    if action == 'SetBoolValue':
        value = _scalar(args.get('boolValue'))
        if value is None:
            raise Unsupported(action, 'variable source')
        return [(SET_BOOL, int(bool(value)), _var(args.get('boolVariable'), variables, action), 0, 0)]
    if action == 'SetIntValue':
        value = _scalar(args.get('intValue'))
        if value is None:
            raise Unsupported(action, 'variable source')
        return [(SET_INT, 0, _var(args.get('intVariable'), variables, action),
                 constants.index(value), 0)]
    if action == 'BoolTest':
        return [(BOOL_TEST, 0, _var(args.get('boolVariable'), variables, action),
                 events.id(args.get('isTrue')), events.id(args.get('isFalse')))]
    if action == 'IntCompare':
        slot = _var(args.get('integer1'), variables, action)
        right = _scalar(args.get('integer2'))
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
        visible = dict(env) if index else {}
        data = state['actionData']
        for i, raw in enumerate(data['actionNames']):
            if not data['actionEnabled'][i]:
                continue
            action = raw.rsplit('.', 1)[-1]
            try:
                args = action_fields(data, i, objects=True)
                compile_action(action, args, events, constants, variables, owner_id, fields,
                               folded, objects, visible)
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
        'SendEventByName to self, NextFrameEvent, the PlayerData accessors and the '
        'cook-time object references (GetOwner, GetHero, FindChild, GetParent, '
        'FindGameObject).',
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
