# Scenery resolution and fixed-region memory experiment

The user explicitly accepted lower scenery resolution to improve continuous
traversal. This experiment preserves the current 98 region boundaries and every
source draw quad, tint, layer, collision edge, animation frame bound and clip.
Knight, nail and enemy texture dimensions, palettes and 4bpp texels are identical
to the previous build. HUD assets are outside these packs and remain unchanged.
Only native scenery SpriteRenderer images receive the lower long-axis cap.

The Windows source images feed the same Lanczos resize and 15-colour quantizer
as the cooker. The 128-pixel replay reproduced every original scenery texture
and palette across all 98 packs before lower-cap measurements were accepted.
Exact canonical texture sharing remains enabled. Pack ordering may change;
source instances and animation references are remapped rather than discarded.

| Long-axis cap | Total pages | Largest region pages | Largest raw pack | Largest stored pack | Total stored bytes |
|---|---:|---:|---:|---:|---:|
| 128, previous quality | 1,273 | 20 | 723,872 | 232,297 | 13,728,308 |
| 80 | 687 | 12 | 460,432 | 167,710 | 9,647,588 |
| 64 | 484 | 8 | 328,208 | 138,234 | 8,001,409 |
| 56 | 411 | 7 | 294,816 | 122,177 | 7,212,283 |
| 48 | 309 | 5 | 228,416 | 104,245 | 6,291,710 |

The initial experiment evaluated three six-page static banks. The 48-pixel
candidate needed at most five pages, which enabled the final four-bank layout:
five pages and 384 CLUTs per bank. At 56 pixels nine regions still need seven
pages; 64 pixels leaves 41 regions above six. These figures concern spatial
texture residency, not GPU fill rate: source quads cover the same screen area.

For each region, the report considers its current pack and every possible pair
of distinct immediate neighbours. At 48 pixels the worst aggregate is 15 pages
and 680,812 decoded bytes. This is a three-region window, not a claim that every
neighbour is simultaneously resident. Four 384 KiB RAM slots are a separate
runtime design choice and reserve 1.5 MiB before other game memory.

Every 48-pixel pack passed both the pinned SDK decoder and the incremental guest
decoder in a 384 KiB arena at scheduling budgets 1, 31, 2048 and 8192 bytes. Largest
raw-plus-stored requirement is 332,661 bytes, leaving 60,555 bytes even under the
conservative assumption of disjoint source/output buffers. The actual decoder
permits safely consumed compressed input to overlap output; raw-plus-stored is
not its admission rule.

Reproduce the isolated experiment:

```sh
.venv/bin/python host/scenery_budget.py --caps 128 80 64 56 48
.venv/bin/python -m unittest discover -s tests -p test_scenery_budget.py
HK_ROOM_ARENA_BYTES=393216 cargo run --release --locked --offline \
  --manifest-path shared/hk-format/Cargo.toml --example check_incremental -- \
  .hkpsx/scenery-budget/cap-48/compressed/*.hk
```

The experiment writes to `.hkpsx/scenery-budget/` only. Its `baseline/` contains
all 98 previous packs, exact source/draw provenance, remote mask bindings,
generated region metadata and source/cooker hashes. Per-cap reports include
all region sizes and neighbour windows. The `compressed/` decoder fixtures use
the same HLZC high-compression LZ4 wrapper as `host/pack_rooms.py`.

These measurements establish memory capacity. Final guest captures, frame
cadence, CD deadline behaviour and physical-drive performance must be verified
separately before claiming seamless traversal. In particular, masks must keep
their owner-state bindings across regions at every texture resolution.

The selected production quality is 48 pixels. The final candidate instead uses
four five-page static banks, each with384 CLUTs, and five256KiB RAM arenas.
Every measured pack already meets those stricter admission bounds, so this
change required no resampling or pack changes. `host/quality.py` records the exact 98-region
layout; the cooker rejects overflow instead of silently introducing new boundaries. The
ignored `comparison.html` shows nine decoded original texture pairs at equal
display sizes. It does not replace final emulator capture verification.


Final runtime evidence is now available in STATUS.md and PERFORMANCE.md: the
two preserved recordings, forward/return, twelve alternating jump crossings and
longer traversal/earned-heal routes all
pass the strict continuity gate on the final four-bank build. Both emulator
renderers were inspected. These results do not establish physical-drive timing
or every future scene's capacity.
