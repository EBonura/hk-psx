# Hollow Knight PS1: current handoff

## Current state (2026-09-17)

Build 94 is the sole disc: King's Pass, Dirtmouth and the whole Forgotten
Crossroads (45 scenes, 693 views) connected by the authored gates,
validated by `cargo run --release -- build --frontend <PSoXide frontend>`
(seven route replays, zero faults). Hazards, checkpoints, breakables, mask
fades, remote masks, grass, neighbours, actor references and reveal
bindings now live only in the loadable HKWMTA01 bank
(object stride 48, three payload words, a fifth u16 index-list section); the
guest reads them through `world::bank_region` and the `Breakable`, `MaskFade`
and `Patch` views, `State::apply` is one pass over the region's objects, and
`State::reset_scene` walks the admitted bank. RAM headroom is 271,340 bytes after the per-texture flags and cores moved
into the resident scene bank, the break-effect tables into the effect-art
chunk and the pogo targets into the world bank; the chunk manifests (about
20 KB) are the last per-scene linked tables (P05 item 7). The Crossroads Zombie Runners are admitted
(controller, clips, corpses, resident audio bank; Charge Dust and death
effects are counted omissions). Route replays run concurrently (1:42 for
seven). Break-effect art streams per scene (one
disc chunk per scene decoded at admission and uploaded to the fixed effect
VRAM rectangles), so every supported emitter in all 18 scenes with breakables
is admitted (210, up from 103). Transient fades and grass cuts are
resident-scene tables; `broken` stays global. The bank's checked parse polls
the pad between regions (that unpolled stretch was the recurring Tutorial
return VBlank miss). `world::REGIONS` is gone: `Region` is a
runtime value that `world::resident` builds from the admitted bank plus
per-scene statics and small variant catalogues; gates and Great Door entries
carry their target region slot from the cookers; `Cache::locate` takes
catalogue scene ids (the disc module otherwise works in manifest indices, a
distinction Tutorial's 0 == 0 had hidden).
Crossroads Climbers (Tiktik) are admitted on top of the Crawlers and Runners
(15 placed instances; climb loop audio not yet presented). Build 95 admits
the Vengeflies (18 of 20 Buzzers, docs/VENGEFLY.md), bounds actor simulation
to one view around the active view (a recorded departure from Unity, marked
`ponytail:` in enemies.rs for review), refreshes the edge table right after a
break, and fixes controller clip remapping (Climber stun, Vengefly clips).
Builds 96 and 97 admit the Gruzzers (19, docs/GRUZZER.md) and Baldurs (11,
docs/BALDUR.md); the recognizer modules are now cook inputs. Build 98
parameterizes the Runner (walk/lunge speeds, waits, body and alert boxes) so
the Bargers and Hornheads and the other-scene Runners are admitted through it;
build 99 adds the Leap attack (5 of 6 Leapers); build 100 admits the Aspids
(18) with the guest's first projectile pool (docs/ASPID.md). Build 102 puts
the source CameraLockArea limits in the world bank and gives the guest a damped
camera with look-ahead (game/src/camera.rs); build 103 applies the source
gate rules (facing into side gates, the 3 s delayed top collider); build 104
runs the source entry sequence for every gate. Build 80 fixed the guest state tables (`broken`, fades, grass) that were sized
for three scenes on a nineteen-scene disc; the new `crossroads-exit` route
(climb authored with `tools/route_sim.py`, gate into Crossroads_02, four
breaks there, gate back) covers that path. The remaining static per-view
payload is six bytes per slot (`REGION_SCENES`, `REGION_SCENE_LOCAL`,
`SCENERY_PACKET_BUDGETS`); the Great Door bindings list only the slots that
see the door; P05's
end state is the whole region table becoming per-scene bank data, and the
per-scene state tables must stop scaling with the catalog (transient fades
and grass resident per scene, `broken` global). Region entry now polls the pad
between `render::init` and the state passes; that stretch was one VBlank
short on the Tutorial return before. Break-effect art beyond scene 4 and Geo
rocks over the shared VRAM budgets are recorded omissions; effect and Geo art
need per-scene streaming (P07/P25). `host/regions.py` reuses every region
whose spec, source inputs and cooker code are unchanged. The scene catalog is
`host/quality.py SCENE_TABLE`. Build speed (user priority) is done for the first pass: no-change or
guest-only build 1:48, cooker-code change about 5:00 with a parallel full
recook; see the ledger's build-speed section for what remains (geometry vs
metadata cook split, replay speed). Deferred with reasons: enemy death puffs and Runner charge dust outside King's Pass need the particle bank in every scene (48 static textures per view with the 4-alpha cells; views already sit near the 416 texture budget), and the Climber loop audio needs SPU RAM (8,032 bytes remain after the Runner bank). Build 105 adds benches (rest, heal, respawn marker, `town-bench` route); build 106 saves the bench rest to the memory card (psx-mc) and offers Continue on the title (`town-continue` route boots from `tools/cards/town-continue.mcd`). Build 107 adds death Geo loss, the Hollow Shade and the soul limiter (`town-shade` route, docs/SHADE.md); headroom is down to 68,988 bytes and wants a budget pass. `kings-death` records a real death on tape and `kings-return` boots from the card it wrote to recover that Shade, covering P13 step 9 (twelve routes). Builds 108 and 109: sound effects and Geo now resample through the shared ffmpeg polyphase filter rather than a box average (audible on the nail swing and footsteps), and the cave ambience bed cooks at 4 kHz, taking headroom from 68,988 to 166,876. The remaining ~100 KB needs the SPU relayout described in docs/BUDGET.md. Step 8 found and fixed a real save-loss window (see the ledger); the save now alternates two card files with a sequence number. The save screen and the bench save prompt are in; the card is never written automatically, which is a deliberate departure from the source. Next: the source save slots menu, Climber loop audio, chunk manifests off the linked binary, P09 well entry details, then Hatcher (needs runtime spawning), Shield, Guard, Mender, Hatcher, Shield, Guard and Mender, Runner dust and death presentation, the P09
well entry details, and moving emitter/style metadata into the world bank; P09 delayed well collider and source camera locks; Crossroads enemy
families beyond the Crawler.

## Previous handoff: P05 scene-owned world metadata

The user authorized execution of the complete-game plan. Track it in
`docs/plans/COMPLETE_GAME_PROGRESS.md`. P01-P04 are validated. The current
import verified 501/501 scenes with zero failures: 463 have complete source
geometry and 38 retain only 757 TextMeshPro plus four legacy TextMesh runtime
glyph-shaping cases. Exact affine circles, particles, trails, tk2d generated
meshes (including animator-resolved null collections) and shipped Unity meshes
are global. The canonical comparison verifies every one of 106 Tutorial/Town
regions and uses provenance for all 20 Town rows that lack `base_path`. P02 now
inventories all 318 PlayMaker scene-load actions alongside 1,119 TransitionPoints.
All 286 dynamic targets have dataflow status: 130 storage/constants, 89 defaults,
three same-object overrides and 64 runtime-injected P09/P22 targets. All 501
scenes have source roles with no exclusion and every TransitionPoint trigger
shape resolves. The installed completion checklist contains 44 boolean/group,
six integer, four nested Godhome and exact charm/SOUL rules. Installed CIL proves
the exact 112 maximum. All 95 contributing fields have managed and serialized
access evidence; all load routes have condition status, downstream owners and
reference cases. The stable registry covers every scene/component/gate/PlayerData
key/asset and the bounded state API costs 93,912 bytes at conservative caps.
The first P05 `HKWMTA01` scene-bank slice is implemented and measured for
Tutorial/Town with checked borrowed views. The scene pack emits metadata after
coverage and the guest Cache validates it in the unused aligned tail of the
scene arena before publishing a scene. A pinned no-disc guest build passes the
final hazard scan with 15,272 bytes before the stack floor; spatial actor and
checkpoint lookup now uses the admitted bank; the current playable
disc remains build 71 because it has the older chunk table. Continue P05 with a
pinned guest differential metadata/geometry/coverage run, and only then replace
the single disc pair.

P06 now has `tools/cook_scene_pack.py`: isolated, write-guarded per-scene packs
under `.hkpsx/scene-packs/`, merged into the matrix by
`tools/world_pack_matrix.py --scene-packs`. The whole catalog is
cooked: 457 scenes measured, 41 without a source envelope, three over the
128-view room limit; costs and causes are in the regenerated matrix and the
P06 evidence section of the ledger. Next for M1: decide the room-limit
ceiling for the three tallest scenes (P05 item 8), measure neighbour-window
and actor-bank costs on top of the isolated packs (P07), and continue P08.

## Completion plan for the next executor

The user requested an extensive execution plan for GPT-5.6-Sol to complete the
whole game. Start with `docs/plans/SOL_START_HERE.md`, then use the master plan
`docs/plans/COMPLETE_GAME_SOL.md`. It defines 32 work packages,20 initial slices,
source/code ownership, validation and the full-game completion audit. This
supersedes historical room-first and FPS-first sequencing below. Planning did
not resume the importer, modify gameplay or replace disc 71. When execution is
requested, start by revalidating state and diagnosing P01's worker failure.


## Current direction: whole-world import

Stopped at explicit user request. Source changes are saved; no further work should
run until requested.344 Python tests pass. Import checkpoints:249 exported scenes,
196 pool-failed jobs,56 unrecorded. Worker termination cause is unknown; final
source/code verification was not reached. Read top of docs/STATUS.md. Resume
with one worker after diagnosis; preserve the unchanged playable disc 71.

User requested importing all areas first, then implementing systems globally.
This supersedes older room-by-room NEXT items below. The current disc 71 stays
available during playtesting. Host-only `host/world_import.py` imports every
Windows BuildSettings scene to `.hkpsx/world-import/`; see docs/WORLD_IMPORT.md.
Do not wait for Runner sound/dust or a particular room script before importing
surrounding scenery. Import, PS1 packing, runtime admission and verified gameplay
are distinct coverage states. Source-only commit/push remains authorized.

## Latest71

User asked to play the latest build again. The sole Downloads/ps1 games/hk-psx
pair is now71: zero final hazards,850-poll CUE smoke passes, inspected image
matches70. Keep this disc available during their playtest. Evidence:
.hkpsx/playtest71/ and .hkpsx/build.json.326 Python and222 hk-sim tests pass.
Current linked headroom8,044B; main frame33,728B/49,152B reserved stack.

Runner is integrated into the guest actor branch and real host art/remap pipeline,
with native combat/corpse/Geo/sensing/clip/event tests. Canonical source admission
remains disabled; production intentionally rejects unbound presentation events.
Next bind Runner sound/Charge Dust and scene/reset cleanup before enabling it.
Complete isolated sound bank20,464B fits SPU495792..516256; host/runner_audio.py
verifies it. Proposed21/22 loop and23 shared call voices need explicit arbitration.
Actor code adds26,652B; measure duplicated generic instantiations and the planned
metadata migration before growing scene tables. Crossroads/well, Climber guest
integration and full-game progression remain unfinished. Read RUNNER.md/STATUS.md.

## User steering takes priority

The user playtested63 and reported slow Great Door progression, missing SFX,
odd enemy behaviour, and a Town boundary that sends the hero back to the gate.
They explicitly requested a controllable HEADLESS original-game runner instead
of GUI testing/manual feedback. That runner now works; use it for reproducible
comparisons. Read tools/hkref/README.md and the top of docs/STATUS.md.
Do not resume generic FPS work. The full-game goal remains unfinished.

Reference command: `tools/hkref/target/release/hkref --work DIR all PROFILE`
(see tools/hkref/README.md). Run `build` again after managed-driver changes. All original inputs/saves are read
only; instruments and captures stay ignored. The original startup/load/landing
sequence has specific prerequisites documented there. Do not force hero control
or misread an unready test as failed original movement.

The user requested a fresh provisional rebuild after source preparation70.
That rebuild now occupies the sole Downloads/ps1 games/hk-psx pair. It passed
full cooking, zero final hazards and an850-poll actual-CUE smoke replay with
no reported faults and an inspected final image identical to69. Evidence:
.hkpsx/playtest70/. No new playable area is included; long traversal checks
below refer to69. Keep this disc available during the user's playtest.

Provisional69 was the previous sole playable disc. It includes
scene-owned visibility certificates, zero final hazards and34,668B linked RAM
headroom. Tests301Python/412reportednative pass. Its850-poll Focus CUE replay
passes with an inspected final image byte-identical to68. Long gate/return/reset
replays also pass with exact restored proof/geometry bytes and matching68 final
images. Two gates take569 loading samples versus508 in68; no FPS claim. Town-ending RAM verification also passes for both banks.
Evidence: .hkpsx/game69/validation.json. Do not replace
the disc during the user’s playtest without further steering.

Source-only70 now includes strict two-instance Runner recognition, shared clip
inventory, Q16 Sweep/AlertRange/LOS queries and synchronous restart/Ready events.
Runner controller/sensing tests pass; current trace proves two original attack
cycles and exposes player-hit scaled-time pauses. The reference audio clock
validator now accepts the evidenced frame-to-fixed callback switch; original
run623 frames revalidates without changing raw evidence. Read docs/RUNNER.md
and tools/hkref/README.md. A separate source-derived Climber controller
models corner motion and stun with scaled-time support; see docs/CLIMBER.md.
Neither new enemy is integrated into guest actors or the disc. Preserve the
remaining physics, scheduling and RNG gaps when binding art, sound and combat.

The isolated full Crossroads pack fits98,808B RAM,7 pages and501 palettes;
.hkpsx/crossroads70/RESULT.md details all12 views, pending shared animation costs
and remaining unsupported Secret Mask/well/camera/actor work. This is capacity
evidence, not a playable third scene. Next integrate scene metadata and the well
connection with shared actor banks, preserving source IDs and state ownership.

Next capacity work: .hkpsx/world-metadata70/PLAN.md and counts.json describe
an offset-based gameplay metadata bank. The current root Region table alone is
20,352B (106x192); anonymous nested arrays are additional, not yet sized.
Preserve sparse persistent source/state IDs and replace edge-cache pointer
identity with scene generation before arena-backed RegionViews. Do not reserve
a guessed-size new arena without removing/measuring the old tables.

Final68 fixed the reverse gate entry.
Three actual-CUE routes now pass: Focus850polls, gate return12,112 and Town
traversal/reset12,562. The last reachesx177.274; both long routes replace the
scene twice and resume with exact cooked scene bytes, queue peak4 and no faults.
Their captures were inspected. See `.hkpsx/game68/validation.json` and top of
STATUS.md. Tests286Python/406reportednative, final hazards0, RAM gap20,348B before
48KiB stack; main32,200/nested16,952.23 particle drops remain. No FPS claim.

Work68 integrates exclusive scene admission at authored gates and expands Town
scenery/collision to0..270/-5..76, using106 stable region IDs (86Tutorial/20Town).
See STATUS.md for final build/replay status; do not infer a playable update from
host packing alone. All current regions inside a scene stay resident. Gate
loading reuses the354,032-byte arena and static atlas slots, with retired GPU
work, VBlank blackout, renderer/animation invalidation and continuous actual
input/audio service. Both scene manifests use origins0; descriptor ownership is
explicit. Production host builds select `scene_gate`; joint mode remains tested.
Native exact decoder replacement proof is emitted on every pack.

Town graves now support source Hierarchy scaling for planar uniform-XY emitters,
validated with original native simulation/BakeMesh probes. All four Town emitters
cook; they use the existing scalar particle path while Tutorial tracks remain
399,432B. A disjoint unused2KiB Geo strip moved to break effects: Geo5,154B within
6,336B and break art6,330B within7,680B. No resolution reduction was introduced.
Read tools/hkref/README.md for probe evidence and the documented watchdog
failure after the second probe's samples had completed.

NEXT: keep actual-CUE Tutorial/Town/return/Select validation green.
Use tools/replay_cue.py and tools/validate_scene_gates.py, inspect images and
hash-bound reports in `.hkpsx/game68/`. Then implement Town gameplay and the well
connection. `.hkpsx/crossroads68/RESULT.md` is a source-grounded landing audit:
Townwell -> Crossroads_01/top1, spawn(52.5,31), downspeed-12; the return well FSM
targetsTown/bot1 despite swapped serialized TransitionPoint strings. Top gate
collision enables after3s. Landing host-only cook fits236,872B/5pages; source
ZombieRunner/Swipe/Climber and conditional MenderBug behavior remain unsupported.
Do not admit that scene blindly or spawn every serialized-active enemy.
Town shops, NPCs, bench, well/Mines transitions and in-game music are unfinished.
Before admitting Crossroads, use `.hkpsx/scene-certificates69/PLAN.md` and its
reproducible counts.py: per-scene exact tile/group certificate bundles need
83,692B Tutorial/27,192B Town; an84KiB reusable arena should reclaim about24KiB
from110,066B of linked tables before loader costs. No implementation yet.
`.hkpsx/crossroads68/RUNNER-CONTRACT.md` and check_runner_contract.py contain
source-checked Zombie Runner behavior, shared animations/audio, LOS/Sweep,
recoil ordering and tests required. Original native trajectory comparison remains
pending; the landing-only cook does not implement this enemy family.


Historical66 added complete Focus charging/heal audio, fixes Crawler/corpse motion
across resident collision regions, and implements source-derived Great Door
blackout and Town entry fade/walk. The source2.5s wait is at destination entry;
it is not a pre-departure delay. Town spawn uses the source gate-ground position.
The renderer consumes observed simulation/input ticks before another render;
final10762-poll Town replay now completes without input faults, queue peak4/16.
A first66 replay overflowed and was rejected; use only `*-final` evidence.

Full tests pass272 Python/402 reported native, final hazards0. Four final-CUE
routes cover Focus, isolated audio, offscreen patrol and the full door encounter.
Read top of STATUS and MUSIC plus `.hkpsx/game66/validation.json`. Main RAM free
53,132B before48KiB stack; main frame32,192B, nested reserve16,960B; enemy helper
frame2,336B. SPU free28,496B after108,208B Focus bank. No physical stack/timing
claim. Town route records23 particle drops; no new FPS or full-parity claim.

Build65's complete44,100Hz stereo CD-DA title and independent Music setting remain.
Completed Pause hands the drive to room loading. Music uses no sample RAM/SPU
bank. Two-repeat/early-Start validation remains in `.hkpsx/music65/`; original
mixer DSP and hardware playback are unverified. Direct CD music is appropriate
for resident boss arenas when implemented, not simultaneous arbitrary room reads.
Build64's full cave-noises RAM-to-SPU ring, running/hard-land/door-hit SFX remain.
Focus uses category rates8kHz sustained/22.05kHz short. Keep source-traced cues
and consistent rates; retain full clips. Current textures and scene arena capacity are unchanged.

Next implement Town's music trigger and remaining interactions, source-trace
remaining enemy/break/death/UI sounds, implement unsupported enemies and advance
progression. No new maps were added in66. Door destruction/alternate hit sounds,
hard-land recovery, flying enemies, inventory/charms/abilities/NPCs/bosses/save and
full-game progression remain unfinished. Use the original headless reference
runner for bounded comparisons; do not claim exact Box2D/RNG/shader/mixer parity.

Work in `/Users/ebonura/Desktop/repos/hk-psx`, branch `main`. Read AGENTS.md,
README.md, docs/BOOTSTRAP.md and docs/STATUS.md; run doctor before another build.
Use the psoxide-debug skill for guest/emulator work.

## Retained gameplay from63

Build66 retains the best54 renderer and the functional Cheats pages from63 in the title
and pause menus. Four toggles: invincibility, Pure Nail21 damage, infinite SOUL,
and nine masks. Pause actions: restore health/SOUL, add5 blue masks (cheat cap20),
and turn all toggles off. X toggles/applies; Left/Right off/on; Circle backs out;
Start pauses/resumes. Session settings survive death and Select reset, but not
relaunch. Charms remain unimplemented and are explicitly identified in the UI.

Invincibility ignores contact damage and preserves safe hazard/fall relocation
without red/blue health loss. Infinite SOUL keeps normal Focus timing. Max masks
fills on enable and clamps on disable; it is not continuous healing. Turning
cheats off does not revoke blue health already granted. HUD pools accommodate
9 red and22 blue masks, wrap after14 columns and keep the Geo label clear.
Original textures/VRAM allocations remain unchanged.

Source nail21/upgrades4 and maxHealthCap9 were checked in Windows PlayerData CIL,
ignored at `.hkpsx/cheats-source/vitals.il`. The gameplay module is
`game/src/cheats.rs`; title state/menu, pause and dialogue provide UI. Runtime
hooks are in main/enemies. Telemetry: HK_CHEATS bits1/2/4/8, HK_CHEAT_NAIL_DAMAGE,
HK_CHEAT_MASK_CAP. Default cheats exactly preserve normal damage semantics.

Validation:247 Python and379 directly reported native tests pass; final MIPS
hazards0. Native tests include default damage parity, lethal invincible hazard
recovery, actual enemy nail/contact integration, Focus drain/heal timing, input
edges, screen-bounded title rendering and expanded blue HUD packets. Three
actual-CUE menu routes pass, showing9 health/99 SOUL,20 blue masks, Select-reset
persistence and normal5damage/5mask settings after disabling cheats. Normal
Lifeblood all4 checks pass; its final image is pixel-identical to54. No faults,
missed polls or boundary waits;14 pre-existing particle drops remain.
No new63 FPS timing claim is made.

Evidence: `.hkpsx/cheats63-validation/validation.json`,
`.hkpsx/cheats63-lifeblood/lifeblood-coverage.json`,
`.hkpsx/cheats63-tests-passed.log`, `.hkpsx/cheats63-build.log`,
`.hkpsx/cheats63-artifacts/`. Captures were inspected.

EXE SHA256:10f85ace6f2f1381caa09ef4cc76932b96d1303ecf93350ddcfbd26d9b40f3dc
BIN SHA256:ff85bd0ebf8619c4cbe8d96ef0225f6eb00fd9487cf3392cc66f3a9e54e46e00
Memory: code340680/data771376/BSS567812/alignment8, span1679876,
end0x801aa204, gap302332 before49152-byte stack. Main32120, nested reserve17032.
Scene arena354032, scratch948/1024. Linked budgets are not stack high-water.

## Mandatory build discipline

Exactly one playable pair, ALWAYS and ONLY in the user's library:
`/Users/ebonura/Downloads/ps1 games/hk-psx.bin` and `hk-psx.cue`.
No candidate, baseline, backup or telemetry disc copies. Root alone builds;
finish every headless reader before replacing the pair. The user opens PSoXide
themselves. Never control its GUI as part of testing this task.

Keep320x240, existing camera framing/layers/effects,48px scenery cap and approved
95% static texture dedup. Windows Steam CrossOver input only, read-only, Unity
6000.0.61f1. SDKa67052ac61b1e9caf078f570dbf238011d3958b6, nightly2026-03-25,
R3000 load-hazard mitigation and final binary checks. Converted assets, CIL,
captures and generated tables stay ignored. User authorized source-only push
main to the private EBonura/hk-psx remote, not burns or retail uploads.

Commands:

```sh
python3 tools/doctor.py
make test
cargo run --release -- build
python3 tools/validate_cheats.py --prepare .hkpsx/new-cheats-check
# Repeat for title, pause and reset after the build has completed:
python3 tools/replay_cue.py --tape .hkpsx/new-cheats-check/title.pxtape --output .hkpsx/new-cheats-check/title --screenshot-interval 10
python3 tools/validate_cheats.py --replay .hkpsx/new-cheats-check
```

## Deferred optimization evidence

Best pre-cheat54, restored identically as57:153late/2965intervals,6 four-tick,
mean1.957504VBlanks. An average near30fps does not meet maximum2VBlanks.54 retains
exact399432-byte particle shape tracks for1513ordinals/71emitters, plus142-byte
bases and256-byte live IDs. Shared compact scene and row events are unchanged.

Rejected this follow-up:58 Z-force lookup185late/9four;59 forced-inline version
161/7;60 no-extra-piece small tile cuts179/5;61 fused region admission175/4;
62 split-cost acceptance184/5. All are reverted. Native correctness or smaller
code does not establish faster gameplay. docs/PERFORMANCE.md records the trials,
corrected fine-grid net costs, mask-prepass timing limitation, seed-cache and
colored-core audits. Avoid repeating them without a new measured reason.

One-pass transactional rectangle subtraction was identified from54 PC samples
but NOT implemented when the user stopped optimization. It would need64bytes
of staged rectangle storage with exact primary-then-reversed-extra ordering,
failure immutability and fallback for input counts above8. Any future trial
must prove equivalence and actual-CUE cadence; no speedup is established.

Both loaded scenes are Tutorial and Town's left entrance,98 spatial regions,
not98 complete rooms. Complete inventory/charms/abilities/NPCs/bosses/saves/music
and full-game progression remain unfinished. docs/ROADMAP.md records the broader
scope. Claude's reviewed clean worktree was removed; its committed history
remains on the preserved Claude branch/remote codex/bootstrap. No sibling edits.
