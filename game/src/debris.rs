//! Original rigid Breakable fragments and the source ObjectBounce lifecycle.
//! A fragment is one object of the scene's metadata bank (KIND_FRAGMENT) decoded
//! where it is used; the 28-slot pool holds whichever are live, whatever their scene.
//! Uniform polygon inertia and implicit angular damping approximate native Box2D.
//! Contact impulses approximate native Box2D; source bounce cache/threshold and
//! sleep tolerances remain explicit and no visual lifetime is invented.
use hk_format::world_meta::{Object, KIND_FRAGMENT};
use hk_format::{i32_at, u32_at, Room};
use hk_sim::ONE;
use psx_math::{Wide, WideU};
pub const CAPACITY: usize = 28;
#[no_mangle]
pub static mut HK_DEBRIS_ACTIVE: u32 = 0;
#[no_mangle]
pub static mut HK_DEBRIS_DRAWN: u32 = 0;
#[no_mangle]
pub static mut HK_DEBRIS_BOUNCES: u32 = 0;
#[no_mangle]
pub static mut HK_DEBRIS_ASLEEP: u32 = 0;
/// Pieces a full pool made room for by retiring its longest-stopped piece.
#[no_mangle]
pub static mut HK_DEBRIS_EVICTED: u32 = 0;
/// Pieces a full pool of moving pieces could not take.
#[no_mangle]
pub static mut HK_DEBRIS_DROPPED: u32 = 0;
/// Finishing pieces that shrank away (asleep eight seconds) or sat off screen ten draws.
#[no_mangle]
pub static mut HK_DEBRIS_FINISHED: u32 = 0;
fn publish_active(value: usize) {
    #[cfg(target_arch = "mips")]
    unsafe {
        core::ptr::write_volatile(core::ptr::addr_of_mut!(HK_DEBRIS_ACTIVE), value as u32);
    }
    #[cfg(not(target_arch = "mips"))]
    let _ = value;
}
fn publish_drawn(value: u32) {
    #[cfg(target_arch = "mips")]
    unsafe {
        core::ptr::write_volatile(core::ptr::addr_of_mut!(HK_DEBRIS_DRAWN), value);
    }
    #[cfg(not(target_arch = "mips"))]
    let _ = value;
}
/// Points a fragment's polygon may carry (host/effects.py FRAGMENT_MAX_POINTS).
pub const MAX_POINTS: usize = 8;
/// FinishingRigidBody, measured on every part that has one: asleep this long it
/// shrinks for the next, then is deactivated (60 Hz ticks).
pub const FINISH_WAIT: u16 = 8 * 60;
pub const FINISH_SHRINK: u16 = 60;
/// Consecutive draws off screen after which a finishing piece is deactivated.
pub const FINISH_OFF_SCREEN: u8 = 10;
/// Q16 degrees per source torque step at one inverse moment, per half of the
/// spin coefficient: (1/2) / 50 * (180 / pi).
const SPIN_UNIT: i32 = 37549;
/// SimpleRock's constant tint, the source (0.489, 0.51, 0.559) in the GPU's 128 = 1.0.
const TINT: (u8, u8, u8) = (63, 65, 72);
#[derive(Clone, Copy, Debug)]
pub struct Spec {
    /// The piece's object index in the scene bank's first region; its identity in the pool.
    pub index: u16,
    /// Its owner Breakable's state index in the scene.
    pub owner: u8,
    pub source: u32,
    pub origin: [i32; 2],
    pub centroid: [i32; 2],
    pub scale: [i32; 2],
    /// The collider's points in the piece's own units, `len` of them.
    pub points: [[i32; 2]; MAX_POINTS],
    pub len: u8,
    /// Q16 degrees/sec increment per unit/sec of vx, for one source torque step. Signed.
    pub torque: i32,
    /// The inverse moment of inertia (Q16) of the polygon at its scale, mass one.
    pub inv_i: i32,
    /// The authored start rotation (Q16 degrees); unused when `random_start`.
    pub start_pose: i32,
    pub random_start: bool,
    pub finishing: bool,
    pub tint: bool,
    pub angle_offset: i16,
}
impl Spec {
    pub fn polygon(&self) -> &[[i32; 2]] {
        &self.points[..self.len as usize]
    }
    /// This spec with `polygon` as its collider (3..=MAX_POINTS points).
    pub const fn with_polygon(mut self, polygon: &[[i32; 2]]) -> Self {
        assert!(polygon.len() >= 3 && polygon.len() <= MAX_POINTS);
        let mut i = 0;
        while i < polygon.len() {
            self.points[i] = polygon[i];
            i += 1;
        }
        self.len = polygon.len() as u8;
        self
    }
    /// The source's constant fling angle offset, mirrored by the nail's side.
    pub const ANGLE_OFFSET: i16 = -60;
}
/// The piece a KIND_FRAGMENT object of the scene bank describes, `index` being that
/// object's index in the bank's first region. None for anything else or a malformed polygon.
pub fn decode(object: &Object<'_>, index: u16) -> Option<Spec> {
    if object.kind() != KIND_FRAGMENT {
        return None;
    }
    let (state, flags, slots) = (object.state_id(), object.flags(), object.bounds());
    let mut points = [[0; 2]; MAX_POINTS];
    let mut len = 0;
    for p in object.polygons().next()?.ok()?.points() {
        *points.get_mut(len)? = p.ok()?;
        len += 1;
    }
    if len < 3 {
        return None;
    }
    // Six signed bits: halves of the AddTorque(vx * coefficient) coefficient.
    let spin_half = ((i32::from(flags) & 0x3F) << 26) >> 26;
    let inv_i = object.extra(2);
    Some(Spec {
        index,
        owner: (state & 0x7F) as u8,
        source: object.source_id(),
        origin: [slots[0], slots[1]],
        centroid: [slots[2], slots[3]],
        scale: [object.extra(0), object.extra(1)],
        points,
        len: len as u8,
        torque: mulq(inv_i, spin_half * SPIN_UNIT),
        inv_i,
        // 1/64 degree to Q16 degrees.
        start_pose: ((state >> 8) as i32) << 10,
        random_start: flags & 64 != 0,
        finishing: flags & 128 != 0,
        tint: flags & 256 != 0,
        angle_offset: Spec::ANGLE_OFFSET,
    })
}
#[derive(Clone, Copy, Debug)]
pub struct Body {
    pub center: [i32; 2],
    pub velocity: [i32; 2],
    pub angle: i32,
    pub omega: i32,
    pub stopped: bool,
    steps: u8,
    cache_ticks: u8,
    cache_last: [i32; 2],
    cache_direction: [i32; 2],
    cache_speed: i32,
    contact: bool,
    contact_edge: [i32; 4],
    sleep_ticks: u8,
    rng: u32,
    pub bounces: u16,
}
fn random(seed: &mut u32) -> u32 {
    *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
    *seed
}
#[inline(always)]
fn same_edge(a: [i32; 4], b: [i32; 4]) -> bool {
    // Array equality lowers to an out-of-line memcmp on our MIPS target.
    ((a[0] ^ b[0]) | (a[1] ^ b[1]) | (a[2] ^ b[2]) | (a[3] ^ b[3])) == 0
}
pub fn launch_range(kind: u8, facing: i32) -> (i32, i32, i32) {
    match kind {
        2 => (70, 110, 3), // numerator over 2: source up multiplier 1.5
        3 => (160, 380, 2),
        _ if facing < 0 => (120, 160, 2),
        _ => (30, 70, 2),
    }
}
impl Body {
    pub fn launch(spec: Spec, kind: u8, facing: i32) -> Self {
        let mut seed = spec.source ^ ((kind as u32) << 24) ^ (facing as u32);
        let (lo, hi, multiplier) = launch_range(kind, facing);
        let angle = (lo + (random(&mut seed) % ((hi - lo) as u32 + 1)) as i32) * ONE;
        let speed = (10 * ONE + (random(&mut seed) % (7 * ONE + 1) as u32) as i32) * multiplier / 2;
        // Source SpinSelf.Start integer angle; activation/angleOffset ordering is
        // not observed in original runtime. Choose Start then Break offset.
        let offset = if kind >= 2 {
            0
        } else {
            spec.angle_offset as i32 * facing
        };
        let random_turn = (random(&mut seed) % 360) as i32;
        // SpinSelf and SimpleRock overwrite the pose in their Start; the others keep the
        // authored one, turned by the break's offset.
        let rotation = if spec.random_start {
            (random_turn + offset).rem_euclid(360) * ONE
        } else {
            (spec.start_pose + offset * ONE).rem_euclid(360 * ONE)
        };
        let com = scaled(rotate(spec.centroid, rotation), spec.scale);
        Self {
            center: [spec.origin[0] + com[0], spec.origin[1] + com[1]],
            velocity: [
                mul_trig(speed, sin(angle + 90 * ONE)),
                mul_trig(speed, sin(angle)),
            ],
            angle: rotation,
            omega: 0,
            stopped: false,
            steps: 0,
            cache_ticks: 0,
            cache_last: [0; 2],
            cache_direction: [0; 2],
            cache_speed: 0,
            contact: false,
            contact_edge: [0; 4],
            sleep_ticks: 0,
            rng: seed,
            bounces: 0,
        }
    }
    pub fn point(&self, local: [i32; 2], centroid: [i32; 2], scale: [i32; 2]) -> [i32; 2] {
        self.point_rotated(local, centroid, scale, Rotation::new(self.angle))
    }
    fn point_rotated(
        &self,
        local: [i32; 2],
        centroid: [i32; 2],
        scale: [i32; 2],
        rotation: Rotation,
    ) -> [i32; 2] {
        let p = rotation.apply([local[0] - centroid[0], local[1] - centroid[1]]);
        let p = scaled(p, scale);
        [self.center[0] + p[0], self.center[1] + p[1]]
    }
    /// One approximately 20ms source physics callback (not one 60Hz game tick).
    pub fn step(&mut self, spec: Spec, count: usize, mut edge: impl FnMut(usize) -> [i32; 4]) {
        if self.stopped {
            return;
        }
        // Retain a stable resting manifold through fixed-point roundoff. Its
        // support span must contain the center of mass, so an unbalanced single
        // corner cannot be frozen just because an impact briefly stopped it.
        if self.steps >= 2
            && self.sleep_ticks > 0
            && self.contact
            && length(self.velocity) <= 655
            && self.omega.abs() <= 2 * ONE
            && supports(*self, spec, self.contact_edge)
            && (0..count).any(|i| same_edge(edge(i), self.contact_edge))
        {
            self.velocity = [0; 2];
            self.omega = 0;
            self.sleep_ticks += 1;
            if self.sleep_ticks >= 25 {
                self.stopped = true;
            }
            return;
        }
        // ObjectBounce.FixedUpdate samples displacement and body speed on
        // every fourth source callback, before the native physics integration.
        if self.cache_ticks == 3 {
            let origin = self.point([0; 2], spec.centroid, spec.scale);
            self.cache_direction = [
                origin[0] - self.cache_last[0],
                origin[1] - self.cache_last[1],
            ];
            self.cache_last = origin;
            self.cache_speed = length(self.velocity);
            self.cache_ticks = 0;
        } else {
            self.cache_ticks += 1;
        }
        let old = *self;
        if self.steps == 1 {
            self.omega = Wide::mul(self.velocity[0], spec.torque)
                .sar(16)
                .clamp(Wide::of(-360 * ONE), Wide::of(360 * ONE))
                .low();
        }
        self.steps = (self.steps + 1).min(2);
        self.omega = psx_math::int32::mul_div_i32(self.omega, 1000, 1001);
        let energy_before = kinetic(self.velocity, self.omega, spec);
        self.velocity[1] = (self.velocity[1] - 60 * ONE / 50).max(-100 * ONE);
        self.center[0] += self.velocity[0] / 50;
        self.center[1] += self.velocity[1] / 50;
        let rotation = self.omega / 50;
        self.angle = (self.angle + rotation).rem_euclid(360 * ONE);
        let mut fraction = ONE;
        let mut hit = None;
        let mut contact_present = false;
                let mut before = [[0; 2]; 16];
        let mut after = [[0; 2]; 16];
        let mut swept = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
        let old_rotation = Rotation::new(old.angle);
        let next_rotation = Rotation::new(self.angle);
        for (index, &p) in spec.polygon().iter().enumerate() {
            before[index] = old.point_rotated(p, spec.centroid, spec.scale, old_rotation);
            after[index] = self.point_rotated(p, spec.centroid, spec.scale, next_rotation);
            for q in [before[index], after[index]] {
                swept[0] = swept[0].min(q[0]);
                swept[1] = swept[1].min(q[1]);
                swept[2] = swept[2].max(q[0]);
                swept[3] = swept[3].max(q[1]);
            }
        }
        // Decode/filter each resident terrain edge once per piece, not once
        // per vertex. Most edges are rejected before any cross products.
        for i in 0..count {
            let e = edge(i);
            contact_present |= old.contact && same_edge(e, old.contact_edge);
            if (e[0] | e[1] | e[2] | e[3]) == 0
                || e[0].min(e[2]) > swept[2]
                || e[0].max(e[2]) < swept[0]
                || e[1].min(e[3]) > swept[3]
                || e[1].max(e[3]) < swept[1]
            {
                continue;
            }
            for j in 0..spec.polygon().len() {
                if let Some(t) = crossing(before[j], after[j], [e[0], e[1]], [e[2], e[3]]) {
                    if t < fraction {
                        fraction = t;
                        hit = Some((e, before[j], after[j]));
                    }
                }
            }
        }
        // Unity's original .01 contact offset keeps a resting manifold alive
        // across tiny positive gaps. Crossing-only contacts spuriously sleep/wake
        // and omit normal/friction impulses between those crossings.
        if hit.is_none() && contact_present {
            let e = old.contact_edge;
            let mut n = unit([e[1] - e[3], e[2] - e[0]]);
            if dot([old.center[0] - e[0], old.center[1] - e[1]], n) < 0 {
                n = [-n[0], -n[1]];
            }
            let mut nearest = None;
            for j in 0..spec.polygon().len() {
                let q = after[j];
                let d = dot([q[0] - e[0], q[1] - e[1]], n);
                if d.abs() <= 655
                    && q[0] >= e[0].min(e[2]) - 655
                    && q[0] <= e[0].max(e[2]) + 655
                    && q[1] >= e[1].min(e[3]) - 655
                    && q[1] <= e[1].max(e[3]) + 655
                    && nearest.is_none_or(|(_, best): (usize, i32)| d < best)
                {
                    nearest = Some((j, d));
                }
            }
            if let Some((j, _)) = nearest {
                fraction = 0;
                hit = Some((e, before[j], after[j]));
            }
        }
        if let Some((edge, a, b)) = hit {
            for i in 0..2 {
                self.center[i] = old.center[i]
                    + psx_math::int32::mul_shr_i32(self.center[i] - old.center[i], fraction, 16);
            }
            self.angle = (old.angle + psx_math::int32::mul_shr_i32(rotation, fraction, 16))
                .rem_euclid(360 * ONE);
            let mut normal = unit([edge[1] - edge[3], edge[2] - edge[0]]);
            if dot([old.center[0] - edge[0], old.center[1] - edge[1]], normal) < 0 {
                normal = [-normal[0], -normal[1]];
            }
            let point = core::array::from_fn(|i| a[i] + mulq(b[i] - a[i], fraction));
            // Native source has8 velocity iterations. Build the bounded
            // contacted-plane manifold from every source vertex within the
            // original .01 contact offset; a single corner impulse cannot settle
            // a flat piece resting on two corners.
            // Velocity impulses do not change center/angle. Reuse the expired
            // sweep array for the unchanged contact points, preserving vertex
            // order and all eight sequential velocity iterations exactly.
            let contact_rotation = Rotation::new(self.angle);
            let mut contacts = 0;
            for &v in spec.polygon() {
                let q = self.point_rotated(v, spec.centroid, spec.scale, contact_rotation);
                if dot([q[0] - edge[0], q[1] - edge[1]], normal).abs() <= 655 {
                    after[contacts] = q;
                    contacts += 1;
                }
            }
            for _ in 0..8 {
                for &q in &after[..contacts] {
                    self.contact_impulse(spec, q, normal);
                }
                if contacts == 0 {
                    self.contact_impulse(spec, point, normal);
                }
            }
            // Exact source event gate and cached-direction reflection. Native
            // contact impulse happens first, then ObjectBounce overrides v.
            if !old.contact && self.cache_speed > ONE {
                let d = unit(self.cache_direction);
                let into = dot(d, normal);
                let reflected = unit([
                    d[0] - 2 * mulq(into, normal[0]),
                    d[1] - 2 * mulq(into, normal[1]),
                ]);
                // Source bounceFactor .5 times Random.Range(.8,1.2).
                let factor = 26214 + (random(&mut self.rng) % 13109) as i32;
                let speed = mulq(self.cache_speed, factor);
                self.velocity = [mulq(reflected[0], speed), mulq(reflected[1], speed)];
                self.bounces = self.bounces.saturating_add(1);
            }
            // Complete the remaining timestep with the resolved velocity. This
            // permits sliding and rebound rather than pinning every impact.
            let remaining = ONE - fraction;
            for i in 0..2 {
                self.center[i] += mulq(self.velocity[i] / 50, remaining);
            }
            self.angle = (self.angle + mulq(self.omega / 50, remaining)).rem_euclid(360 * ONE);
            // Nonpenetration along the contacted source plane. Exact Box2D
            // manifold/decomposition and multi-contact iterations are unported.
            let mut penetration = 0;
            let resolved_rotation = Rotation::new(self.angle);
            for &v in spec.polygon() {
                let p = self.point_rotated(v, spec.centroid, spec.scale, resolved_rotation);
                penetration = penetration.min(dot([p[0] - edge[0], p[1] - edge[1]], normal));
            }
            for i in 0..2 {
                self.center[i] -= mulq(penetration, normal[i]);
            }
            // Static friction can hold a low-speed supported contact against
            // gravity. Use source mu=.2 and angular sleep tolerance2deg/sec;
            // no arbitrary visual stop timer replaces the original .5s sleep.
            let tangent = [-normal[1], normal[0]];
            let along = dot(self.velocity, tangent);
            let support = dot([0, -60 * ONE / 50], normal).abs() / 5;
            let friction = along.clamp(-support, support);
            for i in 0..2 {
                self.velocity[i] -= mulq(friction, tangent[i]);
            }
            if along.abs() <= support && self.omega.abs() <= 2 * ONE {
                self.omega = 0;
            }
            // Do not let discrete nonpenetration correction inject mechanical
            // energy. Native Box2D's position solver is absent; enforce this
            // invariant in our bounded approximation. The source scripted
            // bounce is an intentional velocity override and remains exempt.
            if self.bounces == old.bounces {
                let budget = energy_before
                    .sub_w(Wide::mul(2 * 60 * ONE, self.center[1] - old.center[1]))
                    .max(Wide::ZERO);
                let energy = kinetic(self.velocity, self.omega, spec);
                if energy > budget {
                    // budget < energy, so the quotient is at most ONE, and budget >= 0.
                    let ratio = budget
                        .cast_unsigned()
                        .quotient(energy.plus(65535).sar(16).cast_unsigned())
                        .min(ONE as u32);
                    let factor = WideU::mul(ratio, ONE as u32).isqrt() as i32;
                    for v in &mut self.velocity {
                        *v = mulq(*v, factor);
                    }
                    self.omega = mulq(self.omega, factor);
                }
            }
            self.contact = true;
            self.contact_edge = edge;
            if length(self.velocity) <= 655 && self.omega.abs() <= 2 * ONE {
                self.sleep_ticks = self.sleep_ticks.saturating_add(1);
                if self.sleep_ticks >= 25 {
                    self.stopped = true;
                    self.velocity = [0; 2];
                    self.omega = 0;
                }
            } else {
                self.sleep_ticks = 0;
            }
        } else {
            self.contact = false;
            self.sleep_ticks = 0;
        }
        // Explicit bounded-world guard, not a visual lifetime or recycling rule.
        if self.center.iter().any(|v| v.abs() > 512 * ONE) {
            *self = old;
            self.stopped = true;
        }
    }
    fn contact_impulse(&mut self, spec: Spec, point: [i32; 2], normal: [i32; 2]) {
        let lever = [point[0] - self.center[0], point[1] - self.center[1]];
        let inverse_inertia = spec.inv_i;
        let angular = mulq(self.omega, 1144); // Q16 radians/sec (pi/180).
        let contact_velocity = [
            self.velocity[0] - mulq(angular, lever[1]),
            self.velocity[1] + mulq(angular, lever[0]),
        ];
        let vn = dot(contact_velocity, normal);
        if vn >= 0 {
            return;
        }
        let rn = crossq(lever, normal);
        let denominator = ONE + mulq(mulq(rn, rn), inverse_inertia);
        let impulse = divq(-vn, denominator);
        self.apply_impulse(lever, normal, impulse, inverse_inertia);
        // Geo Small: friction .2, restitution zero. Coulomb tangent impulse.
        let tangent = [-normal[1], normal[0]];
        let angular = mulq(self.omega, 1144);
        let cv = [
            self.velocity[0] - mulq(angular, lever[1]),
            self.velocity[1] + mulq(angular, lever[0]),
        ];
        let rt = crossq(lever, tangent);
        let jt = divq(-dot(cv, tangent), ONE + mulq(mulq(rt, rt), inverse_inertia));
        let cap = impulse / 5;
        self.apply_impulse(lever, tangent, jt.clamp(-cap, cap), inverse_inertia);
    }
    fn apply_impulse(&mut self, lever: [i32; 2], direction: [i32; 2], amount: i32, inv_i: i32) {
        for i in 0..2 {
            self.velocity[i] += mulq(amount, direction[i]);
        }
        let radians = mulq(mulq(crossq(lever, direction), amount), inv_i);
        self.omega = Wide::of(self.omega)
            .plus(mulq(radians, 3754936))
            .clamp(Wide::of(-360 * ONE), Wide::of(360 * ONE))
            .low();
    }
}
fn mulq(a: i32, b: i32) -> i32 {
    psx_math::int32::mul_shr_trunc_i32(a, b, 16)
}
fn divq(a: i32, b: i32) -> i32 {
    psx_math::int32::mul_div_i32(a, ONE, b)
}
fn dot(a: [i32; 2], b: [i32; 2]) -> i32 {
    mulq(a[0], b[0]) + mulq(a[1], b[1])
}
fn crossq(a: [i32; 2], b: [i32; 2]) -> i32 {
    mulq(a[0], b[1]) - mulq(a[1], b[0])
}
fn supports(body: Body, spec: Spec, e: [i32; 4]) -> bool {
    let mut normal = unit([e[1] - e[3], e[2] - e[0]]);
    if dot([body.center[0] - e[0], body.center[1] - e[1]], normal) < 0 {
        normal = [-normal[0], -normal[1]];
    }
    let tangent = [-normal[1], normal[0]];
    let mut lo = i32::MAX;
    let mut hi = i32::MIN;
    let rotation = Rotation::new(body.angle);
    for &v in spec.polygon() {
        let q = body.point_rotated(v, spec.centroid, spec.scale, rotation);
        if dot([q[0] - e[0], q[1] - e[1]], normal).abs() <= 655 {
            let t = dot([q[0] - body.center[0], q[1] - body.center[1]], tangent);
            lo = lo.min(t);
            hi = hi.max(t);
        }
    }
    lo <= 0 && hi >= 0
}
fn kinetic(v: [i32; 2], omega: i32, spec: Spec) -> Wide {
    let radians = mulq(omega, 1144);
    let inv_i = spec.inv_i.max(1);
    // inv_i >= 1, and radians * radians * ONE is nonnegative.
    let rotation = Wide::mul(radians, radians).shl_by(16).unsigned_abs().div_rem_u32(inv_i as u32).0;
    hk_sim::fixed::sum_sq(v[0], v[1]).add_w(rotation.cast_signed())
}
fn length(v: [i32; 2]) -> i32 {
    let (x, y) = (v[0].unsigned_abs(), v[1].unsigned_abs());
    WideU::mul(x, x).mul_add(y, y).isqrt() as i32
}

fn unit(v: [i32; 2]) -> [i32; 2] {
    let len = length(v);
    if len == 0 {
        [0; 2]
    } else {
        [divq(v[0], len), divq(v[1], len)]
    }
}
// Intersection of vertex sweep and original edge. Coordinate differences fit
// +/-1024 world units; the moving numerator is <= one bounded flight step.
fn crossing(a: [i32; 2], b: [i32; 2], c: [i32; 2], d: [i32; 2]) -> Option<i32> {
    if (0..2).any(|i| a[i].min(b[i]) > c[i].max(d[i]) || a[i].max(b[i]) < c[i].min(d[i])) {
        return None;
    }
    let r = [b[0] - a[0], b[1] - a[1]];
    let s = [d[0] - c[0], d[1] - c[1]];
    let q = [c[0] - a[0], c[1] - a[1]];
    let cross = |a: [i32; 2], b: [i32; 2]| Wide::mul(a[0], b[1]).mul_sub(a[1], b[0]);
    let mut den = cross(r, s);
    if den.is_zero() {
        return None;
    }
    let mut t = cross(q, s);
    let mut u = cross(q, r);
    if den.is_negative() {
        den = -den;
        t = -t;
        u = -u;
    }
    if t.is_negative() || t > den || u.is_negative() || u > den {
        return None;
    }
    // Divide first in the rare long-edge case to keep the shift bounded.
    let bits = match den.high() as u32 {
        0 => 32 - den.low_u32().leading_zeros(),
        high => 64 - high.leading_zeros(),
    };
    let shift = bits.saturating_sub(46);
    let den = den.sar(shift);
    // 0 <= t <= den, so t * ONE / den is at most ONE.
    let along = t.sar(shift).cast_unsigned().scale_by_ratio(ONE as u32, den.cast_unsigned());
    Some(along.min((ONE - 1) as u32) as i32)
}
fn scaled(p: [i32; 2], s: [i32; 2]) -> [i32; 2] {
    core::array::from_fn(|i| psx_math::int32::mul_shr_trunc_i32(p[i], s[i], 16))
}
fn mul_trig(v: i32, q14: i32) -> i32 {
    psx_math::int32::mul_shr_trunc_i32(v, q14, 14)
}
/// One angle's sine and cosine, for rotating several points by it: each
/// `sin` pays a division (`rem_euclid`), so a quad's four corners share them.
#[derive(Clone, Copy)]
pub struct Rotation {
    sin: i32,
    cos: i32,
}
impl Rotation {
    pub fn new(angle: i32) -> Self {
        Self {
            sin: sin(angle),
            cos: sin(angle + 90 * ONE),
        }
    }
    pub fn apply(self, p: [i32; 2]) -> [i32; 2] {
        // Keep each signed product's truncation before addition/subtraction.
        [
            mul_trig(p[0], self.cos) - mul_trig(p[1], self.sin),
            mul_trig(p[0], self.sin) + mul_trig(p[1], self.cos),
        ]
    }
}
pub fn rotate(p: [i32; 2], angle: i32) -> [i32; 2] {
    Rotation::new(angle).apply(p)
}
fn sin(angle: i32) -> i32 {
    let angle = angle.rem_euclid(360 * ONE);
    let index = (angle >> 16) as usize;
    let fraction = angle & 65535;
    let a = SIN[index] as i32;
    let b = SIN[(index + 1) % 360] as i32;
    a + (((b - a) * fraction) >> 16)
}
#[derive(Clone, Copy)]
struct Piece {
    scene: u8,
    /// The piece's `Spec::index` in that scene's bank.
    index: u16,
    finishing: bool,
    /// Consecutive draws entirely off screen.
    off_screen: u8,
    /// 60 Hz ticks since the body fell asleep.
    rest: u16,
    body: Body,
}
/// How much of its size a finishing piece keeps, in Q16: whole until it has
/// slept `FINISH_WAIT`, then linearly gone over `FINISH_SHRINK`.
fn keep(piece: &Piece) -> i32 {
    if !piece.finishing || piece.rest <= FINISH_WAIT {
        return ONE;
    }
    let left = (FINISH_WAIT + FINISH_SHRINK).saturating_sub(piece.rest);
    psx_math::int32::mul_div_i32(ONE, i32::from(left), i32::from(FINISH_SHRINK))
}
pub struct Pool {
    pieces: [Option<Piece>; CAPACITY],
    phase: u8,
    pub evicted: u32,
    pub dropped: u32,
    pub finished: u32,
}
impl Pool {
    pub const fn new() -> Self {
        Self {
            pieces: [None; CAPACITY],
            phase: 0,
            evicted: 0,
            dropped: 0,
            finished: 0,
        }
    }
    /// Launch `specs`' pieces (those of one owner that just broke). A piece already
    /// live keeps its flight. A full pool retires its longest-stopped piece for a new
    /// one; when every piece is still moving the newcomer is dropped and counted.
    pub fn spawn(&mut self, scene: usize, specs: impl Iterator<Item = Spec>, kind: u8, facing: i32) {
        assert!(scene < 256);
        for spec in specs {
            if self
                .pieces
                .iter()
                .flatten()
                .any(|p| p.scene as usize == scene && p.index == spec.index)
            {
                continue;
            }
            let slot = match self.pieces.iter().position(|p| p.is_none()) {
                Some(free) => free,
                None => match self.longest_stopped() {
                    Some(old) => {
                        self.evicted = self.evicted.saturating_add(1);
                        old
                    }
                    None => {
                        self.dropped = self.dropped.saturating_add(1);
                        continue;
                    }
                },
            };
            self.pieces[slot] = Some(Piece {
                scene: scene as u8,
                index: spec.index,
                finishing: spec.finishing,
                off_screen: 0,
                rest: 0,
                body: Body::launch(spec, kind, facing),
            });
        }
        self.publish();
    }
    fn longest_stopped(&self) -> Option<usize> {
        let mut best: Option<(usize, u16)> = None;
        for (i, p) in self.pieces.iter().enumerate() {
            if let Some(p) = p.filter(|p| p.body.stopped) {
                if best.is_none_or(|(_, rest)| p.rest > rest) {
                    best = Some((i, p.rest));
                }
            }
        }
        best.map(|(i, _)| i)
    }
    pub fn clear_scene(&mut self, scene: usize) {
        for p in &mut self.pieces {
            if p.as_ref().is_some_and(|p| p.scene as usize == scene) {
                *p = None;
            }
        }
        self.publish();
    }
    /// The body of the live piece with this index (in any scene).
    pub fn body(&self, index: u16) -> Option<Body> {
        self.pieces.iter().flatten().find(|p| p.index == index).map(|p| p.body)
    }
    pub fn active(&self) -> usize {
        self.pieces.iter().flatten().count()
    }
    fn publish(&self) {
        publish_active(self.active());
        #[cfg(target_arch = "mips")]
        unsafe {
            core::ptr::write_volatile(core::ptr::addr_of_mut!(HK_DEBRIS_EVICTED), self.evicted);
            core::ptr::write_volatile(core::ptr::addr_of_mut!(HK_DEBRIS_DROPPED), self.dropped);
            core::ptr::write_volatile(core::ptr::addr_of_mut!(HK_DEBRIS_FINISHED), self.finished);
        }
    }
    /// One 60 Hz tick. `spec_of` resolves a piece's index in the resident bank.
    pub fn tick(
        &mut self,
        scene: usize,
        spec_of: impl Fn(u16) -> Option<Spec>,
        bounds: [i32; 4],
        count: usize,
        mut edge: impl FnMut(usize) -> [i32; 4],
    ) {
        // FinishingRigidBody: asleep for its wait, then its shrink, then gone.
        let mut freed = false;
        for slot in &mut self.pieces {
            let Some(p) = slot else {
                continue;
            };
            if p.scene as usize != scene || !p.body.stopped {
                continue;
            }
            p.rest = p.rest.saturating_add(1);
            if p.finishing && p.rest >= FINISH_WAIT + FINISH_SHRINK {
                *slot = None;
                self.finished = self.finished.saturating_add(1);
                freed = true;
            }
        }
        if freed {
            self.publish();
        }
        self.phase += 50;
        if self.phase < 60 {
            return;
        }
        self.phase -= 60;
        for slot in &mut self.pieces {
            let Some(p) = slot else {
                continue;
            };
            if p.scene as usize != scene || p.body.stopped {
                continue;
            }
            let Some(s) = spec_of(p.index) else {
                continue;
            };
            #[cfg(target_arch = "mips")]
            crate::input::checkpoint();
            // Check resolved poses against actual resident coverage. A
            // floor on the apron edge can bounce a candidate that would
            // otherwise leave coverage; absent floors cannot advance it.
            let covered = |body: &Body| {
                let rotation = Rotation::new(body.angle);
                s.polygon().iter().all(|&v| {
                    let q = body.point_rotated(v, s.centroid, s.scale, rotation);
                    q[0] >= bounds[0] && q[0] <= bounds[2] && q[1] >= bounds[1] && q[1] <= bounds[3]
                })
            };
            if covered(&p.body) {
                let mut next = p.body;
                next.step(s, count, &mut edge);
                if covered(&next) {
                    p.body = next;
                }
            }
        }
    }
    /// Draw the pieces of `scene` this region has a frame for (`frame_of` gives a piece's
    /// frame in the drawn region's animation bank). A finishing piece that stays off screen
    /// for `FINISH_OFF_SCREEN` draws is deactivated, as the source does.
    pub fn draw(
        &mut self,
        scene: usize,
        spec_of: impl Fn(u16) -> Option<Spec>,
        frame_of: impl Fn(u16) -> Option<u16>,
        room: &Room,
        camera: (i32, i32),
    ) -> u32 {
        let mut count = 0;
        let mut freed = false;
        for slot in &mut self.pieces {
            let Some(p) = slot else {
                continue;
            };
            if p.scene as usize != scene {
                continue;
            }
            let (Some(s), Some(frame)) = (spec_of(p.index), frame_of(p.index)) else {
                continue;
            };
            let f = room.frame(frame as usize);
            let texture = u32_at(f, 0) as usize;
            assert!(!room.texture(texture).is_streamed());
            let b = core::array::from_fn::<_, 4, _>(|i| i32_at(f, 4 + i * 4));
            let kept = keep(p);
            let scale = [mulq(s.scale[0], kept), mulq(s.scale[1], kept)];
            let mut verts = [(0i16, 0i16); 4];
            let rotation = Rotation::new(p.body.angle);
            for (v, local) in
                verts
                    .iter_mut()
                    .zip([[b[0], b[3]], [b[2], b[3]], [b[0], b[1]], [b[2], b[1]]])
            {
                let q = p.body.point_rotated(local, s.centroid, scale, rotation);
                *v = (
                    (160 + psx_math::int32::mul_shr_i32((q[0] - camera.0) >> 8, crate::KNIGHT_SCALE as i32, 20))
                        as i16,
                    (120 - psx_math::int32::mul_shr_i32((q[1] - camera.1) >> 8, crate::KNIGHT_SCALE as i32, 20))
                        as i16,
                );
            }
            if verts.iter().all(|v| v.0 < 0)
                || verts.iter().all(|v| v.0 >= 320)
                || verts.iter().all(|v| v.1 < 0)
                || verts.iter().all(|v| v.1 >= 240)
            {
                if p.finishing {
                    p.off_screen = p.off_screen.saturating_add(1);
                    if p.off_screen >= FINISH_OFF_SCREEN {
                        *slot = None;
                        self.finished = self.finished.saturating_add(1);
                        freed = true;
                    }
                }
                continue;
            }
            p.off_screen = 0;
            let colour = if s.tint { TINT } else { (128, 128, 128) };
            crate::render::texture(texture, verts, colour);
            count += 1;
        }
        if freed {
            self.publish();
        }
        #[cfg(target_arch = "mips")]
        unsafe {
            core::ptr::write_volatile(
                core::ptr::addr_of_mut!(HK_DEBRIS_BOUNCES),
                self.pieces
                    .iter()
                    .flatten()
                    .map(|p| p.body.bounces as u32)
                    .sum(),
            );
            core::ptr::write_volatile(
                core::ptr::addr_of_mut!(HK_DEBRIS_ASLEEP),
                self.pieces
                    .iter()
                    .flatten()
                    .filter(|p| p.body.stopped)
                    .count() as u32,
            );
        }
        publish_drawn(count);
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn piece(index: u16, stopped: bool, rest: u16) -> Option<Piece> {
        let spec = Spec {
            index,
            owner: 0,
            source: 1,
            origin: [0; 2],
            centroid: [0; 2],
            scale: [ONE; 2],
            points: [[0; 2]; MAX_POINTS],
            len: 0,
            torque: 0,
            inv_i: ONE,
            start_pose: 0,
            random_start: true,
            finishing: false,
            tint: false,
            angle_offset: Spec::ANGLE_OFFSET,
        }
        .with_polygon(&[[0, 0], [ONE, 0], [0, ONE]]);
        let mut body = Body::launch(spec, 0, 1);
        body.stopped = stopped;
        Some(Piece {
            scene: 0,
            index,
            finishing: false,
            off_screen: 0,
            rest,
            body,
        })
    }

    #[test]
    fn the_piece_retired_for_a_newcomer_is_the_one_stopped_longest() {
        let mut pool = Pool::new();
        pool.pieces[0] = piece(0, true, 20);
        pool.pieces[1] = piece(1, false, 500);
        pool.pieces[2] = piece(2, true, 50);
        pool.pieces[3] = piece(3, true, 5);
        assert_eq!(pool.longest_stopped(), Some(2), "a moving piece's count is not rest");
        pool.pieces[2] = None;
        assert_eq!(pool.longest_stopped(), Some(0));
        let mut moving = Pool::new();
        moving.pieces[0] = piece(0, false, 0);
        assert_eq!(moving.longest_stopped(), None);
    }

    #[test]
    fn a_finishing_piece_keeps_its_size_until_the_wait_is_over_then_loses_it_in_a_second() {
        let mut p = piece(0, true, 0).unwrap();
        p.finishing = true;
        for (rest, kept) in [
            (0, ONE),
            (FINISH_WAIT, ONE),
            (FINISH_WAIT + FINISH_SHRINK / 2, ONE / 2),
            (FINISH_WAIT + FINISH_SHRINK, 0),
        ] {
            p.rest = rest;
            assert_eq!(keep(&p), kept, "rest {rest}");
        }
        p.finishing = false;
        p.rest = FINISH_WAIT + FINISH_SHRINK;
        assert_eq!(keep(&p), ONE);
    }
}

// Mathematical Q14 sine, integer degree samples with linear interpolation.
const SIN: [i16; 360] = [
    0, 286, 572, 857, 1143, 1428, 1713, 1997, 2280, 2563, 2845, 3126, 3406, 3686, 3964, 4240, 4516,
    4790, 5063, 5334, 5604, 5872, 6138, 6402, 6664, 6924, 7182, 7438, 7692, 7943, 8192, 8438, 8682,
    8923, 9162, 9397, 9630, 9860, 10087, 10311, 10531, 10749, 10963, 11174, 11381, 11585, 11786,
    11982, 12176, 12365, 12551, 12733, 12911, 13085, 13255, 13421, 13583, 13741, 13894, 14044,
    14189, 14330, 14466, 14598, 14726, 14849, 14968, 15082, 15191, 15296, 15396, 15491, 15582,
    15668, 15749, 15826, 15897, 15964, 16026, 16083, 16135, 16182, 16225, 16262, 16294, 16322,
    16344, 16362, 16374, 16382, 16384, 16382, 16374, 16362, 16344, 16322, 16294, 16262, 16225,
    16182, 16135, 16083, 16026, 15964, 15897, 15826, 15749, 15668, 15582, 15491, 15396, 15296,
    15191, 15082, 14968, 14849, 14726, 14598, 14466, 14330, 14189, 14044, 13894, 13741, 13583,
    13421, 13255, 13085, 12911, 12733, 12551, 12365, 12176, 11982, 11786, 11585, 11381, 11174,
    10963, 10749, 10531, 10311, 10087, 9860, 9630, 9397, 9162, 8923, 8682, 8438, 8192, 7943, 7692,
    7438, 7182, 6924, 6664, 6402, 6138, 5872, 5604, 5334, 5063, 4790, 4516, 4240, 3964, 3686, 3406,
    3126, 2845, 2563, 2280, 1997, 1713, 1428, 1143, 857, 572, 286, 0, -286, -572, -857, -1143,
    -1428, -1713, -1997, -2280, -2563, -2845, -3126, -3406, -3686, -3964, -4240, -4516, -4790,
    -5063, -5334, -5604, -5872, -6138, -6402, -6664, -6924, -7182, -7438, -7692, -7943, -8192,
    -8438, -8682, -8923, -9162, -9397, -9630, -9860, -10087, -10311, -10531, -10749, -10963,
    -11174, -11381, -11585, -11786, -11982, -12176, -12365, -12551, -12733, -12911, -13085, -13255,
    -13421, -13583, -13741, -13894, -14044, -14189, -14330, -14466, -14598, -14726, -14849, -14968,
    -15082, -15191, -15296, -15396, -15491, -15582, -15668, -15749, -15826, -15897, -15964, -16026,
    -16083, -16135, -16182, -16225, -16262, -16294, -16322, -16344, -16362, -16374, -16382, -16384,
    -16382, -16374, -16362, -16344, -16322, -16294, -16262, -16225, -16182, -16135, -16083, -16026,
    -15964, -15897, -15826, -15749, -15668, -15582, -15491, -15396, -15296, -15191, -15082, -14968,
    -14849, -14726, -14598, -14466, -14330, -14189, -14044, -13894, -13741, -13583, -13421, -13255,
    -13085, -12911, -12733, -12551, -12365, -12176, -11982, -11786, -11585, -11381, -11174, -10963,
    -10749, -10531, -10311, -10087, -9860, -9630, -9397, -9162, -8923, -8682, -8438, -8192, -7943,
    -7692, -7438, -7182, -6924, -6664, -6402, -6138, -5872, -5604, -5334, -5063, -4790, -4516,
    -4240, -3964, -3686, -3406, -3126, -2845, -2563, -2280, -1997, -1713, -1428, -1143, -857, -572,
    -286,
];
