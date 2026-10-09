//! Package the resident source ambience as raw disc chunks, never EXE samples.
//! Ported from host/ambience.py, whose `.hkpsx/ambience.json`,
//! `data/ambience.rs` and `data/ambience/clip_N.adpcm` it reproduces byte for
//! byte (except `tool_sha256`, which names this module).
//!
//! How many loops there are, and which source atmos channels they are, is
//! `RESIDENT_ATMOS_CHANNELS` and nothing else: the clip table, the cue gain
//! arrays, the SPU voices and the streamed clip's index all follow from it.
//! Which clip each channel plays is read off the persistent AudioManager, so a
//! channel can be resident ahead of any scene that enables it.
//!
//! SPU residency follows the area rather than the game. The guest loads a clip
//! at the scene gate that first needs it, into an address `allocate` gives it
//! here, and reuses the bytes of clips the new area does not play. Two clips
//! share SPU only when no scene cue plays both and no scene gate joins a scene
//! that plays one to a scene that plays the other, so a gate never has to cut a
//! stem that is still fading out. The same holds for the clips of every scene
//! one gate away, so the guest can load the next area's clips in the
//! background while the drive is idle. Transitions no gate describes (a
//! respawn at a distant bench, the debug reset) are the guest's to arbitrate.

use crate::common::{err, py_round, Result};
use crate::cook_audio::{jobj, js, sha, u};
use crate::music::{dump, jget, jint, rel, sha_file};
use crate::music_report::{atmos_rate, source_snapshot, RESIDENT_ATMOS_CHANNELS as CHANNELS};
use crate::pyjson::{parse, Json};
use crate::spu::{fnv, loop_payload, run};
use hk_unity::{Obj, Source};
use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::process::Command;

const SPU_START: i64 = 0x18000;
/// The area-music ring sits directly below the world one-shot bank, which
/// sits directly below Focus; ambience must end at or below the ring.
const MUSIC_RING_BYTES: i64 = 16384;
const SFX_END: i64 = 0x14000; // Geo occupies 0x14000..0x18000, below ambience.
/// The SPU voices ambience owns. 0..5 and 16..17 player SFX, 11 the per-scene
/// one-shots, 12..14 Geo, 15 shared by the Great Door, the False Knight, the
/// menu and the world bank, 18..20 Focus, 21..23 Runner.
const VOICES: [i64; 5] = [6, 7, 8, 9, 10];
const SCENE_SFX_VOICE: i64 = 11;
/// The first of them is area music's: it keeps one voice for the life of the disc.
const MUSIC_VOICE: i64 = VOICES[0];
/// The rest are a pool: a stem takes one when it keys on and returns it once it
/// has finished fading out.
const POOL_VOICES: [i64; 4] = [7, 8, 9, 10];
/// The banks stacked above ambience in SPU RAM, lowest first.
const TAIL_BANKS: [&str; 3] = [
    "data/world-sfx.rs",
    "data/focus-audio.rs",
    "data/runner-audio.rs",
];
/// A source Atmos gain above unity clamps to full scale rather than lifting one
/// stem's ceiling above the 16,383 every other bank in this port mixes against.
const BOOST_CEILING_DB: f64 = 6.0;
/// Stems a scene's cue enables but which the scene does not keep resident.
/// Crossroads_04 (Gruz Mother's arena): its cue plays `Rain Indoor` through the
/// `at Cave` snapshot at -60.4 dB, voice gain 16 of 16383. Only a stem at -60 dB
/// or quieter (gain 16 or less) may be listed; the cook refuses anything louder.
const UNLOADED_STEMS: [(&str, &str); 1] = [("level40", "ruins_rain_indoor_loop")];
const INAUDIBLE_GAIN: i64 = 16;

// ---------------------------------------------------------------- Json access

fn jv<'a>(j: &'a Json, key: &str) -> Result<&'a Json> {
    jget(j, key).ok_or_else(|| format!("report lacks {key}"))
}
fn jl<'a>(j: &'a Json, key: &str) -> Result<&'a [Json]> {
    match jv(j, key)? {
        Json::List(l) => Ok(l),
        _ => err(format!("{key} is not a list")),
    }
}
fn ji(j: &Json, key: &str) -> Result<i64> {
    jint(j, key).ok_or_else(|| format!("{key} is not an integer"))
}
fn js_of(j: &Json, key: &str) -> Result<String> {
    match jv(j, key)? {
        Json::Str(s) => Ok(s.clone()),
        _ => err(format!("{key} is not a string")),
    }
}
fn num(j: &Json) -> Option<f64> {
    match j {
        Json::Int(i) => Some(*i as f64),
        Json::Float(f) => Some(*f),
        Json::Bool(b) => Some(*b as i64 as f64),
        _ => None,
    }
}
fn jtruthy(j: &Json) -> bool {
    match j {
        Json::Null => false,
        Json::Bool(b) => *b,
        Json::Int(i) => *i != 0,
        Json::Float(f) => *f != 0.0,
        Json::Str(s) => !s.is_empty(),
        Json::List(l) => !l.is_empty(),
        Json::Obj(o) => !o.is_empty(),
    }
}
fn is_one(j: &Json) -> bool {
    num(j) == Some(1.0)
}
fn ilist(v: &[i64]) -> Json {
    Json::List(v.iter().map(|&i| Json::Int(i)).collect())
}
/// Python's `str(list_of_ints)`.
fn py_list(v: &[i64]) -> String {
    format!(
        "[{}]",
        v.iter().map(i64::to_string).collect::<Vec<_>>().join(", ")
    )
}

// ---------------------------------------------------------------- helpers

/// `volume`: one source mixer gain as an SPU voice volume; 16,383 is full scale.
fn volume(db: f64) -> Result<i64> {
    if !db.is_finite() || db > BOOST_CEILING_DB {
        return err("unsupported mixer gain");
    }
    Ok(py_round(16383.0 * 10f64.powf(db.min(0.0) / 20.0)))
}

/// `boosts`: which channels sit above unity, and by how much, for the report.
fn boosts(gains_db: &[f64]) -> Json {
    Json::Obj(
        CHANNELS
            .iter()
            .zip(gains_db)
            .filter(|(_, &db)| db > 0.0)
            .map(|(ch, &db)| {
                (
                    ch.to_string(),
                    Json::Float(format!("{db:.5}").parse::<f64>().unwrap()),
                )
            })
            .collect(),
    )
}

/// `bank_constant`: one `pub const <name>:<kind>=<integer>;` out of a generated manifest.
fn bank_constant(path: &Path, name: &str, kind: &str) -> Result<i64> {
    let text: String = std::fs::read_to_string(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .split_whitespace()
        .collect();
    let needle = format!("pubconst{name}:{kind}=");
    let mut from = 0;
    while let Some(at) = text[from..].find(&needle) {
        let rest = &text[from + at + needle.len()..];
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if !digits.is_empty() && rest[digits.len()..].starts_with(';') {
            return digits.parse::<i64>().map_err(|e| e.to_string());
        }
        from += at + needle.len();
    }
    err(format!("no {name} in {}", path.display()))
}

fn bank_bytes(path: &Path) -> Result<i64> {
    bank_constant(path, "BANK_BYTES", "usize")
}

/// `tail_base`: where one bank stacked above ambience currently says it starts.
fn tail_base(root: &Path, name: &str) -> Result<i64> {
    for constant in ["SPU_BASE", "BANK_BASE"] {
        if let Ok(v) = bank_constant(&root.join(name), constant, "u32") {
            return Ok(v);
        }
    }
    err(format!("no SPU_BASE or BANK_BASE in {name}"))
}

/// `tail_drift`: tail banks whose declared base now sits inside the bank below it.
fn tail_drift(root: &Path, end: i64) -> Result<Vec<(String, i64, i64)>> {
    let mut drift = Vec::new();
    let mut expected = end;
    for name in TAIL_BANKS {
        let declared = tail_base(root, name)?;
        if declared < expected {
            drift.push((name.to_string(), declared, expected));
        }
        expected = expected.max(declared) + bank_bytes(&root.join(name))?;
    }
    Ok(drift)
}

/// `spu_ceiling`: bytes the tail above ambience costs.
fn spu_ceiling(root: &Path) -> Result<(i64, Vec<(String, i64)>)> {
    let mut banks = Vec::new();
    for name in TAIL_BANKS {
        banks.push((name.to_string(), bank_bytes(&root.join(name))?));
    }
    Ok((banks.iter().map(|b| b.1).sum(), banks))
}

/// `pool_pressure`: most pooled stems that can be keyed on at once, over the cooked cues.
fn pool_pressure(cues: &[Cue]) -> i64 {
    let masks: BTreeSet<i64> = cues.iter().map(|c| c.mask).collect();
    masks
        .iter()
        .flat_map(|a| masks.iter().map(move |b| (a | b).count_ones() as i64))
        .max()
        .unwrap_or(0)
}

/// `neighbour_masks`: per scene, the stems of every scene one resolved gate away.
fn neighbour_masks(cues: &[Cue], edges: &BTreeSet<(i64, i64)>) -> HashMap<i64, i64> {
    let mask: HashMap<i64, i64> = cues.iter().map(|c| (c.scene, c.mask)).collect();
    let mut out: HashMap<i64, i64> = mask.keys().map(|&s| (s, 0)).collect();
    for &(a, b) in edges {
        if mask.contains_key(&a) && mask.contains_key(&b) {
            *out.get_mut(&a).unwrap() |= mask[&b];
            *out.get_mut(&b).unwrap() |= mask[&a];
        }
    }
    out
}

/// `conflicts`: stem pairs that may be in SPU at the same time.
fn conflicts(cues: &[Cue], edges: &BTreeSet<(i64, i64)>) -> BTreeSet<(usize, usize)> {
    let mask: HashMap<i64, i64> = cues.iter().map(|c| (c.scene, c.mask)).collect();
    let mut together: BTreeSet<i64> = cues.iter().map(|c| c.mask).collect();
    together.extend(
        edges
            .iter()
            .filter(|(a, b)| mask.contains_key(a) && mask.contains_key(b))
            .map(|(a, b)| mask[a] | mask[b]),
    );
    together.extend(
        neighbour_masks(cues, edges)
            .iter()
            .map(|(s, n)| mask[s] | n),
    );
    let mut pairs = BTreeSet::new();
    for m in together {
        let stems: Vec<usize> = (0..CHANNELS.len()).filter(|s| m >> s & 1 == 1).collect();
        for &a in &stems {
            for &b in &stems {
                if a != b {
                    pairs.insert((a, b));
                }
            }
        }
    }
    pairs
}

/// `allocate`: first-fit SPU addresses, largest clip first, sharing bytes
/// between clips that can never be resident together.
fn allocate(sizes: &[i64], pairs: &BTreeSet<(usize, usize)>, start: i64) -> Vec<i64> {
    let mut address: HashMap<usize, i64> = HashMap::new();
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by_key(|&s| (-sizes[s], s));
    for stem in order {
        let mut taken: Vec<(i64, i64)> = address
            .iter()
            .filter(|(o, _)| pairs.contains(&(stem, **o)))
            .map(|(&o, &a)| (a, a + sizes[o]))
            .collect();
        taken.sort();
        let mut at = start;
        for (lo, hi) in taken {
            if at + sizes[stem] <= lo {
                break;
            }
            at = at.max((hi + 15) & !15);
        }
        address.insert(stem, at);
    }
    (0..sizes.len()).map(|s| address[&s]).collect()
}

// ---------------------------------------------------------------- assemble

#[derive(Debug)]
struct Clip {
    source: String,
    name: String,
    source_channel: i64,
    rate: i64,
    pitch: i64,
    byte_len: i64,
    checksum: u32,
    sha256: String,
    valid_frames: i64,
    padding_samples: i64,
    converted_payload_sha256: String,
    profile: Json,
    payload: Vec<u8>,
    spu_address: i64,
    shares_spu_with: Vec<i64>,
}

#[derive(Debug)]
struct Cue {
    scene: i64,
    source_scene: String,
    name: String,
    source: String,
    mask: i64,
    gains: Vec<i64>,
    fade_ticks: i64,
    source_fade_seconds: f64,
    source_ambience: Json,
    clamped_boost_db: Json,
    all_channel_snapshot_gains: Json,
    prefetch: i64,
    unloaded: Option<i64>,
}

#[allow(clippy::too_many_arguments)]
fn assemble(
    report: &Json,
    root: &Path,
    scene_files: &[String],
    transitions: &HashMap<String, f64>,
    snapshots: &HashMap<String, Vec<Json>>,
    sfx_end: i64,
    ceiling: i64,
    edges: &BTreeSet<(i64, i64)>,
    pool_voices: usize,
) -> Result<(Vec<Clip>, Vec<Cue>)> {
    if sfx_end > SFX_END {
        return err("ambience overlaps resident SFX bank");
    }
    // Which clip each resident channel plays comes from the persistent
    // AudioManager, not from the admitted scenes, because a resident channel no
    // admitted scene enables still has a loop to load.
    let mut residents: Vec<(i64, &Json)> = Vec::new();
    for r in jl(report, "resident_atmos")? {
        residents.push((ji(r, "channel")?, r));
    }
    let mut got: Vec<i64> = residents.iter().map(|r| r.0).collect();
    got.sort();
    let mut want = CHANNELS.to_vec();
    want.sort();
    if got != want {
        return err("source report resolves a different resident atmos set");
    }
    let resident = |ch: i64| residents.iter().find(|r| r.0 == ch).unwrap().1;
    let channels: HashMap<i64, String> = CHANNELS
        .iter()
        .map(|&ch| Ok((ch, js_of(resident(ch), "clip")?)))
        .collect::<Result<_>>()?;
    for (_, r) in &residents {
        if !jtruthy(jv(r, "loop")?) || !is_one(jv(r, "pitch")?) || !is_one(jv(r, "volume")?) {
            return err("unsupported AudioSource settings");
        }
    }
    let mut cues = Vec::new();
    for scene in jl(report, "scenes")? {
        let file = js_of(scene, "scene_file")?;
        let Some(scene_id) = scene_files.iter().position(|f| *f == file) else {
            continue;
        };
        let managers = jl(scene, "managers")?;
        if managers.len() != 1 {
            return err("ambiguous scene audio manager");
        }
        let manager = &managers[0];
        let all_snapshot = snapshots.get(&file).ok_or("scene without snapshots")?;
        // The scene's own atmosSnapshot sets every resident channel; an enabled
        // channel is then overridden by the atmos cue's snapshot, which is a
        // different object and can disagree. Keep the dB that wins so the
        // clamp record cannot name a level the bank does not play.
        let mut used: Vec<f64> = all_snapshot
            .iter()
            .map(|x| {
                jv(x, "internal_volume_db")
                    .ok()
                    .and_then(num)
                    .ok_or("snapshot without a gain")
            })
            .collect::<std::result::Result<_, _>>()?;
        let mut gains: Vec<i64> = used.iter().map(|&db| volume(db)).collect::<Result<_>>()?;
        let mut mask = 0i64;
        for entry in jl(manager, "ambience")? {
            let ch = ji(entry, "channel")?;
            let idx = CHANNELS
                .iter()
                .position(|&c| c == ch)
                .ok_or("ambience entry outside the resident set")?;
            if channels[&ch] != js_of(entry, "clip")? {
                return err("shared channel changes clip");
            }
            if !jtruthy(jv(entry, "loop")?)
                || !is_one(jv(entry, "pitch")?)
                || !is_one(jv(entry, "volume")?)
            {
                return err("unsupported AudioSource settings");
            }
            let snapshot = jv(entry, "snapshot")?;
            if jtruthy(jv(snapshot, "effects")?)
                || jl(snapshot, "chain")?.iter().any(|x| {
                    jtruthy(jv(x, "mute").unwrap_or(&Json::Null))
                        || jtruthy(jv(x, "solo").unwrap_or(&Json::Null))
                        || !is_one(jv(x, "pitch").unwrap_or(&Json::Null))
                })
            {
                return err("unsupported mixer processing");
            }
            used[idx] =
                num(jv(snapshot, "internal_volume_db")?).ok_or("snapshot without a gain")?;
            gains[idx] = volume(used[idx])?;
            mask |= 1 << idx;
        }
        let seconds = transitions[&file];
        if !seconds.is_finite() || !(0.0..=60.0).contains(&seconds) {
            return err("invalid atmosphere transition");
        }
        cues.push(Cue {
            scene: scene_id as i64,
            source_scene: file,
            name: js_of(jv(manager, "atmos_cue")?, "name")?,
            source: js_of(manager, "source")?,
            mask,
            gains,
            fade_ticks: py_round(seconds * 60.0),
            source_fade_seconds: seconds,
            source_ambience: jv(manager, "ambience")?.clone(),
            clamped_boost_db: boosts(&used),
            all_channel_snapshot_gains: Json::List(all_snapshot.clone()),
            prefetch: 0,
            unloaded: None,
        });
    }
    if cues.iter().map(|c| c.scene).collect::<BTreeSet<_>>()
        != (0..scene_files.len() as i64).collect()
    {
        return err("missing source ambience scenes");
    }
    let live = pool_pressure(&cues);
    if live > pool_voices as i64 {
        return err(format!("{live} stems can be audible at once across a transition and ambience pools {pool_voices} SPU voices ({}); every other voice is allocated", py_list(&POOL_VOICES).replacen('[', "(", 1).replacen(']', ")", 1)));
    }
    let mut clips = Vec::new();
    for &ch in &CHANNELS {
        let rate = atmos_rate(ch);
        let matches: Vec<&Json> = jl(report, "clips")?
            .iter()
            .filter(|c| {
                js_of(c, "source").ok().as_deref() == Some(channels[&ch].as_str())
                    && jint(c, "rate") == Some(rate)
                    && jint(c, "channels") == Some(1)
            })
            .collect();
        if matches.len() != 1 {
            return err(format!("missing or duplicate {rate}Hz mono profile"));
        }
        let c = matches[0];
        let planes = jl(c, "planes")?;
        let frames = ji(c, "frames")?;
        if planes.len() != 1 || frames <= 0 {
            return err("invalid mono source profile");
        }
        let plane = &planes[0];
        let path = root.join(js_of(plane, "path")?);
        let hk = root
            .join(".hkpsx")
            .canonicalize()
            .map_err(|e| e.to_string())?;
        if !path
            .canonicalize()
            .map_err(|e| format!("{}: {e}", path.display()))?
            .starts_with(hk)
        {
            return err("converted payload outside ignored source cache");
        }
        let data = std::fs::read(&path).map_err(|e| e.to_string())?;
        if data.len() as i64 != ji(plane, "bytes")? || sha(&data) != js_of(plane, "sha256")? {
            return err("converted payload hash mismatch");
        }
        if data.len() as i64 != (frames + 27) / 28 * 16
            || ji(c, "padding_samples")? != (-frames).rem_euclid(28)
        {
            return err("encoded length does not preserve valid sample count");
        }
        let payload = loop_payload(&data)?;
        let pitch = py_round((rate * 4096) as f64 / 44100.0);
        if pitch != ji(c, "spu_pitch")? {
            return err("pitch/profile mismatch");
        }
        clips.push(Clip {
            source: js_of(c, "source")?,
            name: js_of(c, "name")?,
            source_channel: ch,
            rate,
            pitch,
            byte_len: payload.len() as i64,
            checksum: fnv(&payload),
            sha256: sha(&payload),
            valid_frames: frames,
            padding_samples: ji(c, "padding_samples")?,
            converted_payload_sha256: js_of(plane, "sha256")?,
            profile: c.clone(),
            payload,
            spu_address: 0,
            shares_spu_with: Vec::new(),
        });
    }
    let pairs = conflicts(&cues, edges);
    let near = neighbour_masks(&cues, edges);
    for c in &mut cues {
        c.prefetch = near[&c.scene] & !c.mask;
    }
    // Scoped residency drops, applied after the conflict pairs above so every
    // stem keeps the address it had. The scene's one-shot bank then sees the
    // stem's bytes as a gap, and ambience::forget cuts the stem if it is still
    // fading in from the previous scene when the bank lands.
    for c in &mut cues {
        for (scene, name) in UNLOADED_STEMS {
            if c.source_scene != scene {
                continue;
            }
            let stem = clips
                .iter()
                .position(|k| k.name == name)
                .ok_or("unloaded stem is not resident")?;
            if c.gains[stem] > INAUDIBLE_GAIN {
                return err(format!(
                    "{name} is audible in {}; it may not be dropped from residency",
                    c.source_scene
                ));
            }
            c.mask &= !(1 << stem);
            c.prefetch &= !(1 << stem);
            c.unloaded = Some(c.unloaded.unwrap_or(0) | (1 << stem));
        }
    }
    let sizes: Vec<i64> = clips.iter().map(|c| c.byte_len).collect();
    for (clip, address) in clips.iter_mut().zip(allocate(&sizes, &pairs, SPU_START)) {
        if address % 16 != 0 || address < SPU_START {
            return err("ambience SPU capacity overflow");
        }
        if address + clip.byte_len > ceiling {
            return err(format!(
                "ambience SPU capacity overflow: {} ends {} bytes past the {ceiling:#x} ceiling",
                clip.name,
                address + clip.byte_len - ceiling
            ));
        }
        clip.spu_address = address;
    }
    let spans: Vec<(i64, i64, i64)> = clips
        .iter()
        .map(|c| (c.source_channel, c.spu_address, c.spu_address + c.byte_len))
        .collect();
    for (i, clip) in clips.iter_mut().enumerate() {
        clip.shares_spu_with = spans
            .iter()
            .enumerate()
            .filter(|(j, o)| *j != i && o.1 < spans[i].2 && spans[i].1 < o.2)
            .map(|(_, o)| o.0)
            .collect();
    }
    cues.sort_by_key(|c| c.scene);
    Ok((clips, cues))
}

/// `decoder_quality`: an independent ffmpeg decode of the looped payload.
fn decoder_quality(root: &Path, clip: &Clip, path: &Path) -> Result<Json> {
    let payload = std::fs::read(path).map_err(|e| e.to_string())?;
    let vag = path.with_extension("vag");
    let decoded_path = path.with_extension("decoded.s16le");
    // The clip's own rate, not the default: VAG playback rate does not change
    // the decoded samples this compares.
    let mut header = b"VAGp".to_vec();
    for word in [0x20u32, 0, payload.len() as u32, clip.rate as u32] {
        header.extend_from_slice(&word.to_be_bytes());
    }
    header.extend_from_slice(&[0; 28]);
    header.extend_from_slice(&payload);
    std::fs::write(&vag, header).map_err(|e| e.to_string())?;
    run(
        Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-i"])
            .arg(&vag)
            .args(["-f", "s16le"])
            .arg(&decoded_path),
        None,
    )?;
    let pcm = crate::spu::samples_of(&std::fs::read(&decoded_path).map_err(|e| e.to_string())?);
    let plane_path = root.join(js_of(&jl(&clip.profile, "planes")?[0], "path")?);
    let source = crate::spu::samples_of(
        &std::fs::read(plane_path.with_extension("s16le")).map_err(|e| e.to_string())?,
    );
    if pcm.len() != payload.len() / 16 * 28 || source.len() as i64 != clip.valid_frames {
        return err("decoder length mismatch");
    }
    let error: i64 = source
        .iter()
        .zip(&pcm)
        .map(|(&a, &b)| (a as i64 - b as i64).pow(2))
        .sum();
    let signal: i64 = source.iter().map(|&x| (x as i64).pow(2)).sum();
    let snr = (signal != 0 && error != 0).then(|| 10.0 * (signal as f64 / error as f64).log10());
    let expected = jv(&jl(&clip.profile, "planes")?[0], "ffmpeg_snr_db")?;
    let expected = if matches!(expected, Json::Null) {
        None
    } else {
        num(expected)
    };
    if snr.is_some() != expected.is_some()
        || snr.is_some_and(|s| (s - expected.unwrap()).abs() > 0.000001)
    {
        return err("loop flags changed decoded sample quality");
    }
    Ok(jobj(vec![
        ("ffmpeg_snr_db", snr.map_or(Json::Null, Json::Float)),
        ("decoded_sha256", Json::Str(sha_file(&decoded_path)?)),
        ("decoded_boundary_delta", Json::Int(pcm[0] as i64 - *pcm.last().unwrap() as i64)),
        ("source_boundary_delta", Json::Int(source[0] as i64 - *source.last().unwrap() as i64)),
        ("zero_tail_samples", Json::Int(clip.padding_samples)),
        ("assessment", js("Full-loop boundary retained plus final-block padding. Filter0 resets history; no click-free claim or waveform alteration.")),
    ]))
}

fn rust_manifest(clips: &[Clip], cues: &[Cue], ring_base: i64) -> String {
    let mut lines: Vec<String> = vec![
        "// Generated by host/ambience.py; descriptors only, no embedded audio.".into(),
        "#[derive(Clone,Copy)] pub struct AmbienceClip {pub byte_len:usize,pub spu_bytes:usize,pub checksum:u32,pub spu_address:u32,pub pitch:u16,pub source_channel:u8}".into(),
        format!("#[derive(Clone,Copy)] pub struct AmbienceCue {{pub mask:u8,pub gains:[i16;{}],pub fade_ticks:u16}}", clips.len()),
        format!("pub const AMBIENCE_CLIPS:[AmbienceClip;{}]=[", clips.len()),
    ];
    for c in clips {
        lines.push(format!("AmbienceClip{{byte_len:{},spu_bytes:{},checksum:{},spu_address:{},pitch:{},source_channel:{}}},", c.byte_len, c.byte_len, c.checksum, c.spu_address, c.pitch, c.source_channel));
    }
    let end = clips
        .iter()
        .map(|c| c.spu_address + c.byte_len)
        .max()
        .unwrap_or(0);
    lines.extend([
        "];".into(),
        format!("pub const AMBIENCE_SPU_START:u32={SPU_START};"),
        "// The end of the widest set any cue or gate keeps resident, not of every clip at once."
            .into(),
        format!("pub const AMBIENCE_SPU_END:u32={end};"),
        "// Area music keeps this voice and this ring for the life of the disc; the".into(),
        "// pool is handed out when a stem keys on and returned when it finishes fading.".into(),
        format!("pub const MUSIC_VOICE:u8={MUSIC_VOICE};"),
        format!("pub const MUSIC_RING_BASE:u32={ring_base};"),
        format!("pub const MUSIC_RING_BYTES:usize={MUSIC_RING_BYTES};"),
        format!(
            "pub const AMBIENCE_POOL_VOICES:[u8;{}]={};",
            POOL_VOICES.len(),
            py_list(&POOL_VOICES)
        ),
        "// The per-scene one-shot banks' voice (host/scene_sfx.py), outside the pool.".into(),
        format!("pub const SCENE_SFX_VOICE:u8={SCENE_SFX_VOICE};"),
        format!("pub const AMBIENCE_SCENES:[AmbienceCue;{}]=[", cues.len()),
    ]);
    for c in cues {
        lines.push(format!(
            "AmbienceCue{{mask:{},gains:{},fade_ticks:{}}},",
            c.mask,
            py_list(&c.gains),
            c.fade_ticks
        ));
    }
    lines.push("];".into());
    lines.push(
        "/// Per scene: stems of the scenes one gate away that its own cue does not play,".into(),
    );
    lines.push("/// which the drive loads in the background while it is idle.".into());
    lines.push(format!(
        "pub const AMBIENCE_PREFETCH:[u8;{}]={};",
        cues.len(),
        py_list(&cues.iter().map(|c| c.prefetch).collect::<Vec<_>>())
    ));
    lines.join("\n") + "\n"
}

/// `gate_edges`: scene-id pairs joined by a resolved gate, from the region cook's report.
pub(crate) fn gate_edges(root: &Path, scene_files: &[String]) -> Result<BTreeSet<(i64, i64)>> {
    let path = root.join("data/regions.json");
    let report =
        parse(&std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?)?;
    let scenes = jl(&report, "scenes")?;
    let files: Vec<String> = scenes
        .iter()
        .map(|s| js_of(s, "file"))
        .collect::<Result<_>>()?;
    if files != scene_files
        || scenes
            .iter()
            .enumerate()
            .any(|(i, s)| ji(s, "scene_id").ok() != Some(i as i64))
    {
        return err("region report covers a different scene catalog");
    }
    let mut edges = BTreeSet::new();
    for s in scenes {
        let id = ji(s, "scene_id")?;
        for g in jl(s, "resolved_gates")? {
            let target = ji(g, "target_scene")?;
            if target != id {
                edges.insert((id.min(target), id.max(target)));
            }
        }
    }
    Ok(edges)
}

/// `cached_report`: the music report, if it still describes this install and these conversions.
fn cached_report(root: &Path, path: &Path, scene_files: &[String]) -> Result<Json> {
    let report =
        parse(&std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?)?;
    let doctor =
        std::fs::read_to_string(root.join(".hkpsx/doctor.json")).map_err(|e| e.to_string())?;
    let doctor = parse(&doctor)?;
    let current = js_of(&jl(&doctor, "installs")?[0], "data_directory")?;
    let dir = js_of(&report, "source_directory")?;
    let resolve = |p: &str| {
        Path::new(p)
            .canonicalize()
            .unwrap_or_else(|_| Path::new(p).to_path_buf())
    };
    if resolve(&dir) != resolve(&current) {
        return err("music cache belongs to a different selected Windows install");
    }
    // Recheck bytes, not modification times: same-size replacements invalidate.
    if let Json::Obj(inputs) = jv(&report, "inputs")? {
        for (name, value) in inputs {
            if sha_file(&Path::new(&dir).join(name))? != js_of(value, "sha256")? {
                return err(format!("music source changed: {name}"));
            }
        }
    }
    if let Json::Obj(tools) = jv(&report, "tool_hashes")? {
        for (name, value) in tools {
            let text = match name.as_str() {
                "music_report.rs" => sha(include_bytes!("music_report.rs")),
                "music.rs" => sha(include_bytes!("music.rs")),
                "spu.rs" => sha(include_bytes!("spu.rs")),
                _ => return err(format!("music conversion code changed: {name}")),
            };
            if Json::Str(text) != *value {
                return err(format!("music conversion code changed: {name}"));
            }
        }
    }
    let scenes: Vec<String> = jl(&report, "scenes")?
        .iter()
        .map(|s| js_of(s, "scene_file"))
        .collect::<Result<_>>()?;
    if scenes != scene_files {
        return err("music report covers a different scene catalog");
    }
    let resident: Vec<i64> = jl(&report, "resident_atmos")?
        .iter()
        .map(|r| ji(r, "channel"))
        .collect::<Result<_>>()?;
    if resident != CHANNELS {
        return err("music report resolves a different resident atmos set");
    }
    let methods = jv(&report, "source_methods")?;
    if sha_file(&root.join(js_of(methods, "path")?))? != js_of(methods, "sha256")? {
        return err("source methods hash mismatch");
    }
    // Every rate the resident set actually reads, not just the default one.
    let rates: BTreeSet<i64> = CHANNELS.iter().map(|&c| atmos_rate(c)).collect();
    for c in jl(&report, "clips")? {
        if rates.contains(&ji(c, "rate")?) && ji(c, "channels")? == 1 {
            for plane in jl(c, "planes")? {
                let p = root.join(js_of(plane, "path")?);
                if sha_file(&p)? != js_of(plane, "sha256")? || !p.with_extension("s16le").is_file()
                {
                    return err("missing/stale converted profile");
                }
            }
        }
    }
    for rate in &rates {
        let have = jl(&report, "clips")?
            .iter()
            .filter(|c| jint(c, "rate") == Some(*rate) && jint(c, "channels") == Some(1))
            .count();
        if have < CHANNELS.iter().filter(|&&c| atmos_rate(c) == *rate).count() {
            return err("missing ambient profile");
        }
    }
    Ok(report)
}

fn ensure_music(root: &Path, source: &Source, path: &Path, scene_files: &[String]) -> Result<Json> {
    match cached_report(root, path, scene_files) {
        Ok(r) => Ok(r),
        Err(_) => {
            println!("Preparing source music/ambience conversions...");
            crate::music_report::cook(root, source, path.parent().unwrap())?;
            cached_report(root, path, scene_files)
        }
    }
}

fn sfx_reservation(root: &Path) -> Result<(i64, Json)> {
    let data =
        std::fs::read(root.join("data/sfx.adpcm")).map_err(|e| format!("data/sfx.adpcm: {e}"))?;
    if bank_bytes(&root.join("data/sfx.rs"))? != data.len() as i64 || data.len() % 16 != 0 {
        return err("SFX bank/manifest mismatch");
    }
    let end = 0x1010 + data.len() as i64;
    if end > SFX_END {
        return err("ambience overlaps resident SFX bank");
    }
    Ok((
        end,
        jobj(vec![
            ("start", Json::Int(0x1010)),
            ("end", Json::Int(end)),
            ("bytes", Json::Int(data.len() as i64)),
            ("sha256", Json::Str(sha(&data))),
            (
                "manifest_sha256",
                Json::Str(sha_file(&root.join("data/sfx.rs"))?),
            ),
        ]),
    ))
}

pub fn cook(root: &Path, source: &Source) -> Result<()> {
    let report_path = root.join(".hkpsx/music/provenance.json");
    let regions = parse(
        &std::fs::read_to_string(root.join("data/regions.json"))
            .map_err(|e| format!("data/regions.json: {e}"))?,
    )?;
    let scene_files: Vec<String> = jl(&regions, "scenes")?
        .iter()
        .map(|s| js_of(s, "file"))
        .collect::<Result<_>>()?;
    let report = ensure_music(root, source, &report_path, &scene_files)?;
    let methods = root.join(js_of(jv(&report, "source_methods")?, "path")?);
    let s = u(Source::new(js_of(&report, "source_directory")?))?;
    let resources = u(s.file("resources.assets"))?;
    let manager = u(s.read(&u(s.object(&resources, 26261))?))?;
    let mut transitions = HashMap::new();
    let mut snapshots: HashMap<String, Vec<Json>> = HashMap::new();
    for file in &scene_files {
        let scene_file = u(s.file(file))?;
        let mut found = Vec::new();
        for info in scene_file.objects.iter().filter(|i| i.class_id == 114) {
            let o = Obj {
                file: scene_file.clone(),
                info: *info,
            };
            if u(s.typename(&o))? == "SceneManager" {
                found.push(o);
            }
        }
        if found.len() != 1 {
            return err(format!("ambiguous SceneManager in {file}"));
        }
        let scene_manager = u(s.read(&found[0]))?;
        let snapshot = u(s.deref(
            &scene_file,
            crate::common::get(&scene_manager, "atmosSnapshot")?,
        ))?;
        transitions.insert(
            file.clone(),
            crate::common::get(&scene_manager, "transitionTime")?
                .float()
                .ok_or("transitionTime is not a number")?,
        );
        let mut list = Vec::new();
        for channel in CHANNELS {
            let sources = crate::common::get(&manager, "atmosSources")?
                .list()
                .ok_or("atmosSources is not a list")?;
            let audio = u(s.deref(&resources, &sources[channel as usize]))?;
            let tree = u(s.read(&audio))?;
            let group = u(s.deref(
                &audio.file,
                crate::common::get(&tree, "OutputAudioMixerGroup")?,
            ))?;
            list.push(source_snapshot(&s, &snapshot, &group)?);
        }
        snapshots.insert(file.clone(), list);
    }
    let (sfx_end, sfx) = sfx_reservation(root)?;
    let (tail_reserved, tail_banks) = spu_ceiling(root)?;
    let edges = gate_edges(root, &scene_files)?;
    let ring_base = tail_base(root, TAIL_BANKS[0])? - MUSIC_RING_BYTES;
    let (mut clips, cues) = assemble(
        &report,
        root,
        &scene_files,
        &transitions,
        &snapshots,
        sfx_end,
        ring_base,
        &edges,
        POOL_VOICES.len(),
    )?;
    let out = root.join("data/ambience");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let work = root.join(".hkpsx/ambience");
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let mut clip_json = Vec::new();
    for (i, c) in clips.iter_mut().enumerate() {
        let path = out.join(format!("clip_{i}.adpcm"));
        std::fs::write(&path, &c.payload).map_err(|e| e.to_string())?;
        let quality_path = work.join(format!("clip_{i}.adpcm"));
        std::fs::write(&quality_path, &c.payload).map_err(|e| e.to_string())?;
        let quality = decoder_quality(root, c, &quality_path)?;
        clip_json.push(jobj(vec![
            ("source", Json::Str(c.source.clone())),
            ("name", Json::Str(c.name.clone())),
            ("source_channel", Json::Int(c.source_channel)),
            ("rate", Json::Int(c.rate)),
            ("pitch", Json::Int(c.pitch)),
            (
                "actual_rate",
                Json::Float((c.pitch * 44100) as f64 / 4096.0),
            ),
            ("byte_len", Json::Int(c.byte_len)),
            ("spu_bytes", Json::Int(c.byte_len)),
            ("checksum", Json::Int(c.checksum as i64)),
            ("sha256", Json::Str(c.sha256.clone())),
            ("valid_frames", Json::Int(c.valid_frames)),
            ("padding_samples", Json::Int(c.padding_samples)),
            (
                "converted_payload_sha256",
                Json::Str(c.converted_payload_sha256.clone()),
            ),
            ("profile", c.profile.clone()),
            ("spu_address", Json::Int(c.spu_address)),
            ("shares_spu_with", ilist(&c.shares_spu_with)),
            ("path", Json::Str(rel(root, &path))),
            ("quality", quality),
        ]));
    }
    let manifest = root.join("data/ambience.rs");
    std::fs::write(&manifest, rust_manifest(&clips, &cues, ring_base))
        .map_err(|e| e.to_string())?;
    let end = clips
        .iter()
        .map(|c| c.spu_address + c.byte_len)
        .max()
        .unwrap_or(0);
    let cue_json: Vec<Json> = cues
        .iter()
        .map(|c| {
            let mut f = vec![
                ("scene", Json::Int(c.scene)),
                ("source_scene", Json::Str(c.source_scene.clone())),
                ("name", Json::Str(c.name.clone())),
                ("source", Json::Str(c.source.clone())),
                ("mask", Json::Int(c.mask)),
                ("gains", ilist(&c.gains)),
                ("fade_ticks", Json::Int(c.fade_ticks)),
                ("source_fade_seconds", Json::Float(c.source_fade_seconds)),
                ("source_ambience", c.source_ambience.clone()),
                ("clamped_boost_db", c.clamped_boost_db.clone()),
                (
                    "all_channel_snapshot_gains",
                    c.all_channel_snapshot_gains.clone(),
                ),
                ("prefetch", Json::Int(c.prefetch)),
            ];
            if let Some(u) = c.unloaded {
                f.push(("unloaded", Json::Int(u)));
            }
            jobj(f)
        })
        .collect();
    // Which loops each distinct cue keeps in SPU, and what that costs.
    let mut by_cue: Vec<(String, Vec<i64>, i64, Vec<String>)> = Vec::new();
    for c in &cues {
        let key = format!("{} (mask {})", c.name, c.mask);
        let stems: Vec<usize> = (0..clips.len()).filter(|i| c.mask >> i & 1 == 1).collect();
        match by_cue.iter_mut().find(|e| e.0 == key) {
            Some(e) => e.3.push(c.source_scene.clone()),
            None => by_cue.push((
                key,
                stems.iter().map(|&i| clips[i].source_channel).collect(),
                stems.iter().map(|&i| clips[i].byte_len).sum(),
                vec![c.source_scene.clone()],
            )),
        }
    }
    let resident_by_cue = Json::Obj(
        by_cue
            .iter()
            .map(|e| {
                (
                    e.0.clone(),
                    jobj(vec![
                        ("channels", ilist(&e.1)),
                        ("spu_bytes", Json::Int(e.2)),
                        (
                            "scenes",
                            Json::List(e.3.iter().map(|s| Json::Str(s.clone())).collect()),
                        ),
                    ]),
                )
            })
            .collect(),
    );
    let drift = tail_drift(root, end)?;
    let total: i64 = clips.iter().map(|c| c.byte_len).sum();
    let ram_cache: i64 = clips
        .iter()
        .filter(|c| c.byte_len < c.byte_len)
        .map(|c| c.byte_len)
        .sum();
    let clamped: Vec<(String, Json)> = cues
        .iter()
        .filter(|c| matches!(&c.clamped_boost_db, Json::Obj(o) if !o.is_empty()))
        .map(|c| (c.source_scene.clone(), c.clamped_boost_db.clone()))
        .collect();
    let result = jobj(vec![
        ("format", js("raw-psx-adpcm-loops-v1")),
        ("clips", Json::List(clip_json)),
        ("cues", Json::List(cue_json)),
        ("sfx_reservation", sfx),
        ("total_bytes", Json::Int(total)),
        ("spu_bytes", Json::Int(total)),
        ("ram_cache_bytes", Json::Int(ram_cache)),
        ("spu_start", Json::Int(SPU_START)),
        ("spu_end", Json::Int(end)),
        // Free between ambience's widest resident set and the first bank
        // stacked above it, which is where those banks are actually cooked.
        ("spu_free_bytes", Json::Int(ring_base - end)),
        ("spu_ceiling", Json::Int(ring_base)),
        ("music_ring", jobj(vec![("base", Json::Int(ring_base)), ("bytes", Json::Int(MUSIC_RING_BYTES)), ("voice", Json::Int(MUSIC_VOICE))])),
        ("gate_edges", Json::List(edges.iter().map(|&(a, b)| ilist(&[a, b])).collect())),
        ("resident_by_cue", resident_by_cue),
        ("spu_tail_reserved_bytes", Json::Int(tail_reserved)),
        ("spu_tail_banks", Json::Obj(tail_banks.iter().map(|(n, b)| (n.clone(), Json::Int(*b))).collect())),
        ("spu_tail_bases", Json::Obj(TAIL_BANKS.iter().map(|n| Ok((n.to_string(), Json::Int(tail_base(root, n)?)))).collect::<Result<_>>()?)),
        ("resident_atmos_channels", ilist(&CHANNELS)),
        ("voices", ilist(&VOICES)),
        ("music_voice", Json::Int(MUSIC_VOICE)),
        ("pool_voices", ilist(&POOL_VOICES)),
        ("stems_live_across_a_transition", Json::Int(pool_pressure(&cues))),
        (
            "voice_pool_scope",
            Json::Str(format!(
                "Ambience owns voices {}..{} as a pool: one is taken when a stem keys on and returned once it has finished fading out, so the resident set is bounded by how many stems can be audible at once rather than by how many clips there are. Voice {MUSIC_VOICE} is area music's.",
                VOICES[1],
                VOICES[VOICES.len() - 1]
            )),
        ),
        ("resident_atmos_rates", Json::Obj(CHANNELS.iter().map(|&ch| (ch.to_string(), Json::Int(atmos_rate(ch)))).collect())),
        ("clamped_boosts", Json::Obj(clamped)),
        ("clamped_boost_scope", Json::Str(format!("Source Atmos gains above unity play at full scale instead. Refused above {} dB.", crate::pyfloat::repr(BOOST_CEILING_DB)))),
        ("source_report_sha256", Json::Str(sha_file(&report_path)?)),
        ("source_method_sha256", Json::Str(sha_file(&methods)?)),
        ("tool_sha256", Json::Str(sha(include_bytes!("ambience.rs")))),
        ("rust_manifest_sha256", Json::Str(sha_file(&manifest)?)),
        ("mixer_scope", js("Source internal Atmos group and parent gains; downstream output mixers/player settings separate.")),
        (
            "transport",
            Json::Str(format!("All {} loops load whole into SPU at the scene gate that first needs them, into addresses shared by clips no cue or gate keeps resident together; a clip no admitted scene plays never loads. Gate loads only. Sector padding is outside checksum/byte_len.", clips.len())),
        ),
        ("omitted_atmos_channels", js("Every source atmos channel outside resident_atmos_channels; cook_music records them per scene in ambience_omitted_channels and the scene cooks a zero mask.")),
        (
            "spu_tail_drift",
            Json::Obj(drift.iter().map(|(n, d, r)| (n.clone(), jobj(vec![("declared", Json::Int(*d)), ("required", Json::Int(*r))]))).collect()),
        ),
    ]);
    dump(&root.join(".hkpsx/ambience.json"), &result)?;
    println!("{} loops: {} bytes, widest resident set ends {end:#x} free {} below the music ring at {ring_base:#x}", clips.len(), total, ring_base - end);
    for e in &by_cue {
        println!("  {}: channels {}, {} SPU bytes", e.0, py_list(&e.1), e.2);
    }
    if !drift.is_empty() {
        // The bank is written either way; what is stale is the tail above it,
        // and saying so beats leaving the next cook to find the overlap.
        for (name, declared, required) in &drift {
            println!(
                "  {name} declares base {declared:#x} and must be {required:#x}; re-run its cook"
            );
        }
        return err("SPU tail banks no longer abut ambience");
    }
    Ok(())
}

pub fn main(root: &Path, source_dir: Option<&Path>) -> Result<()> {
    let source = match source_dir {
        Some(d) => u(Source::new(d))?,
        None => u(Source::from_doctor(root))?,
    };
    cook(root, &source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spu::validate_loop;

    const FILES: [&str; 3] = ["level6", "level7", "level37"];

    struct Fixture {
        root: std::path::PathBuf,
        report: Json,
        snapshots: HashMap<String, Vec<Json>>,
        slots: Vec<Vec<usize>>,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    fn files() -> Vec<String> {
        FILES.iter().map(|s| s.to_string()).collect()
    }

    /// Three disjoint cue shapes dealt round robin over the resident channels,
    /// one 16-byte clip each (the python fixture of tests/test_ambience.py).
    fn fixture(tag: &str) -> Fixture {
        let root = std::env::temp_dir().join(format!("hk-ambience-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(root.join(".hkpsx")).unwrap();
        let mut clips = Vec::new();
        let mut entries = Vec::new();
        for (i, &ch) in CHANNELS.iter().enumerate() {
            let data: Vec<u8> = [vec![12, 0], vec![i as u8; 14]].concat();
            std::fs::write(root.join(format!(".hkpsx/{i}.adpcm")), &data).unwrap();
            let rate = atmos_rate(ch);
            clips.push(jobj(vec![
                ("source", Json::Str(ch.to_string())),
                ("name", Json::Str(ch.to_string())),
                ("rate", Json::Int(rate)),
                ("channels", Json::Int(1)),
                ("frames", Json::Int(27)),
                ("padding_samples", Json::Int(1)),
                (
                    "spu_pitch",
                    Json::Int(py_round((rate * 4096) as f64 / 44100.0)),
                ),
                (
                    "planes",
                    Json::List(vec![jobj(vec![
                        ("path", Json::Str(format!(".hkpsx/{i}.adpcm"))),
                        ("bytes", Json::Int(16)),
                        ("sha256", Json::Str(sha(&data))),
                    ])]),
                ),
            ]));
            entries.push(jobj(vec![
                ("channel", Json::Int(ch)),
                ("clip", Json::Str(ch.to_string())),
                ("loop", Json::Bool(true)),
                ("pitch", Json::Int(1)),
                ("volume", Json::Int(1)),
                (
                    "snapshot",
                    jobj(vec![
                        ("internal_volume_db", Json::Int(0)),
                        ("effects", Json::List(vec![])),
                        (
                            "chain",
                            Json::List(vec![jobj(vec![
                                ("mute", Json::Bool(false)),
                                ("solo", Json::Bool(false)),
                                ("pitch", Json::Int(1)),
                            ])]),
                        ),
                    ]),
                ),
            ]));
        }
        let pooled: Vec<usize> = (0..CHANNELS.len()).collect();
        let slots: Vec<Vec<usize>> = (0..3)
            .map(|i| pooled.iter().copied().skip(i).step_by(3).collect())
            .collect();
        let mut snapshots = HashMap::new();
        let mut scenes = Vec::new();
        for (file, shape) in FILES.iter().zip(&slots) {
            snapshots.insert(
                file.to_string(),
                (0..CHANNELS.len())
                    .map(|s| {
                        jobj(vec![(
                            "internal_volume_db",
                            if shape.contains(&s) {
                                Json::Int(0)
                            } else {
                                Json::Int(-80)
                            },
                        )])
                    })
                    .collect(),
            );
            scenes.push(jobj(vec![
                ("scene_file", js(file)),
                (
                    "managers",
                    Json::List(vec![jobj(vec![
                        ("source", Json::Str(format!("{file}:1"))),
                        ("atmos_cue", jobj(vec![("name", js(file))])),
                        (
                            "ambience",
                            Json::List(shape.iter().map(|&s| entries[s].clone()).collect()),
                        ),
                    ])]),
                ),
            ]));
        }
        let resident = CHANNELS
            .iter()
            .map(|&ch| {
                jobj(vec![
                    ("channel", Json::Int(ch)),
                    ("clip", Json::Str(ch.to_string())),
                    ("loop", Json::Bool(true)),
                    ("pitch", Json::Int(1)),
                    ("volume", Json::Int(1)),
                ])
            })
            .collect();
        let report = jobj(vec![
            ("clips", Json::List(clips)),
            ("scenes", Json::List(scenes)),
            ("resident_atmos", Json::List(resident)),
        ]);
        Fixture {
            root,
            report,
            snapshots,
            slots,
        }
    }
    impl Fixture {
        fn mask(&self, scene: usize) -> i64 {
            self.slots[scene].iter().map(|s| 1 << s).sum()
        }
        fn assemble(
            &self,
            sfx_end: i64,
            ceiling: i64,
            edges: &[(i64, i64)],
            pool: usize,
        ) -> Result<(Vec<Clip>, Vec<Cue>)> {
            let transitions = FILES.iter().map(|f| (f.to_string(), 0.5)).collect();
            assemble(
                &self.report,
                &self.root,
                &files(),
                &transitions,
                &self.snapshots,
                sfx_end,
                ceiling,
                &edges.iter().copied().collect(),
                pool,
            )
        }
        fn ok(&self, edges: &[(i64, i64)]) -> (Vec<Clip>, Vec<Cue>) {
            // Three shapes cannot cover eight channels with every pair's union
            // inside five voices, so the fixture prices against six.
            self.assemble(SFX_END, 0x80000, edges, 6).unwrap()
        }
    }
    fn disjoint(clips: &[Clip], stems: &[usize]) -> bool {
        let mut spans: Vec<(i64, i64)> = stems
            .iter()
            .map(|&s| {
                (
                    clips[s].spu_address,
                    clips[s].spu_address + clips[s].byte_len,
                )
            })
            .collect();
        spans.sort();
        spans.windows(2).all(|w| w[0].1 <= w[1].0)
    }

    #[test]
    fn loop_flags_preserve_every_encoded_sample_nibble() {
        for blocks in [1usize, 2, 17] {
            let original: Vec<u8> = [
                vec![12, 0],
                vec![0x12; 14],
                [vec![0x2a, 0], vec![0x34; 14]].concat().repeat(blocks - 1),
            ]
            .concat();
            let looped = loop_payload(&original).unwrap();
            validate_loop(&looped).unwrap();
            assert_eq!(looped.len(), original.len());
            for (i, (a, b)) in original.iter().zip(&looped).enumerate() {
                if i % 16 != 1 {
                    assert_eq!(a, b);
                }
            }
            assert_eq!(looped[1], if blocks == 1 { 7 } else { 4 });
            assert_eq!(looped[looped.len() - 15], if blocks == 1 { 7 } else { 3 });
            for i in [1, looped.len() - 15] {
                let mut bad = looped.clone();
                bad[i] = 0;
                assert!(validate_loop(&bad).is_err());
            }
        }
        assert_eq!(fnv(b"hello"), 0x4f9f2cab);
    }

    #[test]
    fn invalid_block_headers_and_input_flags_fail_closed() {
        for data in [
            vec![],
            vec![0; 15],
            [vec![0x10], vec![0; 15]].concat(),
            [vec![13], vec![0; 15]].concat(),
            [vec![0; 16], vec![0x50], vec![0; 15]].concat(),
            [vec![0, 1], vec![0; 14]].concat(),
        ] {
            assert!(loop_payload(&data).is_err(), "{data:?}");
        }
    }

    #[test]
    fn bank_preserves_source_channel_masks_gains_and_disjoint_addresses() {
        let f = fixture("bank");
        let (clips, cues) = f.ok(&[]);
        assert_eq!(
            cues.iter().map(|c| c.mask).collect::<Vec<_>>(),
            [f.mask(0), f.mask(1), f.mask(2)]
        );
        assert_eq!(
            cues[0].gains,
            (0..CHANNELS.len())
                .map(|s| if f.slots[0].contains(&s) { 16383 } else { 2 })
                .collect::<Vec<_>>()
        );
        assert_eq!(
            cues.iter().map(|c| c.fade_ticks).collect::<Vec<_>>(),
            [30, 30, 30]
        );
        assert!(!POOL_VOICES.contains(&MUSIC_VOICE));
        for c in &clips {
            assert_eq!(c.checksum, fnv(&c.payload));
            assert_eq!((c.byte_len, c.valid_frames), (16, 27));
            assert!(c.spu_address >= SPU_START);
        }
        // Stems one cue plays together never share bytes; stems no cue and no
        // gate plays together do, which is the point.
        for shape in &f.slots {
            assert!(disjoint(&clips, shape));
        }
        let end = clips
            .iter()
            .map(|c| c.spu_address + c.byte_len)
            .max()
            .unwrap();
        assert_eq!(
            end,
            SPU_START + 16 * f.slots.iter().map(Vec::len).max().unwrap() as i64
        );
        assert!(end < SPU_START + 16 * clips.len() as i64);
        assert!(f
            .assemble(SFX_END, SPU_START + 32, &[], 6)
            .unwrap_err()
            .contains("capacity overflow"));
        assert!(f
            .assemble(SFX_END + 16, 0x80000, &[], 6)
            .unwrap_err()
            .contains("overlaps"));
        std::fs::write(f.root.join(".hkpsx/0.adpcm"), [0u8; 16]).unwrap();
        assert!(f
            .assemble(SFX_END, 0x80000, &[], 6)
            .unwrap_err()
            .contains("hash mismatch"));
    }

    #[test]
    fn capacity_overflow_names_the_clip_and_the_shortfall() {
        // The widest cue holds three 16-byte payloads; room for two leaves its last one 16 over.
        let f = fixture("overflow");
        assert!(f
            .assemble(SFX_END, SPU_START + 32, &[], 6)
            .unwrap_err()
            .contains("ends 16 bytes past"));
    }

    #[test]
    fn a_gate_keeps_both_scenes_stems_apart() {
        let f = fixture("gate");
        let shared = |clips: &[Clip]| {
            f.slots[0].iter().any(|&a| {
                f.slots[1].iter().any(|&b| {
                    a != b
                        && clips[a].spu_address < clips[b].spu_address + 16
                        && clips[b].spu_address < clips[a].spu_address + 16
                })
            })
        };
        assert!(shared(&f.ok(&[]).0));
        let joined = f.ok(&[(0, 1)]).0;
        assert!(!shared(&joined));
        let both: Vec<usize> = f.slots[0].iter().chain(&f.slots[1]).copied().collect();
        assert!(disjoint(&joined, &both));
    }

    #[test]
    fn the_clips_one_gate_away_are_kept_apart_and_listed_for_prefetch() {
        let f = fixture("prefetch");
        let (clips, cues) = f.ok(&[(0, 1), (0, 2)]);
        assert!(disjoint(&clips, &(0..CHANNELS.len()).collect::<Vec<_>>()));
        assert_eq!(cues[0].prefetch, (f.mask(1) | f.mask(2)) & !f.mask(0));
        assert_eq!(cues[1].prefetch, f.mask(0) & !f.mask(1));
        let text = rust_manifest(&clips, &cues, 0x4e1b0);
        assert!(text.contains(&format!(
            "pub const AMBIENCE_PREFETCH:[u8;3]={};",
            py_list(&cues.iter().map(|c| c.prefetch).collect::<Vec<_>>())
        )));
    }

    #[test]
    fn a_tail_bank_above_a_gap_is_not_drift_but_an_overlap_is() {
        let root = std::env::temp_dir().join(format!("hk-ambience-drift-{}", std::process::id()));
        std::fs::create_dir_all(root.join("data")).unwrap();
        for (name, base) in TAIL_BANKS.iter().zip([0x40000, 0x50000, 0x60000]) {
            std::fs::write(
                root.join(name),
                format!("pub const BANK_BYTES: usize = 4096;\npub const SPU_BASE: u32 = {base};\n"),
            )
            .unwrap();
        }
        assert!(tail_drift(&root, 0x30000).unwrap().is_empty());
        assert_eq!(
            tail_drift(&root, 0x40010).unwrap(),
            [(TAIL_BANKS[0].to_string(), 0x40000, 0x40010)]
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn more_stems_audible_at_once_than_the_pool_holds_is_refused() {
        let f = fixture("pool");
        let (_, cues) = f.ok(&[]);
        let live = pool_pressure(&cues);
        // The fixture's cue shapes share nothing, so the union of any two is wider than either.
        assert!(
            live > cues
                .iter()
                .map(|c| c.mask.count_ones() as i64)
                .max()
                .unwrap()
        );
        assert!(live <= 6);
        let message = f
            .assemble(SFX_END, 0x80000, &[], live as usize - 1)
            .unwrap_err();
        assert!(
            message.contains(&format!("{live} stems can be audible at once"))
                && message.contains("SPU voices"),
            "{message}"
        );
    }

    #[test]
    fn spu_ceiling_excludes_the_banks_stacked_above_ambience() {
        let root = std::env::temp_dir().join(format!("hk-ambience-ceiling-{}", std::process::id()));
        std::fs::create_dir_all(root.join("data")).unwrap();
        for (name, size) in TAIL_BANKS.iter().zip([1024, 2048, 512]) {
            std::fs::write(
                root.join(name),
                format!("pub const BANK_BYTES: usize = {size};\n"),
            )
            .unwrap();
        }
        let (reserved, banks) = spu_ceiling(&root).unwrap();
        assert_eq!(reserved, 3584);
        assert_eq!(
            banks.iter().map(|b| b.1).collect::<Vec<_>>(),
            [1024, 2048, 512]
        );
        std::fs::write(root.join(TAIL_BANKS[0]), "nothing useful\n").unwrap();
        assert!(spu_ceiling(&root).unwrap_err().contains("no BANK_BYTES"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sfx_overlap_and_manifest_drift_rejected() {
        let root = std::env::temp_dir().join(format!("hk-ambience-sfx-{}", std::process::id()));
        std::fs::create_dir_all(root.join("data")).unwrap();
        std::fs::write(root.join("data/sfx.adpcm"), [0u8; 16]).unwrap();
        std::fs::write(
            root.join("data/sfx.rs"),
            "pub const BANK_BYTES: usize = 16;",
        )
        .unwrap();
        assert_eq!(sfx_reservation(&root).unwrap().0, 0x1020);
        std::fs::write(
            root.join("data/sfx.rs"),
            "pub const BANK_BYTES: usize = 32;",
        )
        .unwrap();
        assert!(sfx_reservation(&root).unwrap_err().contains("mismatch"));
        let size = SFX_END - 0x1010 + 16;
        std::fs::write(root.join("data/sfx.adpcm"), vec![0u8; size as usize]).unwrap();
        std::fs::write(
            root.join("data/sfx.rs"),
            format!("pub const BANK_BYTES: usize = {size};"),
        )
        .unwrap();
        assert!(sfx_reservation(&root).unwrap_err().contains("overlaps"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn source_gain_above_unity_clamps_to_full_scale_and_is_recorded() {
        assert_eq!(volume(0.0).unwrap(), 16383);
        assert_eq!(volume(1.35545).unwrap(), 16383);
        assert_eq!(volume(BOOST_CEILING_DB).unwrap(), 16383);
        assert!(volume(BOOST_CEILING_DB + 0.001).is_err());
        assert!(volume(f64::INFINITY).is_err());
        assert_eq!(
            boosts(&[-6.0, 1.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
            Json::Obj(vec![("1".into(), Json::Float(1.5))])
        );
        let mut f = fixture("boost");
        let first = CHANNELS[0];
        f.snapshots.get_mut("level6").unwrap()[0] =
            jobj(vec![("internal_volume_db", Json::Float(1.35545))]);
        if let Json::Obj(top) = &mut f.report {
            if let Some((_, Json::List(scenes))) = top.iter_mut().find(|k| k.0 == "scenes") {
                if let Json::Obj(s) = &mut scenes[0] {
                    if let Some((_, Json::List(m))) = s.iter_mut().find(|k| k.0 == "managers") {
                        if let Json::Obj(mm) = &mut m[0] {
                            if let Some((_, Json::List(a))) =
                                mm.iter_mut().find(|k| k.0 == "ambience")
                            {
                                if let Json::Obj(e) = &mut a[0] {
                                    if let Some((_, Json::Obj(sn))) =
                                        e.iter_mut().find(|k| k.0 == "snapshot")
                                    {
                                        sn.iter_mut()
                                            .find(|k| k.0 == "internal_volume_db")
                                            .unwrap()
                                            .1 = Json::Float(1.35545);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        let (_, cues) = f.ok(&[]);
        assert_eq!(cues[0].gains[0], 16383);
        assert_eq!(
            cues[0].clamped_boost_db,
            Json::Obj(vec![(first.to_string(), Json::Float(1.35545))])
        );
        assert_eq!(cues[1].clamped_boost_db, Json::Obj(vec![]));
    }

    #[test]
    fn manifest_lengths_follow_the_resident_channel_set() {
        let clips: Vec<Clip> = CHANNELS
            .iter()
            .enumerate()
            .map(|(i, &ch)| Clip {
                source: String::new(),
                name: String::new(),
                source_channel: ch,
                rate: 4000,
                pitch: 743,
                byte_len: 16 * (i as i64 + 1),
                checksum: i as u32,
                sha256: String::new(),
                valid_frames: 0,
                padding_samples: 0,
                converted_payload_sha256: String::new(),
                profile: Json::Null,
                payload: Vec::new(),
                spu_address: SPU_START + 16 * i as i64,
                shares_spu_with: Vec::new(),
            })
            .collect();
        let cues = vec![Cue {
            scene: 0,
            source_scene: String::new(),
            name: String::new(),
            source: String::new(),
            mask: 1,
            gains: vec![0; clips.len()],
            fade_ticks: 30,
            source_fade_seconds: 0.5,
            source_ambience: Json::Null,
            clamped_boost_db: Json::Null,
            all_channel_snapshot_gains: Json::Null,
            prefetch: 0,
            unloaded: None,
        }];
        let text = rust_manifest(&clips, &cues, 0x4e1b0);
        assert!(text.contains(&format!(
            "pub const AMBIENCE_CLIPS:[AmbienceClip;{}]=[",
            clips.len()
        )));
        assert!(text.contains(&format!("pub gains:[i16;{}]", clips.len())));
        assert!(!text.contains("voice:"));
        assert!(text.contains(&format!("pub const MUSIC_VOICE:u8={MUSIC_VOICE};")));
        assert!(text.contains("pub const AMBIENCE_POOL_VOICES:[u8;4]=[7, 8, 9, 10];"));
    }

    #[test]
    fn the_cache_cannot_silently_use_another_windows_install() {
        let root = std::env::temp_dir().join(format!("hk-ambience-cache-{}", std::process::id()));
        std::fs::create_dir_all(root.join(".hkpsx")).unwrap();
        std::fs::write(
            root.join(".hkpsx/doctor.json"),
            format!(
                "{{\"installs\": [{{\"data_directory\": \"{}\"}}]}}",
                root.join("selected").display()
            ),
        )
        .unwrap();
        let path = root.join("provenance.json");
        std::fs::write(
            &path,
            format!(
                "{{\"source_directory\": \"{}\"}}",
                root.join("old").display()
            ),
        )
        .unwrap();
        assert!(cached_report(&root, &path, &files())
            .unwrap_err()
            .contains("different selected Windows install"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
