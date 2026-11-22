//! The Knight's live attacks for one tick, as typed hit records.
//!
//! Every hero damage source is gathered once per tick into `HeroAttacks`, in
//! the order the receivers have always taken them: the spells first, then the
//! nail, then the Dream Nail. Each entry carries its world shape (transformed
//! once, here, not once per receiver), its damage, its direction and the class
//! that says how a receiver answers it. A new attack (Desolate Dive, Howling
//! Wraiths, a nail art) is one more entry of an existing class.
use crate::{AttackParams, Nail, Player};

/// How a receiver answers a hit. Receivers branch on this, never on `Source`,
/// so a new attack of an existing class needs no receiver change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// NailSlash: `HealthManager.Hit` with the evasion window, the Knight's
    /// recoil and pogo, SOUL on a landed hit, and every nail-only target
    /// (breakables, grass, switches, props, totems, rocks, the shade).
    Slash,
    /// A spell's `damages_enemy`: `HealthManager.Hit` with no recoil and no
    /// SOUL, and `Spell Destroy` on a hidden wall.
    Spell,
    /// `EnemyDreamnailReaction`: no damage, SOUL once per target.
    Dream,
}

/// What made the hit, for telemetry and source-specific effects only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Fireball,
    Nail,
    DreamNail,
}

/// Room for the nail's cooked polygons, the Dream Nail's and any later spell's.
pub const MAX_POINTS: usize = 16;

/// One live hero attack, in world space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeroHit {
    pub source: Source,
    pub class: Class,
    pub damage: u16,
    /// The slash kind (0 normal, 1 alternate, 2 up, 3 down) the effects,
    /// corpse flings and recoils are keyed by; 0 for anything horizontal.
    pub kind: u16,
    /// The side the attack faces: 1 right, -1 left. A wall slash's is the
    /// mirror of the Knight's facing.
    pub facing: i32,
    /// The world box around the shape (min x, min y, max x, max y).
    pub bounds: [i32; 4],
    /// Polygon points; `len` 0 means the shape is `bounds` itself.
    points: [[i32; 2]; MAX_POINTS],
    len: u8,
}

impl HeroHit {
    const EMPTY: Self = Self {
        source: Source::Nail,
        class: Class::Slash,
        damage: 0,
        kind: 0,
        facing: 1,
        bounds: [0; 4],
        points: [[0; 2]; MAX_POINTS],
        len: 0,
    };

    /// A Knight-local polygon placed at `at` facing `facing`. The original
    /// Knight collection faces left at positive scale X, so a local point's x
    /// is negated for a right-facing Knight. This is the one copy of the
    /// transform; every nail, wall slash and Dream Nail shape comes from it.
    pub fn polygon(
        source: Source,
        class: Class,
        local: &[[i32; 2]],
        at: [i32; 2],
        facing: i32,
    ) -> Self {
        assert!((3..=MAX_POINTS).contains(&local.len()));
        let mut hit = Self {
            source,
            class,
            facing,
            len: local.len() as u8,
            bounds: [i32::MAX, i32::MAX, i32::MIN, i32::MIN],
            ..Self::EMPTY
        };
        for (dst, p) in hit.points.iter_mut().zip(local) {
            *dst = [at[0] - p[0] * facing, at[1] + p[1]];
            hit.bounds[0] = hit.bounds[0].min(dst[0]);
            hit.bounds[1] = hit.bounds[1].min(dst[1]);
            hit.bounds[2] = hit.bounds[2].max(dst[0]);
            hit.bounds[3] = hit.bounds[3].max(dst[1]);
        }
        hit
    }

    /// The nail's swing while its hit window is open: the cooked polygon of
    /// its kind, on the side `Nail::facing` gives (mirrored for a wall slash).
    pub fn nail(
        nail: &Nail,
        p: AttackParams,
        polygons: [&[[i32; 2]]; 4],
        player: &Player,
        damage: u16,
    ) -> Option<Self> {
        if !nail.hitting(p) {
            return None;
        }
        let mut hit = Self::polygon(
            Source::Nail,
            Class::Slash,
            polygons[nail.kind as usize],
            [player.x, player.y],
            nail.facing(player),
        );
        hit.kind = nail.kind;
        hit.damage = damage;
        Some(hit)
    }

    /// The Dream Nail's slash, on the Knight's own facing.
    pub fn dream(local: &[[i32; 2]], player: &Player) -> Self {
        Self::polygon(
            Source::DreamNail,
            Class::Dream,
            local,
            [player.x, player.y],
            player.facing,
        )
    }

    /// A box-shaped spell (Vengeful Spirit's ball).
    pub fn spell_box(source: Source, bounds: [i32; 4], damage: u16, facing: i32) -> Self {
        Self {
            source,
            class: Class::Spell,
            damage,
            facing,
            bounds,
            ..Self::EMPTY
        }
    }

    /// The polygon, or an empty slice for a box-shaped hit.
    pub fn points(&self) -> &[[i32; 2]] {
        &self.points[..self.len as usize]
    }

    /// The one overlap test every receiver uses: the exact polygon against
    /// `target`, or the box against it for a box-shaped hit.
    pub fn hits(&self, target: [i32; 4]) -> bool {
        if self.len == 0 {
            overlap(self.bounds, target)
        } else {
            crate::polygon_hits_box(self.points(), target)
        }
    }

    /// `DirectionUtils.GetCardinalDirection` of `damages_enemy`'s direction:
    /// 0 right, 1 up, 2 left, 3 down.
    pub fn cardinal(&self) -> u8 {
        match self.kind {
            2 => 1,
            3 => 3,
            _ if self.facing > 0 => 0,
            _ => 2,
        }
    }

    /// The down slash, the only one that pogoes.
    pub fn pogo(&self) -> bool {
        self.class == Class::Slash && self.kind == 3
    }

    /// The polygon's vertex mean (each point divided first, as the boss
    /// effects have always placed it); the box centre for a box.
    pub fn centre(&self) -> [i32; 2] {
        let n = self.len as i32;
        if n == 0 {
            let b = self.bounds;
            return [b[0] / 2 + b[2] / 2, b[1] / 2 + b[3] / 2];
        }
        self.points()
            .iter()
            .fold([0, 0], |a, p| [a[0] + p[0] / n, a[1] + p[1] / n])
    }
}

fn overlap(a: [i32; 4], b: [i32; 4]) -> bool {
    a[0] <= b[2] && a[2] >= b[0] && a[1] <= b[3] && a[3] >= b[1]
}

/// Spells, the nail swing, the Dream Nail: at most one of each today.
pub const MAX_HITS: usize = 3;

/// This tick's live hero attacks, built once after the Knight has moved and
/// swung, and read by every receiver.
pub struct HeroAttacks {
    hits: [HeroHit; MAX_HITS],
    len: u8,
}

impl Default for HeroAttacks {
    fn default() -> Self {
        Self::new()
    }
}

impl HeroAttacks {
    pub const fn new() -> Self {
        Self {
            hits: [HeroHit::EMPTY; MAX_HITS],
            len: 0,
        }
    }

    /// Append one live attack. Receivers see entries in push order.
    pub fn push(&mut self, hit: HeroHit) {
        assert!((self.len as usize) < MAX_HITS);
        self.hits[self.len as usize] = hit;
        self.len += 1;
    }

    pub fn iter(&self) -> core::slice::Iter<'_, HeroHit> {
        self.hits[..self.len as usize].iter()
    }

    /// The live hits of one class, in order.
    pub fn of(&self, class: Class) -> impl Iterator<Item = &HeroHit> {
        self.iter().filter(move |h| h.class == class)
    }

    /// The live nail swing, if its hit window is open.
    pub fn slash(&self) -> Option<&HeroHit> {
        self.of(Class::Slash).next()
    }

    /// A hit the Knight takes resets the nail (`respond_to_hurt`), so the swing
    /// stops reaching anything for the rest of the tick. Spells already in
    /// flight and the Dream Nail are not affected.
    pub fn end_swing(&mut self) {
        let mut kept = 0;
        for i in 0..self.len as usize {
            if self.hits[i].class != Class::Slash {
                self.hits[kept] = self.hits[i];
                kept += 1;
            }
        }
        self.len = kept as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const P: AttackParams = AttackParams {
        duration: 21,
        cooldown: 25,
        alternate_reset: 30,
        hit_start: 0,
        hit_end: 6,
        ..AttackParams::ZERO
    };
    const TRI: [[i32; 2]; 3] = [[100, 0], [200, 50], [200, -50]];

    fn swing(wall: bool, facing: i32) -> (Nail, Player) {
        let mut n = Nail::new();
        let mut a = Player::spawn(1000, 2000);
        a.grounded = true;
        a.wall_sliding = wall;
        a.facing = facing;
        assert!(n.tick(P, true, 0, &mut a));
        (n, a)
    }

    #[test]
    fn the_transform_mirrors_local_x_and_bounds_the_shape() {
        let right = HeroHit::polygon(Source::Nail, Class::Slash, &TRI, [1000, 2000], 1);
        assert_eq!(right.points(), &[[900, 2000], [800, 2050], [800, 1950]]);
        assert_eq!(right.bounds, [800, 1950, 900, 2050]);
        let left = HeroHit::polygon(Source::Nail, Class::Slash, &TRI, [1000, 2000], -1);
        assert_eq!(left.points(), &[[1100, 2000], [1200, 2050], [1200, 1950]]);
        assert_eq!(left.bounds, [1100, 1950, 1200, 2050]);
    }

    #[test]
    fn a_nail_record_only_exists_in_the_hit_window() {
        let (mut n, mut a) = swing(false, 1);
        let polys = [&TRI[..]; 4];
        let hit = HeroHit::nail(&n, P, polys, &a, 5).expect("live");
        assert_eq!((hit.class, hit.damage, hit.kind, hit.facing), (Class::Slash, 5, 0, 1));
        for _ in 0..6 {
            n.tick(P, false, 0, &mut a);
        }
        assert!(HeroHit::nail(&n, P, polys, &a, 5).is_none());
    }

    #[test]
    fn a_wall_slash_record_is_mirrored_out_of_the_wall() {
        let polys = [&TRI[..]; 4];
        for facing in [1, -1] {
            let (n, a) = swing(true, facing);
            let wall = HeroHit::nail(&n, P, polys, &a, 5).unwrap();
            let plain = HeroHit::polygon(Source::Nail, Class::Slash, &TRI, [a.x, a.y], -facing);
            assert_eq!(wall.facing, -facing);
            assert_eq!(wall.points(), plain.points());
            assert_eq!(wall.cardinal(), if facing > 0 { 2 } else { 0 });
            // It reaches the far side only.
            let far = [a.x + 140 * facing - 5, a.y - 5, a.x + 140 * facing + 5, a.y + 5];
            let near = [a.x - 140 * facing - 5, a.y - 5, a.x - 140 * facing + 5, a.y + 5];
            assert!(wall.hits(far));
            assert!(!wall.hits(near));
        }
    }

    #[test]
    fn cardinals_pogo_and_box_hits() {
        let mut h = HeroHit::polygon(Source::Nail, Class::Slash, &TRI, [0, 0], 1);
        for (kind, facing, cardinal) in [(0, 1, 0), (1, -1, 2), (2, 1, 1), (3, -1, 3)] {
            h.kind = kind;
            h.facing = facing;
            assert_eq!(h.cardinal(), cardinal);
            assert_eq!(h.pogo(), kind == 3);
        }
        let ball = HeroHit::spell_box(Source::Fireball, [0, 0, 10, 10], 15, -1);
        assert!(ball.points().is_empty());
        assert!(ball.hits([10, 10, 20, 20]), "touching boxes overlap");
        assert!(!ball.hits([11, 0, 20, 10]));
        assert!(!ball.pogo());
        assert_eq!(ball.centre(), [5, 5]);
    }

    #[test]
    fn the_list_keeps_order_and_a_hurt_ends_only_the_swing() {
        let mut list = HeroAttacks::new();
        list.push(HeroHit::spell_box(Source::Fireball, [0; 4], 15, 1));
        list.push(HeroHit::polygon(Source::Nail, Class::Slash, &TRI, [0, 0], 1));
        list.push(HeroHit::polygon(Source::DreamNail, Class::Dream, &TRI, [0, 0], 1));
        let order: [Source; 3] = core::array::from_fn(|i| list.iter().nth(i).unwrap().source);
        assert_eq!(order, [Source::Fireball, Source::Nail, Source::DreamNail]);
        assert!(list.slash().is_some());
        list.end_swing();
        assert!(list.slash().is_none());
        assert_eq!(list.iter().count(), 2);
        assert_eq!(list.of(Class::Dream).count(), 1);
    }
}
