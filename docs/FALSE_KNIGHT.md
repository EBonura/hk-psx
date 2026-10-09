# False Knight source contract, and the fight that is admitted

`host/false_knight.py` extracts the whole fight from the installed Windows
source, and the fight runs. The boss is merged into Crossroads_10, hangs dormant
above the arena until the hero crosses the `Battle Scene` trigger, drops in,
chases, jumps, slams, staggers through three phases and dies.
`shared/hk-sim/src/false_knight.rs` is `FalseyControl`,
`shared/hk-sim/src/boss.rs` is `Battle Control`, and
`game/src/enemies.rs`'s `FalseKnightRuntime` binds both to the guest body.

The art is the whole set now, and so is the ending. Every clip `FalseyControl`
plays on the body and the `Hitter`, the `Head` the armour exposes, the `Death
Head` that drops out of it, the empty armour it leaves and `Floor Control`'s
cracked and broken floor are cooked from the installed source by
`host/false_knight_art.py`, at the size the actor path already projected them
to. The last jump breaks the floor, the boss falls into the room below, the
Head is finished there, and the fight's own voices play. The section "The whole
clip set" below has the measurement and what it cost; the sections after it are
the history of why the six-clip subset existed and are kept as a record.

Regenerate everything here with:

    .venv/bin/python host/false_knight.py

which writes `.hkpsx/false-knight/source-contract.json` and prints the summary.

**The barrel art needs a cook before the guest links.** `ActorController::
FalseKnight` carries `barrel_clip` and `barrel_spawn_y` now, and the committed
`data/regions.rs` predates both, so `game` does not build until the cook
regenerates that row. `host/cook.py` and `host/actors.py` are both above the
divider in `host/cook_inputs.txt`, so the next build re-cooks all 693 regions
whatever else it does.

While reading that file: `host/false_knight.py` is **not** in it, above the
divider or below, and it should be. `host/actors.py` imports it, and what it
returns decides the boss's clip bindings, its limitations and now its barrel
art, so a change here changes cooked bytes while every region's reuse key says
it did not. That is the failure mode `cook_inputs.txt`'s own comment calls
silent in one direction. It is left as found rather than fixed inside this
change, because moving a line in that file re-cooks the world by itself.

## The whole clip set, the Death Head and the floor

`host/false_knight_art.py` is a postpass: `regions.py::postpack_actor_bank`
hands it the scene's actor atlas, the generic path cooks only the barrel and a
one-pixel `Blank` for every body slot (`neutral_actor`), and this module appends
every part of every sprite. It sits below the divider in `host/cook_inputs.txt`,
so changing it re-runs the postpass and reuses every cooked region.

**Where the bytes go.** Each sprite is cut into horizontal bands (and columns
where art is wider than a texture page can address), each trimmed to its
non-transparent texels by a small dynamic program that trades texel bytes
against a per-part cost. A sprite's bounding box is mostly air: the boss's 89
sprites' boxes come to 694,312 bytes of 4bpp texels, and all 93 sprites' parts,
the floor's three included, to 432,824. Parts go to
one of two homes, decided clip by clip in the order the fight shows them:

- **The scene's own texture pages**, as ordinary static textures. A scene's
  pages are uploaded at the gate and never kept in RAM, and Crossroads_10's
  scenery used 11 of the 19 pages a scene may own. The plan packs parts beside
  the scenery with the scene bank's own MaxRects packer and keeps a page of
  margin (planned at 19 the real bank came to 20).
- **The animation slots**, streamed from the scene arena, in cells of at most
  64x64, exactly as the six clips were.

Measured on the cook (`.hkpsx/false-knight/art.json`):

| Quantity | Value |
| --- | ---: |
| Sprites (every clip below, deduplicated) | 93 |
| Static parts, texel bytes | 163, 256,716 |
| Streamed parts, texel bytes | 271, 176,108 |
| Crossroads_10 texture pages | 11 -> 19 of 19 |
| Crossroads_10 scene textures / palettes | 322 -> 666 / 241 -> 319 |
| Crossroads_10 resident scene bytes | 288,520 -> 356,096 |
| Scene arena (sized by the largest scene, now Crossroads_10) | 382,052 -> 425,740 |
| `SCENE_TEXTURE_CAPACITY` | 384 -> 672 |

Static clips: Idle, Turn, Jump Antic, Land, Jump, Attack Antic, Attack, Attack
Recover, Rage, Jump Attack Up, Jump Attack Hit 1 and 2, Stun Opened, Death Land,
Death Spaz, and the floor. Streamed: Jump Attack Hit 3, Run Antic, Run, the Head,
Body, Stun Open, Stun Hit, the whole roll, Stun Recover, Death Fall and both
Death Head clips. Several clips share sprites (Land is Jump Antic's, Rage is
Attack's, Death Spaz is Stun Hit's) and cost nothing of their own.

The guest reads the bank through one anchor clip whose first frame is part 0
in every view, and `data/false_knight_art.rs` says which parts make up which
frame of which clip (`game/src/fk_art.rs`). A draw is one quad per part; a part
in the pages needs no animation key, so only streamed parts count against the
20 slots a frame may bind (`enemies::MAX_DRAWS` is 48 quads).

**Behaviour that came with the art.** `hk_sim::false_knight` reports what the
`Head` child plays (`Head Idle`, `Head Hit` on every landed head hit, `Head
Spaz` at `Death Anim Start`), `Blow` (the body turns to the empty `Body`, the
Death Head drops out on the side the boss faces at `Death Head Speed` and slides
until a wall), and `Death Head Land` (`Death Head 2`).

**The floor.** `CRACK` on the last slam of the third rage swaps `Normal 1`/`2`
for `Cracked 1`/`2`; `DESTROY` on the death jump swaps them for `Broken` and
lifts `Break Floor`'s three colliders from every view
(`data/false_knight_floor.rs`, through the scripted-edge scratch the gates use,
now 20 slots). The boss falls to the bottom of Crossroads_10, where `Death
Land`, `Death Open` and `Opened 2` play out and the Knight follows it down to
finish the Head. An actor normally advances only within one view of the hero;
the boss is allowed to keep falling out of that range. `falseKnightDefeated`
(written at `Death Anim Start`, as the source does) and a won arena load the
floor broken, and a won arena also keeps `FK Armour`, which `Battle Control`'s
`Init` destroys on a fresh fight. Its `Tinger` nail bounce, which the cook
turns into a pogo target standing where the fight ends, goes with it; that one
was found on the disc, where it silently bounced every swing at the Head away.

**The voices.** Two, as before: `false_knight_land` and `false_knight_swing` in
the resident bank below Geo. The paused work carried eleven more (the slam's
`false_knight_strike_ground`, the stagger's armour crack, the roll, the floor
break, `boss_final_hit`, two head-hit cries, an attack cry, the heavy landing,
the jump and an armour creak, 74,768 bytes at 8 kHz) in two extra SPU segments:
the gap between ambience's widest set and the music ring, and the 16 KiB above
the Runner bank. Both are the world one-shot bank's on `fix/slice-gaps`, which
also moves Focus and Runner to end at 0x7FFF0, so the eleven are withdrawn
rather than landed on SPU another branch owns. Their place is a bank loaded
with Crossroads_10 at its gate, in SPU the area's ambience does not use there;
until then every state that would play one keeps its shake. Refused, with
sizes in `.hkpsx/audio-provenance.json` (`host/hk-cook/src/cook_audio.rs` `BOSS_REFUSED`):
all of the above, the ceiling break, the Run footsteps, the rage and death
roars (`FKnight_Rage` is 90,736 bytes at 22 kHz), `boss_gushing`,
`boss_explode`, the defeat sting and `breakable_wall_death`.

**What it cost, and where.**

| Budget | Before | After |
| --- | ---: | ---: |
| Crossroads_10 VRAM texture pages | 11 of 19 | 19 of 19 |
| Main RAM unallocated before the stack | 220,332 | see docs/BUDGET.md |
| Crossroads_10 scene payload on disc, stored | 261,593 | 466,127 |
| Gate load into Crossroads_10 (journey route, emulator, c0e0b41 loader) | 197 ticks | 312 ticks |
| Gate load into Crossroads_10 (journey route, pinned emulator, seamless loader) | 48 polls | 206 polls |

No other scene moved. The longer load is the one cost a player sees: the pages
the boss now lives in are read at the gate like every other page. On the
seamless loader the scene's group no longer fits the arena whole, so the gate
reads it chunk by chunk and no background prefetch covers it; the journey tapes
hold that window 158 polls longer than the seamless branch's did. It is the
first thing to win back (a prefix prefetch for groups past the arena, or fewer
static pages), and it has not been attempted here.

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
| 4bpp texels, all frames | 716,110 B (699.3 KiB) | 11,556 B unallocated RAM | no |
| 4bpp texels, all frames | 716,110 B | 393,216 B per room pack, of which the admitted six leave 38,248 free | no |
| Distinct palettes, all frames | 107 | 150 free in Crossroads_10's tightest view | yes |
| HKROOM02 texture records, all frames | 499 | 640 per view, of which the admitted six leave 245 free | no |

The slot row used to read 4 per frame and 8 total, and it was the row this
whole document was written around. It changed because the cache took a static
scenery page; `docs/BUDGET.md` has the census that made that affordable. The
RAM row is read out of `.hkpsx/build.json` and has fallen again, from 73,876
bytes to 63,636, then 51,348, then 41,064, then 32,160, and it is 11,556 now. It only bites the Hollow
Shade route, which moves frames into linked RAM; the room-pack route this art
takes spends none of it.

The CLUT row is no longer a refusal and the measurement is what changed, not the
art. `tools/texture_headroom.py` counts a view's distinct palette *words* rather
than its texture records, because the tiles of one frame are quantized together
and carry byte-identical palettes and the scene bank pools palettes by value.
Crossroads_10's tightest of its 20 views is 266 of 416 with 150 free, and the
whole fight's 499 tiles are 107 distinct palettes. The whole clip set fits the
palette budget. The separate limit on how many 16-byte records HKROOM02 can
address is 640, and that view spends 319 of them.

What refuses is bytes. Room packs are capped at 393,216 each, Crossroads_10's
heaviest view is 244, and the whole fight's 716,110 bytes of 4bpp texels never
come close to fitting beside it.

Per clip, in tiles and bytes, from `measure_animation`'s `clips` rows: `Blank` 1
tile / 2 B, `Turn` 12 / 18,190, `Jump Antic` 18 / 27,464, `Land` 18 / 27,464
(the same three sprites as `Jump Antic`, so the pair costs one of them),
`Attack` 24 / 37,248, `Idle` 30 / 55,366, `Rage` 36 / 56,050, `Stun Recover`
36 / 56,406, `Run` 42 / 53,240.

## What the admitted clip set costs, and what pins it

Six clips: `Idle`, `Turn`, `Jump Antic`, `Land`, `Stun Opened`, `Attack`. As a
bank that is 66 streamed textures and 146,340 texel bytes.

Measured against the real pipeline rather than estimated, which means cooking
each of Crossroads_10's 20 base packs, appending the scene-wide actor bank
(`host/regions.py::postpack_actor_bank`) to every one of them and re-running
`texture_dedup.deduplicate_room`:

| Clip set | Worst view | Bytes of 393,216 | Free | Records of 640 | Palettes of 416 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Idle, Turn | 244 | 279,584 | 113,632 | 347 | 272 |
| + Jump Antic, Land, Stun Opened | 244 | 316,492 | 76,724 | 371 | 276 |
| **+ Attack (admitted)** | 244 | **354,968** | **38,248** | 395 | 279 |
| + Jump | 244 | 383,036 | 10,180 | 413 | 282 |
| + Attack Recover instead of Jump | 244 | 399,104 | over | 422 | 284 |
| eight clips | 244 | n/a | n/a | n/a | refused: the 256 KiB animation and alpha metadata bank |

`Jump` fits and is left out on purpose: 28,068 bytes for the airborne pose, in a
room another agent is adding breakable and effect art to, against a 10,180-byte
margin that would fail the next cook loudly. `Jump Antic` stands in for it.

### The 75,752 bytes that were being paid twice

The binding view is 244, the one that contains the boss's authored transform,
and until this change it was carrying the actor art twice. `host/cook.py`'s
per-view cook appended the actor bank into the base pack, and then
`postpack_actor_bank` appended the scene-wide bank to every view of the scene
and replaced that row's clip indices with its own, so the base copy was paid for
and never read. Measured: view 244's base was 279,584 bytes and is 203,832
without it, and the worst-view figures above roughly double without the fix.
`cook()` still runs `append_actor_art` into an atlas it discards, because that
pass owns the refusal that clears `movement_supported` for a frame the animation
cache cannot hold and that verdict has to match the postpass.

This is a whole-game change, not a Crossroads_10 one: every scene with a
supported actor has one view paying for art it never reads. It has not been
validated by a full recook.

None of that is linked RAM. The bank rides in the scene pack, which is read from
disc into the scene arena, and `pack_scenes.scene_arena_bytes` sizes that arena
from the largest scene. Re-read out of `.hkpsx/packed-scenes.json` rather than
quoted: the arena is 417,748 bytes, Tutorial_01's resident scene is 349,032 and
the difference of 68,716 is the metadata tail every scene is checked against.
Crossroads_10's resident scene is now 301,304, the second largest, so it needs
301,304 + 68,716 = 370,020 and has 47,728 bytes of headroom before it would be
the scene that sets the arena. `SCENE_ARENA_BYTES` does not move and
the art costs nothing against
`memory.unallocated_before_stack_bytes`. What does cost linked bytes is the
`ActorSpec` row in `data/regions.rs` and the controller's code. `EnemyWorld`
measured 5,856 bytes with the standing `Sentry` and 6,112 with the whole fight,
so the fight costs 256 bytes of bss: `false_knight::FalseKnight` is 44 bytes and
`boss::Arena` is 12, and the `Runtime` enum only grows by what the new variant
exceeds the Runner variant by, times the 32 actor slots.

### The three pre-battle Zombies stay out

The merge also brings `Zombie Barger`, `Zombie Runner` and `Zombie Hornhead`,
and `runner.recognize` admits all three. They are not admitted, and now that the
arena runs the reason is only the budget: `Battle Control` kills them with
`KILL ALL ENEMIES` the moment the boss lands, so they would be alive for the
length of the entrance drop, and they cost 99 textures and 45,564 stream bytes
in every one of Crossroads_10's 20 views. Against the 38,248 bytes the admitted
clip set leaves free in view 244 they do not fit, and the thing to spend that on
first is more of the boss. `actor_sources` therefore admits content merged in
from an additive scene one controller at a time: a recognizer has to set
`admit_from_additive_scene`, and only the False Knight does.

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
the RAM does not stretch: `.hkpsx/build.json` reports 11,556 unallocated bytes
before the reserved stack, against 699 KiB of texels, short by a factor of
sixty-two. Read that figure from the build report every time; it was 113,636,
then 73,876, then 63,636, then 51,348, then 41,064, then 32,160, and it is
11,556 now. The boss audio alone took 11,808 of it, so there is now less left
than that one change spent, and `game/src/audio.rs` `include_bytes!`s the sound
bank, which means bank bytes and this figure are the same bytes.

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
2. **One palette for the boss.** Done, and it turned out not to need doing.
   `Atlas.add_tiled` quantizes a whole frame once, because that is what keeps
   the colours identical across a tile seam, and the scene bank pools palettes
   by value before anything reaches VRAM. So the whole fight's 499 tiles are
   107 distinct palette words, `tools/texture_headroom.py` counts those rather
   than texture records, and Crossroads_10's tightest view has 150 free. The
   palette budget is not what refuses the clip set; room-pack bytes are, and no
   amount of palette sharing moves a texel.
3. **Per-clip residency, with no prefetch room.** 699 KiB never fits main RAM,
   and against the live gap the prefetch does not either. The clip bytes were
   measured against 73,876 unallocated bytes, and the gap has since fallen to
   51,348, then 41,064, then 32,160, and then 11,556, so the ratios below are
   recomputed
   rather than re-measured: the heaviest clip, `Stun Recover`, is 56,406 bytes,
   which is 175.4% of the gap and no longer fits on its own at all; the heaviest
   adjacent pair is 112,456 bytes, 349.7% of it. At least nine of the 33
   clips pair with the next heaviest to more than the gap, and that count can
   only have risen; re-run `host/false_knight.py` before quoting it. So holding
   the current clip and prefetching
   the next, which is what made the plan's timing work, does not fit: the
   controller's beat of warning (`Jump Antic` 0.3 s, `S Attack Antic` 1.2 s)
   would have to cover a blocking load of up to 56,406 compressed bytes rather
   than an overlapped one. Whether a load that size lands inside 0.3 s is not
   measured here; the byte figures are.
4. **Stop paying for the actor art twice.** Done, and it is the one that
   actually moved: see the 75,752-byte section above. It tripled the clip set
   without touching a texel, and the same duplication is in every scene with a
   supported actor.

Only after those does `MAX_SCENE_ACTORS` matter. The arena's worst concurrent
set is the boss, its Hitter overlay and its Head, the three pre-battle enemies
and the barrel pool, whose `PersonalObjectPool` reserves 8: fourteen against a
cap of 32 that Crossroads_22 already contends for at 36. Raising that cap is not
part of this work and the boss does not need it: the Hitter and the Head are
boxes on the boss's own runtime rather than actors of their own.

## The fight

Source: `FalseyControl` and `Check Health` on `level48:40` (`False Knight New`),
`Health Check` on `level48:48` (`Head`), `Battle Control` on `level48:10`
(`Battle Scene`), `summon` on `level48:8` (`FK Barrel Summon`) and five
`BG Control` battle gates in `level46`. Each is pinned by a structural digest in
`FSM_SHA256`; `false_knight.fsm_digest` hashes states, transitions, enabled
actions and their serialized parameter bytes while ignoring the per-scene object
references, because `runner.fsm_fingerprint` cannot decode the action kinds
`FalseyControl` uses.

### Cross-checked against the wiki

The installed game's FSMs are the authority here and every number below comes
from them. The community wiki (https://hollowknight.wiki/w/False_Knight, read
2026-09-24) was used as a check on the player-visible shape, and agrees:

| Wiki says | The source, as cooked |
| --- | --- |
| Armour 65 hp per phase, three phases | body HealthManager 65, `Check Health` restores it (SI) |
| Maggot 40 hp, four exposures (3 x 65 + 4 x 40 = 355) | Head HealthManager 40; three `STUN END`s, then `Opened 2` in the room below (SI) |
| Leap, not twice in a row, gone in phase 3 | `Jump`, one per `Move Choice`, `Determine Jump` returns at `Stunned Amount` 2 (SI) |
| Charge a short way, then a leaping bludgeon | `Run` beyond 21 units, handing to the jump attack (SI) |
| Slam with a shockwave across the arena; barrels from phase 2 | `S Attack Recover`: `Shockwave Wave` and `SLAM_BARRELS` (SI) |
| Leaping bludgeon drops barrels in phase 3 | `Barrels?` summons only at `Stunned Amount` 2 (SI) |
| Rage in the centre, repeated slams raining barrels | `Idle Pause`, jump to x 28.9, eight `Rage Slam`s (SI) |
| Floor collapses, the Maggot is finished below | `Floor Break`, `Death Land`, `Opened 2` (SI) |
| 200 Geo and the City Crest | not `FalseyControl`'s: see "What is not implemented" |
| Theme "False Knight" | `Music`'s `ApplyMusicCue` `Boss1`, cooked as CD-DA track 3 (SI) |

SI is source-inspected (the extracted FSM, `host/false_knight.py` and
`host/false_knight_art.py`); what each route replays is the ER evidence.

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
  the leap and slams where it stands. The slam sends a ground wave towards the
  hero from 5.5 units ahead (see "The slam wave") and summons the phase's
  barrels.
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

### The slam wave

`S Attack Recover` spawns the pooled `Shockwave Wave`
(`sharedassets48.assets:63`) 5.5 units ahead of the body and 5.8 below its
transform, which is a fifth of a unit inside the floor, and writes its `Speed`
to 22. The wave is an invisible trigger: `Start Move` multiplies that speed by
0.025 and sets an incrementer of twice it, and `Move` adds the incrementer
every second, so it creeps out at 0.55 units/s and accelerates at 44 units/s².
It stops when its trigger meets terrain or its 1.6-unit ground ray finds
nothing. Every frame it moves it spawns a `Shockwave Spurt` (`:68`) where it
is; the spurts are both what is drawn and what hurts. Each one's `Damage
timing` FSM arms its DamageHero (1 mask) after 0.05 s and disarms it 0.05 s
later, and each recycles when its six-frame 20 fps clip has played.

`host/false_knight_art.py` reads all of that out of the two prefabs and writes
it to `data/false_knight_art.rs` (`FK_WAVE_*`, `FK_SPURT_*`), refusing if an
FSM moves; the spurt's clip is cooked into the boss bank, static, 6 sprites and
1,132 texel bytes. `shared/hk-sim/src/shockwave.rs` is the wave, shared by the
guest (`EnemyWorld::waves`, two slots beside the barrels, 64 bytes each) and by
`tools/boss_sim.rs`, so a tape the simulator approves dodges the same waves the
disc throws. `HK_FK_WAVES` counts spawns and `HK_FK_WAVE_HITS` the spurts that
reached the hero.

Two departures, both measured choices rather than omissions. The guest spawns
a spurt a tick and draws every third one, which shows each of the clip's
three-tick frames once along the trail rather than paying eighteen quads; the
damage still uses every spurt. And `End Pause` keeps spawning for 0.15 s while
the body coasts on into whatever stopped it: actor art here is not depth sorted
against scenery, so those spurts would be drawn over the wall, and the wave
stops at the contact instead.

### Rage

Every stagger the player converts is followed by a fixed sequence: a 0.5 s
pause, a jump to x 28.9, then eight slams from a stationary `Rage` loop with
0.249 s of wind-up each and a turn between them. The eighth slam of the second
conversion cracks
the arena floor; the boss breaks through it on the third.

The body is not invincible during any of that, and the check that ends the fight
is at the end of the rage rather than at the conversion: `Rage Check` reads
`Stunned Amount` only once `Rages` has counted down to zero. So a hero who can
empty the 65 hp again before the eighth slam staggers the boss out of its own
rage and the fight never ends, however many conversions are won. That is source
structure, not a port artifact, and it is why the guest test drives a player who
punishes the stagger and then holds fire while the boss rages.

### Death

After the third conversion the boss jumps to x 34, invulnerable, and slams
through the cracked floor. It lands, opens a final time with no timeout, and the
last head kill starts a fixed 450-tick tail: `Death Anim Start` 1 s, `Steam` 3 s,
`Ready` 1 s, the `Death Head 1` clip 1 s, `Death Head Land` 1.5 s. Only then does
`Decrement Battle Enemies` run and the HealthManager death event fire, which is
what the arena has been waiting on.

### The music

`Music` applies the `Boss1` cue, which `host/area_music.py` cooks as CD-DA
track 3; the guest starts it with `KILL ALL ENEMIES` after the entrance drop,
and a death reload stops it. The same state activates `Area Title` with
`FALSE_KNIGHT`, which the guest shows as the boss title card
(`title_card::show_boss`). It ends where the source ends it: `Floor Break`
transitions the mixer to the `Silent` snapshot over 2 s, so the last Head is
finished to the sound of the room rather than the theme. `music::boss_silence`
fades the CD-DA over those 120 ticks (read from the FSM into
`FK_FLOOR_BREAK_SILENCE_TICKS`), pauses the drive, and holds the area music at
a snapshot no mix uses until a scene or a music region applies one again,
which is what `Silent` does to the source's mixer.

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

## What is wired

`game/src/enemies.rs`'s `FalseKnightRuntime` holds three things: the
`false_knight::FalseKnight` controller, the `boss::Arena` its `Battle Scene`
owns, and the `Head`'s own 40 hp `ActorHealth`. The arena rides on the boss
rather than on a world object because the boss is the only thing in the room it
drives and because `Battle Enemies` never moves except from the boss's own
`Decrement Battle Enemies`. The summoner does **not** ride the boss: it is on
`EnemyWorld` beside the projectile pool it fills, because `Runtime` would
otherwise have paid for it in all thirty-two actor slots.

**Dormant is free.** Until `BATTLE START` the runtime does one AABB a tick,
the hero's body against `Battle Control`'s trigger, and nothing else: no solver,
no terrain ray, no clip clock, and no draw. The source hangs the boss at the
authored transform, x 26.07 y 46.64, inside the ceiling slab above the arena
(layer-8 terrain from y 44 to 47) where the room's own scenery hides it; actor
draws here are not depth sorted against scenery, so the closer answer is to draw
nothing until it drops. That gate matters because Crossroads_10 carried no
actors at all before the boss and the frame is stall-bound: the fight only costs
anything while it is running.

**The trigger is the source's own.** `Battle Scene`'s BoxCollider2D is a
one-unit curtain from x 13.5 to 14.5 spanning y 25.01 to 44.60, cooked into the
`ActorSpec` as a world box. Crossing it runs `Arena::hero_entered`, which sends
`BATTLE START`, and `Start Fall` drops the body: `resolve_actor_spawn` separates
it from the slab once, there rather than on load, and the shared solver takes it
from there. It lands on `Break Floor` at y 26.05, putting its transform at
y 31.64, against the authored `CameraLockArea B` that pins the camera at y 31.37.

**The camera lock needs no wiring.** `Battle Control` enables an authored
`CameraLockArea`, and the world already cooks that one: `CameraLockArea B`
(`level46:100331`, the merged `level48` object) is live in twelve of
Crossroads_10's views with the source's own 21.60 to 33.83 limit. `Action::
CameraLock` is therefore accepted and dropped.

**Only the states that read a sense pay for it.** `GetDistance` costs a square
root and is computed in `Idle` and `Run`; the two 8-unit wall rays are cast only
in `Idle`, because `Walls Check` is reachable only through `Move Choice` and only
`Idle` reaches that; the 9.5-unit fall ray only in `JA Fall` and its death
counterpart. Casting all three every tick would be three passes over the view's
terrain on a frame that has none to spare.

**Both HealthManagers restore instead of dying.** A nail that empties the body's
65 hp gets `Check Health`'s answer, SetHP 65 and STUN, so the body is killable
and never dies; the armour rolls, opens and exposes a `Head` box that sits above
the body box rather than inside it, and emptying its 40 hp is `Health Check`'s
STUN END and one phase. Three of those reach the death sequence, and
`Action::Died` is what marks the actor dead. It declares no corpse, so it stops
being drawn there.

**The Hitter is a box, not a second draw.** The source splits attack art between
the body, which plays `Blank`, and a `Hitter` overlay. This port draws one actor,
so the overlay's clip plays on the body and the `Blank` that would have hidden it
is dropped; the overlay's DamageHero trigger is reproduced as a box reaching
nearly eight units ahead, mirrored onto the side the body faces.

**The shake and the two voices ride one action.** `Action::Effect` carries both
the `SendEventByName` shakes and the `AudioPlaySimple` one-shots, because a
state that shakes and plays at once is one event to the caller and the caller
owns the camera and the SPU alike. `game/src/enemies.rs` answers it with
`camera::request` and `audio::boss_land` / `boss_swing`, which is the only
caller either of them has.

| Source state | Sends |
| --- | --- |
| `Start Fall`, `Slam`, `JA Slam`, `Stun Start` | BigShake |
| `S Land`, `State 2` | AverageShake and `false_knight_land` |
| `Rage Slam` | AverageShake |
| `Jump`, `S Jump`, `JA Jump`, `Jump 2` | EnemyKillShake |
| `Land Noise` | `false_knight_land` |
| `S Attack`, `JA Hit 2` | `false_knight_swing` |

Two of those sit where the phase names do not suggest. `JA Slam` is entered the
moment the jump attack reaches the floor, five ticks before `JA Hit 2` swings
the mace back out of it, so its BigShake is pushed in the landing branch rather
than beside that swing. And `JA Jump 2`, the last jump of the fight, shares this
port's launch arm with `Jump`, `S Jump` and `Jump 2` while sending no shake at
all, so the effect rides the launch table instead of the arm around it.

Priority is what keeps this from turning into one long tremor: the source's
`CameraShake` refuses a request while a higher-priority shake is live, so the
slam's BigShake outlasts the jump that set it up and three slams in a row do not
stack. `game/src/camera.rs` reproduces that refusal and counts it.

**The barrels fall, and they hurt.** `FK Barrel Summon` is a second source
object with its own `summon` FSM and a `PersonalObjectPool` of eight, and the
boss only ever writes `Spawns` and sends SUMMON at it. That split is kept:
`hk_sim::false_knight::Summon` is `summon` and nothing else, and it lives on
`EnemyWorld` beside the pool it fills rather than on the boss runtime, where
`Runtime` would have paid for it in all thirty-two actor slots.

The pooled prefab is `Falling Barrel` (`sharedassets48.assets:61`), and it is
the same shape of object as the Aspid's `Spitter Shot R`: a trigger box on a
dynamic body that damages the hero and ends on terrain. So it rides the same
eight-slot projectile pool, with `Falling Barrel`'s own gravity scale 0.325 and
its own 1.18 by 1.11 box, and a `const` assertion holds `SHOTS` at or above
`BARREL_POOL`. Only one of the two can ever be live: Crossroads_10 is the only
room with a boss and it carries no Aspid.

| Quantity | Measured |
| --- | ---: |
| Barrel sprite, cooked at the 14.8178 px/unit projection | 36 x 33 px |
| Streamed bytes it adds to the scene-wide actor bank | 596 |
| Texture records it adds, of 640 per view | 1 |
| CLUT slots it adds, of 150 free in Crossroads_10's tightest view | 1 |
| Animation slots one barrel binds, of 20 a frame may hold | 1 |
| Crossroads_10's worst base pack, cooked with the barrel | 203,832 |

`MAX_VISIBLE` is 20 tiles and the boss's largest frame binds 12, so the whole
pool of eight fits beside it exactly, with nothing to spare. That is by
construction rather than by luck: `BARREL_POOL` is eight because the rage asks
for eight, and the projectile draw loop stops at `MAX_VISIBLE` rather than
asserting, so a nineteenth tile would be dropped rather than crash.

The first five rows are measured off `host/cook.py`'s own atlas; the base-pack
row is a real cook of catalogue slot 243 into a scratch directory, which is
203,832 bytes with the barrel exactly as it was without it, because the art
rides the scene-wide bank rather than the base. The per-view total after
`postpack_actor_bank` is arithmetic rather than measured: 596 stream bytes plus
a 16-byte texture record plus a 32-byte palette is 644 a view, which takes the
worst view from 353,540 to about 354,184 and leaves about 39,032 free.

What the barrel does not do, and each for its own reason:

- **Spin.** `Idle` rotates it at 720 degrees a second with a randomised sign.
  The projectile draw builds an axis-aligned quad out of the frame's world box,
  so turning one would cost a rotation per barrel per frame on a frame that is
  stall-bound.
- **Vary its size.** `RandomScale` picks 0.8 to 1.0; every barrel here is the
  authored size.
- **Answer the nail.** `Idle`'s `NAIL HIT` sends the barrel away at 45 units/s
  along the attack direction with gravity 0.3. A barrel is not a nail target
  here: the nail pass walks actors, and a barrel is a pool entry.
- **Break into anything.** `Break` hides the sprite, plays Bits and Dust Puff,
  activates Splat, spawns blood and plays a one-shot, then waits three seconds
  before recycling. Here the barrel simply leaves, at once, so it does not hold
  a pool slot through a frame it has nothing to draw.

The pool recycles the oldest live barrel when it is full, which is the source's
own rule, and that is why `HK_FK_BARRELS` and `HK_FK_BARRELS_BROKEN` are two
counters: a recycled barrel never breaks, so a route watching only the spawns
could not tell a fight the player dodged from one where the rage's eight evicted
each other.

### What the barrels cost against the 11,556 bytes

Linked RAM headroom is the tightest budget here and it has moved again:
`memory.unallocated_before_stack_bytes` is **11,556**, down from the 32,160 this
page carried a build ago, and the boss audio alone took 11,808 of it. Every
candidate below was costed against that figure before it was built, not after.

| Where it lands | Barrels |
| --- | ---: |
| `EnemyWorld` bss, measured with `size_of` | 6,112 -> 6,136, so **24 bytes** |
| Sound bank, which `audio.rs` `include_bytes!`s out of the same figure | 0 |
| Scene arena, which `SCENE_ARENA_BYTES` reserves | 0 |
| Room pack, per Crossroads_10 view | about 644 |
| `.text`, which only a link measures | not measured |

The 24 bytes are `Summon` (8) plus the summoner's site (12) plus 4 of
alignment, all on `EnemyWorld` once rather than per actor slot; the pool itself
is free, because `barrel` is a second `bool` beside `impact` and `Option` still
has a niche to put its discriminant in.
`tests/enemy_runtime.rs::the_barrel_pool_is_paid_for_in_padding_and_twenty_bytes`
holds all three of those.

The zero in the arena row is the load-bearing one, and it is why the barrels
were the candidate worth building. The art rides the scene pack, which is read
from disc into the scene arena, and `pack_scenes::scene_arena_bytes` sizes that
arena from the largest scene. From `.hkpsx/packed-scenes.json`: the arena is
417,748, Tutorial_01's resident scene is 349,032 and the 68,716 difference is
the metadata tail every scene is checked against. Crossroads_10's resident scene
is 301,304, the second largest, so it needs 370,020 and has **47,728 bytes** of
headroom before it would set the arena. 644 bytes a view is nowhere near it.

`.text` is the one number that cannot be had without a link, and a link is a
disc build. What was added is `Summon` (four small integer methods), three
one-line accessors on `Shot`, one branch in the projectile loop and one event
arm, so it is hundreds of bytes rather than thousands; the build report is the
only thing that can say how many.

The other three candidates were costed the same way and refused below. None of
them is refused on linked RAM, which is worth saying plainly: the Death Head is
refused on room-pack bytes, the staff on a rotation the draw path cannot
express, and the floor break on what is left of the arena once `Break Floor`
goes. That is the real state of the budget. Nothing large can be added to
Crossroads_10's packs either, so the next content that wants either budget has
to come with something given back.

## The gates, and the recook that sealed the arena

`game/src/battle_gates.rs` is the `great_door.rs` shape the plan asked for: a
cooked `(catalogue slot, gate, edges)` table in `data/battle_gates.rs`, a bit
per gate, and an `apply` that drops the edges of every open gate from the view's
terrain. `host/battle_gates.py` is the join and `.hkpsx/battle-gates.json` the
report. The state is a `static mut` rather than a field of `frame::Game`
because the arena rides on the boss actor several layers inside `enemies.rs`,
and threading a world down there would have cost more call sites than the
feature is worth.

The census, from `host/battle_gates.py` against `data/regions.json`:

| Quantity | Measured |
| --- | ---: |
| `BG Control` gates in the 46 admitted scenes | 15, in 5 scenes |
| Of those, cooked as terrain | 15 |
| Crossroads_10's gates, cooked / not | 5 of 5 |
| Of the 15, open on the first frame | 12 |
| Catalogue slots carrying a gate binding | 31, of 699 |
| Gate edges in the worst slot (slot 214, Crossroads_08) | 11 |
| Scripted exclusion slots in `world::State` | 12, unchanged |

`host/cook.py` bakes every gate's collider whatever `battle_gate`'s
`solid_on_load` says, so `COOKED` and `PLACEMENT_CLOSED` are now independent
masks and the difference between them is the work. Twelve of the fifteen gates
are open from the first frame, and each of those is terrain in a room pack that
the runtime lifts on load: `CLOSED` starts at `PLACEMENT_CLOSED`, so the first
`apply` excludes `!PLACEMENT_CLOSED & COOKED`. Get that wrong and the twelve are
invisible walls, which is what the cook's old skip was avoiding. Get it right
and `BG CLOSE` has edges to restore, which is what seals an arena.

Crossroads_10's `Battle Gate` at x 46.25 and `Battle Gate 1` at x 10.75 are the
two arena ends, both open on load and both now cooked, so the trigger shuts
them. `Battle Gate 2` at x 18.50 y 3.78 and `Battle Gate 2 (1)` at x 11.50
y 12.92 are the pair at the bottom of the room that the source keeps shut until
the fight is won, and `Battle Gate 3` at x 51.64 is the fifth.

The cost of getting there was a whole-world recook, because of where
`host/cook.py` sits: above the divider in `host/cook_inputs.txt`, so editing it
invalidates the per-region reuse key for all 693 cooked regions. It also
inverted the failure mode, on purpose. Before, a wrong binding could only open
terrain that should be shut, in five slots, and a gate that could never close
was silent. Now a missing binding walls a room off, in any of the 31 slots with
a gate, which is a thing a player sees on the first screen. The guards are the
generator refusing a per-slot total over `world::SCRIPT_EDGE_SLOTS`,
`tests/test_boss_arena.py` checking the join in both directions against
`data/regions.json`, and `tests/battle_gates_runtime.rs` asserting the load-time
exclusion set against the table the guest actually links.

The scratch did not have to grow. The worst slot after the recook is
Crossroads_08's slot 214 at 11 of the 12 scripted exclusion slots, three gates
in one view, and no slot carrying a gate also carries a Lifeblood cocoon or the
Great Door.

## What a route can see

`game/src/enemies.rs` and `game/src/battle_gates.rs` export these. The counters
are edges; the live values are mirrored in one place,
`Actor::publish_false_knight`, because the boss runtime is written back from three
different exits and publishing at each of them is how one goes stale.

| Symbol | What it says |
| --- | --- |
| `HK_FK_TRIGGERED` | the hero crossed `Battle Scene`'s trigger and the arena armed |
| `HK_FK_DROPPED` | `Start Fall` separated the body from the ceiling slab |
| `HK_FK_STAGGERS` | `Check Health` took the body to zero and the armour opened |
| `HK_FK_CONVERSIONS` | `Health Check` took the exposed Head to zero, which is a phase |
| `HK_FK_DEATHS` | the HealthManager death event at the end of the 450-tick tail |
| `HK_FK_HP` / `HK_FK_HEAD_HP` | the 65 and the 40, both of which restore |
| `HK_FK_ACTIVE` | out of `Dormant` |
| `HK_FK_EXPOSED` | the armour is open, so the nail reaches the Head |
| `HK_FK_STUNNED` | `Stunned Amount`, which selects the phase table |
| `HK_FK_ARENA` | `Battle Control`'s phase, as `hk_sim::boss::Phase` orders it |
| `HK_FK_ACTIVATED` | the `Activated` PersistentBoolItem |
| `HK_FK_BARRELS` | `Falling Barrel`s the summoner put in the pool |
| `HK_FK_BARRELS_BROKEN` | of those, the ones that reached terrain or the hero |
| `HK_FK_WAVES` / `HK_FK_WAVE_HITS` | `Shockwave Wave`s spawned, and spurts that reached the hero |
| `HK_ARENA_GATES` | bit per gate, set while its collider is solid |
| `HK_ARENA_GATE_CLOSES` / `HK_ARENA_GATE_OPENS` | broadcasts answered |

`tools/replay_cue.py` carries the watch list and does not yet name the two
barrel counters; the one-line addition is in this change's handover note.

The death and the Head merely emptying are different counters on purpose: the
last exposure empties the Head too, and only the tail after it is a death.

## Persistence

The `Battle Scene` PersistentBoolItem and `falseKnightFirstPlop` ride the save
record's PlayerData reserve. `SCRIPT_FIELD_SLOTS` is 16 and the cooked script
bank uses five, so the record's length, magic and every committed card fixture
under `tools/cards` are untouched: nothing about `HKS3` moved.

The two fields sit at the top of the reserve and the cooked bank fills it from
the bottom, so they can only ever meet at the `const` assertion in
`game/src/script.rs` rather than at a wrong value, and
`tests/test_boss_arena.py` checks the same relation from the host side before a
guest build gets that far. They ride the bank's `SCRIPT_FIELD_FNV` even though
the cook does not name them: a bank whose field list moved has moved the slots
under them too, and dropping a won fight is the conservative answer to that.

`Arena::new` therefore takes the source's `Activate` branch on a reload,
`Init` quick-opens the gates, and the fight never re-arms. Within a session the
same store survives a scene reload and a death respawn; across a boot it reaches
the card at the next bench, which is the only thing in this port that ever
writes one.

## What is not implemented

- **Half the gates.** `BG OPEN` and `BG QUICK OPEN` now reach the two gates the
  source loads closed; `BG CLOSE` reaches the other three and moves nothing.
  The section below has the measurement and why the missing half is a recook.
- **The pre-battle Zombies.** Recorded, not admitted, so `KILL ALL ENEMIES` has
  no target. The arena counts only the boss, which is what the source does.
- **The wave's particles.** The wave itself hurts and is drawn (see "The slam
  wave"); its `Roll Dust` and `Burst Rocks Stomp` emitters wait on the particle
  model.
- **The 200 Geo and the City Crest.** Neither is `FalseyControl`'s. The Crest
  is `Key Giver`'s `Shiny Item` (`hasCityKey`, trinket 9) in Crossroads_10's
  lower room, which activates on `FK DEATH` or a loaded `falseKnightDefeated`;
  the Geo is the `Chest` at x 6.7 y 12.78 behind `Battle Gate 2 (1)`, which the
  won arena opens. The port has no pickup or chest object yet, so both wait on
  that system rather than on this fight; the flags that gate them are written.
- **The defeat sting.** `Boss Death Sting`'s `Boss Defeat` is refused on SPU
  bytes with the rest listed under "The voices".
- **The staff.** Refused on rotation, not on bytes, as before: the draw paths
  emit axis-aligned quads, and it needs a bounce model this port does not have.
- **Most particles and two shakes.** `Floor Break`'s fling, the rubble, the
  steam and the blood are particle work; `Blow` and `Steam` shake, `Floor Break`
  shakes, and the Death Head's own Rigidbody2D is a slide along the floor.
- **Most of the fight's voices.** Two play; see "The voices" for the rest and
  where they have to go.
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
- The Godhome variants are recorded, not presented.
- Nothing about the fight has been timed on hardware. It replaces one comparison
  a tick with a terrain solve, up to two rays and a square root for as long as
  the fight runs, and the whole frame is stall-bound: the route positions are the
  test, and they have not been run.
