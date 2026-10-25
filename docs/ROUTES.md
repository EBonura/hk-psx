# Routes: pins, tapes and state-driven (closed loop) play

Status: wired into `cargo hk-build validate`, which plays every route open loop on the pinned frontend and every route with a cut list again closed loop on `psoxide-gate`. The four journey routes have cut lists; no other route does. What is proven and what is not is under "Proof status" below.

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
frame_time = 289             mean frame time over the judged frames, in hundredths of a vblank (289 = 2.89)
long_frames = 3              frames of five vblanks or more the route may still show
pacing = 303                 late frames (over two vblanks) the route may still show (secondary)
art_late = 44                frames the route may still show late art for
missed_vblanks = 12          pad polls the route may still miss (a CD Pause that blocks the game for several VBlanks)
HK_REGION_ID = 150           final telemetry that must be exactly this
HK_HEALTH >= 2               final telemetry that must be at least this
```

## The problem

A tape holds one pad sample per port-1 poll. The poll index is the only clock, so every event after a scene load is placed by how many polls the loads before it took. When a load gets faster or slower, the Knight is somewhere else when the next event lands. Admitting the road to Hornet moved every scene seek by 447 sectors, and `journey-false-knight` and `journey-reload` had to be realigned by hand for the third time in a week.

## The closed loop

`psoxide-gate` (PSoXide-emulator `tools/psoxide-gate`) plays a TOML journey headlessly: a step can play a poll-bound tape (`tape`), start it at the current poll instead of its recorded one (`tape_rebase`), hold buttons until a RAM condition holds (`hold_until`), wait on a RAM condition (`wait_until`), and persist or reboot the memory card. HK exports what a closed loop needs: `HK_GAME_MODE` (0 title, 1 gameplay, 2 loading, 3 disc error), `HK_SCENE_LOADS`, `HK_SCENE_GATE_LOADS`, `HK_REGION_ID`.

The tape stays bit-exact inside a stretch of play; only the loads are closed loop:

1. `cargo hk-build validate` on a disc where the tapes play as recorded writes `route.csv` per route (`HK_GAME_MODE` and `HK_SCENE_LOADS` against `port1_polls`). `cargo hk-build cuts --tape NAME...` turns those into `NAME.cuts`: the polls where each `HK_GAME_MODE == 2` span began and ended, for every route with two or more loads.
2. For a route with a cut list, `validate` generates the journey on the fly: stretch k is the tape between the end of load k-1 and the start of load k. The journey plays stretch 0; then for each load it keeps the stretch's last held buttons down until `HK_SCENE_LOADS >= n` (the Knight is still walking into the gate; mode 2 is not used because a gate that loads while the game plays never shows it), waits for `HK_GAME_MODE == 1`, and plays the next stretch with `tape_rebase`.
3. The route's pins become `ram` asserts at the end. A route in the save journey ends with a card snapshot that the next segment boots from (`card_fixture`), like the open-loop chain.

A load that takes 60 polls more or fewer than in the baseline no longer moves where the Knight is after it (the phase limit below still applies). The rule for a route author is the one oot-psx states: use `wait_until` on the words the game decides, never a fixed wait through a load.

A load line may end in `pad N`: the stretch after that load starts after N neutral ticks (see Phase below). `cuts` files may also carry `wait POLL SYMBOL OP VALUE` lines for drift inside a scene: the tape is cut at that poll and the next stretch is released only when the state holds (a region reached, an enemy dead). Cut where the pad is neutral.

## What is judged where

* Open loop (frontend, `replay_cue.py`, unchanged): the tape completed, no fault counter, no stale vignette or camera miss, late art within `art_late`, the scene-gate verifier for `scene_gate` routes, frame pacing against `frame_time`, `long_frames` and `pacing` (see Frame pacing below). For routes without a cut list the pins are judged here.
* Closed loop (gate): the pins of every route that has a cut list. The open-loop figures of such a route still count, except that its pacing is compared with the ceilings only when the open-loop tape still reaches the same pins on this disc: a tape that drifted did not play the route the ceiling was measured on, and the closed-loop run (which has no frame counter) is what judges the route. The report line says so.

## Running it

```sh
export HK_PSX_GATE=/path/to/psoxide-gate HK_PSX_GATE_SHA256=$(shasum -a 256 $HK_PSX_GATE | cut -d' ' -f1)
cargo hk-build validate --library DIR --jobs 2          # open loop, then closed loop for the routes with cuts
cargo hk-build routes   --library DIR --jobs 2          # closed-loop routes only (HK_VALIDATE_ONLY=a,b narrows)
cargo hk-build cuts --work .hkpsx/validate --tape crossroads-gate   # after a validate on a disc where the tapes play as recorded
cargo hk-build measure --library DIR       # on a disc where the route passes; writes each load's neutral ticks (pad N) into the cuts files
```

The gate must be built from an emulator revision that has `tape_rebase` (`287b51d1` and later, the `codex/gate-tape-rebase-2026-10-11` line). `routes` looks for `DIR/game.map` (the link map of the disc under test) before `.hkpsx/build.json`, so a disc built in another checkout can be tried without rebuilding.

## Phase

A stretch is bit-exact tape, so it replays like the baseline only if the simulation starts it in the same state, and that includes how many ticks the simulation has run since the load resumed. Without any help that number belongs to the disc: the region setup after a load takes 1 or 2 VBlanks depending on the layout, and the gate starts the stretch at a frame boundary, so the Knight stood still for 1 to 4 ticks (the neutral ticks below) and the King's Pass climb, which is 9,000 polls, needs the right parity of them. An EXE a sector longer or a larger prefetch arena changed the number and every closed-loop journey got stuck at its second load.

The fix has two halves, and no number is ever tuned per disc:

* The route lock (`game/src/input.rs lock_resume`). A route under `psoxide-gate` holds L3 (no tape and no screen uses it) from the moment a load starts until gameplay is published; the game sees it down at that point and, when a scene load has resumed and its region setup is done, it acknowledges every poll taken since the resume, waits for one fresh poll, and publishes `HK_GAME_MODE = 1` right after it. The gate sees gameplay at that poll, so the first tick the simulation runs is the next one on every layout. The same hook covers the first scene of a boot. A player's pad and the open-loop tapes never hold L3, so their timing is untouched; the lock exists only in state-driven runs.
* The neutral ticks are part of the route. `cargo hk-build measure --library DIR` plays the closed-loop routes without asking for the lock (the layout's own timing), reads from the guest's `HK_LOAD_LOG` how many controller polls each load's gameplay-is-back point followed its resume by, and writes that as `pad N` on the load line. `routes` and `validate` then ask for the lock and play exactly N neutral ticks before the stretch, on any disc. Regenerate with `cuts` (new baseline) then `measure` (on a disc where the route passes without the lock), and commit both.

The real-time Wait timers of the Crawler FSM (`advance_crawler_fsm`) follow the VBlank counter, which moves with how long each frame took; once a run has asked for the lock they count the simulation's own ticks, so frame pacing no longer reaches the simulation. Plain play keeps the VBlank counter: the open-loop tapes were tuned to it.

## Proof status (2026-10-19)

All on the pinned frontend `b3848054` for the open loop and `psoxide-gate` (emulator lineage `287b51d1`) for the closed loop, four journeys, one set of `.cuts` (measured on a lock-free disc), 12, 8, 21 and 15 checks:

* the pads' reference disc without the lock: 4 of 4 with the pads removed;
* with the lock and the pads: 4 of 4;
* with `HK_PREFETCH_EXTRA=10240` (different load timing: kings runs 83 ticks shorter): 4 of 4;
* the tree with the hk-ram and console-fix changes (a smaller EXE, every sector moved): see the landing notes in `hk-route-phase/NOTES.md`.

Not proven: the unlanded seamless-gates branch; open-loop routes that cross loads are still timed by the length of those loads and need cut lists like the journeys.

## New game: the drop and the shoulder hold

A new game enters King's Pass the way the source does, from the `top1` gate at the top of the shaft, and falls to the ledge (about 250 polls). The recorded routes were made from the ledge spawn and are bit-exact through fights and jumps, and the drop's frame timing, title card and region binds cannot be made to match the ones they were recorded in (a tape delayed by the fall tracked the recorded Knight to within 0.6 units for 6,600 polls and then missed a jump). So a tape that starts a new game holds L1, R1, L2 and R2 together across the load, polls 12 up to the first gameplay poll plus six, and the disc it plays reads them at the press that chooses the save slot and starts on the ledge (R3 would be nicer, but a digital pad, which is what the frontend replays, has none). `cargo hk-build tape-hold NAME FIRST LAST l1+r1+l2+r2` adds the hold to a tape; journey-kings carries it in its route string (`12:l1:18,12:r1:18,12:l2:18,12:r2:18`).

The hold is test input and exists only in a disc built with the `route-input` guest feature: `HK_GUEST_FEATURES=route-input` when building the disc that `validate` and `routes` play. A shipping disc (no feature) never reads the four buttons, so a player who holds them still gets the drop. The drop itself is `tools/journeys/new-game-lands.toml` (route-input disc, no hold) and `new-game-intro.toml` (shipping disc: the movies, the skip prompt, then the drop; `new-game-intro-full.toml` lets both play out), which press no shoulder buttons; `new-game-hold-ignored.toml` (shipping disc: the hold changes nothing) and `new-game-hold-ledge.toml` (route-input disc: the hold starts him on the ledge) prove the gate.

## The replay emulator is 0e87b3a (2026-10-23)

`emulator.lock.json` pins PSoXide-emulator 0e87b3a. From 0b9d577 the emulator charges a textured triangle or quad for the texels that outgrow the texture cache (costs measured on a console by hwtest v2.4: 14.7k to 45k clocks where it used to read 9,959). Hollow Knight draws about 300 large 4-bit textured quads a frame, so its busy scenes cost about twice what they did and 40 of 63 routes passed their old pacing ceilings no more. That is the console's cost, now visible; the game is to be made cheaper in a later pass (the brief is `docs/TEXTURE_CACHE.md`), and the ceilings were re-measured on this emulator at the measured count plus 30 percent, rounded up (the rule of `docs/KNOWN-ISSUE-INPUT-GAP.md`). The count rule itself stopped working on this emulator and was replaced; see Frame pacing.

Three things come with it:

* `missed_vblanks = N` in a pin file is the number of pad polls the route may miss, judged by `tools/replay_cue.py` (any other fault stays fatal). Four boss routes miss between 5 and 11: music stopping calls the SDK's blocking CD Pause, which waits several VBlanks when the drive is mid-read, and the polls that fall due are lost. The old timing happened to put that Pause where the drive was idle. `greenpath-walk` misses one poll while a scene loads.
* A module eviction or install flushes the instruction cache, and a `cache_control` write with a DMA channel running makes the next instruction fetch return the DMA's word on silicon. The slower GPU walks were still running at that point and the gate's `PSOXIDE_CHECK_CC_DMA` stopped `journey-crossroads`. `modules.rs` now waits for the GPU channel to idle (taking only pad polls) before the flush.
* `hornet-road` and `hornet-road-defeated` have cut lists. Pads (`pad N`) are the neutral ticks the baseline recording ran, so they are measured with `cargo hk-build measure` on the environment the tapes were recorded in (a gate built before 0b9d577); measured on the new emulator's own timing they are different numbers and the stretch does not replay.

## Frame pacing: what a ceiling is held to (2026-10-24)

A route's frame pacing is judged on the gameplay frames of its open-loop replay (`host/hk-build/pacing.rs`): the vblanks between two display flips while the game is in play and the room is resident. Loads, gates and the first frame after a view binds are not judged. Three ceilings sit in the pin file:

```text
frame_time = 289    mean frame time, hundredths of a vblank: the judged frames' vblanks over their number, rounded up
long_frames = 3     judged frames of five vblanks or more (the tail a mean can hide)
pacing = 130        judged frames over two vblanks (the 30 fps bar); secondary
```

`frame_time` and `long_frames` are the checks; `pacing` is kept as a secondary check. `cargo hk-build pace-pins [RUN_DIR]` writes all three from a validate run (default `.hkpsx/validate`; run `validate` without `--prune` so the route logs are there) by one rule:

* `frame_time`: the measurement plus 5 percent, rounded up.
* `long_frames`: the measurement plus 25 percent, rounded up, and 2 frames.
* `pacing`: the measurement plus 30 percent, rounded up, and 2 frames, held under the number of judged frames where that leaves it above the measurement plus 5 percent and 2 frames. A route with nearly every frame late therefore gets a ceiling just above its frame count, and leaves the checking to `frame_time`.

A route that has a cut list and whose open-loop tape did not reach its pins on that disc is skipped: it drifted, and its closed loop judges it. `journey-false-knight` and `journey-reload` drift on emulator 0e87b3a (their cards chain from the earlier segments), so they have no frame ceilings; the closed loop has no frame counter.

Why the old rule broke. The only ceiling used to be the count of late frames (`pacing`, the measurement plus 30 percent). That counts frames, and a frame is either late or not, so the count stops moving once most frames are late. On emulator 0e87b3a (the texture-cache cost model) most busy scenes are late on nearly every frame (`baldur-spell` 587 of 590, `grub-jar` 137 of 142), and 30 percent on a count that is already near the number of frames is a ceiling above the number of frames, which no replay can exceed: the check was as blind as one that always fails. Worse, a tape is a fixed number of polls, so the time a replay spans does not change when the game slows down; what changes is how many frames it draws in that time. The sum of the judged frames' vblanks therefore cannot be the ceiling either (it is the replay's length), but the mean frame time, that sum over the number of frames, rises when a frame goes from three vblanks to four, from two to three, or when a scene simply gets heavier, whether or not the late count could still move.

What was measured when the rule was chosen (a05ed03 on 0e87b3a; the pins themselves are measured on the tip they were committed on, see git log): the same code linked at other addresses (a one-iteration loop added to the frame loop, which moves every function after it) moved `frame_time` by 1 percent or less on 55 of 59 routes and lowered it by up to 10 percent on four (`hornet-road` 354 to 318, `secret-c37` 512 to 460); it never raised it past 5 percent. The count of frames of four vblanks went from 7 to 15 on `mound-spell` and from 46 to 52 on `arena-waves` while their means did not move: a frame of four is a three-vblank frame that the layout nudged over the edge. Frames of five or more moved by at most 4 on any route (20 to 24 on `ctrl-climb`), which is what the 25 percent and 2 frames absorb. A route that gets faster should lower its ceilings with `pace-pins` in the same commit, so the next regression is measured from the new figure.

The check does trip. A scratch build of the real tree (7ac2c5b, not committed) with a busy loop of 20,000 iterations at the top of the frame loop, about a fifth of a vblank of CPU a frame, makes `grub-jar` report `frame time 319 over 314` where the real build reads 299 and passes; the old count read 132 late frames of 132 against a ceiling of 179 and had nothing to say. The `long_frames` check did not trip on that case (2 of 2): it is the tail check, and a uniform slowdown is the mean's to catch.

## Limits

* The gate is built from a newer emulator core than the pinned frontend (`b3848054`): pins are function, not cycles. Pacing stays on the frontend.
* New scenes: a route written as goals (walk right until `HK_REGION_ID == n`, `hold_until` on `HK_PLAYER_X`) needs no tape at all; the tape is the exception, kept for bit-exact combat.
