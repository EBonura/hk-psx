//! Every enemy of every scene with the Knight walking past it, swinging, and jumping about, for a
//! long run. Nothing is compared: what is looked for is what the idle scan cannot see because the
//! enemy never meets the Knight, a panic in the guest module (which on the console is a halt), an
//! enemy that leaves the world or jumps across it between two ticks, or one that is still alive
//! after a hundred strikes.
use super::*;
use crate::compare::family_of;
use crate::replay::step_with;
use crate::world_data::load_scene;
use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};

pub fn run(names: &BTreeMap<usize, String>, ticks: usize) {
    let mut rows: BTreeMap<String, [u32; 5]> = BTreeMap::new();
    let mut findings = Vec::new();
    std::panic::set_hook(Box::new(|_| {}));
    for (id, name) in names {
        let regions = load_scene(*id);
        let mut seen: Vec<u32> = Vec::new();
        for first in &regions {
            for (p0, spec0) in first.actors.iter() {
                if !world::contains(first.bounds, p0.x, p0.y) || seen.contains(&p0.source_id) {
                    continue;
                }
                seen.push(p0.source_id);
                let fam = family_of(spec0);
                let row = rows.entry(fam.clone()).or_default();
                row[0] += 1;
                let outcome = catch_unwind(AssertUnwindSafe(|| {
                    let mut w = enemies::EnemyWorld::new();
                    let mut player = Player::spawn(p0.x - 8 * ONE, p0.y);
                    let mut vitals = Vitals::new(VITAL_PARAMS);
                    let mut here = first;
                    let (mut last, mut worst_jump, mut gone_at, mut dead_at) =
                        ((f64::NAN, f64::NAN), 0.0f64, None, None);
                    let mut strikes = 0u32;
                    let mut nail_cooldown = 0;
                    for t in 0..ticks {
                        // The Knight paces 16 units either side of the enemy, 3 units a tick-second.
                        let phase = (t as f64 / 60.0 * 5.0).rem_euclid(64.0);
                        let off = if phase < 32.0 {
                            phase - 16.0
                        } else {
                            48.0 - phase
                        };
                        player.x = p0.x + (off * 65536.0) as i32;
                        player.y = p0.y + if (t / 90) % 3 == 0 { 2 * ONE } else { 0 };
                        player.facing = if off < 0.0 { 1 } else { -1 };
                        let mut nail = Nail::new();
                        let mut strike = None;
                        nail_cooldown = (nail_cooldown as i32 - 1).max(0);
                        if nail_cooldown == 0 && off.abs() < 3.0 && t % 7 == 0 {
                            if let Some(d) = w.debug_actor(*id, p0.source_id) {
                                nail.active = true;
                                nail.age = 0;
                                strike = Some([
                                    d.x - 2 * ONE,
                                    d.y - 2 * ONE,
                                    d.x + 2 * ONE,
                                    d.y + 2 * ONE,
                                ]);
                                strikes += 1;
                                nail_cooldown = 14;
                            }
                        }
                        let cam = [
                            (here.bounds[0] + here.bounds[2]) / 2,
                            (here.bounds[1] + here.bounds[3]) / 2,
                            -38 * ONE,
                        ];
                        step_with(
                            &mut w,
                            here,
                            &regions,
                            &mut player,
                            &mut vitals,
                            cam,
                            &nail,
                            strike,
                        );
                        // The Knight never dies in this run: he is the invincible one.
                        vitals = Vitals::new(VITAL_PARAMS);
                        if let Some(d) = w.debug_actor(*id, p0.source_id) {
                            let (x, y) = (d.x as f64 / 65536.0, d.y as f64 / 65536.0);
                            if last.0.is_finite() {
                                worst_jump = worst_jump.max((x - last.0).hypot(y - last.1));
                            }
                            last = (x, y);
                            if d.dead && dead_at.is_none() {
                                dead_at = Some(t);
                            }
                            if !world::contains(here.bounds, d.x, d.y) {
                                if let Some(next) =
                                    regions.iter().find(|r| world::contains(r.bounds, d.x, d.y))
                                {
                                    here = next;
                                }
                            }
                            let low = regions
                                .iter()
                                .map(|r| r.collision_bounds[1])
                                .min()
                                .unwrap_or(0);
                            if d.y < low - 3 * ONE && gone_at.is_none() && !d.dead {
                                gone_at = Some(t);
                            }
                        }
                    }
                    (worst_jump, gone_at, dead_at, strikes)
                }));
                match outcome {
                    Err(_) => {
                        row[1] += 1;
                        findings.push(format!("{name} {:>6} {fam}: PANIC", p0.source_id));
                    }
                    Ok((jump, gone, dead, strikes)) => {
                        let teleport = jump > 4.0 && fam != "Static";
                        row[2] += teleport as u32;
                        row[3] += gone.is_some() as u32;
                        row[4] += dead.is_some() as u32;
                        if teleport || gone.is_some() {
                            findings.push(format!("{name} {:>6} {fam}: biggest one-tick move {jump:.2}, left the world at {gone:?}, died at {dead:?}, {strikes} strikes", p0.source_id));
                        }
                    }
                }
            }
        }
    }
    println!("family          actors  panics  jumps  fell  died");
    for (f, r) in &rows {
        println!(
            "{f:<14} {:>7} {:>7} {:>6} {:>5} {:>5}",
            r[0], r[1], r[2], r[3], r[4]
        );
    }
    for f in &findings {
        println!("{f}");
    }
}
