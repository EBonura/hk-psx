# Performance investigation

## Deferred 30 fps target (2026-09-15)

The user paused optimization to playtest build63 with cheats. Its renderer
retains54; the timings below describe the pre-cheat54 binary, not a new63 FPS
claim. No further performance experiments are authorized by this playtest task.

The acceptance budget is **at most two VBlanks per rendered gameplay frame**.
The retained pass54 disc (restored as57) averages1.957504VBlanks (approximately30fps at
nominal60Hz), with285 /2,527 /147 /6 intervals at one /two /three /four ticks.
The strict gate still fails on153 of2,965 intervals. Pass48 had189 of2,926,
mean1.983254. One-tick intervals improve the mean; they must not disguise
remaining three/four-tick stutters. Original route mean was2.5523VBlanks.

Pass37 prepares a bounded background packet prefix after final DMA drains,
while the prior GPU frame finishes. Exact camera, framebuffer and final sparse
scenery state are checked before reuse; mismatches fall back. Pass39 retains
its108-byte rectangular-occluder snapshot too. No GPU commands or animation
uploads move before the prior flip. Pass40 salvages Claude's admission-time
bounds rejection using four packed corner indices per draw, consuming480bytes
instead of7,680. Original surviving vertices, texture sampling and draw order
remain unchanged. User explicitly keeps320×240, framing, layers and effects.

Final Lifeblood and Town captures match their references exactly. The long-route
endpoint is unchanged;89 final pixels differ only within the Knight sprite,
with one additional route tick. All scenery/HUD pixels match49. Lifeblood's four
earned-health checks pass; the Great Door opens after13hits. Long/Lifeblood have
zero boundary waits;Town records1 frame-boundary wait ticks without CD/static
uploads. Pre-existing Lifeblood/Town particle drops14/23 remain. No input faults
or missed samples; input hashes stable. Tests247Python/367directnative pass,
final binary hazards0. These are emulator measurements, not silicon proof.

Greedy/tail clipping, compiler-wide O2/opt-s, rotated-cover and texel-cell trials
were rejected or left out after measured regressions. See STATUS for the full
table and CLAUDE_OPTIMIZATION_REVIEW for the removed worktree's salvage review.
CPU scenery/scissor counters can include discarded speculative preparation;
use GPU statistics and displayed intervals for delivered performance, and
prefix counters for attempts, hits and reused packets.

Use `tools/profile_tape.py --max-frame-route-ticks 2 --pc-window-ticks 1
--require-seamless` with the actual EXE/map/canonical CUE and saved input tape.
PC sampling is every 4,096 retired instructions; PC windows are one VBlank in
these runs. Additional idle-loop samples after a speedup are not regressions
in useful work. Normalize GPU/CPU totals by frame count and inspect slow-region
critical paths, not just route means. Initial GPU kicks start earlier, and
cooperative presentation checks reduce final-kick to queue delay to at most
one scanline in burst regions. GPU-heavy regions and particle update bursts
remain above budget. No original-console timing claim is made.

Secondary solid bands were pixel-exact but regressed pass15: the actual GPU
saving was much smaller than a snapshot model, and CPU costs outweighed it.
They are removed. Exact tests and snapshots establish correctness, not speed;
only full actual-CUE timing establishes the measured outcome.

### Broader coverage investigation

The three host-cooked opaque rectangle certificates passed all rounding-phase
and native GPU correctness checks, but their realistic snapshot gain was only
1.03/1.25 ms in regions18/20 while adding66/104 packets. They are not integrated.
Evidence: `.hkpsx/gpu-30fps-audit/coverage-cert/report.json`.

The initial tile oracle was only an upper bound. Production now uses exact
host-certified4px phase coverage (84,284 bytes) and20×15 screen-tile owners.
Eight-piece scissors retain original vertices, UVs and source order. Pass19
regressed because per-cell CPU loops outweighed raster savings. Pass20 replaces
these with event-driven15-word row masks; pass21 prepares during GPU waits.
All64 pass21 four/five-tick frames lacked preparation and had45–115 particles.
Pass22 therefore skips all additional tile processing on a preparation miss.
This preserves ordinary rendering and removes the five-tick spikes;17 four-tick
frames remain. Native GPU hit/miss proofs cover48 frames /3,686,400 identical
pixels. Exactness evidence is `.hkpsx/gpu-30fps-audit/tile-cert/`; slow-frame
correlations are `.hkpsx/tile-spike-audit/`. No timing claim follows from native
raster-only estimates; complete guest replays remain decisive.

## Compact residency and precomputation follow-up (September15)

Pass49 separates startup-only static atlases from runtime scene records. Both
atlases remain in VRAM; the resident arena shrinks1,069,056 ->369,180 bytes.
Packed texture facts are derived from exact final atlas words during cooking,
so scene activation needs no retained palette/texel copies. Row retirement
nodes coalesce same-source cells in one row and preserve exact retirement order.
The measured49 route mean improves1.983254 ->1.978520, but189 intervals remain
late (180 three-tick and9 four-tick). The memory win does not establish stable30fps.

Pass50's direct-column run map removed7.1% of isolated helper instructions and
preserved exact ordered outputs. The complete route instead produced195 late
frames, mean1.978861, with10 four-tick intervals. It was reverted for pass51.
Native microbenchmarks are a screening tool; the actual guest replay decides.

The combined static-field audit found no additional accepted cuts above the
existing1,024-pixel threshold in heavy18/20. A52,928-byte compressed field plus
3,108 metadata also lost certified ranks when substituted for current coverage.
The prototype is not integrated. Exact temporal run-cache tracing found27.18%
hits for256 entries but only319 cycles/call gross overhead margin; region20
has2 hits in2,685 calls. Generic caching is not justified by average hit rate.
The trace matched every route timing row, entire RAM and final display;140,934
calls passed the native return oracle. See ignored precompute49-group-audit
and precompute50-run-cache-audit reports for reproducible data.

The49 critical-path audit instead shows particle/debris simulation bursts
starving GPU submission. All9 four-tick cases have57–85 particles and4–20 debris;
115/189 late intervals have sampled GPU command accounting below two ticks.
Accounting can straddle display frames, so this is not a hard GPU lower bound.
FIRST_KICK_LINES starts within render and omits preceding simulation delay.
This motivated the retained immutable particle precomputation below, with exact
RNG, nonlinear movement, collision and rendered-state comparison. Phase indexing
alone and static edge grids were already measured too small to justify adding.
Evidence: `.hkpsx/precompute49-tail-audit/RESULT.md`.

The source-local scenery-program audit found another possible future cache:
immutable local alpha/core seed partitions plus repaired vertices, while keeping
live cuts and packet-budget decisions current. Exact repair depends on anchor
parity for ties-to-even and viewport-dependent subdivision selection; a single
relative child array is unsafe. Addressable sampled instruction work is only
about1.4–1.7ms in heavy15/18/20 before lookup cost and overlap with idle prefix
preparation. No implementation or guaranteed saving is claimed. Evidence:
`.hkpsx/scenery-program-audit/RESULT.md`.

## Particle precomputation experiments

Pass52 tried exact square-corner reuse: two signed half-size/trigonometric
products derive four corners with the same per-product truncation as the
original rotations. Fractional-angle, signed-size and randomized native tests
passed. The full route nevertheless regressed from179/2,936 late intervals
to193/2,931, mean1.980212 with8 four-tick intervals. The change was reverted.
No reduction in isolated arithmetic is treated as delivered performance proof.
Evidence: `.hkpsx/30fps-pass52-profile/` and corresponding build/test logs.

Pass53 adds host-cooked tracks for1,513 source-seeded particle ordinals across
71 emitters. Each of33 phases stores exact half-size, collision radius, base spin
and quantized opacity in two words. This costs399,432 bytes plus142 bytes of
emitter bases;128 live IDs add256 bytes to the pool. RNG consumption, particle
counts and live force, damping, motion, collision and speed-dependent spin remain
unchanged. Unsupported future emitters retain the scalar path, without clamping.
Generated-slice identity guards prevent generic emitters from binding these tracks.

Pass53 was mixed: mean1.971467,184/2,944 late intervals, including6 four-tick.
Pass54 changes live IDs to one-based with zero as the scalar sentinel. It reaches
mean1.957504,153/2,965 late intervals, including6 four-tick, versus179 and9 in51.
Its main frame is still32,104 bytes, only8 fewer than53. The initial hypothesis
that zero initialization would remove a large temporary was not established.
Exact guest disassembly verifies that the cooked path is used and the old scalar
shape path receives no sampled ticks in the53 profile. Track data raises the
static span to1,675,768 bytes, leaving306,440 before the protected48KiB stack.

The portable source-data proof ran with `tools/test_particle_tracks.py`
(retired with the baked tracks once effect art moved to per-scene disc chunks). It compares199,980 phase values across6,060 admitted/truncated/directional
instances and334,107 cached draw/shape states over650-tick trajectories per owner.
Floor/slope contacts, removed terrain, scene pauses and pool reuse match the
scalar implementation. The lightweight suite includes a synthetic full-pool
trajectory proof and explicit unpackable-emitter fallback. Generated proof
fixtures and source assets remain ignored. Exactness is not a timing guarantee.

An exact53 I-cache event capture reproduced all1,672 route rows byte-for-byte.
It found7.30M scenery self-eviction cycles across186 display changes, including
conflicts between admitted hidden-occluder scans and packet construction4KiB
apart. This is a gross ceiling, not an achievable saving. Total53 I-cache stalls
were already12.48M lower than51, while stack RAM stalls were25.35M higher; this
conflict is not a proved cause of53's mixed result. The bounded55 experiment
keeps cheap empty/bounds/union rejection inline and outlines only the admitted
scan and multi-rectangle subtraction. Its200,000-case native comparison preserves
Boolean results, counters and initialized partitions. The full55 replay gives175/2,957 late intervals, including3 four-tick,
mean1.962800. Compared with54 it reduces the worst stalls but adds22 missed
deadlines overall, so the extraction was reverted. Lifeblood/Town images and
checks remained exact. Evidence: `.hkpsx/pass53-icache-audit/RESULT.md`.

Pass56 keeps world construction out of line. Final MIPS confirms that startup
and reset pass the final State as the return destination. Main shrinks32,104
->18,168 bytes, but a14,624-byte constructor frame still holds intermediate
arrays/copies; combined construction frames total32,792 before nested calls.
This reduces the permanent gameplay frame, not total initialization work.
Despite that benefit, the route regresses against54:158/2,954 late intervals,
including7 four-tick, mean1.964455. It was reverted to retain54's better frame
cadence. This remains a possible later stack-budget change if separately needed.
The actual-CUE health/progression checks and final Lifeblood/Town images passed.

## Follow-up after retained54/57

Pass58 tried a1,513-byte immutable Z-force table for the existing particle track
IDs. Source comparisons verify6,060 exact forces, including4,419 nonzero cases,
plus the existing199,980 phase and334,107 trajectory/draw checks. Wider, signed,
nonzero-X and unbound cases retain scalar evaluation. The248 Python and369
native suite results cover the final candidate. The guest selects the byte table,
but LLVM newly outlines tick_shape and introduces a call and three stack stores
per particle.58 regresses to185/2,935 late intervals,9 four-tick, mean1.977513.
59 forces only that helper inline and reaches161/2,955 late,7 four-tick,
mean1.964129. Both are rejected against54's153/2,965 and6 four-tick. All three
final images match54, health/progression pass, and no traversalCD/staticuploads
or input faults appear. Evidence: `.hkpsx/force58-audit/RESULT.md` and58/59 replay
artifacts. The table is not retained merely because its arithmetic is exact.

Further precompute audits rule out several tempting shortcuts. A128-entry
alpha/core seed cache needs roughly25.6KiB but retains most of its~1.2ms/frame
gross scope in heavy15/18/20 for clipping, ordering and thresholds. A copied
full-local final partition can change rectangle order after viewport clipping,
which alters bounded fallback; source tags and live clipping would still be
required. See `.hkpsx/seed-cache58-audit/RESULT.md`.

Exact native packet census finds only0.69/0.33/0.25ms gross axis colored-core
opportunity in15/18/20. Rotated fog cores offer0.60/1.66ms in18/20 before
certificates, extra partitions and mask-state costs. Semi-transparent flat
replacement must preserve framebuffer bit15, ABR and mask state; matching RGB
alone is insufficient. No colored-core substitution is integrated. Evidence:
`.hkpsx/constant-color59-audit/RESULT.md`.

A minimum-two-VBlank presentation cap also lacks evidence as a late-frame fix.
All285 one-tick54 intervals are inregion8; none occurs within five intervals
before any of153 late frames. Delaying queue submission would also lose current
queued-GPU prefix overlap unless its ownership protocol changed. Keep ordered
60Hz simulation and raw flip intervals; do not disguise missed deadlines with
normalized counters. The cap is not implemented.

Pass60 admitted already-computed256..1023-pixel cuts only when their output
piece count did not exceed the input count. It regressed to179/2,953 late
intervals,5 four-tick, mean1.965459. All three final images were exact54;
Lifeblood passed, and Town completed with2 frame-boundary wait ticks and no
traversal CD/static uploads. A smaller raw GPU total did not establish a win
because the trial also displayed fewer frames. The change was reverted.

Pass61 fused the region-admission extrema scans and reused selected extrema
for the rotated core-size gate. Exact tie/index/overflow checks passed, including
2 million full-i32 quads. Final render initialization shrank528 code bytes and
removed duplicate comparisons, but the route regressed to175/2,943 late
intervals,4 four-tick, mean1.972477. Lifeblood's four checks and Town progression
passed with zero boundary waits; both final images were exact54. The long
endpoint was unchanged, with only a Knight-animation difference in the final
image after one extra route tick. This candidate was also reverted. Evidence:
`.hkpsx/activation58-audit/pass61-full.patch` and60/61 actual-CUE reports.

The finer16x8 tile-grid estimate was corrected to include setup for nonempty
polygons and per-polygon raster rounding. Its incremental modeled GPU credit
is only0.271/0.232ms in regions18/20, before767/858 additional GP0 words and CPU
work. The8x8 grid has0.538/0.790ms credit, before1,556/1,659 extra words. Neither
justifies integration on current evidence. See
`.hkpsx/fine-tiles60-audit/net-cost.json`. A framebuffer mask-bit prepass is also
not a measured performance option: the pinned emulator charges polygon work
before raster mask rejection, so it retains the underlying polygon cost and
adds a prepass. Changing the emulator timing to manufacture a win is excluded.

Pass62 required at least256 removed bbox pixels per additional tile partition,
with the existing1024-pixel floor. It regressed to184/2,945 late intervals,
5 four-tick, mean1.971138, and was reverted. Lifeblood/Town images matched54;
the long-route final difference was confined to the Knight after one extra
route tick. Trace scope showed only1,263 rejected cuts of71,987 successes,
including speculative work; this was packet throttling, not exact GPU costing.
The user then stopped optimization and requested gameplay cheats. The renderer
in63 matches54, with no58..62 production changes retained.

## Debris comparisons and progression build (2026-09-14)

The source-preserving scalar comparison change removes four `memcmp` callsites:
particle pending bits, sleeping debris support lookup and two comparisons in
`Body::step`. Four scalar XOR/OR operations compare an edge, and an OR tests an
empty edge. Final MIPS disassembly confirms all four calls are absent. Existing
physics/differential tests pass; no counts, lifetimes, collision order, parallax
or textures changed. The separate lifetime layout change saves 512 stack bytes.

| Same saved user tape | Mean VBlanks | 2 / 3 / 4 / 5 / 6 frames | Maximum |
| --- | ---: | --- | ---: |
| Rescanned `6517d70` |2.568|1,021 / 1,202 / 30 / 7 / 0|5|
| Door + storage + stack build |2.560|1,039 / 1,192 / 32 / 3 / 1|6|
| Final, with scalar comparisons |2.552|1,052 / 1,191 / 28 / 3 / 0|5|

Reports respectively: `.hkpsx/rescan-2026-09-14-profile/`,
`.hkpsx/progression-final-profile/` (intermediate despite its name), and
`.hkpsx/progression-scalar-profile/` (historical gameplay baseline). All reach the same endpoint,
region 8 at `(163.153656,6.390625)`, health 4. The final 2,274 intervals cover
5,804 route ticks; earlier runs cover 5,803. Frame phase and animation cache
choices shift with pacing, so do not interpret this as an identical-frame GPU
benchmark. Final p95 is three; the strict four-tick gate still fails on three
five-tick intervals. There are no gameplay CD reads, static VRAM uploads, boundary
waits or recorded runtime faults. Animation transfers from resident RAM remain.

The intermediate six-tick frame crossed region 1 -> 2 with about 80 active
particles. Its 120-tick PC window contains 1,482 scenery samples, 1,426 particle/
debris samples, 207 `memcmp` samples and only 22 renderer-init samples. The
window is too broad to attribute an exact frame to one function. Global callsite
samples attributed 608 `memcmp` samples to the targeted debris paths. The
frame's GPU estimate increased only 3.6% against the old activation; collision
work and changed phase overlapped the crossing, without loading. The final
scalar build no longer has a six-tick frame on this tape.

Tradeoff: inlining grows text by 688 bytes. Crossing a 2,048-byte linker alignment
boundary grows static span by 2,048 bytes overall, leaving 13,536 bytes before
the protected stack. BSS is unchanged. The modest measured pacing gain is
retained, but RAM, stack high-water and residual burst cost still need work.
These are emulator observations; original-console timing remains unmeasured.

## Whole-draw culling and rotated-quad occlusion (2026-09-11)

Three experiments on the 2026-09-09 build, each judged by byte comparison of
final frames against that build's runs of the same tapes (`.hkpsx/occl2-*`
versus `.hkpsx/final-*`).

1. **Gouraud fog sheets, rejected before a build.** The `fog` sheet texture is
   a 48x20 blob with five palette colors and 442 of 960 texels transparent,
   drawn additively over 16,000 to 48,000 pixels per instance. The best
   bilinear fit, which is all a four-vertex shaded quad can produce, differs
   on 919 of 960 texels (mean error 3.0 of 31 per channel, maximum 10) and
   would add light over the 442 transparent texels. Not exact, dropped.
2. **Whole-draw culling.** A draw whose on-screen bounding box lies entirely
   under the union of the later opaque black occluder rectangles (the same
   eight largest front-layer solid textures and cores as before) is skipped
   before any packet is written. No clipping is involved, so rotated quads
   and oversized repair quads qualify. `HK_HIDDEN_QUADS` and
   `HK_HIDDEN_PIXELS` count it.
3. **Occlusion subtraction on rotated quads.** The earlier statement that the
   emulator does not rasterise a clipped rotated quad identically is
   withdrawn. PSoXide clips each scanline span to the drawing area and
   evaluates texel coordinates from per-pixel plane equations
   (`for_each_tri_pixel`, `tri_plane_eval` in `gpu/raster.rs`), so the E3/E4
   area cannot change what the surviving pixels sample. The pieces of a
   rotated draw start from its screen bounding box and keep the original
   vertices; `HK_ROTATED_OCCLUSION_QUADS` counts them. Flat cores stay
   axis-aligned only, because the core rectangle itself is a new quad.

Exactness: the crash tape's region 44 frame is byte-identical after (2); after
(3) the only differing pixels lie inside Knight or HUD packet boxes of either
frame (223 at the spawn camera, 2 on the crash tape, 0 on the ascent), which
is the known pacing effect on the Knight's animation and HUD state, not
scenery. A packet census must read a completed list: a RAM dump taken while
the front pass is still being built undercounts foreground packets, which
produced two misleadingly low readings during this work.

| Long 5,810-poll profile | Mean VBlanks | 2 / 3 / 4 / 5 / 6 frames | Max |
| --- | ---: | --- | ---: |
| Kicks + cores + occlusion (2026-09-09) |2.807|444 / 1,581 / 42 / 1 / 0|5|
| + whole-draw culling |2.701|728 / 1,345 / 68 / 7 / 1|6|
| + rotated occlusion (`.hkpsx/final-profile/`) |2.583|989 / 1,216 / 33 / 8 / 1|6|
| + culling CPU trim (`.hkpsx/trim-profile/`) |2.568|1,021 / 1,202 / 30 / 7 / 0|5|

Exact raster of the completed final frames: spawn camera 29.4 to 22.1 ms,
region 44 beside the cocoon 41.8 to 32.8 ms, region 2 on the ascent 28.8 to
24.2 ms. Over the long route the guest culled 23,857 draws (55.0 M pixels)
and subtracted occluders from 37,340 draws, 30,587 of them rotated (141.7 M
pixels). The strict four-tick seamless gate still fails, as it did for the
reference build; the 5 and 6 VBlank frames sit at the region 1 to 2
activation and inside destruction bursts.

CPU: `subtract` rose from 0.15% to 3.2% of retired PC samples and `scenery`
from 11.9% to 17.3%. The culling check now filters occluders by overlap
first, answers immediately when one rectangle contains the box, and only
runs the subtract chain when at least two overlap; `subtract` fell to 1.2%
and the long profile's mean to 2.568 VBlanks. Frames remain GPU-bound.

Semi-black cores were tried and removed. The cooker found the largest
all-0x8000 texel rectangle for 27 scene-0 textures, and the guest drew it as a
flat semi-transparent black quad (E1 for the draw's blend state, E6 for the
mask bit) on axis-aligned averaging draws. The measured upper bound was
0.35 ms at spawn and under 0.11 ms elsewhere, and in practice the core lost
to the opaque core's four-piece budget: 257 packets over the whole crash
tape, none in any final frame, so the code was not kept.

Build note: FMOD's default output needs a usable host audio device and
failed with OUTPUT DRIVERCALL in the Geo audio cook. The Rust decode
(`host/hk-cook/src/fmod.rs`) selects the NOSOUND output; the cooked Geo bank
hash is unchanged.

### Parallax layer breakdown

There are no discrete layers: every source sprite keeps its own depth and the
cooker derives its scroll ratio from it (camera at z -38.1, ratio
38.1 / (z + 38.1)). The 116 regions hold 2,977 unique draws at 312 distinct
depths; a frame on the long route shows 52 to 103 distinct depths (mean 79).
Exact raster of the 2026-09-09 build's completed final frames by band:

| Scroll ratio band | Unique draws | Spawn (29.7 ms) | Region 44 (41.8 ms) | Region 2 (28.8 ms) |
| --- | ---: | ---: | ---: | ---: |
| Foreground, above 1.0 |2,007|9.2|8.1|12.0|
| Play plane, 1.0 |94|0.2|0.1|0.1|
| Near background, 0.9 to 1.0 |512|4.2|10.4|6.3|
| Background, 0.75 to 0.9 |252|6.1|8.6|3.9|
| Background, 0.5 to 0.75 |70|4.2|7.5|4.3|
| Far, 0.3 to 0.5 |35|3.4|4.3|1.0|
| Very far, below 0.3 |7|1.7|1.8|0.3|

The expensive items are a few soft sheets stretched from tiny textures across
most of the screen and rotated: `fog` (48x20) 6.6 ms at spawn and 6.4 ms in
region 44, `black_fader` (48x34) 6.8 ms in region 44, `fall_BG` 5.1 ms and
`haze` 2.6 ms on the ascent. GPU cost is per screen pixel, so texture
resolution is irrelevant and half-resolution far layers would only pay after
a full-screen upscaling blit of about 6 ms. Simplifying the layering was
rejected; the measured exact levers were the two above.

## Frame pacing and the GPU raster budget (2026-09-09)

Every number below is PSoXide's emulated bus clock (33.87 MHz), not hardware.
The emulator charges silicon-calibrated raster time per pixel: textured
2.80 bus cycles, flat opaque 0.54, flat semi-transparent 0.80, fill 0.08.
Semi-transparency does not change textured cost. `tools/frame_packets.py`
parses the last built frame from a RAM dump and applies that model per packet;
`hk-cook gpu-census` (host/hk-cook/src/gpu_census.rs) ranks static draws along a replay's cameras.

Anatomy before this work (stationary spawn camera, per frame): the CPU built
the whole display list in about 40% of a VBlank, then submitted one DMA list,
then spun while the GPU rasterised about 30 ms. A frame therefore took
`ceil((build + raster) / 16.7 ms)` VBlanks, and 30 ms of raster plus any setup
overshoots the second edge at 33.4 ms. The steady state was 3 VBlanks.

Changes, all pixel-exact (stationary final frames byte-identical across builds):

- Packets are kicked progressively as their own DMA lists every eight draws
  whenever the channel is free, so the GPU starts 1.8 ms after the flip
  (`HK_FRAME_FIRST_KICK_LINES`, Timer1 HBlanks) instead of after the whole
  build. The final list waits in the present loop for a free channel so
  simulation ticks and input checkpoints run instead of a spin.
- Black-mask textures carry a cooked largest-opaque-black rectangle
  (`SCENE_BLACK_CORES` in `data/scene_manifest.rs`). On the black-average path
  at full opacity that core is drawn as a flat opaque black quad and the
  textured border as scissor pieces; the same DDA as the alpha covers maps the
  texel rectangle to pixels. The sentinel texel 0x0001 keeps its red bit under
  ordinary tint modulation, so ordinary sprites never take the flat path.
- The eight largest front-layer opaque black rectangles (solid textures and
  cores) are subtracted from earlier axis-aligned draws, one rectangle per draw,
  through the proven E3/E4 scissor packets. Rotated quads were not clipped in
  this build; the 2026-09-11 section above withdraws the claim that the
  emulator rasterises them differently when clipped.

| Long 5,871-poll profile | Mean VBlanks | 2 / 3 / 4 / 5 frames | Max |
| --- | ---: | --- | ---: |
| Coin fix (single list) |3.262|0 / 1,326 / 440 / 13|5|
| Two lists |2.975|170 / 1,659 / 122 / 0|4|
| Progressive kicks |2.865|330 / 1,640 / 56 / 0|4|
| Kicks + cores + occlusion |2.807|444 / 1,581 / 42 / 1|5|

Stationary spawn cadence: 3.00 before, 2.61 now (54 two-VBlank, 85 three).
Frames land on two VBlanks when raster is under about 31 ms. Raster per frame
on the long route is p10 25.5 ms, p25 29.9, p50 33.1, p75 35.6, p90 40.2, so
about a third of frames fit today and the median is 2 ms over the edge.

Remaining exact headroom is small. Measured on three real frames: 42 to 57% of
raster is rotated quads that no rectangle clipping can touch; texels that draw
nothing are 2 to 11%; pixels under later opaque black 7 to 22% at full
resolution, of which single-rectangle subtraction recovers roughly half; flat
cores 2 to 5%. Spawn-camera raster is 28.7 ms with `haze3` alone 6.2 ms
(74,595 px) and `fall_BG` layers 27%. A consistent 30 fps needs the median
frame under about 28 ms, which is 15 to 25% less raster than exact rendering
of the source layering produces. Options that need a decision: draw far
parallax layers at half resolution and upscale, drop or merge the cheapest
distant layers, or accept a mixed 20/30 fps cadence.

Pacing changes move region activations to different ticks, and enemy sync at
activation can change an encounter: the Lifeblood ascent route now ends with
health 3 and wallet 0 instead of 4 and 2 at the same final position. This is
the documented poll-bound-route caveat, not a rendering fault, but it means
the simulation is not independent of frame timing.

Evidence: `.hkpsx/occl2-profile/`, `.hkpsx/pace-anatomy2/`,
`.hkpsx/kick-profile/`, `.hkpsx/split-profile/`, `.hkpsx/frame-anatomy/`,
`.hkpsx/occl2-{ascent,geo-pause,crash}/`.

## Renderer experiment: not promoted

The current source adds20-byte static alpha covers (255,240 bytes across98
regions), DMA-scoped E3/E4 scissors and bounded oversized-quad subdivision.
The alpha path passes728 GPU comparisons exactly; geometry matches its prior
integer-grid reference at100 poses/origins, but retains documented raster
approximation versus the unbounded oracle. Eleven reference rooms retain every
original texture, palette, draw and animation byte. These are correctness
checks, not proof of useful game performance.

Initial stationary render CPU grew71,359→325,191 cycles/frame while textured
GPU work fell1,062,280→1,005,974. Exact inverse clipping reduced that CPU cost
to257,912. Further changes narrow geometry arithmetic, swap row references,
outline large helpers and reuse bounded repair scratch. The latest complete
return telemetry run averages352,591 render cycles, maximum519,090, and still
fails:4 boundary ticks, max input gap2. Normal return has5 boundary ticks.
Other routes can exceed4 frame ticks. No continuity assertion was weakened.

Chunk14 read/decode/upload in the earlier fast candidate explains a hold:
read starts337, decode370–424, VRAM ready432; the boundary is reached426.
The reader baseline decodes368–408 and is VRAM-ready417 before crossing425.
Upload size is unchanged173,056 bytes. Most delay is reduced background CPU
time, not the extra4,100 raw bytes. Preserve the existing downward preload
policy until a change has full route evidence.

Evidence: `.hkpsx/render-suite`, `render-fast-suite`, `render-outline-suite`,
`render-scratch-suite`, their telemetry return runs and snapshot build reports.
Main discs are restored to the exact validated reader hashes below. Candidate
builds are isolated and promotion requires both variants' strict replays and
actual-CUE Focus. The pure input FIFO and FNV shift/add experiment are not
integrated; see NEXT_PROMPT.md for pending work.


The September 8, 2026 user recording reproduces two separate bottlenecks:
GPU submission blocks simulation during ordinary frames, and synchronous room
prefetching repeatedly pauses gameplay for several seconds. The measurements
below are from PSoXide's emulated CPU/bus clock, not original PS1 hardware.

## Current streaming milestone

The normal EXE is
`32aaf06131ee6f6d99f9ec958f269ac51470ba2564034da7e7337d19ea13b5a1`;
BIN is `81ae3a6f34ba9d4cb21ed2df55c1de851082d1d4e7648dc64b51525f847d2092`.
Both normal and telemetry builds pass the same six-route strict suite:

| Route | Samples | Boundary holds | Maximum input gap | Maximum rendered interval |
| --- | ---: | ---: | ---: | ---: |
| Latest user recording | 908 | 0 | 1 VBlank | 4 route ticks |
| Original user recording | 1,585 | 0 | 1 VBlank | 4 route ticks |
| Forward and return | 1,024 | 0 | 1 VBlank | 4 route ticks |
| Repeated boundary jumps | 1,280 | 0 | 1 VBlank | 4 route ticks |
| Original longer traversal | 2,400 | 0 | 1 VBlank | 4 route ticks |
| Earned-SOUL healing route | 2,400 | 0 | 1 VBlank | 4 route ticks |

No load errors, guest faults or discarded sectors were observed. Activation
frames do not exceed ordinary frame maxima. Normal latest-recording rendering
averages3.098 route ticks, about19fps; input and simulation continue at60Hz.
This is not a claim of60fps rendering or physical-console timing. The workload
and positions vary after pauses are removed, so average frame costs are not a
controlled same-camera renderer comparison.

Exact EXE/map/CUE/BIN/tape/emulator hashes, commands, counters and captures:
`.hkpsx/reader-suite/` and `.hkpsx/reader-telemetry-suite/`.
`tools/validate_streaming.py` reproduces the three authored fixtures byte for
byte, plus optional preserved recordings. It fails on any boundary hold,
missed input poll, error or frame-budget regression. Use a fresh output directory:

```sh
python3 tools/validate_streaming.py --output .hkpsx/stream-check \
  --recording .hkpsx/user-new-stops-input/latest.pxtape \
  --recording .hkpsx/seamless-baseline/recording/latest.pxtape \
  --recording captures/focus-reader/input.pxtape --jobs 2
```

The earlier Focus/HUD build had56 held ticks across14→15,16→18 and20→7 on the
same longer input. Overlapping read/decode first removed the serialization,
but changed where later inputs landed and exposed7→8. The final changes are:

- Five256KiB RAM arenas retain an extra forward second-hop target. Three direct
  neighbours remain the GPU working set; no scenery quality reduction was added.
- CD transport and decoding own separate private leases. Neither can overwrite
  the current room, another job or an unfinished GPU upload's RAM source.
- Brief region changes preserve in-flight reads. A completed read no longer in
  the current RAM wishlist or urgent demand retires before decoding. This lets
  the needed next room use its arena without cancelling a live transfer.
- Stored/raw FNV checks remain. LZ4 matches use dependency-safe word copies and
  metadata validation yields after at most eight records, removing an observed
  input-poll gap around admission. All98 chunks match the pinned SDK decoder.
- A small polling wrapper avoids the decoder's large save/restore frame while
  waiting for transport. Read-start errors still allow an active decoder to run
  one bounded unit before reporting the scheduling failure.

Intermediate failing experiments are preserved under `.hkpsx/pipeline-*` and
`.hkpsx/preload-five-suite`. Do not treat their regressions or artifacts as final.
The final arena allocation is1,310,720 bytes, with257,796 linked bytes remaining
before the reserved32KiB main stack. See BUDGET.md for reservations and limits.

The original `ENEMY_ROUTE` and `FOCUS_ROUTE` remain unchanged for streaming
comparisons. Removing holds lets that input walk past the first Crawler before
its stationary attack sequence, so separate `EARNED_COMBAT_ROUTE` and
`EARNED_FOCUS_ROUTE` stop in the actual encounter windows. Gameplay validation
still requires exact source SOUL rewards and a complete33-SOUL heal; it does
not inject state or weaken assertions.

A later host reference uses up to four disjoint alpha-cover scissors per
texture, retaining original vertices/UVs/material/order. At50 recorded poses,
both framebuffer origins and visibility/tint variants,600 full-VRAM comparisons
show zero differences. Mean estimated scenery sample-area saving is11.80%, with
about73 extra quads and919 GP0 words per case. No guest implementation or runtime
speedup has been measured. See `.hkpsx/alpha-scissor-reference/README.md` for
input hashes, CPU renderer method and bounds.

Oversized fog/mask quads currently exceed GPU edge limits and are rejected.
This is a rendering defect requiring clipping/subdivision; the scissor estimate
excludes those rejected triangles. Restoring the layers may increase GPU work.
Earlier sections below retain measurements tied to their historical artifacts.


The ambience integration exposed two frame spikes. Collision segment checks now
reuse cross products and reject disjoint axis bounds before wide arithmetic;
540,709 exhaustive/deterministic/reference comparisons preserve hit semantics.
Decoder slices yield after1024 bytes. Ready destinations activate before an
unrelated speculative VRAM transfer; unfinished upload sources stay pinned.
Moving the decoder after simulation caused missed polls and was reverted.
Failing intermediate artifacts remain under `.hkpsx/ambience-slice-*`,
`.hkpsx/ambience-combat-*` and `.hkpsx/ambience-priority-*`; that milestone's evidence is
`ambience-ready-*`; the current reader build uses `reader-*`. No input or frame-budget assertion was relaxed.

The safe scalar reader now checks a bounded field slice and uses unaligned word
loads, retaining little-endian decoding and rejection of truncated fields.
Tests cover262,144 halfword cases,1.6 million word/signed alignment cases and
3,128 invalid-boundary cases. In the last200 complete stationary telemetry
frames, simulation CPU cost fell from99,025 to55,148 cycles per tick (44.3%),
with identical final framebuffer hash, position and active GPU workload.
Rendering CPU cost fell1.6%; the GPU remains the main presentation limit.
This is stationary CPU headroom, not a whole-game speedup or hardware claim.
Exact input hashes/commands/method: `.hkpsx/reader-idle-comparison.json`.
The baseline's strict traversal check correctly rejected the stationary tape
because it never changes regions; the separate six-route suites establish the
reported traversal coverage. The reader adds2KiB to the linked reservation.

The separate oversized-layer reference now covers50 static source-art poses,
32 synthetic cases and191 individual coverage comparisons. Integer-UV grid
subdivision reduces aggregate disagreement with an explicitly unbounded CPU
raster oracle from455,364 pixels to3,238; maximum scene packets rise157→161.
This is an approximate repair, not Unity or PS1 pixel parity. High-frequency
synthetic textures show substantially greater differences. Restored layers add
about57,933 sampling positions per pose (12.75% aggregate estimate), so their
cost must be measured alongside alpha scissors. The reference uses framebuffer
origin0 and all-visible static state. Door-controlled masks can conceal the
scene; actual break/fade state and both framebuffer origins must be validated
before guest integration. Images and reproduction:
`.hkpsx/oversize-reference/README.md`. No production geometry change is included.

## Preserved baseline

The ignored `.hkpsx/user-slowdowns-baseline/` directory contains the unchanged
EXE, matching link map, source snapshot, build/provenance reports, original input
tape/profile, emulator command/help/hash, replay logs, RAM, and inspected gameplay
captures. Disc files remain exclusively in the user's PS1 library.

- Normal EXE SHA-256: `dcfdd59e193aa932e0844623bb1a487936a6a22cd6f15e3b0ea4acd4a5b01725`.
- Disc BIN SHA-256: `ceb2a62e486a764a8b63f52c296d2202d85e76a2ab0fc7c36ff65c507a78e40d`.
- Emulator SHA-256: `6f8b8cea2466a1df040912f65240882a5992c7df82837be4050e11ab35def8c6`.

The original `PXITAPE2` contains 1,585 six-byte samples starting at pad poll 74.
Current emulator code applies neutral input before that offset and one recorded
sample per completed pad poll. The recording includes START at poll 251, so this
run can be replayed from boot without reconstructing controls or a save state.
The run stopped at poll 1,659. The final captured RAM reported region 6,
position `(113.75, 9.390625)`, five masks, no room-load error, and no guest fault.

The user's aggregate profile spans the recording operation, as verified in
current frontend capture code. It contains 404 guest render-stage hits averaging
1,473,676 cycles, 1,385 update hits averaging 61,742 cycles, and 3.428 simulation
ticks per visual frame. Host hardware rendering averaged 0.453 ms. **That
recording contains guest telemetry, while the preserved normal EXE does not.**
The original aggregate therefore cannot be bound to this exact EXE; the separate
normal replay below uses emulator-owned diagnostics instead.

## Normal replay results

| Measurement | Baseline |
| --- | ---: |
| No-CD frame intervals | 386 |
| Mean no-CD interval | 3.453 route ticks |
| No-CD interval distribution | 213 × 3, 171 × 4, 2 × 5 ticks |
| Frame intervals containing CD loads | 16 |
| Total ticks in those CD intervals | 2,911 |
| Shortest / longest CD interval | 145 / 381 ticks |
| Completed room payload loads | 19 |
| CD sectors read, including header | 1,716 |
| Gameplay display flips | 403 |

A route tick in this run is approximately 571,236 bus cycles. At the PS1 CPU
clock, the no-CD mean is approximately 58 ms, or 17.2 visual frames per second.
The 16 CD intervals total approximately 49.1 emulated seconds; individual
intervals last approximately 2.45–6.43 seconds. These intervals include adjacent
frame work, so they are not pure drive-wait measurements.

The observed region sequence was `1 → 2 → 13 → 14 → 15 → 16 → 15 → 16 → 6`.
At polls 1235–1239, a single interval loaded two payloads totaling 201 sectors.
After arriving in region 6, inputs near polls 1374, 1426, and 1568 each triggered
a 95-sector neighbour load followed by a 77-sector neighbour reload, despite
remaining in region 6. CD Setloc commands resolve against the packed manifest to
regions 16 and 7 respectively. This is speculative cache churn, not mandatory
room transitions.

Retired-instruction sampling attributed 28.84% to
`psx_gpu::draw_sprite_material`, 27.47% to `disc::Cache::advance`, 20.60% to
`SectorReader::read_sector`, and 3.69% to its seek helper. Scenery projection was
only 1.00%. These percentages count instructions, not weighted execution time.
Exact MMIO attribution charged 201.46 million stall cycles to the sprite helper
and 608.50 million to CD read/seek helpers. In this build, the HUD's immediate
sprite writes follow a submitted scenery DMA chain and wait on GPU readiness.

The GPU command census attributed 553.12 million estimated bus cycles to
textured quads during gameplay, about 1.37 million per displayed frame. The
current GPU model charges clipped polygon area at 179/64 bus cycles per pixel,
including transparent texels. This corresponds to roughly 6.4 screen areas per
frame; it is an estimate of submitted textured coverage, not measured opaque
overdraw. Preserving layer appearance remains a requirement for optimizations.

## Reproduction and comparison

The portable runner accepts any matching EXE/map and records the hashes of all
inputs, including the mounted BIN/CUE. It does not create or modify disc images.
Use a fresh output directory for each run:

```sh
python3 tools/profile_tape.py \
  --exe .hkpsx/user-slowdowns-baseline/artifacts/hk-psx.exe \
  --map .hkpsx/user-slowdowns-baseline/artifacts/hk-psx.map \
  --cue "$HOME/Downloads/ps1 games/hk-psx.cue" \
  --tape .hkpsx/user-slowdowns-baseline/recording/latest.pxtape \
  --output .hkpsx/user-slowdowns-comparison
```

Only use that CUE with the baseline EXE while its packed assets remain compatible.
The original run's exact command and hashes are in `replay.json`. The runner
writes route clocks and RAM watches, CPU cycle categories, PC/callsite/window
samples, MMIO and RAM-load attribution, GPU command costs, CD commands, final
CPU display/RAM, and periodic CPU screenshots. GPU command census drains the
capture log, so this runner intentionally does not claim a simultaneous hardware
renderer command replay capture.

`analysis.json` separates frame intervals with and without observed CD reads,
records final player state and region transitions, and includes attribution
limitations. A change to render pacing, collision coverage, or loading policy
can change where the same poll inputs take the player. Compare positions and
transitions alongside frame intervals; identical input files do not establish
identical visual workloads.

## Fix priorities

1. Put HUD/pause primitives into the immutable frame DMA list and submit it
   asynchronously. Continue bounded simulation on VBlank while GPU work drains;
   preserve DMA completion and framebuffer ownership checks before reusing
   packets or changing display state.
2. Prevent speculative neighbour replacement loops. Blocking whole-room
   prefetching must not be described as asynchronous streaming. A later bounded
   background service must explicitly manage CD ownership, sector arrival,
   decompression/checksum work, and the two-slot memory limit.
3. Re-measure before optimizing scenery projection. Remaining GPU area cost
   may warrant transparent-border trimming or an exact 1:1 sprite fast path,
   but both require pixel comparisons and source-art provenance; UV rounding,
   transformed geometry, and layer order must remain correct.

No performance improvement is claimed here until the changed final artifact has
been replayed and its captures inspected.

## First asynchronous streaming comparison

The first normal build with asynchronous frame submission, IRQ-driven CD input,
and deduplicated region chunks was replayed with the same recorded tape.
Evidence is in `.hkpsx/user-slowdowns-after/`; its input hashes and resolved PC attribution are
recorded there. This intermediate EXE/map were replaced by the next build before
a separate snapshot was made; later runner versions snapshot them at launch.
EXE SHA-256:
`4e834422cb58b8685dfcba54956b9923b072706ebb018b075f575a0c9e35cc9c`.
This is an intermediate measured artifact, before the subsequent aligned-word
VRAM upload optimization.

| Measurement | Baseline | First async build |
| --- | ---: | ---: |
| Mean interval without observed CD activity | 3.453 ticks | 3.062 ticks |
| Maximum displayed-frame interval | 381 ticks | 23 ticks |
| Intervals longer than six ticks | 16 | 4 |
| Maximum observed pad-poll gap, including activation | 380 ticks | 20 ticks |
| Room payload loads | 19 | 6 |
| CD sectors read | 1,716 | 569 |
| Guest faults / load errors | 0 / 0 | 0 / 0 |

Forty-seven frame intervals contained CD activity; 43 still lasted only three or
four ticks. CD activity therefore no longer identifies a freeze. The remaining
four long frame intervals lasted 20–23 ticks and coincided with room activation.
Boundary waits themselves continued drawing every three ticks. The guest
recorded 41 total boundary-wait ticks and a maximum of 39 for one wait, no
stream errors, no discarded sectors, and an IRQ maximum of 5,281 Timer 2 ticks.

The instrumented gameplay pad-gap counter was one VBlank, but it excludes the
room activation interval: raw emulator poll observations found four activation
gaps of 18–20 route ticks. The report records both, so the gameplay counter must
not be presented as uninterrupted input handling throughout transitions.

The final state was five masks at `(101.92299, 11.390625)` in region 15. The
baseline ended farther along in region 6; pacing and loading changes alter the
path of identical poll inputs. The CD-byte reduction is consequently a result
for these observed runs, not a claim of identical-workload throughput.

Software and hardware final captures were inspected. A second capture run with
GPU command replay produced the exact same software display pixels as the
instrumented run. Both renderers show the Knight and scenery correctly. The
large foreground covering the Knight near x84 was also visible at baseline
route tick 1800, x84.015; the matching intermediate capture at x84.029 has the
same obstruction. This is a pre-existing scene/layer behavior issue, not evidence
of an asynchronous-render regression, and still needs source-driven resolution.

## Aligned SDK word uploads

The following build replaced byte-at-a-time static VRAM uploads with the SDK's
aligned word upload path. Its EXE/map were snapshotted at replay launch in
`.hkpsx/user-slowdowns-words/artifacts/`. EXE SHA-256:
`9a67c5af20c99f36c7b394722ae0a9fb907bdc42a920930dcee70f07b7830122`.
The tape completed at poll 1,659 with the **same final position, region, health,
room-load count, and sector count** as the first async comparison above.

| Measurement | First async build | Aligned uploads |
| --- | ---: | ---: |
| Maximum displayed-frame interval | 23 ticks | 7 ticks |
| Mean of all displayed-frame intervals | 3.236 ticks | 3.095 ticks |
| Maximum observed pad-poll gap, including activation | 20 ticks | 5 ticks |
| Room payload loads / sectors read | 6 / 569 | 6 / 569 |
| Faults / load errors / stream errors / discarded sectors | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |

All four activation intervals now lasted seven route ticks, approximately
118 ms. Of 455 measured frame intervals, 422 lasted three ticks, 28 lasted four,
one lasted two, and four lasted seven. Background CD activity occurred in 68
intervals and generally overlapped these ordinary frames. One activation
interval had no CD counter change, demonstrating why reports must include the
maximum of **all** frame intervals rather than classify freezes from CD activity.

Total boundary waiting increased from 41 to 53 ticks (maximum individual wait
51): less time spent uploading VRAM can expose time still needed by the
prefetched room. The boundary wait continued drawing. The guest gameplay
pad-gap counter remained one; the independent raw poll-gap maximum was five
route ticks at activation. These are different measurement scopes.

The lower activation cost is a measured improvement on matching routes. Rendering
still generally takes three VBlanks. This result does not establish full-game
performance or original-hardware compatibility.

## Previous candidate verification — before multi-region VRAM

The final candidate adds the bounded decoder optimization, keeps authored door
mask state across cooked regions, and measures pad gaps across activation too.
Its frozen EXE/map and reports are in `.hkpsx/user-slowdowns-final/`.
EXE SHA-256:
`4ea23140719eb76858f31858ec4f219c6bfca2b83bb4ae73231edee4f6843f60`.
Disc BIN SHA-256:
`4515d8d36fc4588f396657b7948e372a167fb1786ee448823000477b090b4a67`.

The original recording again completed at poll 1,659, with five masks at
`(101.92299, 11.390625)` in region 15, six loaded room payloads and 569 sectors.
There were no runtime faults, room-load errors, CD stream errors, or discarded
sectors. Across 455 gameplay frame intervals, 424 lasted three route ticks,
27 lasted four, and four activation intervals lasted seven. The overall mean
was 3.095 ticks, maximum seven; both the independent poll observations and the
corrected guest counter measured a maximum five-tick input gap at activation.

Boundary waiting fell from 53 total ticks / 51 maximum to 36 total / 33 maximum
with the decoder optimization. Waiting continued to render. These are observed
boundary counters, not frame hitches or IRQ duration measurements. The final
maximum CD IRQ duration was 5,281 raw Timer 2 ticks.

The software and hardware final displays were both inspected. The software
pixels from the instrumented replay and the separate hardware-capture replay
were identical. Periodic post-door captures show the formerly blocking mask
cleared, while preserving the other foreground layers.

The combat fixture needed new poll timings because reduced loading waits change
when its jumps reach a ledge. On this exact final build, hold RIGHT from poll 54
through 859, jump at polls 285, 410, 540, 655, and 745 for 24 polls, and retain the
recorded 24-poll early / 26-poll combat nail cadence in `ENEMY_ROUTE`. At the
1,200-poll endpoint, this route produced two enemy hits, one kill, 22 SOUL, five
masks, and no deaths, hazard respawns, or runtime faults. The first Crawler's
remaining HP was −2. This verifies the source-derived two-hit lifecycle and
reward on the final artifact; it does not weaken the existing validator checks.


## Four resident regions and lower-resolution scenery

The next build keeps the existing98 region boundaries and all cooked geometry,
ordering, masks, gameplay data and animation sampling. Scenery is resampled from
the original Windows sprites with a48-pixel longest-axis cap. Exact within-region
texture deduplication still runs. This reduces total stored region data from
13,728,308 to6,291,710bytes and allows four5-page/384-CLUT GPU banks alongside
four384KiB RAM arenas. Shared animation slots retain their previous resolution.

The planner queues the forward exit first, excludes corner-only neighbors, and
keeps nearby return/below routes resident. GPU transfers prepare inactive banks
in64KiB slices after frame retirement, using pinned RAM views. Only fully ready
RAM/GPU pairs may activate. Simulation keeps its VBlank clock across ready
switches; incomplete targets still increment honest boundary-wait counters.

Two earlier candidate checks explain the final policy. The first three-bank
forward/two-ahead policy still waited on reversals and falls. Four banks with a
pure geometric ranking fixed the original recording but failed a faster
forward/jump route: corner neighbors displaced the actual forward target.
The final policy prioritizes that target before speculative corner/vertical work.
These failed candidates are retained under `.hkpsx/seamless-*-v1/v2/v3`.

The normal final-candidate EXE is270,336bytes, SHA-256
`d99878d7c71d3f29e3a1c36a84f12e79c409af8a3c314bba5de87d212976337c`.
The disc BIN is9,765,504bytes, SHA-256
`bf170258aac9dc52fdec75114649b7868133d1c5774a63a698e0616999823cda`.
Its frozen replay EXE/map and input hashes are in `.hkpsx/seamless-user-v4/`.

| Original recorded input | Previous verified build | Four-bank candidate |
| --- | ---: | ---: |
| Total boundary-wait ticks | 36 | 0 |
| Largest boundary wait | 33 | 0 |
| Maximum per-poll VBlank gap | 5 | 1 |
| Longest displayed-frame interval | 7 | 4 |
| Mean displayed-frame interval | 3.095 | 3.152 |
| CD sectors / room loads | 569 / 6 | 600 / 16 |
| Faults / stream errors / discarded sectors | 0 / 0 / 0 | 0 / 0 / 0 |

The final candidate crosses1→2→13→14→15→16→15→16→6 and finishes at
(113.75,9.390625), with5 masks. Of441 gameplay frame intervals,368 last3 route
ticks,70 last4 and3 last2; activation intervals never exceed ordinary ones.
This is approximately19 rendered frames per second with60Hz simulation/input,
not60fps rendering. The earlier path ended atx101.923, so these are comparisons
of the same recorded inputs rather than fixed-position rendering benchmarks.

Independent route sampling watches the SDK VBlank count as well as completed
pad polls. A two-route-tick observation can span two actual VBlanks/polls, or
sample the current VBlank just before its poll. Therefore the gate requires a
maximum guest per-poll gap of1, independent VBlank-minus-poll variation≤1, and
no raw gap beyond the2-tick sampling bound. It does not mistake sampling phase
for a missed update. The final replay meets all three conditions. Synthetic
tests still reject skipped VBlanks, boundary holds, activation spikes and errors.

The separate `.hkpsx/seamless-user-v4-hardware/` run was inspected in both
renderers. Its software pixels exactly match the profiled run; the hardware
viewport displays the Knight, source layers and HUD correctly. Physical-console
CD/GPU timing and arbitrary future routes remain unverified. Additional route
coverage and the full final-disc smoke test are recorded in STATUS.md.


Additional final-artifact routes also pass the same strict continuity gate:

| Route | Observed region traversal | Boundary waits | Maximum pad gap / frame interval |
| --- | --- | ---: | ---: |
| Forward and return | 1→2→13→14→13→2→1 | 0 | 1 / 4 |
| Repeated movement/jumping | 1→2→13→14 | 0 | 1 / 4 |
| Twelve alternating jump crossings | 1→2→13→14, then12 crossings between13 and14 | 0 | 1 / 4 |

These reports are `.hkpsx/seamless-return-v4/`, `seamless-jumping-v4/` and
`seamless-boundary-jumps-v4/`; exact tapes are under `.hkpsx/seamless-routes/`.
The jump-only fixture's initial forward transitions are not misrepresented as
repeated crossings: the separate boundary-jumps tape establishes those. Every
route finishes with5 masks, zero faults/errors/discards and independent
VBlank/poll lag variation≤1. Return and boundary-jump software/hardware captures
were inspected; their software pixels exactly match their profiled counterparts.
Hardware and software rendering still differ in brightness/blending, especially
in the bright opening backdrop; cross-renderer pixel equality is not claimed.

The combat fixture was recalibrated without weakening source-outcome assertions.
RIGHT now ends at poll780 (duration726 from54), and the obsolete jump745 is
removed. The four earlier jumps and attack cadence remain. The exact1200-poll
fixture produces2 hits,1 kill,22 SOUL and5 masks, ending in region18 with no
faults/deaths/hazard respawns. This avoids scheduling a late jump based on loading
pauses that no longer occur. The full validator retains the two-hit reward and
health checks.

## Adjacent opaque-fill certificate audit (September 15)

Joint certificates can cover seams between thin immutable FRONT tilemap fills
that separate19×19 erosion cannot certify. An ideal single-camera union initially
suggested1.224ms of additional region18 raster savings beyond existing cuts.
After exhaustive relative phases and the existing8-piece/1,024-pixel threshold,
pairs retain only0.288ms (+4packets), and groups of3–4 retain0.557ms (+4).
Region15 retains0.171ms with groups. These estimates precede added guest CPU
work, so this version was not integrated. It does not justify the ideal1.224ms
claim for a production implementation. Native checks covered53certificates,
819joint phases and299,184,165 certified pixels. Evidence remains ignored at
`.hkpsx/pass36-certificate-gap-audit/`.

Any future group approach needs per-region contributor membership, common
parallax, complete immutable/live-state checks and minimum contributor rank.
A global certificate attached to one source without checking its neighbours
would be unsafe. No artwork or runtime behavior changed in this audit.


## September 15: capacity and burst follow-up

Pass41 overlaid the owner-grid collection and drawing workspace lifetimes,
allowing16 terminal rectangles in660 scratch bytes. Pass42 uses24 terminal
tile rectangles in756bytes while restoring rectangle subtraction's established
eight-piece CPU limit. No source quad, UV, resolution, layer or effect changes.
Native frozen-frame GPU checks are exact at both framebuffer positions; the
full-route final image remains identical. Pass42 averages1.998278VBlanks, with
217/2,487/188/12 one/two/three/four-tick intervals.200frames remain over budget;
reaching a30fps mean is not passing the maximum-two-tick gate.

Route-attributed average GPU work falls23.544→22.638→22.327ms for40/41/42.
All twelve pass42 four-tick intervals coincide with particle bursts and prefix
misses. Temporary completion instrumentation measured2,906build ends with
zero DMA-busy results: the final-DMA wait averaged0.1005scanline, maximum one.
An earlier packet-ring preparation rewrite has no measured useful window and
was rejected. The diagnostic stamps are removed in the following normal build.
PC samples remain instruction samples, and modeled GPU work remains emulator
rather than original-console evidence. Reports: `.hkpsx/pass42-timing-audit/`,
`.hkpsx/30fps-pass4{1,2}-profile/`.


Pass48 retains32 terminal tile pieces,8 rectangle-subtraction pieces, and948
scratch bytes using exclusive owner/draw phases. Optional seam certificates
add8,180 bytes for124 selected groups. Every member must resolve in the current
region and remain eligible; the minimum member rank retires joint coverage.
The final selection passed2,054 fractional phases and281,016,479 certified
pixel checks against the native GPU, plus12 complete-frame comparisons.
Pass45's repeated linear binding searches introduced activation spikes to five
ticks and were rejected. Pass46/48 use a temporary3,040-entry inverse map and
restore the maximum to four. This map is stack storage, not static RAM.

The retained CPU work includes an exact radix16 square root, reuse of collision
sweeps between the two particle passes, axis-specific enemy probes, an empty
mask preflight and register-resident packed nibble run counts. Only the sweep
cache adds stack (2,048 logical bytes); final function frame is4,352bytes.
Preparing the current state during the initial idle window remains protected
by exact prefix validation. Pass47's whole textured-source outlining regressed
cadence (206 slow intervals versus195 in46) and was reverted; do not repeat it
as an unmeasured cleanup. Final48:189 slow intervals, five at four ticks; the
strict target remains unmet despite the nominal30fps mean.

All final48 actual EXE/CUE route images are pixel-identical to40. The exact
Lifeblood checks and13-hit Great Door progression pass, with no boundary waits
in these replays.14/23 pre-existing particle drops remain. Reports and source
hashes are in `.hkpsx/30fps-pass48-*`; grouped proof detail is in
`.hkpsx/opaque-groups/` and `.hkpsx/pass36-certificate-gap-audit/`.
