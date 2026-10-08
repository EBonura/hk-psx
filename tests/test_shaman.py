"""Compile the Snail Shaman state machine and run its own tests."""
import os, subprocess, tempfile, unittest
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]


class ShamanTests(unittest.TestCase):
    @unittest.skipUnless((ROOT / 'data/shaman.rs').is_file(), 'needs host/shaman.py output')
    def test_shaman_sequence(self):
        with tempfile.TemporaryDirectory(prefix='hk-shaman-test-') as temp:
            binary = Path(temp) / 'shaman-tests'
            compiled = subprocess.run(['rustc', '--edition=2021', '-Awarnings', '--test',
                                       str(ROOT / 'tests/shaman_runtime.rs'), '-o', str(binary)],
                                      capture_output=True, text=True,
                                      env=dict(os.environ, CARGO_MANIFEST_DIR=str(ROOT / 'game')))
            self.assertEqual(compiled.returncode, 0, compiled.stdout + compiled.stderr)
            run = subprocess.run([str(binary)], capture_output=True, text=True)
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
            self.assertIn('the_spell_is_met_summoned_taken_and_woken_from ... ok', run.stdout)


if __name__ == '__main__':
    unittest.main()
