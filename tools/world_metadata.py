#!/usr/bin/env python3
"""Cook checked HKWMTA01 scene metadata from the canonical region catalogue.

This is deliberately separate from ``host/world.py`` while the guest migration
is staged.  It emits one bank per source scene and never rewrites regions.rs or
the playable disc.
"""
import argparse
import hashlib
import sys
from pathlib import Path as _Path
sys.path.insert(0, str(_Path(__file__).resolve().parents[1] / 'host'))
# The one definition of how many debris variants the catalogue may hold,
# beside the function that produces them. Two copies of this number is what
# made the last build fail after the other copy had already been raised.
from effects import DEBRIS_VARIANT_LIMIT
# And the one definition of the guest actor pool, beside the pass that dedups a
# scene's ActorSpecs and hands this encoder the index each placement takes.
from actors import MAX_SCENE_ACTORS, scene_actor_bank
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
KINDS = {"breakable": 1, "grass": 2, "hazard": 3, "checkpoint": 4, "actor": 5, "mask_fade": 6, "remote_mask": 7, "reveal_bindings": 8, "region_statics": 9, "pogo": 10, "camera_lock": 11, "bench": 12, "shroom": 13, "npc": 14, "gate": 15, "geo_enemy": 16, "reveal_mask": 17, "secret": 18}
MAX_REVEAL_CONTROLLERS = 16


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


def secret_rows(row):
    """The secrets one view's bank carries: those the Knight can reach from it
    (its interaction bounds touch the hit box or the Hero Range) and those whose
    art it draws, which it hides once broken and moves while displaced. Every
    view of a scene lists every secret in the region report; a view that
    neither reaches nor draws one gains nothing from its objects, and Tutorial
    _01's 86 views paid 28 KB of metadata arena for them."""
    bounds = row.get("interaction_bounds", row["activation_bounds"])
    def touches(box):
        return bool(box) and not (box[2] < bounds[0] or box[0] > bounds[2] or box[3] < bounds[1] or box[1] > bounds[3])
    return [record for record in row.get("secrets", [])
            if touches(record["box"]) or touches(record.get("hero_range")) or record.get("off_draws")
            or any(entry.get("draw") is not None for entry in record.get("moving_draws", []))]


def encode_scene(scene, rows):
    # The scene's actor types and, per placement, which of them it places and
    # where it stands. Derived here from the same host pass that emits the
    # linked SCENE_ACTORS list rather than read back out of the region report,
    # so the index in the bank always belongs to the catalogue the guest links.
    _, scene_placements = scene_actor_bank(rows)
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
    for row_index, row in enumerate(sorted(rows, key=lambda value: value["chunk_id"])):
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
        for record in secret_rows(row):
            # A hidden wall or cracked floor (host/secret_breaks.py): an ordinary
            # breakable object, so the broken bitmap, the terrain exclusion and
            # the save item carry it, with flag 8 saying a KIND_SECRET object
            # follows it. That object holds the hit counter's rules in its flags
            # (family 0..2, facing 3..4, hits 5..8, spell 9, hero range 10),
            # the hero range as its bounds, the sagging planks' stage quads as
            # its polygons (stage 1 then stage 2, one quad per moving part)
            # and the moving draws as its first payload list.
            state = bounded(record["state_index"], BREAKABLES_PER_SCENE, "secret state")
            if stable_breakables.setdefault(state, record["source"]) != record["source"]:
                raise ValueError("secret state aliases another source object")
            if not 1 <= len(record["hit_polygons"]) <= 8:
                raise ValueError("secret hit polygon count outside 1..8")
            flags = (1 if persistent_object(record) else 0) | 8
            add_object("breakable", record, scene["scene_id"] * BREAKABLES_PER_SCENE + state, flags,
                       record["box"], "hit_polygons",
                       (index_list(record.get("off_draws", []), draw_count, "secret off draw"),
                        index_list([], draw_count, "secret on draw"),
                        index_list(record.get("edge_indices", []), edge_count, "secret edge index")))
            family = bounded(record["family"], 8, "secret family")
            facing = bounded(record["facing"], 4, "secret facing")
            hits = bounded(record["hits"], 16, "secret hits")
            hero = record.get("hero_range")
            quads = []
            moving = []
            for part, entry in enumerate(record["moving_draws"]):
                if entry.get("draw") is not None:
                    moving += [part, bounded(entry["draw"], draw_count, "secret moving draw")]
            stages = [entry.get("stages") for entry in record["moving_draws"]]
            if any(stages):
                if not all(stages) or any(len(s) != 2 for s in stages):
                    raise ValueError("secret sag needs two stages for every plank")
                quads = [stages[part][stage] for stage in range(2) for part in range(len(stages))]
            origin = record["strike_origin"]
            add_object("secret", {"source": record["source"], "quads": quads},
                       scene["scene_id"] * BREAKABLES_PER_SCENE + state,
                       family | facing << 3 | hits << 5 | (512 if record["spell"] else 0) | (1024 if hero else 0),
                       hero or record["box"], "quads",
                       (index_list(moving, 65536, "secret moving draws"), q(origin[0]), q(origin[1])))
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
            # A supported actor is a placement of one of the scene's linked
            # types: flag 64, the type's SCENE_ACTORS[scene] index in the first
            # payload word, and the placement itself in `state` (the
            # scene-unique source id, which is not the object's own for an
            # object merged in from an additive scene), the other two payload
            # words and the flag bits below. An unsupported one stays a
            # recorded object with no guest spec.
            placed = scene_placements.get(record["source"])
            if placed is not None:
                index, place = placed
                flags = (32 | 64
                         | (1 if place["initial_direction"] > 0 else 0)
                         | (2 if place["random_start_direction"] else 0)
                         | (4 if place["start_alert"] else 0)
                         | (8 if place["start_right"] else 0)
                         | (128 if place.get("fsm_activator") else 0)
                         | bounded(place["rotation_quarter"], 4, "climber rotation quarter") << 8)
                if any(abs(place[axis]) > MAX_Q16 for axis in "xy"):
                    raise ValueError("actor placement outside Q16 world bounds: " + record["source"])
                add_object("actor", record, bounded(place["source_id"], 1 << 32, "actor source id"),
                           flags, bounds, None,
                           (bounded(index, MAX_SCENE_ACTORS, "scene actor index"),
                            place["x"], place["y"]))
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
        for record in row.get("benches", []):
            # RestBench: trigger bounds; payload words seat x, seat y (Q16) and
            # the view's Knight sit clip base.
            base = row.get("bench_clip_base")
            if base is None:
                raise ValueError("bench without cooked sit clips: " + record["source"])
            add_object("bench", record, 0xFFFFFFFF, 0, record["bounds"], None,
                       (q(record["position"][0]), q(record["position"][1]), bounded(base, 65536, "bench clip base")))
        for record in row.get("npcs", []):
            # Talkable NPC: the bounds are npc_control's talk trigger; payload
            # words are the NPC's world x, y (Q16) and the view's clip base for
            # its Idle, Talk Left and Talk Right clips, cooked in that order.
            base = record.get("clip_base")
            if base is None:
                raise ValueError("NPC without cooked clips: " + record["source"])
            add_object("npc", record, 0xFFFFFFFF, 0, record["bounds"], None,
                       (q(record["position"][0]), q(record["position"][1]),
                        bounded(base, 65536, "npc clip base")))
        for record in row.get("shrooms", []):
            # BounceShroom: the trigger box alone. The hero's answer is the one
            # SHROOM_BOUNCE_VELOCITY constant, so there is nothing per instance.
            add_object("shroom", record, 0xFFFFFFFF, 0, record["bounds"])
        for record in scene.get("camera_lock_objects", []) if row_index == 0 else []:
            # CameraLockArea, every one of the scene once, in its first region
            # (host/regions.py postpack_camera_locks): trigger bounds; the first
            # two-point polygon carries the (xmin, ymin), (xmax, ymax)
            # camera-centre limits as ValidateBounds leaves them, any further
            # polygons the trigger's own shape when it is not that one box; flag
            # 1 preventLookDown, flag 2 preventLookUp, flag 4 maxPriority, flag 8
            # a Battle Control's lock (live while the scene's arena is closed);
            # payload word 0 the ticks after the scene loads that a lifetime FSM
            # disables the lock (0: never).
            shape = record["limit_points"]
            if not shape or len(shape[0]) != 2 or any(not 3 <= len(polygon) <= 16 for polygon in shape[1:]):
                raise ValueError("camera lock needs two limit points, then 3..16 point triggers")
            owner = record.get("owner_state_id")
            add_object("camera_lock", record, 0xFFFFFFFF if owner is None else int(owner),
                       (1 if record["prevent_look_down"] else 0) | (2 if record["prevent_look_up"] else 0)
                       | (4 if record.get("max_priority") else 0) | (8 if record.get("battle") else 0),
                       record["bounds"], "limit_points",
                       (bounded(record.get("expires_ticks") or 0, 65536, "camera lock lifetime"), 0, 0))
        statics = row.get("statics")
        if statics is not None:
            # Variant catalogue indices the runtime Region value resolves:
            # door debris, particle bank (-1 none), grass impact (-1 none).
            add_object("region_statics", {"source": f"{scene['file']}:0"}, 0xFFFFFFFF, 0, row["activation_bounds"], None,
                       (bounded(statics["door_debris"], DEBRIS_VARIANT_LIMIT, "debris variant"),
                        statics["particle_bank"] if statics["particle_bank"] < 0 else bounded(statics["particle_bank"], 8, "particle bank variant"),
                        statics["grass_impact"] if statics["grass_impact"] < 0 else bounded(statics["grass_impact"], 32, "impact variant")))
        if row_index == 0:
            # A gate belongs to its scene, not to one region: the guest's
            # transition test has always searched every gate of the scene it
            # stands in, so they all ride in the first region and one bounded
            # object scan answers the set the linked table used to hold.
            for gate in scene.get("resolved_gates", []):
                region = bounded(gate["target_region"], 65536, "gate target region")
                target = bounded(gate["target_scene"], 256, "gate target scene")
                side = bounded(gate["side"], 16, "gate side")
                add_object("gate", gate, region | (target << 16) | (side << 24),
                           bounded(gate["delay_ticks"], 65536, "gate collider delay"),
                           gate["bounds"], None,
                           (q(gate["spawn"][0]), q(gate["spawn"][1]), int(gate["entry_vy"])))
            # Geo payouts are keyed by enemy source id across the whole scene,
            # never by view, so they sit beside the gates in the first region.
            # The object's bounds are unread; `flags` bit 1 is `megaFlingGeo`,
            # which is all that is left of the six constant fling words.
            for enemy in scene.get("geo_enemies", []):
                drops = [bounded(value, 65536, "geo drop count") for value in enemy["drops"]]
                add_object("geo_enemy", enemy, drops[0] | (drops[1] << 16),
                           1 if enemy["mega"] else 0, row["activation_bounds"], None,
                           (q(enemy["effect_origin"]["x"]), q(enemy["effect_origin"]["y"]), drops[2]))
            # The scene's reveal-mask controllers, in the order the per-region
            # KIND_REVEAL_BINDINGS pairs index them. `state` is that index, the
            # object's bounds are the trigger's own AABB so the guest's per-tick
            # box test needs no polygon walk, and the polygon itself is the
            # exact shape the hero collider is measured against on a hit.
            seen_reveal = set()
            for index, record in enumerate(scene.get("reveal_mask_controllers", [])):
                source = bounded(record["source_id"], 1 << 32, "reveal source ID")
                if source in seen_reveal or record["controller"] != index:
                    raise ValueError("reveal source aliases or noncanonical index")
                seen_reveal.add(source)
                opacity = record["initial_opacity"]
                if type(opacity) is not int or opacity not in (0, 128):
                    raise ValueError("invalid reveal initial opacity")
                one_way = record.get("one_way", False)
                if type(one_way) is not bool:
                    raise ValueError("invalid reveal one-way flag")
                # An authored secret mask: covered on load, uncovered for good on
                # the first hero entry, because its FSM declares no COVER event.
                if one_way and opacity != 128:
                    raise ValueError("one-way reveal must start covered")
                ticks = bounded(record["fade_ticks"], 601, "reveal fade ticks")
                if not ticks:
                    raise ValueError("reveal duration must be positive")
                # Flags: 1 one-way, 2 starts covered, 4 chimes (`Play Sound`),
                # 8 driven by a secret's break (no trigger of its own; payload
                # word 1 is that secret's state), 16 replays its fade whenever
                # the scene loads with that secret broken, 32 saved under
                # payload word 2's slot. Several trigger boxes are one trigger:
                # the bounds are their union and the guest ORs the polygons.
                triggers = record.get("triggers") or ([record["trigger"]] if record.get("trigger") else [])
                driver = record.get("driver_state")
                if (driver is None) == (not triggers):
                    raise ValueError("a reveal controller needs exactly one of a trigger and a driver")
                if len(triggers) > 8:
                    raise ValueError("reveal controller trigger count outside 0..8")
                corners = [p for trigger in triggers for p in trigger]
                box = ([min(p[0] for p in corners), min(p[1] for p in corners),
                        max(p[0] for p in corners), max(p[1] for p in corners)] if corners else row["activation_bounds"])
                saved = bool(record.get("saved"))
                if saved and not one_way:
                    raise ValueError("only a one-way reveal is saved")
                flags = ((1 if one_way else 0) | (2 if opacity == 128 else 0) | (4 if record.get("plays_sound") else 0)
                         | (8 if driver is not None else 0) | (16 if record.get("replay_on_load") else 0) | (32 if saved else 0))
                add_object("reveal_mask", {"source": f"{scene['file']}:{source}", "triggers": triggers},
                           bounded(index, MAX_REVEAL_CONTROLLERS, "reveal controller"), flags,
                           box, "triggers",
                           (ticks, -1 if driver is None else bounded(driver, BREAKABLES_PER_SCENE, "reveal driver"),
                            bounded(record["persist_slot"], 16, "reveal persist slot") if saved else -1))
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
