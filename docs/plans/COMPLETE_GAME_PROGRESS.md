# Complete-game execution ledger

This ledger tracks implementation of `COMPLETE_GAME_SOL.md`. A package is
validated only when its acceptance criteria have current evidence. Source
extraction, cooking, runtime binding and playable validation remain separate.

## Current authority

- Source branch: `main`; implementation began from commit `8337d68`.
- Windows source: CrossOver Steam installation, Unity `6000.0.61f1`.
- Playable disc: build 71; Tutorial/King's Pass and Town/Dirtmouth only.
- Disc policy: preserve build 71 until the user requests a replacement.
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
| P05 | in progress | Tutorial/Town `HKWMTA01` banks are checked and admitted from the aligned scene-arena tail; spatial actor/checkpoint lookup now resolves through the admitted bank. The guest differentially checks bounds, collision bounds, camera and neighbor lists against legacy regions, and derives the pack index/header capacity from generated manifests. The filtered edge cache now keys by world generation, global region and room view, preventing equal-sized room payload reuse. Full actual-CUE differential replay and migration of auxiliary tables remain. |
| P06 | in progress | `tools/world_pack_matrix.py` joins all 501 verified scene artifacts, dependency inventories and current+neighbor windows, with deterministic hashes and source-cost peaks. It intentionally leaves PS1 RAM/VRAM admission unknown. Production cooking of all scenes, shared art canonical mappings and native pack validation remain. |
| P07 | in progress | Persistent Geo state is now a flat 256-entry source-state pool keyed by scene and authored rock slot, covering the verified 207 GeoRock instances without a `[scene][32]` allocation. Version-2 host plans preserve all 850 directed gates, and the bounded room-residency runtime can safely evict only completed slots outside a keep-set. Native residency/Geo tests pass and the no-disc link is hazard-free with 16,232 bytes before the stack floor. Predictive CD residency and guest streaming admission are still pending. |
| P13 | in progress | Shared persistent stores now have a versioned `HKSV` fixed-buffer snapshot codec for global and mode owners, with strict length, ordering, checksum and capacity validation; active-scene flags reset on load. Memory-card I/O and actual quit/reload routes remain pending. |
| P08 | in progress | Tile-coverage event chains now separate back and front owners, preserving front masks through the complete back pass. The first regression and full hk-format coverage suite pass. Material-family completion, black-fill source coverage, dynamic mask invalidation, actor/effect cache expansion and representative visual replay remain. |
| P09-P12, P14-P31 | pending | Follow dependency order in the master plan. |

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

## Required package completion record

For every validated package, add source contract/version, covered instances and
variants, implementation paths, remaining exceptions, commands actually run,
original-game evidence, guest/disc hashes where applicable, inspected output,
memory/performance effects, persistence/reset evidence and the next package.
