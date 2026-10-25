//! The sound of the opening cinematics, decoded for the intro cooker.
//!
//! The two video clips of `Opening_Sequence` carry digital silence. What the
//! original plays with them is separate AudioClips in the same bank: the piano
//! over the poem (`short_piano_for_opening_text`), then one clip per movie
//! (`Prologue_Cutscene_30fps AUDIO`, `Intro_cinematic_v01 AUDIO`), which the
//! scene's cinematic players start with their video. FMOD decodes the banks
//! (the same way every other cooker here reads a clip), and the WAVs land in
//! `.hkpsx/intro/audio/` for `hk-intro-cook --audio-dir`.
//!
//! Writes `<key>.wav` per clip and `audio.json` (clip name, source, sha256 of
//! the compressed bank, seconds of sound).

use crate::common::{err, Result};
use hk_unity::{Source, Value};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::Path;

/// One clip of the opening: the file it is written to and the AudioClip name.
pub struct Clip {
    pub key: &'static str,
    pub name: &'static str,
}

/// The file the clips sit in (a bank beside the scene's other assets).
const BANK: &str = "sharedassets5.assets";
/// `m_Name` of each clip, in play order.
pub const CLIPS: [Clip; 3] = [
    Clip { key: "excerpt", name: "short_piano_for_opening_text" },
    Clip { key: "prologue", name: "Prologue_Cutscene_30fps AUDIO" },
    Clip { key: "intro", name: "Intro_cinematic_v01 AUDIO" },
];

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The AudioClip called `name`: its tree and the compressed bank's sha256.
fn find_clip(source: &Source, name: &str) -> Result<(Value, String)> {
    const AUDIO_CLIP: i32 = 83;
    let file = source.file(BANK).map_err(|e| format!("{BANK}: {e}"))?;
    let mut found = None;
    for info in file.objects.iter().filter(|o| o.class_id == AUDIO_CLIP) {
        let obj = source
            .object(&file, info.path_id)
            .map_err(|e| e.to_string())?;
        let tree = source.read(&obj).map_err(|e| e.to_string())?;
        if tree.get("m_Name").and_then(Value::str).as_deref() == Some(name) {
            if found.is_some() {
                return err(format!("{BANK} has more than one AudioClip {name}"));
            }
            found = Some(tree);
        }
    }
    let tree = found.ok_or_else(|| format!("{BANK} has no AudioClip {name}"))?;
    let resource = tree
        .get("m_Resource")
        .ok_or("the opening's AudioClip has no m_Resource")?;
    let path = resource.get("m_Source").and_then(Value::str).unwrap_or_default();
    let base = path.rsplit(['/', '\\']).next().unwrap_or(&path).to_string();
    let offset = resource.get("m_Offset").and_then(Value::int).unwrap_or(0) as usize;
    let size = resource.get("m_Size").and_then(Value::int).unwrap_or(0) as usize;
    let bank = source
        .resource(&source.directory.join(&base))
        .map_err(|e| e.to_string())?;
    let bytes = bank
        .get(offset..offset + size)
        .ok_or("the opening's audio is past the end of its resource file")?;
    Ok((tree, hex(&Sha256::digest(bytes))))
}

/// Decodes the three clips into `out`.
pub fn cook(root: &Path, source: &Source, out: &Path) -> Result<String> {
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut rows = Vec::new();
    for clip in &CLIPS {
        let (tree, sha) = find_clip(source, clip.name)?;
        let wav = crate::fmod::clip_wav(root, source, &tree)?;
        let path = out.join(format!("{}.wav", clip.key));
        std::fs::write(&path, &wav).map_err(|e| format!("{}: {e}", path.display()))?;
        let seconds = tree.get("m_Length").and_then(Value::float).unwrap_or(0.0);
        rows.push(json!({
            "key": clip.key, "clip": clip.name, "source": BANK,
            "bank_sha256": sha, "seconds": seconds, "wav_bytes": wav.len(),
        }));
    }
    let report = serde_json::to_string_pretty(&json!({ "clips": rows }))
        .map_err(|e| e.to_string())?;
    std::fs::write(out.join("audio.json"), format!("{report}\n")).map_err(|e| e.to_string())?;
    Ok(format!("intro audio: {} clips written to {}", CLIPS.len(), out.display()))
}

pub fn main(root: &Path, source_dir: Option<&Path>) -> Result<()> {
    let source = match source_dir {
        Some(d) => Source::new(d).map_err(|e| e.to_string())?,
        None => Source::from_doctor(root).map_err(|e| e.to_string())?,
    };
    println!("{}", cook(root, &source, &root.join(".hkpsx/intro/audio"))?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clips_come_in_play_order_with_distinct_files() {
        let keys: Vec<_> = CLIPS.iter().map(|c| c.key).collect();
        assert_eq!(keys, ["excerpt", "prologue", "intro"]);
    }
}
