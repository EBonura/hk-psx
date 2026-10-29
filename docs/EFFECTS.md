# Set-piece effects (fx owners)

What the original does when the Knight strikes something that is not an ordinary Breakable is mostly
spawned prefabs: particle systems, flung rigid sprites, tk2d clips. This port draws the first two through
the break-effect particle pool (`game/src/particles.rs`, `break_effects.rs`) and cooks them in
`host/hk-cook/src/break_effects/fx.rs`. Nothing here is a second particle system.

## How it works

- The cook reads the original's own data. A particle prefab (`Door_dust`, `Dust Hit Med`, `Rubble Fall Short`) or a
  particle system of the scene (a grub jar's `Pt Glass L` and `Pt Glass S`) goes through the same reader as every
  Breakable's debris. A rigid sprite object the original flings (`Particle Rock Small`, `Spatter Blue`,
  `Blue Glob`, the cocoon's cap) becomes a `Burst`: one style whose particles collide with the terrain like the
  rigid bodies do, cut from the object's own sprites (`sheet`).
- Each effect is an emitter of an owner from `FX_OWNER` (0xD000) up (`game/src/fx.rs` mirrors them). They are
  optional in a scene's art budget, like the stalactites': a scene whose effect art has no room goes without,
  never loses a style of its own, and raising the owner then spawns nothing.
- Emitters are cooked at their anchor's origin (the nail's hit, the jar, the cocoon) and raised where the anchor is;
  the Great Door's are cooked in place (offset zero).
- A style may carry a splash (`Style::splash`, bytes 166..167 of the style record): the particle that lands stops
  where it hit, shows another frame of the sheet turned to the surface and lives a few ticks more. Bit 15 of the
  same word makes a burst fly on outside the region's collision coverage instead of pausing there.
- A sprite that never fades is cooked `opaque`: it keeps one opacity level (the densest) instead of three, which is
  a third of its video memory. It shrinks away at the end of its life, as a rigid piece's FinishingRigidBody does.

## Owners

| Owner | What | Source ids |
|---|---|---|
| 0xD001 HIT_DUST | `Dust Hit Med` at the nail (half the puffs) | sharedassets6:500 |
| 0xD010 DOOR_DUST | Great Door `Door_dust`, every hit (half the puffs) | sharedassets6:486 |
| 0xD011 DOOR_RUBBLE | `Rubble Fall Short`, door stages 4 and 8 | sharedassets6:465 |
| 0xD012 DOOR_CHIPS | `Particle Rock Small Transient` x11 over the door | resources:5223 |
| 0xD013 HIT_CHIPS | `Particle Rock Small` x4 at the nail (door, Geo rocks) | resources:4328 |
| 0xD020 COCOON | `Spatter Blue` x50 and `Blue Glob` x9 | resources:5399, sharedassets6:495 |
| 0xD021, 0xD022 | the cocoon's cap, knocked right or left | scene cap sprite |
| 0xD023 SCUTTLER_BLOOD | a struck Scuttler's blood x15 | resources:5399 sprites |
| 0xD030 JAR_GLASS | a jar's glass | scene `Pt Glass L/S` |

| 0xD014 HIT_FLASH | the nail's strike at every door hit: `Strike Nail R`'s spark cluster (its own three sprites), a white core, thirteen white-pink streaks fanned out of the hit and a lavender pool of light on the floor (anchor: the hit; twice on the hits that change the door's pose and the one that opens it) | resources:7281 (tk2d resources:21014 sprites 8 to 10) |
| 0xD015 DOOR_PULSE | the pale light that runs up the door's face with every hit, gone in ten ticks (cooked in place) | the cook's own light |

Round 4 (2026-10-29) read the original's own data for these (the tools were a git-excluded inspector over `hk-unity`,
see NOTES): every nail strike on an object with `slashEffects` (the cocoon's list is `Slash Impact R` and `Strike Nail R`)
spawns the spark cluster of `Strike Nail R` (tk2d resources:21014, sprites 8 to 10, the clip `Sword`, 15 frames a second)
and `Slash Impact` streaks (sprites 45 to 54, single thin streak sprites, 30 frames a second); the cocoon flings
`Spatter Blue` x50 (sprite 1802, a small round blue bubble, speed 5 to 20), `Blue Glob` x8 to 10 (the ring ovals) and turns on
`Splat Effect` (the clip `Splat`, sprites 76, 73, 74, 75, 72, 77, tinted cyan (0, 1, 1), scale 2 by 3), which is the aqua leaves of
the burst; the jar's `Pt Glass L` and `S` draw four pale translucent glass pieces (107 by 428 texels). `light_frames` turns
such a sprite into a cell of light (its luminance times its alpha as the grey of the cell), so the soft additive path adds
the original's own shapes at the style's tint: `strike_frames`, `splat_frames` (one of the six frames, grown over its life,
because the art has no room for six), and the glass sheet. A style whose additive byte has bit 2 shows the cells of its sheet
in turn over the particle's life (the clips: the strike's three frames) instead of a random one. The effects' art in
Tutorial_01 is nearly full (6,912 bytes of the 7,552 the four VRAM fragments hold; the Great Door's own debris takes three times its
cells for the opacity levels), so the door's scene has no hit dust and the cocoon's flung cap is not cooked
(`COCOON_CAP_ART`).

The owners marked "the cook's own light", and the door's rising dust and falling dust are made by `host/hk-cook/src/break_effects/fx.rs` `Glow`: a soft
additive style of a grey art cell (`disc`, `bar`, `lens`, `shard`, `chunk`, drawn by the cook) that the style's tint
colours. A style may `stretch` (Style bytes 168..171): its quad is turned along the particle's velocity and is longer
on each side by that many world units per unit of speed (a streak, a petal, the pulse's tall quad), and a style with
additive bit 1 is a front style, drawn after the Knight and his slash (`Pool::draw_front`) so a flash is not hidden by
the slash. The art is cut for a few texels (`art_size`) and the quad is drawn larger, because the effects' VRAM is eight
kilobytes. The Great Door's `Door_dust` and `Rubble Fall Short` prefabs are not cooked any more: their sprite art went
to the screen as flat grey patches.

The soft styles need a copy of their palette in one of four rows (368, 236..239). A scene whose own uploads reached one
lost every soft style and drew its smoke as dithered coverage (Tutorial_01's "web-like noise" on the door): the cook's
`VRAM_RECTS` now stops at row 236 so no upload reaches them.

`.hkpsx/break-effects/report.json` lists, per scene, which owners fit and why the others were dropped.

## Memory

Nothing here is resident per effect. The tables of a scene (`HKFX0001` chunk: styles, emitters, art, frames) are
decoded at admission into `RESIDENT`, sized by the widest scene (`FX_MAX_*`; Tutorial_01 with 27 styles and 108
emitters), and the effect art rides the scene's own chunk. What the effects added to the resident image was paid
for by storing the particle curves as the distinct tables of each end of their size, opacity and spin (`Curve`: 18, 19
and 18 tables of 33 values serve all 23 curves, 5.4 KB instead of 15.2 KB; `host/hk-cook/src/break_effects.rs` `generate`),
and by building the two scene decoders (`load_scene`, `soft_styles`) for size. The lifeblood cocoon's world code
and the rigid debris solver are room modules (`host/code_modules.py` `lifeblood`, `debris`), read with the rooms
that use them. The debris module (5.8 KB) lands in the worst room (Crossroads_05, the grub jars) on top of the
carried Shade, Death and wake art, so the pool grew by 6,144 bytes (pool fit: room 8, +988 bytes of margin;
main had +688) and the free RAM below the pool is what `docs/BUDGET.md` says.

## Frame cost

A break now raises its own dust and chips as well as the Breakable's debris, and a nail hit on a rock or the
door raises dust, chips and (the door) rubble: more particles in the pool for the second or so a burst lasts.
A grub jar's glass alone is 80 shards. The break particles used to take a pass in every simulation tick, about
half a vblank for eighty shards, and the main loop only lets a frame end when a tick finishes in the vblank it
began in (main.rs, the loop after `begin_deferred_swap`): while a tick took a vblank the frame went on, up to 70
vblanks at the jar (33 and 36 in the first measurement, a King's Pass break took 5 vblanks a frame where main
took 4). The particles are only drawn, so each tick now adds to a count (`Pool::defer_break_tick`) and the
picture takes one pass for the ticks since the last (`State::tick_break_particles`, before `frame::render`;
`break_effects::tick_specs_by`): the velocity follows every tick, the step, shape, spin and contacts are taken
once over the distance the ticks add up to, and one step is exactly one tick. On the grub-jar route the stall
is gone (frame time 279 to 241 against a ceiling of 286, two frames of 5 vblanks and none longer) and
kp-playtest's frames of 5 vblanks went from 29 to 1.
Routes with no break (f08a, husk-guard, greenpath-walk, c37) are unchanged in the census (frames and GPU cycles per frame).
Rigid debris decodes its polygon only for the solver; a draw decodes a `Look` (centroid, scale, tint) and the
frame map's object index is found once per region.

## Not done

The door's `Tute Door Piece A/B`, the jar's rigid glass pieces (the debris bank carries them now, docs/DEBRIS.md) and
lid, `Dig Dust`, the grub's voices, the cocoon's cap, sweat idle and the other five frames of its `Splat` clip, Spatter's
stretch along its velocity, the Slash Impact clips as such (the streaks are the cook's `bar` cell, thick and long as the
original's sprites, not their five sprites).

The Knight's Focus dust is a burst of four per side (the original's loop makes six): drawn as the soft glow it is the
`focus` route's cost (207 of 389 frames with 25 over 2 vblanks on 9a0bca6, 210 of 382 with 38 at six, 206 of 391 with 21 at four).
