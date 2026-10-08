"""Soul totem table: what the guest links."""
import sys
import unittest
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))
import soul_totems


class SoulTotemTests(unittest.TestCase):
    def test_rows_carry_hits_orbs_and_q16_bounds(self):
        text = soul_totems.rust([{'scene': 16, 'local': 0, 'hits': 3, 'orbs': [8, 9], 'wait_ticks': 15,
                                  'bounds': [1.0, 2.0, 3.5, 4.25]}])
        self.assertIn('pub const SOUL_PER_ORB:u16=2;', text)
        self.assertIn('Totem{scene:16,local:0,bounds:[65536,131072,229376,278528],hits:3,orbs:[8,9],wait_ticks:15},', text)



if __name__ == '__main__':
    unittest.main()
