# Shared scene and gameplay budget

The provisional69 build from 2026-09-15 keeps one scene at a time in the RAM
arena and replaces its static VRAM atlases at scene gates. These are linked reservations, not hardware
performance or main-stack high-water measurements. Exact map and artifact hashes
are in `.hkpsx/build.json`; every build replaces the sole playable disc.

| Main RAM allocation | Bytes |
| --- | ---: |
| Linked code |808,092|
| Code-to-data alignment |4|
| Linked data, including menu and HUD |281,440|
| BSS, including shared scene arena and runtime pools |797,540|
| Total static span |1,887,076|
| BIOS/kernel reservation |65,536|
| Linker stack exclusion |49,152|
| Space above initial stack pointer |256|
| Unallocated gap before reserved stack |95,132|
| Room module pool, inside that gap |94,208|
| Free RAM below the module pool |924|

Since rooms stream their enemy code and art (host/code_modules.py), the room
module pool is carved from the top of the gap below the stack, so the free
figure to plan against is the gap less the pool. The 2026-10-02 merge of the camera, enemies, parity, visual and streaming
branches overflowed RAM by 2,944 bytes against a 96 KiB pool; the pool is
92 KiB now (the worst room plus the carried Shade art needs 80,972 bytes),
which left 1,152 bytes below it. Reading the renderer's scenery draws in
place from the scene bank instead of copying them (2026-10-03) returned
16,788 bytes. The playtest fixes (2026-10-03: a vignette factor per draw,
1,408 bytes of data, the stalactite batting and the check) took 4,100, and the soft
Break Dust (palette copies with the semi-transparency bit, drawn in its
authored colours) took 4,104 more, mostly code that crossed an alignment step,
which left 9,736. A broken stalactite's flung rocks, dust and sounds
(2026-10-04, the rocks flying in the Geo coin pool) took 2,072 of those, one
alignment step: 7,664 were free below the pool then. Playing the title and the fights as
XA songs through the player the game carries (game/src/xa_player.rs, the directory
lookup of MUSIC.XA in disc.rs, 2026-10-04) took 2,064 of them: 5,600 are free below the
pool now. The pad driver of the SDK pin that holds the analog request and the
ACK-paced poll (psx-pad, PadReader and require_analog_port1 at boot,
2026-10-04) grew the static span by 4,096 bytes (2,608 of them the driver
itself) and would have left 1,504 below the pool; compiling menu::run, save::write and render::init for size, none of
which runs in a gameplay frame, took it back, so the figure is unchanged. Brightness and the screen
position on the title's Options page (game/src/display.rs, 2026-10-05) took 12 of them, with
`menu::draw_menu` compiled for size too (it grew by 1,512 bytes with three more rows before that) and
the hero light's clip at the Knight's feet, which a platform showed to be wrong, gone: 5,588 are free
below the pool.

The per-tick `HK_TRACE` block for the replay harness (game/src/trace.rs, 2026-10-08) took 512 bytes of
BSS and 1,260 of code, and compiling seventeen functions that only run on a scene change,
a gate, a save or a debug reset for size (`optimize(size)`: the disc cache's init and
prepare_*, enemy sync_region, persist, the save survey, take_gate and the like) gave back
8.2 KB; the Focus effects (23 frames in the ability art, the Burst at half resolution, the focus_fx module) took 12.3 KB of that back, and 924 are free below the pool now. The HUD's Geo coin replaced the word "Geo"
(three glyphs), and the render policy and the audio start added a few words.

Every figure above is the current ordinary build's, read out of
`.hkpsx/build-normal.json`, and `tests/test_budget_doc.py` fails when this table
drifts from it. `pgo` writes `.hkpsx/build-pgo-normal.json` instead, because its
code size follows the profile; `.hkpsx/build.json` is whichever image is on the
disc. The table stood
at 113,636 bytes of headroom for long enough that it was quoted in three
separate agent briefings after it had fallen to 73,876. A budget
document nobody can trust is worse than none, because work gets planned against
it.

The scene arena is sized by each scene's own world metadata bank rather than
the widest one (host/pack_scenes.py `scene_arena_bytes`): Crossroads_10, the
largest resident scene since the False Knight, had been paying for
Tutorial_01's 69,640-byte bank behind it. That took the arena from 428,572
back to 394,944 and is most of the move from 116,504 bytes free (f2d79f2, per
its message) to 141,864.

Four branches landed on 5c3da2d's 139,816, each measured on its own against
it: fix/slice-gaps-3 (soul totems, title cards, pickups) 20,128, the quick
map, Cornifer, the Snail Shaman's spell and the Elder Baldur's rollers
(feat/map-and-spell) 32,404, the Brooding Mawlek (feat/mawlek,
docs/MAWLEK.md) 28,808, and the Husk Guard (5b50498) 10,240. Merged at
42ac0b1 the gap is 44,156, 4,080 below what those figures add up to; that
remainder has not been attributed.

The boss title cards, the Mawlek's roar lock and its arena's mask shard
(feat/boss-polish) cost 8 bytes net on 1766337's 44,156: code grew by 608
and linked data shrank by the same 608, with 8 more of BSS.

The pickup polish (fix/slice-polish: chest and pickup sounds, the kneel,
the chest lid and fling, enemy hit sparks) took 3262273's 44,148 to 33,156,
10,992 bytes: code 2,736, linked data 1,360 and BSS 6,896. Of the BSS,
6,880 is the scene arena, which Tutorial_01 sets: its chest's Open clip
and one held kneeling frame ride its bank (host/pickups.py LEVEL_CAP).

The secrets (feat/secrets, docs/SECRETS.md) took 4736b2e's 33,156 to 28,948,
4,208 bytes: code 18,324, BSS 112, alignment -4 and linked data -14,224. The
data fell because the glyph sheet, Geo and Lifeblood art moved out of the
executable into the title art chunk the boot already reads from disc
(399e989, 17,774 bytes). The scene arena grew 3,908 for Tutorial_01's secret
metadata.

Static data ends at `0x801dcb64`; the reserved stack begins at `0x801f3f00`.
The cheat HUD has nine red and22 blue packet slots, reusing existing art.
There is no heap. The single scene arena occupies 410,612 bytes, replacing five
256KiB room slots (1,310,720 bytes). Renderer template/cover arrays support
1,120 scene-global texture IDs; the animation cache still permits 2,048 keys.
Geo logic, resident art/audio, functional menus, break-effect and Lifeblood
tables increase the static span; the pinned Town/Tutorial pack and its
regenerated tilemap fills changed the data size.
The recovered gap is available for bounded future data and precompute tables.
It is not a measured stack allowance; main-stack high-water remains unmeasured.

The former EXE's `main` prologue allocated 33,216 bytes by itself, exceeding
its 32,768-byte reservation before nested calls. `host/stack_budget.py` now
derives a project linker script from the unchanged pinned SDK, protects 48 KiB,
and checks the actual final main prologue before packaging. The generated
script's content hash is part of its filename so budget changes force a relink.
The current main frame is 19,032 bytes, leaving 30,120 reserved bytes for nested
calls and other stack use; this is **not** an observed high-water measurement.
It was 35,688 at build 154, and moving the 12,112-byte particle pool out to a
static took 17,256 off the frame rather than 12,112, because constructor
temporaries shared its lifetime. Do not read a frame delta as a struct size.
The linker also excludes the 256 bytes above its initial SP from static RAM.
Current evidence: `.hkpsx/build.json` and `.hkpsx/game69/build-provisional.log`.
The outlined enemy terrain helper has a2,336-byte final binary frame, including
a2KiB edge cache reused per actor. It adds no persistent RAM allocation.

Particle lifetime uses a nonzero representation, eliminating the separate
Option tag: the 128-slot pool shrinks from 6,668 to 6,156 bytes before the four-byte cached active count without changing
counts or trajectories. This saves 512 bytes in the stack-resident world state;
it is not 512 bytes reclaimed from BSS. Compiler frame layout also changed, so
do not attribute the entire main-frame reduction to those 512 bytes alone.

Exact particle tracks add399,432 bytes plus142 bytes of emitter bases. Each of
1,513 source-seeded particles has33 two-word phases. The live pool adds256 bytes
for128 track IDs, making it6,416 bytes; Particle remains48 bytes. Tutorial emitters use these tracks; four Town emitters retain scalar evaluation.
These are main-RAM values, not extra VRAM/SPU data or traversal transfers.

Both paragraphs above describe a pool that lived in main's stack frame. It no
longer does. The pool is now a static in `game/src/particles.rs` reached through
`particles::pool()`, so its bytes are linked data and a capacity change costs
main's frame nothing. It is 224 slots of a 52-byte Particle plus a u16 track ID
each, 12,112 bytes, and that much should move out of the main frame and into BSS
at the next cook. The stack figures in this file predate the move and the
224-slot widening; refresh them from `.hkpsx/build.json` rather than from here.
One stack cost did not move: `break_effects::tick_specs` still holds a
per-capacity swept-bounds scratch, 16 bytes a slot, 3,584 at 224.

The main frame increased beyond the pool's256 bytes because compiler-generated
constructor temporaries share its lifetime; do not equate pool and frame growth.
Pass56 reduced main to18,168 bytes by outlining construction, but regressed frame
cadence and was reverted. Its14,624-byte constructor still held intermediate
copies. The48KiB reservation and high-water caveat apply to the retained build.

| Scene | Raw bytes | Stored bytes | RAM offset | Decoder suffix capacity |
| --- | ---: | ---: | ---: | ---: |
| Tutorial |321,956|181,163|0|354,032|
| Town |88,200|53,583|0|354,032|

HKSCNE02 resident data omits static page/palette payloads held in VRAM.
Geometry, animation and alpha metadata share one scene arena; only the active
scene occupies it. A separate86,016-byte aligned BSS arena holds that scene's
HKOCSC01 visibility certificates:83,692 bytes for Tutorial or27,192 for Town.
Those proofs decode through the empty geometry arena before atlas and geometry
admission. Moving aggregate linked proofs into the reusable bank recovers14,320
net linked bytes after additional parser/loader code and alignment. This does
not establish capacity for every original scene. Both arenas are checked against
the final link map; main-stack high-water remains unmeasured.

The renderer uses948 bytes of CPU-only scratchpad. An888-byte draw workspace
(three32-piece partitions, eight occluders and union bounds) overlays the
600-byte owner grid used earlier during coverage collection;60 row-mask bytes
remain disjoint. Explicit phase initialization
ends owner borrows before draw access. These are never DMA sources or stack
storage;76 bytes remain. Row retirement events use2,760 bytes instead of1,560:
one source-row mask replaces repeated per-cell retirements. Packed texture
classification adds320 aligned bytes and replaces runtime atlas inspection.
The linked data above includes101,886 bytes of host-certified4px coverage tables
for1,474 immutable original poses. The host cap is104KiB and the final linker
continues to enforce the protected stack. No extra VRAM is allocated.

The renderer retains 480 local scenery records and 1,032 draw packets; global
texture templates and alpha covers have 1,120 slots. The dedicated2,048-byte CD
IRQ stack and136-byte context are included in BSS. An observed minimum SP across
both stack allocations is not main-stack consumption; high-water remains unknown.

| Gameplay VRAM reservation | Bytes |
| --- | ---: |
| Two 320×240 16-bit framebuffers | 307,200 |
| Nineteen persistent 256×256 4bpp scenery pages | 622,592 |
| Twenty-four 64×64 4bpp animation slots | 49,152 |
| Physical capacity for 1,248 texture CLUTs | 39,936 |
| Health and SOUL HUD rectangles | 2,208 |
| Shared black-mask fade CLUT | 32 |
| Original dialogue font strips and CLUT | 7,712 |
| Geo texels and CLUT strips | 6,336 |
| Lifeblood cocoon, Scuttler and blue-mask texels with CLUTs | 5,120 |
| Breakable-effect texels and CLUTs | 7,680 |
| Total | 1,047,968 |
| Unallocated within 1 MiB | 608 |

Each scene uses page/palette bases(0,0) with exclusive ownership. The
1,248-palette physical reservation is unchanged; the page reservation is 19 of
the 20 the layout can address, because the twentieth is now the animation
cache's second region. Coordinates are VRAM halfwords:

- Framebuffers: x0..319, y0..479. Animation slots: x320..351, y0..255 and
  x960..1023, y256..511.
- Health texels: x352..355, y0..31; CLUTs x352..367, y480..481.
- SOUL texel reservation: x356..383, y0..31; CLUTs x352..367, y482..484.
- Dialogue: texel strips x0..319,y500..511; CLUT x320..335,y481.
- Black-mask fade CLUT: x320..335,y480.
- Static page `p` in 0..18: `(384+(p%10)*64, (p/10)*256)`. `p=19` would be
  `(960,256)`, which is the animation cache's second region instead.
- Bank0 CLUTs: first400 at x0..319,y480..499; last16 at x336..351,y480..495.
- Bank1: x320..383,y256..359. Bank2: x320..383,y360..463.
- The former bank3 CLUT area is no longer reserved for scenery.
- Geo: (320,464,64,16), (352,240,32,16), (352,485,32,27),
  (320,496,32,16), (352,32,8,32), as x/y/w/h halfwords.
- Lifeblood: (352,96,32,80). Break effects: (352,176,32,64), (360,32,24,32),
  (352,64,32,32); the latter two were carved out of Geo's former
  (352,32,32,64) strip. The final2KiB transfer admits Town crystal particles.
- The Hollow Shade reserved 14 CLUT rows at x320..335, y482 and uses two. The
  Knight's ability clips take four of the rest, (320,484,16,1) upward, and use
  two. The charm board's icons take the next four, (320,488,16,1) upward, and
  use all four. The hit flash's silhouette palette (game/src/render.rs) is
  y492, and Goams, stalactites and grub jars (host/props.py) take
  (320,493,16,1) and the row below it. That leaves one row of the fourteen
  unreserved, y495, and two reserved but unused inside the ability block. No
  art here reserves a texel rectangle: all of it lives in linked RAM and reaches
  VRAM through the shared 64x64 animation slots, because an ability is usable
  in any view, a Shade can be in any scene and a pause screen is reachable from
  every view (and props are too few per scene to earn atlas space), so the figure that applies to each is the tightest view in the
  whole world rather than any one scene's.

### Where the sixteen extra animation slots came from

The animation cache was eight slots in a 32x256 halfword strip at x320, which
is half of one texture page, and it cannot grow: the disjointness test leaves
304 halfwords unclaimed in two fragments 16 halfwords wide, against a slot's 16
by 64. A ninth slot had to come from a whole reservation, and the static
scenery pages were the only one large enough.

Measured over all 45 cooked scenes in `.hkpsx/packed-scenes.json`, the page
reservation was never full. Tutorial_01 is 18 pages, Crossroads_ShamanTemple is
9, Crossroads_10 (the False Knight's arena) is 8, and 44 of the 45 are 9 or
fewer. Pages 18 and 19 were reserved and no scene has ever uploaded either.
Page 19 is now the animation cache's second region, at (960,256): a 4bpp
texture page is 64 halfwords by 256 rows, which is exactly four 64x64 slots
across and four down, so one page is worth 16 slots. Page 18 is what remains of
the spare, and Tutorial_01 keeps one page of headroom.

The cost is bounded and measured. No cooked scene loses anything. Every scene
uses page base 0 under scene-gate residency, so the released page is a suffix
of the range and nothing needed relocating. The one thing that no longer fits
is a joint-residency plan whose scenes sum to 20 pages, which the shipped disc
does not use; `tests/test_pack_scenes.py` pins that.

Two costs are not measured here. Raising `MAX_UPLOAD_BYTES` from 16,384 to
49,152 raises a cap rather than a per-frame transfer, since a frame uploads
only its misses, but a 12-tile boss frame changing pose is a 24,576-byte VRAM
write and its GPU and DMA time has not been timed on hardware or in the
emulator. The cache's own storage grows by about 224 BSS bytes for the 16 extra
slot records and the wider request array, which is derived from the struct
layout rather than read off a link map.

Allocator tests check all reservations together. The title temporarily uses
89,600 VRAM bytes that gameplay reuses. Its menus share the existing7,712-byte dialogue font reservation. The source
pack is already in linked RAM and is not a simultaneous extra gameplay VRAM allocation.

## Content and transfer bounds

The 98-region layout retains the source scenery instances, geometry, layers,
parallax and tint. Scenery uses a 48-pixel longest-axis cap. The new Focus clips
add 26 frame records per region. Crawler corpse clips, grass impacts and the
eight admitted door-art variants now share these same budgets. The dynamic draw reserve is 264 packets: up to 8 Knight/nail/enemy packets,
32 combined fragment/grass-effect packets and 128 source-particle packets, plus 80 Geo quads and 16 Lifeblood parts. The28 persistent fragments do not require new VRAM banks.

The three Great Door stage frames add 2,208 animation-bank bytes and three
palettes, using one existing animation slot while visible. Their distinct
source frame bounds are retained. No static page or additional VRAM rectangle
is reserved for the door.

The two compact scene payloads total206,518 stored bytes. The23 startup atlas
chunks hold694,048 raw /321,841 stored bytes. Combined stored size528,359 bytes
is21,873 bytes larger than the former combined scene encoding, trading a small
startup read increase for699,876 bytes removed from the resident arena.
WORLD.PAK begins at LBA1024; its32-entry table binds2 scenes,6 ambience clips,
23 atlas chunks and the final Focus sound bank. Original HKROOM02/full HKSCNE01 files remain host analysis
inputs. Raw/stored/source/decoder hashes are in the packed-scene/build reports.

Startup loads ambience and Focus through the empty arena into SPU, then validates and
uploads atlas chunks synchronously before admitting compact scenes. No atlas
buffer is reused before draw_sync completes. Retry revokes admission before
menu artwork overwrites VRAM, ensuring all pages are restored on the next start.
Pass49 startup reads509 sectors; static uploads total694,048 bytes with a32KiB
maximum chunk. Both counters stay unchanged throughout the long gameplay route.
The24 animation slots retain an independent48KiB maximum per frame. Animation
frames come from resident RAM, with no traversal CD reads or decoding. This is
the current two-scene subset, not full-game residency or hardware timing.

Current SPU allocations, re-read from the banks' own generated constants
(`data/sfx.rs`, `data/geo-audio.rs`, `data/ambience.rs`, `data/world-sfx.rs`,
`data/focus-audio.rs`, `data/runner-audio.rs`) rather than from the last
paragraph that quoted them: player SFX 75,184B at 0x1010..0x135C0, the menu
clips 2,560B above it, Geo 13,888B at 0x14000..0x17640, ambience
0x18000..0x4A0A0, the area music ring 16,384B at 0x4E1B0..0x521B0, the world
one-shots (enemy death, the Knight's death, menu confirm and start, the
Lifeblood cocoon) 59,296B at 0x521B0..0x60950, Focus 108,208B at
0x60950..0x7B000 and Runner 20,464B at 0x7B000..0x7FFF0, below the 16 bytes
`psx_spu::init` parks the disabled reverb work area on. Focus and Runner are
pinned to the top; the world bank is cooked to end where Focus begins, and
`host/ambience.py` stacks the ring and its own ceiling below it.
Ambience is loaded per area since 2026-09-23: a clip is read at the scene gate
that first needs it, into an address it shares with clips of other areas, and
two clips share bytes only when no cue plays both and no gate joins scenes
that do. Every loop is held whole, cave_noises' 99,936B included since area
music took its RAM copy and ring. The widest set any cue or gate keeps
resident is 204,960B (Cave plus the MiscWind loop across the Crossroads gate
between them), against 280,928B plus a 99,936B RAM copy when all eight loops
were resident; Fog Canyon's and the Waterways' loops, which no admitted scene
plays, never load. That leaves **16,656B** of ambience headroom at
0x4A0A0..0x4E1B0, plus gaps of 64B below Geo and 2,496B below ambience. Before
the world bank the headroom was 59,584B and 16,384B above Runner sat unused,
while the cook refused the five world clips against the 64 bytes left below
Geo. The per-scene one-shot banks (docs/SCENE_SFX.md) take no range of their
own: each scene's clips sit in the ambience region's gaps that no stem able to
sound in that scene uses, 16,656 to 56,432 bytes per Crossroads scene. `.hkpsx/ambience.json` `resident_by_cue` lists
each cue's loops and bytes. Before per-area loading, ambience *shrank* 16,272B
when the resident atmos set went from six channels to eight, because eight
loops only fit at 4 kHz and the set that covers the game is cheaper at that
rate than six were at 8 kHz; Focus and Runner moved down with it. Their bases
are hardcoded in `host/focus_audio.py` and `host/runner_audio.py`, and
`host/ambience.py` refuses to finish a cook that leaves either of them stale,
naming the base each one has to take. The cheapest further margin priced but
not taken is still 8,192B from halving the streamed clip's SPU ring, which is
sized for the 8 kHz era and gives the streamed loop 3.58-second halves rather
than the 1.79 s `game/src/audio_stream.rs` is written and commented for. The figures this
paragraph's first version quoted were wrong in three ways and omitted the
Runner bank entirely, which put the free figure 20,464B too high on its own;
`host/ambience.py::spu_ceiling` computes the ceiling the cook actually
enforces. The SDK silent loop is at 0x1000. Player short effects/heal
use 22,050Hz mono; running and Geo use 11,025Hz mono; sustained Focus charging
uses 8,000Hz mono. Ambience is one rate now: all eight resident loops are
4,000Hz mono, because eight do not fit SPU at 8,000. Only
`cave_atmos_misc_3` measures better there than at 8,000; the rest pay between
0.45 and 4.93dB for the coverage, channel by channel beside
`cook_music.RESIDENT_ATMOS_CHANNELS`. All admitted source clips are complete.

The full99,936-byte `cave_noises` is resident in main RAM, feeding a16KiB SPU
ring through8KiB scratch. It keeps the streamed slot under the eight-channel
set: a streamed clip only occupies the ring whatever its length, so which
channel streams is chosen for what stays exercised on hardware rather than for
bytes, and the Cave cue keys this one on in 41 admitted scenes. Other ambience and Focus payloads stay in SPU;
startup uses the scene arena and adds no permanent staging buffer. Focus uses
voice18 and alternating19/20 heals. No playback-time CD read is needed for
these effects. CD-DA title music uses44,100Hz stereo directly from the disc,
without a voice or sample bank; completed Pause precedes room loading.
The final66 startup reads562 data sectors and uploads694,048 static VRAM bytes;
both counters remain unchanged through the validated Town traversal. See
MUSIC.md for source and replay evidence. Full death layers, numerous effects
and boss music remain pending. Physical CD/SPU timing remains unverified.

Pass40 retains480bytes of packed scenery-extrema indices and a108-byte
initialized-prefix occluder snapshot. The former replaces an experimental
7,680-byte full bounds table, recovering7,200bytes. Background packet words
reuse the existing pool after DMA ownership ends; no second framebuffer or
texture allocation is added.


Historical pass48 included8,180bytes of optional grouped seam certificates (124
selected bank groups, maximum32 per region). The inverse pool map uses6,080
logical stack bytes only during admission; final `render::init` frame is6,288,
so main+init totals31,824 before nested calls/IRQ. Collision sweep caching uses
2,048 logical stack bytes: final tick frame4,352, with the observed static
main/State/tick/collide chain totaling30,960. These chains fit the48KiB reserved
stack, but are not a measured high-water guarantee. The subsequent compact-scene split recovered nearly700KiB without removing
these coverage tables. Historical stack chains above are not current high-water measurements.

## Where the remaining headroom went (build 107)

Headroom before the stack is 68,988 bytes, down from 246 KB at build 94 and
116 KB at build 106. The link map ranks the reservations that account for it:

| Reservation | Bytes | Section |
| --- | ---: | --- |
| `disc::BUFFERS` (scene arena) | 421,176 | bss |
| `HK_AMBIENCE_STREAM_SOURCE` | 199,872 | bss |
| `main` | 130,808 | text |
| `disc::COVERAGE_BUFFER` | 83,700 | bss |
| `audio::BANK` | 63,376 | bss |
| `render::PACKETS` | 57,792 | bss |
| `render::TEMPLATES` | 46,080 | bss |
| `render::DRAWS` | 33,792 | bss |
| `data/shade.hk` | 30,386 | data |
| `enemies::Actor::advance_in_view` | 29,656 | text |
| `render::SCISSOR_CACHE` | 28,160 | bss |

Build 109 halved that stream's rate, so the table's 199,872 became 99,936 and
headroom 166,876, which is still the reservation: raising the resident set to
eight channels left `cave_noises` streaming and did not move it. The rest of
this section is the analysis that led there.

The one disproportionate entry is `HK_AMBIENCE_STREAM_SOURCE`: 199,872 bytes,
nearly three times the remaining headroom, holding one scene's streamed
ambience stem in full. The clip is already read from the disc at scene
admission and then fed SPU-ward in chunks by `audio_stream::service`, so the
resident copy is what costs the RAM, not the streaming itself. Replacing it
with a small ring refilled from the CD, the shape `music.rs` already uses,
would recover about 200 KB and take headroom to roughly 269 KB. The risk is
audio underruns from CD contention, which `HK_AUDIO_STREAM_UNDERRUNS` already
fails a route on, so the change is testable rather than speculative.

`render::TEMPLATES` is `SCENE_TEXTURE_CAPACITY` (1,152) times 40 bytes, and the
largest admitted scene uses 1,138 textures, so there is almost no slack there.
The scene and coverage arenas are sized by the largest scene and cannot shrink
without changing what a scene may contain. `main` at 130,808 bytes of text is a
single function; splitting it would help only if the pieces stopped being
inlined into one another.

P14 adds movement abilities whose new Knight clips cost both per-view texture
budget and linked RAM, so the ambience change is worth making before that
package rather than during it.

## What build 109 did, and what is left

Streaming the clip from the disc was the wrong idea: docs/MUSIC.md records that
the resident copy exists precisely to avoid the CDDA and data-read contention
the SDK's `cdda-read-contention` example demonstrates, and all six loops load
once at boot rather than per scene. What the clip actually needed was a lower
rate. 99% of its energy is below 637 Hz and only 0.06% sits above 2 kHz, so it
cooks at 4,000 Hz instead of 8,000, halving the RAM cache to 99,936 bytes for
no audible loss once the resampling is done properly.

Taking the other half would mean making it SPU resident, which needs about
119,000 bytes of SPU that is not free today. The two Ruins loops (77,584 bytes)
are cued in all 45 scenes at gain 16 of 16383, roughly -60 dB, so they are
inaudible on this disc and could be dropped until City of Tears ships; the two
gaps below ambience are another 16,912; the ring itself is 16,384. That
together with the 4 kHz clip fits, but it relocates every audio bank's base
address and changes the cue mask layout, so it is its own change rather than a
tail on this one.

Re-priced at build158 against the SPU map above, because the two numbers in
that paragraph were both wrong. SPU residency needs the clip's own 99,936
bytes, not 119,000, and the two gaps below ambience are 5,120 bytes, not
16,912. Against 8,032 contiguous free, 13,152 counting both gaps, and the
16,384 the ring gives back, a repacked SPU is still 70,400 bytes short. The
shortfall is almost exactly the two Ruins loops, so the trade has not moved:
99,936 bytes of main RAM for two source ambience channels, a recook of every
clip from the licensed install, and new base addresses for the Focus and
Runner banks. It is a content decision before it is a budget one, and nobody
has taken it.

## What bounded the P14 and P15 work (build 133)

Linked headroom was 113,636 bytes at build 133, down from 166,876 at build 107, and is
73,876 now. Three decisions this session were budget decisions, and
the number that decided each is worth keeping, because two of them look like
code problems and are not.

**The per-region texture budget, not RAM, is the binding constraint on art.**
`TEXTURE_BUDGET` is 416 CLUT slots per region. Adding the Knight's ability clips
to the per-region bank put the tightest region at 419 with all ten clips, 421
with only the four movement clips, and still 418 with three of them. Admitting
two Climbers whose recognizer already accepts them would put Crossroads_03 at
421. Both were abandoned rather than tuned, and the Climber projection is a
floor rather than an estimate: `similarity_dedup` protects every animation
frame, so none of a new actor's frames can be collapsed.

**The way past it is the Hollow Shade's route**, which costs linked RAM instead
of textures. 97 ability frames across 16 clips are 36,190 linked bytes and two
CLUT rows, taken from the fourteen the Shade reserved at y482 and never used.
That is why the ability animations ship and the Climbers do not: the abilities
are usable in every view, so they had somewhere else to live; a Climber belongs
to two specific scenes, so it does not.

**The charm board's icons took the same route, and the screen picked their
size.** Forty icons are 5,248 linked bytes at 16x16 across four resident CLUT
rows, and cost no per-view texture slot at all. The atlas route stayed refused,
now for two reasons rather than one: the tightest view in the world is the
figure that applies to a pause screen, and resident texels would also need
2,560 VRAM halfwords against the 304 `residency::SPARE_HALFWORDS` leaves
unclaimed. What settled 16 over 24 or 32 was neither budget. Six rows of
24-pixel icons are 144 scanlines on their own, and with the title, the notch
counter, the six-line description and the footer around them that is 281 of the
screen's 240; `host/charms.py::panel_layout` derives it and the cook refuses
rather than dropping a row or a description line to make a bigger icon fit.

**The scene arena is 421,128 bytes** and the ambience clip's
4 kHz recook freed the last large block. Taking the remaining SPU half still
needs the relocation described above, and nothing this session touched it.

## The coverage arena, and why it cannot share the scene arena (build 158)

`disc::COVERAGE_BUFFER` reserved 83,700 bytes because that was the largest
proof when the constant was written. The largest proof is 80,972 now, so
`host/hk-cook/src/scene_certificates.rs` reserves what the cook actually wrote and keeps
83,700 as `ARENA_CEILING`, the size a proof may not exceed. A ceiling nothing
re-reads goes stale silently; a reservation derived from the manifest beside it
cannot. That returns 2,728 bytes to linked BSS at the next cook.

Merging it into the scene arena was the obvious next question and the answer is
no, twice over. The proof is not admission-only: `render::prepare_back_prefix`
takes a fresh `Cache::coverage()` view every frame the camera moves and reads
the tile and group certificates and bitmaps straight out of the buffer, so its
bytes are live for as long as the scene's are. And the arena has no room. It is
417,748 bytes, which is Tutorial_01's 349,032-byte scene plus the 68,712-byte
world metadata bank plus a four-byte guard, so four bytes of tail survive the
scene that sizes both. An arena holding scene, bank and proof together would
have to be 498,716 bytes to delete an 83,700-byte buffer, a net 2,732 bytes
before the decoder's relocation margin eats into it. That is the same 2,728 the
stale ceiling gives up for nothing, so the risk buys nothing.

Splitting the proof by lifetime does not pay either. The pool map and the
group list are read once in `render::init`, the four certificate and bitmap
sections every frame, and dropping the admission-only pair from the resident
copy would save 6,932 bytes on the scene that sizes the buffer. They cannot be
consumed early, because `bind_tile_groups` needs the admitted `Room` and the
scene decode has already taken the arena by then.
