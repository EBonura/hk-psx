"""The generated-type-tree cache (host/source.py `Generator`) answers without the native library.

libTypeTreeGeneratorAPI crashed a cook worker in about one start in six under load, so a
node list already recorded for the same assemblies must never reach it.
"""
import json
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))

import source


class CacheTest(unittest.TestCase):
    def test_a_recorded_type_runs_no_native_code(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            managed = tmp / 'Managed'
            managed.mkdir()
            (managed / 'A.dll').write_bytes(b'not a real assembly')
            old = source.TYPETREE_CACHE
            source.TYPETREE_CACHE = tmp / 'cache.json'
            try:
                g = source.Generator('6000.0.61f1')
                g.load_local_dll_folder(str(managed))
                g.key()
                source.TYPETREE_CACHE.write_text(json.dumps({g.digest: {'A.dll|T': [['T', 'Base', 0, 0], ['int', 'm_X', 1, 1]]}}))
                g.recorded = None
                g.start = lambda: self.fail('the native library was started for a recorded type')
                nodes = g.get_nodes('A.dll', 'T')
                self.assertEqual([(n.m_Type, n.m_Name, n.m_Level, n.m_MetaFlag) for n in nodes],
                                 [('T', 'Base', 0, 0), ('int', 'm_X', 1, 1)])
                self.assertFalse(g.native)
            finally:
                source.TYPETREE_CACHE = old

    def test_a_changed_assembly_misses(self):
        with tempfile.TemporaryDirectory() as tmp:
            managed = Path(tmp) / 'Managed'
            managed.mkdir()
            (managed / 'A.dll').write_bytes(b'one')
            a = source.Generator('v')
            a.load_local_dll_folder(str(managed))
            a.key()
            (managed / 'A.dll').write_bytes(b'two')
            b = source.Generator('v')
            b.load_local_dll_folder(str(managed))
            b.key()
            self.assertNotEqual(a.digest, b.digest)


if __name__ == '__main__':
    unittest.main()
