"""Source-free tests of the shared breakable definitions and the output gate."""
import sys
import unittest
from pathlib import Path
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'host'))
import breakables as b
import break_effects as fx
import effects


def scene(components, name='level6'):
    """A Scene stand-in exposing only what the definitions read."""
    store = {}
    gos = {}
    for gid, parts in components.items():
        refs = []
        for index, (typ, data) in enumerate(parts, start=gid * 100):
            store[index] = (typ, data)
            refs.append({'component': {'m_FileID': 0, 'm_PathID': index}})
        gos[gid] = {'m_Name': f'object {gid}', 'm_Component': refs}
    return SimpleNamespace(objects=store, gos=gos,
                           file=SimpleNamespace(name=f'/source/{name}'), source=None)


BODY = {'m_BodyType': 0, 'm_Mass': 1.0, 'm_AngularDamping': 0.05, 'm_GravityScale': 1.0,
        'm_Constraints': 0, 'm_UseAutoMass': 0, 'm_LinearDamping': 0.0}
BOUNCE = {'bounceFactor': 0.5, 'speedThreshold': 1, 'playSound': 0,
          'playAnimationOnBounce': 0, 'sendFSMEvent': 0}
BOX = {'m_Size': {'x': 2.0, 'y': 4.0}, 'm_Offset': {'x': 0.5, 'y': 0.0}}


def fragment_parts(**changes):
    parts = {'Rigidbody2D': dict(BODY), 'SpriteRenderer': {'m_Sprite': None},
             'ObjectBounce': dict(BOUNCE), 'BoxCollider2D': dict(BOX),
             'SpinSelf': {'spinFactor': 4.0}}
    parts.update(changes)
    return [(typ, data) for typ, data in parts.items() if data is not None]


class RigidFragmentTests(unittest.TestCase):
    def test_the_definition_admits_the_measured_source_variants(self):
        found = b.rigid_fragment(scene({7: fragment_parts()}), 7)
        self.assertEqual(found['spin'], 'SpinSelf')
        self.assertEqual(found['spin_factor'], 4.0)
        self.assertEqual(found['collider_type'], 'BoxCollider2D')
        # SpinSelfSimple and a spinless part are both authored shapes.
        simple = b.rigid_fragment(scene({7: fragment_parts(
            SpinSelf=None, SpinSelfSimple={'spinFactor': 5.0, 'randomStartRotation': 0, 'waitForCall': 0})}), 7)
        self.assertEqual(simple['spin'], 'SpinSelfSimple')
        self.assertIsNone(b.rigid_fragment(scene({7: fragment_parts(SpinSelf=None)}), 7)['spin'])

    def test_a_part_that_is_not_a_rigid_fling_is_not_an_error(self):
        self.assertIsNone(b.rigid_fragment(scene({7: [('ParticleSystem', {}), ('Transform', {})]}), 7))

    def test_an_unmeasured_body_bounce_or_spin_is_refused_by_name(self):
        cases = {
            'unmeasured rigid fragment body': fragment_parts(Rigidbody2D=dict(BODY, m_GravityScale=0.25)),
            'auto mass': fragment_parts(Rigidbody2D=dict(BODY, m_UseAutoMass=1)),
            'unmeasured fragment bounce factor': fragment_parts(ObjectBounce=dict(BOUNCE, bounceFactor=0.75)),
            'drives sound': fragment_parts(ObjectBounce=dict(BOUNCE, playSound=1)),
            'no ObjectBounce': fragment_parts(ObjectBounce=None),
            'waits for a call': fragment_parts(SpinSelf=None, SpinSelfSimple={
                'spinFactor': 5.0, 'randomStartRotation': 0, 'waitForCall': 1}),
            'two spin behaviours': fragment_parts(SpinSelfSimple={
                'spinFactor': 5.0, 'randomStartRotation': 0, 'waitForCall': 0}),
            'exactly one collider': fragment_parts(PolygonCollider2D={'m_Points': {'m_Paths': []}, 'm_Offset': BOX['m_Offset']}),
        }
        for expected, parts in cases.items():
            with self.subTest(expected):
                with self.assertRaisesRegex(ValueError, expected):
                    b.rigid_fragment(scene({7: parts}), 7)

    def test_a_box_collider_becomes_the_same_four_local_points_as_a_polygon(self):
        parts = {typ: data for typ, data in fragment_parts()}
        self.assertEqual(effects.fragment_polygon({'BoxCollider2D': (0, BOX)}),
                         [[-0.5, -2.0], [1.5, -2.0], [1.5, 2.0], [-0.5, 2.0]])
        polygon = {'m_Points': {'m_Paths': [[{'x': 0.0, 'y': 0.0}, {'x': 1.0, 'y': 0.0}, {'x': 0.0, 'y': 1.0}]]},
                   'm_Offset': {'x': 0.0, 'y': 0.0}}
        self.assertEqual(effects.fragment_polygon({'PolygonCollider2D': (0, polygon)}),
                         [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]])
        with self.assertRaisesRegex(ValueError, 'composite'):
            effects.fragment_polygon({'PolygonCollider2D': (0, dict(polygon, m_Points={'m_Paths': [[], []]}))})
        self.assertIn('BoxCollider2D', parts)


def record(**changes):
    base = {'off_renderer_ids': [1], 'on_renderer_ids': [], 'debris': [],
            'audio': {'options': [{'name': 'breakable_wall_hit_1'}]},
            'mask_fades': [], 'event_errors': [],
            'forwarded_events': {'hit_receiver': None}}
    base.update(changes)
    return base


class DestructionContractTests(unittest.TestCase):
    def setUp(self):
        self.emitters = {}
        self.original = fx.part_emitter, fx.scene_gravity
        fx.scene_gravity = lambda source: -9.81
        def part_emitter(source, sc, gid, gravity):
            value = self.emitters.get(gid, 'rigid')
            if isinstance(value, ValueError):
                raise value
            return None if value == 'rigid' else value
        fx.part_emitter = part_emitter

    def tearDown(self):
        fx.part_emitter, fx.scene_gravity = self.original

    def contract(self, rec, parts=None):
        return b.destruction_contract(scene(parts or {}), rec)

    def test_an_instance_whose_outputs_all_arrive_is_admitted(self):
        found = self.contract(record())
        self.assertEqual(found['authored'], [b.OUTPUT_VISUAL, b.OUTPUT_COLLIDER, b.OUTPUT_AUDIO])
        self.assertEqual(found['missing'], [])
        self.assertEqual(found['refused_outputs'], [])

    def test_an_unreproducible_emitter_refuses_the_instance(self):
        self.emitters[7] = ValueError('unsupported particle modules')
        found = self.contract(record(debris=[{'game_object': 'level6:7'}]))
        self.assertEqual(found['refused_outputs'], [b.OUTPUT_PARTICLES])
        self.assertIn(b.OUTPUT_PARTICLES, found['authored'])

    def test_a_partial_emitter_set_still_refuses(self):
        """One burst arriving does not excuse the other silently vanishing."""
        self.emitters = {7: {'style': {}}, 8: ValueError('particle renderer material')}
        found = self.contract(record(debris=[{'game_object': 'level6:7'}, {'game_object': 'level6:8'}]))
        self.assertEqual(found['refused_outputs'], [b.OUTPUT_PARTICLES])
        self.assertEqual(found['particle_parts'], ['level6:7'])

    def test_a_break_clip_that_is_not_resident_is_reported_but_not_enforced(self):
        found = self.contract(record(audio={'options': [{'name': 'barrel_death_1'}]}))
        self.assertEqual([item['output'] for item in found['missing']], [b.OUTPUT_AUDIO])
        self.assertEqual(found['refused_outputs'], [])
        self.assertNotIn(b.OUTPUT_AUDIO, b.ENFORCED_OUTPUTS)

    def test_a_forwarded_mask_with_no_recognized_handler_refuses(self):
        found = self.contract(record(forwarded_events={'hit_receiver': 'level6:9'},
                                     event_errors=[{'error': 'HIT destination is not one enabled iTweenFadeTo action'}]))
        self.assertEqual(found['refused_outputs'], [b.OUTPUT_MASK])

    def test_a_rigid_fragment_is_authored_output_that_is_measured_not_enforced(self):
        parts = {7: fragment_parts()}
        found = self.contract(record(debris=[{'game_object': 'level6:7'}]), parts)
        self.assertIn(b.OUTPUT_FRAGMENTS, found['authored'])
        self.assertEqual(found['refused_outputs'], [])
        self.assertEqual(len(found['rigid_fragments']), 1)
        self.assertNotIn(b.OUTPUT_FRAGMENTS, b.ENFORCED_OUTPUTS)


class Zero(dict):
    """Any unnamed serialized field reads as zero, so a fixture names only what it tests."""
    def __missing__(self, key):
        return Zero()


class ParticleStyleTests(unittest.TestCase):
    """`style` is a wall of refusals; these pin the two that were wrong.

    The fixture satisfies the checks ahead of each one so the refusal under test
    is the one that fires, and stops at the first check after it.
    """
    def system(self, **changes):
        shape = Zero(enabled=1, type=10, m_Position=Zero(), m_Rotation=Zero(),
                     radius=Zero(), arc=Zero(), m_Scale=Zero())
        constant = Zero(minMaxState=0, scalar=0)
        base = Zero(playOnAwake=1, looping=0, scalingMode=0, moveWithTransform=1,
                    InitialModule=Zero(maxNumParticles=1, gravitySource=0,
                                       startColor=Zero(maxColor=Zero())),
                    ShapeModule=shape, UVModule=Zero(),
                    EmissionModule=Zero(m_BurstCount=0, rateOverDistance=constant))
        base.update(changes)
        return base

    def submodule(self, path_id):
        return Zero(enabled=1, subEmitters=[{'emitter': {'m_FileID': 0, 'm_PathID': path_id}}])

    def refusal(self, **changes):
        with self.assertRaises(Exception) as raised:
            fx.style(self.system(**changes), -9.81)
        return str(raised.exception)

    def test_a_submodule_with_only_null_emitters_does_not_refuse_its_parent(self):
        self.assertNotIn('SubModule', self.refusal(SubModule=self.submodule(0)))
        self.assertIn('SubModule', self.refusal(SubModule=self.submodule(4242)))

    def test_the_random_row_bound_is_a_row_count_not_an_allow_list(self):
        for rows, allowed in ((3, True), (4, True), (6, True), (fx.MAX_SHEET_ROWS, True),
                              (fx.MAX_SHEET_ROWS + 1, False), (0, False)):
            with self.subTest(rows=rows):
                uv = Zero(enabled=1, tilesX=1, animationType=1, rowMode=1, tilesY=rows)
                self.assertEqual('particle random row' not in self.refusal(UVModule=uv), allowed)


if __name__ == '__main__':
    unittest.main()
