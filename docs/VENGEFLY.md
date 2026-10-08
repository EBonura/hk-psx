# Vengefly (Buzzer) controller

`shared/hk-sim/src/vengefly.rs` ports the source PlayMaker `chaser` FSM of the
first flying enemy. It drives the guest actor pool through
`ActorController::Vengefly { startle_clip, chase_clip, turn_fly_clip }`
(`game/src/enemies.rs` `VengeflyRuntime`, recognized by `host/vengefly.py`).
The source records and CIL dumps are in .hkpsx/vengefly/CONTRACT.md.

The body is a gravity-free dynamic Rigidbody2D with a 1.25 x 0.625 box. The
controller owns velocity and sprite facing; the runtime integrates the body
against resident terrain with the bounded solver and adds the Recoil
displacement (15 units/s for 0.25 s toward the hit direction) as a second
solver pass, as the other dynamic actors do.

States: Idle (IdleBuzz roaming around the entry position within one unit at up
to 1.75 units/s, with random per-axis acceleration re-picked every 0.75 to 1 s
and a velocity damp at the roaming edge), Startle (velocity zero, four-frame
clip, faces the hero), Chase (per-axis 0.045 acceleration per fixed step toward
the hero, clamped to 5 units/s; while seen, the In Sight / Out of Sight
ping-pong adds one extra DoBuzz per frame; ten seconds after the last sighting
it stops) and Stop (0.12 per-step deceleration for one second, then Idle
re-samples its roaming origin). Sight is the 7.804-unit alert circle against
the hero body plus an unobstructed terrain raycast; TOOK DAMAGE startles only
from Idle. FaceDirection turns the sprite toward the x velocity with a half
second pause and plays TurnToIdle or TurnToFly.

The source 50 Hz FixedUpdate steps run from an accumulator inside the 60 Hz
tick so the per-step constants stay as authored. Random.Range is a
deterministic per-actor sequence. Not presented: the Startle one-shot, the
live buzz loop and the corpse break pieces (the `breaker` corpse is removed
when it lands; `CorpseSpec` gained `gravity` and `breaker`).
