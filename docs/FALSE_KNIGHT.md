# False Knight source contract and the art that blocks it

`host/false_knight.py` extracts the whole fight from the installed Windows
source and refuses to admit it. The phase machine and the reusable arena
lifecycle exist and are tested (`shared/hk-sim/src/false_knight.rs`,
`shared/hk-sim/src/boss.rs`), but no frame of the boss can reach VRAM through
the current animation path. The measurement is below; read it before designing
any boss art work, because it is the same wall every large enemy will hit.

Regenerate everything here with:

    .venv/bin/python host/false_knight.py

which writes `.hkpsx/false-knight/source-contract.json` and prints the summary.

## Where the boss actually lives

Not in Crossroads_10. `BossLoader`, a `SceneAdditiveLoadConditional` in
`level46`, loads `Crossroads_10_boss` (`level48`) while `falseKnightDefeated` is
false and `Crossroads_10_boss_defeated` (`level49`) once it is true. Neither
additive scene appears in `host/quality.py`'s `SCENE_TABLE`, so no part of the
port cooks, packs or loads them today. Admitting the boss means admitting an
additive scene first; that is a prerequisite, not a detail.

## The measurement

Reproduced by `false_knight.measure_animation`, which runs `host/cook.py`'s own
actor art path (`tk_sprite`, the placement's 1.3 transform scale, the
`FOCAL / -CAM_Z` projection of 14.8178 px per world unit, one streamed texture
per frame) rather than estimating it.

| Quantity | Measured | Budget | Fits |
| --- | ---: | ---: | --- |
| Unique animation sprites | 110 | | |
| Sprites larger than 64x64 | 77 | 0 | no |
| Widest frame / tallest frame | 204 px / 188 px | 64 / 64 | no |
| 64x64 slots for the largest single frame (198x169) | 12 | 4 per frame, 8 total | no |
| 4bpp texels, all frames | 716,110 B (699.3 KiB) | 73,876 B unallocated RAM | no |
| CLUT slots, one palette per frame | 110 | 141 free in Crossroads_10 | yes |

The CLUT number is the only one that fits. `tools/texture_headroom.py` reports
Crossroads_10 at 275 of 416 slots in its tightest of 20 views, leaving 141, so
per-frame palettes would just fit if nothing else changed. That is not the
binding constraint here and sizing the work against it would be a mistake.

The binding constraint is the animation cache. `hk_cache::SLOTS` is 8 slots of
64x64 4bpp in a 32x256 halfword VRAM strip at x 320, each slot upload capped at
2,048 bytes and the whole frame capped at `MAX_UPLOAD_BYTES` 16,384. The False
Knight's largest frame alone is 16,900 bytes across 12 tiles: one frame is
bigger than the entire cache. A frame may now bind a rectangle of slots, but
only `hk_cache::MAX_FRAME_TILES` of them, which is 4; `host/cook.py` still
refuses this actor, now with the slot count rather than the pixel size.

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
the same twelve slots from linked RAM as from a room pack. The RAM does not
stretch either: `.hkpsx/build.json` reports 73,876 unallocated bytes before the
reserved stack, against 699 KiB of texels, short by a factor of ten. Read that
figure from the build report; `docs/BUDGET.md` has been stale about it before.

## The route past it

Three changes, in this order. None of them is shaving frames: the project's
standing constraint is that actor animation sampling is not reduced to fit a
scenery-grade cap, and a 3.2x linear reduction of the largest frame is exactly
that.

1. **Multi-slot frames.** A frame binds a rectangle of 64x64 slots instead of
   one, and the draw submits the tiles. This is implemented for room frames:
   `Atlas.add_tiled` splits an oversized frame into consecutive streamed
   textures, the first carries the grid in the two fields a streamed texture
   never uses for a VRAM origin, and `hk_format::Room::frame_grid`/`frame_tile`
   read it back. The NPC path uses it; the actor path does not, because
   `enemies.rs` draws a frame as one quad.

   The strip did **not** grow, and cannot. `residency::SPARE_HALFWORDS` records
   the measurement: the disjoint allocation test leaves 304 of 524,288
   halfwords unclaimed, 608 bytes, in two fragments 16 halfwords wide at rows
   480 to 495. The widest free run on any row is 16 halfwords and the tallest
   free run in any column is 14 rows; one slot needs 16 by 64. Every other
   halfword is a framebuffer, a static scenery page, a CLUT bank, the font, or
   one of the Geo, Lifeblood, SOUL and break-effect atlases. So `SLOTS` stays
   at 8 and `MAX_UPLOAD_BYTES` stays at 16,384, which is exactly the strip.

   That caps a frame at `hk_cache::MAX_FRAME_TILES`, 4: the other four keys
   `main.rs` can request in one frame are the Knight's body or ability pose,
   the nail effect, the Hollow Shade and the Vengeful Spirit ball. The False
   Knight's 198x169 frame binds 12, and 12 plus those four is 16 slots, 32,768
   bytes against 16,384 allocated and 608 free. Multi-slot frames therefore do
   not admit the boss, and no repack of the current VRAM map will: the room has
   to come out of the 20 static scenery pages, which is a scenery capacity
   decision, not a constant change.
2. **One palette for the boss.** Quantize all 110 frames against a shared
   16-colour palette the way the Shade does, so the whole fight costs a few CLUT
   slots instead of 110 and can be resident in every view of the arena.

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
   and against the live gap the prefetch does not either. Measured per clip
   against 73,876 unallocated bytes: no single clip exceeds the gap, but the
   heaviest, `Stun Recover`, is 56,406 bytes, 76.4% of it, leaving 17,470 bytes
   for everything else the project has left; and the heaviest adjacent pair is
   112,456 bytes, 152.2% of the gap. Nine of the 33 clips pair with the next
   heaviest to more than the gap. So holding the current clip and prefetching
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

## What is not implemented

- No guest admission and no art: see the measurement above. Nothing is wired
  into `host/actors.py`, `game/src/enemies.rs` or the actor pool.
- The `Esc` branch is unreachable in this arena; nothing sets `Hero Escaped`
  outside Godhome, and the `Check Facing` global transition target does not
  exist as a state.
- `Rise` and `Fall` shape vertical speed per 50 Hz FixedUpdate in the source and
  per 60 Hz tick in the sim, so airtime differs slightly.
- Particles, camera shake, audio, the barrel prefabs, the shockwave object, the
  staff, the death head and the Godhome variants are recorded, not presented.
