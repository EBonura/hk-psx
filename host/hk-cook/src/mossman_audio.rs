//! Measure complete Mossman samples in the existing Runner SPU window.
//! Uses a retained encoder, without building tools or touching the original install.
use crate::{
    music::{cook_clip, Resampler},
    pyjson::dumps,
    spu::{fnv, loop_payload, oneshot_payload, Tool},
};
use hk_unity::Source;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
fn final_paths(value: &mut serde_json::Value, from: &str, to: &str) {
    match value {
        serde_json::Value::String(s) => {
            if let Some(tail) = s.strip_prefix(from) {
                *s = format!("{to}{tail}")
            }
        }
        serde_json::Value::Array(a) => {
            for v in a {
                final_paths(v, from, to)
            }
        }
        serde_json::Value::Object(o) => {
            for v in o.values_mut() {
                final_paths(v, from, to)
            }
        }
        _ => {}
    }
}

pub fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.len() != 5 {
        return Err("usage: mossman-runner-audio ORIGINAL_DATA FMOD_ROOT RETAINED_ENCODER NEW_OUTPUT_DIRECTORY".into());
    }
    let out = Path::new(&args[4]);
    if out.exists() {
        return Err("output must not exist; preserve prior receipts".into());
    }
    let next = out.with_extension("next");
    if next.exists() {
        return Err("partial output already exists; review before rerun".into());
    }
    std::fs::create_dir_all(&next)?;
    let source = Source::new(Path::new(&args[1]))?;
    let mut original_instances = Vec::new();
    for (file, gid) in [
        ("level128", 736),
        ("level134", 110),
        ("level135", 808),
        ("level136", 510),
        ("level136", 1082),
    ] {
        let scene = hk_unity::scene::Scene::new(&source, file)?;
        let found =
            crate::runner::recognize(&scene, &source, gid, scene.point(gid, 0.0, 0.0, 0.0)?)
                .map_err(|e| format!("{file}:{gid}: {e}"))?;
        if found.audio.walk_loop != "sharedassets128.assets:17"
            || found.audio.chase
                != [
                    "sharedassets128.assets:18",
                    "sharedassets128.assets:19",
                    "sharedassets128.assets:16",
                ]
            || !found.audio.looped
            || found.audio.initial_pitch != 1.0
            || found.audio.volume != 1.0
            || found.audio.play_on_awake
        {
            return Err("Mossman exact original audio binding drift".into());
        }
        original_instances.push(json!({"source":format!("{file}:{gid}"),"control":serde_json::from_str::<serde_json::Value>(&dumps(&found.control))?}));
    }
    let tool = Tool {
        binary: PathBuf::from(&args[3]),
        scratch: next.join("encoder-scratch"),
    };
    std::fs::create_dir_all(&tool.scratch)?;
    let encoder_sha = format!("{:x}", Sha256::digest(std::fs::read(&tool.binary)?));
    let mut bank = Vec::new();
    let mut rows = Vec::new();
    for (id, rate, looped) in [
        (17, 8000, true),
        (18, 11025, false),
        (19, 11025, false),
        (16, 11025, false),
    ] {
        let file = source.file("sharedassets128.assets")?;
        let obj = source.object(&file, id)?;
        let profile = cook_clip(
            Path::new(&args[2]),
            &tool,
            &source,
            &obj,
            &next,
            rate,
            1,
            Resampler::Ffmpeg,
        )?;
        let raw = std::fs::read(&profile.planes[0].path)?;
        let payload = if looped {
            loop_payload(&raw)?
        } else {
            oneshot_payload(&raw)?
        };
        let offset = bank.len();
        rows.push(json!({"source":obj.sid(),"raw_object_sha256":format!("{:x}",Sha256::digest(obj.raw()?)),
            "source_identity":serde_json::from_str::<serde_json::Value>(&dumps(&profile.source_identity))?,
            "name":profile.name,"rate":profile.rate,"loop":looped,"source_pcm_frames":profile.source_pcm_frames,"source_pcm_rate":profile.source_pcm_rate,
            "valid_frames":profile.frames,"padding_samples":profile.padding_samples,"offset":offset,"bytes":payload.len(),
            "sha256":format!("{:x}",Sha256::digest(&payload)),"pitch":profile.spu_pitch,
            "pitch_multipliers":if looped{vec![1.0,1.0]}else{vec![1.100000023841858,1.2000000476837158]},
            "profile":serde_json::from_str::<serde_json::Value>(&dumps(&profile.json))?}));
        bank.extend(payload);
    }
    let mut report = json!({"source_contract":"Pinned five-instance recognizer and original Anticipate actions, equal chase weights1,1,1; first overwritten pitch draw retained by future guest integration.",
        "original_instances":original_instances,"encoder_sha256":encoder_sha,"spu_base":503808,"spu_end":524288,"reserved_bytes":20480,
        "bank_bytes":bank.len(),"fits_existing_window":bank.len()<=20480,"bank_sha256":format!("{:x}",Sha256::digest(&bank)),"bank_fnv":fnv(&bank),
        "clips":rows,"trimmed":false,"guest_dispatch_implemented":false,"mixed_family_residency":"not audited"});
    final_paths(&mut report, &next.to_string_lossy(), &out.to_string_lossy());
    report["payload_assembly"]=json!("Profile planes are flagless ADPCM. Bank loop gains START4 and last END|REPEAT3 flags; each one-shot gains one silent16-byte END block. Valid source/sample frames remain intact, padding reported explicitly. Profile and bank SHA therefore differ by design.");
    std::fs::write(next.join("mossman-runner.adpcm"), &bank)?;
    if bank.len() <= 20480 {
        let mut header=format!("// Generated from complete original Mossman Runner source clips.\npub const BANK_BYTES:usize={};\npub const BANK_CHECKSUM:u32={};\npub const SCENE_IDS:[u16;4]=[46,49,50,51];\npub const CLIPS:[(u32,usize,u16,u16,u16,bool);4]=[\n",bank.len(),fnv(&bank));
        for clip in report["clips"].as_array().ok_or("clips absent")? {
            let rate = clip["rate"].as_f64().ok_or("rate absent")?;
            let bounds: Vec<_> = clip["pitch_multipliers"]
                .as_array()
                .ok_or("bounds absent")?
                .iter()
                .map(|m| (rate * 4096.0 / 44100.0 * m.as_f64().unwrap()).round() as u16)
                .collect();
            header += &format!(
                "({},{},{},{},{},{}),\n",
                503808 + clip["offset"].as_u64().ok_or("offset")?,
                clip["bytes"],
                clip["pitch"],
                bounds[0],
                bounds[1],
                clip["loop"]
            );
        }
        header += "];\n";
        std::fs::write(next.join("mossman-runner-audio.rs"), header)?;
    }
    std::fs::write(
        next.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    drop(tool);
    std::fs::rename(next, out)?;
    Ok(())
}

/// Canonical build preparation uses the retained encoder, with all samples intact.
pub fn cook(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let source = Source::from_doctor(root)?;
    let encoder = std::env::var_os("HK_SPU_ENCODER")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("tools/psx-audio-cook/target/release/psx-audio-cook"));
    let out = root.join(format!(".hkpsx/mossman-audio/{}", std::process::id()));
    std::fs::create_dir_all(out.parent().ok_or("audio parent missing")?)?;
    run(&[
        "mossman-audio".into(),
        source.directory.to_string_lossy().into_owned(),
        root.to_string_lossy().into_owned(),
        encoder.to_string_lossy().into_owned(),
        out.to_string_lossy().into_owned(),
    ])?;
    std::fs::create_dir_all(root.join("data"))?;
    for (from, to) in [
        ("mossman-runner.adpcm", "mossman-runner-audio.adpcm"),
        ("mossman-runner-audio.rs", "mossman-runner-audio.rs"),
    ] {
        let next = root.join("data").join(format!("{to}.next"));
        std::fs::copy(out.join(from), &next)?;
        std::fs::rename(next, root.join("data").join(to))?;
    }
    Ok(())
}
