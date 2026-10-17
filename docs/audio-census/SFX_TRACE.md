# HK SFX play-trace spike (S2), 2026-10-17

Branch `spike/hk-sfx-trace-2026-10-17` on hk-psx (based on `feat/hk-audio-census-2026-10-17`, `bde657c`). Tools: `tools/hkref` (autopilot hero, `play` command, 9 profiles) and `host/hk-sfx-trace` (Rust: `rates`, `tally`, `tiers`, `edges`). Committed tables, names/counts/seconds/bytes only, no audio: `docs/audio-census/trace/`. No game code changed.

## Result in short

1. The original can be driven and logged headlessly, so the fallback was not needed. `hkref` runs the macOS Steam build in batch mode (private APFS clone, patched driver, muted, sandboxed) and logs every `Play`, `PlayOneShot`, `Stop` and mixer call with clip, frame, source, position and pitch. Logged: a 118-scene survey, a Crossroads route (8 rooms, 249 s), a Greenpath route (9 rooms, 245 s), Dirtmouth with NPC talk, the False Knight (two runs), Gruz Mother and Mawlek.
2. What plays is a quarter of what the census says can play. In 16 room visits (490 s) the Knight triggered 72 of the 289 room-specific sounds the census reaches in those rooms (25% of clips, 26% of bytes at the port's rates). The tighter bound, what each enemy's FSM and each component in the scene can name, is 58% of the census (port-rate bytes) and 46% (at q20) over 106 surveyed traversal rooms; it misses clips reached through spawned prefabs, so the true ceiling sits between the two. Visits were 12 to 40 s, so "played" is a floor.
3. Most calls are not room-specific. Over a route 48 to 54% of calls are Knight sounds, 30 to 35% scenery (grass, water drips, breakables: 20 to 24 distinct sounds at 8 to 13 plays per minute each), 8 to 9% enemy and prop sounds, 7 to 8% loops. The port's cook sources mention 93% of the Knight calls (Crossroads route) and 2% of the scenery and room calls.
4. Peak simultaneous voices (one voice per `Play` source, one per `PlayOneShot`, held for the clip's length): traversal rooms mean 9.3, max 14, with 12 distinct sounds at most; boss fights 16 to 24 raw and 9 to 11 distinct sounds. Gruz Mother's death shower of Geo hits is the 24. One voice per distinct sound covers every measured case.
5. Rates (all 1,866 non-music clips up to 20 s decoded, Rust + FMOD): the SFX set is 49.6 MB at 22,050 Hz, 12.4 MB at the port's flat 5,512 Hz, 18.9 MB at the lowest ladder rate that keeps all but -15 dB of each clip's energy (q15), 25.7 MB at q20. Half the clips are fine at or under 5,512 Hz (q15); 21% need more than 16 kHz. The flat 5,512 Hz already discards more than -10 dB of energy in 31% of clips while spending bytes on low-frequency sounds that need only 1.6 to 4 kHz.
6. For SPU space the area tier does not help; holding exactly the room's reachable set and loading deltas at gates is the smallest footprint. The area tier is a loading group. Layout in section 5: Knight core 98 KB (port rates) to 200 KB (q15), ambience sized per cue instead of the fixed 206 KB region, the rest is one scene region. With the 98 KB core, 260 of 387 traversal rooms hold their whole census-reachable set at q15 and 328 hold it at two thirds of q15 bytes. With the ring freed by XA, 72 of 90 arenas fit at q15.
7. Gate cost: the next room's delta bank is under 0.5 s of CD at 2x in 93% of 219 measured edges at port-level bytes, 63% at q15, 52% at q20. The rest needs the S1 prefetch. SPU upload cost on hardware is unmeasured and is the biggest unknown.

## 1. Trace source

`tools/reference_game.py` (CrossOver) is gone from main (removed in `c928faf`). Its replacement is `tools/hkref`: the native macOS Steam build, `Assembly-CSharp.dll` patched inside a private APFS clone (the Steam install is never written), driver compiled against the game's own assemblies, `AudioListener.volume = 0`, sandbox denies CoreAudio. Read only: Manny's .NET SDK under `.hkpsx/og-reference/deps/dotnet`. Nothing under `.hkpsx`, the bottle or Steam was written.

The driver's `AudioTrace` rewrites the managed call sites of `AudioSource.Play/PlayOneShot/Stop/Pause/UnPause/PlayClipAtPoint` and the mixer calls in `Assembly-CSharp` (62 `PlayOneShot`, 43 `Play`, 31 `Stop` sites, including the PlayMaker audio actions and `HeroAudioController`). Not seen: `playOnAwake` sources and native calls. The looping ambience therefore comes from the static survey (`audiosources.csv`), not from the play logs.

Added on this branch:
- `tools/hkref/mod/Autopilot.cs` and `hkref play PROFILE`: a closed-loop hero on the same virtual device a tape uses. Modes: `fight:Name@X`, `walk:X`, `route:gate,gate,...` (each room is left by its transition gate; a 40 s per-room budget and an off-route recovery use a direct load through the same gate, logged in `autopilot.log`), `talk:N` (walk to each NPC's `Conversation Control`, press Up and Jump). The Knight is healed at 2 HP so a fight is not cut short. Test setups, all in the profiles: teleport into the arena (the False Knight's trigger is a curtain at x 14, y 25 to 44; the Knight stalled at Mawlek's entry in a first attempt), and for one False Knight run enemy HP 5 so the death sequence plays. This is C# inside the in-game driver, like the existing `mod/`; everything else is Rust.
- `host/hk-sfx-trace` (Rust, 5 unit tests): `rates` (FMOD decode through `hk_cook::fmod`, Hann-windowed FFT, out-of-band energy at every SDK ladder rate), `tally` (logs to per-scenario sound sets, calls, peak voices, bytes), `tiers` (area/room split of the census reach), `edges` (delta loads per gate).

Limits: the autopilot is not a player (random jumps, default new-game PlayerData so no abilities, no benches or shops); a room visit is 12 to 40 s; fight length is whatever the autopilot needed; "near" means within 30 world units of the Knight; voice counts assume one voice per request for the clip's length at its pitch (Unity's own virtualisation is not modelled); the rate criterion is necessary, not sufficient (section 4).

## 2. Runs

| Run | What | Game time | Notes |
| --- | --- | ---: | --- |
| survey | 118 scenes: Crossroads_*, Fungus1_*, Town, Room_* | about 8 s per scene with actors | Existing `hkref scenes`: the hero is parked beside each enemy in turn. Also gives actors, FSM audio actions, audio sources, gates |
| RouteCrossroads | 01, 07, 05, 40, 16, 03, 21, 10 | 249 s | 3 rooms left by the 40 s budget, 2 off-route recoveries |
| RouteGreenpath | Fungus1_01, 01b, 02, 06, 07, 19, 10, 30 and others (11 visits) | 245 s | 5 visits hit the budget, 2 off-route recoveries |
| Dirtmouth walk | Town left gate to the well | 30 s | |
| Dirtmouth NPCs | `talk:3`: Elderbug, Gravedigger, Jiji door | 113 s | Only Elderbug's conversation started (3 of his 5 mumbles and the dialogue click) |
| False Knight long | teleport into the arena, 30,000 frames | 498 s | Three armour cycles, boss not killed |
| False Knight short | same, enemy HP 5 | 148 s | Rage x3, `FKnight_death`, the `Boss Defeat` sting |
| Gruz Mother | Crossroads_04, window 600 to 2100 | 25 s | Fight about 17 s to the kill, then the defeat sting. First autopilot version |
| Mawlek | Crossroads_09 teleported to x 74, window 0 to 3700 | 62 s | Full fight and defeat |

## 3. What actually plays

### 3.1 Rooms

Local = sounds only this room (or a few) has. Wide = scenery shared by many rooms (grass, drips, breakables). Knight = persistent set. Bytes are SPU ADPCM (16 B per 28 samples) at the port's 5,512 Hz / at q15.

| Room visit | s | calls | local | wide | Knight | peak voices | peak distinct | local KB | wide KB |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- | --- |
| Crossroads_01 | 17 | 98 | 1 | 17 | 6 | 9 | 9 | 4.2 / 9.9 | 25.8 / 87.3 |
| Crossroads_03 | 31 | 167 | 3 | 10 | 3 | 8 | 8 | 17.3 / 9.9 | 19.3 / 69.1 |
| Crossroads_05 | 28 | 194 | 5 | 14 | 11 | 14 | 10 | 24.0 / 33.3 | 24.3 / 86.8 |
| Crossroads_07 | 40 | 179 | 3 | 8 | 10 | 7 | 6 | 15.7 / 45.6 | 12.2 / 41.0 |
| Crossroads_10 | 40 | 128 | 1 | 5 | 3 | 6 | 5 | 4.2 / 9.9 | 13.5 / 54.0 |
| Crossroads_16 | 29 | 131 | 4 | 13 | 12 | 10 | 8 | 13.3 / 39.6 | 21.0 / 72.8 |
| Crossroads_21 | 24 | 141 | 11 | 6 | 9 | 11 | 10 | 31.6 / 61.1 | 8.4 / 22.5 |
| Crossroads_40 | 40 | 307 | 5 | 20 | 11 | 14 | 12 | 13.6 / 47.2 | 35.2 / 126.1 |
| Fungus1_01b | 19 | 67 | 3 | 9 | 4 | 8 | 6 | 21.4 / 79.0 | 15.7 / 54.9 |
| Fungus1_02 | 40 | 174 | 3 | 6 | 7 | 9 | 8 | 12.4 / 35.8 | 16.5 / 66.0 |
| Fungus1_06 | 40 | 219 | 10 | 8 | 10 | 9 | 9 | 39.3 / 65.6 | 20.0 / 75.2 |
| Fungus1_07 | 40 | 200 | 13 | 13 | 11 | 13 | 10 | 50.9 / 113.0 | 23.4 / 90.1 |
| Fungus1_09 | 12 | 35 | 1 | 6 | 7 | 6 | 6 | 4.6 / 18.2 | 16.5 / 66.0 |
| Fungus1_10 | 40 | 98 | 1 | 4 | 6 | 6 | 6 | 3.8 / 15.3 | 10.4 / 41.7 |
| Fungus1_19 | 10 | 78 | 7 | 8 | 7 | 12 | 11 | 22.6 / 61.5 | 18.8 / 74.9 |
| Fungus1_30 | 40 | 132 | 1 | 7 | 4 | 7 | 7 | 4.6 / 18.2 | 27.6 / 69.2 |

Whole routes. Crossroads: 58 distinct sounds in 249 s (19 local, 24 wide, 15 Knight); 15 sounds carry 90% of the calls (104 KB at q15) and 37 carry 99% (230 KB). Greenpath: 62 distinct (28 local, 20 wide, 14 Knight); 20 sounds carry 90% (154 KB q15), 44 carry 99% (367 KB). Over the 16 visits peak voices are mean 9.3 and max 14; peak distinct sounds mean 8.2 and max 12.

Most frequent, per minute over the whole route (Crossroads / Greenpath): Knight run footsteps 38 / 42 (the grass surface in Greenpath, for which the port has no clip), land soft 34 / 33, jump 30 / 28, zombie footsteps 26, `water_drip_1..4` 10 to 13 each, `grass_move_1..6` 8 to 13 each, `enemy_damage` 12 / 3, `geo_hit_ground_1` 9 / 4, `sword_3` 10 / 4. Everything room-specific beyond these is under 6 per minute and most is under 2.

Dirtmouth: the walk plays 2 local and 2 wide sounds in 30 s. Elderbug's conversation plays `Elderbug_02`, `Elderbug_03`, `Elderbug_first_call` and the dialogue click (`button`, 26 plays); his census set is five mumbles of 2.4 s. The Town census reach is 35 room sounds (210 KB at port rates), mostly NPC and shop content a 30 s walk does not touch.

### 3.2 Played against can play

Over the 16 visits: played local 72 clips, 283 KB (port), 663 KB (q15); FSM-static reach 209 clips, 731 KB (port); census reach 289 clips, 1,076 KB (port; the census figure also counts clips over 6 s). On the 106 surveyed traversal rooms, like for like (Local and Wide, 6 s or less): FSM-static 1,425 clips, 6.2 MB (port), 13.2 MB (q20); census 3,402 clips, 10.6 MB (port), 28.5 MB (q20).

Port coverage of what played, by cook-source mention of the clip name: Crossroads route Knight sounds 12 of 15 clips (598 of 646 calls); room sounds 4 of 17 clips (5 of 119 calls); scenery 2 of 24 (6 of 470 calls). Greenpath: Knight 10 of 14; room 1 of 24; scenery 1 of 20.

### 3.3 Bosses

Windows are fight to defeat. Streamed = clips over 6 s, which belong in XA where the CD is free and not in an SPU bank. KB are SPU-resident clips of 6 s or less.

| Fight | s | calls | local / wide / Knight clips | peak voices | peak distinct | local KB port / q15 / q20 | wide KB port / q15 | streamed |
| --- | ---: | ---: | --- | ---: | ---: | --- | --- | --- |
| False Knight long | 498 | 2,189 | 31 / 7 / 13 | 22 | 11 | 115 / 136 / 180 | 25 / 70 | `FKnight_Rage` 7.2 s |
| False Knight short (HP 5) | 148 | 784 | 34 / 6 / 14 | 20 | 11 | 162 / 211 / 302 | 22 / 60 | Rage, `Boss Defeat` 24.9 s |
| Gruz Mother | 25 | 275 | 10 / 10 / 10 | 24 | 9 | 71 / 95 / 136 | 19 / 69 | `Boss Defeat` |
| Mawlek | 62 | 533 | 16 / 15 / 8 | 16 | 9 | 89 / 129 / 179 | 20 / 68 | `Boss Defeat`, `heart_piece_idle_loop` 9.5 s |

The union of the three False Knight runs is 46 clips of 6 s or less: 203 KB port, 289 KB q15, 388 KB q20. The boss scene's own objects carry 39 such clips (169 KB port, 154 KB q15, 231 KB q20); its census owner set is 42 clips, 88.9 s. Gruz Mother's played union: 20 clips, 91 KB port, 164 KB q15. Mawlek: 31 clips, 109 KB port, 197 KB q15. A boss bank is therefore 150 to 290 KB at q15, and a fight keeps about ten distinct sounds live. For the long False Knight run, 16 sounds carry 90% of the calls in 49 KB (q15) and 32 carry 99% in 135 KB.

Crossroads_10 plus its additive boss scene reach 120 sounds of 6 s or less (492 KB port, 808 KB q15), because conditional content (Shade, journal, soul pickups) hangs off prefabs. The long run does not reach `FKnight_death`, `Fknight_daze` or `boss_stun`; the short run reaches the death and the sting. They are boss-bank members regardless.

## 4. Rates from the decoded clips

`rates.tsv` gives, per clip: peak and RMS dB, the frequency under which 99, 99.9 and 99.99% of the energy lies, the lowest SDK ladder rate whose Nyquist leaves at most -15, -20, -30 or -40 dB of the clip above it (never above the clip's own rate), and bytes at 22,050, at 5,512 and at each of those. 1,866 clips (everything up to 20 s except music), 3,935 s, no decode failures.

| Policy | Whole SFX set | Clips at or under 5,512 Hz |
| --- | ---: | ---: |
| 22,050 Hz cap | 49.6 MB | 0 |
| q40 (-40 dB) | 43.9 MB | 64 |
| q30 | 37.0 MB | 123 |
| q20 | 25.7 MB | 497 |
| q15 | 18.9 MB | 924 |
| port, flat 5,512 Hz | 12.4 MB | all |

At 5,512 Hz the out-of-band energy is worse than -10 dB for 575 clips (31%), between -10 and -15 dB for 364, between -15 and -20 for 423, better than -20 for 504. The port's own notes accept -15 to -26 dB for long low roars, so q15 is "what the port already accepts for its best cases, applied clip by clip". Per-clip rates cost 52% more bytes than flat 5,512 Hz and put them where the energy is: sword slashes, damage and Knight hits want 1.6 to 4 kHz (a slash is 0.4 KB at q15 against 0.7 at port rates); footsteps, Geo, grass, zombies and most impacts want 16 to 22 kHz.

The criterion is energy only. It does not know masking, loudness or how the SPU's Gaussian interpolation colours a rate. The SDK allocator (`psx_audio_cook::rate::allocate`, already used by `scene_sfx::fit`) stays the authority; its job is to fit a tier's budget by lowering the clips where predicted loss is smallest, starting from q15.

Things the decode turned up:
- 37 census clips are rumble waveforms, not audio (`*_vibration`; 29 in the persistent set; 119 KB at port rates, 476 KB at 22,050). They belong to the controller motors. The persistent totals in `CENSUS.md` include them.
- `Boss Defeat` is 24.9 s and counts as an ordinary Local clip in every arena that uses it (313 KB at q15, 137 KB at port rates). It is a sting, so XA.
- The Knight has five footstep surfaces (stone, grass, metal, water, bones), 2 s each (6 to 26 KB). Only stone is in the port. A room needs one run + walk pair: 37 KB at q15, 13 KB at port rates.
- The persistent set without the rumble clips is 158 clips, 269 s: 0.85 MB at port rates, 1.77 MB q15, 2.27 MB q20.

## 5. Tier layout proposal

### 5.1 Principle

Tiers are loading groups. SPU residency should be exactly what the current room can play, because that is the smallest footprint; a gate then costs only the sounds the next room has and the last one lacks (`edges`). The tiers decide where a group lives, when it loads and who owns the bytes.

SPU, 524,288 B:

| Region | Bytes | Lifetime | Basis |
| --- | ---: | --- | --- |
| Base (SDK, capture, silent loop) | 4,112 | always | BUDGET |
| Music ring | 16,384 | traversal; XA arenas free it | census |
| P1 Knight core + Geo | 98,416 at port rates, 200,464 at q15 | resident | 34 clips: footsteps (one surface), jump, lands, falling, sword_1..5, damage, enemy hit and death, Geo, dash, wings, parry, soul pickups, Knight death |
| Surface swap | 12,848 / 37,248 | per room surface (footstep pair) | `rates.tsv` |
| Ambience | per cue, median 120 KB, p90 236 KB (4 kHz) | per room | census; replaces the fixed 206 KB region |
| Scene region | the rest | area end and room end of one pool | below |
| Swap slot | 32 to 64 KB, to be sized | P2 and boss clips on demand | needs the hardware measurement (6) |

At q15 the core is dominated by the footstep pair (37 KB), soul pickups (53 KB), Geo (22 KB), falling (25 KB) and enemy death (12 KB); slashes, damage and dash are almost free. The core need not be q15 everywhere; the allocator trades it against the scene region.

### 5.2 Tiers

| Tier | What | Loaded | Size, port / q15 | Basis |
| --- | --- | --- | --- | --- |
| P1 core | above | boot, save load | 98 / 200 KB | `rates.tsv`, traces |
| P2 ability sets | nail arts, dash, shade cloak, super dash, claw, wings 137 KB q15 (22 clips); spells 167 KB (21 clips); dream nail 172 KB (11 clips); Focus 221 KB (2 clips, of which `focus_health_charging` is a 19.3 s loop to stream) | by unlocked set, at bench rest or pause close | per set | persistent clip groups |
| S surface | footstep pair of the room's surface | with the room | 13 / 37 KB | trace shows five surfaces |
| W scenery | `grass_move_1..6`, grass studs and cuts, `water_drip_1..4`, breakables, platform sounds | with the area, kept | 43 / 153 KB (Crossroads route, 24 clips) | 8 to 13 plays per minute each |
| A area | clips that 25% or more of the area's rooms reach | area gate | Crossroads 99 / 282 KB, Greenpath 97 / 265 KB; most areas 65 to 140 / 165 to 360 KB; Royal Waterways 209 / 577, Colosseum and Godhome larger | `tiers_area.tsv` |
| R room | what the room alone reaches | room gate (delta) | traversal median 38 / 54 KB, p90 112 / 178 KB; whole reach median 88 / 203 KB, p90 169 / 356 KB | `tiers_scene.tsv` |
| B boss | boss and arena sounds of 6 s or less | arena entry | False Knight 154 KB (own objects) to 289 KB (played union) at q15; Gruz Mother 164; Mawlek 197; 90 arenas by census reach: median 100 / 163 KB, p90 210 / 356 KB | boss runs, `tiers_scene.tsv` |
| X stream | `Boss Defeat` 24.9 s, `FKnight_Rage` 7.2 s, `FKnight_death` 5.8 s, other clips over 6 s | XA in arenas | 0 SPU bytes | boss runs |

Area and room are one pool. At a room gate the loader keeps the sounds the next room also reaches, drops the rest and loads the delta; at an area gate it rebuilds the pool. This is the rule `scene_sfx.rs` already follows (first-fit into free gaps, refusals recorded, never trimmed) with the residents shrunk and ambience sized per cue.

### 5.3 Does it fit

Free for the scene region = 524,288 - base - ring - P1 - that room's ambience at 4 kHz, against the room's whole census reach for clips of 6 s or less (an upper bound that includes conditional content).

| Rooms | P1 98 KB | P1 200 KB |
| --- | ---: | ---: |
| 387 traversal rooms: reach fits at q15 bytes | 260 | 161 |
| fits at two thirds of q15 bytes (about the port's byte level) | 328 | 240 |
| 90 arenas, ring and music freed by XA: fits at q15 | 72 | 56 |
| fits at two thirds of q15 | 84 | 74 |

With the port's flat 5,512 Hz for everything and the 98 KB core, 363 of 387 traversal rooms fit. On the surveyed rooms with the tighter FSM-static bound, 103 of 106 fit at port bytes, and 61 of 106 at q20 with a 229 KB core.

Nine traversal rooms have under a third of the q15 bytes they would need even with the small core: Tutorial_01 (King's Pass, 591 KB at q15), Crossroads_ShamanTemple (501), Crossroads_01 (412; its FSM-static bound is 63 KB at port rates), Waterways_09, Ruins_Bathhouse, Town, the Colosseum entry room and two Godhome rooms. Their census reach is mostly conditional or NPC content. Gating emitters on story flags and loading by enemy presence is the lever; the traced rooms played 26% of the reachable bytes.

The fixed 206 KB ambience region is what the present layout wastes. The census gives a per-room median of 120 KB.

### 5.4 Voices

24 hardware voices. Measured: traversal rooms peak at 14 live voices and 12 distinct sounds; bosses at 11 distinct sounds (raw 16 to 24, from Geo showers and repeated hits). Proposal: ring 1, ambience 5, one-shots 12 with one voice per distinct sound (a new play of a clip retriggers it), Knight loops (run, focus, super dash) 3, UI 1, spare 2. In an XA arena the ring voice returns, so 13 one-shots. Capping Geo showers (the 24 had 18 `geo_hit_ground_1` overlapping) is worth more than extra voices.

### 5.5 Loading points

2x CD is 307,200 B/s, so 0.5 s is 153.6 KB.

| Point | What loads | Cost | Notes |
| --- | --- | --- | --- |
| Boot, save load | P1, surface, UI | outside play | |
| Area gate | A, W, ambience | Crossroads 99 / 282 KB = 0.3 / 0.9 s | Load first the sounds that carry 90% of the calls (Crossroads route: 104 KB at q15), the rest after the gate in the background; or prefetch with the area's last room |
| Room gate | R delta, ambience delta | 219 measured edges: delta median 60 / 124 KB, p90 133 / 236 KB, max 314 / 531 KB (port / q15); under 0.5 s in 93% (port) and 63% (q15); the whole reach would be under 0.5 s in 16% | The rest needs the S1 prefetch: stage the next room's bank in RAM, upload at the gate |
| Boss arena entry | B bank (150 to 290 KB q15) with the arena room | 0.5 to 1 s | The XA mode switch costs about 1 s each way on silicon (v1.28). Do both behind the boss intro (the False Knight drops from the ceiling after the trigger) |
| Boss defeat | nothing | | the sting is XA |
| Bench rest, pause close | P2 swap by unlocked set | free (game paused) | |

The 0.5 s ceiling holds at the port's byte level, not at q15 without prefetch. The SPU upload itself is the unknown: `psx_spu::upload_adpcm_unguarded` is programmed I/O, and one scene-load tick of it spans 754,064 bus cycles with 493,004 MMIO stall cycles against 571,236 per VBlank (`docs/KNOWN-ISSUE-INPUT-GAP.md`). Moving 124 KB at a gate over that path is several frames of CPU time. The proposal needs SPU DMA and a throughput measurement on the console before the gate budget is believable.

## 6. Open and next

- Hardware: SPU DMA throughput and the swap slot size. Nothing here is measured on the console; gate budgets count CD time only.
- Trace depth: 12 to 40 s per room, one autopilot seed, default abilities. A human tape through the same driver (`hkref og` already takes tapes) and ability PlayerData would fill in P2 and cover benches, shops and vertical rooms the autopilot leaves by budget. The survey ran 118 of 501 scenes; the other areas are one profile away.
- Rate quality: run the SDK allocator at these tier budgets and score or listen. The numbers say how much room there is, not what it sounds like.
- Conditional emitters: list which census reach comes from story-gated prefabs (Grimm, Shade, the King's Pass Flamebearer) so the cook can drop them per flag state.
- Reclassify `Boss Defeat` and other long clips as stream in the census, and take the rumble clips out of the persistent set.
- Port gap the trace exposes: scenery (grass, drips, breakables), four of five Knight footstep surfaces, `geo_hit_ground`, `acid_splash` and most room sounds are in no bank. They are the most frequent sounds in every room.

## Reproduce

```
cd tools/hkref && cargo build --release --offline
HKREF_DOTNET=<dotnet root> hkref --work W build
hkref --work W scenes profiles/sfx-survey-slice.json            # 118-scene survey
hkref --work W play   profiles/sfx-play-route-crossroads.json   # also -greenpath, -town, -dirtmouth-npcs, -mawlek, -gruz, -false-knight, -false-knight-short
cd host && cargo build --release -p hk-sfx-trace                # always set CARGO_TARGET_DIR
HK_FMOD_LIB=<fmod_toolkit libfmod> hk-sfx-trace rates <Windows hollow_knight_Data> ../docs/audio-census rates.tsv 20
hk-sfx-trace tally ../docs/audio-census rates.tsv out manifest.txt
hk-sfx-trace tiers ../docs/audio-census rates.tsv out
hk-sfx-trace edges ../docs/audio-census rates.tsv W/runs/sfx-survey-slice/scenes.csv out
```
