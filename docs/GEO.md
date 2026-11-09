# Geo, menus and session settings

The current Tutorial subset includes the original five Geo rocks, Crawler death
payouts, animated loose coins, collection and a wallet HUD. These additions use
read-only Windows source assets and observed Assembly-CSharp behavior. They do
not implement shops, saving, charms or recoverable Shades.

## Where the rocks live

All 53 source rocks are cooked (Fungus1_21's three since the per-scene bank, below). Nothing about a rock is linked into
the guest except the coin tables in `data/geo.rs`. A rock is a `KIND_GEO_ROCK` object in the first region of its scene's metadata bank
(`tools/world_metadata.py`: source id, scene-local state, bounds, collision polygons, position and payout counts); the regions that draw it carry its art
quads as `KIND_GEO_ART` (tag 0 while broken, 1 while whole) and what a broken rock hides and unblocks as `KIND_GEO_VIEW` (a draw list then an edge list).
`host/geo.py` `bank_rows` writes those rows into its report and `host/pack_scenes.py` hands them to the encoder. A save restores a rock by its state key
before any bank is admitted (`World::restore_rock`); the first swing that reaches it binds its source id and clamps the saved count to the bank's hits.
Rock art is trimmed to its visible box, cut into stacked strips of at most 16 rows when taller than the reserved VRAM slivers, admitted by an exact bank trial and placed by 400 shuffled restarts when the ordered tries fail. `walk-geo-rock-05` and
`walk-geo-rock-05-restore` play a rock through its bank from seeded cards. Linked as tables they cost 2.8 KB of free RAM; in the bank
the same rocks cost 176 B of free RAM (11,076 to 10,900).

## Gameplay behavior

Four rocks take five hits, emitting two small Geo on every hit and five more on
depletion: 15 each. The special rock takes four hits, emits three per hit and six
more on depletion: 18. The last hit includes both payouts. Exact source polygons,
cooldowns and per-attack/source identities prevent duplicate rewards. Depletion
switches to original broken art. Rock state survives spatial view changes.
The source collision matrix excludes the Knight's solid body from rock contacts;
these rocks deliberately do not add terrain edges. Nail contacts are enabled.

Supported Crawlers emit two small Geo once per death. A failed bounded emission
queue is retryable and never marks the actor paid. Currency uses denominations
1, 5 and 25, a 15-tick pickup lock, and the source wallet cap of 9,999,999. Pickup
is an entry event: entering during the lock requires leaving and entering again.
There is no ordinary pickup lifetime expiry. Actual scene exit discards loose
coins; spatial view crossings retain them. Death clears the wallet, and Select
clears the development session. Shade recovery and save persistence are missing.

The deterministic pool holds 64 coins and 16 pending source emissions. Overflow
retains denomination counts instead of crediting the wallet invisibly. Source
50Hz body callbacks run five times per six 60Hz simulation ticks. Fixed-point
fling, swept boxes, slopes and bounce approximate Unity physics; they are not
Box2D parity. Coins outside the resident collision apron keep their state and
pause physics. Resting coins cache their supporting edge and wake if it changes.

## Art and audio

All 47 source Idle/Air coin animation frames and intact/depleted art for all five
rocks are represented by 57 references to 50 unique textures. Exact source art
sharing chooses dimensions large enough for every alias. A joint 15-colour
palette and 4bpp texels occupy 5,154 bytes. Alpha retains transparent, half-alpha
and opaque classes using STP and PS1 average blending. Full source rendering,
rock gleam, jitter and destruction debris are still incomplete.

Geo art uploads once to disjoint strips totalling 9,920 reserved VRAM bytes.
Scenery CLUT capacity is now 1,248, of which 1,206 are used; an unused strip from
the former fourth CLUT group supplies contiguous space for rock art. Existing
static pages and their palette samples are unchanged by this addition.

Six complete source pickup/hit clips use 13,904 SPU bytes at 11,025Hz mono.
Voices 12–14 handle pickup, hit and destruction. Destruction reuses the existing
first source destruction sample; its second variant remains omitted. Only
1,520 SPU bytes remain above this bank. Events require no CD reads or uploads.

## Menus

The title offers Start Game, Options and Controls. Up/Down selects, Cross/Start
opens, Circle returns, and Left/Right changes volume. Start Game is selected
initially, preserving the first Start/Cross behavior of existing recordings.
Start in gameplay opens a pause menu with Resume, sound effects, ambience and
Controls. Simulation stops while paused. Volume choices apply immediately to
already playing voices and persist for the current run; zero is a real mute.
The original Perpetua font is shared by the title, pause pages and wallet.
Title scenery animation, music and save slots remain missing.

## Reproducible evidence

`tools/replay_cue.py` accepts a poll tape and a new ignored output directory. It
runs the sole canonical CUE with the verified headless embedded boot path,
binds EXE/map/disc/tape/frontend hashes, records counters and saves software,
hardware, VRAM, audio and RAM captures. It neither builds nor copies a disc nor
modifies guest RAM. The current runs and final build identity are in STATUS.md.

Native tests cover source hit/payout rules, polygon exclusion, pickup timing,
queue pressure, denominations, deterministic physics, dynamic support removal,
menu button edges and audio voice behavior. Source provenance, collision matrix
and CIL evidence remain ignored under `.hkpsx/geo-source/`, `.hkpsx/geo-runtime/`
and the Geo cook reports; retail asset dumps are not part of shared docs.

## Geo art per scene (2026-11-08)

Every scene has a Geo bank of its own: the coin frames and that scene's rocks, quantized to one palette and placed together in the five VRAM slivers
of `host/geo.py VRAM_RECTS` (6,336 B of rectangles). A rock is admitted when the bank built from the scene's admitted rocks and the new one still
places (an exact trial, as before, but per scene: rocks of other scenes no longer compete for the slivers). The worst scene, Fungus1_31 with three
tall rocks, needs 5,790 B; scenes without rocks carry the coins alone (3,684 B), so VRAM is right whichever scene came before.

The bank is a block after the particle art in the scene's effect chunk (`host/geo.py block_bytes`, written to `.hkpsx/geo-banks/scene_N.bin`, appended by
`hk-cook break_effects.rs with_geo_bank`, its offset the chunk's last word). `prepare_effect_art` uploads it (`geo_render::upload_scene`) in the same pass
as the particle art, before the scene owns the arena, so it costs no arena and no resident table beyond `COIN_TEX` (the coin frames' u, v and page, 184 B
of BSS). Each rock's art quads (`KIND_GEO_ART`) carry their own texture words (u, v, w, h in payload word 0, clut and tpage in word 1). The metadata-bank
variant (a `KIND_GEO_UPLOAD` object in the first region) was built first and dropped: the scene metadata bank sits in the arena tail, and Tutorial_01 sets
the arena. `walk-geo-rock-21` breaks one of Fungus1_21's rocks from a seeded card; `walk-geo-rock-05` and the chains cover the scenes that had rocks.
