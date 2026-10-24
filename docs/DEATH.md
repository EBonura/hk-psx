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
| Death: burst | `Hero Death Anim` `Initiate`: `EnemyKillShake`, `FreezeMoment`, eight of each slash in every direction | `hit_fx::death`, the same burst |
| Death: sounds | `hero_death_v2` and `hero_damage` together | `audio::hero_death` (4 kHz world bank clip) after the hurt clip |
| Death: body | renderer off, the Hero Death object plays `Death` (18 frames at 10 fps), ash particles, the nail and the head are thrown, then the mesh renderer goes off | the `Death` clip where he fell, 108 ticks, then nothing; ash, nail and head not presented |
| Death: bookkeeping | Remove Geo, Set Shade, save | unchanged (frame.rs, docs/SHADE.md; the card is still only written by the player's "Save game?") |
| Death: fade | `PlayerDead`: wait `DEATH_WAIT` (171 ticks), camera fade out (0.33 s), wait 0.8 s, `ReadyForRespawn`, load | `Game::death_shade`: the fade runs ticks 171 to 191, black to 219 (`DEATH_HOLD_TICKS` 48), then the respawn block and its load, which continues from black (`exit_fade::arm(255, 1)`) |
| Respawn at a bench | camera `RESPAWN` fade in; `Bench Control` `Init Resting`: `Sit Fall Asleep` 1.2 s, `Startle`: `Wake To Sit` (9 frames at 12 fps), then resting until input | `bench::State::respawn`: `Respawned` finds the bench, `Asleep` 72 ticks with the screen fading in over 20, `Waking` 45, then seated like after a rest |
| Respawn at the new-game spawn | `HeroController.Respawn` plays `Wake Up Ground` | `Lying` 83 ticks (the 25 identical frames cooked once), `Rising` (20 frames at 18 fps), then control |
| Respawn at a marker that is no bench (seeded test cards) | error in the original | `Asleep`, `Waking`, then standing |

Open: the slash scale (the FSM's `Scale Min/Max` 1 to 1.75 is applied on top of the prefab's 2; not confirmed against
the original side by side), the death sound's 4 kHz stand-in, `Hit Crack`/`Hit Pt` and the thrown nail and head
(Unity Animator and particle systems, which need the break-effects cooker, a full recook).

## Cost

The art is five Knight clips and two slash clips appended to the ability art (`host/ability_art.py`
`DEATH_CLIPS`, `EFFECT_LIBRARY_CLIPS`, `WAKE_GROUND`): 61 frames, 10,584 linked bytes, in the existing five palette
rows (the slashes join the additive Focus sheet; every frame fits a 64x64 slot, which the cooker now asserts).
The slashes are cooked at a third of their length and full height. The code is `hit_fx.rs`, the `bench.rs`
phases and the death pose and fade in `frame.rs`. Free RAM below the module pool went from 23,756 to 4,660
(19,096 bytes: code +5,964, linked data +13,120, BSS +8; docs/BUDGET.md).

## Pins

`HK_HIT_FX_BURSTS` (hits plus deaths), `HK_BENCH_WAKES` (death respawns that woke asleep), `HK_GROUND_WAKES`
(that got up from the ground) and `HK_DEATH_HOLD_TICKS` (the black ticks before the load) are pinned by
`kings-death` (ground), `husk-guard` and `boss-death` (asleep).
