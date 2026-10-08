"""Verify the actual cooperative presentation state machine without GPU mocks."""
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


class PresentationTests(unittest.TestCase):
    def test_native_busy_idle_schedules(self):
        with tempfile.TemporaryDirectory(prefix="hk-presentation-") as tmp:
            binary = Path(tmp) / "test"
            compile = subprocess.run(
                ["rustc", "--edition=2021", "--test", str(ROOT / "game/src/presentation.rs"), "-o", str(binary)],
                capture_output=True, text=True,
            )
            self.assertEqual(compile.returncode, 0, compile.stderr)
            result = subprocess.run([str(binary)], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
