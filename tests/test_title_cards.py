"""Title card table: what the guest links."""
import sys
import unittest
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))
import title_cards


class TitleCardTests(unittest.TestCase):
    def test_typographic_quotes_become_the_fonts_ascii(self):
        self.assertEqual(title_cards.ascii_text('King’s Pass'), "King's Pass")
        with self.assertRaises(ValueError):
            title_cards.ascii_text('Ω')

    def test_rows_name_their_title_and_trigger(self):
        areas = [{'scene': 1, 'event': 'DIRTMOUTH', 'trigger': [0.0, 0.0, 1.0, 1.0], 'unvisited_ticks': 0,
                  'visited_ticks': 120, 'only_on_revisit': False},
                 {'scene': 0, 'event': 'KINGSPASS', 'trigger': None, 'unvisited_ticks': 120,
                  'visited_ticks': 120, 'only_on_revisit': False}]
        text = title_cards.rust(areas, ['DIRTMOUTH', 'KINGSPASS', 'FALSE_KNIGHT'],
                                [['', 'Dirtmouth', 'The Fading Town'], ['', "King's Pass", ''], ['', 'False Knight', '']])
        self.assertIn('pub const FALSE_KNIGHT:u8=2;', text)
        self.assertIn('Area{scene:1,title:0,trigger:Some([0,0,65536,65536]),unvisited_ticks:0,visited_ticks:120,only_on_revisit:false},', text)
        self.assertIn('Area{scene:0,title:1,trigger:None,', text)


if __name__ == '__main__':
    unittest.main()
