//! The port's enemies against the original's, scene by scene: same camera, same time base, a hero
//! parked out of reach on both sides, so what is compared is what each enemy does on its own.
use super::*;
use crate::og_trace::{OgActor, SceneTrace};
use crate::replay::step;
use crate::world_data::{load_scene, RegionData};
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct PortActor {
    pub source_id: u32,
    pub family: String,
    pub spec_hp: i16,
    pub start: (f64, f64),
    pub ticks: Vec<Tick>,
}

/// One actor on one simulation tick.
#[derive(Clone, Copy, Debug)]
pub struct Tick {
    pub x: f64,
    pub y: f64,
    pub hp: i16,
    pub dead: bool,
    pub clip: u16,
    pub facing: i32,
}

pub fn family_of(spec: &ActorSpec) -> String {
    let dbg = format!("{:?}", spec.controller);
    dbg.split(|c: char| !c.is_alphanumeric()).next().unwrap_or("?").to_string()
}

fn q(v: i32) -> f64 {
    v as f64 / 65536.0
}

/// Runs every placement of `scene` in the region that contains it, with the og camera.
pub fn run_port_scene(scene: usize, trace: &SceneTrace, ticks: usize) -> Vec<PortActor> {
    let regions = load_scene(scene);
    let mut out: Vec<PortActor> = Vec::new();
    let mut taken: Vec<u32> = Vec::new();
    let camera_at = |k: usize| -> [i32; 3] {
        let f = trace.origin + k as i64;
        let c = trace.camera.range(..=f).next_back().map(|(_, c)| *c).or_else(|| trace.camera.values().next().copied()).unwrap_or([0.0, 0.0, -38.1]);
        [(c[0] * 65536.0).round() as i32, (c[1] * 65536.0).round() as i32, (c[2] * 65536.0).round() as i32]
    };
    for here in &regions {
        let mine: Vec<usize> = (0..here.actors.len())
            .filter(|&k| world::contains(here.bounds, here.actors[k].0.x, here.actors[k].0.y) && !taken.contains(&here.actors[k].0.source_id))
            .collect();
        if mine.is_empty() {
            continue;
        }
        let mut w = enemies::EnemyWorld::new();
        let mut player = Player::spawn(-150 * ONE, -150 * ONE);
        let mut vitals = Vitals::new(VITAL_PARAMS);
        let mut recs: Vec<PortActor> = mine
            .iter()
            .map(|&k| {
                let (p, spec) = here.actors[k];
                PortActor { source_id: p.source_id, family: family_of(spec), spec_hp: spec.health.health, start: (q(p.x), q(p.y)), ticks: Vec::with_capacity(ticks) }
            })
            .collect();
        for t in 0..ticks {
            // The Knight stands where the original's did: frozen at the entrance while the scene
            // settles (no trigger events there, which the port cannot tell from a standing
            // Knight, so pairs he stands near are set apart below), then beside each enemy.
            let f = trace.origin + t as i64;
            // (Position-driven rules such as the Walker's turn toward the Knight read it even
            // while he is frozen, so he is not parked out of the room.)
            if let Some(h) = trace.hero.range(..=f).next_back().map(|(_, h)| h) {
                player.x = (h[0] * 65536.0).round() as i32;
                player.y = (h[1] * 65536.0).round() as i32;
                player.facing = trace.face.range(..=f).next_back().map_or(1, |(_, v)| *v);
            }
            step(&mut w, here, &regions, &mut player, &mut vitals, camera_at(t));
            for r in recs.iter_mut() {
                r.ticks.push(match w.debug_actor(scene, r.source_id) {
                    Some(d) => Tick { x: q(d.x), y: q(d.y), hp: d.hp, dead: d.dead, clip: d.clip, facing: d.facing },
                    None => Tick { x: f64::NAN, y: f64::NAN, hp: 0, dead: true, clip: u16::MAX, facing: 0 },
                });
            }
        }
        taken.extend(recs.iter().map(|r| r.source_id));
        out.extend(recs);
    }
    out
}

#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub x: (f64, f64),
    pub y: (f64, f64),
    /// Path length per second over the whole window.
    pub speed: f64,
    pub moving: f64,
    pub turns: u32,
}

fn stats(points: impl Iterator<Item = (f64, f64)>, seconds: f64) -> Stats {
    let mut s = Stats { x: (f64::MAX, f64::MIN), y: (f64::MAX, f64::MIN), ..Default::default() };
    let (mut prev, mut length, mut moving, mut n) = (None::<(f64, f64)>, 0.0, 0usize, 0usize);
    let mut last_dir = 0i32;
    for (x, y) in points.filter(|p| p.0.is_finite()) {
        s.x = (s.x.0.min(x), s.x.1.max(x));
        s.y = (s.y.0.min(y), s.y.1.max(y));
        if let Some((px, py)) = prev {
            let d = ((x - px).powi(2) + (y - py).powi(2)).sqrt();
            length += d;
            if d > 1e-3 {
                moving += 1;
            }
            let dir = if (x - px).abs() > 1e-3 { (x - px).signum() as i32 } else { 0 };
            if dir != 0 {
                if last_dir != 0 && dir != last_dir {
                    s.turns += 1;
                }
                last_dir = dir;
            }
        }
        prev = Some((x, y));
        n += 1;
    }
    s.speed = length / seconds.max(1e-9);
    s.moving = moving as f64 / n.max(1) as f64;
    s
}

pub struct Pair<'a> {
    pub scene: String,
    pub og: &'a OgActor,
    pub port: &'a PortActor,
    pub og_stats: Stats,
    pub port_stats: Stats,
    /// The Knight stood within 12 units of the actor while the scene settled: the original's
    /// was frozen (no trigger events), the port's cannot be, so the pair is set apart.
    pub near_hero: bool,
}

/// One-to-one matches by distance between the og's first sample and the placement.
pub fn match_actors<'a>(scene: &str, trace: &'a SceneTrace, port: &'a [PortActor]) -> (Vec<Pair<'a>>, Vec<&'a OgActor>, Vec<&'a PortActor>) {
    let mut cands: Vec<(f64, usize, usize)> = Vec::new();
    for (i, o) in trace.actors.iter().enumerate() {
        let Some(first) = o.samples.first() else { continue };
        for (j, p) in port.iter().enumerate() {
            let d = ((first.x - p.start.0).powi(2) + (first.y - p.start.1).powi(2)).sqrt();
            if d < 8.0 {
                cands.push((d, i, j));
            }
        }
    }
    cands.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let (mut used_o, mut used_p) = (vec![false; trace.actors.len()], vec![false; port.len()]);
    let mut pairs = Vec::new();
    for (_, i, j) in cands {
        if used_o[i] || used_p[j] {
            continue;
        }
        used_o[i] = true;
        used_p[j] = true;
        let o = &trace.actors[i];
        let idle_end = trace.windows.iter().find(|w| !w.tour).map_or(i64::MAX, |w| w.end);
        let idle: Vec<&crate::og_trace::Sample> = o.samples.iter().filter(|s| s.frame < idle_end).collect();
        if idle.len() < 30 {
            continue;
        }
        let seconds = (idle.last().unwrap().frame - idle.first().unwrap().frame).max(1) as f64 / 60.0;
        let og_stats = stats(idle.iter().map(|s| (s.x, s.y)), seconds);
        let p = &port[j];
        let from = ((trace.first_frame - trace.origin).max(0) as usize).min(p.ticks.len());
        let to = trace.windows.iter().find(|w| !w.tour).map_or(p.ticks.len(), |w| (w.end - trace.origin) as usize).min(p.ticks.len());
        let port_stats = stats(p.ticks[from..to].iter().map(|t| (t.x, t.y)), (to - from) as f64 / 60.0);
        let near_hero = idle.iter().any(|s| trace.hero.get(&s.frame).map_or(false, |h| ((h[0] - s.x).powi(2) + (h[1] - s.y).powi(2)).sqrt() < 12.0));
        pairs.push(Pair { scene: scene.to_string(), og: o, port: p, og_stats, port_stats, near_hero });
    }
    let miss_o = trace.actors.iter().enumerate().filter(|(i, o)| !used_o[*i] && o.samples.len() > 30).map(|(_, o)| o).collect();
    let miss_p = port.iter().enumerate().filter(|(j, _)| !used_p[*j]).map(|(_, p)| p).collect();
    (pairs, miss_o, miss_p)
}

/// Pass/fail of one pair against the stated tolerances.
#[derive(Clone, Copy, Debug, Default)]
pub struct Verdict {
    pub hp: bool,
    pub x_env: bool,
    pub y_env: bool,
    pub speed: bool,
}

pub fn judge(pair: &Pair<'_>) -> Verdict {
    let (o, p) = (&pair.og_stats, &pair.port_stats);
    let span = (o.x.1 - o.x.0).max(p.x.1 - p.x.0);
    let tol_x = (0.10 * span).max(0.6);
    let tol_y = 0.35;
    let sp_tol = 0.15 * o.speed.max(p.speed) + 0.05;
    let og_hp = pair.og.samples.first().map_or(0, |s| s.hp);
    Verdict {
        hp: og_hp == pair.port.spec_hp as i32,
        x_env: (o.x.0 - p.x.0).abs() <= tol_x && (o.x.1 - p.x.1).abs() <= tol_x,
        y_env: (o.y.0 - p.y.0).abs() <= tol_y && (o.y.1 - p.y.1).abs() <= tol_y,
        speed: (o.speed - p.speed).abs() <= sp_tol,
    }
}

pub fn report(run: &std::path::Path, scenes_filter: Option<&str>, names: &BTreeMap<usize, String>) {
    let traces = crate::og_trace::load_run(run);
    let mut by_family: BTreeMap<String, [u32; 5]> = BTreeMap::new();
    let mut fails: Vec<String> = Vec::new();
    let mut near_count = BTreeMap::<String, u32>::new();
    let (mut og_missing, mut port_extra) = (BTreeMap::<String, u32>::new(), BTreeMap::<String, u32>::new());
    for (id, name) in names {
        if scenes_filter.map_or(false, |f| f != name) {
            continue;
        }
        let Some(trace) = traces.get(name) else { continue };
        let ticks = (trace.last_frame - trace.origin).max(0) as usize;
        let port = run_port_scene(*id, trace, ticks);
        let (pairs, miss_o, miss_p) = match_actors(name, trace, &port);
        for p in &pairs {
            if p.near_hero {
                *near_count.entry(p.port.family.clone()).or_default() += 1;
                continue;
            }
            let v = judge(p);
            let row = by_family.entry(p.port.family.clone()).or_default();
            row[0] += 1;
            row[1] += v.hp as u32;
            row[2] += v.x_env as u32;
            row[3] += v.y_env as u32;
            row[4] += v.speed as u32;
            if std::env::var_os("HKBP_ALL").is_some() || !(v.hp && v.x_env && v.y_env && v.speed) {
                fails.push(format!(
                    "{name} {:>6} {:<10} og '{}' x[{:.2},{:.2}] y[{:.2},{:.2}] v{:.2} | port x[{:.2},{:.2}] y[{:.2},{:.2}] v{:.2} | hp:{} x:{} y:{} v:{}",
                    p.port.source_id, p.port.family, p.og.name, p.og_stats.x.0, p.og_stats.x.1, p.og_stats.y.0, p.og_stats.y.1, p.og_stats.speed,
                    p.port_stats.x.0, p.port_stats.x.1, p.port_stats.y.0, p.port_stats.y.1, p.port_stats.speed, v.hp as u8, v.x_env as u8, v.y_env as u8, v.speed as u8
                ));
            }
        }
        for o in miss_o {
            *og_missing.entry(format!("{}", o.name)).or_default() += 1;
        }
        for p in miss_p {
            *port_extra.entry(p.family.clone()).or_default() += 1;
        }
    }
    println!("family        pairs   hp  x-env  y-env  speed");
    for (f, r) in &by_family {
        println!("{f:<12} {:>5} {:>5} {:>6} {:>6} {:>6}", r[0], r[1], r[2], r[3], r[4]);
    }
    println!("set apart (Knight stood within 12 units while the scene settled): {near_count:?}");
    println!("\nfailures:");
    for f in &fails {
        println!("{f}");
    }
    println!("\noriginal enemies with no port placement (name: count):");
    for (n, c) in &og_missing {
        println!("  {n}: {c}");
    }
    println!("port placements with no original match (family: count):");
    for (n, c) in &port_extra {
        println!("  {n}: {c}");
    }
}

/// `dump RUN SCENE SOURCE_ID FROM TO STEP`: the matched pair's positions side by side.
pub fn dump(run: &std::path::Path, names: &BTreeMap<usize, String>, args: &[String]) {
    let (scene_name, source_id) = (&args[0], args[1].parse::<u32>().unwrap());
    let (from, to, stepn): (usize, usize, usize) = (args[2].parse().unwrap(), args[3].parse().unwrap(), args[4].parse().unwrap());
    let traces = crate::og_trace::load_run(run);
    let trace = &traces[scene_name];
    let id = *names.iter().find(|(_, n)| *n == scene_name).unwrap().0;
    let ticks = (trace.last_frame - trace.origin).max(0) as usize;
    let port = run_port_scene(id, trace, ticks);
    let (pairs, _, _) = match_actors(scene_name, trace, &port);
    let pair = pairs.iter().find(|p| p.port.source_id == source_id).expect("pair");
    println!("og '{}' vs port {}  (tick = og frame - {})", pair.og.name, source_id, trace.origin);
    for w in &trace.windows {
        println!("  window {} '{}' ticks {}..{}", if w.tour { "tour" } else { "idle" }, w.target, w.start - trace.origin, w.end - trace.origin);
    }
    for t in (from..to.min(pair.port.ticks.len())).step_by(stepn) {
        let f = trace.origin + t as i64;
        let o = pair.og.samples.iter().find(|s| s.frame == f);
        let p = &pair.port.ticks[t];
        let hero = trace.hero.get(&f).map(|h| format!("hero({:.1},{:.1})", h[0], h[1])).unwrap_or_default();
        match o {
            Some(o) => println!("{t:5}  og ({:8.3},{:8.3}) {:<34} | port ({:8.3},{:8.3}) clip {:3} face {:2} {hero}", o.x, o.y, o.fsm.chars().take(34).collect::<String>(), p.x, p.y, p.clip, p.facing),
            None => println!("{t:5}  og (none)                                              | port ({:8.3},{:8.3}) clip {:3} face {:2} {hero}", p.x, p.y, p.clip, p.facing),
        }
    }
}
