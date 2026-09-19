//! Source `BG Control` arena gates, as terrain the cooked room already holds.
//!
//! A gate is a box collider on the terrain layer with an FSM whose whole
//! observable behaviour is one boolean: `Opened` tests `Start Closed`, and a
//! gate that carries it turns its box back on in `Quick Close` and then waits
//! for the `BG OPEN` its `Battle Scene` sends when the arena is won.
//! `host/cook.py` bakes every gate into the room packs, whichever way that test
//! goes, so this module never has to place a gate: it only has to take one
//! away, which is the same exclusion `great_door.rs` uses for the door it
//! breaks through, and `host/battle_gates.py` is the cooked join.
//!
//! That is why the initial exclusion set is not empty. `CLOSED` starts at
//! `PLACEMENT_CLOSED`, so the first `apply` lifts `!PLACEMENT_CLOSED & COOKED`:
//! every gate the source has open from the first frame, twelve of the fifteen.
//! Each of those is terrain in the pack and an invisible wall without the
//! exclusion, so this is the load-bearing line in the file. It also buys the
//! thing the old shape could not do: because the open gates are terrain, a
//! `BG CLOSE` can put them back and an arena can seal at its ends.
//!
//! The state is a `static mut` rather than a field of `frame::Game` because the
//! arena that drives it rides on the boss actor, several layers inside
//! `enemies.rs`, and threading a world down there would have cost more call
//! sites than the whole feature is worth.
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/battle_gates.rs"));

const _: () = assert!(GATES <= 16, "the closed set is one u16");
// There is deliberately no `COOKED & !PLACEMENT_CLOSED == 0` here any more.
// That assertion encoded the cook baking only the gates that load closed, and
// inverting it would be worse than useless: a gate legitimately carries no row
// when no edge of it survives a region's collision-bounds cull, and asserting
// otherwise would fail a whole-world recook over a gate that moves nothing.
// `host/battle_gates.py` reports those gates by name instead.

/// Bit per gate, set while the gate's collider is solid. Starts at the state
/// `BG Control`'s own start state leaves each gate in, which for twelve of the
/// fifteen is open, so `apply` has edges to lift from the first frame.
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
/// trigger. This is the direction that seals an arena: the gate's edges are in
/// the pack, `apply` was lifting them, and clearing the bit stops. A gate with
/// no row in `COOKED` is still counted as closed, because the arena's own state
/// is what the counter reports; `COOKED` is what says whether it moved terrain.
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
/// Called once per region activation and once per drawn frame. The early test
/// is still the cheapest one there is, but it no longer means "nothing to do
/// until an arena is won": twelve of the fifteen gates load open, so in the
/// five rooms that have gates this does its work from the first frame, and
/// only a room without any is free. A `BG CLOSE` is what makes it stop.
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
