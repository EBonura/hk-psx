"""Run the guest's conservative dynamic-quad culling boundary tests."""
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


class DynamicOcclusionTests(unittest.TestCase):
    def test_native_containment(self):
        with tempfile.TemporaryDirectory(prefix='hk-dynamic-occlusion-') as tmp:
            binary = Path(tmp) / 'test'
            result = subprocess.run(
                ['rustc', '--edition=2021', '--test',
                 str(ROOT / 'tests/dynamic_occlusion_runtime.rs'), '-o', str(binary)],
                capture_output=True, text=True,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            result = subprocess.run([str(binary)], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
