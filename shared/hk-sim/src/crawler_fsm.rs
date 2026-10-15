//! Each admitted Crawler runs its own original graph on the shared HKFSM03 kernel. The caller keeps
//! the body: terrain senses, recoil, damage and corpses.
use crate::fsm3::{Env, Tree, PH_UPDATE};
use hk_format::fsm3::Bank;
include!("../../../data/crawler_fsm3.rs");

#[derive(Clone, Copy)]
pub struct Crawler {
    mem: [u32; RUNTIME_BYTES / 4],
    last_real_tick: u32,
    graph: u8,
    clock_ready: bool,
}
const _: () = assert!(core::mem::size_of::<Crawler>() <= 88);

fn bank(graph: u8) -> Bank<'static> {
    let (_, _, start, len) = GRAPHS[graph as usize];
    // SAFETY: the cooker parses every bank it emits and the module transport verifies its hash.
    unsafe { Bank::from_verified(&BANK[start as usize..start as usize + len as usize]) }
}

impl Crawler {
    #[inline(never)]
    pub fn new(scene: usize, source: u32, seed: u32) -> Option<Self> {
        let graph = GRAPHS.iter().position(|(s, id, _, _)| *s as usize == scene && *id == source)? as u8;
        let mut me = Self { mem: [0; RUNTIME_BYTES / 4], last_real_tick: 0, graph, clock_ready: false };
        let mut env = Env::new();
        // SAFETY: `mem` is the bank's runtime size, 4-aligned.
        unsafe { Tree::init(bank(graph), me.mem.as_mut_ptr().cast(), &mut env, seed) };
        Some(me)
    }
    /// The state the graph is in.
    pub fn state(&mut self) -> u8 {
        let mut env = Env::new();
        // SAFETY: the block was laid out for this bank by `new`.
        unsafe { Tree::attach(bank(self.graph), self.mem.as_mut_ptr().cast(), &mut env) }.state(0)
    }
    #[inline(never)]
    pub fn elapsed_real_ticks(&mut self, now: u32) -> u16 {
        let delta = if self.clock_ready { now.wrapping_sub(self.last_real_tick) } else { 0 };
        self.last_real_tick = now;
        self.clock_ready = true;
        // Admitted Wait durations are bounded to ten seconds. Any longer
        // absence therefore expires the timer even after this saturation.
        delta.min(u16::MAX as u32) as u16
    }
    /// One 60 Hz step: the first one enters the start state.
    #[inline(never)]
    pub fn step(&mut self, env: &mut Env) {
        // SAFETY: the block was laid out for this bank by `new`.
        let mut tree = unsafe { Tree::attach(bank(self.graph), self.mem.as_mut_ptr().cast(), env) };
        tree.step(PH_UPDATE);
    }
    /// An external direction event (the Walker's own messages).
    pub fn event(&mut self, env: &mut Env, event: u8) {
        // SAFETY: as above.
        let mut tree = unsafe { Tree::attach(bank(self.graph), self.mem.as_mut_ptr().cast(), env) };
        tree.event(0, event);
    }
    pub fn die(&mut self) {
        let mut env = Env::new();
        // SAFETY: as above.
        unsafe { Tree::attach(bank(self.graph), self.mem.as_mut_ptr().cast(), &mut env) }.kill(0);
    }
    /// Scratch word `i` of the graph's one FSM (the running Wait's remaining time).
    pub fn scratch_word(&mut self, i: usize) -> i32 {
        let mut env = Env::new();
        // SAFETY: as above.
        unsafe { Tree::attach(bank(self.graph), self.mem.as_mut_ptr().cast(), &mut env) }.scratch_word(0, i)
    }
}
