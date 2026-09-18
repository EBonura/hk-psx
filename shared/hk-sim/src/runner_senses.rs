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
    pub const RUNNER: Self = Self { body_offset_left: BODY_OFFSET_LEFT, body_extents: BODY_EXTENTS, alert_local: ALERT_LOCAL };
    /// From a facing-left body box and alert box relative to the actor origin.
    pub const fn from_boxes(body: [i32; 4], alert: [i32; 4]) -> Self {
        Self {
            body_offset_left: [(body[0] + body[2]) / 2, (body[1] + body[3]) / 2],
            body_extents: [(body[2] - body[0]) / 2, (body[3] - body[1]) / 2],
            alert_local: alert,
        }
    }
}
pub fn body_bounds(actor: [i32; 2], direction: i32) -> Result<[i32; 4], QueryError> {
    body_bounds_of(Shape::RUNNER, actor, direction)
}
pub fn body_bounds_of(shape: Shape, actor: [i32; 2], direction: i32) -> Result<[i32; 4], QueryError> {
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
pub fn alert_overlap_of(shape: Shape, actor: [i32; 2], hero_body: [i32; 4]) -> Result<bool, QueryError> {
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
    point(actor)?;
    point(hero)?;
    if !in_alert {
        return Ok(false);
    }
    if actor == hero {
        return Err(QueryError::ZeroLengthSight);
    }
    let mut blocked = false;
    for i in 0..count {
        let e = edge(i);
        if terrain(e)? && !blocked {
            blocked = segments_intersect(actor, hero, [e[0], e[1]], [e[2], e[3]]);
        }
    }
    Ok(!blocked)
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
        for (index, origin) in origins.iter_mut().enumerate() {
            let fringe = index as i32 - 1;
            *origin = [
                actor[0] + offset[0] + extents[0] * d[0] + extents[0] * d[1].abs() * fringe
                    - d[0] * skin,
                actor[1] + offset[1] + extents[1] * d[1] + extents[1] * d[0].abs() * fringe
                    - d[1] * skin,
            ];
            point(*origin)?;
            point([origin[0] + d[0] * reach, origin[1] + d[1] * reach])?;
        }
        Ok(Self {
            origins,
            direction,
            reach,
        })
    }
    pub fn origins(&self) -> [[i32; 2]; 3] {
        self.origins
    }
    /// Positive ray distance including skin; the endpoint itself is excluded.
    pub fn reach(&self) -> i32 {
        self.reach
    }
    fn hits_edge(&self, e: [i32; 4]) -> bool {
        self.reach > 0
            && self
                .origins
                .iter()
                .any(|&p| axis_hit(p, self.direction, self.reach, e))
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
    facing(direction)?;
    let offset = [-direction * shape.body_offset_left[0], shape.body_offset_left[1]];
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
    for i in 0..count {
        let e = edge(i);
        if terrain(e)? {
            if !result.wall {
                result.wall = wall.hits_edge(e);
            }
            if !result.floor_ahead {
                result.floor_ahead = floor.hits_edge(e);
            }
        }
    }
    Ok(result)
}
