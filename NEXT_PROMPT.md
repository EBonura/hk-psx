# Hollow Knight PS1: current handoff

## Validated state, September 15

Work in `/Users/ebonura/Desktop/repos/hk-psx`, branch `main`. The user asked to
salvage Claude's useful changes, remove its worktree, continue optimization,
and push the improvements to main. The worktree was clean and has been removed.
Its history remains on `claude/hk-psx-project-state-d72a31` at `8db50a5` and
remote `codex/bootstrap`. No sibling repository was changed. See
`docs/CLAUDE_OPTIMIZATION_REVIEW.md` for the complete nine-file salvage review.

**Pass48 is the current single playable disc. Stable 30 fps is NOT achieved.**
At 320×240, the long route averages 1.983254 VBlanks (approximately 30 fps at
nominal 60 Hz): 243 / 2,494 / 184 / 5 intervals at one / two / three / four ticks.
189 of 2,926 exceed the two-VBlank budget, versus 235 of 2,875 at pass40.
One-tick intervals improve the mean but do not excuse the remaining stutters.
The strict gate remains `--max-frame-route-ticks 2 --require-seamless`.

Retained this resumption:
- Exclusive scratch phases overlay the 600-byte owner grid with draw storage.
  32 terminal tile rectangles fit in 948 scratch bytes; ordinary subtraction
  remains bounded to eight. GPU packet capacity remains 1,032.
- All-phase seam proofs across 2–4 original opaque pieces: 124 groups, 8,180
  cooked bytes, complete region membership and live-state guards. A temporary
  3,040-entry inverse map avoids repeated activation searches. Joint coverage
  retires at the earliest contributor and cannot displace a later owner.
- Empty-mask preflight, packed nibble run scanning and preparation during the
  first idle window. Exact camera/state validation still controls prefix reuse.
- Exact radix16 roots, cached collision-particle sweeps and axis-aligned enemy
  probes. Particle trajectories and surviving source sampling are unchanged.

Earlier retained work includes CPU-only background packet preparation during
existing GPU waits, exact sparse state matching, reusable rectangular-occluder
snapshots and complete texture-alias invalidation. No upload or GPU kick moves
before the prior flip; animation lifetime remains unchanged. Claude's early
bounds rejection uses 480 bytes of packed indices rather than 7,680 bytes of
full extrema. Full-frame pipelining was not imported.

Validation:
- 241 Python tests and 358 directly reported native/Rust tests passed.
- Final EXE hazard scan: zero. SDK 7b929ce473ed44b10b0ffca08f413eebf4eeeec4,
  nightly2026-03-25, trim-paths and R3000 mitigation retained.
- Long user route: same endpoint region8, x163.153656/y6.390625, health4,
  exact final reference image, no input faults/missed samples, no traversal
  CD reads/static VRAM uploads/boundary waits. Strict frame interval gate fails.
- Actual-CUE Lifeblood: all four earned-health checks pass; two blue masks
  absorb damage before ordinary health. Final image matches reference.
- Actual-CUE Great Door: 13 hits, transition into Town region93, final image
  matches reference. All three routes now observe zero boundary waits.
  Pre-existing Lifeblood/Town particle drops remain 14/23; these are not solved.
- All replay input hashes remained unchanged. Captures were inspected.

Evidence: `.hkpsx/30fps-pass48-{profile,lifeblood,town}/`,
`.hkpsx/30fps-pass48-{build,tests}.log`, `.hkpsx/30fps-pass48-validation.png`.
EXE SHA256: `364147ad30a4d883db55aba6d5127d804771db98c79780513aec519e59ed2649`
BIN SHA256: `f1899737fef409d0c0814ae31a6f7f1f0e5938260e729e3fde02b4b5ae358838`
`.hkpsx/build.json` binds all current outputs and the exact map. Profile
`artifacts/` contains independent EXE/map/build-report copies; use these for
historical comparisons, since ordinary dist/build files are overwritten.

## Constraints

Keep exactly one playable pair, ALWAYS and ONLY in the user's library:
`/Users/ebonura/Downloads/ps1 games/hk-psx.bin` and `hk-psx.cue`.
No candidate, baseline, backup or telemetry disc copies. Every build replaces
this pair. Root alone builds/writes; concurrent headless readers must finish
before another build. The user opens PSoXide themselves; do not control its GUI.

The user explicitly requires320×240, the same camera framing, all layers and
effects. Keep existing48-pixel scenery cap and approved95% static deduplication;
no further resolution loss, fog removal or lossy merging was authorized.
The original Windows Steam CrossOver files are readonly build inputs, Unity
6000.0.61f1. Never use the macOS copy. Converted assets, captures, source
inspection and generated tables stay ignored. Source-only push to main was
explicitly authorized; no burns or retail uploads. No persistent goal or
scheduled automation was created by this session.

Read AGENTS.md, README.md, docs/BOOTSTRAP.md and docs/STATUS.md; run doctor.
Use psoxide-debug for guest/emulator work. Previously, Claude's worktree
symlinked data/build/dist/.hkpsx into this checkout and concurrent recooks
caused SHA mismatches; that worktree is now removed. Never reproduce this setup.

## Budgets and remaining work

Pass48 code 341,048 / data 369,600 / BSS 1,266,480; static span 1,977,136
ending 0x801f2b30. Gap before the reserved 48 KiB stack: 5,072 bytes. Main frame
25,536, remaining nested reserve 23,616. Scratch 948/1,024 bytes. Group-binding
initialization has a 6,288-byte frame (main + init = 31,824 before deeper calls
and IRQ use). Particle tick has a 4,352-byte frame; main + State + tick + collide
is 30,960 before deeper calls/IRQ use. These are linked budgets, not measured
stack high-water or console timing. Reclaim RAM before expanding content.
Scene arena 1,069,056 bytes, aligned minimum 1,063,224. Both authored scenes/
atlases reside before gameplay; this covers 98 spatial regions of Tutorial/
Town, not 98 full game rooms. VRAM/SPU allocations did not grow this pass.

Earlier retained work: scoped collision readers, exact particle arithmetic,
incremental particle count, sparse visual-state reset, cached RGB/alpha/core
maps, bounded clipping of ordinary and repaired geometry, immutable opaque-tile
certificates, persistent coverage preparation, cooperative presentation,
source Great Door stages/progression and earned Lifeblood checks.
Source Great Door audio/debris/fade, complete effects, Focus controller-route
coverage, bosses/abilities/NPCs/saving/Shade/music/full-game progression remain
incomplete. See feature docs; passing these routes does not imply full parity.

Next performance work must target the remaining 189 slow frames, with actual
cycle and packet measurements. Particle bursts and coverage/prefix misses
remain important; traversal CD/static VRAM loading does not explain this route.
Do not repeat these rejected trials unchanged: compiler-wide O2/opt-s,
greedy/tail scissors, more rectangle collectors, rotated covers, texel-cell
subdivision, full background cache, coarse tile retry or textured-source
outlining (pass47 was slightly slower and fully reverted in pass48).
Final-DMA instrumentation observed 2,906 build ends and zero busy DMA windows;
that wait is not a useful new preparation opportunity. Instrumentation was
removed. Capacity32 fits only through the scratch phase overlay; do not enlarge
it beyond the 1 KiB scratch budget. Exact native pixels alone do not establish
guest performance.

The initial grouped-certificate audit was superseded by production groups:
124 groups, 2,054 joint phases and 281,016,479 certified native GPU pixel checks;
12 full frames / 921,600 exact pixels across both framebuffer positions and
three backgrounds. Source filtering, complete membership, live flags/opacity,
common scale/parallax and earliest-contributor rank are mandatory. Evidence:
`.hkpsx/opaque-groups/` and
`.hkpsx/pass36-certificate-gap-audit/PRODUCTION-GROUPS-RESULT.md`.
Pass45's repeated linear binding caused activation spikes; inverse binding in
pass46 fixed that regression. Keep the inverse map and empty-mask preflight.

## Reproduce

```sh
python3 tools/build.py
make test
python3 tools/profile_tape.py --exe dist/hk-psx.exe --map build/hk-psx-normal.map --cue '/Users/ebonura/Downloads/ps1 games/hk-psx.cue' --tape .hkpsx/user-6246-input/latest.pxtape --output .hkpsx/next-profile --screenshot-interval 60 --max-frame-route-ticks 2 --pc-window-ticks 1 --require-seamless
python3 tools/replay_cue.py --tape .hkpsx/lifeblood-validation-plan/attempt5.pxtape --output .hkpsx/next-lifeblood --screenshot-interval 30
python3 tools/validate_lifeblood.py --replay .hkpsx/next-lifeblood
python3 tools/replay_cue.py --tape .hkpsx/town-validation-search/attempt3.pxtape --output .hkpsx/next-town --screenshot-interval 30
```

Use fresh ignored report paths; preserve artifact hashes. PC samples are every
4,096 retired instructions, not elapsed time. Discarded background preparation
can increment CPU scenery counters without submission: use GPU stats and
actual displayed intervals for delivered performance. Never hide gate failures
by loosening the threshold or dropping input. GPU timing is an emulator model;
original-hardware claims require original-hardware measurements.
