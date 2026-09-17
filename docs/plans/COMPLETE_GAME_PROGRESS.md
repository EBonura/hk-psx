# Complete-game execution ledger

This ledger tracks implementation of `COMPLETE_GAME_SOL.md`. A package is
validated only when its acceptance criteria have current evidence. Source
extraction, cooking, runtime binding and playable validation remain separate.

## Current authority

- Source branch: `main`; implementation began from commit `8337d68`.
- Windows source: CrossOver Steam installation, Unity `6000.0.61f1`.
- Playable disc: build 76; King's Pass, Dirtmouth and seventeen Forgotten
  Crossroads scenes (01 to 10, 12 to 14, 16, 18, 19, 21) connected through the
  authored gates: 19 scenes, 329 views, 200 disc chunks.
- Disc policy: the user authorized rebuilds on 2026-09-16; every build now
  replays the route tapes in `tools/tapes` and fails on any fault.
- Current packages: P05-P08, admit checked metadata, build the catalog-wide
  source matrix, remove scene-multiplied Geo state, and correct the first
  cross-layer tile-occlusion failure before adding streaming admission. The
  playable disc is still preserved.

## Milestones

| Milestone | Packages | State |
| --- | --- | --- |
| M0: recover import | P00-P03 | validated |
| M1: scalable content | P04-P08 | in progress |
| M2: connected world | P09-P12 | pending |
| M3: persistent adventure | P13-P18 | pending |
| M4: full combat roster | P19-P22 | pending |
| M5: complete content | P23-P27 | pending |
| M6: release proof | P28-P31 | pending |

## Package state

| Package | State | Current evidence / next action |
| --- | --- | --- |
| P00 | validated | First-read documents, Git, doctor, source version, disc policy and live-process state rechecked. No importer/emulator process was active. |
| P01 | validated | All 501 BuildSettings scenes completed with source/code verification and zero scene failures. The latest run isolated three signal-11 workers and retried only those scenes successfully in fresh processes; exact attempt records are retained. |
| P02 | validated | Every scene has an evidence-backed role and remains included. All 1,119 TransitionPoints and 318 scripted loads retain geometry, flags, destination dataflow and progression-condition status with downstream owners. All 95 completion fields join serialized access to exact managed readers/writers; installed cap CIL proves the 112 maximum. Fifteen source reference cases cover every observed edge/condition category. |
| P03 | validated | All reusable observed geometry families are extracted across 501 scenes. 463 scenes are geometry-complete; the other 38 retain only 757 TextMeshPro and four legacy TextMesh runtime glyph-shaping cases as explicit renderer/cooking work. All 106 canonical Tutorial/Town regions pass source-geometry and terrain comparison, including 20 Town rows resolved through per-region provenance without `base_path`. |
| P04 | validated | Source-order-independent IDs cover the complete imported catalog with collision checks and zero unresolved assets. Current sparse Breakable/grass IDs have stable aliases. The bounded central state API separates global, mode and active-scene ownership and passes reset, reload, idempotence and exact-size tests. |
| P05 | in progress | Build 81 moved grass into the bank (headroom 252,584) and added the `crossroads-exit` route through Crossroads_02 and back; build 80 moved breakables, mask fades and remote masks into the loadable bank behind checked views and fixed the guest state tables that were sized for three scenes; build 79 moved hazards and checkpoints (`.data` -11,536 bytes, headroom 223,444). The metadata relocation now runs on the actual disc: build 72 exposed and fixed a 1-based/0-based neighbour comparison, the bank being overwritten in the arena tail by later in-place decodes (it is admitted last and decoded inside the tail, and the host gate-load proof stages it the same way), and per-tick re-parsing that overflowed the input queue. Baked per-particle tracks (449 KB of permanent RAM) were replaced by the guest's proven scalar path, and the world metadata manifest follows scene-manifest order. Linked headroom is 401,256 bytes with three scenes. Migration of the remaining static region payloads (grass, breakables, hazards, actors, emitters) to the bank remains. |
| P06 | in progress | `tools/world_pack_matrix.py` joins all 501 verified scene artifacts, dependency inventories and current+neighbor windows, with deterministic hashes and source-cost peaks. `tools/cook_scene_pack.py` now cooks any catalog scene into an isolated, write-guarded pack with the canonical cooker/similarity/scene-bank path, and the matrix merges its measured PS1 costs per scene. The whole-catalog batch is complete: 457 of 501 scenes cook with a passing native decoder, 41 have no source envelope (cinematics, menus, boss-defeated and preload helpers) and three exceed the 128-view HKSCNE room limit. Eleven shared abort causes became explicit recorded exceptions or lossless transformations. Shared art canonical mappings across scenes, neighbour-window costs and production admission remain. |
| P07 | in progress | Persistent Geo state is now a flat 256-entry source-state pool keyed by scene and authored rock slot, covering the verified 207 GeoRock instances without a `[scene][32]` allocation. Version-2 host plans preserve all 850 directed gates, and the bounded room-residency runtime can safely evict only completed slots outside a keep-set. Native residency/Geo tests pass and the no-disc link is hazard-free with 16,232 bytes before the stack floor. Predictive CD residency and guest streaming admission are still pending. |
| P13 | in progress | Shared persistent stores now have a versioned `HKSV` fixed-buffer snapshot codec for global and mode owners, with strict length, ordering, checksum and capacity validation; active-scene flags reset on load. Memory-card I/O and actual quit/reload routes remain pending. |
| P08 | in progress | Tile-coverage event chains now separate back and front owners, preserving front masks through the complete back pass. The first regression and full hk-format coverage suite pass. Material-family completion, black-fill source coverage, dynamic mask invalidation, actor/effect cache expansion and representative visual replay remain. |
| P09 | in progress | Crossroads_01 is scene 2 of a single host scene catalog (`host/quality.py SCENE_TABLE`) that every cooker, generated table and the guest scene count derive from. The authored Town `bot1` gate drops the Knight into Crossroads_01 `top1` and the return gates resolve to Town `bot1`; the `town-well` route tape replays on the final CUE with three scene loads and the Knight walking inside Crossroads. Build 75 adds Crossroads_02 and Crossroads_07 through `GRID_SCENE_LAYOUTS` (one catalog entry per scene); their gates are generated from source and the linked headroom is 376,680 bytes. The `crossroads-floor` route walks the Crossroads_01 floor to the right wall; reaching `right1` needs an authored jump route. Build 78 gives top-side gate entries the source's -12 units/s initial drop (`Gate.entry_vy`, verified for the well; assumed for other top entries). The delayed top-gate collider, source camera locks, the Crossroads enemies beyond the Crawler and return replays remain. |
| P10-P12, P14-P31 | pending | Follow dependency order in the master plan. |

## P01 evidence

### Finding

The previous run completed 249 source scenes. When the parent process received
the requested termination, its `ProcessPoolExecutor` invalidated every submitted
future. The parent then wrote the same generic pool exception as 196 individual
scene failures. This does not identify `Mines_02` or any later scene as corrupt.

### Implemented supervision

- Submit at most `--jobs` isolated one-scene processes at a time.
- Record each worker PID, attempt, start/end time, exit code and signal.
- Bind result files to the exact worker attempt to reject stale output.
- Retry only the isolated timed-out/crashed scene.
- Preserve prior attempt records across resume.
- On SIGINT/SIGTERM, stop owned children, retain completed checkpoints and leave
  active/queued scenes pending instead of recording scene failures.

### Checks and complete-run evidence

- Targeted world import/geometry unit suite: 23 tests passed after the final
  attempt-number and child-termination corrections.
- Three-scene diagnostic with two workers: 3/3 processed, zero worker failures.
- Twenty-scene interruption diagnostic after correction: 12 completed, two
  active workers marked interrupted, zero scene failures and the correct total
  of eight unfinished scenes. The parent exited with the deliberate status 130.
- Latest complete import: 501/501 processed, 463 imported, 38 partial, zero failed;
  source inputs and importer code remained unchanged through final verification.
- Supervision recorded 504 attempts. The first attempts for `Fungus2_14`,
  `Fungus2_15` and `Fungus2_34` ended on signal 11; only those scenes were
  retried and all three completed in fresh workers.
- Extracted source inventory: 414,255 SpriteRenderers, 12,745 tk2d sprites,
  58,852 colliders, 124,947 terrain edges, 18,011 meshes, 3,928 tilemap fills,
  1,696 camera locks and 1,119 transition points.
- Room graph: 914 edges map to an exact destination gate; 196 have an empty
  serialized target, seven name an unresolved scene and two map to a scene but
  not a destination gate. Runtime/PlayMaker overrides remain P02 work.
- Verified dependency inventory remains complete: 1,409 source hashes, 2,615
  textures, 21,741 sprites and 313 animation libraries.
- Targeted importer, geometry and coverage tests: 28 passed.

## P02-P03 current evidence

- `tools/world_transitions.py` parsed all 307 `BeginSceneTransition` and 11
  `LoadLevel` actions with zero parse errors. Literal and variable-bound fields
  remain distinct; declared initial values are candidates, not runtime claims.
- Dynamic destination dataflow accounts for all 286 actions: 130 read
  PlayerData/static/constants, 89 retain serialized defaults, three have exact
  same-object `SetFsmString` branch overrides, and 64 are runtime/API injected.
  The last group is 63 Godhome boss destinations plus one finale continuation
  and is assigned to P09/P22 instead of receiving invented scene names.
- Dynamic entry-gate dataflow records 63 unresolved runtime values, 63 storage
  reads and 87 serialized defaults. Eighty-seven source/default combinations
  produce 91 exact gate matches; two additional Tram defaults remain explicit
  alongside their serialized branch overrides.
- `tools/world_catalog.py` classifies all 501 source scenes with evidence: 402
  gameplay, 63 boss arenas, 13 dream variants, eight cinematics, seven
  transition/bootstrap helpers, three additive boss-defeated variants, two
  credits scenes and one each for menu, completion and shared Knight content.
  No source scene is excluded or left without a primary role.
- All 1,119 TransitionPoints have resolved trigger geometry and retain exact
  serialized door, direction, delay, fade, hazard, offset, snapshot and initial
  activation fields. The catalog covers all 468 component types with owning
  packages and 17,419 serialized PlayerData actions across 815 resolved keys;
  1,328 runtime-variable key actions remain explicit under P13/P14.
- `tools/completion_catalog.py` extracts and hashes the installed
  `PlayerData.CountGameCompletion` and `CountCharms` CIL: 44 boolean/group rules,
  six integer rules, four nested Godhome rules, the exact 40-charm count and the
  33/66/99 SOUL-vessel cases. `SetupNewPlayerData` proves caps 9 and 99;
  `AddGGPlayerDataOverrides` proves three spell caps of 2, four nail upgrades and
  exact cap copying. The installed completion maximum is therefore 112.
- `tools/managed_field_usage.py` indexes all 95 contributing fields across the
  installed Assembly-CSharp CIL. Thirty-three methods contain 426 direct field
  operands: every field has a managed writer, 91 have readers and four nested
  structures have address users. Direct managed callers and serialized
  PlayMaker producer/consumer counts are retained with assembly/method hashes.
- `tools/world_progression.py` attaches progression evidence to all 318 scripted
  loads: 18 have same-state conditions, 176 have broader FSM-reachable
  candidates and 124 have no serialized PlayerData condition. It preserves 21
  same-state postconditions, same-object activation candidates for 204 of 1,119
  TransitionPoints, 25 one-way serialized candidates and 15 exact source
  reference cases. Every unresolved condition is assigned to P09/P13/P14.
- The full Python suite passes 389 tests after the completed P02 catalogs.
- Exact affine boundaries cover all 5,274 CircleCollider2D instances across 292
  scenes. These are dynamic/trigger objects rather than static layer-8 terrain.
- All 12,729 runtime-generated tk2d MeshFilters now reference their extracted
  canonical tk2d geometry rather than appearing as duplicate missing meshes.
- The source resolver retains serialized external paths and maps Unity's shipped
  `Library/unity default resources` pseudo-path to the Windows `Resources`
  file. All 477 Plane and 100 Quad instances now use their exact source meshes.
- All 28,814 ParticleSystem and 28,814 ParticleSystemRenderer payloads are
  retained with transforms, modules, renderer settings, material/mesh
  dependencies and event/sub-emitter references.
- All 831 TrailRenderers retain their serialized curves, timing, materials,
  sorting and emission flags. Sixteen null tk2d collection references resolve
  unambiguously through their animator libraries, including Broken Vessel/Lost
  Kin effects. TextMeshPro submeshes bind to generators through ancestry and
  the four legacy TextMesh renderers are classified explicitly.
- The current verified import completed 501/501 scenes: 463 imported, 38 partial,
  zero failed. It contains 4,505 world meshes, 12,745 tk2d generated-mesh aliases
  and zero extraction errors.
- `tools/compare_world_regions.py` verifies final pack sizes/hashes through
  `.hkpsx/regions-provenance.json`, resolves all 86 Tutorial base-pack rows and
  all 20 Town provenance-only rows, and compares 22,520 cooked draw occurrences
  plus 3,425 terrain-edge occurrences. All source geometry matches exactly after
  applying the cooker's documented near-axis terrain snap.
- Remaining explicit cases are 757 TextMeshPro and four legacy TextMesh runtime
  glyph meshes in 38 scenes. They belong to text shaping/rendering and remain
  visible in the coverage ledger rather than masquerading as missing scenery.
- Representative checks cover TrailRenderer-heavy `Room_Fungus_Shaman`, all four
  animator-resolved Broken Vessel/Lost Kin effect scenes, Tutorial/Town sprite,
  tilemap and terrain cooks, and legacy/TMP text owners. The full Python suite
  passes 370 tests after the final import and comparison changes.

## P04 evidence

- `tools/world_identity.py` builds a disk-backed collision-checked registry for
  501 scenes, 1,997,203 components, 1,119 gates, 815 PlayerData keys and 7,004
  assets. All source references resolve. Scene/component IDs use source scene
  paths and PathIDs instead of `levelN`/BuildSettings order; external art IDs are
  explicitly source-version scoped.
- The first relocation retains 37 current Breakable `scene×128` aliases and 39
  grass `scene×1024` aliases. Duplicate region copies cannot claim different
  owners. Spawned-instance identity requires prefab, authored owner and stable
  slot IDs.
- `shared/hk-sim/src/persistent.rs` provides sorted fixed-capacity global/mode
  stores, checked central mutations, idempotent rewards and a bounded active
  scene owner. Reset fixtures cover scene leave/return, bench, death, quit/load,
  dream return, challenge/mode reset and snapshot reload.
- Exact current-source capacities cost 84,536 bytes for all 5,283 global keys,
  8,200 bytes for 512 mode keys and 1,176 bytes for 1,024 active flags plus 64
  timers: 93,912 bytes total, with no per-scene multiplication.
- The complete Python suite passes 393 tests and the complete hk-sim suite passes
  228 tests after P04. The disc remains build 71; this host/state package did not
  recook or replace it.

## P05 first slice evidence

- `tools/world_metadata.py` emits versioned `HKWMTA01` banks from the canonical
  region catalogue without rewriting `data/regions.rs` or the disc. The source
  catalogue hash is recorded in the ignored metadata report.
- `shared/hk-format/src/world_meta.rs` provides checked little-endian views with
  fixed strides, checked arithmetic, zero-padding checks, global neighbour IDs,
  stable state IDs, bounded object/polygon spans and allocation-free iterators.
  It rejects wrong magic/stride, trailing bytes, out-of-range spans and invalid
  polygon point references before publishing a view.
- Current measured banks are Tutorial: 86 regions, 452 objects, 112 polygons,
  1,026 points, 32,072 bytes; Town: 20 regions, five objects, five polygons,
  118 points, 2,844 bytes. `check_world_meta` parses both generated banks.
- The guest `State` edge cache now uses an explicit world generation and global
  region ID and borrowed room-view key. Scene admission, reset and retry revoke
  it before any arena reuse; the view key disambiguates equal-sized room
  payloads while generation protects the reusable arena.
- `host/pack_scenes.py` now emits the metadata banks into the ignored pack
  report, assigns them after scene coverage, and binds raw/stored checksums plus
  the source fingerprint into `WORLD_META_MANIFEST`. `game/src/disc.rs` stages
  those banks in a dedicated bounded buffer, validates the wire view and checks
  every global region owner before publishing the scene. The build 71 link
  report remains bound to the unchanged playable EXE/BIN/CUE; no disc
  replacement is claimed by this slice.

### Next concrete action

Run the pinned guest build with the new metadata chunks and compare Tutorial/Town
metadata against geometry and coverage traces before replacing the playable
disc. Then move the same admission contract to Geo, Lifeblood, dialogue, Great
Door, render-budget and disc mapping tables. In parallel, use the catalog-wide
pack matrix to parameterize production cooking and identify the first scenes
whose exact cropped/paletted PS1 costs can be admitted. Runtime text shaping
remains with the renderer/UI packages.

## P06 first cooking slice evidence

- `tools/cook_scene_pack.py` replaces the ad hoc `.hkpsx/town67` and
  `.hkpsx/crossroads70` probe scripts with a tracked tool. It reuses `cook(...,
  write_shared=False, stage_for_similarity=True)`, `postpack_checkpoints`,
  `postpack_masks`, `bind_regions`, `postpack_similarity(root=...)` and
  `pack_scenes(residency='scene_gate', generate_world=False)`, assigns grass
  state indices exactly like the canonical cook, and writes only under
  `.hkpsx/scene-packs/<scene>/`. An audit-hook write guard aborts on any other
  path; canonical `data/regions.json`, `data/room.hk`, `.hkpsx/packed-scenes.json`
  and `.hkpsx/regions-provenance.json` kept their previous timestamps.
- Envelope rule: active tk2d tilemaps plus gate triggers and gate spawn points,
  padded 1/5/1/1 world units, camera bounds equal to the tilemap; camera-lock
  trigger volumes are excluded after the first Crossroads run cooked a wasted
  row of empty views below the map. Views use the canonical 24x16 stepping and
  halve on the cooker's budget errors.
- Crossroads_01 (`level37`): 20 views, 103,960 resident bytes, 63,161 stored,
  7 pages, 501 palettes, 507 textures, native scene-gate decoder PASS, geometry
  packet bound PASS, 10 breakables, 27 grass, one unsupported reveal controller
  (Secret Mask `level37:4778`), six inventoried actors of which only Crawler is
  movement-supported, 39.7 seconds. The earlier hand-laid 12-view probe measured
  98,808 resident bytes with the same atlas; the difference is view bookkeeping.
- `tools/world_pack_matrix.py --scene-packs .hkpsx/scene-packs` fills
  `ps1_ram_bytes`/`ps1_vram_bytes` only for `cooked` packs and labels
  `over_budget`/`failed`/`no_envelope` scenes explicitly; `pack_admission`
  counts every label. Tests: 408 Python tests pass including seven new
  envelope/layout/split/merge cases.
- Commands: `.venv/bin/python tools/cook_scene_pack.py Crossroads_01`,
  `.venv/bin/python tools/cook_scene_pack.py --all`, then
  `.venv/bin/python tools/world_pack_matrix.py`.
- Not claimed: playable rooms, actor banks, audio, effects, camera behavior or
  neighbour-window residency. The whole-catalog batch was started after this
  slice; its results are recorded when complete.

## P06 whole-catalog cooking evidence

- Three `tools/cook_scene_pack.py --all` passes with four `--shard` processes
  cooked every catalog scene: 457 cooked, 41 `no_envelope`, three over the
  128-view HKSCNE room limit (Abyss_21 136 views, White_Palace_13 135,
  White_Palace_20 156). 11,538 views were cooked in 336 CPU minutes; 258 views
  were halved on cook-time budgets and no scene needed a pack-time recook after
  the similarity guards landed.
- Measured isolated resident costs: median 91,076 bytes, maximum 316,892
  (Tutorial_01, 18 pages, 623,968 VRAM bytes for pages plus palettes). No scene
  exceeds the current 354,032-byte scene arena on its own; two scenes need 16
  or more of the 20 pages and Cliffs_01 needs 1,084 of the 1,248 palettes. These
  exclude actor banks, audio, effects and neighbour windows.
- Recorded exceptions: 1,697 unsupported cook records across the measured
  scenes and 2,000 actor records inventoried, with only Crawler and Runner
  families movement-supported. `.hkpsx/world-import/pack-matrix.{json,md}`
  carries the per-scene costs, `pack_admission` counts and
  `pack_failure_causes`.
- Shared causes fixed during the batch, each with a regression test and a
  source-only commit: unsupported GrassCut topology, soft-edged reveal masks,
  near-camera or over-stretched sprites, actor frames above the 64x64
  animation cache, markerless or one-shot hazard checkpoints and unreadable
  GameObjects are recorded instead of aborting; hazard polygons above 16
  vertices are tiled into exact pieces (`host/polygons.py`); the constant 4x4
  collapse and approximate replacements keep every draw's packet bound and
  every region's packet reservation; two tilemaps may share a render root;
  views exceeding runtime page/texture/byte/packet/actor budgets are halved.
- Remaining structural ceilings for P05/P07: the 128-room scene header limit
  for the three tallest scenes, and the 20-page/1,248-palette caps once
  neighbour windows and actor banks are added.

## P09 first slice and disc validation evidence (builds 72 to 74)

- `cargo run --release -- build` (root `hk-psx-build`) replaced `tools/build.py`;
  it cooks with caches, builds the guest, replaces the sole pair, writes the
  report and replays `tools/tapes/{focus,gate-return,town-reset,town-resident,
  town-well}.pxtape`, failing on faults or incomplete tapes.
- Build 72 (first disc since 71) failed three ways before passing: WorldMeta
  neighbour ids compared 1-based to 0-based indices; the bank staged in the
  scene-arena tail was overwritten by the coverage/atlas/scene in-place
  decodes, which relocate compressed bytes to the end of the arena they are
  given; `world_metadata()` re-ran the checked parse per spatial lookup and
  overflowed the 16-slot input queue (fault 4). Fixes: compare `expected+1`,
  admit the bank last and decode it inside the tail, extend
  `check_scene_gate_load` with `--meta`, and return the admission-validated view.
- Build 74 adds Crossroads_01 (20 views, chunk ids 107..126, 104,652 resident
  bytes, 7 pages, 501 palettes, 6,480-byte metadata bank). The link first
  overflowed RAM by 34,800 bytes because baked particle tracks grew from
  399,432 to 448,800 bytes with ten new emitters; with `BAKE_TRACKS=False`
  every emitter uses the scalar path the tracks were proven against and the
  link has 401,256 bytes before the stack floor. The stale linked-table budget
  in `host/opaque_tiles.py` now scales with the scene count because the guest
  streams per-scene certificate bundles instead. Ambience for Crossroads
  uses the King's Pass channel set (mask 57) read from its SceneManager.
- Replays on build 74: focus 850 polls; gate return (3 scene loads, 2 gates);
  Town reset; Town resident (x 174.6); Town well (13,074 polls, 3 scene loads,
  2 gate loads, final region 110 at x 90.6, y 5.4). All report zero faults and
  zero missed VBlanks. `.hkpsx/validate/*/` holds the captures.
- Not claimed: original well entry velocity or delayed collider, source camera
  locks, Runner/Climber/Mender behaviour, Crossroads audio cues beyond ambience,
  performance parity in Crossroads.
- Build 75 (five scenes, 154 views) surfaced and fixed: the music report cache
  ignoring the scene catalog, break-effect emitters outside the bounded particle
  model (now recorded per breakable), the Geo art quantization sheet exceeding
  256 rows because rock prefabs repeat across scenes (distinct images are now
  quantized once), and Geo rocks whose art exceeds the 32-pixel cell (recorded
  as `unsupported_rocks`). Six routes pass; headroom 376,680 bytes.
- Build 76 (19 scenes, 329 views) needed: the region sanity bound raised from
  256 to 1024 (no guest table depends on it), the flat-floor catalogue widened
  to eight heights (a guest slice), music regions without a snapshot tolerated,
  break-effect art admitted per scene in catalog order inside its 7,680-byte
  VRAM reservation (scenes 5 and later have their 107 emitter parts recorded as
  omissions until effect art streams per scene), and the scene arena derived
  from the pack (largest resident scene plus largest metadata bank, sector and
  LZ4 in-place margins, now 354,068 bytes) instead of a fixed constant that the
  wider similarity pass overflowed by 36 bytes. Per-region cook reuse
  (`data/regions/region-NNN/cook-cache.json`, keyed by spec, provenance inputs
  and the shared `host/cook_inputs.txt` code list) makes catalog-only growth
  cost only the new views. Six routes pass; linked headroom is 219,292 bytes,
  about 1 KB of static region payload per view, which makes the P05 migration of
  region payloads into the loadable bank the next structural requirement.

## P05 payload migration: measured order for the next executor

Static region payload measured on build 76 (329 views): `.data` grew 142,528
bytes for the 175 Crossroads views, about 815 bytes per view. Text share of the
generated `data/regions.rs` fields, as a proxy for their binary weight:

| Field | Share | Why it is large | Guest consumers |
| --- | ---: | --- | --- |
| `actors` | 188 KB | the scene actor bank is appended to every region pack, so each supported actor's `ActorSpec` is emitted once per region with region-specific clip indices (9 distinct actors, 330 emitted specs) | `enemies::sync_region` |
| `breakables` | 69 KB | per-region props with edge lists and hit polygons; 297 emitted for 187 distinct | `world::State::apply`, strike/fade paths |
| `neighbours` | 61 KB text, small binary | index lists; already present in the `HKWMTA01` bank | region activation |
| `hazards` | 15 KB | polygons already present in the bank as records | `world::hazard_contact` |
| `reveal_bindings` | 12 KB | per-region draw bindings | reveal masks |
| `checkpoints` | 6 KB | already present in the bank | `world::checkpoint_record` |

Step (1) is done in build 77: the appended scene actor bank pads each region's
clip table to one scene-wide clip index, so `data/regions.rs` now holds nine
`static ACTOR_S*` specs referenced from 329 regions instead of 330 inline
copies. Measured effect on linked RAM: none (`.data` 497,984 vs 498,160 bytes,
headroom 218,628 vs 219,292), so the text share above overstated the binary
weight of actors; the per-view growth sits in the remaining nested arrays
(breakables with edge lists and polygons, grass patches, hazards, `Region`
itself at 196 bytes). Link-map attribution on build 77: `hk_psx::world::REGIONS`
is 64,484 bytes (329 x 196) and the anonymous constant sections holding the
nested per-region arrays total 306,265 bytes across 1,714 sections, about
930 bytes per view, so the nested arrays, not the `Region` records or the
actor specs, are what the bank must absorb.

Order: (1) actor specs shared per scene (done), then move them into the bank;
(2) hazards and checkpoints (done in build 79); (3) breakables, their mask
fades and remote masks (done in build 80); (4) grass patches (done in build
81); (5) neighbours; (6) the `Region` scalars themselves. Each step must keep
the route replays green and record the new headroom.

Build 81 removed `Region.grass`: grass objects carry their off and on draw
indices as payload words, `world::grass`/`region_grass` read them, and
`State::apply` became one outlined pass over the region's objects (grass and
breakable visibility, own and remote fades) instead of three. The first build
81 candidate missed one VBlank on `gate-return` and `town-reset` at the scene
gate return into Tutorial: region entry runs `render::init` plus every state
pass without a pad poll since the previous frame, and the bank-driven passes
tipped that stretch over a VBlank. Region entry now services the pad
(`input::checkpoint`) between `render::init` and the state passes. Measured
against build 80: `.data` 459,232 -> 444,208 bytes (-15,024), `.text`
413,216 -> 413,896, linked headroom 238,248 -> 252,584 bytes.

Build 81 adds the seventh route, `crossroads-exit`: the `crossroads-floor`
tape plus a climb authored offline with `tools/route_sim.py` (the shared
`hk_sim` step over the cooked room edges of chunks 110/111/115/116) up the
three floating blocks to the Crossroads_01 right ledge, the gate into
Crossroads_02 (scene 3, region 127), four statue breaks there (state ids past
the old three-scene tables: `HK_BREAK_COUNT` 13 -> 17), and the return gate
into Crossroads_01 (`State::reset_scene(3)` on the bank), ending on the ledge
at (99.5, 13.39) with five scene loads, four gate loads and zero faults. It is
a gate route (`validate_scene_gates.py` checks the resident bank).

Build 80 removed `Region.breakables` and `Region.remote_masks`. The bank gained
a fifth section of u16 index lists (stride 2); a breakable object carries its
off/on/edge lists as three (first | count << 16) payload words, its flags as
persistent | semi << 1 | door_sound << 2 | fade_ticks << 6, and is followed by
its `KIND_MASK_FADE` objects (draw list word, ticks); `KIND_REMOTE_MASK`
objects carry the owner state id, draw list, ticks and owner total.
`tools/world_metadata.py` now owns every breakable validation that
`host/world.py` used to apply before emitting Rust. The guest reads the
admitted bank through `disc::admitted_world_metadata` and
`world::bank_region` (`world::breakables`, `world::remote_masks`), with the
edge helpers outlined into `broken_edge` after a first iterator-heavy version
grew `.text` by 29 KB. `State::reset_scene` walks the admitted bank instead of
the static table. Host tests select a bank per thread through
`world::TEST_BANK`; `tools/test_world.py` cooks the scene 0 bank and a
synthetic fixture bank for them. Measured against build 79: `.data` 486,448 ->
459,232 bytes (-27,216), `.text` 402,384 -> 413,216 (+10,832), scene arena
354,068 -> 361,732 (banks grew), linked headroom 223,444 -> 238,248 bytes;
six routes pass.

Build 80 also fixed a latent fault: `world::SCENES` was a hard-coded 3 while
the disc carried 19 scenes, so `State::broken`, `fade_left`, `fade_active` and
`grass` were sized for three scenes. Breaking an object in scene 3 or later
would have indexed past the tables and leaving any such scene would have hit
`assert!(scene < SCENES)` in `reset_scene`. The generated `data/regions.rs`
now emits `pub const SCENES` from the catalog and `world.rs` asserts it equals
`disc::SCENE_COUNT` at compile time. No route reached those scenes, which is
why the six replays never caught it; a route into Crossroads_02 or later is
still owed. For the full game these tables must not scale with the catalog
either: `fade_left` alone would be 128 KB at 501 scenes, so transient fades and
grass should become resident-scene state while `broken` stays global (8 KB).

Build 79 removed `Region.hazards` and `Region.checkpoints`. The HKWMTA01
object record grew from 36 to 48 bytes with three kind-specific payload words
(hazard: origin x, damage | respawn << 16; checkpoint: spawn x, spawn y,
facing), `tools/world_metadata.py` owns the DamageHero respawn convention, and
`world::hazard_contact`/`world::checkpoint` read the admitted bank region
(resolved once per region entry by `disc::world_region_index`). Measured:
`.data` 497,984 -> 486,448 bytes (-11,536), linked headroom 218,628 -> 223,444
bytes; six routes pass. The remaining static payload per view is breakables
(297 records, 1,188 polygon points, edge/draw lists), grass patches (443 x 28
bytes), neighbours (1,760 x 4 bytes), mask bindings and `Region` itself, so
even the complete migration of the nested arrays leaves about 300 bytes per
view static; the full game needs the `Region` table itself to become
per-scene bank data, which is the P05 end state.

## Required package completion record

For every validated package, add source contract/version, covered instances and
variants, implementation paths, remaining exceptions, commands actually run,
original-game evidence, guest/disc hashes where applicable, inspected output,
memory/performance effects, persistence/reset evidence and the next package.
