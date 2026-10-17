//! The Hornet adapter end to end on the host: the original's FSMs on a flat floor, the Knight walking up to the Encounter,
//! the fight, a kill by nail strikes, the corpse that writes `hornet1Defeated` and sends `HORNET LEAVE`.
#![cfg(feature = "boss")]
use hk_sim::hornet::fight;
use hk_sim::hornet_fight::{field, Effect, Fight, Input, Phase, Strike};
use hk_sim::ONE;

/// The floor under the boss's authored place (its collider's bottom edge), 100 units either side.
fn floor(_: usize) -> [i32; 4] {
    [0, 27 * ONE + 3000, 100 * ONE, 27 * ONE + 3000]
}

struct Run {
    f: Box<Fight>,
    effects: Vec<Effect>,
    hero: [i32; 2],
    convo: bool,
    tick: u32,
}

impl Run {
    fn new() -> Self {
        let mut f = Box::new(Fight::blank());
        f.begin(7, false, [0; 8]);
        Run { f, effects: vec![], hero: [5 * ONE, 27 * ONE + 3000], convo: false, tick: 0 }
    }
    fn tick(&mut self, strike: Option<Strike<'_>>) {
        let h = self.hero;
        let body = [h[0] - ONE / 4, h[1], h[0] + ONE / 4, h[1] + ONE + ONE / 2];
        let input = Input {
            hero: h,
            hero_body: body,
            camera: [h[0], h[1], -38 * ONE],
            dt_scaled: ONE,
            dt_real: ONE,
            strike,
            conversation_done: std::mem::take(&mut self.convo),
        };
        let mut out = vec![];
        self.f.step(&input, 1, &floor, &mut |e| out.push(e));
        for e in &out {
            if matches!(e, Effect::Conversation { .. }) {
                // The dialogue box finishes a few seconds after it starts; the fight waits on it.
                self.convo = true;
            }
        }
        self.effects.extend(out);
        self.tick += 1;
    }
    fn run(&mut self, ticks: u32) {
        for _ in 0..ticks {
            self.tick(None);
        }
    }
}

#[test]
fn the_fight_starts_when_the_knight_walks_into_the_encounter() {
    let mut r = Run::new();
    assert_eq!(r.f.phase, Phase::Asleep);
    // Far from the Encounter's box nothing happens for a good while.
    r.run(300);
    assert!(!r.effects.iter().any(|e| matches!(e, Effect::Conversation { .. })), "{:?}", r.effects);
    // Walk in: the Encounter FSM starts the title, the conversation and the music.
    r.hero = [27 * ONE, 27 * ONE + 3000];
    r.run(600);
    assert!(r.effects.iter().any(|e| matches!(e, Effect::Conversation { .. })), "the Encounter never began its conversation: {:?}", &r.effects[..r.effects.len().min(12)]);
}

#[test]
fn a_boss_that_is_asked_to_hold_still_stays_in_its_arena() {
    let mut r = Run::new();
    r.hero = [27 * ONE, 27 * ONE + 3000];
    r.run(1500);
    let x = r.f.boss.world.ent[0].pos[0];
    let [lo, hi] = fight::ARENA_X;
    assert!(x >= lo && x <= hi, "x {x}");
}

#[test]
fn killing_hornet_runs_the_corpse_to_the_defeat_and_the_leave() {
    let mut r = Run::new();
    r.hero = [27 * ONE, 27 * ONE + 3000];
    r.run(900);
    // Strike her wherever she is, every 20 ticks, with the nail's damage, until she dies.
    let mut guard = 0;
    while r.f.phase != Phase::Dying && r.f.phase != Phase::Gone {
        let at = r.f.boss.world.world_of(0).0;
        let poly = [[at[0] - ONE, at[1] - 2 * ONE], [at[0] + ONE, at[1] - 2 * ONE], [at[0] + ONE, at[1] + ONE], [at[0] - ONE, at[1] + ONE]];
        r.tick(Some(Strike { polygon: &poly, kind: 0, damage: 5, facing: 1 }));
        r.run(19);
        guard += 1;
        assert!(guard < 400, "hp {}", r.f.health.hp);
    }
    assert!(r.effects.iter().any(|e| matches!(e, Effect::Died)));
    let mut seen = vec![];
    for _ in 0..60 * 40 {
        r.tick(None);
        let st = r.f.corpse_state();
        if seen.last() != Some(&st) {
            seen.push(st);
        }
    }
    eprintln!("corpse states {:?} effects after death {:?}", seen, &r.effects[r.effects.len().saturating_sub(12)..]);
    let wrote = r.effects.iter().any(|e| matches!(e, Effect::Persist { field: field::HORNET1_DEFEATED, value } if *value != 0));
    assert!(wrote, "the corpse never wrote hornet1Defeated");
    assert!(r.effects.iter().any(|e| matches!(e, Effect::CloakLeaves)), "the corpse never sent HORNET LEAVE");
    assert_eq!(r.f.phase, Phase::Gone);
}

#[test]
fn a_defeated_hornet_is_not_loaded() {
    let mut f = Box::new(Fight::blank());
    let mut pd = [0; 8];
    pd[field::HORNET1_DEFEATED as usize] = 1;
    f.begin(1, true, pd);
    assert_eq!(f.phase, Phase::Gone);
}

#[test]
fn the_corpse_tree_alone_runs_to_its_end() {
    use hk_sim::hornet::Boss;
    let mut c = Box::new(Boss::blank_corpse());
    c.init(3);
    c.world.ent[0].pos = [30 * ONE, 28 * ONE, 0];
    c.enter();
    let mut seen = vec![c.tree().state(0)];
    for _ in 0..600 {
        c.env.dt_scaled = ONE;
        c.env.dt_real = ONE;
        c.step(1);
        let s = c.tree().state(0);
        if seen.last() != Some(&s) {
            seen.push(s);
        }
    }
    let off = c.tree().block().inactive(0);
    eprintln!("alone: {:?} pd {:?} inactive {}", seen, c.pd, off);
}

#[test]
fn a_standing_knight_is_fought_with_the_whole_move_set() {
    let mut r = Run::new();
    r.hero = [27 * ONE, 27 * ONE + 3000];
    let mut states = std::collections::BTreeSet::new();
    let mut xs = (i32::MAX, i32::MIN);
    for _ in 0..60 * 90 {
        r.tick(None);
        states.insert(r.f.boss.tree().state(0));
        let x = r.f.boss.world.ent[0].pos[0];
        xs = (xs.0.min(x), xs.1.max(x));
    }
    let hurt = r.effects.iter().filter(|e| matches!(e, Effect::HeroHurt { .. })).count();
    eprintln!("Control states visited {}, x range {:.1}..{:.1}, hero hurt {} ticks", states.len(), xs.0 as f64 / 65536.0, xs.1 as f64 / 65536.0, hurt);
    assert!(states.len() >= 10, "{states:?}");
    assert!(xs.1 - xs.0 > 3 * ONE, "she never moved");
    assert!(hurt > 0, "a Knight who stands still is never touched");
}
