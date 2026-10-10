#![allow(dead_code)] // includes game modules by path and exercises part of each
//! Trigger overlap for the script runtime.
//!
//! The phases are edges, and an edge kept in the wrong place is the failure
//! this whole seam exists to avoid: Enter repeating on every state re-entry, or
//! never firing at all because the state was entered with the Knight already
//! inside. These pin the sequencing that decides it.
use hk_sim::script::{TRIGGER_ENTER, TRIGGER_EXIT, TRIGGER_STAY};
#[allow(clippy::all, unexpected_cfgs)] // game source, linted with the game
#[path = "../../../game/src/script_state.rs"]
mod script_state;
use script_state::{overlaps, Overlaps};

const ONE: i32 = 65536;
/// One volume, from (10, 0) to (12, 4) in world units.
const VOLUME: [i32; 4] = [10 * ONE, 0, 12 * ONE, 4 * ONE];
fn body(x: i32) -> [i32; 4] {
    [x * ONE - ONE / 2, 0, x * ONE + ONE / 2, 2 * ONE]
}

fn walk(path: &[i32]) -> (Vec<bool>, Vec<bool>, Vec<bool>) {
    let mut inside = Overlaps::<1>::new();
    let (mut enters, mut stays, mut exits) = (vec![], vec![], vec![]);
    for &x in path {
        inside.refresh(|_| overlaps(VOLUME, body(x)));
        enters.push(inside.holds(0, TRIGGER_ENTER));
        stays.push(inside.holds(0, TRIGGER_STAY));
        exits.push(inside.holds(0, TRIGGER_EXIT));
    }
    (enters, stays, exits)
}

#[test]
fn enter_fires_once_at_the_edge_and_exit_once_on_the_way_out() {
    // Walk in from the left, stand still for two ticks, walk out to the right.
    let (enters, stays, exits) = walk(&[8, 9, 11, 11, 11, 14, 15]);
    assert_eq!(
        enters,
        [false, false, true, false, false, false, false],
        "Enter is the tick the overlap begins, not every tick inside it"
    );
    assert_eq!(stays, [false, false, true, true, true, false, false]);
    assert_eq!(
        exits,
        [false, false, false, false, false, true, false],
        "Exit is the tick it ends, and does not repeat afterwards"
    );
}

#[test]
fn a_scene_load_with_the_knight_already_inside_reads_as_an_entry() {
    // Unity creates the collider around wherever the Knight arrived, so the
    // first overlap it sees is an entry. `clear` is what a scene load calls.
    let mut inside = Overlaps::<1>::new();
    inside.refresh(|_| true);
    inside.refresh(|_| true);
    assert!(
        !inside.holds(0, TRIGGER_ENTER),
        "a continuing overlap is not an entry"
    );
    inside.clear();
    inside.refresh(|_| true);
    assert!(inside.holds(0, TRIGGER_ENTER));
}

#[test]
fn a_volume_the_bank_does_not_have_is_never_in_any_phase() {
    let mut inside = Overlaps::<1>::new();
    inside.refresh(|_| true);
    for phase in [TRIGGER_ENTER, TRIGGER_STAY, TRIGGER_EXIT, 9] {
        assert!(
            !inside.holds(1, phase),
            "phase {phase} of an index off the end"
        );
    }
    assert!(!inside.holds(0, 9), "a phase this runtime does not know");
}

#[test]
fn a_touching_edge_counts_as_inside() {
    // The bench and the NPC talk range use the same inclusive test, so a
    // trigger whose edge the hero body just reaches has to agree with them.
    assert!(overlaps(VOLUME, [12 * ONE, 0, 13 * ONE, ONE]));
    assert!(!overlaps(VOLUME, [12 * ONE + 1, 0, 13 * ONE, ONE]));
    assert!(
        !overlaps(VOLUME, [10 * ONE, 5 * ONE, 12 * ONE, 6 * ONE]),
        "above the volume is outside it"
    );
}
