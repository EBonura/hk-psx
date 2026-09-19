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

from script_ir import (CMP_GT, CMP_LT, HERO_TRIGGER, INT_COMPARE, REPEAT, SET_BOOL,
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
