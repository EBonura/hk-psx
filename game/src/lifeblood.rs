//! Source HealthCocoon/ScuttlerControl subset. Two authored bugs, no heap.
//! Run/land/activation/heal timings come from the installed CIL. Terrain uses
//! the existing 60Hz swept-box solver rather than claiming native Box2D parity.
use hk_sim::{polygon_hits_box, Params, Player, ONE};
#[derive(Clone, Copy, Debug)]
pub struct Spec {
    pub scene: usize,
    pub origin: [i32; 2],
    pub bounds: [i32; 4],
    pub fling_speed: [i32; 2],
    pub fling_angle: [i32; 2],
    pub spread: [i32; 2],
    pub scale: [i32; 2],
    pub speed: [i32; 2],
    pub body: [i32; 4],
    pub gravity: i32,
    pub acceleration: i32,
    pub activate_ticks: u16,
    pub heal_ticks: u16,
    pub land_ticks: u16,
    pub bounce_ticks: u16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Air,
    Land,
    Run,
    Bounce,
    Healing,
}
#[derive(Clone, Copy, Debug)]
pub struct Bug {
    pub body: Player,
    pub vx: i32,
    pub scale: i32,
    pub max_speed: i32,
    pub phase: Phase,
    pub age: u32,
    pub animation_age: u32,
    pub timer: u16,
    pub direction: i32,
}
pub struct World {
    pub bugs: [Option<Bug>; 2],
    opened: bool,
    opened_age: u16,
    rng: u32,
    pub granted: u32,
    pub struck: u32,
}
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Strike {
    pub opened: bool,
    pub hit_bugs: u16,
}
#[no_mangle]
pub static mut HK_LIFEBLOOD_OPENED: u32 = 0;
#[no_mangle]
pub static mut HK_LIFEBLOOD_ACTIVE: u32 = 0;
#[no_mangle]
pub static mut HK_LIFEBLOOD_GRANTED: u32 = 0;
#[no_mangle]
pub static mut HK_LIFEBLOOD_STRUCK: u32 = 0;
fn checkpoint() {
    #[cfg(not(test))]
    crate::input::checkpoint();
}
impl World {
    pub const fn new() -> Self {
        Self {
            bugs: [None; 2],
            opened: false,
            opened_age: 0,
            rng: 12337,
            granted: 0,
            struck: 0,
        }
    }
    pub fn reset(&mut self) {
        *self = Self::new();
        self.publish();
    }
    pub fn opened(&self) -> bool {
        self.opened
    }
    /// The cocoon's `PersistentBoolItem` says it was opened in an earlier
    /// session or before a death: seat it open, splat finished, no bugs.
    pub fn restore_opened(&mut self) {
        self.opened = true;
        self.opened_age = 15;
        self.bugs = [None; 2];
        self.publish();
    }
    /// Splat2 ends on its transparent seventh frame at6/24seconds.
    pub fn splat_visible(&self) -> bool {
        self.opened && self.opened_age < 15
    }
    fn publish(&self) {
        #[cfg(not(test))]
        unsafe {
            HK_LIFEBLOOD_OPENED = u32::from(self.opened);
            HK_LIFEBLOOD_ACTIVE = self
                .bugs
                .iter()
                .flatten()
                .filter(|b| b.phase != Phase::Healing)
                .count() as u32;
            HK_LIFEBLOOD_GRANTED = self.granted;
            HK_LIFEBLOOD_STRUCK = self.struck;
        }
    }
    #[inline(never)]
    pub fn strike_with(&mut self, spec: Spec, scene: usize, polygon: &[[i32; 2]]) -> Strike {
        if scene != spec.scene || !(3..=16).contains(&polygon.len()) {
            return Strike::default();
        }
        let mut event = Strike::default();
        if !self.opened && polygon_hits_box(polygon, spec.bounds) {
            self.opened = true;
            event.opened = true;
            for slot in &mut self.bugs {
                let angle = range(&mut self.rng, spec.fling_angle);
                let speed = range(&mut self.rng, spec.fling_speed);
                let x = spec.origin[0] + range(&mut self.rng, [-spec.spread[0], spec.spread[0]]);
                let y = spec.origin[1] + range(&mut self.rng, [-spec.spread[1], spec.spread[1]]);
                let mut body = Player::spawn(x, y);
                body.vy = mul(speed, sin(angle));
                *slot = Some(Bug {
                    body,
                    vx: mul(speed, sin(angle + 90 * ONE)),
                    scale: range(&mut self.rng, spec.scale),
                    max_speed: range(&mut self.rng, spec.speed),
                    phase: Phase::Air,
                    age: 0,
                    animation_age: 0,
                    timer: 0,
                    direction: 1,
                });
            }
        }
        for bug in self.bugs.iter_mut().flatten() {
            if bug.phase == Phase::Healing || bug.age < u32::from(spec.activate_ticks) {
                continue;
            }
            if polygon_hits_box(polygon, bug.bounds(spec)) {
                bug.phase = Phase::Healing;
                bug.timer = spec.heal_ticks;
                bug.vx = 0;
                bug.body.vy = 0;
                event.hit_bugs += 1;
                self.struck += 1;
            }
        }
        self.publish();
        event
    }
    /// One gameplay60Hz tick. Healing delay also advances outside the current
    /// view's collision apron; views are not Unity scene unloads.
    #[inline(never)]
    pub fn tick_with(
        &mut self,
        spec: Spec,
        scene: usize,
        hero_x: i32,
        coverage: [i32; 4],
        count: usize,
        edge: impl Fn(usize) -> [i32; 4],
    ) -> u16 {
        if scene != spec.scene {
            return 0;
        }
        if self.opened {
            self.opened_age = self.opened_age.saturating_add(1);
        }
        let mut healed = 0;
        for slot in &mut self.bugs {
            let Some(bug) = slot.as_mut() else {
                continue;
            };
            checkpoint();
            if bug.phase == Phase::Healing {
                bug.timer = bug.timer.saturating_sub(1);
                if bug.timer == 0 {
                    *slot = None;
                    healed += 1;
                }
                continue;
            }
            bug.age = bug.age.saturating_add(1);
            // Covered-room edges only; no invented collision in unloaded views.
            if bug.body.x < coverage[0]
                || bug.body.x > coverage[2]
                || bug.body.y < coverage[1]
                || bug.body.y > coverage[3]
            {
                continue;
            }
            bug.step(spec, hero_x, count, &edge, &mut self.rng);
        }
        self.granted = self.granted.saturating_add(u32::from(healed));
        self.publish();
        healed
    }
    /// Original Heal registers an unload callback. Pending grants happen once
    /// on an actual scene exit; unstruck live bugs do not grant health.
    pub fn leave_scene(&mut self) -> u16 {
        let pending = self
            .bugs
            .iter()
            .flatten()
            .filter(|b| b.phase == Phase::Healing)
            .count() as u16;
        self.bugs = [None; 2];
        self.opened_age = 15;
        self.granted = self.granted.saturating_add(u32::from(pending));
        self.publish();
        pending
    }
}
impl Bug {
    pub fn bounds(&self, spec: Spec) -> [i32; 4] {
        [
            self.body.x + mul(spec.body[0], self.scale),
            self.body.y + mul(spec.body[1], self.scale),
            self.body.x + mul(spec.body[2], self.scale),
            self.body.y + mul(spec.body[3], self.scale),
        ]
    }
    fn step(
        &mut self,
        spec: Spec,
        hero_x: i32,
        count: usize,
        edge: &impl Fn(usize) -> [i32; 4],
        rng: &mut u32,
    ) {
        self.animation_age = self.animation_age.wrapping_add(1);
        if self.phase == Phase::Land || self.phase == Phase::Bounce {
            self.timer = self.timer.saturating_sub(1);
            if self.timer == 0 {
                self.phase = Phase::Run;
                self.animation_age = 0;
            }
        }
        if self.phase == Phase::Run {
            // Source Run yields one extra Update when crossing the hero's X.
            let dir = if hero_x >= self.body.x { 1 } else { -1 };
            if dir == self.direction {
                self.vx =
                    (self.vx - dir * spec.acceleration).clamp(-self.max_speed, self.max_speed);
            }
            self.direction = dir;
        }
        let p = Params {
            speed: self.vx.abs(),
            gravity: spec.gravity,
            fall: i32::MAX,
            half_width: mul(spec.body[2], self.scale),
            bottom: mul(spec.body[1], self.scale),
            top: mul(spec.body[3], self.scale),
            ..Params::ZERO
        };
        let old = self.body;
        self.body.step(p, self.vx.signum(), false, count, edge);
        if self.phase == Phase::Air && !old.grounded && self.body.grounded {
            self.phase = Phase::Land;
            self.timer = spec.land_ticks;
            self.animation_age = 0;
        }
        // Source horizontal ray hits terrain while running: speed5, angle
        // 50..70 or110..130, then0.5s before Run resumes. Swept contact is the
        // bounded approximation to its width/2+0.1 raycast and ObjectBounce.
        let dx = self.vx / 60;
        if self.phase == Phase::Run && dx != 0 && self.body.x != old.x + dx {
            let angles = if self.vx > 0 {
                [110 * ONE, 130 * ONE]
            } else {
                [50 * ONE, 70 * ONE]
            };
            let angle = range(rng, angles);
            self.vx = mul(5 * ONE, sin(angle + 90 * ONE));
            self.body.vy = mul(5 * ONE, sin(angle));
            self.body.grounded = false;
            self.phase = Phase::Bounce;
            self.timer = spec.bounce_ticks;
        }
    }
}
fn mul(a: i32, b: i32) -> i32 {
    psx_math::int32::mul_shr_i32(a, b, 16)
}
fn range(rng: &mut u32, span: [i32; 2]) -> i32 {
    assert!(span[1] >= span[0]);
    *rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
    span[0] + ((u64::from(*rng) * (span[1] - span[0]) as u64) >> 32) as i32
}
const SINE: [i32; 91] = [
    0, 1144, 2287, 3430, 4572, 5712, 6850, 7987, 9121, 10252, 11380, 12505, 13626, 14742, 15855,
    16962, 18064, 19161, 20252, 21336, 22415, 23486, 24550, 25607, 26656, 27697, 28729, 29753,
    30767, 31772, 32768, 33754, 34729, 35693, 36647, 37590, 38521, 39441, 40348, 41243, 42126,
    42995, 43852, 44695, 45525, 46341, 47143, 47930, 48703, 49461, 50203, 50931, 51643, 52339,
    53020, 53684, 54332, 54963, 55578, 56175, 56756, 57319, 57865, 58393, 58903, 59396, 59870,
    60326, 60764, 61183, 61584, 61966, 62328, 62672, 62997, 63303, 63589, 63856, 64104, 64332,
    64540, 64729, 64898, 65048, 65177, 65287, 65376, 65446, 65496, 65526, 65536,
];
fn sin(angle: i32) -> i32 {
    let a = angle.rem_euclid(360 * ONE);
    let quadrant = a / (90 * ONE);
    let mut t = a % (90 * ONE);
    if quadrant == 1 || quadrant == 3 {
        t = 90 * ONE - t;
    }
    let i = (t / ONE) as usize;
    let v = if i == 90 {
        ONE
    } else {
        SINE[i] + mul(SINE[i + 1] - SINE[i], t % ONE)
    };
    if quadrant >= 2 {
        -v
    } else {
        v
    }
}

#[cfg(not(test))]
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/lifeblood.rs"));

#[derive(Clone, Copy)]
pub struct Clip {
    pub start: usize,
    pub count: usize,
    pub fps: u32,
    pub wrap: u8,
}
#[derive(Clone, Copy)]
pub struct Art {
    pub u: u8,
    pub v: u8,
    pub w: u8,
    pub h: u8,
    pub clut: u16,
    pub tpage: u16,
    pub bounds: [i32; 4],
}
pub struct Frame {
    pub parts: &'static [Art],
}
pub struct Upload {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    pub offset: usize,
}
pub struct Binding {
    pub off: &'static [u16],
    pub edges: &'static [u16],
}
impl Bug {
    pub fn frame(&self, clips: [Clip; 3]) -> usize {
        let index = match self.phase {
            Phase::Air => 0,
            Phase::Land => 1,
            _ => 2,
        };
        let c = clips[index];
        let frame = (u64::from(self.animation_age) * u64::from(c.fps) / 60) as usize;
        c.start
            + if c.wrap == 0 {
                frame % c.count
            } else {
                frame.min(c.count - 1)
            }
    }
}
#[cfg(not(test))]
impl World {
    pub fn strike(&mut self, scene: usize, polygon: &[[i32; 2]]) -> Strike {
        self.strike_with(LIFE_SPEC, scene, polygon)
    }
    pub fn tick(
        &mut self,
        scene: usize,
        hero_x: i32,
        coverage: [i32; 4],
        count: usize,
        edge: impl Fn(usize) -> [i32; 4],
    ) -> u16 {
        self.tick_with(LIFE_SPEC, scene, hero_x, coverage, count, edge)
    }
}
#[cfg(not(test))]
mod presentation {
    use super::*;
    use crate::{render, world};
    use psx_gpu::{
        material::{BlendMode, TextureMaterial},
        ot::OrderingTable,
        prim::{QuadTextured, Sprite},
    };
    use psx_vram::{upload_bytes, VramRect};
    #[no_mangle]
    pub static mut HK_LIFEBLOOD_DRAWN: u32 = 0;
    /// Upload the cocoon art from the boot art chunk the title stages.
    pub fn upload(boot: &[u8]) {
        let (at, len) = crate::boot_art::LIFE_ART;
        let data = &boot[at..at + len];
        assert!(data.len() <= 5120);
        for entry in LIFE_UPLOADS {
            assert!(hk_cache::residency::LIFE_RECTS
                .iter()
                .any(|&(x, y, w, h)| entry.x as usize >= x
                    && entry.y as usize >= y
                    && entry.x as usize + entry.w as usize <= x + w
                    && entry.y as usize + entry.h as usize <= y + h));
            let len = entry.w as usize * entry.h as usize * 2;
            upload_bytes(
                VramRect::new(entry.x, entry.y, entry.w, entry.h),
                &data[entry.offset..entry.offset + len],
            );
        }
    }
    /// The cocoon's bindings in a catalogue region, or nothing. `LIFE_BINDINGS`
    /// is sparse and sorted so the linked table grows with the cocoons rather
    /// than with every view the catalogue admits.
    fn region_binding(region: usize) -> Option<&'static Binding> {
        let key = u16::try_from(region).ok()?;
        let at = LIFE_BINDINGS.binary_search_by_key(&key, |&(r, _)| r).ok()?;
        Some(&LIFE_BINDINGS[at].1)
    }
    /// `region` is the Knight's view (the colliders he stands in), `view` the
    /// one being drawn (whose cooked cocoon draws the opened cocoon hides);
    /// they differ while the camera is outside the Knight's view's range.
    pub fn apply(life: &World, state: &mut world::State, region: usize, view: usize) {
        if let Some(b) = region_binding(view).filter(|_| life.opened()) {
            for &draw in b.off {
                render::set_visible(draw as usize, false);
            }
        }
        let bound = region_binding(region).filter(|_| life.opened());
        if let Some(b) = bound {
            state.set_lifeblood_edges(b.edges);
        } else {
            state.set_lifeblood_edges(&[]);
        }
    }
    fn material(art: Art) -> TextureMaterial {
        TextureMaterial::blended(art.clut, art.tpage, (128, 128, 128), BlendMode::Average)
    }
    fn draw_frame(
        index: usize,
        origin: [i32; 2],
        scale: i32,
        facing: i32,
        camera: (i32, i32),
    ) -> u32 {
        let mut drawn = 0;
        for &art in LIFE_FRAMES[index].parts {
            let b = art.bounds;
            let world = [[b[0], b[3]], [b[2], b[3]], [b[0], b[1]], [b[2], b[1]]];
            let vertices = world.map(|[x, y]| {
                let x = origin[0] + mul(x, scale) * facing;
                let y = origin[1] + mul(y, scale);
                (
                    160 + (((i64::from(x) - i64::from(camera.0)) * i64::from(crate::KNIGHT_SCALE))
                        >> 28) as i32,
                    120 - (((i64::from(y) - i64::from(camera.1)) * i64::from(crate::KNIGHT_SCALE))
                        >> 28) as i32,
                )
            });
            if vertices.iter().all(|p| p.0 < 0)
                || vertices.iter().all(|p| p.0 >= 320)
                || vertices.iter().all(|p| p.1 < 0)
                || vertices.iter().all(|p| p.1 >= 240)
            {
                continue;
            }
            let right = (u16::from(art.u) + u16::from(art.w) - 1) as u8;
            let bottom = (u16::from(art.v) + u16::from(art.h) - 1) as u8;
            let template = QuadTextured::with_material(
                [(0, 0); 4],
                [
                    (art.u, art.v),
                    (right, art.v),
                    (art.u, bottom),
                    (right, bottom),
                ],
                material(art),
            );
            render::resident_quad(&template, vertices.map(|(x, y)| (x as i16, y as i16)));
            drawn += 1;
        }
        drawn
    }
    #[inline(never)]
    pub fn draw(life: &World, region: usize, camera: (i32, i32)) -> u32 {
        let mut drawn = 0;
        if world::scene_of(region) == LIFE_SPEC.scene {
            drawn += if !life.opened() {
                draw_frame(INTACT, LIFE_SPEC.origin, ONE, 1, camera)
            } else if life.splat_visible() {
                draw_frame(SPLAT, SPLAT_ORIGIN, ONE, 1, camera)
            } else {
                0
            };
            for bug in life
                .bugs
                .iter()
                .flatten()
                .filter(|b| b.phase != Phase::Healing)
            {
                // Cooked bug geometry/art uses maximum source scale1.5.
                drawn += draw_frame(
                    bug.frame(LIFE_CLIPS),
                    [bug.body.x, bug.body.y],
                    bug.scale * 2 / 3,
                    bug.direction,
                    camera,
                );
            }
        }
        assert!(drawn <= 16);
        unsafe {
            HK_LIFEBLOOD_DRAWN = drawn;
        }
        drawn
    }
    static mut BLUE: [Sprite; 22] =
        [const { Sprite::new(0, 0, 0, 0, (0, 0), 0, 128, 128, 128) }; 22];
    /// Twenty testing masks plus the authored two-bug cocoon fit this pool.
    /// Use the original Blue Idle image, not a blue tint of a white mask.
    pub fn append_hud(ot: &mut OrderingTable<1>, max_health: u16, blue_health: u16) {
        assert!(
            blue_health <= 22,
            "Lifeblood HUD exceeds admitted mask count"
        );
        let parts = LIFE_FRAMES[BLUE_HUD].parts;
        assert_eq!(parts.len(), 1);
        let art = parts[0];
        for i in (0..blue_health).rev() {
            unsafe {
                let (x, y) = crate::hud::mask_position(max_health + i);
                BLUE[i as usize] = Sprite::with_material(
                    x,
                    y,
                    u16::from(art.w),
                    u16::from(art.h),
                    (art.u, art.v),
                    material(art),
                );
                ot.add(0, &mut BLUE[i as usize], Sprite::WORDS);
            }
        }
    }
}
#[cfg(not(test))]
pub use presentation::{append_hud, apply, draw, upload};
