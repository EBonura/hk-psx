"""The PS1 <-> original-game tape bridge must round-trip and refuse unmapped buttons."""
import csv
from pathlib import Path
import tempfile
import unittest

from tools import og_compare as og


class TapeBridgeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def test_round_trip_through_reference_csv(self):
        masks = [0, 0, 0x20, 0x20, 0x4020, 0x20, 0, 0x8000, 0]
        og.write_tape(self.root / 'a.pxtape', masks)
        self.assertEqual(og.read_tape(self.root / 'a.pxtape'), masks)
        og.main(['tape-to-csv', str(self.root / 'a.pxtape'), str(self.root / 'a.csv'), '--from-poll', '2'])
        rows = og.read_rows(self.root / 'a.csv')
        self.assertEqual(rows, [(0, 0x20), (2, 0x4020), (3, 0x20), (4, 0), (5, 0x8000), (6, 0)])
        og.main(['csv-to-tape', str(self.root / 'a.csv'), str(self.root / 'b.pxtape'),
                 '--frames', '7', '--lead-in', '2'])
        self.assertEqual(og.read_tape(self.root / 'b.pxtape'), masks)

    def test_unmapped_buttons_are_refused_unless_dropped(self):
        masks = [0, 0x0400, 0x0420]  # L1 (dash in the port) has no reference binding
        with self.assertRaises(ValueError):
            og.masks_to_rows(masks)
        self.assertEqual(og.masks_to_rows(masks, drop_unmapped=True), [(0, 0), (2, 0x20)])

    def test_tape_length_is_checked(self):
        (self.root / 'bad.pxtape').write_bytes(og.MAGIC + b'\x02\x00\x00\x00\x00\x00\x00\x00' + b'\x00' * 6)
        with self.assertRaises(ValueError):
            og.read_tape(self.root / 'bad.pxtape')

    def test_compare_reports_first_divergence(self):
        (self.root / 'game.map').write_text(
            '     VMA      LMA     Size Align Out     In      Symbol\n'
            '801ee5dc 801ee5dc        4     4                 HK_PLAYER_X\n'
            '801ee5e0 801ee5e0        4     4                 HK_PLAYER_Y\n')
        with (self.root / 'route.csv').open('w', newline='') as f:
            w = csv.writer(f)
            w.writerow(['route_tick', 'port1_polls', 'ram_801ee5dc', 'ram_801ee5e0'])
            for poll in range(6):
                x = int((10 + poll * 0.5) * og.ONE)
                y = (-2 * og.ONE) & 0xffffffff  # negative Q16 as the frontend prints it
                w.writerow([poll * 2, poll, x, y])
        with (self.root / 'state.csv').open('w', newline='') as f:
            w = csv.writer(f)
            w.writerow(['test_frame', 'x', 'y', 'scene', 'time_scale'])
            w.writerow([-1, 0, 0, 'Tutorial_01', 1])
            for frame in range(4):
                w.writerow([frame, 11 + frame * 0.5 + (0.2 if frame == 3 else 0), -2, 'Tutorial_01', 1])
        result = og.compare(og.ps1_track(self.root / 'route.csv', self.root / 'game.map'),
                            og.reference_track(self.root / 'state.csv'),
                            ps1_anchor_poll=2, reference_anchor_frame=0, frames=4, tolerance=0.05)
        self.assertEqual(result['first_over_tolerance'], 3)
        self.assertEqual(result['missing'], 0)
        self.assertAlmostEqual(result['rows'][0]['dy'], 0.0)

    def test_settle_inserts_idle_polls_before_the_window(self):
        masks = [0, 0, 0x4000, 0, 0, 0x8020, 0x20, 0]
        og.write_tape(self.root / 'a.pxtape', masks)
        og.main(['settle', str(self.root / 'a.pxtape'), str(self.root / 'b.pxtape'), '--at-poll', '4', '--idle', '3'])
        self.assertEqual(og.read_tape(self.root / 'b.pxtape'), masks[:4] + [0, 0, 0] + masks[4:])
        with self.assertRaises(SystemExit):  # never cut a held press in two
            og.main(['settle', str(self.root / 'a.pxtape'), str(self.root / 'c.pxtape'), '--at-poll', '5', '--idle', '3'])

    def test_compare_keys_on_the_consumed_sample_when_exported(self):
        # Two ticks can run in one VBlank, and the row's poll count is read
        # mid-frame: rows skip samples and their poll count jitters.
        (self.root / 'game.map').write_text(
            '801ee5dc 801ee5dc        4     4                 HK_PLAYER_X\n'
            '801ee5e0 801ee5e0        4     4                 HK_PLAYER_Y\n'
            '801ee5e4 801ee5e4        4     4                 HK_SIM_TICKS\n'
            '801ee5e8 801ee5e8        4     4                 HK_PLAYER_FACING\n'
            '801ee5ec 801ee5ec        4     4                 HK_SIM_PAD\n')
        tape = [0, 0, 0, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20]
        # (route tick, row poll count, ticks simulated): tick n consumed sample n - 1.
        rows = [(0, 1, 1), (1, 3, 2), (2, 3, 3), (3, 4, 4), (4, 6, 6), (5, 7, 7), (6, 7, 8), (7, 9, 9)]
        with (self.root / 'route.csv').open('w', newline='') as f:
            w = csv.writer(f)
            w.writerow(['route_tick', 'port1_polls', 'ram_801ee5dc', 'ram_801ee5e0', 'ram_801ee5e4', 'ram_801ee5e8', 'ram_801ee5ec'])
            for tick, polls, ticks in rows:
                sample = ticks - 1
                w.writerow([tick, polls, 10 * og.ONE + sample * og.ONE // 4, 0, ticks, 1, tape[sample]])
        with (self.root / 'state.csv').open('w', newline='') as f:
            w = csv.writer(f)
            w.writerow(['test_frame', 'x', 'y', 'scene', 'time_scale', 'facing_right'])
            for frame in range(7):
                w.writerow([frame, 10 + (frame + 2) * 0.25, 0, 'Tutorial_01', 1, 'True'])
        result = og.compare(og.ps1_track(self.root / 'route.csv', self.root / 'game.map'),
                            og.reference_track(self.root / 'state.csv'),
                            ps1_anchor_poll=2, reference_anchor_frame=0, frames=7, tolerance=0.01, tape=tape)
        self.assertEqual(result['keyed_on'], 'HK_SIM_TICKS')
        self.assertEqual(result['worst'], 0)
        self.assertEqual(result['missing'], 1)  # sample 4 shared a VBlank with sample 5
        self.assertEqual(result['facing_mismatches'], 0)
        self.assertEqual(result['pad_mismatches'], 0)

if __name__ == '__main__':
    unittest.main()
