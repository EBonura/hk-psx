//! Greenpath's Hornet (Fungus1_04_boss): the original fight's FSMs as one boss tree, the scene objects they act on, and the
//! adapter surface the actor runtime calls (`Hornet::step`).
//!
//! The data is the cooker's (host/hk-fsm `fsm3-boss`): the bank, the object table, and which object each FSM is on.
//! The caller owns what is native in the original: the Rigidbody2D solve against terrain, the colliders' overlap with the
//! Knight, the HealthManager, the sprite animators (it advances `Ent::clip`, counts `anim_left`, raises `anim_event`) and
//! everything a request asks for.
use crate::fsm3::{Env, Tree, PH_ENTER, PH_FIXED, PH_UPDATE};
use crate::fsm3_world::{World, MAX_ENT};
use hk_format::fsm3::Bank;
include!("../../../data/hornet3_fsm3.rs");

pub fn bank() -> Bank<'static> {
    // SAFETY: the cooker parses the bank it emits and the module transport hash-checks it.
    unsafe { Bank::from_verified(&BANK) }
}

/// A Hornet fight in progress: the tree's memory, its scene objects and the environment of the pass.
pub struct Hornet {
    mem: [u32; RUNTIME_BYTES / 4 + 1],
    pub world: World,
    pub env: Env,
    /// The FSM-visible PlayerData reserve (`HORNET_FIELDS`).
    pub pd: [i32; 8],
}

impl Hornet {
    pub const fn blank() -> Self {
        Self { mem: [0; RUNTIME_BYTES / 4 + 1], world: World::new(), env: Env::new(), pd: [0; 8] }
    }
    /// Lay the tree and its objects out in their authored state. The first `step` enters every active FSM's start state.
    pub fn init(&mut self, seed: u32) {
        self.world.load(&ENTS);
        self.env = Env::new();
        self.env.world = &mut self.world;
        self.env.pd = self.pd.as_mut_ptr();
        // SAFETY: `mem` is the bank's runtime size, 4-aligned.
        let mut tree = unsafe { Tree::init(bank(), self.mem.as_mut_ptr().cast(), &mut self.env, seed) };
        crate::fsm3b::object::sync_tree(&mut tree);
    }
    /// The tree with the environment attached, for one pass.
    pub fn tree(&mut self) -> Tree<'_> {
        self.env.world = &mut self.world;
        self.env.pd = self.pd.as_mut_ptr();
        // SAFETY: laid out by `init` for this bank.
        unsafe { Tree::attach(bank(), self.mem.as_mut_ptr().cast(), &mut self.env) }
    }
    /// One simulation frame: the fixed steps owed, then the update pass.
    pub fn step(&mut self, fixed_steps: u32) {
        for _ in 0..fixed_steps {
            self.tree().step(PH_FIXED);
            self.integrate();
        }
        self.tree().step(PH_UPDATE);
    }
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
    /// Enter the start states (the very first pass of a fresh tree).
    pub fn enter(&mut self) {
        self.tree().step(PH_ENTER);
    }
}
const _: () = assert!(ENTS.len() <= MAX_ENT);
