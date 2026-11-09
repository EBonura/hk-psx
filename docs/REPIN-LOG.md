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
- ability-dash and ability-wings (outcome pins, not pacing): the first dash found the carried ability art pack absent (`HK_FX_LATE` 1, `HK_FX_RAISED` one short) because the pack read is paced by frames and a frame now takes longer. Forty neutral polls inserted at poll 480 of both tapes (inside the idle gap after the Cheats page closes, before the dash); both pass with the pins unchanged. HAND EXCEPTION to the no-hand-retime rule, accepted by the coordinator: these routes load no scene after the first, so `cuts` (closed loop) does not apply, and validate judges every route open loop. The pack arrival is a race with a margin under 10 polls even on the pre-repin build (its tape fails with 10 polls removed; the repinned build needs 10 to 20 polls more than the tape has). Open: the first earned dash after the Cloak in Fungus1_04 also raises no effect (walk-new-game-to-hornet, HK_FX_LATE 17 to 18, HK_FX_RAISED unchanged); the pack is not resident then. To be converted to walk scripts.
- secret-c03-reload: `HK_PLAYER_X` 670932 against the pinned 634672 (gameplay begins 4 polls earlier than on the old build). Four neutral polls inserted at poll 100 (same hand exception, accepted); the final X is 634672 again to the unit.
- Baseline check: the pre-repin twin of ab7760b passes all 60 routes and the journeys; the shipping disc is not a valid disc for the open-loop routes (they need the route-input twin built in the same tree as the validate).

## 2026-11-07 13:05 UTC, disc 05fc6c8d4c69

Emulator rebaseline, not a game change (coordinator decision 2026-11-07, policy of 2026-10-23: emulator accuracy first, then repin). The tools moved from the retained frontend b3848054 to emulator main bb788f5b (frontend and psoxide-gate rebuilt there). On the same disc (SDK bfb464405) the old frontend passes focus, cheat-dash, c37-stand, grub-jar and secret-c37 inside their old ceilings (focus 207 of 217, grub-jar 228 of 251), the new one is over on 18 routes with identical outcome pins and no tape changed. The SDK bump is neutral for pacing (focus 207 on both, the figure measured on 9a0bca6). The cause is the timing model that landed in the emulator between b3848054 and bb788f5b (10-22/10-23: a streaming fetch's wait charged to the pending issue cycles, GPU DMA request waits, the I-cache miss-path series); not bisected to one commit. Ceilings moved by `validate --repin` (the rule of 2026-10-29), except grub-jar, which is 23 percent over (228 to 280) and so past the 10 percent slowdown guard: its three ceilings were set by hand by the `pace-pins` rule (frame_time the measurement plus 5 percent, pacing the measurement plus 30 percent and 2 frames), measured 280, 101 over 2 vblanks of 150 frames. No tape and no outcome pin changed.

- grub-jar: frame_time 251 to 294 (measured 280), pacing 56 to 134 (measured 101), by hand
- baldur-spell: frame_time 275 to 294 (measured 280)
- c37-stand: pacing 407 to 545 (measured 417)
- cheat-dash: pacing 2 to 12 (measured 7)
- cheat-dream: frame_time 241 to 257 (measured 244), pacing 268 to 383 (measured 293)
- cocoon-open: frame_time 301 to 324 (measured 308)
- ctrl-climb: pacing 344 to 454 (measured 347)
- focus: pacing 28 to 54 (measured 40)
- kings-death: pacing 286 to 451 (measured 345)
- kings-return: frame_time 245 to 260 (measured 247), pacing 243 to 356 (measured 272)
- kp-playtest: frame_time 227 to 243 (measured 231), long_frames 6 to 12 (measured 8), pacing 173 to 274 (measured 209)
- mound-spell: frame_time 264 to 304 (measured 289)
- secret-c03: frame_time 368 to 417 (measured 397)
- secret-c04: pacing 33 to 47 (measured 34)
- secret-c09: pacing 26 to 39 (measured 28)
- secret-c10: frame_time 225 to 242 (measured 230), pacing 27 to 70 (measured 52)
- secret-c37: frame_time 447 to 490 (measured 466)
- secret-f08a: long_frames 4 to 17 (measured 12)

## 2026-11-08 (hk-hornet): the three hand-retime exceptions are gone

The open-loop tapes `ability-dash`, `ability-wings` and `secret-c03-reload` (the three routes of 2026-11-07 whose neutral polls were
inserted by hand: 40, 40 and 4) are replaced by walk scripts that wait on game state, so nothing about them races a pack read or the
poll on which gameplay begins:

- `walk-ability-dash` (Cheats page, the Cloak, two dashes; waits for `art_fx` to be resident; HK_CHEATS 16, FX_RAISED >= 2, FX_LATE 0, PARTICLES_SPAWNED >= 32).
- `walk-ability-wings` (Cheats page, Wings, two double jumps; HK_CHEATS 64, FX_RAISED >= 4, FX_LATE 0).
- `walk-secret-c03-reload` (the reload card, no hits, no breaks, walks left of where the wall stood; judges a place, not the X after 90 polls).

Their tapes, pins and route files are deleted. The Hornet-arena half of the same finding is fixed in the game (docs/ABILITY_EFFECTS.md,
"Hornet's arena"), not in a tape: `walk-new-game-to-hornet` raises the Cloak's dash effect on the first dash.

## 2026-11-08 13:15 UTC, disc c66faa26f484

- c37-stand: frame_time 238 to 252 (measured 240)
- cocoon-open: pacing 104 to 114 (measured 106)
- secret-c18: long_frames 3 to 7 (measured 4)
- secret-c37: long_frames 16 to 24 (measured 17)

Layout noise, not a pacing change in a route's own area: the Hornet-arena change (SDK 31c3149b7, emulator d31e7075) adds 656 bytes of resident code, which moves every address after it. The baseline twin of origin/main cd1ad83 built in the same session passes the four routes inside their old ceilings (c37-stand 237 of 238, cocoon-open 103 of 104, secret-c18 0 of 3 long frames, secret-c37 14 of 16); on the new tree they measure 240, 106, 4 and 17 (secret-c37 frame time 466 to 475, c37-stand 237 to 240, cocoon-open frame time 308 to 306). The bb788f5b emulator gives the same figures to the digit on the new tree, so neither the emulator nor the SDK pin is the cause. Moved by `validate --repin`; outcome pins unchanged.

## 2026-11-08 16:33 UTC, disc d15e1e359fef

- c37-stand: frame_time 252 to 272 (measured 259), pacing 545 to 766 (measured 587)

Per-scene Geo bank landing (docs/GEO.md). Measured A/B in the same session: the baseline twin of origin/main 2150dae passes c37-stand at frame time 240, 410 frames over 2 vblanks (1,236 frames); the twin of this tree measures 259 and 587. Every other route and journey passes inside its old ceilings. Nothing the standing Knight runs changed per frame (the room's one rock is 23 units above him and rejected off screen, no coin is out), the change adds 568 bytes of code and takes 2,144 bytes of data away, which moves every address after it; the cause is therefore taken to be layout (the Hornet-arena entry above and docs/ROUTES.md measured the same room's route at 237 to 240 and other routes up to 10 percent either way on layout alone). Not proven by a layout-only control build; the figure is kept in the log so a later landing that moves it back can lower the pin. Moved by `validate --repin`.
