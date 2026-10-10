#![allow(dead_code)] // includes game modules by path and exercises part of each
#[allow(clippy::all, unexpected_cfgs)] // game source, linted with the game
#[path = "../../../game/src/input_queue.rs"]
mod input_queue;
use input_queue::{Error, Queue, Sample, CAPACITY};

#[test]
fn short_pulses_and_multiple_queued_samples_keep_exact_order_and_timestamps() {
    let mut q = Queue::new();
    for (tick, buttons) in [(1, 0), (2, 0x4000), (3, 0), (5, 0x0040), (6, 0)] {
        q.push(tick, buttons).unwrap();
    }
    assert_eq!(q.latest_seen(), Some(6));
    for (tick, buttons) in [(1, 0), (2, 0x4000), (3, 0), (5, 0x0040), (6, 0)] {
        assert_eq!(q.take(6), Ok(Some(Sample { tick, buttons })));
        assert_eq!(q.last_consumed_hold(), buttons);
    }
    assert_eq!(q.take(6), Ok(None));
    assert_eq!(q.pending_count(), 0);
}

#[test]
fn gaps_and_future_samples_do_not_fabricate_polls_or_change_consumed_hold() {
    let mut q = Queue::new();
    q.push(2, 0x55AA).unwrap();
    assert_eq!(q.take(1), Ok(None));
    assert_eq!(q.last_consumed_hold(), 0);
    assert_eq!(q.take(2).unwrap().unwrap().buttons, 0x55AA);
    q.push(8, 0).unwrap();
    for tick in 3..8 {
        assert_eq!(q.take(tick), Ok(None));
        assert_eq!(q.last_consumed_hold(), 0x55AA);
        assert_eq!(q.pending_count(), 1);
    }
    assert_eq!(
        q.take(8),
        Ok(Some(Sample {
            tick: 8,
            buttons: 0
        }))
    );
}

#[test]
fn capacity_overflow_is_explicit_transactional_and_retry_preserves_ring_order() {
    let mut q = Queue::new();
    for tick in 1..=CAPACITY as u32 {
        q.push(tick, tick as u16).unwrap();
    }
    let full = q.clone();
    assert_eq!(q.push(17, 17), Err(Error::Full));
    assert_eq!(q, full);
    for tick in 1..=8 {
        assert_eq!(q.take(16).unwrap().unwrap().tick, tick);
    }
    for tick in 17..=24 {
        q.push(tick, tick as u16).unwrap();
    }
    for tick in 9..=24 {
        assert_eq!(
            q.take(24),
            Ok(Some(Sample {
                tick,
                buttons: tick as u16
            }))
        );
    }
    assert_eq!(q.take(24), Ok(None));
}

#[test]
fn vblank_u32_wrap_is_ordered_without_losing_zero_timestamp() {
    let mut q = Queue::new();
    q.reset(u32::MAX - 2);
    for tick in [u32::MAX - 1, u32::MAX, 0, 1] {
        q.push(tick, tick as u16).unwrap();
    }
    for tick in [u32::MAX - 1, u32::MAX, 0, 1] {
        assert_eq!(q.take(1).unwrap().unwrap().tick, tick);
    }
    assert_eq!(q.latest_seen(), Some(1));
    assert_eq!(q.take(1), Ok(None));
}

#[test]
fn invalid_order_and_ambiguous_half_wrap_never_mutate_queue() {
    let mut q = Queue::new();
    q.reset(100);
    q.push(101, 7).unwrap();
    let before = q.clone();
    for (tick, error) in [
        (101, Error::DuplicateTick),
        (100, Error::OutOfOrder),
        (101u32.wrapping_add(1 << 31), Error::AmbiguousWrap),
    ] {
        assert_eq!(q.push(tick, 8), Err(error));
        assert_eq!(q, before);
    }
    assert_eq!(q.take(99), Err(Error::OutOfOrder));
    assert_eq!(q, before);
    assert_eq!(
        q.take(100u32.wrapping_add(1 << 31)),
        Err(Error::AmbiguousWrap)
    );
    assert_eq!(q, before);
    q.take(101).unwrap();
    let consumed = q.clone();
    assert_eq!(q.take(100), Err(Error::OutOfOrder));
    assert_eq!(q, consumed);
}

#[test]
fn reset_discards_history_and_does_not_claim_a_hardware_poll() {
    let mut q = Queue::new();
    q.push(1, 123).unwrap();
    q.take(1).unwrap();
    q.push(2, 456).unwrap();
    q.reset(900);
    assert_eq!(q.pending_count(), 0);
    assert_eq!(q.latest_seen(), None);
    assert_eq!(q.last_consumed_hold(), 0);
    assert_eq!(q.take(900), Ok(None));
    assert_eq!(q.push(900, 9), Err(Error::DuplicateTick));
    q.push(901, 9).unwrap();
    assert_eq!(q.take(901).unwrap().unwrap().buttons, 9);
}
