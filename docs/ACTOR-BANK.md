# Actor art banks: any actor family, scoped by data

Status 2026-10-31. The mechanism of docs/BOSS-ART.md (sprites cut into parts of at most 64x64 texels, shared palettes, a bank coder,
decoded into the animation cache when a frame first needs them), generalised from one boss to the ordinary actors of any scene.
The first user is the three pre-battle husks of the False Knight arena (Crossroads_10); the first test scene is Crossroads_01's
two Zombie Runners.

## Why

The room path cooks every frame of every clip an actor plays as tiles of the view's texture table, and a view has 640 records,
256 KiB of animation tail and 416 CLUT slots. An actor family with many clips (three husks with all their clips: 99 sprites)
does not fit the arena views next to the False Knight. The bank pays in RAM only what the family's coded art costs, outside the
view's tables.

## What it is

- One **bank** per scene: a `hk_format::boss::Bank` blob (header, clip/sequence/sprite/part tables, then the parts' coded
  texels, live clips first and the corpse clips last) with one set of up to 16 shared palettes. It is cut by byte into two
  memory pieces that the guest reads as one string (`actor_bank::Pieces`): the **head** is a data package in the module
  pool (`art_*` in `DATA_MODULES`, carried by the scene like `art_goam`: read at the gate, resident until the next scene
  admission, read-only, `modules::bytes`), and **the rest** is read-only data linked into the image
  (`data/actor_banks.rs`). Both cost RAM from the same pot, the free RAM below the pool; how much goes in the pool is
  `package_bytes`, a number tuned against the room's pool slack (the pool gate prints it). With the coder below the whole
  Crossroads_10 bank (26,572 B) fits the package, so the rest is empty and nothing is linked.
- The reader (`bind`/`attach`, `set`, `upload`, and the decoder it runs) lives in the **`bank_reader` code module**, carried
  by the same scenes as the data package (`scene_modules`); the resident image keeps the statics, the table reads, the draw
  and two guarded hooks in `boss_hooks.rs` (`bind_bank`, `upload`: a resident call into a module that is not loaded traps, so
  each checks `modules::loaded(BANK_READER)`; `host/module_edges.json` lists exactly these two edges). The module may not
  inline resident helpers that read resident-only data (`animation_cache::dest` and `modules::bytes` are reached through
  `boss_hooks::upload_texels` and `package`).

## The bank coder (shared with the boss bank)

One coder for every bank, cooked by `hk-cook` (`boss_codec.rs`, chosen per part in `boss_bank::store_parts`) and decoded by one
function, `hk_format::boss::decode_part`, which is generic over the blob so each reader owns its copy (the actor bank in
`bank_reader`, Hornet's in `hornet_art`). A part's `coded` word says how it is stored: `RAW` (0x8000) as it is, neither flag
packbits (the first coder, so banks cooked before the table decode unchanged), `CODED` (0x4000) with the bank's Huffman table,
which sits after the part table. The cooker keeps the smallest of the three for each part and writes no table at all if the
table does not pay for itself.

A `CODED` part is a stream of tokens over its pixels in reading order: a literal pixel, a run of transparent pixels, a run of
copies of the previous pixel, a run of copies of the pixel above. Each token is Huffman coded with a code chosen by the pixel
before it (16 codes, at most 12 bits, canonical; the table is stored as code lengths in a form the decoder walks without
building anything). The format is in the doc comment of `hk_format::boss::code`.

Measured on the real banks (lossless, every part decodes to the texels it was cut from; texel data including the table):

| bank | packbits | coded | change | blob |
| --- | ---: | ---: | ---: | ---: |
| husks (Crossroads_10, 99 parts) | 34,690 | 22,753 | -34.4% | 39,420 -> 27,483 |
| Hornet (335 parts, 229 sprites) | 129,732 | 83,424 | -35.7% | 142,800 -> 95,626 (70 chunks -> 47) |

Variants measured on the way (same banks, husks / Hornet): plain Huffman on the left pixel -28% / -11%; with runs of
transparent pixels -31% / -32%; adding runs of copies of the previous pixel and of the pixel above, the shipped one, -34% /
-36%. The entropy bound with the left pixel as context was -35% / -34%; adding the pixel above as context (256 codes) reaches
-48% / -43% only before paying for its tables, and costs the same decode time, so it was not taken.

### What it costs in time

The decode runs once per part, at its upload (the animation cache keeps it afterwards), not per frame. Measured on the
emulator with a bench that decodes every part of a bank with the production code and times it with Timer2 (system clock / 8),
packbits against the coder:

| bank | packbits per part | coded per part | longest coded part |
| --- | ---: | ---: | ---: |
| husks | 0.25 ms | 1.5 ms | 2.0 ms |
| Hornet | 0.36 ms | 1.6 ms | 4.6 ms |

(Figures are averages over the parts of the bank. The emulator charges about 3.7 cycles for every instruction, so what matters
is the instruction count; a console with its instruction cache hot would be faster, which is unmeasured.) A husk sprite is one
part, so a new husk frame costs about 1.5 ms once; Hornet's sprites average 1.5 parts, so a new frame of hers costs about
2.4 ms, 7 ms for the worst, against about 0.5 ms before: a hitch of up to half a vblank the first time each frame is shown,
and none after. The first coder took 2.6 ms and 8.9 ms (a call inside the loop and a bit-by-bit code walk); the decoder
keeps its state in registers, looks up codes of up to 4 bits in one step, and walks the longer ones.
- In each **bank view** the actors' frames are ordinary frame records (clips, lengths, completion and every validator work
  unchanged) whose texture is the first **carrier** texture, a 4x4 streamed texture whose stream is `HKBP`, the bank id and
  a palette index, and whose palette is that bank palette. The carriers are how the bank's CLUTs reach VRAM with the scene's.
  The sprite index rides in the low bytes of the frame box's first two words (`actor_bank::frame_sprite`).
- **Scoping by data.** A bank actor is listed (`row['actors']`) only in the views that carry the bank. The guest already steps
  and draws only the actors its bound view lists, so an actor in a bank-less view neither asserts nor costs anything. The
  catch is pop-in: a view draws every actor it lists and no other, and the renderer draws the view whose camera range holds
  the camera, which can be a neighbour of the Knight's. An actor must therefore be listed in every view from which it can be
  on screen: the default is the views whose interaction bounds hold its placement, `reach` widens that for actors that wander
  or stand near a view edge, and `views` overrides it (`"all"` for every view of the scene).

## Turning it on for a scene

Add an entry to `host/actor_banks.json` and run a normal build. Nothing else changes: an empty `scenes` object changes no
cooked byte (the work is a postpass over the finished view packs, `regions.postpack_art_bank`).

```
{"scenes": {"Crossroads_10": {
  "id": 6,                      bank id (the carriers name it)
  "package": "art_husks",       the module pool data package that carries the head of the bank (name it art_*)
  "package_bytes": 35000,       bytes of the bank in the package (multiple of 4); the rest is linked data
  "palettes": 8,                shared palettes = carrier textures in every bank view
  "actors": ["Zombie Barger", "Zombie Runner", "Zombie Hornhead"],   names or `source` ids
  "reach": 8,                   optional: widen the default views by this many world units
  "views": [235, 236]}}}       optional: chunk ids, or "all"
```

- Budget what the bank costs: the package counts in the room's pool need (`module_publish.rs` fails the build if it does
  not fit); the linked rest costs image RAM; every bank view pays the carriers' texture records (`row['textures']` against 640),
  its clip and frame records, and the scene's resident raw grows by what the views do not share (a scene that sizes the
  arena pays it 1:1 in BSS).
- Only the Zombie family (`ZombieSwipeWalker`) has its clip bindings in `host/actor_bank.py` (`bindings`, a copy of the chain in
  `cook.append_actor_art`). Another family adds its chain there; the cooker itself (`hk-cook actor-bank`) takes any library.
- The additive-scene rule of `actors.py` (an actor merged from an additive scene is refused until its controller opts in) is
  lifted for the entry's actors by `actor_bank.admit`: being in the entry is the opt-in.
- Hornet: her bank is the chunked form (`boss_bank.rs`, carried as scene textures), which a scene with one big boss uses. An
  ordinary actor family in her room (or any other) takes an entry like the one above; both can coexist only if their cache keys
  do not collide, so a scene carries one or the other for now (`KEY_BASE` is shared).

## Checks


## Hornet's arena

Her bank needs no switch: `boss_bank::build` (the cook of Fungus1_04's art) already goes through `store_parts`, so the coder is
on for her whenever it pays, and the guest's `boss_bank::upload` reads parts through `Bank::decode_into`, which decodes all
three forms. What it does to her: 335 parts and 229 sprites, 129,732 B of packbits become 83,424 B coded (the table is 1,000 B
of that), and the bank string 142,800 B becomes 95,626 B, so her scene carries **47 chunk textures instead of 70** (the cook
log prints `chunks: 47`), 46 KB of the arena's stream. Her decode time per part is in the table above: about 1.6 ms a part,
2.4 ms a sprite, 7 ms at worst, once per frame of her animation the first time it is shown.

Two ways to use the room that frees, both data-only: (a) keep the chunked form and let the scene stream shrink, which needs
nothing but the cook; (b) move her bank to an entry in `host/actor_banks.json` like the husks', which puts it in the module
pool beside `bank_reader` and not in the scene stream (the bank is then 95,626 B of pool, so it only helps if her room has the
slack; the two forms cannot be carried by one scene, `KEY_BASE` is shared). The decoder she uses is the copy in `hornet_art`; a
build with both (`HORNET`, as `main` ships) links and passes the pool gate: the husks' arena carries `bank_reader` and hers
carries `hornet_art`. Her room itself was not walked in this lane (the decode of her bank is proven on the MIPS bench: every
part equal to the packbits decode).

- Cook: the bank views are re-validated by `row_over_budget`; the postpass refuses a view that exceeds its tables.
- Pool: the package counts in the room's pool need; `module_publish.rs` fails the build if it does not fit.
- Guest counters (`HK_ACTOR_BANK_*`): `UPLOADS` > 0 where actors are on screen; `MISSES`, `UNBOUND` must stay 0.
- A/B: a scene moved from the room path to the bank must match frame for frame within the shared palettes' quantiser error.
