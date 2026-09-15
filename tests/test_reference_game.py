"""Headless reference reports must prove gameplay, not merely a clean exit."""
import csv
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from argparse import Namespace

from tools import reference_game as reference


class ReferenceValidationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / 'unity.log').write_text('NullGfxDevice\nHK_REFERENCE_STOP code=0 reason=completed input frames\n')
        (self.root / 'driver.log').write_text('READY\nSTOP completed input frames code=0\n')
        self.rows = [dict(test_frame=i, scene='Tutorial_01', input_attached='True',
                          x=34.5 + i, y=11.4, buttons=mask) for i, mask in enumerate([0, 32, 0])]
        (self.root / 'input.csv').write_text('test_frame,buttons\n0,0\n1,0x20\n2,0\n')
        self.write_rows()
        (self.root / 'input-events.csv').write_text('test_frame,buttons\n0,0\n1,32\n2,0\n')

    def write_rows(self):
        with (self.root / 'state.csv').open('w') as f:
            writer = csv.DictWriter(f, fieldnames=self.rows[0].keys())
            writer.writeheader()
            writer.writerows(self.rows)

    def test_complete_input_and_gameplay_pass(self):
        self.assertTrue(reference.validate_run(self.root, 3)['passed'])

    def test_clean_exit_without_gameplay_fails(self):
        (self.root / 'state.csv').unlink()
        self.assertFalse(reference.validate_run(self.root, 3)['passed'])

    def test_skipped_frame_fails(self):
        self.rows.pop(1)
        self.write_rows()
        self.assertFalse(reference.validate_run(self.root, 3)['passed'])

    def test_wrong_consumed_button_fails(self):
        self.rows[1]['buttons'] = 0
        self.write_rows()
        result = reference.validate_run(self.root, 3)
        self.assertFalse(result['passed'])
        self.assertTrue(any('controller differs' in e for e in result['errors']))

    def test_gameplay_exception_fails_shutdown_exception_reported(self):
        log = self.root / 'unity.log'
        content = log.read_text()
        log.write_text('NullReferenceException: gameplay\n' + content)
        self.assertFalse(reference.validate_run(self.root, 3)['passed'])
        log.write_text(content + 'NullReferenceException: teardown\n')
        result = reference.validate_run(self.root, 3)
        self.assertTrue(result['passed'])
        self.assertEqual(result['shutdown_exceptions'], ['NullReferenceException: teardown'])

    def test_extra_native_input_tick_is_also_checked(self):
        with (self.root / 'input-events.csv').open('a') as f:
            f.write('1,0\n')
        self.assertFalse(reference.validate_run(self.root, 3)['passed'])

    def test_leading_zero_decimal_matches_managed_parser(self):
        (self.root / 'input.csv').write_text('test_frame,buttons\n0,0\n1,00032\n2,0\n')
        self.assertTrue(reference.validate_run(self.root, 3)['passed'])

    def test_truncated_crash_log_is_reported_as_failure(self):
        with (self.root / 'state.csv').open('a') as f:
            f.write('3,Tutorial_01')
        self.assertFalse(reference.validate_run(self.root, 3)['passed'])

    def test_command_cannot_escape_run_or_override_tape(self):
        work = self.root / 'work'
        run = work / 'runs' / 'live'
        run.mkdir(parents=True)
        with patch.object(reference, 'WORK', work):
            with self.assertRaises(ValueError):
                reference.command(Namespace(name='../escape', text='quit'))
            (run / 'input.csv').write_text('test_frame,buttons\n')
            with self.assertRaises(ValueError):
                reference.command(Namespace(name='live', text='buttons 0x20'))
            with self.assertRaises(ValueError):
                reference.command(Namespace(name='live', text='buttons 0x100'))
            (run / 'run.json').write_text('{}')
            with self.assertRaises(ValueError):
                reference.command(Namespace(name='live', text='quit'))


if __name__ == '__main__':
    unittest.main()
