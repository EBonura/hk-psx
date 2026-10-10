"""The cooked charm catalogue, and the guest rules it feeds, against the source.

Two halves. The first checks the generated table the guest links: every charm
has a drawable name, a positive notch cost, wrapped description lines and either
an effect or a recorded reason it has none. The second compiles the real
`game/src/charms.rs` natively and runs its own notch-board and overcharm tests,
the way `tests/test_shade.py` compiles the Shade runtime.

Neither half touches the installed game: `host/charms.py` is what reads it, and
this runs against what that cooked.
"""
import json, os, re, subprocess, tempfile, unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TABLE = ROOT / 'data/charms.rs'
ICONS = ROOT / 'data/charm-icons.hk'
REPORT = ROOT / '.hkpsx/charm-catalog.json'
BUILD = ROOT / '.hkpsx/build.json'


def cooked():
    return TABLE.read_text()


@unittest.skipUnless(TABLE.is_file(), 'run host/charms.py first')
class CookedCatalogueTests(unittest.TestCase):
    def test_every_charm_is_drawable_and_priced(self):
        text = cooked()
        self.assertEqual(int(re.search(r'CHARM_COUNT:usize=(\d+);', text).group(1)), 40)
        self.assertEqual(int(re.search(r'STARTING_NOTCHES:u8=(\d+);', text).group(1)), 3)
        self.assertEqual(int(re.search(r'OVERCHARM_BREAK_ATTEMPT:u8=(\d+);', text).group(1)), 5)
        rows = re.findall(r'Charm\{name:"([^"]*)",cost:(\d+),lines:&\[([^\]]*)\]', text)
        self.assertEqual(len(rows), 40)
        for name, cost, lines in rows:
            self.assertTrue(name.isascii() and name.isprintable(), name)
            self.assertGreaterEqual(int(cost), 1, name)
            drawn = re.findall(r'"([^"]*)"', lines)
            self.assertTrue(drawn, f'{name} has no description')
            for line in drawn:
                # The guest font strip is ASCII 32..126 and nothing else.
                self.assertTrue(all(32 <= ord(c) <= 126 for c in line), f'{name}: {line!r}')
            self.assertLessEqual(len(drawn), 6, f'{name} needs {len(drawn)} description lines')

    @unittest.skipUnless(REPORT.is_file(), 'run host/charms.py first')
    def test_every_effectless_charm_names_the_missing_system(self):
        report = json.loads(REPORT.read_text())
        implemented = set(report['implemented'])
        unimplemented = {int(k): v for k, v in report['unimplemented'].items()}
        self.assertEqual(implemented | set(unimplemented), set(range(1, 41)))
        self.assertFalse(implemented & set(unimplemented))
        for charm, reason in unimplemented.items():
            self.assertTrue(reason and len(reason) >= 15, f'charm {charm}: {reason!r}')
        # The effects the guest tests assert on, straight from the source.
        effects = report['effects']
        self.assertEqual(effects['soul_per_hit_base'], 11)
        self.assertEqual(effects['soul_per_hit_charms'], {'20': 3, '21': 8})
        self.assertEqual(effects['fragile_heart_masks'], 2)
        self.assertEqual(effects['nail_multipliers']['fragile_strength'], 1.5)
        self.assertEqual(effects['stalwart_shell_ticks']['invulnerable_ticks'], 106)
        self.assertEqual(effects['stalwart_shell_ticks']['recoil_ticks'], 5)
        self.assertEqual(effects['grubsong']['alone'], 15)

    @unittest.skipUnless(REPORT.is_file(), 'run host/charms.py first')
    def test_icons_were_measured_before_being_refused(self):
        icons = json.loads(REPORT.read_text())['icons']
        # The pause screen is reachable from every view, so the number that
        # applies is the tightest view in the world, not any one scene's.
        self.assertEqual(icons['texture_budget'], 416)
        self.assertEqual(len(icons['source_sprite_pixels']), 40)
        self.assertGreater(icons['animation_slots'], 0)
        # The slot half of the refusal is derived, not asserted, because it
        # stopped being true: reclaiming an unused scenery page took the cache
        # from 8 slots to 24, and the recorded verdict went on interpolating the
        # new number into the old argument. The atlas route above is refused on
        # a reason that cannot move; this one is a live comparison.
        self.assertEqual(icons['shade_route_slots_needed'], icons['rows_on_screen'] + 4)
        self.assertEqual(icons['shade_route_fits'],
                         icons['shade_route_slots_needed'] <= icons['animation_slots'])
        # The atlas route needs somewhere resident to put the texels as well as
        # a CLUT slot, and that half does not turn on a CLUT count at all.
        self.assertGreater(min(icons['resident_vram_halfwords'].values()),
                           icons['spare_halfwords'])

    @unittest.skipUnless(REPORT.is_file(), 'run host/charms.py first')
    def test_the_icon_size_is_the_largest_the_screen_can_seat(self):
        icons = json.loads(REPORT.read_text())['icons']
        layouts = icons['panel_layouts']
        seatable = [int(side) for side, layout in layouts.items() if layout['fits']]
        self.assertTrue(seatable, icons['verdict'])
        self.assertEqual(icons['icon_px'], max(seatable))
        for side, layout in layouts.items():
            self.assertEqual(layout['fits'], layout['scanlines_needed'] <= icons['panel_scanlines'])
        # What must not change is the reason: the screen refuses the larger
        # sizes, and a trimmed row count or a shorter description is not allowed
        # to buy a bigger icon.
        self.assertEqual(icons['rows_on_screen'], 6)
        # The size was chosen when every offered size fit linked RAM, so the
        # screen was the only thing refusing them. That is no longer true:
        # headroom fell from 51,348 to 11,556 over one day of boss work, and
        # 32x32's 20,480 bytes stopped fitting. The chosen size still does fit,
        # which is the part that has to hold, and the two constraints now agree
        # rather than one doing all the work.
        if BUILD.is_file():
            headroom = json.loads(BUILD.read_text())['memory']['unallocated_before_stack_bytes']
            chosen = icons['shade_route_linked_bytes'][f"{icons['icon_px']}x{icons['icon_px']}"]
            self.assertLess(chosen, headroom,
                            'the cooked icon set no longer fits linked RAM; it is not a screen '
                            'decision any more and the catalogue should say so')

    @unittest.skipUnless(ICONS.is_file(), 'run host/charms.py first')
    def test_the_cooked_icons_are_one_square_frame_each(self):
        text = cooked()
        def const(name, pattern=r'(\d+)'):
            return int(re.search(rf'{name}:\w+={pattern};', text).group(1))
        side = const('ICON_PX')
        palettes = const('ICON_PALETTE_COUNT')
        # A 4bpp row rounds the width up to four pixels, and the whole frame has
        # to reach VRAM in one 64x64 animation slot upload.
        self.assertEqual(const('ICON_BYTES'), (side + 3) // 4 * 2 * side)
        self.assertLessEqual(const('ICON_BYTES'), 2048)
        self.assertEqual(const('ICON_PALETTE_BYTES'), palettes * 32)
        self.assertEqual(ICONS.stat().st_size, palettes * 32 + 40 * const('ICON_BYTES'))
        table = re.search(r'ICON_PALETTE:\[u8;CHARM_COUNT\]=\[([0-9,]+)\];', text).group(1)
        rows = [int(v) for v in table.split(',')]
        self.assertEqual(len(rows), 40)
        self.assertTrue(all(0 <= r < palettes for r in rows))
        # Every palette is used, or a resident CLUT row is being held for
        # nothing; the rows come out of the block the Shade reserved.
        self.assertEqual(sorted(set(rows)), list(range(palettes)))


class CharmRuntimeTests(unittest.TestCase):
    def test_glyph_budget_constant_matches_the_dialogue_module(self):
        real = re.search(r'const CAP\s*:\s*usize\s*=\s*(\d+);', (ROOT / 'game/src/dialogue.rs').read_text())
        stub = re.search(r'CAP: usize = (\d+);', (ROOT / 'tests/charms_runtime.rs').read_text())
        self.assertTrue(real and stub)
        self.assertEqual(real.group(1), stub.group(1))

    @unittest.skipUnless(TABLE.is_file(), 'run host/charms.py first')
    def test_notch_board_overcharm_and_effects(self):
        build = subprocess.run(['cargo', 'build', '--locked', '--manifest-path',
                                str(ROOT / 'shared/hk-sim/Cargo.toml'), '--message-format=json'],
                               check=True, capture_output=True, text=True)
        artifacts = [json.loads(line) for line in build.stdout.splitlines()]
        libraries = [Path(f) for a in artifacts if a.get('reason') == 'compiler-artifact'
                     and a['target']['name'] == 'hk_sim' for f in a['filenames'] if f.endswith('.rlib')]
        self.assertEqual(len(libraries), 1)
        library = libraries[0]
        with tempfile.TemporaryDirectory(prefix='hk-charms-test-') as temp:
            binary = Path(temp) / 'charm-tests'
            env = dict(os.environ, CARGO_MANIFEST_DIR=str(ROOT / 'game'))
            compiled = subprocess.run(
                ['rustc', '--edition=2021', '-Awarnings', '--test', str(ROOT / 'tests/charms_runtime.rs'),
                 '--extern', 'hk_sim=' + str(library), '-L', 'dependency=' + str(library.parent / 'deps'),
                 '-o', str(binary)], env=env, capture_output=True, text=True)
            self.assertEqual(compiled.returncode, 0, compiled.stdout + compiled.stderr)
            # Serialised, because the charm tests share one process-wide
            # `static mut STATE`: the module owns the live board the way the
            # guest does. Ordering is no longer a hazard (each case that
            # touches it resets first) but parallel access to a `static mut`
            # is a race whatever the order. tests/test_shop.py does the same.
            run = subprocess.run([str(binary), '--test-threads=1'], capture_output=True, text=True)
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)


if __name__ == '__main__':
    unittest.main()
