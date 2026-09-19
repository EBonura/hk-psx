"""The guest camera's CameraShake against the serialized source FSM.

Two halves. The Rust half runs `game/src/camera.rs` itself over a stub world,
so the arbitration and the decay are exercised rather than described. The Python
half re-reads `resources.assets:22920` and checks the table the guest compiled
in against the extents, durations and priorities the source actually serializes,
because that table is the whole difference between the four shakes.
"""
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))

# CameraShake on _GameCameras/CameraParent, the FSM every SendEventByName
# "<name>Shake" in the game reaches.
CAMERA_SHAKE = ('resources.assets', 22920)
# (FSM variable, ShakingX state, Priority, Duration seconds) in the order
# game/src/camera.rs declares `Shake`.
EXPECTED = (('SmallShake', 'ShakingSmall', 3.0, 0.5),
            ('EnemyKillShake', 'ShakingKill', 6.0, 0.5),
            ('AverageShake', 'ShakingAverage', 7.0, 1.0),
            ('BigShake', 'ShakingBig', 10.0, 1.0))


def guest_table():
    """The SHAKES rows compiled into the guest, parsed out of the source file."""
    text = (ROOT / 'game/src/camera.rs').read_text()
    body = text.split('const SHAKES: [(i32, u16, u8); 4] = [', 1)[1].split('];', 1)[0]
    return [tuple(int(v) for v in row.strip(' ()').split(',')) for row in body.split('), (')]


class CameraShakeTests(unittest.TestCase):
    def test_actual_camera_module_shakes_decays_and_arbitrates(self):
        with tempfile.TemporaryDirectory(prefix='hk-camera-shake-') as tmp:
            binary = Path(tmp) / 'test'
            source = ROOT / 'tests/camera_shake_runtime.rs'
            compiled = subprocess.run(['rustc', '--edition=2021', '-Awarnings', '--test',
                                       str(source), '-o', str(binary)],
                                      env=dict(os.environ), capture_output=True, text=True)
            self.assertEqual(compiled.returncode, 0, compiled.stdout + compiled.stderr)
            result = subprocess.run([str(binary)], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_guest_table_matches_the_serialized_shake_fsm(self):
        from source import Source
        from focus import action_fields
        s = Source()
        fsm = s.read(s.file(CAMERA_SHAKE[0]).objects[CAMERA_SHAKE[1]])['fsm']
        self.assertEqual(fsm['name'], 'CameraShake')
        extents = {v['name']: v['value'] for v in fsm['variables']['vector3Variables']}
        states = {state['name']: state for state in fsm['states']}
        # Every shake an enemy can send must still route through its own
        # To <name> Shake arbiter; a missing one would silently never fire.
        routes = {t['fsmEvent']['name']: t['toState'] for t in fsm['globalTransitions']}
        for (variable, state_name, priority, duration), row in zip(EXPECTED, guest_table(), strict=True):
            self.assertIn(variable, routes, variable)
            data = states[state_name]['actionData']
            actions = {name.rsplit('.', 1)[-1]: i for i, name in enumerate(data['actionNames'])
                       if data['actionEnabled'][i]}
            shake = action_fields(data, actions['ShakePositionV2'])
            self.assertEqual(shake['Duration']['value'], duration, state_name)
            # FpsLimit is the action's own sample rate; the guest runs it once
            # per 60 Hz simulation tick, so the two have to agree.
            self.assertEqual(shake['FpsLimit']['value'], 60.0, state_name)
            self.assertTrue(shake['IsCameraShake']['value'], state_name)
            self.assertFalse(shake['IsLooping']['value'], state_name)
            # SetFloatValue writes Priority; To <name> Shake refuses anything
            # that is not strictly below it.
            self.assertEqual(action_fields(data, actions['SetFloatValue'])['floatValue']['value'],
                             priority, state_name)
            gate = states[routes[variable]]['actionData']
            arbiter = action_fields(gate, next(i for i, name in enumerate(gate['actionNames'])
                                               if name.endswith('.FloatCompare') and gate['actionEnabled'][i]))
            self.assertEqual(arbiter['float2']['value'], priority, variable)
            self.assertEqual(arbiter['lessThan'], 'FINISHED', variable)
            self.assertEqual(arbiter['greaterThan'], '', variable)
            self.assertEqual(arbiter['equal'], '', variable)
            axes = extents[variable]
            self.assertEqual(axes['x'], axes['y'], variable)
            self.assertEqual(axes['z'], 0.0, variable)
            self.assertEqual(row, (round(axes['x'] * 65536), round(duration * 60), int(priority)),
                             f'{variable} row in game/src/camera.rs')


if __name__ == '__main__':
    unittest.main()
