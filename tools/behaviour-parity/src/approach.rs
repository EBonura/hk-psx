//! The Knight walking up to an enemy, in the original and in the port.
//!
//! The survey's approach mode stands the Knight on the terrain 14 units to one side of each
//! distinct enemy and walks him toward it at 4 units per second. The port's enemy is stood where
//! the original's was when the walk began and the Knight is replayed from the original's trace.
//! What is compared is the detection: whether the enemy reacts (a burst of speed well above what it
//! was doing) and how far from the Knight it was when it did.
use super::*;
use crate::compare::{match_actors, run_port_scene, PortActor};
use crate::og_trace::{load_run, OgActor};
use crate::tour::{base_name, run_visits};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug)]
struct Onset {
    frame: usize,
    distance: f64,
}

/// The first 3-frame stretch faster than `threshold` units per second, and the Knight's distance then.
fn onset(points: &[(f64, f64)], hero: &[(f64, f64)], threshold: f64) -> Option<Onset> {
    for i in 3..points.len().min(hero.len()) {
        let v = (points[i].0 - points[i - 3].0).hypot(points[i].1 - points[i - 3].1) * 20.0;
        if v > threshold {
            return Some(Onset { frame: i, distance: (points[i].0 - hero[i].0).hypot(points[i].1 - hero[i].1) });
        }
    }
    None
}

fn mean_speed(points: &[(f64, f64)]) -> f64 {
    let mut len = 0.0;
    for w in points.windows(2) {
        len += (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1);
    }
    len * 60.0 / points.len().max(1) as f64
}

pub fn report(run: &std::path::Path, names: &BTreeMap<usize, String>, only: Option<&str>) {
    let traces = load_run(run);
    let mut by_family: BTreeMap<String, [u32; 4]> = BTreeMap::new();
    for (id, name) in names {
        if only.map_or(false, |o| o != name) {
            continue;
        }
        let Some(trace) = traces.get(name) else { continue };
        let ticks = (trace.last_frame - trace.origin).max(0) as usize;
        let port = run_port_scene(*id, trace, ticks);
        let (pairs, _, _) = match_actors(name, trace, &port);
        let mut visits: Vec<(usize, u32, (f64, f64))> = Vec::new();
        let mut chosen: Vec<(usize, &OgActor, &PortActor)> = Vec::new();
        for (wi, w) in trace.windows.iter().enumerate().filter(|(_, w)| w.approach) {
            let near = pairs.iter().filter(|p| base_name(&p.og.name) == w.target).min_by(|a, b| {
                let d = |p: &&crate::compare::Pair| p.og.samples.iter().find(|s| s.frame >= w.start).map_or(f64::MAX, |s| (s.x - w.x).hypot(s.y - w.y));
                d(a).partial_cmp(&d(b)).unwrap()
            });
            let Some(p) = near else { continue };
            let Some(at) = p.og.samples.iter().filter(|s| s.frame <= w.start - 6).last().map(|s| (s.x, s.y)) else { continue };
            visits.push((wi, p.port.source_id, at));
            chosen.push((wi, p.og, p.port));
        }
        let ran = run_visits(*id, trace, &visits);
        for (wi, og, pa) in chosen {
            let w = &trace.windows[wi];
            let Some(pt) = ran.get(&wi) else { continue };
            let hero: Vec<(f64, f64)> = (w.start..w.end).map(|f| trace.hero.range(..=f).next_back().map_or((0.0, 0.0), |(_, h)| (h[0], h[1]))).collect();
            let o_pts: Vec<(f64, f64)> = og.samples.iter().filter(|s| s.frame >= w.start && s.frame < w.end).map(|s| (s.x, s.y)).collect();
            let from = (w.start - trace.origin) as usize;
            let p_pts: Vec<(f64, f64)> = pt[from.min(pt.len())..].iter().map(|t| (t.x, t.y)).collect();
            if o_pts.len() < 60 || p_pts.len() < 60 || p_pts.iter().any(|p| !p.0.is_finite()) {
                continue;
            }
            // Threshold: well above what the enemy does on its own (a Crawler's 4 u/s stays under it).
            let base = mean_speed(&o_pts[..30]).max(mean_speed(&p_pts[..30]));
            let threshold = (2.5 * base + 1.5).max(5.0);
            let (o, p) = (onset(&o_pts, &hero, threshold), onset(&p_pts, &hero, threshold));
            let react_ok = o.is_some() == p.is_some();
            let range_ok = match (o, p) {
                (Some(a), Some(b)) => (a.distance - b.distance).abs() <= (0.3 * a.distance.max(b.distance)).max(1.5),
                _ => true,
            };
            let row = by_family.entry(pa.family.clone()).or_default();
            row[0] += 1;
            row[1] += react_ok as u32;
            row[2] += (react_ok && range_ok) as u32;
            if std::env::var_os("HKBP_ALL").is_some() || !(react_ok && range_ok) {
                let show = |o: Option<Onset>| o.map_or("none".to_string(), |o| format!("frame {} at {:.1}", o.frame, o.distance));
                println!("{name} {:>6} {:<10} '{}' threshold {:.1} u/s | og {} | port {} | react:{} range:{}", pa.source_id, pa.family, og.name, threshold, show(o), show(p), react_ok as u8, range_ok as u8);
            }
        }
    }
    println!("\nfamily        walks  react  range");
    for (f, r) in &by_family {
        println!("{f:<12} {:>6} {:>6} {:>6}", r[0], r[1], r[2]);
    }
}
