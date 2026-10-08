#!/usr/bin/env python3
"""Build a deterministic whole-world content and residency matrix.

This joins the verified scene extraction, dependency inventory and authored
room graph.  It deliberately reports source/cooked readiness separately:
serialized bytes and source 4bpp estimates are not PS1 RAM or VRAM costs.
The matrix is therefore useful for planning and prefetch design without
silently admitting an uncooked scene as playable content.
"""
import argparse
import hashlib
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def _sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _by_name(rows):
    result = {}
    for row in rows:
        name = row.get("scene_name") or Path(row["path"]).stem
        if name in result:
            raise ValueError(f"duplicate scene name: {name}")
        result[name] = row
    return result


def _artifact(scene, name, artifact_root=None):
    item = scene.get("outputs", {}).get(name)
    if not item:
        return None
    result = {
        "sha256": item.get("sha256"),
        "compressed_bytes": item.get("bytes"),
        "uncompressed_bytes": item.get("uncompressed_bytes"),
    }
    if artifact_root is not None:
        path = artifact_root / scene["file"] / item["path"]
        if not path.is_file():
            raise ValueError(f"missing extracted artifact: {path}")
        actual = _sha(path)
        if actual != item.get("sha256"):
            raise ValueError(f"artifact hash mismatch: {path}")
        if path.stat().st_size != item.get("bytes"):
            raise ValueError(f"artifact size mismatch: {path}")
        result["verified_path"] = str(path)
        result["verified"] = True
    return result


def _resource_union(rows, key):
    return sorted({value for row in rows for value in row.get(key, [])})


ADMISSION = {"cooked": "isolated_pack_measured", "over_budget": "isolated_pack_over_budget",
             "failed": "isolated_pack_failed", "no_envelope": "no_source_envelope"}


def _cause(error):
    """Collapse an error string to its shared cause: exception type and message without numbers."""
    return re.sub(r"\d+", "N", error).strip()


def _cooking(pack):
    """PS1 cost fields from a tools/cook_scene_pack.py summary, or the source-only defaults."""
    if not pack:
        return {"ps1_ram_bytes": None, "ps1_vram_bytes": None, "admission": "source_matrix_only"}
    measured = pack.get("status") == "cooked"
    return {
        "ps1_ram_bytes": pack.get("resident_bytes") if measured else None,
        "ps1_vram_bytes": (pack["page_bytes"] + pack["palette_bytes"]) if measured else None,
        "admission": ADMISSION.get(pack.get("status"), "isolated_pack_failed"),
        "pack": {key: pack.get(key) for key in (
            "status", "regions", "view_splits", "pages", "palettes", "resident_stored_bytes", "decoder_status",
            "geometry_packet_bound", "unique_breakables", "unique_grass", "source_sha256", "summary_path", "error")},
        "pack_unsupported_errors": len(pack.get("unsupported_cook_errors", [])),
        "pack_actors": len(pack.get("actors", [])),
    }


def build(world, inventory, graph, artifact_root=None, scene_packs=None):
    """Return the matrix, rejecting stale or incomplete source evidence."""
    scene_packs = scene_packs or {}
    coverage = world.get("coverage", {})
    if (world.get("run", {}).get("status") != "verified"
            or not world.get("inputs_unchanged")
            or not coverage.get("all_scenes_processed")
            or coverage.get("failed_scenes")):
        raise ValueError("world import is not verified and failure-free")
    if not inventory.get("complete"):
        raise ValueError("dependency inventory is incomplete")

    scenes = _by_name(world.get("scenes", []))
    rooms = _by_name(inventory.get("rooms", []))
    nodes = _by_name(graph.get("rooms", []))
    if set(scenes) != set(rooms):
        missing = sorted(set(scenes) - set(rooms))
        extra = sorted(set(rooms) - set(scenes))
        raise ValueError(f"scene inventory mismatch: missing={missing[:3]} extra={extra[:3]}")

    edges_by_scene = {}
    for edge in graph.get("edges", []):
        edges_by_scene.setdefault(edge.get("source_scene"), []).append({
            "source_id": edge.get("source_id"),
            "gate_name": edge.get("gate_name"),
            "target_scene": edge.get("target_scene"),
            "entry_point": edge.get("entry_point"),
            "resolution": edge.get("resolution"),
            "component_enabled": edge.get("component_enabled"),
            "active_in_hierarchy": edge.get("active_in_hierarchy"),
            "serialized_flags": edge.get("serialized_flags", {}),
        })
    for values in edges_by_scene.values():
        values.sort(key=lambda value: (str(value.get("source_id")), str(value.get("gate_name"))))

    rows = []
    for name in sorted(scenes, key=lambda value: (scenes[value]["index"], value)):
        source = scenes[name]
        room = rooms[name]
        node = nodes.get(name, {})
        neighbours = sorted(set(node.get("neighbours", [])) - {name})
        window_names = [name] + [value for value in neighbours if value in scenes]
        window_rows = [rooms[value] for value in window_names]
        unsupported = source.get("unsupported", [])
        rows.append({
            "index": source["index"],
            "file": source["file"],
            "scene_name": name,
            "path": source["path"],
            "import_status": source.get("status"),
            "source_fingerprint": source.get("fingerprint"),
            "geometry": _artifact(source, "geometry", artifact_root),
            "components": _artifact(source, "components", artifact_root),
            "counts": source.get("counts", {}),
            "unsupported_count": len(unsupported),
            "unsupported_types": sorted({item.get("type", "unknown") for item in unsupported}),
            "dependency": {
                "texture_count": len(room.get("texture_ids", [])),
                "sprite_count": len(room.get("sprite_ids", [])),
                "animation_count": len(room.get("animation_ids", [])),
                "source_texture_bytes": room.get("source_texture_bytes", 0),
                "source_atlas_4bpp_estimate_bytes": room.get("source_atlas_4bpp_estimate_bytes", 0),
                "unsupported_count": len(room.get("unsupported", [])),
                "scan_failed": bool(room.get("scan_failed")),
                "completeness": room.get("dependency_completeness"),
            },
            "neighbours": neighbours,
            "transitions": edges_by_scene.get(name, []),
            "window": {
                "scene_names": window_names,
                "texture_count": len(_resource_union(window_rows, "texture_ids")),
                "sprite_count": len(_resource_union(window_rows, "sprite_ids")),
                "animation_count": len(_resource_union(window_rows, "animation_ids")),
                "geometry_compressed_bytes": sum((scenes[value].get("outputs", {}).get("geometry", {}).get("bytes") or 0)
                                                  for value in window_names),
                "geometry_uncompressed_bytes": sum((scenes[value].get("outputs", {}).get("geometry", {}).get("uncompressed_bytes") or 0)
                                                    for value in window_names),
                "inventory_incomplete": sorted(value for value in window_names
                                                 if rooms[value].get("scan_failed")
                                                 or rooms[value].get("unresolved_behavior_types")),
            },
            "cooking": {
                # world_import.import_one emits exactly imported/partial/failed,
                # and summarize's `all_geometry_resolved` counts only 'imported'.
                # "complete" was a fourth status no code path produces.
                "source_geometry_ready": source.get("status") == "imported",
                "source_dependencies_ready": not room.get("scan_failed"),
                **_cooking(scene_packs.get(name)),
            },
        })

    by_raw = sorted(rows, key=lambda row: (
        -(row["geometry"] or {}).get("uncompressed_bytes", 0), row["index"]))
    by_window = sorted(rows, key=lambda row: (
        -row["window"]["geometry_uncompressed_bytes"], row["index"]))
    pack_status = {}
    causes = {}
    for row in rows:
        pack_status[row["cooking"]["admission"]] = pack_status.get(row["cooking"]["admission"], 0) + 1
        error = (row["cooking"].get("pack") or {}).get("error")
        if error:
            causes.setdefault(_cause(error), []).append(row["scene_name"])
    measured = [row for row in rows if row["cooking"]["ps1_ram_bytes"] is not None]
    largest_pack = max(measured, key=lambda row: (row["cooking"]["ps1_ram_bytes"], -row["index"])) if measured else None
    return {
        "format": "HKWORLD_PACK_MATRIX01",
        "scope": "Whole-world source content matrix; not PS1 pack admission",
        "world_fingerprint": world.get("fingerprint"),
        "inventory_fingerprint": inventory.get("fingerprint"),
        "scene_count": len(rows),
        "all_scenes_included": len(rows) == coverage.get("catalog_scenes"),
        "source_coverage": {
            "catalog_scenes": coverage.get("catalog_scenes"),
            "processed_scenes": coverage.get("processed_scenes"),
            "imported_scenes": coverage.get("imported_scenes"),
            "partial_scenes": coverage.get("partial_scenes"),
            "failed_scenes": coverage.get("failed_scenes"),
        },
        "peaks": {
            "largest_geometry_uncompressed": {
                "scene_name": by_raw[0]["scene_name"],
                "bytes": (by_raw[0]["geometry"] or {}).get("uncompressed_bytes", 0),
            },
            "largest_current_plus_neighbours_geometry": {
                "scene_name": by_window[0]["scene_name"],
                "scene_names": by_window[0]["window"]["scene_names"],
                "bytes": by_window[0]["window"]["geometry_uncompressed_bytes"],
            },
            "largest_current_plus_neighbours_texture_union": max(
                rows, key=lambda row: (row["window"]["texture_count"], -row["index"]))["window"]["texture_count"],
            "largest_isolated_pack_resident": None if largest_pack is None else {
                "scene_name": largest_pack["scene_name"],
                "bytes": largest_pack["cooking"]["ps1_ram_bytes"],
                "vram_bytes": largest_pack["cooking"]["ps1_vram_bytes"],
            },
        },
        "pack_admission": pack_status,
        "pack_failure_causes": [{"cause": cause, "scenes": len(names), "scene_names": sorted(names)}
                                for cause, names in sorted(causes.items(), key=lambda item: (-len(item[1]), item[0]))],
        "scenes": rows,
        "limitations": [
            "Geometry/components are verified extraction artifacts, not guest scene packs.",
            "Texture and animation counts are source dependency unions; PS1 costs stay unset for scenes without an isolated pack measured by tools/cook_scene_pack.py.",
            "Isolated pack costs cover static scenery, tilemap fills, supported breakables/grass and the scene bank only; actor banks, audio, effects, scripts and neighbour windows are not included.",
            "Serialized neighbours are candidate residency windows; runtime/scripted destinations and conditional exits still require P09/P10 evidence.",
            "No scene is admitted as playable by this report and no source object is discarded.",
        ],
    }


def markdown(matrix):
    lines = ["# Whole-world content matrix", "",
             "This report joins all imported scenes, source dependencies and authored neighbour windows. It is not a PS1 packing or gameplay-completion claim.", "",
             f"- Scenes: {matrix['scene_count']} (all catalogued: {matrix['all_scenes_included']})",
             f"- Largest extracted geometry: {matrix['peaks']['largest_geometry_uncompressed']['scene_name']} ({matrix['peaks']['largest_geometry_uncompressed']['bytes']} bytes uncompressed)",
             f"- Largest current+neighbour geometry window: {matrix['peaks']['largest_current_plus_neighbours_geometry']['scene_name']} ({matrix['peaks']['largest_current_plus_neighbours_geometry']['bytes']} bytes uncompressed)",
             f"- Largest current+neighbour texture union: {matrix['peaks']['largest_current_plus_neighbours_texture_union']} source fragments", "",
             f"- Pack admission: {json.dumps(matrix.get('pack_admission', {}), sort_keys=True)}",
             "| Scene | Import | Neighbours | Geometry | Textures | Sprites | Animations | PS1 RAM | PS1 VRAM | Matrix admission |",
             "| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |"]
    if matrix["peaks"].get("largest_isolated_pack_resident"):
        peak = matrix["peaks"]["largest_isolated_pack_resident"]
        lines.insert(7, f"- Largest isolated pack: {peak['scene_name']} ({peak['bytes']} resident bytes, {peak['vram_bytes']} VRAM bytes)")
    for item in matrix.get("pack_failure_causes", []):
        lines.insert(len(lines) - 2, f"- Pack failure cause ({item['scenes']} scenes): `{item['cause']}`")
    for row in matrix["scenes"]:
        geometry = (row["geometry"] or {}).get("uncompressed_bytes", 0)
        ram = row["cooking"]["ps1_ram_bytes"] if row["cooking"]["ps1_ram_bytes"] is not None else ""
        vram = row["cooking"]["ps1_vram_bytes"] if row["cooking"]["ps1_vram_bytes"] is not None else ""
        lines.append(f"| {row['scene_name']} | {row['import_status']} | {len(row['neighbours'])} | {geometry} | {row['dependency']['texture_count']} | {row['dependency']['sprite_count']} | {row['dependency']['animation_count']} | {ram} | {vram} | {row['cooking']['admission']} |")
    lines.extend(["", "`ps1_ram_bytes`/`ps1_vram_bytes` come only from isolated scene packs; they stay unset for scenes not yet cooked by `tools/cook_scene_pack.py`.", ""])
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--world", type=Path, default=ROOT / ".hkpsx/world-import/report.json")
    parser.add_argument("--inventory", type=Path, default=ROOT / ".hkpsx/room-inventory.json")
    parser.add_argument("--graph", type=Path, default=ROOT / ".hkpsx/world-import/room-graph.json")
    parser.add_argument("--output", type=Path, default=ROOT / ".hkpsx/world-import/pack-matrix.json")
    parser.add_argument("--scene-packs", type=Path, default=ROOT / ".hkpsx/scene-packs",
                        help="root of tools/cook_scene_pack.py outputs; missing root means no PS1 costs")
    args = parser.parse_args()
    world = json.loads(args.world.read_text())
    from cook_scene_pack import read_summaries
    packs = read_summaries(args.scene_packs) if args.scene_packs.is_dir() else {}
    data = build(world, json.loads(args.inventory.read_text()), json.loads(args.graph.read_text()),
                 artifact_root=args.world.parent, scene_packs=packs)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n")
    args.output.with_suffix(".md").write_text(markdown(data))
    print(json.dumps({"scenes": data["scene_count"], "output": str(args.output), "markdown": str(args.output.with_suffix('.md'))}, indent=2))


if __name__ == "__main__":
    main()
