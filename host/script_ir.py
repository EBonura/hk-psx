"""Compile PlayMaker FSMs to the HKSCR01 IR (P10 steps 2 and 4).

Only the first slice is supported: comparisons and branches, booleans and ints,
waits, and state and event sends. Every other action is an explicit refusal
naming the action, because step 7 is clear that an unsupported required action
must block its scene rather than compile to a no-op that lets the scene claim to
be implemented.

The refusals are the useful output right now. Running this over the admitted
scenes says exactly how much of the authored behaviour the current slice covers
and which action to implement next.

No retail payload is embedded; the report goes to .hkpsx.
"""
import collections
from focus import action_fields

# Opcode numbers, matching hk_sim::script::Code.
NOP, WAIT, SET_BOOL, SET_INT, BOOL_TEST, INT_COMPARE, SEND_EVENT, NEXT_FRAME, NATIVE = range(9)
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

class Constants:
    def __init__(self):
        self.values = []
    def index(self, value):
        value = int(value)
        if value not in self.values:
            self.values.append(value)
        return self.values.index(value)

def _variables(fsm):
    out = {}
    for group in fsm['variables'].values():
        if not isinstance(group, list):
            continue
        for v in group:
            if isinstance(v, dict) and 'name' in v:
                out.setdefault(v['name'], len(out))
    return out

def compile_fsm(fsm, owner_id=None):
    """One FSM definition to (states, transitions, ops, constants, events).

    Variables are a span in the caller's pool, so there is no slot cap here.
    Raises Unsupported on the first action this slice does not implement.
    """
    events = Events()
    constants = Constants()
    variables = _variables(fsm)
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
                                      events, constants, variables, owner_id))
        states.append((first_op, len(ops) - first_op, first_transition,
                       len(transitions) - first_transition))
    return {'states': states, 'transitions': transitions, 'ops': ops,
            'constants': constants.values, 'events': events.ids,
            'state_names': names, 'variables': variables, 'var_count': len(variables)}

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

def compile_action(action, fields, events, constants, variables, owner_id=None):
    """Ops for one enabled action, or Unsupported."""
    if action == 'GetOwner':
        # The owner of an FSM never changes, so this resolves at cook time into
        # the object's stable id rather than needing an object reference at
        # runtime. A caller that cannot supply one is refused, not defaulted.
        if owner_id is None:
            raise Unsupported(action, 'no stable owner id supplied')
        return [(SET_INT, 0, _var(fields.get('storeGameObject'), variables, action),
                 constants.index(owner_id), 0)]
    if action == 'Wait':
        time = _scalar(fields.get('time'))
        if time is None:
            raise Unsupported(action, 'variable wait time')
        if fields.get('realTime'):
            raise Unsupported(action, 'real-time wait')
        return [(WAIT, 0, max(1, round(float(time) * 60)), events.id(fields.get('finishEvent')), 0)]
    if action == 'NextFrameEvent':
        return [(NEXT_FRAME, 0, events.id(fields.get('sendEvent')), 0, 0)]
    if action == 'SendEventByName':
        target = fields.get('eventTarget')
        # Only a send to self is in this slice; a send to another FSM needs the
        # instance table that step 5's scope work builds.
        if isinstance(target, dict) and target.get('target') not in (None, 0):
            raise Unsupported(action, 'send to another FSM')
        return [(SEND_EVENT, 0, events.id(fields.get('sendEvent')), 0, 0)]
    if action == 'SetBoolValue':
        value = _scalar(fields.get('boolValue'))
        if value is None:
            raise Unsupported(action, 'variable source')
        return [(SET_BOOL, int(bool(value)), _var(fields.get('boolVariable'), variables, action), 0, 0)]
    if action == 'SetIntValue':
        value = _scalar(fields.get('intValue'))
        if value is None:
            raise Unsupported(action, 'variable source')
        return [(SET_INT, 0, _var(fields.get('intVariable'), variables, action),
                 constants.index(value), 0)]
    if action == 'BoolTest':
        return [(BOOL_TEST, 0, _var(fields.get('boolVariable'), variables, action),
                 events.id(fields.get('isTrue')), events.id(fields.get('isFalse')))]
    if action == 'IntCompare':
        slot = _var(fields.get('integer1'), variables, action)
        right = _scalar(fields.get('integer2'))
        if right is None:
            raise Unsupported(action, 'variable right-hand side')
        index = constants.index(right)
        out = []
        for key, selector in (('lessThan', CMP_LT), ('equal', CMP_EQ), ('greaterThan', CMP_GT)):
            event = events.id(fields.get(key))
            if event:
                out.append((INT_COMPARE, selector, slot, index, event))
        return out
    raise Unsupported(action)

def survey(source, scene_files):
    """How much of the admitted scenes' authored behaviour this slice compiles."""
    compiled = collections.Counter()
    blockers = collections.Counter()
    blocking_fsms = collections.Counter()
    for name, scene in scene_files.items():
        file = source.file(name)
        for obj in file.objects.values():
            if obj.type.name != 'MonoBehaviour':
                continue
            try:
                if source.typename(obj) != 'PlayMakerFSM':
                    continue
                fsm = source.read(obj)['fsm']
            except Exception:
                continue
            compiled['instances'] += 1
            try:
                # The FSM's own path id is its stable owner identity here; the
                # cooked bank uses the same source id the world metadata does.
                program = compile_fsm(fsm, owner_id=obj.path_id & 0x7fffffff)
            except Unsupported as refusal:
                blockers[refusal.action] += 1
                blocking_fsms[fsm['name']] += 1
                continue
            compiled['compiled'] += 1
            compiled['ops'] += len(program['ops'])
    return {'instances': compiled['instances'], 'compiled': compiled['compiled'],
            'compiled_ops': compiled['ops'],
            'first_blocking_action': dict(blockers.most_common(25)),
            'blocked_fsms': dict(blocking_fsms.most_common(20))}

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
        'An FSM is counted as blocked by the first action that refuses it, so the '
        'blocker histogram ranks what to implement next rather than counting every '
        'unsupported action in the scene.',
        'Nothing is cooked into the disc yet: this measures the compiler, it does not '
        'ship a script bank.',
    ]
    dump(ROOT / '.hkpsx/script-ir-coverage.json', out)
    print(f"{out['compiled']} of {out['instances']} FSM instances compile "
          f"({out['compiled'] / max(1, out['instances']):.1%}), {out['compiled_ops']} ops")
    print('first blocking action:')
    for action, n in list(out['first_blocking_action'].items())[:12]:
        print(f'  {action:32} blocks {n}')
