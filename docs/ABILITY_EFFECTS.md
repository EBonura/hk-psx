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
| Monarch Wings double jump | `Double J Wings` (`Double Jump Wings 2`, 6 frames at 16 fps), `White Flash DJ` (holds 0.125 s, fades in 0.4 s), `Double J Feather` (moth feathers, 65 a second for 0.4 s) | the wings from `art_fx`; the flash as a small round glow of blue light (one cooked frame, 3.2 units across, held 8 ticks and faded over 24); the feathers are not presented (their texture is not in a scene's particle art) |
| Vengeful Spirit | `Fireball Top`: `Fireball Blast`, `particles`, `white_light`; the ball sheds soul orbs (80 a second); `Wall Impact` when a wall stops it, `Ball End` when it expires | `Fireball Blast` and `Wall Impact` from `art_fx_spell`, `Ball End` (linked art); the orbs are not presented (a cloud of about thirty more quads in a spell fight, which tipped the Baldur fight from 2 to 3 vblanks a frame on 150 of its frames) |
| Focus, heal | Lines Anim, Burst, White Flash R, Dust, Can Focus Particles | already presented (`focus_fx.rs`) |
| Desolate Dive, Howling Wraiths, nail arts | `Q Slam`, `Scr Heads`, `Cyclone Slash`, `Great Slash`, `Dash Slash` | the abilities are not in the port |
| Crystal Heart | the `SD *` effects | not presented (the body poses are) |

## How it is built

The clips are cooked by `host/ability_art.py` (`FX_CLIPS`) as the last two streamed packs, `art_fx` (dash, wings,
the double jump's glow, wall puff) and `art_fx_spell` (cast blast, wall impact): carried modules like the Death and
respawn art (docs/DEATH.md "Streamed art"). Each frame is cut at `density` screen pixels a texel at the size it is
drawn (1.0, the Knight's own, for the dash, the wings and the puff; 1.7 for the cast blast and 1.35 for the wall
impact, whose frames are screen-sized), its colour premultiplied by its alpha before the resize, faded to black over
2.5 texels at the frame's edge (so no quad shows a rectangle), and cut into parts of at most 64x64 texels, one
animation slot each; a part is cropped to the texels that carry light and dropped when none does. The texels are
indices of the bright palette row (CLUT y487), refitted on the effects' own colour (the original's navy-to-white ramp
with a violet rim) and, at a sixth of the weight, on the Focus Burst and Soul Burst frames that share it. The first
cooked version stored them as a grey ramp tinted by the guest, at 2 to 5 screen pixels a texel (the pack had to stay
inside three sectors), which showed as blocks and clipped cores.

What the density costs: every part of a frame is an animation-cache miss, uploaded through the GPU port when the
effect frame changes (about every second tick). Vengeful Spirit's blast and impact at 1.25 pixels a texel (four parts
a frame) took the Baldur fight (route baldur-spell, 54 casts) from 3.54 to 3.55 vblanks a frame (frame time 203
against 202); at 1.7 and 1.35 they are two parts a frame and the route is back at 202. The packs are 13,429 and
11,703 bytes.

Tone (`FX_TONE`, host/ability_art.py): each premultiplied value v becomes `255 * gain * (v / 255) ** gamma` before the
frame is cut. The effects add to scenes that are bright blue in places, so the soft haze of the wings (gamma 1.9, gain
0.8) and the wall impact's core (gamma 1.4, gain 0.72) saturated to a white disc, and the puff (gamma 0.7, gain 1.8), a
pale dust, added nothing visible to a lit wall. The double jump's blue glow is dimmer (24, 40, 96 at its centre).

A cooking bug that the first version carried: `fx_parts` reused the loop variables `tx` and `ty` for the lit box of a part,
so every part after the first in a row of parts was cut from rows shifted by the first part's margin. Frames of two
parts side by side (the wings in their wide frames, the blast) lost the tops of their feathers and the wall impact at
four parts showed blocks where parts overlapped; the part corners are now their own variables.

The spell's blast and wall impact are drawn after the front scenery (`ability_fx::draw_world`, frame.rs): the original
draws them over the floor of the Elder Baldur's bridge and the arches below it (checked against the original with
`tools/hkref`, same room, cast with 99 SOUL), and the port's floor strip drawn after the effects hid the middle of the
fan and left only its lower sparks showing under it. The effects that follow the Knight stay a hair behind him.

Those sizes are more than the three sectors the pool-fit check counts per carried package, so the two packs are
*optional* (`host/code_modules.py` `OPTIONAL`): the build's guarantee (the worst room beside the three largest of the
Shade's art, the Death clip and the respawn clips) leaves them out. `hk-module-publish` writes `HK_FX_FIT` per
manifest scene: which of the two packs fit beside the scene's own modules and the Death and respawn clips, and which
fit beside the Shade's art too. The guest asks for a pack only where it fits (`modules::fx_fit`, frame.rs
`carry_ability_art`; the Shade's row while its art is wanted there), so the guarantee is never spent on them. Where a
pack does not fit the effect is not drawn (`HK_FX_LATE` counts the casts that wanted one).

The guest (`game/src/ability_fx.rs`) raises an effect on the state edge that starts it (`on_edges`, the cast, the
ball's end), ages it every tick, and draws every part of its frame at the Knight (children of the Knight prefab) or
at the place it was spawned (the spell effects, facing the way they were cast). `Game::carry_ability_art` asks for
each pack into the scene he is in, and into the scene a gate leads to as soon as he has chosen it, whenever he owns
an ability that draws one of its effects. The effects are the first thing in a frame to go undrawn when the animation
cache's slots are spoken for (`request`). The parts are decoded straight into the GPU port a word at a time
(`game/src/unpack.rs`, with its own test).

The dust is the break-effects cooker's (docs/BREAK_EFFECTS.md): the Knight's owners are cooked into every scene
(`host/hk-cook/src/break_effects.rs`, `HERO_*_OWNER`), at no resident cost beyond the style and emitter rows.

## Pins

`HK_FX_RAISED` (effects raised) and `HK_FX_LATE` (raised while their art was absent, pinned at zero) on the
four ability routes: `cheat-dash`, `cheat-wings`, `cheat-claw` and `cheat-spell`.
