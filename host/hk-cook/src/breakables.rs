//! Source-driven Breakable subset and the FSM-authored breakable families
//! (host/breakables.py): C# Breakables, arena gates, hidden walls, cracked
//! floors and infected vines.
//!
//! Records are `Json` trees whose key order is the Python dict's, because the
//! cook writes them out. Errors carry the text Python's `str(exception)` gave,
//! since the refusals are recorded verbatim in the reports.

use crate::break_effects::{part_emitter, prefab_emitters, scene_gravity, Found};
use crate::common::{collider_polygons, components, err, Result};
use crate::cook_audio::{jobj, js, u};
use crate::false_knight::fsm_digest;
use crate::music::value_json;
use crate::pyjson::Json;
use crate::pyset::{int_hash, PySet};
use hk_unity::playmaker::{action_fields, Fields};
use hk_unity::scene::Scene;
use hk_unity::{base_name, Obj, Value};
use std::collections::{BTreeSet, HashMap};

pub const MAX_SCENE_BREAKABLES: usize = 128;
pub const MAX_HIT_POINTS: usize = 16;

pub const OUTPUT_VISUAL: &str = "visual";
pub const OUTPUT_COLLIDER: &str = "collider";
pub const OUTPUT_AUDIO: &str = "audio";
pub const OUTPUT_PARTICLES: &str = "particles";
pub const OUTPUT_FRAGMENTS: &str = "fragments";
pub const OUTPUT_MASK: &str = "mask";
pub const OUTPUTS: [&str; 6] = [
    OUTPUT_VISUAL,
    OUTPUT_COLLIDER,
    OUTPUT_AUDIO,
    OUTPUT_PARTICLES,
    OUTPUT_FRAGMENTS,
    OUTPUT_MASK,
];
/// The one break sample resident in the SPU bank below 0x14000.
pub const RESIDENT_BREAK_CLIPS: [&str; 1] = ["breakable_wall_hit_1"];
pub const TERRAIN_LAYER: i64 = 8;
pub const ENFORCED_OUTPUTS: [&str; 3] = [OUTPUT_VISUAL, OUTPUT_PARTICLES, OUTPUT_MASK];

// ---------------------------------------------------------------- small helpers

/// `tree[key]`, failing the way a Python `KeyError` reads.
pub(crate) fn k<'a>(v: &'a Value, key: &str) -> Result<&'a Value> {
    v.get(key).ok_or_else(|| format!("'{key}'"))
}

pub(crate) fn kf(v: &Value, key: &str) -> Result<f64> {
    k(v, key)?
        .float()
        .ok_or_else(|| format!("{key} is not a number"))
}

pub(crate) fn ki(v: &Value, key: &str) -> Result<i64> {
    k(v, key)?
        .int()
        .ok_or_else(|| format!("{key} is not an int"))
}

pub(crate) fn ks(v: &Value, key: &str) -> Result<String> {
    k(v, key)?
        .str()
        .ok_or_else(|| format!("{key} is not a string"))
}

/// `tree[key]` as a list.
pub(crate) fn kl<'a>(v: &'a Value, key: &str) -> Result<&'a [Value]> {
    k(v, key)?
        .list()
        .ok_or_else(|| format!("{key} is not a list"))
}

pub(crate) fn jf(v: f64) -> Json {
    Json::Float(v)
}
pub(crate) fn ji(v: i64) -> Json {
    Json::Int(v)
}
pub(crate) fn jb(v: bool) -> Json {
    Json::Bool(v)
}
pub(crate) fn jl(v: Vec<Json>) -> Json {
    Json::List(v)
}
pub(crate) fn jstrs<S: AsRef<str>>(v: &[S]) -> Json {
    Json::List(v.iter().map(|s| js(s.as_ref())).collect())
}
pub(crate) fn jfloats(v: &[f64]) -> Json {
    Json::List(v.iter().map(|&f| jf(f)).collect())
}

/// Python's `dict.update` on an ordered object: an existing key keeps its place.
pub(crate) fn jset(obj: &mut Json, key: &str, value: Json) {
    if let Json::Obj(f) = obj {
        match f.iter_mut().find(|e| e.0 == key) {
            Some(slot) => slot.1 = value,
            None => f.push((key.to_string(), value)),
        }
    }
}

pub(crate) fn jget<'a>(j: &'a Json, key: &str) -> Option<&'a Json> {
    match j {
        Json::Obj(f) => f.iter().find(|e| e.0 == key).map(|e| &e.1),
        _ => None,
    }
}

pub(crate) fn jint(j: &Json) -> i64 {
    match j {
        Json::Int(i) => *i,
        _ => 0,
    }
}

pub(crate) fn jstr(j: &Json) -> String {
    match j {
        Json::Str(s) => s.clone(),
        _ => String::new(),
    }
}

/// `file` of a scene: its name without a directory.
pub(crate) fn file_of(sc: &Scene) -> String {
    base_name(&sc.base.name).to_string()
}

/// `sc.point(gid)` as a JSON list.
pub(crate) fn point(sc: &Scene, gid: i64) -> Result<[f64; 3]> {
    u(sc.point(gid, 0.0, 0.0, 0.0))
}

/// `_local_id(ref)`: the path id of an in-file reference.
pub(crate) fn local_id(r: &Value) -> Result<i64> {
    if ki(r, "m_FileID")? != 0 {
        return err("external Breakable scene-object reference unsupported");
    }
    ki(r, "m_PathID")
}

/// A component of a GameObject: object id, type name, serialized tree.
pub(crate) type Comp<'a> = (i64, &'a str, &'a Value);

/// `_descendants(sc, root)` in the iteration order of the Python set it returns.
pub fn descendants(sc: &Scene, root: i64) -> Result<Vec<i64>> {
    let mut children: HashMap<i64, Vec<i64>> = HashMap::new();
    for o in &sc.objects {
        if sc.transforms.contains_key(&o.id) {
            let father = ki(k(&o.tree, "m_Father")?, "m_PathID")?;
            children.entry(father).or_default().push(o.id);
        }
    }
    let start = *sc.go_transform.get(&root).ok_or("'root'")?;
    let mut stack = vec![start];
    let mut result: PySet<i64> = PySet::new();
    while let Some(tid) = stack.pop() {
        if result.contains(&tid, int_hash(tid)) {
            return err("cyclic Breakable part hierarchy");
        }
        result.add(tid, int_hash(tid));
        if let Some(c) = children.get(&tid) {
            stack.extend(c.iter().copied());
        }
    }
    let mut out: PySet<i64> = PySet::new();
    for tid in result.iter() {
        let t = sc.transform(*tid).ok_or_else(|| format!("{tid}"))?;
        let gid = ki(k(t, "m_GameObject")?, "m_PathID")?;
        out.add(gid, int_hash(gid));
    }
    Ok(out.iter().copied().collect())
}

/// `sorted(_descendants(...))`, as a set for membership and difference.
pub(crate) fn descendant_set(sc: &Scene, root: i64) -> Result<BTreeSet<i64>> {
    Ok(descendants(sc, root)?.into_iter().collect())
}

/// Python's `round(x, 4)`.
fn round4(x: f64) -> f64 {
    format!("{x:.4}").parse().unwrap_or(x)
}

// ----------------------------------------------------------------- audio, rigid

fn clip(
    sc: &Scene,
    file: &std::sync::Arc<hk_unity::serialized::SerializedFile>,
    reference: &Value,
    weight: f64,
) -> Result<Json> {
    let c = u(sc.source.deref(file, reference))?;
    let data = u(sc.source.read(&c))?;
    Ok(jobj(vec![
        ("source", js(&c.sid())),
        ("weight", jf(weight)),
        ("name", Json::Str(ks(&data, "m_Name")?)),
        ("channels", value_json(k(&data, "m_Channels")?)),
        ("frequency", value_json(k(&data, "m_Frequency")?)),
        ("seconds", value_json(k(&data, "m_Length")?)),
        (
            "compression_format",
            value_json(k(&data, "m_CompressionFormat")?),
        ),
        ("resource", value_json(k(&data, "m_Resource")?)),
    ]))
}

/// `_audio(sc, tree)`: both source break-sound paths, not only the weighted table.
pub fn audio(sc: &Scene, tree: &Value) -> Result<Json> {
    let event = k(tree, "breakAudioEvent")?;
    let mut out = jobj(vec![
        ("event", value_json(event)),
        ("table_id", Json::Null),
        ("options", jl(vec![])),
    ]);
    let table_ref = k(tree, "breakAudioClipTable")?;
    let mut options = Vec::new();
    if ki(table_ref, "m_PathID")? != 0 {
        let obj = u(sc.source.deref(&sc.base, table_ref))?;
        let table = u(sc.source.read(&obj))?;
        jset(&mut out, "table_id", js(&obj.sid()));
        jset(&mut out, "pitch_min", value_json(k(&table, "pitchMin")?));
        jset(&mut out, "pitch_max", value_json(k(&table, "pitchMax")?));
        for option in kl(&table, "options")? {
            let c = k(option, "Clip")?;
            if ki(c, "m_PathID")? != 0 {
                options.push(clip(sc, &obj.file, c, kf(option, "Weight")?)?);
            }
        }
    } else if ki(k(event, "Clip")?, "m_PathID")? != 0 {
        jset(&mut out, "event_clip", jb(true));
        jset(&mut out, "pitch_min", value_json(k(event, "PitchMin")?));
        jset(&mut out, "pitch_max", value_json(k(event, "PitchMax")?));
        options.push(clip(sc, &sc.base, k(event, "Clip")?, 1.0)?);
    }
    jset(&mut out, "options", jl(options));
    Ok(out)
}

const RIGID_BODIES: [[f64; 5]; 3] = [
    [0.0, 1.0, 0.05, 1.0, 0.0],
    [0.0, 1.0, 0.05, 0.9, 0.0],
    [0.0, 1.0, 3.0, 1.0, 0.0],
];
const RIGID_BOUNCE_FACTORS: [f64; 3] = [0.1, 0.4, 0.5];
const SPIN_COMPONENTS: [&str; 2] = ["SpinSelf", "SpinSelfSimple"];

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-5
}

/// A number as Python prints it inside a tuple.
fn pynum(v: &Value) -> String {
    match v {
        Value::F32(f) => crate::pyfloat::repr(*f as f64),
        Value::F64(f) => crate::pyfloat::repr(*f),
        other => other.int().map_or_else(String::new, |i| i.to_string()),
    }
}

/// `rigid_fragment(sc, part_gid)`: the shared rigid-fling definition, a reason
/// this part is not one (Err), or None when it is not a rigid fling at all.
pub fn rigid_fragment(sc: &Scene, part_gid: i64) -> Result<Option<Json>> {
    let mut comps: Vec<(&str, i64, &Value)> = Vec::new();
    for (index, typ, data) in components(sc, part_gid)? {
        match comps.iter_mut().find(|c| c.0 == typ) {
            Some(slot) => *slot = (typ, index, data),
            None => comps.push((typ, index, data)),
        }
    }
    let find = |name: &str| comps.iter().find(|c| c.0 == name);
    if find("Rigidbody2D").is_none() || find("SpriteRenderer").is_none() {
        return Ok(None);
    }
    let colliders: Vec<&str> = comps
        .iter()
        .map(|c| c.0)
        .filter(|t| t.ends_with("Collider2D"))
        .collect();
    let spins: Vec<&str> = SPIN_COMPONENTS
        .iter()
        .copied()
        .filter(|n| find(n).is_some())
        .collect();
    if colliders.len() != 1 {
        return err(format!(
            "rigid fragment needs exactly one collider, has {}",
            colliders.len()
        ));
    }
    if spins.len() > 1 {
        return err("rigid fragment carries two spin behaviours");
    }
    if find("ObjectBounce").is_none() {
        return err("rigid fragment has no ObjectBounce landing behaviour");
    }
    let body = find("Rigidbody2D").unwrap().2;
    let fields = [
        kf(body, "m_BodyType")?,
        kf(body, "m_Mass")?,
        round4(kf(body, "m_AngularDamping")?),
        round4(kf(body, "m_GravityScale")?),
        kf(body, "m_Constraints")?,
    ];
    if k(body, "m_UseAutoMass")?.truthy() || k(body, "m_LinearDamping")?.truthy() {
        return err("rigid fragment uses auto mass or linear damping");
    }
    if !RIGID_BODIES
        .iter()
        .any(|variant| fields.iter().zip(variant).all(|(a, b)| close(*a, *b)))
    {
        let shown = [
            pynum(k(body, "m_BodyType")?),
            crate::pyfloat::repr(fields[1]),
            crate::pyfloat::repr(fields[2]),
            crate::pyfloat::repr(fields[3]),
            pynum(k(body, "m_Constraints")?),
        ];
        return err(format!(
            "unmeasured rigid fragment body ({})",
            shown.join(", ")
        ));
    }
    let bounce = find("ObjectBounce").unwrap().2;
    let factor = kf(bounce, "bounceFactor")?;
    if !RIGID_BOUNCE_FACTORS.iter().any(|v| close(factor, *v)) {
        return err(format!(
            "unmeasured fragment bounce factor {}",
            pynum(k(bounce, "bounceFactor")?)
        ));
    }
    if kf(bounce, "speedThreshold")? != 1.0
        || ["playSound", "playAnimationOnBounce", "sendFSMEvent"]
            .iter()
            .any(|key| k(bounce, key).is_ok_and(Value::truthy))
    {
        return err("fragment bounce drives sound, animation or an FSM event");
    }
    let spin = spins.first().map(|n| find(n).unwrap().2);
    if let (Some(&"SpinSelfSimple"), Some(s)) = (spins.first(), spin) {
        if k(s, "randomStartRotation")?.truthy() || k(s, "waitForCall")?.truthy() {
            return err("SpinSelfSimple fragment waits for a call or randomizes its start");
        }
    }
    Ok(Some(jobj(vec![
        (
            "game_object",
            Json::Str(format!("{}:{part_gid}", file_of(sc))),
        ),
        ("renderer", ji(find("SpriteRenderer").unwrap().1)),
        ("collider_type", js(colliders[0])),
        ("spin", spins.first().map_or(Json::Null, |s| js(s))),
        (
            "spin_factor",
            match spin {
                Some(s) => value_json(k(s, "spinFactor")?),
                None => jf(0.0),
            },
        ),
        ("bounce_factor", value_json(k(bounce, "bounceFactor")?)),
        (
            "body",
            jl(vec![
                value_json(k(body, "m_BodyType")?),
                value_json(k(body, "m_Mass")?),
                jf(fields[2]),
                jf(fields[3]),
                value_json(k(body, "m_Constraints")?),
            ]),
        ),
    ])))
}

/// A literal action parameter.
pub(crate) enum Lit {
    F(f64),
    B(bool),
    I(i64),
}

/// `_literal_action(data, name, kind, size)`.
pub(crate) fn literal_action(data: &Value, name: &str, kind: i64, size: i64) -> Result<Lit> {
    let names = kl(data, "paramName")?;
    let index = names
        .iter()
        .position(|n| n.str().as_deref() == Some(name))
        .ok_or_else(|| format!("'{name}' is not in list"))?;
    let ints = |key: &str| -> Result<Vec<i64>> {
        Ok(kl(data, key)?
            .iter()
            .map(|v| v.int().unwrap_or(0))
            .collect())
    };
    if ints("paramDataType")?[index] != kind || ints("paramByteDataSize")?[index] != size {
        return err(format!("unsupported serialized action parameter {name}"));
    }
    let start = ints("paramDataPos")?[index];
    let raw: Vec<u8> = ints("byteData")?.into_iter().map(|b| b as u8).collect();
    if start < 0 || start + size > raw.len() as i64 {
        return err("action parameter byte range");
    }
    let value = &raw[start as usize..(start + size) as usize];
    if (kind == 15 || kind == 17) && value[value.len() - 1] != 0 {
        return err(format!("variable action parameter {name}"));
    }
    if kind == 15 {
        let n = f32::from_le_bytes(value[..4].try_into().unwrap()) as f64;
        if !n.is_finite() {
            return err("non-finite action float");
        }
        return Ok(Lit::F(n));
    }
    if kind == 17 {
        if value[0] > 1 {
            return err("non-boolean action literal");
        }
        return Ok(Lit::B(value[0] != 0));
    }
    if value.len() != 4 {
        return err(format!("unpack requires a buffer of 4 bytes"));
    }
    Ok(Lit::I(i32::from_le_bytes(value.try_into().unwrap()) as i64))
}

fn lit_f(l: Lit) -> f64 {
    match l {
        Lit::F(f) => f,
        Lit::B(b) => b as i64 as f64,
        Lit::I(i) => i as f64,
    }
}

fn lit_b(l: Lit) -> bool {
    match l {
        Lit::B(b) => b,
        Lit::F(f) => f != 0.0,
        Lit::I(i) => i != 0,
    }
}

fn lit_i(l: Lit) -> i64 {
    match l {
        Lit::I(i) => i,
        Lit::F(f) => f as i64,
        Lit::B(b) => b as i64,
    }
}

/// `mask_fades(sc, receiver_gid)`: strictly recognize an authored HIT ->
/// iTweenFadeTo mask controller. Returns (fades, errors).
pub fn mask_fades(sc: &Scene, receiver_gid: i64) -> Result<(Vec<Json>, Vec<Json>)> {
    let file = file_of(sc);
    let (mut fades, mut errors) = (Vec::new(), Vec::new());
    for (index, kind, tree) in components(sc, receiver_gid)? {
        if kind != "PlayMakerFSM" {
            continue;
        }
        let one = (|| -> Result<Json> {
            let fsm = k(tree, "fsm")?;
            let start = k(fsm, "startState")?.str();
            let states = kl(fsm, "states")?;
            // `next(...)` of an empty generator raises StopIteration, whose text is empty.
            let initial = states
                .iter()
                .find(|st| k(st, "name").ok().and_then(Value::str) == start)
                .ok_or_else(String::new)?;
            let transition = kl(initial, "transitions")?
                .iter()
                .find(|t| {
                    k(t, "fsmEvent")
                        .and_then(|e| k(e, "name"))
                        .ok()
                        .and_then(Value::str)
                        .as_deref()
                        == Some("HIT")
                })
                .ok_or_else(String::new)?;
            let to = k(transition, "toState")?.str();
            let state = states
                .iter()
                .find(|st| k(st, "name").ok().and_then(Value::str) == to)
                .ok_or_else(String::new)?;
            let data = k(state, "actionData")?;
            let names = kl(data, "actionNames")?;
            let enabled = kl(data, "actionEnabled")?;
            if names.len() != 1
                || names[0].str().as_deref() != Some("HutongGames.PlayMaker.Actions.iTweenFadeTo")
                || enabled.len() != 1
                || enabled[0].int() != Some(1)
            {
                return err("HIT destination is not one enabled iTweenFadeTo action");
            }
            let owner = kl(data, "fsmOwnerDefaultParams")?;
            if owner.len() != 1 || ki(&owner[0], "ownerOption")? != 0 {
                return err("iTweenFadeTo target is not owner");
            }
            let alpha = lit_f(literal_action(data, "alpha", 15, 5)?);
            let duration = lit_f(literal_action(data, "time", 15, 5)?);
            let delay = lit_f(literal_action(data, "delay", 15, 5)?);
            let children = lit_b(literal_action(data, "includeChildren", 17, 2)?);
            let ease = lit_i(literal_action(data, "easeType", 7, 4)?);
            let looped = lit_i(literal_action(data, "loopType", 7, 4)?);
            let realtime = lit_b(literal_action(data, "realTime", 17, 2)?);
            if !(0.0..=1.0).contains(&alpha)
                || !(duration > 0.0 && duration <= 10.0)
                || delay != 0.0
                || ease != 21
                || looped != 0
                || realtime
            {
                return err("unsupported iTweenFadeTo timing/ease/loop");
            }
            let gids = if children {
                descendants(sc, receiver_gid)?
            } else {
                vec![receiver_gid]
            };
            let mut renderers: Vec<(String, i64, f64)> = Vec::new();
            for gid in gids {
                for (rid, rtype, renderer) in components(sc, gid)? {
                    if rtype == "SpriteRenderer" && k(renderer, "m_Enabled")?.truthy() {
                        renderers.push((
                            format!("{file}:{rid}"),
                            rid,
                            kf(k(renderer, "m_Color")?, "a")?,
                        ));
                    }
                }
            }
            if renderers.is_empty() {
                return err("mask controller has no sprite renderers");
            }
            Ok(jobj(vec![
                ("source_fsm", Json::Str(format!("{file}:{index}"))),
                ("fsm_name", value_json(k(fsm, "name")?)),
                ("from_state", value_json(k(initial, "name")?)),
                ("to_state", value_json(k(state, "name")?)),
                ("event", js("HIT")),
                (
                    "renderers",
                    jl(renderers
                        .iter()
                        .map(|(s, id, a)| {
                            jobj(vec![
                                ("source", js(s)),
                                ("id", ji(*id)),
                                ("initial_alpha", jf(*a)),
                            ])
                        })
                        .collect()),
                ),
                (
                    "renderer_ids",
                    jl(renderers.iter().map(|r| ji(r.1)).collect()),
                ),
                (
                    "renderer_sources",
                    jl(renderers.iter().map(|r| js(&r.0)).collect()),
                ),
                ("target_alpha", jf(alpha)),
                ("seconds", jf(duration)),
                ("ticks_60hz", ji((duration * 60.0 - 1e-5).ceil() as i64)),
                ("include_children", jb(children)),
                ("ease", js("linear")),
                ("ease_enum", ji(ease)),
            ]))
        })();
        match one {
            Ok(j) => fades.push(j),
            Err(e) => errors.push(jobj(vec![
                ("source", Json::Str(format!("{file}:{index}"))),
                ("type", js("forwarded Breakable event handler")),
                ("error", Json::Str(e)),
            ])),
        }
    }
    Ok((fades, errors))
}

/// The pieces of a Breakable record `destruction_contract` reads.
pub struct ContractInput<'a> {
    pub has_visual: bool,
    pub clips: Vec<String>,
    pub debris: Vec<String>,
    pub hit_receiver: bool,
    pub mask_fades: bool,
    pub event_errors: Vec<&'a str>,
}

fn missing(output: &str, part: Option<&str>, reason: &str) -> Json {
    let mut f = vec![("output", js(output))];
    if let Some(p) = part {
        f.push(("part", js(p)));
    }
    f.push(("reason", js(reason)));
    jobj(f)
}

/// `destruction_contract(sc, record, gravity)`.
pub fn destruction_contract(sc: &Scene, record: &ContractInput, gravity: f64) -> Result<Json> {
    let (mut authored, mut missing_list): (Vec<&str>, Vec<Json>) = (Vec::new(), Vec::new());
    if record.has_visual {
        authored.push(OUTPUT_VISUAL);
    }
    authored.push(OUTPUT_COLLIDER);
    if !record.clips.is_empty() {
        authored.push(OUTPUT_AUDIO);
        let absent: BTreeSet<&String> = record
            .clips
            .iter()
            .filter(|n| !RESIDENT_BREAK_CLIPS.contains(&n.as_str()))
            .collect();
        if !absent.is_empty() {
            missing_list.push(missing(
                OUTPUT_AUDIO,
                None,
                &format!(
                    "break clip not resident: {}",
                    absent
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ));
        }
    }
    let (mut emitters, mut fragments): (Vec<Json>, Vec<Json>) = (Vec::new(), Vec::new());
    for part in &record.debris {
        let part_gid: i64 = part
            .split(':')
            .nth(1)
            .and_then(|v| v.parse().ok())
            .ok_or("bad debris part")?;
        match part_emitter(&sc.source, sc, part_gid, gravity, false, false, None, false) {
            Err(error) => {
                authored.push(OUTPUT_PARTICLES);
                missing_list.push(missing(OUTPUT_PARTICLES, Some(part), &error));
                continue;
            }
            Ok(Found::None) => {}
            Ok(_) => {
                authored.push(OUTPUT_PARTICLES);
                emitters.push(js(part));
                continue;
            }
        }
        match rigid_fragment(sc, part_gid) {
            Err(error) => {
                authored.push(OUTPUT_FRAGMENTS);
                missing_list.push(missing(OUTPUT_FRAGMENTS, Some(part), &error));
            }
            Ok(Some(rigid)) => {
                authored.push(OUTPUT_FRAGMENTS);
                fragments.push(rigid);
            }
            Ok(None) => {
                missing_list.push(missing(
                    OUTPUT_PARTICLES,
                    Some(part),
                    "debris part is neither a particle system nor a rigid fling",
                ));
                authored.push(OUTPUT_PARTICLES);
            }
        }
    }
    if record.hit_receiver {
        authored.push(OUTPUT_MASK);
        if !record.mask_fades {
            let reasons = record.event_errors.join("; ");
            missing_list.push(missing(
                OUTPUT_MASK,
                None,
                if reasons.is_empty() {
                    "no recognized handler"
                } else {
                    &reasons
                },
            ));
        }
    }
    let authored: Vec<&str> = OUTPUTS
        .iter()
        .copied()
        .filter(|n| authored.contains(n))
        .collect();
    let refused = refused_outputs(&missing_list, &ENFORCED_OUTPUTS);
    Ok(jobj(vec![
        ("authored", jstrs(&authored)),
        ("missing", jl(missing_list)),
        ("refused_outputs", refused),
        ("particle_parts", jl(emitters)),
        ("rigid_fragments", jl(fragments)),
    ]))
}

/// `sorted({item['output'] for item in missing} & set(enforced))`.
pub(crate) fn refused_outputs(missing: &[Json], enforced: &[&str]) -> Json {
    let found: BTreeSet<String> = missing
        .iter()
        .filter_map(|m| jget(m, "output").map(jstr))
        .filter(|o| enforced.contains(&o.as_str()))
        .collect();
    jl(found.into_iter().map(Json::Str).collect())
}

// ------------------------------------------------------------------ FSM readers

/// `_state_actions(fsm, name)`: (action, fields) for one state's enabled actions.
pub(crate) fn state_actions(fsm: &Value, name: &str) -> Result<Vec<(String, Fields)>> {
    let states: Vec<&Value> = kl(fsm, "states")?
        .iter()
        .filter(|s| k(s, "name").ok().and_then(Value::str).as_deref() == Some(name))
        .collect();
    if states.len() != 1 {
        return err(format!(
            "expected exactly one '{name}' state, found {}",
            states.len()
        ));
    }
    let data = k(states[0], "actionData")?;
    let names = kl(data, "actionNames")?;
    let enabled = kl(data, "actionEnabled")?;
    let mut out = Vec::new();
    for (index, raw) in names.iter().enumerate() {
        if !enabled.get(index).is_some_and(Value::truthy) {
            continue;
        }
        let raw = raw.str().unwrap_or_default();
        out.push((
            raw.rsplit('.').next().unwrap_or("").to_string(),
            u(action_fields(data, index, false))?,
        ));
    }
    Ok(out)
}

/// `_fsm_variables(fsm)`: serialized variables by name, object references left out.
pub(crate) fn fsm_variables(fsm: &Value) -> Result<Vec<(String, Value)>> {
    let mut out: Vec<(String, Value)> = Vec::new();
    if let Value::Map(groups) = k(fsm, "variables")? {
        for (_, group) in groups {
            let Some(items) = group.list() else { continue };
            for v in items {
                if !matches!(v, Value::Map(_)) {
                    continue;
                }
                let (Some(name), Some(value)) = (v.get("name"), v.get("value")) else {
                    continue;
                };
                if matches!(value, Value::Map(_)) && value.get("m_PathID").is_some() {
                    continue;
                }
                let name = name.str().unwrap_or_default();
                match out.iter_mut().find(|e| e.0 == name) {
                    Some(slot) => slot.1 = value.clone(),
                    None => out.push((name, value.clone())),
                }
            }
        }
    }
    Ok(out)
}

pub(crate) fn var<'a>(vars: &'a [(String, Value)], name: &str) -> Option<&'a Value> {
    vars.iter().find(|e| e.0 == name).map(|e| &e.1)
}

/// A field of an action's decoded fields.
pub(crate) fn fv<'a>(fields: &'a Fields, key: &str) -> Option<&'a Value> {
    fields.iter().find(|(n, _)| n == key).map(|(_, v)| v)
}

/// `value in (0, False)` / `(0, 1, False, True)` for a serialized scalar.
pub(crate) fn is_int_in(v: Option<&Value>, allowed: &[i64]) -> bool {
    match v {
        Some(Value::Bool(b)) => allowed.contains(&(*b as i64)),
        Some(Value::Int(i)) => allowed.contains(i),
        Some(Value::UInt(i)) => allowed.contains(&(*i as i64)),
        Some(Value::F32(f)) => allowed.iter().any(|a| *a as f64 == *f as f64),
        Some(Value::F64(f)) => allowed.iter().any(|a| *a as f64 == *f),
        _ => false,
    }
}

// ------------------------------------------------------------------ arena gates

const BG_CONTROL_FSM: &str = "BG Control";
const BG_CONTROL_SHA256: &str = "98a0b55f39c2f01fa2e16c353d581532179c4893a03437b10cbee49b19e8ca9f";
const BG_CONTROL_PLACEMENT_VARIABLE: &str = "Start Closed";
const BG_CONTROL_START_STATE: &str = "Opened";
const BG_CONTROL_START_ACTIONS: [&str; 4] =
    ["GetOwner", "BoolTest", "Tk2dPlayAnimation", "SetCollider"];
const BG_CONTROL_CLOSE_EVENT: &str = "BG QUICK CLOSE";
const BG_CONTROL_CLOSE_STATE: &str = "Quick Close";
const BG_CONTROL_OPEN_EVENT: &str = "BG OPEN";

fn sets_own_collider(action: &str, fields: &Fields, expected: bool) -> Result<()> {
    if action != "SetCollider" {
        return err(format!("expected SetCollider, found {action}"));
    }
    let target = fv(fields, "gameObject");
    let owner_ok = target
        .filter(|t| matches!(t, Value::Map(_)))
        .is_some_and(|t| t.get("ownerOption").and_then(Value::int) == Some(0));
    if !owner_ok {
        return err("SetCollider does not act on the gate itself");
    }
    let active = fv(fields, "active");
    let bad = match active {
        Some(a @ Value::Map(_)) => {
            a.get("useVariable").is_some_and(Value::truthy)
                || a.get("value").is_some_and(Value::truthy) != expected
        }
        _ => true,
    };
    if bad {
        return err(format!(
            "SetCollider no longer sets the gate collider to {}",
            if expected { "True" } else { "False" }
        ));
    }
    Ok(())
}

fn box_of(points: &[(f64, f64)]) -> [f64; 4] {
    use crate::common::{py_max, py_min};
    [
        py_min(points.iter().map(|p| p.0)),
        py_min(points.iter().map(|p| p.1)),
        py_max(points.iter().map(|p| p.0)),
        py_max(points.iter().map(|p| p.1)),
    ]
}

pub(crate) fn polygons_json(polygons: &[Vec<(f64, f64)>]) -> Json {
    jl(polygons
        .iter()
        .map(|poly| jl(poly.iter().map(|p| jfloats(&[p.0, p.1])).collect()))
        .collect())
}

/// `battle_gate(sc, gid, fsm)`.
fn battle_gate(sc: &Scene, gid: i64, fsm: &Value) -> Result<Json> {
    let start = ks(fsm, "startState")?;
    if start != BG_CONTROL_START_STATE {
        return err(format!("BG Control starts in '{start}'"));
    }
    let actions = state_actions(fsm, BG_CONTROL_START_STATE)?;
    if actions.iter().map(|a| a.0.as_str()).collect::<Vec<_>>() != BG_CONTROL_START_ACTIONS {
        return err("BG Control start state is not the authored action sequence");
    }
    let test = &actions[1].1;
    let variable = fv(test, "boolVariable");
    let reads_placement = variable
        .filter(|v| matches!(v, Value::Map(_)))
        .is_some_and(|v| {
            v.get("useVariable").is_some_and(Value::truthy)
                && v.get("name").and_then(Value::str).as_deref()
                    == Some(BG_CONTROL_PLACEMENT_VARIABLE)
        });
    if !reads_placement {
        return err("BG Control start test does not read the placement variable");
    }
    if fv(test, "isTrue").and_then(Value::str).as_deref() != Some(BG_CONTROL_CLOSE_EVENT)
        || fv(test, "isFalse").is_some_and(Value::truthy)
        || fv(test, "everyFrame").is_some_and(Value::truthy)
    {
        return err("BG Control start test no longer only closes the gate");
    }
    sets_own_collider(&actions[3].0, &actions[3].1, false)?;
    let closing = state_actions(fsm, BG_CONTROL_CLOSE_STATE)?;
    if closing.is_empty() {
        return err("BG Control quick close does nothing");
    }
    sets_own_collider(&closing[0].0, &closing[0].1, true)?;
    let waits = kl(fsm, "states")?
        .iter()
        .filter(|s| ks(s, "name").is_ok_and(|n| n == BG_CONTROL_CLOSE_STATE))
        .any(|s| {
            kl(s, "transitions").is_ok_and(|ts| {
                ts.iter().any(|t| {
                    k(t, "fsmEvent")
                        .and_then(|e| ks(e, "name"))
                        .is_ok_and(|n| n == BG_CONTROL_OPEN_EVENT)
                })
            })
        });
    if !waits {
        return err("BG Control quick close no longer waits for the arena to open it");
    }
    let vars = fsm_variables(fsm)?;
    let start_closed = var(&vars, BG_CONTROL_PLACEMENT_VARIABLE);
    if !is_int_in(start_closed, &[0, 1]) {
        return err(format!(
            "BG Control '{BG_CONTROL_PLACEMENT_VARIABLE}' is not a serialized boolean"
        ));
    }
    let colliders: Vec<Comp> = components(sc, gid)?
        .into_iter()
        .filter(|c| c.1.ends_with("Collider2D"))
        .collect();
    if colliders.len() != 1 {
        return err(format!(
            "arena gate carries {} colliders, not one",
            colliders.len()
        ));
    }
    let (body_id, collider_type, body) = colliders[0];
    if collider_type != "BoxCollider2D" {
        return err(format!("arena gate collider is a {collider_type}"));
    }
    if !k(body, "m_Enabled")?.truthy() || k(body, "m_IsTrigger")?.truthy() {
        return err("arena gate collider is not a serialized solid");
    }
    let layer = ki(sc.go(gid).ok_or("'gid'")?, "m_Layer")?;
    if layer != TERRAIN_LAYER {
        return err(format!("arena gate is on layer {layer}, not terrain"));
    }
    let polygons = collider_polygons(sc, gid, collider_type, body)?;
    let points: Vec<(f64, f64)> = polygons.iter().flatten().copied().collect();
    let closed = start_closed.is_some_and(Value::truthy);
    Ok(jobj(vec![
        ("gid", ji(gid)),
        ("position", jfloats(&point(sc, gid)?)),
        ("start_closed", jb(closed)),
        ("solid_on_load", jb(closed)),
        ("collider_source", Json::Str(sc.sid(body_id))),
        ("box", jfloats(&box_of(&points))),
        ("opens_on", js(BG_CONTROL_OPEN_EVENT)),
        ("opens_in_port", jb(false)),
        (
            "limitations",
            jstrs(&[
                "Gate animation, dust and slam audio are not reproduced; only the collider state the room loads with is answered",
                "A gate that starts closed stays closed: no Battle Scene arena runs",
            ]),
        ),
    ]))
}

/// `battle_gates(sc, errors)`: every active arena gate in the scene.
pub fn battle_gates(sc: &Scene, mut errors: Option<&mut Vec<Json>>) -> Result<Vec<Json>> {
    let file = file_of(sc);
    let mut result = Vec::new();
    let mut ids: Vec<&hk_unity::scene::SceneObject> = sc.objects.iter().collect();
    ids.sort_by_key(|o| o.id);
    for o in ids {
        if o.typename != "PlayMakerFSM" || ks(k(&o.tree, "fsm")?, "name")? != BG_CONTROL_FSM {
            continue;
        }
        let gid = local_id(k(&o.tree, "m_GameObject")?)?;
        if sc.go(gid).is_none() || !sc.active(gid) {
            continue;
        }
        let one = (|| -> Result<Json> {
            let fsm = k(&o.tree, "fsm")?;
            let digest = fsm_digest(fsm, &[BG_CONTROL_PLACEMENT_VARIABLE])?;
            if digest != BG_CONTROL_SHA256 {
                return err(format!("unverified BG Control variant: {digest}"));
            }
            let mut record = battle_gate(sc, gid, fsm)?;
            jset(&mut record, "source", Json::Str(format!("{file}:{}", o.id)));
            jset(
                &mut record,
                "game_object",
                Json::Str(format!("{file}:{gid}")),
            );
            jset(
                &mut record,
                "name",
                value_json(k(sc.go(gid).unwrap(), "m_Name")?),
            );
            jset(&mut record, "fsm_sha256", Json::Str(digest));
            Ok(record)
        })();
        match (one, errors.as_deref_mut()) {
            (Ok(r), _) => result.push(r),
            (Err(e), None) => return Err(e),
            (Err(e), Some(list)) => list.push(jobj(vec![
                ("id", Json::Str(format!("{file}:{}", o.id))),
                ("type", js(BG_CONTROL_FSM)),
                ("error", Json::Str(e)),
            ])),
        }
    }
    Ok(result)
}

// -------------------------------------------------------- FSM-authored families
//
// Everything below reads a family whose behaviour lives in a PlayMaker FSM
// rather than a C# component: selection is by FSM definition, never by object
// name, and a definition is pinned by `fsm_digest`.

/// Python's `repr` of a str.
pub(crate) fn pystr(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::new();
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// Python's `repr` of a serialized scalar (or None).
pub(crate) fn pyrepr(v: Option<&Value>) -> String {
    match v {
        None => "None".to_string(),
        Some(Value::Bool(b)) => (if *b { "True" } else { "False" }).to_string(),
        Some(Value::Int(i)) => i.to_string(),
        Some(Value::UInt(i)) => i.to_string(),
        Some(Value::F32(f)) => crate::pyfloat::repr(*f as f64),
        Some(Value::F64(f)) => crate::pyfloat::repr(*f),
        Some(Value::Str(s)) => pystr(&String::from_utf8_lossy(s)),
        Some(_) => String::new(),
    }
}

/// `definition_name(fsm)`: an FSM definition's name with an editor copy suffix removed.
pub fn definition_name(fsm: &Value) -> String {
    let name = fsm.get("name").and_then(Value::str).unwrap_or_default();
    let t = name.trim();
    if let Some(inner) = t.strip_suffix(')') {
        if let Some(open) = inner.rfind('(') {
            let digits = &inner[open + 1..];
            if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
                return inner[..open].trim_end().to_string();
            }
        }
    }
    t.to_string()
}

fn state_names(fsm: &Value) -> Result<BTreeSet<String>> {
    Ok(kl(fsm, "states")?
        .iter()
        .filter_map(|s| s.get("name").and_then(Value::str))
        .collect())
}

fn string_variable(fsm: &Value, name: &str) -> Result<String> {
    let vars = fsm_variables(fsm)?;
    match var(&vars, name) {
        None => Ok(String::new()),
        Some(v @ Value::Str(_)) => Ok(v.str().unwrap_or_default()),
        Some(_) => err(format!("{} is not a serialized string", pystr(name))),
    }
}

fn named_object(sc: &Scene, name: &str) -> Result<i64> {
    let found: Vec<i64> = sc
        .objects
        .iter()
        .filter(|o| sc.gos.contains_key(&o.id))
        .filter(|o| o.tree.get("m_Name").and_then(Value::str).as_deref() == Some(name))
        .filter(|o| sc.active(o.id))
        .map(|o| o.id)
        .collect();
    if found.len() != 1 {
        return err(format!(
            "{} names {} active scene objects, not one",
            pystr(name),
            found.len()
        ));
    }
    Ok(found[0])
}

/// One `SendEventByName`.
struct Send {
    event: String,
    to_children: bool,
}

fn send_events(fsm: &Value, state_name: &str) -> Result<Vec<Send>> {
    let mut out = Vec::new();
    for state in kl(fsm, "states")? {
        if ks(state, "name")? != state_name {
            continue;
        }
        let data = k(state, "actionData")?;
        let names = kl(data, "actionNames")?;
        let enabled = kl(data, "actionEnabled")?;
        for (index, raw) in names.iter().enumerate() {
            if !enabled.get(index).is_some_and(Value::truthy)
                || !raw.str().unwrap_or_default().ends_with(".SendEventByName")
            {
                continue;
            }
            let fields = u(action_fields(data, index, false))?;
            let target = fv(&fields, "eventTarget").ok_or("'eventTarget'")?;
            let event = fv(&fields, "sendEvent").ok_or("'sendEvent'")?;
            out.push(Send {
                event: ks(event, "value")?,
                to_children: k(k(target, "sendToChildren")?, "value")?.truthy(),
            });
        }
    }
    Ok(out)
}

const HIDDEN_WALL_SPELL_ATTACK_TYPE: i64 = 2;
const HIDDEN_WALL_UNCOVER: &str = "UNCOVER";
const HIDDEN_WALL_PLACEMENT_VARIABLES: [&str; 3] = ["Facing", "Mask Name", "CamLock Name"];

/// A pinned hidden-wall or cracked-floor shape.
pub struct Shape {
    pub sha256: String,
    pub definition: &'static str,
    start: &'static str,
    /// Hidden walls: how the break reaches the mask; floors: the hit gate.
    pub kind: &'static str,
    pub renderer: &'static str,
    states: &'static [&'static str],
    clear_booleans: &'static [&'static str],
    empty_strings: &'static [&'static str],
}

const WALL_V2_STATES: [&str; 23] = [
    "Idle",
    "Check If Nail",
    "Hit",
    "Initiate",
    "Check Direction",
    "Break",
    "Hit Right",
    "Return Right",
    "Hit Left",
    "Return Left",
    "Hit Down",
    "Return Down",
    "Hit Up",
    "Pause Frame",
    "Destroy",
    "Pause",
    "Activated",
    "Activated?",
    "Ruin Lift?",
    "Deactivate",
    "Get Refs",
    "PD Bool?",
    "Spell Destroy",
];
const WALL_TK2D_STATES: [&str; 19] = [
    "Idle",
    "Check If Nail",
    "Hit",
    "Initiate",
    "Check Direction",
    "Break",
    "Hit Right",
    "Return Right",
    "Hit Left",
    "Return Left",
    "Hit Down",
    "Return Down",
    "Hit Up",
    "Pause Frame",
    "Damage",
    "Destroy",
    "Pause",
    "Activated",
    "Spell Destroy",
];

fn hidden_wall_shape_for(digest: &str) -> Option<Shape> {
    match digest {
        "5ed4602f92093bd200971a86dec0e40c9af85ca39e4bf40924b864b5ebfb47ea" => Some(Shape {
            sha256: digest.to_string(),
            definition: "breakable_wall_v2",
            start: "Get Refs",
            kind: "children",
            renderer: "SpriteRenderer",
            states: &WALL_V2_STATES,
            clear_booleans: &["Ruin Lift"],
            empty_strings: &["PlayerData Bool"],
        }),
        "e316a8d60e3e4f8269f5d3ac97cfbda4e2cb7715495a24778123019e3d534283" => Some(Shape {
            sha256: digest.to_string(),
            definition: "FSM",
            start: "Pause",
            kind: "name",
            renderer: "tk2dSprite",
            states: &WALL_TK2D_STATES,
            clear_booleans: &[],
            empty_strings: &["CamLock Name"],
        }),
        _ => None,
    }
}

/// `hidden_wall_shape(fsm)`: the pinned hidden-wall shape this FSM is, or None.
pub fn hidden_wall_shape(fsm: &Value) -> Result<Option<Shape>> {
    let names = state_names(fsm)?;
    let matches =
        |states: &[&str]| names.len() == states.len() && states.iter().all(|s| names.contains(*s));
    if !matches(&WALL_V2_STATES) && !matches(&WALL_TK2D_STATES) {
        return Ok(None);
    }
    let digest = fsm_digest(fsm, &HIDDEN_WALL_PLACEMENT_VARIABLES)?;
    let Some(shape) = hidden_wall_shape_for(&digest) else {
        return err(format!("unverified hidden wall variant: {digest}"));
    };
    if definition_name(fsm) != shape.definition {
        return err(format!(
            "hidden wall digest {digest} under definition {}",
            pystr(&definition_name(fsm))
        ));
    }
    let start = ks(fsm, "startState")?;
    if start != shape.start {
        return err(format!("hidden wall starts in {}", pystr(&start)));
    }
    let _ = shape.states;
    Ok(Some(shape))
}

/// `_uncover_targets(sc, gid, fsm, shape)`: GameObject ids that receive the wall's UNCOVER.
fn uncover_targets(sc: &Scene, gid: i64, fsm: &Value, shape: &Shape) -> Result<Vec<i64>> {
    let mut sends = Vec::new();
    for state in ["Break", "Activated"] {
        for send in send_events(fsm, state)? {
            if send.event == HIDDEN_WALL_UNCOVER {
                sends.push(send);
            }
        }
    }
    if sends.is_empty() {
        return err("hidden wall no longer broadcasts UNCOVER");
    }
    if shape.kind == "children" {
        if !sends.iter().all(|s| s.to_children) {
            return err("hidden wall UNCOVER no longer reaches its children");
        }
        let mut out: Vec<i64> = descendant_set(sc, gid)?
            .into_iter()
            .filter(|g| *g != gid && sc.active(*g))
            .collect();
        out.sort_unstable();
        return Ok(out);
    }
    if sends.iter().any(|s| s.to_children) {
        return err("named-target hidden wall UNCOVER also broadcasts to children");
    }
    let name = string_variable(fsm, "Mask Name")?;
    if name.is_empty() {
        return err("hidden wall names no mask to uncover");
    }
    Ok(vec![named_object(sc, &name)?])
}

/// `uncover_drivers(sc)`: scene objects a hidden wall uncovers, keyed by the
/// object that receives UNCOVER.
pub fn uncover_drivers(sc: &Scene) -> Result<Vec<(i64, Json)>> {
    let mut drivers: Vec<(i64, Json)> = Vec::new();
    let mut objects: Vec<&hk_unity::scene::SceneObject> = sc.objects.iter().collect();
    objects.sort_by_key(|o| o.id);
    for o in objects {
        if o.typename != "PlayMakerFSM" || !k(&o.tree, "m_Enabled")?.truthy() {
            continue;
        }
        let gid = local_id(k(&o.tree, "m_GameObject")?)?;
        if sc.go(gid).is_none() || !sc.active(gid) {
            continue;
        }
        let fsm = k(&o.tree, "fsm")?;
        let Ok(Some(shape)) = hidden_wall_shape(fsm) else {
            continue;
        };
        let Ok(targets) = uncover_targets(sc, gid, fsm, &shape) else {
            continue;
        };
        let driver = jobj(vec![
            ("source", Json::Str(sc.sid(o.id))),
            ("game_object", Json::Str(sc.sid(gid))),
            ("name", value_json(k(sc.go(gid).unwrap(), "m_Name")?)),
            ("definition", js(shape.definition)),
            ("fsm_sha256", Json::Str(shape.sha256.clone())),
            ("uncover", js(shape.kind)),
        ]);
        for target in targets {
            if !drivers.iter().any(|d| d.0 == target) {
                drivers.push((target, driver.clone()));
            }
        }
    }
    Ok(drivers)
}

/// `_particle_outputs(sc, gids, gravity, authored, missing, played)`.
fn particle_outputs(
    sc: &Scene,
    gids: &[i64],
    gravity: f64,
    authored: &mut Vec<&'static str>,
    missing: &mut Vec<Json>,
    played: &BTreeSet<i64>,
) {
    for &gid in gids {
        match part_emitter(
            &sc.source,
            sc,
            gid,
            gravity,
            played.contains(&gid),
            false,
            None,
            false,
        ) {
            Err(error) => {
                authored.push(OUTPUT_PARTICLES);
                missing.push(self::missing(OUTPUT_PARTICLES, Some(&sc.sid(gid)), &error));
            }
            Ok(Found::None) => {}
            Ok(_) => authored.push(OUTPUT_PARTICLES),
        }
    }
}

/// `_action_slots(data, index)`: parameter name to flat-run index for one action's own slice.
fn action_slots(data: &Value, index: usize) -> Result<Vec<(String, usize)>> {
    let names = kl(data, "paramName")?;
    let starts = kl(data, "actionStartIndex")?;
    let start = starts
        .get(index)
        .and_then(Value::int)
        .ok_or("list index out of range")? as usize;
    let end = if index + 1 < kl(data, "actionNames")?.len() {
        starts
            .get(index + 1)
            .and_then(Value::int)
            .ok_or("list index out of range")? as usize
    } else {
        names.len()
    };
    let mut out: Vec<(String, usize)> = Vec::new();
    for i in start..end {
        let name = names.get(i).and_then(Value::str).unwrap_or_default();
        match out.iter_mut().find(|e| e.0 == name) {
            Some(slot) => slot.1 = i,
            None => out.push((name, i)),
        }
    }
    Ok(out)
}

fn slot_of(slots: &[(String, usize)], name: &str) -> Option<usize> {
    slots.iter().find(|e| e.0 == name).map(|e| e.1)
}

fn int_at(data: &Value, key: &str, i: usize) -> Result<i64> {
    kl(data, key)?
        .get(i)
        .and_then(Value::int)
        .ok_or_else(|| "list index out of range".to_string())
}

fn list_at(data: &Value, key: &str, pos: i64) -> Result<Value> {
    let list = kl(data, key)?;
    let i = if pos < 0 {
        list.len() as i64 + pos
    } else {
        pos
    };
    list.get(i as usize)
        .cloned()
        .ok_or_else(|| "list index out of range".to_string())
}

/// `_child_bindings(sc, gid, fsm)`: object variable to scene object, for every literal FindChild.
fn child_bindings(sc: &Scene, gid: i64, fsm: &Value) -> Result<Vec<(String, Option<i64>)>> {
    let owner = owner_variable(fsm)?;
    let tid = *sc.go_transform.get(&gid).ok_or("'gid'")?;
    let mut children: Vec<(String, Vec<i64>)> = Vec::new();
    for child in kl(sc.transform(tid).ok_or("'tid'")?, "m_Children")? {
        let ct = sc
            .transform(ki(child, "m_PathID")?)
            .ok_or("child transform")?;
        let found = ki(k(ct, "m_GameObject")?, "m_PathID")?;
        let name = ks(sc.go(found).ok_or("child object")?, "m_Name")?;
        match children.iter_mut().find(|e| e.0 == name) {
            Some(slot) => slot.1.push(found),
            None => children.push((name, vec![found])),
        }
    }
    let mut bound: Vec<(String, Option<i64>)> = Vec::new();
    for state in kl(fsm, "states")? {
        let data = k(state, "actionData")?;
        let names = kl(data, "actionNames")?;
        let enabled = kl(data, "actionEnabled")?;
        for (index, raw) in names.iter().enumerate() {
            if !enabled.get(index).is_some_and(Value::truthy)
                || !raw.str().unwrap_or_default().ends_with(".FindChild")
            {
                continue;
            }
            let fields = u(action_fields(data, index, false))?;
            let target = fv(&fields, "gameObject").ok_or("'gameObject'")?;
            if k(target, "ownerOption")?.truthy()
                && k(target, "gameObject")?.get("name").and_then(Value::str) != owner
            {
                continue;
            }
            let child_name = fv(&fields, "childName").ok_or("'childName'")?;
            if k(child_name, "useVariable")?.truthy() {
                continue;
            }
            let slots = action_slots(data, index)?;
            let Some(slot) = slot_of(&slots, "storeResult") else {
                continue;
            };
            if int_at(data, "paramDataType", slot)? != 19 {
                continue;
            }
            let stored = list_at(
                data,
                "fsmGameObjectParams",
                int_at(data, "paramDataPos", slot)?,
            )?;
            if !stored.get("useVariable").is_some_and(Value::truthy)
                || !stored.get("name").is_some_and(Value::truthy)
            {
                continue;
            }
            let wanted = ks(child_name, "value")?;
            let found = children
                .iter()
                .find(|e| e.0 == wanted)
                .map(|e| e.1.clone())
                .unwrap_or_default();
            let value = if found.len() == 1 {
                Some(found[0])
            } else {
                None
            };
            let key = ks(&stored, "name")?;
            match bound.iter_mut().find(|e| e.0 == key) {
                Some(slot) => slot.1 = value,
                None => bound.push((key, value)),
            }
        }
    }
    Ok(bound)
}

/// `_played_emitters(sc, gid, fsm)`: scene objects an enabled PlayParticleEmitter starts.
fn played_emitters(sc: &Scene, gid: i64, fsm: &Value) -> Result<BTreeSet<i64>> {
    let bound = child_bindings(sc, gid, fsm)?;
    let mut played = BTreeSet::new();
    for state in kl(fsm, "states")? {
        let data = k(state, "actionData")?;
        let names = kl(data, "actionNames")?;
        let enabled = kl(data, "actionEnabled")?;
        for (index, raw) in names.iter().enumerate() {
            if !enabled.get(index).is_some_and(Value::truthy)
                || !raw
                    .str()
                    .unwrap_or_default()
                    .ends_with(".PlayParticleEmitter")
            {
                continue;
            }
            let fields = u(action_fields(data, index, false))?;
            let target = fv(&fields, "gameObject").ok_or("'gameObject'")?;
            if !k(target, "ownerOption")?.truthy() {
                played.insert(gid);
                continue;
            }
            let name = k(target, "gameObject")?.get("name").and_then(Value::str);
            if let Some(found) = bound
                .iter()
                .find(|e| Some(&e.0) == name.as_ref())
                .and_then(|e| e.1)
            {
                played.insert(found);
            }
        }
    }
    Ok(played)
}

/// A CreateObject prefab: its place and whether it carries a ParticleSystem.
struct Prefab {
    source: String,
    name: String,
    particles: bool,
    reference: Value,
    origin: [f64; 3],
    rotation: [f64; 3],
}

/// `_prefab_particles(source, file, ref)`.
fn prefab_particles(sc: &Scene, reference: &Value) -> Result<Option<(String, String, bool)>> {
    if ki(reference, "m_PathID")? == 0 {
        return Ok(None);
    }
    let prefab = u(sc.source.deref(&sc.base, reference))?;
    let tree = u(sc.source.read(&prefab))?;
    if tree.get("m_Component").is_none() {
        return err("CreateObject target is not a GameObject");
    }
    let mut particles = false;
    for component in kl(&tree, "m_Component")? {
        if let Ok(obj) = sc.source.deref(&prefab.file, k(component, "component")?) {
            // ParticleSystem's class id.
            if obj.class_id() == 198 {
                particles = true;
            }
        }
    }
    Ok(Some((prefab.sid(), ks(&tree, "m_Name")?, particles)))
}

/// `_action_parameters(...)`: one named typed parameter of every enabled `action` in one state.
fn action_parameters(
    fsm: &Value,
    state_name: &str,
    action: &str,
    parameter: &str,
    kind: i64,
    table: &str,
) -> Result<Vec<Value>> {
    let mut out = Vec::new();
    for state in kl(fsm, "states")? {
        if ks(state, "name")? != state_name {
            continue;
        }
        let data = k(state, "actionData")?;
        let names = kl(data, "actionNames")?;
        let enabled = kl(data, "actionEnabled")?;
        for (index, raw) in names.iter().enumerate() {
            if !enabled.get(index).is_some_and(Value::truthy)
                || !raw
                    .str()
                    .unwrap_or_default()
                    .ends_with(&format!(".{action}"))
            {
                continue;
            }
            let starts = kl(data, "actionStartIndex")?;
            let start = starts
                .get(index)
                .and_then(Value::int)
                .ok_or("list index out of range")? as usize;
            let end = if index + 1 < names.len() {
                starts
                    .get(index + 1)
                    .and_then(Value::int)
                    .ok_or("list index out of range")? as usize
            } else {
                kl(data, "paramName")?.len()
            };
            for i in start..end {
                let pname = kl(data, "paramName")?.get(i).and_then(Value::str);
                if pname.as_deref() == Some(parameter) && int_at(data, "paramDataType", i)? == kind
                {
                    out.push(list_at(data, table, int_at(data, "paramDataPos", i)?)?);
                }
            }
        }
    }
    Ok(out)
}

const FSM_VECTOR3: i64 = 28;
const FSM_VECTOR3_BYTES: i64 = 13;

/// `_vector_parameter(data, slot)`: one serialized FsmVector3, or None when unset.
fn vector_parameter(data: &Value, slot: usize) -> Result<Option<[f64; 3]>> {
    if int_at(data, "paramDataType", slot)? != FSM_VECTOR3
        || int_at(data, "paramByteDataSize", slot)? != FSM_VECTOR3_BYTES
    {
        return err("unsupported serialized vector parameter");
    }
    let raw: Vec<u8> = kl(data, "byteData")?
        .iter()
        .map(|v| v.int().unwrap_or(0) as u8)
        .collect();
    let start = int_at(data, "paramDataPos", slot)?;
    if start < 0 || start + FSM_VECTOR3_BYTES > raw.len() as i64 {
        return err("action parameter byte range");
    }
    let start = start as usize;
    if raw[start + 12] != 0 {
        return Ok(None);
    }
    let mut v = [0.0; 3];
    for (i, o) in v.iter_mut().enumerate() {
        *o = f32::from_le_bytes(raw[start + i * 4..start + i * 4 + 4].try_into().unwrap()) as f64;
    }
    if !v.iter().all(|x| x.is_finite()) {
        return err("non-finite action vector");
    }
    Ok(Some(v))
}

/// `_owner_variable(fsm)`: the object variable GetOwner stores.
fn owner_variable(fsm: &Value) -> Result<Option<String>> {
    for state in kl(fsm, "states")? {
        let data = k(state, "actionData")?;
        let names = kl(data, "actionNames")?;
        let enabled = kl(data, "actionEnabled")?;
        for (index, raw) in names.iter().enumerate() {
            if !enabled.get(index).is_some_and(Value::truthy)
                || !raw.str().unwrap_or_default().ends_with(".GetOwner")
            {
                continue;
            }
            let slots = action_slots(data, index)?;
            let Some(slot) = slot_of(&slots, "storeGameObject") else {
                continue;
            };
            if int_at(data, "paramDataType", slot)? != 19 {
                continue;
            }
            let stored = list_at(
                data,
                "fsmGameObjectParams",
                int_at(data, "paramDataPos", slot)?,
            )?;
            if stored.get("useVariable").is_some_and(Value::truthy) {
                if let Some(name) = stored
                    .get("name")
                    .filter(|n| n.truthy())
                    .and_then(Value::str)
                {
                    return Ok(Some(name));
                }
            }
        }
    }
    Ok(None)
}

/// `_spawn_transform(...)`: where CreateObject puts the object it instantiates.
fn spawn_transform(
    sc: &Scene,
    gid: i64,
    fsm: &Value,
    data: &Value,
    slots: &[(String, usize)],
) -> Result<([f64; 3], [f64; 3])> {
    let slot = |n: &str| slot_of(slots, n).ok_or_else(|| format!("'{n}'"));
    let point_v = list_at(
        data,
        "fsmGameObjectParams",
        int_at(data, "paramDataPos", slot("spawnPoint")?)?,
    )?;
    let offset = vector_parameter(data, slot("position")?)?;
    let rotation = vector_parameter(data, slot("rotation")?)?;
    let pointer = point_v.get("value").ok_or("'value'")?;
    if ki(pointer, "m_PathID")? != 0 || !point_v.get("useVariable").is_some_and(Value::truthy) {
        return err("the spawn point is not the FSM object variable this port can resolve");
    }
    let owner = owner_variable(fsm)?;
    let name = point_v.get("name").and_then(Value::str);
    if owner.is_none() || name != owner {
        return err(format!(
            "the spawn point {} is not the object GetOwner stores",
            match &name {
                Some(n) => pystr(n),
                None => "None".to_string(),
            }
        ));
    }
    let mut origin = point(sc, gid)?;
    if let Some(o) = offset {
        for i in 0..3 {
            origin[i] += o[i];
        }
    }
    let Some(rotation) = rotation else {
        return err("the spawn takes its rotation from the spawn point, which is not read");
    };
    Ok((origin, rotation))
}

/// `_create_object_prefabs(sc, gid, fsm, state_name)`.
fn create_object_prefabs(
    sc: &Scene,
    gid: i64,
    fsm: &Value,
    state_name: &str,
) -> Result<Vec<Prefab>> {
    let mut out = Vec::new();
    for state in kl(fsm, "states")? {
        if ks(state, "name")? != state_name {
            continue;
        }
        let data = k(state, "actionData")?;
        let names = kl(data, "actionNames")?;
        let enabled = kl(data, "actionEnabled")?;
        for (index, raw) in names.iter().enumerate() {
            if !enabled.get(index).is_some_and(Value::truthy)
                || !raw.str().unwrap_or_default().ends_with(".CreateObject")
            {
                continue;
            }
            let slots = action_slots(data, index)?;
            if !["gameObject", "spawnPoint", "position", "rotation"]
                .iter()
                .all(|n| slot_of(&slots, n).is_some())
            {
                return err("unsupported serialized CreateObject parameters");
            }
            let target = slot_of(&slots, "gameObject").unwrap();
            if int_at(data, "paramDataType", target)? != 19 {
                return err("unsupported serialized CreateObject target");
            }
            let reference = list_at(
                data,
                "fsmGameObjectParams",
                int_at(data, "paramDataPos", target)?,
            )?;
            if reference.get("useVariable").is_some_and(Value::truthy) {
                continue;
            }
            let pointer = reference.get("value").ok_or("'value'")?.clone();
            let Some((source, name, particles)) = prefab_particles(sc, &pointer)? else {
                continue;
            };
            if !particles {
                continue;
            }
            let (origin, rotation) = spawn_transform(sc, gid, fsm, data, &slots)?;
            out.push(Prefab {
                source,
                name,
                particles,
                reference: pointer,
                origin,
                rotation,
            });
        }
    }
    Ok(out)
}

/// `_prefab_particle_outputs(...)`.
fn prefab_particle_outputs(
    sc: &Scene,
    gid: i64,
    fsm: &Value,
    states: &[&str],
    gravity: f64,
    authored: &mut Vec<&'static str>,
    missing: &mut Vec<Json>,
) {
    for state in states {
        let prefabs = match create_object_prefabs(sc, gid, fsm, state) {
            Ok(p) => p,
            Err(error) => {
                authored.push(OUTPUT_PARTICLES);
                missing.push(self::missing(
                    OUTPUT_PARTICLES,
                    None,
                    &format!("the break spawns a prefab and {error}"),
                ));
                continue;
            }
        };
        for prefab in prefabs {
            authored.push(OUTPUT_PARTICLES);
            let _ = prefab.particles;
            if let Err(error) = prefab_emitters(
                &sc.source,
                &sc.base,
                sc,
                &prefab.reference,
                gravity,
                prefab.origin,
                Some(prefab.rotation),
                false,
            ) {
                missing.push(self::missing(
                    OUTPUT_PARTICLES,
                    Some(&prefab.source),
                    &format!(
                        "the break instantiates the {} particle prefab, and {error}",
                        pystr(&prefab.name)
                    ),
                ));
            }
        }
    }
}

/// `_break_clips(sc, fsm, state_name)`: AudioPlayerOneShotSingle clip names played by one state.
fn break_clips(sc: &Scene, fsm: &Value, state_name: &str) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for clip in action_parameters(
        fsm,
        state_name,
        "AudioPlayerOneShotSingle",
        "audioClip",
        24,
        "fsmObjectParams",
    )? {
        let reference = k(&clip, "value")?;
        if ki(reference, "m_PathID")? != 0 {
            let obj = u(sc.source.deref(&sc.base, reference))?;
            names.push(ks(&u(sc.source.read(&obj))?, "m_Name")?);
        }
    }
    Ok(names)
}

/// `_audio_output(clips, authored, missing)`: break audio, measured and never enforced.
fn audio_output(clips: &[String], authored: &mut Vec<&'static str>, missing: &mut Vec<Json>) {
    if clips.is_empty() {
        return;
    }
    authored.push(OUTPUT_AUDIO);
    let absent: BTreeSet<&String> = clips
        .iter()
        .filter(|n| !RESIDENT_BREAK_CLIPS.contains(&n.as_str()))
        .collect();
    if !absent.is_empty() {
        missing.push(self::missing(
            OUTPUT_AUDIO,
            None,
            &format!(
                "break clip not resident: {}",
                absent
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
    }
}

/// The `ParticleSystem`-carrying descendants of a secret, ascending.
fn emitters_below(sc: &Scene, gid: i64) -> Result<Vec<i64>> {
    let mut out = Vec::new();
    for g in descendant_set(sc, gid)? {
        if g == gid {
            continue;
        }
        if components(sc, g)?.iter().any(|c| c.1 == "ParticleSystem") {
            out.push(g);
        }
    }
    Ok(out)
}

/// `[min x, min y, max x, max y]` of the points of some polygons.
fn polygons_box(polygons: &[Vec<(f64, f64)>]) -> [f64; 4] {
    let points: Vec<(f64, f64)> = polygons.iter().flatten().copied().collect();
    box_of(&points)
}

/// `hidden_wall(sc, gid, index, fsm, shape, gravity, drawn_renderers)`: one hidden
/// wall's authored break, and which of it the port can produce.
pub fn hidden_wall(
    sc: &Scene,
    gid: i64,
    index: i64,
    fsm: &Value,
    shape: &Shape,
    gravity: f64,
    drawn_renderers: Option<&BTreeSet<String>>,
) -> Result<Json> {
    let vars = fsm_variables(fsm)?;
    for name in shape.clear_booleans {
        if !is_int_in(var(&vars, name), &[0]) {
            return err(format!(
                "{} is set, so the load path is not the plain one",
                pystr(name)
            ));
        }
    }
    for name in shape.empty_strings {
        if !string_variable(fsm, name)?.is_empty() {
            return err(format!(
                "{} names a save-backed lookup this port cannot answer",
                pystr(name)
            ));
        }
    }
    if !is_int_in(var(&vars, "Activated"), &[0]) {
        return err("hidden wall is serialized already broken");
    }
    if !is_int_in(var(&vars, "Facing"), &[0, 1, 2, 3]) {
        return err(format!(
            "hidden wall Facing {} is outside the authored switch",
            pyrepr(var(&vars, "Facing"))
        ));
    }
    let hits = var(&vars, "Hits");
    let hit_count = match hits {
        Some(Value::Int(i)) => Some(*i),
        Some(Value::UInt(i)) => i64::try_from(*i).ok(),
        _ => None,
    };
    let hits = match hit_count {
        Some(h) if (1..=MAX_HIT_POINTS as i64).contains(&h) => h,
        _ => {
            return err(format!(
                "hidden wall hit count {} is not a serialized 1..16 integer",
                pyrepr(hits)
            ))
        }
    };
    let colliders: Vec<Comp> = components(sc, gid)?
        .into_iter()
        .filter(|c| c.1.ends_with("Collider2D"))
        .collect();
    if colliders.len() != 1 {
        return err(format!(
            "hidden wall carries {} colliders, not one",
            colliders.len()
        ));
    }
    let (body_id, collider_type, body) = colliders[0];
    if collider_type != "BoxCollider2D" {
        return err(format!("hidden wall collider is a {collider_type}"));
    }
    if !k(body, "m_Enabled")?.truthy() || k(body, "m_IsTrigger")?.truthy() {
        return err("hidden wall collider is not a serialized solid");
    }
    let layer = ki(sc.go(gid).ok_or("'gid'")?, "m_Layer")?;
    if layer != TERRAIN_LAYER {
        return err(format!("hidden wall is on layer {layer}, not terrain"));
    }
    let polygons = collider_polygons(sc, gid, collider_type, body)?;

    let mut comps: HashMap<&str, i64> = HashMap::new();
    for (i, typ, _) in components(sc, gid)? {
        comps.insert(typ, i);
    }
    let mut authored: Vec<&'static str> = vec![OUTPUT_COLLIDER, OUTPUT_VISUAL];
    let mut missing_list: Vec<Json> = Vec::new();
    let Some(&renderer) = comps.get(shape.renderer) else {
        return err(format!("hidden wall has no {} to hide", shape.renderer));
    };
    let renderer_source = sc.sid(renderer);
    if shape.renderer != "SpriteRenderer" {
        missing_list.push(missing(
            OUTPUT_VISUAL,
            None,
            &format!(
                "the wall is drawn by a {}, which no cooked draw carries",
                shape.renderer
            ),
        ));
    } else if drawn_renderers.is_some_and(|d| !d.contains(&renderer_source)) {
        missing_list.push(missing(
            OUTPUT_VISUAL,
            None,
            "the wall renderer is not cooked into any draw",
        ));
    }
    let clips = break_clips(sc, fsm, "Break")?;
    audio_output(&clips, &mut authored, &mut missing_list);
    let emitters = emitters_below(sc, gid)?;
    particle_outputs(
        sc,
        &emitters,
        gravity,
        &mut authored,
        &mut missing_list,
        &played_emitters(sc, gid, fsm)?,
    );
    prefab_particle_outputs(
        sc,
        gid,
        fsm,
        &["Break"],
        gravity,
        &mut authored,
        &mut missing_list,
    );

    let mut uncovers = Vec::new();
    let reveals = crate::reveal_masks::reveal_mask_sources(sc)?;
    let admitted: BTreeSet<String> = reveals
        .controllers
        .iter()
        .filter_map(|c| jget(c, "game_object").map(jstr))
        .collect();
    let refused: Vec<(String, String)> = reveals
        .unsupported
        .iter()
        .map(|r| {
            (
                jget(r, "source").map(jstr).unwrap_or_default(),
                jget(r, "error").map(jstr).unwrap_or_default(),
            )
        })
        .collect();
    for target in uncover_targets(sc, gid, fsm, shape)? {
        for (controller, kind, tree) in components(sc, target)? {
            if kind != "PlayMakerFSM" {
                continue;
            }
            let source_id = sc.sid(controller);
            let mut entry = jobj(vec![
                ("source", Json::Str(source_id.clone())),
                ("game_object", Json::Str(sc.sid(target))),
                (
                    "name",
                    value_json(k(sc.go(target).ok_or("'target'")?, "m_Name")?),
                ),
                ("definition", Json::Str(definition_name(k(tree, "fsm")?))),
            ]);
            if admitted.contains(&sc.sid(target)) {
                jset(&mut entry, "reveal", js("admitted"));
            } else if let Some(r) = refused.iter().rev().find(|r| r.0 == source_id) {
                jset(&mut entry, "reveal", js("refused"));
                jset(&mut entry, "reveal_error", Json::Str(r.1.clone()));
            } else {
                jset(&mut entry, "reveal", js("not a reveal controller"));
            }
            uncovers.push(entry);
        }
    }
    if !uncovers.is_empty() {
        authored.push(OUTPUT_MASK);
        let unusable: Vec<&Json> = uncovers
            .iter()
            .filter(|e| jget(e, "reveal").map(jstr).as_deref() != Some("admitted"))
            .collect();
        if !unusable.is_empty() {
            let list: Vec<String> = unusable
                .iter()
                .map(|e| {
                    format!(
                        "{} ({})",
                        jget(e, "name").map(jstr).unwrap_or_default(),
                        jget(e, "reveal").map(jstr).unwrap_or_default()
                    )
                })
                .collect();
            missing_list.push(missing(
                OUTPUT_MASK,
                None,
                &format!("the break uncovers {}", list.join(", ")),
            ));
        }
    }
    let authored: Vec<&str> = OUTPUTS
        .iter()
        .copied()
        .filter(|n| authored.contains(n))
        .collect();
    let refused_out = refused_outputs(&missing_list, &ENFORCED_OUTPUTS);
    Ok(jobj(vec![
        ("gid", ji(gid)),
        ("source", Json::Str(sc.sid(index))),
        ("game_object", Json::Str(sc.sid(gid))),
        ("name", value_json(k(sc.go(gid).unwrap(), "m_Name")?)),
        ("definition", js(shape.definition)),
        ("fsm_sha256", Json::Str(shape.sha256.clone())),
        ("position", jfloats(&point(sc, gid)?)),
        ("nail_hits", ji(hits)),
        ("spell_attack_type", ji(HIDDEN_WALL_SPELL_ATTACK_TYPE)),
        ("facing", value_json(var(&vars, "Facing").unwrap())),
        ("collider_source", Json::Str(sc.sid(body_id))),
        ("hit_polygons", polygons_json(&polygons)),
        ("box", jfloats(&polygons_box(&polygons))),
        ("renderer_source", Json::Str(renderer_source)),
        ("renderer_type", js(shape.renderer)),
        ("uncovers", jl(uncovers)),
        ("break_clips", jstrs(&clips)),
        (
            "destruction",
            jobj(vec![
                ("authored", jstrs(&authored)),
                ("missing", jl(missing_list)),
                ("refused_outputs", refused_out),
            ]),
        ),
        (
            "limitations",
            jstrs(&[
                format!(
                    "{hits} nail hits or one spell; the port has no multi-hit break, so this count is recorded and not yet run"
                ),
                "Recoil, hit sparks, the camera shake the break sends to CameraShake and the PersistentBoolItem that keeps a wall broken across a reload are not reproduced".to_string(),
            ]),
        ),
    ]))
}

/// The text `'; '.join(...)` of the enforced missing outputs.
fn enforced_reasons(destruction: &Json, enforced: &[&str]) -> String {
    let mut parts = Vec::new();
    if let Some(Json::List(items)) = jget(destruction, "missing") {
        for item in items {
            let output = jget(item, "output").map(jstr).unwrap_or_default();
            if enforced.contains(&output.as_str()) {
                parts.push(format!(
                    "{output} ({})",
                    jget(item, "reason").map(jstr).unwrap_or_default()
                ));
            }
        }
    }
    parts.join("; ")
}

fn has_refused(destruction: &Json) -> bool {
    matches!(jget(destruction, "refused_outputs"), Some(Json::List(l)) if !l.is_empty())
}

/// Sorted PlayMakerFSM objects that are enabled and on an active scene object.
fn live_fsms<'a>(sc: &'a Scene<'_>) -> Result<Vec<(i64, i64, &'a Value)>> {
    let mut objects: Vec<&hk_unity::scene::SceneObject> = sc.objects.iter().collect();
    objects.sort_by_key(|o| o.id);
    let mut out = Vec::new();
    for o in objects {
        if o.typename != "PlayMakerFSM" || !k(&o.tree, "m_Enabled")?.truthy() {
            continue;
        }
        let gid = local_id(k(&o.tree, "m_GameObject")?)?;
        if sc.go(gid).is_none() || !sc.active(gid) {
            continue;
        }
        out.push((o.id, gid, &o.tree));
    }
    Ok(out)
}

/// Record `error` in `errors` or raise it, as the Python `errors is None` test does.
fn report(errors: &mut Option<&mut Vec<Json>>, entry: Json, error: String) -> Result<()> {
    match errors.as_deref_mut() {
        Some(list) => {
            list.push(entry);
            Ok(())
        }
        None => Err(error),
    }
}

/// `hidden_walls(sc, errors, contract, drawn_renderers)`: every active hidden wall in the scene.
pub fn hidden_walls(
    sc: &Scene,
    mut errors: Option<&mut Vec<Json>>,
    contract: bool,
    drawn_renderers: Option<&BTreeSet<String>>,
) -> Result<Vec<Json>> {
    let mut gravity: Option<f64> = None;
    let mut result = Vec::new();
    for (index, gid, tree) in live_fsms(sc)? {
        let fsm = k(tree, "fsm")?;
        let shape = match hidden_wall_shape(fsm) {
            Ok(Some(shape)) => shape,
            Ok(None) => continue,
            Err(error) => {
                report(
                    &mut errors,
                    jobj(vec![
                        ("id", Json::Str(sc.sid(index))),
                        ("type", js("hidden wall")),
                        ("error", Json::Str(error.clone())),
                    ]),
                    error,
                )?;
                continue;
            }
        };
        let one = (|| -> Result<Json> {
            let g = match gravity {
                Some(g) => g,
                None => {
                    let g = scene_gravity(&sc.source)?;
                    gravity = Some(g);
                    g
                }
            };
            let record = hidden_wall(sc, gid, index, fsm, &shape, g, drawn_renderers)?;
            let destruction = jget(&record, "destruction").unwrap();
            if contract && has_refused(destruction) {
                return err(format!(
                    "authored destruction output the port cannot produce: {}",
                    enforced_reasons(destruction, &ENFORCED_OUTPUTS)
                ));
            }
            Ok(record)
        })();
        match one {
            Ok(r) => result.push(r),
            Err(error) => report(
                &mut errors,
                jobj(vec![
                    ("id", Json::Str(sc.sid(index))),
                    ("type", js("hidden wall")),
                    ("error", Json::Str(error.clone())),
                ]),
                error,
            )?,
        }
    }
    Ok(result)
}

// -------------------------------------------------------------- cracked floors

pub const CRACKED_FLOOR_NAIL_HITS: i64 = 3;
const CRACKED_FLOOR_SOLID_CHILD: &str = "Solid";
const FLOOR_STATES: [&str; 9] = [
    "Idle",
    "Check If Nail",
    "Hit",
    "Initiate",
    "Break",
    "Hit 1",
    "Hit 2",
    "Pause",
    "Activated",
];
const FLOOR_OPEN_STATES: [&str; 10] = [
    "Idle",
    "Check If Nail",
    "Hit",
    "Initiate",
    "Break",
    "Break Wood",
    "Hit 1",
    "Hit 2",
    "Pause",
    "Activated",
];

fn cracked_floor_shape_for(digest: &str) -> Option<(&'static str, &'static [&'static str])> {
    match digest {
        "b74c4cdb9150a87be0a398b4d50bab0a1ebf40f99c2a1a746768b8573ed72d55" => {
            Some(("Hero Range and attack type", &FLOOR_STATES))
        }
        "72a2d729923a6564f3059f52037143bc90ad25cbf7975787bb5d4f59883d3f12" => {
            Some(("any Nail Attack trigger", &FLOOR_OPEN_STATES))
        }
        _ => None,
    }
}

/// `cracked_floor_shape(fsm)`: the pinned cracked-floor shape this FSM is, or None.
pub fn cracked_floor_shape(fsm: &Value) -> Result<Option<Shape>> {
    let names = state_names(fsm)?;
    let matches =
        |states: &[&str]| names.len() == states.len() && states.iter().all(|s| names.contains(*s));
    if !matches(&FLOOR_STATES) && !matches(&FLOOR_OPEN_STATES) {
        return Ok(None);
    }
    let digest = fsm_digest(fsm, &[])?;
    let Some((gate, states)) = cracked_floor_shape_for(&digest) else {
        return err(format!("unverified cracked floor variant: {digest}"));
    };
    if definition_name(fsm) != "break_floor" {
        return err(format!(
            "cracked floor digest {digest} under definition {}",
            pystr(&definition_name(fsm))
        ));
    }
    let start = ks(fsm, "startState")?;
    if start != "Pause" {
        return err(format!("cracked floor starts in {}", pystr(&start)));
    }
    Ok(Some(Shape {
        sha256: digest,
        definition: "break_floor",
        start: "Pause",
        kind: gate,
        renderer: "",
        states,
        clear_booleans: &[],
        empty_strings: &[],
    }))
}

/// `cracked_floor(sc, gid, index, fsm, shape, gravity)`.
pub fn cracked_floor(
    sc: &Scene,
    gid: i64,
    index: i64,
    fsm: &Value,
    shape: &Shape,
    gravity: f64,
) -> Result<Json> {
    let vars = fsm_variables(fsm)?;
    if !is_int_in(var(&vars, "Activated"), &[0]) {
        return err("cracked floor is serialized already broken");
    }
    if !is_int_in(var(&vars, "Hits"), &[0]) {
        return err("cracked floor is serialized part way through its hits");
    }
    let colliders: Vec<Comp> = components(sc, gid)?
        .into_iter()
        .filter(|c| c.1.ends_with("Collider2D"))
        .collect();
    if colliders.len() != 1 {
        return err(format!(
            "cracked floor carries {} hit colliders, not one",
            colliders.len()
        ));
    }
    let (body_id, collider_type, body) = colliders[0];
    if !k(body, "m_Enabled")?.truthy() || !k(body, "m_IsTrigger")?.truthy() {
        return err("cracked floor hit collider is not an enabled trigger");
    }
    let polygons = collider_polygons(sc, gid, collider_type, body)?;

    let mut solids = Vec::new();
    for child in descendant_set(sc, gid)? {
        if child == gid {
            continue;
        }
        let name = ks(sc.go(child).ok_or("'child'")?, "m_Name")?;
        if name != CRACKED_FLOOR_SOLID_CHILD || !sc.active(child) {
            continue;
        }
        for (i, t, c) in components(sc, child)? {
            if t.ends_with("Collider2D")
                && k(c, "m_Enabled")?.truthy()
                && !k(c, "m_IsTrigger")?.truthy()
            {
                solids.push(Json::Str(sc.sid(i)));
            }
        }
    }
    if solids.is_empty() {
        return err(format!(
            "cracked floor has no active {CRACKED_FLOOR_SOLID_CHILD} collider to remove"
        ));
    }
    let mut authored: Vec<&'static str> = vec![OUTPUT_COLLIDER, OUTPUT_VISUAL];
    let mut missing_list: Vec<Json> = Vec::new();
    let mut clips = break_clips(sc, fsm, "Break")?;
    clips.extend(break_clips(sc, fsm, "Break Wood")?);
    audio_output(&clips, &mut authored, &mut missing_list);
    let emitters = emitters_below(sc, gid)?;
    particle_outputs(
        sc,
        &emitters,
        gravity,
        &mut authored,
        &mut missing_list,
        &played_emitters(sc, gid, fsm)?,
    );
    prefab_particle_outputs(
        sc,
        gid,
        fsm,
        &["Break", "Break Wood"],
        gravity,
        &mut authored,
        &mut missing_list,
    );
    let mut renderers = Vec::new();
    for child in descendant_set(sc, gid)? {
        if child == gid {
            continue;
        }
        for (i, t, d) in components(sc, child)? {
            if t == "SpriteRenderer" && k(d, "m_Enabled")?.truthy() && sc.active(child) {
                renderers.push(Json::Str(sc.sid(i)));
            }
        }
    }
    let authored: Vec<&str> = OUTPUTS
        .iter()
        .copied()
        .filter(|n| authored.contains(n))
        .collect();
    let refused_out = refused_outputs(&missing_list, &ENFORCED_OUTPUTS);
    Ok(jobj(vec![
        ("gid", ji(gid)),
        ("source", Json::Str(sc.sid(index))),
        ("game_object", Json::Str(sc.sid(gid))),
        ("name", value_json(k(sc.go(gid).unwrap(), "m_Name")?)),
        ("definition", js(shape.definition)),
        ("fsm_sha256", Json::Str(shape.sha256.clone())),
        ("position", jfloats(&point(sc, gid)?)),
        ("nail_hits", ji(CRACKED_FLOOR_NAIL_HITS)),
        ("hit_gate", js(shape.kind)),
        ("hit_collider", Json::Str(sc.sid(body_id))),
        ("hit_polygons", polygons_json(&polygons)),
        ("box", jfloats(&polygons_box(&polygons))),
        ("solid_collider_sources", jl(solids)),
        ("renderer_sources", jl(renderers)),
        (
            "destruction",
            jobj(vec![
                ("authored", jstrs(&authored)),
                ("missing", jl(missing_list)),
                ("refused_outputs", refused_out),
            ]),
        ),
        (
            "limitations",
            jstrs(&[
                format!(
                    "{CRACKED_FLOOR_NAIL_HITS} nail hits with the two staged sag frames between them; the port has no multi-hit break, so this count is recorded and not yet run"
                ),
                "The flung wood and rock pool objects, the camera shake and the PersistentBoolItem that keeps a floor broken across a reload are not reproduced".to_string(),
            ]),
        ),
    ]))
}

/// `cracked_floors(sc, errors, contract)`: every active cracked floor in the scene.
pub fn cracked_floors(
    sc: &Scene,
    mut errors: Option<&mut Vec<Json>>,
    contract: bool,
) -> Result<Vec<Json>> {
    let mut gravity: Option<f64> = None;
    let mut result = Vec::new();
    for (index, gid, tree) in live_fsms(sc)? {
        let fsm = k(tree, "fsm")?;
        let shape = match cracked_floor_shape(fsm) {
            Ok(Some(shape)) => shape,
            Ok(None) => continue,
            Err(error) => {
                report(
                    &mut errors,
                    jobj(vec![
                        ("id", Json::Str(sc.sid(index))),
                        ("type", js("cracked floor")),
                        ("error", Json::Str(error.clone())),
                    ]),
                    error,
                )?;
                continue;
            }
        };
        let one = (|| -> Result<Json> {
            let g = match gravity {
                Some(g) => g,
                None => {
                    let g = scene_gravity(&sc.source)?;
                    gravity = Some(g);
                    g
                }
            };
            let record = cracked_floor(sc, gid, index, fsm, &shape, g)?;
            let destruction = jget(&record, "destruction").unwrap();
            if contract && has_refused(destruction) {
                return err(format!(
                    "authored destruction output the port cannot produce: {}",
                    enforced_reasons(destruction, &ENFORCED_OUTPUTS)
                ));
            }
            Ok(record)
        })();
        match one {
            Ok(r) => result.push(r),
            Err(error) => report(
                &mut errors,
                jobj(vec![
                    ("id", Json::Str(sc.sid(index))),
                    ("type", js("cracked floor")),
                    ("error", Json::Str(error.clone())),
                ]),
                error,
            )?,
        }
    }
    Ok(result)
}

// ------------------------------------------------------------- infected vines

pub const VINE_COMPONENT: &str = "BreakableInfectedVine";
const VINE_DEPTH_CENTRE: f64 = 0.004000000189989805;
const VINE_DEPTH_RANGE: f64 = 1.0;
const VINE_HIT_TAGS: [&str; 3] = ["Nail Attack", "Hero Spell", "HeroBox"];
pub const OUTPUT_ANIMATION: &str = "animation";
const VINE_ENFORCED_OUTPUTS: [&str; 3] = [OUTPUT_VISUAL, OUTPUT_PARTICLES, OUTPUT_ANIMATION];

/// One part a vine names: `(record, component types)`.
fn vine_parts(sc: &Scene, refs: &[Value], what: &str) -> Result<Vec<(Json, Vec<String>)>> {
    let file = file_of(sc);
    let mut parts = Vec::new();
    for r in refs {
        if ki(r, "m_PathID")? == 0 {
            continue;
        }
        if ki(r, "m_FileID")? != 0 {
            return err(format!("vine {what} lives outside this scene"));
        }
        let part_gid = ki(r, "m_PathID")?;
        let Some(go) = sc.go(part_gid) else {
            return err(format!("vine {what} is not a readable scene object"));
        };
        let mut types: Vec<String> = components(sc, part_gid)?
            .iter()
            .map(|c| c.1.to_string())
            .collect();
        types.sort();
        parts.push((
            jobj(vec![
                ("game_object", Json::Str(format!("{file}:{part_gid}"))),
                ("name", value_json(k(go, "m_Name")?)),
                ("components", jstrs(&types)),
            ]),
            types,
        ));
    }
    Ok(parts)
}

/// `infected_vine(sc, gid, tree)`.
pub fn infected_vine(sc: &Scene, gid: i64, tree: &Value) -> Result<Json> {
    let position = point(sc, gid)?;
    let inert = (position[2] - VINE_DEPTH_CENTRE).abs() > VINE_DEPTH_RANGE;
    let blobs = vine_parts(sc, kl(tree, "blobs")?, "blob")?;
    let effects = vine_parts(sc, kl(tree, "effects")?, "effect")?;
    let colliders: Vec<Comp> = components(sc, gid)?
        .into_iter()
        .filter(|c| c.1.ends_with("Collider2D"))
        .collect();
    if colliders.len() != 1 {
        return err(format!(
            "vine carries {} colliders, not one",
            colliders.len()
        ));
    }
    let (body_id, collider_type, body) = colliders[0];
    if !k(body, "m_IsTrigger")?.truthy() {
        return err("vine hit collider is not a trigger");
    }
    let spatter_keys = [
        "spatterAmount",
        "spatterAngleMin",
        "spatterAngleMax",
        "spatterSpeedMin",
        "spatterSpeedMax",
    ];
    let spatter: Vec<&Value> = spatter_keys
        .iter()
        .map(|key| k(tree, key))
        .collect::<Result<_>>()?;
    let finite_numbers = spatter.iter().all(|v| {
        matches!(
            v,
            Value::Int(_) | Value::UInt(_) | Value::F32(_) | Value::F64(_)
        ) && v.float().is_some_and(f64::is_finite)
    });
    if !finite_numbers {
        return err("vine spatter parameters are not finite numbers");
    }
    let mut authored: Vec<&'static str> = Vec::new();
    let mut missing_list: Vec<Json> = Vec::new();
    if !blobs.is_empty() {
        authored.push(OUTPUT_VISUAL);
        for (record, types) in &blobs {
            if !types.iter().any(|t| t == "SpriteRenderer") {
                missing_list.push(jobj(vec![
                    ("output", js(OUTPUT_VISUAL)),
                    ("part", jget(record, "game_object").cloned().unwrap()),
                    ("reason", js("vine blob has no SpriteRenderer to hide")),
                ]));
            }
        }
        let animated = blobs
            .iter()
            .filter(|(_, types)| types.iter().any(|t| t == "Animator"))
            .count();
        if animated > 0 {
            authored.push(OUTPUT_ANIMATION);
            missing_list.push(missing(
                OUTPUT_ANIMATION,
                None,
                &format!(
                    "{animated} vine blobs animate through Mecanim, which no cooked animation path reaches"
                ),
            ));
        }
    }
    if spatter[0].truthy() {
        authored.push(OUTPUT_PARTICLES);
        missing_list.push(missing(
            OUTPUT_PARTICLES,
            None,
            &format!(
                "{} blood spatters per blob come from the global pool, which this port has no emitter for",
                spatter[0].float().unwrap_or(0.0) as i64
            ),
        ));
    }
    if !effects.is_empty() {
        authored.push(OUTPUT_ANIMATION);
        let names: Vec<String> = effects
            .iter()
            .map(|(r, _)| jget(r, "name").map(jstr).unwrap_or_default())
            .collect();
        missing_list.push(missing(
            OUTPUT_ANIMATION,
            None,
            &format!(
                "vine effects are tk2d clips on separate objects: {}",
                names.join(", ")
            ),
        ));
    }
    let refused_out = refused_outputs(&missing_list, &VINE_ENFORCED_OUTPUTS);
    let file = file_of(sc);
    let set: BTreeSet<&str> = authored.iter().copied().collect();
    let set: Vec<&str> = set.into_iter().collect();
    Ok(jobj(vec![
        ("gid", ji(gid)),
        ("name", value_json(k(sc.go(gid).ok_or("'gid'")?, "m_Name")?)),
        ("position", jfloats(&position)),
        ("game_object", Json::Str(format!("{file}:{gid}"))),
        ("hit_collider", Json::Str(format!("{file}:{body_id}"))),
        ("collider_type", js(collider_type)),
        ("inert_by_depth", jb(inert)),
        ("hit_tags", jstrs(&VINE_HIT_TAGS)),
        (
            "spatter",
            jl(spatter.iter().map(|v| value_json(v)).collect()),
        ),
        (
            "audio_pitch",
            jl(vec![
                value_json(k(tree, "audioPitchMin")?),
                value_json(k(tree, "audioPitchMax")?),
            ]),
        ),
        ("blobs", jl(blobs.into_iter().map(|b| b.0).collect())),
        ("effects", jl(effects.into_iter().map(|b| b.0).collect())),
        ("persistent", jb(false)),
        (
            "destruction",
            jobj(vec![
                ("authored", jstrs(&set)),
                ("missing", jl(missing_list)),
                ("refused_outputs", refused_out),
            ]),
        ),
        (
            "limitations",
            jstrs(&["Vine state is not persistent in the source either; a cut vine returns when the scene reloads"]),
        ),
    ]))
}

/// `infected_vines(sc, errors, contract)`: every BreakableInfectedVine the source leaves hittable.
pub fn infected_vines(
    sc: &Scene,
    mut errors: Option<&mut Vec<Json>>,
    contract: bool,
) -> Result<Vec<Json>> {
    let file = file_of(sc);
    let mut result = Vec::new();
    let mut objects: Vec<&hk_unity::scene::SceneObject> = sc.objects.iter().collect();
    objects.sort_by_key(|o| o.id);
    for o in objects {
        if o.typename != VINE_COMPONENT {
            continue;
        }
        let gid = local_id(k(&o.tree, "m_GameObject")?)?;
        let enabled = o.tree.get("m_Enabled").is_none_or(Value::truthy);
        if sc.go(gid).is_none() || !enabled || !sc.active(gid) {
            continue;
        }
        let mut inert = false;
        let one = (|| -> Result<Json> {
            let mut record = infected_vine(sc, gid, &o.tree)?;
            jset(&mut record, "source", Json::Str(format!("{file}:{}", o.id)));
            if jget(&record, "inert_by_depth") == Some(&Json::Bool(true)) {
                inert = true;
                let z = match jget(&record, "position") {
                    Some(Json::List(p)) => match p.get(2) {
                        Some(Json::Float(z)) => *z,
                        _ => 0.0,
                    },
                    _ => 0.0,
                };
                return err(format!(
                    "vine is outside the authored depth band and Start disables it: z {z:.3}"
                ));
            }
            let destruction = jget(&record, "destruction").unwrap();
            if contract && has_refused(destruction) {
                return err(format!(
                    "authored destruction output the port cannot produce: {}",
                    enforced_reasons(destruction, &VINE_ENFORCED_OUTPUTS)
                ));
            }
            Ok(record)
        })();
        match one {
            Ok(r) => result.push(r),
            Err(error) => report(
                &mut errors,
                jobj(vec![
                    ("id", Json::Str(format!("{file}:{}", o.id))),
                    ("type", js(VINE_COMPONENT)),
                    ("error", Json::Str(error.clone())),
                    ("source_inert", jb(inert)),
                ]),
                error,
            )?,
        }
    }
    Ok(result)
}

// ------------------------------------------------------------ C# Breakables

/// `breakable_sources(sc, bounds, errors, contract)`: authored records,
/// optionally overlapping (xmin, ymin, xmax, ymax).
pub fn breakable_sources(
    sc: &Scene,
    bounds: Option<[f64; 4]>,
    mut errors: Option<&mut Vec<Json>>,
    contract: bool,
) -> Result<Vec<Json>> {
    let file = file_of(sc);
    let mut gravity: Option<f64> = None;
    let mut all_ids: Vec<i64> = Vec::new();
    for o in &sc.base.objects {
        if o.class_id == 114 {
            let obj = Obj {
                file: sc.base.clone(),
                info: *o,
            };
            if u(sc.source.typename(&obj))? == "Breakable" {
                all_ids.push(o.path_id);
            }
        }
    }
    all_ids.sort_unstable();
    if all_ids.len() > MAX_SCENE_BREAKABLES {
        return err(format!(
            "Breakable scene state budget exceeded: {} > {MAX_SCENE_BREAKABLES}",
            all_ids.len()
        ));
    }
    let mut result = Vec::new();
    for (state_index, &index) in all_ids.iter().enumerate() {
        let one = (|| -> Result<Option<Json>> {
            let Some(object) = sc.object(index) else {
                return err("Breakable schema was not successfully read");
            };
            let tree = &object.tree;
            let gid = local_id(k(tree, "m_GameObject")?)?;
            if !k(tree, "m_Enabled")?.truthy() || !sc.active(gid) {
                return Ok(None);
            }
            let position = point(sc, gid)?;
            if !(kf(tree, "inertForegroundThreshold")? <= position[2]
                && position[2] <= kf(tree, "inertBackgroundThreshold")?)
            {
                return Ok(None);
            }
            let colliders: Vec<Comp> = components(sc, gid)?
                .into_iter()
                .filter(|c| c.1.ends_with("Collider2D"))
                .collect();
            if colliders.is_empty() {
                return err("Breakable has no body collider");
            }
            let (body_id, collider_type, body) = colliders[0];
            if !k(body, "m_Enabled")?.truthy() {
                return Ok(None);
            }
            let hit_polygons = collider_polygons(sc, gid, collider_type, body)?;
            let boxed = polygons_box(&hit_polygons);
            if let Some(b) = bounds {
                if boxed[2] < b[0] || boxed[0] > b[2] || boxed[3] < b[1] || boxed[1] > b[3] {
                    return Ok(None);
                }
            }
            let mut whole_gids: BTreeSet<i64> = BTreeSet::new();
            let mut remnant_gids: BTreeSet<i64> = BTreeSet::new();
            for r in kl(tree, "wholeParts")? {
                if ki(r, "m_PathID")? != 0 {
                    whole_gids.extend(descendants(sc, local_id(r)?)?);
                }
            }
            for r in kl(tree, "remnantParts")? {
                if ki(r, "m_PathID")? != 0 {
                    remnant_gids.extend(descendants(sc, local_id(r)?)?);
                }
            }
            let whole_renderer = k(tree, "wholeRenderer")?;
            let mut off: BTreeSet<i64> = if ki(whole_renderer, "m_PathID")? != 0 {
                BTreeSet::from([local_id(whole_renderer)?])
            } else {
                BTreeSet::new()
            };
            let mut on: BTreeSet<i64> = BTreeSet::new();
            let mut disabled: BTreeSet<i64> = BTreeSet::from([body_id]);
            for part_gid in whole_gids.union(&remnant_gids) {
                for (part_id, part_type, part) in components(sc, *part_gid)? {
                    if part_type == "SpriteRenderer" && k(part, "m_Enabled")?.truthy() {
                        if whole_gids.contains(part_gid) {
                            off.insert(part_id);
                        } else {
                            on.insert(part_id);
                        }
                    } else if part_type.ends_with("Collider2D")
                        && whole_gids.contains(part_gid)
                        && k(part, "m_Enabled")?.truthy()
                    {
                        disabled.insert(part_id);
                    }
                }
            }
            for renderer_id in off.union(&on) {
                if sc.object(*renderer_id).map(|o| o.typename.as_str()) != Some("SpriteRenderer") {
                    return err("non-SpriteRenderer Breakable static part");
                }
            }
            if off.intersection(&on).next().is_some() {
                return err("Breakable renderer is both whole and remnant");
            }
            let mut persistence = Vec::new();
            for (cid, ctype, component) in components(sc, gid)? {
                if ctype == "PersistentBoolItem" {
                    let data = k(component, "persistentBoolData")?;
                    persistence.push(jobj(vec![
                        ("source", Json::Str(format!("{file}:{cid}"))),
                        ("authored_id", value_json(k(data, "id")?)),
                        (
                            "runtime_id_if_empty",
                            value_json(k(sc.go(gid).ok_or("'gid'")?, "m_Name")?),
                        ),
                        ("authored_scene_name", value_json(k(data, "sceneName")?)),
                        (
                            "semi_persistent",
                            jb(k(component, "semiPersistent")?.truthy()),
                        ),
                        ("dont_save", jb(k(component, "dontSave")?.truthy())),
                    ]));
                }
            }
            let mut debris = Vec::new();
            let mut debris_names = Vec::new();
            for r in kl(tree, "debrisParts")? {
                if ki(r, "m_PathID")? == 0 {
                    continue;
                }
                let part_gid = local_id(r)?;
                let mut comps = Vec::new();
                for (part_id, part_type, part) in components(sc, part_gid)? {
                    let mut c = vec![
                        ("source", Json::Str(format!("{file}:{part_id}"))),
                        ("type", js(part_type)),
                    ];
                    if ["Rigidbody2D", "SpinSelf", "ObjectBounce"].contains(&part_type) {
                        c.push(("serialized", value_json(part)));
                    }
                    comps.push(jobj(c));
                }
                debris_names.push(format!("{file}:{part_gid}"));
                debris.push(jobj(vec![
                    ("game_object", Json::Str(format!("{file}:{part_gid}"))),
                    (
                        "name",
                        value_json(k(sc.go(part_gid).ok_or("'part'")?, "m_Name")?),
                    ),
                    ("position", jfloats(&point(sc, part_gid)?)),
                    ("components", jl(comps)),
                ]));
            }
            let receiver = k(tree, "hitEventReciever")?;
            let (fades, event_errors) = if ki(receiver, "m_PathID")? != 0 {
                mask_fades(sc, local_id(receiver)?)?
            } else {
                (vec![], vec![])
            };
            let audio_record = audio(sc, tree)?;
            let hit_receiver = if ki(receiver, "m_PathID")? != 0 {
                Json::Str(u(sc.source.deref(&sc.base, receiver))?.sid())
            } else {
                Json::Null
            };
            let mut record = jobj(vec![
                ("source", Json::Str(format!("{file}:{index}"))),
                ("game_object", Json::Str(format!("{file}:{gid}"))),
                ("gid", ji(gid)),
                ("name", value_json(k(sc.go(gid).ok_or("'gid'")?, "m_Name")?)),
                ("state_index", ji(state_index as i64)),
                ("scene_state_count", ji(all_ids.len() as i64)),
                ("position", jfloats(&position)),
                ("hit_points", ji(1)),
                ("box", jfloats(&boxed)),
                ("hit_polygons", polygons_json(&hit_polygons)),
                ("body_collider", Json::Str(format!("{file}:{body_id}"))),
                ("body_is_trigger", jb(k(body, "m_IsTrigger")?.truthy())),
                ("off_renderer_ids", jl(off.iter().map(|&i| ji(i)).collect())),
                ("on_renderer_ids", jl(on.iter().map(|&i| ji(i)).collect())),
                (
                    "disabled_collider_ids",
                    jl(disabled.iter().map(|&i| ji(i)).collect()),
                ),
                (
                    "off_renderer_sources",
                    jl(off.iter().map(|i| Json::Str(format!("{file}:{i}"))).collect()),
                ),
                (
                    "on_renderer_sources",
                    jl(on.iter().map(|i| Json::Str(format!("{file}:{i}"))).collect()),
                ),
                (
                    "disabled_collider_sources",
                    jl(disabled.iter().map(|&i| Json::Str(sc.sid(i))).collect()),
                ),
                ("persistence", jl(persistence)),
                ("audio", audio_record.clone()),
                ("mask_fades", jl(fades.clone())),
                ("event_errors", jl(event_errors.clone())),
                ("debris", jl(debris)),
                (
                    "fling_speed",
                    jl(vec![
                        value_json(k(tree, "flingSpeedMin")?),
                        value_json(k(tree, "flingSpeedMax")?),
                    ]),
                ),
                ("angle_offset", value_json(k(tree, "angleOffset")?)),
                (
                    "forwarded_events",
                    jobj(vec![
                        ("hit_receiver", hit_receiver.clone()),
                        (
                            "receiver_event",
                            if ki(receiver, "m_PathID")? != 0 { js("HIT") } else { Json::Null },
                        ),
                        (
                            "self_event",
                            if k(tree, "forwardBreakEvent")?.truthy() { js("BREAK") } else { Json::Null },
                        ),
                    ]),
                ),
                (
                    "limitations",
                    jstrs(&[
                        "Debris physics, dust and impact effects, audio playback and forwarded FSM events require separate runtime support",
                        "PersistentBoolItem identity/state is recorded; no memory-card or retail-save compatibility is implied",
                    ]),
                ),
            ]);
            if contract {
                let g = match gravity {
                    Some(g) => g,
                    None => {
                        let g = scene_gravity(&sc.source)?;
                        gravity = Some(g);
                        g
                    }
                };
                let clip_names: Vec<String> = match jget(&audio_record, "options") {
                    Some(Json::List(l)) => l
                        .iter()
                        .map(|o| jget(o, "name").map(jstr).unwrap_or_default())
                        .collect(),
                    _ => vec![],
                };
                let event_messages: Vec<String> = event_errors
                    .iter()
                    .map(|e| jget(e, "error").map(jstr).unwrap_or_default())
                    .collect();
                let input = ContractInput {
                    has_visual: !off.is_empty() || !on.is_empty(),
                    clips: clip_names,
                    debris: debris_names,
                    hit_receiver: hit_receiver != Json::Null,
                    mask_fades: !fades.is_empty(),
                    event_errors: event_messages.iter().map(String::as_str).collect(),
                };
                let destruction = destruction_contract(sc, &input, g)?;
                jset(&mut record, "destruction", destruction.clone());
                if has_refused(&destruction) {
                    return err(format!(
                        "authored destruction output the port cannot produce: {}",
                        enforced_reasons(&destruction, &ENFORCED_OUTPUTS)
                    ));
                }
            }
            Ok(Some(record))
        })();
        match one {
            Ok(Some(r)) => result.push(r),
            Ok(None) => {}
            Err(error) => report(
                &mut errors,
                jobj(vec![
                    ("id", Json::Str(format!("{file}:{index}"))),
                    ("type", js("Breakable")),
                    ("error", Json::Str(error.clone())),
                ]),
                error,
            )?,
        }
    }
    Ok(result)
}

/// `bind_breakables(records, draws, edges)`: attach the region's draw and edge
/// indices without changing the stable scene state indices.
pub fn bind_breakables(
    records: &[Json],
    draw_sources: &[String],
    edge_sources: &[String],
) -> Vec<Json> {
    let mut draw_ids: HashMap<&str, usize> = HashMap::new();
    for (i, s) in draw_sources.iter().enumerate() {
        draw_ids.insert(s, i);
    }
    let strs = |j: Option<&Json>| -> Vec<String> {
        match j {
            Some(Json::List(l)) => l.iter().map(jstr).collect(),
            _ => vec![],
        }
    };
    let mut out = Vec::new();
    for record in records {
        let mut bound = record.clone();
        let off = strs(jget(record, "off_renderer_sources"));
        let on = strs(jget(record, "on_renderer_sources"));
        let disabled = strs(jget(record, "disabled_collider_sources"));
        let pick = |names: &[String]| -> Json {
            jl(names
                .iter()
                .filter_map(|s| draw_ids.get(s.as_str()).map(|&i| ji(i as i64)))
                .collect())
        };
        jset(&mut bound, "off_draws", pick(&off));
        jset(&mut bound, "on_draws", pick(&on));
        jset(
            &mut bound,
            "edge_indices",
            jl(edge_sources
                .iter()
                .enumerate()
                .filter(|(_, s)| disabled.contains(s))
                .map(|(i, _)| ji(i as i64))
                .collect()),
        );
        let fades: Vec<Json> = match jget(record, "mask_fades") {
            Some(Json::List(l)) => l
                .iter()
                .map(|f| {
                    let mut f = f.clone();
                    let sources = strs(jget(&f, "renderer_sources"));
                    jset(&mut f, "draw_indices", pick(&sources));
                    f
                })
                .collect(),
            _ => vec![],
        };
        jset(&mut bound, "mask_fades", jl(fades));
        let unresident: Vec<String> = off
            .iter()
            .chain(on.iter())
            .filter(|s| !draw_ids.contains_key(s.as_str()))
            .cloned()
            .collect();
        jset(
            &mut bound,
            "unresident_renderer_sources",
            jstrs(&unresident),
        );
        out.push(bound);
    }
    out
}
