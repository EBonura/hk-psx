# Seamless gates: spike results

Branch `spike/hk-seamless-2026-10-17`, not for landing. Written 2026-10-17 against main `15ec4f6` (library disc, SDK pin 355dc911, emulator pin b3848054). Every number below is measured on a headless replay of a disc built from this tree unless it says estimate. One VBlank (vb) is 1/60 s (the emulator runs 60 per emulated second; a PAL console would stretch every millisecond figure by a fifth).

Target (Manny, 2026-10-17): no black beyond the fade where we can manage it, and a pause under half a second as the hard ceiling.

## Recommendation

1. The load is already mostly hidden under the 20 vb exit fade for a gate whose next scene is fully prefetched: median 8 to 9 vb (about 140 ms) of black beyond the fade, and 22 of the 46 gates of the dwell-400 tour inside 11 vb. What spoils it is coverage: 28 of 96 measured gates read part or all of their scene at the gate (0.5 to 3 s), and those are all in a few families (below). Fix coverage first. It is cheap and measured.
2. Then move decode out of the gate. Decode ahead into unused VRAM pages (prototyped here) takes 70 to 130 ms off a gate where it applies and needs no cook change. It applies in full to 66 of the 122 gate edges between admitted scenes and to at least half of the destination's pages on 104 of 122.
3. Do not try to hold a second decoded scene in RAM yet. The numbers say it fits on 115 of 122 edges, but it is the largest change (a second arena slot, two live scene views), and after steps 1 and 2 it buys at most the last ~150 ms.
4. Three rooms need their own answer (False Knight arena, Shaman Temple, King's Pass): their resident scene leaves under 30 KB of arena slack, so nothing can be staged while the Knight is inside. Exit gates there stay at 0.6 to 1.5 s until the arena itself shrinks or the exit group is split (below).
5. Decision for Manny, cheap and large: the gate's black includes a fixed 19 vb (0.32 s) entry lead that the port copies from the source (settle plus fade delay in `great_door.rs`), counted after the load ends. As far as the port's reading of `EnterScene` goes, the source starts that lead once the scene is active. If the lead ran while our load finishes, a load of up to 19 vb beyond the fade would cost no extra black. The main loop starts the entry counter only after the load, so this means ticking it during the load; it departs from the source's sequencing, so it is your call.

Estimate, not measured: with steps 1 and 2 a fully covered gate has about 3 vb of black beyond the fade and 19 vb of entry lead, 0.37 s in total, under the ceiling. Without the lead overlap (decision 5) the uncovered tail stays above 0.5 s.

## Anatomy of a gate today (measured, `HK_GATE_LOG`)

Timeline of a gate whose scene was prefetched, from the median of 68 gates:

| Step | vb | Notes |
| --- | ---: | --- |
| Live frames while the exit fade starts | 6 to 7 | `frame::Exit`, the Knight walks out |
| Rest of the 20 vb fade, run during the load | 13 | `exit_fade.rs` (`EXIT_FADE_TICKS` 20, the source's 0.33 s) |
| Load beyond the fade | 8 | the part the Knight sees as black |
| Entry lead (black) | 19 (18 to 20) | `shade()` is 255 until settle plus fade delay pass |
| Fade in | 29 | |

Where the load goes on a fully prefetched gate (mean over 68 gates, ms): LZ4 256, upload 46, region binding 30, stored hash 12, coverage 10, display 8, metadata 7, everything else 11. That is 388 ms against the 217 ms the fade covers. CD time is 6 ms, because the bytes are already there. On a cold gate the mean is CD 538 to 633, LZ4 220 to 270, stored hash 84 to 110, and 344 ms of PREP when the room carries enemy code.

Averages across the measured gates: a scene group is 116 sectors (237 KB stored) and its resident decode is 107 KB for the median scene (367,628 B for the False Knight arena).

### Measured gates

Tour = `game/src/gate_tour_list.rs` (163 steps, Town through Crossroads and Greenpath), run through the real gate path on a new game. The replay driver stops at 6 billion instructions, so the runs cover steps 1 to 88 (cold) and 1 to 106 (best case); runs marked dwell 400 cover steps 1 to 46. "Excess" is load beyond the fade remainder; "total black" adds the 19 vb entry lead.

| Build | Gates | Load median (vb) | Excess median (ms) | Excess p90 (ms) | Total black within 0.5 s |
| --- | ---: | ---: | ---: | ---: | ---: |
| Cold: Knight stands, no prefetch | 87 | 66 | 883 | 1733 | 2 of 87 |
| Best-case prefetch (`prefetch-oracle`, dwell 150) | 105 | 23 | 167 | 1183 | 66 of 105 |
| Same, dwell 400, steps 1 to 46 | 46 | 25.5 | 200 | 1467 | 25 of 46 |
| Plus 10 KB extra arena tail (`HK_PREFETCH_EXTRA=10240`) | 46 | 25.5 | 200 | 1467 | 25 of 46 |
| Plus `vram-ahead` | 46 | 24 | 183 | 1467 | 26 of 46 |

The prefetch is what exists on main (proximity radius, `main.rs` `predict_gate`). The cold row shows what a gate costs without it; the best-case row feeds it the tour's next scene from the start of the dwell, which is what a Knight lingering near a gate gives it. A Knight sprinting through a room gets something between the two; that range is not measured.

### Why 28 of 96 gates are not covered

The prefetch lands in the arena tail the resident scene leaves free, so it holds `ARENA - resident(source)` bytes. In the best-case run, of the 26 distinct uncovered edges:

* Most are a few sectors short, not far (for example 165 of 169, 166 of 169, 142 of 146, 154 of 155). A missing tail costs a new seek, 15 to 30 vb, for 3 to 5 sectors. The 10 KB extra tail turned 4 of the 46 into fully covered gates (30 to 34) and cut the worst excess from 177 to 120 vb.
* Entering Fungus1_01, Fungus1_01b and Crossroads_11_alt from each other loses its prefetch to the room's code read (PREP 684 to 726 ms, prefix 2 to 5 sectors), excess 88 to 131 vb. The order of the code chunk read and the group prefetch needs work.
* King's Pass (resident 400 KB), the False Knight arena (370 KB) and the Shaman Temple (372 KB) leave 0 to 30 KB of slack, so their exits read at the gate (excess 25 to 88 vb), and the 400 KB bank of King's Pass does not fit under Dirtmouth either (Dirtmouth to King's Pass: 88 vb). The 388 KB group of Crossroads_10 does not fit under its neighbours (Crossroads_06 to Crossroads_10: 80 vb).

The arena is 410,612 B in the shipped cook (`SCENE_ARENA_BYTES` in `data/scene_manifest.rs`; the 354,032 B in `hk-gaps/GAPS.md` is an older figure). 17,132 B are free below the module pool in the ordinary build; the instrumented builds have less, and 16 KB of extra tail overflowed `.bss` by 4,276 B.

## What the next scene needs, and what exists to hold it

Facts from the cooked scenes (`.hkpsx/packed-scenes.json`, 60 admitted scenes, 122 gate edges between them):

| Resource | Capacity | Per scene (median / max) | What fits beside the current scene |
| --- | --- | --- | --- |
| Scene arena (decoded scene bank plus metadata) | 410,612 B | 107 KB / 367,628 B | both residents fit on 115 of 122 edges; the 7 that do not involve the False Knight arena, Shaman Temple, Tutorial_01 |
| Static VRAM pages | 19 (page 18 is the quick map's) | 9 / 19 | the whole destination fits on 66 of 122 edges; half or more on 104; none on 2. Mean fraction of destination pages that fit: 0.82 |
| Palettes | 1,248 | peak 386 | not the limit on any measured edge |
| Compressed group on disc | n/a | 237 KB mean, 388 KB max | needs the arena tail or a decode window |
| SPU one-shot bank | 16 to 56 KB free per scene (`docs/SCENE_SFX.md`) | small | the ambience prefetch already loads neighbours' stems; the bank is one chunk of the group |
| Module pool (enemy code and art) | 94,208 B | 80,972 B worst room | already streams on approach (`modules.rs`, `plan_code`) and is not the problem |
| Cross-scene shared pages | | | none: 0 of 544 atlas pages (32 KB each) have identical bytes in two scenes, so there is nothing to keep resident across a gate without a cook that shares pages inside an area (unmeasured upside) |

CD drive availability by room type (what a prefetch can count on):

| Room type | Drive state | Consequence |
| --- | --- | --- |
| Ordinary rooms | Shared with the area music refill: 16 sectors read from the CD interrupt whenever the 7.8 s FIFO drops, about 4 percent of 2x bandwidth plus its seeks; prefetch and clip reads queue behind a refill in flight | Prefetch has the drive about 90 percent of the time. A measured 171-sector group read takes about 0.9 s |
| Boss arenas during a fight | The fight's XA song holds the drive (`MUSIC.XA`; reading any other sector stops XA); the arena gates are closed | No reads at all, and no gate can be taken. Music is the only thing streaming, so the song can use the drive freely (Manny's decision). The exit scene can be prefetched only after the fight ends |
| Boss arenas after the fight | Drive free once the XA stops | The exit gate can prefetch, but these rooms are the ones with 0 to 30 KB of arena slack |
| Title, menus, save | The title owns the drive until its handoff | Not a gate case |
| Dialogue, shops, benches | Free; area stream keeps its small refills | Normal prefetch |

## Prototypes in this branch

### `vram-ahead` (game/src/disc.rs `ahead_step`, feature `vram-ahead`)

While the Knight is in scene A and a whole-group prefetch of B has landed in the arena tail, each present-loop idle slice decodes one of B's atlas chunks (one 32 KB page each, ~11 KB stored) into the gap between A's resident bytes and the tail, then uploads it to pages and palettes that A does not use. B takes the low end of the page range if A sits high, else the high end up to page 17. The gate then skips every atlas chunk already done. Placement is a per-scene shift on the manifest's bases (`scene_page_base` and `scene_palette_base` are now functions of a runtime shift; four call sites changed: `render.rs` twice, `vram_cache.rs`, `disc.rs`). Refused when the group is not whole in the tail, when the pages or palettes do not fit beside A, or when the largest chunk does not fit in the gap.

Measured (steps 1 to 46, dwell 400, 8 decoder steps per idle slice): 9 scenes were fully prepared ahead and 77 chunks decoded; 34 plans were refused (13 not a whole group, 20 pages, 1 gap, 0 palettes). On the 8 gates that found chunks waiting, load fell by 3 to 7 vb on 4 of them, by 2 on one, and by 0 or 1 on three that had other limits (over the 8, LZ4 274 to 201 ms and upload 49 to 27 ms on average). Median over all 46 gates: 12 to 11 vb of excess. It cannot do more because only 66 of 122 edges leave room for the whole page set, and the scene bank itself (not moved ahead) is most of what remains.

Cost: about 1 ms of CPU per frame while it works (the game is CPU-bound at 22 fps; a real implementation should spend slices only in genuine idle time); no extra RAM; no cook change. Risk found while building it: a host test extracts `Cache::admit_scenes` into a probe harness, so hooks must live in `select`, not inside it.

### Decoder slice (`HK_DECODE_STEP`)

`DECODE_STEP` is 1,024 byte operations per pad checkpoint because 4,096 once carried a poll over a VBlank in the False Knight journey. With 8,192 on the best-case mini tour (7 gates, no input faults in this run) LZ4 per gate fell from 340 to 208 ms on a covered gate and load by 2 to 3 vb. It is the cheapest single lever and also the one with a known failure; it needs the routes re-run, not a decision from this tour.

### Frame captures of three Crossroads gates

Crossroads_05 to 07, 07 to 33 and 33 to 07, captured every 4 ticks. Dark span (frames below 4 percent luminance, which includes the fade-in's first frames, so it reads about 0.4 s long on a covered gate):

| Gate | Cold | Best-case prefetch | Plus 10 KB tail |
| --- | ---: | ---: | ---: |
| Crossroads_05 to Crossroads_07 | 152 ticks | 108 | 76 |
| Crossroads_07 to Crossroads_33 | 64 | 24 | 28 |
| Crossroads_33 to Crossroads_07 | 132 | 60 | 56 |

The contact sheets are `runs/gate1-sheet.png`, `gate2-sheet.png`, `gate3-sheet.png` in the lane directory (`/Users/ebonura/Desktop/repos/campaign-2026-10-15/hk-catalog/runs/`); game frames stay out of the repository. The gate logs for the same runs show load 33 and 32 vb without the tail and 29 and 29 with it on the two partially covered gates.

## What a split arena would need (not built)

A second resident slot needs: `SceneDesc.ram_offset` per slot (it is 0 for every scene today), a second `Scene` view alive during gameplay (views are slices, so this is plausible), a second metadata bank, and the renderer's per-scene tables (`TEMPLATES`, covers, `ACTIVE_BANK`) built for the incoming scene without disturbing the current one. RAM fits on 115 of 122 edges by the resident sizes alone, but not with the compressed group and the decode window present at the same time, so in practice it pairs with a streaming read of the scene chunk rather than a whole-group prefetch. Estimated gain after the steps above: the remaining ~150 ms (scene decode, validation, region binding). Not recommended before the earlier steps are measured in play.

## Open and unmeasured

* A walking Knight's prefetch coverage (the tour stands still or is fed the answer). The proximity radius (`PREFETCH_RADIUS`) and its lead time need a measured walk per gate.
* Gates 107 to 163 of the tour (Greenpath beyond Fungus1_19) and all gates in the 441 unadmitted scenes.
* Hardware: no console timing for any of this.
* Whether a PAL console makes the fade and lead longer in step with the load or only the load.

## Reproduce

Needs the lane setup of `campaign-2026-10-15/hk-catalog` (seeded cook, `HK_OVERLAY_SDK`, the shared frontend). Instrumented builds end with the budget-doc test failing, because the features add code; the disc and `.hkpsx/build.json` are already written by then (`HK_PREFETCH_EXTRA` builds also fail `build_report.py`, so `build.json` has to be pointed at the new disc by hand for `replay_cue.py`).

```sh
# Tape: new game, then idle (tools/validate.py poll_tape from the repo root's tools/):
#   events 9:start:1,12:cross:1,150:start:1,153:up:1,156:up:1,159:cross:1,162:right:1,168:circle:1,172:circle:1, 70000 polls
HK_TOUR_DWELL=400 HK_GUEST_FEATURES=prefetch-oracle cargo hk-build build --no-validate --library LIB ...
tools/replay_cue.py --tape tour.pxtape --output OUT --screenshot-interval 100000 --frontend ... --cue LIB/hk-psx.cue
# OUT/ram.bin: HK_GATE_LOG (32 words per gate: [3] trigger vblank, [4] load start, [5] display on, [28] fade-in start,
# [29]/[30] prefetched and total sectors, [11+phase-1] phase time in HBlanks of 63.6 us), HK_GATE_COUNT, HK_AHEAD_*.
```

Features: `gate-tour` (scripted tour and phase timing), `prefetch-oracle` (feed the tour's next scene to the prefetch), `vram-ahead`. Env: `HK_TOUR_DWELL`, `HK_PREFETCH_EXTRA`, `HK_AHEAD_STEPS`, `HK_DECODE_STEP`.
