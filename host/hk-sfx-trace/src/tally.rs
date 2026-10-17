//! Fold the original's audio-call logs (hkref: `survey-*`, `play`) into what a
//! scenario actually played: distinct sounds, calls, peak simultaneous voices,
//! SPU bytes at several rates, next to the census's "can play here" bound.

use crate::csv::{table, tsv, Row};
use crate::rates::adpcm_bytes;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

const FPS: f64 = 60.0;
/// A sound is "near" when its source is within this many world units of the
/// Knight (the screen is about 30 wide); UI and Knight sounds sit on the Knight.
const NEAR: f64 = 30.0;
/// A scene-local clip longer than this is a stream candidate (XA in an arena where the
/// CD is free), not an SPU bank entry: the 24.9 s boss defeat sting, a 7.2 s roar.
const STREAM_SECS: f64 = 6.0;

#[derive(Clone, Default)]
struct ClipInfo {
    secs: f64,
    rate: i64,
    cat: String,
}

#[derive(Clone, Default)]
struct Need {
    r15: i64,
    r20: i64,
    r30: i64,
    r40: i64,
}

#[derive(Default)]
struct Stat {
    calls: usize,
    near: usize,
    first: i64,
    looped: bool,
    hierarchy: String,
    callsite: String,
}

struct Scenario {
    name: String,
    scene: String,
    kind: String,
    frames: i64,
    calls: Vec<Call>,
    stats: BTreeMap<String, Stat>,
}

struct Call {
    frame: i64,
    op: String,
    clip: String,
    callsite: String,
    hierarchy: String,
    source_id: String,
    pitch: f64,
    near: bool,
}

fn f(r: &Row, k: &str) -> f64 {
    r.get(k).and_then(|v| v.parse().ok()).unwrap_or(f64::NAN)
}
fn s<'a>(r: &'a Row, k: &str) -> &'a str {
    r.get(k).map(String::as_str).unwrap_or("")
}

/// Calls of one run directory between two test frames, tagged near/far from the Knight.
fn read_calls(dir: &Path, from: i64, to: i64, hero: &HeroTrack) -> Vec<Call> {
    let mut out = Vec::new();
    for r in table(&dir.join("audio-calls.csv")) {
        let q = f(&r, "queued_test_frame") as i64;
        if q < from || q > to {
            continue;
        }
        let op = s(&r, "operation");
        if !matches!(op, "Play" | "PlayOneShot" | "PlayClipAtPoint" | "Stop") {
            continue;
        }
        let (x, y) = (f(&r, "x"), f(&r, "y"));
        let near = match hero.range(..=q).next_back() {
            Some((_, (hx, hy))) if x.is_finite() && y.is_finite() => {
                (x - hx).abs() <= NEAR && (y - hy).abs() <= NEAR
            }
            _ => true,
        };
        out.push(Call {
            frame: q,
            op: op.to_string(),
            clip: s(&r, "clip_or_snapshot").to_string(),
            callsite: s(&r, "callsite")
                .split('@')
                .next()
                .unwrap_or("")
                .to_string(),
            hierarchy: s(&r, "hierarchy").to_string(),
            source_id: s(&r, "source_id").to_string(),
            pitch: f(&r, "pitch").max(0.1),
            near,
        });
    }
    out
}

/// The Knight's (x, y) per test frame.
type HeroTrack = BTreeMap<i64, (f64, f64)>;

fn hero_track(dir: &Path) -> HeroTrack {
    let mut m = BTreeMap::new();
    for r in table(&dir.join("state.csv")) {
        let t = f(&r, "test_frame");
        if t >= 0.0 {
            m.insert(t as i64, (f(&r, "x"), f(&r, "y")));
        }
    }
    m
}

fn loops_of(dir: &Path, scene: &str) -> BTreeSet<String> {
    table(&dir.join("survey-audiosources.csv"))
        .into_iter()
        .filter(|r| s(r, "scene") == scene && s(r, "loop") == "True")
        .map(|r| s(&r, "clip").to_string())
        .collect()
}

fn scenario(
    name: &str,
    scene: &str,
    kind: &str,
    frames: i64,
    calls: Vec<Call>,
    loops: BTreeSet<String>,
    info: &BTreeMap<String, ClipInfo>,
) -> Scenario {
    let mut stats: BTreeMap<String, Stat> = BTreeMap::new();
    for c in &calls {
        if c.op == "Stop"
            || c.clip.is_empty()
            || info
                .get(&c.clip)
                .is_some_and(|i| matches!(i.cat.as_str(), "Music" | "Atmos"))
        {
            continue;
        }
        let st = stats.entry(c.clip.clone()).or_insert_with(|| Stat {
            first: c.frame,
            ..Default::default()
        });
        st.calls += 1;
        st.near += c.near as usize;
        st.first = st.first.min(c.frame);
        if st.calls == 1 {
            st.callsite = c.callsite.clone();
            st.hierarchy = c.hierarchy.clone();
        }
        st.looped |= c.op == "Play"
            && (loops.contains(&c.clip) || info.get(&c.clip).is_some_and(|i| i.secs >= 12.0));
    }
    Scenario {
        name: name.into(),
        scene: scene.into(),
        kind: kind.into(),
        frames,
        calls,
        stats,
    }
}

/// Peak simultaneous one-shot voices. A `PlayOneShot` holds a voice for its clip's
/// length at its pitch. A `Play` holds the source's one voice from the call to the
/// clip's end, a later `Play` or `Stop` of the same source, whichever comes first.
/// Looping sources and music are counted apart (ambience pool, music ring).
fn peak_voices(
    sc: &Scenario,
    info: &BTreeMap<String, ClipInfo>,
    near_only: bool,
) -> (usize, usize, f64, String) {
    let secs_of = |clip: &str, pitch: f64| info.get(clip).map(|i| i.secs).unwrap_or(1.0) / pitch;
    let counted = |c: &Call| {
        !c.clip.is_empty()
            && (!near_only || c.near)
            && !sc.stats.get(&c.clip).is_some_and(|s| s.looped)
            && !info
                .get(&c.clip)
                .is_some_and(|i| matches!(i.cat.as_str(), "Music" | "Atmos"))
    };
    let mut ev: Vec<(f64, i32, &str)> = Vec::new();
    let mut open: BTreeMap<&str, (f64, f64, &str)> = BTreeMap::new(); // source -> (start, natural end, clip)
    fn close<'a>(ev: &mut Vec<(f64, i32, &'a str)>, start: f64, end: f64, clip: &'a str) {
        ev.push((start, 1, clip));
        ev.push((end.max(start), -1, clip));
    }
    for c in &sc.calls {
        let t = c.frame as f64 / FPS;
        match c.op.as_str() {
            "PlayOneShot" | "PlayClipAtPoint" if counted(c) => {
                close(&mut ev, t, t + secs_of(&c.clip, c.pitch), &c.clip)
            }
            "Play" => {
                if let Some((start, end, clip)) = open.remove(c.source_id.as_str()) {
                    close(&mut ev, start, end.min(t), clip);
                }
                if counted(c) {
                    open.insert(
                        c.source_id.as_str(),
                        (t, t + secs_of(&c.clip, c.pitch), &c.clip),
                    );
                }
            }
            "Stop" => {
                if let Some((start, end, clip)) = open.remove(c.source_id.as_str()) {
                    close(&mut ev, start, end.min(t), clip);
                }
            }
            _ => {}
        }
    }
    for (_, (start, end, clip)) in open {
        close(&mut ev, start, end, clip);
    }
    ev.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap().then(a.1.cmp(&b.1)));
    let (mut peak, mut peak_distinct, mut at, mut set) = (0i32, 0usize, 0.0, String::new());
    let mut active: BTreeMap<&str, i32> = BTreeMap::new();
    for (t, d, clip) in ev {
        *active.entry(clip).or_default() += d;
        active.retain(|_, n| *n > 0);
        let cur: i32 = active.values().sum();
        peak_distinct = peak_distinct.max(active.len());
        if cur > peak {
            peak = cur;
            at = t;
            set = active
                .iter()
                .map(|(c, n)| {
                    if *n > 1 {
                        format!("{c}x{n}")
                    } else {
                        c.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join("|");
        }
    }
    (peak as usize, peak_distinct, at, set)
}

pub fn run(census: &str, rates: &str, out: &str, manifest: &str) -> Result<(), String> {
    let census = Path::new(census);
    // clip name -> info (duplicates: the longest, Persistent wins)
    let mut info: BTreeMap<String, ClipInfo> = BTreeMap::new();
    for r in tsv(&census.join("clips.tsv"))? {
        if r.len() < 8 {
            continue;
        }
        let ci = ClipInfo {
            secs: r[4].parse().unwrap_or(0.0),
            rate: r[3].parse().unwrap_or(44100),
            cat: r[7].clone(),
        };
        let e = info.entry(r[1].clone()).or_insert_with(|| ci.clone());
        if ci.cat == "Persistent" || (e.cat != "Persistent" && ci.secs > e.secs) {
            *e = ci;
        }
    }
    let mut need: BTreeMap<String, Need> = BTreeMap::new();
    if rates != "-" {
        for r in tsv(Path::new(rates))? {
            if r.len() >= 13 {
                need.insert(
                    r[1].clone(),
                    Need {
                        r15: r[9].parse().unwrap_or(22050),
                        r20: r[10].parse().unwrap_or(22050),
                        r30: r[11].parse().unwrap_or(22050),
                        r40: r[12].parse().unwrap_or(22050),
                    },
                );
            }
        }
    }
    // census reach per scene
    let mut reach: BTreeMap<String, (usize, u64)> = BTreeMap::new();
    for r in tsv(&census.join("scene_spu.tsv"))? {
        if r.len() >= 9 {
            reach.insert(
                r[0].clone(),
                (r[3].parse().unwrap_or(0), r[8].parse().unwrap_or(0)),
            );
        }
    }

    // clip names the port's cookers mention (a quoted literal in the audio sources)
    let mut ported: BTreeSet<String> = BTreeSet::new();
    let cook_src = census.join("../../host/hk-cook/src");
    for f in [
        "cook_audio.rs",
        "focus_audio.rs",
        "geo_audio.rs",
        "runner_audio.rs",
        "mossman_audio.rs",
        "scene_sfx.rs",
    ] {
        if let Ok(t) = std::fs::read_to_string(cook_src.join(f)) {
            for part in t.split('"').skip(1).step_by(2) {
                if info.contains_key(part) {
                    ported.insert(part.to_string());
                }
            }
        }
    }
    let mut scenarios: Vec<Scenario> = Vec::new();
    // looping AudioSources per scene, from the survey runs (a play run does not list sources)
    let mut known_loops: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    // clips an FSM action, a static component field or an AudioSource can reach, per scene (survey runs)
    let mut known_fsm: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let text = std::fs::read_to_string(manifest).map_err(|e| format!("{manifest}: {e}"))?;
    for line in text
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
    {
        let p: Vec<&str> = line.split_whitespace().collect();
        match p.as_slice() {
            ["survey", dir] => {
                let base = PathBuf::from(dir).join("og-survey");
                let mut attempts: Vec<PathBuf> = std::fs::read_dir(&base)
                    .map_err(|e| format!("{}: {e}", base.display()))?
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.join("survey-timeline.csv").exists())
                    .collect();
                attempts.sort();
                // the last attempt that finished a scene owns it
                let mut owner: BTreeMap<String, PathBuf> = BTreeMap::new();
                for a in &attempts {
                    for r in table(&a.join("survey-scenes.csv")) {
                        if s(&r, "status").starts_with("ok") {
                            owner.insert(s(&r, "scene").to_string(), a.clone());
                        }
                    }
                }
                for a in &attempts {
                    for r in table(&a.join("survey-audiosources.csv")) {
                        if s(&r, "loop") == "True" {
                            known_loops
                                .entry(s(&r, "scene").to_string())
                                .or_default()
                                .insert(s(&r, "clip").to_string());
                        }
                    }
                }
                for r in table(&PathBuf::from(dir).join("fsm-audio.csv")) {
                    if s(&r, "kind") != "audio" {
                        continue;
                    }
                    for tok in s(&r, "clips_or_objects").split('|') {
                        if let Some((_, v)) = tok.split_once('=') {
                            if info.contains_key(v) {
                                known_fsm
                                    .entry(s(&r, "scene").to_string())
                                    .or_default()
                                    .insert(v.to_string());
                            }
                        }
                    }
                }
                for r in table(&PathBuf::from(dir).join("actors.csv")) {
                    for tok in s(&r, "static_clips").split('|') {
                        if let Some((_, v)) = tok.split_once('=') {
                            if info.contains_key(v) {
                                known_fsm
                                    .entry(s(&r, "scene").to_string())
                                    .or_default()
                                    .insert(v.to_string());
                            }
                        }
                    }
                }
                for r in table(&PathBuf::from(dir).join("audiosources.csv")) {
                    let c = s(&r, "clip");
                    if info.contains_key(c) {
                        known_fsm
                            .entry(s(&r, "scene").to_string())
                            .or_default()
                            .insert(c.to_string());
                    }
                }
                let mut cache: BTreeMap<PathBuf, (Vec<Row>, HeroTrack)> = BTreeMap::new();
                for (scene, a) in &owner {
                    let (tl, hero) = cache
                        .entry(a.clone())
                        .or_insert_with(|| (table(&a.join("survey-timeline.csv")), hero_track(a)));
                    let w: Vec<&Row> = tl.iter().filter(|r| s(r, "scene") == scene).collect();
                    let (Some(from), Some(to)) = (
                        w.iter().map(|r| f(r, "start_frame") as i64).min(),
                        w.iter().map(|r| f(r, "end_frame") as i64).max(),
                    ) else {
                        continue;
                    };
                    let calls = read_calls(a, from, to, hero);
                    scenarios.push(scenario(
                        scene,
                        scene,
                        "survey",
                        to - from,
                        calls,
                        loops_of(a, scene),
                        &info,
                    ));
                }
            }
            ["play", name, dir] | ["play", name, dir, _, _] => {
                let d = PathBuf::from(dir);
                let hero = hero_track(&d);
                let mut first_frame = 0i64;
                let mut last = hero.keys().next_back().copied().unwrap_or(0);
                // optional window in test frames (a boss fight, not the walk to it)
                if let ["play", _, _, from, to] = p.as_slice() {
                    first_frame = from.parse().map_err(|_| format!("bad window in: {line}"))?;
                    last = last.min(to.parse().map_err(|_| format!("bad window in: {line}"))?);
                }
                // the scene the Knight is in at each test frame
                let mut scene_at: BTreeMap<i64, String> = BTreeMap::new();
                let mut frames_in: BTreeMap<String, i64> = BTreeMap::new();
                for r in table(&d.join("state.csv")) {
                    let t = f(&r, "test_frame");
                    if t >= 0.0 {
                        scene_at.insert(t as i64, s(&r, "scene").to_string());
                        *frames_in.entry(s(&r, "scene").to_string()).or_default() += 1;
                    }
                }
                let calls = read_calls(&d, first_frame, last, &hero);
                if frames_in.len() > 1 {
                    for (scene, n) in &frames_in {
                        let part: Vec<Call> = read_calls(&d, first_frame, last, &hero)
                            .into_iter()
                            .filter(|c| {
                                scene_at
                                    .range(..=c.frame)
                                    .next_back()
                                    .is_some_and(|(_, sc)| sc == scene)
                            })
                            .collect();
                        scenarios.push(scenario(
                            &format!("{name}:{scene}"),
                            scene,
                            "play",
                            *n,
                            part,
                            known_loops.get(scene).cloned().unwrap_or_default(),
                            &info,
                        ));
                    }
                }
                let first = frames_in.keys().next().cloned().unwrap_or_default();
                scenarios.push(scenario(
                    name,
                    if frames_in.len() == 1 { &first } else { "" },
                    if frames_in.len() > 1 {
                        "play-total"
                    } else {
                        "play"
                    },
                    last - first_frame,
                    calls,
                    known_loops.get(&first).cloned().unwrap_or_default(),
                    &info,
                ));
            }
            _ => return Err(format!("bad manifest line: {line}")),
        }
    }
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;

    let rate_of = |name: &str, which: u8| -> i64 {
        match (which, need.get(name)) {
            (0, Some(n)) => n.r30,
            (1, Some(n)) => n.r40,
            (2, Some(n)) => n.r20,
            (3, Some(n)) => n.r15,
            _ => 22050,
        }
    };
    let mut summary = String::from("# scenario\tkind\tseconds\tcalls\tnear_calls\tdistinct\tlocal_distinct\tpersistent_distinct\tloops\twide_distinct\tpeak_voices_all\tpeak_voices_near\tpeak_distinct_sounds\tpeak_at_s\tpeak_set\tlocal_B@22050\tlocal_B@5512\tlocal_B@min15\tlocal_B@min20\tlocal_B@min30\tlocal_B@min40\twide_B@5512\twide_B@min15\twide_B@min20\tstream_distinct\tstream_seconds\tcover90_clips\tcover90_B@m15\tcover99_clips\tcover99_B@m15\tfsm_reach_local_clips\tfsm_reach_B@5512\tfsm_reach_B@min20\tcensus_reach_local_clips\tcensus_reach_B@5512\n");
    let mut detail = String::from("# scenario\tclip\tcategory\tin_port_cook\tcalls\tnear_calls\tcalls_per_min\tseconds\tsrc_rate\tloop\tfirst_s\tcallsite\tsource\n");
    for sc in &scenarios {
        let secs = sc.frames as f64 / FPS;
        let (mut local, mut pers, mut loops, mut wide) = (0, 0, 0, 0);
        let (mut w55, mut w15, mut w20) = (0u64, 0u64, 0u64);
        let (mut streamed, mut stream_secs) = (0usize, 0f64);
        let (mut b22, mut b55, mut b15, mut b20, mut b30, mut b40) =
            (0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
        let (mut calls, mut near) = (0, 0);
        for (clip, st) in &sc.stats {
            let ci = info.get(clip).cloned().unwrap_or(ClipInfo {
                cat: "unknown".into(),
                secs: 0.0,
                rate: 44100,
            });
            calls += st.calls;
            near += st.near;
            if st.looped {
                loops += 1;
            } else if ci.cat == "Persistent" {
                pers += 1;
            } else if ci.cat == "Wide" {
                wide += 1;
                w55 += adpcm_bytes(ci.secs, 5512, ci.rate);
                w15 += adpcm_bytes(ci.secs, rate_of(clip, 3), ci.rate);
                w20 += adpcm_bytes(ci.secs, rate_of(clip, 2), ci.rate);
            } else if ci.secs > STREAM_SECS {
                streamed += 1;
                stream_secs += ci.secs;
            } else {
                local += 1;
                b22 += adpcm_bytes(ci.secs, 22050, ci.rate);
                b55 += adpcm_bytes(ci.secs, 5512, ci.rate);
                b15 += adpcm_bytes(ci.secs, rate_of(clip, 3), ci.rate);
                b20 += adpcm_bytes(ci.secs, rate_of(clip, 2), ci.rate);
                b30 += adpcm_bytes(ci.secs, rate_of(clip, 0), ci.rate);
                b40 += adpcm_bytes(ci.secs, rate_of(clip, 1), ci.rate);
            }
            let _ = writeln!(
                detail,
                "{}\t{}\t{}\t{}\t{}\t{}\t{:.2}\t{:.3}\t{}\t{}\t{:.1}\t{}\t{}",
                sc.name,
                clip,
                ci.cat,
                ported.contains(clip) as u8,
                st.calls,
                st.near,
                if secs > 0.0 {
                    st.calls as f64 * 60.0 / secs
                } else {
                    0.0
                },
                ci.secs,
                ci.rate,
                st.looped as u8,
                st.first as f64 / FPS,
                st.callsite,
                st.hierarchy
            );
        }
        let (pv, pn) = (peak_voices(sc, &info, false), peak_voices(sc, &info, true));
        // how few of the scene-local sounds carry most of the calls: bytes at the m15 policy
        let mut by_calls: Vec<(usize, u64)> = sc
            .stats
            .iter()
            .filter(|(c, st)| {
                !st.looped
                    && info
                        .get(*c)
                        .is_some_and(|i| i.cat != "Persistent" && i.secs <= STREAM_SECS)
            })
            .map(|(c, st)| {
                (
                    st.calls,
                    adpcm_bytes(info[c].secs, rate_of(c, 3), info[c].rate),
                )
            })
            .collect();
        by_calls.sort_by_key(|b| std::cmp::Reverse(b.0));
        let total_calls: usize = by_calls.iter().map(|x| x.0).sum();
        let cover = |share: f64| -> (usize, u64) {
            let (mut n, mut got, mut bytes) = (0, 0usize, 0u64);
            for (c, b) in &by_calls {
                if got as f64 >= share * total_calls as f64 {
                    break;
                }
                got += c;
                bytes += b;
                n += 1;
            }
            (n, bytes)
        };
        let (c90, c99) = (cover(0.90), cover(0.99));
        let (mut fl, mut fb, mut fb20) = (0usize, 0u64, 0u64);
        for clip in known_fsm.get(&sc.scene).into_iter().flatten() {
            let ci = &info[clip];
            if matches!(ci.cat.as_str(), "Local" | "Wide") && ci.secs <= STREAM_SECS {
                fl += 1;
                fb += adpcm_bytes(ci.secs, 5512, ci.rate);
                fb20 += adpcm_bytes(ci.secs, rate_of(clip, 2), ci.rate);
            }
        }
        let (rc, rb) = reach.get(&sc.scene).copied().unwrap_or((0, 0));
        let _ = writeln!(
            summary,
            "{}\t{}\t{:.1}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.1}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.1}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            sc.name, sc.kind, secs, calls, near, sc.stats.len(), local, pers, loops, wide,
            pv.0, pn.0, pv.1, pv.2, pv.3, b22, b55, b15, b20, b30, b40, w55, w15, w20, streamed, stream_secs, c90.0, c90.1, c99.0, c99.1, fl, fb, fb20, rc, rb
        );
    }
    std::fs::write(Path::new(out).join("trace_summary.tsv"), summary).map_err(|e| e.to_string())?;
    std::fs::write(Path::new(out).join("trace_clips.tsv"), detail).map_err(|e| e.to_string())?;
    eprintln!("{} scenarios", scenarios.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(frame: i64, op: &str, clip: &str, source: &str) -> Call {
        Call {
            frame,
            op: op.into(),
            clip: clip.into(),
            callsite: String::new(),
            hierarchy: String::new(),
            source_id: source.into(),
            pitch: 1.0,
            near: true,
        }
    }

    fn info() -> BTreeMap<String, ClipInfo> {
        ["step", "hit"]
            .iter()
            .map(|n| {
                (
                    n.to_string(),
                    ClipInfo {
                        secs: 2.0,
                        rate: 44100,
                        cat: "Local".into(),
                    },
                )
            })
            .collect()
    }

    #[test]
    fn a_source_holds_one_voice_but_one_shots_stack() {
        let info = info();
        // 60 Play calls of one source in a second are one voice; three one-shots overlap as three.
        let mut calls: Vec<Call> = (0..60).map(|f| call(f, "Play", "step", "7")).collect();
        calls.extend((0..3).map(|f| call(f, "PlayOneShot", "hit", "9")));
        let sc = scenario("t", "t", "play", 60, calls, BTreeSet::new(), &info);
        let (peak, distinct, _, _) = peak_voices(&sc, &info, false);
        assert_eq!(peak, 4);
        assert_eq!(distinct, 2);
    }

    #[test]
    fn a_stop_ends_the_voice() {
        let info = info();
        let calls = vec![
            call(0, "Play", "step", "7"),
            call(6, "Stop", "step", "7"),
            call(30, "Play", "step", "7"),
        ];
        let sc = scenario("t", "t", "play", 60, calls, BTreeSet::new(), &info);
        assert_eq!(peak_voices(&sc, &info, false).0, 1);
    }
}
