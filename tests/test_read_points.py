"""Tablet layout bounds without source assets or localized payload fixtures."""
import sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'host'))
from read_points import wrap_page
class ReadLayout(unittest.TestCase):
    def test_original_breaks_wrap_without_truncating_words(self):
        pages=wrap_page('one two three<br><br>four five',[5]*95,width=35,lines_per_page=3)
        self.assertEqual(pages,[['one two','three',''],['four','five']])
    def test_oversize_words_and_unhandled_markup_are_rejected(self):
        for text in ['abcdefgh','<sprite=1>','bad\ttext']:
            with self.assertRaises(ValueError):wrap_page(text,[5]*95,width=35)
    def test_typographic_quotes_keep_meaning_with_ascii_font(self):
        self.assertEqual(wrap_page('“It’s fine”',[5]*95),[['"It\'s fine"']])
