//! Route pins and state-driven (closed loop) routes. See docs/ROUTES.md.
//!
//! A route is a recorded tape, `tools/tapes/NAME.pxtape`, and a pin file beside it,
//! `tools/tapes/NAME.pins`. The driver globs the pin files; a lane that adds a route adds
//! two files and edits no table.
//!
//! Pin file, one entry per line (`#` starts a comment):
//!
//! ```text
//! card = town-continue.mcd     boots from tools/cards/town-continue.mcd (absent: a formatted card)
//! scene_gate = true            also run the resident-scene verifier on the open-loop replay
//! journey = 2                  segment 2 of the save journey: boots from the card segment 1 wrote
//! frame_time = 289             mean frame time in hundredths of a vblank over the judged frames
//! long_frames = 3              frames of four or more vblanks the route may still show
//! pacing = 303                 late frames (over two vblanks) the route may still show
//! art_late = 44                frames the route may still show late art for
//! missed_vblanks = 12          pad polls the route may still miss (read by tools/replay_cue.py)
//! HK_REGION_ID = 150           final telemetry that must be exactly this
//! HK_HEALTH >= 2               final telemetry that must be at least this
//! ```
//!
//! A tape counts frames, so every scene load that takes a different number of polls moves
//! everything after it. `NAME.cuts` (written by `cargo hk-build cuts` from a baseline replay)
//! lists where the scene loads were; a route that has one is also played closed loop: the
//! tape is cut at those loads and `psoxide-gate` plays each stretch from the poll where
//! the game reports gameplay again, waiting on `HK_SCENE_LOADS` and `HK_GAME_MODE` through
//! every load. The pins of such a route are judged on that run.
use super::Result;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    pub symbol: String,
    /// `false`: exactly `value`; `true`: at least `value`.
    pub at_least: bool,
    pub value: u64,
}

#[derive(Debug, Clone, Default)]
pub struct Route {
    pub name: String,
    pub card: Option<String>,
    pub scene_gate: bool,
    pub journey: Option<u32>,
    /// Secondary ceiling: gameplay frames over two vblanks. Zero when absent.
    pub pacing: u64,
    /// Mean frame time over the judged frames, in hundredths of a vblank (289 = 2.89 vblanks).
    /// The figure that still moves when most frames are late. `None`: not checked.
    pub frame_time: Option<u64>,
    /// Gameplay frames of four or more vblanks (the tail). `None`: not checked.
    pub long_frames: Option<u64>,
    pub art_late: u64,
    /// Pad polls the route may miss; the replay itself judges it (tools/replay_cue.py).
    pub missed_vblanks: u64,
    pub pins: Vec<Pin>,
    /// Scene loads found in the baseline replay; empty when the route has no cut list.
    pub cuts: Option<Cuts>,
}

impl Route {
    /// The ceilings the route's pin file holds its frame pacing to (host/hk-build/pacing.rs).
    pub fn pacing_pins(&self) -> super::pacing::PacingPins {
        super::pacing::PacingPins { pacing: self.pacing, frame_time: self.frame_time, long_frames: self.long_frames }
    }
}

pub fn parse_pins(name: &str, text: &str) -> Result<Route> {
    let mut route = Route {
        name: name.to_string(),
        ..Route::default()
    };
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let at = |what: &str| format!("{name}.pins line {}: {what}", index + 1);
        let number = |v: &str| -> Result<u64> { v.parse().map_err(|_| at("not a number").into()) };
        if let Some((key, value)) = line.split_once(">=") {
            route.pins.push(Pin {
                symbol: key.trim().to_string(),
                at_least: true,
                value: number(value.trim())?,
            });
        } else if let Some((key, value)) = line.split_once('=') {
            let (key, value) = (key.trim(), value.trim());
            match key {
                "card" => route.card = Some(value.to_string()),
                "scene_gate" => route.scene_gate = value == "true",
                "journey" => route.journey = Some(number(value)? as u32),
                "pacing" => route.pacing = number(value)?,
                "frame_time" => route.frame_time = Some(number(value)?),
                "long_frames" => route.long_frames = Some(number(value)?),
                "art_late" => route.art_late = number(value)?,
                "missed_vblanks" => route.missed_vblanks = number(value)?,
                k if k.starts_with("HK_") || k.starts_with("__") => route.pins.push(Pin {
                    symbol: k.to_string(),
                    at_least: false,
                    value: number(value)?,
                }),
                _ => return Err(at(&format!("unknown key {key}")).into()),
            }
        } else {
            return Err(at("expected `key = value` or `SYMBOL >= value`").into());
        }
    }
    Ok(route)
}

/// Every route with a pin file, plain routes first (by name), then the journey in its order.
pub fn load(root: &Path) -> Result<Vec<Route>> {
    let dir = root.join("tools/tapes");
    let mut names: Vec<String> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            e.file_name()
                .to_str()
                .and_then(|n| n.strip_suffix(".pins"))
                .map(str::to_owned)
        })
        .collect();
    names.sort();
    let mut routes = Vec::new();
    for name in names {
        let mut route = parse_pins(&name, &std::fs::read_to_string(dir.join(format!("{name}.pins")))?)?;
        if !dir.join(format!("{name}.pxtape")).is_file() {
            return Err(format!("tools/tapes/{name}.pins has no tape {name}.pxtape").into());
        }
        let cuts = dir.join(format!("{name}.cuts"));
        if cuts.is_file() {
            route.cuts = Some(Cuts::parse(&std::fs::read_to_string(cuts)?)?);
        }
        routes.push(route);
    }
    routes.sort_by_key(|r| (r.journey.is_some(), r.journey, r.name.clone()));
    let mut seen = BTreeMap::new();
    for r in &routes {
        if let Some(j) = r.journey {
            if let Some(other) = seen.insert(j, &r.name) {
                return Err(format!("journey position {j} is claimed by {other} and {}", r.name).into());
            }
        }
    }
    Ok(routes)
}

/// One scene load seen by the baseline replay, in port-1 polls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Load {
    /// Poll count when `HK_GAME_MODE` became 2.
    pub start: usize,
    /// Poll count when it left 2.
    pub end: usize,
    /// `HK_SCENE_LOADS` at that moment.
    pub loads_after: u64,
    /// Neutral ticks played between the game saying gameplay is back and the next stretch. A game
    /// built with `route-lock` resumes the simulation on a poll boundary, so these are the only
    /// ticks the simulation runs before the stretch, on every disc layout. The baseline gives the
    /// number: a replay without the lock ran as many neutral ticks as the region setup and the
    /// gate's frame granularity left, 1 to 4 on the discs measured (docs/ROUTES.md, "Phase").
    pub pad: usize,
}

/// A state the game must reach before the next stretch of tape is played, for drift
/// inside a scene (see docs/ROUTES.md). The tape is cut at `poll`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wait {
    pub poll: usize,
    pub symbol: String,
    pub op: String,
    pub value: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cuts {
    pub loads: Vec<Load>,
    pub waits: Vec<Wait>,
}

impl Cuts {
    pub fn parse(text: &str) -> Result<Cuts> {
        let mut cuts = Cuts::default();
        for line in text.lines() {
            let f: Vec<&str> = line.split_whitespace().collect();
            match f.as_slice() {
                [] => {}
                [c, ..] if c.starts_with('#') => {}
                ["load", a, b, c] => cuts.loads.push(Load {
                    start: a.parse()?,
                    end: b.parse()?,
                    loads_after: c.parse()?,
                    pad: 0,
                }),
                ["load", a, b, c, "pad", n] => cuts.loads.push(Load {
                    start: a.parse()?,
                    end: b.parse()?,
                    loads_after: c.parse()?,
                    pad: n.parse()?,
                }),
                ["wait", poll, symbol, op, value] => cuts.waits.push(Wait {
                    poll: poll.parse()?,
                    symbol: symbol.to_string(),
                    op: op.to_string(),
                    value: value.parse()?,
                }),
                _ => return Err(format!("bad cuts line `{line}`").into()),
            }
        }
        Ok(cuts)
    }

    pub fn render(&self, header: &str) -> String {
        let mut out = String::new();
        for line in header.lines() {
            let _ = writeln!(out, "# {line}");
        }
        for l in &self.loads {
            if l.pad > 0 {
                let _ = writeln!(out, "load {} {} {} pad {}", l.start, l.end, l.loads_after, l.pad);
            } else {
                let _ = writeln!(out, "load {} {} {}", l.start, l.end, l.loads_after);
            }
        }
        for w in &self.waits {
            let _ = writeln!(out, "wait {} {} {} {}", w.poll, w.symbol, w.op, w.value);
        }
        out
    }
}

/// Where the baseline replay (`replay_cue.py` output: `route.csv` and `command.json`) saw the
/// game load a scene: the spans of `HK_GAME_MODE == 2`.
pub fn loads_from_replay(dir: &Path) -> Result<Vec<Load>> {
    let record: Value = serde_json::from_slice(&std::fs::read(dir.join("command.json"))?)?;
    let column = |name: &str| -> Result<String> {
        let address = record["watches"][name]
            .as_str()
            .ok_or_else(|| format!("command.json has no watch {name}"))?;
        Ok(format!("ram_{}", address.trim_start_matches("0x")))
    };
    let text = std::fs::read_to_string(dir.join("route.csv"))?;
    loads_from_csv(&text, &column("HK_GAME_MODE")?, &column("HK_SCENE_LOADS")?)
}

pub fn loads_from_csv(text: &str, mode: &str, loads: &str) -> Result<Vec<Load>> {
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().ok_or("empty route.csv")?.split(',').collect();
    let at = |n: &str| {
        header
            .iter()
            .position(|h| *h == n)
            .ok_or_else(|| format!("route.csv has no column {n}"))
    };
    let (c_polls, c_mode, c_loads) = (at("port1_polls")?, at(mode)?, at(loads)?);
    let mut spans = Vec::new();
    let mut open: Option<usize> = None;
    for line in lines {
        let f: Vec<&str> = line.split(',').collect();
        let polls: usize = f[c_polls].parse().unwrap_or(0);
        let (m, l): (u64, u64) = (f[c_mode].parse().unwrap_or(0), f[c_loads].parse().unwrap_or(0));
        match (open, m) {
            (None, 2) => open = Some(polls),
            (Some(start), m) if m != 2 => {
                spans.push(Load { start, end: polls, loads_after: l, pad: 0 });
                open = None;
            }
            _ => {}
        }
    }
    Ok(spans)
}

/// The mask of a button by the name a journey uses (`r3`, `cross`, ...).
pub fn button_mask(name: &str) -> Option<u16> {
    BUTTONS.iter().find(|(n, _)| *n == name).map(|(_, m)| *m)
}

const BUTTONS: [(&str, u16); 16] = [
    ("select", 1), ("l3", 2), ("r3", 4), ("start", 8), ("up", 16), ("right", 32), ("down", 64),
    ("left", 128), ("l2", 256), ("r2", 512), ("l1", 1024), ("r1", 2048), ("triangle", 4096),
    ("circle", 8192), ("cross", 16384), ("square", 32768),
];

/// A PXITAPE2 tape: one pad mask per port-1 poll.
pub struct Tape {
    rows: Vec<[u8; 6]>,
}

impl Tape {
    pub fn parse(bytes: &[u8]) -> Result<Tape> {
        if bytes.len() < 16 || &bytes[..8] != b"PXITAPE2" {
            return Err("not a PXITAPE2 tape".into());
        }
        let count = u32::from_le_bytes(bytes[8..12].try_into()?) as usize;
        if bytes.len() != 16 + count * 6 {
            return Err("tape length does not match its header".into());
        }
        Ok(Tape {
            rows: bytes[16..].chunks(6).map(|c| c.try_into().unwrap()).collect(),
        })
    }
    pub fn len(&self) -> usize {
        self.rows.len()
    }
    fn mask(&self, i: usize) -> u16 {
        u16::from_le_bytes([self.rows[i][0], self.rows[i][1]])
    }
    fn slice(&self, lo: usize, hi: usize) -> Vec<u8> {
        let mut out = b"PXITAPE2".to_vec();
        out.extend_from_slice(&((hi - lo) as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        for r in &self.rows[lo..hi] {
            out.extend_from_slice(r);
        }
        out
    }
}

/// What `journey` needs to know about the disc under test.
pub struct Setup<'a> {
    pub name: &'a str,
    pub disc: &'a Path,
    pub symbols: &'a Path,
    /// Port-1 card image the run boots from, if any, and its SHA-256.
    pub card: Option<(&'a Path, String)>,
    /// Write the card back as a snapshot at the end (the next journey segment boots from it).
    pub snapshot: bool,
    /// Read the guest's scene-load telemetry at every gameplay-is-back point and at the end, as
    /// deliberately failing asserts whose detail carries the values (`cargo hk-build measure`).
    pub probe: bool,
    /// Hold L3 from the load's start until gameplay is published, which asks the game for the route
    /// lock (input::lock_requested). Off for a measuring run, which wants the layout's own timing.
    pub lock: bool,
}

enum Cut<'a> {
    Load(&'a Load),
    Wait(&'a Wait),
}

/// Write `NAME.toml` and its stretch tapes to `out`; the journey plays each stretch from the
/// poll where the game says gameplay is back.
pub fn write_journey(route: &Route, tape: &Tape, cuts: &Cuts, setup: &Setup, out: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(out)?;
    let mut order: Vec<(usize, Cut)> = cuts
        .loads
        .iter()
        .map(|l| (l.start, Cut::Load(l)))
        .chain(cuts.waits.iter().map(|w| (w.poll, Cut::Wait(w))))
        .collect();
    order.sort_by_key(|(poll, _)| *poll);
    let name = setup.name;
    let mut toml = String::new();
    let _ = writeln!(toml, "# Generated by cargo hk-build from tools/tapes/{}.pxtape and its cut list.", route.name);
    let _ = writeln!(toml, "name = \"{name}\"\ntitle = \"{name} (state-driven)\"");
    let _ = writeln!(
        toml,
        "disc = [\"{}\"]\nsymbols = [\"{}\"]\nmax_ticks = 3000000",
        setup.disc.display(),
        setup.symbols.display()
    );
    if let Some((path, sha)) = &setup.card {
        let _ = writeln!(toml, "card_fixture = {{ path = \"{}\", sha256 = \"{sha}\" }}", path.display());
    }
    let seg = |k: usize| format!("{name}-seg{k}.pxtape");
    // Stretch k runs from the end of cut k-1 to the start of cut k.
    let mut lo = 0usize;
    let mut pad = 0usize;
    let mut load_index = 0usize;
    for k in 0..=order.len() {
        let hi = order.get(k).map_or(tape.len(), |(poll, _)| *poll).min(tape.len());
        let lo_k = lo.min(hi);
        if hi > lo_k {
            std::fs::write(out.join(seg(k)), tape.slice(lo_k, hi))?;
            if pad > 0 {
                let _ = writeln!(toml, "\n[[step]]\nlabel = \"{pad} neutral ticks before stretch {k}\"\nwait = {pad}");
            }
            let _ = writeln!(toml, "\n[[step]]\nlabel = \"play stretch {k} (polls {lo_k}..{hi} of the baseline)\"\ntape = \"{}\"", seg(k));
            if k > 0 {
                let _ = writeln!(toml, "tape_rebase = true");
            }
            // A tape step is capped at 3600 ticks unless it says otherwise.
            let _ = writeln!(toml, "timeout = {}", (hi - lo_k) + 1800);
        }
        let Some((_, cut)) = order.get(k) else { break };
        match cut {
            Cut::Load(load) => {
                let held = if hi > lo_k { tape.mask(hi - 1) } else { 0 };
                let mut names: Vec<String> =
                    BUTTONS.iter().filter(|(_, m)| held & m != 0).map(|(n, _)| format!("\"{n}\"")).collect();
                if setup.lock {
                    names.push("\"l3\"".into());
                }
                if !names.is_empty() {
                    // The stretch ends mid-walk: keep the last buttons down until the load has been admitted.
                    // HK_SCENE_LOADS, not HK_GAME_MODE == 2: a gate that loads while the game keeps playing
                    // (seamless doorways) never shows mode 2, and the walk must still reach it.
                    let _ = writeln!(toml, "\n[[step]]\nlabel = \"keep walking until load {k} is admitted\"\npress = [{}]\ntimeout = 7200", names.join(", "));
                    let _ = writeln!(toml, "[step.hold_until]\nkind = \"ram\"\nsym = \"HK_SCENE_LOADS\"\nop = \"ge\"\nvalue = {}", load.loads_after);
                }
                if setup.lock {
                    // L3 stays down until the game publishes gameplay: that asks for the route lock.
                    let _ = writeln!(toml, "\n[[step]]\nlabel = \"wait for gameplay (route lock requested)\"\npress = [\"l3\"]\ntimeout = 7200");
                    let _ = writeln!(toml, "[step.hold_until]\nkind = \"ram\"\nsym = \"HK_GAME_MODE\"\nop = \"eq\"\nvalue = 1");
                } else {
                    let _ = writeln!(toml, "\n[[step]]\nlabel = \"wait for scene load {k} to be admitted\"\ntimeout = 7200");
                    let _ = writeln!(toml, "[step.wait_until]\nkind = \"ram\"\nsym = \"HK_SCENE_LOADS\"\nop = \"ge\"\nvalue = {}", load.loads_after);
                    let _ = writeln!(toml, "\n[[step]]\nlabel = \"wait for gameplay\"\ntimeout = 7200");
                    let _ = writeln!(toml, "[step.wait_until]\nkind = \"ram\"\nsym = \"HK_GAME_MODE\"\nop = \"eq\"\nvalue = 1");
                }
                if setup.probe {
                    let _ = writeln!(toml, "\n[[step]]\nlabel = \"probe the poll count\"\ncheckpoint = \"probe{k}\"\ngolden = false\nno_matrix = true");
                    let _ = writeln!(toml, "[[step.assert]]\nkind = \"ram\"\nsym = \"HK_INPUT_POLLS\"\nop = \"eq\"\nvalue = -1\nnote = \"probe-polls-{}\"", load_index);
                }
                lo = load.end;
                pad = load.pad;
                load_index += 1;
            }
            Cut::Wait(wait) => {
                let _ = writeln!(toml, "\n[[step]]\nlabel = \"wait for {} {} {}\"\ntimeout = 7200", wait.symbol, wait.op, wait.value);
                let _ = writeln!(toml, "[step.wait_until]\nkind = \"ram\"\nsym = \"{}\"\nop = \"{}\"\nvalue = {}", wait.symbol, wait.op, wait.value);
                lo = wait.poll;
                pad = 0;
            }
        }
    }
    if setup.probe && load_index > 0 {
        let _ = writeln!(toml, "\n[[step]]\nlabel = \"probe the resume polls\"\ncheckpoint = \"probe-resume\"\ngolden = false\nno_matrix = true");
        for i in 0..load_index.min(16) {
            let _ = writeln!(toml, "[[step.assert]]\nkind = \"ram\"\nsym = \"HK_LOAD_LOG\"\noffset = {}\nop = \"eq\"\nvalue = -1\nnote = \"probe-resume-{i}\"", 16 * i);
        }
    }
    // Pins first; the card snapshot after them so a failed run still says why.
    if !route.pins.is_empty() {
        let _ = writeln!(toml, "\n[[step]]\nlabel = \"the route's end-state pins\"\ncheckpoint = \"end\"\ngolden = false\nno_matrix = true");
        for p in &route.pins {
            let _ = writeln!(
                toml,
                "[[step.assert]]\nkind = \"ram\"\nsym = \"{}\"\nop = \"{}\"\nvalue = {}\nnote = \"{}\"",
                p.symbol,
                if p.at_least { "ge" } else { "eq" },
                p.value,
                p.symbol
            );
        }
    }
    if setup.snapshot {
        let _ = writeln!(toml, "\n[[step]]\nlabel = \"hand the card to the next segment\"\n[step.card]\naction = \"snapshot\"");
    }
    let file = out.join(format!("{name}.toml"));
    std::fs::write(&file, toml)?;
    Ok(file)
}


/// The `psoxide-gate` that plays closed-loop routes: an external binary, bound to its SHA-256
/// the way the profiling frontend is (`HK_PSX_GATE` + `HK_PSX_GATE_SHA256`, or `--gate`).
pub struct Gate {
    pub binary: PathBuf,
    pub sha256: String,
}

impl Gate {
    pub fn resolve(flag: Option<&Path>) -> Result<Gate> {
        let binary = flag
            .map(Path::to_path_buf)
            .or_else(|| std::env::var_os("HK_PSX_GATE").map(PathBuf::from))
            .ok_or("closed-loop routes need psoxide-gate: pass --gate PATH or set HK_PSX_GATE and HK_PSX_GATE_SHA256")?;
        let expected = std::env::var("HK_PSX_GATE_SHA256")
            .map_err(|_| "an external psoxide-gate requires HK_PSX_GATE_SHA256")?;
        if !binary.is_file() {
            return Err(format!("psoxide-gate not found at {}", binary.display()).into());
        }
        let sha256 = super::sha256_file(&binary)?;
        if !sha256.eq_ignore_ascii_case(expected.trim()) {
            return Err(format!("psoxide-gate {} has SHA-256 {sha256}, expected {expected}", binary.display()).into());
        }
        Ok(Gate { binary, sha256 })
    }
}

/// A closed-loop run in flight.
pub struct ClosedRun {
    name: String,
    dir: PathBuf,
    child: std::io::Result<std::process::Child>,
}

/// The outcome of a closed-loop run.
pub struct Closed {
    pub ok: bool,
    /// The card the route left behind (journey routes only), for the next segment.
    pub card: Option<PathBuf>,
    /// Why not, one line per failed check.
    pub failures: Vec<String>,
    pub headline: String,
}

fn sha256_hex(path: &Path) -> Result<String> {
    super::sha256_file(path)
}

/// Generate the route's journey for the disc in `library` and start the gate on it.
/// `card_in` is the card a journey segment boots from (what the segment before it left).
pub fn spawn_closed(
    root: &Path,
    gate: &Gate,
    library: &Path,
    out: &Path,
    route: &Route,
    card_in: Option<&Path>,
    probe: bool,
) -> Result<ClosedRun> {
    let cuts = route.cuts.as_ref().ok_or("route has no cut list")?;
    // The link map of the disc under test: game.map beside it in the library (a disc built elsewhere
    // than the last build), else the last build's.
    let map = if library.join("game.map").is_file() {
        library.join("game.map")
    } else {
        let build: Value = serde_json::from_slice(&std::fs::read(root.join(".hkpsx/build.json"))?)?;
        PathBuf::from(build["link_map"]["path"].as_str().ok_or("build.json has no link map")?)
    };
    let tape = Tape::parse(&std::fs::read(root.join(format!("tools/tapes/{}.pxtape", route.name)))?)?;
    let dir = out.join(format!("{}-sd", route.name));
    let _ = std::fs::remove_dir_all(&dir);
    let fixture = match (&route.card, card_in) {
        (Some(card), _) => Some(root.join("tools/cards").join(card)),
        (None, Some(card)) => Some(card.to_path_buf()),
        (None, None) => None,
    };
    let card = match &fixture {
        Some(path) => Some((path.as_path(), sha256_hex(path)?)),
        None => None,
    };
    let name = format!("{}-sd", route.name);
    let cue = super::disc::disc_cue(&library);
    let setup = Setup {
        name: &name,
        disc: &cue,
        symbols: &map,
        card: card.as_ref().map(|(p, s)| (*p, s.clone())),
        snapshot: route.journey.is_some(),
        probe,
        lock: !probe,
    };
    let toml = write_journey(route, &tape, cuts, &setup, &dir)?;
    let child = std::process::Command::new(&gate.binary)
        .current_dir(&dir)
        .arg("run")
        .arg(&toml)
        .args(["--disc"])
        .arg(&cue)
        .args(["--no-hw", "--out"])
        .arg(dir.join("report"))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();
    Ok(ClosedRun { name: route.name.clone(), dir, child })
}

/// Wait for the gate and read its verdict.
pub fn finish_closed(run: ClosedRun) -> Result<Closed> {
    let output = run.child?.wait_with_output()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::write(run.dir.join("gate.log"), &text)?;
    let failures: Vec<String> = text
        .lines()
        .filter(|l| l.trim_start().starts_with("FAIL") || l.trim_start().starts_with("ABORT"))
        .map(|l| l.trim().to_string())
        .collect();
    let headline = text
        .lines()
        .find(|l| l.starts_with("PASS ") || l.starts_with("FAIL ") || l.starts_with("ABORT "))
        .unwrap_or("no verdict from psoxide-gate")
        .to_string();
    let ok = output.status.success() && failures.is_empty() && headline.starts_with("PASS ");
    // The newest card receipt is the snapshot at the end of the journey.
    let cards = run.dir.join("report").join(format!("{}-sd", run.name)).join("cards");
    let mut receipts: Vec<PathBuf> = std::fs::read_dir(&cards)
        .map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    receipts.sort();
    let card = match receipts.last() {
        Some(last) => {
            let to = run.dir.join(format!("{}.mcd", run.name));
            std::fs::copy(last, &to)?;
            Some(to)
        }
        None => None,
    };
    Ok(Closed { ok, card, failures, headline })
}

/// The neutral ticks each load left the baseline simulation before the stretch that followed it:
/// the controller polls taken when gameplay was back (`probe-polls-i`) minus the polls taken when
/// load i resumed (`probe-resume-i`, from the guest's `HK_LOAD_LOG`), both in the gate log's
/// failing-assert detail. Valid only for a run that did not ask for the route lock,
/// where the gate's stretch starts after the region setup the layout happened to cost.
pub fn parse_probes(gate_log: &str, loads: usize) -> Result<Vec<usize>> {
    let find = |kind: &str, i: usize| -> Result<usize> {
        let note = format!("[probe-{kind}-{i}]");
        let line = gate_log.lines().find(|l| l.contains(&note)).ok_or(format!("no {note} in the gate log"))?;
        let after = line.split("value = [").nth(1).ok_or(format!("no value in `{line}`"))?;
        Ok(after.split(|c: char| !c.is_ascii_digit()).next().unwrap_or("").parse()?)
    };
    (0..loads)
        .map(|i| {
            let (polls, resume) = (find("polls", i)?, find("resume", i)?);
            polls.checked_sub(resume).ok_or_else(|| format!("load {i}: polls {polls} before resume {resume}").into())
        })
        .collect()
}

/// `text` (a cuts file) with the `pad` of its load lines replaced, in order.
pub fn set_pads(text: &str, pads: &[usize]) -> String {
    let mut k = 0;
    let mut out = String::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.first() == Some(&"load") && f.len() >= 4 {
            let pad = pads.get(k).copied().unwrap_or(0);
            k += 1;
            if pad > 0 {
                let _ = writeln!(out, "load {} {} {} pad {}", f[1], f[2], f[3], pad);
            } else {
                let _ = writeln!(out, "load {} {} {}", f[1], f[2], f[3]);
            }
        } else {
            let _ = writeln!(out, "{line}");
        }
    }
    out
}

/// `tape` with the buttons in `mask` held from sample `first` up to (not including) `last`. A new
/// game's tape holds R3 across the load so that the game begins on the ledge rather than with the
/// opening drop (game/src/main.rs), the state every recorded route is bit-exact from.
pub fn hold_buttons(tape: &[u8], first: usize, last: usize, mask: u16) -> Result<Vec<u8>> {
    let parsed = Tape::parse(tape)?;
    if first >= last || last > parsed.len() {
        return Err(format!("polls {first}..{last} are outside the tape ({} polls)", parsed.len()).into());
    }
    let mut out = tape.to_vec();
    for i in first..last {
        let at = 16 + i * 6;
        let held = u16::from_le_bytes([out[at], out[at + 1]]) | mask;
        out[at..at + 2].copy_from_slice(&held.to_le_bytes());
    }
    Ok(out)
}

/// `tape` with `ticks` more copies of sample `poll` right after it: the Knight is locked for those
/// ticks (a hard landing), so the recorded walk resumes where it would have after a soft landing.
pub fn stall_sample(tape: &[u8], poll: usize, ticks: usize) -> Result<Vec<u8>> {
    let parsed = Tape::parse(tape)?;
    if poll >= parsed.len() {
        return Err(format!("poll {poll} is outside the tape ({} polls)", parsed.len()).into());
    }
    let at = 16 + poll * 6;
    let mut out = tape[..at + 6].to_vec();
    for _ in 0..ticks {
        out.extend_from_slice(&tape[at..at + 6]);
    }
    out.extend_from_slice(&tape[at + 6..]);
    out[8..12].copy_from_slice(&((parsed.len() + ticks) as u32).to_le_bytes());
    Ok(out)
}

/// `cargo hk-build cuts`: write `NAME.cuts` for every route whose baseline replay in `from`
/// (a `validate` output directory) loaded at least `minimum` scenes after the first.
pub fn write_cuts(root: &Path, from: &Path, names: &[String], minimum: usize) -> Result<()> {
    let header = |disc: &str| {
        format!(
            "Scene loads of the baseline replay (polls), cut by `cargo hk-build cuts`.\nbaseline disc sha256 {disc}\nsee docs/ROUTES.md"
        )
    };
    let disc = {
        let record: Value = serde_json::from_slice(&std::fs::read(from.join("summary.json"))?)?;
        record["disc_sha256"].as_str().unwrap_or("not recorded").to_string()
    };
    for route in load(root)? {
        if !names.is_empty() && !names.iter().any(|n| *n == route.name) {
            continue;
        }
        let dir = from.join(&route.name);
        let loads = match loads_from_replay(&dir) {
            Ok(l) => l,
            Err(e) => {
                println!("{}: no baseline replay ({e})", route.name);
                continue;
            }
        };
        let path = root.join(format!("tools/tapes/{}.cuts", route.name));
        if loads.len() < minimum {
            println!("{}: {} scene load(s), stays open loop", route.name, loads.len());
            let _ = std::fs::remove_file(&path);
            continue;
        }
        let mut cuts = route.cuts.clone().unwrap_or_default();
        cuts.loads = loads;
        std::fs::write(&path, cuts.render(&header(&disc)))?;
        println!("{}: {} scene loads -> {}", route.name, cuts.loads.len(), path.display());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_repositorys_pin_files_load_and_name_a_tape() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let routes = load(&root).unwrap();
        assert!(routes.len() > 40, "{} routes", routes.len());
        let journey: Vec<_> = routes.iter().filter(|r| r.journey.is_some()).map(|r| r.name.as_str()).collect();
        assert_eq!(journey, ["journey-kings", "journey-crossroads", "journey-false-knight", "journey-reload"]);
    }

    /// The journey routes whose open-loop tape never reaches the pins (the chained cards make it
    /// drift on emulator 0e87b3a): their closed loop judges them, nothing measures their frames.
    const OPEN_LOOP_NEVER_JUDGED: [&str; 2] = ["journey-false-knight", "journey-reload"];

    #[test]
    fn every_judged_route_pins_its_frame_time_and_its_tail() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for r in load(&root).unwrap() {
            if OPEN_LOOP_NEVER_JUDGED.contains(&r.name.as_str()) {
                assert!(r.frame_time.is_none() && r.long_frames.is_none(), "{} is not measured", r.name);
                continue;
            }
            let ft = r.frame_time.unwrap_or_else(|| panic!("{}.pins has no frame_time", r.name));
            assert!((100..=1000).contains(&ft), "{}.pins frame_time {ft}", r.name);
            assert!(r.long_frames.is_some(), "{}.pins has no long_frames", r.name);
        }
    }

    #[test]
    fn pin_files_parse() {
        let r = parse_pins(
            "x",
            "# c\ncard = a.mcd\nscene_gate = true\njourney = 2\npacing = 5\nframe_time = 231\nlong_frames = 1\nart_late = 3\nmissed_vblanks = 7\nHK_A = 7\nHK_B >= 2\n",
        )
        .unwrap();
        assert_eq!(r.card.as_deref(), Some("a.mcd"));
        assert!(r.scene_gate);
        assert_eq!((r.journey, r.pacing, r.art_late, r.missed_vblanks), (Some(2), 5, 3, 7));
        assert_eq!((r.frame_time, r.long_frames), (Some(231), Some(1)));
        assert_eq!(r.pins.len(), 2);
        assert!(!r.pins[0].at_least && r.pins[0].value == 7);
        assert!(r.pins[1].at_least && r.pins[1].value == 2);
        assert!(parse_pins("x", "bogus = 1").is_err());
        assert!(parse_pins("x", "HK_A = x").is_err());
    }

    #[test]
    fn loads_come_from_game_mode_spans() {
        let csv = "ticks,port1_polls,m,l\n0,0,2,0\n1,0,2,0\n2,3,1,1\n3,4,1,1\n4,9,2,1\n5,9,2,1\n6,14,1,2\n";
        let loads = loads_from_csv(csv, "m", "l").unwrap();
        assert_eq!(
            loads,
            vec![
                Load { start: 0, end: 3, loads_after: 1, pad: 0 },
                Load { start: 9, end: 14, loads_after: 2, pad: 0 }
            ]
        );
    }

    #[test]
    fn probes_give_each_loads_neutral_ticks() {
        let log = "x FAIL a/probe1: ram HK_INPUT_POLLS eq -1: value = [26 (@0x1)] [probe-polls-0]\n\
                   x FAIL a/probe2: ram HK_INPUT_POLLS eq -1: value = [9400 (@0x1)] [probe-polls-1]\n\
                   x FAIL a/r: ram HK_LOAD_LOG eq -1: value = [23 (@0x1)] [probe-resume-0]\n\
                   x FAIL a/r: ram HK_LOAD_LOG eq -1: value = [9399 (@0x1)] [probe-resume-1]\n";
        assert_eq!(parse_probes(log, 2).unwrap(), vec![3, 1]);
        assert!(parse_probes(log, 3).is_err());
    }

    #[test]
    fn a_held_button_is_added_to_the_polls_it_covers() {
        let mut bytes = b"PXITAPE2".to_vec();
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        for mask in [0u16, 0x8, 0] {
            bytes.extend_from_slice(&mask.to_le_bytes());
            bytes.extend_from_slice(&[0x80; 4]);
        }
        let tape = Tape::parse(&hold_buttons(&bytes, 1, 3, 4).unwrap()).unwrap();
        assert_eq!((0..3).map(|i| tape.mask(i)).collect::<Vec<_>>(), vec![0, 0xc, 4]);
        assert!(hold_buttons(&bytes, 2, 4, 4).is_err());
    }

    #[test]
    fn a_stall_repeats_one_sample_and_keeps_the_rest_in_order() {
        let mut bytes = b"PXITAPE2".to_vec();
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        for mask in [1u16, 2, 3] {
            bytes.extend_from_slice(&mask.to_le_bytes());
            bytes.extend_from_slice(&[0x80; 4]);
        }
        let tape = Tape::parse(&stall_sample(&bytes, 1, 2).unwrap()).unwrap();
        assert_eq!((0..tape.len()).map(|i| tape.mask(i)).collect::<Vec<_>>(), vec![1, 2, 2, 2, 3]);
        assert!(stall_sample(&bytes, 3, 2).is_err());
    }

    #[test]
    fn pads_are_rewritten_in_place() {
        let text = "# header\nload 21 24 1\nload 9280 9374 2 pad 5\nwait 5 HK_X eq 1\n";
        assert_eq!(set_pads(text, &[3, 0]), "# header\nload 21 24 1 pad 3\nload 9280 9374 2\nwait 5 HK_X eq 1\n");
    }

    #[test]
    fn cuts_round_trip() {
        let cuts = Cuts {
            loads: vec![Load { start: 4, end: 9, loads_after: 1, pad: 3 }],
            waits: vec![Wait { poll: 20, symbol: "HK_REGION_ID".into(), op: "eq".into(), value: 240 }],
        };
        assert_eq!(Cuts::parse(&cuts.render("baseline x\nsecond line")).unwrap(), cuts);
    }

    #[test]
    fn journey_plays_stretches_and_waits_through_loads() {
        let mut bytes = b"PXITAPE2".to_vec();
        bytes.extend_from_slice(&12u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        for i in 0..12u16 {
            bytes.extend_from_slice(&(if i == 4 { 32u16 } else { 0 }).to_le_bytes());
            bytes.extend_from_slice(&[0, 0, 0, 0]);
        }
        let tape = Tape::parse(&bytes).unwrap();
        let cuts = Cuts {
            loads: vec![Load { start: 5, end: 8, loads_after: 1, pad: 2 }],
            waits: vec![],
        };
        let route = parse_pins("r", "HK_A = 1\nHK_B >= 2\n").unwrap();
        let dir = std::env::temp_dir().join(format!("hk-routes-test-{}", std::process::id()));
        let setup = Setup {
            name: "r-sd",
            disc: Path::new("/d.cue"),
            symbols: Path::new("/m.map"),
            card: None,
            snapshot: true,
            probe: false,
            lock: false,
        };
        let file = write_journey(&route, &tape, &cuts, &setup, &dir).unwrap();
        let toml = std::fs::read_to_string(file).unwrap();
        assert!(toml.contains("polls 0..5") && toml.contains("polls 8..12"));
        assert!(toml.contains("tape_rebase = true"));
        // The load's neutral ticks are played once, between the load and the stretch that follows it.
        assert_eq!(toml.matches("wait = 2").count(), 1);
        assert!(toml.find("wait = 2").unwrap() > toml.find("polls 0..5").unwrap());
        assert!(toml.find("wait = 2").unwrap() < toml.find("polls 8..12").unwrap());
        assert!(toml.contains("press = [\"right\"]"));
        assert!(toml.contains("sym = \"HK_SCENE_LOADS\"\nop = \"ge\"\nvalue = 1"));
        assert!(toml.contains("op = \"ge\"\nvalue = 2"));
        assert!(toml.contains("action = \"snapshot\""));
        assert!(dir.join("r-sd-seg0.pxtape").is_file() && dir.join("r-sd-seg1.pxtape").is_file());
        assert!(!toml.contains("l3"), "a measuring run must not ask for the route lock");
        // A playing run holds L3 through the load and until gameplay is published.
        let locked = write_journey(&route, &tape, &cuts, &Setup { lock: true, ..setup }, &dir).unwrap();
        let toml = std::fs::read_to_string(locked).unwrap();
        assert!(toml.contains("press = [\"right\", \"l3\"]"));
        assert!(toml.contains("press = [\"l3\"]"));
        assert!(toml.contains("sym = \"HK_GAME_MODE\"\nop = \"eq\"\nvalue = 1"));
        assert!(!toml.contains("wait_until"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
