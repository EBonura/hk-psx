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
| 0xD030 JAR_GLASS | a jar's glass, one opacity level (Crossroads_05 has no room for `Pt Glass L` at three) | scene `Pt Glass L/S` |
| 0xD040 HARD_LAND_DUST | `HardLandEffect`'s Dust child, in every scene | resources:5272, 7198 |
| 0xD041, 0xD042 HARD_LAND_ROCKS | `Particle Rock Small Shrinker` x3 at 95..140 and 40..85 degrees | resources:4329 |

`.hkpsx/break-effects/report.json` lists, per scene, which owners fit and why the others were dropped.

## Not done

Strike Nail R's own art (the impact is the grass Slash Impact), the door's `Tute Door Piece A/B`, the jar's rigid
glass pieces (the debris bank carries them now, docs/DEBRIS.md) and lid, `Dig Dust`, the grub's voices, the cocoon's
sweat idle and splat animation, Spatter's stretch along its velocity.
