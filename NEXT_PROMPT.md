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

Current64 adds source footsteps/hard-land audio/Great Door hit, complete-loop
RAM-to-SPU ambience streaming, and original managed audio-call tracing. Read the
new STATUS/MUSIC sections and audio64 evidence before changing transport. No
music playback exists yet: next implement CD-fed title/music streaming using
136,704B contiguous free SPU and63,448B main-RAM headroom. Retain consistent
sample rates within categories; the user allows lower long-sample rates if
necessary. Then source-trace remaining cues and fix Great Door/Crawler/Town
issues. The2.5s door delay is a destination-entry delay; Town still ends at x48.

Work in `/Users/ebonura/Desktop/repos/hk-psx`, branch `main`. Read AGENTS.md,
README.md, docs/BOOTSTRAP.md and docs/STATUS.md; run doctor before another build.
Use the psoxide-debug skill for guest/emulator work.

## Retained gameplay from63

Build64 retains the best54 renderer and the functional Cheats pages from63 in the title
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
