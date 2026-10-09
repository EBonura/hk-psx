//! Source-mapped resident one-shots; long death layers stay outside the EXE.
//! Ported from host/cook_audio.py, whose output it reproduces byte for byte
//! (data/sfx.*, data/world-sfx.*, data/ui_sfx.adpcm, data/door.adpcm,
//! .hkpsx/audio-extra and .hkpsx/audio-provenance.json, except the
//! generator hash, which names this module instead of the Python file).
//!
//! The hero's one-shots, the False Knight's two voices and the Knight's extra
//! one-shots share one SPU window below the Geo bank; the SDK's rate
//! allocator (`psx-audio-cook plan`) picks each clip's rate from the ladder
//! so they all fit at the least band loss. Menu clips sit above that bank and
//! the world one-shots that did not fit go in a second bank below the Focus
//! bank. Every clip is read through the component that plays it, so a patch
//! that rewires one fails the cook instead of shipping the old sound.
//!
//! Clip pipeline: the FSB decoded by FMOD (`crate::fmod`), folded to mono and
//! resampled by the SDK resampler (hero, boss and extra one-shots) or by
//! ffmpeg (menu, death and world clips), and encoded by the SDK encoder.

use crate::common::{err, get, py_round, Result};
use crate::fmod::clip_wav;
use crate::pyjson::{dumps, Json};
use crate::spu::{decode_oneshot, ffmpeg_mono, fnv, read_wav, Tool};
use hk_unity::playmaker::{action_fields, field};
use hk_unity::serialized::Cursor;
use hk_unity::typetree::{Flavor, Reader};
use hk_unity::{Obj, Source, Value};
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::Path;

const SPU_BASE: i64 = 0x1010;
/// Root layout reserves 0x14000..0x18000 for Geo; ambience begins at 0x18000.
const BANK_LIMIT: i64 = 0x14000 - SPU_BASE;
/// The world bank sits directly below the Focus bank.
const FOCUS_SPU_BASE: i64 = crate::focus_audio::SPU_BASE;
const EVENTS: [&str; 8] = [
    "door",
    "jump",
    "land",
    "nail",
    "hurt",
    "enemy_hit",
    "hard_land",
    "footsteps_run",
];
/// Hero one-shots never go above the rate they shipped at; Manny's rule is to
/// halve the rate where room is needed, never to trim a clip.
const RATE_LADDER: [i64; 3] = [22050, 11025, 5512];

/// The Knight's sounds the port did not play, appended after the boss voices
/// so the dedicated-voice indices never move: (event, file, path id, clip,
/// source volume, where the source plays it).
const HERO_EXTRA: [(&str, &str, i64, &str, f64, &str); 7] = [
    (
        "nail_alt",
        "resources.assets",
        1326,
        "sword_4",
        1.0,
        "Knight/Attacks/AltSlash AudioSource",
    ),
    (
        "nail_down",
        "resources.assets",
        1195,
        "sword_2",
        1.0,
        "Knight/Attacks/DownSlash AudioSource",
    ),
    (
        "dash",
        "resources.assets",
        1186,
        "hero_dash",
        1.0,
        "Knight/Sounds/Dash AudioSource",
    ),
    (
        "walljump",
        "resources.assets",
        1302,
        "hero_wall_jump",
        0.5,
        "Knight/Sounds/Walljump AudioSource",
    ),
    (
        "wings",
        "resources.assets",
        1347,
        "hero_wings",
        1.0,
        "HeroController.doubleJumpClip",
    ),
    (
        "claw",
        "resources.assets",
        1250,
        "hero_mantis_claw",
        1.0,
        "HeroController.mantisClawClip",
    ),
    (
        "shade_dash",
        "resources.assets",
        1153,
        "hero_shade_dash_1",
        1.0,
        "HeroController.shadowDashClip",
    ),
];

/// The False Knight's own voices, appended after the hero events in the same
/// resident bank: (event, file, path id, clip, rate, states that play it).
/// `FalseyControl` plays each through `AudioPlaySimple` at volume 1.0, pitch 1.0.
struct BossEvent {
    event: &'static str,
    file: &'static str,
    path_id: i64,
    name: &'static str,
    rate: i64,
    states: &'static [&'static str],
}
const BOSS_EVENTS: [BossEvent; 2] = [
    BossEvent {
        event: "boss_land",
        file: "sharedassets32.assets",
        path_id: 131,
        name: "false_knight_land",
        rate: 11025,
        states: &["S Land", "State 2", "Land Noise"],
    },
    BossEvent {
        event: "boss_swing",
        file: "sharedassets48.assets",
        path_id: 39,
        name: "false_knight_swing",
        rate: 22050,
        states: &["S Attack", "JA Hit 2"],
    },
];

/// Every other clip `FalseyControl` reaches, and the state that plays it. None
/// is admitted, and the refusal is measured (bytes at each rate), not asserted.
const BOSS_REFUSED: [(&str, i64, &str, &str); 18] = [
    (
        "sharedassets48.assets",
        28,
        "false_knight_strike_ground",
        "Slam, Rage Slam, JA Slam, Floor Break",
    ),
    (
        "sharedassets19.assets",
        31,
        "false_knight_ceiling_break",
        "Start Fall, Floor Break",
    ),
    (
        "sharedassets46.assets",
        22,
        "false_knight_damage_armour_final",
        "Stun Start",
    ),
    (
        "sharedassets48.assets",
        45,
        "false_knight_jump",
        "Jump 2, JA Jump 2",
    ),
    (
        "sharedassets6.assets",
        171,
        "false_knight_land_1st_time",
        "Rubble End, Death Land",
    ),
    (
        "sharedassets48.assets",
        21,
        "false_knight_roll",
        "Stun Land",
    ),
    ("sharedassets48.assets", 29, "zombie_guard_footstep", "Run"),
    (
        "sharedassets32.assets",
        143,
        "zombie_shield_raise",
        "Open Uuup, Death Open",
    ),
    (
        "sharedassets32.assets",
        87,
        "zombie_shield_move",
        "Open Uuup, Death Open",
    ),
    (
        "sharedassets48.assets",
        30,
        "FKnight_Rage",
        "Jump 2, Esc Jump",
    ),
    ("sharedassets48.assets", 38, "FKnight_death", "Steam"),
    (
        "sharedassets32.assets",
        135,
        "boss_final_hit",
        "Death Anim Start",
    ),
    ("sharedassets32.assets", 62, "boss_gushing", "Steam"),
    ("sharedassets32.assets", 99, "boss_explode", "Blow"),
    (
        "sharedassets40.assets",
        30,
        "Boss Defeat",
        "Boss Death Sting",
    ),
    (
        "sharedassets6.assets",
        102,
        "breakable_wall_death",
        "Floor Break",
    ),
    ("resources.assets", 1308, "enemy_death_sword", "Recover"),
    ("resources.assets", 1248, "enemy_damage", "Recover"),
];

/// The menu's own clips, from resources.assets' one MenuAudioController.
const MENU_AUDIO_CONTROLLER: i64 = 23421;
const UI_EVENTS: [(&str, &str, i64); 2] = [
    ("select", "ui_change_selection", 11025),
    ("slider", "ui_option_click", 11025),
];
const UI_WORLD: [(&str, &str); 3] = [
    ("submit", "ui_button_confirm"),
    ("cancel", "ui_button_confirm"),
    ("startGame", "spa_heal"),
];

/// One-shots every scene can play that did not fit below the Geo bank:
/// (event, where the source names the clip, clip name, rate). Each rate was
/// picked from the share of the clip's source energy above the new Nyquist.
const WORLD_EVENTS: [(&str, &str, &str, i64); 5] = [
    (
        "enemy_death",
        "EnemyDeathEffects.enemyDeathSwordAudio",
        "enemy_death_sword",
        22050,
    ),
    (
        "hero_death",
        "Hero Death FSM Start layer 0",
        "hero_death_v2",
        4000,
    ),
    (
        "ui_confirm",
        "MenuAudioController.submit/cancel",
        "ui_button_confirm",
        5512,
    ),
    (
        "ui_start",
        "MenuAudioController.startGame",
        "spa_heal",
        8000,
    ),
    (
        "cocoon_break",
        "HealthCocoon.deathSound",
        "health_cocoon_break",
        11025,
    ),
];
/// The Crawler's EnemyDeathEffects in King's Pass, and the Health Cocoon.
const ENEMY_DEATH_EFFECTS: i64 = 12681;
const HEALTH_COCOON: i64 = 12337;
/// The serialized HeroController tail is not parsed by the cooker. This hash
/// binds its environment0 Dust mapping to the actual-original reflection audit.
const MOVEMENT_HERO_SHA256: &str =
    "8a4a799f36e522e7caccffb0c9c345e9a82eb9b2ef2d73024049439340e4e345";

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
pub(crate) fn sha(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}
pub(crate) fn u<T>(r: hk_unity::Result<T>) -> Result<T> {
    r.map_err(|e| e.to_string())
}
pub(crate) fn div(a: i64, b: i64) -> f64 {
    a as f64 / b as f64
}
pub(crate) fn js(s: &str) -> Json {
    Json::Str(s.to_string())
}
pub(crate) fn jobj(fields: Vec<(&str, Json)>) -> Json {
    Json::Obj(
        fields
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    )
}
pub(crate) fn jfloat(v: &Value, key: &str) -> Result<f64> {
    get(v, key)?
        .float()
        .ok_or_else(|| format!("{key} is not a number"))
}
pub(crate) fn ints_of(v: &Value, key: &str) -> Result<Vec<i64>> {
    Ok(get(v, key)?
        .list()
        .ok_or_else(|| format!("{key} is not a list"))?
        .iter()
        .map(|x| x.int().unwrap_or(0))
        .collect())
}
pub(crate) fn strs_of(v: &Value, key: &str) -> Result<Vec<String>> {
    Ok(get(v, key)?
        .list()
        .ok_or_else(|| format!("{key} is not a list"))?
        .iter()
        .map(|x| x.str().unwrap_or_default())
        .collect())
}
/// `bytes(data['byteData'])`, whether the tree read it as bytes or as a list.
pub(crate) fn byte_data(v: &Value) -> Result<Vec<u8>> {
    match get(v, "byteData")? {
        Value::Bytes(b) => Ok(b.clone()),
        other => Ok(other
            .list()
            .ok_or("byteData is not a list")?
            .iter()
            .map(|x| x.int().unwrap_or(0) as u8)
            .collect()),
    }
}
/// Python's `byte_data[offset:offset + size]`.
pub(crate) fn slice(bytes: &[u8], offset: i64, size: i64) -> Vec<u8> {
    let lo = (offset.max(0) as usize).min(bytes.len());
    let hi = ((offset + size).max(0) as usize).min(bytes.len()).max(lo);
    bytes[lo..hi].to_vec()
}

/// What `encode(convert_wav(...))` will weigh, without spending the encode:
/// one 16-byte block per 28 samples plus the terminator.
fn encoded_bytes(frames: i64, source_rate: i64, rate: i64) -> Result<i64> {
    if frames <= 0 || source_rate <= 0 || rate <= 0 {
        return err("cannot size a clip without a source duration");
    }
    Ok((py_round(div(frames * rate, source_rate)) + 27) / 28 * 16 + 16)
}

/// Source frame count and rate from the WAV header alone, no PCM decode.
fn wav_frames(data: &[u8]) -> Result<(i64, i64)> {
    let w = read_wav(data)?;
    if w.width != 2 || !(w.channels == 1 || w.channels == 2) {
        return err("unvalidated source PCM format");
    }
    Ok((
        (w.data.len() / (2 * w.channels as usize)) as i64,
        w.rate as i64,
    ))
}

// ---------------------------------------------------------------- FSM contracts

/// An enabled immediate one-shot in one FSM state.
struct Played {
    clip: Option<String>,
    volume: Option<f64>,
    pitch: [f64; 2],
}

/// Every enabled immediate one-shot in one FSM state, with its clip. `resolve`
/// maps a serialized AudioClip PPtr to a source id.
fn audio_actions(data: &Value, resolve: &dyn Fn(&Value) -> Result<String>) -> Result<Vec<Played>> {
    let names = strs_of(data, "actionNames")?;
    let enabled = get(data, "actionEnabled")?.list().unwrap_or(&[]).to_vec();
    let starts = ints_of(data, "actionStartIndex")?;
    let param_names = strs_of(data, "paramName")?;
    let kinds = ints_of(data, "paramDataType")?;
    let positions = ints_of(data, "paramDataPos")?;
    let sizes = ints_of(data, "paramByteDataSize")?;
    let bytes = byte_data(data)?;
    let mut out = Vec::new();
    for (action, name) in names.iter().enumerate() {
        let short = name.rsplit('.').next().unwrap_or("");
        if !matches!(short, "AudioPlaySimple" | "AudioPlayerOneShotSingle")
            || !enabled.get(action).is_some_and(Value::truthy)
        {
            continue;
        }
        let start = starts[action] as usize;
        let end = if action + 1 < names.len() {
            starts[action + 1] as usize
        } else {
            param_names.len()
        };
        let mut record = Played {
            clip: None,
            volume: None,
            pitch: [1.0, 1.0],
        };
        for i in start..end {
            let field_name = param_names[i].as_str();
            if kinds[i] == 24 && matches!(field_name, "oneShotClip" | "audioClip") {
                let objects = get(data, "fsmObjectParams")?
                    .list()
                    .ok_or("fsmObjectParams is not a list")?;
                let obj = objects
                    .get(positions[i] as usize)
                    .ok_or("fsmObjectParams index out of range")?;
                if get(obj, "useVariable")?.truthy()
                    || get(obj, "typeName")?.str().as_deref() != Some("UnityEngine.AudioClip")
                {
                    return err("dynamic boss clip reference");
                }
                record.clip = Some(resolve(get(obj, "value")?)?);
            } else if kinds[i] == 15 && matches!(field_name, "volume" | "pitchMin" | "pitchMax") {
                let raw = slice(&bytes, positions[i], sizes[i]);
                if sizes[i] < 4 || raw.len() < 4 {
                    return err("unvalidated boss audio scalar");
                }
                let value = f32::from_le_bytes(raw[..4].try_into().unwrap()) as f64;
                if !value.is_finite() {
                    return err("non-finite boss audio scalar");
                }
                match field_name {
                    "volume" => record.volume = Some(value),
                    "pitchMin" => record.pitch[0] = value,
                    _ => record.pitch[1] = value,
                }
            }
        }
        out.push(record);
    }
    Ok(out)
}

/// Bind every admitted boss voice to the `FalseyControl` state that plays it.
/// A state that stopped playing its clip, gained a second copy of it, or moved
/// off unit gain and pitch fails the cook.
fn false_knight_audio_contract(
    fsm: &Value,
    resolve: &dyn Fn(&Value) -> Result<String>,
) -> Result<Vec<Json>> {
    if get(fsm, "name")?.str().as_deref() != Some("FalseyControl") {
        return err("False Knight audio FSM changed");
    }
    let states = get(fsm, "states")?.list().ok_or("states is not a list")?;
    // A dict comprehension keeps the last state of a repeated name.
    let state_of = |name: &str| {
        states
            .iter()
            .rev()
            .find(|s| s.get("name").and_then(Value::str).as_deref() == Some(name))
    };
    let mut bindings = Vec::new();
    for b in &BOSS_EVENTS {
        let sid = format!("{}:{}", b.file, b.path_id);
        for &wanted in b.states {
            let state = state_of(wanted)
                .ok_or_else(|| format!("missing False Knight audio state: {wanted}"))?;
            let played: Vec<Played> = audio_actions(get(state, "actionData")?, resolve)?
                .into_iter()
                .filter(|a| a.clip.as_deref() == Some(&sid))
                .collect();
            if played.len() != 1 {
                return err(format!("{wanted} no longer plays {} exactly once", b.name));
            }
            if played[0].volume != Some(1.0) || played[0].pitch != [1.0, 1.0] {
                return err(format!("{wanted} changed the {} gain or pitch", b.name));
            }
        }
        bindings.push(jobj(vec![
            ("event", js(b.event)),
            ("source", Json::Str(sid)),
            ("name", js(b.name)),
            ("sample_rate", Json::Int(b.rate)),
            (
                "states",
                Json::List(b.states.iter().map(|s| js(s)).collect()),
            ),
            ("action", js("AudioPlaySimple")),
            (
                "method",
                Json::Str(format!("FalseyControl.{}", b.states.join("/"))),
            ),
        ]));
    }
    Ok(bindings)
}

/// Validate the exact source action before sharing the resident wall clip:
/// the pitch register bounds of the Great Door's hit.
fn great_door_hit_contract(fsm: &Value, sample_rate: i64) -> Result<[i64; 2]> {
    if get(fsm, "name")?.str().as_deref() != Some("Great Door") {
        return err("Great Door sound FSM changed");
    }
    let states = get(fsm, "states")?.list().ok_or("states is not a list")?;
    let state = states
        .iter()
        .find(|s| s.get("name").and_then(Value::str).as_deref() == Some("Hit"))
        .ok_or("Great Door has no Hit state")?;
    let data = get(state, "actionData")?;
    let names = strs_of(data, "actionNames")?;
    let enabled = get(data, "actionEnabled")?.list().unwrap_or(&[]).to_vec();
    let actions: Vec<usize> = names
        .iter()
        .enumerate()
        .filter(|(i, n)| {
            n.ends_with(".AudioPlayRandom") && enabled.get(*i).is_some_and(Value::truthy)
        })
        .map(|(i, _)| i)
        .collect();
    if actions.len() != 1 {
        return err("Great Door hit sound action changed");
    }
    let fields = u(action_fields(data, actions[0], false))?;
    let pitch: Vec<&Value> = ["pitchMin", "pitchMax"]
        .iter()
        .map(|n| field(&fields, n).ok_or_else(|| format!("Great Door has no {n}")))
        .collect::<Result<_>>()?;
    if pitch
        .iter()
        .any(|p| p.get("useVariable").is_some_and(Value::truthy))
    {
        return err("Great Door pitch is dynamic");
    }
    let values: Vec<f64> = pitch
        .iter()
        .map(|p| jfloat(p, "value"))
        .collect::<Result<_>>()?;
    if values
        .iter()
        .zip([0.85, 1.15])
        .any(|(a, b)| (a - b).abs() > 1e-6)
    {
        return err("Great Door pitch bounds changed");
    }
    let pptr = |id: i64| {
        Value::Map(vec![
            ("m_FileID".into(), Value::Int(2)),
            ("m_PathID".into(), Value::Int(id)),
        ])
    };
    let expected = Value::List(vec![pptr(92), pptr(99)]);
    if !get(data, "unityObjectParams")?.py_eq(&expected) {
        return err("Great Door hit clip selection changed");
    }
    Ok([
        py_round((sample_rate * 4096) as f64 / 44100.0 * values[0]),
        py_round((sample_rate * 4096) as f64 / 44100.0 * values[1]),
    ])
}

/// One authored death layer: constant, immediate AudioPlayerOneShotSingle.
struct Layer {
    clip: Value,
    volume: f64,
    pitch: [f64; 2],
    delay: f64,
    action_index: usize,
}

/// Only constant, immediate source AudioPlayerOneShotSingle actions.
fn death_action_layers(data: &Value) -> Result<Vec<Layer>> {
    let names = strs_of(data, "actionNames")?;
    let enabled = get(data, "actionEnabled")?.list().unwrap_or(&[]).to_vec();
    let starts = ints_of(data, "actionStartIndex")?;
    let param_names = strs_of(data, "paramName")?;
    let kinds = ints_of(data, "paramDataType")?;
    let positions = ints_of(data, "paramDataPos")?;
    let sizes = ints_of(data, "paramByteDataSize")?;
    let bytes = byte_data(data)?;
    let mut layers = Vec::new();
    for (action, name) in names.iter().enumerate() {
        if !name.ends_with(".AudioPlayerOneShotSingle") {
            continue;
        }
        if !enabled.get(action).is_some_and(Value::truthy) {
            return err("death audio action disabled in source");
        }
        let start = starts[action] as usize;
        let end = if action + 1 < names.len() {
            starts[action + 1] as usize
        } else {
            param_names.len()
        };
        let mut fields: Vec<(&str, usize)> = Vec::new();
        for i in start..end {
            match fields.iter_mut().find(|(k, _)| *k == param_names[i]) {
                Some(slot) => slot.1 = i,
                None => fields.push((&param_names[i], i)),
            }
        }
        let index_of = |f: &str| {
            fields
                .iter()
                .find(|(k, _)| *k == f)
                .map(|(_, i)| *i)
                .ok_or_else(|| format!("death audio action has no {f}"))
        };
        let literal = |f: &str| -> Result<f64> {
            let i = index_of(f)?;
            let raw = slice(&bytes, positions[i], sizes[i]);
            if kinds[i] != 15 || raw.len() != 5 || raw[4] != 0 {
                return err("dynamic or unvalidated death audio scalar");
            }
            let value = f32::from_le_bytes(raw[..4].try_into().unwrap()) as f64;
            if !value.is_finite() {
                return err("non-finite death audio scalar");
            }
            Ok(value)
        };
        let index = index_of("audioClip")?;
        if kinds[index] != 24 {
            return err("unvalidated death AudioClip parameter");
        }
        let objects = get(data, "fsmObjectParams")?
            .list()
            .ok_or("fsmObjectParams is not a list")?;
        let obj = objects
            .get(positions[index] as usize)
            .ok_or("fsmObjectParams index out of range")?;
        if get(obj, "useVariable")?.truthy()
            || get(obj, "typeName")?.str().as_deref() != Some("UnityEngine.AudioClip")
        {
            return err("dynamic death clip reference");
        }
        let (volume, delay) = (literal("volume")?, literal("delay")?);
        let pitch = [literal("pitchMin")?, literal("pitchMax")?];
        if !(0.0..=1.0).contains(&volume) || delay != 0.0 || pitch != [1.0, 1.0] {
            return err("changed death voice gain, timing or pitch");
        }
        layers.push(Layer {
            clip: get(obj, "value")?.clone(),
            volume,
            pitch,
            delay,
            action_index: action,
        });
    }
    if layers.len() != 2 {
        return err("expected both authored death layers");
    }
    Ok(layers)
}

// ---------------------------------------------------------------- cooked clips

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Resampler {
    Ffmpeg,
    Sdk,
}

/// One cooked clip: its payload, and the fields the provenance records in the
/// order Python's dict held them.
struct Cooked {
    encoded: Vec<u8>,
    meta: Vec<(String, Json)>,
    source_frames: i64,
    source_rate: i64,
    sample_rate: i64,
    samples: i64,
    volume_q14: i64,
}

impl Cooked {
    fn meta(&mut self, key: &str, value: Json) {
        match self.meta.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = value,
            None => self.meta.push((key.to_string(), value)),
        }
    }
}

/// A cooked clip placed in a bank: `dict(meta, event=..., offset=..., bytes=...)`.
struct Record {
    event: String,
    name: String,
    offset: i64,
    bytes: i64,
    sample_rate: i64,
    samples: i64,
    volume_q14: i64,
    json: Vec<(String, Json)>,
}

/// No partial admission: the clips in order, each a whole decodable one-shot,
/// back to back. Returns the bank and a record per clip.
fn pack(items: Vec<(String, Cooked)>) -> Result<(Vec<u8>, Vec<Record>)> {
    let mut out = Vec::new();
    let mut records = Vec::new();
    for (event, c) in items {
        decode_oneshot(&c.encoded)?;
        let mut json = c.meta.clone();
        json.push(("event".into(), Json::Str(event.clone())));
        json.push(("offset".into(), Json::Int(out.len() as i64)));
        json.push(("bytes".into(), Json::Int(c.encoded.len() as i64)));
        let name = c
            .meta
            .iter()
            .find(|(k, _)| k == "name")
            .and_then(|(_, v)| {
                if let Json::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            })
            .unwrap_or_default();
        records.push(Record {
            event,
            name,
            offset: out.len() as i64,
            bytes: c.encoded.len() as i64,
            sample_rate: c.sample_rate,
            samples: c.samples,
            volume_q14: c.volume_q14,
            json,
        });
        out.extend_from_slice(&c.encoded);
    }
    Ok((out, records))
}

fn pack_bank(
    items: Vec<(String, Cooked)>,
    boss: usize,
    extra: usize,
    limit: i64,
) -> Result<(Vec<u8>, Vec<Record>)> {
    let wanted: Vec<&str> = EVENTS
        .iter()
        .copied()
        .chain(BOSS_EVENTS.iter().map(|b| b.event))
        .chain(HERO_EXTRA.iter().map(|e| e.0))
        .collect();
    if items.len() != EVENTS.len() + boss + extra
        || items
            .iter()
            .map(|i| i.0.as_str())
            .ne(wanted.iter().copied())
    {
        return err("resident sound event table is incomplete or reordered");
    }
    let (bank, records) = pack(items)?;
    if bank.len() as i64 > limit {
        return err(format!(
            "resident SFX bank {} exceeds {limit} bytes",
            bank.len()
        ));
    }
    Ok((bank, records))
}

struct Cook<'a> {
    root: &'a Path,
    source: &'a Source,
    tool: Tool,
    /// Source files the cooked clips came from: name, bytes, sha256.
    inputs: Vec<(String, Json)>,
}

impl Cook<'_> {
    fn read(&self, o: &Obj) -> Result<Value> {
        u(self.source.read(o))
    }
    fn object(&self, file: &str, id: i64) -> Result<Obj> {
        let f = u(self.source.file(file))?;
        u(self.source.object(&f, id))
    }
    fn deref(&self, from: &Obj, pptr: &Value) -> Result<Obj> {
        u(self.source.deref(&from.file, pptr))
    }
    fn expect_type(&self, o: &Obj, want: &str, what: &str) -> Result<()> {
        if u(self.source.typename(o))? != want {
            return err(what.to_string());
        }
        Ok(())
    }

    /// The clip's single WAV, after checking its name.
    fn source_wav(&self, clip: &Obj, expected: &str) -> Result<(Value, Vec<u8>)> {
        let tree = self.read(clip)?;
        if tree.get("m_Name").and_then(Value::str).as_deref() != Some(expected) {
            return err(format!("changed source sound mapping: {expected}"));
        }
        let wav = clip_wav(self.root, self.source, &tree)?;
        Ok((tree, wav))
    }

    /// Record a source file the first time a cooked clip draws on it.
    fn note_input(&mut self, name: &str) -> Result<()> {
        let base = name.rsplit('/').next().unwrap_or(name);
        let path = self.source.directory.join(base);
        if name.is_empty() || !path.is_file() || self.inputs.iter().any(|(k, _)| k == base) {
            return Ok(());
        }
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        self.inputs.push((
            base.to_string(),
            jobj(vec![
                ("bytes", Json::Int(bytes.len() as i64)),
                ("sha256", Json::Str(sha(&bytes))),
            ]),
        ));
        Ok(())
    }

    fn cook(
        &mut self,
        clip: &Obj,
        expected: &str,
        volume: f64,
        rate: i64,
        origin: Json,
        resampler: Resampler,
    ) -> Result<Cooked> {
        if !(0.0..=1.0).contains(&volume) {
            return err(format!("changed source sound mapping: {expected}"));
        }
        let (tree, wav) = self.source_wav(clip, expected)?;
        let (pcm, mut meta) = convert_wav(&self.tool, &wav, rate, resampler)?;
        let encoded = self.tool.encode_oneshot(&pcm)?;
        let mut reconstructed = decode_oneshot(&encoded)?;
        reconstructed.truncate(pcm.len());
        let n = pcm.len().max(1) as i64;
        let mse = div(
            pcm.iter()
                .zip(&reconstructed)
                .map(|(&a, &b)| (a as i64 - b as i64).pow(2))
                .sum::<i64>(),
            n,
        );
        let power = div(pcm.iter().map(|&a| a as i64 * a as i64).sum::<i64>(), n);
        let resource = tree
            .get("m_Resource")
            .and_then(|r| r.get("m_Source"))
            .and_then(Value::str)
            .unwrap_or_default();
        self.note_input(hk_unity::base_name(&clip.file.name))?;
        self.note_input(&resource)?;
        let volume_q14 = py_round(0x3fff as f64 * volume / 3.0);
        meta.extend([
            ("source_id".to_string(), Json::Str(clip.sid())),
            ("name".into(), js(expected)),
            ("origin".into(), origin),
            ("source_volume".into(), Json::Float(volume)),
            ("volume_q14".into(), Json::Int(volume_q14)),
            ("output_sha256".into(), Json::Str(sha(&encoded))),
            (
                "pcm_peak".into(),
                Json::Int(pcm.iter().map(|&a| (a as i64).abs()).max().unwrap_or(0)),
            ),
            (
                "decoded_peak".into(),
                Json::Int(
                    reconstructed
                        .iter()
                        .map(|&a| (a as i64).abs())
                        .max()
                        .unwrap_or(0),
                ),
            ),
            ("rmse".into(), Json::Float(mse.sqrt())),
            (
                "snr_db".into(),
                if mse != 0.0 && power != 0.0 {
                    Json::Float(10.0 * (power / mse).log10())
                } else {
                    Json::Null
                },
            ),
        ]);
        let rate_of = |k: &str| {
            meta.iter()
                .find(|(n, _)| n == k)
                .and_then(|(_, v)| if let Json::Int(i) = v { Some(*i) } else { None })
                .unwrap_or(0)
        };
        Ok(Cooked {
            source_frames: rate_of("source_frames"),
            source_rate: rate_of("source_rate"),
            sample_rate: rate,
            samples: pcm.len() as i64,
            volume_q14,
            encoded,
            meta,
        })
    }

    /// The rate each of `rows` (key, wav, top rate) takes from the SDK
    /// allocator: its top rate or one halving below it, so all fit `budget`.
    fn allocate_rates(&self, rows: &[(String, Vec<u8>, i64)], budget: i64) -> Result<Vec<i64>> {
        let mut request = vec![format!("budget\t{budget}")];
        let mut ladders = Vec::new();
        for (i, (_, data, top)) in rows.iter().enumerate() {
            let (frames, source_rate) = wav_frames(data)?;
            // At most one halving below the rate a clip shipped at.
            let ladder: Vec<i64> = RATE_LADDER
                .iter()
                .copied()
                .filter(|&r| r <= *top)
                .take(2)
                .collect();
            let sizes = ladder
                .iter()
                .map(|&r| encoded_bytes(frames, source_rate, r))
                .collect::<Result<Vec<_>>>()?;
            let path = self.tool.scratch.join(format!("plan-{i}.wav"));
            std::fs::write(&path, data).map_err(|e| e.to_string())?;
            let join = |v: &[i64]| v.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
            request.push(format!(
                "1\t{}\t{}\t{}\t{}",
                ladder.len() - 1,
                path.display(),
                join(&ladder),
                join(&sizes)
            ));
            ladders.push(ladder);
        }
        let out = self.tool.plan(&(request.join("\n") + "\n"))?;
        let lines: Vec<&str> = out.lines().filter(|l| !l.trim().is_empty()).collect();
        if lines.is_empty() || lines[0].trim() == "none" {
            return err("the hero sounds do not fit the SFX window at any allowed rate");
        }
        if lines.len() != ladders.len() {
            return err("zip() argument 2 is shorter than argument 1");
        }
        lines
            .iter()
            .zip(&ladders)
            .map(|(l, ladder)| {
                Ok(ladder[l
                    .split('\t')
                    .next()
                    .unwrap()
                    .parse::<usize>()
                    .map_err(|e| e.to_string())?])
            })
            .collect()
    }
}

/// Mono fold-down and resample, preserving the complete duration.
pub(crate) fn convert_wav(
    tool: &Tool,
    data: &[u8],
    rate: i64,
    resampler: Resampler,
) -> Result<(Vec<i16>, Vec<(String, Json)>)> {
    let wav = read_wav(data)?;
    if wav.width != 2 || !(wav.channels == 1 || wav.channels == 2) {
        return err("unvalidated source PCM format or rate conversion");
    }
    let (channels, source_rate) = (wav.channels as i64, wav.rate as i64);
    let frames = (wav.data.len() / (2 * wav.channels as usize)) as i64;
    let mut meta = vec![
        ("source_rate".to_string(), Json::Int(source_rate)),
        ("source_channels".into(), Json::Int(channels)),
        ("source_frames".into(), Json::Int(frames)),
        ("sample_rate".into(), Json::Int(rate)),
    ];
    if frames == 0 {
        meta.push(("samples".into(), Json::Int(0)));
        return Ok((Vec::new(), meta));
    }
    let pcm = match resampler {
        Resampler::Sdk => tool.resample(data, rate)?,
        Resampler::Ffmpeg => ffmpeg_mono(data, rate)?,
    };
    // The resampler primes its filter, so a clip shorter than that window
    // can come back truncated; fail rather than ship it missing a head or tail.
    let expected = py_round(div(frames * rate, source_rate));
    if (pcm.len() as i64 - expected).abs() > 1 {
        return err(format!(
            "resampled length {} is not the expected {expected}",
            pcm.len()
        ));
    }
    meta.push(("samples".into(), Json::Int(pcm.len() as i64)));
    meta.push((
        "resampler".into(),
        js(if resampler == Resampler::Sdk {
            "sdk"
        } else {
            "ffmpeg"
        }),
    ));
    Ok((pcm, meta))
}

// ---------------------------------------------------------------- main

/// `(scene_name, file)` of every scene of the catalogue (quality.SCENE_TABLE,
/// as data/regions.json carries it).
fn scene_table(root: &Path) -> Result<Vec<(String, String)>> {
    let path = root.join("data/regions.json");
    let report: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?,
    )
    .map_err(|e| e.to_string())?;
    report["scenes"]
        .as_array()
        .ok_or("data/regions.json has no scenes")?
        .iter()
        .map(|s| {
            Ok((
                s["scene_name"]
                    .as_str()
                    .ok_or("scene without a name")?
                    .to_string(),
                s["file"]
                    .as_str()
                    .ok_or("scene without a file")?
                    .to_string(),
            ))
        })
        .collect()
}

/// A `[x, y]` pair of floats, as Python held a clip's pitch range.
fn pair(a: f64, b: f64) -> Json {
    Json::List(vec![Json::Float(a), Json::Float(b)])
}

pub fn cook(root: &Path, source: &Source) -> Result<()> {
    let mut c = Cook {
        root,
        source,
        tool: Tool::build(root, "hk-cook-audio")?,
        inputs: Vec::new(),
    };
    let resources_file = u(source.file("resources.assets"))?;
    let resources = |id: i64| u(source.object(&resources_file, id));
    let controller = resources(22332)?;
    c.expect_type(
        &controller,
        "HeroAudioController",
        "source HeroAudioController changed",
    )?;
    let hero_audio = c.read(&controller)?;
    if sha(resources(20602)?.raw().map_err(|e| e.to_string())?) != MOVEMENT_HERO_SHA256 {
        return err(
            "HeroController movement audio linkage changed; revalidate environment Dust mapping",
        );
    }
    // Footsteps are cooked for the verified Dust environment (type 0). Scenes
    // whose SceneManager selects another surface keep the Dust set and are
    // recorded as an explicit omission instead of stopping the cook.
    let scenes = scene_table(root)?;
    let found: Vec<Result<Option<(String, i64)>>> = scenes
        .par_iter()
        .map(|(name, file)| {
            let f = u(source.file(file))?;
            let mut managers = Vec::new();
            for info in f.objects.iter().filter(|i| i.class_id == 114) {
                let o = Obj {
                    file: f.clone(),
                    info: *info,
                };
                if u(source.typename(&o))? == "SceneManager" {
                    managers.push(o);
                }
            }
            if managers.len() != 1 {
                return err(format!(
                    "{name}: expected one SceneManager for movement audio"
                ));
            }
            let environment = u(source.read(&managers[0]))?
                .get("environmentType")
                .and_then(Value::int)
                .ok_or("SceneManager without environmentType")?;
            Ok((environment != 0).then(|| (name.clone(), environment)))
        })
        .collect();
    let mut omissions = Vec::new();
    for f in found {
        if let Some((scene, environment)) = f? {
            omissions.push(jobj(vec![
                ("scene", Json::Str(scene)),
                ("environment_type", Json::Int(environment)),
                (
                    "note",
                    js("footsteps use the Dust set; this surface set is not cooked"),
                ),
            ]));
        }
    }
    if !omissions.is_empty() {
        println!(
            "Movement audio: {} scenes use a non-Dust footstep environment (Dust set substituted)",
            omissions.len()
        );
    }

    // The hero's one-shots, each resolved through the component that plays it.
    let mut selected: Vec<(&str, Obj, &str, f64, Json)> = Vec::new();
    for (event, name, field_name) in [
        ("jump", "hero_jump", "jump"),
        ("land", "hero_land_soft", "softLanding"),
        ("hurt", "hero_damage_less_harsh", "takeHit"),
        ("hard_land", "hero_land_hard mono test", "hardLanding"),
        ("footsteps_run", "hero_run_footsteps_stone", "footStepsRun"),
    ] {
        let src = c.deref(&controller, get(&hero_audio, field_name)?)?;
        let tree = c.read(&src)?;
        let pitch = jfloat(&tree, "m_Pitch")?;
        if matches!(event, "hard_land" | "footsteps_run")
            && (get(&tree, "Loop")?.truthy() || pitch != 1.0)
        {
            return err("movement audio loop/pitch contract changed");
        }
        // Unity 6 moved the resource reference; the deprecated clip is null.
        let clip = c.deref(&src, get(&tree, "m_Resource")?)?;
        let mut origin = vec![
            ("component", Json::Str(controller.sid())),
            ("field", js(field_name)),
            ("audio_source", Json::Str(src.sid())),
            ("resource_field", js("m_Resource")),
            ("method", js("HeroAudioController.PlaySound")),
        ];
        if event == "footsteps_run" {
            origin.extend([
                ("environment_type", Json::Int(0)),
                ("environment_field", js("HeroController.footstepsRunDust")),
                ("hero_source_sha256", js(MOVEMENT_HERO_SHA256)),
                ("selection", js("Runtime-verified Dust mapping equals default run source; complete nonlooping sequence")),
            ]);
        }
        selected.push((event, clip, name, jfloat(&tree, "m_Volume")?, jobj(origin)));
    }
    let nail = resources(24289)?;
    c.expect_type(&nail, "NailSlash", "source NailSlash changed")?;
    let go = c.read(&c.deref(&nail, get(&c.read(&nail)?, "m_GameObject")?)?)?;
    let mut sources = Vec::new();
    for comp in get(&go, "m_Component")?.list().unwrap_or(&[]) {
        sources.push(c.deref(&nail, get(comp, "component")?)?);
    }
    let source_obj = sources
        .into_iter()
        .find(|o| o.class_id() == 82)
        .ok_or("NailSlash has no AudioSource")?;
    let tree = c.read(&source_obj)?;
    selected.push((
        "nail",
        c.deref(&source_obj, get(&tree, "m_Resource")?)?,
        "sword_3",
        jfloat(&tree, "m_Volume")?,
        jobj(vec![
            ("component", Json::Str(nail.sid())),
            ("audio_source", Json::Str(source_obj.sid())),
            ("method", js("NailSlash.StartSlash")),
            ("selection", js("authored normal Slash voice")),
        ]),
    ));
    let infected = c.object("level6", 12567)?;
    c.expect_type(
        &infected,
        "InfectedEnemyEffects",
        "source first Crawler hit effects changed",
    )?;
    let impact = get(&c.read(&infected)?, "impactAudio")?.clone();
    selected.push((
        "enemy_hit",
        c.deref(&infected, get(&impact, "Clip")?)?,
        "enemy_damage",
        jfloat(&impact, "Volume")?,
        jobj(vec![
            ("component", Json::Str(infected.sid())),
            ("field", js("impactAudio")),
            ("method", js("InfectedEnemyEffects.RecieveHitEffect")),
            (
                "source_pitch_range",
                pair(jfloat(&impact, "PitchMin")?, jfloat(&impact, "PitchMax")?),
            ),
        ]),
    ));
    selected.push((
        "door",
        c.object("sharedassets6.assets", 92)?,
        "breakable_wall_hit_1",
        1.0,
        jobj(vec![
            ("method", js("Breakable.Break")),
            ("selection", js("existing opening-door clip")),
        ]),
    ));
    let pick = |event: &str| selected.iter().find(|s| s.0 == event).unwrap();

    // One allocation over everything the window holds: the hero events at the
    // rate each shipped at as their ceiling, the boss voices, the Knight's
    // extra one-shots, with the menu clips' fixed bytes taken off the budget first.
    let menu_bytes = 2560;
    let mut rows: Vec<(String, Vec<u8>, i64)> = Vec::new();
    for e in EVENTS {
        let s = pick(e);
        rows.push((
            format!("hero {e}"),
            c.source_wav(&s.1, s.2)?.1,
            if e == "footsteps_run" { 11025 } else { 22050 },
        ));
    }
    for b in &BOSS_EVENTS {
        rows.push((
            format!("boss {}", b.event),
            c.source_wav(&c.object(b.file, b.path_id)?, b.name)?.1,
            b.rate,
        ));
    }
    for e in &HERO_EXTRA {
        rows.push((
            format!("extra {}", e.0),
            c.source_wav(&c.object(e.1, e.2)?, e.3)?.1,
            22050,
        ));
    }
    let rates = c.allocate_rates(&rows, BANK_LIMIT - menu_bytes)?;
    let rate_of = |key: &str| {
        rows.iter()
            .position(|r| r.0 == key)
            .map(|i| rates[i])
            .unwrap()
    };
    println!(
        "Hero SFX rates (SDK allocator): {}",
        rows.iter()
            .zip(&rates)
            .map(|(r, rate)| format!("{} {rate}", r.0.split_once(' ').unwrap().1))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let mut items = Vec::new();
    for e in EVENTS {
        let (_, clip, name, volume, origin) = pick(e);
        items.push((
            e.to_string(),
            c.cook(
                clip,
                name,
                *volume,
                rate_of(&format!("hero {e}")),
                origin.clone(),
                Resampler::Sdk,
            )?,
        ));
    }

    // The False Knight lives in level48, the additive boss scene; its
    // FalseyControl is read directly, because the clip PPtrs are the file's own.
    let boss_scene = u(source.file("level48"))?;
    let mut controls = Vec::new();
    for info in boss_scene.objects.iter().filter(|i| i.class_id == 114) {
        let o = Obj {
            file: boss_scene.clone(),
            info: *info,
        };
        if u(source.typename(&o))? == "PlayMakerFSM"
            && get(&c.read(&o)?, "fsm")?
                .get("name")
                .and_then(Value::str)
                .as_deref()
                == Some("FalseyControl")
        {
            controls.push(o);
        }
    }
    if controls.len() != 1 {
        return err("expected one FalseyControl in the boss scene");
    }
    let control = controls.remove(0);
    let boss_tree = c.read(&control)?;
    let resolve = |r: &Value| -> Result<String> { Ok(u(source.deref(&boss_scene, r))?.sid()) };
    let boss_bindings = false_knight_audio_contract(get(&boss_tree, "fsm")?, &resolve)?;
    let mut boss_items = Vec::new();
    for b in &BOSS_EVENTS {
        let clip = c.object(b.file, b.path_id)?;
        let rate = rate_of(&format!("boss {}", b.event));
        let origin = jobj(vec![
            ("component", Json::Str(control.sid())),
            (
                "states",
                Json::List(b.states.iter().map(|s| js(s)).collect()),
            ),
            ("action", js("AudioPlaySimple")),
            (
                "method",
                Json::Str(format!("FalseyControl.{}", b.states.join("/"))),
            ),
        ]);
        let cooked = c.cook(&clip, b.name, 1.0, rate, origin, Resampler::Sdk)?;
        // The refusal table is sized rather than cooked, so prove the estimator
        // against a clip that went through the real path before trusting it.
        let predicted = encoded_bytes(cooked.source_frames, cooked.source_rate, rate)?;
        if predicted != cooked.encoded.len() as i64 {
            return err(format!(
                "{}: predicted {predicted} encoded bytes, cooked {}",
                b.name,
                cooked.encoded.len()
            ));
        }
        boss_items.push((b.event.to_string(), cooked));
    }
    let mut extra_items = Vec::new();
    for (event, file, path_id, name, volume, place) in HERO_EXTRA {
        let clip = c.object(file, path_id)?;
        extra_items.push((
            event.to_string(),
            c.cook(
                &clip,
                name,
                volume,
                rate_of(&format!("extra {event}")),
                jobj(vec![("where", js(place))]),
                Resampler::Sdk,
            )?,
        ));
    }
    let (boss_n, extra_n) = (boss_items.len(), extra_items.len());
    items.extend(boss_items);
    items.extend(extra_items);
    let (bank, records) = pack_bank(items, boss_n, extra_n, BANK_LIMIT)?;

    // What the fight asked for and this bank could not hold, in bytes.
    let free = BANK_LIMIT - bank.len() as i64;
    let mut refused = Vec::new();
    let mut fitting = Vec::new();
    for (file, path_id, name, states) in BOSS_REFUSED {
        let clip = c.object(file, path_id)?;
        let (_, wav) = c.source_wav(&clip, name)?;
        let (frames, source_rate) = wav_frames(&wav)?;
        let sizes: Vec<(i64, i64)> = [22050, 11025, 8000]
            .iter()
            .map(|&r| Ok((r, encoded_bytes(frames, source_rate, r)?)))
            .collect::<Result<_>>()?;
        let fits: Vec<String> = sizes
            .iter()
            .filter(|(_, s)| *s <= free)
            .map(|(r, _)| r.to_string())
            .collect();
        if !fits.is_empty() {
            let python_dict = format!(
                "{{{}}}",
                sizes
                    .iter()
                    .map(|(r, s)| format!("'{r}': {s}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            fitting.push(format!(
                "  note: {name} would now fit at {} Hz ({python_dict}) against {free} free",
                fits.join("/")
            ));
        }
        refused.push(jobj(vec![
            ("source", Json::Str(format!("{file}:{path_id}"))),
            ("name", js(name)),
            ("states", js(states)),
            ("source_frames", Json::Int(frames)),
            ("source_rate", Json::Int(source_rate)),
            (
                "encoded_bytes",
                Json::Obj(
                    sizes
                        .iter()
                        .map(|(r, s)| (r.to_string(), Json::Int(*s)))
                        .collect(),
                ),
            ),
            ("free_bytes", Json::Int(free)),
            (
                "fits_at",
                Json::List(fits.iter().map(|f| Json::Str(f.clone())).collect()),
            ),
        ]));
    }
    let admitted: HashSet<&str> = BOSS_EVENTS.iter().map(|b| b.name).collect();
    let both: Vec<&str> = {
        let mut v: Vec<&str> = BOSS_REFUSED
            .iter()
            .map(|r| r.2)
            .filter(|n| admitted.contains(n))
            .collect();
        v.sort_unstable();
        v
    };
    if !both.is_empty() {
        return err(format!(
            "a clip is both admitted and refused: {}",
            both.join(", ")
        ));
    }
    println!(
        "False Knight: {boss_n} voices admitted, {} refused, {free} bank bytes free",
        refused.len()
    );
    for note in &fitting {
        println!("{note}");
    }

    let menu = resources(MENU_AUDIO_CONTROLLER)?;
    let menu_audio = c.read(&menu)?;
    c.expect_type(&menu, "MenuAudioController", "MenuAudioController moved")?;
    for (field_name, name) in UI_WORLD {
        let clip = c.read(&c.deref(&menu, get(&menu_audio, field_name)?)?)?;
        if clip.get("m_Name").and_then(Value::str).as_deref() != Some(name) {
            return err(format!("changed menu clip mapping: {field_name}"));
        }
    }
    let mut ui_items = Vec::new();
    for (field_name, name, rate) in UI_EVENTS {
        let clip = c.deref(&menu, get(&menu_audio, field_name)?)?;
        let cooked = c.cook(
            &clip,
            name,
            1.0,
            rate,
            jobj(vec![
                ("component", Json::Str(menu.sid())),
                ("field", js(field_name)),
            ]),
            Resampler::Ffmpeg,
        )?;
        ui_items.push((field_name, cooked));
    }
    let ui_blob: Vec<u8> = ui_items
        .iter()
        .flat_map(|(_, c)| c.encoded.clone())
        .collect();
    if bank.len() + ui_blob.len() > BANK_LIMIT as usize {
        return err(format!(
            "menu clips {} do not fit the {} bytes above the SFX bank",
            ui_blob.len(),
            BANK_LIMIT - bank.len() as i64
        ));
    }
    let write = |path: &str, bytes: &[u8]| {
        std::fs::write(root.join(path), bytes).map_err(|e| format!("{path}: {e}"))
    };
    write("data/ui_sfx.adpcm", &ui_blob)?;
    println!(
        "Menu clips: {} bytes above the SFX bank, {} left",
        ui_blob.len(),
        BANK_LIMIT - bank.len() as i64 - ui_blob.len() as i64
    );

    let great_door = c.object("level6", 12139)?;
    let great_door_pitch =
        great_door_hit_contract(get(&c.read(&great_door)?, "fsm")?, rate_of("hero door"))?;
    write("data/sfx.adpcm", &bank)?;
    // Preserve the existing artifact/API for diagnostics and old build helpers.
    let door = records.iter().find(|r| r.event == "door").unwrap();
    write(
        "data/door.adpcm",
        &bank[door.offset as usize..(door.offset + door.bytes) as usize],
    )?;
    let rows_of = |wanted: &[&str]| -> String {
        records
            .iter()
            .filter(|r| wanted.contains(&r.event.as_str()))
            .map(|r| {
                format!(
                    "    ({},{},{}), // {}\n",
                    SPU_BASE + r.offset,
                    r.sample_rate,
                    r.volume_q14,
                    r.event
                )
            })
            .collect()
    };
    let mut text = format!("pub const BANK_BYTES: usize = {};\n", bank.len());
    text += &format!("pub const BANK_CHECKSUM: u32 = {};\n", fnv(&bank));
    text += &format!(
        "pub const SAMPLES: [(u32,u32,i16);{}] = [\n{}];\n",
        EVENTS.len(),
        rows_of(&EVENTS)
    );
    let boss: Vec<&str> = BOSS_EVENTS.iter().map(|b| b.event).collect();
    text += &format!(
        "/// False Knight one-shots, on the shared voice: {}.\n",
        boss.join(", ")
    );
    text += &format!(
        "pub const BOSS_SAMPLES: [(u32,u32,i16);{}] = [\n{}];\n",
        boss.len(),
        rows_of(&boss)
    );
    let extra: Vec<&str> = HERO_EXTRA.iter().map(|e| e.0).collect();
    text += &format!(
        "/// The Knight's other one-shots, played by reconfiguring a hero voice: {}.\n",
        extra.join(", ")
    );
    text += &format!(
        "pub const EXTRA_SAMPLES: [(u32,u32,i16);{}] = [\n{}];\n",
        extra.len(),
        rows_of(&extra)
    );
    text += &format!(
        "pub const GREAT_DOOR_HIT_PITCH:[u16;2]=[{}, {}];\n",
        great_door_pitch[0], great_door_pitch[1]
    );
    let (mut ui_rows, mut offset) = (String::new(), SPU_BASE + bank.len() as i64);
    for (field_name, cooked) in &ui_items {
        ui_rows += &format!(
            "    ({offset},{},{}), // {field_name}\n",
            cooked.sample_rate, cooked.volume_q14
        );
        offset += cooked.encoded.len() as i64;
    }
    text += &format!("/// Menu clips (data/ui_sfx.adpcm), uploaded at SPU {} before the title: select, slider.\n", SPU_BASE + bank.len() as i64);
    text += &format!("pub const UI_BYTES: usize = {};\n", ui_blob.len());
    text += &format!(
        "pub const UI_SAMPLES: [(u32,u32,i16);{}] = [\n{ui_rows}];\n",
        ui_items.len()
    );
    // Complete nonlooping sequences restart on the first 60 Hz tick after end.
    for (event, constant) in [
        ("land", "SOFT_LANDING_TICKS"),
        ("footsteps_run", "RUN_SEQUENCE_TICKS"),
    ] {
        let r = records.iter().find(|r| r.event == event).unwrap();
        text += &format!(
            "pub const {constant}:u16={};\n",
            (r.samples * 60 + r.sample_rate - 1) / r.sample_rate
        );
    }
    // Read the validated serialized scalar prefix of HeroController, before the unresolved tail.
    let hero = resources(20602)?;
    c.expect_type(&hero, "HeroController", "HeroController source changed")?;
    let mut node = (*u(source.mono_node(&hero))?).clone();
    let cut = node
        .children
        .iter()
        .position(|n| &*n.name == "hero_state")
        .ok_or("HeroController has no hero_state")?;
    node.children.truncate(cut);
    let constants = u(Reader {
        c: Cursor::new(u(hero.raw())?, hero.file.big_endian),
        flavor: Flavor::Python,
    }
    .read(&node))?;
    let big_fall = jfloat(&constants, "BIG_FALL_TIME")?;
    if (big_fall - 1.1).abs() > 1e-6 {
        return err("hard landing fall duration changed");
    }
    let hard_ticks = (big_fall * 60.0).floor() as i64 + 1;
    text += &format!("pub const HARD_FALL_MIN_TICKS:u16={hard_ticks};\n");
    write("data/sfx.rs", text.as_bytes())?;

    // The original death FSM starts both layers together. Preserve full clips
    // separately for future CD->SPU boot loading; neither enters the current EXE.
    let death_fsm = resources(24938)?;
    let death_tree = c.read(&death_fsm)?;
    let start = get(&death_tree, "fsm")?
        .get("states")
        .and_then(Value::list)
        .and_then(|l| {
            l.iter()
                .find(|s| s.get("name").and_then(Value::str).as_deref() == Some("Start"))
        })
        .ok_or("death FSM has no Start state")?;
    let layers = death_action_layers(get(start, "actionData")?)?;
    let death_dir = root.join(".hkpsx/audio-extra");
    std::fs::create_dir_all(&death_dir).map_err(|e| e.to_string())?;
    let mut deferred = Vec::new();
    for (layer, name) in layers.iter().zip(["hero_death_v2", "hero_damage"]) {
        let clip = c.deref(&controller, &layer.clip)?;
        let origin = jobj(vec![
            ("component", Json::Str(death_fsm.sid())),
            ("state", js("Start")),
            ("action", js("AudioPlayerOneShotSingle")),
            ("action_index", Json::Int(layer.action_index as i64)),
            ("pitch", pair(layer.pitch[0], layer.pitch[1])),
            ("delay", Json::Float(layer.delay)),
        ]);
        let mut cooked = c.cook(&clip, name, layer.volume, 22050, origin, Resampler::Ffmpeg)?;
        let path = death_dir.join(format!("{name}.adpcm"));
        std::fs::write(&path, &cooked.encoded).map_err(|e| e.to_string())?;
        let bytes = cooked.encoded.len() as i64;
        cooked.meta(
            "path",
            Json::Str(format!(".hkpsx/audio-extra/{name}.adpcm")),
        );
        cooked.meta("bytes", Json::Int(bytes));
        cooked.meta("runtime_integrated", Json::Bool(false));
        deferred.push(Json::Obj(cooked.meta));
    }

    // The world bank. Every clip is resolved through the component that plays
    // it, so a patch that rewires one fails here instead of shipping the old one.
    let level6 = u(source.file("level6"))?;
    let effects = u(source.object(&level6, ENEMY_DEATH_EFFECTS))?;
    let cocoon = u(source.object(&level6, HEALTH_COCOON))?;
    if u(source.typename(&effects))? != "EnemyDeathEffects"
        || u(source.typename(&cocoon))? != "HealthCocoon"
    {
        return err("source enemy death or cocoon component moved");
    }
    let sword = get(&c.read(&effects)?, "enemyDeathSwordAudio")?.clone();
    let cocoon_tree = c.read(&cocoon)?;
    let cocoon_go = c.read(&c.deref(&cocoon, get(&cocoon_tree, "m_GameObject")?)?)?;
    let mut cocoon_sources = Vec::new();
    let mut component_objects = Vec::new();
    for comp in get(&cocoon_go, "m_Component")?.list().unwrap_or(&[]) {
        component_objects.push(c.deref(&cocoon, get(comp, "component")?)?);
    }
    for o in component_objects.iter().filter(|o| o.class_id() == 82) {
        cocoon_sources.push(c.read(o)?);
    }
    if cocoon_sources.len() != 1 {
        return err("Health Cocoon no longer has exactly one AudioSource");
    }
    let menu_origin = |f: &str| jobj(vec![("component", Json::Str(menu.sid())), ("field", js(f))]);
    let one = |v: f64| (v, v);
    let world_sources: Vec<(Obj, f64, (f64, f64), Json)> = vec![
        (
            c.deref(&effects, get(&sword, "Clip")?)?,
            jfloat(&sword, "Volume")?,
            (jfloat(&sword, "PitchMin")?, jfloat(&sword, "PitchMax")?),
            jobj(vec![("component", Json::Str(effects.sid())), ("field", js("enemyDeathSwordAudio")), ("method", js("EnemyDeathEffects.EmitSound"))]),
        ),
        (
            c.deref(&controller, &layers[0].clip)?,
            layers[0].volume,
            (layers[0].pitch[0], layers[0].pitch[1]),
            jobj(vec![
                ("component", Json::Str(death_fsm.sid())),
                ("state", js("Start")),
                ("action_index", Json::Int(layers[0].action_index as i64)),
                ("note", js("the second layer, hero_damage, is stood in for by the hurt clip on voice 4")),
            ]),
        ),
        (c.deref(&menu, get(&menu_audio, "submit")?)?, 1.0, one(1.0), menu_origin("submit, cancel")),
        (c.deref(&menu, get(&menu_audio, "startGame")?)?, 1.0, one(1.0), menu_origin("startGame")),
        (
            c.deref(&cocoon, get(&cocoon_tree, "deathSound")?)?,
            jfloat(&cocoon_sources[0], "m_Volume")?,
            one(jfloat(&cocoon_sources[0], "m_Pitch")?),
            jobj(vec![("component", Json::Str(cocoon.sid())), ("field", js("deathSound")), ("method", js("HealthCocoon.PlaySound"))]),
        ),
    ];
    let mut world_items = Vec::new();
    for ((event, place, name, rate), (clip, volume, pitch, origin)) in
        WORLD_EVENTS.iter().zip(world_sources)
    {
        if volume != 1.0 {
            return err(format!(
                "{name}: source gain {} is not the shared voice gain",
                crate::pyfloat::repr(volume)
            ));
        }
        let origin = match origin {
            Json::Obj(mut f) => {
                f.push(("where".into(), js(place)));
                Json::Obj(f)
            }
            _ => unreachable!(),
        };
        let mut cooked = c.cook(&clip, name, volume, *rate, origin, Resampler::Ffmpeg)?;
        if encoded_bytes(cooked.source_frames, cooked.source_rate, *rate)?
            != cooked.encoded.len() as i64
        {
            return err(format!("{name}: encoded size does not match the estimator"));
        }
        let register = [
            py_round((rate * 4096) as f64 / 44100.0 * pitch.0),
            py_round((rate * 4096) as f64 / 44100.0 * pitch.1),
        ];
        if !register.iter().all(|&r| 0 < r && r < 0x4000) {
            return err(format!("{name}: pitch register out of range"));
        }
        cooked.meta("source_pitch", pair(pitch.0, pitch.1));
        cooked.meta(
            "pitch_register",
            Json::List(register.iter().map(|&r| Json::Int(r)).collect()),
        );
        world_items.push((event.to_string(), cooked, register));
    }
    let registers: Vec<[i64; 2]> = world_items.iter().map(|i| i.2).collect();
    let (world, world_records) = pack(world_items.into_iter().map(|(e, c, _)| (e, c)).collect())?;
    let world_base = FOCUS_SPU_BASE - world.len() as i64;
    write("data/world-sfx.adpcm", &world)?;
    let mut text = String::from(
        "// Generated by host/cook_audio.py: the world one-shots, streamed before the title.\n",
    );
    text += &format!("pub const SPU_BASE: u32 = {world_base};\n");
    text += &format!("pub const BANK_BYTES: usize = {};\n", world.len());
    text += &format!("pub const BANK_CHECKSUM: u32 = {};\n", fnv(&world));
    text += &format!(
        "/// (SPU address, rate, gain, pitch register min, max): {}.\n",
        world_records
            .iter()
            .map(|r| r.event.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    text += &format!(
        "pub const SAMPLES: [(u32,u32,i16,u16,u16);{}] = [\n",
        world_records.len()
    );
    for (r, reg) in world_records.iter().zip(&registers) {
        text += &format!(
            "    ({},{},{},{},{}), // {}\n",
            world_base + r.offset,
            r.sample_rate,
            r.volume_q14,
            reg[0],
            reg[1],
            r.event
        );
    }
    text += "];\n";
    write("data/world-sfx.rs", text.as_bytes())?;
    println!(
        "World SFX: {} bytes at {world_base:#x}..{FOCUS_SPU_BASE:#x}, {}",
        world.len(),
        world_records
            .iter()
            .map(|r| format!("{} {}B@{}", r.name, r.bytes, r.sample_rate))
            .collect::<Vec<_>>()
            .join(", ")
    );
    for name in ["level6", "Managed/Assembly-CSharp.dll", "level48"] {
        let bytes =
            std::fs::read(source.directory.join(name)).map_err(|e| format!("{name}: {e}"))?;
        let entry = jobj(vec![
            ("bytes", Json::Int(bytes.len() as i64)),
            ("sha256", Json::Str(sha(&bytes))),
        ]);
        match c.inputs.iter_mut().find(|(k, _)| k == name) {
            Some(slot) => slot.1 = entry,
            None => c.inputs.push((name.to_string(), entry)),
        }
    }

    let events: Vec<Json> = records.iter().map(|r| Json::Obj(r.json.clone())).collect();
    let world_events: Vec<Json> = world_records
        .iter()
        .map(|r| Json::Obj(r.json.clone()))
        .collect();
    let provenance = jobj(vec![
        ("environment_omissions", Json::List(omissions)),
        ("source", Json::Str(source.directory.display().to_string())),
        ("inputs", Json::Obj(c.inputs.clone())),
        ("generator_sha256", Json::Str(sha(include_bytes!("cook_audio.rs")))),
        ("spu_base", Json::Int(SPU_BASE)),
        ("spu_bytes", Json::Int(bank.len() as i64)),
        ("resident_bank_limit", Json::Int(BANK_LIMIT)),
        ("output_sha256", Json::Str(sha(&bank))),
        ("events", Json::List(events)),
        ("deferred_death_layers", Json::List(deferred)),
        (
            "world_sfx",
            jobj(vec![
                ("spu_base", Json::Int(world_base)),
                ("spu_end", Json::Int(world_base + world.len() as i64)),
                ("bytes", Json::Int(world.len() as i64)),
                ("output_sha256", Json::Str(sha(&world))),
                ("voice", Json::Int(15)),
                ("events", Json::List(world_events)),
                ("transport", js("WORLD.PAK chunk read before the title screen; the bootstrap retries it if that read failed")),
                (
                    "limitations",
                    Json::List(
                        [
                            "All five share voice 15 with the Great Door, the False Knight and the menu clips; a new play retriggers over whatever that voice was playing.",
                            "The death FSM also starts hero_damage with hero_death_v2; the hurt clip already playing on voice 4 (hero_damage_less_harsh) stands in for it.",
                            "Rates below the category rates are chosen from measured energy above the new Nyquist; see WORLD_EVENTS.",
                        ]
                        .iter()
                        .map(|s| js(s))
                        .collect(),
                    ),
                ),
            ]),
        ),
        (
            "great_door_hit",
            jobj(vec![
                ("component", Json::Str(great_door.sid())),
                ("state", js("Hit")),
                ("resident_variant", js("sharedassets6.assets:92")),
                ("voice", Json::Int(15)),
                ("pitch_register_bounds", Json::List(great_door_pitch.iter().map(|&p| Json::Int(p)).collect())),
                ("missing_variants", Json::List(vec![js("sharedassets6.assets:99"), js("sharedassets6.assets:102")])),
            ]),
        ),
        (
            "false_knight",
            jobj(vec![
                ("component", Json::Str(control.sid())),
                ("scene", js("level48")),
                ("admitted", Json::List(boss_bindings)),
                ("refused", Json::List(refused)),
                // Voice 15 is shared with the Great Door: every one of the 24 SPU
                // voices is allocated, and no scene holds both the door
                // (Tutorial_01) and the fight (Crossroads_10).
                ("voice", Json::Int(15)),
                ("bank_free_bytes", Json::Int(free)),
                (
                    "limitations",
                    Json::List(
                        [
                            "The resident bank is bounded by the Geo bank at 0x14000; clips every scene needs go to the world bank instead, and a per-scene bank does not exist yet.",
                            "false_knight_strike_ground, the slam impact itself, is refused at every rate this port uses; the slam keeps its BigShake and its swing and has no boom.",
                            "`Stun Land` plays false_knight_land through AudioPlayerOneShotSingle at pitch 1.15 rather than AudioPlaySimple at 1.0; the pitched variant is not admitted.",
                            "The boss shares one voice, so a swing retriggers over a landing tail the way every other event class here does, rather than mixing as the source does.",
                        ]
                        .iter()
                        .map(|s| js(s))
                        .collect(),
                    ),
                ),
            ]),
        ),
        (
            "movement_audio",
            jobj(vec![
                ("hard_fall_min_ticks", Json::Int(hard_ticks)),
                ("hard_fall_seconds", Json::Float(big_fall)),
                ("simulation_hz", Json::Int(60)),
                ("short_sfx_rate", js("SDK allocator per clip (see events)")),
                ("long_movement_rate", Json::Int(11025)),
                (
                    "methods",
                    Json::List(["HeroController.FallCheck", "HeroController.ShouldHardLand", "HeroController.DoHardLanding", "HeroAudioController.PlaySound"].iter().map(|s| js(s)).collect()),
                ),
                (
                    "limitations",
                    Json::List(
                        [
                            "Hard landing audio only; original0.8s recovery/animation not introduced here",
                            "Running Dust sequence only; walk-zone and other environment sequences remain unsupported",
                            "AudioSource isPlaying resampled to bounded60Hz clip-duration counters",
                            "Pause stops/restarts the footstep sequence rather than resuming its exact sample cursor",
                        ]
                        .iter()
                        .map(|s| js(s))
                        .collect(),
                    ),
                ),
            ]),
        ),
        (
            "limitations",
            Json::List(
                [
                    "Mono 11025 Hz hero SFX resampled by the SDK shared resampler (halved from 22050 Hz); SDK psx-audio-cook ADPCM",
                    "Ordinary SFX use pitch1.0; Great Door hit uses source pitch bounds with deterministic guest RNG",
                    "Nail uses one authored Slash sound for all supported directions",
                    "Unity mixer/positional effects omitted; source voice volumes scaled1/3 for mix headroom",
                    "Full-rate death layers are cooked separately for reference; the guest plays hero_death_v2 from the world bank at 4000 Hz",
                ]
                .iter()
                .map(|s| js(s))
                .collect(),
            ),
        ),
    ]);
    std::fs::write(
        root.join(".hkpsx/audio-provenance.json"),
        dumps(&provenance),
    )
    .map_err(|e| e.to_string())?;
    println!(
        "Resident SFX: {} bytes; {} events; full death layers deferred",
        bank.len(),
        records.len()
    );
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
pub(crate) mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    pub(crate) fn int(i: i64) -> Value {
        Value::Int(i)
    }
    pub(crate) fn list(v: Vec<Value>) -> Value {
        Value::List(v)
    }
    pub(crate) fn text(s: &str) -> Value {
        Value::Str(s.as_bytes().to_vec())
    }
    pub(crate) fn map(fields: Vec<(&str, Value)>) -> Value {
        Value::Map(fields.into_iter().map(|(k, v)| (k.into(), v)).collect())
    }
    pub(crate) fn bytes_of(v: &[u8]) -> Value {
        list(v.iter().map(|&b| int(b as i64)).collect())
    }
    fn scalar(v: f32) -> Vec<u8> {
        let mut b = v.to_le_bytes().to_vec();
        b.push(0);
        b
    }

    /// The SDK encoder, built once for every test that needs it and shared
    /// (its scratch files are one set).
    pub(crate) fn tool() -> MutexGuard<'static, Tool> {
        static TOOL: OnceLock<Mutex<Tool>> = OnceLock::new();
        TOOL.get_or_init(|| {
            Mutex::new(
                Tool::build(
                    &Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
                    "hk-cook-test",
                )
                .expect("psx-audio-cook builds"),
            )
        })
        .lock()
        .unwrap_or_else(|e| e.into_inner())
    }
    fn encode(samples: &[i16]) -> Vec<u8> {
        tool().encode_oneshot(samples).unwrap()
    }
    fn wav(samples: &[i16], channels: u16, rate: u32) -> Vec<u8> {
        let mut out = Vec::new();
        let data_len = samples.len() * 2;
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * 2 * channels as u32).to_le_bytes());
        out.extend_from_slice(&(2 * channels).to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data_len as u32).to_le_bytes());
        for s in samples {
            out.extend_from_slice(&s.to_le_bytes());
        }
        out
    }
    fn converted(data: &[u8], rate: i64) -> Result<(Vec<i16>, Vec<(String, Json)>)> {
        convert_wav(&tool(), data, rate, Resampler::Ffmpeg)
    }
    fn meta_int(meta: &[(String, Json)], key: &str) -> i64 {
        match meta.iter().find(|(k, _)| k == key) {
            Some((_, Json::Int(i))) => *i,
            other => panic!("no int {key}: {:?}", other.map(|o| &o.1)),
        }
    }

    // The two clips BOSS_EVENTS admits, keyed by the path id the fixture FSM cites.
    fn boss_clip(r: &Value) -> Result<String> {
        match r.get("m_PathID").and_then(Value::int) {
            Some(1) => Ok("sharedassets32.assets:131".into()),
            Some(2) => Ok("sharedassets48.assets:39".into()),
            other => err(format!("unknown clip {other:?}")),
        }
    }
    /// One FalseyControl state whose single enabled AudioPlaySimple plays a clip.
    fn audio_state(name: &str, path_id: i64, enabled: i64) -> Value {
        map(vec![
            ("name", text(name)),
            (
                "actionData",
                map(vec![
                    (
                        "actionNames",
                        list(vec![text("AudioPlaySimple.AudioPlaySimple")]),
                    ),
                    ("actionEnabled", list(vec![int(enabled)])),
                    ("actionStartIndex", list(vec![int(0)])),
                    ("paramName", list(vec![text("oneShotClip"), text("volume")])),
                    ("paramDataPos", list(vec![int(0), int(0)])),
                    ("paramDataType", list(vec![int(24), int(15)])),
                    ("paramByteDataSize", list(vec![int(0), int(5)])),
                    ("byteData", bytes_of(&scalar(1.0))),
                    (
                        "fsmObjectParams",
                        list(vec![map(vec![
                            ("typeName", text("UnityEngine.AudioClip")),
                            ("useVariable", int(0)),
                            (
                                "value",
                                map(vec![("m_FileID", int(10)), ("m_PathID", int(path_id))]),
                            ),
                        ])]),
                    ),
                ]),
            ),
        ])
    }
    fn false_knight_fsm(name: &str, skip_first: bool, disable_first: bool) -> Value {
        let mut states = Vec::new();
        for (i, s) in ["S Land", "State 2", "Land Noise"].iter().enumerate() {
            states.push(audio_state(
                s,
                1,
                if disable_first && i == 0 { 0 } else { 1 },
            ));
        }
        for s in ["S Attack", "JA Hit 2"] {
            states.push(audio_state(s, 2, 1));
        }
        if skip_first {
            states.remove(0);
        }
        map(vec![("name", text(name)), ("states", list(states))])
    }

    fn death_actions(dynamic: bool, disable_second: bool) -> Value {
        let (mut names, mut pos, mut kinds, mut sizes, mut data, mut objects) =
            (vec![], vec![], vec![], vec![], vec![], vec![]);
        for layer in 0..2 {
            names.push(text("audioClip"));
            pos.push(int(layer));
            kinds.push(int(24));
            sizes.push(int(0));
            objects.push(map(vec![
                ("typeName", text("UnityEngine.AudioClip")),
                ("useVariable", int((dynamic && layer == 0) as i64)),
                (
                    "value",
                    map(vec![("m_FileID", int(0)), ("m_PathID", int(layer + 1))]),
                ),
            ]));
            for (name, value) in [
                ("pitchMin", 1.0),
                ("pitchMax", 1.0),
                ("volume", 1.0),
                ("delay", 0.0),
            ] {
                names.push(text(name));
                pos.push(int(data.len() as i64));
                kinds.push(int(15));
                sizes.push(int(5));
                data.extend(scalar(value));
            }
        }
        map(vec![
            (
                "actionNames",
                list(vec![
                    text(
                        "AudioPlayerOneShotSingle.AudioPlayerOneShotSingle"
                    );
                    2
                ]),
            ),
            (
                "actionEnabled",
                list(vec![int(1), int(if disable_second { 0 } else { 1 })]),
            ),
            ("actionStartIndex", list(vec![int(0), int(5)])),
            ("paramName", list(names)),
            ("paramDataPos", list(pos)),
            ("paramDataType", list(kinds)),
            ("paramByteDataSize", list(sizes)),
            ("byteData", bytes_of(&data)),
            ("fsmObjectParams", list(objects)),
        ])
    }

    #[test]
    fn death_mapping_retains_both_immediate_source_layers() {
        let layers = death_action_layers(&death_actions(false, false)).unwrap();
        assert_eq!(
            layers
                .iter()
                .map(|l| l.clip.get("m_PathID").and_then(Value::int))
                .collect::<Vec<_>>(),
            [Some(1), Some(2)]
        );
        assert_eq!(
            layers.iter().map(|l| l.delay).collect::<Vec<_>>(),
            [0.0, 0.0]
        );
    }

    #[test]
    fn death_mapping_rejects_dynamic_or_disabled_audio() {
        assert!(death_action_layers(&death_actions(true, false)).is_err());
        assert!(death_action_layers(&death_actions(false, true)).is_err());
    }

    #[test]
    fn terminal_block_is_silent_and_never_repeats() {
        let encoded = encode(&[1000; 29]);
        assert_eq!(encoded.len(), 48);
        assert_eq!(
            encoded[encoded.len() - 16..],
            [vec![12, 1], vec![0; 14]].concat()
        );
        let decoded = decode_oneshot(&encoded).unwrap();
        assert_eq!(decoded.len(), 56);
        assert!(decoded[29..].iter().all(|&x| x == 0));
    }

    #[test]
    fn signed_extrema_decode_without_overflow() {
        // Full-scale content at the Nyquist rate: the shared encoder band-
        // limits it, so only the decode's clamping is pinned here.
        let samples: Vec<i16> = [-32768, 32767, -4096, 4096]
            .iter()
            .copied()
            .cycle()
            .take(28)
            .collect();
        let decoded = decode_oneshot(&encode(&samples)).unwrap();
        assert_eq!(decoded.len(), 28);
        assert!(decoded.iter().all(|&x| (-32768..=32767).contains(&x)));
    }

    #[test]
    fn a_full_scale_tone_codes_closely() {
        let samples: Vec<i16> = (0..280)
            .map(|i| py_round(32000.0 * (i as f64 * 0.3).sin()) as i16)
            .collect();
        let decoded = decode_oneshot(&encode(&samples)).unwrap();
        let error: f64 = samples
            .iter()
            .zip(&decoded)
            .map(|(&a, &b)| (a as f64 - b as f64).powi(2))
            .sum();
        let signal: f64 = samples.iter().map(|&a| (a as f64).powi(2)).sum();
        assert!(10.0 * (signal / error).log10() > 20.0);
    }

    #[test]
    fn downmix_and_resample_preserve_duration_and_level() {
        // Real clip lengths, since the resampler primes its filter.
        let stereo: Vec<i16> = [1000, -1000, 3000, 1000, -1000, -3000]
            .iter()
            .copied()
            .cycle()
            .take(24000)
            .collect();
        let (pcm, meta) = converted(&wav(&stereo, 2, 44100), 22050).unwrap();
        assert_eq!(meta_int(&meta, "source_frames"), 12000);
        assert_eq!(meta_int(&meta, "source_channels"), 2);
        assert_eq!(pcm.len(), 6000);
        assert_eq!(meta_int(&meta, "samples"), 6000);
        // A constant survives the conversion exactly: no droop, no DC shift.
        let (flat, _) = converted(&wav(&vec![12345; 97600], 1, 48000), 11025).unwrap();
        assert_eq!(flat.len(), 22418);
        assert!(flat.iter().all(|&x| x == 12345));
    }

    #[test]
    fn resample_rejects_the_signal_above_the_new_nyquist() {
        let samples: Vec<i16> = [10000, -10000].iter().copied().cycle().take(4000).collect();
        let (pcm, _) = converted(&wav(&samples, 1, 44100), 11025).unwrap();
        // A real filter leaves a finite stopband residue; 40 dB down is the property that matters.
        assert!(pcm.iter().map(|&x| (x as i32).abs()).max().unwrap() < 1000);
    }

    #[test]
    fn empty_clip_has_no_invented_sample() {
        let (pcm, meta) = converted(&wav(&[], 1, 48000), 11025).unwrap();
        assert!(pcm.is_empty());
        assert_eq!(meta_int(&meta, "samples"), 0);
    }

    #[test]
    fn a_truncated_resample_is_rejected_rather_than_shipped() {
        // Shorter than the filter's priming window: must fail, not lose audio.
        assert!(converted(&wav(&[1000, -1000, 3000], 1, 44100), 22050).is_err());
    }

    #[test]
    fn bad_loop_flags_are_rejected() {
        let mut bank = encode(&[1000; 28]);
        bank[1] = 4;
        assert!(decode_oneshot(&bank).is_err());
    }

    fn full_items() -> Vec<(String, Cooked)> {
        let names: Vec<&str> = EVENTS
            .iter()
            .copied()
            .chain(BOSS_EVENTS.iter().map(|b| b.event))
            .chain(HERO_EXTRA.iter().map(|e| e.0))
            .collect();
        names
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let encoded = encode(&vec![1000; i + 1]);
                (
                    n.to_string(),
                    Cooked {
                        encoded,
                        meta: Vec::new(),
                        source_frames: 0,
                        source_rate: 0,
                        sample_rate: 22050,
                        samples: i as i64 + 1,
                        volume_q14: 5461,
                    },
                )
            })
            .collect()
    }

    #[test]
    fn budget_is_atomic_and_sample_starts_aligned() {
        let (bank, records) = pack_bank(
            full_items(),
            BOSS_EVENTS.len(),
            HERO_EXTRA.len(),
            BANK_LIMIT,
        )
        .unwrap();
        assert!(records.iter().all(|r| r.offset % 16 == 0));
        assert_eq!(
            records.iter().map(|r| r.bytes).sum::<i64>(),
            bank.len() as i64
        );
        // The hero events keep the first eight indices, which audio.rs binds
        // one-to-one to its dedicated voices.
        assert_eq!(
            records
                .iter()
                .take(8)
                .map(|r| r.event.as_str())
                .collect::<Vec<_>>(),
            EVENTS
        );
        assert!(pack_bank(
            full_items(),
            BOSS_EVENTS.len(),
            HERO_EXTRA.len(),
            bank.len() as i64 - 1
        )
        .is_err());
        let mut short = full_items();
        short.pop();
        assert!(pack_bank(short, BOSS_EVENTS.len(), HERO_EXTRA.len(), BANK_LIMIT).is_err());
        let mut swapped = full_items();
        swapped.swap(0, 1);
        assert!(pack_bank(swapped, BOSS_EVENTS.len(), HERO_EXTRA.len(), BANK_LIMIT).is_err());
        assert!(bank.len() as i64 <= BANK_LIMIT);
    }

    #[test]
    fn encoded_size_is_predicted_exactly_before_a_clip_is_cooked() {
        // The refusal table is sized rather than cooked, so the estimator has
        // to agree with the real path on every shape, including the exact
        // multiple of 28 and the one sample the resampler may round away.
        for (frames, source_rate, rate) in [
            (28, 44100, 44100),
            (29, 44100, 44100),
            (100000, 44100, 22050),
            (60543, 44100, 11025),
            (10884, 44100, 22050),
            (1, 44100, 8000),
        ] {
            let samples = py_round(div(frames * rate, source_rate)) as usize;
            assert_eq!(
                encoded_bytes(frames, source_rate, rate).unwrap(),
                encode(&vec![0; samples]).len() as i64
            );
        }
        assert!(encoded_bytes(0, 44100, 22050).is_err());
    }

    #[test]
    fn boss_contract_refuses_a_state_that_stopped_playing_its_clip() {
        let fsm = false_knight_fsm("FalseyControl", false, false);
        let bindings = false_knight_audio_contract(&fsm, &boss_clip).unwrap();
        assert_eq!(bindings.len(), BOSS_EVENTS.len());
        let Json::Obj(first) = &bindings[0] else {
            panic!()
        };
        assert_eq!(first[0], ("event".to_string(), js("boss_land")));
        assert!(false_knight_audio_contract(
            &false_knight_fsm("FalseyControl", true, false),
            &boss_clip
        )
        .is_err());
        assert!(false_knight_audio_contract(
            &false_knight_fsm("Something Else", false, false),
            &boss_clip
        )
        .is_err());
        assert!(false_knight_audio_contract(
            &false_knight_fsm("FalseyControl", false, true),
            &boss_clip
        )
        .is_err());
    }

    #[test]
    fn a_great_door_that_moved_its_pitch_is_refused() {
        let hit = |lo: f32, hi: f32, clips: [i64; 2]| {
            let mut bytes = scalar(lo);
            bytes.extend(scalar(hi));
            let pptr = |i: i64| map(vec![("m_FileID", int(2)), ("m_PathID", int(i))]);
            map(vec![
                ("name", text("Great Door")),
                (
                    "states",
                    list(vec![map(vec![
                        ("name", text("Hit")),
                        (
                            "actionData",
                            map(vec![
                                (
                                    "actionNames",
                                    list(vec![text(
                                        "HutongGames.PlayMaker.Actions.AudioPlayRandom",
                                    )]),
                                ),
                                ("actionEnabled", list(vec![int(1)])),
                                ("actionStartIndex", list(vec![int(0)])),
                                ("paramName", list(vec![text("pitchMin"), text("pitchMax")])),
                                ("paramDataType", list(vec![int(15), int(15)])),
                                ("paramDataPos", list(vec![int(0), int(5)])),
                                ("paramByteDataSize", list(vec![int(5), int(5)])),
                                ("byteData", bytes_of(&bytes)),
                                (
                                    "unityObjectParams",
                                    list(vec![pptr(clips[0]), pptr(clips[1])]),
                                ),
                            ]),
                        ),
                    ])]),
                ),
            ])
        };
        assert_eq!(
            great_door_hit_contract(&hit(0.85, 1.15, [92, 99]), 22050).unwrap(),
            [1741, 2355]
        );
        assert!(great_door_hit_contract(&hit(0.9, 1.15, [92, 99]), 22050).is_err());
        assert!(great_door_hit_contract(&hit(0.85, 1.15, [92, 102]), 22050).is_err());
    }

    #[test]
    fn slices_clamp_like_python() {
        assert_eq!(slice(&[1, 2, 3], 1, 5), [2, 3]);
        assert_eq!(slice(&[1, 2, 3], 7, 2), Vec::<u8>::new());
    }
}
