//! The Knight beside an enemy, in the original and in the port.
//!
//! The survey's tour stands the Knight (invincible) on each distinct enemy in turn for a few seconds.
//! The port's enemy is stood where the original's was when the visit began and the Knight is
//! replayed from the original's own trace, so what is compared is the reaction: does it move at
//! once, how fast, how far, how close does it get to the Knight. The walk direction and the
//! phase of its timers at that moment are not shared, so only the reaction is judged.
use super::*;
use crate::compare::{match_actors, run_port_scene, PortActor, Tick};
use crate::og_trace::{load_run, OgActor, SceneTrace, Window};
use crate::replay::step;
use crate::world_data::load_scene;
use std::collections::BTreeMap;

fn q(v: i32) -> f64 {
    v as f64 / 65536.0
}

/// "Zombie Runner (2)" and "Buzzer 3" are instances of "Zombie Runner" and "Buzzer".
pub fn base_name(n: &str) -> String {
    let mut s = n.replace("(Clone)", "").trim().to_string();
    if s.ends_with(')') {
        if let Some(i) = s.rfind(" (") {
            s.truncate(i);
        }
    }
    let t = s.trim_end_matches(|c: char| c.is_ascii_digit()).to_string();
    if t.len() < s.len() && t.ends_with(' ') {
        s = t.trim_end().to_string();
    }
    s.trim().to_string()
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Reaction {
    /// Path length over the visit, in units.
    pub path: f64,
    /// Fastest 3-frame stretch, in units per second.
    pub vmax: f64,
    /// Closest the enemy came to the Knight.
    pub nearest: f64,
    /// Frames from the visit's start to the first stretch faster than 3 units per second.
    pub onset: Option<usize>,
}

fn reaction(points: &[(f64, f64)], hero: &[(f64, f64)]) -> Reaction {
    let mut r = Reaction { nearest: f64::MAX, ..Default::default() };
    for i in 1..points.len() {
        r.path += (points[i].0 - points[i - 1].0).hypot(points[i].1 - points[i - 1].1);
    }
    for i in 3..points.len() {
        let v = (points[i].0 - points[i - 3].0).hypot(points[i].1 - points[i - 3].1) * 20.0;
        r.vmax = r.vmax.max(v);
        if v > 3.0 && r.onset.is_none() {
            r.onset = Some(i);
        }
    }
    for (p, h) in points.iter().zip(hero) {
        r.nearest = r.nearest.min((p.0 - h.0).hypot(p.1 - h.1));
    }
    r
}

/// The port's actors for `scene`, run again with each visit's target stood where the original's was.
pub(crate) fn run_visits(scene: usize, trace: &SceneTrace, visits: &[(usize, u32, (f64, f64))]) -> BTreeMap<usize, Vec<Tick>> {
    let regions = load_scene(scene);
    let mut out = BTreeMap::new();
    let end = (trace.last_frame - trace.origin).max(0) as usize;
    for (window, source_id, at) in visits {
        let w = &trace.windows[*window];
        let Some(mut here) = regions.iter().find(|r| r.actors.iter().any(|(p, _)| p.source_id == *source_id && world::contains(r.bounds, p.x, p.y))) else { continue };
        let mut world_ = enemies::EnemyWorld::new();
        let mut vitals = Vitals::new(VITAL_PARAMS);
        let mut player = Player::spawn(-150 * ONE, -150 * ONE);
        let sync = (w.start - trace.origin - 6).max(0) as usize;
        let mut ticks = Vec::new();
        for t in 0..end.min((w.end - trace.origin) as usize) {
            let f = trace.origin + t as i64;
            if let Some(h) = trace.hero.range(..=f).next_back().map(|(_, h)| h) {
                player.x = (h[0] * 65536.0).round() as i32;
                player.y = (h[1] * 65536.0).round() as i32;
                player.facing = trace.face.range(..=f).next_back().map_or(1, |(_, v)| *v);
            }
            if t == sync {
                world_.debug_place(scene, *source_id, (at.0 * 65536.0).round() as i32, (at.1 * 65536.0).round() as i32);
            }
            let c = trace.camera.range(..=f).next_back().map(|(_, c)| *c).unwrap_or([0.0, 0.0, -38.1]);
            let cam = [(c[0] * 65536.0).round() as i32, (c[1] * 65536.0).round() as i32, (c[2] * 65536.0).round() as i32];
            step(&mut world_, here, &regions, &mut player, &mut vitals, cam);
            let d = world_.debug_actor(scene, *source_id);
            if let Some(d) = d {
                if !world::contains(here.bounds, d.x, d.y) {
                    if let Some(next) = regions.iter().find(|r| world::contains(r.bounds, d.x, d.y)) {
                        here = next;
                    }
                }
            }
            ticks.push(match d {
                Some(d) => Tick { x: q(d.x), y: q(d.y), hp: d.hp, dead: d.dead, clip: d.clip, facing: d.facing, phase: d.phase },
                None => Tick { x: f64::NAN, y: f64::NAN, hp: 0, dead: true, clip: u16::MAX, facing: 0, phase: [0; 2] },
            });
        }
        out.insert(*window, ticks);
    }
    out
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
        // One visit per tour window: the pair of the visited type nearest the window's target.
        let mut visits: Vec<(usize, u32, (f64, f64))> = Vec::new();
        let mut chosen: Vec<(usize, &OgActor, &PortActor)> = Vec::new();
        for (wi, w) in trace.windows.iter().enumerate().filter(|(_, w)| w.tour && !w.poke && !w.approach) {
            let near = pairs
                .iter()
                .filter(|p| base_name(&p.og.name) == w.target)
                .min_by(|a, b| {
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
            let w: &Window = &trace.windows[wi];
            let Some(pt) = ran.get(&wi) else { continue };
            let hero: Vec<(f64, f64)> = (w.start..w.end).map(|f| trace.hero.range(..=f).next_back().map_or((0.0, 0.0), |(_, h)| (h[0], h[1]))).collect();
            let o_pts: Vec<(f64, f64)> = og.samples.iter().filter(|s| s.frame >= w.start && s.frame < w.end).map(|s| (s.x, s.y)).collect();
            let from = (w.start - trace.origin) as usize;
            let p_pts: Vec<(f64, f64)> = pt[from.min(pt.len())..].iter().map(|t| (t.x, t.y)).filter(|p| p.0.is_finite()).collect();
            if o_pts.len() < 30 || p_pts.len() < 30 {
                continue;
            }
            let o = reaction(&o_pts, &hero);
            let p = reaction(&p_pts, &hero);
            let both_idle = o.onset.is_none() && p.onset.is_none();
            let react_ok = o.onset.is_some() == p.onset.is_some();
            let speed_ok = both_idle || (o.vmax - p.vmax).abs() <= (0.35 * o.vmax.max(p.vmax)).max(1.0);
            let near_ok = (o.nearest - p.nearest).abs() <= (0.35 * o.nearest.max(p.nearest)).max(1.5);
            let row = by_family.entry(pa.family.clone()).or_default();
            row[0] += 1;
            row[1] += react_ok as u32;
            row[2] += speed_ok as u32;
            row[3] += near_ok as u32;
            if std::env::var_os("HKBP_ALL").is_some() || !(react_ok && speed_ok && near_ok) {
                println!(
                    "{name} {:>6} {:<10} '{}' og path {:6.1} vmax {:5.1} nearest {:5.1} onset {:?} | port path {:6.1} vmax {:5.1} nearest {:5.1} onset {:?} | react:{} speed:{} near:{}",
                    pa.source_id, pa.family, og.name, o.path, o.vmax, o.nearest, o.onset, p.path, p.vmax, p.nearest, p.onset, react_ok as u8, speed_ok as u8, near_ok as u8
                );
            }
        }
    }
    println!("\nfamily        visits  react  speed  nearest");
    for (f, r) in &by_family {
        println!("{f:<12} {:>7} {:>6} {:>6} {:>8}", r[0], r[1], r[2], r[3]);
    }
}
