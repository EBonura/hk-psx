//! No-charm nail attack and source GrassCut renderer-switch subset.
//! All durations are 60 Hz ticks. Polygon coordinates are Knight-local Q16.16.
use crate::Player;

#[derive(Clone, Copy)]
pub struct AttackParams {
    pub duration: u16,
    pub cooldown: u16,
    pub alternate_reset: u16,
    pub hit_start: u16,
    pub hit_end: u16,
    /// `ATTACK_QUEUE_STEPS`: a press this long before the swing becomes legal
    /// still lands, the same buffer the jump and the dash get.
    pub queue_ticks: u16,
    /// `ATTACK_RECOVERY_TIME`: while the swing is younger than this, CanDash
    /// refuses a dash out of it.
    pub recovery_ticks: u16,
}
#[derive(Clone, Copy)]
pub struct Grass {
    pub bounds: [i32; 4],
    pub off_draw: usize,
    pub on_draw: usize,
}
impl AttackParams {
    /// Every field zero, so a caller that only needs a swing's shape (the
    /// enemy nails, the test fixtures) does not restate the hero's buffers.
    pub const ZERO: Self = Self {
        duration: 0, cooldown: 0, alternate_reset: 0, hit_start: 0, hit_end: 0,
        queue_ticks: 0, recovery_ticks: 0,
    };
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Nail {
    pub active: bool,
    pub age: u16,
    /// Normal, alternate, up, down: matches the cooked polygon/effect order.
    pub kind: u16,
    cooldown: u16,
    since_attack: u16,
    alternate: bool,
    was_button: bool,
    queuing: bool,
    queue: u16,
}
impl Default for Nail {
    fn default() -> Self {
        Self::new()
    }
}
impl Nail {
    pub const fn new() -> Self {
        Self {
            active: false,
            age: 0,
            kind: 0,
            cooldown: 0,
            since_attack: u16::MAX,
            alternate: false,
            was_button: false,
            queuing: false,
            queue: 0,
        }
    }
    /// Call after movement once per simulation tick. Returns true on a new swing.
    /// Source animation controller sees the *toggled* altAttack state: the first
    /// horizontal effect is SlashEffect while the body plays SlashAlt.
    pub fn tick(
        &mut self,
        p: AttackParams,
        button: bool,
        vertical: i32,
        player: &mut Player,
    ) -> bool {
        self.cooldown = self.cooldown.saturating_sub(1);
        self.since_attack = self.since_attack.saturating_add(1);
        if self.active {
            self.age = self.age.saturating_add(1);
            if self.age >= p.duration {
                self.active = false;
            }
        }
        if button && !self.was_button {
            self.queuing = true;
            self.queue = 0;
        }
        self.was_button = button;
        // CanAttack refuses a swing while dashing; the source keeps the press
        // queued across that, so it fires the moment the dash releases.
        let legal = !self.active && self.cooldown == 0 && player.dash_left == 0
            && !matches!(player.super_dash, crate::SuperDash::Travelling(_));
        let start = self.queuing && button && legal;
        if self.queuing {
            if start || !button || self.queue >= p.queue_ticks {
                self.queuing = false;
            } else {
                self.queue += 1;
            }
        }
        if start {
            if self.since_attack > p.alternate_reset {
                self.alternate = false;
            }
            self.kind = if vertical > 0 {
                2
            } else if vertical < 0 && !player.grounded {
                3
            } else {
                let k = u16::from(self.alternate);
                self.alternate = !self.alternate;
                k
            };
            self.age = 0;
            self.active = true;
            self.cooldown = p.cooldown;
            self.since_attack = 0;
        }
        player.attack_recovering = self.active && self.age < p.recovery_ticks;
        if self.active {
            player.animation = [6, 5, 7, 8][self.kind as usize];
            player.animation_tick = self.age as u32;
        }
        start
    }
    pub fn effect_clip(&self) -> usize {
        9 + self.kind as usize
    }
    pub fn hitting(&self, p: AttackParams) -> bool {
        self.active && self.age >= p.hit_start && self.age < p.hit_end
    }
    /// Exact polygon-versus-box overlap for the bounded, source-extracted grass
    /// triggers. Each source component is destroyed after cutting; the bitset
    /// models that one-shot state. Rendering consumes off_draw/on_draw metadata.
    pub fn cut_grass(
        &self,
        p: AttackParams,
        polygons: [&[[i32; 2]]; 4],
        grass: &[Grass],
        player: &Player,
        cut: &mut u32,
    ) -> u32 {
        if !self.hitting(p) {
            return 0;
        }
        let points = polygons[self.kind as usize];
        if !(3..=16).contains(&points.len()) {
            return 0;
        }
        let mut polygon = [[0; 2]; 16];
        for (dst, src) in polygon.iter_mut().zip(points) {
            // The original Knight collection faces left at positive scale X.
            *dst = [player.x - src[0] * player.facing, player.y + src[1]];
        }
        let mut bounds = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
        for point in &polygon[..points.len()] {
            bounds[0] = bounds[0].min(point[0]);
            bounds[1] = bounds[1].min(point[1]);
            bounds[2] = bounds[2].max(point[0]);
            bounds[3] = bounds[3].max(point[1]);
        }
        let mut changed = 0;
        for (i, g) in grass.iter().take(32).enumerate() {
            let bit = 1_u32 << i;
            // Conservative broad phase avoids 64-bit cross products for distant
            // source triggers. Exact polygon tests still decide every overlap.
            if *cut & bit == 0
                && bounds[0] <= g.bounds[2]
                && bounds[2] >= g.bounds[0]
                && bounds[1] <= g.bounds[3]
                && bounds[3] >= g.bounds[1]
                && polygon_hits_box(&polygon[..points.len()], g.bounds)
            {
                changed |= bit;
            }
        }
        *cut |= changed;
        changed
    }
}
// Cooked local polygons fit ±16 units; room bounds fit ±512. With Q16.16,
// the cross products fit i64 (worst-case absolute sum is below 2^54).
fn cross(a: [i32; 2], b: [i32; 2], p: [i32; 2]) -> i64 {
    (b[0] as i64 - a[0] as i64) * (p[1] as i64 - a[1] as i64)
        - (b[1] as i64 - a[1] as i64) * (p[0] as i64 - a[0] as i64)
}
pub(crate) fn segments_intersect(a: [i32; 2], b: [i32; 2], c: [i32; 2], d: [i32; 2]) -> bool {
    let (ax0, ax1, ay0, ay1) = (
        a[0].min(b[0]),
        a[0].max(b[0]),
        a[1].min(b[1]),
        a[1].max(b[1]),
    );
    let (cx0, cx1, cy0, cy1) = (
        c[0].min(d[0]),
        c[0].max(d[0]),
        c[1].min(d[1]),
        c[1].max(d[1]),
    );
    // Strict disjointness only: touching boxes still require the exact test.
    // Distant segments avoid all four 64-bit cross products on the R3000.
    if ax1 < cx0 || cx1 < ax0 || ay1 < cy0 || cy1 < ay0 {
        return false;
    }
    let (ac, ad, ca, cb) = (
        cross(a, b, c),
        cross(a, b, d),
        cross(c, d, a),
        cross(c, d, b),
    );
    ((ac > 0 && ad < 0 || ac < 0 && ad > 0) && (ca > 0 && cb < 0 || ca < 0 && cb > 0))
        || (ac == 0 && (ax0..=ax1).contains(&c[0]) && (ay0..=ay1).contains(&c[1]))
        || (ad == 0 && (ax0..=ax1).contains(&d[0]) && (ay0..=ay1).contains(&d[1]))
        || (ca == 0 && (cx0..=cx1).contains(&a[0]) && (cy0..=cy1).contains(&a[1]))
        || (cb == 0 && (cx0..=cx1).contains(&b[0]) && (cy0..=cy1).contains(&b[1]))
}
#[cfg(test)]
#[path = "combat_intersection_tests.rs"]
mod intersection_tests;
pub fn polygon_hits_box(poly: &[[i32; 2]], bounds: [i32; 4]) -> bool {
    if !(3..=16).contains(&poly.len()) {
        return false;
    }
    let [x0, y0, x1, y1] = bounds;
    let mut extent = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
    for p in poly {
        if (x0..=x1).contains(&p[0]) && (y0..=y1).contains(&p[1]) {
            return true;
        }
        extent[0] = extent[0].min(p[0]);
        extent[1] = extent[1].min(p[1]);
        extent[2] = extent[2].max(p[0]);
        extent[3] = extent[3].max(p[1]);
    }
    // Most trigger polygons are distant from the player. Reuse the vertex
    // scan to reject them once, before four segment tests per polygon edge.
    // Strict separation preserves all touching cases. Reversed legacy bounds
    // retain the old exact path rather than changing their interpretation.
    if x0 <= x1 && y0 <= y1
        && (extent[2] < x0 || extent[0] > x1 || extent[3] < y0 || extent[1] > y1)
    {
        return false;
    }
    let corners = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
    let mut inside = false;
    let q = corners[0];
    for i in 0..poly.len() {
        let a = poly[i];
        let b = poly[(i + 1) % poly.len()];
        for j in 0..4 {
            if segments_intersect(a, b, corners[j], corners[(j + 1) % 4]) {
                return true;
            }
        }
        if (a[1] > q[1]) != (b[1] > q[1]) {
            let side = cross(a, b, q);
            if (b[1] > a[1] && side > 0) || (b[1] < a[1] && side < 0) {
                inside = !inside;
            }
        }
    }
    inside
}
#[cfg(test)]
mod tests {
    use super::*;
    const P: AttackParams = AttackParams {
        duration: 21,
        cooldown: 25,
        alternate_reset: 30,
        hit_start: 2,
        hit_end: 6, ..AttackParams::ZERO };
    #[test]
    fn press_edges_cooldown_alternate_and_reset() {
        let mut n = Nail::new();
        let mut a = Player::spawn(0, 0);
        a.grounded = true;
        assert!(n.tick(P, true, 0, &mut a));
        assert_eq!((n.kind, a.animation), (0, 6));
        for _ in 0..50 {
            assert!(!n.tick(P, true, 0, &mut a));
        }
        n.tick(P, false, 0, &mut a);
        assert!(n.tick(P, true, 0, &mut a));
        assert_eq!(n.kind, 0);
        for _ in 0..24 {
            n.tick(P, false, 0, &mut a);
        }
        assert!(n.tick(P, true, 0, &mut a));
        assert_eq!((n.kind, a.animation), (1, 5));
    }
    #[test]
    fn grounded_down_becomes_horizontal_air_down_and_up_are_distinct() {
        for (ground, vertical, expected) in [(true, -1, 0), (false, -1, 3), (true, 1, 2)] {
            let mut n = Nail::new();
            let mut a = Player::spawn(0, 0);
            a.grounded = ground;
            n.tick(P, true, vertical, &mut a);
            assert_eq!(n.kind, expected);
        }
    }
    #[test]
    fn active_window_exact_polygon_and_one_shot_cut() {
        let tri = [[-200, 0], [-100, 100], [-100, -100]];
        let ps = [&tri[..]; 4];
        let grass = [
            Grass {
                bounds: [145, -5, 155, 5],
                off_draw: 0,
                on_draw: 1,
            },
            Grass {
                bounds: [195, 80, 205, 90],
                off_draw: 2,
                on_draw: 3,
            },
        ];
        let mut n = Nail::new();
        let mut a = Player::spawn(0, 0);
        let mut cut = 0;
        n.tick(P, true, 0, &mut a);
        assert_eq!(n.cut_grass(P, ps, &grass, &a, &mut cut), 0);
        for _ in 0..2 {
            n.tick(P, false, 0, &mut a);
        }
        assert_eq!(n.cut_grass(P, ps, &grass, &a, &mut cut), 1);
        assert_eq!(n.cut_grass(P, ps, &grass, &a, &mut cut), 0);
        a.facing = -1;
        cut = 0;
        assert_eq!(n.cut_grass(P, ps, &grass, &a, &mut cut), 0);
        for _ in 0..4 {
            n.tick(P, false, 0, &mut a);
        }
        a.facing = 1;
        assert_eq!(n.cut_grass(P, ps, &grass, &a, &mut cut), 0);
    }
    #[test]
    fn polygon_containment_crossing_and_disjoint() {
        let square = [[0, 0], [100, 0], [100, 100], [0, 100]];
        assert!(polygon_hits_box(&square, [10, 10, 20, 20]));
        assert!(polygon_hits_box(&square, [-10, 40, 110, 50]));
        assert!(!polygon_hits_box(&square, [110, 110, 120, 120]));
    }
}

#[cfg(test)]
#[path = "combat_polygon_bounds_tests.rs"]
mod polygon_bounds_tests;
