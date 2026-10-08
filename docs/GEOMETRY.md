# Source terrain fills

The under-platform gaps reported in September 2026 had a concrete extraction
cause: the scenery cooker admitted `SpriteRenderer` objects but omitted the
`MeshRenderer` chunks belonging to original `tk2dTileMap` render data. Decorative
platform edges survived while their opaque black interiors did not. The opening
platform's `level6:7793` chunk alone contains 495 such source cells.

`host/tilemap_fill.py` selects descendants of an enabled, active tilemap's actual
`renderData` reference. Names do not authorize geometry. Admission verifies the
original triangles, white vertex colors, planar coordinates, material, shader
blend state and texture sampling footprint. The observed `tk2d/BlendVertexColor`
material samples only fully opaque black, including a conservative extra texel
around its entire UV range. Unsupported active tilemap meshes fail explicitly.

Each admitted triangle must be half of an integer unit cell; its partner must
complete that cell without overlap. The cooker merges occupied cells into
disjoint rectangles and independently reconstructs their union. Missing cells
remain missing: this does not fill genuine holes or invent underfloor geometry.
Hierarchy transforms, depth, parallax, layer and sorting order are retained.

| Source scene | Meshes | Occupied cells | Source triangles | Merged rectangles |
| --- | ---: | ---: | ---: | ---: |
| Tutorial | 21 | 8,826 | 17,652 | 232 |
| Town | 15 | 3,250 | 6,500 | 37 |

The existing draw ABI carries the rectangles with a shared 4×4 constant-black
texture. Palette index 1 remains the nontransparent PS1 sentinel; the renderer's
red-127 modulation produces actual black. No scenery image or animation is
resampled for this fix. Preflight against the previous 98-region metadata added
1,595 draw records (70,180 bytes), at most 40 per region, with a projected maximum
464 draws. Final combined build budgets are reported separately.

## Native validation

Five host tests cover triangle rejection, both diagonals, randomized occupancy,
holes, disjoint merging, conservative sample support and source culling. A native
software-GPU comparison of original cell quads against merged textured quads
matched **66,355,200 pixels**, covering all 36 meshes, fractional camera positions,
reflections and both framebuffer locations.

A separate renderer proof compared original textured black against the retained
opaque flat-quad path: another **66,355,200 pixels** matched. Flat packets are
used only for proven constant-black texels at full opacity; faded masks retain
their textured path.
The immutable scissor-candidate gate only skips optional work; rejected
candidates retain their original full quad. Ten native packet/cache tests also
passed, including reuse between scissored, flat and textured DMA packets.

Two occlusion experiments were rejected for this build. Same-parallax whole-quad
occlusion passed 206,438,400 synthetic GPU pixel comparisons, but found **zero
usable pairs across all 98 actual regions**. Its activation search and per-draw
lookup were removed. Screen-space subtraction of a later opaque fill also
preserved pixels (223,948,800 comparisons using actual cooked textures), but
estimated only about **2% less static raster work** at the opening and across
region-center samples after existing alpha scissors. That estimate assumes
visible source variants and is not a measured speedup; the additional clipping
and packet-allocation work was not adopted. Local measurements and scripts are
in `.hkpsx/screen-occlusion-audit/`.

Reproduce tracked tests:

```sh
.venv/bin/python -m unittest discover -s tests -p test_tilemap_fill.py
cargo test --locked --offline --manifest-path shared/hk-format/Cargo.toml --test draw_packet --test alpha_scissor_cache
```

Local source IDs, hashes, native commands and proof inputs are preserved in
`.hkpsx/geometry-hole-audit/README.md`, `validation.json` and
`flat-validation.json`. These are native software-GPU and source-coverage
results, not final CUE validation, hardware timing evidence, or a claim that all
original mesh/material systems are supported. Inspected actual-CUE captures
(`.hkpsx/burst-fix2-*/screenshots/`) show opaque platform interiors and a black
interior where an earlier run showed a dark-blue wedge; performance results and
hashes are in `docs/STATUS.md`.
