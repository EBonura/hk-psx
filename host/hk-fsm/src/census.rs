//! Corpus access for the FSM census: every `HealthManager` placement in the
//! original data and the PlayMaker FSMs on its GameObject subtree.
//!
//! The row index (which GameObjects carry a HealthManager or a PlayMakerFSM) comes
//! from the `hk-feature-census` cache `source-objects.jsonl.gz`; the Transform
//! hierarchy and the FSM graphs are read straight from the Windows Unity files
//! with `hk-unity`, so nothing here depends on a Python reader.
use hk_unity::{Source, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// One `HealthManager` GameObject and the FSMs found on its subtree.
#[derive(Clone, Debug)]
pub struct Root {
    pub level: String,
    pub scene: String,
    pub go: i64,
    pub name: String,
    /// GameObjects in the subtree, the root included.
    pub ngo: usize,
    /// (owning GameObject, source id `file:path_id`, FSM name) in subtree walk order.
    pub fsms: Vec<(i64, String, String)>,
}

impl Root {
    /// The "main game" filter of DESIGN.md section 1.1.
    pub fn is_main(&self) -> bool {
        !(self.scene.starts_with("GG_") || self.scene.starts_with("Dream") || self.scene.starts_with('@'))
    }
}

struct Row {
    level: String,
    scene: String,
    go: i64,
    name: String,
    sid: String,
    fsm_name: String,
}

fn str_of(v: &serde_json::Value, key: &str) -> String {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("").to_owned()
}

/// Stream the cache through the system gzip (the convention of tools/census) and
/// keep only the two component kinds the census needs, in file order.
fn rows(cache: &Path) -> Result<(Vec<Row>, Vec<Row>)> {
    let mut child = Command::new("gzip").arg("-dc").arg(cache).stdout(Stdio::piped()).spawn()?;
    let reader = BufReader::with_capacity(1 << 20, child.stdout.take().ok_or("gzip stdout")?);
    let (mut hm, mut fsm) = (vec![], vec![]);
    for line in reader.lines() {
        let line = line?;
        // Keys are sorted, so the component name opens the object (compact or spaced).
        let head = line.get(..48).unwrap_or(&line).replace(": ", ":");
        let kind = if head.starts_with("{\"component\":\"HealthManager\"") {
            0
        } else if head.starts_with("{\"component\":\"PlayMakerFSM\"") {
            1
        } else {
            continue;
        };
        let v: serde_json::Value = serde_json::from_str(&line)?;
        let row = Row {
            level: str_of(&v, "level"),
            scene: str_of(&v, "scene"),
            go: v.get("game_object_id").and_then(|x| x.as_i64()).ok_or("game_object_id")?,
            name: str_of(&v, "game_object_name"),
            sid: str_of(&v, "sid"),
            fsm_name: str_of(&v, "fsm_name"),
        };
        if kind == 0 { hm.push(row) } else { fsm.push(row) }
    }
    if !child.wait()?.success() {
        return Err("gzip failed: corrupt or truncated cache".into());
    }
    Ok((hm, fsm))
}

/// Child GameObjects of every GameObject of one file, through the Transform tree.
pub fn hierarchy(source: &Source, level: &str) -> Result<HashMap<i64, Vec<i64>>> {
    let file = source.file(level)?;
    // File object order is the child order, as the Python reader had it.
    let mut father = vec![];
    let mut go_of = HashMap::new();
    let mut tr_of_go = HashMap::new();
    for info in &file.objects {
        if info.class_id != 4 && info.class_id != 224 {
            continue;
        }
        let obj = source.object(&file, info.path_id)?;
        let Ok(tree) = source.read(&obj) else { continue };
        let g = tree.get("m_GameObject").and_then(|p| p.pptr()).map(|p| p.1).unwrap_or(0);
        let f = tree.get("m_Father").and_then(|p| p.pptr()).map(|p| p.1).unwrap_or(0);
        father.push((info.path_id, f));
        go_of.insert(info.path_id, g);
        tr_of_go.insert(g, info.path_id);
    }
    // transform id -> child GameObjects
    let mut children: HashMap<i64, Vec<i64>> = HashMap::new();
    for (tr, f) in &father {
        if go_of.contains_key(f) {
            children.entry(*f).or_default().push(go_of[tr]);
        }
    }
    let mut by_go = HashMap::new();
    for (g, tr) in &tr_of_go {
        by_go.insert(*g, children.remove(tr).unwrap_or_default());
    }
    Ok(by_go)
}

/// The PlayMakerFSM components on the given GameObjects: `(level, game object) -> [(sid, FSM name)]`.
pub fn fsms_on(cache: &Path, wanted: &[(&str, i64)]) -> Result<HashMap<(String, i64), Vec<(String, String)>>> {
    let (_, fsm) = rows(cache)?;
    let mut out: HashMap<(String, i64), Vec<(String, String)>> = HashMap::new();
    for r in fsm {
        if wanted.iter().any(|(l, g)| *l == r.level && *g == r.go) {
            out.entry((r.level, r.go)).or_default().push((r.sid, r.fsm_name));
        }
    }
    Ok(out)
}

/// All HealthManager placements, levels in sorted order, placements in cache order.
pub fn load_roots(source: &Source, cache: &Path, threads: usize) -> Result<Vec<Root>> {
    let (hm, fsm) = rows(cache)?;
    // A GameObject with two HealthManager components is one placement.
    let hm = {
        let mut seen = std::collections::HashSet::new();
        hm.into_iter().filter(|r| seen.insert((r.level.clone(), r.go))).collect::<Vec<_>>()
    };
    let mut fsm_on: HashMap<(String, i64), Vec<(String, String)>> = HashMap::new();
    for r in fsm {
        fsm_on.entry((r.level, r.go)).or_default().push((r.sid, r.fsm_name));
    }
    let mut levels: Vec<String> = hm.iter().map(|r| r.level.clone()).collect();
    levels.sort();
    levels.dedup();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let out = std::sync::Mutex::new(BTreeMap::<String, Vec<Root>>::new());
    std::thread::scope(|s| {
        for _ in 0..threads.max(1) {
            s.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let Some(level) = levels.get(i) else { break };
                let kids = match hierarchy(source, level) {
                    Ok(k) => k,
                    Err(e) => {
                        eprintln!("skip {level}: {e}");
                        continue;
                    }
                };
                let mut roots = vec![];
                for r in hm.iter().filter(|r| &r.level == level) {
                    let mut seen = vec![];
                    let mut stack = vec![r.go];
                    while let Some(g) = stack.pop() {
                        seen.push(g);
                        if let Some(c) = kids.get(&g) {
                            stack.extend(c.iter().copied());
                        }
                    }
                    let mut fsms = vec![];
                    for g in &seen {
                        for (sid, name) in fsm_on.get(&(level.clone(), *g)).into_iter().flatten() {
                            fsms.push((*g, sid.clone(), name.clone()));
                        }
                    }
                    roots.push(Root { level: level.clone(), scene: r.scene.clone(), go: r.go, name: r.name.clone(), ngo: seen.len(), fsms });
                }
                out.lock().unwrap().insert(level.clone(), roots);
            });
        }
    });
    Ok(out.into_inner().unwrap().into_values().flatten().collect())
}

/// What the census keeps of one PlayMaker FSM.
#[derive(Clone, Debug)]
pub struct Fsm {
    pub name: String,
    pub states: Vec<StateSummary>,
    /// Variable count per `*Variables` group.
    pub vars: BTreeMap<String, usize>,
    pub events: usize,
    pub globals: usize,
    /// Hash of name, state and action order (with enable flags) and event names.
    pub def: String,
}

#[derive(Clone, Debug)]
pub struct StateSummary {
    pub name: String,
    pub transitions: usize,
    /// Short names of the enabled actions, in authored order.
    pub enabled: Vec<String>,
    pub disabled: usize,
}

impl Fsm {
    pub fn var_total(&self) -> usize {
        self.vars.values().sum()
    }
    pub fn enabled_total(&self) -> usize {
        self.states.iter().map(|s| s.enabled.len()).sum()
    }
    pub fn max_state_actions(&self) -> usize {
        self.states.iter().map(|s| s.enabled.len()).max().unwrap_or(0)
    }
}

pub fn summarize(fsm: &Value) -> Result<Fsm> {
    let name = fsm.get("name").and_then(Value::str).unwrap_or_default();
    let mut h = Sha256::new();
    h.update(name.as_bytes());
    let mut states = vec![];
    for st in fsm.get("states").and_then(Value::list).ok_or("states")? {
        let sname = st.get("name").and_then(Value::str).unwrap_or_default();
        h.update([0]);
        h.update(sname.as_bytes());
        let d = st.get("actionData");
        let names = d.and_then(|d| d.get("actionNames")).and_then(Value::list).unwrap_or(&[]);
        let on = d.and_then(|d| d.get("actionEnabled")).and_then(Value::list).unwrap_or(&[]);
        let mut enabled = vec![];
        let mut disabled = 0;
        for (i, n) in names.iter().enumerate() {
            let full = n.str().unwrap_or_default();
            let short = full.rsplit('.').next().unwrap_or("").to_owned();
            let en = on.get(i).is_some_and(Value::truthy);
            h.update([1, en as u8]);
            h.update(short.as_bytes());
            if en { enabled.push(short) } else { disabled += 1 }
        }
        states.push(StateSummary {
            name: sname,
            transitions: st.get("transitions").and_then(Value::list).map_or(0, <[_]>::len),
            enabled,
            disabled,
        });
    }
    let events = fsm.get("events").and_then(Value::list).unwrap_or(&[]);
    for e in events {
        h.update([2]);
        h.update(e.get("name").and_then(Value::str).unwrap_or_default().as_bytes());
    }
    let mut vars = BTreeMap::new();
    if let Some(Value::Map(groups)) = fsm.get("variables") {
        for (k, v) in groups {
            if k.ends_with("Variables") {
                if let Some(l) = v.list() {
                    vars.insert(k.to_string(), l.len());
                }
            }
        }
    }
    Ok(Fsm {
        name,
        states,
        vars,
        events: events.len(),
        globals: fsm.get("globalTransitions").and_then(Value::list).map_or(0, <[_]>::len),
        def: format!("{:x}", h.finalize())[..16].to_owned(),
    })
}

/// Read and summarize every distinct source id, files in parallel.
pub fn load_fsms(source: &Source, sids: &[String], threads: usize) -> Result<(HashMap<String, Fsm>, Vec<(String, String)>)> {
    let mut by_file: BTreeMap<&str, Vec<(&String, i64)>> = BTreeMap::new();
    for sid in sids {
        let (f, p) = sid.rsplit_once(':').ok_or("sid")?;
        by_file.entry(f).or_default().push((sid, p.parse()?));
    }
    let files: Vec<_> = by_file.into_iter().collect();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let out = std::sync::Mutex::new((HashMap::new(), vec![]));
    std::thread::scope(|s| {
        for _ in 0..threads.max(1) {
            s.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let Some((f, items)) = files.get(i) else { break };
                let file = match source.file(f) {
                    Ok(x) => x,
                    Err(e) => {
                        out.lock().unwrap().1.push((f.to_string(), e.to_string()));
                        continue;
                    }
                };
                for (sid, pid) in items {
                    let r = (|| -> Result<Fsm> {
                        let obj = source.object(&file, *pid)?;
                        let tree = source.read(&obj)?;
                        summarize(tree.get("fsm").ok_or("fsm")?)
                    })();
                    let mut o = out.lock().unwrap();
                    match r {
                        Ok(v) => {
                            o.0.insert((*sid).clone(), v);
                        }
                        Err(e) => o.1.push(((*sid).clone(), e.to_string())),
                    }
                }
            });
        }
    });
    Ok(out.into_inner().unwrap())
}
