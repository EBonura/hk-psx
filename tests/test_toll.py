"""Compile the Toll Machines' state machine and run its own tests."""
import os, subprocess, tempfile, unittest
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]


class TollTests(unittest.TestCase):
    @unittest.skipUnless((ROOT / 'data/toll.rs').is_file(), 'needs host/tolls.py output')
    def test_toll_box(self):
        with tempfile.TemporaryDirectory(prefix='hk-toll-test-') as temp:
            binary = Path(temp) / 'toll-tests'
            compiled = subprocess.run(['rustc', '--edition=2021', '-Awarnings', '--test',
                                       str(ROOT / 'tests/toll_runtime.rs'), '-o', str(binary)],
                                      capture_output=True, text=True,
                                      env=dict(os.environ, CARGO_MANIFEST_DIR=str(ROOT / 'game')))
            self.assertEqual(compiled.returncode, 0, compiled.stdout + compiled.stderr)
            run = subprocess.run([str(binary)], capture_output=True, text=True)
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
            self.assertIn('a_short_wallet_can_only_say_no_and_nothing_is_taken ... ok', run.stdout)


if __name__ == '__main__':
    unittest.main()
