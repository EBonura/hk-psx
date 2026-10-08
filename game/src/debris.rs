//! Original door fragments and source ObjectBounce lifecycle.
//! Uniform polygon inertia and implicit angular damping approximate native Box2D.
//! Contact impulses approximate native Box2D; source bounce cache/threshold and
//! sleep tolerances remain explicit and no visual lifetime is invented.
use hk_format::{i32_at, u32_at, Room};
use hk_sim::ONE;
pub const CAPACITY: usize = 28;
#[no_mangle]
pub static mut HK_DEBRIS_ACTIVE: u32 = 0;
#[no_mangle]
pub static mut HK_DEBRIS_DRAWN: u32 = 0;
#[no_mangle]
pub static mut HK_DEBRIS_BOUNCES: u32 = 0;
#[no_mangle]
pub static mut HK_DEBRIS_ASLEEP: u32 = 0;
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
#[derive(Clone, Copy, Debug)]
pub struct Spec {
    pub id: u8,
    pub door: u16,
    pub frame: u16,
    pub source: u32,
    pub origin: [i32; 2],
    pub centroid: [i32; 2],
    pub scale: [i32; 2],
    pub polygon: &'static [[i32; 2]],
    /// Q16 degrees/sec increment per unit/sec of vx, for one source torque step.
    pub torque: i32,
    pub angle_offset: i16,
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
        let rotation = (((random(&mut seed) % 360) as i32 + offset).rem_euclid(360)) * ONE;
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
        self.point_rotated(local,centroid,scale,Rotation::new(self.angle))
    }
    fn point_rotated(&self,local:[i32;2],centroid:[i32;2],scale:[i32;2],rotation:Rotation)->[i32;2] {
        let p=rotation.apply([local[0]-centroid[0],local[1]-centroid[1]]);
        let p=scaled(p,scale);
        [self.center[0]+p[0],self.center[1]+p[1]]
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
            self.omega = ((self.velocity[0] as i64 * spec.torque as i64) >> 16)
                .clamp(-360 * ONE as i64, 360 * ONE as i64) as i32;
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
        assert!(spec.polygon.len() <= 16);
        let mut before = [[0; 2]; 16];
        let mut after = [[0; 2]; 16];
        let mut swept = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
        let old_rotation=Rotation::new(old.angle);let next_rotation=Rotation::new(self.angle);
        for (index, &p) in spec.polygon.iter().enumerate() {
            before[index] = old.point_rotated(p, spec.centroid, spec.scale,old_rotation);
            after[index] = self.point_rotated(p, spec.centroid, spec.scale,next_rotation);
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
            for j in 0..spec.polygon.len() {
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
            for j in 0..spec.polygon.len() {
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
                    + (((self.center[i] - old.center[i]) as i64 * fraction as i64) >> 16) as i32;
            }
            self.angle = (old.angle + ((rotation as i64 * fraction as i64) >> 16) as i32)
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
            let contact_rotation=Rotation::new(self.angle);let mut contacts=0;
            for &v in spec.polygon {
                let q=self.point_rotated(v,spec.centroid,spec.scale,contact_rotation);
                if dot([q[0]-edge[0],q[1]-edge[1]],normal).abs()<=655 {
                    after[contacts]=q;contacts+=1;
                }
            }
            for _ in 0..8 {
                for &q in &after[..contacts] {self.contact_impulse(spec,q,normal);}
                if contacts==0 {self.contact_impulse(spec,point,normal);}
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
            let mut penetration = 0;let resolved_rotation=Rotation::new(self.angle);
            for &v in spec.polygon {
                let p = self.point_rotated(v, spec.centroid, spec.scale,resolved_rotation);
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
                let budget = (energy_before
                    - 2 * 60 * ONE as i64 * (self.center[1] as i64 - old.center[1] as i64))
                    .max(0);
                let energy = kinetic(self.velocity, self.omega, spec);
                if energy > budget {
                    let ratio = (budget / ((energy + 65535) >> 16)).clamp(0, ONE as i64) as u64;
                    let factor = sqrt(ratio * ONE as u64) as i32;
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
        // Recover inverse moment from the existing authored torque coefficient:
        // torque = invI * (4/50) * degrees_per_radian.
        let inverse_inertia = mulq(spec.torque, 14298);
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
        self.omega = (self.omega as i64 + mulq(radians, 3754936) as i64)
            .clamp(-360 * ONE as i64, 360 * ONE as i64) as i32;
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
    let rotation=Rotation::new(body.angle);
    for &v in spec.polygon {
        let q = body.point_rotated(v, spec.centroid, spec.scale,rotation);
        if dot([q[0] - e[0], q[1] - e[1]], normal).abs() <= 655 {
            let t = dot([q[0] - body.center[0], q[1] - body.center[1]], tangent);
            lo = lo.min(t);
            hi = hi.max(t);
        }
    }
    lo <= 0 && hi >= 0
}
fn kinetic(v: [i32; 2], omega: i32, spec: Spec) -> i64 {
    let radians = mulq(omega, 1144) as i64;
    let inv_i = mulq(spec.torque, 14298).max(1) as i64;
    v[0] as i64 * v[0] as i64 + v[1] as i64 * v[1] as i64 + radians * radians * ONE as i64 / inv_i
}
fn sqrt(n: u64) -> u32 { psx_math::int32::isqrt_u64(n) }
fn length(v: [i32; 2]) -> i32 {
    sqrt((v[0] as i64 * v[0] as i64 + v[1] as i64 * v[1] as i64) as u64) as i32
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
    let r = [b[0] as i64 - a[0] as i64, b[1] as i64 - a[1] as i64];
    let s = [d[0] as i64 - c[0] as i64, d[1] as i64 - c[1] as i64];
    let q = [c[0] as i64 - a[0] as i64, c[1] as i64 - a[1] as i64];
    let cross = |a: [i64; 2], b: [i64; 2]| a[0] * b[1] - a[1] * b[0];
    let mut den = cross(r, s);
    if den == 0 {
        return None;
    }
    let mut t = cross(q, s);
    let mut u = cross(q, r);
    if den < 0 {
        den = -den;
        t = -t;
        u = -u;
    }
    if t < 0 || t > den || u < 0 || u > den {
        return None;
    }
    // Divide first in the rare long-edge case to keep the shift bounded.
    let shift = (64 - den.leading_zeros()).saturating_sub(46);
    let den = den >> shift;
    Some((((t >> shift) * ONE as i64) / den).min((ONE - 1) as i64) as i32)
}
fn scaled(p: [i32; 2], s: [i32; 2]) -> [i32; 2] {
    core::array::from_fn(|i| psx_math::int32::mul_shr_trunc_i32(p[i], s[i], 16))
}
fn mul_trig(v: i32, q14: i32) -> i32 {
    psx_math::int32::mul_shr_trunc_i32(v, q14, 14)
}
/// One angle's sine and cosine, for rotating several points by it: each
/// `sin` pays a division (`rem_euclid`), so a quad's four corners share them.
#[derive(Clone,Copy)]
pub struct Rotation {sin:i32,cos:i32}
impl Rotation {
    pub fn new(angle:i32)->Self {Self{sin:sin(angle),cos:sin(angle+90*ONE)}}
    pub fn apply(self,p:[i32;2])->[i32;2] {
        // Keep each signed product's truncation before addition/subtraction.
        [mul_trig(p[0],self.cos)-mul_trig(p[1],self.sin),
         mul_trig(p[0],self.sin)+mul_trig(p[1],self.cos)]
    }
}
pub fn rotate(p: [i32; 2], angle: i32) -> [i32; 2] {Rotation::new(angle).apply(p)}
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
    body: Body,
}
pub struct Pool {
    pieces: [Option<Piece>; CAPACITY],
    phase: u8,
}
impl Pool {
    pub const fn new() -> Self {
        Self {
            pieces: [None; CAPACITY],
            phase: 0,
        }
    }
    pub fn spawn(&mut self, scene: usize, specs: &[Spec], door: usize, kind: u8, facing: i32) {
        assert!(scene < 256);
        for &s in specs.iter().filter(|s| s.door as usize == door) {
            let slot = &mut self.pieces[s.id as usize];
            if slot.is_none() {
                *slot = Some(Piece {
                    scene: scene as u8,
                    body: Body::launch(s, kind, facing),
                });
            }
        }
        publish_active(self.active());
    }
    pub fn clear_scene(&mut self, scene: usize) {
        for p in &mut self.pieces {
            if p.as_ref().is_some_and(|p| p.scene as usize == scene) {
                *p = None;
            }
        }
        publish_active(self.active());
    }
    pub fn body(&self, id: usize) -> Option<Body> {
        self.pieces[id].map(|p| p.body)
    }
    pub fn active(&self) -> usize {
        self.pieces.iter().flatten().count()
    }
    pub fn tick(
        &mut self,
        scene: usize,
        specs: &[Spec],
        bounds: [i32; 4],
        count: usize,
        mut edge: impl FnMut(usize) -> [i32; 4],
    ) {
        self.phase += 50;
        if self.phase < 60 {
            return;
        }
        self.phase -= 60;
        for s in specs {
            if let Some(p) = &mut self.pieces[s.id as usize] {
                if p.scene as usize != scene || p.body.stopped {
                    continue;
                }
                #[cfg(target_arch="mips")]
                crate::input::checkpoint();
                // Check resolved poses against actual resident coverage. A
                // floor on the apron edge can bounce a candidate that would
                // otherwise leave coverage; absent floors cannot advance it.
                let covered = |body: &Body| {
                    let rotation=Rotation::new(body.angle);
                    s.polygon.iter().all(|&v| {
                        let q = body.point_rotated(v, s.centroid, s.scale,rotation);
                        q[0] >= bounds[0]
                            && q[0] <= bounds[2]
                            && q[1] >= bounds[1]
                            && q[1] <= bounds[3]
                    })
                };
                if covered(&p.body) {
                    let mut next = p.body;
                    next.step(*s, count, &mut edge);
                    if covered(&next) {
                        p.body = next;
                    }
                }
            }
        }
    }
    pub fn draw(&self, scene: usize, specs: &[Spec], room: &Room, camera: (i32, i32)) -> u32 {
        let mut count = 0;
        for &s in specs {
            let Some(p) = &self.pieces[s.id as usize] else {
                continue;
            };
            if p.scene as usize != scene {
                continue;
            }
            let f = room.frame(s.frame as usize);
            let texture = u32_at(f, 0) as usize;
            assert!(!room.texture(texture).is_streamed());
            let b = core::array::from_fn::<_, 4, _>(|i| i32_at(f, 4 + i * 4));
            let mut verts = [(0i16, 0i16); 4];
            let rotation=Rotation::new(p.body.angle);
            for (v, local) in
                verts
                    .iter_mut()
                    .zip([[b[0], b[3]], [b[2], b[3]], [b[0], b[1]], [b[2], b[1]]])
            {
                let q = p.body.point_rotated(local, s.centroid, s.scale,rotation);
                *v = (
                    (160 + ((((q[0] - camera.0) as i64 >> 8) * crate::KNIGHT_SCALE as i64) >> 20))
                        as i16,
                    (120 - ((((q[1] - camera.1) as i64 >> 8) * crate::KNIGHT_SCALE as i64) >> 20))
                        as i16,
                );
            }
            if verts.iter().all(|v| v.0 < 0)
                || verts.iter().all(|v| v.0 >= 320)
                || verts.iter().all(|v| v.1 < 0)
                || verts.iter().all(|v| v.1 >= 240)
            {
                continue;
            }
            crate::render::texture(texture, verts, (128, 128, 128));
            count += 1;
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
