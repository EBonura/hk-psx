"""Source-free tests of the arena-gate and infected-vine recognizers.

Both recognizers decide something a wrong answer makes worse than no answer: a
gate's verdict either walls a room off or opens one the original keeps shut, and
a vine admitted without its output loses its blobs in silence. So every test
here is either "the authored shape decodes to the authored meaning" or "this
mutation is refused, by name".

The action tables are built the way the measured `BG Control` serializes them:
compact scalars index a shared byteData blob, owner and string parameters index
their own typed tables. tools/breakable_catalog.py --recognize is what checks
the definitions against the real source.
"""
import struct
import sys
import unittest
from pathlib import Path
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'host'))
import breakables as b
from false_knight import fsm_digest


class Table:
    """One state's actionData, accumulated parameter by parameter."""

    def __init__(self):
        self.names, self.enabled, self.starts = [], [], []
        self.param_names, self.kinds, self.positions, self.sizes = [], [], [], []
        self.data = bytearray()
        self.owners, self.strings, self.objects, self.floats = [], [], [], []

    def action(self, name, params, enabled=True):
        self.starts.append(len(self.param_names))
        self.names.append('HutongGames.PlayMaker.Actions.' + name)
        self.enabled.append(int(enabled))
        for param, kind, value in params:
            self.param_names.append(param)
            self.kinds.append(kind)
            if kind in (18, 19, 20):
                table = {18: self.strings, 19: self.objects, 20: self.owners}[kind]
                self.positions.append(len(table))
                self.sizes.append(0)
                table.append(value)
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
                'fsmOwnerDefaultParams': self.owners, 'fsmStringParams': self.strings,
                'fsmGameObjectParams': self.objects, 'fsmFloatParams': self.floats}


def boolean(value, variable=''):
    """A compact FsmBool: value byte, useVariable byte, then the variable name."""
    return bytes([int(bool(value)), int(bool(variable))]) + variable.encode()


def number(value):
    return struct.pack('<f', value) + b'\0'


def flag(value):
    return bytes([int(bool(value))])


OWNER = {'ownerOption': 0, 'gameObject': {'useVariable': 0, 'name': '',
                                          'value': {'m_FileID': 0, 'm_PathID': 0}}}
CHILD = {'ownerOption': 1, 'gameObject': {'useVariable': 1, 'name': 'Dust',
                                          'value': {'m_FileID': 0, 'm_PathID': 9}}}
TEXT = {'useVariable': 0, 'name': '', 'value': ''}


def state(name, table, transitions=()):
    return {'name': name, 'actionData': table.build(),
            'transitions': [{'fsmEvent': {'name': event}, 'toState': target}
                            for event, target in transitions]}


def opened(test_true=b.BG_CONTROL_CLOSE_EVENT, test_false='', variable=b.BG_CONTROL_PLACEMENT_VARIABLE,
           collider=False, extra=None, every_frame=False):
    table = Table()
    table.action('GetOwner', [('storeGameObject', 19, {'useVariable': 1, 'name': 'Self'})])
    table.action('BoolTest', [('boolVariable', 17, boolean(False, variable)),
                              ('isTrue', 23, test_true.encode()), ('isFalse', 23, test_false.encode()),
                              ('everyFrame', 1, flag(every_frame))])
    table.action('Tk2dPlayAnimation', [('gameObject', 20, OWNER), ('animLibName', 18, TEXT),
                                       ('clipName', 18, dict(TEXT, value='BG Opened'))])
    table.action('SetCollider', [('gameObject', 20, extra if extra else OWNER),
                                 ('active', 17, boolean(collider))])
    return state(b.BG_CONTROL_START_STATE, table,
                 [('BG CLOSE', 'Close 1'), (b.BG_CONTROL_CLOSE_EVENT, b.BG_CONTROL_CLOSE_STATE)])


def quick_close(collider=True, first='SetCollider', open_event=b.BG_CONTROL_OPEN_EVENT):
    table = Table()
    table.action(first, [('gameObject', 20, OWNER), ('active', 17, boolean(collider))])
    table.action('Wait', [('time', 15, number(.2)), ('finishEvent', 23, b'FINISHED'),
                          ('realTime', 1, flag(False))])
    return state(b.BG_CONTROL_CLOSE_STATE, table,
                 [(open_event, 'Open'), ('FINISHED', 'Double Close')])


def gate_fsm(start_closed=False, start=None, close=None, start_state=b.BG_CONTROL_START_STATE):
    return {'name': b.BG_CONTROL_FSM, 'startState': start_state, 'globalTransitions': [],
            'states': [start if start is not None else opened(),
                       close if close is not None else quick_close()],
            'variables': {'boolVariables': [{'name': b.BG_CONTROL_PLACEMENT_VARIABLE,
                                             'value': int(start_closed), 'useVariable': 1}],
                          'gameObjectVariables': [{'name': 'Self', 'useVariable': 1,
                                                   'value': {'m_FileID': 0, 'm_PathID': 0}}],
                          'categories': ['']}}


BOX = {'m_Size': {'x': .5, 'y': 4.0}, 'm_Offset': {'x': 0., 'y': 0.},
       'm_Enabled': 1, 'm_IsTrigger': 0}


def scene(parts, layers=None, positions=None, active=None, file='level44'):
    """A Scene stand-in exposing only what the recognizers read."""
    store, gos = {}, {}
    for gid, components in parts.items():
        refs = []
        for index, (typ, data) in enumerate(components, start=gid * 100):
            store[index] = (typ, data)
            refs.append({'component': {'m_FileID': 0, 'm_PathID': index}})
        gos[gid] = {'m_Name': f'Battle Gate {gid}', 'm_Component': refs,
                    'm_Layer': (layers or {}).get(gid, b.TERRAIN_LAYER)}
    origins = positions or {}

    def point(gid, x=0., y=0., z=0.):
        base = origins.get(gid, (10., 20., 0.))
        return (base[0] + x, base[1] + y, base[2] + z)

    # `sid` rather than the file name and a raw index, because the real one
    # resolves an additively merged object back to the file it was serialized
    # in. This stub has no merge, so it is the plain form.
    return SimpleNamespace(objects=store, gos=gos, source=None,
                           file=SimpleNamespace(name=f'/source/{file}'),
                           sid=lambda i: f'{file}:{i}',
                           point=point, active=lambda gid: (active or {}).get(gid, True))


def gate_scene(fsm=None, collider=None, extra=(), **kwargs):
    components = [('PlayMakerFSM', {'m_GameObject': {'m_FileID': 0, 'm_PathID': 7},
                                    'fsm': fsm if fsm is not None else gate_fsm()}),
                  ('BoxCollider2D', dict(BOX) if collider is None else collider)]
    return scene({7: components + list(extra)}, **kwargs)


class ArenaGateTests(unittest.TestCase):
    def test_a_gate_that_starts_open_is_not_solid_when_the_room_loads(self):
        found = b.battle_gate(gate_scene(), 7, gate_fsm(start_closed=False))
        self.assertFalse(found['solid_on_load'])
        self.assertFalse(found['start_closed'])
        # The join key the geometry cook needs, and the box it covers.
        self.assertEqual(found['collider_source'], 'level44:701')
        self.assertEqual(found['box'], [9.75, 18.0, 10.25, 22.0])
        self.assertFalse(found['opens_in_port'])

    def test_a_gate_that_starts_closed_is_solid_and_stays_that_way(self):
        found = b.battle_gate(gate_scene(), 7, gate_fsm(start_closed=True))
        self.assertTrue(found['solid_on_load'])
        self.assertEqual(found['opens_on'], b.BG_CONTROL_OPEN_EVENT)
        self.assertFalse(found['opens_in_port'])

    def test_every_departure_from_the_authored_shape_is_refused_by_name(self):
        cases = {
            'BG Control starts in': gate_fsm(start_state='Open'),
            'not the authored action sequence': gate_fsm(
                start=state(b.BG_CONTROL_START_STATE, Table().action('SetCollider', [
                    ('gameObject', 20, OWNER), ('active', 17, boolean(False))]))),
            'does not read the placement variable': gate_fsm(start=opened(variable='Closed')),
            'no longer only closes the gate': gate_fsm(start=opened(test_true='BG CLOSE')),
            'does not act on the gate itself': gate_fsm(start=opened(extra=CHILD)),
            'sets the gate collider to False': gate_fsm(start=opened(collider=True)),
            'quick close does nothing': gate_fsm(close=state(b.BG_CONTROL_CLOSE_STATE, Table())),
            'sets the gate collider to True': gate_fsm(close=quick_close(collider=False)),
            'expected SetCollider, found Wait': gate_fsm(close=quick_close(first='Wait')),
            'waits for the arena to open it': gate_fsm(close=quick_close(open_event='BG QUICK OPEN')),
        }
        for reason, fsm in cases.items():
            with self.subTest(reason):
                with self.assertRaises(ValueError) as caught:
                    b.battle_gate(gate_scene(fsm=fsm), 7, fsm)
                self.assertIn(reason, str(caught.exception))

    def test_a_gate_whose_collider_is_not_a_serialized_terrain_box_is_refused(self):
        cases = {
            'carries 2 colliders, not one': dict(extra=[('BoxCollider2D', dict(BOX))]),
            'collider is a PolygonCollider2D': dict(collider=None, extra=[]),
            'is not a serialized solid': dict(collider=dict(BOX, m_IsTrigger=1)),
            'not terrain': dict(layers={7: 13}),
        }
        for reason, kwargs in cases.items():
            with self.subTest(reason):
                built = gate_scene(**kwargs)
                if reason.startswith('collider is a'):
                    built.objects[701] = ('PolygonCollider2D', {'m_Points': {'m_Paths': [[]]},
                                                                'm_Offset': {'x': 0., 'y': 0.},
                                                                'm_Enabled': 1, 'm_IsTrigger': 0})
                with self.assertRaises(ValueError) as caught:
                    b.battle_gate(built, 7, gate_fsm())
                self.assertIn(reason, str(caught.exception))

    def test_a_disabled_collider_is_refused_rather_than_read_as_open(self):
        # An open gate and a gate whose box was switched off in the editor look
        # the same to the guest but are not the same authored object.
        with self.assertRaises(ValueError):
            b.battle_gate(gate_scene(collider=dict(BOX, m_Enabled=0)), 7, gate_fsm())

    def test_the_sweep_pins_the_definition_and_refuses_a_changed_one(self):
        fsm = gate_fsm(start_closed=True)
        built = gate_scene(fsm=fsm)
        errors = []
        self.assertEqual(b.battle_gates(built, errors=errors), [])
        self.assertEqual(len(errors), 1)
        self.assertIn('unverified BG Control variant', errors[0]['error'])
        # With the digest of this definition pinned, the same scene admits it,
        # and `Start Closed` stays outside the digest so both placements pass.
        pinned = b.BG_CONTROL_SHA256
        try:
            b.BG_CONTROL_SHA256 = fsm_digest(fsm, (b.BG_CONTROL_PLACEMENT_VARIABLE,))
            found = b.battle_gates(built)
            self.assertEqual(len(found), 1)
            self.assertTrue(found[0]['solid_on_load'])
            self.assertEqual(found[0]['source'], 'level44:700')
            self.assertEqual(b.battle_gates(gate_scene(fsm=gate_fsm(start_closed=False)))[0]['solid_on_load'],
                             False)
        finally:
            b.BG_CONTROL_SHA256 = pinned

    def test_an_inactive_gate_is_not_in_the_world_and_is_not_an_error(self):
        errors = []
        self.assertEqual(b.battle_gates(gate_scene(active={7: False}), errors=errors), [])
        self.assertEqual(errors, [])


VINE_BLOB = ('SpriteRenderer', {}), ('Animator', {}), ('Transform', {})
VINE_EFFECT = ('tk2dSpriteAnimator', {}), ('Transform', {})


def vine_component(blobs=(), effects=(), amount=5):
    return {'m_GameObject': {'m_FileID': 0, 'm_PathID': 7}, 'm_Enabled': 1,
            'blobs': [{'m_FileID': 0, 'm_PathID': gid} for gid in blobs],
            'effects': [{'m_FileID': 0, 'm_PathID': gid} for gid in effects],
            'spatterAmount': amount, 'spatterAngleMin': 40., 'spatterAngleMax': 140.,
            'spatterSpeedMin': 10., 'spatterSpeedMax': 20.,
            'audioPitchMin': .8, 'audioPitchMax': 1.1}


def vine_scene(component, z=0.004, blob_parts=VINE_BLOB, effect_parts=VINE_EFFECT, trigger=True):
    parts = {7: [(b.VINE_COMPONENT, component),
                 ('BoxCollider2D', dict(BOX, m_IsTrigger=int(trigger)))]}
    for ref in component['blobs']:
        parts[ref['m_PathID']] = list(blob_parts)
    for ref in component['effects']:
        parts[ref['m_PathID']] = list(effect_parts)
    return scene(parts, positions={7: (10., 20., z)}, file='level59')


class InfectedVineTests(unittest.TestCase):
    def test_a_vine_authors_output_the_port_cannot_produce_and_is_refused(self):
        errors = []
        component = vine_component(blobs=(11, 12), effects=(21, 22))
        self.assertEqual(b.infected_vines(vine_scene(component), errors=errors), [])
        self.assertEqual(len(errors), 1)
        reason = errors[0]['error']
        self.assertIn('2 vine blobs animate through Mecanim', reason)
        self.assertIn('5 blood spatters per blob come from the global pool', reason)
        self.assertIn('tk2d clips on separate objects', reason)

    def test_the_authored_shape_is_still_reported_when_the_contract_is_off(self):
        # The effect and art passes need to see every vine, refused or not.
        found = b.infected_vines(vine_scene(vine_component(blobs=(11,), effects=(21,))),
                                 contract=False)
        self.assertEqual(len(found), 1)
        self.assertEqual(found[0]['spatter'], [5, 40., 140., 10., 20.])
        self.assertEqual(found[0]['hit_tags'], list(b.VINE_HIT_TAGS))
        self.assertEqual(sorted(found[0]['destruction']['authored']),
                         [b.OUTPUT_ANIMATION, b.OUTPUT_PARTICLES, b.OUTPUT_VISUAL])
        self.assertFalse(found[0]['persistent'])

    def test_a_vine_outside_the_authored_depth_band_is_never_hittable(self):
        # BreakableInfectedVine.Start disables the collider and the component
        # itself, so admitting one of these would put a breakable where the
        # original has scenery.
        for z, inert in ((b.VINE_DEPTH_CENTRE + b.VINE_DEPTH_RANGE, False),
                         (b.VINE_DEPTH_CENTRE + b.VINE_DEPTH_RANGE + .05, True),
                         (b.VINE_DEPTH_CENTRE - b.VINE_DEPTH_RANGE - .05, True)):
            with self.subTest(z=z):
                built = vine_scene(vine_component(), z=z)
                self.assertEqual(b.infected_vine(built, 7, built.objects[700][1])['inert_by_depth'],
                                 inert)
                errors = []
                b.infected_vines(built, errors=errors)
                self.assertEqual(bool(errors) and 'outside the authored depth band' in errors[0]['error'],
                                 inert)

    def test_a_vine_with_no_output_at_all_is_admitted(self):
        # Nothing in the catalogue authors this, but it is the boundary the
        # contract draws: a vine that produces nothing loses nothing.
        found = b.infected_vines(vine_scene(vine_component(amount=0)))
        self.assertEqual(len(found), 1)
        self.assertEqual(found[0]['destruction']['refused_outputs'], [])

    def test_parts_outside_the_scene_and_a_solid_hit_box_are_refused(self):
        component = vine_component(blobs=(11,))
        component['blobs'][0]['m_FileID'] = 4
        errors = []
        b.infected_vines(vine_scene(component), errors=errors)
        self.assertIn('vine blob lives outside this scene', errors[0]['error'])
        errors = []
        b.infected_vines(vine_scene(vine_component(), trigger=False), errors=errors)
        self.assertIn('hit collider is not a trigger', errors[0]['error'])

    def test_a_blob_with_no_renderer_is_refused_rather_than_silently_dropped(self):
        errors = []
        b.infected_vines(vine_scene(vine_component(blobs=(11,)), blob_parts=(('Transform', {}),)),
                         errors=errors)
        self.assertIn('vine blob has no SpriteRenderer to hide', errors[0]['error'])


if __name__ == '__main__':
    unittest.main()
