"""Compile the Grubfather's state machine and run its own tests."""
import os, subprocess, tempfile, unittest
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]


class GrubfatherTests(unittest.TestCase):
    @unittest.skipUnless((ROOT / 'data/grubfather.rs').is_file() and (ROOT / 'data/pickups.rs').is_file(),
                         'needs host/grubfather.py and host/pickups.py output')
    def test_king_control(self):
        with tempfile.TemporaryDirectory(prefix='hk-grubfather-test-') as temp:
            binary = Path(temp) / 'grubfather-tests'
            compiled = subprocess.run(['rustc', '--edition=2021', '-Awarnings', '--test',
                                       str(ROOT / 'tests/grubfather_runtime.rs'), '-o', str(binary)],
                                      capture_output=True, text=True,
                                      env=dict(os.environ, CARGO_MANIFEST_DIR=str(ROOT / 'game')))
            self.assertEqual(compiled.returncode, 0, compiled.stdout + compiled.stderr)
            run = subprocess.run([str(binary)], capture_output=True, text=True)
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
            for name in ('he_beckons_until_the_knight_comes_then_pays_each_grub_in_turn',
                         'the_last_reward_sets_the_final_flag_and_stops', 'the_rewards_are_the_originals'):
                self.assertIn(f'{name} ... ok', run.stdout)


if __name__ == '__main__':
    unittest.main()
