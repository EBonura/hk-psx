//! Soul totems (host/soul_totems.py): a nail hit takes one of the totem's
//! `Value` and pays 8 or 9 soul orbs of `AddMPCharge(2)` each, then the totem
//! waits 0.25 s; at zero it is depleted. The count left rides the world store
//! (kind `SoulTotem`) because the source's `PersistentIntItem` does, and like
//! that item's `semiPersistent` flag every totem refills when the Knight rests.
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../data/soul_totems.rs"
));
use crate::persist::{self, Kind};
use hk_sim::polygon_hits_box;

const N: usize = TOTEMS.len();
static mut COOLDOWN: [u8; N] = [0; N];
static mut SWING: [u32; N] = [u32::MAX; N];
static mut RANDOM: u32 = 0x544f5445;
/// Nail hits a totem paid for, and the SOUL they paid.
#[no_mangle]
pub static mut HK_TOTEM_HITS: u32 = 0;
#[no_mangle]
pub static mut HK_TOTEM_SOUL: u32 = 0;

fn left(t: &Totem) -> u8 {
    persist::get(Kind::SoulTotem, t.scene as usize, t.local as usize).unwrap_or(t.hits)
}
/// One nail swing's polygon against the totems of `scene`; `swing` is the
/// attack counter, so one swing pays a totem once however long it overlaps.
/// Returns the SOUL earned this tick.
pub fn strike(scene: usize, polygon: &[[i32; 2]], swing: u32) -> u16 {
    let mut soul = 0;
    for (i, t) in TOTEMS.iter().enumerate() {
        if t.scene as usize != scene || !polygon_hits_box(polygon, t.bounds) {
            continue;
        }
        let remaining = left(t);
        unsafe {
            if remaining == 0 || COOLDOWN[i] != 0 || SWING[i] == swing {
                continue;
            }
            COOLDOWN[i] = t.wait_ticks;
            SWING[i] = swing;
            RANDOM = RANDOM.wrapping_mul(1664525).wrapping_add(1013904223);
            let span = (t.orbs[1] - t.orbs[0]) as u32 + 1;
            let orbs = t.orbs[0] as u16 + ((RANDOM >> 16) % span) as u16;
            soul += orbs * SOUL_PER_ORB;
            HK_TOTEM_HITS = HK_TOTEM_HITS.saturating_add(1);
            HK_TOTEM_SOUL = HK_TOTEM_SOUL.saturating_add(u32::from(orbs * SOUL_PER_ORB));
        }
        persist::set(
            Kind::SoulTotem,
            t.scene as usize,
            t.local as usize,
            remaining - 1,
        );
    }
    soul
}
/// Once per simulation tick: the `Wait` after each hit.
pub fn tick() {
    unsafe {
        for c in (*(&raw mut COOLDOWN)).iter_mut() {
            *c = c.saturating_sub(1);
        }
    }
}
/// A bench rest: every `semiPersistent` totem is full again.
pub fn rest() {
    persist::store().clear_kind(Kind::SoulTotem);
    persist::publish();
}
