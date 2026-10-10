//! The Dream Nail's own timing, hitbox and reward, from the Hero's FSM (host/dream_nail.py).
//!
//! Every phase but the slash ends on its own tk2d clip, so the clip lengths are the timings.
//! Dream Gate is deliberately not read: none of it is reachable in the admitted scenes.

use crate::breakables::{jf, jfloats, jl, k, kf, ki, kl, ks};
use crate::common::{err, py_round, Result};
use crate::cook_audio::{jobj, js, u};
use crate::focus::{assert_that, behaviours, field, hero, hero_fsm, FsmReader};
use crate::music::value_json;
use crate::pyjson::Json;
use hk_unity::{Obj, Source, Value};

pub const CLIPS: [&str; 4] = ["DN Start", "DN Charge", "DN Slash Antic", "DN Slash"];

/// `_knight_animation(source)`: the tk2d animation table that carries the Knight's clips.
pub(crate) fn knight_animation(source: &Source) -> Result<Value> {
    for o in behaviours(source, "tk2dSpriteAnimation")? {
        let table = u(source.read(&o))?;
        let names: Vec<String> = kl(&table, "clips")?
            .iter()
            .filter_map(|c| c.get("name").and_then(Value::str))
            .collect();
        if ["Idle", "Run", "Slash", "Airborne", "Focus"]
            .iter()
            .all(|n| names.iter().any(|h| h == n))
        {
            return Ok(table);
        }
    }
    err("no Knight animation table")
}

fn identity_rotation() -> Value {
    let f = |n: &str, v: f64| -> (std::sync::Arc<str>, Value) { (n.into(), Value::F64(v)) };
    Value::Map(vec![f("x", 0.0), f("y", 0.0), f("z", 0.0), f("w", 1.0)])
}

/// `_hitbox(source, hero_gid)`: the `Dream Effects > Hitbox` polygon, in Knight-local units.
fn hitbox(source: &Source, hero_gid: i64) -> Result<Json> {
    let file = u(source.file("resources.assets"))?;
    for info in &file.objects {
        // GameObject
        if info.class_id != 1 {
            continue;
        }
        let o = Obj {
            file: file.clone(),
            info: *info,
        };
        let go = u(source.read(&o))?;
        if ks(&go, "m_Name")? != "Hitbox" {
            continue;
        }
        let mut components = Vec::new();
        for c in kl(&go, "m_Component")? {
            components.push(u(source.deref(&file, k(c, "component")?))?);
        }
        // PolygonCollider2D is class 60, Transform class 4.
        let Some(collider) = components.iter().find(|c| c.class_id() == 60) else {
            continue;
        };
        let transform = components
            .iter()
            .find(|c| c.class_id() == 4)
            .ok_or_else(String::new)?;
        let t = u(source.read(transform))?;
        // Walk up to the Knight, refusing any ancestor that moves or scales the polygon.
        let mut chain: Vec<Value> = Vec::new();
        let mut parent = t.clone();
        while ki(k(&parent, "m_Father")?, "m_PathID")? != 0 {
            let next = u(source.deref(&file, k(&parent, "m_Father")?))?;
            parent = u(source.read(&next))?;
            chain.push(parent.clone());
            if ki(k(&parent, "m_GameObject")?, "m_PathID")? == hero_gid {
                break;
            }
        }
        let reached = chain
            .last()
            .map(|c| ki(k(c, "m_GameObject")?, "m_PathID").map(|g| g == hero_gid))
            .transpose()?
            .unwrap_or(false);
        if !reached {
            continue;
        }
        for ancestor in &chain[..chain.len() - 1] {
            let moved = ["x", "y", "z"].iter().any(|ax| {
                !kf(k(ancestor, "m_LocalPosition").unwrap(), ax).is_ok_and(|v| v == 0.0)
                    || !kf(k(ancestor, "m_LocalScale").unwrap(), ax).is_ok_and(|v| v == 1.0)
            });
            if moved {
                return err("non-identity Dream Nail ancestor unsupported");
            }
        }
        if !k(&t, "m_LocalRotation")?.py_eq(&identity_rotation()) {
            return err("rotated Dream Nail hitbox unsupported");
        }
        let c = u(source.read(collider))?;
        let paths = kl(k(&c, "m_Points")?, "m_Paths")?;
        if paths.len() != 1 || !(3..=16).contains(&paths[0].list().map_or(0, <[Value]>::len)) {
            return err("Dream Nail polygon budget");
        }
        let (pos, off, scale) = (
            k(&t, "m_LocalPosition")?,
            k(&c, "m_Offset")?,
            k(&t, "m_LocalScale")?,
        );
        let mut poly = Vec::new();
        for p in paths[0].list().unwrap() {
            poly.push((
                kf(pos, "x")? + (kf(p, "x")? + kf(off, "x")?) * kf(scale, "x")?,
                kf(pos, "y")? + (kf(p, "y")? + kf(off, "y")?) * kf(scale, "y")?,
            ));
        }
        if poly
            .iter()
            .any(|p| !p.0.is_finite() || !p.1.is_finite() || p.0.abs() > 16.0 || p.1.abs() > 16.0)
        {
            return err("Dream Nail local polygon exceeds Q16 bound");
        }
        return Ok(jobj(vec![
            ("collider", Json::Str(collider.sid())),
            ("transform", Json::Str(transform.sid())),
            (
                "polygon",
                jl(poly.iter().map(|p| jfloats(&[p.0, p.1])).collect()),
            ),
        ]));
    }
    err("no Dream Nail Hitbox under the Knight")
}

/// `source_dream_nail_values(source)`.
pub fn source_dream_nail_values(source: &Source) -> Result<Json> {
    let (_, hero_gid) = hero(source)?;
    let (_, fsm) = hero_fsm(source, "Dream Nail")?;
    let r = FsmReader::new(&fsm)?;
    for state in ["Start", "Charge", "Slash Antic", "Slash"] {
        let d = k(r.state(state)?, "actionData")?;
        let names = kl(d, "actionNames")?;
        let enabled = kl(d, "actionEnabled")?;
        let ends_on_clip = names.iter().enumerate().any(|(i, n)| {
            enabled.get(i).is_some_and(Value::truthy)
                && n.str().unwrap_or_default().rsplit('.').next()
                    == Some("Tk2dPlayAnimationWithEvents")
        });
        assert_that(ends_on_clip, "state ends on its clip")?;
    }
    assert_that(
        !r.actions("Charge", "ListenForDreamNail")?.is_empty(),
        "charge listens",
    )?;
    let wait = r.actions("Slash", "Wait")?;
    assert_that(wait.len() == 1, "one slash wait")?;
    let table = knight_animation(source)?;
    let mut clips: Vec<(String, f64)> = Vec::new();
    for c in kl(&table, "clips")? {
        let name = ks(c, "name")?;
        if CLIPS.contains(&name.as_str()) {
            let seconds = kl(c, "frames")?.len() as f64 / kf(c, "fps")?;
            match clips.iter_mut().find(|e| e.0 == name) {
                Some(slot) => slot.1 = seconds,
                None => clips.push((name, seconds)),
            }
        }
    }
    let get = |n: &str| -> Result<f64> {
        clips
            .iter()
            .find(|e| e.0 == n)
            .map(|e| e.1)
            .ok_or_else(|| format!("assertion failed: Knight animation lost {n}"))
    };
    let slash_wait = k(field(&wait[0], "time")?, "value")?;
    Ok(jobj(vec![
        ("start", jf(get("DN Start")?)),
        ("charge", jf(get("DN Charge")?)),
        ("antic", jf(get("DN Slash Antic")?)),
        ("slash", jf(get("DN Slash")?)),
        ("cancelable_after", value_json(slash_wait)),
        ("hitbox", hitbox(source, hero_gid)?),
        ("soul", Json::Int(33)),
        (
            "limitations",
            jl(vec![
                js("Dream Gate: setting, warping, the essence cost and the Godhome branches are not read or implemented."),
                js("Dream dialogue is a target-supplied convo title; only the GENERIC set the admitted enemies carry is bound."),
                js("The charge, slash and impact presentation is not cooked."),
            ]),
        ),
    ]))
}

fn jnum(j: &Json, key: &str) -> Result<f64> {
    match crate::breakables::jget(j, key) {
        Some(Json::Float(f)) => Ok(*f),
        Some(Json::Int(i)) => Ok(*i as f64),
        _ => Err(format!("'{key}'")),
    }
}

/// `generated_dream_nail_params(values)`.
pub fn generated_dream_nail_params(values: &Json) -> Result<String> {
    let ticks = |seconds: f64| py_round(seconds * 60.0).max(1);
    let soul = match crate::breakables::jget(values, "soul") {
        Some(Json::Int(i)) => *i,
        _ => return Err("'soul'".to_string()),
    };
    let fields = [
        ("start_ticks", ticks(jnum(values, "start")?)),
        ("charge_ticks", ticks(jnum(values, "charge")?)),
        ("antic_ticks", ticks(jnum(values, "antic")?)),
        ("slash_ticks", ticks(jnum(values, "slash")?)),
        ("cancelable_ticks", ticks(jnum(values, "cancelable_after")?)),
        ("soul", soul),
    ];
    let mut out = String::from(
        "pub const DREAM_NAIL_PARAMS: hk_sim::DreamNailParams = hk_sim::DreamNailParams {",
    );
    out += &fields
        .iter()
        .map(|(k, v)| format!("{k}:{v}"))
        .collect::<Vec<_>>()
        .join(",");
    out += "};\n";
    out += "pub const DREAM_NAIL_POLYGON: &[[i32;2]] = &[";
    let poly = match crate::breakables::jget(values, "hitbox")
        .and_then(|h| crate::breakables::jget(h, "polygon"))
    {
        Some(Json::List(l)) => l.clone(),
        _ => return Err("'hitbox'".to_string()),
    };
    let mut pts = Vec::new();
    for p in &poly {
        let Json::List(c) = p else {
            return Err("polygon".to_string());
        };
        let f = |j: &Json| match j {
            Json::Float(f) => *f,
            Json::Int(i) => *i as f64,
            _ => f64::NAN,
        };
        pts.push(format!(
            "[{},{}]",
            py_round(f(&c[0]) * 65536.0),
            py_round(f(&c[1]) * 65536.0)
        ));
    }
    out += &pts.join(",");
    out += "];\n";
    Ok(out)
}
