"""The activation gate walker, which decides what leaves the cooked world.

This module removes objects. A false positive deletes part of the world and is
very hard to notice afterwards, so the property these tests care about most is
that anything unfamiliar refuses and keeps the object.
"""
import sys, unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'host'))

from activation import Inert, Refused, _walk


def action_data(actions):
    """Build the compact PlayMaker action encoding `action_fields` reads.

    `actions` is a list of `(name, [(field, kind, value), ...])`. Only the kinds
    the gate vocabulary uses are supported: 23 is a plain string in `byteData`,
    18 indexes `fsmStringParams` (whose entries are FsmString records, not bare
    strings), 20 indexes `fsmOwnerDefaultParams`, and 17 is a compact bool.
    """
    names, enabled, starts = [], [], []
    param_name, param_kind, param_pos, param_size = [], [], [], []
    byte_data, strings, owners = bytearray(), [], []
    for name, fields in actions:
        starts.append(len(param_name))
        names.append('HutongGames.PlayMaker.Actions.' + name)
        enabled.append(1)
        for field, kind, value in fields:
            param_name.append(field)
            param_kind.append(kind)
            if kind == 23:
                param_pos.append(len(byte_data))
                encoded = value.encode('utf8')
                param_size.append(len(encoded))
                byte_data.extend(encoded)
            elif kind == 18:
                param_pos.append(len(strings))
                param_size.append(0)
                strings.append({'useVariable': 0, 'name': '', 'value': value})
            elif kind == 20:
                param_pos.append(len(owners))
                param_size.append(0)
                owners.append({'ownerOption': value, 'gameObject': {}})
            elif kind == 17:
                param_pos.append(len(byte_data))
                # One byte of payload, then useVariable, then the name.
                encoded = bytes([1 if value else 0, 0])
                param_size.append(len(encoded))
                byte_data.extend(encoded)
            else:
                raise AssertionError(f'unsupported test field kind {kind}')
    return {'actionNames': names, 'actionEnabled': enabled, 'actionStartIndex': starts,
            'paramName': param_name, 'paramDataType': param_kind, 'paramDataPos': param_pos,
            'paramByteDataSize': param_size, 'byteData': list(byte_data),
            'fsmStringParams': strings, 'fsmGameObjectParams': [], 'fsmOwnerDefaultParams': owners,
            'functionCallParams': [], 'fsmEventTargetParams': [], 'fsmVarParams': []}


def state(name, actions, transitions=()):
    return {'name': name, 'actionData': action_data(actions),
            'transitions': [{'fsmEvent': {'name': e}, 'toState': t} for e, t in transitions]}


def gate_fsm(check_actions, deactivate=True):
    """The `deactivate_ifnot_playerdatabool` shape: test a flag, then act."""
    activate = [('ActivateGameObject', [
        ('gameObject', 20, 0), ('activate', 17, not deactivate)])]
    return {'name': 'FSM', 'startState': 'Check', 'globalTransitions': [], 'variables': {},
            'states': [state('Check', check_actions, [('OFF', 'Act')]),
                       state('Act', activate)]}


TEST_FLAG = [('PlayerDataBoolTest', [
    ('boolName', 18, 'someFlag'), ('isTrue', 23, ''), ('isFalse', 23, 'OFF')])]


class ActivationWalkerTests(unittest.TestCase):
    def test_a_false_flag_reaches_the_deactivation(self):
        scope, activate, fields = _walk(gate_fsm(TEST_FLAG), {}, {'someFlag': 0})
        self.assertEqual((scope, activate), ('self', False))
        self.assertEqual(fields, ['someFlag'])

    def test_a_true_flag_sends_no_event_and_leaves_the_object_alone(self):
        # isTrue is empty, which is the shape the source uses for "do nothing".
        with self.assertRaises(Inert):
            _walk(gate_fsm(TEST_FLAG), {}, {'someFlag': 1})

    def test_an_unfamiliar_action_refuses_rather_than_removing(self):
        # The property that matters: a gate whose state does anything this
        # vocabulary does not fully decode must keep the object.
        actions = [('SetFsmBool', [('setValue', 17, True)])] + TEST_FLAG
        with self.assertRaises(Refused) as caught:
            _walk(gate_fsm(actions), {}, {'someFlag': 0})
        self.assertIn('SetFsmBool', str(caught.exception))

    def test_a_flag_absent_from_the_fresh_save_refuses(self):
        # An unknown field is not a false field. Defaulting it would remove an
        # object on a guess.
        with self.assertRaises(Refused):
            _walk(gate_fsm(TEST_FLAG), {}, {})

    def test_a_gate_that_loops_refuses_instead_of_spinning(self):
        fsm = gate_fsm(TEST_FLAG)
        fsm['states'][0]['transitions'] = [{'fsmEvent': {'name': 'OFF'}, 'toState': 'Check'}]
        with self.assertRaises(Refused) as caught:
            _walk(fsm, {}, {'someFlag': 0})
        self.assertIn('loops', str(caught.exception))

    def test_a_global_transition_refuses_because_it_can_leave_the_result(self):
        fsm = gate_fsm(TEST_FLAG)
        fsm['globalTransitions'] = [{'fsmEvent': {'name': 'RESET'}, 'toState': 'Check'}]
        with self.assertRaises(Refused):
            _walk(fsm, {}, {'someFlag': 0})

    def test_activation_must_be_the_last_action_of_its_state(self):
        # A later action could undo or redirect it, so the walk cannot claim the
        # activation is the outcome.
        fsm = gate_fsm(TEST_FLAG)
        fsm['states'][1] = state('Act', [
            ('ActivateGameObject', [('gameObject', 20, 0), ('activate', 17, False)]),
            ('PlayerDataBoolTest', [('boolName', 18, 'other'), ('isTrue', 23, ''), ('isFalse', 23, 'X')])])
        with self.assertRaises(Refused):
            _walk(fsm, {}, {'someFlag': 0})


if __name__ == '__main__':
    unittest.main()
