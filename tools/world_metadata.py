#!/usr/bin/env python3
"""Cook checked HKWMTA01 scene metadata from the canonical region catalogue.

This is deliberately separate from ``host/world.py`` while the guest migration
is staged.  It emits one bank per source scene and never rewrites regions.rs or
the playable disc.
"""
import argparse
import hashlib
import json
import math
import struct
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MAGIC = b"HKWMTA01"
HEADER = 160
REGION_STRIDE, OBJECT_STRIDE, POLYGON_STRIDE, POINT_STRIDE = 76, 36, 8, 8
MAX_Q16 = 512 * 65536
KINDS = {"breakable": 1, "grass": 2, "hazard": 3, "checkpoint": 4, "actor": 5}


def q(value):
    if not math.isfinite(float(value)) or abs(float(value)) > 512:
        raise ValueError(f"world coordinate outside Q16 bounds: {value}")
    return int(round(float(value) * 65536))


def rect(values):
    if len(values) != 4 or values[0] > values[2] or values[1] > values[3]:
        raise ValueError(f"invalid world rectangle: {values}")
    return tuple(q(v) for v in values)


def source_id(value):
    try:
        return int(str(value).rsplit(":", 1)[1])
    except (IndexError, ValueError) as error:
        raise ValueError(f"invalid source identity: {value!r}") from error


def fnv1a(data):
    value = 2166136261
    for byte in data:
        value = ((value ^ byte) * 16777619) & 0xFFFFFFFF
    return value


def align(value):
    return (value + 3) & ~3


def polygon_list(record, key):
    polygons = record.get(key, []) or []
    result = []
    for polygon in polygons:
        if not 3 <= len(polygon) <= 16:
            raise ValueError(f"{key} polygon outside 3..16 vertices")
        result.append([tuple(q(v) for v in point) for point in polygon])
    return result


def encode_scene(scene, rows):
    objects = []
    polygons = []
    points = []
    regions = []

    def add_object(kind, record, state, flags, bounds, shape_key=None):
        shape = polygon_list(record, shape_key) if shape_key else []
        polygon_first = len(polygons)
        for polygon in shape:
            first = len(points)
            points.extend((x, y) for x, y in polygon)
            polygons.append((first, len(polygon)))
        objects.append((source_id(record["source"]), state, KINDS[kind], flags,
                        rect(bounds), polygon_first, len(shape)))

    for row in sorted(rows, key=lambda value: value["chunk_id"]):
        object_first = len(objects)
        region_polygon_first = len(polygons)
        for record in row.get("breakables", []):
            flags = (1 if record.get("persistent") else 0) | (2 if record.get("semi_persistent") else 0)
            add_object("breakable", record, scene["scene_id"] * 128 + int(record["state_index"]),
                       flags, record["box"], "hit_polygons")
        for record in row.get("grass", []):
            add_object("grass", record, scene["scene_id"] * 1024 + int(record["state_index"]),
                       4, record["bounds"])
        for record in row.get("hazards", []):
            add_object("hazard", record, 0xFFFFFFFF, 8, record["bounds"], "world_polygons")
        for record in row.get("checkpoints", []):
            add_object("checkpoint", record, 0xFFFFFFFF, 16, record["bounds"], "world_polygons")
        for record in row.get("actors", []):
            bounds = record.get("bounds", row["activation_bounds"])
            add_object("actor", record, 0xFFFFFFFF, 32, bounds)
        region_polygon_count = len(polygons) - region_polygon_first
        neighbour_first = len(points)
        neighbours = [int(value) for value in row.get("neighbour_chunks", [])]
        if any(value <= 0 or value > 0xFFFFFFFF for value in neighbours):
            raise ValueError("neighbour chunk outside u32 range")
        points.extend((value, 0) for value in neighbours)
        regions.append((int(row["chunk_id"]), rect(row["activation_bounds"]),
                        rect(row.get("collision_bounds", row["activation_bounds"])),
                        rect(row["camera_bounds"]), neighbour_first, len(neighbours),
                        object_first, len(objects) - object_first, region_polygon_first,
                        region_polygon_count))

    counts = [len(regions), len(objects), len(polygons), len(points)]
    strides = [REGION_STRIDE, OBJECT_STRIDE, POLYGON_STRIDE, POINT_STRIDE]
    sections = []
    payload = bytearray(b"\0" * HEADER)
    for section, (count, stride) in enumerate(zip(counts, strides)):
        start = align(len(payload))
        payload.extend(b"\0" * (start - len(payload)))
        offset = len(payload)
        if section == 0:
            for row in regions:
                chunk, bounds, collision, camera, nf, nc, of, oc, pf, pc = row
                payload.extend(struct.pack("<I12i6I", chunk, *bounds, *collision, *camera,
                                           nf, nc, of, oc, pf, pc))
        elif section == 1:
            for source, state, kind, flags, bounds, pf, pc in objects:
                payload.extend(struct.pack("<IIHH4i2I", source, state, kind, flags,
                                           *bounds, pf, pc))
        elif section == 2:
            for first, poly_count in polygons:
                payload.extend(struct.pack("<2I", first, poly_count))
        else:
            for x, y in points:
                payload.extend(struct.pack("<2i", x, y))
        if len(payload) - offset != count * stride:
            raise AssertionError(f"metadata section stride mismatch section={section} got={len(payload)-offset} expected={count*stride}")
        sections.append((offset, count, stride))
    total = align(len(payload))
    payload.extend(b"\0" * (total - len(payload)))
    payload[:8] = MAGIC
    struct.pack_into("<II", payload, 8, int(scene["scene_id"]), fnv1a(scene["file"].encode()))
    digest = hashlib.sha256(json.dumps({"scene": scene, "regions": rows}, sort_keys=True).encode()).digest()
    payload[16:48] = digest
    struct.pack_into("<4I", payload, 48, len(regions), total, 0, len(sections))
    for index, (offset, count, stride) in enumerate(sections):
        struct.pack_into("<3I", payload, 64 + index * 12, offset, count, stride)
    return bytes(payload), {"scene_id": scene["scene_id"], "scene_name": scene["scene_name"],
                           "region_count": len(regions), "object_count": len(objects),
                           "polygon_count": len(polygons), "point_count": len(points),
                           "bytes": len(payload), "sha256": hashlib.sha256(payload).hexdigest(),
                           "section_bytes": [count * stride for count, stride in zip(counts, strides)]}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--regions", type=Path, default=ROOT / "data/regions.json")
    parser.add_argument("--output-root", type=Path, default=ROOT / ".hkpsx/world-metadata")
    parser.add_argument("--scene", type=int, action="append")
    args = parser.parse_args()
    report = json.loads(args.regions.read_text())
    wanted = set(args.scene) if args.scene is not None else {s["scene_id"] for s in report["scenes"]}
    args.output_root.mkdir(parents=True, exist_ok=True)
    result = {"format": MAGIC.decode(), "source_regions_sha256": hashlib.sha256(args.regions.read_bytes()).hexdigest(), "banks": []}
    for scene in sorted(report["scenes"], key=lambda value: value["scene_id"]):
        if scene["scene_id"] not in wanted:
            continue
        rows = [row for row in report["regions"] if row["scene_id"] == scene["scene_id"]]
        payload, bank = encode_scene(scene, rows)
        path = args.output_root / f"scene-{scene['scene_id']:03}.hkwm"
        path.write_bytes(payload)
        bank["path"] = str(path)
        result["banks"].append(bank)
    (args.output_root / "report.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
