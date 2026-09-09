# Shared scene residency and source inventories

The current build represents 98 spatial region views of supported Tutorial_01
content and the Town entrance. It loads two shared scene banks before gameplay:
86 Tutorial views and 12 Town views. These remain bounded authored subsets, not
complete implementations of either Unity scene or the whole game. The fixed
region layout retains camera, collision and interaction boundaries; it no longer
requires separate CD reads and scenery uploads at each spatial boundary.

Geo coin/rock art is also embedded and uploaded once into disjoint VRAM strips;
its 5,154-byte bank requires no traversal streaming. Scenery palette admission
is capped at 1,248 (1,206 used), allowing an unused CLUT strip to hold Geo art.
Geo sounds occupy a further 13,904 SPU bytes. These are bounded additions for
the current subset, not a whole-game capacity claim.

## Current runtime implementation

The data path is CD → checksummed compressed scene → immutable decoded scene RAM
→ shared static VRAM pages. All current region views are admitted before
simulation starts, including the supported Town gate destination. Animation
frames then transfer from resident scene RAM into eight reusable VRAM slots.
No region traversal requests new scene data or replaces a static scenery bank.
The old wishlist calls currently remain compatibility hooks with no deferred
CD/decode work behind them.

| Resident allocation | Tutorial_01 subset | Town entrance | Total |
| --- | ---: | ---: | ---: |
| Region views | 86 | 12 | 98 |
| Raw scene bytes | 929,692 | 116,756 | 1,046,448 |
| Global texture entries | 1,115 | 145 | 1,260 |
| Immutable palettes | 1,065 | 141 | 1,206 |
| Static 4bpp pages | 18 | 2 | 20 |

The shared RAM arena reserves 1,179,648 bytes. Scenes decode in sequence, largest
first, using the remaining suffix as in-place scratch. Once admitted, the
previous scene's raw prefix stays immutable. The raw total leaves 133,200 bytes
inside this arena; this is not total executable headroom. Code, renderer tables,
actor/effect pools, stacks and other reservations require final linker evidence.

The title-time SDK reader verifies the pack table. The existing CD IRQ wrapper
handles payload transfers and chains to the pinned SDK VBlank/fault handler.
Foreground decoding verifies stored FNV, performs bounded relocation/HLZC work,
verifies raw FNV and runs `SceneValidation`. Each validation work unit checks one
record, descriptor, duplicate-ID comparison, section layout or transition;
the loader yields with a budget of eight units. The synchronous parser uses the
same ordered checks. No partially checked scene becomes a Room view.

The guest uploads all admitted static pages and palettes at startup in bounded
64 KiB slices, then invalidates the GPU texture cache before marking them ready.
Tutorial uses physical pages 0–17 and palettes 0–1064; Town uses pages 18–19 and
palettes 1065–1205. The historical four-bank coordinate helpers now map these
shared ranges; they do not imply four evicting region banks. Exact disjoint
coordinates are in [FORMAT.md](FORMAT.md).

Region activation indexes `REGION_SCENE_LOCAL` and constructs an O(1) immutable
`Scene::room(index)` view. Local draw/frame/clip/edge order is preserved through
reference arrays. Texture and palette IDs are global within a scene. Activation
rebuilds only local draw/visibility/scissor state; global texture templates are
rebuilt only when changing scenes. Gameplay ownership remains tied to original
source IDs, so crossing a residency boundary does not reset actors or effects.

Eight 64x64 4bpp animation slots reserve 16 KiB beside the framebuffers. Knight,
optional nail and up to six actors request at most eight unique textures per
frame, with at most 16 KiB uploaded on misses. Requests are pinned before
replacement, and prior DMA/GPU completion is required before overwriting slots.
Keys cover 2,048 global texture IDs. Same-scene region changes preserve the
cache; changing to a different scene resets it because scene-local global IDs
belong to different namespaces. Static pages and palettes remain resident.

## Validation status and earlier streaming work

Native validation passed both current scene files with cursor budgets 1, 8, 31
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

WORLD.PAK now has eight entries: two scene payloads followed by six complete
ambience clips. The clips are verified and uploaded to SPU through the empty
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
new B-plus-neighbours union, retained resources, additions and releases. Source
gate references remain available for a future scheduler. This is a deterministic
dependency proposal, not the guest's two-scene startup admission policy. Conditional gates
may eventually reduce eligible neighbours, but unsupported conditions are not
silently guessed.

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
source dependency inventory and the current guest's shared-scene startup loader.

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
