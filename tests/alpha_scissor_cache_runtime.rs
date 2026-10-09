use super::{Entry, Secondary, UNPREPARED};
fn cover(rects: &[[u8; 4]]) -> [u8; 20] {
    let mut c = [0; 20];
    c[0] = rects.len() as u8;
    for (i, r) in rects.iter().enumerate() {
        c[4 + i * 4..8 + i * 4].copy_from_slice(r);
    }
    c
}
fn quad(left: i16, top: i16, dx: i16, dy: i16) -> [(i16, i16); 4] {
    [
        (left, top),
        (left + dx, top),
        (left, top + dy),
        (left + dx, top + dy),
    ]
}
#[test]
fn bounded_storage() {
    assert_eq!(core::mem::size_of::<Entry>(), 40);
    assert_eq!(core::mem::size_of::<Secondary>(), 5184);
    assert!(Secondary::EMPTY
        .entries
        .iter()
        .flatten()
        .all(|e| e.count == UNPREPARED));
}
#[test]
fn exact_keys_two_way_hits_and_eviction() {
    let mut cache = Secondary::EMPTY;
    let mut out = Entry::EMPTY;
    let spans = [191, -173];
    let bucket = Secondary::bucket(0, spans);
    let keys: Vec<_> = (0..10000u16)
        .filter(|&t| Secondary::bucket(t, spans) == bucket)
        .take(3)
        .collect();
    let c = cover(&[[4, 3, 20, 21]]);
    for &t in &keys[..2] {
        cache.prepare(&mut out, spans, t, 32, 32, &c);
    }
    let victim = cache.victim[bucket];
    cache.prepare(&mut out, spans, keys[0], 32, 32, &c);
    assert_eq!(
        cache.victim[bucket], victim,
        "hit must not run preparation/insertion"
    );
    assert_eq!((out.texture, out.spans), (keys[0], spans));
    cache.prepare(&mut out, spans, keys[2], 32, 32, &c);
    assert!(!cache.entries[bucket].iter().any(|e| e.texture == keys[0]));
    assert!(cache.entries[bucket].iter().any(|e| e.texture == keys[1]));
    cache.prepare(&mut out, spans, keys[0], 32, 32, &c);
    assert!(!cache.entries[bucket].iter().any(|e| e.texture == keys[1]));
}
#[test]
fn bank_reset_changes_same_key_metadata() {
    let mut cache = Secondary::EMPTY;
    let mut old = Entry::EMPTY;
    let mut new = Entry::EMPTY;
    let xy = quad(-20, 5, 280, 210);
    let a = cover(&[[0, 0, 7, 31]]);
    let b = cover(&[[20, 0, 11, 31]]);
    let first = new.map_cached(xy, 4, 32, 32, &a, &mut cache);
    cache.invalidate();
    old.invalidate();
    new.invalidate();
    assert!(cache
        .entries
        .iter()
        .flatten()
        .all(|e| e.count == UNPREPARED));
    let expected = old.map(xy, 4, 32, 32, &b);
    assert_ne!(first, expected);
    assert_eq!(new.map_cached(xy, 4, 32, 32, &b, &mut cache), expected);
}
#[test]
fn translated_clipped_reflections_and_alternating_phases() {
    let mut cache = Secondary::EMPTY;
    let mut old = [Entry::EMPTY; 32];
    let mut new = [Entry::EMPTY; 32];
    let covers = [
        cover(&[
            [0, 0, 7, 63],
            [16, 0, 7, 63],
            [32, 0, 7, 63],
            [56, 0, 7, 63],
        ]),
        cover(&[[4, 7, 40, 29]]),
        cover(&[]),
        cover(&[[0, 0, 63, 63]]),
    ];
    for frame in 0..160 {
        for i in 0..32 {
            let c = &covers[i % 4];
            let dx = 173 + ((frame + i) & 1) as i16;
            let dy = 137 + ((frame / 2 + i) & 1) as i16;
            let dx = if i & 4 != 0 { -dx } else { dx };
            let dy = if i & 8 != 0 { -dy } else { dy };
            let xy = quad(
                (frame * 7 % 700) as i16 - 190,
                (frame * 11 % 650) as i16 - 170,
                dx,
                dy,
            );
            let expected = old[i].map(xy, i as u16, 64, 64, c);
            assert_eq!(
                new[i].map_cached(xy, i as u16, 64, 64, c, &mut cache),
                expected,
                "frame {frame} draw {i}"
            );
        }
    }
}
#[test]
fn fallback_empty_invalid_and_geometry_rejection_parity() {
    let mut cache = Secondary::EMPTY;
    let covers = [
        cover(&[[0, 0, 31, 31]]),
        cover(&[]),
        cover(&[[30, 30, 10, 10]]),
        [255; 20],
        cover(&[[3, 4, 13, 12]]),
    ];
    let geometries = [
        quad(0, 0, 320, 240),
        quad(0, 0, 0, 240),
        quad(0, 0, 320, 0),
        quad(0, 0, 1023, 511),
        quad(0, 0, 1024, 511),
        quad(0, 0, 1023, 512),
        quad(319, 239, -319, -239),
        quad(-320, -240, 320, 240),
        [(0, 0), (320, 1), (0, 240), (320, 240)],
    ];
    for width in [0, 1, 32, 256, 257] {
        cache.invalidate();
        for (i, c) in covers.iter().enumerate() {
            let mut old = Entry::EMPTY;
            let mut new = Entry::EMPTY;
            for &v in &geometries {
                assert_eq!(
                    new.map_cached(v, i as u16, width, 32, c, &mut cache),
                    old.map(v, i as u16, width, 32, c)
                );
            }
        }
    }
}
#[test]
fn randomized_eviction_and_boundary_differential() {
    let mut cache = Secondary::EMPTY;
    let mut old = [Entry::EMPTY; 480];
    let mut new = [Entry::EMPTY; 480];
    let mut r = 0x56e25103u32;
    let covers = [
        cover(&[[0, 0, 19, 63], [32, 0, 15, 63]]),
        cover(&[[10, 10, 32, 32]]),
    ];
    for n in 0..100000 {
        r = r.wrapping_mul(1664525).wrapping_add(1013904223);
        let i = (r as usize >> 8) % 480;
        let t = (i % 193) as u16;
        let dx = 31 + ((r >> 16) % 993) as i16;
        let dy = 17 + ((r >> 6) % 495) as i16;
        let dx = if r & 1 != 0 { -dx } else { dx };
        let dy = if r & 2 != 0 { -dy } else { dy };
        let v = quad(
            ((r >> 10) % 700) as i16 - 190,
            ((r >> 20) % 500) as i16 - 130,
            dx,
            dy,
        );
        let c = &covers[t as usize % 2];
        assert_eq!(
            new[i].map_cached(v, t, 64, 64, c, &mut cache),
            old[i].map(v, t, 64, 64, c),
            "iteration {n}"
        );
        if n % 8191 == 0 {
            cache.invalidate();
            for e in &mut old {
                e.invalidate();
            }
            for e in &mut new {
                e.invalidate();
            }
        }
    }
}

fn assert_entry_same(a: &Entry, b: &Entry) {
    assert_eq!(a.rects, b.rects);
    assert_eq!(a.spans, b.spans);
    assert_eq!(
        (a.texture, a.count, a.reserved),
        (b.texture, b.count, b.reserved)
    );
}
fn compare_output(
    old: &mut Entry,
    new: &mut Entry,
    old_cache: &mut Secondary,
    new_cache: &mut Secondary,
    xy: [(i16, i16); 4],
    texture: u16,
    width: u16,
    height: u16,
    c: &[u8],
    out: &mut super::MappedScissors,
) {
    let expected = old.map_cached(xy, texture, width, height, c, old_cache);
    let actual = new.map_cached_into(xy, texture, width, height, c, new_cache, out);
    assert_eq!(actual, expected.is_some());
    if let Some(expected) = expected {
        assert_eq!(out.as_slice(), &expected.rects[..expected.count]);
        assert_eq!(out.saved_pixels(), expected.saved_pixels);
    } else {
        assert_eq!(out.len(), 0);
        assert_eq!(out.saved_pixels(), 0);
    }
    assert_entry_same(old, new);
    assert_eq!(old_cache.victim, new_cache.victim);
    // Mapping output must not change preparation, hit, or eviction behaviour.
    for (a, b) in old_cache
        .entries
        .iter()
        .flatten()
        .zip(new_cache.entries.iter().flatten())
    {
        assert_entry_same(a, b);
    }
}
#[test]
fn in_place_result_reuses_poisoned_storage_across_all_mapping_outcomes() {
    use core::mem::MaybeUninit;
    let mut storage = MaybeUninit::<super::MappedScissors>::uninit();
    unsafe {
        storage
            .as_mut_ptr()
            .cast::<u8>()
            .write_bytes(0xa5, core::mem::size_of::<super::MappedScissors>());
    }
    let out = super::MappedScissors::initialize(&mut storage);
    assert!(out.as_slice().is_empty());
    assert_eq!(out.saved_pixels(), 0);
    let mut old = Entry::EMPTY;
    let mut new = Entry::EMPTY;
    let mut old_cache = Secondary::EMPTY;
    let mut new_cache = Secondary::EMPTY;
    let covers = [
        cover(&[[0, 0, 7, 31], [16, 0, 7, 31]]),
        cover(&[]),
        cover(&[[0, 0, 31, 31]]),
        cover(&[[30, 30, 10, 10]]),
        [255; 20],
        cover(&[[3, 4, 13, 12]]),
        cover(&[
            [3, 4, 13, 12],
            [3, 4, 13, 12],
            [3, 4, 13, 12],
            [3, 4, 13, 12],
        ]),
    ];
    let geometries = [
        quad(0, 0, 320, 240),
        quad(0, 0, 0, 240),
        quad(0, 0, 320, 0),
        quad(0, 0, 1023, 511),
        quad(0, 0, 1024, 511),
        quad(0, 0, 1023, 512),
        quad(319, 239, -319, -239),
        quad(-320, -240, 320, 240),
        [(0, 0), (320, 1), (0, 240), (320, 240)],
        quad(310, 230, 10, 10),
    ];
    for width in [0, 1, 32, 256, 257] {
        for (texture, c) in covers.iter().enumerate() {
            old_cache.invalidate();
            new_cache.invalidate();
            old.invalidate();
            new.invalidate();
            for &xy in &geometries {
                compare_output(
                    &mut old,
                    &mut new,
                    &mut old_cache,
                    &mut new_cache,
                    xy,
                    texture as u16,
                    width,
                    32,
                    c,
                    out,
                );
            }
        }
    }
    // Count-zero success is distinct from fallback, including reuse after data.
    old.invalidate();
    new.invalidate();
    old_cache.invalidate();
    new_cache.invalidate();
    compare_output(
        &mut old,
        &mut new,
        &mut old_cache,
        &mut new_cache,
        quad(0, 0, 320, 240),
        0,
        32,
        32,
        &cover(&[]),
        out,
    );
    assert_eq!(out.len(), 0);
    assert_eq!(out.saved_pixels(), 76800);
}
#[test]
fn in_place_result_randomized_cache_and_bank_differential() {
    let mut old_cache = Secondary::EMPTY;
    let mut new_cache = Secondary::EMPTY;
    let mut old = [Entry::EMPTY; 480];
    let mut new = [Entry::EMPTY; 480];
    let mut storage = core::mem::MaybeUninit::uninit();
    let out = super::MappedScissors::initialize(&mut storage);
    let covers = [
        cover(&[[0, 0, 19, 63], [32, 0, 15, 63]]),
        cover(&[[10, 10, 32, 32]]),
        cover(&[]),
        cover(&[[0, 0, 63, 63]]),
    ];
    let mut seed = 0x66152837u32;
    for n in 0..100000 {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let i = (seed as usize >> 8) % 480;
        let texture = (i % 193) as u16;
        let dx = 1 + ((seed >> 16) % 1023) as i16;
        let dy = 1 + ((seed >> 6) % 511) as i16;
        let xy = quad(
            ((seed >> 10) % 1000) as i16 - 340,
            ((seed >> 20) % 750) as i16 - 255,
            if seed & 1 == 0 { dx } else { -dx },
            if seed & 2 == 0 { dy } else { -dy },
        );
        let c = &covers[(texture as usize + n / 8191) % covers.len()];
        compare_output(
            &mut old[i],
            &mut new[i],
            &mut old_cache,
            &mut new_cache,
            xy,
            texture,
            64,
            64,
            c,
            out,
        );
        if (n + 1) % 8191 == 0 {
            old_cache.invalidate();
            new_cache.invalidate();
            for e in &mut old {
                e.invalidate();
            }
            for e in &mut new {
                e.invalidate();
            }
        }
    }
}
