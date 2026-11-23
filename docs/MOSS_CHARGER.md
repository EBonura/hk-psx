# Moss Charger

`host/moss_charger.py` recognises it, `shared/hk-sim/src/moss_charger.rs` runs it and
`game/src/enemies.rs` (`MossChargerRuntime`, `advance_moss_charger`, room module `moss_charger`) carries it.
Placed four times in Fungus1_10 (`level139`) and three times in Fungus1_21 (`level149`), all with one
`Mossy Control` digest. Fungus1_21's lower one (`level149:1665`) has a tk2d sprite scale of 0.94 in y and is
refused (its frame boxes would scale with it).

## `Mossy Control` (33 states)

- `Init Pause` 0.5 s, then `Init`: collider and renderer off, `Attack Range` and `Range Grass` detached
  (`SetParent` null) so the range box stays at the tuft. `X Min`/`X Max` are the tuft's x less/plus half the
  box's world width (`BoundsBoxCollider`, scale 1.175), then 2 inside.
- `Hidden`: `CheckAlertRange` on the box. `Emerge Pause` `WaitRandom` 0.5 to 1 s. `Hero Beyond?`: the Knight
  past `X Max` or below `X Min` cancels. `Left or Right?` is an even coin. `Emerge Right` puts it 14 right of
  the Knight charging left (scale -1), `Emerge Left` 14 left charging right; a side past the reach tries the
  other one next frame (`Pause`, `Pause 2`).
- `Emerge`: `SetPosition`, velocity a quarter of 15, collider and renderer on, `Appear` (6 frames at 12 fps).
- `Charge`: velocity 15. Two `RayCast2d`, repeatInterval 2, terrain: forward from (0, -0.5) 5.5 long, and down
  from 6.5 ahead (-0.5) 3 long. A forward hit or no ground is `SUBMERGE`.
- `Submerge`: `Decelerate` 0.7 per fixed step (and on entry), `Disappear` to its trigger frame 5, `Submerge
  Grass effect` to the end, `Submerge CD` (collider and renderer off, invincible, back to the tuft, 0.35 s).
- `BLOCKED HIT` (global, sent by `HealthManager.Invincible`; IL) goes to `Line Loop`, which counts `Looper` and
  whose `State 2` runs `GotoPreviousState`. PlayMaker sets `PreviousActiveState` on exit (IL `Fsm.ExitState`),
  so the previous state of `State 2` is `Line Loop` itself: one blocked hit loops there in its frame until
  `Looper` passes 11 and leaves for `Burst`. The first tink on the charge flings it out.
- `Burst`: gravity 1.5, `Stun`, then by `GetAttackDirection` 0 right 70 deg at 18, 1 up 90 at 20, 2 left 110
  at 18, 3 down 270 at 10, and `In Air` the next frame (invincible cleared, collider on). A floor contact is
  `Land` (x velocity 0), `Get Up` (4 frames at 18 fps), `Direction`, `Run L`/`Run R` away from the Knight:
  `AccelerateVelocity` 0.5 per fixed step clamped to 10, the Knight crossing to its front turns it, a forward ray
  (2) or no ground 3 ahead (1.3 down) or 1 s ends it. `Dig Start` (collider off, gravity 0, `Decelerate` 0.4,
  `Escape` to frame 5) and `Dig` (x 0, to the end) cast `Dig Check` 1 down; no ground falls back to `In Air`.

## Body and health

Layer 11, identity transform, mass 10, gravity 0 until `Burst`, constraints 4. `HealthManager` hp 15, 8 small
Geo, invincible with `preventInvincibleEffect` (a silent block). `Recoil` 15 for 0.15 s, `preventRecoilUp`.
`DamageHero` 1. No collider of its own: tk2d builds a box from the frame's sprite definition (the recogniser
proves every frame against the constants), `SetCollider(false)` holds while the clip does not change, and the
next sprite change switches it back on (`Escape` after `Dig Start`).

Corpse: `Corpse MossCharger v2`, box 1.05 by 1, bounce 0.3, fling 15, spawned 0.5 above, `Death Air` and
`Death Land` two frames each, once.

## Not presented

The grass particles (`Range Grass`, `Grass Burst`, `Charge Grass`, `Grass Submerge`, `Grass Dig`, `G Explode
1`), the burst's hit effect, the `AverageShake` and `EnemyKillShake` camera shakes, and every sound (the
controller raises `moss_charger::Sound` cues; no scene bank carries the clips yet).

## Tests

`moss_charger::tests` (timings, reach, rays, burst, run, colliders) and
`enemy_runtime::a_moss_charger_surfaces_charges_and_one_tink_bursts_it_out_of_the_grass` (the guest runtime).
