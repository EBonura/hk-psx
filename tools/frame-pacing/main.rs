//! Frame pacing of replay runs (the frontend's `route.csv` plus the run's
//! `command.json` watch map, as written by the work-dir replay harness).
//!
//! Counts only gameplay ticks: `HK_GAME_MODE` at its most common value and
//! `HK_ROOM_LOAD_STATE` at its most common (resident) value, so title screens,
//! loads and gates are left out. A presented frame is the run of vblanks
//! between two display flips; the report gives presented fps, the longest
//! frame in vblanks, and how many frames took more than two (the 30 fps bar).
//!
//! Usage: hk-frame-pacing RUN_DIR [RUN_DIR...]   (one JSON line per run)
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

fn mode<'a>(values: impl Iterator<Item = &'a str>) -> String {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for v in values {
        *counts.entry(v).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by_key(|&(_, n)| n)
        .map(|(v, _)| v.to_string())
        .unwrap_or_default()
}

fn main() {
    for run in std::env::args().skip(1) {
        let dir = Path::new(&run);
        let command: Value =
            serde_json::from_str(&fs::read_to_string(dir.join("command.json")).unwrap()).unwrap();
        let column = |name: &str| {
            command["watch"][name]
                .as_str()
                .map(|a| format!("ram_{:0>8}", a.trim_start_matches("0x")))
        };
        let text = fs::read_to_string(dir.join("route.csv")).unwrap();
        let mut lines = text.lines();
        let header: Vec<&str> = lines.next().unwrap().split(',').collect();
        let at = |name: &str| header.iter().position(|h| *h == name);
        let (flip, game, load) = (
            at("display_start_changed").unwrap(),
            column("HK_GAME_MODE").and_then(|c| at(&c)),
            column("HK_ROOM_LOAD_STATE").and_then(|c| at(&c)),
        );
        let rows: Vec<Vec<&str>> = lines.map(|l| l.split(',').collect()).collect();
        let play = game.map(|g| mode(rows.iter().map(|r| r[g])));
        let ready = load.map(|l| mode(rows.iter().map(|r| r[l])));
        let gameplay = |r: &Vec<&str>| {
            game.map_or(true, |g| Some(r[g]) == play.as_deref())
                && load.map_or(true, |l| Some(r[l]) == ready.as_deref())
        };
        let (mut ticks, mut flips, mut longest, mut over, mut since) =
            (0u64, 0u64, 0u64, 0u64, 0u64);
        let mut histogram: HashMap<u64, u64> = HashMap::new();
        for r in &rows {
            if !gameplay(r) {
                since = 0;
                continue;
            }
            ticks += 1;
            since += 1;
            if r[flip] == "1" {
                flips += 1;
                if since > 0 {
                    longest = longest.max(since);
                    if since > 2 {
                        over += 1;
                    }
                    *histogram.entry(since).or_default() += 1;
                }
                since = 0;
            }
        }
        let mut hist: Vec<(u64, u64)> = histogram.into_iter().collect();
        hist.sort();
        println!(
            "{}",
            json!({
                "run": dir.file_name().unwrap().to_string_lossy(), "gameplay_ticks": ticks, "frames": flips,
                "fps": if ticks > 0 { (flips as f64 / ticks as f64 * 59.94 * 1000.0).round() / 1000.0 } else { 0.0 },
                "longest_vblanks": longest, "frames_over_2_vblanks": over,
                "vblanks_per_frame": hist.iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>(),
            })
        );
    }
}
