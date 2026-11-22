//! The cooked FSM programs, running against the resident scene.
//!
//! `hk_sim::script3` runs the HKFSM03 banks `hk_fsm_cook` cooks from the scene FSMs, on the same kernel the
//! enemies use, and knows nothing about this game. This is the seam between them: the trigger overlaps, the
//! cooked fields' binding to PlayerData (`player_data.rs`) and the per-scene instance pool.
//!
//! The one service that needs care is `hero_trigger`. Unity raises
//! OnTriggerEnter2D from the physics step, not from the state that happens to
//! be listening, so the overlap bits are refreshed once per tick, here, before
//! any instance runs and whatever state each instance is in. An edge remembered
//! inside the executor would instead fire Enter every time a state that watches
//! a volume is re-entered with the Knight already standing in it.
use hk_sim::script::{Code, Op, State, Transition};
use hk_sim::script3::{Scripts, INSTANCE_COUNT};
#[path = "script_state.rs"]
mod script_state;
use script_state::{overlaps, Overlaps};

/// One cooked FSM definition, as `host/cook_scripts.py` still writes them. The guest runs the HKFSM03 banks
/// `hk_fsm_cook` cooks from the same source FSMs (`hk_sim::script3`); these tables are kept only so the
/// generated file keeps one shape until the Python script IR is retired, and the linker drops them.
#[allow(dead_code)]
pub struct ScriptDef {
    pub states: &'static [State],
    pub transitions: &'static [Transition],
    pub ops: &'static [Op],
    pub constants: &'static [i32],
    pub var_count: u16,
    pub start: u16,
    pub initial: &'static [i32],
    pub fields: &'static [u16],
}
/// One placement: which scene and which owner it is.
#[allow(dead_code)]
pub struct ScriptInstance {
    pub scene: u16,
    pub def: u16,
    pub var_base: u16,
    pub source_id: u32,
}
/// A trigger volume a `Trigger2dEvent` watches, as the world rectangle the hero body is tested against. The source
/// collider's AABB: exact for the boxes, which is all but two of them, and an outer bound for a polygon or circle.
pub struct ScriptVolume {
    pub scene: u16,
    pub source_id: u32,
    pub bounds: [i32; 4],
}
#[allow(dead_code)]
const _UNUSED_CODE: Option<Code> = None;
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/scripts.rs"));

const VOLUMES: usize = SCRIPT_VOLUMES.len();
const FIELDS: usize = SCRIPT_FIELD_NAMES.len();
/// The `player_data` field each cooked slot names, bound by its source name at
/// compile time. A bank that cooks a field the store has no row for fails the
/// build here: add the row to `player_data::NAMES` (append only).
pub const FIELD_MAP: [crate::player_data::Field; FIELDS] = {
    let mut map = [0; FIELDS];
    let mut i = 0;
    while i < FIELDS {
        map[i] = crate::player_data::find(SCRIPT_FIELD_NAMES[i]);
        assert!(map[i] < crate::player_data::COUNT, "a cooked script field has no player_data row");
        i += 1;
    }
    map
};
/// HKS4 and HKS5 records hold at most this many; `save::migrate` reads them.
const _: () = assert!(
    FIELDS <= crate::save::SCRIPT_FIELD_SLOTS,
    "the cooked script bank has outgrown the save record's reserve"
);
const _: () = assert!(
    INSTANCE_COUNT == SCRIPT_INSTANCES.len(),
    "the HKFSM03 script banks and host/cook_scripts.py chose different instances"
);
const fn same_instances() -> bool {
    let mut i = 0;
    while i < INSTANCE_COUNT {
        if hk_sim::script3::INSTANCES[i].0 != SCRIPT_INSTANCES[i].scene
            || hk_sim::script3::INSTANCES[i].1 != SCRIPT_INSTANCES[i].source_id
        {
            return false;
        }
        i += 1;
    }
    true
}
const _: () = assert!(same_instances(), "the HKFSM03 script banks list the instances in another order");

static mut RUNNING: Scripts = Scripts::new();
static mut INSIDE: Overlaps<VOLUMES> = Overlaps::new();
/// The kernel's view of PlayerData, in cooked slot order: filled from the store
/// before a tick and written back after one that wrote. Not a store of its own.
static mut STORE: [i32; FIELDS] = [0; FIELDS];

/// Instances ticked on the last tick, which is the resident scene's set.
#[no_mangle]
pub static mut HK_SCRIPT_ACTIVE: u32 = 0;
/// State changes taken by a script. The most direct evidence a program ran.
#[no_mangle]
pub static mut HK_SCRIPT_TRANSITIONS: u32 = 0;
/// PlayerData writes a script made.
#[no_mangle]
pub static mut HK_SCRIPT_WRITES: u32 = 0;
/// One bit per cooked field, set while that field is non-zero. The bit order is
/// SCRIPT_FIELD_NAMES, which .hkpsx/script-bank.json lists.
#[no_mangle]
pub static mut HK_SCRIPT_FLAGS: u32 = 0;
/// A tick on which a script hit a bound (a transition budget or a full event queue). Never expected; the counter is
/// here so a bank that trips one is visible rather than quiet.
#[no_mangle]
pub static mut HK_SCRIPT_HALTS: u32 = 0;
/// Native calls a script made. HKFSM03 has no native escape (every action is a typed handler), so this stays zero;
/// the symbol stays so the telemetry readers keep one shape.
#[no_mangle]
pub static mut HK_SCRIPT_NATIVE: u32 = 0;
/// Whether a loaded record's script fields were taken. Zero after a boot with an
/// HKS4 or HKS5 record whose field list is not this bank's, which the loader
/// refuses rather than mapping onto the wrong fields; an HKS6 record names them.
#[no_mangle]
pub static mut HK_SCRIPT_RESTORED: u32 = 0;

fn volume_of(scene: usize, source: u32) -> Option<usize> {
    SCRIPT_VOLUMES.iter().position(|v| v.scene as usize == scene && v.source_id == source)
}

/// Whether the loaded record's script fields were taken (`persist::load`).
pub fn boot(taken: bool) {
    unsafe { HK_SCRIPT_RESTORED = u32::from(taken) }
}

/// Clear the whole bank, for a new game or a session reset. The PlayerData a
/// script wrote is `persist::reset`'s to clear, with the rest of the store.
pub fn reset() {
    unsafe {
        (*(&raw mut RUNNING)).forget();
        STORE = [0; FIELDS];
        HK_SCRIPT_ACTIVE = 0;
        HK_SCRIPT_TRANSITIONS = 0;
        HK_SCRIPT_WRITES = 0;
        HK_SCRIPT_FLAGS = 0;
        HK_SCRIPT_HALTS = 0;
        HK_SCRIPT_NATIVE = 0;
        HK_SCRIPT_RESTORED = 0;
    }
}

/// One 60 Hz tick of the resident scene's scripts. `body` is the hero body
/// rectangle, the same one the tablet reader and the NPC range already use.
pub fn tick(scene: usize, body: [i32; 4]) {
    unsafe {
        let scripts = &mut *(&raw mut RUNNING);
        // The family code the scripts run is a room module: nothing runs until the room's has landed.
        if !crate::modules::loaded_all(crate::modules::FSM3_SCRIPTS3) {
            return;
        }
        if scripts.scene() != scene {
            // A scene load brings its FSMs up fresh and their colliders are created around wherever the Knight has
            // arrived, so the overlap the last scene left is forgotten.
            scripts.seat(scene);
            (*(&raw mut INSIDE)).clear();
        }
        // Every volume first, so an instance entering a state this tick reads
        // the same overlap as one that has been sitting in it.
        (*(&raw mut INSIDE)).refresh(|index| {
            let volume = &SCRIPT_VOLUMES[index];
            volume.scene as usize == scene && overlaps(volume.bounds, body)
        });
        let inside = &*(&raw const INSIDE);
        let pd = crate::player_data::store();
        for (slot, &field) in FIELD_MAP.iter().enumerate() {
            STORE[slot] = pd.get(field);
        }
        let report = scripts.tick(
            |i| {
                let Some(v) = volume_of(scene, SCRIPT_INSTANCES[i].source_id) else { return 0 };
                let mut bits = 0;
                for phase in 0..3u8 {
                    if inside.holds(v, phase) {
                        bits |= 1 << phase;
                    }
                }
                bits
            },
            &mut *(&raw mut STORE),
        );
        if report.writes != 0 {
            for (slot, &field) in FIELD_MAP.iter().enumerate() {
                pd.set(field, STORE[slot]);
            }
        }
        HK_SCRIPT_ACTIVE = report.active;
        HK_SCRIPT_TRANSITIONS += report.transitions;
        HK_SCRIPT_WRITES += report.writes;
        HK_SCRIPT_HALTS += report.faults;
        let mut flags = 0;
        for slot in 0..FIELDS.min(32) {
            flags |= u32::from(STORE[slot] != 0) << slot;
        }
        HK_SCRIPT_FLAGS = flags;
    }
}
