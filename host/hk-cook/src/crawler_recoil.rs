//! Crawler recoil displacement under the original 50 Hz fixed updates.
use crate::common::{component_records, f64_of, Result};
use hk_unity::{scene::Scene, Source};
use serde_json::Value;
use std::{collections::BTreeSet, path::Path};

pub fn fixed_steps(duration: f64) -> Result<u8> {
    if !duration.is_finite() || duration < 0.0 {
        return Err("invalid Crawler recoil duration".into());
    }
    let steps = (duration / 0.02 - 1e-5).ceil() as i64;
    u8::try_from(steps).map_err(|_| "Crawler recoil exceeds fixed-step budget".into())
}

/// Update only the existing Crawler types after the retained legacy writer.
/// The actual original Recoil component must still match the admitted contract.
pub fn bind_generated_specs(root: &Path) -> Result<()> {
    let report: Value = serde_json::from_slice(
        &std::fs::read(root.join("data/regions.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let source = Source::from_doctor(root).map_err(|e| e.to_string())?;
    let mut text =
        std::fs::read_to_string(root.join("data/regions.rs")).map_err(|e| e.to_string())?;
    let mut checked = BTreeSet::new();
    let mut scenes = BTreeSet::new();
    for region in report["regions"].as_array().ok_or("regions absent")? {
        for actor in region["actors"].as_array().ok_or("actors absent")? {
            if actor["movement_supported"] != true
                || actor["movement_control"]["kind"] != "WalkLeftRight"
            {
                continue;
            }
            let sid = actor["source"].as_str().ok_or("Crawler source absent")?;
            if !checked.insert(sid.to_string()) {
                continue;
            }
            let scene_id = region["scene_id"].as_u64().ok_or("Crawler scene absent")?;
            let meta = report["scenes"]
                .as_array()
                .ok_or("scenes absent")?
                .iter()
                .find(|s| s["scene_id"] == scene_id)
                .ok_or("Crawler scene not admitted")?;
            let scene = Scene::new(&source, meta["file"].as_str().ok_or("scene file absent")?)
                .map_err(|e| e.to_string())?;
            let gid = actor["game_object"]
                .as_i64()
                .ok_or("Crawler owner absent")?;
            let records = component_records(&scene, gid);
            let recoil: Vec<_> = records.iter().filter(|r| r.1 == "Recoil").collect();
            if recoil.len() != 1 {
                return Err(format!("{sid}: Crawler needs one original Recoil"));
            }
            let recoil = recoil[0].2;
            if recoil
                .get("m_GameObject")
                .and_then(|v| v.get("m_PathID"))
                .and_then(hk_unity::Value::int)
                != Some(gid)
            {
                return Err(format!("{sid}: Recoil owner differs"));
            }
            let speed = f64_of(recoil, "recoilSpeedBase")?;
            let duration = f64_of(recoil, "recoilDuration")?;
            // This increment changes the ten currently admitted Crawlers only.
            if speed != 15.0 || (duration - 0.15000000596046448).abs() > 1e-12 {
                return Err(format!("{sid}: Crawler recoil contract differs"));
            }
            scenes.insert(scene_id);
        }
    }
    if checked.len() != 10 {
        return Err("admitted Crawler placement set differs".into());
    }
    let steps = fixed_steps(0.15000000596046448)?;
    for scene in scenes {
        let marker = format!("static SCENE_ACTORS_{scene}: &[hk_sim::ActorSpec] = &[");
        let start = text.find(&marker).ok_or("Crawler spec table absent")? + marker.len();
        let end = start
            + text[start..]
                .find("];\n")
                .ok_or("Crawler spec table end absent")?;
        let mut section = text[start..end].to_string();
        let mut cursor = 0;
        let mut count = 0;
        while let Some(at) = section[cursor..].find("controller:hk_sim::ActorController::Crawler,")
        {
            let at = cursor + at;
            let old = "controller:hk_sim::ActorController::Crawler,";
            let new =
                format!("controller:hk_sim::ActorController::Crawler {{recoil_steps:{steps}}},");
            section.replace_range(at..at + old.len(), &new);
            cursor = at + new.len();
            count += 1;
        }
        if count == 0
            && !section.contains(&format!(
                "controller:hk_sim::ActorController::Crawler {{recoil_steps:{steps}}},"
            ))
        {
            return Err("admitted Crawler spec absent".into());
        }
        text.replace_range(start..end, &section);
    }
    std::fs::write(root.join("data/regions.rs"), text).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_float_duration_has_eight_fixed_updates() {
        assert_eq!(fixed_steps(0.15000000596046448).unwrap(), 8);
        assert_eq!(fixed_steps(0.2).unwrap(), 10);
        assert_eq!(fixed_steps(0.0).unwrap(), 0);
    }
    #[test]
    fn invalid_or_excessive_fixed_duration_is_rejected() {
        assert!(fixed_steps(f64::NAN).is_err());
        assert!(fixed_steps(-0.1).is_err());
        assert!(fixed_steps(10.0).is_err());
    }
}
