//! Reading the install: clips, cues, and the clips each scene can play.

use crate::model::*;
use crate::summary;
use hk_unity::scene::build_settings;
use hk_unity::serialized::SerializedFile;
use hk_unity::{base_name, Obj, Source, Value};
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;

type R<T> = Result<T, String>;

fn e<T: std::fmt::Display>(x: T) -> String {
    x.to_string()
}

/// Everything the install says about audio.
pub struct World {
    pub clips: Vec<Clip>,
    pub cues: Vec<Cue>,
    pub atmos_cues: Vec<Atmos>,
    /// The persistent AudioManager's atmos channel clips, by channel.
    pub atmos_channels: Vec<Option<usize>>,
    pub scenes: Vec<SceneRec>,
}

#[derive(Default, Clone)]
pub struct SceneRec {
    pub index: usize,
    pub name: String,
    pub file: String,
    pub area: &'static str,
    pub loc: Option<Loc>,
    /// Clips the scene's objects can play, with the strongest way they got there.
    pub clips: BTreeMap<usize, bool>,
    /// Cue index to how the scene reaches it.
    pub cues: BTreeMap<usize, Reach>,
    pub atmos: Option<usize>,
    pub snapshot: String,
    /// (cue, mixer snapshot name) pairs the scene's manager and regions can apply.
    pub states: BTreeSet<(usize, String)>,
    pub cue_delay: f64,
    /// (kind, normalised owner name) to the clips found under it.
    pub owners: BTreeMap<(String, String), BTreeSet<usize>>,
    /// Loop AudioSource clips placed in the scene itself.
    pub loop_clips: BTreeSet<usize>,
    pub objects: usize,
    pub unread: usize,
}

fn pptrs(v: &Value, out: &mut Vec<(i32, i64)>) {
    match v {
        Value::Map(fields) => {
            if fields.len() == 2 {
                if let Some((f, p)) = v.pptr() {
                    if p != 0 {
                        out.push((f, p));
                    }
                    return;
                }
            }
            for (_, c) in fields {
                pptrs(c, out);
            }
        }
        Value::List(items) => {
            for c in items {
                pptrs(c, out);
            }
        }
        _ => {}
    }
}

fn get_pid(v: &Value, key: &str) -> i64 {
    v.get(key).and_then(Value::pptr).map(|p| p.1).unwrap_or(0)
}

fn data_files(source: &Source) -> R<Vec<String>> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(&source.directory).map_err(e)? {
        let n = entry.map_err(e)?.file_name().to_string_lossy().into_owned();
        let keep = n == "globalgamemanagers.assets"
            || n == "resources.assets"
            || (n.starts_with("sharedassets") && n.ends_with(".assets"));
        if keep {
            names.push(n);
        }
    }
    names.sort();
    Ok(names)
}

fn clip_of(source: &Source, obj: &Obj) -> R<Clip> {
    let t = source.read(obj).map_err(e)?;
    let file = base_name(&obj.file.name).to_string();
    let g = |k: &str| t.get(k);
    let stored = match g("m_Resource") {
        Some(r) if r.get("m_Size").and_then(Value::int).unwrap_or(0) > 0 => {
            r.get("m_Size").and_then(Value::int).unwrap_or(0) as u64
        }
        _ => match g("m_AudioData") {
            Some(Value::Bytes(b)) => b.len() as u64,
            _ => 0,
        },
    };
    Ok(Clip {
        key: format!("{file}:{}", obj.path_id()),
        name: g("m_Name").and_then(Value::str).unwrap_or_default(),
        file,
        path_id: obj.path_id(),
        channels: g("m_Channels").and_then(Value::int).unwrap_or(0),
        rate: g("m_Frequency").and_then(Value::int).unwrap_or(0),
        seconds: g("m_Length").and_then(Value::float).unwrap_or(0.0),
        stored,
    })
}

fn all_clips(source: &Source, files: &[String]) -> R<Vec<Clip>> {
    let per: Vec<R<Vec<Clip>>> = files
        .par_iter()
        .map(|name| {
            let file = source.file(name).map_err(e)?;
            let mut out = Vec::new();
            for info in file.objects.iter().filter(|o| o.class_id == 83) {
                let obj = Obj {
                    file: file.clone(),
                    info: *info,
                };
                out.push(clip_of(source, &obj)?);
            }
            Ok(out)
        })
        .collect();
    let mut clips = Vec::new();
    for p in per {
        clips.extend(p?);
    }
    Ok(clips)
}

struct Cues {
    cue_ix: HashMap<String, usize>,
    atmos_ix: HashMap<String, usize>,
}

/// Every MusicCue and AtmosCue in the shared files, and the AudioManager's atmos channels.
fn read_cues(
    source: &Source,
    files: &[String],
    clip_ix: &HashMap<(String, i64), usize>,
    clips: &[Clip],
) -> R<(Vec<Cue>, Vec<Atmos>, Vec<Option<usize>>, Cues)> {
    let found: Vec<R<Vec<(Obj, String)>>> = files
        .par_iter()
        .map(|name| {
            let file = source.file(name).map_err(e)?;
            let mut out = Vec::new();
            for info in file.objects.iter().filter(|o| o.class_id == 114) {
                let obj = Obj {
                    file: file.clone(),
                    info: *info,
                };
                if let Ok(s) = source.mono_script(&obj) {
                    if matches!(s.2.as_str(), "MusicCue" | "AtmosCue" | "AudioManager") {
                        out.push((obj, s.2.clone()));
                    }
                }
            }
            Ok(out)
        })
        .collect();
    let mut cues = Vec::new();
    let mut atmos = Vec::new();
    let mut idx = Cues {
        cue_ix: HashMap::new(),
        atmos_ix: HashMap::new(),
    };
    let mut manager: Option<(Obj, Value)> = None;
    let clip_of_pptr = |from: &Obj, p: &Value| -> Option<usize> {
        let o = source.deref(&from.file, p).ok()?;
        if o.class_id() != 83 {
            return None;
        }
        clip_ix
            .get(&(base_name(&o.file.name).to_string(), o.path_id()))
            .copied()
    };
    let mut atmos_objs = Vec::new();
    for f in found {
        for (obj, class) in f? {
            let tree = source.read(&obj).map_err(e)?;
            let key = obj.sid();
            match class.as_str() {
                "MusicCue" => {
                    let mut c = Cue {
                        key: key.clone(),
                        name: tree.get("m_Name").and_then(Value::str).unwrap_or_default(),
                        event: tree
                            .get("originalMusicEventName")
                            .and_then(Value::str)
                            .unwrap_or_default(),
                        ..Cue::default()
                    };
                    for info in tree
                        .get("channelInfos")
                        .and_then(Value::list)
                        .unwrap_or(&[])
                    {
                        let l = info.get("clip").and_then(|p| clip_of_pptr(&obj, p));
                        c.layers.push(l);
                    }
                    for alt in tree
                        .get("alternatives")
                        .and_then(Value::list)
                        .unwrap_or(&[])
                    {
                        if let Some(t) =
                            alt.get("Cue").and_then(|p| source.deref(&obj.file, p).ok())
                        {
                            c.alternatives.push(t.sid());
                        }
                    }
                    idx.cue_ix.insert(key, cues.len());
                    cues.push(c);
                }
                "AtmosCue" => {
                    idx.atmos_ix.insert(key.clone(), atmos.len());
                    atmos_objs.push(tree.clone());
                    atmos.push(Atmos {
                        name: tree.get("m_Name").and_then(Value::str).unwrap_or_default(),
                        channels: tree
                            .get("isChannelEnabled")
                            .and_then(Value::list)
                            .unwrap_or(&[])
                            .iter()
                            .enumerate()
                            .filter(|(_, v)| v.truthy())
                            .map(|(i, _)| i)
                            .collect(),
                    });
                }
                _ => manager = Some((obj, tree)),
            }
        }
    }
    let (mobj, mtree) = manager.ok_or("no AudioManager")?;
    let mut channels = Vec::new();
    for s in mtree
        .get("atmosSources")
        .and_then(Value::list)
        .unwrap_or(&[])
    {
        let clip = source
            .deref(&mobj.file, s)
            .ok()
            .and_then(|a| source.read(&a).ok().map(|t| (a, t)))
            .and_then(|(a, t)| {
                ["m_Resource", "m_audioClip"]
                    .iter()
                    .filter_map(|k| t.get(k))
                    .find_map(|p| clip_of_pptr(&a, p))
            });
        channels.push(clip);
    }
    let _ = clips;
    Ok((cues, atmos, channels, idx))
}

/// A node of the prefab graph reached from scene objects.
struct Node {
    clips: Vec<usize>,
    cue: Option<usize>,
    next: Vec<(u32, i64)>,
}

struct Graph<'a> {
    source: &'a Source,
    files: Vec<Arc<SerializedFile>>,
    ids: HashMap<String, u32>,
    nodes: HashMap<(u32, i64), Arc<Node>>,
    clip_ix: &'a HashMap<(String, i64), usize>,
    cues: &'a Cues,
    stop: &'static [&'static str],
    followed: HashMap<String, usize>,
    closure: HashMap<(u32, i64), Arc<(Vec<usize>, Vec<usize>)>>,
}

const FOLLOW: [i32; 5] = [1, 4, 82, 114, 224];

impl<'a> Graph<'a> {
    fn file_id(&mut self, file: &Arc<SerializedFile>) -> u32 {
        if let Some(i) = self.ids.get(&file.name) {
            return *i;
        }
        let i = self.files.len() as u32;
        self.files.push(file.clone());
        self.ids.insert(file.name.clone(), i);
        i
    }

    /// The (file id, path id) a PPtr written in `from` points at.
    fn resolve(&mut self, from: u32, file_id: i32, pid: i64) -> Option<(u32, i64)> {
        if file_id == 0 {
            return Some((from, pid));
        }
        let path = self.files[from as usize]
            .externals
            .get(file_id as usize - 1)?
            .path
            .clone();
        let f = self.source.file(&path).ok()?;
        Some((self.file_id(&f), pid))
    }

    fn node(&mut self, at: (u32, i64)) -> Option<Arc<Node>> {
        if let Some(n) = self.nodes.get(&at) {
            return Some(n.clone());
        }
        let file = self.files[at.0 as usize].clone();
        let info = *file.object(at.1)?;
        let obj = Obj {
            file: file.clone(),
            info,
        };
        let key = (base_name(&file.name).to_string(), at.1);
        let mut node = Node {
            clips: Vec::new(),
            cue: None,
            next: Vec::new(),
        };
        if info.class_id == 83 {
            if let Some(i) = self.clip_ix.get(&key) {
                node.clips.push(*i);
            }
        } else if FOLLOW.contains(&info.class_id) {
            let mut follow = true;
            if info.class_id == 114 {
                if let Ok(s) = self.source.mono_script(&obj) {
                    let sid = obj.sid();
                    if s.2 == "MusicCue" {
                        node.cue = self.cues.cue_ix.get(&sid).copied();
                        follow = false;
                    } else if s.2 == "AtmosCue" || self.stop.contains(&s.2.as_str()) {
                        follow = false;
                    }
                    *self.followed.entry(s.2.clone()).or_default() += follow as usize;
                } else {
                    follow = false;
                }
            }
            if follow {
                if let Ok(tree) = self.source.read(&obj) {
                    let mut raw = Vec::new();
                    pptrs(&tree, &mut raw);
                    for (f, p) in raw {
                        if let Some(t) = self.resolve(at.0, f, p) {
                            node.next.push(t);
                        }
                    }
                }
            }
        }
        let n = Arc::new(node);
        self.nodes.insert(at, n.clone());
        Some(n)
    }

    /// Every clip and cue reachable from `start`, through the followable
    /// classes. Cached per start node: a prefab many objects point at is walked once.
    fn reach(&mut self, start: (u32, i64)) -> Arc<(Vec<usize>, Vec<usize>)> {
        if let Some(r) = self.closure.get(&start) {
            return r.clone();
        }
        let mut seen: BTreeSet<(u32, i64)> = BTreeSet::new();
        let mut clips = BTreeSet::new();
        let mut cues = BTreeSet::new();
        let mut stack = vec![start];
        while let Some(at) = stack.pop() {
            if !seen.insert(at) {
                continue;
            }
            let Some(n) = self.node(at) else { continue };
            clips.extend(n.clips.iter().copied());
            cues.extend(n.cue);
            stack.extend(n.next.iter().copied());
        }
        let r = Arc::new((clips.into_iter().collect(), cues.into_iter().collect()));
        self.closure.insert(start, r.clone());
        r
    }
}

struct Obj2 {
    id: i64,
    class: i32,
    typename: String,
    tree: Value,
}

fn scan_scene(
    source: &Source,
    graph: &mut Graph,
    cues: &Cues,
    rec: &mut SceneRec,
    clip_ix: &HashMap<(String, i64), usize>,
) -> R<()> {
    let file = source.file(&rec.file).map_err(e)?;
    let fid = graph.file_id(&file);
    let objs: Vec<Obj> = file
        .objects
        .iter()
        .filter(|i| i.class_id != 43)
        .map(|i| Obj {
            file: file.clone(),
            info: *i,
        })
        .collect();
    let read: Vec<Option<Obj2>> = objs
        .par_iter()
        .map(|o| {
            let t = source.typename(o).ok()?;
            let v = source.read(o).ok()?;
            Some(Obj2 {
                id: o.path_id(),
                class: o.class_id(),
                typename: t,
                tree: v,
            })
        })
        .collect();
    rec.objects = objs.len();
    rec.unread = read.iter().filter(|r| r.is_none()).count();
    let read: Vec<Obj2> = read.into_iter().flatten().collect();

    // Hierarchy: GameObject name, parent GameObject, enemy and NPC marks.
    let mut go_name: HashMap<i64, String> = HashMap::new();
    let mut tf_go: HashMap<i64, i64> = HashMap::new();
    let mut tf_father: HashMap<i64, i64> = HashMap::new();
    let mut go_tf: HashMap<i64, i64> = HashMap::new();
    let mut enemy: BTreeSet<i64> = BTreeSet::new();
    let mut npc: BTreeSet<i64> = BTreeSet::new();
    for o in &read {
        match o.class {
            1 => {
                go_name.insert(
                    o.id,
                    o.tree
                        .get("m_Name")
                        .and_then(Value::str)
                        .unwrap_or_default(),
                );
            }
            4 | 224 => {
                let g = get_pid(&o.tree, "m_GameObject");
                tf_go.insert(o.id, g);
                go_tf.insert(g, o.id);
                tf_father.insert(o.id, get_pid(&o.tree, "m_Father"));
            }
            _ => {}
        }
        let g = get_pid(&o.tree, "m_GameObject");
        match o.typename.as_str() {
            "HealthManager" => {
                enemy.insert(g);
            }
            "PlayMakerFSM" => {
                let n = o
                    .tree
                    .get("fsm")
                    .and_then(|f| f.get("name"))
                    .and_then(Value::str)
                    .unwrap_or_default();
                if n == "Conversation Control" || n == "npc_control" || n == "Shop Control" {
                    npc.insert(g);
                }
            }
            _ => {}
        }
    }
    let parent_go = |g: i64| -> Option<i64> {
        let t = go_tf.get(&g)?;
        let f = tf_father.get(t)?;
        tf_go.get(f).copied()
    };
    let owner = |g: i64| -> (String, String) {
        let mut cur = Some(g);
        let mut hops = 0;
        while let Some(x) = cur {
            if enemy.contains(&x) {
                return (
                    "enemy".into(),
                    normalise(go_name.get(&x).map(String::as_str).unwrap_or("?")),
                );
            }
            hops += 1;
            if hops > 64 {
                break;
            }
            cur = parent_go(x);
        }
        let mut cur = Some(g);
        let mut hops = 0;
        while let Some(x) = cur {
            if npc.contains(&x) {
                return (
                    "npc".into(),
                    normalise(go_name.get(&x).map(String::as_str).unwrap_or("?")),
                );
            }
            hops += 1;
            if hops > 64 {
                break;
            }
            cur = parent_go(x);
        }
        (
            "object".into(),
            normalise(go_name.get(&g).map(String::as_str).unwrap_or("?")),
        )
    };

    let note_cue = |rec: &mut SceneRec, ix: usize, how: Reach| {
        let slot = rec.cues.entry(ix).or_insert(how);
        if how < *slot {
            *slot = how;
        }
    };
    for o in &read {
        if o.class == 1 {
            continue;
        }
        // Named music fields first, so the generic walk below finds them already filed.
        let cue_field = |key: &str| -> Option<usize> {
            let p = o.tree.get(key)?.pptr()?;
            if p.1 == 0 || p.0 == 0 {
                return None;
            }
            let path = file.externals.get(p.0 as usize - 1)?.path.clone();
            let f = source.file(&path).ok()?;
            let obj = source.object(&f, p.1).ok()?;
            cues.cue_ix.get(&obj.sid()).copied()
        };
        let snap_field = |key: &str| -> String {
            (|| {
                let p = o.tree.get(key)?.pptr()?;
                if p.1 == 0 || p.0 == 0 {
                    return None;
                }
                let path = file.externals.get(p.0 as usize - 1)?.path.clone();
                let f = source.file(&path).ok()?;
                let ob = source.object(&f, p.1).ok()?;
                source.read(&ob).ok()?.get("m_Name").and_then(Value::str)
            })()
            .unwrap_or_default()
        };
        match o.typename.as_str() {
            "SceneManager" => {
                if let Some(c) = cue_field("musicCue") {
                    note_cue(rec, c, Reach::SceneCue);
                    rec.states.insert((c, snap_field("musicSnapshot")));
                }
                rec.cue_delay = o
                    .tree
                    .get("musicDelayTime")
                    .and_then(Value::float)
                    .unwrap_or(0.0);
                if let Some(p) = o.tree.get("atmosCue").and_then(Value::pptr) {
                    if p.0 != 0 && p.1 != 0 {
                        if let Some(path) = file.externals.get(p.0 as usize - 1) {
                            if let Ok(f) = source.file(&path.path) {
                                if let Ok(ob) = source.object(&f, p.1) {
                                    rec.atmos = cues.atmos_ix.get(&ob.sid()).copied();
                                }
                            }
                        }
                    }
                }
                if let Some(p) = o.tree.get("musicSnapshot").and_then(Value::pptr) {
                    if p.0 != 0 && p.1 != 0 {
                        if let Some(path) = file.externals.get(p.0 as usize - 1) {
                            if let Ok(f) = source.file(&path.path) {
                                if let Ok(ob) = source.object(&f, p.1) {
                                    rec.snapshot = source
                                        .read(&ob)
                                        .ok()
                                        .and_then(|t| t.get("m_Name").and_then(Value::str))
                                        .unwrap_or_default();
                                }
                            }
                        }
                    }
                }
            }
            "MusicRegion" => {
                for (k, sk) in [
                    ("enterMusicCue", "enterMusicSnapshot"),
                    ("exitMusicCue", "exitMusicSnapshot"),
                ] {
                    if let Some(c) = cue_field(k) {
                        note_cue(rec, c, Reach::Region);
                        rec.states.insert((c, snap_field(sk)));
                    }
                }
            }
            _ => {}
        }
        let mut raw = Vec::new();
        pptrs(&o.tree, &mut raw);
        if raw.is_empty() {
            continue;
        }
        let g = get_pid(&o.tree, "m_GameObject");
        let own = if g != 0 {
            owner(g)
        } else {
            ("object".into(), o.typename.clone())
        };
        let is_loop_source =
            o.typename == "AudioSource" && o.tree.get("Loop").is_some_and(Value::truthy);
        let mut clips = BTreeSet::new();
        let mut cue_set = BTreeSet::new();
        for (f, p) in raw {
            if f == 0 {
                continue; // inside the scene file: hierarchy links, never a shared asset
            }
            let Some(t) = graph.resolve(fid, f, p) else {
                continue;
            };
            let r = graph.reach(t);
            cue_set.extend(r.1.iter().copied());
            let direct = graph.files[t.0 as usize]
                .object(t.1)
                .map(|i| i.class_id == 83)
                .unwrap_or(false);
            for &c in &r.0 {
                let slot = rec.clips.entry(c).or_insert(direct);
                *slot |= direct;
                clips.insert(c);
            }
        }
        for c in cue_set {
            note_cue(
                rec,
                c,
                if o.typename == "PlayMakerFSM" || o.typename != "SceneManager" {
                    Reach::Script
                } else {
                    Reach::Indirect
                },
            );
        }
        if !clips.is_empty() {
            if is_loop_source {
                rec.loop_clips.extend(clips.iter().copied());
            }
            rec.owners.entry(own).or_default().extend(clips);
        }
    }
    let _ = clip_ix;
    Ok(())
}

pub fn run(dir: &str, out: &Path) -> R<()> {
    let source = Source::new(dir).map_err(e)?;
    let files = data_files(&source)?;
    eprintln!("{} data files", files.len());
    let clips = all_clips(&source, &files)?;
    eprintln!("{} AudioClips", clips.len());
    let mut clip_ix: HashMap<(String, i64), usize> = HashMap::new();
    for (i, c) in clips.iter().enumerate() {
        clip_ix.insert((c.file.clone(), c.path_id), i);
    }
    let (cues, atmos_cues, atmos_channels, cue_ix) = read_cues(&source, &files, &clip_ix, &clips)?;
    eprintln!("{} MusicCues, {} AtmosCues", cues.len(), atmos_cues.len());

    let settings = build_settings(&source).map_err(e)?;
    let mut order: Vec<(usize, String, String)> = settings
        .iter()
        .map(|(n, f)| {
            (
                f.trim_start_matches("level")
                    .parse::<usize>()
                    .unwrap_or(usize::MAX),
                n.clone(),
                f.clone(),
            )
        })
        .collect();
    order.sort();
    let limit: usize = std::env::var("CENSUS_LIMIT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(usize::MAX);
    let only: Option<String> = std::env::var("CENSUS_SCENE").ok();
    let mut graph = Graph {
        source: &source,
        files: Vec::new(),
        ids: HashMap::new(),
        nodes: HashMap::new(),
        clip_ix: &clip_ix,
        cues: &cue_ix,
        stop: &[
            "AudioManager",
            "GameManager",
            "UIManager",
            "GameCameras",
            "HeroController",
            "PlayerData",
            "StaticVariableList",
            "SceneManager",
            "CustomSceneManager",
        ],
        followed: HashMap::new(),
        closure: HashMap::new(),
    };
    let mut scenes = Vec::new();
    let t0 = std::time::Instant::now();
    for (index, name, file) in order.into_iter().take(limit) {
        if let Some(o) = &only {
            if &name != o {
                continue;
            }
        }
        let mut rec = SceneRec {
            index,
            area: area_of(&name),
            loc: Some(loc_of(&name)),
            name: name.clone(),
            file,
            ..SceneRec::default()
        };
        if let Err(err) = scan_scene(&source, &mut graph, &cue_ix, &mut rec, &clip_ix) {
            eprintln!("{name}: {err}");
        }
        eprintln!(
            "{:>3} {:<34} {:>5.0}s objs {:>6} clips {:>4} cues {} nodes {}",
            index,
            name,
            t0.elapsed().as_secs_f64(),
            rec.objects,
            rec.clips.len(),
            rec.cues.len(),
            graph.nodes.len()
        );
        scenes.push(rec);
    }
    let mut followed: Vec<_> = graph.followed.iter().collect();
    followed.sort_by_key(|x| std::cmp::Reverse(*x.1));
    eprintln!(
        "followed script classes (top): {:?}",
        &followed[..followed.len().min(25)]
    );

    let world = World {
        clips,
        cues,
        atmos_cues,
        atmos_channels,
        scenes,
    };
    std::fs::create_dir_all(out).map_err(e)?;
    summary::write(&world, out)
}
