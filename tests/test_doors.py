"""Where a door leads, and every shape that must refuse to answer.

A door is the one gate whose destination is not in the component the cooker
reads. `TransitionPoint` carries `targetScene` and `entryPoint`, but a door with
a `Door Control` FSM never departs through them: the FSM listens for UP and
calls `BeginSceneTransition` with its own variables. Three of the seventeen
doors in the admitted scenes prove the component's fields are not read, because
they hold something else entirely.

The property these tests care about most is the refusal. A door with no edge is
visible the moment someone walks into it. A door with the wrong edge sends the
player into the wrong room, and nothing says so.
"""
import sys, unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))
sys.path.insert(0, str(ROOT / 'tests'))

from regions import door_destination
from test_activation import action_data

DOOR = 7  # any GameObject path id; the walker only compares it


def transition(scene='Room_temple', gate='left1', variables=None):
    """One `BeginSceneTransition`, optionally reading FSM variables.

    The action's own inline `sceneName`/`entryGateName` are stale editor
    defaults in the shipped game, `Room_temple` on every door in the admitted
    scenes, so a test that only pinned the literals would pin the wrong value.
    """
    data = action_data([('BeginSceneTransition', [('sceneName', 18, scene),
                                                  ('entryGateName', 18, gate)])])
    for index, name in enumerate(variables or {}):
        data['fsmStringParams'][index] = {'useVariable': 1, 'name': name, 'value': scene}
    return data


def fsm(name='Door Control', states=None, variables=None):
    return {'name': name, 'states': states if states is not None else [],
            'variables': {'stringVariables': [{'name': k, 'value': v}
                                              for k, v in (variables or {}).items()]}}


def scene(*components):
    """A stand-in carrying only what the door walker reads."""
    class Stub:
        objects = {index: ('PlayMakerFSM', {'m_GameObject': {'m_PathID': DOOR}, 'fsm': value})
                   for index, value in enumerate(components)}
    return Stub()


def door(states, variables=None, name='Door Control'):
    return door_destination(scene(fsm(name, states, variables)), DOOR)


class DoorWalkerTests(unittest.TestCase):
    def test_the_shipped_shape_reads_its_variables_not_its_literals(self):
        states = [{'name': 'Change Scene',
                   'actionData': transition('Room_temple', 'left1',
                                            {'New Scene': 1, 'Entry Gate': 1})}]
        self.assertEqual(door(states, {'New Scene': 'Room_shop', 'Entry Gate': 'left1'}),
                         ('Room_shop', 'left1'))

    def test_a_literal_destination_is_taken_as_written(self):
        states = [{'name': 'Change Scene', 'actionData': transition('Town', 'bot1')}]
        self.assertEqual(door(states), ('Town', 'bot1'))

    def test_the_state_may_be_called_anything(self):
        # The walker looks for the action, not for a state name, because a state
        # name is a label and the action is the behaviour.
        states = [{'name': 'Idle', 'actionData': action_data([])},
                  {'name': 'Somewhere Else', 'actionData': transition('Town', 'bot1')}]
        self.assertEqual(door(states), ('Town', 'bot1'))

    def test_a_variable_the_walker_cannot_resolve_refuses(self):
        states = [{'name': 'Change Scene',
                   'actionData': transition('Room_temple', 'left1',
                                            {'New Scene': 1, 'Entry Gate': 1})}]
        # The variable is not in the table, so the shipped value is unknown.
        self.assertIsNone(door(states, {'Entry Gate': 'left1'}))
        # Present but empty is just as unknown.
        self.assertIsNone(door(states, {'New Scene': '', 'Entry Gate': 'left1'}))

    def test_a_door_with_no_door_control_refuses(self):
        states = [{'name': 'Change Scene', 'actionData': transition('Town', 'bot1')}]
        # Town's door_dreamReturn carries only `Set Compass Point`, and its
        # destination is not in the scene at all.
        self.assertIsNone(door(states, name='Set Compass Point'))
        self.assertIsNone(door_destination(scene(), DOOR))

    def test_two_door_controls_or_two_transitions_refuse(self):
        states = [{'name': 'Change Scene', 'actionData': transition('Town', 'bot1')}]
        both = scene(fsm('Door Control', states), fsm('Door Control', states))
        self.assertIsNone(door_destination(both, DOOR))
        twice = [{'name': 'Change Scene', 'actionData': transition('Town', 'bot1')},
                 {'name': 'Other', 'actionData': transition('Crossroads_01', 'top1')}]
        self.assertIsNone(door(twice))

    def test_a_disabled_transition_is_not_a_destination(self):
        data = transition('Town', 'bot1')
        data['actionEnabled'] = [0]
        self.assertIsNone(door([{'name': 'Change Scene', 'actionData': data}]))

    def test_an_fsm_on_another_object_is_not_this_door(self):
        stub = scene(fsm('Door Control', [{'name': 'Change Scene',
                                           'actionData': transition('Town', 'bot1')}]))
        self.assertIsNone(door_destination(stub, DOOR + 1))


@unittest.skipUnless((ROOT / '.hkpsx/doctor.json').is_file(), 'needs the installed game')
class InstalledDoorTests(unittest.TestCase):
    """The three scenes whose doors the fix actually changes."""

    @classmethod
    def setUpClass(cls):
        from source import Source
        from scene import Scene
        cls.source = Source()
        cls.scenes = {name: Scene(cls.source, file) for name, file in
                      (('Town', 'level7'), ('Crossroads_01', 'level37'), ('Crossroads_06', 'level42'))}

    def doors(self, name, active_only=True):
        sc = self.scenes[name]
        out = {}
        for _, (kind, tree) in sc.objects.items():
            if kind != 'TransitionPoint':
                continue
            gid = tree['m_GameObject']['m_PathID']
            if active_only and not sc.active(gid):
                continue
            out[sc.gos[gid]['m_Name']] = (tree['targetScene'], tree['entryPoint'],
                                          door_destination(sc, gid))
        return out

    def test_dirtmouths_doors_name_the_scenes_their_fsms_load(self):
        # Sly's, the stag station's and Bretta's doors sit under the `open`
        # building children Check Opened turns off on a fresh save, so they
        # are read with the inactive doors included; none of them cooks a gate.
        town = self.doors('Town', active_only=False)
        active = self.doors('Town')
        for door in ('door_sly', 'door_station', 'door_bretta'):
            self.assertNotIn(door, active)
        self.assertEqual(town['door_sly'][:2], ('', ''), 'the component ships empty')
        self.assertEqual(town['door_sly'][2], ('Room_shop', 'left1'))
        self.assertEqual(town['door_station'][2], ('Room_Town_Stag_Station', 'left1'))
        self.assertEqual(town['door_bretta'][2], ('Room_Bretta', 'right1'))
        # door_dreamReturn has no Door Control, so it keeps producing no edge.
        self.assertIsNone(town['door_dreamReturn'][2])
        self.assertEqual(town['door_dreamReturn'][:2], ('', ''))
        # door_jiji already cooked an edge; the FSM agrees, so nothing moves.
        self.assertEqual(town['door_jiji'][:2], ('Room_Ouiji', 'left1'))
        self.assertEqual(town['door_jiji'][2], ('Room_Ouiji', 'left1'))
        # door_mapper agrees too, and stays gateless on its own evidence: the
        # TransitionPoint component itself ships disabled.
        self.assertEqual(town['door_mapper'][2], ('Room_mapper', 'left1'))

    def test_the_one_door_whose_two_records_disagree(self):
        door1 = self.doors('Crossroads_01')['door1']
        # The level author put the gate in targetScene and the scene in
        # entryPoint. 'bot1' is not a scene, so this cooks nothing today, and
        # `scene_metadata` deliberately leaves it that way: the walker reads the
        # FSM but the caller only fills an empty pair, never an argument.
        self.assertEqual(door1[:2], ('bot1', 'Town'))
        self.assertEqual(door1[2], ('Town', 'bot1'))

    def test_a_disagreeing_door_is_recorded_and_left_alone(self):
        from regions import scene_metadata
        sc = self.scenes['Crossroads_01']
        meta = scene_metadata(sc, {'scene_id': 2, 'scene_name': 'Crossroads_01', 'file': 'level37'})
        row = next(g for g in meta['gates'] if g['name'] == 'door1')
        self.assertEqual((row['target_scene'], row['entry_point']), ('bot1', 'Town'))
        self.assertEqual(row['door_control'],
                         {'target_scene': 'Town', 'entry_point': 'bot1',
                          'disagrees_with_serialized': True})

    def test_an_empty_pair_is_the_only_one_the_fsm_fills(self):
        from regions import scene_metadata
        meta = scene_metadata(self.scenes['Town'], {'scene_id': 1, 'scene_name': 'Town', 'file': 'level7'})
        rows = {g['name']: g for g in meta['gates']}
        # door_sly is the one door whose pair the FSM fills (door_destination is
        # tested on it above), but it is a child of Sly_shop/open, which
        # Dirtmouth's Check Opened turns off on a fresh save: no gate cooks.
        self.assertNotIn('door_sly', rows)
        # door_jiji already had the pair and the FSM agrees, so it is untouched.
        self.assertEqual((rows['door_jiji']['target_scene'], rows['door_jiji']['entry_point']),
                         ('Room_Ouiji', 'left1'))
        self.assertNotIn('disagrees_with_serialized', rows['door_jiji']['door_control'])
        # door_dreamReturn has no Door Control at all, so it carries no record.
        self.assertNotIn('door_control', rows['door_dreamReturn'])

    def test_the_shaman_temple_door_closes_a_one_way_pair(self):
        # Crossroads_ShamanTemple's left1 already targets Crossroads_06/door1.
        self.assertEqual(self.doors('Crossroads_06')['door1'][2],
                         ('Crossroads_ShamanTemple', 'left1'))

    def test_a_cooked_door_gate_is_entered_on_up_and_not_walked_through(self):
        """Side 5, because a door collider is too short for the point test.

        Every other gate is a tall side or a floor the Knight falls through, and
        `world::gate` tests the player's own origin against it. A door's collider
        is a strip of floor a quarter of a unit tall, and the origin sits 1.39
        units above the Knight's feet, so that test passes straight over it: the
        two doors in the catalogue cooked for several builds and neither could
        ever fire. The guest answers a side 5 gate with the hero body and an UP
        press instead, which is what `Door Control` itself does.
        """
        import re
        import sys
        # The gate table left data/regions.rs when the gates moved into the
        # world metadata bank, so this reads the cook's own resolver rather than
        # a generated file. Matching the old text found nothing and passed as a
        # missing table rather than failing as a missing gate.
        sys.path.insert(0, str(ROOT / 'host'))
        report = ROOT / 'data/regions.json'
        if not report.is_file():
            self.skipTest('run host/regions.py first')
        import json
        catalogue = json.loads(report.read_text())
        if catalogue.get('complete') is not True:
            self.skipTest('the cooked catalogue is unfinished; the cook this build runs will finish it')
        from world import resolve_gates
        params = (ROOT / 'data/params.rs').read_text()
        bottom = int(re.search(r'bottom:(-?\d+)', params).group(1)) / 65536
        # resolve_gates hands back trigger_bounds in world units; the old
        # regions.rs table held them as Q16, which is why this divided.
        doors = [(scene['scene_id'], list(gate['bounds']))
                 for scene in catalogue['scenes']
                 for gate in resolve_gates(catalogue, catalogue['regions'], scene)
                 if gate['side'] == 5]
        self.assertTrue(doors, 'the catalogue cooks at least one door gate')
        import json
        cooked = json.loads((ROOT / 'data/regions.json').read_text())['scenes']
        by_id = {scene['scene_id']: scene for scene in cooked}
        for scene_id, bounds in doors:
            # Shorter than the Knight's own origin sits above his feet, which is
            # the measurement that makes the point test wrong for these.
            self.assertLess(bounds[3] - bounds[1], -bottom, bounds)
            source = [g for g in by_id[scene_id]['gates']
                      if 'trigger_bounds' in g
                      and all(abs(a - b) < 1e-4 for a, b in zip(g['trigger_bounds'], bounds))]
            self.assertEqual(len(source), 1, bounds)
            # Only a TransitionPoint with a Door Control FSM becomes a door, and
            # a gate named for a side keeps that side rather than being one.
            self.assertIn('door_control', source[0])
            self.assertFalse(source[0]['name'].startswith(('left', 'right', 'top', 'bot')))

    def test_nothing_in_these_scenes_writes_the_variables_the_walker_reads(self):
        """The serialized variable is only the shipped value if nothing sets it."""
        from focus import action_fields
        writers = []
        for name, sc in self.scenes.items():
            for _, (kind, tree) in sc.objects.items():
                if kind != 'PlayMakerFSM':
                    continue
                for st in tree['fsm']['states']:
                    data = st['actionData']
                    for i, action in enumerate(data['actionNames']):
                        if not data['actionEnabled'][i]:
                            continue
                        if action.rsplit('.', 1)[-1] not in (
                                'SetStringValue', 'SetFsmString', 'BuildString', 'GetLanguageString'):
                            continue
                        fields = action_fields(data, i)
                        target = fields.get('stringVariable') or fields.get('storeResult') \
                            or fields.get('variableName')
                        if isinstance(target, dict) and target.get('name') in ('New Scene', 'Entry Gate'):
                            writers.append((name, tree['fsm']['name'], st['name']))
        self.assertEqual(writers, [])


if __name__ == '__main__':
    unittest.main()
