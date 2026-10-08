# PS1 runtime

Native no_std PS1 guest using the pinned PSoXide SDK. The runtime consumes the
validated pack from `data/`, uploads resident textures, renders the original
chamber, polls input and advances deterministic simulation on VBlank edges.

Build through `cargo hk-build build` at repository root so source conversion,
the exact SDK pin, final-binary hazard scan and disc packaging stay in sync.
See `../docs/STATUS.md` for actual supported behavior and measured limitations.
