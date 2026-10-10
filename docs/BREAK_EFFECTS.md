# Breakable destruction effects

The supported Tutorial breakables now spawn their original particle families when
the existing nail collision breaks them. This covers all 33 admitted C#
`Breakable` objects: seven doors, 22 statues/poles/shells, and four ordinary
health-plant shells. The current Town entrance has no admitted Breakables. The
main Lifeblood cocoon is a separate object and implementation.

Previously, only the doors' four rigid fragments were simulated; the other
objects switched to their broken scenery without their authored particles.
The new extraction resolves 71 particle emitters into ten parameter styles.
Doors retain their rigid fragments and gain their source stone particles and
break dust. The other props use their own stone or plant particle sprites.

## Source and runtime behavior

The cooker (`host/hk-cook/src/break_effects.rs`, ported from
`host/break_effects.py`) reads the Windows installation directly and writes
`data/break_effects.rs` and one compressed chunk per catalogue scene under
`.hkpsx/break-effects/`. Its ignored report at
`.hkpsx/break-effects/report.json` records source IDs, particle-system hashes,
Windows file hashes, allocation rectangles, and cooked payload hashes. It does
not change room geometry or scene atlases.

The runtime preserves authored emission rates, per-emitter maximum counts,
initial lifetimes/speeds/sizes/colors/rotation, shape transforms, random sprite
rows, forces/gravity, velocity limits, rotation curves, opacity and size curves,
and terrain-collision parameters. Breakable's directional angle offset follows
the nail direction. Long-lived fragments retain source lifetimes up to ten
seconds. Spatial region changes do not restart effects; scene reset clears them.

All effects share the 224-particle pool and its primitive allowance. The pool
holds the largest admitted source emitter, Crossroads_09's 210; one emitter
across the refused cracked floors authors 250 and stays refused on capacity.
Full pools count rejected spawns instead of overwriting existing effects. Widening
particle ages to support longer lifetimes fits existing struct padding: the pool
allocation remains 6,668 bytes. Input checkpoints run during emission and
collision processing.

Each tick integrates every particle first and unions their swept boxes, then
decodes the room's non-empty edges once into a 128-entry scratch, keeping only
edges that touch that union, and finally collides each particle against that
subset in edge order. Rooms with more edges than the scratch keep the direct
callback. The result equals the naive per-particle scan exactly; the first
guest build without this stage stalled the fixed-step loop and filled the poll
queue on a 105-particle burst.

## Art and allocation

The bank contains four original stone cells, four plant cells, and door dust.
The cells are sampled at the current camera projection, with three baked opacity
levels: 27 art records occupy 4,800 VRAM bytes including palettes. Upload happens
once at startup; breaking objects does not read the disc or upload textures.

VRAM reservations are expressed in halfwords:

| Owner | Rectangle `(x, y, width, height)` |
|---|---|
| Break effects | `(352, 176, 32, 64)` |
| Break effects | `(360, 32, 24, 32)` |

The second rectangle was removed from Geo's available allocation space. The
shared VRAM occupancy tests cover these reservations alongside scene, animation,
font, HUD, Geo and Lifeblood allocations.

## Fidelity limits

This is a bounded native recreation, not Unity particle-system parity. It uses
deterministic local RNG, 60 Hz integration/damping, 33-point sampled curves,
radius/edge contacts and a collision lifetime-loss approximation. The PS1 palette,
three opacity levels and Average blending approximate the original lighting and
transparency. Pool overflow can reduce simultaneous particles and is counted.

Limit Velocity over Lifetime (`ClampVelocityModule`) is cooked as Unity documents it: each
update, a particle faster than the magnitude loses `dampen` of the excess
(`speed - (speed - limit) * dampen`, at 60 Hz here). Audited over every cooked
system (the 640 break-effect records, which include the Knight's dust): 608 use the
limit, all with a constant magnitude, no separate axes and no drag, and all in local
space. That is the world limit for every unscaled system (magnitude 12 on 435, 0 on
123, 1 on 47 with the dust's dampen of 0.1), and the three grave poles at scale 0.81
have a limit of zero, so no scaled system has a limit to scale. The cook now refuses a
curve or a range of magnitudes, and a non-zero local-space limit under a scaled
emitter, instead of reading them at one
point; none of today's systems is refused.

Geo rocks still lack their separately authored chips, hit jitter and gleam.
Their existing intact/depleted art and payouts remain; they are not given a
substitute stone-particle burst. Breakable nail flashes, directional hit dust,
probabilistic `containingParticles` events, and unrelated scripted destruction
are also not claimed complete.

## Validation

Host extraction/allocation, Geo allocation, native particle and native world
integration tests pass in `make test`. The native differential in the particle
suite compares the optimized tick with a naive reference over a full 224-slot
pool for 650 ticks, on both the scratch path and the fallback path; the ignored
`.hkpsx/break-effects/check_optimized_source.py` repeats it with the generated
emitters and cooked terrain for all 33 owners (2,745,600 slot states, exact).

Actual-CUE replays in `.hkpsx/burst-fix2-ascent/` (eight breaks, 768 particles,
36 rejected by the full pool) and `.hkpsx/burst-fix2-geo-pause/` (six breaks,
730 particles, none rejected) complete with zero input faults or missed
VBlanks. Inspected captures show the bursts animating and settling. The long
profile's worst frame during a burst is seven VBlanks; see STATUS.md. Emulator
timing is not console timing.
