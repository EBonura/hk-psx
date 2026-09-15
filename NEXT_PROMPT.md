# Hollow Knight PS1: current handoff

## Validated state, September 15

Work in `/Users/ebonura/Desktop/repos/hk-psx`, branch `main`. The user asked to
salvage Claude's useful changes, remove its worktree, continue optimization,
and push the improvements to main. The worktree was clean and has been removed.
Its history remains on `claude/hk-psx-project-state-d72a31` at `8db50a5` and
remote `codex/bootstrap`. No sibling repository was changed. See
`docs/CLAUDE_OPTIMIZATION_REVIEW.md` for the complete nine-file salvage review.

**Pass40 is the current single playable disc. Stable 30 fps is NOT achieved.**
At320×240, the long route averages2.018783 VBlanks (about29.7fps):
184 /2,456 /232 /3 intervals at one /two /three /four ticks.235 of2,875 exceed
the two-VBlank budget. Pass36 was2.159226, with427 of2,688 over budget.
One-tick intervals improve the mean but do not excuse the remaining stutters.
The strict gate remains `--max-frame-route-ticks 2 --require-seamless`.

Retained this resumption:
- CPU-only next-background packet preparation during the existing GPU wait,
  with exact camera, framebuffer, sparse final visibility/gain/opacity matching.
  Source boundaries are atomic; stale prefixes fall back to ordinary emission.
- Reuse its108-byte initialized rectangular-occluder snapshot on exact hits.
  Alias invalidation covers BACK textures, FRONT collector textures and every
  texture participating in a tile certificate. No upload or GPU kick moves
  before the prior flip; animation lifetime remains unchanged.
- Claude's early bounds rejection, using480bytes of packed corner indices
  rather than7,680bytes of full extrema. Remaining original vertices/UVs and
  repair paths are unchanged. Full-frame pipelining was not imported.

Validation:
-236 Python tests and352 directly reported native/Rust tests passed.
- Final EXE hazard scan: zero. SDK7b929ce473ed44b10b0ffca08f413eebf4eeeec4,
  nightly2026-03-25, trim-paths and R3000 mitigation retained.
- Long user route: same endpoint region8, x163.153656/y6.390625, health4,
  exact final reference image, no input faults/missed samples, no traversal
  CD reads/static VRAM uploads/boundary waits. Strict frame interval gate fails.
- Actual-CUE Lifeblood: all four earned-health checks pass; two blue masks
  absorb damage before ordinary health. Final image matches reference.
- Actual-CUE Great Door:13 hits, transition intoTown region93, final image
  matches reference. One gate-transition wait tick remains. Pre-existing
  Lifeblood/Town particle drops remain14/23; do not claim those are solved.
- All replay input hashes remained unchanged. Captures were inspected.

Evidence: `.hkpsx/30fps-pass40-{profile,lifeblood,town}/`,
`.hkpsx/30fps-pass40-{build,tests}.log`, `.hkpsx/30fps-pass40-validation.png`.
EXE SHA256: `bb9d813577de1bbaf1d7c13b7239c7702b3a507a1491c7908c4fcb4283081b66`
BIN SHA256: `313bba9fcc82d22186be37a292540ef60ed06bfaf33027a6a9c60b123a67163d`
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

Pass40 code336,560 /data359,760 /BSS1,266,476; static span1,962,796 ending
0x801ef32c. Gap before the reserved48KiB stack:19,412bytes. Main frame25,536,
remaining nested reserve23,616. Scratch972/1,024bytes. These are linked budgets,
not stack high-water or console timing. Scene arena1,069,056bytes, aligned
minimum1,063,224. Both authored scenes/atlases reside before gameplay; this
covers98 spatial regions of Tutorial/Town, not98 full game rooms.

Earlier retained work: scoped collision readers, exact particle arithmetic,
incremental particle count, sparse visual-state reset, cached RGB/alpha/core
maps, bounded8-piece clipping of ordinary and repaired geometry, immutable
opaque-tile certificates, persistent coverage preparation, cooperative
presentation, source Great Door stages/progression and earned Lifeblood checks.
Source Great Door audio/debris/fade, complete effects, Focus controller-route
coverage, bosses/abilities/NPCs/saving/Shade/music/full-game progression remain
incomplete. See feature docs; do not infer full parity from passing these routes.

Next performance work must target remaining slow frames, with actual cycle
and packet measurements. Do not repeat these rejected trials unchanged:
compiler-wideO2/opt-s, greedy/tail scissors, more rectangle collectors,
rotated covers, texel-cell subdivision, full background cache or coarse tile
retry. Exact native pixels alone do not establish guest performance.

New bounded GPU audit: union-before-erosion certificates for adjacent immutable
FRONT tilemap fills are exact in principle, but2-member pairs save only0.288ms
in frozenregion18; groups3–4 save0.557ms (+4packets), region15 only0.171ms.
The initial ideal whole-union1.224ms estimate does not survive conservative
all-phase/split constraints. Do not integrate this version at that return.
Proof:53certificates,819joint phases,299,184,165 certified pixel checks;
`.hkpsx/pass36-certificate-gap-audit/`. Region membership, common parallax,
all member visibility and earliest contributor rank are required for any
future version. No production changes from this audit were made.

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
