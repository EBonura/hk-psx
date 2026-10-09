//! Exact full Runner loop/calls, prepared in ignored storage for guest binding.
//! Ported from host/runner_audio.py, whose `data/runner-audio.*` and
//! `.hkpsx/runner71/audio/report.json` it reproduces byte for byte (except the
//! report's `identity` entries for the code, which name the Rust modules).

use crate::common::{err, get, py_round, Result};
use crate::cook_audio::{div, jobj, js, sha, u};
use crate::focus_audio::fields;
use crate::music::{cook_clip, dump, jget, jint, rel, sha_file, Profile, Resampler};
use crate::pyjson::{dumps_sorted, parse, Json};
use crate::runner::{runner_actors, ASSEMBLIES};
use crate::spu::{fnv, loop_payload, oneshot_payload, validate_blocks, validate_loop, Tool};
use hk_unity::scene::Scene;
use hk_unity::{Source, Value};
use std::path::{Path, PathBuf};

/// Immediately above the Focus bank and ending at 0x7FFF0, below the 16 bytes
/// psx_spu::init parks the disabled reverb work area on.
pub const SPU_BASE: i64 = 503808;
pub const SPU_END: i64 = 524288;
pub const GAIN: i64 = 5461;
pub const SOURCES: [(&str, &str, i64, bool); 3] = [
    ("walk_loop", "sharedassets32.assets:63", 8000, true),
    ("chase_1", "sharedassets37.assets:27", 11025, false),
    ("chase_2", "sharedassets37.assets:26", 11025, false),
];
const INITIAL_PITCH: f64 = 0.9973541498184204;
const CHASE_BOUNDS: [f64; 2] = [0.8500000238418579, 1.149999976158142];

fn require(ok: bool, message: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        err(message)
    }
}

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
fn jf(j: &Json, key: &str) -> Result<f64> {
    match jv(j, key)? {
        Json::Int(i) => Ok(*i as f64),
        Json::Float(f) => Ok(*f),
        _ => err(format!("{key} is not a number")),
    }
}
fn jstr(j: &Json, key: &str) -> Result<String> {
    match jv(j, key)? {
        Json::Str(s) => Ok(s.clone()),
        _ => err(format!("{key} is not a string")),
    }
}
fn jb(j: &Json, key: &str) -> Result<bool> {
    match jv(j, key)? {
        Json::Bool(b) => Ok(*b),
        _ => err(format!("{key} is not a boolean")),
    }
}
fn floats(v: &[f64]) -> Json {
    Json::List(v.iter().map(|&f| Json::Float(f)).collect())
}
fn ints(v: &[i64]) -> Json {
    Json::List(v.iter().map(|&i| Json::Int(i)).collect())
}

/// `object_identity`: the clip's metadata and encoded-resource hashes.
fn object_identity(source: &Source, obj: &hk_unity::Obj) -> Result<(Json, Value)> {
    let tree = u(source.read(obj))?;
    let resource = get(&tree, "m_Resource")?;
    let name = get(resource, "m_Source")?.str().ok_or("m_Source")?;
    let path = source.directory.join(&name);
    let resolved = path
        .canonicalize()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    require(
        resolved.starts_with(source.directory.canonicalize().map_err(|e| e.to_string())?),
        "audio resource escapes source",
    )?;
    let (offset, size) = (
        get(resource, "m_Offset")?.int().unwrap_or(0) as usize,
        get(resource, "m_Size")?.int().unwrap_or(0) as usize,
    );
    let all = std::fs::read(&resolved).map_err(|e| e.to_string())?;
    let raw = all
        .get(offset..offset + size)
        .ok_or("truncated source audio")?;
    let mut text = String::new();
    dumps_sorted(&tree, &mut text);
    Ok((
        jobj(vec![
            ("clip_metadata_sha256", Json::Str(sha(text.as_bytes()))),
            ("encoded_resource_sha256", Json::Str(sha(raw))),
        ]),
        tree,
    ))
}

/// `source_contract`: the Runner placements and clips the bank is specified from.
fn source_contract(source: &Source) -> Result<Json> {
    let scene = u(Scene::new(source, "level37"))?;
    let actors = runner_actors(&scene, source)?;
    require(
        actors.len() == 2,
        "expected both strictly recognized Runners",
    )?;
    for a in &actors {
        let b = &a.audio;
        let ok = b.walk_loop == SOURCES[0].1
            && b.chase == [SOURCES[1].1, SOURCES[2].1]
            && b.looped
            && b.volume == 1.0
            && b.initial_pitch == INITIAL_PITCH
            && !b.play_on_awake;
        require(ok, "Runner AudioSource contract changed")?;
    }
    let mut actor_records = Vec::new();
    for actor in &actors {
        let records = crate::common::component_records(&scene, actor.game_object);
        let fsm_tree = records
            .iter()
            .find(|r| r.1 == "PlayMakerFSM")
            .ok_or("Runner has no FSM")?
            .2;
        let fsm = get(fsm_tree, "fsm")?;
        let anticipate = get(
            get(fsm, "states")?
                .list()
                .unwrap_or(&[])
                .iter()
                .find(|s| s.get("name").and_then(Value::str).as_deref() == Some("Anticipate"))
                .ok_or("no Anticipate state")?,
            "actionData",
        )?;
        let names = get(anticipate, "actionNames")?.list().unwrap_or(&[]);
        let enabled = get(anticipate, "actionEnabled")?.list().unwrap_or(&[]);
        let picked: Vec<usize> = names
            .iter()
            .enumerate()
            .filter(|(i, n)| {
                n.str().is_some_and(|n| n.ends_with(".AudioPlayRandom"))
                    && enabled.get(*i).is_some_and(Value::truthy)
            })
            .map(|(i, _)| i)
            .collect();
        require(picked.len() == 1, "Runner chase action changed")?;
        let action = fields(anticipate, picked[0])?;
        let pitch: Vec<&Value> = ["pitchMin", "pitchMax"]
            .iter()
            .map(|k| hk_unity::playmaker::field(&action, k).ok_or_else(|| format!("missing {k}")))
            .collect::<Result<_>>()?;
        require(
            pitch
                .iter()
                .all(|p| !p.get("useVariable").is_some_and(Value::truthy)),
            "dynamic Runner chase pitch",
        )?;
        let bounds: Vec<f64> = pitch
            .iter()
            .map(|p| p.get("value").and_then(Value::float).ok_or("pitch value"))
            .collect::<std::result::Result<_, _>>()?;
        require(
            bounds
                .iter()
                .zip([0.85, 1.15])
                .all(|(a, b)| (a - b).abs() < 1e-6),
            "Runner chase pitch bounds changed",
        )?;
        let audio_id = records
            .iter()
            .find(|r| r.1 == "AudioSource")
            .ok_or("Runner has no AudioSource")?
            .0;
        actor_records.push(jobj(vec![
            ("actor", Json::Str(actor.source.clone())),
            ("audio_source", Json::Str(format!("level37:{audio_id}"))),
            ("source_volume", Json::Float(1.0)),
            ("initial_pitch", Json::Float(INITIAL_PITCH)),
            ("chase_pitch_bounds", floats(&bounds)),
            ("chase_weights", floats(&[1.0, 1.0])),
        ]));
    }
    let first_bounds = jl(&actor_records[0], "chase_pitch_bounds")?.to_vec();
    let mut clips = Vec::new();
    for (event, sid, rate, looped) in SOURCES {
        let (file, pid) = sid.split_once(':').unwrap();
        let obj = u(source.object(
            &u(source.file(file))?,
            pid.parse::<i64>().map_err(|e| e.to_string())?,
        ))?;
        require(obj.class_id() == 83, "Runner source is not AudioClip")?;
        let (identity, tree) = object_identity(source, &obj)?;
        clips.push(jobj(vec![
            ("event", js(event)),
            ("source", js(sid)),
            ("rate", Json::Int(rate)),
            ("loop", Json::Bool(looped)),
            (
                "name",
                Json::Str(get(&tree, "m_Name")?.str().unwrap_or_default()),
            ),
            ("source_identity", identity),
            (
                "source_length_seconds",
                Json::Float(get(&tree, "m_Length")?.float().ok_or("m_Length")?),
            ),
            ("source_volume", Json::Float(1.0)),
            ("gain", Json::Int(GAIN)),
            (
                "pitch_multipliers",
                if looped {
                    floats(&[INITIAL_PITCH, INITIAL_PITCH])
                } else {
                    Json::List(first_bounds.clone())
                },
            ),
        ]));
    }
    Ok(jobj(vec![
        ("actors", Json::List(actor_records)),
        ("clips", Json::List(clips)),
        (
            "scope",
            js("Bank specification only; no guest playback or voice allocation is installed"),
        ),
    ]))
}

/// `assemble`: pure bank specification: verify source-bound profiles; never
/// truncate to fit.
pub fn assemble(
    profiles: &[Profile],
    root: &Path,
    contract: &Json,
    spu_base: i64,
) -> Result<(Vec<u8>, Json)> {
    require(
        spu_base == SPU_BASE && spu_base % 16 == 0,
        "Runner SPU tail changed; re-audit layout",
    )?;
    let specs = jl(contract, "clips")?;
    require(
        profiles.len() == SOURCES.len() && SOURCES.len() == specs.len(),
        "Runner bank inventory incomplete",
    )?;
    let mut bank: Vec<u8> = Vec::new();
    let mut records = Vec::new();
    for ((profile, spec), (event, sid, rate, looped)) in profiles.iter().zip(specs).zip(SOURCES) {
        require(
            jstr(spec, "event")? == event
                && jstr(spec, "source")? == sid
                && ji(spec, "rate")? == rate
                && jb(spec, "loop")? == looped,
            "Runner source specification reordered or changed",
        )?;
        require(
            profile.source == sid && profile.rate == rate && profile.channels == 1,
            "Runner profile source/category rate changed",
        )?;
        require(
            profile.name == jstr(spec, "name")?
                && profile.source_identity == *jv(spec, "source_identity")?,
            "stale Runner source identity",
        )?;
        require(
            profile.spu_pitch == py_round(div(rate * 4096, 44100)),
            "Runner profile pitch mismatch",
        )?;
        require(
            profile.planes.len() == 1 && profile.frames > 0,
            "invalid Runner mono profile",
        )?;
        // ffmpeg may round the final rational resampling interval by one sample.
        let length = jf(spec, "source_length_seconds")?;
        require(
            profile.source_pcm_frames > 0
                && profile.source_pcm_rate > 0
                && (div(profile.source_pcm_frames, profile.source_pcm_rate) - length).abs()
                    <= 1.0 / profile.source_pcm_rate as f64 + 1e-6
                && (profile.frames as f64
                    - (profile.source_pcm_frames * rate) as f64 / profile.source_pcm_rate as f64)
                    .abs()
                    <= 1.0,
            "Runner profile duration was trimmed or changed",
        )?;
        let plane = &profile.planes[0];
        let path: PathBuf = if plane.path.is_absolute() {
            plane.path.clone()
        } else {
            root.join(&plane.path)
        };
        let hk = root
            .join(".hkpsx")
            .canonicalize()
            .map_err(|e| e.to_string())?;
        require(
            path.canonicalize()
                .map_err(|e| format!("{}: {e}", path.display()))?
                .starts_with(hk),
            "Runner profile escapes ignored storage",
        )?;
        let raw = std::fs::read(&path).map_err(|e| e.to_string())?;
        require(
            raw.len() as i64 == plane.bytes
                && plane.bytes == profile.bytes
                && sha(&raw) == plane.sha256,
            "Runner profile payload integrity mismatch",
        )?;
        require(
            raw.len() as i64 == (profile.frames + 27) / 28 * 16
                && profile.padding_samples == (-profile.frames).rem_euclid(28),
            "Runner encoded length/padding mismatch",
        )?;
        let payload = if looped {
            loop_payload(&raw)?
        } else {
            oneshot_payload(&raw)?
        };
        let offset = bank.len() as i64;
        let multipliers: Vec<f64> = jl(spec, "pitch_multipliers")?
            .iter()
            .map(|m| match m {
                Json::Float(f) => *f,
                Json::Int(i) => *i as f64,
                _ => f64::NAN,
            })
            .collect();
        let Json::Obj(mut record) = spec.clone() else {
            return err("clip spec is not an object");
        };
        record.extend([
            ("offset".to_string(), Json::Int(offset)),
            ("spu_address".into(), Json::Int(spu_base + offset)),
            ("byte_len".into(), Json::Int(payload.len() as i64)),
            ("checksum".into(), Json::Int(fnv(&payload) as i64)),
            ("sha256".into(), Json::Str(sha(&payload))),
            ("pitch".into(), Json::Int(profile.spu_pitch)),
            (
                "pitch_bounds".into(),
                ints(
                    &multipliers
                        .iter()
                        .map(|p| py_round((rate * 4096) as f64 / 44100.0 * p))
                        .collect::<Vec<_>>(),
                ),
            ),
            ("valid_frames".into(), Json::Int(profile.frames)),
            ("padding_samples".into(), Json::Int(profile.padding_samples)),
        ]);
        records.push(Json::Obj(record));
        bank.extend_from_slice(&payload);
    }
    require(
        spu_base + bank.len() as i64 <= SPU_END,
        &format!(
            "complete Runner bank {} exceeds available tail {}; no samples trimmed",
            bank.len(),
            SPU_END - spu_base
        ),
    )?;
    let descriptor = jobj(vec![
        ("spu_base", Json::Int(spu_base)),
        ("spu_end", Json::Int(spu_base + bank.len() as i64)),
        (
            "spu_free_bytes",
            Json::Int(SPU_END - spu_base - bank.len() as i64),
        ),
        ("byte_len", Json::Int(bank.len() as i64)),
        ("checksum", Json::Int(fnv(&bank) as i64)),
        ("sha256", Json::Str(sha(&bank))),
        ("clips", Json::List(records)),
        (
            "proposed_voices",
            jobj(vec![
                ("movement_loops", ints(&[21, 22])),
                ("creature_calls", ints(&[23])),
            ]),
        ),
    ]);
    validate_bank(&bank, &descriptor)?;
    Ok((bank, descriptor))
}

fn floats_eq(j: &Json, key: &str, want: &[f64]) -> bool {
    jl(j, key).is_ok_and(|l| {
        l.len() == want.len()
            && l.iter().zip(want).all(|(a, b)| {
                matches!(a, Json::Float(f) if f == b)
                    || matches!(a, Json::Int(i) if *i as f64 == *b)
            })
    })
}

/// `validate_bank`: the layout, checksums and flags of a cooked bank against its descriptor.
pub fn validate_bank(bank: &[u8], descriptor: &Json) -> Result<()> {
    let end = ji(descriptor, "spu_end")?;
    require(
        ji(descriptor, "spu_base")? == SPU_BASE
            && end == SPU_BASE + bank.len() as i64
            && end <= SPU_END
            && ji(descriptor, "spu_free_bytes")? == SPU_END - end,
        "Runner bank SPU layout mismatch",
    )?;
    require(
        bank.len() as i64 == ji(descriptor, "byte_len")?
            && fnv(bank) as i64 == ji(descriptor, "checksum")?
            && sha(bank) == jstr(descriptor, "sha256")?,
        "Runner bank checksum mismatch",
    )?;
    let clips = jl(descriptor, "clips")?;
    require(
        clips.len() == SOURCES.len(),
        "Runner bank clip count mismatch",
    )?;
    let mut offset = 0i64;
    for (record, (event, sid, rate, looped)) in clips.iter().zip(SOURCES) {
        require(
            jstr(record, "event")? == event
                && jstr(record, "source")? == sid
                && ji(record, "rate")? == rate
                && jb(record, "loop")? == looped,
            "Runner descriptor source/rate mismatch",
        )?;
        require(
            ji(record, "offset")? == offset
                && ji(record, "spu_address")? == SPU_BASE + offset
                && offset % 16 == 0,
            "Runner clip layout mismatch",
        )?;
        let multipliers: [f64; 2] = if looped {
            [INITIAL_PITCH; 2]
        } else {
            CHASE_BOUNDS
        };
        require(
            jf(record, "source_volume")? == 1.0
                && ji(record, "gain")? == GAIN
                && floats_eq(record, "pitch_multipliers", &multipliers),
            "Runner descriptor source gain/pitch changed",
        )?;
        let bounds: Vec<i64> = multipliers
            .iter()
            .map(|p| py_round((rate * 4096) as f64 / 44100.0 * p))
            .collect();
        require(
            ji(record, "pitch")? == py_round(div(rate * 4096, 44100))
                && jl(record, "pitch_bounds")?
                    .iter()
                    .map(|b| if let Json::Int(i) = b { *i } else { -1 })
                    .collect::<Vec<_>>()
                    == bounds,
            "Runner descriptor pitch mismatch",
        )?;
        let byte_len = ji(record, "byte_len")?;
        let data = bank
            .get(offset as usize..((offset + byte_len) as usize).min(bank.len()))
            .unwrap_or(&[]);
        require(
            data.len() as i64 == byte_len
                && fnv(data) as i64 == ji(record, "checksum")?
                && sha(data) == jstr(record, "sha256")?,
            "Runner clip checksum mismatch",
        )?;
        validate_blocks(data)?;
        if looped {
            validate_loop(data)?;
        } else {
            require(
                data[data.len() - 16..] == [vec![12, 1], vec![0; 14]].concat()[..]
                    && data[..data.len() - 16].chunks_exact(16).all(|b| b[1] == 0),
                "Runner terminal ADPCM flags mismatch",
            )?;
        }
        let frames = ji(record, "valid_frames")?;
        let expected = (frames + 27) / 28 * 16 + if looped { 0 } else { 16 };
        require(
            frames > 0
                && data.len() as i64 == expected
                && ji(record, "padding_samples")? == (-frames).rem_euclid(28),
            "Runner valid sample layout mismatch",
        )?;
        offset += data.len() as i64;
    }
    require(offset == bank.len() as i64, "Runner bank trailing bytes")
}

/// `verify_outputs`: reject stale inputs/code and altered packed output without
/// rewriting evidence.
pub fn verify_outputs(root: &Path, report_path: &Path) -> Result<Json> {
    let report = parse(
        &std::fs::read_to_string(report_path)
            .map_err(|e| format!("{}: {e}", report_path.display()))?,
    )?;
    if let Json::Obj(identity) = jv(&report, "identity")? {
        for (path, expected) in identity {
            let p = root.join(path);
            require(
                p.is_file() && Json::Str(sha_file(&p)?) == *expected,
                &format!("stale Runner input/code: {path}"),
            )?;
        }
    }
    let payload = root.join(jstr(&report, "path")?);
    let parent = report_path
        .parent()
        .unwrap()
        .canonicalize()
        .map_err(|e| e.to_string())?;
    require(
        payload
            .canonicalize()
            .map_err(|e| e.to_string())?
            .starts_with(parent),
        "Runner output path escapes report directory",
    )?;
    let bank = std::fs::read(&payload).map_err(|e| e.to_string())?;
    validate_bank(&bank, jv(&report, "bank")?)?;
    Ok(report)
}

pub fn cook(root: &Path, source: &Source, out: &Path, verify: bool) -> Result<()> {
    let hk = root.join(".hkpsx");
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    require(
        out.canonicalize()
            .map_err(|e| e.to_string())?
            .starts_with(hk.canonicalize().map_err(|e| e.to_string())?),
        "Runner output must remain in ignored storage",
    )?;
    let report_path = out.join("report.json");
    if verify {
        let report = verify_outputs(root, &report_path)?;
        println!(
            "Verified complete Runner bank: {} bytes",
            ji(jv(&report, "bank")?, "byte_len")?
        );
        return Ok(());
    }
    let contract = source_contract(source)?;
    let focus_path = root.join(".hkpsx/focus-audio.json");
    let focus = parse(
        &std::fs::read_to_string(&focus_path)
            .map_err(|e| format!("{}: {e}", focus_path.display()))?,
    )?;
    // The Focus bank (with the ability sounds above it) ends at or below this
    // bank's base; the Focus cook records the free bytes between them.
    let focus_ok = ji(&focus, "spu_base")? + ji(&focus, "byte_len")? == ji(&focus, "spu_end")?
        && ji(&focus, "spu_end")? <= SPU_BASE
        && sha_file(&root.join(jstr(&focus, "path")?))? == jstr(&focus, "sha256")?
        && sha_file(&root.join("data/focus-audio.rs"))? == jstr(&focus, "manifest_sha256")?;
    require(focus_ok, "Focus bank/tail integrity mismatch")?;
    let dir = &source.directory;
    let mut paths: Vec<PathBuf> = vec![
        dir.join("globalgamemanagers"),
        dir.join("level37"),
        focus_path.clone(),
        root.join(jstr(&focus, "path")?),
        root.join("data/focus-audio.rs"),
    ];
    paths.extend(ASSEMBLIES.iter().map(|a| dir.join("Managed").join(a.0)));
    for spec in jl(&contract, "clips")? {
        let sid = jstr(spec, "source")?;
        let (file, pid) = sid.split_once(':').unwrap();
        let obj = u(source.object(
            &u(source.file(file))?,
            pid.parse::<i64>().map_err(|e| e.to_string())?,
        ))?;
        paths.push(dir.join(file));
        paths.push(
            dir.join(
                get(get(&u(source.read(&obj))?, "m_Resource")?, "m_Source")?
                    .str()
                    .unwrap_or_default(),
            ),
        );
    }
    for name in [
        "runner_audio.rs",
        "runner.rs",
        "music.rs",
        "spu.rs",
        "focus_audio.rs",
    ] {
        paths.push(root.join("host/hk-cook/src").join(name));
    }
    paths.sort();
    paths.dedup();
    let mut identity = Vec::new();
    for p in &paths {
        identity.push((rel(root, p), Json::Str(sha_file(p)?)));
    }
    let tool = Tool::build(root, "hk-runner-audio")?;
    let mut profiles = Vec::new();
    for spec in jl(&contract, "clips")? {
        let sid = jstr(spec, "source")?;
        let (file, pid) = sid.split_once(':').unwrap();
        let obj = u(source.object(
            &u(source.file(file))?,
            pid.parse::<i64>().map_err(|e| e.to_string())?,
        ))?;
        profiles.push(cook_clip(
            root,
            &tool,
            source,
            &obj,
            out,
            ji(spec, "rate")?,
            1,
            Resampler::Ffmpeg,
        )?);
    }
    let (bank, descriptor) = assemble(&profiles, root, &contract, SPU_BASE)?;
    for (path, value) in &identity {
        require(
            Json::Str(sha_file(&root.join(path))?) == *value,
            "Runner inputs/code changed during conversion",
        )?;
    }
    let payload_path = out.join("runner-audio.adpcm");
    std::fs::write(&payload_path, &bank).map_err(|e| e.to_string())?;
    dump(
        &report_path,
        &jobj(vec![
            ("path", Json::Str(rel(root, &payload_path))),
            ("identity", Json::Obj(identity)),
            ("contract", contract),
            ("profiles", Json::List(profiles.into_iter().map(|p| p.json).collect())),
            ("bank", descriptor.clone()),
            (
                "limitations",
                Json::List(
                    [
                        "Full clips retained; category downsampling and mono fold-down reduce fidelity.",
                        "Loop padding is preserved; source AudioSource pitch can persist across chase and walk callbacks.",
                        "SPU voice assignment, gain mixing and playback scheduling require guest integration and validation.",
                    ]
                    .iter()
                    .map(|s| js(s))
                    .collect(),
                ),
            ),
        ]),
    )?;
    verify_outputs(root, &report_path)?;
    // Guest bank: resident ADPCM above the focus bank plus its clip table.
    std::fs::write(root.join("data/runner-audio.adpcm"), &bank).map_err(|e| e.to_string())?;
    let mut rows = String::new();
    for c in jl(&descriptor, "clips")? {
        let bounds: Vec<i64> = jl(c, "pitch_bounds")?
            .iter()
            .map(|b| if let Json::Int(i) = b { *i } else { 0 })
            .collect();
        rows += &format!(
            "    ({},{},{},{},{},{}), // {}\n",
            ji(c, "spu_address")?,
            ji(c, "byte_len")?,
            ji(c, "pitch")?,
            bounds[0],
            bounds[1],
            jb(c, "loop")?,
            jstr(c, "event")?
        );
    }
    // The guest streams this bank from the disc, so it carries the pack chunk's
    // expected length and FNV-1a checksum, not the bytes themselves.
    let manifest = format!(
        "// Generated from complete Windows source clips; see ignored Runner audio provenance.\npub const BANK_BASE: u32 = {SPU_BASE};\npub const BANK_BYTES: usize = {};\npub const BANK_CHECKSUM: u32 = {};\n/// (SPU address, bytes, nominal pitch, min pitch, max pitch, loops): walk loop, chase 1, chase 2.\npub const CLIPS: [(u32,usize,u16,u16,u16,bool);{}] = [\n{rows}];\n",
        bank.len(),
        fnv(&bank),
        jl(&descriptor, "clips")?.len()
    );
    std::fs::write(root.join("data/runner-audio.rs"), manifest).map_err(|e| e.to_string())?;
    println!(
        "Runner audio: {} bytes; SPU end {} remaining {}",
        bank.len(),
        ji(&descriptor, "spu_end")?,
        ji(&descriptor, "spu_free_bytes")?
    );
    Ok(())
}

pub fn main(root: &Path, args: &[String], source_dir: Option<&Path>) -> Result<()> {
    let source = match source_dir {
        Some(d) => u(Source::new(d))?,
        None => u(Source::from_doctor(root))?,
    };
    let out = args
        .iter()
        .position(|a| a == "--out")
        .and_then(|i| args.get(i + 1))
        .map(|p| root.join(p))
        .unwrap_or_else(|| root.join(".hkpsx/runner71/audio"));
    cook(root, &source, &out, args.iter().any(|a| a == "--verify"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music::Plane;

    struct Fixture {
        root: PathBuf,
        profiles: Vec<Profile>,
        contract: Json,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    fn identity(i: usize) -> Json {
        jobj(vec![
            ("clip_metadata_sha256", Json::Str(format!("metadata{i}"))),
            ("encoded_resource_sha256", Json::Str(format!("source{i}"))),
        ])
    }
    fn fixture(tag: &str) -> Fixture {
        let root = std::env::temp_dir().join(format!("hk-runner-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(root.join(".hkpsx")).unwrap();
        let (mut profiles, mut clips) = (Vec::new(), Vec::new());
        for (i, (event, sid, rate, looped)) in SOURCES.iter().enumerate() {
            let raw: Vec<u8> = [
                vec![12, 0],
                vec![0x21 + i as u8; 14],
                vec![28, 0],
                vec![0x12 + i as u8; 14],
            ]
            .concat();
            let path = root.join(format!(".hkpsx/{i}.adpcm"));
            std::fs::write(&path, &raw).unwrap();
            clips.push(jobj(vec![
                ("event", js(event)),
                ("source", js(sid)),
                ("rate", Json::Int(*rate)),
                ("loop", Json::Bool(*looped)),
                ("name", Json::Str(format!("fixture{i}"))),
                ("source_identity", identity(i)),
                ("source_length_seconds", Json::Float(53.0 / *rate as f64)),
                ("source_volume", Json::Float(1.0)),
                ("gain", Json::Int(GAIN)),
                (
                    "pitch_multipliers",
                    if *looped {
                        floats(&[INITIAL_PITCH; 2])
                    } else {
                        floats(&CHASE_BOUNDS)
                    },
                ),
            ]));
            profiles.push(Profile {
                source: sid.to_string(),
                name: format!("fixture{i}"),
                rate: *rate,
                channels: 1,
                frames: 53,
                spu_pitch: py_round((rate * 4096) as f64 / 44100.0),
                padding_samples: 3,
                source_pcm_frames: 53,
                source_pcm_rate: *rate,
                bytes: raw.len() as i64,
                source_identity: identity(i),
                planes: vec![Plane {
                    path: PathBuf::from(format!(".hkpsx/{i}.adpcm")),
                    bytes: raw.len() as i64,
                    sha256: sha(&raw),
                }],
                json: Json::Null,
            });
        }
        Fixture {
            root,
            profiles,
            contract: jobj(vec![("clips", Json::List(clips))]),
        }
    }
    impl Fixture {
        fn assemble(&self) -> Result<(Vec<u8>, Json)> {
            assemble(&self.profiles, &self.root, &self.contract, SPU_BASE)
        }
    }
    fn clone_profile(p: &Profile) -> Profile {
        Profile {
            source: p.source.clone(),
            name: p.name.clone(),
            rate: p.rate,
            channels: p.channels,
            frames: p.frames,
            spu_pitch: p.spu_pitch,
            padding_samples: p.padding_samples,
            source_pcm_frames: p.source_pcm_frames,
            source_pcm_rate: p.source_pcm_rate,
            bytes: p.bytes,
            source_identity: p.source_identity.clone(),
            planes: p
                .planes
                .iter()
                .map(|x| Plane {
                    path: x.path.clone(),
                    bytes: x.bytes,
                    sha256: x.sha256.clone(),
                })
                .collect(),
            json: Json::Null,
        }
    }
    fn set(j: &mut Json, key: &str, v: Json) {
        if let Json::Obj(f) = j {
            f.iter_mut().find(|k| k.0 == key).unwrap().1 = v;
        }
    }
    fn clip_mut(d: &mut Json, i: usize) -> &mut Json {
        let Json::Obj(f) = d else { panic!() };
        let Json::List(l) = &mut f.iter_mut().find(|k| k.0 == "clips").unwrap().1 else {
            panic!()
        };
        &mut l[i]
    }

    #[test]
    fn full_duration_flags_offsets_and_checksum() {
        let f = fixture("full");
        let raw: Vec<Vec<u8>> = f
            .profiles
            .iter()
            .map(|p| std::fs::read(f.root.join(&p.planes[0].path)).unwrap())
            .collect();
        let (bank, desc) = f.assemble().unwrap();
        assert_eq!(bank.len(), 128);
        let clips = jl(&desc, "clips").unwrap();
        assert_eq!(
            clips
                .iter()
                .map(|c| ji(c, "offset").unwrap())
                .collect::<Vec<_>>(),
            [0, 32, 80]
        );
        assert_eq!(
            clips
                .iter()
                .map(|c| ji(c, "spu_address").unwrap())
                .collect::<Vec<_>>(),
            [SPU_BASE, SPU_BASE + 32, SPU_BASE + 80]
        );
        assert_eq!((bank[1], bank[17]), (4, 3));
        for (i, c) in clips.iter().enumerate() {
            let (o, n) = (
                ji(c, "offset").unwrap() as usize,
                ji(c, "byte_len").unwrap() as usize,
            );
            let data = &bank[o..o + n];
            assert_eq!(ji(c, "valid_frames").unwrap(), 53);
            assert_eq!(ji(c, "checksum").unwrap(), fnv(data) as i64);
            if i > 0 {
                assert_eq!(&data[..data.len() - 16], &raw[i][..]);
                assert_eq!(
                    data[data.len() - 16..],
                    [vec![12, 1], vec![0; 14]].concat()[..]
                );
            }
        }
        assert_eq!(bank[2..16], raw[0][2..16]);
        assert_eq!(bank[18..32], raw[0][18..32]);
    }

    #[test]
    fn wrong_rate_source_pitch_and_duration_rejected() {
        let f = fixture("wrong");
        let changes: Vec<Box<dyn Fn(&mut Profile)>> = vec![
            Box::new(|p| p.rate = 8000),
            Box::new(|p| p.source = SOURCES[0].1.to_string()),
            Box::new(|p| p.channels = 2),
            Box::new(|p| p.spu_pitch = 10),
            Box::new(|p| {
                p.frames = 28;
                p.padding_samples = 0
            }),
            Box::new(|p| p.source_identity = Json::Obj(vec![])),
        ];
        for (n, change) in changes.iter().enumerate() {
            let mut profiles: Vec<Profile> = f.profiles.iter().map(clone_profile).collect();
            change(&mut profiles[1]);
            assert!(
                assemble(&profiles, &f.root, &f.contract, SPU_BASE).is_err(),
                "change {n} was admitted"
            );
        }
        let reversed: Vec<Profile> = f.profiles.iter().rev().map(clone_profile).collect();
        assert!(assemble(&reversed, &f.root, &f.contract, SPU_BASE).is_err());
        let short: Vec<Profile> = f.profiles.iter().take(2).map(clone_profile).collect();
        assert!(assemble(&short, &f.root, &f.contract, SPU_BASE).is_err());
    }

    #[test]
    fn altered_input_bytes_flags_padding_and_path_rejected() {
        let mut f = fixture("altered");
        let path = f.root.join(&f.profiles[0].planes[0].path);
        let raw = std::fs::read(&path).unwrap();
        std::fs::write(&path, [&raw[..raw.len() - 1], &[0u8][..]].concat()).unwrap();
        assert!(f.assemble().unwrap_err().contains("integrity"));
        for replacement in [
            [vec![0x5c, 0], raw[2..].to_vec()].concat(),
            [vec![12, 1], raw[2..].to_vec()].concat(),
        ] {
            std::fs::write(&path, &replacement).unwrap();
            f.profiles[0].planes[0].sha256 = sha(&replacement);
            assert!(f.assemble().is_err());
        }
        std::fs::write(&path, &raw).unwrap();
        f.profiles[0].planes[0].sha256 = sha(&raw);
        f.profiles[0].padding_samples = 0;
        assert!(f.assemble().unwrap_err().contains("padding"));
        f.profiles[0].padding_samples = 3;
        let outside =
            std::env::temp_dir().join(format!("hk-runner-outside-{}.adpcm", std::process::id()));
        std::fs::write(&outside, &raw).unwrap();
        f.profiles[0].planes[0].path = outside.clone();
        assert!(f.assemble().unwrap_err().contains("ignored storage"));
        let _ = std::fs::remove_file(outside);
    }

    #[test]
    fn complete_bank_overflow_fails_without_trimming() {
        // One block more than the whole tail above the Focus bank, derived
        // rather than written down.
        let mut f = fixture("overflow");
        let blocks = ((SPU_END - SPU_BASE) / 16 + 1) as usize;
        let raw = [vec![12u8, 0], vec![0; 14]].concat().repeat(blocks);
        std::fs::write(f.root.join(&f.profiles[0].planes[0].path), &raw).unwrap();
        let p = &mut f.profiles[0];
        p.frames = blocks as i64 * 28;
        p.source_pcm_frames = blocks as i64 * 28;
        p.padding_samples = 0;
        p.bytes = raw.len() as i64;
        p.planes[0].bytes = raw.len() as i64;
        p.planes[0].sha256 = sha(&raw);
        set(
            clip_mut(&mut f.contract, 0),
            "source_length_seconds",
            Json::Float((blocks * 28) as f64 / SOURCES[0].2 as f64),
        );
        assert!(f.assemble().unwrap_err().contains("no samples trimmed"));
        assert!(assemble(&f.profiles, &f.root, &f.contract, SPU_BASE + 16)
            .unwrap_err()
            .contains("tail changed"));
    }

    #[test]
    fn output_corruption_and_layout_rejected() {
        let f = fixture("corrupt");
        let (bank, desc) = f.assemble().unwrap();
        let mut damaged = bank.clone();
        *damaged.last_mut().unwrap() = 1;
        assert!(validate_bank(&damaged, &desc)
            .unwrap_err()
            .contains("checksum"));
        for (field, value) in [
            ("offset", Json::Int(16)),
            ("spu_address", Json::Int(0)),
            ("rate", Json::Int(8000)),
            ("pitch", Json::Int(1)),
            ("valid_frames", Json::Int(80)),
            ("gain", Json::Int(1)),
            ("pitch_bounds", ints(&[1, 2])),
        ] {
            let mut broken = desc.clone();
            set(clip_mut(&mut broken, 1), field, value);
            assert!(validate_bank(&bank, &broken).is_err(), "{field}");
        }
        // Even if integrity stamps are recomputed, bad transport flags fail.
        let mut bad = bank.clone();
        bad[1] = 0;
        let mut broken = desc.clone();
        set(&mut broken, "checksum", Json::Int(fnv(&bad) as i64));
        set(&mut broken, "sha256", Json::Str(sha(&bad)));
        set(
            clip_mut(&mut broken, 0),
            "checksum",
            Json::Int(fnv(&bad[..32]) as i64),
        );
        set(
            clip_mut(&mut broken, 0),
            "sha256",
            Json::Str(sha(&bad[..32])),
        );
        assert!(validate_bank(&bad, &broken).unwrap_err().contains("loop"));
    }

    #[test]
    fn verify_rejects_stale_input_and_output_without_rewriting() {
        let f = fixture("verify");
        let (bank, desc) = f.assemble().unwrap();
        let payload = f.root.join(".hkpsx/bank.adpcm");
        std::fs::write(&payload, &bank).unwrap();
        let input = f.root.join("input");
        std::fs::write(&input, b"original").unwrap();
        let report = f.root.join(".hkpsx/report.json");
        let text = crate::pyjson::dumps(&jobj(vec![
            ("path", Json::Str(payload.display().to_string())),
            ("bank", desc),
            (
                "identity",
                Json::Obj(vec![(
                    input.display().to_string(),
                    Json::Str(sha(b"original")),
                )]),
            ),
        ]));
        std::fs::write(&report, &text).unwrap();
        verify_outputs(&f.root, &report).unwrap();
        std::fs::write(&input, b"stale").unwrap();
        assert!(verify_outputs(&f.root, &report)
            .unwrap_err()
            .contains("stale"));
        std::fs::write(&input, b"original").unwrap();
        let mut damaged = bank.clone();
        *damaged.last_mut().unwrap() = 1;
        std::fs::write(&payload, damaged).unwrap();
        assert!(verify_outputs(&f.root, &report)
            .unwrap_err()
            .contains("checksum"));
        assert_eq!(std::fs::read_to_string(&report).unwrap(), text);
    }
}
