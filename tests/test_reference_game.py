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
        self.audio_rows = [dict.fromkeys(reference.AUDIO_HEADER, '')]
        self.audio_rows[0].update(sequence='0', queued_test_frame='-1', unity_frame='1',
                                  time='0.01666667', fixed_time='0', input_tick='0', input_updates='0',
                                  operation='PlayOneShot', callsite='System.Void Hero::Jump()@IL_0042',
                                  clip_or_snapshot='clip,"quoted"\nname', volume_scale='0.75', phase='request')
        self.write_audio()

    def write_audio(self):
        with (self.root / 'audio-calls.csv').open('w', newline='') as f:
            writer = csv.DictWriter(f, fieldnames=reference.AUDIO_HEADER)
            writer.writeheader()
            writer.writerows(self.audio_rows)

    def write_rows(self):
        with (self.root / 'state.csv').open('w') as f:
            writer = csv.DictWriter(f, fieldnames=self.rows[0].keys())
            writer.writeheader()
            writer.writerows(self.rows)

    def test_complete_input_and_gameplay_pass(self):
        result = reference.validate_run(self.root, 3)
        self.assertTrue(result['passed'])
        self.assertEqual(result['audio_calls']['requests'], 1)
        self.assertEqual(result['audio_calls']['observed_call_sites'], 1)
        self.assertEqual(result['audio_calls']['operations'], {'PlayOneShot': 1})

    def test_missing_and_malformed_audio_evidence_fails(self):
        path = self.root / 'audio-calls.csv'
        path.unlink()
        self.assertFalse(reference.validate_run(self.root, 3)['passed'])
        path.write_text('sequence,phase\n0,request\n')
        self.assertFalse(reference.validate_run(self.root, 3)['passed'])
        self.write_audio()
        with path.open('a') as f:
            f.write('1,0,10')
        self.assertFalse(reference.validate_run(self.root, 3)['passed'])

    def test_audio_sequence_phase_and_numeric_corruption_fail(self):
        original = dict(self.audio_rows[0])
        for key, value in [('sequence', '1'), ('phase', 'audible'), ('operation', 'Invented'),
                           ('callsite', ''), ('pitch', 'nan'), ('time', 'inf'),
                           ('queued_test_frame', '-2'), ('volume_scale', '')]:
            with self.subTest(key=key):
                self.audio_rows = [dict(original, **{key: value})]
                self.write_audio()
                self.assertFalse(reference.validate_run(self.root, 3)['passed'])

    def test_repeated_audio_in_same_frame_valid_but_backward_clock_fails(self):
        self.audio_rows.append(dict(self.audio_rows[0], sequence='1'))
        self.write_audio()
        self.assertTrue(reference.validate_run(self.root, 3)['passed'])
        self.audio_rows[1]['unity_frame'] = '0'
        self.write_audio()
        self.assertFalse(reference.validate_run(self.root, 3)['passed'])

    def test_audio_frame_to_fixed_clock_switch_in_same_frame_is_valid(self):
        # Actual Unity 6 sequence: SceneManager.Start then a grass trigger callback.
        first = dict(self.audio_rows[0], unity_frame='1473', time='24.55',
                     fixed_time='24.51999', input_tick='701', input_updates='699')
        fixed = dict(first, sequence='1', time='24.5399914', fixed_time='24.5399914')
        resumed = dict(first, sequence='2', fixed_time='24.5399914',
                       input_tick='702', input_updates='700')
        self.audio_rows = [first, fixed, resumed]
        self.write_audio()
        result = reference.validate_run(self.root, 3)
        self.assertTrue(result['passed'], result['errors'])
        self.assertEqual(result['audio_calls']['fixed_clock_context_switches'], 1)
        # Validation must retain native timestamps, not clamp or rewrite evidence.
        with (self.root / 'audio-calls.csv').open(newline='') as stream:
            self.assertEqual(list(csv.DictReader(stream))[1]['time'], '24.5399914')

    def test_audio_clock_switch_exception_does_not_hide_corruption(self):
        first = dict(self.audio_rows[0], unity_frame='1473', time='24.55',
                     fixed_time='24.51999', input_tick='701', input_updates='699')
        fixed = dict(first, sequence='1', time='24.5399914', fixed_time='24.5399914')
        for change in [dict(time='24.53'),  # Not actually the fixed clock.
                       dict(time='24.51999', fixed_time='24.51999'),  # No advance.
                       dict(time='24.50', fixed_time='24.50'),
                       dict(unity_frame='1474'), dict(unity_frame='1472'),
                       dict(input_tick='700'), dict(input_updates='698')]:
            with self.subTest(change=change):
                self.audio_rows = [first, dict(fixed, **change)]
                self.write_audio()
                self.assertFalse(reference.validate_run(self.root, 3)['passed'])

    def test_audio_logging_failure_invalidates_otherwise_complete_trace(self):
        with (self.root / 'unity.log').open('a') as f:
            f.write('HKReference AudioTrace logging failed: disk full\n')
        self.assertFalse(reference.validate_run(self.root, 3)['passed'])

    def test_header_only_audio_reports_zero_observed_coverage(self):
        self.audio_rows = []
        self.write_audio()
        result = reference.validate_run(self.root, 3)
        self.assertTrue(result['passed'])
        self.assertEqual(result['audio_calls']['requests'], 0)
        self.assertEqual(result['audio_calls']['observed_call_sites'], 0)

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
            reference.command(Namespace(name='live', text='particle-probe'))
            self.assertTrue((run / 'command.txt').read_text().rstrip().endswith(' particle-probe'))
            with self.assertRaises(ValueError):
                reference.command(Namespace(name='live', text='particle-probe extra'))
            (run / 'run.json').write_text('{}')
            with self.assertRaises(ValueError):
                reference.command(Namespace(name='live', text='quit'))


if __name__ == '__main__':
    unittest.main()
