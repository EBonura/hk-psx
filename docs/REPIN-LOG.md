# Re-pin log

Pacing ceilings that `cargo hk-build validate --repin` moved on its own, newest last. A frame time more than 10 percent over its pin is not re-pinned (it fails); everything here is drift.

## 2026-10-30 10:11 UTC, disc 36c4af4b1303

- secret-c03: long_frames 22 to 31 (measured 23)
## 2026-10-30 14:05 UTC, disc 198eb24f74b4

- secret-c37: frame_time 415 to 438 (measured 417)
## 2026-10-31, disc 000daa9d9b15 (hk-husks, by hand: a route with a cut list is not re-pinned by --repin)

- journey-crossroads: long_frames 40 to 43 (measured 43). The route never enters Crossroads_10 and no scene it crosses has an actor bank, so this is resident-layout drift from the husks' reader (about 6 KB of resident code moved every address after it); frame time 207 of 257 and frames over 2 vblanks 462 of 1816 are unchanged, and the closed loop passes 10 of 10.
- Retired from `validate` (pins deleted, tapes, routes and captures kept for the host boss simulation, tests/test_boss_sim.py): boss-fight, boss-death, boss-wave, journey-false-knight and journey-reload. They replay a fight recorded in an arena without the three husks (the Barger hurts the Knight before the boss lands, as in the original); hk-walker's closed-loop walks (walk-false-knight-from-bench-47, walk-new-game-to-crest, and walk-husk-hits) are the fight gate now. journey-reload's coverage (save after the Crest, reset, reload) is walk-crest-save-reload (2026-10-31): the Crest bit and the 12 world items survive a power cycle at the Mound's bench.
