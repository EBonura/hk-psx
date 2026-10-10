"""Checks that the Greenpath recognizers and their controllers agree on the numbers they share.

`host/plant_trap.py`, `host/moss_charger.py` and the Shaker section of `host/runner.py` prove each
placement against constants they carry a copy of; `shared/hk-sim/src/{plant_trap,moss_charger,runner}.rs`
carry the other copy. Two copies of a number are evidence only while they agree, and clip arrays are
indexed by position, so both are pinned here without needing the installed source.
"""
import re
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))
import fat_fly
import moss_charger
import plant_trap
import runner

SIM = ROOT / 'shared/hk-sim/src'
ONE = 65536


def text(name):
    return (SIM / name).read_text()


def rust_const(source, name):
    found = re.search(rf'pub const {name}: [^=]+=\s*(.+?);\n', source, re.S)
    if found is None:
        raise AssertionError(f'no pub const {name}')
    body = re.sub(r'\s+', '', found.group(1))
    nums = re.sub(r'(\d)_(\d)', r'\1\2', body)
    nums = re.sub(r'(\d)_(\d)', r'\1\2', nums)
    return eval(nums.replace('[', '(').replace(']', ')'), {'ONE': ONE, '__builtins__': {}})


def flat(value):
    return [flat(v) if isinstance(v, tuple) else v for v in value] if isinstance(value, tuple) else value


def variants(source, enum):
    body = re.search(rf'pub enum {enum} \{{(.*?)\n\}}', source, re.S)
    return re.findall(r'^\s{4}(\w+),$', body.group(1), re.M)


class PlantTrapTests(unittest.TestCase):
    def test_clip_slots_follow_the_controller(self):
        # `Clip::slot`: SnapReady 0, Snap 1, Retract and Rest 2.
        self.assertEqual(plant_trap.CLIP_SLOTS, ('ready', 'snap', 'retract'))
        slot = re.search(r'pub const fn slot\(self\) -> usize \{(.*?)\n    \}', text('plant_trap.rs'), re.S).group(1)
        self.assertIn('Clip::SnapReady => 0', slot)
        self.assertIn('Clip::Snap => 1', slot)
        self.assertIn('Clip::Retract | Clip::Rest => 2', slot)
        self.assertEqual(re.search(r'pub const COUNT: usize = (\d+);', text('plant_trap.rs')).group(1), '3')

    def test_the_jaw_boxes_are_the_ones_the_recognizer_proves(self):
        source = text('plant_trap.rs')
        snap = plant_trap.COLLIDERS['Snap']
        retract = plant_trap.COLLIDERS['Retract']
        self.assertEqual(flat(rust_const(source, 'SNAP_0')), snap[0])
        self.assertEqual(flat(rust_const(source, 'SNAP_1')), snap[1])
        self.assertEqual(flat(rust_const(source, 'SNAP_2')), snap[2])
        self.assertEqual(snap[2], retract[0])
        self.assertEqual(flat(rust_const(source, 'RETRACT_1')), retract[1])
        self.assertEqual(flat(rust_const(source, 'RETRACT_2')), retract[2])
        self.assertEqual(flat(rust_const(source, 'DETECT')), plant_trap.DETECT_Q16)

    def test_the_timings_are_the_authored_waits(self):
        source = text('plant_trap.rs')
        actions = plant_trap.ACTIONS
        self.assertEqual(rust_const(source, 'READY_TICKS'), round(actions[('Ready', 'Wait')]['time'] * 60))
        self.assertEqual(rust_const(source, 'SNAP_TICKS'), round(actions[('Snap', 'Wait')]['time'] * 60))
        self.assertEqual(rust_const(source, 'COOLDOWN_TICKS'), round(actions[('Cooldown', 'Wait')]['time'] * 60))
        frames, fps, _, _ = plant_trap.CLIPS['Retract']
        self.assertEqual(rust_const(source, 'RETRACT_TICKS'), round(frames * 60 / fps))


class MossChargerTests(unittest.TestCase):
    def test_clip_slots_follow_the_controller_in_declaration_order(self):
        source = text('moss_charger.rs')
        names = variants(source[source.index('pub enum Clip'):], 'Clip')
        snake = [re.sub(r'(?<!^)(?=[A-Z])', '_', name).lower() for name in names]
        self.assertEqual([slot for slot, _ in moss_charger.CLIP_SLOTS], snake)
        self.assertEqual(int(re.search(r'pub const COUNT: usize = (\d+);', source).group(1)), len(snake))

    def test_the_collider_boxes_are_the_ones_the_recognizer_proves(self):
        source = text('moss_charger.rs')
        for name, value in (('BIG', moss_charger.BIG), ('BIG_LOW', moss_charger.BIG_LOW), ('BIG_MID', moss_charger.BIG_MID),
                            ('BIG_HIGH', moss_charger.BIG_HIGH), ('STUN', moss_charger.STUN), ('RUN', moss_charger.RUN)):
            self.assertEqual(flat(rust_const(source, name)), value, name)

    def test_the_flings_and_the_ray_offsets_are_the_proved_ones(self):
        source = text('moss_charger.rs')
        fling = flat(rust_const(source, 'BURST_FLING'))
        actions = moss_charger.ACTIONS
        for cardinal, state in enumerate(('Fly Right', 'FlyUp', 'Fly Left', 'Fly Down')):
            fields = actions[(state, 'SetVelocityAsAngle')]
            self.assertEqual(fling[cardinal], [int(fields['angle']), int(fields['speed'])], state)
        self.assertEqual(rust_const(source, 'CHARGE_SPEED'), 15 * ONE)
        self.assertIn('EMERGE_SPEED: i32 = CHARGE_SPEED / 4;', source)
        self.assertEqual(moss_charger.ACTIONS[('Emerge', 'FloatMultiply')]['multiplyBy'], .25)
        self.assertEqual(rust_const(source, 'RUN_ACCELERATION'), ONE // 2)
        self.assertEqual(rust_const(source, 'RUN_MAX'), 10 * ONE)
        self.assertEqual(rust_const(source, 'BURST_HITS'), actions[('Line Loop', 'IntCompare')]['integer2'] + 1)


class ShakerTests(unittest.TestCase):
    def test_the_gas_polygon_is_the_one_the_recognizer_proves(self):
        source = text('runner.rs')
        gas = source[source.index('pub mod gas'):]
        self.assertEqual(flat(rust_const(gas, 'POLYGON')), runner.GAS_POLYGON_Q16)
        self.assertEqual(flat(rust_const(gas, 'ORIGIN')), runner.GAS_ORIGIN_Q16)

    def test_the_gas_timings_are_the_authored_waits(self):
        source = text('runner.rs')
        gas = source[source.index('pub mod gas'):]
        actions = runner.GAS_ACTIONS
        self.assertEqual(rust_const(gas, 'ANTIC_TICKS'), round(actions[('Attack Antic', 'Wait')]['time'] * 60))
        self.assertEqual(rust_const(gas, 'BURST_TICKS'), round(actions[('Attack', 'Wait')]['time'] * 60))
        self.assertEqual(rust_const(gas, 'COOL_TICKS'), round(actions[('CD', 'Wait')]['time'] * 60))
        self.assertEqual(rust_const(gas, 'IDLE_TICKS'), round(actions[('Idle Pause', 'Wait')]['time'] * 60))
        self.assertEqual(rust_const(gas, 'TWEEN_TICKS'), round(actions[('Attack', 'iTweenScaleTo')]['time'] * 60))
        self.assertEqual(rust_const(gas, 'START_SCALE'), round(actions[('Attack', 'SetScale')]['x'] * ONE))
        delay = runner.GAS_ACTIONS[('Attack Delay', 'WaitRandom')]
        self.assertIn(f"Self::Delay => [{round(delay['timeMax'] * 60)}, {round(delay['timeMin'] * 60)}]", source)

    def test_the_three_variants_have_distinct_digests(self):
        digests = {runner.FSM_SHA256, runner.LEAP_FSM_SHA256, runner.MOSSMAN_FSM_SHA256, runner.GAS_FSM_SHA256}
        self.assertEqual(len(digests), 4)


class FatFlyTests(unittest.TestCase):
    def test_the_constants_are_the_authored_numbers(self):
        source = text('fat_fly.rs')
        self.assertEqual(rust_const(source, 'SPEED'), 4 * ONE)
        self.assertEqual(rust_const(source, 'WAKE_DISTANCE'), int(fat_fly.ACTIONS[('fat fly bounce', 'Initialise', 'FloatCompare')]['float2']) * ONE)
        self.assertEqual(rust_const(source, 'ANTIC_TICKS'), round(fat_fly.ACTIONS[('Fatty Fly Attack', 'Attack Antic', 'Wait')]['time'] * 60))
        self.assertEqual(rust_const(source, 'COOLDOWN_TICKS'), round(fat_fly.ACTIONS[('Fatty Fly Attack', 'CD', 'Wait')]['time'] * 60))
        frames, fps, _ = fat_fly.CLIPS['Attack']
        self.assertEqual(rust_const(source, 'ATTACK_TICKS'), round(frames * 60 / fps))
        self.assertEqual(rust_const(source, 'ATTACK_TRIGGER_TICKS'), round(fat_fly.ATTACK_TRIGGER_FRAME * 60 / fps))
        self.assertEqual(rust_const(source, 'DECELERATION'), round(.1 * ONE))
        self.assertEqual(rust_const(source, 'SHOT_SPEED'), int(fat_fly.SHOT_SPEED) * ONE)
        # 12 * cos 45 degrees, to the nearest Q16 unit.
        self.assertEqual(rust_const(source, 'SHOT_DIAGONAL'), round(12 * 2 ** -.5 * ONE))


if __name__ == '__main__':
    unittest.main()
