"""The NPC, dialogue-selection and dialogue-text catalog P18 step 1 asks for.

P18 wants NPC state and dialogue selection taken from the installed game rather
than transcribed, and the source spreads one NPC across several places.

`npc_control` is a shared template FSM that every talkable NPC carries; it owns
the prompt, the range trigger, taking control of the hero and the turn/approach
motion, and it is parameterized only by its own variables, so the per NPC values
are extracted rather than the (identical) states.

`Conversation Control` is the per NPC FSM and the only place the choice of line
lives. It reads PlayerData into FSM variables and then runs a column of test
actions, each sending an event. PlayMaker stops executing a state's remaining
actions as soon as one of them causes a transition (`FsmState::ActivateActions`
returns at the first `Fsm.IsSwitchingState`), so the tests are a priority list in
declaration order and must be read in that order. `focus.action_parameters` is
what preserves that order for the array shaped actions.

The line itself is `DialogueBox.StartConversation(convName, sheetName)` called
through `CallMethodProper`, so the localization key and its sheet are action
arguments. The text is in an AES-256-ECB encrypted language sheet; this reuses
`items.language_sheet`, which reads the key out of `Encryption::.cctor` rather
than embedding it. There is no single dialogue sheet: each conversation names
its own (`Elderbug`, `Minor NPC`, `CP2`, `Zote`, ...).

Whether an NPC exists at all is `DeactivateIfPlayerdataTrue`/`False` components
on the object or an ancestor, plus self-disabling branches inside the NPC's own
FSMs (Elderbug Grimm deactivates itself unless the troupe is in town; the
Gravedigger ghost goes inert without the Dream Nail). Both are evaluated against
`PlayerData::SetupNewPlayerData`, which is what a fresh save starts from.

Extraction only. Nothing here is cooked or bound to the guest, no NPC exists in
the port today, and no retail payload is embedded. The report goes to
.hkpsx/npc-catalog.json.
"""
import json
from source import Source, ROOT, dump
from scene import Scene
# Only for its BuildSettings scene-name to file map and the order assertion
# that comes with it. Scenes are parsed here and dropped, not cached.
from shops import Catalog
from focus import action_fields, action_parameters, fsm_variables
# The language sheet reader, its key derivation and the new-save PlayerData all
# already exist in items.py and are reused rather than reimplemented.
from items import _encryption_key, language_sheet, playerdata_defaults

ADMITTED = ROOT / '.hkpsx/selected-regions.json'
CONTROL_FSM = 'npc_control'
CONVO_FSM = 'Conversation Control'
DREAM_FSM = 'npc_dream_dialogue'
TITLE_SHEET = 'Titles'
DEACTIVATORS = ('DeactivateIfPlayerdataTrue', 'DeactivateIfPlayerdataFalse')
# npc_control is one template shared by every NPC, so only its variables differ.
CONTROL_VARIABLES = ('Prompt Name', 'Turn Anim Name', 'Can Talk', 'Always Faces Hero',
                     'Turns To Speak', 'Sprite Faces Right', 'Move To Offset',
                     'Hero Always Left', 'Hero Always Right')

UNKNOWN = object()
WALK_LIMIT = 64
# How many conversations one NPC may speak before it starts repeating. The port
# remembers where an NPC has got to in a fixed-width per-NPC cursor, so a longer
# chain is refused rather than truncated to the branches that happen to fit.
MAX_CONVERSATIONS = 4

# Actions that branch and are decoded below. Anything else that can send an
# event stops a walk instead of being guessed at.
BRANCHING = {'PlayerDataBoolTest', 'PlayerDataBoolTrueAndFalse', 'PlayerDataBoolAllTrue',
             'BoolTest', 'BoolTestMulti', 'BoolAllTrue', 'IntSwitch', 'IntCompare',
             'SendEvent', 'SendEventByName'}
# Actions that never finish by themselves: a state holding one is where the FSM
# sits waiting for the player, not a state a static walk can step past.
WAITING = {'Trigger2dEvent', 'ListenForDown', 'ListenForUp', 'ListenForJump',
           'ListenForAttack', 'ListenForCast', 'ListenForQuickMap',
           'CheckTrackTriggerCount', 'FaceObject'}
# Actions a walk may step past: they send no event, so they cannot end a state.
# Most are presentation, audio, object plumbing or arithmetic no decoded test
# reads; the PlayerData and variable ones do carry state and are evaluated in
# `_walk` before it reaches this set.
INERT = {
    'GetOwner', 'GetParent', 'GetHero', 'FindChild', 'FindGameObject', 'SetGameObject',
    'SetFsmGameObject', 'ActivateGameObject', 'ActivateAllChildren', 'DestroyObject',
    'DestroySelf', 'SetSpriteRenderer', 'SetSpriteRendererSprite', 'SetMeshRenderer',
    'SetCollider', 'SetScale', 'GetScale', 'SetPosition', 'GetPosition', 'Vector3AddXYZ',
    'SetFsmBool', 'SetFsmString', 'SetFsmInt', 'SetFsmFloat', 'SetFloatValue',
    'SetStringValue', 'BuildString', 'ConvertIntToString', 'ConvertStringToInt',
    'ConvertIntToFloat', 'ConvertFloatToInt', 'FloatMultiply', 'SetTextMeshProText',
    'AudioPlayerOneShot', 'AudioPlayerOneShotSingle', 'AudioPlaySimple', 'AudioPlay',
    'AudioPlayInState', 'AudioStop', 'SetAudioVolume', 'Tk2dPlayAnimation', 'Tk2dPlayFrame',
    'Tk2dSpriteGetId', 'PlayParticleEmitter', 'StopParticleEmitter', 'SetParticleEmission',
    'SetParticleEmissionRate', 'SetParticleScale', 'iTweenMoveBy', 'SendMessage',
    'SendEventToRegister', 'ShowPromptMarker', 'HidePromptMarker', 'AddTrackTrigger',
    'ForceHeroFootstepSound', 'SetVelocity2d', 'CreateObject', 'SpawnObjectFromGlobalPool',
    'PlayerDataIntAdd', 'IncrementPlayerDataInt', 'SetPlayerDataBool', 'SetPlayerDataInt',
    'GetPlayerDataBool', 'GetPlayerDataInt', 'SetBoolValue', 'SetIntValue',
    'IntCompareToBool',
    'CallMethodProper', 'Wait', 'NextFrameEvent', 'Tk2dWatchAnimationEvents',
    'Tk2dPlayAnimationWithEvents',
}
# The state ends and sends this event once every action in it has finished.
FINISH_FIELDS = ('finishEvent', 'animationCompleteEvent')
PD_WRITES = {'SetPlayerDataBool': 'boolName', 'SetPlayerDataInt': 'intName',
             'IncrementPlayerDataInt': 'intName', 'PlayerDataIntAdd': 'intName'}


# ------------------------------------------------------------------ FSM reading


def _short(name):
    return name.rsplit('.', 1)[-1]


def _word(value):
    """A compact field as a literal, or `$Name` when it reads an FSM variable."""
    if isinstance(value, dict) and 'useVariable' in value and 'value' in value:
        return '$' + value['name'] if value['useVariable'] else value['value']
    return value


def _variable(value):
    """The FSM variable a compact field reads, or None when it is a literal."""
    if isinstance(value, dict) and value.get('useVariable') and 'value' in value:
        return value['name']
    return None


def _enabled(state):
    """(index, action kind) of one state's enabled actions, in declaration order."""
    data = state['actionData']
    return [(i, _short(n)) for i, n in enumerate(data['actionNames'])
            if data['actionEnabled'][i]]


def _positional(data, index):
    """The unnamed, array-shaped parameters of one action, in declaration order."""
    return [v for name, v in action_parameters(data, index) if name is None]


def _owner_target(field):
    """Whether an fsmOwnerDefault action acts on the object that owns the FSM."""
    inner = field.get('gameObject') if isinstance(field, dict) else None
    if isinstance(inner, dict) and 'ownerOption' in inner:
        return inner['ownerOption'] == 0
    return True


def _truth(value):
    """PlayerData and PlayMaker both carry bools as 0/1 here."""
    if value is UNKNOWN or value is None:
        return UNKNOWN
    return bool(value)


# --------------------------------------------------------------- one NPC's FSM


def _reads(state):
    """{FSM variable: PlayerData field} for the reads one state performs."""
    data = state['actionData']
    out = {}
    for i, kind in _enabled(state):
        if kind not in ('GetPlayerDataBool', 'GetPlayerDataInt'):
            continue
        fields = action_fields(data, i)
        name = _variable(fields.get('storeValue'))
        field = _word(fields.get('boolName') or fields.get('intName'))
        if name and isinstance(field, str):
            out[name] = field
    return out


def _origins(fsm):
    """{FSM variable: the PlayerData field(s) it is ever read from} for one FSM.

    A `BoolTest` names a variable, not a field, so the condition only reads as a
    progression gate once the variable is traced back to its source. A variable
    fed by more than one field keeps all of them rather than picking one.
    """
    out = {}
    for state in fsm['states']:
        for name, field in _reads(state).items():
            out.setdefault(name, [])
            if field not in out[name]:
                out[name].append(field)
    return out


def _writes(state):
    """Every PlayerData write one state performs."""
    data = state['actionData']
    out = []
    for i, kind in _enabled(state):
        key = PD_WRITES.get(kind)
        if not key:
            continue
        fields = action_fields(data, i)
        field = _word(fields.get(key))
        if not isinstance(field, str) or not field:
            continue
        entry = {'action': kind, 'field': field}
        amount = fields.get('value', fields.get('amount'))
        if amount is not None:
            entry['value'] = _word(amount)
        out.append(entry)
    return out


def _clips(state):
    """Every animation clip one state plays, with whether it plays it on itself."""
    data = state['actionData']
    out = []
    for i, kind in _enabled(state):
        if kind not in ('Tk2dPlayAnimation', 'Tk2dPlayAnimationWithEvents'):
            continue
        fields = action_fields(data, i)
        clip = _word(fields.get('clipName'))
        if not isinstance(clip, str) or not clip:
            continue
        entry = {'clip': clip, 'on_self': _owner_target(fields)}
        if not entry['on_self']:
            entry['target'] = _variable(fields['gameObject'].get('gameObject'))
        out.append(entry)
    return out


def _lines(state, variables):
    """The localization keys one state speaks, with the sheet each one names."""
    data = state['actionData']
    out = []
    for i, kind in _enabled(state):
        fields = action_fields(data, i)
        if kind == 'CallMethodProper':
            if (_word(fields.get('behaviour')) != 'DialogueBox'
                    or _word(fields.get('methodName')) != 'StartConversation'):
                continue
            args = [v for v in _positional(data, i)
                    if isinstance(v, dict) and 'stringValue' in v]
            if len(args) != 2:
                raise ValueError('DialogueBox.StartConversation no longer takes '
                                 f'two arguments: {len(args)}')
            key, sheet = (_argument(a, variables) for a in args)
            entry = {'key': key[0], 'sheet': sheet[0], 'action': kind}
            if key[1]:
                entry['key_from_variable'] = key[1]
            if sheet[1]:
                entry['sheet_from_variable'] = sheet[1]
            out.append(entry)
        elif kind == 'GetLanguageString':
            entry = {'key': _word(fields.get('convName')),
                     'sheet': _word(fields.get('sheetName')), 'action': kind}
            for name, key in (('convName', 'key'), ('sheetName', 'sheet')):
                variable = _variable(fields.get(name))
                if variable:
                    entry[key + '_from_variable'] = variable
                    value = variables.get(variable)
                    entry[key] = value if isinstance(value, str) else None
            out.append(entry)
    return out


def _argument(arg, variables):
    """One FsmVar argument as (value, variable it came from)."""
    if arg.get('useVariable'):
        name = arg['variableName']
        value = variables.get(name)
        return (value if isinstance(value, str) else None), name
    return arg['stringValue'], None


# --------------------------------------------------------------- the conditions


def _branch(data, index, kind):
    """One branching action normalized to terms plus the events it can send.

    `terms` are ANDed. A term names either a PlayerData field or an FSM variable,
    and the value the term expects. Semantics come from the shipped actions:
    `BoolTestMulti::DoAllTrue` compares every variable against its paired state
    and sends trueEvent only when all of them agree, and
    `PlayerDataBoolTrueAndFalse::OnEnter` sends isTrue only when the first bool
    is set and the second is not.
    """
    fields = action_fields(data, index)
    if kind == 'PlayerDataBoolTest':
        return {'action': kind, 'terms': [{'field': _word(fields['boolName']), 'expect': True}],
                'event': fields['isTrue'], 'else_event': fields['isFalse']}
    if kind == 'PlayerDataBoolTrueAndFalse':
        return {'action': kind,
                'terms': [{'field': _word(fields['trueBool']), 'expect': True},
                          {'field': _word(fields['falseBool']), 'expect': False}],
                'event': fields['isTrue'], 'else_event': fields['isFalse']}
    if kind == 'PlayerDataBoolAllTrue':
        names = [v for v in _positional(data, index) if isinstance(v, str)]
        return {'action': kind, 'terms': [{'field': n, 'expect': True} for n in names],
                'event': fields.get('sendEvent'), 'else_event': None}
    if kind == 'BoolTest':
        return {'action': kind,
                'terms': [{'variable': _variable(fields['boolVariable']), 'expect': True}],
                'event': fields['isTrue'], 'else_event': fields['isFalse']}
    if kind == 'BoolAllTrue':
        names = [_variable(v) for v in _positional(data, index) if _variable(v)]
        return {'action': kind, 'terms': [{'variable': n, 'expect': True} for n in names],
                'event': fields.get('sendEvent'), 'else_event': None}
    if kind == 'BoolTestMulti':
        positional = _positional(data, index)
        names = [_variable(v) for v in positional if _variable(v)]
        states = [v['value'] for v in positional
                  if isinstance(v, dict) and not v.get('useVariable') and 'value' in v]
        if len(names) != len(states) or not names:
            raise ValueError('BoolTestMulti no longer decodes to paired variable '
                             f'and state arrays: {len(names)} and {len(states)}')
        return {'action': kind,
                'terms': [{'variable': n, 'expect': bool(s)} for n, s in zip(names, states)],
                'event': fields.get('trueEvent'), 'else_event': fields.get('falseEvent')}
    if kind == 'IntSwitch':
        positional = _positional(data, index)
        values = [v['value'] for v in positional
                  if isinstance(v, dict) and not v.get('useVariable') and 'value' in v]
        events = [v for v in positional if isinstance(v, str)]
        if len(values) != len(events) or not values:
            raise ValueError('IntSwitch no longer decodes to matching value and '
                             f'event arrays: {len(values)} and {len(events)}')
        return {'action': kind, 'variable': _variable(fields['intVariable']),
                'cases': [{'value': v, 'event': e} for v, e in zip(values, events)]}
    if kind == 'IntCompare':
        return {'action': kind, 'left': _word(fields['integer1']),
                'right': _word(fields['integer2']),
                'equal': fields.get('equal'), 'less_than': fields.get('lessThan'),
                'greater_than': fields.get('greaterThan')}
    if kind in ('SendEvent', 'SendEventByName'):
        return {'action': kind, 'terms': [], 'event': _word(fields.get('sendEvent')),
                'else_event': None}
    raise ValueError(f'{kind} is not a decoded branching action')


def _integer(word, variables):
    """An IntCompare operand: a literal, or the FSM variable `$Name` names."""
    if isinstance(word, str) and word.startswith('$'):
        word = variables.get(word[1:], UNKNOWN)
    if isinstance(word, int) and not isinstance(word, bool):
        return word
    return UNKNOWN


def _value(term, playerdata, variables):
    if 'field' in term:
        field = term['field']
        return _truth(playerdata[field]) if field in playerdata else UNKNOWN
    name = term.get('variable')
    if name is None or name not in variables:
        return UNKNOWN
    return _truth(variables[name])


def _fires(branch, playerdata, variables):
    """What this branch sends in the given state: {event, matched} or UNKNOWN.

    `matched` says whether the branch's own terms held, so a reader can tell an
    event sent because a flag is set from one sent because it is clear.
    """
    if branch['action'] == 'IntSwitch':
        current = variables.get(branch['variable'], UNKNOWN)
        if current is UNKNOWN or isinstance(current, bool) or not isinstance(current, int):
            return UNKNOWN
        for case in branch['cases']:
            if case['value'] == current:
                return {'event': case['event'], 'matched': True, 'case': current}
        return {'event': '', 'matched': False, 'case': current}
    if branch['action'] == 'IntCompare':
        left = _integer(branch['left'], variables)
        right = _integer(branch['right'], variables)
        if left is UNKNOWN or right is UNKNOWN:
            return UNKNOWN
        event = (branch['equal'] if left == right else
                 branch['less_than'] if left < right else branch['greater_than'])
        return {'event': event, 'matched': True, 'compared': [left, right]}
    for term in branch['terms']:
        value = _value(term, playerdata, variables)
        if value is UNKNOWN:
            return UNKNOWN
        if value != term['expect']:
            return {'event': branch['else_event'], 'matched': False}
    return {'event': branch['event'], 'matched': True}


# ------------------------------------------------------------------- the walker


def _target(fsm, states, state, event):
    """The state an event moves to from here, honouring the FSM's globals."""
    if not event:
        return None
    for transition in states[state]['transitions']:
        if transition['fsmEvent']['name'] == event:
            return transition['toState'] or None
    for transition in fsm['globalTransitions']:
        if transition['fsmEvent']['name'] == event:
            return transition['toState'] or None
    return None


def _finish_event(state):
    """The event a state sends once all of its actions have finished."""
    data = state['actionData']
    for i, _kind in _enabled(state):
        fields = action_fields(data, i)
        for name in FINISH_FIELDS:
            if isinstance(fields.get(name), str) and fields[name]:
                return fields[name]
    return 'FINISHED'


def _walk(fsm, start, playerdata, variables, origins):
    """Step the FSM from one state with a fixed PlayerData, refusing to guess.

    Only the decoded branches above are evaluated, and the first one that causes
    a transition wins, which is what `FsmState::ActivateActions` does. The walk
    stops at the first action it cannot decide, and says which one, rather than
    assuming it does nothing.
    """
    states = {s['name']: s for s in fsm['states']}
    playerdata = dict(playerdata)
    variables = dict(variables)
    current = start
    path, lines, writes, seen = [], [], [], []

    def stop(**extra):
        return {'path': path, 'lines': lines, 'writes': writes,
                'settles_in': None, **extra}

    while current in states and len(path) < WALK_LIMIT:
        if current in seen:
            return stop(loops_back_to=current)
        seen.append(current)
        state = states[current]
        step = {'state': current, 'took': None}
        waits = [k for _i, k in _enabled(state) if k in WAITING]
        blocked = [k for _i, k in _enabled(state)
                   if k not in INERT and k not in BRANCHING and k not in WAITING]
        for line in _lines(state, variables):
            if line not in lines:
                lines.append(line)
        writes.extend(_writes(state))
        data = state['actionData']
        moved = None
        undecided = None
        for i, kind in _enabled(state):
            if kind in PD_WRITES:
                fields = action_fields(data, i)
                field = _word(fields.get(PD_WRITES[kind]))
                amount = _word(fields.get('value'))
                if isinstance(field, str) and not isinstance(amount, str):
                    playerdata[field] = amount
                continue
            if kind in ('GetPlayerDataBool', 'GetPlayerDataInt'):
                fields = action_fields(data, i)
                name = _variable(fields.get('storeValue'))
                field = _word(fields.get('boolName') or fields.get('intName'))
                if name:
                    variables[name] = playerdata.get(field, UNKNOWN)
                continue
            if kind in ('SetBoolValue', 'SetIntValue'):
                fields = action_fields(data, i)
                name = _variable(fields.get('boolVariable') or fields.get('intVariable'))
                value = _word(fields.get('boolValue', fields.get('intValue')))
                if name and not isinstance(value, str):
                    variables[name] = value
                continue
            if kind == 'IntCompareToBool':
                # Not a branch: it stores the comparison in up to three bool
                # variables and a later test reads them. Elderbug's Convo Choice
                # derives `Is Steel Soul Mode` from permadeathMode this way, and
                # leaving it undecoded stops every walk that reaches him after
                # metElderbug is set.
                fields = action_fields(data, i)
                left = _integer(_word(fields.get('integer1')), variables)
                right = _integer(_word(fields.get('integer2')), variables)
                for key, holds in (('equalBool', lambda a, b: a == b),
                                   ('lessThanBool', lambda a, b: a < b),
                                   ('greaterThanBool', lambda a, b: a > b)):
                    name = _variable(fields.get(key))
                    if not name:
                        continue
                    variables[name] = (UNKNOWN if UNKNOWN in (left, right)
                                       else holds(left, right))
                continue
            if kind not in BRANCHING:
                if kind in WAITING or kind not in INERT:
                    break
                continue
            branch = _branch(data, i, kind)
            fired = _fires(branch, playerdata, variables)
            if fired is UNKNOWN:
                undecided = _describe(branch, origins)
                break
            destination = _target(fsm, states, current, fired['event'])
            if destination:
                step['took'] = dict(fired, branch=_describe(branch, origins),
                                    to_state=destination)
                moved = destination
                break
        path.append(step)
        if undecided is not None:
            return stop(undecided_in=current, undecided_branch=undecided)
        if moved:
            current = moved
            continue
        if blocked:
            return stop(undecided_in=current, not_decoded=sorted(set(blocked)))
        if waits:
            return _settled(fsm, states, path, lines, writes, current,
                            waits_for=sorted(set(waits)))
        # A state that speaks waits for the dialogue box to close, which returns
        # CONVO_FINISH rather than the FINISHED every other state ends on.
        event = _finish_event(state)
        destination = _target(fsm, states, current, event)
        if destination is None and _lines(state, variables):
            destination = _target(fsm, states, current, 'CONVO_FINISH')
        if not destination:
            return _settled(fsm, states, path, lines, writes, current)
        current = destination
    return stop(stopped='walk limit' if current in states else f'no state {current!r}')


def _settled(fsm, states, path, lines, writes, current, **extra):
    out = {'path': path, 'lines': lines, 'writes': writes, 'settles_in': current, **extra}
    if _self_disables(states[current]):
        out['disables_the_npc'] = True
    # The npc_control template raises CONVO START when the player presses up in
    # range, so a settling state that has no transition for it cannot be talked
    # to, however present the object is.
    out['accepts_convo_start'] = bool(_target(fsm, states, current, 'CONVO START'))
    return out


def _describe(branch, origins):
    """A branch with each variable term traced back to the PlayerData it reads."""
    out = dict(branch)
    terms = []
    for term in branch.get('terms', []):
        term = dict(term)
        fields = origins.get(term.get('variable'))
        if fields:
            term['reads'] = fields
        terms.append(term)
    if terms:
        out['terms'] = terms
    return out


def _conditional(branch):
    """Whether a branch tests something, as opposed to sending a fixed event."""
    return branch['action'] in ('IntSwitch', 'IntCompare') or bool(branch.get('terms'))


def _choices(fsm):
    """The names of the states that branch on a condition."""
    return {state['name'] for state in fsm['states']
            if any(_conditional(_branch(state['actionData'], i, kind))
                   for i, kind in _enabled(state) if kind in BRANCHING)}


def _speaks_via(states, start, choices, depth=8):
    """The speaking states one choice reaches without passing another choice.

    A conversation reaches its line through the dialogue-box states rather than
    straight from the test, so a one hop answer misses most of them; stopping at
    the next choice state is what keeps this from making every state reach
    everything through the FSM's own Idle.
    """
    found, seen, edge = [], {start}, [start]
    for _ in range(depth):
        following = []
        for name in edge:
            for transition in states[name]['transitions']:
                target = transition['toState']
                if not target or target not in states or target in seen:
                    continue
                seen.add(target)
                if _lines(states[target], {}):
                    found.append(target)
                elif target not in choices:
                    following.append(target)
        edge = following
        if not edge:
            break
    return sorted(found)


def _selection(fsm, origins):
    """Every state that chooses between lines, and its tests in priority order."""
    states = {s['name']: s for s in fsm['states']}
    choices = _choices(fsm)
    out = []
    for state in fsm['states']:
        if state['name'] not in choices:
            continue
        data = state['actionData']
        branches = []
        for i, kind in _enabled(state):
            if kind not in BRANCHING:
                continue
            branch = _describe(_branch(data, i, kind), origins)
            if branch['action'] == 'IntSwitch':
                for case in branch['cases']:
                    case['to_state'] = _target(fsm, states, state['name'], case['event'])
            elif branch['action'] == 'IntCompare':
                for key in ('equal', 'less_than', 'greater_than'):
                    branch[key + '_to_state'] = _target(fsm, states, state['name'],
                                                        branch[key])
            else:
                branch['to_state'] = _target(fsm, states, state['name'], branch.get('event'))
                branch['else_to_state'] = _target(fsm, states, state['name'],
                                                  branch.get('else_event'))
            branches.append(branch)
        out.append({'state': state['name'], 'branches': branches,
                    'leads_to_lines_in': _speaks_via(states, state['name'], choices)})
    return out


def _states(fsm, variables, origins):
    """Every state of a conversation FSM, with nothing summarized blind."""
    out = []
    for state in fsm['states']:
        entry = {'name': state['name'],
                 'actions': [k for _i, k in _enabled(state)],
                 'transitions': {t['fsmEvent']['name']: t['toState']
                                 for t in state['transitions']},
                 'lines': _lines(state, variables),
                 'playerdata_writes': _writes(state),
                 'animations': _clips(state)}
        branches = [_describe(_branch(state['actionData'], i, k), origins)
                    for i, k in _enabled(state) if k in BRANCHING]
        if branches:
            entry['branches'] = branches
        undecoded = sorted({k for _i, k in _enabled(state)
                            if k not in INERT and k not in BRANCHING and k not in WAITING})
        if undecoded:
            entry['not_decoded'] = undecoded
        if _self_disables(state):
            entry['disables_the_npc'] = True
        out.append(entry)
    return out


def _self_disables(state):
    """Whether a state switches its own object off or destroys it."""
    data = state['actionData']
    for i, kind in _enabled(state):
        fields = action_fields(data, i)
        if kind == 'DestroySelf':
            return True
        if kind == 'ActivateGameObject' and _owner_target(fields):
            if _word(fields.get('activate')) is False:
                return True
        if kind == 'DestroyObject' and _owner_target(fields):
            return True
    return False


# ------------------------------------------------------------------ scene shape


def _ancestors(scene, gid):
    """The object ids above one object, nearest first."""
    out, current = [], gid
    while True:
        tid = scene.go_transform.get(current)
        father = scene.transforms[tid]['m_Father']['m_PathID'] if tid else 0
        if not father or father not in scene.transforms:
            return out
        current = scene.transforms[father]['m_GameObject']['m_PathID']
        if current not in scene.gos:
            return out
        out.append(current)


def _descendants(scene, gid):
    """One object and everything under it."""
    out, stack = [], [gid]
    while stack:
        current = stack.pop()
        out.append(current)
        tid = scene.go_transform.get(current)
        for child in (scene.transforms[tid]['m_Children'] if tid else []):
            cid = child['m_PathID']
            if cid in scene.transforms:
                kid = scene.transforms[cid]['m_GameObject']['m_PathID']
                if kid in scene.gos:
                    stack.append(kid)
    return out


def _components(scene, gid, kind):
    out = []
    for component in scene.gos[gid]['m_Component']:
        cid = component['component']['m_PathID']
        if cid in scene.objects and scene.objects[cid][0] == kind:
            out.append((cid, scene.objects[cid][1]))
    return out


def _fsms(scene, gid):
    return {tree['fsm']['name']: (cid, tree['fsm'])
            for cid, tree in _components(scene, gid, 'PlayMakerFSM')}


def _gates(scene, gid, defaults):
    """The progression gates that decide whether this object survives OnEnable.

    `DeactivateIfPlayerdataFalse::OnEnable` switches the object off when the bool
    is clear and `...True` switches it off when the bool is set, so a gate on an
    ancestor removes the NPC just as surely as one on the NPC.
    """
    out = []
    for owner in [gid] + _ancestors(scene, gid):
        for kind in DEACTIVATORS:
            for _cid, tree in _components(scene, owner, kind):
                field = tree['boolName']
                hides_when = kind == 'DeactivateIfPlayerdataTrue'
                value = _truth(defaults[field]) if field in defaults else UNKNOWN
                entry = {'object': scene.gos[owner]['m_Name'], 'component': kind,
                         'field': field, 'hidden_when': hides_when,
                         'new_save_value': None if value is UNKNOWN else value,
                         'hides_on_new_save': None if value is UNKNOWN
                         else value == hides_when}
                if entry not in out:
                    out.append(entry)
    return out


def _animation(source, scene, gid):
    """Every sprite animation library under this NPC, with its clips."""
    out = []
    for owner in _descendants(scene, gid):
        for _cid, tree in _components(scene, owner, 'tk2dSpriteAnimator'):
            obj = source.ref(scene.file, tree['library'])
            library = source.read(obj)
            clips = [c for c in library['clips'] if c['frames']]
            default = library['clips'][tree['defaultClipId']]['name'] \
                if 0 <= tree['defaultClipId'] < len(library['clips']) else None
            out.append({
                'object': scene.gos[owner]['m_Name'],
                'source': source.sid(obj),
                'default_clip': default,
                'plays_automatically': bool(tree['playAutomatically']),
                'clips': [{'name': c['name'], 'fps': c['fps'], 'frames': len(c['frames']),
                           'wrap_mode': c['wrapMode'], 'loop_start': c['loopStart']}
                          for c in clips],
            })
    return out


# ------------------------------------------------------------------- one NPC


def npc(source, scene_name, scene, gid, fsms, defaults):
    """Everything P18 step 1 asks for about one NPC, from this scene alone."""
    convo_id, convo = fsms.get(CONVO_FSM, (None, None))
    control_id, control = fsms.get(CONTROL_FSM, (None, None))
    entry = {
        'scene': scene_name,
        'object': scene.gos[gid]['m_Name'],
        'source': source.sid(scene.file.objects[gid]),
        'parents': [scene.gos[g]['m_Name'] for g in reversed(_ancestors(scene, gid))],
        'position': [round(v, 4) for v in scene.point(gid)],
        'serialized_active': scene.active(gid),
        'fsms': sorted(fsms),
    }
    if control is not None:
        variables = fsm_variables(control)
        entry['control'] = {'source': source.sid(scene.file.objects[control_id]),
                            'fsm': CONTROL_FSM,
                            'parameters': {k: variables[k] for k in CONTROL_VARIABLES
                                           if k in variables}}
    entry['gates'] = _gates(scene, gid, defaults)
    entry['animation'] = _animation(source, scene, gid)
    # The clips the NPC plays on itself are the presentation list. Clips its own
    # FSMs play on the hero or on a child are not, and a clip no library under
    # the NPC carries is called out rather than quietly listed as needed.
    played, known = [], {c['name'] for a in entry['animation'] for c in a['clips']}
    for _cid, fsm in fsms.values():
        for state in fsm['states']:
            for clip in _clips(state):
                if clip['on_self'] and clip['clip'] not in played:
                    played.append(clip['clip'])
    entry['self_clips'] = played
    entry['clips_not_in_library'] = [c for c in played if c not in known]
    entry['dream_dialogue'] = _dream(source, scene, gid)

    if convo is not None:
        variables = fsm_variables(convo)
        origins = _origins(convo)
        entry['conversation'] = {
            'fsm': CONVO_FSM,
            'source': source.sid(scene.file.objects[convo_id]),
            'start_state': convo['startState'],
            'global_transitions': {t['fsmEvent']['name']: t['toState']
                                   for t in convo['globalTransitions']},
            'title_key': _title_key(convo),
            'variable_origins': origins,
            'selection': _selection(convo, origins),
            'states': _states(convo, variables, origins),
            'new_save_startup': _walk(convo, convo['startState'], defaults,
                                      variables, origins),
        }
        choice = _first_choice(convo, origins)
        if choice:
            entry['conversation']['new_save_dialogue'] = dict(
                _walk(convo, choice, defaults, variables, origins), entered_at=choice)

    other = []
    for name, (cid, fsm) in sorted(fsms.items()):
        if name in (CONVO_FSM, CONTROL_FSM):
            continue
        other.append({'fsm': name, 'source': source.sid(scene.file.objects[cid]),
                      'start_state': fsm['startState'],
                      'new_save_startup': _walk(fsm, fsm['startState'], defaults,
                                                fsm_variables(fsm), _origins(fsm))})
    if other:
        entry['other_fsms'] = other
    return entry


def _title_key(fsm):
    """The Titles-sheet prefix the conversation raises as the NPC's name plate."""
    for state in fsm['states']:
        data = state['actionData']
        for i, kind in _enabled(state):
            if kind != 'SetFsmString':
                continue
            fields = action_fields(data, i)
            if _word(fields.get('variableName')) == 'Area Event':
                value = _word(fields.get('setValue'))
                if isinstance(value, str) and value:
                    return value
    return None


def _first_choice(fsm, origins):
    """The state the conversation enters to pick a line.

    The template is always Idle -> a facing and dialogue-box preamble -> one
    choice state, and the preamble reads the hero's position and the NPC's scale,
    which no static walk can resolve. Those preamble states carry no decoded
    branch, so they are not in the selection at all; the first selection state
    that can reach a state which speaks is the choice, and the new-save walk
    starts there rather than in the preamble.
    """
    for entry in _selection(fsm, origins):
        if entry['leads_to_lines_in']:
            return entry['state']
    return None


def _conversation_entries(states, variables, walk):
    """One walk's spoken entries, each keeping the PlayerData its state writes.

    PlayMaker runs a state's `SetPlayerDataBool` as it enters, before the
    DialogueBox call beside it has spoken, so the pairing is per entry. The
    report's walk keeps the lines and the writes in separate lists.
    """
    return [dict(line, state=step['state'], writes=_writes(states[step['state']]))
            for step in walk['path'] for line in _lines(states[step['state']], variables)]


def fresh_save_conversation(source, scene, gid, defaults=None):
    """The entries one NPC's `Conversation Control` speaks on a fresh save.

    The walk starts at the choice state for the reason `_first_choice` records.
    """
    return conversation_chain(source, scene, gid, defaults, limit=1)[0]['entries']


def conversation_chain(source, scene, gid, defaults=None, limit=MAX_CONVERSATIONS):
    """Every conversation one NPC speaks in turn, starting from a fresh save.

    `Conversation Control` picks its line from PlayerData and the state it picks
    then writes the flags that change the next pick, so what a player hears is a
    chain rather than one line: walk, apply what the walk wrote, walk again. The
    chain stops at the first conversation that leaves PlayerData exactly as it
    found it, because from there the source's own answer can no longer change;
    that last conversation is the one it repeats for the rest of the game.

    Elderbug is the worked example: the intro writes metElderbug, the second
    talk is History 1 and writes elderbugHistory1, and the third is the generic
    bench line, which writes nothing and so repeats forever.
    """
    if defaults is None:
        defaults = playerdata_defaults(source.directory / 'Managed/Assembly-CSharp.dll')
    name = scene.gos[gid]['m_Name']
    _cid, convo = _fsms(scene, gid)[CONVO_FSM]
    variables = fsm_variables(convo)
    origins = _origins(convo)
    start = _first_choice(convo, origins)
    if start is None:
        raise LookupError(f'{name} chooses no line from PlayerData')
    states = {s['name']: s for s in convo['states']}
    playerdata = dict(defaults)
    chain, spoken = [], []
    while True:
        walk = _walk(convo, start, playerdata, variables, origins)
        entries = _conversation_entries(states, variables, walk)
        if not entries:
            raise LookupError(f'{name} speaks nothing on conversation {len(chain)}; '
                              f"the walk stops in {walk.get('undecided_in') or walk['settles_in']}")
        keys = [(entry.get('key'), entry.get('sheet')) for entry in entries]
        if keys in spoken:
            raise LookupError(f'{name} returns to conversation {spoken.index(keys)} '
                              f'after {len(chain)}; a saturating cursor cannot express a cycle')
        spoken.append(keys)
        # A write whose value is chosen at runtime cannot be replayed here, and
        # guessing it would silently pick one arm of the next conversation.
        changed = False
        for write in walk['writes']:
            value = write.get('value')
            if not isinstance(value, (int, bool)) or isinstance(value, str):
                raise LookupError(f'{name} writes {write["field"]} with a runtime value')
            if playerdata.get(write['field']) != value:
                playerdata[write['field']] = value
                changed = True
        chain.append({'entries': entries, 'writes': walk['writes'], 'terminal': not changed,
                      'states': [step['state'] for step in walk['path']],
                      'settles_in': walk['settles_in'], 'stops_in': walk.get('undecided_in'),
                      'not_decoded': walk.get('not_decoded')})
        if not changed:
            return chain
        if len(chain) > limit:
            raise LookupError(f'{name} speaks more than {limit} conversations before repeating')


def _dream(source, scene, gid):
    """The dream nail line each npc_dream_dialogue under this NPC speaks."""
    out = []
    for owner in _descendants(scene, gid):
        for _cid, fsm in _fsms(scene, owner).values():
            if fsm['name'] != DREAM_FSM:
                continue
            variables = fsm_variables(fsm)
            out.append({'object': scene.gos[owner]['m_Name'],
                        'key': variables.get('Convo Name'),
                        'sheet': variables.get('Sheet Name'),
                        'serialized_active': scene.active(owner)})
    return out


# ------------------------------------------------------------------- the report


def admitted_scenes():
    scenes = json.loads(ADMITTED.read_text())['scenes']
    return [(s['scene_name'], s['file']) for s in scenes]


def _present(entry):
    """Whether this NPC exists on a new save, from the gates and its own FSMs.

    Three things can remove an NPC: a Deactivate component on it or an ancestor,
    the scene shipping it inactive, and its own FSM walking into a state that
    switches the object off. All three are checked; an undecidable gate returns
    None rather than a guess.
    """
    reasons = []
    for gate in entry['gates']:
        if gate['hides_on_new_save'] is None:
            return None, [f"{gate['field']} is not a new-save default"]
        if gate['hides_on_new_save']:
            reasons.append(f"{gate['component']} on {gate['object']}: {gate['field']}")
    if not entry['serialized_active']:
        reasons.append('the scene ships it inactive')
    walks = [(entry['conversation']['fsm'], entry['conversation']['new_save_startup'])
             for _ in [0] if 'conversation' in entry]
    walks += [(f['fsm'], f['new_save_startup']) for f in entry.get('other_fsms', [])]
    for name, walk in walks:
        if walk.get('disables_the_npc'):
            reasons.append(f"{name} settles in {walk['settles_in']}, which "
                           'switches the object off')
    return (not reasons), reasons


def collect():
    source = Source()
    catalog = Catalog(source)
    defaults = playerdata_defaults(source.directory / 'Managed/Assembly-CSharp.dll')
    key = _encryption_key(source.directory / 'Managed')
    admitted = admitted_scenes()

    npcs, skipped, parsed = [], [], []
    for scene_name, file in admitted:
        if catalog.files.get(scene_name) != file:
            raise LookupError(f'{scene_name} is not {file} in BuildSettings')
        # Parsed once and dropped: holding forty five scenes at once is not
        # needed and every NPC answer comes from its own scene.
        scene = Scene(source, file)
        parsed.append(scene_name)
        owners, dreamers = {}, {}
        for _cid, (kind, tree) in scene.objects.items():
            if kind != 'PlayMakerFSM':
                continue
            gid = tree['m_GameObject']['m_PathID']
            if tree['fsm']['name'] in (CONTROL_FSM, CONVO_FSM):
                owners[gid] = True
            elif tree['fsm']['name'] == DREAM_FSM:
                above = _ancestors(scene, gid)
                dreamers[above[0] if above else gid] = True
        for gid in sorted(owners):
            npcs.append(npc(source, scene_name, scene, gid, _fsms(scene, gid), defaults))
        # An object that only answers the Dream Nail is not an NPC by this
        # recognizer, but Dirtmouth has one the plan names, so they are recorded
        # rather than silently dropped.
        for gid in sorted(dreamers):
            if gid in owners:
                continue
            skipped.append({'scene': scene_name, 'object': scene.gos[gid]['m_Name'],
                            'source': source.sid(scene.file.objects[gid]),
                            'position': [round(v, 4) for v in scene.point(gid)],
                            'serialized_active': scene.active(gid),
                            'gates': _gates(scene, gid, defaults),
                            'fsms': sorted(_fsms(scene, gid)),
                            'dream_dialogue': _dream(source, scene, gid),
                            'reason': 'dream nail only: no npc_control and no '
                                      'Conversation Control'})
        source.files.pop(file, None)

    for entry in npcs:
        present, reasons = _present(entry)
        entry['present_on_new_save'] = present
        entry['absent_because'] = reasons
        startup = entry.get('conversation', {}).get('new_save_startup', {})
        if present is None or (present and startup.get('settles_in') is None):
            entry['talkable_on_new_save'] = None
            if present:
                reasons.append(f"present; where {entry['conversation']['fsm']} settles "
                               'is undecided, see new_save_startup')
        else:
            entry['talkable_on_new_save'] = bool(present
                                                 and startup.get('accepts_convo_start'))
            if present and not entry['talkable_on_new_save']:
                reasons.append(f"present but {entry['conversation']['fsm']} settles in "
                               f"{startup['settles_in']!r}, which does not answer "
                               'CONVO START')

    text, missing = _text(source, key, npcs, skipped)
    report = {
        'format': 'HKNPC01',
        'admitted_scenes': len(admitted),
        'scenes_parsed': sorted(parsed),
        'scenes_with_npcs': sorted({e['scene'] for e in npcs}),
        'recognizer': 'a GameObject carrying the npc_control or Conversation '
                      'Control PlayMaker FSM',
        'selection_order': "each selection state's branches are in declaration "
                           'order and the first one that causes a transition wins',
        'new_save_source': 'PlayerData::SetupNewPlayerData',
        'language_sheets': {name: len(entries) for name, entries in sorted(text.items())},
        'npc_count': len(npcs),
        'present_on_new_save': sorted(f"{e['scene']}/{e['object']}" for e in npcs
                                      if e['present_on_new_save']),
        'talkable_on_new_save': sorted(f"{e['scene']}/{e['object']}" for e in npcs
                                       if e['talkable_on_new_save']),
        'npcs': npcs,
        'dream_nail_only': skipped,
        'dialogue': _dialogue(text, npcs, skipped),
        'missing_keys': missing,
        'limitations': [
            'Extraction only. No NPC, prompt, dialogue box or conversation exists '
            'in the port today, and nothing here is cooked or bound to the guest.',
            'Only the English sheets are decrypted. The other eleven languages are '
            'not read and not checked for disagreement.',
            'Dialogue selection is read as a priority list because PlayMaker stops '
            'a state at the first action that causes a transition '
            '(FsmState::ActivateActions). That was read from the shipped '
            'PlayMaker.dll, not observed at runtime.',
            'The new-save walks evaluate only the decoded branch actions and stop '
            'at the first action they cannot decide, which is reported per walk as '
            'not_decoded or undecided_in. They are a static evaluation of '
            'SetupNewPlayerData, not a trace of the running game.',
            'SendEventByName is treated as a branch whenever the event it sends '
            'matches a transition in the current state or a global transition. Its '
            'event target is not resolved, so an event aimed at another FSM that '
            'happens to share a name would be read as a local branch.',
            'The preamble states between Idle and the choice state read the hero '
            'position and the NPC scale, so the new-save dialogue walk starts at '
            'the choice state and skips the facing and dialogue-box states.',
            'Conversations that branch on a player choice (the YN dialogue box) '
            'are recorded as states and transitions. The box itself, its prompts '
            'and the cancel path are not extracted.',
            'Animation clip wrap modes are the raw tk2d serialized ints; the enum '
            'names are not read from the assembly.',
            'Idle motion, chatter, voice, the prompt marker art and the interaction '
            'ranges are not extracted; only the npc_control parameters and the '
            'clip inventory are.',
            'Quest rewards, item hand-offs and scene changes an NPC triggers are '
            'visible as PlayerData writes and transitions only. Nothing that a '
            'branch spawns as a prefab is followed.',
            'The language sheet reader, its AES key derivation and the new-save '
            'PlayerData are imported from items.py. Two extractors now decrypt '
            'language sheets in different ways (shops.py carries its own AES and a '
            'literal key); a shared reader would be the right home.',
        ],
    }
    dump(ROOT / '.hkpsx/npc-catalog.json', report)
    return report


def _text(source, key, npcs, skipped):
    """Every sheet the admitted NPCs name, decrypted once each."""
    wanted = set()
    for entry in npcs + skipped:
        for dream in entry['dream_dialogue']:
            if dream['sheet']:
                wanted.add(dream['sheet'])
        for state in entry.get('conversation', {}).get('states', []):
            for line in state['lines']:
                if isinstance(line['sheet'], str) and line['sheet']:
                    wanted.add(line['sheet'])
    wanted.add(TITLE_SHEET)
    sheets = {name: language_sheet(source, name, key) for name in sorted(wanted)}
    missing = []
    for entry in npcs + skipped:
        for dream in entry['dream_dialogue']:
            dream['text'] = _lookup(sheets, dream, missing, entry)
        for state in entry.get('conversation', {}).get('states', []):
            for line in state['lines']:
                line['text'] = _lookup(sheets, line, missing, entry)
        conversation = entry.get('conversation', {})
        for walk in ('new_save_startup', 'new_save_dialogue'):
            for line in conversation.get(walk, {}).get('lines', []):
                line['text'] = _lookup(sheets, line, missing, entry)
        prefix = conversation.get('title_key')
        if prefix:
            conversation['title'] = {part: sheets[TITLE_SHEET].get(f'{prefix}_{part}')
                                     for part in ('SUPER', 'MAIN', 'SUB')}
    return sheets, missing


def _lookup(sheets, line, missing, entry):
    sheet, key = line.get('sheet'), line.get('key')
    if not isinstance(sheet, str) or not isinstance(key, str) or not sheet or not key:
        source = line.get('key_from_variable') or line.get('sheet_from_variable')
        reason = 'the key or the sheet is chosen at runtime'
        if source:
            reason += f'; it comes from the FSM variable {source!r}'
        _miss(missing, entry, key, sheet, reason)
        return None
    if key not in sheets.get(sheet, {}):
        _miss(missing, entry, key, sheet, f'EN_{sheet} has no such entry')
        return None
    return sheets[sheet][key]


def _miss(missing, entry, key, sheet, reason):
    record = {'npc': entry['object'], 'scene': entry['scene'], 'key': key,
              'sheet': sheet, 'reason': reason}
    if record not in missing:
        missing.append(record)


def _dialogue(sheets, npcs, skipped):
    """Every key the admitted NPCs can speak, with its decrypted text."""
    out = {}
    for entry in npcs + skipped:
        lines = list(entry['dream_dialogue'])
        for state in entry.get('conversation', {}).get('states', []):
            lines.extend(state['lines'])
        for line in lines:
            if isinstance(line.get('key'), str) and isinstance(line.get('sheet'), str):
                out.setdefault(f"{line['sheet']}/{line['key']}", line.get('text'))
    return dict(sorted(out.items()))


# ------------------------------------------------------------------ the summary


def _term(term):
    reads = term.get('reads') or ([term['variable']] if term.get('variable') else [])
    name = term.get('field') or '/'.join(str(r) for r in reads)
    return name if term['expect'] else 'not ' + name


def _operand(word, value):
    return f'{word}={value}' if isinstance(word, str) else str(value)


def _condition(took):
    branch = took['branch']
    if branch['action'] == 'IntSwitch':
        return f"{branch['variable']} == {took.get('case')}"
    if branch['action'] == 'IntCompare':
        left, right = took.get('compared', ['?', '?'])
        return f'{_operand(branch["left"], left)} vs {_operand(branch["right"], right)}'
    terms = ' and '.join(_term(t) for t in branch.get('terms', []))
    if not terms:
        return branch['action']
    return terms if took.get('matched') else f'not ({terms})'


def _walk_summary(walk, indent='    '):
    lines = []
    for step in walk.get('path', []):
        took = step['took']
        if took:
            lines.append(f"{indent}{step['state']:<20} {_condition(took)} "
                         f"-> {took['event']} -> {took['to_state']}")
        else:
            lines.append(f"{indent}{step['state']}")
    for line in walk.get('lines', []):
        lines.append(f"{indent}[{line['sheet']}/{line['key']}]")
        for paragraph in (line['text'] or '').split('<page>'):
            lines.append(indent + '  ' + paragraph.strip().replace('\n', ' '))
    if walk.get('undecided_in'):
        stopper = walk.get('not_decoded') or walk.get('undecided_branch', {}).get('action')
        lines.append(f'{indent}(stopped in {walk["undecided_in"]}: {stopper})')
    return lines


def summary(report):
    lines = [f"{report['npc_count']} NPCs in {len(report['scenes_with_npcs'])} of the "
             f"{report['admitted_scenes']} admitted scenes, "
             f"{len(report['dialogue'])} dialogue keys decrypted. "
             f"{len(report['talkable_on_new_save'])} can be talked to on a new save.", '']
    for scene in sorted({e['scene'] for e in report['npcs']}):
        lines.append(f'{scene}:')
        for entry in report['npcs']:
            if entry['scene'] != scene:
                continue
            title = (entry.get('conversation', {}).get('title') or {}).get('MAIN')
            here = {True: 'speaks today', False: 'not today',
                    None: 'undecided'}[entry['talkable_on_new_save']]
            lines.append(f"  {entry['object']:<24} {title or '':<10} "
                         f"x={entry['position'][0]:>7.2f}  {here}")
            for reason in entry['absent_because']:
                lines.append(f'      {reason}')
        lines.append('')
    alongside = [e for e in report['dream_nail_only']
                 if e['scene'] in report['scenes_with_npcs']]
    lines.append(f"{len(report['dream_nail_only'])} objects answer only the Dream Nail, "
                 f'{len(alongside)} of them in a scene that also has NPCs:')
    for entry in alongside:
        keys = ', '.join(str(d['key']) for d in entry['dream_dialogue'])
        lines.append(f"  {entry['scene']}/{entry['object']}: {keys}")
    lines.append('')
    lines.append('What Dirtmouth says on a new save:')
    for entry in report['npcs']:
        if entry['scene'] != 'Town' or not entry['talkable_on_new_save']:
            continue
        walk = entry['conversation'].get('new_save_dialogue')
        if not walk:
            lines.append(f"  {entry['object']}: no PlayerData choice state; its lines "
                         'are in the states listed in the report.')
            continue
        lines.append(f"  {entry['object']}, from {walk['entered_at']!r}:")
        lines.extend(_walk_summary(walk, '      '))
    lines.append('')
    if report['missing_keys']:
        lines.append(f"{len(report['missing_keys'])} keys could not be resolved; "
                     'see missing_keys in the report.')
    return '\n'.join(lines)


if __name__ == '__main__':
    print(summary(collect()))
