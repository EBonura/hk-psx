# Cooked formats: HKSCNE01 and HKROOM02

The current guest loads one compact HKSCNE02 scene bank at a time, exposing
106 region views across Tutorial_01 and Town. HKSCNE01 is the host's full bank;
HKROOM02 remains the per-region intermediate. HKOCSC01 stores each scene's
visibility certificates separately. See STATUS.md for actual-CUE evidence and
remaining gameplay gaps; formats alone establish neither source parity nor
whole-game capacity.

## Scene world metadata: HKWMTA01

P05 adds a separate per-scene metadata bank so world ownership can move out of
permanently linked Rust tables without changing the existing geometry payload.
The fixed 160-byte little-endian header contains magic, source scene ID, the
compact scene FNV, a 32-byte source metadata fingerprint, region count, total
length, flags, section count and four `(offset,count,stride)` descriptors. The
four aligned sections are region records (76 bytes), object/state records (36
bytes), polygon descriptors (8 bytes) and Q16 point records (8 bytes). Padding
must be zero and every descriptor uses its versioned stride; native Rust layout
is never serialized.

Region records retain strictly increasing, nonzero global chunk IDs,
activation/collision/camera bounds,
global-neighbour spans, object spans and polygon spans. Object records carry a
source identity, stable state ID, kind/ownership flags, bounds and polygon
spans. Neighbours are global IDs and therefore are not incorrectly compared
with the local region count of the owning scene. The allocation-free reader
`hk_format::WorldMeta` validates all arithmetic, section bounds, alignment,
object and polygon spans, 3..16-vertex polygon limits and nonzero global
neighbours before exposing `WorldRegion`, `WorldObject`, `WorldPolygon` and
bounded iterators tied to the immutable bank lifetime.

The current host cook measures the first two banks at 32,072 bytes (Tutorial,
86 regions) and 2,844 bytes (Town, 20 regions). These are measured payloads,
not an approved final arena size. Actor clip/effect banks, auxiliary Geo,
dialogue, Great Door, render-budget and disc mappings remain in their existing
tables until later P05 migration slices prove differential equivalence.

## Scene visibility: HKOCSC01

Little-endian scalars; four-byte-aligned sections, with all padding zero.
The80-byte header contains magic8, scene ID u32, compact-scene raw FNV u32,
atlas inventory FNV u32, draw-pool count u32, grid shift u32 (2), reserved u32
(0), then six (byte offset u32, element count u32) descriptors. Sections occur
in canonical order: tile certificates, tile bitmap words, pool-to-certificate
IDs, group certificates, group bitmap words, groups. A certificate is12 bytes:
gx i16, gy i16, width u16, height u16, bitmap BIT offset u32. A group is10 bytes:
certificate u16, four ordered draw-pool member IDs u16.65535 is the empty ID.
Map IDs and bitmap offsets relocate; source geometry and bitmap words do not.

Atlas FNV-1a consumes each owner's ordered tuple of LE u32 values: kind, first,
count, raw byte length, raw FNV. Stored/raw chunk checksums and header identities
bind proofs to geometry and atlases. Incremental validation rejects malformed
ranges, padding, bit tails, map IDs and group members before exposing typed
borrowed slices. Admission stages the data through empty scene scratch, copies
it into a reusable84KiB bank, then loads atlases and geometry. Rendering requires
matching owners for all three; no view survives mutable scene replacement.

## Resident scene: HKSCNE01

All scalars are little-endian. Sections start on four-byte boundaries; intervening
and final alignment bytes must be zero. The 128-byte header is:

| Offset | Bytes | Meaning |
| --- | ---: | --- |
| 0 | 8 | `HKSCNE01` |
| 8 | 4 | Original scene ID |
| 12 | 36 | Nine u32 values: rooms, textures, draw pool, frame pool, clip pool, edge pool, palettes, pages, tail bytes |
| 48 | 4 | Exact total byte length |
| 52 | 4 | Required `HAS_ALPHA_COVERS = 1` |
| 56 | 8 | Reserved zero |
| 64 | 40 | Ten absolute u32 section offsets: textures, draws, frames, clips, edges, palettes, pages, tail, room descriptors, local references |
| 104 | 24 | Reserved zero |

Global texture/draw/frame/clip/edge records retain the sizes and field encodings
listed below for HKROOM02. Identical records and palettes may be shared. Texture
and palette IDs are global within a scene; different scenes have independent ID
namespaces. Indexed pixel planes may be shared by textures with different
palettes without changing their final sampled colour words.

Each 40-byte room descriptor contains a nonzero unique original region chunk ID,
four u32 local counts (draws, frames, clips, edges), four absolute u32 offsets to
its reference arrays, and a reserved zero u32. The four arrays hold u16 indices
into the corresponding global pools. Arrays occur in descriptor/section order
with checked zero alignment. Local ordering remains unchanged: a clip's frame
range still addresses that room's local frame array, while each frame's texture
ID addresses the global texture table. Particle/debris frame indices in derived
world metadata therefore remain local and need no extra rebasing.

Scene limits are 128 room descriptors, 2,048 textures, 65,535 records in each
global draw/frame/clip/edge pool, 1,536 palettes, 20 pages and 256 KiB of tail.
Palettes may be fewer than textures. Local room limits remain 1,024 draws, 2,048
frames, 128 clips and 1,024 edges. These format limits are independent of the
combined runtime RAM/VRAM admission limits.

`SceneValidation::new(bytes)` checks the fixed header and section layout.
`step(bytes, work_budget)` checks at most that many records, comparisons, layouts
or transitions and returns completion without publishing a borrowed Scene.
Duplicate chunk-ID comparisons also consume individual units. Budget zero makes
no progress; length is checked every call, including after completion. The
private arena must remain unchanged across calls and subsequent admitted views.
The loader keeps both stored/raw checksums and uses eight-unit validation steps.
`Scene::parse` uses the same cursor; `Scene::validated_view` requires previously
completed validation of identical immutable bytes.

After admission, generated `REGION_SCENE_LOCAL` indices select
`Scene::room(index)` directly, without scanning scene descriptors or rerunning
validation. Room draw/frame/clip/edge access resolves one local u16 reference;
texture/page/palette access uses shared storage. `Room::byte_len()` describes the
backing scene length for a resident view, not a separate regional payload.

## Per-region host intermediate: HKROOM02

All integers are explicitly little-endian. The allocation-free reader validates
exact length, counts, references, texture-page bounds, animation ranges and
coordinate limits before upload. No native struct casts or alignment assumptions
are used. Regions occur in the following order, without padding between tables.

| Region | Record size | Meaning |
| --- | ---: | --- |
| Header | 40 | Eight-byte magic, six u32 counts (static pages, textures, draws, frames, clips, edges), u32 tail byte count at offset 32, u32 feature flags at offset 36 |
| Texture | 16 | Six u16 (page, u, v, original width, original height, palette index), then u32 tail offset (animation pixels or optional static alpha-cover record) |
| Draw | 44 | u16 texture, u16 foreground, u32 Q12 pixel scale, eight i32 preprojected Q8 vertex coordinates, RGB tint bytes, material byte (0 legacy / 1 admitted black source-alpha Average) |
| Frame | 20 | u32 texture, four i32 Q16 local bounds (xmin, ymin, xmax, ymax) |
| Clip | 16 | u32 start, count, Q16 fps, packed wrap mode (low 16) and loopStart (high 16) |
| Edge | 16 | Four i32 Q16 world coordinates, including source slopes |
| Palettes | 32 each | 16 BGR555/STP entries; colour word zero is transparent; the block holds one record per texture, and records may repeat |
| Static pages | 32768 each | 256x256 4bpp texels, low nibble first |
| Tail | Header byte count | Packed 4bpp animation rows and alignment padding, then optional static alpha-cover records |

HKROOM02 limits are 20 static pages, 640 texture records and the palette block
that matches them, 1,024 draws, 2,048 frames, 128 clips, 1,024 edges and 256 KiB
of combined animation/alpha-cover tail.
At least one texture is required; zero static pages are allowed. Indices,
coordinate ranges and division bounds are checked. Clip modes 0/1/2 mean
Loop/LoopSection/Once. Knight clips preserve loop sections and authored fps;
frame events remain in the local intermediate report and are not generally
executed. Current source cooking admits up to five pages, 640 texture records
and 416 CLUT slots per intermediate region, with a 384 KiB host ceiling. These
are input-cook limits, not independent runtime slots in the shared-scene build.

## What a CLUT slot costs, and what it does not

A slot is a distinct palette, not a texture record. The block holds one 32-byte
entry per texture because that is how the reader sizes the section, but nothing
requires the entries to differ and nothing uploads that block: `host/scene_bank.py`
pools palettes by value while building the scene bank, `compact_resident` strips
the whole palette section out of HKSCNE02, and `bootstrap_atlases` uploads the
scene's pooled words. A region's slot cost is therefore the number of distinct
words its textures name, checked against the 416-slot budget, while the record
count is checked separately against the 640 the table can address.

The two used to be the same number, so counting textures was right until frames
began tiling. `Atlas.add_tiled` quantizes a whole frame once, which is what keeps
colours identical across a tile seam, so every tile of a frame carries
byte-identical palette words. Measured on the False Knight's whole clip set: 110
unique sprites cook to 499 tiles carrying 499 palette entries and **107 distinct
palette values**, so counting records overstated its slot cost by 4.7x. Measured
across all 699 cooked regions, 154,615 texture records carry 151,208 distinct
palettes: 3,407 slots, 2.2%, a median of 3 per view, and the tightest view in the
world (Tutorial_01 chunk 47) goes from 3 free slots to 14.

A separate palette count in the header was measured and rejected. It would need
a new magic, a 44-byte header, and a recook of every pack, and the only thing it
buys over counting distinct values is a shorter block in the host's own
intermediate files: 3,407 entries, 109,024 bytes, none of it resident, linked or
in VRAM, because the resident bank carries no palette section at all. The
accounting is where the saving is; the storage is not.

The renderer reserves 248 dynamic packet positions within its fixed 1,032-packet
capacity: player/nail/actors, bounded impacts/debris, and up to 128 source-derived
particles and 80 Geo quads. Mandatory scenery and optional scissor expansion retain separate
checked budgets. Texture/frame limits alone do not establish RAM or packet fit.

## Static and streamed textures

A static texture has a page index below the static-page count. With legacy
feature flags zero, its tail offset must be zero. With `HAS_ALPHA_COVERS`, the
offset addresses its alpha-cover record instead. Its UV rectangle must fit
inside its 256x256 page. Width
and height are nonzero; widened arithmetic rejects overflowing coordinates.
Scenery draw records must reference static textures.

A streamed texture has page `65535`, u/v both zero, and nonzero width/height no
larger than 64x64. Its offset is relative to the beginning of the tail and must
be divisible by four. A row occupies `((width + 3) & ~3) / 2` bytes: the texel
width is rounded up to four for a complete VRAM word. The stored texture width
remains the original image width. Right-hand padding is transparent zero and
must not expand the rendered UV range or shrink the sprite inside its geometry.
The image byte count is this row stride times its original height. Each complete
image range must fit in the bank; gaps and final padding align offsets to four
bytes. `Room::stream_pixels` returns the image rows without alignment bytes after
the image; `Room::stream_byte_count` includes the whole tail, including any
alpha-cover metadata.

Padding is added after quantization. Moving frames from static pages into this
bank preserves every original quantized texel, palette, image dimension and
frame bound. The current user-authorized scenery reduction separately resamples native static
SpriteRenderers to a 48-pixel longest axis; it preserves source quad geometry and
layers. Knight, nail, enemy and HUD sampling remains unchanged. Palettes
remain immutable in their shared scene VRAM ranges; animation streaming replaces
texels only. This avoids palette replacement and CLUT-cache invalidation during
animation. Texture-cache invalidation is still required after slot replacement.

## Optional static alpha covers

Header offset 36 contains a little-endian feature mask. Bit 0 is
`HAS_ALPHA_COVERS = 1`; zero retains the legacy HKROOM02 encoding. Any other
set bit is rejected. Fixed table sizes and section offsets remain unchanged.
Legacy readers reject the feature flag rather than misinterpreting static
texture offsets. Updated readers accept both encodings.

When enabled, the cooker appends one 20-byte record for every canonical static
texture, in texture-index order, after all animation images and their final
four-byte alignment padding. Each static texture's `stream_offset` points to
its record relative to the tail start. The offset is divisible by four. Streamed
textures continue to use that field for image pixels and have no alpha cover.
Records contain no texture or palette data:

| Record offset | Bytes | Meaning |
| --- | ---: | --- |
| 0 | 1 | Rectangle count, 0 through 4 |
| 1 | 3 | Reserved, all zero |
| 4 | 16 | Four slots of `[x, y, width_minus_one, height_minus_one]`, each component u8 |

Coordinates are local to the texture. Actual width/height are the stored byte
plus one, so a full 256x256 image is representable. Rectangles use exclusive
right/bottom edges and must be mutually disjoint; touching edges are allowed.
Unused rectangle slots are all zero. Count zero describes no visible texels.

`host/alpha_covers.py` derives support from the final indexed texels and all
16 PSX palette words. A sampled colour word of zero is transparent, regardless
of its palette index; `0x8000` is visible black. It excludes transfer padding.
The deterministic greedy guillotine algorithm starts with a tight support box,
then repeatedly selects the split with the largest positive area saving, up to
four rectangles. Ties retain rectangle, axis and cut iteration order. Host
assertions check disjointness and coverage of every visible texel. A cover may
include transparent texels; it may never omit a visible one. This adds metadata
without resampling, editing palettes, cropping source quads or changing layers.

`Room::alpha_cover(texture)` returns `Option<&[u8]>` containing the complete
20-byte record; legacy rooms and streamed textures return `None`. The shared
validator checks aligned/in-range offsets, count, reserved/unused zeros,
texture-relative rectangle bounds and all rectangle pairs. This remains bounded
work within one texture-validation step, with at most four rectangles and six
pair checks. The guest does not scan image pixels. Full parsing and resumable
validation use the same record checks, and no room is published before all
metadata validates. Coverage correctness is established by the host cooker.

Exact texture deduplication, scene actor-bank append and lossless delta
reconstruction regenerate the same canonical records. Texture request mappings
and source provenance remain available. Cook reports distinguish
`animation_bytes`, `alpha_cover_bytes` and their combined `stream_bytes`;
`format_features` records the enabled flag. The cook cache includes the cover
algorithm's source hash.

The historical alpha-cover preservation milestone contained 12,762 static texture records:
255,240 raw alpha-cover bytes, ranging from 220 to 5,020 bytes per region. Its
largest complete raw room was 238,024 bytes (chunk 47), leaving 24,120 bytes within
its actual 256 KiB arena. Raw size alone does not establish in-place decode
safety; the packer separately checks both decoder implementations at that arena
size. This is not a compressed-disc size or runtime performance result.

At that historical milestone, all eleven preserved reference regions matched the actor-appended packs in
every texture's dimensions, palette words and texels; complete static pages,
palette tables, draw/frame/clip/edge bytes and animation-bank bytes were also
identical. Only the feature header, static record offsets and appended cover
records differed. The hash-bound local evidence is
`.hkpsx/alpha-cover-final-preservation/report.json`. Synthetic host
coverage/preservation tests and malformed-record/full-versus-incremental reader
tests pass. Final executable, rendering and frame-pacing validation remain
separate gates; these preservation checks do not establish runtime improvement.

## Animation and shared scenery VRAM residency

Twenty-four 64x64 4bpp animation slots reserve 48 KiB: eight in the strip at
x320 and sixteen in the texture page static scenery page 19 gave up. At most
twenty-four unique textures are requested per frame, and a frame larger than
one slot binds a rectangle of up to twenty of them. A miss transfers at most
2 KiB, giving a 48 KiB worst-case upload; a frame only uploads its misses.
Requested slots are pinned before eviction, and prior DMA/GPU completion is
required before replacement. The 2,048-key cache survives same-scene region activation; actual
scene changes reset it before another scene's global IDs are reused. Its frames
come from resident scene RAM, without animation-time CD seeks.

A scene's static pages and palettes are loaded before gameplay. Under
scene-gate residency every scene owns page and palette base 0 exclusively, so
the reservation holds the largest scene rather than a sum: measured over the 45
cooked scenes, that is Tutorial_01 at 18 pages and 1,087 CLUTs, against 19
pages and 1,248 CLUTs reserved. Texture templates use the scene's page/palette base plus the texture's
local global index. Region activation rebuilds local draw state only; scene
changes rebuild global texture templates. All scene pixels remain resident.

Coordinates below are VRAM halfwords, not 4bpp texels. The four legacy bank
coordinate groups now address disjoint shared ranges, without region eviction:

| Use | Allocation |
| --- | --- |
| Two 320x240 framebuffers | x=0..319, y=0..479 |
| Animation slots | x=320..351, y=0..255 and x=960..1023, y=256..511 |
| HUD icon texels | x=352..383, y=0..31, within assigned icon slots |
| HUD palettes | x=352..367, y=480 and y=481 |
| Reveal-mask fade CLUT | x=320..335, y=480 |
| Dialogue font CLUT | x=320..335, y=481 |
| Dialogue font texels | x=0..319, y=500..511 |
| CLUT group 0, entries 0..399 | x=0..319, y=480..499, twenty CLUTs per row |
| CLUT group 0, entries 400..415 | x=336..351, y=480..495 |
| CLUT group 1, 416 entries | x=320..383, y=256..359, four CLUTs per row |
| CLUT group 2, 416 entries | x=320..383, y=360..463, four CLUTs per row |
| Geo texels/CLUT allocation | (320,464,64,16), (352,240,32,16), (352,485,32,27), (320,496,32,16), (352,32,32,64); x/y/w/h in halfwords |
| Static physical page p=0..18 | x=384+(p%10)*64, y=(p/10)*256; each 64 words by 256 rows |

Global physical palette index p maps to group `p/416` and entry `p%416`; scene
texture palette IDs remain independent of texture count. Under scene-gate
residency every scene's page and palette base is 0. Native allocation checks
cover the nineteen reserved pages, 1,248 admitted CLUT positions and disjoint
Geo/UI/animation areas; the twentieth page slot is the animation cache's second
region and the same checks prove nothing else writes it. The unused fourth CLUT group is no longer reserved for scenery; admission
rejects totals above 1,248, and Geo reuses part of that area. Actual EXE memory and emulator/hardware checks remain
separate from these coordinate proofs.

## Disc and scene admission

`host/pack_scenes.py` turns the complete 98-region HKROOM02 snapshot into two
lossless HKSCNE01 payloads and `data/scene_manifest.rs`. WORLD.PAK uses the pinned
SDK's `PSOXWPAK` container: a 28-byte header and 24-byte entries padded to sectors.
The current eight-entry table fits one 2,048-byte sector: scene chunks 1–2,
then six ambience chunks. IDs/extents/counts/checksums are validated against the
generated manifests. The SDK provides the world-pack LBA; no guest filesystem
is required.

Chunk ids number payloads by kind (scenes, ambience clips, atlases, Focus,
coverage, world metadata, effect art, audio banks), but since 2026-09-23 the
payloads are placed in read order, through mkisopsx's `--world-pack-order-file`:
bootstrap's banks and clips first, then one contiguous group per manifest scene
holding its coverage, effect art, atlases, scene and world metadata, in the
order a gate reads them. `host/build_guest.py` `disc_order` writes that order and
`game/src/disc.rs` `member` walks the same one, refusing a directory that differs.
Filed by kind, a gate load paid five long seeks; grouped it pays one. Measured
in the emulator on the Town to Crossroads_01 gate: 3.55 s to 2.62 s of CD time.

A compressed payload is `HLZC`, little-endian u32 raw length, then a raw LZ4 block.
Compression is used only when the framed payload is smaller than the raw scene.
Each scene descriptor binds source scene ID, stored/raw byte counts and FNV-1a
checksums, RAM offset, available decode suffix, page base and palette base.
Sector padding is excluded from payload hashes. FNV is an integrity check, not
cryptographic authentication; host provenance separately records SHA-256.

One aligned arena reserves 1,179,648 bytes. Tutorial decodes first into its
929,692-byte prefix; Town uses the remaining 249,956-byte suffix and retains
116,756 decoded bytes. Total resident data is 1,046,448 bytes. Compressed input
relocates to the active suffix's tail, with overlap checks preventing output
from overtaking unread input. The host verifies the exact sequential allocation
against the pinned SDK and resumable decoder at budgets including one byte.
Earlier admitted prefixes stay unchanged, including on retries after a later
scene fails. Raw totals alone are not proof of in-place decoding safety.

At startup, the existing CD IRQ service transfers one payload at a time and
chains to the SDK's VBlank/fault handler. Foreground checksum, relocation and
decode work is bounded, followed by incremental scene validation. Six complete
ambience clips first reuse the empty arena for verified SPU uploads. Both scenes
are then admitted, their disjoint VRAM ranges uploaded, and gameplay starts.
A failed admission remains in the menu retry path; a partial scene is never
published. Subsequent region selection is an immutable view change with no CD
transfer, decompression or static VRAM replacement.

Native checks cover both actual scene banks and 96 same-scene consecutive
region transitions (85 Tutorial plus 11 Town), with shared animation frames
hitting their existing slots without uploads. Crossing between the two scenes
resets the animation cache and is not a reuse case. The final EXE/CUE long replay is recorded in STATUS.md, with zero loading waits
but remaining frame-budget failures. Old five-arena neighbour-loader timings
are historical and do not describe this implementation.

Raw banks, compression reports and generated manifests are ignored build inputs.
Only the canonical BIN/CUE is delivered to `~/Downloads/ps1 games/`; temporary
disc images must also stay in that library. See [STREAMING.md](STREAMING.md) for
source inventories, historical work and remaining whole-game limitations.

## Derived world metadata

`data/regions.json` retains source file:pathID identities, activation/camera/
collision ranges, per-region counts and pack hashes, owned collider indices,
intact/remnant draw indices, actor clip references, hazards and authored hazard
checkpoint links. Per-region `scene.json` records retain the base cook's atlas entries, draws,
frames, clips and source sprite references; `unsupported.json` retains unresolved
records. These base reports precede the actor-bank append. Final pack hashes,
texture counts and adjusted actor clip references are in `regions.json`; its
`base_path` and `base_sha256` identify the unchanged base pack. Source scene
inventories and cooked region inventories answer different residency questions.

`host/world.py` emits bounded `data/regions.rs` tables consumed alongside the
binary scene views. They include gate destinations, region neighbours, exact hit and
hazard polygons, checkpoint positions and persistent-object flags. Current
session state provides 128 breakable bits and 1024 grass bits per scene across
two scenes. Renderer visibility and gain are reconstructed when a region loads.
Source persistent breakables survive grid changes and scene resets; ordinary
props and grass reset on scene reset. This is session persistence, not a memory
card save or the retail bench/progression system. Source reveal controllers use
separately tracked opacity and the admitted binary-black palette path; intermediate PS1 blending remains an approximation
of the original continuous alpha fade. Independent
remote mask bindings retain the original owner state/clock when a renderer is
visible in a region that does not contain its owning door collider.

`data/params.rs` carries source-derived movement, nail, recoil and vital values;
actor and pogo metadata are also generated separately. Exact polygon tests use
bounded Q16 coordinates and i64 cross products. The gameplay subsets do not
constitute a general PlayMaker VM or complete original-game behavior. Incompatible changes
to fixed binary tables require a new magic and matching reader/cooker. Optional
features may use explicitly checked header bits while retaining those tables.

## Boot-only menu: HKMENU03

The separate menu pack remains embedded in the executable so its title and load
status are available before reading room data from CD. Its fixed layout is:

| Region | Bytes | Meaning |
| --- | ---: | --- |
| Header | 16 | `<8sHHI`: magic, width320, height240, total pixel bytes89088 |
| Palette | 512 | 256 RGB555 entries; opaque black uses `0x8000` |
| Left title tile | 61440 | 256x240 8bpp indices |
| Right title tile | 15360 | 64x240 8bpp indices |
| Loading patch | 6144 | 192x32 8bpp indices |
| Retry patch | 6144 | 192x32 8bpp indices |
| Font advances | 96 | Original Perpetua glyph advances, starting at ASCII32 |

Total pack size is 89,712 bytes; temporary texture/palette residency is 89,600
VRAM bytes. The guest checks magic, dimensions, pixel count and exact size before
upload. Both patches use the already selected title palette, and replace the
screen rectangle at (64,177) without changing the background or logo. The first
shows `LOADING...`; the second shows `DISC ERROR - START TO RETRY`. A retry requires
releasing and pressing Start or Cross. Room upload reuses this menu texture space
once loading succeeds.

The title is a static composition of source logo, background and font, with a
guest-controlled brightness fade and these status patches. It is not a flattened
gameplay room or the full retail menu state machine. Animated menu scenery, music
remain unfinished. Main, options and controls pages are implemented with the
existing resident dialogue font; volume choices apply to active voices and
persist for this run. First Start/Cross still starts the game.

## Resident Geo assets

`host/geo.py` emits `data/geo.hk` and typed Rust upload/art/clip/source tables.
This is a build-paired embedded bank, not a separate disc ABI. Its 5,154 bytes
contain 50 unique 4bpp textures and one 15-colour palette (index zero transparent).
57 frame references cover all 47 original coin Idle/Air frames and five intact
plus five depleted rock references. The five reserved rectangles total 9,920
bytes. Upload ranges and UV/CLUT bounds are checked before gameplay; no mining
or pickup event reads the CD or allocates another VRAM slot.

`host/hk-cook/src/geo_audio.rs` emits six complete 11,025Hz mono ADPCM clips plus descriptors.
The 13,904-byte resident bank occupies SPU `0x7c3c0..0x7fa10`. Voices 12–14
serve pickup, hit and destruction; destruction reuses the existing first source
clip. The second original destruction variant does not fit and remains omitted.
