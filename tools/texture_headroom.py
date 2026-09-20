"""What is left of the 416-slot CLUT budget, per scene and globally.

The CLUT budget is the binding constraint on this port, tighter than RAM, and it
is measured per view rather than per scene: a scene's answer is its worst view,
because that is the one that has to hold whatever gets added.

A slot is a distinct palette, not a texture. HKROOM02 stores one 32-byte entry
per texture, so the two counts used to be the same number and this tool reported
the texture count. They stopped being the same when frames started tiling: the
tiles of one frame are quantized together and carry byte-identical palette
words, and nothing uploads a region's block as it stands, because the scene bank
pools palettes by value and a scene uploads that pool. Measured over the False
Knight's whole clip set, 499 tiles carry 107 distinct palettes. So the slot
column below counts distinct words and the texture column is reported beside it,
because that one is bounded separately by what HKROOM02's table can address.

Two different questions have very different answers, which is the reason this
exists. Art that only has to exist where it stands is measured against its own
scene's tightest view, and most scenes have room. Art that has to exist in every
view, a pause-screen icon or a HUD element, is measured against the tightest
view in the whole world, and that number is small enough that the answer is
usually the Hollow Shade route instead: frames in linked RAM, reaching VRAM
through shared animation slots.
"""
import collections, json, sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))

from cook import MAX_ROOM_TEXTURES  # the format's own limit, not a second copy of it


def view_cluts(report):
    """Distinct palettes per view, read from the cooked packs.

    The cook report carries this as `cluts` from now on, but the packs already
    on disc predate the field, so a pack that has not been recooked is measured
    rather than assumed. Both answers are the same bytes either way.
    """
    from texture_dedup import clut_count
    out = {}
    for row in report['regions']:
        if 'cluts' in row:
            out[row['chunk_id']] = row['cluts']
        else:
            out[row['chunk_id']] = clut_count((ROOT / row['path']).read_bytes())
    return out


def headroom(report, cluts):
    """Per scene: view count, tightest view, its chunk, and slots left there."""
    cap = report['quality']['texture_budget']
    worst = collections.defaultdict(lambda: (-1, -1, -1))
    views = collections.Counter()
    for row in report['regions']:
        views[row['scene_name']] += 1
        worst[row['scene_name']] = max(worst[row['scene_name']],
                                       (cluts[row['chunk_id']], row['textures'], row['chunk_id']))
    scenes = {name: {'views': views[name], 'tightest_cluts': c, 'tightest_textures': t,
                     'tightest_chunk': chunk, 'headroom': cap - c,
                     'texture_headroom': MAX_ROOM_TEXTURES - t}
              for name, (c, t, chunk) in worst.items()}
    return {'texture_budget': cap, 'texture_table_limit': MAX_ROOM_TEXTURES, 'scenes': scenes,
            'global_headroom': min(s['headroom'] for s in scenes.values()),
            'global_texture_headroom': min(s['texture_headroom'] for s in scenes.values()),
            'note': 'global_headroom is what art resident in every view may cost; a '
                    'scene headroom is what art standing only in that scene may cost. '
                    'A slot is a distinct palette; texture_headroom is the separate '
                    'limit on how many 16-byte records HKROOM02 can address.'}


def main():
    report = json.loads((ROOT / 'data/regions.json').read_text())
    out = headroom(report, view_cluts(report))
    rows = sorted(out['scenes'].items(), key=lambda kv: kv[1]['headroom'])
    print(f"{out['texture_budget']} CLUT slots per view, {sum(s['views'] for s in out['scenes'].values())} "
          f"views in {len(out['scenes'])} scenes")
    print(f"{'scene':26}{'views':>6}{'cluts':>7}{'free':>6}{'textures':>10}{'free':>6}  chunk")
    for name, s in rows:
        print(f"{name:26}{s['views']:6}{s['tightest_cluts']:7}{s['headroom']:6}"
              f"{s['tightest_textures']:10}{s['texture_headroom']:6}  {s['tightest_chunk']}")
    print(f"\nresident in every view: {out['global_headroom']} slots "
          f"(set by {rows[0][0]} view {rows[0][1]['tightest_chunk']})")
    return out


if __name__ == '__main__':
    sys.exit(0 if main() else 0)
