//! The cooked FSM programs, running against the resident scene.
//!
//! `hk_sim::script` is the executor and knows nothing about this game;
//! `host/cook_scripts.py` is the cooker. This is the seam between them: the
//! instance pool, the variable pool, the PlayerData store and the `Host` the
//! executor calls back into.
//!
//! The one service that needs care is `hero_trigger`. Unity raises
//! OnTriggerEnter2D from the physics step, not from the state that happens to
//! be listening, so the overlap bits are refreshed once per tick, here, before
//! any instance runs and whatever state each instance is in. An edge remembered
//! inside the executor would instead fire Enter every time a state that watches
//! a volume is re-entered with the Knight already standing in it.
use hk_sim::script::{Code, Halt, Host, Instance, NativeCall, Op, Program, State, Transition};
#[path = "script_state.rs"] mod script_state;
use script_state::{overlaps, Overlaps};

/// One cooked FSM definition. Instances bind to it and own only their variables.
pub struct ScriptDef {
    pub states: &'static [State],
    pub transitions: &'static [Transition],
    pub ops: &'static [Op],
    pub constants: &'static [i32],
    pub var_count: u16,
    /// PlayMaker's start state, which is not always the first one authored.
    pub start: u16,
    pub initial: &'static [i32],
    /// This definition's PlayerData field indices, into the bank's store.
    pub fields: &'static [u16],
}
/// One placement: which definition, which scene, and where its variables sit.
pub struct ScriptInstance {
    pub scene: u16,
    pub def: u16,
    pub var_base: u16,
    pub source_id: u32,
}
/// A trigger volume a `HeroTrigger` op names, as the world rectangle the hero
/// body is tested against. The source collider's AABB: exact for the boxes,
/// which is all but two of them, and an outer bound for a polygon or circle.
pub struct ScriptVolume {
    pub scene: u16,
    pub source_id: u32,
    pub bounds: [i32; 4],
}
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/scripts.rs"));

const INSTANCES: usize = SCRIPT_INSTANCES.len();
const VOLUMES: usize = SCRIPT_VOLUMES.len();
const FIELDS: usize = SCRIPT_FIELD_NAMES.len();

static mut RUNNING: [Instance; INSTANCES] = [const { Instance::new() }; INSTANCES];
static mut VARS: [i32; SCRIPT_VARS] = [0; SCRIPT_VARS];
/// The PlayerData a script reads and writes. Session-scoped: the memory card
/// record does not carry it yet, so a flag a script sets is lost on a quit
/// where the source would have kept it.
static mut STORE: [i32; FIELDS] = [0; FIELDS];
static mut INSIDE: Overlaps<VOLUMES> = Overlaps::new();
/// The scene the pool is seated in; `usize::MAX` before the first tick.
static mut SEATED: usize = usize::MAX;

/// Instances ticked on the last tick, which is the resident scene's set.
#[no_mangle] pub static mut HK_SCRIPT_ACTIVE: u32 = 0;
/// State changes taken by a script. The most direct evidence a program ran.
#[no_mangle] pub static mut HK_SCRIPT_TRANSITIONS: u32 = 0;
/// PlayerData writes a script made.
#[no_mangle] pub static mut HK_SCRIPT_WRITES: u32 = 0;
/// One bit per cooked field, set while that field is non-zero. The bit order is
/// SCRIPT_FIELD_NAMES, which .hkpsx/script-bank.json lists.
#[no_mangle] pub static mut HK_SCRIPT_FLAGS: u32 = 0;
/// A tick that stopped on a bound rather than finishing. Never expected; the
/// counter is here so a bank that trips one is visible rather than quiet.
#[no_mangle] pub static mut HK_SCRIPT_HALTS: u32 = 0;
/// `Op::Native` reaching the host. The cooked bank emits none, so this staying
/// at zero is what says the bank and this module still agree.
#[no_mangle] pub static mut HK_SCRIPT_NATIVE: u32 = 0;

fn program(def: &ScriptDef) -> Program<'static> {
    Program { states: def.states, transitions: def.transitions, ops: def.ops,
              constants: def.constants, var_count: def.var_count }
}
fn volume_of(scene: usize, object: i32) -> Option<usize> {
    SCRIPT_VOLUMES.iter().position(|v| v.scene as usize == scene && v.source_id as i32 == object)
}

/// The game behind one running instance. `fields` is its own definition's, so
/// the field index an op carries stays local to the program that cooked it.
struct Game {
    scene: usize,
    fields: &'static [u16],
}
impl Host for Game {
    fn native(&mut self, _call: NativeCall) {
        // No compiled action reaches native code yet. Counting rather than
        // asserting keeps a future bank visible in telemetry instead of taking
        // the disc down on the first placement that needs one.
        unsafe { HK_SCRIPT_NATIVE += 1 }
    }
    fn player_data(&mut self, field: u16) -> i32 {
        match self.fields.get(field as usize) {
            Some(&slot) => unsafe { STORE[slot as usize] },
            None => 0,
        }
    }
    fn set_player_data(&mut self, field: u16, value: i32) {
        if let Some(&slot) = self.fields.get(field as usize) {
            unsafe {
                STORE[slot as usize] = value;
                HK_SCRIPT_WRITES += 1;
            }
        }
    }
    fn hero_trigger(&mut self, object: i32, phase: u8) -> bool {
        let Some(index) = volume_of(self.scene, object) else { return false };
        unsafe { INSIDE.holds(index, phase) }
    }
}

/// Seat every instance of `scene` on its authored start state and forget the
/// previous scene's overlap, which is what a scene load does in the original:
/// the FSMs come up fresh and their colliders are created around wherever the
/// Knight has arrived.
fn seat(scene: usize) {
    unsafe {
        SEATED = scene;
        for (index, instance) in SCRIPT_INSTANCES.iter().enumerate() {
            if instance.scene as usize != scene {
                continue;
            }
            let def = &SCRIPT_DEFS[instance.def as usize];
            RUNNING[index] = Instance::new();
            RUNNING[index].state = def.start;
            let base = instance.var_base as usize;
            for slot in 0..def.var_count as usize {
                VARS[base + slot] = def.initial.get(slot).copied().unwrap_or(0);
            }
        }
        INSIDE.clear();
    }
}

/// Clear the whole bank, for a new game or a session reset. The PlayerData a
/// script wrote goes with it, because nothing carries it across yet.
pub fn reset() {
    unsafe {
        SEATED = usize::MAX;
        for slot in 0..FIELDS {
            STORE[slot] = 0;
        }
        HK_SCRIPT_ACTIVE = 0;
        HK_SCRIPT_TRANSITIONS = 0;
        HK_SCRIPT_WRITES = 0;
        HK_SCRIPT_FLAGS = 0;
        HK_SCRIPT_HALTS = 0;
        HK_SCRIPT_NATIVE = 0;
    }
}

/// One 60 Hz tick of the resident scene's scripts. `body` is the hero body
/// rectangle, the same one the tablet reader and the NPC range already use.
pub fn tick(scene: usize, body: [i32; 4]) {
    unsafe {
        if SEATED != scene {
            seat(scene);
        }
        // Every volume first, so an instance entering a state this tick reads
        // the same overlap as one that has been sitting in it.
        INSIDE.refresh(|index| {
            let volume = &SCRIPT_VOLUMES[index];
            volume.scene as usize == scene && overlaps(volume.bounds, body)
        });
        let mut active = 0;
        for (index, instance) in SCRIPT_INSTANCES.iter().enumerate() {
            if instance.scene as usize != scene {
                continue;
            }
            active += 1;
            let def = &SCRIPT_DEFS[instance.def as usize];
            let base = instance.var_base as usize;
            let before = RUNNING[index].state;
            let mut host = Game { scene, fields: def.fields };
            let vars = &mut VARS[base..base + def.var_count as usize];
            if RUNNING[index].tick(program(def), vars, &mut host) != Halt::Ran {
                HK_SCRIPT_HALTS += 1;
            }
            if RUNNING[index].state != before {
                HK_SCRIPT_TRANSITIONS += 1;
            }
        }
        HK_SCRIPT_ACTIVE = active;
        let mut flags = 0;
        for slot in 0..FIELDS.min(32) {
            flags |= u32::from(STORE[slot] != 0) << slot;
        }
        HK_SCRIPT_FLAGS = flags;
    }
}
