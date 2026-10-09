//! hkperf: the frame-pacing driver. Replays route tapes on side-built discs and reports
//! the 30 fps floor (gameplay frames over two vblanks, CPU or GPU bound), fps, final-state
//! equality between builds, and exact instruction, stall and per-function cycle profiles.
//!
//! The work directory W (`HKPERF_WORK`, default this crate's parent) holds a clone of this
//! repository as `W/hk-psx` and, per build:
//!   W/builds/<build>/{hk-psx.map,hk-psx-link.map,disc/hk-psx.cue}
//! made with `host/build_guest.py --no-prepass --work W/builds/<build>` and
//! `cargo hk-build disc --work W/builds/<build> --library W/builds/<build>/disc`.
//! Runs land in W/runs/<tag>-<route>/{command.json,route.csv,gpu.csv,summary.json,...}.
//! Tapes for routes outside tools/tapes go in W/tapes-ext. Frontend: `HKPERF_FRONTEND`, else
//! the emulator pinned in emulator.lock.json under `.hkpsx/emulator`.
//!
//! usage:
//!   hkperf sweep <build> [--tag T] [--jobs N] [--routes a,b] [run options]
//!   hkperf run <build> <route> [--tag T] [run options]
//!     run options: --shots N  --gpu  --prof  --windows N  --cycles  --stalls  --stop POLL  --audio
//!   hkperf budget <run>... [--list]        frames over two vblanks of a run, by cause
//!   hkperf table <tag> [<old tag>] [--routes a,b]
//!   hkperf state <tag a> <tag b> [--routes a,b]     final-state equality between two sweeps
//!   hkperf prof <run> <build> [--top N]    instructions per function (--prof run)
//!   hkperf stalls <run> <build> [--top N]  I-cache and main-RAM load stall cycles per function (--stalls run)
//!   hkperf cost <run> <build> [--top N]    issue plus stall cycles per function (--prof --stalls run)
//!   hkperf obframes <run> <build> [--top N]  functions in the over-budget frames (--windows 1 run)
//!   hkperf ticks <run> <build> FROM TO     functions per route tick (--windows 1 run)
//!   hkperf shotcmp <run a> <run b>
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

type Res<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn work() -> PathBuf {
    std::env::var_os("HKPERF_WORK")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".."))
        .canonicalize()
        .unwrap()
}
fn repo() -> PathBuf {
    work().join("hk-psx")
}

fn frontend() -> PathBuf {
    if let Some(f) = std::env::var_os("HKPERF_FRONTEND") {
        return PathBuf::from(f);
    }
    let pin: Value = serde_json::from_slice(&std::fs::read(repo().join("emulator.lock.json")).unwrap()).unwrap();
    let rev = pin["revision"].as_str().unwrap();
    repo().join(".hkpsx/emulator").join(rev).join("target/release/frontend")
}

/// Symbols with a plain name from an LLD map (VMA LMA Size Align Name).
fn symbols(map: &Path) -> HashMap<String, u32> {
    let text = std::fs::read_to_string(map).unwrap_or_default();
    let mut out = HashMap::new();
    for line in text.lines() {
        let s: Vec<&str> = line.split_whitespace().collect();
        if s.len() == 5 {
            if let Ok(a) = u32::from_str_radix(s[0], 16) {
                out.insert(s[4].to_string(), a);
            }
        }
    }
    out
}

/// Function ranges from the link map: (start, size, demangled name).
fn functions(link_map: &Path) -> Vec<(u32, u32, String)> {
    let text = std::fs::read_to_string(link_map).unwrap_or_default();
    let mut fs = Vec::new();
    for line in text.lines() {
        let s: Vec<&str> = line.split_whitespace().collect();
        if s.len() >= 5 && s[0].len() == 8 && s[3] == "1" && !s[4].starts_with('/') && !s[4].starts_with('.') {
            if let (Ok(a), Ok(n)) = (u32::from_str_radix(s[0], 16), u32::from_str_radix(s[2], 16)) {
                fs.push((a, n, s[4..].join(" ")));
            }
        }
    }
    fs.sort();
    fs
}

fn identifiers(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in text.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_alphanumeric() || c == '_' {
            cur.push(c);
        } else {
            if (cur.starts_with("HK_") || cur.starts_with("__psx_rt")) && !out.contains(&cur) {
                out.push(cur.clone());
            }
            cur.clear();
        }
    }
    out
}

/// The validator's watch list (tools/replay_cue.py WATCHES) plus the frame-timing words.
fn watch_names() -> Vec<String> {
    let text = std::fs::read_to_string(repo().join("tools/replay_cue.py")).unwrap();
    let start = text.find("WATCHES='''").unwrap();
    let end = text.find("def digest").unwrap();
    let mut names = identifiers(&text[start..end]);
    for extra in "HK_SCENE_SFX_MISSED HK_SCENE_SFX_LOADS HK_SCENE_SFX_SCENE HK_MODULE_RESIDENT HK_MODULE_INSTALLS \
        HK_MODULE_BG_INSTALLS HK_MODULE_GATE_INSTALLS HK_MODULE_EVICTIONS HK_MODULE_GATE_HITS HK_MODULE_GATE_MISSES \
        HK_MODULE_MISSING HK_MODULE_BG_ABORTS HK_MODULE_BG_NOROOM HK_MODULE_POOL_USED HK_MODULE_POOL_PEAK \
        HK_MODULE_GATE_HBLANKS HK_MODULE_GATE_WAIT_HBLANKS HK_MODULE_BG_FRAMES HK_MODULE_TRAPS HK_PREFETCH_HITS \
        HK_PREFETCH_READS HK_PREFETCH_WASTED HK_SIM_TICKS HK_SIM_PAD HK_PLAYER_FACING HK_CAMERA_LOCK HK_CAMERA_VIEW_MISS \
        HK_GLOW_QUADS HK_GLOW_SKIPPED HK_FRAME_FIRST_KICK_LINES HK_FRAME_LAST_KICK_LINES HK_FRAME_FLIP_LINES \
        HK_EARLY_BACK_FRAMES HK_TILE_HIDDEN_QUADS HK_STALACTITES_FALLEN HK_STALACTITE_ROCKS HK_STALACTITE_ROCKS_DROPPED HK_STALACTITE_EMBEDDED_BREAKS HK_GOAMS_UP HK_PROPS_DRAWN"
        .split_whitespace()
    {
        if !names.iter().any(|n| n == extra) {
            names.push(extra.to_string());
        }
    }
    names
}

fn watches(build: &str) -> BTreeMap<String, u32> {
    let syms = symbols(&work().join("builds").join(build).join("hk-psx.map"));
    let mut out = BTreeMap::new();
    for n in watch_names() {
        if let Some(a) = syms.get(&n) {
            out.insert(n, *a);
        }
    }
    for (array, n) in [("HK_SCENE_SFX", 36), ("HK_HERO_EXTRA_SFX", 7), ("HK_ABILITY_SFX", 7), ("HK_SFX_EVENT_COUNTS", 8)] {
        if let Some(a) = syms.get(array) {
            for i in 0..n {
                out.insert(format!("{array}[{i}]"), a + 4 * i);
            }
        }
    }
    out
}

fn main_rs() -> String {
    std::fs::read_to_string(repo().join("host/hk-build/main.rs")).unwrap()
}

/// Every quoted lowercase name in `("name", ...)` tuples between two markers.
fn tuples(text: &str, from: &str, to: &str) -> Vec<Vec<String>> {
    let a = text.find(from).unwrap();
    let b = a + text[a..].find(to).unwrap();
    let body = &text[a..b];
    let mut out = Vec::new();
    for part in body.split('(').skip(1) {
        let inner = part.split(')').next().unwrap_or("");
        let fields: Vec<String> = inner.split(',').map(|f| f.trim().trim_matches('"').to_string()).collect();
        out.push(fields);
    }
    out
}

fn routes() -> Vec<String> {
    let mut r: Vec<String> = tuples(&main_rs(), "const ROUTES", "];").into_iter().map(|f| f[0].clone()).filter(|n| !n.starts_with('&')).collect();
    for extra in ["f01-moss", "f17-charger"] {
        r.push(extra.into());
    }
    r
}

fn tape_and_card(route: &str) -> (PathBuf, Option<PathBuf>) {
    let ext = work().join("tapes-ext").join(format!("{route}.pxtape"));
    if ext.is_file() {
        let card = work().join("tapes-ext").join(format!("{route}.mcd"));
        return (ext, card.is_file().then_some(card));
    }
    let cards = tuples(&main_rs(), "const MEMCARDS", "const JOURNEY");
    let card = cards.iter().find(|f| f[0] == route).map(|f| repo().join("tools/cards").join(&f[1]));
    (repo().join("tools/tapes").join(format!("{route}.pxtape")), card)
}

#[derive(Clone, Default)]
struct RunOpts {
    shots: Option<u32>,
    gpu: bool,
    prof: bool,
    windows: Option<u32>,
    cycles: bool,
    stalls: bool,
    stop: Option<u32>,
    audio: bool,
    callsites: bool,
    owners: bool,
}

fn col(watch: &Value, name: &str) -> Option<String> {
    watch[name].as_str().map(|h| format!("ram_{:0>8}", h.trim_start_matches("0x")))
}

struct Csv {
    header: HashMap<String, usize>,
    rows: Vec<Vec<String>>,
}
impl Csv {
    fn read(path: &Path) -> Res<Csv> {
        let text = std::fs::read_to_string(path)?;
        let mut lines = text.lines();
        let header = lines.next().ok_or("empty csv")?.split(',').enumerate().map(|(i, h)| (h.to_string(), i)).collect();
        let rows = lines.map(|l| l.split(',').map(str::to_string).collect()).collect();
        Ok(Csv { header, rows })
    }
    fn get<'a>(&self, row: &'a [String], name: &str) -> &'a str {
        self.header.get(name).and_then(|&i| row.get(i)).map(|s| s.as_str()).unwrap_or("")
    }
    fn num(&self, row: &[String], name: &str) -> i64 {
        self.get(row, name).parse().unwrap_or(0)
    }
}

/// The most common value of a column.
fn mode(csv: &Csv, name: &str) -> String {
    let mut c: HashMap<&str, usize> = HashMap::new();
    for r in &csv.rows {
        *c.entry(csv.get(r, name)).or_default() += 1;
    }
    c.into_iter().max_by_key(|(_, n)| *n).map(|(k, _)| k.to_string()).unwrap_or_default()
}

/// First route tick in gameplay (most common game mode and room load state).
fn first_gameplay_tick(run: &Path) -> Option<i64> {
    let cmd: Value = serde_json::from_slice(&std::fs::read(run.join("command.json")).ok()?).ok()?;
    let csv = Csv::read(&run.join("route.csv")).ok()?;
    let (gm, ls) = (col(&cmd["watch"], "HK_GAME_MODE")?, col(&cmd["watch"], "HK_ROOM_LOAD_STATE")?);
    let (play, ready) = (mode(&csv, &gm), mode(&csv, &ls));
    csv.rows.iter().find(|r| csv.get(r, &gm) == play && csv.get(r, &ls) == ready).map(|r| csv.num(r, "route_tick"))
}

fn run_one(build: &str, route: &str, tag: &str, o: &RunOpts) -> Res<i32> {
    let w = work();
    let out = w.join("runs").join(format!("{tag}-{route}"));
    if out.join("summary.json").is_file() {
        return Ok(0);
    }
    if out.exists() {
        std::fs::remove_dir_all(&out)?;
    }
    std::fs::create_dir_all(&out)?;
    let (tape, card) = tape_and_card(route);
    let data = std::fs::read(&tape)?;
    let count = u32::from_le_bytes(data[8..12].try_into()?);
    let start = u32::from_le_bytes(data[12..16].try_into()?);
    let watch = watches(build);
    let cue = w.join("builds").join(build).join("disc/hk-psx.cue");
    let mut cmd: Vec<String> = vec![frontend().display().to_string(), "launch".into(), "--path".into(), cue.display().to_string(), "--embedded-playtest".into()];
    let p = |n: &str| out.join(n).display().to_string();
    let stop = o.stop.unwrap_or(start + count);
    cmd.extend(["--config-dir".into(), p("emulator"), "--steps".into(), "6000000000".into(), "--input-tape".into(), tape.display().to_string(),
        "--stop-at-poll".into(), stop.to_string(), "--route-log".into(), p("route.csv"), "--dump-ram".into(), p("ram.bin"),
        "--dump-display".into(), p("display.ppm")]);
    if o.audio {
        cmd.extend(["--dump-audio".into(), p("audio.wav")]);
    }
    if let Some(n) = o.shots {
        cmd.extend(["--route-screenshot-dir".into(), p("shots"), "--route-screenshot-interval".into(), n.to_string()]);
    }
    if o.gpu {
        cmd.extend(["--gpu-frame-stats-log".into(), p("gpu.csv")]);
    }
    let first = if o.prof || o.stalls {
        first_gameplay_tick(&w.join("runs").join(format!("{build}-{route}"))).unwrap_or(0)
    } else {
        0
    };
    if o.prof {
        cmd.extend(["--pc-line-log".into(), p("lines.csv"), "--pc-line-start-route-tick".into(), first.to_string()]);
    }
    if let Some(n) = o.windows {
        cmd.extend(["--pc-sample-window-log".into(), p("windows.csv"), "--pc-sample-window-ticks".into(), n.to_string(), "--pc-sample-instructions".into(), "61".into()]);
    }
    if o.owners {
        cmd[0] = w.join("emu-owners/target/release/frontend").display().to_string();
        cmd.extend(["--dump-draws".into(), p("draws.csv"), "--dump-hw".into(), p("hw.ppm")]);
    }
    if o.callsites {
        cmd.extend(["--pc-sample-callsite-log".into(), p("callsites.csv"), "--pc-sample-instructions".into(), "97".into()]);
    }
    if o.cycles {
        cmd.extend(["--cpu-cycle-profile-log".into(), p("cycles.csv")]);
    }
    if o.stalls {
        cmd.extend(["--icache-stall-line-log".into(), p("icache.csv"), "--icache-stall-line-start-route-tick".into(), first.to_string(),
            "--ram-load-stall-line-log".into(), p("ramload.csv"), "--ram-load-stall-line-start-route-tick".into(), first.to_string()]);
    }
    for a in watch.values() {
        cmd.extend(["--route-watch-u32".into(), format!("{a:#x}")]);
    }
    if let Some(card) = card {
        let name = card.file_name().unwrap();
        if card.is_file() {
            std::fs::copy(&card, out.join(name))?;
        }
        cmd.extend(["--memcard".into(), out.join(name).display().to_string()]);
    }
    let wjson: serde_json::Map<String, Value> = watch.iter().map(|(k, v)| (k.clone(), json!(format!("{v:#x}")))).collect();
    std::fs::write(out.join("command.json"), serde_json::to_string_pretty(&json!({"command": cmd, "watch": wjson, "first_gameplay_tick": first}))?)?;
    let log = std::fs::File::create(out.join("replay.log"))?;
    let mut child = Command::new(&cmd[0]).args(&cmd[1..]).env("HK_PIXEL_OWNERS", if o.owners { out.display().to_string() } else { String::new() }).stdout(log.try_clone()?).stderr(log).spawn()?;
    {
        use std::io::Write;
        let mut pids = std::fs::OpenOptions::new().append(true).create(true).open(w.join("PIDS"))?;
        writeln!(pids, "{} frontend hkperf {tag}-{route}", child.id())?;
    }
    let status = child.wait()?;
    let ram = std::fs::read(out.join("ram.bin")).unwrap_or_default();
    let mut fin = serde_json::Map::new();
    for (n, a) in &watch {
        let off = (*a & 0x1f_ffff) as usize;
        if off + 4 <= ram.len() {
            fin.insert(n.clone(), json!(u32::from_le_bytes(ram[off..off + 4].try_into()?)));
        }
    }
    let logtext = std::fs::read_to_string(out.join("replay.log")).unwrap_or_default();
    let stopline: Vec<&str> = logtext.lines().filter(|l| l.contains("port1-polls")).collect();
    // Screenshots are reduced to a hash list (shots.txt) unless HKPERF_KEEP_SHOTS is set.
    let shots = out.join("shots");
    if shots.is_dir() && std::env::var_os("HKPERF_KEEP_SHOTS").is_none() {
        let mut names: Vec<_> = std::fs::read_dir(&shots)?.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        names.sort();
        let mut list = String::new();
        for p in &names {
            let mut h: u64 = 0xcbf29ce484222325;
            for b in std::fs::read(p)? { h = (h ^ b as u64).wrapping_mul(0x100000001b3); }
            list.push_str(&format!("{} {h:016x}\n", p.file_name().unwrap().to_string_lossy()));
            std::fs::remove_file(p)?;
        }
        std::fs::write(out.join("shots.txt"), list)?;
        std::fs::remove_dir(&shots)?;
    }
    let code = status.code().unwrap_or(-1);
    std::fs::write(out.join("summary.json"), serde_json::to_string_pretty(&json!({
        "exit": code, "final": fin, "stop": stopline.last().copied().unwrap_or(""), "polls_expected": stop}))?)?;
    if std::env::var_os("HKPERF_KEEP_RAM").is_none() && !o.owners {
        let _ = std::fs::remove_file(out.join("ram.bin"));
    }
    Ok(code)
}

struct Budget {
    frames: usize,
    over: Vec<(i64, usize, &'static str)>,
    gpu: Vec<f64>,
    lines: Vec<(i64, i64, i64)>,
    bind_frames: usize,
    saves: usize,
    ticks: usize,
    flips: usize,
}

fn budget(run: &Path) -> Res<Budget> {
    let cmd: Value = serde_json::from_slice(&std::fs::read(run.join("command.json"))?)?;
    let w = &cmd["watch"];
    let csv = Csv::read(&run.join("route.csv"))?;
    let (gm, ls) = (col(w, "HK_GAME_MODE").ok_or("no mode")?, col(w, "HK_ROOM_LOAD_STATE").ok_or("no load")?);
    let (play, ready) = (mode(&csv, &gm), mode(&csv, &ls));
    let ok: Vec<bool> = csv.rows.iter().map(|r| csv.get(r, &gm) == play && csv.get(r, &ls) == ready).collect();
    let flips: Vec<usize> = (0..csv.rows.len()).filter(|&i| csv.get(&csv.rows[i], "display_start_changed") == "1").collect();
    let blocked = col(w, "HK_INPUT_BLOCKED_VBLANKS");
    let (fl, lk) = (col(w, "HK_FRAME_FLIP_LINES"), col(w, "HK_FRAME_LAST_KICK_LINES"));
    let mut b = Budget { frames: 0, over: vec![], gpu: vec![], lines: vec![], bind_frames: 0, saves: 0, ticks: 0, flips: 0 };
    let gpucyc: Option<HashMap<i64, i64>> = Csv::read(&run.join("gpu.csv")).ok().map(|g| g.rows.iter().map(|r| (g.num(r, "route_tick"), g.num(r, "gpu_cycles"))).collect());
    for (i, r) in csv.rows.iter().enumerate() {
        if ok[i] {
            b.ticks += 1;
            if csv.get(r, "display_start_changed") == "1" {
                b.flips += 1;
            }
        }
    }
    for pair in flips.windows(2) {
        let (a, z) = (pair[0], pair[1]);
        if !(a..=z).all(|k| ok[k]) {
            continue;
        }
        b.frames += 1;
        if z - a <= 2 {
            continue;
        }
        if let Some(bl) = &blocked {
            if csv.num(&csv.rows[z], bl) > csv.num(&csv.rows[a], bl) {
                b.saves += 1;
                continue;
            }
        }
        let fk = col(w, "HK_FRAME_FIRST_KICK_LINES");
        let (ta, tz) = (csv.num(&csv.rows[a], "route_tick"), csv.num(&csv.rows[z], "route_tick"));
        let gsum: Option<i64> = gpucyc.as_ref().map(|m| (ta + 1..=tz).map(|t| m.get(&t).copied().unwrap_or(0)).sum());
        b.gpu.push(gsum.map(|g| g as f64 / 565045.0).unwrap_or(-1.0));
        let binds = col(w, "HK_VIEW_BINDS");
        if let Some(bc) = &binds { if csv.num(&csv.rows[z], bc) > csv.num(&csv.rows[a], bc) { b.bind_frames += 1; } }
        let cause = match (&fl, &lk) {
            (Some(f), _) if ((z - a - 1) as i64) * 263 - csv.num(&csv.rows[z], f) > 60 => "SIM",
            (Some(f), _) if gsum.is_some() && fk.is_some() => {
                let span = (csv.num(&csv.rows[z], f) - csv.num(&csv.rows[z], fk.as_ref().unwrap())).max(1) as f64;
                if gsum.unwrap() as f64 / 2148.0 > 0.85 * span && csv.num(&csv.rows[z], fk.as_ref().unwrap()) < 80 { "GPU" } else { "CPU" }
            }
            (Some(f), Some(l)) if csv.num(&csv.rows[z], f) - csv.num(&csv.rows[z], l) > 30 => "GPU",
            (Some(_), Some(_)) => "CPU",
            _ => "?",
        };
        let g = |c: &Option<String>| c.as_ref().map(|c| csv.num(&csv.rows[z], c)).unwrap_or(-1);
        b.lines.push((g(&fk), g(&lk), g(&fl)));
        b.over.push((csv.num(&csv.rows[z], "route_tick"), z - a, cause));
    }
    Ok(b)
}

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}
fn positional(args: &[String]) -> Vec<String> {
    let valued = ["--tag", "--jobs", "--routes", "--shots", "--windows", "--stop", "--top"];
    let mut out = vec![];
    let mut i = 0;
    while i < args.len() {
        if valued.contains(&args[i].as_str()) {
            i += 2;
            continue;
        }
        if !args[i].starts_with("--") {
            out.push(args[i].clone());
        }
        i += 1;
    }
    out
}
fn run_opts(args: &[String]) -> RunOpts {
    let has = |n: &str| args.iter().any(|a| a == n);
    let num = |n: &str| arg_value(args, n).map(|v| v.parse().unwrap());
    RunOpts { shots: num("--shots"), gpu: has("--gpu"), prof: has("--prof"), windows: num("--windows"), cycles: has("--cycles"),
        stalls: has("--stalls"), stop: num("--stop"), audio: has("--audio"), callsites: has("--callsites"), owners: has("--owners") }
}
fn route_list(args: &[String]) -> Vec<String> {
    arg_value(args, "--routes").map(|r| r.split(',').map(str::to_string).collect()).unwrap_or_else(routes)
}

fn faults(fin: &Value) -> Vec<String> {
    ["__psx_rt_fault_count", "HK_INPUT_FAULT", "HK_FLIP_TIMEOUTS", "HK_AUDIO_STREAM_UNDERRUNS", "HK_SCENE_SFX_MISSED", "HK_ROOM_LOAD_ERROR",
        "HK_CD_STREAM_ERROR", "HK_MODULE_MISSING", "HK_MODULE_TRAPS", "HK_INPUT_SKIPPED_TICKS", "HK_INPUT_DROPPED_SAMPLES", "HK_INPUT_MISSED_VBLANKS",
        "HK_SCENERY_REPAIR_FAILURES", "HK_MUSIC_ERROR"]
        .iter()
        .filter(|k| fin[**k].as_u64().unwrap_or(0) != 0)
        .map(|k| format!("{k}={}", fin[*k]))
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let pos = positional(&args);
    let w = work();
    let action = pos.first().cloned().unwrap_or_default();
    match action.as_str() {
        "run" => {
            let tag = arg_value(&args, "--tag").unwrap_or_else(|| pos[1].clone());
            println!("{:?}", run_one(&pos[1], &pos[2], &tag, &run_opts(&args)));
        }
        "sweep" => {
            let build = pos[1].clone();
            let tag = arg_value(&args, "--tag").unwrap_or_else(|| build.clone());
            let jobs: usize = arg_value(&args, "--jobs").map(|v| v.parse().unwrap()).unwrap_or(12);
            let queue = Arc::new(Mutex::new(route_list(&args)));
            let opts = run_opts(&args);
            let handles: Vec<_> = (0..jobs)
                .map(|_| {
                    let (q, b, t, o) = (queue.clone(), build.clone(), tag.clone(), opts.clone());
                    std::thread::spawn(move || loop {
                        let r = { q.lock().unwrap().pop() };
                        let Some(r) = r else { break };
                        let res = run_one(&b, &r, &t, &o);
                        println!("{t}-{r}: {res:?}");
                    })
                })
                .collect();
            for h in handles {
                h.join().unwrap();
            }
        }
        "budget" => {
            for run in &pos[1..] {
                let d = w.join("runs").join(run);
                match budget(&d) {
                    Ok(b) => {
                        let cpu = b.over.iter().filter(|o| o.2 == "CPU").count();
                        let gpu = b.over.iter().filter(|o| o.2 == "GPU").count();
                        let sim = b.over.iter().filter(|o| o.2 == "SIM").count();
                        println!("{run:36} frames {:5} over2 {:5} worst {} SIM {sim} CPU {cpu} GPU {gpu} fps {:.2} saves {} binds {}", b.frames, b.over.len(),
                            b.over.iter().map(|o| o.1).max().unwrap_or(2), b.flips as f64 / b.ticks.max(1) as f64 * 59.94, b.saves, b.bind_frames);
                        if args.iter().any(|a| a == "--list") {
                            for (k, o) in b.over.iter().enumerate() {
                                println!("   tick {} vblanks {} {} gpu {:.2} vbl  first/last kick, flip lines {:?}", o.0, o.1, o.2, b.gpu[k], b.lines[k]);
                            }
                        }
                    }
                    Err(e) => println!("{run}: {e}"),
                }
            }
        }
        "table" => {
            let new = &pos[1];
            let old = pos.get(2);
            let mut rows = vec![];
            let (mut tn, mut to, mut fn_, mut fo, mut nr) = (0usize, 0usize, 0f64, 0f64, 0usize);
            for r in route_list(&args) {
                let n = budget(&w.join("runs").join(format!("{new}-{r}"))).ok();
                let o = old.and_then(|o| budget(&w.join("runs").join(format!("{o}-{r}"))).ok());
                rows.push((r, n, o));
            }
            rows.sort_by_key(|(r, n, _)| (std::cmp::Reverse(n.as_ref().map(|b| b.over.len()).unwrap_or(0)), r.clone()));
            println!("| route | frames | over 2 vbl | worst | SIM | CPU | GPU | fps |{}", if let Some(o) = old { format!(" over ({o}) | fps ({o}) |") } else { String::new() });
            println!("|---|---|---|---|---|---|---|---|{}", if old.is_some() { "---|---|" } else { "" });
            for (r, n, o) in &rows {
                let Some(n) = n else {
                    println!("| {r} | missing |");
                    continue;
                };
                let fps = |b: &Budget| b.flips as f64 / b.ticks.max(1) as f64 * 59.94;
                let cnt = |c: &str| n.over.iter().filter(|x| x.2 == c).count();
                print!("| {r} | {} | {} | {} | {} | {} | {} | {:.2} |", n.frames, n.over.len(), n.over.iter().map(|x| x.1).max().unwrap_or(2),
                    cnt("SIM"), cnt("CPU"), cnt("GPU"), fps(n));
                tn += n.over.len();
                if let Some(o) = o {
                    print!(" {} | {:.2} |", o.over.len(), fps(o));
                    to += o.over.len();
                    fn_ += fps(n);
                    fo += fps(o);
                    nr += 1;
                } else if old.is_some() {
                    print!(" - | - |");
                }
                println!();
            }
            println!("\nover-budget total {tn}{}; routes with any: {} of {}", if old.is_some() { format!(" (old {to}); mean fps over {nr} common routes {:.3} vs {:.3}", fn_ / nr.max(1) as f64, fo / nr.max(1) as f64) } else { String::new() },
                rows.iter().filter(|x| x.1.as_ref().is_some_and(|b| !b.over.is_empty())).count(), rows.len());
        }
        "state" => {
            let (a, b) = (&pos[1], &pos[2]);
            let ignore = |k: &str| {
                k == "HK_SCENE_SFX[10]" || k == "HK_SCENE_SFX[11]" || ["HK_HERO_EXTRA_SFX", "HK_ABILITY_SFX", "HK_SFX_EVENT_COUNTS", "HK_OVERLAY_", "HK_MODULE_", "HK_PREFETCH_", "HK_FRAME_", "HK_EARLY_BACK", "HK_TILE_HIDDEN"].iter().any(|p| k.starts_with(p))
            };
            let mut same = 0;
            for r in route_list(&args) {
                let load = |t: &str| -> Option<Value> { serde_json::from_slice(&std::fs::read(w.join("runs").join(format!("{t}-{r}")).join("summary.json")).ok()?).ok() };
                let (Some(x), Some(y)) = (load(a), load(b)) else {
                    println!("{r}: missing run");
                    continue;
                };
                let diff: Vec<String> = x["final"].as_object().unwrap().iter()
                    .filter(|(k, v)| !ignore(k) && y["final"].get(k.as_str()) != Some(v))
                    .map(|(k, v)| format!("{k} {v}->{}", y["final"][k.as_str()]))
                    .collect();
                let f = faults(&y["final"]);
                let stop_eq = x["stop"] == y["stop"];
                if diff.is_empty() && f.is_empty() && stop_eq && y["exit"] == 0 {
                    same += 1;
                } else {
                    println!("{r}: exit {} stop_equal {stop_eq} faults {f:?} diff {diff:?}", y["exit"]);
                }
            }
            println!("{same} routes identical final state, no faults");
        }
        "prof" | "obframes" => {
            let run = w.join("runs").join(&pos[1]);
            let fs = functions(&w.join("builds").join(&pos[2]).join("hk-psx-link.map"));
            let top: usize = arg_value(&args, "--top").map(|v| v.parse().unwrap()).unwrap_or(40);
            let starts: Vec<u32> = fs.iter().map(|f| f.0).collect();
            let name = |pc: u32| -> String {
                let j = starts.partition_point(|&s| s <= pc);
                if j > 0 && pc < fs[j - 1].0 + fs[j - 1].1.max(16) { fs[j - 1].2.clone() } else { "?".into() }
            };
            if action == "prof" {
                let csv = Csv::read(&run.join("lines.csv")).unwrap();
                let mut agg: HashMap<String, u64> = HashMap::new();
                for r in &csv.rows {
                    let pc = u32::from_str_radix(r[0].trim_start_matches("0x"), 16).unwrap_or(0);
                    *agg.entry(name(pc)).or_default() += r[1].parse::<u64>().unwrap_or(0);
                }
                let total: u64 = agg.values().sum();
                let mut v: Vec<_> = agg.into_iter().collect();
                v.sort_by_key(|x| std::cmp::Reverse(x.1));
                println!("gameplay instructions {total} header {:?}", csv.header.keys().collect::<Vec<_>>());
                for (k, n) in v.iter().take(top) {
                    println!("{n:11} {:5.1}% {}", 100.0 * *n as f64 / total as f64, &k[..k.len().min(110)]);
                }
            } else {
                let b = budget(&run).unwrap();
                let csv = Csv::read(&run.join("windows.csv")).unwrap();
                let wcol = "window_start_tick".to_string();
                let mut win: HashMap<i64, HashMap<String, u64>> = HashMap::new();
                for r in &csv.rows {
                    let t = csv.num(r, &wcol);
                    let pc = u32::from_str_radix(csv.get(r, "pc").trim_start_matches("0x"), 16).unwrap_or(0);
                    *win.entry(t).or_default().entry(name(pc)).or_default() += csv.num(r, "samples") as u64;
                }
                let mut total: HashMap<String, u64> = HashMap::new();
                for (tick, vbl, cause) in &b.over {
                    let mut c: HashMap<String, u64> = HashMap::new();
                    for t in (tick - *vbl as i64)..=*tick {
                        for (k, v) in win.get(&t).into_iter().flatten() {
                            *c.entry(k.clone()).or_default() += v;
                        }
                    }
                    let tot: u64 = c.values().sum::<u64>().max(1);
                    let mut v: Vec<_> = c.iter().collect();
                    v.sort_by_key(|x| std::cmp::Reverse(*x.1));
                    println!("tick {tick} {vbl} vbl {cause}: {}", v.iter().take(6).map(|(k, n)| format!("{}% {}", **n * 100 / tot, k.rsplit("::").next().unwrap_or(k))).collect::<Vec<_>>().join(", "));
                    {
                        let _ = cause;
                        for (k, n) in c {
                            *total.entry(k).or_default() += n;
                        }
                    }
                }
                let tot: u64 = total.values().sum::<u64>().max(1);
                let mut v: Vec<_> = total.into_iter().collect();
                v.sort_by_key(|x| std::cmp::Reverse(x.1));
                println!("\nall over-budget frames, top functions:");
                for (k, n) in v.iter().take(top) {
                    println!("{:5.1}% {}", 100.0 * *n as f64 / tot as f64, &k[..k.len().min(110)]);
                }
            }
        }
        "stalls" => {
            // stalls RUN BUILD [--top N]: the run's I-cache refill and main-RAM load stall cycles
            // (--stalls runs) summed per function of the build's link map.
            let run = w.join("runs").join(&pos[1]);
            let fs = functions(&w.join("builds").join(&pos[2]).join("hk-psx-link.map"));
            let starts: Vec<u32> = fs.iter().map(|f| f.0).collect();
            let name = |pc: u32| -> String {
                let j = starts.partition_point(|&s| s <= pc);
                if j > 0 && pc < fs[j - 1].0 + fs[j - 1].1.max(16) { fs[j - 1].2.clone() } else { format!("{pc:#x}") }
            };
            let top: usize = arg_value(&args, "--top").map(|v| v.parse().unwrap()).unwrap_or(30);
            for (file, label) in [("icache.csv", "I-cache refill"), ("ramload.csv", "main-RAM load")] {
                let Ok(csv) = Csv::read(&run.join(file)) else { continue };
                let mut agg: HashMap<String, u64> = HashMap::new();
                for r in &csv.rows {
                    let pc = u32::from_str_radix(r[0].trim_start_matches("0x"), 16).unwrap_or(0);
                    *agg.entry(name(pc)).or_default() += r[1].parse::<u64>().unwrap_or(0);
                }
                let total: u64 = agg.values().sum();
                let mut v: Vec<_> = agg.into_iter().collect();
                v.sort_by_key(|x| std::cmp::Reverse(x.1));
                println!("{label} stall cycles {total}");
                for (k, n) in v.iter().take(top) {
                    println!("{n:11} {:5.1}% {}", 100.0 * *n as f64 / total.max(1) as f64, &k[..k.len().min(110)]);
                }
            }
        }
        "cost" => {
            // cost RUN BUILD [--top N]: per function, instructions (issue cycles) plus I-cache refill and
            // main-RAM load stall cycles, from a run made with --prof --stalls.
            let run = w.join("runs").join(&pos[1]);
            let fs = functions(&w.join("builds").join(&pos[2]).join("hk-psx-link.map"));
            let starts: Vec<u32> = fs.iter().map(|f| f.0).collect();
            let name = |pc: u32| -> String {
                let j = starts.partition_point(|&s| s <= pc);
                if j > 0 && pc < fs[j - 1].0 + fs[j - 1].1.max(16) { fs[j - 1].2.clone() } else { format!("{pc:#x}") }
            };
            let top: usize = arg_value(&args, "--top").map(|v| v.parse().unwrap()).unwrap_or(40);
            let mut agg: HashMap<String, [u64; 3]> = HashMap::new();
            for (k, file) in ["lines.csv", "icache.csv", "ramload.csv"].iter().enumerate() {
                let Ok(csv) = Csv::read(&run.join(file)) else { continue };
                for r in &csv.rows {
                    let pc = u32::from_str_radix(r[0].trim_start_matches("0x"), 16).unwrap_or(0);
                    agg.entry(name(pc)).or_default()[k] += r[1].parse::<u64>().unwrap_or(0);
                }
            }
            let total: u64 = agg.values().map(|v| v[0] + v[1] + v[2]).sum();
            let mut v: Vec<_> = agg.into_iter().collect();
            v.sort_by_key(|x| std::cmp::Reverse(x.1[0] + x.1[1] + x.1[2]));
            println!("total cycles (issue+icache+ramload) {total}");
            println!("   cycles    share   issue  icache  ramload  function");
            for (k, c) in v.iter().take(top) {
                let t = c[0] + c[1] + c[2];
                println!("{t:11} {:5.1}% {:6.1}M {:6.1}M {:6.1}M  {}", 100.0 * t as f64 / total.max(1) as f64, c[0] as f64 / 1e6, c[1] as f64 / 1e6, c[2] as f64 / 1e6, &k[..k.len().min(100)]);
            }
        }
        "ranges" => {
            // PSOXIDE_LIMIT_FREE / _PROFILE range lines for functions whose name contains a pattern.
            let fs = functions(&w.join("builds").join(&pos[1]).join("hk-psx-link.map"));
            for f in &fs {
                if pos[2..].iter().any(|p| f.2.contains(p.as_str())) {
                    println!("{:08x} {:08x} {}", f.0, f.0 + f.1, f.2.replace(' ', "_"));
                }
            }
        }
        "callers" => {
            // Callers (by $ra, else the stack return words) of the samples inside functions matching a pattern.
            let fs = functions(&w.join("builds").join(&pos[2]).join("hk-psx-link.map"));
            let starts: Vec<u32> = fs.iter().map(|f| f.0).collect();
            let name = |pc: u32| -> String { let j = starts.partition_point(|&s| s <= pc); if j > 0 && pc < fs[j - 1].0 + fs[j - 1].1.max(16) { fs[j - 1].2.clone() } else { format!("{pc:#x}") } };
            let csv = Csv::read(&w.join("runs").join(&pos[1]).join("callsites.csv")).unwrap();
            let hx = |s: &str| u32::from_str_radix(s.trim_start_matches("0x"), 16).unwrap_or(0);
            let mut agg: HashMap<String, u64> = HashMap::new();
            let mut total = 0u64;
            for r in &csv.rows {
                if !name(hx(&r[0])).contains(pos[3].as_str()) { continue; }
                let n: u64 = r[4].parse().unwrap_or(0);
                total += n;
                let key = format!("{} <- {} <- {}", name(hx(&r[1])), name(hx(&r[2])), name(hx(&r[3])));
                *agg.entry(key).or_default() += n;
            }
            let mut v: Vec<_> = agg.into_iter().collect();
            v.sort_by_key(|x| std::cmp::Reverse(x.1));
            for (k, n) in v.iter().take(15) { println!("{:5.1}% {}", 100.0 * *n as f64 / total.max(1) as f64, k); }
        }
        "owners" => {
            owners(&w.join("runs").join(&pos[1]), arg_value(&args, "--top").map(|v| v.parse().unwrap()).unwrap_or(30));
        }
        "ticks" => {
            // ticks RUN BUILD FROM TO: top functions per route tick from a --windows 1 run.
            let fs = functions(&w.join("builds").join(&pos[2]).join("hk-psx-link.map"));
            let starts: Vec<u32> = fs.iter().map(|f| f.0).collect();
            let name = |pc: u32| -> String { let j = starts.partition_point(|&s| s <= pc); if j > 0 && pc < fs[j - 1].0 + fs[j - 1].1.max(16) { fs[j - 1].2.clone() } else { format!("{pc:#x}") } };
            let (from, to): (i64, i64) = (pos[3].parse().unwrap(), pos[4].parse().unwrap());
            let csv = Csv::read(&w.join("runs").join(&pos[1]).join("windows.csv")).unwrap();
            let mut win: BTreeMap<i64, HashMap<String, u64>> = BTreeMap::new();
            for r in &csv.rows {
                let t = csv.num(r, "window_start_tick");
                if t < from || t > to { continue; }
                let pc = u32::from_str_radix(csv.get(r, "pc").trim_start_matches("0x"), 16).unwrap_or(0);
                *win.entry(t).or_default().entry(name(pc)).or_default() += csv.num(r, "samples") as u64;
            }
            for (t, c) in win {
                let tot: u64 = c.values().sum::<u64>().max(1);
                let mut v: Vec<_> = c.into_iter().collect();
                v.sort_by_key(|x| std::cmp::Reverse(x.1));
                println!("tick {t} ({tot} samples): {}", v.iter().take(6).map(|(k, n)| format!("{}% {}", n * 100 / tot, k.rsplit("::").next().unwrap_or(k).chars().take(40).collect::<String>())).collect::<Vec<_>>().join(", "));
            }
        }
        "cells" => {
            // cells RUN: from an emu-owners pixels.bin, the pixel writes of the dumped frame hidden by
            // later opaque writes, exactly and if trimmed at 16, 8 and 4 px screen cells.
            let raw: Vec<u32> = std::fs::read(w.join("runs").join(&pos[1]).join("pixels.bin")).unwrap().chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
            let mut last = vec![0u32; 1024 * 512];
            for p in raw.chunks_exact(2) { if p[0] >> 31 != 0 { last[p[1] as usize] = (p[0] & 0x7fff_ffff) + 1; } }
            let total = raw.len() / 2;
            let exact = raw.chunks_exact(2).filter(|p| (p[0] & 0x7fff_ffff) + 1 < last[p[1] as usize]).count();
            print!("writes {total} ({:.2} screens); hidden exactly {exact} ({:.0}%)", total as f64 / 76800.0, exact as f64 * 100.0 / total as f64);
            for c in [16usize, 8, 4] {
                let mut minl: HashMap<usize, u32> = HashMap::new();
                for (i, &l) in last.iter().enumerate() {
                    let (x, y) = (i % 1024, i / 1024);
                    let k = (y / c) * 1024 + x / c;
                    let e = minl.entry(k).or_insert(u32::MAX);
                    *e = (*e).min(l);
                }
                let hid = raw.chunks_exact(2).filter(|p| { let i = p[1] as usize; let k = (i / 1024 / c) * 1024 + (i % 1024) / c; (p[0] & 0x7fff_ffff) + 1 < minl[&k] }).count();
                print!("; {c}px cells {hid} ({:.0}%)", hid as f64 * 100.0 / total as f64);
            }
            println!();
        }
        "imgdiff" => {
            // imgdiff A.ppm B.ppm OUT.png: count differing pixels; write A | B | diff (red) side by side.
            let (a, aw, ah) = read_ppm(Path::new(&pos[1]));
            let (b, _, _) = read_ppm(Path::new(&pos[2]));
            let mut n = 0;
            let mut bbox = [usize::MAX, usize::MAX, 0, 0];
            let mut out = vec![0u8; aw * 3 * ah * 3];
            for y in 0..ah {
                for x in 0..aw {
                    let i = (y * aw + x) * 3;
                    let d = a[i..i + 3] != b[i..i + 3];
                    if d && n < 8 { println!("  ({x},{y}) {:?} vs {:?}", &a[i..i + 3], &b[i..i + 3]); }
                    if d { n += 1; bbox = [bbox[0].min(x), bbox[1].min(y), bbox[2].max(x), bbox[3].max(y)]; }
                    let o = (y * aw * 3 + x) * 3;
                    out[o..o + 3].copy_from_slice(&a[i..i + 3]);
                    out[o + aw * 3..o + aw * 3 + 3].copy_from_slice(&b[i..i + 3]);
                    let g = (a[i] as u32 + a[i + 1] as u32 + a[i + 2] as u32) as u8 / 6;
                    out[o + aw * 6..o + aw * 6 + 3].copy_from_slice(&if d { [255, 0, 0] } else { [g, g, g] });
                }
            }
            println!("differing pixels {n} bbox {bbox:?}");
            if let Some(p) = pos.get(3) { write_png(Path::new(p), aw * 3, ah, &out); }
        }
        "gate" => {
            // gate TAG_A TAG_B: lockstep screenshot sets per route (distinct frames only in A / only in B).
            let mut bad = 0;
            for r in route_list(&args) {
                let set = |t: &str| -> Option<std::collections::HashSet<String>> {
                    let d = w.join("runs").join(format!("{t}-{r}"));
                    let text = std::fs::read_to_string(d.join("shots.txt")).ok()?;
                    // Gameplay ticks only (steady mode and load state, as budget defines them).
                    let cmd: Value = serde_json::from_slice(&std::fs::read(d.join("command.json")).ok()?).ok()?;
                    let csv = Csv::read(&d.join("route.csv")).ok()?;
                    let (gm, ls) = (col(&cmd["watch"], "HK_GAME_MODE")?, col(&cmd["watch"], "HK_ROOM_LOAD_STATE")?);
                    let (play, ready) = (mode(&csv, &gm), mode(&csv, &ls));
                    let ok: std::collections::HashSet<i64> = csv.rows.iter().filter(|x| csv.get(x, &gm) == play && csv.get(x, &ls) == ready).map(|x| csv.num(x, "route_tick")).collect();
                    Some(text.lines().filter_map(|l| { let mut it = l.split_whitespace(); let n = it.next()?; let tick: i64 = n.trim_start_matches("tick-").trim_end_matches(".ppm").parse().ok()?; if ok.contains(&tick) && ok.contains(&(tick - 6)) { it.next().map(str::to_string) } else { None } }).collect())
                };
                let (Some(a), Some(b)) = (set(&pos[1]), set(&pos[2])) else { println!("{r}: missing"); continue; };
                let (oa, ob) = (a.difference(&b).count(), b.difference(&a).count());
                if oa + ob > 0 { bad += 1; }
                println!("{r}: {} vs {} distinct, only A {oa}, only B {ob}", a.len(), b.len());
            }
            println!("routes with any difference: {bad}");
        }
        "shotcmp" => {
            // Lockstep pixel gate: distinct screenshot hashes (shots.txt, consecutive
            // duplicates collapsed) and distinct flipped-frame GP0 hashes (gpu.csv)
            // of two runs of one route. Prints what each has that the other lacks.
            let seq = |run: &str| -> Vec<(String, String)> {
                let text = std::fs::read_to_string(w.join("runs").join(run).join("shots.txt")).unwrap_or_default();
                let mut out: Vec<(String, String)> = vec![];
                for l in text.lines() {
                    let mut it = l.split_whitespace();
                    let (n, h) = (it.next().unwrap_or("").to_string(), it.next().unwrap_or("").to_string());
                    if out.last().map(|x| &x.1) != Some(&h) { out.push((n, h)); }
                }
                out
            };
            let flips = |run: &str| -> Vec<String> {
                Csv::read(&w.join("runs").join(run).join("gpu.csv")).map(|c| c.rows.iter().filter(|r| c.get(r, "display_start_changed") == "1").map(|r| c.get(r, "frame_draw_hash").to_string()).collect()).unwrap_or_default()
            };
            for (what, a, b) in [("screenshots", seq(&pos[1]).into_iter().map(|x| (x.0, x.1)).collect::<Vec<_>>(), seq(&pos[2])),
                ("flip packets", flips(&pos[1]).into_iter().map(|h| (String::new(), h)).collect(), flips(&pos[2]).into_iter().map(|h| (String::new(), h)).collect())] {
                let hb: std::collections::HashSet<&String> = b.iter().map(|x| &x.1).collect();
                let ha: std::collections::HashSet<&String> = a.iter().map(|x| &x.1).collect();
                let only_a: Vec<_> = a.iter().filter(|x| !hb.contains(&x.1)).collect();
                let only_b: Vec<_> = b.iter().filter(|x| !ha.contains(&x.1)).collect();
                println!("{what}: {} vs {} distinct; only in A {} only in B {}", ha.len(), hb.len(), only_a.len(), only_b.len());
                for x in only_a.iter().take(5) { println!("  A only {} {}", x.0, x.1); }
                for x in only_b.iter().take(5) { println!("  B only {} {}", x.0, x.1); }
            }
        }
        _ => {
            eprintln!("see the header of src/main.rs for usage");
            std::process::exit(2);
        }
    }
}

fn s11(v: u32) -> i32 {
    let v = (v & 0x7ff) as i32;
    if v & 0x400 != 0 { v - 0x800 } else { v }
}

/// Written vs surviving pixels per GP0 packet of a station frame dumped by the
/// pixel-owner emulator (HK_PIXEL_OWNERS: owners.bin, writes.bin; --dump-draws).
fn owners(dir: &Path, top: usize) {
    let text = std::fs::read_to_string(dir.join("draws.csv")).unwrap();
    let rows: Vec<Vec<&str>> = text.lines().map(|l| l.split(',').collect()).collect();
    let start = rows.iter().rposition(|r| r.get(1) == Some(&"02")).unwrap();
    let fill = &rows[start];
    let fy = (u32::from_str_radix(fill[3], 16).unwrap() >> 16) as usize;
    let own: Vec<u32> = std::fs::read(dir.join("owners.bin")).unwrap().chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    let erased: Vec<u32> = std::fs::read(dir.join("erased.bin")).unwrap_or_default().chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    let writes: Vec<u32> = std::fs::read(dir.join("writes.bin")).unwrap().chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    let mut surv: HashMap<u32, u32> = HashMap::new();
    for y in 0..240 {
        for x in 0..320 {
            *surv.entry(own[(fy + y) * 1024 + x]).or_default() += 1;
        }
    }
    let (mut tw, mut ts, mut quads, mut scissors) = (0u64, 0u64, 0u32, 0u32);
    let mut per = vec![];
    let mut ops: BTreeMap<String, (u32, u64, u64)> = BTreeMap::new();
    let mut out = String::from("index,op,written,surviving,left,top,right,bottom\n");
    for r in &rows[start..] {
        let i: usize = r[0].parse().unwrap();
        let w = writes.get(i).copied().unwrap_or(0) as u64;
        let s = w.saturating_sub(erased.get(i).copied().unwrap_or(0) as u64);
        let _ = &surv;
        tw += w;
        ts += s;
        let e = ops.entry(r[1].to_string()).or_default();
        e.0 += 1;
        e.1 += w;
        e.2 += s;
        if r[1] == "E3" { scissors += 1; }
        let op = u32::from_str_radix(r[1], 16).unwrap_or(0);
        if op == 0x28 && r.len() >= 6 {
            let vs: Vec<(i32, i32)> = [2, 3, 4, 5].iter().map(|&k| { let v = u32::from_str_radix(r[k], 16).unwrap(); (s11(v), s11(v >> 16)) }).collect();
            println!("  flat quad {i}: {:?} written {w} surviving {s}", vs);
        }
        if (0x2c..=0x2f).contains(&op) && r.len() >= 10 {
            quads += 1;
            let vs: Vec<(i32, i32)> = [3, 5, 7, 9].iter().map(|&k| { let v = u32::from_str_radix(r[k], 16).unwrap(); (s11(v), s11(v >> 16)) }).collect();
            let b = [vs.iter().map(|v| v.0).min().unwrap(), vs.iter().map(|v| v.1).min().unwrap(), vs.iter().map(|v| v.0).max().unwrap(), vs.iter().map(|v| v.1).max().unwrap()];
            out.push_str(&format!("{i},{},{w},{s},{},{},{},{}\n", r[1], b[0], b[1], b[2], b[3]));
            per.push((w - s, w, s, i, r[1].to_string(), r[4].to_string(), r[6].to_string()));
        } else if w > 0 {
            out.push_str(&format!("{i},{},{w},{s},,,,\n", r[1]));
        }
    }
    std::fs::write(dir.join("owners.csv"), out).unwrap();
    println!("frame y {fy}: packets {} (textured quads {quads}, draw-area changes {scissors}); written {tw} ({:.2} screens), kept (not erased by a later opaque write) {ts}, erased {} ({:.0}%)",
        rows.len() - start, tw as f64 / 76800.0, tw - ts, (tw - ts) as f64 * 100.0 / tw.max(1) as f64);
    for (op, (n, w, s)) in &ops {
        if *w > 0 { println!("  op {op}: {n} packets, written {w}, surviving {s}"); }
    }
    per.sort_by(|a, b| b.0.cmp(&a.0));
    println!("top packets by overwritten pixels (overwritten, written, surviving, index, op, uv0clut, uv1tpage):");
    for p in per.iter().take(top) { println!("  {:?}", p); }
    let mut by: HashMap<(String, String), (u64, u64, u32)> = HashMap::new();
    for p in &per {
        let e = by.entry((p.5[..4].to_string(), p.6[..4].to_string())).or_default();
        e.0 += p.0;
        e.1 += p.1;
        e.2 += 1;
    }
    let mut v: Vec<_> = by.into_iter().collect();
    v.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    println!("by (clut, tpage): overwritten, written, packets");
    for (k, x) in v.iter().take(top) { println!("  {:?} {:?}", k, x); }
}

fn read_ppm(path: &Path) -> (Vec<u8>, usize, usize) {
    let data = std::fs::read(path).unwrap();
    let mut fields = vec![];
    let mut i = 0;
    while fields.len() < 4 {
        while data[i].is_ascii_whitespace() { i += 1; }
        let s = i;
        while !data[i].is_ascii_whitespace() { i += 1; }
        fields.push(String::from_utf8_lossy(&data[s..i]).to_string());
    }
    i += 1;
    let (w, h): (usize, usize) = (fields[1].parse().unwrap(), fields[2].parse().unwrap());
    (data[i..i + w * h * 3].to_vec(), w, h)
}

/// RGB PNG with stored (uncompressed) deflate blocks.
fn write_png(path: &Path, w: usize, h: usize, rgb: &[u8]) {
    fn crc(data: &[u8]) -> u32 {
        let mut c = 0xffff_ffffu32;
        for &b in data {
            c ^= b as u32;
            for _ in 0..8 { c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 }; }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8], body: &[u8]) {
        out.extend((body.len() as u32).to_be_bytes());
        let mut c = kind.to_vec();
        c.extend(body);
        out.extend(&c);
        out.extend(crc(&c).to_be_bytes());
    }
    let mut raw = Vec::with_capacity((w * 3 + 1) * h);
    for y in 0..h { raw.push(0); raw.extend(&rgb[y * w * 3..(y + 1) * w * 3]); }
    let mut z = vec![0x78, 0x01];
    for (k, block) in raw.chunks(65535).enumerate() {
        let last = (k + 1) * 65535 >= raw.len();
        z.push(last as u8);
        z.extend((block.len() as u16).to_le_bytes());
        z.extend((!(block.len() as u16)).to_le_bytes());
        z.extend(block);
    }
    let (mut s1, mut s2) = (1u32, 0u32);
    for &b in &raw { s1 = (s1 + b as u32) % 65521; s2 = (s2 + s1) % 65521; }
    z.extend(((s2 << 16) | s1).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = (w as u32).to_be_bytes().to_vec();
    ihdr.extend((h as u32).to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    std::fs::write(path, out).unwrap();
}
