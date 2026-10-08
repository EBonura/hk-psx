//! The real arena-gate module, compiled against the real cooked table.
//!
//! The thing worth testing here is not the bit arithmetic, it is the join: a
//! gate binding that names the wrong edge index silently changes a room's
//! terrain, and the failure a player sees is a wall where there is no wall, or
//! a hole where there should be one. Neither shows up in a build log. So this
//! drives `data/battle_gates.rs` itself rather than a fixture, and asserts what
//! the guest would exclude for every catalogue slot in it.
//!
//! `game/src/world.rs` is a thousand lines of terrain cache that this module
//! touches through exactly one method, so the stub below is that method.
#![allow(dead_code)]
pub mod world {
    /// `State::append_script_edges`: the bounded scratch the Lifeblood cocoons,
    /// the Great Door and the gates share, skipping duplicates the way the real
    /// one does so an over-budget binding shows up as an overflow here too.
    pub const SCRIPT_EDGE_SLOTS: usize = 20;
    /// `world::scripted_terrain_changed`: the guest re-applies scripted
    /// terrain in the same tick; nothing here moves terrain, so it is a no-op.
    pub fn scripted_terrain_changed() {}
    pub struct State {
        pub edges: Vec<u16>,
    }
    impl State {
        pub fn new() -> Self {
            Self { edges: Vec::new() }
        }
        pub fn append_script_edges(&mut self, edges: &[u16]) {
            for &edge in edges {
                if self.edges.contains(&edge) {
                    continue;
                }
                assert!(self.edges.len() < SCRIPT_EDGE_SLOTS, "scripted exclusion scratch overflowed");
                self.edges.push(edge);
            }
        }
    }
}
/// The gates' slam and open sounds (game/src/scene_sfx.rs), counted.
mod scene_sfx {
    pub const GATE_SLAM: u8 = 0;
    pub const GATE_OPEN: u8 = 1;
    pub static PLAYS: std::sync::Mutex<Vec<u8>> = std::sync::Mutex::new(Vec::new());
    pub fn play(event: u8) { PLAYS.lock().unwrap().push(event); }
}
/// The floor state hides two scenery draws and keeps `FK Armour` hidden until
/// the arena is won; which draws is the cooked table's business, not this one's.
pub mod render {
    pub fn set_visible(_: usize, _: bool) {}
}
/// `Close 2` sends `EnemyKillShake`; counted, so the test sees it once per close.
pub mod camera {
    pub enum Shake { Kill }
    pub static SHAKES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    pub fn request(_: Shake) { SHAKES.fetch_add(1, std::sync::atomic::Ordering::Relaxed); }
}
pub mod persist {
    pub const FALSE_KNIGHT_DEFEATED: u32 = 0;
    pub fn player(_: u32) -> bool { false }
}
#[path = "../game/src/battle_gates.rs"]
mod battle_gates;
use battle_gates::*;

/// Every catalogue slot the table binds, and what it excludes right now.
fn excluded(slot: usize) -> Vec<u16> {
    let mut state = world::State::new();
    battle_gates::apply(&mut state, slot, slot);
    state.edges
}
fn bound_slots() -> Vec<usize> {
    let mut slots: Vec<usize> = REGIONS.iter().map(|&(slot, _, _)| slot as usize).collect();
    slots.dedup();
    slots
}
fn gates_of(slot: usize) -> Vec<u8> {
    REGIONS.iter().filter(|&&(row, _, _)| row as usize == slot).map(|&(_, gate, _)| gate).collect()
}
fn scene_of(gate: u8) -> usize {
    SCENE_GATES.iter().find(|&&(_, mask)| mask & (1 << gate) != 0).expect("every gate has a scene").0 as usize
}
/// What a slot must exclude for a given closed mask: the edges of every gate
/// the table binds there whose bit is clear, in the order `apply` appends them.
/// Written out of the table rather than repeating `apply`'s arithmetic, so a
/// test that passes says the two agree about which gate owns which edge.
fn expected(slot: usize, closed: u16) -> Vec<u16> {
    REGIONS.iter()
        .filter(|&&(row, gate, _)| row as usize == slot && closed & (1 << gate) == 0)
        .flat_map(|&(_, _, edges)| edges.iter().copied())
        .collect()
}

fn main() {
    // The table itself, before anything runs against it.
    assert!(GATES <= 16, "the closed set is one u16");
    // There is deliberately no `COOKED & !PLACEMENT_CLOSED == 0` assertion here
    // any more. `host/cook.py` bakes every gate, so most of COOKED is gates the
    // source loads open, and those are precisely the edges a fresh session has
    // to lift. The fresh-session block below is what replaced this check.
    let every_gate: u16 = if GATES == 16 { u16::MAX } else { (1 << GATES) - 1 };
    let mut seen = 0u16;
    for &(_, mask) in SCENE_GATES {
        assert_eq!(seen & mask, 0, "a gate belongs to one scene");
        seen |= mask;
    }
    assert_eq!(seen, every_gate, "every gate belongs to a scene");
    assert!(REGIONS.windows(2).all(|w| (w[0].0, w[0].1) < (w[1].0, w[1].1)),
            "REGIONS must be sorted by slot then gate for the binary search in apply()");
    let mut bound = 0u16;
    for &(_, gate, edges) in REGIONS {
        assert!(!edges.is_empty(), "a binding with no edges is a row that should not exist");
        bound |= 1 << gate;
    }
    assert_eq!(bound, COOKED, "COOKED must be exactly the gates REGIONS binds");

    // A fresh session is the cooked world with every gate the source loads open
    // lifted out of it, and nothing else. This is the assertion that matters
    // most, and since the recook it can go wrong in both directions. Lift too
    // little and a gate the source has open from the first frame is an invisible
    // wall no sprite explains, in five rooms, one of which is cut between its
    // only two doors. Lift too much and one of the three gates that do load
    // closed stops being terrain, opening a route the source does not have.
    reset();
    for slot in bound_slots() {
        assert_eq!(excluded(slot), expected(slot, PLACEMENT_CLOSED),
                   "slot {slot} on load");
    }

    // One scene at a time, because the source broadcast is per room: winning
    // Crossroads_10 must not open a gate in Crossroads_04, and must not shut one
    // either. Every other scene's gates stay at their placement state, so the
    // slots outside this room keep lifting whatever they lifted on load.
    for &(scene, mask) in SCENE_GATES {
        reset();
        arena_entry(scene as usize, true);
        assert_eq!(unsafe { HK_ARENA_GATES } as u16, PLACEMENT_CLOSED & !mask);
        for slot in bound_slots() {
            // Winning the False Knight's arena also breaks its floor, whose
            // edges `apply` lifts ahead of the gates'.
            let mut want: Vec<u16> = if scene as usize == FK_FLOOR_SCENE {
                FK_BREAK_FLOOR_EDGES.iter().filter(|&&(row, _)| row as usize == slot).map(|&(_, e)| e).collect()
            } else { Vec::new() };
            want.extend(expected(slot, PLACEMENT_CLOSED & !mask));
            assert_eq!(excluded(slot), want, "slot {slot} after scene {scene} was won");
        }
        // And every slot of the room holds floor and gates together.
        for &(slot, _) in FK_BREAK_FLOOR_EDGES.iter() { excluded(slot as usize); }
    }

    // The direction the whole recook was for: the arena's `BG CLOSE` has to put
    // terrain back, not merely leave it alone. A gate that loads open is lifted
    // on the first frame and must stop being lifted once the trigger fires, or
    // the arena has no ends.
    reset();
    for &(scene, _) in SCENE_GATES { close(scene as usize); }
    for slot in bound_slots() {
        assert!(excluded(slot).is_empty(), "slot {slot} still lifts terrain after BG CLOSE");
    }

    // The fight itself: the trigger shuts every gate in the room, the win opens
    // them, and a scene load in between puts each one back where `BG Control`
    // places it rather than where the last visit left it.
    let arena = scene_of(COOKED.trailing_zeros() as u8);
    let arena_mask = SCENE_GATES.iter().find(|&&(s, _)| s as usize == arena).unwrap().1;
    reset();
    arena_entry(arena, false);
    assert_eq!(unsafe { HK_ARENA_GATES } as u16, PLACEMENT_CLOSED);
    close(arena);
    assert_eq!(unsafe { HK_ARENA_GATES } as u16, PLACEMENT_CLOSED | arena_mask);
    // Left mid-fight and came back: the arena is unwon, so the gates are placed
    // closed again and nothing is lifted.
    arena_entry(arena, false);
    assert_eq!(unsafe { HK_ARENA_GATES } as u16, PLACEMENT_CLOSED);
    close(arena);
    open(arena);
    assert_eq!(unsafe { HK_ARENA_GATES } as u16, PLACEMENT_CLOSED & !arena_mask);
    // Reload with the persisted bit: the same state, with no fight in between.
    arena_entry(arena, true);
    assert_eq!(unsafe { HK_ARENA_GATES } as u16, PLACEMENT_CLOSED & !arena_mask);
    assert_eq!(unsafe { HK_ARENA_GATE_CLOSES }, 2);
    assert_eq!(unsafe { HK_ARENA_GATE_OPENS }, 2);
    reset();
    assert_eq!(unsafe { HK_ARENA_GATES } as u16, PLACEMENT_CLOSED);
    assert_eq!(unsafe { (HK_ARENA_GATE_CLOSES, HK_ARENA_GATE_OPENS) }, (0, 0));

    // A slot with no binding costs nothing and changes nothing, which is the
    // path every scene in the game but five takes on every frame.
    reset();
    arena_entry(arena, true);
    let unbound = (0..4096).find(|slot| !bound_slots().contains(slot)).unwrap();
    assert!(excluded(unbound).is_empty());
    // What each gate shows, which is what the art draws (battle_gate_art.rs).
    // The clip tables are the source's: `BG Opened` and `BG Closed` hold one
    // sprite, `BG Close 1` hands over to `BG Close 2` when it finishes, and the
    // open/close clips end on the pose the gate is left in.
    let shown = |gate: usize| { let (clip, age) = pose(gate); (clip, clip_frame(clip, age)) };
    let last = |clip: u8| *CLIP_FRAMES[clip as usize].last().unwrap() as usize;
    assert_eq!(CLIP_FRAMES[CLIP_OPENED as usize].len(), 1);
    assert_eq!(CLIP_FRAMES[CLIP_CLOSED as usize].len(), 1);
    assert_eq!(last(CLIP_CLOSE_2), last(CLIP_CLOSED), "Close 2 ends on the closed pose");
    reset();
    for gate in 0..GATES {
        let want = if PLACEMENT_CLOSED & (1 << gate) != 0 { CLIP_CLOSED } else { CLIP_OPENED };
        assert_eq!(pose(gate).0, want, "gate {gate} on load");
    }
    let scene = scene_of((!PLACEMENT_CLOSED & every_gate).trailing_zeros() as u8);
    let mask = SCENE_GATES.iter().find(|&&(s, _)| s as usize == scene).unwrap().1;
    arena_entry(scene, false);
    close(scene);
    let open_gate = (0..GATES).find(|&g| mask & !PLACEMENT_CLOSED & (1 << g) != 0).unwrap();
    assert_eq!(shown(open_gate), (CLIP_CLOSE_1, CLIP_FRAMES[CLIP_CLOSE_1 as usize][0] as usize));
    for gate in (0..GATES).filter(|&g| mask & PLACEMENT_CLOSED & (1 << g) != 0) {
        // `Quick Close` has no BG CLOSE transition: a gate placed closed stays put.
        assert_eq!(pose(gate).0, CLIP_CLOSED, "gate {gate} placed closed ignores BG CLOSE");
    }
    // Three frames at 12 fps is 15 ticks, then `Close 2` plays out and holds;
    // its entry shakes the camera once a gate, and starts the slam flash.
    let shakes = camera::SHAKES.load(std::sync::atomic::Ordering::Relaxed);
    for _ in 0..15 { tick(); }
    assert_eq!(pose(open_gate).0, CLIP_CLOSE_2);
    let opened_in_scene = (mask & !PLACEMENT_CLOSED).count_ones();
    assert_eq!(camera::SHAKES.load(std::sync::atomic::Ordering::Relaxed) - shakes, opened_in_scene);
    assert_eq!(sprites(open_gate).1, Some(CLIP_FRAMES[CLIP_EFFECT as usize][0] as usize));
    for _ in 0..15 { tick(); }
    assert_eq!(sprites(open_gate).1, None, "the flash is over after its three frames");
    for _ in 0..30 { tick(); }
    assert_eq!(shown(open_gate).1, last(CLIP_CLOSED));
    open(scene);
    assert_eq!(pose(open_gate).0, CLIP_OPEN);
    for _ in 0..60 { tick(); }
    assert_eq!(shown(open_gate).1, last(CLIP_OPEN), "BG Open holds its last frame");
    // A won arena on entry is `Quick Open`: the held open pose at once.
    arena_entry(scene, true);
    assert!((0..GATES).filter(|&g| mask & (1 << g) != 0).all(|g| pose(g).0 == CLIP_OPENED));
    reset();
    println!("arena gate table, placement state, per-scene broadcast, exclusions and poses passed");
}
