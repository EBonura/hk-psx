//! Recognize the Husk Guard (`Zombie Guard`) of Crossroads_21 and Crossroads_48.
//! Ported from host/husk_guard.py (and the false_knight_art.py prefab helpers it uses).
//!
//! The controller lives in shared/hk-sim/src/husk_guard.rs; this module admits a
//! placement only when its serialized shape matches the one read to write that
//! controller, and proves that the prefab constants the controller carries are this
//! placement's own. `Zombie Guard` has no Walker: one 48-state FSM owns every velocity.

use crate::common::{component_records, err, get, py_round, Result};
use crate::cook_audio::{jobj, js, u};
use crate::prefab::{
    actions_of, enum_param, literal, one_of, prefab_box, prefab_parts, single_state,
};
use crate::pyjson::Json;
use crate::recog::{body_box, check_assemblies, child_map, clips_ok, xy};
use crate::runner::{axis_aligned_bounds, fsm_fingerprint, ticks};
use hk_unity::playmaker::field;
use hk_unity::scene::Scene;
use hk_unity::{Obj, Source, Value};

const FSM_NAME: &str = "Zombie Guard";
const FSM_SHA256: &str = "d90edca6658df5ec636075f6c355dbb1b2673c0db62af98c447b09aba317a870";
/// name: (frames, fps, wrapMode).
const CLIPS: [(&str, usize, f64, i64, Option<i64>); 17] = [
    ("Walk", 10, 12.0, 0, None),
    ("Turn", 2, 12.0, 2, None),
    ("Dormant", 1, 30.0, 6, None),
    ("Wake", 6, 12.0, 2, None),
    ("Idle", 7, 12.0, 0, None),
    ("Run", 6, 10.0, 0, None),
    ("Stop Run", 6, 12.0, 2, None),
    ("Stop Walk", 2, 12.0, 2, None),
    ("Anticipate", 6, 12.0, 2, None),
    ("Attack2", 7, 15.0, 2, None),
    ("Startle", 4, 12.0, 2, None),
    ("Stomp Antic", 3, 12.0, 1, None),
    ("Stomp Jump", 4, 12.0, 2, None),
    ("Stomp Land", 6, 12.0, 2, None),
    ("Death Stun", 1, 30.0, 6, None),
    ("Death Air", 3, 12.0, 2, None),
    ("Death Land", 9, 12.0, 2, None),
];
/// `husk_guard::Clip::slot` order with the clip each names; Walk and Turn are the shared slots.
pub(crate) const CLIP_SLOTS: [(&str, &str); 12] = [
    ("dormant", "Dormant"),
    ("wake", "Wake"),
    ("idle", "Idle"),
    ("run", "Run"),
    ("stop_run", "Stop Run"),
    ("stop_walk", "Stop Walk"),
    ("anticipate", "Anticipate"),
    ("attack", "Attack2"),
    ("startle", "Startle"),
    ("stomp_antic", "Stomp Antic"),
    ("stomp_jump", "Stomp Jump"),
    ("stomp_land", "Stomp Land"),
];
const ALERT_Q16: [i64; 4] = [-1102971, -259850, 1102971, 181207];
const ATTACK_Q16: [i64; 4] = [-327680, -246088, 327680, 172687];
const OVERHEAD_Q16: [i64; 4] = [-101253, 12880, 101253, 244406];
const SWIPE_Q16: [i64; 4] = [-323584, -70656, 142336, 299008];
const WAVE_SPEED: f64 = 18.0;
const WAVE_SCALE: f64 = 1.25;
const FRAME_STRIDE: [&str; 6] = ["Walk", "Idle", "Run", "Stop Run", "Wake", "Stomp Land"];
/// Scenes whose room stream cannot take the guard's bank.
const BANK_REFUSED: [(&str, &str); 1] = [("Crossroads_21", "Crossroads_21 region stream (~76 KiB) + its other actors (~46 KiB) + the guard (~197 KiB) exceed the 256 KiB room bank")];

fn q(v: f64) -> i64 {
    py_round(v * 65536.0)
}

fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Bool(b) => Some(*b as i64 as f64),
        o => o.float(),
    }
}

/// The numbers `husk_guard.rs` carries for the stomp shockwave, and where they came from.
struct Wave {
    start_speed: i64,
    accel: i64,
    wave_box: [i64; 4],
    ground_ray: i64,
    spurt_box: [i64; 4],
    damage_from: i64,
    damage_to: i64,
    damage: i64,
    spurt_ticks: i64,
    library: (String, i64),
    clip: String,
    sources: (String, String),
}

/// `Land`'s pooled `Shockwave Wave` and the `Shockwave Spurt` it leaves.
fn wave(sc: &Scene, source: &Source, fsm: &Value) -> Result<Wave> {
    let land = single_state(fsm, "Land")?;
    let mut waves = Vec::new();
    for (f, _) in actions_of(land, "SpawnObjectFromGlobalPool")? {
        let target = field(&f, "gameObject")
            .and_then(|g| g.get("value"))
            .ok_or("no spawn target")?
            .clone();
        if get(&u(source.read(&u(sc.deref(&target))?))?, "m_Name")?
            .str()
            .as_deref()
            == Some("Shockwave Wave")
        {
            waves.push(target);
        }
    }
    if waves.len() != 2 {
        return err("Land no longer sends two shockwaves");
    }
    let mut speeds = Vec::new();
    for (f, _) in actions_of(land, "SetFsmFloat")? {
        speeds.push(literal(&f, "setValue")?);
    }
    let mut scales = Vec::new();
    for (f, _) in actions_of(land, "SetScale")? {
        scales.push(literal(&f, "x")?);
    }
    let only = |v: &[f64], x: f64| !v.is_empty() && v.iter().all(|e| *e == x);
    if !only(&speeds, WAVE_SPEED) || !only(&scales, WAVE_SCALE) {
        return err("Land changed its shockwave speed or scale");
    }
    let wave_o = u(sc.deref(&waves[0]))?;
    let wave_parts = prefab_parts(source, &wave_o)?;
    let wfsm = wave_parts
        .iter()
        .find(|p| {
            p.0 == "PlayMakerFSM"
                && p.2
                    .get("fsm")
                    .and_then(|f| f.get("name"))
                    .and_then(Value::str)
                    .as_deref()
                    == Some("shockwave")
        })
        .and_then(|p| p.2.get("fsm"))
        .ok_or("no shockwave FSM")?;
    let start = single_state(wfsm, "Start Move")?;
    let mut operators = actions_of(start, "FloatOperator")?;
    if operators.len() != 1 {
        return err("Start Move needs exactly one FloatOperator");
    }
    let (operator, op_index) = operators.remove(0);
    if enum_param(start, op_index, "operation")? != Some(2) {
        return err("Start Move no longer multiplies the incrementer");
    }
    let accel_factor = literal(&operator, "float2")?;
    let start_factor = literal(&one_of(start, "FloatMultiplyV2")?, "multiplyBy")?;
    let ray = literal(
        &one_of(single_state(wfsm, "Move")?, "RayCast2d")?,
        "distance",
    )?;
    let mut spurt_o: Option<Obj> = None;
    let right = get(single_state(wfsm, "Right")?, "actionData")?;
    let names = get(right, "actionNames")?.list().unwrap_or(&[]);
    let enabled = get(right, "actionEnabled")?.list().unwrap_or(&[]);
    let ints = |k: &str| -> Vec<i64> {
        right
            .get(k)
            .and_then(Value::list)
            .unwrap_or(&[])
            .iter()
            .map(|x| x.int().unwrap_or(0))
            .collect()
    };
    let (starts, types, pos) = (
        ints("actionStartIndex"),
        ints("paramDataType"),
        ints("paramDataPos"),
    );
    let params = get(right, "paramName")?.list().unwrap_or(&[]).len();
    let gos = get(right, "fsmGameObjectParams")?.list().unwrap_or(&[]);
    for (i, nm) in names.iter().enumerate() {
        if nm.str().is_some_and(|s| s.ends_with("SetGameObject"))
            && enabled.get(i).is_some_and(Value::truthy)
        {
            let end = if i + 1 < names.len() {
                starts[i + 1] as usize
            } else {
                params
            };
            for j in starts[i] as usize..end {
                if types[j] == 19 {
                    if let Some(v) = gos.get(pos[j] as usize).and_then(|g| g.get("value")) {
                        if v.is_map() && v.get("m_PathID").and_then(Value::int).unwrap_or(0) != 0 {
                            spurt_o = Some(u(source.deref(&wave_o.file, v))?);
                        }
                    }
                }
            }
        }
    }
    let Some(spurt_o) = spurt_o else {
        return err("the wave no longer names its spurt");
    };
    let spurt_parts = prefab_parts(source, &spurt_o)?;
    let timing = spurt_parts
        .iter()
        .find(|p| {
            p.0 == "PlayMakerFSM"
                && p.2
                    .get("fsm")
                    .and_then(|f| f.get("name"))
                    .and_then(Value::str)
                    .as_deref()
                    == Some("Damage timing")
        })
        .and_then(|p| p.2.get("fsm"))
        .ok_or("no Damage timing FSM")?;
    let arm = literal(&one_of(single_state(timing, "Wait")?, "Wait")?, "time")?;
    let armed = literal(&one_of(single_state(timing, "Activate")?, "Wait")?, "time")?;
    let damage_fields = one_of(single_state(timing, "Activate")?, "SetDamageHeroAmount")?;
    let damage = match field(&damage_fields, "damageDealt") {
        Some(v) if v.is_map() => literal(&damage_fields, "damageDealt")?,
        Some(v) => num(v).ok_or("damageDealt")?,
        None => return err("no damageDealt"),
    };
    let animator = spurt_parts
        .iter()
        .find(|p| p.0 == "tk2dSpriteAnimator")
        .map(|p| &p.2)
        .ok_or("no animator")?;
    let library = u(source.deref(&spurt_o.file, get(animator, "library")?))?;
    let tree = u(source.read(&library))?;
    let clip = get(&tree, "clips")?
        .list()
        .unwrap_or(&[])
        .get(get(animator, "defaultClipId")?.int().unwrap_or(-1) as usize)
        .ok_or("default clip")?;
    if get(clip, "name")?.str().as_deref() != Some("Shockwave Spurt")
        || get(clip, "wrapMode")?.int() != Some(2)
    {
        return err("the spurt no longer plays Shockwave Spurt once");
    }
    let wb = prefab_box(&wave_parts)?;
    let wb: Vec<f64> = wb
        .iter()
        .enumerate()
        .map(|(i, v)| if i % 2 == 0 { v * WAVE_SCALE } else { *v })
        .collect();
    let sb = prefab_box(&spurt_parts)?;
    let frames = get(clip, "frames")?.list().unwrap_or(&[]).len() as f64;
    Ok(Wave {
        start_speed: q(WAVE_SPEED * start_factor),
        accel: q(WAVE_SPEED * accel_factor),
        wave_box: [q(wb[0]), q(wb[1]), q(wb[2]), q(wb[3])],
        ground_ray: q(ray),
        spurt_box: [q(sb[0]), q(sb[1]), q(sb[2]), q(sb[3])],
        damage_from: ticks(arm),
        damage_to: ticks(arm + armed),
        damage: damage.trunc() as i64,
        spurt_ticks: ticks(frames / get(clip, "fps")?.float().ok_or("fps")?),
        library: (
            hk_unity::base_name(&library.file.name).to_string(),
            library.path_id(),
        ),
        clip: get(clip, "name")?.str().unwrap_or_default(),
        sources: (wave_o.sid(), spurt_o.sid()),
    })
}

/// `_local_box`: a child's trigger box relative to the actor, in the unmirrored art frame.
fn local_box(sc: &Scene, gid: i64, scale_x: Option<f64>) -> Result<[i64; 4]> {
    let boxes: Vec<&Value> = component_records(sc, gid)
        .into_iter()
        .filter(|r| r.1 == "BoxCollider2D")
        .map(|r| r.2)
        .collect();
    if boxes.len() != 1
        || !get(boxes[0], "m_Enabled")?.truthy()
        || !get(boxes[0], "m_IsTrigger")?.truthy()
    {
        return err("Husk Guard child needs one trigger box");
    }
    let b = boxes[0];
    let t = sc
        .transform(*sc.go_transform.get(&gid).ok_or("no transform")?)
        .ok_or("transform missing")?;
    let (scale, pos) = (xy(t, "m_LocalScale")?, xy(t, "m_LocalPosition")?);
    let sx = scale_x.unwrap_or(scale[0]).abs();
    let sy = scale[1].abs();
    let (off, size) = (xy(b, "m_Offset")?, xy(b, "m_Size")?);
    let (cx, cy) = (pos[0] + off[0] * sx, pos[1] + off[1] * sy);
    let (w, h) = (size[0] * sx, size[1] * sy);
    Ok([
        q(cx - w / 2.0),
        q(cy - h / 2.0),
        q(cx + w / 2.0),
        q(cy + h / 2.0),
    ])
}

pub fn recognize(
    sc: &Scene,
    source: &Source,
    gid: i64,
    position: [f64; 3],
    scene_name: Option<&str>,
) -> Result<Json> {
    if let Some((_, why)) = BANK_REFUSED.iter().find(|b| Some(b.0) == scene_name) {
        return err(format!("Husk Guard refused by the room bank budget: {why}"));
    }
    check_assemblies(source, "Husk Guard")?;
    let records = component_records(sc, gid);
    let fsms: Vec<&Value> = records
        .iter()
        .filter(|r| r.1 == "PlayMakerFSM")
        .map(|r| r.2)
        .collect();
    let mut names: Vec<String> = fsms
        .iter()
        .map(|d| {
            d.get("fsm")
                .and_then(|f| f.get("name"))
                .and_then(Value::str)
                .unwrap_or_default()
        })
        .collect();
    names.sort();
    if names != [FSM_NAME] {
        return err(format!(
            "unsupported Husk Guard FSM set: {}",
            names.join(", ")
        ));
    }
    if !get(fsms[0], "m_Enabled")?.truthy() {
        return err("Husk Guard FSM disabled");
    }
    let fsm = get(fsms[0], "fsm")?;
    let fingerprint = fsm_fingerprint(fsm)?;
    if fingerprint != FSM_SHA256 {
        return err("unverified Husk Guard FSM variant");
    }
    if get(fsm, "startState")?.str().as_deref() != Some("Initiate")
        || fsm
            .get("globalTransitions")
            .and_then(Value::list)
            .is_some_and(|l| !l.is_empty())
    {
        return err("Husk Guard FSM starts elsewhere or has global transitions");
    }
    let mut vars: Vec<(String, Value)> = Vec::new();
    if let Some(Value::Map(groups)) = fsm.get("variables") {
        for (_, group) in groups {
            for v in group.list().unwrap_or(&[]) {
                if v.is_map()
                    && v.get("name").is_some()
                    && !v.get("value").is_some_and(Value::is_map)
                {
                    let name = v.get("name").and_then(Value::str).unwrap_or_default();
                    let val = v.get("value").cloned().ok_or("variable without a value")?;
                    match vars.iter_mut().find(|x| x.0 == name) {
                        Some(slot) => slot.1 = val,
                        None => vars.push((name, val)),
                    }
                }
            }
        }
    }
    let var = |k: &str| vars.iter().find(|x| x.0 == k).map(|x| &x.1);
    for (k, v) in [
        ("Chase Distance", 9.0),
        ("Roam Distance", 23.5),
        ("Woken", 1.0),
        ("Clubs In A Row", 0.0),
        ("Stomps In A Row", 0.0),
    ] {
        if var(k).and_then(num) != Some(v) {
            return err(format!("unsupported Husk Guard variable {k}"));
        }
    }
    let tid = *sc.go_transform.get(&gid).ok_or("actor has no transform")?;
    let m = u(sc.world(tid))?;
    if (m[0][0].abs() - 1.0).abs() > 1e-6 || (m[1][1] - 1.0).abs() > 1e-6 || m[0][1].abs() > 1e-6 {
        return err("unsupported Husk Guard transform");
    }
    let health: Vec<&Value> = records
        .iter()
        .filter(|r| r.1 == "HealthManager")
        .map(|r| r.2)
        .collect();
    if health.len() != 1 || get(health[0], "hp")?.int() != Some(70) {
        return err("unsupported Husk Guard health");
    }
    let damage: Vec<&Value> = records
        .iter()
        .filter(|r| r.1 == "DamageHero")
        .map(|r| r.2)
        .collect();
    if damage.len() != 1 || get(damage[0], "damageDealt")?.int() != Some(1) {
        return err("unsupported Husk Guard contact damage");
    }
    let bodies: Vec<&Value> = records
        .iter()
        .filter(|r| r.1 == "Rigidbody2D")
        .map(|r| r.2)
        .collect();
    if bodies.len() != 1 || get(bodies[0], "m_GravityScale")?.float() != Some(1.0) {
        return err("unsupported Husk Guard body");
    }
    let body = body_box(&records, "Husk Guard")?;
    let kids = child_map(sc, tid)?;
    let kid = |n: &str| kids.iter().find(|k| k.0 == n).map(|k| k.1 .0);
    for name in [
        "Attack Range",
        "Alert Range New",
        "Overhead Detect",
        "Swipe",
    ] {
        if kid(name).is_none() {
            return err(format!("Husk Guard is missing its {name}"));
        }
    }
    let alert = local_box(sc, kid("Alert Range New").unwrap(), None)?;
    let attack = local_box(sc, kid("Attack Range").unwrap(), Some(10.0))?;
    let overhead = local_box(sc, kid("Overhead Detect").unwrap(), None)?;
    let swipe = kid("Swipe").unwrap();
    if sc.active(swipe) {
        return err("Husk Guard Swipe is authored active");
    }
    let swipe_records = component_records(sc, swipe);
    let polys: Vec<&Value> = swipe_records
        .iter()
        .filter(|r| r.1 == "PolygonCollider2D")
        .map(|r| r.2)
        .collect();
    let hits: Vec<&Value> = swipe_records
        .iter()
        .filter(|r| r.1 == "DamageHero")
        .map(|r| r.2)
        .collect();
    if polys.len() != 1 || hits.len() != 1 || get(hits[0], "damageDealt")?.int() != Some(2) {
        return err("unsupported Husk Guard Swipe");
    }
    let points = get(polys[0], "m_Points")?
        .get("m_Paths")
        .and_then(Value::list)
        .and_then(|p| p.first())
        .ok_or("no path")?;
    let pts = points.list().ok_or("path is not a list")?;
    let coord = |k: &str| -> Result<Vec<f64>> {
        pts.iter()
            .map(|p| get(p, k)?.float().ok_or_else(|| "point".to_string()))
            .collect()
    };
    let (px, py) = (coord("x")?, coord("y")?);
    let lo = |v: &[f64]| v.iter().cloned().fold(f64::INFINITY, f64::min);
    let hi = |v: &[f64]| v.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let swipe_box = [q(lo(&px)), q(lo(&py)), q(hi(&px)), q(hi(&py))];
    for (key, got, want) in [
        ("alert", alert, ALERT_Q16),
        ("attack", attack, ATTACK_Q16),
        ("overhead", overhead, OVERHEAD_Q16),
        ("swipe", swipe_box, SWIPE_Q16),
    ] {
        if got != want {
            return err(format!(
                "Husk Guard {key} box {got:?} is not the audited prefab one"
            ));
        }
    }
    let animator = records
        .iter()
        .rev()
        .find(|r| r.1 == "tk2dSpriteAnimator")
        .map(|r| r.2)
        .ok_or("no tk2dSpriteAnimator")?;
    let library_o = u(sc.deref(get(animator, "library")?))?;
    let library = u(source.read(&library_o))?;
    if let Some(name) = clips_ok(&library, &CLIPS)? {
        return err(format!("unsupported Husk Guard animation: {name}"));
    }
    let w = wave(sc, source, fsm)?;
    if (
        w.start_speed,
        w.accel,
        w.wave_box,
        w.ground_ray,
        w.spurt_box,
        w.damage_from,
        w.damage_to,
        w.damage,
        w.spurt_ticks,
    ) != (
        29491,
        2359296,
        [-46363, 21182, 7464, 115753],
        104858,
        [-16352, -118, 10151, 114151],
        3,
        6,
        1,
        18,
    ) {
        return err("Husk Guard shockwave is not the one husk_guard.rs carries");
    }
    let body_world = axis_aligned_bounds(&m, xy(body, "m_Offset")?, xy(body, "m_Size")?)?;
    let body_q16: Vec<i64> = body_world
        .iter()
        .enumerate()
        .map(|(i, v)| q(v - position[i % 2]))
        .collect();
    let ints = |a: &[i64]| Json::List(a.iter().map(|&v| Json::Int(v)).collect());
    let mut art = vec![
        ("walk".to_string(), js("Walk")),
        ("turn".to_string(), js("Turn")),
    ];
    art.extend(CLIP_SLOTS.iter().map(|(k, v)| (k.to_string(), js(v))));
    let limitations: Vec<String> = vec![
        "Stomp Cooldown waits on a WAIT event with no transition; read literally the guard would stand for good after its first stomp. The shipped game's guards keep attacking, so the port goes on to Cooldown after the 0.4 s wait.".into(),
        "The FSM's Facing Right is localScale +1 while the art, the Swipe polygon and the run dust all put the front at -x and Slam Origin/Burst Rocks at +x; the port faces the sprite where the body moves and puts the club, the Swipe and the slam in front.".into(),
        "Hero Solid (the back the Knight can stand on), the thrown club of the corpse, particles, audio and camera shakes are not presented.".into(),
        "Walk, Idle, Run, Stop Run, Wake and Stomp Land keep every other frame at half the rate (same durations), to fit Crossroads_21's 256 KiB actor bank.".into()    ];
    Ok(jobj(vec![
        ("kind", js("HuskGuard")),
        ("guest_enabled", Json::Bool(true)),
        ("fsm_sha256", Json::Str(fingerprint)),
        ("library_source", Json::Str(library_o.sid())),
        // `Initiate` branches on `Start Facing Left`; the art faces -x.
        (
            "initial_direction",
            Json::Int(if var("Start Facing Left").is_some_and(Value::truthy) {
                -1
            } else {
                1
            }),
        ),
        (
            "boxes_q16",
            jobj(vec![
                ("alert", ints(&alert)),
                ("attack", ints(&attack)),
                ("overhead", ints(&overhead)),
                ("swipe", ints(&swipe_box)),
            ]),
        ),
        ("body_bounds_q16", ints(&body_q16)),
        (
            "wave",
            jobj(vec![
                ("start_speed", Json::Int(w.start_speed)),
                ("accel", Json::Int(w.accel)),
                ("wave_box", ints(&w.wave_box)),
                ("ground_ray", Json::Int(w.ground_ray)),
                ("spurt_box", ints(&w.spurt_box)),
                ("damage_from", Json::Int(w.damage_from)),
                ("damage_to", Json::Int(w.damage_to)),
                ("damage", Json::Int(w.damage)),
                ("spurt_ticks", Json::Int(w.spurt_ticks)),
                (
                    "library",
                    Json::List(vec![Json::Str(w.library.0.clone()), Json::Int(w.library.1)]),
                ),
                ("clip", Json::Str(w.clip.clone())),
                (
                    "sources",
                    jobj(vec![
                        ("wave", Json::Str(w.sources.0.clone())),
                        ("spurt", Json::Str(w.sources.1.clone())),
                    ]),
                ),
            ]),
        ),
        ("art_bindings", Json::Obj(art)),
        (
            "frame_stride",
            Json::Obj(
                FRAME_STRIDE
                    .iter()
                    .map(|k| (k.to_string(), Json::Int(2)))
                    .collect(),
            ),
        ),
        (
            "extra_art",
            jobj(vec![
                (
                    "spurt",
                    jobj(vec![
                        ("file", Json::Str(w.library.0)),
                        ("path_id", Json::Int(w.library.1)),
                        ("clip", Json::Str(w.clip)),
                    ]),
                ),
                (
                    "slam",
                    jobj(vec![
                        ("file", js("resources.assets")),
                        ("path_id", Json::Int(21298)),
                        ("clip", js("Slam")),
                    ]),
                ),
            ]),
        ),
        (
            "limitations",
            Json::List(limitations.into_iter().map(Json::Str).collect()),
        ),
    ]))
}
