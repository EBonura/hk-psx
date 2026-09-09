# Lossless room delta prototype

This is an offline experiment, not the shipped guest format. It uses the 116
cooked regions selected by the local build. No disc images, guest code, source
assets or cooker output are changed. Transfer sizes below are byte/sector
counts, not emulator or hardware timing measurements.

## Reproduce

After a complete cook and pack, with the project's Python environment:

```sh
.venv/bin/python -m unittest discover -s tests -p test_region_delta.py
.venv/bin/python host/region_delta.py \
  --cd-log .hkpsx/prefetch-baseline/doors-cd.csv
```

The ignored report is `.hkpsx/region-delta-analysis.json`. It binds the selected
metadata, packed report, prototype code and CD trace by SHA-256, and includes
each room's verified raw SHA-256. The historic CD trace requires the matching
packed sector table; do not interpret it against a later, differently sized
pack. Without that baseline, omit `--cd-log` to analyze the current room packs.

The four synthetic tests cover odd atlas coordinates, odd texture widths,
stream padding, moved atlas placements, palette changes, an empty donor,
corrupt patches and incorrect donors. The full local run reconstructs all 116
rooms and all 762 directed neighbor transitions byte-for-byte.

## Measured content reuse

Texture identity is exact width, height, all 16 PSX CLUT words, and every 4bpp
texel. It does not assume matching source IDs imply matching cooked output.
Atlas placement and padding are excluded from identity but restored exactly.

| Measurement | Bytes/count |
| --- | ---: |
| Current raw room packs | 60,494,244 bytes |
| Current compressed room packs | 16,992,852 bytes |
| Texture references across rooms | 28,977 |
| Distinct palette-and-texel textures | 2,055 |
| Canonical texture bytes with repetition | 54,022,854 bytes |
| Unique canonical texture bytes | 4,135,003 bytes |
| Unique textures compressed independently | 1,462,937 bytes |
| All uncompressed room metadata prefixes | 1,907,760 bytes |

The compressed texture-bank number excludes recipes, directory/index overhead
and disc layout. Giving every texture its own sector-aligned extent requires
2,095 sectors (4,290,560 bytes) before those additions and could cause many
seeks. The complete unique bank also does not fit in a 768 KiB room slot.

Across the 762 directed neighbor transitions, full-room transfers total
119,899,553 bytes; lossless patches total 43,447,213 bytes: **63.8% fewer bytes**.
The median reduction is 66.3%; the range is 27.7–93.1%. Sector-rounded transfers
fall from 58,906 to 21,601 sectors. These totals weight every neighbor edge once,
not by an observed player's route.

| Transition | Full compressed room | Patch |
| --- | ---: | ---: |
| 1 → 2 | 206,696 | 83,994 |
| 2 → 13 | 192,975 | 60,621 |
| 13 → 14 | 202,593 | 70,671 |
| 14 → 15 | 207,067 | 63,562 |

Storing all transition patches **adds** 43.4 MB if all current full fallbacks
remain. A shared bank reduces duplication on disc, but needs a suitable
sequential layout rather than one seek per texture.

## Baseline prefetch waste

The preserved door CD trace reads chunks
`1, 2, 13, 4, 13, 14, 5, 14, 5, 15`: ten loads totaling 1,775,205 bytes.
Repeated chunks account for 533,909 bytes (30.1%). The matching gameplay result
had ten completed loads and two room-cache hits.

Regions 4 and 5 lie below the center-coordinate boundary at y=11. The native
same-input geometry probe selects only regions 1, 2, 13 and 14; the source floor
keeps the player's center at y=11.390625. The former prefetch rule predicted
downward transitions while falling near that boundary, replacing useful
horizontal neighbors. This explanation combines the CD trace, source geometry
and host simulation; the trace itself does not record selected-region history.
Floor-aware prediction is being investigated separately from this format.

## Format and guest constraints

`HKDLTA01` contains the target size and metadata prefix, donor/target SHA-256,
one reference per target texture, and missing canonical textures. References
either name a texture in the immutable donor room or a missing texture in the
patch. Optional whole-patch HLZC compression is used for the host measurements.
The final target hash covers the entire reconstructed HKROOM02, including
metadata, palettes, atlas padding and the animation bank.

Whole-patch decompression is unsuitable as the immediate guest implementation:
the largest uncompressed patch is 436,837 bytes. In 415 of 762 transitions,
target room plus uncompressed patch exceeds the second 768 KiB arena; the worst
case is 1,136,497 bytes. The prototype's Python allocations are not a PS1 RAM
budget or an overlap-safe in-place decoding proof.

The next bounded guest experiment should keep the current room arena immutable,
write the next room's metadata into the other arena, and copy matching donor
textures directly into their target atlas locations. Missing textures should
arrive as sequential records with individual HLZC/raw framing, decoded through
one aligned scratch buffer. The measured largest canonical texture is **8,228
bytes: 8 KiB of texels plus 36 bytes of dimensions and CLUT**; the largest
compressed record is 3,275 bytes before future record headers. Every blob's
in-place SDK decode must be checked, with raw fallback where necessary.

A fixed transition index, bounded metadata, sequential CD record consumption,
record bounds/checksums and a final full-room checksum are required before
switching arena ownership. This still needs measured scratch, stack and CPU
costs. It also retains complete static VRAM uploads on region changes; avoiding
those uploads would require a separate stable-placement/cache design.
