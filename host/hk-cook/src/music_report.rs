//! The source music and ambience inventory and its conversions: every scene's
//! music cues, regions and atmos cues, each resident atmos channel, and every
//! clip they play cooked at the rates the later steps read.
//! Ported from host/cook_music.py `main`, whose `.hkpsx/music/provenance.json`,
//! `capacity.json`, `source-methods.il` and per-clip conversions it reproduces
//! byte for byte (except the `tool_hashes`, which name the Rust modules).
//!
//! No guest build integration: complete clips remain ignored host assets.

use crate::common::{collider_polygons, components, err, get, path_id, Result};
use crate::cook_audio::{jobj, js, sha, u};
use crate::music::{cook_clip, dump, jget, rel, sha_file, value_json, Resampler};
use crate::pyfloat;
use crate::pyjson::Json;
use crate::spu::Tool;
use hk_dotnet::il::{self, Operand};
use hk_dotnet::{Assembly, Table};
use hk_unity::scene::Scene;
use hk_unity::{Obj, Source, Value};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

/// Atmos channels with a resident guest loop. This tuple is the whole decision:
/// host/ambience.py derives the clip table length, the cue gain arrays, the SPU
/// voices and the streamed clip's index from it, and the guest derives its own
/// half from the cooked table. (The measurements behind the choice are the
/// comment above `RESIDENT_ATMOS_CHANNELS` in the retired host/cook_music.py;
/// they are kept in docs/MUSIC.md.)
pub const RESIDENT_ATMOS_CHANNELS: [i64; 8] = [0, 1, 4, 5, 7, 9, 10, 15];
/// Cook rate per resident channel unless RESIDENT_ATMOS_RATES names one.
pub const DEFAULT_ATMOS_RATE: i64 = 4000;
/// The cook rate for one resident atmos channel (no channel earns its own today).
pub fn atmos_rate(_channel: i64) -> i64 {
    DEFAULT_ATMOS_RATE
}

fn truthy(v: &Value, key: &str) -> Result<bool> {
    Ok(get(v, key)?.truthy())
}
fn has_path(v: &Value, key: &str) -> Result<bool> {
    Ok(path_id(get(v, key)?).unwrap_or(0) != 0)
}

/// `source_audio_ref`: Unity 6 uses AudioResource while legacy m_audioClip is often null.
pub fn source_audio_ref(tree: &Value) -> Result<&Value> {
    for name in ["m_Resource", "m_audioClip"] {
        if let Some(v) = tree.get(name) {
            if v.is_map() && path_id(v).unwrap_or(0) != 0 {
                return Ok(v);
            }
        }
    }
    err("AudioSource has no direct audio resource")
}

/// `source_snapshot`: a mixer snapshot's values along one group's parent chain.
pub fn source_snapshot(source: &Source, snapshot_obj: &Obj, group_obj: &Obj) -> Result<Json> {
    let snap = u(source.read(snapshot_obj))?;
    let group = u(source.read(group_obj))?;
    let mixer_obj = u(source.deref(&group_obj.file, get(&group, "m_AudioMixer")?))?;
    let mixer = u(source.read(&mixer_obj))?;
    let constant = get(&mixer, "m_MixerConstant")?;
    if u(source.deref(&snapshot_obj.file, get(&snap, "m_AudioMixer")?))?.sid() != mixer_obj.sid() {
        return err("snapshot and group belong to different mixers");
    }
    let list = |k: &str| get(constant, k)?.list().ok_or_else(|| format!("{k} is not a list"));
    let index = list("snapshotGUIDs")?.iter().position(|g| g.py_eq(get(&snap, "m_SnapshotID").unwrap())).ok_or("snapshot GUID not in mixer")?;
    let values = get(&list("snapshots")?[index], "values")?.list().ok_or("values is not a list")?;
    let mut group_index = list("groupGUIDs")?.iter().position(|g| g.py_eq(get(&group, "m_GroupID").unwrap())).ok_or("group GUID not in mixer")? as i64;
    let groups = list("groups")?;
    let mut chain = Vec::new();
    let mut total = 0.0;
    let mut first = true;
    while group_index >= 0 {
        let g = &groups[group_index as usize];
        let at = |k: &str| -> Result<&Value> { values.get(get(g, k)?.int().unwrap_or(-1) as usize).ok_or_else(|| "mixer value index out of range".to_string()) };
        let volume = at("volumeIndex")?.float().unwrap_or(0.0);
        total = if first { volume } else { total + volume };
        first = false;
        chain.push(jobj(vec![("group_index", Json::Int(group_index)), ("volume_db", value_json(at("volumeIndex")?)), ("pitch", value_json(at("pitchIndex")?)), ("mute", value_json(get(g, "mute")?)), ("solo", value_json(get(g, "solo")?))]));
        group_index = get(g, "parentConstantIndex")?.int().unwrap_or(-1);
        if chain.len() > 64 {
            return err("cyclic mixer group parents");
        }
    }
    Ok(jobj(vec![
        ("snapshot_source", Json::Str(snapshot_obj.sid())),
        ("snapshot_name", value_json(get(&snap, "m_Name")?)),
        ("mixer_source", Json::Str(mixer_obj.sid())),
        ("group_source", Json::Str(group_obj.sid())),
        ("group_name", value_json(get(&group, "m_Name")?)),
        ("internal_volume_db", Json::Float(total)),
        ("chain", Json::List(chain)),
        ("effects", value_json(get(constant, "effects")?)),
        ("output_group", value_json(get(&mixer, "m_OutputGroup")?)),
        ("scope", js("Selected mixer snapshot only; output mixers/player settings remain separate")),
    ]))
}

// ---------------------------------------------------------------- inventory

struct Inventory<'a> {
    source: &'a Source,
    clips: BTreeMap<String, Obj>,
    cues: Vec<(String, Json)>,
}

impl Inventory<'_> {
    fn clip(&mut self, from: &Obj, pptr: &Value) -> Result<String> {
        let obj = u(self.source.deref(&from.file, pptr))?;
        if obj.class_id() != 83 {
            return err("audio resource is not an AudioClip");
        }
        let sid = obj.sid();
        self.clips.insert(sid.clone(), obj);
        Ok(sid)
    }

    /// A cue recorded once, in the order it was first reached.
    fn cue(&mut self, obj: Obj) -> Result<String> {
        let sid = obj.sid();
        if self.cues.iter().any(|c| c.0 == sid) {
            return Ok(sid);
        }
        if u(self.source.typename(&obj))? != "MusicCue" {
            return err("music reference is not a MusicCue");
        }
        let tree = u(self.source.read(&obj))?;
        let slot = self.cues.len();
        self.cues.push((sid.clone(), Json::Null));
        let mut channels = Vec::new();
        for (i, c) in get(&tree, "channelInfos")?.list().unwrap_or(&[]).iter().enumerate() {
            let clip = if has_path(c, "clip")? { Json::Str(self.clip(&obj, get(c, "clip")?)?) } else { Json::Null };
            channels.push(jobj(vec![("channel", Json::Int(i as i64)), ("sync", value_json(get(c, "sync")?)), ("clip", clip)]));
        }
        let mut alternatives = Vec::new();
        for alt in get(&tree, "alternatives")?.list().unwrap_or(&[]) {
            let target = u(self.source.deref(&obj.file, get(alt, "Cue")?))?;
            alternatives.push(jobj(vec![("player_data_bool", value_json(get(alt, "PlayerDataBoolKey")?)), ("cue", Json::Str(self.cue(target)?))]));
        }
        self.cues[slot].1 = jobj(vec![
            ("source", Json::Str(sid.clone())),
            ("name", value_json(get(&tree, "m_Name")?)),
            ("channels", Json::List(channels)),
            ("event", value_json(get(&tree, "originalMusicEventName")?)),
            ("track", value_json(get(&tree, "originalMusicTrackNumber")?)),
            ("alternatives", Json::List(alternatives)),
        ]);
        Ok(sid)
    }
}

/// One AudioManager atmos source, read for a resident or enabled channel.
fn atmos_source(inv: &mut Inventory, resources: &Obj, manager: &Value, channel: i64) -> Result<(Obj, Value, String)> {
    let sources = get(manager, "atmosSources")?.list().ok_or("atmosSources is not a list")?;
    let audio_obj = u(inv.source.deref(&resources.file, sources.get(channel as usize).ok_or("atmos channel out of range")?))?;
    let audio = u(inv.source.read(&audio_obj))?;
    let clip = inv.clip(&audio_obj, source_audio_ref(&audio)?)?;
    Ok((audio_obj, audio, clip))
}

fn audio_fields(audio_obj: &Obj, audio: &Value, clip: String, channel: i64) -> Result<Vec<(&'static str, Json)>> {
    Ok(vec![
        ("channel", Json::Int(channel)),
        ("audio_source", Json::Str(audio_obj.sid())),
        ("clip", Json::Str(clip)),
        ("loop", value_json(get(audio, "Loop")?)),
        ("pitch", value_json(get(audio, "m_Pitch")?)),
        ("volume", value_json(get(audio, "m_Volume")?)),
        ("play_on_awake", value_json(get(audio, "m_PlayOnAwake")?)),
    ])
}

fn inventory(source: &Source, scene_files: &[String]) -> Result<(Json, BTreeMap<String, Obj>)> {
    let resources_file = u(source.file("resources.assets"))?;
    // The persistent AudioManager.
    let mut managers = Vec::new();
    for info in resources_file.objects.iter().filter(|i| i.class_id == 114) {
        let o = Obj { file: resources_file.clone(), info: *info };
        if u(source.typename(&o))? == "AudioManager" {
            managers.push(o);
        }
    }
    if managers.len() != 1 {
        return err("expected one persistent AudioManager");
    }
    let manager_obj = managers.remove(0);
    let manager = u(source.read(&manager_obj))?;
    let mut inv = Inventory { source, clips: BTreeMap::new(), cues: Vec::new() };
    // Which clip each resident channel plays, read once off the persistent
    // AudioManager rather than off whichever admitted scene happens to enable
    // the channel: the two are the same answer, but only this one exists for a
    // channel no admitted scene enables.
    let mut resident = Vec::new();
    for channel in RESIDENT_ATMOS_CHANNELS {
        let (audio_obj, audio, clip) = atmos_source(&mut inv, &manager_obj, &manager, channel)?;
        resident.push(jobj(audio_fields(&audio_obj, &audio, clip, channel)?));
    }
    let mut scenes = Vec::new();
    for file in scene_files {
        let scene = u(Scene::new(source, file))?;
        let mut managers_json = Vec::new();
        let mut regions = Vec::new();
        let mut fsm_actions = Vec::new();
        for o in &scene.objects {
            let (index, tree) = (o.id, &o.tree);
            match o.typename.as_str() {
                "SceneManager" => {
                    let music_cue = if has_path(tree, "musicCue")? {
                        let target = u(scene.deref(get(tree, "musicCue")?))?;
                        Json::Str(inv.cue(target)?)
                    } else {
                        Json::Null
                    };
                    let mut record = vec![
                        ("source", Json::Str(format!("{file}:{index}"))),
                        ("music_cue", music_cue),
                        ("music_delay", value_json(get(tree, "musicDelayTime")?)),
                        ("music_transition", value_json(get(tree, "musicTransitionTime")?)),
                        ("ambience", Json::List(Vec::new())),
                    ];
                    if has_path(tree, "musicSnapshot")? {
                        let snap = u(scene.deref(get(tree, "musicSnapshot")?))?;
                        let name = value_json(get(&u(source.read(&snap))?, "m_Name")?);
                        record.push(("music_snapshot", jobj(vec![("source", Json::Str(snap.sid())), ("name", name)])));
                    }
                    let atmos_obj = u(scene.deref(get(tree, "atmosCue")?))?;
                    let atmos = u(source.read(&atmos_obj))?;
                    record.push(("atmos_cue", jobj(vec![("source", Json::Str(atmos_obj.sid())), ("name", value_json(get(&atmos, "m_Name")?))])));
                    let snapshot = u(source.deref(&atmos_obj.file, get(&atmos, "snapshot")?))?;
                    let mut omitted = Vec::new();
                    let mut ambience = Vec::new();
                    for (channel, enabled) in get(&atmos, "isChannelEnabled")?.list().unwrap_or(&[]).iter().enumerate() {
                        if !enabled.truthy() {
                            continue;
                        }
                        let channel = channel as i64;
                        if !RESIDENT_ATMOS_CHANNELS.contains(&channel) {
                            // The guest keeps one loop per resident channel; a stem
                            // outside that set is an explicit omission for the scene.
                            omitted.push(Json::Int(channel));
                            continue;
                        }
                        let (audio_obj, audio, clip) = atmos_source(&mut inv, &manager_obj, &manager, channel)?;
                        let group = u(source.deref(&audio_obj.file, get(&audio, "OutputAudioMixerGroup")?))?;
                        let mut fields = audio_fields(&audio_obj, &audio, clip, channel)?;
                        fields.push(("snapshot", source_snapshot(source, &snapshot, &group)?));
                        ambience.push(jobj(fields));
                    }
                    record.push(("ambience_omitted_channels", Json::List(omitted)));
                    let mut record: Vec<(String, Json)> = record.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
                    record.iter_mut().find(|(k, _)| k == "ambience").unwrap().1 = Json::List(ambience);
                    managers_json.push(Json::Obj(record));
                }
                "MusicRegion" => {
                    let gid = path_id(get(tree, "m_GameObject")?).unwrap_or(0);
                    let dirtmouth = truthy(tree, "dirtmouth")?;
                    let mut record = vec![
                        ("source", Json::Str(format!("{file}:{index}"))),
                        ("active", Json::Bool(scene.active(gid))),
                        ("enabled", Json::Bool(truthy(tree, "m_Enabled")?)),
                        ("name", value_json(get(scene.go(gid).ok_or("MusicRegion without a GameObject")?, "m_Name")?)),
                        ("dirtmouth_condition", Json::Bool(dirtmouth)),
                        ("mines_delay", Json::Bool(truthy(tree, "minesDelay")?)),
                        ("enter_seconds", value_json(get(tree, "enterTransitionTime")?)),
                        ("dirtmouth_first_cue_fade_seconds", if dirtmouth { Json::Float(1.0) } else { Json::Null }),
                        ("exit_seconds", value_json(get(tree, "exitTransitionTime")?)),
                    ];
                    let mut polygons = Vec::new();
                    for (_, typ, col) in components(&scene, gid)? {
                        if typ.ends_with("Collider2D") && truthy(col, "m_Enabled")? {
                            for polygon in collider_polygons(&scene, gid, typ, col)? {
                                polygons.push(Json::List(polygon.into_iter().map(|(x, y)| Json::List(vec![Json::Float(x), Json::Float(y)])).collect()));
                            }
                        }
                    }
                    record.push(("polygons", Json::List(polygons)));
                    for field in ["enterMusicCue", "exitMusicCue"] {
                        let value = if has_path(tree, field)? {
                            let target = u(scene.deref(get(tree, field)?))?;
                            Json::Str(inv.cue(target)?)
                        } else {
                            Json::Null
                        };
                        record.push((field, value));
                    }
                    for field in ["enterMusicSnapshot", "exitMusicSnapshot"] {
                        if !has_path(tree, field)? {
                            // Authored regions may leave a snapshot unset.
                            record.push((field, Json::Null));
                            continue;
                        }
                        let obj = u(scene.deref(get(tree, field)?))?;
                        let name = value_json(get(&u(source.read(&obj))?, "m_Name")?);
                        record.push((field, jobj(vec![("source", Json::Str(obj.sid())), ("name", name)])));
                    }
                    regions.push(jobj(record));
                }
                "PlayMakerFSM" => {
                    for state in get(get(tree, "fsm")?, "states")?.list().unwrap_or(&[]) {
                        for action in get(get(state, "actionData")?, "actionNames")?.list().unwrap_or(&[]) {
                            let name = action.str().unwrap_or_default();
                            if name.contains("Music") {
                                fsm_actions.push(jobj(vec![("source", Json::Str(format!("{file}:{index}"))), ("state", value_json(get(state, "name")?)), ("action", Json::Str(name))]));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        scenes.push(jobj(vec![("scene_file", js(file)), ("managers", Json::List(managers_json)), ("music_regions", Json::List(regions)), ("music_fsm_actions", Json::List(fsm_actions))]));
    }
    let report = jobj(vec![
        ("scenes", Json::List(scenes)),
        ("resident_atmos", Json::List(resident)),
        ("music_cues", Json::List(inv.cues.into_iter().map(|c| c.1).collect())),
        ("audio_manager", Json::Str(manager_obj.sid())),
    ]);
    Ok((report, inv.clips))
}

// ---------------------------------------------------------------- capacity

fn jstr(j: &Json, key: &str) -> String {
    match jget(j, key) {
        Some(Json::Str(s)) => s.clone(),
        _ => String::new(),
    }
}
fn jlist<'a>(j: &'a Json, key: &str) -> &'a [Json] {
    match jget(j, key) {
        Some(Json::List(l)) => l,
        _ => &[],
    }
}

fn capacity(report: &Json, clips: &[Json]) -> Result<Json> {
    let mut candidates = Vec::new();
    for (rate, channels) in [(22050i64, 2i64), (22050, 1), (11025, 1)] {
        let bps = (rate * channels * 16) as f64 / 28.0;
        candidates.push(jobj(vec![
            ("rate", Json::Int(rate)),
            ("channels", Json::Int(channels)),
            ("bytes_per_second", Json::Float(bps)),
            ("fraction_of_double_speed_2048_sector_bandwidth", Json::Float(bps / (150 * 2048) as f64)),
            ("spu_ring_256k_seconds", Json::Float((256 * 1024) as f64 / bps)),
            ("spu_half_128k_seconds", Json::Float((128 * 1024) as f64 / bps)),
            ("spu_ring_384k_seconds", Json::Float((384 * 1024) as f64 / bps)),
            ("spu_half_192k_seconds", Json::Float((192 * 1024) as f64 / bps)),
        ]));
    }
    let by = |sid: &str, rate: i64, channels: i64| -> Result<i64> {
        let c = clips.iter().find(|c| jstr(c, "source") == sid && crate::music::jint(c, "rate") == Some(rate) && crate::music::jint(c, "channels") == Some(channels)).ok_or_else(|| format!("no cooked profile {sid} {rate}/{channels}"))?;
        crate::music::jint(c, "bytes").ok_or_else(|| "profile without bytes".to_string())
    };
    let mut ambient = Vec::new();
    for scene in jlist(report, "scenes") {
        for manager in jlist(scene, "managers") {
            let stems = jlist(manager, "ambience");
            let ids: Vec<String> = stems.iter().map(|x| jstr(x, "clip")).collect();
            let sum = |rate: i64| -> Result<i64> { ids.iter().map(|s| by(s, rate, 1)).sum() };
            ambient.push(jobj(vec![
                ("scene", Json::Str(jstr(scene, "scene_file"))),
                ("clips", Json::List(ids.iter().map(|s| Json::Str(s.clone())).collect())),
                ("source_snapshot_internal_db", Json::List(stems.iter().map(|x| jget(x, "snapshot").and_then(|s| jget(s, "internal_volume_db")).cloned().unwrap_or(Json::Null)).collect())),
                ("mono10000_complete_loop_bytes", Json::Int(sum(10000)?)),
                ("mono8000_complete_loop_bytes", Json::Int(sum(8000)?)),
                ("stereo22050_combined_bps", Json::Int(ids.len() as i64 * 25200)),
                ("mono11025_complete_loop_bytes", Json::Int(sum(11025)?)),
                ("note", js("Each enabled stem retained; very quiet rain is not silently removed. Output mixer/player gains unresolved.")),
            ]));
        }
    }
    // Every resident loop, not just the ones an admitted scene enables, which
    // is what the bank actually pays for.
    let all: HashSet<String> = jlist(report, "resident_atmos").iter().map(|r| jstr(r, "clip")).collect();
    let all_sum = |rate: i64| -> Result<i64> { all.iter().map(|s| by(s, rate, 1)).sum() };
    Ok(jobj(vec![
        ("stream_profiles", Json::List(candidates)),
        ("ambience", Json::List(ambient)),
        ("all_resident_ambient_loops_8000_mono_bytes", Json::Int(all_sum(8000)?)),
        ("all_resident_ambient_loops_10000_mono_bytes", Json::Int(all_sum(10000)?)),
        ("ram_staging_proposal_bytes", Json::Int(8192)),
        ("cd_sector_bytes", Json::Int(2048)),
        ("double_speed_sectors_per_second", Json::Int(150)),
        ("staging_4_sector_fill_seconds", Json::Float(4.0 / 150.0)),
        ("spu_capacity_bytes", Json::Int(512 * 1024)),
        ("reserved_sdk_low_spu_bytes", Json::Int(0x1010)),
        ("separate_sfx_budget_bytes", Json::Int(32 * 1024)),
        ("remaining_spu_after_384k_ring_and_sfx", Json::Int(512 * 1024 - 0x1010 - 32 * 1024 - 384 * 1024)),
        ("integration_status", js("Host groundwork only; no music or ambience enabled in guest, no CD audio arbitration or SPU refill implementation")),
        (
            "blockers",
            Json::List(
                [
                    "SPU IRQ/refill state machine is absent from pinned high-level SDK",
                    "Single CD command owner must schedule room and audio extents with refill deadlines",
                    "8KiB staging plus code/state needs rechecking against final SFX-linked main RAM map",
                    "Keep enabled ambience playing across cue changes; exact phase/mixer transitions/player settings not yet reproduced",
                    "Dirtmouth music trigger is outside current Town coverage",
                ]
                .iter()
                .map(|s| js(s))
                .collect(),
            ),
        ),
    ]))
}

// ---------------------------------------------------------------- IL

/// Python's `repr(str)`.
fn py_repr(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut out = String::from(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\x{:02x}", c as u32)),
            c if c.is_control() => {
                if (c as u32) < 0x100 {
                    out.push_str(&format!("\\x{:02x}", c as u32))
                } else if (c as u32) < 0x10000 {
                    out.push_str(&format!("\\u{:04x}", c as u32))
                } else {
                    out.push_str(&format!("\\U{:08x}", c as u32))
                }
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

fn token_text(asm: &Assembly, t: u32) -> String {
    let (table, rid) = ((t >> 24) as u8, t & 0x00ff_ffff);
    let name = |tb: Table, col: usize| asm.string(asm.get(tb, rid, col)).to_string();
    match table {
        0x70 => py_repr(&asm.user_string(rid)),
        0x01 => name(Table::TypeRef, 1),
        0x02 => name(Table::TypeDef, 1),
        0x04 => name(Table::Field, 1),
        0x06 => name(Table::MethodDef, 3),
        0x08 => name(Table::Param, 2),
        0x0A => name(Table::MemberRef, 1),
        0x2A => name(Table::GenericParam, 3),
        _ => format!("token(0x{t:08X})"),
    }
}

/// inspect_il.py `inspect`: the CIL of the named types' methods, as text.
fn inspect(assembly: &Path, types: &[&str]) -> Result<String> {
    let asm = Assembly::open(assembly).map_err(|e| e.0)?;
    let mut lines: Vec<String> = Vec::new();
    let type_rows = asm.rows(Table::TypeDef);
    for rid in 1..=type_rows {
        let tname = asm.string(asm.get(Table::TypeDef, rid, 1));
        if !types.contains(&tname) {
            continue;
        }
        let first = asm.get(Table::TypeDef, rid, 5);
        let last = if rid < type_rows { asm.get(Table::TypeDef, rid + 1, 5) } else { asm.rows(Table::MethodDef) + 1 };
        for m in first..last {
            let rva = asm.get(Table::MethodDef, m, 0);
            if rva == 0 {
                continue;
            }
            let method = asm.string(asm.get(Table::MethodDef, m, 3));
            lines.push(format!("\n{tname}::{method} RVA={rva:x}"));
            let at = asm.offset(rva).map_err(|e| e.0)?;
            let data = asm.data();
            let head = if data[at] & 3 == 2 { 1u32 } else { (u16::from_le_bytes([data[at], data[at + 1]]) >> 12) as u32 * 4 };
            let code = il::body(&asm, rva).map_err(|e| e.0)?;
            for ins in il::decode(code).map_err(|e| e.0)? {
                let by_name = |index: u32| -> String {
                    match ins.name {
                        "ldarg.s" | "ldarga.s" | "starg.s" | "ldarg" | "ldarga" | "starg" => format!("argument(0x{index:04X})"),
                        "ldloc.s" | "ldloca.s" | "stloc.s" | "ldloc" | "ldloca" | "stloc" => format!("local(0x{index:04X})"),
                        _ => index.to_string(),
                    }
                };
                let operand = match ins.operand {
                    Operand::None => "None".to_string(),
                    Operand::I8(v) => v.to_string(),
                    Operand::U8(v) => by_name(v as u32),
                    Operand::U16(v) => by_name(v as u32),
                    Operand::I32(v) => v.to_string(),
                    Operand::I64(v) => v.to_string(),
                    Operand::F32(v) => pyfloat::repr(v as f64),
                    Operand::F64(v) => pyfloat::repr(v),
                    Operand::Token(t) => token_text(&asm, t),
                    Operand::Target(t) => (t + head).to_string(),
                    Operand::Switch(n) => {
                        let o = ins.offset as usize;
                        let base = o + 5 + 4 * n as usize;
                        let targets: Vec<String> = (0..n as usize)
                            .map(|i| {
                                let d = i32::from_le_bytes(code[o + 5 + 4 * i..o + 9 + 4 * i].try_into().unwrap()) as i64;
                                ((base as i64 + d) as u32 + head).to_string()
                            })
                            .collect();
                        format!("[{}]", targets.join(", "))
                    }
                };
                lines.push(format!("{:04x} {:16} {}", ins.offset + head, ins.name, operand));
            }
        }
    }
    Ok(lines.join("\n"))
}

// ---------------------------------------------------------------- main

fn scene_files(root: &Path) -> Result<Vec<String>> {
    let path = root.join("data/regions.json");
    let report: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?).map_err(|e| e.to_string())?;
    report["scenes"].as_array().ok_or("data/regions.json has no scenes")?.iter().map(|s| Ok(s["file"].as_str().ok_or("scene without a file")?.to_string())).collect()
}

pub fn cook(root: &Path, source: &Source, output: &Path) -> Result<()> {
    let hkpsx = root.join(".hkpsx");
    std::fs::create_dir_all(&hkpsx).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(output).map_err(|e| e.to_string())?;
    if !output.canonicalize().map_err(|e| e.to_string())?.starts_with(hkpsx.canonicalize().map_err(|e| e.to_string())?) {
        return err("music assets must remain ignored under .hkpsx");
    }
    let tool = Tool::build(root, "hk-music")?;
    let (report, objects) = inventory(source, &scene_files(root)?)?;
    let ambience: HashSet<String> = jlist(&report, "resident_atmos").iter().map(|r| jstr(r, "clip")).collect();
    // Every resident clip is cooked at both 8000 and 4000 whatever the
    // resident rates currently say, so re-rating a channel is a re-run of
    // ambience rather than a full source re-conversion. 10000 feeds the
    // capacity table below and nothing in the bank.
    let mut tasks: Vec<(&Obj, i64, i64)> = Vec::new();
    for (sid, obj) in &objects {
        println!("AUDIO {sid} {}", u(source.read(obj))?.get("m_Name").and_then(Value::str).unwrap_or_default());
        let mut rates = vec![(22050, 2), (11025, 1)];
        if ambience.contains(sid) {
            rates.extend([(10000, 1), (8000, 1), (4000, 1)]);
        }
        tasks.extend(rates.into_iter().map(|(rate, channels)| (obj, rate, channels)));
    }
    // The encoder dominates; a few conversions at a time, results in task order.
    let pool = rayon::ThreadPoolBuilder::new().num_threads(3).build().map_err(|e| e.to_string())?;
    let cooked: Vec<Json> = pool.install(|| {
        use rayon::prelude::*;
        tasks.par_iter().map(|(obj, rate, channels)| cook_clip(root, &tool, source, obj, output, *rate, *channels, Resampler::Ffmpeg).map(|p| p.json)).collect::<Result<Vec<_>>>()
    })?;
    let capacity = capacity(&report, &cooked)?;
    let types = ["AudioManager", "SceneManager", "MusicRegion", "MusicCue", "AudioLoopMaster", "<BeginApplyAtmosCue>d__12", "<BeginApplyMusicCue>d__14", "<FadeIn>d__14"];
    let methods = output.join("source-methods.il");
    std::fs::write(&methods, inspect(&source.directory.join("Managed/Assembly-CSharp.dll"), &types)?).map_err(|e| e.to_string())?;
    // The engine's script and default-resource containers are opened here to
    // resolve MonoScripts and built-in assets, which UnityPy does inside its own
    // environment without recording them as loaded; they are not cook inputs.
    const IMPLICIT: [&str; 2] = ["globalgamemanagers.assets", "Resources/unity default resources"];
    let mut inputs: HashSet<String> = source.loaded_files().into_iter().filter(|n| !IMPLICIT.contains(&n.as_str())).collect();
    let managed = source.directory.join("Managed");
    for entry in std::fs::read_dir(&managed).map_err(|e| e.to_string())? {
        let p = entry.map_err(|e| e.to_string())?.path();
        if p.extension().is_some_and(|e| e == "dll") {
            inputs.insert(format!("Managed/{}", p.file_name().unwrap().to_string_lossy()));
        }
    }
    for obj in objects.values() {
        let tree = u(source.read(obj))?;
        inputs.insert(get(get(&tree, "m_Resource")?, "m_Source")?.str().unwrap_or_default());
    }
    let mut sorted: Vec<String> = inputs.into_iter().collect();
    sorted.sort();
    let mut input_rows = Vec::new();
    for name in sorted {
        let p = source.directory.join(&name);
        input_rows.push((name.clone(), jobj(vec![("sha256", Json::Str(sha_file(&p)?)), ("bytes", Json::Int(std::fs::metadata(&p).map_err(|e| e.to_string())?.len() as i64))])));
    }
    let mut full = match report {
        Json::Obj(f) => f,
        _ => unreachable!(),
    };
    full.push(("clips".into(), Json::List(cooked)));
    full.push(("capacity".into(), capacity.clone()));
    full.push(("source_methods".into(), jobj(vec![("path", Json::Str(rel(root, &methods))), ("sha256", Json::Str(sha_file(&methods)?)), ("types", Json::List(types.iter().map(|t| js(t)).collect()))])));
    full.push((
        "verified_behaviors".into(),
        Json::List(
            [
                "MusicRegion accepts Hero layer9; Dirtmouth first-cue fade1s, already-Dirtmouth fade3s, exit6s",
                "ApplyAtmosCue starts enabled nonplaying channels and stops disabled channels after snapshot transition; already-playing channel phase is retained",
                "MusicCue resolves conditional nymmInTown alternative before comparing current cue identity",
            ]
            .iter()
            .map(|s| js(s))
            .collect(),
        ),
    ));
    full.push(("source_directory".into(), Json::Str(source.directory.display().to_string())));
    full.push(("inputs".into(), Json::Obj(input_rows)));
    full.push((
        "tool_hashes".into(),
        jobj(vec![("music_report.rs", Json::Str(sha(include_bytes!("music_report.rs")))), ("music.rs", Json::Str(sha(include_bytes!("music.rs")))), ("spu.rs", Json::Str(sha(include_bytes!("spu.rs"))))]),
    ));
    dump(&output.join("provenance.json"), &Json::Obj(full))?;
    dump(&output.join("capacity.json"), &capacity)?;
    println!("{}", crate::pyjson::dumps(&capacity));
    Ok(())
}

pub fn main(root: &Path, source_dir: Option<&Path>) -> Result<()> {
    let source = match source_dir {
        Some(d) => u(Source::new(d))?,
        None => u(Source::from_doctor(root))?,
    };
    cook(root, &source, &root.join(".hkpsx/music"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cook_audio::tests::{int, map};

    #[test]
    fn unity6_resource_precedes_empty_legacy_clip() {
        let reference = map(vec![("m_FileID", int(0)), ("m_PathID", int(1151))]);
        let empty = map(vec![("m_PathID", int(0))]);
        let tree = map(vec![("m_Resource", reference.clone()), ("m_audioClip", empty.clone())]);
        assert_eq!(source_audio_ref(&tree).unwrap(), &reference);
        assert_eq!(source_audio_ref(&map(vec![("m_audioClip", reference.clone())])).unwrap(), &reference);
        assert!(source_audio_ref(&map(vec![("m_Resource", empty)])).unwrap_err().contains("no direct audio"));
    }

    #[test]
    fn python_string_repr_picks_its_quotes() {
        assert_eq!(py_repr("Music Region"), "'Music Region'");
        assert_eq!(py_repr("it's"), "\"it's\"");
        assert_eq!(py_repr("a\\b\n"), "'a\\\\b\\n'");
    }
}
