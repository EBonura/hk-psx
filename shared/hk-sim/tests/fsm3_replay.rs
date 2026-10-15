//! G1 of docs/FSM-V2-DESIGN.md section 4: the original's PlayMaker trace replayed into the HKFSM03 kernel.
//!
//! `tools/hkref` runs the original game headless and writes `fsmtrace.csv` (state entries, events with their cause,
//! transitions, actions, variable changes and one ENV row per frame). This test feeds the kernel what the
//! original's environment gave its FSMs (positions, the hero, the clock, what the senses and the physics
//! reported, the events raised from outside the tree) and requires the kernel to enter the same states, raise
//! the same events and take the same transitions, in the same order per FSM and within one frame of the
//! original. It is skipped unless HK_FSM_TRACE_DIR names a hkref runs directory.
//!
//! cargo test --features trace --test fsm3_replay -- --nocapture
#![cfg(feature = "trace")]
use hk_format::fsm3::Bank;
use hk_sim::fsm3::trace::{self, Cause, Ev};
use hk_sim::fsm3::{Env, Tree, PH_COLLISION_STAY, PH_FIXED, PH_UPDATE};
use hk_sim::{WalkParams, WalkState, ONE};
use std::collections::HashMap;
use std::sync::Mutex;

static SEEN: Mutex<Vec<Ev>> = Mutex::new(Vec::new());
static LOCK: Mutex<()> = Mutex::new(());
fn sink(e: Ev) {
    SEEN.lock().unwrap().push(e);
}

// ---- the oracle ------------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Row {
    steps: i64,
    unity: i64,
    kind: String,
    fsm: String,
    state: String,
    a: String,
    b: String,
}

fn fields(line: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut quoted = false;
    for ch in line.chars() {
        match ch {
            '"' => quoted = !quoted,
            ',' if !quoted => out.push(std::mem::take(&mut cur)),
            c => cur.push(c),
        }
    }
    out.push(cur);
    out
}

fn load(path: &std::path::Path) -> Vec<Row> {
    let text = std::fs::read_to_string(path).unwrap();
    text.lines()
        .skip(1)
        .filter_map(|l| {
            let f = fields(l);
            if f.len() < 9 {
                return None;
            }
            Some(Row {
                steps: f[1].parse().ok()?,
                unity: f[2].parse().ok()?,
                kind: f[3].clone(),
                fsm: f[4].clone(),
                state: f[5].clone(),
                a: f[6].clone(),
                b: f[7].clone(),
            })
        })
        .collect()
}

struct Names {
    fsms: Vec<String>,
    states: Vec<Vec<String>>,
    events: Vec<String>,
    vars: Vec<HashMap<String, (u16, u16)>>,
}
fn names(label: &str) -> Names {
    let text = std::fs::read_to_string(format!("{}/../../data/{label}_names.txt", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let mut n = Names { fsms: vec![], states: vec![], events: vec![], vars: vec![] };
    for l in text.lines() {
        let p: Vec<&str> = l.split('\t').collect();
        match p[0] {
            "fsm" => {
                n.fsms.push(p[2].to_owned());
                n.states.push(vec![]);
                n.vars.push(HashMap::new());
            }
            "state" => n.states[p[1].parse::<usize>().unwrap()].push(p[3].to_owned()),
            "var" => {
                n.vars[p[1].parse::<usize>().unwrap()]
                    .insert(p[2].to_owned(), (p[3].parse().unwrap(), p[4].parse().unwrap()));
            }
            "event" => n.events.push(p[2].to_owned()),
            _ => {}
        }
    }
    n
}

/// What the original's environment gave one actor on one frame.
#[derive(Default, Clone)]
struct Frame {
    unity: i64,
    steps: i64,
    rt: f64,
    time: f64,
    hero: [f64; 3],
    cam: [f64; 3],
    pos: [f64; 3],
    external: Vec<(usize, String)>,
    anim_end: bool,
    /// One entry per bounce the original made this frame (the collision callback repeats), as contact bits.
    bonks: Vec<u8>,
    forced: Vec<f64>,
    /// Latest values of the variables the senses stored.
    can_see: Option<bool>,
    in_range: Option<bool>,
}

fn q16(f: f64) -> i32 {
    (f * 65536.0).round() as i32
}
fn floats(s: &str) -> Vec<f64> {
    s.split(' ').filter_map(|x| x.parse().ok()).collect()
}

/// Observable stream entry: (unity frame, kind, fsm, name, target state).
type Obs = (i64, &'static str, usize, String, String);

struct Run<'a> {
    bank: Bank<'static>,
    names: &'a Names,
    configure: &'a dyn Fn(&mut Env),
    collision: bool,
}

fn fsm_index(names: &Names, id: &str) -> Option<usize> {
    let fsm_name = id.rsplit_once('/')?.1;
    names.fsms.iter().position(|f| f == fsm_name)
}

/// Replay one actor, named as the trace names it ("Crawler 1#94288").
fn replay(run: &Run, rows: &[Row], root: &str, seed: u32) -> (Vec<Obs>, Vec<Obs>) {
    let names = run.names;
    // Frames from the ENV rows, one per Unity frame from the first to the last; a frame with no row holds the
    // last known environment and takes none of the physics steps.
    let mut known: Vec<Frame> = vec![];
    let mut root_id = String::new();
    let prefix = format!("{root}/");
    for r in rows.iter().filter(|r| r.kind == "ENV") {
        let mut f = Frame { unity: r.unity, steps: r.steps, ..Default::default() };
        let mut found = false;
        for part in r.a.split(';') {
            if let Some(v) = part.strip_prefix("rt ") {
                f.rt = v.parse().unwrap_or(0.0);
            } else if let Some(v) = part.strip_prefix("time ") {
                f.time = v.parse().unwrap_or(0.0);
            } else if let Some(v) = part.strip_prefix("hero ") {
                let x = floats(v);
                f.hero = [x[0], x[1], x[2]];
            } else if let Some(v) = part.strip_prefix("cam ") {
                let x = floats(v);
                f.cam = [x[0], x[1], x[2]];
            } else if let Some((id, rest)) = part.split_once('@') {
                if id == root {
                    found = true;
                    root_id = id.to_owned();
                    let x = floats(rest);
                    f.pos = [x[0], x[1], x[2]];
                }
            }
        }
        // The actor is not in the picture until the trace has seen it (before that it was still loading).
        if found && known.last().is_none_or(|l| l.unity != f.unity) {
            known.push(f);
        }
    }
    assert!(!known.is_empty(), "no ENV rows");
    let first = known[0].unity;
    let last = known.last().unwrap().unity;
    let mut frames: Vec<Frame> = vec![];
    let mut at = 0;
    for u in first..=last {
        if known[at].unity < u && at + 1 < known.len() && known[at + 1].unity <= u {
            at += 1;
        }
        let mut f = known[at].clone();
        f.unity = u;
        if known[at].unity != u {
            f.steps = known[at].steps; // no new physics steps until a frame that has a row
        }
        frames.push(f);
    }
    let frame_at = |u: i64| -> usize { (u.clamp(first, last) - first) as usize };
    // Oracle stream and the inputs the rows imply.
    let mut theirs: Vec<Obs> = vec![];
    let mut last_action: HashMap<String, String> = HashMap::new();
    for r in rows.iter().filter(|r| r.fsm.starts_with(&prefix)) {
        let Some(f) = fsm_index(names, &r.fsm) else { continue };
        let at = frame_at(r.unity);
        match r.kind.as_str() {
            "STATE_ENTER" => theirs.push((r.unity.max(first), "STATE", f, r.a.clone(), String::new())),
            "TRANSITION" => theirs.push((r.unity.max(first), "TRANS", f, r.a.clone(), r.b.clone())),
            "ACTION_ENTER" | "ACTION_TICK" => {
                last_action.insert(r.fsm.clone(), r.a.clone());
            }
            "EVENT" => {
                let known = names.events.contains(&r.a);
                // An event another FSM's SendEventByName raises is as much from outside the tree as a callback.
                if r.b.starts_with("ext:") || r.b.starts_with("action:SendEventByName") {
                    // FINISHED is the machine's own event; the original logs it without an action cause.
                    if known && r.a != "FINISHED" {
                        frames[at].external.push((f, r.a.clone()));
                    }
                } else if known {
                    theirs.push((r.unity.max(first), "EVENT", f, r.a.clone(), String::new()));
                    if r.a == "ANIM END" {
                        frames[at].anim_end = true;
                    }
                    if r.b.starts_with("action:CheckCollisionSide") {
                        let bit = match r.a.as_str() {
                            "BONK UP" => 1,
                            "BONK RIGHT" => 2,
                            "BONK DOWN" => 4,
                            "BONK LEFT" => 8,
                            _ => 0,
                        };
                        frames[at].bonks.push(bit);
                    }
                }
            }
            "FORCED_EVENT" => {}
            "VARS" => {
                let la = last_action.get(&r.fsm).cloned().unwrap_or_default();
                for kv in r.a.split(';') {
                    let Some((k, v)) = kv.split_once('=') else { continue };
                    match k {
                        "Can See Hero" => frames[at].can_see = Some(v == "true"),
                        "In Alert Range" => frames[at].in_range = Some(v == "true"),
                        "Angle" if la == "RandomFloat" => frames[at].forced.push(v.parse().unwrap_or(0.0)),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    // Forced events carry the FSM as "Name/Fsm" without an instance id: they belong to the actor named by the prefix.
    let base = root.split('#').next().unwrap();
    let _ = root_id;
    for r in rows.iter().filter(|r| r.kind == "FORCED_EVENT") {
        let (go, fsm) = r.fsm.split_once('/').unwrap();
        if go == base {
            if let Some(f) = names.fsms.iter().position(|n| n == fsm) {
                if names.events.contains(&r.a) {
                    let at = frame_at(r.unity);
                    frames[at].external.push((f, r.a.clone()));
                }
            }
        }
    }

    // Our side.
    let _g = LOCK.lock().unwrap();
    SEEN.lock().unwrap().clear();
    unsafe { trace::SINK = Some(sink) };
    let mut mem = vec![0u32; run.bank.runtime_bytes().div_ceil(4)];
    let mut env = Env::new();
    (run.configure)(&mut env);
    let mut can_see = false;
    let mut in_range = false;
    // SAFETY: `mem` is the bank's runtime size and 4-aligned.
    unsafe { Tree::init(run.bank, mem.as_mut_ptr().cast(), &mut env, seed) };
    let mut ours: Vec<Obs> = vec![];
    let mut previous_steps = frames[0].steps;
    let mut previous_rt = frames[0].rt;
    let mut previous_time = frames[0].time;
    let mut walk_senses = 0u32;
    for fr in &frames {
        env.pos = [q16(fr.pos[0]), q16(fr.pos[1]), q16(fr.pos[2])];
        env.hero = [q16(fr.hero[0]), q16(fr.hero[1]), q16(fr.hero[2])];
        env.camera = [q16(fr.cam[0]), q16(fr.cam[1]), q16(fr.cam[2])];
        // Both clocks as the original ran them: the frames are not 1/60 s apart, so the waits get the real elapsed time.
        env.dt_real = q16((fr.rt - previous_rt).max(0.0) * 60.0);
        env.dt_scaled = q16((fr.time - previous_time).max(0.0) * 60.0);
        previous_rt = fr.rt;
        previous_time = fr.time;
        if let Some(c) = fr.can_see {
            can_see = c;
        }
        if let Some(c) = fr.in_range {
            in_range = c;
        }
        env.can_see_hero = can_see;
        env.in_alert_range = in_range;
        if fr.anim_end {
            env.anim_left = 0;
        }
        let draws: Vec<f64> = fr.forced.clone();
        env.forced_n = draws.len().min(8) as u8;
        for (i, v) in draws.iter().take(8).enumerate() {
            env.forced_random[i] = q16(*v);
        }
        // Wander the Walker's senses deterministically; the original's physics is not part of the trace.
        walk_senses += 1;
        env.senses = (walk_senses % 19 != 0, walk_senses % 67 == 3, walk_senses % 83 == 4);
        let steps = (fr.steps - previous_steps).clamp(0, 4);
        previous_steps = fr.steps;
        // SAFETY: as above.
        let mut tree = unsafe { Tree::attach(run.bank, mem.as_mut_ptr().cast(), &mut env) };
        for (f, e) in &fr.external {
            let id = names.events.iter().position(|n| n == e).unwrap() as u8;
            tree.event(*f as u8, id);
        }
        for _ in 0..steps {
            tree.step(PH_FIXED);
        }
        // The engine calls the collision actions back once per bounce it saw; the kernel answers each.
        for bit in &fr.bonks {
            tree.env.contacts = *bit;
            tree.step(PH_COLLISION_STAY);
        }
        tree.step(PH_UPDATE);
        env.anim_left = if fr.anim_end { 0xffff } else { env.anim_left };
        env.sound = 0;
        env.dirty = 0;
        env.play_count = 0;
        for e in SEEN.lock().unwrap().drain(..) {
            if std::env::var("HK_REPLAY_RAW").is_ok() {
                eprintln!("raw {} {:?}", fr.unity, e);
            }
            match e {
                Ev::StateEnter { fsm, state } => ours.push((
                    fr.unity,
                    "STATE",
                    fsm as usize,
                    names.states[fsm as usize][state as usize].clone(),
                    String::new(),
                )),
                Ev::Transition { fsm, event, to, .. } => ours.push((
                    fr.unity,
                    "TRANS",
                    fsm as usize,
                    names.events[event as usize].clone(),
                    names.states[fsm as usize][to as usize].clone(),
                )),
                Ev::Event { fsm, event, cause, .. } => {
                    if matches!(cause, Cause::Action(_) | Cause::Queue) {
                        ours.push((
                            fr.unity,
                            "EVENT",
                            fsm as usize,
                            names.events[event as usize].clone(),
                            String::new(),
                        ));
                    }
                }
                _ => {}
            }
        }
    }
    unsafe { trace::SINK = None };
    (ours, theirs)
}

/// Print a stream when HK_REPLAY_DEBUG is set.
fn dump(label: &str, v: &[Obs]) {
    if std::env::var("HK_REPLAY_DEBUG").is_ok() {
        for o in v.iter().take(40) {
            eprintln!("{label} {}@{} f{} {} {}", o.1, o.0, o.2, o.3, o.4);
        }
    }
}

/// Compare per FSM: same sequence, each entry within `skew` frames.
fn compare(label: &str, ours: &[Obs], theirs: &[Obs], fsm_count: usize, skew: i64) -> Result<usize, String> {
    let mut total = 0;
    // An event the machine then takes as a transition is the transition; keep only the events nobody took.
    let thin = |v: &[Obs], f: usize| -> Vec<Obs> {
        let own: Vec<&Obs> = v.iter().filter(|o| o.2 == f).collect();
        let mut out = vec![];
        for (i, o) in own.iter().enumerate() {
            if o.1 == "EVENT" && own.get(i + 1).is_some_and(|n| n.1 == "TRANS" && n.3 == o.3) {
                continue;
            }
            out.push((*o).clone());
        }
        out
    };
    for f in 0..fsm_count {
        let (ta, tb) = (thin(ours, f), thin(theirs, f));
        let a: Vec<&Obs> = ta.iter().collect();
        let b: Vec<&Obs> = tb.iter().collect();
        // The original starts logging one step before the first frame; its first state entry is ours.
        let n = a.len().min(b.len());
        for i in 0..n {
            let same = a[i].1 == b[i].1 && a[i].3 == b[i].3 && a[i].4 == b[i].4;
            if !same || (a[i].0 - b[i].0).abs() > skew {
                let ctx = |v: &[&Obs]| {
                    v[i.saturating_sub(2)..(i + 3).min(v.len())]
                        .iter()
                        .map(|o| format!("{}@{} {} {}", o.1, o.0, o.3, o.4))
                        .collect::<Vec<_>>()
                        .join(" | ")
                };
                return Err(format!(
                    "{label} FSM {f}: entry {i} differs\n  ours:   {}\n  theirs: {}",
                    ctx(&a),
                    ctx(&b)
                ));
            }
        }
        if a.len() != b.len() {
            // The window can end between a transition and the rows that follow it; allow one entry of slack.
            if a.len().abs_diff(b.len()) > 1 {
                return Err(format!("{label} FSM {f}: {} entries of ours against {} of theirs", a.len(), b.len()));
            }
        }
        total += n;
    }
    Ok(total)
}

/// Report how much of the graph a replay exercised.
fn coverage(label: &str, n: &Names, all: &[(Vec<Obs>, Vec<Obs>)], checked: usize) {
    let mut seen: Vec<std::collections::BTreeSet<String>> = vec![Default::default(); n.fsms.len()];
    let mut events = std::collections::BTreeSet::new();
    for (ours, _) in all {
        for o in ours {
            if o.1 == "STATE" {
                seen[o.2].insert(o.3.clone());
            }
            if o.1 == "TRANS" {
                events.insert(format!("{}:{}", o.2, o.3));
            }
        }
    }
    let parts: Vec<String> =
        (0..n.fsms.len()).map(|f| format!("{} {}/{} states", n.fsms[f], seen[f].len(), n.states[f].len())).collect();
    eprintln!("{label}: {checked} observations matched; {} ({} distinct transitions)", parts.join(", "), events.len());
}

fn trace_dir() -> Option<std::path::PathBuf> {
    std::env::var("HK_FSM_TRACE_DIR").ok().map(Into::into)
}

fn roots(rows: &[Row], base: &str) -> Vec<String> {
    let mut v: Vec<String> = rows
        .iter()
        .filter(|r| r.fsm.starts_with(base) && r.fsm.contains('#'))
        .map(|r| r.fsm.split_once('/').unwrap().0.to_owned())
        .collect();
    v.sort();
    v.dedup();
    v
}

#[test]
fn crawler_matches_the_original() {
    let Some(dir) = trace_dir() else { return eprintln!("HK_FSM_TRACE_DIR not set: skipped") };
    let rows = load(&dir.join("crawler-fsmtrace/og/fsmtrace.csv"));
    let n = names("crawler3");
    // SAFETY: the cooker parsed this bank.
    let bank = leak_bank("crawler3");
    let configure = |e: &mut Env| {
        e.walk = WalkState::new(-1, false);
        e.walk_params = WalkParams { speed: 4 * ONE, turn_ticks: 7, turn_cooldown_ticks: 60 };
    };
    let run = Run { bank, names: &n, configure: &configure, collision: false };
    let mut checked = 0;
    let mut all = vec![];
    for root in roots(&rows, "Crawler ") {
        let (ours, theirs) = replay(&run, &rows, &root, 1);
        all.push((ours.clone(), theirs.clone()));
        // A realTime Wait finishes on the wall clock, which the original samples at an instant inside its frame:
        // two of its ~11 ms frames is as close as that can be matched. A game-time Wait (Start R) must be within one.
        match compare(&root, &ours, &theirs, 1, 2) {
            Ok(k) => checked += k,
            Err(e) => panic!("{e}"),
        }
        let after_start_r = |v: &[Obs]| -> Vec<i64> {
            v.windows(2)
                .filter(|w| w[0].1 == "STATE" && w[0].3 == "Start R" && w[1].1 == "TRANS" && w[1].3 == "FINISHED")
                .map(|w| w[1].0)
                .collect()
        };
        for (a, b) in after_start_r(&ours).iter().zip(after_start_r(&theirs)) {
            assert!((a - b).abs() <= 1, "the game-time Wait ended at {a}, the original's at {b}");
        }
    }
    coverage("Crawler", &n, &all, checked);
    assert!(checked >= 9, "the replay should compare the directed starts, saw {checked}");
}

fn leak_bank(label: &str) -> Bank<'static> {
    let bytes = std::fs::read(format!("{}/../../data/{label}.hkf3", env!("CARGO_MANIFEST_DIR"))).unwrap();
    // SAFETY: the cooker parsed this bank (and `Bank::parse` below says so again).
    Bank::parse(Box::leak(bytes.into_boxed_slice())).unwrap()
}

mod vengefly_ids {
    include!("../../../data/vengefly_fsm3.rs");
}

#[test]
fn vengefly_matches_the_original() {
    let Some(dir) = trace_dir() else { return eprintln!("HK_FSM_TRACE_DIR not set: skipped") };
    let rows = load(&dir.join("vengefly-fsmtrace/og/fsmtrace.csv"));
    let n = names("vengefly3");
    let configure = |e: &mut Env| {
        e.alert_range = 511463;
        e.clips[0] = (vengefly_ids::CLIP_STARTLE, 20);
    };
    let run = Run { bank: leak_bank("vengefly3"), names: &n, configure: &configure, collision: false };
    let mut checked = 0;
    let mut all = vec![];
    for root in roots(&rows, "Buzzer") {
        let (ours, theirs) = replay(&run, &rows, &root, 1);
        all.push((ours.clone(), theirs.clone()));
        dump("ours  ", &ours);
        dump("theirs", &theirs);
        match compare(&root, &ours, &theirs, 2, 1) {
            Ok(k) => checked += k,
            Err(e) => panic!("{e}"),
        }
    }
    coverage("Vengefly", &n, &all, checked);
    assert!(checked >= 30, "the replay should compare a chase, saw {checked}");
}

#[test]
fn gruzzer_matches_the_original() {
    let Some(dir) = trace_dir() else { return eprintln!("HK_FSM_TRACE_DIR not set: skipped") };
    let rows = load(&dir.join("gruzzer-fsmtrace/og/fsmtrace.csv"));
    let n = names("gruzzer3");
    let configure = |_: &mut Env| {};
    let run = Run { bank: leak_bank("gruzzer3"), names: &n, configure: &configure, collision: true };
    let mut checked = 0;
    let mut all = vec![];
    for root in roots(&rows, "Fly") {
        let (ours, theirs) = replay(&run, &rows, &root, 1);
        all.push((ours.clone(), theirs.clone()));
        match compare(&root, &ours, &theirs, 1, 1) {
            Ok(k) => checked += k,
            Err(e) => panic!("{e}"),
        }
    }
    coverage("Gruzzer", &n, &all, checked);
    assert!(checked >= 100, "the replay should compare many bounces, saw {checked}");
}

/// GetDistance's numeric contract, fixed before the run: the Q16.16 distance of the two positions the original saw
/// agrees with the float it stored to within 2^-10 world units (the positions themselves are 32-bit floats).
#[test]
fn gruzzer_get_distance_is_within_its_bound() {
    let Some(dir) = trace_dir() else { return eprintln!("HK_FSM_TRACE_DIR not set: skipped") };
    let rows = load(&dir.join("gruzzer-fsmtrace/og/fsmtrace.csv"));
    let mut env_rows: HashMap<i64, &Row> = HashMap::new();
    for r in rows.iter().filter(|r| r.kind == "ENV") {
        env_rows.insert(r.unity, r);
    }
    let (mut checked, mut worst) = (0, 0.0f64);
    for r in rows.iter().filter(|r| r.kind == "VARS" && r.fsm.contains("Bouncer Control")) {
        let Some(d) = r.a.split(';').find_map(|kv| kv.strip_prefix("Distance=")).and_then(|v| v.parse::<f64>().ok())
        else {
            continue;
        };
        let Some(env) = env_rows.get(&r.unity) else { continue };
        let root = r.fsm.split_once('/').unwrap().0;
        let (mut cam, mut pos) = (None, None);
        for part in env.a.split(';') {
            if let Some(v) = part.strip_prefix("cam ") {
                cam = Some(floats(v));
            } else if let Some((id, rest)) = part.split_once('@') {
                if id == root {
                    pos = Some(floats(rest));
                }
            }
        }
        let (Some(c), Some(p)) = (cam, pos) else { continue };
        if d == 0.0 {
            continue;
        }
        let ours = hk_sim::fsm3_ai::distance([q16(p[0]), q16(p[1]), q16(p[2])], [q16(c[0]), q16(c[1]), q16(c[2])])
            as f64
            / 65536.0;
        worst = worst.max((ours - d).abs());
        checked += 1;
    }
    eprintln!("GetDistance: {checked} samples, worst error {worst:.6} units");
    assert!(checked > 0 && worst <= 1.0 / 1024.0, "worst error {worst}");
}
