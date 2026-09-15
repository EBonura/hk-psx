# Complete-game execution ledger

This ledger tracks implementation of `COMPLETE_GAME_SOL.md`. A package is
validated only when its acceptance criteria have current evidence. Source
extraction, cooking, runtime binding and playable validation remain separate.

## Current authority

- Source branch: `main`; implementation began from commit `8337d68`.
- Windows source: CrossOver Steam installation, Unity `6000.0.61f1`.
- Playable disc: build 71; Tutorial/King's Pass and Town/Dirtmouth only.
- Disc policy: preserve build 71 until the user requests a replacement.
- Current packages: P02-P03, classify whole-world source coverage and close
  reusable geometry gaps.

## Milestones

| Milestone | Packages | State |
| --- | --- | --- |
| M0: recover import | P00-P03 | in progress |
| M1: scalable content | P04-P08 | pending |
| M2: connected world | P09-P12 | pending |
| M3: persistent adventure | P13-P18 | pending |
| M4: full combat roster | P19-P22 | pending |
| M5: complete content | P23-P27 | pending |
| M6: release proof | P28-P31 | pending |

## Package state

| Package | State | Current evidence / next action |
| --- | --- | --- |
| P00 | validated | First-read documents, Git, doctor, source version, disc policy and live-process state rechecked. No importer/emulator process was active. |
| P01 | validated | All 501 BuildSettings scenes completed with source/code verification and zero scene failures. One isolated `Mines_16` worker ended on signal 11; its automatic fresh-process retry completed successfully and both attempt records are retained. |
| P02 | in progress | The coverage ledger inventories every scene, 1,119 TransitionPoint edges, 318 PlayMaker scene-load actions, 468 component types and 515 PlayMaker action types without claiming runtime support. Nineteen scripted actions map to exact gates, 11 LoadLevel actions map to scenes, 286 use runtime variables, and 174 of those variables have a catalogued initial-scene candidate. Resolve variable producers and classify scene roles with evidence. |
| P03 | in progress | 453/501 scenes now have complete source geometry extraction. Five explicit gap families remain. Exact affine circles, shipped Unity built-in meshes, particle payloads/renderers and tk2d generated-mesh aliases are global. TrailRenderer parameters are next. |
| P04-P31 | pending | Follow dependency order in the master plan. |

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
- Complete import: 501/501 processed, 2 imported, 499 partial, zero failed;
  source inputs and importer code remained unchanged through final verification.
- Supervision recorded 502 attempts: 501 completed directly and one
  `Mines_16` attempt ended on signal 11. Only that scene was retried, and the
  retry completed in a fresh worker.
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
- The current verified import completed 501/501 scenes: 453 imported, 48 partial,
  zero failed. One `Pre_Menu_Intro` worker ended on signal 11; its per-run retry
  completed, proving retry budget is independent of persistent attempt numbers.
- Remaining gaps: 748 TextMeshPro glyph meshes in 38 scenes; 25 other
  runtime-generated meshes in seven scenes; 831 TrailRenderers in six scenes;
  16 null tk2d sprite collections in four scenes; four MeshRenderers without an
  exact single MeshFilter in one scene.

### Next concrete action

Preserve exact TrailRenderer source parameters/material dependencies for all 831
instances, then inspect the 25 unowned runtime meshes and 16 null tk2d
collections individually. Text shaping remains a separate renderer/cooking
system and must not be reported as extracted glyph geometry.

## Required package completion record

For every validated package, add source contract/version, covered instances and
variants, implementation paths, remaining exceptions, commands actually run,
original-game evidence, guest/disc hashes where applicable, inspected output,
memory/performance effects, persistence/reset evidence and the next package.
