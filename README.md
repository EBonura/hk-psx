# hk-psx

A native Hollow Knight port experiment for original PlayStation, built from the
user's Windows Steam copy in CrossOver. The last validated playable build includes
King's Pass scenery and terrain, breakable doors, grass, the Knight's movement
and nail attacks, three source Crawlers, damage, health masks, hazard checkpoints,
nail recoil/pogo, Focus healing, a SOUL meter, six resident sound effects and
original cave ambience. The title offers Start Game, Options, Controls and Cheats; the
pause menu supports separate sound-effect and ambience levels, including mute.
Cheats include invincibility, Pure Nail damage, unlimited SOUL and nine masks.
Press Start, then choose Cheats for health/SOUL refill and extra Lifeblood.
All cheats default off and last for the running session. Charms are not yet
implemented. FPS optimization is paused while the user playtests this build.
Five source Geo rocks can be mined, Crawlers drop Geo, and animated coins bounce
and collect into the wallet HUD. Original pickup and rock-hit sounds are resident.
Three tutorial tablets use the original text, font and interaction triggers.
Crawlers leave animated corpses; cut grass has source slash impacts and broken
doors leave persistent fragments that bounce and settle. Source grass, death and
breakable-destruction particles are active, the latter for all 33 supported
Tutorial breakables, and platform interiors use the source tilemap fills. The
Lifeblood cocoon releases two Scuttlers; striking them earns two temporary blue
health masks, which absorb damage before ordinary health. A controller replay
verifies the complete interaction. The final Great Door now shows its three
source damage stages and opens after thirteen hits, allowing entry into Town. Full original effects remain incomplete. Verified black masks now darken the background using the
PS1's limited blend modes; this is not full source-shader parity.

98 bounded regions cover Tutorial_01 and Dirtmouth's left entrance. All their
scene data now stays in one 369,180-byte RAM arena; both scenery atlases stay in
VRAM throughout gameplay. Their CPU copies are discarded after startup uploads,
with exact host-cooked particle tracks using part of the recovered RAM and
leaving about295KiB unallocated for future bounded data. Lossless sharing across regions uses 20 static 4bpp
pages and 1,209 palettes. The existing approved 95% static matches remain; no
further resolution reduction or lossy merging was added. Scenery keeps its
48-pixel long-axis cap; Knight, enemy and HUD resolution is unchanged.

The title loads six ambience loops and the static atlases through the empty
scene arena, then admits both compact scene banks once. Region crossings
select local views without CD reads, decoding or static texture uploads. Eight
animation slots fetch frames from resident RAM and retain cached frames across
same-scene crossings. The long user replay now has zero boundary-loading waits
(previously 478 ticks), with no traversal CD reads or static uploads. The best preserved optimization run
renders at one to four VBlanks per frame (mean 1.957504, approximately30fps on the long
replay);153 of2,965 intervals still exceed two VBlanks, so the strict30fps gate fails. Current build identity
and ongoing experiments are recorded in docs/STATUS.md. This covers the current authored subset, not the full game's residency
or physical-console timing. See [docs/STATUS.md](docs/STATUS.md) for evidence and
[docs/BUDGET.md](docs/BUDGET.md) for measured allocations.

Development now has a [controlled headless Windows reference runner](docs/ORIGINAL_REFERENCE.md)
for repeatable original-game input/state comparisons. It runs an isolated copy
with an internal virtual controller and logs player, enemy and FSM state;
NullGfx runs do not establish image or audible-output parity.

This is an early port, with substantial missing systems and unverified paths.
Flying enemies, full scene scripting, saving, inventory, abilities, NPCs, bosses,
music, Geo spending, Shade recovery and complete-game progression remain unfinished.
See [docs/STATUS.md](docs/STATUS.md) for tested behavior and limitations.
The offline per-scene inventory and neighbour preload planner are documented in
[docs/STREAMING.md](docs/STREAMING.md).

The current game is always `~/Downloads/ps1 games/hk-psx.cue`. There are no
alternate candidate or telemetry discs; every build updates this same entry.

## Build locally

Prerequisites: Python 3.11+, Rust/rustup, Git, mipsel-none-elf-objdump, the Windows
Steam install, and the sibling PSoXide-sdk checkout containing the pinned revision.
The first build installs pinned host Python packages in .venv. Rust selects
nightly-2026-03-25 and uses SDK components without the 3D engine.

```sh
make run
# Build without opening the emulator:
python3 tools/build.py
# Explicit Windows installation and SDK source overrides:
python3 tools/build.py --hollow-knight "/path/to/Hollow Knight" --sdk-source "../PSoXide-sdk"
```

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
python3 tools/build.py --telemetry
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
