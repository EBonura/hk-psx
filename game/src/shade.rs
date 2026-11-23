//! The Hollow Shade left behind by death, and the Geo pool it carries.
//!
//! The Shade can appear in whichever scene the Knight died in, so its frames
//! are not in any room's atlas: they live in linked RAM and reach VRAM through
//! the shared 64x64 animation slots under keys above `SCENE_TEXTURE_CAPACITY`.
//! Only its CLUTs are resident. Behaviour is hk_sim::shade; see docs/SHADE.md.
use psx_fx::NrLcg;
use hk_sim::shade::{mirrored, Action, Clip as Pose, Phase, Senses, Shade, ShadowBall, Spells};
use psx_math::Wide;
use hk_sim::{Params, Player, ONE};

#[derive(Clone, Copy)]
pub struct Clip {
    pub start: usize,
    pub count: usize,
    pub fps: u32,
    pub wrap: u8,
}
#[derive(Clone, Copy)]
pub struct Frame {
    pub offset: usize,
    pub width: u16,
    pub height: u16,
    pub clut: usize,
    pub bounds: [i32; 4],
}
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/shade.rs"));
/// The Shade's palettes and frames: a carried data package (modules.rs
/// `carry`), in the pool only while the Shade's scene is, not linked.
#[cfg(not(test))]
fn data() -> Option<&'static [u8]> {
    crate::modules::data(crate::modules::ART_SHADE)
}
#[cfg(test)]
fn data() -> Option<&'static [u8]> {
    Some(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../data/shade.hk"
    )))
}
/// Animation-cache keys start above every room's texture table.
pub const KEY_BASE: u16 = crate::disc::SCENE_TEXTURE_CAPACITY as u16;
#[no_mangle]
pub static mut HK_SHADE_PRESENT: u32 = 0;
#[no_mangle]
pub static mut HK_SHADE_GEO_POOL: u32 = 0;
#[no_mangle]
pub static mut HK_SHADE_HP: u32 = 0;
#[no_mangle]
pub static mut HK_SHADE_KILLS: u32 = 0;
#[no_mangle]
pub static mut HK_SHADE_X: i32 = 0;
#[no_mangle]
pub static mut HK_SHADE_Y: i32 = 0;

/// Everything death records and a save has to carry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Record {
    pub present: bool,
    pub scene: u32,
    pub position: [i32; 2],
    pub hp: u16,
    pub geo_pool: u32,
}
struct Live {
    control: Shade,
    position: [i32; 2],
    hp: u16,
    clip: usize,
    age: u32,
    facing: i32,
    /// HealthManager invincibility between nail hits, as the actors use.
    hit_cooldown: u16,
    /// The body's bottom met terrain on the last step: the Quake's LAND.
    grounded: bool,
    /// `nail_clash_tink` answered this Slash already (one trigger entry a swing).
    parried: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Events {
    /// The x of whatever touched the hero body this tick (the Shade's body or
    /// Slash, a spell hit, the Shadow Ball): DamageHero's direction source.
    pub touched: Option<i32>,
    /// The Shade died: this much Geo returns to the wallet.
    pub returned_geo: u32,
}
pub struct World {
    record: Record,
    live: Option<Live>,
    /// The Shadow Ball flies on its own, so it outlives the Shade's death.
    ball: Option<ShadowBall>,
    seed: u32,
}
const HIT_COOLDOWN: u16 = 12;
/// Greenpath's Hornet fills her room's pool (her module, her trees and state, the death and respawn art), so no Shade is
/// born there, and none carried into it, while she lives: a Shade recorded in her arena waits for her defeat and appears on
/// the visit after it (docs/HORNET.md, "The Shade in her arena").
#[cfg(all(feature = "hornet", not(test)))]
fn waits_for_hornet(scene: usize) -> bool {
    scene == crate::enemies::hornet::HORNET_ART_SCENE && !crate::persist::player(crate::persist::HORNET_1_DEFEATED)
}
#[cfg(any(not(feature = "hornet"), test))]
fn waits_for_hornet(_scene: usize) -> bool {
    false
}
/// Source `shadeHealth` is clamp(maxHealth / 2, 1, 99) and the Shade's own HP
/// is `nailDamage` times that, which is the prefab's 10 at this port's values.
pub fn death_health(max_health: u16, nail_damage: u16) -> u16 {
    nail_damage.saturating_mul((max_health / 2).clamp(1, 99))
}
impl World {
    pub const fn new() -> Self {
        Self {
            record: Record {
                present: false,
                scene: 0,
                position: [0; 2],
                hp: 0,
                geo_pool: 0,
            },
            live: None,
            ball: None,
            seed: 0x5ade,
        }
    }
    pub fn record(&self) -> Record {
        self.record
    }
    pub fn restore(&mut self, record: Record) {
        self.record = record;
        self.live = None;
        self.publish();
    }
    /// The soul limiter runs exactly while a Shade is owed.
    /// Keep the Shade's art carried into the scene it waits in, so that
    /// scene's gate loads it; with no Shade waiting, into a boss fight's scene
    /// (`current`, a catalogue id): XA holds the drive for the whole fight,
    /// so a Shade born in it must find its art already in.
    #[cfg(not(test))]
    pub fn carry(&self, current: Option<usize>) {
        let scene = if self.record.present {
            Some(self.record.scene as usize)
        } else {
            current.filter(|_| crate::music::boss_active())
        };
        let scene = scene.filter(|&s| !waits_for_hornet(s));
        crate::modules::carry(
            crate::modules::ART_SHADE,
            scene.and_then(crate::disc::manifest_index),
        );
    }
    #[cfg(not(test))]
    /// Whether `carry` asks for the Shade's art into the scene `current` (`modules::fx_fit`'s light or full row).
    pub fn art_wanted(&self, current: usize) -> bool {
        (self.record.present && self.record.scene as usize == current) || crate::music::boss_active()
    }
    pub fn soul_limited(&self) -> bool {
        self.record.present
    }
    /// Hero Death Anim's Remove Geo and Set Shade, in that order. A second
    /// death overwrites the pool, so the earlier Shade's Geo is forfeit.
    pub fn record_death(&mut self, scene: usize, position: [i32; 2], hp: u16, wallet: u32) {
        self.record = Record {
            present: true,
            scene: scene as u32,
            position,
            hp,
            geo_pool: wallet,
        };
        self.live = None;
        self.publish();
    }
    /// SceneManager::Start instantiates the Shade when the scene matches;
    /// `spells` is what its `Init` reads from PlayerData.
    pub fn enter_scene(&mut self, scene: usize, spells: Spells) {
        self.ball = None;
        self.live = if self.record.present && self.record.scene as usize == scene && !waits_for_hornet(scene) {
            self.seed = NrLcg::step(self.seed);
            Some(Live {
                control: Shade::new(self.record.position, self.seed, spells),
                position: self.record.position,
                hp: self.record.hp,
                clip: CLIP_IDLE,
                age: 0,
                facing: -1,
                hit_cooldown: 0,
                grounded: false,
                parried: false,
            })
        } else {
            None
        };
        self.publish();
    }
    pub fn present_here(&self) -> bool {
        self.live.is_some()
    }
    /// The Shade, or a Shadow Ball it cast, is in this scene: `tick` has work.
    pub fn active_here(&self) -> bool {
        self.live.is_some() || self.ball.is_some()
    }
    pub fn position(&self) -> Option<[i32; 2]> {
        self.live.as_ref().map(|l| l.position)
    }
    fn body(position: [i32; 2]) -> [i32; 4] {
        [
            position[0] + BODY_BOUNDS[0],
            position[1] + BODY_BOUNDS[1],
            position[0] + BODY_BOUNDS[2],
            position[1] + BODY_BOUNDS[3],
        ]
    }
    /// A child's box, mirrored with the facing: the Shade's art faces left,
    /// so its localScale x is the opposite of `facing`.
    fn child_box(position: [i32; 2], facing: i32, local: [i32; 4]) -> [i32; 4] {
        mirrored(position, -facing, local)
    }
    fn overlap(a: [i32; 4], b: [i32; 4]) -> bool {
        a[0] <= b[2] && a[2] >= b[0] && a[1] <= b[3] && a[3] >= b[1]
    }
    /// `nail_clash_tink`: the nail entering the live Slash parries it, once a
    /// Slash. The caller recoils the Knight by the slash's direction.
    pub fn parry(&mut self, slash: &hk_sim::HeroHit) -> bool {
        let Some(live) = self.live.as_mut() else {
            return false;
        };
        if !live.control.slashing() || live.parried || !slash.hits(Self::child_box(live.position, live.facing, SLASH_BOUNDS)) {
            return false;
        }
        live.parried = true;
        true
    }
    /// One 60 Hz step against the scene terrain.
    #[cfg_attr(not(test), optimize(size))]
    pub fn tick(
        &mut self,
        hero: [i32; 2],
        hero_body: [i32; 4],
        count: usize,
        edge: impl Fn(usize) -> [i32; 4],
    ) -> Events {
        let mut events = Events::default();
        if let Some(ball) = self.ball.as_mut() {
            if !ball.tick() {
                self.ball = None;
            } else if Self::overlap(ball.bounds(BALL_BOUNDS), hero_body) {
                events.touched = Some(ball.position[0]);
            }
        }
        let Some(live) = self.live.as_mut() else {
            return events;
        };
        // DamageHero's direction is taken from where the Shade stood before it moved.
        let before = live.position[0];
        live.age = live.age.saturating_add(1);
        let dx = live.position[0].clamp(hero_body[0], hero_body[2]) - live.position[0];
        let dy = live.position[1].clamp(hero_body[1], hero_body[3]) - live.position[1];
        let in_range = hk_sim::fixed::within(dx, dy, ALERT_RADIUS as i32);
        let can_see_hero = in_range && !segment_hits_terrain(live.position, hero, count, &edge);
        let senses = Senses {
            position: live.position,
            hero,
            can_see_hero,
            quake_range: Self::overlap(Self::child_box(live.position, live.facing, QUAKE_RANGE), hero_body),
            scream_range: Self::overlap(Self::child_box(live.position, live.facing, SCREAM_RANGE), hero_body),
            grounded: live.grounded,
        };
        let actions = live.control.tick(senses);
        for action in actions.iter() {
            match action {
                Action::Play(pose, frame) => {
                    live.clip = pose_clip(pose);
                    live.age = frame;
                }
                Action::Facing(facing) => live.facing = facing,
                Action::MoveTo(position) => live.position = position,
                Action::Shoot(facing) => self.ball = Some(ShadowBall::new(live.position, facing)),
                Action::SlashOn => live.parried = false,
                Action::Velocity(_) | Action::SlashOff => {}
            }
        }
        let v = live.control.velocity();
        live.grounded = false;
        if v != [0; 2] && !live.control.tweening() {
            let mut body = Player::spawn(live.position[0], live.position[1]);
            let params = Params {
                speed: v[0].abs(),
                fall: 100 * ONE,
                half_width: (BODY_BOUNDS[2] - BODY_BOUNDS[0]) / 2,
                bottom: BODY_BOUNDS[1],
                top: BODY_BOUNDS[3],
                ..Params::ZERO
            };
            body.vy = v[1];
            body.step(params, v[0].signum(), false, count, &edge);
            live.position = [body.x, body.y];
            live.grounded = body.grounded;
        }
        if live.hit_cooldown > 0 {
            live.hit_cooldown -= 1;
        }
        let at = live.position;
        let touched = if live.control.vulnerable() {
            let box_ = if live.control.slashing() {
                Self::child_box(at, live.facing, SLASH_BOUNDS)
            } else {
                Self::body(at)
            };
            Self::overlap(box_, hero_body)
        } else {
            false
        };
        // Quake Hit and Scream Hit are their own DamageHero children.
        let spell = (live.control.quake_hitting() && Self::overlap(Self::child_box(at, live.facing, QUAKE_HIT), hero_body))
            || (live.control.scream_hitting() && {
                let c = [at[0] - SCREAM_HIT[0] * live.facing, at[1] + SCREAM_HIT[1]];
                let dx = c[0].clamp(hero_body[0], hero_body[2]) - c[0];
                let dy = c[1].clamp(hero_body[1], hero_body[3]) - c[1];
                hk_sim::fixed::within(dx, dy, SCREAM_HIT[2])
            });
        if touched || spell {
            events.touched = Some(before);
        }
        if live.control.phase() == Phase::Gone {
            events.returned_geo = self.record.geo_pool;
            self.record = Record::default();
            self.live = None;
            unsafe {
                HK_SHADE_KILLS = HK_SHADE_KILLS.saturating_add(1);
            }
        } else {
            self.record.position = live.position;
            self.record.hp = live.hp;
        }
        self.publish();
        events
    }
    /// A nail strike. Returns whether it connected.
    pub fn strike(&mut self, polygon: &[[i32; 2]], damage: u16) -> bool {
        let Some(live) = self.live.as_mut() else {
            return false;
        };
        if !live.control.vulnerable() || live.hit_cooldown != 0 {
            return false;
        }
        if !hk_sim::polygon_hits_box(polygon, Self::body(live.position)) {
            return false;
        }
        live.hit_cooldown = HIT_COOLDOWN;
        live.hp = live.hp.saturating_sub(damage);
        let position = live.position;
        let actions = if live.hp == 0 {
            // Death Start credits the pool; the caller adds it on the Gone tick.
            live.control.die()
        } else {
            live.control.took_damage(Senses {
                position,
                hero: position,
                can_see_hero: false,
                quake_range: false,
                scream_range: false,
                grounded: false,
            })
        };
        for action in actions.iter() {
            if let Action::Play(pose, frame) = action {
                live.clip = pose_clip(pose);
                live.age = frame;
            }
        }
        self.publish();
        true
    }
    fn publish(&self) {
        unsafe {
            HK_SHADE_PRESENT = u32::from(self.record.present);
            HK_SHADE_GEO_POOL = self.record.geo_pool;
            HK_SHADE_HP = self.record.hp as u32;
            HK_SHADE_X = self.record.position[0];
            HK_SHADE_Y = self.record.position[1];
        }
    }
    fn frame_index(&self) -> Option<usize> {
        let live = self.live.as_ref()?;
        let clip = SHADE_CLIPS[live.clip];
        let frame = hk_sim::fixed::mul_div_floor(u32::from(live.age), u32::from(clip.fps), 60) as usize;
        Some(
            clip.start
                + if clip.wrap == 0 {
                    frame % clip.count
                } else {
                    frame.min(clip.count - 1)
                },
        )
    }
    /// Append this frame's animation key, as the actors do.
    pub fn append_needed(&self, needed: &mut [u16], len: &mut usize) {
        // Not in the pool (a read failed or is still due): nothing to upload.
        // Asked only when a Shade is live, so a miss counts as late art
        // (modules::HK_MODULE_ART_LATE) and a frame without one does not.
        if let Some(index) = self.frame_index() {
            if data().is_none() {
                return;
            }
            assert!(*len < needed.len(), "animation working set exceeded");
            needed[*len] = KEY_BASE + index as u16;
            *len += 1;
        }
    }
}
fn pose_clip(pose: Pose) -> usize {
    match pose {
        Pose::Idle => CLIP_IDLE,
        Pose::Startle => CLIP_STARTLE,
        Pose::Fly => CLIP_FLY,
        Pose::TurnToFly => CLIP_TURNTOFLY,
        Pose::SlashAntic => CLIP_SLASH_ANTIC,
        Pose::Slash => CLIP_SLASH,
        Pose::SlashCd => CLIP_SLASH_CD,
        Pose::RetreatStart => CLIP_RETREAT_START,
        Pose::RetreatEnd => CLIP_RETREAT_END,
        Pose::DeathStart => CLIP_DEATH_START,
        Pose::Death => CLIP_DEATH,
        // The spell clips are not cooked (host/shade.py `limitations`): the
        // Shade holds Fly while it casts, dives and screams.
        Pose::CastAntic
        | Pose::CastCharge
        | Pose::Cast
        | Pose::QuakeAntic
        | Pose::QuakeStart
        | Pose::Quake
        | Pose::QuakeLand
        | Pose::ScreamAntic
        | Pose::Scream
        | Pose::ScreamEnd => CLIP_FLY,
    }
}
/// Texels for one Shade frame, for the animation cache's upload closure.
pub fn texels(index: usize) -> Option<(&'static [u8], u16, u16)> {
    let frame = SHADE_FRAMES.get(index)?;
    let start = PALETTE_BYTES + frame.offset;
    let len = (frame.width as usize + 3) / 4 * 2 * frame.height as usize;
    let data = data()?;
    #[cfg(not(test))]
    presentation::palettes(data);
    Some((data.get(start..start + len)?, frame.width, frame.height))
}
/// Straight-line terrain occlusion, as the flying actors use for sight.
fn segment_hits_terrain(
    a: [i32; 2],
    b: [i32; 2],
    count: usize,
    edge: &impl Fn(usize) -> [i32; 4],
) -> bool {
    let (ox, oy) = (a[0], a[1]);
    let (rx, ry) = (b[0] - ox, b[1] - oy);
    for i in 0..count {
        let e = edge(i);
        let (x0, y0, x1, y1) = (e[0], e[1], e[2], e[3]);
        let (sx, sy) = (x1 - x0, y1 - y0);
        let denominator = Wide::mul(rx, sy).mul_sub(ry, sx);
        if denominator.is_zero() {
            continue;
        }
        let t = Wide::mul(x0 - ox, sy).mul_sub(y0 - oy, sx);
        let u = Wide::mul(x0 - ox, ry).mul_sub(y0 - oy, rx);
        let (t, u) = if denominator.is_negative() { (-t, -u) } else { (t, u) };
        let denominator = denominator.abs();
        if !t.is_negative() && t <= denominator && !u.is_negative() && u <= denominator {
            return true;
        }
    }
    false
}

#[cfg(not(test))]
mod presentation {
    use super::*;
    use psx_gpu::{
        material::{BlendMode, TextureMaterial},
        prim::QuadTextured,
    };
    use psx_vram::{upload_bytes, Clut, VramRect};
    #[no_mangle]
    pub static mut HK_SHADE_DRAWN: u32 = 0;
    static mut PALETTES_UP: bool = false;
    /// One resident CLUT row per cooked palette, above the dialogue palette:
    /// uploaded the first time the Shade's art is in, and again after
    /// `upload` (boot, retry) forgets them.
    pub fn upload() {
        unsafe {
            PALETTES_UP = false;
        }
    }
    pub(super) fn palettes(data: &[u8]) {
        if unsafe { PALETTES_UP } || data.len() < PALETTE_BYTES {
            return;
        }
        for i in 0..PALETTE_COUNT {
            upload_bytes(
                VramRect::new(
                    CLUT_RECT.0,
                    CLUT_RECT.1 + i as u16,
                    CLUT_RECT.2,
                    CLUT_RECT.3,
                ),
                &data[i * 32..i * 32 + 32],
            );
        }
        unsafe {
            PALETTES_UP = true;
        }
    }
    impl World {
        #[inline(never)]
        pub fn draw(&self, camera: (i32, i32)) -> u32 {
            let (Some(index), Some(live)) = (self.frame_index(), self.live.as_ref()) else {
                return 0;
            };
            // Return Pause's SetMeshRenderer false, until Reform.
            if live.control.hidden() {
                return 0;
            }
            if data().is_none() {
                return 0;
            }
            let frame = SHADE_FRAMES[index];
            let (u, v) = crate::render::animation_uv(KEY_BASE + index as u16);
            let b = frame.bounds;
            let world = [[b[0], b[3]], [b[2], b[3]], [b[0], b[1]], [b[2], b[1]]];
            let vertices = world.map(|[x, y]| {
                // Every sprite here mirrors for facing +1 (position - x * facing): the art faces
                // left, and the Shade's facing -1 is left, like its slash box above.
                let x = live.position[0] - x * live.facing;
                let y = live.position[1] + y;
                (
                    160 + hk_sim::fixed::project(x, camera.0, crate::KNIGHT_SCALE as i32, 28),
                    120 - hk_sim::fixed::project(y, camera.1, crate::KNIGHT_SCALE as i32, 28),
                )
            });
            if vertices.iter().all(|p| p.0 < 0)
                || vertices.iter().all(|p| p.0 >= 320)
                || vertices.iter().all(|p| p.1 < 0)
                || vertices.iter().all(|p| p.1 >= 240)
            {
                return 0;
            }
            let right = (u16::from(u) + frame.width - 1) as u8;
            let bottom = (u16::from(v) + frame.height - 1) as u8;
            let clut = Clut::new(CLUT_RECT.0, CLUT_RECT.1 + frame.clut as u16).uv_word();
            let tpage = crate::render::animation_tpage_word(KEY_BASE + index as u16);
            let template = QuadTextured::with_material(
                [(0, 0); 4],
                [(u, v), (right, v), (u, bottom), (right, bottom)],
                TextureMaterial::blended(clut, tpage, (128, 128, 128), BlendMode::Average),
            );
            crate::render::resident_quad(&template, vertices.map(|(x, y)| (x as i16, y as i16)));
            unsafe {
                HK_SHADE_DRAWN = HK_SHADE_DRAWN.saturating_add(1);
            }
            1
        }
    }
}
#[cfg(not(test))]
pub use presentation::upload;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_recorded_health_follows_the_source_formula() {
        // maxHealth 5 and nailDamage 5 reproduce the prefab's HealthManager 10.
        assert_eq!(death_health(5, 5), SHADE_HP);
        assert_eq!(death_health(1, 5), 5, "clamped to at least one hit");
        assert_eq!(death_health(9, 5), 20);
    }
    #[test]
    fn a_second_death_overwrites_the_pool_and_the_place() {
        let mut w = World::new();
        assert!(!w.soul_limited());
        w.record_death(3, [ONE, 2 * ONE], 10, 41);
        assert!(w.soul_limited());
        w.record_death(7, [0, 0], 10, 0);
        assert_eq!(w.record().geo_pool, 0, "the earlier pool is forfeit");
        assert_eq!(w.record().scene, 7);
    }
    #[test]
    fn the_shade_only_spawns_in_the_scene_that_recorded_it() {
        let mut w = World::new();
        w.record_death(3, [ONE, 2 * ONE], 10, 41);
        w.enter_scene(4, Spells::default());
        assert!(!w.present_here());
        w.enter_scene(3, Spells::default());
        assert!(w.present_here());
    }
    #[test]
    fn every_clip_index_resolves_to_a_cooked_frame() {
        for clip in SHADE_CLIPS {
            assert!(clip.count > 0 && clip.start + clip.count <= SHADE_FRAMES.len());
        }
        for index in 0..SHADE_FRAMES.len() {
            let (bytes, w, h) = texels(index).expect("frame texels");
            assert_eq!(bytes.len(), (w as usize + 3) / 4 * 2 * h as usize);
            assert!(bytes.len() <= 2048, "a frame must fit one animation slot");
        }
    }
}
