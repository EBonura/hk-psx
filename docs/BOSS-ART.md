# Boss art: one bank per boss, carried by its scene

Status, 2026-10-25. Built and replayed on Greenpath's Hornet (Hornet 1): the build numbers are under "Built". Measured, not built, for the other bosses (table at the end).

## The problem

A boss's art is not scenery. Hornet plays 43 clips on her own library and the Hornet NPC sheet, at several scales, and the
room format was sized for scenery: 640 texture records and 416 palette slots per view, 256 KiB of streamed texels per scene, and
the False Knight's bank already uses 19 of 19 VRAM pages. `hk-cook hornet-measure` (the old way of cooking her) asked for 459
sprites, 835 parts, 496 KB, 16 pages and about two CLUTs per part. Nothing in that list was a visual choice; it was a
representation that spends bytes on art nobody sees.

## What the fight actually needs (`hk-cook boss-measure`)

Measured on the Steam files with the room quantizer, over four simulated fights (`shared/hk-sim/tests/hornet_clip_trace.rs`: the
Knight walks into the Encounter, chases her and strikes until she dies; every clip every object plays is logged).

| | sprites | texel bytes |
| --- | ---: | ---: |
| what the tree cooker names (every clip any object of a library could play, on every object of it) | 485 | 496 KB as whole 64x64 cell grids |
| clips each object plays (her body: all 31 her trees name; effects and corpse: what the fights play, plus effect-named clips) | 229 | 262 KB trimmed |
| the same, run-length coded | 229 | 130 KB (LZ4 per part: 115 KB) |

Where the 485 came from: the tree cooker gives every animated object of a library every clip name the tree mentions, and one
object (the Flash) stands at 1.385 x 1.05 scale, so the whole library was cooked a second time for a clip it never leaves.
Her opaque texels are 23% of her pixels, so cutting empty 64x64 cells and trimming the rest to their opaque box keeps 67% of the
bytes of whole frames, and run-length coding halves that again (transparent runs and flat fills).

Time structure (clips on screen in a window, `boss-measure --trace`): 1 s windows 35 KB trimmed on average and 77 KB at worst;
3 s 56 and 92 KB; 10 s 88 and 143 KB. Her moves are all in rotation within any ten seconds, so there is no phase that lets a
smaller set stand in for the rest. Per-state residency would only save half of a bank that already fits.

## Options, with numbers (Hornet)

| option | what it costs | verdict |
| --- | --- | --- |
| A. as before: whole frames as tile grids appended to the room | 485 sprites, 835 parts, 496 KB against 640 texture records, 416 palettes and 256 KiB of stream per scene | does not fit, and nothing about it is tunable |
| B. raise `MAX_TEXTURES` and the palette banks | about 26 to 29 KB of resident BSS (the per-texture tables in `render.rs`) for 1,100 textures; the 256 KiB stream cap and the palettes are untouched | no: pays resident RAM for every room to serve one |
| C. stream clips from disc during the fight | worst 10 s working set 143 KB trimmed (about 70 KB coded); worst 1 s 77 KB. Boss music is XA (the Shade's `carry` comment: "XA holds the drive for the whole fight"), a data read stops it and costs about a second each way | no for Hornet. Stays the answer for banks above the scene's stream cap, loaded at the arena gate and at beats where the game already waits (conversations, phase changes, the stun), never mid-attack |
| D. share palettes across sprites | 16 shared 15-colour palettes for 229 sprites add 4.27 RMS per colour channel over the room quantizer's own result; 0.07% of texels move by more than 32. See the table below | yes |
| E. Manny's dual-frame CLUT packing (two frames in one 4bpp texture, two CLUTs) | two frames in 4 bits leave each three colours and a transparent index: 33 RMS, 57% of texels off by more than 32, 10 of 218 sprites stay within 3 RMS of their 15-colour version | does not pay for Hornet: her frames have many colours. It saves texels only where a frame is near-flat; keep it for such art (silhouettes, masks, glows). At 8bpp it saves no bytes, only texture records |
| F. a boss budget separate from scenery | texture records +70 of 640, palettes +16 of 416, scene stream +142 KB, no VRAM page, no new CLUT row, no new key range beyond 335 of the 2,048 animation keys | yes: this is the chosen path |

Shared palette sweep (229 sprites; `boss-measure`, RMS per colour channel, 0 to 255):

| palettes | added over the room quantizer (RMS / worst texel / >32 off) | against the source art (RMS / >32 off) |
| ---: | --- | --- |
| 128 | 1.34 / 20 / 0.00% | 6.97 / 2.17% |
| 64 | 2.62 / 57 / 0.00% | 6.84 / 1.83% |
| 32 | 3.50 / 67 / 0.01% | 6.68 / 1.43% |
| **16** | **4.25 / 67 / 0.07%** | **6.49 / 0.83%** |
| 8 | 4.89 / 67 / 0.15% | 6.59 / 0.59% |
| 4 | 5.64 / 157 / 0.17% | 6.66 / 0.32% |
| 2 | 6.48 / 157 / 1.18% | 7.24 / 1.13% |
| 1 | 8.72 / 45 / 4.18% | 9.11 / 4.50% |

(The room quantizer's own error against the source is higher than the shared palettes' because it re-decides anti-aliased edge
texels against a dark background prior; the first column is the honest "what sharing adds".) Sixteen is where the curve flattens.
`scratch`-free proof: `hk-cook hornet-bank --sheet DIR` writes `hornet-bank-sheet.png`, the original art beside what the guest
decodes out of the chunks.

## The path

**Cook** (`host/hk-cook/src/boss_art.rs`, `boss_bank.rs`, `hornet_scene.rs`):

1. The sprites each object plays, at the size they have on screen (`hornet_clips.tsv` from the simulated fights; her body gets all
   of its clips; objects the fights never reach get their effect-named clips). A clip the bank lacks draws nothing and bumps
   `HK_BOSS_CLIP_MISSES`.
2. Each sprite through the room quantizer (the same pixels the old path made), then palettes merged greedily, always the pair that
   adds least error, down to 16, and each sprite's texels recoloured onto its group's palette.
3. Each sprite cut into 64x64 cells, empty cells dropped, the rest trimmed to their opaque box: a *part*.
4. Each part packbits-coded (the tokens `ability_art.rs` already decodes), or stored raw when coding does not shrink it.
5. Tables (clips, frame sequence, sprites, parts) and coded texels become one byte string, cut into 2,048-byte chunks, each led by
   `HKBC`, the bank id and its index. Each chunk is appended to **every region of the boss's scene as an ordinary 64x64
   streamed texture** (`mossman_scene::append`). Chunk `i` names palette `i % 16`, so the first sixteen carry the palettes.

**Why chunks as textures.** The scene pack stores, deduplicates, loads and frees streamed textures already. The bank therefore
needs no new chunk on disc, no loader, no arena change and no CD read: it is in RAM with the scene, and the seamless-gate work
that streams scenes streams it. The palettes reach VRAM with the scene's CLUTs, so no VRAM rectangle is reserved.

**Guest** (`game/src/boss_bank.rs`, format in `shared/hk-format/src/boss.rs`):

- `attach` finds the chunks by their magic once per scene and keeps their texture ids (96 at most).
- A part's animation-cache key is `KEY_BASE + part`, above every other art set's (and the dialogue panel's tiles). The cache's upload closure asks
  `boss_bank::upload` (through `boss_hooks.rs`), which decodes the part (at most 2,048 bytes) into a stack buffer and uploads it to its slot.
- The Hornet module (`enemies/hornet.rs`) turns each visible object of the live tree into the parts of the sprite its clip shows,
  projects them with the same camera maths as before and queues them. `frame.rs` adds the queue's keys to the working set and
  draws the queue after the actors, with the hit flash through the same silhouette CLUT. A part that finds no room in the
  working set is counted in `HK_BOSS_DROPS` (pop-in), which a journey pins at 0.

Cost: see `docs/BUDGET.md` (resident bytes, pool fit) and the build numbers in `hk-boss-art/NOTES.md`.

## Built (Hornet 1, 2026-10-25, disc build of `hk-psx` on `feat/hk-hornet2-2026-10-25`)

| | before the bank | with it |
| --- | ---: | ---: |
| Fungus1_04 scene stream (limit 256 KiB) | 41,624 | 184,984 (the bank is 70 x 2,048 = 143,360) |
| Fungus1_04 scene textures (records) | 224 | 294 (a view holds at most 640) |
| Fungus1_04 scene palettes | 197 | 213 (a view holds at most 416) |
| Fungus1_04 scene pages | 11 | 11 |
| `hornet` room module, the fight (a window is 53,504 at the pool's base) | | 49,200 |
| `hornet_art` room module, the reader and the draw | | 14,456 |
| `art_hornet` data package, state block and trees | | 15,976 |
| Hornet room in the pool (carries the death and respawn art and the ability effects, no Shade) | | 98,084 of 98,304 |
| free RAM below the pool | 9,008 (main) | 8,368 (shipping disc) |

On the disc the Knight crossing her arena (`hornet-road`) saw 70 chunks found, no part dropped (`HK_BOSS_DROPS` 0) and no clip the bank
lacks (`HK_BOSS_CLIP_MISSES` 0). The bank costs the scene its load: 143 KB more streamed bytes at the Fungus1_04 gate, with no read
during the fight.

What the build taught that the design did not know. A module and its fixups must stay inside one `lui` window, and the first
layout moved the pool to a window boundary to fit a 65 KB module; with the resident image where main is today that leaves nothing
below the pool, so her trees (9,828 bytes of read-only data) went to the room's data package and her drawing to a second module,
which leaves the fight module under the window at the pool's own base. Modules cannot call each other, so what both need (an
object's composed transform, the bank reader's memory) lives in the state block. The gate path must service the pad between the
steps of a big install, and a frame that needs two dozen new parts must take the pad poll between uploads, or a poll is late.
Resident helpers kept alive only by a module are paid for in resident bytes (`State::edge`, 1,768 bytes): the module takes the
actors' shared copy instead.

## The other bosses

`hk-cook boss-survey SCENE...` totals every clip of every sprite library the animators of a boss scene use, at the scale they
stand at: an upper bound. It is not Hornet's "what is played" figure, which for Hornet is 130 KB against this upper bound of 288 KB
for her library, and it double counts a library that stands at several scales.

| boss (scene) | library clips / sprites | trimmed | coded | against the scene stream cap (256 KiB, shared with the scene) |
| --- | ---: | ---: | ---: | --- |
| Hornet 1 (Fungus1_04_boss) | 122 / 452 (her library) | 593 KB | 288 KB | played subset 130 KB: fits (built) |
| Mantis Lords (Fungus2_15_boss) | 74 / 254 | 684 KB | 378 KB | over as an upper bound; three lords play one library, phases bring them in one at a time: per-phase banks |
| Soul Master (Ruins1_24_boss) | Fire Effect 14 / 63, cracks 4 / 16; the Mage Lord library is refused (tk2d wrap mode 3, ping-pong) | 184 KB | 130 KB | fits once ping-pong wrap is in the bank format |
| Hollow Knight (Room_Final_Boss_Core) | 380 / 1,881 (one library, 22 objects, several scales) and 208 / 843 (the Knight) | 5.3 MB | 2.6 MB | no: needs per-phase banks and a measured played set; far past RAM |
| Radiance, Grimm, Pure Vessel | see `scratch` of the survey run in `hk-boss-art/NOTES.md` | | | |

What this says: Hornet 1 and Soul Master fit the one-bank path. Bosses whose played set is over the scene's 256 KiB stream
share (Mantis Lords, Hollow Knight, Radiance) need the segmented form of the same bank: one bank per fight phase, each a set of
chunk textures, the next phase's bank read in by the scene-load path at a beat where the game is already waiting. The format
needs no change for it (the bank id and chunk indices already name segments). The XA rule above decides when a read may happen.
That part is designed here and not built: no boss other than Hornet 1 has a fight tree in the build.
