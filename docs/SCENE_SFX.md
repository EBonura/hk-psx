# Per-scene one-shot banks

Sounds only some scenes need (a bench, a secret, an arena gate, an enemy
family's voice, a boss's clips) live in a per-scene bank instead of the
resident banks, which are full. The model is ambience's.

## How it works

- `host/hk-cook/src/scene_sfx.rs` holds the catalogue, `EVENTS`: one row per sound, with
  its source clip, rate, where it is needed and a priority. For each scene it
  admits the events its content asks for in priority order, placing each clip
  first-fit into SPU bytes that no ambience stem able to sound in that scene
  uses (the scene's cue plus the stems of every scene one gate away, which the
  ambience prefetch may load and which cover the cue fading out after a gate).
  A clip that does not fit is refused whole and listed in
  `.hkpsx/scene-sfx.json` with its size, and in the cook's printout.
- Each scene's bank is one WORLD.PAK chunk, first in that scene's disc group,
  so the gate load reads it with the scene and no extra seek.
  `disc::admit_scenes` checks it and `scene_sfx::upload` writes each clip to
  its cooked address after the scene's ambience stems are in. A resident stem
  the bank lands on is forgotten (`ambience::forget`) and reloads when a cue
  next wants it.
- Playback is on a pool of five SPU voices shared by weight (`audio::POOL_VOICES`:
  11, ambience's `SCENE_SFX_VOICE`; 13 and 14, the Geo rocks'; 10, which ambience no
  longer needs since no transition keeps four stems audible; 16, the hard landing's).
  The Geo rock clips and the hard landing play on it too. A new sound takes a voice
  whose clip has ended (its length in simulation ticks from its bytes and pitch);
  when all five are sounding it cuts the lightest one (`scene_sfx::weight`: ambient,
  world, enemy, boss; the oldest of equals), never one heavier than itself, which
  is then not played and counted in `HK_POOL_DROPS` (cuts in `HK_POOL_CUTS`). A
  loop (`loop_tick`) holds its voice until it stops or a heavier sound takes it.
  `host/hk-cook/src/ambience.rs` refuses a catalogue whose cues need more than
  ambience's three pool voices (7, 8, 9) across a transition.

## The spell block

Desolate Dive's and Howling Wraiths' sounds (`hero_quake_spell_prepare` 3,000 Hz, `hero_quake_spell_impact` 3,000 Hz,
`hero_scream_spell` 5,512 Hz; 960 + 3,280 + 9,568 = 13,808 bytes, rates and measured losses beside their rows) can sound in
any scene, and a row in every scene's bank costs twelve bytes of resident RAM a scene. They are `["spells"]` rows, admitted to
no scene, cooked into one block at one SPU address: the highest address the most scenes leave free after their stems and
banks (`spell_block`; today 0x4ABC0, the top of the ambience region, free in 62 of 66 scenes: not Tutorial_01, Crossroads_04,
Crossroads_09 or Crossroads_10, whose banks use it). The block rides at the end of the chunk of the free scenes the Knight
can enter with it gone: those a gate away from a scene that is not free, and the bench scenes he continues or respawns at
(14 today, `BANKS` flag `SPELL_CARRIED`). `upload` sends it only when it is not in SPU already; a bank that uses its bytes
(`SPELL_CLOBBERED`) or any stem written over them (`overwritten`) clears `SPELL_READY`. While it is set `play` finds the
three in `SPELL_ROWS`; otherwise they miss, and the caller plays the stand-in. `.hkpsx/scene-sfx.json` `spells` lists the
address, the scenes where it is free and the carriers.

## API (guest)

```rust
crate::scene_sfx::play(crate::scene_sfx::FALSE_KNIGHT_STRIKE_GROUND);
```

Event constants are generated into `data/scene_sfx.rs` from the catalogue
names, upper-cased. `play` is a no-op when the resident bank does not hold the
event (not cooked for this scene, or refused for room); those calls are counted
in `HK_SCENE_SFX_MISSED`, and plays per event in `HK_SCENE_SFX`.

To add a sound: add a row to `EVENTS` (clip file, path id, name, rate,
`('scene', name)` or another selector, priority), rebuild, and call `play` at
the source state that plays it. Check the cook's printout for the scene's
remaining bytes and any refusal.

## Room (measured on the current cook)

Free SPU per scene, before any bank (sum of gaps / largest gap):
King's Pass, Dirtmouth and Crossroads_01 27,552 / 16,656; Crossroads_04 and
Crossroads_50 16,656; Crossroads_11_alt 33,568; most Crossroads scenes,
Crossroads_10 (the False Knight) included, 56,432 / 39,776; Sly's shop and
Crossroads_46b 176,800 and more.

The False Knight's bank now holds thirteen of the fight's and arena's clips in
Crossroads_10's 56,432 bytes, with 1,168 left: the slam's
`false_knight_strike_ground`, the rage roar `FKnight_Rage`, `boss_final_hit`,
the jump, the heavy landing, the stagger's armour crack, the roll, both arena
gates, the dying roar `FKnight_death`, `false_knight_ceiling_break` and the two
armour creaks. Only the Run footstep is refused (1,168 bytes, one byte-run
short across two gaps). What made room is per-clip rates and tails rather than
a house rate: each rate is the lowest that keeps nearly all of that clip's
energy (the long, low roars and impacts lose -15 to -26 dB above 1.5 kHz, so
they play at 3,000 Hz), and a row may cut its tail (`trim_tail`: at -40 dB
below the peak, or at a fixed length where the source cuts the clip short
itself, as `Steam` does the dying roar at 3.0 s). The catalogue carries
each clip's measured loss beside its row, and `.hkpsx/scene-sfx.json` each
clip's kept length and the energy its trim dropped. `Boss Defeat`, the 25 s
defeat sting, is still out: 56,944 bytes at 4,000 Hz is the whole bank.

The two roars play on the resident banks' shared voice (`play_shared`), not
the scene voice, so the rage's slams, which follow the roar within a second,
do not cut it off.
