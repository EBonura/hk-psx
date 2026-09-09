# Status, 2026-09-09

## Current build: geometry fills, destruction effects, Lifeblood and burst pacing

This build is validated on the sole disc and supersedes the Geo milestone's
hashes below. It restores the opaque black `tk2dTileMap` interiors the cooker
had skipped, spawns the source `Breakable` particle families for all 33
supported Tutorial breakables, adds the Lifeblood cocoon, Scuttler and
blue-health logic, and fixes the simulation stall that the first destruction
burst caused. Details: [GEOMETRY.md](GEOMETRY.md),
[BREAK_EFFECTS.md](BREAK_EFFECTS.md), [LIFEBLOOD.md](LIFEBLOOD.md).

### Destruction burst and input queue

The first actual-CUE replays after a burst panicked with `input sampler
contract`, fault `QueueFull`. Around 105 particles each rescanned every room
edge through a callback that returned 16 bytes through memory and tested them
with `memcmp`; a sim tick then cost more than one VBlank, the fixed-step loop
fell behind real time and the 16-sample poll queue filled after about 24
VBlanks (`.hkpsx/break-overload-profile/`, fault at poll 383).

The tick now integrates every particle first, unions their swept boxes, decodes
the non-empty room edges once per tick into a 128-entry scratch keeping only
edges that touch that union, then collides each particle against that subset in
edge order. Array equality tests became scalar tests, `lerp` uses an exact
32-bit division identity instead of `__divdi3`, and source-plane particles skip
a 64-bit division per draw. `tick_debris` builds its filtered edge cache in one
pass over the exclusion lists. Particle counts and trajectories are unchanged:
the synthetic full-pool differential, the fallback path beyond the scratch and
the generated-source differential (33 owners, 650 ticks, 2,745,600 slot states)
match the naive reference exactly, and both actual-CUE routes end at the same
positions as the intermediate build (`.hkpsx/burst-fix-*`).

| Long 5,871-poll profile | Worst frame | Frames over 6 VBlanks | Poll queue peak |
| --- | ---: | ---: | ---: |
| Before this fix | fault at poll 383 | n/a | 16 (full) |
| Edge scratch only | 21 VBlanks | 6 | 11 |
| Two-pass particle tick | 7 VBlanks | 0 | 4 |
| Current build | 5 VBlanks | 0 | 2 |

The user's first play recording on that build (2,958 polls, preserved with its
hash in `.hkpsx/user-crash-input/`) froze the screen in region 45 with the same
`QueueFull` fault from a second path: 16 loose Geo coins with seven broken
objects, each coin re-decoding and re-filtering every room edge through
`State::edge` per tick (`.hkpsx/user-crash-4/`, reproduced by extending the
tape 600 polls past the freeze; the tape ends at the freeze because a halted
guest stops polling). `State` now owns the filtered edge table, refreshed once
per simulation tick in `tick_debris` and invalidated by every mutator of the
Geo, Lifeblood and breakable exclusion sets, so coins, Scuttlers, enemies,
corpses and the Knight read one table. The extended tape now completes all
3,558 polls with a poll queue peak of 2, 17 coins active
at most, ending in region 44 with wallet 30
(`.hkpsx/coin-fix-crash/`). Both standard routes end at identical positions.

Any sustained simulation tick above one VBlank still halts the game by design:
the 16-sample poll queue faults rather than dropping input. That is the right
contract for a fixed-step simulation, but it means every per-body edge scan
must stay bounded; grep for `state.edge(` before adding another body type.

### Final validation

`python3 tools/build.py` wrote the sole disc; the final EXE has zero R3000
hazards (`.hkpsx/occl-build5.log`). `make test` passes 210 Python and 279
Rust/native tests (`.hkpsx/occl2-tests.log`). Actual-CUE replays completed
with unchanged EXE/map/disc/tape/frontend hashes and zero guest, CD, input,
missed-input and geometry-repair faults:

- `.hkpsx/occl2-profile/`: the standard 5,810-poll user tape, 2,068
  render intervals, mean 2.8066 VBlanks, p95 3, maximum 5; 43 intervals
  exceed four VBlanks, so **the strict four-tick gate still fails**. Endpoint (163.154, 6.39062), region
  8, health 4, 2 kills, 6 breaks, 730 particles
  spawned, none dropped, 42 post-start activations with zero
  boundary waits. The preceding Geo build measured mean 3.3995, maximum 5.
- `.hkpsx/occl2-ascent/`: the 2,450-poll Lifeblood route breaks
  8 objects and spawns 768 particles, 36 rejected
  by the full 128-slot pool. It ends in region 2 at (54.4249,
  11.3906) and never reaches the cocoon; Lifeblood remains unverified in play.
- `.hkpsx/occl2-geo-pause/`: the 6,971-poll mining/pause route
  repeats the Geo milestone's results (wallet 18, one depleted rock,
  2 kills, 6 breaks, 730 particles).

Captures every 30 or 60 route ticks were inspected around the first burst and
at the route ends: stone and plant particles animate and settle, platform
interiors are opaque black, and the inspected frames show no background wedge
under a platform. A dark King's Pass frame that previously showed a dark-blue
wedge on its right now shows black tilemap interior there. During a
100-particle burst frames take four to five VBlanks.

Linked code 332,124B, data 267,936B, BSS 1,360,368B, static span 1,960,432B; the gap
before the reserved 32KiB stack is 38,160B. Main-stack high-water remains
unmeasured. EXE 602,112B, SHA256 `d598d0f4c6e9d0fc4eff73ca7e8b516c48bab0dfefce47c093ee198ffcce3f90`. BIN 3,544,464B, SHA256
`a39c1b98e9c6563c2b2e38d9eec1742e77dfc5f42e59ac86f0084ee1ec021e2a`. Map SHA256 `2ce1aa339530a9ce6a9137c35847859641fbbb0df775608ca89af6a10b1f8dbf`.

### Frame pacing

The renderer now kicks packets progressively as separate DMA lists so the GPU
starts 1.8 ms after the flip, draws cooked black-mask cores as flat quads and
subtracts the largest front-layer opaque black rectangles from earlier
axis-aligned draws, all pixel-exact. The long profile's mean render interval
fell from 3.262 to 2.807 VBlanks (444 of 2,068 frames at two), the stationary
spawn camera from 3.00 to 2.61. Frames fit two VBlanks below about 31 ms of
emulated raster; the route's median frame is 33.1 ms, so a consistent 30 fps
still needs 15 to 25% less raster than exact rendering of the source layering
produces. Analysis, tools and the decision options are in
[PERFORMANCE.md](PERFORMANCE.md). Because pacing moves region activations, the
ascent route now ends with health 3 and wallet 0 at the same position.

## Geo and functional menus: preceding validated build

The sole normal disc now includes five source Geo rocks, Crawler payouts,
animated loose coins, collection and a wallet HUD. Mining preserves original
hit counts, cooldowns and per-hit/final payouts; depleted rocks use original
broken art. Coins retain state across spatial views and use bounded deterministic
physics. The source collision matrix confirms rocks do not block the Knight.
Original pickup/rock-hit sounds are resident. Details and limits: [GEO.md](GEO.md).

The title has Start Game, Options and Controls. Start in gameplay opens a pause
menu. Sound-effect and ambience levels apply to playing voices, including full
mute, and persist for this run. Cross/Start on the initially selected Start Game
still enters directly, retaining the existing recording's launch sequence.

All 47 coin Idle/Air animation frames plus intact/depleted rocks use 57 frame
references to 50 unique textures. Their 5,154-byte 4bpp bank occupies disjoint
VRAM strips. Reducing the unused scenery CLUT reservation to 1,248 slots (1,206
used) provides space without altering current scenery textures. The new 13,904B
Geo sound bank leaves 1,520 SPU bytes. No mining or pickup event reads the CD.

Final linked code is 278,296B, data 239,840B, BSS 1,462,288B; the gap before the
reserved 32KiB main stack is 18,160B. Main-stack high-water remains unmeasured.
Preserve the particle, scene-admission, title, dialogue and Geo outlining
boundaries: inlining large functions can exceed MIPS PC16 branches. The actual
final EXE passes its R3000 hazard scan with zero hazards.

### Final validation

Actual-CUE replays completed with unchanged EXE/map/disc/tape/frontend hashes.
The 6,971-poll long route plus mining/pause tail depleted one five-hit rock,
registered two Crawler kills, spawned 19 Geo and collected 18, leaving one coin
and no pending payout. All five rock-hit events, one destruction event and
16 pickup sound events fired. The wallet stayed visible; source coins, intact/
broken rocks, title/options/controls, pause pages and the level were inspected
in 16 final-build captures, including software and hardware final views.

The initial feature replay exposed five missed input samples at 14 moving
coins. Production checkpoints between coins/emissions and bounded contact scans
remove all five in the final replay. Guest, CD, input, missed-input and geometry
repair fault counters stay zero. Pausing preserves the Knight's position across
280 route samples and resumes correctly. Title and pause settings both work;
a separate full-mute replay has zero nonzero PCM samples. Listening quality and
physical SPU output remain unverified.

The standard 5,871-poll route retains zero boundary waits, zero traversal CD
reads and zero static texture uploads: sectors481, scene admissions2 and
static VRAM bytes693,952 remain constant across41 post-start activations.
Its mean render interval is3.3995VBlanks, p954, maximum5. **The strict four-tick
frame gate still fails:20 of1,707 intervals take five ticks.** This is slightly
slower on average than the shared-only3.3797 result; changed pacing can also
alter trajectories, so the runs are not frame-identical A/B measurements.
No complete-seamlessness or physical-console performance claim is made.

The suite passes198 Python tests and244 Rust/native tests, with further native
scenarios run inside Python wrappers. Ten Geo scenarios include64 coins against
1,024 edges with differential physics checks. The final EXE hazard scan passes.
Evidence: `.hkpsx/gameplay-expansion-final-tests.log`,
`.hkpsx/gameplay-expansion-final-build.log`, `.hkpsx/gameplay-final-cue/`,
`.hkpsx/gameplay-final-menu/`, `.hkpsx/gameplay-final-muted/`,
`.hkpsx/gameplay-final-profile/analysis.json`,
`.hkpsx/gameplay-final-review/validation.json` and its contact sheet.

Replay without rebuilding or opening the GUI (choose a new ignored output dir):

```sh
python3 tools/replay_cue.py --tape .hkpsx/gameplay-expansion-input/geo-pause.pxtape --output .hkpsx/geo-replay --screenshot-interval 30
python3 tools/profile_tape.py --exe dist/hk-psx.exe --map build/hk-psx-normal.map --cue "$HOME/Downloads/ps1 games/hk-psx.cue" --tape .hkpsx/user-6246-input/latest.pxtape --output .hkpsx/geo-profile --require-seamless
```

The profile command intentionally returns nonzero while its frame gate fails.
Input-tape construction is recorded in `.hkpsx/gameplay-expansion-input/README.md`.

The canonical pair remains `~/Downloads/ps1 games/hk-psx.bin/.cue`.
EXE SHA256 `5dac23d7f1da68ff9975c7b748349060bf41cb0d2a654cbb02b34c0e8a835e2e`;
BIN SHA256 `345a178b71fa03fe2ec92093d45b5f4a1cfdad0f4071d69b278f80ac0c66f89e`.
`.hkpsx/build.json` binds exact output sizes/hashes, source reports and map.

Still missing: Geo spending/shops, saving, recoverable Shades, full rock gleam/
jitter/destruction effects, title animation/music, flying enemies and complete
scene scripting/progression. Death currently removes Geo permanently; this is
an explicit incomplete system. Rendering cadence and memory headroom need work
before larger gameplay/content expansion. No full-game or hardware parity claim.

## Previous milestone: shared scene residency

The single main disc now contains two HKSCNE01 scene banks instead of98
independent room payloads. All98 local region views and the currently authored
Tutorial/Town content stay in RAM and VRAM. Exact index-plane sharing with
independent palettes preserves sampled16-bit texels; no additional resolution
reduction or lossy merge was applied beyond the approved95% cook.

| Resident scene | Raw RAM bytes | Stored disc bytes | Static pages | Palettes |
| --- | ---: | ---: | ---: | ---: |
| Tutorial |929,692|452,831|18|1,065|
| Town entrance |116,756|46,395|2|141|
| Total |1,046,448|499,226|20|1,206|

The1,179,648-byte shared arena admits the larger scene first, then decodes Town
in the remaining suffix. Both pinned SDK and incremental decoding match exact
raw bytes and preserve the first admitted scene. Scene validation yields bounded
work; local draw/frame/clip/edge references retain their original ordering.
All98 serialized region geometry packet bounds pass. Source world metadata is
regenerated even on a cook-cache hit. Animation keys refer to global scene
textures, retaining slots across spatial crossings and resetting on scene changes.
Native review covers96 same-scene consecutive pairs; all reuse checks show zero
uploads. The single cross-scene pair resets its animation namespace.

The linked normal build uses225,100 code bytes,204,976 data bytes and1,461,452
BSS bytes. It leaves107,060 bytes before the reserved32KiB stack, versus44,428
previously. Main-stack high-water and hardware performance remain unmeasured.
The final EXE has zero R3000 hazards. Startup scene admission is outlined to
avoid the MIPS PC16 range error; preserve that boundary and the particle outlines.

### Current replay evidence

Both the actual main CUE and profiled EXE complete the same5871-poll long tape,
ending at(163.153656,6.390625), region8, health4, with two kills. Across43
post-start region activations the CD sector counter stays481, admitted scenes
stay2 and static VRAM bytes stay693,952: **zero gameplay CD reads, scene decodes
or static texture uploads**. Both boundary-wait counters stay zero. Input,
guest/CD and geometry-repair fault counters stay zero. All input VBlanks were
sampled. This removes measured loading freezes on this route.

| Long replay | Boundary waits | Worst wait | Mean frame interval | p95 | Maximum |
| --- | ---: | ---: | ---: | ---: | ---: |
| Previous95% build |478ticks|93ticks|3.663VBlanks|5|6|
| Shared scene build |0ticks|0ticks|3.380VBlanks|4|5|

The strict seamless gate **still fails only its four-tick frame budget**:
16 of1,717 measured intervals take five VBlanks. Average interval falls7.74%,
but simulation/poll paths can diverge; matching inputs/endpoints do not make
this a frame-identical A/B. Rendering cadence is now the next performance task.
Startup loading is longer because both scenes are admitted before play.

Actual-CUE software and hardware final captures show the level;22 sampled
captures from the107-image title/loading/gameplay sequence were inspected.
The first-door replay shows four active/drawn fragments and source grass
particles. The long route finishes with20 settled fragments and66 bounces,
275 particles spawned and zero pool drops. Particle totals differ with route
and pacing. Full effects/shaders remain incomplete. This tape stays in Tutorial;
Town banks and all views pass native validation, but natural Town traversal and
its audio change have not been newly demonstrated.

Validation:422 tests pass (186Python +236Rust/native), including exact scene
mapping, palette sharing, bounded validation and sequential decoder checks.
Evidence: `.hkpsx/shared-residency-tests.log`, `.hkpsx/shared-residency-build.log`,
`.hkpsx/packed-scenes.json`, `.hkpsx/shared-residency-profile/analysis.json`,
`.hkpsx/shared-residency-cue/`, `.hkpsx/shared-residency-door-cue/`.
The actual CUE and EXE/map/tape/emulator hashes are bound in replay reports;
inputs remained unchanged during validation. The prior report is retained in
`.hkpsx/shared-residency-baseline/` without another disc copy.

Current playable file: `/Users/ebonura/Downloads/ps1 games/hk-psx.cue`.
No candidate/backup/telemetry disc was created, and no GUI was opened.
EXE SHA256: `f50f1ef460e770d68792c8f905227800b35552bd28f9aec6162fc03cd63aab9d`.
BIN SHA256: `703ad32325a15e71720392832d842070d35d42bef3bf956c2d02e018d3d2f91e`.

Next: profile the remaining renderer/activation cost, verify natural Town
progression, then continue source effects, Geo and gameplay coverage. The
whole-game inventory has unresolved dependencies and no admitted whole-game
budget; fitting this current subset does not establish full-game residency.

## Previous milestone and investigation history

Everything below records the preceding95% region-streaming build. Its hashes,
allocations and timing figures are historical; the section above describes the
current playable disc and supersedes its next-step streaming proposals.

## Production95% consolidation — built and replayed

The user's95% cutoff is now applied to all eligible same-material static pairs,
including different original sprites. The fresh cook has37qualifying pairs in
4direct-representative groups and15global aliases; the lowest admitted member
score is95.1491%. Every member matches its FINAL representative, with no
transitive threshold chaining. No80% merges were applied. Animation/effect frames
and shaped masks are protected. Exact full-quad strict-black masks also share a
4x4constant tile, with unchanged CLUT/index and complete sampled-word verification.
The larger sourcecanonical proposal was not applied: source identity alone did
not meet the approved95% metric.

Across98packs this removes669texture entries,13pages and471476raw bytes versus
the same fresh cook before merging. These are summed per-region savings, not
471476bytes of simultaneously resident VRAM. Final maxima:5pages,392textures,
244968raw bytes. `host/similarity_dedup.py` verifies geometry/material/frame/clip/
collision preservation and stages every pack before publication. Full proof:
`.hkpsx/similarity-dedup.json`. `tools/build.py` fingerprints the matcher/helpers.

Deterministic MaxRects packs unchanged texels. Four416-CLUT banks occupy4096
previously unused VRAM bytes, with all allocations checked together for overlap.
Texture pages, framebuffer, font, HUD and animation-cache reservations are intact.
The common4x4tile preserves oversized-quad UV-grid repair;1x1was rejected by the
geometry admission check. The final constant GPU proof compares12902400pixels
across tints, fades, rotations and both framebuffers with exact results:
`.hkpsx/lossless-room-fallback/gpu-report.json`.

This build also integrates the prepared original grass/death particles, bouncing
and settling persistent door fragments, deterministic scenery dimensions/cache
keys, fully-soft-black material admission and the corrected black endpoint.
All particle families remain present. Large particle spawn/tick/draw functions
are outlined to resolve the MIPS PC16 branch-range compiler failure; final EXE
hazard scanning passes. These systems remain bounded approximations of Unity.

The earlier80% visual gallery is historical review data. Optional lossless
palette-plane sharing and HKSCNE01 shared scene banks remain host/parser work;
the legacy guest still uploads region banks. Do not infer seamless traversal
from offline global-bank capacity results.

## Current playable build

The sole playable entry is
`/Users/ebonura/Downloads/ps1 games/hk-psx.cue`, referencing `hk-psx.bin` beside it.
Every build replaces that pair. No candidate, telemetry, backup or versioned
disc entries are kept. `.hkpsx/build.json` identifies the current EXE/map/disc.
The user opens PSoXide themselves; validation below used isolated headless runs.

Current normal EXE SHA256:
`a172ed26e52493716a5fcddd47790b71238e4de0fd961f72e1f320fbdcef2d7d`

Current BIN SHA256:
`67cb0c385fadc1ddfc0015c0787a08e2eeda112db7b7a64ac14b539cb42c06e8`

This is an incomplete Tutorial/Dirtmouth port. Existing source-derived systems
include layered/parallax scenery and terrain, Knight movement/nail attacks,
breakable doors and grass, three Crawlers, damage/health/SOUL, recoil/pogo,
hazard checkpoints, Focus, six resident sound effects and cave ambience. The
minimal title uses original assets. Six source visibility controllers and safe
geometry extents address the previously disappearing level. Continuous VBlank
input sampling remains active through CD/decoder/GPU work.

## Latest recording and changes

The6246-sample host profile and5871-poll input tape(start91) are preserved with
hashes in `.hkpsx/user-6246-input/`. Host frame timing is not guest PS1 timing.

- Crawler deaths now play the original Air/Land corpse clips and retain the last
  corpse frame. Dead actors cannot cause damage or award repeated SOUL. Contact
  response is a bounded approximation, not a full Unity/Box2D reproduction.
- Cut grass now shows the original slash-impact clips. Seven authored doors
  activate their four original fragments, with28 persistent source-indexed
  records. Source launch/gravity and approximate rotation are implemented;
  fragments bounce and settle on terrain. Source grass/death particle subsets
  are active with a128-particle pool; full source effects remain incomplete.
  Grass impacts and fragments share a32-packet draw limit.
- Verified premultiplied black-mask materials now darken through PS1 averaging
  instead of ineffective additive black. Opaque cores and transparent texels
  are unchanged. Intermediate alpha is an explicit50% blend approximation;
  mixed-color materials are not silently converted. This is not full shader parity.
- Three tutorial tablets use original text, Perpetua font, source prompt markers
  and exact collider triggers. Up/Down opens; Cross advances/closes; Circle
  closes. Close/attack input is consumed until released. Movement locks while
  reading; damage and scene changes close it. Source dialogue choreography,
  alignment and presentation effects remain incomplete.
- Completed compressed reads are retained in existing RAM arenas for possible
  reuse. Urgent inactive VRAM uploads can be cancelled after GPU completion.
  Vertical motion can prioritize an approaching direct exit. New region entry
  installs its wishlist before pumping. Background reading and decoding overlap;
  only an explicit urgent demand can defer an unrelated new decode.

The source audit decoded all88 Tutorial FSMs. The first recorded read attempt
atx113.80 was slightly outside the first tablet trigger. There is no read trigger
at(166.75,6.39); the nearby Geo Rock is attack-driven and its payouts remain
unsupported. Do not widen source triggers arbitrarily. The other pickup found
in the audit is inactive and elsewhere. Evidence: `.hkpsx/read-point-followup/`.

## Validation and remaining stutters

All404 tests pass (179Python and225Rust/native), recorded in
`.hkpsx/dedup-95-final-tests.log`. The final EXE has zero R3000 hazards.
All98packs pass pinned and incremental LZ4 decoding in five256KiB arenas.
The link leaves44428bytes before the reserved32KiB main stack. Main-stack
high-water and physical-console timing remain unmeasured. See BUDGET.md.

Both the profiled finalEXE and actual mainCUE complete all5871polls, with identical
endpoint(163.153656,6.390625), region8, health4 and two kills. Actual software/
hardware final captures, first-door captures and a97-capture gameplay sequence
were checked; a21-frame contact sheet includes the darkest capture. The level
remains visible. Evidence: `.hkpsx/dedup-95-profile/`, `.hkpsx/dedup-95-cue/`,
`.hkpsx/dedup-95-door-cue/`. The route has zero guest/CD/input/geometry-repair
faults and zero missed input VBlanks. It spawns400particles with zero dropped,
records66debris bounces, and finishes with20settled persistent fragments.

| Long recording | Boundary-wait ticks | Longest wait | Mean frame interval | Maximum |
| --- | ---: | ---: | ---: | ---: |
| Earlier baseline |648|86|3.633VBlanks|7VBlanks|
| Previous main |646|90|3.636VBlanks|7VBlanks|
| Current95% main |478|93|3.663VBlanks|6VBlanks|

Total waiting falls26.0% against the previous main, with82loads and4989sectors
(previous84/5218). The worst boundary wait increases90→93ticks, mean frame
interval is slightly higher and p95 remains5VBlanks. **The strict seamless gate
still fails.** This is a combined art/effects/packing build, not a controlled
single-variable performance test, and matching tape/endpoints do not guarantee
identical trajectories. Do not claim overall rendering performance improved.

The first integrated experiment had438waiting ticks but followed a different
upper route. Subsequent builds exposed a decoder-priority regression; it was
removed after demonstrating40ticks of idle decoder time followed by a21tick
crossing stall. Fresh-wishlist activation alone did not improve aggregate waits,
and its proposed exact eviction mechanism remains inferred. These experiments
are retained under `.hkpsx/user-6246-update1/`, `user-6246-final/`,
`user-6246-activation/` and the associated stall audits. They are not alternate
disc files. Do not compare poll-bound totals as identical-trajectory benchmarks.

A targeted actual-CUE first-door run verifies four active and four drawn pieces;
its before/after capture sequence and both renderers were inspected. The original
actual-guest tablet test opens source12118 at(110.758,31.390625) and Cross closes
it, using controller input without patched RAM. That test used the first
integrated EXE; later controller replays take a different route and do not
activate tablets. Native production GPU tests cover all four source pages in
both framebuffers. Evidence: `.hkpsx/read-points/guest-1/`,
`.hkpsx/read-points/native-gpu-report.json`, and first-door CUE reports.

## Next work and commands

The immediate performance task is to preserve/read the needed rooms earlier
across vertical and brief-return transitions. The latest full trace still has
ordinary traversal holds; do not label the port seamless because input is sampled
continuously. Compare the first divergent transition and exact read/decode/upload
leases before trusting an aggregate score. A separate compressed cache or new
prediction policy needs measured admission and RAM/VRAM evidence.

Remaining gameplay includes Geo drops/rocks, full particles/shaders, other enemy
families, complete scene scripting, NPCs, saving, inventory, abilities, bosses,
music, title options and complete-game progression. The existing earned-Focus
validation tape does not meet its coverage assertion; do not claim newly tested
healing without a successful route. Source physics/effect approximations are
recorded in the ignored destruction/door-debris audits.

Build: `python3 tools/build.py`. Tests: `make test`.
Replay with `tools/profile_tape.py --exe dist/hk-psx.exe --map build/hk-psx-normal.map`,
the main CUE, preserved tape, a fresh ignored output and `--require-seamless`.
Actual-CUE helper: `.hkpsx/user-6246-input/replay_main_cue.py --output ...`.
Never launch the user's GUI or create a second playable disc to validate changes.
