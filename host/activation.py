"""PlayerData gates the original evaluates on scene load, answered at cook time.

Three authored shapes hide an object on a fresh save, and the port runs none of
them, so it shows objects the original removes:

  * the `DeactivateIfPlayerdataFalse` and `DeactivateIfPlayerdataTrue`
    components, whose `OnEnable` reads one bool and calls `SetActive(false)`;
  * the `deactivate_ifnot_playerdatabool` FSM template, which waits one frame,
    reads one bool and deactivates its own owner (Crossroads_38's Fat Grub King
    carries one as an FSM called simply "FSM");
  * the `activate_if_pd_bool` template, which reads one bool, compares it with a
    per-instance polarity flag and activates or deactivates every direct child
    of its owner (64 instances are named `Activate Infected`);
  * an FSM whose fresh-save path reads one bool and reaches a state that only
    calls `DestroySelf` without detaching children. Dirtmouth's `Bretta Bench`
    is one: until Bretta is rescued its `Control` FSM destroys the bench seat,
    and with it the `Blush` sprite the port otherwise drew floating over the
    bench.

Nothing here matches on those names. The recognizer walks the FSM the component
actually runs and checks every field it reads, so a fourth instance of the same
authored shape under a different name is covered and a renamed FSM that does
something else is not.

Evaluating them once, against `PlayerData::SetupNewPlayerData`, is a
simplification of what the original does, which is to re-evaluate on every scene
load. It is exactly equivalent only while no gate flag can change, and today
none can: the port has no acquisition for any of them. A save that set one would
need the runtime script executor and a renderer that can drop a draw, and this
module would then be the wrong place for the answer.

Only removal is applied. The `Activate Infected` FSM also activates children on
the other branch, and an FSM may activate its own owner; neither is acted on,
because adding content the port does not draw today is a separate question. Both
are counted in `report()`.

The failure mode is fixed in one direction. A gate applies only when its whole
shape matches and its bool is a known fresh-save value, so anything unfamiliar
leaves the object exactly where it is and lands in `report()['refused']`. A
false positive would delete part of the world silently; a false negative leaves
things as they are today. `report()['inert']` is separate: those are recognized
gates whose fresh-save branch is simply not wired up, which is an answer.
"""
from functools import lru_cache

from focus import action_fields
from items import playerdata_defaults

# FSM templates are shared by many instances across many scenes, and reading one
# is a typetree read, so they are resolved once per process.
_TEMPLATES = {}

# Components whose OnEnable deactivates the owner, keyed by the bool value that
# makes them fire. ActivateIfPlayerdataTrue only ever activates, and
# DeactivateIfPlayerdataFalseDelayed hides the object after a delay it is
# visible for, so neither is a load-time removal; both are reported instead.
GATE_COMPONENTS = {'DeactivateIfPlayerdataFalse': False, 'DeactivateIfPlayerdataTrue': True}
REPORTED_COMPONENTS = ('ActivateIfPlayerdataTrue', 'DeactivateIfPlayerdataFalseDelayed')
# The actions a pure load-time gate is built from. An FSM that uses anything
# else does more than gate activation, so it is never walked and never removes.
GATE_VOCABULARY = frozenset({'NextFrameEvent', 'PlayerDataBoolTest', 'BoolTest', 'GetOwner',
                             'ActivateGameObject', 'ActivateAllChildren', 'DestroySelf', 'FindChild'})
ACTIVATION_ACTIONS = frozenset({'ActivateGameObject', 'ActivateAllChildren'})
# The walk also understands these on the path to a DestroySelf, and only there:
# GetPosition only stores into variables, and FloatInRange is a runtime test
# (typically on the hero's position) whose events are accepted only when each
# one is the event the fresh-save branch sends anyway, so the answer holds for
# every position. Nothing after a DestroySelf runs, so the states the path never
# enters may hold anything; an activation gate stays inside GATE_VOCABULARY.
DESTROY_PATH_VOCABULARY = frozenset({'GetPosition', 'FloatInRange'})
# Both recognized shapes are three and six states. A larger FSM sharing this
# vocabulary is something else and is refused rather than walked.
GATE_STATE_LIMIT = 6


class Refused(Exception):
    """This FSM or component is not one of the recognized gates, so it hides nothing."""


class Inert(Exception):
    """A recognized gate whose fresh-save branch sends no event, so it never fires.

    This is an answer rather than a refusal: the same authored shape carries
    both senses, and the sense that is not wired up is how the original says
    "leave it alone".
    """


@lru_cache(None)
def _fresh_save(assembly):
    return playerdata_defaults(assembly)


def fresh_save_bool(playerdata, name):
    """One PlayerData bool as a new save starts it, or a refusal."""
    if not isinstance(name, str) or name not in playerdata:
        raise Refused(f'{name!r} is not a field SetupNewPlayerData starts')
    value = playerdata[name]
    # SetupNewPlayerData writes bools as 0 or 1. Anything else is an int, a
    # float or a value the CIL reader could not fold, and is not a gate input.
    if value not in (0, 1) or isinstance(value, float):
        raise Refused(f'{name} starts as {value!r}, which is not a bool')
    return bool(value)


def _enabled(state):
    """One state's enabled actions as (short name, index), in authored order."""
    d = state['actionData']
    return [(n.rsplit('.', 1)[-1], i) for i, n in enumerate(d['actionNames']) if d['actionEnabled'][i]]


def _vocabulary(fsm):
    return {kind for state in fsm['states'] for kind, _ in _enabled(state)}


def _gate_like(actions):
    """Whether an FSM reads a PlayerData bool and changes what is active at all."""
    return 'PlayerDataBoolTest' in actions and bool(actions & (ACTIVATION_ACTIONS | {'DestroySelf'}))


def _transitions(state):
    out = {}
    for t in state.get('transitions', []):
        event = t['fsmEvent']['name'] if isinstance(t.get('fsmEvent'), dict) else t.get('eventName')
        out[event] = t['toState']
    return out


def _branch(name):
    if not isinstance(name, str):
        raise Refused('a gate branch is not an event name')
    if not name:
        raise Inert('the fresh-save branch of this gate sends no event')
    return name


def _resolve(value, variables):
    """A compact PlayMaker scalar, with an FSM variable substituted in."""
    if not isinstance(value, dict) or 'useVariable' not in value:
        raise Refused('a gate field is not a compact scalar')
    if not value['useVariable']:
        return value['value']
    if value['name'] not in variables:
        raise Refused(f'a gate reads undeclared variable {value["name"]!r}')
    return variables[value['name']]


def _declared(fsm):
    """An FSM's variables as name to (value, overridable by an instance)."""
    out = {}
    for group in fsm['variables'].values():
        if not isinstance(group, list):
            continue
        for v in group:
            if not isinstance(v, dict) or 'name' not in v:
                continue
            if v['name'] in out and out[v['name']][0] != v.get('value'):
                raise Refused(f'a gate declares {v["name"]!r} twice with different values')
            out[v['name']] = (v.get('value'), bool(v.get('showInInspector')))
    return out


def instantiate(source, file, component):
    """The FSM one PlayMakerFSM component actually runs, and its variable values.

    A component with an `fsmTemplate` does not run its own serialized states:
    `PlayMakerFSM.InitTemplate` rebuilds the FSM from the template's copy and
    keeps only the instance's variables, and `OverrideVariableValues` applies
    those by name over the template's, for the template variables the template
    exposes to the inspector. That is how one `activate_if_pd_bool` template
    serves both polarities of `Activate Infected`, so reading the instance's own
    copy would read a value the game never uses.
    """
    reference = component.get('fsmTemplate') or {}
    if not reference.get('m_PathID'):
        declared = _declared(component['fsm'])
        return component['fsm'], {name: value for name, (value, _) in declared.items()}, None
    try:
        obj = source.ref(file, reference)
        key = (obj.assets_file.name, obj.path_id)
        if key not in _TEMPLATES:
            asset = source.read(obj)
            _TEMPLATES[key] = (asset['fsm'], asset.get('m_Name'))
        template, name = _TEMPLATES[key]
    except Refused:
        raise
    except Exception as ex:
        raise Refused(f'the FSM template could not be read: {ex}')
    overrides = _declared(component['fsm'])
    values = {}
    for variable, (value, exposed) in _declared(template).items():
        values[variable] = overrides[variable][0] if exposed and variable in overrides else value
    return template, values, name


def _walk(fsm, variables, playerdata):
    """Run one gate FSM against the fresh save, or refuse.

    The walk is the FSM's own semantics rather than a name match: every state on
    the taken path must hold only actions this vocabulary understands with every
    field checked, and each branch must resolve to exactly one event, so the
    walk either reaches one activation action or refuses. States off the path
    cannot change the answer, because the terminal state has no transitions out
    and the FSM declares no global transition into one.
    """
    if fsm.get('globalTransitions'):
        raise Refused('a gate with a global transition can leave its result')
    states = {}
    for state in fsm['states']:
        if state['name'] in states:
            raise Refused('a gate declares one state name twice')
        states[state['name']] = state
    if len(states) > GATE_STATE_LIMIT:
        raise Refused(f'{len(states)} states is larger than either recognized gate')
    name, owner_variable, seen, fields = fsm.get('startState'), None, set(), []
    # `FindChild` on the owner, by the variable it stores into: Dirtmouth's
    # `Check Opened` buildings find their `open` and `closed` children and then
    # set each one, so its terminal state holds one ActivateGameObject per child.
    found = {}
    # Events a runtime test earlier in the current state may send before the
    # fresh-save branch is reached.
    maybe = set()
    while True:
        if name in seen:
            raise Refused('a gate loops before it activates anything')
        seen.add(name)
        if name not in states:
            raise Refused(f'a gate branches to unknown state {name!r}')
        state = states[name]
        actions = _enabled(state)
        d = state['actionData']
        pending = None
        maybe.clear()
        named = {}
        for position, (kind, index) in enumerate(actions):
            last = position == len(actions) - 1
            f = action_fields(d, index, objects=True)
            if kind == 'FindChild':
                target, store = f.get('gameObject'), f.get('storeResult')
                child = _resolve(f.get('childName'), variables) if isinstance(f.get('childName'), dict) else f.get('childName')
                if not isinstance(target, dict) or target.get('ownerOption') != 0:
                    raise Refused('FindChild searches something other than the owner')
                if not isinstance(child, str) or not child:
                    raise Refused('FindChild names no child')
                if not isinstance(store, dict) or not store.get('useVariable') or not store.get('name'):
                    raise Refused('FindChild does not store into a variable')
                found[store['name']] = child
                continue
            if kind == 'ActivateGameObject' and isinstance(f.get('gameObject'), dict) \
                    and f['gameObject'].get('ownerOption') == 1:
                target = f['gameObject'].get('gameObject')
                if not isinstance(target, dict) or not target.get('useVariable') or target.get('name') not in found:
                    raise Refused('a gate activates an object it did not find among its own children')
                # `recursive` only changes whether children's own flags are
                # rewritten; a deactivated parent hides its subtree either way.
                if f.get('everyFrame') or f.get('resetOnExit'):
                    raise Refused('a gate child activation is repeated or undone on exit')
                activate = _resolve(f.get('activate'), variables)
                if not isinstance(activate, bool):
                    raise Refused('a gate child activation does not carry a plain flag')
                named[found[target['name']]] = activate
                if last:
                    return 'named', named, fields
                continue
            if named:
                raise Refused('a gate state sets found children and then does something else')
            if kind == 'GetOwner':
                store = f.get('storeGameObject')
                if not isinstance(store, dict) or not store.get('useVariable') or not store.get('name'):
                    raise Refused('GetOwner does not store into a variable')
                owner_variable = store['name']
            elif kind == 'NextFrameEvent':
                # It fires from OnUpdate, so any later action in this state
                # would still run on entry. The recognized shapes have none.
                if not last:
                    raise Refused('NextFrameEvent is not the last action of its state')
                send = f.get('sendEvent')
                if not isinstance(send, str) or not send:
                    raise Refused('NextFrameEvent sends no event')
                pending = send
            elif kind == 'PlayerDataBoolTest':
                field = _resolve(f.get('boolName'), variables)
                value = fresh_save_bool(playerdata, field)
                fields.append(field)
                pending = _branch(f.get('isTrue') if value else f.get('isFalse'))
            elif kind == 'BoolTest':
                if f.get('everyFrame'):
                    raise Refused('a gate BoolTest repeats every frame')
                value = _resolve(f.get('boolVariable'), variables)
                pending = _branch(f.get('isTrue') if value else f.get('isFalse'))
            elif kind == 'ActivateGameObject':
                if not last:
                    raise Refused('ActivateGameObject is not the last action of its state')
                target = f.get('gameObject')
                # FsmOwnerDefault.UseOwner is 0; anything else names an object
                # this pass cannot resolve to a scene id.
                if not isinstance(target, dict) or target.get('ownerOption') != 0:
                    raise Refused('a gate activates something other than its own owner')
                if f.get('everyFrame') or f.get('resetOnExit'):
                    raise Refused('a gate activation is repeated or undone on exit')
                return 'self', bool(_resolve(f.get('activate'), variables)), fields
            elif kind == 'ActivateAllChildren':
                if not last:
                    raise Refused('ActivateAllChildren is not the last action of its state')
                target = f.get('gameObject')
                if not isinstance(target, dict) or not target.get('useVariable') \
                        or target.get('name') != owner_variable:
                    raise Refused('a gate activates the children of something other than its owner')
                activate = f.get('activate')
                if not isinstance(activate, bool):
                    raise Refused('ActivateAllChildren does not carry a plain flag')
                return 'children', activate, fields
            elif kind == 'GetPosition':
                if f.get('everyFrame'):
                    raise Refused('a gate GetPosition repeats every frame')
            elif kind == 'FloatInRange':
                if f.get('everyFrame'):
                    raise Refused('a gate FloatInRange repeats every frame')
                for key in ('trueEvent', 'falseEvent'):
                    event = f.get(key)
                    if not isinstance(event, str):
                        raise Refused('a runtime test branch is not an event name')
                    if event:
                        maybe.add(event)
            elif kind == 'DestroySelf':
                if not last:
                    raise Refused('DestroySelf is not the last action of its state')
                if maybe:
                    raise Refused('a runtime test can leave the destroying state first')
                if _resolve(f.get('detachChildren'), variables):
                    raise Refused('a gate that detaches its children leaves them in the world')
                return 'destroy', False, fields
            else:
                raise Refused(f'{kind} is not part of a recognized gate')
            if pending is not None:
                break
        if pending is None:
            raise Refused(f'gate state {name!r} activates nothing and sends no event')
        if maybe - {pending}:
            raise Refused(f'a runtime test in gate state {name!r} can leave it by another event')
        transitions = _transitions(state)
        if pending not in transitions:
            raise Refused(f'gate state {name!r} has no transition for {pending}')
        name = transitions[pending]


class Gates:
    """Which objects of one scene the original's load-time gates remove.

    `off` is the answer `Scene.active` needs: the GameObject ids a gate turns
    off on a fresh save. Everything else here exists so the blast radius can be
    read and reviewed rather than trusted.
    """

    def __init__(self, scene):
        self.scene = scene
        self.off = set()
        self.removed = []
        self.activates = []
        self.refused = []
        self.inert = []
        self.other_fsms = 0
        playerdata = _fresh_save(str(scene.source.directory / 'Managed' / 'Assembly-CSharp.dll'))
        children = {}
        for t in scene.transforms.values():
            children.setdefault(t['m_Father']['m_PathID'], []).append(t['m_GameObject']['m_PathID'])
        for sid, (typename, tree) in scene.objects.items():
            gid = tree.get('m_GameObject', {}).get('m_PathID') if isinstance(tree, dict) else None
            if gid not in scene.gos:
                continue
            if typename in REPORTED_COMPONENTS:
                self._refuse(sid, gid, typename, f'{typename} is not a load-time removal')
            elif typename in GATE_COMPONENTS:
                self._component(sid, gid, typename, tree, playerdata)
            elif typename == 'PlayMakerFSM':
                self._fsm(sid, gid, tree, playerdata, children)

    def _name(self, gid):
        return self.scene.gos[gid]['m_Name']

    def _authored_active(self, gid):
        """What `Scene.active` answered before the gates, so the report can say
        which removals actually change the cooked world."""
        scene = self.scene
        while True:
            if gid not in scene.gos or gid not in scene.go_transform or not scene.gos[gid]['m_IsActive']:
                return False
            father = scene.transforms[scene.go_transform[gid]]['m_Father']['m_PathID']
            if not father:
                return True
            if father not in scene.transforms:
                return False
            gid = scene.transforms[father]['m_GameObject']['m_PathID']

    def _refuse(self, sid, gid, gate, reason):
        self.refused.append({'source': sid, 'object': self._name(gid), 'gate': gate, 'reason': reason})

    def _remove(self, sid, gid, gate, field, via=None):
        self.off.add(gid)
        row = {'source': sid, 'object_id': gid, 'object': self._name(gid), 'gate': gate, 'field': field}
        if via is not None:
            row['child_of'] = via
        self.removed.append(row)

    def _component(self, sid, gid, typename, tree, playerdata):
        # OnEnable never runs on a disabled component, so a disabled one is not
        # a gate at all.
        if not tree.get('m_Enabled'):
            return self._refuse(sid, gid, typename, 'the component is disabled')
        field = tree.get('boolName')
        try:
            value = fresh_save_bool(playerdata, field)
        except Refused as refusal:
            return self._refuse(sid, gid, typename, str(refusal))
        if value is GATE_COMPONENTS[typename]:
            self._remove(sid, gid, typename, field)

    def _fsm(self, sid, gid, tree, playerdata, children):
        # The serialized copy is only the cheap filter that keeps this pass off
        # the thousands of ordinary FSMs. The answer comes from the FSM the
        # component actually runs, which is checked again below, so a copy that
        # has drifted from its template can only cost a gate, never invent one.
        actions = _vocabulary(tree['fsm'])
        if not _gate_like(actions):
            self.other_fsms += 1
            return
        gate = tree['fsm']['name']
        # Only a destroying FSM may hold other actions, and only off its path.
        if not actions <= GATE_VOCABULARY and 'DestroySelf' not in actions:
            return self._refuse(sid, gid, gate, 'the FSM does more than gate activation: '
                                                + ', '.join(sorted(actions - GATE_VOCABULARY)))
        if not tree.get('m_Enabled'):
            return self._refuse(sid, gid, gate, 'the FSM component is disabled')
        if gid not in self.scene.go_transform:
            return self._refuse(sid, gid, gate, 'the gate owner has no transform in this scene')
        try:
            fsm, variables, template = instantiate(self.scene.source, self.scene.file, tree)
            # The template names the authored shape, and several differently
            # named FSMs share one, so it is the identifier worth reporting.
            gate = f'{gate} ({template})' if template else gate
            actions = _vocabulary(fsm)
            if not _gate_like(actions) or (not actions <= GATE_VOCABULARY and 'DestroySelf' not in actions):
                raise Refused('the FSM it runs is not the pure gate its own copy is')
            scope, activate, fields = _walk(fsm, variables, playerdata)
            if scope != 'destroy' and not actions <= GATE_VOCABULARY:
                raise Refused('the FSM does more than gate activation: '
                              + ', '.join(sorted(actions - GATE_VOCABULARY)))
        except Inert as reason:
            self.inert.append({'source': sid, 'object': self._name(gid), 'gate': gate, 'reason': str(reason)})
            return
        except Refused as refusal:
            return self._refuse(sid, gid, gate, str(refusal))
        except Exception as ex:
            return self._refuse(sid, gid, gate, f'unreadable: {ex}')
        if scope == 'named':
            # One building swap: the fresh-save branch turns each found child on
            # or off. Only the removal applies, as everywhere here.
            kids = {self.scene.gos[k]['m_Name']: k for k in children.get(self.scene.go_transform[gid], [])
                    if k in self.scene.gos}
            field = ', '.join(fields)
            for child, on in sorted(activate.items()):
                if child not in kids:
                    return self._refuse(sid, gid, gate, f'the found child {child!r} is not a direct child')
            for child, on in sorted(activate.items()):
                if on:
                    self.activates.append({'source': sid, 'object': f'{self._name(gid)}/{child}', 'gate': gate,
                                           'field': field, 'authored_inactive': int(not self.scene.gos[kids[child]]['m_IsActive'])})
                else:
                    self._remove(sid, kids[child], gate, field, self._name(gid))
            return
        # A destroyed owner takes its children with it through `Scene.active`.
        targets = [gid] if scope in ('self', 'destroy') else sorted(children.get(self.scene.go_transform[gid], []))
        targets = [t for t in targets if t in self.scene.gos]
        field = ', '.join(fields)
        if activate:
            # The other branch of the same gate. Showing an object the port does
            # not draw today is a separate change, so this only gets counted.
            self.activates.append({'source': sid, 'object': self._name(gid), 'gate': gate, 'field': field,
                                   'authored_inactive': sum(not self.scene.gos[t]['m_IsActive'] for t in targets)})
            return
        for target in targets:
            self._remove(sid, target, gate, field, self._name(gid) if scope == 'children' else None)

    def report(self):
        """The blast radius. `hidden` is what actually leaves the cooked world:
        an object the authored hierarchy already had inactive was never there."""
        hidden = [row for row in self.removed if self._authored_active(row['object_id'])]
        return {'removed': self.removed, 'activates': self.activates, 'refused': self.refused,
                'inert': self.inert, 'removed_count': len(self.off), 'other_fsms': self.other_fsms,
                'hidden_count': len({row['object_id'] for row in hidden}), 'hidden': hidden}


if __name__ == '__main__':
    import sys
    from source import ROOT, Source, dump
    from scene import Scene
    from quality import SCENE_TABLE

    source = Source()
    scenes = {}
    for entry in SCENE_TABLE:
        report = Scene(source, entry['file']).gates.report()
        if report['removed'] or report['activates'] or report['refused']:
            scenes[entry['scene_name']] = report
    total = sum(r['removed_count'] for r in scenes.values())
    hidden = sum(r['hidden_count'] for r in scenes.values())
    dump(ROOT / '.hkpsx/activation-gates.json',
         {'scene_count': len(SCENE_TABLE), 'removed_total': total, 'hidden_total': hidden, 'scenes': scenes})
    print(f'{total} objects gated off across {len(SCENE_TABLE)} admitted scenes, '
          f'{hidden} of them authored active and so newly hidden')
    for name, report in scenes.items():
        if not report['hidden']:
            continue
        print(f"  {name}: {report['hidden_count']} of {report['removed_count']}")
        for row in report['hidden']:
            via = f" (child of {row['child_of']})" if 'child_of' in row else ''
            print(f"      {row['object']}{via} [{row['gate']} {row['field']}]")
    if '--refused' in sys.argv:
        for name, report in scenes.items():
            for row in report['refused']:
                print(f"  refused {name} {row['object']} [{row['gate']}]: {row['reason']}")
        for name, report in scenes.items():
            for row in report['activates']:
                print(f"  activates {name} {row['object']} [{row['gate']} {row['field']}]:"
                      f" {row['authored_inactive']} authored inactive children stay hidden")
