//! The compact actor tier on its first family, the Plant Trap (Fool Eater): the tree's clip sequence and its
//! timing, the box each frame puts on the object, and the death.
use hk_sim::fsm_actor::{self as fa, FsmActor};

const SNAP_READY: u8 = 1;
const SNAP: u8 = 2;
const RETRACT: u8 = 0;

/// Plays (tick, clip) over `ticks` ticks of a hero that is inside the trigger from tick 10 to tick 12.
fn timeline(ticks: u32, inside: impl Fn(u32) -> bool) -> Vec<(u32, u8, u32)> {
    let (mut trap, first) = FsmActor::new(0, 1);
    assert_eq!(first, None, "the tree plays nothing on entry: the object rests on its last Retract frame");
    let mut out = Vec::new();
    for t in 0..ticks {
        if let Some((clip, from)) = trap.tick(inside(t)) {
            out.push((t, clip, from));
        }
    }
    out
}

#[test]
fn rests_on_the_last_retract_frame() {
    let (clip, tick) = fa::rest(0);
    assert_eq!(clip, RETRACT);
    assert_eq!(fa::frame_at(0, clip, tick), 6);
    assert_eq!(fa::hurt_box(0, clip, tick), None, "a resting trap has no collider");
}

#[test]
fn a_hero_that_never_enters_leaves_it_asleep() {
    assert_eq!(timeline(600, |_| false), vec![]);
}

#[test]
fn snaps_after_the_ready_wait_then_retracts_and_cools_down() {
    let line = timeline(600, |t| (10..12).contains(&t));
    // The hero entering the detector sends DETECT the same tick (Detect Hero's Trigger2dEvent, then Hit).
    let clips: Vec<u8> = line.iter().map(|p| p.1).collect();
    assert_eq!(clips, vec![SNAP_READY, SNAP, RETRACT], "{line:?}");
    let ticks: Vec<u32> = line.iter().map(|p| p.0).collect();
    println!("plays at ticks {ticks:?}");
    // Wait 0.75 s then Wait 1.0 s: 45 and 60 ticks.
    assert_eq!(ticks[1] - ticks[0], 45);
    assert_eq!(ticks[2] - ticks[1], 60);
}

#[test]
fn only_the_snap_and_early_retract_frames_carry_a_box() {
    assert!(fa::hurt_box(0, SNAP_READY, 0).is_none());
    assert!(fa::hurt_box(0, SNAP, 0).is_some());
    assert!(fa::hurt_box(0, SNAP, 14).is_some(), "the last Snap frame holds for the rest of the Wait");
    assert!(fa::hurt_box(0, RETRACT, 0).is_some());
    assert!(fa::hurt_box(0, RETRACT, 15).is_none(), "frame 3 of Retract has no collider");
}

#[test]
fn a_second_visit_snaps_again_after_the_cooldown() {
    let line = timeline(900, |t| (10..12).contains(&t) || (400..402).contains(&t));
    let clips: Vec<u8> = line.iter().map(|p| p.1).collect();
    assert_eq!(clips, vec![SNAP_READY, SNAP, RETRACT, SNAP_READY, SNAP, RETRACT], "{line:?}");
}

#[test]
fn a_hero_standing_in_the_trigger_does_not_restart_a_snap_in_progress() {
    let line = timeline(600, |t| t >= 10);
    let clips: Vec<u8> = line.iter().map(|p| p.1).collect();
    assert_eq!(&clips[..3], &[SNAP_READY, SNAP, RETRACT]);
}

#[test]
fn dying_stops_the_tree() {
    let (mut trap, _) = FsmActor::new(0, 1);
    trap.die();
    for t in 0..300 {
        assert_eq!(trap.tick(t > 5), None);
    }
    assert_eq!(fa::death(0), Some(3));
}
