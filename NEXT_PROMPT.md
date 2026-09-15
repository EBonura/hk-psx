# Hollow Knight PS1: current handoff

## Validated state, September 15

Work in `/Users/ebonura/Desktop/repos/hk-psx`, branch `main`. The user asked to
salvage Claude's useful changes, remove its worktree, continue optimization,
and push the improvements to main. The worktree was clean and has been removed.
Its history remains on `claude/hk-psx-project-state-d72a31` at `8db50a5` and
remote `codex/bootstrap`. No sibling repository was changed. See
`docs/CLAUDE_OPTIMIZATION_REVIEW.md` for the complete nine-file salvage review.

**Pass51 is the current single playable disc. Stable30fps is NOT achieved.**
At320×240 the long route averages1.976839 VBlanks:256 /2,501 /170 /9 intervals
at one /two /three /four ticks.179/2,936 exceed two ticks, versus189/2,926 in48.
One-tick intervals improve the mean but do not excuse remaining stutters.
The strict gate remains `--max-frame-route-ticks 2 --require-seamless`.

Retained this resumption:

- HKSCNE02 removes static atlas bytes from resident RAM. Bootstrap uploads23
  validated chunks through the empty arena, synchronizes each upload, then
  admits compact scenes. Arena1,069,056 ->369,180 bytes; raw records369,172.
  Retry resets admissions before menu art can overwrite the atlas. Packed320B
  texture flags replace CPU palette/texel reads during activation.
- Host full HKSCNE01 files and existing raw/stored provenance fields remain
  intact for texture/occlusion analysis. New resident_raw/resident_stored fields
  bind compact scene chunks. Never pair a pre49 EXE with the new-format disc.
- Tile retirements use source-row masks rather than one event per cell. Exact
  ordering is preserved, including disconnected cells from the same owner.
- Scratch remains948/1024 bytes. The pass50 direct-column run map was
  pixel-exact but regressed late intervals189 ->195 and was reverted.
  Its7.1% isolated instruction reduction did not improve delivered cadence.
- Existing32 terminal pieces,8 subtract pieces,124 grouped seam certificates,
  inverse activation binding, exact roots/sweeps and prefix reuse remain.

Earlier retained work includes CPU-only background packet preparation during
existing GPU waits, exact sparse state matching, reusable rectangular-occluder
snapshots and complete texture-alias invalidation. No upload or GPU kick moves
before the prior flip; animation lifetime remains unchanged. Claude's early
bounds rejection uses 480 bytes of packed indices rather than 7,680 bytes of
full extrema. Full-frame pipelining was not imported.

Validation:

- 246 Python tests and365 directly reported native/Rust tests passed.
- Final EXE hazard scan0. SDK7b929ce473ed44b10b0ffca08f413eebf4eeeec4,
  nightly2026-03-25, trim-paths and R3000 mitigation retained.
- Long route endpoint region8 x163.153656/y6.390625 health4, no input faults,
  missed polls, traversal CD/static uploads or boundary waits. Strict frame
  gate fails. Final89 changed pixels are exclusively within the Knight sprite;
  scenery/HUD identical to49. Final route includes one additional tick.
- Actual-CUE Lifeblood passes all four health checks; final image exact49.
- Actual-CUE Great Door13hits, opened1, transition1, Town region93, image exact49.
  Lifeblood boundary waits0; Town2 ticks waiting for a frame boundary without
  CD/static upload work. Pre-existing particle drops14/23 remain unresolved.
- Inputs hash-stable; captures inspected. Final actualCUE tests are complete.

Evidence: `.hkpsx/30fps-pass51-{profile,lifeblood,town}/`,
`.hkpsx/30fps-pass51-{build,tests}.log`, `.hkpsx/30fps-pass51-validation.png`,
and `.hkpsx/30fps-pass51-final-difference.png`.
EXE SHA256: `b9dcb98abf2fb765a1da69e6a157a77873d35605ec2bd4313ce6ad2437f0ff4c`
BIN SHA256: `0d34a4dedbdd1629d2d766550b6f2350bdbc2fd3201d89ea7f03a42bf167534e`
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
explicitly authorized; no burns or retail uploads. An active persistent goal
records the strict FPS gate followed by full
start-to-finish gameplay and remaining content. Do not mark it complete for
average30fps or a first ending. No scheduled automation was created.
Gameplay implementation begins after the FPS gate; see docs/ROADMAP.md.

Read AGENTS.md, README.md, docs/BOOTSTRAP.md and docs/STATUS.md; run doctor.
Use psoxide-debug for guest/emulator work. Previously, Claude's worktree
symlinked data/build/dist/.hkpsx into this checkout and concurrent recooks
caused SHA mismatches; that worktree is now removed. Never reproduce this setup.

## Budgets and remaining work

Pass51 code337,340 /data369,216 /BSS567,800; static span1,274,360 ending
0x801471f8. Gap before reserved48KiB stack707,848 bytes. Main frame25,464,
remaining nested reserve23,688. Scratch948/1,024 bytes. These are linked budgets,
not measured stack high-water. Scene arena369,180, aligned minimum369,176.
Both authored scenes/atlases reside before gameplay:98 spatial regions of
Tutorial/Town, not98 full rooms. Static atlas payload694,048 bytes remains in
VRAM;23 separate startup chunks total321,841 stored bytes. Compact scene chunks
add206,518 stored bytes. TotalWORLD.PAK entries31 including6 ambience clips.
VRAM/SPU allocations are unchanged. Native full/compact comparisons verified
146,798 runtime records across98 regions, unchanged animation/alpha bytes and
all final texture flags. Tests cover startup retries, every atlas destination,
GPU synchronization before buffer reuse and checksum failures.

Earlier retained work: scoped collision readers, exact particle arithmetic,
incremental particle count, sparse visual-state reset, cached RGB/alpha/core
maps, bounded clipping of ordinary and repaired geometry, immutable opaque-tile
certificates, persistent coverage preparation, cooperative presentation,
source Great Door stages/progression and earned Lifeblood checks.
Source Great Door audio/debris/fade, complete effects, Focus controller-route
coverage, bosses/abilities/NPCs/saving/Shade/music/full-game progression remain
incomplete. See feature docs; passing these routes does not imply full parity.

Next performance work must target remaining slow frames using actual cycle
and packet measurements. Pass49 audit found all9 four-tick stalls during
particle/debris bursts;115/189 late intervals had sampled GPU accounting below
the two-tick budget. Exact timelines show CPU simulation catch-up starving GPU
submissions. FIRST_KICK_LINES starts inside render and excludes that prior
simulation delay. Evidence: .hkpsx/precompute49-tail-audit/RESULT.md.
The next candidate is materially broader immutable particle precomputation,
with exact RNG, collisions, nonlinear state and all rendered fields preserved.
Phase-index-only caching (~0.03ms) and static edge grids (~0.043ms) were already
rejected; existing cached sweeps/edge filtering must not be rediscovered.
A combined static coverage field was also rejected:52,928 compressed bytes plus
3,108 metadata yielded no additional cuts above the1,024-pixel threshold in
heavy18/20, while a replacement lost certified ownership. Free RAM alone does
not justify it. Report .hkpsx/precompute49-group-audit/report.md.
Exact temporal run memoization audit is in .hkpsx/precompute50-run-cache-audit;
a256-entry cheap hash predicts27.18% hits but only319 cycles/call overhead
margin. Heavy region20 has2/2,685 hits;15 allows161 cycles/call. Global caching
is not recommended. The49 trace matches full RAM/display/all timing rows and
140,934 native return-oracle calls; the50 helper was faster in isolation but regressed full-route cadence.
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
