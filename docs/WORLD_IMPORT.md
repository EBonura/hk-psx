# Whole-world import and system development

Development now imports all Windows BuildSettings scenes before expanding shared
systems. Finishing an enemy, effect or room script is no longer a prerequisite
for extracting that room's surroundings. The source catalog includes menus,
cinematics, alternate versions and boss scenes; names do not silently exclude
content. Imported source data is distinct from PS1 packaging and playable coverage.

```sh
python3 tools/doctor.py
.venv/bin/python host/world_import.py --jobs 3
# Resume only matching source/code fingerprints and intact output hashes:
.venv/bin/python host/world_import.py --jobs 3 --resume
python3 tools/world_import_report.py
.venv/bin/python tools/world_transitions.py
python3 tools/world_coverage.py
.venv/bin/python tools/compare_world_regions.py
.venv/bin/python tools/world_catalog.py
.venv/bin/python tools/completion_catalog.py
.venv/bin/python tools/managed_field_usage.py
.venv/bin/python tools/world_progression.py
.venv/bin/python tools/world_identity.py
.venv/bin/python tools/world_metadata.py
```

Outputs stay in ignored `.hkpsx/world-import/`; no guest build, canonical cooked
files or disc is changed. `--limit N` is a diagnostic subset. Each scene uses a
fresh isolated worker, and concurrency is bounded to at most four processes.
`--scene-timeout` and `--worker-retries` supervise only that scene. Attempt files
retain exact PID, exit/signal and retry evidence. An interrupted run stops owned
children and leaves active/queued scenes pending rather than inventing failures.

Each scene has compressed geometry and component records, preserving original
source IDs, transforms, instance flags, scripts and serialized parameters.
Geometry includes unculled sprite references and world geometry, terrain segments,
mesh data, tilemap cell unions, collider metadata, cameras and exits. Circle
colliders use exact affine parametric boundaries, preserving transformed ellipses
without polygon approximation. Runtime-generated tk2d MeshFilters reference the
canonical extracted tk2d sprite geometry rather than duplicating it. Unity Plane
and Quad meshes resolve from the shipped Windows `Resources/unity default
resources` file. Particle systems/renderers and TrailRenderers retain serialized
parameters, transforms and resource dependencies. Null tk2d collection references
with a unique animator-library binding are resolved without guessing. Unsupported
geometry and read errors remain explicit. Binary/nonfinite serialized values use
lossless tagged JSON. Dynamic prefab closure and runtime text glyph shaping remain
separate systems.

The source dependency inventory can be reused only when its catalog and recorded
input content hashes match. It supplies sprite/animation/texture dependencies;
source atlas size is never treated as a PS1 cost. Whole-world texture quantization,
packing, deduplication and measured scene budgets remain a separate pass.

`catalog.json` retains the source build indices. `report.json` distinguishes
processed scenes, geometry imports, partial imports and failures. Its component
inventory counts types and affected scenes, with gameplay support explicitly not
evaluated. `room-graph.json` maps literal serialized scene/gate names. Runtime
`scripted-transitions.json` inventories every serialized PlayMaker
`BeginSceneTransition` and `LoadLevel` action, including disabled actions,
literal targets, variable bindings and declared initial values. Variable values,
FSM reachability, progression conditions and non-PlayMaker loads remain
unresolved; a mapped edge or initial candidate is not a verified traversal. The
report also traces local PlayerData/static/constant reads and same-object
`SetFsmString` overrides. Runtime-injected values remain explicit with an owner.

Every run hashes the Windows inputs and importer code. Resume verifies output
hashes and retries failed scene imports. A final source/code check must pass
before the report records unchanged inputs. Scene read failures do not stop
other imports and do not count as successfully imported scenes.

`compare_world_regions.py` binds every canonical Tutorial/Town row through
`.hkpsx/regions-provenance.json`, verifies final pack hashes and sizes, and checks
cooked scenery/tilemap quads and terrain against the matching whole-world source
geometry. It supports both Tutorial's post-packed `base_path` rows and Town's 20
provenance-only rows; a missing `base_path` never skips a region. Its ignored
report is `tutorial-town-comparison.json`.

`world-catalog.json` assigns all 501 source scenes an evidence-backed role
without excluding any scene. It retains all 1,119 TransitionPoints with exact
serialized direction, delay, fade, entry and trigger geometry; groups every
component type under implementation owners; and catalogs serialized PlayerData
producers and consumers. `completion-catalog.json` extracts the installed
`PlayerData.CountGameCompletion` and `CountCharms` CIL into a hash-bound rule
checklist. Installed initialization and Godhome override methods prove health,
SOUL, spell and nail caps and therefore the exact maximum of 112.
`managed-field-usage.json` joins serialized PlayMaker counts with every direct
managed reader, writer and address use for all 95 contributing fields.

`world-progression.json` attaches same-state and FSM-reachable PlayerData
condition/effect candidates to all scripted loads, records same-object evidence
for every TransitionPoint and flags serialized one-way scene pairs. It selects
source identities for each edge/condition category as later reference cases.
FSM graph reachability remains candidate evidence until P09/P13/P14 execute and
trace the actual branch.

`world-identities.sqlite` is the P04 collision-checked source identity registry.
It uses scene paths rather than BuildSettings indices, retains compatibility
aliases for current sparse Breakable/grass IDs and assigns explicit state
ownership. See `docs/STATE_IDENTITY.md`; the database and report stay ignored.

`world_metadata.py` is the first P05 scene-bank cooker. It writes ignored
`HKWMTA01` banks under `.hkpsx/world-metadata/`, one per cooked source scene,
and binds each bank to the current `regions.json` hash. The scene pack writes
its compressed admission copies under `.hkpsx/world-metadata-packed/`, records
the compressed/raw checksums and assigns metadata chunks after coverage. The
wire reader is `shared/hk-format/src/world_meta.rs`; it checks fixed strides,
aligned sections, zero padding, global-neighbour IDs, object spans and polygon
points before publishing borrowed views. `game/src/disc.rs` now admits each
bank in the unused aligned tail of the scene arena and cross-checks all global
region owners; `host/build_guest.py --no-disc` can validate this guest without
touching the sole playable BIN/CUE pair. The chunk layout still needs a pinned
guest differential run before replacing that disc.

`tools/cook_scene_pack.py` is the first P06 production-cooking slice. It runs
the canonical region cooker, similarity pass, reveal-mask binding and the
`HKSCNE`/`HKWMTA01` scene-bank packer against any catalog scene, writing only
under the ignored `.hkpsx/scene-packs/<scene>/` root; a process-wide write guard
aborts the cook if any helper touches `data/`, canonical `.hkpsx/` reports or
the disc. The activation envelope is the union of active tk2d tilemaps, gate
triggers and gate spawn points, padded 1/5/1/1 units; camera-lock volumes are
excluded because authors size them far beyond the map, and camera bounds follow
the tilemap like the original camera clamp. Views use the canonical 24x16
stepping and halve on the cooker's budget errors. Each `summary.json` records
resident/stored bytes, pages, palettes, decoder status, unsupported cook errors
and the actor inventory; `tools/world_pack_matrix.py --scene-packs` merges them
into `ps1_ram_bytes`/`ps1_vram_bytes` plus a per-scene admission label that
still never claims playability. Costs cover static scenery, tilemap fills,
supported breakables/grass and the scene bank only: actor banks, audio, effects,
scripts and neighbour windows are not included.

The next passes are:

1. Resolve common geometry/dependency gaps across the world, and cook shared art
   with per-scene inventories at existing quality settings.
2. Replace the fixed two-scene guest metadata tables with bounded scene-loaded
   metadata. Measure the actual cooked costs before choosing arena sizes.
3. Implement shared gameplay families across the imported catalog: enemies,
   interactions, breakables, NPCs, benches, abilities, progression and bosses.
4. Validate each system against controlled original-game traces and representative
   guest routes, then verify long progression paths.

The current Tutorial/Town disc remains the playtest build. Host extraction does
not add playable areas, establish original behavior parity or prove RAM/VRAM fit.
