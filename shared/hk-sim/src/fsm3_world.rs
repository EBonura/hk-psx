//! The scene objects a boss tree acts on, and the native requests its actions make (docs/FSM-V2-DESIGN.md 2.4, 2.5).
//!
//! A light tree (Crawler, Gruzzer, Vengefly) is one body: the kernel's `Env` carries everything its actions touch. A boss
//! tree owns a small scene graph: the boss, its hit boxes, its needle and effect sprites, the hero-sensing ranges. The
//! cook lists those objects (the entity table, handle 0 is the boss) and the actor adapter owns one `World` for them.
//! Handlers read and write the table directly, the way Unity's actions do (a `SetScale` is visible to the next action of
//! the same state), and ask the engine for what only it can do (spawn, play a sound, shake the camera) with a request
//! the adapter drains after the pass.
//!
//! Transforms are 2D: a local position, a local scale and one rotation about z, composed up the parent chain. Positions
//! are Q16.16 world units, angles Q16.16 degrees.
use hk_format::fsm3::NONE8;

/// Objects of one tree (the bank's entity count is at most this).
pub const MAX_ENT: usize = 32;
/// Requests one step may make. A step past it drops the request and counts it in `World::lost`.
pub const MAX_REQ: usize = 24;
/// No clip has been played on the object.
pub const NO_CLIP: u32 = u32::MAX;
/// The deepest parent chain `world_of` composes (the Hornet tree's is three).
const DEPTH: usize = 6;

/// `Ent::flags`.
pub mod flag {
    /// The GameObject is active in the scene.
    pub const ACTIVE: u8 = 1;
    /// Its renderer (mesh or sprite) is enabled.
    pub const MESH: u8 = 2;
    /// Its collider is enabled.
    pub const COLLIDER: u8 = 4;
    /// The collider is a trigger.
    pub const TRIGGER: u8 = 8;
    /// Its Rigidbody2D is kinematic.
    pub const KINEMATIC: u8 = 16;
    /// The sprite animator is playing a clip that was started this step (the adapter clears it after it applies the play).
    pub const PLAYED: u8 = 32;
    /// The object has a Rigidbody2D: its velocity moves it (the boss's own body is the terrain solver's, handle 0).
    pub const BODY: u8 = 64;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ent {
    pub parent: u8,
    pub flags: u8,
    /// Local transform.
    pub pos: [i32; 3],
    pub scale: [i32; 2],
    pub rot: i32,
    /// The Rigidbody2D's velocity and gravity scale (only objects with a body use them).
    pub vel: [i32; 2],
    pub gravity: i32,
    /// `DamageHero.damageDealt`.
    pub damage: u8,
    pub tag: u8,
    /// The clip last played (a cooked string id), the tick it started at, and the ticks it has left (0xffff loops).
    pub clip: u32,
    pub clip_from: u16,
    pub anim_left: u16,
    /// A frame trigger of the clip fired and has not been taken by a `Tk2dPlayAnimationWithEvents` yet.
    pub anim_event: bool,
    /// The frame a `Tk2dPlayFrame` froze the sprite on, or -1 while a clip plays.
    pub frame: i16,
    /// The GameObject's name as a tree string id (FindChild and FindGameObject compare it), 0xffff if the tree never names it.
    pub name: u16,
    /// The box collider's size and offset (`SetBoxCollider2DSizeVector`).
    pub box_size: [i32; 2],
    pub box_offset: [i32; 2],
    /// What the actor adapter found of the hero against this object's trigger this step: bit 0 entered, 1 inside, 2 left.
    pub trig: u8,
}

impl Ent {
    pub const fn new() -> Self {
        Self {
            parent: NONE8,
            flags: flag::ACTIVE | flag::MESH,
            pos: [0; 3],
            scale: [crate::ONE; 2],
            rot: 0,
            vel: [0; 2],
            gravity: 0,
            damage: 0,
            tag: 0,
            clip: NO_CLIP,
            clip_from: 0,
            anim_left: 0xffff,
            anim_event: false,
            frame: -1,
            name: 0xffff,
            box_size: [0; 2],
            box_offset: [0; 2],
            trig: 0,
        }
    }
    #[inline(always)]
    pub fn on(&self, f: u8) -> bool {
        self.flags & f != 0
    }
    #[inline(always)]
    pub fn set(&mut self, f: u8, on: bool) {
        if on {
            self.flags |= f;
        } else {
            self.flags &= !f;
        }
    }
}

/// What an action asked the engine for. The record is `kind | object << 8 | flags << 16` and three values.
pub mod k {
    /// Create a pooled object (`values`: pool id, x, y); the handle the adapter gives it goes to `World::spawned`.
    pub const SPAWN: u8 = 1;
    /// Remove the object from the scene for good.
    pub const DESTROY: u8 = 2;
    /// Play a sound: values clip id, volume, pitch; flags 1 = from a spawned one-shot player, 2 = a looping voice owned by the state.
    pub const AUDIO: u8 = 3;
    pub const AUDIO_STOP: u8 = 4;
    /// A music cue: values cue id, delay, transition time.
    pub const MUSIC: u8 = 5;
    pub const SNAPSHOT: u8 = 6;
    /// Start (flags 1) or stop (0) a particle emitter on the object.
    pub const PARTICLES: u8 = 7;
    /// A camera shake or a hero-facing message addressed by name: values message id.
    pub const MESSAGE: u8 = 8;
    /// The collider's size and offset changed (values: w, h packed as two i16 would lose range, so see `Ent` mirror in the box table).
    pub const BOX: u8 = 9;
    /// `SetInvincible`: flags bit 0 invincible, bits 1.. the direction mask.
    pub const INVINCIBLE: u8 = 10;
    /// `SetRecoilSpeed`: value the new speed.
    pub const RECOIL: u8 = 11;
    /// A conversation on the dialogue box: value the conversation id.
    pub const CONVERSATION: u8 = 12;
    /// `HeroController` calls: flags 0 relinquish control, 1 regain it.
    pub const HERO: u8 = 13;
    /// The object's parent changed (value 0 = the new parent handle, flags bit 0/1 reset local position/rotation).
    pub const PARENT: u8 = 14;
    /// The object's renderer, collider, activation or similar toggle that the adapter mirrors from the table.
    pub const SYNC: u8 = 15;
    /// A PlayerData write that has a native side (disablePause).
    pub const PLAYERDATA: u8 = 16;
    /// A PlayMaker variable the engine owns (`SetFsmFloat` on the Recoil component's "recoil" FSM): values fsm name id, variable name id, value; the flags carry the action's op.
    pub const SETVAR: u8 = 17;
    /// A call into a native component (`HeroController.RelinquishControl`, `DialogueBox.StartConversation`, `GameManager.AwardAchievement`): values behaviour name id, method name id, first argument.
    pub const CALL: u8 = 18;
    /// `SpawnRandomObjectsV2`: values prefab handle, count drawn between the action's min and max, the speed range's low end (the adapter flings them).
    pub const SPAWN_RANDOM: u8 = 19;
}

/// An object's starting state as the cooker writes it (host/hk-fsm boss.rs).
#[derive(Clone, Copy, Debug)]
pub struct EntInit {
    pub parent: u8,
    pub flags: u8,
    pub pos: [i32; 3],
    pub scale: [i32; 2],
    pub rot: i32,
    pub gravity: i32,
    pub damage: u8,
    pub name: u16,
    pub box_size: [i32; 2],
    pub box_offset: [i32; 2],
}

#[derive(Clone, Copy)]
pub struct World {
    pub ent: [Ent; MAX_ENT],
    pub n: u8,
    pub req: [[i32; 4]; MAX_REQ],
    pub req_n: u8,
    /// Requests dropped because the queue was full.
    pub lost: u8,
    /// The handle of the object the last `SPAWN` made (NONE8 if none).
    pub spawned: u8,
    /// Objects outside the tree that actions look up by name: (name id, ENT_HERO / ENT_CAMERA).
    pub outside: [(u16, u8); 4],
    /// Length in 60 Hz ticks of the clips the tree plays (0xffff loops), by cooked clip id.
    pub clips: [(u32, u16); 40],
    pub clip_count: u8,
}

impl World {
    pub const fn new() -> Self {
        Self { ent: [Ent::new(); MAX_ENT], n: 0, req: [[0; 4]; MAX_REQ], req_n: 0, lost: 0, spawned: NONE8, outside: [(0xffff, NONE8); 4], clips: [(NO_CLIP, 0); 40], clip_count: 0 }
    }
    #[inline(always)]
    pub fn get(&self, h: u8) -> Option<&Ent> {
        self.ent[..self.n as usize].get(h as usize)
    }
    #[inline(always)]
    pub fn get_mut(&mut self, h: u8) -> Option<&mut Ent> {
        let n = self.n as usize;
        self.ent[..n].get_mut(h as usize)
    }
    /// Lay the scene objects out as the cooker listed them.
    pub fn load(&mut self, init: &[EntInit]) {
        *self = Self::new();
        self.n = init.len().min(MAX_ENT) as u8;
        for (e, i) in self.ent.iter_mut().zip(init) {
            *e = Ent::new();
            e.parent = i.parent;
            e.flags = i.flags;
            e.pos = i.pos;
            e.scale = i.scale;
            e.rot = i.rot;
            e.gravity = i.gravity;
            e.damage = i.damage;
            e.name = i.name;
            e.box_size = i.box_size;
            e.box_offset = i.box_offset;
        }
    }
    /// Append a request. `object` is the target handle (NONE8 for none).
    #[inline(never)]
    pub fn request(&mut self, kind: u8, object: u8, flags: u8, values: [i32; 3]) {
        let n = self.req_n as usize;
        if n >= MAX_REQ {
            self.lost = self.lost.saturating_add(1);
            return;
        }
        self.req[n] = [kind as i32 | (object as i32) << 8 | (flags as i32) << 16, values[0], values[1], values[2]];
        self.req_n += 1;
    }

    /// The ticks a clip lasts (0xffff loops), 0xffff for a clip the adapter did not list.
    #[inline(never)]
    pub fn clip_ticks(&self, id: u32) -> u16 {
        for i in 0..self.clip_count as usize {
            if self.clips[i].0 == id {
                return self.clips[i].1;
            }
        }
        0xffff
    }
    /// Whether object `h` and every ancestor are active (Unity's `activeInHierarchy`).
    #[inline(never)]
    pub fn active_in_hierarchy(&self, h: u8) -> bool {
        let mut at = h;
        for _ in 0..DEPTH {
            match self.get(at) {
                Some(e) => {
                    if !e.on(flag::ACTIVE) {
                        return false;
                    }
                    at = e.parent;
                }
                None => return true,
            }
        }
        true
    }
    /// The object `name` under `parent` (a direct child), or NONE8.
    #[inline(never)]
    pub fn find_child(&self, parent: u8, name: u16) -> u8 {
        for i in 0..self.n {
            let e = &self.ent[i as usize];
            if e.parent == parent && e.name == name {
                return i;
            }
        }
        NONE8
    }
    /// The composed transform of object `h`: (position, scale, rotation). A parentless object is its local transform.
    #[inline(never)]
    pub fn world_of(&self, h: u8) -> ([i32; 3], [i32; 2], i32) {
        let mut chain = [NONE8; DEPTH];
        let mut depth = 0;
        let mut at = h;
        while depth < DEPTH {
            match self.get(at) {
                Some(e) => {
                    chain[depth] = at;
                    depth += 1;
                    at = e.parent;
                }
                None => break,
            }
        }
        // Fold from the root down.
        let mut pos = [0i32; 3];
        let mut scale = [crate::ONE; 2];
        let mut rot = 0i32;
        for i in (0..depth).rev() {
            let e = self.ent[chain[i] as usize];
            let (sx, sy) = (mul(e.pos[0], scale[0]), mul(e.pos[1], scale[1]));
            let (sin, cos) = sincos(rot);
            pos = [pos[0] + mul(sx, cos) - mul(sy, sin), pos[1] + mul(sx, sin) + mul(sy, cos), pos[2] + e.pos[2]];
            scale = [mul(scale[0], e.scale[0]), mul(scale[1], e.scale[1])];
            rot = rot.wrapping_add(e.rot);
        }
        (pos, scale, rot)
    }

    /// Where object `h`'s local position must be for it to stand at world position `at` (x, y; z kept).
    #[inline(never)]
    pub fn local_for(&self, h: u8, at: [i32; 3]) -> [i32; 3] {
        let parent = self.get(h).map_or(NONE8, |e| e.parent);
        if self.get(parent).is_none() {
            return at;
        }
        let (pp, ps, pr) = self.world_of(parent);
        let (dx, dy) = (at[0] - pp[0], at[1] - pp[1]);
        let (sin, cos) = sincos(-pr);
        let (lx, ly) = (mul(dx, cos) - mul(dy, sin), mul(dx, sin) + mul(dy, cos));
        [div(lx, ps[0]), div(ly, ps[1]), at[2] - pp[2]]
    }
}

/// Q16.16 multiply.
#[inline(always)]
pub fn mul(a: i32, b: i32) -> i32 {
    psx_math::int32::mul_shr_i32(a, b, 16)
}
/// Q16.16 divide; a zero divisor leaves the value (a scale of zero would otherwise trap).
#[inline(always)]
pub fn div(a: i32, b: i32) -> i32 {
    if b == 0 {
        a
    } else {
        psx_math::int32::mul_div_i32(a, crate::ONE, b)
    }
}

/// (sin, cos) of an angle in Q16.16 degrees, as Q16.16.
#[inline(never)]
pub fn sincos(deg: i32) -> (i32, i32) {
    // 4096 table units a turn: degrees * 4096 / 360.
    let q12 = (psx_math::int32::mul_div_i32(deg, 4096, 360 * crate::ONE).rem_euclid(4096)) as u16;
    (psx_math::sincos::sin_q12(q12) << 4, psx_math::sincos::cos_q12(q12) << 4)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ONE;

    #[test]
    fn a_child_composes_with_its_parents_scale_and_rotation() {
        let mut w = World::new();
        w.n = 2;
        w.ent[0].pos = [10 * ONE, 4 * ONE, 0];
        w.ent[0].scale = [-ONE, ONE];
        w.ent[1].parent = 0;
        w.ent[1].pos = [2 * ONE, 0, 0];
        let (p, s, _) = w.world_of(1);
        assert_eq!((p[0], p[1]), (8 * ONE, 4 * ONE), "a flipped parent mirrors its children's offsets");
        assert_eq!(s, [-ONE, ONE]);
        // A quarter turn puts the child's x offset on y.
        w.ent[0].scale = [ONE, ONE];
        w.ent[0].rot = 90 * ONE;
        let (p, _, r) = w.world_of(1);
        assert!((p[0] - 10 * ONE).abs() < 64 && (p[1] - 6 * ONE).abs() < 64, "{p:?}");
        assert_eq!(r, 90 * ONE);
    }

    #[test]
    fn local_for_inverts_world_of() {
        let mut w = World::new();
        w.n = 2;
        w.ent[0].pos = [10 * ONE, 4 * ONE, 0];
        w.ent[0].scale = [-ONE, ONE];
        w.ent[0].rot = 30 * ONE;
        w.ent[1].parent = 0;
        let local = w.local_for(1, [7 * ONE, 9 * ONE, 0]);
        w.ent[1].pos = local;
        let (p, _, _) = w.world_of(1);
        assert!((p[0] - 7 * ONE).abs() < 200 && (p[1] - 9 * ONE).abs() < 200, "{p:?}");
    }

    #[test]
    fn requests_beyond_the_queue_are_counted_not_kept() {
        let mut w = World::new();
        for i in 0..MAX_REQ + 3 {
            w.request(k::AUDIO, 0, 0, [i as i32, 0, 0]);
        }
        assert_eq!((w.req_n as usize, w.lost), (MAX_REQ, 3));
    }
}
