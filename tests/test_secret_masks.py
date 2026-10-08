"""Source-free tests for the one-way secret-mask reveal subset.

The reversible masks are covered by test_reveal_masks.py. This file covers what
is different about the authored `unmasker` six-state shape: it uncovers on the
first hero entry and never covers again, so an instance that cannot take its
whole authored fade with it is refused instead of half-opening.
"""
import copy
import pathlib
import struct
import sys
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / 'host'))
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / 'tests'))
from region_delta import layout
from reveal_masks import (SECRET_ACTIVATED_SECONDS, bind_regions, refuse_partial_one_way,
                          verify_secret_states)
from test_constant_textures import room


def value(v=None, name=''):
    return {'value': v, 'name': name, 'useVariable': bool(name)}


def fade(alpha, time):
    return {'gameObject': {'ownerOption': 0, 'gameObject': value()},
            'alpha': value(alpha), 'time': value(time), 'delay': value(0),
            'includeChildren': value(True), 'namedValueColor': value('_Color'),
            'easeType': 21, 'loopType': 0, 'realTime': value(False),
            'stopOnExit': value(True), 'loopDontFinish': value(True),
            'startEvent': '', 'finishEvent': ''}


def states():
    """The enabled action sequence every catalogue instance serializes."""
    return {
        'Pause': [('WaitForHeroInPosition',
                   {'sendEvent': 'FINISHED', 'skipIfAlreadyPositioned': value(False)})],
        'Idle': [('BoolTest', {'boolVariable': value(None, 'Activated'), 'isTrue': 'ACTIVATE',
                               'isFalse': '', 'everyFrame': False})],
        'Idle Stay': [('Trigger2dEvent', {'trigger': 1, 'sendEvent': 'UNCOVER',
                                          'collideTag': value('Player'), 'collideLayer': value('')})],
        'Fade': [('iTweenFadeTo', fade(0, .5)),
                 ('SetBoolValue', {'boolVariable': value(None, 'Activated'),
                                   'boolValue': value(True), 'everyFrame': False}),
                 ('BoolTest', {'boolVariable': value(None, 'Play Sound'), 'isTrue': 'SOUND',
                               'isFalse': '', 'everyFrame': False})],
        'Activate': [('iTweenFadeTo', fade(0, SECRET_ACTIVATED_SECONDS))],
        'Sound': [('AudioPlayerOneShotSingle', {})],
    }


def variables(activated=0, sound=1):
    return {'Activated': activated, 'Play Sound': sound}


def controller(source, one_way=True, renderers=('f:1',)):
    return {'controller': 0, 'source': source, 'name': source, 'source_id': 1,
            'renderer_sources': list(renderers), 'one_way': one_way,
            'trigger': [[1, 1], [2, 1], [2, 2], [1, 2]], 'fade_ticks': 30, 'initial_opacity': 128}


def soft(raw):
    """The same room with one CLUT word the subtractive fade cannot use."""
    changed = bytearray(raw)
    _, prefix, _, _ = layout(raw)
    struct.pack_into('<H', changed, prefix + 2 * 2, 0x4210)
    return bytes(changed)


class SecretMaskRecognizer(unittest.TestCase):
    def test_authored_shape_yields_the_fade_clock_and_the_chime_flag(self):
        self.assertEqual(verify_secret_states(states(), variables()), (30, True))
        self.assertEqual(verify_secret_states(states(), variables(sound=0)), (30, False))

    def test_a_mask_shipped_already_revealed_is_refused(self):
        # Nothing in the port answers `Activated`, so the only honest reading of
        # a placement that ships revealed is a refusal, not a covered mask.
        with self.assertRaisesRegex(ValueError, 'already revealed'):
            verify_secret_states(states(), variables(activated=1))

    def test_every_authored_field_the_reveal_depends_on_is_pinned(self):
        cases = [
            ('Idle Stay', 0, 'trigger', 2, 'trigger semantics'),
            ('Idle Stay', 0, 'sendEvent', 'COVER', 'trigger semantics'),
            ('Idle', 0, 'isTrue', 'OTHER', 'activation test'),
            ('Fade', 1, 'boolVariable', value(None, 'Other'), 'persistent bookkeeping'),
            ('Fade', 2, 'isTrue', 'OTHER', 'sound test'),
        ]
        for state, index, key, replacement, message in cases:
            data = copy.deepcopy(states())
            data[state][index][1][key] = replacement
            with self.assertRaisesRegex(ValueError, message):
                verify_secret_states(data, variables())
        # The unreachable already-revealed path still has to agree about where
        # the mask ends up, or half the definition would be free to drift.
        data = copy.deepcopy(states())
        data['Activate'][0][1]['alpha'] = value(1)
        with self.assertRaisesRegex(ValueError, 'activate target'):
            verify_secret_states(data, variables())
        data = copy.deepcopy(states())
        data['Fade'][0][1]['alpha'] = value(1)
        with self.assertRaisesRegex(ValueError, 'fade target'):
            verify_secret_states(data, variables())
        data = copy.deepcopy(states())
        data['Fade'].append(('Unknown', {}))
        with self.assertRaisesRegex(ValueError, 'action sequence Fade'):
            verify_secret_states(data, variables())
        data = copy.deepcopy(states())
        del data['Sound']
        with self.assertRaisesRegex(ValueError, 'reveal states'):
            verify_secret_states(data, variables())


class OneWayFadeContract(unittest.TestCase):
    def scene(self, *records):
        return {0: {'controllers': list(records), 'unsupported': []}}

    def test_a_renderer_that_never_becomes_a_draw_refuses_the_controller(self):
        by_scene = self.scene(controller('a', renderers=('f:1', 'f:2')))
        moved = refuse_partial_one_way(by_scene, {0: [(0, 'f:1', None)]})
        self.assertEqual(by_scene[0]['controllers'], [])
        self.assertEqual(moved, {0: {}})
        self.assertIn('1 of 2 renderers are not cooked', by_scene[0]['unsupported'][0]['error'])

    def test_an_unfadeable_draw_fades_with_gain_rather_than_refusing(self):
        # A coloured or soft member fades through its gain in the guest
        # (reveal_masks.rs apply), so only art that is never drawn refuses.
        by_scene = self.scene(controller('a', renderers=('f:1', 'f:2')))
        refuse_partial_one_way(by_scene, {0: [(0, 'f:1', 'bad'), (0, 'f:1', 'bad'),
                                              (0, 'f:1', 'bad'), (0, 'f:2', None)]})
        self.assertEqual(len(by_scene[0]['controllers']), 1)
        self.assertEqual(by_scene[0].get('unsupported', []), [])

    def test_survivors_are_renumbered_so_the_guest_pool_stays_contiguous(self):
        first = controller('a', renderers=('f:1', 'f:0'))
        second = dict(controller('b', renderers=('f:2',)), controller=1)
        third = dict(controller('c', one_way=False, renderers=('f:3',)), controller=2)
        by_scene = self.scene(first, second, third)
        moved = refuse_partial_one_way(by_scene, {0: [(0, 'f:1', None), (1, 'f:2', None)]})
        self.assertEqual([r['source'] for r in by_scene[0]['controllers']], ['b', 'c'])
        self.assertEqual([r['controller'] for r in by_scene[0]['controllers']], [0, 1])
        self.assertEqual(moved, {0: {1: 0, 2: 1}})

    def test_a_reversible_controller_keeps_the_per_draw_exception(self):
        # A mask that covers again can afford a static draw: the hero leaves and
        # the room is as it was. Only the one-way rule escalates.
        by_scene = self.scene(controller('a', one_way=False))
        refuse_partial_one_way(by_scene, {0: [(0, 'f:1', 'bad')]})
        self.assertEqual(len(by_scene[0]['controllers']), 1)
        self.assertEqual(by_scene[0]['unsupported'], [])


class OneWayBinding(unittest.TestCase):
    def bind(self, raw, one_way):
        region = {'chunk_id': 1, 'scene_id': 0, 'draws': 1, 'path': 'unused'}
        report = {'regions': [region]}
        by_scene = {0: {'controllers': [controller('a', one_way, ('f:1',))], 'unsupported': []}}
        bind_regions(report, by_scene, {1: ['f:1']}, {1: raw})
        return region, by_scene[0]

    def test_a_black_mask_binds_either_way(self):
        for one_way in (True, False):
            region, scene = self.bind(room(), one_way)
            self.assertEqual(region['reveal_mask_bindings'],
                             [{'controller': 0, 'draw': 0, 'renderer_source': 'f:1'}])
            self.assertEqual(scene['unsupported'], [])

    def test_a_soft_mask_binds_and_is_recorded_as_a_gain_fade(self):
        for one_way in (False, True):
            region, scene = self.bind(soft(room()), one_way)
            self.assertEqual(region['reveal_mask_bindings'],
                             [{'controller': 0, 'draw': 0, 'renderer_source': 'f:1'}])
            self.assertEqual(region['reveal_mask_gain'][0]['fade'], 'gain')
            self.assertEqual(len(scene['controllers']), 1)


if __name__ == '__main__':
    unittest.main()
