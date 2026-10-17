//! The resident HKFSM03 kernel (docs/FSM-V2-DESIGN.md sections 2.1 to 2.3).
//!
//! Everything a PlayMaker FSM needs that is not an action: entering a state, the ordered
//! action pass, event transitions, the finished set, the delayed-event queue and the
//! operand API the family handlers call. It is not generic and calls nothing through a
//! pointer: the simulation runs on the 1 KiB scratchpad stack and the build's stack guard
//! refuses recursion and register calls there, so family dispatch is a direct call per
//! family (each an import site while its module is evicted) and cross-FSM events are
//! queued and drained iteratively, never delivered by a nested call.
//!
//! A tree is one actor's FSMs. Its runtime block is plain bytes the caller owns (inline in
//! the actor), laid out from the bank by `init`:
//!
//! ```text
//! 8 B tree header   rng u32, queue length u8, faults u8, spare u16
//! queue             4 B each: event u8, target u8 (0xff = every FSM that handles it), delay u16
//! FSM headers       4 B each: state, previous state, flags, spare
//! FSM data          cells (i32), scratch bytes, bits, object handles, finished bitset
//! ```
//!
//! What the world gives an action, and what the action does to it, goes through `Env`, a plain
//! struct the actor adapter fills before a step and reads after it.
use crate::{WalkParams, WalkState};
use hk_format::fsm3::{self as f3, Bank, F_FIXED, F_LATE, F_UPDATE, NONE8};

pub const PH_ENTER: u8 = 0;
pub const PH_UPDATE: u8 = 1;
pub const PH_FIXED: u8 = 2;
pub const PH_LATE: u8 = 3;
pub const PH_COLLISION_ENTER: u8 = 4;
pub const PH_COLLISION_STAY: u8 = 5;

/// A handler's result: bit 0 = the action finished, the rest = 1 + the event it raises (0 = none).
pub const R_CONT: u16 = 0;
pub const R_FIN: u16 = 1;
#[inline(always)]
pub const fn r_event(event: u8) -> u16 {
    ((event as u16) + 1) << 1
}
/// A result: `fin` says the action finished, `event` is raised (an event that is NONE raises nothing).
#[inline(always)]
pub const fn r_with(fin: bool, event: u8) -> u16 {
    (fin as u16) | if event == NONE8 { 0 } else { r_event(event) }
}
/// The stateless-action default: finished unless the action stays active across frames.
#[inline(always)]
pub const fn r_stateless(flags: u8) -> u16 {
    if flags & (F_UPDATE | F_FIXED | F_LATE) == 0 {
        R_FIN
    } else {
        R_CONT
    }
}

/// +1 when a heading moves right, -1 otherwise: the sign FaceDirection reads of a velocity along it.
pub fn heading_sign(angle: i32, speed: i32) -> i32 {
    let a = angle.rem_euclid(360 * crate::ONE);
    if speed != 0 && !(90 * crate::ONE..=270 * crate::ONE).contains(&a) {
        1
    } else {
        -1
    }
}

#[cfg(feature = "trace")]
pub mod trace {
    //! The kernel's observable stream, for comparing it with the original's (tools/hkref FsmTrace).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Cause {
        /// Raised by the action at this index of the state.
        Action(u8),
        /// All actions of the state finished.
        Finish,
        /// Handed to the tree from outside (`Tree::event`).
        External,
        /// Delivered from the tree's event queue.
        Queue,
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Ev {
        StateEnter { fsm: u8, state: u8 },
        Action { fsm: u8, state: u8, index: u8, family: u8, op: u8, phase: u8 },
        ActionFinish { fsm: u8, state: u8, index: u8 },
        Event { fsm: u8, state: u8, event: u8, cause: Cause },
        Transition { fsm: u8, event: u8, from: u8, to: u8 },
    }
    pub static mut SINK: Option<fn(Ev)> = None;
    pub fn emit(e: Ev) {
        // SAFETY: tests install the sink before running a tree and run one tree at a time.
        unsafe {
            if let Some(f) = SINK {
                f(e)
            }
        }
    }
}
macro_rules! trace {
    ($e:expr) => {
        #[cfg(feature = "trace")]
        {
            trace::emit($e);
        }
    };
}

/// Transitions one step (an event or a tick) may take before the FSM is stopped.
pub const BUDGET: u8 = 32;
const FSM_HEADER: usize = 4;
const TREE_HEADER: usize = 8;
const QUEUE_ENTRY: usize = 4;

const ENTERED: u8 = 1;
const FIN_SENT: u8 = 2;
const DEAD: u8 = 4;

/// What the actor gives the actions and takes back. All Q16.16.
#[derive(Clone, Copy, Debug)]
pub struct Env {
    /// Time since the previous step in Q16.16 simulation ticks (one tick = 65536): the game clock a plain `Wait`
    /// counts, and the wall clock a `Wait` with realTime counts (it keeps running through a pause).
    pub dt_scaled: i32,
    pub dt_real: i32,
    pub pos: [i32; 3],
    pub hero: [i32; 3],
    pub camera: [i32; 3],
    /// The Walker's three senses: grounded, wall ahead, ledge ahead.
    pub senses: (bool, bool, bool),
    pub walk: WalkState,
    pub walk_params: WalkParams,
    /// Written by WalkLeftRight: this step's horizontal speed (signed).
    pub velocity: i32,
    /// Up, right, down, left contact bits for CheckCollisionSide.
    pub contacts: u8,
    /// Gruzzer body: heading and speed set by SetVelocityAsAngle, the facing FaceDirection derives.
    pub angle: i32,
    pub speed: i32,
    pub facing: i8,
    /// The body's velocity. FaceDirection reads its sign; the Gruzzer keeps only the heading's sign here.
    pub vel: [i32; 2],
    /// Vengefly senses, supplied by the actor: the line-of-sight detector's two answers and the alert radius.
    pub can_see_hero: bool,
    pub in_alert_range: bool,
    pub alert_range: i32,
    /// Ticks left in the clip being played (0xffff loops, 0 = it has completed).
    pub anim_left: u16,
    /// Clips asked for this step, in order: (a cooked string id, the tick to start at). `play_count` of them.
    pub plays: [(u32, u16); 3],
    pub play_count: u8,
    /// (clip string id, length in 60 Hz ticks, 0xffff = loops) for the clips this actor plays.
    pub clips: [(u32, u16); 4],
    /// One-shot sounds asked for this step (bit set), and the audio pitch last set.
    pub sound: u8,
    pub pitch: i32,
    /// What the actions touched this step: bit 0 the velocity, bit 1 the facing.
    pub dirty: u8,
    /// The save record's PlayerData reserve (null when the tree has no business with it) and the writes made.
    pub pd: *mut i32,
    pub pd_writes: u16,
    /// The hero's contact with this script's trigger volume this tick: bit 0 entered, 1 inside, 2 left.
    pub trigger: u8,
    /// Measurement handlers only (fsm3_hard.rs): the requests a pass made, the handle of the object just spawned and
    /// whether the clip fired a frame trigger.
    #[cfg(feature = "hard-ops")]
    pub req: [[i32; 4]; 8],
    #[cfg(feature = "hard-ops")]
    pub req_n: u8,
    #[cfg(feature = "hard-ops")]
    pub spawned: u8,
    #[cfg(feature = "hard-ops")]
    pub anim_trigger: bool,
    /// Oracle replay only: values the next RandomFloat calls return instead of drawing (consumed in order).
    #[cfg(feature = "trace")]
    pub forced_random: [i32; 8],
    #[cfg(feature = "trace")]
    pub forced_n: u8,
}
pub const NO_PLAY: u32 = u32::MAX;
pub const SOUND_STARTLE: u8 = 1;
pub const DIRTY_VEL: u8 = 1;
pub const DIRTY_FACING: u8 = 2;

impl Env {
    pub const fn new() -> Self {
        Self {
            dt_scaled: 65536,
            dt_real: 65536,
            pos: [0; 3],
            hero: [0; 3],
            camera: [0; 3],
            senses: (false, false, false),
            walk: WalkState::new(-1, false),
            walk_params: WalkParams { speed: 0, turn_ticks: 0, turn_cooldown_ticks: 0 },
            velocity: 0,
            contacts: 0,
            angle: 0,
            speed: 0,
            facing: -1,
            vel: [0; 2],
            can_see_hero: false,
            in_alert_range: false,
            alert_range: 0,
            anim_left: 0xffff,
            plays: [(NO_PLAY, 0); 3],
            play_count: 0,
            clips: [(NO_PLAY, 0xffff); 4],
            sound: 0,
            pitch: 0,
            dirty: 0,
            pd: core::ptr::null_mut(),
            pd_writes: 0,
            trigger: 0,
            #[cfg(feature = "hard-ops")]
            req: [[0; 4]; 8],
            #[cfg(feature = "hard-ops")]
            req_n: 0,
            #[cfg(feature = "hard-ops")]
            spawned: 0xff,
            #[cfg(feature = "hard-ops")]
            anim_trigger: false,
            #[cfg(feature = "trace")]
            forced_random: [0; 8],
            #[cfg(feature = "trace")]
            forced_n: 0,
        }
    }
}
impl Env {
    /// Where entity `handle` is: the actor, the hero, the camera; anything else is the origin.
    pub fn position_of(&self, handle: u8) -> [i32; 3] {
        match handle {
            f3::ENT_SELF => self.pos,
            f3::ENT_HERO => self.hero,
            f3::ENT_CAMERA => self.camera,
            _ => [0; 3],
        }
    }
}
impl Default for Env {
    fn default() -> Self {
        Self::new()
    }
}

/// One actor's FSMs: the bank, the runtime block, the world.
pub struct Tree<'a> {
    pub bank: Bank<'a>,
    mem: *mut u8,
    pub env: &'a mut Env,
    budget: u8,
}

/// What a handler sees: the tree, the FSM it belongs to and the action's operands.
pub struct Ctx<'t, 'a> {
    pub t: &'t mut Tree<'a>,
    pub f: u8,
    pub flags: u8,
    pub phase: u8,
    /// Index of the action within its state.
    pub index: u8,
    ops: usize,
    /// Byte offsets from the runtime block of this FSM's regions: cells, scratch bytes, bits, object handles,
    /// finished set. Worked out once per pass so an operand access is an add, not a walk over the bank.
    at: [u16; 5],
}

pub type Handler = fn(&mut Ctx<'_, '_>, u8) -> u16;

#[inline(always)]
fn dir_u16(bank: Bank<'_>, f: u8, k: usize) -> usize {
    bank.u16_at(bank.dir_at(f) + k)
}

/// Byte offset of FSM `f`'s data in the runtime block (4-aligned): after the headers and the queue,
/// each FSM's data in bank order. `f` = the FSM count gives the end.
fn fsm_base(bank: Bank<'_>, f: u8) -> usize {
    let mut at = TREE_HEADER + QUEUE_ENTRY * bank.queue_cap() as usize + FSM_HEADER * bank.fsm_count() as usize;
    for g in 0..f {
        at = (at + 3) & !3;
        at += 4 * dir_u16(bank, g, 4)
            + bank.byte(bank.dir_at(g) + 10) as usize
            + dir_u16(bank, g, 6).div_ceil(8)
            + dir_u16(bank, g, 8)
            + bank.byte(bank.dir_at(g) + 3) as usize;
    }
    (at + 3) & !3
}

/// Where FSM `f`'s data regions start, as byte offsets from the block: 0 cells, 1 scratch bytes, 2 bits,
/// 3 object handles, 4 finished set.
#[inline(never)]
fn layout(b: Bank<'_>, f: u8) -> [u16; 5] {
    let mut at = fsm_base(b, f);
    let mut out = [0u16; 5];
    for k in 0..5 {
        out[k] = at as u16;
        at += match k {
            0 => 4 * dir_u16(b, f, 4),
            1 => b.byte(b.dir_at(f) + 10) as usize,
            2 => dir_u16(b, f, 6).div_ceil(8),
            3 => dir_u16(b, f, 8),
            _ => 0,
        };
    }
    out
}

/// A tree's runtime block read without a world: what an actor asks of its graph between steps.
#[derive(Clone, Copy)]
pub struct Block<'a> {
    bank: Bank<'a>,
    mem: *mut u8,
}
impl<'a> Block<'a> {
    /// # Safety
    /// `mem` must have been laid out by `Tree::init` for this exact bank.
    #[inline(always)]
    pub unsafe fn new(bank: Bank<'a>, mem: *mut u8) -> Self {
        Self { bank, mem }
    }
    #[inline(always)]
    unsafe fn fh(&self, f: u8) -> *mut u8 {
        self.mem.add(TREE_HEADER + QUEUE_ENTRY * self.bank.queue_cap() as usize + FSM_HEADER * f as usize)
    }
    /// The state FSM `f` is in.
    #[inline(never)]
    pub fn state(&self, f: u8) -> u8 {
        unsafe { *self.fh(f) }
    }
    /// The state it came from.
    pub fn previous(&self, f: u8) -> u8 {
        unsafe { *self.fh(f).add(1) }
    }
    #[inline(never)]
    pub fn dead(&self, f: u8) -> bool {
        unsafe { *self.fh(f).add(2) & DEAD != 0 }
    }
    /// Stop FSM `f` for good (the actor died).
    #[inline(never)]
    pub fn kill(&self, f: u8) {
        unsafe { *self.fh(f).add(2) |= DEAD }
    }
    /// Cell `i` of FSM `f` (tests and telemetry; handlers use the operand API).
    #[inline(never)]
    pub fn cell(&self, f: u8, i: usize) -> i32 {
        unsafe { self.mem.add(layout(self.bank, f)[0] as usize).cast::<i32>().add(i).read_unaligned() }
    }
    pub fn set_cell(&self, f: u8, i: usize, v: i32) {
        unsafe { self.mem.add(layout(self.bank, f)[0] as usize).cast::<i32>().add(i).write_unaligned(v) }
    }
    #[inline(never)]
    pub fn scratch_word(&self, f: u8, off: usize) -> i32 {
        unsafe { self.mem.add(layout(self.bank, f)[1] as usize).add(off).cast::<i32>().read_unaligned() }
    }
    pub fn bit_value(&self, f: u8, i: usize) -> bool {
        unsafe { *self.mem.add(layout(self.bank, f)[2] as usize).add(i >> 3) & (1 << (i & 7)) != 0 }
    }
}

impl<'a> Tree<'a> {
    /// Lay a tree out in `mem` and apply every FSM's initial images. FSMs start un-entered:
    /// the first `step` enters their start state.
    ///
    /// # Safety
    /// `mem` must be `bank.runtime_bytes()` long and 4-aligned; `bank` must have passed `Bank::parse`.
    #[inline(never)]
    pub unsafe fn init(bank: Bank<'a>, mem: *mut u8, env: &'a mut Env, seed: u32) -> Self {
        let n = bank.fsm_count();
        let total = bank.runtime_bytes();
        core::ptr::write_bytes(mem, 0, total);
        mem.cast::<u32>().write(seed);
        let cap = bank.queue_cap() as usize;
        for f in 0..n {
            let d = bank.dir(f);
            let h = mem.add(TREE_HEADER + QUEUE_ENTRY * cap + FSM_HEADER * f as usize);
            *h = d.start;
            *h.add(1) = d.start;
            let cells = fsm_base(bank, f);
            let bits = cells + 4 * d.cells as usize + d.scratch as usize;
            let objs = bits + (d.bits as usize).div_ceil(8);
            // Initial images: non-zero cells, then the bit image (if any), then the object handles.
            let mut p = d.init_at as usize;
            for _ in 0..d.init_count {
                let slot = if d.wide {
                    let s = bank.u16_at(p);
                    p += 2;
                    s
                } else {
                    let s = bank.byte(p) as usize;
                    p += 1;
                    s
                };
                let v = i32::from_le_bytes([bank.byte(p), bank.byte(p + 1), bank.byte(p + 2), bank.byte(p + 3)]);
                p += 4;
                mem.add(cells + 4 * slot).cast::<i32>().write_unaligned(v);
            }
            let tail = d.end as usize - p;
            let bit_bytes = tail - d.objs as usize;
            core::ptr::copy_nonoverlapping(bank.bytes().as_ptr().add(p), mem.add(bits), bit_bytes);
            core::ptr::copy_nonoverlapping(bank.bytes().as_ptr().add(p + bit_bytes), mem.add(objs), d.objs as usize);
        }
        debug_assert!(fsm_base(bank, n) <= total);
        Self { bank, mem, env, budget: BUDGET }
    }

    /// Attach an already initialised runtime block (the actor keeps it between steps).
    ///
    /// # Safety
    /// `mem` must have been laid out by `init` for this exact bank.
    #[inline(always)]
    pub unsafe fn attach(bank: Bank<'a>, mem: *mut u8, env: &'a mut Env) -> Self {
        Self { bank, mem, env, budget: BUDGET }
    }

    #[inline(always)]
    unsafe fn fh(&self, f: u8) -> *mut u8 {
        self.mem.add(TREE_HEADER + QUEUE_ENTRY * self.bank.queue_cap() as usize + FSM_HEADER * f as usize)
    }
    /// One of FSM `f`'s data regions, see `layout`.
    #[inline(never)]
    unsafe fn region(&self, f: u8, k: usize) -> *mut u8 {
        self.mem.add(layout(self.bank, f)[k] as usize)
    }

    #[inline(always)]
    pub fn block(&self) -> Block<'a> {
        Block { bank: self.bank, mem: self.mem }
    }
    pub fn state(&self, f: u8) -> u8 {
        self.block().state(f)
    }
    pub fn previous(&self, f: u8) -> u8 {
        self.block().previous(f)
    }
    pub fn dead(&self, f: u8) -> bool {
        self.block().dead(f)
    }
    pub fn kill(&mut self, f: u8) {
        self.block().kill(f)
    }
    pub fn cell(&self, f: u8, i: usize) -> i32 {
        self.block().cell(f, i)
    }
    pub fn set_cell(&mut self, f: u8, i: usize, v: i32) {
        self.block().set_cell(f, i, v)
    }
    pub fn scratch_word(&self, f: u8, off: usize) -> i32 {
        self.block().scratch_word(f, off)
    }
    pub fn bit_value(&self, f: u8, i: usize) -> bool {
        self.block().bit_value(f, i)
    }
    /// Transitions refused because the step ran out of budget.
    pub fn faults(&self) -> u8 {
        unsafe { *self.mem.add(5) }
    }
    pub fn rng(&self) -> u32 {
        unsafe { self.mem.cast::<u32>().read() }
    }

    // ---- the machine ------------------------------------------------------------------

    /// Take the transition for `event` in FSM `f`, if the state or the globals have one.
    /// Returns whether the FSM changed state (the caller then runs the enter pass).
    #[inline(never)]
    unsafe fn transition(&mut self, f: u8, event: u8) -> bool {
        let b = self.bank;
        let h = self.fh(f);
        let rec = self.bank.state(f, *h);
        let mut target = NONE8;
        let mut at = rec.block as usize;
        for _ in 0..rec.transitions {
            if b.byte(at) == event {
                target = b.byte(at + 1);
                break;
            }
            at += 2;
        }
        if target == NONE8 {
            let d = self.bank.dir(f);
            let mut g = d.globals_at as usize;
            for _ in 0..d.globals {
                if b.byte(g) == event {
                    target = b.byte(g + 1);
                    break;
                }
                g += 2;
            }
        }
        if target == NONE8 {
            return false;
        }
        if self.budget == 0 {
            *h.add(2) |= DEAD;
            *self.mem.add(5) = (*self.mem.add(5)).saturating_add(1);
            return false;
        }
        self.budget -= 1;
        trace!(trace::Ev::Transition { fsm: f, event, from: *h, to: target });
        *h.add(1) = *h;
        *h = target;
        *h.add(2) &= !(ENTERED | FIN_SENT);
        core::ptr::write_bytes(self.region(f, 4), 0, self.bank.byte(self.bank.dir_at(f) + 3) as usize);
        true
    }

    /// Run the enter pass of the state FSM `f` is in, and again after every transition it takes.
    #[inline(never)]
    unsafe fn enter(&mut self, f: u8) {
        loop {
            let h = self.fh(f);
            if *h.add(2) & (ENTERED | DEAD) != 0 {
                return;
            }
            *h.add(2) |= ENTERED;
            trace!(trace::Ev::StateEnter { fsm: f, state: *h });
            if !self.pass(f, PH_ENTER) {
                return;
            }
        }
    }

    /// One ordered pass over the current state's actions. Returns whether a transition was taken.
    #[inline(never)]
    unsafe fn pass(&mut self, f: u8, phase: u8) -> bool {
        let b = self.bank;
        let h = self.fh(f);
        let rec = self.bank.state(f, *h);
        let lay = layout(self.bank, f);
        let fin = self.mem.add(lay[4] as usize);
        let mut at = rec.block as usize + 2 * rec.transitions as usize;
        for i in 0..rec.actions {
            let head = b.byte(at);
            let (fam, flags, op) = (head & 0xf, head >> 4, b.byte(at + 1));
            let k = self.bank.arity_at(at);
            let done = *fin.add((i >> 3) as usize) & (1 << (i & 7)) != 0;
            let wanted = match phase {
                PH_ENTER => true,
                PH_UPDATE => flags & F_UPDATE != 0,
                PH_FIXED => flags & F_FIXED != 0,
                PH_LATE => flags & F_LATE != 0,
                _ => true,
            };
            if !done && wanted {
                let mut c = Ctx { t: &mut *(self as *mut Self), f, flags, phase, index: i, ops: at + 2, at: lay };
                trace!(trace::Ev::Action { fsm: f, state: *h, index: i, family: fam, op, phase });
                let r = dispatch(&mut c, fam, op);
                if r & R_FIN != 0 {
                    *fin.add((i >> 3) as usize) |= 1 << (i & 7);
                    trace!(trace::Ev::ActionFinish { fsm: f, state: *h, index: i });
                }
                let ev = (r >> 1) as u8;
                if ev != 0 {
                    trace!(trace::Ev::Event { fsm: f, state: *h, event: ev - 1, cause: trace::Cause::Action(i) });
                }
                if ev != 0 && self.transition(f, ev - 1) {
                    return true;
                }
            }
            at += 2 + 2 * k;
        }
        // FINISHED once, when every action of the state has finished.
        if *h.add(2) & FIN_SENT == 0 {
            let na = rec.actions as usize;
            let mut all = true;
            for i in 0..na {
                if *fin.add(i >> 3) & (1 << (i & 7)) == 0 {
                    all = false;
                    break;
                }
            }
            if all {
                *h.add(2) |= FIN_SENT;
                trace!(trace::Ev::Event { fsm: f, state: *h, event: f3::FINISHED, cause: trace::Cause::Finish });
                return self.transition(f, f3::FINISHED);
            }
        }
        false
    }

    /// Deliver every queued event that is due. Iterative: a delivery may queue more.
    #[inline(never)]
    unsafe fn drain(&mut self) {
        let mut guard = 0u8;
        loop {
            let q = self.mem.add(TREE_HEADER);
            let n = *self.mem.add(4) as usize;
            let mut at = n;
            for i in 0..n {
                if q.add(QUEUE_ENTRY * i).add(2).cast::<u16>().read_unaligned() == 0 {
                    at = i;
                    break;
                }
            }
            if at == n || guard == BUDGET {
                return;
            }
            guard += 1;
            let e = q.add(QUEUE_ENTRY * at);
            let (ev, target) = (*e, *e.add(1));
            // Remove it, keeping the order of the rest.
            core::ptr::copy(e.add(QUEUE_ENTRY), e, QUEUE_ENTRY * (n - at - 1));
            *self.mem.add(4) = (n - 1) as u8;
            self.budget = BUDGET;
            for g in 0..self.bank.fsm_count() {
                if (target == NONE8 && self.bank.handles(g, ev)) || target == g {
                    trace!(trace::Ev::Event { fsm: g, state: *self.fh(g), event: ev, cause: trace::Cause::Queue });
                    if *self.fh(g).add(2) & DEAD == 0 && self.transition(g, ev) {
                        self.enter(g);
                    }
                }
            }
        }
    }

    /// Age the queue by one simulation tick: entries not yet due count down.
    #[inline(never)]
    unsafe fn age(&mut self) {
        let q = self.mem.add(TREE_HEADER);
        for i in 0..*self.mem.add(4) as usize {
            let d = q.add(QUEUE_ENTRY * i).add(2).cast::<u16>();
            let v = d.read_unaligned();
            if v > 1 {
                d.write_unaligned(v - 1);
            } else if v == 1 {
                d.write_unaligned(0);
            }
        }
    }

    /// Run one phase over every FSM of the tree, in bank order. Entering a start state is part of
    /// the first step. Update steps also age and deliver the delayed events.
    #[inline(never)]
    pub fn step(&mut self, phase: u8) {
        unsafe {
            if phase == PH_UPDATE {
                self.age();
                self.drain();
            }
            for f in 0..self.bank.fsm_count() {
                if *self.fh(f).add(2) & DEAD != 0 {
                    continue;
                }
                self.budget = BUDGET;
                self.enter(f);
                if phase != PH_ENTER && self.pass(f, phase) {
                    self.enter(f);
                }
                self.drain();
            }
        }
    }

    /// Hand FSM `f` an event from outside the tree (a message, a feed). Its transition and enter
    /// pass run now.
    #[inline(never)]
    pub fn event(&mut self, f: u8, event: u8) {
        unsafe {
            if *self.fh(f).add(2) & DEAD != 0 {
                return;
            }
            self.budget = BUDGET;
            trace!(trace::Ev::Event { fsm: f, state: *self.fh(f), event, cause: trace::Cause::External });
            if self.transition(f, event) {
                self.enter(f);
            }
            self.drain();
        }
    }
}

// ---- family dispatch ------------------------------------------------------------------

/// A family whose module code is not written yet refuses its actions: the cook never emits one.
#[inline(never)]
pub fn refuse(_c: &mut Ctx<'_, '_>, _op: u8) -> u16 {
    R_CONT
}

#[inline(never)]
fn dispatch(c: &mut Ctx<'_, '_>, family: u8, op: u8) -> u16 {
    use crate::{
        fsm3_ai, fsm3_animation, fsm3_audio, fsm3_events, fsm3_flow, fsm3_math, fsm3_object, fsm3_playerdata,
        fsm3_timers, fsm3_transform,
    };
    #[cfg(feature = "hard-ops")]
    {
        use crate::fsm3_hard as h;
        let r = match family {
            0 => h::flow(c, op),
            1 => h::math(c, op),
            2 => h::events(c, op),
            4 => h::transform(c, op),
            5 => h::ai(c, op),
            7 => h::animation(c, op),
            8 => h::object(c, op),
            9 => h::audio(c, op),
            10 => h::particles(c, op),
            11 => h::collider(c, op),
            12 => h::combat(c, op),
            _ => R_CONT,
        };
        if r != R_CONT {
            return r;
        }
    }
    match family {
        0 => fsm3_flow::entry(c, op),
        1 => fsm3_math::entry(c, op),
        2 => fsm3_events::entry(c, op),
        3 => fsm3_timers::entry(c, op),
        4 => fsm3_transform::entry(c, op),
        5 => fsm3_ai::entry(c, op),
        7 => fsm3_animation::entry(c, op),
        8 => fsm3_object::entry(c, op),
        9 => fsm3_audio::entry(c, op),
        13 => fsm3_playerdata::entry(c, op),
        _ => refuse(c, op),
    }
}

// ---- operand API (what handlers import) ---------------------------------------------------

impl<'t, 'a> Ctx<'t, 'a> {
    #[inline(always)]
    unsafe fn reg(&self, k: usize) -> *mut u8 {
        self.t.mem.add(self.at[k] as usize)
    }
    #[inline(always)]
    fn bit_at(&self, p: u16) -> bool {
        unsafe { *self.reg(2).add((p >> 3) as usize) & (1 << (p & 7)) != 0 }
    }
    /// Operand `n` as its raw word.
    #[inline(always)]
    pub fn word(&self, n: u8) -> u16 {
        self.t.bank.word(self.ops + 2 * n as usize)
    }

    /// Operand word `w` as a value. `q16` selects the INLINE reading: a Q16.16 literal or a plain integer.
    #[inline(never)]
    pub fn value_of(&self, w: u16, q16: bool) -> i32 {
        let (tag, p) = f3::split(w);
        unsafe {
            match tag {
                f3::T_CELL => self.reg(0).cast::<i32>().add(p as usize).read_unaligned(),
                f3::T_INLINE => {
                    let v = f3::inline_value(p);
                    if q16 {
                        v << 16
                    } else {
                        v
                    }
                }
                f3::T_CONST => self.t.bank.pool(self.f, p as usize),
                f3::T_BIT => self.bit_at(p) as i32,
                _ => 0,
            }
        }
    }
    /// Operand `n` as a Q16.16 value (`q16`) or an integer.
    #[inline(always)]
    pub fn val(&self, n: u8, q16: bool) -> i32 {
        self.value_of(self.word(n), q16)
    }
    /// Operand `n` as a Q16.16 value.
    #[inline(always)]
    pub fn fixed(&self, n: u8) -> i32 {
        self.val(n, true)
    }
    /// Operand `n` as an integer.
    #[inline(always)]
    pub fn int(&self, n: u8) -> i32 {
        self.val(n, false)
    }
    /// Operands to skip before an action's own parameters: 1 when its owner-default parameter was
    /// cooked as an explicit object operand, 0 when it means the owner and the operand was dropped.
    #[inline(always)]
    pub fn skip(&self) -> u8 {
        (self.flags & f3::F_OWNER == 0) as u8
    }
    /// The entity an owner-default parameter names.
    #[inline(always)]
    pub fn target(&self) -> u8 {
        if self.flags & f3::F_OWNER != 0 {
            f3::ENT_SELF
        } else {
            self.obj(0)
        }
    }
    /// Whether operand `n` names nothing (the original "None" variable, an unset event).
    #[inline(always)]
    pub fn is_none(&self, n: u8) -> bool {
        f3::split(self.word(n)).0 == f3::T_NONE
    }

    /// Store `v` through operand `n` (a cell). A NONE operand discards it, as PlayMaker does.
    #[inline(never)]
    pub fn set(&mut self, n: u8, v: i32) {
        let (tag, p) = f3::split(self.word(n));
        if tag == f3::T_CELL {
            unsafe { self.reg(0).cast::<i32>().add(p as usize).write_unaligned(v) }
        }
    }

    /// Operand `n` as a boolean (a bit, or an INLINE 0/1).
    #[inline(never)]
    pub fn bit(&self, n: u8) -> bool {
        let (tag, p) = f3::split(self.word(n));
        if tag == f3::T_BIT {
            self.bit_at(p)
        } else {
            p != 0
        }
    }
    #[inline(never)]
    pub fn set_bit(&mut self, n: u8, v: bool) {
        let (tag, p) = f3::split(self.word(n));
        if tag == f3::T_BIT {
            unsafe {
                let byte = self.reg(2).add((p >> 3) as usize);
                *byte = (*byte & !(1 << (p & 7))) | ((v as u8) << (p & 7));
            }
        }
    }

    /// Operand `n` as an entity handle (NONE8 when unset).
    #[inline(never)]
    pub fn obj(&self, n: u8) -> u8 {
        let (tag, p) = f3::split(self.word(n));
        if tag == f3::T_OBJ {
            unsafe { *self.reg(3).add(p as usize) }
        } else {
            NONE8
        }
    }
    #[inline(never)]
    pub fn set_obj(&mut self, n: u8, handle: u8) {
        let (tag, p) = f3::split(self.word(n));
        if tag == f3::T_OBJ {
            unsafe { *self.reg(3).add(p as usize) = handle }
        }
    }

    /// Operand `n` as an event id (NONE8 when the event is unset).
    #[inline(never)]
    pub fn event(&self, n: u8) -> u8 {
        let (tag, p) = f3::split(self.word(n));
        if tag == f3::T_EVENT {
            p as u8
        } else {
            NONE8
        }
    }

    /// Ask for clip `id` from tick `from`; a later request this step comes after it.
    pub fn play(&mut self, id: u32, from: u16) {
        let e = self.env();
        let n = (e.play_count as usize).min(e.plays.len() - 1);
        e.plays[n] = (id, from);
        if (e.play_count as usize) < e.plays.len() {
            e.play_count += 1;
        }
    }

    /// The PlayerData field named by operand 0 (a save-record slot).
    #[inline(never)]
    pub fn pd_read(&mut self) -> i32 {
        let slot = self.int(0) as usize;
        let e = self.env();
        if e.pd.is_null() {
            0
        } else {
            // SAFETY: as in the writer, the slot is inside the reserve.
            unsafe { *e.pd.add(slot) }
        }
    }

    /// Operand word `w` (an array element) as a boolean.
    #[inline(never)]
    pub fn bit_of(&self, w: u16) -> bool {
        let (tag, p) = f3::split(w);
        if tag == f3::T_BIT {
            self.bit_at(p)
        } else {
            p != 0
        }
    }

    /// Operand word `w` (an array element) as an event id.
    #[inline(always)]
    pub fn event_of(&self, w: u16) -> u8 {
        let (tag, p) = f3::split(w);
        if tag == f3::T_EVENT {
            p as u8
        } else {
            NONE8
        }
    }

    /// Scratch bytes at `off` of this FSM, as a 32-bit word, a 16-bit half or a byte (stateful actions
    /// keep their timers here).
    #[inline(never)]
    pub fn scratch32(&self, off: u8) -> i32 {
        unsafe { self.reg(1).add(off as usize).cast::<i32>().read_unaligned() }
    }
    #[inline(never)]
    pub fn set_scratch32(&mut self, off: u8, v: i32) {
        unsafe { self.reg(1).add(off as usize).cast::<i32>().write_unaligned(v) }
    }
    #[inline(never)]
    pub fn scratch16(&self, off: u8) -> i16 {
        unsafe { self.reg(1).add(off as usize).cast::<i16>().read_unaligned() }
    }
    #[inline(never)]
    pub fn set_scratch16(&mut self, off: u8, v: i16) {
        unsafe { self.reg(1).add(off as usize).cast::<i16>().write_unaligned(v) }
    }
    #[inline(never)]
    pub fn scratch8(&self, off: u8) -> u8 {
        unsafe { *self.reg(1).add(off as usize) }
    }
    #[inline(never)]
    pub fn set_scratch8(&mut self, off: u8, v: u8) {
        unsafe { *self.reg(1).add(off as usize) = v }
    }

    /// The next 32-bit pseudo-random value (a Q16.16-friendly LCG, deterministic per tree).
    #[inline(never)]
    pub fn rand(&mut self) -> u32 {
        unsafe {
            let p = self.t.mem.cast::<u32>();
            let x = p.read().wrapping_mul(1664525).wrapping_add(1013904223);
            p.write(x);
            x
        }
    }

    /// Queue `event` for delivery: to every FSM of the tree that handles it (`target` = NONE8) or to
    /// one. `delay` ticks; 0 means once the current pass is over.
    #[inline(never)]
    pub fn send(&mut self, event: u8, target: u8, delay: u16) {
        if event == NONE8 {
            return;
        }
        unsafe {
            let n = *self.t.mem.add(4) as usize;
            if n >= self.t.bank.queue_cap() as usize {
                *self.t.mem.add(5) = (*self.t.mem.add(5)).saturating_add(1);
                return;
            }
            let e = self.t.mem.add(TREE_HEADER + QUEUE_ENTRY * n);
            *e = event;
            *e.add(1) = target;
            e.add(2).cast::<u16>().write_unaligned(delay);
            *self.t.mem.add(4) = (n + 1) as u8;
        }
    }

    /// Array operand `n`: its element count and the byte offset of the first element.
    #[inline(never)]
    pub fn array(&self, n: u8) -> (usize, usize) {
        let (_, p) = f3::split(self.word(n));
        self.t.bank.array(self.f, p as usize)
    }
    /// Element `i` of an array whose first element is at byte `first`.
    #[inline(always)]
    pub fn array_word(&self, first: usize, i: usize) -> u16 {
        self.t.bank.word(first + 2 * i)
    }

    pub fn env(&mut self) -> &mut Env {
        self.t.env
    }
}
