# Complete Hollow Knight on PS1: execution plan for GPT-5.6-Sol

Status: implementation plan, not a completion claim. Prepared on 2026-09-15
against source commit `ecc697e` on `main` in `/Users/ebonura/Desktop/repos/hk-psx`.
The intended executor is GPT-5.6-Sol. This document deliberately gives small,
verifiable work packages so that the executor can carry out sustained work
without repeatedly rediscovering the project or narrowing the objective.

## 1. The actual assignment

Complete the native PS1 port of the installed Windows version of Hollow Knight.
Import the whole world, implement shared systems across it, and make the game
playable from a normal new game through its story, optional content and endings.
An opening demo, many extracted scenes, one working boss, or one ending does
not satisfy this assignment.

Follow this order of emphasis:

1. Recover and finish whole-world extraction and content accounting.
2. Remove architecture limits that prevent the world from being packaged and
   loaded on a PS1, while preserving the existing playable game.
3. Make the world loadable and its authored connections functional.
4. Implement shared gameplay systems across all affected scenes.
5. Close progression, bosses, quests, optional content, presentation and modes.
6. Validate complete playthroughs and hardware constraints.

A system can use a representative room as its test fixture. That does not make
that room the unit of development: after validating the system, apply it to
all source instances that satisfy its verified contract and report exceptions.
Do not spend a long sequence of updates perfecting one room while the rest of
the world remains outside the pipeline.

The immediate user request is to prepare this plan. It does not authorize
starting an implementation run or replacing the user's disc during planning.
When the user hands execution to the next agent, continue from the first
incomplete task below. Do not regenerate this plan instead of doing that task.

## 2. Definition of complete

Maintain separate evidence for each requirement. No single percentage, passing
build, inventory scan or smoke replay can prove all of them.

| Requirement | Evidence needed before claiming completion |
| --- | --- |
| World content | Every installed scene classified; every gameplay scene and reachable variant has a validated pack, supported content records and an explicit place in the transition/progression graph |
| Authentic start | Normal menu, opening flow, first landing, initial equipment and control handoff work without debug setup |
| Movement and combat | All installed movement abilities, spells, nail arts, upgrades, damage rules and interaction contracts work against source-derived tests |
| Progression | New-game routes reach every ending and all optional content without cheats, test teleports, synthetic grants or bypassed prerequisites |
| Enemies and bosses | Every required source archetype and variant has working movement, attacks, hitboxes, damage, death, rewards, audio and effects; boss/arena retry paths work |
| Collection and economy | Items, charms, notches, fragments, resources, Geo, shops, quest rewards, journal and map systems obey source conditions and persistence |
| World interactions | Breakables, moving objects, hazards, secrets, benches, NPCs, stations, lifts, switches and scripted events function throughout the world |
| Persistence | Save/load, benches, death, Shade, quitting, revisits, mode-specific resets and interrupted writes preserve the correct state |
| Presentation | All required layers, masks, animations, particles, sound cues, ambience, music, dialogue, cinematics and UI have a working PS1 representation |
| Modes and optional content | Installed content expansions, optional bosses/quests/challenges, alternate endings and unlocked modes are accounted for and tested |
| Performance | 320×240 rendering targets stable 30 fps with responsive 60 Hz simulation, supported by representative and worst-case measurements; hardware claims have hardware evidence |
| Loading | Traversal meets the user's seamlessness objective; required disc reads, decompression and VRAM uploads are scheduled ahead of need, with measured worst-case transitions |
| Reliability | No corrupt packs, stale scene pointers, silent required-object drops, buffer overflows, audio underruns or progression softlocks in the validation matrix |
| Reproducibility | A fresh authorized Windows install can produce the same source-bound content and a bootable final build using documented tools |
| Delivery | One playable BIN/CUE pair in the specified library, source-only repository, clean handoff and exact build/provenance/validation records |

Use the installed source's completion calculation and content catalog as the
exhaustive authority. A commonly known completion percentage is not an adequate
substitute for extracting the contributing flags and items. Verify that maximum
counter, but also test content that does not contribute to it. In particular,
one story ending is an intermediate milestone; optional quests, challenge
content, alternate outcomes and modes still belong to the final audit.

Classify non-gameplay scenes honestly: menus, movies, loading helpers, additive
bootstrap scenes, unused/test scenes and state-dependent variants. Retain their
records. Excluding a scene from playable-room counts requires source/reference
justification, not a name prefix or the fact that it currently fails to cook.
Platform services such as Steam integration need an explicit PS1-appropriate
policy; their absence must not erase in-game unlocks or completion conditions.

## 3. Non-negotiable working constraints

- Work in this repository. Read `AGENTS.md`, `README.md`, `docs/BOOTSTRAP.md`,
  `NEXT_PROMPT.md` and the top of `docs/STATUS.md` before editing.
- Windows Steam CrossOver source only, read-only. Never fall back to macOS.
  Re-run `python3 tools/doctor.py` after resuming or source updates.
- Original reference runs use the isolated copy and isolated saves created by
  `tools/reference_game.py`. Never instrument the retail installation itself.
- Use Rust and the existing SDK for the guest; keep Unity/Python/extraction
  libraries entirely on the host.
- Current tested SDK revision is `7b929ce473ed44b10b0ffca08f413eebf4eeeec4`;
  current toolchain is `nightly-2026-03-25`. Change either only for a demonstrated
  reason with a fresh validation record.
- Keep 320×240, the established full-screen framing, current layers/effects,
  the 48-pixel scenery cap and the approved 95% static texture sharing policy.
  Do not lower the framebuffer resolution to escape a budget failure.
- Preserve actor/HUD animation sampling. The static scenery cap is not permission
  to apply that cap indiscriminately to Knight/enemy animation or interface art.
- The user permits lower audio sample rates for long samples when applied
  consistently by sound category. Preserve timing, duration and loop behavior.
- Exactly one playable pair:
  `~/Downloads/ps1 games/hk-psx.bin` and `hk-psx.cue`.
  Every temporary/diagnostic disc also belongs in that library and must be
  removed by the replacement workflow. No candidate/baseline/backup disc copies.
- Current playtest disc71 remains untouched until the user requests/authorizes
  its replacement. A source-only commit does not imply the disc was rebuilt.
- The user opens PSoXide's GUI. Use headless guest runs; do not drive its GUI.
- Source-only pushes to private `EBonura/hk-psx` have been authorized. Commit
  only code/docs/tests. Assets, imported components, decompilations, images,
  traces, converted audio and discs remain ignored and must never be pushed.
- No automatic burns or publishing of assets. Hardware testing needs the user's
  actual console workflow. Lack of hardware evidence is not hardware success.
- Follow the `psoxide-debug` skill for guest, emulator and hardware work, while
  checking current project commands rather than copying older sibling flags.
- Keep integer/fixed-point ranges, event queues, pools, decompression and memory
  ownership bounded. Overflow or content rejection must be visible and actionable.
- Report what is source-derived, approximated, implemented, guest-bound and
  actually validated. Those words are not interchangeable.

The user later paused generic FPS optimization to test and expand gameplay.
Do not restore an indefinite "nothing ships until 30 fps" gate from historical
ROADMAP text. Track and repair regressions during expansion, then meet the final
performance requirement as part of completion. The current build is not proven
stable 30 fps on hardware.

## 4. Verified starting state and where to find it

### 4.1 Repository and playable build

At plan creation, `main` and the preceding source handoff are `ecc697e`:
`Add resumable whole-world source import and coverage reporting`.
Recheck Git rather than assuming that remains HEAD after execution starts.

Playable disc71 contains Tutorial/King's Pass and Town/Dirtmouth, partitioned
into 106 spatial views. These are two scenes, not 106 original rooms.

| Artifact | SHA-256 |
| --- | --- |
| `dist/hk-psx.exe` | `d3c4caf1f2dbdf1f1357ef65809af5631c6a86635b2b07e8e27eff8c27855201` |
| Downloads `hk-psx.bin` | `4fa805dfa4aeec272d743b8d2ce9721ded164971f43d5ff02ba99e7b02b52d1a` |
| Downloads `hk-psx.cue` | `02b422df61db9e49567c088712422c6af982f66ccfba6edded1609cc9ca60248` |

Use `.hkpsx/build.json` as the build/artifact authority. It is large; extract
specific fields with Python instead of printing the entire file.

Existing playable systems include basic Knight movement/nail combat, three
Crawlers, health/SOUL/Focus, Geo and Geo rocks, source tutorial tablets, grass,
breakables/debris, Lifeblood/Scuttlers, the Great Door and Tutorial↔Town travel,
menus/cheats/volume controls, some SFX/ambience and CD-streamed title music.
These implementations have documented limits; do not infer complete source
parity from this list. Full saves, progression, abilities, charms, NPCs, bosses,
world connectivity and gameplay music are not finished.

Disc71 passed its final build and zero-hazard check, plus an 850-poll CUE smoke
with an inspected image matching70. Long gate/return/reset evidence belongs to
build69 unless a newer run explicitly repeats it. Last host change passed344
Python tests; those are host/native integration checks, not 344 gameplay routes.

### 4.2 Memory and hard-coded limits

Disc71 linked measurements: code391,628 B, data720,432 B, BSS862,100 B;
static span1,974,164 B; 49,152 B reserved stack; only8,044 B gap before that
reservation. `main` uses33,728 B; nested reserve is15,424 B, with no measured
main-stack high-water. Older `docs/BUDGET.md` tables describe69 and must not be
mistaken for current71 figures.

One354,032-byte geometry/animation/metadata arena and one86,016-byte coverage arena are
already reserved. Current static VRAM supports20 scenery pages and1,248 CLUTs;
Tutorial uses18 pages/1,068 palettes, Town6/390. These budgets do not prove that
any larger scene fits. The existing VRAM reservation has very little slack.

Concrete growth barriers include:

- `host/regions.py`: fixed two-scene catalog and measured region layout.
- `host/world.py`: `SCENES = 2`, contiguous region IDs and a256-region limit.
- `game/src/world.rs`: `SCENES = 2`, linked `&'static` region metadata and arrays
  sized by scenes×maximum objects.
- `game/src/disc.rs`:256 pack-entry slots; increasing scenes also adds atlases,
  coverage and audio chunks, so one entry per scene is not the actual total.
- Region-indexed generated Geo, Lifeblood, Great Door, dialogue, scenery budget,
  disc mapping and texture tables outside `world::Region`.
- `world::State` caches raw region/room addresses; those cannot identify data
  after an arena is reused for a different scene.
- Current fixed animation, actor, particle, draw and effect pools need measured
  full-world requirements and admission policies, not arbitrary count increases.

### 4.3 Whole-world import: incomplete, recoverable work

The source BuildSettings catalog has501 scenes. The older complete dependency
inventory records21,741 sprite fragments,2,615 source textures and313 animation
libraries. Its1,409 recorded source file hashes matched at the previous run.
These are source dependency counts, not unique cooked PS1 texture costs.

The new run in `.hkpsx/world-import/` was stopped at the user's request:

- 249 scenes exported:2 with no reported geometry gaps,247 partial.
- 196 additional jobs recorded pool failures after a worker terminated abruptly.
- 56 scenes had no recorded result.
- The pool's first reported failed scene was `Mines_02`, but asynchronous scheduling
  does not prove that scene caused the worker termination.
- The cause is unknown. Do not label196 scenes individually corrupt.
- Final source/code verification and fresh graph finalization were not reached.

The snapshot contains207,738 SpriteRenderer instances,205,556 extracted quads,
8,143 default tk2d frames,60,558 terrain segments,1,454 extracted world meshes,
688 camera locks and583 gates across the exported scenes. Counts include
inactive alternatives. Summed per-scene `unique_sprites` is not a world-wide
unique texture count.

Existing implementation:
`host/world_import.py`, `host/world_geometry.py`,
`tools/world_import_report.py`, `tests/test_world_import.py`,
`tests/test_world_geometry.py`, `docs/WORLD_IMPORT.md`.
Read `report.json`, `stopped.json`, `import.log` and per-scene `result.json` files.
Compressed geometry/components are host records, not PS1 packs.

### 4.4 Existing unfinished system work to reuse

- `shared/hk-sim/src/runner.rs` and `runner_senses.rs`: source-derived Runner
  state machine/senses, including synchronous restart ordering and tests.
- `game/src/enemies.rs`: Runner guest branch, damage/corpse/Geo and ordered
  presentation events. Canonical source admission is still disabled.
- `game/src/main.rs::unbound_runner_event` intentionally panics. Do not enable
  Runners in playable packs before installing a real presentation owner.
- `host/runner_audio.py`: prepared20,464-byte full-sample bank at SPU495,792..
  516,256, leaving8,032 B if bound. Proposed voices21/22/23 need arbitration;
  the bank is not loaded or played by the current guest.
- `shared/hk-sim/src/climber.rs`: source-derived pure Climber controller, not
  bound to guest actors. Outside turns have original traces; inside turns,
  stun and death still need native observation.
- Isolated Crossroads_01 full-scene pack:12 views,98,808 B resident geometry,
 7 static pages,501 palettes. This is a capacity probe, not playable admission.
  Well behavior, camera locks, Secret Mask, conditional Mender and actor/effect
  bindings remain incomplete.
- `.hkpsx/world-metadata70/PLAN.md`: detailed relocation design for `HKWMTA01`.
  It predates71's larger actor code. Reuse its lifetime/state analysis; remeasure
  sizes rather than accepting its old headroom numbers.

### 4.5 Code map for the executor

| Work | Existing entry points |
| --- | --- |
| Unity serialization/hierarchy | `host/source.py`, `host/scene.py`, `host/inspect_source.py`, `host/inspect_il.py` |
| All-scene import/dependencies | `host/world_import.py`, `host/world_geometry.py`, `host/room_inventory.py`, `host/room_graph.py`, `host/residency_plan.py` |
| Scenery/packing/dedup | `host/cook.py`, `host/regions.py`, `host/scene_bank.py`, `host/pack_scenes.py`, `host/similarity_dedup.py`, `host/texture_dedup.py` |
| Tile fills/materials/visibility | `host/tilemap_fill.py`, `host/materials.py`, `host/reveal_masks.py`, `host/scene_certificates.py`; guest `render.rs`, `scenery_*`, `opaque_*`, `reveal_masks.rs` |
| Scene loading | guest `disc.rs`, `cd_stream.rs`, `room_decode.rs`, `room_residency.rs`, `texture_upload.rs`, `vram_cache.rs`, `scene_transition.rs` |
| Hero/core simulation | `shared/hk-sim/src/lib.rs`, `combat.rs`, `nail_response.rs`, `vitals.rs`, `focus.rs`; guest `input*`, `presentation.rs`, `main.rs` |
| Actors/corpses | `host/actors.py`, `host/runner.py`, `host/effects.py`; shared `actors.rs`, `corpse.rs`, `runner*`, `climber.rs`; guest `enemies.rs` |
| World interactions | `host/world.py`, `breakables.py`, `hazards.py`, `geo.py`, `lifeblood.py`, `great_door.py`; guest matching modules plus `world.rs` |
| Effects | `host/particles.py`, `break_effects.py`; guest `particles.rs`, `debris.rs`, `impact.rs`, `break_effects.rs` |
| Audio/music | `host/cook_audio.py`, `cook_music.py`, `ambience.py`, `focus_audio.py`, `runner_audio.py`, `title_music.py`; guest `audio*`, `ambience*`, `focus_audio*`, `music.rs` |
| Menus/text | `host/cook_menu.py`, `cook_hud.py`, `read_points.py`; guest `menu*`, `pause.rs`, `cheats.rs`, `hud*`, `dialogue.rs` |
| Builds/validation | `host/hk-build/main.rs` (root Cargo driver), `host/build_guest.py`, `host/build_report.py`, `host/stack_budget.py`, `tools/replay_cue.py`, `tools/validate_scene_gates.py`, `tools/validate.py`, `Makefile` |
| Original oracle | `tools/reference_game.py`, `tools/reference/Driver.cs`, `EnemyTrace.cs`, other managed probes, `docs/ORIGINAL_REFERENCE.md` |

Paths mentioned later as **proposed** do not exist yet. Create them deliberately;
do not call hypothetical CLIs or claim their checks already ran.

## 5. Operating method for Sol

### 5.1 A work package, not a vague turn

For each package below, use this loop:

1. Read its dependencies and current evidence; select one bounded missing slice.
2. Record a concrete before/after behavior and the files likely to change.
3. Read source data/CIL and obtain an original trace when behavior is uncertain.
4. Implement a shared solution and apply it to all verified matching instances.
5. Test semantics and failure paths, then run guest integration when applicable.
6. Inspect captures/audio and the final artifacts. Do not equate exit0 with
   correct output or a successful cooker with gameplay readiness.
7. Update coverage, budget, evidence and remaining exceptions.
8. Commit a coherent source-only change and continue to the next slice.

Do not hide behind never-ending infrastructure. Every infrastructure package
must say which playable/system change it enables and prove that dependency.
Conversely, do not add a room-specific hack solely to produce a screenshot.

Use a tracked execution ledger (proposed `docs/plans/COMPLETE_GAME_PROGRESS.md`)
for human decisions and an ignored machine ledger (proposed
`.hkpsx/completion/coverage.json`) for source IDs, generated assets and evidence.
Maintain one current next task in `NEXT_PROMPT.md`; keep history below it.

### 5.2 States and evidence schema

Each scene, object family and feature needs separate status fields:

- discovered; imported; dependencies resolved; source contract documented;
- cooked; format validated; fits active budgets; runtime bound;
- connected; behavior validated; persistence validated; presentation validated;
- end-to-end validated; hardware validated when required.

A useful record includes source version/hash, stable source ID, controller/type,
instance/scene coverage, implementation files, unresolved variants, original
trace paths, guest replay paths, relevant build hashes, measured budgets and
current failure reason. Use explicit `not_run`, `unsupported`, `failed` and
`passed`; avoid an ambiguous single `complete` boolean.

For system tasks, record how many matching instances exist, how many are admitted
and the exact remaining exceptions. For counts, distinguish source objects,
room occurrences, canonical definitions, runtime instances and packed assets.

### 5.3 Checkpoint and continuation discipline

A handoff should say: current source commit, current disc hash, selected package,
changed files, commands actually run, exact results, live process/session handle
if any, failures, next command and what it should prove. A log path is not proof
that a process is still running. Inspect the actual handle before waiting or
restarting. After a user stop, terminate owned processes, preserve atomic
checkpoints and save the handoff. Do not silently restart them.

Never spin through the same failed build/import without changing the hypothesis
or collecting new evidence. Narrow a reproducer while preserving the full target.
Do not loosen a verifier merely because it rejects the implementation; check its
source contract first and preserve evidence for any justified validator change.

If delegation is authorized, separate owners by subsystem or host/guest/test
boundary. One owner performs canonical cooking/disc replacement and integration.
Do not have two agents edit `main.rs`, generated canonical data or the final disc
concurrently. Read-only audits and fixture preparation can proceed independently.

## 6. Milestones and dependency order

| Milestone | Packages | Visible outcome |
| --- | --- | --- |
| M0: recover import | P00–P03 | All source scenes accounted for, failures isolated, exhaustive work inventory |
| M1: scalable content | P04–P08 | Whole-world packs and bounded loadable scene data, no two-scene table ceiling |
| M2: connected world | P09–P12 | Authored traversal and reusable scripting/interactions work across the catalog |
| M3: persistent adventure | P13–P18 | New game, saving, abilities, inventory, economy, NPCs and transport form a real play loop |
| M4: full combat roster | P19–P22 | All actor families, bosses and arena challenges use shared runtime systems |
| M5: complete content | P23–P27 | Story/optional progression, presentation, music, modes and ending paths work |
| M6: release proof | P28–P31 | Performance, full routes, hardware and reproducible delivery are audited |

Milestones are checkpoints, not permission to postpone all audio/UI until the
end. Each actor or interaction includes its own audiovisual events and state
cleanup when integrated; P25/P26 perform world-wide closure and mixing/quality
work. Likewise, persistence keys are designed early even before memory-card UI.

P04/P05 can advance while P03 closes exceptional geometry. Source imports must
not depend on gameplay completeness. Production admission does depend on the
required systems: use a clearly marked development room viewer for unsupported
rooms rather than shipping invisible enemies, fake unlocked gates or no-op FSMs.

## 7. Detailed execution packages

### P00 — Establish a trustworthy baseline

**Depends on:** none. **Primary files:** current handoff, build report, Git state.

1. Read the first-read documents and this plan; identify later user steering.
2. Check Git status/branch/remotes and current disc files/hashes. Preserve user
   edits. Do not reset, clean ignored assets or recook as a reconnaissance step.
3. Re-run doctor. Record source directory/version and SDK/toolchain identities.
4. Confirm no prior importer/emulator/reference process remains active.
5. Verify the saved344-test log and relevant disc71 replay evidence exist; do
   not count historical tests as current after changing their covered code.
6. Create the execution ledger with M0–M6 and P00–P31, all future work pending.
7. Record the authoritative playable coverage as two scenes and the import
   failure as unresolved. Make the first implementation task P01.

**Acceptance:** another agent can identify source vs playable state from one
handoff; no disc/source-input mutation occurred; the next task is unambiguous.

### P01 — Repair import orchestration and recover the stopped pass

**Depends on:** P00. **Primary files:** `host/world_import.py`, its tests/logs.

1. Read the import log around the first lost worker and inspect checkpoint
   timestamps. Determine which jobs were running concurrently; do not assume
   `Mines_02`, the first failed future reported, was the culprit.
2. Reproduce with one worker and process/resource diagnostics. Check exit code,
   signal, memory, temporary disk use and native extractor errors. A segfault,
   out-of-memory termination and a Python exception need different fixes.
3. Add bounded per-job scheduling and record worker PID/scene/start/exit details.
   Avoid submitting all remaining work into a pool that can die as one unit.
4. Preserve completed atomic outputs. Mark pool-cancelled jobs as pending/retry,
   not individually corrupted source scenes. Stop/recreate the pool in a bounded
   way after a worker death and isolate the offending scene for diagnosis.
5. Add graceful interruption: finish or discard the current atomic artifact,
   stop owned children, record an interrupted status, retain usable checkpoints.
6. Add resource/time limits based on observations, not arbitrary silent skips.
   A scene exceeding them remains an explicit unresolved job with a reproducer.
7. Verify resume rejects modified output bytes and mismatched source/reader
   fingerprints, retries real failures, and produces deterministic artifacts.
8. Note the present fingerprint includes every host Python file. Code changes
   currently invalidate all checkpoints. Initially honor that behavior; if
   narrowing dependencies, document an exact dependency closure and test that
   relevant serializer/extractor changes still invalidate affected outputs.
9. Resume single-worker processing. Parallelize only after the failure cause
   and safe per-worker memory are known. Complete final source/code verification.

**Acceptance:** every501 catalog entry has a trustworthy result or a individually
reproduced extraction failure; no pool failure is misreported as scene corruption;
resume and interruption tests pass; final input/code checks pass. P02/P03 own
remaining content errors—do not mark them solved by the orchestration fix.

### P02 — Build an exhaustive world and progression catalog

**Depends on:** P01. **Primary files:** room inventory/graph/import, proposed
coverage generator and content manifests.

1. Index all BuildSettings scenes with stable file/build IDs and exact names.
2. Classify gameplay rooms, additive helpers, menus, cinematics, dream/boss
   variants, end states and unused content with evidence for every exclusion.
3. Extract literal TransitionPoints, trigger shape, source transform, entry
   offsets, orientation, delays, fade flags and activation conditions.
4. Decode other scene-load routes: well FSMs, dreams, stations, elevators,
   benches/respawns, boss arenas, cinematics, ending scripts and title transitions.
5. Distinguish source-default adjacency from effective runtime targets. Existing
   Town well data demonstrates that serialized target strings can be overridden.
6. Generate directed edges with prerequisites and postconditions. Report unknown
   targets, duplicate gate names, unresolved overrides and one-way transitions.
7. Build catalogs for actors, bosses, interactables, items, shops, quests, audio,
   animations, particle systems and PlayerData/FSM state consumers.
8. Extract the installed completion/unlock logic into an auditable checklist.
   Associate every contributing flag with its producer and all consumers.
9. Generate reference cases for each connection/condition and distinguish
   reachable current-state variants from inactive potential content.

**Acceptance:** the scope of the whole game is explicit; no scene is omitted
because of cooker limitations; every gameplay edge and content type is mapped
or explicitly unresolved with its owning task. A graph traversal is a planning
check, not proof that the guest can physically traverse it.

### P03 — Complete source geometry and dependency extraction globally

**Depends on:** P01/P02. **Primary files:** `world_geometry.py`, `source.py`,
`scene.py`, `room_inventory.py`, tilemap/material readers.

1. Group extraction gaps by type/reason/affected scene count. Fix the highest
   reusable gaps first, preserving per-instance exceptions.
2. Distinguish null runtime-generated MeshFilters, tk2d-generated mesh geometry,
   Unity built-in meshes and missing/corrupt asset references. Avoid duplicate
   geometry when a tk2d source already supplies the generated mesh.
3. Add remaining sprite modes and required renderer types from observed data:
   sliced/tiled/clipped sprites, native animation-driven renderers, lines/trails,
   meshes and particle renderers. Preserve source UVs, order, colors and transforms.
4. Preserve exact collider geometry and metadata, including circles/capsules,
   composite shapes, offsets, trigger masks and moving-parent relationships.
   Host shape extraction is separate from implementing the guest solver.
5. Extract particle parameters, emitter transforms, event links and dependencies;
   the current Scene adapter deliberately omits native particle payloads.
6. Resolve animation clips/events, inactive alternatives, prefab hierarchies,
   Resources/dynamic asset references and spawned-object dependency closure.
   Detect cycles and share canonical definitions instead of expanding infinitely.
7. Separate immutable geometry from script-controlled visibility/transforms.
   Retain inverse/remasker helpers as conditional content; never render them as
   unconditional scenery or delete them from the source model.
8. Compare new Tutorial/Town geometry with existing cooked provenance. Repair the
   previous comparison tool's missing-`base_path` assumption using the actual
   per-region provenance mapping; do not skip regions that lack that field.
9. Validate representative exported scenes from every renderer/material/shape
   family against the original and verify all source-backed references resolve.

**Acceptance:** all gameplay scene geometry/dependencies are extractable or have
explicit remaining cases; no dropped mesh, inactive variant or prefab dependency
is hidden by a nominal successful import. Preserve local proofs and source hashes.

### P04 — Define stable IDs, ownership and persistent state

**Depends on:** P02; coordinate before P05–P18 formats become permanent.

1. Define source identity independently of build ordering: source scene/asset
   identity plus object ID and, for spawned instances, stable prefab/instance
   identity. Document source-version changes and migration behavior.
2. Separate global persistent flags from active-room/transient state. Do not
   multiply the current416 B/scene state allocation by the whole scene catalog.
3. Catalog original reset policies: scene exit, bench rest, death, quit/load,
   dream return, challenge reset and mode reset. Derive them per object family.
4. Keep current sparse breakable/grass IDs during the first relocation. Current
   IDs use scene×128 and scene×1024 respectively; list positions are not IDs.
5. Design a compact, measured global flag store plus bounded active-scene state
   and active timers. Preserve persistent fades and delayed events correctly
   when the source says they survive a transition.
6. Make collection/reward/kill/quest transitions idempotent. A scene revisit,
   duplicate regional binding or retry must not duplicate rewards or erase flags.
7. Define a central state mutation/event API with stable save schema IDs rather
   than independent per-feature booleans with inconsistent side effects.

**Acceptance:** reset/persistence fixtures cover scene leave/return, death and
reload; duplicate object occurrences affect one owner; state costs are measured;
source order changes do not silently remap collected items or progression.

### P05 — Move world metadata out of permanently linked tables

**Depends on:** P04. **Primary files:** `host/world.py`, `game/src/world.rs`,
`disc.rs`, `shared/hk-format`; reuse the detailed metadata70 plan.

1. Re-measure current linked allocations, anonymous arrays and call frames.
   The old20,352-byte Region-root figure predates later changes and excludes
   referenced arrays; it is not a guaranteed reclaimable total.
2. Implement a versioned little-endian offset format such as the proposed
   `HKWMTA01`: fixed header, typed sections, counts/strides, checked element spans,
   source/scene identity, total size, alignment and corruption checks.
3. Encode region directory, object/state definitions, local draw/edge bindings,
   grass, masks/reveals, hazards/checkpoints/polygons, actors/corpses, debris,
   particle styles/curves/emitters, floor heights and neighbor lists.
4. Use explicit wire scalar sizes and flags. Never serialize native Rust slices,
   `usize`, `Option` layout or pointers and cast them back on PS1.
5. Implement allocation-free checked views/iterators with lifetimes tied to the
   owning scene bank. Return copied scalar outcomes where a borrow would escape.
6. Replace edge-cache address identity with scene generation plus region ID and
   relevant mutation version. Invalidate before any arena overwrite, including
   reload/retry of the same scene at the same address.
7. Migrate every auxiliary generated table, including Geo/Lifeblood/dialogue/
   Great Door/render-budget/disc mapping dependencies. Moving Region alone is
   not enough to scale to the full game.
8. Remove fixed catalog and256-entry assumptions using measured hierarchical
   directories or loadable index pages. Check all chunk/index width bounds.
9. Allocate the new bank only after measuring encoded section sizes and removing
   replaced linked storage in the same comparison. Avoid temporarily retaining
   both complete representations in a production binary that barely fits.
10. Differentially compare old/new Tutorial/Town state, geometry bindings, resets
    and scene admissions; exercise corrupt spans and stale-generation access.

**Acceptance:** the guest's permanent memory does not grow linearly with all
room metadata; old routes still work; invalid banks are rejected safely; actual
link map and stack checks pass; there are no surviving borrowed pointers after
scene replacement. Document remaining fixed limits with measured justification.

### P06 — Cook all scenes and shared art without gameplay admission gates

**Depends on:** P02/P03; coordinate formats with P05.

1. Parameterize hard-coded cooker/postpack paths and catalogs. Support explicit
   output roots and scene lists without mutating canonical Tutorial/Town data.
2. Extract shared source definitions once, then bind instances per scene.
   Separate static scenery, animation libraries, UI, audio and effect resources.
3. Apply existing scenery sampling rules consistently. Keep original instance
   transforms/parallax and actor animation quality. Do not silently cull objects
   because their gameplay controller is unsupported.
4. Maintain distinct host preview/geometry packs and production-ready packs.
   Unsupported required actors or scripts may appear in development reports;
   production admission must not silently discard them.
5. Cook all animation frames needed by each clip/event/state, including death,
   attack, recovery, damaged and transformation phases, not just defaults.
6. Apply exact dedup, approved95% similarity and palette-plane sharing with
   reproducible canonical mapping. For multi-member clusters, verify every
   member against the canonical representative; transitive chaining alone can
   merge textures that are not95% similar to the representative.
7. Preserve alpha coverage, silhouettes, material/blend class, UV/orientation
   and CLUT ownership. A different palette is safe sharing only when the indexed
   texel plane and per-instance palette interpretation actually agree.
8. Produce per-scene inventories and current+neighbor unions, including worst
   simultaneously visible animation/effect sets and state-dependent alternatives.
9. Measure compressed bytes, resident bytes, texture pages, palettes, primitive
   demand, dependency closure and decode/upload work. Split oversized residency
   partitions without changing camera coverage, collision or world coordinates.
10. Validate every produced pack with the native reader and corruption tests.
    No truncation, wraparound IDs or placeholder capacity numbers.

**Acceptance:** whole-world cooking has an exhaustive pack/budget matrix;
shared art has verifiable canonical mappings; every over-budget scene has a
specific plan; unsupported gameplay no longer blocks source/art extraction.

### P07 — Build the bounded streaming and residency system

**Depends on:** P05/P06. **Primary files:** disc/CD/cache/decoder/upload modules,
`host/residency_plan.py`, scene packer and generated manifests.

1. Measure largest active geometry, metadata, coverage, animation, audio and
   decompression scratch requirements together. Reserve based on the combined
   peak, including stack and DMA constraints, not each subsystem in isolation.
2. Use RAM as a compressed backing cache for nearby assets where measured costs
   permit. RAM is not directly sampleable GPU texture memory: schedule explicit
   decode and VRAM upload before the corresponding primitive is submitted.
3. Precompute scene dependency sets, common assets and directed-edge fetch/keep/
   evict plans. Include conditional exits, elevators, dreams and fast travel.
4. Prioritize current visible assets, imminent frames and chosen exit data before
   speculative neighbors. Pin GPU/in-flight resources until use completes.
5. If current plus every neighbor does not fit, preserve the priority order and
   use bounded compressed subsets/predictive fetch. Do not promise universal
   simultaneous residency from the source inventory alone.
6. Put related compressed assets contiguously on disc and measure seek/read cost.
   Replace per-object blocking reads with bounded service steps, not a new driver
   unless the SDK demonstrably lacks a required primitive.
7. Define explicit read→verify→decode→upload→admit state transitions. Keep old
   content valid until replacement is ready; handle cancellation and retries.
8. Keep input sampling and required audio service running during transfers.
   Do not run accumulated loading time as a burst of gameplay ticks afterward.
9. Exercise reverse movement, rapidly changing exits, large scene entries,
   return gates and cache-pressure cases. Record misses, queue depths, required
   asset deadlines, upload bytes and stall duration.
10. Retain the current safe blackout loader as a recovery/development path while
    building prefetch. It is not the final seamlessness success criterion.

**Acceptance:** required visible content is resident before use, no stale atlas/
CLUT access occurs, mid-scene traversal avoids blocking loads, transition timing
is measured and the remaining stalls are explicit. RAM/VRAM budgets hold under
worst-case neighbor and animation pressure.

### P08 — Make rendering correct across the full catalog

**Depends on:** P03/P06/P07. **Primary files:** material/tilemap/reveal cookers,
renderer, geometry, visibility and animation caches.

1. Classify all source shader/material families and their actual visual roles.
   Implement faithful PS1 representations for required families, preserving
   layers, sorting, tint, alpha, additive/subtractive behavior and parallax.
2. Fix black-fill holes by tracing source tilemap/mesh coverage and material
   semantics. Do not paint arbitrary screen-space polygons over gaps.
3. Apply precomputed occlusion only when the visibility proof remains valid.
   Dynamic breakables, moving masks, fades, animated silhouettes and camera
   locks must invalidate or bypass incompatible certificates.
4. Generalize reveal/secret masks and animated foreground/background helpers.
   Preserve objects needed after a switch, hit or persistence change.
5. Extend actor/effect texture caching for full frame libraries and simultaneous
   bosses/minions without stale frames or invisible entities on cache misses.
6. Bound primitive demand using actual worst-case scene and dynamic content.
   Required gameplay objects cannot disappear because a packet pool filled.
7. Compare multiple fixed cameras and traversals from representative art families,
   with effects both idle and active. Include the earlier user-reported holes.
8. Validate full-screen gameplay/title transitions, display buffer ownership,
   draw origins and VRAM reuse to prevent the previous black-screen failures.

**Acceptance:** every required renderer family has a tested representation;
geometry never disappears during residency changes; approved scene images and
mask/occlusion invalidation cases pass. List remaining visual approximations
rather than claiming shader-level equivalence with the original.

### P09 — Connect authored scenes and implement camera/entry behavior

**Depends on:** P02/P05/P07; requires the relevant renderer support from P08.

1. Replace the two-scene gate list with source-bound transitions and per-entry
   behavior descriptors. Support horizontal, vertical, door, dream, transport
   and scripted transitions without conflating their control/fade rules.
2. Keep destination resolution separate from physical entry placement. Apply
   source offsets, collision grounding, initial velocity, facing, forced walk,
   landing, fades and control delays in their correct order.
3. Implement camera global bounds, authored lock areas, dead zones/look behavior,
   vertical transitions, boss locks and script overrides. No guessed clamp
   should be promoted from a capacity probe into production behavior.
4. Use the existing Town well case as an early integration test: effective
   Town well→Crossroads_01/top1, landing placement/velocity and return→Town/bot1
   must follow source FSM behavior, including the delayed top-gate collision.
5. Remove development world-boundary teleport fallbacks. Uncovered coordinates
   should be diagnosed during development; production must load the right scene
   or use the authored boundary rather than silently returning to an old gate.
6. Validate every graph edge with its source and destination entry records,
   then build representative controller-driven traversal cases for each family.
7. Test reverse traversal, transitions during falling/recoil, paused entry,
   death near a trigger, input held through loading and repeated retries.
8. Permit broad scene exploration in a labeled development harness, but keep
   story-completion tests free of debug warps and prerequisite bypasses.

**Acceptance:** mapped scene connections have correct destination, placement,
control and camera behavior. Test travel does not teleport to a development
fallback, trap the Knight or lose persistent state. Remaining scripted edge
families are explicit and owned by the relevant package.

### P10 — Implement a shared event, variable and scripting runtime

**Depends on:** P02/P04/P05. **Primary files:** new host IR/compiler and bounded
shared/guest scripting modules; existing PlayMaker parsers are inputs.

1. Inventory actual PlayMaker actions, enabled states, variable types, object
   references and event producers across all scenes/prefabs. Rank reusable
   actions by required instances and progression impact, not just raw count.
2. Choose a compact typed intermediate representation for the supported source
   behavior: state transitions, conditions, timers, variables, references and
   calls into native systems. Keep original source/state/action identity in
   local provenance and debug telemetry.
3. Use native controllers for physics-heavy/common actor behavior where that is
   clearer and faster. Use the shared script executor for declarative room,
   interaction and quest flow. A universal Unity/PlayMaker VM is not required,
   but every required source behavior must have an explicit implemented mapping.
4. Implement common operations in slices: comparisons/branches, booleans/ints/
   fixed floats, waits, state/event sends, activation, animation/tween commands,
   audio/effect events, inventory/PlayerData changes and scene transitions.
5. Preserve local/global scope, owner references, start/enable/disable events,
   synchronous nested event semantics and delayed events. Runner's same-callback
   restart is an existing example that cannot be turned into an extra frame.
6. Model Update, FixedUpdate, animation events, scaled time, hit-stop, pause and
   end-of-frame tokens deliberately. Document each conversion to the60Hz guest.
7. Bound nested dispatch depth, pending events, active scripts and operation
   budget. Reject unsupported required actions during production cooking;
   never compile them as no-ops and call the scene implemented.
8. Add trace comparison of event/state order for representative FSMs. Include
   self-transitions, disable/re-enable, missing targets and repeated enter events.

**Acceptance:** a growing verified action set runs the same source-authored
behavior in all matching instances; unsupported actions retain exact identity
and block the relevant production feature. Callback/timer ordering tests pass
without unbounded execution or silent event drops.

### P11 — Generalize physics, moving objects and environmental hazards

**Depends on:** P03/P09/P10 as needed; coordinate with P14 hero abilities.

1. Catalog collision layers, terrain shape families, one-way behavior and
   gameplay triggers from the installed source. Separate static terrain,
   platform motion, attack hitboxes and contact-damage volumes.
2. Extend fixed-point collision for the actual required slopes, corners,
   platforms, lifts and dynamic terrain. Preserve directional one-way rules,
   contact normals, offsets, grounded detection and body-size changes.
3. Implement moving platforms and lifts with rider transport, blockage and
   control/switch rules; avoid applying motion twice or letting riders tunnel.
4. Implement spikes, pits, water/acid/lava-like source hazards where present,
   crushers, saws, collapsing floors, bouncy/pogo targets and environmental
   damage with the original immunity/checkpoint rules.
5. Distinguish regular damage, hazard respawn and death. Verify camera/scene
   behavior during each, and prevent repeated contact from consuming all health
   before the authored invulnerability period completes.
6. Implement environmental forces and region-specific movement effects from
   source evidence rather than tuning a generic platformer to look plausible.
7. Test high speed, thin geometry, frame boundaries, simultaneous platform and
   hero motion, source scale variants and extreme world coordinates.

**Acceptance:** each required shape/motion/hazard family has numeric fixtures
and a representative original/guest route; no tunnels, stuck corners or wrong
checkpoint recovery appear under bounded worst-case velocities.

### P12 — Complete breakables, secrets and reusable interactions

**Depends on:** P04/P08/P10/P11.

1. Convert current Tutorial-specific bindings into shared breakable definitions
   plus per-instance collision/render/reward/sound/effect bindings.
2. Cover grass, pots, signs, doors, multi-hit gates, Geo rocks, hidden walls,
   cracked floors, ability-only barriers and persistent switches as distinct
   verified source contracts where their conditions differ.
3. Implement hit direction, damage thresholds, staged visuals, debris, particles,
   audio, collider removal, mask fades, rewards and source reset policies.
4. Ensure every cut or slain object has its expected destruction/death output;
   no "just disappears" fallback for an unsupported clip or unloaded effect.
5. Add a generic inspect/talk/use interaction system with source triggers,
   priority, facing, grounded/control restrictions and input-edge behavior.
6. Generalize tablets/signs and multi-page text; distinguish interact prompts
   from dream-nail speech, shops, dialogue choices and transport prompts.
7. Use stable ownership so overlapping spatial regions cannot duplicate hits,
   rewards, prompt displays or particle emitters.
8. Apply each verified definition to all matching scene instances and list
   unresolved scripts/variants rather than adding room-name switches.

**Acceptance:** global source occurrence coverage is reported; breakables emit
complete audiovisual/reward output, doors remove the right geometry, secrets
persist as authored and all inspectable points respond to the correct inputs.

### P13 — Implement the real new-game, bench, death and save loop

**Depends on:** P04/P09/P10/P12; coordinate card I/O with SDK capabilities.

1. Replace the development bootstrap with the normal title→new game→opening→
   first landing flow. Keep a separate explicit debug launch path for fixtures.
2. Implement original initial state and unlock defaults; no hidden cheat grants.
   Ensure menus/options/controls reflect actual supported bindings.
3. Implement benches: sit/stand, health/SOUL and other source effects, respawn
   location, map update and charm/other allowed configuration changes.
4. Implement death, Geo/SOUL consequences, Shade spawn/persistence/recovery,
   replacement/second-death rules and applicable NPC recovery interactions.
5. Implement a versioned save codec for global flags, inventory, quests, maps,
   benches, completion, options and modes. Keep transient scene state separate.
6. Inspect the SDK's existing memory-card APIs. Use them where available;
   prototype missing functionality only for a demonstrated blocker.
7. Measure complete save size against actual card allocation. Add integrity,
   versioning, atomic/recoverable writes, file-selection UI and explicit errors
   for no card, full card, corrupt data and interrupted operations.
8. Test power interruption at meaningful write stages using the emulator/card
   model, preserving the last valid save. Never alter the user's existing
   original saves or overwrite unrelated card slots in test setup.
9. Verify quit/relaunch returns to a legal state with the same progression,
   not just an in-process state reset. Keep old save fixtures for migrations.

**Acceptance:** a clean new game can acquire progress, rest, quit, reload, die,
recover its Shade and continue without duplicates/loss. Card failures are
recoverable and source-specific reset semantics are preserved.

### P14 — Complete the Knight's movement and traversal abilities

**Depends on:** P11/P13; share state/animation/event APIs with P10.

1. Re-audit existing run/jump/air control/release buffering, landing and recoil
   against original traces. Fix measured mismatches before adding complex moves.
2. Implement each source movement ability and upgrade with acquisition,
   conditions, cooldowns, velocity/acceleration, collision and animation events:
   dash, wall interaction/jump, super dash, double jump and upgraded dash behavior.
3. Implement applicable hazard/traversal immunity and special traversal states,
   including source-specific acid access, darkness/light constraints and special
   surfaces. Use the extracted ability catalog as exhaustive authority.
4. Integrate attacks, pogo, spell casting, damage, hit-stop, pause and scripted
   movement cancellation. Verify input buffering and priority when buttons overlap.
5. Implement Dream Nail/related travel abilities with their separate target,
   resource and state-change rules; do not treat them as another nail swing.
6. Expose controls on PS1 with a coherent mapping and updated control screens.
   Extend both the original virtual controller and guest replay input schema
   where current tools only map movement/jump/nail/Focus.
7. Test ability boundaries and combinations: wall→jump→dash, pogo→dash, dash into
   gate/hazard, ability recovery near a moving platform, and repeated input.
8. Apply authored unlock prerequisites. Cheats may exercise mechanics, but only
   normal acquisition routes count toward progression validation.

**Acceptance:** every traversal ability and upgrade is source-derived, bound to
controls/art/audio, persists correctly and opens only its authored paths. Numeric
trajectory/timing comparisons accompany representative controller routes.

### P15 — Complete spells, nail upgrades, nail arts and combat rules

**Depends on:** P10/P13/P14/P19 interfaces.

1. Extract all spell/nail-art definitions, acquisition and upgrade paths, SOUL
   costs, charge/release timing, hit phases and cancellation rules.
2. Implement projectiles, area attacks, downward attacks and invulnerability
   phases with bounded lifetimes, collision, owner/damage attribution and effects.
3. Implement nail damage tiers, chargeable nail arts, nail length/speed modifiers,
   directional attacks, parries, pogo and enemy/hero recoil from source contracts.
4. Centralize damage resolution: hit source/type, damage amount, invulnerability,
   contact rules, hit-stop, armor/shields, multi-hit cooldown and death ordering.
5. Preserve SOUL gain/drain/refund behavior and spell-vs-Focus input priority.
   Ensure pause, transition, death and aborted casts release their resources.
6. Integrate all required animation frames, trails, particles, impacts and sounds.
7. Verify each attack against stationary, moving, armored and boss targets, plus
   simultaneous hits and interaction with destructible/environmental targets.

**Acceptance:** every source combat acquisition can be earned and used; damage,
resource/timing rules and presentation are verified. No attack works only because
of a debug grant or a special-case test enemy.

### P16 — Implement inventory, equipment, charms and modifiers

**Depends on:** P04/P13/P14/P15.

1. Generate the complete item/charm/notch/fragment/resource catalog from source,
   including identifiers, descriptions, acquisition routes, costs and effects.
2. Implement inventory and charm UI with navigation, text, icons, equipped-state
   feedback and source restrictions on where changes are allowed.
3. Implement mask/SOUL fragments and assembled upgrades, nail tiers, permanent
   abilities, collectibles and category-specific counters without hard-coded
   tutorial maxima. Preserve Lifeblood as a separate temporary health pool.
4. Implement notch costs, equip/unequip, overcharm rules, fragile/broken/upgraded
   variants and quest transformations with their persistence and economy links.
5. Build typed modifier composition with source evaluation/rounding order.
   Avoid unrelated modules each applying the same charm bonus independently.
6. Cover charm effects on movement, combat, SOUL/Focus, health, minions, Geo and
   special interactions. Maintain an explicit unimplemented-effect inventory.
7. Test each charm alone, source-special combinations and pairwise interactions
   by effect domain, then stress relevant multi-charm combinations. Do not claim
   exhaustive combination testing from a handful of fixtures.
8. Keep cheat menu grants separate from authoritative inventory progression.
   Update cheats to actual implemented systems; do not advertise fake "all charms".

**Acceptance:** all installed charms/items are acquired/equipped/transformed as
source permits; effects, stacking and UI are functional; save/load round trips
preserve the correct state and unlocks.

### P17 — Complete economy, shops, map and journal systems

**Depends on:** P12/P13/P16.

1. Implement shared purchase transactions: eligibility, price modifiers, stock,
   currency deduction, delivery, persistence and once-only side effects.
2. Bind each shop/NPC catalog to original unlock/revisit conditions; cover nail
   upgrades, repair services, charm/notch purchases and quest-gated stock.
3. Implement collection/reward delivery for Geo, eggs, relic-like inventory,
   keys and other source currencies/resources. Prevent double collection after
   a transition, interrupted purchase or repeated event.
4. Implement map ownership/reveal, purchased map data, explored-room tracking,
   bench updates, markers/pins, compass behavior and map UI using source rules.
5. Implement journal entries, kill counts, reveal thresholds, descriptions and
   completion rewards. Variant enemies must map to the correct journal identity.
6. Derive text/icon resources and UI layout from the original; make them legible
   at320×240 without leaking host implementation details into product screens.
7. Test insufficient funds, full/owned inventory, cancelled choice, repeat
   purchase, reload after purchase and simultaneous reward events.

**Acceptance:** all source purchase/reward paths work, the world can be navigated
with the map system and journal progress persists with correct counters.

### P18 — Implement NPCs, quests, dialogue and world transport

**Depends on:** P10/P12/P13/P16/P17.

1. Extract NPC state/dialogue selection, interaction conditions, choices, motion,
   facing, animation, chatter/voice and persistence. Keep text in source-derived
   localization tables rather than embedding room-specific strings in guest code.
2. Implement shared dialogue and quest primitives: conditional lines, choices,
   rewards, flags, waits, animation cues, actor activation and scene changes.
3. Create a quest dependency graph with alternate branches and mutually exclusive
   outcomes. Bind scene variants/NPC locations to those same canonical flags.
4. Implement stations/fast travel, trams, lifts, toll gates and return routes with
   unlock/payment/cinematic rules and correct destination state.
5. Cover NPC death/disappearance, rescue/failure conditions, dream interactions,
   shopkeeper changes and multi-step reward chains present in the installed game.
6. Validate dialogue choice cancellation, interruption by damage/scene load,
   revisits before/after each quest phase and save/load between stages.
7. Use source-generated coverage to ensure every NPC/quest has an owner and test;
   named well-known characters are examples, not an exhaustive manual checklist.

**Acceptance:** NPC/quest/transport catalogs have no silently unbound required
instances; branches produce the correct flags, rewards, appearance and dialogue.
Every transport destination supports arrival, departure and persistence.

### P19 — Finish the common actor runtime and existing Runner/Climber work

**Depends on:** P04/P07/P10/P11/P15 interfaces.

1. Preserve source identity, spawn/despawn conditions, health, hitboxes, contact
   damage, animation, physics, AI state, sound/effects and reward ownership in
   a shared actor interface with bounded pools.
2. Finish Runner presentation first: install the prepared sample bank, loop/call
   ownership and voice arbitration; bind Charge Dust and other required events;
   stop/release on scene exit, reset, disable and death.
3. Replace `unbound_runner_event` only when all required events have live owners.
   Then enable verified Runner variants in canonical cooking and test both
   source instances plus equivalent instances across the catalog.
4. Bind Climber terrain sensing, transforms, corner motion, art/audio, combat and
   corpse behavior. Obtain original traces for inside turns, stun and death
   before treating those branches as validated.
5. Preserve source conditional spawns such as Mender; serialized active state is
   not enough to decide whether an enemy exists in the current game state.
6. Resolve original Update/physics/scaled-time order and per-actor RNG contracts.
   Record controlled seeds and observed differences rather than claiming Unity
   random parity from a deterministic guest alone.
7. Test actors crossing internal spatial partitions, offscreen patrol, missing
   terrain during preparation, defeat→return behavior and reward exactly once.
8. Measure Actor/state/code-size changes against the current tight link budget.
   Investigate duplicated generic instantiations only with actual symbol evidence.

**Acceptance:** Runner/Climber are truly in playable builds with complete event
cleanup, not just pure tests or cooked art. The shared actor interface scales to
other families without scene-name conditionals or persistent data references.

### P20 — Implement the complete normal-enemy roster by family

**Depends on:** P19; P10 supplies scripted variants.

1. Generate the complete enemy/controller/variant list and group by verified
   behavior rather than appearance alone. Preserve exceptions in a source table.
2. Implement in coverage order: ground walkers/chargers, wall/ceiling crawlers,
   jumpers, flyers, pursuit/swoop actors, projectile shooters, burrow/ambush
   actors, shields/armor, turrets, split/spawn/minion actors and special hazards.
   Adjust order using actual progression dependencies and catalog counts.
3. For each family, extract sensing/ranges, acceleration/speeds, cooldowns,
   attack hit phases, terrain rules, stun/recoil, turning, despawn and death.
4. Bind idle/movement/attack/damage/death/corpse clips, sound loops/calls/impacts,
   VFX, Geo/journal rewards and persistence as part of the same feature.
5. Apply verified family contracts across all rooms; add explicit parameter or
   state variants where source differs instead of loosening a strict recognizer.
6. Test multi-enemy interaction, simultaneous deaths, projectiles crossing view
   boundaries, offscreen enemies and bounded resource pressure.
7. Require one original/guest behavior trace per distinct state-machine variant,
   supplemented by generated static contract checks for every admitted instance.

**Acceptance:** no required normal enemy type remains a placeholder or invisible
object; the coverage ledger links every source variant to runtime code, art,
effects, sound, rewards and validation evidence.

### P21 — Implement every boss and its arena lifecycle

**Depends on:** P14–P20 plus relevant progression flags.

1. Generate an exhaustive boss/variant roster from source, including dream,
   upgraded and challenge versions. Do not treat visually similar versions as
   identical or rely on a remembered list of famous bosses.
2. Build reusable arena lifecycle: entry/intro, lock doors, health phases,
   camera/music changes, minion/projectile ownership, stagger, death, reward,
   unlock/exit, player death, retry and dream/challenge return.
3. Implement source attack selection, conditions and RNG, telegraphs, movement,
   hitboxes, damage, timing, vulnerability and phase transitions for each boss.
4. Prefer shared attack primitives for dashes, leaps, aimed/spread projectiles,
   beams, hazards and summons, with source parameters and per-boss sequencing.
5. Include all phase art/animation, arena effects, death/defeat sequences, sound
   and music events. Boss rooms must not be considered complete with silent
   attacks, wrong hit windows or missing phase transitions.
6. Exercise win/loss/retry, phase boundaries, simultaneous lethal events,
   different weapon/ability tiers and relevant charm interactions.
7. Validate representative complete fights through controller input, not only
   forced health changes. Test all distinct attack branches with controlled
   setup separately; those tests supplement the normal fight routes.

**Acceptance:** every boss/variant can be entered, fought, defeated or retried
with correct state/rewards and without softlock. No boss is "implemented" solely
because it spawns or one attack looks right.

### P22 — Complete arena waves, challenge rules and encounter composition

**Depends on:** P18/P20/P21.

1. Implement shared wave sequencing, spawn conditions, delays, arena hazards,
   floor/wall changes, intermissions, rewards and retry/reset rules.
2. Bind all installed arena/challenge content, including Colosseum-style trials,
   boss sequences and applicable rematch/hall/pantheon systems from source.
3. Implement challenge modifiers, bindings, health/resource restrictions and
   victory/unlock bookkeeping; normal-world progress must not be corrupted by
   temporary challenge state or restored incorrectly afterward.
4. Precompute peak concurrent actor/projectile/effect/audio needs per wave.
   Implement streaming/activation schedules that meet those peaks; silent actor
   admission failure changes encounter difficulty and is unacceptable.
5. Test every wave transition, simultaneous last kills, player death at the same
   time as victory, rest/intermission behavior and retry without a process reset.

**Acceptance:** all challenge sequences run from entry through success/failure
and return, with exact required spawns, modifiers, rewards and persistence.

### P23 — Close story progression and every ending

**Depends on:** P09–P22; work incrementally as these systems become available.

1. Build the executable progression manifest from P02: acquisition flags,
   keys, dream/essence-like counters, major objectives, transformations,
   branching NPC outcomes and terminal ending predicates.
2. Cover every required major region and branch identified by the source catalog.
   Familiar areas such as Crossroads, Greenpath, fungal regions, City, waterways,
   crystal regions, Deepnest, gardens, basin/abyss, edge/hive, dream areas and
   palace/challenge spaces are navigation aids; the manifest is exhaustive.
3. Implement scene-state replacements and altered-world variants with correct
   trigger flags, connections, encounters and visual/audio changes.
4. Implement major-objective interactions and sequences through the same shared
   scripting/state APIs; no debug flag writes in normal routes.
5. Validate a clean new-game route through a first normal ending. Record it as
   a milestone with its exact missing optional/alternate content still listed.
6. Validate every other ending and mutually exclusive branch using independent
   saves rooted in genuine prior progression; clearly distinguish replay setup
   saves from clean new-game final-route evidence.
7. Audit ability gating and sequence variations. Supported original sequence
   breaks should not be accidentally forbidden; unintended port softlocks or
   progression shortcuts must not be accepted as alternate paths.

**Acceptance:** all ending predicates and their required gameplay paths are
functional, correct cinematics/credits/return states occur, and no debug grant,
warp, skipped required encounter or manually patched save is used as final proof.

### P24 — Close optional content, collectibles and installed expansions

**Depends on:** P16–P23.

1. Generate a complete checklist from source for optional rooms, secret paths,
   collectibles, rescues, NPC rewards, dream encounters, journal entries and
   challenges. Compare checklist totals with source counters and conditions.
2. Implement every remaining optional quest and its possible outcomes, including
   content associated with the installed expansions and alternate boss modes.
3. Verify the game's maximum completion calculation using collected source flags.
   Then audit non-contributing content separately so a maximum counter cannot
   conceal missing endings, dialogue, optional fights or challenges.
4. Test returning after later upgrades/state changes, mutually exclusive rewards,
   hidden entrances, special keys, NPC relocations and one-time events.
5. Ensure all relevant content is represented in save state, map/journal/UI,
   achievements/unlocks where applicable and world-state changes.

**Acceptance:** the source-derived optional-content matrix has no unimplemented
required entries; maximum completion and the separately listed non-contributing
content have real gameplay evidence.

### P25 — Finish animation, particles, cinematics and visual polish globally

**Depends on:** P08/P10 and the content packages using each effect.

1. Compare the animation/event inventory with all runtime bindings. Resolve
   missing clips, blend/transition behavior, frame events and disabled/triggered
   animations, including backgrounds and environmental loops.
2. Extend particle/effect support by source parameter family: emitter shapes,
   scale spaces, lifetimes, curves, motion, spin, color, collision, child emitters,
   trails and renderer material behavior. Keep source-specific exceptions visible.
3. Choose scalar simulation or offline tracks based on measured CPU/RAM cost and
   faithful behavior. The existing Tutorial track bank already costs399,432 B;
   do not scale that permanent allocation linearly to the whole world.
4. Stream scene/effect banks and share immutable trajectories/curves where valid.
   Keep actor feedback, pickups and required hazard effects available on time.
5. Implement title animation, opening and ending presentation, dream transitions,
   cutscenes and credits. Inspect source media types before choosing SDK playback
   or a source-frame-based representation; measure CD/CPU/audio synchronization.
6. Compare original/guest captures at meaningful states, not only empty rooms.
   Fix geometry holes, incorrect transparency, layer omissions, palette changes,
   transition flashes and texture popping with source-bound explanations.
7. Record explicit PS1 rendering approximations. Do not delete difficult layers,
   animations or effects and count the content as completed.

**Acceptance:** every required clip/effect/cinematic is bound and observable;
resource-pressure routes do not silently lose required feedback or scene layers;
remaining differences are documented and within the agreed port presentation.

### P26 — Complete SFX, ambience, music and CD scheduling

**Depends on:** P06/P07/P10; integrate alongside individual system packages.

1. Extract an exhaustive cue catalog from AudioSources, FSMs, animation events,
   scripts and music/ambience controllers. Include source clip, duration, loop
   points, pitch/volume ranges, spatial behavior, trigger and stop conditions.
2. Define a small consistent set of sample-rate categories. Preserve existing
   measured categories as starting points; lower long-sample quality by category
   when necessary, not clip-by-clip emergency truncation.
3. Implement shared voice ownership, priorities, looping, pitch, mixing and
   stop/release behavior across player, enemies, environment, UI and scripted
   events. Critical warnings/hit cues need guaranteed admission.
4. Use per-scene/area SPU banks plus bounded RAM streaming where measured. Current
   Runner binding nearly fills SPU RAM; it is not a template for making every
   sound permanently resident.
5. Extend title CD music to relevant scene/boss cues. Inspect pinned SDK support
   before choosing CDDA, an available compressed/interleaved stream path, or
   buffered SPU ADPCM. Do not assume an unverified SDK streaming API exists.
6. Build a complete disc-duration/capacity budget before storing all music as
   CDDA. Count audio sectors, data, alignment, duplicate banks and streaming
   placement. The whole score cannot be accepted just because the title track fits.
7. Design one CD owner/scheduler for scene data and music. A drive cannot service
   arbitrary data seeks while simultaneously promising uninterrupted unrelated
   audio playback. Prefetch, interleave or buffer according to measured demand.
8. Implement cue transitions, intros/loops, boss phases, fades, pause/resume,
   dream/cinematic changes and return-to-area restoration without restarting
   every track unnecessarily or leaving an old cue active.
9. Validate missing cues using original API traces, then listen to guest audio
   captures for rate/pitch, clipping, loops, clicks, timing and level consistency.
   NullGfx source logs establish trigger calls, not audible-output equivalence.
10. Stress scene traversal with music, many SFX and ambience simultaneously.
    Measure underruns, stolen voices, late cues, CD errors and buffer headroom.

**Acceptance:** the cue coverage matrix is complete; source timing/stop conditions
work, audio quality is consistent by category, music fits the disc and actual
streaming schedule, and test routes have no underruns or silent critical cues.

### P27 — Finish menus, localization, modes, settings and accessibility

**Depends on:** P13/P16–P26.

1. Complete title/pause screens, save slots, inventory/map/journal, settings,
   control help, confirmations, loading/error recovery and unlock presentation.
2. Implement installed game-mode selection/unlock/death/save rules, including
   permadeath/challenge-specific modes where present. Keep separate save schemas
   or mode tags so normal progress cannot leak into temporary challenge state.
3. Complete localization data loading, glyph coverage, line wrapping, pagination
   and choices at320×240. Inventory every shipped language and unsupported glyph
   case; do not silently drop localized strings or substitute missing text.
4. Make controller mappings complete and consistent across gameplay, menus,
   dialogue and transport. Test disconnected controller/reconnection and held
   buttons across transitions with the SDK's actual capabilities.
5. Verify volume controls apply to all channels/categories, including CD music,
   streamed ambience, new enemy cues and cutscenes; persist settings as designed.
6. Preserve original gameplay-assistance/accessibility options that exist in the
   installed version, and make any PS1-specific constraints explicit.
7. Keep developer cheats honest, optional and off by default. Mark cheat/test
   use in validation telemetry so completion routes can reject accidental grants.

**Acceptance:** every gameplay system is usable through its real interface and
controller bindings; language/mode/settings paths are accounted for and saves
remain correct across mode changes and menu transitions.

### P28 — Meet performance and seamlessness requirements across content

**Depends on:** scalable architecture and representative content from prior tasks;
measure throughout development, close the final target here.

1. Build a benchmark matrix covering ordinary traversal, worst overdraw,
   multiple enemies/projectiles, particle-heavy breakables, boss phases, large
   effect transitions, UI/text, music streaming and the largest resident scene.
2. Record CPU stages, GPU work using current measured frontend capabilities,
   VBlank cadence, input sampling, CD commands/latency, decode/upload work,
   animation misses and audio underruns. Wall-clock emulation speed is irrelevant.
3. Target responsive60Hz simulation and two-VBlank presentation at320×240.
   Report late frames and worst-case cadence, not just average fps. Distinguish
   authored pause/hit-stop/cinematic pacing from unintended performance stalls.
4. Apply measured shared fixes: spatial indices for terrain/actors, compact
   region lookup, reusable animation decode, cache-friendly data, batched GPU
   submission, bounded incremental loaders and verified occlusion precompute.
5. Precompute only where total benefit includes permanent/active RAM, disc bytes,
   decode time, invalidation and cache behavior. "More tables" is not a free fix.
6. Use the existing optimization history in `docs/PERFORMANCE.md` to avoid
   repeating rejected trials. A smaller/faster-looking host or Rust patch is
   not evidence of better guest cadence.
7. Validate changes with equivalent routes/artifacts and image/state comparisons.
   Do not lower resolution, remove content, reduce simulation correctness or
   disable effects to manufacture a passing performance report.
8. Measure console CPU/GPU/CD/SPU behavior on hardware before making physical
   performance claims; coordinate hardware capture with the user separately.
9. Close every unintended traversal stall with predictive loading or a measured
   architectural improvement. Source-authored transitions still need correct
   pacing; their presence does not excuse long extra port-only freezes.

**Acceptance:** representative and worst-case cases meet the final pacing/loading
criteria, no regression is hidden in an average, and the evidence clearly
separates emulator results from measured console behavior.

### P29 — Run full progression, branch and persistence validation

**Depends on:** P13–P28; partial routes are built earlier.

1. Create a declarative route suite with starting state provenance, controller
   schedule, expected gates/items/flags, assertions, timeouts and capture points.
2. Maintain a clean-new-game route through each ending and optional-content
   branch, using independent saves where outcomes are exclusive. Do not count
   a patched endgame save as a new-game-to-ending run.
3. Test all scene transition families and condition variants, every boss phase,
   every required pickup and quest stage, every transport path and all reset
   policies. Use generated coverage to find unvisited/unasserted cases.
4. Add save/quit/relaunch checkpoints throughout progression, not only at the
   opening. Include death/Shade, hazards, interrupted purchases, defeated bosses,
   dream returns and challenge retries.
5. Test adversarial but legal input sequences: input held during loads, pause at
   transitions, simultaneous kills/hits, repeated interactions and rapid revisits.
6. Compare source/guest state at semantic checkpoints, allowing documented
   numeric tolerances and random branching. Do not align Unity and PS1 simply
   by frame number when their physics clocks differ.
7. Inspect images/audio at designated points, alongside state assertions. A
   healthy state log does not prove the level rendered or the music played.
8. Produce an evidence matrix joining each completion requirement to exact
   source/EXE/CUE/tool hashes, route result and observed outputs. Resolve gaps.

**Acceptance:** all ending/optional/mode routes and state lifecycles are covered;
no debug grants, unexplained skips, silent missing content or unresolved
progression softlocks remain. Weak/missing evidence stays incomplete.

### P30 — Hardware, corruption and long-run reliability validation

**Depends on:** P28/P29; hardware scheduling may require the user.

1. Build the final non-telemetry executable with the tested toolchain/SDK;
   require zero final R3000 hazards and checked memory/stack bounds.
2. Exercise malformed headers, bad checksums, truncated streams, invalid IDs,
   missing audio/art, CD read retries and interrupted memory-card writes.
   Failure must preserve a recoverable UI/state rather than arbitrary memory.
3. Run long scene-cycling and combat sessions to detect stale references,
   fragmenting caches, leaking voices, accumulating events and ID reuse bugs.
4. Measure stack high-water separately for main and IRQ stacks under their
   worst nested paths. Linked reservation alone is not this measurement.
5. Run the relevant SDK/hardware-test battery and real console routes with
   known media/loader setup. Read current skill/project instructions first.
6. Verify video/display, DMA completion, CD data/music transitions, SPU loops/
   envelopes, controller timing and memory-card reliability on hardware.
7. Record exact executable/disc/media/platform/route identities. Emulator
   agreement is useful evidence but cannot replace a failed console result.

**Acceptance:** no memory corruption, stale resource ownership or long-run leaks;
error recovery is tested; required physical-console behavior has direct evidence.
If hardware is unavailable, the hardware portion remains pending, explicitly.

### P31 — Reproducible build, final audit and delivery

**Depends on:** all other packages.

1. Audit every requirement in section2 against the current evidence matrix.
   Search for unsupported/not-run/failed items and inspect what they cover.
   A green aggregate report is not enough if the underlying cases are narrow.
2. Build from a clean supported environment with the user's own Windows assets.
   Verify source discovery, pinned dependencies, deterministic cooking, license
   notices, file provenance and documented failure messages after source updates.
3. Confirm the source repository contains no retail files, captures, imported
   object data, decoded audio, discs or secrets. Keep local artifacts ignored.
4. Check the complete disc budget/layout, music/data addressing and all required
   files. Do not create multiple playable copies as a delivery shortcut.
5. Once replacement is authorized, build the sole library pair, validate that
   exact final CUE, inspect its captures and verify `.hkpsx/build.json` hashes.
6. Update README, controls, supported-content matrix, known limitations, budget,
   performance and validation docs from actual final evidence. Remove obsolete
   top-level instructions that contradict the final architecture.
7. Commit/push source-only changes and provide a concise release handoff naming
   the exact playable path, build identity, validation scope and any unresolved
   limitations. If required limitations remain, do not call the whole game done.

**Acceptance:** all section2 requirements have matching authoritative evidence,
reproduction works, the one delivered disc matches the final tested source and
no missing content or unverified platform claim is concealed.

## 8. First execution sequence: what Sol should actually do next

These are small implementation slices, not another invitation to redesign the
entire project. Complete them in order, recording evidence after each.

| Slice | Concrete change/work | Proof required |
| --- | --- | --- |
| S01 | Re-read state and create the execution ledger | Source/disc identities and pending P01 recorded; no build launched |
| S02 | Diagnose the lost import worker with a single-scene/one-worker reproducer | Actual process exit/signal/resource evidence, not an OOM guess |
| S03 | Fix bounded worker supervision and interrupted-run reporting | Tests for abrupt exit, cancelled jobs, retry limits and checkpoint survival |
| S04 | Validate resume fingerprints and output hashes | Corrupt/stale outputs rejected; intact compatible outputs reused |
| S05 | Process all501 source scenes and complete final verification | Full catalog accounting and final unchanged-input/code evidence |
| S06 | Produce prioritized geometry/dependency gap lists | Counts by shared cause and source identities, no per-room ad hoc queue |
| S07 | Resolve the largest reusable extraction gaps | All matching instances reprocessed; malformed exceptions retained |
| S08 | Generate complete effective transition/progression manifests | Literal and scripted destinations distinguished; unresolved cases enumerated |
| S09 | Measure metadata formats and current link allocations | Per-section sizes and before/after ownership map |
| S10 | Add the checked scene metadata encoder/reader | Round-trip, truncation, overflow, wrong-scene and corrupt-span tests |
| S11 | Replace region-pointer cache identity | Same-address scene replacement test proves invalidation |
| S12 | Migrate current Tutorial/Town metadata and auxiliary owners | Existing actual-CUE gate/return/reset routes and matching state/captures |
| S13 | Remove global scene/region/chunk ceilings structurally | Large catalog fixture and real world manifest load within measured memory |
| S14 | Cook shared world art and scene budget matrix | Every scene accounted for; oversize packs explicitly reported |
| S15 | Add predictive residency and a development scene explorer | Required resources admitted before draw; unsupported content clearly reported |
| S16 | Bind generic transitions/camera entries, beginning with the well family | Original↔guest source-bound entry, return and control timing checks |
| S17 | Implement the first common scripting/state primitives | One real family applied across all verified instances with event-order proof |
| S18 | Finish Runner and Climber integration as shared actor examples | Real presentation ownership, source traces and guest combat/death/revisit proof |
| S19 | Implement persistent bench/save/death foundations | Quit/reload/death/Shade round trips through actual gameplay |
| S20 | Continue P14 onward by system, using the full coverage ledger | Each package expands working behavior across the imported world |

Some slices can share source-only work while independent tasks run. Do not
freeze the whole import because one renderer family is incomplete. Likewise,
do not ship the development explorer as if it were the playable whole game.

The metadata relocation needs a safe intermediate guest build. While the user
is preserving disc71, use source/native tests and the existing source-only link
approach after inspecting its mechanics. Do not invent a second playable disc
for A/B testing. When a new playable build is requested, replace the sole pair
and validate it before handing it over.

## 9. Cross-system coverage checklist

The generated source catalog is exhaustive; this checklist prevents common
categories from being forgotten while implementing it.

### World and progression

- Main path, optional branches, hidden rooms, dream realms, rematch/arena variants.
- Region entry/exit, wells, lifts, fast travel, trams, one-way drops and scripted warps.
- World-state replacements, temporary/dream state, locked/unlocked gates and return flow.
- Ability/key/quest conditions, scene activation, spawn suppression and branch exclusivity.
- Normal opening, all ending conditions, ending presentation, credits and post-ending state.
- Installed expansion content, including Hidden Dreams, Grimm Troupe, Lifeblood
  and Godmaster content where represented by the installed source, not just base rooms.
- Optional challenge paths such as the palace/Path of Pain and boss-sequence content.
- Installed modes such as Steel Soul/Godseeker where present, unlocks and mode-specific saves.

### Player, objects and economy

- Movement, all ability upgrades, combat/spells/nail arts, recoil, damage and immunity.
- Masks/SOUL/Lifeblood, fragment assembly, currency/resource caps and once-only rewards.
- Full charm/notch roster, overcharm, fragile/broken/upgraded states and effect combinations.
- Inventory, map/compass/pins, journal, item descriptions and shop/service transactions.
- Benches, ordinary death, hazard respawn, Shade, quit/load and mode-specific death behavior.
- Breakables, grass, doors, secrets, rocks, switches, moving/temporary platforms and hazards.
- NPC dialogue/choices, rescues, quests, relocations, rewards and altered-world behavior.

### Combat and presentation

- Every enemy family and source variant, contact hitboxes, terrain senses and conditional spawns.
- Projectiles, minions, armor, stun, death/corpses, Geo/journal updates and audio cleanup.
- Every boss phase, arena lock, death/retry, dream/rematch variant and challenge modifier.
- Every required animation/event, background layer, mask/fill, particle/debris/trail and cinematic.
- Player/enemy/environment/UI SFX, ambience layers, music cues, fades/loops and volume settings.
- Title/pause/controls/settings, readable dialogue and choice UI, save selection and recovery.
- Source language/glyph coverage, controller behavior and developer-cheat isolation.

### Runtime and delivery

- Memory, stack, VRAM/CLUTs, SPU, CD capacity, file/chunk/index bounds and decompression peaks.
- Scene-generation ownership, cache invalidation, persistent IDs and asynchronous cancellation.
- 30fps render target/60Hz input-simulation behavior, dynamic content peaks and seamless traversal.
- Cold boot, long play, reload, corrupt data, unavailable disc/card and recovery behavior.
- Clean source-only repository, reproducible bring-your-own-assets build and one final disc pair.

## 10. Evidence and test design

### 10.1 Original-game oracle

Read `docs/ORIGINAL_REFERENCE.md` before using the reference harness.

Existing commands (run from the repository root):

```sh
python3 tools/doctor.py
python3 tools/reference_game.py prepare
python3 tools/reference_game.py run --name UNIQUE_RUN_NAME --frames 300 --tape tools/reference/tapes/movement.csv --timeout 90
python3 tools/reference_game.py compare --left FIRST_RUN_NAME --right SECOND_RUN_NAME
```

Replace uppercase names with new concrete run directory names. Prepare again
after managed-driver changes; the preparer binds the source and driver hashes.
For live control, use the documented `command` mailbox and wait for actual
acknowledgement before sending the next command. File creation does not prove
Unity executed the action. The mailbox holds the latest command, not a queue.

The original uses a60Hz capture clock and roughly50Hz physics. Compare elapsed
scaled time, physics steps, animation completion and event order. Several native
input updates can occur in one captured frame, especially while loading.
Do not compare Unity frame numbers directly with PS1 frames. Record hit-stop,
slowdown, pause, RNG and initialization prerequisites.

Teleport/scene commands are valuable setup for isolated behaviors. They do not
prove authentic traversal timing or clean story progression. NullGfx logs can
prove state and requested audio/effect calls; they cannot prove visual or
listening parity. Use actual rendered/audio output for presentation comparisons.
The current reference bootstrap bypasses the opening movie deliberately; P13
must add a separate normal-start test rather than calling that setup authentic.

### 10.2 Host and native checks

Existing broad entry point:

```sh
make test
```

Use targeted Python/native tests during individual changes, then the relevant
full suites for integration. Do not rewrite tests to mirror implementation
without a source contract. Meaningful test families include:

| Family | Necessary assertions |
| --- | --- |
| Import | Worker crash vs scene error, bounded retry, interruption, resume, code/source/output hash mismatch, all catalog entries accounted for |
| Geometry | Hierarchy/flip/scale, inactive alternatives, exact shape paths, tile cell coverage, missing built-ins, dynamic geometry ownership |
| Binary formats | Round trip, every truncation boundary, offset/count multiplication overflow, alignment, wrong identity/stride/version, checksum failure |
| State | Stable IDs, duplicate occurrences, once-only rewards, correct scene/bench/death resets, pending timers and save migration |
| Scripting | Reentry/order, scaled/unscaled waits, missing target, queue overflow, enable/disable, deferred events and invalid required actions |
| Gameplay | Source-specific numbers, attack windows, immunity, resource costs, interactions and ability/quest prerequisites |
| Streaming | Required-resource deadlines, cancellation, return path, stale generation, decode corruption, GPU pins and music/data contention |
| Audio | Full duration, rate categories, loop markers, pitch, mixing, voice ownership and release, underruns and volume persistence |
| Saves | Codec version/CRC, power interruption, full/missing card, partial write, mode separation and exactly-once purchase/reward persistence |

A parser's "valid" result only proves its encoded constraints. A full gameplay
assertion requires that the cooked data actually expresses the source behavior.

### 10.3 Guest validation and playable-build ownership

`cargo run --release -- build` builds/cooks, **replaces the sole playable pair** and replays the route tapes.
Do not run it during a requested disc-preservation period. A normal authorized
build must pass final executable hazard and stack/link checks.

For an authorized rebuilt disc, existing headless replay syntax is:

```sh
python3 tools/replay_cue.py --tape PATH_TO_REAL_TAPE --output .hkpsx/validation/UNIQUE_ROUTE --screenshot-interval 120
python3 tools/validate_scene_gates.py .hkpsx/validation/UNIQUE_ROUTE --minimum-loads 3
```

Those path names are placeholders, not files claimed to exist. Inspect the
chosen tape and current frontend's actual `--help` before using a new scenario.
The current tape runner verifies exact controller poll termination, watches
faults and binds input/artifact hashes; it is not automatically an assertion
for every newly added gameplay system. Extend the relevant watcher/validator
with meaningful completion assertions and expected source identities.

Useful existing evidence locations:

- `.hkpsx/playtest71/`: actual final71 build and850-poll Focus smoke.
- `.hkpsx/game69/validation.json`: long gate/return/reset evidence for69.
- `.hkpsx/runner71/`: Runner art/audio/controller/link/native evidence.
- `.hkpsx/runner70/` and `.hkpsx/enemy-trace70/`: original Runner captures/analysis.
- `.hkpsx/climber70/`: source contract and partial original controller proof.
- `.hkpsx/crossroads70/RESULT.md`: isolated full Crossroads capacity, not playability.
- `.hkpsx/world-metadata70/PLAN.md`: relocation analysis, with historical byte counts.
- `.hkpsx/world-import/`: interrupted whole-world extraction and its tests.

Verify files still exist and match their referenced hashes before relying on
them. A filename with a larger number does not make it a validated newer disc.
Never use a stale map to interpret current RAM addresses.

### 10.4 Representative content plus exhaustive accounting

Not every static object needs a bespoke hand-written replay. Use exhaustive
source contract/pack validation for all instances, representative original/guest
behavior routes for each distinct contract, and full progression routes joining
the systems. Add a new route whenever a variant changes semantics, resource peak
or progression risk. Report the unobserved cases honestly.

For each system, include at least:

1. Ordinary successful use.
2. Invalid/prerequisite-denied use.
3. Boundary/cancellation/interruption.
4. Death, transition or reset during its active lifetime.
5. Save/reload when it changes persistent state.
6. Simultaneous instances and measured resource pressure.
7. A different scene containing the same supported family.

For renderer/audio systems, also inspect actual images/audio. For progression,
verify prerequisites and postconditions, not merely the final coordinates.

## 11. Decisions that must be measured rather than guessed

| Decision | Evidence to collect | Wrong shortcut to avoid |
| --- | --- | --- |
| Why the importer worker died | Exact scene/PID, exit/signal, resources and repeatable reproducer | Calling196 scenes corrupt or assuming OOM from a generic pool exception |
| Per-scene memory architecture | Full world section/animation/metadata/decode size distributions and linked reclaim | Increasing every static maximum until the linker fails |
| Neighbor residency | Current+neighbor union costs and travel/read deadlines | Assuming unused RAM can hold every adjacent room |
| Full-world disc layout | Complete unique data and audio sector budget plus seeks | Appending all music as CDDA because title playback works |
| Texture sharing | Canonical representative comparisons, alpha/material/palette proof | Chaining95% pair matches until unrelated silhouettes share texels |
| Script execution strategy | Common action/state graphs and source callback semantics | Building an unrestricted Unity VM or treating unknown actions as no-ops |
| Enemy family generalization | Source contract differences and representative traces | Classifying by similar sprites or relaxing strict checks to force admission |
| Particle representation | Source parameters, fidelity, CPU/RAM/disc costs | Permanently linking exact tracks for every emitter in the game |
| Performance improvement | Equivalent guest routes, stage timings and images/state | Reporting host runtime or lower code size as PS1 fps improvement |
| Save format/capacity | Actual persistent-state size, card API/storage behavior and failure tests | Dumping native pointer-rich structs to a card |
| Hardware readiness | Exact final disc/console observations | Treating an emulator pass as silicon proof |

If a measured hardware limit cannot be met under the user's existing quality
constraints, bring back a concrete comparison with actual byte/timing/visual
costs. Do not silently reduce content or reinterpret "whole game" as one ending.
Keep doing independent authorized work while a genuinely necessary decision is
pending. Do not ask the user to debug every room manually.

## 12. Mistakes to avoid from this project's history

- Black screen despite moving VRAM: inspect display/draw environment, buffer
  selection and transition state, not only texture uploads.
- Geometry holes: source opaque tilemap fills and conditional masks matter;
  decorative SpriteRenderers alone do not reconstruct the world.
- Missing destruction: collision removal is not a full breakable implementation;
  source animation/debris/particle/sound/reward ownership is part of the feature.
- Lifeblood is additional temporary health consumed before ordinary masks, not
  a heal effect or an ordinary maximum-health upgrade.
- Source Town well strings are overridden by FSM behavior. Literal adjacency
  is useful inventory, not universally correct runtime routing.
- An authored13-hit Great Door is different from a port-induced loading freeze;
  measure and compare before changing source rules to mask perceived slowness.
- A full original animation library may be shared across many instances. Do not
  store per-instance copies or cook only the currently visible/default frame.
- Reusing an arena address does not preserve object identity. Generation-based
  cache invalidation is required, including retry/reset of the same scene.
- Source code/host output is not a playable update. State which disc was rebuilt
  and tested, or explicitly say it stayed unchanged.
- Do not create competing "fixed", "candidate", "latest2" or diagnostic discs.
- Do not make more source-only preparation updates indefinitely after the core
  import architecture works. Each following milestone must expand playable
  systems and routes, with the actual disc refreshed when authorized.
- Do not turn user playtesting into the primary discovery loop. Automate
  original/guest comparisons and use user reports as additional evidence.

## 13. Completion audit template

Before marking any package done, fill this in the execution ledger:

```text
Package / feature:
Source contract and source-version hash:
Required instances / variants / scenes:
Implementation and runtime binding:
Remaining unsupported instances or conditions:
Host/native tests actually run:
Original reference runs and what they prove:
Actual guest EXE/CUE hashes and routes:
State assertions, images and audio inspected:
RAM / stack / VRAM / SPU / CD / frame results where relevant:
Persistence / retry / reset evidence:
Unverified or approximated behavior:
Outcome: pending | partial | validated
Next concrete action:
```

Before marking **the whole game** complete:

1. Expand section2 into individual requirements using the final generated content
   catalog. Include all named modes/branches, every relevant scene classification,
   every implemented family and each unresolved exception.
2. Link each requirement to current authoritative evidence. Check the content of
   that evidence and its build/source identity rather than counting green badges.
3. Verify source imports, cooked packs, runtime bindings, connectivity and
   gameplay/persistence/presentation checks separately.
4. Inspect all final-route outcomes and code paths that detect dropped required
   objects, missing frames, unsupported actions and failed reads/streams.
5. Confirm maximum source completion plus non-contributing optional content,
   alternate endings/modes and the authentic new-game flow.
6. Reconcile all memory/performance and hardware requirements. Explain any
   approximation that remains; missing required evidence is not a pass.
7. Verify the final delivered pair, clean source-only Git state and reproducible
   builder. Do not mark complete because a time/token budget is nearly exhausted.

## 14. Copy/paste handoff prompt for GPT-5.6-Sol

```text
Work in /Users/ebonura/Desktop/repos/hk-psx. Execute the completion plan in
 docs/plans/COMPLETE_GAME_SOL.md and follow AGENTS.md, README.md,
 docs/BOOTSTRAP.md, NEXT_PROMPT.md and the latest docs/STATUS.md.

The objective is the entire installed Hollow Knight game playable on original
PS1 hardware, including optional content, endings and installed modes. Import
all areas, then implement shared systems globally. Do not redefine completion
as a demo, imported assets, a first boss, or one ending.

First revalidate the current repository, source and playable-disc state. Then
perform P01: diagnose and repair the interrupted whole-world importer, preserving
valid checkpoints and distinguishing lost-pool jobs from actual scene failures.
Proceed through the plan's dependency order. Implement and validate concrete
work; do not spend the run rewriting this plan or merely reporting status.

The plan was prepared against ecc697e. At that point only Tutorial and Town
were playable in disc71. Whole-world import had249 exported scenes,196 pool
failures and56 unrecorded scenes, without final source verification. Recheck
these facts before relying on them. Runner has a guest branch but its canonical
admission is disabled until real audio/dust ownership is bound; Climber is still
unbound. Current memory headroom was only8,044 bytes before the reserved stack.

Use the read-only Windows CrossOver installation and isolated controlled
original reference runner. Never use the macOS source or mutate original saves.
Keep320x240, current framing/layers/effects,48px scenery and approved95% sharing.
Use the pinned SDK/toolchain and final R3000 hazard checks. Do not fake missing
systems, no-op required scripts or silently drop required objects to fit budgets.

Preserve the user's current disc until replacement is requested/authorized.
Exactly one playable pair belongs in ~/Downloads/ps1 games/hk-psx.bin/.cue.
Use headless PSoXide; the user opens its GUI. No duplicate discs or automatic burns.
Source-only commits/pushes to private EBonura/hk-psx are authorized; retail assets,
converted files and captures remain ignored. Keep working changes and checkpoints
recoverable, and update the execution ledger plus NEXT_PROMPT.md after each
coherent package. Report actual tested behavior, remaining gaps and the exact
source-versus-playable-build state. Continue until the full plan is satisfied
or the user explicitly changes/stops the assignment.
```
