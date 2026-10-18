//! Nail-struck switches, and the gates, pillars and toll gates they open.
//!
//! The source runs each of them as a PlayMaker FSM; `hk_sim::switches` holds the same machines as
//! plain tables (states, entry actions, waits, events), and this is the runtime that plays them:
//! it owns what the tables leave to the caller, which is the hit test of the nail against a
//! switch's trigger box, the camera shake, the persisted `Activated` bools, the terrain edges of
//! a gate (lifted when its machine turns the collider off, the way `battle_gates.rs` lifts an
//! open arena gate's) and the art. `host/hk-cook/src/interactables.rs` is the cooker.
//!
//! A scene load recreates every machine from the store: a switch that was struck starts in its
//! activated state, where it tells its targets so (`ACTIVATE AGAIN`, then `ACTIVATE`), and a
//! pillar that finished opening starts open. Nothing here is per-frame work outside the scenes
//! that hold a switch or a gate.
use crate::persist::{self, Kind};
use hk_format::Room;
use hk_sim::switches::{
    Action, Actions, Event, Instance, Machine, Role, METAL_GATE, PILLAR_CONTROL, ROLES, SWITCH_CONTROL, TOLL_GATE,
    TOLL_SWITCH,
};

#[derive(Clone, Copy)]
pub struct Switch {
    pub scene: u8,
    pub local: u8,
    pub machine: u8,
    pub flip: u8,
    pub pos: [i32; 2],
    pub hit: [i32; 4],
    pub clips: [u16; ROLES],
    pub frame: u8,
    pub links: u16,
    pub link_count: u8,
}
#[derive(Clone, Copy)]
pub struct Target {
    pub scene: u8,
    pub local: u8,
    pub machine: u8,
    pub flip: u8,
    pub pos: [i32; 2],
    pub clips: [u16; ROLES],
}
#[derive(Clone, Copy)]
pub struct Clip {
    pub first: u16,
    pub count: u8,
    pub fps: u8,
    pub looping: bool,
}
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/interactables.rs"));

/// Nail swings that struck a switch, and the targets whose collider is off right now (one bit each,
/// in their scene's order): what a route reads.
#[no_mangle]
pub static mut HK_SWITCH_HITS: u32 = 0;
#[no_mangle]
pub static mut HK_INTERACT_OPEN: u32 = 0;
/// Pieces drawn, and pieces left out for want of an animation slot (must stay zero: a gate that
/// drops out of a frame is pop-in).
#[no_mangle]
pub static mut HK_INTERACT_DRAWS: u32 = 0;
#[no_mangle]
pub static mut HK_INTERACT_ART_DROPPED: u32 = 0;

const NO_ROLE: u8 = u8::MAX;
const MS: usize = SCENE_SWITCHES;
const MT: usize = SCENE_TARGETS;

#[derive(Clone, Copy)]
struct Live {
    inst: Instance,
    /// The role of the clip the machine last started, and the scene clock when it did.
    role: u8,
    start: u32,
    /// The collider is off (the target's edges are lifted).
    off: bool,
    /// The art is gone.
    gone: bool,
}

struct Scene {
    scene: usize,
    sw_first: usize,
    sw_count: usize,
    tg_first: usize,
    tg_count: usize,
    clock: u32,
    swing: [u32; MS],
    sw: [Option<Live>; MS],
    tg: [Option<Live>; MT],
}

static mut NOW: Scene = Scene {
    scene: usize::MAX,
    sw_first: 0,
    sw_count: 0,
    tg_first: 0,
    tg_count: 0,
    clock: 0,
    swing: [u32::MAX; MS],
    sw: [None; MS],
    tg: [None; MT],
};

fn now() -> &'static mut Scene {
    unsafe { &mut *(&raw mut NOW) }
}

fn machine_of(id: u8) -> &'static Machine {
    match id {
        0 => &TOLL_SWITCH,
        1 => &SWITCH_CONTROL,
        2 => &TOLL_GATE,
        3 => &PILLAR_CONTROL,
        _ => &METAL_GATE,
    }
}

/// Ticks each role's clip plays for, from the cooked clips (a looping clip never ends).
fn clip_ticks(clips: &[u16; ROLES]) -> [u16; ROLES] {
    let mut out = [0; ROLES];
    for (role, &clip) in clips.iter().enumerate() {
        if clip != NO_CLIP {
            let c = &CLIPS[clip as usize];
            out[role] = if c.looping { u16::MAX } else { (c.count as u32 * 60).div_ceil(c.fps.max(1) as u32) as u16 };
        }
    }
    out
}

/// Forget the scene: the next tick or edge query builds it again from the store. Called when a
/// save is loaded or the session is reset, which a scene id alone does not tell.
pub fn invalidate() {
    now().scene = usize::MAX;
}

/// Build a scene's machines as a load does.
#[inline(never)]
fn enter(scene: usize) {
    let sw_first = SWITCHES.partition_point(|s| (s.scene as usize) < scene);
    let sw_count = SWITCHES[sw_first..].iter().take_while(|s| s.scene as usize == scene).count().min(MS);
    let tg_first = TARGETS.partition_point(|t| (t.scene as usize) < scene);
    let tg_count = TARGETS[tg_first..].iter().take_while(|t| t.scene as usize == scene).count().min(MT);
    *now() = Scene {
        scene,
        sw_first,
        sw_count,
        tg_first,
        tg_count,
        clock: 0,
        swing: [u32::MAX; MS],
        sw: [None; MS],
        tg: [None; MT],
    };
    unsafe { HK_INTERACT_OPEN = 0 }
    for i in 0..tg_count {
        let t = &TARGETS[tg_first + i];
        let restored = t.local != 255 && persist::get(Kind::Switch, scene, t.local as usize).is_some();
        let (inst, out) = Instance::new(machine_of(t.machine), clip_ticks(&t.clips), restored);
        now().tg[i] = Some(Live { inst, role: NO_ROLE, start: 0, off: false, gone: false });
        act_target(i, out);
    }
    for i in 0..sw_count {
        let s = &SWITCHES[sw_first + i];
        let restored = persist::get(Kind::Switch, scene, s.local as usize).is_some();
        let (inst, out) = Instance::new(machine_of(s.machine), clip_ticks(&s.clips), restored);
        now().sw[i] = Some(Live { inst, role: NO_ROLE, start: 0, off: false, gone: false });
        act_switch(i, out);
    }
}

fn ensure(scene: usize) {
    if now().scene != scene {
        enter(scene);
    }
}

/// What a target's machine asked of the world.
fn act_target(i: usize, out: Actions) {
    let n = now();
    let scene = n.scene;
    let t = &TARGETS[n.tg_first + i];
    for action in out.iter() {
        let Some(live) = n.tg[i].as_mut() else { return };
        match action {
            Action::Clip(role) => {
                live.role = role as u8;
                live.start = n.clock;
            }
            Action::Persist => {
                if t.local != 255 {
                    persist::set(Kind::Switch, scene, t.local as usize, 1);
                }
            }
            Action::Shake => crate::camera::request(crate::camera::Shake::Kill),
            Action::ColliderOff => lift(live, i),
            Action::Destroy => {
                live.gone = true;
                lift(live, i);
            }
            Action::Hide => live.gone = true,
            Action::Send(_) => {}
        }
    }
}

fn lift(live: &mut Live, i: usize) {
    if !live.off {
        live.off = true;
        unsafe { HK_INTERACT_OPEN |= 1 << i }
        crate::world::scripted_terrain_changed();
    }
}

/// What a switch's machine asked of the world: its events reach its targets at once, as PlayMaker's
/// `SendEventByName` does.
fn act_switch(i: usize, out: Actions) {
    let n = now();
    let scene = n.scene;
    let s = &SWITCHES[n.sw_first + i];
    for action in out.iter() {
        let Some(live) = n.sw[i].as_mut() else { return };
        match action {
            Action::Clip(role) => {
                live.role = role as u8;
                live.start = n.clock;
            }
            Action::Persist => persist::set(Kind::Switch, scene, s.local as usize, 1),
            Action::Shake => crate::camera::request(crate::camera::Shake::Kill),
            Action::Send(event) => {
                for l in 0..s.link_count as usize {
                    let target = LINKS[s.links as usize + l] as usize;
                    let mut reply = Actions::none();
                    if let Some(t) = now().tg[target].as_mut() {
                        t.inst.event(event, &mut reply);
                    }
                    act_target(target, reply);
                }
            }
            Action::ColliderOff | Action::Destroy | Action::Hide => {}
        }
    }
}

/// One simulation tick of the current scene's machines. A new scene is built first.
#[inline(never)]
pub fn tick(scene: usize) {
    ensure(scene);
    let n = now();
    if n.sw_count == 0 && n.tg_count == 0 {
        return;
    }
    n.clock = n.clock.wrapping_add(1);
    for i in 0..n.sw_count {
        let mut out = Actions::none();
        if let Some(live) = now().sw[i].as_mut() {
            live.inst.tick(&mut out);
        }
        act_switch(i, out);
    }
    for i in 0..now().tg_count {
        let mut out = Actions::none();
        if let Some(live) = now().tg[i].as_mut() {
            live.inst.tick(&mut out);
        }
        act_target(i, out);
    }
}

/// One nail swing's polygon against the scene's switches; `swing` is the attack counter, so one
/// swing strikes a switch once however long it overlaps.
#[inline(never)]
pub fn strike(scene: usize, polygon: &[[i32; 2]], swing: u32) {
    ensure(scene);
    for i in 0..now().sw_count {
        let s = &SWITCHES[now().sw_first + i];
        if now().swing[i] == swing || !hk_sim::polygon_hits_box(polygon, s.hit) {
            continue;
        }
        now().swing[i] = swing;
        let mut out = Actions::none();
        if let Some(live) = now().sw[i].as_mut() {
            live.inst.event(Event::NailHit, &mut out);
        }
        if out.iter().next().is_some() {
            unsafe { HK_SWITCH_HITS = HK_SWITCH_HITS.wrapping_add(1) }
        }
        act_switch(i, out);
    }
}

/// Lift the terrain of every target whose collider is off in `region`'s view (after the
/// Lifeblood refresh, beside the gates' and the Blockers').
pub fn apply(state: &mut crate::world::State, region: usize) {
    let first = EDGES.partition_point(|&(slot, ..)| (slot as usize) < region);
    for &(slot, target, from, count) in &EDGES[first..] {
        if slot as usize != region {
            break;
        }
        ensure(TARGETS[target as usize].scene as usize);
        let n = now();
        let k = (target as usize).wrapping_sub(n.tg_first);
        if k < n.tg_count && n.tg[k].is_some_and(|l| l.off) {
            state.append_script_edges(&EDGE_LIST[from as usize..from as usize + count as usize]);
        }
    }
}

// ------------------------------------------------------------------ art

/// A piece is one of the sprite's tiles; at most four per instance on screen.
const MAX_DRAWS: usize = (MS + MT) * 4;
/// Half a screen at KNIGHT_SCALE plus half a pillar.
const REACH_X: i32 = 14 * 65536;
const REACH_Y: i32 = 12 * 65536;

/// The pieces one frame draws and each sheet's texture.
pub struct Draws {
    count: usize,
    texture: [u16; 32],
    origin: [[i32; 2]; MAX_DRAWS],
    flip: [u8; MAX_DRAWS],
    piece: [u16; MAX_DRAWS],
}

impl Draws {
    pub const NONE: Self = Self {
        count: 0,
        texture: [0; 32],
        origin: [[0; 2]; MAX_DRAWS],
        flip: [0; MAX_DRAWS],
        piece: [0; MAX_DRAWS],
    };
}

/// The sprite a machine's clip shows at the scene clock.
fn sprite_of(live: &Live, clips: &[u16; ROLES], hold: Option<u8>, clock: u32) -> Option<usize> {
    if live.gone || live.role == NO_ROLE {
        return None;
    }
    let clip = clips[live.role as usize];
    if clip == NO_CLIP {
        return None;
    }
    let c = &CLIPS[clip as usize];
    let count = c.count as usize;
    let step = match hold {
        Some(frame) => (frame as usize).min(count - 1),
        None => {
            let t = (clock.wrapping_sub(live.start)).min(1 << 16) * c.fps as u32 / 60;
            if c.looping {
                t as usize % count
            } else {
                (t as usize).min(count - 1)
            }
        }
    };
    Some(CLIP_SPRITES[c.first as usize + step] as usize)
}

fn view_frame(region: usize) -> Option<usize> {
    ART.binary_search_by_key(&(region as u16), |&(slot, _)| slot).ok().map(|i| ART[i].1 as usize)
}

/// Pick the pieces on screen and append each sheet they use once.
#[inline(never)]
pub fn prepare(room: &Room, region: usize, scene: usize, camera: (i32, i32), needed: &mut [u16], len: &mut usize) -> Draws {
    let mut draws = Draws::NONE;
    let n = now();
    if n.scene != scene || (n.sw_count == 0 && n.tg_count == 0) {
        return draws;
    }
    let Some(first) = view_frame(region) else {
        return draws;
    };
    let mut push = |draws: &mut Draws, pos: [i32; 2], flip: u8, sprite: usize| {
        if (pos[0] - camera.0).abs() > REACH_X || (pos[1] - camera.1).abs() > REACH_Y {
            return;
        }
        let (from, count) = SPRITES[sprite];
        for p in from as usize..from as usize + count as usize {
            if draws.count < MAX_DRAWS {
                draws.origin[draws.count] = pos;
                draws.flip[draws.count] = flip;
                draws.piece[draws.count] = p as u16;
                draws.count += 1;
            }
        }
    };
    for i in 0..n.sw_count {
        let s = &SWITCHES[n.sw_first + i];
        let hold = (s.machine == 0 && n.sw[i].is_some_and(|l| l.role == Role::Activated as u8)).then_some(s.frame);
        if let Some(sprite) = n.sw[i].as_ref().and_then(|l| sprite_of(l, &s.clips, hold, n.clock)) {
            push(&mut draws, s.pos, s.flip, sprite);
        }
    }
    for i in 0..n.tg_count {
        let t = &TARGETS[n.tg_first + i];
        if let Some(sprite) = n.tg[i].as_ref().and_then(|l| sprite_of(l, &t.clips, None, n.clock)) {
            push(&mut draws, t.pos, t.flip, sprite);
        }
    }
    // Each sheet a drawn piece sits on, once; a sheet with no room drops its pieces (counted)
    // rather than draw from a slot nobody bound.
    let mut used = 0u32;
    for i in 0..draws.count {
        used |= 1 << PIECES[draws.piece[i] as usize].0;
    }
    let mut kept = 0u32;
    for sheet in 0..32 {
        if used & (1 << sheet) == 0 {
            continue;
        }
        let key = room.frame_grid(first + sheet).0 as u16;
        draws.texture[sheet] = key;
        if needed[..*len].contains(&key) {
            kept |= 1 << sheet;
            continue;
        }
        if *len < needed.len() {
            needed[*len] = key;
            *len += 1;
            kept |= 1 << sheet;
        }
    }
    if kept != used {
        let mut m = 0;
        for i in 0..draws.count {
            if kept & (1 << PIECES[draws.piece[i] as usize].0) != 0 {
                draws.origin[m] = draws.origin[i];
                draws.flip[m] = draws.flip[i];
                draws.piece[m] = draws.piece[i];
                m += 1;
            }
        }
        unsafe { HK_INTERACT_ART_DROPPED = HK_INTERACT_ART_DROPPED.wrapping_add((draws.count - m) as u32) }
        draws.count = m;
    }
    draws
}

impl Draws {
    /// The pieces on the gameplay plane, untinted.
    #[inline(never)]
    pub fn draw(&self, camera: (i32, i32)) -> u32 {
        for i in 0..self.count {
            let p = self.origin[i];
            let (sheet, rect, b) = PIECES[self.piece[i] as usize];
            let fx = if self.flip[i] & 1 != 0 { -1 } else { 1 };
            let fy = if self.flip[i] & 2 != 0 { -1 } else { 1 };
            let coords = [(b[0], b[3]), (b[2], b[3]), (b[0], b[1]), (b[2], b[1])];
            let mut v = [(0i16, 0i16); 4];
            for (k, (x, y)) in coords.iter().enumerate() {
                v[k] = (
                    (160 + ((((p[0] + x * fx - camera.0) >> 8) * crate::KNIGHT_SCALE) >> 20)) as i16,
                    (120 - ((((p[1] + y * fy - camera.1) >> 8) * crate::KNIGHT_SCALE) >> 20)) as i16,
                );
            }
            crate::render::texture_sub(self.texture[sheet as usize] as usize, v, (128, 128, 128), rect);
        }
        unsafe { HK_INTERACT_DRAWS = HK_INTERACT_DRAWS.wrapping_add(self.count as u32) }
        self.count as u32
    }
}
