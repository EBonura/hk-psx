//! Bounded HealthManager subset, separate from unresolved enemy AI.
pub const MAX_ACTORS: usize = 32;
#[derive(Clone, Copy, Debug)]
pub struct WalkParams {
    pub speed: i32,
    pub turn_ticks: u16,
    pub turn_cooldown_ticks: u16,
}
#[derive(Clone, Copy, Debug)]
pub enum ActorController {
    Crawler,
    Runner {
        idle_clip: u16,
        anticipate_clip: u16,
        lunge_clip: u16,
        cooldown_clip: u16,
        /// Walker speeds/waits and the FSM lunge speed of this placement.
        params: crate::runner::Params,
        /// Alert trigger box relative to the actor origin (facing-left frame).
        alert: [i32; 4],
    },
    /// Surface-following Tiktik driven by `crate::climber::Climber`.
    Climber {
        stun_clip: u16,
        start_right: bool,
    },
    /// Gravity-free Buzzer driven by `crate::vengefly::Vengefly`; `walk_clip`
    /// is Idle and `turn_clip` TurnToIdle.
    Vengefly {
        startle_clip: u16,
        chase_clip: u16,
        turn_fly_clip: u16,
    },
    /// Gravity-free bouncing Fly driven by `crate::gruzzer::Gruzzer`; the
    /// single Fly clip is both `walk_clip` and `turn_clip`.
    Gruzzer,
    /// Rolling Baldur driven by `crate::baldur::Baldur`; `walk_clip` is Idle.
    Baldur {
        start_clip: u16,
        roll_clip: u16,
        stop_clip: u16,
    },
    /// Aspid Hunter driven by `crate::aspid::Aspid`; `walk_clip` is Fly and
    /// `turn_clip` TurnToFly. The shot clips belong to the projectile pool.
    Aspid {
        fire_clip: u16,
        shot_clip: u16,
        impact_clip: u16,
        /// FSM `startAlert`: skips Idle on the first frame.
        start_alert: bool,
    },
}
#[derive(Clone, Copy, Debug)]
pub struct ActorSpec {
    pub source_id: u32,
    pub controller: ActorController,
    pub x: i32,
    pub y: i32,
    /// Spawn-orientation collider bounds relative to the actor transform.
    pub bounds: [i32; 4],
    pub health: EnemyParams,
    pub walk: WalkParams,
    pub initial_direction: i32,
    pub random_start_direction: bool,
    pub walk_clip: u16,
    pub turn_clip: u16,
    pub corpse: Option<crate::CorpseSpec>,
    pub recoil_speed: i32,
    pub recoil_ticks: u16,
}
#[derive(Clone, Copy, Debug)]
pub struct EnemyParams {
    pub health: i16,
    pub contact_damage: u16,
    pub evasion_ticks: u16,
    pub invincible: bool,
    pub damage_override: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Ignored,
    Blocked,
    Damaged,
    Killed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActorHealth {
    pub hp: i16,
    pub evasion_ticks: u16,
    pub dead: bool,
}
impl ActorHealth {
    pub const fn new(p: EnemyParams) -> Self {
        Self {
            hp: p.health,
            evasion_ticks: 0,
            dead: p.health <= 0,
        }
    }
    pub fn tick(&mut self) {
        self.evasion_ticks = self.evasion_ticks.saturating_sub(1);
    }
    pub fn contact_damage(&self, p: EnemyParams) -> u16 {
        if self.dead {
            0
        } else {
            p.contact_damage
        }
    }
    pub fn hit(&mut self, p: EnemyParams, damage: u16) -> Hit {
        if self.dead || damage == 0 || self.evasion_ticks != 0 {
            return Hit::Ignored;
        }
        if p.invincible {
            return Hit::Blocked;
        }
        let dealt = if p.damage_override { 1 } else { damage as i32 };
        // HealthManager.SubtractHealth clamps overkill to -50, not wraparound.
        self.hp = (self.hp as i32 - dealt).max(-50) as i16;
        if self.hp <= 0 {
            self.dead = true;
            Hit::Killed
        } else {
            self.evasion_ticks = p.evasion_ticks;
            Hit::Damaged
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WalkState {
    pub direction: i32,
    pub animation_tick: u32,
    pub turn_remaining: u16,
    pub cooldown_remaining: u16,
    initial_turn: bool,
}
impl WalkState {
    /// Random initial reversal is supplied by the caller's deterministic RNG.
    pub const fn new(direction: i32, initial_turn: bool) -> Self {
        Self {
            direction,
            animation_tick: 0,
            turn_remaining: 0,
            cooldown_remaining: 0,
            initial_turn,
        }
    }
    /// Original WalkLeftRight turns only on a grounded wall/ledge probe and
    /// waits for the source turn clip before changing the transform's facing.
    pub fn tick(&mut self, p: WalkParams, grounded: bool, wall: bool, ledge: bool) -> i32 {
        assert!(self.direction == -1 || self.direction == 1);
        self.cooldown_remaining = self.cooldown_remaining.saturating_sub(1);
        if self.turn_remaining != 0 {
            self.turn_remaining -= 1;
            self.animation_tick = self.animation_tick.wrapping_add(1);
            if self.turn_remaining == 0 {
                self.direction = -self.direction;
                self.animation_tick = 0;
            }
            return 0;
        }
        if self.initial_turn || (grounded && (wall || ledge) && self.cooldown_remaining == 0) {
            self.initial_turn = false;
            self.cooldown_remaining = p.turn_cooldown_ticks;
            self.turn_remaining = p.turn_ticks;
            self.animation_tick = 0;
            if p.turn_ticks == 0 {
                self.direction = -self.direction;
            }
            return 0;
        }
        self.animation_tick = self.animation_tick.wrapping_add(1);
        self.direction * p.speed
    }
}

/// Resolve only an actor's initial overlap with resident Terrain segments.
///
/// HKROOM02 stores two-sided edges, not polygon interiors or one-way normals.
/// Choose the smallest axis translation that separates the box from an actual
/// crossing segment, matching the walking solver's axis-separated representation.
/// This is not Unity/Box2D solver parity and must not run on ongoing movement.
/// Coordinates and box extents share the guest's validated +/-512-unit bounds.
/// Returns false without changing the body if eight contact passes cannot find
/// a non-overlapping placement in a dense chain of intersecting contacts.
pub fn resolve_actor_spawn(
    body: &mut crate::Player,
    p: crate::Params,
    count: usize,
    edge: impl Fn(usize) -> [i32; 4],
) -> bool {
    let mut candidate = *body;
    for pass in 0..=8 {
        let bounds = [
            candidate.x - p.half_width,
            candidate.y + p.bottom,
            candidate.x + p.half_width,
            candidate.y + p.top,
        ];
        let mut correction: Option<[i32; 2]> = None;
        for i in 0..count {
            if let Some(delta) = spawn_separation(bounds, edge(i)) {
                if correction
                    .is_none_or(|old| delta[0].abs() + delta[1].abs() < old[0].abs() + old[1].abs())
                {
                    correction = Some(delta);
                }
            }
        }
        let Some([dx, dy]) = correction else {
            *body = candidate;
            return true;
        };
        if pass == 8 {
            return false;
        }
        candidate.x += dx;
        candidate.y += dy;
    }
    false
}

fn spawn_separation(b: [i32; 4], e: [i32; 4]) -> Option<[i32; 2]> {
    let [left, bottom, right, top] = b;
    let [x0, y0, x1, y1] = e;
    if (x0 == x1 && y0 == y1)
        || x0.max(x1) <= left
        || x0.min(x1) >= right
        || y0.max(y1) <= bottom
        || y0.min(y1) >= top
    {
        return None;
    }
    // Clip the segment to each box slab. Rounding outwards by at most one Q16
    // unit ensures a sloped edge cannot remain inside after integer projection.
    fn range(a0: i32, b0: i32, a1: i32, b1: i32, lo: i32, hi: i32) -> (i32, i32) {
        if a0 == a1 {
            return (b0.min(b1), b0.max(b1));
        }
        let (a0, b0, a1, b1) = if a0 > a1 {
            (a1, b1, a0, b0)
        } else {
            (a0, b0, a1, b1)
        };
        let den = (a1 - a0) as i64;
        let at = |a: i32| (a - a0) as i64 * (b1 - b0) as i64;
        let n0 = at(lo.max(a0));
        let n1 = at(hi.min(a1));
        (
            b0 + n0.min(n1).div_euclid(den) as i32,
            b0 + (-(-n0.max(n1)).div_euclid(den)) as i32,
        )
    }
    let (low_y, high_y) = range(x0, y0, x1, y1, left, right);
    if high_y <= bottom || low_y >= top {
        return None;
    }
    let (low_x, high_x) = range(y0, x0, y1, x1, bottom, top);
    if high_x <= left || low_x >= right {
        return None;
    }
    let options = [
        [0, high_y - bottom],
        [0, low_y - top],
        [high_x - left, 0],
        [low_x - right, 0],
    ];
    options.into_iter().min_by_key(|d| d[0].abs() + d[1].abs())
}

/// Exact segment test for a horizontal/vertical closed ray. Bounds overlap
/// already proves the edge crosses the ray's supporting axis; only the edge's
/// two orientation signs remain. Q16.16 world coordinates are within ±512, so
/// differences fit i32 and signed32×32 products fit i64.
fn axis_ray_hits(ray:[i32;4],edge:[i32;4])->bool {
    let [left,bottom,right,top]=ray;
    let [x0,y0,x1,y1]=edge;
    debug_assert!(left==right || bottom==top);
    if x0.max(x1)<left || x0.min(x1)>right || y0.max(y1)<bottom || y0.min(y1)>top {return false;}
    let dx=x1-x0;let dy=y1-y0;
    let a=dx as i64*(bottom-y0)as i64-dy as i64*(left-x0)as i64;
    let b=dx as i64*(top-y0)as i64-dy as i64*(right-x0)as i64;
    a==0 || b==0 || (a<0)!=(b<0)
}

/// Source probes against the cooked Terrain edges, including sloped segments.
/// Returns grounded, wall ahead, and missing floor ahead. Q16.16 bounds must
/// remain inside ±512 world units, matching the cooked scene limits.
pub fn walker_senses(
    bounds: [i32; 4],
    direction: i32,
    count: usize,
    edge: impl Fn(usize) -> [i32; 4],
) -> (bool, bool, bool) {
    let [left, bottom, right, _] = bounds;
    let x = left + (right - left) / 2;
    let y = bottom + crate::ONE / 2;
    let reach = (right - left) / 2 + crate::ONE / 10;
    let front = x + direction * reach;
    let ground_ray = [x, y-crate::ONE, x, y];
    let wall_ray = [x.min(front), y, x.max(front), y];
    let floor_ray = [front, y-crate::ONE, front, y];
    let mut found = [false; 3];
    for i in 0..count {
        let [x0, y0, x1, y1] = edge(i);
        if x0 == x1 && y0 == y1 {
            continue;
        } // Disabled/degenerate terrain sentinel.
        for (index, ray) in [ground_ray, wall_ray, floor_ray].iter().enumerate() {
            // Each sense is existential: a later edge cannot undo a hit.
            if !found[index] {
                found[index] = axis_ray_hits(*ray,[x0,y0,x1,y1]);
            }
        }
    }
    (found[0], found[1], !found[2])
}

#[cfg(test)]
mod tests {
    use super::*;
    const P: EnemyParams = EnemyParams {
        health: 8,
        contact_damage: 1,
        evasion_ticks: 12,
        invincible: false,
        damage_override: false,
    };
    fn body_params() -> crate::Params {
        crate::Params {
            speed: crate::ONE,
            jump: 0,
            gravity: 60 * crate::ONE,
            fall: 100 * crate::ONE,
            hold_ticks: 0,
            min_ticks: 0,
            half_width: crate::ONE / 2,
            bottom: -crate::ONE,
            top: crate::ONE,
        }
    }
    #[test]
    fn spawn_crossing_floor_and_ceiling_uses_nearest_separation() {
        let p = body_params();
        let floor = [-4 * crate::ONE, 0, 4 * crate::ONE, 0];
        let mut body = crate::Player::spawn(0, crate::ONE - crate::ONE / 8);
        assert!(resolve_actor_spawn(&mut body, p, 1, |_| floor));
        assert_eq!(body.y, crate::ONE);
        body.step(p, 0, false, 1, |_| floor);
        assert!(body.grounded);
        let mut body = crate::Player::spawn(0, -crate::ONE + crate::ONE / 8);
        assert!(resolve_actor_spawn(&mut body, p, 1, |_| floor));
        assert_eq!(body.y, -crate::ONE);
        assert!(!body.grounded);
    }
    #[test]
    fn spawn_clear_touching_disabled_and_segment_endpoint_are_unchanged() {
        let p = body_params();
        let edges = [[-4 * crate::ONE, 0, 4 * crate::ONE, 0], [0; 4]];
        for y in [crate::ONE, 2 * crate::ONE, -2 * crate::ONE] {
            let mut body = crate::Player::spawn(0, y);
            let before = body;
            assert!(resolve_actor_spawn(&mut body, p, 2, |i| edges[i]));
            assert_eq!(body, before);
        }
        let mut body = crate::Player::spawn(0, 0);
        let before = body;
        assert!(resolve_actor_spawn(&mut body, p, 1, |_| [
            crate::ONE / 2,
            0,
            crate::ONE,
            0
        ]));
        assert_eq!(body, before);
    }
    #[test]
    fn spawn_sloped_edge_and_corner_resolve_without_winding_assumptions() {
        let p = body_params();
        let edge = [-4 * crate::ONE, -crate::ONE, 4 * crate::ONE, crate::ONE];
        let mut forward = crate::Player::spawn(0, crate::ONE);
        let mut reverse = forward;
        assert!(resolve_actor_spawn(&mut forward, p, 1, |_| edge));
        assert!(resolve_actor_spawn(&mut reverse, p, 1, |_| [
            edge[2], edge[3], edge[0], edge[1]
        ]));
        assert_eq!(forward, reverse);
        assert_eq!(forward.y, crate::ONE + crate::ONE / 8);
        for _ in 0..30 {
            forward.step(p, 1, false, 1, |_| edge);
        }
        assert!(forward.grounded);
        let edges = [
            [-4 * crate::ONE, 0, 4 * crate::ONE, 0],
            [0, -4 * crate::ONE, 0, 4 * crate::ONE],
        ];
        let mut corner = crate::Player::spawn(crate::ONE / 4, crate::ONE - crate::ONE / 8);
        assert!(resolve_actor_spawn(&mut corner, p, 2, |i| edges[i]));
        assert_eq!((corner.x, corner.y), (crate::ONE / 2, crate::ONE));
    }
    #[test]
    fn contact_chain_exceeding_budget_is_bounded_and_transactional() {
        let p = body_params();
        let edges: [[i32; 4]; 12] = core::array::from_fn(|i| {
            [
                -4 * crate::ONE,
                i as i32 * crate::ONE,
                4 * crate::ONE,
                i as i32 * crate::ONE,
            ]
        });
        let mut body = crate::Player::spawn(0, crate::ONE / 2);
        let before = body;
        assert!(!resolve_actor_spawn(&mut body, p, edges.len(), |i| edges[i]));
        assert_eq!(body, before);
    }
    #[test]
    fn actual_third_crawler_spawn_stays_on_authored_four_unit_platform() {
        // level6:12548 Rigidbody/BoxCollider transform; Terrain level6:8366.
        let p = crate::Params {
            half_width: (48128 + 45056) / 2,
            bottom: -69632,
            top: -10240,
            ..body_params()
        };
        let edge = [
            157 * crate::ONE,
            4 * crate::ONE,
            140 * crate::ONE,
            4 * crate::ONE,
        ];
        let offset = (-45056 + 48128) / 2;
        let mut body = crate::Player::spawn(10041311 + offset, 326102);
        assert_eq!(4 * crate::ONE - (body.y + p.bottom), 5674);
        assert!(resolve_actor_spawn(&mut body, p, 1, |_| edge));
        assert_eq!(body.y, 331776);
        for _ in 0..120 {
            body.step(p, -1, false, 1, |_| edge);
        }
        assert_eq!(body.y, 331776);
        assert!(body.grounded);
        // First crawler is already above its ten-unit floor, unchanged at spawn.
        let mut first = crate::Player::spawn(8225424 + offset, 727249);
        let before = first;
        assert!(resolve_actor_spawn(&mut first, p, 1, |_| [
            120 * crate::ONE,
            10 * crate::ONE,
            130 * crate::ONE,
            10 * crate::ONE
        ]));
        assert_eq!(first, before);
    }
    #[test]
    fn two_base_nail_hits_kill_and_disable_contact_once() {
        let mut actor = ActorHealth::new(P);
        assert_eq!(actor.hit(P, 5), Hit::Damaged);
        for _ in 0..11 {
            actor.tick();
            assert_eq!(actor.hit(P, 5), Hit::Ignored);
        }
        actor.tick();
        assert_eq!(actor.hit(P, 5), Hit::Killed);
        assert_eq!(actor.hp, -2);
        assert_eq!(actor.contact_damage(P), 0);
        assert_eq!(actor.hit(P, 5), Hit::Ignored);
    }
    #[test]
    fn invincible_override_and_overkill_do_not_overflow() {
        let mut actor = ActorHealth::new(P);
        assert_eq!(
            actor.hit(
                EnemyParams {
                    invincible: true,
                    ..P
                },
                5
            ),
            Hit::Blocked
        );
        assert_eq!(actor.hp, 8);
        assert_eq!(
            actor.hit(
                EnemyParams {
                    damage_override: true,
                    ..P
                },
                u16::MAX
            ),
            Hit::Damaged
        );
        assert_eq!(actor.hp, 7);
        for _ in 0..12 {
            actor.tick();
        }
        assert_eq!(actor.hit(P, u16::MAX), Hit::Killed);
        assert_eq!(actor.hp, -50);
    }
    #[test]
    fn authored_probe_turns_at_ledge_and_observes_clip_and_cooldown() {
        let p = WalkParams {
            speed: 4 * crate::ONE,
            turn_ticks: 6,
            turn_cooldown_ticks: 60,
        };
        let bounds = [-crate::ONE / 2, 0, crate::ONE / 2, crate::ONE];
        let floor = [-4 * crate::ONE, 0, crate::ONE / 2, 0];
        let senses = walker_senses(bounds, 1, 1, |_| floor);
        assert_eq!(senses, (true, false, true));
        let mut walk = WalkState::new(1, false);
        assert_eq!(walk.tick(p, senses.0, senses.1, senses.2), 0);
        for _ in 0..5 {
            assert_eq!(walk.tick(p, true, false, true), 0);
            assert_eq!(walk.direction, 1);
        }
        assert_eq!(walk.tick(p, true, false, true), 0);
        assert_eq!(walk.direction, -1);
        assert_eq!(walk.tick(p, true, true, true), -p.speed); // cooldown guards another turn
    }
    #[test]
    fn axis_rays_match_general_exact_intersections() {
        let check=|a:[i32;2],b:[i32;2],c:[i32;2],d:[i32;2]| {
            let ray=[a[0].min(b[0]),a[1].min(b[1]),a[0].max(b[0]),a[1].max(b[1])];
            assert_eq!(axis_ray_hits(ray,[c[0],c[1],d[0],d[1]]),
                crate::combat::segments_intersect(a,b,c,d),"{a:?} {b:?} {c:?} {d:?}");
        };
        let points:[[i32;2];25]=core::array::from_fn(|i|[(i%5)as i32-2,(i/5)as i32-2]);
        for &a in &points {for &b in &points {
            if a[0]!=b[0] && a[1]!=b[1] {continue;}
            for &c in &points {for &d in &points {check(a,b,c,d);}}
        }}
        let mut seed=18u32;
        let mut next=||{seed=seed.wrapping_mul(1664525).wrapping_add(1013904223);(seed%(1024*crate::ONE)as u32)as i32-512*crate::ONE};
        for i in 0..100000 {
            let a=[next(),next()];let mut b=[next(),next()];b[i&1]=a[i&1];
            check(a,b,[next(),next()],[next(),next()]);
        }
    }
    #[test]
    fn repeated_probe_hits_preserve_all_senses_and_edge_checkpoints() {
        let q=crate::ONE;
        let bounds=[-q/2,0,q/2,q];
        let edges=[[0;4],[-4*q,0,4*q,0],[q/2,-q,q/2,q],
            [-q,-q,q,q],[-q,q,q,-q],[-q/2,-q,-q/2,q],
            [10*q,10*q,11*q,11*q]];
        for direction in [-1,1] {
            for shift in 0..edges.len() {
                let mut expected=(false,false,true);
                for e in edges {
                    let one=walker_senses(bounds,direction,1,|_|e);
                    expected.0|=one.0;expected.1|=one.1;expected.2&=one.2;
                }
                let calls=core::cell::Cell::new(0);
                let actual=walker_senses(bounds,direction,edges.len()*3,|i| {
                    calls.set(calls.get()+1);edges[(i+shift)%edges.len()]
                });
                assert_eq!(actual,expected);
                assert_eq!(calls.get(),edges.len()*3);
            }
        }
    }
    #[test]
    fn disabled_zero_edge_does_not_create_ground_or_wall_at_origin() {
        assert_eq!(
            walker_senses(
                [
                    -crate::ONE / 2,
                    -crate::ONE / 2,
                    crate::ONE / 2,
                    crate::ONE / 2
                ],
                1,
                1,
                |_| [0; 4]
            ),
            (false, false, true)
        );
    }
    #[test]
    fn airborne_ledge_does_not_trigger_turn_but_seeded_start_can() {
        let p = WalkParams {
            speed: crate::ONE,
            turn_ticks: 2,
            turn_cooldown_ticks: 60,
        };
        let mut walk = WalkState::new(-1, false);
        assert_eq!(walk.tick(p, false, true, true), -p.speed);
        let mut seeded = WalkState::new(-1, true);
        assert_eq!(seeded.tick(p, false, false, false), 0);
    }
}
