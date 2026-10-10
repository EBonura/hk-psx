//! A run profile: one scene scenario, described once, driving both games.

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Watch {
    pub chan: String,
    pub symbol: String,
    pub scale: f64,
    pub signed: bool,
}

#[derive(Clone, Debug)]
pub struct EnemyMatch {
    pub pattern: String,
    pub chan: String,
    pub events: Vec<(String, String)>,
}

#[derive(Clone, Debug)]
pub struct Profile {
    pub name: String,
    pub dir: PathBuf,
    pub raw: Value,
    // port
    pub disc: PathBuf,
    pub map: PathBuf,
    pub frontend: PathBuf,
    pub card: Option<PathBuf>,
    pub tape: Option<PathBuf>,
    pub events: Option<String>,
    pub polls: usize,
    pub shot_interval: u32,
    pub watch: Vec<Watch>,
    pub counters: Vec<(String, String)>,
    // window (sim ticks, port clock)
    pub start_tick: Option<i64>,
    pub ticks: usize,
    // original
    pub scene: String,
    pub gate: String,
    pub player_data: String,
    pub hide: Vec<String>,
    pub enemies: Vec<EnemyMatch>,
    pub og_timeout: u64,
    pub fx_off: Vec<String>,
    pub seed: u32,
    pub sweep: Vec<String>,
    pub sweep_frames: usize,
    pub tour_frames: usize,
    pub tour_targets: usize,
}

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or("").to_string()
}
/// A path in a profile: `~/` is the home directory, a relative path is relative
/// to the hk-psx checkout root, an absolute path is taken as is.
fn expand(x: &str) -> PathBuf {
    if let Some(r) = x.strip_prefix("~/") {
        return PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(r);
    }
    let p = PathBuf::from(x);
    if p.is_absolute() {
        p
    } else {
        crate::og::repo_root().join(p)
    }
}
fn p(v: &Value, k: &str) -> Option<PathBuf> {
    v.get(k)
        .and_then(Value::as_str)
        .filter(|x| !x.is_empty())
        .map(expand)
}

impl Profile {
    pub fn load(path: &Path, work: &Path) -> Result<Profile, String> {
        let raw: Value = serde_json::from_str(
            &fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?,
        )
        .map_err(|e| e.to_string())?;
        let port = raw.get("port").ok_or("profile needs a 'port' object")?;
        let og = raw.get("og").ok_or("profile needs an 'og' object")?;
        let win = raw.get("window").cloned().unwrap_or(Value::Null);
        let name = s(&raw, "name");
        if name.is_empty() {
            return Err("profile needs a name".into());
        }
        let watch = port
            .get("watch")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .map(|w| Watch {
                        chan: s(w, "chan"),
                        symbol: s(w, "symbol"),
                        scale: w.get("scale").and_then(Value::as_f64).unwrap_or(1.0),
                        signed: w.get("signed").and_then(Value::as_bool).unwrap_or(false),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let counters = port
            .get("counters")
            .and_then(Value::as_array)
            .map(|a| a.iter().map(|c| (s(c, "symbol"), s(c, "event"))).collect())
            .unwrap_or_default();
        let enemies = og
            .get("enemies")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .map(|e| EnemyMatch {
                        pattern: s(e, "match"),
                        chan: s(e, "chan"),
                        events: e
                            .get("events")
                            .and_then(Value::as_object)
                            .map(|m| {
                                m.iter()
                                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                                    .collect()
                            })
                            .unwrap_or_default(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let hide = og
            .get("hide")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        Ok(Profile {
            dir: work.join("runs").join(&name),
            name,
            raw: raw.clone(),
            disc: p(port, "disc").ok_or("port.disc")?,
            map: p(port, "map").ok_or("port.map")?,
            frontend: p(port, "frontend")
                .or_else(|| std::env::var("HKREF_FRONTEND").ok().map(PathBuf::from))
                .ok_or(
                    "set port.frontend or HKREF_FRONTEND to the PSoXide headless frontend binary",
                )?,
            card: p(port, "card"),
            tape: p(port, "tape"),
            events: port.get("events").and_then(Value::as_str).map(String::from),
            polls: port
                .get("polls")
                .and_then(Value::as_u64)
                .ok_or("port.polls")? as usize,
            shot_interval: port
                .get("shot_interval")
                .and_then(Value::as_u64)
                .unwrap_or(0) as u32,
            watch,
            counters,
            start_tick: win.get("start").and_then(Value::as_i64),
            ticks: win.get("ticks").and_then(Value::as_u64).unwrap_or(600) as usize,
            scene: s(og, "scene"),
            gate: s(og, "gate"),
            player_data: s(og, "player_data"),
            hide,
            enemies,
            fx_off: og
                .get("fx_off")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            sweep: og
                .get("sweep")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            sweep_frames: og.get("sweep_frames").and_then(Value::as_u64).unwrap_or(60) as usize,
            tour_frames: og.get("tour_frames").and_then(Value::as_u64).unwrap_or(75) as usize,
            tour_targets: og.get("tour_targets").and_then(Value::as_u64).unwrap_or(12) as usize,
            seed: og.get("seed").and_then(Value::as_u64).unwrap_or(1) as u32,
            og_timeout: og.get("timeout").and_then(Value::as_u64).unwrap_or(300),
        })
    }
}
