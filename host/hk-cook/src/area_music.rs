//! Cook area music as mono ADPCM streams.
//! Ported from host/area_music.py, whose `data/music/track_N.adpcm`,
//! `data/area_music.rs` and `.hkpsx/area-music.json` it reproduces byte for
//! byte (except the report's `code` table, which names the Rust modules).
//!
//! Hollow Knight's area music is a MusicCue of up to six looping layers, and a
//! scene's music snapshot decides which of them are audible: Crossroads'
//! Normal plays its bass and main layers, its Sub Area only the main one. The
//! guest has one voice and one SPU ring for music, fed from main RAM and
//! refilled from CD between room loads, so each (cue, audible layer set) the
//! admitted scenes and their music regions can reach is premixed into one mono
//! stream here. What a snapshot changes on top of that is the stream's volume.
//!
//! Streams are 22,050 Hz mono PSX ADPCM, resampled to a whole number of
//! 2,048-byte sectors (a multiple of 3,584 samples) so the refill never reads
//! past a loop: the loop keeps every source sample and plays up to half a
//! 3,584-sample step fast or slow, under a cent (0.058%) for every loop past
//! 6 s; the 5.36 s drone layer is the exception at 0.071%. Payload flags are
//! zero; the guest's ring owns loop and boundary flags.
//!
//! The title and the fights' songs are XA-ADPCM on the disc, cooked by
//! xa_music.rs. Source: the persistent AudioManager's music channels and
//! mixer, each admitted scene's SceneManager and its MusicRegion colliders (as
//! bounding boxes).

use crate::ambience::gate_edges;
use crate::common::{err, get, py_round, Result};
use crate::cook_audio::{jobj, js, sha, u};
use crate::music::{decoded_source, dump, encoder_metric, jget, jint, rel, sha_file};
use crate::music_report::source_snapshot;
use crate::pyjson::{dumps_sorted_compact, parse, Json};
use crate::spu::{fnv, read_wav, run, samples_of, Tool};
use hk_unity::{Obj, Source};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

const RATE: i64 = 22050;
const SECTOR: i64 = 2048;
const SECTOR_SAMPLES: i64 = SECTOR / 16 * 28;
/// A layer quieter than this under a snapshot is off: the source mutes layers
/// at about -60 dB rather than stopping them.
const AUDIBLE_DB: f64 = -40.0;
const FULL_SCALE: f64 = 16383.0;
const TICKS: f64 = 60.0;
const KEEP: i64 = 255;

fn pitch() -> i64 {
    py_round((RATE * 4096) as f64 / 44100.0)
}

/// Samples at RATE, rounded to whole sectors of ADPCM.
fn sector_samples(frames: i64, source_rate: i64) -> i64 {
    let exact = (frames * RATE) as f64 / source_rate as f64;
    (py_round(exact / SECTOR_SAMPLES as f64)).max(1) * SECTOR_SAMPLES
}

/// numpy's `sinc`.
fn sinc(x: f64) -> f64 {
    let y = std::f64::consts::PI * if x == 0.0 { 1.0e-20 } else { x };
    y.sin() / y
}

/// Resample `x` to exactly `count` samples over the same span. The ratio is
/// within 0.1% of one, so no anti-alias band is lost; the loop wraps.
fn lanczos_resample(x: &[f64], count: usize) -> Vec<f64> {
    let taps = 16i64;
    let n = x.len() as i64;
    let mut out = vec![0.0; count];
    let step = n as f64 / count as f64;
    let half = taps / 2;
    let mut start = 0usize;
    while start < count {
        let end = count.min(start + (1 << 16));
        for (slot, i) in out[start..end].iter_mut().zip(start..end) {
            let t = i as f64 * step;
            let base = t.floor() as i64;
            let frac = t - base as f64;
            let (mut acc, mut norm) = (0.0f64, 0.0f64);
            for k in (-half + 1)..=half {
                let d = frac - k as f64;
                let w = sinc(d) * sinc(d / half as f64);
                acc += w * x[(base + k).rem_euclid(n) as usize];
                norm += w;
            }
            *slot = acc / norm;
        }
        start = end;
    }
    out
}

struct Layer {
    pcm: Vec<f64>,
    identity: Json,
    source_frames: i64,
    source_rate: i64,
}

/// One source layer as mono float PCM at RATE, `frames` long.
fn layer_pcm(
    root: &Path,
    source: &Source,
    obj: &Obj,
    folder: &Path,
    frames: usize,
) -> Result<Layer> {
    let tree = u(source.read(obj))?;
    let (wav_path, wav_bytes, identity) = decoded_source(root, source, &tree, folder)?;
    let raw = folder.join(format!("{RATE}-1.s16le"));
    run(
        Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-i"])
            .arg(&wav_path)
            .args(["-ar", &RATE.to_string(), "-ac", "1", "-f", "s16le"])
            .arg(&raw),
        None,
    )?;
    let pcm: Vec<f64> = samples_of(&std::fs::read(&raw).map_err(|e| e.to_string())?)
        .into_iter()
        .map(|s| s as f64)
        .collect();
    let w = read_wav(&wav_bytes)?;
    Ok(Layer {
        pcm: lanczos_resample(&pcm, frames),
        identity,
        source_frames: (w.data.len() / (2 * w.channels as usize)) as i64,
        source_rate: w.rate as i64,
    })
}

fn music_groups(source: &Source) -> Result<Vec<Obj>> {
    let resources = u(source.file("resources.assets"))?;
    let mut managers = Vec::new();
    for info in resources.objects.iter().filter(|i| i.class_id == 114) {
        let o = Obj {
            file: resources.clone(),
            info: *info,
        };
        if u(source.typename(&o))? == "AudioManager" {
            managers.push(o);
        }
    }
    if managers.len() != 1 {
        return err("expected one persistent AudioManager");
    }
    let manager = u(source.read(&managers[0]))?;
    let mut groups = Vec::new();
    for r in get(&manager, "musicSources")?.list().unwrap_or(&[]) {
        let audio = u(source.deref(&resources, r))?;
        let tree = u(source.read(&audio))?;
        if !get(&tree, "Loop")?.truthy() || get(&tree, "m_Pitch")?.float() != Some(1.0) {
            return err("unsupported music AudioSource");
        }
        groups.push(u(
            source.deref(&audio.file, get(&tree, "OutputAudioMixerGroup")?)
        )?);
    }
    Ok(groups)
}

fn by_sid(source: &Source, sid: &str) -> Result<Obj> {
    let (name, path) = sid.rsplit_once(':').ok_or("bad source id")?;
    let file = u(source.file(name))?;
    u(source.object(&file, path.parse::<i64>().map_err(|e| e.to_string())?))
}

// ---------------------------------------------------------------- plan

struct SceneState {
    scene: i64,
    source_scene: String,
    family: i64,
    snapshot: i64,
    delay_ticks: i64,
    fade_ticks: i64,
}
struct Region {
    scene: i64,
    source: String,
    boxed: [i64; 4],
    enter_family: i64,
    enter_snapshot: i64,
    enter_fade_ticks: i64,
    exit_family: i64,
    exit_snapshot: i64,
    exit_fade_ticks: i64,
    polygon_is_box: bool,
}

fn jnum(j: &Json) -> Result<f64> {
    match j {
        Json::Int(i) => Ok(*i as f64),
        Json::Float(f) => Ok(*f),
        _ => err("not a number"),
    }
}
fn jstring(j: &Json, key: &str) -> Result<String> {
    match jget(j, key) {
        Some(Json::Str(s)) => Ok(s.clone()),
        _ => err(format!("{key} is not a string")),
    }
}
fn jlist<'a>(j: &'a Json, key: &str) -> Result<&'a [Json]> {
    match jget(j, key) {
        Some(Json::List(l)) => Ok(l),
        _ => err(format!("{key} is not a list")),
    }
}
fn jfloat(j: &Json, key: &str) -> Result<f64> {
    jnum(jget(j, key).ok_or_else(|| format!("report lacks {key}"))?)
}

/// `reachable`: (family, snapshot) pairs a player can hear. Every scene may be
/// arrived at fresh (a Continue boots at its bench); gates carry the state
/// across, a scene applies its own state on arrival and a region its enter and
/// exit.
fn reachable(
    scenes: &[SceneState],
    regions: &[Region],
    snapshots: &[String],
    edges: &BTreeSet<(i64, i64)>,
) -> BTreeSet<(i64, i64)> {
    let mut neighbours: BTreeMap<i64, BTreeSet<i64>> = BTreeMap::new();
    for &(a, b) in edges {
        neighbours.entry(a).or_default().insert(b);
        neighbours.entry(b).or_default().insert(a);
    }
    let start = snapshots
        .iter()
        .position(|s| s == "Normal")
        .map_or(KEEP, |i| i as i64);
    let apply = |state: (i64, i64), family: i64, snapshot: i64| {
        (
            if family == KEEP { state.0 } else { family },
            if snapshot == KEEP { state.1 } else { snapshot },
        )
    };
    let mut seen = BTreeSet::new();
    let mut queue: Vec<(i64, (i64, i64))> =
        scenes.iter().map(|x| (x.scene, (KEEP, start))).collect();
    let mut heard = BTreeSet::new();
    while let Some((scene, state)) = queue.pop() {
        if !seen.insert((scene, state)) {
            continue;
        }
        let x = &scenes[scene as usize];
        let mut inside: BTreeSet<(i64, i64)> = [apply(state, x.family, x.snapshot)].into();
        for _ in 0..2 {
            for r in regions.iter().filter(|r| r.scene == scene) {
                for y in inside.clone() {
                    let e = apply(y, r.enter_family, r.enter_snapshot);
                    inside.insert(e);
                    inside.insert(apply(e, r.exit_family, r.exit_snapshot));
                }
            }
        }
        heard.extend(
            inside
                .iter()
                .filter(|y| y.0 != KEEP && y.1 != KEEP)
                .copied(),
        );
        for &n in neighbours.get(&scene).into_iter().flatten() {
            for &y in &inside {
                queue.push((n, y));
            }
        }
    }
    heard
}

struct Mix {
    layers: Vec<(i64, String, f64)>,
}
struct Family {
    cue: String,
    name: String,
    mixes: Vec<Mix>,
}

struct Plan {
    snapshots: Vec<String>,
    gains: Vec<(String, Vec<f64>)>,
    scenes: Vec<SceneState>,
    regions: Vec<Region>,
    table: Vec<Family>,
}

/// Families (cues), the snapshot table and per-scene/region music states.
fn plan(
    root: &Path,
    report: &Json,
    source: &Source,
    groups: &[Obj],
    scene_files: &[String],
) -> Result<Plan> {
    let cues: Vec<&Json> = jlist(report, "music_cues")?.iter().collect();
    let cue_of = |sid: &str| {
        cues.iter()
            .rev()
            .find(|c| jstring(c, "source").ok().as_deref() == Some(sid))
            .copied()
            .ok_or_else(|| format!("no cue {sid}"))
    };
    let mut families: Vec<String> = Vec::new();
    let mut snapshots: Vec<String> = Vec::new();
    let mut snapshot_objects: Vec<(String, String)> = Vec::new();
    let family = |sid: Option<String>, families: &mut Vec<String>| -> i64 {
        match sid {
            None => KEEP,
            Some(sid) => {
                if !families.contains(&sid) {
                    families.push(sid.clone());
                }
                families.iter().position(|f| *f == sid).unwrap() as i64
            }
        }
    };
    let mut snapshot = |r: Option<&Json>, snapshots: &mut Vec<String>| -> Result<i64> {
        let Some(r) = r.filter(|r| !matches!(r, Json::Null)) else {
            return Ok(KEEP);
        };
        let (name, src) = (jstring(r, "name")?, jstring(r, "source")?);
        if !snapshots.contains(&name) {
            snapshots.push(name.clone());
            snapshot_objects.push((name.clone(), src));
        } else if snapshot_objects.iter().find(|s| s.0 == name).unwrap().1 != src {
            return err(format!("two snapshots share a name: {name}"));
        }
        Ok(snapshots.iter().position(|s| *s == name).unwrap() as i64)
    };
    let mut scenes = Vec::new();
    let mut regions = Vec::new();
    let rows = jlist(report, "scenes")?;
    if rows
        .iter()
        .map(|r| jstring(r, "scene_file"))
        .collect::<Result<Vec<_>>>()?
        != scene_files
    {
        return err("music report covers a different catalogue");
    }
    let opt_str = |j: Option<&Json>| -> Option<String> {
        match j {
            Some(Json::Str(s)) => Some(s.clone()),
            _ => None,
        }
    };
    for (index, row) in rows.iter().enumerate() {
        let managers = jlist(row, "managers")?;
        if managers.len() != 1 {
            return err("ambiguous SceneManager");
        }
        let m = &managers[0];
        let fam = family(opt_str(jget(m, "music_cue")), &mut families);
        let snap = snapshot(jget(m, "music_snapshot"), &mut snapshots)?;
        scenes.push(SceneState {
            scene: index as i64,
            source_scene: jstring(row, "scene_file")?,
            family: fam,
            snapshot: snap,
            delay_ticks: py_round(jfloat(m, "music_delay")? * TICKS),
            fade_ticks: py_round(jfloat(m, "music_transition")? * TICKS),
        });
        for region in jlist(row, "music_regions")? {
            let flag = |k: &str| matches!(jget(region, k), Some(Json::Bool(true)));
            if !(flag("active") && flag("enabled")) {
                continue;
            }
            let polygons = jlist(region, "polygons")?;
            let points: Vec<(f64, f64)> = polygons
                .iter()
                .flat_map(|poly| match poly {
                    Json::List(p) => p
                        .iter()
                        .map(|pt| match pt {
                            Json::List(xy) => Ok((jnum(&xy[0])?, jnum(&xy[1])?)),
                            _ => err("bad polygon point"),
                        })
                        .collect::<Vec<_>>(),
                    _ => Vec::new(),
                })
                .collect::<Result<_>>()?;
            if points.is_empty() {
                continue;
            }
            let min =
                |f: fn(&(f64, f64)) -> f64| points.iter().map(f).fold(f64::INFINITY, f64::min);
            let max =
                |f: fn(&(f64, f64)) -> f64| points.iter().map(f).fold(f64::NEG_INFINITY, f64::max);
            let boxed = [min(|p| p.0), min(|p| p.1), max(|p| p.0), max(|p| p.1)]
                .map(|v| py_round(v * 65536.0));
            let enter_family = family(opt_str(jget(region, "enterMusicCue")), &mut families);
            let enter_snapshot = snapshot(jget(region, "enterMusicSnapshot"), &mut snapshots)?;
            let enter_fade_ticks = py_round(jfloat(region, "enter_seconds")? * TICKS);
            let exit_family = family(opt_str(jget(region, "exitMusicCue")), &mut families);
            let exit_snapshot = snapshot(jget(region, "exitMusicSnapshot"), &mut snapshots)?;
            let exit_fade_ticks = py_round(jfloat(region, "exit_seconds")? * TICKS);
            regions.push(Region {
                scene: index as i64,
                source: jstring(region, "source")?,
                boxed,
                enter_family,
                enter_snapshot,
                enter_fade_ticks,
                exit_family,
                exit_snapshot,
                exit_fade_ticks,
                polygon_is_box: polygons
                    .iter()
                    .all(|p| matches!(p, Json::List(l) if l.len() == 4))
                    && polygons.len() == 1,
            });
        }
    }
    let mut gains = Vec::new();
    for name in &snapshots {
        let obj = by_sid(
            source,
            &snapshot_objects.iter().find(|s| s.0 == *name).unwrap().1,
        )?;
        let mut list = Vec::new();
        for g in groups {
            list.push(jfloat(
                &source_snapshot(source, &obj, g)?,
                "internal_volume_db",
            )?);
        }
        gains.push((name.clone(), list));
    }
    let edges = gate_edges(root, scene_files)?;
    let live = reachable(&scenes, &regions, &snapshots, &edges);
    let mut table = Vec::new();
    for (f, sid) in families.iter().enumerate() {
        // nymmInTown would select Dirtmouth's accordion alternative; nothing
        // admitted sets it, so the first cue is the one cooked.
        let cue = cue_of(sid)?;
        let mut mixes = Vec::new();
        for (sn, name) in snapshots.iter().enumerate() {
            let mut layers = Vec::new();
            if live.contains(&(f as i64, sn as i64)) {
                let g = &gains.iter().find(|g| g.0 == *name).unwrap().1;
                for c in jlist(cue, "channels")? {
                    let channel = jint(c, "channel").ok_or("cue channel")?;
                    if let Some(Json::Str(clip)) = jget(c, "clip") {
                        let gain = *g
                            .get(channel as usize)
                            .ok_or("snapshot has no such music channel")?;
                        if gain > AUDIBLE_DB {
                            layers.push((channel, clip.clone(), gain));
                        }
                    }
                }
            }
            mixes.push(Mix { layers });
        }
        table.push(Family {
            cue: sid.clone(),
            name: jstring(cue, "name")?,
            mixes,
        });
    }
    Ok(Plan {
        snapshots,
        gains,
        scenes,
        regions,
        table,
    })
}

// ---------------------------------------------------------------- cook

struct TrackLayer {
    channel: i64,
    clip: String,
    relative_db: f64,
    identity: Option<Json>,
    source_frames: i64,
    source_rate: i64,
}
struct Track {
    key: (String, Vec<i64>),
    family: String,
    layers: Vec<TrackLayer>,
    json_tail: Vec<(String, Json)>,
    sectors: i64,
    byte_len: i64,
    checksum: u32,
    seconds: f64,
    headroom_db: f64,
    rate_error: f64,
}

/// Python's `round(x, 3)`.
fn round3(x: f64) -> f64 {
    format!("{x:.3}").parse().unwrap()
}

fn rust_manifest(
    tracks: &[Track],
    snapshots: &[String],
    mixes: &[Vec<(i64, i64)>],
    scenes: &[SceneState],
    regions: &[Region],
) -> String {
    let state =
        |f: i64, s: i64, t: i64| format!("MusicState{{family:{f},snapshot:{s},fade_ticks:{t}}}");
    let mut lines: Vec<String> = vec![
        "// Generated by host/area_music.py; descriptors only, no embedded audio.".into(),
        "#[derive(Clone,Copy)] pub struct MusicTrack {pub sectors:u32,pub byte_len:usize,pub checksum:u32}".into(),
        "#[derive(Clone,Copy)] pub struct MusicMix {pub track:u8,pub volume:i16}".into(),
        "#[derive(Clone,Copy)] pub struct MusicState {pub family:u8,pub snapshot:u8,pub fade_ticks:u16}".into(),
        "#[derive(Clone,Copy)] pub struct MusicScene {pub state:MusicState,pub delay_ticks:u16}".into(),
        "#[derive(Clone,Copy)] pub struct MusicRegion {pub scene:u16,pub box_:[i32;4],pub enter:MusicState,pub exit:MusicState}".into(),
        format!("/// No change: keep the playing cue or snapshot.\npub const MUSIC_KEEP:u8={KEEP};"),
        format!("pub const MUSIC_PITCH:u16={};", pitch()),
        format!("pub const MUSIC_SNAPSHOTS:usize={};", snapshots.len()),
        format!("pub const MUSIC_TRACKS:[MusicTrack;{}]=[", tracks.len()),
    ];
    for t in tracks {
        lines.push(format!(
            "MusicTrack{{sectors:{},byte_len:{},checksum:{}}},",
            t.sectors, t.byte_len, t.checksum
        ));
    }
    lines.push("];".into());
    lines.push(format!(
        "/// [family][snapshot]: which premix plays and how loud; track {KEEP} is silence."
    ));
    lines.push(format!(
        "pub const MUSIC_MIXES:[[MusicMix;{}];{}]=[",
        snapshots.len(),
        mixes.len()
    ));
    for row in mixes {
        lines.push(format!(
            "[{}],",
            row.iter()
                .map(|(t, v)| format!("MusicMix{{track:{t},volume:{v}}}"))
                .collect::<Vec<_>>()
                .join(",")
        ));
    }
    lines.push("];".into());
    lines.push(format!(
        "pub const MUSIC_SCENES:[MusicScene;{}]=[",
        scenes.len()
    ));
    for x in scenes {
        lines.push(format!(
            "MusicScene{{state:{},delay_ticks:{}}},",
            state(x.family, x.snapshot, x.fade_ticks),
            x.delay_ticks
        ));
    }
    lines.push("];".into());
    lines.push(format!(
        "pub const MUSIC_REGIONS:[MusicRegion;{}]=[",
        regions.len()
    ));
    for r in regions {
        lines.push(format!(
            "MusicRegion{{scene:{},box_:[{}, {}, {}, {}],enter:{},exit:{}}},",
            r.scene,
            r.boxed[0],
            r.boxed[1],
            r.boxed[2],
            r.boxed[3],
            state(r.enter_family, r.enter_snapshot, r.enter_fade_ticks),
            state(r.exit_family, r.exit_snapshot, r.exit_fade_ticks)
        ));
    }
    lines.push("];".into());
    lines.join("\n") + "\n"
}

fn cook_tracks(
    root: &Path,
    source: &Source,
    report: &Json,
    scene_files: &[String],
) -> Result<Json> {
    let out = root.join(".hkpsx/area-music");
    let manifest = root.join("data/area_music.rs");
    let sdk = js_source_directory(report)?;
    let s = u(Source::new(sdk))?;
    let _ = source;
    let groups = music_groups(&s)?;
    let p = plan(root, report, &s, &groups, scene_files)?;
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let tool = Tool::build(root, "hk-area-music")?;
    let mut clip_objects: BTreeMap<String, Obj> = BTreeMap::new();
    for f in &p.table {
        for m in &f.mixes {
            for (_, sid, _) in &m.layers {
                clip_objects.insert(sid.clone(), by_sid(&s, sid)?);
            }
        }
    }
    let mut tracks: Vec<Track> = Vec::new();
    let mut mix_table: Vec<Vec<(i64, i64)>> = Vec::new();
    for f in &p.table {
        let mut row = Vec::new();
        for m in &f.mixes {
            if m.layers.is_empty() {
                row.push((KEEP, 0));
                continue;
            }
            let top = m
                .layers
                .iter()
                .map(|l| l.2)
                .fold(f64::NEG_INFINITY, f64::max);
            // One stream per audible layer set: Normal Soft is Normal 10 dB
            // down with the layers half a dB apart, and plays Normal's stream.
            let key = (
                f.cue.clone(),
                m.layers.iter().map(|l| l.0).collect::<Vec<_>>(),
            );
            let found = match tracks.iter().position(|t| t.key == key) {
                Some(i) => i,
                None => {
                    tracks.push(Track {
                        key,
                        family: f.name.clone(),
                        layers: m
                            .layers
                            .iter()
                            .map(|(ch, sid, g)| TrackLayer {
                                channel: *ch,
                                clip: sid.clone(),
                                relative_db: round3(g - top),
                                identity: None,
                                source_frames: 0,
                                source_rate: 0,
                            })
                            .collect(),
                        json_tail: Vec::new(),
                        sectors: 0,
                        byte_len: 0,
                        checksum: 0,
                        seconds: 0.0,
                        headroom_db: 0.0,
                        rate_error: 0.0,
                    });
                    tracks.len() - 1
                }
            };
            row.push((
                found as i64,
                py_round(FULL_SCALE * 10f64.powf(top.min(0.0) / 20.0)),
            ));
        }
        mix_table.push(row);
    }
    for (index, t) in tracks.iter_mut().enumerate() {
        let mut lengths: Vec<(String, f64, i64)> = Vec::new();
        for layer in &t.layers {
            let tree = u(s.read(&clip_objects[&layer.clip]))?;
            let entry = (
                layer.clip.clone(),
                get(&tree, "m_Length")?.float().ok_or("m_Length")?,
                get(&tree, "m_Frequency")?.int().ok_or("m_Frequency")?,
            );
            match lengths.iter_mut().find(|l| l.0 == entry.0) {
                Some(slot) => *slot = entry,
                None => lengths.push(entry),
            }
        }
        let seconds: BTreeSet<String> = lengths.iter().map(|l| format!("{:.3}", l.1)).collect();
        if seconds.len() != 1 {
            return err(format!("layers of one cue differ in length: {}", t.family));
        }
        let (length, frequency) = (lengths[0].1, lengths[0].2);
        let source_frames = py_round(length * frequency as f64);
        let frames = sector_samples(source_frames, frequency);
        let mut mix = vec![0.0f64; frames as usize];
        for layer in &mut t.layers {
            let folder = out.join(layer.clip.replace(':', "-"));
            std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
            let l = layer_pcm(
                root,
                &s,
                &clip_objects[&layer.clip],
                &folder,
                frames as usize,
            )?;
            let scale = 10f64.powf(layer.relative_db / 20.0);
            for (m, p) in mix.iter_mut().zip(&l.pcm) {
                *m += p * scale;
            }
            layer.identity = Some(l.identity);
            layer.source_frames = l.source_frames;
            layer.source_rate = l.source_rate;
        }
        let peak = mix.iter().fold(0.0f64, |a, &x| a.max(x.abs()));
        let headroom = if peak != 0.0 {
            1.0f64.min(32767.0 / peak)
        } else {
            1.0
        };
        let pcm: Vec<i16> = mix
            .iter()
            .map(|&x| (x * headroom).round_ties_even().clamp(-32768.0, 32767.0) as i16)
            .collect();
        let mono = out.join(format!("track_{index}.s16le"));
        std::fs::write(
            &mono,
            pcm.iter()
                .flat_map(|s| s.to_le_bytes())
                .collect::<Vec<u8>>(),
        )
        .map_err(|e| e.to_string())?;
        let data = tool.encode(&pcm, "whole")?;
        std::fs::write(out.join(format!("track_{index}.adpcm")), &data)
            .map_err(|e| e.to_string())?;
        let metric = encoder_metric(&pcm, &data)?;
        if data.len() as i64 != frames / 28 * 16
            || data.len() as i64 % SECTOR != 0
            || data.chunks_exact(16).any(|b| b[1] != 0)
        {
            return err("music stream is not whole sectors of flagless ADPCM");
        }
        let path = root.join("data/music").join(format!("track_{index}.adpcm"));
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::write(&path, &data).map_err(|e| e.to_string())?;
        t.sectors = data.len() as i64 / SECTOR;
        t.byte_len = data.len() as i64;
        t.checksum = fnv(&data);
        t.seconds = frames as f64 / RATE as f64;
        t.headroom_db = 20.0 * headroom.log10();
        t.rate_error = frames as f64 / ((source_frames * RATE) as f64 / frequency as f64) - 1.0;
        t.json_tail = vec![
            ("path".into(), Json::Str(rel(root, &path))),
            ("frames".into(), Json::Int(frames)),
            ("seconds".into(), Json::Float(t.seconds)),
            ("byte_len".into(), Json::Int(t.byte_len)),
            ("sectors".into(), Json::Int(t.sectors)),
            ("checksum".into(), Json::Int(t.checksum as i64)),
            ("sha256".into(), Json::Str(sha(&data))),
            ("headroom_db".into(), Json::Float(t.headroom_db)),
            ("rate_error".into(), Json::Float(t.rate_error)),
            ("encoder_metric".into(), metric),
        ];
    }
    std::fs::create_dir_all(manifest.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(
        &manifest,
        rust_manifest(&tracks, &p.snapshots, &mix_table, &p.scenes, &p.regions),
    )
    .map_err(|e| e.to_string())?;
    let track_json: Vec<Json> = tracks
        .iter()
        .map(|t| {
            let mut f = vec![
                ("family".to_string(), Json::Str(t.family.clone())),
                (
                    "layers".into(),
                    Json::List(
                        t.layers
                            .iter()
                            .map(|l| {
                                jobj(vec![
                                    ("channel", Json::Int(l.channel)),
                                    ("clip", Json::Str(l.clip.clone())),
                                    ("relative_db", Json::Float(l.relative_db)),
                                    ("source_identity", l.identity.clone().unwrap_or(Json::Null)),
                                    ("source_frames", Json::Int(l.source_frames)),
                                    ("source_rate", Json::Int(l.source_rate)),
                                ])
                            })
                            .collect(),
                    ),
                ),
            ];
            f.extend(t.json_tail.clone());
            Json::Obj(f)
        })
        .collect();
    Ok(jobj(vec![
        ("format", js("hk-area-music-v1")),
        ("rate", Json::Int(RATE)),
        ("pitch", Json::Int(pitch())),
        ("families", Json::List(p.table.iter().map(|f| Json::Str(f.name.clone())).collect())),
        ("snapshots", Json::List(p.snapshots.iter().map(|s| Json::Str(s.clone())).collect())),
        ("snapshot_layer_db", Json::Obj(p.gains.iter().map(|(n, g)| (n.clone(), Json::List(g.iter().map(|&x| Json::Float(x)).collect()))).collect())),
        ("mixes", Json::List(mix_table.iter().map(|row| Json::List(row.iter().map(|&(t, v)| jobj(vec![("track", Json::Int(t)), ("volume", Json::Int(v))])).collect())).collect())),
        ("tracks", Json::List(track_json)),
        (
            "scenes",
            Json::List(
                p.scenes
                    .iter()
                    .map(|x| jobj(vec![("scene", Json::Int(x.scene)), ("source_scene", Json::Str(x.source_scene.clone())), ("family", Json::Int(x.family)), ("snapshot", Json::Int(x.snapshot)), ("delay_ticks", Json::Int(x.delay_ticks)), ("fade_ticks", Json::Int(x.fade_ticks))]))
                    .collect(),
            ),
        ),
        (
            "regions",
            Json::List(
                p.regions
                    .iter()
                    .map(|r| {
                        jobj(vec![
                            ("scene", Json::Int(r.scene)),
                            ("source", Json::Str(r.source.clone())),
                            ("box", Json::List(r.boxed.iter().map(|&v| Json::Int(v)).collect())),
                            ("enter_family", Json::Int(r.enter_family)),
                            ("enter_snapshot", Json::Int(r.enter_snapshot)),
                            ("enter_fade_ticks", Json::Int(r.enter_fade_ticks)),
                            ("exit_family", Json::Int(r.exit_family)),
                            ("exit_snapshot", Json::Int(r.exit_snapshot)),
                            ("exit_fade_ticks", Json::Int(r.exit_fade_ticks)),
                            ("polygon_is_box", Json::Bool(r.polygon_is_box)),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("manifest_sha256", Json::Str(sha_file(&manifest)?)),
        (
            "limitations",
            Json::List(
                [
                    "Layer sets are premixed per (cue, audible layers): a Normal to Sub Area change switches premix at the same offset once the buffered audio ahead of it has played, instead of fading the bass layer.",
                    "MusicRegion colliders are tested as their bounding boxes.",
                    "nymmInTown (the Dirtmouth accordion) is not tracked; Dirtmouth plays its first cue.",
                    "Source mixer effects and exact transition curves are not reproduced; volume ramps are linear.",
                ]
                .iter()
                .map(|s| js(s))
                .collect(),
            ),
        ),
    ]))
}

fn js_source_directory(report: &Json) -> Result<String> {
    jstring(report, "source_directory")
}

pub fn cook(root: &Path, source: &Source) -> Result<()> {
    let provenance = root.join(".hkpsx/music/provenance.json");
    let report_path = root.join(".hkpsx/area-music.json");
    let manifest = root.join("data/area_music.rs");
    let regions = parse(
        &std::fs::read_to_string(root.join("data/regions.json"))
            .map_err(|e| format!("data/regions.json: {e}"))?,
    )?;
    let scene_files: Vec<String> = jlist(&regions, "scenes")?
        .iter()
        .map(|s| jstring(s, "file"))
        .collect::<Result<_>>()?;
    let inputs = jobj(vec![("provenance", Json::Str(sha_file(&provenance)?))]);
    let code = jobj(vec![
        (
            "area_music.rs",
            Json::Str(sha(include_bytes!("area_music.rs"))),
        ),
        ("music.rs", Json::Str(sha(include_bytes!("music.rs")))),
        ("spu.rs", Json::Str(sha(include_bytes!("spu.rs")))),
    ]);
    if report_path.exists() {
        let old = parse(&std::fs::read_to_string(&report_path).map_err(|e| e.to_string())?)?;
        let tracks = jlist(&old, "tracks").unwrap_or(&[]);
        let same = |a: Option<&Json>, b: &Json| {
            a.is_some_and(|a| dumps_sorted_compact(a) == dumps_sorted_compact(b))
        };
        let intact = tracks.iter().all(|t| {
            jstring(t, "path").ok().is_some_and(|p| {
                !Path::new(&p).is_absolute()
                    && sha_file(&root.join(&p)).ok() == jstring(t, "sha256").ok()
            })
        }) && manifest.exists();
        if same(jget(&old, "inputs"), &inputs)
            && same(jget(&old, "code"), &code)
            && intact
            && jstring(&old, "manifest_sha256").ok() == sha_file(&manifest).ok()
        {
            println!("Area music cache verified");
            return Ok(());
        }
    }
    let report = parse(
        &std::fs::read_to_string(&provenance)
            .map_err(|e| format!("{}: {e}", provenance.display()))?,
    )?;
    let mut result = match cook_tracks(root, source, &report, &scene_files)? {
        Json::Obj(f) => f,
        _ => unreachable!(),
    };
    result.push(("inputs".into(), inputs));
    result.push(("code".into(), code));
    dump(&report_path, &Json::Obj(result.clone()))?;
    for (i, t) in jlist(&Json::Obj(result), "tracks")?.iter().enumerate() {
        let layers: Vec<i64> = jlist(t, "layers")?
            .iter()
            .filter_map(|l| jint(l, "channel"))
            .collect();
        println!(
            "track {i}: {} layers {} {:.2}s {} bytes headroom {:.2} dB rate error {:+.4}%",
            jstring(t, "family")?,
            format!(
                "[{}]",
                layers
                    .iter()
                    .map(i64::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            jfloat(t, "seconds")?,
            jint(t, "byte_len").unwrap_or(0),
            jfloat(t, "headroom_db")?,
            jfloat(t, "rate_error")? * 100.0
        );
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

    #[test]
    fn loops_are_whole_sectors_within_a_cent() {
        for (seconds, rate) in [
            (153.6f64, 44100i64),
            (152.47061224489795, 44100),
            (103.74512471655329, 44100),
            (5.36, 44100),
            (0.01, 48000),
        ] {
            let frames = py_round(seconds * rate as f64);
            let out = sector_samples(frames, rate);
            assert_eq!(out % SECTOR_SAMPLES, 0);
            assert_eq!(out / 28 * 16 % SECTOR, 0);
            // Half a step of 3,584 samples: under a cent for any loop past 6 s.
            if seconds > 6.0 {
                assert!(
                    (out as f64 / (frames as f64 * RATE as f64 / rate as f64) - 1.0).abs()
                        < 0.00058
                );
            }
            if seconds > 1.0 {
                assert!(
                    (out as f64 - frames as f64 * RATE as f64 / rate as f64).abs()
                        <= SECTOR_SAMPLES as f64 / 2.0
                );
            }
        }
        // 153.6 s is already a whole number of sectors at 22,050 Hz.
        assert_eq!(
            sector_samples(py_round(153.6 * 44100.0), 44100),
            py_round(153.6 * 22050.0)
        );
    }

    #[test]
    fn resample_hits_the_exact_length_and_keeps_a_tone() {
        let n = 10000;
        let x: Vec<f64> = (0..n)
            .map(|t| (2.0 * std::f64::consts::PI * t as f64 / 50.0).sin() * 1000.0 + 300.0)
            .collect();
        let y = lanczos_resample(&x, n + 7);
        assert_eq!(y.len(), n + 7);
        let mean = y.iter().sum::<f64>() / y.len() as f64;
        assert!((mean - 300.0).abs() < 2.0);
        assert!((y.iter().map(|v| (v - 300.0).abs()).fold(0.0, f64::max) - 1000.0).abs() < 15.0);
    }

    #[test]
    fn reachable_follows_gates_scene_states_and_regions() {
        // Scene 0 plays family 0 Normal; scene 1 keeps the cue under Sub Area;
        // scene 2 keeps everything and has a region entering family 1 Normal.
        let scene = |scene, family, snapshot| SceneState {
            scene,
            source_scene: String::new(),
            family,
            snapshot,
            delay_ticks: 0,
            fade_ticks: 0,
        };
        let scenes = [
            scene(0, 0, 1),
            scene(1, KEEP, 2),
            scene(2, KEEP, KEEP),
            scene(3, KEEP, 0),
        ];
        let regions = [Region {
            scene: 2,
            source: String::new(),
            boxed: [0; 4],
            enter_family: 1,
            enter_snapshot: 1,
            enter_fade_ticks: 0,
            exit_family: KEEP,
            exit_snapshot: 0,
            exit_fade_ticks: 0,
            polygon_is_box: true,
        }];
        let names: Vec<String> = ["Silent", "Normal", "Sub Area"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let heard = reachable(&scenes, &regions, &names, &[(0, 1), (1, 2)].into());
        assert!(heard.contains(&(0, 1)) && heard.contains(&(0, 2)));
        assert!(heard.contains(&(1, 1)) && heard.contains(&(1, 0)));
        // The region's cue carries back through the gate into Sub Area.
        assert!(heard.contains(&(1, 2)));
        // Scene 3 has no gate and no cue: a fresh arrival there hears nothing.
        assert_eq!(heard.len(), 5);
        assert!(heard.iter().all(|p| p.0 != KEEP && p.1 != KEEP));
    }

    #[test]
    fn manifest_states_tracks_and_regions() {
        let track = Track {
            key: (String::new(), vec![]),
            family: String::new(),
            layers: vec![],
            json_tail: vec![],
            sectors: 945,
            byte_len: 945 * 2048,
            checksum: 7,
            seconds: 0.0,
            headroom_db: 0.0,
            rate_error: 0.0,
        };
        let mixes = vec![vec![(KEEP, 0), (0, 16383)]];
        let scene = SceneState {
            scene: 0,
            source_scene: String::new(),
            family: 0,
            snapshot: 1,
            delay_ticks: 60,
            fade_ticks: 300,
        };
        let region = Region {
            scene: 0,
            source: String::new(),
            boxed: [1, 2, 3, 4],
            enter_family: KEEP,
            enter_snapshot: 0,
            enter_fade_ticks: 60,
            exit_family: KEEP,
            exit_snapshot: 1,
            exit_fade_ticks: 120,
            polygon_is_box: true,
        };
        let names: Vec<String> = ["Silent", "Normal"].iter().map(|s| s.to_string()).collect();
        let text = rust_manifest(&[track], &names, &mixes, &[scene], &[region]);
        assert!(text.contains("pub const MUSIC_TRACKS:[MusicTrack;1]=["));
        assert!(text.contains("MusicTrack{sectors:945,byte_len:1935360,checksum:7},"));
        assert!(text.contains("pub const MUSIC_MIXES:[[MusicMix;2];1]=["));
        assert!(text.contains(
            "MusicScene{state:MusicState{family:0,snapshot:1,fade_ticks:300},delay_ticks:60},"
        ));
        assert!(text.contains(&format!("MusicRegion{{scene:0,box_:[1, 2, 3, 4],enter:MusicState{{family:{KEEP},snapshot:0,fade_ticks:60}}")));
        assert!(text.contains(&format!("pub const MUSIC_PITCH:u16={};", pitch())));
    }
}
