# Host tools

Host-only Python source readers, cooker, local CIL inspection and guest build
orchestration. Dependencies are locked in requirements.lock and installed in
the ignored .venv by `cargo hk-build build`. Retail inputs are read-only.

`paths.py` defines the sole disc destination: `~/Downloads/ps1 games/`.
Cooked assets and source/provenance inspection output remain ignored in the
repository. The guest does not link any extraction library.

`room_inventory.py` scans Windows BuildSettings scenes for serialized sprite,
material and tk2d animation dependencies, sharing its pass with `room_graph.py`
for literal TransitionPoint exits. Reports and source hashes stay in `.hkpsx/`.
`residency_plan.py` precomputes current-room/neighbour unions plus one
deterministic directed fetch/keep/evict record for every authored gate. It
preserves gate IDs and serialized conditions, and refuses executable eviction
when target content is incomplete. Unknown cooked costs remain unknown; source
atlas sizes are not a PS1 memory budget. These host plans do not yet drive
runtime room swaps.
