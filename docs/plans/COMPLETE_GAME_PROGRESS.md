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
| P02 | in progress | A generated coverage ledger now inventories every scene, 1,119 transition edges, 466 component types and 515 serialized PlayMaker action types without claiming runtime support. Resolve source/runtime transition overrides and classify scene roles with evidence. |
| P03 | in progress | Nine reusable extraction-gap families are ranked by scene coverage. The first implementation target is exact affine circle-collider geometry: 5,274 instances in 292 scenes. |
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

### Next concrete action

Extract exact transformed circle boundaries for all 5,274 serialized
CircleCollider2D instances, regenerate the verified 501-scene report, then
continue down the reusable geometry-gap inventory while P02 resolves scripted
transition destinations.

## Required package completion record

For every validated package, add source contract/version, covered instances and
variants, implementation paths, remaining exceptions, commands actually run,
original-game evidence, guest/disc hashes where applicable, inspected output,
memory/performance effects, persistence/reset evidence and the next package.
