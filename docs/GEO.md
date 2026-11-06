# Geo, menus and session settings

The current Tutorial subset includes the original five Geo rocks, Crawler death
payouts, animated loose coins, collection and a wallet HUD. These additions use
read-only Windows source assets and observed Assembly-CSharp behavior. They do
not implement shops, saving, charms or recoverable Shades.

## Where the rocks live

50 of the 53 source rocks are cooked; the three of Fungus1_21 stay out because the art bank cannot hold their art. Nothing about a rock is linked into
the guest except the coin and art-bank tables in `data/geo.rs`. A rock is a `KIND_GEO_ROCK` object in the first region of its scene's metadata bank
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

## The three refused rocks (Fungus1_21), the per-scene bank trial (2026-11-06)

Geo art is one bank uploaded at boot (`geo_render::upload`, `data/geo.rs GEO_UPLOADS`, placed in the five VRAM slivers of `host/geo.py VRAM_RECTS`,
6,336 B of rectangles). The bank must hold the union of every rock of every admitted scene plus the coins, which is why Fungus1_21's three rocks
no longer fit after the other 50. The design that was proposed (coin art in a fixed region, each scene's rock art cooked and uploaded alone into
the rest) was built as far as the host trial and measured; it does not fit. Numbers (host/geo.py as of the trial, branch `hk-world-geo-bank-wip`):

* Coin art alone: 46 distinct textures and a 16-halfword palette, 3,684 B. That leaves 2,652 B of the 6,336 B for rocks.
* Per scene, the rocks' own bank (palette plus distinct textures): 620 to 814 B for most scenes, 1,356 B for seven (Tutorial_01's neighbours with four
  or five rocks, Crossroads_05 and 07's, Fungus1_01), 2,138 B for Fungus1_31 (three tall rocks). Fungus1_21's three rocks: 1,076 B (four textures: a 35x36
  rock cut in three 12-row strips and a 35x12 piece).
* Fixed coin region searched over 419 placements (MaxRects with 400 shuffles, coins kept out of the tall 32x27 sliver): the rock region left is 1,478 to
  1,988 halfwords in pieces, the largest the whole tall sliver (864 halfwords). Fungus1_21's bank places in every one of them. Seven of the 30
  scenes that hold rocks never do (Fungus1_31 and six scenes with four or five rocks): the rocks are 7 to 9 halfwords wide and 12 to 22 rows tall, a
  coin layout cannot leave them room, and the best of the 419 layouts has the same seven failures as the first. So a per-scene rock bank beside a
  fixed coin region would lose rocks that are cooked today.
* The global bank does not fit by plain arithmetic, whatever the packing: the coins and all 53 rocks are 59 distinct textures, 6,722 B, in 6,336 B of
  slivers (the 50 admitted rocks are 5,702 B, 90 percent). A placement needs about 8 percent slack on top, so Fungus1_21 needs about 1.1 KB more reserved VRAM.

What would fit: (1) 1.1 KB more reserved Geo VRAM (the same mechanism, no guest change, the three rocks cook): the free pieces in `residency.rs` are the 304
spare halfwords (BUDGET.md) and the 64x4 piece at (320, 460) the claim list keeps out of the spare count (256 halfwords), together about 1.1 KB, but both are
thin, so `STRIP_ROWS` would have to drop to 4 for them (more quads per rock, each costs a draw); or (2) a full bank per scene (the coins and that scene's rocks
placed together, the way the global bank is today: the worst scene needs 5,790 B and all 30 fit the 6,336 B), with the coin entries in the scene bank instead of
a linked table. That costs the bank bytes (3.7 KB of coin texels in each rock-bearing scene's metadata bank, which sets the scene arena for the peak scene) and
376 B of resident RAM for a coin table filled at admission; or (3) the three rocks stay refused (they sit past the tolls, off the toll's Geo income).
The guest side of (2)/(1-variant) is on the WIP branch: `KIND_GEO_UPLOAD` (25) objects in the bank's first region, rock art quads carrying their own
u/v/w/h/clut/tpage words, `geo_render::upload_scene` at bank admission.
