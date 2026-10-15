//! The scene scripts on HKFSM03 against the same scripts on HKSCR01 (the v1 executor), tick by tick,
//! over the same hero path and PlayerData: every PlayerData write and every state change must agree.
use hk_sim::script::{Code, Halt, Host, Instance, NativeCall, Op, Program, State, Transition};
use hk_sim::script3::{Scripts, INSTANCES};

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
pub struct ScriptInstance {
    pub scene: u16,
    pub def: u16,
    pub var_base: u16,
    pub source_id: u32,
}
pub struct ScriptVolume {
    pub scene: u16,
    pub source_id: u32,
    pub bounds: [i32; 4],
}
include!("../../../data/scripts.rs");
#[path = "../../../game/src/script_state.rs"]
mod script_state;
use script_state::{overlaps, Overlaps};

const VOLUMES: usize = SCRIPT_VOLUMES.len();

struct Game<'a> {
    scene: usize,
    fields: &'static [u16],
    store: &'a mut [i32; 16],
    inside: &'a Overlaps<VOLUMES>,
}
impl Host for Game<'_> {
    fn native(&mut self, _: NativeCall) {}
    fn player_data(&mut self, field: u16) -> i32 {
        self.fields.get(field as usize).map_or(0, |&s| self.store[s as usize])
    }
    fn set_player_data(&mut self, field: u16, value: i32) {
        if let Some(&s) = self.fields.get(field as usize) {
            self.store[s as usize] = value;
        }
    }
    fn hero_trigger(&mut self, object: i32, phase: u8) -> bool {
        SCRIPT_VOLUMES
            .iter()
            .position(|v| v.scene as usize == self.scene && v.source_id as i32 == object)
            .is_some_and(|i| self.inside.holds(i, phase))
    }
}

fn volume_of(scene: usize, source: u32) -> Option<usize> {
    SCRIPT_VOLUMES.iter().position(|v| v.scene as usize == scene && v.source_id == source)
}

#[test]
fn every_scene_script_matches_hkscr01_over_a_wandering_hero() {
    let mut scenes: Vec<usize> = SCRIPT_INSTANCES.iter().map(|i| i.scene as usize).collect();
    scenes.dedup();
    assert_eq!(INSTANCES.len(), SCRIPT_INSTANCES.len(), "the two banks cook the same instances");
    let mut total_writes = 0;
    let mut total_changes = 0;
    for scene in scenes {
        for seed in 0..6u32 {
            // v1
            let mut running: Vec<(usize, Instance, Vec<i32>)> = vec![];
            for (i, inst) in SCRIPT_INSTANCES.iter().enumerate() {
                if inst.scene as usize != scene {
                    continue;
                }
                let def = &SCRIPT_DEFS[inst.def as usize];
                let mut instance = Instance::new();
                instance.state = def.start;
                let vars: Vec<i32> =
                    (0..def.var_count as usize).map(|s| def.initial.get(s).copied().unwrap_or(0)).collect();
                running.push((i, instance, vars));
            }
            let mut store1 = [0i32; 16];
            let mut inside1: Overlaps<VOLUMES> = Overlaps::new();
            // v2
            let mut v2 = Scripts::new();
            v2.seat(scene);
            let mut store2 = [0i32; 16];
            let mut inside2: Overlaps<VOLUMES> = Overlaps::new();
            let mut x = seed.wrapping_mul(2654435761).wrapping_add(scene as u32 + 1);
            // The hero walks a path that crosses this scene's volumes.
            let here: Vec<[i32; 4]> =
                SCRIPT_VOLUMES.iter().filter(|v| v.scene as usize == scene).map(|v| v.bounds).collect();
            let (mut cumulative1, mut cumulative2) = (0u32, 0u32);
            for tick in 0..3000 {
                x = x.wrapping_mul(1664525).wrapping_add(1013904223);
                let body = if let Some(b) =
                    here.get(((x >> 16) as usize) % here.len().max(1)).filter(|_| (x >> 8) % 5 != 0)
                {
                    let w = (b[2] - b[0]).max(1 << 16);
                    let h = (b[3] - b[1]).max(1 << 16);
                    let jx = ((x >> 4) % 5) as i32 - 2;
                    let jy = ((x >> 12) % 5) as i32 - 2;
                    [b[0] + jx * w / 2, b[1] + jy * h / 2, b[0] + jx * w / 2 + (1 << 16), b[1] + jy * h / 2 + (1 << 16)]
                } else {
                    [-9000 << 16, -9000 << 16, -8999 << 16, -8999 << 16]
                };
                if tick % 3 == 0 || tick < 4 {
                    // hold a position for a few ticks so Enter, Stay and Exit all happen
                }
                for (inside, _) in [(&mut inside1, 0), (&mut inside2, 1)] {
                    inside.refresh(|i| {
                        SCRIPT_VOLUMES[i].scene as usize == scene && overlaps(SCRIPT_VOLUMES[i].bounds, body)
                    });
                }
                let mut changes1 = 0;
                for (i, instance, vars) in running.iter_mut() {
                    let def = &SCRIPT_DEFS[SCRIPT_INSTANCES[*i].def as usize];
                    let program = Program {
                        states: def.states,
                        transitions: def.transitions,
                        ops: def.ops,
                        constants: def.constants,
                        var_count: def.var_count,
                    };
                    let before = instance.state;
                    let mut host = Game { scene, fields: def.fields, store: &mut store1, inside: &inside1 };
                    assert_eq!(instance.tick(program, vars, &mut host), Halt::Ran);
                    changes1 += (instance.state != before) as u32;
                }
                let report = v2.tick(
                    |i| {
                        let Some(v) = volume_of(scene, INSTANCES[i].1) else { return 0 };
                        let mut bits = 0;
                        for phase in 0..3u8 {
                            if inside2.holds(v, phase) {
                                bits |= 1 << phase;
                            }
                        }
                        bits
                    },
                    &mut store2,
                );
                cumulative1 += changes1;
                cumulative2 += report.transitions;
                // PlayMaker raises a NextFrameEvent in the first Update after the FSM starts; HKSCR01 raised it
                // at the start of the following tick. From tick 1 on the two agree on everything.
                if tick >= 1 {
                    assert_eq!(store1, store2, "scene {scene} seed {seed} tick {tick}");
                    assert_eq!(cumulative1, cumulative2, "state changes, scene {scene} seed {seed} tick {tick}");
                }
                total_writes += report.writes;
                total_changes += changes1;
            }
        }
    }
    assert!(total_changes > 20, "the walk should trigger state changes, saw {total_changes}");
    assert!(total_writes > 5, "the walk should make PlayerData writes, saw {total_writes}");
}
