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
| Desolate Dive | `Q Charge`, `Q Trail`, `Q Flash Start`, `Q Flash Slam`, `Q Slam` (`Quake Blast`), `Q1 Pillar`, `Q Orbs`; the Knight's `Quake Antic`, `Quake Fall`, `Quake Land` | the Knight's three clips and `Q Slam` from the `art_spell` pack (`ability_fx::quake_land` at the landing). Not cooked: `Q Charge`, `Q Trail`, the two flashes, `Q1 Pillar`, `Q Orbs`, and the spell's sounds (`hero_quake_spell_prepare`, `hero_quake_spell_impact`): the hard landing's sound stands in. Where the pack is not resident the earlier stand-ins play (the Crystal Heart's ground charge, the ordinary fall, `HardLand`, the Focus dust). `BigShake` and `EnemyKillShake` are the camera's |
| Howling Wraiths | `Scr Heads` (`Scream1`), `Scr Base`, `Scr Orbs`; the Knight's `Scream Start`, `Scream`, `Scream End` | the Knight's three clips and `Scr Heads` from `art_spell` (`ability_fx::scream` at the burst). Not cooked: `Scr Base`, `Scr Orbs` and `hero_scream_spell`: the fireball's cast sound stands in. Without the pack the fireball's antic and cast clips stand in for the body. `BigShake` is the camera's |
| Nail arts | `Cyclone Slash`, `Great Slash`, `Dash Slash` | the abilities are not in the port |
| Crystal Heart | the `SD *` effects | not presented (the body poses are) |

## How it is built

The clips are cooked by `host/ability_art.py` (`FX_CLIPS`) as streamed packs, `art_fx` (dash, wings,
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

## Desolate Dive and Howling Wraiths (`art_spell`)

The last streamed pack, after `art_fx_spell`: `Quake Slam` (`Q Slam`'s `Quake Blast`, 6 frames, 10 parts) and `Scream Heads`
(`Scr Heads`' `Scream1`, 13 frames, 12 parts), both at 1.7 screen pixels a texel, then the Knight's `Quake Antic`, `Quake Fall`,
`Quake Land`, `Scream Start`, `Scream` and `Scream End` (27 frames at his own density). The two effects are children of the
Knight's `Spells`: their transforms and library are read from the prefab (`knight_child_effect`), and since `Hit Box Control`
keeps them facing as authored whichever way he faces, they are raised facing that way. They are mapped to the bright palette
without being fitted into it (`FX_PALETTE_FROZEN`), and the poses take the palette of the body sheet that holds
`Fireball1 Cast` (`coverage_texels`: the coverage class `quantize_alpha_coverage` would give each texel, then the nearest colour
of that class), so the pack takes no CLUT row and every earlier frame is unchanged. 21,993 bytes: the heads 11,593, the
poses 7,881, the slam 2,519, frame records included.

It is the third optional pack (`HK_FX_FIT` is a u16 a scene, three bits a row, filled in pack order so it only takes what
`art_fx` and `art_fx_spell` leave). Against the module report of the current landing chain (58 rooms) it fits beside the room
and the Death and respawn clips in 54 rooms, and beside the Shade's art too in 25. The guest carries it while he owns either
spell (`ability_fx::wants_quake_scream`); where it is not resident the poses fall back to the stand-ins
(`main.rs` `spell_stand_in`) and the landing raises the Focus dust instead of the slam.

Resident cost: eight clip rows, the effect frame indices, a `STREAM_PACKS` row, `HK_FX_FIT` doubled (66 bytes) and a sixth
carry slot. The nine effect slots and the sixth carry slot unrolled `ability_fx::tick`/`request`/`draw_world` and
`modules::carry`/`wanted`/`fetch_pending`, which are now built for size; with that, free RAM below the module pool went from
5,148 to 5,724 bytes.

The spells' sounds are not cooked. The Focus range of SPU RAM, where the ability one-shots ride, has 32 bytes left, and the
three clips need about 19 KB at the lowest rates the ability ladder allows; a per-scene bank row (`["all"]`, docs/SCENE_SFX.md)
costs a 12-byte `ENTRIES` row per scene and event in resident RAM (about 2.4 KB for the three across 66 scenes).

## Pins

`HK_FX_RAISED` (effects raised) and `HK_FX_LATE` (raised while their art was absent, pinned at zero) on the
four ability routes: `cheat-dash`, `cheat-wings`, `cheat-claw` and `cheat-spell`.

## Hornet's arena: the Cloak's first dash (2026-11-08)

Fungus1_04 is the one room where the Knight earns an ability and uses it in the same visit, and its pool is the tightest in the
build: her fight code (`hornet`, 49,848 bytes), her art code and her state block (15,844 and 15,976) plus the Death and respawn clips
leave +5,424 bytes against the 14,336 that `art_fx` reserves. `HK_FX_FIT` therefore said "does not fit", the pack was never asked for in
that room, and every dash there raised no effect (`HK_FX_LATE` counted them, `HK_FX_RAISED` stayed at zero). It was not a timing race:
found by the first dash of `walk-new-game-to-hornet`, reproduced on the pre-fix disc by `walk-fungus1-32-to-hornet` (late 1, raised 0).

The fix gives the room's boss a way to hand her modules back. `modules::retire_boss` (called by her tick once the Cloak is taken, and on
every later visit that has it) takes `RETIRE_AFTER_BOSS` (her `hornet` module, `host/code_modules.py`) out of what the room wants and
pins, so the pool may reuse the space; her art code and state block stay because the item panel still runs from them (its countdown
now sits in `convo_tick`). `HK_FX_FIT` has a third row (bits 4 and 5): the packs that fit beside the room less the retired modules.
`fx_fit` answers with it while a retirement is in force in the resident room, `carry_ability_art` then asks for `art_fx`, and the pool's
background fetch reads it into her old space during the item panel. Nothing resident changes (free RAM is the same as before the
change); the pool fit check is untouched because it judges the room as it stands before the fight.

Proof: `walk-fungus1-31-bench-to-hornet` and `walk-new-game-to-hornet` (the same back half) check, right after Cross closes the item panel, that `art_fx` is resident
and, after the first dash with no wait, `HK_FX_DASH_RAISED >= 1` (a counter of dash effects alone, since the chain's earlier spell casts
count in `HK_FX_RAISED` and `HK_FX_LATE`). On the disc without the change the same walk fails the residency check and the dash count (on
that disc `HK_FX_LATE` goes 0, 1, 2 over the first two dashes and `HK_FX_RAISED` stays 0).
