# Moss Charger source contract

`host/moss_charger.py` recognises it, `shared/hk-sim/src/moss_charger.rs` runs it and
`game/src/enemies.rs` (`MossChargerRuntime`, `advance_moss_charger`) carries it. Four are
placed in Fungus1_10 (`level139`) and one in Fungus1_17 (`level146`). One placement also
carries an `FSM` whose `GRIMMKIN SPAWN` event deactivates it, which only the Grimm Troupe
sends; it is allowed by digest and never fires on a fresh save.

## No collider of its own

The root has no collider: tk2d builds one from the sprite definition of the frame
showing (`tk2dBaseSprite.UpdateCollider`, IL: `colliderType` 2 is a box centred on
`colliderVertices[0]` with half extents `colliderVertices[1]`, 1 disables it). The
recogniser reads every frame's box and proves it against the constants in the sim. The
big charging box is 3.45 by 2.11, the stun and run boxes about 1.05 by 0.75; the last
seven `Disappear` frames and the last eight `Escape` frames have none. `SetCollider`
and `SetMeshRenderer` switch the hidden body off, and tk2d switches the collider
back on at the next sprite change (the IL sets `enabled` when it updates a disabled box),
which only matters while the clip plays.

## `Mossy Control`

Start `Init Pause`; globals `ZERO HP` to `Detach` and `BLOCKED HIT` to `Line Loop`.

- `Hidden`: `CheckAlertRange` on the `Attack Range` child (box 28 by 1.38 at scale
  1.175, detached to stay at the tuft by `SetParent(null)`). `IN RANGE` to
  `Emerge Pause`: `WaitRandom` 0.5 to 1 s, then `Hero Beyond?`.
- `Init` computes `X Min`/`X Max` as the tuft x less/plus half the range box's width
  (`BoundsBoxCollider` stores the collider's `bounds.size.x`), then plus/minus 2. The Knight
  outside that sends `CANCEL` back to `Hidden`. Otherwise a coin picks `Emerge Left` or
  `Emerge Right`; each puts it 14 units to that side of the Knight (`RandomFloat(14, 14)`) and
  falls over to the other side if that is outside the reach. Emerging on the right means
  charging left. It is placed at (Appear X, tuft y), velocity `Current Charge Speed` * 0.25,
  and `Appear` plays (six frames).
- `Charge`: velocity 15 toward the Knight. `RayCast2d` (IL: the origin is the position plus
  `fromPosition` as written, only the direction is turned by Self space) forward from y -0.5, 5.5
  long, and down from `RayDown X` (6.5 ahead, -0.5) 3 long. A forward hit or no ground sends
  `SUBMERGE`. `TAKE DAMAGE` goes to `Line Loop` (below).
- `Submerge`: `Decelerate` 0.7 per fixed step (IL: toward zero on each axis, never past it),
  `Disappear` to its trigger frame 5, then `Submerge Grass effect` to the end of the clip,
  `Submerge CD` (collider off, renderer off, invincible, back to the tuft, 0.35 s), `Play Range`,
  `Hidden`.
- `Line Loop`: `HealthManager.invincible` is set from the start (and `preventInvincibleEffect`),
  so every nail hit is a `BLOCKED HIT`: it counts one and re-enters the interrupted state. The
  twelfth (`Looper` > 11) is `LOOP COMPLETE` to `Burst`. The counter is never reset.
- `Burst`: gravity scale 1.5, `Stun`, flung by the attack's cardinal (`GetAttackDirection`):
  right 70 degrees at 18, up 90 at 20, left 110 at 18, down 270 at 10. `In Air` clears
  `invincible`; bottom contact is `Land`, then `Get Up` (four frames at 18 fps), `Direction`
  and `Run L`/`Run R`: it runs away from the Knight (`AccelerateVelocity` 0.5 per fixed step to
  10, `TurnRun` clip, a ray 2 ahead and a ground ray 3 ahead and 1.3 down), turning when the
  Knight crosses it, for at most a second. `On Ground?` goes to `Dig Start` (`Escape` to frame
  5, `Decelerate` 0.4) and `Dig` (to the end), then `Submerge CD` again. It is vulnerable
  (hp 15) from `In Air` until `Submerge CD`.

## Body and health

Layer 11, identity transform, gravity scale 0 (1.5 while bursting), mass 10, constraints 4.
`HealthManager` hp 15, 8 small Geo, invincible. `Recoil` 15 for 0.15 s, `preventRecoilUp`.
`DamageHero` 1. `NonBouncer` inactive: a down slash pogoes.

## Corpse

`Corpse MossCharger v2`: box 1.05 by 1, bounce 0.3, gravity 0.8, fling 15, spawned 0.5 above;
`Death Air` (two frames) and `Death Land` (two frames), both once.

## Not presented

Grass puffs, the hit effects, camera shakes and every sound. `Dig Check`'s ray (the FALL
branch) is not cast: the dig always finds ground.
