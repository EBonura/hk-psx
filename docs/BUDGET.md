# Shared scene and gameplay budget

The validated63 cheat-menu build from 2026-09-15 holds both currently authored scenes in one RAM
arena and persistent VRAM atlases. These are linked reservations, not hardware
performance or main-stack high-water measurements. Exact map and artifact hashes
are in `.hkpsx/build.json`; every build replaces the sole playable disc.

| Main RAM allocation | Bytes |
| --- | ---: |
| Linked code |340,680|
| Code-to-data alignment |8|
| Linked data, including menu, HUD and sound bank |771,376|
| BSS, including shared scene arena and runtime pools |567,812|
| Total static span |1,679,876|
| BIOS/kernel reservation |65,536|
| Linker stack exclusion |49,152|
| Space above initial stack pointer |256|
| Unallocated gap before reserved stack |302,332|

Static data ends at `0x801aa204`; the reserved stack begins at `0x801f3f00`.
The cheat HUD has nine red and22 blue packet slots, reusing existing art.
There is no heap. The single scene arena occupies369,180 bytes, replacing five
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
The current main frame is 32,120 bytes, leaving 17,032 reserved bytes for nested
calls and other stack use; this is **not** an observed high-water measurement.
The linker also excludes the 256 bytes above its initial SP from static RAM.
Evidence: `.hkpsx/stack-budget-2026-09-14.json`.

Particle lifetime uses a nonzero representation, eliminating the separate
Option tag: the 128-slot pool shrinks from 6,668 to 6,156 bytes before the four-byte cached active count without changing
counts or trajectories. This saves 512 bytes in the stack-resident world state;
it is not 512 bytes reclaimed from BSS. Compiler frame layout also changed, so
do not attribute the entire main-frame reduction to those 512 bytes alone.

Exact particle tracks add399,432 bytes plus142 bytes of emitter bases. Each of
1,513 source-seeded particles has33 two-word phases. The live pool adds256 bytes
for128 track IDs, making it6,416 bytes; Particle remains48 bytes. All current
emitters fit, while unpackable future emitters retain scalar evaluation.
These are main-RAM values, not extra VRAM/SPU data or traversal transfers.
The main frame increased beyond the pool's256 bytes because compiler-generated
constructor temporaries share its lifetime; do not equate pool and frame growth.
Pass56 reduced main to18,168 bytes by outlining construction, but regressed frame
cadence and was reverted. Its14,624-byte constructor still held intermediate
copies. The48KiB reservation and high-water caveat apply to the retained build.

| Scene | Raw bytes | Stored bytes | RAM offset | Decoder suffix capacity |
| --- | ---: | ---: | ---: | ---: |
| Tutorial |321,956|181,163|0|369,180|
| Town entrance |47,216|25,355|321,956|47,224|

HKSCNE02 resident data omits the static page/palette payloads already held in
VRAM. Runtime records, animation data and alpha maps remain resident and exact.
Raw data totals369,172 bytes; the arena retains8 bytes beyond this and4 beyond
the proven369,176-byte aligned minimum. Startup-only atlas chunks decode/upload
through the same empty arena before scene views are admitted. Host analysis
keeps the original HKSCNE01 records/payloads separately; these are not on-disc
resident scene copies. The native sequence proof exercises all23 atlas chunks,
both scene decoders, budgets1,31,1024,2048,8192 and immutable admitted prefixes.
The largest ambience startup read is200,704 bytes and fits this same buffer.

The renderer uses948 bytes of CPU-only scratchpad. An888-byte draw workspace
(three32-piece partitions, eight occluders and union bounds) overlays the
600-byte owner grid used earlier during coverage collection;60 row-mask bytes
remain disjoint. Explicit phase initialization
ends owner borrows before draw access. These are never DMA sources or stack
storage;76 bytes remain. Row retirement events use2,760 bytes instead of1,560:
one source-row mask replaces repeated per-cell retirements. Packed texture
classification adds320 aligned bytes and replaces runtime atlas inspection.
The linked data above includes84,284 bytes of host-certified4px coverage tables
(62,160 bitmap,15,708 descriptors,6,376 shared draw lookup,40 slice overhead).
All1309 certificates cover immutable original poses; no new VRAM is allocated.

The renderer retains 480 local scenery records and 1,032 draw packets; global
texture templates and alpha covers have 1,120 slots. The dedicated2,048-byte CD
IRQ stack and136-byte context are included in BSS. An observed minimum SP across
both stack allocations is not main-stack consumption; high-water remains unknown.

| Gameplay VRAM reservation | Bytes |
| --- | ---: |
| Two 320×240 16-bit framebuffers | 307,200 |
| Twenty persistent 256×256 4bpp scenery pages | 655,360 |
| Eight 64×64 4bpp animation slots | 16,384 |
| Physical capacity for 1,248 texture CLUTs | 39,936 |
| Health and SOUL HUD rectangles | 2,208 |
| Shared black-mask fade CLUT | 32 |
| Original dialogue font strips and CLUT | 7,712 |
| Geo texels and CLUT strips | 8,384 |
| Lifeblood cocoon, Scuttler and blue-mask texels with CLUTs | 5,120 |
| Breakable-effect texels and CLUTs | 5,632 |
| Total | 1,047,968 |
| Unallocated within 1 MiB | 608 |

Tutorial uses 18 pages and 1,068 palettes; Town entrance uses 2 pages and 141
palettes. All 20 pages are populated. The 1,209 admitted palettes leave 39 unused
CLUT slots inside the reservation. Scene page/palette bases are (0,0) and (18,1068).
The old bank-coordinate helpers now map global page and palette indices; those
coordinate groups are not runtime eviction units. Coordinates are VRAM halfwords:

- Framebuffers: x0..319, y0..479. Animation slots: x320..351, y0..255.
- Health texels: x352..355, y0..31; CLUTs x352..367, y480..481.
- SOUL texel reservation: x356..383, y0..31; CLUTs x352..367, y482..484.
- Dialogue: texel strips x0..319,y500..511; CLUT x320..335,y481.
- Black-mask fade CLUT: x320..335,y480.
- Static page `p=bank*5+page`: `(384+(p%10)*64, (p/10)*256)`.
- Bank0 CLUTs: first400 at x0..319,y480..499; last16 at x336..351,y480..495.
- Bank1: x320..383,y256..359. Bank2: x320..383,y360..463.
- The former bank3 CLUT area is no longer reserved for scenery.
- Geo: (320,464,64,16), (352,240,32,16), (352,485,32,27),
  (320,496,32,16), (352,32,8,32), (352,64,32,32), as x/y/w/h halfwords.
- Lifeblood: (352,96,32,80). Break effects: (352,176,32,64), (360,32,24,32);
  the second was carved out of Geo's former (352,32,32,64) strip.

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
WORLD.PAK begins at LBA1024; its31-entry table binds2 scenes,6 ambience clips
and23 atlas chunks. Original HKROOM02/full HKSCNE01 files remain host analysis
inputs. Raw/stored/source/decoder hashes are in the packed-scene/build reports.

The title loads ambience through the empty arena into SPU, then validates and
uploads atlas chunks synchronously before admitting compact scenes. No atlas
buffer is reused before draw_sync completes. Retry revokes admission before
menu artwork overwrites VRAM, ensuring all pages are restored on the next start.
Pass49 startup reads509 sectors; static uploads total694,048 bytes with a32KiB
maximum chunk. Both counters stay unchanged throughout the long gameplay route.
Eight animation slots retain an independent16KiB maximum per frame. Animation
frames come from resident RAM, with no traversal CD reads or decoding. This is
the current two-scene subset, not full-game residency or hardware timing.

Six original effects occupy 31,984 SPU bytes at `0x1010..0x8D00`, voices0..5.
The SDK silent loop is at0x1000. Five effects use22,050Hz mono; damage uses11,025Hz.
They are uploaded once and trigger without CD access. Six complete8kHz mono
ambience loops add472,768 SPU bytes at `0x8D00..0x7C3C0`, voices6..11, leaving
15,424 bytes above the ambience bank. Geo pickup/hit clips occupy13,904 of
those bytes at `0x7C3C0..0x7FA10`, voices12..14, leaving1,520 bytes. All six
Geo clips are complete11,025Hz mono samples; rock destruction reuses the existing
first destruction variant. The second source variant remains omitted.
Ambience loops load through the empty shared scene arena at
startup, adding no staging buffer to main RAM and no traversal CD traffic.
The disc adds235 payload sectors. Full death layers and longer music transport
remain pending; see FOCUS.md and MUSIC.md. Physical CD latency, audio quality
and frame cadence still require hardware evidence.

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
