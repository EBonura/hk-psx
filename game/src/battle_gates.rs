//! Source `BG Control` arena gates, as terrain the cooked room already holds.
//!
//! A gate is a box collider on the terrain layer with an FSM whose whole
//! observable behaviour is one boolean: `Opened` tests `Start Closed`, and a
//! gate that carries it turns its box back on in `Quick Close` and then waits
//! for the `BG OPEN` its `Battle Scene` sends when the arena is won.
//! `host/cook.py` bakes exactly those gates into the room packs and skips the
//! rest, because a gate the source has open from the first frame would be an
//! invisible wall. So this module never has to place a gate: it only has to
//! take one away, which is the same exclusion `great_door.rs` uses for the door
//! it breaks through, and `host/battle_gates.py` is the cooked join.
//!
//! The state is a `static mut` rather than a field of `frame::Game` because the
//! arena that drives it rides on the boss actor, several layers inside
//! `enemies.rs`, and threading a world down there would have cost more call
//! sites than the whole feature is worth.
//!
//! What this cannot do is seal an arena. Three of Crossroads_10's five gates,
//! including both arena ends, are open on load and therefore carry no terrain
//! at all, so `BG CLOSE` has nothing to restore. Closing them needs the cook to
//! bake every gate and this table to carry the open ones too, which is a
//! whole-world recook: see host/battle_gates.py.
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/battle_gates.rs"));

const _: () = assert!(GATES <= 16, "the closed set is one u16");
const _: () = assert!(COOKED & !PLACEMENT_CLOSED == 0, "the cook bakes only gates that load closed");

/// Bit per gate, set while the gate's collider is solid. Starts at the state
/// `BG Control`'s own start state leaves each gate in.
static mut CLOSED: u16 = PLACEMENT_CLOSED;

/// The gates standing right now, as the bit mask above. Without this "the arena
/// sealed" is a claim no replay can check: a gate draws nothing, so the only
/// evidence a route could otherwise offer is the Knight failing to walk
/// somewhere, which a dozen other faults produce.
#[no_mangle] pub static mut HK_ARENA_GATES: u32 = 0;
/// `BG CLOSE` broadcasts answered, and `BG OPEN`/`BG QUICK OPEN` answered.
#[no_mangle] pub static mut HK_ARENA_GATE_CLOSES: u32 = 0;
#[no_mangle] pub static mut HK_ARENA_GATE_OPENS: u32 = 0;

fn scene_mask(scene: usize) -> u16 {
    let mut found = 0;
    for &(id, mask) in SCENE_GATES {
        if id as usize == scene { found |= mask; }
    }
    found
}
/// Mirror the live mask in one place: every entry point below moves it, and
/// publishing at each of them is how one of them ends up not publishing.
fn publish() {
    unsafe { HK_ARENA_GATES = CLOSED as u32 }
}
/// `Battle Control`'s `Pause` -> `Init`, run when the arena's scene seats its
/// actors. A scene load recreates every `BG Control` at its placement state, so
/// the room's gates go back to that first, whatever the last visit left them
/// in; then `Init` quick-opens them if the arena is already won.
pub fn arena_entry(scene: usize, activated: bool) {
    let mask = scene_mask(scene);
    unsafe { CLOSED = (CLOSED & !mask) | (PLACEMENT_CLOSED & mask) }
    if activated { open(scene); } else { publish(); }
}
/// `BG CLOSE`, broadcast to every gate in the room when the hero crosses the
/// trigger. A gate the cook left out of the terrain is still counted as closed,
/// because the arena's own state is what the counter reports; `COOKED` is what
/// says whether closing it moved anything.
pub fn close(scene: usize) {
    unsafe {
        CLOSED |= scene_mask(scene);
        HK_ARENA_GATE_CLOSES = HK_ARENA_GATE_CLOSES.saturating_add(1);
    }
    publish();
}
/// `BG OPEN` two seconds after the last enemy dies, and `BG QUICK OPEN` on
/// entering an arena that was already won.
pub fn open(scene: usize) {
    unsafe {
        CLOSED &= !scene_mask(scene);
        HK_ARENA_GATE_OPENS = HK_ARENA_GATE_OPENS.saturating_add(1);
    }
    publish();
}
/// The development reset, back to every gate's placement state.
pub fn reset() {
    unsafe {
        CLOSED = PLACEMENT_CLOSED;
        HK_ARENA_GATE_CLOSES = 0;
        HK_ARENA_GATE_OPENS = 0;
    }
    publish();
}
/// Drop the edges of every open gate from this view's terrain, after the
/// Lifeblood refresh and beside the Great Door's own exclusions.
///
/// Called once per region activation and once per drawn frame, so the first
/// thing it does is the cheapest test there is: until an arena is won, every
/// gate the cook baked is standing and the room's terrain is the pack's.
pub fn apply(state: &mut crate::world::State, region: usize) {
    let lifted = unsafe { !CLOSED } & COOKED;
    if lifted == 0 { return; }
    let slot = region as u16;
    let first = REGIONS.partition_point(|&(row, _, _)| row < slot);
    for &(row, gate, edges) in &REGIONS[first..] {
        if row != slot { break; }
        if lifted & (1 << gate) != 0 { state.append_script_edges(edges); }
    }
}
