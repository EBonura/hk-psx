//! Numbers the cooked actor catalogue carries, against the source records it was cooked from.
//!
//! This is the cheap layer under the trace comparison: hit points, contact damage, recoil and
//! the invincibility flags of every placement the guest has a controller for, read from the
//! cooked `ActorSpec` and from the source component dump in `regions.json`. The cook asserts most
//! of these as it writes them; this restates them from the other side so a regression in either
//! shows up here by family.
use super::*;
use crate::compare::family_of;
use crate::world_data::{data_dir, load_scene};
use serde_json::Value;
use std::collections::BTreeMap;

pub fn run(names: &BTreeMap<usize, String>) {
    let text = std::fs::read_to_string(data_dir().join("regions.json")).expect("regions.json");
    let json: Value = serde_json::from_str(&text).expect("regions.json parses");
    // (scene id, source id) -> source record
    let mut source: BTreeMap<(usize, u32), &Value> = BTreeMap::new();
    for region in json["regions"].as_array().unwrap() {
        let scene = region["scene_id"].as_u64().unwrap() as usize;
        for a in region["actors"].as_array().unwrap() {
            let id = a["source"]
                .as_str()
                .and_then(|s| s.rsplit(':').next())
                .and_then(|n| n.parse::<u32>().ok());
            if let Some(id) = id {
                source.entry((scene, id)).or_insert(a);
            }
        }
    }
    let mut rows: BTreeMap<String, [u32; 8]> = BTreeMap::new();
    let mut fails = Vec::new();
    for scene in names.keys() {
        let mut seen = Vec::new();
        for region in load_scene(*scene) {
            for (p, spec) in region.actors {
                if seen.contains(&p.source_id) {
                    continue;
                }
                seen.push(p.source_id);
                let Some(src) = source.get(&(*scene, p.source_id)) else {
                    rows.entry(family_of(spec)).or_default()[7] += 1;
                    continue;
                };
                let fam = family_of(spec);
                let row = rows.entry(fam.clone()).or_default();
                row[0] += 1;
                let hm = &src["health_manager"];
                let hp_ok = hm["hp"].as_i64() == Some(spec.health.health as i64);
                let dmg = src["DamageHero"]["damageDealt"].as_u64().unwrap_or(0) as u16;
                let dmg_ok = dmg == spec.health.contact_damage;
                let rec = &src["Recoil"];
                let (rs, rd) = (
                    rec["recoilSpeedBase"].as_f64(),
                    rec["recoilDuration"].as_f64(),
                );
                let rec_speed_ok = match rs {
                    Some(v) => (v * 65536.0).round() as i32 == spec.recoil_speed,
                    None => spec.recoil_speed == 0,
                };
                let rec_ticks_ok = match rd {
                    Some(v) => (v * 60.0).round() as u16 == spec.recoil_ticks,
                    None => spec.recoil_ticks == 0,
                };
                let flags_ok = (hm["invincible"].as_i64().unwrap_or(0) != 0)
                    == spec.health.invincible
                    && (hm["damageOverride"].as_i64().unwrap_or(0) != 0)
                        == spec.health.damage_override;
                // The body box: the source's first collider, relative to the actor, either way round
                // (a mirrored placement flips x), to within two Q16 units.
                let bounds_ok = match (
                    src["colliders"][0]["bounds"].as_array(),
                    src["position"].as_array(),
                ) {
                    (Some(b), Some(pos)) => {
                        let v: Vec<f64> =
                            b.iter().map(|n| n.as_f64().unwrap_or(f64::NAN)).collect();
                        let (px, py) = (
                            pos[0].as_f64().unwrap_or(f64::NAN),
                            pos[1].as_f64().unwrap_or(f64::NAN),
                        );
                        let rel = [v[0] - px, v[1] - py, v[2] - px, v[3] - py];
                        let got = spec.bounds.map(|n| n as f64 / 65536.0);
                        let near = |a: [f64; 4], b: [f64; 4]| {
                            a.iter()
                                .zip(b)
                                .all(|(x, y)| (x - y).abs() < 2.5 / 65536.0 + 1e-4)
                        };
                        near(got, rel)
                            || near(got, [-rel[2], rel[1], -rel[0], rel[3]])
                            || near(got, [rel[0], -rel[3], rel[2], -rel[1]])
                            || near(got, [-rel[3], rel[0], -rel[1], rel[2]])
                    }
                    _ => true,
                };
                for (i, ok) in [
                    hp_ok,
                    dmg_ok,
                    rec_speed_ok,
                    rec_ticks_ok,
                    flags_ok,
                    bounds_ok,
                ]
                .into_iter()
                .enumerate()
                {
                    row[1 + i] += ok as u32;
                }
                if !(hp_ok && dmg_ok && rec_speed_ok && rec_ticks_ok && flags_ok && bounds_ok) {
                    fails.push(format!(
                        "{} {} {fam}: hp {}/{} dmg {dmg}/{} recoil {rs:?}/{} ticks {rd:?}/{} inv {}/{} override {}/{} bounds {bounds_ok}",
                        names[scene], p.source_id, hm["hp"], spec.health.health, spec.health.contact_damage, spec.recoil_speed as f64 / 65536.0,
                        spec.recoil_ticks, hm["invincible"], spec.health.invincible, hm["damageOverride"], spec.health.damage_override
                    ));
                }
            }
        }
    }
    println!("family         types   hp  dmg  rec-v rec-t flags bounds  (no source record)");
    for (f, r) in &rows {
        println!(
            "{f:<14} {:>4} {:>5} {:>4} {:>6} {:>5} {:>5} {:>6}  {}",
            r[0], r[1], r[2], r[3], r[4], r[5], r[6], r[7]
        );
    }
    for f in &fails {
        println!("FAIL {f}");
    }
}
