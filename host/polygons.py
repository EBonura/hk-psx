"""Split simple polygons into bounded-vertex pieces with an identical union.

Guest hazard and checkpoint records hold at most 16 vertices per polygon while
source PolygonCollider2D paths may have many more (Abyss_08 spikes reach 46).
Ear clipping triangulates the polygon, the triangulation's dual tree is
partitioned into connected groups of at most limit-2 triangles, and each
group's outer boundary is one simple polygon. Overlap/containment tests
against the pieces are equivalent to the original because the pieces tile it
exactly; every piece vertex is an original vertex.
"""
from fractions import Fraction

POLYGON_VERTEX_LIMIT = 16


def _area2(a, b, c):
    return (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])


def _inside_triangle(p, a, b, c):
    d = (_area2(a, b, p), _area2(b, c, p), _area2(c, a, p))
    return not (any(v < 0 for v in d) and any(v > 0 for v in d))


def triangulate(points):
    """Ear-clip a simple polygon of any orientation into counter-clockwise index triangles."""
    pts = [(Fraction(x), Fraction(y)) for x, y in points]
    idx = [i for i in range(len(pts)) if pts[i] != pts[i - 1]]
    if len(idx) < 3:
        raise ValueError('polygon needs three distinct vertices')
    area = sum(_area2((0, 0), pts[idx[i]], pts[idx[(i + 1) % len(idx)]]) for i in range(len(idx)))
    if area == 0:
        raise ValueError('degenerate polygon')
    if area < 0:
        idx.reverse()
    triangles = []
    while len(idx) > 3:
        n = len(idx)
        for k in range(n):
            a, b, c = idx[k - 1], idx[k], idx[(k + 1) % n]
            cross = _area2(pts[a], pts[b], pts[c])
            if cross < 0:
                continue  # reflex vertex
            if cross == 0:
                del idx[k]  # collinear vertex adds no area
                break
            if any(_inside_triangle(pts[o], pts[a], pts[b], pts[c]) for o in idx if o not in (a, b, c)):
                continue
            triangles.append((a, b, c))
            del idx[k]
            break
        else:
            raise ValueError('polygon is not simple')
    if _area2(*(pts[i] for i in idx)) > 0:
        triangles.append(tuple(idx))
    return triangles


def bounded_polygons(points, limit=POLYGON_VERTEX_LIMIT):
    """Return pieces of at most `limit` vertices whose union is exactly `points`."""
    points = list(points)
    if len(points) <= limit:
        return [points]
    triangles = triangulate(points)
    edges = {}
    for t, tri in enumerate(triangles):
        for k in range(3):
            edges.setdefault(frozenset((tri[k], tri[(k + 1) % 3])), []).append(t)
    neighbours = {t: set() for t in range(len(triangles))}
    for owners in edges.values():
        for t in owners:
            neighbours[t].update(o for o in owners if o != t)
    # Dual tree of the triangulation: partition it into connected groups of at
    # most limit-2 triangles, merging children into parents greedily.
    parent = {0: None}
    order = [0]
    for t in order:
        for n in neighbours[t]:
            if n not in parent:
                parent[n] = t
                order.append(n)
    if len(order) != len(triangles):
        raise ValueError('triangulation is not connected')
    group = {t: t for t in range(len(triangles))}
    members = {t: {t} for t in range(len(triangles))}
    for t in reversed(order):
        p = parent[t]
        if p is None:
            continue
        mine, theirs = group[t], group[p]
        if len(members[mine]) + len(members[theirs]) <= limit - 2:
            for m in members[mine]:
                group[m] = theirs
            members[theirs] |= members.pop(mine)
    pieces = []
    for tris in members.values():
        directed = {(triangles[t][k], triangles[t][(k + 1) % 3]) for t in tris for k in range(3)}
        boundary = {a: b for a, b in directed if (b, a) not in directed}
        if len(boundary) != len(tris) + 2:
            raise ValueError('piece boundary is not simple')
        start = next(iter(boundary))
        cycle = [start]
        while boundary[cycle[-1]] != start:
            cycle.append(boundary[cycle[-1]])
        if len(cycle) != len(boundary):
            raise ValueError('piece boundary is not one cycle')
        pieces.append([points[i] for i in cycle])
    return pieces
