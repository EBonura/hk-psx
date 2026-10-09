//! Every enemy of every scene, alone, for a long run: the faults that show without an original to
//! compare with. An enemy that walks off the bottom of its room, never moves though its family
//! patrols, reverses many times a second, or ends up where another enemy's corpse of terrain
//! says it cannot be, is a bug whichever side is right about the rest.
use super::*;
use crate::compare::{family_of, run_port_scene_with, PortActor};
use crate::og_trace::SceneTrace;
use crate::world_data::load_scene;
use std::collections::BTreeMap;

fn empty_trace() -> SceneTrace {
    SceneTrace {
        scene: String::new(),
        first_frame: 0,
        origin: 0,
        last_frame: 0,
        windows: Vec::new(),
        face: BTreeMap::new(),
        camera: BTreeMap::new(),
        hero: BTreeMap::new(),
        actors: Vec::new(),
    }
}

/// Families whose members patrol or fly on their own and so must move in 40 seconds.
fn must_move(family: &str) -> bool {
    matches!(family, "Crawler" | "Runner" | "Climber" | "MossWalker" | "Vengefly" | "Mosquito" | "AcidFlyer" | "Aspid" | "Gruzzer" | "ZombieShield" | "Pigeon")
}

pub fn run(names: &BTreeMap<usize, String>, ticks: usize) {
    let mut rows: BTreeMap<String, [u32; 6]> = BTreeMap::new();
    let mut findings = Vec::new();
    for (id, name) in names {
        let regions = load_scene(*id);
        let low = regions.iter().map(|r| r.collision_bounds[1]).min().unwrap_or(0) as f64 / 65536.0;
        let actors: Vec<PortActor> = run_port_scene_with(*id, &empty_trace(), ticks, Some(&|r| [(r.bounds[0] + r.bounds[2]) / 2, (r.bounds[1] + r.bounds[3]) / 2, -38 * ONE]));
        for a in &actors {
            let row = rows.entry(a.family.clone()).or_default();
            row[0] += 1;
            let live: Vec<_> = a.ticks.iter().filter(|t| t.x.is_finite()).collect();
            let (mut lo_x, mut hi_x, mut lo_y, mut hi_y) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
            for t in &live {
                lo_x = lo_x.min(t.x);
                hi_x = hi_x.max(t.x);
                lo_y = lo_y.min(t.y);
                hi_y = hi_y.max(t.y);
            }
            let fell = lo_y < low - 2.0;
            let frozen = must_move(&a.family) && hi_x - lo_x < 0.05 && hi_y - lo_y < 0.05;
            // A reversal of the horizontal direction, counted over the second half (steady state).
            let mut turns = 0;
            let mut last = 0.0f64;
            let half = live.len() / 2;
            for w in live[half..].windows(2) {
                let d = w[1].x - w[0].x;
                if d.abs() > 1e-3 {
                    if last != 0.0 && d.signum() != last.signum() {
                        turns += 1;
                    }
                    last = d;
                }
            }
            let jitter = turns as f64 / (live.len() - half).max(1) as f64 * 60.0 > 1.5;
            let teleport = live.windows(2).any(|w| (w[1].x - w[0].x).hypot(w[1].y - w[0].y) > 3.0 && !matches!(a.family.as_str(), "Static"));
            row[1] += fell as u32;
            row[2] += frozen as u32;
            row[3] += jitter as u32;
            row[4] += teleport as u32;
            row[5] += (live.is_empty()) as u32;
            if fell || frozen || jitter || teleport {
                findings.push(format!(
                    "{name} {:>6} {:<10} start ({:.2},{:.2}) x[{:.2},{:.2}] y[{:.2},{:.2}] fell:{} frozen:{} jitter:{} ({turns} turns) jump:{}",
                    a.source_id, a.family, a.start.0, a.start.1, lo_x, hi_x, lo_y, hi_y, fell as u8, frozen as u8, jitter as u8, teleport as u8
                ));
            }
        }
    }
    println!("family          actors  fell  frozen  jitter  jump  absent");
    for (f, r) in &rows {
        println!("{f:<14} {:>7} {:>5} {:>7} {:>7} {:>5} {:>7}", r[0], r[1], r[2], r[3], r[4], r[5]);
    }
    for f in &findings {
        println!("{f}");
    }
}
