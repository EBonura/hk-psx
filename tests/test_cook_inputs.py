"""host/cook_inputs.txt: the cook store key and the Rust steps' inputs.

What a region or the cook stage recooks is decided by the code and files it actually ran
(host/depcache.py), not by this list. The build (host/hk-build/main.rs) hashes every path for the cook
store key and the non-Python ones for the Rust steps. It does not strip a trailing comment, so an
inline one turns the whole line into a path that does not exist.
"""
import sys, unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))

DIVIDER = '# --- postpass only ---'


def sections():
    """The paths each reader sees: (above the divider, below it)."""
    above, found, below = [], False, []
    for line in (ROOT / 'host/cook_inputs.txt').read_text().splitlines():
        line = line.strip()
        if line == DIVIDER:
            found = True
            continue
        if not line or line.startswith('#'):
            continue
        (below if found else above).append(line)
    assert found, 'the postpass divider is what splits the two readers'
    return above, below


class CookInputsTests(unittest.TestCase):
    def test_every_listed_path_exists(self):
        # A path that does not exist makes the build's sha256_file fail, and a
        # trailing inline comment is the way that happens by accident.
        above, below = sections()
        for path in above + below:
            with self.subTest(path=path):
                self.assertTrue((ROOT / path).is_file(), path)

    def test_no_path_is_listed_twice(self):
        above, below = sections()
        self.assertEqual(len(above + below), len(set(above + below)))

    def test_the_cook_tracking_files_are_listed_for_the_store_key(self):
        above, below = sections()
        for path in ('host/depcache.py', 'host/region_units.py', 'host/region_plan.py', 'host/stagecache.py'):
            self.assertIn(path, above + below)


if __name__ == '__main__':
    unittest.main()
