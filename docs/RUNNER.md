# Crossroads Zombie Runner implementation

The controller in `shared/hk-sim/src/runner.rs` is source preparation for the
Crossroads_01 enemies. It is not connected to the guest actor pool or present in
the current provisional70 disc. Crawler behavior is unchanged.

The supported original variant has Coward=false and Reverse=false. Its Walker
pauses and turns at walls/holes; its separate Swipe FSM attacks when both the
alert-range and terrain line-of-sight queries pass, or on an accepted damage
event. Nominal movement speeds are1.5 units/s walking and6 during a lunge.
The caller supplies animation-completion tokens, preserving the original
anticipation/lunge/cooldown sequence instead of advancing it with a second
unrelated timer. Source clip durations at60Hz are25/40/5 ticks, followed by a
15-tick idle. Damage precedes horizontal-recoil reset; vertical recoil does not
trigger that reset. Death is terminal.

`step_walker` and `step_swipe` expose the independent component callbacks so
reference traces can select their observed order. The convenience `step` uses
Walker then Swipe as a deterministic guest policy; original Unity execution
order and50Hz physics phases remain unverified. StartMoving now reenters
Walker.Update synchronously, including its countdown, before Reset clears turn
cooldown. Ready then checks the cached alert/LOS conditions immediately, allowing
Idle to restart an attack in the same callback. Horizontal recoil requires fresh
Senses sampled after the damage callback with the current facing; this invokes
no extra physics or detector update. Ordered superseded velocity/animation writes
are retained. Regression tests cover both attack cycles observed in the native
trace, startup and an already-active/completed turn. Pause selection is injected,
with the source's reversed authored endpoints retained. This is not a claim of
Unity random-number or trajectory equivalence.

The state uses20 bytes in the native test build; ordered transient actions use
260 bytes after allowing the13-command startup/turn/attack path. No heap is used
by the controller. A guest link and stack budget are
still required when actor integration makes the code reachable on PS1.

The strict host recognizer in `host/runner.py` identifies both original instances
and records one shared34-sprite library, exact transformed body/AlertRange bounds,
and source sound IDs. It rejects changed FSM/assembly fingerprints, body filters,
gravity/rotation lock and unscaled animation. The cooker tracks this recognizer
in its cache dependencies. `runner_senses.rs` supplies bounded Q16 body/range
queries, LOS and the original three-ray Sweep geometry. Its10 tests cover slopes,
mirroring, collinear rays and strict endpoints. These edge queries do not claim
Box2D filled-collider, contact-slop or trigger-callback parity.

Before enabling these enemies, bind the terrain queries and shared animation art,
health/recoil,
corpse and Geo ownership; bind footsteps/chase/damage/death sounds and Charge
Dust. The two instances share one animation library. Unsupported variants must
remain explicit. Native tests exercise source state/event contracts, not full
original physics or audible/visual parity.

The controlled original run `runner-crossroads70` supplies578 complete samples
per Runner, including two full attack cycles for the first actor. Anticipation
takes25 samples, cooldown5 and idle15 with normal time scale. Each observed lunge
travels about4.08 units over34 physics steps; its69/70 capture samples include
player-hit slowdowns. Do not change the nominal clip duration to69/70 ticks.
The nominal deterministic controller still depends on its caller's simulation
and animation clocks. `.hkpsx/runner70/native-timing.json` records sampled
durations and excludes the inactive Bursting Zombie's incomplete fields.

Local source evidence and reproduction live in
`.hkpsx/crossroads68/RUNNER-CONTRACT.md`, `runner-provenance.json` and
`check_runner_contract.py`; retail records and decompilation stay ignored.
