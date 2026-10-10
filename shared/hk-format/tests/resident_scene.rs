use hk_format::{Error, Scene};
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
    hk_format::u32_at(b, 64 + section * 4) as usize
}
fn compact_fixture() -> Vec<u8> {
    let raw = fixture();
    let start = offset(&raw, 5);
    let end = offset(&raw, 7);
    let gap = end - start;
    let mut b = [&raw[..start], &raw[end..]].concat();
    b[..8].copy_from_slice(b"HKSCNE02");
    let n = b.len();
    put(&mut b, 48, n as u32);
    for i in 0..10 {
        let old = offset(&raw, i);
        put(
            &mut b,
            64 + i * 4,
            if old <= start {
                old
            } else if old < end {
                start
            } else {
                old - gap
            } as u32,
        );
    }
    let rooms = offset(&b, 8);
    for i in 0..2 {
        for j in 0..4 {
            let at = rooms + i * 40 + 20 + j * 4;
            let old = hk_format::u32_at(&b, at);
            put(&mut b, at, old - gap as u32);
        }
    }
    b
}
#[test]
fn compact_scene_preserves_all_runtime_access_and_rejects_discarded_payload_access() {
    let raw = fixture();
    let b = compact_fixture();
    let old = Scene::parse(&raw).unwrap();
    let new = Scene::parse(&b).unwrap();
    assert_eq!(old.page_count(), new.page_count());
    assert_eq!(old.palette_count(), new.palette_count());
    for i in 0..2 {
        let a = old.room(i).unwrap();
        let c = new.room(i).unwrap();
        assert!(a.has_atlas_payload());
        assert!(!c.has_atlas_payload());
        assert_eq!(a.counts, c.counts);
        assert_eq!(a.draw(0), c.draw(0));
        assert_eq!(a.frame(0), c.frame(0));
        assert_eq!(a.clip(0), c.clip(0));
        assert_eq!(a.edge(0), c.edge(0));
        assert_eq!(a.draw_pool_index(0), c.draw_pool_index(0));
        assert_eq!(a.alpha_cover(a.texture(0)), c.alpha_cover(c.texture(0)));
        assert_eq!(a.stream_pixels(a.texture(1)), c.stream_pixels(c.texture(1)));
        assert!(std::panic::catch_unwind(|| c.page(0)).is_err());
        assert!(std::panic::catch_unwind(|| c.palettes()).is_err());
    }
}
#[test]
fn compact_scene_checks_logical_atlas_counts_relocated_references_and_exact_flags() {
    let b = compact_fixture();
    for (at, value) in [
        (52, 3),
        (52, 0),
        (88, offset(&b, 6) as u32 + 4),
        (40, 0),
        (36, 0),
    ] {
        let mut bad = b.clone();
        put(&mut bad, at, value);
        assert!(Scene::parse(&bad).is_err(), "field{at}");
    }
    let mut bad = b.clone();
    bad[..8].copy_from_slice(b"HKSCNE01");
    assert!(Scene::parse(&bad).is_err());
    let mut bad = b.clone();
    let at = offset(&b, 8) + 20;
    put(&mut bad, at, offset(&fixture(), 9) as u32);
    assert!(Scene::parse(&bad).is_err());
    let mut bad = b.clone();
    let textures = offset(&b, 0);
    word(&mut bad, textures + 10, 1);
    assert!(Scene::parse(&bad).is_err());
    let mut bad = b.clone();
    word(&mut bad, textures, 1);
    assert!(Scene::parse(&bad).is_err());
}
#[test]
fn scene_rooms_preserve_local_order_and_share_pixels_without_copying() {
    let b = fixture();
    let s = Scene::parse(&b).unwrap();
    assert_eq!(s.id(), 7);
    assert_eq!(s.room_count(), 2);
    let a = s.room(0).unwrap();
    let c = s.room_by_chunk(3).unwrap();
    assert!(a.scene_resident());
    assert_eq!(a.counts, [1, 2, 1, 1, 1, 1]);
    assert_eq!(a.edge(0)[2], 65536);
    assert_eq!(c.edge(0)[2], 131072);
    assert_eq!(a.texture(1).palette, 0);
    assert_eq!(a.stream_pixels(a.texture(1)), Some(&[1, 0][..]));
    assert_eq!(a.page(0).as_ptr(), c.page(0).as_ptr());
    assert_eq!(a.palettes().len(), 32);
    assert!(s.room(2).is_none());
    assert!(s.room_by_chunk(2).is_none());
    let admitted = unsafe { Scene::validated_view(&b) };
    assert_eq!(admitted.room(1).unwrap().edge(0), c.edge(0));
}
#[test]
fn malformed_header_offsets_padding_and_truncation_never_publish() {
    let b = fixture();
    for length in [0, 7, 40, 127, 128, b.len() - 1] {
        assert!(Scene::parse(&b[..length]).is_err());
    }
    for (at, value) in [
        (12, u32::MAX),
        (16, 2049),
        (36, 1537),
        (40, 21),
        (44, u32::MAX),
        (48, 0),
        (52, 0),
        (56, 1),
        (104, 1),
        (64, 132),
    ] {
        let mut bad = b.clone();
        put(&mut bad, at, value);
        assert!(Scene::parse(&bad).is_err(), "field{at}");
    }
    let mut bad = b.clone();
    let refs = offset(&b, 9);
    bad[refs + 2] = 1;
    assert!(Scene::parse(&bad).is_err());
}
#[test]
fn every_indirect_reference_is_validated_before_record_access() {
    let b = fixture();
    let refs = offset(&b, 9);
    for at in [refs, refs + 4, refs + 8, refs + 12] {
        let mut bad = b.clone();
        word(&mut bad, at, 65535);
        assert!(matches!(Scene::parse(&bad), Err(Error::Reference)));
    }
    let mut bad = b.clone();
    let rooms = offset(&b, 8);
    put(&mut bad, rooms + 20, u32::MAX);
    assert!(Scene::parse(&bad).is_err());
    let mut bad = b.clone();
    put(&mut bad, rooms + 40, 1);
    assert!(matches!(Scene::parse(&bad), Err(Error::Reference)));
}
#[test]
fn global_palette_bounds_and_room_local_clip_ranges_are_distinct() {
    let b = fixture();
    let mut bad = b.clone();
    let textures = offset(&b, 0);
    word(&mut bad, textures + 10, 1);
    assert!(matches!(Scene::parse(&bad), Err(Error::Reference))); // <texture count but >=palette count
    let mut bad = b.clone();
    let clip = offset(&b, 3);
    put(&mut bad, clip, 1);
    assert!(Scene::parse(&bad).is_err());
    let mut bad = b.clone();
    let stream = offset(&b, 7);
    bad[stream] = 5;
    assert!(Scene::parse(&bad).is_err());
}
