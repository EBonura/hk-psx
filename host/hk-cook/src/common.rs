//! Helpers the Python cookers share across modules (geo.q16, actors._component_records,
//! breakables.collider_polygons, the PlayMaker contract checks in props.py, ...),
//! with Python's rounding and ordering kept.

use hk_unity::playmaker::{action_fields, Fields};
use hk_unity::scene::Scene;
use hk_unity::Value;

pub type Result<T> = std::result::Result<T, String>;

pub fn err<T>(msg: impl Into<String>) -> Result<T> {
    Err(msg.into())
}

/// Python `round(v)` (half to even) to an integer.
pub fn py_round(v: f64) -> i64 {
    v.round_ties_even() as i64
}

/// geo.py `q16`: 16.16 fixed point with its range check.
pub fn q16(v: f64) -> Result<i64> {
    if !v.is_finite() || v.abs() > 32767.0 {
        return err("Geo fixed-point range");
    }
    Ok(py_round(v * 65536.0))
}

/// geo.py `rust_array`.
pub fn rust_array<T: std::fmt::Display>(values: &[T]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(",")
    )
}

pub fn get<'a>(v: &'a Value, key: &str) -> Result<&'a Value> {
    v.get(key).ok_or_else(|| format!("missing field {key}"))
}

pub fn f64_of(v: &Value, key: &str) -> Result<f64> {
    get(v, key)?
        .float()
        .ok_or_else(|| format!("{key} is not a number"))
}

pub fn int_of(v: &Value, key: &str) -> Result<i64> {
    get(v, key)?
        .int()
        .ok_or_else(|| format!("{key} is not an int"))
}

pub fn str_of(v: &Value, key: &str) -> Result<String> {
    get(v, key)?
        .str()
        .ok_or_else(|| format!("{key} is not a string"))
}

pub fn path_id(v: &Value) -> Option<i64> {
    v.get("m_PathID").and_then(Value::int)
}

pub fn go_of(tree: &Value) -> Option<i64> {
    tree.get("m_GameObject").and_then(path_id)
}

/// actors.py `_component_records`: every object of the scene on GameObject `gid`, in order.
pub fn component_records<'a>(sc: &'a Scene, gid: i64) -> Vec<(i64, &'a str, &'a Value)> {
    sc.objects
        .iter()
        .filter(|o| go_of(&o.tree) == Some(gid))
        .map(|o| (o.id, o.typename.as_str(), &o.tree))
        .collect()
}

/// breakables.py `_components`: the GameObject's component list, resolved in the scene.
pub fn components<'a>(sc: &'a Scene, gid: i64) -> Result<Vec<(i64, &'a str, &'a Value)>> {
    let go = sc.go(gid).ok_or("no such GameObject")?;
    let mut out = Vec::new();
    for c in get(go, "m_Component")?.list().unwrap_or(&[]) {
        let r = get(c, "component")?;
        if r.get("m_FileID").and_then(Value::int).unwrap_or(0) != 0 {
            return err("external Breakable scene-object reference unsupported");
        }
        let id = path_id(r).unwrap_or(0);
        if let Some(o) = sc.object(id) {
            out.push((o.id, o.typename.as_str(), &o.tree));
        }
    }
    Ok(out)
}

pub fn one<'a>(records: &[(i64, &str, &'a Value)], kind: &str) -> Result<(i64, &'a Value)> {
    let hits: Vec<_> = records.iter().filter(|r| r.1 == kind).collect();
    if hits.len() != 1 {
        return err(format!("expected one {kind}"));
    }
    Ok((hits[0].0, hits[0].2))
}

pub fn fsm<'a>(records: &[(i64, &str, &'a Value)], name: &str) -> Result<&'a Value> {
    let fsms: Vec<&Value> = records
        .iter()
        .filter(|r| r.1 == "PlayMakerFSM")
        .filter_map(|r| r.2.get("fsm"))
        .filter(|f| f.get("name").and_then(Value::str).as_deref() == Some(name))
        .collect();
    if fsms.len() != 1 {
        return err(format!("expected one {name} FSM"));
    }
    Ok(fsms[0])
}

pub fn has_fsm(records: &[(i64, &str, &Value)], name: &str) -> bool {
    records.iter().any(|r| {
        r.1 == "PlayMakerFSM"
            && r.2
                .get("fsm")
                .and_then(|f| f.get("name"))
                .and_then(Value::str)
                .as_deref()
                == Some(name)
    })
}

/// props.py `_states`: the states by name, checked against a transition contract.
pub fn states<'a>(
    fsm: &'a Value,
    contract: &[(&str, &[(&str, &str)])],
    who: &str,
) -> Result<Vec<(String, &'a Value)>> {
    let mut by_name: Vec<(String, &Value)> = Vec::new();
    for s in get(fsm, "states")?.list().unwrap_or(&[]) {
        let n = str_of(s, "name")?;
        match by_name.iter_mut().find(|(k, _)| *k == n) {
            Some(slot) => slot.1 = s,
            None => by_name.push((n, s)),
        }
    }
    for (name, expected) in contract {
        let Some((_, s)) = by_name.iter().find(|(k, _)| k == name) else {
            return err(format!("unsupported {who} transitions: {name}"));
        };
        let got: Vec<(String, String)> = get(s, "transitions")?
            .list()
            .unwrap_or(&[])
            .iter()
            .map(|t| {
                let e = t
                    .get("fsmEvent")
                    .and_then(|e| e.get("name"))
                    .and_then(Value::str)
                    .unwrap_or_default();
                (e, t.get("toState").and_then(Value::str).unwrap_or_default())
            })
            .collect();
        let want: Vec<(String, String)> = expected
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect();
        if got != want {
            return err(format!("unsupported {who} transitions: {name}"));
        }
    }
    Ok(by_name)
}

pub fn state<'a>(states: &[(String, &'a Value)], name: &str) -> Result<&'a Value> {
    states
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| *v)
        .ok_or_else(|| format!("no state {name}"))
}

/// props.py `_actions`: the enabled actions of one kind, decoded.
pub fn actions(state: &Value, kind: &str) -> Result<Vec<Fields>> {
    let d = get(state, "actionData")?;
    let names = get(d, "actionNames")?.list().unwrap_or(&[]);
    let enabled = get(d, "actionEnabled")?.list().unwrap_or(&[]);
    let mut out = Vec::new();
    for (i, n) in names.iter().enumerate() {
        let n = n.str().unwrap_or_default();
        if n.rsplit('.').next() == Some(kind) && enabled.get(i).is_some_and(Value::truthy) {
            out.push(action_fields(d, i, false).map_err(|e| e.to_string())?);
        }
    }
    Ok(out)
}

/// props.py `_scalar`: a compact scalar's value, or its variable's name.
pub fn scalar(v: &Value) -> Value {
    if v.is_map() {
        if v.get("useVariable").is_some_and(Value::truthy) {
            return v.get("name").cloned().unwrap_or(Value::Bool(false));
        }
        return v.get("value").cloned().unwrap_or(Value::Bool(false));
    }
    v.clone()
}

pub fn field<'a>(f: &'a Fields, key: &str) -> Result<&'a Value> {
    hk_unity::playmaker::field(f, key).ok_or_else(|| format!("action lacks {key}"))
}

/// props.py `_variables`.
pub fn variables(fsm: &Value) -> Vec<(String, Value)> {
    let mut out: Vec<(String, Value)> = Vec::new();
    if let Some(Value::Map(groups)) = fsm.get("variables") {
        for (_, g) in groups {
            let Value::List(items) = g else { continue };
            for v in items {
                if let (Some(n), Some(val)) = (v.get("name"), v.get("value")) {
                    let n = n.str().unwrap_or_default();
                    match out.iter_mut().find(|(k, _)| *k == n) {
                        Some(slot) => slot.1 = val.clone(),
                        None => out.push((n, val.clone())),
                    }
                }
            }
        }
    }
    out
}

/// props.py `_kids`: direct children by name (a later same-named child wins).
pub fn kids(sc: &Scene, gid: i64) -> Result<Vec<(String, i64)>> {
    let tid = *sc.go_transform.get(&gid).ok_or("object has no transform")?;
    let t = sc.transform(tid).ok_or("transform missing")?;
    let mut out: Vec<(String, i64)> = Vec::new();
    for child in get(t, "m_Children")?.list().unwrap_or(&[]) {
        let ct = sc
            .transform(path_id(child).unwrap_or(0))
            .ok_or("child transform missing")?;
        let kid = go_of(ct).unwrap_or(0);
        if let Some(go) = sc.go(kid) {
            let name = str_of(go, "m_Name")?;
            match out.iter_mut().find(|(k, _)| *k == name) {
                Some(slot) => slot.1 = kid,
                None => out.push((name, kid)),
            }
        }
    }
    Ok(out)
}

pub fn kid(kids: &[(String, i64)], name: &str) -> Option<i64> {
    kids.iter().find(|(k, _)| k == name).map(|(_, v)| *v)
}

/// Python's `min`/`max` over floats: the first of equal values wins.
pub fn py_min(v: impl IntoIterator<Item = f64>) -> f64 {
    v.into_iter()
        .reduce(|a, b| if b < a { b } else { a })
        .unwrap()
}
pub fn py_max(v: impl IntoIterator<Item = f64>) -> f64 {
    v.into_iter()
        .reduce(|a, b| if b > a { b } else { a })
        .unwrap()
}

/// props.py `_box_world`: world bounds of a BoxCollider2D under any quarter turn.
pub fn box_world(sc: &Scene, gid: i64, b: &Value) -> Result<[f64; 4]> {
    let (ox, oy) = (
        f64_of(get(b, "m_Offset")?, "x")?,
        f64_of(get(b, "m_Offset")?, "y")?,
    );
    let (hx, hy) = (
        f64_of(get(b, "m_Size")?, "x")? / 2.0,
        f64_of(get(b, "m_Size")?, "y")? / 2.0,
    );
    let mut pts = Vec::new();
    for dx in [-hx, hx] {
        for dy in [-hy, hy] {
            let p = sc
                .point(gid, ox + dx, oy + dy, 0.0)
                .map_err(|e| e.to_string())?;
            pts.push((p[0], p[1]));
        }
    }
    Ok([
        py_min(pts.iter().map(|p| p.0)),
        py_min(pts.iter().map(|p| p.1)),
        py_max(pts.iter().map(|p| p.0)),
        py_max(pts.iter().map(|p| p.1)),
    ])
}

/// props.py `quarter`: (quarter turns, mirror, horizontal stretch) of a 2x2 world matrix.
pub fn quarter(m: &[[f64; 4]; 4]) -> Result<(i64, i64, f64)> {
    let (a, b, c, d) = (m[0][0], m[0][1], m[1][0], m[1][1]);
    let (cos, sin) = (d, -b);
    if ((cos.abs() + sin.abs()) - 1.0).abs() > 1e-3 || cos.abs().min(sin.abs()) > 1e-3 {
        return err(format!(
            "prop rotation is not a quarter turn: {a},{b},{c},{d}"
        ));
    }
    let (rc, rs) = (py_round(cos), py_round(sin));
    let q = match (rc, rs) {
        (1, 0) => 0,
        (0, 1) => 1,
        (-1, 0) => 2,
        (0, -1) => 3,
        _ => return err("prop rotation is not a quarter turn"),
    };
    let x = a * rc as f64 + c * rs as f64;
    if (a * rs as f64 - c * rc as f64).abs() > 1e-3 || x.abs() < 1e-3 {
        return err(format!(
            "prop matrix is not a mirrored rotation: {a},{b},{c},{d}"
        ));
    }
    Ok((q, if x > 0.0 { 1 } else { -1 }, x.abs()))
}

/// breakables.py `collider_polygons` (MAX_HIT_POINTS 16).
pub fn collider_polygons(
    sc: &Scene,
    gid: i64,
    kind: &str,
    tree: &Value,
) -> Result<Vec<Vec<(f64, f64)>>> {
    let off = get(tree, "m_Offset")?;
    let (ox, oy) = (f64_of(off, "x")?, f64_of(off, "y")?);
    let paths: Vec<Vec<(f64, f64)>> = match kind {
        "BoxCollider2D" => {
            let (x, y) = (
                f64_of(get(tree, "m_Size")?, "x")? / 2.0,
                f64_of(get(tree, "m_Size")?, "y")? / 2.0,
            );
            vec![vec![(-x, -y), (x, -y), (x, y), (-x, y)]]
        }
        "PolygonCollider2D" => get(get(tree, "m_Points")?, "m_Paths")?
            .list()
            .unwrap_or(&[])
            .iter()
            .map(|p| {
                p.list()
                    .unwrap_or(&[])
                    .iter()
                    .map(|q| {
                        (
                            q.get("x").and_then(Value::float).unwrap_or(0.0),
                            q.get("y").and_then(Value::float).unwrap_or(0.0),
                        )
                    })
                    .collect()
            })
            .collect(),
        other => return err(format!("unsupported Breakable hit collider: {other}")),
    };
    let mut out = Vec::new();
    for path in paths {
        if !(3..=16).contains(&path.len()) {
            return err("Breakable hit polygon requires 3..16 source vertices");
        }
        let mut pts = Vec::new();
        for (x, y) in path {
            let p = sc
                .point(gid, x + ox, y + oy, 0.0)
                .map_err(|e| e.to_string())?;
            if !p[0].is_finite() || !p[1].is_finite() || p[0].abs() > 512.0 || p[1].abs() > 512.0 {
                return err("Breakable world coordinate exceeds bounded Q16 range");
            }
            pts.push((p[0], p[1]));
        }
        out.push(pts);
    }
    if out.is_empty() {
        return err("Breakable has no hit polygon");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding_and_fixed_point_follow_python() {
        assert_eq!(py_round(2.5), 2);
        assert_eq!(py_round(3.5), 4);
        assert_eq!(py_round(-2.5), -2);
        assert_eq!(q16(0.5).unwrap(), 32768);
        assert_eq!(q16(-1.0 / 131072.0).unwrap(), 0); // -0.5 rounds to even
        assert!(q16(40000.0).is_err());
        assert!(q16(f64::NAN).is_err());
        assert_eq!(rust_array(&[1, -2, 3]), "[1,-2,3]");
        assert_eq!(rust_array::<i64>(&[]), "[]");
    }

    #[test]
    fn quarter_turns() {
        let m = |a: f64, b: f64, c: f64, d: f64| {
            let mut x = [[0.0; 4]; 4];
            x[0][0] = a;
            x[0][1] = b;
            x[1][0] = c;
            x[1][1] = d;
            x
        };
        assert_eq!(quarter(&m(1.0, 0.0, 0.0, 1.0)).unwrap(), (0, 1, 1.0));
        assert_eq!(quarter(&m(-2.0, 0.0, 0.0, 1.0)).unwrap(), (0, -1, 2.0));
        assert_eq!(quarter(&m(0.0, -1.0, 1.0, 0.0)).unwrap(), (1, 1, 1.0));
        assert_eq!(quarter(&m(-1.0, 0.0, 0.0, -1.0)).unwrap(), (2, 1, 1.0));
        assert!(quarter(&m(0.7, -0.7, 0.7, 0.7)).is_err());
        assert!(quarter(&m(1.0, 0.0, 0.0, 2.0)).is_err());
    }

    #[test]
    fn python_min_max_keep_the_first_of_equals() {
        assert!(py_min([0.0, -0.0]).is_sign_positive());
        assert!(py_max([-0.0, 0.0]).is_sign_negative());
        assert_eq!(py_min([3.0, 1.0, 2.0]), 1.0);
    }
}
