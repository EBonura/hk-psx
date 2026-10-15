# Host tools

Host-only Python source readers, cooker, local CIL inspection and guest build
orchestration. Dependencies are locked in requirements.lock and installed in
the ignored .venv by `cargo hk-build build`. Retail inputs are read-only.

`paths.py` selects the disc destination. `cargo hk-build build --library DIR`
packages the disc in `DIR`, records that path in the build report, and uses it
for validation. The default is `~/Downloads/ps1 games/`.
`cargo hk-build cook --hollow-knight DIR --sdk-source DIR` runs the same source
and SDK setup as `build`, then stops after the regions cook. A fresh checkout
first hashes the complete source input list, then cooks regions in parallel with
cache records bound to that manifest. A later `build` checks the completed provenance and
output hashes before reusing the cooked result.
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

`hk-fsm` also holds the FSM v2 measuring tools (docs/FSM-V2-DESIGN.md). `fsm-census` tabulates the
PlayMaker actions of every HealthManager tree from the Windows Unity files and the
`hk-feature-census` row cache: `cargo run --release --manifest-path host/Cargo.toml -p hk-fsm-cook
--bin fsm-census -- DATA CACHE OUT`. `fsm3-sizes` lowers the Crawler, Gruzzer, Vengefly and Hornet 1
trees to the proposed HKFSM03 layout (`v3.rs`), checks the decode against the source graph and
reports real byte sizes. Both write to a directory outside the repository; the banks are cooked
from retail data and are not committed.
