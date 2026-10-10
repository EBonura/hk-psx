//! Per-frame traces of the original, as `tools/hkref scenes` writes them.
//!
//! A run directory holds one `a<NN>` attempt directory per original process (a crashed or
//! time-boxed process is followed by another that resumes). A scene's idle window is the
//! `idle` row of its `survey-timeline.csv`; `actors.csv` has one row per active HealthManager per
//! test frame, `camera.csv` the hero and camera, `survey-scenes.csv` the scene's status.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

type Row = BTreeMap<String, String>;

fn split_csv(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for ch in line.chars() {
        match ch {
            '"' => quoted = !quoted,
            ',' if !quoted => out.push(std::mem::take(&mut cur)),
            _ => cur.push(ch),
        }
    }
    out.push(cur);
    out
}

pub fn table(path: &Path) -> Vec<Row> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut lines = text.lines();
    let Some(header) = lines.next() else {
        return Vec::new();
    };
    let head = split_csv(header);
    lines
        .filter_map(|l| {
            let cells = split_csv(l);
            (cells.len() >= head.len()).then(|| head.iter().cloned().zip(cells).collect())
        })
        .collect()
}

fn num(r: &Row, k: &str) -> f64 {
    r.get(k).and_then(|v| v.parse().ok()).unwrap_or(f64::NAN)
}

#[derive(Clone, Debug)]
pub struct Sample {
    pub frame: i64,
    pub x: f64,
    pub y: f64,
    pub hp: i32,
    pub dead: bool,
    pub fsm: String,
}

#[derive(Clone, Debug)]
pub struct OgActor {
    pub id: String,
    pub name: String,
    pub samples: Vec<Sample>,
}

/// One stretch of a scene's run: the idle settle, or the hero's visit to one enemy.
#[derive(Clone, Debug)]
pub struct Window {
    /// A visit to an enemy (hero beside it or a strike on it), as opposed to the idle settle.
    pub tour: bool,
    /// The window of a strike series on `target`.
    pub poke: bool,
    /// The Knight walking up to `target` from a distance.
    pub approach: bool,
    pub x: f64,
    pub y: f64,
    pub target: String,
    pub start: i64,
    pub end: i64,
}

#[derive(Clone, Debug)]
pub struct SceneTrace {
    pub scene: String,
    /// First frame of the idle window; the scene was loaded some frames before it.
    pub first_frame: i64,
    /// First frame the camera was traced in this scene (the load), which replay starts from.
    pub origin: i64,
    pub last_frame: i64,
    pub windows: Vec<Window>,
    pub face: BTreeMap<i64, i32>,
    /// Camera position by frame (`camera_x`, `camera_y`, `camera_z`).
    pub camera: BTreeMap<i64, [f64; 3]>,
    pub hero: BTreeMap<i64, [f64; 2]>,
    pub actors: Vec<OgActor>,
}

/// The attempt directories of a survey run, oldest first.
pub fn attempts(run: &Path) -> Vec<PathBuf> {
    let base = run.join("og-survey");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&base)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .map_or(false, |n| n.to_string_lossy().starts_with('a'))
                })
                .collect()
        })
        .unwrap_or_default();
    dirs.sort();
    dirs
}

/// One nail strike the original's driver landed on an enemy (`survey-pokes.csv`).
#[derive(Clone, Debug)]
pub struct PokeHit {
    pub scene: String,
    pub target: String,
    pub id: String,
    pub frame: i64,
    pub hp_before: i32,
    pub hp_after: i32,
    pub dead: bool,
}

/// Every strike of every finished scene of a poke run, in the order they were struck.
pub fn load_pokes(run: &Path) -> Vec<PokeHit> {
    let mut out = Vec::new();
    for dir in attempts(run) {
        for r in table(&dir.join("survey-pokes.csv")) {
            out.push(PokeHit {
                scene: r["scene"].clone(),
                target: r["target"].clone(),
                id: r["id"].clone(),
                frame: num(&r, "frame") as i64,
                hp_before: num(&r, "hp_before") as i32,
                hp_after: num(&r, "hp_after") as i32,
                dead: r.get("dead").map_or(false, |d| d == "1"),
            });
        }
    }
    out
}

/// Every finished scene of a survey run, keyed by scene name (a later attempt wins).
pub fn load_run(run: &Path) -> BTreeMap<String, SceneTrace> {
    let mut out = BTreeMap::new();
    for dir in attempts(run) {
        let finished: Vec<String> = table(&dir.join("survey-scenes.csv"))
            .into_iter()
            .filter(|r| r.get("status").map_or(false, |s| s.starts_with("ok")))
            .map(|r| r["scene"].clone())
            .collect();
        if finished.is_empty() {
            continue;
        }
        let mut windows: BTreeMap<String, Vec<Window>> = BTreeMap::new();
        for r in table(&dir.join("survey-timeline.csv")) {
            windows.entry(r["scene"].clone()).or_default().push(Window {
                tour: matches!(
                    r.get("phase").map(String::as_str),
                    Some("tour") | Some("poke") | Some("approach")
                ),
                approach: r.get("phase").map(String::as_str) == Some("approach"),
                poke: r.get("phase").map(String::as_str) == Some("poke"),
                x: num(&r, "x"),
                y: num(&r, "y"),
                target: r.get("target").cloned().unwrap_or_default(),
                start: num(&r, "start_frame") as i64,
                end: num(&r, "end_frame") as i64,
            });
        }
        let mut traces: BTreeMap<String, SceneTrace> = finished
            .iter()
            .filter_map(|s| {
                let w = windows.remove(s)?;
                let first = w.iter().map(|x| x.start).min()?;
                let last = w.iter().map(|x| x.end).max()?;
                Some((
                    s.clone(),
                    SceneTrace {
                        scene: s.clone(),
                        first_frame: first,
                        origin: first,
                        last_frame: last,
                        windows: w,
                        face: BTreeMap::new(),
                        camera: BTreeMap::new(),
                        hero: BTreeMap::new(),
                        actors: Vec::new(),
                    },
                ))
            })
            .collect();
        for r in table(&dir.join("camera.csv")) {
            let Some(t) = r.get("scene").and_then(|s| traces.get_mut(s)) else {
                continue;
            };
            let f = num(&r, "test_frame") as i64;
            if f >= 0 && f < t.last_frame {
                t.origin = t.origin.min(f);
                t.camera.insert(
                    f,
                    [
                        num(&r, "camera_x"),
                        num(&r, "camera_y"),
                        num(&r, "camera_z"),
                    ],
                );
                t.hero.insert(f, [num(&r, "hero_x"), num(&r, "hero_y")]);
                t.face.insert(
                    f,
                    if r.get("facing_right").map_or(true, |v| v == "True") {
                        1
                    } else {
                        -1
                    },
                );
            }
        }
        let mut by_id: BTreeMap<(String, String), OgActor> = BTreeMap::new();
        for r in table(&dir.join("actors.csv")) {
            let Some(t) = r.get("scene").and_then(|s| traces.get(s)) else {
                continue;
            };
            let f = num(&r, "test_frame") as i64;
            if f < t.first_frame || f >= t.last_frame {
                continue;
            }
            let id = r["id"].clone();
            by_id
                .entry((r["scene"].clone(), id.clone()))
                .or_insert_with(|| OgActor {
                    id,
                    name: r["name"].clone(),
                    samples: Vec::new(),
                })
                .samples
                .push(Sample {
                    frame: f,
                    x: num(&r, "x"),
                    y: num(&r, "y"),
                    hp: num(&r, "hp") as i32,
                    dead: r.get("dead").map_or(false, |d| d == "1"),
                    fsm: r.get("fsms").cloned().unwrap_or_default(),
                });
        }
        for ((scene, _), actor) in by_id {
            if let Some(t) = traces.get_mut(&scene) {
                t.actors.push(actor);
            }
        }
        out.extend(traces);
    }
    out
}
