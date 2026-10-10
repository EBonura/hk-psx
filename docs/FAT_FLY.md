# Fat Fly source contract

`host/fat_fly.py` recognises it, `shared/hk-sim/src/fat_fly.rs` runs it and
`game/src/enemies.rs` (`FatFlyRuntime`, `advance_fat_fly`) carries it. Four are
placed in Fungus1_19 (`level147`), all with the same two FSM digests.

## Body

Layer 11, identity transform, depth near zero. One `BoxCollider2D`, size
0.890625 by 0.84375, offset (0.0390625, -0.03125). `Rigidbody2D` dynamic, mass 1,
gravity scale 0, no damping, constraints 4: the gravity-free body the Gruzzer,
Vengefly and Aspid use. `Recoil` 15 for 0.15 s. `DamageHero` 1, hazard type 1.
`HealthManager` hp 10, 4 small Geo, no refusal flag. `EnemyDreamnailReaction`
pays the ordinary SOUL. Two children, `Stopper` (two boxes inside the body box)
and `EnemyDetector`, add no reach and are not carried. `FSMActivator` enables
both FSMs on activation; the guest takes them as enabled.

Library `sharedassets147.assets:98`:

| clip | frames | fps | wrap | note |
|---|---|---|---|---|
| Fly | 8 | 12 | loop | |
| Attack | 8 | 12 | once | trigger on frame 5 |
| Death Air | 2 | 12 | once | |
| Death Land | 1 | 30 | loop | |

## `fat fly bounce`

Start `Initialise`; globals `STOP` to `Stopped` and `GO UP` to `Go Up`.

- `Initialise`: `GetDistance(Self, Hero) < 25`, every frame, then `Aim`.
- `Aim`: send `START` to the attack FSM, store the angle to the Knight
  (`GetAngleToTarget2D`), go to `Left or Right?`.
- `Left or Right?`: angle below 90 or at 270 and over faces right (scale -1),
  otherwise left (scale 1; the art faces left). Then `Fly 2`.
- `Fly 2`: `SetVelocityAsAngle(Angle, Speed 4)` every fixed step; a collision
  stay goes to `Collision Check`, which reads the contact normal and re-aims
  with the Gruzzer's table (up 320 to 350 or 190 to 220 by facing, down 10 to 40
  or 140 to 170, a wall 140 to 170 / 190 to 220 / 10 to 40 / 320 to 350 by
  whether it was heading up).
- `Stopped`: stores the velocity's angle and waits for `WAKE`, which goes to
  `Left or Right?`.

## `Fatty Fly Attack`

Start `Sleep`; global `TAKE DAMAGE` to `Attack Antic`.

- `Sleep` waits for `START`, then `Wait`: send `WAKE`, `WaitRandom` 2 to 3 s.
- `Attack Antic`: `Decelerate` 0.1 per fixed step, `Wait` 0.35 s.
- `Attack Antic 2`: send `STOP`, play `Attack` (trigger event on frame 5,
  complete event at the end), `Decelerate`. The trigger goes to `Attack`.
- `Attack`: four `FlingObjectsFromGlobalPool` of `Spitter Shot R` at 45, 135,
  225 and 315 degrees, speed `Shot Speed` 12. Ends on the clip completing (15
  ticks after the trigger) before its own `Wait` 0.5 s.
- `CD`: play `Fly`, `Wait` 0.5 s, then `Wait`.

`Decelerate` (IL read) moves each velocity axis 0.1 toward zero per fixed step
and never past it. In `Attack Antic` it runs after `SetVelocityAsAngle` in the
same fixed update (the object lists the bounce FSM first), so the bounce speed is
a little under 4; from `Attack Antic 2` the bounce FSM is `Stopped` and what is
left drifts until `Wait` sends `WAKE`.

## Shot

The pooled prefab is the Aspid's `Spitter Shot R` (`sharedassets32.assets:198`):
gravity scale 0.05, box 0.640625 by 0.5625, damage 1. It rides the existing shot
pool as `ShotKind::Spit`. Its clips `Idle` (4 at 20 fps) and `Impact` (6 at 20
fps) are cooked with the actor.

## Corpse

`Corpse Fat Fly`: circle collider radius 0.6 offset (0.02, -0.01), gravity scale
0.8, `ObjectBounce` 0.6, fling speed 20, a `Corpse` with no special flag. It lands
and stays. The circle is its bounding box here.

## Not presented

Wing buzz loop and shot sounds, `Corpse Flame` and `Corpse Steam`, spore clouds,
the `damages_enemy` FSM (it never meets another enemy), `SetZ` depth.
Bonk sides come from the blocked solver axis instead of the contact normal.
