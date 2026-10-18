//! Nail-struck switches, the gates, pillars and toll gates they open, and the stag station's props.
//!
//! The source runs each of them as a PlayMaker FSM; `hk_sim::switches` holds the same machines as
//! plain tables (states, entry actions, waits, events), and this is the runtime that plays them:
//! it owns what the tables leave to the caller, which is the hit test of the nail against an
//! object's trigger box, the camera shake, the persisted `Activated` bools, the terrain edges of a
//! gate (lifted when its machine turns the collider off, the way `battle_gates.rs` lifts an open
//! arena gate's) and the art. `host/hk-cook/src/interactables.rs` is the cooker.
//!
//! A scene load recreates every machine from the store: a switch that was struck starts in its
//! activated state, where it tells its targets so (`ACTIVATE AGAIN`, then `ACTIVATE`), a pillar that
//! finished opening starts open, and the stag station's props start from the stag network's state
//! (`stag.rs`). Nothing here is per-frame work outside the scenes that hold such an object.
use crate::persist::{self, Kind};
use hk_format::Room;
use hk_sim::switches::{
    Action, Actions, Event, Instance, Machine, Role, METAL_GATE, PILLAR_CONTROL, ROLES, STAG_ARRIVAL, STAG_BELL, STAG_BELL_BOX,
    STAG_BODY, STAG_GRATE, STATIC_PROP, SWITCH_CONTROL, TOLL_GATE, TOLL_SWITCH,
};

/// The machines, by the number the cook gives them (host/hk-cook/src/interactables.rs).
pub const M_STAG_BELL_BOX: u8 = 6;
pub const M_STAG_BODY: u8 = 8;

#[derive(Clone, Copy)]
pub struct Obj {
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
pub struct Clip {
    pub first: u16,
    pub count: u8,
    pub fps: u8,
    pub looping: bool,
}
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/interactables.rs"));

/// Nail swings that struck a switch or the bell, and the objects whose collider is off right now (one
/// bit each, in their scene's order): what a route reads.
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
const MO: usize = SCENE_OBJECTS;
/// Sheets one scene may carry (host/hk-cook interactables.rs `MAX_SHEETS`).
const SHEETS: usize = 128;

#[derive(Clone, Copy)]
struct Live {
    inst: Instance,
    /// The role of the clip the machine last started, and the scene clock when it did.
    role: u8,
    start: u32,
    /// The collider is off (the object's edges are lifted).
    off: bool,
    /// The art is gone.
    gone: bool,
}

struct Scene {
    scene: usize,
    first: usize,
    count: usize,
    clock: u32,
    swing: [u32; MO],
    live: [Option<Live>; MO],
}

static mut NOW: Scene = Scene { scene: usize::MAX, first: 0, count: 0, clock: 0, swing: [u32::MAX; MO], live: [None; MO] };

fn now() -> &'static mut Scene {
    unsafe { &mut *(&raw mut NOW) }
}

fn machine_of(id: u8) -> &'static Machine {
    match id {
        0 => &TOLL_SWITCH,
        1 => &SWITCH_CONTROL,
        2 => &TOLL_GATE,
        3 => &PILLAR_CONTROL,
        4 => &METAL_GATE,
        5 => &STAG_GRATE,
        6 => &STAG_BELL_BOX,
        7 => &STAG_BELL,
        8 => &STAG_BODY,
        9 => &STAG_ARRIVAL,
        _ => &STATIC_PROP,
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

/// Whether an object loads in its restored state: a switch or pillar by its persisted `Activated`, a
/// stag station's prop by the stag network (`stag.rs`).
fn restored(o: &Obj, scene: usize) -> bool {
    if o.local != 255 {
        return persist::get(Kind::Switch, scene, o.local as usize).is_some();
    }
    match o.machine {
        5 | 6 | 7 => crate::stag::open(scene),
        8 | 9 => crate::stag::here(scene),
        _ => false,
    }
}

/// Build a scene's machines as a load does.
#[inline(never)]
fn enter(scene: usize) {
    let first = OBJECTS.partition_point(|o| (o.scene as usize) < scene);
    let count = OBJECTS[first..].iter().take_while(|o| o.scene as usize == scene).count().min(MO);
    *now() = Scene { scene, first, count, clock: 0, swing: [u32::MAX; MO], live: [None; MO] };
    unsafe { HK_INTERACT_OPEN = 0 }
    // Every machine exists before any entry action runs, and the entry actions that send events run
    // after the ones that do not (a switch loaded activated tells its gate, whose own entry has to have
    // happened first, or the gate would be put back in its idle pose over the answer).
    let mut entries = [Actions::none(); MO];
    for i in 0..count {
        let o = &OBJECTS[first + i];
        let (inst, out) = Instance::new(machine_of(o.machine), clip_ticks(&o.clips), restored(o, scene));
        now().live[i] = Some(Live { inst, role: NO_ROLE, start: 0, off: false, gone: false });
        entries[i] = out;
    }
    for sends in [false, true] {
        for i in 0..count {
            act(i, entries[i], sends);
        }
    }
}

fn ensure(scene: usize) {
    if now().scene != scene {
        enter(scene);
    }
}

/// What an object's machine asked of the world: its events reach its links at once, as
/// PlayMaker's `SendEventByName` does. `only` picks the sends or everything else, which the load
/// orders (see `enter`); a running machine does both through `run`.
fn act(i: usize, out: Actions, only: bool) {
    let n = now();
    let scene = n.scene;
    let o = &OBJECTS[n.first + i];
    for action in out.iter() {
        if only != matches!(action, Action::Send(_)) {
            continue;
        }
        let Some(live) = now().live[i].as_mut() else { return };
        match action {
            Action::Clip(role) => {
                live.role = role as u8;
                live.start = now().clock;
            }
            Action::Persist => {
                if o.local != 255 {
                    persist::set(Kind::Switch, scene, o.local as usize, 1);
                }
            }
            Action::Shake => crate::camera::request(crate::camera::Shake::Kill),
            Action::ColliderOff => lift(live, i),
            Action::Destroy => {
                live.gone = true;
                lift(live, i);
            }
            Action::Hide => live.gone = true,
            Action::Show => live.gone = false,
            Action::Arrive => crate::stag::arrive(scene),
            Action::Send(event) => {
                for l in 0..o.link_count as usize {
                    send(LINKS[o.links as usize + l] as usize, event);
                }
            }
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

/// Deliver an event to one object of the scene.
fn send(i: usize, event: Event) {
    let mut reply = Actions::none();
    if let Some(t) = now().live.get_mut(i).and_then(Option::as_mut) {
        t.inst.event(event, &mut reply);
    }
    run(i, reply);
}

/// Everything a machine asked for, the plain actions first.
fn run(i: usize, out: Actions) {
    act(i, out, false);
    act(i, out, true);
}

/// Deliver an event to every object of `machine` in `scene` (the stag module's `Paid`).
pub fn send_to(scene: usize, machine: u8, event: Event) {
    ensure(scene);
    for i in 0..now().count {
        if OBJECTS[now().first + i].machine == machine {
            send(i, event);
        }
    }
}

/// The stag stands idle in `scene`: it can be talked to.
pub fn stag_ready(scene: usize) -> bool {
    let n = now();
    n.scene == scene
        && (0..n.count).any(|i| {
            OBJECTS[n.first + i].machine == M_STAG_BODY
                && n.live[i].is_some_and(|l| !l.gone && l.role == Role::Idle as u8)
        })
}

/// One simulation tick of the current scene's machines. A new scene is built first.
#[inline(never)]
pub fn tick(scene: usize) {
    ensure(scene);
    if now().count == 0 {
        return;
    }
    now().clock = now().clock.wrapping_add(1);
    for i in 0..now().count {
        let mut out = Actions::none();
        if let Some(live) = now().live[i].as_mut() {
            live.inst.tick(&mut out);
        }
        run(i, out);
    }
}

/// One nail swing's polygon against the scene's switches and bell; `swing` is the attack counter, so
/// one swing strikes an object once however long it overlaps.
#[inline(never)]
pub fn strike(scene: usize, polygon: &[[i32; 2]], swing: u32) {
    ensure(scene);
    for i in 0..now().count {
        let o = &OBJECTS[now().first + i];
        if o.hit == [0; 4] || now().swing[i] == swing || !hk_sim::polygon_hits_box(polygon, o.hit) {
            continue;
        }
        now().swing[i] = swing;
        let mut out = Actions::none();
        if let Some(live) = now().live[i].as_mut() {
            // A bell that is not standing yet, or hidden, has nothing for the nail to ring.
            if !live.gone {
                live.inst.event(Event::NailHit, &mut out);
            }
        }
        if out.iter().next().is_some() {
            unsafe { HK_SWITCH_HITS = HK_SWITCH_HITS.wrapping_add(1) }
        }
        run(i, out);
    }
}

/// Lift the terrain of every object whose collider is off in `region`'s view (after the
/// Lifeblood refresh, beside the gates' and the Blockers').
pub fn apply(state: &mut crate::world::State, region: usize) {
    let first = EDGES.partition_point(|&(slot, ..)| (slot as usize) < region);
    for &(slot, object, from, count) in &EDGES[first..] {
        if slot as usize != region {
            break;
        }
        ensure(OBJECTS[object as usize].scene as usize);
        let n = now();
        let k = (object as usize).wrapping_sub(n.first);
        if k < n.count && n.live[k].is_some_and(|l| l.off) {
            state.append_script_edges(&EDGE_LIST[from as usize..from as usize + count as usize]);
        }
    }
}

// ------------------------------------------------------------------ art

/// Pieces one frame may draw: the stag alone is four, the rest of a scene's props a handful.
const MAX_DRAWS: usize = MO * 6;
/// Half a screen at KNIGHT_SCALE plus half a pillar.
const REACH_X: i32 = 14 * 65536;
const REACH_Y: i32 = 12 * 65536;

/// The pieces one frame draws and each sheet's texture.
pub struct Draws {
    count: usize,
    texture: [u16; SHEETS],
    origin: [[i32; 2]; MAX_DRAWS],
    flip: [u8; MAX_DRAWS],
    piece: [u16; MAX_DRAWS],
}

impl Draws {
    pub const NONE: Self = Self {
        count: 0,
        texture: [0; SHEETS],
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
    if n.scene != scene || n.count == 0 {
        return draws;
    }
    let Some(first) = view_frame(region) else {
        return draws;
    };
    for i in 0..n.count {
        let o = &OBJECTS[n.first + i];
        if (o.pos[0] - camera.0).abs() > REACH_X || (o.pos[1] - camera.1).abs() > REACH_Y {
            continue;
        }
        let hold = (o.machine == 0 && n.live[i].is_some_and(|l| l.role == Role::Activated as u8)).then_some(o.frame);
        let Some(sprite) = n.live[i].as_ref().and_then(|l| sprite_of(l, &o.clips, hold, n.clock)) else {
            continue;
        };
        let (from, count) = SPRITES[sprite];
        for p in from as usize..from as usize + count as usize {
            if draws.count < MAX_DRAWS {
                draws.origin[draws.count] = o.pos;
                draws.flip[draws.count] = o.flip;
                draws.piece[draws.count] = p as u16;
                draws.count += 1;
            }
        }
    }
    // Each sheet a drawn piece sits on, once; a sheet with no room drops its pieces (counted) rather
    // than draw from a slot nobody bound.
    let mut used = [0u32; SHEETS / 32];
    for i in 0..draws.count {
        let sheet = PIECES[draws.piece[i] as usize].0 as usize;
        used[sheet / 32] |= 1 << (sheet % 32);
    }
    let mut dropped = [0u32; SHEETS / 32];
    for sheet in 0..SHEETS {
        if used[sheet / 32] & (1 << (sheet % 32)) == 0 {
            continue;
        }
        let key = room.frame_grid(first + sheet).0 as u16;
        draws.texture[sheet] = key;
        if needed[..*len].contains(&key) {
            continue;
        }
        if *len < needed.len() {
            needed[*len] = key;
            *len += 1;
        } else {
            dropped[sheet / 32] |= 1 << (sheet % 32);
        }
    }
    if dropped != [0; SHEETS / 32] {
        let mut m = 0;
        for i in 0..draws.count {
            let sheet = PIECES[draws.piece[i] as usize].0 as usize;
            if dropped[sheet / 32] & (1 << (sheet % 32)) == 0 {
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
