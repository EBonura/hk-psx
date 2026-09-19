"""The PlayMaker to script-IR compiler.

The subtle part is the read-only variable fold: it substitutes a variable's
serialized initial value into an action, which is only sound when nothing writes
that variable. A wrong answer there is silent rather than a refusal, which is
why it is pinned here. The opcode numbers are checked against the Rust enum on
import, so a drift fails this suite too.
"""
import sys, unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'host'))
sys.path.insert(0, str(Path(__file__).resolve().parent))

from script_ir import (CMP_GT, CMP_LT, INT_COMPARE, SET_BOOL, WAIT, Constants, Events,
                       Unsupported, compile_action, compile_fsm)
from test_activation import action_data, state


def fsm(states, variables=()):
    """An FSM whose declared variables carry serialized initial values."""
    return {'name': 'T', 'startState': states[0]['name'], 'globalTransitions': [],
            'variables': {'boolVariables': [{'name': n, 'value': v} for n, v in variables]},
            'states': states}


class CompilerTests(unittest.TestCase):
    def compile_one(self, action, fields, variables=None, folded=None):
        data = action_data([(action, fields)])
        from focus import action_fields
        return compile_action(action, action_fields(data, 0, objects=True), Events(),
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


if __name__ == '__main__':
    unittest.main()
