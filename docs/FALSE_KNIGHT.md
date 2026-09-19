# False Knight source contract, and the standing subset that is admitted

`host/false_knight.py` extracts the whole fight from the installed Windows
source. A standing subset of it is admitted: the boss is merged into
Crossroads_10, cooked and drawn, and it idles and turns to face the Knight.
Nothing else about the fight is wired. The phase machine and the reusable arena
lifecycle exist and are tested (`shared/hk-sim/src/false_knight.rs`,
`shared/hk-sim/src/boss.rs`).

The animation cache used to be the reason no frame could reach VRAM, and it is
not any more: it is 24 slots since a static scenery page no scene had ever used
became its second region, and the boss's largest frame binds 12 of them. The
actor art path used to be the second reason, because it drew a frame as one
quad; it now tiles an actor frame the way NPC art already did. What decides how
much of the fight can be on screen is residency: tiling makes a frame drawable
and multiplies what it costs a view, so the clip set is admitted a clip at a
time. `Idle` and `Turn` fit and are in; the other 33 clips are 457 more CLUT
slots and are not. Read the measurement below before designing any boss art
work, because the same wall meets every large enemy.

Regenerate everything here with:

    .venv/bin/python host/false_knight.py

which writes `.hkpsx/false-knight/source-contract.json` and prints the summary.

## Where the boss actually lives

Not in Crossroads_10. `BossLoader`, a `SceneAdditiveLoadConditional` in
`level46`, loads `Crossroads_10_boss` (`level48`) while `falseKnightDefeated` is
false and `Crossroads_10_boss_defeated` (`level49`) once it is true. Neither
additive scene appears in `host/quality.py`'s `SCENE_TABLE`, and until now no
part of the port cooked, packed or loaded them.

It is not a `SCENE_TABLE` row, and the measurement says so rather than the
prose. `tools/cook_scene_pack.py Crossroads_10_boss` returns `no_envelope`: the
tool derives a candidate's camera and activation envelope from its tilemaps,
gates and camera locks, and `level48` has no `tk2dTileMap` at all. Its 339
objects are 56 GameObjects, 12 SpriteRenderers of which 4 are active, 9
`tk2dSprite`/`tk2dSpriteAnimator` actors, 5 HealthManagers, 2 CameraLockAreas, 2
PersonalObjectPools and a MusicRegion, spanning x 14.00 to 50.93 and y 2.10 to
46.64, entirely inside Crossroads_10's own x -7.0 to 80.2. (An earlier revision
of this page gave that span as x 25.95 to 35.88 and y 2.10 to 24.33, which was
the SpriteRenderers alone and left out both the boss and the three Zombies.) The
scene has no envelope because it is not a room: it is content merged into
Crossroads_10's existing views under the `falseKnightDefeated` gate.

`host/scene.py` does that merge, and it is a recognizer rather than a table.
`Scene.__init__` walks its room's `SceneAdditiveLoadConditional` components,
answers `needsPlayerDataBool` once from `SetupNewPlayerData` the way
activation.py answers the load-time gates, resolves the chosen scene name
against BuildSettings, and reads that file into the same scene. A loader that
uses the int, extra-test or PersistentBoolItem branches is refused rather than
guessed, because it would pick a different scene than this answers.
Crossroads_10 is the only one of the 46 admitted scenes that carries such a
loader, so the blast radius is one room.

Both files number their serialized objects from 1 and their ids collide
completely, so the additive file's ids are shifted by `ADDITIVE_ID_BASE`
(100,000) and every PPtr inside its trees is moved with them: an `m_FileID` of 0
takes the same shift, and anything else is remapped into a merged externals list
because the two files list their externals in different orders. `Scene.sid`
still reports an object under the file it was really serialized in, so a record
says `level48:325` rather than something no source tool can open, while
`ActorSpec.source_id` carries the shifted id because it has to be unique inside
the scene.

### The arena floor was missing

The merge is not only how the boss gets in. Crossroads_10's own terrain has no
floor under the arena: at the boss's x column the horizontal layer-8 edges are
y 44 (the ceiling slab) and then y 2 and 0. Every arena floor edge belongs to
`level48`: `Break Floor` at y 26.05 spanning x 9.00 to 64.47, `Break Floor` at
y 22.67 spanning x 14.27 to 65.04, and three `Floor` boxes. Cooking view 240
with and without the merge moves its terrain from 34 edges to 50, all twelve new
horizontal edges coming from `level48`. Before this change a player who reached
the arena fell roughly twenty-four units to the bottom of the room.

The rest of the merge is cheap. That same view goes from 448 draws to 450, 303
textures to 305 and 202,200 pack bytes to 202,680: four active SpriteRenderers,
of which two land in this view.

## The measurement

Reproduced by `false_knight.measure_animation`, which runs `host/cook.py`'s own
actor art path (`tk_sprite`, the placement's 1.3 transform scale, the
`FOCAL / -CAM_Z` projection of 14.8178 px per world unit, one streamed texture
per frame) rather than estimating it.

| Quantity | Measured | Budget | Fits |
| --- | ---: | ---: | --- |
| Unique animation sprites | 110 | | |
| Sprites larger than one 64x64 slot | 77 | | |
| Widest frame / tallest frame | 204 px / 188 px | 252 texture axis | yes |
| 64x64 slots for the largest single frame (198x169) | 12 | 20 per frame, 24 total | yes |
| 64x64 tiles, all frames | 499 | | |
| 4bpp texels, all frames | 716,110 B (699.3 KiB) | 51,348 B unallocated RAM | no |
| CLUT slots, one palette per tile | 499 | 141 free in Crossroads_10 | no |

The slot row used to read 4 per frame and 8 total, and it was the row this
whole document was written around. It changed because the cache took a static
scenery page; `docs/BUDGET.md` has the census that made that affordable. The
RAM row is read out of `.hkpsx/build.json` and has fallen again, from 73,876
bytes to 63,636 and now 51,348. It only bites the Hollow Shade route, which
moves frames into linked RAM; the room-pack route the standing subset takes
spends none of it.

The CLUT row is the one the tiling change moved, and it moved the wrong way.
`tools/texture_headroom.py` reports Crossroads_10 at 275 of 416 slots in its
tightest of 20 views, leaving 141. One palette per *frame* would have been 110
and would have fit. A tile is a texture, HKROOM02 sizes its palette block at one
32-byte entry per texture, and the region uploads that block whole, so the
110 frames cost 499 slots rather than 110. Whole-fight residency in a
Crossroads_10 view is over by 3.5x on CLUTs, and 732,078 room bytes against a
393,216-byte room budget is over by 1.9x. Nothing about that is a reason not to
tile: the alternative was a frame that could not be drawn at all.

Per clip, in tiles, from `measure_animation`'s `clips` rows: `Blank` 1, `Turn`
12, `Jump Antic` 18, `Idle` 30, `Stun Recover` 36, `Rage` 36, `Run` 42. A
standing boss is affordable in Crossroads_10 today: `Idle` and `Turn` together
are 42 tiles and 73,556 bytes. The phase-one clip set is not.

## What the standing admission actually costs

Cooked rather than estimated. `Idle` and `Turn` through `Atlas.add_tiled` come
to 42 streamed textures, 73,564 stream bytes, 7 frames and 2 clips, no resident
page, largest frame 185x119.

Appending that bank to Crossroads_10's view packs and re-running
`texture_dedup.deduplicate_room` adds exactly 42 textures to each, with no
interaction with what is already there:

| View | Textures now | With the boss | Pack bytes |
| ---: | ---: | ---: | ---: |
| 240 | 303 | 345 | 202,200 -> 277,952 |
| 241 | 297 | 339 | 202,012 -> 277,764 |
| 237 | 291 | 333 | 202,192 -> 277,944 |

Two cautions on those numbers. The tightest view is 240 at 303, not the 241 that
`tools/texture_headroom.py` reports, because that tool reads `textures` out of
`data/regions.json` and the packs on disc carry more than that field records
(view 241 is 275 in the metadata and 297 in its own header). The CLUT cost is
the pack's texture count, so 345 of 416 is the number that matters, leaving 71.
Room bytes stay well inside the 393,216 budget.

None of that is linked RAM. The bank rides in the scene pack, which is read from
disc into the scene arena, and `pack_scenes.scene_arena_bytes` sizes that arena
from the largest scene: Tutorial_01 at 352,412 resident bytes, giving 421,128.
Crossroads_10 is 150,396, so roughly 76 KB more leaves it around 226 KB, still
126 KB below the scene that sets the arena. `SCENE_ARENA_BYTES` does not move and
the art costs nothing against
`memory.unallocated_before_stack_bytes`. What does cost linked bytes is one
172-byte `ActorSpec` row in `data/regions.rs` and the controller's code;
`EnemyWorld` measured 5,856 bytes both with and without the new `Runtime`
variant, because `FalseKnightRuntime` is 12 bytes against a Runner variant that
already sets the enum's size, so there is no bss growth.

### The three pre-battle Zombies stay out

The merge also brings `Zombie Barger`, `Zombie Runner` and `Zombie Hornhead`,
and `runner.recognize` admits all three. They are not admitted, for two reasons
that agree. `Battle Control` kills them with `KILL ALL ENEMIES` the moment the
boss lands, and nothing runs that arena yet, so they would stand in a fight that
does not exist. And they cost 99 textures and 45,564 stream bytes: with the boss
that is 141, which takes view 240 to 444 of 416. `actor_sources` therefore
admits content merged in from an additive scene one controller at a time: a
recognizer has to set `admit_from_additive_scene`, and only the False Knight
does.

The animation cache was the binding constraint and is not any more.
`hk_cache::SLOTS` was 8 slots of 64x64 4bpp in a 32x256 halfword VRAM strip at
x 320, capped at `MAX_UPLOAD_BYTES` 16,384: the False Knight's largest frame
alone is 16,900 bytes across 12 tiles, so one frame was bigger than the whole
cache. `SLOTS` is now 24 and `MAX_FRAME_TILES` is 20, so 12 tiles beside the
four keys `main.rs` always reserves is 16 of 24, 32,768 bytes of 49,152.

`host/cook.py` no longer refuses this actor. The actor path calls
`Atlas.add_tiled`, the same admission NPC art uses, and
`enemies.rs::prepare_draws` expands a frame into one `Draw` per tile with
`Room::frame_tile`'s share of the world box, so `append_needed` declares one
animation key per tile and `Draws::draw` submits one quad per tile. Tiles are
culled individually, so a boss leaning out of the view spends neither a quad nor
a slot on the part off screen. What the actor path still refuses is a frame past
the 252-pixel texture axis, because the clamp behind it would resample the art
and actor sampling is never reduced. `MAX_VISIBLE` in `enemies.rs` counts tiles
rather than actors and is 20, the slots the cache holds beyond the four
`main.rs` reserves; `main.rs` sizes its request array from
`hk_cache::MAX_REQUESTS` and a `const` assertion holds the two together.

Per-clip figures, since they are what a streaming route would have to hold:
the heaviest clip is `Stun Recover` at 6 unique frames and 56,406 bytes; `Rage`,
`Idle`, `Jump Attack Up`, `Run` and `Attack Recover` are 52,844 to 56,050. The
clips reachable in phase one without a stagger come to 44 unique frames and
454,616 bytes. Splitting a frame into 64x64 tiles costs no extra bytes: 64 is a
multiple of the four pixels a padded 4bpp row rounds to, so the tiles of a
198x169 frame come to the same 16,900 bytes the whole frame does.

## Why the Hollow Shade route does not solve it

`host/ability_art.py` and `game/src/ability_art.rs` move frames into linked RAM
and stream them through the same shared slots. That removes the room pack and
CLUT cost, and `host/shade.py` shows the palette trick: one resident 16-colour
CLUT for a whole clip set, which would take the False Knight from 110 CLUTs to a
handful. It does not change the slot geometry. `game/src/animation_cache.rs`
still rejects any payload over 2,048 bytes per slot, so a 198x169 frame needs
the same twelve slots from linked RAM as from a room pack. Twelve now fit, but
the RAM does not stretch: `.hkpsx/build.json` reports 51,348 unallocated bytes
before the reserved stack, against 699 KiB of texels, short by a factor of
fourteen. Read that figure from the build report every time; it was 113,636,
then 73,876, then 63,636, and it is 51,348 now.

## The route past it

Three changes, in this order. None of them is shaving frames: the project's
standing constraint is that actor animation sampling is not reduced to fit a
scenery-grade cap, and a 3.2x linear reduction of the largest frame is exactly
that.

1. **Multi-slot frames, and the slots to hold them.** Done. A frame binds a
   rectangle of 64x64 slots and the draw submits the tiles. `Atlas.add_tiled`
   splits an oversized frame into consecutive streamed textures, the first
   carries the grid in the two fields a streamed texture never uses for a VRAM
   origin, and `hk_format::Room::frame_grid`/`frame_tile` read it back.

   The strip itself still cannot grow, and that was never the way. It is half
   of one texture page, boxed in on both sides, and `residency::SPARE_HALFWORDS`
   is the measurement: 304 of 524,288 halfwords unclaimed, 608 bytes, in two
   fragments 16 halfwords wide at rows 480 to 495; the widest free run on any
   row is 16 halfwords and the tallest free run in any column is 14 rows,
   against a slot's 16 by 64. A ninth slot had to come from a whole
   reservation.

   It came from the static scenery pages, and the census is why that was
   affordable rather than a trade. The 20 pages are a whole-scene reservation,
   not a per-view one, and `.hkpsx/packed-scenes.json` has all 45 cooked scenes:
   the largest, Tutorial_01, is 18 pages; the next, Crossroads_ShamanTemple, is
   9; Crossroads_10 itself is 8; 44 of the 45 are 9 or fewer. Pages 18 and 19
   were reserved and no scene ever uploaded either. Page 19 is now the animation
   cache's second region at (960,256), and a 4bpp texture page is exactly four
   64x64 slots across and four down, so `SLOTS` is 24, `MAX_UPLOAD_BYTES` is
   49,152 and `MAX_FRAME_TILES` is 20. No cooked scene lost anything and
   Tutorial_01 keeps page 18 as headroom. `docs/BUDGET.md` carries the cost
   that was not free: the 24,576-byte VRAM write a 12-tile pose change costs has
   not been timed.

   The False Knight's 198x169 frame binds 12, and 12 beside the four keys
   `main.rs` always reserves (the Knight's body or ability pose, the nail
   effect, the Hollow Shade and the Vengeful Spirit ball) is 16 of the 24.

   The actor path takes that route now. `host/cook.py`'s `append_actor_art`
   calls `Atlas.add_tiled` and `enemies.rs::prepare_draws` emits one `Draw` per
   tile, so a tiled actor shows all of itself rather than its top-left corner.
   Proved end to end on the real art rather than a fixture: the boss's largest
   frame, `Death Fall` at 198x169, cooks to a 4x3 grid of 12 streamed textures,
   `hk_format::Room::parse` accepts the pack, every tile is a streamed texture
   inside one 64x64 slot and inside `animation_cache`'s 2,048-byte per-slot cap,
   the tiles partition the frame's world box with no gap or overlap, and the
   twelve screen quads meet on every seam and cover 198x168 pixels of the
   320x240 view. Total stream payload 16,900 bytes; a full miss on all twelve
   slots is a 24,576-byte VRAM write, which is still not timed.
2. **One palette for the boss.** Quantize all 110 frames against a shared
   16-colour palette the way the Shade does, so the whole fight costs a few CLUT
   slots instead of 499 and can be resident in every view of the arena.

   This moved from a nice-to-have to the binding constraint when the frames
   started tiling: the cost is one CLUT per tile, and the 110 frames are 499
   tiles against 141 free slots in Crossroads_10's tightest view. Nothing else
   on this list closes a 3.5x gap.

   A shared palette cannot reduce the CLUT count inside HKROOM02 as it stands.
   The pack's palette block is exactly one 32-byte entry per texture, the region
   uploads that block whole, and `tools/texture_headroom.py` counts a view's
   CLUT slots as its texture count. Two textures pointing at one palette index
   still cost two slots. `Atlas.add_tiled` quantizes a whole frame once anyway,
   because that is what keeps the colours identical across a tile seam, but the
   saving needs a separate palette count in the format, which is its own work.
   Measured: the Stag's Idle cooks to 200 textures, four tiles on each of 50
   frames, and the region carrying it reaches 358 of 416 CLUT slots.
3. **Per-clip residency, with no prefetch room.** 699 KiB never fits main RAM,
   and against the live gap the prefetch does not either. The clip bytes were
   measured against 73,876 unallocated bytes, and the gap has since fallen to
   51,348, so the ratios below are recomputed rather than re-measured: the
   heaviest clip, `Stun Recover`, is 56,406 bytes, which is 109.8% of the gap
   and no longer fits on its own at all; the heaviest adjacent pair is 112,456
   bytes, 219.0% of it. At least nine of the 33
   clips pair with the next heaviest to more than the gap, and that count can
   only have risen; re-run `host/false_knight.py` before quoting it. So holding
   the current clip and prefetching
   the next, which is what made the plan's timing work, does not fit: the
   controller's beat of warning (`Jump Antic` 0.3 s, `S Attack Antic` 1.2 s)
   would have to cover a blocking load of up to 56,406 compressed bytes rather
   than an overlapped one. Whether a load that size lands inside 0.3 s is not
   measured here; the byte figures are.

Only after those does `MAX_SCENE_ACTORS` matter. The arena's worst concurrent
set is the boss, its Hitter overlay and its Head, the three pre-battle enemies
and the barrel pool, whose `PersonalObjectPool` reserves 8: fourteen against a
cap of 32 that Crossroads_22 already contends for at 36. Raising that cap is not
part of this work and the boss does not need it.

## The fight

Source: `FalseyControl` and `Check Health` on `level48:40` (`False Knight New`),
`Health Check` on `level48:48` (`Head`), `Battle Control` on `level48:10`
(`Battle Scene`), `summon` on `level48:8` (`FK Barrel Summon`) and five
`BG Control` battle gates in `level46`. Each is pinned by a structural digest in
`FSM_SHA256`; `false_knight.fsm_digest` hashes states, transitions, enabled
actions and their serialized parameter bytes while ignoring the per-scene object
references, because `runner.fsm_fingerprint` cannot decode the action kinds
`FalseyControl` uses.

### Health and staggers

The body carries 65 hp with 0.25 s invulnerability. `Check Health` watches for
`ZERO HP` from its own HealthManager, sends `STUN` and restores the body to 65,
so the body never dies: reaching zero staggers it. The armour rolls, lands,
pauses and opens, exposing a `Head` with 40 hp and 0.15 s invulnerability. Its
`Health Check` restores it to 40 and sends `STUN END`, which is the only thing
that advances a phase. Three of those kill the boss: 195 body damage and 120
head damage, plus whatever a failed stagger costs.

`Opened` closes itself again after 5 s (`STUN FAIL`), and that timer restarts on
every head hit because `Hit` re-enters `Opened`. A timed-out stagger costs the
player the window and the boss nothing: `Stun Fail` does not touch
`Stunned Amount`.

Neither HealthManager drops Geo. `smallGeoDrops`, `mediumGeoDrops` and
`largeGeoDrops` are all zero on both, and the `Geo Pool` child `FalseyControl`
finds at startup is never used by any state. The reward is progression:
`falseKnightDefeated`, `killedFalseKnight`, `newDataFalseKnight`,
`killsFalseKnight` 0, `openedMapperShop`, `corn_crossroadsLeft`, the journal
entry and the achievement.

### Phases

`Stunned Amount` selects the table. `To Phase 2` and `To Phase 3` rewrite it
during the rage that follows each stagger.

| Stunned Amount | Idle wait | Jump-attack barrels | Slam barrels |
| ---: | --- | --- | --- |
| 0 | 1.0 s flat | none | none |
| 1 | 0.8 to 1.0 s | 2 to 3 | 2 to 3 |
| 2 | 0.8 to 1.0 s | 2 to 2 | 3 to 4 |

Two further phase behaviours are not in that table. `Determine Jump` returns
outright when `Stunned Amount` is exactly 2, so the third phase never plain
jumps. `Barrels?` skips the summon unless `Stunned Amount` is at least 2, so the
jump attack only ever summons in the third phase even though the table gives it
counts from the second.

### Attack selection

`Move Choice` clears the turn counter, then tests distance every frame: beyond
21 units it chases (`Run Antic`, then `Run` at 14 units/s until the gap is under
14, handing straight to the jump attack without spending its budget). Within 21
it picks one of `SMASH`, `JUMP ATTACK` and `JUMP` at equal weight. A branch
whose counter is spent sends `RETURN` and the state re-rolls.

- **Slam.** At most three in a row. Leaps 12 to 18 units past the hero and slams
  back, unless the hero is already 12 or more units away, in which case it skips
  the leap and slams where it stands. The slam emits a ground wave at 22 units/s
  from 5.5 units ahead and summons the phase's barrels.
- **Jump attack.** At most four in a row. Aims 3 units past the hero, scaled by
  0.58 and clamped to 12, then drops on it; the slam commits as soon as terrain
  is within 9.5 units below. Recoils at 3 units/s, then half that.
- **Plain jump.** One per visit to `Move Choice`, and none at all in phase 3.
  Half the time it aims at the hero's x clamped to [15, 42], scaled by 0.9 and
  clamped to 12; half the time it jumps randomly. The random branch casts two
  8-unit terrain rays: a wall on one side forces a jump the other way at exactly
  5 units, because the clamp that follows the sample discards the magnitude.
  With no wall the magnitude is 5 to 10 either way.

Facing the hero costs a `Turn` (0.167 s) and the fourth turn without an attack
forces `Move Choice` instead.

### Rage

Every stagger the player converts is followed by a fixed sequence: a 0.5 s
pause, a jump to x 28.9, then eight slams from a stationary `Rage` loop with
0.249 s of wind-up each and a turn between them. The eighth slam of the second
conversion cracks
the arena floor; the boss breaks through it on the third.

### Death

After the third conversion the boss jumps to x 34, invulnerable, and slams
through the cracked floor. It lands, opens a final time with no timeout, and the
last head kill starts a fixed 450-tick tail: `Death Anim Start` 1 s, `Steam` 3 s,
`Ready` 1 s, the `Death Head 1` clip 1 s, `Death Head Land` 1.5 s. Only then does
`Decrement Battle Enemies` run and the HealthManager death event fire, which is
what the arena has been waiting on.

## The arena, which is the reusable part

`shared/hk-sim/src/boss.rs` is `Battle Control` and nothing boss-specific. Every
later boss room reuses it.

`Pause` to `Init` branches on the `Activated` PersistentBoolItem. Already
activated: quick-open the gates with no animation, activate the floor, and the
control destroys itself, so a cleared arena never fights again across any number
of reloads. Not activated: wait on the trigger box.

The hero crossing the trigger sets `Battle Enemies` to 1, locks the camera,
broadcasts `BATTLE START` and `BG CLOSE` to every gate in the room and the boss
scene. All five `BG Control` gates in Crossroads_10 answer, not only the two at
the arena's ends; two of them (`Battle Gate 2`, `Battle Gate 2 (1)`) carry
`Start Closed` set, which is their only difference from the other three and the
reason the digest excludes that variable.

The counter is not automatic. `HealthManager.Die` only decrements
`Battle Enemies` when something has called `SetBattleScene` on it, and nothing
in this arena does, so the three pre-battle zombies are killed by
`KILL ALL ENEMIES` without touching the count. `Battle Enemies` moves only when
the boss's own `Decrement Battle Enemies` state runs. `Start` also has no
every-frame comparison; only `Kill Zombies` does, so the arena cannot end before
the boss has landed and cleared the room, whatever the counter says.

At zero the arena writes `Activated` and waits two seconds before `BG OPEN`. The
write happens before the wait, so a save taken during it already counts as
cleared.

## The standing subset, which is what is wired

`shared/hk-sim/src/false_knight.rs`'s `Sentry` is `FalseyControl`'s `Idle` and
`Turn R`/`Turn L` and nothing else, and it is the only part of the fight the
guest runs. It holds a facing and a countdown. `Idle` compares the hero's x
against its own every frame, which is the whole sense; entering a turn writes
the facing immediately, the way `Turn R` writes `Facing Right` true and sets the
transform scale to +1.3 before its clip plays, and holds `Turn` for its own ten
ticks. `host/false_knight.py::recognize_placement` admits the placement,
re-checking the `FalseyControl` and `Check Health` digests so a source change
fails at admission, and `game/src/enemies.rs`'s `FalseKnightRuntime` runs it.

Three things about it are deliberate and none of them is source behaviour.

**It falls in on its own.** The source hangs the boss in `Dormant` at the
authored transform, x 26.07 y 46.64, which is inside the ceiling slab above the
arena (layer-8 terrain from y 44 to 47), and drops it on `BATTLE START`. Nothing
sends `BATTLE START`: `boss.rs`'s `Arena` is written and tested but not wired to
a trigger box, gates or the camera lock. So the runtime separates the body out
of the slab with the same `resolve_actor_spawn` every actor uses, lets it fall
under the shared solver, and latches `settled` on the first ground contact. It
lands on `Break Floor` at y 26.05, putting its transform at y 31.64, against an
authored `CameraLockArea B` that pins the camera at y 31.37. The position is
produced by the port's own terrain solver from the authored transform; it is not
written down anywhere.

**It costs one comparison a tick once it is down.** Crossroads_10 carried no
actors at all before this, and the frame is stall-bound, so the settle latch is
load-bearing: after it lands the solver is never run again, because the source
body is a kinematic transform between attacks and the subset has no attacks.

**It is admitted invincible.** The source body is not, but `Check Health`
restores it to 65 every time it reaches zero rather than letting it die, and the
stagger that consumes that is not implemented. A killable body would vanish on
its fifth nail hit, which is further from the source than refusing the damage.

## What is not implemented

- Everything past standing. The attacks, the staggers, the rage, the death
  sequence, the Hitter overlay, the Head, the barrels and the floor break are
  all in `false_knight.rs` and none of it is bound to the guest controller.
- The arena. `boss.rs` holds `Battle Control` and is tested, but no trigger box,
  gate, camera lock or persistent bool is wired to it, so `BATTLE START` is never
  sent and the pre-battle Zombies are never cleared.
- `host/cook.py::append_actor_art` needs a `FalseKnight` arm in its clip-binding
  dispatch, and `host/effects.py::append_corpse_art` needs to skip a controller
  that declares no corpse. The recognizer already carries `art_bindings`
  (`walk` -> Idle, `turn` -> Turn) so both can read the record rather than grow
  another controller branch.
- Corpse and Aspid shot art still cannot tile. `host/effects.py`'s `_ClipCooker`
  calls `Atlas.add(streamed=True)`, which refuses a frame past one slot, and the
  shot loop in `prepare_draws` draws one quad without consulting the grid. An
  admitted actor whose corpse clip has an oversized frame therefore fails the
  cook loudly rather than rendering wrongly, which is the safe order, but it is
  the next gap in the same change.
- The `Esc` branch is unreachable in this arena; nothing sets `Hero Escaped`
  outside Godhome, and the `Check Facing` global transition target does not
  exist as a state.
- `Rise` and `Fall` shape vertical speed per 50 Hz FixedUpdate in the source and
  per 60 Hz tick in the sim, so airtime differs slightly.
- Particles, camera shake, audio, the barrel prefabs, the shockwave object, the
  staff, the death head and the Godhome variants are recorded, not presented.
