# hkref: the original game next to the port

`hkref` runs the original Hollow Knight (the macOS Steam build, instrumented) and the
PS1 port (headless PSoXide) from one input tape and one scene profile, turns both
into per-tick traces, and reports where they differ: hero motion, enemy state, hits,
sounds, screenshots at matched ticks. It also sweeps scenes of the original for
ground truth (every sound a scene plays, every actor in it).

It replaces the old Windows and CrossOver runner. The original runs natively on macOS
in batch mode: no window, no audio (the sandbox denies the audio services and the
driver sets `AudioListener.volume = 0`).

## What it does not ship

The repository contains no game files. The in-game driver (`mod/`, C#) is compiled
against the assemblies of the Hollow Knight you already own, and the patcher
(`patcher/`, Mono.Cecil) rewrites **a private copy** of `Assembly-CSharp.dll`. The
copy is an APFS clone (`cp -c`) of your Steam install, made inside the work directory,
so nothing is written to the Steam install. The patcher refuses to run unless the
target carries the `.hkref-copy` marker that `hkref build` puts in its own clone, and
refuses any path under `steamapps`. Nothing a run produces (traces, screenshots,
the clone) belongs in git.

Note this is a different thing from the Windows copy the cook reads (`AGENTS.md`): the
macOS build is only an oracle to measure against, never an input to the disc.

## Requirements

* macOS with an APFS volume, Apple silicon or Intel. The Steam game is an x86_64 Unity
  2020.2.2f1 Mono build; on Apple silicon it runs under Rosetta. Tested: Hollow Knight
  1.5.78 from Steam.
* The game installed through Steam at
  `~/Library/Application Support/Steam/steamapps/common/Hollow Knight/hollow_knight.app`
  (or set `HKREF_GAME` to the `.app`). Steam does not need to be running.
* A .NET SDK 9 (compiles the driver, builds the patcher). Found in this order:
  `HKREF_DOTNET` (directory holding `dotnet`), `.hkpsx/og-reference/deps/dotnet`,
  `DOTNET_ROOT`, `/usr/local/share/dotnet`, Homebrew, `~/.dotnet`. The first patcher
  build restores Mono.Cecil 0.11.6 from NuGet (network, or an existing NuGet cache).
* Rust (stable) for hkref itself; `cargo build --release --offline` works once
  `serde_json` and `png` are in the cargo cache.
* For the port side: a built `hk-psx` disc, its link map, and the PSoXide headless
  frontend (`HKREF_FRONTEND` or `port.frontend` in the profile).

## Run it

```sh
cd tools/hkref
cargo build --release --offline
export HKREF_DOTNET=/path/to/dotnet-root      # if not auto-detected
export HKREF_FRONTEND=/path/to/PSoXide/target/release/frontend
W=~/hkref-work                                 # any scratch directory, not in the repo
target/release/hkref --work $W build           # clone the game, compile the driver, patch the clone
target/release/hkref --work $W all profiles/kp-playtest.json     # port, original, diff, sheets
target/release/hkref --work $W sweep profiles/gruz-mother.json 1,2,3,4,5   # RNG seeds
target/release/hkref --work $W scenes profiles/scene-sweep-crossroads.json # per-scene sweep
```

Commands: `build`, `tape gen|info`, `port`, `og`, `sweep`, `scenes`, `diff`, `sheet`,
`all`. Output goes to `$W/runs/<profile>/` (traces, `diff.md`, plots) and
`$W/approval/` (side-by-side PNG sheets).

Tests: `cargo test --release --offline`.

### Do not skip: SteamAppId

Every launch of the original must carry `SteamAppId=367520` and `SteamGameId=367520` in
its environment (hkref sets them, and `build` also writes `steam_appid.txt` next to the
binary). Without them the game calls `SteamAPI.RestartAppIfNecessary`, which starts the
**Steam client** through `steam://run` and can download a bootstrap into the fake
`HOME`. If you start the binary by hand, set both variables first.

Other launch details hkref handles: a fake `HOME` inside the work directory (the
game's prefs and saves stay out of yours), the sandbox profile that denies CoreAudio,
exact-PID bookkeeping in `$W/PIDS`, a retry on the rare native crash.

## Profiles

A profile is JSON: a `port` object (disc, link map, memory card or event list/tape,
the guest symbols to watch), a `window` (how many sim ticks), and an `og` object (the
scene and entry gate to warp to, which enemies to follow and which FSM states mean
which event, the RNG seed, the effects to switch off). Paths: `~/` is your home, a
relative path is relative to the repository root. See `profiles/`.

## What the driver observes

Per run, in `$W/runs/<name>/og/`: `state.csv` (hero position, velocity, health, input
state per frame), `actors.csv` (every active HealthManager near the hero with its
PlayMaker FSM states), `audio-calls.csv` (every Play/PlayOneShot with clip and
caller), `camera.csv`, `input-events.csv`, `driver.log`, `frames/*.png`.

Timing: the original's physics step is 50 Hz and the port's sim is 60 Hz, so
comparisons are in sim ticks of the port; the original's input latency is one or two
ticks depending on where a press falls against its fixed step.

### Survey modes (behaviour comparison)

`scenes` visits scenes of the original one at a time. Besides the tour it has two modes that give the
enemy-behaviour comparison in `tools/behaviour-parity` something to compare against, switched on by
`og.env` in the profile (`og.actor_stride` 1 writes every frame):

* poke: `{"HK_REFERENCE_SURVEY_POKE": "60"}` strikes each distinct enemy through the game's own
  `HealthManager.Hit` with the no-charm nail (5 damage, from the left unless
  `HK_REFERENCE_POKE_DIRECTION` says otherwise), every `HK_REFERENCE_POKE_GAP` (30) frames from
  `HK_REFERENCE_POKE_FIRST` (30), and writes `survey-pokes.csv` (frame, hit points before and after).
* approach: `{"HK_REFERENCE_SURVEY_APPROACH": "480"}` stands the Knight on the terrain
  `HK_REFERENCE_APPROACH_DISTANCE` (14) units to the left of each distinct enemy (the right if the left has
  none) and walks him toward it at `HK_REFERENCE_APPROACH_SPEED` (4) units per second, so detection ranges
  and first reactions are measured at a real distance.

Both replace the tour; the idle settle (`sweep_frames`) comes first. The Knight is invincible and frozen
while the scene settles (no trigger events), which is what `behaviour-parity` models on the port side.

## Limits

* The port exports little state; the guest's `HK_TRACE` block (game/src/trace.rs, built with `HK_GUEST_FEATURES=trace`) would widen it.
* Random AI choices differ by seed: compare against a spread (`sweep`), not one run.
  The driver seeds `UnityEngine.Random` (`HK_REFERENCE_SEED`) so a run repeats.
* Open-loop tapes diverge after the first hit.
* Muting was verified by the sandbox and the volume, not by ear.
* Screenshots are batch-mode Metal renders at 960x540 of every camera in depth order,
  with the original's four camera image effects on (the colour grade, see below).

## The colour grade in batch mode

The main camera carries four image effects: `BloomOptimized`, `FastNoise` (film grain),
`ColorCorrectionCurves` and `BrightnessEffect`. In a batch-mode screenshot they used to
turn the whole picture black except the hero's light, the hero and the HUD. Bisecting
the effects one at a time showed `FastNoise` alone caused it: the shader draws grain
into a private render texture that the effect redraws only when
`Time.frameCount % frameRateMultiplier == 0` (every fourth frame in the game) and
re-creates, empty, whenever the render size changes. A screenshot renders at a size of
its own, so on three frames in four the grain texture was an empty one and the
multiply blacked the picture out. The curves, bloom and brightness effects are fine
(their LUT and materials were checked: `HK_REFERENCE_DUMP_POST=1` prints them).

The driver now sets `frameRateMultiplier` to `Always` for the duration of a shot and
restores `UnityEngine.Random.state` afterwards (the grain draws from it, and a
screenshot must not change what the AI rolls). `og.fx_off` in a profile can still
switch effects off. `hkref imgstat` prints a PNG's mean colour, which catches a black
frame; `HK_REFERENCE_FX_FRAMES=<test frame>` writes the per-effect variants
(`-only-X`, `-all-but-X`, `-nopost`, `-raw`, `-white`) for a frame.
