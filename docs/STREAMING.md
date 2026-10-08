# Shared scene residency and source inventories

The current layout contains106 spatial views:86 in Tutorial_01 and20 across
Town0..270/-5..76. These cover supported scenery/collision and existing actors;
Town shops, NPCs, bench scripts and the well connection remain unimplemented.
See STATUS.md for final-CUE validation and the current disc identity.

## Current runtime implementation

Production builds select `scene_gate` residency. One354,032-byte arena and the
same static VRAM slots belong to one scene at a time. Tutorial needs321,956
resident bytes,18 pages and1,068 palettes; Town needs88,200 resident bytes,
6 pages and390 palettes. Their combined410,156 resident bytes need not coexist.
The host packer retains `joint` mode for allocator regression tests; production
guest builds require `scene_gate`.

Startup uploads the shared effects and sound banks and admits Tutorial. At an
authored exit, the guest retires outgoing GPU work, blacks the display at
VBlank, revokes old scene/atlas readiness and invalidates renderer and animation
caches. The outgoing Room borrow ends before replacement. First, a scene-owned HKOCSC01 visibility bank is read, incrementally decoded
and validated in empty geometry scratch, then copied into its84KiB auxiliary
arena. Each destination atlas is read and decoded through the same arena, verified and uploaded; the compact
scene then replaces staging data. Only a fully checked scene, visibility bank and matching page/palette
inventory become visible. Both framebuffers are cleared before
display resumes. Failed admission keeps the display black and reports an error.

The CD IRQ path remains responsible for payload transfers. Main-thread
checkpoints service the controller and RAM-to-SPU ambience during reads,
decoding and uploads. Loading has explicit input ownership: it acknowledges
observed samples without growing the gameplay queue or resetting its history.
Simulation resumes from the last observed input timestamp, with jump/Start
holds preserved. This prevents a long load from becoming gameplay catch-up.
The existing Great Door destination fade/walk follows admission.

Every spatial view inside the active scene remains resident. Region activation
constructs an O(1) immutable `Scene::room(index)` view and refreshes local
render/collision state without CD reads, scene decoding or static atlas uploads.
Original source IDs own persistent breakable/actor state. Eight64x64 4bpp
animation slots fetch frames from resident RAM, preserving cached frames across
same-scene boundaries and resetting ownership on a scene change.

`AtlasDesc.scene_index` and contiguous half-open `SCENE_ATLAS_RANGES` describe
exclusive ownership. Physical scenery pages0..17 and palettes0..1067 are reused
for Town's smaller inventory; the reservation still permits20 pages and1,248
palettes. Host and guest reject incomplete inventories, wrong bases, wrong
owners and over-capacity scenes. Shared Geo, Lifeblood, HUD, dialogue and break
art occupy disjoint persistent strips; see BUDGET.md for exact coordinates.
Scene exits still require a blackout/load. This is not background neighbourhood
prefetch, arbitrary whole-game residency or a physical-console timing claim.

The real decoder is checked against cooked scenes with repeated forward/reverse
replacements and budgets1,31,1024,2048,8192. Runtime tests cover admission,
replacement failures, GPU/blackout ordering and loading-input handoff. Actual-CUE
commands, captures and transfer counters are required separately; native tests
alone do not prove a playable transition. `tools/validate_scene_gates.py` checks
final geometry and visibility arena bytes, matching owners/load counts, and
same-scene transfer stability from those replays.

## Validation status and earlier streaming work

Earlier joint-residency native validation passed both scene files with cursor budgets 1, 8, 31
and 2048. Across 85 Tutorial and 11 Town consecutive region pairs, the same
original streamed frame retains its cache slot with zero misses or upload bytes:
**96 same-scene transitions**. The additional transition between the two scenes
requires a cache reset and is not part of that reuse claim. The actual banks'
page and palette coordinates also pass native bounds checks. Local evidence is
`.hkpsx/resident-scene-review/`, with source/input hashes and a reproducible
harness. The packer separately verifies the exact sequential in-place decode
layout against the pinned SDK and incremental decoder.

The final EXE and actual CUE now pass the long recorded route with zero
boundary waits and zero gameplay CD/static-upload activity across43 region
changes. Both final renderers show the level; all input VBlanks were sampled
and fault counters stay zero. Rendering remains uneven:16 intervals take five
VBlanks, failing the strict four-tick frame budget. Native Town/all-view checks
do not replace an actual natural Town-transition replay, which remains untested.
See STATUS.md for hashes, complete measurements and hardware limitations.

Earlier builds streamed independent HKROOM02 regions through five 256 KiB RAM
arenas and four evicting VRAM banks, with direction-based neighbour prefetch,
concurrent reading/decompression and retained compressed payloads. Those paths
produced real late-destination movement holds. For example, the historical
`.hkpsx/user-6246-overlap/` run recorded 646 boundary-wait ticks (maximum 90),
versus 648 in its earlier baseline. Those measurements and earlier short-route
passes do not describe the new shared-scene runtime. The retained preload/delta
experiments remain useful history, not the active admission policy.

## Cooked inputs and shared scene packaging

`data/regions.json` retains each region's source scene, activation/camera/
collision/interaction bounds, neighbours, original HKROOM02 size/hash, draw and
frame references, breakables, grass, actors, particles, hazards and checkpoints.
Per-region base `scene.json` and `unsupported.json` reports retain extraction
provenance and unresolved behavior. Actor-bank postprocessing maps supported
Crawler art to local clip indices; this does not add arbitrary enemy types.

`host/pack_scenes.py` consumes the complete region snapshot and emits two
HKSCNE01 banks under `data/scenes/`, `data/scene_manifest.rs` and the ignored
`.hkpsx/packed-scenes.json` report. Shared pixel planes, palette deduplication
and global record pools preserve the final cooked data. This packaging step
performs no additional resampling; prior source cooking and user-authorized
static texture deduplication remain separate operations. Local frame indices,
including particle and debris references in world metadata, do not need rebasing.

The manifest binds stored/raw lengths and FNV checksums, RAM offsets, available
in-place scratch, static page/palette bases and direct region-to-scene indices.
Host provenance additionally records SHA-256. `host/pack_rooms.py` and the old
region manifests describe the earlier independent-region format workflow;
current guest packaging uses `pack_scenes.py`.

WORLD.PAK currently has38 entries: two scene payloads, six complete ambience
clips,27 atlas chunks, one Focus bank and two scene visibility banks. The clips are verified and uploaded to SPU through the empty
shared arena before scene admission; they do not remain in scene RAM or require
traversal-time CD reads. The title/retry UI remains available during admission.
Disc images are written only to `~/Downloads/ps1 games/` as the canonical game
pair; retail assets, converted banks, captures and reports remain ignored.

## Whole-game source inventory

The independent host scanner enumerates the Windows build's 501 scenes and
follows serialized SpriteRenderer, material and tk2d animation references. It
records individual sprite fragments alongside their backing retail textures:
two rooms can share an atlas while needing different cropped PS1 images.
TransitionPoint targetScene and entryPoint links establish potential neighbours;
inactive gates retain their original flags. Missing targets, ambiguous gate
names, unresolved frames and reader errors remain explicit. Progression checks,
scripted warps and dynamically spawned dependencies are not inferred from names.

The refreshed scan completed all 501 scenes using the corrected reader. An
actual `--resume` run then verified its reader fingerprint and 1,409 consumed
source-file hashes. All 25,149 scene PlayMaker components parsed strictly,
covering 182,450 states and 608,296 actions across 515 action types, with no FSM
parse or direct-reference read failures. This is serialization coverage, not
execution of those state machines.

The scanner now follows direct FSM references to supported sprite, texture,
material and tk2d assets conservatively, including disabled states/actions. It
recorded 2,119 such references to 82 distinct asset objects. Compared with the
previous scan this added 28 source textures and 60 sprite fragments:

| Refreshed inventory | Count |
| --- | ---: |
| Source textures | 2,615 |
| Catalogued sprite fragments | 21,741 |
| Sprite fragments used by scene dependency sets | 19,995 |
| Referenced tk2d animation libraries | 313 |
| Unresolved animation-frame references | 96 |
| Literal transition edges | 1,119 |
| Edges resolved to an exact destination gate | 914 |

The 96 unresolved animation frames remain alongside each library's valid
frames. They produce repeated animator diagnostics where libraries are shared;
there are 246 animator and 16 tk2d sprite diagnostics across the rooms. The graph
also retains 196 empty targets, seven unresolved scene targets and two targets
whose scene resolves but whose gate does not. No missing destination is invented.

FSM object references to GameObjects, FsmTemplates, audio and other unsupported
types remain recorded, but their dependencies are not recursively expanded.
Dynamic string loads, runtime parameter selection and prefab activation remain
unresolved. Thus a successful full parse does not establish dependency closure.
All whole-scene sprite cooked-cost fields remain unknown.

Both neighbour plans were regenerated from matching inventory/graph hashes.
The audit covers 501 scene windows and 850 window transitions, retaining every
known resource and leaving eviction disabled while admission is unresolved.
The Tutorial_01 / Cliffs_02 / Town window contains 1,416 resources: 1,395 sprite
fragments and 21 material-only texture dependencies, backed by 118 source
atlases. Its 702 current-room resources are ordered before 714 additional
neighbour resources. Every scene window remains unadmitted because exact costs
and runtime dependency closure are not established.

Local evidence is recorded in `.hkpsx/room-inventory-refresh-summary.json`,
`.hkpsx/residency-plan-summary.json` and `.hkpsx/residency-plan-audit.json`.
The old reports have been replaced; future reader/source changes require a fresh
scan rather than accepting a stale resume fingerprint. Reproduce the workflow:

```sh
.venv/bin/python host/room_inventory.py
# Resume only an interrupted scan with matching reader and source fingerprints:
.venv/bin/python host/room_inventory.py --resume
.venv/bin/python host/residency_plan.py --inventory .hkpsx/room-inventory.json --graph .hkpsx/room-graph.json --out .hkpsx/residency-plan.json
.venv/bin/python host/residency_plan.py --inventory .hkpsx/room-inventory.json --graph .hkpsx/room-graph.json --current Tutorial_01 --out .hkpsx/tutorial-residency-plan.json
```

Reports remain in `.hkpsx/room-inventory.json` and `.hkpsx/room-graph.json`.
Resume checks the reader identity and recorded consumed-source hashes, including
external texture streams. A `--limit` scan is an explicit diagnostic subset.
Completed scan coverage does not mean complete runtime dependency closure or
that every object type has been implemented.

The offline planner lists current-room resources first, then additional
neighbour resources, deduplicating stable sprite IDs. For A → B it computes the
new B-plus-neighbours union, retained resources, additions and releases. It now
also emits one directed record per authored gate, preserving source/entry IDs and
serialized conditions with explicit `fetch_before_gate_crossing`,
`keep_during_transition` and `evict_after_target_admission` sets. Incomplete
target windows never receive an executable eviction list. This is a deterministic
dependency proposal, not the guest's exclusive scene-gate admission policy.
Conditional gates may eventually reduce eligible neighbours, but unsupported
conditions are not silently guessed.

The catalog-wide source matrix at
`.hkpsx/world-import/pack-matrix.{json,md}` now joins those dependency sets to
all 501 imported scene artifacts and authored neighbour windows. It records
per-scene geometry/component hashes, source sprite/texture/animation counts and
window peaks, while leaving `ps1_ram_bytes` and `ps1_vram_bytes` unset. The
largest current-plus-neighbour source window is Hive_03_c with four neighbours:
52,062,693 uncompressed geometry bytes and 246 source texture fragments. This
is the input to production crop/palette cooking, not a claim that those source
bytes can be resident on PS1.

Source atlas dimensions, compressed retail bytes and hypothetical 4bpp estimates
are not exact cooked RAM or VRAM costs. Cropping, resampling, palettes, frame
working sets and packing determine those costs. Unknown dependencies and costs
remain unknown; the planner does not drop a neighbour merely to make a budget
fit. Its asset totals also exclude code, stack, actor state, sound and metadata.
Cooked costs for the current bounded regions do not prove whole-room or
whole-game neighbourhood residency.

## Separate neighbour-delta experiment

[DELTA_PROTOTYPE.md](DELTA_PROTOTYPE.md) records the host-only lossless neighbour
payload experiment and its scratch-memory constraints. It is separate from the
source dependency inventory and the current guest's scene-gate loader.

[TEXTURE_DEDUP.md](TEXTURE_DEDUP.md) documents exact within-region texture sharing,
source-reference preservation and the measured static-page savings on the
pre-deduplication region baseline.

[SCENERY_BUDGET.md](SCENERY_BUDGET.md) records the separate, user-authorized
resolution reduction, fixed layout, unchanged animations and decoder checks.
Source-wide inventory cost fields remain unknown: these 98 cooked regions do
not establish complete-scene or whole-game memory requirements.

[MUSIC.md](MUSIC.md) records source-authentic opening ambience and later Dirtmouth
music, complete host-cooked audio profiles and SPU capacity measurements. The
guest loads six complete ambience loops through the unoccupied shared arena at
startup, before publishing any room, then plays from SPU RAM without traversal
CD reads. The eight-entry pack table binds two scenes and six clips. Longer music
transport and natural Cave-to-Town audio transition validation remain unfinished.
