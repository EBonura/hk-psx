# Controlled headless Windows reference

`tools/reference_game.py` runs an isolated copy of the user's Windows Hollow
Knight installation under CrossOver with Unity `-batchmode -nographics`.
It injects a managed test driver and a virtual InControl device. It does not
use desktop input, a game window, the macOS Steam copy, or the PS1 disc.

## Prepare and run

Run from the repository root:

```sh
python3 tools/doctor.py
python3 tools/reference_game.py prepare
python3 tools/reference_game.py run --name movement-example --frames 300 --tape tools/reference/tapes/movement.csv --timeout 90
```

Run names must be new single directory names. Reports live under
`.hkpsx/og-reference/runs/<name>/`. A successful process exit is insufficient:
the runner checks actual Tutorial gameplay, consecutive consumed input frames,
the complete input schedule, controller attachment, and gameplay exceptions.
It returns nonzero on a failed check or watchdog timeout.

Preparation needs the locally installed CrossOver and .NET SDK 9. The tested SDK
is 9.0.318, downloaded from Microsoft's release metadata with its SHA-512 verified;
local dependency provenance is `.hkpsx/og-reference/deps/sdk.json`. The default
executable is `.hkpsx/og-reference/deps/dotnet/dotnet`. On another machine pass
`--dotnet /absolute/path/to/dotnet`. Pass `--crossover-bin /absolute/path/to/bin`
if CrossOver is not installed in `/Applications` and cannot be discovered from
its running wineserver. The current machine uses an App Translocation path;
do not copy that temporary path to another machine.

The patcher uses Mono.Cecil 0.11.6, pinned in its project file; NuGet restores it.
Cecil is MIT licensed ([upstream license](https://github.com/jbevain/cecil/blob/master/LICENSE.txt)).
The managed driver compiles against the *original* game's assemblies, not a
previously modified assembly. No Unity/retail DLLs are tracked or distributed.

## Input and state control

CSV input has header `test_frame,buttons`. Frames are nonnegative decimal
integers, strictly increasing. Masks are active-high PS1 bits in decimal or
`0x` hexadecimal. Each row replaces the entire held mask. The last value stays
held until an explicit zero event releases it. Blank lines and whole-line `#`
comments are accepted. Unknown button bits and ambiguous schedules are rejected.

| Button | Mask | Original action |
| --- | --- | --- |
| Start | `0x0008` | Pause |
| Up / Right / Down / Left | `0x0010` / `0x0020` / `0x0040` / `0x0080` | Direction |
| Circle | `0x2000` | Cast/Focus (`HeroActions.cast`) |
| Cross | `0x4000` | Jump |
| Square | `0x8000` | Nail attack |

Circle deliberately binds `cast`: the installed original's `ListenForCast`
action reads it for Focus/spells; its similarly named `focus` field is unused.
Additional abilities/buttons remain to be mapped and tested.

For a live programmatically controlled run, omit `--tape`, select a sufficiently
long frame budget, and monitor `driver.log` for `READY` or read `state.csv`.
Send commands from another process:

```sh
# Start this in a separate terminal/process and wait for READY:
python3 tools/reference_game.py run --name live-test --frames 12000 --timeout 180

python3 tools/reference_game.py command --name live-test 'buttons 0x20'
python3 tools/reference_game.py command --name live-test 'buttons 0'
python3 tools/reference_game.py command --name live-test 'teleport 120 15'
python3 tools/reference_game.py command --name live-test 'scene Town left1'
python3 tools/reference_game.py command --name live-test 'quit'
```

Commands are written atomically to the run's mailbox and acknowledged in
`driver.log` with Unity/test frame numbers. Acknowledgement, not file creation,
proves execution. This is a latest-command mailbox: wait for acknowledgement
before sending another command. Live buttons cannot override a loaded tape.
Explicit quit is accepted for a live run that already recorded valid gameplay.

Teleport and direct scene loading are **test setup**, not evidence of authentic
room-transition timing. Teleport resets the hero's velocity; scene setup uses
an original build-settings scene and named entry gate through GameManager.
Use controller-driven traversal for timing/door comparisons. These setup
commands do not grant abilities or replace the original physics/AI.

## Initialization and timing

Tested original: Unity 6000.0.61f1, Windows Steam build 22529139. The original
Assembly-CSharp SHA-256 is
`e9048ef6a633970f735e01ec166d3959f610eaea7a88d827d48d67b1e5fb87bd`.
Preparation fingerprints **every installed game file** and original save input,
verifies all retained cloned files except the deliberately patched assembly,
and rejects stale copies after updates. Run reports bind the driver/source and
patched assembly hashes and recheck original inputs before and after execution.

The isolated bootstrap confirms the default language, leaves menu input,
loads the original `Knight_Pickup` scene additively, and waits for its original
HeroController. This matches the prerequisite in `OpeningSequence`. It then
calls `OnWillActivateFirstLevel` and `GameManager.LoadScene("Tutorial_01")`.
The opening movie is bypassed as explicit scene-test setup. The splash Animator
is retained; an early hypothesis that it required bypassing was disproved.
Gameplay logic and state-machine actions remain original. Managed audio API
call sites are instrumented as described below; each wrapper retains the native call.

The normal first-landing sequence is retained. Its `Initial Fall Impact` FSM
relinquishes control, waits 0.75s and 3.25s in successive states, then waits for the
get-up animation before returning control. Tape frame 0 starts only after the
original reports PLAYING, grounded, not transitioning, accepting input, and an
attached controller for 30 consecutive driver frames.

Unity `Time.captureDeltaTime` is set to 1/60 for the reference test clock. The
installed original's physics timestep remains unchanged (observed approximately
0.02s/50Hz). Readiness waits for their common 100ms phase, adding at most
five frames; physics is not reset. These are different clocks: compare elapsed simulation time and
logged physics steps, not PS1 vs Unity frame numbers blindly. Each tape mask is
queued in LateUpdate for the following real InControl update. Input history,
WasPressed/WasReleased, action processing, and hero movement remain original.
Every original input-device update is logged, including extra updates during
scene loading. Several such updates can share one scheduled test frame; the
held mask remains unchanged until the next LateUpdate. Validation checks all
logged device updates against the tape, not just the final sampled mask.
RNG and asynchronous loading are not globally made deterministic;
repeatability claims apply only to the measured scenario.

## Validation evidence

Final 300-frame runs `movement-phase-1` and `movement-phase-2` start at different
Unity frames (1440 and 1434), but match all masks, velocity, health, control state
and relative physics-step counts. Maximum position difference is 0.00001 world
units, within the 0.0001 comparison tolerance. The earlier unaligned test could
vary by a physics step; it is superseded by the phase-aligned runner.

Run a repeat comparison with:

```sh
python3 tools/reference_game.py compare --left movement-phase-1 --right movement-phase-2
```

The comparison reports explicit fields and tolerance; it does not compare every
random enemy/particle state. Reports are under `.hkpsx/og-reference/comparisons/`.
The `live-v2` run verifies actual mailbox movement and release, direct Town
loading, teleport to (120, 15), and command shutdown: 128 sampled gameplay frames,
130 original input updates, no gameplay exceptions. The live test preceded the
readiness-phase alignment; its control protocol is unchanged. One final movement
run and the live run reported an original CameraController exception during
process teardown; reports preserve it separately.

Nine host regression tests exercise false-success rejection, missing/skipped
frames, mismatched sampled/native-tick masks, decimal-mask parsing, exception
classification and command restrictions:

```sh
python3 -m unittest tests.test_reference_game
```

## Reports and limits

- `state.csv`: consumed input frame/mask/tick, Unity frame/time/fixed time, hero
  position/velocity/health, game/transition/input states, delta times and physics
  step count. Samples are taken in LateUpdate after original frame logic.
- `input-events.csv`: every virtual-device update, its queued test frame, Unity
  frame, original input tick and applied mask, including extra scene-load polls.
- `observations.csv`: original FSM state changes, enemy HealthManager poses/HP,
  destruction, and sampled AudioSource configuration. Hierarchies identify
  objects; runtime instance IDs are process-local, not source asset IDs.
- `driver.log`: setup, READY, command acknowledgements, watchdog/errors and stop.
- `audio-calls.csv`: intercepted managed Play/PlayOneShot/Stop/Pause/UnPause,
  PlayClipAtPoint, mixer snapshot TransitionTo and mixer SetFloat requests.
  Rows include the original caller/IL offset, owner hierarchy, clip/snapshot,
  source pitch/volume, one-shot scale, Unity time and input-update counters.
- `hero-audio-config.csv`: once-ready reflected footstep and landing object
  fields, including configured clip names, sample counts and frequencies.
- `unity.log`: original runtime diagnostics, including the NullGfxDevice proof.
- `run.json`: version/input provenance and validation; gameplay exceptions fail
  the run. Exceptions after the explicit completion marker are separately
  reported as teardown diagnostics.

Observation discovery occurs on scene loads and every 30 samples, with explicit
caps of 2048 FSMs/1024 actors/512 audio sources. Coverage-limit rows report overflow.
Polling can miss intermediate FSM transitions, transient objects and one-shot
sounds. Audio rows in `observations.csv` prove configuration only.

Audio call tracing wraps exact signatures in the original Assembly-CSharp DLL;
preparation reports every patched caller, original opcode/signature, and the
remaining audio API references. Native Unity calls still receive their original
arguments exactly once, and their return values/exceptions are retained. A row
has phase `request`: it is written before the native call and does not prove
that the call succeeded or produced audible sound under NullGfx/no audio device.
The queued test frame may precede its next input update; use the Unity frame and
input counters to align events. Source `.clip` is not substituted for the explicit
PlayOneShot clip, and volume scale is recorded separately from source volume.

Coverage excludes native engine starts (`playOnAwake`), reflection/delegates,
other assemblies, unsupported overloads, and calls before driver initialization
or after trace disposal. An original code path that calls
another traced game helper can still be observed at that helper's final audio
call. The local PlayMaker DLL had no direct AudioSource/mixer references in the
inspected build. Mixer automation, audio output, complete cue coverage,
and original sample asset IDs are not inferred from these rows. Runtime IDs are
process-local; source clip linkage requires serialized asset provenance.
Wrapper compilation/static validation is separate from a successful reference
replay; inspect the new run's audio CSV before making missing-SFX claims.
The nine compiled wrappers passed an IL check for exactly one native call,
unchanged argument ordering, a preceding log call, and no exception handler
around the native invocation. Local evidence is
`.hkpsx/audio-trace-validation/wrapper-validation.json`.

NullGfx produces no rendered image. It cannot establish visual parity, shader/
particle coverage, audible output, or on-screen culling-dependent behaviour.
Gameplay traces are useful evidence within these limits; visual comparisons
will require a separate rendered capture path. See Unity's
[desktop headless mode documentation](https://docs.unity3d.com/6000.0/Documentation/Manual/desktop-headless-mode.html).

The APFS clone, isolated bottle, saves, dependencies, binaries and logs all stay
under ignored `.hkpsx/og-reference/`. The original Steam install and saves are
read-only inputs. The Patcher changes only the copied assembly: startup driver
hooks, persistent-data-path redirection, and managed audio call instrumentation.
The isolated bottle additionally
separates platform preferences/saves from the Steam bottle. Normal guest/disc
build mechanics are unaffected.

## Native particle scaling probe (68)

After preparing the current driver, an explicit diagnostic command runs fresh
synthetic particle systems inside the isolated native Unity player:

```sh
python3 tools/reference_game.py command --name LIVE_RUN particle-probe
```

Wait for the command acknowledgement in `driver.log`, then inspect
`particle-scale.csv`. The command does not modify retail scene objects or
prefabs. It compares Shape and Hierarchy scaling at unit scale, a uniform
0.81203 scale, and the source Town grave's nearly uniform XY scale with its
slightly different Z scale. Each case emits one particle and samples its initial
state and state after native simulation. Separate cases isolate launch speed,
world-space velocity, world-space force and velocity limiting. CPU `BakeMesh`
also records the actual billboard dimensions under NullGfx; this verifies mesh
size, not final rendered lighting or rasterization.

The native measurements show Hierarchy scaling affects initial speed, the
world-space velocity and force modules, and rendered size. The velocity-limit
magnitude remains in world units. `Particle.GetCurrentSize` returns the unscaled
size even though the baked billboard has scaled dimensions. This distinction
is consistent with Unity's [scaling-mode documentation](https://docs.unity3d.com/ScriptReference/ParticleSystemScalingMode.html).

The three Town grave emitters use Circle shapes confined to XY, world-space
simulation, zero gravity modifier and no Z velocity or force. Their serialized
XY scales differ by about 1.2e-7 (below Q16 scale precision); Z differs by about
0.00024. The host accepts only uniform-XY, unsheared, planar Hierarchy emitters
and converts size, speed, force and velocity to the existing guest world-space
fields. It preserves emitter geometry, lifetime, angular values, velocity limit
and dimensionless collision settings. Other scaling modes, tilted/nonplanar
Hierarchy motion, nonuniform XY and nonzero gravity fail explicitly pending
further native validation. Existing Shape-mode effects retain their parameters.

Evidence is retained in `.hkpsx/town68/particle-validation.json`,
`break-particles.json`, `break-transforms.jsonl`, and the isolated reference runs
`particle-scale68` and `particle-planar68`. The first uniform probe's complete
reference run passed. The second command produced and acknowledged all 48
native samples, but later ordinary gameplay slowed severely and ended through
the watchdog; launcher cleanup also timed out before the process subsequently
exited. Its CSV and baked-mesh contracts pass independently, and original inputs
were rechecked unchanged. Do not cite that second run as successful gameplay
or claim full Unity particle/RNG/collision parity from these bounded probes.

## Sampled Runner and Climber trace

Every actual driver capture also writes `enemy-trace.csv`. This is a separate,
read-only observer; it does not change `observations.csv`, send FSM events, run
extra physics queries, or invoke enemy callbacks. Rows join to `state.csv` by
`test_frame` and `unity_frame`, and include the same cumulative `physics_steps`.
Scene, hierarchy and controller instance ID identify actors across captures.

The trace records health/death, active/enabled status, transform and Rigidbody2D
positions, rotation and velocity, and the animator's native `currentClip`,
`previousFrame`, `clipTime` and `state` fields. The column
`animation_previous_frame` is specifically that backing field, not a frame
recomputed from elapsed time. Walker samples add state, facing, stop reason,
pause/walk/turn timers, the `Zombie Swipe` FSM state, and cached
`LineOfSightDetector.canSeeHero` / `AlertRange.isHeroInRange` values. Climber
samples add direction, clockwise orientation, previous positions and whether a
turn coroutine handle is present. Its phase is explicitly `not_exposed`: a Unity
coroutine handle cannot establish which coroutine instruction is executing.

Discovery includes inactive objects in loaded scenes, on scene changes and every
30 captures. Tracking is capped at 128 instances per controller type; Unity's
per-type discovery API still allocates the initial complete result. Scan counts,
omitted actors, sampled destruction notices and logging errors are written to
`enemy-trace-notices.log`; absent components or fields appear in each row's
`missing` column. Check these before using a run as evidence.

These are LateUpdate snapshots, not callback instrumentation. They cannot prove
intermediate FSM/coroutine states, execution order within a frame, or actors
created and destroyed between discovery passes. Use position, velocity,
animation and state together when comparing a controlled route with the guest.

The controlled `runner-crossroads70` run captured623 gameplay frames and2,671
managed audio requests with unchanged original inputs. Its initial validation
failed because an audio request switched from frame time to the advancing
physics clock within the same Unity frame. The corrected validator accepts this
specific clock-context switch while retaining sequence, frame, input and fixed
clock checks. Raw evidence and its initial failed report remain unchanged;
`.hkpsx/enemy-trace70/audio-clock-revalidation.json` binds the revalidation to
their hashes. This validates recorded requests, not audible output.

Both active Runners and both Climbers have578 samples with complete fields.
An inactive Bursting Zombie has missing animator/Swipe fields and is explicitly
excluded from Runner behavior comparisons. The first Runner completes two attack
cycles. Their69/70-sample lunges contain player-hit slowdowns: both advance34
physics steps and approximately4.08 world units, rather than69/70 ordinary game
ticks. Join `time_scale` from state.csv and use scaled time for such comparisons.
The driver and enemy trace format Single values at different precision, so join
integer capture IDs exactly and compare numeric clocks with float tolerance.
`.hkpsx/runner70/analyze-native-trace.py` reproduces `native-timing.json`.
The route uses explicit scene load/teleport commands and does not establish
normal well-entry timing or guest trajectory parity.
