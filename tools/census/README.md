# Original feature evidence census

Standalone Rust tools; all generated retail-source archives stay outside Git.

`cargo run --manifest-path tools/census/Cargo.toml --bin hk-feature-census -- ORIGINAL_WINDOWS_DATA OUTPUT`

`cargo run --manifest-path tools/census/Cargo.toml --bin report -- ORIGINAL_OUTPUT COOKED_PORT_ROOT REPORT_OUTPUT AUDITED_REGISTRY_JSON`

`cargo run --manifest-path tools/census/Cargo.toml --bin semantic -- ORIGINAL_OUTPUT REPORT_OUTPUT`

`cargo test --manifest-path tools/census/Cargo.toml --bin hk-feature-census`

The original reader requires the supported Windows Unity installation. Every BuildSettings scene plus resources.assets is read independently, preserving source IDs and raw object hashes. Semantic fields are explicitly typed; generic name matching never establishes implementation. Source component counts and semantic family counts use different units. Sharedasset prefabs, graph reachability and encounter aliases require further audit; semantic_census_complete remains false.

The registry records a reviewed original source contract and implementation subset. Both original and port hashes must match on rerun. Journey exercise claims are disabled until structured source-contract, disc and journey hashes plus exact assertion PASS and matching FAIL exclusion can be validated. Aggregate hits, file names and scene visits are insufficient. Missing evidence means unknown rather than implicit support.

Report and semantic readers accept a generated `source-objects.jsonl` or stream `source-objects.jsonl.gz` through the system gzip tool. Every compressed read checks the child's exit status and rejects corrupt or truncated input even if complete rows were produced before the error. No full archive expansion is needed. Scanner output stays plain; compress that generated cache only after recording both compressed SHA and decompressed SHA and validating a round trip. Retail Unity files remain read only. Report contract output replaces the prior complete file only after checked input EOF; a failed read leaves a `.partial` file and returns an error.

`semantic ORIGINAL_OUTPUT REPORT_OUTPUT AUTHORED_GRAPH_DIRECTORY` adds targeted source graphs to the streamed semantic catalogue. Generate graph files with `fsm-graph ORIGINAL_WINDOWS_DATA FILE:PATH_ID OUTPUT_JSON`; no full source rescan or cache expansion is needed. Each graph source ID and raw-object SHA must match the existing cached row. Duplicate, absent or stale source bindings fail the run. Graph artifact hashes are recorded. Only explicitly bound immutable instance choices and unique FINISHED transitions select authored writes; external events and alternative edges stop the trace. Queen/King fragment branches remain instance-specific. Additional authored-write units never remove existing unknown queue rows, prove runtime reachability, imply port implementation or grant journey credit. Mantis Claw's hasWalljump is one ability contract; canWallJump is a supporting flag.
