//! The music ring's state machine and FIFO bookkeeping against the cooked
//! pitch and voice, so the boundary window follows whatever rate area music is
//! cooked at rather than a literal. The includes stand in for game/src/music.rs,
//! which supplies these to the stream module in the guest.
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/ambience.rs"));
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/area_music.rs"));
#[path = "../game/src/audio_stream.rs"]
mod stream;
use stream::*;

/// The NTSC boundary period and its PAL counterpart at the same pitch.
const NTSC: u32 = BOUNDARY_TICKS;
const PAL: u32 = BOUNDARY_TICKS * 5 / 6;

#[test]
fn pal_ntsc_boundaries_refill_only_the_previous_half() {
    for period in [PAL, PAL + 1, NTSC, NTSC + 1] {
        for lag in 0..7 {
            let mut state = State::new();
            state.start(0);
            for now in 1..period * 200 + 1 {
                let boundary = now > lag && (now - lag) % period == 0;
                match state.poll(now, boundary) {
                    Poll::Refill(half) => {
                        assert!(boundary);
                        assert_eq!(half, (((now - lag) / period - 1) & 1) as usize);
                        assert!(state.complete(now));
                    }
                    Poll::Idle => assert!(!boundary),
                    Poll::Fault => panic!("unexpected fault at {now}"),
                }
            }
        }
    }
}

#[test]
fn stale_lost_and_early_events_stop_before_refill() {
    let mut state = State::new();
    state.start(0);
    assert_eq!(state.poll(MAX_POLL_GAP, false), Poll::Fault);
    assert_eq!(state.poll(100, true), Poll::Idle);
    state.start(0);
    assert_eq!(state.poll(1, true), Poll::Fault);
    state.start(0);
    for now in 1..=MAX_BOUNDARY_TICKS {
        assert_eq!(state.poll(now, false), Poll::Idle);
    }
    assert_eq!(state.poll(MAX_BOUNDARY_TICKS + 1, false), Poll::Fault);
    state.start(0);
    for now in 1..NTSC {
        assert_eq!(state.poll(now, false), Poll::Idle);
    }
    assert_eq!(state.poll(NTSC, true), Poll::Refill(0));
    assert!(!state.complete(NTSC + MAX_POLL_GAP));
    assert_eq!(state.poll(NTSC + MAX_POLL_GAP + 1, true), Poll::Idle);
}

#[test]
fn incomplete_dma_and_duplicate_event_never_grant_second_refill() {
    let mut state = State::new();
    state.start(0);
    for now in 1..NTSC {
        state.poll(now, false);
    }
    assert_eq!(state.poll(NTSC, true), Poll::Refill(0));
    assert_eq!(state.poll(NTSC + 1, true), Poll::Fault);
    state.start(0);
    for now in 1..NTSC {
        state.poll(now, false);
    }
    assert_eq!(state.poll(NTSC, true), Poll::Refill(0));
    assert!(state.complete(NTSC));
    assert_eq!(state.poll(NTSC + 1, true), Poll::Fault);
}

#[test]
fn timestamp_wrap_and_restart_reestablish_a() {
    let mut state = State::new();
    let start = u32::MAX - 40;
    state.start(start);
    for delta in 1..NTSC {
        assert_eq!(state.poll(start.wrapping_add(delta), false), Poll::Idle);
    }
    assert_eq!(state.poll(start.wrapping_add(NTSC), true), Poll::Refill(0));
    assert!(state.complete(start.wrapping_add(NTSC)));
    state.stop();
    state.start(500);
    for now in 501..500 + NTSC {
        state.poll(now, false);
    }
    assert_eq!(state.poll(500 + NTSC, true), Poll::Refill(0));
}

#[test]
fn fifo_hands_out_whole_halves_and_never_reads_past_the_wrap_or_the_loop() {
    let mut fifo = Fifo::new();
    let mut cursor = Cursor::new(37);
    let mut taken = 0;
    let mut loops = 0;
    for _ in 0..1000 {
        let n = cursor.span(&fifo, CHUNK_SECTORS);
        if n > 0 {
            assert!(fifo.write + n <= FIFO_SECTORS, "a read crossed the FIFO's wrap");
            assert!(cursor.next as usize + n <= 37, "a read crossed the loop");
            assert!(fifo.commit(n));
            if cursor.advance(n) {
                loops += 1;
            }
        }
        assert!(fifo.filled <= FIFO_SECTORS);
        if let Some(at) = fifo.take_half() {
            assert_eq!(at % HALF_SECTORS, 0);
            assert!(at + HALF_SECTORS <= FIFO_SECTORS, "a half straddled the wrap");
            taken += 1;
        }
    }
    assert_eq!(taken, 1000);
    let read = taken * HALF_SECTORS + fifo.filled;
    assert_eq!(loops * 37 + cursor.next as usize, read);
    // Nothing buffered, nothing handed out; an overlong commit is refused.
    let mut empty = Fifo::new();
    assert_eq!(empty.take_half(), None);
    assert!(!empty.commit(FIFO_SECTORS + 1));
    assert!(!empty.commit(0));
}

#[test]
fn a_half_gets_exactly_the_ring_flags_and_keeps_every_nibble() {
    let mut seed = 71u32;
    let mut bytes = vec![0u8; HALF_BYTES];
    for b in bytes.iter_mut() {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        *b = (seed >> 24) as u8;
    }
    for half in 0..2 {
        let mut out = bytes.clone();
        assert!(flag_half(&mut out, half));
        for block in (0..HALF_BYTES).step_by(16) {
            assert_eq!(out[block], bytes[block]);
            assert_eq!(&out[block + 2..block + 16], &bytes[block + 2..block + 16]);
            let expected = if half == 0 && block == 0 { 4 } else if half == 1 && block == HALF_BYTES - 16 { 3 } else { 0 };
            assert_eq!(out[block + 1], expected);
        }
    }
    assert!(!flag_half(&mut bytes.clone(), 2));
    assert!(!flag_half(&mut bytes[..16].to_vec(), 0));
}
