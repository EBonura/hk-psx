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

    def doors(self, name):
        sc = self.scenes[name]
        out = {}
        for _, (kind, tree) in sc.objects.items():
            if kind != 'TransitionPoint':
                continue
            gid = tree['m_GameObject']['m_PathID']
            if not sc.active(gid):
                continue
            out[sc.gos[gid]['m_Name']] = (tree['targetScene'], tree['entryPoint'],
                                          door_destination(sc, gid))
        return out

    def test_dirtmouths_doors_name_the_scenes_their_fsms_load(self):
        town = self.doors('Town')
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
        self.assertEqual((rows['door_sly']['target_scene'], rows['door_sly']['entry_point']),
                         ('Room_shop', 'left1'))
        self.assertNotIn('disagrees_with_serialized', rows['door_sly']['door_control'])
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
