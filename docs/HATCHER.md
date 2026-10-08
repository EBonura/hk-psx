# Hatcher and Hatcher Baby source contract

`host/remaining_actors.py` extracted the contract below; `level57:5007`
(Crossroads_19) is the reference placement and `level57:5010` the reference
baby. The Hatcher was the one unadmitted family in the P19/P20 package whose
blocker was a missing runtime capability rather than a missing controller, so
read the pool section before the flight section.

This is implemented now, in `host/hatcher.py`, `shared/hk-sim/src/hatcher.rs`
and `game/src/enemies.rs`. Five of the six placed Hatchers and 45 of the 68
babies are admitted: Crossroads_19, Crossroads_27 and Crossroads_35. Read
"What was implemented" at the end for what the three parts below turned into,
including the two claims in the pool section that did not survive re-measuring.

## Placements

Six Hatchers sit inside a packed region envelope: Crossroads_19 (`level57:5007`),
Crossroads_22 (`level59:6937`), Crossroads_27 (`level61:4256`, `4257`, `4258`)
and Crossroads_35 (`level65:4206`). Four more, the `Hatcher NP` copies in
Crossroads_22 at x 148.44 and 155.64, sit outside every region envelope and are
not placements.

The babies are 68 pre-placed pooled instances, all parked at world
(100, 100, 0.375), which is why `actor_sources` never sees them as placed: 15
under `Hatcher Cage (1)` in Crossroads_19, 23 under `Hatcher Cage (2)` in
Crossroads_22, 15 under `Hatcher Cage` in Crossroads_27 and 15 under
`Hatcher Cage (2)` in Crossroads_35. Each scene has exactly one cage, shared by
every Hatcher in it: the three Crossroads_27 Hatchers draw from the same 15.

## Why actor_sources refuses both

For both the Hatcher and the baby, `walker_control` raises
`no single supported Crawler FSM` and every other gate declines: neither carries
a `Climber`, a `Walker` or a `PersonalObjectPool`, and neither name matches the
`Buzzer`/`Fly`/`Roller`/`Spitter` prefixes. Nothing about the flight was
examined and rejected; the family has simply never had a recognizer.

The real blocker is upstream of the recognizer. The babies are at (100, 100),
outside every region's interaction bounds, so `cook.py` filters them out of the
region's actor list before any recognizer runs. Admitting a Hatcher without its
cage would produce a Hatcher that can never fire.

## Hatcher body and vitals

Layer 11, identity world basis. One enabled `BoxCollider2D`, size
1.3125 by 1.84375, offset -0.125, -0.171875, edge radius 0, not a trigger.
`Rigidbody2D` dynamic (`m_BodyType` 0), gravity scale 0, linear damping 0,
constraints 4, which is the same gravity free dynamic body the Vengefly, Gruzzer
and Aspid already use. `Recoil` with `recoilSpeedBase` 20 and `recoilDuration`
0.15, `freezeInPlace` 0, `preventRecoilUp` 0. `DamageHero` deals 1,
`hazardType` 1.

`HealthManager`: hp 20, enemyType 0, smallGeoDrops 10, no medium or large,
every refusal flag (`invincible`, `invincibleFromDirection`, `hasSpecialDeath`,
`hasAlternateHitAnimation`, `damageOverride`, `ignoreKillAll`, `megaFlingGeo`)
zero, so the existing `ActorSpec` shape covers it.

`EnemyDreamnailReaction` pays the ordinary SOUL (convoTitle GENERIC, convoAmount
8). `FSMActivator` has `activateStaggered` 1 and `PersistentBoolItem` is
semi persistent; both are recorded and not run, as they are for the Gruzzer.

Animation library `sharedassets57.assets:79`, animator enabled,
`playAutomatically` false, `isRealtime` false.

| index | clip       | frames | fps | wrapMode | loopStart |
|-------|------------|--------|-----|----------|-----------|
| 0     | Fly        | 6      | 12  | 1 (loop section) | 2 |
| 1     | Fire       | 8      | 15  | 2 (once) | 0 |
| 2     | Burst      | 3      | 12  | 2 (once) | 0 |
| 3     | Corpse     | 1      | 30  | 6 (single frame) | 0 |
| 4     | Death Air  | 1      | 30  | 0 (loop) | 0 |
| 5     | Death Land | 4      | 18  | 2 (once) | 0 |

## Hatcher sensing

The child `Alert Range New` sits at the Hatcher's own position with local scale
15.608528137207031 uniform and one trigger `CircleCollider2D` of radius 0.5 at
zero offset, so the alert circle is 7.8042640686 units, the same circle
`shared/hk-sim/src/buzz.rs` already implements for the Aspid and the Vengefly.
Its `alert_range` FSM raycasts from itself to the hero against layer mask 8 and
writes the result into the Hatcher FSM's `Alert Range` bool (`FSM Name`
"Hatcher", `Bool Name` "Alert Range"); leaving the circle writes false. The
sense is therefore "hero inside 7.804 units and the straight line to the hero is
unobstructed", identical to the admitted flyers.

## Hatcher FSM

One `PlayMakerFSM` named `Hatcher`, start state `Initiate`, no global
transitions. Variables that matter: `Hatched Max` 5 (unused on the live path,
see `Hatched Max Check`), `startAlert` false on this placement, and the object
references `Cage`, `Hero`, `Self` and `Shot`.

- `Initiate`: `GetOwner`, `GetHero`, and `FindGameObject(withTag "Extra Tag")`
  into `Cage`. The cage object carries `m_Tag` 20054 in every scene, and there
  is exactly one per scene. Goes to `Idle`.
- `Idle`: play `Fly` and hold frame 2, run `FaceDirection` every frame with
  `spriteFacesRight` false, `playNewAnimation` false and `pauseBetweenTurns`
  false, and roam with `IdleBuzz(waitMin 0.75, waitMax 1.0, speedMax 1.75,
  accelerationMax 15.0, roamingRange 1.0)`. These are the same IdleBuzz numbers
  the Vengefly and Aspid use. Two enabled `BoolTest`s send `ALERT`: `startAlert`
  once on entry, and `Alert Range` every frame. The three disabled actions above
  them are the older `line_of_sight_alert` plus `alert_range` pair; the live gate
  is the single `Alert Range` bool.
- `Distance Fly`: set audio pitch 1.2, play `Fly` from frame 2, and run
  `DistanceFly(distance 6.0, speedMax 3.5, acceleration 0.1, targetsHeight true,
  height 3.5)` with `FaceObject(spriteFacesRight false, everyFrame)`. So it
  holds 6 units of horizontal separation and 3.5 units above the hero at up to
  3.5 units per second with 0.1 acceleration per fixed step. `WaitRandom(2.0,
  3.0)` sends `WAIT`; the disabled `RandomFloat(3, 4)` into `Fire Timer` and its
  `Wait` are the superseded form.
- `Hatched Max Check`: `GetChildCount(Cage)` into `Cage Children`, then compare
  against 0. Equal or less sends `TRUE` back to `Distance Fly`; greater sends
  `FALSE` to `Fire Anticipate`. The disabled `IntCompare(Spawned, Hatched Max)`
  above it is the superseded limit, so the live cap is "the cage still has a
  child", not "five spawned".
- `Fire Anticipate`: zero the velocity, play `Fire`, `Wait 0.335` to `Fire`.
- `Fire`: play a one shot (pitch 0.85 to 1.15), `GetRandomChild(Cage)` into
  `Shot`, and send `CANCEL` back to `Distance Fly` if the cage is empty.
  Otherwise read the Hatcher's own position into `Spawn X`/`Spawn Y`, subtract
  1.0 from `Spawn Y`, `SetPosition(Shot, Spawn X, Spawn Y)`, send `SPAWN` to the
  shot, `SetVelocity2d(Shot, y = -5)`, and fling 5 to 6 objects from the global
  pool at speed 2 to 5 over angles 210 to 330. `Tk2dWatchAnimationEvents` returns
  `WAIT` when the `Fire` clip completes, which goes back to `Distance Fly`. The
  two disabled actions are the `Spawned` counter and a `SetFsmGameObject` that
  told the shot who spawned it.

There is a second FSM on the object, `flyer_receive_direction_msg`. It idles
until an external `GO UP`/`GO DOWN`/`GO LEFT`/`GO RIGHT` arrives, clamps the
velocity on that axis to one sign, then applies `AddForce2dV2` of 40 with a max
speed of 10 on that axis every frame for one second, optionally resets the z
rotation and forwards `DIR MSG` to its children. The sender is the scene's
`EnemyDetector` object through its `enemy_message` FSM. The admitted Aspid
carries the same FSM and the port does not run it, so this is an existing,
accepted omission rather than new work.

## Hatcher Baby body and vitals

Layer 11. One enabled `BoxCollider2D`, size 0.359375 by 0.375, offset
-0.0234375, -0.03125, edge radius 0. `Rigidbody2D` dynamic, gravity scale 0,
linear damping 0, constraints 4. No `Recoil` component at all. `DamageHero`
deals 1. `ObjectBounce` with `bounceFactor` 0.3 and `speedThreshold` 1.0.

`HealthManager`: hp 5, enemyType 4, no geo drops, every refusal flag zero.
`EnemyDeathEffects` has a null `corpsePrefab`, `corpseFlingSpeed` 0,
`effectOrigin` 0, -0.5, `enemyDeathType` 4, `playerDataName` "Hatchling": the
baby has no corpse object.

Animation library `resources.assets:20637`, `playAutomatically` true.

| index | clip  | frames | fps | wrapMode | loopStart |
|-------|-------|--------|-----|----------|-----------|
| 0     | Fly   | 10     | 12  | 1 (loop section) | 2 |
| 1     | Death | 5      | 18  | 2 (once) | 0 |

## Hatcher Baby FSM

One `PlayMakerFSM` named `Control`, start state `Init`, plus the same
`flyer_receive_direction_msg` helper.

- `Init`: `GetParent` into `Cage`, `GetHero`, and
  `SetHealthManagerReset(self, reset true)`. Goes to `Inert`.
- `Inert`: `SetVelocity2d` to zero, then wait for `SPAWN`.
- `Chase`: `SetParent` with no parent, which unparents it from the cage,
  `FaceDirection(spriteFacesRight false, everyFrame, pauseBetweenTurns true,
  pauseTime 0.4)`, and `ChaseObject(speedMax 5.0, acceleration 0.1,
  targetSpread 1.5, spreadResetTimeMin 1.0, spreadResetTimeMax 2.0)`. The only
  transition out is `CENTIPEDE DEATH` to `Death`.
- `Death`: fling 3 to 4 objects from the global pool at speed 5 to 10 over 0 to
  360 degrees with origin variation 0.25 on both axes,
  `SetDamageHeroAmount(self, 1)`, `SetHP(self, 5)`, `SetIsDead(self, false)`,
  `SetParent` back to the cage and `SetPosition(0, 0)`. Then `FINISHED` to
  `Inert`.

So a baby is never created or destroyed. It is recycled: reparented, healed back
to 5, un-deaded and moved to the cage's local origin, ready to be picked again.

## What the port actually needs

The flight is nearly free. `IdleBuzz` and `DistanceFly` already exist in
`shared/hk-sim/src/buzz.rs` with different constants, the 7.804 alert circle plus
terrain ray already exists, `FaceDirection`/`FaceObject` already exist, the
gravity free dynamic body on the bounded terrain solver already exists, and the
`Fire` anticipate is a clip plus a 0.335 second wait. The baby's `ChaseObject` is
the only genuinely new action, and it is the Vengefly chase with a random target
offset of up to 1.5 units re-drawn every 1 to 2 seconds.

The capability the port lacks is the pool, and it has three separate parts:

1. **Cook time membership.** `cook.py` selects a region's actors by position
   against `interaction_bounds`. The babies are at (100, 100). The cooker needs
   to follow a Hatcher's cage (the single scene object tagged `Extra Tag`) and
   pull its children into the same region as the Hatcher, regardless of where
   they are parked.
2. **Pool budget.** `actor_sources` refuses a scene with more than 32 supported
   actors and `generated_actor_specs` refuses more than 32 `ActorSpec`s, and
   `postpack_actor_bank` builds one scene wide bank that every region of the
   scene shares, so the cap is per scene. Three of the four scenes fit:
   Crossroads_19 has 2 admitted actors today and would reach 18, Crossroads_27
   has 0 and would reach 18, Crossroads_35 has 0 and would reach 16.
   Crossroads_22 does not: 12 admitted Aspids plus 1 Hatcher plus 23 babies is
   36. Either the cage becomes a separate bounded sub-pool with its own cap (the
   Aspid shot pool is the precedent: eight slots inside `EnemyWorld`, not actor
   slots), or Crossroads_22's cage is truncated and the divergence recorded. The
   source cap is "while the cage has a child", so a truncated cage changes how
   many babies that Hatcher can ever release.
3. **Runtime activation and reset.** A guest actor needs three operations it does
   not have: move an inactive actor to an arbitrary position and make it live
   (`SetPosition` plus `SPAWN`), give it an initial velocity (y -5), and on death
   return it to the pool with hp restored and its dead flag cleared rather than
   removing it. Note that the source never instantiates anything, so no allocator
   is required; the guest needs a parked/live flag and a reset, which is strictly
   less than general spawning.

## Limitations a first implementation must state

- The cage is shared by every Hatcher in the scene, so two Hatchers in
  Crossroads_27 race for the same babies. A per Hatcher pool would be a
  divergence.
- `GetRandomChild` picks uniformly from the remaining cage children; the guest
  needs a deterministic per actor sequence, as the Vengefly and Runner already do
  for `Random.Range`.
- The Hatcher corpse is `sharedassets57.assets:29`, `Corpse Hatcher v2`, carrying
  a `CorpseHatcher` component rather than the plain `Corpse` that
  `host/effects.py` recognizes, with `corpseFlingSpeed` 15 and a zero spawn
  point. Its behaviour was not extracted; the `Burst` clip on the Hatcher library
  suggests the corpse releases the remaining babies, and that must be confirmed
  before the corpse is bound.
- The baby has no `Recoil` component, so nail hits do not displace it; the
  generic recoil pass must be skipped for this controller.
- `ObjectBounce` (factor 0.3, threshold 1.0) on the baby is not the terrain
  solver's slide; a first implementation that reuses the solver should say so.
- `FSMActivator.activateStaggered`, `PersistentBoolItem`, `SetZ` depth
  randomisation, the `flyer_receive_direction_msg` push, the global pool flings
  on both fire and baby death, the audio pitch ramp to 1.2 and every one shot are
  not presented.
- All six placed Hatchers share the same shape: `startAlert` false,
  `Hatched Max` 5, identity world basis and the 15.608528137207031 alert scale.
  Only the Crossroads_35 Hatcher (`level65:4206`) differs, carrying the same box
  collider twice, so a recognizer built on `runner._one` refuses it the way
  `host/climber.py` refuses the duplicate box Climbers; use `runner.body_box`,
  which already accepts one or two identical boxes.

## What was implemented

The spawning contract is a **cook-time reservation**, not an allocator. Every
baby a Hatcher can ever release is seated with the rest of the scene's actors
before the scene loads, parked in `Control`'s `Inert` state at the cage's own
authored position. A release moves one of them and clears its parked flag; a
death restores its hp and puts it back. Nothing is created or destroyed, which
is also what the source does, so the bounded 32-slot pool is an invariant of the
cook rather than something a release has to test at runtime.

A parked member costs one branch a tick: `EnemyWorld::tick` skips it before the
resident-view lookup, and `prepare_draws` skips it before the clip lookup.

Two of the three parts the pool section above asks for did not survive
re-measuring:

1. **Cook-time membership needed no new plumbing.**
   `regions.py::postpack_actor_bank` calls `actor_sources(sc)` with no bounds
   and then replaces every region row's supported actors with that scene-wide
   list, so a recognized baby at (100, 100) already reaches every region of its
   scene. The claim that `cook.py` filters it out is true only of the
   intermediate per-region pack, which the postpass overwrites.
2. **The pool budget re-measures as stated**, and Crossroads_22 really does not
   fit: 12 admitted Aspids plus 1 Hatcher plus a cage of 23 is 36 of 32. It is
   refused on a different clause first, though (see below), and the cage was
   never truncated: `host/hatcher.py::family_budget` refuses the scene's whole
   family, because a shorter cage silently changes how many babies that Hatcher
   can release.
3. **Runtime activation and reset** is `hk_sim::hatcher::Baby::release` and
   `::park`, plus `EnemyWorld::release_baby` and `Actor::park_baby`.

A third bound joined the two above. `enemies.rs::prepare_draws` asserts on
`MAX_VISIBLE`, which is the 20 animation slots a frame may bind, and a cage is
the only thing that has ever taken a scene near it: Crossroads_19 and
Crossroads_27 both reach 18. Every Hatcher and baby frame measures inside one
64x64 slot (worst frames 37x36 and 31x30), as do Crossroads_19's existing Aspid
and Zombie Leaper, so the scene's actor count is its worst-case tile count and
`family_budget` refuses a family that would push it past 20.

Crossroads_22 is refused on its FSM set, not its budget: its Hatcher and all 23
of its cage carry a third FSM, `Remove on battle start`, which takes them out of
the room when the arena begins and which nothing here runs. Its four `Hatcher NP`
copies are refused separately, for standing outside the room's own runtime
bounds under `Hatcher Summon` parents; they are arena content for P22, not
placements.
