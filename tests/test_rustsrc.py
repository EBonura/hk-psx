"""host/rustsrc.py reads the same facts however `cargo fmt` lays the source out."""
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))
import rustsrc

HAND = (
    'pub const SLOTS:usize=24;pub const NEG:[i32;4]=[-1102971,-259850,1102971,181207];\n'
    'pub enum Clip{Walk,Turn,Idle}\n'
    'pub struct Save{pub magic:[u8;4],pub scene:u16}\n'
    'pub const CAP:usize=SLOTS*2+0x10;\n'
)
FORMATTED = '''
/// A doc comment mentioning `pub const SLOTS: usize = 99;` must not count.
pub const SLOTS: usize = 24; // trailing note

pub const NEG: [i32; 4] = [
    -1_102_971,
    -259_850, /* inline */
    1_102_971,
    181_207,
];

#[derive(Clone, Copy)]
pub enum Clip {
    /// First.
    Walk,
    Turn,
    #[default]
    Idle,
}

pub struct Save {
    pub magic: [u8; 4],
    pub scene: u16,
}

pub const CAP: usize = SLOTS as usize
    * 2
    + 0x10;
'''


class RustSrcTests(unittest.TestCase):
    def test_layout_does_not_change_what_is_read(self):
        for text in (HAND, FORMATTED):
            text = rustsrc.compact(text)
            self.assertEqual(rustsrc.const_int(text, 'SLOTS'), 24)
            self.assertEqual(rustsrc.const_int(text, 'CAP'), 64)
            self.assertEqual(rustsrc.const_ints(text, 'NEG'), [-1102971, -259850, 1102971, 181207])
            self.assertEqual(rustsrc.enum_variants(text, 'Clip'), ['Walk', 'Turn', 'Idle'])
            self.assertEqual(rustsrc.struct_fields(text, 'Save'), ['magic', 'scene'])
            self.assertEqual(rustsrc.consts(text)['NEG'][2], 1102971)

    def test_both_layouts_compact_to_the_same_text_for_a_declaration(self):
        self.assertEqual(rustsrc.compact('pub const A : u16 =\n    1 ;'), 'pub const A:u16=1;')
        self.assertEqual(rustsrc.compact('f(a,\n  b,\n)'), 'f(a,b)')

    def test_comments_and_strings_are_not_confused(self):
        text = rustsrc.compact('const A: &str = "// not a comment"; // real one\nconst B: u8 = b\'x\';')
        self.assertIn('"// not a comment"', text)
        self.assertNotIn('real one', text)
        self.assertEqual(rustsrc.const_expr(text, 'B'), "b'x'")

    def test_contains_ignores_layout(self):
        text = rustsrc.compact(FORMATTED)
        self.assertTrue(rustsrc.contains(text, 'pub scene : u16'))
        self.assertFalse(rustsrc.contains(text, 'pub scene: u32'))

    def test_a_missing_constant_is_a_key_error(self):
        with self.assertRaises(KeyError):
            rustsrc.const_int(rustsrc.compact(HAND), 'NOPE')

    def test_the_real_budgets_the_cook_depends_on_are_readable(self):
        self.assertGreater(rustsrc.const_int('shared/hk-cache/src/residency.rs', 'SPARE_HALFWORDS'), 0)
        self.assertGreater(rustsrc.const_int('shared/hk-cache/src/lib.rs', 'SLOTS'), 0)
        self.assertGreater(rustsrc.const_int('game/src/world.rs', 'SCRIPT_EDGE_SLOTS'), 0)


if __name__ == '__main__':
    unittest.main()
