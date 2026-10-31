"""Conservative mandatory packet admission for the native scenery grid repair.

No source asset extraction or disc writes. Reads final cooked HKROOM02 packs,
checks metadata hashes, and emits one per-region packet reservation. The bound
counts every source cell, independent of camera visibility or mutable masks.
"""
from __future__ import annotations
import argparse
from fractions import Fraction
import hashlib
import json
from pathlib import Path
import struct

CAP = 1032
ACTOR_RESERVE = 264  # 128 particles +32 impacts/debris +8 actors +80 Geo +16 Lifeblood parts
CHILD_CAPACITY = 64
MAX_DIVISIONS = 64
MAX_COORDINATE = 1 << 20
# A visible integer child has minX<=319,maxX>=1,minY<=239,maxY>=1.
# Width704 gives -703<=X<=1023; height511 gives -510<=Y<=750.
SAFE_WIDTH = 704
SAFE_HEIGHT = 511


def ceil_fraction(value: Fraction) -> int:
    return -(-value.numerator // value.denominator)


def axis_offsets(span: int, n: int) -> list[int]:
    # Python round(Fraction) is exact nearest-even, matching the native helper.
    return sorted({round(Fraction(i * span, n)) for i in range(n + 1)}) if span else [0, 0]


SCREEN_WIDTH = 320
SCREEN_HEIGHT = 240


class Packets(int):
    """A draw's grid packet count (the int itself, which CHILD_CAPACITY and the
    cook's admission test compare) that also carries `on_screen`, the most
    packets the draw can emit at any camera position.

    The renderer (game/src/scenery_geometry.rs `subdivide_in`) drops every child
    wholly left of x 0, right of x 320, above y 0 or below y 240, so a huge
    background fog split into an 8x8 grid emits at most the handful of cells a
    320x240 window can touch. `mandatory_packets` sums `on_screen`, so every
    caller that totals draws through it (the budget table, the scene pack check,
    the similarity reservation guard) gets the sound bound without a new
    argument; `grid=True` sums the counts of the whole grid instead.
    """
    on_screen: int

    def __new__(cls, grid: int, on_screen: int):
        value = super().__new__(cls, grid)
        value.on_screen = min(grid, on_screen)
        return value


def cells_in_window(span_q8: int, texels: int, steps: list[int], window: int) -> int | None:
    """Most grid cells along one axis that a `window`-pixel stretch can touch.

    `span_q8` is the cooked Q24.8 length of the quad along the axis, `texels` the
    texture range (width-1) that axis maps, `steps` the texel step of each cell.
    Projection floors a common offset, so the projected length is at least
    span//256, and each shared boundary is rounded to a pixel, which can shave
    one pixel off a cell. Cells that touch the open window (0, window) are
    consecutive; all but the first and last lie inside it. None: no bound.
    """
    if texels < 1:
        return len(steps)
    narrowest = Fraction((span_q8 // 256) * min(steps), texels) - 1
    if narrowest <= 0:
        return None
    return min(len(steps), window // ceil_fraction(narrowest) + 2)


def on_screen_packets(xy: list[tuple[int, int]], width: int, height: int, divisions: int, grid: int) -> int:
    """The most packets one draw emits on screen: the cells a 320x240 window can
    touch, for an axis-aligned quad; `grid` (the whole grid) for any other."""
    if grid == 1:
        return 1
    (x0, y0), (x1, y1), (x2, y2), (x3, y3) = xy
    if y0 == y1 and y2 == y3 and x0 == x2 and x1 == x3:
        u_span, v_span, u_window, v_window = abs(x1 - x0), abs(y2 - y0), SCREEN_WIDTH, SCREEN_HEIGHT
    elif x0 == x1 and x2 == x3 and y0 == y2 and y1 == y3:
        u_span, v_span, u_window, v_window = abs(y1 - y0), abs(x2 - x0), SCREEN_HEIGHT, SCREEN_WIDTH
    else:
        return grid
    us, vs = axis_offsets(width - 1, divisions), axis_offsets(height - 1, divisions)
    along_u = cells_in_window(u_span, width - 1, [b - a for a, b in zip(us, us[1:])], u_window)
    along_v = cells_in_window(v_span, height - 1, [b - a for a, b in zip(vs, vs[1:])], v_window)
    if along_u is None or along_v is None:
        return grid
    return along_u * along_v


def packet_bound(xy: list[tuple[int, int]], width: int, height: int) -> dict:
    """xy are the cooked signed Q24.8 screen-coordinate values before camera.

    Parent projection floors after a common camera subtraction. Every projected
    edge component is bounded by ceil(abs(cooked_delta)/256). A bilinear cell's
    extent is bounded by its two derivative maxima times exact UV intervals,
    plus one pixel for the difference of two nearest-rounded shared vertices.
    """
    if width < 1 or height < 1 or width > 256 or height > 256:
        raise ValueError('invalid texture dimensions')
    projected_bbox = [(max(p[k] for p in xy) - min(p[k] for p in xy) + 255) // 256 for k in range(2)]
    if projected_bbox[0] <= SAFE_WIDTH and projected_bbox[1] <= SAFE_HEIGHT:
        return {'packets': Packets(1, 1), 'divisions': 1, 'child_extent_bound': projected_bbox}
    dx = [max((abs(xy[1][k]-xy[0][k])+255)//256, (abs(xy[3][k]-xy[2][k])+255)//256) for k in range(2)]
    dy = [max((abs(xy[2][k]-xy[0][k])+255)//256, (abs(xy[3][k]-xy[1][k])+255)//256) for k in range(2)]
    n = 2
    while n <= MAX_DIVISIONS:
        us, vs = axis_offsets(width-1,n), axis_offsets(height-1,n)
        du = Fraction(max(b-a for a,b in zip(us,us[1:])),width-1) if width > 1 else Fraction(1)
        dv = Fraction(max(b-a for a,b in zip(vs,vs[1:])),height-1) if height > 1 else Fraction(1)
        extent = [ceil_fraction(dx[k]*du + dy[k]*dv) + 1 for k in range(2)]
        if extent[0] <= SAFE_WIDTH and extent[1] <= SAFE_HEIGHT:
            grid = (len(us)-1)*(len(vs)-1)
            return {'packets': Packets(grid, on_screen_packets(xy, width, height, n, grid)),
                    'divisions': n, 'child_extent_bound': extent}
        n *= 2
    raise ValueError(f'no safe bounded grid for texture {width}x{height}, source bbox {projected_bbox}')


def texture_draws_safe(raw: bytes, texture: int, width: int, height: int) -> bool:
    """True when every draw of `texture` in a cooked HKROOM02 pack keeps a bounded
    packet grid if the texture became width×height (similarity/constant collapse)."""
    counts = struct.unpack_from('<6I', raw, 8)
    draw_start = 40 + counts[1]*16
    for index in range(counts[2]):
        pos = draw_start + index*44
        if struct.unpack_from('<H', raw, pos)[0] != texture:
            continue
        coords = struct.unpack_from('<8i', raw, pos+8)
        try:
            bound = packet_bound(list(zip(coords[::2], coords[1::2])), width, height)
        except ValueError:
            return False
        if bound['packets'] > CHILD_CAPACITY:
            return False
    return True


def check_camera_arithmetic(xy, scale, camera):
    """Verify post-shift/subtraction bounds across the region camera box.

    Camera multiplication MUST use i64 before >>12; some valid source scales
    exceed i32 at runtime camera bounds. Endpoint checks suffice by monotonicity.
    """
    largest = 0
    largest_product = 0
    for k in range(2):
        for endpoint in (camera[k],camera[k+2]):
            q = round(endpoint * 65536)
            product = (q >> 8) * scale
            largest_product = max(largest_product,abs(product))
            offset = product >> 12
            for p in xy:
                difference = p[k] - offset
                if not -(1<<31) <= difference < (1<<31):
                    raise ValueError('camera subtraction exceeds native i32')
                projected = 160 + (difference >> 8) if k == 0 else 120 - (difference >> 8)
                largest = max(largest,abs(projected))
                if abs(projected) > MAX_COORDINATE:
                    raise ValueError('projection exceeds native geometry arithmetic bound')
    return largest, largest_product


def mandatory_packets(packets: list[int], groups: list[list[int]], grid: bool = False) -> int:
    """A view's mandatory packet total: every draw's packets, except that each
    decor group (game/src/decor.rs shows exactly one of its frames at a time,
    the others hidden) counts only its largest frame.

    A draw given as `Packets` counts the packets it can emit on screen; `grid`
    counts every cell of its grid instead (the figure before on-screen bounds,
    and plain ints, which carry no more than the grid)."""
    if not grid:
        packets = [getattr(p, 'on_screen', p) for p in packets]
    grouped = set()
    total = 0
    for group in groups:
        if group:
            total += max(packets[i] for i in group)
            grouped.update(group)
    return total + sum(p for i, p in enumerate(packets) if i not in grouped)


def view_packets(room: bytes, groups: list[list[int]]) -> dict:
    """Mandatory packet totals of one cooked HKROOM02 pack: `grid` (every cell
    of every draw's grid) and `on_screen` (what the renderer can emit)."""
    counts = struct.unpack_from('<6I', room, 8)
    draw_start = 40 + counts[1]*16
    packets = []
    for index in range(counts[2]):
        pos = draw_start + index*44
        texture, = struct.unpack_from('<H', room, pos)
        coords = struct.unpack_from('<8i', room, pos+8)
        w, h = struct.unpack_from('<2H', room, 40 + texture*16 + 6)
        packets.append(packet_bound(list(zip(coords[::2], coords[1::2])), w, h)['packets'])
    return {'grid': mandatory_packets(packets, groups, grid=True),
            'on_screen': mandatory_packets(packets, groups)}


#: Views whose on-screen total leaves fewer packets than this are reported.
NEAR_BUDGET = 64


def check_view_budget(chunk_id: int, scene: str, room: bytes, groups: list[list[int]]) -> dict:
    """Total one view, print a line when it is near the budget, and raise the
    moment it is over, so a long cook fails on the slot that breaks it.

    `headroom` is what is left of CAP - ACTOR_RESERVE after the on-screen
    total. A view whose whole-grid total alone is over the limit is `clamped`:
    the budget table holds the limit (the optional-scissor allowance is then 0)
    and the on-screen bound is what proves the view fits."""
    totals = view_packets(room, groups)
    limit = CAP - ACTOR_RESERVE
    headroom = limit - totals['on_screen']
    if headroom < 0:
        raise ValueError(f'region {chunk_id} ({scene}) needs {totals["on_screen"]}+{ACTOR_RESERVE} packets > {CAP} '
                         f'even with only the cells a 320x240 window can touch ({totals["grid"]} counting every cell)')
    if headroom < NEAR_BUDGET or totals['grid'] > limit:
        print(f'PACKETS region {chunk_id} {scene}: on screen {totals["on_screen"]} of {limit} '
              f'(headroom {headroom}), whole grid {totals["grid"]}'
              + (', budget clamped to the limit' if totals['grid'] > limit else ''), flush=True)
    return {**totals, 'headroom': headroom}


def print_tightest(rows: list[dict], regions: list[dict], count: int = 12) -> None:
    """The views with the least headroom under CAP - ACTOR_RESERVE, so the next
    overflow shows in the build log long before it fails a cook."""
    names = {r['chunk_id']: r.get('scene_name', '?') for r in regions}
    limit = CAP - ACTOR_RESERVE
    clamped = sum(r['grid_packets'] > limit for r in rows)
    print(f'Scenery packets: {len(rows)} views, limit {limit} (CAP {CAP} less {ACTOR_RESERVE} for actors, '
          f'effects and Geo); {clamped} view(s) only fit by their on-screen bound', flush=True)
    for r in sorted(rows, key=lambda r: r['headroom'])[:count]:
        flag = ' NEAR BUDGET' if r['headroom'] < NEAR_BUDGET else ''
        print(f"  region {r['chunk_id']:4} {names[r['chunk_id']]:<24} on screen {r['on_screen_packets']:4} "
              f"grid {r['grid_packets']:4} headroom {r['headroom']:4}{flag}", flush=True)


def generate(root: Path, metadata: Path | None = None) -> dict:
    metadata = metadata or root/'data/regions.json'
    regions = json.loads(metadata.read_text())['regions']
    result = []
    for region in regions:
        data = (root/region['path']).read_bytes()
        digest = hashlib.sha256(data).hexdigest()
        if digest != region['sha256']:
            raise ValueError(f"stale room hash: {region['path']}")
        if data[:8] != b'HKROOM02':
            raise ValueError('unsupported room format')
        counts = struct.unpack_from('<6I',data,8)
        draw_start = 40 + counts[1]*16
        draws = []
        for index in range(counts[2]):
            pos = draw_start + index*44
            texture, = struct.unpack_from('<H',data,pos)
            scale, = struct.unpack_from('<i',data,pos+4)
            coords = struct.unpack_from('<8i',data,pos+8)
            xy = list(zip(coords[::2],coords[1::2]))
            texpos = 40 + texture*16
            page,u,v,w,h = struct.unpack_from('<5H',data,texpos)
            if u+w > 256 or v+h > 256:
                raise ValueError('UV rectangle exceeds one page')
            bound = packet_bound(xy,w,h)
            if bound['packets'] > CHILD_CAPACITY:
                raise ValueError(f"region {region['chunk_id']} draw {index} needs {bound['packets']} children > {CHILD_CAPACITY}")
            largest, product = check_camera_arithmetic(xy,scale,region['camera_bounds'])
            draws.append({'draw':index,'texture':texture,'max_abs_projection':largest,'max_abs_camera_product':product,**bound})
        groups = [g['draws'] for g in region.get('decor', [])]
        on_screen = mandatory_packets([d['packets'] for d in draws], groups)
        grid = mandatory_packets([d['packets'] for d in draws], groups, grid=True)
        if on_screen + ACTOR_RESERVE > CAP:
            raise ValueError(f"region {region['chunk_id']} needs {on_screen}+{ACTOR_RESERVE} packets > {CAP}")
        # The table is what the renderer subtracts to size its optional scissor
        # allowance, so a view that always fit keeps its whole-grid figure (its
        # rendering does not change) and a view that only fits by the on-screen
        # bound is held at the limit.
        mandatory = min(grid, CAP - ACTOR_RESERVE)
        result.append({'chunk_id':region['chunk_id'],'path':region['path'],'sha256':digest,
                       'draws':counts[2],'mandatory_packets':mandatory,'on_screen_packets':on_screen,
                       'grid_packets':grid,'headroom':CAP - ACTOR_RESERVE - on_screen,
                       'maximum_draw_packets':max(d['packets']for d in draws),
                       'max_abs_projection':max(d['max_abs_projection']for d in draws),
                       'max_abs_camera_product':max(d['max_abs_camera_product']for d in draws),
                       'expanded_draws':[d for d in draws if d['packets']>1]})
    result.sort(key=lambda r:r['chunk_id'])
    if [r['chunk_id']for r in result] != list(range(1,len(result)+1)):
        raise ValueError('region chunk IDs must be contiguous and one-based')
    report = {'format':'HKSCENERYBUDGET01','capacity':CAP,'actor_reserve':ACTOR_RESERVE,'child_capacity':CHILD_CAPACITY,
              'scope':'all source cells and all region camera positions; ignores visibility for reservation',
              'helper_sha256':hashlib.sha256((root/'game/src/scenery_geometry.rs').read_bytes()).hexdigest(),
              'host_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
              'regions':result}
    (root/'data/scenery_budgets.rs').write_text('// Generated by host/scenery_geometry.py; indexed by chunk_id - 1.\n'
        +f'pub const DYNAMIC_PACKET_RESERVE:usize={ACTOR_RESERVE};\n'
        +'pub const SCENERY_PACKET_BUDGETS: &[u16] = &[\n'
        + ''.join(f"    {r['mandatory_packets']}, // chunk {r['chunk_id']}\n" for r in result)+'];\n')
    if any(not 0 <= r['mandatory_packets'] < 65536 for r in result):
        raise ValueError('scenery packet budget exceeds u16')
    print_tightest(result, regions)
    (root/'.hkpsx/scenery-geometry-budgets.json').write_text(json.dumps(report,indent=2)+'\n')
    return report


if __name__ == '__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1])
    args=parser.parse_args()
    report=generate(args.root)
    rows=report['regions']
    print(f"{len(rows)} regions admitted; mandatory max {max(r['mandatory_packets']for r in rows)}+{ACTOR_RESERVE}/{CAP}; "
          f"max per draw {max(r['maximum_draw_packets']for r in rows)}; max projected coordinate {max(r['max_abs_projection']for r in rows)}")
