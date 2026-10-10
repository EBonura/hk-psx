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
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../data/battle_gates.rs"
));
// `Floor Control`'s floor and `FK Armour`, cooked by host/false_knight_art.py.
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../data/false_knight_floor.rs"
));
/// Rows of a `(slot, value)` table for one catalogue slot.
fn slot_rows(table: &'static [(u16, u16)], slot: usize) -> impl Iterator<Item = u16> {
    let slot = slot as u16;
    let first = table.partition_point(|&(row, _)| row < slot);
    table[first..]
        .iter()
        .take_while(move |&&(row, _)| row == slot)
        .map(|&(_, value)| value)
}

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
/// The scene (plus one) whose arena a `BG CLOSE` sealed and no open has
/// lifted: a Battle Control's `CameraLockArea B` is live for exactly that span
/// (camera.rs, `LOCK_BATTLE`).
static mut FIGHTING: u8 = 0;
/// Whether `scene`'s arena is sealed for a fight.
pub fn fighting(scene: usize) -> bool {
    unsafe { FIGHTING as usize == scene + 1 }
}

/// The clip each gate is showing, `CLIP_OPENED` ... `CLIP_OPEN` (the index
/// into the generated `CLIP_FRAMES`), and the gate clock when it started.
/// `BG Control` plays a clip in every state it enters, so this is the state as
/// far as anything visible goes; `CLOSED` stays the collider's truth.
pub const CLIP_OPENED: u8 = 0;
pub const CLIP_CLOSE_1: u8 = 1;
pub const CLIP_CLOSE_2: u8 = 2;
pub const CLIP_CLOSED: u8 = 3;
pub const CLIP_OPEN: u8 = 4;
/// The slam effect's `BG Effect`, started by `Close 2`.
pub const CLIP_EFFECT: u8 = 5;
static mut POSE: [u8; GATES] = placement_poses();
static mut POSE_START: [u32; GATES] = [0; GATES];
/// Simulation ticks, advanced by `tick`: the clips' clock.
static mut CLOCK: u32 = 0;
/// The pose `Opened` leaves a gate in on load: `Quick Close` plays `BG Closed`
/// for a gate that starts closed, and the rest hold `BG Opened`.
const fn placement_poses() -> [u8; GATES] {
    let mut poses = [CLIP_OPENED; GATES];
    let mut i = 0;
    while i < GATES {
        if PLACEMENT_CLOSED & (1 << i) != 0 {
            poses[i] = CLIP_CLOSED;
        }
        i += 1;
    }
    poses
}
/// Start `clip` on every gate in `mask`. Out of line and bit-walked rather
/// than a loop over all `GATES`, which LLVM unrolled into every caller.
#[inline(never)]
fn set_poses(mut mask: u16, clip: u8) {
    while mask != 0 {
        let gate = mask.trailing_zeros() as usize;
        unsafe {
            POSE[gate] = clip;
            POSE_START[gate] = CLOCK;
        }
        if clip == CLIP_CLOSE_1 {
            unsafe { POSE_MASK |= 1 << gate }
        } else {
            unsafe { POSE_MASK &= !(1 << gate) }
        }
        mask &= mask - 1;
    }
}
/// One simulation tick of the gates' clips. The tick a gate's `Close 1` hands
/// over to `Close 2` is that state's entry, which sends `EnemyKillShake` to
/// the camera; every gate of the arena gets there on the same tick, and the
/// shake's own priority test drops the repeats as the original's does.
pub fn tick() {
    unsafe { CLOCK = CLOCK.wrapping_add(1) }
    let mut bits = unsafe { POSE_MASK };
    while bits != 0 {
        let gate = bits.trailing_zeros() as usize;
        bits &= bits - 1;
        if unsafe {
            POSE[gate] == CLIP_CLOSE_1 && CLOCK.wrapping_sub(POSE_START[gate]) == CLOSE_1_TICKS
        } {
            crate::camera::request(crate::camera::Shake::Kill);
        }
    }
}
/// Gates whose pose changed since load and might still be in `Close 1`, so
/// `tick` costs one test outside the five arena rooms.
static mut POSE_MASK: u16 = 0;
/// `BG Close 1`'s length in ticks: its frames at its own rate.
const CLOSE_1_TICKS: u32 = {
    let fps = CLIP_FPS[CLIP_CLOSE_1 as usize] as u32;
    if fps == 0 {
        0
    } else {
        (CLIP_FRAMES[CLIP_CLOSE_1 as usize].len() as u32 * 60).div_ceil(fps)
    }
};
/// The clip a gate shows and how many ticks it has been playing. `Close 1`
/// hands over to `Close 2` when its clip finishes (`Tk2dPlayAnimationWithEvents`
/// sends FINISHED on the clip's last frame), which is the one transition the
/// FSM takes on its own; every other clip holds its last frame.
pub fn pose(gate: usize) -> (u8, u32) {
    let (clip, age) = unsafe { (POSE[gate], CLOCK.wrapping_sub(POSE_START[gate])) };
    if clip == CLIP_CLOSE_1 && age >= CLOSE_1_TICKS {
        return (CLIP_CLOSE_2, age - CLOSE_1_TICKS);
    }
    (clip, age)
}
/// The gate sprite (an index into `SPRITE_RECT`) a clip shows at an age. Ages
/// past a few minutes only ever read a held last frame, so they are clamped
/// before the multiply rather than widened to 64 bits.
pub fn clip_frame(clip: u8, age: u32) -> usize {
    let frames = CLIP_FRAMES[clip as usize];
    let index = (age.min(1 << 16) * CLIP_FPS[clip as usize] as u32 / 60) as usize;
    frames[index.min(frames.len() - 1)] as usize
}
/// The sprite one gate shows right now, and its slam effect's while `Close 2`
/// is playing it: `BG Effect` once at its own rate, then nothing (the clip's
/// last sprite is a dot host/battle_gates.py leaves out).
#[inline(never)]
pub fn sprites(gate: usize) -> (usize, Option<usize>) {
    let (clip, age) = pose(gate);
    let effect = if clip == CLIP_CLOSE_2 {
        let frames = CLIP_FRAMES[CLIP_EFFECT as usize];
        let index = (age.min(1 << 16) * CLIP_FPS[CLIP_EFFECT as usize] as u32 / 60) as usize;
        frames.get(index).map(|&f| f as usize)
    } else {
        None
    };
    (clip_frame(clip, age), effect)
}
/// The gates of one scene, for drawing.
pub fn scene_gates(scene: usize) -> u16 {
    scene_mask(scene)
}

/// The gates standing right now, as the bit mask above. Without this "the arena
/// sealed" is a claim no replay can check: a gate draws nothing, so the only
/// evidence a route could otherwise offer is the Knight failing to walk
/// somewhere, which a dozen other faults produce.
#[no_mangle]
pub static mut HK_ARENA_GATES: u32 = 0;
/// `Floor Control` on the False Knight's arena floor: 0 whole, 1 cracked
/// (`Crack`), 2 broken (`Break`, or `Activate` on a won arena).
#[no_mangle]
pub static mut HK_FK_FLOOR: u32 = 0;
pub const FLOOR_WHOLE: u8 = 0;
pub const FLOOR_CRACKED: u8 = 1;
pub const FLOOR_BROKEN: u8 = 2;
/// The arena floor's state and whether `FK Armour` stands. Both belong to the
/// one scene with a `Floor Control`, and a scene load recreates both from the
/// save, so one value of each is the whole state.
static mut FLOOR: u8 = FLOOR_WHOLE;
static mut ARMOUR: bool = false;
/// `BG CLOSE` broadcasts answered, and `BG OPEN`/`BG QUICK OPEN` answered.
#[no_mangle]
pub static mut HK_ARENA_GATE_CLOSES: u32 = 0;
#[no_mangle]
pub static mut HK_ARENA_GATE_OPENS: u32 = 0;

fn scene_mask(scene: usize) -> u16 {
    let mut found = 0;
    for &(id, mask) in SCENE_GATES {
        if id as usize == scene {
            found |= mask;
        }
    }
    found
}
/// Mirror the live mask in one place: every entry point below moves it, and
/// publishing at each of them is how one of them ends up not publishing.
fn publish() {
    unsafe {
        HK_ARENA_GATES = CLOSED as u32;
        HK_FK_FLOOR = FLOOR as u32
    }
    crate::world::scripted_terrain_changed();
}
/// `Battle Control`'s `Pause` -> `Init`, run when the arena's scene seats its
/// actors. A scene load recreates every `BG Control` at its placement state, so
/// the room's gates go back to that first, whatever the last visit left them
/// in; then `Init` quick-opens them if the arena is already won.
pub fn arena_entry(scene: usize, activated: bool) {
    let mask = scene_mask(scene);
    unsafe {
        CLOSED = (CLOSED & !mask) | (PLACEMENT_CLOSED & mask);
        FIGHTING = 0
    }
    set_poses(mask & PLACEMENT_CLOSED, CLIP_CLOSED);
    set_poses(mask & !PLACEMENT_CLOSED, CLIP_OPENED);
    // `Floor Control`'s `Check Broken` asks `falseKnightDefeated`, and
    // `Battle Control`'s `Init` destroys `FK Armour` unless the arena was won.
    if scene == FK_FLOOR_SCENE {
        let defeated = activated || crate::persist::player(crate::persist::FALSE_KNIGHT_DEFEATED);
        unsafe {
            FLOOR = if defeated { FLOOR_BROKEN } else { FLOOR_WHOLE };
            ARMOUR = activated;
        }
    }
    if activated {
        // `Quick Open` plays `BG Opened` and lifts the collider, silently.
        set_poses(mask, CLIP_OPENED);
        lift(scene);
    } else {
        publish();
    }
}
/// `CRACK`: the last slam of the second rage.
pub fn floor_crack() {
    unsafe {
        if FLOOR == FLOOR_WHOLE {
            FLOOR = FLOOR_CRACKED;
        }
    }
    publish();
}
/// `DESTROY`: the death jump comes down through it. The terrain goes in the
/// same simulation tick (`world::scripted_terrain_changed`).
pub fn floor_break() {
    unsafe {
        FLOOR = FLOOR_BROKEN;
    }
    publish();
}
pub fn floor() -> u8 {
    unsafe { FLOOR }
}
/// Whether a nail-bounce box is `FK Armour`'s `Tinger` while the armour is
/// gone: `Battle Control`'s `Init` destroys the armour, bounce box and all,
/// unless the arena was already won.
pub fn armour_tink_hidden(bounds: [i32; 4]) -> bool {
    // The world bank rounds a box its own way; a sixteenth of a unit is far
    // below anything two different bounce boxes could share.
    const SLACK: i32 = 4096;
    !unsafe { ARMOUR }
        && FK_ARMOUR_TINK
            .iter()
            .any(|b| (0..4).all(|i| (b[i] - bounds[i]).abs() <= SLACK))
}
/// `BG CLOSE`, broadcast to every gate in the room when the hero crosses the
/// trigger. This is the direction that seals an arena: the gate's edges are in
/// the pack, `apply` was lifting them, and clearing the bit stops. A gate with
/// no row in `COOKED` is still counted as closed, because the arena's own state
/// is what the counter reports; `COOKED` is what says whether it moved terrain.
pub fn close(scene: usize) {
    crate::scene_sfx::play(crate::scene_sfx::GATE_SLAM);
    // Only a gate in `Opened`, `Open` or `Quick Open` has a BG CLOSE
    // transition; one already in `Quick Close`, `Double Close` or `Close 2`
    // stays as it is.
    set_poses(scene_mask(scene) & !unsafe { CLOSED }, CLIP_CLOSE_1);
    unsafe {
        CLOSED |= scene_mask(scene);
        FIGHTING = scene as u8 + 1;
        HK_ARENA_GATE_CLOSES = HK_ARENA_GATE_CLOSES.saturating_add(1);
    }
    publish();
}
/// `BG OPEN` two seconds after the last enemy dies, and `BG QUICK OPEN` on
/// entering an arena that was already won.
pub fn open(scene: usize) {
    crate::scene_sfx::play(crate::scene_sfx::GATE_OPEN);
    // BG OPEN is answered by the closed states only (`Close 2`, `Quick Close`,
    // `Double Close`); `Open` plays `BG Open` and holds its last frame.
    set_poses(scene_mask(scene) & unsafe { CLOSED }, CLIP_OPEN);
    lift(scene);
}
/// The open itself; `BG QUICK OPEN` on entering a won arena is silent.
fn lift(scene: usize) {
    unsafe {
        CLOSED &= !scene_mask(scene);
        FIGHTING = 0;
        HK_ARENA_GATE_OPENS = HK_ARENA_GATE_OPENS.saturating_add(1);
    }
    publish();
}
/// The development reset, back to every gate's placement state.
pub fn reset() {
    unsafe {
        CLOSED = PLACEMENT_CLOSED;
        POSE = placement_poses();
        POSE_START = [0; GATES];
        POSE_MASK = 0;
        FIGHTING = 0;
        FLOOR = FLOOR_WHOLE;
        ARMOUR = false;
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
/// `region` is the Knight's view (its terrain), `view` the one being drawn
/// (its cooked draws): they differ while a camera lock or the follow lag keeps
/// the camera in a neighbour, and a draw index is only meaningful in its own
/// view's draw list.
pub fn apply(state: &mut crate::world::State, region: usize, view: usize) {
    // The arena floor first: its draws are per-frame state, re-applied after
    // every visibility reset like the Geo rocks' and the cocoon's.
    let floor = unsafe { FLOOR };
    if floor != FLOOR_WHOLE {
        for draw in slot_rows(&FK_FLOOR_NORMAL_DRAWS, view) {
            crate::render::set_visible(draw as usize, false);
        }
    }
    if floor == FLOOR_BROKEN {
        for edge in slot_rows(&FK_BREAK_FLOOR_EDGES, region) {
            state.append_script_edges(&[edge]);
        }
    }
    if !unsafe { ARMOUR } {
        for draw in slot_rows(&FK_ARMOUR_DRAWS, view) {
            crate::render::set_visible(draw as usize, false);
        }
    }
    let lifted = unsafe { !CLOSED } & COOKED;
    if lifted == 0 {
        return;
    }
    let slot = region as u16;
    let first = REGIONS.partition_point(|&(row, _, _)| row < slot);
    for &(row, gate, edges) in &REGIONS[first..] {
        if row != slot {
            break;
        }
        if lifted & (1 << gate) != 0 {
            state.append_script_edges(edges);
        }
    }
}
