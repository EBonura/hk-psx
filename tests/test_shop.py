"""The cooked shop table, and the purchase rules it feeds, against the source.

Three halves, in the shape `tests/test_charms.py` uses. The first checks the
generated table the guest links: fourteen rows, drawable names, wrapped text and
the price from the language sheet rather than the stale serialized field. The
second compiles the real `game/src/shop.rs` natively against the real
`game/src/charms.rs` and runs its transaction tests. The third compiles the same
module under `no_std`, because the guest has no standard library and the native
harness would not notice.

Nothing here touches the installed game: `host/shops.py` is what reads it, and
this runs against what that cooked.
"""
import json, os, re, subprocess, tempfile, unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TABLE = ROOT / 'data/shop.rs'
CATALOG = ROOT / '.hkpsx/shop-catalog.json'
COOKED = ROOT / '.hkpsx/shop-table.json'
# The charm module keeps its inventory in a process-wide `static mut`, and its
# own tests reset it between cases, so they only hold in declaration order.
# Running this binary in parallel reorders them; see the note in the report.
TEST_ARGS = ['--test-threads=1']


def shared_library():
    """The hk_sim rlib the guest modules compile against."""
    build = subprocess.run(['cargo', 'build', '--locked', '--manifest-path',
                            str(ROOT / 'shared/hk-sim/Cargo.toml'), '--message-format=json'],
                           check=True, capture_output=True, text=True)
    artifacts = [json.loads(line) for line in build.stdout.splitlines()]
    libraries = [Path(f) for a in artifacts if a.get('reason') == 'compiler-artifact'
                 and a['target']['name'] == 'hk_sim' for f in a['filenames'] if f.endswith('.rlib')]
    assert len(libraries) == 1, f'expected one hk_sim rlib, got {libraries}'
    return libraries[0]


@unittest.skipUnless(TABLE.is_file(), 'run host/shops.py first')
class CookedStockTests(unittest.TestCase):
    def setUp(self):
        self.text = TABLE.read_text()
        self.rows = re.findall(
            r'Item\{name:"([^"]*)",lines:&\[([^\]]*)\],cost:(\d+),', self.text)

    def test_every_row_is_drawable_and_priced(self):
        self.assertEqual(int(re.search(r'ITEM_COUNT:usize=(\d+);', self.text).group(1)), 14)
        self.assertEqual(len(self.rows), 14)
        for name, lines, cost in self.rows:
            self.assertTrue(name.isascii() and name.isprintable(), name)
            self.assertGreater(int(cost), 0, name)
            drawn = re.findall(r'"([^"]*)"', lines)
            self.assertTrue(drawn, f'{name} has no description')
            self.assertLessEqual(len(drawn), 6, f'{name} needs {len(drawn)} description lines')
            for line in drawn:
                # The guest font strip is ASCII 32..126 and nothing else.
                self.assertTrue(all(32 <= ord(c) <= 126 for c in line), f'{name}: {line!r}')

    def test_the_fuse_and_discount_constants_come_from_the_source(self):
        constants = dict(re.findall(r'pub const (\w+):\w+=(\d+);', self.text))
        self.assertEqual(constants['SHARDS_PER_MASK'], '4')
        self.assertEqual(constants['FRAGMENTS_PER_VESSEL'], '3')
        self.assertEqual(constants['SOUL_PER_VESSEL'], '33')
        self.assertEqual(constants['MASK_CAP'], '9')
        self.assertEqual(constants['STARTING_MASKS'], '5')
        # `ShopItemStats::OnEnable` discounts on Defender's Crest, charm 10.
        self.assertEqual(constants['DISCOUNT_CHARM'], '10')

    def test_the_two_stock_lists_are_the_sources_own(self):
        base = [int(v) for v in re.search(r'BASE_STOCK:&\[u8\]=&\[([0-9,]*)\];', self.text).group(1).split(',')]
        alt = [int(v) for v in re.search(r'ALTERNATE_STOCK:&\[u8\]=&\[([0-9,]*)\];', self.text).group(1).split(',')]
        self.assertEqual(len(base), 8)
        self.assertEqual(len(alt), 14)
        self.assertEqual(len(set(alt)), 14, 'a row cannot appear twice in one list')
        self.assertTrue(set(base) < set(alt), 'the key only ever adds rows')
        self.assertEqual(len(re.findall(r'Flag::Slot\(\d+\)',
                         re.search(r'ALTERNATE_WHEN:&\[Flag\]=&\[([^\]]*)\]', self.text).group(1))), 2)

    @unittest.skipUnless(CATALOG.is_file(), 'run host/shops.py first')
    def test_the_cooked_price_is_the_sheet_price_not_the_serialized_one(self):
        catalog = json.loads(CATALOG.read_text())
        items = {i['object']: i for lst in catalog['sly_shop']['stock']['lists'].values() for i in lst}
        self.assertEqual(len(items), 14)
        stale = [i for i in items.values() if not i['cost_is_serialized']]
        # Eleven of the fourteen. The earlier catalog note said nine, which is
        # the count without the two later mask shards that serialize the first
        # shard's 150 and sell for 800 and 1500.
        self.assertEqual(len(stale), 11, 'the serialized costs are stale editor data')
        costs = sorted(int(cost) for _, _, cost in self.rows)
        self.assertEqual(costs, sorted(i['cost'] for i in items.values()))
        # The serialized values would have shipped a different shop.
        self.assertNotEqual(costs, sorted(i['serialized_cost'] for i in items.values()))
        self.assertEqual(sum(costs), 9260)

    @unittest.skipUnless(COOKED.is_file(), 'run host/shops.py first')
    def test_the_purchase_rules_were_read_from_the_shipped_assembly(self):
        rules = json.loads(COOKED.read_text())['rules']
        self.assertAlmostEqual(rules['discount_factor'], 0.8, places=6)
        self.assertEqual(rules['discount_charm'], 10)
        for key in ('listing', 'affording', 'discount_rule', 'price_source'):
            self.assertGreater(len(rules[key]), 30, key)


class ShopRuntimeTests(unittest.TestCase):
    def test_glyph_budget_constant_matches_the_dialogue_module(self):
        real = re.search(r'const CAP:usize=(\d+);', (ROOT / 'game/src/dialogue.rs').read_text())
        stub = re.search(r'CAP: usize = (\d+);', (ROOT / 'tests/shop_runtime.rs').read_text())
        self.assertTrue(real and stub)
        self.assertEqual(real.group(1), stub.group(1))

    @unittest.skipUnless(TABLE.is_file(), 'run host/shops.py first')
    def test_the_purchase_transaction(self):
        library = shared_library()
        with tempfile.TemporaryDirectory(prefix='hk-shop-test-') as temp:
            binary = Path(temp) / 'shop-tests'
            env = dict(os.environ, CARGO_MANIFEST_DIR=str(ROOT / 'game'))
            compiled = subprocess.run(
                ['rustc', '--edition=2021', '-Awarnings', '--test', str(ROOT / 'tests/shop_runtime.rs'),
                 '--extern', 'hk_sim=' + str(library), '-L', 'dependency=' + str(library.parent / 'deps'),
                 '-o', str(binary)], env=env, capture_output=True, text=True)
            self.assertEqual(compiled.returncode, 0, compiled.stdout + compiled.stderr)
            run = subprocess.run([str(binary), *TEST_ARGS], capture_output=True, text=True)
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)

    @unittest.skipUnless(TABLE.is_file(), 'run host/shops.py first')
    def test_the_module_still_compiles_without_a_standard_library(self):
        """The guest is `no_std`, and the native harness would not notice."""
        library = shared_library()
        with tempfile.TemporaryDirectory(prefix='hk-shop-nostd-') as temp:
            source = Path(temp) / 'guest.rs'
            # The charm panel's drawing calls plus the shop counter's prompt
            # marker are the only thing either module needs from outside, so
            # they stand in with their real signatures rather than the module
            # being mocked out.
            source.write_text(
                '#![no_std]\n'
                'pub mod dialogue {\n'
                '    pub const CAP: usize = 416;\n'
                '    pub(crate) fn panel_text(_x: i16, _y: i16, _value: &str) {}\n'
                '    pub(crate) fn panel_number(_x: i16, _y: i16, _value: u32) {}\n'
                '    pub(crate) fn panel_width(_value: &str) -> i16 { 0 }\n'
                '    pub(crate) fn panel_frame(_x: i16, _y: i16, _w: u16, _h: u16) {}\n'
                '    pub(crate) fn panel_prompt(_c: (i32, i32), _p: [i32; 2], _l: &str) {}\n'
                '}\n'
                f'#[path = "{ROOT}/game/src/charms.rs"] pub mod charms;\n'
                f'#[path = "{ROOT}/game/src/shop.rs"] pub mod shop;\n')
            env = dict(os.environ, CARGO_MANIFEST_DIR=str(ROOT / 'game'))
            compiled = subprocess.run(
                ['rustc', '--edition=2021', '-Awarnings', '--crate-type=rlib', str(source),
                 '--extern', 'hk_sim=' + str(library), '-L', 'dependency=' + str(library.parent / 'deps'),
                 '-o', str(Path(temp) / 'guest.rlib')], env=env, capture_output=True, text=True)
            self.assertEqual(compiled.returncode, 0, compiled.stdout + compiled.stderr)


if __name__ == '__main__':
    unittest.main()
