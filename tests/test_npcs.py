"""The conversation chain an NPC speaks, and what stops it being cooked.

`Conversation Control` answers from PlayerData and then writes the flag that
changes its own next answer, so what a player hears is a chain. These build the
serialized shape PlayMaker actually ships, rather than the decoded dicts, so the
walker's own readers stay in the test.
"""
import struct
import sys
import unittest
from pathlib import Path
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'host'))
import npcs
from npcs import conversation_chain, MAX_CONVERSATIONS
from npc_dialogue import conversations

FSM_STRING, FSM_VAR, COMPACT_INT, COMPACT_BOOL = 18, 39, 16, 17


def actions(*specs):
    """Serialized actionData for `(kind, [(param, type, value), ...])` actions.

    A compact scalar's value is either a literal or `(literal, variable)`, which
    is how PlayMaker serializes a field that reads an FSM variable instead.
    """
    names, starts = [], []
    param_name, param_type, param_pos, param_size = [], [], [], []
    blob, strings, variables = bytearray(), [], []
    for kind, params in specs:
        starts.append(len(param_name))
        names.append(kind)
        for name, datatype, value in params:
            param_name.append(name)
            param_type.append(datatype)
            if datatype in (FSM_STRING, FSM_VAR):
                pool = strings if datatype == FSM_STRING else variables
                param_pos.append(len(pool))
                param_size.append(0)
                pool.append(value)
                continue
            literal, reads = value if isinstance(value, tuple) else (value, '')
            raw = (struct.pack('<i' if datatype == COMPACT_INT else '<?', literal)
                   + bytes([1 if reads else 0]) + reads.encode('utf8'))
            param_pos.append(len(blob))
            param_size.append(len(raw))
            blob += raw
    return {'actionNames': names, 'actionEnabled': [1] * len(names), 'actionStartIndex': starts,
            'paramName': param_name, 'paramDataType': param_type, 'paramDataPos': param_pos,
            'paramByteDataSize': param_size, 'byteData': list(blob),
            'fsmStringParams': strings, 'fsmVarParams': variables,
            'fsmOwnerDefaultParams': [], 'functionCallParams': [], 'fsmEventTargetParams': []}


def speaks(key, sheet='Minor NPC'):
    return ('CallMethodProper', [('behaviour', FSM_STRING, 'DialogueBox'),
                                 ('methodName', FSM_STRING, 'StartConversation'),
                                 ('', FSM_VAR, {'stringValue': key, 'useVariable': False}),
                                 ('', FSM_VAR, {'stringValue': sheet, 'useVariable': False})])


def writes(field, value=True):
    return ('SetPlayerDataBool', [('boolName', FSM_STRING, field),
                                  ('value', COMPACT_BOOL, value)])


def tests(field, is_true='', is_false=''):
    return ('PlayerDataBoolTest', [('boolName', FSM_STRING, field),
                                   ('isTrue', FSM_STRING, is_true),
                                   ('isFalse', FSM_STRING, is_false)])


def state(name, transitions=(), *specs):
    return {'name': name, 'actionData': actions(*specs),
            'transitions': [{'fsmEvent': {'name': event}, 'toState': target}
                            for event, target in transitions]}


def fsm(*states):
    return {'name': 'Conversation Control', 'startState': 'Idle', 'states': list(states),
            'globalTransitions': [], 'variables': {'boolVariables': []}}


def scene(convo, name='Fixture', gate=None):
    """The lookups the chain walk and the page cook make on their scene."""
    objects = {1: ('PlayMakerFSM', {'m_GameObject': {'m_PathID': 7}, 'fsm': convo})}
    if gate:
        objects[2] = ('DeactivateIfPlayerdataTrue', {'m_GameObject': {'m_PathID': 8},
                                                     'boolName': gate})
    return SimpleNamespace(
        gos={7: {'m_Name': name, 'm_Component': [{'component': {'m_PathID': 1}}]}},
        objects=objects)


def chain(convo, defaults, **kwargs):
    return conversation_chain(None, scene(convo), 7, defaults, **kwargs)


# Elderbug's shape: a choice on the met bool, an intro whose second state writes
# it, a follow-up that writes its own once-only flag, and a generic line that
# writes nothing and so repeats.
ELDERBUG = fsm(
    state('Idle', [('CONVO START', 'Convo Choice')]),
    state('Convo Choice', [('MEET', 'Intro'), ('HIST', 'History'), ('FINISHED', 'Generic')],
          tests('metFixture', is_false='MEET'), tests('historyFixture', is_false='HIST')),
    state('Intro', [('FINISHED', 'Intro Main')], speaks('INTRO')),
    state('Intro Main', [('FINISHED', 'Idle')], speaks('INTRO_MAIN'), writes('metFixture')),
    state('History', [('FINISHED', 'Idle')], speaks('HISTORY'), writes('historyFixture')),
    state('Generic', [('FINISHED', 'Idle')], speaks('GENERIC')))
DEFAULTS = {'metFixture': False, 'historyFixture': False}


class ConversationChainTests(unittest.TestCase):
    def keys(self, chained):
        return [[entry['key'] for entry in conversation['entries']] for conversation in chained]

    def test_each_conversation_is_the_one_its_own_writes_lead_to(self):
        chained = chain(ELDERBUG, DEFAULTS)
        self.assertEqual(self.keys(chained), [['INTRO', 'INTRO_MAIN'], ['HISTORY'], ['GENERIC']])
        self.assertEqual([c['terminal'] for c in chained], [False, False, True])
        # The write rides with the entry whose own state performs it, because
        # PlayMaker runs it on enter, before the line beside it has spoken.
        intro = chained[0]['entries']
        self.assertEqual([w['field'] for w in intro[0]['writes']], [])
        self.assertEqual([w['field'] for w in intro[1]['writes']], ['metFixture'])

    def test_a_conversation_that_changes_nothing_ends_the_chain(self):
        # Myla's repeat line rewrites metMiner, which is already set. The source
        # cannot move on from there, so neither does the cursor.
        repeat = fsm(
            state('Idle', [('CONVO START', 'Convo Choice')]),
            state('Convo Choice', [('MEET', 'Meet'), ('FINISHED', 'Repeat')],
                  tests('metFixture', is_false='MEET')),
            state('Meet', [('FINISHED', 'Idle')], speaks('MEET'), writes('metFixture')),
            state('Repeat', [('FINISHED', 'Idle')], speaks('REPEAT'), writes('metFixture')))
        chained = chain(repeat, {'metFixture': False})
        self.assertEqual(self.keys(chained), [['MEET'], ['REPEAT']])
        self.assertEqual([c['terminal'] for c in chained], [False, True])

    def test_a_chain_longer_than_the_saved_cursor_is_refused(self):
        with self.assertRaisesRegex(LookupError, 'more than 1 conversation'):
            chain(ELDERBUG, DEFAULTS, limit=1)
        self.assertEqual(len(chain(ELDERBUG, DEFAULTS, limit=MAX_CONVERSATIONS)), 3)

    def test_a_conversation_that_speaks_nothing_is_refused(self):
        # The Stag settles in its own choice state on a fresh save: every branch
        # needs a station the port cannot have opened yet.
        silent = fsm(
            state('Idle', [('CONVO START', 'Convo Choice')]),
            state('Convo Choice', [('OPEN', 'Line')], tests('openedStation', is_true='OPEN')),
            state('Line', [('FINISHED', 'Idle')], speaks('LINE')))
        with self.assertRaisesRegex(LookupError, 'speaks nothing on conversation 0'):
            chain(silent, {'openedStation': False})

    def test_a_cycle_is_refused_rather_than_cooked_as_a_chain(self):
        # Two conversations that hand the choice back and forth: a cursor that
        # only ever saturates cannot express one.
        flip = fsm(
            state('Idle', [('CONVO START', 'Convo Choice')]),
            state('Convo Choice', [('A', 'A'), ('FINISHED', 'B')],
                  tests('flag', is_false='A')),
            state('A', [('FINISHED', 'Idle')], speaks('A'), writes('flag', True)),
            state('B', [('FINISHED', 'Idle')], speaks('B'), writes('flag', False)))
        with self.assertRaisesRegex(LookupError, 'returns to conversation 0'):
            chain(flip, {'flag': False})

    def test_int_compare_to_bool_feeds_the_test_that_reads_it(self):
        # Elderbug's Convo Choice derives `Is Steel Soul Mode` from permadeathMode
        # this way and a later BoolTest reads it; leaving it undecoded stopped
        # every walk that reached him after metElderbug was set.
        mode, steel = ('Permadeath Mode', 'Is Steel Soul Mode')
        derived = fsm(
            state('Idle', [('CONVO START', 'Convo Choice')]),
            state('Convo Choice', [('STEEL', 'Steel'), ('FINISHED', 'Normal')],
                  ('GetPlayerDataInt', [('intName', FSM_STRING, 'permadeathMode'),
                                        ('storeValue', COMPACT_INT, (0, mode))]),
                  ('IntCompareToBool', [('integer1', COMPACT_INT, (0, mode)),
                                        ('integer2', COMPACT_INT, 0),
                                        ('greaterThanBool', COMPACT_BOOL, (False, steel))]),
                  ('BoolTest', [('boolVariable', COMPACT_BOOL, (False, steel)),
                                ('isTrue', FSM_STRING, 'STEEL'),
                                ('isFalse', FSM_STRING, '')])),
            state('Steel', [('FINISHED', 'Idle')], speaks('STEEL')),
            state('Normal', [('FINISHED', 'Idle')], speaks('NORMAL')))
        self.assertEqual(self.keys(chain(derived, {'permadeathMode': 0})), [['NORMAL']])
        self.assertEqual(self.keys(chain(derived, {'permadeathMode': 1})), [['STEEL']])
        self.assertIn('IntCompareToBool', npcs.INERT)


class CookedPageTests(unittest.TestCase):
    """The panel pages, and the page each conversation's own write lands on."""
    SHEET = {'INTRO': 'one', 'INTRO_MAIN': 'two<page>three',
             'HISTORY': 'four', 'GENERIC': 'five'}

    def cook(self, gate=None):
        # One unit per glyph: every fixture line is far inside the panel width,
        # so the page count is the authored `<page>` count and nothing else.
        return conversations(None, scene(ELDERBUG, gate=gate),
                             {'game_object': 7, 'name': 'Fixture'},
                             [1] * 95, {'Minor NPC': dict(self.SHEET)}, DEFAULTS)

    def test_the_write_lands_on_the_page_its_own_entry_starts(self):
        cooked = self.cook()
        self.assertEqual([len(c['pages']) for c in cooked], [3, 1, 1])
        # Intro 1 speaks one page and writes nothing; Intro Main opens on page 1.
        self.assertEqual([c['advance_page'] for c in cooked], [1, 0, 1])
        # The terminal conversation's advance page is one the panel cannot
        # reach, which is how "this one repeats" reaches the guest.
        self.assertEqual(cooked[2]['advance_page'], len(cooked[2]['pages']))

    def test_a_write_the_same_scene_gates_an_object_on_is_refused(self):
        # activation.py answered that gate once, at cook time, so a conversation
        # that moves it would contradict the world already packed.
        with self.assertRaisesRegex(ValueError, r"writes \['metFixture'\]"):
            self.cook(gate='metFixture')


if __name__ == '__main__':
    unittest.main()
