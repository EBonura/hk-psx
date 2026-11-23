# Fat Fly (Obble)

`host/fat_fly.py` recognises it, `shared/hk-sim/src/fat_fly.rs` runs it and `game/src/enemies.rs`
(`FatFlyRuntime`, `advance_fat_fly`, room module `fat_fly`) carries it. Four are placed in Fungus1_19
(`level147`) and five in Fungus1_31 (`level158`), all with the same two FSM digests. Both FSMs are authored
disabled and switched on by the object's `FSMActivator` (staggered), so the guest seats them running. Two
of Fungus1_31's are mirrored (scale x -1) and face right until their first aim.

## Body

Layer 11, identity transform (or the x mirror). `BoxCollider2D` 0.890625 by 0.84375 at (0.0390625,
-0.03125); `Rigidbody2D` dynamic, gravity 0, constraints 4. `Recoil` 15 for 0.15 s. `DamageHero` 1.
`HealthManager` hp 10, 4 small Geo. Library: Fly 8 at 12 (loop), Attack 8 at 12 (once, trigger frame 5),
Death Air 2 at 12, Death Land 1 at 30.

## `fat fly bounce`

`Initialise` waits for `GetDistance(Self, Hero) < 25`; `Aim` sends START to the attack FSM and takes the
angle to the Knight. `Left or Right?` (FloatSwitch: below 90 right, below 270 left, else right) sets the
scale; `Fly 2` writes `SetVelocityAsAngle(Angle, 4)` every fixed step. A collision stay goes to `Collision
Check`, which reads the contact normal (x before y) and re-aims: a ceiling 320..350 facing right or 190..220
left, a floor 10..40 or 140..170, a right wall 140..170 heading up or 190..220 heading down, a left wall
10..40 or 320..350. `STOP` stores the velocity's angle and writes nothing more until `WAKE`.

## `Fatty Fly Attack`

`Sleep` waits for START. `Wait` sends WAKE and waits 2 to 3 s. `Attack Antic` (`Decelerate` 0.1 per fixed
step and on entry, 0.35 s), `Attack Antic 2` (STOP, `Attack` to its trigger frame, `Decelerate`), `Attack`
(four `FlingObjectsFromGlobalPool` of `Spitter Shot R` at 45, 135, 225 and 315 degrees at `Shot Speed` 12;
ends on the clip, 15 ticks later, before its own 0.5 s), `CD` (Fly, 0.5 s). `TAKE DAMAGE` (global) goes to
`Attack Antic` from anywhere, even asleep.

## Shot and corpse

The pooled shot is the Aspid's `Spitter Shot R` (gravity 0.05, damage 1, Idle 4 at 20, Impact 6 at 20), on
the same pool. The corpse is `Corpse Fat Fly`: fling 20, gravity 0.8, ObjectBounce 0.6, a circle collider
of radius 0.6 solved as its bounding box, Death Air then a looping one-frame Death Land.

## Not presented

The wing loop, the attack and shot sounds, the shot's spatter, the corpse's spin (`SpinSelfSimple`), steam
and flame. A volley shares the tick's four-slot fire queue with any Aspid or Blocker firing on the same tick.

## Tests

`fat_fly::tests` (wake, speed, attack cycle, hit wind-up, re-aim table),
`enemy_runtime::a_fat_fly_floats_at_the_hero_and_a_hit_makes_it_throw_four_shots`, and
`tests/test_greenpath_enemies.py` (the recognizer's pins against the controller's constants).
