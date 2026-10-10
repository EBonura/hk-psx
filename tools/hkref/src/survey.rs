//! Scene survey: ground truth from the original for a list of scenes. The driver
//! (mod/SceneSurvey.cs) visits each scene, records what it holds and tours the
//! hero past each distinct enemy; this module runs it (resuming across player
//! crashes) and folds the raw per-process files into per-scene data files.

use crate::og::{self, table};
use crate::profile::Profile;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

type Row = BTreeMap<String, String>;

fn q(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "'"))
    } else {
        s.to_string()
    }
}
fn line(f: &[&str]) -> String {
    f.iter().map(|x| q(x)).collect::<Vec<_>>().join(",") + "\n"
}
/// Per (operation, clip): count, windows, callsite and hierarchy.
type AudioAgg = (usize, BTreeSet<String>, String, String);

fn g<'a>(r: &'a Row, k: &str) -> &'a str {
    r.get(k).map(String::as_str).unwrap_or("")
}

pub fn run(p: &Profile, work: &Path) -> Result<(), String> {
    if p.sweep.is_empty() {
        return Err("profile needs og.sweep (scene names or Prefix_* patterns, or *)".into());
    }
    let env = og::Env::detect(work);
    let attempts = og::survey(&env, p)?;
    summarise(p, &attempts)
}

/// Fold the attempt directories into the output files in the profile's run dir.
pub fn summarise(p: &Profile, attempts: &[PathBuf]) -> Result<(), String> {
    // Each attempt's tables are parsed once, not once per scene.
    let mut cache: BTreeMap<PathBuf, std::rc::Rc<Vec<Row>>> = BTreeMap::new();
    let mut load = |path: PathBuf| -> std::rc::Rc<Vec<Row>> {
        cache
            .entry(path.clone())
            .or_insert_with(|| std::rc::Rc::new(table(&path)))
            .clone()
    };
    // Which attempt finished each scene (the last one wins: a scene cut short by a
    // watchdog stop is redone by the next process and its partial rows are ignored).
    let mut owner: BTreeMap<String, (usize, Row)> = BTreeMap::new();
    for (i, a) in attempts.iter().enumerate() {
        for r in table(&a.join("survey-scenes.csv")) {
            owner.insert(g(&r, "scene").to_string(), (i, r));
        }
    }
    let mut crashed: BTreeMap<String, String> = BTreeMap::new();
    if let Ok(t) = fs::read_to_string(p.dir.join("og-survey/state/survey-done.txt")) {
        for l in t.lines() {
            let c: Vec<&str> = l.split('\t').collect();
            if c.len() == 2 && c[1] == "crash" {
                crashed.insert(c[0].to_string(), c[1].to_string());
            }
        }
    }
    let mut actors_raw = String::from("scene,id,name,path,x,y,z,hp,active_in_hierarchy,active_self,enemy_type,fsms,clips,fsm_states\n");
    let mut fsm_raw = String::from(
        "scene,kind,object,fsm,state,action,clips_or_objects,params,incoming_events\n",
    );
    let mut src_raw = String::from("scene,object,clip,loop,play_on_awake,volume,spatial_blend,min_distance,max_distance,mixer_group,active_in_hierarchy,enabled\n");
    let mut sfx_csv = String::from("scene,op,clip,calls,windows,callsite,object\n");
    let mut actors_csv = String::from("scene,actor,instances,active_at_load,hp,enemy_type,positions,start_states,states_seen,all_states,static_clips\n");
    let mut scenes_csv = String::from("scene,status,load_frames,exits,actor_types,actors,sfx_clips_played,fsm_audio_actions,audio_sources\n");
    let mut md = format!("# Scene survey: {}\n\n| scene | status | exits | actor types | actors | clips played | static FSM audio | audio sources |\n|---|---|---|---|---|---|---|---|\n", p.name);
    let (mut n_ok, mut n_bad) = (0, 0);
    for (scene, (ai, srow)) in &owner {
        let dir = &attempts[*ai];
        let status = g(srow, "status");
        if status.starts_with("ok") {
            n_ok += 1
        } else {
            n_bad += 1
        }
        // census rows
        let actors: Vec<Row> = load(dir.join("survey-actors.csv"))
            .iter()
            .filter(|r| g(r, "scene") == scene)
            .cloned()
            .collect();
        for r in &actors {
            actors_raw.push_str(&line(
                &[
                    "scene",
                    "id",
                    "name",
                    "path",
                    "x",
                    "y",
                    "z",
                    "hp",
                    "active_in_hierarchy",
                    "active_self",
                    "enemy_type",
                    "fsms",
                    "clips",
                    "fsm_states",
                ]
                .map(|k| g(r, k)),
            ));
        }
        let fsm: Vec<Row> = load(dir.join("survey-fsm-audio.csv"))
            .iter()
            .filter(|r| g(r, "scene") == scene)
            .cloned()
            .collect();
        for r in &fsm {
            fsm_raw.push_str(&line(
                &[
                    "scene",
                    "kind",
                    "object",
                    "fsm",
                    "state",
                    "action",
                    "clips_or_objects",
                    "params",
                    "incoming_events",
                ]
                .map(|k| g(r, k)),
            ));
        }
        let srcs: Vec<Row> = load(dir.join("survey-audiosources.csv"))
            .iter()
            .filter(|r| g(r, "scene") == scene)
            .cloned()
            .collect();
        for r in &srcs {
            src_raw.push_str(&line(
                &[
                    "scene",
                    "object",
                    "clip",
                    "loop",
                    "play_on_awake",
                    "volume",
                    "spatial_blend",
                    "min_distance",
                    "max_distance",
                    "mixer_group",
                    "active_in_hierarchy",
                    "enabled",
                ]
                .map(|k| g(r, k)),
            ));
        }
        // actor types: group census rows by name stripped of its instance suffix, add the
        // FSM states the periodic samples saw
        let samples = load(dir.join("actors.csv"));
        let mut seen_by_id: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for s in samples.iter().filter(|s| g(s, "scene") == scene) {
            let set = seen_by_id.entry(g(s, "id").to_string()).or_default();
            for st in g(s, "fsms").split(';').filter(|x| !x.is_empty()) {
                set.insert(st.to_string());
            }
        }
        let mut types: BTreeMap<String, Vec<&Row>> = BTreeMap::new();
        for r in &actors {
            types.entry(base_name(g(r, "name"))).or_default().push(r);
        }
        for (name, list) in &types {
            let active = list
                .iter()
                .filter(|r| g(r, "active_in_hierarchy") == "1")
                .count();
            let pos: Vec<String> = list
                .iter()
                .take(8)
                .map(|r| {
                    format!(
                        "{:.1} {:.1}",
                        g(r, "x").parse::<f64>().unwrap_or(0.0),
                        g(r, "y").parse::<f64>().unwrap_or(0.0)
                    )
                })
                .collect();
            let start: BTreeSet<&str> = list
                .iter()
                .flat_map(|r| g(r, "fsms").split(';'))
                .filter(|x| !x.is_empty())
                .collect();
            let mut seen: BTreeSet<String> = BTreeSet::new();
            for r in list {
                if let Some(s) = seen_by_id.get(g(r, "id")) {
                    seen.extend(s.iter().cloned());
                }
            }
            let clips: BTreeSet<&str> = list
                .iter()
                .flat_map(|r| g(r, "clips").split('|'))
                .filter(|x| !x.is_empty())
                .collect();
            actors_csv.push_str(&line(&[
                scene,
                name,
                &list.len().to_string(),
                &active.to_string(),
                g(list[0], "hp"),
                g(list[0], "enemy_type"),
                &pos.join(" | "),
                &start.into_iter().collect::<Vec<_>>().join(" "),
                &seen.into_iter().collect::<Vec<_>>().join(" "),
                g(list[0], "fsm_states"),
                &clips.into_iter().collect::<Vec<_>>().join("|"),
            ]));
        }
        // sounds: every Play / PlayOneShot / snapshot transition inside the scene's windows
        let tl: Vec<(i64, i64, String)> = load(dir.join("survey-timeline.csv"))
            .iter()
            .filter(|r| g(r, "scene") == scene)
            .map(|r| {
                let label = if g(r, "phase") == "idle" {
                    "idle".to_string()
                } else {
                    format!("near {}", g(r, "target"))
                };
                (
                    g(r, "start_frame").parse().unwrap_or(0),
                    g(r, "end_frame").parse().unwrap_or(0),
                    label,
                )
            })
            .collect();
        let mut agg: BTreeMap<(String, String), AudioAgg> = BTreeMap::new();
        if let (Some(lo), Some(hi)) = (tl.iter().map(|t| t.0).min(), tl.iter().map(|t| t.1).max()) {
            for r in load(dir.join("audio-calls.csv")).iter() {
                let f: i64 = g(r, "queued_test_frame").parse().unwrap_or(-1);
                let op = g(r, "operation");
                if f < lo
                    || f >= hi
                    || !matches!(
                        op,
                        "Play" | "PlayOneShot" | "PlayClipAtPoint" | "TransitionTo"
                    )
                {
                    continue;
                }
                let win = tl
                    .iter()
                    .find(|t| f >= t.0 && f < t.1)
                    .map(|t| t.2.clone())
                    .unwrap_or_else(|| "other".into());
                let clip = if g(r, "clip_or_snapshot").is_empty() {
                    format!("(source clip) {}", g(r, "hierarchy"))
                } else {
                    g(r, "clip_or_snapshot").to_string()
                };
                let e = agg.entry((op.to_string(), clip)).or_insert((
                    0,
                    BTreeSet::new(),
                    g(r, "callsite").to_string(),
                    g(r, "hierarchy").to_string(),
                ));
                e.0 += 1;
                e.1.insert(win);
            }
        }
        for ((op, clip), (n, wins, site, obj)) in &agg {
            sfx_csv.push_str(&line(&[
                scene,
                op,
                clip,
                &n.to_string(),
                &wins.iter().cloned().collect::<Vec<_>>().join(" | "),
                site,
                obj,
            ]));
        }
        let audio_actions = fsm.iter().filter(|r| g(r, "kind") == "audio").count();
        scenes_csv.push_str(&line(&[
            scene,
            status,
            g(srow, "load_frames"),
            g(srow, "gates"),
            &types.len().to_string(),
            &actors.len().to_string(),
            &agg.len().to_string(),
            &audio_actions.to_string(),
            &srcs.len().to_string(),
        ]));
        let exits = g(srow, "gates")
            .split(';')
            .filter(|x| !x.is_empty())
            .count();
        md.push_str(&format!(
            "| {scene} | {status} | {exits} | {} | {} | {} | {audio_actions} | {} |\n",
            types.len(),
            actors.len(),
            agg.len(),
            srcs.len()
        ));
    }
    for s in crashed.keys() {
        if !owner.contains_key(s) {
            scenes_csv.push_str(&line(&[s, "crash", "", "", "", "", "", "", ""]));
            md.push_str(&format!("| {s} | crash (player died) | | | | | | |\n"));
            n_bad += 1;
        }
    }
    md.push_str(&format!("\n{} scenes surveyed ok, {} with a problem (timeouts, no hero, crashes).\n\nFiles: `scenes.csv` (status, exits), `actors.csv` (types, positions, FSM states), `actors-all.csv` (every HealthManager), `sfx.csv` (clips played at runtime, with window and call site), `fsm-audio.csv` (clips and spawned objects every FSM action can reach, with state and incoming events), `audiosources.csv`.\n", n_ok, n_bad));
    for (name, data) in [
        ("actors-all.csv", actors_raw),
        ("fsm-audio.csv", fsm_raw),
        ("audiosources.csv", src_raw),
        ("sfx.csv", sfx_csv),
        ("actors.csv", actors_csv),
        ("scenes.csv", scenes_csv),
        ("scenes.md", md.clone()),
    ] {
        fs::write(p.dir.join(name), data).map_err(|e| e.to_string())?;
    }
    println!("{md}");
    Ok(())
}

/// "Crawler (2)" and "Fly 3" are instances of "Crawler" and "Fly".
fn base_name(n: &str) -> String {
    let mut s = n.replace("(Clone)", "").trim().to_string();
    if s.ends_with(')') {
        if let Some(i) = s.rfind(" (") {
            s.truncate(i);
        }
    }
    let t = s.trim_end_matches(|c: char| c.is_ascii_digit()).to_string();
    if t.len() < s.len() && t.ends_with(' ') {
        s = t.trim_end().to_string();
    }
    s.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn instance_suffixes_are_stripped() {
        assert_eq!(base_name("Crawler (2)"), "Crawler");
        assert_eq!(base_name("Fly 3"), "Fly");
        assert_eq!(base_name("Zombie Runner"), "Zombie Runner");
        assert_eq!(base_name("Mosquito(Clone)"), "Mosquito");
        assert_eq!(base_name("Buzzer 12"), "Buzzer");
    }
}
