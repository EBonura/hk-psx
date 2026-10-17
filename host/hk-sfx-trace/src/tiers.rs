//! Split every scene's reachable scene-local sounds into an area tier (shared by
//! many rooms of the area, loaded at area gates) and a room tier (the rest, loaded
//! with the room), and size each at four rate policies.
//!
//! Clips longer than 6 s are left out (stream candidates).
//!
//! Policies: `port` the port's 5,512 Hz one-shot rate, `m20` and `m30` the lowest
//! SDK ladder rate that leaves at most -20 / -30 dB of the clip above its Nyquist
//! (`rates`), `full` the 22,050 Hz cap. A clip with no rate measurement (longer
//! than the rates cutoff) counts at the cap.

use crate::csv::tsv;
use crate::rates::adpcm_bytes;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

const POLICIES: [&str; 5] = ["port", "m15", "m20", "m30", "full"];
const NP: usize = 5;
/// A scene-local clip longer than this is a stream candidate (XA), not an SPU bank entry.
const STREAM_SECS: f64 = 6.0;

/// (scene, area, location, total, area-tier and room-tier bytes per policy)
type RoomRow = (String, String, String, [u64; NP], [u64; NP], [u64; NP]);

struct Clip {
    name: String,
    secs: f64,
    rate: i64,
    cat: String,
    m15: i64,
    m20: i64,
    m30: i64,
}

impl Clip {
    fn bytes(&self, policy: usize) -> u64 {
        let r = match policy {
            0 => 5512,
            1 => self.m15,
            2 => self.m20,
            3 => self.m30,
            _ => 22050,
        };
        adpcm_bytes(self.secs, r, self.rate)
    }
}

fn pct(v: &mut [u64], p: f64) -> u64 {
    if v.is_empty() {
        return 0;
    }
    v.sort_unstable();
    v[(((v.len() - 1) as f64) * p).round() as usize]
}

pub fn run(census: &str, rates: &str, out: &str) -> Result<(), String> {
    let census = Path::new(census);
    let mut measured: BTreeMap<String, (i64, i64, i64)> = BTreeMap::new();
    for r in tsv(Path::new(rates))? {
        if r.len() >= 13 {
            measured.insert(
                r[0].clone(),
                (
                    r[9].parse().unwrap_or(22050),
                    r[10].parse().unwrap_or(22050),
                    r[11].parse().unwrap_or(22050),
                ),
            );
        }
    }
    let mut clips: BTreeMap<String, Clip> = BTreeMap::new(); // by key
    for r in tsv(&census.join("clips.tsv"))? {
        if r.len() < 8 {
            continue;
        }
        let (m15, m20, m30) = measured
            .get(&r[0])
            .copied()
            .unwrap_or((22050, 22050, 22050));
        clips.insert(
            r[0].clone(),
            Clip {
                name: r[1].clone(),
                secs: r[4].parse().unwrap_or(0.0),
                rate: r[3].parse().unwrap_or(44100),
                cat: r[7].clone(),
                m15,
                m20,
                m30,
            },
        );
    }
    // scene -> (area, location)
    let mut scenes: BTreeMap<String, (String, String)> = BTreeMap::new();
    for r in tsv(&census.join("scenes.tsv"))? {
        if r.len() >= 5 {
            scenes.insert(r[1].clone(), (r[2].clone(), r[4].clone()));
        }
    }
    // scene -> local clip keys (Local and Wide: the persistent set is its own tier)
    let mut reach: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for r in tsv(&census.join("scene_clips.tsv"))? {
        if r.len() < 5 {
            continue;
        }
        if let Some(c) = clips.get(&r[1]) {
            if matches!(c.cat.as_str(), "Local" | "Wide") && c.secs <= STREAM_SECS {
                reach.entry(r[0].clone()).or_default().insert(r[1].clone());
            }
        }
    }
    let mut areas: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (s, (a, l)) in &scenes {
        if l == "traversal" || l == "boss arena" {
            areas.entry(a.clone()).or_default().push(s.clone());
        }
    }
    let pol = |prefix: &str| {
        POLICIES
            .iter()
            .map(|p| format!("{prefix}@{p}"))
            .collect::<Vec<_>>()
            .join("\t")
    };
    let stat = |prefix: &str| {
        POLICIES
            .iter()
            .flat_map(|p| ["med", "p90", "max"].map(|q| format!("{prefix}_{q}@{p}")))
            .collect::<Vec<_>>()
            .join("\t")
    };
    let mut by_scene = format!(
        "# scene\tarea\tlocation\troom_clips\t{}\t{}\t{}\n",
        pol("total_B"),
        pol("area_B"),
        pol("room_B")
    );
    let mut by_area = format!(
        "# area\ttraversal_rooms\tarena_rooms\tarea_set_clips\t{}\t{}\t{}\n",
        pol("area_B"),
        stat("trav_room"),
        stat("arena_room")
    );
    let mut room_rows: Vec<RoomRow> = Vec::new();
    for (area, rooms) in &areas {
        let n_trav = rooms.iter().filter(|s| scenes[*s].1 == "traversal").count();
        let need = 2.max(n_trav.div_ceil(4));
        let mut count: BTreeMap<&String, usize> = BTreeMap::new();
        for s in rooms {
            for k in reach.get(s).into_iter().flatten() {
                *count.entry(k).or_default() += 1;
            }
        }
        let area_set: BTreeSet<&String> = count
            .iter()
            .filter(|(_, n)| **n >= need)
            .map(|(k, _)| *k)
            .collect();
        let mut a_bytes = [0u64; NP];
        for k in &area_set {
            for (p, b) in a_bytes.iter_mut().enumerate() {
                *b += clips[*k].bytes(p);
            }
        }
        let (mut trav, mut arena): (Vec<[u64; NP]>, Vec<[u64; NP]>) = (vec![], vec![]);
        for s in rooms {
            let (mut tot, mut ar, mut ro) = ([0u64; NP], [0u64; NP], [0u64; NP]);
            let (mut n_area, mut n_room) = (0, 0);
            for k in reach.get(s).into_iter().flatten() {
                let in_area = area_set.contains(k);
                if in_area {
                    n_area += 1
                } else {
                    n_room += 1
                }
                for p in 0..NP {
                    let b = clips[k].bytes(p);
                    tot[p] += b;
                    if in_area {
                        ar[p] += b
                    } else {
                        ro[p] += b
                    }
                }
            }
            let _ = n_area;
            let _ = n_room;
            if scenes[s].1 == "traversal" {
                trav.push(ro)
            } else {
                arena.push(ro)
            }
            room_rows.push((s.clone(), area.clone(), scenes[s].1.clone(), tot, ar, ro));
        }
        let col = |v: &[[u64; NP]], p: usize| -> Vec<u64> { v.iter().map(|x| x[p]).collect() };
        let mut line = format!("{area}\t{n_trav}\t{}\t{}", arena.len(), area_set.len());
        for b in a_bytes {
            let _ = write!(line, "\t{b}");
        }
        for set in [&trav, &arena] {
            for p in 0..NP {
                for q in [0.5, 0.9, 1.0] {
                    let _ = write!(line, "\t{}", pct(&mut col(set, p), q));
                }
            }
        }
        let _ = writeln!(by_area, "{line}");
    }
    for (s, a, l, tot, ar, ro) in &room_rows {
        let n = reach.get(s).map(|x| x.len()).unwrap_or(0);
        let mut line = format!("{s}\t{a}\t{l}\t{n}");
        for set in [tot, ar, ro] {
            for b in set {
                let _ = write!(line, "\t{b}");
            }
        }
        let _ = writeln!(by_scene, "{line}");
    }
    // the persistent tier at each policy
    let mut pers = [0u64; NP];
    let mut names = Vec::new();
    for c in clips.values().filter(|c| c.cat == "Persistent") {
        for (p, b) in pers.iter_mut().enumerate() {
            *b += c.bytes(p);
        }
        names.push(c.name.as_str());
    }
    let _ = writeln!(
        by_area,
        "# persistent tier: {} clips; bytes {} = {:?}",
        names.len(),
        POLICIES.join("/"),
        pers
    );
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    std::fs::write(Path::new(out).join("tiers_area.tsv"), by_area).map_err(|e| e.to_string())?;
    std::fs::write(Path::new(out).join("tiers_scene.tsv"), by_scene).map_err(|e| e.to_string())?;
    eprintln!("{} areas, {} rooms", areas.len(), room_rows.len());
    Ok(())
}
