# Aspid Hunter (Spitter) controller and shots

`shared/hk-sim/src/aspid.rs` ports the PlayMaker `spitter` FSM of the
Crossroads shooting flyer. It drives the guest actor pool through
`ActorController::Aspid { fire_clip, shot_clip, impact_clip, start_alert }`
(`game/src/enemies.rs` `AspidRuntime`, recognized by `host/aspid.py`). Source
records and CIL are in .hkpsx/aspid/CONTRACT.md. The shared IdleBuzz and
DistanceFly actions live in `shared/hk-sim/src/buzz.rs` (the Vengefly uses
IdleBuzz too).

Idle roams with IdleBuzz and faces its x velocity. The 7.804-unit alert
circle plus a clear terrain ray sends it into Distance Fly: it accelerates
toward the hero when farther than 7 units and away when nearer (0.1 per fixed
step, 4 units/s), faces the hero with the TurnToFly clip, and every 1.5 to
2.25 s checks the range (14 units) and a clear ray. A clear check flies back
for half a second at 8.25 units, then Fire Long anticipates for 45 ticks
(trigger frame 9) while keeping 9 units; the shot leaves toward the hero at
15 units/s and the clip finishes before the next Distance Fly. Eight seconds
without the hero inside the 12.1-unit unalert circle (and visible) return it
to Idle. Hits only recoil it (15 for 0.15 s); the FSM has no damage
transition. Placements with `startAlert` skip Idle.

Shots are an eight-slot pool inside `EnemyWorld`: a `Spitter Shot R` flies
straight under gravity 0.05, is removed on a terrain contact of its movement
segment or on a hero box overlap (1 damage through the existing hurt path),
plays Impact for 18 ticks and leaves. The Idle and Impact clips are cooked
from the shot library at the prefab scale (0.7 x 0.8). Not presented: the
shot's rotation and stretch, the dribble spatter, audio and shockwave. The
collider-less `Corpse Spitter` (massless) is removed on its first landing.
