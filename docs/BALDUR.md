# Baldur (Roller) controller

`shared/hk-sim/src/baldur.rs` ports the PlayMaker `Roller` FSM of the
Shaman Temple rolling enemy. It drives the guest actor pool through
`ActorController::Baldur { start_clip, roll_clip, stop_clip }`
(`game/src/enemies.rs` `BaldurRuntime`, recognized by `host/baldur.py`).
Source records and CIL are in .hkpsx/baldur/CONTRACT.md.

Idle holds the horizontal velocity at zero (the gravity body still falls) and
faces the hero. When the hero is inside the 21.14 x 1.9 alert box and in line
of sight, the Baldur turns toward it, plays Start (24 ticks), draws a roll
time of 2 to 3 s and rolls: 0.45 units/s of acceleration per frame, clamped
at 11 units/s. A wall contact flips the direction and launches the body at
12 units/s along 115 or 65 degrees; landing resumes the roll the other way.
When the roll time runs out it plays Stop, rests for half a second and
returns to Idle. Recoil's horizontal event resets the roll speed to zero and
the roll re-enters, so nail hits keep interrupting the charge.

The runtime keeps last frame's blocked-axis and grounded flags as the WALL
and GROUND senses, runs the body at gravity scale 0.8 (48 units/s^2) with the
bounded terrain solver, resolves the spawn once like the Crawlers, and adds
the generic Recoil displacement (25 units/s for 0.15 s). The corpse is the
source `Corpse Roller Spawned`: fling 15 from 0.2 above the body, gravity
0.8, no bounce, and `CorpseSpec.remove_after_land` 120 ticks standing in for
the roll-out, shrink and destroy sequence. Not presented: roll dust, the roll
audio loop, land effects, the corpse's circle body, 0.7 damping and spin.
