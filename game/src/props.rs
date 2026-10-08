//! Goams, falling stalactites and grub jars (cooked by host/hk-cook/src/props.rs).
//!
//! None of them is an enemy, and each belongs to a scene rather than a view:
//! the state here is the current scene's, rebuilt on every scene entry the way
//! a Unity scene load rebuilds its objects, except what a save records (a
//! freed grub). Frames live in linked RAM and reach VRAM through the shared
//! 64x64 animation slots under keys above the charm board's; the cooked draws
//! they replace are hidden per drawn view, and the cooked damage boxes of
//! Goams and stalactites are ignored by `world::State::hazard_contact`, since
//! this module decides when each one hurts.
//!
//! - Goam (`Worm Control`): Up (Idle clip, 1 s), Retract (clip to its end),
//!   Down (1 s, collider off), Burst (clip to its end, collider on), repeating
//!   from `Start Down` or Up. Contact damage 1 while the collider is on.
//! - Stalactite (`StalactiteControl`): harmless while it hangs; the Knight
//!   entering its range with a clear line to it starts a 0.5 s fall delay, then
//!   it falls under gravity, hurts on contact, and embeds where it meets
//!   terrain. A nail hit (`OnTriggerEnter2D`, Nail Attack) makes it harmless
//!   and starts a hanging one falling; by the attack's direction an upward
//!   slash breaks it, a side slash bats it away 45 degrees below the
//!   horizontal and a downward one straight down, at `hitVelocity` without
//!   gravity, until it embeds. A break flings the rocks below, raises its dust
//!   where it is and plays `breakSound`; a bat plays `hitSound`, a fall
//!   starting `startFallSound`, and an embedding raises the landing dust.
//! - Stalactite rock (`Particle Rock Small`): DebrisParticle picks its sprite,
//!   scale and black tint (it is drawn without the spin of its torque step).
//!   It flies as a coin of no value in the Geo pool (`geo::ROCK`), whose
//!   ObjectBounce already reflects a body off terrain while it is faster than
//!   its threshold; FinishingRigidBody shrinks it away once it has lain for
//!   its sleep and wait, counted from its first contact. It is not recycled
//!   off screen.
//! - Grub jar (`Bottle Control` + `Grub Control`): the grub idles, and cries
//!   while the Knight is close; a nail hit on the jar with the Knight in range
//!   breaks it, the grub is freed (1 s), plays Freed and leaves.

#[derive(Clone, Copy)]
pub struct Part {
    pub offset: usize,
    pub width: u16,
    pub height: u16,
    pub clut: u8,
    pub bounds: [i32; 4],
}
#[derive(Clone, Copy)]
pub struct Clip {
    pub first: u16,
    pub count: u16,
    /// Frames per second, x256.
    pub fps: u32,
    pub wrap: u8,
    pub loop_start: u16,
}
#[derive(Clone, Copy)]
pub struct Goam {
    pub scene: u16,
    pub position: [i32; 2],
    pub quarter: u8,
    pub mirror: i8,
    pub stretch: i32,
    pub start_down: bool,
    pub collider_on: bool,
    pub hurt: [i32; 4],
    pub hazard: u32,
}
#[derive(Clone, Copy)]
pub struct Stalactite {
    pub scene: u16,
    pub position: [i32; 2],
    pub frame: u16,
    pub embedded: u16,
    /// Damage box relative to the position.
    pub hurt: [i32; 4],
    pub trigger: [i32; 4],
    pub hazard: u32,
}
/// The rock an upward slash flings, shared by every stalactite.
#[derive(Clone, Copy)]
pub struct RockSpec {
    /// Rocks per shatter, inclusive.
    pub count: [u16; 2],
    /// Whole units per second, inclusive.
    pub speed: [u16; 2],
    pub scale: [i32; 2],
    /// The scale the rock frames were cut at.
    pub art_scale: i32,
    /// Chance of the black tint.
    pub black: i32,
    /// Box collider in the rock's own units.
    pub collider: [i32; 4],
    /// Degrees per second of spin per unit per second of vx, at scale 1.
    pub spin: i32,
    pub gravity: i32,
    pub bounce: i32,
    pub bounce_threshold: i32,
    /// At rest this long before Physics2D puts it to sleep.
    pub sleep_ticks: u16,
    /// Asleep this long before it starts to shrink.
    pub wait_ticks: u16,
    pub shrink_ticks: u16,
}
#[derive(Clone, Copy)]
pub struct Grub {
    pub scene: u16,
    pub local: u8,
    pub jar: [i32; 2],
    pub grub: [i32; 2],
    pub mirror: i8,
    pub glass: u16,
    pub body: [i32; 4],
    pub reach: [i32; 4],
    pub close: [i32; 4],
}
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/props.rs"));
// The prop art is room art: it streams with the rooms that place each kind of
// prop (host/code_modules.py, modules.rs); only the palettes stay resident.
#[cfg(not(test))]
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/props_art.rs"));

pub const MAX_GOAMS: usize = 24;
pub const MAX_STALACTITES: usize = 12;
pub const MAX_GRUBS: usize = 4;
const NONE: u16 = u16::MAX;
const ONE: i32 = hk_sim::ONE;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GoamPhase { Up, Retract, Down, Burst }
#[derive(Clone, Copy)]
struct GoamState { index: u16, phase: GoamPhase, ticks: u32, collider: bool }
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fall { Hanging, Triggered(u16), Falling, Embedded, Broken }
#[derive(Clone, Copy)]
struct StalactiteState {
    index: u16, phase: Fall, x: i32, y: i32, vx: i32, vy: i32,
    /// Struck by the nail: `heroDamage` is gone and it never hurts again.
    struck: bool,
    /// Batted: no gravity, and drawn turned by this many eighths of a turn
    /// (+1 counterclockwise, -1 clockwise; `body.rotation`).
    batted: Option<i8>,
}
/// The nail attack's direction, as `damages_enemy`'s `direction` sorts it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NailDirection { Right, Left, Up, Down }
impl StalactiteState {
    /// The `corners` turn it is drawn with: none, or an eighth either way.
    fn turn(&self) -> u8 {
        match self.batted { Some(1) => EIGHTH_CCW, Some(-1) => EIGHTH_CW, _ => 0 }
    }
}
/// `corners` turn bits above the quarter turns: a further eighth of a turn
/// counterclockwise or clockwise (a batted stalactite's `body.rotation`).
pub const EIGHTH_CCW: u8 = 4;
pub const EIGHTH_CW: u8 = 8;
/// Further `corners` bits: y scaled as x is (a rock), and the frame
/// modulated black.
pub const UNIFORM: u8 = 16;
pub const BLACK: u8 = 32;
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GrubPhase { Jarred, Freed(u16), Leaving(u16), Gone }
#[derive(Clone, Copy)]
struct GrubState { index: u16, phase: GrubPhase, ticks: u32, cry: u16, close: bool, facing: i8 }

/// What a tick asks of the Knight: contact damage (1, never a hazard respawn)
/// with its recoil direction.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Tick {
    pub hurt: Option<i32>,
    /// Stalactites that started to fall (`startFallSound`).
    pub fell: u8,
    /// A stalactite that embedded: its slot in the scene, its STALACTITES
    /// index and the point it hit.
    pub landed: Option<(u8, u16, [i32; 2])>,
}
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Strike {
    pub freed: u8,
    pub broken: u8,
    pub batted: u8,
    /// The last struck stalactite's box, where its `Strike Nail R` spawns.
    pub impact: Option<[i32; 4]>,
    /// A stalactite an upward slash broke: its slot in the scene, its
    /// STALACTITES index and where. The caller flings its rocks (`fling`).
    pub shattered: Option<(u8, u16, [i32; 2])>,
}

pub struct World {
    scene: usize,
    goams: [Option<GoamState>; MAX_GOAMS],
    stalactites: [Option<StalactiteState>; MAX_STALACTITES],
    grubs: [Option<GrubState>; MAX_GRUBS],
    rng: u32,
}
#[no_mangle]
pub static mut HK_GOAMS_UP: u32 = 0;
#[no_mangle]
pub static mut HK_STALACTITES_FALLEN: u32 = 0;
#[no_mangle]
pub static mut HK_GRUBS_FREED: u32 = 0;
#[no_mangle]
pub static mut HK_PROPS_DRAWN: u32 = 0;
/// Rocks flung, and rocks a full coin pool dropped.
#[no_mangle]
pub static mut HK_STALACTITE_ROCKS: u32 = 0;
#[no_mangle]
pub static mut HK_STALACTITE_ROCKS_DROPPED: u32 = 0;

fn overlap(a: [i32; 4], b: [i32; 4]) -> bool {
    a[0] <= b[2] && a[2] >= b[0] && a[1] <= b[3] && a[3] >= b[1]
}
fn offset(b: [i32; 4], p: [i32; 2]) -> [i32; 4] {
    [b[0] + p[0], b[1] + p[1], b[2] + p[0], b[3] + p[1]]
}
/// Ticks a once-clip takes to reach its end at its own rate.
pub fn clip_ticks(clip: usize) -> u32 {
    let c = CLIPS[clip];
    (u32::from(c.count) * 60 * 256).div_ceil(c.fps)
}
/// The frame a clip shows `ticks` after it started.
pub fn clip_frame(clip: usize, ticks: u32) -> usize {
    let c = CLIPS[clip];
    let count = u32::from(c.count);
    let elapsed = (u64::from(ticks) * u64::from(c.fps) / (60 * 256)) as u32;
    let start = u32::from(c.loop_start);
    let frame = match c.wrap {
        0 => elapsed % count,
        1 if elapsed >= count && count > start => start + (elapsed - start) % (count - start),
        _ => elapsed.min(count - 1),
    };
    CLIP_FRAMES[(u32::from(c.first) + frame) as usize] as usize
}
/// `Segment` crossing any terrain edge, as the enemies' line-of-sight uses.
fn segment_hits_terrain(a: [i32; 2], b: [i32; 2], count: usize, edge: &impl Fn(usize) -> [i32; 4]) -> bool {
    (0..count).any(|i| {
        let e = edge(i);
        !hk_sim::empty_edge(&e) && crosses(a, b, e)
    })
}
// The guest links the enemies' own segment test once; the host tests,
// which compile this module alone, keep this copy of it.
#[cfg(not(test))]
use crate::enemies::segment_crosses as crosses;
/// Whether segment `a`-`b` crosses edge `e`.
#[cfg(test)]
fn crosses(a: [i32; 2], b: [i32; 2], e: [i32; 4]) -> bool {
    let (ox, oy) = (a[0] as i64, a[1] as i64);
    let (rx, ry) = (b[0] as i64 - ox, b[1] as i64 - oy);
    let (ax, ay) = (e[0] as i64, e[1] as i64);
    let (sx, sy) = (e[2] as i64 - ax, e[3] as i64 - ay);
    let den = rx * sy - ry * sx;
    if den == 0 { return false; }
    let (qx, qy) = (ax - ox, ay - oy);
    let (mut t, mut u, mut d) = (qx * sy - qy * sx, qx * ry - qy * rx, den);
    if d < 0 { t = -t; u = -u; d = -d; }
    t >= 0 && t <= d && u >= 0 && u <= d
}
const fn mulq(a: i32, b: i32) -> i32 { ((a as i64 * b as i64) >> 16) as i32 }
/// The rock's body in the Geo coin pool (`geo::ROCK`): its collider at the
/// middle of its scale range, of no value, and stopped dead where it lies.
#[cfg(not(test))]
pub const ROCK_COIN: crate::geo::CoinSpec = {
    let (s, c) = (STALACTITE_ROCKS.scale, STALACTITE_ROCKS.collider);
    let s = (s[0] + s[1]) / 2;
    crate::geo::CoinSpec {
        value: 0, gravity: -STALACTITE_ROCKS.gravity,
        body_offset: [mulq((c[0] + c[2]) / 2, s), mulq((c[1] + c[3]) / 2, s)],
        half: [mulq((c[2] - c[0]) / 2, s), mulq((c[3] - c[1]) / 2, s)],
        pickup_offset: [0; 2], pickup_half: [0; 2],
        bounce: STALACTITE_ROCKS.bounce, threshold: STALACTITE_ROCKS.bounce_threshold, friction: ONE,
    }
};
/// Ticks a rock lies before it is gone: asleep, waiting and shrinking.
pub const ROCK_TICKS: u32 = (STALACTITE_ROCKS.sleep_ticks + STALACTITE_ROCKS.wait_ticks + STALACTITE_ROCKS.shrink_ticks) as u32;
/// A rock's look in its coin denomination: the frame (two bits), the black
/// tint and one of this many scales (three bits).
const ROCK_SCALES: u32 = 8;
const _: () = assert!(STALACTITE_ROCK_FRAMES.len() <= 4);
/// The frame, origin, `corners` bits and scale a rock coin is drawn with.
#[cfg(not(test))]
fn rock_draw(c: &crate::geo::Coin) -> (usize, [i32; 2], u8, i32) {
    let spec = STALACTITE_ROCKS;
    // The frames were cut at art_scale.
    const LOW: i32 = (STALACTITE_ROCKS.scale[0] as i64 * ONE as i64 / STALACTITE_ROCKS.art_scale as i64) as i32;
    const HIGH: i32 = (STALACTITE_ROCKS.scale[1] as i64 * ONE as i64 / STALACTITE_ROCKS.art_scale as i64) as i32;
    let look = c.denomination;
    let mut scale = LOW + (HIGH - LOW) * i32::from(look >> 3 & 7) / (ROCK_SCALES as i32 - 1);
    // FinishingRigidBody shrinks it to nothing over its last shrink_ticks.
    let shrink_from = u32::from(spec.sleep_ticks + spec.wait_ticks);
    if c.age > shrink_from {
        scale = scale * (ROCK_TICKS.saturating_sub(c.age)) as i32 / i32::from(spec.shrink_ticks.max(1));
    }
    let bits = UNIFORM | if look & 4 != 0 { BLACK } else { 0 };
    (STALACTITE_ROCK_FRAMES[usize::from(look & 3)] as usize, [c.x, c.y], bits, scale)
}
fn polygon_hits_box(polygon: &[[i32; 2]], b: [i32; 4]) -> bool {
    let (mut lo, mut hi) = ([i32::MAX; 2], [i32::MIN; 2]);
    for p in polygon {
        lo = [lo[0].min(p[0]), lo[1].min(p[1])];
        hi = [hi[0].max(p[0]), hi[1].max(p[1])];
    }
    overlap([lo[0], lo[1], hi[0], hi[1]], b)
}

impl World {
    pub const fn new() -> Self {
        Self { scene: usize::MAX, goams: [None; MAX_GOAMS], stalactites: [None; MAX_STALACTITES],
               grubs: [None; MAX_GRUBS], rng: 0x5eed_9ab1 }
    }
    /// A scene entered: every prop starts over, as the scene's objects do on a
    /// load, but a jar whose grub was freed stays broken (`freed(local)`).
    pub fn enter_scene(&mut self, scene: usize, freed: impl Fn(usize) -> bool) {
        if self.scene == scene { return; }
        self.scene = scene;
        self.goams = [None; MAX_GOAMS];
        self.stalactites = [None; MAX_STALACTITES];
        self.grubs = [None; MAX_GRUBS];
        let mut n = 0;
        for (i, g) in GOAMS.iter().enumerate().filter(|(_, g)| g.scene as usize == scene) {
            assert!(n < MAX_GOAMS, "Goams in one scene");
            let (phase, collider) = if g.start_down { (GoamPhase::Down, false) } else { (GoamPhase::Up, g.collider_on) };
            self.goams[n] = Some(GoamState { index: i as u16, phase, ticks: 0, collider });
            n += 1;
        }
        n = 0;
        for (i, s) in STALACTITES.iter().enumerate().filter(|(_, s)| s.scene as usize == scene) {
            assert!(n < MAX_STALACTITES, "stalactites in one scene");
            self.stalactites[n] = Some(StalactiteState { index: i as u16, phase: Fall::Hanging, x: s.position[0], y: s.position[1],
                vx: 0, vy: 0, struck: false, batted: None });
            n += 1;
        }
        n = 0;
        for (i, g) in GRUBS.iter().enumerate().filter(|(_, g)| g.scene as usize == scene) {
            assert!(n < MAX_GRUBS, "grub jars in one scene");
            let phase = if freed(g.local as usize) { GrubPhase::Gone } else { GrubPhase::Jarred };
            let cry = self.cry_wait();
            self.grubs[n] = Some(GrubState { index: i as u16, phase, ticks: 0, cry, close: false, facing: g.mirror });
            n += 1;
        }
    }
    fn cry_wait(&mut self) -> u16 {
        let span = u32::from(GRUB_CRY_TICKS[1] - GRUB_CRY_TICKS[0]) + 1;
        GRUB_CRY_TICKS[0] + self.random(span) as u16
    }
    /// 0 to `span - 1`.
    fn random(&mut self, span: u32) -> u32 {
        self.rng = self.rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.rng >> 8) % span
    }
    /// `FlingObjects` for the stalactite an upward slash broke at `at`: each
    /// rock's `DebrisParticle.OnEnable` draws its sprite, scale and tint, then
    /// the fling its speed and direction, and the Geo coin pool flies it.
    #[cfg(not(test))]
    #[inline(never)]
    pub fn fling(&mut self, geo: &mut crate::geo::World, at: [i32; 2]) {
        let spec = STALACTITE_ROCKS;
        let count = u32::from(spec.count[0]) + self.random(u32::from(spec.count[1] - spec.count[0]) + 1);
        for _ in 0..count {
            let mut look = self.random(STALACTITE_ROCK_FRAMES.len() as u32) as u8 | (self.random(ROCK_SCALES) as u8) << 3;
            if (self.random(ONE as u32) as i32) < spec.black { look |= 4; }
            let speed = (u32::from(spec.speed[0]) + self.random(u32::from(spec.speed[1] - spec.speed[0]) + 1)) as i32 * ONE;
            let degrees = self.random(360) as i32;
            let counter = if geo.launch_rock(self.scene, at, degrees, speed, look) { &raw mut HK_STALACTITE_ROCKS }
                else { &raw mut HK_STALACTITE_ROCKS_DROPPED };
            unsafe { *counter = (*counter).wrapping_add(1); }
        }
    }
    /// The cooked hazard with this source id belongs to a Goam or a stalactite
    /// of the current scene, whose damage this module runs instead.
    pub fn owns_hazard(&self, source_id: u32) -> bool {
        source_id != 0
            && (self.goams.iter().flatten().any(|g| GOAMS[g.index as usize].hazard == source_id)
                || self.stalactites.iter().flatten().any(|s| STALACTITES[s.index as usize].hazard == source_id))
    }
    /// One 60 Hz simulation tick. `body` is the Knight's box, `centre` his
    /// position; terrain is the resident view's edges.
    pub fn tick(&mut self, body: [i32; 4], centre: [i32; 2], count: usize, edge: impl Fn(usize) -> [i32; 4]) -> Tick {
        let mut out = Tick::default();
        let hurt_from = |x: i32| if centre[0] < x { -1 } else { 1 };
        for g in self.goams.iter_mut().flatten() {
            let spec = GOAMS[g.index as usize];
            g.ticks += 1;
            let (next, collider) = match g.phase {
                GoamPhase::Up if g.ticks >= u32::from(GOAM_UP_TICKS) => (Some(GoamPhase::Retract), g.collider),
                GoamPhase::Retract if g.ticks >= clip_ticks(GOAM_CLIPS[1] as usize) => (Some(GoamPhase::Down), false),
                GoamPhase::Down if g.ticks >= u32::from(GOAM_DOWN_TICKS) => (Some(GoamPhase::Burst), true),
                GoamPhase::Burst if g.ticks >= clip_ticks(GOAM_CLIPS[3] as usize) => (Some(GoamPhase::Up), g.collider),
                _ => (None, g.collider),
            };
            if let Some(phase) = next {
                g.phase = phase;
                g.ticks = 0;
                g.collider = collider;
                if phase == GoamPhase::Up {
                    unsafe { HK_GOAMS_UP = HK_GOAMS_UP.wrapping_add(1); }
                }
            }
            if g.collider && overlap(spec.hurt, body) {
                out.hurt = Some(hurt_from(spec.position[0]));
            }
        }
        for (slot, s) in self.stalactites.iter_mut().enumerate() {
            let Some(s) = s else { continue };
            let spec = STALACTITES[s.index as usize];
            match s.phase {
                Fall::Hanging => {
                    if overlap(spec.trigger, body) && !segment_hits_terrain(spec.position, centre, count, &edge) {
                        s.phase = Fall::Triggered(STALACTITE_DELAY_TICKS);
                        out.fell += 1;
                    }
                }
                Fall::Triggered(0) => {
                    s.phase = Fall::Falling;
                    unsafe { HK_STALACTITES_FALLEN = HK_STALACTITES_FALLEN.wrapping_add(1); }
                }
                Fall::Triggered(left) => s.phase = Fall::Triggered(left - 1),
                Fall::Falling => {
                    // A batted one flies at its hit velocity, gravity scale 0.
                    if s.batted.is_none() { s.vy -= STALACTITE_GRAVITY / 60; }
                    let tip = |x: i32, y: i32| [x, y + spec.hurt[1]];
                    let before = (s.x, s.y);
                    s.x += s.vx / 60;
                    s.y += s.vy / 60;
                    if segment_hits_terrain(tip(before.0, before.1), tip(s.x, s.y), count, &edge) {
                        s.phase = Fall::Embedded;
                        out.landed = Some((slot as u8, s.index, tip(s.x, s.y)));
                    } else if s.y < spec.position[1] - 256 * hk_sim::ONE || (s.x - spec.position[0]).abs() > 256 * hk_sim::ONE {
                        s.phase = Fall::Broken;
                    } else if !s.struck && overlap(offset(spec.hurt, [s.x, s.y]), body) {
                        out.hurt = Some(hurt_from(s.x));
                    }
                }
                Fall::Embedded | Fall::Broken => {}
            }
        }
        for g in self.grubs.iter_mut().flatten() {
            let spec = GRUBS[g.index as usize];
            g.ticks += 1;
            match g.phase {
                GrubPhase::Jarred => {
                    let close = overlap(spec.close, body);
                    if close != g.close {
                        g.close = close;
                        g.ticks = 0;
                    }
                    if close {
                        // FaceObject: the grub's art faces left unmirrored.
                        g.facing = if centre[0] < spec.grub[0] { 1 } else { -1 };
                    }
                }
                GrubPhase::Freed(left) => {
                    g.phase = if left == 0 { g.ticks = 0; GrubPhase::Leaving(0) } else { GrubPhase::Freed(left - 1) };
                }
                GrubPhase::Leaving(_) => {
                    if g.ticks >= clip_ticks(GRUB_CLIPS[2] as usize).max(u32::from(GRUB_LEAVE_TICKS)) {
                        g.phase = GrubPhase::Gone;
                    }
                }
                GrubPhase::Gone => {}
            }
        }
        out
    }
    /// The nail's swept polygon this tick. Returns what broke; the caller
    /// records each freed grub (`freed_local`).
    pub fn strike(&mut self, polygon: &[[i32; 2]], body: [i32; 4], direction: NailDirection, mut freed_local: impl FnMut(usize)) -> Strike {
        let mut out = Strike::default();
        if polygon.len() < 3 { return out; }
        for slot in 0..MAX_STALACTITES {
            let Some(s) = self.stalactites[slot].as_mut() else { continue };
            let spec = STALACTITES[s.index as usize];
            // The trigger fires while the nail overlaps it; a batted one that
            // is still inside the swing is not struck again.
            let hurt = offset(spec.hurt, [s.x, s.y]);
            if s.batted.is_some() || !matches!(s.phase, Fall::Hanging | Fall::Triggered(_) | Fall::Falling)
                || !polygon_hits_box(polygon, hurt) {
                continue;
            }
            s.struck = true;
            out.impact = Some(hurt);
            // Euler(0, 0, angle) * down * hitVelocity: (sin, -cos) of the angle.
            const DIAGONAL: i32 = 46341; // 1/sqrt(2), Q16
            let speed = (i64::from(STALACTITE_HIT_SPEED) * i64::from(DIAGONAL) >> 16) as i32;
            let (eighths, vx, vy) = match direction {
                NailDirection::Up => {
                    s.phase = Fall::Broken;
                    out.broken += 1;
                    out.shattered = Some((slot as u8, s.index, [s.x, s.y]));
                    continue;
                }
                NailDirection::Right => (1, speed, -speed),
                NailDirection::Left => (-1, -speed, -speed),
                NailDirection::Down => (0, 0, -STALACTITE_HIT_SPEED),
            };
            s.phase = Fall::Falling;
            s.batted = Some(eighths);
            s.vx = vx;
            s.vy = vy;
            out.batted += 1;
        }
        for g in self.grubs.iter_mut().flatten() {
            let spec = GRUBS[g.index as usize];
            // `Shatter` runs only while the Knight stands in `Hero Range`.
            if g.phase == GrubPhase::Jarred && polygon_hits_box(polygon, spec.body) && overlap(spec.reach, body) {
                g.phase = GrubPhase::Freed(GRUB_FREE_TICKS);
                g.ticks = 0;
                out.freed += 1;
                freed_local(spec.local as usize);
                unsafe { HK_GRUBS_FREED = HK_GRUBS_FREED.wrapping_add(1); }
            }
        }
        out
    }
    /// Every frame to draw now, as (frame, origin, quarter, horizontal scale).
    pub fn visible(&self, mut f: impl FnMut(usize, [i32; 2], u8, i32)) {
        for g in self.goams.iter().flatten() {
            let spec = GOAMS[g.index as usize];
            let clip = GOAM_CLIPS[match g.phase { GoamPhase::Up => 0, GoamPhase::Retract => 1, GoamPhase::Down => 2, GoamPhase::Burst => 3 }] as usize;
            f(clip_frame(clip, g.ticks), spec.position, spec.quarter,
              i32::from(spec.mirror) * spec.stretch);
        }
        for s in self.stalactites.iter().flatten() {
            let spec = STALACTITES[s.index as usize];
            match s.phase {
                Fall::Broken => {}
                Fall::Embedded if spec.embedded == NONE => {}
                Fall::Embedded => f(spec.embedded as usize, [s.x, s.y], s.turn(), hk_sim::ONE),
                _ => f(spec.frame as usize, [s.x, s.y], s.turn(), hk_sim::ONE),
            }
        }
        for g in self.grubs.iter().flatten() {
            let spec = GRUBS[g.index as usize];
            let scale = i32::from(g.facing) * hk_sim::ONE;
            match g.phase {
                GrubPhase::Jarred => {
                    let clip = if g.close { GRUB_CLIPS[1] } else { GRUB_CLIPS[0] } as usize;
                    f(clip_frame(clip, g.ticks), spec.grub, 0, scale);
                    f(spec.glass as usize, spec.jar, 0, hk_sim::ONE);
                }
                GrubPhase::Freed(_) => f(clip_frame(GRUB_CLIPS[0] as usize, g.ticks), spec.grub, 0, scale),
                GrubPhase::Leaving(_) => f(clip_frame(GRUB_CLIPS[2] as usize, g.ticks), spec.grub, 0, scale),
                GrubPhase::Gone => {}
            }
        }
    }
    /// The scene starts over on the next `enter_scene`, even the same one
    /// (a death reloads the scene the Knight died in).
    pub fn leave(&mut self) { self.scene = usize::MAX; }
    #[cfg(test)]
    fn goam_phase(&self, n: usize) -> (GoamPhase, bool) {
        let g = self.goams[n].unwrap();
        (g.phase, g.collider)
    }
    #[cfg(test)]
    fn fall(&self, n: usize) -> Fall { self.stalactites[n].unwrap().phase }
    #[cfg(test)]
    fn grub_phase(&self, n: usize) -> GrubPhase { self.grubs[n].unwrap().phase }
}

/// Stalactite effect owners in the cooked break effects:
/// `STALACTITE_EFFECT_OWNER | slot << 1 | kind` (host/hk-cook/src/break_effects.rs
/// `STALACTITE_OWNER`), kind 0 an upward slash's dust and 1 the landing's.
pub const STALACTITE_EFFECT_OWNER: usize = 0xC000;
/// Raise one stalactite's dust where it is now: its emitters are cooked at
/// its hanging position.
#[cfg(not(test))]
#[inline(never)]
pub fn stalactite_dust(scene: usize, slot: u8, index: u16, kind: usize, at: [i32; 2]) {
    let s = STALACTITES[index as usize].position;
    let owner = STALACTITE_EFFECT_OWNER | (slot as usize) << 1 | kind;
    crate::world::particles::pool().spawn_break_at(scene, owner, [at[0] - s[0], at[1] - s[1]]);
}
/// The cooked draws the drawn view shows for props this module draws itself.
pub fn view_draws(view: usize) -> &'static [u16] {
    match BINDINGS.binary_search_by_key(&(view as u16), |&(r, _)| r) {
        Ok(at) => BINDINGS[at].1,
        Err(_) => &[],
    }
}
/// Hide them, after `world::State::apply` has reset the view's visibility.
#[cfg(not(test))]
pub fn apply(view: usize) {
    for &draw in view_draws(view) { crate::render::set_visible(draw as usize, false); }
}
/// A frame's world corners for one part, origin-relative box `b`.
pub fn corners(b: [i32; 4], origin: [i32; 2], quarter: u8, sx: i32) -> [[i32; 2]; 4] {
    // A rock scales its y extent as x: once here, on the box.
    let b = if quarter & UNIFORM != 0 { [b[0], mulq(b[1], sx), b[2], mulq(b[3], sx)] } else { b };
    // tk2d vertex order the quads use: top-left, top-right, bottom-left, bottom-right.
    let local = [[b[0], b[3]], [b[2], b[3]], [b[0], b[1]], [b[2], b[1]]];
    local.map(|[x, y]| {
        let x = ((i64::from(x) * i64::from(sx)) >> 16) as i32;
        let (x, y) = match quarter & 3 { 0 => (x, y), 1 => (-y, x), 2 => (-x, -y), _ => (y, -x) };
        // An eighth of a turn: (x -+ y, y +- x) / sqrt(2).
        let diagonal = |v: i32| ((i64::from(v) * 46341) >> 16) as i32;
        let (x, y) = if quarter & EIGHTH_CCW != 0 { (diagonal(x - y), diagonal(x + y)) }
            else if quarter & EIGHTH_CW != 0 { (diagonal(x + y), diagonal(y - x)) } else { (x, y) };
        [origin[0] + x, origin[1] + y]
    })
}

#[cfg(not(test))]
mod presentation {
    use super::*;
    use psx_gpu::{material::{BlendMode, TextureMaterial}, prim::QuadTextured};
    use psx_vram::{upload_bytes, Clut, VramRect};
    /// Animation-cache keys, above the charm board's.
    pub const KEY_BASE: u16 = crate::charms::KEY_BASE + crate::charms::CHARM_COUNT as u16;
    const _: () = assert!((KEY_BASE as usize) + PARTS.len() <= hk_cache::MAX_KEYS);
    const MAX_DRAWS: usize = 16;
    /// The frame's props, decided before the working set is prepared: only
    /// parts whose keys fit the working set are drawn.
    static mut DRAWS: [(u16, [i32; 2], u8, i32); MAX_DRAWS] = [(0, [0; 2], 0, 0); MAX_DRAWS];
    static mut DRAW_COUNT: usize = 0;
    pub fn upload() {
        for i in 0..PALETTE_COUNT {
            upload_bytes(VramRect::new(CLUT_RECT.0, CLUT_RECT.1 + i as u16, CLUT_RECT.2, CLUT_RECT.3),
                &PALETTES[i * 32..i * 32 + 32]);
        }
    }
    pub fn texels(part: usize) -> Option<(&'static [u8], u16, u16)> {
        let p = PARTS.get(part)?;
        let (package, start) = PART_ART[part];
        let art = crate::modules::data(crate::modules::ART_GRUB + package as usize)?;
        let start = start as usize;
        let len = (usize::from(p.width) + 3) / 4 * 2 * usize::from(p.height);
        art.get(start..start + len).map(|t| (t, p.width, p.height))
    }
    fn project(p: [i32; 2], camera: (i32, i32)) -> (i32, i32) {
        (160 + (((i64::from(p[0]) - i64::from(camera.0)) * i64::from(crate::KNIGHT_SCALE)) >> 28) as i32,
         120 - (((i64::from(p[1]) - i64::from(camera.1)) * i64::from(crate::KNIGHT_SCALE)) >> 28) as i32)
    }
    fn on_screen(frame: usize, origin: [i32; 2], quarter: u8, sx: i32, camera: (i32, i32)) -> bool {
        let (first, count) = FRAMES[frame];
        (first..first + count).any(|part| {
            let v = corners(PARTS[part as usize].bounds, origin, quarter, sx).map(|p| project(p, camera));
            !(v.iter().all(|p| p.0 < 0) || v.iter().all(|p| p.0 >= 320)
              || v.iter().all(|p| p.1 < 0) || v.iter().all(|p| p.1 >= 240))
        })
    }
    impl World {
        /// Choose this frame's props and add their keys to the working set,
        /// only as far as it has room: a prop whose parts do not all fit is
        /// left out of the frame rather than drawn in part.
        pub fn append_needed(&self, camera: (i32, i32), rocks: &crate::geo::World, needed: &mut [u16], len: &mut usize) {
            let mut n = 0;
            let mut add = |frame: usize, origin: [i32; 2], quarter: u8, sx: i32| {
                if n == MAX_DRAWS || !on_screen(frame, origin, quarter, sx, camera) { return; }
                let (first, count) = FRAMES[frame];
                // A gate ends with its room's art (disc::admit_scenes); only if
                // that failed is a prop left out until its art has arrived.
                if !(first..first + count).all(|part| crate::modules::loaded(crate::modules::ART_GRUB + PART_ART[part as usize].0 as usize)) {
                    unsafe { crate::modules::HK_MODULE_ART_LATE = crate::modules::HK_MODULE_ART_LATE.saturating_add(1); }
                    return;
                }
                let mut room = *len;
                for part in first..first + count {
                    let key = KEY_BASE + part;
                    if needed[..room].contains(&key) { continue; }
                    if room == needed.len() { return; }
                    needed[room] = key;
                    room += 1;
                }
                *len = room;
                unsafe { DRAWS[n] = (frame as u16, origin, quarter, sx); }
                n += 1;
            };
            self.visible(&mut add);
            for c in rocks.coins().filter(|c| c.denomination & crate::geo::ROCK != 0 && c.scene as usize == self.scene) {
                let (frame, origin, quarter, sx) = rock_draw(c);
                add(frame, origin, quarter, sx);
            }
            unsafe { DRAW_COUNT = n; }
        }
        pub fn draw(&self, camera: (i32, i32)) -> u32 {
            let mut drawn = 0;
            for i in 0..unsafe { DRAW_COUNT } {
                let (frame, origin, quarter, sx) = unsafe { DRAWS[i] };
                let shade = if quarter & BLACK != 0 { 0 } else { 128 };
                let (first, count) = FRAMES[frame as usize];
                for part in first..first + count {
                    let p = PARTS[part as usize];
                    let key = KEY_BASE + part;
                    let (u, v) = crate::render::animation_uv(key);
                    let right = (u16::from(u) + p.width - 1) as u8;
                    let bottom = (u16::from(v) + p.height - 1) as u8;
                    let clut = Clut::new(CLUT_RECT.0, CLUT_RECT.1 + u16::from(p.clut)).uv_clut_word();
                    let template = QuadTextured::with_material([(0, 0); 4], [(u, v), (right, v), (u, bottom), (right, bottom)],
                        TextureMaterial::blended(clut, crate::render::animation_tpage_word(key), (shade, shade, shade), BlendMode::Average));
                    let verts = corners(p.bounds, origin, quarter, sx).map(|c| {
                        let (x, y) = project(c, camera);
                        (x.clamp(-1024, 1023) as i16, y.clamp(-1024, 1023) as i16)
                    });
                    crate::render::resident_quad(&template, verts);
                    drawn += 1;
                }
            }
            unsafe { HK_PROPS_DRAWN = drawn; }
            drawn
        }
    }
}
#[cfg(not(test))]
pub use presentation::{upload, texels, KEY_BASE};

#[cfg(test)]
mod tests {
    use super::*;
    fn scene_of<T>(list: &[T], scene: impl Fn(&T) -> u16) -> Option<usize> {
        list.first().map(|t| scene(t) as usize)
    }
    const FAR: [i32; 4] = [-500 * hk_sim::ONE, -500 * hk_sim::ONE, -499 * hk_sim::ONE, -499 * hk_sim::ONE];
    #[test]
    fn a_goam_cycles_up_retract_down_burst_and_hurts_only_with_its_collider_on() {
        let Some(scene) = scene_of(GOAMS, |g| g.scene) else { return };
        let mut w = World::new();
        w.enter_scene(scene, |_| false);
        let first = GOAMS.iter().position(|g| g.scene as usize == scene).unwrap();
        let spec = GOAMS[first];
        let (mut phase, _) = w.goam_phase(0);
        let mut seen = std::vec::Vec::new();
        let inside = spec.hurt;
        for _ in 0..600 {
            assert!(w.tick(FAR, [0, 0], 0, |_| [0; 4]).hurt.is_none());
            let (now, _) = w.goam_phase(0);
            if now != phase { seen.push(now); phase = now; }
        }
        let expected = [GoamPhase::Retract, GoamPhase::Down, GoamPhase::Burst, GoamPhase::Up];
        let start = seen.iter().position(|p| *p == GoamPhase::Retract).unwrap();
        assert_eq!(&seen[start..start + 4], &expected);
        // Contact: a Knight standing in the box is hurt while the collider is
        // on and never while the Goam is down.
        let mut w = World::new();
        w.enter_scene(scene, |_| false);
        for _ in 0..600 {
            let t = w.tick(inside, [inside[0], inside[1]], 0, |_| [0; 4]);
            let (phase, collider) = w.goam_phase(0);
            if phase == GoamPhase::Down { assert!(!collider); }
            if !collider {
                // Only this Goam's box is at `inside`, unless another overlaps.
                let others = w.goams.iter().flatten().skip(1)
                    .any(|g| g.collider && overlap(GOAMS[g.index as usize].hurt, inside));
                assert!(t.hurt.is_none() || others);
            } else {
                assert!(t.hurt.is_some());
            }
        }
    }
    #[test]
    fn a_stalactite_hangs_harmless_then_falls_hurts_and_embeds() {
        let Some(scene) = scene_of(STALACTITES, |s| s.scene) else { return };
        let mut w = World::new();
        w.enter_scene(scene, |_| false);
        let spec = STALACTITES[STALACTITES.iter().position(|s| s.scene as usize == scene).unwrap()];
        let hanging = offset(spec.hurt, spec.position);
        // Touching a hanging one does nothing, outside its trigger.
        assert!(w.tick(hanging, [hanging[0], hanging[1]], 0, |_| [0; 4]).hurt.is_none() || overlap(spec.trigger, hanging));
        let mut w = World::new();
        w.enter_scene(scene, |_| false);
        let t = spec.trigger;
        let x = spec.position[0];
        let knight = [x - hk_sim::ONE / 2, t[1], x + hk_sim::ONE / 2, t[1] + 2 * hk_sim::ONE];
        let floor = t[1] - hk_sim::ONE;
        let edge = move |_| [spec.position[0] - 4 * hk_sim::ONE, floor, spec.position[0] + 4 * hk_sim::ONE, floor];
        w.tick(knight, [knight[0], knight[1]], 1, edge);
        assert_eq!(w.fall(0), Fall::Triggered(STALACTITE_DELAY_TICKS));
        for _ in 0..=STALACTITE_DELAY_TICKS { w.tick(FAR, [0, 0], 1, edge); }
        assert_eq!(w.fall(0), Fall::Falling);
        let mut hurt = false;
        for _ in 0..600 {
            hurt |= w.tick(knight, [knight[0], knight[1]], 1, edge).hurt.is_some();
            if w.fall(0) == Fall::Embedded { break; }
        }
        assert!(hurt, "a falling stalactite hurts the Knight under it");
        assert_eq!(w.fall(0), Fall::Embedded);
        // An upward slash breaks a hanging one.
        let mut w = World::new();
        w.enter_scene(scene, |_| false);
        let box_ = offset(spec.hurt, spec.position);
        let poly = [[box_[0], box_[1]], [box_[2], box_[1]], [box_[2], box_[3]]];
        assert_eq!(w.strike(&poly, FAR, NailDirection::Up, |_| {}).broken, 1);
        assert_eq!(w.fall(0), Fall::Broken);
    }
    #[test]
    fn a_struck_stalactite_is_batted_away_harmless_and_embeds() {
        let Some(scene) = scene_of(STALACTITES, |s| s.scene) else { return };
        let spec = STALACTITES[STALACTITES.iter().position(|s| s.scene as usize == scene).unwrap()];
        let box_ = offset(spec.hurt, spec.position);
        let poly = [[box_[0], box_[1]], [box_[2], box_[1]], [box_[2], box_[3]]];
        let floor = spec.position[1] - 6 * hk_sim::ONE;
        let edge = move |_| [spec.position[0] - 40 * hk_sim::ONE, floor, spec.position[0] + 40 * hk_sim::ONE, floor];
        for (direction, sign) in [(NailDirection::Right, 1), (NailDirection::Left, -1), (NailDirection::Down, 0)] {
            let mut w = World::new();
            w.enter_scene(scene, |_| false);
            let hit = w.strike(&poly, FAR, direction, |_| {});
            assert_eq!((hit.broken, hit.batted), (0, 1));
            assert_eq!(w.fall(0), Fall::Falling);
            // Struck once: still inside the swing, it is not struck again.
            assert_eq!(w.strike(&poly, FAR, direction, |_| {}).batted, 0);
            // A Knight standing in its path is never hurt by it now.
            let mut hurt = false;
            let mut ticks = 0;
            while w.fall(0) == Fall::Falling && ticks < 600 {
                let s = w.stalactites[0].unwrap();
                let path = offset(spec.hurt, [s.x, s.y]);
                hurt |= w.tick(path, [path[0], path[1]], 1, edge).hurt.is_some();
                ticks += 1;
            }
            assert!(!hurt, "a struck stalactite does not hurt");
            assert_eq!(w.fall(0), Fall::Embedded);
            let s = w.stalactites[0].unwrap();
            // Straight lines at hitVelocity, 45 degrees below the horizontal
            // for a side slash: as far across as down.
            let (dx, dy) = (s.x - spec.position[0], spec.position[1] - s.y);
            assert!(dy > 0);
            assert_eq!(dx.signum(), sign);
            if sign != 0 { assert!((dx.abs() - dy).abs() <= 2 * hk_sim::ONE, "dx {dx} dy {dy}"); }
            assert!(ticks < 60, "50 units a second covers six units in well under a second");
        }
    }
    #[test]
    fn an_upward_slash_breaks_a_stalactite_where_it_hangs() {
        let Some(scene) = scene_of(STALACTITES, |s| s.scene) else { return };
        let spec = STALACTITES[STALACTITES.iter().position(|s| s.scene as usize == scene).unwrap()];
        let mut w = World::new();
        w.enter_scene(scene, |_| false);
        let box_ = offset(spec.hurt, spec.position);
        let poly = [[box_[0], box_[1]], [box_[2], box_[1]], [box_[2], box_[3]]];
        let hit = w.strike(&poly, FAR, NailDirection::Up, |_| {});
        assert_eq!((hit.broken, hit.batted, hit.impact), (1, 0, Some(box_)));
        assert_eq!(hit.shattered.map(|(slot, _, at)| (slot, at)), Some((0, spec.position)));
        assert_eq!(w.fall(0), Fall::Broken);
        assert!(ROCK_TICKS > 0);
    }
    #[test]
    fn a_jar_breaks_only_with_the_knight_in_range_and_a_freed_grub_stays_gone() {
        let Some(scene) = scene_of(GRUBS, |g| g.scene) else { return };
        let spec = GRUBS[GRUBS.iter().position(|g| g.scene as usize == scene).unwrap()];
        let poly = [[spec.body[0], spec.body[1]], [spec.body[2], spec.body[1]], [spec.body[2], spec.body[3]]];
        let mut w = World::new();
        w.enter_scene(scene, |_| false);
        assert_eq!(w.strike(&poly, FAR, NailDirection::Right, |_| panic!("out of range")).freed, 0);
        let mut saved = None;
        assert_eq!(w.strike(&poly, spec.reach, NailDirection::Right, |l| saved = Some(l)).freed, 1);
        assert_eq!(saved, Some(spec.local as usize));
        for _ in 0..1000 { w.tick(FAR, [0, 0], 0, |_| [0; 4]); }
        assert_eq!(w.grub_phase(0), GrubPhase::Gone);
        let mut w = World::new();
        w.enter_scene(scene, |l| l == spec.local as usize);
        assert_eq!(w.grub_phase(0), GrubPhase::Gone);
    }
    #[test]
    fn corners_turn_and_mirror_the_frame_box() {
        let b = [-hk_sim::ONE, 0, hk_sim::ONE, 2 * hk_sim::ONE];
        assert_eq!(corners(b, [0, 0], 0, hk_sim::ONE)[0], [-hk_sim::ONE, 2 * hk_sim::ONE]);
        assert_eq!(corners(b, [0, 0], 0, -hk_sim::ONE)[0], [hk_sim::ONE, 2 * hk_sim::ONE]);
        assert_eq!(corners(b, [0, 0], 2, hk_sim::ONE)[0], [hk_sim::ONE, -2 * hk_sim::ONE]);
        assert_eq!(corners(b, [0, 0], 1, hk_sim::ONE)[0], [-2 * hk_sim::ONE, -hk_sim::ONE]);
        // A rock scales y as x; the black bit changes no corner.
        assert_eq!(corners(b, [0, 0], UNIFORM | BLACK, hk_sim::ONE / 2)[0], [-hk_sim::ONE / 2, hk_sim::ONE]);
    }
    #[test]
    fn every_clip_frame_and_part_exists() {
        for c in CLIPS { assert!(usize::from(c.first + c.count) <= CLIP_FRAMES.len()); }
        for &f in CLIP_FRAMES { assert!((f as usize) < FRAMES.len()); }
        for &(first, count) in FRAMES { assert!(usize::from(first + count) <= PARTS.len()); }
        for p in PARTS { assert!(p.width <= 64 && p.height <= 64); }
    }
}
