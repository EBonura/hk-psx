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
    pub const SCRIPT_EDGE_SLOTS: usize = 12;
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
#[path = "../game/src/battle_gates.rs"]
mod battle_gates;
use battle_gates::*;

/// Every catalogue slot the table binds, and what it excludes right now.
fn excluded(slot: usize) -> Vec<u16> {
    let mut state = world::State::new();
    battle_gates::apply(&mut state, slot);
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

fn main() {
    // The table itself, before anything runs against it.
    assert!(GATES <= 16, "the closed set is one u16");
    assert_eq!(COOKED & !PLACEMENT_CLOSED, 0,
               "the cook bakes a gate's collider only when the source loads it closed, so a gate \
                with terrain that does not start closed means cook.py and this table disagree");
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

    // A fresh session is the cooked world unchanged. This is the assertion that
    // matters most: three of these gates are the only terrain standing between
    // two halves of a room, and a default that lifted one would open a route
    // the source does not have.
    for slot in bound_slots() {
        assert!(excluded(slot).is_empty(), "slot {slot} lifts terrain before any arena is won");
    }

    // One scene at a time, because the source broadcast is per room: winning
    // Crossroads_10 must not open a gate in Crossroads_04.
    for &(scene, mask) in SCENE_GATES {
        reset();
        arena_entry(scene as usize, true);
        assert_eq!(unsafe { HK_ARENA_GATES } as u16, PLACEMENT_CLOSED & !mask);
        for slot in bound_slots() {
            let owned: Vec<u16> = REGIONS.iter()
                .filter(|&&(row, gate, _)| row as usize == slot && mask & (1 << gate) != 0)
                .flat_map(|&(_, _, edges)| edges.iter().copied()).collect();
            assert_eq!(excluded(slot), owned, "slot {slot} after scene {scene} was won");
        }
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
    println!("arena gate table, placement state, per-scene broadcast and exclusions passed");
}
