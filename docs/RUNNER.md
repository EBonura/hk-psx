# Crossroads Zombie Runner implementation

The controller in `shared/hk-sim/src/runner.rs` now has an explicit branch in
the guest actor pool, selected by `ActorController::Runner`. Since build 89 the
recognized Runners are admitted on the disc with the resident audio bank
(`game/src/runner_audio.rs`, voices 21 to 23); Charge Dust and source death
effects are counted, not presented. Before that, admission was disabled until
presentation and Crossroads scene bindings were ready; native tests exercise the actual guest module with a recording event
sink. The production sink rejects unbound Runner presentation instead of silently
discarding sound/dust events. Existing Crawler behavior remains separately tested.

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

The controller state uses20 bytes in the native test build; ordered transient actions use
260 bytes after allowing the13-command startup/turn/attack path. No heap is used
by the controller. RunnerRuntime adds its animation/velocity/RNG state for32B;
the current native Actor is128B and the32-slot enemy pool is4KiB. Final PS1
link/stack costs must be checked alongside the active scene metadata.

The strict host recognizer in `host/runner.py` identifies both original instances
and records one shared34-sprite library, exact transformed body/AlertRange bounds,
and source sound IDs. It rejects changed FSM/assembly fingerprints, body filters,
gravity/rotation lock and unscaled animation. The cooker tracks this recognizer
in its cache dependencies. `runner_senses.rs` supplies bounded Q16 body/range
queries, LOS and the original three-ray Sweep geometry. Its10 tests cover slopes,
mirroring, collinear rays and strict endpoints. These edge queries do not claim
Box2D filled-collider, contact-slop or trigger-callback parity.

Guest integration binds terrain queries, real cooked clip completion, facing,
health/recoil order, terminal corpses and once-only Geo payout. Actors retain
their own resident collision view and suspend on unavailable terrain. For these
source Runners the solver foot has a983-Q16 clearance: native rest y2.45249987
over source floor y1 with collider bottom-1.4375 establishes about.015 units.
Contact and sensing retain the actual source box. This avoids a false collinear
floor-as-wall hit while remaining a bounded solver adaptation, not Box2D parity.

The source art pass now cooks seven live and two corpse clips into a shared bank:
both actors use43 frame references and34 textures, totaling13,252 stream bytes.
All live and corpse clip bindings remap during scene-bank append. Two actual
source probe packs with different preceding clip counts pass and repeated
postpasses are byte-identical. Evidence: .hkpsx/runner-art70/report.json.
Runner corpses use their own zero spawn offset, .2 bounce factor, Single Air
clip and eight-frame Land clip. Crawler offset, .3 bounce distribution and clip
timing remain unchanged; equivalent validated Crawler assets can live in other
bundles. The shared terrain solver still approximates source contacts/friction.

Ordered AudioStop/AudioPlay/ChaseSound/DustStart/DustStop/Destroy events include
source identity, position and facing. The caller owns their presentation and
scene/reset cleanup. Native tests do not establish audible/visual parity.
Footstep/chase playback, Charge Dust, source death effects and the well connection
remain required before canonical admission.

`host/hk-cook/src/runner_audio.rs` prepares a full20,464B source bank in ignored storage.
Movement uses8kHz; both creature calls use11.025kHz. At the current SPU tail
495,792 it ends516,256, leaving8,032B. Source pitch/gain, full sample counts,
transport flags, hashes and layout are verified; no trimming is used to fit.
This is not installed guest audio. Proposed voices21/22/23 and any resulting
simultaneous-call arbitration still require an explicit playback policy.
Evidence: .hkpsx/runner71/audio/report.json; regenerate or verify with
`cargo run --release --manifest-path host/Cargo.toml -p hk-cook -- runner-audio [--verify]`.

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

## Variants (build 98)

The Zombie Swipe FSM is shared by the Runner, the Husk Bully (Zombie Barger)
and the Hornhead. `hk_sim::runner::Params` carries the placement's walk speed,
lunge speed and pause wait/time endpoints, `runner_senses::Shape` its body and
alert boxes, and `host/runner.py::fsm_fingerprint` verifies the FSM structure
and scalar fields instead of the per-scene serialization. Bargers keep the
Runner's 1.5 walk with a 14 units/s lunge and shorter pauses; Hornheads walk
at 2.5 with a 9 units/s lunge and recoil at 15.

The Leaper (build 99) is the same walker with `Attack::Leap`: the Attack clip's
trigger frame launches at ((hero x - self x) * 1.25, 20) under gravity scale
.8, bottom contact plays Land, then Idle .5 s and StartWalker; it has no damage
or recoil transitions.
