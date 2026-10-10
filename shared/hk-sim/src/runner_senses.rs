//! Level37 Zombie Runner sensing over borrowed, active Terrain edges (mask 256).
//!
//! Source: Windows Sweep constructor/Check and LineOfSightDetector.Update in
//! .hkpsx/crossroads68/runner-actions-Assembly-CSharp.dll.il / runner-source.il;
//! transformed BoxCollider values in RUNNER-CONTRACT.md. Both source actors
//! have unrotated unit-magnitude XY scale, mirrored X, edgeXAdjuster=0.
//!
//! Q16 coordinates are restricted to +/-512 world units, local collider values
//! and sweep reach to +/-16. Checked construction rejects unsupported bounds;
//! i64 products are bounded below 2^54. No heap, division, floating point or
//! terrain copy. Queries visit O(edge_count) edges, rejecting disabled zero-length
//! sentinels. Caller must filter inactive/broken/foreign-region/non-Terrain edges.
//!
//! This is an exact segment model of the quantized input, NOT Physics2D parity.
//! Box2D contact slop, trigger-enter timing, start-inside filled colliders,
//! queriesStartInColliders, one-sided edges and source float rounding are not
//! represented by an edge soup. Closed/collinear LOS conservatively blocks;
//! Sweep excludes its far endpoint but includes its origin. Zero-length LOS
//! returns an explicit error pending original query evidence. Rotated/scaled
//! body variants and non-box Hero shapes require a different query contract.
use crate::{combat::segments_intersect, ONE};

pub const WORLD_LIMIT: i32 = 512 * ONE;
pub const LOCAL_LIMIT: i32 = 16 * ONE;
/// round(source 0.1f * 65536); source quantization occurs only once.
pub const SKIN: i32 = 6554;
/// Child position + scale * (collider offset +/- half size), rounded to Q16.
/// Raw child size alone is not the effective detection box.
pub const ALERT_LOCAL: [i32; 4] = [-365036, -144507, 365036, 12730];
pub const BODY_EXTENTS: [i32; 2] = [39936, 53248];
pub const BODY_OFFSET_LEFT: [i32; 2] = [-11264, -40960];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryError {
    CoordinateLimit,
    InvalidBounds,
    LocalLimit,
    InvalidFacing,
    ZeroLengthSight,
}
fn point(p: [i32; 2]) -> Result<(), QueryError> {
    if p.iter()
        .any(|&v| !(-WORLD_LIMIT..=WORLD_LIMIT).contains(&v))
    {
        Err(QueryError::CoordinateLimit)
    } else {
        Ok(())
    }
}
fn bounds(b: [i32; 4]) -> Result<(), QueryError> {
    point([b[0], b[1]])?;
    point([b[2], b[3]])?;
    if b[0] > b[2] || b[1] > b[3] {
        Err(QueryError::InvalidBounds)
    } else {
        Ok(())
    }
}
fn terrain(e: [i32; 4]) -> Result<bool, QueryError> {
    point([e[0], e[1]])?;
    point([e[2], e[3]])?;
    Ok(e[0] != e[2] || e[1] != e[3])
}
fn facing(direction: i32) -> Result<(), QueryError> {
    if direction == -1 || direction == 1 {
        Ok(())
    } else {
        Err(QueryError::InvalidFacing)
    }
}
fn translated(local: [i32; 4], actor: [i32; 2]) -> Result<[i32; 4], QueryError> {
    point(actor)?;
    let result = [
        actor[0] + local[0],
        actor[1] + local[1],
        actor[0] + local[2],
        actor[1] + local[3],
    ];
    bounds(result)?;
    Ok(result)
}
/// Body collider and alert trigger of one Zombie Swipe variant, in the
/// facing-left placement frame. `RUNNER` is the two level37 instances; Barger
/// and Hornhead placements carry their own boxes through the actor spec.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    pub body_offset_left: [i32; 2],
    pub body_extents: [i32; 2],
    pub alert_local: [i32; 4],
}
impl Shape {
    pub const RUNNER: Self = Self {
        body_offset_left: BODY_OFFSET_LEFT,
        body_extents: BODY_EXTENTS,
        alert_local: ALERT_LOCAL,
    };
    /// From a facing-left body box and alert box relative to the actor origin.
    pub const fn from_boxes(body: [i32; 4], alert: [i32; 4]) -> Self {
        Self {
            body_offset_left: [(body[0] + body[2]) / 2, (body[1] + body[3]) / 2],
            body_extents: [(body[2] - body[0]) / 2, (body[3] - body[1]) / 2],
            alert_local: alert,
        }
    }
    /// From a placement's own body box, as the cooker reads it off the scene:
    /// the world box relative to the actor, so a mirrored placement (starting
    /// to face right, `initial_direction` 1) carries the mirror image of the
    /// facing-left box `from_boxes` wants. The alert box is mirror-stable
    /// (the cooker refuses one that is not), so it is taken as it is.
    pub const fn from_placement(body: [i32; 4], alert: [i32; 4], initial_direction: i32) -> Self {
        let left = if initial_direction > 0 {
            [-body[2], body[1], -body[0], body[3]]
        } else {
            body
        };
        Self::from_boxes(left, alert)
    }
}
pub fn body_bounds(actor: [i32; 2], direction: i32) -> Result<[i32; 4], QueryError> {
    body_bounds_of(Shape::RUNNER, actor, direction)
}
pub fn body_bounds_of(
    shape: Shape,
    actor: [i32; 2],
    direction: i32,
) -> Result<[i32; 4], QueryError> {
    facing(direction)?;
    let ox = -direction * shape.body_offset_left[0];
    translated(
        [
            ox - shape.body_extents[0],
            shape.body_offset_left[1] - shape.body_extents[1],
            ox + shape.body_extents[0],
            shape.body_offset_left[1] + shape.body_extents[1],
        ],
        actor,
    )
}
pub fn alert_bounds(actor: [i32; 2]) -> Result<[i32; 4], QueryError> {
    // Both X extents are equal; the parent's facing mirror leaves this box intact.
    translated(ALERT_LOCAL, actor)
}
/// Closed AABB overlap against the caller's actual Hero body, not Hero origin.
/// Touch counts in this segment model; original trigger/contact tolerances need
/// runtime evidence. A valid zero-area Hero box remains a point/line query.
pub fn alert_overlap(actor: [i32; 2], hero_body: [i32; 4]) -> Result<bool, QueryError> {
    alert_overlap_of(Shape::RUNNER, actor, hero_body)
}
pub fn alert_overlap_of(
    shape: Shape,
    actor: [i32; 2],
    hero_body: [i32; 4],
) -> Result<bool, QueryError> {
    let a = translated(shape.alert_local, actor)?;
    bounds(hero_body)?;
    Ok(
        a[0] <= hero_body[2]
            && a[2] >= hero_body[0]
            && a[1] <= hero_body[3]
            && a[3] >= hero_body[1],
    )
}
/// Source LOS first gates on its configured AlertRange, then casts from actor
/// transform to Hero transform without a facing cone. Edges are closed here.
pub fn line_of_sight(
    actor: [i32; 2],
    hero: [i32; 2],
    in_alert: bool,
    count: usize,
    edge: impl Fn(usize) -> [i32; 4],
) -> Result<bool, QueryError> {
    line_of_sight_near(actor, hero, in_alert, count, edge, all_edges)
}
/// `line_of_sight` visiting only the edges `near` returns for the sight
/// segment's box (see `EdgeMask`).
pub fn line_of_sight_near(
    actor: [i32; 2],
    hero: [i32; 2],
    in_alert: bool,
    count: usize,
    edge: impl Fn(usize) -> [i32; 4],
    near: fn([i32; 4]) -> EdgeMask,
) -> Result<bool, QueryError> {
    point(actor)?;
    point(hero)?;
    if !in_alert {
        return Ok(false);
    }
    if actor == hero {
        return Err(QueryError::ZeroLengthSight);
    }
    let mask = near([
        actor[0].min(hero[0]),
        actor[1].min(hero[1]),
        actor[0].max(hero[0]),
        actor[1].max(hero[1]),
    ]);
    let mut blocked = false;
    each_edge(count, mask, |i| {
        let e = edge(i);
        if terrain(e)? && !blocked {
            blocked = segments_intersect(actor, hero, [e[0], e[1]], [e[2], e[3]]);
        }
        Ok(())
    })?;
    Ok(!blocked)
}
/// The terrain edges a query may need to visit: bit `i % 32` of word `i / 32`
/// stands for edge `i`, for the first 128 edges; edges from 128 on are always
/// visited. A `near` function given a query box must set the bit of every edge
/// whose own closed box meets that box, and of every edge `terrain` rejects.
/// Every query here can only hit an edge whose box meets its own (the hit point
/// is on both), so it gives the same answer as visiting all of them, in the
/// same index order.
pub type EdgeMask = [u32; 4];
/// Visit every edge.
pub const ALL_EDGES: EdgeMask = [u32::MAX; 4];
/// A `near` that selects every edge.
pub fn all_edges(_: [i32; 4]) -> EdgeMask {
    ALL_EDGES
}
/// A room's first 128 edges bucketed into 16 vertical columns, for `near`.
/// Build it whenever the edge list changes; any edge set works, so it is exact
/// for the list it was built from. Edges `terrain` rejects sit in every column,
/// zero edges (`[0; 4]`, removed terrain) in none.
#[derive(Clone, Copy)]
pub struct EdgeColumns {
    left: i32,
    shift: u32,
    columns: [EdgeMask; EdgeColumns::COLUMNS],
}
impl EdgeColumns {
    pub const COLUMNS: usize = 16;
    /// Selects every edge until built.
    pub const ALL: Self = Self {
        left: 0,
        shift: 31,
        columns: [ALL_EDGES; Self::COLUMNS],
    };
    #[inline(never)]
    pub fn build(edges: &[[i32; 4]]) -> Self {
        let edges = &edges[..edges.len().min(128)];
        let live = |e: &[i32; 4]| !crate::empty_edge(e) && terrain(*e).is_ok();
        let left = edges
            .iter()
            .filter(|e| live(e))
            .map(|e| e[0].min(e[2]))
            .min()
            .unwrap_or(0);
        let right = edges
            .iter()
            .filter(|e| live(e))
            .map(|e| e[0].max(e[2]))
            .max()
            .unwrap_or(0);
        // Columns a power of two wide, so a lookup is a shift and a clamp.
        let mut shift = 0;
        while shift < 31 && ((right as i64 - left as i64) >> shift) >= Self::COLUMNS as i64 {
            shift += 1;
        }
        let mut index = Self {
            left,
            shift,
            columns: [[0; 4]; Self::COLUMNS],
        };
        for (i, e) in edges.iter().enumerate() {
            if crate::empty_edge(e) {
                continue;
            }
            let (first, last) = if terrain(*e).is_ok() {
                (index.column(e[0].min(e[2])), index.column(e[0].max(e[2])))
            } else {
                (0, Self::COLUMNS - 1)
            };
            for column in &mut index.columns[first..=last] {
                column[i / 32] |= 1 << (i % 32);
            }
        }
        index
    }
    #[inline]
    fn column(&self, x: i32) -> usize {
        ((x as i64 - self.left as i64) >> self.shift).clamp(0, Self::COLUMNS as i64 - 1) as usize
    }
    /// Every edge whose x range meets `bounds[0]..=bounds[2]` (a superset of
    /// those whose box meets `bounds`).
    #[inline]
    pub fn near(&self, bounds: [i32; 4]) -> EdgeMask {
        let mut mask = [0; 4];
        for column in &self.columns[self.column(bounds[0])..=self.column(bounds[2])] {
            for w in 0..4 {
                mask[w] |= column[w];
            }
        }
        mask
    }
}
/// Call `f` for each edge below `count` that `mask` selects, in index order.
#[inline(always)]
pub fn each_edge(
    count: usize,
    mask: EdgeMask,
    mut f: impl FnMut(usize) -> Result<(), QueryError>,
) -> Result<(), QueryError> {
    for (word, &bits) in mask.iter().enumerate() {
        let mut bits = bits;
        while bits != 0 {
            let i = word * 32 + bits.trailing_zeros() as usize;
            if i >= count {
                return Ok(());
            }
            f(i)?;
            bits &= bits - 1;
        }
    }
    for i in 128..count {
        f(i)?;
    }
    Ok(())
}
/// The edges `each_edge` visits, as an iterator: one copy of a loop body
/// instead of two (the masked words and the tail past 128), where code size
/// matters more than the tail's speed.
pub fn selected(count: usize, mask: EdgeMask) -> Selected {
    Selected {
        mask,
        next: 0,
        count,
    }
}
pub struct Selected {
    mask: EdgeMask,
    next: usize,
    count: usize,
}
impl Iterator for Selected {
    type Item = usize;
    #[inline]
    fn next(&mut self) -> Option<usize> {
        let mut i = self.next;
        while i < self.count.min(128) {
            let bits = self.mask[i / 32] >> (i % 32);
            if bits != 0 {
                i += bits.trailing_zeros() as usize;
                break;
            }
            i = (i | 31) + 1;
        }
        if i >= self.count {
            return None;
        }
        self.next = i + 1;
        Some(i)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Down,
    Up,
}
impl Direction {
    fn vector(self) -> [i32; 2] {
        match self {
            Self::Left => [-1, 0],
            Self::Right => [1, 0],
            Self::Down => [0, -1],
            Self::Up => [0, 1],
        }
    }
    fn axis_sign(self) -> (usize, i64) {
        match self {
            Self::Left => (0, -1),
            Self::Right => (0, 1),
            Self::Down => (1, -1),
            Self::Up => (1, 1),
        }
    }
}
/// Three parallel rays from the source collider fringe. Origins are public for
/// source/guest probe comparison; mutation is prevented by private storage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sweep {
    origins: [[i32; 2]; 3],
    direction: Direction,
    reach: i32,
    /// Closed box around all three rays, origins to far ends. A hit is a
    /// point on the edge and on a ray, so an edge whose own box misses this
    /// one cannot be hit: `hits_edge` rejects it with four i32 compares
    /// before the exact i64 test. Crossroads_37's thirteen Walker actors
    /// sweep every edge of their views twice a tick, and the exact test on
    /// every edge was over half of that room's CPU time.
    bounds: [i32; 4],
}
impl Sweep {
    /// Offset already includes localScale; extents are positive world AABB
    /// extents. Non-positive requested distance is the source's no-hit case.
    pub fn new(
        actor: [i32; 2],
        offset: [i32; 2],
        extents: [i32; 2],
        direction: Direction,
        requested: i32,
        skin: i32,
    ) -> Result<Self, QueryError> {
        point(actor)?;
        if offset
            .iter()
            .any(|&v| !(-LOCAL_LIMIT..=LOCAL_LIMIT).contains(&v))
            || extents.iter().any(|&v| !(0..=LOCAL_LIMIT).contains(&v))
            || !(-LOCAL_LIMIT..=LOCAL_LIMIT).contains(&requested)
            || !(0..=LOCAL_LIMIT).contains(&skin)
        {
            return Err(QueryError::LocalLimit);
        }
        let d = direction.vector();
        let reach = if requested > 0 { requested + skin } else { 0 };
        let mut origins = [[0; 2]; 3];
        let mut bounds = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
        for (index, origin) in origins.iter_mut().enumerate() {
            let fringe = index as i32 - 1;
            *origin = [
                actor[0] + offset[0] + extents[0] * d[0] + extents[0] * d[1].abs() * fringe
                    - d[0] * skin,
                actor[1] + offset[1] + extents[1] * d[1] + extents[1] * d[0].abs() * fringe
                    - d[1] * skin,
            ];
            point(*origin)?;
            let end = [origin[0] + d[0] * reach, origin[1] + d[1] * reach];
            point(end)?;
            for p in [*origin, end] {
                bounds = [
                    bounds[0].min(p[0]),
                    bounds[1].min(p[1]),
                    bounds[2].max(p[0]),
                    bounds[3].max(p[1]),
                ];
            }
        }
        Ok(Self {
            origins,
            direction,
            reach,
            bounds,
        })
    }
    pub fn origins(&self) -> [[i32; 2]; 3] {
        self.origins
    }
    /// Positive ray distance including skin; the endpoint itself is excluded.
    pub fn reach(&self) -> i32 {
        self.reach
    }
    #[inline(always)]
    fn hits_edge(&self, e: [i32; 4]) -> bool {
        let b = self.bounds;
        self.reach > 0
            && e[0].max(e[2]) >= b[0]
            && e[0].min(e[2]) <= b[2]
            && e[1].max(e[3]) >= b[1]
            && e[1].min(e[3]) <= b[3]
            && self.hits_edge_exact(e)
    }
    #[inline(never)]
    fn hits_edge_exact(&self, e: [i32; 4]) -> bool {
        self.origins
            .iter()
            .any(|&p| axis_hit(p, self.direction, self.reach, e))
    }
    /// The exact test alone, without the box rejection, for tests that prove
    /// the rejection never changes an answer.
    #[doc(hidden)]
    pub fn hits_edge_unfiltered(&self, e: [i32; 4]) -> bool {
        self.reach > 0 && self.hits_edge_exact(e)
    }
    #[doc(hidden)]
    pub fn hits_edge_filtered(&self, e: [i32; 4]) -> bool {
        self.hits_edge(e)
    }
    pub fn hits(&self, count: usize, edge: impl Fn(usize) -> [i32; 4]) -> Result<bool, QueryError> {
        let mut hit = false;
        for i in 0..count {
            let e = edge(i);
            if terrain(e)? && !hit {
                hit = self.hits_edge(e);
            }
        }
        Ok(hit)
    }
}
// Distance along a cardinal ray as a rational numerator/positive denominator.
// All input points have been checked inside +/-512. Differences <=2^26,
// numerator <=2^53, reach*denominator <=2^47. Avoid rounded intersections:
// a sloped hit less than one Q16 unit before the far end must still count.
fn axis_hit(p: [i32; 2], direction: Direction, reach: i32, e: [i32; 4]) -> bool {
    let (axis, sign) = direction.axis_sign();
    let other = 1 - axis;
    let a = [e[0], e[1]];
    let b = [e[2], e[3]];
    if p[other] < a[other].min(b[other]) || p[other] > a[other].max(b[other]) {
        return false;
    }
    let mut denominator = b[other] as i64 - a[other] as i64;
    if denominator == 0 {
        let x = (a[axis] as i64 - p[axis] as i64) * sign;
        let y = (b[axis] as i64 - p[axis] as i64) * sign;
        return x.max(y) >= 0 && x.min(y) < reach as i64;
    }
    let mut numerator = (a[axis] as i64 - p[axis] as i64) * denominator
        + (p[other] as i64 - a[other] as i64) * (b[axis] as i64 - a[axis] as i64);
    numerator *= sign;
    if denominator < 0 {
        denominator = -denominator;
        numerator = -numerator;
    }
    numerator >= 0 && numerator < reach as i64 * denominator
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WalkerQueries {
    pub wall: bool,
    pub floor_ahead: bool,
}
/// Source wall and hole Sweeps for the two validated Runner variants. Hole
/// offset is (body.extents.x + .5 + edgeXAdjuster=0) * facing, distance .25.
/// Returns floor-present (not missing-floor), matching runner::Senses.
pub fn walker_queries(
    actor: [i32; 2],
    direction: i32,
    count: usize,
    edge: impl Fn(usize) -> [i32; 4],
) -> Result<WalkerQueries, QueryError> {
    walker_queries_of(Shape::RUNNER, actor, direction, count, edge)
}
pub fn walker_queries_of(
    shape: Shape,
    actor: [i32; 2],
    direction: i32,
    count: usize,
    edge: impl Fn(usize) -> [i32; 4],
) -> Result<WalkerQueries, QueryError> {
    walker_queries_near(shape, actor, direction, count, edge, all_edges)
}
/// `walker_queries_of` visiting only the edges `near` returns for the box
/// around both sweeps (see `EdgeMask`).
pub fn walker_queries_near(
    shape: Shape,
    actor: [i32; 2],
    direction: i32,
    count: usize,
    edge: impl Fn(usize) -> [i32; 4],
    near: fn([i32; 4]) -> EdgeMask,
) -> Result<WalkerQueries, QueryError> {
    facing(direction)?;
    let offset = [
        -direction * shape.body_offset_left[0],
        shape.body_offset_left[1],
    ];
    let forward = shape.body_extents[0] + ONE / 2;
    let wall = Sweep::new(
        actor,
        offset,
        shape.body_extents,
        if direction < 0 {
            Direction::Left
        } else {
            Direction::Right
        },
        forward,
        SKIN,
    )?;
    point(actor)?;
    let floor = Sweep::new(
        [actor[0] + direction * forward, actor[1]],
        offset,
        shape.body_extents,
        Direction::Down,
        ONE / 4,
        SKIN,
    )?;
    let mut result = WalkerQueries {
        wall: false,
        floor_ahead: false,
    };
    let (a, b) = (wall.bounds, floor.bounds);
    let mask = near([
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]);
    each_edge(count, mask, |i| {
        let e = edge(i);
        if terrain(e)? {
            if !result.wall {
                result.wall = wall.hits_edge(e);
            }
            if !result.floor_ahead {
                result.floor_ahead = floor.hits_edge(e);
            }
        }
        Ok(())
    })?;
    Ok(result)
}
