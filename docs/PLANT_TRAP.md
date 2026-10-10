# Plant Trap source contract

`host/plant_trap.py` recognises it, `shared/hk-sim/src/plant_trap.rs` runs it and
`game/src/enemies.rs` carries it. One in Fungus1_01 (`level128`), one in
Fungus1_19 (`level147`), identical FSM digests.

## No collider on the object

The root has no `BoxCollider2D`. tk2d builds one at run time:
`tk2dBaseSprite.UpdateCollider` (IL) reads the sprite definition of the frame
showing. `physicsEngine` 1 is 2D; `colliderType` 2 is a box centred on
`colliderVertices[0]` with half extents `colliderVertices[1]` (scaled by the
sprite scale, 1 here); `colliderType` 1 disables the collider. So the trap hurts
and can be hurt only while its jaws are open: `Snap` frames 0 to 2 and `Retract`
frames 0 to 2. Boxes (Q16, relative to the trap): see `plant_trap.rs`; the
recogniser re-derives each from the sprite definitions and refuses a mismatch.

## `Plant Trap Control`

Start `Init` (`FindChild Ready Grass`) then `Idle`.

- `Idle` waits for `DETECT`, sent by the `Detector` child's `Detect Hero` FSM
  while the Knight is inside its trigger box (offset (0.125, -1.9319), size
  2.46875 by 1.2612, layer 13).
- `Ready`: play `Snap Ready` (6 frames, loop section from frame 1), `Wait` 0.75 s.
- `Snap`: play `Snap` (3 frames at 12 fps, once), `Wait` 1 s.
- `Retract`: play `Retract` (7 frames at 12 fps) to completion.
- `Cooldown`: `Wait` 0.5 s, then `Init` and `Idle`.

At rest the object shows sprite 19, which is `Retract`'s last frame, and returns
to it, so `Idle` shows that frame (the `Idle` clip is not used).

`HealthManager` hp 16, 9 small Geo. `DamageHero` 1, hazard 1. No `Recoil`, no
`Rigidbody2D`. `damages_enemy` is a generic FSM that damages enemies the trap's
collider meets; none are near.

## Corpse

`Corpse Plant Trap` has no body: its FSM plays `Death` (7 frames at 12 fps, once)
where the trap stood and hides the renderer. It is the hold form of the guest
corpse.
