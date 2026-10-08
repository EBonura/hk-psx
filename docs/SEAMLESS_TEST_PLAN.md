# Seamless transition validation

This plan targets uninterrupted traversal after the initial loading screen.
The preserved starting point is `.hkpsx/seamless-baseline/`: EXE/map, generated
metadata, provenance/build/validation reports, source, and the original user
recording. BIN/CUE hashes are recorded, with no copied disc outside the library.
The initial artifact is `4ea23140719eb76858f31858ec4f219c6bfca2b83bb4ae73231edee4f6843f60`.

The starting artifact has boundary waits and activation stalls. Current replay
results and their exact build hashes are recorded in `docs/PERFORMANCE.md`; the
criteria below remain mandatory for every final traversal fixture.

## Continuity gate

`tools/profile_tape.py --require-seamless` requires all of the following after
initial gameplay begins:

- Both boundary wait counters remain zero.
- The guest's global maximum pad gap is at most one VBlank. Independent
  observations compare completed polls against the SDK VBlank counter: their
  lag varies by at most one count (the current not-yet-polled VBlank). Raw
  sampled spans are at most two route ticks; two sampled ticks can contain two
  polls and must not be mislabeled as a missed update.
- Every displayed-frame interval fits the explicit frame budget, initially four
  route ticks. Activation intervals must also be no longer than the longest
  ordinary interval on that route. Loading/activation states are included.
- Runtime fault, room-load error, and stream-error counters are never nonzero;
  recovered errors are failures too. Discarded CD sectors remain zero.
- The route crosses at least one region and finishes in gameplay.

CD activity alone is not a hitch. Reports retain all-frame, CD-active,
CD-inactive, boundary-wait, and activation observations separately. The
`--max-frame-route-ticks` argument can tighten the four-tick ceiling if the new
renderer establishes a smaller ordinary budget; it must not be raised merely
to accommodate transition stalls.

Optional diagnostics include upload bytes, maximum bytes per frame, VRAM bank
hits/misses, active bank, and pending region. These support attribution but do
not replace direct frame and polling continuity measurements.

```sh
python3 tools/profile_tape.py \
  --exe dist/hk-psx.exe --map build/hk-psx-normal.map \
  --cue "$HOME/Downloads/ps1 games/hk-psx.cue" \
  --tape .hkpsx/seamless-baseline/recording/latest.pxtape \
  --output .hkpsx/seamless-user --require-seamless
```

Use the exact map recorded by the current `.hkpsx/build.json`; the example name
is not an instruction to use a stale map. Each output directory must be new.
The runner freezes EXE/map before launching and binds all inputs by hashes.

## Required actual traversals

1. **Original recording:** replay the preserved 1,585-sample tape, starting at
   poll 74. Confirm the expected sequence of visited source regions and inspect
   the visible Knight/layers; pacing changes can alter the endpoint.
2. **Forward traversal:** continue the existing door route through multiple
   region boundaries toward the first Crawler. Confirm several actual region
   changes, source barriers broken, and no death or teleport masking a stall.
3. **Return traversal:** reverse direction through previously visited regions,
   then go forward again. Confirm bank hits and actual reverse region changes;
   standing at a wall does not satisfy the test. Verify persistent grass/door
   state and correct palettes after the cached return.
4. **Repeated jumps at a boundary:** after reaching a verified boundary,
   alternate movement and jump repeatedly on both sides. Confirm repeated
   vertical/horizontal region selections, with no alternating-neighbour load
   churn, bank overwrite, animation palette corruption, or input gaps.

Timed fixtures must be calibrated against the final build's actual player and
region observations. The former combat route demonstrated why a jump scheduled
during a boundary wait can be lost; silently retaining an obsolete route does
not test the intended crossing. Keep assertions on visited regions, state, and
combat outcomes alongside performance gates.

For each route, preserve the exact tape, executable/map hashes, CD command and
bank diagnostics, final RAM, and software captures. Repeat representative runs
with hardware command replay and inspect captures around both sides of region
changes. GPU command census drains its capture log, so hardware replay is a
separate run. Compare software pixels across the two runs to ensure profiling
has not changed the deterministic output.

## Independent VRAM layout audit

The implemented layout uses 16-bit VRAM word coordinates. Four banks each hold
five static 4bpp pages and 384 palettes. Physical pages are `bank * 5 + local_page`,
with origins `(384 + (page % 10) * 64, (page / 10) * 256)`. Each page occupies
64×256 words. The four banks fill all twenty reserved static pages.

| Allocation | Word coordinates / rows |
| --- | --- |
| Framebuffers | x0–319, y0–479 |
| Twenty-four animation slots | x320–351, y0–255 and x960–1023, y256–511 |
| HUD icon storage | x352–355, y0–31 |
| HUD palettes | x352–367, y480–481 |
| Bank 0 palettes | x0–319, y480–499; 20 CLUTs/row, final row partial |
| Bank 1 palettes | x320–383, y256–351; 4 CLUTs/row |
| Bank 2 palettes | x320–383, y352–447; 4 CLUTs/row |
| Bank 3 palettes | x352–383, y32–223; 2 CLUTs/row |

A separate geometric audit checked all framebuffer, animation, HUD, static-page,
and CLUT rectangles: zero overlap and all rectangles inside 1024×512 words.
All 1,536 encoded CLUT addresses are unique. The implemented-layout report is
`.hkpsx/seamless-baseline/vram-four-bank-audit.json`. The older
`vram-bank-audit.json` preserves the superseded three-bank proposal separately.
The current GPU upload slice is bounded to 64 KiB; continuity measurements must
include those upload frames. This bank capacity does not establish full-game
asset compatibility or original-hardware performance.

The guest allocator tests must cover:

- Every bank/page and all 384 palettes map to these reserved locations, including
  the first/last CLUT and the partial bank 0 palette row. Reject page index 5
  and bank index 4.
- Static sprites and streamed Knight/nail/enemy frames use the same active-bank
  palette mapping. Streamed textures may occupy shared animation texel slots
  while referencing different palette banks.
- A bank is ready only after all texels and palettes are uploaded and the GPU
  texture cache has been invalidated. Partial or failed uploads never activate.
- The current bank and banks referenced by outstanding GPU work cannot be
  overwritten. A completed framebuffer contains pixels, so it has no continuing
  texture reference by itself; protect the active room's future draw resources.
- Shared animation slots cannot identify a texture solely by a room-local ID
  across transitions. Use a residency epoch/global key or invalidate explicitly,
  and measure any resulting upload work in the activation continuity test.
- Eviction followed by re-entry restores the correct texels, CLUTs, and material
  state. A cache hit must not activate stale local-ID templates.
- Five static pages per bank bound VRAM use only. Metadata, animation streams, decompression
  buffers, and staging work still need separate RAM and per-tick budgets.

The v2 phase diagnostic in `.hkpsx/poll-phase-v2/phase-analysis.json` verified
this distinction: after gameplay began, VBlank count minus pad polls was only
144 or145, despite raw two-tick spans. The earlier raw-one-tick-only gate was
a sampling error; it was corrected with independent clock evidence and tests.
The actual zero boundary-wait and global one-VBlank polling requirements remain.


## Completed final-build checks

The final v4 artifact passes all134 host/native tests and the complete CUE/EXE
smoke validator. The original tape, forward/return and separate boundary-jumps
replays pass the strict gate: zero waits, maximum per-poll gap1 VBlank, independent
poll/VBlank phase variation≤1, frame maximum4, and no activation spikes/errors.
The boundary-jumps tape performs twelve observed alternations between13/14;
the older jumping tape only advances through1→2→13→14 and does not establish
repeated crossings on its own. Exact reports and hashes are in PERFORMANCE.md.

Generate the asset-free regression tapes reproducibly with:

```sh
python3 tools/seamless_routes.py --output .hkpsx/seamless-routes
python3 tools/profile_tape.py --exe dist/hk-psx.exe \
  --map build/hk-psx-normal.map \
  --cue "$HOME/Downloads/ps1 games/hk-psx.cue" \
  --tape .hkpsx/seamless-routes/boundary-jumps.pxtape \
  --output .hkpsx/boundary-jumps-new --require-seamless
```

The generator's four outputs were compared byte-for-byte with the tapes actually
tested. Use a fresh output directory for each profile run. The user's original
tape remains a separate ignored read-only input. Initial title loading, unknown
future paths and physical hardware remain outside these route-specific claims.
