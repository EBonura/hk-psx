# Routes: pins, tapes and state-driven (closed loop) play

Status: landed. `cargo hk-build validate` plays every route open loop on the pinned frontend, and every route with a cut list again closed loop on `psoxide-gate`. Design and first proof 2026-10-17 on main `acb16c8`; wired into the driver 2026-10-18 on `cbf459a`.

## A route is three files

```
tools/tapes/NAME.pxtape   the recorded pad, one sample per port-1 poll (PXITAPE2)
tools/tapes/NAME.pins     what the route must show at its end, and its metadata
tools/tapes/NAME.cuts     where the baseline replay saw the game load a scene (optional)
```

The driver globs `*.pins`; a lane that adds a route adds files and edits no table (the tables that used to sit in `host/hk-build/main.rs` were the merge hotspot). A pin file is one entry per line, `#` starts a comment:

```text
card = town-continue.mcd     boots from tools/cards/town-continue.mcd (absent: a freshly formatted card)
scene_gate = true            also run the resident-scene verifier on the open-loop replay
journey = 2                  segment 2 of the save journey: boots from the card segment 1 wrote
pacing = 303                 late frames (over two vblanks) the route may still show
art_late = 44                frames the route may still show late art for
HK_REGION_ID = 150           final telemetry that must be exactly this
HK_HEALTH >= 2               final telemetry that must be at least this
```

## The problem

A tape holds one pad sample per port-1 poll. The poll index is the only clock, so every event after a scene load is placed by how many polls the loads before it took. When a load gets faster or slower, the Knight is somewhere else when the next event lands. Admitting the road to Hornet moved every scene seek by 447 sectors, and `journey-false-knight` and `journey-reload` were realigned by hand for the third time in a week; `HK_NO_PREFETCH=1 ./build` (a disc whose gameplay is identical and whose gate loads are slower) fails the unchanged `crossroads-gate` tape on four pins.

## The closed loop

`psoxide-gate` (PSoXide-emulator `tools/psoxide-gate`) plays a TOML journey headlessly: a step can play a poll-bound tape (`tape`), start it at the current poll instead of its recorded one (`tape_rebase`), hold buttons until a RAM condition holds (`hold_until`), wait on a RAM condition (`wait_until`), and persist or reboot the memory card. HK exports what a closed loop needs: `HK_GAME_MODE` (0 title, 1 gameplay, 2 loading, 3 disc error), `HK_SCENE_LOADS`, `HK_SCENE_GATE_LOADS`, `HK_REGION_ID`.

The tape stays bit-exact inside a stretch of play; only the loads are closed loop:

1. `cargo hk-build validate` on a disc where the tapes play as recorded writes `route.csv` per route (`HK_GAME_MODE` and `HK_SCENE_LOADS` against `port1_polls`). `cargo hk-build cuts --tape NAME...` turns those into `NAME.cuts`: the polls where each `HK_GAME_MODE == 2` span began and ended, for every route with two or more loads.
2. For a route with a cut list, `validate` generates the journey on the fly: stretch k is the tape between the end of load k-1 and the start of load k. The journey plays stretch 0; then for each load it keeps the stretch's last held buttons down until `HK_GAME_MODE == 2` (the Knight is still walking into the gate), waits for `HK_SCENE_LOADS >= n` and `HK_GAME_MODE == 1`, and plays the next stretch with `tape_rebase`.
3. The route's pins become `ram` asserts at the end. A route in the save journey ends with a card snapshot that the next segment boots from (`card_fixture`), like the open-loop chain.

A load that takes 60 polls more or fewer than in the baseline now moves nothing after it. The rule for a route author is the one oot-psx states: use `wait_until` on the words the game decides, never a fixed wait through a load.

`cuts` files may also carry `wait POLL SYMBOL OP VALUE` lines for drift inside a scene: the tape is cut at that poll and the next stretch is released only when the state holds (a region reached, an enemy dead). Cut where the pad is neutral.

## What is judged where

* Open loop (frontend, `replay_cue.py`, unchanged): the tape completed, no fault counter, no stale vignette or camera miss, late art within `art_late`, the scene-gate verifier for `scene_gate` routes, pacing against `pacing`. For routes without a cut list the pins are judged here.
* Closed loop (gate): the pins of every route that has a cut list. The open-loop figures of such a route still count, except that its pacing is compared with the ceiling only when the open-loop tape still reaches the same pins on this disc: a tape that drifted did not play the route the ceiling was measured on, and the closed-loop run (which has no late-frame counter) is what judges the route. The report line says so.

## Running it

```sh
export HK_PSX_GATE=/path/to/psoxide-gate HK_PSX_GATE_SHA256=$(shasum -a 256 $HK_PSX_GATE | cut -d' ' -f1)
cargo hk-build validate --library DIR --jobs 2          # open loop, then closed loop for the routes with cuts
cargo hk-build routes   --library DIR --jobs 2          # closed-loop routes only (HK_VALIDATE_ONLY=a,b narrows)
cargo hk-build cuts --work .hkpsx/validate --tape crossroads-gate   # after a validate on a disc where the tapes play as recorded
```

The gate must be built from an emulator revision that has `tape_rebase` (`287b51d1` and later, the `codex/gate-tape-rebase-2026-10-11` line). `routes` looks for `DIR/game.map` (the link map of the disc under test) before `.hkpsx/build.json`, so a disc built in another checkout can be tried without rebuilding.

## Limits

* The gate is built from a newer emulator core than the pinned frontend (`b3848054`): pins are function, not cycles, and the proof discs pass with both. Pacing stays on the frontend.
* Phase inside a stretch. HK samples input on polls and simulates at the source's 50 Hz phase (`docs/STREAMING.md`). A stretch starts at the poll where `HK_GAME_MODE` returned to 1, so its phase against the simulation is whatever the load end gives. If a route with frame-exact input right after a load drifts, add `wait` cuts or the OoT answer, a `route-lock` guest feature that snaps the simulation clock to the VBlank grid at scene entry. Not needed so far; see the proof below.
* New scenes: a route written as goals (walk right until `HK_REGION_ID == n`, `hold_until` on `HK_PLAYER_X`) needs no tape at all; the tape is the exception, kept for bit-exact combat.
