"""Compile Cornifer's conversation state machine and run its own tests."""
import os, subprocess, tempfile, unittest
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]


class MapperTests(unittest.TestCase):
    @unittest.skipUnless((ROOT / 'data/cornifer.rs').is_file(), 'needs host/cornifer.py output')
    def test_cornifer_conversation(self):
        with tempfile.TemporaryDirectory(prefix='hk-mapper-test-') as temp:
            binary = Path(temp) / 'mapper-tests'
            compiled = subprocess.run(['rustc', '--edition=2021', '-Awarnings', '--test',
                                       str(ROOT / 'tests/mapper_runtime.rs'), '-o', str(binary)],
                                      capture_output=True, text=True,
                                      env=dict(os.environ, CARGO_MANIFEST_DIR=str(ROOT / 'game')))
            self.assertEqual(compiled.returncode, 0, compiled.stdout + compiled.stderr)
            run = subprocess.run([str(binary)], capture_output=True, text=True)
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
            self.assertIn('first_talk_meets_sells_and_mentions_iselda ... ok', run.stdout)


if __name__ == '__main__':
    unittest.main()
