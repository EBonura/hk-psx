"""Source-free tests of the hidden-wall and cracked-floor recognizers.

Both families take several hits and then take a piece of the room away, so a
wrong answer here is worse than no answer: an over-eager admission opens a wall
onto a black mask it cannot fade, and a selector that matches on the wrong thing
either misses instances or sweeps in strangers. So the tests are about exactly
those two risks.

What is pinned:

  Selection   The definition name is normalised and then confirmed twice, by
              state-name signature and by `false_knight.fsm_digest`. The three
              `Break Wall 2` instances ship under the definition name `FSM`,
              which names dozens of unrelated behaviours, so the digest is what
              actually selects and the name only labels.
  Routing     Whether the break reaches the mask is read from the serialized
              `sendToChildren` flag and the scene graph, never assumed. An
              inactive receiver is not a receiver.
  Refusal     The step 4 rule: a wall whose authored output the port cannot
              produce is refused rather than admitted without it.

tools/breakable_catalog.py --recognize is what checks these definitions against
the real source; this file checks that the code around them says no when it
should.
"""
import struct
import sys
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'host'))
import breakables as b
from false_knight import fsm_digest


class Table:
    """One state's actionData, accumulated parameter by parameter."""

    def __init__(self):
        self.names, self.enabled, self.starts = [], [], []
        self.param_names, self.kinds, self.positions, self.sizes = [], [], [], []
        self.data = bytearray()
        self.tables = {18: [], 19: [], 20: [], 24: [], 31: []}

    def action(self, name, params, enabled=True):
        self.starts.append(len(self.param_names))
        self.names.append('HutongGames.PlayMaker.Actions.' + name)
        self.enabled.append(int(enabled))
        for param, kind, value in params:
            self.param_names.append(param)
            self.kinds.append(kind)
            if kind in self.tables:
                self.positions.append(len(self.tables[kind]))
                self.sizes.append(0)
                self.tables[kind].append(value)
            else:
                self.positions.append(len(self.data))
                self.sizes.append(len(value))
                self.data.extend(value)
        return self

    def build(self):
        return {'actionNames': self.names, 'actionEnabled': self.enabled,
                'actionStartIndex': self.starts, 'paramName': self.param_names,
                'paramDataType': self.kinds, 'paramDataPos': self.positions,
                'paramByteDataSize': self.sizes, 'byteData': list(self.data),
                'fsmStringParams': self.tables[18], 'fsmGameObjectParams': self.tables[19],
                'fsmOwnerDefaultParams': self.tables[20], 'fsmObjectParams': self.tables[24],
                'fsmEventTargetParams': self.tables[31]}


def text(value=''):
    return {'useVariable': 0, 'name': '', 'value': value}


def number(value):
    return struct.pack('<f', value) + b'\0'


def flag(value):
    return bytes([int(bool(value))])


def event_target(variable='Self', to_children=True, target=1):
    """An FsmEventTarget aimed at the object one FSM variable holds."""
    return {'target': target, 'excludeSelf': text(0),
            'gameObject': {'ownerOption': 1, 'gameObject': {'useVariable': 1, 'name': variable,
                                                            'value': {'m_FileID': 0, 'm_PathID': 0}}},
            'fsmName': text(''), 'sendToChildren': text(int(bool(to_children))),
            'fsmComponent': {'m_FileID': 0, 'm_PathID': 0}}


def send(event, **kwargs):
    return [('eventTarget', 31, event_target(**kwargs)), ('sendEvent', 18, text(event)),
            ('delay', 15, number(0.)), ('everyFrame', 1, flag(False))]


def state(name, table=None, transitions=()):
    return {'name': name, 'actionData': (table or Table()).build(),
            'transitions': [{'fsmEvent': {'name': event}, 'toState': target}
                            for event, target in transitions]}


def wall_fsm(definition='breakable_wall_v2', shape=None, to_children=True, extra_states=(),
             variables=None, start=None, break_sends=None):
    """A stand-in carrying one of the pinned state-name signatures."""
    shape = shape or b.HIDDEN_WALL_SHAPES[WALL_V2]
    table = Table()
    for event, kwargs in (break_sends if break_sends is not None
                          else [(b.HIDDEN_WALL_UNCOVER, {'to_children': to_children})]):
        table.action('SendEventByName', send(event, **kwargs))
    states = [state(name, table if name == 'Break' else None) for name in sorted(shape['states'])]
    values = {'Facing': 2, 'Hits': b.HIDDEN_WALL_NAIL_HITS, 'Activated': 0, 'Ruin Lift': 0}
    values.update(variables or {})
    return {'name': definition, 'startState': start or shape['start'],
            'globalTransitions': [], 'states': states + list(extra_states),
            'variables': {'intVariables': [{'name': k, 'value': v} for k, v in values.items()
                                           if isinstance(v, int) and not isinstance(v, bool)],
                          'stringVariables': [{'name': k, 'value': v} for k, v in values.items()
                                              if isinstance(v, str)],
                          'categories': ['']}}


WALL_V2 = '5ed4602f92093bd200971a86dec0e40c9af85ca39e4bf40924b864b5ebfb47ea'
WALL_NAMED = 'e316a8d60e3e4f8269f5d3ac97cfbda4e2cb7715495a24778123019e3d534283'
FLOOR_STAGED = 'b74c4cdb9150a87be0a398b4d50bab0a1ebf40f99c2a1a746768b8573ed72d55'
FLOOR_WOOD = '72a2d729923a6564f3059f52037143bc90ad25cbf7975787bb5d4f59883d3f12'


def scene(tree, names, active=None, file='level46', positions=None):
    """A Scene stand-in: `tree` is child gid -> parent gid, 0 for a root."""
    store, gos, transforms, go_transform = {}, {}, {}, {}
    for gid, parent in tree.items():
        tid = gid + 1000
        transforms[tid] = {'m_GameObject': {'m_FileID': 0, 'm_PathID': gid},
                           'm_Father': {'m_FileID': 0, 'm_PathID': parent + 1000 if parent else 0},
                           'm_Children': []}
        go_transform[gid] = tid
        store[tid] = ('Transform', transforms[tid])
        gos[gid] = {'m_Name': names.get(gid, f'object {gid}'), 'm_Component': [],
                    'm_Layer': b.TERRAIN_LAYER}
    for gid, parent in tree.items():
        if parent:
            transforms[parent + 1000]['m_Children'].append({'m_FileID': 0, 'm_PathID': gid + 1000})
    return SimpleNamespace(objects=store, gos=gos, transforms=transforms,
                           go_transform=go_transform, source=None,
                           file=SimpleNamespace(name=f'/source/{file}'),
                           sid=lambda i: f'{file}:{i}',
                           point=lambda gid, *rest: (positions or {}).get(gid, (0.0, 0.0, 0.0)),
                           active=lambda gid: (active or {}).get(gid, True))


def owner_default(variable=None):
    """An FsmOwnerDefault: the FSM's own object, or the one a variable holds."""
    return {'ownerOption': 1 if variable else 0,
            'gameObject': {'useVariable': int(bool(variable)), 'name': variable or '',
                           'value': {'m_FileID': 0, 'm_PathID': 0}}}


def object_variable(name):
    return {'useVariable': 1, 'name': name, 'value': {'m_FileID': 0, 'm_PathID': 0}}


def vector(x, y, z, unset=False):
    """An FsmVector3 as PlayMaker byte-serializes it, with its UseVariable flag."""
    return struct.pack('<3f', x, y, z) + bytes([int(unset)])


def refs_fsm(finds=(), plays=(), owner='Self', spawn=None):
    """A wall-shaped FSM that binds children in `Get Refs` and plays them in `Break`."""
    refs = Table()
    if owner:
        refs.action('GetOwner', [('storeGameObject', 19, object_variable(owner))])
    for child, variable, target in finds:
        refs.action('FindChild', [('gameObject', 20, owner_default(target)),
                                  ('childName', 18, text(child)),
                                  ('storeResult', 19, object_variable(variable))])
    run = Table()
    for variable in plays:
        run.action('PlayParticleEmitter', [('gameObject', 20, owner_default(variable)),
                                           ('emit', 16, struct.pack('<i', 0) + b'\0')])
    if spawn is not None:
        point, offset, rotation = spawn
        run.action('CreateObject', [('gameObject', 19, object_variable('')),
                                    ('spawnPoint', 19, point),
                                    ('position', 28, offset), ('rotation', 28, rotation)])
    return {'name': 'breakable_wall_v2', 'startState': 'Get Refs', 'globalTransitions': [],
            'states': [state('Get Refs', refs), state('Break', run)],
            'variables': {'categories': ['']}}


class DefinitionNames(unittest.TestCase):
    """The definition name is normalised before it is compared to anything."""

    def test_an_editor_copy_suffix_is_not_a_different_definition(self):
        for raw in ('breakable_wall_v2', 'breakable_wall_v2 (1)', ' breakable_wall_v2 (12) '):
            self.assertEqual(b.definition_name({'name': raw}), 'breakable_wall_v2')

    def test_a_number_inside_the_name_survives(self):
        self.assertEqual(b.definition_name({'name': 'Break Wall 2'}), 'Break Wall 2')


class Selection(unittest.TestCase):
    """Signature screens, digest decides, definition name only labels."""

    def test_an_unrelated_fsm_is_not_a_candidate_at_all(self):
        fsm = {'name': 'glow_bug', 'startState': 'Idle', 'globalTransitions': [],
               'states': [state('Idle')], 'variables': {'categories': ['']}}
        self.assertIsNone(b.hidden_wall_shape(fsm))
        self.assertIsNone(b.cracked_floor_shape(fsm))

    def test_the_signature_alone_does_not_admit_an_edited_variant(self):
        # Same states, different serialized actions: the shape a name-and-shape
        # selector would take and the digest refuses.
        with self.assertRaises(ValueError) as caught:
            b.hidden_wall_shape(wall_fsm())
        self.assertIn('unverified hidden wall variant', str(caught.exception))

    def test_a_pinned_wall_reports_its_shape_and_its_digest(self):
        fsm = wall_fsm()
        digest = fsm_digest(fsm, b.HIDDEN_WALL_PLACEMENT_VARIABLES)
        with patch.dict(b.HIDDEN_WALL_SHAPES,
                        {digest: dict(b.HIDDEN_WALL_SHAPES[WALL_V2], states=frozenset(
                            s['name'] for s in fsm['states']))}):
            found = b.hidden_wall_shape(fsm)
        self.assertEqual(found['sha256'], digest)
        self.assertEqual(found['definition'], 'breakable_wall_v2')
        self.assertEqual(found['uncover'], 'children')

    def test_the_right_digest_under_the_wrong_definition_is_refused(self):
        # A digest excludes placement variables and PPtrs, so it cannot see the
        # definition name on its own; the name is checked separately rather than
        # trusted, because that is the pair that identifies the family.
        fsm = wall_fsm(definition='unmasker')
        digest = fsm_digest(fsm, b.HIDDEN_WALL_PLACEMENT_VARIABLES)
        with patch.dict(b.HIDDEN_WALL_SHAPES,
                        {digest: dict(b.HIDDEN_WALL_SHAPES[WALL_V2], states=frozenset(
                            s['name'] for s in fsm['states']))}):
            with self.assertRaises(ValueError) as caught:
                b.hidden_wall_shape(fsm)
        self.assertIn("under definition 'unmasker'", str(caught.exception))

    def test_a_wall_that_starts_somewhere_else_is_refused(self):
        fsm = wall_fsm(start='Idle')
        digest = fsm_digest(fsm, b.HIDDEN_WALL_PLACEMENT_VARIABLES)
        with patch.dict(b.HIDDEN_WALL_SHAPES,
                        {digest: dict(b.HIDDEN_WALL_SHAPES[WALL_V2], states=frozenset(
                            s['name'] for s in fsm['states']))}):
            with self.assertRaises(ValueError) as caught:
                b.hidden_wall_shape(fsm)
        self.assertIn("starts in 'Idle'", str(caught.exception))

    def test_the_catalogue_shapes_agree_with_themselves(self):
        # The four pins are the measured ones; this only guards the tables from
        # drifting apart from the family they claim to describe.
        for digest, shape in b.HIDDEN_WALL_SHAPES.items():
            self.assertEqual(len(digest), 64)
            self.assertIn('Break', shape['states'])
            self.assertIn(shape['start'], shape['states'])
            self.assertIn(shape['uncover'], ('children', 'name'))
        for digest, shape in b.CRACKED_FLOOR_SHAPES.items():
            self.assertEqual(len(digest), 64)
            self.assertIn('Break', shape['states'])
            self.assertIn(shape['start'], shape['states'])
        self.assertIn(WALL_V2, b.HIDDEN_WALL_SHAPES)
        self.assertIn(WALL_NAMED, b.HIDDEN_WALL_SHAPES)
        self.assertEqual(set(b.CRACKED_FLOOR_SHAPES), {FLOOR_STAGED, FLOOR_WOOD})


class UncoverRouting(unittest.TestCase):
    """Which objects the break actually reaches, read rather than assumed."""

    def wall_scene(self, **kwargs):
        # wall -> Masks -> Mask 1, plus a mask the scene ships inactive.
        return scene({1: 0, 2: 1, 3: 2, 4: 2},
                     {1: 'Breakable Wall', 2: 'Masks', 3: 'Mask 1', 4: 'Mask 1 (1)'}, **kwargs)

    def test_the_broadcast_reaches_every_active_descendant(self):
        found = b._uncover_targets(self.wall_scene(), 1, wall_fsm(),
                                   b.HIDDEN_WALL_SHAPES[WALL_V2])
        self.assertEqual(found, [2, 3, 4])

    def test_an_inactive_receiver_is_not_a_receiver(self):
        # Crossroads_18 ships two of its four mask objects inactive; an FSM that
        # is not running cannot answer the event, so counting it would turn a
        # wall that works into a wall refused for a mask nobody can see.
        found = b._uncover_targets(self.wall_scene(active={4: False}), 1, wall_fsm(),
                                   b.HIDDEN_WALL_SHAPES[WALL_V2])
        self.assertEqual(found, [2, 3])

    def test_a_subtree_wall_that_stops_broadcasting_to_children_is_refused(self):
        with self.assertRaises(ValueError) as caught:
            b._uncover_targets(self.wall_scene(), 1, wall_fsm(to_children=False),
                               b.HIDDEN_WALL_SHAPES[WALL_V2])
        self.assertIn('no longer reaches its children', str(caught.exception))

    def test_a_wall_that_stops_broadcasting_uncover_at_all_is_refused(self):
        with self.assertRaises(ValueError) as caught:
            b._uncover_targets(self.wall_scene(), 1,
                               wall_fsm(break_sends=[('BREAK', {})]),
                               b.HIDDEN_WALL_SHAPES[WALL_V2])
        self.assertIn('no longer broadcasts UNCOVER', str(caught.exception))

    def test_the_named_shape_resolves_its_mask_by_name_exactly_once(self):
        shape = b.HIDDEN_WALL_SHAPES[WALL_NAMED]
        fsm = wall_fsm(definition='FSM', shape=shape, to_children=False,
                       variables={'Mask Name': 'break_wall_masks'})
        found = b._uncover_targets(scene({1: 0, 9: 0}, {1: 'Break Wall 2', 9: 'break_wall_masks'}),
                                   1, fsm, shape)
        self.assertEqual(found, [9])

    def test_a_name_that_resolves_to_none_or_to_two_objects_is_refused(self):
        shape = b.HIDDEN_WALL_SHAPES[WALL_NAMED]
        fsm = wall_fsm(definition='FSM', shape=shape, to_children=False,
                       variables={'Mask Name': 'break_wall_masks'})
        cases = {
            'names 0 active scene objects': scene({1: 0}, {1: 'Break Wall 2'}),
            'names 2 active scene objects': scene(
                {1: 0, 9: 0, 10: 0},
                {1: 'Break Wall 2', 9: 'break_wall_masks', 10: 'break_wall_masks'}),
        }
        for reason, sc in cases.items():
            with self.subTest(reason):
                with self.assertRaises(ValueError) as caught:
                    b._uncover_targets(sc, 1, fsm, shape)
                self.assertIn(reason, str(caught.exception))

    def test_an_inactive_duplicate_does_not_make_the_name_ambiguous(self):
        shape = b.HIDDEN_WALL_SHAPES[WALL_NAMED]
        fsm = wall_fsm(definition='FSM', shape=shape, to_children=False,
                       variables={'Mask Name': 'break_wall_masks'})
        sc = scene({1: 0, 9: 0, 10: 0},
                   {1: 'Break Wall 2', 9: 'break_wall_masks', 10: 'break_wall_masks'},
                   active={10: False})
        self.assertEqual(b._uncover_targets(sc, 1, fsm, shape), [9])

    def test_a_named_wall_that_also_broadcasts_to_children_is_refused(self):
        shape = b.HIDDEN_WALL_SHAPES[WALL_NAMED]
        fsm = wall_fsm(definition='FSM', shape=shape, to_children=True,
                       variables={'Mask Name': 'break_wall_masks'})
        with self.assertRaises(ValueError) as caught:
            b._uncover_targets(scene({1: 0, 9: 0}, {1: 'Break Wall 2', 9: 'break_wall_masks'}),
                               1, fsm, shape)
        self.assertIn('also broadcasts to children', str(caught.exception))

    def test_a_named_wall_with_no_mask_name_is_refused(self):
        shape = b.HIDDEN_WALL_SHAPES[WALL_NAMED]
        fsm = wall_fsm(definition='FSM', shape=shape, to_children=False,
                       variables={'Mask Name': ''})
        with self.assertRaises(ValueError) as caught:
            b._uncover_targets(scene({1: 0}, {1: 'Break Wall 2'}), 1, fsm, shape)
        self.assertIn('names no mask to uncover', str(caught.exception))


class BreakAudio(unittest.TestCase):
    """Break audio is measured against the resident bank and never enforced."""

    def test_a_resident_clip_is_authored_output_with_nothing_missing(self):
        authored, missing = [], []
        b._audio_output(list(b.RESIDENT_BREAK_CLIPS), authored, missing)
        self.assertEqual(authored, [b.OUTPUT_AUDIO])
        self.assertEqual(missing, [])

    def test_an_absent_clip_is_reported_by_name_and_does_not_refuse(self):
        authored, missing = [], []
        b._audio_output(['breakable_wall_death', 'secret_discovered_temp'], authored, missing)
        self.assertEqual(missing[0]['output'], b.OUTPUT_AUDIO)
        self.assertIn('breakable_wall_death, secret_discovered_temp', missing[0]['reason'])
        self.assertNotIn(b.OUTPUT_AUDIO, b.ENFORCED_OUTPUTS)

    def test_a_silent_break_authors_no_audio_at_all(self):
        authored, missing = [], []
        b._audio_output([], authored, missing)
        self.assertEqual((authored, missing), ([], []))


class ContractShape(unittest.TestCase):
    """The step 4 rule, as the two families spell it out."""

    def test_the_outputs_a_missing_reason_can_refuse_are_the_shared_ones(self):
        # Nothing family-specific is enforced: a hidden wall is refused by the
        # same three outputs a barrel is, so the two answers stay comparable.
        self.assertEqual(b.ENFORCED_OUTPUTS,
                         (b.OUTPUT_VISUAL, b.OUTPUT_PARTICLES, b.OUTPUT_MASK))

    def test_the_measured_hit_counts_are_the_serialized_ones(self):
        self.assertEqual(b.HIDDEN_WALL_NAIL_HITS, 4)
        self.assertEqual(b.CRACKED_FLOOR_NAIL_HITS, 3)
        self.assertEqual(b.HIDDEN_WALL_SPELL_ATTACK_TYPE, 2)

    def test_a_particle_emitter_the_model_refuses_is_authored_and_missing(self):
        def refuse(source, sc, gid, gravity, played=False):
            raise ValueError('particle simulation space')

        authored, missing = [], []
        sc = scene({1: 0, 2: 1}, {1: 'Breakable Wall', 2: 'Particle_rocks_large'})
        with patch('break_effects.part_emitter', refuse):
            b._particle_outputs(sc, [2], -60.0, authored, missing)
        self.assertEqual(authored, [b.OUTPUT_PARTICLES])
        self.assertEqual(missing[0], {'output': b.OUTPUT_PARTICLES, 'part': 'level46:2',
                                      'reason': 'particle simulation space'})

    def test_an_emitter_the_model_runs_is_authored_and_not_missing(self):
        authored, missing = [], []
        sc = scene({1: 0, 2: 1}, {1: 'Breakable Wall', 2: 'Particle_rocks_large'})
        with patch('break_effects.part_emitter', lambda *a, **k: {'style': {}}):
            b._particle_outputs(sc, [2], -60.0, authored, missing)
        self.assertEqual((authored, missing), ([b.OUTPUT_PARTICLES], []))

    def test_a_part_that_is_not_an_emitter_authors_nothing(self):
        authored, missing = [], []
        sc = scene({1: 0, 2: 1}, {1: 'Breakable Wall', 2: 'Camera Locks'})
        with patch('break_effects.part_emitter', lambda *a, **k: None):
            b._particle_outputs(sc, [2], -60.0, authored, missing)
        self.assertEqual((authored, missing), ([], []))


class RevealRefusalNamesTheWall(unittest.TestCase):
    """A collider-less secret mask says which wall it is waiting on."""

    def test_the_refusal_names_the_driver_when_there_is_one(self):
        import reveal_masks
        driver = {'source': 'level46:8629', 'name': 'Breakable Wall',
                  'definition': 'breakable_wall_v2'}
        with patch.object(b, 'uncover_drivers', lambda sc: {7: driver}):
            message = reveal_masks.no_collider_reason(None, 7)
        self.assertTrue(message.startswith(reveal_masks.NO_COLLIDER))
        self.assertIn('Breakable Wall (level46:8629, definition breakable_wall_v2)', message)
        self.assertIn('has to be admitted first', message)

    def test_a_mask_with_no_driver_keeps_the_plain_refusal(self):
        import reveal_masks
        with patch.object(b, 'uncover_drivers', lambda sc: {}):
            message = reveal_masks.no_collider_reason(None, 7)
        self.assertEqual(message, reveal_masks.NO_COLLIDER)


class ResolvedActions(unittest.TestCase):
    """An emitter an action starts, and a prefab spawn, are read not assumed.

    Both families author one-shot emitters with `playOnAwake` off, waiting for a
    `PlayParticleEmitter`. Reading that as "nothing starts this" refused the
    whole family on a property that was never the blocker, so the action is
    traced back to the child it names. The same applies to the break's
    `CreateObject`: its prefab and its place are both literals, so the emitters
    it brings resolve at cook time instead of needing a runtime that spawns.
    """

    def wall(self):
        return scene({1: 0, 2: 1, 3: 1}, {1: 'Breakable Wall', 2: 'Particle_rocks_small',
                                          3: 'Particle_rocks_large'},
                     positions={1: (10.0, 20.0, -0.1)})

    def test_a_variable_a_literal_find_child_binds_resolves_to_that_child(self):
        fsm = refs_fsm(finds=[('Particle_rocks_large', 'Particles Large', None)],
                       plays=['Particles Large'])
        self.assertEqual(b._played_emitters(self.wall(), 1, fsm), {3})

    def test_a_lookup_aimed_at_another_object_binds_nothing(self):
        # `FindChild` on a variable that is not the one GetOwner stores looks
        # somewhere this port has not resolved, so its result is not a binding.
        fsm = refs_fsm(finds=[('Particle_rocks_large', 'Particles Large', 'Masks')],
                       plays=['Particles Large'])
        self.assertEqual(b._played_emitters(self.wall(), 1, fsm), set())

    def test_an_ambiguous_child_name_binds_nothing(self):
        sc = scene({1: 0, 2: 1, 3: 1}, {1: 'Breakable Wall', 2: 'Dust', 3: 'Dust'})
        fsm = refs_fsm(finds=[('Dust', 'Dust Break 1', None)], plays=['Dust Break 1'])
        self.assertEqual(b._played_emitters(sc, 1, fsm), set())

    def test_an_action_on_the_owner_plays_the_owner(self):
        self.assertEqual(b._played_emitters(self.wall(), 1, refs_fsm(plays=[None])), {1})

    def test_a_played_emitter_is_the_only_one_the_flag_reaches(self):
        fsm = refs_fsm(finds=[('Particle_rocks_large', 'Particles Large', None)],
                       plays=['Particles Large'])
        seen = {}

        def emitter(source, sc, gid, gravity, played=False):
            seen[gid] = played
            return {'style': {}}

        authored, missing = [], []
        with patch('break_effects.part_emitter', emitter):
            b._particle_outputs(self.wall(), [2, 3], -60.0, authored, missing,
                                b._played_emitters(self.wall(), 1, fsm))
        self.assertEqual(seen, {2: False, 3: True})
        self.assertEqual(missing, [])

    def test_get_owner_names_the_variable_that_holds_the_fsm_object(self):
        self.assertEqual(b._owner_variable(refs_fsm()), 'Self')
        self.assertIsNone(b._owner_variable(refs_fsm(owner=None)))

    def spawn_slots(self, point, offset, rotation):
        fsm = refs_fsm(spawn=(point, offset, rotation))
        data = fsm['states'][1]['actionData']
        return fsm, data, b._action_slots(data, 0)

    def test_an_unset_vector_reads_as_unset_rather_than_as_zero(self):
        _, data, slots = self.spawn_slots(object_variable('Self'), vector(0, 0, 0, unset=True),
                                          vector(-72.5, -180.0, -180.0))
        self.assertIsNone(b._vector_parameter(data, slots['position']))
        self.assertEqual(b._vector_parameter(data, slots['rotation']), (-72.5, -180.0, -180.0))

    def test_the_spawn_is_the_owner_position_plus_the_literal_offset(self):
        fsm, data, slots = self.spawn_slots(object_variable('Self'), vector(0, -2, 0),
                                            vector(-72.5, -180.0, -180.0))
        origin, rotation = b._spawn_transform(self.wall(), 1, fsm, data, slots)
        self.assertEqual([round(v, 4) for v in origin], [10.0, 18.0, -0.1])
        self.assertEqual(rotation, (-72.5, -180.0, -180.0))

    def test_an_unset_offset_leaves_the_owner_position_alone(self):
        fsm, data, slots = self.spawn_slots(object_variable('Self'), vector(0, 0, 0, unset=True),
                                            vector(0, 0, 0))
        origin, _ = b._spawn_transform(self.wall(), 1, fsm, data, slots)
        self.assertEqual([round(v, 4) for v in origin], [10.0, 20.0, -0.1])

    def test_a_spawn_point_that_is_not_the_owner_is_refused(self):
        fsm, data, slots = self.spawn_slots(object_variable('Masks'), vector(0, 0, 0),
                                            vector(0, 0, 0))
        with self.assertRaises(ValueError) as caught:
            b._spawn_transform(self.wall(), 1, fsm, data, slots)
        self.assertIn("'Masks' is not the object GetOwner stores", str(caught.exception))

    def test_a_rotation_taken_from_the_spawn_point_is_refused(self):
        fsm, data, slots = self.spawn_slots(object_variable('Self'), vector(0, 0, 0),
                                            vector(0, 0, 0, unset=True))
        with self.assertRaises(ValueError) as caught:
            b._spawn_transform(self.wall(), 1, fsm, data, slots)
        self.assertIn('takes its rotation from the spawn point', str(caught.exception))


if __name__ == '__main__':
    unittest.main()
