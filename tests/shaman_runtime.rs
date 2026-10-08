//! Native run of the Snail Shaman's state machine (`game/src/shaman.rs`)
//! against the real PlayerData bit numbers. The guest half (drawing, the
//! static, the linked frames) is out of the test build.
#![allow(dead_code)]
#[path = "../game/src/persist.rs"]
mod persist;
mod shade {
    #[derive(Clone, Copy)]
    pub struct Clip { pub start: usize, pub count: usize, pub fps: u32, pub wrap: u8 }
}
#[path = "../game/src/shaman.rs"]
mod shaman;
