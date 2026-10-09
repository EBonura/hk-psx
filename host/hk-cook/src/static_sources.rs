//! The static, non-enemy shapes a scene hands the guest: damage hazards, bounce
//! shrooms and pogo targets. Ported from host/actors.py (`hazard_sources`,
//! `shroom_sources`, `pogo_sources`).

use crate::colliders::colliders;
use crate::common::{component_records, err, get, go_of, Result};
use crate::cook_audio::u;
use crate::pyjson::Json;
use hk_unity::scene::Scene;
use hk_unity::Value;

fn jstr(s: &str) -> Json {
    Json::Str(s.to_string())
}

fn field<'a>(c: &'a Json, key: &str) -> Option<&'a Json> {
    match c {
        Json::Obj(f) => f.iter().find(|k| k.0 == key).map(|k| &k.1),
        _ => None,
    }
}

fn floats(j: &Json) -> Vec<f64> {
    match j {
        Json::List(l) => l
            .iter()
            .map(|v| match v {
                Json::Float(f) => *f,
                Json::Int(i) => *i as f64,
                _ => f64::NAN,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Whether the collider's box misses the optional query bounds.
fn outside(bounds: Option<[f64; 4]>, b: &[f64]) -> bool {
    bounds.is_some_and(|q| b[2] < q[0] || b[0] > q[2] || b[3] < q[1] || b[1] > q[3])
}

fn gos_in_order<'a>(sc: &'a Scene<'a>) -> impl Iterator<Item = (i64, &'a Value)> {
    sc.objects
        .iter()
        .filter(move |o| sc.gos.contains_key(&o.id))
        .map(|o| (o.id, &o.tree))
}

fn name_of(sc: &Scene, gid: i64) -> Result<String> {
    Ok(get(sc.go(gid).ok_or("no such GameObject")?, "m_Name")?
        .str()
        .unwrap_or_default())
}

fn point_json(sc: &Scene, gid: i64) -> Result<Json> {
    Ok(Json::List(
        u(sc.point(gid, 0.0, 0.0, 0.0))?
            .iter()
            .map(|&f| Json::Float(f))
            .collect(),
    ))
}

/// `{**collider, **extra}`: existing keys keep their place, new ones follow.
fn merged(collider: &Json, extra: Vec<(&str, Json)>) -> Json {
    let Json::Obj(mut fields) = collider.clone() else {
        return collider.clone();
    };
    for (k, v) in extra {
        match fields.iter_mut().find(|f| f.0 == k) {
            Some(slot) => slot.1 = v,
            None => fields.push((k.to_string(), v)),
        }
    }
    Json::Obj(fields)
}

/// `hazard_sources(sc, bounds)`: static enabled DamageHero shapes.
pub fn hazard_sources(sc: &Scene, bounds: Option<[f64; 4]>) -> Result<Vec<Json>> {
    let health_gos: Vec<i64> = sc
        .objects
        .iter()
        .filter(|o| o.typename == "HealthManager")
        .filter_map(|o| go_of(&o.tree))
        .collect();
    let mut result = Vec::new();
    for o in &sc.objects {
        if o.typename != "DamageHero"
            || !get(&o.tree, "m_Enabled")?.truthy()
            || get(&o.tree, "damageDealt")?.float().ok_or("damageDealt")? <= 0.0
        {
            continue;
        }
        let gid = go_of(&o.tree).unwrap_or(0);
        if health_gos.contains(&gid) || !sc.active(gid) {
            continue;
        }
        let records = component_records(sc, gid);
        let Json::List(found) = colliders(sc, gid, &records)? else {
            continue;
        };
        for collider in &found {
            let Some(b) = field(collider, "bounds") else {
                continue;
            };
            if outside(bounds, &floats(b)) {
                continue;
            }
            result.push(merged(
                collider,
                vec![
                    ("source", Json::Str(sc.sid(o.id))),
                    (
                        "collider_source",
                        field(collider, "source").cloned().unwrap_or(Json::Null),
                    ),
                    ("name", Json::Str(name_of(sc, gid)?)),
                    (
                        "damage",
                        crate::music::value_json(get(&o.tree, "damageDealt")?),
                    ),
                    (
                        "hazard_type",
                        crate::music::value_json(get(&o.tree, "hazardType")?),
                    ),
                    ("position", point_json(sc, gid)?),
                ],
            ));
        }
    }
    if result.len() > 64 {
        return err("hazard region exceeds bounded 64-shape pool");
    }
    Ok(result)
}

/// `shroom_sources(sc, bounds)`: enabled BounceShroom triggers.
pub fn shroom_sources(sc: &Scene, bounds: Option<[f64; 4]>) -> Result<Vec<Json>> {
    let mut result = Vec::new();
    for o in &sc.objects {
        if o.typename != "BounceShroom" || !get(&o.tree, "m_Enabled")?.truthy() {
            continue;
        }
        let gid = go_of(&o.tree).unwrap_or(0);
        if !sc.active(gid) {
            continue;
        }
        // The owning object's FSMs are recorded, not cooked.
        let mut fsms: Vec<String> = sc
            .objects
            .iter()
            .filter(|f| f.typename == "PlayMakerFSM" && go_of(&f.tree) == Some(gid))
            .map(|f| {
                get(get(&f.tree, "fsm")?, "name")?
                    .str()
                    .ok_or_else(|| "fsm name".to_string())
            })
            .collect::<Result<_>>()?;
        fsms.sort();
        let records = component_records(sc, gid);
        let Json::List(found) = colliders(sc, gid, &records)? else {
            continue;
        };
        for collider in &found {
            let Some(b) = field(collider, "bounds") else {
                continue;
            };
            if !field(collider, "trigger").is_some_and(|t| matches!(t, Json::Bool(true))) {
                continue;
            }
            let b = floats(b);
            if outside(bounds, &b) {
                continue;
            }
            // The guest carries the box alone, so a shape wider than the collider would silently grow the target.
            let distinct = |v: Vec<f64>| -> Vec<f64> {
                let mut out: Vec<f64> = Vec::new();
                for x in v {
                    if !out.contains(&x) {
                        out.push(x);
                    }
                }
                out.sort_by(|a, c| a.partial_cmp(c).unwrap());
                out
            };
            let Some(Json::List(polygons)) = field(collider, "world_polygons") else {
                continue;
            };
            for polygon in polygons {
                let Json::List(points) = polygon else {
                    continue;
                };
                let (xs, ys): (Vec<f64>, Vec<f64>) = points
                    .iter()
                    .map(|p| {
                        let v = floats(p);
                        (v[0], v[1])
                    })
                    .unzip();
                if points.len() != 4
                    || distinct(xs) != distinct(vec![b[0], b[2]])
                    || distinct(ys) != distinct(vec![b[1], b[3]])
                {
                    return err(format!(
                        "BounceShroom collider is not an axis-aligned box: {}",
                        match field(collider, "source") {
                            Some(Json::Str(s)) => s.clone(),
                            _ => String::new(),
                        }
                    ));
                }
            }
            result.push(Json::Obj(vec![
                ("source".into(), Json::Str(sc.sid(o.id))),
                ("collider_source".into(), field(collider, "source").cloned().unwrap_or(Json::Null)),
                ("name".into(), Json::Str(name_of(sc, gid)?)),
                ("bounds".into(), field(collider, "bounds").cloned().unwrap_or(Json::Null)),
                ("owner_fsms".into(), Json::List(fsms.iter().map(|s| jstr(s)).collect())),
                (
                    "limitations".into(),
                    Json::List(vec![jstr("Down slash response only: no shroom bob, bounce animation or particles"), jstr("The owner FSMs are not run, so a PlayerData gate that would deactivate this object is ignored")]),
                ),
            ]));
        }
    }
    Ok(result)
}

/// `pogo_sources(sc)`: static, enabled source NailSlash targets; special and dynamic bouncers explicit.
pub fn pogo_sources(sc: &Scene) -> Result<Json> {
    let (mut result, mut unsupported): (Vec<Json>, Vec<Json>) = (Vec::new(), Vec::new());
    for (gid, go) in gos_in_order(sc) {
        let layer = get(go, "m_Layer")?.int().unwrap_or(-1);
        if !matches!(layer, 11 | 17 | 19) || !sc.active(gid) {
            continue;
        }
        let records = component_records(sc, gid);
        let has = |kind: &str| records.iter().any(|r| r.1 == kind);
        if has("HealthManager") {
            continue; // Separate moving source-ID actor state, never spawn-position copies.
        }
        if records
            .iter()
            .any(|r| r.1 == "NonBouncer" && r.2.get("active").is_some_and(Value::truthy))
        {
            continue;
        }
        let Json::List(found) = colliders(sc, gid, &records)? else {
            continue;
        };
        if found.is_empty() {
            continue;
        }
        let mut blockers: Vec<&str> = ["BigBouncer", "BounceShroom", "PlayMakerFSM"]
            .into_iter()
            .filter(|k| has(k))
            .collect();
        if records.iter().any(|r| {
            r.1 == "Rigidbody2D"
                && get(r.2, "m_BodyType")
                    .map(|b| b.int() != Some(2))
                    .unwrap_or(true)
        }) {
            blockers.push("moving Rigidbody2D");
        }
        if !blockers.is_empty() {
            blockers.sort();
            unsupported.push(Json::Obj(vec![
                ("game_object".into(), Json::Str(sc.sid(gid))),
                ("name".into(), Json::Str(name_of(sc, gid)?)),
                (
                    "reason".into(),
                    Json::Str(format!("special/dynamic pogo: {}", blockers.join(", "))),
                ),
            ]));
            continue;
        }
        for collider in found {
            if field(&collider, "bounds").is_none() {
                unsupported.push(collider);
                continue;
            }
            let polygons = match field(&collider, "world_polygons") {
                Some(Json::List(l)) => l.clone(),
                _ => Vec::new(),
            };
            let sizes_ok = (1..=8).contains(&polygons.len())
                && polygons
                    .iter()
                    .all(|p| matches!(p, Json::List(pts) if (3..=16).contains(&pts.len())));
            if !sizes_ok {
                unsupported.push(Json::Obj(vec![
                    (
                        "source".into(),
                        field(&collider, "source").cloned().unwrap_or(Json::Null),
                    ),
                    ("reason".into(), jstr("pogo polygon bound")),
                ]));
                continue;
            }
            result.push(merged(
                &collider,
                vec![
                    ("game_object", Json::Str(sc.sid(gid))),
                    ("name", Json::Str(name_of(sc, gid)?)),
                    ("layer", Json::Int(layer)),
                    ("horizontal_and_up", Json::Bool(layer == 11)),
                ],
            ));
        }
    }
    if result.len() > 128 {
        return err("static pogo pool exceeds 128 targets per scene");
    }
    Ok(Json::Obj(vec![
        ("targets".into(), Json::List(result)),
        ("unsupported".into(), Json::List(unsupported)),
        ("source_methods".into(), Json::List(vec![jstr("NailSlash.OnTriggerEnter2D"), jstr("NailSlash.OnTriggerStay2D")])),
        ("policy".into(), jstr("Static layer11/17/19 normal bouncers only; no active NonBouncer, HealthManager or special/dynamic bouncer")),
    ]))
}

fn set_field(fields: &mut Vec<(String, Json)>, key: &str, value: Json) {
    match fields.iter_mut().find(|f| f.0 == key) {
        Some(slot) => slot.1 = value,
        None => fields.push((key.to_string(), value)),
    }
}

fn int_of(j: Option<&Json>) -> Result<i64> {
    match j {
        Some(Json::Int(i)) => Ok(*i),
        _ => err("expected an int in the region report"),
    }
}

/// `postpack_pogo(report, source)`: the reproducible metadata-only pass that records each
/// scene's static pogo targets and binds them to the regions whose activation envelope they touch.
pub fn postpack_pogo(
    report: &mut Json,
    source: &hk_unity::Source,
    scene_count: usize,
) -> Result<()> {
    let Json::Obj(top) = report else {
        return err("report is not an object");
    };
    let take = |top: &mut Vec<(String, Json)>, key: &str| -> Result<Vec<Json>> {
        match top.iter_mut().find(|f| f.0 == key) {
            Some((_, Json::List(l))) => Ok(std::mem::take(l)),
            _ => err(format!("report lacks {key}")),
        }
    };
    let mut scenes = take(top, "scenes")?;
    let mut regions = take(top, "regions")?;
    let outcome = (|| -> Result<()> {
        // Sorted by scene id, which must be the dense range the world scene table holds.
        let mut order: Vec<usize> = (0..scenes.len()).collect();
        order.sort_by_key(|&i| {
            field(&scenes[i], "scene_id")
                .and_then(|j| if let Json::Int(v) = j { Some(*v) } else { None })
                .unwrap_or(i64::MAX)
        });
        let ids: Vec<Option<i64>> = order
            .iter()
            .map(|&i| {
                field(&scenes[i], "scene_id").and_then(|j| {
                    if let Json::Int(v) = j {
                        Some(*v)
                    } else {
                        None
                    }
                })
            })
            .collect();
        if ids.iter().enumerate().any(|(n, id)| *id != Some(n as i64)) || scenes.len() > scene_count
        {
            return err("pogo scene IDs must match bounded world scene table");
        }
        for &si in &order {
            let scene_id = int_of(field(&scenes[si], "scene_id"))?;
            let file = match field(&scenes[si], "file").or_else(|| field(&scenes[si], "scene_file"))
            {
                Some(Json::Str(f)) => f.clone(),
                _ => return err("scene without a file"),
            };
            let sc = Scene::new(source, &file).map_err(|e| e.to_string())?;
            let records = pogo_sources(&sc)?;
            let mut owned: Vec<(String, i64)> = Vec::new();
            for region in regions
                .iter()
                .filter(|r| field(r, "scene_id") == Some(&Json::Int(scene_id)))
            {
                let Some(Json::List(props)) = field(region, "breakables") else {
                    return err("region lacks breakables");
                };
                for prop in props {
                    let Some(Json::List(colliders)) = field(prop, "disabled_collider_sources")
                    else {
                        return err("breakable lacks disabled_collider_sources");
                    };
                    let state = scene_id * 128 + int_of(field(prop, "state_index"))?;
                    for c in colliders {
                        let Json::Str(c) = c else { continue };
                        match owned.iter_mut().find(|o| o.0 == *c) {
                            Some(slot) => slot.1 = state,
                            None => owned.push((c.clone(), state)),
                        }
                    }
                }
            }
            let Json::Obj(mut record_fields) = records else {
                return err("pogo records");
            };
            let Some(slot) = record_fields.iter_mut().find(|f| f.0 == "targets") else {
                return err("pogo targets");
            };
            let Json::List(targets) = &mut slot.1 else {
                return err("pogo targets");
            };
            for target in targets.iter_mut() {
                let Json::Obj(tf) = target else { continue };
                let source_id = match tf.iter().find(|f| f.0 == "source") {
                    Some((_, Json::Str(s))) => s.clone(),
                    _ => String::new(),
                };
                set_field(
                    tf,
                    "breakable_state_id",
                    owned
                        .iter()
                        .find(|o| o.0 == source_id)
                        .map_or(Json::Null, |o| Json::Int(o.1)),
                );
            }
            let targets = targets.clone();
            // Targets ride in the world bank as objects of every region whose activation envelope they touch.
            for region in regions
                .iter_mut()
                .filter(|r| field(r, "scene_id") == Some(&Json::Int(scene_id)))
            {
                let b = floats(
                    field(region, "activation_bounds").ok_or("region lacks activation_bounds")?,
                );
                let touching: Vec<Json> = targets
                    .iter()
                    .filter(|t| {
                        let tb = field(t, "bounds").map(floats).unwrap_or_default();
                        tb[0] <= b[2] && tb[2] >= b[0] && tb[1] <= b[3] && tb[3] >= b[1]
                    })
                    .cloned()
                    .collect();
                if let Json::Obj(rf) = region {
                    set_field(rf, "pogo_targets", Json::List(touching));
                }
            }
            if let Json::Obj(sf) = &mut scenes[si] {
                set_field(sf, "static_pogo", Json::Obj(record_fields));
            }
        }
        Ok(())
    })();
    let Json::Obj(top) = report else {
        unreachable!()
    };
    for (key, value) in [("scenes", scenes), ("regions", regions)] {
        if let Some(slot) = top.iter_mut().find(|f| f.0 == key) {
            slot.1 = Json::List(value);
        }
    }
    outcome
}
