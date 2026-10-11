# Original feature evidence census

Standalone Rust tools; all generated retail-source archives stay outside Git.

`cargo run --manifest-path tools/census/Cargo.toml --bin hk-feature-census -- ORIGINAL_WINDOWS_DATA OUTPUT`

`cargo run --manifest-path tools/census/Cargo.toml --bin report -- ORIGINAL_OUTPUT COOKED_PORT_ROOT REPORT_OUTPUT AUDITED_REGISTRY_JSON`

`cargo run --manifest-path tools/census/Cargo.toml --bin semantic -- ORIGINAL_OUTPUT REPORT_OUTPUT`

`cargo test --manifest-path tools/census/Cargo.toml --bin hk-feature-census`

The original reader requires the supported Windows Unity installation. Every BuildSettings scene plus resources.assets is read independently, preserving source IDs and raw object hashes. Semantic fields are explicitly typed; generic name matching never establishes implementation. Source component counts and semantic family counts use different units. Sharedasset prefabs, graph reachability and encounter aliases require further audit; semantic_census_complete remains false.

The registry records a reviewed original source contract and implementation subset. Both original and port hashes must match on rerun. Journey exercise claims are disabled until structured source-contract, disc and journey hashes plus exact assertion PASS and matching FAIL exclusion can be validated. Aggregate hits, file names and scene visits are insufficient. Missing evidence means unknown rather than implicit support.

Report and semantic readers accept a generated `source-objects.jsonl` or stream `source-objects.jsonl.gz` through the system gzip tool. Every compressed read checks the child's exit status and rejects corrupt or truncated input even if complete rows were produced before the error. No full archive expansion is needed. Scanner output stays plain; compress that generated cache only after recording both compressed SHA and decompressed SHA and validating a round trip. Retail Unity files remain read only. Report contract output replaces the prior complete file only after checked input EOF; a failed read leaves a `.partial` file and returns an error.
