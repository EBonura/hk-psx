import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))
sys.dont_write_bytecode = True
import code_modules as cm  # noqa: E402
from stack_budget import prepare_linker  # noqa: E402


class LayerTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.script = cm.layer(prepare_linker(ROOT, Path(self.tmp.name)).read_text())

    def tearDown(self):
        self.tmp.cleanup()

    def test_modules_precede_text_so_their_rules_match_first(self):
        for name in cm.MODULES:
            self.assertLess(self.script.index(f'.mod_{name} '), self.script.index('    .text : {'))

    def test_each_module_has_its_own_link_address(self):
        for k, name in enumerate(cm.MODULES):
            self.assertIn(f'.mod_{name} {cm.link_address(k):#x} :', self.script)

    def test_bios_loads_text_and_data_only_and_the_pool_is_carved(self):
        self.assertIn('LONG(__image_end - __text_start);', self.script)
        self.assertIn('LENGTH = EXE_HEAD_BYTES + STACK_INIT - LOAD_ADDR - STACK_RESERVE - POOL_BYTES', self.script)
        self.assertIn('__heap_end   = POOL_BASE;', self.script)

    def test_code_rules_come_before_tables(self):
        body = self.script[self.script.index('.mod_false_knight '):]
        body = body[:body.index('}')]
        self.assertLess(body.rindex('*(.text.'), body.index('*(.rodata.'))


class LinkSpaceTest(unittest.TestCase):
    def test_link_addresses_sit_0x8000_past_a_window(self):
        for k in range(len(cm.MODULES)):
            base = cm.link_address(k)
            self.assertEqual(base & 0xffff, 0x8000)
            # One HI16 value covers the whole 64 KiB a module may use.
            self.assertEqual(cm.window(base), cm.window(base + cm.LINK_STRIDE - 1))

    def test_module_of_maps_addresses_back(self):
        self.assertEqual(cm._module_of(cm.link_address(1) + 0x40), (1, 0x40))
        self.assertEqual(cm._module_of(0x80010000), (None, None))


class SceneTableTest(unittest.TestCase):
    REGIONS = ('static SCENE_ACTORS_7: &[hk_sim::ActorSpec] = &[hk_sim::ActorSpec {controller:hk_sim::ActorController::FalseKnight {x:1}}];\n'
               'static SCENE_ACTORS_9: &[hk_sim::ActorSpec] = &[hk_sim::ActorSpec {controller:hk_sim::ActorController::Runner {x:1}},'
               'hk_sim::ActorSpec {controller:hk_sim::ActorController::HuskGuard {x:1}},hk_sim::ActorSpec {controller:hk_sim::ActorController::ZombieShield {x:1}}];\n')

    def test_rooms_get_the_modules_their_actors_need(self):
        packed = {'scenes': [{'scene_id': 9}, {'scene_id': 3}, {'scene_id': 7}]}
        names = list(cm.MODULES)
        # A boss room also carries the arena controller (room logic).
        self.assertEqual(cm.scene_table(packed, self.REGIONS),
                         {0: [names.index('husks')], 2: sorted([names.index('false_knight'), names.index('arena')])})


class MapFilterTest(unittest.TestCase):
    MAP = ''.join([
        '     VMA      LMA     Size Align Out     In      Symbol\n',
        '80408000 80408000     5270     4 .mod_false_knight\n',
        '80408000 80408000     1340     4         a.o:(.text.fk)\n',
        '80418000 80418000     4650     4 .mod_mawlek\n',
        '80418000 80418000     1660     4         a.o:(.text.mw)\n',
        '80010000 80010000    bff74    16 .text\n',
        '80010000 80010000        8     4         a.o:(.text._start)\n',
    ])

    def test_keeps_only_the_named_module(self):
        kept = cm.map_with_only(self.MAP, 'mawlek')
        self.assertNotIn('.mod_false_knight', kept)
        self.assertIn('.text.mw', kept)

    def test_drops_every_module_for_the_resident_exe(self):
        kept = cm.map_with_only(self.MAP)
        self.assertNotIn('.mod_', kept)
        self.assertIn('.text._start', kept)


class HashTest(unittest.TestCase):
    def test_word_hash_matches_the_guest_fold(self):
        data = bytes(range(16))
        h = 0x811c9dc5
        for i in range(0, 16, 4):
            h = ((h ^ int.from_bytes(data[i:i + 4], 'little')) * 0x01000193) & 0xffffffff
        self.assertEqual(cm.word_hash(data), h)


if __name__ == '__main__':
    unittest.main()


class PropsArtTest(unittest.TestCase):
    """Room art: the prop art splits into one contiguous package per prop kind."""
    def setUp(self):
        rs, hk = ROOT / 'data/props.rs', ROOT / 'data/props.hk'
        if not rs.is_file() or not hk.is_file():
            self.skipTest('no cooked props in this checkout')
        self.rs, self.hk = rs.read_text(), hk.read_bytes()

    def test_packages_cover_the_art_once(self):
        blobs, rust = cm.props_art(self.rs, self.hk)
        palettes = int(cm.re.search(r'PALETTE_COUNT:usize=(\d+)', self.rs)[1]) * 32
        self.assertEqual(sum(len(b) for b in blobs.values()), len(self.hk) - palettes)
        self.assertIn('pub const PART_ART:', rust)
        self.assertEqual(set(blobs), set(cm.DATA_MODULES))

    def test_rooms_that_place_props_carry_their_art(self):
        scenes = cm.props_scenes(self.rs)
        self.assertTrue(any('art_grub' in v for v in scenes.values()))
        for v in scenes.values():
            self.assertLessEqual(v, set(cm.DATA_MODULES))


class CompressedArtTests(unittest.TestCase):
    def test_art_chunks_compress_only_when_they_decode_in_place(self):
        import lz4.block
        # Texel-like bytes: few distinct values, short runs (4bpp sprite art).
        state, blob = 1, bytearray()
        for _ in range(20000):
            state = (state * 1103515245 + 12345) & 0x7fffffff
            blob.append((state >> 16) & 0x33)
        blob = bytes(blob)
        z = cm.art_stored(blob)
        self.assertIsNotNone(z)
        self.assertEqual(lz4.block.decompress(z[8:], uncompressed_size=len(blob)), blob)
        reserve = cm.art_reserve(len(blob), len(z))
        start = reserve - -(-len(z) // 2048) * 2048
        self.assertTrue(cm.inplace_fits(z, len(blob), start + len(z)))
        # With the input starting at the arena front there is no room to decode in place.
        self.assertFalse(cm.inplace_fits(z, len(blob), len(z)))
        # Incompressible bytes, or a saving under one sector, ship raw.
        self.assertIsNone(cm.art_stored(bytes((i * 7919) & 255 for i in range(3000))))
