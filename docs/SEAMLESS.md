# Seamless gates

How the port keeps a doorway (a scene gate, `scene_transition::load`) short, what each piece costs, and what is still above half a second. Target (Manny, 2026-10-17 and 10-25): no loading screens, a pause under 0.5 s, no pop-in (a gate ends only with its room's art).

"Black" below is vblanks (1/60 s) from the end of the 20 vb exit fade (`exit_fade.rs`) to the first lit frame, which includes the entry lead (19 vb, `great_door.rs`) that now runs during the load instead of after it. Half a second is 30 vb. A load has about 41 vb (0.68 s) from its start before black passes 30.

## What a gate does

Group `1+s` of WORLD.PAK is everything a gate reads for scene `s` (code chunk, one-shot bank, coverage, effect art, atlases, scene, metadata), in the order `admit_scenes` consumes it. The scene arena (`SCENE_ARENA_BYTES`, whole sectors) holds the resident scene and its metadata bank; whatever it leaves free (the slack) is where the group of the gate the Knight approaches is read ahead, as a prefix of its sectors, while he walks. The gate reads the rest (one read, parked on the next sector), decodes each chunk as its sectors land, and hides the end of that work under the entry lead.

* `main.rs predict_gate` names the likely gate every fourth frame; `PREFETCH_RADIUS` (16 units, about 2 s of running) starts the read. A room whose slack is at most 24 sectors uses `TIGHT_RADIUS` (60), because its prefix can only be built ahead in spare VRAM pages (below).
* Code of the next room is read into the module pool first (`modules::plan_code`); the runner audio bank of the other family rides below the prefix (`plan_bank`); a partial prefix leaves the drive parked on the first sector it lacks (`maybe_park`), so the gate's read pays a 12 ms seek instead of the 85 to 335 ms of the silicon seek curve.
* The prefetched chunks are hashed in idle time (`preverify_step`), so the gate skips their stored-hash pass.
* `DECODE_STEP` is 8,192 byte operations between pad checkpoints (1,024 before): the checkpoints were a fifth of the decode. 16,384 saved more but lost pad polls in journey-kings and journey-crossroads.
* Spare-page spill (`Spill`, `vram_cache::spill_read`): a room whose slack is at most 24 sectors cannot stage much under its own scene. Its prefix then keeps its first sectors in the static VRAM pages the resident scene does not use (16 sectors a page, page 18 is the quick map's), read a few sectors at a time through the slack or through eight sectors the module pool lends (`modules::claim_top`, taken back by any allocation that has nowhere else to go), uploaded when the GPU is idle, summed per page, and read back at the gate with GP0(C0) into the group's place. A page whose sum no longer matches is read from the disc instead. `HK_NO_SPILL` turns it off; `HK_SPILL_*` counters and word 22 of the gate record report it.
* Groups too large for the arena stage in two windows (`split_fits`). The metadata tail of the arena holds the last sector's padding as well as the in-place margin, and the arena is whole sectors (`pack_scenes.scene_arena_bytes`); without that Tutorial_01's group did not fit the second window and was read as 19 separate chunk reads (181 vb of black).

## Measured (walk tour, steps 0 to 101 of `game/src/gate_tour_list.rs`, emulator pin b3848054)

The tour (`--features gate-tour,tour-walk`, `HK_GATE_LOG`) walks the Knight straight from each entry to the next gate at running speed, ignoring terrain, so the predictor sees the shortest realistic lead (43 of the 164 steps are returns through the gate just used). Read the log with `tools/gatetool`. Step 102 (Crossroads_47 to Crossroads_03) never completes on any build and stops the run.

| Build | gates within 0.5 s | black median / p90 / max | sum of black over 30 vb |
| --- | ---: | ---: | ---: |
| main 7226e94 behaviour (study knobs off) | 33 of 102 | 37 / 97 / 196 | 2,373 |
| prefetch work of the 10-17 branch on main | 73 | 24 / 59 / 181 | 887 |
| + arena padding fix, spill, radius 16, decode step 8,192 (this code, rebased onto main 9c48f99, tour of 10-26) | 76 | 23 / 50 / 146 | 630 |

Run to run the same gate moves by 20 vb or more when anything else in the build changes, so compare sums, not single gates. A real play check: Manny's 10-24 16:16 tape replayed on this build (gate-log, route-input) drifts and crosses two doorways, King's Pass to Dirtmouth at 46 vb (92 on main before this work) and Dirtmouth to Crossroads_01 at 16; on 10-24 the same tape crossed five, four of them 20 to 22 vb.

## Still above 0.5 s

(26 of the 102 tour gates, from the 10-26 tour of the rebased code; the tour build itself missed 10 pad vblanks in 74,000 polls, the shipping twin passes every route and journey check.)

| Gate | black (vb) | Why |
| --- | ---: | --- |
| Tutorial_01 to Town (every new game) | 38 to 46 | Town's group is 172 sectors; King's Pass leaves no slack (341,532 B scene and 73,516 B metadata in the arena), 83 sectors are spilled, the rest is CPU: stored hash 218 ms, LZ4 290, upload 90 |
| Town to Tutorial_01 | 69 | scene and metadata (window B, 99 sectors) are read at the gate, 0.6 s of CD then 0.3 s of LZ4 |
| Crossroads_06 or 21 to Crossroads_10 (False Knight arena) | 84 and 146 | window B is the 100-sector scene chunk, which holds the boss's animation frames (220,924 B of its 367,628 B); 0.75 s of CD, 0.3 s of LZ4, and on the second a 0.7 s ambience clip read |
| Crossroads_06 to Crossroads_ShamanTemple | 73 | window B again (0.56 s of CD) |
| Fungus1_01 to 01b or Crossroads_11_alt | 70 and 91 | a return through the gate just used: the runner bank read in flight at the trigger is waited for (`bank_settle`, 0.3 s) and the group has not started |
| Crossroads_08, 19, 50, 4, 27, 9 and a few others | 42 to 72 | slack 0 to 40 sectors of 116 to 167, spill pages and arrival time limit the prefix |

Measured on the way and not kept: a prefetch radius of 32 (735 against 817 vblanks of black over 30 on the tour) or 64 (644, once ended a gate without its room's art); from 24 up, hornet-road lost 13 pad polls and a music refill while Fungus1_21's gate loaded, which 16 does not. Scene groups laid out by gate adjacency (annealed over the silicon seek curve), a head park for groups with no slack, and a trusted metadata parse changed the sum of black over 30 vb by less than the run-to-run noise (patch kept in the lane notes).

## What would move the rest

* False Knight arena: take the boss's art out of the scene bank. A lazy boss bank, like Hornet's, would cut the scene chunk by 40 to 50 percent and the arena's gate decode with it. This is a cook and guest change (`host/false_knight_art.py`, `fk_art.rs`), not a tuning one.
* King's Pass: its refs (101,860 B) and draws (135,652 B) are the bulk of the resident scene; delta-coded reference lists would free about 45 KB of arena, which is also what the arena as a whole (sized by this room) costs every other room.
* The stored hash is a byte-serial FNV (11 cycles a byte, 100 to 240 ms a gate that is not preverified); a word-wise hash is a cook-wide change.

## Reproduce

```sh
HK_GUEST_FEATURES=route-input,tour-walk cargo hk-build build --no-validate --library LIB ...
# tape: python3 -c "import tools.validate as v; v.poll_tape(Path('tour.pxtape'), '9:start:1,12:cross:1,12:l1:18,12:r1:18,12:l2:18,12:r2:18,40:start:1,43:up:1,46:up:1,49:cross:1,52:right:1,58:circle:1,62:circle:1', 220000)"
frontend launch --path LIB/'Hollow Knight (hk-psx).cue' --embedded-playtest --input-tape tour.pxtape --stop-at-poll 74000 --steps 400000000000 --dump-ram ram.bin ...
cargo run --release --manifest-path tools/gatetool/Cargo.toml -- scene-names.tsv run=ram.bin:LIB/game.map
```

Instrumented builds need `HK_GATE_RECORDS` small enough for BSS, and end with the budget-doc test failing because the features add code (the disc is written first). `HK_NO_LEAD_OVERLAP`, `HK_NO_BANK_AHEAD`, `HK_NO_PARK`, `HK_OLD_PREDICT`, `HK_NO_GATE_SLICES`, `HK_NO_SPILL` switch the pieces off for A/B builds.
