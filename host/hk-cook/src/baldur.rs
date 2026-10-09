//! Recognize the verified Baldur (Roller) `Roller` FSM variant for guest
//! admission. Ported from host/baldur.py.
//!
//! The controller lives in shared/hk-sim/src/baldur.rs; this module admits only
//! placed instances whose FSM parameters and body match the audited contract.

use crate::common::{component_records, err, get, Result};
use crate::cook_audio::{jobj, js, u};
use crate::pyjson::Json;
use crate::recog::{
    check_assemblies, clip_is, clips_by_name, near, only, scalar, state, states, transitions,
    variables, xy,
};
use crate::runner::axis_aligned_bounds;
use hk_unity::playmaker::action_fields;
use hk_unity::scene::Scene;
use hk_unity::{Source, Value};

/// (name, frames, fps, wrap mode).
const CLIPS: [(&str, usize, f64, i64); 4] = [
    ("Start", 4, 10.0, 2),
    ("Stop", 4, 10.0, 2),
    ("Idle", 4, 12.0, 0),
    ("Roll", 3, 12.0, 0),
];
const BODY_SIZE: [f64; 2] = [1.09375, 1.09375];
const BODY_OFFSET: [f64; 2] = [-0.015625, -0.109375];
const ALERT_SCALE: [f64; 2] = [21.139999389648438, 1.899999976158142];
const ALERT_OFFSET_Y: f64 = 0.3100000023841858;
const VARIABLES: [(&str, f64); 5] = [
    ("Acceleration", 0.45),
    ("Max Speed", 11.0),
    ("Roll time Min", 2.0),
    ("Roll time Max", 3.0),
    ("Stop Time", 0.5),
];
const TRANSITIONS: [(&str, &[(&str, &str)]); 17] = [
    ("Initiate", &[("FINISHED", "Idle")]),
    ("Idle", &[("ALERT", "Facing Check")]),
    (
        "Facing Check",
        &[("LEFT", "Start Left"), ("RIGHT", "Start Right")],
    ),
    ("Start Right", &[("FINISHED", "Start")]),
    ("Start Left", &[("FINISHED", "Start")]),
    ("Start", &[("WAIT", "Left or right?")]),
    ("Left or right?", &[("RIGHT", "Roll R"), ("LEFT", "Roll L")]),
    (
        "Roll R",
        &[
            ("WALL", "Collide Right"),
            ("STOP", "Stop"),
            ("RECOIL HORIZONTAL", "Recoil Decel R"),
        ],
    ),
    (
        "Roll L",
        &[
            ("WALL", "Collide Left"),
            ("STOP", "Stop"),
            ("RECOIL HORIZONTAL", "Recoil Decel L"),
        ],
    ),
    ("Collide Right", &[("WAIT", "In Air")]),
    ("Collide Left", &[("WAIT", "In Air")]),
    ("In Air", &[("GROUND", "Land")]),
    ("Land", &[("FINISHED", "Left or right?")]),
    ("Stop", &[("WAIT", "Rest")]),
    ("Rest", &[("WAIT", "Idle")]),
    ("Recoil Decel R", &[("FINISHED", "Roll R")]),
    ("Recoil Decel L", &[("FINISHED", "Roll L")]),
];
const ANGLES: [(&str, f64, f64); 2] =
    [("Collide Right", 115.0, 12.0), ("Collide Left", 65.0, 12.0)];

/// Admit the placement at `gid`, or refuse with the first failed check.
pub fn recognize(sc: &Scene, source: &Source, gid: i64) -> Result<Json> {
    let records = component_records(sc, gid);
    check_assemblies(source, "Baldur")?;
    let fsms: Vec<&Value> = records
        .iter()
        .filter(|r| r.1 == "PlayMakerFSM")
        .filter_map(|r| r.2.get("fsm"))
        .collect();
    if fsms.len() != 1
        || get(fsms[0], "name")?.str().as_deref() != Some("Roller")
        || get(fsms[0], "startState")?.str().as_deref() != Some("Initiate")
    {
        return err("no single Baldur Roller FSM");
    }
    let fsm = fsms[0];
    let vars = variables(fsm);
    for (name, value) in VARIABLES {
        if !near(vars.iter().find(|(k, _)| k == name).map(|(_, v)| *v), value) {
            return err(format!("unsupported Baldur variable: {name}"));
        }
    }
    let sts = states(fsm)?;
    for (name, expected) in TRANSITIONS {
        let ok = state(&sts, name)
            .map(transitions)
            .transpose()?
            .is_some_and(|t| {
                t.len() == expected.len()
                    && t.iter()
                        .zip(expected.iter())
                        .all(|(a, b)| a.0 == b.0 && a.1 == b.1)
            });
        if !ok {
            return err(format!("unsupported Baldur transitions: {name}"));
        }
    }
    for (name, angle, speed) in ANGLES {
        let data = get(state(&sts, name).ok_or("missing state")?, "actionData")?;
        let names = get(data, "actionNames")?.list().unwrap_or(&[]);
        let enabled = get(data, "actionEnabled")?.list().unwrap_or(&[]);
        let matches: Vec<usize> = names
            .iter()
            .enumerate()
            .filter(|(i, n)| {
                n.str().is_some_and(|n| n.ends_with("SetVelocityAsAngle"))
                    && enabled.get(*i).is_some_and(Value::truthy)
            })
            .map(|(i, _)| i)
            .collect();
        if matches.len() != 1 {
            return err(format!("unsupported Baldur collide: {name}"));
        }
        let fields = u(action_fields(data, matches[0], false))?;
        let field = |k: &str| fields.iter().find(|f| f.0 == k).map(|f| scalar(&f.1));
        if !near(field("angle").as_ref(), angle) || !near(field("speed").as_ref(), speed) {
            return err(format!("unsupported Baldur collide launch: {name}"));
        }
    }
    for name in ["Roll R", "Roll L"] {
        let data = get(state(&sts, name).ok_or("missing state")?, "actionData")?;
        let all = get(data, "actionNames")?.list().unwrap_or(&[]);
        let enabled = get(data, "actionEnabled")?.list().unwrap_or(&[]);
        let short: Vec<String> = all
            .iter()
            .enumerate()
            .filter(|(i, _)| enabled.get(*i).is_some_and(Value::truthy))
            .map(|(_, n)| {
                n.str()
                    .unwrap_or_default()
                    .rsplit('.')
                    .next()
                    .unwrap_or("")
                    .to_string()
            })
            .collect();
        let add = if name == "Roll R" {
            "FloatAdd"
        } else {
            "FloatSubtract"
        };
        if !short.iter().any(|n| n == add)
            || !short.iter().any(|n| n == "FloatClamp")
            || short.iter().filter(|n| *n == "CheckCollisionSide").count() != 3
            || !short.iter().any(|n| n == "FloatCompare")
        {
            return err(format!("unsupported Baldur roll actions: {name}"));
        }
        let every = enabled.len() == all.len()
            && enabled
                .iter()
                .all(|e| e.float() == Some(1.0) || e.int() == Some(1));
        let index = if every {
            short.iter().position(|n| n == add).unwrap()
        } else {
            all.iter()
                .position(|n| n.str().is_some_and(|n| n.ends_with(add)))
                .ok_or("no add action")?
        };
        let fields = u(action_fields(data, index, false))?;
        if fields
            .iter()
            .find(|f| f.0 == "perSecond")
            .map(|f| scalar(&f.1))
            .is_some_and(|v| v.truthy())
        {
            return err("unsupported Baldur per-second acceleration");
        }
    }
    let matrix = u(sc.world(*sc.go_transform.get(&gid).ok_or("actor has no transform")?))?;
    // Placed Rollers start mirrored (scale.x -1); the FSM sets the scale sign
    // itself from the first Idle frame, so only the magnitude matters here.
    if (matrix[0][0].abs() - 1.0).abs() > 1e-6
        || (matrix[1][1] - 1.0).abs() > 1e-6
        || matrix[0][1].abs() > 1e-6
        || matrix[1][0].abs() > 1e-6
    {
        return err("unsupported Baldur initial rotation or scale");
    }
    // Some placed Rollers carry the same box twice; identical copies are one body.
    let bodies: Vec<&Value> = records
        .iter()
        .filter(|r| r.1 == "BoxCollider2D")
        .map(|r| r.2)
        .collect();
    let key = |b: &Value| {
        (
            b.get("m_Size").cloned(),
            b.get("m_Offset").cloned(),
            b.get("m_IsTrigger").cloned(),
        )
    };
    if !(1..=2).contains(&bodies.len())
        || bodies.iter().any(|b| {
            let (k, k0) = (key(b), key(bodies[0]));
            !(k.0
                .as_ref()
                .zip(k0.0.as_ref())
                .is_some_and(|(a, b)| a.py_eq(b))
                && k.1
                    .as_ref()
                    .zip(k0.1.as_ref())
                    .is_some_and(|(a, b)| a.py_eq(b))
                && k.2
                    .as_ref()
                    .zip(k0.2.as_ref())
                    .is_some_and(|(a, b)| a.py_eq(b)))
        })
    {
        return err("unsupported Baldur body colliders");
    }
    let body = bodies[0];
    let (size, offset) = (xy(body, "m_Size")?, xy(body, "m_Offset")?);
    if !get(body, "m_Enabled")?.truthy()
        || get(body, "m_IsTrigger")?.truthy()
        || get(body, "m_EdgeRadius")?.float() != Some(0.0)
        || !near(Some(&Value::F64(size[0])), BODY_SIZE[0])
        || !near(Some(&Value::F64(size[1])), BODY_SIZE[1])
        || !near(Some(&Value::F64(offset[0])), BODY_OFFSET[0])
        || !near(Some(&Value::F64(offset[1])), BODY_OFFSET[1])
    {
        return err("unsupported Baldur body collider");
    }
    let (_, rigid) = only(&records, "Rigidbody2D", "Baldur")?;
    if get(rigid, "m_BodyType")?.int() != Some(0)
        || !near(rigid.get("m_GravityScale"), 0.8)
        || get(rigid, "m_LinearDamping")?.float() != Some(0.0)
        || get(rigid, "m_Constraints")?.int() != Some(4)
    {
        return err("unsupported Baldur rigid body");
    }
    let (_, recoil) = only(&records, "Recoil", "Baldur")?;
    if get(recoil, "freezeInPlace")?.truthy()
        || get(recoil, "recoilSpeedBase")?.float() != Some(25.0)
        || !near(recoil.get("recoilDuration"), 0.15)
        || get(recoil, "preventRecoilUp")?.truthy()
    {
        return err("unsupported Baldur recoil variant");
    }
    let (_, sight) = only(&records, "LineOfSightDetector", "Baldur")?;
    let ranges = get(sight, "alertRanges")?.list().unwrap_or(&[]);
    if !get(sight, "m_Enabled")?.truthy() || ranges.len() != 1 {
        return err("unsupported Baldur line of sight detector");
    }
    let alert_id = get(&ranges[0], "m_PathID")?.int().unwrap_or(0);
    let alert_gid = get(
        get(
            &sc.object(alert_id)
                .ok_or("alert range is not in the scene")?
                .tree,
            "m_GameObject",
        )?,
        "m_PathID",
    )?
    .int()
    .unwrap_or(0);
    if get(sc.go(alert_gid).ok_or("no such GameObject")?, "m_Name")?
        .str()
        .as_deref()
        != Some("Alert Range New")
        || !sc.active(alert_gid)
    {
        return err("unsupported Baldur alert range object");
    }
    let alert_records = component_records(sc, alert_gid);
    let (_, boxed) = only(&alert_records, "BoxCollider2D", "Baldur")?;
    let alert = sc
        .transform(
            *sc.go_transform
                .get(&alert_gid)
                .ok_or("alert object has no transform")?,
        )
        .ok_or("alert transform missing")?;
    let unit = Value::Map(vec![
        ("x".into(), Value::F64(1.0)),
        ("y".into(), Value::F64(1.0)),
    ]);
    let zero = Value::Map(vec![
        ("x".into(), Value::F64(0.0)),
        ("y".into(), Value::F64(0.0)),
    ]);
    let scale = xy(alert, "m_LocalScale")?;
    let position = xy(alert, "m_LocalPosition")?;
    if !get(boxed, "m_IsTrigger")?.truthy()
        || !get(boxed, "m_Size")?.py_eq(&unit)
        || !get(boxed, "m_Offset")?.py_eq(&zero)
        || !near(Some(&Value::F64(scale[0])), ALERT_SCALE[0])
        || !near(Some(&Value::F64(scale[1])), ALERT_SCALE[1])
        || !near(Some(&Value::F64(position[0])), 0.0)
        || !near(Some(&Value::F64(position[1])), ALERT_OFFSET_Y)
    {
        return err("unsupported Baldur alert range");
    }
    let (_, animator) = only(&records, "tk2dSpriteAnimator", "Baldur")?;
    let library = u(source.read(&u(sc.deref(get(animator, "library")?))?))?;
    let by_name = clips_by_name(&library)?;
    for (name, frames, fps, wrap) in CLIPS {
        if !clip_is(
            by_name.iter().find(|(k, _)| k == name).map(|(_, v)| *v),
            frames,
            fps,
            wrap,
        ) {
            return err(format!("unsupported Baldur animation: {name}"));
        }
    }
    let identity = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let bounds = axis_aligned_bounds(&identity, BODY_OFFSET, BODY_SIZE)?;
    let floats = |v: &[f64]| Json::List(v.iter().map(|&f| Json::Float(f)).collect());
    Ok(jobj(vec![
        ("kind", js("Baldur")),
        ("guest_enabled", Json::Bool(true)),
        ("body_bounds_local", floats(&bounds)),
        ("alert_bounds_local", floats(&[-ALERT_SCALE[0] / 2.0, ALERT_OFFSET_Y - ALERT_SCALE[1] / 2.0, ALERT_SCALE[0] / 2.0, ALERT_OFFSET_Y + ALERT_SCALE[1] / 2.0])),
        (
            "limitations",
            Json::List(
                [
                    "Gravity body on the bounded terrain solver; WALL and GROUND come from the blocked solver axes instead of the contact rays.",
                    "Roll dust, the roll audio loop and land effects are not presented.",
                    "The rolling corpse uses box bounds and the solver slide instead of the circle body with 0.7 linear damping, and is removed 120 ticks after landing.",
                ]
                .iter()
                .map(|s| js(s))
                .collect(),
            ),
        ),
    ]))
}
