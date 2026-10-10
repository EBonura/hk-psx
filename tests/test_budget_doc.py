"""docs/BUDGET.md's allocation table against the build that produced it.

The table sat at 113,636 bytes of headroom while the real figure fell to 73,876,
and in between it was quoted in three separate pieces of planning. A stale
budget is worse than no budget: nobody checks a number a document states
plainly, and work gets scoped against it.

Only the headline allocation table is pinned. The prose below it is a record of
what particular builds measured and is meant to age.

The figures are the ordinary build's, from build-normal.json. build.json is
whatever image is on the disc, and after `pgo` that is the profile-guided one,
whose code size depends on the profile; `pgo` writes its own report beside it.
"""
import json, os, re, unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
import sys
sys.path.insert(0, str(ROOT / 'host'))
import rustsrc
DOC = ROOT / 'docs/BUDGET.md'
BUILD = ROOT / '.hkpsx/build-normal.json'

# Table row label to the `memory` key in .hkpsx/build-normal.json it must equal.
ROWS = {
    'Linked code': 'code_bytes',
    'Code-to-data alignment': 'code_data_alignment_bytes',
    'Linked data, including menu and HUD': 'data_bytes',
    'BSS, including shared scene arena and runtime pools': 'bss_bytes',
    'Total static span': 'static_span_bytes',
    'Linker stack exclusion': 'stack_reserved_bytes',
    'Unallocated gap before reserved stack': 'unallocated_before_stack_bytes',
    'Room module pool, inside that gap': 'module_pool_bytes',
    'Free RAM below the module pool': 'free_below_pool_bytes',
}


def table():
    """Every `| label |number|` row of the doc, as {label: int}."""
    rows = {}
    for label, value in re.findall(r'^\|\s*([^|]+?)\s*\|\s*([\d,]+)\s*\|$', DOC.read_text(), re.M):
        rows[label] = int(value.replace(',', ''))
    return rows


# The build driver runs this suite before cooking, when build-normal.json is
# still the previous build's: any change that moves memory would fail there
# until the doc matched a build not yet made. It sets HK_PRE_BUILD for that
# run and checks this file on its own after the new report is written.
@unittest.skipUnless(BUILD.is_file(), 'run a build first')
@unittest.skipIf(os.environ.get('HK_PRE_BUILD'), 'checked against the new report after the build')
class BudgetDocTests(unittest.TestCase):
    def setUp(self):
        self.memory = json.loads(BUILD.read_text())['memory']
        self.rows = table()

    def test_every_pinned_row_matches_the_build(self):
        for label, key in ROWS.items():
            with self.subTest(row=label):
                self.assertIn(label, self.rows, f'{label} is no longer a row in docs/BUDGET.md')
                self.assertEqual(self.rows[label], self.memory[key],
                                 f'docs/BUDGET.md says {self.rows[label]:,} for {label}; '
                                 f'the build says {self.memory[key]:,}')

    def test_the_headroom_figure_is_the_one_people_quote(self):
        # Named separately because this is the number that gets planned against,
        # and it is the one that went stale.
        # Since rooms stream their code and data the pool sits inside the gap,
        # so the free figure is the gap less the pool.
        self.assertEqual(self.rows['Free RAM below the module pool'],
                         self.memory['free_below_pool_bytes'])

    def test_the_scene_arena_figure_in_the_prose_matches(self):
        # Not a table row, but quoted as a plain number in the prose and just as
        # capable of going stale: it read 354,032 while the live arena was
        # 421,128, found while correcting the headroom figure.
        import re
        arena = self.memory['scene_arena_bytes']
        text = DOC.read_text()
        quoted = re.search(r'scene arena occupies ([\d,]+) bytes', text)
        self.assertIsNotNone(quoted, 'the scene arena sentence moved; repoint this test')
        self.assertEqual(int(quoted.group(1).replace(',', '')), arena)

    def test_the_static_end_address_matches(self):
        self.assertIn(self.memory['static_end'], DOC.read_text(),
                      'the quoted static end address is from an older build')


class VramTableTests(unittest.TestCase):
    """The gameplay VRAM table, which has no build report behind it.

    Nothing regenerates this table, so the only thing keeping it honest is that
    it has to add up and agree with the cache constants. It moved when a static
    scenery page became the animation cache's second region, and the next such
    move should fail here rather than be noticed a package later.
    """
    VRAM_BYTES = 1024 * 512 * 2
    PAGE_BYTES = 256 * 256 // 2

    def setUp(self):
        self.text = DOC.read_text()
        section = self.text.split('| Gameplay VRAM reservation | Bytes |')[1]
        self.rows = dict(re.findall(r'^\|\s*([^|]+?)\s*\|\s*([\d,]+)\s*\|$',
                                    section.split('\n\n')[0], re.M))
        self.rows = {k: int(v.replace(',', '')) for k, v in self.rows.items()}

    def row(self, ending):
        matches = [k for k in self.rows if k.endswith(ending)]
        self.assertEqual(len(matches), 1, f'no single row ends with {ending!r}')
        return matches[0], self.rows[matches[0]]

    def test_the_reservations_add_up_to_the_total_and_the_spare(self):
        parts = {k: v for k, v in self.rows.items() if k not in ('Total', 'Unallocated within 1 MiB')}
        self.assertEqual(sum(parts.values()), self.rows['Total'])
        self.assertEqual(self.rows['Total'] + self.rows['Unallocated within 1 MiB'], self.VRAM_BYTES)

    def test_the_animation_row_is_the_cache_upload_cap(self):
        # MAX_UPLOAD_BYTES is the whole cache, so the two are the same number
        # stated in two places, which is the only reason the doc can be checked.
        cap = rustsrc.const_int(ROOT / 'shared/hk-cache/src/lib.rs', 'MAX_UPLOAD_BYTES')
        label, bytes_ = self.row('4bpp animation slots')
        self.assertEqual(bytes_, cap, f'{label} disagrees with hk_cache::MAX_UPLOAD_BYTES')
        self.assertEqual(bytes_ % (64 * 64 // 2), 0, 'not a whole number of 64x64 4bpp slots')

    def test_the_scenery_row_is_a_whole_number_of_pages_the_layout_can_address(self):
        label, bytes_ = self.row('4bpp scenery pages')
        self.assertEqual(bytes_ % self.PAGE_BYTES, 0, f'{label} is not a whole number of pages')
        residency = ROOT / 'shared/hk-cache/src/residency.rs'
        banks = rustsrc.const_int(residency, 'BANKS')
        per_bank = rustsrc.const_int(residency, 'PAGES')
        self.assertLessEqual(bytes_ // self.PAGE_BYTES, banks * per_bank,
                             'more pages than page_xy can address')


if __name__ == '__main__':
    unittest.main()
