//! Native run of the Toll Machines' box (`game/src/toll.rs`) against the cooked table (`data/toll.rs`). The
//! guest half of the module (the one box, its telemetry and its drawing) is out of the test build; the state
//! machine it drives is all here.
#![allow(dead_code)]
#[path = "../game/src/toll.rs"]
mod toll;
