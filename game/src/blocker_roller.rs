//! The Elder Baldur's spat Roller (`Spawn Roller v2`, host/blocker_roller.py).
//!
//! `Attack Choose` picks it half the time once PlayerData `fireballLevel` is
//! above 0 and none is out (`Can Roller?`): the Blocker's `Fire` flings it with
//! the goop's own launch, and it lands and rolls the way the Blocker faces,
//! then runs the Mound Baldur's `Roller` FSM (hk_sim::baldur) at Max Speed 14.
//! Three nail hits kill it (HealthManager 15 against nail damage 5), each
//! worth the ordinary SOUL of a nail hit, which is what refills the vessel
//! for the Blocker's fourth cast; touching it costs a mask (DamageHero 1).
//!
//! One at a time, in the Blocker's scene only; it goes with the scene. Its
//! frames are in the quick map's page with the Shaman's palette
//! (data/shaman_art.rs). Not presented: the roll dust and audio, the death
//! air/land/end clips (it is removed on the killing hit, with the ordinary
//! enemy death sound and kill shake), Recoil's vertical part, and the line of
//! sight half of `Idle`'s alert (the range box alone restarts a roll).
use hk_sim::{baldur, Params, Player, ONE};
use crate::shaman::{MapFrame, ROLLER_CLIPS, ROLLER_FRAMES};

/// `BoxCollider2D` of the prefab: offset (-0.015625, -0.109375), size 1.09375.
const HALF_WIDTH: i32 = 35840;
const BOTTOM: i32 = -42998;
const TOP: i32 = 28672;
const CENTRE_X: i32 = -1024;
/// `SetHP` 15 in `Initiate`.
const HP: u16 = 15;
/// Recoil: 25 units/s for .15 s.
const RECOIL_SPEED: i32 = 25 * ONE;
const RECOIL_TICKS: u16 = 9;
/// One payout per swing, as the shade and the enemies take it.
const HIT_COOLDOWN: u16 = 12;

#[derive(Clone, Copy)]
struct Roller {
    scene: usize,
    x: i32,
    y: i32,
    vx: i32,
    vy: i32,
    grounded: bool,
    wall: bool,
    facing: i32,
    controller: baldur::Baldur,
    clip: baldur::Clip,
    age: u32,
    hp: u16,
    cooldown: u16,
    recoil_left: u16,
    recoil_dir: i32,
}
static mut ROLLER: Option<Roller> = None;
static mut SEED: u32 = 0x5eed_0b1d;
#[no_mangle] pub static mut HK_ROLLERS_SPAWNED: u32 = 0;
#[no_mangle] pub static mut HK_ROLLERS_KILLED: u32 = 0;
#[no_mangle] pub static mut HK_ROLLER_HITS: u32 = 0;

fn me() -> &'static mut Option<Roller> { unsafe { &mut *(&raw mut ROLLER) } }
/// `Can Roller?`: the spell is had and no roller is out.
pub fn can_spawn() -> bool {
    crate::persist::store().levels[crate::persist::FIREBALL_LEVEL] > 0 && me().is_none()
}
/// `Fire` from the `Roller` branch.
pub fn spawn(scene: usize, at: [i32; 2], velocity: [i32; 2], moving_right: bool) {
    let seed = unsafe { SEED = SEED.wrapping_mul(1664525).wrapping_add(1013904223); SEED };
    *me() = Some(Roller { scene, x: at[0], y: at[1], vx: velocity[0], vy: velocity[1], grounded: false, wall: false,
        facing: if moving_right { 1 } else { -1 }, controller: baldur::Baldur::spawned(seed, moving_right),
        clip: baldur::Clip::Roll, age: 0, hp: HP, cooldown: 0, recoil_left: 0, recoil_dir: 0 });
    unsafe { HK_ROLLERS_SPAWNED = HK_ROLLERS_SPAWNED.wrapping_add(1); }
}
/// A scene left, a death or a reset: it does not outlive the room.
pub fn reset() { *me() = None; }
fn body(r: &Roller) -> [i32; 4] {
    [r.x + CENTRE_X - HALF_WIDTH, r.y + BOTTOM, r.x + CENTRE_X + HALF_WIDTH, r.y + TOP]
}
#[derive(Default)]
pub struct Events { pub hits: u8, pub killed: bool, pub touched: Option<i32> }
/// One simulation tick in `scene`: the FSM, the body against terrain, the
/// nail (`polygon` is this tick's swept nail when it is hitting, else None)
/// and the hero's body.
pub fn tick(scene: usize, hero: [i32; 2], hero_body: [i32; 4], polygon: Option<&[[i32; 2]]>, damage: u16,
            count: usize, edge: impl Fn(usize) -> [i32; 4]) -> Events {
    let mut events = Events::default();
    let Some(r) = me().as_mut() else { return events };
    if r.scene != scene { *me() = None; return events; }
    r.age = r.age.wrapping_add(1);
    r.cooldown = r.cooldown.saturating_sub(1);
    // Idle's ALERT: the hero body in `Alert Range New` (21.14 x 1.9 at y
    // +0.31, the Mound Baldur's box). CheckCanSeeHero's ray is not cast here.
    let alert = [r.x - 692715, r.y - 41943, r.x + 692715, r.y + 82575];
    let in_alert = alert[0] <= hero_body[2] && alert[2] >= hero_body[0] && alert[1] <= hero_body[3] && alert[3] >= hero_body[1];
    let senses = baldur::Senses { actor_x: r.x, hero_x: hero[0], can_see_hero: in_alert, wall: r.wall, grounded: r.grounded };
    for action in r.controller.tick(senses).iter() {
        match action {
            baldur::Action::VelocityX(v) => r.vx = v,
            baldur::Action::Velocity(v) => { r.vx = v[0]; r.vy = v[1]; r.grounded = false; }
            baldur::Action::Play(clip) => { r.clip = clip; r.age = 0; }
            baldur::Action::Facing(f) => r.facing = f,
        }
    }
    let mut player = Player::spawn(r.x + CENTRE_X, r.y);
    player.vy = r.vy;
    player.grounded = r.grounded;
    let mut p = Params { speed: r.vx.abs(), gravity: 48 * ONE, fall: 100 * ONE, half_width: HALF_WIDTH,
        bottom: BOTTOM, top: TOP, ..Params::ZERO };
    let old_x = player.x;
    player.step(p, r.vx.signum(), false, count, &edge);
    r.wall = r.vx != 0 && player.x != old_x + r.vx / 60;
    if r.recoil_left != 0 {
        r.recoil_left -= 1;
        p.speed = RECOIL_SPEED;
        p.gravity = 0;
        player.vy = 0;
        player.step(p, r.recoil_dir, false, count, &edge);
    }
    r.vy = player.vy;
    r.grounded = player.grounded;
    r.x = player.x - CENTRE_X;
    r.y = player.y;
    let b = body(r);
    if let Some(polygon) = polygon {
        if r.cooldown == 0 && hk_sim::polygon_hits_box(polygon, b) {
            r.cooldown = HIT_COOLDOWN;
            r.hp = r.hp.saturating_sub(damage);
            events.hits = 1;
            r.recoil_dir = if hero[0] < r.x { 1 } else { -1 };
            r.recoil_left = RECOIL_TICKS;
            for action in r.controller.horizontal_recoil().iter() {
                if let baldur::Action::VelocityX(v) = action { r.vx = v; }
            }
            unsafe { HK_ROLLER_HITS = HK_ROLLER_HITS.wrapping_add(1); }
            if r.hp == 0 {
                events.killed = true;
                *me() = None;
                unsafe { HK_ROLLERS_KILLED = HK_ROLLERS_KILLED.wrapping_add(1); }
                return events;
            }
        }
    }
    if b[0] <= hero_body[2] && b[2] >= hero_body[0] && b[1] <= hero_body[3] && b[3] >= hero_body[1] {
        events.touched = Some(if hero[0] < r.x { -1 } else { 1 });
    }
    events
}
fn frame() -> Option<(usize, [i32; 2], i32)> {
    let r = me().as_ref()?;
    let clip = &ROLLER_CLIPS[match r.clip { baldur::Clip::Idle => 0, baldur::Clip::Start => 1, baldur::Clip::Roll => 2, baldur::Clip::Stop => 3 }];
    let f = (u64::from(r.age) * u64::from(clip.fps) / 60) as usize;
    Some((clip.start + if clip.wrap == 0 { f % clip.count } else { f.min(clip.count - 1) }, [r.x, r.y], r.facing))
}
/// Draw it from the map page, mirrored when it faces right (the art faces left).
#[inline(never)]
pub fn draw(camera: (i32, i32)) -> u32 {
    use psx_gpu::{material::{BlendMode, TextureMaterial}, prim::QuadTextured};
    use psx_vram::{Clut, TexDepth, Tpage};
    use crate::game_map::{MAP_CLUT_XY, MAP_PAGE_XY};
    if !crate::game_map::ready() { return 0; }
    let Some((index, at, facing)) = frame() else { return 0 };
    let f: &MapFrame = &ROLLER_FRAMES[index];
    let b = f.bounds;
    let world = [[b[0], b[3]], [b[2], b[3]], [b[0], b[1]], [b[2], b[1]]];
    let vertices = world.map(|[wx, wy]| {
        let (px, py) = (at[0] - wx * facing, at[1] + wy);
        ((160 + (((i64::from(px) - i64::from(camera.0)) * i64::from(crate::KNIGHT_SCALE)) >> 28)) as i32,
         (120 - (((i64::from(py) - i64::from(camera.1)) * i64::from(crate::KNIGHT_SCALE)) >> 28)) as i32)
    });
    if vertices.iter().all(|p| p.0 < 0) || vertices.iter().all(|p| p.0 >= 320)
        || vertices.iter().all(|p| p.1 < 0) || vertices.iter().all(|p| p.1 >= 240) { return 0; }
    let (u, v) = (f.u as u8, f.v as u8);
    let (right, bottom) = ((f.u + f.w - 1) as u8, (f.v + f.h - 1) as u8);
    let clut = Clut::new(MAP_CLUT_XY[f.clut].0, MAP_CLUT_XY[f.clut].1).uv_clut_word();
    let tpage = Tpage::new(MAP_PAGE_XY.0, MAP_PAGE_XY.1, TexDepth::Bit4).uv_tpage_word(0);
    let template = QuadTextured::with_material([(0, 0); 4], [(u, v), (right, v), (u, bottom), (right, bottom)],
        TextureMaterial::blended(clut, tpage, (128, 128, 128), BlendMode::Average));
    crate::render::resident_quad(&template, vertices.map(|(x, y)| (x as i16, y as i16)));
    1
}
