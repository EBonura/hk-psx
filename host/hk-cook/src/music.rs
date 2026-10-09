//! The clip conversion the music, ambience, Focus and Runner cooks share
//! (host/cook_music.py `cook_clip`, `decoded_source`, `sha`): a source
//! AudioClip decoded by FMOD, converted to mono or stereo PCM at a rate, encoded
//! by the SDK encoder, and checked by an independent ffmpeg decode.

use crate::common::{err, py_round, Result};
use crate::cook_audio::{div, js, sha, u};
use crate::fmod::clip_wav;
use crate::pyjson::{dumps, dumps_sorted, Json};
use crate::spu::{decode, read_wav, run, samples_of, Tool};
use hk_unity::{Obj, Source, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

/// cook_music.py `sha`: the sha256 of a file.
pub fn sha_file(path: &Path) -> Result<String> {
    Ok(sha(&std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?))
}

/// The JSON a type-tree value becomes through `json.dump(..., default=list)`.
pub fn value_json(v: &Value) -> Json {
    match v {
        Value::Bool(b) => Json::Bool(*b),
        Value::Int(i) => Json::Int(*i),
        Value::UInt(i) => Json::Int(*i as i64),
        Value::F32(f) => Json::Float(*f as f64),
        Value::F64(f) => Json::Float(*f),
        Value::Str(s) => Json::Str(String::from_utf8_lossy(s).into_owned()),
        Value::Bytes(b) => Json::List(b.iter().map(|&x| Json::Int(x as i64)).collect()),
        Value::List(l) => Json::List(l.iter().map(value_json).collect()),
        Value::Map(m) => Json::Obj(m.iter().map(|(k, x)| (k.to_string(), value_json(x))).collect()),
    }
}

/// host/source.py `dump`: `json.dumps(data, indent=2)`, no trailing newline.
pub fn dump(path: &Path, data: &Json) -> Result<()> {
    std::fs::write(path, dumps(data)).map_err(|e| format!("{}: {e}", path.display()))
}

/// host/source.py `rel`: a path as reports record it, relative to the checkout.
pub fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().into_owned()
}

/// Find a key of an ordered JSON object.
pub fn jget<'a>(j: &'a Json, key: &str) -> Option<&'a Json> {
    match j {
        Json::Obj(f) => f.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}
pub fn jint(j: &Json, key: &str) -> Option<i64> {
    match jget(j, key)? {
        Json::Int(i) => Some(*i),
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Resampler {
    Ffmpeg,
    Sdk,
}

/// One encoded plane of a profile.
pub struct Plane {
    pub path: PathBuf,
    pub bytes: i64,
    pub sha256: String,
}

/// cook_music.py `cook_clip`'s result: the fields later steps read, and the
/// whole report entry.
pub struct Profile {
    pub source: String,
    pub name: String,
    pub rate: i64,
    pub channels: i64,
    pub frames: i64,
    pub spu_pitch: i64,
    pub padding_samples: i64,
    pub source_pcm_frames: i64,
    pub source_pcm_rate: i64,
    pub bytes: i64,
    pub source_identity: Json,
    pub planes: Vec<Plane>,
    pub json: Json,
}

/// `decoded_source`: the clip's WAV under `folder` and its identity (metadata
/// and encoded-resource hashes).
fn decoded_source(root: &Path, source: &Source, tree: &Value, folder: &Path) -> Result<(PathBuf, Vec<u8>, Json)> {
    let r = tree.get("m_Resource").ok_or("AudioClip without m_Resource")?;
    let name = r.get("m_Source").and_then(Value::str).ok_or("m_Resource without m_Source")?;
    let path = source.directory.join(&name);
    let resolved = path.canonicalize().map_err(|e| format!("{}: {e}", path.display()))?;
    if !resolved.starts_with(source.directory.canonicalize().map_err(|e| e.to_string())?) {
        return err("audio resource escapes Windows source");
    }
    let (offset, size) = (r.get("m_Offset").and_then(Value::int).unwrap_or(0) as usize, r.get("m_Size").and_then(Value::int).unwrap_or(0) as usize);
    let all = std::fs::read(&resolved).map_err(|e| e.to_string())?;
    let encoded = all.get(offset..offset + size).ok_or("truncated source audio resource")?;
    let mut text = String::new();
    dumps_sorted(tree, &mut text);
    let identity = Json::Obj(vec![("clip_metadata_sha256".into(), Json::Str(sha(text.as_bytes()))), ("encoded_resource_sha256".into(), Json::Str(sha(encoded)))]);
    let wav = clip_wav(root, source, tree)?;
    let wav_path = folder.join("source.wav");
    std::fs::write(&wav_path, &wav).map_err(|e| e.to_string())?;
    let mut stamp = match &identity {
        Json::Obj(f) => f.clone(),
        _ => unreachable!(),
    };
    stamp.push(("wav_sha256".into(), Json::Str(sha(&wav))));
    dump(&folder.join("source-wav.json"), &Json::Obj(stamp))?;
    Ok((wav_path, wav, identity))
}

pub fn ffmpeg_version() -> Result<String> {
    let out = run(Command::new("ffmpeg").arg("-version"), None)?;
    Ok(String::from_utf8_lossy(&out).lines().next().unwrap_or("").to_string())
}

/// `cook_clip`: convert and encode a clip at `rate`, one plane per channel.
/// `restart` is the encoder's loop mode (spu_encode.py without `--ring`).
#[allow(clippy::too_many_arguments)]
pub fn cook_clip(root: &Path, tool: &Tool, source: &Source, obj: &Obj, out: &Path, rate: i64, channels: i64, resampler: Resampler) -> Result<Profile> {
    let sid = obj.sid();
    let tree = u(source.read(obj))?;
    let folder = out.join(sid.replace(':', "-"));
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let (wav_path, wav_bytes, source_identity) = decoded_source(root, source, &tree, &folder)?;
    let wav = read_wav(&wav_bytes)?;
    if wav.width != 2 {
        return err("expected source PCM16");
    }
    let source_frames = (wav.data.len() / (2 * wav.channels as usize)) as i64;
    let mut version = ffmpeg_version()?;
    let raw_path = folder.join(format!("{rate}-{channels}.s16le"));
    let pcm: Vec<i16> = match resampler {
        Resampler::Sdk => {
            if channels != 1 {
                return err("the SDK resampler path is mono");
            }
            version = "psx_audio_cook::resample::Sinc (SDK shared resampler)".into();
            let pcm = tool.resample(&wav_bytes, rate)?;
            std::fs::write(&raw_path, pcm.iter().flat_map(|s| s.to_le_bytes()).collect::<Vec<u8>>()).map_err(|e| e.to_string())?;
            pcm
        }
        Resampler::Ffmpeg => {
            run(Command::new("ffmpeg").args(["-v", "error", "-y", "-i"]).arg(&wav_path).args(["-ar", &rate.to_string(), "-ac", &channels.to_string(), "-f", "s16le"]).arg(&raw_path), None)?;
            samples_of(&std::fs::read(&raw_path).map_err(|e| e.to_string())?)
        }
    };
    if pcm.len() % channels as usize != 0 {
        return err("partial PCM frame");
    }
    let frames = (pcm.len() / channels as usize) as i64;
    let mut planes = Vec::new();
    let mut plane_json = Vec::new();
    for channel in 0..channels as usize {
        let mono: Vec<i16> = pcm.iter().skip(channel).step_by(channels as usize).copied().collect();
        let mono_path = folder.join(format!("{rate}-{channels}-ch{channel}.s16le"));
        std::fs::write(&mono_path, mono.iter().flat_map(|s| s.to_le_bytes()).collect::<Vec<u8>>()).map_err(|e| e.to_string())?;
        let encoded = mono_path.with_extension("adpcm");
        // spu_encode.py: the SDK encoder, a restartable stream, its own metric.
        let data = tool.encode(&mono, "restart")?;
        std::fs::write(&encoded, &data).map_err(|e| e.to_string())?;
        let decoded = decode(&data)?;
        let signal: i64 = mono.iter().map(|&s| s as i64 * s as i64).sum();
        let error: i64 = mono.iter().zip(&decoded).map(|(&s, &d)| (s as i64 - d as i64).pow(2)).sum();
        let metric = Json::Obj(vec![
            ("samples".into(), Json::Int(mono.len() as i64)),
            ("signal_energy".into(), Json::Float(signal as f64)),
            ("error_energy".into(), Json::Float(error as f64)),
            ("snr_db".into(), Json::Float(if error > 0 && signal > 0 { 10.0 * (signal as f64 / error as f64).log10() } else { 999.0 })),
        ]);
        if data.len() as i64 != (frames + 27) / 28 * 16 || data.chunks_exact(16).any(|b| b[1] != 0) {
            return err("invalid encoded payload");
        }
        // Independent FFmpeg decode confirms valid framing and records an
        // external quality metric.
        let vag = encoded.with_extension("vag");
        let mut header = b"VAGp".to_vec();
        for word in [0x20u32, 0, data.len() as u32, rate as u32] {
            header.extend_from_slice(&word.to_be_bytes());
        }
        header.extend_from_slice(&[0; 28]);
        header.extend_from_slice(&data);
        std::fs::write(&vag, header).map_err(|e| e.to_string())?;
        let decoded_path = encoded.with_extension("decoded.s16le");
        run(Command::new("ffmpeg").args(["-v", "error", "-y", "-i"]).arg(&vag).args(["-f", "s16le"]).arg(&decoded_path), None)?;
        let recon = samples_of(&std::fs::read(&decoded_path).map_err(|e| e.to_string())?);
        if recon.len() as i64 != (frames + 27) / 28 * 28 {
            return err("external decode frame mismatch");
        }
        let err2: i64 = mono.iter().zip(&recon).map(|(&a, &b)| (a as i64 - b as i64).pow(2)).sum();
        let sig2: i64 = mono.iter().map(|&a| (a as i64).pow(2)).sum();
        let sha256 = sha(&data);
        let shown = rel(root, &encoded);
        plane_json.push(Json::Obj(vec![
            ("path".into(), Json::Str(shown)),
            ("bytes".into(), Json::Int(data.len() as i64)),
            ("sha256".into(), Json::Str(sha256.clone())),
            ("encoder_metric".into(), metric),
            ("ffmpeg_snr_db".into(), if sig2 != 0 && err2 != 0 { Json::Float(10.0 * (sig2 as f64 / err2 as f64).log10()) } else { Json::Null }),
        ]));
        planes.push(Plane { path: encoded, bytes: data.len() as i64, sha256 });
    }
    let name = tree.get("m_Name").and_then(Value::str).unwrap_or_default();
    let pitch = py_round(div(rate * 4096, 44100));
    let bytes: i64 = planes.iter().map(|p| p.bytes).sum();
    let padding = (-frames).rem_euclid(28);
    let json = Json::Obj(vec![
        ("source".into(), Json::Str(sid.clone())),
        ("name".into(), js(&name)),
        ("source_metadata".into(), value_json(&tree)),
        ("source_identity".into(), source_identity.clone()),
        ("source_wav_sha256".into(), Json::Str(sha(&wav_bytes))),
        ("source_pcm_frames".into(), Json::Int(source_frames)),
        ("source_pcm_rate".into(), Json::Int(wav.rate as i64)),
        ("source_pcm_channels".into(), Json::Int(wav.channels as i64)),
        ("rate".into(), Json::Int(rate)),
        ("spu_pitch".into(), Json::Int(pitch)),
        ("spu_actual_rate".into(), Json::Float((pitch * 44100) as f64 / 4096.0)),
        ("channels".into(), Json::Int(channels)),
        ("frames".into(), Json::Int(frames)),
        ("seconds".into(), Json::Float(div(frames, rate))),
        ("padding_samples".into(), Json::Int(padding)),
        ("bytes".into(), Json::Int(bytes)),
        ("bytes_per_second".into(), Json::Float((rate * channels * 16) as f64 / 28.0)),
        ("planes".into(), Json::List(plane_json)),
        ("conversion".into(), Json::Str(version)),
        ("flags".into(), js("All payload flags zero; runtime transport must install loop/chunk boundaries")),
        ("loop_boundary".into(), js("Full-clip sample count preserved in metadata; final ADPCM block has at most27 zero samples")),
    ]);
    Ok(Profile {
        source: sid,
        name,
        rate,
        channels,
        frames,
        spu_pitch: pitch,
        padding_samples: padding,
        source_pcm_frames: source_frames,
        source_pcm_rate: wav.rate as i64,
        bytes,
        source_identity,
        planes,
        json,
    })
}
