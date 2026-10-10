//! A completed CD transfer is reusable compressed data, never a validated Room.
#[allow(clippy::all, unexpected_cfgs, dead_code)] // game source, linted with the game
#[path = "../../../game/src/room_decode.rs"]
mod room_decode;
#[allow(clippy::all, unexpected_cfgs, dead_code)] // game source, linted with the game
#[path = "../../../game/src/room_residency.rs"]
mod room_residency;
use room_decode::{Decoder, Error};
use room_residency::{Residency, EMPTY};
fn raw_room() -> Vec<u8> {
    let mut b = vec![0u8; 92];
    b[..8].copy_from_slice(b"HKROOM02");
    b[12..16].copy_from_slice(&1u32.to_le_bytes());
    b[32..36].copy_from_slice(&4u32.to_le_bytes());
    b[40..42].copy_from_slice(&u16::MAX.to_le_bytes());
    b[46..48].copy_from_slice(&1u16.to_le_bytes());
    b[48..50].copy_from_slice(&1u16.to_le_bytes());
    b
}
fn stored_room(raw: &[u8]) -> Vec<u8> {
    let mut s = b"HLZC".to_vec();
    s.extend_from_slice(&(raw.len() as u32).to_le_bytes());
    s.extend_from_slice(&[0xf0, raw.len() as u8 - 15]);
    s.extend_from_slice(raw);
    s
}
fn scenario(corruption: u8, budget: usize) -> Result<Vec<u8>, Error> {
    let raw = raw_room();
    let stored = stored_room(&raw);
    let mut r = Residency::new();
    let current = r.reserve(0, true).unwrap();
    r.admit(current, 0, 92);
    r.select(0);
    let slot = r.reserve(1, true).unwrap();
    let mut arena = vec![0u8; 256];
    arena[..stored.len()].copy_from_slice(&stored);
    r.set_wanted(&[2]);
    r.park_completed(slot, 1);
    assert_eq!(r.next_decode(EMPTY), None);
    assert_eq!(r.select(1), None);
    assert!(!r.protect_upload(Some(1)));
    assert!(r.find(1).is_none());
    // A later turn requests the same source; no new reserve/read is possible.
    r.set_wanted(&[1, 2]);
    assert_eq!(r.next_decode(EMPTY), Some(slot));
    assert!(!r.can_reserve(1, true));
    if corruption == 1 {
        arena[12] ^= 1;
    }
    if corruption == 3 {
        arena[10] = 0;
    } // Valid compressed stream, malformed decoded magic.
    let stored_hash = if corruption == 3 {
        psx_pack::fnv1a32(&arena[..stored.len()])
    } else {
        psx_pack::fnv1a32(&stored)
    };
    let raw_hash = if corruption == 2 {
        0
    } else if corruption == 3 {
        let mut bad = raw.clone();
        bad[0] = 0;
        psx_pack::fnv1a32(&bad)
    } else {
        psx_pack::fnv1a32(&raw)
    };
    assert_eq!(r.claim_stored(slot), 1);
    let mut decoder = Decoder::new(stored.len(), stored_hash, raw.len(), raw_hash);
    for _ in 0..10_000 {
        assert!(r.find(1).is_none());
        assert!(r.loading(1));
        match decoder.step(&mut arena, budget) {
            Ok(None) => {}
            Ok(Some(len)) => {
                r.admit(slot, 1, len);
                assert_eq!(r.select(1), Some(true));
                arena.truncate(len);
                return Ok(arena);
            }
            Err(error) => {
                r.release(slot);
                assert!(r.find(1).is_none() && r.stored(1).is_none());
                assert!(r.find(0).is_some());
                return Err(error);
            }
        }
    }
    panic!("stored decoder never completed")
}
#[test]
fn parked_payload_reuses_original_bytes_and_only_publishes_after_all_validation() {
    for budget in [1, 31, 1024] {
        assert_eq!(scenario(0, budget), Ok(raw_room()));
    }
}
#[test]
fn stored_raw_and_format_corruption_cannot_publish_a_parked_payload() {
    for budget in [1, 31, 1024] {
        assert_eq!(scenario(1, budget), Err(Error::Checksum));
        assert_eq!(scenario(2, budget), Err(Error::Checksum));
        assert_eq!(scenario(3, budget), Err(Error::RoomFormat));
    }
}
