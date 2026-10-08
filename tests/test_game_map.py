"""The quick map's generated tables against the VRAM layout and the scene list."""
import json, re, unittest
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]
import sys
sys.path.insert(0, str(ROOT / 'host'))


def residency():
    text = (ROOT / 'shared/hk-cache/src/residency.rs').read_text()
    return re.search(r'pub const MAP_PAGE:\(usize,usize,usize,usize\)=\(384\+\(18%10\)\*64,\(18/10\)\*256,64,256\);', text)


class GameMapLayout(unittest.TestCase):
    def test_cook_constants_match_the_vram_reservation(self):
        import game_map
        self.assertIsNotNone(residency(), 'residency.rs MAP_PAGE is no longer page 18')
        self.assertEqual(game_map.PAGE_XY, (384 + (18 % 10) * 64, (18 // 10) * 256))
        # The palettes sit in the page's own last row, inside its reservation.
        self.assertEqual(game_map.CLUT_XY, [(896 + 16 * i, 511) for i in range(4)])

    @unittest.skipUnless((ROOT / '.hkpsx/packed-scenes.json').is_file(), 'needs the packed scene report')
    def test_only_the_arena_shares_the_map_page(self):
        """Page 18 is the map's except in a scene that needs all nineteen
        pages; the guest reads the map back after one. That is one scene today,
        the False Knight's arena, and a second would be worth knowing about."""
        packed = json.loads((ROOT / '.hkpsx/packed-scenes.json').read_text())
        full = [s['scene_id'] for s in packed['scenes'] if s['pages'] >= 19]
        self.assertLessEqual(len(full), 1, f'scenes {full} all overwrite the map page')

    @unittest.skipUnless((ROOT / 'data/game_map.rs').is_file(), 'needs host/game_map.py output')
    def test_generated_tables_are_consistent(self):
        text = (ROOT / 'data/game_map.rs').read_text()
        rows = int(re.search(r'pub const MAP_ROWS:u16=(\d+);', text).group(1))
        self.assertLess(rows, 256)
        blob = (ROOT / 'data/game-map.bin').read_bytes()
        self.assertEqual(len(blob), 128 * (rows + 1))
        for u, v, w, h in re.findall(r'MapTex\{u:(\d+),v:(\d+),w:(\d+),h:(\d+)\}', text):
            self.assertLessEqual(int(u) + int(w), 256)
            self.assertLessEqual(int(v) + int(h), rows)
        regions = json.loads((ROOT / 'data/regions.json').read_text())
        count = max(s['scene_id'] for s in regions['scenes']) + 1
        self.assertIn(f'pub static SCENE_MAP:[SceneMap;{count}]', text)
        # Cornifer's room and the Town are charted; the False Knight's arena is too.
        names = {s['scene_id']: s['scene_name'] for s in regions['scenes']}
        charted = {names[int(sid)] for sid, flag in re.findall(r'MapRoom\{scene:(\d+),area:\d+,charted:(true)', text)}
        self.assertTrue({'Town', 'Tutorial_01', 'Crossroads_33', 'Crossroads_10'} <= charted)


if __name__ == '__main__':
    unittest.main()
