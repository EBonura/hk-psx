# fsm-exp: FSM v2 guest experiments

Host-built MIPS guest used by the truth pass (docs/FSM-V2-DESIGN.md, step 0):

* the resident HKFSM03 kernel (`src/kernel.rs`), run on the real Hornet 1 bank
  (written by `fsm3-sizes`, never committed: it is cooked from the original data) with stub handlers, and thirteen
  real handlers (`src/families.rs`) to measure bytes per action type;
* dispatch-cost micro benchmarks: direct call, `jalr` through a 14-word table, 14-arm
  `match`, two-level table;
* the same image goes through the SDK's `hazard-patch` and `hazard-scan` with its link map.

`run.sh` builds it the way the SDK examples are built (same delay-slot flags, same linker
script), runs the hazard tools, sizes the kernel from the map and runs the image in the
headless PSoXide frontend. Needs the hydrated `.psoxide` and a built frontend. Results are
emulator cycles (the calibrated model), not silicon.
