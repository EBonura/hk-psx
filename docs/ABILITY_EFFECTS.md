# The Knight's ability effects

What the original draws around the Knight when he uses an ability, which of it the port presents, and
where each piece lives. Source records are from the installed Windows build (`resources.assets`: the
`Knight` prefab's `Effects` and `Spells` children, `HeroController`, `Spell Control`).

| Ability | Original | Port |
| --- | --- | --- |
| Mothwing Cloak dash | `HeroDash`: `Dash Burst` plays `Dash Effect` (8 frames at 25 fps, a child at (4.11, -0.55) mirrored behind him), `Dash Ash` emits 200 a second while the dash runs | `ability_fx.rs`: the `Dash Effect` clip from the `art_fx` pack, and the `Dash Ash` particle system as the Knight's owners `0xFFFD`/`0xFFFC` (one side each), a burst every sixth tick |
| Dash on the ground | `backDashPrefab` (`Hero Dash Puff`, dust, bone and grass bursts) at his feet | not presented |
| Shade Cloak dash | `Shadow Burst`, `Shadow Ring`, `Shadow Dash Blobs`, `Sharp Shadow` | not presented (the body pose and the sound are) |
| Mantis Claw wall slide | `Wallslide Dust`: 100 a second while he slides | owner `0xFFFB`, ten particles every sixth tick of a slide |
| Mantis Claw wall jump | `Wall Puff` (6 frames at 20 fps) | `art_fx` clip `Wall Puff`, at the Knight |
| Monarch Wings double jump | `Double J Wings` (`Double Jump Wings 2`, 6 frames at 16 fps), `White Flash DJ` (holds 0.125 s, fades in 0.4 s), `Double J Feather` (moth feathers, 65 a second for 0.4 s) | the wings and the flash from `art_fx`; the feathers are not presented (their texture is not in a scene's particle art) |
| Vengeful Spirit | `Fireball Top`: `Fireball Blast`, `particles`, `white_light`; the ball sheds soul orbs (80 a second); `Wall Impact` when a wall stops it, `Ball End` when it expires | `Fireball Blast` and `Wall Impact` from `art_fx`, `Ball End` (linked art); the orbs are not presented (a cloud of about thirty more quads in a spell fight, which tipped the Baldur fight from 2 to 3 vblanks a frame on 150 of its frames) |
| Focus, heal | Lines Anim, Burst, White Flash R, Dust, Can Focus Particles | already presented (`focus_fx.rs`) |
| Desolate Dive, Howling Wraiths, nail arts | `Q Slam`, `Scr Heads`, `Cyclone Slash`, `Great Slash`, `Dash Slash` | the abilities are not in the port |
| Crystal Heart | the `SD *` effects | not presented (the body poses are) |

## How it is built

The clips are cooked by `host/ability_art.py` (`FX_CLIPS`) as the third streamed pack, `art_fx`
(`data/art-fx.hk`, a carried module like the Death and respawn art, docs/DEATH.md "Streamed art"). Every
palette row of the VRAM block is taken, so the frames carry none: each texel's premultiplied brightness is
fitted to the grey ramp of the bright palette (the Burst's), and the guest draws them adding with a tint
(`ability_art::draw_light`). The pack stays within three sectors, so the pool-fit check, which holds the three
largest carried packages at once, does not grow.

The guest (`game/src/ability_fx.rs`) raises an effect on the state edge that starts it (`on_edges`, the
cast, the ball's end), ages it every tick, and draws it at the Knight (children of the Knight prefab) or at the
place it was spawned (the spell effects, facing the way they were cast). `Game::carry_ability_art` asks for the
pack into the scene he is in, and into the scene a gate leads to as soon as he has chosen it, whenever he
owns an ability that draws one. An effect whose pack has not arrived is not raised (`HK_FX_LATE`; the pool holds three carried packages at most, so while the Shade's art, the Death clip and the respawn clips are all carried, in a boss fight lost in the Shade's own room, the pack waits for one of them to leave), and the
effects are the first thing in a frame to go undrawn when the animation cache's slots are spoken for.

The dust is the break-effects cooker's (docs/BREAK_EFFECTS.md): the Knight's owners are cooked into every scene
(`host/hk-cook/src/break_effects.rs`, `HERO_*_OWNER`), at no resident cost beyond the style and emitter rows.

## Pins

`HK_FX_RAISED` (effects raised) and `HK_FX_LATE` (raised while their art was absent, pinned at zero) on the
four ability routes: `cheat-dash`, `cheat-wings`, `cheat-claw` and `cheat-spell`.
