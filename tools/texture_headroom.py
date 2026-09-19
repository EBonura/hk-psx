"""What is left of the 416-slot texture budget, per scene and globally.

The CLUT budget is the binding constraint on this port, tighter than RAM, and it
is measured per view rather than per scene: a scene's answer is its worst view,
because that is the one that has to hold whatever gets added.

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


def headroom(report):
    """Per scene: view count, tightest view, its chunk, and slots left there."""
    cap = report['quality']['texture_budget']
    worst = collections.defaultdict(lambda: (-1, -1))
    views = collections.Counter()
    for row in report['regions']:
        views[row['scene_name']] += 1
        worst[row['scene_name']] = max(worst[row['scene_name']], (row['textures'], row['chunk_id']))
    scenes = {name: {'views': views[name], 'tightest_textures': t, 'tightest_chunk': chunk,
                     'headroom': cap - t}
              for name, (t, chunk) in worst.items()}
    return {'texture_budget': cap, 'scenes': scenes,
            'global_headroom': min(s['headroom'] for s in scenes.values()),
            'note': 'global_headroom is what art resident in every view may cost; a '
                    'scene headroom is what art standing only in that scene may cost.'}


def main():
    out = headroom(json.loads((ROOT / 'data/regions.json').read_text()))
    rows = sorted(out['scenes'].items(), key=lambda kv: kv[1]['headroom'])
    print(f"{out['texture_budget']} CLUT slots per view, {sum(s['views'] for s in out['scenes'].values())} "
          f"views in {len(out['scenes'])} scenes")
    print(f"{'scene':26}{'views':>6}{'tightest':>9}{'headroom':>9}  chunk")
    for name, s in rows:
        print(f"{name:26}{s['views']:6}{s['tightest_textures']:9}{s['headroom']:9}  {s['tightest_chunk']}")
    print(f"\nresident in every view: {out['global_headroom']} slots "
          f"(set by {rows[0][0]} view {rows[0][1]['tightest_chunk']})")
    return out


if __name__ == '__main__':
    sys.exit(0 if main() else 0)
