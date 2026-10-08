"""Check admission-time extrema against the original four-vertex projection."""
import subprocess, tempfile, unittest
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]
class SceneryBoundsTests(unittest.TestCase):
    def test_native_projection_parity(self):
        with tempfile.TemporaryDirectory(prefix="hk-bounds-") as tmp:
            out = Path(tmp) / "test"
            result = subprocess.run(["rustc", "--edition=2021", "--test", str(ROOT / "tests/scenery_bounds_runtime.rs"), "-o", str(out)], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            result = subprocess.run([str(out)], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
