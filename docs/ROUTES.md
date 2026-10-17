# State-driven routes

Status: design with a working proof of concept (`tools/census/src/bin/route-split.rs`). Written 2026-10-17 against main `acb16c8`.

## The problem

A route tape (`tools/tapes/*.pxtape`, format PXITAPE2) holds one pad sample per port-1 poll. The poll index is the only clock, so every event after a scene load is placed by how many polls the loads before it took. When a load gets faster or slower, the Knight is somewhere else when the next event lands. That is why three tapes needed realigning in one week (`journey-false-knight` twice, `journey-reload` once), and why 441 more scenes cannot be re-taped by hand.

The failure is easy to reproduce without touching the game. `HK_NO_PREFETCH=1 ./build` gives a disc whose gameplay is identical and whose gate loads are slower. The unchanged `crossroads-gate` tape then fails on it:

| Disc | `HK_SCENE_GATE_LOADS` | `HK_REGION_ID` | `HK_ENEMY_KILLS` | `HK_ENEMY_HITS` |
| --- | ---: | ---: | ---: | ---: |
| main 15ec4f6 (library disc) | 2 | 150 | 1 | 2 |
| same source, `HK_NO_PREFETCH=1` | 1 | 107 | 0 | 0 |

(pinned emulator frontend b3848054, `tools/replay_cue.py`, card `town-continue.mcd`; the pins are the `REQUIRED` rows of `host/hk-build/main.rs`.)

## What already exists

* `psoxide-gate` (PSoXide-emulator `tools/psoxide-gate`) plays a TOML journey headlessly. A step can play a poll-bound tape (`tape`), start it at the current poll instead of its recorded one (`tape_rebase`), hold buttons until a RAM condition holds (`hold_until`), and wait on a RAM condition (`wait_until`, kinds `ram`, `pixels`, `flat`, `dark`). RAM conditions name a symbol from the link map, so every `#[no_mangle] HK_*` word the game already exports is usable.
* oot-psx did the same for its journeys (`chore/oot-deterministic-gates-2026-10-17`, `docs/gate-lock-journeys.md`): journeys in simulation ticks, `wait_until` on game state for loads, fades and dialogue, plus a `gatelock` test feature that makes one simulation tick per frame on a fixed VBlank grid so that a pad hold of k frames is read by exactly k ticks at any phase. OoT needed that feature because its simulation is paced at 20 Hz against 60 VBlanks. Its hash chain (`gate-lock diff`) proves a perf change did not alter the game.

## The HK equivalent

HK already exports what a closed loop needs: `HK_GAME_MODE` (0 title, 1 gameplay, 2 loading, 3 disc error), `HK_SCENE_LOADS`, `HK_SCENE_GATE_LOADS`, `HK_REGION_ID`. The proposal keeps every existing tape bit-exact inside a stretch of play and makes only the loads closed-loop:

1. Run the tape once on a known-good disc with `replay_cue.py`. `route.csv` records `HK_GAME_MODE` and `HK_SCENE_LOADS` against `port1_polls` for every tick.
2. Cut the tape at each stretch where `HK_GAME_MODE == 2`. Stretch k is the polls between the end of load k-1 and the start of load k.
3. Emit one journey: play stretch 0; then for each load keep the stretch's last held buttons down until `HK_GAME_MODE == 2` (the Knight is still walking into the gate), `wait_until HK_SCENE_LOADS >= n`, `wait_until HK_GAME_MODE == 1`, and play the next stretch with `tape_rebase = true`.
4. End with a `ram` assert per `REQUIRED` pin of the route.

A load that takes 60 polls more or fewer than in the baseline now moves nothing after it. The rule for a route author is the one OoT states: use `wait_until` on the words the game decides, never a fixed wait through a load.

`tools/census/src/bin/route-split.rs` does steps 2 to 4:

```sh
route-split --tape tools/tapes/crossroads-gate.pxtape --csv BASELINE/route.csv --map BASELINE/game.map \
  --symbols MAP_OF_DISC_UNDER_TEST --card tools/cards/town-continue.mcd --out OUT --name crossroads-gate-sd \
  --disc DISC.cue --pin HK_SCENE_GATE_LOADS=2 --pin HK_REGION_ID=150 --pin HK_ENEMY_KILLS=1 --pin HK_ENEMY_HITS=2
psoxide-gate run OUT/crossroads-gate-sd.toml --disc DISC.cue --no-hw --out OUT/report
```

## Proof of concept

Both journeys below were cut from one baseline run on the library disc and played by a `psoxide-gate` built from the emulator branch that added `tape_rebase` (the gate's core is older than the pinned frontend, so the control run is on the same gate).

| Route | Disc | Open-loop tape (frontend) | State-driven journey (gate) |
| --- | --- | --- | --- |
| `crossroads-gate` (2 gates, a kill and 2 hits) | library disc | pass | pass, 6 of 6 pins |
| `crossroads-gate` | `HK_NO_PREFETCH=1` | fails 4 pins (above) | pass, 6 of 6 pins (`HK_SCENE_GATE_LOADS` 2, `HK_REGION_ID` 150, kills 1, hits 2) |
| `journey-kings` (new game to Dirtmouth, save) | library disc | pass | pass, 8 of 8 pins |
| `journey-kings` | `HK_NO_PREFETCH=1` | pass (the one gate there is not prefetched either way) | pass, 8 of 8 pins |

The journey is generated, so the tapes stay the source of truth and nothing is re-recorded.

## What it does not do yet

* **Phase inside a stretch.** HK samples input on polls and simulates at the source's 50 Hz phase (`docs/STREAMING.md`: after a load the simulation resumes from the last observed input timestamp). The rebase starts the next stretch at the poll where `HK_GAME_MODE` returned to 1, so the stretch's phase against the simulation is whatever the load end gives. The two routes above show it is stable enough for a combat route, but a route with a frame-exact jump right after a load may need what OoT built: a `route-lock` guest feature (test builds only) that snaps the simulation clock to the VBlank grid when a scene entry finishes, so a stretch always starts on the same tick phase. The main loop already realigns the frame clock after every gate load (`hk_clock.realign` in `main.rs`), which is why the phase is stable today; a lock feature would make that alignment independent of the input timeline too. It touches the guest, so it is left as the next step, not done here.
* **Pacing.** The gate does not produce the late-frame counts that `PACING_CEILINGS` pins. Keep `replay_cue.py` and the existing tapes for pacing on the baseline disc, and use the state-driven journeys for function. They answer different questions, and only the function pins were the ones breaking.
* **The gate binary.** It must be built at the emulator pin (b3848054 or later with `tape_rebase`) and driven by `cargo hk-build validate` (a `journeys/` directory beside `tools/tapes/`, run after the replays). Per-route pin files (the refactor in `hk-gaps/PLAN.md` section 5) are the natural place for the `--pin` values, so the generator reads them instead of the command line.
* **Authoring new routes.** `route-split` converts recorded tapes. For the 441 new scenes a route is written as goals (walk right until `HK_REGION_ID == n`, `hold_until` on `HK_PLAYER_X`), which `psoxide-gate` already supports; the tape is then the exception, kept for bit-exact combat.

## Recommendation

Adopt the split journeys as the gating form for function pins, keep tapes for pacing, and add the `route-lock` feature only when a real route needs frame-exact input right after a load. Convert the 55 routes in one pass with `route-split` once per-route pin files exist; the journey routes chain one card through power cycles, which needs the gate's `card` steps (snapshot and persisted reboot), so they convert last.
