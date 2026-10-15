//! The scene scripts on the shared HKFSM03 kernel: the same banks and the same kernel the enemies run.
//!
//! An instance is bound to its source scene; only the resident scene's instances (at most `LIVE`) have a
//! runtime block. `tick` runs them once, in cooked order, against the hero's trigger overlap and the save
//! record's PlayerData reserve.
use crate::fsm3::{Block, Env, Tree, PH_UPDATE};
use hk_format::fsm3::Bank;
include!("../../../data/script_fsm3.rs");

pub const INSTANCE_COUNT: usize = INSTANCES.len();
const fn most_per_scene() -> usize {
    let mut best = 0;
    let mut i = 0;
    while i < INSTANCES.len() {
        let mut n = 0;
        let mut j = 0;
        while j < INSTANCES.len() {
            if INSTANCES[j].0 == INSTANCES[i].0 {
                n += 1;
            }
            j += 1;
        }
        if n > best {
            best = n;
        }
        i += 1;
    }
    best
}
/// Instances one scene can have live at once.
pub const LIVE: usize = most_per_scene();
const WORDS: usize = RUNTIME_BYTES / 4;

/// What one tick did.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Report {
    pub active: u32,
    pub transitions: u32,
    pub writes: u32,
    pub faults: u32,
}

pub struct Scripts {
    mem: [[u32; WORDS]; LIVE],
    /// Instance index of each live block.
    live: [u8; LIVE],
    count: u8,
    scene: usize,
}

fn bank(instance: usize) -> Bank<'static> {
    let (_, _, at, len) = INSTANCES[instance];
    // SAFETY: the cooker parses every bank it emits and the module transport hash-checks the bundle.
    unsafe { Bank::from_verified(&BANK[at as usize..at as usize + len as usize]) }
}

impl Scripts {
    pub const fn new() -> Self {
        Self { mem: [[0; WORDS]; LIVE], live: [0; LIVE], count: 0, scene: usize::MAX }
    }
    pub fn scene(&self) -> usize {
        self.scene
    }
    pub fn forget(&mut self) {
        self.scene = usize::MAX;
        self.count = 0;
    }
    /// Bring up every instance of `scene` in its start state.
    #[inline(never)]
    pub fn seat(&mut self, scene: usize) {
        self.scene = scene;
        self.count = 0;
        for (i, row) in INSTANCES.iter().enumerate() {
            if row.0 as usize != scene {
                continue;
            }
            let n = self.count as usize;
            let mut env = Env::new();
            // SAFETY: each block is the bank's runtime size or more, 4-aligned.
            unsafe { Tree::init(bank(i), self.mem[n].as_mut_ptr().cast(), &mut env, 1) };
            self.live[n] = i as u8;
            self.count += 1;
        }
    }
    pub fn live(&self) -> &[u8] {
        &self.live[..self.count as usize]
    }
    /// One 60 Hz tick of the resident scene's scripts. `trigger(instance)` is the hero's overlap bits with
    /// that instance's volume (entered, inside, left); `pd` is the PlayerData reserve.
    #[inline(never)]
    pub fn tick(&mut self, trigger: impl Fn(usize) -> u8, pd: &mut [i32]) -> Report {
        let mut report = Report::default();
        for n in 0..self.count as usize {
            let i = self.live[n] as usize;
            let mut env = Env::new();
            env.pd = pd.as_mut_ptr();
            env.trigger = trigger(i);
            // SAFETY: the block was laid out for this bank by `seat`.
            let mut tree = unsafe { Tree::attach(bank(i), self.mem[n].as_mut_ptr().cast(), &mut env) };
            let block = tree.block();
            let before = block.state(0);
            tree.step(PH_UPDATE);
            report.active += 1;
            report.transitions += (block.state(0) != before) as u32;
            report.faults += tree.faults() as u32;
            report.writes += env.pd_writes as u32;
        }
        report
    }
    /// State of live block `n` (tests).
    pub fn state(&mut self, n: usize) -> u8 {
        let i = self.live[n] as usize;
        // SAFETY: as above.
        unsafe { Block::new(bank(i), self.mem[n].as_mut_ptr().cast()) }.state(0)
    }
}
