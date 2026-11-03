"""host/depcache.py: a recorded unit is stale exactly when something it used changed.

The cache that skips a recook must never say "fresh" for a unit whose output would differ. Each test
runs a tiny module under the tracker, edits one thing, and asks whether the record notices. The
tests run against a scratch tree standing in for the checkout.
"""
import importlib
import os
import stat
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'host'))
import depcache  # noqa: E402


class Tree:
    """A scratch checkout with host/ and tools/, and the depcache module pointed at it."""

    def __init__(self):
        self.dir = tempfile.TemporaryDirectory()
        self.root = Path(self.dir.name).resolve()
        for name in ('host', 'tools', '.hkpsx', 'data'):
            (self.root / name).mkdir()
        self.saved = (depcache.ROOT, depcache.CODE_DIRS, depcache.SKIP_FILE_DIRS, depcache.SKIP_FILES, depcache.DEPS_DIR)
        depcache.ROOT = self.root
        depcache.CODE_DIRS = (self.root / 'host', self.root / 'tools')
        depcache.SKIP_FILE_DIRS = tuple(str(self.root / n) + os.sep for n in ('.venv', '.git', '.hkpsx/cookdeps'))
        depcache.SKIP_FILES = ()
        depcache.DEPS_DIR = self.root / '.hkpsx/cookdeps'
        depcache._FILE_CODE.clear()
        depcache._LOADED.clear()
        depcache._VERDICT.clear()
        sys.path.insert(0, str(self.root / 'host'))
        self.modules = []

    def write(self, rel, text):
        path = self.root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(textwrap.dedent(text))
        # The stat key the file cache uses must differ even when the size does not.
        stamp = path.stat().st_mtime_ns + 5_000_000_000
        os.utime(path, ns=(stamp, stamp))
        return path

    def record(self, entry, tracker_args=None):
        """Run `entry(module_name)` under a tracker, importing inside it; return the dependency record."""
        with depcache.Tracker(**(tracker_args or {})) as tracker:
            entry()
        return tracker.record()

    def forget(self, *names):
        for name in names:
            sys.modules.pop(name, None)
            self.modules.append(name)
        importlib.invalidate_caches()

    def close(self):
        for name in self.modules:
            sys.modules.pop(name, None)
        sys.path.remove(str(self.root / 'host'))
        depcache.ROOT, depcache.CODE_DIRS, depcache.SKIP_FILE_DIRS, depcache.SKIP_FILES, depcache.DEPS_DIR = self.saved
        depcache._FILE_CODE.clear()
        self.dir.cleanup()


class DepcacheTests(unittest.TestCase):
    def setUp(self):
        self.tree = Tree()
        self.addCleanup(self.tree.close)

    def stale(self, record):
        depcache._FILE_CODE.clear()
        return depcache.stale(record, None)

    def two_function_module(self):
        self.tree.write('host/unit_mod.py', """
            LIMIT = 3

            def used(x):
                return x + LIMIT

            def unused(x):
                return x * 2
        """)
        self.tree.forget('unit_mod')

        def entry():
            import unit_mod
            unit_mod.used(1)
        return self.tree.record(entry)

    def test_an_untouched_unit_is_fresh(self):
        self.assertEqual(self.stale(self.two_function_module()), [])

    def test_editing_a_function_the_unit_never_ran_keeps_it_fresh(self):
        record = self.two_function_module()
        self.tree.write('host/unit_mod.py', """
            LIMIT = 3

            def used(x):
                return x + LIMIT

            def unused(x):
                return x * 2 + 1000
        """)
        self.assertEqual(self.stale(record), [])

    def test_editing_a_function_the_unit_ran_makes_it_stale(self):
        record = self.two_function_module()
        self.tree.write('host/unit_mod.py', """
            LIMIT = 3

            def used(x):
                return x + LIMIT + 1

            def unused(x):
                return x * 2
        """)
        self.assertTrue(any('function host/unit_mod.py:used' in r for r in self.stale(record)))

    def test_a_comment_or_blank_line_is_not_an_edit(self):
        record = self.two_function_module()
        self.tree.write('host/unit_mod.py', """
            # A new comment, and the lines move.

            LIMIT = 3


            def used(x):
                # explained
                return x + LIMIT

            def unused(x):
                return x * 2
        """)
        self.assertEqual(self.stale(record), [])

    def test_editing_a_module_level_constant_makes_it_stale(self):
        record = self.two_function_module()
        self.tree.write('host/unit_mod.py', """
            LIMIT = 4

            def used(x):
                return x + LIMIT

            def unused(x):
                return x * 2
        """)
        self.assertTrue(any('module level of host/unit_mod.py' in r for r in self.stale(record)))

    def test_a_function_that_module_level_code_ran_at_import_is_a_dependency(self):
        self.tree.write('host/table_mod.py', """
            def build():
                return [1, 2, 3]

            TABLE = build()
        """)
        self.tree.forget('table_mod')
        record = self.tree.record(lambda: importlib.import_module('table_mod'))
        self.assertEqual(self.stale(record), [])
        self.tree.write('host/table_mod.py', """
            def build():
                return [1, 2, 3, 4]

            TABLE = build()
        """)
        self.assertTrue(any('function host/table_mod.py:build' in r for r in self.stale(record)))

    def test_a_result_cached_in_process_still_shows_the_code_that_made_it(self):
        self.tree.write('host/memo_mod.py', """
            _MEMO = {}

            def table():
                if 'x' not in _MEMO:
                    _MEMO['x'] = compute()
                return _MEMO['x']

            def compute():
                return 41
        """)
        self.tree.forget('memo_mod')

        def entry():
            import memo_mod
            memo_mod.table()
            memo_mod.table()
        record = self.tree.record(entry)
        self.tree.write('host/memo_mod.py', """
            _MEMO = {}

            def table():
                if 'x' not in _MEMO:
                    _MEMO['x'] = compute()
                return _MEMO['x']

            def compute():
                return 42
        """)
        self.assertTrue(any('compute' in r for r in self.stale(record)))

    def test_a_file_read_through_open_is_a_dependency(self):
        self.tree.write('data/numbers.txt', '1\n')
        self.tree.write('host/reader.py', """
            from pathlib import Path

            def read():
                with open(Path(__file__).resolve().parents[1] / 'data/numbers.txt') as stream:
                    return stream.read()
        """)
        self.tree.forget('reader')

        def entry():
            import reader
            reader.read()
        record = self.tree.record(entry)
        self.assertEqual(self.stale(record), [])
        self.tree.write('data/numbers.txt', '2\n')
        self.assertEqual(self.stale(record), ['file data/numbers.txt'])

    def test_a_file_the_unit_wrote_is_an_output_not_an_input(self):
        self.tree.write('host/writer.py', """
            from pathlib import Path

            def write():
                path = Path(__file__).resolve().parents[1] / 'data/made.txt'
                path.write_text('made')
                return path.read_text()
        """)
        self.tree.forget('writer')

        def entry():
            import writer
            writer.write()
        record = self.tree.record(entry)
        self.tree.write('data/made.txt', 'edited by hand')
        self.assertEqual(self.stale(record), [])

    def test_a_file_replaced_by_rename_is_an_output_too(self):
        self.tree.write('data/report.json', '{"v": 1}')
        self.tree.write('host/updater.py', """
            from pathlib import Path

            def update():
                path = Path(__file__).resolve().parents[1] / 'data/report.json'
                text = path.read_text()
                temp = path.with_suffix('.tmp')
                temp.write_text(text + ' ')
                temp.replace(path)
        """)
        self.tree.forget('updater')

        def entry():
            import updater
            updater.update()
        record = self.tree.record(entry)
        self.assertNotIn('data/report.json', record['files'])
        self.assertEqual(self.stale(record), [])

    def test_a_deleted_or_renamed_module_makes_it_stale(self):
        record = self.two_function_module()
        path = self.tree.root / 'host/unit_mod.py'
        path.rename(self.tree.root / 'host/unit_mod_renamed.py')
        self.assertTrue(self.stale(record))
        (self.tree.root / 'host/unit_mod_renamed.py').rename(path)
        self.assertEqual(self.stale(record), [])
        path.unlink()
        self.assertTrue(self.stale(record))

    def test_a_listed_directory_is_a_dependency(self):
        (self.tree.root / 'data/listed').mkdir()
        self.tree.write('data/listed/a.txt', 'a')
        self.tree.write('host/lister.py', """
            import os
            from pathlib import Path

            def names():
                return sorted(os.listdir(Path(__file__).resolve().parents[1] / 'data/listed'))
        """)
        self.tree.forget('lister')

        def entry():
            import lister
            lister.names()
        record = self.tree.record(entry)
        self.assertEqual(self.stale(record), [])
        self.tree.write('data/listed/b.txt', 'b')
        self.assertEqual(self.stale(record), ['directory data/listed'])

    def test_a_program_the_unit_ran_is_a_dependency(self):
        tool = self.tree.root / 'data/tool.sh'
        tool.write_text('#!/bin/sh\necho one\n')
        tool.chmod(tool.stat().st_mode | stat.S_IXUSR)
        self.tree.write('host/runs_tool.py', """
            import subprocess
            from pathlib import Path

            def run():
                tool = Path(__file__).resolve().parents[1] / 'data/tool.sh'
                return subprocess.run([str(tool)], capture_output=True).stdout
        """)
        self.tree.forget('runs_tool')

        def entry():
            import runs_tool
            runs_tool.run()
        record = self.tree.record(entry)
        self.assertEqual(self.stale(record), [])
        tool.write_text('#!/bin/sh\necho two\n')
        self.assertEqual(self.stale(record), ['program data/tool.sh'])

    def test_something_the_tracker_cannot_see_is_never_fresh(self):
        self.tree.write('host/shells.py', """
            import os

            def run():
                os.system('true')
        """)
        self.tree.forget('shells')

        def entry():
            import shells
            shells.run()
        record = self.tree.record(entry)
        self.assertTrue(self.stale(record))

    def test_an_hk_switch_is_part_of_the_stamp_but_a_path_variable_is_not(self):
        record = self.two_function_module()
        os.environ['HK_COOK_STORE'] = '/somewhere/else'
        self.addCleanup(os.environ.pop, 'HK_COOK_STORE', None)
        self.assertEqual(self.stale(record), [])
        os.environ['HK_SOME_SWITCH'] = 'off'
        self.addCleanup(os.environ.pop, 'HK_SOME_SWITCH', None)
        self.assertTrue(self.stale(record))

    def test_a_retail_file_is_judged_by_the_manifest_hash(self):
        retail_dir = self.tree.root / 'retail'
        retail_dir.mkdir()
        (retail_dir / 'level0').write_bytes(b'scene')
        self.tree.write('host/retail_reader.py', """
            def read(path):
                with open(path, 'rb') as stream:
                    return stream.read()
        """)
        self.tree.forget('retail_reader')
        retail = depcache.Retail(str(retail_dir), {'level0': 'aaa'})

        def entry():
            import retail_reader
            retail_reader.read(str(retail_dir / 'level0'))
        record = self.tree.record(entry, {'retail': retail})
        self.assertEqual(record['retail'], {'level0': 'aaa'})
        self.assertEqual(depcache.stale(record, retail), [])
        self.assertEqual(depcache.stale(record, depcache.Retail(str(retail_dir), {'level0': 'bbb'})),
                         ['retail file level0'])
        self.assertEqual(depcache.stale(record, None), ['retail source not checked by the build driver'])

    def test_the_depcache_version_is_part_of_the_stamp(self):
        record = self.two_function_module()
        saved = depcache.VERSION
        self.addCleanup(setattr, depcache, 'VERSION', saved)
        depcache.VERSION = saved + 1
        self.assertTrue(self.stale(record))

    def test_records_are_stored_once_and_found_by_name(self):
        record = self.two_function_module()
        name = depcache.save(record)
        self.assertEqual(depcache.save(record), name)
        self.assertEqual(depcache.load(name), record)
        self.assertEqual(depcache.stale_by_name(name, None), [])
        self.assertEqual(depcache.stale_by_name('0' * 64, None), ['dependency record missing'])
        self.assertEqual(depcache.collect_garbage(set()), 1)
        self.assertEqual(depcache.collect_garbage(set()), 0)


if __name__ == '__main__':
    unittest.main()
