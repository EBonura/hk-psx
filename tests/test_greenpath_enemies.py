"""The Moss Charger's and Fat Fly's recognizers against the constants their controllers hold, without retail data."""
import re
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))

import fat_fly  # noqa: E402
import moss_charger  # noqa: E402


def rust_consts(name):
    text = (ROOT / 'shared/hk-sim/src' / name).read_text()
    out = {}
    for key, body in re.findall(r'pub const (\w+): [^=]+= ([^;]+);', text):
        out[key] = body.replace('_', '').strip()
    return out


def boxes(body):
    return [int(v) for v in re.findall(r'-?\d+', body)]


class MossChargerContract(unittest.TestCase):
    def setUp(self):
        self.rust = rust_consts('moss_charger.rs')

    def test_the_frame_boxes_are_the_ones_the_recognizer_proves(self):
        for name, value in (('BIG', moss_charger.BIG), ('BIG_LOW', moss_charger.BIG_LOW), ('BIG_MID', moss_charger.BIG_MID),
                            ('BIG_HIGH', moss_charger.BIG_HIGH), ('STUN', moss_charger.STUN), ('RUN', moss_charger.RUN)):
            self.assertEqual(boxes(self.rust[name]), value, name)

    def test_the_clip_timings_are_the_ones_the_controller_counts(self):
        def ticks(frames, fps):
            return -(-frames * 60 // int(fps))
        clips = moss_charger.CLIPS
        self.assertEqual(int(self.rust['APPEAR_TICKS']), ticks(*clips['Appear'][:2]))
        self.assertEqual(int(self.rust['DISAPPEAR_TICKS']), ticks(*clips['Disappear'][:2]))
        self.assertEqual(int(self.rust['DISAPPEAR_TRIGGER_TICKS']), ticks(moss_charger.TRIGGERS['Disappear'][0], 12))
        self.assertEqual(int(self.rust['ESCAPE_TICKS']), ticks(*clips['Escape'][:2]))
        self.assertEqual(int(self.rust['ESCAPE_TRIGGER_TICKS']), ticks(moss_charger.TRIGGERS['Escape'][0], 12))
        self.assertEqual(int(self.rust['GET_UP_TICKS']), ticks(*clips['Get Up'][:2]))
        self.assertEqual(int(self.rust['SUBMERGE_CD_TICKS']),
                         round(moss_charger.ACTIONS[('Submerge CD', 'Wait', 0)]['time'] * 60))

    def test_the_cooked_clip_slots_follow_the_controller_clip_order(self):
        text = (ROOT / 'shared/hk-sim/src/moss_charger.rs').read_text()
        order = re.search(r'pub enum Clip \{([^}]*)\}', text)[1].split(',')
        names = [n.strip() for n in order if n.strip() and not n.strip().startswith('#')]
        self.assertEqual(len(names), len(moss_charger.CLIP_SLOTS))
        self.assertEqual([n.lower() for n in names], [slot.replace('_', '') for slot, _ in moss_charger.CLIP_SLOTS])


class FatFlyContract(unittest.TestCase):
    def setUp(self):
        self.rust = rust_consts('fat_fly.rs')

    def test_the_wait_antic_and_attack_timings_are_the_pinned_ones(self):
        a = fat_fly.ACTIONS
        self.assertEqual(int(self.rust['ANTIC_TICKS']), round(a[(fat_fly.ATTACK_FSM, 'Attack Antic', 'Wait', 0)]['time'] * 60))
        self.assertEqual(int(self.rust['COOLDOWN_TICKS']), round(a[(fat_fly.ATTACK_FSM, 'CD', 'Wait', 0)]['time'] * 60))
        wait = a[(fat_fly.ATTACK_FSM, 'Wait', 'WaitRandom', 0)]
        self.assertEqual(boxes(self.rust['WAIT_TICKS']), [round(wait['timeMin'] * 60), round(wait['timeMax'] * 60)])
        frames, fps, _, triggers = fat_fly.CLIPS['Attack']
        self.assertEqual(int(self.rust['ATTACK_TRIGGER_TICKS']), triggers[0] * 60 // int(fps))
        self.assertEqual(int(self.rust['ATTACK_TICKS']), frames * 60 // int(fps))

    def test_speeds_and_the_shot_diagonal(self):
        self.assertEqual(self.rust['SPEED'], '4 * ONE')
        self.assertEqual(fat_fly.VARIABLES[(fat_fly.BOUNCE_FSM, 'Speed')], 4.)
        shot = fat_fly.VARIABLES[(fat_fly.ATTACK_FSM, 'Shot Speed')]
        self.assertEqual(int(self.rust['SHOT_DIAGONAL']), round(shot * 0.5 ** 0.5 * 65536))
        self.assertEqual(int(self.rust['DECELERATION']),
                         round(fat_fly.ACTIONS[(fat_fly.ATTACK_FSM, 'Attack Antic', 'Decelerate', 0)]['deceleration'] * 65536))


if __name__ == '__main__':
    unittest.main()
