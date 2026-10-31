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

Lit frames (2026-10-30, host/hk-build/brightness.rs): the open-loop journeys keep a screenshot every 400 ticks, and every one taken in gameplay after 120 ticks in the same room must have at least 14 percent of its pixels above black level 24. A build that drew the False Knight's approach rooms almost black at 60 fps (their alpha covers were still in the unread tail of the scene's stream) passed every counter, pin and outcome check; its rooms were 8 to 14 percent lit and a healthy build's darkest settled frames are 19 to 30. The validate report prints the darkest settled frame of each journey.

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

## Closed loop first (2026-10-29)

Open-loop tapes that cross a code change move by chance: the same pad poll lands in another frame phase, a fight resolves differently, a pacing figure shifts by a few frames. Chasing those by hand cost hours (journey-kings' one missed poll, the hornet routes' chaotic divergence). The rules now are:

* A route with a cut list (`.cuts`) is judged closed loop and keeps its exact pins. Everything else is judged on its outcome pins (`HK_REGION_ID`, boss dead, the flags and counters a player would notice) and its pacing follows the disc.
* Pins compare with `=`, `>=` and `<=`, so health, Geo and counts that may wander are bands (`HK_HEALTH >= 3`, `HK_GEO <= 120`).
* `cargo hk-build validate --repin` moves the pacing ceilings (`pacing`, `frame_time`, `long_frames`) of an open-loop route that only exceeded them, never one whose outcome pins failed, writes the new pin files and appends the old and new figures to `docs/REPIN-LOG.md`; commit both with the change that caused the move. A frame time more than 10 percent over its pin is not drift: the route fails and the lane investigates (`repin.rs`, `SLOWDOWN_PERCENT`). A lane chases a move only when an outcome pin or a slowdown fails.
* `cargo hk-build determinism` (HK_VALIDATE_ONLY narrows) replays each selected open-loop route twice on the same disc and fails if any final telemetry word or the per-poll route log differs. It pins no number, and it is what separates "the tape moved because the code did" from "the replay is not repeatable". cocoon-open, focus and grub-jar are identical on the round 5 shipping disc.
* Judge a reward or any presentation on game state (`HK_PICKUPS_TAKEN`, `HK_CHESTS_OPENED`, the flags), not on pixels: the presentation lanes change what the screen shows.

`cargo hk-build tape-extend NAME POLLS` adds neutral polls to the end of a tape; `tape-hold` then presses into them, which is how a route is continued past where it was recorded (journey-reload goes on to take the City Crest this way).

`cargo hk-build validate --resume` skips an open-loop route (no cut list, not a journey segment) that already passed under the same disc, judge (the `hk-psx-build` executable, `replay_cue.py`, `validate_scene_gates.py`, the frontend), tape, pin file and card fixture. Verdicts are kept in `.hkpsx/validate-verdicts/` (a validate run does not wipe them), written as each route passes, so a validate that was killed half way resumes where it stopped, a repeat on an identical disc replays nothing, and editing one route's pins replays that route alone. A failed route is never cached. Without `--resume` nothing is read from the store, so a run is as before.

A journey segment boots from the card the one before it wrote, so `validate` and `routes` run every earlier segment when one is named (`HK_VALIDATE_ONLY=journey-reload` runs kings, crossroads and false-knight first and says so). Run alone, a later segment used to boot a new game on an empty card and time out at its first load (region 1, "hold_until timed out").

## Walks: journeys written as goals (2026-10-30)

A walk script (`tools/walks/*.walk`, compiler `host/hk-build/walk.rs`) drives the pad from game state and plays no recorded input: every step holds buttons until a telemetry word crosses a value, waits on one, or taps for a few ticks. `cargo hk-build walk SCRIPT LIBRARY OUT.toml` compiles one to a `psoxide-gate` journey; `routes` and `validate` play every runnable script (on the library named by `HK_WALK_LIBRARY` when the tapes run on the route-input twin, else on `--library`) (a file whose first line is `name NAME`; the others are fragments that `include` into it) on the shipping disc with the same gate, after the closed-loop routes (`HK_VALIDATE_ONLY=walk-new-game-to-well` narrows). No gate change was needed: the gate's `hold_until` and `wait_until` already end at once when their condition holds, so every goal is a dead band and a reaction costs no time when there is nothing to react to.

Macros (the header of `walk.rs` has the grammar): `walk X` goes to x X from either side; `leap TX left|right N X Y` walks to a take-off, jumps with the direction held N ticks, and waits for the landing at (X, Y); `bash N` swings at a wall; `strike`, `guard N` and `fight N` react to foes; `heal` takes a Focus; `check NAME COND ; COND` is the end-state assert. The hop recipes (`leap` lines) were read off the journey tapes' replays with `jumps.sh` and `stations.sh` style tools: where the Knight stood at each jump press and where it landed.

Reactions are expressions the gate evaluates (psoxide-gate `kind = "expr"`) over raw words the guest publishes in `EnemyWorld::publish_walk_words` and `publish_walk_senses` (`game/src/main.rs`); none of the logic is code on the disc. The standing definitions are `host/hk-build/walk-prelude.walk` (`$strike_left`, `$strike_right`, `$strike_up`, `$turn_left`, `$turn_right`, `$jumpstrike`, `$focus`, `$gap_left`, `$gap_right`, with `$foe_*` under them), which every script has; the macros use them.

| Word | Meaning |
| --- | --- |
| `HK_ACT[32]` | `{x, y}` (Q16) of each live bodied actor of the loaded scene by slot; x is `i32::MAX` for a slot with none |
| `HK_PLAYER_X/Y/VY`, `HK_PLAYER_FACING` | the Knight's place, vertical speed, and facing (1 right, -1 left) |
| `HK_NAIL_BUSY` | 1 while the nail cannot start a swing (mid-swing, cooling down, a hit's recoil, a hard landing) |
| `HK_WALK_READY` | 1 on the ground with no landing lock and no gate entry running |
| `HK_HEALTH`, `HK_HEALTH_MAX`, `HK_SOUL`, `HK_FOCUS_LOCKED` | for the Focus expression |
| `HK_FK_DX`, `HK_FK_PHASE` | the False Knight's offset (`i32::MAX` with none) and what he is doing (1 winding up, 2 attacking, 3 armour rolling, 4 head exposed) |
| `HK_FK_X/Y/BODY/FLAGS`, `HK_WAVE` | his place, body box, flags (bit 0 armour refuses a nail, bit 1 rage) and the slam waves |

The idiom is `hold BUTTON until expr !$REACTION`: the step takes no time when the reaction is already false, so a script can carry a reaction at every station without slowing the run down. A tick of the pad turns the Knight toward a foe before a swing; a hit gives a recoil, so on a small platform strike overhead first (`guard` does).

`new-game-to-arena.walk` (now the first part of `new-game-to-crest.walk`, see "The False Knight and the City Crest" below; shipping disc, no cheat, no route-input; 21,596 ticks, 12 checks, the same ticks and verdict twice): the title, Start Game, an empty profile, the skip of the poem and movies, the drop into King's Pass, the corridor with its walls and Crawlers, the shaft up (twenty `leap`s with a `guard` after each, a `snipe` at the Fly over the narrow ledge), the Great Door (13 hits), Dirtmouth, the bench (save), the well, Crossroads_01, 07, 33, 08, then 13 (the pit, three Goams waited out with `HK_WALK_GOAM_LEFT/_RIGHT`), 42 (the stair of hops over the Goam pits), 19 (two hops and up out through the gap), 03 (floor, step, out at the left into 47), 47 (the bench: health back to 5, the save), 03 again (the left-hand shaft by its ledges, a Fly let come), 21 (down the slope, along the floor, out at the left) and 10 (up the ledges to the arena's right-hand mouth at x 64, y 27.4). It stops there. The fragments are `new-game`, `kings-corridor`, `kings-shaft`, `kings-pass`, `dirtmouth`, `crossroads-01`, `-07`, `-33`, `-08`, `-13`, `-42`, `-19`, `-03a`, `-47`, `-03b`, `-21`, `-10`. Not authored yet: the fight (the trigger is at x 13 on the arena floor; he drops, runs and slams; `HK_FK_DX`, `HK_FK_PHASE`, `HK_FK_EXPOSED`, `HK_FK_HEAD_HP` are published), the room below the broken floor, the Crest.

More macros (`walk.rs` header): `drop X Y` walks off a ledge and waits for the ground (a jump pressed in mid-air does nothing); every landing waits for `HK_WALK_READY` (on the ground, no hard-landing lock, no gate entry running); `at X Y` waits for it first (a gate entry owns the pad for its settle, fade and walk, so nothing pressed in it takes); `snipe` holds jump until `HK_WALK_JUMPSTRIKE` is 0 (a foe above the swing's reach, within a jump's) and swings at the top. More senses (`game/src/main.rs` `publish_walk_senses`, `props.rs`): `HK_WALK_TURN_LEFT/_RIGHT` (a foe in that swing's box and the Knight faces away: a script turns only then, so a ledge a unit wide keeps its Knight), the strike senses report only a foe the Knight can swing at now (not in a hit's recoil or freeze: a button held through those is never seen as a new press, which used to hang `guard` for its whole timeout), `HK_WALK_GOAM_LEFT/_RIGHT` (ticks left in the Down window of the nearest Goam on that side on the Knight's level: 0 hot, 255 none near; Goams are the walls of Crossroads_13 and 42).

The walk found the Cornifer latch (fixed in 8952aad): `mapper::cancel` latched `release` from a pad seen rooms ago and, with the module unloaded, nothing cleared it, so the Knight could not jump, swing or cast in Crossroads_19. `walk-new-game-to-arena` is its end-to-end regression: it jumps out of the entry corridor of 19 straight after the gate from 42.

Known fragility: three Vengeflies in the King's Pass shaft are the only thing that moves the run, and a hit on a small ledge can knock him off it. The script passes with 1 mask at the top and heals at the bench; a change that moves a Fly's first sighting by a few ticks can fail a leap's landing wait, which then names the ledge.

`earned-focus.walk` (`walk-earned-focus`) replaces the tape proof `cargo hk-build earned-focus` (red on main since 30de54f moved new games to the top-gate drop, which shifted every recorded tick): the same new game, the King's Pass corridor played with `fight` (no healing on the way), `check before` (4 hits, 2 kills, SOUL 44, a mask missing), Focus held until `HK_FOCUS_HEALED` is 1, `check after` (health back to 5, SOUL 11, no refund, no death, no cheat). 4,151 ticks on the shipping disc. It reads game state only, so it does not move when a load or a frame gets faster.

### The False Knight and the City Crest (2026-10-30 night)

`new-game-to-crest.walk` (`walk-new-game-to-crest`, shipping disc, no cheat, no route-input; 37,403 ticks, 15 checks, the same ticks and verdict twice) is the whole of the milestone: the walk to the arena (above), the fight, the fall into the room below, the final Head, the City Crest. `fk-from-bench-47.walk` (`walk-false-knight-from-bench-47`, 20,460 ticks) is the same from the bench save in Crossroads_47 on, for iterating on the fight in about two minutes of wall time instead of four: it boots `tools/cards/crossroads-47-bench.mcd` (a card taken by a `snapshot` step right after the bench in a run of the walk, then copied from the gate's `report/NAME-sd/cards/`), presses Continue and plays the rest. `card FILE loads N` in a walk script boots from `tools/cards/FILE` and takes N off every `HK_SCENE_LOADS` constant after it (a continued game counts its scene loads from the load of the save, 10 fewer than the run that made the card).

The fight is a round of reactions repeated (`include false-knight-round.walk times 2000`; the rounds after his death do nothing). Every reaction is a hold that ends on an expression, so it costs no time when there is nothing to react to, and every long hold also ends when a slam wave comes. The expressions (`false-knight-defs.walk`, `def NAME EXPRESSION` and `$NAME` in a script) are evaluated by the gate over raw words, not by the guest: the Knight's position, nail and facing, `HK_FK_DX` and `HK_FK_PHASE` (1 winding up, 2 in the air, 3 armour rolling, 4 Head open), and the raw words `HK_FK_X/Y/BODY/FLAGS` (his place, his body box, bit 0 armour refuses a nail, bit 1 rage) and `HK_WAVE` (two slots of x and direction) that `EnemyWorld::foe_senses` writes. Needs a gate with `kind = "expr"` (PSoXide-emulator 1d4f6d4e or later); `walk.rs` takes `hold X until expr EXPRESSION` and `wait until expr EXPRESSION`.

What the rounds do, in order: jump a slam wave when it is within 5 units (its first ticks are slow, so earlier is too early: a jump lasts about 45 ticks and a wave 7 units off takes about 40 to arrive; the jump held until the wave has passed clears the spurts), Focus when he is 17 units or more off, give ground while he winds up or is in the air within 14 units, stay at the far end in a rage, follow him down once the floor has broken, stand under the Head while the armour rolls or is open, jump to the Head when a swing cannot reach it (the Head is 0.05 above an up-slash from the floor), close on him when he stands idle out of reach, then swing (up, right, left; each only when the nail is ready and his body or Head is in the swing's box, never while his armour refuses a nail and never in a rage, so staggers equal conversions: 3 and 3). The measured run: 3 staggers, 3 conversions, 21 waves of which 3 hit (two in the first three, while he was closing on the Knight), jump attacks that landed on him now and then, so Focus (SOUL from the nail hits) brought him back to 5 masks each time; no death, 5 masks at the end, `HK_FK_DEATHS` 1 about 1,290 rounds in.

The room below: the Knight lands at the bottom (y 3.39) and finishes the Head there; Key Giver's shiny is at x 38 (`city-crest.walk`: walk to 37.9, up, `HK_PICKUPS_TAKEN` 1). `chest.walk` then takes the chest at x 6.7 y 12.8: back along the floor, a leap over the 3.9-high pillar at x 18.3 to 18.8, a `hop` onto the plate at x 12.9 to 14.4 (top 6.7) and a `hop` with `lead 0` up the ledge's face to its top (Knight height 12.39), then a swing from x 9.5 facing left (`HK_CHESTS_OPENED` 1); the walks end 16 checks (8 for the bench walk) after 37,812 and 20,801 ticks. Next section's recipe for a boss: publish the boss's position, box and a flag word, write the swing's box as a `def`, and copy `false-knight-round.walk`.
