"""docs/BUDGET.md's allocation table against the build that produced it.

The table sat at 113,636 bytes of headroom while the real figure fell to 73,876,
and in between it was quoted in three separate pieces of planning. A stale
budget is worse than no budget: nobody checks a number a document states
plainly, and work gets scoped against it.

Only the headline allocation table is pinned. The prose below it is a record of
what particular builds measured and is meant to age.
"""
import json, re, unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DOC = ROOT / 'docs/BUDGET.md'
BUILD = ROOT / '.hkpsx/build.json'

# Table row label to the `memory` key in .hkpsx/build.json it must equal.
ROWS = {
    'Linked code': 'code_bytes',
    'Code-to-data alignment': 'code_data_alignment_bytes',
    'Linked data, including menu, HUD and sound bank': 'data_bytes',
    'BSS, including shared scene arena and runtime pools': 'bss_bytes',
    'Total static span': 'static_span_bytes',
    'Linker stack exclusion': 'stack_reserved_bytes',
    'Unallocated gap before reserved stack': 'unallocated_before_stack_bytes',
}


def table():
    """Every `| label |number|` row of the doc, as {label: int}."""
    rows = {}
    for label, value in re.findall(r'^\|\s*([^|]+?)\s*\|\s*([\d,]+)\s*\|$', DOC.read_text(), re.M):
        rows[label] = int(value.replace(',', ''))
    return rows


@unittest.skipUnless(BUILD.is_file(), 'run a build first')
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
        self.assertEqual(self.rows['Unallocated gap before reserved stack'],
                         self.memory['unallocated_before_stack_bytes'])

    def test_the_static_end_address_matches(self):
        self.assertIn(self.memory['static_end'], DOC.read_text(),
                      'the quoted static end address is from an older build')


if __name__ == '__main__':
    unittest.main()
