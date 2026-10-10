//! What the per-enemy recognizers (host/baldur.py, aspid.py, ...) share: the
//! audited-assembly check, the FSM lookups and the tolerant comparisons they
//! make against the values the audit recorded.

use crate::common::{err, get, Result};
use crate::cook_audio::sha;
use crate::runner::ASSEMBLIES;
use hk_unity::{Source, Value};

/// Python `abs(float(a) - float(b)) <= 1e-6`; a non-number is a failure.
pub fn near(a: Option<&Value>, b: f64) -> bool {
    a.and_then(|v| match v {
        Value::Bool(x) => Some(*x as i64 as f64),
        other => other.float(),
    })
    .is_some_and(|a| (a - b).abs() <= 1e-6)
}

/// `_scalar`: a compact FSM parameter's variable name or its value.
pub fn scalar(v: &Value) -> Value {
    if v.is_map() {
        if v.get("useVariable").is_some_and(Value::truthy) {
            v.get("name").cloned().unwrap_or(Value::Str(Vec::new()))
        } else {
            v.get("value").cloned().unwrap_or(Value::Bool(false))
        }
    } else {
        v.clone()
    }
}

/// `_only`: the single record of a kind, or a refusal naming `who`.
pub fn only<'a>(
    records: &[(i64, &str, &'a Value)],
    kind: &str,
    who: &str,
) -> Result<(i64, &'a Value)> {
    let matches: Vec<_> = records.iter().filter(|r| r.1 == kind).collect();
    if matches.len() != 1 {
        return err(format!("{who} requires exactly one {kind}"));
    }
    Ok((matches[0].0, matches[0].2))
}

/// The audited managed assemblies still hash as audited.
pub fn check_assemblies(source: &Source, who: &str) -> Result<()> {
    for (name, expected) in ASSEMBLIES {
        let bytes = std::fs::read(source.directory.join("Managed").join(name))
            .map_err(|e| format!("{name}: {e}"))?;
        if sha(&bytes) != expected {
            return err(format!(
                "{who} methods require a fresh source audit: {name}"
            ));
        }
    }
    Ok(())
}

/// `{v['name']: v['value'] for group in fsm['variables'].values() if list for v in group if dict with name and value}`.
pub fn variables(fsm: &Value) -> Vec<(String, &Value)> {
    let mut out: Vec<(String, &Value)> = Vec::new();
    if let Some(Value::Map(groups)) = fsm.get("variables") {
        for (_, group) in groups {
            for v in group.list().unwrap_or(&[]) {
                if v.is_map() && v.get("name").is_some() && v.get("value").is_some() {
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
}

/// `{st['name']: st for st in fsm['states']}`: the last of a repeated name wins.
pub fn states(fsm: &Value) -> Result<Vec<(String, &Value)>> {
    let mut out: Vec<(String, &Value)> = Vec::new();
    for st in get(fsm, "states")?.list().ok_or("states is not a list")? {
        let n = get(st, "name")?.str().unwrap_or_default();
        match out.iter_mut().find(|(k, _)| *k == n) {
            Some(slot) => slot.1 = st,
            None => out.push((n, st)),
        }
    }
    Ok(out)
}

pub fn state<'a>(states: &[(String, &'a Value)], name: &str) -> Option<&'a Value> {
    states.iter().find(|(k, _)| k == name).map(|(_, v)| *v)
}

/// `[(t['fsmEvent']['name'], t['toState']) for t in state['transitions']]`.
pub fn transitions(state: &Value) -> Result<Vec<(String, String)>> {
    get(state, "transitions")?
        .list()
        .unwrap_or(&[])
        .iter()
        .map(|t| {
            Ok((
                get(get(t, "fsmEvent")?, "name")?.str().unwrap_or_default(),
                get(t, "toState")?.str().unwrap_or_default(),
            ))
        })
        .collect()
}

/// A tk2d library's clips keyed by name (clips with an empty name left out).
pub fn clips_by_name(library: &Value) -> Result<Vec<(String, &Value)>> {
    let mut out: Vec<(String, &Value)> = Vec::new();
    for c in get(library, "clips")?.list().unwrap_or(&[]) {
        let n = get(c, "name")?.str().unwrap_or_default();
        if n.is_empty() {
            continue;
        }
        match out.iter_mut().find(|(k, _)| *k == n) {
            Some(slot) => slot.1 = c,
            None => out.push((n, c)),
        }
    }
    Ok(out)
}

/// `(len(clip['frames']), clip['fps'], clip['wrapMode']) == (frames, fps, wrap)`.
pub fn clip_is(clip: Option<&Value>, frames: usize, fps: f64, wrap: i64) -> bool {
    clip.is_some_and(|c| {
        c.get("frames").and_then(Value::list).map(<[Value]>::len) == Some(frames)
            && c.get("fps").and_then(|f| match f {
                Value::Bool(_) => None,
                other => other.float(),
            }) == Some(fps)
            && c.get("wrapMode").and_then(Value::int) == Some(wrap)
    })
}

pub fn xy(v: &Value, key: &str) -> Result<[f64; 2]> {
    let p = get(v, key)?;
    Ok([
        get(p, "x")?.float().ok_or("not a number")?,
        get(p, "y")?.float().ok_or("not a number")?,
    ])
}

/// `body_box`: the actor's own BoxCollider2D; some placements carry the same box twice.
pub fn body_box<'a>(records: &[(i64, &str, &'a Value)], who: &str) -> Result<&'a Value> {
    let boxes: Vec<&Value> = records
        .iter()
        .filter(|r| r.1 == "BoxCollider2D")
        .map(|r| r.2)
        .collect();
    let same = |a: &Value, b: &Value| {
        ["m_Size", "m_Offset", "m_IsTrigger", "m_Enabled"]
            .iter()
            .all(|k| a.get(k).zip(b.get(k)).is_some_and(|(x, y)| x.py_eq(y)))
    };
    if !(1..=2).contains(&boxes.len()) || boxes.iter().any(|b| !same(b, boxes[0])) {
        return err(format!("unsupported {who} body colliders"));
    }
    Ok(boxes[0])
}

/// `focus.fsm_variables`: name to serialized value, refusing a repeated name whose values differ.
pub fn variables_strict(fsm: &Value) -> Result<Vec<(String, Option<&Value>)>> {
    let mut out: Vec<(String, Option<&Value>)> = Vec::new();
    if let Some(Value::Map(groups)) = fsm.get("variables") {
        for (_, group) in groups {
            for v in group.list().unwrap_or(&[]) {
                if !v.is_map() || v.get("name").is_none() {
                    continue;
                }
                let name = v.get("name").and_then(Value::str).unwrap_or_default();
                let value = v.get("value");
                match out.iter_mut().find(|(k, _)| *k == name) {
                    Some(slot) => {
                        let differs = match (slot.1, value) {
                            (Some(a), Some(b)) => !a.py_eq(b),
                            (None, None) => false,
                            _ => true,
                        };
                        if differs {
                            return err(format!(
                                "FSM {:?} declares {name:?} twice with different values",
                                fsm.get("name").and_then(Value::str).unwrap_or_default()
                            ));
                        }
                        slot.1 = value;
                    }
                    None => out.push((name, value)),
                }
            }
        }
    }
    Ok(out)
}

/// What an audited action parameter must be.
pub enum Want {
    F(f64),
    I(i64),
    S(&'static str),
    B(bool),
}

/// One audited `(state, action, expected parameters)` row of an `ACTIONS` table.
pub type ActionRow<'a> = (&'a str, &'a str, &'a [(&'a str, Want)]);

/// The `ACTIONS` table check of the Aspid and Vengefly recognizers: each
/// `(state, action)` must have exactly one enabled action of that name, whose
/// compact parameters equal the audited values (numbers within 1e-6 when the
/// audit recorded a float).
pub fn check_actions(sts: &[(String, &Value)], table: &[ActionRow], who: &str) -> Result<()> {
    for &(st, action, expected) in table {
        let data = get(state(sts, st).ok_or("missing state")?, "actionData")?;
        let names = get(data, "actionNames")?.list().unwrap_or(&[]);
        let enabled = get(data, "actionEnabled")?.list().unwrap_or(&[]);
        let matches: Vec<usize> = names
            .iter()
            .enumerate()
            .filter(|(i, n)| {
                n.str()
                    .is_some_and(|n| n.rsplit('.').next() == Some(action))
                    && enabled.get(*i).is_some_and(Value::truthy)
            })
            .map(|(i, _)| i)
            .collect();
        if matches.len() != 1 {
            return err(format!("unsupported {who} action set: {st}/{action}"));
        }
        let fields = hk_unity::playmaker::action_fields(data, matches[0], false)
            .map_err(|e| e.to_string())?;
        for (key, want) in expected {
            let actual = fields.iter().find(|f| f.0 == *key).map(|f| scalar(&f.1));
            let numeric = |a: &Value| {
                matches!(
                    a,
                    Value::Int(_) | Value::UInt(_) | Value::F32(_) | Value::F64(_)
                )
            };
            let ok = match want {
                Want::F(v) => match &actual {
                    Some(a) if numeric(a) => near(Some(a), *v),
                    other => other.as_ref().is_some_and(|a| a.py_eq(&Value::F64(*v))),
                },
                Want::I(v) => actual.as_ref().is_some_and(|a| a.py_eq(&Value::Int(*v))),
                Want::S(s) => actual.as_ref().and_then(Value::str).as_deref() == Some(*s),
                Want::B(b) => actual.as_ref().is_some_and(|a| a.py_eq(&Value::Bool(*b))),
            };
            if !ok {
                return err(format!("unsupported {who} parameter: {st}/{action}.{key}"));
            }
        }
    }
    Ok(())
}

/// The children of a game object by name, in object order (the last of a name wins).
pub fn children_of(sc: &hk_unity::scene::Scene, gid: i64) -> Result<Vec<(String, i64)>> {
    let tid = *sc.go_transform.get(&gid).ok_or("object has no transform")?;
    let mut out: Vec<(String, i64)> = Vec::new();
    for o in sc.objects.iter().filter(|o| o.typename == "Transform") {
        if get(get(&o.tree, "m_Father")?, "m_PathID")?.int() != Some(tid) {
            continue;
        }
        let g = get(get(&o.tree, "m_GameObject")?, "m_PathID")?
            .int()
            .unwrap_or(0);
        let name = get(sc.go(g).ok_or("child without a GameObject")?, "m_Name")?
            .str()
            .unwrap_or_default();
        match out.iter_mut().find(|c| c.0 == name) {
            Some(slot) => slot.1 = g,
            None => out.push((name, g)),
        }
    }
    Ok(out)
}

/// The audited clips (name, frames, fps, wrap mode, optional loop start) a library must carry;
/// the first one that differs.
pub fn clips_ok(
    library: &Value,
    table: &[(&str, usize, f64, i64, Option<i64>)],
) -> Result<Option<String>> {
    let by_name = clips_by_name(library)?;
    for &(name, frames, fps, wrap, loop_start) in table {
        let clip = by_name.iter().find(|(k, _)| k == name).map(|(_, v)| *v);
        let ok = clip_is(clip, frames, fps, wrap)
            && loop_start.is_none_or(|l| {
                clip.and_then(|c| c.get("loopStart"))
                    .map_or(0, |v| v.int().unwrap_or(-1))
                    == l
            });
        if !ok {
            return Ok(Some(name.to_string()));
        }
    }
    Ok(None)
}

/// Python `{name: (gid, tid)}` over a transform's `m_Children`, the last of a name winning.
pub fn child_map(sc: &hk_unity::scene::Scene, tid: i64) -> Result<Vec<(String, (i64, i64))>> {
    let mut children: Vec<(String, (i64, i64))> = Vec::new();
    for child in get(sc.transform(tid).ok_or("transform missing")?, "m_Children")?
        .list()
        .unwrap_or(&[])
    {
        let ctid = get(child, "m_PathID")?.int().unwrap_or(0);
        let kid = get(
            get(
                sc.transform(ctid).ok_or("child transform missing")?,
                "m_GameObject",
            )?,
            "m_PathID",
        )?
        .int()
        .unwrap_or(0);
        let name = get(sc.go(kid).ok_or("child without a GameObject")?, "m_Name")?
            .str()
            .unwrap_or_default();
        match children.iter_mut().find(|c| c.0 == name) {
            Some(slot) => slot.1 = (kid, ctid),
            None => children.push((name, (kid, ctid))),
        }
    }
    Ok(children)
}

/// `tree[key] == want` with Python's equality; a missing field is the KeyError.
pub fn eq_field(tree: &Value, key: &str, want: &Value) -> Result<bool> {
    Ok(get(tree, key)?.py_eq(want))
}

/// `bool(tree[key])`.
pub fn flag(tree: &Value, key: &str) -> Result<bool> {
    Ok(get(tree, key)?.truthy())
}

/// `any(abs(matrix[i][j] - (1 if i == j else 0)) > 1e-6 for i in range(2) for j in range(2))`, negated:
/// the world matrix has an identity 2x2 basis.
pub fn identity_basis(m: &[[f64; 4]; 4]) -> bool {
    (0..2).all(|i| (0..2).all(|j| (m[i][j] - if i == j { 1.0 } else { 0.0 }).abs() <= 1e-6))
}

/// A tk2dSprite with no tint, no scale and no collider of its own to keep in step
/// (`_color` and `_scale` at one, `boxCollider2D` null, no `polygonCollider2D`).
pub fn plain_sprite(sprite: &Value) -> Result<bool> {
    let color = Value::Map(
        ["r", "g", "b", "a"]
            .iter()
            .map(|k| ((*k).into(), Value::F64(1.0)))
            .collect(),
    );
    let scale = Value::Map(
        ["x", "y", "z"]
            .iter()
            .map(|k| ((*k).into(), Value::F64(1.0)))
            .collect(),
    );
    Ok(get(sprite, "_color")?.py_eq(&color)
        && get(sprite, "_scale")?.py_eq(&scale)
        && !get(get(sprite, "boxCollider2D")?, "m_PathID")?.truthy()
        && !get(sprite, "polygonCollider2D")?.truthy())
}
