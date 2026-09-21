# hk-psx

A native Hollow Knight port experiment for original PlayStation, built from the
user's Windows Steam copy in CrossOver. The current build includes
King's Pass scenery and terrain, breakable doors, grass, the Knight's movement
and nail attacks, three source Crawlers, damage, health masks, hazard checkpoints,
nail recoil/pogo, Focus healing with charging/heal audio, a SOUL meter, player and Geo sound effects,
original cave ambience and CD-streamed title music. The title offers Start Game, Options, Controls and Cheats; the
pause menu supports separate sound-effect, ambience and music levels, including mute.
Cheats include invincibility, Pure Nail damage, unlimited SOUL and nine masks.
Press Start, then choose Cheats for health/SOUL refill and extra Lifeblood.
All cheats default off and last for the running session. Charms are not yet
implemented. FPS optimization is paused while the user playtests this build.
Five source Geo rocks can be mined, Crawlers drop Geo, and animated coins bounce
and collect into the wallet HUD. Original pickup and rock-hit sounds are resident.
Three tutorial tablets use the original text, font and interaction triggers.
Crawlers continue patrolling across resident collision regions and leave animated corpses; cut grass has source slash impacts and broken
doors leave persistent fragments that bounce and settle. Source grass, death and
breakable-destruction particles are active, the latter for all 33 supported
Tutorial breakables, and platform interiors use the source tilemap fills. The
Lifeblood cocoon releases two Scuttlers; striking them earns two temporary blue
health masks, which absorb damage before ordinary health. A controller replay
verifies the complete interaction. The final Great Door now shows its three
source damage stages and opens after thirteen hits, followed by the source-derived
blackout, Town entry fade and scripted walk-in. Full original effects remain incomplete. Verified black masks now darken the background using the
PS1's limited blend modes; this is not full source-shader parity.

106 bounded views cover Tutorial_01 and Town0..270/-5..76. One354,032-byte
RAM arena and the same static VRAM slots are reused at scene exits. All views
within the active scene stay resident; walking across internal boundaries needs
no further scene reads or static uploads. Scene exits use a black-screen load
with controller/audio servicing and renderer/animation cache invalidation.

Tutorial needs18 static4bpp pages and1,068 palettes; Town needs6pages/390palettes.
The approved95% static matches and48-pixel scenery cap remain. Knight, enemies
and HUD retain their existing resolution. Provisional71 has8,044B
unallocated before the reserved stack. Visibility tables now occupy one reusable
84KiB scene bank. Final-CUE gate/return/reset replays pass, including travel tox177
in Town; the opening, return and reset images match68 exactly.
Scene exits still pause for loading; this does not establish seamless whole-game traversal. See
[STATUS.md](docs/STATUS.md) and [BUDGET.md](docs/BUDGET.md) for current evidence.
No new30fps or physical-console performance claim is made.

Development now has a [controlled headless Windows reference runner](docs/ORIGINAL_REFERENCE.md)
for repeatable original-game input/state comparisons. It runs an isolated copy
with an internal virtual controller and logs player, enemy and FSM state;
NullGfx runs do not establish image or audible-output parity.

Development now follows a [whole-world import, then shared systems](docs/WORLD_IMPORT.md) workflow. Host import coverage is separate from the two currently playable scenes.

This is an early port, with substantial missing systems and unverified paths.
Flying enemies, full scene scripting, saving, inventory, abilities, NPCs, bosses,
in-game music, Geo spending, Shade recovery and complete-game progression remain unfinished.
See [docs/STATUS.md](docs/STATUS.md) for tested behavior and limitations.
The offline per-scene inventory and neighbour preload planner are documented in
[docs/STREAMING.md](docs/STREAMING.md).

The current game is always `~/Downloads/ps1 games/hk-psx.cue`. There are no
alternate candidate or telemetry discs; every build updates this same entry.

## Build locally

Prerequisites: Python 3.11+, Rust/rustup, Git, mipsel-none-elf-objdump, the Windows
Steam install, and the sibling PSoXide checkout containing the pinned revision.
The first build installs pinned host Python packages in .venv. Rust selects
nightly-2026-03-25 and uses SDK components without the 3D engine.

```sh
# Cook (cached), build the guest, replace the sole disc pair, write the report
# and replay the validation routes in tools/tapes against the final CUE:
cargo run --release -- build
# Skip the replays, or point at another emulator frontend:
cargo run --release -- build --no-validate
cargo run --release -- validate --frontend ../PSoXide-emulator/target/release/frontend
# Explicit Windows installation and SDK source overrides:
cargo run --release -- build --hollow-knight "/path/to/Hollow Knight" --sdk-source "../PSoXide"
```

The driver is the root Rust package (`host/hk-build/main.rs`); it orders the
Python cookers under `host/`, the guest cargo build, the disc packer and the
replays, and fails the build when a route stops early or reports a fault.
Three caches keep it fast: the region cook (per region, misses cooked by one
worker process per scene), the asset stage (skipped when no cooker code, the
region report or a generated output changed) and the menu; the seven route
replays run concurrently. A guest-only change builds and validates in about
two minutes, a cooker change in about five. `--recook` bypasses the caches.

Outputs: dist/hk-psx.exe and `~/Downloads/ps1 games/hk-psx.bin` plus
`~/Downloads/ps1 games/hk-psx.cue`. The PS1 library is the ONLY disc destination;
temporary and diagnostic discs also stay there, with no repository duplicates.
Every build updates the single `~/Downloads/ps1 games/hk-psx.bin/.cue` pair.
There are no candidate, baseline or telemetry disc copies. `--telemetry` selects
an instrumented build at the same path; the legacy `--candidate` flag is an alias
for a normal build. `.hkpsx/build.json` identifies the currently playable EXE,
map and disc hashes. Run gameplay and traversal validators separately; building
does not claim those checks pass. See `docs/STATUS.md` for known failures.

On the title, Up/Down selects Start Game, Options or Controls; Cross/Start opens
the selection and Circle returns. Left/Right adjusts volumes in Options. The
initial selection is Start Game. In gameplay, D-pad moves, Cross jumps, Square
swings the nail. Hold Circle to focus/heal using SOUL. Up + Square attacks
upward; airborne Down + Square attacks downward. At a tablet's Inspect prompt,
press Up or Down to read, Cross to advance/close, or Circle to close.
Start opens/closes the pause menu; its Controls page lists the bindings.
Settings persist for this run. Select resets the development session, including
Geo. Death currently clears the wallet without a recoverable Shade. Health
loss and hazard recovery use source-derived values; death returns to the opening
marker. The development spawn bypasses the cinematic and grants no abilities.
Region changes preserve breakables and enemy state inside the scene. The authored
Tutorial/Dirtmouth gate is replay-verified; progression beyond the current
Town entrance remains unfinished.
Open the CUE to play: the standalone EXE now needs its matching disc mounted.
Room entry shows a loading prompt; a failed read offers Start to retry.

The builder locates Windows files, checks source and generated-output hashes,
cooks when those inputs change, validates the pack, compiles the guest, checks
final executable code for R3000 hazards and writes the disc. --recook forces
conversion; --telemetry includes emulator-only profiling. Normal builds omit it.
Only the tested Windows format is accepted. The macOS copy is never a fallback.
An explicit telemetry build replaces the same disc and EXE; a subsequent normal
build restores the normal instrumentation setting.

## Emulator and validation

```sh
cargo run --release -- build --telemetry
python3 tools/validate.py --emulator ../PSoXide-emulator/target/release/frontend
```

The validation tool runs the actual CUE with the verified embedded/HLE boot path,
scripted pad input, title/entry checks, grass/reset, door progression, enemy combat,
software/hardware display captures, audio, stack profiling and repeatability checks. It writes ignored local
evidence and a JSON summary. This emulator boot
path does not prove real BIOS or console boot. The disc has no Sony system area;
a suitable homebrew loader is required for later hardware testing. No disc is
burned automatically.

```sh
.venv/bin/python -m unittest discover -s tests
cargo test --manifest-path shared/hk-format/Cargo.toml --locked
cargo test --manifest-path shared/hk-sim/Cargo.toml --locked
cargo test --manifest-path shared/hk-cache/Cargo.toml --locked
python3 tools/test_world.py
.venv/bin/python host/inspect_source.py
.venv/bin/python host/inspect_il.py HeroController HeroAnimationController GameManager
```

## Layout and source-only policy

- host/: Unity readers, cooker, local inspection and guest build orchestration.
- shared/: checked cooked format and deterministic simulation crates.
- game/: no_std PS1 guest using PSoXide hardware components.
- tools/: Windows discovery, one-command build and emulator validation.
- docs/: source observations, budgets, format and current handoff.
- .hkpsx/, .psoxide/, .venv/, local/, reference/, data/, build/, dist/, captures/:
  ignored local dependencies, retail inputs, converted content and evidence.

Steam files and saves are read-only. Do not distribute generated assets or discs.
See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for licenses and source tools.
