"""The PlayMaker to script-IR compiler.

The subtle part is the read-only variable fold: it substitutes a variable's
serialized initial value into an action, which is only sound when nothing writes
that variable. A wrong answer there is silent rather than a refusal, which is
why it is pinned here. The opcode numbers are checked against the Rust enum on
import, so a drift fails this suite too.

The Trigger2dEvent tests pin the other kind of silent wrong answer: a trigger
that compiles but watches for the wrong body, or one that compiles and can never
fire. Both look like an implemented trigger from outside, so each refusal case
has a test naming it.
"""
import sys, unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'host'))
sys.path.insert(0, str(Path(__file__).resolve().parent))

from script_ir import (CMP_GT, CMP_LT, HERO_TRIGGER, INT_COMPARE, REPEAT, SET_BOOL, SET_INT,
                       TRIGGER_ENTER, TRIGGER_EXIT, TRIGGER_STAY, WAIT, Constants, Events,
                       Unsupported, compile_action, compile_fsm, decoded_action, source_id)
from test_activation import action_data, state


def fsm(states, variables=()):
    """An FSM whose declared variables carry serialized initial values."""
    return {'name': 'T', 'startState': states[0]['name'], 'globalTransitions': [],
            'variables': {'boolVariables': [{'name': n, 'value': v} for n, v in variables]},
            'states': states}


class CompilerTests(unittest.TestCase):
    def compile_one(self, action, fields, variables=None, folded=None):
        data = action_data([(action, fields)])
        return compile_action(action, decoded_action(data, 0), Events(),
                              Constants(), variables if variables is not None else {},
                              folded=folded)

    def test_a_wait_becomes_ticks_and_its_finish_event(self):
        ops = self.compile_one('Wait', [('time', 17, True), ('finishEvent', 23, 'DONE')])
        # The compact bool encoder gives 1.0, which is 60 ticks.
        self.assertEqual(len(ops), 1)
        self.assertEqual(ops[0][0], WAIT)
        self.assertEqual(ops[0][2], 60)

    def test_an_unsupported_action_names_itself(self):
        with self.assertRaises(Unsupported) as caught:
            self.compile_one('FlingObjectsFromGlobalPool', [])
        self.assertEqual(caught.exception.action, 'FlingObjectsFromGlobalPool')

    def test_a_set_bool_needs_a_variable_slot_not_a_literal(self):
        # Writing through a literal would silently write nowhere.
        with self.assertRaises(Unsupported):
            self.compile_one('SetBoolValue', [('boolValue', 17, True)])

    def test_an_int_compare_emits_one_op_per_branch_it_actually_has(self):
        # The source action carries three branches; only the two with an event
        # become ops, each with its own comparison selector.
        ops = self.compile_one('IntCompare',
                               [('integer1', 16, 'soul'), ('integer2', 16, 33),
                                ('lessThan', 23, 'LT'), ('greaterThan', 23, 'GT')],
                               variables={'soul': 0})
        self.assertEqual([op[0] for op in ops], [INT_COMPARE, INT_COMPARE])
        self.assertEqual([op[1] for op in ops], [CMP_LT, CMP_GT])

    def test_an_int_compare_against_a_variable_refuses(self):
        # A moving right-hand side is not a constant-pool index.
        with self.assertRaises(Unsupported):
            self.compile_one('IntCompare',
                             [('integer1', 16, 'soul'), ('integer2', 16, 'cost'),
                              ('lessThan', 23, 'LT')],
                             variables={'soul': 0, 'cost': 1})


class Tree:
    """A scene resolver over one parent and its named children."""

    def __init__(self, children):
        self.children = children

    def child(self, gid, name):
        return self.children.get((gid, name))

    def parent(self, gid):
        return None

    def named(self, name):
        return None


def bind_owner(data, action_index, variable):
    """Point one action's FsmOwnerDefault at a variable instead of the owner."""
    owner = data['fsmOwnerDefaultParams'][action_index]
    owner['gameObject'] = {'useVariable': 1, 'name': variable, 'value': None}
    return data


class ResolverTests(unittest.TestCase):
    """A reference stored in one state and read in another.

    PlayMaker abandons a state's remaining actions the moment one transitions,
    so a write is only readable elsewhere when every path to the read has
    already taken it. The narrow rule for that was the start state's opening
    run; this is the general one, and the failure it has to keep refusing is a
    write on one branch read on another.
    """

    def program(self, states, start=None, children=None):
        definition = fsm(states, variables=[('Self', None), ('Found', None), ('Flag', False)])
        definition['startState'] = start or states[0]['name']
        return compile_fsm(definition, owner_id=7,
                           objects=Tree(children if children is not None else {(7, 'Door'): 42}))

    def ops_of(self, program, state):
        first, count, _t, _tc = program['states'][program['state_names'].index(state)]
        return program['ops'][first:first + count]

    def test_a_dominating_store_is_readable_in_a_later_state(self):
        # A does GetOwner into Self and can only go to B, so B reads Self.
        a = state('A', [('GetOwner', [('storeGameObject', 16, 'Self')])], [('GO', 'B')])
        b = state('B', [('FindChild', [('gameObject', 20, 1), ('childName', 18, 'Door'),
                                       ('storeResult', 16, 'Found')])])
        bind_owner(b['actionData'], 0, 'Self')
        program = self.program([a, b])
        # FindChild resolves to a constant store of the child's source id.
        self.assertEqual([op[0] for op in self.ops_of(program, 'B')], [SET_INT])
        self.assertIn(42, program['constants'])

    def test_a_store_on_one_branch_is_not_readable_on_another(self):
        # START can reach C without passing through A, so Self may be unset.
        start = state('START', [], [('LEFT', 'A'), ('RIGHT', 'C')])
        a = state('A', [('GetOwner', [('storeGameObject', 16, 'Self')])], [('GO', 'C')])
        c = state('C', [('FindChild', [('gameObject', 20, 1), ('childName', 18, 'Door'),
                                       ('storeResult', 16, 'Found')])])
        bind_owner(c['actionData'], 0, 'Self')
        with self.assertRaises(Unsupported) as caught:
            self.program([start, a, c])
        self.assertIn('cannot fix', caught.exception.detail)

    def test_a_state_does_not_read_its_own_store_before_it_happens(self):
        # The FindChild sits ahead of the GetOwner in the same state, so on the
        # first entry the variable holds nothing.
        first = state('A', [('FindChild', [('gameObject', 20, 1), ('childName', 18, 'Door'),
                                           ('storeResult', 16, 'Found')]),
                            ('GetOwner', [('storeGameObject', 16, 'Self')])])
        bind_owner(first['actionData'], 0, 'Self')
        with self.assertRaises(Unsupported):
            self.program([first])

    def test_a_store_behind_an_action_that_could_transition_is_not_trusted(self):
        # BoolTest can leave the state, so nothing after it is guaranteed to
        # have run by the time another state reads the variable.
        a = state('A', [('BoolTest', [('boolVariable', 17, True), ('isTrue', 23, 'GO')]),
                        ('GetOwner', [('storeGameObject', 16, 'Self')])], [('GO', 'B')])
        b = state('B', [('FindChild', [('gameObject', 20, 1), ('childName', 18, 'Door'),
                                       ('storeResult', 16, 'Found')])])
        bind_owner(b['actionData'], 0, 'Self')
        with self.assertRaises(Unsupported):
            self.program([a, b])

    def test_the_start_state_is_the_authored_one_not_the_first(self):
        # The old rule read the prologue out of states[0]. An FSM whose start
        # state is elsewhere got the wrong state's writes, or none.
        dead = state('DEAD', [('GetOwner', [('storeGameObject', 16, 'Self')])])
        real = state('REAL', [('GetOwner', [('storeGameObject', 16, 'Self')])], [('GO', 'USE')])
        use = state('USE', [('FindChild', [('gameObject', 20, 1), ('childName', 18, 'Door'),
                                           ('storeResult', 16, 'Found')])])
        bind_owner(use['actionData'], 0, 'Self')
        program = self.program([dead, real, use], start='REAL')
        self.assertEqual([op[0] for op in self.ops_of(program, 'USE')], [SET_INT])

    def test_a_child_the_parent_does_not_have_stores_the_null_the_source_stores(self):
        a = state('A', [('GetOwner', [('storeGameObject', 16, 'Self')])], [('GO', 'B')])
        b = state('B', [('FindChild', [('gameObject', 20, 1), ('childName', 18, 'Door'),
                                       ('storeResult', 16, 'Found')])])
        bind_owner(b['actionData'], 0, 'Self')
        program = self.program([a, b], children={(7, 'Door'): 0})
        self.assertIn(0, program['constants'])


class GlobalTransitionTests(unittest.TestCase):
    def test_a_definition_with_a_global_transition_refuses(self):
        # A global transition fires from any state. Compiling the FSM without it
        # would turn an event that leaves into an event that goes nowhere, which
        # nothing downstream could tell from a state with nothing to do.
        states = [state('Idle', [])]
        definition = fsm(states)
        definition['globalTransitions'] = [{'fsmEvent': {'name': 'HERO LEAVE'},
                                            'toState': 'Idle'}]
        with self.assertRaises(Unsupported) as caught:
            compile_fsm(definition)
        self.assertIn('global transitions', caught.exception.detail)


class ReadOnlyFoldTests(unittest.TestCase):
    """A variable mentioned by exactly one action is only read, so its initial
    value is constant for the run. A second mention might be the write."""

    def flag_test(self, name):
        return ('PlayerDataBoolTest', [('boolName', 18, name), ('isTrue', 23, ''),
                                       ('isFalse', 23, 'OFF')])

    def test_a_variable_only_read_is_folded_into_the_field(self):
        # One action mentions "PD Bool Name", whose initial value is the real
        # PlayerData field. This is Crossroads_47's Grate Control shape.
        read = ('PlayerDataBoolTest', [('boolName', 18, None), ('isTrue', 23, ''),
                                       ('isFalse', 23, 'OFF')])
        data = action_data([read])
        data['fsmStringParams'][0] = {'useVariable': 1, 'name': 'PD Bool Name', 'value': ''}
        program = compile_fsm(fsm([{'name': 'Check', 'actionData': data, 'transitions': []}],
                                  variables=[('PD Bool Name', 'openedCrossroads')]))
        self.assertIn('openedCrossroads', program['player_data_fields'])

    def test_a_variable_mentioned_twice_is_not_folded(self):
        # The second mention might be the write, so the value is not constant
        # and the action must refuse rather than use a stale initial value.
        read = ('PlayerDataBoolTest', [('boolName', 18, None), ('isTrue', 23, ''),
                                       ('isFalse', 23, 'OFF')])
        first, second = action_data([read]), action_data([read])
        for data in (first, second):
            data['fsmStringParams'][0] = {'useVariable': 1, 'name': 'Field', 'value': 'someFlag'}
        states = [{'name': 'A', 'actionData': first, 'transitions': []},
                  {'name': 'B', 'actionData': second, 'transitions': []}]
        with self.assertRaises(Unsupported):
            compile_fsm(fsm(states, variables=[('Field', 'someFlag')]))


OWNER = 0x1234


def trigger_data(phase, tag='Player', layer='', event='HIT', store=''):
    """The exact `Trigger2dEvent` field set every admitted instance carries.

    `action_data` has no encoding for the two shapes this action needs, so they
    are appended here: the Trigger2DType is a raw four-byte enum parameter
    rather than one of the compact scalars, and `storeCollider` is an
    FsmGameObject. `tag=None` is PlayMaker's None, an unbound field that reads
    as the empty string, which is how 816 of the 1,026 instances leave it.
    """
    data = action_data([('Trigger2dEvent', [('collideTag', 18, tag or ''),
                                            ('collideLayer', 18, layer),
                                            ('sendEvent', 23, event)])])
    if tag is None:
        data['fsmStringParams'][0] = {'useVariable': 1, 'name': '', 'value': ''}
    for name, kind, pos, size in [('storeCollider', 19, len(data['fsmGameObjectParams']), 0),
                                  ('trigger', 7, len(data['byteData']), 4)]:
        data['paramName'].append(name)
        data['paramDataType'].append(kind)
        data['paramDataPos'].append(pos)
        data['paramByteDataSize'].append(size)
    data['fsmGameObjectParams'].append({'useVariable': 1, 'name': store,
                                        'value': {'m_FileID': 0, 'm_PathID': 0}})
    data['byteData'].extend(phase.to_bytes(4, 'little', signed=True))
    return data


class Volume:
    """A stand-in for the scene resolver, answering only about one trigger owner."""

    def __init__(self, reachable=('Player',), has_trigger=True):
        self.reachable, self.has = set(reachable), has_trigger

    def reachable_layers(self, gid):
        return set(self.reachable)

    def has_trigger(self, gid):
        return self.has


class TriggerTests(unittest.TestCase):
    """Trigger2dEvent is the first refusal for 274 FSM instances, and the port
    can answer exactly one of the questions it asks: is the hero body inside
    this volume. Every other collider it can name is refused, because a trigger
    that fires for the wrong body is worse than one that does not compile."""

    def compile_trigger(self, phase=TRIGGER_STAY, objects=None, **kwargs):
        data = trigger_data(phase, **kwargs)
        return compile_action('Trigger2dEvent', decoded_action(data, 0), Events(), Constants(),
                              {}, owner_id=OWNER, objects=objects or Volume())

    def test_a_player_tagged_trigger_watches_the_owners_volume_every_tick(self):
        ops = self.compile_trigger()
        self.assertEqual(ops, [(HERO_TRIGGER, TRIGGER_STAY | REPEAT, 0, 0, 1)])

    def test_every_authored_phase_reaches_the_op(self):
        for phase in (TRIGGER_ENTER, TRIGGER_STAY, TRIGGER_EXIT):
            self.assertEqual(self.compile_trigger(phase)[0][1], phase | REPEAT)

    def test_the_watched_object_is_the_owners_cooked_id(self):
        # The action names no object: PlayMaker delivers the callback to the
        # FSM's own GameObject, so that is the volume and the id the op carries.
        events, constants = Events(), Constants()
        compile_action('Trigger2dEvent', decoded_action(trigger_data(TRIGGER_ENTER), 0),
                       events, constants, {}, owner_id=OWNER, objects=Volume())
        self.assertEqual(constants.values, [source_id(OWNER)])

    def test_an_untagged_trigger_compiles_where_physics_admits_the_hero_alone(self):
        # Hero Detector is the layer the level authors used for this, and its
        # matrix row admits Player and nothing else named.
        ops = self.compile_trigger(tag=None)
        self.assertEqual(ops[0][0], HERO_TRIGGER)

    def test_an_untagged_trigger_something_else_can_enter_refuses(self):
        # An Enemy Detector volume watches for enemies, which the guest has no
        # collider for. Answering it with the hero body would fire for the
        # wrong thing rather than not at all.
        with self.assertRaises(Unsupported) as caught:
            self.compile_trigger(tag=None, objects=Volume(reachable=('Player', 'Enemies')))
        self.assertIn('Enemies', caught.exception.detail)

    def test_a_trigger_watching_a_collider_of_the_knight_refuses(self):
        # HeroBox and the attack tags are child colliders of the Knight, each a
        # different shape from the body the port can test.
        for tag in ('HeroBox', 'Nail Attack', 'Dream Attack', 'Hero Spell'):
            with self.assertRaises(Unsupported) as caught:
                self.compile_trigger(tag=tag)
            self.assertIn(tag, caught.exception.detail)

    def test_a_layer_filter_refuses(self):
        with self.assertRaises(Unsupported):
            self.compile_trigger(layer='Enemies')

    def test_a_trigger_on_an_object_with_no_collider_refuses(self):
        # Six Area Title Controllers are authored this way. Unity never raises
        # the callback for them, so the op could only ever be a trigger that
        # silently never fires.
        with self.assertRaises(Unsupported):
            self.compile_trigger(objects=Volume(has_trigger=False))

    def test_a_trigger_that_stores_the_colliding_object_refuses(self):
        # The store happens before the send and only when the filter matched, so
        # a separate op beside this one would hold the Knight while the volume
        # is empty.
        with self.assertRaises(Unsupported):
            self.compile_trigger(store='Hero')

    def test_a_trigger_with_no_event_refuses(self):
        with self.assertRaises(Unsupported):
            self.compile_trigger(event='')

    def test_a_trigger_without_a_scene_resolver_refuses(self):
        with self.assertRaises(Unsupported):
            compile_action('Trigger2dEvent', decoded_action(trigger_data(TRIGGER_STAY), 0),
                           Events(), Constants(), {}, owner_id=OWNER)


if __name__ == '__main__':
    unittest.main()
