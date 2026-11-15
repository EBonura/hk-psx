//! `hk-walkplan room PREV SCENE NEXT`: the authoring loop for one room, closed over the disc.
//!
//! The planner (`search`) knows the room's collision and the guest's own `Player::step`, nothing else: not foes,
//! not a hit that knocks the Knight off a take-off. So a room is written one hop at a time. Plan the first hop from
//! where the Knight really is, run the walk so far on the disc, read where the hop landed (a `probe` after each),
//! and plan the next hop from there. A hop that lands somewhere else stays in the script (the gate is deterministic,
//! so the prefix replays exactly) and the next plan starts from the real place. The result is an ordinary walk
//! script, `tools/walks/NAME.walk`, that the walk compiler turns into a journey like any other, and its room alone
//! (no card, no probes, the exit waited for by the spawn of the next room) as `NAME.frag`, which a chain `include`s after the
//! previous room's fragment so SOUL and masks carry from room to room (a card does not save SOUL).
//!
//! ```text
//! hk-walkplan room PREV SCENE NEXT --lib DIR [--name N] [--between 'line;line'] [--open X0,Y0,X1,Y1]...
//!                  [--spawn X Y REGION] [--goal X Y] [--tail 'line;line'] [--swing PX PY LO HI LX LY] [--tol T] [--max N]
//!                  [--pre FILE] [--card FILE] [--fireball N] [--work DIR]
//! ```
//!
//! `--card cold` plays no card and no Continue: the walk starts from a cold boot and `--pre` is the whole chain so far, so the
//! plan sees the chain's own SOUL, masks, Geo and dead foes (one round replays the chain, a few minutes). `HK_WALKPLAN_FINE`
//! (world units, for example 0.05) lets a room whose only way on is a fragile jump be planned: such a jump costs more, so
//! it is taken only when no steadier way exists. `HK_WALKPLAN_SOURCES=x0,y0,x1,y1` prints the scene object that owns each
//! collision edge touching the box.
//!
//! `PREV` and `NEXT` are scene names: the Knight arrives from `PREV` (its gate's spawn is where the run starts) and
//! leaves by the gate that leads to `NEXT`. `--lib` is a directory with the disc and its `game.map` (the library disc
//! works). Without `--card` a card is seeded in `SCENE` at the arrival spawn with `tools/seed_card.py` (the existing
//! fixture tool); with it, the card must already hold the run's state. `--between` is a walk fragment played after
//! every hop that is not the last (`{LO}` and `{HI}` stand for the ends of the floor the hop should land on, for `retreat`): a reaction (`guard 2`, `fight 3`, `heal`) or one of the patterns in
//! `tools/walks/patterns/`. `--pre` is a fragment played once before the first hop. The runner needs
//! `HK_PSX_GATE` (the expression-capable gate) in the environment, as every walk does, and the host tool
//! `host/target/release/hk-psx-build` (or `HK_PSX_BUILD`).

use std::path::{Path, PathBuf};
use std::process::Command;

use hk_sim::ONE;

use crate::hop::{self, Checkpoint, Floor, JumpSpec};
use crate::{
    floor_span, load_edges, load_hazards, search, set_banned, set_goal_box, set_hazards, standing,
    walk_lines, Ban, Edge,
};

/// A wide hop is run again (at most `RETRIES` passes in `RETRY_TICKS` ticks) until the Knight stands where the plan landed
/// him or anywhere on that floor; a tight one (a landing or an apex without its margin, `hop.rs`) gets `TIGHT_RETRIES` passes in
/// `TIGHT_RETRY_TICKS`, a guard on its foes and a heal on every pass.
const RETRIES: u32 = 3;
/// A room that bans more jumps than this is not planned: something other than the jumps is wrong.
const MAX_BANS: usize = 24;
const RETRY_TICKS: u32 = 900;
const TIGHT_RETRIES: u32 = 6;
const TIGHT_RETRY_TICKS: u32 = 1800;
/// How near the planned landing counts as landed (a little wider than `LAND_X`/`LAND_Y`, so a hop the planner accepted is never
/// repeated), and how far past the ends of the landing floor a standing Knight still counts as on it.
const RETRY_X: f64 = 0.9;
const RETRY_Y: f64 = 0.3;
const FLOOR_SLACK: f64 = 0.05;

/// True when a hop from (x, y) to (ex, ey) moves the Knight out of the retry band, so the loop's first test fails.
fn leaves_band(x: f64, y: f64, ex: f64, ey: f64) -> bool {
    (x - ex).abs() > RETRY_X + 0.3 || (y - ey).abs() > RETRY_Y + 0.1
}

/// One hop of the script so far: where its lines sit in `body`, what its loop waits for and what it is.
struct Hop {
    start: usize,
    end: usize,
    /// The landing a retry loop waits for (None: not wrapped).
    retry: Option<(f64, f64)>,
    spec: Option<JumpSpec>,
    /// The floor the Knight stands on before the hop: the bounds its `clear` fights inside (None: strike in place).
    prev_span: Option<(f64, f64)>,
    /// The hazard respawn markers a miss of this hop can send him to: the one before it and the one after.
    markers: Vec<(f64, f64)>,
    /// The marker after the hop.
    marker_after: (f64, f64),
}

/// How a hop's lines are wrapped: the shipped fragment loops until landed; the play version the planner runs on the disc
/// unrolls the same passes as guarded blocks, so a hop that never lands ends at a probe (and is banned) instead of aborting.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Wrap {
    Ship,
    Play,
}

/// The gate expression that says the Knight landed: at the planned point or anywhere on the landing floor, at its height.
fn landed(q: &impl Fn(f64) -> i64, (ex, ey): (f64, f64), floor: &Floor) -> String {
    format!(
        "(abs(i32(HK_PLAYER_X) - {}) <= {} || (i32(HK_PLAYER_X) >= {} && i32(HK_PLAYER_X) <= {})) && abs(i32(HK_PLAYER_Y) - {}) <= {}",
        q(ex),
        q(RETRY_X),
        q(floor.lo - FLOOR_SLACK),
        q(floor.hi + FLOOR_SLACK),
        q(ey),
        q(RETRY_Y),
    )
}

/// `body` with every hop that has a landing to wait for wrapped (see `Wrap`): a `clear` while a foe is in the hop's box, then
/// the passes, each starting with the recovery blocks (the way back from a floor a miss can end on). The planner plays the
/// wrapped play version, so what is judged on the disc is what the fragment runs.
fn retrying(
    body: &[String],
    hops: &[Hop],
    name: &str,
    q: impl Fn(f64) -> i64,
    room: &hop::Room,
    wrap: Wrap,
) -> Vec<String> {
    let mut out = body.to_vec();
    for (k, hop) in hops.iter().enumerate().rev() {
        let Some((ex, ey)) = hop.retry else { continue };
        let Ok(stand) = standing(room.edges, ex, ey) else { continue };
        let floor = Floor::of(room.edges, stand);
        let hop_name = format!("{name}-h{:03}", k + 1);
        let tight = hop.spec.is_some_and(|s| s.tight.any());
        let landed = landed(&q, (ex, ey), &floor);
        let recovery = hop
            .spec
            .map(|spec| hop::recoveries(room, &hop_name, &spec, floor, &hop.markers, &q))
            .unwrap_or_default();
        let clear = match hop.prev_span {
            Some((lo, hi)) => format!("clear {hop_name}-arc {ex:.3} {ey:.3} {lo:.2} {hi:.2}"),
            None => format!("clear {hop_name}-arc {ex:.3} {ey:.3}"),
        };
        let mut pass: Vec<String> = Vec::new();
        pass.extend(recovery);
        if tight {
            // A tight hop meets its foes again on every pass: clear the box and heal before each try.
            pass.push(clear.replacen("-arc", "-tight", 1));
            pass.push("heal".to_string());
        }
        pass.extend(body[hop.start..hop.end].iter().cloned());
        let mut block: Vec<String> = vec![clear];
        match wrap {
            Wrap::Ship => {
                let (max, ticks) = if tight { (TIGHT_RETRIES, TIGHT_RETRY_TICKS) } else { (RETRIES, RETRY_TICKS) };
                block.push(format!("loop {hop_name} until expr {landed} max {max} timeout {ticks}"));
                block.extend(pass);
                block.push("end".to_string());
            }
            Wrap::Play => {
                for attempt in 1..=RETRIES {
                    block.push(format!("if expr !({landed})"));
                    // Block names are the journey's: each unrolled pass has its own.
                    block.extend(pass.iter().map(|l| l.replacen("-tight", &format!("-tight{attempt}"), 1)));
                    block.push("end".to_string());
                }
            }
        }
        out.splice(hop.start..hop.end, block);
    }
    out
}

/// A swing at a vine from a hanging platform: the Knight stands at `stand` on a platform that reaches `span`, one cut of
/// the vine drops him onto the Activated Plat at `land`. The planner does not search it (the way down exists only once the
/// vine is cut); it writes it as one more guarded hop of the room.
#[derive(Clone, Copy, Debug)]
struct Swing {
    stand: (f64, f64),
    span: (f64, f64),
    land: (f64, f64),
}

/// Passes of a swing: a hit in the swing's recoil or a Mosquito's knock leaves the vine uncut, the Knight on the platform.
const SWING_PASSES: u32 = 3;
/// How near the platform's height counts as still on it, and how near the Activated Plat's as landed on it (world units).
const SWING_ON_Y: f64 = 0.5;
const SWING_LAND_X: f64 = 1.5;
const SWING_LAND_Y: f64 = 0.8;

/// The lines of a swing hop, the same vocabulary as a jump hop: a `clear` while a foe is in reach of the platform, a heal,
/// then passes that each run only while the Knight is still on the platform and has not landed below (so a pass after a
/// good cut costs no ticks), every pass clearing and healing first, and a wait on the landing as the judge.
fn swing_lines(name: &str, swing: &Swing, q: impl Fn(f64) -> i64) -> Vec<String> {
    let Swing { stand: (px, py), span: (lo, hi), land: (lx, ly) } = *swing;
    let landed = format!(
        "abs(i32(HK_PLAYER_X) - {}) <= {} && abs(i32(HK_PLAYER_Y) - {}) <= {}",
        q(lx),
        q(SWING_LAND_X),
        q(ly),
        q(SWING_LAND_Y)
    );
    let on_platform = format!("abs(i32(HK_PLAYER_Y) - {}) <= {}", q(py), q(SWING_ON_Y));
    let mut out = vec![format!("clear {name}-swing-arc {px:.3} {py:.3} {lo:.2} {hi:.2}"), "heal".to_string()];
    for pass in 1..=SWING_PASSES {
        out.push(format!("if expr !({landed}) && {on_platform}"));
        if pass > 1 {
            out.push(format!("clear {name}-swing{pass}-arc {px:.3} {py:.3} {lo:.2} {hi:.2}"));
            out.push("heal".to_string());
        }
        out.push(format!("walk {px:.2}"));
        out.push("hold right for 2".to_string());
        out.push("press square for 3 rest 40".to_string());
        out.push("end".to_string());
    }
    out.push(format!("wait until expr {landed} timeout 400"));
    out
}

/// A hop that ends this far (world units) from where its plan said counts as landed as planned.
const LAND_X: f64 = 0.7;
const LAND_Y: f64 = 0.35;

struct Options {
    prev: String,
    scene: String,
    next: String,
    lib: PathBuf,
    name: Option<String>,
    between: Vec<String>,
    open: Vec<[f64; 4]>,
    spawn: Option<(f64, f64, i64)>,
    goal: Option<(f64, f64)>,
    tail: Option<Vec<String>>,
    swing: Option<Swing>,
    tolerance: f64,
    max_hops: usize,
    pre: Option<PathBuf>,
    card: Option<String>,
    fireball: Option<u32>,
    retry: bool,
    work: PathBuf,
}

fn usage() -> String {
    "usage: hk-walkplan room PREV SCENE NEXT --lib DIR [--name N] [--between 'line;line'] [--open X0,Y0,X1,Y1]... \
     [--spawn X Y REGION] [--goal X Y] [--tail 'line;line'] [--tol T] [--max N] [--pre FILE] [--card FILE] \
     [--fireball N] [--work DIR]"
        .to_string()
}

fn lines(text: &str) -> Vec<String> {
    text.split(';')
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

fn parse(args: &[String]) -> Result<Options, String> {
    let [prev, scene, next, rest @ ..] = args else {
        return Err(usage());
    };
    let mut o = Options {
        prev: prev.clone(),
        scene: scene.clone(),
        next: next.clone(),
        lib: PathBuf::new(),
        name: None,
        between: Vec::new(),
        open: Vec::new(),
        spawn: None,
        goal: None,
        tail: None,
        swing: None,
        tolerance: 0.6,
        max_hops: 40,
        pre: None,
        card: None,
        fireball: None,
        retry: true,
        work: std::env::temp_dir().join("hk-walkplan"),
    };
    let mut it = rest.iter();
    let number = |it: &mut std::slice::Iter<String>| -> Result<f64, String> {
        it.next().and_then(|v| v.parse().ok()).ok_or_else(usage)
    };
    while let Some(flag) = it.next() {
        match flag.as_str() {
            "--lib" => o.lib = PathBuf::from(it.next().ok_or_else(usage)?),
            "--name" => o.name = Some(it.next().ok_or_else(usage)?.clone()),
            "--between" => o.between = lines(it.next().ok_or_else(usage)?),
            "--tail" => o.tail = Some(lines(it.next().ok_or_else(usage)?)),
            "--open" => {
                let b: Vec<f64> = it
                    .next()
                    .ok_or_else(usage)?
                    .split(',')
                    .filter_map(|v| v.parse().ok())
                    .collect();
                o.open.push(<[f64; 4]>::try_from(b).map_err(|_| usage())?);
            }
            "--spawn" => {
                let (x, y) = (number(&mut it)?, number(&mut it)?);
                o.spawn = Some((x, y, number(&mut it)? as i64));
            }
            "--goal" => o.goal = Some((number(&mut it)?, number(&mut it)?)),
            "--swing" => {
                let mut n = [0.0; 6];
                for v in &mut n {
                    *v = number(&mut it)?;
                }
                o.swing = Some(Swing { stand: (n[0], n[1]), span: (n[2], n[3]), land: (n[4], n[5]) });
            }
            "--tol" => o.tolerance = number(&mut it)?,
            "--max" => o.max_hops = number(&mut it)? as usize,
            "--pre" => o.pre = Some(PathBuf::from(it.next().ok_or_else(usage)?)),
            "--card" => o.card = Some(it.next().ok_or_else(usage)?.clone()),
            "--fireball" => o.fireball = Some(number(&mut it)? as u32),
            "--no-retry" => o.retry = false,
            "--work" => o.work = PathBuf::from(it.next().ok_or_else(usage)?),
            _ => return Err(usage()),
        }
    }
    if o.lib.as_os_str().is_empty() {
        return Err(usage());
    }
    Ok(o)
}

/// One scene of `data/regions.json`: its id and the gates the cook resolved.
struct Scene {
    id: i64,
    gates: Vec<serde_json::Value>,
}

fn scene(root: &Path, name: &str) -> Result<Scene, String> {
    let path = root.join("data/regions.json");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let found = json["scenes"]
        .as_array()
        .and_then(|all| all.iter().find(|s| s["scene_name"] == name))
        .ok_or_else(|| format!("no scene named {name}"))?;
    Ok(Scene {
        id: found["scene_id"].as_i64().ok_or("scene without an id")?,
        gates: found["resolved_gates"]
            .as_array()
            .cloned()
            .unwrap_or_default(),
    })
}

fn numbers(value: &serde_json::Value) -> Vec<f64> {
    value
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_f64()).collect())
        .unwrap_or_default()
}

/// The hazard respawn markers of a scene (`checkpoints` of its regions in `data/regions.json`), each once.
fn load_checkpoints(root: &Path, name: &str) -> Result<Vec<Checkpoint>, String> {
    let path = root.join("data/regions.json");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut out: Vec<Checkpoint> = Vec::new();
    for region in json["regions"].as_array().into_iter().flatten().filter(|r| r["scene_name"] == name) {
        for c in region["checkpoints"].as_array().into_iter().flatten() {
            let (b, s) = (numbers(&c["bounds"]), numbers(&c["spawn"]));
            if let ([x0, y0, x1, y1], [sx, sy]) = (b.as_slice(), s.as_slice()) {
                let marker = Checkpoint { bounds: [*x0, *y0, *x1, *y1], spawn: (*sx, *sy) };
                if !out.contains(&marker) {
                    out.push(marker);
                }
            }
        }
    }
    Ok(out)
}

/// The first hop of a plan.
struct First {
    lines: Vec<String>,
    /// Where it should end: x, and y when the plan knows it.
    expect: Option<(f64, Option<f64>)>,
    /// Whether it is the last one.
    last: bool,
    /// How many lines of the plan it took.
    used: usize,
    /// The jump it is, when it is one.
    spec: Option<JumpSpec>,
}

/// The first hop of a plan: its lines, where it should end (x, and y when the plan knows it), and whether it is the
/// last one. A jump's landing is waited for by `HK_WALK_READY` (never a timeout, which would abort the run).
fn first_hop(plan: &[String]) -> First {
    let (mut group, mut expect, mut spec) = (Vec::new(), None, None);
    for (n, line) in plan.iter().enumerate() {
        let code = line.split('#').next().unwrap_or("");
        let t: Vec<&str> = code.split_whitespace().collect();
        let last = n + 1 == plan.len();
        match t.as_slice() {
            ["wait", "until", "y", "between", lo, hi, ..] => {
                let x = line
                    .split("land at x ")
                    .nth(1)
                    .and_then(|r| r.split_whitespace().next())
                    .and_then(|r| r.parse::<f64>().ok());
                if let (Some(x), Ok(lo), Ok(hi)) = (x, lo.parse::<f64>(), hi.parse::<f64>()) {
                    expect = Some((x, Some((lo + hi) / 2.0)));
                }
                spec = JumpSpec::parse(line);
                continue;
            }
            ["drop", x, y] => {
                expect = x
                    .parse()
                    .ok()
                    .zip(y.parse().ok())
                    .map(|(x, y)| (x, Some(y)));
                group.push(line.clone());
                return First { lines: group, expect, last, used: n + 1, spec };
            }
            ["wait", "until", "HK_WALK_READY", ..] => {
                group.push(line.clone());
                return First { lines: group, expect, last, used: n + 1, spec };
            }
            _ => group.push(line.clone()),
        }
    }
    if let Some(x) = group.iter().rev().find_map(|l| {
        let t: Vec<&str> = l
            .split('#')
            .next()
            .unwrap_or("")
            .split_whitespace()
            .collect();
        (t.first() == Some(&"walk"))
            .then(|| t.get(1).and_then(|v| v.parse::<f64>().ok()))
            .flatten()
    }) {
        expect = Some((x, None));
    }
    First { lines: group, expect, last: true, used: plan.len(), spec }
}

/// What the probe after a hop read: the Knight's place and the words that say how it went.
#[derive(Debug)]
struct Probe {
    x: f64,
    y: f64,
    health: i64,
    region: i64,
    loads: i64,
    deaths: i64,
}

/// The probes of a gate log: `FAIL  journey/checkpoint: ram WORD eq N: value = [V (@addr)] [probe NAME]`, one per word.
fn probes(log: &str) -> Vec<(String, Vec<(String, i64)>)> {
    let mut out: Vec<(String, Vec<(String, i64)>)> = Vec::new();
    for line in log.lines() {
        let Some(at) = line.find("[probe ") else {
            continue;
        };
        let word = line[at + 7..].trim_end_matches(']').to_string();
        let Some(slash) = line.find('/') else {
            continue;
        };
        let checkpoint: String = line[slash + 1..]
            .chars()
            .take_while(|c| *c != ':')
            .collect();
        let Some(v) = line
            .split("value = [")
            .nth(1)
            .and_then(|r| r.split_whitespace().next())
        else {
            continue;
        };
        let Ok(value) = v.parse::<i64>() else {
            continue;
        };
        match out.last_mut() {
            Some((name, words)) if *name == checkpoint => words.push((word, value)),
            _ => out.push((checkpoint, vec![(word, value)])),
        }
    }
    out
}

fn probe_of(words: &[(String, i64)]) -> Option<Probe> {
    let get = |k: &str| words.iter().find(|(w, _)| w == k).map(|(_, v)| *v);
    Some(Probe {
        x: get("x")? as f64 / f64::from(ONE),
        y: get("y")? as f64 / f64::from(ONE),
        health: get("HK_HEALTH")?,
        region: get("HK_REGION_ID")?,
        loads: get("HK_SCENE_LOADS")?,
        deaths: get("HK_DEATHS")?,
    })
}

/// Compile `script` against `lib` and play it, returning the gate's log.
fn play(root: &Path, o: &Options, script: &Path, name: &str) -> Result<String, String> {
    std::fs::create_dir_all(&o.work).map_err(|e| format!("{}: {e}", o.work.display()))?;
    let toml = o.work.join(format!("{name}.toml"));
    let builder = std::env::var_os("HK_PSX_BUILD").map_or_else(
        || root.join("host/target/release/hk-psx-build"),
        PathBuf::from,
    );
    let built = Command::new(&builder)
        .arg("walk")
        .arg(script)
        .arg(&o.lib)
        .arg(&toml)
        .output()
        .map_err(|e| format!("{}: {e}", builder.display()))?;
    if !built.status.success() {
        return Err(format!(
            "the walk did not compile: {}",
            String::from_utf8_lossy(&built.stderr)
        ));
    }
    let gate = std::env::var_os("HK_PSX_GATE")
        .ok_or("HK_PSX_GATE is not set (the expression-capable gate)")?;
    let out = Command::new(gate)
        .args(["run"])
        .arg(&toml)
        .args(["--no-hw", "--out"])
        .arg(o.work.join(name))
        .current_dir(root)
        .output()
        .map_err(|e| format!("gate: {e}"))?;
    Ok(format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    ))
}

pub fn run(args: &[String]) -> Result<(), String> {
    let o = parse(args)?;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let (prev, cur, next) = (
        scene(&root, &o.prev)?,
        scene(&root, &o.scene)?,
        scene(&root, &o.next)?,
    );
    // Where the run starts: the arrival spawn of the gate in PREV that leads here.
    let (sx, sy, region) = match o.spawn {
        Some(s) => s,
        None => {
            let g = prev
                .gates
                .iter()
                .find(|g| g["target_scene"].as_i64() == Some(cur.id))
                .ok_or_else(|| format!("{} has no gate to {}", o.prev, o.scene))?;
            let spawn = numbers(&g["spawn"]);
            (
                spawn[0],
                spawn[1],
                g["target_region"]
                    .as_i64()
                    .ok_or("gate without a target region")?,
            )
        }
    };
    // Where it ends: the gate in SCENE that leads to NEXT.
    let exit = cur
        .gates
        .iter()
        .find(|g| g["target_scene"].as_i64() == Some(next.id))
        .ok_or_else(|| format!("{} has no gate to {}", o.scene, o.next))?;
    let (side, bounds) = (exit["side"].as_i64().unwrap_or(0), numbers(&exit["bounds"]));
    let (gx, gy) = o.goal.unwrap_or_else(|| {
        let y = if side == 4 { bounds[3] } else { bounds[1] } + 1.39;
        let x = match side {
            1 => bounds[2] + 0.6,
            2 => bounds[0] - 0.6,
            _ => (bounds[0] + bounds[2]) / 2.0,
        };
        (x, y)
    });
    eprintln!(
        "room {}: spawn ({sx}, {sy}) region {region}, goal ({gx:.2}, {gy:.2}), exit side {side}",
        o.scene
    );

    let name = o.name.clone().unwrap_or_else(|| {
        format!(
            "x-{}",
            o.scene
                .replace("Crossroads_", "c")
                .replace("Fungus1_", "f")
                .replace('_', "")
                .to_lowercase()
        )
    });
    let card = match &o.card {
        Some(card) => card.clone(),
        None => {
            let card = format!("{name}.mcd");
            let mut seed = Command::new("python3");
            seed.arg(root.join("tools/seed_card.py"))
                .args(["--from-card"])
                .arg(root.join("tools/cards/crossroads-47-bench.mcd"))
                .args(["--output"])
                .arg(root.join("tools/cards").join(&card))
                .args([
                    "--scene",
                    &cur.id.to_string(),
                    "--region",
                    &region.to_string(),
                ])
                .args([
                    "--x",
                    &sx.to_string(),
                    "--y",
                    &sy.to_string(),
                    "--facing",
                    "1",
                ]);
            if let Some(level) = o.fireball {
                seed.args(["--fireball", &level.to_string()]);
            }
            let done = seed.output().map_err(|e| format!("seed_card.py: {e}"))?;
            if !done.status.success() {
                return Err(format!(
                    "seed_card.py: {}",
                    String::from_utf8_lossy(&done.stderr)
                ));
            }
            card
        }
    };
    // `--card cold`: the run starts from a cold boot and `--pre` is the whole chain so far (no card, no continue).
    let header: Vec<String> = if card == "cold" {
        vec![format!("name {name}"), "max_ticks 600000".to_string()]
    } else {
        vec![
            format!("name {name}"),
            "max_ticks 60000".to_string(),
            format!("card {card}"),
            "wait 300".to_string(),
            "press cross".to_string(),
            "wait 40".to_string(),
            "press cross".to_string(),
            "wait until HK_GAME_MODE == 1 timeout 1200".to_string(),
        ]
    };
    let pre: Vec<String> = match &o.pre {
        Some(file) => std::fs::read_to_string(file)
            .map_err(|e| format!("{}: {e}", file.display()))?
            .lines()
            .map(String::from)
            .collect(),
        None => Vec::new(),
    };
    // The exit is waited for by where the Knight appears in the next room (the exit gate's spawn), not by a region or a
    // load count, so the fragment chains after any other: a region id changes as a room streams, and a load count is
    // absolute.
    let spawn = numbers(&exit["spawn"]);
    let q = |v: f64| (v * f64::from(ONE)).round() as i64;
    let reached = format!(
        "expr abs(i32(HK_PLAYER_X) - {}) <= {} && abs(i32(HK_PLAYER_Y) - {}) <= {}",
        q(*spawn.first().ok_or("exit gate without a spawn")?),
        5 * ONE,
        q(*spawn.get(1).ok_or("exit gate without a spawn")?),
        // A gate above or below drops the Knight to a floor a few units from the spawn point.
        if side == 3 || side == 4 {
            6 * ONE
        } else {
            3 * ONE
        }
    );
    let tail = o.tail.clone().unwrap_or_else(|| match side {
        1 => vec![format!("hold left until {reached} timeout 900")],
        2 => vec![format!("hold right until {reached} timeout 900")],
        _ => vec![format!("wait until {reached} timeout 600")],
    });
    let swing = o.swing.map(|s| swing_lines(&name, &s, q)).unwrap_or_default();
    let tail: Vec<String> = tail
        .into_iter()
        .chain(swing)
        .chain([
            "wait until HK_GAME_MODE == 1 timeout 1800".to_string(),
            "probe end x y HK_REGION_ID HK_SCENE_LOADS HK_HEALTH HK_DEATHS".to_string(),
        ])
        .collect();

    // A vertical exit (up or down) is taken by touching its trigger box.
    set_goal_box((side == 3 || side == 4).then(|| [bounds[0], bounds[1], bounds[2], bounds[3]]));
    let edges: Vec<Edge> = load_edges(&root, &o.scene, &o.open)?;
    // HK_WALKPLAN_NO_HAZARDS=1: plan through hazard boxes too (to see whether a room is solvable at all).
    if std::env::var("HK_WALKPLAN_NO_HAZARDS").is_ok_and(|v| v == "1") {
        set_hazards(Vec::new());
    } else {
        set_hazards(load_hazards(&root, &o.scene)?);
    }
    let script_path = root.join("tools/walks").join(format!("{name}.walk"));
    // The script so far: every hop played with a probe after it, where each ended, and where it should have.
    let (mut body, mut ends): (Vec<String>, Vec<usize>) = (Vec::new(), Vec::new());
    let mut expect: Vec<Option<(f64, Option<f64>)>> = Vec::new();
    // Where each hop's lines sit in `body`, and what wraps it.
    let mut hops: Vec<Hop> = Vec::new();
    // The jumps that missed on the disc even with their passes and recoveries: the plan does not take them again.
    let mut bans: Vec<Ban> = Vec::new();
    let checkpoints = load_checkpoints(&root, &o.scene)?;
    let room = hop::Room { edges: &edges, checkpoints: &checkpoints, arrival: (sx, sy) };
    let (mut cx, mut cy) = (sx, sy);
    let mut done = false;
    // Where the room starts, and whether a hop has already taken the exit (a vertical exit is taken by touching its
    // trigger, which can come before the planned goal).
    body.push("probe start x y HK_HEALTH HK_REGION_ID HK_SCENE_LOADS HK_DEATHS".to_string());
    let mut exited = false;
    for round in 1..=o.max_hops {
        // Plan the rest of the room from where the Knight really is, and play all of it: hops that land as planned
        // cost nothing more than their own ticks, the first that does not stops the round and the next plan starts
        // from where it ended.
        let first_new = expect.len();
        let mut finished = exited;
        if !exited {
            let start = standing(&edges, cx, cy)?;
            let Some(plan) = search(&edges, start, (gx, gy), o.tolerance, true) else {
                eprintln!("no plan from ({cx:.2}, {cy:.2})");
                break;
            };
            let mut rest: Vec<String> = walk_lines(&plan, &edges, &start)
                .into_iter()
                .filter(|l| !l.starts_with("at "))
                .collect();
            loop {
                let First { lines: mut hop, expect: mut exp, last, used, spec } = first_hop(&rest);
                rest.drain(..used);
                // A bottom exit is a gap in the floor: the last hop walks off its edge and keeps the direction held
                // until the next room has loaded, since a `drop` would wait for a floor that is not there.
                if last && side == 4 {
                    if let Some(at) = hop.iter().position(|l| l.starts_with("drop ")) {
                        let drop_x = hop[at].split_whitespace().nth(1).and_then(|v| v.parse::<f64>().ok());
                        let from_x = hop[..at].iter().rev().find_map(|l| {
                            l.split('#').next()?.strip_prefix("walk ")?.trim().parse::<f64>().ok()
                        });
                        if let Some((dx, fx)) = drop_x.zip(from_x) {
                            let way = if dx < fx { "left" } else { "right" };
                            hop.truncate(at);
                            hop.push(format!("hold {way} until {reached} timeout 900"));
                            exp = None;
                        }
                    }
                }
                let k = expect.len() + 1;
                // A jump that lands as planned is the common case; one that falls back (a take-off a tenth of a unit
                // off, a hit mid-air) is run again until the Knight stands where the plan said, instead of leaving
                // every later hop to start from the wrong ledge. The loop is tested before its first pass, so only a
                // hop that really moves the Knight is wrapped.
                let (hx, hy) = match expect.last() {
                    Some(Some((px, py))) if expect.len() > first_new => (*px, py.unwrap_or(cy)),
                    _ => (cx, cy),
                };
                let retry = match exp {
                    Some((ex, Some(ey))) if o.retry && !hop.iter().any(|l| l.starts_with("drop ")) => {
                        leaves_band(hx, hy, ex, ey).then_some((ex, ey))
                    }
                    _ => None,
                };
                let hop_start = body.len();
                body.extend(hop.iter().map(|l| l.replace("timeout 240", "timeout 400")));
                let marker = hops.last().map_or(room.arrival, |h| h.marker_after);
                let marker_after = spec.map_or(marker, |s| hop::marker_after(&room, &edges, &s, marker));
                let mut markers = vec![marker];
                if marker_after != marker {
                    markers.push(marker_after);
                }
                let prev_span = expect
                    .last()
                    .copied()
                    .flatten()
                    .and_then(|(px, py)| floor_span(&edges, px, py.unwrap_or(cy)).ok());
                hops.push(Hop { start: hop_start, end: body.len(), retry, spec, prev_span, markers, marker_after });
                body.push(format!(
                    "probe p{k:03} x y HK_HEALTH HK_REGION_ID HK_SCENE_LOADS HK_DEATHS"
                ));
                if !last {
                    // `{LO}` and `{HI}`: the floor under the Knight where the hop should land (a retreat stays on it).
                    let (ex, ey) = exp.map_or((cx, cy), |(x, y)| (x, y.unwrap_or(cy)));
                    let (lo, hi) = floor_span(&edges, ex, ey).unwrap_or((ex - 1.0, ex + 1.0));
                    body.extend(o.between.iter().map(|l| {
                        l.replace("{LO}", &format!("{lo:.2}"))
                            .replace("{HI}", &format!("{hi:.2}"))
                    }));
                }
                expect.push(exp);
                ends.push(body.len());
                finished = last;
                if last || rest.is_empty() {
                    break;
                }
            }
        }
        // The fragment: this room from its arrival to its exit (what a chain includes); the walk is a card start plus it.
        let mut frag: Vec<String> = vec![format!("at {sx:.2} {sy:.2}")];
        frag.extend(retrying(&body, &hops, &name, q, &room, Wrap::Play));
        if finished {
            frag.extend(tail.iter().cloned());
        }
        // `--pre` is whatever got the Knight to the arrival (another room's fragment, a fight): it is in the walk, not
        // in the fragment.
        let walk_text: Vec<String> = header
            .iter()
            .chain(pre.iter())
            .chain(frag.iter())
            .cloned()
            .collect();
        std::fs::write(&script_path, walk_text.join("\n") + "\n")
            .map_err(|e| format!("{}: {e}", script_path.display()))?;
        if finished {
            let path = script_path.with_extension("frag");
            let mut chained = vec![format!("at {sx:.2} {sy:.2}")];
            chained.extend(retrying(&body, &hops, &name, q, &room, Wrap::Ship));
            chained.extend(tail.iter().cloned());
            let kept: Vec<&str> = chained
                .iter()
                .map(String::as_str)
                .filter(|l| !l.starts_with("probe "))
                .collect();
            std::fs::write(&path, kept.join("\n") + "\n")
                .map_err(|e| format!("{}: {e}", path.display()))?;
        }
        let log = play(&root, &o, &script_path, &name)?;
        let all = probes(&log);
        let reads: Vec<Probe> = all
            .iter()
            .filter(|(n, _)| n.starts_with('p'))
            .filter_map(|(_, w)| probe_of(w))
            .collect();
        if exited {
            done = all
                .iter()
                .any(|(n, w)| n == "end" && probe_of(w).is_some_and(|p| p.deaths == 0));
            eprintln!(
                "{}",
                all.iter()
                    .find(|(n, _)| n == "end")
                    .map_or("no end probe".to_string(), |(_, w)| format!("end {w:?}"))
            );
            break;
        }
        let base_loads = all
            .iter()
            .find(|(n, _)| n == "start")
            .and_then(|(_, w)| probe_of(w))
            .map(|p| p.loads);
        let mut stopped = false;
        for j in first_new..expect.len() {
            let Some(read) = reads.get(j) else {
                eprintln!(
                    "the run ended before probe {}:\n{}",
                    j + 1,
                    log.lines()
                        .rev()
                        .take(8)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect::<Vec<_>>()
                        .join("\n")
                );
                // The hop the run stopped in (a walk to its take-off that never arrived, a hold that timed out) is no way on:
                // ban the jump and plan again from where the hop before it ended.
                if j == reads.len() && o.retry && hops[j].spec.is_some() && bans.len() < MAX_BANS {
                    let spec = hops[j].spec.expect("a jump");
                    if let Ok(from) = standing(&edges, spec.from.0, spec.from.1) {
                        bans.push(Ban { cell: crate::cell(&from), dir: spec.dir, held: spec.held, steer: spec.steer });
                        set_banned(bans.clone());
                        eprintln!("hop {} banned (the run stopped in it): {spec:?}", j + 1);
                        body.truncate(hops[j].start);
                        ends.truncate(j);
                        expect.truncate(j);
                        hops.truncate(j);
                        stopped = true;
                    }
                }
                break;
            };
            let exp = expect[j];
            // Landed as planned: near the planned point, or anywhere on the planned landing floor (every hop starts with a
            // walk to its own take-off, so where on the floor he stands does not matter).
            let ok = exp.is_none_or(|(ex, ey)| {
                let near = (read.x - ex).abs() <= LAND_X && ey.is_none_or(|ey| (read.y - ey).abs() <= LAND_Y);
                let on_floor = ey.is_some_and(|ey| {
                    standing(&edges, ex, ey).is_ok_and(|stand| {
                        let floor = Floor::of(&edges, stand);
                        (read.y - ey).abs() <= LAND_Y && read.x >= floor.lo - FLOOR_SLACK && read.x <= floor.hi + FLOOR_SLACK
                    })
                });
                near || on_floor
            });
            eprintln!(
                "round {round} hop {}: at ({:.2}, {:.2}) health {} (region {}, load {}) expected {:?} {}",
                j + 1,
                read.x,
                read.y,
                read.health,
                read.region,
                read.loads,
                exp,
                if ok { "ok" } else { "OFF" }
            );
            if read.health <= 0 || read.deaths > 0 {
                eprintln!("dead");
                break;
            }
            // A tall room streams its chunks, which counts as a load too: only a hop that did not land where the plan
            // said and came with a load took the exit.
            if base_loads.is_some_and(|b| read.loads > b) && !ok {
                // This hop took the exit (a scene load came with it): the room is done, play the tail once to prove it.
                eprintln!("hop {} took the exit", j + 1);
                body.truncate(ends[j]);
                ends.truncate(j + 1);
                expect.truncate(j + 1);
                hops.truncate(j + 1);
                // The exit hop's landing is in the next room: a retry loop would wait for it in the wrong scene.
                hops[j].retry = None;
                exited = true;
                stopped = true;
                break;
            }
            let jump = hops[j].spec;
            if !ok && o.retry && jump.is_some() && bans.len() < MAX_BANS {
                // The jump missed with all its passes and recoveries: it is no way over this gap on this disc. Ban it and plan
                // the room again from where the hop before it ended; the lines of the hop go.
                let spec = jump.expect("a jump");
                if let Ok(from) = standing(&edges, spec.from.0, spec.from.1) {
                    bans.push(Ban { cell: crate::cell(&from), dir: spec.dir, held: spec.held, steer: spec.steer });
                    set_banned(bans.clone());
                    eprintln!("hop {} banned: {spec:?}", j + 1);
                }
                body.truncate(hops[j].start);
                ends.truncate(j);
                expect.truncate(j);
                hops.truncate(j);
                stopped = true;
                break;
            }
            if !ok {
                // Keep the hop (the run is deterministic, so the prefix replays) and plan again from here.
                body.truncate(ends[j]);
                ends.truncate(j + 1);
                expect.truncate(j + 1);
                hops.truncate(j + 1);
                // A hop that missed stays as it is: its replay is what the hops after it were planned from.
                hops[j].retry = None;
                (cx, cy) = (read.x, read.y);
                stopped = true;
                break;
            }
            (cx, cy) = (read.x, read.y);
            if j + 1 == expect.len() && finished {
                done = all
                    .iter()
                    .any(|(n, w)| n == "end" && probe_of(w).is_some_and(|p| p.deaths == 0));
                eprintln!(
                    "{}",
                    all.iter()
                        .find(|(n, _)| n == "end")
                        .map_or("no end probe".to_string(), |(_, w)| format!("end {w:?}"))
                );
            }
        }
        if !stopped {
            break;
        }
    }
    eprintln!(
        "script {} {}",
        script_path.display(),
        if done { "done" } else { "NOT done" }
    );
    if done {
        Ok(())
    } else {
        Err("the room is not solved".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|l| l.to_string()).collect()
    }

    /// A floor at y 2 from x -10 to 20 and a ledge at y 5 from x 6 to 14, as the cooked packs draw them (a top surface
    /// runs right to left), and the Knight's origin 1.39 above a floor.
    fn two_floors() -> Vec<Edge> {
        let line = |a: (f64, f64), b: (f64, f64)| {
            let q = |v: f64| (v * 65536.0).round() as i32;
            [q(a.0), q(a.1), q(b.0), q(b.1)]
        };
        vec![
            line((20.0, 2.0), (-10.0, 2.0)),
            line((14.0, 5.0), (6.0, 5.0)),
            line((6.0, 5.0), (6.0, 2.0)),
            line((14.0, 2.0), (14.0, 5.0)),
        ]
    }

    fn q(v: f64) -> i64 {
        (v * 65536.0).round() as i64
    }

    fn hop_of(start: usize, end: usize, ex: f64, ey: f64, spec: Option<JumpSpec>) -> Hop {
        Hop {
            start,
            end,
            retry: Some((ex, ey)),
            spec,
            prev_span: Some((1.0, 5.0)),
            markers: vec![(0.0, 3.39)],
            marker_after: (0.0, 3.39),
        }
    }

    #[test]
    fn a_hop_gets_a_clear_guard_and_a_floor_wide_landing_test_and_no_tick_rest() {
        let edges = two_floors();
        let room = hop::Room { edges: &edges, checkpoints: &[], arrival: (0.0, 3.39) };
        let body = strings(&[
            "probe start x y",
            "walk 1.00",
            "hold right+cross for 8",
            "wait until HK_WALK_READY == 1 timeout 300",
            "probe p001 x y",
        ]);
        let hops = vec![hop_of(1, 4, 10.0, 6.39, None)];
        let out = retrying(&body, &hops, "x-t", q, &room, Wrap::Ship);
        assert_eq!(out[1], "clear x-t-h001-arc 10.000 6.390 1.00 5.00", "{out:?}");
        let looped = &out[2];
        assert!(looped.starts_with("loop x-t-h001 until expr (abs(i32(HK_PLAYER_X) - 655360) <= 58982 || (i32(HK_PLAYER_X) >= "), "{looped}");
        // The whole landing floor is in the test, with the slack: the Knight can stand anywhere from its first to its last step.
        let floor = Floor::of(&edges, standing(&edges, 10.0, 6.39).expect("the ledge"));
        assert!(floor.lo < 6.5 && floor.hi > 13.5, "{} {}", floor.lo, floor.hi);
        assert!(looped.contains(&format!("i32(HK_PLAYER_X) >= {} && i32(HK_PLAYER_X) <= {})", q(floor.lo - FLOOR_SLACK), q(floor.hi + FLOOR_SLACK))), "{looped}");
        assert!(looped.ends_with("max 3 timeout 900"), "{looped}");
        assert_eq!(out[out.len() - 2], "end");
        assert!(!out.iter().any(|l| l.starts_with("wait ") && l.split_whitespace().nth(1).is_some_and(|n| n.parse::<u32>().is_ok())), "no tick-count rest: {out:?}");
    }

    #[test]
    fn the_play_version_unrolls_the_passes_as_guards_so_a_miss_ends_at_a_probe() {
        let edges = two_floors();
        let room = hop::Room { edges: &edges, checkpoints: &[], arrival: (0.0, 3.39) };
        let body = strings(&["walk 1.00", "hold right+cross for 8", "wait until HK_WALK_READY == 1 timeout 300", "probe p001 x y"]);
        let hops = vec![hop_of(0, 3, 10.0, 6.39, None)];
        let out = retrying(&body, &hops, "x-t", q, &room, Wrap::Play);
        assert_eq!(out.iter().filter(|l| l.starts_with("if expr !(")).count(), RETRIES as usize, "{out:?}");
        assert!(!out.iter().any(|l| l.starts_with("loop ")), "a loop that never lands aborts the run: {out:?}");
        assert_eq!(out.last().map(String::as_str), Some("probe p001 x y"));
    }

    #[test]
    fn a_tight_hop_gets_more_passes_and_a_clear_and_a_heal_on_each() {
        let edges = two_floors();
        let room = hop::Room { edges: &edges, checkpoints: &[], arrival: (0.0, 3.39) };
        let body = strings(&["walk 1.00", "hold right+cross for 8", "wait until HK_WALK_READY == 1 timeout 300"]);
        let spec = JumpSpec { dir: 1, held: 8, steer: 8, from: (1.0, 3.39), tight: crate::Tight { edge: true, ..Default::default() } };
        let hops = vec![hop_of(0, 3, 10.0, 6.39, Some(spec))];
        let out = retrying(&body, &hops, "x-t", q, &room, Wrap::Ship);
        assert!(out.iter().any(|l| l.starts_with("loop x-t-h001 until") && l.ends_with("max 6 timeout 1800")), "{out:?}");
        assert!(out.iter().any(|l| l.starts_with("clear x-t-h001-tight ")) && out.iter().any(|l| l == "heal"), "{out:?}");
    }

    #[test]
    fn a_hop_that_did_not_move_him_is_not_wrapped_and_a_landing_line_carries_its_jump() {
        let edges = two_floors();
        let room = hop::Room { edges: &edges, checkpoints: &[], arrival: (0.0, 3.39) };
        let body = strings(&["walk 1.00", "probe p001 x y"]);
        let mut plain = hop_of(0, 1, 1.0, 3.39, None);
        plain.retry = None;
        assert_eq!(retrying(&body, &[plain], "x-t", q, &room, Wrap::Ship), body);
        let hop = first_hop(&strings(&[
            "walk 5.00   # take-off",
            "hold right+cross for 34",
            "wait until y between 5.24 5.54 timeout 240   # land at x 16.50 jump 1 34 37 from 5.000 3.390 tight apex",
            "wait until HK_WALK_READY == 1 timeout 300",
            "walk 20.00",
        ]));
        let spec = hop.spec.expect("the jump the landing line describes");
        assert_eq!((spec.dir, spec.held, spec.steer, spec.from), (1, 34, 37, (5.0, 3.39)));
        assert!(spec.tight.apex);
    }

    #[test]
    fn a_jump_hop_ends_at_the_ready_wait_and_expects_its_landing() {
        let plan = strings(&[
            "walk 10.00   # take-off",
            "hold right+cross for 20",
            "wait until y between 5.24 5.54 timeout 240   # land at x 16.50",
            "wait until HK_WALK_READY == 1 timeout 300",
            "walk 20.00",
        ]);
        let First { lines, expect, last, .. } = first_hop(&plan);
        assert_eq!(lines.len(), 3, "the landing wait is replaced by the ready wait: {lines:?}");
        let (x, y) = expect
            .and_then(|(x, y)| y.map(|y| (x, y)))
            .expect("a landing");
        assert!((x - 16.5).abs() < 1e-9 && (y - 5.39).abs() < 1e-9);
        assert!(!last);
    }

    #[test]
    fn a_swing_is_guarded_passes_and_a_wait_on_the_landing() {
        let swing = Swing { stand: (115.79, 23.83), span: (114.91, 118.06), land: (115.79, 4.35) };
        let lines = swing_lines("d06b", &swing, q);
        assert_eq!(lines.first().map(String::as_str), Some("clear d06b-swing-arc 115.790 23.830 114.91 118.06"));
        assert_eq!(lines.iter().filter(|l| l.starts_with("if expr")).count(), SWING_PASSES as usize);
        assert_eq!(lines.iter().filter(|l| *l == "end").count(), SWING_PASSES as usize);
        assert!(lines.iter().filter(|l| l.starts_with("press square")).count() == SWING_PASSES as usize);
        assert!(lines.last().is_some_and(|l| l.starts_with("wait until expr")));
    }

    #[test]
    fn a_drop_ends_its_hop_and_the_last_walk_is_the_last_hop() {
        let First { lines, expect, last, .. } =
            first_hop(&strings(&["walk 4.00", "drop 5.00 2.39", "walk 9.00"]));
        assert_eq!(
            (lines.len(), expect, last),
            (2, Some((5.0, Some(2.39))), false)
        );
        let First { lines, expect, last, .. } = first_hop(&strings(&["walk 4.00", "walk 9.00"]));
        assert_eq!((lines.len(), expect, last), (2, Some((9.0, None)), true));
    }

    #[test]
    fn probes_are_read_back_from_the_gate_log() {
        let log = "\
  FAIL  j/p001: ram HK_PLAYER_X eq -65536: value = [131072 (@0x8012e858)] [probe x]
  FAIL  j/p001: ram HK_PLAYER_Y eq -65536: value = [196608 (@0x8012e85c)] [probe y]
  FAIL  j/p001: ram HK_HEALTH eq -1: value = [4 (@0x800d2280)] [probe HK_HEALTH]
  FAIL  j/p001: ram HK_REGION_ID eq -1: value = [754 (@0x1)] [probe HK_REGION_ID]
  FAIL  j/p001: ram HK_SCENE_LOADS eq -1: value = [1 (@0x2)] [probe HK_SCENE_LOADS]
  FAIL  j/p001: ram HK_DEATHS eq -1: value = [0 (@0x3)] [probe HK_DEATHS]
  FAIL  j/p002: ram HK_PLAYER_X eq -65536: value = [65536 (@0x8012e858)] [probe x]
";
        let all = probes(log);
        assert_eq!(all.len(), 2);
        let read = probe_of(&all[0].1).expect("a full set");
        assert_eq!(
            (read.x, read.y, read.health, read.region, read.loads),
            (2.0, 3.0, 4, 754, 1)
        );
        assert!(probe_of(&all[1].1).is_none());
    }
}
