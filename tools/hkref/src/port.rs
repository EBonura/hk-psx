//! Port side: replay a tape on the PSoXide headless frontend and read the
//! guest's exported HK_* words every vblank, then collapse them to sim ticks.

use crate::profile::Profile;
use crate::tape;
use crate::trace::{Event, Trace};
use crate::util::{self, append_pid};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Words always watched; the hero and camera channels need them.
const CORE: &[&str] = &[
    "HK_PLAYER_X",
    "HK_PLAYER_Y",
    "HK_PLAYER_FACING",
    "HK_HEALTH",
    "HK_SOUL",
    "HK_SIM_TICKS",
    "HK_SIM_PAD",
    "HK_CAMERA_X",
    "HK_CAMERA_Y",
    "HK_REGION_ID",
    "HK_GAME_MODE",
];

pub fn symbols(map: &Path) -> Result<BTreeMap<String, u32>, String> {
    let t = fs::read_to_string(map).map_err(|e| format!("{}: {e}", map.display()))?;
    let mut m = BTreeMap::new();
    for l in t.lines() {
        let c: Vec<&str> = l.split_whitespace().collect();
        if c.len() == 5 {
            if let Ok(a) = u32::from_str_radix(c[0], 16) {
                m.insert(c[4].to_string(), a);
            }
        }
    }
    Ok(m)
}

pub fn run(p: &Profile, work: &Path) -> Result<PathBuf, String> {
    let out = p.dir.join("port");
    fs::create_dir_all(out.join("emulator")).map_err(|e| e.to_string())?;
    let sym = symbols(&p.map)?;
    // tape
    let tape_path = out.join("input.pxtape");
    if let Some(ev) = &p.events {
        tape::write(&tape_path, &tape::from_events(ev, p.polls)?)?;
    } else if let Some(t) = &p.tape {
        fs::copy(t, &tape_path).map_err(|e| e.to_string())?;
    } else {
        return Err("port needs events or tape".into());
    }
    let mut cmd = Command::new(&p.frontend);
    cmd.args(["launch", "--path"])
        .arg(&p.disc)
        .args(["--embedded-playtest", "--config-dir"])
        .arg(out.join("emulator"))
        .args(["--steps", "6000000000", "--input-tape"])
        .arg(&tape_path)
        .args(["--stop-at-poll", &p.polls.to_string(), "--route-log"])
        .arg(out.join("route.csv"));
    if p.shot_interval > 0 {
        let d = out.join("shots");
        fs::create_dir_all(&d).map_err(|e| e.to_string())?;
        cmd.arg("--route-screenshot-dir")
            .arg(d)
            .args(["--route-screenshot-interval", &p.shot_interval.to_string()]);
    }
    if let Some(card) = &p.card {
        let c = out.join("card.mcd");
        fs::copy(card, &c).map_err(|e| format!("card: {e}"))?;
        cmd.arg("--memcard").arg(c);
    }
    let mut names: Vec<String> = CORE.iter().map(|s| s.to_string()).collect();
    names.extend(p.watch.iter().map(|w| w.symbol.clone()));
    names.extend(p.counters.iter().map(|c| c.0.clone()));
    names.sort();
    names.dedup();
    for n in &names {
        let a = sym
            .get(n)
            .ok_or(format!("symbol {n} not in {}", p.map.display()))?;
        cmd.arg("--route-watch-u32").arg(format!("0x{a:08x}"));
    }
    let log = fs::File::create(out.join("replay.log")).map_err(|e| e.to_string())?;
    let child = cmd
        .stdout(Stdio::from(log.try_clone().map_err(|e| e.to_string())?))
        .stderr(Stdio::from(log))
        .spawn()
        .map_err(|e| format!("frontend: {e}"))?;
    append_pid(work, &format!("port-{}", p.name), child.id());
    eprintln!(
        "[port] frontend pid {} replaying {} polls",
        child.id(),
        p.polls
    );
    let st = util::wait_with_timeout(child, 1800)?;
    if !st {
        return Err("frontend failed or timed out".into());
    }
    Ok(out)
}

fn signed(v: u32) -> f64 {
    v as i32 as f64
}

/// Collapse the per-vblank rows to one row per sim tick (the last row that shows
/// that tick count is the state at its end) and name the channels.
/// The tick-collapsed trace, its events and the tick-to-frame map.
pub type Normalised = (Trace, Vec<Event>, BTreeMap<i64, u32>);

pub fn normalise(p: &Profile) -> Result<Normalised, String> {
    let out = p.dir.join("port");
    let sym = symbols(&p.map)?;
    let text = fs::read_to_string(out.join("route.csv")).map_err(|e| e.to_string())?;
    let mut lines = text.lines();
    let head: Vec<&str> = lines.next().ok_or("empty route.csv")?.split(',').collect();
    let col = |name: &str| -> Result<usize, String> {
        let a = sym.get(name).ok_or(format!("symbol {name}"))?;
        let h = format!("ram_{a:08x}");
        head.iter()
            .position(|x| *x == h)
            .ok_or(format!("route.csv lacks {h}"))
    };
    let (rt, rp) = (
        head.iter()
            .position(|x| *x == "route_tick")
            .ok_or("route_tick")?,
        head.iter()
            .position(|x| *x == "port1_polls")
            .ok_or("port1_polls")?,
    );
    let c = |n: &str| col(n);
    let (cx, cy, cf, chp, csoul, cst, cpad, ccx, ccy, creg, cmode) = (
        c("HK_PLAYER_X")?,
        c("HK_PLAYER_Y")?,
        c("HK_PLAYER_FACING")?,
        c("HK_HEALTH")?,
        c("HK_SOUL")?,
        c("HK_SIM_TICKS")?,
        c("HK_SIM_PAD")?,
        c("HK_CAMERA_X")?,
        c("HK_CAMERA_Y")?,
        c("HK_REGION_ID")?,
        c("HK_GAME_MODE")?,
    );
    let extra: Vec<(usize, &crate::profile::Watch)> = p
        .watch
        .iter()
        .map(|w| Ok((col(&w.symbol)?, w)))
        .collect::<Result<_, String>>()?;
    let ctr: Vec<(usize, &str)> = p
        .counters
        .iter()
        .map(|(s, e)| Ok((col(s)?, e.as_str())))
        .collect::<Result<_, String>>()?;
    // last row per sim tick
    let mut last: BTreeMap<i64, Vec<u32>> = BTreeMap::new();
    let mut route_of: BTreeMap<i64, u32> = BTreeMap::new();
    for l in lines {
        let r: Vec<&str> = l.split(',').collect();
        if r.len() != head.len() {
            continue;
        }
        let v: Vec<u32> = r
            .iter()
            .map(|x| x.parse::<u64>().unwrap_or(0) as u32)
            .collect();
        let tick = v[cst] as i64;
        if tick == 0 {
            continue;
        }
        last.insert(tick, v.clone());
        route_of.insert(tick, v[rt]);
        let _ = rp;
    }
    let mut tr = Trace::default();
    let mut ev = Vec::new();
    let mut prev_ctr: Vec<Option<u32>> = vec![None; ctr.len()];
    for (tick, v) in &last {
        tr.push_tick(*tick);
        tr.set("hero.x", signed(v[cx]) / 65536.0);
        tr.set("hero.y", signed(v[cy]) / 65536.0);
        tr.set("hero.face", if signed(v[cf]) < 0.0 { -1.0 } else { 1.0 });
        tr.set("hero.hp", v[chp] as f64);
        tr.set("hero.soul", v[csoul] as f64);
        tr.set("cam.x", signed(v[ccx]) / 65536.0);
        tr.set("cam.y", signed(v[ccy]) / 65536.0);
        tr.set("port.region", v[creg] as f64);
        tr.set("port.mode", v[cmode] as f64);
        tr.set("input.pad", v[cpad] as f64);
        for (i, w) in &extra {
            let raw = if w.signed {
                signed(v[*i])
            } else {
                v[*i] as f64
            };
            tr.set(&w.chan, raw / w.scale);
        }
        if let Some(prev) = tr.get("hero.hp", *tick - 1).or_else(|| {
            if tr.len() >= 2 {
                tr.num["hero.hp"].get(tr.len() - 2).copied()
            } else {
                None
            }
        }) {
            let now = v[chp] as f64;
            if prev.is_finite() && now < prev {
                ev.push(Event {
                    tick: *tick,
                    kind: "hero.hurt".into(),
                    name: String::new(),
                    value: prev - now,
                });
            }
        }
        for (k, (i, name)) in ctr.iter().enumerate() {
            if let Some(pv) = prev_ctr[k] {
                if v[*i] > pv {
                    ev.push(Event {
                        tick: *tick,
                        kind: name.to_string(),
                        name: String::new(),
                        value: (v[*i] - pv) as f64,
                    });
                }
            }
            prev_ctr[k] = Some(v[*i]);
        }
    }
    Ok((densify(&tr), ev, route_of))
}

/// The guest sometimes simulates two ticks inside one vblank (a catch-up burst
/// after a slow frame), so the per-vblank sample misses the first of them. Fill
/// those ticks by linear interpolation and flag them in `port.catchup` (the
/// count of such ticks is itself a frame-dip measure).
fn densify(tr: &Trace) -> Trace {
    let mut out = Trace::default();
    for (i, &t) in tr.ticks.iter().enumerate() {
        if i > 0 {
            let prev = tr.ticks[i - 1];
            for m in prev + 1..t {
                let a = (m - prev) as f64 / (t - prev) as f64;
                out.push_tick(m);
                for (k, v) in &tr.num {
                    let (x, y) = (v[i - 1], v[i]);
                    if k.starts_with("input.") || k.starts_with("port.") {
                        continue;
                    }
                    if x.is_finite() && y.is_finite() {
                        out.set(
                            k,
                            if k.ends_with(".x") || k.ends_with(".y") {
                                x + (y - x) * a
                            } else {
                                x
                            },
                        );
                    }
                }
                out.set("port.catchup", 1.0);
            }
        }
        out.push_tick(t);
        for (k, v) in &tr.num {
            if v[i].is_finite() {
                out.set(k, v[i]);
            }
        }
        for (k, v) in &tr.text {
            out.set_text(k, &v[i]);
        }
        out.set("port.catchup", 0.0);
    }
    out
}

/// The poll index a sim tick consumed: find `offset` with `pad(t) == tape[t + offset]`.
pub fn tape_offset(tr: &Trace, masks: &[u16]) -> Option<(usize, usize)> {
    let mut best = (usize::MAX, 0usize);
    for off in 0..masks.len().min(4000) {
        let mut bad = 0usize;
        let mut n = 0usize;
        for (i, t) in tr.ticks.iter().enumerate() {
            let idx = *t as usize + off;
            if idx >= masks.len() {
                break;
            }
            let pv = tr.num["input.pad"][i];
            if !pv.is_finite() {
                continue;
            }
            let pad = pv as u16;
            n += 1;
            if pad != masks[idx] {
                bad += 1;
            }
        }
        if n * 10 >= tr.ticks.len() * 9 && bad < best.0 {
            best = (bad, off);
        }
    }
    if best.0 == usize::MAX {
        None
    } else {
        Some((best.1, best.0))
    }
}
