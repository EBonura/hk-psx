//! Native run of Cornifer's conversation (`game/src/mapper.rs`) against the
//! real PlayerData bit numbers in `game/src/persist.rs`. The guest half of the
//! module (the static conversation, its drawing and telemetry) is out of the
//! test build; the state machine it drives is all here.
#![allow(dead_code)]
#[path = "../game/src/persist.rs"]
mod persist;
#[path = "../game/src/mapper.rs"]
mod mapper;
