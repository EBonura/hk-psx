//! Complete source Geo one-shots in the reserved 16 KiB SPU range below ambience.
//! Ported from host/geo_audio.py, whose output it reproduces byte for byte
//! (data/geo-audio.adpcm, data/geo-audio.rs and .hkpsx/geo-audio-provenance.json,
//! except the `tool_sha256` table, which names the Rust modules instead).
//!
//! Runs after cook_audio: it re-cooks the resident bank's door clip the way
//! the allocator chose and proves the resident bytes are that source clip.

use crate::common::{err, get, Result};
use crate::cook_audio::{byte_data, convert_wav, div, hex, ints_of, jobj, js, sha, strs_of, u, Resampler};
use crate::pyjson::{dumps, Json};
use crate::spu::{decode_oneshot, fnv, Tool};
use hk_unity::{Obj, Source, Value};
use sha2::{Digest, Sha256};
use std::path::Path;

const SPU_BASE: i64 = 0x14000;
const BANK_LIMIT: i64 = 0x18000 - SPU_BASE;
const SOURCES: [(&str, i64, &str); 6] = [
    ("resources.assets", 1179, "geo_small_collect_1"),
    ("resources.assets", 1144, "geo_small_collect_2"),
    ("resources.assets", 1352, "geo_small_collect_2"),
    ("sharedassets6.assets", 149, "geo_rock_hit_1"),
    ("sharedassets6.assets", 173, "geo_rock_hit_2"),
    ("sharedassets6.assets", 103, "geo_rock_hit_3"),
];

/// Admit only the observed Self, equal-weight, pitch-one source action; the
/// clip PPtrs it plays.
fn random_audio_action(data: &Value) -> Result<Vec<Value>> {
    let names = strs_of(data, "actionNames")?;
    let actions: Vec<usize> = names.iter().enumerate().filter(|(_, n)| *n == "HutongGames.PlayMaker.Actions.AudioPlayRandom").map(|(i, _)| i).collect();
    if actions.len() != 1 {
        return err("expected one AudioPlayRandom");
    }
    let a = actions[0];
    if !get(data, "actionEnabled")?.list().and_then(|l| l.get(a)).is_some_and(Value::truthy) {
        return err("disabled audio action");
    }
    let starts = ints_of(data, "actionStartIndex")?;
    let param_names = strs_of(data, "paramName")?;
    let kinds = ints_of(data, "paramDataType")?;
    let positions = ints_of(data, "paramDataPos")?;
    let sizes = ints_of(data, "paramByteDataSize")?;
    let bytes = byte_data(data)?;
    let start = starts[a] as usize;
    let end = if a + 1 < names.len() { starts[a + 1] as usize } else { param_names.len() };
    let mut fields: Vec<(&str, usize)> = Vec::new();
    for i in start..end {
        if param_names[i].is_empty() {
            continue;
        }
        match fields.iter_mut().find(|(k, _)| *k == param_names[i]) {
            Some(slot) => slot.1 = i,
            None => fields.push((&param_names[i], i)),
        }
    }
    let mut keys: Vec<&str> = fields.iter().map(|f| f.0).collect();
    keys.sort_unstable();
    if keys != ["audioClips", "gameObject", "pitchMax", "pitchMin", "weights"] {
        return err("changed audio action fields");
    }
    let field = |n: &str| fields.iter().find(|(k, _)| *k == n).unwrap().1;
    let scalar = |i: usize| -> Result<f64> {
        let raw = crate::cook_audio::slice(&bytes, positions[i], sizes[i]);
        if kinds[i] != 15 || raw.len() != 5 || raw[4] != 0 {
            return err("dynamic audio scalar");
        }
        let value = f32::from_le_bytes(raw[..4].try_into().unwrap()) as f64;
        if value != 1.0 {
            return err("changed audio weight or pitch");
        }
        Ok(value)
    };
    let array_sizes = ints_of(data, "arrayParamSizes")?;
    let array_types = strs_of(data, "arrayParamTypes")?;
    let array = |name: &str, typename: &str| -> Result<Vec<usize>> {
        let i = field(name);
        if kinds[i] != 12 {
            return err("changed audio array type");
        }
        let index = positions[i] as usize;
        let count = *array_sizes.get(index).ok_or("arrayParamSizes index out of range")?;
        if array_types.get(index).map(String::as_str) != Some(typename) || !(1..=3).contains(&count) || i + count as usize >= end {
            return err("changed audio array");
        }
        let range = i + 1..i + 1 + count as usize;
        if range.clone().any(|j| !param_names[j].is_empty()) {
            return err("named array element");
        }
        Ok(range.collect())
    };
    let i = field("gameObject");
    if kinds[i] != 19 {
        return err("changed audio owner type");
    }
    let owners = get(data, "fsmGameObjectParams")?.list().ok_or("fsmGameObjectParams is not a list")?;
    let owner = owners.get(positions[i] as usize).ok_or("fsmGameObjectParams index out of range")?;
    if get(owner, "useVariable")?.int() != Some(1) || get(owner, "name")?.str().as_deref() != Some("Self") {
        return err("changed audio owner");
    }
    let clip_slots = array("audioClips", "UnityEngine.AudioClip")?;
    let mut clips = Vec::new();
    for j in clip_slots {
        if kinds[j] != 5 {
            return err("dynamic clip reference");
        }
        let params = get(data, "unityObjectParams")?.list().ok_or("unityObjectParams is not a list")?;
        let r = params.get(positions[j] as usize).ok_or("unityObjectParams index out of range")?;
        if get(r, "m_PathID")?.int().unwrap_or(0) == 0 {
            return err("null clip");
        }
        clips.push(r.clone());
    }
    let weights = array("weights", "HutongGames.PlayMaker.FsmFloat")?.into_iter().map(scalar).collect::<Result<Vec<_>>>()?;
    if clips.len() != weights.len() {
        return err("clip/weight count mismatch");
    }
    scalar(field("pitchMin"))?;
    scalar(field("pitchMax"))?;
    Ok(clips)
}

struct Clip {
    source: String,
    encoded: Vec<u8>,
    meta: Vec<(String, Json)>,
}

/// One bank record: the clip's meta, then where it landed.
fn pack_bank(items: &[Clip], limit: i64) -> Result<(Vec<u8>, Vec<Json>, Vec<i64>)> {
    if items.len() != SOURCES.len() {
        return err("incomplete Geo bank");
    }
    let mut bank = Vec::new();
    let mut records = Vec::new();
    let mut addresses = Vec::new();
    for (clip, (file, pid, _)) in items.iter().zip(SOURCES) {
        if clip.source != format!("{file}:{pid}") {
            return err("reordered Geo source IDs");
        }
        decode_oneshot(&clip.encoded)?;
        let mut meta = clip.meta.clone();
        meta.push(("source".into(), Json::Str(clip.source.clone())));
        meta.push(("offset".into(), Json::Int(bank.len() as i64)));
        meta.push(("bytes".into(), Json::Int(clip.encoded.len() as i64)));
        meta.push(("spu_address".into(), Json::Int(SPU_BASE + bank.len() as i64)));
        records.push(Json::Obj(meta));
        addresses.push(SPU_BASE + bank.len() as i64);
        bank.extend_from_slice(&clip.encoded);
    }
    if bank.len() as i64 > limit {
        return err("Geo bank exceeds remaining SPU capacity");
    }
    Ok((bank, records, addresses))
}

pub fn cook(root: &Path, source: &Source) -> Result<()> {
    let tool = Tool::build(root, "hk-geo-audio")?;
    let resources = u(source.file("resources.assets"))?;
    let level = u(source.file("level6"))?;
    let read = |o: &Obj| u(source.read(o));
    let sid_of = |file: &std::sync::Arc<hk_unity::serialized::SerializedFile>, pptr: &Value| -> Result<String> { Ok(u(source.deref(file, pptr))?.sid()) };

    // The event's one AudioSource, at unit gain and pitch, unmuted.
    let audio_source = |obj: &Obj| -> Result<String> {
        let go = read(&u(source.deref(&obj.file, get(&read(obj)?, "m_GameObject")?))?)?;
        let mut audio = Vec::new();
        for c in get(&go, "m_Component")?.list().unwrap_or(&[]) {
            audio.push(u(source.deref(&obj.file, get(c, "component")?))?);
        }
        let audio: Vec<&Obj> = audio.iter().filter(|o| o.class_id() == 82).collect();
        if audio.len() != 1 {
            return err("expected one event AudioSource");
        }
        let tree = read(audio[0])?;
        if get(&tree, "m_Volume")?.float() != Some(1.0) || get(&tree, "m_Pitch")?.float() != Some(1.0) || get(&tree, "Mute")?.truthy() {
            return err("changed event AudioSource gain/pitch/mute");
        }
        Ok(audio[0].sid())
    };
    let mut coins = Vec::new();
    for (pid, ids) in [(25168, [1179, 1144]), (26170, [1179, 1144]), (27009, [1179, 1352])] {
        let obj = u(source.object(&resources, pid))?;
        if u(source.typename(&obj))? != "GeoControl" {
            return err("changed GeoControl");
        }
        let mut refs = Vec::new();
        for r in get(&read(&obj)?, "pickupSounds")?.list().unwrap_or(&[]) {
            refs.push(sid_of(&resources, r)?);
        }
        if refs != ids.iter().map(|i| format!("resources.assets:{i}")).collect::<Vec<_>>() {
            return err("changed pickup sound mapping");
        }
        coins.push(jobj(vec![("component", Json::Str(obj.sid())), ("clips", Json::List(refs.into_iter().map(Json::Str).collect())), ("audio_source", Json::Str(audio_source(&obj)?))]));
    }
    let mut rocks = Vec::new();
    for pid in [12121, 12126, 12186, 12227, 12275] {
        let obj = u(source.object(&level, pid))?;
        let tree = read(&obj)?;
        let states = get(get(&tree, "fsm")?, "states")?.list().ok_or("states is not a list")?;
        let mut result = vec![("fsm".to_string(), Json::Str(obj.sid())), ("audio_source".into(), Json::Str(audio_source(&obj)?))];
        for (state, ids) in [("Check Direction", &[149, 173, 103][..]), ("Destroy", &[92, 99][..])] {
            let found = states.iter().rev().find(|s| s.get("name").and_then(Value::str).as_deref() == Some(state)).ok_or_else(|| format!("missing state {state}"))?;
            let mut refs = Vec::new();
            for r in random_audio_action(get(found, "actionData")?)? {
                refs.push(sid_of(&level, &r)?);
            }
            if refs != ids.iter().map(|i| format!("sharedassets6.assets:{i}")).collect::<Vec<_>>() {
                return err("changed rock sound mapping");
            }
            result.push((state.to_string(), Json::List(refs.into_iter().map(Json::Str).collect())));
        }
        rocks.push(Json::Obj(result));
    }

    let cook_clip = |file: &str, pid: i64, name: &str, rate: i64, resampler: Resampler| -> Result<(Clip, Vec<u8>)> {
        let obj = u(source.object(&u(source.file(file))?, pid))?;
        let tree = read(&obj)?;
        if tree.get("m_Name").and_then(Value::str).as_deref() != Some(name) {
            return err("changed source clip");
        }
        let wav = crate::fmod::clip_wav(root, source, &tree)?;
        let (pcm, mut meta) = convert_wav(&tool, &wav, rate, resampler)?;
        let encoded = tool.encode_oneshot(&pcm)?;
        let mut decoded = decode_oneshot(&encoded)?;
        decoded.truncate(pcm.len());
        let n = pcm.len().max(1) as i64;
        let mse = div(pcm.iter().zip(&decoded).map(|(&a, &b)| (a as i64 - b as i64).pow(2)).sum::<i64>(), n);
        let power = div(pcm.iter().map(|&a| a as i64 * a as i64).sum::<i64>(), n);
        let int = |k: &str| meta.iter().find(|(n, _)| n == k).and_then(|(_, v)| if let Json::Int(i) = v { Some(*i) } else { None }).unwrap_or(0);
        let duration = div(int("source_frames"), int("source_rate"));
        meta.extend([
            ("name".to_string(), js(name)),
            ("source_wav_sha256".into(), Json::Str(sha(&wav))),
            ("encoded_sha256".into(), Json::Str(sha(&encoded))),
            ("snr_db".into(), if mse != 0.0 && power != 0.0 { Json::Float(10.0 * (power / mse).log10()) } else { Json::Null }),
            ("source_duration_seconds".into(), Json::Float(duration)),
        ]);
        Ok((Clip { source: obj.sid(), encoded, meta }, wav))
    };
    let mut items = Vec::new();
    for (file, pid, name) in SOURCES {
        items.push(cook_clip(file, pid, name, 11025, Resampler::Ffmpeg)?.0);
    }
    let (bank, records, addresses) = pack_bank(&items, BANK_LIMIT)?;

    // The door clip is the resident hero bank's first sample, cooked by
    // cook_audio at the rate its allocator chose and with its resampler:
    // re-cook it the same way to prove the bytes are that source clip.
    let path = root.join(".hkpsx/audio-provenance.json");
    let hero: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?).map_err(|e| e.to_string())?;
    let door_record = hero["events"].as_array().and_then(|l| l.iter().find(|e| e["event"] == "door")).ok_or("no door event in the resident bank")?;
    if door_record["source_id"] != "sharedassets6.assets:92" || door_record["offset"] != 0 {
        return err("resident door sample moved");
    }
    let door_rate = door_record["sample_rate"].as_i64().ok_or("door without a sample rate")?;
    let door_resampler = match door_record.get("resampler").and_then(|r| r.as_str()) {
        None | Some("ffmpeg") => Resampler::Ffmpeg,
        Some("sdk") => Resampler::Sdk,
        Some(other) => return err(format!("unknown resampler {other}")),
    };
    let (door, _) = cook_clip("sharedassets6.assets", 92, "breakable_wall_hit_1", door_rate, door_resampler)?;
    let resident = std::fs::read(root.join("data/sfx.adpcm")).map_err(|e| format!("data/sfx.adpcm: {e}"))?;
    if resident.get(..door.encoded.len()) != Some(&door.encoded[..]) {
        return err("resident door sample no longer equals source rock-break clip");
    }
    std::fs::write(root.join("data/geo-audio.adpcm"), &bank).map_err(|e| e.to_string())?;
    // The guest streams this bank from the disc, so it carries the pack chunk's
    // expected length and FNV-1a checksum rather than the bytes themselves.
    let mut manifest = String::from("// Generated from complete Windows source clips; see ignored Geo audio provenance.\n");
    manifest += &format!("pub const BANK_BYTES: usize = {};\n", bank.len());
    manifest += &format!("pub const BANK_CHECKSUM: u32 = {};\n", fnv(&bank));
    manifest += "const SAMPLES: [(u32,u32,i16);6] = [\n";
    for a in &addresses {
        manifest += &format!("    ({a},11025,5461),\n");
    }
    manifest += "];\n";
    std::fs::write(root.join("data/geo-audio.rs"), manifest).map_err(|e| e.to_string())?;

    let mut inputs = Vec::new();
    for file in ["resources.assets", "resources.resource", "sharedassets6.assets", "sharedassets6.resource", "level6", "Managed/Assembly-CSharp.dll"] {
        let p = source.directory.join(file);
        if p.is_file() {
            inputs.push((file.to_string(), Json::Str(sha(&std::fs::read(&p).map_err(|e| e.to_string())?))));
        }
    }
    let mut break_reuse = door.meta.clone();
    break_reuse.extend([
        ("source".to_string(), js("sharedassets6.assets:92")),
        ("address".into(), Json::Int(0x1010)),
        ("resident_bank_sha256".into(), Json::Str(sha(&resident))),
    ]);
    let tool_sha = |bytes: &[u8]| Json::Str(hex(&Sha256::digest(bytes)));
    let provenance = jobj(vec![
        ("version", Json::Int(1)),
        ("source_inputs", Json::Obj(inputs)),
        ("origins", jobj(vec![("coins", Json::List(coins)), ("rocks", Json::List(rocks))])),
        ("clips", Json::List(records)),
        ("bytes", Json::Int(bank.len() as i64)),
        ("spu_base", Json::Int(SPU_BASE)),
        ("spu_end", Json::Int(SPU_BASE + bank.len() as i64)),
        ("remaining_spu_bytes", Json::Int(BANK_LIMIT - bank.len() as i64)),
        ("bank_sha256", Json::Str(sha(&bank))),
        ("voices", jobj(vec![("pickup", Json::Int(12)), ("hit", Json::Int(13)), ("break", Json::Int(14))])),
        ("source_gain", Json::Float(1.0)),
        ("mix_headroom_gain", Json::Float(1.0 / 3.0)),
        ("break_reuse", Json::Obj(break_reuse)),
        (
            "limitations",
            Json::List(
                [
                    "Second rock-destruction variant sharedassets6.assets:99 omitted for SPU budget.",
                    "Complete clips folded to mono and polyphase-resampled to 11025 Hz before SDK psx-audio-cook ADPCM.",
                    "Equal-weight deterministic event PRNG differs from original Unity RNG.",
                    "One voice per event class; rapid same-class retriggers replace that class.",
                    "Initial source pitch 1 used; inherited Geo bounce pitch, spatial attenuation and source mixer are not reproduced.",
                ]
                .iter()
                .map(|s| js(s))
                .collect(),
            ),
        ),
        (
            "source_methods",
            Json::List(["GeoControl.PlayCollectSound: Random.Range and AudioSource.PlayOneShot", "AudioPlayRandom.DoPlayRandomClip: weighted choice, pitch and PlayOneShot"].iter().map(|s| js(s)).collect()),
        ),
        (
            "tool_sha256",
            jobj(vec![
                ("geo_audio.rs", tool_sha(include_bytes!("geo_audio.rs"))),
                ("cook_audio.rs", tool_sha(include_bytes!("cook_audio.rs"))),
                ("spu.rs", tool_sha(include_bytes!("spu.rs"))),
            ]),
        ),
    ]);
    std::fs::write(root.join(".hkpsx/geo-audio-provenance.json"), dumps(&provenance)).map_err(|e| e.to_string())?;
    println!("Geo audio: {}/{BANK_LIMIT} bytes, complete six clips, end {:#x}", bank.len(), SPU_BASE + bank.len() as i64);
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
    use crate::cook_audio::tests::{bytes_of, int, list, map, text, tool};

    /// The observed action, with one thing the test breaks.
    struct Action {
        enabled: i64,
        bytes: Vec<u8>,
        owner: &'static str,
        first_clip: i64,
        sizes: i64,
        kind_of_second_slot: i64,
    }
    fn action(a: &Action) -> Value {
        let pptr = |i: i64| map(vec![("m_FileID", int(2)), ("m_PathID", int(i))]);
        map(vec![
            ("actionNames", list(vec![text("HutongGames.PlayMaker.Actions.AudioPlayRandom")])),
            ("actionEnabled", list(vec![int(a.enabled)])),
            ("actionStartIndex", list(vec![int(0)])),
            ("paramName", list(["gameObject", "audioClips", "", "", "weights", "", "", "pitchMin", "pitchMax"].iter().map(|s| text(s)).collect())),
            ("paramDataType", list([19, 12, 5, a.kind_of_second_slot, 12, 15, 15, 15, 15].iter().map(|&x| int(x)).collect())),
            ("paramDataPos", list([0, 0, 0, 1, 1, 0, 5, 10, 15].iter().map(|&x| int(x)).collect())),
            ("paramByteDataSize", list([0, 0, 0, 0, 0, 5, 5, 5, 5].iter().map(|&x| int(x)).collect())),
            ("byteData", bytes_of(&a.bytes)),
            ("arrayParamSizes", list(vec![int(a.sizes), int(2)])),
            ("arrayParamTypes", list(vec![text("UnityEngine.AudioClip"), text("HutongGames.PlayMaker.FsmFloat")])),
            ("fsmGameObjectParams", list(vec![map(vec![("useVariable", int(1)), ("name", text(a.owner))])])),
            ("unityObjectParams", list(vec![pptr(a.first_clip), pptr(99)])),
        ])
    }
    fn observed() -> Action {
        let one: Vec<u8> = [1.0f32.to_le_bytes().to_vec(), vec![0]].concat();
        Action { enabled: 1, bytes: one.repeat(4), owner: "Self", first_clip: 92, sizes: 2, kind_of_second_slot: 5 }
    }

    #[test]
    fn source_action_only_constant_equal_weight_self() {
        let clips = random_audio_action(&action(&observed())).unwrap();
        assert_eq!(clips.iter().map(|c| c.get("m_PathID").and_then(Value::int)).collect::<Vec<_>>(), [Some(92), Some(99)]);
        let breaks: Vec<Box<dyn Fn(&mut Action)>> = vec![
            Box::new(|a| a.enabled = 0),
            Box::new(|a| a.bytes[4] = 1),
            Box::new(|a| a.bytes[13] = 64),
            Box::new(|a| a.owner = "Other"),
            Box::new(|a| a.first_clip = 0),
            Box::new(|a| a.sizes = 3),
            Box::new(|a| a.kind_of_second_slot = 24),
        ];
        for (i, b) in breaks.iter().enumerate() {
            let mut bad = observed();
            b(&mut bad);
            assert!(random_audio_action(&action(&bad)).is_err(), "mutation {i} was admitted");
        }
    }

    fn items(sources: &[(&str, i64)]) -> Vec<Clip> {
        sources
            .iter()
            .enumerate()
            .map(|(i, (file, pid))| Clip { source: format!("{file}:{pid}"), encoded: tool().encode_oneshot(&vec![i as i16 * 1000; 28]).unwrap(), meta: Vec::new() })
            .collect()
    }
    fn ordered() -> Vec<(&'static str, i64)> {
        SOURCES.iter().map(|s| (s.0, s.1)).collect()
    }

    #[test]
    fn complete_ordered_bank_exact_limit_and_distinct_same_names() {
        let (bank, _, addresses) = pack_bank(&items(&ordered()), 192).unwrap();
        assert_eq!(bank.len(), 192);
        assert_eq!(addresses[5], SPU_BASE + 160);
        assert_ne!(bank[32..64], bank[64..96]);
        assert!(pack_bank(&items(&ordered()), 191).is_err());
        assert!(pack_bank(&items(&ordered()[..5]), BANK_LIMIT).is_err());
        let mut reversed = ordered();
        reversed.reverse();
        assert!(pack_bank(&items(&reversed), BANK_LIMIT).is_err());
    }
}
