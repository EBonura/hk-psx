#[path = "../../../game/src/input_queue.rs"]
mod input_queue;
#[path = "../../../game/src/input_sampler.rs"]
mod input_sampler;
use input_sampler::{Fault, Sampler};
use std::cell::Cell;
#[test]
fn boot_excluded_and_many_checkpoints_poll_once_per_actual_vblank() {
    let calls = Cell::new(0);
    let mut s = Sampler::new();
    for tick in 100..200 {
        assert_eq!(
            s.checkpoint(tick, || {
                calls.set(calls.get() + 1);
                (tick, 0)
            }),
            Ok(false)
        );
    }
    assert_eq!(calls.get(), 0);
    s.start(200).unwrap();
    for tick in 200..210 {
        for _ in 0..200 {
            s.checkpoint(tick, || {
                calls.set(calls.get() + 1);
                (tick, tick as u16)
            })
            .unwrap();
        }
    }
    assert_eq!(calls.get(), 9);
    assert_eq!(s.stats().polls, 9);
    assert_eq!(s.stats().max_poll_gap, 1);
    assert_eq!(s.latest_seen(), Some(209));
    assert_eq!(s.pending_count(), 9);
    for tick in 201..210 {
        assert_eq!(s.consume(tick), Ok(tick as u16));
    }
}
#[test]
fn pulses_during_cpu_work_survive_later_consecutive_fixed_updates() {
    let mut s = Sampler::new();
    s.start(80).unwrap();
    for (tick, bits) in [(81, 0), (82, 0x4000), (83, 0), (84, 0x8000), (85, 0)] {
        s.checkpoint(tick, || (tick, bits)).unwrap();
    }
    for (tick, bits) in [(81, 0), (82, 0x4000), (83, 0), (84, 0x8000), (85, 0)] {
        assert_eq!(s.consume(tick), Ok(bits));
    }
    assert_eq!(s.stats().pending_peak, 5);
    assert_eq!(s.stats().missed_vblanks, 0);
}
#[test]
fn missed_historical_ticks_hold_state_without_inventing_hardware_polls() {
    let mut s = Sampler::new();
    s.start(10).unwrap();
    s.checkpoint(11, || (11, 0x4000)).unwrap();
    assert_eq!(s.consume(11), Ok(0x4000));
    s.checkpoint(14, || (14, 0)).unwrap();
    assert_eq!(s.consume(12), Ok(0x4000));
    assert_eq!(s.consume(13), Ok(0x4000));
    assert_eq!(s.consume(14), Ok(0));
    assert_eq!(s.stats().polls, 2);
    assert_eq!(s.stats().max_poll_gap, 3);
    assert_eq!(s.stats().missed_vblanks, 2);
}
#[test]
fn transfer_crossing_vblank_uses_completion_timestamp_without_duplicate_poll() {
    let mut s = Sampler::new();
    s.start(100).unwrap();
    s.checkpoint(101, || (102, 7)).unwrap();
    assert_eq!(s.latest_seen(), Some(102));
    assert_eq!(
        s.checkpoint(102, || panic!("duplicate transaction")),
        Ok(false)
    );
    assert_eq!(s.consume(101), Ok(0));
    assert_eq!(s.consume(102), Ok(7));
    assert_eq!(s.stats().polls, 1);
    assert_eq!(s.stats().max_poll_gap, 0);
    assert_eq!(s.stats().missed_vblanks, 1);
}
#[test]
fn ring_and_vblank_wrap_preserve_exact_order() {
    let mut s = Sampler::new();
    let mut tick = u32::MAX - 5;
    s.start(tick).unwrap();
    for _ in 0..8 {
        for _ in 0..8 {
            tick = tick.wrapping_add(1);
            s.checkpoint(tick, || (tick, tick as u16)).unwrap();
        }
        for i in (0..8).rev() {
            let consumed = tick.wrapping_sub(i);
            assert_eq!(s.consume(consumed), Ok(consumed as u16));
        }
    }
    assert_eq!(s.pending_count(), 0);
    assert_eq!(s.stats().polls, 64);
    assert_eq!(s.stats().max_poll_gap, 1);
}
#[test]
fn full_queue_drops_its_oldest_sample_and_keeps_its_presses() {
    let mut s = Sampler::new();
    s.start(0).unwrap();
    // Tick 1 presses 0x40 for that one poll only, then nothing is held.
    for tick in 1..=16 {
        s.checkpoint(tick, || (tick, if tick == 1 { 0x40 } else { 0 }))
            .unwrap();
    }
    assert_eq!(s.checkpoint(17, || (17, 0x10)), Ok(true));
    assert_eq!(s.fault(), None);
    assert_eq!(s.stats().polls, 17);
    assert_eq!(s.stats().dropped_samples, 1);
    assert_eq!(s.pending_count(), 16);
    assert_eq!(s.latest_seen(), Some(17));
    // Tick 1's sample is gone, its press is not: it arrives on tick 1 as a
    // one-tick press, and tick 2 reads its own sample.
    assert_eq!(s.consume(1), Ok(0x40));
    assert_eq!(s.consume(2), Ok(0));
    for tick in 3..=16 {
        assert_eq!(s.consume(tick), Ok(0));
    }
    assert_eq!(s.consume(17), Ok(0x10));
    assert_eq!(s.fault(), None);
}
#[test]
fn a_held_button_dropped_from_a_full_queue_is_not_reported_as_a_new_press() {
    let mut s = Sampler::new();
    s.start(0).unwrap();
    s.checkpoint(1, || (1, 0x40)).unwrap();
    assert_eq!(s.consume(1), Ok(0x40));
    for tick in 2..=18 {
        s.checkpoint(tick, || (tick, 0x40)).unwrap();
    }
    assert_eq!(s.stats().dropped_samples, 1);
    assert_eq!(s.consume(2), Ok(0x40));
    assert_eq!(s.consume(3), Ok(0x40));
}
#[test]
fn bound_lag_skips_ticks_keeps_presses_and_the_hold() {
    let mut s = Sampler::new();
    s.start(0).unwrap();
    // Press 0x40 on tick 2 only, and start holding 0x10 on tick 5.
    for tick in 1..=12 {
        let buttons = if tick == 2 {
            0x40
        } else if tick >= 5 {
            0x10
        } else {
            0
        };
        s.checkpoint(tick, || (tick, buttons)).unwrap();
    }
    // Within the limit: nothing happens.
    assert_eq!(s.bound_lag(12, 12), Ok(None));
    // Twelve waiting against a limit of eight: skip to tick 4.
    assert_eq!(s.bound_lag(12, 8), Ok(Some(4)));
    assert_eq!(s.stats().skipped_ticks, 4);
    assert_eq!(s.pending_count(), 8);
    // Tick 5 carries the skipped press beside its own hold; then the hold.
    assert_eq!(s.consume(5), Ok(0x50));
    assert_eq!(s.consume(6), Ok(0x10));
    // Never past the last observation: a later clock read skips only to
    // what was polled.
    assert_eq!(s.bound_lag(40, 2), Ok(Some(10)));
    assert_eq!(s.consume(11), Ok(0x10));
    assert_eq!(s.fault(), None);
}
#[test]
fn bound_lag_refuses_a_scene_load_and_an_unstarted_sampler() {
    let mut s = Sampler::new();
    assert_eq!(s.bound_lag(10, 8), Err(Fault::NotStarted));
    let mut s = Sampler::new();
    s.start(0).unwrap();
    s.begin_scene_load(0).unwrap();
    assert_eq!(s.bound_lag(10, 8), Err(Fault::SceneLoadActive));
}
#[test]
fn bad_clock_and_skipped_update_are_explicit_faults() {
    for (now, expected) in [
        (99, Fault::ClockOrder),
        (100u32.wrapping_add(1 << 31), Fault::AmbiguousClock),
    ] {
        let mut s = Sampler::new();
        s.start(100).unwrap();
        assert_eq!(
            s.checkpoint(now, || panic!("bad clock cannot poll")),
            Err(expected)
        );
    }
    let mut s = Sampler::new();
    s.start(100).unwrap();
    assert_eq!(s.checkpoint(101, || (100, 0)), Err(Fault::ClockOrder));
    let mut s = Sampler::new();
    s.start(100).unwrap();
    assert_eq!(s.consume(102), Err(Fault::NonConsecutiveUpdate));
    let mut s = Sampler::new();
    s.start(100).unwrap();
    assert_eq!(s.consume(101), Err(Fault::FutureUpdate));
    let mut s = Sampler::new();
    s.start(100).unwrap();
    assert_eq!(s.start(101), Err(Fault::AlreadyStarted));
}
#[test]
fn storage_is_bounded_and_has_no_heap_owner() {
    assert!(std::mem::size_of::<Sampler>() <= 256);
    assert!(!std::mem::needs_drop::<Sampler>());
    assert_ne!(Fault::QueueFull.code(), 0);
}

#[test]
fn fast_guard_preserves_backwards_clock_and_latched_fault_validation() {
    let mut s = Sampler::new();
    assert!(!s.poll_due(10));
    s.start(10).unwrap();
    assert!(!s.poll_due(10));
    assert!(s.poll_due(11));
    s.checkpoint(11, || (11, 0)).unwrap();
    assert!(!s.poll_due(11));
    assert!(s.poll_due(10));
    assert_eq!(
        s.checkpoint(10, || panic!("backwards clock must not poll")),
        Err(Fault::ClockOrder)
    );
    assert!(s.poll_due(11));
    assert_eq!(
        s.checkpoint(11, || panic!("latched fault must not poll")),
        Err(Fault::ClockOrder)
    );
}

#[test]
fn explicit_scene_load_drains_bounded_queue_preserves_history_and_resumes_edges() {
    let mut s = Sampler::new();
    s.start(100).unwrap();
    s.checkpoint(101, || (101, 0)).unwrap();
    s.consume(101).unwrap();
    // Some real observations may already be waiting when this simulation tick
    // enters a scene gate. The gate acknowledges them as loading-time input.
    for tick in 102..=117 {
        s.checkpoint(tick, || (tick, 0x4000)).unwrap();
    }
    s.begin_scene_load(101).unwrap();
    assert_eq!(s.pending_count(), 0);
    for tick in 118..=717 {
        assert!(s
            .checkpoint(tick, || (tick, if tick % 2 == 0 { 0x4000 } else { 0 }))
            .unwrap());
        assert_eq!(s.pending_count(), 0);
        assert!(!s
            .checkpoint(tick, || panic!("same VBlank must not poll"))
            .unwrap());
    }
    assert_eq!(s.end_scene_load(), Ok(717));
    assert_eq!(s.held_buttons(), 0);
    assert_eq!(s.stats().polls, 617);
    assert_eq!(s.stats().loading_samples, 616);
    assert_eq!(s.stats().loading_ticks, 616);
    assert_eq!(s.stats().pending_peak, 16);
    assert_eq!(s.stats().missed_vblanks, 0);
    // A press in the first resumed frame is preserved, never acknowledged by
    // the previous load merely because it follows end_scene_load immediately.
    s.checkpoint(718, || (718, 0x4000)).unwrap();
    assert_eq!(s.consume(718), Ok(0x4000));
    s.checkpoint(719, || (719, 0)).unwrap();
    assert_eq!(s.consume(719), Ok(0));
    assert_eq!(s.stats().loading_samples, 616);
    assert_eq!(s.stats().polls, 619);
}

#[test]
fn scene_load_retains_missed_polls_wrap_and_transfer_completion_boundary() {
    let mut s = Sampler::new();
    s.start(u32::MAX - 2).unwrap();
    s.checkpoint(u32::MAX - 1, || (u32::MAX - 1, 1)).unwrap();
    s.consume(u32::MAX - 1).unwrap();
    s.begin_scene_load(u32::MAX - 1).unwrap();
    s.checkpoint(u32::MAX, || (0, 2)).unwrap();
    assert_eq!(s.stats().missed_vblanks, 1);
    s.checkpoint(100, || (100, 4)).unwrap();
    assert_eq!(s.stats().missed_vblanks, 100);
    assert_eq!(s.stats().max_poll_gap, 100);
    assert_eq!(s.stats().loading_ticks, 102);
    assert_eq!(s.stats().loading_samples, 2);
    assert_eq!(s.end_scene_load(), Ok(100));
    assert_eq!(s.held_buttons(), 4);
    s.checkpoint(101, || (101, 8)).unwrap();
    assert_eq!(s.consume(101), Ok(8));
    s.begin_scene_load(101).unwrap();
    s.checkpoint(102, || (102, 8)).unwrap();
    assert_eq!(s.end_scene_load(), Ok(102));
    assert_eq!(s.stats().missed_vblanks, 100);
    assert_eq!(s.stats().loading_ticks, 103);
    // A blocking card transfer inside a load: its gaps are blocked, not missed,
    // and the flag does not outlive the load.
    assert_eq!(s.begin_blocking_transfer(), Err(Fault::SceneLoadInactive));
    let mut s = Sampler::new();
    s.start(0).unwrap();
    s.checkpoint(1, || (1, 0)).unwrap();
    s.consume(1).unwrap();
    s.begin_scene_load(1).unwrap();
    s.begin_blocking_transfer().unwrap();
    s.checkpoint(4, || (4, 0)).unwrap();
    assert_eq!(
        (
            s.stats().missed_vblanks,
            s.stats().blocked_vblanks,
            s.stats().max_poll_gap
        ),
        (0, 2, 0)
    );
    assert_eq!(s.end_scene_load(), Ok(4));
    s.checkpoint(7, || (7, 0)).unwrap();
    assert_eq!(
        (s.stats().missed_vblanks, s.stats().blocked_vblanks),
        (2, 2)
    );
}

#[test]
fn scene_load_ownership_never_masks_faults_or_allows_simulation_during_pause() {
    let mut s = Sampler::new();
    assert_eq!(s.begin_scene_load(0), Err(Fault::NotStarted));
    let mut s = Sampler::new();
    s.start(10).unwrap();
    assert_eq!(s.end_scene_load(), Err(Fault::SceneLoadInactive));
    let mut s = Sampler::new();
    s.start(10).unwrap();
    assert_eq!(s.begin_scene_load(11), Err(Fault::NonConsecutiveUpdate));
    let mut s = Sampler::new();
    s.start(10).unwrap();
    s.begin_scene_load(10).unwrap();
    assert_eq!(s.begin_scene_load(10), Err(Fault::SceneLoadActive));
    assert_eq!(s.end_scene_load(), Err(Fault::SceneLoadActive));
    let mut s = Sampler::new();
    s.start(10).unwrap();
    s.begin_scene_load(10).unwrap();
    s.checkpoint(11, || (11, 0)).unwrap();
    assert_eq!(s.consume(11), Err(Fault::SceneLoadActive));
    assert_eq!(
        s.checkpoint(12, || panic!("fault must remain latched")),
        Err(Fault::SceneLoadActive)
    );
}
