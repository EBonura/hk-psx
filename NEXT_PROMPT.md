# Hollow Knight PS1 port: current handoff

## Delivery and working constraints

Work in `/Users/ebonura/Desktop/repos/hk-psx`. Read AGENTS.md, README.md,
docs/BOOTSTRAP.md and the current section of docs/STATUS.md; rerun tools/doctor.py.
Use the psoxide-debug skill. Windows CrossOver Steam assets are read-only build
inputs. Never substitute the macOS installation. Push only to the private
EBonura/hk-psx `codex/bootstrap` branch after clean actual-CUE replays; no
other publishing, burns, Steam changes or unnecessary sibling changes. The full port remains the goal;
this is an incomplete playable Tutorial/Town subset, not full-game parity.

The user explicitly requires ONE playable disc pair:
`/Users/ebonura/Downloads/ps1 games/hk-psx.bin` and `hk-psx.cue`.
Every build replaces this pair. Never create candidate, telemetry, backup or
versioned disc entries elsewhere. `--telemetry` replaces the same pair;
`--candidate` is only a deprecated compatibility alias. Old evidence reports
may mention deleted candidate files; those are historical, not instructions.
Internal EXE/map/log/capture snapshots may remain under ignored `.hkpsx/`.

The user opens PSoXide themselves. Do not launch, control or restart its GUI.
Use isolated headless runs. Multiple agents were explicitly authorized; assign
separate files and have the root perform all builds/disc mutations after they
report stable. Do not invent a persistent goal or automation.

SDK revision: `7b929ce473ed44b10b0ffca08f413eebf4eeeec4`.
Toolchain: nightly-2026-03-25. Preserve trim-paths, R3000 mitigation and final
EXE hazard scans. Emulator evidence is not original-hardware performance proof.

## Latest implementation: geometry fills, destruction effects, Lifeblood

Read the current section of docs/STATUS.md and `.hkpsx/build.json` before
citing results. The sole disc now has the source tilemap black fills
(host/tilemap_fill.py, docs/GEOMETRY.md), Breakable particle families for all
33 supported Tutorial breakables (host/break_effects.py, game/src/break_effects.rs,
docs/BREAK_EFFECTS.md) and Lifeblood cocoon/Scuttler/blue-health logic
(host/lifeblood.py, game/src/lifeblood.rs, hk-sim vitals, docs/LIFEBLOOD.md).

The first burst replays faulted with `QueueFull`: 105 particles each rescanned
every room edge through a 16-byte callback plus `memcmp`, a sim tick exceeded a
VBlank and the poll queue filled. `tick_specs` now integrates all particles,
unions their swept boxes, decodes non-empty edges once per tick into a bounded
scratch filtered by that union and collides against the subset; `lerp` and
the particle draw avoid 64-bit software division; `State::fill_edges` builds
the debris edge cache in one pass. Exact differential tests (synthetic pool,
fallback path, generated source: 2,745,600 slot states) guard trajectories.

Long profile `.hkpsx/burst-fix2-profile/`: 5,810 polls, mean 3.3028 VBlanks,
p95 4, max 7, 22 of 1,757 intervals over four ticks, poll queue
peak 4, zero faults, same endpoint (163.154, 6.39062) region 8. Strict
seamless gate STILL FAILS, as in the Geo milestone (max 5 there). Ascent route
`.hkpsx/burst-fix2-ascent/` ends at (54.4249, 11.3906) region 2
without reaching the cocoon: Lifeblood is unverified in gameplay. Geo/pause
route `.hkpsx/burst-fix2-geo-pause/` repeats the Geo milestone results.

Linked code 317,660, data 261,920, BSS 1,360,088; gap 58,920B before the 32KiB stack.
VRAM has 608 unallocated bytes inside 1 MiB after the Lifeblood and break-effect
rectangles. 210 Python + 279 Rust/native tests pass (`.hkpsx/burst-fix-tests.log`).
Build log `.hkpsx/burst-fix-build3.log`, zero hazards.
EXE SHA256 `d01705a5e0eea8904502cf4c6e75aee5730e0d8e722d89413682e62085b6478d`.
BIN SHA256 `adc5bf0740676c5f9497a91f7052c4157d2f61602d8a999faf49caf3b461701a`.

Next: the user will record a gameplay capture; the target is 30 fps. Steady
frames are GPU-bound (about 1,171,949 estimated GPU cycles and 187 textured
quads per frame, CPU mostly idle in the present-wait loop), so the lever is
overdraw of large blended scenery layers, not CPU. Also pending: a Lifeblood
route that actually opens the cocoon, Geo chips/gleam/jitter effects and the
remaining destruction paths listed in BREAK_EFFECTS.md.

## Previous implementation: Geo and functional menus

Read STATUS.md and `.hkpsx/build.json` before citing results. The sole normal disc
contains source Geo rocks, Crawler payouts, animated pickups, wallet HUD, Geo
sounds, title options/controls and an in-game pause/settings panel. Root owns
builds. Agents have completed/frozen their production files. No goal/automation.
See docs/GEO.md for behavior, source evidence and explicit approximations.

Host: geo.py and geo_audio.py. Guest: geo.rs, geo_render.rs, geo_audio.rs,
menu.rs/menu_state.rs, pause.rs, volume.rs; main/enemies/world/dialogue integrate
their state and callbacks. All five Geo rocks were absent from the old scenery
cook, so standalone bindings draw intact/depleted source art. Source matrix
excludes Knight solid-body contact with rock layer19; do not insert terrain.
Geo death events use retryable callbacks, bounded coins/pending counts and exact
source IDs to prevent duplicate payouts across spatial views. Select resets Geo
and counters. Death clears wallet; Shade recovery remains missing.

Geo art:57references/50unique textures/5154bytes; all47coin Idle/Air frames plus
five intact/five broken references. Scenery now capped1248CLUTs,1206used. The
unused former fourth CLUT group contributes a contiguous Geo rectangle. Shared
allocator proof includes five GEO_RECTS;9,920B reserved, VRAM9,824B unallocated.
Geo sound bank13,904B atSPU0x7c3c0..0x7fa10 leaves1,520B; six complete11025Hz
sourceclips, voices12..14. Break reuses existingfirst sourceclip; secondomitted.

Linked code278296,data239840,BSS1462288; gap18,160B before32KiB mainstack.
Mainstackhighwater unknown; reclaim RAM before major expansion. Preserve outlines
in particles,admit_scenes,menu::run,dialogue::prepare,andGeo to avoidPC16overflow.
198Python+244Rust/native tests pass; Pythonwrappers include10Geoscenarios and
native menu/audio traces. Zero finalEXEhazards. Final build log:
.hkpsx/gameplay-expansion-final-build.log; tests:gameplay-expansion-final-tests.log.

Final evidence is under `.hkpsx/gameplay-final-{cue,menu,muted,profile}/`.
All actual-CUE runs complete with unchanged hashes and zero guest/CD/input/
missed-input/geometry faults. Mining tail: 6,971 polls, five rock hits, one depleted
rock, two kills, 19 Geo spawned, 18 collected, one remaining coin. Both title and
pause settings/controls work; full-mute PCM is exactly zero. The Knight stays
still throughout the pause. Sixteen final-build captures, including SW/HW final
views, were inspected. Validation JSON/contact sheet: gameplay-final-review/.

First feature mining replay lost five input samples with 14 moving coins;
production checkpoints between coins/emissions and every32 contact edges fixed
this in the final CUE. Native64-coin/1,024-edge differential preserves physics.
Final EXE SHA256:
`5dac23d7f1da68ff9975c7b748349060bf41cb0d2a654cbb02b34c0e8a835e2e`.
Final BIN SHA256:
`345a178b71fa03fe2ec92093d45b5f4a1cfdad0f4071d69b278f80ac0c66f89e`.

The standard5,871-poll profile has zero boundary waits, zero traversal CD/static
uploads,41 post-start activations, sectors481/staticVRAM693952/admissions2
constant. Mean3.3995VBlanks, p954,max5;20 of1,707 intervals exceed four ticks.
Strict seamless gate STILL FAILS. Slightly slower mean than shared-only3.3797;
not a frame-identical A/B. Next performance task remains renderer cost, and
main RAM needs recovery/high-water measurement before adding substantial data.
Next gameplay work includes source Shade recovery, spending/shops, NPCs/saving,
flying enemies and additional effects; none are implemented by this milestone.

## Shared residency retained

Two HKSCNE01 scene banks hold98authoredlocalviews in one1,179,648B RAMarena:
Tutorial929,692B at0, Town116,756B at929,692;18+2staticpages,1065+141CLUTs.
No extra quality reductions beyond approved95%staticdedup; exactindex sharing
preserves sampled16bittexels and independentpalettes. Animation slots preserve
keys within scene and reset onactualscenechange. MAX_TEXTURES/MAX_KEYS2048.
All current sceneRAM/staticVRAM admission isbeforeplay. No per-regionCDloads.
The disc8chunks=2scenes+6ambience,1headersector. All98viewgeometrybounds and
sequentialsuffixdecoders validated. .hkpsx/packed-scenes.json binds sources.

Previous shared-only profile:.hkpsx/shared-residency-profile/analysis.json:
zero boundarywaits,CD481/staticVRAM693952constant,2admissions;mean3.380VBlanks,
p954,max5,16of1717intervals exceed4. Fullseamlessness not solved. Current feature
build needs its own numbers from STATUS. Source effects, flying enemies, NPCs,
Shades, shops, saving and fullprogression remain incomplete. NaturalTown gate
and full audio transition are not newlydemonstrated by the Tutorial recording.

## Earlier request and evidence

The latest user recording has6246 host samples and reports stutters, vanished
cut objects/enemies, translucent black areas, and two failed read attempts.
Preserved input: `.hkpsx/user-6246-input/latest.pxtape` (5871polls,start91),
matching hostCSV and input/build hashes. Host frame timing is not guest timing.
Baseline `.hkpsx/user-6246-baseline/`:648 boundary ticks,max86.
First integrated replay `.hkpsx/user-6246-update1/`:438,max81; still fails
seamlessness. The paths and measurements for the final combined build are in
the current section of docs/STATUS.md. Always read that before citing results.

Earlier integration: retained completed compressed reads; urgent inactive VRAM
upload cancellation; source Crawler death/corpse clips; source grass slash
impacts; source black-material admission/blending; original tablet text/font,
source trigger polygons and bounded dialogue UI. Door fragments, motion-aware
vertical preload priority, fresh activation wishlists and urgent-only decoder
admission are integrated. Earlier evidence is `.hkpsx/user-6246-overlap/` and
`user-6246-overlap-cue/`:646waiting ticks,max90, zero guest/CD/input faults;
strict seamless gate FAILS.348tests pass. No meaningful aggregate loading
improvement over baseline. See STATUS/build report for exact hashes and limits.

Tablet interaction evidence: `.hkpsx/read-points/guest-1/` opens source12118
using controller input atx110.758,y31.390625, then Cross closes it. No RAM state
was injected. Source audit `.hkpsx/read-point-followup/` confirms exactly three
Tutorial lore tablets. The recording's attempt atx113.80 is slightly outside
the first trigger. There is no read trigger at166.75,6.39; nearby Geo Rock is
attack-driven and its payouts remain unsupported. Do not widen authored
triggers arbitrarily. Up/Down opens; Cross advances/closes; Circle closes.

Known limitations include remaining transition stalls, incomplete original effects,
Geo drops/rocks, source shaders and dialogue choreography. Door first-flight
physics and corpse contact response are explicitly documented approximations.
Do not claim these reproduce the entire original effects or physics system.

## Reproduce and validate

Build only with `python3 tools/build.py` (or `make build`). Run `make test`.
Main artifact identity is `.hkpsx/build.json`; verify its hashes before handoff.

Replay the actual saved long tape with `tools/profile_tape.py`, using
`--exe dist/hk-psx.exe --map build/hk-psx-normal.map`, the sole main CUE,
`--tape .hkpsx/user-6246-input/latest.pxtape`, a fresh ignored output directory,
`--screenshot-interval 60 --require-seamless`. A failed strict gate is a real
remaining problem; do not relabel it a pass because inputs remained responsive.
Pacing changes can alter a poll-bound route, so compare positions/transitions
as well as totals. Use the map snapshot belonging to each run, not today's map.

Actual-CUE boot helper: `.hkpsx/user-6246-input/replay_main_cue.py --output ...`.
It reads the current main build report, verifies unchanged hashes and captures
both emulator renderers. It never builds or writes disc copies. Inspect final
captures. Do not combine GPU census flags with `--dump-hw`.

The previous earned-Focus tape did not satisfy its coverage assertions; do not
claim Focus healing newly validated without a fresh successful route. Preserve
existing exact checksum, source visibility controllers, safe geometry extent
and input-sampler fixes. Keep current reports distinct from older experiments.
