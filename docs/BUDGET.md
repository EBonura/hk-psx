# Shared scene and gameplay budget

The normal build on2026-09-09 holds both currently authored scenes in one RAM
arena and persistent VRAM atlases. These are linked reservations, not hardware
performance or main-stack high-water measurements. Exact map and artifact hashes
are in `.hkpsx/build.json`; every build replaces the sole playable disc.

| Main RAM allocation | Bytes |
| --- | ---: |
| Linked code |334,608|
| Code-to-data alignment |0|
| Linked data, including menu, HUD and sound bank |267,504|
| BSS, including shared scene arena and runtime pools |1,360,396|
| Total static span |1,962,508|
| BIOS/kernel reservation |65,536|
| Linker stack exclusion |32,768|
| Space above initial stack pointer |256|
| Unallocated gap before reserved stack |36,084|

Static data ends at `0x801ef20c`; the reserved stack begins at `0x801f7f00`.
There is no heap. The single scene arena occupies1,179,648 bytes, replacing five
256KiB room slots (1,310,720 bytes). Renderer template/cover arrays and animation
key lookup now support2,048 scene-global texture IDs. Geo logic, resident art/audio, functional menus, break-effect and Lifeblood
tables increase the static span; the pinned Town/Tutorial pack and its
regenerated tilemap fills changed the data size.
The remaining unallocated gap is not a measured safe stack allowance; profile
main-stack high-water and reclaim RAM before substantially expanding content.

| Scene | Raw bytes | Stored bytes | RAM offset | Decoder suffix capacity |
| --- | ---: | ---: | ---: | ---: |
| Tutorial |929,692|452,831|0|1,179,648|
| Town entrance |116,756|46,395|929,692|249,956|

Raw resident data totals1,046,448 bytes. Remaining arena space is133,200 bytes;
it is part of BSS and is not additional unallocated heap/stack space. The build
checks exact sequential decoding through pinned SDK and incremental decoders,
including budgets1,31,1024,2048,8192. Each later read/decode uses a private suffix
and preserves the admitted prefix. Sector padding, FNV hashes, scene IDs, local
references and all serialized region geometry bounds pass before packaging.

The renderer retains1,024 local scenery records and1,032 draw packets; global
texture templates and alpha covers have2,048 slots. The dedicated2,048-byte CD
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

Tutorial uses18 pages and1,065 palettes; Town entrance uses2 pages and141
palettes. All20 pages are populated. The1,206 admitted palettes leave42 unused
CLUT slots inside the reservation. Scene page/palette bases are(0,0) and(18,1065).
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
eight admitted door-art variants now share these same budgets. The dynamic draw reserve is248 packets: up to8 Knight/nail/enemy packets,
32 combined fragment/grass-effect packets and128 source-particle packets, plus80 Geo quads. The28 persistent fragments do not require new VRAM banks.

The two shared scene payloads total499,226 stored bytes. The original98
HKROOM02 files remain host inputs and geometry proofs, not separate disc chunks.
WORLD.PAK begins at LBA1024; the8-entry table binds2scenes and6ambience clips in
one header sector. Exact source, raw, stored and decoder code hashes are in
`.hkpsx/packed-scenes.json` and the build/replay reports.

The title reads ambience through the empty shared arena, uploads it to SPU, then
loads/validates both scenes and uploads their atlases. Static VRAM upload work
uses64KiB slices before play; all pages/CLUTs then remain immutable. During the
long replay CDsectors stay481 and staticVRAM upload bytes stay693,952 throughout
the measured route. Eight animation slots retain an independent16KiB maximum per
frame; this replay's measured maximum is1,942 bytes. Animation frames come from
resident RAM, with no traversal CD reads or decoding. This does not establish
full-game residency or hardware timing.

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
