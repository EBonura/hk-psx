"""The reserved CLUT block at x320, y482 is a live allocation map, so pin it.

Three separate documents said eight of the fourteen rows were free after a
commit spent four of them, including the comment in host/charms.py that the next
feature would read before claiming rows. Nothing computed the number, so nothing
noticed. Overrunning the block writes a palette over the dialogue font's, which
shows up as a recoloured pause screen and not as an error.

The per-view 416-slot CLUT budget is a different meter and does not apply here:
this block sits outside all three per-view banks, which is exactly why these
rows are worth reserving and worth counting.
"""
import re
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))
import rustsrc

# Each consumer: the row it starts at, how many rows nobody else may take, and
# the generated constant saying how many it actually fills. A consumer that
# reserves more than it uses is fine and says so; one that fills more than it
# reserved is the overrun this exists to catch.
def consumers():
    import ability_art
    import charms
    import props
    import shade
    return [
        ('Hollow Shade', shade.CLUT[1], generated('data/shade.rs', 'PALETTE_COUNT'),
         generated('data/shade.rs', 'PALETTE_COUNT')),
        ('ability clips', ability_art.CLUT[1], ability_art.CLUT_ROWS,
         generated_minus('data/ability-art.rs', 'PALETTE_COUNT', 1)),
        # The heal's flash palette is the last of the ability art's, on the one row left over.
        ('heal flash', ability_art.FLASH_CLUT_Y, 1, 1),
        ('charm icons', charms.ICON_CLUT[1], generated('data/charms.rs', 'ICON_PALETTE_COUNT'),
         generated('data/charms.rs', 'ICON_PALETTE_COUNT')),
        # game/src/render.rs FLASH_CLUT_Y: one fixed row, uploaded at boot.
        ('hit flash', render_row('FLASH_CLUT_Y'), 1, 1),
        ('props', props.CLUT[1], props.CLUT_ROWS, generated('data/props.rs', 'PALETTE_COUNT')),
    ]


def render_row(name):
    return rustsrc.const_int(ROOT / 'game/src/render.rs', name)


def generated_minus(path, name, k):
    value = generated(path, name)
    return None if value is None else value - k


def generated(path, name):
    """A usize constant out of a cooked Rust file, or None when it is not cooked."""
    file = ROOT / path
    if not file.is_file():
        return None
    try:
        return rustsrc.const_int(file, name)
    except KeyError:
        return None


class ClutRowTests(unittest.TestCase):
    def setUp(self):
        import shade
        self.base, self.rows = shade.CLUT[1], shade.CLUT_ROWS
        self.consumers = consumers()
        if any(reserved is None or used is None for _, _, reserved, used in self.consumers):
            self.skipTest('the CLUT palettes are not cooked; the cook writes them')

    def test_every_consumer_fits_the_block_and_no_two_overlap(self):
        taken = {}
        for name, start, reserved, used in self.consumers:
            self.assertLessEqual(used, reserved, f'{name} fills {used} of {reserved} reserved rows')
            for row in range(start, start + reserved):
                self.assertIn(row, range(self.base, self.base + self.rows),
                              f'{name} claims y{row}, outside the {self.rows}-row block at y{self.base}')
                self.assertNotIn(row, taken, f'{name} and {taken.get(row)} both claim y{row}')
                taken[row] = name

    def test_the_documented_free_rows_are_the_rows_that_are_actually_free(self):
        """docs/BUDGET.md names the free range, and a feature will believe it."""
        claimed = {row for _, start, reserved, _ in self.consumers
                   for row in range(start, start + reserved)}
        free = sorted(set(range(self.base, self.base + self.rows)) - claimed)
        doc = (ROOT / 'docs/BUDGET.md').read_text()
        if not free:
            self.assertIn('The reserved CLUT block is full', doc,
                          'every row is claimed, which docs/BUDGET.md should say')
            return
        # assertIn would print the whole document on failure, which buries the
        # one number the failure is about.
        span = f'y{free[0]}' if len(free) == 1 else f'y{free[0]}..{free[-1]}'
        self.assertTrue(span in doc,
                        f'{len(free)} rows are free, y{free[0]}..{free[-1]}, '
                        'and docs/BUDGET.md does not say so')


if __name__ == '__main__':
    unittest.main()
