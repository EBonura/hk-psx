//! Necessary camera-independent conditions for the collector's existing gates.
//! Rejection only removes impossible occluders; ordinary scenery still draws.
pub fn possible(xy: &[i32; 8], minimum_core_area: i32) -> bool {
    // floor((a-camera)/256) and floor((b-camera)/256) cannot be equal
    // when |a-b| >=256. Both vertices use the same camera translation.
    for (a, b) in [(1, 3), (0, 4), (2, 6), (5, 7)] {
        if xy[a].abs_diff(xy[b]) >= 256 {
            return false;
        }
    }
    // Integer projection can increase a span by at most its fractional tail.
    // abs_diff plus quotient/remainder avoids overflow even at i32 extremes.
    let ceiling = |a: i32, b: i32| {
        let d = a.abs_diff(b);
        d / 256 + u32::from(d % 256 != 0)
    };
    let w = ceiling(xy[0], xy[2]).min(320);
    let h = ceiling(xy[1], xy[5]).min(240);
    // Match collect_occluders' bbox-area gate, preceding the core-area gate.
    i64::from(w * h) >= i64::from(minimum_core_area) * 2
}
