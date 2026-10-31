//! The compact actor tier: an enemy family's original PlayMaker trees on the HKFSM03 kernel, with no body of its
//! own (host/enemy_families.json; docs/ENEMY-FAMILIES.md).
//!
//! The caller owns the placement (position and quarter turn), the hero's body, the health and the hit flash. This
//! module owns what the tree decides: which clip plays when, and what box the sprite's frame puts on the object
//! (tk2d writes a Box sprite's collider into the object each time the frame changes). `tick` is one nominal 60 Hz
//! frame; the tree's waits count simulation ticks.
use crate::fsm3::{Block, Env, Tree, PH_ENTER, PH_UPDATE};
use hk_format::fsm3::Bank;

/// A clip as the animation draw reads it: frames, rate (a whole number of frames a second), wrap (0 loop, 1 loop
/// from `loop_start`, 2 once) and the frame a section loop returns to.
#[derive(Clone, Copy, Debug)]
pub struct ClipInfo {
    pub frames: u8,
    pub fps: u8,
    pub wrap: u8,
    pub loop_start: u8,
}

/// What the cooker (host/hk-fsm `enemy_family.rs`) knows of one family.
pub struct Family {
    pub name: &'static str,
    pub bank: &'static [u8],
    /// The bound clips, in manifest order.
    pub clips: &'static [ClipInfo],
    /// `(clip, frame, box)`: the hurt box a frame puts on the object, in the actor's own frame, Q16.
    pub boxes: &'static [(u8, u8, [i32; 4])],
    /// `(string id of a clip name in the bank, clip)`.
    pub slots: &'static [(u32, u8)],
    /// `(string id, length in 60 Hz ticks)` of the finite clips the animation actions wait for.
    pub env_clips: [(u32, u16); 4],
    /// The clip the corpse plays (255: none) and the clip the object rests on before its tree first plays one.
    pub death_slot: u8,
    pub rest_slot: u8,
}
include!("../../../data/enemy_families.rs");

const MEM_WORDS: usize = MAX_RUNTIME.div_ceil(4);

fn family(id: u8) -> &'static Family {
    &FAMILIES[id as usize]
}
fn bank(id: u8) -> Bank<'static> {
    // SAFETY: the cooker parses the bank it emits and the module transport hash-checks it.
    unsafe { Bank::from_verified(family(id).bank) }
}

/// The clip the tree played this step, and the tick it starts at.
pub type Play = Option<(u8, u32)>;

/// One placement's trees.
#[derive(Clone, Copy)]
pub struct FsmActor {
    mem: [u32; MEM_WORDS],
    anim_left: u16,
    family: u8,
    /// The hero was inside the trigger box last tick.
    inside: bool,
}
const _: () = assert!(core::mem::size_of::<FsmActor>() <= 72, "an FsmActor must fit the Runtime enum's 92 bytes");

impl FsmActor {
    /// The tree spawned already running: its start states entered where the object stands. The first clip the
    /// tree plays, if it plays one on entry, is returned.
    #[inline(never)]
    pub fn new(family: u8, seed: u32) -> (Self, Play) {
        let mut me = Self { mem: [0; MEM_WORDS], anim_left: 0xffff, family, inside: false };
        let mut env = me.env(0);
        // SAFETY: `mem` is at least the bank's runtime size (MAX_RUNTIME), 4-aligned.
        let mut tree = unsafe { Tree::init(bank(family), me.mem.as_mut_ptr().cast(), &mut env, seed) };
        tree.step(PH_ENTER);
        let play = me.harvest(&env);
        (me, play)
    }
    fn env(&self, trigger: u8) -> Env {
        let mut env = Env::new();
        env.anim_left = self.anim_left;
        env.clips = family(self.family).env_clips;
        env.trigger = trigger;
        env
    }
    /// Fold what the actions did back into the actor, and say which clip they started.
    fn harvest(&mut self, env: &Env) -> Play {
        self.anim_left = env.anim_left;
        let (id, from) = *env.plays[..env.play_count as usize].last()?;
        let slot = family(self.family).slots.iter().find(|s| s.0 == id)?.1;
        Some((slot, u32::from(from)))
    }
    /// One nominal 60 Hz frame. `inside`: the hero's body overlaps the trigger box now.
    #[inline(never)]
    pub fn tick(&mut self, inside: bool) -> Play {
        let entered = inside && !self.inside;
        let left = !inside && self.inside;
        self.inside = inside;
        if self.anim_left != 0 && self.anim_left != 0xffff {
            self.anim_left -= 1;
        }
        let mut env = self.env(u8::from(entered) | u8::from(inside) << 1 | u8::from(left) << 2);
        // SAFETY: `mem` was laid out for this bank by `new`.
        let mut tree = unsafe { Tree::attach(bank(self.family), self.mem.as_mut_ptr().cast(), &mut env) };
        tree.step(PH_UPDATE);
        self.harvest(&env)
    }
    /// The object is destroyed: its trees stop for good.
    #[inline(never)]
    pub fn die(&mut self) {
        // SAFETY: as above.
        let block = unsafe { Block::new(bank(self.family), self.mem.as_mut_ptr().cast()) };
        for f in 0..bank(self.family).fsm_count() {
            block.kill(f);
        }
    }
}

/// The clip the object shows before its tree has played one, and the tick that holds its last frame.
#[inline(never)]
pub fn rest(id: u8) -> (u8, u32) {
    let f = family(id);
    let c = f.clips[f.rest_slot as usize];
    (f.rest_slot, u32::from(c.frames) * 60 / u32::from(c.fps))
}

/// The clip a dying object plays, if it has one.
#[inline(never)]
pub fn death(id: u8) -> Option<u8> {
    Some(family(id).death_slot).filter(|s| *s != 255)
}

/// The frame of `clip` that plays at `tick` (the animation draw's own rule).
#[inline(never)]
pub fn frame_at(id: u8, clip: u8, tick: u32) -> u8 {
    let c = family(id).clips[clip as usize];
    let elapsed = crate::fixed::mul_div_floor(tick, u32::from(c.fps), 60);
    let n = u32::from(c.frames);
    let start = u32::from(c.loop_start);
    let frame = match c.wrap {
        0 => elapsed % n,
        1 if elapsed >= n => start + (elapsed - start) % (n - start),
        1 => elapsed,
        _ => elapsed.min(n - 1),
    };
    frame as u8
}

/// Whether a once-clip has played out.
#[inline(never)]
pub fn finished(id: u8, clip: u8, tick: u32) -> bool {
    let c = family(id).clips[clip as usize];
    c.wrap == 2 && crate::fixed::mul_div_floor(tick, u32::from(c.fps), 60) >= u32::from(c.frames)
}

/// The box the frame playing at `tick` puts on the object, in the actor's own frame (None: no collider).
#[inline(never)]
pub fn hurt_box(id: u8, clip: u8, tick: u32) -> Option<[i32; 4]> {
    let frame = frame_at(id, clip, tick);
    family(id).boxes.iter().find(|b| b.0 == clip && b.1 == frame).map(|b| b.2)
}

/// A box of the actor's frame turned by `quarter` quarter turns about the origin.
#[inline(never)]
pub fn turned(b: [i32; 4], quarter: i32) -> [i32; 4] {
    let turn = |x: i32, y: i32| match quarter.rem_euclid(4) {
        0 => [x, y],
        1 => [-y, x],
        2 => [-x, -y],
        _ => [y, -x],
    };
    let (p, q) = (turn(b[0], b[1]), turn(b[2], b[3]));
    [p[0].min(q[0]), p[1].min(q[1]), p[0].max(q[0]), p[1].max(q[1])]
}
