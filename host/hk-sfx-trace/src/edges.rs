//! What a gate really costs: the sounds of the room being entered that the room being
//! left does not already have resident (a delta load), against loading the whole
//! reachable set (flat) or only the room-specific remainder (the area-tier model).
//!
//! Edges come from the survey's exits (`scenes.csv`: `gate@x y->Scene:entry;`). Both
//! rooms must be traversal rooms of the census. Scene-local sounds only (Local and
//! Wide); the persistent set stays resident throughout.

use crate::csv::{table, tsv};
use crate::rates::adpcm_bytes;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

const POLICIES: [&str; 3] = ["port", "m15", "m20"];

pub fn run(census: &str, rates: &str, survey_scenes: &str, out: &str) -> Result<(), String> {
    let census = Path::new(census);
    let mut min: BTreeMap<String, (i64, i64)> = BTreeMap::new();
    for r in tsv(Path::new(rates))? {
        if r.len() >= 13 {
            min.insert(
                r[0].clone(),
                (
                    r[9].parse().unwrap_or(22050),
                    r[10].parse().unwrap_or(22050),
                ),
            );
        }
    }
    // key -> (category, seconds, source rate, m15, m20)
    let mut clips: BTreeMap<String, (String, f64, i64, i64, i64)> = BTreeMap::new();
    for r in tsv(&census.join("clips.tsv"))? {
        if r.len() >= 8 {
            let (a, b) = min.get(&r[0]).copied().unwrap_or((22050, 22050));
            clips.insert(
                r[0].clone(),
                (
                    r[7].clone(),
                    r[4].parse().unwrap_or(0.0),
                    r[3].parse().unwrap_or(44100),
                    a,
                    b,
                ),
            );
        }
    }
    let bytes = |k: &str, p: usize| -> u64 {
        let (_, secs, src, m15, m20) = &clips[k];
        adpcm_bytes(*secs, [5512, *m15, *m20][p], *src)
    };
    let mut loc: BTreeMap<String, String> = BTreeMap::new();
    for r in tsv(&census.join("scenes.tsv"))? {
        if r.len() >= 5 {
            loc.insert(r[1].clone(), r[4].clone());
        }
    }
    let mut reach: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for r in tsv(&census.join("scene_clips.tsv"))? {
        if r.len() >= 5
            && clips
                .get(&r[1])
                .is_some_and(|c| matches!(c.0.as_str(), "Local" | "Wide") && c.1 <= 6.0)
        {
            reach.entry(r[0].clone()).or_default().insert(r[1].clone());
        }
    }
    let mut text = format!(
        "# from\tto\treach_to_B@port\treach_to_B@m15\treach_to_B@m20\tdelta_B@port\tdelta_B@m15\tdelta_B@m20\n"
    );
    let mut deltas: Vec<[u64; 3]> = Vec::new();
    let mut flats: Vec<[u64; 3]> = Vec::new();
    let empty = BTreeSet::new();
    for row in table(Path::new(survey_scenes)) {
        let from = row.get("scene").cloned().unwrap_or_default();
        if loc.get(&from).map(String::as_str) != Some("traversal") {
            continue;
        }
        let mut seen = BTreeSet::new();
        for exit in row
            .get("exits")
            .map(String::as_str)
            .unwrap_or("")
            .split(';')
        {
            let Some((_, dest)) = exit.split_once("->") else {
                continue;
            };
            let Some((to, _)) = dest.split_once(':') else {
                continue;
            };
            if to.is_empty()
                || !seen.insert(to.to_string())
                || loc.get(to).map(String::as_str) != Some("traversal")
            {
                continue;
            }
            let (a, b) = (
                reach.get(&from).unwrap_or(&empty),
                reach.get(to).unwrap_or(&empty),
            );
            let (mut flat, mut delta) = ([0u64; 3], [0u64; 3]);
            for k in b {
                for p in 0..3 {
                    let n = bytes(k, p);
                    flat[p] += n;
                    if !a.contains(k) {
                        delta[p] += n;
                    }
                }
            }
            let _ = writeln!(
                text,
                "{from}\t{to}\t{}\t{}\t{}\t{}\t{}\t{}",
                flat[0], flat[1], flat[2], delta[0], delta[1], delta[2]
            );
            deltas.push(delta);
            flats.push(flat);
        }
    }
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    std::fs::write(Path::new(out).join("edges_delta.tsv"), &text).map_err(|e| e.to_string())?;
    let pct = |v: &[[u64; 3]], p: usize, q: f64| -> u64 {
        let mut c: Vec<u64> = v.iter().map(|x| x[p]).collect();
        c.sort_unstable();
        c.get(((c.len().max(1) - 1) as f64 * q).round() as usize)
            .copied()
            .unwrap_or(0)
    };
    let mut s = format!("# {} traversal edges\n# policy\tflat_med\tflat_p90\tflat_max\tdelta_med\tdelta_p90\tdelta_max\n", deltas.len());
    for (p, name) in POLICIES.iter().enumerate() {
        let _ = writeln!(
            s,
            "{name}\t{}\t{}\t{}\t{}\t{}\t{}",
            pct(&flats, p, 0.5),
            pct(&flats, p, 0.9),
            pct(&flats, p, 1.0),
            pct(&deltas, p, 0.5),
            pct(&deltas, p, 0.9),
            pct(&deltas, p, 1.0)
        );
    }
    std::fs::write(Path::new(out).join("edges_summary.tsv"), &s).map_err(|e| e.to_string())?;
    eprintln!("{} edges", deltas.len());
    Ok(())
}
