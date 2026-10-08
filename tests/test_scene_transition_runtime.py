"""Source-free execution of the production scene-gate ownership handoff."""
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


class SceneTransitionRuntimeTests(unittest.TestCase):
    def test_native_scene_handoff_contract(self):
        with tempfile.TemporaryDirectory(prefix='hk-scene-transition-') as temporary:
            executable = Path(temporary) / 'scene-transition-runtime-tests'
            subprocess.run([
                'rustc', '--edition=2021', '-Awarnings', '--test',
                str(ROOT / 'tests/scene_transition_runtime.rs'), '-o', str(executable),
            ], cwd=ROOT, check=True)
            subprocess.run([str(executable)], cwd=ROOT, check=True)


if __name__ == '__main__':
    unittest.main()
