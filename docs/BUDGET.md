# Shared scene and gameplay budget

The current ordinary build keeps one scene at a time in the RAM
arena and replaces its static VRAM atlases at scene gates. These are linked reservations, not hardware
performance or main-stack high-water measurements. Exact map and artifact hashes
are in `.hkpsx/build.json`; every build replaces the sole playable disc.

| Main RAM allocation | Bytes |
| --- | ---: |
| Linked code |791,520|
| Code-to-data alignment |32|
| Linked data, including menu and HUD |282,184|
| BSS, including shared scene arena and runtime pools |792,044|
| Total static span |1,865,788|
| BIOS/kernel reservation |65,536|
| Linker stack exclusion |49,152|
| Space above initial stack pointer |256|
| Unallocated gap before reserved stack |116,420|
| Room module pool, inside that gap |108,544|
| Free RAM below the module pool |7,876|

The 2026-10-16 no-64-bit pass (every `i64`/`u64`/`i128` in guest source moved onto `psx_math::Wide`, `WideU` and `Mask64`, and the shared `hk_sim::exact` divide helpers replaced by the SDK's `wide_div` ops) took linked code from 809,856 to 793,876 bytes, linked data from 278,792 to 278,568 and BSS from 797,612 to 797,588, so 17,948 bytes are free below the module pool (1,732 before).

The 2026-10-17 frame-time pass (the early back pass skips a frame whose Knight art would upload, and every checkpoint takes a CD event the interrupt wrapper missed) took linked code from 793,876 to 794,696 bytes (820), so 17,132 bytes are free below the module pool.

The 2026-10-17 FSM v2 landing (HKFSM03 kernel, bank format and family code; Crawler, Gruzzer, Vengefly and the scene scripts on it, the v1 executor and the HKSCR01 interpreter gone) left the resident image where it was: linked code 794,696 to 795,812 (+1,116), data 278,568 to 278,280 (-288), BSS 797,588 to 796,908 (-680), so 17,024 bytes were free below the old pool. The worst room (Crossroads: Vengefly, Aspid, Climber and Crawler with the grub jar art, plus the 30,388-byte carried Shade art) needed 96,080 pool bytes against a 90,112-byte limit (the pool less its 4 KiB window fence), because the shared action code (`fsm3_logic` 3,604, `fsm3_world` 1,068 and `fsm3_body` 7,884 bytes, carried per room by the families its banks use) is paid by every room that has a bank. The pool is 100 KiB now (102,400), which leaves 8,788 bytes free below it and 2,224 bytes of margin in that room (98,304 limit). `hk-module-publish` prints the six tightest rooms on every build ("pool fit").

The 2026-10-17 restore of the lost features (Crossroads_22's waves arena with its 64-quad draw list and arena runtime, the Knight's rising Focus motes, the death reset of the soul totems, the Mosquito's recoil speed), rebased on the FSM v2 family-mask build, took linked code from 795,812 to 801,668 bytes (5,856) and linked data from 278,280 to 280,264 (1,984, mostly the arena's cooked tables and the motes' style), so 932 bytes are free below the module pool (8,788 before).

The 2026-10-17 `-Cpanic=immediate-abort` build (host/build_guest.py; `PSOXIDE_PANIC_MESSAGES=1` restores the formatted panic) turned every panic, bounds check and overflow check into a bare `break`. `core::fmt`, the panic `Location` records, their file names and the assertion message strings left the image: linked data fell from 280,264 to 255,192 bytes (25,072) while linked code grew from 801,668 to 802,240 (572, the compiler inlines differently once the panic calls are gone), so 25,444 bytes are free below the module pool (932 before). psx-rt's exception handler still records Cause, EPC and BadVAddr (`__psx_rt_fault_*`), which every route already fails on. Each module image now reserves 96 hazard-trampoline words (SDK `overlay_pack::TRAMPOLINE_WORDS`, `code_modules.TRAMP_WORDS`) instead of 32.

The generic native components and the road to Hornet's arena (Fungus1_22, Fungus1_21 and Fungus1_04: 70 more views, with the region and atlas index tables narrowed to two bytes to pay for them, and the Cloak pickup) on top of the abort build left 29,620 bytes free below the module pool (25,444 before): linked code 802,240 to 796,372, linked data 255,192 to 256,968, BSS 796,924 to 796,828. Four cold functions (the save write, the menu, the panel text, the render init) are built for size. `optimize(size)` on `disc.rs`'s prepare functions did nothing, and on `world.rs`'s `reset_scene`, `break_effects::load_scene` and `Actor::new` it made the image 1.4 KB bigger (inlining changes), so those stay as they were. The boss tier of the FSM kernel and Hornet's fight are in the source and out of the build (`HK_HORNET_ART`, docs/HORNET.md).

The 2026-10-19 console-fix pass (the bench Save notice, card errors and retries, the card-manager title and icon, the Great Door's per-hit reaction and the break shake; game/src/save_ui.rs, docs/SAVE.md) took linked code from 797,492 to 805,384 bytes (7,892) and linked data from 257,256 to 257,800, so 19,740 bytes are free below the module pool (28,212 before). The two `card-diag` features (card_diag.rs) are not in the ordinary build.

The 2026-10-19 polish pass (the original dialogue box, a crisper font and Geo coin, exact title-card glyphs, and the new game's drop in from King's Pass's top gate) took 6,888 bytes of the 60,260 that were free below the module pool, leaving 53,372: linked code 758,912 to 762,160 (+3,248: the ornate panel's layout and backboard, the animation-cache branch for its tiles, the title card's two glyph paths, the new-game entry), linked data 263,432 to 266,456 (+3,024: 3,568 bytes of dialogue ornaments and their 56-byte tile table, less the title card's glyph arrays halved from 48 to 24 and the removed Geo coin quad), BSS +616. The ornaments (`data/dialogue-art.hk`, host/hk-cook/src/dialogue_art.rs) are the whole of the data cost; they reach VRAM through the animation cache's 64x64 slots on demand, so VRAM, which has 304 spare halfwords, pays nothing. The Geo coin lives in a spare cell of the font strip (boot art chunk, no RAM) and replaced `geo_render::hud_coin`. The opening-movie player is a blob in the boot art chunk (intro/), so it costs no resident data, and the shipping disc carries it (docs/INTRO.md): its resident side is the hand-off (`intro::launch`, 344 bytes, and `take`, 152, with the eight-word mailbox) and the resume path in `main`, offset by `#[inline(never)]` on `audio::set_volume`, `music::set_volume` and `music::apply` (LLVM inlined them into `main` or not as the code around them moved; one copy each is what the title menu and the resume share). The shipping disc with the movies has 12,648 bytes free below the pool; main before this change had 12,480.

The 2026-10-22 menus pass (the original crest and flourishes on the pause pages and the charm board, a dimmed view and a glow behind the chosen row, the title pointer and flourishes) took 6,576 bytes of the 53,372 that were free below the module pool, leaving 46,796: linked code 762,160 to 763,680 (+1,520), linked data 266,456 to 271,512 (+5,056: the dialogue art blob grows from 3,568 to 8,592 bytes for 12 tiles, the pause crest and flourishes among them). The title ornaments (3,576 bytes) ride the boot art chunk and cost no RAM.

The 2026-10-22 font and charm board pass (the panel font rasterised unhinted at 16 coverage levels and drawn adding, so its strip, advances table and boot chunk stay 7,712 and 95 bytes; the charm board as the source's grid of forty slots with the equipped row and the notch circles, its icons in three 64x64 sheets instead of six slots a page, and the Options and Select Game pages on their own panel) took 1,540 bytes of the 46,796 that were free below the module pool, leaving 45,256: linked code 763,680 to 763,224 (-456), linked data 271,512 to 273,496 (+1,984: the board's 51 icon quads, which are initialised data, replace six, and the dialogue art blob grows from 8,592 to 8,892 bytes for the slot ring and two notch circles), BSS +4. The charm icons stay 5,248 bytes (four palettes of 32 and forty 16x16 icons, now packed sixteen to a sheet).

The 2026-10-22 14 px font pass (Perpetua at 14 px grid fitted, the TrajanPro-Bold capitals for the menu buttons, headings and labels, every menu and panel laid out again for the new sizes) took 2,752 bytes of the 45,256 that were free below the module pool, leaving 42,504: linked code 763,224 to 765,152 (+1,928: the capitals' draw, the cheats window, the larger layout code), linked data 273,496 to 274,328 (+832: the capitals' advances and the layout constants, the charm icon quads and art unchanged), BSS +0. The font image is 12,832 bytes (palette, then 256 x 100 texels: Perpetua's 95 glyphs in 13x14 cells, the capitals and the Geo coin in 13x12 cells) and rides the boot art chunk into VRAM, so it costs no RAM. It sits at x320, y360 (sampled from the texture page at y256, texel rows 104 to 203), which is the third CLUT bank's region: the cooked scenes need 386 of the first bank's 416 palettes (SCENE_TOTAL_PALETTES) and their tightest view 248, so no scene palette moves and none is lost. What the move does cost is the theoretical capacity: scenes are admitted into the first two banks (832 palettes, `SCENE_CLUTS`, pack_scenes.py `MAX_PALETTES`) instead of the first three (1,248; the cooked peak was 569 in an earlier build). Rows 500 to 511 (320 halfwords wide) are free again.

The 2026-10-22 round 5 menus pass (no hint rows anywhere, the conversation box for Cornifer, the Snail Shaman and the bench's notices, the charm board laid out as the source's pane with a purple backdrop, the soul vessel's glow, a right-hand column and a 2x picture, area cards in 40 px Perpetua) moved the free RAM below the module pool from 42,488 (the same tree at the 14 px commit) to 43,248 (the shipping disc; the route-input test disc used for the gates has 56 bytes less): linked code 765,152 to 767,532, linked data 274,344 to 273,160, BSS 797,816 to 795,856. The gains: the glyph packet table is 360 entries instead of 416 (the worst screen, the charm board, counts 357 with spaces; a tablet page is at most 161), the area cards draw through that table instead of their own rectangle and quad arrays, and a charm's description is one newline-joined string instead of a pointer a line. The costs: the card strip (`data/card-font.hk`, 7,680 bytes, boot art chunk, VRAM rows 500 to 511 which were free, no scene palette), a 24x24 orb tile in the dialogue art (9,180 bytes now), the cards' band table (472 bytes of data) and the ornate box, backdrop and board code.

Since rooms stream their enemy code and art (host/code_modules.py), the room
module pool is carved from the top of the gap below the stack, so the free
figure to plan against is the gap less the pool. The 2026-10-02 merge of the camera, enemies, parity, visual and streaming
branches overflowed RAM by 2,944 bytes against a 96 KiB pool; the pool is
92 KiB now (the worst room plus the carried Shade art needs 80,972 bytes),
which left 1,152 bytes below it. Reading the renderer's scenery draws in
place from the scene bank instead of copying them (2026-10-03) returned
16,788 bytes. The playtest fixes (2026-10-03: a vignette factor per draw,
1,408 bytes of data, the stalactite batting and the check) took 4,100, and the soft
Break Dust (palette copies with the semi-transparency bit, drawn in its
authored colours) took 4,104 more, mostly code that crossed an alignment step,
which left 9,736. A broken stalactite's flung rocks, dust and sounds
(2026-10-04, the rocks flying in the Geo coin pool) took 2,072 of those, one
alignment step: 7,664 were free below the pool then. Playing the title and the fights as
XA songs through the player the game carries (game/src/xa_player.rs, the directory
lookup of MUSIC.XA in disc.rs, 2026-10-04) took 2,064 of them: 5,600 are free below the
pool now. The pad driver of the SDK pin that holds the analog request and the
ACK-paced poll (psx-pad, PadReader and require_analog_port1 at boot,
2026-10-04) grew the static span by 4,096 bytes (2,608 of them the driver
itself) and would have left 1,504 below the pool; compiling menu::run, save::write and render::init for size, none of
which runs in a gameplay frame, took it back, so the figure is unchanged. Brightness and the screen
position on the title's Options page (game/src/display.rs, 2026-10-05) took 12 of them, with
`menu::draw_menu` compiled for size too (it grew by 1,512 bytes with three more rows before that) and
the hero light's clip at the Knight's feet, which a platform showed to be wrong, gone: 5,588 are free
below the pool.

The per-tick `HK_TRACE` block for the replay harness (game/src/trace.rs, 2026-10-08) took 512 bytes of
BSS and 1,260 of code, and compiling seventeen functions that only run on a scene change,
a gate, a save or a debug reset for size (`optimize(size)`: the disc cache's init and
prepare_*, enemy sync_region, persist, the save survey, take_gate and the like) gave back
8.2 KB; the Focus effects (23 frames in the ability art, the Burst at half resolution, the focus_fx module) took 12.3 KB of that back, and 924 were free below the pool.
Three changes since returned 8.7 KB without touching a pixel. The `HK_TRACE` block is built only
with the `trace` feature (game/Cargo.toml), which no shipped disc enables: 2,560 bytes. The ability
art is run-length coded per frame and decoded straight into the GPU port when the animation cache
uploads a frame (game/src/ability_art.rs `upload_frame`, no staging buffer): its payload went from
42,600 to 33,729 bytes while the Focus Burst went from half to full resolution (5,424 bytes of
texels raw, 2,520 coded) and the Burst's two bright frames took a palette of their own in the
ability block's spare CLUT row. The Knight's Focus dust (Dust L and Dust R cooked into every
scene, raised every six ticks while Focus runs) cost a style and an emitter row of room in the
effect tables (styles 18, emitters 100) and a few words of code: 9,620 were free below the pool at that stage (with the 50 Hz input phase merged in). The HUD's Geo coin replaced the word "Geo"
(three glyphs), and the render policy and the audio start added a few words.

The allocation table above is the current ordinary build's, read out of
`.hkpsx/build-normal.json`, and `tests/test_budget_doc.py` fails when this table
drifts from it. `pgo` writes `.hkpsx/build-pgo-normal.json` instead, because its
code size follows the profile; `.hkpsx/build.json` is whichever image is on the
disc. The table stood
at 113,636 bytes of headroom for long enough that it was quoted in three
separate agent briefings after it had fallen to 73,876. A budget
document nobody can trust is worse than none, because work gets planned against
it.

The scene arena is sized by each scene's own world metadata bank rather than
the widest one (host/pack_scenes.py `scene_arena_bytes`): Crossroads_10, the
largest resident scene since the False Knight, had been paying for
Tutorial_01's 69,640-byte bank behind it. That took the arena from 428,572
back to 394,944 and is most of the move from 116,504 bytes free (f2d79f2, per
its message) to 141,864.

Four branches landed on 5c3da2d's 139,816, each measured on its own against
it: fix/slice-gaps-3 (soul totems, title cards, pickups) 20,128, the quick
map, Cornifer, the Snail Shaman's spell and the Elder Baldur's rollers
(feat/map-and-spell) 32,404, the Brooding Mawlek (feat/mawlek,
docs/MAWLEK.md) 28,808, and the Husk Guard (5b50498) 10,240. Merged at
42ac0b1 the gap is 44,156, 4,080 below what those figures add up to; that
remainder has not been attributed.

The boss title cards, the Mawlek's roar lock and its arena's mask shard
(feat/boss-polish) cost 8 bytes net on 1766337's 44,156: code grew by 608
and linked data shrank by the same 608, with 8 more of BSS.

The pickup polish (fix/slice-polish: chest and pickup sounds, the kneel,
the chest lid and fling, enemy hit sparks) took 3262273's 44,148 to 33,156,
10,992 bytes: code 2,736, linked data 1,360 and BSS 6,896. Of the BSS,
6,880 is the scene arena, which Tutorial_01 sets: its chest's Open clip
and one held kneeling frame ride its bank (host/pickups.py LEVEL_CAP).

The secrets (feat/secrets, docs/SECRETS.md) took 4736b2e's 33,156 to 28,948,
4,208 bytes: code 18,324, BSS 112, alignment -4 and linked data -14,224. The
data fell because the glyph sheet, Geo and Lifeblood art moved out of the
executable into the title art chunk the boot already reads from disc
(399e989, 17,774 bytes). The scene arena grew 3,908 for Tutorial_01's secret
metadata.

The 2026-10-19 pass (one terrain reader type for every solver, so Player::step and the Actor::advance_* family compile once; the route lock and its 256-byte load log) took linked code from 805,384 to 754,836 bytes, data from 257,800 to 258,536 and BSS from 796,868 to 797,132, so 69,284 bytes are free below the module pool (19,740 before).

The 2026-10-23 scenery trimming policy (`trim_policy::verdict`, a three-frame count where one slow frame used to decide) moved the shipping disc by +132 bytes of linked code, +4 of BSS and 12 of code-to-data alignment: free below the pool 53,372 to 53,224.

The 2026-10-23 round 6 menus pass and its follow-ups (menu animations after the original's data: page sequences, flourish unfurl, cursor leaf opening, dialogue box fade and typewriter, charm board fade and icon flight, area card timing; the Cornifer price as coin and number; "Game saved" in the Saving box; then the NPCs inert under the first-visit area title, the Listen prompt's flourish, the title screen's particles and the overcharm damage doubling) leave 43,156 bytes free below the module pool, against the 43,248 of round 5 (the trimming policy above took 148 of its own on the way): linked code 766,644, linked data 274,072, BSS 795,916. The glyph routine `text_gain` is no longer inlined at each of the panel's text calls, which returned roughly 8 KB of code and paid for the animations; the title particles, the prompt marker and the page sequences are built out of line (`#[inline(never)]`, the particles for size) so they do not grow their callers. The pointer's five opening frames, the smoke frames and the speck (7,192 bytes in all) ride the boot art chunk and cost no RAM; the prompt flourish is 612 bytes of the dialogue art, whose cooker limit rose from 9,216 to 16,384 bytes.

The 2026-10-23 replay on emulator 0e87b3a (texture-cache cost accuracy) added one guard to the room-module loader: `flush_instruction_cache_idle` waits for the GPU channel to idle before the instruction cache is flushed, because a flush while DMA2 was still walking the previous frame's list is a hazard on silicon. The helper is kept out of line; inlined it took about 1,600 bytes more, out of line it takes 288 (linked code 766,652 to 766,936, free below the module pool 43,156 to 42,868).

The 2026-10-23 object state pass (cut grass and broken stones stay changed in every view that draws or holds them: one `KIND_REMOTE_ART` stream per region, `State::apply`, `fill_edges` and `broken_edge` following the scene table; see `docs/STATE_IDENTITY.md`) moved the shipping disc by +2,764 bytes of linked code, +48 of data and +4,440 of BSS, all of it the scene arena (the 4,294 bindings of the whole game, 4 bytes each plus 2 per draw or edge index, peak scene 60): free below the pool 36,860 to 29,620.

The 2026-10-24 round 7 and 8 title passes (the style's own background behind every title page and the logo as a piece of its own; then all ten styles of the original with their colour grades, particles and animated sprites, the unlock rules and a profile record on the memory card, and the Cheats page's "Unlock all menu styles" action; docs/MENU_STYLES.md, docs/MENUS.md) leave 24,764 bytes free below the module pool against 29,620 before (-4,856): linked code 770,496 to 774,964 (+4,468), linked data 279,304 to 279,672 (+368: the ten style names and the style table), BSS +8, code-to-data alignment +12. The code is the one cost of the system: the particle and layer player (`extras`, `emitter`, `layer`, `piece`) about 2.7 KB, the style read and upload (`Cache::stage_menu`, `upload_style`, the song's suspend and resume) about 1.3 KB, the selector, the Cheats row and the profile record (`save::unlock_styles`, its read in `survey`) about 0.9 KB, less the Voidheart-only particle code the player replaces (1.5 KB). The pictures (89,712 bytes each), logos, particle tracks and flipbooks are disc data: the boot art chunk is 1,230,848 bytes (248,420 before), of which the first 163,840 are read at boot (the title starts sooner than with the 248,420-byte read); a style other than Modern is read from its own sectors when the options page picks it, into the scene arena after the common part. Video memory: the extras use x 768 to 959, rows 0 to 255 and a palette row at (768, 479), title only. One way to give the code back: the title (menu.rs, about 18 KB) as an overlay module in the room pool, which is idle at the title.

The 2026-10-24 death pass (docs/DEATH.md: the Knight's Death clip, the dark slashes every hit throws, the fade to black and the bench or ground respawn) moved the free RAM below the module pool from 23,756 to 4,660: linked code 775,664 to 781,628 (the slash burst, the death pose and fade, the bench's respawn phases), linked data 279,960 to 293,080 (61 more ability frames, 10,584 run-length coded bytes in the existing five palette rows, and their table rows), BSS 800,420 to 800,428.

The 2026-10-24 texture-placement pass (scene atlas textures placed by draw order, so consecutive draws share a texture page; host/scene_bank.py `ordered_pack`) moved the free RAM below the module pool from 4,660 to 2,804: linked data 293,080 to 294,936 (+1,856) and nothing else. The placement may add up to four atlas pages to a scene when that removes page switches, and each added page is one 32-byte `AtlasDesc` row in `data/scene_manifest.rs` (693 atlas rows against 635, 58 added). The scene arena stays 415,052 bytes.

The 2026-10-24 streamed death art pass (docs/DEATH.md, "Streamed art") moved the Knight's death and respawn art out of linked data: the Death clip (18 frames) and the respawn clips (32 frames) are now two carried pool packages, `art_death` (5,862 bytes) and `art_wake` (5,510), read from the disc when a moment needs them. Linked data fell by 11,264 (294,936 to 283,672 on the draw-order texture packing base: the frames, and their table rows for the 50 streamed frames) while linked code rose from 781,628 to 783,160 (+1,532: the frame lookup, the carry rule and the boss song's wait for it), so 12,480 bytes are free below the module pool (2,804 before; 23,756 before the death pass and the texture packing). What stays linked is the death pass's code (+5,964 over 23,756) and its 11 slash frames and clip tables.

The 2026-10-25 ability effects pass (docs/ABILITY_EFFECTS.md: the Knight's dash burst, wall puff, double jump wings and flash, Vengeful Spirit's blast and impact, and the dash's and the wall slide's dust) took 3,640 of the 12,648 bytes that were free below the module pool, leaving 9,008: linked code 782,988 to 785,684 (+2,696: `ability_fx.rs` 2,850 bytes with its state, the carry and frame calls in `frame.rs`, `modules::carry` and `ability_art::ready` out of line, and one shared draw body for the three ability draws, which were two bodies before), linked data 283,688 to 284,552 (+864: the two new break-effect curves), BSS +72. The art is a carried pack of its own, `art_fx` (5,890 bytes, three sectors), which does not grow the pool-fit sum (the Shade's package and two of three sectors); no palette row and no VRAM is spent.

The 2026-10-25 opening-movies hand-off (docs/INTRO.md: the movie program is called from the running game and returns into it, instead of chain-loading `PSX.EXE` and booting twice; the poem and its piano play before the movies) took linked code from 785,684 to 785,232 bytes: the mailbox settings copy, `take` and the resume path in `main` are gone, `intro::play` and its 156-byte enter/leave stub replace `launch`; 9,472 bytes are free below the module pool (9,008 on the main it was cut from). The three `#[inline(never)]` marks on `audio::set_volume`, `music::set_volume` and `music::apply` stay: without them the same tree linked 1,004 bytes larger. The program itself is in the boot art chunk and costs no RAM; its movie buffers (412 KB for the Prologue) are the scene arena, idle between the title and the first scene's load.
The 2026-10-25 first-scene prefetch (docs/INTRO.md: the first scene's sound banks, ambience clips, sound effects, coverage, effect art and atlases load while the opening poem plays, so only the scene, its metadata and its code wait for the movies) took linked code from 785,232 to 785,136 bytes (-96) and left linked data and BSS where they were, so 9,568 bytes are free below the module pool (9,472 before). The shipping disc's new code is `intro::play`'s three calls and the poem's step (`intro::tick`, `call_program`, about 400 bytes), `Cache::preload` and the early branch of `admit_scenes` (about 150), and a call at each of the admission's checkpoints (`disc::checkpoint`, 40); it is paid for by `prepare_ambience` (4,108 bytes, which `main` had inlined in a speed-optimised copy of about 4,540) going out of line under `optimize(size)` for the build that has the poem, and by `prepare_effect_art` under `optimize(size)` there as well. The route-input twin and every other build without the `intro` feature link the same bytes as before (the twin's disc is byte for byte the main's). The hand-off block is the last 192 bytes of RAM above the game's initial stack pointer (the 256 bytes over it are nobody's), no linked data.

The 2026-10-30 headroom pass took 5,824 bytes back, so 8,140 are free below the module pool (2,332 before, on 1a0606e): the CD interrupt's stack went from 2,048 to 256 bytes (`hk_cd_interrupt` reaches `service_irq` and nothing deeper, 24 bytes by the SDK's stack-guard plus the wrapper's 16, and 36 bytes at most in a 0xa5 fill scan after 69 route tapes; `host/hazards.py` `irq_stack_guard` now fails a link whose tree needs more than the stack holds), and the ambience clip prefetch lands its two-sector pieces in the music FIFO's free sectors instead of a 4,096-byte stage buffer of its own (code +72, BSS -4,096 with the stack's -1,792). The FIFO itself, 98,304 bytes, is unchanged.
The music FIFO went from 48 to 44 sectors (98,304 to 90,112 bytes, 7.8 to 7.2 s) on Manny's call, which took 8,192 bytes of BSS and leaves 14,188 free below the module pool (5,980 before). Measured with the ambience prefetch in the FIFO's free sectors, on the 69 route tapes and the 7 closed-loop routes in the emulator (silicon room-load timing is unmeasured): the lowest fill moved from 20 to 16 sectors (2.6 s) at the False Knight arena gate and from 24 to 18 on the open-loop tapes, with no underruns; 40 sectors would have left 8 sectors (1.3 s) there. The False Knight arena gate loads took 45/46, 46/46 and 22/24 VBlanks at 48/44 sectors; a smaller FIFO sits under the room prefetch's 32-sector comfort threshold more often (one mid-route gate load went from 60 to 96 VBlanks, another route's gates got faster).
## Module pool slack (2026-10-30)

The module pool (108,544 bytes, the top of the gap below the stack) is sized for the tightest room; every other room leaves most of it idle. A room may put read-only data in that idle part as one of its own packages: an `art_*` package listed in the scene's module list (host/code_modules.py `table`), read with the scene group from the disc and installed by `modules::admit_art`, then read with `modules::data(id)` as a `&'static [u8]`. It stays resident until the next scene admission. The cook counts the package in the room's pool need, and the room gate fails the build when `own + carried` exceeds the pool less its 4 KiB window fence (`host/code_modules.py` and `host/hk-build/module_publish.rs`, which prints a `pool fit:` line for the six tightest rooms).

`build/modules.json` lists every room as `{room, bytes (own), carried, with_carried, spare, modules}`; `spare` is the pool limit (104,448) less `with_carried`, the bytes a room can still take. On 1a0606e plus the headroom pass:

| Room | Own modules | Spare |
| --- | --- | ---: |
| 14 Crossroads_10 (False Knight arena) | false_knight, debris | 36,384 |
| 48 Brooding Mawlek's | mawlek | 45,952 |
| 10 Gruz Mother's | gruzzer, gruz_mother, fsm3, debris | 26,640 |
| 21 Husk Guard's | husks, debris | 38,748 |
| 4 Hornet's | hornet, hornet_art, art_hornet, debris | 6,132 |
| 9 and 11 Greenpath crawler rooms | enemies, art_grub | 408 and 7,636 |

Two cheaper-looking sources of RAM are not slack. `render::TILE_EVENTS`, `render::SCISSOR_SECONDARY` and `render::POOL_INFO` are rebuilt or read in every frame or view bind (they exist to make those faster), and `dialogue::GLYPHS` carries the Geo HUD every gameplay frame and the pause and shop panels, which can open in a fight. Aliasing any of them would switch off a speed cache or a screen. Idle pool bytes need no such trade, because a room that is not using them cannot be affected.

Static data ends at `0x801d783c`; the reserved stack begins at `0x801f3f00`.
The cheat HUD has nine red and22 blue packet slots, reusing existing art.
There is no heap. The single scene arena occupies 419,840 bytes, replacing five
256KiB room slots (1,310,720 bytes). Renderer template/cover arrays support
1,120 scene-global texture IDs; the animation cache still permits 2,048 keys.
Geo logic, resident art/audio, functional menus, break-effect and Lifeblood
tables increase the static span; the pinned Town/Tutorial pack and its
regenerated tilemap fills changed the data size.
The recovered gap is available for bounded future data and precompute tables.
It is not a measured stack allowance; main-stack high-water remains unmeasured.

The former EXE's `main` prologue allocated 33,216 bytes by itself, exceeding
its 32,768-byte reservation before nested calls. `host/stack_budget.py` now
derives a project linker script from the unchanged pinned SDK, protects 48 KiB,
and checks the actual final main prologue before packaging. The generated
script's content hash is part of its filename so budget changes force a relink.
The current main frame is 24,824 bytes, leaving 24,328 reserved bytes for nested
calls and other stack use; this is **not** an observed high-water measurement.
It was 35,688 at build 154, and moving the 12,112-byte particle pool out to a
static took 17,256 off the frame rather than 12,112, because constructor
temporaries shared its lifetime. Do not read a frame delta as a struct size.
The linker also excludes the 256 bytes above its initial SP from static RAM.
Current evidence: `.hkpsx/build.json` and `.hkpsx/game69/build-provisional.log`.
The outlined enemy terrain helper has a2,336-byte final binary frame, including
a2KiB edge cache reused per actor. It adds no persistent RAM allocation.

Particle lifetime uses a nonzero representation, eliminating the separate
Option tag: the 128-slot pool shrinks from 6,668 to 6,156 bytes before the four-byte cached active count without changing
counts or trajectories. This saves 512 bytes in the stack-resident world state;
it is not 512 bytes reclaimed from BSS. Compiler frame layout also changed, so
do not attribute the entire main-frame reduction to those 512 bytes alone.

Exact particle tracks add399,432 bytes plus142 bytes of emitter bases. Each of
1,513 source-seeded particles has33 two-word phases. The live pool adds256 bytes
for128 track IDs, making it6,416 bytes; Particle remains48 bytes. Tutorial emitters use these tracks; four Town emitters retain scalar evaluation.
These are main-RAM values, not extra VRAM/SPU data or traversal transfers.

Both paragraphs above describe a pool that lived in main's stack frame. It no
longer does. The pool is now a static in `game/src/particles.rs` reached through
`particles::pool()`, so its bytes are linked data and a capacity change costs
main's frame nothing. It is 224 slots of a 52-byte Particle plus a u16 track ID
each, 12,112 bytes, and that much should move out of the main frame and into BSS
at the next cook. The stack figures in this file predate the move and the
224-slot widening; refresh them from `.hkpsx/build.json` rather than from here.
One stack cost did not move: `break_effects::tick_specs` still holds a
per-capacity swept-bounds scratch, 16 bytes a slot, 3,584 at 224.

The main frame increased beyond the pool's256 bytes because compiler-generated
constructor temporaries share its lifetime; do not equate pool and frame growth.
Pass56 reduced main to18,168 bytes by outlining construction, but regressed frame
cadence and was reverted. Its14,624-byte constructor still held intermediate
copies. The48KiB reservation and high-water caveat apply to the retained build.

| Scene | Raw bytes | Stored bytes | RAM offset | Decoder suffix capacity |
| --- | ---: | ---: | ---: | ---: |
| Tutorial |321,956|181,163|0|354,032|
| Town |88,200|53,583|0|354,032|

HKSCNE02 resident data omits static page/palette payloads held in VRAM.
Geometry, animation and alpha metadata share one scene arena; only the active
scene occupies it. A separate86,016-byte aligned BSS arena holds that scene's
HKOCSC01 visibility certificates:83,692 bytes for Tutorial or27,192 for Town.
Those proofs decode through the empty geometry arena before atlas and geometry
admission. Moving aggregate linked proofs into the reusable bank recovers14,320
net linked bytes after additional parser/loader code and alignment. This does
not establish capacity for every original scene. Both arenas are checked against
the final link map; main-stack high-water remains unmeasured.

The renderer uses948 bytes of CPU-only scratchpad. An888-byte draw workspace
(three32-piece partitions, eight occluders and union bounds) overlays the
600-byte owner grid used earlier during coverage collection;60 row-mask bytes
remain disjoint. Explicit phase initialization
ends owner borrows before draw access. These are never DMA sources or stack
storage;76 bytes remain. Row retirement events use2,760 bytes instead of1,560:
one source-row mask replaces repeated per-cell retirements. Packed texture
classification adds320 aligned bytes and replaces runtime atlas inspection.
The linked data above includes101,886 bytes of host-certified4px coverage tables
for1,474 immutable original poses. The host cap is104KiB and the final linker
continues to enforce the protected stack. No extra VRAM is allocated.

The renderer retains 480 local scenery records and 1,032 draw packets; global
texture templates and alpha covers have 1,120 slots. The dedicated2,048-byte CD
IRQ stack and136-byte context are included in BSS. An observed minimum SP across
both stack allocations is not main-stack consumption; high-water remains unknown.

| Gameplay VRAM reservation | Bytes |
| --- | ---: |
| Two 320×240 16-bit framebuffers | 307,200 |
| Nineteen persistent 256×256 4bpp scenery pages | 622,592 |
| Twenty-four 64×64 4bpp animation slots | 49,152 |
| Physical capacity for 1,248 texture CLUTs | 39,936 |
| Health and SOUL HUD rectangles | 2,208 |
| Shared black-mask fade CLUT | 32 |
| Original dialogue font strips and CLUT | 7,712 |
| Geo texels and CLUT strips | 6,336 |
| Lifeblood cocoon, Scuttler and blue-mask texels with CLUTs | 5,120 |
| Breakable-effect texels and CLUTs | 7,680 |
| Total | 1,047,968 |
| Unallocated within 1 MiB | 608 |

Each scene uses page/palette bases(0,0) with exclusive ownership. The
1,248-palette physical reservation is unchanged; the page reservation is 19 of
the 20 the layout can address, because the twentieth is now the animation
cache's second region. Coordinates are VRAM halfwords:

- Framebuffers: x0..319, y0..479. Animation slots: x320..351, y0..255 and
  x960..1023, y256..511.
- Health texels: x352..355, y0..31; CLUTs x352..367, y480..481.
- SOUL texel reservation: x356..383, y0..31; CLUTs x352..367, y482..484.
- Dialogue: texel strips x0..319,y500..511; CLUT x320..335,y481.
- Black-mask fade CLUT: x320..335,y480.
- Static page `p` in 0..18: `(384+(p%10)*64, (p/10)*256)`. `p=19` would be
  `(960,256)`, which is the animation cache's second region instead.
- Bank0 CLUTs: first400 at x0..319,y480..499; last16 at x336..351,y480..495.
- Bank1: x320..383,y256..359. Bank2: x320..383,y360..463.
- The former bank3 CLUT area is no longer reserved for scenery.
- Geo: (320,464,64,16), (352,240,32,16), (352,485,32,27),
  (320,496,32,16), (352,32,8,32), as x/y/w/h halfwords.
- Lifeblood: (352,96,32,80). Break effects: (352,176,32,64), (360,32,24,32),
  (352,64,32,32); the latter two were carved out of Geo's former
  (352,32,32,64) strip. The final2KiB transfer admits Town crystal particles.
- The Hollow Shade reserved 14 CLUT rows at x320..335, y482 and uses two. The
  Knight's ability clips take four of the rest, (320,484,16,1) upward, and use
  all four. The charm board's icons take the next four, (320,488,16,1) upward, and
  use all four. The hit flash's silhouette palette (game/src/render.rs) is
  y492, and Goams, stalactites and grub jars (host/props.py) take
  (320,493,16,1) and the row below it. That left one row of the fourteen
  unreserved, y495, which the heal's white flash takes for its grey palette (the
  ability art's fifth). The reserved CLUT block is full. No
  art here reserves a texel rectangle: all of it lives in linked RAM and reaches
  VRAM through the shared 64x64 animation slots, because an ability is usable
  in any view, a Shade can be in any scene and a pause screen is reachable from
  every view (and props are too few per scene to earn atlas space), so the figure that applies to each is the tightest view in the
  whole world rather than any one scene's.

### Where the sixteen extra animation slots came from

The animation cache was eight slots in a 32x256 halfword strip at x320, which
is half of one texture page, and it cannot grow: the disjointness test leaves
304 halfwords unclaimed in two fragments 16 halfwords wide, against a slot's 16
by 64. A ninth slot had to come from a whole reservation, and the static
scenery pages were the only one large enough.

Measured over all 45 cooked scenes in `.hkpsx/packed-scenes.json`, the page
reservation was never full. Tutorial_01 is 18 pages, Crossroads_ShamanTemple is
9, Crossroads_10 (the False Knight's arena) is 8, and 44 of the 45 are 9 or
fewer. Pages 18 and 19 were reserved and no scene has ever uploaded either.
Page 19 is now the animation cache's second region, at (960,256): a 4bpp
texture page is 64 halfwords by 256 rows, which is exactly four 64x64 slots
across and four down, so one page is worth 16 slots. Page 18 is what remains of
the spare, and Tutorial_01 keeps one page of headroom.

The cost is bounded and measured. No cooked scene loses anything. Every scene
uses page base 0 under scene-gate residency, so the released page is a suffix
of the range and nothing needed relocating. The one thing that no longer fits
is a joint-residency plan whose scenes sum to 20 pages, which the shipped disc
does not use; `tests/test_pack_scenes.py` pins that.

Two costs are not measured here. Raising `MAX_UPLOAD_BYTES` from 16,384 to
49,152 raises a cap rather than a per-frame transfer, since a frame uploads
only its misses, but a 12-tile boss frame changing pose is a 24,576-byte VRAM
write and its GPU and DMA time has not been timed on hardware or in the
emulator. The cache's own storage grows by about 224 BSS bytes for the 16 extra
slot records and the wider request array, which is derived from the struct
layout rather than read off a link map.

Allocator tests check all reservations together. The title temporarily uses
89,600 VRAM bytes that gameplay reuses. Its menus share the existing7,712-byte dialogue font reservation. The source
pack is already in linked RAM and is not a simultaneous extra gameplay VRAM allocation.

## Content and transfer bounds

The 98-region layout retains the source scenery instances, geometry, layers,
parallax and tint. Scenery uses a 48-pixel longest-axis cap. The new Focus clips
add 26 frame records per region. Crawler corpse clips, grass impacts and the
eight admitted door-art variants now share these same budgets. The dynamic draw reserve is 264 packets: up to 8 Knight/nail/enemy packets,
32 combined fragment/grass-effect packets and 128 source-particle packets, plus 80 Geo quads and 16 Lifeblood parts. The28 persistent fragments do not require new VRAM banks.

The three Great Door stage frames add 2,208 animation-bank bytes and three
palettes, using one existing animation slot while visible. Their distinct
source frame bounds are retained. No static page or additional VRAM rectangle
is reserved for the door.

The two compact scene payloads total206,518 stored bytes. The23 startup atlas
chunks hold694,048 raw /321,841 stored bytes. Combined stored size528,359 bytes
is21,873 bytes larger than the former combined scene encoding, trading a small
startup read increase for699,876 bytes removed from the resident arena.
WORLD.PAK begins at LBA1024; its32-entry table binds2 scenes,6 ambience clips,
23 atlas chunks and the final Focus sound bank. Original HKROOM02/full HKSCNE01 files remain host analysis
inputs. Raw/stored/source/decoder hashes are in the packed-scene/build reports.

Startup loads ambience and Focus through the empty arena into SPU, then validates and
uploads atlas chunks synchronously before admitting compact scenes. No atlas
buffer is reused before draw_sync completes. Retry revokes admission before
menu artwork overwrites VRAM, ensuring all pages are restored on the next start.
Pass49 startup reads509 sectors; static uploads total694,048 bytes with a32KiB
maximum chunk. Both counters stay unchanged throughout the long gameplay route.
The24 animation slots retain an independent48KiB maximum per frame. Animation
frames come from resident RAM, with no traversal CD reads or decoding. This is
the current two-scene subset, not full-game residency or hardware timing.

Current SPU allocations, re-read from the banks' own generated constants
(`data/sfx.rs`, `data/geo-audio.rs`, `data/ambience.rs`, `data/world-sfx.rs`,
`data/focus-audio.rs`, `data/runner-audio.rs`) rather than from the last
paragraph that quoted them: player SFX 75,184B at 0x1010..0x135C0, the menu
clips 2,560B above it, Geo 13,888B at 0x14000..0x17640, ambience
0x18000..0x4A0A0, the area music ring 16,384B at 0x4E1B0..0x521B0, the world
one-shots (enemy death, the Knight's death, menu confirm and start, the
Lifeblood cocoon) 59,296B at 0x521B0..0x60950, Focus 108,208B at
0x60950..0x7B000 and Runner 20,464B at 0x7B000..0x7FFF0, below the 16 bytes
`psx_spu::init` parks the disabled reverb work area on. Focus and Runner are
pinned to the top; the world bank is cooked to end where Focus begins, and
`host/hk-cook/src/ambience.rs` stacks the ring and its own ceiling below it.
Ambience is loaded per area since 2026-09-23: a clip is read at the scene gate
that first needs it, into an address it shares with clips of other areas, and
two clips share bytes only when no cue plays both and no gate joins scenes
that do. Every loop is held whole, cave_noises' 99,936B included since area
music took its RAM copy and ring. The widest set any cue or gate keeps
resident is 204,960B (Cave plus the MiscWind loop across the Crossroads gate
between them), against 280,928B plus a 99,936B RAM copy when all eight loops
were resident; Fog Canyon's and the Waterways' loops, which no admitted scene
plays, never load. That leaves **16,656B** of ambience headroom at
0x4A0A0..0x4E1B0, plus gaps of 64B below Geo and 2,496B below ambience. Before
the world bank the headroom was 59,584B and 16,384B above Runner sat unused,
while the cook refused the five world clips against the 64 bytes left below
Geo. The per-scene one-shot banks (docs/SCENE_SFX.md) take no range of their
own: each scene's clips sit in the ambience region's gaps that no stem able to
sound in that scene uses, 16,656 to 56,432 bytes per Crossroads scene. `.hkpsx/ambience.json` `resident_by_cue` lists
each cue's loops and bytes. Before per-area loading, ambience *shrank* 16,272B
when the resident atmos set went from six channels to eight, because eight
loops only fit at 4 kHz and the set that covers the game is cheaper at that
rate than six were at 8 kHz; Focus and Runner moved down with it. Their bases
are hardcoded in `host/hk-cook/src/focus_audio.rs` and `host/hk-cook/src/runner_audio.rs`, and
`host/hk-cook/src/ambience.rs` refuses to finish a cook that leaves either of them stale,
naming the base each one has to take. The cheapest further margin priced but
not taken is still 8,192B from halving the streamed clip's SPU ring, which is
sized for the 8 kHz era and gives the streamed loop 3.58-second halves rather
than the 1.79 s `game/src/audio_stream.rs` is written and commented for. The figures this
paragraph's first version quoted were wrong in three ways and omitted the
Runner bank entirely, which put the free figure 20,464B too high on its own;
`host/hk-cook/src/ambience.rs `spu_ceiling`` computes the ceiling the cook actually
enforces. The SDK silent loop is at 0x1000. Player short effects/heal
use 22,050Hz mono; running and Geo use 11,025Hz mono; sustained Focus charging
uses 8,000Hz mono. Ambience is one rate now: all eight resident loops are
4,000Hz mono, because eight do not fit SPU at 8,000. Only
`cave_atmos_misc_3` measures better there than at 8,000; the rest pay between
0.45 and 4.93dB for the coverage, channel by channel beside
`RESIDENT_ATMOS_CHANNELS` (host/hk-cook/src/music_report.rs). All admitted source clips are complete.

The full99,936-byte `cave_noises` is resident in main RAM, feeding a16KiB SPU
ring through8KiB scratch. It keeps the streamed slot under the eight-channel
set: a streamed clip only occupies the ring whatever its length, so which
channel streams is chosen for what stays exercised on hardware rather than for
bytes, and the Cave cue keys this one on in 41 admitted scenes. Other ambience and Focus payloads stay in SPU;
startup uses the scene arena and adds no permanent staging buffer. Focus uses
voice18 and alternating19/20 heals. No playback-time CD read is needed for
these effects. CD-DA title music uses44,100Hz stereo directly from the disc,
without a voice or sample bank; completed Pause precedes room loading.
The final66 startup reads562 data sectors and uploads694,048 static VRAM bytes;
both counters remain unchanged through the validated Town traversal. See
MUSIC.md for source and replay evidence. Full death layers, numerous effects
and boss music remain pending. Physical CD/SPU timing remains unverified.

Pass40 retains480bytes of packed scenery-extrema indices and a108-byte
initialized-prefix occluder snapshot. The former replaces an experimental
7,680-byte full bounds table, recovering7,200bytes. Background packet words
reuse the existing pool after DMA ownership ends; no second framebuffer or
texture allocation is added.


Historical pass48 included8,180bytes of optional grouped seam certificates (124
selected bank groups, maximum32 per region). The inverse pool map uses6,080
logical stack bytes only during admission; final `render::init` frame is6,288,
so main+init totals31,824 before nested calls/IRQ. Collision sweep caching uses
2,048 logical stack bytes: final tick frame4,352, with the observed static
main/State/tick/collide chain totaling30,960. These chains fit the48KiB reserved
stack, but are not a measured high-water guarantee. The subsequent compact-scene split recovered nearly700KiB without removing
these coverage tables. Historical stack chains above are not current high-water measurements.

## Where the remaining headroom went (build 107)

Headroom before the stack is 68,988 bytes, down from 246 KB at build 94 and
116 KB at build 106. The link map ranks the reservations that account for it:

| Reservation | Bytes | Section |
| --- | ---: | --- |
| `disc::BUFFERS` (scene arena) | 421,176 | bss |
| `HK_AMBIENCE_STREAM_SOURCE` | 199,872 | bss |
| `main` | 130,808 | text |
| `disc::COVERAGE_BUFFER` | 83,700 | bss |
| `audio::BANK` | 63,376 | bss |
| `render::PACKETS` | 57,792 | bss |
| `render::TEMPLATES` | 46,080 | bss |
| `render::DRAWS` | 33,792 | bss |
| `data/shade.hk` | 30,386 | data |
| `enemies::Actor::advance_in_view` | 29,656 | text |
| `render::SCISSOR_CACHE` | 28,160 | bss |

Build 109 halved that stream's rate, so the table's 199,872 became 99,936 and
headroom 166,876, which is still the reservation: raising the resident set to
eight channels left `cave_noises` streaming and did not move it. The rest of
this section is the analysis that led there.

The one disproportionate entry is `HK_AMBIENCE_STREAM_SOURCE`: 199,872 bytes,
nearly three times the remaining headroom, holding one scene's streamed
ambience stem in full. The clip is already read from the disc at scene
admission and then fed SPU-ward in chunks by `audio_stream::service`, so the
resident copy is what costs the RAM, not the streaming itself. Replacing it
with a small ring refilled from the CD, the shape `music.rs` already uses,
would recover about 200 KB and take headroom to roughly 269 KB. The risk is
audio underruns from CD contention, which `HK_AUDIO_STREAM_UNDERRUNS` already
fails a route on, so the change is testable rather than speculative.

`render::TEMPLATES` is `SCENE_TEXTURE_CAPACITY` (1,152) times 40 bytes, and the
largest admitted scene uses 1,138 textures, so there is almost no slack there.
The scene and coverage arenas are sized by the largest scene and cannot shrink
without changing what a scene may contain. `main` at 130,808 bytes of text is a
single function; splitting it would help only if the pieces stopped being
inlined into one another.

P14 adds movement abilities whose new Knight clips cost both per-view texture
budget and linked RAM, so the ambience change is worth making before that
package rather than during it.

## What build 109 did, and what is left

Streaming the clip from the disc was the wrong idea: docs/MUSIC.md records that
the resident copy exists precisely to avoid the CDDA and data-read contention
the SDK's `cdda-read-contention` example demonstrates, and all six loops load
once at boot rather than per scene. What the clip actually needed was a lower
rate. 99% of its energy is below 637 Hz and only 0.06% sits above 2 kHz, so it
cooks at 4,000 Hz instead of 8,000, halving the RAM cache to 99,936 bytes for
no audible loss once the resampling is done properly.

Taking the other half would mean making it SPU resident, which needs about
119,000 bytes of SPU that is not free today. The two Ruins loops (77,584 bytes)
are cued in all 45 scenes at gain 16 of 16383, roughly -60 dB, so they are
inaudible on this disc and could be dropped until City of Tears ships; the two
gaps below ambience are another 16,912; the ring itself is 16,384. That
together with the 4 kHz clip fits, but it relocates every audio bank's base
address and changes the cue mask layout, so it is its own change rather than a
tail on this one.

Re-priced at build158 against the SPU map above, because the two numbers in
that paragraph were both wrong. SPU residency needs the clip's own 99,936
bytes, not 119,000, and the two gaps below ambience are 5,120 bytes, not
16,912. Against 8,032 contiguous free, 13,152 counting both gaps, and the
16,384 the ring gives back, a repacked SPU is still 70,400 bytes short. The
shortfall is almost exactly the two Ruins loops, so the trade has not moved:
99,936 bytes of main RAM for two source ambience channels, a recook of every
clip from the licensed install, and new base addresses for the Focus and
Runner banks. It is a content decision before it is a budget one, and nobody
has taken it.

## What bounded the P14 and P15 work (build 133)

Linked headroom was 113,636 bytes at build 133, down from 166,876 at build 107, and is
73,876 now. Three decisions this session were budget decisions, and
the number that decided each is worth keeping, because two of them look like
code problems and are not.

**The per-region texture budget, not RAM, is the binding constraint on art.**
`TEXTURE_BUDGET` is 416 CLUT slots per region. Adding the Knight's ability clips
to the per-region bank put the tightest region at 419 with all ten clips, 421
with only the four movement clips, and still 418 with three of them. Admitting
two Climbers whose recognizer already accepts them would put Crossroads_03 at
421. Both were abandoned rather than tuned, and the Climber projection is a
floor rather than an estimate: `similarity_dedup` protects every animation
frame, so none of a new actor's frames can be collapsed.

**The way past it is the Hollow Shade's route**, which costs linked RAM instead
of textures. 97 ability frames across 16 clips are 36,190 linked bytes and two
CLUT rows, taken from the fourteen the Shade reserved at y482 and never used.
That is why the ability animations ship and the Climbers do not: the abilities
are usable in every view, so they had somewhere else to live; a Climber belongs
to two specific scenes, so it does not.

**The charm board's icons took the same route, and the screen picked their
size.** Forty icons are 5,248 linked bytes at 16x16 across four resident CLUT
rows, and cost no per-view texture slot at all. The atlas route stayed refused,
now for two reasons rather than one: the tightest view in the world is the
figure that applies to a pause screen, and resident texels would also need
2,560 VRAM halfwords against the 304 `residency::SPARE_HALFWORDS` leaves
unclaimed. What settled 16 over 24 or 32 was neither budget. Six rows of
24-pixel icons are 144 scanlines on their own, and with the title, the notch
counter, the six-line description and the footer around them that is 281 of the
screen's 240; `host/charms.py::panel_layout` derives it and the cook refuses
rather than dropping a row or a description line to make a bigger icon fit.

**The scene arena is 421,128 bytes** and the ambience clip's
4 kHz recook freed the last large block. Taking the remaining SPU half still
needs the relocation described above, and nothing this session touched it.

## The coverage arena, and why it cannot share the scene arena (build 158)

`disc::COVERAGE_BUFFER` reserved 83,700 bytes because that was the largest
proof when the constant was written. The largest proof is 80,972 now, so
`host/hk-cook/src/scene_certificates.rs` reserves what the cook actually wrote and keeps
83,700 as `ARENA_CEILING`, the size a proof may not exceed. A ceiling nothing
re-reads goes stale silently; a reservation derived from the manifest beside it
cannot. That returns 2,728 bytes to linked BSS at the next cook.

Merging it into the scene arena was the obvious next question and the answer is
no, twice over. The proof is not admission-only: `render::prepare_back_prefix`
takes a fresh `Cache::coverage()` view every frame the camera moves and reads
the tile and group certificates and bitmaps straight out of the buffer, so its
bytes are live for as long as the scene's are. And the arena has no room. It is
417,748 bytes, which is Tutorial_01's 349,032-byte scene plus the 68,712-byte
world metadata bank plus a four-byte guard, so four bytes of tail survive the
scene that sizes both. An arena holding scene, bank and proof together would
have to be 498,716 bytes to delete an 83,700-byte buffer, a net 2,732 bytes
before the decoder's relocation margin eats into it. That is the same 2,728 the
stale ceiling gives up for nothing, so the risk buys nothing.

Splitting the proof by lifetime does not pay either. The pool map and the
group list are read once in `render::init`, the four certificate and bitmap
sections every frame, and dropping the admission-only pair from the resident
copy would save 6,932 bytes on the scene that sizes the buffer. They cannot be
consumed early, because `bind_tile_groups` needs the admitted `Room` and the
scene decode has already taken the arena by then.

The 2026-10-19 console-fix pass (gate arrivals grounded by the cooker, the Knight's LookUp/LookDown clips, loop-section ability clips, the Shade and Vengeful Spirit mirror fixes, Myla's hum, pickaxe and sung line, gate-entry and look telemetry, the soft-dust gain) took linked code from 754,836 to 758,912 bytes (4,076) and linked data from 258,536 to 263,432 (4,896: 17 look frames and the loop table, scene-sfx rows), so 60,260 bytes are free below the module pool (69,284 before); static end 0x801cc39c.

The 2026-10-25 title overlay (docs/MENUS.md "The title as a boot overlay"): `menu.rs` apart from `skip_drop` and its statics is the boot code module `title`, read from its own disc chunk right before the title art, installed into the module pool for the title and the loading frame, and evicted before the first scene. Free RAM below the module pool went from 9,568 to 24,856 on the shipping disc: linked code 785,136 to 770,432, linked data 284,552 to 283,960. The module is 18,188 bytes of image (a 19,064-byte chunk) in the pool while the title is up; the resident forwarders (`title.rs`) and `skip_drop` stay.

The 2026-10-25 Hornet landing (docs/HORNET.md, docs/BOSS-ART.md): her fight, her drawing and her trees are two code modules and a data package carried by her room (Fungus1_04), and the resident image holds three small hook calls, the draw pass's packet loop and the telemetry words. Linked code went to 771,460 bytes, data to 284,424 and BSS to 800,640; free RAM below the module pool went from 24,856 to 23,264 on the shipping disc (static end 0x801d5420). Her room needs 79,652 pool bytes and carries 18,432 (the Knight's Death and respawn clips and the ability effects): 98,084 of 98,304. The resident mask holds 32 modules and packages, so the Acid Flyer's code moved into the Mosquito's module.

The ability effects at the Knight's own pixel density (docs/ABILITY_EFFECTS.md) ride two optional carried packs (`art_fx`, `art_fx_spell`, 14,336 bytes each), asked for only where the scene's pool holds them (`HK_FX_FIT`). Free RAM below the module pool on the shipping disc went from 23,088 to 24,436 with them, measured on the rebased tree; the Baldur and Blocker code share one module to keep the resident mask at 32 bits.

The 2026-10-26 seamless gates landing (docs/SEAMLESS.md: prefetch predictor, runner audio bank read ahead, head park, entry-hold overlap, spare-VRAM-page spill, whole-sector scene arena with the last sector's padding) took linked code from 770,092 to 776,016 bytes, linked data from 284,616 to 284,888 and BSS from 800,652 to 801,356 (the spill state, the gate counters and the prefetch plan), so free RAM below the module pool on the shipping disc went from 24,436 to 17,540 (-6,896; the route-input twin is 18,276 free). The scene arena grew from 415,052 to 415,744 bytes: it is whole sectors now, so the last sector's padding fits the second window of Tutorial_01's group.

The 2026-10-26 set-piece effects landing (docs/EFFECTS.md, docs/DEBRIS.md): the Great Door's dust, rubble and chips, the Lifeblood cocoon, the grub jar's glass, the nail hit's dust and chips and 409 rigid debris pieces ride the break-effect pool. Linked code went from 776,016 to 775,740 bytes and linked data from 284,888 to 278,584 (the particle curves are stored as 55 single-end tables, 5.4 KB for what was 15.2 KB, and the scene decoders are built for size), BSS grew from 801,356 to 805,756, and the scene arena from 415,744 to 419,840 (the metadata banks carry the pieces). The cocoon's world code and the debris solver are the room modules `lifeblood` and `debris` (the debris module is about 5.8 KB and lands in the worst room, Crossroads_05, on top of the carried Shade, Death and wake art), so the module pool grew by 6,144 bytes to 108,544 (the build's pool-fit check passes); free RAM below the pool on the shipping disc went from 17,540 to 13,572. The resident set of modules is two words (`modules::Mods`, `Mask64`): the two optional ability packs take ids 32 and 33, while a room's own modules stay the first 32 ids and one word.

The 2026-10-29 set-piece effects pass (docs/EFFECTS.md, round 3: the Great Door's hit flash, light pulse and rising dust, the cocoon's petals, streaks and wash, the grub jar's flash, halo and shards; stretched additive quads, a front draw pass, 40 styles per scene instead of 28) took linked code from 775,740 to 777,448 bytes and linked data from 278,584 to 281,064, so free RAM below the module pool on the shipping disc went from 13,572 to 9,380 (-4,192; the pool stays 108,544, and the floor is 8,192).

Known frame-time cost of that pass: the Knight's Focus dust is now drawn as the soft glow (it was a dithered speckle that skipped most particles). The `focus` route (King's Pass, no break) went from frame time 207 of 389 frames with 25 over 2 vblanks (worst streak 8) to 210 of 382 with 38 (worst streak 18), measured on twins of 9a0bca6 and of this commit; its pin is 40. Frame time of the routes without that dust is unchanged to within one hundredth of a vblank.

The 2026-10-29 doorways pass (docs/SEAMLESS.md): the scene, atlas and world-metadata chunks are checked with a word hash (four bytes a round) instead of byte-wise FNV, and the runner audio bank at a gate is read and validated in one pass. Linked code went from 777,448 to 779,268 bytes and linked data from 281,064 to 281,112, so free RAM below the module pool on the shipping disc went from 9,380 to 7,508 (-1,872; the pool stays 108,544).

The 2026-10-29 round 4 of the set-piece effects (docs/EFFECTS.md: the strike's spark sprites and streaks, the cocoon's bubbles and splat, the jar's glass pieces as the original's own sprites, cells that follow a clip's frames) took linked code from 779,268 to 778,244 bytes and linked data from 281,112 to 281,240 so free RAM below the module pool on the shipping disc went from 7,508 (main after the doorways landing) to 8,404, measured on the shipping build of this commit (+896). The effects' art in Tutorial_01 is 6,912 of the 7,552 bytes the four VRAM fragments hold.

The 2026-10-29 round 5 of the set-piece effects (docs/EFFECTS.md: needles in place of the strike's sprite clusters, aimed streaks, two frames of the splat, the jar's large glass pieces cut at 13 texels so they fit Crossroads_05) took linked data from 281,240 to 281,400 bytes (the new glow curves and styles) and left linked code at 778,244, so free RAM below the module pool on the shipping disc went from 8,404 to 8,244 (the floor is 8,192). It also took the three routes round 3 had made slower back to main's figures: secret-c37 395 of 59 frames (417 in round 4), secret-c04 199 (204), journey-crossroads 33 frames of 5+ vblanks (38).

The 2026-10-29 doorways pass (docs/SEAMLESS.md): the stored scene, atlas and metadata chunks take a word hash, a gate swaps the runner audio bank only into a room that places a Runner or a Zombie Shield (the family look-up is cached per scene, 64 bytes of BSS), and the break particles' draw takes a due pad poll every 16th particle. Linked code went from 778,244 to 779,920 bytes and linked data from 281,400 to 281,000, so free RAM below the module pool on the shipping disc went from 8,244 to 6,916 (-1,328; the pool stays 108,544).

The 2026-10-30 boss title card (the original's small capitals at the lower right, `title_card.rs`; the glyphs are the card strip's, `font_strip.rs` `BOSS_LINES`) took linked code from 779,920 to 780,512 bytes and linked data from 281,000 to 280,808, so free RAM below the module pool on the shipping build went from 6,916 (main c2ba280) to 6,516 (-400). The boss glyphs share the area glyphs' tables and one lookup: a first version with its own boss tables cost 1.9 KB, and the lookup over the strip's characters has to be a binary search, because the compiler folds a linear one into a 212 word table.

The 2026-10-30 reward pickup (a shiny's `INSPECT` prompt marker through the NPCs' marker, and the item message at the lower left for 4.2 s instead of a centred card) took linked code from 780,512 to 780,688 bytes and BSS from 805,820 to 805,836 (the pickups' prompt point), so free RAM below the module pool on the shipping build went from 6,516 to 6,308 (-208).

The 2026-10-30 boss presentation (the False Knight's landing and slam dust, the floor mist and the entrance landing's sparks as glow emitters of Crossroads_10, `fx.rs` owners 0xD040 to 0xD042, raised by `frame.rs` from a three word static the actor loop leaves; and his wave drawn as every spurt instead of every third) took linked code from 780,688 to 781,252 bytes and linked data from 281,000 to 280,872, so free RAM below the module pool on the shipping build went from 6,308 to 5,676 (-632). Measured on dev ships of the same tree (7,252 free before): the dust, mist and sparks -328, the dense wave -304. A first version called `fx::raise` from the actor loop and cost 1,248 for the dust alone: any call there grew `advance_in_view` by 956 bytes; the glow curves are shared with the existing glows (grow 1.3, hold 0.25 and 0.5), which saved 297 bytes of tables. The effects' art in Crossroads_10 takes 3,285 of the 3,776 halfwords the four VRAM rects hold. Not done: the orange balls of light of the entrance (the 13 texel glow cell drew them as hard blocks that lasted two frames), and the wave's white (the only quad flash is the infected orange).

The 2026-10-30 False Knight arena split (the scene's stream after its alpha covers, 219,136 bytes of the boss's streamed frames, is a chunk of its own that the room reads after the gate; docs/SEAMLESS.md) took the free RAM below the module pool on the shipping build from 5,676 to 3,044 (-2,632). Both figures are `ship-final.sh` builds, main 795c877 and this tree: linked code 781,252 to 783,840 (+2,588), linked data unchanged at 280,872, BSS 805,844 to 805,900 (+56). The code is the tail's reader (a state machine in a few words of statics: `tail_arm`, `start_tail`, `land_tail`, `tail_service`, `tail_finish`, and `Gap` in room_decode.rs) and the split's tables, built for size; the retries (three tries per burst or check) stay, a real drive fails reads.

The 2026-10-30 NPC interact hint (the original's white up arrow and the word `Interact` under the Knight's feet while an NPC is in reach, `dialogue::interact_hint`: four flat boxes and the dialogue font; the hero's position reaches `dialogue::prepare` through the `HERO` static) took linked code from 783,840 to 784,276 bytes and linked data from 280,872 to 280,904 (the four boxes of the packet table), so free RAM below the module pool on the shipping build went from 3,044 to 2,556 (-488). Main's arena split had already taken free RAM from 5,676 to 3,044 before this pass.

The 2026-10-30 light cells (docs/EFFECTS.md round 6: the soft disc, the bar and the petal cut at 32 texels, bars and petals as long cells of the rows they light, the cocoon's splat as eight petals) are cook only: the guest is main's, so linked code, BSS and the pool are unchanged, and the effects' VRAM rectangles hold the same 7,680 bytes (the largest scene's effect art, Crossroads_05, is 7,032). Only linked data moved, by 224 bytes (280,904 to 281,128: the cooked art tables are sized by the widest scene), so free RAM below the module pool on the shipping build went from 2,556 to 2,332. A first version that generated the disc into an animation slot and drew needles as gouraud polygons cost 3.6 KB of resident code and was dropped. The animation cache's per-frame key count (24 allowed) peaked at 18 in Hornet's fight and 14 in the False Knight's on a probe build.

The 2026-10-30 bench interact hint (the original's `Interact` hint at a bench the Knight can sit on, as at an NPC; `bench::State::tick` raises `dialogue::HINT_ONLY` from the bench search it already runs, and does that search every eighth tick unless UP is pressed, because every tick cost secret-c37 0.04 of a vblank a frame) took linked code from 784,368 to 784,504 bytes and BSS from 800,020 to 800,036, so free RAM below the module pool on the shipping build went from 8,140 to 7,980 (-160).

The 2026-10-31 boss death effects (the False Knight's armour bleeding orange through `Steam` and bursting at `Blow`, as hkref shows: `boss_fx` requests from the death state, two emitters per boss scene cooked from `BossLook` data at `fx.rs` owners 0xD043 and 0xD044, and `camera::frame_x` holding the Death Head in frame) took linked code from 786,256 to 786,488 bytes and BSS from 791,908 to 791,924, so free RAM below the module pool on the shipping build went from 14,188 to 13,932 (-256). The cost is the request static and its drain in the frame, the camera's pull and the boss tick's block; a later boss's death adds data and one call and no new code.

The 2026-10-31 False Knight walk (docs/ROUTES.md, "The False Knight and the City Crest") publishes raw words and lets the gate derive the rest: `HK_FK_X`, `HK_FK_Y`, `HK_FK_BODY` (four words), `HK_FK_FLAGS` and `HK_WAVE` (two slots of x and direction), written by `EnemyWorld::foe_senses` where it already finds the boss. It took linked code from 786,488 to 786,664 bytes (+176), linked data from 281,304 to 281,400 (+96) and BSS from 791,924 to 791,964 (+40), so free RAM below the module pool on the shipping build went from 13,932 to 13,620 (-312). The derived senses the walker published before (`HK_WALK_STRIKE_*`, `TURN_*`, `JUMPSTRIKE`, `FOCUS`, `HK_FOE_GAP_*`) can now be expressions over raw words in the gate and come out of the guest in a follow-up.

The 2026-10-31 move of the walker's derived senses out of the guest (`HK_WALK_STRIKE_*`, `TURN_*`, `JUMPSTRIKE`, `FOCUS`, `HK_FOE_GAP_*`, `HK_FOE_LIVE`, `HK_FOE_DX/DY` and the `FoeSenses` code that computed them; they are expressions in `host/hk-build/walk-prelude.walk` now, evaluated by the gate over a raw `HK_ACT` table of the live bodied actors, `HK_PLAYER_VY` and `HK_HEALTH_MAX`) took linked code from 786,664 to 785,832 bytes (-832), linked data from 281,400 to 281,656 (+256, the table) and BSS from 791,964 to 791,924 (-40), so free RAM below the module pool on the shipping build went from 13,620 to 14,236 (+616). The False Knight words stay (they are raw), and the Goam sense (`HK_WALK_GOAM_*`, a window of a Goam's own state) stays in `props.rs`.

The 2026-10-31 boss hit flare (the nail's centre throws a white core and fans of thin streaks, `boss_fx::HIT` and owner 0xD045 cooked from `BossLook`'s `hit` layers; the False Knight's draws white out instead of flashing the infected orange, `FLASH_WHITE`) and the thicker death burst (data only) took linked code from 785,832 to 786,412 bytes, so free RAM below the module pool on the shipping build went from 14,236 to 13,660 (-576). The cost is the contact point's centroid and request at the hit, the white flash mapping in the draw path and the third owner in the frame's drain.

The 2026-10-31 actor art bank and the three False Knight arena husks (docs/ACTOR-BANK.md; Barger, Hornhead and Runner, 99 sprites and 25 clips between them) took 10,248 bytes of the 13,660 that were free below the module pool, leaving 3,412: linked code 786,412 to 792,628 (+6,216: the bank reader `actor_bank` bind, upload, set and read 3.0 KB, the draw branch in `prepare_draws` and `Draws::push_part` 1.8 KB, `frame::render` +1.3 KB with the hit-flash quad that must be resident because the bank draws it where Hornet's module is not loaded), linked data 281,656 to 285,576 (+3,920: 3,420 bytes of the bank's linked rest, the tail of its 39,420 bytes that did not fit the room's pool slack, and its tables), BSS +104. The head of the bank (36,000 bytes) is the `art_husks` data package in Crossroads_10's pool: room 14 is 18,952 (false_knight) + 6,376 (debris) + 36,000 (art_husks) + the 43,008 carried = 104,336 of 104,448 (+112). The scene arena did not grow (the six bank views' records add 4.7 KB to Crossroads_10's resident raw, inside the arena's slack). What could leave resident, in order: a better bank coder (static Huffman by left pixel measured -28 percent on the husks' 34,690 coded bytes, -11 percent on Hornet's 129,732) moves the linked rest into the pool and opens the slack for the reader as an overlay module (3.0 KB); the draw branch is on the hot path.

The 2026-10-31 shared bank coder and the bank reader as a room module (docs/ACTOR-BANK.md; the `CODED` coder of `hk_format::boss`, `bank_reader` in Crossroads_10's pool) gave 5,888 bytes back, so 9,300 are free below the module pool on the shipping disc (3,412 before; the pool stays 108,544 and the 8,192 lane margin is met again): linked code 792,628 to 789,980 (-2,648: the reader's bind, set and upload and the decoder (about 5.5 KB) left for the module, less the resident hooks `boss_hooks::bind_bank`, `upload`, `upload_texels`, `package`, `actor_bank::clear`), linked data 285,576 to 282,296 (-3,280: the 3,420 B linked rest is gone, the whole 27,483-byte bank fits the package), BSS +16. Crossroads_10's pool: 18,952 (false_knight) + 6,376 (debris) + 5,252 (`bank_reader`) + 26,572 (`art_husks`) + the 43,008 carried = 100,160 of 104,448 (+4,288, from +112). The coder on the same texels: husks 34,690 to 22,753 bytes (-34.4 percent, table included), Hornet 129,732 to 83,424 (-35.7 percent), so her scene carries 47 chunk textures instead of 70 (`HK_BOSS_BANK_FOUND` pins 70 to 47). Cost: a part decodes in about 1.5 ms (husks) and 1.6 ms (Hornet; 4.6 ms the largest) where packbits took 0.3 ms, once per part at its first use. The shipping build without the opening movies (`ship.sh`) has 1,280 bytes more free (10,580).

The 2026-10-31 switch system (docs/WORLD_LINKS.md) took no free RAM: linked code went 789,980 to 789,928 bytes (-52) and the code-to-data alignment grew by the same 52, so 9,300 bytes stay free below the module pool. The nail branch for a switch is one flag test inside `State::strike`'s ordinary break (no function of its own), and `debris::Pool::spawn` is `#[inline(always)]` into its one caller: without that, adding any code to `strike` made LLVM leave `Pool::spawn` out of line, which moved 3.5 KB into the debris module and put rooms 14 and 9 past the code pool (at the time room 14 had +112 B; it prints as `pool fit: room 14` in the build log, +4,288 B now). The switches' data is in the scene banks on disc: Crossroads_06, 33 and Fungus1_22 gained their lever and gate records inside the arena's slack.

The 2026-10-31 two-word room module sets (host/code_modules.py `SCENE_MODULES_HI`, game/src/modules.rs; a room may name any module id below 56, where it could name 32) moved the shipping disc's free RAM below the module pool from 9,300 to 9,316: linked code 789,928 to 790,072, linked data 282,296 to 282,152, alignment 88 to 72. Measured on a build without the opening movies with one probe row in the high list: 10,580 to 10,468 (the alignment step differs by build, so the mechanism itself costs 112 bytes there and nothing on the shipping disc). Each room that names an id of 32 or more adds 4 bytes of table. Module images did not grow; the aspid and false_knight images shrank by about 400 bytes each.
