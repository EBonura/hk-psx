//! Cook complete Focus sounds into one startup-loaded SPU bank.
//! Ported from host/focus_audio.py, whose output it reproduces byte for byte
//! (data/focus-audio.adpcm, data/focus-audio.rs and .hkpsx/focus-audio.json,
//! except the report's `identity.code` table, which names the Rust modules).
//!
//! Windows retail inputs are read-only. Both clips are cooked at half the rate
//! they shipped at (charge 8 kHz to 4 kHz, heal 22.05 kHz to 11.025 kHz)
//! through the SDK's shared resampler: halve, never trim. The bytes that frees
//! inside the Focus range carry the Knight's Crystal Heart and Vengeful Spirit
//! sounds (ABILITY_SAMPLES), so nothing above or below this bank moves.

use crate::common::{err, get, py_round, Result};
use crate::cook_audio::{jobj, js, sha, u};
use crate::fmod::clip_wav;
use crate::music::{cook_clip, dump, ffmpeg_version, jget, rel, sha_file, value_json, Resampler};
use crate::pyjson::{dumps_sorted_compact, from_serde, Json};
use crate::spu::{fnv, loop_payload, oneshot_payload, read_wav, Tool};
use hk_unity::playmaker::{action_fields, field, Fields};
use hk_unity::{Source, Value};
use std::path::Path;

/// Focus and Runner are the top of SPU RAM: Runner ends at 0x7FFF0, the last
/// 16 bytes being where psx_spu::init parks the (disabled) reverb work area,
/// and Focus sits directly below it. The world one-shots (cook_audio), the
/// music ring and ambience's ceiling stack downward from here, so this base is
/// what moves when either bank grows; host/ambience.py refuses an overlap.
pub const SPU_BASE: i64 = 0x60950;
const SPU_END: i64 = 0x80000;
/// The Runner bank's fixed base (host/runner_audio.py): this bank, ability
/// sounds included, must end at or below it.
const RUNNER_BASE: i64 = 0x7B000;
const CHARGE_RATE: i64 = 3200;
const HEAL_RATE: i64 = 11025;
/// The Knight's sounds that ride the Focus range (source: the Superdash FSM on
/// the Knight and Fireball Top's Fireball Cast FSM, resources.assets). Order is
/// the guest's ABILITY index. Rates come from the SDK's rate allocator.
const ABILITY: [(&str, i64, &str); 7] = [
    ("super_charge", 1289, "hero_super_dash_charge"),
    ("super_ready", 1214, "hero_super_dash_ready"),
    ("super_burst", 1321, "hero_super_dash_burst"),
    ("super_wall", 1314, "hero_super_dash_impact_wall"),
    ("super_brake", 1351, "hero_super_dash_air_brake"),
    ("fireball", 1361, "hero_fireball"),
    ("focus_ready", 1271, "focus_ready"),
];
/// The port's categories: a one-shot under a second ships at 22,050 Hz, a longer
/// clip at 11,025 Hz; the allocator may halve a clip once and never further.
const ABILITY_LADDER: [i64; 3] = [22050, 11025, 5512];
const GAIN: i64 = 5461;
/// These assemblies were inspected for AudioPlay, FadeAudio.OnExit and pooled
/// PlayAudioAndRecycle semantics. A changed executable requires a fresh audit.
const ASSEMBLIES: [(&str, &str); 2] = [
    ("Managed/Assembly-CSharp.dll", "e9048ef6a633970f735e01ec166d3959f610eaea7a88d827d48d67b1e5fb87bd"),
    ("Managed/PlayMaker.dll", "0ef0e7829d125e1f632c8a189260ec6c6882630be6932c8c7ae032efbc53469a"),
];

fn require(ok: bool, message: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        err(message)
    }
}

fn ability_ladder(frames: i64, source_rate: i64) -> Vec<i64> {
    let top = if frames < source_rate { 22050 } else { 11025 };
    ABILITY_LADDER.iter().copied().filter(|&r| r <= top).take(2).collect()
}

/// focus_audio.py `fields`: the compact scalar fields of one action, plus the
/// game-object, object and unity-object parameters it names.
pub fn fields(data: &Value, index: usize) -> Result<Fields> {
    let mut result = u(action_fields(data, index, false))?;
    let names = get(data, "actionNames")?.list().map_or(0, <[Value]>::len);
    let starts: Vec<i64> = get(data, "actionStartIndex")?.list().unwrap_or(&[]).iter().map(|x| x.int().unwrap_or(0)).collect();
    let param_names = get(data, "paramName")?.list().unwrap_or(&[]);
    let start = starts[index] as usize;
    let end = if index + 1 < names { starts[index + 1] as usize } else { param_names.len() };
    for i in start..end {
        let kind = get(data, "paramDataType")?.list().and_then(|l| l.get(i)).and_then(Value::int).unwrap_or(0);
        let key = match kind {
            19 => "fsmGameObjectParams",
            24 => "fsmObjectParams",
            11 => "unityObjectParams",
            _ => continue,
        };
        let pos = get(data, "paramDataPos")?.list().and_then(|l| l.get(i)).and_then(Value::int).unwrap_or(0);
        let list = get(data, key)?.list().ok_or_else(|| format!("{key} is not a list"))?;
        let at = if pos < 0 { list.len() as i64 + pos } else { pos };
        let value = list.get(at as usize).ok_or_else(|| format!("{key} index out of range"))?.clone();
        let name = param_names.get(i).and_then(Value::str).filter(|s| !s.is_empty()).unwrap_or_else(|| i.to_string());
        match result.iter_mut().find(|(k, _)| *k == name) {
            Some(slot) => slot.1 = value,
            None => result.push((name, value)),
        }
    }
    Ok(result)
}

fn literal(v: &Value) -> Result<&Value> {
    require(!get(v, "useVariable")?.truthy(), "variable-valued Focus audio parameter")?;
    get(v, "value")
}
fn is_pptr(v: &Value, file: i64, path: i64) -> bool {
    v.py_eq(&Value::Map(vec![("m_FileID".into(), Value::Int(file)), ("m_PathID".into(), Value::Int(path))]))
}
fn is_num(v: &Value, x: f64) -> bool {
    v.float() == Some(x)
}
fn fget<'a>(f: &'a Fields, key: &str) -> Result<&'a Value> {
    field(f, key).ok_or_else(|| format!("missing field {key}"))
}

fn charge_target(a: &Fields) -> bool {
    let Some(target) = field(a, "gameObject") else { return false };
    let object = target.get("gameObject");
    target.get("ownerOption").is_some_and(|o| is_num(o, 1.0))
        && object.and_then(|o| o.get("useVariable")).is_some_and(Value::truthy)
        && object.and_then(|o| o.get("name")).and_then(Value::str).as_deref() == Some("Charge Audio")
}

/// One enabled-or-not action of a state, as `source_contract` decodes it.
pub struct Act {
    action: String,
    enabled: Value,
    fields: Fields,
}
type States = Vec<(String, Vec<Act>)>;

/// A dict built from pairs: a repeated key keeps its first position and its last value.
fn dict<V>(pairs: Vec<(String, V)>) -> Vec<(String, V)> {
    let mut out: Vec<(String, V)> = Vec::new();
    for (k, v) in pairs {
        match out.iter_mut().find(|(n, _)| *n == k) {
            Some(slot) => slot.1 = v,
            None => out.push((k, v)),
        }
    }
    out
}
fn lookup<'a, V>(d: &'a [(String, V)], key: &str) -> Option<&'a V> {
    d.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

/// Validate decoded source actions, preserving start/repeat/fade/stop intent;
/// returns the fade length in 60 Hz ticks.
fn validate_contract(states: &States, globals: &[(String, String)]) -> Result<i64> {
    let actions = |state: &str, kind: &str| -> Result<Vec<&Fields>> {
        let acts = lookup(states, state).ok_or_else(|| format!("missing Focus state {state}"))?;
        Ok(acts.iter().filter(|a| a.enabled.truthy() && a.action.rsplit('.').next() == Some(kind)).map(|a| &a.fields).collect())
    };
    let starts = actions("Focus Start", "AudioPlay")?;
    require(starts.len() == 1 && charge_target(starts[0]), "Focus charge start changed")?;
    require(is_num(literal(fget(starts[0], "volume")?)?, 1.0) && is_pptr(literal(fget(starts[0], "oneShotClip")?)?, 0, 0), "Focus charge is no longer normal Play at unit gain")?;
    let heals = actions("Focus Heal", "AudioPlayerOneShotSingle")?;
    require(heals.len() == 1, "Focus heal action changed")?;
    let heal = heals[0];
    for key in ["volume", "pitchMin", "pitchMax"] {
        require(is_num(literal(fget(heal, key)?)?, 1.0), "Focus heal gain or pitch changed")?;
    }
    require(is_num(literal(fget(heal, "delay")?)?, 0.0), "Focus heal delay changed")?;
    require(is_pptr(literal(fget(heal, "audioClip")?)?, 0, 1260) && is_pptr(literal(fget(heal, "audioPlayer")?)?, 0, 4126), "Focus heal clip/prefab changed")?;
    let mut fades = Vec::new();
    for state in ["Focus Cancel", "Focus Get Finish"] {
        let found = actions(state, "FadeAudio")?;
        require(found.len() == 1 && charge_target(found[0]), "Focus fade target changed")?;
        let fade = found[0];
        require(is_num(literal(fget(fade, "startVolume")?)?, 1.0) && is_num(literal(fget(fade, "endVolume")?)?, 0.0), "Focus fade range changed")?;
        let seconds = literal(fget(fade, "time")?)?.float().ok_or("Focus fade time is not a number")?;
        require((seconds - 0.33).abs() < 1e-6, "Focus fade duration changed")?;
        fades.push(seconds);
    }
    for state in ["Regain Control", "Cancel Some", "FSM Cancel", "Cancel All"] {
        require(actions(state, "AudioStop")?.into_iter().any(charge_target), &format!("Focus stop missing from {state}"))?;
    }
    for state in ["Focus", "Full HP?", "Focus Heal"] {
        for kind in ["AudioPlay", "AudioStop", "FadeAudio"] {
            require(!actions(state, kind)?.into_iter().any(charge_target), "Focus repeat now changes charging voice")?;
        }
    }
    for (event, state) in [("LEAVING SCENE", "Cancel Some"), ("FSM CANCEL", "FSM Cancel"), ("HERO DAMAGED", "Reset Cam Zoom")] {
        require(lookup(globals, event).map(String::as_str) == Some(state), "Focus interruption routing changed")?;
    }
    Ok(py_round(fades[0] * 60.0))
}

fn pptr_is(tree: &Value, key: &str, file: i64, path: i64) -> Result<bool> {
    Ok(is_pptr(get(tree, key)?, file, path))
}

fn source_contract(source: &Source) -> Result<(Json, i64)> {
    let file = u(source.file("resources.assets"))?;
    let object = |id: i64| u(source.object(&file, id));
    let obj = object(21207)?;
    require(u(source.typename(&obj))? == "PlayMakerFSM", "Focus FSM type changed")?;
    let tree = u(source.read(&obj))?;
    let fsm = get(&tree, "fsm")?;
    require(get(fsm, "name")?.str().as_deref() == Some("Spell Control"), "Focus FSM identity changed")?;
    let mut states: States = Vec::new();
    for s in get(fsm, "states")?.list().ok_or("states is not a list")? {
        let data = get(s, "actionData")?;
        let names = get(data, "actionNames")?.list().ok_or("actionNames is not a list")?;
        let enabled = get(data, "actionEnabled")?.list().ok_or("actionEnabled is not a list")?;
        let mut acts = Vec::new();
        for (i, name) in names.iter().enumerate() {
            acts.push(Act { action: name.str().unwrap_or_default(), enabled: enabled.get(i).cloned().ok_or("actionEnabled index out of range")?, fields: fields(data, i)? });
        }
        states.push((get(s, "name")?.str().unwrap_or_default(), acts));
    }
    let states = dict(states);
    let globals = dict(
        get(fsm, "globalTransitions")?
            .list()
            .unwrap_or(&[])
            .iter()
            .map(|t| Ok((get(get(t, "fsmEvent")?, "name")?.str().unwrap_or_default(), get(t, "toState")?.str().unwrap_or_default())))
            .collect::<Result<Vec<_>>>()?,
    );
    let fade_ticks = validate_contract(&states, &globals)?;
    let mut transitions: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for s in get(fsm, "states")?.list().unwrap_or(&[]) {
        let t = get(s, "transitions")?.list().unwrap_or(&[]).iter().map(|t| Ok((get(get(t, "fsmEvent")?, "name")?.str().unwrap_or_default(), get(t, "toState")?.str().unwrap_or_default()))).collect::<Result<Vec<_>>>()?;
        transitions.push((get(s, "name")?.str().unwrap_or_default(), dict(t)));
    }
    let transitions = dict(transitions);
    for (state, event, target) in [("Reset Cam Zoom", "FINISHED", "Cancel All"), ("Focus Heal", "WAIT", "Full HP?"), ("Full HP?", "FINISHED", "Focus")] {
        let t = lookup(&transitions, state).ok_or_else(|| format!("missing Focus state {state}"))?;
        require(lookup(t, event).map(String::as_str) == Some(target), "Focus continuation changed")?;
    }
    let init = lookup(&states, "Init").ok_or("missing Focus state Init")?;
    let mut bound = false;
    for a in init.iter().filter(|a| a.enabled.truthy() && a.action.ends_with(".FindChild")) {
        let f = &a.fields;
        bound |= literal(fget(f, "childName")?)?.str().as_deref() == Some("Charge Audio")
            && get(fget(f, "storeResult")?, "name")?.str().as_deref() == Some("Charge Audio")
            && get(get(fget(f, "gameObject")?, "gameObject")?, "name")?.str().as_deref() == Some("Focus Effects");
    }
    require(bound, "Charge Audio child binding changed")?;
    let charge = u(source.read(&object(13927)?))?;
    let heal = u(source.read(&object(13920)?))?;
    let recycle = u(source.read(&object(25641)?))?;
    require(pptr_is(&charge, "m_GameObject", 0, 5518)? && pptr_is(&charge, "m_Resource", 0, 1160)?, "charge AudioSource binding changed")?;
    require(pptr_is(&heal, "m_GameObject", 0, 4126)? && pptr_is(&recycle, "audioSource", 0, 13920)?, "heal pool binding changed")?;
    for (audio, looped) in [(&charge, true), (&heal, false)] {
        require(get(audio, "Loop")?.truthy() == looped && is_num(get(audio, "m_Pitch")?, 1.0) && is_num(get(audio, "m_Volume")?, 1.0) && !get(audio, "Mute")?.truthy(), "Focus AudioSource settings changed")?;
        require(pptr_is(audio, "OutputAudioMixerGroup", 0, 3829)?, "Focus Actors mixer routing changed")?;
    }
    let act_json = |a: &Act| {
        jobj(vec![
            ("action", Json::Str(a.action.clone())),
            ("enabled", value_json(&a.enabled)),
            ("fields", Json::Obj(a.fields.iter().map(|(k, v)| (k.clone(), value_json(v))).collect())),
        ])
    };
    let mut selected = Vec::new();
    for k in ["Focus Start", "Focus Heal", "Focus Cancel", "Focus Get Finish", "Regain Control", "Cancel Some", "FSM Cancel", "Cancel All"] {
        let acts = lookup(&states, k).ok_or_else(|| format!("missing Focus state {k}"))?;
        selected.push((k.to_string(), Json::List(acts.iter().map(act_json).collect())));
    }
    let contract = jobj(vec![
        ("fsm", Json::Str(obj.sid())),
        ("fade_ticks", Json::Int(fade_ticks)),
        ("source_fade_seconds", Json::Float(0.33000001311302185)),
        ("charge_audio_source", js("resources.assets:13927")),
        ("heal_audio_source", js("resources.assets:13920")),
        ("charge_clip", js("resources.assets:1160")),
        ("heal_clip", js("resources.assets:1260")),
        ("states", Json::Obj(selected)),
        ("global_transitions", Json::Obj(globals.iter().map(|(k, v)| (k.clone(), Json::Str(v.clone()))).collect())),
        ("charge_source", value_json(&charge)),
        ("heal_source", value_json(&heal)),
    ]);
    Ok((contract, fade_ticks))
}

/// focus_audio.py `assemble`: the charging loop and the heal one-shot back to
/// back; returns the bank and the loop's length.
fn assemble(charge: &[u8], heal: &[u8], ambience_end: i64) -> Result<(Vec<u8>, usize)> {
    require((0..=SPU_BASE).contains(&ambience_end) && SPU_BASE % 16 == 0, "Focus bank overlaps ambience")?;
    let (charge, heal) = (loop_payload(charge)?, oneshot_payload(heal)?);
    require(SPU_BASE + (charge.len() + heal.len()) as i64 <= SPU_END, "Focus bank exceeds SPU RAM")?;
    let n = charge.len();
    Ok(([charge, heal].concat(), n))
}

struct Ability {
    event: &'static str,
    name: &'static str,
    rate: i64,
    samples: i64,
    seconds: f64,
    source_rate: i64,
    source_frames: i64,
    payload: Vec<u8>,
}

/// The ABILITY one-shots, each whole, at the rates the SDK allocator picks to
/// fit `budget` bytes with the least band loss.
fn ability_sounds(root: &Path, tool: &Tool, source: &Source, budget: i64) -> Result<Vec<Ability>> {
    let file = u(source.file("resources.assets"))?;
    let mut wavs = Vec::new();
    let mut request = vec![format!("budget\t{budget}")];
    for (i, (event, pid, name)) in ABILITY.iter().enumerate() {
        let clip = u(source.object(&file, *pid))?;
        let tree = u(source.read(&clip))?;
        require(tree.get("m_Name").and_then(Value::str).as_deref() == Some(*name), &format!("ability clip identity changed: {name}"))?;
        let data = clip_wav(root, source, &tree)?;
        let path = tool.scratch.join(format!("{event}.wav"));
        std::fs::write(&path, &data).map_err(|e| e.to_string())?;
        let w = read_wav(&data)?;
        let (frames, src) = ((w.data.len() / (2 * w.channels as usize)) as i64, w.rate as i64);
        let ladder = ability_ladder(frames, src);
        let sizes: Vec<i64> = ladder.iter().map(|&r| (py_round(frames as f64 * r as f64 / src as f64) + 27) / 28 * 16 + 16).collect();
        let join = |v: &[i64]| v.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
        request.push(format!("1\t{}\t{}\t{}\t{}", ladder.len() - 1, path.display(), join(&ladder), join(&sizes)));
        wavs.push((i, *event, *name, data, frames, src, ladder));
    }
    let answer = tool.plan(&(request.join("\n") + "\n"))?;
    let lines: Vec<&str> = answer.lines().collect();
    require(!lines.is_empty() && lines[0].trim() != "none", "ability sounds do not fit the Focus range at any allowed rate")?;
    let steps: Vec<usize> = lines.iter().filter(|l| !l.trim().is_empty()).map(|l| l.split('\t').next().unwrap().parse::<usize>().map_err(|e| e.to_string())).collect::<Result<_>>()?;
    require(steps.len() == wavs.len(), "zip() argument 2 is shorter than argument 1")?;
    let mut out = Vec::new();
    for ((_, event, name, data, frames, src, ladder), step) in wavs.into_iter().zip(steps) {
        let rate = ladder[step];
        let pcm = tool.resample(&data, rate)?;
        let payload = oneshot_payload(&tool.encode(&pcm, "restart")?)?;
        out.push(Ability { event, name, rate, samples: pcm.len() as i64, seconds: pcm.len() as f64 / rate as f64, source_rate: src, source_frames: frames, payload });
    }
    Ok(out)
}

pub fn cook(root: &Path, source: &Source) -> Result<()> {
    let out = root.join(".hkpsx/focus-audio");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let (contract, fade_ticks) = source_contract(source)?;
    let mut inputs = Vec::new();
    for name in ["globalgamemanagers", "resources.assets", "resources.resource"].into_iter().chain(ASSEMBLIES.iter().map(|a| a.0)) {
        inputs.push((name.to_string(), Json::Str(sha_file(&source.directory.join(name))?)));
    }
    for (name, digest) in ASSEMBLIES {
        require(jget(&Json::Obj(inputs.clone()), name) == Some(&Json::Str(digest.into())), "Focus action implementation changed; re-audit assembly semantics")?;
    }
    // The bank does not depend on ambience, which is cooked after it and
    // places itself below SPU_BASE (host/ambience.py refuses an overlap). A
    // previous ambience cook is still checked against, when there is one.
    let ambience_path = root.join(".hkpsx/ambience.json");
    let ambience_end = if ambience_path.exists() {
        let j: serde_json::Value = serde_json::from_slice(&std::fs::read(&ambience_path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        j["spu_end"].as_i64().ok_or("ambience.json without spu_end")?
    } else {
        SPU_BASE
    };
    require(ambience_end <= SPU_BASE, "Focus SPU reservation overlaps ambience")?;
    let code = Json::Obj(vec![
        ("focus_audio.rs".into(), Json::Str(sha(include_bytes!("focus_audio.rs")))),
        ("music.rs".into(), Json::Str(sha(include_bytes!("music.rs")))),
        ("spu.rs".into(), Json::Str(sha(include_bytes!("spu.rs")))),
    ]);
    let identity = jobj(vec![("inputs", Json::Obj(inputs)), ("code", code), ("conversion", Json::Str(ffmpeg_version()?))]);
    let report_path = root.join(".hkpsx/focus-audio.json");
    let payload_path = root.join("data/focus-audio.adpcm");
    let manifest = root.join("data/focus-audio.rs");
    if report_path.exists() && payload_path.exists() && manifest.exists() {
        let old = std::fs::read_to_string(&report_path).map_err(|e| e.to_string())?;
        // The report is dumped from `identity` unchanged, so a cached run
        // carries the same structure for it.
        if let Ok(old) = serde_json::from_str::<serde_json::Value>(&old) {
            let same = old.get("identity").is_some_and(|i| dumps_sorted_compact(&from_serde(i)) == dumps_sorted_compact(&identity));
            if same && old["sha256"] == sha_file(&payload_path)? && old["manifest_sha256"] == sha_file(&manifest)? {
                println!("Focus audio cache verified");
                return Ok(());
            }
        }
    }
    let tool = Tool::build(root, "hk-focus-audio")?;
    let file = u(source.file("resources.assets"))?;
    let mut profiles = Vec::new();
    for (pid, rate, name) in [(1160, CHARGE_RATE, "focus_health_charging"), (1260, HEAL_RATE, "focus_health_heal")] {
        let clip = u(source.object(&file, pid))?;
        require(u(source.read(&clip))?.get("m_Name").and_then(Value::str).as_deref() == Some(name), "Focus clip identity changed")?;
        profiles.push(cook_clip(root, &tool, source, &clip, &out, rate, 1, Resampler::Sdk)?);
    }
    let raw: Vec<Vec<u8>> = profiles.iter().map(|p| std::fs::read(&p.planes[0].path).map_err(|e| e.to_string())).collect::<Result<_>>()?;
    let (focus, charge_bytes) = assemble(&raw[0], &raw[1], ambience_end)?;
    let mut abilities = ability_sounds(root, &tool, source, RUNNER_BASE - SPU_BASE - focus.len() as i64)?;
    let mut bank = focus.clone();
    for a in &abilities {
        bank.extend_from_slice(&a.payload);
    }
    require(SPU_BASE + bank.len() as i64 <= RUNNER_BASE, "Focus and ability sounds overlap the Runner bank")?;
    let checksum = fnv(&bank);
    std::fs::write(&payload_path, &bank).map_err(|e| e.to_string())?;
    let mut rows = String::new();
    let mut offset = SPU_BASE + focus.len() as i64;
    let mut ability_json = Vec::new();
    for a in &mut abilities {
        rows += &format!("    ({offset},{},{GAIN}), // {} ({})\n", a.rate, a.event, a.name);
        let bytes = a.payload.len() as i64;
        ability_json.push(jobj(vec![
            ("event", js(a.event)),
            ("name", js(a.name)),
            ("rate", Json::Int(a.rate)),
            ("samples", Json::Int(a.samples)),
            ("seconds", Json::Float(a.seconds)),
            ("source_rate", Json::Int(a.source_rate)),
            ("source_frames", Json::Int(a.source_frames)),
            ("spu_address", Json::Int(offset)),
            ("bytes", Json::Int(bytes)),
        ]));
        offset += bytes;
    }
    let constants: [(&str, &str, i64); 9] = [
        ("SPU_BASE", "u32", SPU_BASE),
        ("BANK_BYTES", "usize", bank.len() as i64),
        ("BANK_CHECKSUM", "u32", checksum as i64),
        ("CHARGE_BYTES", "usize", charge_bytes as i64),
        ("FOCUS_BYTES", "usize", focus.len() as i64),
        ("CHARGE_RATE", "u32", CHARGE_RATE),
        ("HEAL_RATE", "u32", HEAL_RATE),
        ("FADE_TICKS", "u32", fade_ticks),
        ("GAIN", "i16", GAIN),
    ];
    let mut text = String::from("// Generated complete Focus audio descriptor. Samples are loaded from CD.\n");
    for (name, kind, value) in constants {
        text += &format!("pub const {name}:{kind}={value};\n");
    }
    text += &format!("/// The Knight's ability one-shots after the Focus clips: {}.\n", abilities.iter().map(|a| a.event).collect::<Vec<_>>().join(", "));
    text += &format!("pub const ABILITY_SAMPLES:[(u32,u32,i16);{}]=[\n{rows}];\n", abilities.len());
    std::fs::write(&manifest, text).map_err(|e| e.to_string())?;
    let report = jobj(vec![
        ("identity", identity),
        ("path", Json::Str(rel(root, &payload_path))),
        ("byte_len", Json::Int(bank.len() as i64)),
        ("sha256", Json::Str(sha_file(&payload_path)?)),
        ("manifest_sha256", Json::Str(sha_file(&manifest)?)),
        ("checksum", Json::Int(checksum as i64)),
        ("spu_base", Json::Int(SPU_BASE)),
        ("spu_end", Json::Int(SPU_BASE + bank.len() as i64)),
        ("spu_free_bytes", Json::Int(RUNNER_BASE - SPU_BASE - bank.len() as i64)),
        ("charge_bytes", Json::Int(charge_bytes as i64)),
        ("heal_bytes", Json::Int((focus.len() - charge_bytes) as i64)),
        ("focus_bytes", Json::Int(focus.len() as i64)),
        ("abilities", Json::List(ability_json)),
        ("resampler", js("SDK psx_audio_cook::resample::Sinc")),
        ("contract", contract),
        ("profiles", Json::List(profiles.into_iter().map(|p| p.json).collect())),
        ("gain", Json::Int(GAIN)),
        ("voices", jobj(vec![("charge", Json::Int(18)), ("heal", Json::List(vec![Json::Int(19), Json::Int(20)]))])),
        (
            "limitations",
            Json::List(
                [
                    "Mono and category downsampling reduce fidelity; complete clips are retained.",
                    "SNR measures ADPCM reconstruction against resampled PCM, not retail fidelity.",
                    "Actors mixer DSP and downstream user mix are not replicated; gain uses the existing SFX headroom convention.",
                    "Charge loops have block padding; original FadeAudio exit clamps to zero before the nominal fade can complete.",
                    "Guest playback and hardware timing require separate validation.",
                ]
                .iter()
                .map(|s| js(s))
                .collect(),
            ),
        ),
    ]);
    dump(&report_path, &report)?;
    println!("Focus audio: {} bytes; SPU end {:#x} remaining {}", bank.len(), SPU_BASE + bank.len() as i64, SPU_END - SPU_BASE - bank.len() as i64);
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
    use crate::cook_audio::tests::{int, map};

    fn value(v: Value) -> Value {
        map(vec![("useVariable", Value::Bool(false)), ("value", v)])
    }
    fn pptr(path: i64) -> Value {
        map(vec![("m_FileID", int(0)), ("m_PathID", int(path))])
    }
    fn target() -> Value {
        map(vec![("ownerOption", int(1)), ("gameObject", map(vec![("useVariable", Value::Bool(true)), ("name", Value::Str(b"Charge Audio".to_vec()))]))])
    }
    fn action(kind: &str, fields: Vec<(&str, Value)>) -> Act {
        Act { action: kind.into(), enabled: Value::Bool(true), fields: fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect() }
    }
    fn fixture() -> (States, Vec<(String, String)>) {
        let mut states: States = ["Focus Start", "Focus Heal", "Focus Cancel", "Focus Get Finish", "Regain Control", "Cancel Some", "FSM Cancel", "Cancel All", "Focus", "Full HP?"].iter().map(|s| (s.to_string(), Vec::new())).collect();
        let set = |states: &mut States, name: &str, acts: Vec<Act>| states.iter_mut().find(|s| s.0 == name).unwrap().1 = acts;
        set(&mut states, "Focus Start", vec![action("AudioPlay", vec![("gameObject", target()), ("volume", value(int(1))), ("oneShotClip", value(pptr(0)))])]);
        set(
            &mut states,
            "Focus Heal",
            vec![action(
                "AudioPlayerOneShotSingle",
                vec![("volume", value(int(1))), ("pitchMin", value(int(1))), ("pitchMax", value(int(1))), ("delay", value(int(0))), ("audioClip", value(pptr(1260))), ("audioPlayer", value(pptr(4126)))],
            )],
        );
        for s in ["Focus Cancel", "Focus Get Finish"] {
            set(&mut states, s, vec![action("FadeAudio", vec![("gameObject", target()), ("startVolume", value(int(1))), ("endVolume", value(int(0))), ("time", value(Value::F32(0.33)))])]);
        }
        for s in ["Regain Control", "Cancel Some", "FSM Cancel", "Cancel All"] {
            set(&mut states, s, vec![action("AudioStop", vec![("gameObject", target())])]);
        }
        let globals = [("LEAVING SCENE", "Cancel Some"), ("FSM CANCEL", "FSM Cancel"), ("HERO DAMAGED", "Reset Cam Zoom")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        (states, globals)
    }

    #[test]
    fn full_bank_flags_preserve_all_sample_blocks() {
        let charge = [vec![12, 0], vec![0; 14], vec![28, 0], vec![0x31; 14]].concat();
        let heal = [vec![12, 0], vec![0x12; 14]].concat();
        let (bank, length) = assemble(&charge, &heal, SPU_BASE).unwrap();
        assert_eq!(length, charge.len());
        assert_eq!(bank.len(), charge.len() + heal.len() + 16);
        assert_eq!(bank[1], 4);
        assert_eq!(bank[charge.len() - 15], 3);
        assert_eq!(bank[2..16], charge[2..16]);
        assert_eq!(bank[18..32], charge[18..32]);
        assert_eq!(bank[length..length + heal.len()], heal[..]);
        assert_eq!(bank[bank.len() - 16..], [vec![12, 1], vec![0; 14]].concat()[..]);
    }

    #[test]
    fn invalid_input_and_capacity_rejected() {
        let block = [vec![12, 0], vec![0; 14]].concat();
        for payload in [vec![], vec![0; 15], [vec![0x5c, 0], vec![0; 14]].concat(), [vec![12, 1], vec![0; 14]].concat()] {
            assert!(oneshot_payload(&payload).is_err(), "{payload:?}");
        }
        assert!(assemble(&block, &block, SPU_BASE + 16).is_err());
        assert!(assemble(&block.repeat(10000), &block, SPU_BASE).is_err());
    }

    #[test]
    fn nominal_fade_and_interrupt_routes() {
        let (states, globals) = fixture();
        assert_eq!(validate_contract(&states, &globals).unwrap(), 20);
        for name in ["Focus Start", "Focus Heal", "Focus Cancel", "Focus Get Finish", "Regain Control", "Cancel Some", "FSM Cancel", "Cancel All"] {
            let (mut broken, _) = fixture();
            broken.iter_mut().find(|s| s.0 == name).unwrap().1[0].enabled = Value::Bool(false);
            assert!(validate_contract(&broken, &globals).is_err(), "{name}");
        }
        let mut broken = globals.clone();
        broken.iter_mut().find(|g| g.0 == "HERO DAMAGED").unwrap().1 = "Focus".into();
        assert!(validate_contract(&states, &broken).is_err());
    }

    #[test]
    fn repeat_never_restarts_or_stops_charge() {
        for kind in ["AudioPlay", "AudioStop", "FadeAudio"] {
            let (mut states, globals) = fixture();
            states.iter_mut().find(|s| s.0 == "Focus").unwrap().1.push(action(kind, vec![("gameObject", target())]));
            assert!(validate_contract(&states, &globals).is_err(), "{kind}");
        }
    }

    #[test]
    fn changed_clip_volume_pitch_and_fade_rejected() {
        for (state, key, replacement) in [("Focus Start", "volume", int(0)), ("Focus Heal", "pitchMax", int(2)), ("Focus Heal", "delay", Value::F32(0.1)), ("Focus Heal", "audioClip", pptr(42)), ("Focus Cancel", "time", Value::F32(0.5))] {
            let (mut broken, globals) = fixture();
            let acts = &mut broken.iter_mut().find(|s| s.0 == state).unwrap().1;
            acts[0].fields.iter_mut().find(|f| f.0 == key).unwrap().1 = value(replacement);
            assert!(validate_contract(&broken, &globals).is_err(), "{key}");
        }
    }
}
