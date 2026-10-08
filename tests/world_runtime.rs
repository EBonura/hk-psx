//! Host-only world integration tests; compile with the shared host rlibs.
extern crate hk_format;
extern crate hk_sim;
const KNIGHT_SCALE:i32=60693;
#[path="../game/src/impact.rs"]
mod impact;
mod render {
    pub const SOURCE_ALPHA_COVERAGE:u8=1;
    pub fn texture_material_alpha(_:usize,_:[(i16,i16);4],_:(u8,u8,u8),_:u8){}
    pub fn texture(_:usize,_:[(i16,i16);4],_:(u8,u8,u8)) {}
    use std::sync::Mutex;
    pub static GAINS: Mutex<Vec<(usize, u8)>> = Mutex::new(Vec::new());
    pub static TEST_LOCK: Mutex<()> = Mutex::new(());
    pub fn reset_visibility() {
        GAINS.lock().unwrap().clear();
    }
    pub fn set_visible(_: usize, _: bool) {}
    pub fn set_gain(draw: usize, gain: u8) {
        GAINS.lock().unwrap().push((draw, gain));
    }
    pub fn set_opacity(_: usize, _: u8) {}
    pub fn black_mask(_: usize) -> bool { false }
    pub fn draw_is_front(_: usize) -> bool { false }
    pub fn draw_scenery_offset(_: usize, _: [i32; 2], _: (i32, i32)) -> u32 { 1 }
    pub fn draw_scenery_quad(_: usize, _: [[i32; 2]; 4], _: (i32, i32)) -> u32 { 1 }
}
#[path = "../game/src/secret_breaks.rs"]
mod secret_breaks;
#[path = "../game/src/reveal_masks.rs"]
mod reveal_masks;
#[path = "../game/src/world.rs"]
mod world;
use hk_sim::{AttackParams, Nail, Params, Player};
use world::{Region, State};
static FIXTURE_BANK: &[u8] = include_bytes!(env!("HK_TEST_FIXTURE_BANK"));
static SCENE0_BANK: &[u8] = include_bytes!(env!("HK_TEST_SCENE0_BANK"));
/// Select the bank world.rs reads on this test thread.
fn use_bank(bytes: &'static [u8]) {
    world::TEST_BANK.with(|bank| bank.set(bytes));
}
/// Runtime Region values of every scene 0 slot, from the cooked bank.
fn scene0_regions() -> Vec<(usize, Region)> {
    use_bank(SCENE0_BANK);
    let bank = hk_format::WorldMeta::parse(SCENE0_BANK).unwrap();
    bank.regions().map(|m| {
        let index = m.global_id() as usize - 1;
        (index, world::resident(index).expect("scene 0 region resident"))
    }).collect()
}
fn scene0_region(global_id: usize) -> Region {
    scene0_regions().into_iter().find(|(_, r)| r.global_id == global_id).unwrap().1
}
/// (id, persistent, fade_ticks) of every scene 0 breakable in the cooked bank.
fn scene0_breakables() -> Vec<(usize, bool, u16)> {
    let bank = hk_format::WorldMeta::parse(SCENE0_BANK).unwrap();
    bank.regions().flat_map(world::breakables).map(|b| (b.id(), b.persistent(), b.fade_ticks())).collect()
}
const PARAMS: AttackParams = AttackParams {
    duration: 21,
    cooldown: 25,
    alternate_reset: 30,
    hit_start: 2,
    hit_end: 6, ..AttackParams::ZERO };
const TRIANGLE: &[[i32; 2]] = &[[100, 0], [200, 0], [100, 100]];
const SQUARE: &[[i32; 2]] = &[[100, 0], [200, 0], [200, 100], [100, 100]];
// FIXTURE_BANK (tools/test_world.py): region 1 holds breakable 0 (TRIANGLE,
// fade draws [5] for 60 ticks) and breakable 1 (SQUARE, no fade), both
// persistent door-sound objects with off [1], on [2], edges [1]; region 2 is empty.
fn region() -> Region {
    use_bank(FIXTURE_BANK);
    Region {
        global_id: 1,
        scene: 0,
        bounds: [0, 0, 1000, 1000],
        collision_bounds: [0, 0, 1000, 1000],
        camera: [0, 0, 1000, 1000],
        grass_impact:None,
    door_debris:&[],
    particle_bank:None,
    grass_emitters:&[],
    }
}
#[test]
fn exact_source_polygon_rejects_box_false_positive_and_breaks_once() {
    let mut state = State::new();
    let mut nail = Nail::new();
    nail.active = true;
    nail.age = 2;
    let player = Player::spawn(0, 0);
    let attack = &[[-185, 80], [-180, 80], [-180, 85], [-185, 85]][..];
    let hit = state.strike(&region(), &nail, PARAMS, [attack; 4], &player, 0, [0; 4]);
    assert_eq!((hit.broken, hit.door_sounds), (1, 1));
    assert!(!state.broken(0));
    assert!(state.broken(1));
    assert_eq!(
        state
            .strike(&region(), &nail, PARAMS, [attack; 4], &player, 0, [0; 4])
            .broken,
        0
    );
    nail.age = 6;
    assert_eq!(
        state
            .strike(&region(), &nail, PARAMS, [SQUARE; 4], &player, 0, [0; 4])
            .broken,
        0
    );
}
#[test]
fn source_fade_advances_once_without_restarting_on_repeat_hit() {
    let _lock = render::TEST_LOCK.lock().unwrap();
    let mut state = State::new();
    assert!(state.break_object(0, 60));
    for _ in 0..30 {
        state.tick();
    }
    assert!(!state.break_object(0, 60));
    assert_eq!(state.fade_left[0], 30);
    let r = region();
    state.apply(&r);
    assert_eq!(*render::GAINS.lock().unwrap(), vec![(5, 64)]);
    for _ in 0..30 {
        state.tick();
    }
    state.apply(&r);
    assert_eq!(*render::GAINS.lock().unwrap(), vec![(5, 0)]);
}
#[test]
fn actual_generated_persistence_survives_scene_reset_and_grass_is_transient() {
    use_bank(SCENE0_BANK);
    let all = scene0_breakables();
    let (persistent, _, fade) = *all.iter().find(|b| b.1).unwrap();
    let (temporary, _, _) = *all.iter().find(|b| !b.1).unwrap();
    let scene = persistent / 128;
    let mut state = State::new();
    state.break_object(persistent, fade);
    state.break_object(temporary, 0);
    // Broken bits of other scenes persist across this scene's reset; grass
    // is resident-scene state and every cut clears with the reset.
    state.break_object((1 - scene) * 128 + 5, 0);
    state.cut_grass(scene * 1024 + 1023);
    state.cut_grass(scene * 1024 + 7);
    state.reset_scene(scene);
    assert!(state.broken(persistent));
    assert!(!state.broken(temporary));
    assert!(state.broken((1 - scene) * 128 + 5));
    assert!(!state.cut(scene * 1024 + 1023));
    assert!(!state.cut(scene * 1024 + 7));
}
#[test]
fn only_owned_edges_disappear() {
    let mut bytes = vec![0u8; 40 + 16 + 2 * 16 + 32 + 32768];
    bytes[..8].copy_from_slice(b"HKROOM02");
    bytes[8] = 1;
    bytes[12] = 1;
    bytes[28] = 2;
    bytes[46] = 1;
    bytes[48] = 1;
    for (i, e) in [[1i32, 2, 3, 2], [4, 5, 4, 6]].iter().enumerate() {
        for (j, v) in e.iter().enumerate() {
            let p = 56 + i * 16 + j * 4;
            bytes[p..p + 4].copy_from_slice(&v.to_le_bytes());
        }
    }
    let room = hk_format::Room::parse(&bytes).unwrap();
    let mut state = State::new();
    region();
    state.break_object(0, 60);
    assert_eq!(state.edge(&region(), &room, 0), [1, 2, 3, 2]);
    assert_eq!(state.edge(&region(), &room, 1), [0; 4]);
}

#[test]
fn one_pass_edge_fill_matches_per_edge_filter() {
    let mut bytes = vec![0u8; 40 + 16 + 2 * 16 + 32 + 32768];
    bytes[..8].copy_from_slice(b"HKROOM02");
    bytes[8] = 1;
    bytes[12] = 1;
    bytes[28] = 2;
    bytes[46] = 1;
    bytes[48] = 1;
    for (i, e) in [[1i32, 2, 3, 2], [4, 5, 4, 6]].iter().enumerate() {
        for (j, v) in e.iter().enumerate() {
            let p = 56 + i * 16 + j * 4;
            bytes[p..p + 4].copy_from_slice(&v.to_le_bytes());
        }
    }
    let room = hk_format::Room::parse(&bytes).unwrap();
    let filled = |state: &mut State| {
        let mut cache = [[0i32; 4]; 2];
        state.fill_edges(&region(), &room, &mut cache);
        let want: Vec<_> = (0..2).map(|i| state.edge(&region(), &room, i)).collect();
        assert_eq!(cache.to_vec(), want, "direct path");
        // The per-tick refreshed table must agree, and every mutator must
        // invalidate it so a stale table never answers edge().
        state.refresh_edges(&region(), &room);
        let cached: Vec<_> = (0..2).map(|i| state.edge(&region(), &room, i)).collect();
        assert_eq!(cached, want, "cached path");
        cached
    };
    let mut state = State::new();
    assert_eq!(filled(&mut state), vec![[1, 2, 3, 2], [4, 5, 4, 6]]);
    state.set_geo_edges(&[0, 9]);
    assert_eq!(state.edge(&region(), &room, 0), [0; 4], "set_geo_edges invalidates");
    assert_eq!(filled(&mut state), vec![[0; 4], [4, 5, 4, 6]]);
    state.break_object(0, 60);
    assert_eq!(state.edge(&region(), &room, 1), [0; 4], "break_object invalidates");
    assert_eq!(filled(&mut state), vec![[0; 4], [0; 4]]);
    let mut state = State::new();
    state.set_lifeblood_edges(&[1, 300]);
    assert_eq!(filled(&mut state), vec![[1, 2, 3, 2], [0; 4]]);
    state.set_lifeblood_edges(&[]);
    assert_eq!(state.edge(&region(), &room, 1), [4, 5, 4, 6], "set_lifeblood_edges invalidates");
    state.set_lifeblood_edges(&[1]);
    filled(&mut state);
    state.append_script_edges(&[0]);
    assert_eq!(state.edge(&region(), &room, 0), [0; 4], "scripted door invalidates cached collision");
    assert_eq!(filled(&mut state), vec![[0; 4], [0; 4]], "door and Lifeblood exclusions coexist");
    for _ in 0..16 {state.append_script_edges(&[0]);}
    assert_eq!(filled(&mut state), vec![[0; 4], [0; 4]], "repeated frame application does not consume capacity");
    state.set_lifeblood_edges(&[]);
    state.append_script_edges(&[]);
    assert_eq!(filled(&mut state), vec![[1, 2, 3, 2], [4, 5, 4, 6]], "region refresh clears prior exclusions");
}

// Minimal source room used to exercise cache identity independently of the
// current cooker tables. Distinct payloads can deliberately have equal counts.
fn edge_room_bytes(edges:&[[i32;4]])->Vec<u8> {
    let mut bytes=vec![0u8;40+16+edges.len()*16+32+32768];
    bytes[..8].copy_from_slice(b"HKROOM02");bytes[8]=1;bytes[12]=1;
    bytes[28..32].copy_from_slice(&(edges.len()as u32).to_le_bytes());
    bytes[46]=1;bytes[48]=1;
    for(i,e)in edges.iter().enumerate(){for(j,v)in e.iter().enumerate(){
        let p=56+i*16+j*4;bytes[p..p+4].copy_from_slice(&v.to_le_bytes());
    }}
    bytes
}

#[test]
fn edge_cache_reuses_immutable_source_and_invalidates_only_actual_mutations() {
    let bytes=edge_room_bytes(&[[1,2,3,2],[4,5,4,6]]);
    let room=hk_format::Room::parse(&bytes).unwrap();
    let r=region();let mut state=State::new();
    assert!(state.refresh_edges(&r,&room));
    for _ in 0..60 {
        state.set_geo_edges(&[]);state.set_lifeblood_edges(&[]);state.append_script_edges(&[]);
        assert!(!state.refresh_edges(&r,&room),"unchanged controllers retain terrain");
    }
    state.set_geo_edges(&[0]);assert!(state.refresh_edges(&r,&room));
    state.set_geo_edges(&[0]);assert!(!state.refresh_edges(&r,&room));
    state.set_lifeblood_edges(&[1]);assert!(state.refresh_edges(&r,&room));
    state.set_lifeblood_edges(&[1]);state.append_script_edges(&[1]);
    assert!(!state.refresh_edges(&r,&room),"duplicate scripted binding is unchanged");
    state.set_lifeblood_edges(&[]);assert!(state.refresh_edges(&r,&room));
    state.append_script_edges(&[1]);assert!(state.refresh_edges(&r,&room));
    assert_eq!(state.edge(&r,&room,1),[0;4]);
    state.break_object(0, 60);assert!(state.refresh_edges(&r,&room));
    assert!(!state.break_object(0, 60));assert!(!state.refresh_edges(&r,&room));
    state.reset_scene(0);assert!(state.refresh_edges(&r,&room));
}

#[test]
fn edge_cache_distinguishes_equal_count_sources_regions_and_caches_oversized_room_prefix() {
    let a=edge_room_bytes(&[[1,2,3,2],[4,5,4,6]]);
    let b=edge_room_bytes(&[[11,12,13,12],[14,15,14,16]]);
    let a=hk_format::Room::parse(&a).unwrap();let b=hk_format::Room::parse(&b).unwrap();
    let r=region();let mut other=region();other.global_id=2;
    let mut state=State::new();state.break_object(0, 60);
    assert!(state.refresh_edges(&r,&a));assert_eq!(state.edge(&r,&a,1),[0;4]);
    // A source switch must never answer with the old cache, even before refresh.
    assert_eq!(state.edge(&r,&b,0),[11,12,13,12]);
    assert!(state.refresh_edges(&r,&b));assert!(!state.refresh_edges(&r,&b));
    // Same scene bytes, different authored local bindings.
    assert_eq!(state.edge(&other,&b,1),[14,15,14,16]);
    assert!(state.refresh_edges(&other,&b));assert!(!state.refresh_edges(&other,&b));
    // 129 is Crossroads_10's chunk 237, the one cooked view in the catalogue
    // past the cap. It must still get a table for its first EDGE_CACHE edges:
    // refusing one made every terrain query rescan the bank's breakables, which
    // lost two VBlanks of pad service a tick and overflowed the queue on the disc.
    let edges=vec![[101,102,103,102];129];let big=edge_room_bytes(&edges);
    let big=hk_format::Room::parse(&big).unwrap();
    assert!(state.refresh_edges(&other,&big));
    assert!(!state.refresh_edges(&other,&big),"an oversized room caches its prefix once");
    for i in 0..129 {assert_eq!(state.edge(&other,&big,i),edges[i]);}
    assert!(state.refresh_edges(&r,&a),"return from the oversized room rebuilds source");
}

#[test]
fn hazards_and_checkpoints_use_polygons_and_preserve_distant_spawn() {
    const P: hk_sim::Params = hk_sim::Params { half_width: 2, bottom: -2, top: 2, ..Params::ZERO };
    // One-region HKWMTA01 bank: a hazard and a checkpoint sharing TRIANGLE.
    fn put(b: &mut [u8], at: usize, v: i32) { b[at..at + 4].copy_from_slice(&v.to_le_bytes()); }
    let (regions, objects, polygons, points) = (160usize, 236usize, 332usize, 348usize);
    let mut bank = vec![0u8; 348 + 6 * 8];
    bank[..8].copy_from_slice(b"HKWMTA01");
    let total = bank.len() as i32;
    put(&mut bank, 52, total);
    put(&mut bank, 60, 5);
    for (i, (offset, count, stride)) in [(regions, 1, 76), (objects, 2, 48), (polygons, 2, 8), (points, 6, 8), (total as usize, 0, 2)].iter().enumerate() {
        put(&mut bank, 64 + i * 12, *offset as i32);
        put(&mut bank, 68 + i * 12, *count);
        put(&mut bank, 72 + i * 12, *stride);
    }
    put(&mut bank, regions, 1);
    put(&mut bank, regions + 64, 2);
    put(&mut bank, regions + 72, 2);
    for (o, (kind, extra)) in [(3i32, [150, 1 | 1 << 16, 0]), (4, [9000, 3000, -1])].iter().enumerate() {
        let at = objects + o * 48;
        bank[at + 8..at + 10].copy_from_slice(&(*kind as u16).to_le_bytes());
        for (i, v) in [100, 0, 200, 100].iter().enumerate() { put(&mut bank, at + 12 + i * 4, *v); }
        put(&mut bank, at + 28, o as i32);
        put(&mut bank, at + 32, 1);
        for (i, v) in extra.iter().enumerate() { put(&mut bank, at + 36 + i * 4, *v); }
        put(&mut bank, polygons + o * 8, (o * 3) as i32);
        put(&mut bank, polygons + o * 8 + 4, 3);
        for (i, p) in TRIANGLE.iter().enumerate() {
            put(&mut bank, points + (o * 3 + i) * 8, p[0]);
            put(&mut bank, points + (o * 3 + i) * 8 + 4, p[1]);
        }
    }
    let meta = hk_format::WorldMeta::parse(&bank).unwrap();
    let r = meta.region(0).unwrap();
    let missed = Player::spawn(185, 85);
    assert_eq!(world::hazard_contact(r, &missed, P), None);
    assert_eq!(world::checkpoint(r, &missed, P), None);
    let hit = Player::spawn(115, 20);
    assert_eq!(world::hazard_contact(r, &hit, P), Some((1, true, -1)));
    assert_eq!(world::checkpoint(r, &hit, P), Some(([9000, 3000], -1)));
    let mut state = State::new();
    assert_eq!(state.hazard_contact(r, &missed, P, |_| false), None);
    assert_eq!(state.checkpoint(r, &missed, P), None);
    assert_eq!(state.hazard_contact(r, &hit, P, |_| false), Some((1, true, -1)));
    assert_eq!(state.checkpoint(r, &hit, P), Some(([9000, 3000], -1)));
}

/// The per-region trigger table answers exactly what the whole-region scan
/// does, on every cooked disc bank, around every hazard and checkpoint and
/// across region changes and admissions.
#[test]
fn cached_triggers_match_the_region_scan_on_every_disc_bank() {
    let params = movement_params();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.hkpsx/world-metadata-packed");
    let mut banks: Vec<_> = std::fs::read_dir(&dir).expect("cooked disc banks").flatten()
        .map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "hkwm")).collect();
    banks.sort();
    assert!(!banks.is_empty());
    let mut state = State::new();
    let (mut probes, mut hazards, mut checkpoints) = (0u32, 0u32, 0u32);
    for path in &banks {
        let bytes = std::fs::read(path).unwrap();
        let meta = hk_format::WorldMeta::parse(&bytes).unwrap();
        state.begin_world_admission();
        // Two passes in opposite region order so every region is rebound
        // after another region's table.
        for pass in 0..2 {
            for i in 0..meta.region_count() {
                let r = meta.region(if pass == 0 { i } else { meta.region_count() - 1 - i }).unwrap();
                let mut spots = Vec::new();
                for o in r.objects().flatten().filter(|o| o.kind() == world::META_HAZARD || o.kind() == world::META_CHECKPOINT) {
                    let b = o.bounds();
                    for sx in 0..9 { for sy in 0..9 {
                        let x = b[0] - hk_sim::ONE + (b[2] - b[0] + 2 * hk_sim::ONE) / 8 * sx;
                        let y = b[1] - hk_sim::ONE + (b[3] - b[1] + 2 * hk_sim::ONE) / 8 * sy;
                        spots.push((x, y));
                    }}
                }
                let b = r.bounds();
                for sx in 0..5 { for sy in 0..5 { spots.push((b[0] + (b[2] - b[0]) / 4 * sx, b[1] + (b[3] - b[1]) / 4 * sy)); } }
                for (x, y) in spots {
                    let player = Player::spawn(x, y);
                    let hazard = world::hazard_contact(r, &player, params);
                    let checkpoint = world::checkpoint(r, &player, params);
                    assert_eq!(state.hazard_contact(r, &player, params, |_| false), hazard, "{} region {} at {x},{y}", path.display(), r.global_id());
                    assert_eq!(state.checkpoint(r, &player, params), checkpoint, "{} region {} at {x},{y}", path.display(), r.global_id());
                    probes += 1;
                    hazards += u32::from(hazard.is_some());
                    checkpoints += u32::from(checkpoint.is_some());
                }
            }
        }
    }
    assert!(hazards > 0 && checkpoints > 0, "{probes} probes: {hazards} hazard and {checkpoints} checkpoint hits");
}

/// `State::gate`'s decoded list answers what the bank scan does, around every
/// gate of every cooked disc bank, for both facings, UP, recoil and the
/// collider delay, across admissions.
#[test]
fn cached_gates_match_the_bank_scan() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.hkpsx/world-metadata-packed");
    let mut banks: Vec<_> = std::fs::read_dir(&dir).expect("cooked disc banks").flatten()
        .map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "hkwm")).collect();
    banks.sort();
    let mut state = State::new();
    let key = |g: world::Gate| (g.target_scene, g.target_region, g.bounds, g.spawn, g.entry_vy, g.side, g.delay_ticks);
    let (mut probes, mut taken) = (0u32, 0u32);
    for path in &banks {
        let bytes: &'static [u8] = Box::leak(std::fs::read(path).unwrap().into_boxed_slice());
        use_bank(bytes);
        state.begin_world_admission();
        let scene = hk_format::WorldMeta::parse(bytes).unwrap().scene_id() as usize;
        let gates: Vec<_> = world::gates(scene).collect();
        for g in &gates {
            let b = g.bounds;
            for sx in 0..5 { for sy in 0..5 {
                let x = b[0] - hk_sim::ONE + (b[2] - b[0] + 2 * hk_sim::ONE) / 4 * sx;
                let y = b[1] - hk_sim::ONE + (b[3] - b[1] + 2 * hk_sim::ONE) / 4 * sy;
                for facing in [-1, 1] { for up in [false, true] { for recoiling in [false, true] {
                    for ticks in [0, g.delay_ticks as u32, 10_000] {
                        let mut player = Player::spawn(x, y);
                        player.facing = facing;
                        let body = [x - hk_sim::ONE / 4, y - hk_sim::ONE, x + hk_sim::ONE / 4, y];
                        let want = world::gate(scene, &player, body, up, recoiling, ticks);
                        assert_eq!(state.gate(scene, &player, body, up, recoiling, ticks).map(key), want.map(key),
                                   "{} at {x},{y}", path.display());
                        probes += 1;
                        taken += u32::from(want.is_some());
                    }
                }}}
            }}
        }
        // Another scene's id while this bank is admitted: no gate.
        assert!(state.gate(scene + 1, &Player::spawn(0, 0), [0; 4], true, false, 10_000).is_none());
    }
    assert!(taken > 0, "{probes} probes");
}

fn movement_params() -> hk_sim::Params {
    hk_sim::Params { speed: 8 * hk_sim::ONE, jump: 16 * hk_sim::ONE, gravity: 48 * hk_sim::ONE, fall: 20 * hk_sim::ONE, hold_ticks: 12, min_ticks: 5, half_width: hk_sim::ONE / 4, bottom: -hk_sim::ONE, top: 0, ..hk_sim::Params::ZERO }
}
fn room_with_edges(edges: &[[i32; 4]]) -> Vec<u8> {
    let mut bytes = vec![0u8; 40 + 16 + edges.len() * 16 + 32 + 32768];
    bytes[..8].copy_from_slice(b"HKROOM02");
    bytes[8] = 1;
    bytes[12] = 1;
    bytes[28..32].copy_from_slice(&(edges.len() as u32).to_le_bytes());
    bytes[46] = 1;
    bytes[48] = 1;
    for (i, edge) in edges.iter().enumerate() {
        for (j, value) in edge.iter().enumerate() {
            let p = 56 + i * 16 + j * 4;
            bytes[p..p + 4].copy_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}
#[test]
fn vertical_prefetch_rejects_solid_floor_and_allows_real_downward_exit() {
    use hk_sim::ONE;
    let bounds = [-5 * ONE, 0, 5 * ONE, 10 * ONE];
    let mut player = Player::spawn(0, 2 * ONE);
    player.vy = -4 * ONE;
    let before = player;
    let floor = [-5 * ONE, ONE, 5 * ONE, ONE];
    assert_eq!(
        world::vertical_exit(bounds, &player, movement_params(), 1, |_| floor),
        None
    );
    assert_eq!(
        world::vertical_exit(bounds, &player, movement_params(), 0, |_| [0; 4]),
        Some(-ONE / 16)
    );
    assert_eq!(player, before, "look-ahead must not modify gameplay");
}
#[test]
fn vertical_prefetch_rejects_ceiling_and_short_jump_apex() {
    use hk_sim::ONE;
    let bounds = [-5 * ONE, 0, 5 * ONE, 10 * ONE];
    let mut player = Player::spawn(0, 9 * ONE);
    player.vy = 12 * ONE;
    let ceiling = [-5 * ONE, 9 * ONE + ONE / 2, 5 * ONE, 9 * ONE + ONE / 2];
    assert_eq!(
        world::vertical_exit(bounds, &player, movement_params(), 1, |_| ceiling),
        None
    );
    assert_eq!(
        world::vertical_exit(bounds, &player, movement_params(), 0, |_| [0; 4]),
        Some(10 * ONE + ONE / 16)
    );
    player.y = 8 * ONE;
    player.vy = 6 * ONE;
    assert_eq!(
        world::vertical_exit(bounds, &player, movement_params(), 0, |_| [0; 4]),
        None
    );
}
#[test]
fn vertical_prefetch_respects_removed_owned_floor_and_thirty_tick_bound() {
    use hk_sim::ONE;
    let bounds = [-5 * ONE, 0, 5 * ONE, 10 * ONE];
    let mut player = Player::spawn(0, 2 * ONE);
    player.vy = -4 * ONE;
    let bytes = room_with_edges(&[
        [4 * ONE, 0, 4 * ONE, 4 * ONE],
        [-5 * ONE, ONE, 5 * ONE, ONE],
    ]);
    let room = hk_format::Room::parse(&bytes).unwrap();
    let mut state = State::new();
    let r = region();
    assert_eq!(
        world::vertical_exit(bounds, &player, movement_params(), 2, |i| state
            .edge(&r, &room, i)),
        None
    );
    state.break_object(0, 60);
    assert_eq!(
        world::vertical_exit(bounds, &player, movement_params(), 2, |i| state
            .edge(&r, &room, i)),
        Some(-ONE / 16)
    );
    let mut params = movement_params();
    params.gravity = 0;
    player.vy = -ONE;
    let calls = std::cell::Cell::new(0);
    assert_eq!(
        world::vertical_exit(bounds, &player, params, 1, |_| {
            calls.set(calls.get() + 1);
            [0; 4]
        }),
        None
    );
    assert_eq!(
        calls.get(),
        60,
        "two terrain sweeps per step, at most thirty steps"
    );
}
#[test]
fn generated_region_prefetch_falls_back_horizontally_when_floor_blocks_drop() {
    use hk_sim::ONE;
    use_bank(SCENE0_BANK);
    let bank = hk_format::WorldMeta::parse(SCENE0_BANK).unwrap();
    let bounds_of = |global: u32| bank.region_by_global_id(global).unwrap().bounds();
    let candidate = bank.regions()
            .find_map(|meta| {
                let r = scene0_region(meta.global_id() as usize);
                let neighbours: Vec<u32> = meta.neighbours().flatten().filter(|&n| bank.region_by_global_id(n).is_some()).collect();
                let x = r.bounds[0] + (r.bounds[2] - r.bounds[0]) / 2;
                let y = r.bounds[1] + 2 * ONE;
                let horizontal = neighbours.iter().copied().find(|&n| {
                    world::contains(bounds_of(n), r.bounds[2] + ONE / 16, y)
                });
                let downward = neighbours.iter().copied().find(|&n| {
                    world::contains(bounds_of(n), x, r.bounds[1] - ONE / 16)
                });
                match (horizontal, downward) {
                    (Some(h), Some(d)) if h != d => Some((r.global_id - 1, x, y, h as usize - 1, d as usize - 1)),
                    _ => None,
                }
            })
            .expect("generated room table must contain horizontal/downward alternatives");
    let (index, x, y, horizontal, downward) = candidate;
    let r = world::resident(index).unwrap();
    let mut player = Player::spawn(x, y);
    player.vy = -4 * ONE;
    let bytes = room_with_edges(&[[
        r.bounds[0] - ONE,
        r.bounds[1] + ONE,
        r.bounds[2] + ONE,
        r.bounds[1] + ONE,
    ]]);
    let room = hk_format::Room::parse(&bytes).unwrap();
    let state = State::new();
    assert_eq!(
        world::upcoming(index, &player, &room, &state, movement_params()),
        Some(horizontal)
    );
    let bytes = room_with_edges(&[]);
    let empty = hk_format::Room::parse(&bytes).unwrap();
    assert_eq!(
        world::upcoming(index, &player, &empty, &state, movement_params()),
        Some(downward)
    );
}

#[test]
fn door1_mask_survives_region_entry_outside_its_collider() {
    let _lock = render::TEST_LOCK.lock().unwrap();
    // Original level6:12793 state38 owns FSM12055 -> renderer11301.
    // Region14 includes the mask but not Door1's x59.59 collision/attack shape.
    use_bank(SCENE0_BANK);
    let (owner, _, fade) = scene0_breakables().into_iter().find(|b| b.0 == 38 && b.2 == 60).unwrap();
    let distant = scene0_regions().into_iter().map(|(_, r)| r).find(|r|
        world::contains(r.bounds, 84 * hk_sim::ONE + hk_sim::ONE/32, 11 * hk_sim::ONE + hk_sim::ONE/2))
        .unwrap();
    let distant = &distant;
    let bank = world::bank_region(distant).unwrap();
    assert!(!world::breakables(bank).any(|b| b.id() == owner));
    let bindings: Vec<(usize, Vec<u16>)> = world::remote_masks(bank).filter(|m| m.0 == owner)
        .map(|(o, _, fade)| (o, fade.draws().collect())).collect();
    assert!(!bindings.is_empty());
    let mut state = State::new();
    state.break_object(owner, fade);
    for _ in 0..30 { state.tick(); }
    for binding in &bindings {
        // Isolate a single application to avoid sharing the rendering stub
        // with unrelated fade tests run by the native parallel test harness.
        assert_eq!(state.fade_left[binding.0],30);
    }
    state.apply(distant);
    let gains = render::GAINS.lock().unwrap().clone();
    for binding in &bindings { for draw in &binding.1 {
        assert!(gains.contains(&(*draw as usize,64)));
    }}
    for _ in 0..30 { state.tick(); }
    state.apply(distant);
    let gains = render::GAINS.lock().unwrap().clone();
    for binding in &bindings { for draw in &binding.1 {
        assert!(gains.contains(&(*draw as usize,0)));
    }}
    assert!(state.broken(owner));
}

#[test]
fn actual_crawler_spawn_resolves_against_complete_cooked_terrain() {
    use hk_sim::{resolve_actor_spawn, Params, ONE};
    use_bank(SCENE0_BANK);
    for source_id in [12546,12548] {
        let (index,region,placement,spec)=scene0_regions().into_iter().find_map(|(index,r)| {
            world::region_actors(&r).find(|(p,_)| p.source_id==source_id &&
                world::contains(r.bounds,p.x,p.y)).map(|(p,s)|(index,r,p,s))
        }).expect("authored Tutorial crawler activation region");
        let region=&region;
        let path=format!("{}/../data/regions/chunk_{}.hk",env!("CARGO_MANIFEST_DIR"),index+1);
        let bytes=std::fs::read(path).unwrap();let room=hk_format::Room::parse(&bytes).unwrap();
        let state=State::new();let b=spec.bounds;
        let p=Params { speed:spec.walk.speed, gravity:60*ONE, fall:100*ONE, half_width:(b[2]-b[0])/2, bottom:b[1], top:b[3], ..Params::ZERO };
        let mut body=Player::spawn(placement.x+b[0]+p.half_width,placement.y);
        assert!(resolve_actor_spawn(&mut body,p,room.counts[5],|i|state.edge(region,&room,i)));
        assert_eq!(body.y,if source_id==12548 {331776} else {placement.y});
        for _ in 0..60 {body.step(p,0,false,room.counts[5],|i|state.edge(region,&room,i));}
        assert!(body.grounded);
        if source_id==12548 {assert_eq!(body.y,331776);}
    }
}

#[test]
fn authored_inverse_remask_bindings_preserve_one_controller_across_regions() {
    use hk_sim::{Params, ONE};
    // The catalogue belongs to the scene, so it is read once from the admitted
    // bank rather than borrowed from whichever region happens to be resident.
    use_bank(SCENE0_BANK);
    let masks:Vec<_>=world::reveal_masks(0).collect();
    let index=masks.iter().position(|m|m.source_id==12038).expect("source inverse_remask_right FSM");
    assert_eq!(masks[index].fade_ticks,30);
    assert_eq!(masks[index].initial_opacity,0);
    // The cooked AABB of the authored trigger, which is what the tick tests
    // before it asks the bank for the polygon itself.
    assert_eq!(masks[index].bounds,[10462305,822671,13202367,2153911]);
    let p=Params { half_width:ONE/4, bottom:-ONE, ..Params::ZERO };
    let inside=Player::spawn(180*ONE,20*ONE);
    let outside=Player::spawn(154*ONE,20*ONE);
    let reaches=|c:usize,body:[i32;4]|world::reveal_trigger_reaches(0,c,body);
    let mut controller=reveal_masks::State::new();controller.scene_ready(0,world::reveal_masks(0));
    for _ in 0..30 {controller.tick(&outside,p,reaches);}
    assert_eq!(controller.opacity(index),0);
    for _ in 0..15 {controller.tick(&inside,p,reaches);}
    assert_eq!(controller.opacity(index),64);
    // Idle opacity still answers per controller while no scene is bound.
    assert_eq!(world::reveal_initial_opacity(0,index),0);
    let mut binding_count=0;
    for (id,region) in scene0_regions() {
        let region=&region;
        // Re-readying mid-tween must not restart it, however many regions of
        // the scene the hero walks through while a mask is fading.
        controller.scene_ready(0,world::reveal_masks(0));assert_eq!(controller.opacity(index),64);
        let path=format!("{}/../data/regions/chunk_{}.hk",env!("CARGO_MANIFEST_DIR"),id+1);
        let bytes=std::fs::read(path).unwrap();let room=hk_format::Room::parse(&bytes).unwrap();
        for binding in world::reveal_bindings(region) {
            assert!((binding.controller as usize)<masks.len());
            assert!((binding.draw as usize)<room.counts[2]);
            if binding.controller as usize==index {binding_count+=1;}
        }
    }
    assert!(binding_count>1,"revealed source sprite spans several residency regions");
    for _ in 0..30 {controller.tick(&outside,p,reaches);}
    assert_eq!(controller.opacity(index),0);
}


#[test]
fn active_fades_match_full_timer_reference_including_simultaneous_expiry() {
    let mut state=State::new();let mut reference=[0u16;world::BREAKABLES_PER_SCENE];
    for (id,ticks) in [(0,0),(1,1),(31,17),(32,17),(63,2),(96,17),(127,65535)] {
        assert!(state.break_object(id,ticks));reference[id]=ticks;
    }
    for tick in 0..65536 {
        state.tick();for time in &mut reference{*time=time.saturating_sub(1);}
        assert_eq!(state.fade_left,reference,"timer tick{tick}");
        if tick==7 {assert!(!state.break_object(31,100),"repeat hits never restart");}
    }
}
#[test]
fn scene_reset_clears_only_its_transient_active_fades_and_allows_reactivation() {
    use_bank(SCENE0_BANK);
    let all=scene0_breakables();
    let (b,_,_)=all.iter().copied().find(|b|!b.1).expect("authored transient breakable");
    // A persistent object's fade is the one a scene reset must leave running.
    let (other,_,_)=all.iter().copied().find(|b|b.1).expect("authored persistent breakable");
    let mut state=State::new();assert!(state.break_object(b,19));
    assert!(state.break_object(other,19));
    state.tick();assert_eq!(state.fade_left[b],18);assert_eq!(state.fade_left[other],18);
    state.reset_scene(0);assert_eq!(state.fade_left[b],0);
    state.tick();assert_eq!(state.fade_left[b],0);assert_eq!(state.fade_left[other],17);
    assert!(state.break_object(b,2));
    state.tick();assert_eq!(state.fade_left[b],1);state.tick();assert_eq!(state.fade_left[b],0);
    let clean=State::new();assert!(clean.fade_left.iter().all(|&n|n==0));
}


#[test]
fn scoped_edge_reader_matches_direct_queries_across_mutations_sources_and_fallback() {
    let a=edge_room_bytes(&[[1,2,3,2],[4,5,4,6]]);
    let b=edge_room_bytes(&[[11,12,13,12],[14,15,14,16]]);
    let a=hk_format::Room::parse(&a).unwrap();let b=hk_format::Room::parse(&b).unwrap();
    let r=region();let mut state=State::new();
    let check=|state:&State,room:&hk_format::Room| {
        let read=state.edge_reader(&r,room);
        for i in 0..room.counts[5]{assert_eq!(read(i),state.edge(&r,room,i));}
    };
    check(&state,&a);state.refresh_edges(&r,&a);check(&state,&a);check(&state,&b);
    state.break_object(0,60);check(&state,&a);state.refresh_edges(&r,&a);check(&state,&a);
    state.set_geo_edges(&[0]);check(&state,&a);state.refresh_edges(&r,&a);check(&state,&a);
    let big=edge_room_bytes(&vec![[101,102,103,102];129]);
    let big=hk_format::Room::parse(&big).unwrap();state.refresh_edges(&r,&big);check(&state,&big);
}

#[test]
fn foreign_view_exclusions_follow_segments_not_local_indices() {
    use hk_sim::ONE;
    let a=[ONE,0,2*ONE,0];let b=[3*ONE,0,4*ONE,0];
    let c=[5*ONE,0,6*ONE,0];
    let source_bytes=edge_room_bytes(&[a,b]);
    let target_bytes=edge_room_bytes(&[c,[a[2],a[3],a[0],a[1]],b]);
    let source=hk_format::Room::parse(&source_bytes).unwrap();
    let target=hk_format::Room::parse(&target_bytes).unwrap();
    let current=region();let mut other=region();other.global_id=2;
    let mut state=State::new();state.set_geo_edges(&[0]);
    state.set_lifeblood_edges(&[1,999]);
    assert_eq!(state.edge_in_view(&current,&source,&other,&target,0),c);
    assert_eq!(state.edge_in_view(&current,&source,&other,&target,1),[0;4]);
    assert_eq!(state.edge_in_view(&current,&source,&other,&target,2),[0;4]);
    other.scene=1-current.scene;
    assert_eq!(state.edge_in_view(&current,&source,&other,&target,1),[a[2],a[3],a[0],a[1]]);
}

#[test]
fn actor_terrain_scratch_matches_direct_filter_for_both_views_and_mutations() {
    let source_bytes=edge_room_bytes(&[[1,2,3,2],[4,5,4,6]]);
    let target_bytes=edge_room_bytes(&[[4,6,4,5],[7,8,9,8],[1,2,3,2]]);
    let source=hk_format::Room::parse(&source_bytes).unwrap();
    let target=hk_format::Room::parse(&target_bytes).unwrap();
    let active=region();let other=region();
    let check=|state:&State,r:&Region,room:&hk_format::Room| {
        let mut scratch=vec![[0;4];room.counts[5]];
        state.fill_edges_in_view(&active,&source,r,room,&mut scratch);
        assert_eq!(scratch,(0..room.counts[5]).map(|i|state.edge_in_view(&active,&source,r,room,i)).collect::<Vec<_>>());
    };
    let mut state=State::new();
    for step in 0..5 {
        match step {
            1=>{state.set_geo_edges(&[0,999]);state.set_lifeblood_edges(&[1]);},
            2=>{state.break_object(0,60);},
            3=>{state.set_geo_edges(&[]);state.set_lifeblood_edges(&[]);},
            4=>{state.reset_scene(active.scene);},
            _=>{}
        }
        check(&state,&active,&source);check(&state,&other,&target);
        state.refresh_edges(&active,&source);
        check(&state,&active,&source);check(&state,&other,&target);
    }
}
