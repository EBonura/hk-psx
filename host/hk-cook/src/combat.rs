//! Verified no-charm nail and GrassCut source subset (host/combat.py); no generic FSM execution.

use crate::breakables::{
    file_of, jb, jf, jfloats, jget, ji, jint, jl, jstr, k, kf, ki, kl, ks, local_id, point,
};
use crate::common::{err, py_max, py_min, py_round, Result};
use crate::cook_audio::{jobj, js, u};
use crate::music::value_json;
use crate::pyjson::Json;
use hk_unity::scene::Scene;
use hk_unity::serialized::SerializedFile;
use hk_unity::{Source, Value};
use std::sync::Arc;

/// HealthManager.NonFatalHit's evasion window after a hit that does not kill.
pub const HIT_EVASION_SECONDS: f64 = 0.2;

/// Ceiling in the 60 Hz guest, tolerating serialized float32 roundoff at integers.
pub fn ticks(seconds: f64) -> i64 {
    (seconds * 60.0 - 1e-5).ceil() as i64
}

/// `grass_sources(sc, bounds, errors)`: supported GrassCut instances in `bounds`.
pub fn grass_sources(
    sc: &Scene,
    bounds: Option<[f64; 4]>,
    mut errors: Option<&mut Vec<Json>>,
) -> Result<Vec<Json>> {
    let mut result = Vec::new();
    let scene_file = file_of(sc);
    for o in &sc.objects {
        if o.typename != "GrassCut" || !k(&o.tree, "m_Enabled")?.truthy() {
            continue;
        }
        match grass_source(sc, &o.tree, o.id, bounds, &scene_file) {
            Ok(Some(r)) => result.push(r),
            Ok(None) => {}
            Err(ex) => match errors.as_deref_mut() {
                Some(list) => list.push(jobj(vec![
                    ("id", Json::Str(format!("{scene_file}:{}", o.id))),
                    ("type", js("unsupported GrassCut")),
                    ("error", Json::Str(ex)),
                ])),
                None => return Err(ex),
            },
        }
    }
    if result.len() > 32 {
        return err("GrassCut pool exceeds 32");
    }
    Ok(result)
}

#[allow(clippy::nonminimal_bool)]
fn grass_source(
    sc: &Scene,
    t: &Value,
    i: i64,
    bounds: Option<[f64; 4]>,
    scene_file: &str,
) -> Result<Option<Json>> {
    let gid = ki(k(t, "m_GameObject")?, "m_PathID")?;
    if !sc.active(gid) {
        return Ok(None);
    }
    let pos = point(sc, gid)?;
    // GrassBehaviour.Start destroys collision beyond its gameplay depth band.
    let area = bounds.unwrap_or([20.0, 0.0, 70.0, 25.0]);
    if !(area[0] < pos[0] && pos[0] < area[2])
        || !(area[1] < pos[1] && pos[1] < area[3])
        || (pos[2] - 0.004).abs() > 1.8
    {
        return Ok(None);
    }
    let mut cols = Vec::new();
    for c in &sc.objects {
        if c.typename == "BoxCollider2D"
            && ki(k(&c.tree, "m_GameObject")?, "m_PathID")? == gid
            && k(&c.tree, "m_Enabled")?.truthy()
        {
            cols.push(c);
        }
    }
    if cols.len() != 1 || !k(&cols[0].tree, "m_IsTrigger")?.truthy() {
        return err("unsupported GrassCut collider");
    }
    let (disable, enable) = (kl(t, "disable")?, kl(t, "enable")?);
    if disable.len() != 1
        || enable.len() != 1
        || k(t, "disableColliders")?.truthy()
        || k(t, "enableColliders")?.truthy()
    {
        return err("unsupported GrassCut renderer/collider topology");
    }
    let (off, on) = (ki(&disable[0], "m_PathID")?, ki(&enable[0], "m_PathID")?);
    if ki(&disable[0], "m_FileID")? != 0 || ki(&enable[0], "m_FileID")? != 0 {
        return err("external GrassCut renderer");
    }
    let kind = |id: i64| {
        sc.object(id)
            .map(|o| o.typename.as_str())
            .ok_or_else(|| id.to_string())
    };
    if kind(off)? != "SpriteRenderer" || kind(on)? != "SpriteRenderer" {
        return err("non-sprite GrassCut renderer");
    }
    let c = &cols[0].tree;
    let size = k(c, "m_Size")?;
    let o = k(c, "m_Offset")?;
    let (sx, sy) = (kf(size, "x")? / 2.0, kf(size, "y")? / 2.0);
    let mut corners = Vec::new();
    for (x, y) in [(-sx, -sy), (sx, -sy), (sx, sy), (-sx, sy)] {
        corners.push(u(sc.point(gid, x + kf(o, "x")?, y + kf(o, "y")?, 0.0))?);
    }
    for (a, b) in corners.iter().zip(corners.iter().cycle().skip(1)) {
        if (a[0] - b[0]).abs() > 0.005 && (a[1] - b[1]).abs() > 0.005 {
            return err("rotated GrassCut box unsupported");
        }
    }
    let bx = [
        py_min(corners.iter().map(|p| p[0])),
        py_min(corners.iter().map(|p| p[1])),
        py_max(corners.iter().map(|p| p[0])),
        py_max(corners.iter().map(|p| p[1])),
    ];
    if bx.iter().any(|v| !v.is_finite() || v.abs() > 512.0) {
        return err("GrassCut world coordinate exceeds Q16 bound");
    }
    Ok(Some(jobj(vec![
        ("source", Json::Str(format!("{scene_file}:{i}"))),
        (
            "collider",
            Json::Str(format!("{scene_file}:{}", cols[0].id)),
        ),
        ("off", ji(off)),
        ("on", ji(on)),
        ("box", jfloats(&bx)),
        ("serialized", value_json(t)),
    ])))
}

fn identity_rotation() -> Value {
    let f = |n: &str, v: f64| -> (std::sync::Arc<str>, Value) { (n.into(), Value::F64(v)) };
    Value::Map(vec![f("x", 0.0), f("y", 0.0), f("z", 0.0), f("w", 1.0)])
}

/// `nail_sources(s, rf, hero_gid)`: the Knight's four NailSlash objects, in
/// Slash, AltSlash, UpSlash, DownSlash order.
pub fn nail_sources(source: &Source, rf: &Arc<SerializedFile>, hero_gid: i64) -> Result<Vec<Json>> {
    let names = ["Slash", "AltSlash", "UpSlash", "DownSlash"];
    let mut result: Vec<(String, Json)> = Vec::new();
    for info in &rf.objects {
        // MonoBehaviour
        if info.class_id != 114 {
            continue;
        }
        let o = hk_unity::Obj {
            file: rf.clone(),
            info: *info,
        };
        if u(source.typename(&o))? != "NailSlash" {
            continue;
        }
        let n = u(source.read(&o))?;
        let go = u(source.deref(rf, k(&n, "m_GameObject")?))?;
        let g = u(source.read(&go))?;
        let name = ks(&g, "m_Name")?;
        if !names.contains(&name.as_str()) {
            continue;
        }
        let mut components = Vec::new();
        for c in kl(&g, "m_Component")? {
            components.push(u(source.deref(rf, k(c, "component")?))?);
        }
        // Transform is class 4, PolygonCollider2D class 60.
        let tr = components
            .iter()
            .find(|c| c.class_id() == 4)
            .ok_or_else(String::new)?;
        let t = u(source.read(tr))?;
        let mut parent = u(source.read(&u(source.deref(rf, k(&t, "m_Father")?))?))?;
        while ki(k(&parent, "m_GameObject")?, "m_PathID")? != hero_gid
            && ki(k(&parent, "m_Father")?, "m_PathID")? != 0
        {
            // The intermediate 'Attacks' transform must be identity for local bounds.
            let moved = ["x", "y", "z"].iter().any(|ax| {
                !kf(k(&parent, "m_LocalPosition").unwrap(), ax).is_ok_and(|v| v == 0.0)
                    || !kf(k(&parent, "m_LocalScale").unwrap(), ax).is_ok_and(|v| v == 1.0)
            });
            if moved || !k(&parent, "m_LocalRotation")?.py_eq(&identity_rotation()) {
                return err("non-identity nail ancestor unsupported");
            }
            let next = u(source.deref(rf, k(&parent, "m_Father")?))?;
            parent = u(source.read(&next))?;
        }
        if ki(k(&parent, "m_GameObject")?, "m_PathID")? != hero_gid {
            continue;
        }
        if !k(&t, "m_LocalRotation")?.py_eq(&identity_rotation()) {
            return err("rotated NailSlash unsupported");
        }
        let co = components
            .iter()
            .find(|c| c.class_id() == 60)
            .ok_or_else(String::new)?;
        let c = u(source.read(co))?;
        let paths = kl(k(&c, "m_Points")?, "m_Paths")?;
        if paths.len() != 1 || !(3..=16).contains(&paths[0].list().map_or(0, <[Value]>::len)) {
            return err("NailSlash polygon budget");
        }
        let pos = k(&t, "m_LocalPosition")?;
        let scale = k(&n, "scale")?;
        let off = k(&c, "m_Offset")?;
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
            return err("NailSlash local polygon exceeds Q16 bound");
        }
        let record = jobj(vec![
            ("source", Json::Str(o.sid())),
            ("transform", Json::Str(tr.sid())),
            ("collider", Json::Str(co.sid())),
            ("clip", value_json(k(&n, "animName")?)),
            ("position", value_json(pos)),
            ("scale", value_json(scale)),
            (
                "polygon",
                jl(poly.iter().map(|p| jfloats(&[p.0, p.1])).collect()),
            ),
            ("serialized", value_json(&n)),
        ]);
        match result.iter_mut().find(|e| e.0 == name) {
            Some(slot) => slot.1 = record,
            None => result.push((name, record)),
        }
    }
    if result.len() != 4 {
        return err("Knight NailSlash objects missing");
    }
    names
        .iter()
        .map(|n| {
            result
                .iter()
                .find(|e| e.0 == *n)
                .map(|e| e.1.clone())
                .ok_or_else(|| "Knight NailSlash objects missing".to_string())
        })
        .collect()
}

/// `transformed_box(box, nail)`.
pub fn transformed_box(b: [f64; 4], nail: &Json) -> Result<[f64; 4]> {
    let p = jget(nail, "position").ok_or("'position'")?;
    let s = jget(nail, "scale").ok_or("'scale'")?;
    let f = |j: &Json, key: &str| match jget(j, key) {
        Some(Json::Float(v)) => Ok(*v),
        Some(Json::Int(v)) => Ok(*v as f64),
        _ => Err(format!("'{key}'")),
    };
    Ok([
        f(p, "x")? + b[0] * f(s, "x")?,
        f(p, "y")? + b[1] * f(s, "y")?,
        f(p, "x")? + b[2] * f(s, "x")?,
        f(p, "y")? + b[3] * f(s, "y")?,
    ])
}

/// `generated_params(constants, dt, nails, grass, draws)`: the Rust source text of the
/// attack, nail polygon and grass tables. Fills `off_draw`/`on_draw` of each grass record.
pub fn generated_params(
    constants: &[(String, f64)],
    dt: f64,
    nails: &[Json],
    grass: &mut [Json],
    draws: &[String],
) -> Result<String> {
    let float_list = |j: &Json, key: &str| -> Vec<f64> {
        match jget(j, key) {
            Some(Json::List(l)) => l
                .iter()
                .map(|v| match v {
                    Json::Float(f) => *f,
                    Json::Int(i) => *i as f64,
                    _ => f64::NAN,
                })
                .collect(),
            _ => vec![],
        }
    };
    for n in nails {
        if let Some(Json::List(poly)) = jget(n, "polygon") {
            for pt in poly {
                if let Json::List(p) = pt {
                    for v in p {
                        let v = match v {
                            Json::Float(f) => *f,
                            Json::Int(i) => *i as f64,
                            _ => f64::NAN,
                        };
                        if !v.is_finite() || v.abs() > 16.0 {
                            return err("NailSlash local polygon exceeds Q16 bound");
                        }
                    }
                }
            }
        }
    }
    for g in grass.iter() {
        if float_list(g, "box")
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 512.0)
        {
            return err("GrassCut world coordinate exceeds Q16 bound");
        }
    }
    let constant = |name: &str| -> Result<f64> {
        constants
            .iter()
            .find(|c| c.0 == name)
            .map(|c| c.1)
            .ok_or_else(|| format!("'{name}'"))
    };
    let mut out =
        String::from("pub const ATTACK_PARAMS: hk_sim::AttackParams = hk_sim::AttackParams {");
    // HeroController::.ctor sets ATTACK_QUEUE_STEPS to 5, a constructor literal.
    let attack_queue_steps = 5.0;
    let values: [(&str, i64); 7] = [
        ("duration", ticks(constant("ATTACK_DURATION")?)),
        ("cooldown", ticks(constant("ATTACK_COOLDOWN_TIME")?)),
        ("alternate_reset", ticks(constant("ALT_ATTACK_RESET")?)),
        ("hit_start", ticks(dt)),
        ("hit_end", ticks(5.0 * dt)),
        ("queue_ticks", py_round(attack_queue_steps * dt * 60.0)),
        ("recovery_ticks", ticks(constant("ATTACK_RECOVERY_TIME")?)),
    ];
    out += &values
        .iter()
        .map(|(k, v)| format!("{k}:{v}"))
        .collect::<Vec<_>>()
        .join(",");
    out += "};\n";
    out += "pub const NAIL_POLYGONS: [&[[i32;2]];4] = [";
    let polys: Vec<String> = nails
        .iter()
        .map(|n| {
            let pts: Vec<String> = match jget(n, "polygon") {
                Some(Json::List(l)) => l
                    .iter()
                    .map(|p| {
                        let c = float_list(&Json::Obj(vec![("p".into(), p.clone())]), "p");
                        format!(
                            "[{},{}]",
                            py_round(c[0] * 65536.0),
                            py_round(c[1] * 65536.0)
                        )
                    })
                    .collect(),
                _ => vec![],
            };
            format!("&[{}]", pts.join(","))
        })
        .collect();
    out += &polys.join(",");
    out += "];\n";
    out += "pub const GRASS: &[hk_sim::Grass] = &[\n";
    let ids = |source: &str| draws.iter().position(|d| d == source);
    for g in grass.iter_mut() {
        let scene_file = jget(g, "source")
            .map(jstr)
            .unwrap_or_else(|| "level6:0".to_string())
            .split(':')
            .next()
            .unwrap_or("")
            .to_string();
        let (off, on) = (jint(jget(g, "off").unwrap()), jint(jget(g, "on").unwrap()));
        let (a, b) = (
            ids(&format!("{scene_file}:{off}")),
            ids(&format!("{scene_file}:{on}")),
        );
        let (Some(a), Some(b)) = (a, b) else {
            return err("GrassCut renderer omitted by chamber culling");
        };
        crate::breakables::jset(g, "off_draw", ji(a as i64));
        crate::breakables::jset(g, "on_draw", ji(b as i64));
        let bounds: Vec<String> = float_list(g, "box")
            .iter()
            .map(|v| py_round(v * 65536.0).to_string())
            .collect();
        out += &format!(
            "hk_sim::Grass {{bounds:[{}],off_draw:{a},on_draw:{b}}},\n",
            bounds.join(",")
        );
    }
    Ok(out + "];\n")
}

#[allow(dead_code)]
fn _keep() {
    let _ = (jb, jf, local_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_tolerate_float32_roundoff_at_integers() {
        assert_eq!(ticks(0.35), 21);
        assert_eq!(ticks(1.0 / 60.0), 1);
        assert_eq!(ticks(0.2), 12);
    }

    #[test]
    fn a_transformed_box_scales_then_offsets() {
        let nail = jobj(vec![
            ("position", jobj(vec![("x", jf(1.0)), ("y", jf(2.0))])),
            ("scale", jobj(vec![("x", jf(2.0)), ("y", jf(-1.0))])),
        ]);
        assert_eq!(
            transformed_box([1.0, 1.0, 3.0, 2.0], &nail).unwrap(),
            [3.0, 1.0, 7.0, 0.0]
        );
    }
}
