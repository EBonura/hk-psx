//! Recognize the verified Windows Zombie Swipe/Walker variant, without spawning
//! it. Ported from host/runner.py `recognize` (and the Runner end of
//! host/actors.py `actor_sources`), for the Runner audio cook.
//!
//! The recognition keeps every source check of the Python: the Walker's
//! parameters, the structural FSM fingerprint, the assembly hashes, the body,
//! sensing and animation contracts, and the audio references. What it does not
//! carry is the cooked output (parameters in Q16, clip tables, sprite lists),
//! which only the actor cook reads.
//!
//! `actor_sources` tries one recognizer after another and the first to accept
//! an actor claims it. A Zombie Swipe/Leap actor is only ever claimed by the
//! Runner gate: every earlier gate wants a component or an FSM the Runner does
//! not have (Crawler, Climber, Roller, Hatcher ...) except the three that share
//! its `Walker` and are taken off by name (Mawlek Body, Zombie Shield, Zombie
//! Guard), which are excluded here the same way.

use crate::common::{component_records, err, get, path_id, py_round, Result};
use crate::cook_audio::{sha, u};
use crate::pyjson::{dumps_sorted_compact, Json};
use hk_unity::playmaker::action_fields;
use hk_unity::scene::Scene;
use hk_unity::{Obj, Source, Value};

/// Structural FSM fingerprint (`fsm_fingerprint`): states, transitions, enabled
/// actions and their scalar fields, variables other than Lunge Speed. Every
/// Zombie Swipe placement in the catalog shares it.
const FSM_SHA256: &str = "73f11594e0115a43a66c8d1695a65916b81bfe6057c180636eed3330aaad92c2";
/// Zombie Leap (Leaper): same Walker, a leap attack instead of the swipe.
const LEAP_FSM_SHA256: &str = "15ecb1c0984cd4955e5412dfae354441eb9a9d19c6b2bff9d7b6fc564dc7fe47";
type Clips = [(&'static str, i64)];
const LEAP_CLIPS: &Clips = &[("Idle", 0), ("Walk", 0), ("Turn", 2), ("Attack", 2), ("Land", 2), ("Death Air", 6), ("Death Land", 2)];
const CLIPS: &Clips = &[("Idle", 0), ("Walk", 0), ("Turn", 2), ("Attack Anticipate", 2), ("Attack Lunge", 2), ("Attack Cooldown", 2), ("Fall", 1), ("Death Air", 6), ("Death Land", 2)];
/// The managed assemblies the recognized methods were audited against.
pub const ASSEMBLIES: [(&str, &str); 4] = [
    ("Assembly-CSharp.dll", "e9048ef6a633970f735e01ec166d3959f610eaea7a88d827d48d67b1e5fb87bd"),
    ("PlayMaker.dll", "0ef0e7829d125e1f632c8a189260ec6c6882630be6932c8c7ae032efbc53469a"),
    ("TeamCherry.TK2D.dll", "b443474e6cf6eb03debe5346884a51893621cc39a9b2666c160034fbd5783da7"),
    ("Assembly-CSharp-firstpass.dll", "2c9b97488f3f8d2e29c8af2e3200e378216652904be22c2678eb5d2cdaa95499"),
];

fn is_float(v: &Value) -> bool {
    matches!(v, Value::F32(_) | Value::F64(_))
}
fn eq_num(v: Option<&Value>, x: f64) -> bool {
    v.is_some_and(|v| match v {
        Value::Str(_) | Value::Bytes(_) | Value::List(_) | Value::Map(_) => false,
        other => other.float() == Some(x) || other.int().map(|i| i as f64) == Some(x),
    })
}
fn float_in(v: Option<&Value>, lo_exclusive: f64, hi_inclusive: f64) -> Option<f64> {
    let v = v.filter(|v| is_float(v))?;
    let f = v.float()?;
    (lo_exclusive < f && f <= hi_inclusive).then_some(f)
}

/// Python's `repr()` of a value the type trees hand the cookers.
pub fn py_value_repr(v: &Value) -> String {
    match v {
        Value::Bool(b) => if *b { "True" } else { "False" }.into(),
        Value::Int(i) => i.to_string(),
        Value::UInt(i) => i.to_string(),
        Value::F32(f) => crate::pyfloat::repr(*f as f64),
        Value::F64(f) => crate::pyfloat::repr(*f),
        Value::Str(s) => crate::music_report::py_repr(&String::from_utf8_lossy(s)),
        Value::Bytes(b) => format!("b'{}'", b.iter().map(|x| format!("\\x{x:02x}")).collect::<String>()),
        Value::List(l) => format!("[{}]", l.iter().map(py_value_repr).collect::<Vec<_>>().join(", ")),
        Value::Map(m) => format!("{{{}}}", m.iter().map(|(k, x)| format!("{}: {}", crate::music_report::py_repr(k), py_value_repr(x))).collect::<Vec<_>>().join(", ")),
    }
}

/// `walker_parameters`: reject C# controller variants the runner model does not
/// represent. Walk speed, the pause wait/time ranges and the FSM `Lunge Speed`
/// are the placement parameters; every other Walker field must match the
/// audited Runner.
fn walker_parameters(walker: &Value, lunge_speed: Option<&Value>) -> Result<()> {
    let expected: [(&str, Value); 16] = [
        ("rightScale", Value::F64(-1.0)),
        ("edgeXAdjuster", Value::F64(0.0)),
        ("turnPause", Value::F64(1.0)),
        ("turnAfterIdlePercentage", Value::Int(0)),
        ("pauses", Value::Int(1)),
        ("idleClip", Value::Str(b"Idle".to_vec())),
        ("walkClip", Value::Str(b"Walk".to_vec())),
        ("turnClip", Value::Str(b"Turn".to_vec())),
        ("ambush", Value::Int(0)),
        ("startInactive", Value::Int(0)),
        ("waitForHeroX", Value::Int(0)),
        ("preventTurn", Value::Int(0)),
        ("ignoreHoles", Value::Int(0)),
        ("preventTurningToFaceHero", Value::Int(0)),
        ("preventScaleChange", Value::Int(0)),
        ("m_Enabled", Value::Int(1)),
    ];
    for (name, value) in &expected {
        if !walker.get(name).is_some_and(|w| w.py_eq(value)) {
            return err(format!("unsupported Runner Walker field: {name}"));
        }
    }
    let speed = float_in(walker.get("walkSpeedR"), 0.0, 16.0).filter(|s| walker.get("walkSpeedL").and_then(Value::float) == Some(-s));
    if speed.is_none() {
        return err("unsupported Runner Walker field: walkSpeedL");
    }
    if float_in(lunge_speed, 0.0, 32.0).is_none() {
        return err("unsupported Runner lunge speed");
    }
    for (low, high) in [("pauseWaitMin", "pauseWaitMax"), ("pauseTimeMin", "pauseTimeMax")] {
        for key in [low, high] {
            if float_in(walker.get(key), 0.0, 10.0).is_none() {
                return err(format!("unsupported Runner Walker field: {low}"));
            }
        }
    }
    Ok(())
}

/// `fsm_fingerprint`: structure and scalar parameters of the Zombie Swipe FSM,
/// without the per-scene object references and the placement's Lunge Speed.
fn fsm_fingerprint(fsm: &Value) -> Result<String> {
    let scalar = |v: &Value| -> String {
        if v.is_map() {
            if v.get("useVariable").is_some_and(Value::truthy) {
                format!("VAR:{}", v.get("name").and_then(Value::str).unwrap_or_default())
            } else {
                match v.get("value").or_else(|| v.get("name")) {
                    Some(x) => py_value_repr(x),
                    None => "None".into(),
                }
            }
        } else {
            py_value_repr(v)
        }
    };
    let js = |s: String| Json::Str(s);
    let pair = |a: String, b: String| Json::List(vec![js(a), js(b)]);
    let mut variables: Vec<(String, String)> = Vec::new();
    if let Some(Value::Map(groups)) = fsm.get("variables") {
        for (_, group) in groups {
            for v in group.list().unwrap_or(&[]) {
                if v.is_map() && v.get("name").is_some() && v.get("value").is_some_and(|x| !x.is_map()) && v.get("name").and_then(Value::str).as_deref() != Some("Lunge Speed") {
                    variables.push((v.get("name").and_then(Value::str).unwrap_or_default(), py_value_repr(v.get("value").unwrap())));
                }
            }
        }
    }
    variables.sort();
    let transitions = |list: Option<&Value>| -> Result<Vec<Json>> {
        list.and_then(Value::list)
            .unwrap_or(&[])
            .iter()
            .map(|t| Ok(pair(get(get(t, "fsmEvent")?, "name")?.str().unwrap_or_default(), get(t, "toState")?.str().unwrap_or_default())))
            .collect()
    };
    let mut states = Vec::new();
    for state in get(fsm, "states")?.list().ok_or("states is not a list")? {
        let data = get(state, "actionData")?;
        let names = get(data, "actionNames")?.list().unwrap_or(&[]);
        let enabled = get(data, "actionEnabled")?.list().unwrap_or(&[]);
        let mut actions = Vec::new();
        for (index, name) in names.iter().enumerate() {
            let mut fields: Vec<(String, String)> = match action_fields(data, index, false) {
                Ok(f) => f.into_iter().filter(|(k, _)| k != "gameObject").map(|(k, v)| (k, scalar(&v))).collect(),
                Err(e) => vec![("error".into(), e.to_string())],
            };
            fields.sort();
            actions.push(Json::List(vec![js(name.str().unwrap_or_default()), Json::Int(enabled.get(index).and_then(Value::int).unwrap_or(0)), Json::List(fields.into_iter().map(|(k, v)| pair(k, v)).collect())]));
        }
        states.push(Json::List(vec![js(get(state, "name")?.str().unwrap_or_default()), Json::List(transitions(state.get("transitions"))?), Json::List(actions)]));
    }
    let summary = Json::Obj(vec![
        ("name".into(), js(get(fsm, "name")?.str().unwrap_or_default())),
        ("start".into(), js(get(fsm, "startState")?.str().unwrap_or_default())),
        ("variables".into(), Json::List(variables.into_iter().map(|(a, b)| pair(a, b)).collect())),
        ("globals".into(), Json::List(transitions(fsm.get("globalTransitions"))?)),
        ("states".into(), Json::List(states)),
    ]);
    Ok(sha(dumps_sorted_compact(&summary).as_bytes()))
}

/// `clip_contract`: every Zombie Swipe library carries the same clip set with
/// the same wrap modes; frame counts and rates differ per variant. `Fall` is
/// optional (the controller never plays it).
fn clip_contract(clips: &[Value], expected: &Clips) -> Result<()> {
    let names: Vec<String> = clips.iter().map(|c| get(c, "name").ok().and_then(Value::str).unwrap_or_default()).collect();
    let mut unique = names.clone();
    unique.sort();
    unique.dedup();
    let required = expected.iter().filter(|e| e.0 != "Fall");
    if unique.len() != names.len() || !required.clone().all(|e| names.iter().any(|n| n == e.0)) || !names.iter().all(|n| expected.iter().any(|e| e.0 == n)) {
        return err("unsupported Runner animation inventory");
    }
    for (name, wrap) in expected {
        let Some(clip) = clips.iter().zip(&names).find(|(_, n)| n == name).map(|(c, _)| c) else { continue };
        let frames = get(clip, "frames")?.list().unwrap_or(&[]);
        let fps = get(clip, "fps")?.float().unwrap_or(0.0);
        if get(clip, "wrapMode")?.int() != Some(*wrap) || frames.is_empty() || fps <= 0.0 {
            return err(format!("unsupported Runner animation: {name}"));
        }
        // The Leaper's Attack trigger frame is the launch cue.
        if *name != "Attack" && frames.iter().any(|f| f.get("triggerEvent").is_some_and(Value::truthy)) {
            return err(format!("unsupported Runner frame event: {name}"));
        }
        let loop_start = get(clip, "loopStart")?.int().unwrap_or(-1);
        if !(0 <= loop_start && (loop_start as usize) < frames.len()) {
            return err(format!("invalid Runner loop start: {name}"));
        }
    }
    Ok(())
}

/// `body_contract`: reject physics variants the pending actor integration cannot honor.
fn body_contract(rigid: &Value) -> Result<()> {
    let bits = |_: ()| Value::Map(vec![("m_Bits".into(), Value::Int(0))]);
    let expected: [(&str, Value); 12] = [
        ("m_BodyType", Value::Int(0)),
        ("m_Simulated", Value::Bool(true)),
        ("m_UseAutoMass", Value::Bool(false)),
        ("m_UseFullKinematicContacts", Value::Bool(false)),
        ("m_Mass", Value::F64(1.0)),
        ("m_LinearDamping", Value::F64(0.0)),
        ("m_Interpolate", Value::Int(0)),
        ("m_SleepingMode", Value::Int(1)),
        ("m_Constraints", Value::Int(4)),
        ("m_Material", Value::Map(vec![("m_FileID".into(), Value::Int(0)), ("m_PathID".into(), Value::Int(0))])),
        ("m_IncludeLayers", bits(())),
        ("m_ExcludeLayers", bits(())),
    ];
    for (name, value) in &expected {
        if !rigid.get(name).is_some_and(|r| r.py_eq(value)) {
            return err(format!("unsupported Runner rigid body field: {name}"));
        }
    }
    // Hornheads use continuous collision detection; the swept solver covers both.
    if !rigid.get("m_CollisionDetection").and_then(Value::int).is_some_and(|v| v == 0 || v == 1) {
        return err("unsupported Runner rigid body field: m_CollisionDetection");
    }
    // Runners fall at gravity scale 1, Leapers at .8.
    if !rigid.get("m_GravityScale").and_then(Value::float).is_some_and(|g| g == 1.0 || g == 0.800000011920929) {
        return err("unsupported Runner rigid body field: m_GravityScale");
    }
    Ok(())
}

/// `axis_aligned_bounds`: transform all four collider corners, retaining child
/// scaling and offsets.
fn axis_aligned_bounds(m: &[[f64; 4]; 4], offset: [f64; 2], size: [f64; 2]) -> Result<[f64; 4]> {
    if m.iter().flatten().any(|v| !v.is_finite()) {
        return err("nonfinite Runner collider transform");
    }
    if [(0, 1), (1, 0), (0, 2), (1, 2)].iter().any(|&(r, c)| m[r][c].abs() > 1e-6) {
        return err("rotated Runner collider unsupported");
    }
    if m[0][0] == 0.0 || m[1][1] == 0.0 {
        return err("degenerate Runner collider transform");
    }
    if offset.iter().chain(&size).any(|v| !v.is_finite()) || size[0].min(size[1]) <= 0.0 {
        return err("invalid Runner collider extent");
    }
    let points: Vec<[f64; 2]> = [(-1.0, -1.0), (-1.0, 1.0), (1.0, -1.0), (1.0, 1.0)]
        .iter()
        .map(|s: &(f64, f64)| {
            let sign = [s.0, s.1];
            [0, 1].map(|i| m[i][3] + m[i][i] * (offset[i] + sign[i] * size[i] / 2.0))
        })
        .collect();
    let min = |i: usize| points.iter().map(|p| p[i]).fold(f64::INFINITY, f64::min);
    let max = |i: usize| points.iter().map(|p| p[i]).fold(f64::NEG_INFINITY, f64::max);
    Ok([min(0), min(1), max(0), max(1)])
}

fn one<'a>(records: &[(i64, &str, &'a Value)], kind: &str) -> Result<(i64, &'a Value)> {
    let matches: Vec<_> = records.iter().filter(|r| r.1 == kind).collect();
    if matches.len() != 1 {
        return err(format!("Runner requires exactly one {kind}"));
    }
    Ok((matches[0].0, matches[0].2))
}

/// `driving_fsm`: the Walker's own FSM, chosen by name. A placement may carry a
/// second FSM that drives no movement (`enemy_corpse`), so the number of
/// PlayMakerFSM components is not the contract; exactly one movement FSM is.
fn driving_fsm<'a>(records: &[(i64, &str, &'a Value)]) -> Result<&'a Value> {
    let matches: Vec<&Value> = records
        .iter()
        .filter(|r| r.1 == "PlayMakerFSM" && r.2.get("fsm").and_then(|f| f.get("name")).and_then(Value::str).is_some_and(|n| n == "Zombie Swipe" || n == "Zombie Leap"))
        .map(|r| r.2)
        .collect();
    if matches.len() != 1 {
        return err("Runner requires exactly one Zombie Swipe or Zombie Leap FSM");
    }
    Ok(matches[0])
}

/// The audio references of a recognized Runner (`recognize()['audio_sources']`).
#[derive(Debug, Clone, PartialEq)]
pub struct Audio {
    pub walk_loop: String,
    pub chase: Vec<String>,
    pub looped: bool,
    pub volume: f64,
    pub initial_pitch: f64,
    pub play_on_awake: bool,
}

fn local_ref(r: &Value) -> Result<i64> {
    let (file, path) = r.pptr().ok_or("not a PPtr")?;
    if file != 0 || path == 0 {
        return err("external or missing Runner sensing component");
    }
    Ok(path)
}

/// `recognize`: the Runner's audio bindings, or the first source check it fails.
pub fn recognize(sc: &Scene, source: &Source, gid: i64, position: [f64; 3]) -> Result<Audio> {
    let records = component_records(sc, gid);
    let (walker_id, walker) = one(&records, "Walker")?;
    let fsm_component = driving_fsm(&records)?;
    let fsm = get(fsm_component, "fsm")?;
    let variables = {
        let mut out: Vec<(String, &Value)> = Vec::new();
        if let Some(Value::Map(groups)) = fsm.get("variables") {
            for (_, group) in groups {
                for v in group.list().unwrap_or(&[]) {
                    if v.is_map() && v.get("name").is_some() && v.get("value").is_some_and(|x| !x.is_map()) {
                        let n = v.get("name").and_then(Value::str).unwrap_or_default();
                        match out.iter_mut().find(|(k, _)| *k == n) {
                            Some(slot) => slot.1 = v.get("value").unwrap(),
                            None => out.push((n, v.get("value").unwrap())),
                        }
                    }
                }
            }
        }
        out
    };
    let variable = |name: &str| variables.iter().find(|(k, _)| k == name).map(|(_, v)| *v);
    let leap = get(fsm, "name")?.str().as_deref() == Some("Zombie Leap");
    let one_float = Value::F64(1.0);
    walker_parameters(walker, if leap { Some(&one_float) } else { variable("Lunge Speed") })?;
    let _ = walker_id;
    // The serialized FSM embeds owner references, so the fingerprint covers its
    // structure and scalar parameters.
    let fingerprint = fsm_fingerprint(fsm)?;
    if !get(fsm_component, "m_Enabled")?.truthy() || fingerprint != if leap { LEAP_FSM_SHA256 } else { FSM_SHA256 } {
        return err("unverified Runner FSM variant");
    }
    if leap && !eq_num(variable("Idle Time"), 0.5) {
        return err("unsupported Leaper idle time");
    }
    for (name, expected) in ASSEMBLIES {
        let bytes = std::fs::read(source.directory.join("Managed").join(name)).map_err(|e| format!("{name}: {e}"))?;
        if sha(&bytes) != expected {
            return err(format!("Runner methods require a fresh source audit: {name}"));
        }
    }
    let matrix = u(sc.world(*sc.go_transform.get(&gid).ok_or("actor has no transform")?))?;
    // Placements are authored facing left, except where the transform carries a
    // plain x mirror; anything else (a rotation, a non-unit magnitude, a y flip)
    // stays refused.
    if position[2].abs() > 0.01 {
        return err("Runner depth differs from guest source plane");
    }
    let layer = get(sc.go(gid).ok_or("no such GameObject")?, "m_Layer")?.int();
    if layer != Some(11) || (matrix[0][0].abs() - 1.0).abs() > 1e-6 || (matrix[1][1] - 1.0).abs() > 1e-6 || matrix[0][1].abs() > 1e-6 || matrix[1][0].abs() > 1e-6 {
        return err("unsupported Runner layer or initial scale");
    }
    let (_, body) = one(&records, "BoxCollider2D")?;
    let (_, rigid) = one(&records, "Rigidbody2D")?;
    if !get(body, "m_Enabled")?.truthy() || get(body, "m_IsTrigger")?.truthy() || !eq_num(body.get("m_EdgeRadius"), 0.0) {
        return err("unsupported Runner body collider");
    }
    body_contract(rigid)?;
    let xy = |v: &Value, key: &str| -> Result<[f64; 2]> {
        let p = get(v, key)?;
        Ok([get(p, "x")?.float().ok_or("not a number")?, get(p, "y")?.float().ok_or("not a number")?])
    };
    let body_bounds = axis_aligned_bounds(&matrix, xy(body, "m_Offset")?, xy(body, "m_Size")?)?;
    let (los_id, los) = one(&records, "LineOfSightDetector")?;
    let range_id = local_ref(get(walker, "alertRange")?)?;
    let ranges: Vec<i64> = get(los, "alertRanges")?.list().unwrap_or(&[]).iter().map(local_ref).collect::<Result<_>>()?;
    if local_ref(get(walker, "lineOfSightDetector")?)? != los_id || !get(los, "m_Enabled")?.truthy() || ranges != [range_id] {
        return err("Runner sensing references differ");
    }
    let alert_object = sc.object(range_id).ok_or("alert range is not in the scene")?;
    if alert_object.typename != "AlertRange" || !get(&alert_object.tree, "m_Enabled")?.truthy() {
        return err("Runner alert component disabled or changed");
    }
    let range_go = local_ref(get(&alert_object.tree, "m_GameObject")?)?;
    if !sc.active(range_go) {
        return err("Runner alert object inactive");
    }
    let range_records = component_records(sc, range_go);
    let (_, collider) = one(&range_records, "BoxCollider2D")?;
    if !get(collider, "m_Enabled")?.truthy() || !get(collider, "m_IsTrigger")?.truthy() || !eq_num(collider.get("m_EdgeRadius"), 0.0) {
        return err("unsupported Runner alert trigger");
    }
    let alert_bounds = axis_aligned_bounds(&u(sc.world(*sc.go_transform.get(&range_go).ok_or("alert object has no transform")?))?, xy(collider, "m_Offset")?, xy(collider, "m_Size")?)?;
    let (_, animator) = one(&records, "tk2dSpriteAnimator")?;
    if !get(animator, "m_Enabled")?.truthy() || get(animator, "isRealtime")?.truthy() {
        return err("Runner requires enabled scaled-time animation");
    }
    let library = u(sc.deref(get(animator, "library")?))?;
    let library_tree = u(source.read(&library))?;
    let clips = get(&library_tree, "clips")?.list().ok_or("clips is not a list")?;
    clip_contract(clips, if leap { LEAP_CLIPS } else { CLIPS })?;
    if leap {
        let attack = clips.iter().find(|c| c.get("name").and_then(Value::str).as_deref() == Some("Attack")).ok_or("no Attack clip")?;
        let triggers = get(attack, "frames")?.list().unwrap_or(&[]).iter().filter(|f| f.get("triggerEvent").is_some_and(Value::truthy)).count();
        if triggers != 1 {
            return err("Leaper Attack clip needs exactly one trigger frame");
        }
    }
    let mut sprites = std::collections::BTreeSet::new();
    for clip in clips {
        for frame in get(clip, "frames")?.list().unwrap_or(&[]) {
            let collection = u(source.deref(&library.file, get(frame, "spriteCollection")?))?;
            sprites.insert((collection.sid(), get(frame, "spriteId")?.int().unwrap_or(0)));
        }
    }
    if sprites.is_empty() {
        return err("Runner sprite inventory empty");
    }
    let (_, audio_source) = one(&records, "AudioSource")?;
    // Unity 6 stores the assigned loop in m_Resource; m_audioClip is null here.
    let looped = u(sc.deref(get(audio_source, "m_Resource")?))?;
    let anticipate = get(
        get(fsm, "states")?.list().unwrap_or(&[]).iter().find(|s| s.get("name").and_then(Value::str).as_deref() == Some("Anticipate")).ok_or("no Anticipate state")?,
        "actionData",
    )?;
    let chase: Vec<Obj> = get(anticipate, "unityObjectParams")?.list().unwrap_or(&[]).iter().map(|r| u(sc.deref(r))).collect::<Result<_>>()?;
    if chase.len() != 2 || [&looped, &chase[0], &chase[1]].iter().any(|o| o.class_id() != 83) {
        return err("Runner audio references differ from the verified FSM");
    }
    let audio = Audio {
        walk_loop: looped.sid(),
        chase: chase.iter().map(Obj::sid).collect(),
        looped: get(audio_source, "Loop")?.truthy(),
        volume: get(audio_source, "m_Volume")?.float().ok_or("m_Volume")?,
        initial_pitch: get(audio_source, "m_Pitch")?.float().ok_or("m_Pitch")?,
        play_on_awake: get(audio_source, "m_PlayOnAwake")?.truthy(),
    };
    let relative_q16 = |bounds: [f64; 4]| -> Result<[i64; 4]> {
        let origin = [position[0], position[1]];
        let out = [0, 1, 2, 3].map(|i| py_round((bounds[i] - origin[i % 2]) * 65536.0));
        if out.iter().any(|v| v.abs() > 16 * 65536) {
            return err("Runner local bounds exceed Q16 contract");
        }
        Ok(out)
    };
    let (body_q16, alert_q16) = (relative_q16(body_bounds)?, relative_q16(alert_bounds)?);
    // The level37 Runner shape is hk_sim::runner_senses::Shape::RUNNER.
    if alert_q16[0] != -alert_q16[2] || body_q16[0] >= body_q16[2] || body_q16[1] >= body_q16[3] || alert_q16[1] >= alert_q16[3] {
        return err("Runner sensing shape is not a mirror-stable box");
    }
    Ok(audio)
}

/// A recognized Zombie Swipe actor.
pub struct RunnerActor {
    pub source: String,
    pub game_object: i64,
    pub audio: Audio,
}

/// `actor_sources(scene)` filtered to the actors whose movement control is a
/// ZombieSwipeWalker: every enabled, active HealthManager the Runner gate
/// accepts.
pub fn runner_actors(sc: &Scene, source: &Source) -> Result<Vec<RunnerActor>> {
    let mut out = Vec::new();
    for o in &sc.objects {
        if o.typename != "HealthManager" || !get(&o.tree, "m_Enabled")?.truthy() {
            continue;
        }
        let gid = path_id(get(&o.tree, "m_GameObject")?).unwrap_or(0);
        if !sc.active(gid) {
            continue;
        }
        let records = component_records(sc, gid);
        if !records.iter().any(|r| r.1 == "Walker") {
            continue;
        }
        // Taken off the Runner's gate by name, as in actor_sources.
        let name = get(sc.go(gid).ok_or("no such GameObject")?, "m_Name")?.str().unwrap_or_default();
        if name == "Mawlek Body" || name.starts_with("Zombie Shield") || name.starts_with("Zombie Guard") {
            continue;
        }
        let position = u(sc.point(gid, 0.0, 0.0, 0.0))?;
        if let Ok(audio) = recognize(sc, source, gid, position) {
            out.push(RunnerActor { source: sc.sid(o.id), game_object: gid, audio });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cook_audio::tests::{int, list, map, text};

    #[test]
    fn python_reprs_follow_the_value_kinds() {
        assert_eq!(py_value_repr(&Value::Bool(true)), "True");
        assert_eq!(py_value_repr(&Value::F32(0.85)), "0.8500000238418579");
        assert_eq!(py_value_repr(&text("Idle")), "'Idle'");
        assert_eq!(py_value_repr(&list(vec![int(1), text("a")])), "[1, 'a']");
        assert_eq!(py_value_repr(&map(vec![("m_FileID", int(0))])), "{'m_FileID': 0}");
    }

    #[test]
    fn a_walker_must_match_the_audited_runner() {
        let mut fields: Vec<(&str, Value)> = vec![
            ("rightScale", Value::F32(-1.0)),
            ("edgeXAdjuster", Value::F32(0.0)),
            ("turnPause", Value::F32(1.0)),
            ("turnAfterIdlePercentage", int(0)),
            ("pauses", int(1)),
            ("idleClip", text("Idle")),
            ("walkClip", text("Walk")),
            ("turnClip", text("Turn")),
            ("ambush", int(0)),
            ("startInactive", int(0)),
            ("waitForHeroX", int(0)),
            ("preventTurn", int(0)),
            ("ignoreHoles", int(0)),
            ("preventTurningToFaceHero", int(0)),
            ("preventScaleChange", int(0)),
            ("m_Enabled", int(1)),
            ("walkSpeedR", Value::F32(1.5)),
            ("walkSpeedL", Value::F32(-1.5)),
            ("pauseWaitMin", Value::F32(1.0)),
            ("pauseWaitMax", Value::F32(2.0)),
            ("pauseTimeMin", Value::F32(1.0)),
            ("pauseTimeMax", Value::F32(2.0)),
        ];
        let lunge = Value::F32(6.0);
        assert!(walker_parameters(&map(fields.clone()), Some(&lunge)).is_ok());
        assert!(walker_parameters(&map(fields.clone()), None).is_err());
        assert!(walker_parameters(&map(fields.clone()), Some(&int(6))).is_err());
        fields[4] = ("pauses", int(0));
        assert!(walker_parameters(&map(fields.clone()), Some(&lunge)).unwrap_err().contains("pauses"));
        fields[4] = ("pauses", int(1));
        fields[17] = ("walkSpeedL", Value::F32(-1.0));
        assert!(walker_parameters(&map(fields), Some(&lunge)).unwrap_err().contains("walkSpeedL"));
    }

    #[test]
    fn clip_inventory_and_wrap_modes_are_checked() {
        let clip = |name: &str, wrap: i64, trigger: bool| {
            map(vec![
                ("name", text(name)),
                ("frames", list(vec![map(vec![("triggerEvent", Value::Bool(trigger))])])),
                ("fps", Value::F32(12.0)),
                ("wrapMode", int(wrap)),
                ("loopStart", int(0)),
            ])
        };
        let full: Vec<Value> = CLIPS.iter().filter(|c| c.0 != "Fall").map(|c| clip(c.0, c.1, false)).collect();
        assert!(clip_contract(&full, CLIPS).is_ok());
        let mut missing = full.clone();
        missing.pop();
        assert!(clip_contract(&missing, CLIPS).is_err());
        let mut wrong_wrap = full.clone();
        wrong_wrap[0] = clip("Idle", 2, false);
        assert!(clip_contract(&wrong_wrap, CLIPS).is_err());
        let mut eventful = full.clone();
        eventful[0] = clip("Idle", 0, true);
        assert!(clip_contract(&eventful, CLIPS).unwrap_err().contains("frame event"));
        let mut extra = full;
        extra.push(clip("Dance", 0, false));
        assert!(clip_contract(&extra, CLIPS).is_err());
    }

    #[test]
    fn collider_corners_follow_the_transform() {
        let identity = [[1.0, 0.0, 0.0, 10.0], [0.0, 1.0, 0.0, 20.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        assert_eq!(axis_aligned_bounds(&identity, [1.0, 0.0], [2.0, 4.0]).unwrap(), [10.0, 18.0, 12.0, 22.0]);
        let mut rotated = identity;
        rotated[0][1] = 0.5;
        assert!(axis_aligned_bounds(&rotated, [0.0; 2], [1.0; 2]).is_err());
        assert!(axis_aligned_bounds(&identity, [0.0; 2], [0.0, 1.0]).is_err());
    }
}
