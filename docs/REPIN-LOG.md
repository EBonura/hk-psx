# Re-pin log

Pacing ceilings that `cargo hk-build validate --repin` moved on its own, newest last. A frame time more than 10 percent over its pin is not re-pinned (it fails); everything here is drift.

## 2026-10-30 10:11 UTC, disc 36c4af4b1303

- secret-c03: long_frames 22 to 31 (measured 23)
## 2026-10-30 14:05 UTC, disc 198eb24f74b4

- secret-c37: frame_time 415 to 438 (measured 417)
## 2026-10-31, disc 000daa9d9b15 (hk-husks, by hand: a route with a cut list is not re-pinned by --repin)

- journey-crossroads: long_frames 40 to 43 (measured 43). The route never enters Crossroads_10 and no scene it crosses has an actor bank, so this is resident-layout drift from the husks' reader (about 6 KB of resident code moved every address after it); frame time 207 of 257 and frames over 2 vblanks 462 of 1816 are unchanged, and the closed loop passes 10 of 10.
- Retired from `validate` (pins deleted, tapes, routes and captures kept for the host boss simulation, tests/test_boss_sim.py): boss-fight, boss-death, boss-wave, journey-false-knight and journey-reload. They replay a fight recorded in an arena without the three husks (the Barger hurts the Knight before the boss lands, as in the original); hk-walker's closed-loop walks (walk-false-knight-from-bench-47, walk-new-game-to-crest, and walk-husk-hits) are the fight gate now. journey-reload's coverage (save after the Crest, reset, reload) is walk-crest-save-reload (2026-10-31): the Crest bit and the 12 world items survive a power cycle at the Mound's bench.
## 2026-10-31 (hk-walker, by hand)

- Retired from `validate` (pins deleted; the tape, route and cuts kept for a host simulation): hornet-fight, whose kill needed the pause menu's Invincible and Pure Nail cheats (`HK_CHEATS` 3). `walk-hornet-fight` replaces it: the same fight closed loop, nail only, no cheat, with the seal, the open gates, the Cloak and the dash checked on game state.
## 2026-11-01 (hk-world, by hand)

- hornet-road and hornet-road-defeated: the neutral ticks before the Fungus1_21 stretch (`load 6577 6636 2` in their `.cuts`) go from 2 to 1. Landing 2 (switch art in Fungus1_22's lower hall, vine platforms) moved the 22 to 21 load by one tick; the tape inside Fungus1_21 follows the Moss Chargers' phase, which passed with 1 and 3 and failed with 0, 2 and 4. `cargo hk-build measure` cannot read the pad on a disc where the route does not pass, so it was found by trying each value (the other 60 journeys of `validate --prune` passed unchanged). hk-walker's closed-loop route through 22 and 21 replaces this tape.
## 2026-11-02 (hk-enemies, by hand, disc 066464cfee87)

- journey-crossroads: long_frames 43 to 44 (measured 44). Resident layout drift: the route crosses no room with a family actor and loads no `actor_fsm` module; resident code moved by -780 bytes. Frame time 207 of 257 and 474 frames over 2 vblanks, closed loop 10 of 10.

## 2026-11-03 (hk-world, by hand)

- grub-jar: pacing 56 to 80 (measured 61 frames over 2 vblanks of 178; frame time 238 of its 251 ceiling, no frame of 5+ vblanks). Not layout drift: the same twin build with the Geo rock draws skipped measures 46 (frame time 227), so the cost is the Crossroads_05 rock beside the grub jar, whose art is now two stacked strips (Geo rocks 50 of 53).

- grub-jar: pacing back from 80 to 56. The 80 was set for 61 frames over 2 vblanks while the rocks' art was linked; with the rocks in the scene banks the route measures 42 (frame time 224 of 251, 189 frames), so the old limit holds and a limit follows the measure.

## 2026-11-06 (hk-resident, by hand, twin disc a20f2390ee2f)

- secret-c04: pacing 29 to 33 (measured 30; main 1954aa7 measures 27). `--repin` offered 41 and was cut back to the measurement plus 10 percent so the pin keeps its edge.
- secret-c37: frame_time 438 to 447 (measured 442; main 1954aa7 measures 436 of 438). Same cut-back from the offered 465.
- Cause for both: layout drift from the scene spec chunks (resident code +4,476 bytes) and the one extra sector each scene load reads. Both routes begin in a freshly loaded scene, and frame time and long frames are where a load's extra sector shows (about 1 percent). No gate, hit or fault changed.

## 2026-11-07 (unify-hk, by hand, twin disc ffeffbea2692): the 30 fps presentation cap

Manny's decision (2026-11-06): Hollow Knight takes the SDK's default 30 fps cap (`PresentCap`, SDK main c75f63928); the opt-outs are gone. A frame now lasts at least 2 VBlanks, so every frame of a light room measures exactly 2.00 vblanks and `--repin` (which refuses a frame time more than 10 percent over its pin) cannot move these pins. Each new pin is the measurement plus 10 percent.

- ability-claw: frame_time 150 to 220 (measured 200).
- ability-dash: frame_time 135 to 220 (measured 200).
- ability-wings: frame_time 130 to 220 (measured 200).
- town-reset2: frame_time 189 to 220 (measured 200).
- town-shade: frame_time 124 to 220 (measured 200).
- Every other pacing pin held; their frames were already at or above the cap or under their ceilings.
- ability-dash and ability-wings (outcome pins, not pacing): the first dash found the carried ability art pack absent (`HK_FX_LATE` 1, `HK_FX_RAISED` one short) because the pack read is paced by frames and a frame now takes longer. Forty neutral polls inserted at poll 480 of both tapes (inside the idle gap after the Cheats page closes, before the dash); both pass with the pins unchanged. These routes load no scene after the first, so `cuts` (closed loop) does not apply to them.
- secret-c03-reload: `HK_PLAYER_X` 670932 against the pinned 634672 (gameplay begins 4 polls earlier than on the old build). Four neutral polls inserted at poll 100; the final X is 634672 again to the unit.
- Baseline check: the pre-repin twin of ab7760b passes all 60 routes and the journeys; the shipping disc is not a valid disc for the open-loop routes (they need the route-input twin built in the same tree as the validate).
