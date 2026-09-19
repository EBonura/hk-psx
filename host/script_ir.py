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

class Unsupported(Exception):
    """An action this slice cannot compile. Carries the action for the report."""
    def __init__(self, action, detail=''):
        super().__init__(f'{action}{": " + detail if detail else ""}')
        self.action = action

class Events:
    """Interned event names; zero is reserved for 'no event'."""
    def __init__(self):
        self.ids = {}
    def id(self, name):
        if not name:
            return 0
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

def compile_fsm(fsm, owner_id=None):
    """One FSM definition to (states, transitions, ops, constants, events).

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
    names = [s['name'] for s in fsm['states']]
    index_of = {name: i for i, name in enumerate(names)}
    states, transitions, ops = [], [], []
    for state in fsm['states']:
        first_op, first_transition = len(ops), len(transitions)
        for t in state['transitions']:
            target = t['toState']
            if target not in index_of:
                # PlayMaker allows a transition with no target; it is a no-op.
                continue
            transitions.append((events.id(t['fsmEvent']['name']), index_of[target]))
        data = state['actionData']
        for i, raw in enumerate(data['actionNames']):
            if not data['actionEnabled'][i]:
                continue
            ops.extend(compile_action(raw.rsplit('.', 1)[-1], action_fields(data, i, objects=True),
                                      events, constants, variables, owner_id, fields, folded))
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

def compile_action(action, args, events, constants, variables, owner_id=None, fields=None,
                   folded=None):
    """Ops for one enabled action, or Unsupported."""
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
    if action == 'GetOwner':
        # The owner of an FSM never changes, so this resolves at cook time into
        # the object's stable id rather than needing an object reference at
        # runtime. A caller that cannot supply one is refused, not defaulted.
        if owner_id is None:
            raise Unsupported(action, 'no stable owner id supplied')
        return [(SET_INT, 0, _var(args.get('storeGameObject'), variables, action),
                 constants.index(owner_id), 0)]
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
        return [(SEND_EVENT, 0, events.id(args.get('sendEvent')), 0, 0)]
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

def unsupported_actions(fsm, owner_id=None):
    """Every action in this FSM the slice cannot compile, not just the first.

    Ranking by the first refusal is a treadmill: an FSM compiles only when all
    of its actions do, so what matters is the whole set an FSM is missing.
    """
    events, constants, fields = Events(), Constants(), Fields()
    variables = _variables(fsm)
    mentions = _variable_mentions(fsm)
    folded = {name: value for name, value in _variable_values(fsm).items()
              if mentions[name] == 1}
    missing = set()
    for state in fsm['states']:
        data = state['actionData']
        for i, raw in enumerate(data['actionNames']):
            if not data['actionEnabled'][i]:
                continue
            action = raw.rsplit('.', 1)[-1]
            try:
                compile_action(action, action_fields(data, i, objects=True), events, constants,
                               variables, owner_id, fields, folded)
            except Unsupported as refusal:
                missing.add(refusal.action)
            except Exception:
                missing.add(action)
    return missing

def scene_fsms(source, name):
    """The scene's FSMs as (path id, definition, runs_natively).

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
    return [(path_id, tree['fsm'], tree['m_GameObject']['m_PathID'] in native)
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
    unparsed = {}
    for name in scene_files:
        try:
            fsms = scene_fsms(source, name)
        except Exception as error:
            # Without a parse there is no native-actor set, so counting this
            # scene's FSMs as script work would put back exactly the
            # overstatement this measurement exists to remove. Name the scene
            # instead, so a silent drop cannot be mistaken for coverage.
            unparsed[name] = str(error)
            continue
        for path_id, fsm, runs_natively in fsms:
            if runs_natively:
                compiled['on_native_actors'] += 1
                native_fsms[fsm['name']] += 1
                continue
            compiled['instances'] += 1
            # The FSM's own path id is its stable owner identity here; the
            # cooked bank uses the same source id the world metadata does.
            owner_id = path_id & 0x7fffffff
            try:
                program = compile_fsm(fsm, owner_id=owner_id)
            except Unsupported as refusal:
                blockers[refusal.action] += 1
                blocking_fsms[fsm['name']] += 1
                missing = unsupported_actions(fsm, owner_id=owner_id)
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
            'on_native_actors': compiled['on_native_actors'],
            'native_actor_fsms': dict(native_fsms.most_common(20)),
            'unparsed_scenes': unparsed,
            'first_blocking_action': dict(blockers.most_common(25)),
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
        'SendEventByName to self and NextFrameEvent.',
        'FSMs on GameObjects a supported native actor controller already runs are counted '
        'under on_native_actors and left out of the instances denominator, because their '
        'behaviour is already implemented in Rust rather than waiting on the script slice.',
        'An actor counts as native only where actors.py reports movement_supported and only '
        'on its own GameObject, so an enemy the port draws but does not yet drive, and a '
        'detector or trigger volume that only messages one, both stay script work.',
        'first_blocking_action counts each blocked FSM once, by the action that refuses '
        'first; one_action_from_compiling and missing_sets use the whole set an FSM is '
        'missing and are the ones to rank by.',
        'Nothing is cooked into the disc yet: this measures the compiler, it does not '
        'ship a script bank.',
    ]
    dump(ROOT / '.hkpsx/script-ir-coverage.json', out)
    print(f"{out['compiled']} of {out['instances']} script-side FSM instances compile "
          f"({out['compiled'] / max(1, out['instances']):.1%}), {out['compiled_ops']} ops; "
          f"{out['on_native_actors']} further instances sit on native actor controllers")
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
