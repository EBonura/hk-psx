# hk-psx working instructions

## Objective

Build a native Hollow Knight port for original PS1 hardware using the user's
Windows Steam copy installed in CrossOver as build input, with a reproducible bring-your-own-assets
workflow like the sibling hl-psx project. Begin with an authentic playable
room and expand toward the full game. Do not confuse a prototype with a
complete port or promise full compatibility before investigating the source.

## First reads

Read README.md, docs/BOOTSTRAP.md and NEXT_PROMPT.md. Re-run tools/doctor.py
because Steam may still be installing or updating. Check applicable local
instructions before working in another repository. Use the psoxide-debug
skill for guest build, emulator and hardware work.

## Implementation

- Prefer Rust and the existing PSoXide SDK for the guest. Select and record
  an exact tested SDK revision and toolchain after examining current siblings.
- Separate host tools, cooked formats and the no_std guest. Python is fine
  for initial analysis. Do not place extraction libraries in the guest.
- Observe the installed Unity version, assemblies, serialization and content
  systems before choosing extractors. Distinguish observations from hypotheses.
- Derive room geometry, sprites, animation and gameplay parameters from the
  local original where possible. Record unresolved behaviours and unsupported
  object types. Do not silently replace the game with generic platforming.
- Keep simulation deterministic and use bounded pools and fixed-point guest
  arithmetic. Make overflow, fractional precision and memory use explicit.
- Measure RAM, stack, VRAM, SPU RAM, primitive counts and CD access. Target
  responsive 60 Hz simulation; rendering cadence must be justified by evidence.
- Use true VBlank pacing, the current R3000 hazard mitigation and final-binary
  hazard checks, and nightly Cargo trim-paths where supported. Verify current
  build mechanics instead of copying stale commands blindly.
- Use existing SDK subsystems before inventing hardware drivers. Do not import
  the 3D engine wholesale without a measured need.

## Local inputs and scope

- Keep exactly one playable disc pair: `~/Downloads/ps1 games/hk-psx.bin` and
  `hk-psx.cue`. The user explicitly rejected candidate, baseline and telemetry
  copies. Update the main pair with the current build; do not leave an older
  version under the main name. Keep diagnostic reports/maps/captures internally.
  An explicit telemetry build replaces this same pair. Report unresolved replay
  failures honestly; a successful build does not imply seamless performance.

- Use the Windows installation in CrossOver. Do not use the macOS Steam copy
  or fall back to it when Windows source discovery fails. Re-derive all version,
  assembly and asset-format assumptions from the Windows files.
- Steam files and saves are read-only build inputs. Do not change the install.
- Disc images must ALWAYS and ONLY be written to `~/Downloads/ps1 games/`,
  the user's PS1 library. This includes temporary and diagnostic BIN/CUE images.
  Do not retain duplicate discs in this repository, even in ignored directories.
- All other game assets, converted data, decompilations and captures
  stay in the ignored directories documented in README.md. Record hashes and
  source IDs in local provenance reports. Keep shared docs free of asset dumps.
- Work in hk-psx. Read sibling projects for patterns; only change a sibling
  when necessary for a demonstrated blocker and explain that change.
- Do not publish, push a remote, burn a disc or alter Steam settings as part
  of routine development. This task starts as a local repository.
- Check licenses before reusing code. Use lawful local inputs and public
  documentation; do not rely on leaked source or redistribute retail content.

## Validation and handoff

Smoke-test the actual final EXE/CUE in PSoXide and inspect gameplay captures.
Guest telemetry flags and current frontend CLI must be verified before
interpreting measurements. Emulator wall-clock speed is not PS1 performance;
GPU/DMA/CD timing limitations mean hardware claims require hardware evidence.
Keep a reproducible route, commands, logs and images for each milestone.
Update docs/STATUS.md when implementation begins with what works, what is
missing, current commands, evidence and the next concrete step. Never report
unrun tests, unobserved timings or placeholder gameplay as original-game parity.
