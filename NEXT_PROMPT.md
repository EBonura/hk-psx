# Hollow Knight PS1: current handoff

## User steering takes priority

The user playtested63 and reported slow Great Door progression, missing SFX,
odd enemy behaviour, and a Town boundary that sends the hero back to the gate.
They explicitly requested a controllable HEADLESS original-game runner instead
of GUI testing/manual feedback. That runner now works; use it for reproducible
comparisons. Read docs/ORIGINAL_REFERENCE.md and the top of docs/STATUS.md.
Do not resume generic FPS work. The full-game goal remains unfinished.

Reference command: `python3 tools/reference_game.py run --name NEW_NAME
--frames 300 --tape tools/reference/tapes/movement.csv --timeout 90` (one line).
Prepare again after managed-driver changes. All original inputs/saves are read
only; instruments and captures stay ignored. The original startup/load/landing
sequence has specific prerequisites documented there. Do not force hero control
or misread an unready test as failed original movement.

Provisional69 is now the sole playable disc at the user’s request. It includes
scene-owned visibility certificates, zero final hazards and34,668B linked RAM
headroom. Tests301Python/412reportednative pass. Its850-poll Focus CUE replay
passes with an inspected final image byte-identical to68. Long gate/return/reset
replays also pass with exact restored proof/geometry bytes and matching68 final
images. Two gates take569 loading samples versus508 in68; no FPS claim. Town-ending RAM verification also passes for both banks.
Evidence: .hkpsx/game69/validation.json. Do not replace
the disc during the user’s playtest without further steering.

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
loading reuses the369,180-byte arena and static atlas slots, with retired GPU
work, VBlank blackout, renderer/animation invalidation and continuous actual
input/audio service. Both scene manifests use origins0; descriptor ownership is
explicit. Production host builds select `scene_gate`; joint mode remains tested.
Native exact decoder replacement proof is emitted on every pack.

Town graves now support source Hierarchy scaling for planar uniform-XY emitters,
validated with original native simulation/BakeMesh probes. All four Town emitters
cook; they use the existing scalar particle path while Tutorial tracks remain
399,432B. A disjoint unused2KiB Geo strip moved to break effects: Geo5,154B within
6,336B and break art6,330B within7,680B. No resolution reduction was introduced.
Read docs/ORIGINAL_REFERENCE.md for probe evidence and the documented watchdog
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
Scene arena369180, scratch948/1024. Linked budgets are not stack high-water.

## Mandatory build discipline

Exactly one playable pair, ALWAYS and ONLY in the user's library:
`/Users/ebonura/Downloads/ps1 games/hk-psx.bin` and `hk-psx.cue`.
No candidate, baseline, backup or telemetry disc copies. Root alone builds;
finish every headless reader before replacing the pair. The user opens PSoXide
themselves. Never control its GUI as part of testing this task.

Keep320x240, existing camera framing/layers/effects,48px scenery cap and approved
95% static texture dedup. Windows Steam CrossOver input only, read-only, Unity
6000.0.61f1. SDK7b929ce473ed44b10b0ffca08f413eebf4eeeec4, nightly2026-03-25,
R3000 load-hazard mitigation and final binary checks. Converted assets, CIL,
captures and generated tables stay ignored. User authorized source-only push
main to the private EBonura/hk-psx remote, not burns or retail uploads.

Commands:

```sh
python3 tools/doctor.py
make test
python3 tools/build.py
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
