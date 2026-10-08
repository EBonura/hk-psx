use hk_sim::ONE;
const KNIGHT_SCALE: i32 = 60693;
mod render {
    pub const SOURCE_ALPHA_COVERAGE: u8 = 1;
    std::thread_local! {pub static DRAWS:std::cell::RefCell<Vec<(usize,u8)>>=const{std::cell::RefCell::new(Vec::new())};}
    pub fn texture(_: usize, _: [(i16, i16); 4], _: (u8, u8, u8)) {}
    pub fn texture_material_alpha(id: usize, _: [(i16, i16); 4], _: (u8, u8, u8), material: u8) {
        DRAWS.with(|d| d.borrow_mut().push((id, material)));
    }
}
#[path = "../../../game/src/debris.rs"]
pub mod debris;
mod world {
    pub use crate::debris;
}
#[path = "../../../game/src/particles.rs"]
mod particles;
use particles::{Bank, EmitterSpec, Pool, Sample, Style, CAPACITY};
static CURVES: [Sample; 65] = [Sample {
    size: ONE,
    alpha: [255, 255],
}; 65];
static STYLES: [Style; 2] = [
    Style {
        life: [42, 78],
        speed: [3 * ONE, 30 * ONE],
        size: [45875, 58982],
        force: [-8 * ONE, -4 * ONE],
        dampen: 19661,
        rotation: 200 * ONE,
        count: 25,
        duration: 6,
        uv_scale: 65529,
        colors: [[128; 3]; 2],
        curves: &CURVES,
    },
    Style {
        life: [36, 36],
        speed: [8 * ONE; 2],
        size: [ONE / 2, 85197],
        force: [1285816; 2],
        dampen: 1966,
        rotation: 0,
        count: 50,
        duration: 6,
        uv_scale: 65529,
        colors: [[114, 54, 0], [128, 115, 91]],
        curves: &CURVES,
    },
];
static GRASS: [[u16; 4]; 3] = [[0, 1, 2, 3], [4, 5, 6, 7], [8, 9, 10, 11]];
static DEATH: [[u16; 4]; 9] = [
    [12, 13, 14, 15],
    [16, 17, 18, 19],
    [20, 21, 22, 23],
    [24, 25, 26, 27],
    [28, 29, 30, 31],
    [32, 33, 34, 35],
    [36, 37, 38, 39],
    [40, 41, 42, 43],
    [44, 45, 46, 47],
];
const BANK: Bank = Bank {
    frames: [&GRASS, &DEATH],
    styles: &STYLES,
    death_offset: [0, -13107, 0],
};
const EMITTER: EmitterSpec = EmitterSpec {
    state: 0,
    source: 12289,
    origin: [0; 3],
    basis: [[0, ONE, 0], [0, 0, ONE], [ONE / 100, 0, 0]],
    direction: [0, ONE, 0],
};
fn room() -> Vec<u8> {
    let mut b = Vec::from(*b"HKROOM02");
    for n in [1u32, 48, 0, 48, 0, 0, 0, 0] {
        b.extend(n.to_le_bytes());
    }
    for i in 0..48u16 {
        for n in [0u16, 0, 0, 4, 4, i] {
            b.extend(n.to_le_bytes());
        }
        b.extend(0u32.to_le_bytes());
    }
    for i in 0..48i32 {
        for n in [i, -ONE / 2, -ONE / 2, ONE / 2, ONE / 2] {
            b.extend(n.to_le_bytes());
        }
    }
    b.resize(b.len() + 48 * 32 + 32768, 0);
    b
}
#[test]
fn exact_source_counts_are_spread_over_six_ticks_not_one_generic_burst() {
    let raw = room();
    let r = hk_format::Room::parse(&raw).unwrap();
    let mut p = Pool::new();
    p.spawn_grass(0, EMITTER, BANK);
    assert_eq!((p.active(), p.spawned, p.dropped), (25, 25, 0));
    for frame in 1..=6 {
        assert_eq!(p.draw(0, Some(BANK), &r, (0, 0)), frame * 25 / 6);
        p.tick(0, Some(BANK));
    }
    let mut death = Pool::new();
    death.spawn_death(0, 12546, [0; 3], BANK);
    assert_eq!(death.active(), 50);
    for frame in 1..=6 {
        assert_eq!(death.draw(0, Some(BANK), &r, (0, 0)), frame * 50 / 6);
        death.tick(0, Some(BANK));
    }
    // A per-slot bound rather than a total, because the total was a stack
    // budget and the pool is no longer on the stack: it is a static, so its
    // bytes come out of the linked gap instead of main's frame. What is still
    // worth guarding is the slot, since every capacity change multiplies it and
    // Particle is exactly packed, so one stray byte costs a whole word.
    assert!(core::mem::size_of::<Pool>() <= CAPACITY * 56 + 64);
}
#[test]
fn source_lifetimes_expire_and_grid_residency_does_not_restart() {
    let mut p = Pool::new();
    p.spawn_grass(0, EMITTER, BANK);
    for _ in 0..41 {
        p.tick(0, Some(BANK));
    }
    assert_eq!(p.active(), 25);
    p.tick(1, Some(BANK));
    assert_eq!(p.active(), 25);
    for _ in 0..43 {
        p.tick(0, Some(BANK));
    }
    assert_eq!(p.active(), 0);
    p.spawn_death(0, 12546, [0; 3], BANK);
    for _ in 0..35 {
        p.tick(0, Some(BANK));
    }
    assert_eq!(p.active(), 50);
    for _ in 0..6 {
        p.tick(0, Some(BANK));
    }
    assert_eq!(p.active(), 0);
}
#[test]
fn overflow_is_exact_counted_and_never_overwrites_existing_particles() {
    // Written against CAPACITY rather than against a literal, because the
    // capacity is a budget that moves: it went 128 to 224 so one authored
    // emitter of 210 could be admitted. What this pins is that a spawn past the
    // pool drops exactly the excess and leaves every live particle alone, which
    // is true at any capacity. Pinning the number instead made this test fail
    // for saying 128 when the answer was 150.
    let death = 50usize; // spawn_death's authored count
    let grass = 25usize; // spawn_grass's
    let whole = CAPACITY / death;
    let mut p = Pool::new();
    for burst in 0..whole {
        p.spawn_death(0, 12546 + burst as u32, [0; 3], BANK);
    }
    assert_eq!(p.active(), whole * death);
    // One burst more than fits. The pool takes the room it has and counts the
    // rest as dropped rather than overwriting anything already flying.
    p.spawn_death(0, 12546 + whole as u32, [0; 3], BANK);
    let spilled = whole * death + death - CAPACITY;
    assert_eq!((p.active(), p.spawned, p.dropped), (CAPACITY, CAPACITY as u32, spilled as u32));
    p.spawn_grass(0, EMITTER, BANK);
    assert_eq!(p.dropped, (spilled + grass) as u32);
    p.clear_scene(1);
    assert_eq!(p.active(), CAPACITY);
    p.clear_scene(0);
    assert_eq!(p.active(), 0);
    p.spawn_grass(0, EMITTER, BANK);
    assert_eq!(p.active(), 25);
}
#[test]
fn original_cells_animation_and_average_material_are_used() {
    let raw = room();
    let r = hk_format::Room::parse(&raw).unwrap();
    let mut p = Pool::new();
    p.spawn_death(0, 12546, [0; 3], BANK);
    p.draw(0, Some(BANK), &r, (0, 0));
    let first = render::DRAWS.with(|v| v.borrow().clone());
    assert!(first.iter().all(|&(id, m)| id == 15 && m == 1));
    for _ in 0..25 {
        p.tick(0, Some(BANK));
    }
    render::DRAWS.with(|v| v.borrow_mut().clear());
    p.draw(0, Some(BANK), &r, (0, 0));
    assert!(render::DRAWS.with(|v| v.borrow().iter().all(|&(id, m)| id >= 31 && m == 1)));
    assert_eq!(p.draw(1, Some(BANK), &r, (0, 0)), 0);
    assert_eq!(p.active(), 50);
}
#[test]
fn identical_source_seed_yields_identical_draws_and_offscreen_culling_preserves_life() {
    let raw = room();
    let r = hk_format::Room::parse(&raw).unwrap();
    let mut a = Pool::new();
    let mut b = Pool::new();
    a.spawn_grass(0, EMITTER, BANK);
    b.spawn_grass(0, EMITTER, BANK);
    for _ in 0..20 {
        a.tick(0, Some(BANK));
        b.tick(0, Some(BANK));
    }
    render::DRAWS.with(|v| v.borrow_mut().clear());
    a.draw(0, Some(BANK), &r, (0, 0));
    let expected = render::DRAWS.with(|v| v.borrow().clone());
    render::DRAWS.with(|v| v.borrow_mut().clear());
    b.draw(0, Some(BANK), &r, (0, 0));
    assert_eq!(expected, render::DRAWS.with(|v| v.borrow().clone()));
    assert_eq!(a.draw(0, Some(BANK), &r, (100 * ONE, 100 * ONE)), 0);
    assert_eq!(a.active(), 25);
}
