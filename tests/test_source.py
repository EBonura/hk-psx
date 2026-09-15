"""Source external paths retain exact shipped-resource resolution."""
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]/'host'))
from source import source_path


class SourcePathTests(unittest.TestCase):
    def test_root_and_unity_library_resources_resolve_without_basename_guessing(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory).resolve()
            (root/'level0').write_bytes(b'level')
            (root/'Resources').mkdir()
            builtin=root/'Resources'/'unity default resources'
            builtin.write_bytes(b'meshes')
            self.assertEqual(source_path(root,'level0'),root/'level0')
            self.assertEqual(source_path(root,'Library/unity default resources'),builtin)
            self.assertEqual(source_path(root,'Resources\\unity default resources'),builtin)

    def test_missing_and_escaping_externals_fail_explicitly(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaises(FileNotFoundError):
                source_path(directory,'missing.assets')
            with self.assertRaisesRegex(ValueError,'Unsafe'):
                source_path(directory,'../outside.assets')


if __name__ == '__main__':
    unittest.main()
