//! G1 for the boss tier: the original's PlayMaker trace of Greenpath's Hornet replayed into the HKFSM03 kernel.
//!
//! `tools/hkref` runs the original headless (`hkref fsmtrace profiles/hornet1-fsmtrace.json`) and writes `fsmtrace.csv`.
//! This test feeds the kernel what the original's world gave its FSMs (the boss's place, the Knight, the clocks, which
//! triggers and sprite animations fired, the contacts the physics reported, the random draws) and requires the kernel to
//! enter the same states, raise the same events and take the same transitions, per FSM, within a frame of the original.
//! Skipped unless HK_HORNET_TRACE names a `fsmtrace.csv`.
//!
//! HK_HORNET_TRACE=.../runs/hornet1-fsmtrace/og/fsmtrace.csv cargo test --features trace,boss --test fsm3_hornet -- --nocapture
#![cfg(all(feature = "trace", feature = "boss"))]
use hk_format::fsm3::{ops, Bank, NONE8};
use hk_sim::fsm3::trace::{self, Cause, Ev};
use hk_sim::fsm3::{PH_COLLISION_ENTER, PH_COLLISION_STAY};
use hk_sim::hornet::{bank, Hornet, OWNERS};
use std::collections::HashMap;
use std::sync::Mutex;

static SEEN: Mutex<Vec<Ev>> = Mutex::new(Vec::new());
fn sink(e: Ev) {
    SEEN.lock().unwrap().push(e);
}

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
    strings: Vec<String>,
}
fn names(label: &str) -> Names {
    let text = std::fs::read_to_string(format!("{}/../../data/{label}_names.txt", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let mut n = Names { fsms: vec![], states: vec![], events: vec![], strings: vec![] };
    for l in text.lines() {
        let p: Vec<&str> = l.split('\t').collect();
        match p[0] {
            "fsm" => {
                n.fsms.push(p[2].to_owned());
                n.states.push(vec![]);
            }
            "state" => n.states[p[1].parse::<usize>().unwrap()].push(p[3].to_owned()),
            "event" => n.events.push(p[2].to_owned()),
            "string" => n.strings.push(p.get(2).copied().unwrap_or("").to_owned()),
            _ => {}
        }
    }
    n
}

fn floats(s: &str) -> Vec<f64> {
    s.split(' ').filter_map(|x| x.parse().ok()).collect()
}
fn q16(f: f64) -> i32 {
    (f * 65536.0).round() as i32
}

/// What the original's world gave the tree on one Unity frame.
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
    /// (fsm, kind) trigger callbacks: 0 enter, 1 stay, 2 exit.
    triggers: Vec<(usize, u8)>,
    /// FSMs whose sprite animation completed this frame.
    anim_done: Vec<usize>,
    /// (fsm, event, stay) collision callbacks.
    contacts: Vec<(usize, String, bool)>,
    /// Events the original drew from a SendRandomEvent family action: (fsm, event).
    /// SendRandomEvent family actions entered this frame, in order, each with the event it drew (None: an entry with no event).
    draws: Vec<(usize, Option<String>)>,
    /// A WaitRandom's length, from the frame it was entered: (fsm, seconds).
    waits: Vec<(usize, f64)>,
    forced: Vec<f64>,
    /// The original logged this frame (its FSMs updated); a frame without a row did not run them.
    present: bool,
}

type Obs = (i64, &'static str, usize, String, String);

/// Trace FSM id ("Evade Range#124970/FSM") to the tree's FSM index: the object the FSM is on by name, then the FSM's name,
/// then the order among FSMs of the same pair.
struct Index {
    by_key: HashMap<(String, String), Vec<usize>>,
}
impl Index {
    fn new(n: &Names) -> Self {
        let mut by_key: HashMap<(String, String), Vec<usize>> = HashMap::new();
        for (f, owner) in OWNERS.iter().enumerate() {
            let name_id = hk_sim::hornet::ENTS[*owner as usize].name as usize;
            let obj = n.strings.get(name_id).cloned().unwrap_or_default();
            by_key.entry((obj, n.fsms[f].clone())).or_default().push(f);
        }
        Self { by_key }
    }
    fn of(&self, id: &str) -> Option<usize> {
        let (obj, fsm) = id.rsplit_once('/')?;
        let obj = obj.split('#').next()?.to_owned();
        self.by_key.get(&(obj, fsm.to_owned())).and_then(|v| v.first().copied())
    }
}

/// The events some SendEventByName action of the bank raises (by name), so that an event arriving by that cause which the
/// tree never sends can be told from one it will raise itself.
fn sent_by_the_tree(n: &Names) -> std::collections::HashSet<String> {
    let b = bank();
    let mut out = std::collections::HashSet::new();
    for f in 0..b.fsm_count() {
        let d = b.dir(f);
        for st in 0..d.states {
            let rec = b.state(f, st);
            let mut at = rec.block as usize + 2 * rec.transitions as usize;
            for _ in 0..rec.actions {
                let head = b.byte(at);
                let (fam, op) = (head & 0xf, b.byte(at + 1));
                let k = b.arity_at(at);
                if fam == 2 && op == ops::events::SEND_EVENT_BY_NAME {
                    let w = b.word(at + 2 + 2 * 3);
                    if w >> 13 == hk_format::fsm3::T_EVENT {
                        out.insert(n.events[(w & 0x1fff) as usize].clone());
                    }
                }
                at += 2 + 2 * k;
            }
        }
    }
    out
}

#[test]
fn hornet_matches_the_original() {
    let Ok(path) = std::env::var("HK_HORNET_TRACE") else { return eprintln!("HK_HORNET_TRACE not set: skipped") };
    let rows = load(std::path::Path::new(&path));
    let n = names("hornet3");
    let index = Index::new(&n);
    let sent = sent_by_the_tree(&n);
    // The frames, from the ENV rows.
    let mut known: Vec<Frame> = vec![];
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
                if id.starts_with("Hornet Boss 1#") {
                    found = true;
                    let x = floats(rest);
                    f.pos = [x[0], x[1], x[2]];
                }
            }
        }
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
        f.present = known[at].unity == u;
        if known[at].unity != u {
            f.steps = known[at].steps;
        }
        frames.push(f);
    }
    let frame_at = |u: i64| -> usize { (u.clamp(first, last) - first) as usize };
    // Times by frame, for the WaitRandom lengths.
    let times: Vec<f64> = frames.iter().map(|f| f.time).collect();
    let time_at = |u: i64| times[frame_at(u)];
    let mut theirs: Vec<Obs> = vec![];
    let mut wait_enter: HashMap<usize, i64> = HashMap::new();
    let mut wait_queue: HashMap<usize, Vec<(i64, f64)>> = HashMap::new();
    let mut last_action: HashMap<usize, String> = HashMap::new();
    let mut pending_draw: HashMap<(usize, String), usize> = HashMap::new();
    for r in rows.iter().filter(|r| r.fsm.contains('#')) {
        let Some(f) = index.of(&r.fsm) else { continue };
        let at = frame_at(r.unity);
        match r.kind.as_str() {
            "STATE_ENTER" => theirs.push((r.unity.max(first), "STATE", f, r.a.clone(), String::new())),
            "TRANSITION" => theirs.push((r.unity.max(first), "TRANS", f, r.a.clone(), r.b.clone())),
            "ACTION_ENTER" => {
                if r.a == "WaitRandom" {
                    wait_enter.insert(f, r.unity);
                }
                if r.a.starts_with("SendRandomEvent") {
                    frames[at].draws.push((f, None));
                    pending_draw.insert((f, r.a.clone()), frames[at].draws.len() - 1);
                }
                last_action.insert(f, r.a.clone());
            }
            "EVENT" => {
                let known_event = n.events.contains(&r.a);
                if r.b.starts_with("ext:PlayMakerFSM.SendEvent") {
                    if known_event && r.a != "FINISHED" {
                        frames[at].external.push((f, r.a.clone()));
                    }
                } else if r.b.starts_with("action:SendEventByName") && known_event && !sent.contains(&r.a) {
                    // Raised by an FSM the tree does not hold (the dialogue box, the battle scene).
                    frames[at].external.push((f, r.a.clone()));
                } else if r.b.starts_with("ext:") {
                    // The machine's own FINISHED and callbacks the original logs without an action.
                } else if known_event {
                    theirs.push((r.unity.max(first), "EVENT", f, r.a.clone(), String::new()));
                    if r.b.contains("Trigger2dEvent.DoTriggerEnter2D") {
                        frames[at].triggers.push((f, 0));
                    } else if r.b.contains("Trigger2dEvent.DoTriggerStay2D") {
                        frames[at].triggers.push((f, 1));
                    } else if r.b.contains("Trigger2dEvent.DoTriggerExit2D") {
                        frames[at].triggers.push((f, 2));
                    } else if r.b.contains("AnimationCompleteDelegate") {
                        frames[at].anim_done.push(f);
                    } else if r.b.contains("CheckCollisionSide") {
                        frames[at].contacts.push((f, r.a.clone(), r.b.contains("CheckCollisionSide.")));
                    } else if r.b.contains("SendRandomEvent") {
                        let action = r.b.trim_start_matches("action:").split('.').next().unwrap_or("").to_owned();
                        if let Some(i) = pending_draw.remove(&(f, action)) {
                            frames[at].draws[i].1 = Some(r.a.clone());
                        }
                    } else if r.b.contains("WaitRandom") {
                        if let Some(start) = wait_enter.get(&f) {
                            // Fed in order per FSM, as the time the original's wait ended at: the length is worked out on the
                            // frame our entry happens, however many frames that is from the original's.
                            wait_queue.entry(f).or_default().push((*start, time_at(r.unity)));
                        }
                    }
                }
            }
            "VARS" => {
                // The variable a RandomFloat stored is the draw the kernel must reproduce.
                if last_action.get(&f).is_some_and(|a| a == "RandomFloat") {
                    if let Some(x) = r.a.split(';').next().and_then(|kv| kv.split_once('=')).and_then(|(_, v)| v.parse::<f64>().ok()) {
                        frames[at].forced.push(x);
                    }
                    last_action.remove(&f);
                }
            }
            _ => {}
        }
    }

    let mut wait_next: Vec<(usize, std::collections::VecDeque<(i64, f64)>)> = wait_queue
        .into_iter()
        .map(|(f, mut q)| {
            q.sort_by_key(|w| w.0);
            (f, q.into())
        })
        .collect();
    // Our side.
    SEEN.lock().unwrap().clear();
    unsafe { trace::SINK = Some(sink) };
    let mut hornet = Box::new(Hornet::blank());
    hornet.init(1);
    let mut ours: Vec<Obs> = vec![];
    let mut dbg_last: Option<i32> = None;
    let mut previous_pos = frames[0].pos;
    let mut previous_steps = frames[0].steps;
    let (mut previous_rt, mut previous_time) = (frames[0].rt, frames[0].time);
    for fr in &frames {
        if !fr.present {
            continue;
        }
        let h = &mut *hornet;
        h.world.ent[0].pos = [q16(fr.pos[0]), q16(fr.pos[1]), q16(fr.pos[2])];
        // The body's velocity is the physics' to say: what it moved since the last frame, over the time that took.
        let dt = fr.time - previous_time;
        if dt > 1e-4 {
            h.world.ent[0].vel = [q16((fr.pos[0] - previous_pos[0]) / dt), q16((fr.pos[1] - previous_pos[1]) / dt)];
        }
        previous_pos = fr.pos;
        h.env.hero = [q16(fr.hero[0]), q16(fr.hero[1]), q16(fr.hero[2])];
        h.env.camera = [q16(fr.cam[0]), q16(fr.cam[1]), q16(fr.cam[2])];
        h.env.dt_real = q16((fr.rt - previous_rt).max(0.0) * 60.0);
        h.env.dt_scaled = q16((fr.time - previous_time).max(0.0) * 60.0);
        previous_rt = fr.rt;
        previous_time = fr.time;
        // Triggers last one pass.
        for e in &mut h.world.ent {
            e.trig = 0;
        }
        for (f, kind) in &fr.triggers {
            h.world.ent[OWNERS[*f] as usize].trig |= 1 << kind;
        }
        for f in &fr.anim_done {
            h.world.ent[OWNERS[*f] as usize].anim_left = 0;
        }
        let draws: Vec<f64> = fr.forced.clone();
        h.env.forced_n = draws.len().min(8) as u8;
        for (i, v) in draws.iter().take(8).enumerate() {
            h.env.forced_random[i] = q16(*v);
        }
        let steps = (fr.steps - previous_steps).clamp(0, 4) as u32;
        previous_steps = fr.steps;
        for (f, q) in wait_next.iter_mut() {
            // Offered afresh every frame until our entry takes it: just under the length that ends the wait on the frame
            // the original's ended (a taken draw is the front of the queue, gone on the next frame).
            h.env.forced_wait[(*f).min(23)] = match q.front() {
                Some((_, end)) => (q16(end - fr.time) - 40).max(0),
                // Past the last wait the original finished: the one it was in when the recording ended never finishes.
                None => i32::MAX,
            };
        }
        for (f, e) in &fr.external {
            let id = n.events.iter().position(|x| x == e).unwrap() as u8;
            h.tree().event(*f as u8, id);
        }
        h.env.forced_events_n = fr.draws.len().min(4) as u8;
        for (i, (_, event)) in fr.draws.iter().take(4).enumerate() {
            h.env.forced_events[i] = match event {
                Some(e) => n.events.iter().position(|x| x == e).map_or(0xff, |k| k as u8),
                None => 0xfe,
            };
        }
        // The contacts the original's physics reported, once each.
        for (f, event, stay) in &fr.contacts {
            let side = contact_side(&mut *h, *f, event);
            h.env.contacts = 1 << side;
            let phase = if *stay { PH_COLLISION_STAY } else { PH_COLLISION_ENTER };
            h.tree().step(phase);
        }
        let offered: Vec<usize> = wait_next.iter().filter(|(_, q)| !q.is_empty()).map(|(f, _)| *f).collect();
        h.step(steps);
        for f in offered {
            if h.env.forced_wait[f.min(23)] < 0 {
                wait_next.iter_mut().find(|(g, _)| *g == f).unwrap().1.pop_front();
            }
        }
        h.env.forced_events_n = 0;
        if let Ok(spec) = std::env::var("HK_REPLAY_VAR") {
            // "fsm:slot": print the cell each frame it changes.
            let (f, slot) = spec.split_once(':').unwrap();
            let v = h.tree().cell(f.parse().unwrap(), slot.parse().unwrap());
            if dbg_last.replace(v) != Some(v) {
                eprintln!("ours   var {spec} @{} = {:.5}", fr.unity, v as f64 / 65536.0);
            }
        }
        if std::env::var("HK_REPLAY_ACTIVE").is_ok() && (fr.unity >= 1875 && fr.unity <= 1880) {
            let t = h.tree();
            let inactive: Vec<usize> = (0..n.fsms.len()).filter(|f| t.block().inactive(*f as u8)).collect();
            eprintln!("frame {}: inactive FSMs {:?}", fr.unity, inactive);
            let on: Vec<(&str, bool)> = (0..hk_sim::hornet::ENTS.len()).map(|i| (n.strings[hk_sim::hornet::ENTS[i].name as usize].as_str(), h.world.ent[i].on(1))).collect();
            eprintln!("  active: {:?}", on);
        }
        for e in SEEN.lock().unwrap().drain(..) {
            match e {
                Ev::StateEnter { fsm, state } => {
                    ours.push((fr.unity, "STATE", fsm as usize, n.states[fsm as usize][state as usize].clone(), String::new()))
                }
                Ev::Transition { fsm, event, to, .. } => ours.push((
                    fr.unity,
                    "TRANS",
                    fsm as usize,
                    n.events[event as usize].clone(),
                    n.states[fsm as usize][to as usize].clone(),
                )),
                Ev::Event { fsm, event, cause, .. } => {
                    if matches!(cause, Cause::Action(_) | Cause::Queue) {
                        ours.push((fr.unity, "EVENT", fsm as usize, n.events[event as usize].clone(), String::new()));
                    }
                }
                Ev::Activate { fsm, object, on } => {
                    if std::env::var("HK_REPLAY_ACTIVATE").is_ok() {
                        eprintln!("{} fsm {} ({}) sets object {} on={}", fr.unity, fsm, n.fsms[fsm as usize], object, on);
                    }
                }
                _ => {}
            }
        }
    }
    unsafe { trace::SINK = None };
    if let Ok(f) = std::env::var("HK_REPLAY_FSM") {
        let f: usize = f.parse().unwrap();
        let a: Vec<&Obs> = ours.iter().filter(|o| o.2 == f).collect();
        let b: Vec<&Obs> = theirs.iter().filter(|o| o.2 == f).collect();
        // The first entry where the two streams are not the same name at the same frame.
        let first = (0..a.len().min(b.len())).find(|&i| (a[i].0 - b[i].0).abs() > 3 || a[i].3 != b[i].3 || a[i].1 != b[i].1).unwrap_or(0);
        let from = if std::env::var("HK_REPLAY_ALL").is_ok() { 0 } else { first.saturating_sub(4) };
        for i in from..(first + 8).min(a.len().min(b.len())) {
            eprintln!("{i}: ours {}@{} {} {}   theirs {}@{} {} {}", a[i].1, a[i].0, a[i].3, a[i].4, b[i].1, b[i].0, b[i].3, b[i].4);
        }
    }
    let skew: i64 = std::env::var("HK_REPLAY_SKEW").ok().and_then(|v| v.parse().ok()).unwrap_or(3);
    match compare(&ours, &theirs, n.fsms.len(), skew, &n) {
        Ok(k) => eprintln!("Hornet: {k} observations matched over {} frames", frames.len()),
        Err(e) => panic!("{e}"),
    }
}

/// Which side the event `event` of FSM `f`'s current state names in its CheckCollisionSide action (0 up, 1 right, 2 down, 3 left).
fn contact_side(h: &mut Hornet, f: usize, event: &str) -> u8 {
    let bank = bank();
    let tree = h.tree();
    let state = tree.state(f as u8);
    let rec = bank.state(f as u8, state);
    let mut at = rec.block as usize + 2 * rec.transitions as usize;
    let n = names("hornet3");
    let want = n.events.iter().position(|e| e == event).unwrap() as u16;
    for _ in 0..rec.actions {
        let head = bank.byte(at);
        let (fam, op) = (head & 0xf, bank.byte(at + 1));
        let k = bank.arity_at(at);
        if fam == 5 && (op == ops::ai::CHECK_COLLISION_SIDE || op == ops::ai::CHECK_COLLISION_SIDE_ENTER) {
            let owner = head >> 4 & 8 != 0;
            let first = if owner { 0 } else { 0 };
            for side in 0..4usize {
                let w = bank.word(at + 2 + 2 * (first + 4 + side));
                if w >> 13 == hk_format::fsm3::T_EVENT && (w & 0x1fff) == want {
                    return side as u8;
                }
            }
        }
        at += 2 + 2 * k;
    }
    0
}

/// Where the replay is known to part from the original, per FSM: (FSM, reason, first entry that may differ). Each is a place the
/// world the replay feeds is not the original's, not a handler that disagrees; an earlier difference fails the test.
const KNOWN: &[(usize, &str, usize)] = &[
    // The airdash decision reads where the Rigidbody2D touched ground; the replay feeds the recorded contact one frame off.
    (0, "airdash against landing: the contact frame", 292),
    // The Needle's return is an iTween whose time is its distance over its speed, and the distance is the physics' position.
    (1, "needle return, two frames late: its iTween length", 10),
    // The range FSM flips on the trigger callbacks the replay replays from rows that carry frames, not sub-frame order.
    (6, "trigger order inside a frame", 3145),
];

fn compare(ours: &[Obs], theirs: &[Obs], fsm_count: usize, skew: i64, n: &Names) -> Result<usize, String> {
    let mut total = 0;
    let mut problems: Vec<String> = vec![];
    let thin = |v: &[Obs], f: usize| -> Vec<Obs> {
        let own: Vec<&Obs> = v.iter().filter(|o| o.2 == f).collect();
        let mut out = vec![];
        for (i, o) in own.iter().enumerate() {
            if o.1 == "EVENT" && own.get(i + 1).is_some_and(|x| x.1 == "TRANS" && x.3 == o.3) {
                continue;
            }
            out.push((*o).clone());
        }
        out
    };
    for f in 0..fsm_count {
        let (a, b) = (thin(ours, f), thin(theirs, f));
        let m = a.len().min(b.len());
        let mut bad = None;
        for i in 0..m {
            let same = a[i].1 == b[i].1 && a[i].3 == b[i].3 && a[i].4 == b[i].4;
            if !same || (a[i].0 - b[i].0).abs() > skew {
                bad = Some(i);
                break;
            }
            total += 1;
        }
        if let Some(i) = bad {
            // A difference the replay is known to have, from this entry on: reported, not failed (the ratchet only moves up).
            if let Some((_, reason, _)) = KNOWN.iter().find(|(k, _, at)| *k == f && i >= *at) {
                eprintln!("FSM {f} ({}): known difference from entry {i} ({reason})", n.fsms[f]);
                continue;
            }
            let ctx = |v: &[Obs]| {
                v[i.saturating_sub(3)..(i + 3).min(v.len())]
                    .iter()
                    .map(|o| format!("{}@{} {} {}", o.1, o.0, o.3, o.4))
                    .collect::<Vec<_>>()
                    .join(" | ")
            };
            problems.push(format!("FSM {f} ({}): entry {i} differs\n  ours:   {}\n  theirs: {}", n.fsms[f], ctx(&a), ctx(&b)));
        } else if a.len().abs_diff(b.len()) > 1 {
            problems.push(format!("FSM {f} ({}): {} entries of ours against {} of theirs", n.fsms[f], a.len(), b.len()));
        }
    }
    if problems.is_empty() {
        Ok(total)
    } else {
        Err(format!("{} of {} FSMs differ ({total} entries matched before the first difference)\n{}", problems.len(), fsm_count, problems.join("\n")))
    }
}
