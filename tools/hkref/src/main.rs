//! hkref: drive the original Hollow Knight (macOS Steam build, instrumented) and
//! the PS1 port (headless PSoXide) from one input tape and one scene profile,
//! normalise both into a common per-tick trace, and report where they diverge.
//!
//! Usage: hkref [--work DIR] <command> ...
//!   build                         compile the in-game driver and patch the CoW clone
//!   tape gen OUT EVENTS COUNT     events like 100:right:40,150:cross:12
//!   tape info TAPE
//!   port PROFILE                  replay on the port, write traces + window
//!   og PROFILE                    replay the same inputs on the original
//!   sweep PROFILE 1,2,3           original under several RNG seeds vs the port
//!   scenes PROFILE                survey og.sweep scenes (names, Prefix_* or *): actors, FSM audio, sounds
//!   scenes-summary PROFILE        rebuild the survey files from the runs already on disk
//!   diff PROFILE                  channel/event diff + plots
//!   sheet PROFILE                 side-by-side PNG sheets at matched ticks
//!   all PROFILE                   port, og, diff, sheet
//!   imgstat PNG...                mean RGB of each PNG (a black-frame check)
//!   montage OUT.png COLS PNG...   contact sheet, each image scaled to HKREF_MONTAGE_WIDTH (default 480)

mod diff;
mod img;
mod og;
mod port;
mod profile;
mod survey;
mod tape;
mod trace;
mod util;

use profile::Profile;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use trace::Trace;

fn work_dir(args: &mut Vec<String>) -> PathBuf {
    if let Some(i) = args.iter().position(|a| a == "--work") {
        let d = args.remove(i + 1);
        args.remove(i);
        return PathBuf::from(d);
    }
    std::env::var("HKREF_WORK")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().unwrap())
}

fn load(p: &str, work: &Path) -> Result<Profile, String> {
    Profile::load(Path::new(p), work)
}

fn still(tr: &Trace, t: i64) -> bool {
    (1..=8).all(|d| {
        match (
            tr.get("hero.x", t - d),
            tr.get("hero.x", t),
            tr.get("hero.y", t - d),
            tr.get("hero.y", t),
        ) {
            (Some(a), Some(b), Some(c), Some(e)) => (a - b).abs() < 1e-4 && (c - e).abs() < 1e-4,
            _ => false,
        }
    })
}

fn cmd_port(p: &Profile, work: &Path) -> Result<(), String> {
    port::run(p, work)?;
    let (tr, ev, route_of) = port::normalise(p)?;
    let masks = tape::read(&p.dir.join("port/input.pxtape"))?;
    let (off, bad) =
        port::tape_offset(&tr, &masks).ok_or("cannot align sim ticks with the tape")?;
    let first = tr
        .ticks
        .iter()
        .copied()
        .find(|t| {
            tr.get("port.mode", *t) == Some(1.0) && tr.get("hero.x", *t).is_some_and(|x| x != 0.0)
        })
        .ok_or("the port never reached gameplay (mode 1 with a hero)")?;
    let press = tr
        .ticks
        .iter()
        .copied()
        .find(|t| *t >= first && tr.get("input.pad", *t).is_some_and(|v| v != 0.0))
        .unwrap_or(first + 60);
    let mut t0 = p.start_tick.unwrap_or((press - 12).max(first + 5));
    if p.start_tick.is_none() {
        while t0 > first + 5 && !still(&tr, t0) {
            t0 -= 1;
        }
    }
    let last = *tr.ticks.last().unwrap();
    let n = p.ticks.min((last - t0) as usize);
    tr.save(&p.dir.join("trace-port.csv"))?;
    trace::save_events(&p.dir.join("events-port.csv"), &ev)?;
    let mut shots = vec![];
    if let Ok(rd) = fs::read_dir(p.dir.join("port/shots")) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if let Some(r) = name
                .strip_prefix("tick-")
                .and_then(|s| s.strip_suffix(".ppm"))
                .and_then(|s| s.parse::<u32>().ok())
            {
                let tick = route_of
                    .iter()
                    .filter(|(_, rt)| **rt <= r)
                    .map(|(t, _)| *t)
                    .max();
                if let Some(t) = tick {
                    if t >= t0 && t < t0 + n as i64 {
                        shots.push(json!({"tick": t, "route": r, "file": name}));
                    }
                }
            }
        }
    }
    shots.sort_by_key(|s| s["tick"].as_i64());
    shots.dedup_by_key(|s| s["tick"].as_i64());
    let start = (
        tr.get("hero.x", t0).unwrap_or(0.0),
        tr.get("hero.y", t0).unwrap_or(0.0),
        tr.get("hero.face", t0).unwrap_or(1.0),
    );
    let w = json!({"t0": t0, "ticks": n, "tape_offset": off, "tape_mismatches": bad, "first_gameplay_tick": first,
        "start": [start.0, start.1, start.2], "shots": shots});
    fs::write(
        p.dir.join("window.json"),
        serde_json::to_string_pretty(&w).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    eprintln!("[port] {} sim ticks traced; window t0={t0} n={n}; tape offset {off} ({bad} pad mismatches); start ({:.3},{:.3})", tr.len(), start.0, start.1);
    Ok(())
}

fn read_window(p: &Profile) -> Result<Value, String> {
    serde_json::from_str(
        &fs::read_to_string(p.dir.join("window.json"))
            .map_err(|_| "run `port` first (no window.json)".to_string())?,
    )
    .map_err(|e| e.to_string())
}

fn cmd_og(p: &Profile, work: &Path, shots: usize) -> Result<(), String> {
    let w = read_window(p)?;
    let (t0, n, off) = (
        w["t0"].as_i64().unwrap(),
        w["ticks"].as_u64().unwrap() as usize,
        w["tape_offset"].as_u64().unwrap() as usize,
    );
    let masks = tape::read(&p.dir.join("port/input.pxtape"))?;
    let mut unmapped = 0u16;
    let rows = tape::window_rows(&masks, off + t0 as usize, n, &mut unmapped);
    if unmapped != 0 {
        eprintln!("[og] warning: buttons {unmapped:#06x} have no binding in the original driver and were dropped");
    }
    let all = w["shots"].as_array().unwrap();
    let k = shots.min(all.len());
    let mut frames: Vec<usize> = if k == 0 {
        vec![]
    } else {
        (0..k)
            .map(|i| {
                (all[i * (all.len() - 1).max(1) / (k - 1).max(1)]["tick"]
                    .as_i64()
                    .unwrap()
                    - t0) as usize
            })
            .collect()
    };
    frames.sort();
    frames.dedup();
    let start = w["start"].as_array().unwrap();
    let st = (
        start[0].as_f64().unwrap(),
        start[1].as_f64().unwrap(),
        start[2].as_f64().unwrap(),
    );
    let env = og::Env::detect(work);
    let win = og::Window {
        frames: n,
        rows,
        start: Some(st),
        shot_frames: frames,
    };
    // The Unity player crashes natively now and then (a worker-thread heap fault,
    // seen once in about 30 runs); a run is deterministic, so retry.
    let mut attempt = 1;
    while let Err(e) = og::run(&env, p, &win) {
        if attempt >= 3 {
            return Err(e);
        }
        eprintln!("[og] attempt {attempt} failed ({e}); retrying");
        attempt += 1;
    }
    let port_tr = Trace::load(&p.dir.join("trace-port.csv"))?;
    let mut anchors = std::collections::BTreeMap::new();
    for e in &p.enemies {
        if let (Some(x), Some(y)) = (
            port_tr.get(&format!("{}.x", e.chan), t0),
            port_tr.get(&format!("{}.y", e.chan), t0),
        ) {
            anchors.insert(e.chan.clone(), (x, y));
        }
    }
    let (tr, ev) = og::normalise(p, t0, &anchors)?;
    tr.save(&p.dir.join("trace-og.csv"))?;
    trace::save_events(&p.dir.join("events-og.csv"), &ev)?;
    eprintln!("[og] {} frames traced", tr.len());
    Ok(())
}

/// Run the original under several RNG seeds and put the port's event times next to
/// the spread of the original's. Random AI choices (which attack, when) differ by
/// seed, so a single run cannot say "earlier" or "later" for those.
fn cmd_sweep(p: &Profile, work: &Path, seeds: &[u32]) -> Result<(), String> {
    let mut per_seed: Vec<(u32, Vec<trace::Event>)> = vec![];
    for sd in seeds {
        let mut q = p.clone();
        q.seed = *sd;
        cmd_og(&q, work, 0)?;
        let ev = trace::load_events(&p.dir.join("events-og.csv"))?;
        fs::copy(
            p.dir.join("events-og.csv"),
            p.dir.join(format!("events-og-seed{sd}.csv")),
        )
        .map_err(|e| e.to_string())?;
        per_seed.push((*sd, ev));
    }
    let pe = trace::load_events(&p.dir.join("events-port.csv"))?;
    let w = read_window(p)?;
    let (t0, n) = (w["t0"].as_i64().unwrap(), w["ticks"].as_i64().unwrap());
    let mut kinds: Vec<String> = pe
        .iter()
        .chain(per_seed.iter().flat_map(|(_, e)| e.iter()))
        .map(|e| e.kind.clone())
        .filter(|k| k != "sfx" && !k.ends_with(".fsm"))
        .collect();
    kinds.sort();
    kinds.dedup();
    let mut s = format!("# Seed sweep: {}\n\nSeeds {:?}. Ticks of the first four occurrences of each event kind inside the window ({}..{}); `-` means fewer occurrences.\n\n| kind | port | {} |\n|---|---|{}\n", p.name, seeds, t0, t0 + n,
        seeds.iter().map(|x| format!("original seed {x}")).collect::<Vec<_>>().join(" | "), "---|".repeat(seeds.len()));
    let first = |ev: &[trace::Event], k: &str| -> String {
        let v: Vec<String> = ev
            .iter()
            .filter(|e| e.kind == k && e.tick >= t0 && e.tick < t0 + n)
            .take(4)
            .map(|e| e.tick.to_string())
            .collect();
        if v.is_empty() {
            "-".into()
        } else {
            v.join(", ")
        }
    };
    for k in &kinds {
        s.push_str(&format!(
            "| {k} | {} | {} |\n",
            first(&pe, k),
            per_seed
                .iter()
                .map(|(_, e)| first(e, k))
                .collect::<Vec<_>>()
                .join(" | ")
        ));
    }
    fs::write(p.dir.join("sweep.md"), &s).map_err(|e| e.to_string())?;
    println!("{s}");
    Ok(())
}

fn cmd_diff(p: &Profile) -> Result<(), String> {
    let w = read_window(p)?;
    let (t0, n) = (
        w["t0"].as_i64().unwrap(),
        w["ticks"].as_u64().unwrap() as usize,
    );
    let (o, pt) = (
        Trace::load(&p.dir.join("trace-og.csv"))?,
        Trace::load(&p.dir.join("trace-port.csv"))?,
    );
    let (oe, pe) = (
        trace::load_events(&p.dir.join("events-og.csv"))?,
        trace::load_events(&p.dir.join("events-port.csv"))?,
    );
    let mut tol = std::collections::BTreeMap::new();
    if let Some(m) = p.raw.get("tolerance").and_then(Value::as_object) {
        for (k, v) in m {
            if let Some(f) = v.as_f64() {
                tol.insert(k.clone(), f);
            }
        }
    }
    let opt = diff::Opts { t0, n, tol };
    let note = format!("Port tape offset {} polls (sim tick t consumed poll t+{}); original driven from the same masks, teleported to the port's start state.", w["tape_offset"], w["tape_offset"]);
    let md = diff::report(&p.name, &o, &pt, &oe, &pe, &opt, &note);
    fs::write(p.dir.join("diff.md"), &md).map_err(|e| e.to_string())?;
    let pd = p.dir.join("plots");
    fs::create_dir_all(&pd).map_err(|e| e.to_string())?;
    for c in diff::channels(&o, &pt, &opt) {
        if c.name.ends_with(".step") || c.name.contains("face") {
            continue;
        }
        img::plot(&c.name, &c.o, &c.p, t0).save_png(&pd.join(format!("{}.png", c.name)))?;
    }
    println!("{md}");
    Ok(())
}

fn cmd_sheet(p: &Profile, approval: &Path) -> Result<(), String> {
    let w = read_window(p)?;
    let t0 = w["t0"].as_i64().unwrap();
    fs::create_dir_all(approval).map_err(|e| e.to_string())?;
    let mut n = 0;
    for s in w["shots"].as_array().unwrap() {
        let tick = s["tick"].as_i64().unwrap();
        let of = p.dir.join(format!("og/frames/f{:05}.png", tick - t0));
        if !of.exists() {
            continue;
        }
        let (a, b) = (
            img::Img::load_png(&of)?,
            img::Img::load_ppm(&p.dir.join("port/shots").join(s["file"].as_str().unwrap()))?,
        );
        let ph = 360usize;
        let (a, b) = (a.resize(a.w * ph / a.h, ph), b.resize(b.w * ph / b.h, ph));
        let mut sheet = img::Img::new(a.w + b.w + 12, ph + 24, [24, 24, 24]);
        sheet.blit(&a, 0, 24);
        sheet.blit(&b, (a.w + 12) as i64, 24);
        sheet.rect(0, 0, a.w as i64, 20, [30, 90, 220]);
        sheet.rect((a.w + 12) as i64, 0, b.w as i64, 20, [230, 120, 20]);
        sheet.text(6, 4, "ORIGINAL", 2, [255, 255, 255]);
        sheet.text((a.w + 18) as i64, 4, "PORT", 2, [255, 255, 255]);
        sheet.text(
            (a.w as i64) / 2 + 60,
            4,
            &format!("T{tick}"),
            2,
            [255, 255, 255],
        );
        sheet.save_png(&approval.join(format!("{}-t{:05}.png", p.name, tick)))?;
        n += 1;
    }
    eprintln!("[sheet] {n} side-by-side sheets in {}", approval.display());
    Ok(())
}

fn real_main() -> Result<(), String> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let work = work_dir(&mut args);
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match a.as_slice() {
        ["build"] => og::build(&og::Env::detect(&work)),
        ["tape", "gen", out, events, count] => tape::write(
            Path::new(out),
            &tape::from_events(events, count.parse().map_err(|_| "count")?)?,
        ),
        ["tape", "info", f] => {
            let m = tape::read(Path::new(f))?;
            println!("{} polls\n{}", m.len(), tape::describe(&m));
            Ok(())
        }
        ["port", pr] => cmd_port(&load(pr, &work)?, &work),
        ["og", pr] => cmd_og(&load(pr, &work)?, &work, 8),
        ["og", pr, n] => cmd_og(&load(pr, &work)?, &work, n.parse().map_err(|_| "og: shot count".to_string())?),
        ["sweep", pr, seeds] => {
            let list: Vec<u32> = seeds.split(',').filter_map(|x| x.parse().ok()).collect();
            cmd_sweep(&load(pr, &work)?, &work, &list)
        }
        ["imgstat", files @ ..] => {
            for f in files {
                let i = img::Img::load_png(Path::new(f))?;
                let mut s = [0u64; 3];
                for p in i.px.chunks(3) {
                    for k in 0..3 {
                        s[k] += p[k] as u64;
                    }
                }
                let n = (i.w * i.h) as f64;
                println!(
                    "{f} {:.1} {:.1} {:.1}",
                    s[0] as f64 / n,
                    s[1] as f64 / n,
                    s[2] as f64 / n
                );
            }
            Ok(())
        }
        ["montage", out, cols, files @ ..] => {
            let cols: usize = cols.parse().map_err(|_| "cols")?;
            let cw: usize = std::env::var("HKREF_MONTAGE_WIDTH")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(480);
            let imgs: Vec<img::Img> = files
                .iter()
                .map(|f| {
                    img::Img::load_png(Path::new(f)).map(|i| {
                        let h = i.h * cw / i.w;
                        i.resize(cw, h)
                    })
                })
                .collect::<Result<_, _>>()?;
            let (cw, ch) = (cw, imgs.iter().map(|i| i.h).max().unwrap_or(0) + 18);
            let rows = imgs.len().div_ceil(cols);
            let mut sheet = img::Img::new(cols * (cw + 4), rows * (ch + 4), [24, 24, 24]);
            for (k, (i, f)) in imgs.iter().zip(files.iter()).enumerate() {
                let (x, y) = ((k % cols) * (cw + 4), (k / cols) * (ch + 4));
                sheet.blit(i, x as i64, (y + 18) as i64);
                let name = Path::new(f)
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                sheet.text(
                    x as i64 + 2,
                    y as i64 + 3,
                    &name.to_uppercase(),
                    1,
                    [255, 255, 255],
                );
            }
            sheet.save_png(Path::new(out))
        }
        ["scenes", pr] => survey::run(&load(pr, &work)?, &work),
        ["scenes-summary", pr] => {
            let p = load(pr, &work)?;
            let mut dirs: Vec<_> = fs::read_dir(p.dir.join("og-survey"))
                .map_err(|e| e.to_string())?
                .flatten()
                .map(|e| e.path())
                .filter(|d| {
                    d.file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with('a'))
                })
                .collect();
            dirs.sort();
            survey::summarise(&p, &dirs)
        }
        ["diff", pr] => cmd_diff(&load(pr, &work)?),
        ["sheet", pr] => {
            let p = load(pr, &work)?;
            cmd_sheet(&p, &work.join("approval"))
        }
        ["all", pr] => {
            let p = load(pr, &work)?;
            cmd_port(&p, &work)?;
            cmd_og(&p, &work, 8)?;
            cmd_diff(&p)?;
            cmd_sheet(&p, &work.join("approval"))
        }
        _ => {
            eprintln!(
                "{}",
                include_str!("main.rs")
                    .lines()
                    .take_while(|l| l.starts_with("//!"))
                    .map(|l| l.trim_start_matches("//!"))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
            Err("bad arguments".into())
        }
    }
}

fn main() {
    if let Err(e) = real_main() {
        eprintln!("hkref: {e}");
        std::process::exit(1);
    }
}
