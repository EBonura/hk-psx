"""Pin the authored numbers of a PlayMaker FSM, for the recognizers that hold them as constants.

A recognizer admits a placement by structural digest (`runner.fsm_fingerprint`);
this reads the same FSM back and proves the handful of values the controller
actually carries (waits, speeds, ranges, clip names) against it, so the digest
says nothing changed and these say what the constants are.
"""
import math

from focus import action_fields


def state(fsm, name):
    return next(st for st in fsm['states'] if st['name'] == name)


def variables(fsm):
    """The FSM's plain variables by name (vectors and object references left out)."""
    return {v['name']: v['value'] for group in fsm['variables'].values() if isinstance(group, list)
            for v in group if isinstance(v, dict) and 'name' in v and 'value' in v
            and not isinstance(v['value'], dict)}


def enabled_actions(who, fsm_state, action):
    """Every enabled instance of one action in a state, as decoded fields."""
    data = fsm_state['actionData']
    found = [i for i, n in enumerate(data['actionNames'])
             if n.rsplit('.', 1)[-1] == action and data['actionEnabled'][i]]
    return [action_fields(data, i, objects=True) for i in found]


def one_action(who, fsm_state, action):
    found = enabled_actions(who, fsm_state, action)
    if len(found) != 1:
        raise ValueError(f'{who} {fsm_state["name"]} no longer carries one enabled {action}')
    return found[0]


def _same(value, want):
    if isinstance(want, float):
        return isinstance(value, (int, float)) and not isinstance(value, bool) and math.isclose(
            float(value), want, rel_tol=0, abs_tol=1e-6)
    return value == want


def check_action(who, fsm_state, action, expected):
    """`expected` maps a field to its literal, or to ('var', name) for one bound to a variable."""
    fields = one_action(who, fsm_state, action)
    for key, want in expected.items():
        value = fields.get(key)
        if isinstance(want, tuple):
            ok = isinstance(value, dict) and bool(value.get('useVariable')) and value.get('name') == want[1]
        else:
            if isinstance(value, dict):
                ok = not value.get('useVariable') and _same(value.get('value'), want)
            else:
                ok = _same(value, want)
        if not ok:
            raise ValueError(f'unsupported {who} parameter: {fsm_state["name"]}/{action}.{key}')
    return fields


def check_transitions(who, fsm, expected, globals_):
    if [(t['fsmEvent']['name'], t['toState']) for t in fsm.get('globalTransitions', [])] != globals_:
        raise ValueError(f'unsupported {who} global transitions: {fsm["name"]}')
    for name, transitions in expected.items():
        actual = [(t['fsmEvent']['name'], t['toState']) for t in state(fsm, name)['transitions']]
        if actual != transitions:
            raise ValueError(f'unsupported {who} transitions: {fsm["name"]}/{name}')


def raw_params(data, index):
    """An action's parameters as (kind, bytes) by name, for the kinds `focus.action_fields`
    leaves alone (vectors)."""
    start = data['actionStartIndex'][index]
    end = data['actionStartIndex'][index + 1] if index + 1 < len(data['actionNames']) else len(data['paramName'])
    out = {}
    for i in range(start, end):
        pos, size = data['paramDataPos'][i], data['paramByteDataSize'][i]
        out[data['paramName'][i] or str(i)] = (data['paramDataType'][i], bytes(data['byteData'][pos:pos + size]))
    return out


def vector2(raw):
    """A serialized FsmVector2 parameter: x, y, useVariable, then the variable name."""
    import struct
    kind, payload = raw
    if kind != 37 or len(payload) < 9:
        raise ValueError('not a compact Vector2 parameter')
    x, y = struct.unpack('<ff', payload[:8])
    return x, y, bool(payload[8]), payload[9:].decode('utf8')


def check_vectors(who, fsm_state, action, nth, expected):
    """The `nth` enabled instance of an action must carry these Vector2 literals
    (`field: (x, y)`) or variable bindings (`field: ('var', name)`)."""
    data = fsm_state['actionData']
    found = [i for i, n in enumerate(data['actionNames'])
             if n.rsplit('.', 1)[-1] == action and data['actionEnabled'][i]]
    if len(found) <= nth:
        raise ValueError(f'{who} {fsm_state["name"]} no longer carries {action} #{nth}')
    params = raw_params(data, found[nth])
    for field, want in expected.items():
        x, y, use, name = vector2(params[field])
        ok = (use and name == want[1]) if want[0] == 'var' else (not use and _same(x, want[0]) and _same(y, want[1]))
        if not ok:
            raise ValueError(f'unsupported {who} parameter: {fsm_state["name"]}/{action}.{field}')


def check_enums(who, fsm_state, action, nth, expected):
    """The `nth` enabled instance of an action must carry these enum values (`field: int`)."""
    import struct
    data = fsm_state['actionData']
    found = [i for i, n in enumerate(data['actionNames'])
             if n.rsplit('.', 1)[-1] == action and data['actionEnabled'][i]]
    if len(found) <= nth:
        raise ValueError(f'{who} {fsm_state["name"]} no longer carries {action} #{nth}')
    params = raw_params(data, found[nth])
    for field, want in expected.items():
        kind, payload = params[field]
        if kind != 7 or len(payload) < 4 or struct.unpack('<i', payload[:4])[0] != want:
            raise ValueError(f'unsupported {who} parameter: {fsm_state["name"]}/{action}.{field}')


def fingerprint(fsm):
    """`runner.fsm_fingerprint`, tolerant of the parameter kinds that name their variable
    differently (function calls, variable references). Equal to it wherever that one succeeds."""
    import hashlib
    import json

    def scalar(value):
        if isinstance(value, dict):
            if value.get('useVariable'):
                return 'VAR:' + str(value.get('name', value.get('variableName', '')))
            return repr(value.get('value', value.get('name')))
        return repr(value)
    summary = {'name': fsm['name'], 'start': fsm['startState'],
               'variables': sorted((v['name'], repr(v['value'])) for group in fsm['variables'].values() if isinstance(group, list)
                                   for v in group if isinstance(v, dict) and 'name' in v and 'value' in v
                                   and not isinstance(v['value'], dict) and v['name'] != 'Lunge Speed'),
               'globals': [(t['fsmEvent']['name'], t['toState']) for t in fsm.get('globalTransitions', [])], 'states': []}
    for st in fsm['states']:
        data = st['actionData']
        actions = []
        for index, name in enumerate(data['actionNames']):
            try:
                fields = action_fields(data, index)
            except ValueError as error:
                fields = {'error': str(error)}
            actions.append((name, int(data['actionEnabled'][index]),
                            sorted((k, scalar(v)) for k, v in fields.items() if k != 'gameObject')))
        summary['states'].append((st['name'], [(t['fsmEvent']['name'], t['toState']) for t in st['transitions']], actions))
    return hashlib.sha256(json.dumps(summary, sort_keys=True, default=str).encode()).hexdigest()
