# The Knight's hit, death and respawn

What the original does when the Knight takes a hit, dies and comes back, what the port does, and where each
part lives. Source records are from the installed Windows build (`resources.assets`: `HeroController`,
the `Knight Damage`, `Hero Death Anim` and `Bench Control` FSMs).

## The sequence

| Step | Original | Port |
| --- | --- | --- |
| Hit: recoil, hit stop, invulnerability | `StartRecoil`: recoil velocity, `FreezeMoment`, `INVUL_TIME` | `Vitals::hurt` (docs/DAMAGE.md) |
| Hit: camera, sound | `AverageShake`; `hero_damage` through the audio controller | `camera::Shake::Average`; `audio::hurt` (`hero_damage_less_harsh`) |
| Hit: vignette | `DAMAGE` to the vignette | the approved hurt pulse |
| Hit: slashes | two or three `Slash Effect GhostDark1` and `2` each side, 20 to 35 u/s, left 140 to 220 degrees, right within 40 | `hit_fx.rs`: same counts, speeds and angles, the clips' own frames, additive |
| Hit: `Hit Crack`, `Hit Pt 1/2` | Unity Animator sprite (scale 2.1, random turn) and two particle systems | not presented (not tk2d clips; the tk2d `Hit Crack` and `Death Crack` clips are one-pixel placeholders) |
| Hit of 2: `takeHitDouble`, last-HP burst, leak steam, `DAMAGE LOWPASS` | Unity Animator, particle systems, audio snapshot | not presented |
| Death: hit stop | `Initiate` calls `FreezeMoment`: the time scale is 0 for 0.35 s of real time, then eases back over 0.1 s | `Vitals::hurt` freezes the world for `DEATH_FREEZE_TICKS` (24); the 171-tick death wait starts when it ends |
| Death: burst | `Hero Death Anim` `Initiate`: `EnemyKillShake`, eight of each slash in every direction (and a white flare that is not scaled by the time scale) | `hit_fx::death`, the same slashes (frozen with the world during the hit stop); the flare is not presented |
| Death: sounds | `hero_death_v2` and `hero_damage` together | `audio::hero_death` (4 kHz world bank clip) after the hurt clip |
| Death: body | renderer off, the Hero Death object plays `Death` (18 frames at 10 fps), ash particles, the nail and the head are thrown, then the mesh renderer goes off | the `Death` clip where he fell, 108 ticks, then nothing; ash, nail and head not presented |
| Death: bookkeeping | Remove Geo, Set Shade, save | unchanged (frame.rs, docs/SHADE.md; the card is still only written by the player's "Save game?") |
| Death: fade | `PlayerDead`: wait `DEATH_WAIT` (171 ticks of game time), then `RESPAWN FADE` (the camera's `RespawnFadingOut` waits 0.75 s), `ReadyForRespawn` 0.8 s after the fade starts, load | `Game::death_shade`: the fade runs 45 ticks from the end of the wait (`DEATH_FADE_TICKS`), black for the last 3 of the 48 (`DEATH_HOLD_TICKS`), then the respawn block and its load, which continues from black (`exit_fade::arm(255, 1)`) |
| Respawn at a bench | camera `RESPAWN` fade in; `Bench Control` `Init Resting`: `Sit Fall Asleep` 1.2 s, `Startle`: `Wake To Sit` (9 frames at 12 fps), then resting until input | `bench::State::respawn`: `Respawned` finds the bench, `Asleep` 73 ticks with the screen fading in over 36 (the camera's `FadeIn` waits 0.6 s), `Waking` 45, then seated like after a rest |
| Respawn at the new-game spawn | `HeroController.Respawn` plays `Wake Up Ground` | `Lying` 83 ticks (the 25 identical frames cooked once), `Rising` (20 frames at 18 fps, then the last frame's ticks: control returns 153 ticks after the clip starts, as `Wake Up Ground` completes) |
| Respawn at a marker that is no bench (seeded test cards) | error in the original | `Asleep`, `Waking`, then standing |

Open: the slash scale (the FSM's `Scale Min/Max` 1 to 1.75 is applied on top of the prefab's 2; not confirmed against
the original side by side), the death sound's 4 kHz stand-in, `Hit Crack`/`Hit Pt` and the thrown nail and head
(Unity Animator and particle systems, which need the break-effects cooker, a full recook).

## Timeline against the original

Measured on the original with hkref (tools/hkref; Crossroads_04, one mask, walking into a Zombie Barger; the
Knight dies, respawns at the new-game spawn or on the Town bench; PlayMaker trace of `CameraFade`,
`Hero Death Anim` and `Bench Control`) and on the port with the route replays (`kings-death` for the ground wake,
`boss-death` for the bench wake). Ticks are 60 Hz; the original's frames are 1/60 s of game time except in the
hit stop, whose length is real time (0.35 s plus the 0.1 s ease: 22 to 24 ticks lost across runs, 24 at a steady 60 frames a second).
The original's load is its own (109 to 190 frames on a Mac) and is not compared: the port respawns in the scene
it is in without a load, and loads from the disc otherwise.

| Phase (from the killing hit) | Original | Port before | Port now |
| --- | ---: | ---: | ---: |
| Hit stop | 24 | 0 | 24 |
| Death wait to the fade | 171 | 171 | 171 |
| Fade out (`RESPAWN FADE`) | 45 | 20 | 45 |
| Black before the load | 3 | 28 | 3 |
| Kill to `ReadyForRespawn` | 243 | 219 | 243 to 244 |
| Fade in after the load | 36 | 20 | 36 |
| Bench: `Sit Fall Asleep` until `Startle` | 73 | 72 | 73 |
| Bench: `Wake To Sit` | 45 | 45 | 45 |
| Bench: respawn to control | 119 | 117 | 119 |
| Ground: respawn to control (`Wake Up Ground`) | 154 | 151 | 153 |
| Kill to control, bench, no load | 362 | 336 | 362 |
| Kill to control, ground, no load | 397 | 371 | 396 to 397 |

So the port was 26 to 27 ticks shorter than the original over the whole sequence, not longer: it lacked the hit stop of the
killing blow, faded out in 20 ticks and sat black for 28 where the original fades for 45 and is black for 3, faded in
over 20 where the original takes 36, and gave control back 3 ticks before `Wake Up Ground` completes.
Still different: the white flare of the death burst and the thrown nail, head and ash (not presented); the original's
load; the Death clip's body is the same 108 ticks.

## Cost

The art is five Knight clips and two slash clips appended to the ability art (`host/ability_art.py`
`DEATH_CLIPS`, `EFFECT_LIBRARY_CLIPS`, `WAKE_GROUND`): 61 frames, 10,584 linked bytes, in the existing five palette
rows (the slashes join the additive Focus sheet; every frame fits a 64x64 slot, which the cooker now asserts).
The slashes are cooked at a third of their length and full height. The code is `hit_fx.rs`, the `bench.rs`
phases and the death pose and fade in `frame.rs`. Free RAM below the module pool went from 23,756 to 4,660
(19,096 bytes: code +5,964, linked data +13,120, BSS +8; docs/BUDGET.md).
The Death clip and the respawn clips are not linked (next section): with them streamed, free RAM below the
module pool is 12,480 on the draw-order texture packing base (code +7,496 over the 23,756 baseline, linked data +3,712 of which 1,856 are the packing's atlas rows).

## Streamed art

The Knight's Death clip (18 frames) and respawn clips (Sit Fall Asleep, Wake To Sit, Wake Up Ground Lie and
Rise, 32 frames) live on the disc as two carried pool packages, `art_death` and `art_wake` (the same mechanism
as the Shade's art, `art_shade`; `host/code_modules.py` `CARRIED`, `game/src/modules.rs` `carry`). The cooker
(`host/ability_art.py` `STREAM_PACKS`) writes each pack as a table of 28-byte frame records followed by the
coded texels (`data/art-death.hk`, `data/art-wake.hk`); the clip table, the palette rows and the animation keys
stay linked, so the draw path is unchanged. `ability_art::ready(clip)` says whether a clip can be drawn and
`locate` reads a frame's record from the resident pack. When a pack is not resident the Knight is simply not
drawn for that tick (`HK_STREAM_ART_LATE` counts a Death clip that was due and not there; the pin is 0). The
pool holds at most the three largest carried packages at once (the host pool-fit check).

What asks for them (`Game::carry_streamed_art`, once a frame):

* `art_death` is carried into the current scene while a hit could kill (health plus Lifeblood at most 4, the most
  one hit takes) or a boss fight is on, and until the clip ends. A background pool fetch reads it when the
  drive is free. A boss song takes the drive for the whole fight, so `music::boss_track` holds the song until
  `modules::service` has planned what the fight carries and `modules::fetch_pending` (also true while a
  carried package of the current scene is not here yet) lets it go: the Shade's art, then the Death clip, then
  the respawn clips are read in the moments before the song starts.
* `art_wake` is carried into the scene he wakes in from the moment the respawn load starts
  (`frame.rs`, the respawn branch calls `carry_streamed_art` again), so that gate keeps it or reads it with the
  scene's own art; in a boss fight it is read with the Death clip, otherwise as soon as the Death clip is over,
  in the room he died in. `bench.rs` waits one tick for it if it is somehow still missing (the screen is black).

Measured on the routes (before the hit stop and the longer fades): death to respawn takes no longer than with the linked art (221 ticks on kings-death, 220
on boss-death, 221 and 220 on husk-guard before and after) and the frames match the linked art's to the pixel except for ambient noise and
one-tick offsets of fades and animations; the hit frames are identical.

The Knight's ability effects are two more packs, `art_fx` and `art_fx_spell` (docs/ABILITY_EFFECTS.md): carried into a scene (and into the scene a gate leads to, as soon as he has chosen the gate) while he owns an ability that draws one of their effects and the scene's row of `HK_FX_FIT` says the pack fits there. They are optional in the pool-fit check, which counts only the Shade's, the Death clip's and the respawn clips' packages.

To stream another group of frames (hk-effects' cocoon, door dust and grub jar): cook the clips after the last pack's
in `ability_art.py` and add a row to `STREAM_PACKS`, add a module of that name to `CARRIED` in `code_modules.py`
and a `STREAMS` entry in `ability_art.rs`, and call `modules::carry(module, scene)` from whatever knows the
moment is coming (a cocoon room, a door about to open) a few seconds ahead. Keep to what fits: the pool-fit check
fails the build if the worst room plus the three largest packages no longer fits.


## Pins

`HK_HIT_FX_BURSTS` (hits plus deaths), `HK_BENCH_WAKES` (death respawns that woke asleep), `HK_GROUND_WAKES`
(that got up from the ground) and `HK_DEATH_HOLD_TICKS` (the black ticks before the load) are pinned by
`kings-death` (ground), `husk-guard` and `boss-death` (asleep).
