//! Greenpath's Hornet (Fungus1_04_boss): the original fight's FSMs as one boss tree, the scene objects they act on, and the
//! adapter surface the actor runtime calls (`Boss::step`). The corpse the death effects spawn is a second tree of the same
//! kind (`corpse`), run by the same struct.
//!
//! The data is the cooker's (host/hk-fsm `cook3::cook_hornet3`): the bank, the object table, and which object each FSM is
//! on. The caller owns what is native in the original: the Rigidbody2D solve against terrain, the colliders' overlap with
//! the Knight, the HealthManager, the sprite animators (it advances `Ent::clip`, counts `anim_left`, raises `anim_event`)
//! and everything a request asks for (`hornet_fight`).
use crate::fsm3::{Env, Tree, PH_ENTER, PH_FIXED, PH_UPDATE};
use crate::fsm3_world::{ClipInfo, EntInit, World, MAX_ENT};
use hk_format::fsm3::Bank;

/// One cooked boss tree.
pub struct Data {
    pub bank: &'static [u8],
    pub ents: &'static [EntInit],
    /// The object each FSM is on, by FSM.
    pub owners: &'static [u8],
    pub clips: &'static [ClipInfo],
    /// The sprite library each object's animator plays from (255: none), one byte per object.
    pub ent_lib: &'static [u8],
    /// The clip each animator starts on, by object.
    pub defaults: &'static [(u8, u32)],
    pub runtime: usize,
}

/// The fight: the boss object's subtree plus the Encounter FSM that wakes it.
pub mod fight {
    include!("../../../data/hornet3_fsm3.rs");
    pub static DATA: super::Data = super::Data { bank: &BANK, ents: &ENTS, owners: &OWNERS, clips: &CLIPS, ent_lib: &ENT_LIB, defaults: &DEFAULT_CLIPS, runtime: RUNTIME_BYTES };
}
/// `Corpse Hornet 1`: what the death effects spawn. Its first state writes `hornet1Defeated`, its last sends `HORNET LEAVE`.
pub mod corpse {
    include!("../../../data/hornet_corpse3_fsm3.rs");
    pub static DATA: super::Data = super::Data { bank: &BANK, ents: &ENTS, owners: &OWNERS, clips: &CLIPS, ent_lib: &ENT_LIB, defaults: &DEFAULT_CLIPS, runtime: RUNTIME_BYTES };
}
pub use fight::{ENTS, OWNERS};

/// Words of tree memory: the larger of the two trees (`fight`) plus one for the alignment slack.
const MEM_WORDS: usize = 232;
const _: () = assert!(fight::RUNTIME_BYTES / 4 + 1 <= MEM_WORDS && corpse::RUNTIME_BYTES / 4 + 1 <= MEM_WORDS);
const _: () = assert!(fight::ENTS.len() <= MAX_ENT && corpse::ENTS.len() <= MAX_ENT);

#[cfg_attr(target_arch = "mips", optimize(size))]
pub fn bank() -> Bank<'static> {
    // SAFETY: the cooker parses the bank it emits and the module transport hash-checks it.
    unsafe { Bank::from_verified(fight::DATA.bank) }
}

/// A boss tree in progress: its memory, its scene objects and the environment of the pass.
pub struct Boss {
    data: *const Data,
    mem: [u32; MEM_WORDS],
    pub world: World,
    pub env: Env,
    /// The FSM-visible PlayerData reserve (`HORNET_FIELDS`).
    pub pd: [i32; 8],
}
/// The fight's tree.
pub type Hornet = Boss;

impl Boss {
    #[cfg_attr(target_arch = "mips", optimize(size))]
    pub const fn blank() -> Self {
        Self::with(&fight::DATA)
    }
    #[cfg_attr(target_arch = "mips", optimize(size))]
    pub const fn blank_corpse() -> Self {
        Self::with(&corpse::DATA)
    }
    const fn with(data: &'static Data) -> Self {
        Self { data: data as *const Data, mem: [0; MEM_WORDS], world: World::new(), env: Env::new(), pd: [0; 8] }
    }
    #[cfg_attr(target_arch = "mips", optimize(size))]
    /// Point a zeroed tree at its data (the guest keeps trees in zeroed pool memory).
    pub fn bind(&mut self, data: &'static Data) {
        self.data = data as *const Data;
    }
    #[cfg_attr(target_arch = "mips", optimize(size))]
    pub fn data(&self) -> &'static Data {
        // SAFETY: only ever set from a 'static.
        unsafe { &*self.data }
    }
    #[cfg_attr(target_arch = "mips", optimize(size))]
    fn bank(&self) -> Bank<'static> {
        // SAFETY: as in `bank`.
        unsafe { Bank::from_verified(self.data().bank) }
    }
    #[cfg_attr(target_arch = "mips", optimize(size))]
    /// Lay the tree and its objects out in their authored state. The first `step` enters every active FSM's start state.
    pub fn init(&mut self, seed: u32) {
        self.world.set_clips(self.data().clips, self.data().ent_lib);
        self.world.load(self.data().ents);
        self.env = Env::new();
        self.env.world = &mut self.world;
        self.env.pd = self.pd.as_mut_ptr();
        for &(ent, clip) in self.data().defaults {
            let ticks = self.world.clip_ticks(ent, clip);
            if let Some(e) = self.world.get_mut(ent) {
                e.clip = clip;
                e.clip_from = 0;
                e.anim_left = ticks;
            }
        }
        // SAFETY: `mem` is large enough for either bank (`MEM_WORDS`), 4-aligned.
        let mut tree = unsafe { Tree::init(self.bank(), self.mem.as_mut_ptr().cast(), &mut self.env, seed) };
        crate::fsm3b::object::sync_tree(&mut tree);
    }
    #[cfg_attr(target_arch = "mips", optimize(size))]
    /// The tree with the environment attached, for one pass.
    pub fn tree(&mut self) -> Tree<'_> {
        self.env.world = &mut self.world;
        self.env.pd = self.pd.as_mut_ptr();
        // SAFETY: laid out by `init` for this bank.
        unsafe { Tree::attach(self.bank(), self.mem.as_mut_ptr().cast(), &mut self.env) }
    }
    #[cfg_attr(target_arch = "mips", optimize(size))]
    /// One simulation frame: the fixed steps owed, then the update pass.
    pub fn step(&mut self, fixed_steps: u32) {
        for _ in 0..fixed_steps {
            self.tree().step(PH_FIXED);
            self.integrate();
        }
        self.tree().step(PH_UPDATE);
    }
    #[cfg_attr(target_arch = "mips", optimize(size))]
    /// The physics step after the fixed-update scripts: every body but the boss's own moves by its velocity (the 50 Hz step).
    fn integrate(&mut self) {
        for i in 1..self.world.n as usize {
            let e = &mut self.world.ent[i];
            if e.on(crate::fsm3_world::flag::BODY) && e.on(crate::fsm3_world::flag::ACTIVE) {
                e.pos[0] += psx_math::int32::mul_shr_i32(e.vel[0], 1311, 16);
                e.pos[1] += psx_math::int32::mul_shr_i32(e.vel[1], 1311, 16);
            }
        }
    }
    #[cfg_attr(target_arch = "mips", optimize(size))]
    /// Enter the start states (the very first pass of a fresh tree).
    pub fn enter(&mut self) {
        self.tree().step(PH_ENTER);
    }
}
