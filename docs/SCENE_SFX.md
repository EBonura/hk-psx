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
- Playback is on SPU voice 11 (`ambience::SCENE_SFX_VOICE`), which left the
  ambience pool for this: the admitted cues keep at most four stems audible
  across a transition, and `host/ambience.py` refuses a catalogue that needs
  five. A new play retriggers the voice.

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
