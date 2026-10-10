#[allow(clippy::all, unexpected_cfgs, dead_code)] // game source, linted with the game
#[path = "../../../game/src/alpha_scissor_cache.rs"]
mod cached;
#[allow(clippy::all, unexpected_cfgs, dead_code)] // game source, linted with the game
#[path = "../../../game/src/alpha_scissor.rs"]
mod original;
use cached::Scissors;
fn map(v: [(i16, i16); 4], w: u16, h: u16, c: &[u8]) -> Option<Scissors> {
    {
        let mut e = cached::Entry::EMPTY;
        e.map(v, 0, w, h, c)
    }
}
fn cover(rects: &[[u16; 4]]) -> [u8; 20] {
    let mut result = [0; 20];
    result[0] = rects.len() as u8;
    for (i, r) in rects.iter().enumerate() {
        result[4 + i * 4..8 + i * 4].copy_from_slice(&[
            r[0] as u8,
            r[1] as u8,
            (r[2] - r[0] - 1) as u8,
            (r[3] - r[1] - 1) as u8,
        ]);
    }
    result
}
fn quad(x: i16, y: i16, w: i16, h: i16, flip: u8) -> [(i16, i16); 4] {
    let (l, r) = if flip & 1 != 0 {
        (x + w, x)
    } else {
        (x, x + w)
    };
    let (t, b) = if flip & 2 != 0 {
        (y + h, y)
    } else {
        (y, y + h)
    };
    [(l, t), (r, t), (l, b), (r, b)]
}
// Independent per-screen-pixel DDA oracle, intentionally no inverse intervals.
fn samples(a: i16, b: i16, size: u16, screen: i32) -> Vec<(i16, i32)> {
    let (a, b) = (a as i32, b as i32);
    let span = (b - a).abs();
    let step = if b < a {
        -((size as i32 - 1) * 4096 / span)
    } else {
        (size as i32 - 1) * 4096 / span
    };
    let seed = if b < a {
        (size as i32 - 1) * 4096 + 2048
    } else {
        2048
    };
    (a.min(b).max(0)..a.max(b).min(screen))
        .map(|p| (p as i16, (seed + (p - a.min(b)) * step) >> 12))
        .collect()
}
fn reference(v: [(i16, i16); 4], w: u16, h: u16, c: &[u8; 20]) -> Option<Scissors> {
    let xs = samples(v[0].0, v[1].0, w, 320);
    let ys = samples(v[0].1, v[2].1, h, 240);
    if xs.is_empty() || ys.is_empty() {
        return None;
    }
    let mut out = Scissors {
        rects: [[0; 4]; 4],
        count: 0,
        saved_pixels: (xs.len() * ys.len()) as u32,
    };
    for i in 0..c[0] as usize {
        let p = 4 + i * 4;
        let (l, t, r, b) = (
            c[p] as i32,
            c[p + 1] as i32,
            c[p] as i32 + c[p + 2] as i32 + 1,
            c[p + 1] as i32 + c[p + 3] as i32 + 1,
        );
        let xx: Vec<_> = xs.iter().filter(|(_, u)| l <= *u && *u < r).collect();
        let yy: Vec<_> = ys.iter().filter(|(_, v)| t <= *v && *v < b).collect();
        if !xx.is_empty() && !yy.is_empty() {
            out.rects[out.count] = [
                xx[0].0,
                yy[0].0,
                xx.last().unwrap().0 + 1,
                yy.last().unwrap().0 + 1,
            ];
            out.count += 1;
            out.saved_pixels -= (xx.len() * yy.len()) as u32;
        }
    }
    if out.saved_pixels > 128u32.max(128 * out.count.saturating_sub(1) as u32) {
        Some(out)
    } else {
        None
    }
}
fn occupancy(v: [(i16, i16); 4], w: u16, h: u16, c: &[u8; 20], out: Scissors) {
    for (x, u) in samples(v[0].0, v[1].0, w, 320) {
        for (y, t) in samples(v[0].1, v[2].1, h, 240) {
            let expected = (0..c[0] as usize).any(|i| {
                let p = 4 + i * 4;
                u >= c[p] as i32
                    && u < c[p] as i32 + c[p + 2] as i32 + 1
                    && t >= c[p + 1] as i32
                    && t < c[p + 1] as i32 + c[p + 3] as i32 + 1
            });
            let count = out.rects[..out.count]
                .iter()
                .filter(|r| r[0] <= x && x < r[2] && r[1] <= y && y < r[3])
                .count();
            assert_eq!(count, expected as usize, "screen{x},{y} texel{u},{t}");
        }
    }
}
#[test]
fn dimensions_reflections_clipping_and_fractional_dda_match_pixel_reference() {
    let dimensions = [1, 2, 3, 7, 16, 31, 64, 127, 255, 256];
    let spans = [1, 2, 3, 7, 17, 63, 127, 319, 320, 511, 1023];
    let origins = [
        (-1023, -511),
        (-37, -19),
        (-1, -1),
        (0, 0),
        (101, 83),
        (319, 239),
        (320, 240),
    ];
    let mut checks = 0;
    for &w in &dimensions {
        for &h in &dimensions {
            for (i, &span) in spans.iter().enumerate() {
                let hs = spans[(i + 3) % spans.len()].min(511);
                for &(x, y) in &origins {
                    for flip in 0..4 {
                        let v = quad(x, y, span, hs, flip);
                        let cases = [
                            cover(&[]),
                            cover(&[[0, 0, w, h]]),
                            cover(&[[
                                w / 3,
                                h / 3,
                                (2 * w / 3 + 1).min(w),
                                (2 * h / 3 + 1).min(h),
                            ]]),
                        ];
                        for c in cases {
                            assert_eq!(
                                map(v, w, h, &c),
                                reference(v, w, h, &c),
                                "{v:?} tex{w}x{h} cover{c:?}"
                            );
                            checks += 1;
                        }
                    }
                }
            }
        }
    }
    assert_eq!(checks, 92400);
}
#[test]
fn four_disjoint_covers_have_exact_nonoverlapping_screen_occupancy() {
    let c = cover(&[
        [0, 0, 5, 7],
        [7, 0, 13, 8],
        [0, 10, 8, 16],
        [11, 12, 16, 16],
    ]);
    for v in [
        quad(-17, -31, 333, 277, 0),
        quad(-17, -31, 333, 277, 1),
        quad(-17, -31, 333, 277, 2),
        quad(-17, -31, 333, 277, 3),
        quad(301, 201, 70, 64, 3),
    ] {
        let out = map(v, 16, 16, &c).unwrap();
        assert_eq!(Some(out), reference(v, 16, 16, &c));
        occupancy(v, 16, 16, &c, out);
    }
}
#[test]
fn conservative_fallback_and_strict_savings_threshold() {
    let empty = cover(&[]);
    assert!(map(quad(0, 0, 16, 8, 0), 16, 8, &empty).is_none());
    assert_eq!(
        map(quad(0, 0, 43, 3, 0), 1, 1, &empty)
            .unwrap()
            .saved_pixels,
        129
    );
    assert!(map(
        quad(0, 0, 320, 240, 0),
        256,
        256,
        &cover(&[[0, 0, 256, 256]])
    )
    .is_none());
    for (w, h) in [(0, 16), (16, 0), (257, 16), (16, 257)] {
        assert!(map(quad(0, 0, 32, 32, 0), w, h, &empty).is_none());
    }
    for v in [
        quad(0, 0, 0, 32, 0),
        quad(0, 0, 32, 0, 0),
        quad(0, 0, 1024, 32, 0),
        quad(0, 0, 32, 512, 0),
        quad(i16::MIN, i16::MIN, 1, 1, 0),
        [(0, 0), (32, 1), (0, 32), (32, 32)],
    ] {
        assert!(map(v, 256, 256, &empty).is_none());
    }
    assert!(map(quad(0, 0, 32, 32, 0), 16, 16, &empty[..19]).is_none());
    let mut invalid = empty;
    invalid[0] = 5;
    assert!(map(quad(0, 0, 32, 32, 0), 16, 16, &invalid).is_none());
    invalid = empty;
    invalid[2] = 1;
    assert!(map(quad(0, 0, 32, 32, 0), 16, 16, &invalid).is_none());
    assert!(map(quad(0, 0, 32, 32, 0), 16, 16, &cover(&[[15, 0, 17, 2]])).is_none());
}

#[test]
fn expansion_threshold_uses_visible_scissor_count_and_strict_comparison() {
    // span = texture_width-1 gives exactly one texel per screen pixel here.
    for (height, rects, threshold) in [
        (1, vec![[0, 0, 63, 1], [128, 0, 192, 1]], 128),
        (
            2,
            vec![[0, 0, 41, 1], [64, 0, 107, 1], [128, 0, 171, 1]],
            256,
        ),
        (
            3,
            vec![
                [0, 0, 31, 1],
                [64, 0, 96, 1],
                [128, 0, 160, 1],
                [192, 0, 224, 1],
            ],
            384,
        ),
    ] {
        let v = quad(0, 0, 255, height, 0);
        assert!(map(v, 256, 1, &cover(&rects)).is_none());
        let mut smaller = rects;
        smaller[0][2] -= 1;
        let out = map(v, 256, 1, &cover(&smaller)).unwrap();
        assert_eq!(out.saved_pixels, threshold + height as u32);
        assert_eq!(out.count, smaller.len());
    }
}

fn original_as_cached(v: [(i16, i16); 4], w: u16, h: u16, c: &[u8]) -> Option<Scissors> {
    original::map(v, w, h, c).map(|s| Scissors {
        rects: s.rects,
        count: s.count,
        saved_pixels: s.saved_pixels,
    })
}
#[test]
fn warm_translation_and_key_changes() {
    assert_eq!(std::mem::size_of::<cached::Entry>(), 40);
    let zero = cached::Entry::EMPTY;
    let bytes = unsafe { std::slice::from_raw_parts(&zero as *const _ as *const u8, 40) };
    assert!(
        bytes.iter().all(|b| *b == 0),
        "cache must remain zero-filled BSS"
    );
    let mut checks = 0;
    for (w, h) in [(1, 256), (256, 1), (2, 3), (11, 13), (31, 48), (256, 256)] {
        let covers = [
            cover(&[]),
            cover(&[[0, 0, w, h]]),
            cover(&[[w / 3, h / 3, (2 * w / 3 + 1).min(w), (2 * h / 3 + 1).min(h)]]),
        ];
        for (texture, c) in covers.iter().enumerate() {
            let mut entry = cached::Entry::EMPTY;
            for (sx, sy) in [
                (7, 13),
                (137, 143),
                (511, 319),
                (1023, 511),
                (1023, 510),
                (1022, 511),
                (511, 319),
            ] {
                for flip in 0..4 {
                    for y in [-511, -240, -19, -1, 0, 37, 239, 240, 511] {
                        for x in [-1023, -320, -319, -17, -1, 0, 101, 319, 320, 1023] {
                            let v = quad(x, y, sx, sy, flip);
                            assert_eq!(
                                entry.map(v, texture as u16, w, h, c),
                                original_as_cached(v, w, h, c),
                                "warm{v:?} size{w},{h}"
                            );
                            checks += 1;
                        }
                    }
                }
            }
        }
    }
    println!("{checks} warm/key parity cases");
}
#[test]
fn texture_identity_reset_and_fallbacks() {
    let mut e = cached::Entry::EMPTY;
    let v = quad(-17, -31, 333, 277, 3);
    let cases = [
        cover(&[]),
        cover(&[[0, 0, 16, 16]]),
        cover(&[[0, 0, 4, 5], [7, 8, 16, 16]]),
    ];
    for _ in 0..3 {
        for (i, c) in cases.iter().enumerate() {
            assert_eq!(
                e.map(v, i as u16, 16, 16, c),
                original_as_cached(v, 16, 16, c)
            );
        }
    }
    // The same numeric texture ID is a different immutable asset after room init.
    e.invalidate();
    assert_eq!(
        e.map(v, 0, 16, 16, &cases[2]),
        original_as_cached(v, 16, 16, &cases[2])
    );
    for bad in [
        vec![],
        vec![0; 19],
        vec![0; 21],
        vec![5; 20],
        vec![1, 0, 0, 0, 15, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    ] {
        e = cached::Entry::EMPTY;
        assert_eq!(
            e.map(v, 0, 16, 16, &bad),
            original_as_cached(v, 16, 16, &bad)
        );
    }
    let c = cover(&[[0, 0, 3, 4]]);
    for bad in [
        quad(0, 0, 1024, 511, 0),
        quad(0, 0, 320, 512, 0),
        quad(0, 0, 0, 12, 0),
        [(0, 0), (40, 1), (1, 40), (40, 40)],
    ] {
        assert_eq!(
            e.map(bad, 2, 16, 16, &c),
            original_as_cached(bad, 16, 16, &c)
        );
    }
}

#[test]
fn immutable_fallback_survives_every_geometry_but_not_texture_lease_reset() {
    let partial = cover(&[[3, 2, 7, 9]]);
    let fallback_covers = [
        cover(&[[0, 0, 16, 16]]).to_vec(),
        vec![255; 20],
        vec![0; 19],
        cover(&[[15, 0, 17, 3]]).to_vec(),
        vec![4, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    ];
    for c in fallback_covers {
        let mut e = cached::Entry::EMPTY;
        let first = quad(0, 0, 320, 240, 0);
        assert_eq!(
            e.map(first, 2, 16, 16, &c),
            original_as_cached(first, 16, 16, &c)
        );
        for v in [
            quad(-200, -37, 511, 319, 3),
            quad(0, 0, 7, 3, 0),
            quad(0, 0, 1024, 512, 0),
            quad(320, 240, 333, 277, 0),
            quad(-1023, -511, 1023, 511, 0),
            [(0, 0), (40, 1), (1, 40), (40, 40)],
        ] {
            assert_eq!(e.map(v, 2, 16, 16, &c), original_as_cached(v, 16, 16, &c));
        }
        // Changing texture IDs must invalidate cached metadata rejection.
        assert_eq!(
            e.map(first, 3, 16, 16, &partial),
            original_as_cached(first, 16, 16, &partial)
        );
        assert!(e.map(first, 3, 16, 16, &partial).is_some());
        e.invalidate();
        assert_eq!(
            e.map(first, 2, 16, 16, &partial),
            original_as_cached(first, 16, 16, &partial)
        );
    }
}
