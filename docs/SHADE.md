# The Hollow Shade, Geo loss and the soul limiter

`shared/hk-sim/src/shade.rs` ports the PlayMaker `Shade Control` FSM of the
`Hollow Shade` prefab; `game/src/shade.rs` runs it, owns the persistent record
and draws it; `host/shade.py` audits the prefab and cooks its art. Source
records and CIL are in .hkpsx/shade/CONTRACT.md.

## Death

Hero Death Anim drains SOUL to zero, moves the whole wallet into `geoPool`,
records the Shade (scene, position, and `shadeHealth` = clamp(maxHealth / 2,
1, 99), whose own HP is `nailDamage` times that, so 10 here), saves the game
and starts the soul limiter. A second death overwrites the pool, so the
earlier Shade's Geo is forfeit, exactly as in the source. No object in the 45
admitted scenes carries the `Shade Marker` tag, so the Shade always appears
at the death position through the FSM's own fallback. Death now writes the
memory card even when no bench has been rested; that save points Continue at
the new-game spawn.

## The Shade

It spawns when the Knight enters the scene that recorded it. Idle until the
hero is inside its 7.27-unit alert circle with a clear line of sight, then
Startle and Fly: ChaseObject accelerates 0.2 per fixed step per axis and
ChaseObjectV2 adds 0.16 along the direction, both clamped to 4 units/s. After
1 to 2 s it takes the attack chain, which at this port's spell levels falls
straight through to Position: DistanceFly holds 3 units out while always
levelling with the hero's y (`targetsHeight`), and a hero within 5 units and
0.2 of the Shade's own height triggers Slash Antic, a half-second wind-up,
then an 8 units/s lunge whose damage box is live for the Slash state alone.
Drifting more than 25 units (`Max Roam`) from the spawn retreats and resets it
to Idle. Contact and the Slash each deal 1. Killing it returns the pool to the
wallet, clears the record and ends the soul limiter.

While a Shade is owed, `StartSoulLimiter` caps the vessel at 66 rather than
99, and the HUD bar with it.

## Presentation

The Shade can appear in any scene, so its art is in no room atlas. Frames sit
in linked RAM and reach VRAM through the shared 64x64 animation slots under
keys above `SCENE_TEXTURE_CAPACITY`; only two 16-colour CLUTs are resident, in
the free rows above the dialogue palette. Not presented: every particle
system, the death orbs and Soul Orb HUD event, all Shade audio, the light
effect, the `nail_clash_tink` parry on the Slash child, and the map compass,
journal and story records. The Slash polygon becomes its axis-aligned bounds
and the retreat iTween path a straight interpolation. The Fireball, Quake,
Scream, Friendly, Lake and Jar branches are unreachable at this port's levels;
`host/shade.py` fails the build if any of them stops being gated on a zero
level.

## When the card is written

Never on its own. The source autosaves on a bench rest and again on death;
this port asks instead. Sitting heals and sets the respawn marker as the source
does, then a "Save game?" prompt takes Yes or No, and only Yes writes. Death
records the Shade, the Geo pool and the soul limiter in memory alone, so they
reach the card at the next bench the player chooses to save at. That is a
deliberate departure from the source, at the project owner's request, not an
unimplemented autosave.

## Saving safely

`Card::write` frees a file's directory entry before writing its data and only
restores it at the end, so a save that used one file spent roughly 190 ms in a
state where the old copy was already unfindable and the new one not yet
readable. Measured by stopping the emulator at cycle points across the write:
every cut in that window left a card the game could not load at all, with the
previous save destroyed. `game/src/save.rs` now alternates between two files,
each carrying a sequence number, and takes the valid record with the higher
sequence at boot. An interrupted write therefore never touches the copy being
relied on. `tools/validate_power_cut.py`, which the build runs after the
routes, cuts power early, mid, late and just past the transfer and requires a
loadable save every time.

## Routes

`kings-death` kills the Knight for real: it walks the King's Pass traversal
with the nail silent past the fifth breakable, so the Crawler there survives
and the sixth obstacle stays unbroken, leaving the Knight standing beside both
until it dies. It proves death records a Shade at the death position with
nailDamage times clamp(maxHealth / 2, 1, 99) health and writes the card with no
bench rested. Its wallet is zero, so the Geo transfer is the other route's job.

`kings-return` closes the loop on real data: it boots from the card
`kings-death` itself wrote, so the Shade it meets was recorded by an actual
death rather than seeded by hand, and walking the same traversal back must find
it waiting and recover it without dying again. That is the quit, reload and
recover path end to end.

`town-shade` boots from `tools/cards/town-shade.mcd`, a card whose save record
already carries a Shade six units from the Dirtmouth bench with a 99-Geo pool,
so the whole loop is exercised without a recorded death: it spawns, chases,
takes three masks off the Knight, dies to the nail, and returns the pool on top
of the saved six.
