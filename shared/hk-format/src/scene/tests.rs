//! The pre-cursor parser is retained only as an independent error-order oracle.
extern crate std;
use super::*;
use std::{vec, vec::Vec};
impl<'a> Scene<'a> {
    fn parse_reference(bytes: &'a [u8]) -> Result<Self, Error> {
        let scene = Self::header(bytes)?;
        let first = scene.room(0).ok_or(Error::Header)?;
        for i in 0..scene.counts[1] {
            if first.texture(i).palette as usize >= scene.counts[6] {
                return Err(Error::Reference);
            }
            first.validate_record(0, i)?;
        }
        let mut next = scene.offsets[9];
        for room_index in 0..scene.counts[0] {
            let p = scene.offsets[8] + room_index * DESCRIPTOR;
            let chunk = u32_at(bytes, p);
            if chunk == 0 || u32_at(bytes, p + 36) != 0 {
                return Err(Error::Header);
            }
            for prior in 0..room_index {
                if u32_at(bytes, scene.offsets[8] + prior * DESCRIPTOR) == chunk {
                    return Err(Error::Reference);
                }
            }
            let room = scene.room(room_index).ok_or(Error::Reference)?;
            for section in 0..4 {
                let count = room.counts[section + 2];
                if count > [1024, 2048, 128, 1024][section] {
                    return Err(Error::Limit);
                }
                let start = u32_at(bytes, p + 20 + section * 4) as usize;
                let aligned = next.checked_add(3).ok_or(Error::Truncated)? & !3;
                if start != aligned
                    || start > bytes.len()
                    || bytes[next..start].iter().any(|&b| b != 0)
                {
                    return Err(Error::Header);
                }
                next = start
                    .checked_add(count.checked_mul(2).ok_or(Error::Truncated)?)
                    .ok_or(Error::Truncated)?;
                if next > bytes.len() {
                    return Err(Error::Truncated);
                }
                for i in 0..count {
                    if u16_at(bytes, start + i * 2) as usize >= scene.counts[section + 2] {
                        return Err(Error::Reference);
                    }
                    room.validate_record(section + 1, i)?;
                }
            }
        }
        if (next.checked_add(3).ok_or(Error::Truncated)? & !3) != bytes.len()
            || bytes[next..].iter().any(|&b| b != 0)
        {
            return Err(Error::Truncated);
        }
        Ok(scene)
    }
}
fn put(b: &mut [u8], at: usize, n: u32) {
    b[at..at + 4].copy_from_slice(&n.to_le_bytes());
}
fn word(b: &mut [u8], at: usize, n: u16) {
    b[at..at + 2].copy_from_slice(&n.to_le_bytes());
}
fn fixture() -> Vec<u8> {
    let counts = [2usize, 2, 2, 1, 1, 2, 1, 1, 24];
    let sizes = [32, 88, 20, 16, 32, 32, 32768, 24, 80, 32];
    let mut offsets = [0usize; 10];
    let mut end = 128;
    for i in 0..10 {
        offsets[i] = end;
        end += sizes[i];
    }
    let mut b = vec![0; end];
    b[..8].copy_from_slice(b"HKSCNE01");
    put(&mut b, 8, 7);
    for (i, n) in counts.iter().enumerate() {
        put(&mut b, 12 + i * 4, *n as u32);
    }
    put(&mut b, 48, end as u32);
    put(&mut b, 52, 1);
    for (i, o) in offsets.iter().enumerate() {
        put(&mut b, 64 + i * 4, *o as u32);
    }
    let t = offsets[0];
    word(&mut b, t + 6, 1);
    word(&mut b, t + 8, 1);
    word(&mut b, t + 16, 65535);
    word(&mut b, t + 22, 1);
    word(&mut b, t + 24, 1);
    put(&mut b, t + 28, 20);
    for i in 0..2 {
        put(&mut b, offsets[1] + i * 44 + 4, 65536);
    }
    put(&mut b, offsets[2], 1);
    put(&mut b, offsets[3] + 4, 1);
    put(&mut b, offsets[3] + 8, 65536);
    put(&mut b, offsets[4] + 8, 65536);
    put(&mut b, offsets[4] + 16 + 8, 131072);
    b[offsets[7]] = 1;
    b[offsets[7] + 20] = 1;
    for i in 0..2 {
        let p = offsets[8] + i * 40;
        put(&mut b, p, 1 + i as u32 * 2);
        for s in 0..4 {
            put(&mut b, p + 4 + s * 4, 1);
            put(&mut b, p + 20 + s * 4, (offsets[9] + i * 16 + s * 4) as u32);
        }
        word(&mut b, offsets[9] + i * 16, i as u16);
        word(&mut b, offsets[9] + i * 16 + 12, i as u16);
    }
    b
}
fn offset(b: &[u8], section: usize) -> usize {
    crate::u32_at(b, 64 + section * 4) as usize
}

#[test]
fn draw_pool_identity_follows_each_room_reference() {
    let b = fixture();
    let scene = Scene::parse(&b).unwrap();
    assert_eq!(scene.room(0).unwrap().draw_pool_index(0), Some(0));
    assert_eq!(scene.room(1).unwrap().draw_pool_index(0), Some(1));
    let mut reordered = b.clone();
    let refs = offset(&reordered, 9);
    word(&mut reordered, refs, 1);
    word(&mut reordered, refs + 16, 0);
    let scene = Scene::parse(&reordered).unwrap();
    assert_eq!(scene.room(0).unwrap().draw_pool_index(0), Some(1));
    assert_eq!(scene.room(1).unwrap().draw_pool_index(0), Some(0));
}

fn incremental(bytes: &[u8], budget: usize) -> Result<(), Error> {
    let mut cursor = SceneValidation::new(bytes)?;
    for _ in 0..100_000 {
        if cursor.step(bytes, budget)? {
            return Ok(());
        }
    }
    panic!("cursor did not terminate");
}
#[test]
fn original_parser_error_order_matches_yielded_validation() {
    let b = fixture();
    for budget in [1, 8, 31, usize::MAX] {
        assert_eq!(
            incremental(&b, budget),
            Scene::parse_reference(&b).map(|_| ())
        );
    }
    // Every byte includes all pool records, descriptor/reference arrays, fixed
    // headers and padding; opaque page texels are correctly not reinterpreted.
    for at in 0..b.len() {
        let mut bad = b.clone();
        bad[at] ^= 0x80;
        let expected = Scene::parse_reference(&bad).map(|_| ());
        assert_eq!(incremental(&bad, 1), expected, "mutation {at}");
        assert_eq!(
            Scene::parse(&bad).map(|_| ()),
            expected,
            "direct mutation {at}"
        );
    }
    for len in 0..b.len() {
        assert_eq!(
            incremental(&b[..len], 8),
            Scene::parse_reference(&b[..len]).map(|_| ()),
            "length {len}"
        );
    }
}
#[test]
fn one_work_unit_never_hides_duplicate_or_record_loops_and_zero_is_idle() {
    let b = fixture();
    let mut cursor = SceneValidation::new(&b).unwrap();
    let mut seen = [false; 7];
    let mut steps = 0;
    loop {
        let before = (
            cursor.phase,
            cursor.room_index,
            cursor.section,
            cursor.index,
            cursor.next,
        );
        let done = cursor.step(&b, 0).unwrap();
        assert_eq!(
            before
                == (
                    cursor.phase,
                    cursor.room_index,
                    cursor.section,
                    cursor.index,
                    cursor.next
                ),
            true
        );
        assert_eq!(done, cursor.phase == Phase::Done);
        seen[match cursor.phase {
            Phase::Textures => 0,
            Phase::Descriptor => 1,
            Phase::Duplicate => 2,
            Phase::Section => 3,
            Phase::Record => 4,
            Phase::Tail => 5,
            Phase::Done => 6,
        }] = true;
        if done {
            break;
        }
        // A short arena is rejected at every phase before touching any record.
        assert_eq!(cursor.step(&b[..b.len() - 1], 8), Err(Error::Truncated));
        cursor.step(&b, 1).unwrap();
        steps += 1;
        if before.0 == Phase::Duplicate && before.3 < before.1 {
            assert_eq!(cursor.index, before.3 + 1);
            assert!(cursor.phase == Phase::Duplicate);
        }
        if before.0 == Phase::Record && before.3 < cursor.local_counts[before.2] {
            assert_eq!(cursor.index, before.3 + 1);
            assert!(cursor.phase == Phase::Record);
        }
    }
    assert!(seen.into_iter().all(|s| s));
    assert_eq!(steps, 36);
    assert_eq!(cursor.step(&b, 8), Ok(true));
}
#[test]
fn duplicate_chunk_is_rejected_on_its_own_bounded_comparison() {
    let mut b = fixture();
    let rooms = offset(&b, 8);
    put(&mut b, rooms + 40, 1);
    let mut cursor = SceneValidation::new(&b).unwrap();
    while cursor.phase != Phase::Duplicate || cursor.room_index != 1 {
        assert!(!cursor.step(&b, 1).unwrap());
    }
    assert_eq!(cursor.index, 0);
    assert_eq!(cursor.step(&b, 1), Err(Error::Reference));
}
#[test]
fn admitted_large_global_texture_ids_keep_local_frame_and_shared_stream_identity() {
    let mut b = fixture();
    let old_offsets: Vec<_> = (0..10).map(|i| offset(&b, i)).collect();
    let extra = 2046 * 16;
    let copy = b[old_offsets[0]..old_offsets[0] + 16].to_vec();
    let added: Vec<_> = copy.iter().copied().cycle().take(extra).collect();
    b.splice(old_offsets[1]..old_offsets[1], added);
    put(&mut b, 16, 2048);
    let len = b.len();
    put(&mut b, 48, len as u32);
    for i in 1..10 {
        put(&mut b, 64 + i * 4, (old_offsets[i] + extra) as u32);
    }
    let rooms = offset(&b, 8);
    for room in 0..2 {
        for section in 0..4 {
            let p = rooms + room * 40 + 20 + section * 4;
            let v = u32_at(&b, p);
            put(&mut b, p, v + extra as u32);
        }
    }
    let textures = offset(&b, 0);
    let streamed = b[textures + 16..textures + 32].to_vec();
    b[textures + 2047 * 16..textures + 2048 * 16].copy_from_slice(&streamed);
    let frame = offset(&b, 2);
    put(&mut b, frame, 2047);
    incremental(&b, 8).unwrap();
    let scene = Scene::parse(&b).unwrap();
    let a = scene.room(0).unwrap();
    let c = scene.room(1).unwrap();
    assert_eq!(u32_at(a.frame(0), 0), 2047);
    assert_eq!(a.frame(0).as_ptr(), c.frame(0).as_ptr());
    assert_eq!(a.stream_pixels(a.texture(2047)), Some(&[1, 0][..]));
    assert_eq!(
        a.stream_pixels(a.texture(2047)).unwrap().as_ptr(),
        c.stream_pixels(c.texture(2047)).unwrap().as_ptr()
    );
    assert_eq!(a.edge(0)[2], 65536);
    assert_eq!(c.edge(0)[2], 131072);
}
