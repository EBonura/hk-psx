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
fn full_queue_faults_before_poll_and_never_silently_discards_samples() {
    let mut s = Sampler::new();
    s.start(0).unwrap();
    for tick in 1..=16 {
        s.checkpoint(tick, || (tick, tick as u16)).unwrap();
    }
    assert_eq!(
        s.checkpoint(17, || panic!("must not query hardware without capacity")),
        Err(Fault::QueueFull)
    );
    assert_eq!(s.stats().polls, 16);
    assert_eq!(s.pending_count(), 16);
    assert_eq!(s.latest_seen(), Some(16));
    assert_eq!(
        s.checkpoint(18, || panic!("fault must remain latched")),
        Err(Fault::QueueFull)
    );
    assert_eq!(s.fault(), Some(Fault::QueueFull));
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
    assert_eq!(s.checkpoint(10, || panic!("backwards clock must not poll")),Err(Fault::ClockOrder));
    assert!(s.poll_due(11));
    assert_eq!(s.checkpoint(11, || panic!("latched fault must not poll")),Err(Fault::ClockOrder));
}
