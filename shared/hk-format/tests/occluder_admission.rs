#[allow(clippy::all, unexpected_cfgs, dead_code)] // game source, linted with the game
#[path = "../../../game/src/occluder_admission.rs"]
mod admission;

// Existing collector projection and axis/bounding-box tests, independent of
// the new unprojected admission math. i64 subtraction additionally exercises
// i32 endpoint extremes without constructing overflowing guest coordinates.
fn qualifies(xy: &[i32; 8], cx: i64, cy: i64, minimum: i32) -> bool {
    let v: [(i64, i64); 4] = core::array::from_fn(|k| {
        (
            160 + ((i64::from(xy[2 * k]) - cx) >> 8),
            120 - ((i64::from(xy[2 * k + 1]) - cy) >> 8),
        )
    });
    if v[0].1 != v[1].1 || v[0].0 != v[2].0 || v[1].0 != v[3].0 || v[2].1 != v[3].1 {
        return false;
    }
    let w = (v[0].0.max(v[1].0).min(320) - v[0].0.min(v[1].0).max(0)).max(0);
    let h = (v[0].1.max(v[2].1).min(240) - v[0].1.min(v[2].1).max(0)).max(0);
    w * h >= i64::from(minimum) * 2
}
#[test]
fn fractional_spans_reflections_and_nearly_aligned_vertices_never_false_reject() {
    let mut checked = 0;
    for flip in 0..4 {
        for (w, h, skew) in [
            (16384, 16384, 0),
            (16383, 16383, 0),
            (16385, 16385, 0),
            (16384, 16384, 255),
            (16384, 16384, 256),
            (255, 100000, 0),
            (256, 100000, 0),
        ] {
            let (l, r) = if flip & 1 == 0 { (0, w) } else { (w, 0) };
            let (t, b) = if flip & 2 == 0 { (0, h) } else { (h, 0) };
            let xy = [l, t, r, t + skew, l, b, r, b];
            for threshold in [2048, 2060] {
                let admitted = admission::possible(&xy, threshold);
                for fx in 0..256 {
                    for fy in 0..256 {
                        if qualifies(&xy, i64::from(w / 2 + fx), i64::from(h / 2 + fy), threshold) {
                            assert!(admitted, "{xy:?} phase{fx},{fy} threshold{threshold}");
                        }
                        checked += 1;
                    }
                }
            }
        }
    }
    assert_eq!(checked, 3_670_016);
}
#[test]
fn full_integer_extremes_and_exact_area_boundaries() {
    let huge = [
        i32::MIN,
        i32::MIN,
        i32::MAX,
        i32::MIN,
        i32::MIN,
        i32::MAX,
        i32::MAX,
        i32::MAX,
    ];
    assert!(admission::possible(&huge, 38400));
    assert!(!admission::possible(&huge, 38401));
    let square = [0, 0, 64 * 256, 0, 0, 64 * 256, 64 * 256, 64 * 256];
    assert!(admission::possible(&square, 2048));
    assert!(!admission::possible(&square, 2049));
    assert!(!admission::possible(&[0; 8], 1));
}
#[test]
fn cooked_source_draws_match_original_projection_for_random_world_cameras() {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/regions");
    if !directory.exists() {
        return;
    } // Synthetic proof tests require no retail assets.
    let mut paths: Vec<_> = std::fs::read_dir(directory)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "hk"))
        .collect();
    paths.sort();
    let mut checked = 0;
    let mut rng = 0x12345678u32;
    for path in paths {
        let bytes = std::fs::read(path).unwrap();
        let room = hk_format::Room::parse(&bytes).unwrap();
        for i in 0..room.counts[2] {
            let draw = room.draw(i);
            let xy = core::array::from_fn(|k| hk_format::i32_at(draw, 8 + 4 * k));
            let scale = i64::from(hk_format::i32_at(draw, 4));
            let admitted = admission::possible(&xy, 2048);
            for _ in 0..64 {
                let camera: [i64; 2] = core::array::from_fn(|_| {
                    rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
                    i64::from(rng % (210 * 65536))
                });
                let [cx, cy] = camera.map(|q| ((q >> 8) * scale) >> 12);
                if qualifies(&xy, cx, cy, 2048) {
                    assert!(admitted, "source draw{i} xy{xy:?} camera{camera:?}");
                }
                checked += 1;
            }
        }
    }
    assert!(checked > 0, "no cooked draw projections were checked");
    eprintln!("validated {checked} source draw/camera projections");
}
