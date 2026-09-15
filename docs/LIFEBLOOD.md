# Lifeblood

The current authored subset contains the Tutorial health cocoon and its two
Health Scuttlers (Lifeseeds). This adds a separate temporary blue-health pool;
it does not implement Lifeblood charms or full original effect/audio parity.

## Source behavior

All observations come from the installed Windows Unity build, read without
modifying the installation. Detailed structures, method hashes and asset
provenance remain under ignored `.hkpsx/lifeblood/` and
`.hkpsx/lifeblood-provenance.json`.

- `level6:12337`, HealthCocoon on GameObject 973, sits at approximately
  `(56.9749,62.0903)`. Its nail trigger is the source box
  `[56.0283,62.0453,58.4683,65.4475]`. One valid hit opens it, disables both
  source colliders, and releases exactly two `sharedassets6.assets:436`
  Health Scuttlers. Ordinary decorative plant shells are separate Breakables.
- The authored fling uses speed 10–15, angle 40–140 degrees and ±0.5 units of XY
  spread. ScuttlerControl randomizes scale 1.35–1.5 and maximum speed 6–9. Its
  Rigidbody gravity scale 0.6 combines with source gravity 60 to give 36.
- Scuttlers grant health when struck, not by touching the Knight. Their first
  0.25 seconds reject hits. After landing, the three-frame12 fps land animation
  finishes before running. Run accelerates away from the Knight by 0.3 per
  Update. Its wall response launches at speed 5, angle 50–70 or110–130 degrees,
  and resumes running after0.5 seconds.
- ScuttlerControl.Heal waits 1.2 seconds, then sends `ADD BLUE HEALTH`. A registered
  Unity scene-unload callback performs a pending grant early and unregisters
  itself, preventing duplicate grants. `resources.assets:20959` adds exactly
  one to `healthBlue` per event.
- PlayerData.TakeHealth consumes `healthBlue` first and applies only positive
  overflow to ordinary health. Focus heals ordinary masks without replenishing
  blue masks. No-charm UpdateBlueHealth resets the temporary pool; MaxHealth
  calls this reset, while MaxHealthKeepBlue explicitly preserves it.

## Runtime integration

`game/src/lifeblood.rs` owns a fixed two-bug pool and one cocoon-open flag.
`Vitals.blue_health` stores the separate pool. Damage absorption occurs after
existing invulnerability/death/hazard admission checks, so ignored hits consume
neither pool. A blue-only hit still produces the normal admitted recoil or
hazard response.

Main ticks existing Lifeblood state before dispatching the current nail strike.
Newly spawned bugs and newly scheduled rewards therefore retain their full
15- and 72-tick delays. Lifeblood simulation pauses with gameplay and global
hit-freeze. Loading another view of the same Unity scene does not reset it.
Actual scene exits flush pending grants once and discard unstruck loose bugs,
while preserving the opened cocoon. SELECT restart and completed death respawn
reset both cocoon state and Vitals. Hazard respawns retain the remaining pool.
There is no playable bench interaction yet; `reset_blue_health` provides the
source reset operation for that future integration.

The HUD uses the actual first Blue Idle pose from the source health collection,
placed after the normal masks. It is not a blue tint of a white-mask image.
Two blue HUD packets cover the one admitted two-bug cocoon. Expanding cocoon or
charm coverage must expand this explicit display bound too.

## Art and memory

The startup-only bank is `data/lifeblood.hk`, with generated descriptors in
`data/lifeblood.rs`. It contains fourteen source frames: intact cocoon,
Spawn 4/Land 3/Run 4, the cocoon splat's first pose, and the blue HUD pose. No retail
asset is checked into shared documentation.

The bank occupies **4,902 of 5,120 bytes** reserved at VRAM halfword rectangle
`(352,96,32,80)`, including its CLUT. Textures are divided into lossless strips
where needed; thirty stored parts preserve every admitted frame's pixels.
The full VRAM allocation test also includes framebuffers, animation, scene
pages/palettes, existing HUD, dialogue, Geo and independent break effects.
No gameplay-time texture upload or new SPU allocation is required.

Native layout measurement gives `World` 136 bytes and `Bug` 60 bytes, with only two
bug slots and no heap. Generated metadata and packet storage are additional;
the final guest build report remains authoritative for total RAM/stack usage.
World drawing has an explicit 16-quad limit. The intact cocoon uses five parts;
two visible bugs plus the splat use at most six parts in the admitted states.

## Approximations and remaining work

- Existing 60 Hz swept boxes replace Unity's 50 Hz Box2D contacts. The original
  floor ObjectBounce response, horizontal ray lookahead and collision ordering
  are not exact. Bugs outside the current collision apron retain state and
  pause their movement rather than falling through missing geometry.
- Source variation uses a separate deterministic PRNG, not Unity's RNG sequence.
- Art shares a 15-color source RGB palette with the established ordered
  transparent/half/full alpha approximation. Intact cocoon and bug frames use
  the existing camera projection. The cyan splat retains its exact 2×3 world
  scale, but samples its source image at 33×21 to fit the bounded bank.
- Cocoon sweat and blue-mask appearance/break animation are not implemented.
  The source cocoon splat is **not a permanent remnant**: Splat2 has seven
  frames at 24 fps, with its last frame fully transparent. The first-pose runtime
  approximation retires at the blank-frame onset of 15 ticks, including outside
  the collision apron, and is expired on a real scene exit. Intermediate
  animation frames remain unimplemented.
- Cocoon cap/burst debris, dedicated sounds, the Scuttler death splat/particles,
  charm interactions and original spatial audio are not yet implemented.

## Validation and route evidence

Run:

```sh
.venv/bin/python host/lifeblood.py
.venv/bin/python -m unittest discover -s tests -p test_lifeblood.py
cargo test --manifest-path shared/hk-sim/Cargo.toml --test lifeblood
cargo test --manifest-path shared/hk-sim/Cargo.toml vitals
cargo test --manifest-path shared/hk-cache/Cargo.toml residency
```

The targeted suite passed eight lifecycle/earned-health tests, eight Vitals
checks, ten residency checks and three host/native presentation checks. Tests
cover source hit lock and delayed grants, no touch reward, duplicate rejection,
scene unload versus view residency, landing/fleeing/bounce, deterministic fling,
blue-first damage and Focus, complete texture splitting and atlas ownership.
The native presentation harness compiles the actual runtime module with traced
GPU calls and verifies blue HUD placement, source art references and cocoon/bug
submission. It is not a substitute for an emulator screenshot or hardware test.

The source target lies in view 62, bounds `[48,59,72,75]`. Nearby source ledges
include x57–64/y45, x64–67/y50, x57–63/y52, x51–64/y57 and x64–69/y58.
The old whole-ascent planner (`.hkpsx/burst-fix2-ascent/`) assumed cleared
breakables and omitted damage, so its actual-CUE route never reached the cocoon.
On September 14 a new continuation from the user's extended crash recording
reached `(57.15,58.39)` on the actual CUE. Its local planner rejects the extracted
hazard polygons; without that check the first jump struck the authored stalactite.
Planning only generates controller inputs; it does not alter guest RAM or the disc.

The 6,168-poll controller tape at
`.hkpsx/lifeblood-validation-plan/attempt5.pxtape` now demonstrates the complete
interaction on the September 11 baseline (`6517d70`):

- The jumping upward nail strike opens the cocoon and releases two bugs.
- Horizontal attacks after landing strike both bugs. Grants arrive 72 simulation
  ticks later, and two original blue masks appear after the ordinary HUD masks.
- Returning to the stalactite produces blue/ordinary health pairs
  `(2,4) -> (1,4) -> (0,4) -> (0,3)`. The first two hits consume only the
  temporary pool; the third damages ordinary health.
- The actual-CUE replay completes without guest/CD/input/geometry-repair faults
  and with unchanged input hashes. Software captures of the cocoon, bugs and
  each health state were inspected. Evidence: `.hkpsx/lifeblood-validation-5/`,
  including `health-sequence.png` and `lifeblood-coverage.json`.

Reproduce against the current build, then check the observed interaction:

```sh
python3 tools/replay_cue.py --tape .hkpsx/lifeblood-validation-plan/attempt5.pxtape --output .hkpsx/lifeblood-recheck --screenshot-interval 30
python3 tools/validate_lifeblood.py --replay .hkpsx/lifeblood-recheck
```

Use a fresh output directory and inspect captures after guest changes. The
coverage gate rejects unearned masks, incomplete damage coverage and death/reset
masquerading as absorption. It does not establish hardware timing or complete
source effect/audio parity.

The current September 14 EXE was rechecked with that same tape in
`.hkpsx/progression-scalar-lifeblood/`: all four coverage checks pass, with zero
faults and unchanged inputs. `health-sequence.png` visibly records two, one and
zero blue masks followed by ordinary damage. Final state is region 44, ordinary
health 3, blue health 0, Geo 30, no deaths. Its crash-prefix burst still records
14 rejected particle spawns; that is separate from the two successful health
grants and remains an effects limitation.
