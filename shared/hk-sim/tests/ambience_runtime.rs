//! The ambience mixer and bank checks run against the cooked manifest
//! (`data/ambience.rs`, written by `cargo hk-build build`), so they only build
//! once a cook has run in this checkout; `build.rs` sets `cooked_data` then.
#[cfg(cooked_data)]
#[path = "ambience_runtime/cooked.rs"]
mod cooked;

#[cfg(not(cooked_data))]
#[test]
#[ignore = "skipped: data/ambience.rs is not cooked in this checkout (run `cargo hk-build build`)"]
fn ambience_runtime_needs_cooked_data() {}
