# Bootstrap observations

Recorded 2026-09-08 during setup. Recheck all installation and toolchain state.

## Authoritative local game source: Windows in CrossOver

- User explicitly selected the Windows CrossOver copy and excluded macOS Steam.
- Steam app ID: `367520`.
- Install observed at
  `~/Library/Application Support/CrossOver/Bottles/Steam/drive_c/Program Files (x86)/Steam/steamapps/common/Hollow Knight/`.
- Windows executable: `hollow_knight.exe`, 672,256 bytes.
- Unity data: `hollow_knight_Data/`.
- `globalgamemanagers` contains the version string `6000.0.61f1` in its header.
- `Managed/Assembly-CSharp.dll`: 3,652,608 bytes.
- `Managed/Assembly-CSharp-firstpass.dll`: 95,744 bytes.
- `Managed/PlayMaker.dll`: 212,480 bytes.
- `Managed/TeamCherry.TK2D.dll`: 165,888 bytes.
- Manifest fields observed: build ID `22529139`, StateFlags `4`,
  BytesToDownload and BytesDownloaded both `1091282960`, BytesToStage and
  BytesStaged both `5231995691`. These are observed installation metadata;
  no complete content-integrity check has been run.

The Windows Unity data and managed assemblies are present. The starting approach
remains a native reimplementation with host-side content extraction.
No assemblies have been decompiled, scenes parsed or assets extracted yet.
PlayMaker is present; how much gameplay it controls is still unknown.
The earlier macOS source observations are superseded. Do not use that copy as
input or carry its Unity version, assembly sizes or format assumptions forward.
The doctor now searches CrossOver bottles and resolves Windows drive paths;
it rejects macOS layouts, including explicit .app or Resources/Data overrides.

## Nearby reference projects

All are siblings under `~/Desktop/repos/`:

- `hl-psx`: local install discovery, asset cooking, source-only packaging,
  standalone user build, memory reporting and PS1 runtime lessons. Its root
  README documents `cargo hk-build build` and explicit source overrides.
- `PSoXide`: the current workspace contains SDK, engine, emulator and tooling.
  See `sdk/README.md`, `sdk/examples/hello-input/`,
  `engine/examples/game-breakout/`, `docs/playtest-profiling.md`, and
  `tools/hazard_scan.py`.
- `PSoXide-editor`, `PSoXide-emulator`, `PSoXide`: also present. hl-psx's
  README describes a repository split. Resolve ownership and tested revisions
  before choosing dependencies; do not assume every checkout is interchangeable.
- `pico8-psx` / `psxcel`: potential 2D/controller examples, to inspect as needed.

hl-psx currently declares `nightly-2026-03-25`; that is an observation, not a
toolchain selection for hk-psx. No dependencies have been copied or pinned here.

## Next evidence needed

1. Stable completed install fingerprint, actual loaded assembly set and game version.
2. Scene-name mapping, source sprite collections/atlases and animation formats.
3. Collision, camera, room transition and gameplay/FSM representation.
4. Tested minimal PSoXide guest build and compatible disc tooling.
5. Measured room residency and art quality at candidate PS1 resolutions.

The next task should produce implementation and evidence, starting with the
smallest original room that exercises the pipeline. Setup alone establishes
neither feasibility of the whole game nor a working PS1 executable.
