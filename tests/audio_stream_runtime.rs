//! The stream state machine against the cooked ambience bank, so the boundary
//! window follows whatever rate the cave bed is cooked at rather than a
//! literal. The include stands in for game/src/ambience.rs, which supplies
//! these to the stream module in the guest.
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/ambience.rs"));
pub const STREAM_INDEX: usize = 3;
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

fn source(bytes: usize) -> Vec<u8> {
    let mut out = vec![0; bytes];
    let mut seed = 71u32;
    for block in 0..bytes / 16 {
        out[block * 16] = if block == 0 {
            0
        } else {
            (block % 5 * 16 + block % 13) as u8
        };
        for i in 2..16 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            out[block * 16 + i] = (seed >> 24) as u8;
        }
    }
    out[1] = 4;
    out[bytes - 15] |= 3;
    out
}

fn decode(block: &[u8], history: &mut [i32; 2]) -> [i16; 28] {
    let filter = (block[0] >> 4) as usize;
    let shift = block[0] & 15;
    let pos = [0, 60, 115, 98, 122][filter];
    let neg = [0, 0, -52, -55, -60][filter];
    let mut output = [0i16; 28];
    for sample in 0..28 {
        let nibble = (block[2 + sample / 2] >> ((sample & 1) * 4)) & 15;
        let signed = ((nibble as i16) << 12) >> shift;
        let value =
            (signed as i32 + (history[0] * pos + history[1] * neg + 32) / 64).clamp(-32768, 32767);
        history[1] = history[0];
        history[0] = value;
        output[sample] = value as i16;
    }
    output
}

#[test]
fn ring_matches_full_source_pcm_across_non_aligned_loop_boundaries() {
    for length in [16, 48, HALF_BYTES - 16, HALF_BYTES + 48, SOURCE_BYTES] {
        let source = source(length);
        let mut ring = [[0u8; HALF_BYTES]; 2];
        let mut cursor = 0;
        let mut wraps = 0;
        for half in 0..2 {
            wraps += fill_half(&source, &mut cursor, half, &mut ring[half]).unwrap();
        }
        let mut reference = [0; 2];
        let mut actual = [0; 2];
        let count = (length / HALF_BYTES * 4 + 12).max(12);
        for played_half in 0..count {
            let half = played_half & 1;
            for block in 0..HALF_BYTES / 16 {
                let index = (played_half * HALF_BYTES + block * 16) % length;
                let src = &source[index..index + 16];
                let dst = &ring[half][block * 16..block * 16 + 16];
                assert_eq!(src[0], dst[0]);
                assert_eq!(&src[2..], &dst[2..]);
                let expected_flag = if half == 0 && block == 0 {
                    4
                } else if half == 1 && block == HALF_BYTES / 16 - 1 {
                    3
                } else {
                    0
                };
                assert_eq!(dst[1], expected_flag);
                assert_eq!(decode(src, &mut reference), decode(dst, &mut actual));
            }
            wraps += fill_half(&source, &mut cursor, half, &mut ring[half]).unwrap();
        }
        assert_eq!(wraps as usize, (count + 2) * HALF_BYTES / length);
        assert_eq!(cursor, (count + 2) * HALF_BYTES % length);
    }
}

#[test]
fn invalid_copy_inputs_leave_cursor_and_destination_untouched() {
    let mut out = [0xabu8; HALF_BYTES];
    for (data, mut cursor, half) in [
        (vec![], 0, 0),
        (vec![0; 15], 0, 0),
        (vec![0; 16], 16, 0),
        (vec![0; 32], 1, 0),
        (vec![0; 16], 0, 2),
    ] {
        let original = cursor;
        assert_eq!(fill_half(&data, &mut cursor, half, &mut out), None);
        assert_eq!(cursor, original);
        assert!(out.iter().all(|&v| v == 0xab));
    }
}
