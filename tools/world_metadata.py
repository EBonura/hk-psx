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
REGION_STRIDE, OBJECT_STRIDE, POLYGON_STRIDE, POINT_STRIDE, INDEX_STRIDE = 76, 48, 8, 8, 2
BREAKABLES_PER_SCENE = 128
GRASS_PER_SCENE = 1024
MAX_FADE_TICKS = 1023  # ten flag bits
MAX_Q16 = 512 * 65536
KINDS = {"breakable": 1, "grass": 2, "hazard": 3, "checkpoint": 4, "actor": 5, "mask_fade": 6, "remote_mask": 7, "reveal_bindings": 8, "region_statics": 9, "pogo": 10, "camera_lock": 11}
MAX_REVEAL_CONTROLLERS = 16
MAX_SCENE_ACTORS = 32


def damagehero_respawns(hit_type):
    """DamageHero's serialized integer is NOT GlobalEnums.HazardType.

    HeroBox copies the integer unchanged into HeroController.TakeDamage's fourth
    argument. Installed TakeDamage calls StartRecoil for0/1, DieFromHazard for
    2(spikes)/3(acid), logs4(lava), and calls pit recovery for5 only in its normal
    damage branch. The latter two require separate handling, not this bool ABI.
    """
    if type(hit_type) is not int or hit_type not in (0, 1, 2, 3):
        raise ValueError(f'unsupported DamageHero hit type {hit_type}; lava/pit semantics require explicit handling')
    return hit_type in (2, 3)


def persistent_object(record):
    # Full persistent flags survive the session's death/reset operation. Objects
    # without PersistentBoolItem, dontSave, and semiPersistent objects reset.
    return any(not p['dont_save'] and not p['semi_persistent'] for p in record.get('persistence', []))


def bounded(value, bound, name):
    if type(value) is not int or not 0 <= value < bound:
        raise ValueError(f'{name} outside bounded range 0..{bound-1}')
    return value


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
    minimum = 2 if key == "limit_points" else 3
    for polygon in polygons:
        if not minimum <= len(polygon) <= 16:
            raise ValueError(f"{key} polygon outside {minimum}..16 vertices")
        result.append([tuple(q(v) for v in point) for point in polygon])
    return result


def encode_scene(scene, rows):
    objects = []
    polygons = []
    points = []
    indices = []
    regions = []

    def index_list(values, bound, name):
        """Append a u16 list to the index section; return its packed span word."""
        first = len(indices)
        for value in values:
            indices.append(bounded(value, bound, name))
        if first > 0xFFFF or len(values) > 0xFFFF:
            raise ValueError(f"{name} list exceeds the u16 span")
        return first | (len(values) << 16)

    def add_object(kind, record, state, flags, bounds, shape_key=None, extra=(0, 0, 0)):
        shape = polygon_list(record, shape_key) if shape_key else []
        polygon_first = len(polygons)
        for polygon in shape:
            first = len(points)
            points.extend((x, y) for x, y in polygon)
            polygons.append((first, len(polygon)))
        if len(extra) != 3 or any(not -2**31 <= int(v) < 2**31 for v in extra):
            raise ValueError(f"{kind} payload words outside i32: {extra}")
        objects.append((source_id(record["source"]), state, KINDS[kind], flags,
                        rect(bounds), polygon_first, len(shape), tuple(int(v) for v in extra)))

    def fade_record(fade, name):
        ticks = bounded(fade["ticks_60hz"], MAX_FADE_TICKS + 1, f"{name} ticks")
        if not ticks or fade["target_alpha"] != 0 or fade["ease"] != "linear":
            raise ValueError(f"{name} requires a positive-duration linear fade to zero")
        if any(renderer["initial_alpha"] != 1 for renderer in fade["renderers"]):
            raise ValueError(f"{name} requires observed full initial alpha")
        return ticks

    stable_breakables = {}
    stable_grass = {}
    for row in sorted(rows, key=lambda value: value["chunk_id"]):
        object_first = len(objects)
        region_polygon_first = len(polygons)
        draw_count = row.get("draws", 0)
        edge_count = row.get("edges", 0)
        for record in row.get("breakables", []):
            state = bounded(record["state_index"], BREAKABLES_PER_SCENE, "breakable state")
            if record.get("hit_points", 1) != 1:
                raise ValueError("Breakable C# subset requires observed one-hit behavior")
            if record.get("scene_state_count", 0) > BREAKABLES_PER_SCENE:
                raise ValueError("authored Breakable scene state exceeds budget")
            if stable_breakables.setdefault(state, record["source"]) != record["source"]:
                raise ValueError("breakable state aliases different source objects")
            if not 1 <= len(record["hit_polygons"]) <= 8:
                raise ValueError("Breakable polygon count exceeds 1..8 bound")
            fades = [(fade_record(fade, "mask fade"), fade.get("draw_indices", [])) for fade in record.get("mask_fades", [])]
            fade_ticks = max([ticks for ticks, _ in fades], default=0)
            # Play the resident source sample only when the authored table points
            # to that clip. Several poles use the same clip as the tutorial doors.
            options = record.get("audio", {}).get("options", [])
            door_sound = len(options) == 1 and options[0]["name"] == "breakable_wall_hit_1"
            flags = ((1 if persistent_object(record) else 0) | (2 if record.get("semi_persistent") else 0)
                     | (4 if door_sound else 0) | (fade_ticks << 6))
            add_object("breakable", record, scene["scene_id"] * BREAKABLES_PER_SCENE + state, flags,
                       record["box"], "hit_polygons",
                       (index_list(record.get("off_draws", []), draw_count, "breakable off draw"),
                        index_list(record.get("on_draws", []), draw_count, "breakable on draw"),
                        index_list(record.get("edge_indices", []), edge_count, "breakable edge index")))
            for ticks, draws in fades:
                add_object("mask_fade", record, scene["scene_id"] * BREAKABLES_PER_SCENE + state, 0, record["box"],
                           None, (index_list(draws, draw_count, "mask fade draw"), ticks, 0))
        for binding in row.get("remote_mask_bindings", []):
            state = bounded(binding["state_index"], BREAKABLES_PER_SCENE, "mask owner state")
            if stable_breakables.setdefault(state, binding["owner_source"]) != binding["owner_source"]:
                raise ValueError("mask state aliases different source objects")
            ticks = fade_record(binding["fade"], "remote mask fade")
            total = bounded(binding["owner_fade_ticks"], MAX_FADE_TICKS + 1, "mask owner fade ticks")
            if total < ticks:
                raise ValueError("invalid remote mask fade")
            add_object("remote_mask", {"source": binding["owner_source"]},
                       scene["scene_id"] * BREAKABLES_PER_SCENE + state, 0, row["activation_bounds"], None,
                       (index_list(binding["fade"]["draw_indices"], draw_count, "remote mask draw"), ticks, total))
        for record in row.get("grass", []):
            state = bounded(record["state_index"], GRASS_PER_SCENE, "grass state")
            if stable_grass.setdefault(state, record["source"]) != record["source"]:
                raise ValueError("grass state aliases different source objects")
            add_object("grass", record, scene["scene_id"] * GRASS_PER_SCENE + state, 4, record["bounds"], None,
                       (bounded(record["off_draw"], draw_count, "grass off draw"),
                        bounded(record["on_draw"], draw_count, "grass on draw"), 0))
        for record in row.get("hazards", []):
            damage = int(record["damage"])
            if not 1 <= len(record["world_polygons"]) <= 8 or not 0 <= damage < 65536:
                raise ValueError("hazard outside guest bounds")
            add_object("hazard", record, 0xFFFFFFFF, 8, record["bounds"], "world_polygons",
                       (q(record["position"][0]), damage | (int(damagehero_respawns(record["hazard_type"])) << 16), 0))
        for record in row.get("checkpoints", []):
            if record.get("fire_once") or not 1 <= len(record["world_polygons"]) <= 8:
                raise ValueError("checkpoint outside guest bounds")
            add_object("checkpoint", record, 0xFFFFFFFF, 16, record["bounds"], "world_polygons",
                       (q(record["spawn"][0]), q(record["spawn"][1]), 1 if record["respawn_facing_right"] else -1))
        for record in row.get("actors", []):
            bounds = record.get("bounds", row["activation_bounds"])
            # Supported actors carry their SCENE_ACTORS[scene] index (flag 64);
            # unsupported ones stay recorded without a guest spec.
            if "spec_index" in record:
                add_object("actor", record, 0xFFFFFFFF, 32 | 64, bounds, None,
                           (bounded(record["spec_index"], MAX_SCENE_ACTORS, "scene actor index"), 0, 0))
            else:
                add_object("actor", record, 0xFFFFFFFF, 32, bounds)
        bindings = row.get("reveal_mask_bindings", [])
        if bindings:
            controllers = scene.get("reveal_controllers", MAX_REVEAL_CONTROLLERS)
            owned = set()
            pairs = []
            for binding in bindings:
                if binding["draw"] in owned:
                    raise ValueError("multiple reveal owners for draw")
                owned.add(binding["draw"])
                pairs += [bounded(binding["controller"], controllers, "reveal controller"),
                          bounded(binding["draw"], draw_count, "reveal draw")]
            add_object("reveal_bindings", {"source": f"{scene['file']}:0"}, 0xFFFFFFFF, 0,
                       row["activation_bounds"], None, (index_list(pairs, 65536, "reveal binding"), 0, 0))
        for record in row.get("pogo_targets", []):
            # Static NailSlash targets: down-slash only unless flag 1; state is
            # the owning breakable's id (0xFFFFFFFF when none).
            polygons_count = len(record["world_polygons"])
            if not 1 <= polygons_count <= 16:
                raise ValueError("pogo target polygon count outside 1..16")
            owner = record.get("breakable_state_id")
            add_object("pogo", record, 0xFFFFFFFF if owner is None else int(owner),
                       1 if record["horizontal_and_up"] else 0, record["bounds"], "world_polygons")
        for record in row.get("camera_locks", []):
            # CameraLockArea: trigger bounds; one two-point polygon carries the
            # resolved (xmin, ymin), (xmax, ymax) camera-centre limits; flag 1
            # preventLookDown, flag 2 preventLookUp.
            if len(record["limit_points"]) != 1 or len(record["limit_points"][0]) != 2:
                raise ValueError("camera lock needs exactly two limit points")
            add_object("camera_lock", record, 0xFFFFFFFF,
                       (1 if record["prevent_look_down"] else 0) | (2 if record["prevent_look_up"] else 0),
                       record["bounds"], "limit_points")
        statics = row.get("statics")
        if statics is not None:
            # Variant catalogue indices the runtime Region value resolves:
            # door debris, particle bank (-1 none), grass impact (-1 none).
            add_object("region_statics", {"source": f"{scene['file']}:0"}, 0xFFFFFFFF, 0, row["activation_bounds"], None,
                       (bounded(statics["door_debris"], 8, "debris variant"),
                        statics["particle_bank"] if statics["particle_bank"] < 0 else bounded(statics["particle_bank"], 8, "particle bank variant"),
                        statics["grass_impact"] if statics["grass_impact"] < 0 else bounded(statics["grass_impact"], 32, "impact variant")))
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

    counts = [len(regions), len(objects), len(polygons), len(points), len(indices)]
    strides = [REGION_STRIDE, OBJECT_STRIDE, POLYGON_STRIDE, POINT_STRIDE, INDEX_STRIDE]
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
            for source, state, kind, flags, bounds, pf, pc, extra in objects:
                payload.extend(struct.pack("<IIHH4i2I3i", source, state, kind, flags,
                                           *bounds, pf, pc, *extra))
        elif section == 2:
            for first, poly_count in polygons:
                payload.extend(struct.pack("<2I", first, poly_count))
        elif section == 3:
            for x, y in points:
                payload.extend(struct.pack("<2i", x, y))
        else:
            for value in indices:
                payload.extend(struct.pack("<H", value))
        if len(payload) - offset != count * stride:
            raise AssertionError(f"metadata section stride mismatch section={section} got={len(payload)-offset} expected={count*stride}")
        sections.append((offset, count, stride))
    total = align(len(payload))
    payload.extend(b"\0" * (total - len(payload)))
    payload[:8] = MAGIC
    # `raw_fnv` is the identity of the source scene bank when the packer has
    # one available; the standalone cooker falls back to the stable scene-file
    # identity.  The bank checksum is reported separately for decompression.
    scene_identity = int(scene.get("raw_fnv", fnv1a(scene["file"].encode())))
    struct.pack_into("<II", payload, 8, int(scene["scene_id"]), scene_identity)
    digest = hashlib.sha256(json.dumps({"scene": scene, "regions": rows}, sort_keys=True).encode()).digest()
    payload[16:48] = digest
    struct.pack_into("<4I", payload, 48, len(regions), total, 0, len(sections))
    for index, (offset, count, stride) in enumerate(sections):
        struct.pack_into("<3I", payload, 64 + index * 12, offset, count, stride)
    return bytes(payload), {"scene_id": scene["scene_id"], "scene_name": scene["scene_name"],
                           "region_count": len(regions), "object_count": len(objects),
                           "polygon_count": len(polygons), "point_count": len(points), "index_count": len(indices),
                           "bytes": len(payload), "sha256": hashlib.sha256(payload).hexdigest(),
                           "raw_fnv": scene_identity, "bank_fnv": fnv1a(payload),
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
