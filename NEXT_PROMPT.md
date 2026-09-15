# Hollow Knight PS1: current handoff

## Validated state, September 15

Work in `/Users/ebonura/Desktop/repos/hk-psx`, branch `main`. The user asked to
salvage Claude's useful changes, remove its worktree, continue optimization,
and push the improvements to main. The worktree was clean and has been removed.
Its history remains on `claude/hk-psx-project-state-d72a31` at `8db50a5` and
remote `codex/bootstrap`. No sibling repository was changed. See
`docs/CLAUDE_OPTIMIZATION_REVIEW.md` for the complete nine-file salvage review.

**Pass54 is retained and restored as57 in the single playable disc. Stable30fps
is NOT achieved.** The long route averages1.957504 VBlanks:285 /2,527 /147 /6
intervals at one /two /three /four ticks;153/2,965 exceed two, versus179/2,936
and9 four-tick in51. Strict gate remains `--max-frame-route-ticks 2 --require-seamless`.

Retained source: exact source-seeded destruction-particle tracks for1,513
ordinals/71 emitters.33 two-word phases cache half-size, radius, base spin and
opacity. Host RNG consumption is exact; live physics/collision stays runtime.
Generated-slice identity guards and unpackable-emitter scalar fallback remain.
One-based live IDs use0 for empty/scalar. Table payload399,432B, emitter bases142B,
live IDs256B; Particle48B, Pool6,416B. Earlier compact HKSCNE02 atlas separation,
row events and exact occlusion/prefix preparation from51 are unchanged.

Rejected experiments:52 square-corner reuse193 late;53 MAX live sentinel184late;
55 admitted hidden-scan outlining175late/3four;56 outlined State construction
158late/7four.54 has153late/6four.56 did shrink main32,104 ->18,168B, but its14,624B
constructor still contains intermediate copies, and cadence worsened. It is
reverted.55's exact cache trace proved persistent4KiB scenery conflicts, not an
automatic speedup from extraction. Production render.rs and world.rs match51.

Validation:247 Python/367directnative tests, hazards0. The portable proof
`.venv/bin/python tools/test_particle_tracks.py` adds199,980 exact source phases
and334,107 trajectory shape/draw comparisons including contacts and pool reuse.
It requires local cooked Windows reports and writes ignored proof fixtures.
Long endpointregion8 x163.153656/y6.390625 health4, no input faults/missed polls,
traversal CD/static uploads or boundary waits. Final89 changed pixels versus51
are within the Knight; scenery/HUD exact. Actual-CUE Lifeblood all4checks pass,
Great Door13hits/opened1/transition1/Town93. Both final images exact51. Life waits0,
Town1 tick waiting for frame boundary withoutCD/staticupload; drops14/23 remain.

Evidence: `.hkpsx/30fps-pass54-{profile,lifeblood,town}/`,54validation.png,
55/56 replays and logs, `.hkpsx/particle-track-audit/portable/proof.json`,
`.hkpsx/pass53-icache-audit/RESULT.md`, `.hkpsx/scenery-program-audit/RESULT.md`.
The57 rebuild restores the retained54 binary, with identity proof in
`.hkpsx/30fps-pass57-identity.json`. Historical profile artifacts contain frozen
EXE/map/build reports; no historical disc copies exist. Source is pushed to main.

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

Retained54/57 code338,116 /data769,840 /BSS567,800; static span1,675,768 ending
0x801a91f8. Gap before reserved48KiB stack306,440 bytes. Main frame32,104,
remaining nested reserve17,048. Scratch948/1,024 bytes. Linked budgets are not
measured stack high-water. Scene arena369,180, aligned minimum369,176.
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
Particle precomputation is now retained. The54 tail audit reports116 background
prefix misses and66 frames with40+ particles among153 late intervals. Next
inspect residual simulation/scenery critical paths, not more blind code layout
trials. Immutable Z-force interpolation is a small bounded possibility; the
source-program audit estimates1.4–1.7ms gross addressable work in heavy15/18/20
before lookup and idle-prefix overlap. Exact repair requires anchor parity and
viewport-dependent subdivision; no such cache is implemented.
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
