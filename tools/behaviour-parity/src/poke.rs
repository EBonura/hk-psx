//! Nail strikes on each enemy, in the original and in the port, and what the enemy does next.
//!
//! The original's driver (`tools/hkref`, survey poke mode) strikes each distinct enemy of a scene
//! through `HealthManager.Hit` with the Knight's no-charm nail from the left, every 30 frames, and
//! logs the enemy per frame. Here the same scene is run natively with the enemy stood where the
//! original's was when its series began and the same strikes landed on the same ticks, and the two
//! are compared: hit points after each strike, how many it takes, how far and how fast the enemy
//! recoils, where the corpse comes to rest.
use super::*;
use crate::compare::{match_actors, run_port_scene, PortActor, Tick};
use crate::og_trace::{load_pokes, load_run, OgActor, PokeHit, SceneTrace};
use crate::replay::step_with;
use crate::world_data::{load_scene, RegionData};
use std::collections::BTreeMap;

/// What one enemy did over its series of strikes, from either side.
#[derive(Clone, Debug, Default)]
pub struct Series {
    /// Hit points after each strike (the original's own record, the port's from its state).
    pub hp: Vec<i32>,
    /// Signed x displacement from the pre-strike position, 6, 12 and 24 frames after the first strike.
    pub dx: [f64; 3],
    pub dy: [f64; 3],
    /// Frames from the first strike until the enemy is dead (None: never within the window).
    pub death: Option<usize>,
    /// Distance the enemy travelled between its death and the end of the window.
    pub corpse_travel: f64,
}

fn q(v: i32) -> f64 {
    v as f64 / 65536.0
}

fn series(
    hits: &[i64],
    start: i64,
    positions: impl Fn(i64) -> Option<(f64, f64, i32, bool)>,
    end: i64,
) -> Series {
    let mut s = Series::default();
    let Some(&first) = hits.first() else { return s };
    let (px, py, _, _) = positions(first - 1)
        .or_else(|| positions(first))
        .unwrap_or((f64::NAN, f64::NAN, 0, false));
    // The walk goes on under the recoil (and resumes after it) in whichever direction the enemy
    // was going, which the two sides need not share, so the displacement is measured against
    // that walk carried on from the five frames before the strike.
    let (vx, vy) = match positions(first - 6) {
        Some((x0, y0, _, _)) => ((px - x0) / 5.0, (py - y0) / 5.0),
        None => (0.0, 0.0),
    };
    for (k, off) in [6i64, 12, 24].into_iter().enumerate() {
        if let Some((x, y, _, _)) = positions(first + off) {
            let t = (off + 1) as f64;
            s.dx[k] = x - (px + vx * t);
            s.dy[k] = y - (py + vy * t);
        }
    }
    for &h in hits {
        if let Some((_, _, hp, _)) = positions(h + 2) {
            s.hp.push(hp);
        }
    }
    let mut dead_at = None;
    for f in first..end {
        if positions(f).map_or(false, |p| p.3) {
            dead_at = Some(f);
            break;
        }
    }
    s.death = dead_at.map(|d| (d - first) as usize);
    if let Some(d) = dead_at {
        let mut last = positions(d).map(|p| (p.0, p.1));
        let mut travel = 0.0;
        for f in d + 1..end {
            if let (Some((lx, ly)), Some((x, y, _, _))) = (last, positions(f)) {
                travel += ((x - lx).powi(2) + (y - ly).powi(2)).sqrt();
                last = Some((x, y));
            }
        }
        s.corpse_travel = travel;
    }
    let _ = start;
    s
}

pub fn og_series(actor: &OgActor, hits: &[&PokeHit], end: i64) -> Series {
    let by_frame: BTreeMap<i64, &crate::og_trace::Sample> =
        actor.samples.iter().map(|s| (s.frame, s)).collect();
    let frames: Vec<i64> = hits.iter().map(|h| h.frame).collect();
    // The original's driver stamps the frame it struck on; the strike lands that frame's physics.
    let mut s = series(
        &frames,
        frames.first().copied().unwrap_or(0),
        |f| by_frame.get(&f).map(|s| (s.x, s.y, s.hp, s.dead)),
        end,
    );
    s.hp = hits.iter().map(|h| h.hp_after).collect();
    // The original keeps an enemy in its trace only while it is active: when it vanishes it is dead.
    if s.death.is_none() {
        if let Some(h) = hits.iter().find(|h| h.dead) {
            s.death = Some((h.frame - frames[0]) as usize);
        }
    }
    s
}

pub struct PortSeries {
    pub series: Series,
}

/// Run the scene's strike windows natively. Returns the port actors' ticks (index = tick since the
/// trace origin) for the matched targets, with the strikes landed.
pub fn run_port_pokes(
    scene: usize,
    trace: &SceneTrace,
    targets: &[(u32, Vec<i64>, (f64, f64))],
) -> Vec<(u32, Vec<Tick>)> {
    let regions = load_scene(scene);
    let end = (trace.last_frame - trace.origin).max(0) as usize;
    let mut out: Vec<(u32, Vec<Tick>)> = Vec::new();
    for (source_id, hit_frames, sync_at) in targets {
        // The region the actor starts in, as the idle comparison uses.
        let Some(mut here) = regions.iter().find(|r| {
            r.actors.iter().any(|(p, _)| p.source_id == *source_id)
                && r.actors
                    .iter()
                    .any(|(p, _)| p.source_id == *source_id && world::contains(r.bounds, p.x, p.y))
        }) else {
            continue;
        };
        let mut w = enemies::EnemyWorld::new();
        let mut vitals = Vitals::new(VITAL_PARAMS);
        let mut player = Player::spawn(-150 * ONE, -150 * ONE);
        let first_hit = hit_frames[0] - trace.origin;
        let camera_at = |k: usize| -> [i32; 3] {
            let f = trace.origin + k as i64;
            let c = trace
                .camera
                .range(..=f)
                .next_back()
                .map(|(_, c)| *c)
                .unwrap_or([0.0, 0.0, -38.1]);
            [
                (c[0] * 65536.0).round() as i32,
                (c[1] * 65536.0).round() as i32,
                (c[2] * 65536.0).round() as i32,
            ]
        };
        let mut ticks = Vec::new();
        // Settle a little before the first strike, from the original's position at the same moment.
        let sync_tick = (first_hit - 20).max(0);
        for t in 0..end.min((hit_frames.last().unwrap() - trace.origin) as usize + 120) {
            let f = trace.origin + t as i64;
            if let Some(h) = trace.hero.range(..=f).next_back().map(|(_, h)| h) {
                player.x = (h[0].clamp(-500.0, 500.0) * 65536.0).round() as i32;
                player.y = (h[1].clamp(-500.0, 500.0) * 65536.0).round() as i32;
            }
            if t as i64 == sync_tick {
                w.debug_place(
                    scene,
                    *source_id,
                    (sync_at.0 * 65536.0).round() as i32,
                    (sync_at.1 * 65536.0).round() as i32,
                );
            }
            // The tick a strike lands on: the Knight beside the enemy, facing right, swinging.
            let hit_now = hit_frames.iter().any(|h| h - trace.origin == t as i64);
            let mut nail = Nail::new();
            let mut strike = None;
            if hit_now {
                // The original struck from the left with the Knight where he stood (frozen at the
                // entrance), so he stays there and the nail reaches the actor's body wherever it is.
                if let Some(d) = w.debug_actor(scene, *source_id) {
                    player.facing = 1;
                    nail.active = true;
                    nail.age = 0;
                    strike = Some([d.x - 2 * ONE, d.y - 2 * ONE, d.x + 2 * ONE, d.y + 2 * ONE]);
                }
            }
            step_with(
                &mut w,
                here,
                &regions,
                &mut player,
                &mut vitals,
                camera_at(t),
                &nail,
                strike,
            );
            let d = w.debug_actor(scene, *source_id);
            if let Some(d) = &d {
                if !world::contains(here.bounds, d.x, d.y) {
                    if let Some(next) = regions.iter().find(|r| world::contains(r.bounds, d.x, d.y))
                    {
                        here = next;
                    }
                }
            }
            ticks.push(match d {
                Some(d) => Tick {
                    x: q(d.x),
                    y: q(d.y),
                    hp: d.hp,
                    dead: d.dead,
                    clip: d.clip,
                    facing: d.facing,
                    phase: d.phase,
                    detail: crate::compare::detail_id(&d.detail),
                },
                None => Tick {
                    x: f64::NAN,
                    y: f64::NAN,
                    hp: 0,
                    dead: true,
                    clip: u16::MAX,
                    facing: 0,
                    phase: [0; 2],
                    detail: 0,
                },
            });
        }
        out.push((*source_id, ticks));
    }
    out
}

pub fn report(run: &std::path::Path, names: &BTreeMap<usize, String>, only: Option<&str>) {
    let traces = load_run(run);
    let pokes = load_pokes(run);
    let (mut total, mut ok_hp, mut ok_kill, mut ok_recoil) = (0u32, 0u32, 0u32, 0u32);
    let mut by_family: BTreeMap<String, [u32; 5]> = BTreeMap::new();
    for (id, name) in names {
        if only.map_or(false, |o| o != name) {
            continue;
        }
        let Some(trace) = traces.get(name) else {
            continue;
        };
        let ticks = (trace.last_frame - trace.origin).max(0) as usize;
        let port = run_port_scene(*id, trace, ticks);
        let (pairs, _, _) = match_actors(name, trace, &port);
        // Hits by original actor id.
        let mut hits: BTreeMap<String, Vec<&PokeHit>> = BTreeMap::new();
        for h in pokes.iter().filter(|h| &h.scene == name) {
            hits.entry(h.id.clone()).or_default().push(h);
        }
        let mut targets: Vec<(u32, Vec<i64>, (f64, f64))> = Vec::new();
        let mut meta: Vec<(&PortActor, &OgActor, Vec<&PokeHit>)> = Vec::new();
        for p in &pairs {
            let Some(hs) = hits.get(&p.og.id) else {
                continue;
            };
            let first = hs[0].frame;
            let at =
                p.og.samples
                    .iter()
                    .filter(|s| s.frame <= first - 20)
                    .last()
                    .or(p.og.samples.first())
                    .map(|s| (s.x, s.y))
                    .unwrap_or(p.port.start);
            targets.push((p.port.source_id, hs.iter().map(|h| h.frame).collect(), at));
            meta.push((p.port, p.og, hs.clone()));
        }
        let results = run_port_pokes(*id, trace, &targets);
        for (source_id, ticks) in &results {
            let Some((port_actor, og, hs)) = meta.iter().find(|m| m.0.source_id == *source_id)
            else {
                continue;
            };
            let hit_frames: Vec<i64> = hs.iter().map(|h| h.frame).collect();
            let end = hit_frames.last().unwrap() + 100;
            let o = og_series(og, hs, end);
            let p = series(
                &hit_frames,
                hit_frames[0],
                |f| {
                    let t = (f - trace.origin) as usize;
                    ticks.get(t).map(|k| (k.x, k.y, k.hp as i32, k.dead))
                },
                end,
            );
            total += 1;
            let hp_ok =
                o.hp.iter()
                    .zip(&p.hp)
                    .all(|(a, b)| (*a).max(-1) == (*b).max(-1))
                    && o.hp.len() <= p.hp.len() + 1;
            let kill_ok = o.death.is_some() == p.death.is_some();
            // The recoil, 12 frames on, within a quarter of the original's travel or half a unit.
            let r_ok = (o.dx[1] - p.dx[1]).abs() <= (0.25 * o.dx[1].abs()).max(0.5)
                && (o.dy[1] - p.dy[1]).abs() <= 0.5;
            ok_hp += hp_ok as u32;
            ok_kill += kill_ok as u32;
            ok_recoil += r_ok as u32;
            let row = by_family.entry(port_actor.family.clone()).or_default();
            row[0] += 1;
            row[1] += hp_ok as u32;
            row[2] += kill_ok as u32;
            row[3] += r_ok as u32;
            if std::env::var_os("HKBP_ALL").is_some() || !(hp_ok && kill_ok && r_ok) {
                println!(
                    "{name} {:>6} {:<10} '{}' hp og {:?} port {:?} | dx6/12/24 og {:.2}/{:.2}/{:.2} port {:.2}/{:.2}/{:.2} | dy12 og {:.2} port {:.2} | death og {:?} port {:?} corpse og {:.1} port {:.1} | hp:{} kill:{} recoil:{}",
                    port_actor.source_id, port_actor.family, og.name, o.hp, p.hp, o.dx[0], o.dx[1], o.dx[2], p.dx[0], p.dx[1], p.dx[2], o.dy[1], p.dy[1], o.death, p.death,
                    o.corpse_travel, p.corpse_travel, hp_ok as u8, kill_ok as u8, r_ok as u8
                );
            }
        }
    }
    println!("\nfamily        series   hp  kill  recoil");
    for (f, r) in &by_family {
        println!("{f:<12} {:>6} {:>4} {:>5} {:>7}", r[0], r[1], r[2], r[3]);
    }
    println!("all: {total} series, hp {ok_hp}, kill {ok_kill}, recoil {ok_recoil}");
}
