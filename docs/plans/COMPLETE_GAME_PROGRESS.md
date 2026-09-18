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
| P05 | in progress | Build 87 compacted the Great Door bindings and packet budgets (headroom 298,296); build 86 made fades and grass resident-scene state and stepped the bank validation through pad polls (headroom 294,200); build 85 narrowed the per-slot scene table (headroom 296,248); build 84 removed the static region table: `Region` is a runtime value from the bank plus per-scene statics (headroom 294,200); build 83 moved actor references and reveal bindings into the bank (headroom 259,416); build 82 moved neighbours (headroom 260,776); build 81 moved grass into the bank (headroom 252,584) and added the `crossroads-exit` route through Crossroads_02 and back; build 80 moved breakables, mask fades and remote masks into the loadable bank behind checked views and fixed the guest state tables that were sized for three scenes; build 79 moved hazards and checkpoints (`.data` -11,536 bytes, headroom 223,444). The metadata relocation now runs on the actual disc: build 72 exposed and fixed a 1-based/0-based neighbour comparison, the bank being overwritten in the arena tail by later in-place decodes (it is admitted last and decoded inside the tail, and the host gate-load proof stages it the same way), and per-tick re-parsing that overflowed the input queue. Baked per-particle tracks (449 KB of permanent RAM) were replaced by the guest's proven scalar path, and the world metadata manifest follows scene-manifest order. Linked headroom is 401,256 bytes with three scenes. Migration of the remaining static region payloads (grass, breakables, hazards, actors, emitters) to the bank remains. |
| P06 | in progress | Build 90 cooks and packs the whole Forgotten Crossroads (45 scenes, 693 views) with explicit omissions for the surface audio, atmos channels and actors the guest does not support yet. Previously:  in progress | `tools/world_pack_matrix.py` joins all 501 verified scene artifacts, dependency inventories and current+neighbor windows, with deterministic hashes and source-cost peaks. `tools/cook_scene_pack.py` now cooks any catalog scene into an isolated, write-guarded pack with the canonical cooker/similarity/scene-bank path, and the matrix merges its measured PS1 costs per scene. The whole-catalog batch is complete: 457 of 501 scenes cook with a passing native decoder, 41 have no source envelope (cinematics, menus, boss-defeated and preload helpers) and three exceed the 128-view HKSCNE room limit. Eleven shared abort causes became explicit recorded exceptions or lossless transformations. Shared art canonical mappings across scenes, neighbour-window costs and production admission remain. |
| P07 | in progress | Build 94 admits the Crossroads Climbers (kinematic surface follower on the resident terrain, rotated body and sprite, freeze stun, corpse); build 89 admits the Crossroads Zombie Runners (controller, clips, corpses, audio bank on voices 21 to 23; dust and death effects recorded omissions) and replays the routes concurrently; build 88 streams break-effect art per scene through one disc chunk per scene decoded at admission and uploaded to the fixed effect VRAM rectangles; 210 emitters across 18 scenes with no VRAM omissions. Previously: persistent Geo state is now a flat 256-entry source-state pool keyed by scene and authored rock slot, covering the verified 207 GeoRock instances without a `[scene][32]` allocation. Version-2 host plans preserve all 850 directed gates, and the bounded room-residency runtime can safely evict only completed slots outside a keep-set. Native residency/Geo tests pass and the no-disc link is hazard-free with 16,232 bytes before the stack floor. Predictive CD residency and guest streaming admission are still pending. |
| P13 | in progress | Build 105 admits the five catalog benches (rest, heal, respawn marker; `town-bench` route). Earlier: shared persistent stores now have a versioned `HKSV` fixed-buffer snapshot codec for global and mode owners, with strict length, ordering, checksum and capacity validation; active-scene flags reset on load. Memory-card I/O and actual quit/reload routes remain pending. |
| P08 | in progress | Tile-coverage event chains now separate back and front owners, preserving front masks through the complete back pass. The first regression and full hk-format coverage suite pass. Material-family completion, black-fill source coverage, dynamic mask invalidation, actor/effect cache expansion and representative visual replay remain. |
| P09 | in progress | Build 104 runs the source entry sequence (fade, lead, forced walk; hidden top drop) for every gate; build 103 applies the source gate rules (side gates need the Knight facing in, no transition while recoiling, delayed top colliders); build 102 adds the source CameraLockArea limits (world bank kind 11) and a damped camera with look-ahead; per-entry behaviour descriptors (forced walk, fades, control delays) remain. Earlier: Crossroads_01 is scene 2 of a single host scene catalog (`host/quality.py SCENE_TABLE`) that every cooker, generated table and the guest scene count derive from. The authored Town `bot1` gate drops the Knight into Crossroads_01 `top1` and the return gates resolve to Town `bot1`; the `town-well` route tape replays on the final CUE with three scene loads and the Knight walking inside Crossroads. Build 75 adds Crossroads_02 and Crossroads_07 through `GRID_SCENE_LAYOUTS` (one catalog entry per scene); their gates are generated from source and the linked headroom is 376,680 bytes. The `crossroads-floor` route walks the Crossroads_01 floor to the right wall; reaching `right1` needs an authored jump route. Build 78 gives top-side gate entries the source's -12 units/s initial drop (`Gate.entry_vy`, verified for the well; assumed for other top entries). The delayed top-gate collider, source camera locks, the Crossroads enemies beyond the Crawler and return replays remain. |
| P19/P20 | in progress | Builds 89 to 101: Runners (with Bargers, Hornheads and Leapers through the parameterized Runner), Climbers, Vengeflies, Gruzzers, Baldurs and Aspid Hunters (first projectile pool) are admitted: 120 of 144 placed Crossroads actors. Remaining: Hatchers (runtime spawning), Shields, Guards, Mender, Myla, mirrored placements, Runner dust and death effects, Climber loop audio. |
| P10-P12, P14-P18, P21-P31 | pending | Follow dependency order in the master plan. |

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
81); (5) neighbours (done in build 82); (6) actors and reveal bindings (done
in build 83); (7) the `Region` scalars themselves (done in build 84). Each
step must keep the route replays green and record the new headroom.

Build 84 removed `world::REGIONS`. `Region` is now a `Copy` value that
`world::resident(slot)` builds from the admitted bank (identity, bounds,
collision bounds, camera) plus per-scene statics (`SCENE_EMITTERS`,
`SCENE_REVEAL_MASKS`) and three small variant catalogues (`DEBRIS_CATALOG`,
`PARTICLE_BANKS`, `IMPACT_CATALOG`) indexed by the region's
`KIND_REGION_STATICS` bank object, which `host/world.py` annotates on the
region rows before the bank is cooked. Gates carry `target_region` and the
Great Door entries carry `region`, both located by the cookers, so the guest
no longer needs any cross-scene bounds table; spatial region changes use
`disc::Cache::locate`. The only per-view table left in the world module is
`REGION_SCENES` (one byte per slot); `REGION_SCENE_LOCAL` in the scene
manifest (eight bytes per slot), `SCENERY_PACKET_BUDGETS` and the Great Door
`REGIONS` bindings are the remaining per-slot statics for P05 item 8.

Two scene-identity faults surfaced on the way. The disc module works in
manifest scene indices (allocation order) while `Region.scene` and the bank
carry catalogue scene ids; Tutorial is 0 in both, which had hidden that
`Cache::locate` was compared against the wrong kind by every caller, so
checkpoint spawns and enemy cross-region terrain never resolved outside the
first scene, and a first build 84 candidate rejected the Town bank at
admission. `Cache::locate` now takes catalogue scene ids and checks them
against the admitted bank, and the admission differential relies on the
existing owner check. Measured against build 83: `.data` 430,048 -> 386,832
bytes (-43,216), `.text` 417,820 -> 422,120, scene arena 365,140 -> 369,268
(one statics object per region in the largest bank), linked headroom 259,416
-> 294,200 bytes; seven routes pass. The guest's permanent memory no longer
grows with per-view metadata except for the tables named above.

Build 85 narrowed `REGION_SCENE_LOCAL` to `(u16, u8)` (manifest scene index,
room index inside the scene bank) with the packer bounding both; `.data`
386,832 -> 384,784, headroom 294,200 -> 296,248 bytes; seven routes pass.

Build 86 made the transient state tables resident-scene-local: `fade_left`,
`fade_active` and the grass cut bits are indexed by the state within the
resident scene (every scene leave resets its scene and exactly one scene is
resident), while `broken` keeps every scene's session-persistent bit. At 501
scenes this removes about 190 KB of main-frame tables (`fade_left` alone was
128 KB); on the nineteen-scene disc the main frame shrinks by 7 KB inside the
unchanged stack reservation. The recurring one-VBlank miss at the Tutorial
return was finally located in the route CSV: it happened during loading, in
`WorldMeta::parse`, whose reference validation grew with every object kind
moved into the bank and ran without a poll. `WorldMeta::parse_with` now
steps a callback after each region and the disc module polls there, and the
admission differential polls per region too; the region-entry checkpoints
from builds 81 and 83 stay as belt and braces. The `town-reset` tape ended a
few polls short of its final Tutorial reload once loading polled more often,
so it gained 180 idle samples. Headroom 294,200 bytes (`.text` +2 KB for the
stepped parse); seven routes pass with zero missed VBlanks.

Build 87 finished P05 item 8 for the tables the guest links: the Great Door
bindings are a sorted `(slot, Binding)` list of the 86 slots that see the
door instead of an `Option` per catalogue slot, and `SCENERY_PACKET_BUDGETS`
is `u16`. `.data` 386,432 -> 382,016, headroom 294,200 -> 298,296 bytes;
seven routes pass. Per-view statics now total four bytes per slot
(`REGION_SCENES`, `REGION_SCENE_LOCAL`) plus two for the packet budget.

## P07/P25 first slice: break-effect art streams per scene (build 88)

Break-effect texels no longer share one linked sheet admitted in catalog
order. `host/break_effects.py` cooks one art bank per catalog scene inside the
same fixed VRAM rectangles (largest sheet 4,800 bytes), writes each as an HLZC
chunk (`.hkpsx/break-effects/scene_N.hkfx.z`, four zero bytes for a scene
without emitters) and emits per-scene `SCENE_STYLES`, `SCENE_EMITTERS`,
`SCENE_ART` and `SCENE_UPLOADS` tables plus `EFFECT_ART_MANIFEST`. The pack
gains one chunk per scene after the world metadata chunks (`build_guest.py`
copies them as `chunk_N.effectart`); `disc::Cache::prepare_effect_art` reads
and decodes the admitted scene's chunk into the arena front before the scene
decode and uploads it to VRAM (`break_effects::upload_scene`), and
`is_ready` includes the effect scene. The particle pool already threaded the
scene through `spawn_break`/`tick_break`, so the guest now indexes styles,
emitters and art by the particle's scene. Result: 210 supported emitters
across 18 scenes with no VRAM omissions (build 87 admitted 103 and omitted
36 parts of scenes 8 and 16); the remaining `other_parts` are unsupported
particle styles and non-particle debris, unchanged. Baked particle tracks
and their retired proof harness (`tools/test_particle_tracks.py`,
`tests/particle_tracks_source.rs`) are removed; every emitter uses the
scalar path that build 74 made canonical. Cost: `.data` 382,016 -> 395,104
(the doubled emitter and per-scene style tables are still linked), `.text`
422,836 -> 426,144, headroom 298,296 -> 281,912 bytes; seven routes pass.
Emitter and style metadata are per breakable and belong in the world bank
with their owner (P05 item 7 continues there); Geo art has no omissions on
this disc (24 rocks, 5,154 of 8,192 VRAM bytes) and follows the same chunk
pattern when it overflows.

## Crossroads Zombie Runners admitted (build 89)

The strict recognizer's `pending_movement_control` is now promoted to a
supported `ZombieSwipeWalker` control in `host/actors.py`, so the two
Crossroads_01 Runners (and the Runners in Crossroads_05 and _21) cook their
seven live and two corpse clips into the scene actor banks and emit
`ActorController::Runner` specs. The corpse "Single" clip uses tk2d wrap mode
6, which the scene format rejects; `cook.guest_wrap` maps a one-frame Single
to Once and fails closed on other modes (both cookers, `cook.py` and
`effects.py`, use it). `host/runner_audio.py` is an asset script writing
`data/runner-audio.adpcm` and `.rs` (20,464 bytes at SPU 495,792 to 516,256,
8,032 bytes left); `game/src/runner_audio.rs` uploads it after the Geo bank
and plays the walk loop per Runner on voices 21 and 22 at the authored
initial pitch (a third simultaneous Runner is counted as dropped) and the two
chase calls on voice 23 at the controller's pitch clamped to the authored
bounds. `main::runner_event` binds the sink: AudioPlay/AudioStop/Destroy drive
the loops, ChaseSound the calls; DustStart/DustStop are counted in
`HK_RUNNER_DUST_EVENTS` and not drawn, and source death effects are not
presented (both recorded as actor limitations). Two boot assertions in the
bank check were wrong on first contact with real data (the loop's authored
pitch sits below its nominal rate; the ADPCM header byte carries a filter
nibble) and were fixed. On the routes the Runners chase and swipe: the
`crossroads-floor` replay records 24 loop starts, 14 chase calls, 28 dust
events and 2 enemy hits with the invincibility cheat holding health at 5;
seven routes pass. Memory: `.text` 426,144 -> 428,144, `.data` 395,104 ->
395,152 plus the 20 KB bank, headroom 281,912 -> 279,832 bytes.

## Build speed (user priority, 2026-09-17)

Measured before: region cook with every per-region cache hit about 200 s
(postpack, actor banks, dedup and generate ran regardless), full recook 12 to
30 min whenever any cooker file in `host/cook_inputs.txt` changed, asset
scripts 45 s plus `pack_scenes` with its decoder proof 43 s every build, guest
link and disc about 10 s, seven route replays 7 min sequential; about 16 min
for a guest-only change.

Changes, each verified byte-identical on the cooked chunks and green on the
seven routes:

- Route replays run concurrently in the driver (1:42 wall for seven; the
  emulator is deterministic).
- `alpha_covers.record` persists its results in `.hkpsx/alpha-covers-cache.json`
  (72k calls per warm cook were two thirds of its time), and
  `texture_dedup.deduplicate_room` caches each (pack, replacements, code) result
  under `.hkpsx/dedup-cache/`; the region report is checkpointed only after
  fresh cooks and at most every tenth region instead of after every cached
  one. Warm cook: 200 s -> 23 s.
- The driver's asset cache (`.hkpsx/assets-cache.json`, keyed on every host and
  tool Python file, the region report and the cook verdict, verified against
  the hashes of everything under `data/` and the packer state) skips the
  twelve asset scripts and the guest prepass (`build_guest.prepass`: scenery
  budgets, `pack_scenes`, opaque tiles and groups, scene certificates) when
  nothing they read changed; `build_guest.py --no-prepass` reuses the packed
  scenes on disk.
- Cache misses are cooked by one worker process per scene (`regions.py
  --precook SCENE`), up to cpu_count - 2 at once, region 1's scene first because
  it writes the shared knight and gameplay files; the parent loop then reuses
  every worker result through the per-region cache. Full recook: 253 s.

Measured after: no-change or guest-only build 1:48 (of which the seven
parallel replays are about 1:40), cooker-code change about 5:00 including the
full parallel recook. Not done: a split between the geometry cook and the
metadata/postpack stages (the cook is one function whose art appends depend on
the effects code), which would make actor and effects changes cost the 23 s
warm path instead of the 4 min recook; and speeding the replays themselves,
which are bound by the longest route. Cook workers occasionally die with
SIGSEGV at startup under parallel load (never alone); the parent retries a
signalled worker up to twice and caps workers at eight.

## Build 90: the whole Forgotten Crossroads (45 scenes, 693 views)

All remaining Crossroads scenes joined `host/quality.py SCENE_TABLE` with the
envelopes the isolated pack matrix measured (`.hkpsx/scene-packs/*/summary.json`):
11_alt, 15, 22, 25, 27, 30, 31, 33, 35 to 40, 42, 43, 45, 46, 46b, 47 to 50,
52 and ShamanTemple (boss, preload and defeated variants excluded). The
parallel cook did 693 regions in about five minutes. Three cookers stopped on
new content and now record explicit omissions instead: `actor_sources` bounds
the 32-slot pool by supported actors only (Crossroads_50 and ShamanTemple hold
more HealthManagers than that as records), Runners whose depth is not the
measured guest plane stay unsupported, `cook_audio` substitutes the Dust
footstep set for the two scenes with another environment (Crossroads_30,
ShamanTemple) and `cook_music` omits atmos channels without a resident loop
(channel 15 in Crossroads_46b and Crossroads_50; the guest keeps six resident
loops on voices 6 to 11 and the SPU has 8 KB free). Seven routes pass; the new
scenes are reachable through their authored gates but no route enters them yet.
Actor inventory across the 45 scenes: 127 HealthManager actors, 14 supported
(Crawlers and Runners); unsupported families by count: Buzzer 20, Climber 15,
Fly 12, Roller 11, Zombie Barger 9, Spitter 8, Zombie Hornhead 8, Zombie
Runner variants 7, Zombie Leaper 6, Hatcher 6, Zombie Guard 2, Blocker 2.

Memory: `.data` 415,168 -> 558,400 bytes, headroom 259,352 -> 109,540. The
growth is the per-scene auxiliary tables P05 item 7 names: texture flags and
black/opaque cores for every texture of every scene (4,192 textures, about
38 KB), break-effect emitters/styles/art/uploads for all scenes (411 emitters,
about 55 KB), Pogo targets (281), atlas/coverage/metadata descriptors (about
20 KB), opaque groups and gates. At 5.5 KB per scene the full game cannot link
these; the next slices move them into the per-scene chunks (texture flags and
cores into the scene bank, effect tables into the effect-art chunk, pogo
targets into the world bank).

Build 91 moved the per-texture flags and black/opaque cores into the resident
scene bank: `with_texture_attributes` appends one 12-byte record per texture
after the reference arrays (offset word 104, `hk_format::scene::ATTRIBUTE_STRIDE`),
the stepped scene validation checks the section, and `render::init` fills two
`[[u8;4]; MAX_TEXTURES]` arrays from the admitted `Scene` view instead of
borrowing linked slices. `SCENE_TEXTURE_FLAGS`, `SCENE_BLACK_CORES` and
`SCENE_OPAQUE_CORES` are gone from the manifest. The opaque tile and group
caches now key on the packed scene order too (the attribute bytes reordered the
allocation and their per-index bindings went stale). `.data` 558,400 ->
421,536 bytes, `.bss` +22 KB (the core arrays and the larger resident banks
in the arena), headroom 109,540 -> 222,316; seven routes pass.

Build 92 moved the break-effect tables into the effect-art chunk. The chunk is
now `HKFX0001`: a 32-byte header (style, emitter, art, upload and frame counts,
texel base), 168-byte style records that reference the linked `CURVES` table
by index and a per-chunk frame table by span, 64-byte emitters, 8-byte art,
12-byte uploads with absolute texel offsets, then the texels.
`break_effects::load_scene` parses it at admission into bounded resident
tables (`FX_MAX_*`: 16 styles, 96 emitters, 48 art, 48 uploads, 96 frames;
the 45 scenes peak at 10/71/33/36) and uploads the texels; `scene_styles`,
`scene_emitters` and the art lookup read the resident scene only. The
per-scene `SCENE_STYLES`/`SCENE_EMITTERS`/`SCENE_ART`/`SCENE_UPLOADS` statics
are gone; only the eleven distinct sample curves still link. `.data` 421,536
-> 384,368 bytes, headroom 222,316 -> 257,132; seven routes pass (the
`crossroads-exit` statue breaks spawn from the resident tables).

Build 93 moved the static NailSlash (pogo) targets into the world bank:
`postpack_pogo` attaches each scene target to every region whose activation
envelope it touches, the encoder writes them as `KIND_POGO` objects (polygons
in the polygon section, flag 1 for horizontal-and-up, state = owning
breakable id or none) and `enemies::pogo_contact` iterates
`world::pogo_targets(region)`; `data/pogo.rs` is no longer generated. `.data`
384,368 -> 363,296 bytes, headroom 257,132 -> 271,340 (the world banks grew
by the targets, arena 386,968); seven routes pass. The chunk manifests
(atlas, coverage, metadata, effect descriptors, about 20 KB) are the last
per-scene linked tables.

## Crossroads Climbers (Tiktik) admitted (build 94)

`host/climber.py::recognize` admits the placed Climber instances that match
the audited contract (.hkpsx/climber70/CONTRACT.md: speed 2, spin 0.25 s,
padding 0.1, min turn 0.25, the 1.09375 x 0.921875 body at offset
(0.015625, 0.4765625), kinematic body, freeze-in-place recoil, Walk 4@10 loop,
Stun 7@12 loop-section, Death Air 8@30, Death Land 3@15, looping
play-on-awake audio source) and `actors.py` promotes them; `cook.py` binds
Walk (also as the turn clip, since the source walk animation continues through
turns) and Stun; `effects.py` accepts the Climber corpse (fling speed 20,
bounce 0.45, `resetRotation`, zero spawn offset) and `hk_sim::CorpseSpec`
gained `fling_speed`, scaling the 15-unit launch table. The shared
`ActorController::Climber { stun_clip, start_right }` selects a
`ClimberRuntime` in the guest actor pool: the kinematic controller from
`shared/hk-sim/src/climber.rs` owns position, velocity, rotation and phase;
the runtime advances position by the cardinal velocity each tick, casts the
controller's point rays against the resident terrain edges
(`climber_ray_hit`: TransformPoint with rotation and scale, TransformDirection
with rotation, nearest segment hit within the ray length) for attachment,
ground and wall sensing, dispatches the immediate first Walk iteration after
attachment, drives turns and the stun timer, freezes in place on an accepted
hit (`Recoil.OnHandleFreeze`) instead of displacement recoil, and dies into
the shared corpse path. The body box rotates with the transform (axis aligned
again at every cardinal rest) and the sprite is drawn rotated by the
controller's Q16 degrees through the debris sine table. Admitted: Crossroads_01
(2), Crossroads_07 (7 of its 8 actors), Crossroads_05 and Crossroads_50; 15
placed Climbers in total. Route telemetry (`HK_CLIMBER_X/Y/HP` for
level37:5145) shows the Crossroads_01 Climber circling its 6 x 2 block at
2 units/s with cardinal turns at each corner; seven routes pass. Not yet
presented: the climb loop audio (recorded limitation); the rotated sprite has
telemetry but no frame-level visual check on the routes, whose cameras never
reach a Climber. `Actor` is 208 bytes (pool 6,656). `.text` +22 KB.

Build 83 removed `Region.actors` and `Region.reveal_bindings`. Supported
actor objects (flag 64) carry their index into the new per-scene
`SCENE_ACTORS[scene]` static, which `host/world.py` assigns as `spec_index` on
the actor records before the bank is cooked; a region's reveal bindings are
one `KIND_REVEAL_BINDINGS` object whose index list interleaves (controller,
draw) pairs, bounded by the scene's controller count that `pack_scenes.py`
passes to the encoder. `enemies::sync_region` and the per-actor spec lookups
read `world::region_actors`; `reveal_masks::State::apply` takes a binding
iterator. The Tutorial return missed a VBlank again with the bank-driven
entry passes, so region entry now polls the pad before the passes, between
them and after `render::init`; the longest stretch between polls on the
`gate-return` replay is 315k CPU cycles in gameplay (a VBlank is about
564k). Measured against build 82: `.data` 435,632 -> 429,712 bytes (-5,920),
`.text` 414,288 -> 418,400, scene arena 361,732 -> 365,140 (the Tutorial bank
grew by the binding lists), linked headroom 260,776 -> 259,416 bytes. The
arena scales with the largest scene bank while the static tables scaled with
every view, so the trade is the intended one even though this step's net is
slightly negative on the nineteen-scene disc; seven routes pass.

Build 82 removed `Region.neighbours`. `world::upcoming` resolves neighbours
through the admitted bank (global chunk ids, bounds from the bank; neighbours
in another scene's bank are left to the scene gates) and the admission
differential checks each neighbour id against the catalogue instead of the
static list. `preload.rs` still has its own `Cell` abstraction and is not
linked into the guest. Measured against build 81: `.data` 444,208 -> 435,632
bytes (-8,576), linked headroom 252,584 -> 260,776 bytes; seven routes pass.

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

## Vengeflies (Buzzers) admitted, actor simulation bounded (build 95)

`host/vengefly.py::recognize` admits the Buzzer instances whose `chaser` FSM
parameters, transitions, body (1.25 x 0.625 box at offset (0, -0.1875),
dynamic gravity-free Rigidbody2D), Recoil (15 for 0.25 s), alert circle
(7.804 units) and clips (Idle 5@12, TurnToIdle 7@12 loop-section, Startle 4@12
once, Chase 4@12, TurnToFly 6@12) match .hkpsx/vengefly/CONTRACT.md (CIL of
IdleBuzz, ChaseObject, Decelerate, FaceDirection, LineOfSightDetector, Recoil
in the same directory). `shared/hk-sim/src/vengefly.rs` is the controller:
IdleBuzz roaming (per-axis random acceleration over 0.75 to 1 s windows,
roaming-edge damping), sight from the alert circle against the hero body plus
the existing `runner_senses::line_of_sight`, Startle (velocity zero, faces the
hero, 20 ticks), ChaseObject (0.045 per fixed step toward the hero, 5 units/s
clamp, one extra DoBuzz per frame while seen for the In Sight / Out of Sight
ping-pong), the ten-second attention span and Stop's 0.12 deceleration, with
a 50 Hz fixed-step accumulator inside the 60 Hz tick so the per-step
constants stay authored (docs/VENGEFLY.md). The guest `VengeflyRuntime`
(`ActorController::Vengefly { startle_clip, chase_clip, turn_fly_clip }`,
walk = Idle, turn = TurnToIdle) integrates the body with the bounded solver at
zero gravity, reuses the generic recoil displacement pass, and the corpse is
the source `breaker` variant: `CorpseSpec` gained `gravity` (42 here, 48 for
the earlier corpses) and `breaker` (removed on landing; break pieces not
presented). 18 of 20 placed Buzzers are admitted (two share a body-collider
variant the recognizer rejects), three of them in King's Pass, so every route
now meets a live Buzzer.

That exposed two guest problems the seven routes had never hit. First, a
Buzzer kill near the King's Pass exit left three coins and death particles
alive while the knight cut grass: `State::strike` invalidated the edge table
and the rest of that tick (coins, enemies, Lifeblood) fell back to the
per-query breakable scan (`broken_edge` rebuilds the bank region and decodes
every object per edge), several VBlanks in one frame; the input queue (16
polls) filled and the sampler faulted. `main.rs` now calls
`state.refresh_edges` right after a break or depleted rock. Second, with a
whole scene resident every actor of the scene advanced every tick, each
re-decoding the 128 edges of its own view plus the broken-object exclusions
(`Room::edge` under `advance_in_view` was the largest single enemy cost).
`enemies.rs` keeps four keyed terrain copies (`ActorEdges`, keyed by target
view, active view and a new `State::edge_epoch` that every exclusion change
bumps) and only advances actors within one view (24 x 16 units) of the active
view; farther actors hold their state. That gate is a recorded departure from
Unity, which keeps simulating them, and is marked `ponytail:` in the code for
review. Input queue peak on `gate-return` went from 11 back to 7 (the scene
load) with pending 1 to 2 in gameplay; `HK_INPUT_PENDING` is new telemetry.
`regions.py::remap_actor_clips` now remaps every `*_clip` binding into the
scene clip space: the Climber's `stun_clip` and the new Vengefly clips were
left in region-local numbering, which drew a stunned Tiktik or a Buzzer with
the knight's frames. `Actor` is 256 bytes (pool 8 KiB; folding the three
optional runtimes into one enum would give back about 120 bytes each).
Linked headroom 193,508 bytes, scene arena 415,488; seven routes pass; hk-sim
(Vengefly tests included), Python (424) and world runtime (47) suites pass.

## Gruzzers and Baldurs admitted (builds 96 and 97)

Build 96 admits the Gruzzers (Fly, 19 placed across Crossroads_07, _25 and
_50; `host/gruzzer.py`, `shared/hk-sim/src/gruzzer.rs`, docs/GRUZZER.md,
.hkpsx/gruzzer/CONTRACT.md). The `Bouncer Control` FSM waits for the main
camera within 44 units, then flies at 5.2 units/s along a random angle; the
controller keeps the angle and the FSM's `Facing Right` flag, the guest turns
the angle into a velocity through the debris sine table, moves the
gravity-free body with the bounded solver and reports the blocked axis as the
bonk side in the CheckCollisionSide order (up, right, down, left), each of
which re-aims from the authored ranges. Its corpse is a breaker that bounces
twice (`CorpseSpec.smash_bounces`, Corpse.Land counts one bounce per frame and
smashes once the count reaches smashBounces) with 0.7 bounce and 0.7 gravity.
Two cook-side fixes came with it: `host/cook_inputs.txt` now lists the
recognizer modules (climber, vengefly, gruzzer, baldur), which the whole-cook
cache had never fingerprinted, so a recognizer change alone did not recook;
and the grass-impact variant budget went from 8 to 32 (`host/world.py`,
`tools/world_metadata.py`), because per-scene clip indices shift as actor
art is appended and the deduplicated catalogue outgrew 8 with more actor
scenes. Headroom 177,124 bytes; seven routes pass.

Build 97 admits the Baldurs (Roller, 11 placed in Crossroads_ShamanTemple;
`host/baldur.py`, `shared/hk-sim/src/baldur.rs`, docs/BALDUR.md,
.hkpsx/baldur/CONTRACT.md). The `Roller` FSM idles facing the hero with x
velocity held at zero, wakes on the 21.14 x 1.9 alert box plus line of sight,
plays Start, draws a 2 to 3 s roll time, accelerates 0.45 units/s per frame
toward the hero up to 11 units/s, flips and launches at 12 units/s along 115
or 65 degrees on a wall contact, resumes the roll the other way on landing,
and stops, rests half a second and idles when the roll time runs out;
Recoil's horizontal event resets the roll speed. The guest `BaldurRuntime`
runs the gravity body (scale 0.8) with last frame's blocked axis and grounded
flag as the WALL and GROUND senses, resolves the spawn once like the
Crawlers, and adds the generic Recoil displacement (25 for 0.15 s). Placed
Rollers start mirrored (scale.x -1, the FSM sets the sign itself) and some
carry their body box twice; both are accepted. The corpse prefab has no
Corpse component: its `corpse` FSM lands, plays Death Land while rolling
faster than 2 units/s, then shrinks and destroys itself; the port gives it
`CorpseSpec.remove_after_land` 120 ticks with the box bounds of its 0.43
circle. `Actor` is 312 bytes; folding the five optional runtimes into one
enum is the obvious next saving.

## Husk Bullies and Hornheads through the parameterized Runner (build 98)

Every Zombie Swipe placement in the catalog runs the same PlayMaker FSM: a
structural fingerprint (`runner.py::fsm_fingerprint`: states, transitions,
enabled actions and their scalar fields, variables other than `Lunge Speed`)
is identical across Runner, Barger and Hornhead objects in seven scenes; the
old whole-serialization sha differed per scene only through embedded owner
references, which is why the Runners outside Crossroads_01 read as
"unverified variants". The differences are Walker fields (walk speed 1.5 or
2.5, pause wait/time ranges) and the FSM `Lunge Speed` (6, 14, 9), plus each
placement's body and alert boxes, clip frame counts and corpse prefab
(`Corpse Zombie Basic One/Three/Five`, same structure). `hk_sim::runner` now
takes `Params { walk_speed, lunge_speed, walking_wait, paused_wait }`
(`Params::RUNNER` is the level37 variant; the wait sampler receives the
endpoints), `runner_senses::Shape` carries the body and alert boxes
(`Shape::RUNNER` for the old constants, `from_boxes` from the actor spec), and
`ActorController::Runner` gained `params` and `alert`. The recognizer accepts
depth within 0.01, continuous collision detection (Hornheads), libraries
without `Fall` (never played) and variant clip counts, and the corpse admits
the three Basic prefabs by name and player data. Admitted: 9 Runners, 9
Bargers, 8 Hornheads (from 4 Runners). Still unsupported in the family:
Leapers (their own `Zombie Leap` FSM), Shields (no pauses, own FSM), Guards,
Myla, one Runner with an `enemy_corpse` FSM and one that starts inactive.
Headroom 168,932 bytes; seven routes pass.

## Leapers through a Leap attack on the Runner walker (build 99)

The Leaper shares the Walker with the Runner family and layers the `Zombie
Leap` FSM: Ready (alert by name plus sight) -> StopWalker, Jump X Speed =
(Hero X - Self X) * 1.25, face -> Anticipate (velocity zero, Attack clip,
wait for its trigger frame 3 of 11 at 12 fps) -> Launch (velocity (Jump X,
20)) -> Lunge until bottom contact -> Land clip -> Idle .5 s -> Reset
(StartWalker). `hk_sim::runner::Params` gained `attack: Attack::Swipe |
Attack::Leap { trigger_ticks, jump_speed_y, jump_x_factor, idle_ticks }` and
`gravity` (Runners fall at gravity scale 1, Leapers at .8), `Senses` gained
`grounded`, and the Leap FSM has no TOOK DAMAGE or RECOIL HORIZONTAL
transitions, so those callbacks are no-ops for it. `host/runner.py`
fingerprints the Leap FSM separately (`LEAP_FSM_SHA256`), reads the trigger
frame from the Attack clip and binds Attack as the anticipate and lunge slots
and Land as the cooldown slot; `Corpse Zombie Leaper` joins the admitted
Runner-family corpses. Admitted: 5 of 6 Leapers (the mirrored placement in
Crossroads_37 is still rejected, as the Walker facing of a mirrored start is
unverified). The Leaper's own attack samples are not in the Runner audio bank;
the guest plays the Runner chase sound in their place. Seven routes pass.

## Aspid Hunters and the first projectiles (build 100)

`host/aspid.py` admits the Spitters (18 placed, including the `startAlert`
arena placements that skip Idle) against .hkpsx/aspid/CONTRACT.md;
`shared/hk-sim/src/aspid.rs` ports the `spitter` FSM (IdleBuzz roaming,
7.804 alert circle plus sight, DistanceFly at 7 units with 0.1 per fixed step
and 4 units/s, 1.5 to 2.25 s checks of range 14 and a clear ray, Fly Back at
8.25 for half a second, Fire Long anticipation to its trigger frame 9 at 9
units, one FireAtTarget shot at 15 units/s, the 12.1 unalert circle with an
8 s Range Out Timer, no damage transition), sharing IdleBuzz and DistanceFly
through the new `shared/hk-sim/src/buzz.rs` (the Vengefly now uses the same
IdleBuzz). The guest gained its first projectile pool: `EnemyWorld.shots`
(eight slots) holds `Spitter Shot R` bodies flying straight under gravity
0.05; a shot ends on a terrain contact of its movement segment or on a hero
box overlap (one damage through the existing cheat-aware hurt path), plays
Impact for 18 ticks and leaves. The shot Idle and Impact clips are cooked
from the shot library at the prefab scale (0.7 x 0.8) by a shared
`_ClipCooker` that the corpse art now uses too; the fire cue travels as a
`RunnerEventKind::Fire` collected inside `EnemyWorld::tick` and spawned after
the actor pass. `Corpse Spitter` is `massless` (no collider; the source lets
it fall out of the scene) and is removed on its first landing here.
`host/similarity_dedup.py` still capped one cooked view at 256 KiB from the
per-room era; it now uses `quality.ROOM_BYTE_BUDGET` (384 KiB), since the
resident bank is bounded by the scene arena, after a Crossroads_03 view with
the new shot art reached 264 KiB. 118 of 144 placed actors are admitted;
headroom 146,404 bytes (Actor is 400 bytes: the runtime enum fold is due);
seven routes pass. Not presented: shot rotation and stretch, dribble spatter,
Aspid audio.

## One runtime per actor (build 101)

`enemies::Actor` carried six optional controller runtimes side by side; it
now holds one `Runtime` enum (Walker, Runner, Climber, Vengefly, Gruzzer,
Baldur, Aspid) with small accessors, so an actor costs the largest runtime
rather than the sum: `Actor` 400 -> 176 bytes, the 32-slot `EnemyWorld`
13,024 -> 5,856 bytes. The two ShamanTemple Buzzers that carry their body box
twice are admitted (`runner.py::body_box`, shared with the Baldur); the two
duplicate-box Climbers in Crossroads_03 and _05 are not, because their views
sit at the 416 texture budget after similarity dedup and the extra Climber art
pushed one to 421. That is the first time actor art has hit the texture
budget; a cook-side policy that drops an actor's art (leaving it an
unsupported record) when its view overflows is the next step if more
placements need it. 120 of 144 placed actors are admitted; headroom
148,452 bytes; seven routes pass.

## Source camera locks and a damped camera (build 102)

P09 step 3, first slice. The 122 enabled `CameraLockArea` objects of the
catalog now ride in the world bank as kind 11 objects (trigger bounds plus one
two-point polygon with the camera-centre limits; flags 1 preventLookDown, 2
preventLookUp). `host/regions.py::postpack_camera_locks` attaches every lock
whose trigger touches a region and resolves the source `-1` (and 9999) sides
the way `CameraController.LockToArea` does, to the scene camera bounds (14.6 /
xLimit / 8.3 / yLimit in camera-centre units); Crossroads_18 authors an
inverted x pair, which the guest clamps to the sorted pair as Mathf.Clamp
effectively does. The guest's new `camera.rs` replaces the per-view instant
clamp: the camera target follows the hero with the source one-unit look-ahead
and 0.075 s damping, the camera follows the target with 0.15 s damping at
most 30 units/s (per-tick exponential approach with the serialized
CameraTarget/CameraController time constants, not SmoothDamp's spring), both
clamped to the lock whose trigger the hero body overlaps (the last in bank
order, as the source keeps the most recently entered zone) or the view's
camera bounds; a hero displacement over 20 units in a tick (gate, respawn)
snaps. Fall catcher, look up/down, quake and super-dash offsets are not
modelled. `tools/world_metadata.py` joined the cook inputs. Headroom 136,620
bytes; seven routes pass.

## Source gate rules: facing into side gates, delayed top colliders (build 103)

P09 steps 2 and 4. `TransitionPoint.TryDoTransition` only fires a left or
right gate while the Knight faces into it; a recoiling Knight never
transitions (the source zeroes its velocity and translates it back out of the
trigger); top and bottom gates fire on contact. Top gates ship with a disabled BoxCollider2D and a
`Delay Collider` FSM (Wait 3 s, SetCollider) so the arriving Knight cannot
bounce straight back out; in the admitted catalog that is Crossroads_01
`top1` (the Tutorial `top1` leads outside the catalog). `host/regions.py`
records each gate's `collider_delay` and initial collider state,
`host/world.py` emits `Gate.side` (1 left, 2 right, 3 top, 4 bottom) and
`Gate.delay_ticks`, and `world::gate` takes the recoil state and a per-scene
tick counter (`scene_ticks`, reset when the scene changes). The development
`gate_cooldown` (90 ticks after any transition) stays as a guard. Headroom
136,620 bytes; seven routes pass.

## Every gate enters through the source sequence (build 104)

P09 step 2. The Great Door's audited HeroController.EnterScene side sequence
(settle 10 ticks, camera fade after 7 over 30, lead 12, then the forced walk
at RUN_SPEED for 28 ticks with input ignored) now runs for every left and
right gate: `great_door::World` takes the walk-in direction explicitly
(`begin_gate_entry(scene, direction)`; a right exit enters at the
destination's left gate walking right) and no longer marks the Great Door's
one-time entry consumed for ordinary gates. Top gates use the audited top
sequence (`begin_top_entry`: hidden with gravity off through the settle and
lead waits, then the -12 units/s drop with input ignored for 0.33 s;
`.hkpsx/crossroads68/RESULT.md`); the Knight is not drawn while hidden.
Bottom gates keep the immediate placement. The `town-well` and
`crossroads-exit` routes end a few units earlier because their tapes were
recorded with instant control; both still complete. Headroom 134,572 bytes;
seven routes pass. Not modelled: entryDelay and delayBeforeEnter fields
(zero on every admitted gate), super-dash entries, the door-entry animation
and the horizontal enter wait extra.

## Benches: rest, heal and respawn marker (build 105)

P13 step 3, first slice. `host/benches.py` cooks every active RestBench with
a `Bench Control` FSM (Town, Crossroads_04, _30, _47, ShamanTemple; the
inactive Crossroads_01 Ascender is skipped) into world bank kind 12 objects
(trigger box, seat, Knight sit clip base); the Knight's Sit, Sit Idle and Get
Off clips are cooked only into views that contain a bench trigger, at a
constant index after the focus clips. `game/src/bench.rs` follows the source
FSM subset: UP with the hero body inside the trigger while grounded and in
control starts the rest, the Knight slides to the seat over 0.2 s and sits
(Sit then Sit Idle), Rest Burst at 0.5 s heals to full and sets the respawn
marker (scene, seat, facing, slot), any jump/attack/direction press after
0.6 s plays Get Off and control returns 0.85 s later. Death now respawns at
the last bench seat instead of the new-game spawn (standing, not the source
wake-on-bench). Not modelled: the map, charm prompt, sleep, bench tilt, Save
Game (next slice: memory card) and the bench audio. New route `town-bench`
(the town-resident tape cut at the Dirtmouth bench, UP, wait, LEFT) must end
with `HK_BENCH_RESTS` 1 (`REQUIRED` in the build driver); eight routes pass.
Headroom 132,524 bytes.

## Required package completion record

For every validated package, add source contract/version, covered instances and
variants, implementation paths, remaining exceptions, commands actually run,
original-game evidence, guest/disc hashes where applicable, inspected output,
memory/performance effects, persistence/reset evidence and the next package.
