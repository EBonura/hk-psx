# Gruzzer (Fly) controller

`shared/hk-sim/src/gruzzer.rs` ports the PlayMaker `Bouncer Control` FSM of
the bouncing Crossroads flyer. It drives the guest actor pool through
`ActorController::Gruzzer` (`game/src/enemies.rs` `GruzzerRuntime`,
recognized by `host/gruzzer.py`). Source records and CIL are in
.hkpsx/gruzzer/CONTRACT.md.

The Fly waits until the main camera is within 44 units (3D, so about 22
units in the plane), then picks a random angle and flies at 5.2 units/s. The
controller keeps the angle and the FSM's `Facing Right` flag; the runtime
turns the angle into a velocity through the debris sine table, moves the
gravity-free body with the bounded terrain solver, adds the generic Recoil
displacement pass, and reports the blocked axis as a bonk (up, right, down,
left, the CheckCollisionSide order). Each bonk re-aims from the authored
range: ceiling and floor bonks choose by facing, wall bonks by whether the
angle was in the upper half. FaceDirection follows the x velocity sign with
no pause and no turn clip; the single Fly clip is both walk and turn clip.

Death uses the shared corpse path with the source `Corpse Fly` parameters:
fling 20, gravity 0.7, bounce 0.7 and `breaker` with `smash_bounces` 3, so
the corpse bounces twice and is removed on its third landing (`CorpseSpec`
gained `smash_bounces`). Not presented: the corpse spin and break pieces, the
buzz loop, SetZ depth randomization and FSMActivator staggering.
