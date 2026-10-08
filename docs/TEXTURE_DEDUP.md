# Exact texture sharing within a region

The cooker canonicalizes the final PS1 texture representation: width, height,
all sixteen CLUT words, and every indexed 4bpp texel. Static and streamed images
remain separate storage classes. Matching source IDs alone are insufficient;
different dimensions, palette words or texels do not alias. Different source
RGBA images may safely share an entry when their final PS1 representations are
identical.

`Atlas.add` still performs the same resize and 15-colour quantization. It returns
a canonical texture index, which individual draw/frame records retain alongside
their own source IDs, geometry, tint, layer and animation timing. The source
128-pixel scenery quality limit was unchanged for this deduplication experiment.
The later user-authorized 48-pixel scenery reduction is measured separately in
SCENERY_BUDGET.md. This shares image storage; it
does not merge repeated scenery instances or remove parallax layers.

The scene-wide actor append also passes through `deduplicate_room`, because
some base regions already carry the same actor images. That postpass remaps
texture references and repacks palettes/pages/stream data without altering
frame order, clip IDs, local animation bounds or collision data. The HKROOM02
binary layout is unchanged. Per-region reports retain the original request to
canonical index mapping and the append-stage old-to-canonical map, so source
ownership can still be traced through the final pack.

## Baseline measurements

The separate experiment used the 116-region build before deduplication. It
rewrote copies under `.hkpsx/texture-dedup/`; the baseline packs were verified
unchanged during that comparison. These aggregate totals are across independent
regions, not simultaneous PS1 residency:

| Quantity | Before | After |
| --- | ---: | ---: |
| Texture records | 28,977 | 27,148 |
| Static 32 KiB pages | 1,666 | 1,528 |
| Animation-bank bytes | 3,067,732 | 3,029,184 |
| Total raw pack bytes | 60,494,244 | 55,845,920 |
| Opening-region static pages | 17 | 16 |
| Opening-region texture records | 270 | 260 |

Of 116 regions, 65 freed one page, 32 freed two, three freed three, and sixteen
kept the same page count. That is 138 fewer static pages in aggregate, or 8.3%.
The busiest examples moved from twenty pages to seventeen. The resulting free
VRAM can accommodate additional admitted textures; it is not proof that any
arbitrary larger scene or animation set fits.

Every remapped texture was compared byte-for-byte, including its palette.
All 116 outputs passed the native HKROOM02 reader, and all draw/frame geometry,
ordering, clips and edges were checked unchanged. A separate opening-region
source recook verified all 243 draw records and 127 animation-frame records,
including their source metadata, exact texels and palettes. Nine cooker tests,
three texture-dedup tests, four delta tests and three region-plan tests passed.
No GPU timing improvement is inferred from these storage measurements.

The completed normal refresh now needs 98 spatial regions, totaling 46,577,816
raw bytes, because fewer areas require splitting. Its final packs pass the native
reader; metadata hashes/counts, actor clip indices and cook-cache code/output
hashes match. The opening is 578,236 bytes with 260 textures and sixteen static
pages. Region count and later IDs therefore differ from the fixed 116-region
comparison above. These are cook results; final executable/disc measurements and
guest validation belong to the corresponding complete build.

Reproduce the separate pack comparison after a complete matching cook/build:

```sh
.venv/bin/python host/texture_dedup.py \
  --metadata .hkpsx/selected-regions.json \
  --output .hkpsx/texture-dedup
.venv/bin/python -m unittest discover -s tests -p test_cook.py
.venv/bin/python -m unittest discover -s tests -p test_texture_dedup.py
```

The report binds the metadata, cooker and compactor code by SHA-256 and retains
each input/output pack digest and texture map. Re-running against a newer build
measures that build; it does not recreate the earlier baseline totals.

## Transparent-border opportunity

An offline audit measured fully transparent index-zero borders without changing
any geometry. At the opening camera-range midpoint, a guarded nontransparent
screen-space bounding rectangle reduced the sum of clipped draw AABBs by 11.64%.
Across one midpoint sample in every baseline region, the reduction was 7.29%.
The calculation included initially hidden variants and allowed one texel plus
one output pixel of guard. These are bounding-box estimates, not exact raster
coverage or GPU time.

A future per-draw scissor could keep original vertices and UV interpolation
while excluding proven transparent borders. It would need careful conservative
bounds, packet/state restoration and raster comparison. Cropping the quad itself
could change rounding and sampling. Both approaches are deferred; the current
change only deduplicates exact texture storage. Audit details remain in
`.hkpsx/texture-dedup/transparent-bounds.json`.
