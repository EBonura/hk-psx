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
//!                  [--spawn X Y REGION] [--goal X Y] [--tail 'line;line'] [--tol T] [--max N]
//!                  [--pre FILE] [--card FILE] [--fireball N] [--work DIR]
//! ```
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

use crate::{
    floor_span, load_edges, load_hazards, search, set_hazards, standing, walk_lines, Edge,
};

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
    tolerance: f64,
    max_hops: usize,
    pre: Option<PathBuf>,
    card: Option<String>,
    fireball: Option<u32>,
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
        tolerance: 0.6,
        max_hops: 40,
        pre: None,
        card: None,
        fireball: None,
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
            "--tol" => o.tolerance = number(&mut it)?,
            "--max" => o.max_hops = number(&mut it)? as usize,
            "--pre" => o.pre = Some(PathBuf::from(it.next().ok_or_else(usage)?)),
            "--card" => o.card = Some(it.next().ok_or_else(usage)?.clone()),
            "--fireball" => o.fireball = Some(number(&mut it)? as u32),
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

/// The first hop of a plan: its lines, where it should end (x, and y when the plan knows it), and whether it is the
/// last one. A jump's landing is waited for by `HK_WALK_READY` (never a timeout, which would abort the run).
fn first_hop(plan: &[String]) -> (Vec<String>, Option<(f64, Option<f64>)>, bool, usize) {
    let (mut group, mut expect) = (Vec::new(), None);
    for (n, line) in plan.iter().enumerate() {
        let code = line.split('#').next().unwrap_or("");
        let t: Vec<&str> = code.split_whitespace().collect();
        let last = n + 1 == plan.len();
        match t.as_slice() {
            ["wait", "until", "y", "between", lo, hi, ..] => {
                let x = line
                    .split("land at x ")
                    .nth(1)
                    .and_then(|r| r.trim().parse::<f64>().ok());
                if let (Some(x), Ok(lo), Ok(hi)) = (x, lo.parse::<f64>(), hi.parse::<f64>()) {
                    expect = Some((x, Some((lo + hi) / 2.0)));
                }
                continue;
            }
            ["drop", x, y] => {
                expect = x
                    .parse()
                    .ok()
                    .zip(y.parse().ok())
                    .map(|(x, y)| (x, Some(y)));
                group.push(line.clone());
                return (group, expect, last, n + 1);
            }
            ["wait", "until", "HK_WALK_READY", ..] => {
                group.push(line.clone());
                return (group, expect, last, n + 1);
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
    (group, expect, true, plan.len())
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
    let header = [
        format!("name {name}"),
        "max_ticks 60000".to_string(),
        format!("card {card}"),
        "wait 300".to_string(),
        "press cross".to_string(),
        "wait 40".to_string(),
        "press cross".to_string(),
        "wait until HK_GAME_MODE == 1 timeout 1200".to_string(),
        "wait until HK_WALK_READY == 1 timeout 900".to_string(),
    ];
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
        3 * ONE
    );
    let tail = o.tail.clone().unwrap_or_else(|| match side {
        1 => vec![format!("hold left until {reached} timeout 900")],
        2 => vec![format!("hold right until {reached} timeout 900")],
        _ => vec![format!("wait until {reached} timeout 600")],
    });
    let tail: Vec<String> = tail
        .into_iter()
        .chain([
            "wait until HK_GAME_MODE == 1 timeout 1800".to_string(),
            "probe end x y HK_REGION_ID HK_SCENE_LOADS HK_HEALTH HK_DEATHS".to_string(),
        ])
        .collect();

    let edges: Vec<Edge> = load_edges(&root, &o.scene, &o.open)?;
    set_hazards(load_hazards(&root, &o.scene)?);
    let script_path = root.join("tools/walks").join(format!("{name}.walk"));
    // The script so far: every hop played with a probe after it, where each ended, and where it should have.
    let (mut body, mut ends): (Vec<String>, Vec<usize>) = (Vec::new(), Vec::new());
    let mut expect: Vec<Option<(f64, Option<f64>)>> = Vec::new();
    let (mut cx, mut cy) = (sx, sy);
    let mut done = false;
    for round in 1..=o.max_hops {
        // Plan the rest of the room from where the Knight really is, and play all of it: hops that land as planned
        // cost nothing more than their own ticks, the first that does not stops the round and the next plan starts
        // from where it ended.
        let start = standing(&edges, cx, cy)?;
        let Some(plan) = search(&edges, start, (gx, gy), o.tolerance, true) else {
            eprintln!("no plan from ({cx:.2}, {cy:.2})");
            break;
        };
        let mut rest: Vec<String> = walk_lines(&plan, &edges, &start)
            .into_iter()
            .filter(|l| !l.starts_with("at "))
            .collect();
        let first_new = expect.len();
        let mut finished;
        loop {
            let (hop, exp, last, used) = first_hop(&rest);
            rest.drain(..used);
            let k = expect.len() + 1;
            body.extend(hop.iter().map(|l| l.replace("timeout 240", "timeout 400")));
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
        // The fragment: this room from its arrival to its exit (what a chain includes); the walk is a card start plus it.
        let mut frag: Vec<String> = vec![format!("at {sx:.2} {sy:.2}")];
        frag.extend(body.iter().cloned());
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
            let kept: Vec<&str> = frag
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
                break;
            };
            let exp = expect[j];
            let ok = exp.is_none_or(|(ex, ey)| {
                (read.x - ex).abs() <= LAND_X && ey.is_none_or(|ey| (read.y - ey).abs() <= LAND_Y)
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
            if !ok {
                // Keep the hop (the run is deterministic, so the prefix replays) and plan again from here.
                body.truncate(ends[j]);
                ends.truncate(j + 1);
                expect.truncate(j + 1);
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

    #[test]
    fn a_jump_hop_ends_at_the_ready_wait_and_expects_its_landing() {
        let plan = strings(&[
            "walk 10.00   # take-off",
            "hold right+cross for 20",
            "wait until y between 5.24 5.54 timeout 240   # land at x 16.50",
            "wait 6",
            "wait until HK_WALK_READY == 1 timeout 300",
            "walk 20.00",
        ]);
        let (hop, expect, last, _) = first_hop(&plan);
        assert_eq!(
            hop.len(),
            4,
            "the landing wait is replaced by the ready wait: {hop:?}"
        );
        let (x, y) = expect
            .and_then(|(x, y)| y.map(|y| (x, y)))
            .expect("a landing");
        assert!((x - 16.5).abs() < 1e-9 && (y - 5.39).abs() < 1e-9);
        assert!(!last);
    }

    #[test]
    fn a_drop_ends_its_hop_and_the_last_walk_is_the_last_hop() {
        let (hop, expect, last, _) =
            first_hop(&strings(&["walk 4.00", "drop 5.00 2.39", "walk 9.00"]));
        assert_eq!(
            (hop.len(), expect, last),
            (2, Some((5.0, Some(2.39))), false)
        );
        let (hop, expect, last, _) = first_hop(&strings(&["walk 4.00", "walk 9.00"]));
        assert_eq!((hop.len(), expect, last), (2, Some((9.0, None)), true));
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
