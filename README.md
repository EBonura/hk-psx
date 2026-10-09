# hk-psx

> **Largely written with agentic coding.** I direct the agents and test their work in two places: PSoXide's emulator, which profiles every cycle, and a real PlayStation, which shows me where the emulator is wrong. Working between them is where the accuracy and the speed come from. [How PSoXide is built](https://ebonura.github.io/PSoXide/how-its-built/)

A native Hollow Knight port experiment for original PlayStation, built from the
user's Windows Steam copy in CrossOver. Nothing is hand-authored: rooms, sprites,
text, audio and gameplay numbers are cooked from the local install by the
Python cookers in `host/`.

The authoritative record of what works is the execution ledger,
[docs/plans/COMPLETE_GAME_PROGRESS.md](docs/plans/COMPLETE_GAME_PROGRESS.md),
and the numbers in it are the ones to plan against. This page is a summary of
it and goes stale first. [SOL_START_HERE.md](docs/plans/SOL_START_HERE.md) is
an old handoff snapshot and is marked as such.

## What is on the disc

- 60 scenes in 943 streamed views: King's Pass, Dirtmouth, Sly's shop, 43
  Forgotten Crossroads scenes including the Ancestral Mound, and 14 Greenpath
  scenes. Rooms stream from CD at scene gates behind a black load.
- The Knight's movement, nail (side, up, down, pogo), Focus and SOUL, damage,
  hazards and checkpoints, death with a Hollow Shade to recover, and the source
  entry sequences at every gate type (side walk-ins, top drops, bottom rises,
  doors).
- Enemies from 14 native controllers written against the source numbers
  (Crawlers, the Husk family, Climbers, Vengeflies, Gruzzers, Baldurs, Aspid
  Hunters, Hatchers, Zombie Shield, Blocker, Pigeons, Egg Sacs and more), and
  the False Knight, which can be fought fairly and killed. Greenpath is still
  almost empty of enemies.
- Geo from rocks and enemies, Sly's shop with its purchase logic, the charm
  board (all 40 charms, six with working effects), Lifeblood, breakable walls,
  the Great Door, arena gates, Elderbug's conversation and the tablets, all with
  the original text and font.
- Benches and a memory-card save with four profiles, two alternating copies
  each and a power-cut check. The save carries the bench, wallet, Shade,
  charms, conversations, script PlayerData and Sly's half of PlayerData, and
  since HKS5 the world: beaten bosses, broken walls, mined rocks, opened
  cocoons, revealed secrets and the ability bits (`game/src/persist.rs`).
  An HKS4 save still loads.
- Title, Options (separate SFX, ambience and music levels, brightness and screen
  position), Controls and a
  Cheats page. Abilities (Mothwing Cloak, Mantis Claw, Monarch Wings, Crystal
  Heart, Shade Cloak, Dream Nail, Vengeful Spirit) are implemented but only the
  Cheats page grants them; no pickup does yet.
- Title music on CD audio and eight ambience channels. There is no area or
  boss music in play yet.

What is missing, in the order it limits play: abilities cannot be earned, most
of Greenpath's enemies and every boss but the False Knight are absent, there is
no map, most enemy and boss sounds are missing, and the emulator measures the
game in the low 20s of frames per second against a 30 fps goal. No hardware
timing has been measured. The ledger names each gap and its cause.

## Validation

Every build replays 26 route tapes from `tools/tapes` against the final CUE and
fails on any fault or on a pinned value (`REQUIRED` in `host/hk-build/main.rs`).
Most boot from a card fixture under `tools/cards` so each tests one thing. Four
do not: the journey (`journey-kings`, `journey-crossroads`,
`journey-false-knight`, `journey-reload`) starts a new game on an empty card,
climbs King's Pass, saves in Dirtmouth, crosses the Crossroads to the stag
bench, kills the False Knight, saves, and reloads to find the boss gone and the
arena open, carrying one card through four power cycles. It uses the
Invincibility cheat to travel and turns it off for the fight, and a pin proves
the kill happened with no cheat on.

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
cargo hk-build build
# Skip the replays, or run them on another emulator frontend than the one
# pinned in emulator.lock.json (the default, built once under .hkpsx/emulator):
cargo hk-build build --no-validate
cargo hk-build validate --frontend ../PSoXide-emulator/target/release/frontend
# The same build with the guest profile-guided by emulator replays of
# tools/tapes/kings-climb and crossroads-gate (or each --tape PATH given).
# The replays run on the emulator pinned in emulator.lock.json, exported from
# ../PSoXide-emulator (or --emulator-source DIR) and built once under
# .hkpsx/emulator, so a given pin always yields the same profile and image.
# Needs a checkout path without spaces; the profile is rebuilt every run:
cargo hk-build pgo
# Explicit Windows installation and SDK source overrides:
cargo hk-build build --hollow-knight "/path/to/Hollow Knight" --sdk-source "../PSoXide"
```

`cargo hk-build` is an alias (`.cargo/config.toml`) for the driver, which lives in
the host tool workspace (`host/Cargo.toml`, with the Rust cookers); the repository root is the PS1 game's
workspace, so the guest's symbols and layout do not depend on where the
checkout sits. The driver (`host/hk-build/main.rs`) orders the
cookers, the guest cargo build, the disc packer and the replays, and fails the build when a route stops early or reports a fault.
Three caches keep it fast: the region cook (per region, misses cooked by one
worker process per scene), the asset stage (skipped when no cooker code, the
region report or a generated output changed) and the menu. The independent
route replays run concurrently and the four journey segments run one after
another beside them, each on the card the one before it wrote, so the journey
sets the validation's wall time. `--recook` bypasses the caches.

The cookers are moving from Python (`host/*.py`) to Rust, one tool at a time,
each port byte-identical to the Python it replaces before the Python goes.
The Rust side is `host/hk-unity` (the reader for the install's Unity files,
with `hk-dotnet` for the managed assemblies), `host/hk-pil` (the image
operations the cookers use, written from the textbook definitions and matching
the earlier Pillow output pixel for pixel), `host/hk-lz4` (the LZ4 block codec,
written from the published format; its blocks decode to the same bytes as the
Python lz4 module's but are about 0.8 percent smaller on the real cook files) and `host/hk-cook` (the
cookers; `props` so far), which the driver runs in-process.

Outputs: dist/hk-psx.exe and `~/Downloads/ps1 games/hk-psx.bin` plus
`~/Downloads/ps1 games/hk-psx.cue`. The PS1 library is the ONLY disc destination;
temporary and diagnostic discs also stay there, with no repository duplicates.
Every build updates the single `~/Downloads/ps1 games/hk-psx.bin/.cue` pair.
There are no candidate, baseline or telemetry disc copies. `--telemetry` selects
an instrumented build at the same path; the legacy `--candidate` flag is an alias
for a normal build. `.hkpsx/build.json` identifies the currently playable EXE,
map and disc hashes. Run gameplay and traversal validators separately; building
does not claim those checks pass. See `docs/STATUS.md` for known failures.

On the title, Up/Down selects Start Game, Options, Controls or Cheats;
Cross/Start opens the selection and Circle returns. Options also sets
brightness (five steps either way, one blended quad over the frame, free at the
default) and the screen position (16 pixels either way in the display range);
like the volumes they last for the session. Start Game lists the four
save profiles: an empty one starts a new game at the opening marker (there is
no opening cinematic) and a used one continues from its bench. In gameplay, D-pad moves, Cross jumps, Square swings
the nail, and holding Circle focuses to heal with SOUL. Up + Square attacks
upward; airborne Down + Square attacks downward. At a bench, Up sits and the
prompt saves to the card in port 1. At a tablet's Inspect prompt, press Up or
Down to read, Cross to advance or close, Circle to close. Start opens the pause
menu, whose Controls page lists every binding. Select resets the development
session. Open the CUE to play: the standalone EXE needs its matching disc.

The builder locates Windows files, checks source and generated-output hashes,
cooks when those inputs change, validates the pack, compiles the guest, checks
final executable code for R3000 hazards and writes the disc. --recook forces
conversion; --telemetry includes emulator-only profiling. Normal builds omit it.
Only the tested Windows format is accepted. The macOS copy is never a fallback.
An explicit telemetry build replaces the same disc and EXE; a subsequent normal
build restores the normal instrumentation setting.

## Emulator and validation

```sh
cargo hk-build build --telemetry
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
