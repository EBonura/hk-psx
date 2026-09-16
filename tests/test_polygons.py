"""Bounded polygon pieces tile the original hazard shape exactly."""
import random
import sys
import unittest
from fractions import Fraction
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'host'))
from polygons import bounded_polygons, triangulate, POLYGON_VERTEX_LIMIT


def area(points):
    return abs(sum(Fraction(points[i][0]) * Fraction(points[(i + 1) % len(points)][1])
                   - Fraction(points[(i + 1) % len(points)][0]) * Fraction(points[i][1])
                   for i in range(len(points)))) / 2


def inside(points, x, y):
    hit = False
    for i in range(len(points)):
        (x0, y0), (x1, y1) = points[i], points[(i + 1) % len(points)]
        if (y0 > y) != (y1 > y) and x < x0 + (y - y0) * (x1 - x0) / (y1 - y0):
            hit = not hit
    return hit


def sawtooth(teeth):
    """Concave comb: a flat bottom edge and `teeth` spikes along the top."""
    top = []
    for i in range(teeth):
        top += [(2 * i, 1.0), (2 * i + 1, 4.0)]
    return [(0.0, 0.0), (2 * teeth, 0.0)] + top[::-1]


class BoundedPolygonTests(unittest.TestCase):
    def check_tiling(self, polygon, limit=POLYGON_VERTEX_LIMIT):
        pieces = bounded_polygons(polygon, limit)
        self.assertTrue(all(3 <= len(p) <= limit for p in pieces), [len(p) for p in pieces])
        self.assertEqual(sum(area(p) for p in pieces), area(polygon))
        rng = random.Random(7)
        xs = [p[0] for p in polygon]; ys = [p[1] for p in polygon]
        for _ in range(300):
            x = rng.uniform(min(xs) - 1, max(xs) + 1) + 0.0137
            y = rng.uniform(min(ys) - 1, max(ys) + 1) + 0.0071
            self.assertEqual(inside(polygon, x, y), any(inside(p, x, y) for p in pieces), (x, y))
        return pieces

    def test_small_polygons_are_returned_unchanged(self):
        square = [(0, 0), (1, 0), (1, 1), (0, 1)]
        self.assertEqual(bounded_polygons(square), [square])

    def test_46_vertex_comb_splits_into_few_exact_pieces(self):
        polygon = sawtooth(22)
        self.assertEqual(len(polygon), 46)
        pieces = self.check_tiling(polygon)
        self.assertLessEqual(len(pieces), 8)
        self.assertLessEqual(len(pieces), 8)
        self.assertTrue(all(v in polygon for p in pieces for v in p))

    def test_orientation_and_repeated_vertices_do_not_matter(self):
        polygon = sawtooth(10)
        self.check_tiling(polygon[::-1])
        self.check_tiling([polygon[0]] + polygon)  # duplicated consecutive point

    def test_degenerate_input_is_rejected(self):
        with self.assertRaises(ValueError):
            triangulate([(0, 0), (1, 1), (2, 2), (3, 3)])
        with self.assertRaises(ValueError):
            bounded_polygons([(0, 0), (1, 1), (2, 2)] * 6, 5)


if __name__ == '__main__':
    unittest.main()
