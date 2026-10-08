"""host/cook_inputs.txt, which two readers parse differently on purpose.

The build (host/hk-build/main.rs) hashes every path in the file to decide
whether to run the cook at all. host/regions.py hashes only the paths above the
postpass divider to decide whether a single region can reuse its earlier pack.
Both mistakes this arrangement allows are quiet: a postpass file left out of the
file entirely means the whole-cook cache hits and the postpass never runs, and a
postpass file listed above the divider recooks every region for a change that
cannot alter one cooked byte.

Neither reader strips a trailing comment, so an inline one turns the whole line
into a path that does not exist.
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

    def test_no_path_sits_in_both_sections(self):
        above, below = sections()
        self.assertEqual(set(above) & set(below), set())
        self.assertEqual(len(above + below), len(set(above + below)))

    def test_the_postpass_code_is_listed_but_not_hashed_per_region(self):
        # These run after the per-region cook, over packs it already wrote.
        above, below = sections()
        for path in ('host/regions.py', 'host/npc_sources.py', 'host/npc_dialogue.py',
                     'host/npcs.py'):
            self.assertIn(path, below)
            self.assertNotIn(path, above)

    def test_regions_agrees_with_this_parse(self):
        # The real reader, not a second copy of the rule.
        import regions
        source = Path(regions.__file__).read_text()
        self.assertIn(repr(DIVIDER), source,
                      'host/regions.py must break on the same divider string')


if __name__ == '__main__':
    unittest.main()
