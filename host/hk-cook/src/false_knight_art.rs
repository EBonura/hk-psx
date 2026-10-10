//! The source-reading and table-writing parts of host/false_knight_art.py: the
//! frame decomposition, the shockwave contract, the placement facts the guest
//! needs beside the art, and the generated Rust tables.
//!
//! What stays in Python until the sprite cook is ported is everything that touches
//! pixels: extracting each tk2d sprite, quantizing it, packing it into the scene's
//! texture pages and appending the parts to the actor atlas.

use crate::actors::Row;
use crate::common::{component_records, err, get, go_of, py_round, Result};
use crate::cook_audio::{jobj, js, u};
use crate::prefab::{
    actions_of, enum_param, literal, num, one_of, prefab_box, prefab_parts, single_state,
};
use crate::pyfloat;
use crate::pyjson::Json;
use crate::recog::{variables, xy};
use crate::static_sources::pogo_sources;
use hk_unity::playmaker::field;
use hk_unity::scene::Scene;
use hk_unity::{Obj, Source, Value};

/// `cook.SLOT_PIXELS`, `cook.MAX_TEXTURE_AXIS`.
const SLOT_PIXELS: usize = 64;
const MAX_TEXTURE_AXIS: usize = 252;
const SPURT_CLIP: &str = "Shockwave Spurt";
/// `FLOOR_STATES`, `FLOOR_NORMAL`, `ARMOUR_SPRITES`, `BREAK_FLOOR`.
const FLOOR_STATES: [(&str, [&str; 2]); 2] = [
    ("Cracked", ["Cracked 1", "Cracked 2"]),
    ("Broken", ["Broken", ""]),
];
const FLOOR_NORMAL: [&str; 2] = ["Normal 1", "Normal 2"];
const ARMOUR_SPRITES: [&str; 2] = ["body", "staff_piece"];
const BREAK_FLOOR: &str = "Break Floor";
/// A part costs a 16-byte texture record, a 20-byte frame record, a 20-byte alpha cover and a quad.
const PART_COST: i64 = 256;
const STREAMED_PART_COST: i64 = 192;
const MAX_STREAMED_PARTS: usize = 12;
const MAX_STATIC_PARTS: usize = 16;
/// The first page ids of host/pack_scenes: additive-scene objects carry shifted ids from here.
const ADDITIVE_ID_BASE: i64 = 100000;

pub(crate) fn aligned(n: i64) -> i64 {
    (n + 3) / 4 * 4
}

pub(crate) fn rect_bytes(rect: &[i64; 4]) -> i64 {
    aligned(rect[2]) / 2 * rect[3]
}

/// `decompose(plane, w, h, streamed)`: the cheapest cut of a frame into trimmed parts.
pub fn decompose(plane: &[u8], w: usize, h: usize, streamed: bool) -> Result<Vec<[i64; 4]>> {
    let width = if streamed {
        SLOT_PIXELS
    } else {
        MAX_TEXTURE_AXIS
    };
    let left = (0..h)
        .flat_map(|y| (0..w).filter(move |&x| plane[y * w + x] != 0))
        .min()
        .unwrap_or(0);
    let columns = ((w - left).div_ceil(width)).max(1);
    // Per row and column: (first, last) opaque x or None.
    let mut spans: Vec<Vec<Option<(usize, usize)>>> = Vec::with_capacity(h);
    for y in 0..h {
        let row = &plane[y * w..(y + 1) * w];
        let mut cells = Vec::with_capacity(columns);
        for c in 0..columns {
            let (x0, x1) = (left + c * width, w.min(left + c * width + width));
            let hit: Vec<usize> = (x0..x1).filter(|&x| row[x] != 0).collect();
            cells.push(if hit.is_empty() {
                None
            } else {
                Some((hit[0], *hit.last().unwrap()))
            });
        }
        spans.push(cells);
    }
    let tallest = if streamed { SLOT_PIXELS } else { h.min(256) };
    let cap = if streamed {
        MAX_STREAMED_PARTS
    } else {
        MAX_STATIC_PARTS
    };
    type Choice = (i64, usize, Option<(usize, Vec<[i64; 4]>)>);
    let mut best: Vec<Option<Choice>> = vec![None; h + 1];
    best[0] = Some((0, 0, None));
    for y1 in 1..=h {
        let mut choice: Option<Choice> = None;
        // Grow the band upward from row y1-1, keeping each column's extents.
        let mut ext: Vec<Option<[usize; 4]>> = vec![None; columns];
        let lowest = y1.saturating_sub(tallest);
        for y0 in (lowest..y1).rev() {
            for c in 0..columns {
                let Some(span) = spans[y0][c] else { continue };
                ext[c] = Some(match ext[c] {
                    None => [span.0, span.1, y0, y0],
                    Some(e) => [e[0].min(span.0), e[1].max(span.1), y0, e[3]],
                });
            }
            let Some((base_cost, base_count, _)) = &best[y0] else {
                continue;
            };
            let pieces: Vec<[i64; 4]> = ext
                .iter()
                .flatten()
                .map(|e| {
                    [
                        e[0] as i64,
                        e[2] as i64,
                        (e[1] - e[0] + 1) as i64,
                        (e[3] - e[2] + 1) as i64,
                    ]
                })
                .collect();
            let count = base_count + pieces.len();
            if count > cap {
                continue;
            }
            let cost = base_cost
                + pieces
                    .iter()
                    .map(|p| {
                        rect_bytes(p)
                            + if streamed {
                                STREAMED_PART_COST
                            } else {
                                PART_COST
                            }
                    })
                    .sum::<i64>();
            if choice.as_ref().is_none_or(|c| (cost, count) < (c.0, c.1)) {
                choice = Some((cost, count, Some((y0, pieces))));
            }
        }
        best[y1] = choice;
    }
    if best[h].is_none() {
        return err(format!("a {w}x{h} frame cannot be cut into {cap} parts"));
    }
    let (mut parts, mut y) = (Vec::new(), h);
    while y != 0 {
        let (y0, pieces) = best[y].as_ref().unwrap().2.clone().unwrap();
        let mut joined = pieces;
        joined.extend(parts);
        parts = joined;
        y = y0;
    }
    Ok(parts)
}

/// `_lerp`: Python's floor division on exact integers.
fn lerp(a: i64, b: i64, at: i64, of: i64) -> i64 {
    (a as i128 + ((b - a) as i128 * at as i128).div_euclid(of as i128)) as i64
}

/// `part_box(box_q16, w, h, rect)`: a part's share of the frame's world box.
pub fn part_box(b: [i64; 4], w: i64, h: i64, rect: [i64; 4]) -> [i64; 4] {
    let [x, y, pw, ph] = rect;
    [
        lerp(b[0], b[2], x, w),
        lerp(b[3], b[1], y + ph, h),
        lerp(b[0], b[2], x + pw, w),
        lerp(b[3], b[1], y, h),
    ]
}

pub(crate) fn q(v: f64) -> i64 {
    py_round(v * 65536.0)
}

/// `guest_wrap(clip)`: tk2d wrapMode to the guest's loop/loop-section/once set.
pub fn guest_wrap(clip: &Value) -> Result<i64> {
    let mode = get(clip, "wrapMode")?.int().unwrap_or(-1);
    let count = get(clip, "frames")?.list().map_or(0, <[Value]>::len);
    match mode {
        0..=2 => Ok(mode),
        6 if count == 1 => Ok(2),
        _ => err(format!(
            "unsupported tk2d wrap mode {mode} for {count}-frame clip"
        )),
    }
}

fn int_json(v: i64) -> Json {
    Json::Int(v)
}

/// The shockwave contract `shockwave_source` reads, plus the spurt's library.
pub struct Shockwave {
    pub params: Json,
    pub library: Obj,
    pub clip_name: String,
    pub clip: Value,
}

pub(crate) fn falsey_control<'a>(sc: &'a Scene, gid: i64) -> Result<&'a Value> {
    component_records(sc, gid)
        .into_iter()
        .rev()
        .find(|r| {
            r.1 == "PlayMakerFSM"
                && r.2
                    .get("fsm")
                    .and_then(|f| f.get("name"))
                    .and_then(Value::str)
                    .as_deref()
                    == Some("FalseyControl")
        })
        .and_then(|r| r.2.get("fsm"))
        .ok_or_else(|| "no FalseyControl".to_string())
}

/// `_snapshot_name`: the audio snapshot a TransitionToAudioSnapshot names.
fn snapshot_name(sc: &Scene, source: &Source, state: &Value, index: usize) -> Result<String> {
    let data = get(state, "actionData")?;
    let ints = |k: &str| -> Vec<i64> {
        data.get(k)
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
    let count = get(data, "actionNames")?.list().unwrap_or(&[]).len();
    let params = get(data, "paramName")?.list().unwrap_or(&[]).len();
    let end = if index + 1 < count {
        starts[index + 1] as usize
    } else {
        params
    };
    for i in starts[index] as usize..end {
        if types[i] == 24 {
            let objects = get(data, "fsmObjectParams")?.list().unwrap_or(&[]);
            let reference = objects.get(pos[i] as usize).and_then(|o| o.get("value"));
            if let Some(r) =
                reference.filter(|r| r.get("m_PathID").and_then(Value::int).unwrap_or(0) != 0)
            {
                let obj = u(sc.deref(r))?;
                return Ok(get(&u(source.read(&obj))?, "m_Name")?
                    .str()
                    .unwrap_or_default());
            }
        }
    }
    err("state names no snapshot")
}

/// `shockwave_source(s, sc, fk_gid)`: `S Attack Recover`'s ground wave, read from the two pooled prefabs.
pub fn shockwave_source(sc: &Scene, source: &Source, fk_gid: i64) -> Result<Shockwave> {
    let control = falsey_control(sc, fk_gid)?;
    let recover = single_state(control, "S Attack Recover")?;
    let origin = one_of(recover, "SetVector3XYZ")?;
    let spawn = one_of(recover, "SpawnObjectFromGlobalPool")?;
    let set_speed = one_of(recover, "SetFsmFloat")?;
    let named = |key: &str| {
        field(&set_speed, key)
            .and_then(|v| v.get("value"))
            .and_then(Value::str)
    };
    if named("variableName").as_deref() != Some("Speed")
        || named("fsmName").as_deref() != Some("shockwave")
    {
        return err("S Attack Recover no longer writes the wave Speed");
    }
    let speed = literal(&set_speed, "setValue")?;
    let wave_o = u(sc.deref(
        field(&spawn, "gameObject")
            .and_then(|g| g.get("value"))
            .ok_or("no spawn target")?,
    ))?;
    let wave_name = get(&u(source.read(&wave_o))?, "m_Name")?
        .str()
        .unwrap_or_default();
    if wave_name != "Shockwave Wave" {
        return err(format!("S Attack Recover spawns {wave_name}"));
    }
    let wave_parts = prefab_parts(source, &wave_o)?;
    let fsm_of = |parts: &'_ [(String, Obj, Value)], name: &str| -> Result<Value> {
        parts
            .iter()
            .find(|p| {
                p.0 == "PlayMakerFSM"
                    && p.2
                        .get("fsm")
                        .and_then(|f| f.get("name"))
                        .and_then(Value::str)
                        .as_deref()
                        == Some(name)
            })
            .and_then(|p| p.2.get("fsm"))
            .cloned()
            .ok_or_else(|| format!("no {name} FSM"))
    };
    let fsm = fsm_of(&wave_parts, "shockwave")?;
    let start = single_state(&fsm, "Start Move")?;
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
    let movement = single_state(&fsm, "Move")?;
    let ray = literal(&one_of(movement, "RayCast2d")?, "distance")?;
    one_of(movement, "Trigger2dEventLayer")?;
    let mut spurt_o: Option<Obj> = None;
    let right = get(single_state(&fsm, "Right")?, "actionData")?;
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
    let params_len = get(right, "paramName")?.list().unwrap_or(&[]).len();
    let gos = get(right, "fsmGameObjectParams")?.list().unwrap_or(&[]);
    for (i, nm) in names.iter().enumerate() {
        if nm.str().is_some_and(|s| s.ends_with("SetGameObject"))
            && enabled.get(i).is_some_and(Value::truthy)
        {
            let end = if i + 1 < names.len() {
                starts[i + 1] as usize
            } else {
                params_len
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
    let spurt_name = get(&u(source.read(&spurt_o))?, "m_Name")?
        .str()
        .unwrap_or_default();
    if spurt_name != "Shockwave Spurt" {
        return err(format!("the wave spawns {spurt_name}"));
    }
    let spurt_parts = prefab_parts(source, &spurt_o)?;
    let timing = fsm_of(&spurt_parts, "Damage timing")?;
    let arm = literal(&one_of(single_state(&timing, "Wait")?, "Wait")?, "time")?;
    let armed = literal(&one_of(single_state(&timing, "Activate")?, "Wait")?, "time")?;
    let scalar_or_plain = |fields: &hk_unity::playmaker::Fields, key: &str| -> Result<f64> {
        match field(fields, key) {
            Some(v) if v.is_map() => literal(fields, key),
            Some(v) => num(v).ok_or_else(|| "damageDealt".to_string()),
            None => err("no damageDealt"),
        }
    };
    let damage = scalar_or_plain(
        &one_of(single_state(&timing, "Activate")?, "SetDamageHeroAmount")?,
        "damageDealt",
    )?;
    if scalar_or_plain(
        &one_of(single_state(&timing, "Deactivate")?, "SetDamageHeroAmount")?,
        "damageDealt",
    )? != 0.0
    {
        return err("the spurt no longer disarms its DamageHero");
    }
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
    let clip_name = get(clip, "name")?.str().unwrap_or_default();
    if clip_name != SPURT_CLIP || guest_wrap(clip)? != 2 {
        return err("the spurt no longer plays Shockwave Spurt once");
    }
    let lifetime = get(clip, "frames")?.list().unwrap_or(&[]).len() as f64
        / get(clip, "fps")?.float().ok_or("fps")?;
    let t = |seconds: f64| py_round(seconds * 60.0);
    // `Floor Break` takes the mixer to `Silent`, which is where the fight's music ends.
    let snap_state = single_state(control, "Floor Break")?;
    let mut snaps = actions_of(snap_state, "TransitionToAudioSnapshot")?;
    if snaps.len() != 1 {
        return err("Floor Break needs exactly one TransitionToAudioSnapshot");
    }
    let (snap, snap_index) = snaps.remove(0);
    let snapshot = snapshot_name(sc, source, snap_state, snap_index)?;
    if snapshot != "Silent" {
        return err(format!("Floor Break now transitions to {snapshot}"));
    }
    let ints4 = |a: [f64; 4]| Json::List(a.iter().map(|&v| int_json(q(v))).collect());
    let params = jobj(vec![
        ("origin_y", int_json(q(literal(&origin, "y")?))),
        ("start_speed", int_json(q(speed * start_factor))),
        ("accel", int_json(q(speed * accel_factor))),
        ("box", ints4(prefab_box(&wave_parts)?)),
        ("ground_ray", int_json(q(ray))),
        ("spurt_box", ints4(prefab_box(&spurt_parts)?)),
        ("damage_from", int_json(t(arm))),
        ("damage_to", int_json(t(arm + armed))),
        ("damage", int_json(damage.trunc() as i64)),
        ("spurt_ticks", int_json(t(lifetime))),
        (
            "silence_ticks",
            int_json(t(literal(&snap, "transitionTime")?)),
        ),
        (
            "sources",
            jobj(vec![
                ("wave", Json::Str(wave_o.sid())),
                ("spurt", Json::Str(spurt_o.sid())),
                ("library", Json::Str(library.sid())),
            ]),
        ),
    ]);
    Ok(Shockwave {
        params,
        library,
        clip_name,
        clip: clip.clone(),
    })
}

// --- placement facts ----------------------------------------------------------------------

/// `_children(sc, gid)`: name to game object over every transform parented to it, the last winning.
pub(crate) fn children(sc: &Scene, gid: i64) -> Result<Vec<(String, i64)>> {
    let tid = *sc.go_transform.get(&gid).ok_or("object has no transform")?;
    let mut out: Vec<(String, i64)> = Vec::new();
    for o in sc.objects.iter().filter(|o| o.typename == "Transform") {
        if get(get(&o.tree, "m_Father")?, "m_PathID")?.int() != Some(tid) {
            continue;
        }
        let g = go_of(&o.tree).unwrap_or(0);
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

pub(crate) fn child(kids: &[(String, i64)], name: &str) -> Result<i64> {
    kids.iter()
        .find(|k| k.0 == name)
        .map(|k| k.1)
        .ok_or_else(|| format!("missing child {name}"))
}

/// `_component(sc, gid, kind)`: the one component of a kind on a game object.
pub(crate) fn component<'a>(sc: &'a Scene, gid: i64, kind: &str) -> Result<&'a Value> {
    let found: Vec<&Value> = component_records(sc, gid)
        .into_iter()
        .filter(|r| r.1 == kind)
        .map(|r| r.2)
        .collect();
    if found.len() != 1 {
        return err(format!(
            "expected one {kind} on {}",
            get(sc.go(gid).ok_or("no GameObject")?, "m_Name")?
                .str()
                .unwrap_or_default()
        ));
    }
    Ok(found[0])
}

/// `_component_ids(sc, gid, kind)`: the game object's components of a kind, in component order.
pub(crate) fn component_ids(sc: &Scene, gid: i64, kind: &str) -> Result<Vec<i64>> {
    let mut out = Vec::new();
    for c in get(sc.go(gid).ok_or("no GameObject")?, "m_Component")?
        .list()
        .unwrap_or(&[])
    {
        let id = get(get(c, "component")?, "m_PathID")?.int().unwrap_or(0);
        if sc.object(id).is_some_and(|o| o.typename == kind) {
            out.push(id);
        }
    }
    Ok(out)
}

pub(crate) fn component_id(sc: &Scene, gid: i64, kind: &str) -> Result<i64> {
    let found = component_ids(sc, gid, kind)?;
    if found.len() != 1 {
        return err(format!(
            "expected one {kind} on {}",
            get(sc.go(gid).ok_or("no GameObject")?, "m_Name")?
                .str()
                .unwrap_or_default()
        ));
    }
    Ok(found[0])
}

/// The first additive-scene game object of a name, in object order.
pub(crate) fn additive_named(sc: &Scene, name: &str) -> Result<i64> {
    sc.objects
        .iter()
        .filter(|o| o.typename == "GameObject" && o.id >= ADDITIVE_ID_BASE)
        .find(|o| o.tree.get("m_Name").and_then(Value::str).as_deref() == Some(name))
        .map(|o| o.id)
        .ok_or_else(|| format!("no {name} in an additive scene"))
}

pub(crate) fn local(sc: &Scene, gid: i64) -> Result<([f64; 2], [f64; 2])> {
    let t = sc
        .transform(*sc.go_transform.get(&gid).ok_or("no transform")?)
        .ok_or("transform missing")?;
    Ok((xy(t, "m_LocalPosition")?, xy(t, "m_LocalScale")?))
}

/// The placement facts of the False Knight's scene that `source_art` returns beside the
/// sprites (its `objects`), with every check it makes on the way.
pub fn source_objects(sc: &Scene, source: &Source, fk: &Row) -> Result<Json> {
    let fk_gid = fk.game_object;
    let m = u(sc.world(*sc.go_transform.get(&fk_gid).ok_or("no transform")?))?;
    let sprite = fk.part("tk2dSprite").ok_or("no tk2dSprite")?;
    let sprite_scale = xy(sprite, "_scale")?;
    let scale = [
        (m[0][0] * sprite_scale[0]).abs(),
        (m[1][1] * sprite_scale[1]).abs(),
    ];
    if (scale[0] - scale[1]).abs() > 1e-6 {
        return err("False Knight art assumes a uniform transform scale");
    }
    let kids = children(sc, fk_gid)?;
    let (head, death_head) = (child(&kids, "Head")?, child(&kids, "Death Head")?);
    let ((_, head_scale), (dh_pos, dh_scale)) = (local(sc, head)?, local(sc, death_head)?);
    for gid in [head, death_head] {
        let tk = component(sc, gid, "tk2dSprite")?;
        let (color, s) = (get(get(tk, "_color")?, "a")?.float(), xy(tk, "_scale")?);
        if color != Some(1.0) || s[0] != 1.0 || s[1] != 1.0 {
            return err("False Knight child sprite is tinted or scaled");
        }
    }
    if (head_scale[0] - 1.0).abs() > 1e-6 || (dh_scale[0] - dh_scale[1]).abs() > 1e-6 {
        return err("False Knight Head/Death Head scale changed");
    }
    let wave = shockwave_source(sc, source, fk_gid)?;
    // Floor sprites: SpriteRenderers on `FK Floor`, placed relative to it.
    let floor_gid = additive_named(sc, "FK Floor")?;
    let floor_children = children(sc, floor_gid)?;
    let fm = u(sc.world(*sc.go_transform.get(&floor_gid).ok_or("no transform")?))?;
    let anchor = [fm[0][3], fm[1][3]];
    for (_, names) in FLOOR_STATES {
        for name in names.iter().filter(|n| !n.is_empty()) {
            let gid = child(&floor_children, name)?;
            let renderer = component(sc, gid, "SpriteRenderer")?;
            if get(get(renderer, "m_Color")?, "a")?.float() != Some(1.0)
                || get(renderer, "m_FlipX")?.truthy()
                || get(renderer, "m_FlipY")?.truthy()
            {
                return err(format!("floor sprite {name} is tinted or flipped"));
            }
            let wm = u(sc.world(*sc.go_transform.get(&gid).ok_or("no transform")?))?;
            if wm[0][1].abs() > 1e-6 || wm[1][0].abs() > 1e-6 {
                return err(format!("floor sprite {name} is rotated"));
            }
        }
    }
    let vars = variables(falsey_control(sc, fk_gid)?);
    let speed = vars
        .iter()
        .find(|(k, _)| k == "Death Head Speed")
        .map(|(_, v)| *v)
        .ok_or("no Death Head Speed")?;
    let fl = |a: f64, b: f64| Json::List(vec![Json::Float(a), Json::Float(b)]);
    let mut floor_sources = Vec::new();
    for name in FLOOR_NORMAL {
        floor_sources.push((
            name.to_string(),
            Json::Str(sc.sid(component_id(
                sc,
                child(&floor_children, name)?,
                "SpriteRenderer",
            )?)),
        ));
    }
    let break_floor = component_ids(sc, child(&floor_children, BREAK_FLOOR)?, "BoxCollider2D")?;
    let armour = additive_named(sc, "FK Armour")?;
    let armour_children = children(sc, armour)?;
    let mut armour_sources = Vec::new();
    for name in ARMOUR_SPRITES {
        armour_sources.push(Json::Str(sc.sid(component_id(
            sc,
            child(&armour_children, name)?,
            "SpriteRenderer",
        )?)));
    }
    // The armour's `Tinger` is a TinkEffect box the cook turns into a nail bounce.
    let tinger = sc.sid(component_id(
        sc,
        child(&armour_children, "Tinger")?,
        "BoxCollider2D",
    )?);
    let Json::Obj(pogo) = pogo_sources(sc)? else {
        return err("pogo sources");
    };
    let Some((_, Json::List(targets))) = pogo.into_iter().find(|p| p.0 == "targets") else {
        return err("pogo targets");
    };
    let tink: Vec<Json> = targets
        .into_iter()
        .filter(|t| matches!(t, Json::Obj(f) if f.iter().any(|k| k.0 == "source" && k.1 == Json::Str(tinger.clone()))))
        .filter_map(|t| if let Json::Obj(f) = t { f.into_iter().find(|k| k.0 == "bounds").map(|k| k.1) } else { None })
        .collect();
    if tink.len() != 1 {
        return err("FK Armour Tinger is no longer one static nail bounce");
    }
    let speed_json = match speed {
        Value::Int(i) => Json::Int(*i),
        other => Json::Float(other.float().ok_or("Death Head Speed")?),
    };
    Ok(jobj(vec![
        ("wave", wave.params),
        ("death_head_speed", speed_json),
        (
            "death_head_offset",
            fl(dh_pos[0] * scale[0], dh_pos[1] * scale[1]),
        ),
        ("floor_anchor", fl(anchor[0], anchor[1])),
        ("floor_sources", Json::Obj(floor_sources)),
        (
            "break_floor_sources",
            Json::List(break_floor.iter().map(|&i| Json::Str(sc.sid(i))).collect()),
        ),
        ("armour_sources", Json::List(armour_sources)),
        ("armour_tink", Json::List(tink)),
    ]))
}

// --- generated tables --------------------------------------------------------------------

fn jfield<'a>(j: &'a Json, key: &str) -> Result<&'a Json> {
    match j {
        Json::Obj(f) => f
            .iter()
            .find(|k| k.0 == key)
            .map(|k| &k.1)
            .ok_or_else(|| format!("missing {key}")),
        _ => err("not an object"),
    }
}

fn jf(j: &Json) -> Result<f64> {
    match j {
        Json::Int(i) => Ok(*i as f64),
        Json::Float(f) => Ok(*f),
        _ => err("not a number"),
    }
}

fn pyval(j: &Json) -> String {
    match j {
        Json::Int(i) => i.to_string(),
        Json::Float(f) => pyfloat::repr(*f),
        Json::Str(s) => s.clone(),
        other => crate::pyjson::dumps(other),
    }
}

/// One art clip row: first sequence entry, frames, fps (Q16), wrap, loop start, name.
pub type ClipRow = (i64, i64, i64, i64, i64, String);

/// `rust_table(...)`: data/false_knight_art.rs.
pub fn rust_table(
    anchor_clip: i64,
    sprite_rows: &[(i64, i64, bool)],
    clip_rows: &[ClipRow],
    sequence: &[i64],
    floor_rows: &[(String, Vec<i64>)],
    objects: &Json,
) -> Result<String> {
    let mut lines: Vec<String> = vec![
        "// Generated by host/false_knight_art.py from the installed source; do not edit.".into(),
        "// The False Knight's parts: see docs/FALSE_KNIGHT.md.".into(),
        format!("pub const FK_ART_ANCHOR_CLIP: u16 = {anchor_clip};"),
        "/// Per sprite: first part (frames after the anchor clip's first), parts, streamed."
            .into(),
        format!(
            "pub const FK_ART_SPRITES: [(u16, u8, bool); {}] = [",
            sprite_rows.len()
        ),
    ];
    lines.extend(
        sprite_rows
            .iter()
            .map(|(a, b, c)| format!("    ({a}, {b}, {}),", if *c { "true" } else { "false" })),
    );
    lines.push("];".into());
    lines.push("/// Per art clip, in `ArtClip` order: first sequence entry, frames, fps (Q16), wrap, loop start.".into());
    lines.push(format!(
        "pub const FK_ART_CLIPS: [(u16, u8, u32, u8, u8); {}] = [",
        clip_rows.len()
    ));
    lines.extend(
        clip_rows
            .iter()
            .map(|(a, b, c, d, e, n)| format!("    ({a}, {b}, {c}, {d}, {e}), // {n}")),
    );
    lines.push("];".into());
    lines.push(format!(
        "pub const FK_ART_SEQUENCE: [u16; {}] = [{}];",
        sequence.len(),
        sequence
            .iter()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    ));
    for (state, indices) in floor_rows {
        lines.push(format!(
            "pub const FK_FLOOR_{}: [u16; {}] = [{}];",
            state.to_uppercase(),
            indices.len(),
            indices
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let pair = |key: &str| -> Result<(f64, f64)> {
        match jfield(objects, key)? {
            Json::List(l) if l.len() == 2 => Ok((jf(&l[0])?, jf(&l[1])?)),
            _ => err(format!("{key} is not a pair")),
        }
    };
    let (ax, ay) = pair("floor_anchor")?;
    let (dx, dy) = pair("death_head_offset")?;
    lines.push(
        "/// `FK Floor`'s world position, which the floor sprites' boxes are relative to.".into(),
    );
    lines.push(format!(
        "pub const FK_FLOOR_ANCHOR: [i32; 2] = [{}, {}];",
        q(ax),
        q(ay)
    ));
    lines.push(
        "/// The `Death Head`'s local position, times the body's scale, as authored (facing left)."
            .into(),
    );
    lines.push(format!(
        "pub const FK_DEATH_HEAD_OFFSET: [i32; 2] = [{}, {}];",
        q(dx),
        q(dy)
    ));
    lines.push("/// `Death Head Speed`, Q16 units a second.".into());
    lines.push(format!(
        "pub const FK_DEATH_HEAD_SPEED: i32 = {};",
        q(jf(jfield(objects, "death_head_speed")?)?)
    ));
    let w = jfield(objects, "wave")?;
    let sources = jfield(w, "sources")?;
    let boxed = |key: &str| -> Result<String> {
        match jfield(w, key)? {
            Json::List(l) => Ok(format!(
                "[{}]",
                l.iter().map(pyval).collect::<Vec<_>>().join(", ")
            )),
            _ => err("box"),
        }
    };
    let v = |key: &str| -> Result<String> { Ok(pyval(jfield(w, key)?)) };
    lines.push(format!(
        "/// `Shockwave Wave` ({}) and its `Shockwave Spurt` ({}).",
        pyval(jfield(sources, "wave")?),
        pyval(jfield(sources, "spurt")?)
    ));
    lines.push(
        "/// Spawn height below the body's transform, `S Attack Recover`'s SetVector3XYZ.".into(),
    );
    lines.push(format!(
        "pub const FK_WAVE_ORIGIN_Y: i32 = {};",
        v("origin_y")?
    ));
    lines.push("/// `Start Move`: the set Speed times its factor, then twice the set Speed added a second (Q16).".into());
    lines.push(format!(
        "pub const FK_WAVE_START_SPEED: i32 = {};",
        v("start_speed")?
    ));
    lines.push(format!("pub const FK_WAVE_ACCEL: i32 = {};", v("accel")?));
    lines.push("/// The wave's terrain trigger and ground ray, rightward frame, Q16.".into());
    lines.push(format!(
        "pub const FK_WAVE_BOX: [i32; 4] = {};",
        boxed("box")?
    ));
    lines.push(format!(
        "pub const FK_WAVE_GROUND_RAY: i32 = {};",
        v("ground_ray")?
    ));
    lines.push("/// A spurt's DamageHero box, rightward frame, Q16, armed from tick FROM to TO of its life.".into());
    lines.push(format!(
        "pub const FK_SPURT_BOX: [i32; 4] = {};",
        boxed("spurt_box")?
    ));
    lines.push(format!(
        "pub const FK_SPURT_DAMAGE_FROM: u16 = {};",
        v("damage_from")?
    ));
    lines.push(format!(
        "pub const FK_SPURT_DAMAGE_TO: u16 = {};",
        v("damage_to")?
    ));
    lines.push(format!(
        "pub const FK_SPURT_DAMAGE: u16 = {};",
        v("damage")?
    ));
    lines.push("/// Ticks a spurt lives: its clip, played once, then recycled.".into());
    lines.push(format!(
        "pub const FK_SPURT_TICKS: u16 = {};",
        v("spurt_ticks")?
    ));
    lines.push("/// `Floor Break`'s TransitionToAudioSnapshot to `Silent`, in ticks.".into());
    lines.push(format!(
        "pub const FK_FLOOR_BREAK_SILENCE_TICKS: u16 = {};",
        v("silence_ticks")?
    ));
    Ok(lines.join("\n") + "\n")
}

/// One generated binding table: its doc line and either (slot, index) rows or boxes.
pub struct Binding {
    pub name: String,
    pub doc: String,
    pub rows: Vec<(i64, i64)>,
    pub boxes: Option<Vec<[i64; 4]>>,
}

/// `rust_bindings(bindings, scene_id)`: data/false_knight_floor.rs.
pub fn rust_bindings(bindings: &[Binding], scene_id: i64) -> String {
    let mut lines: Vec<String> = vec![
        "// Generated by host/false_knight_art.py from the cooked region report; do not edit."
            .into(),
        "// (catalogue slot, index) rows, sorted by slot for a binary search.".into(),
        "/// The guest scene id `Floor Control` and `FK Armour` stand in.".into(),
        format!("pub const FK_FLOOR_SCENE: usize = {scene_id};"),
    ];
    for b in bindings {
        lines.push(format!("/// {}", b.doc));
        if let Some(boxes) = &b.boxes {
            lines.push(format!(
                "pub const {}: [[i32; 4]; {}] = [{}];",
                b.name,
                boxes.len(),
                boxes
                    .iter()
                    .map(|x| format!(
                        "[{}]",
                        x.iter().map(i64::to_string).collect::<Vec<_>>().join(", ")
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            continue;
        }
        lines.push(format!(
            "pub const {}: [(u16, u16); {}] = [{}];",
            b.name,
            b.rows.len(),
            b.rows
                .iter()
                .map(|(a, c)| format!("({a}, {c})"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    lines.join("\n") + "\n"
}

/// `region_bindings(rows, objects)`: per catalogue slot, the draws and edges the floor and
/// armour state move. `draws_of(chunk_id)` gives the sources of that region's `scene.json` draws.
pub fn region_bindings(
    rows: &[(i64, Vec<String>)],
    objects: &Json,
    draws_of: &dyn Fn(i64) -> Result<Vec<String>>,
) -> Result<Vec<Binding>> {
    let strings = |key: &str| -> Result<Vec<String>> {
        match jfield(objects, key)? {
            Json::List(l) => Ok(l
                .iter()
                .filter_map(|s| {
                    if let Json::Str(s) = s {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .collect()),
            Json::Obj(f) => Ok(f
                .iter()
                .filter_map(|(_, s)| {
                    if let Json::Str(s) = s {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .collect()),
            _ => err(format!("{key} is not a list")),
        }
    };
    let mut tink: Vec<[i64; 4]> = Vec::new();
    if let Json::List(boxes) = jfield(objects, "armour_tink")? {
        for b in boxes {
            let Json::List(v) = b else {
                return err("tink box");
            };
            let r = [q(jf(&v[0])?), q(jf(&v[1])?), q(jf(&v[2])?), q(jf(&v[3])?)];
            if !tink.contains(&r) {
                tink.push(r);
            }
        }
    }
    let (normal_sources, armour_sources, floor_sources) = (
        strings("floor_sources")?,
        strings("armour_sources")?,
        strings("break_floor_sources")?,
    );
    let mut normal: Vec<(i64, i64)> = Vec::new();
    let mut armour: Vec<(i64, i64)> = Vec::new();
    let mut edges: Vec<(i64, i64)> = Vec::new();
    for (chunk_id, edge_sources) in rows {
        // The runtime's catalogue slot is the row's index, which the cook numbers from 1 as chunk ids.
        let slot = chunk_id - 1;
        for (i, d) in draws_of(*chunk_id)?.iter().enumerate() {
            if normal_sources.contains(d) {
                normal.push((slot, i as i64));
            }
            if armour_sources.contains(d) {
                armour.push((slot, i as i64));
            }
        }
        for (i, source) in edge_sources.iter().enumerate() {
            if floor_sources.contains(source) {
                edges.push((slot, i as i64));
            }
        }
    }
    if [&normal, &armour, &edges]
        .iter()
        .any(|t| t.iter().any(|&(a, b)| a > 65535 || b > 65535))
    {
        return err("floor binding index exceeds u16");
    }
    for t in [&mut normal, &mut armour, &mut edges] {
        t.sort();
    }
    tink.sort();
    let rows_binding = |name: &str, doc: &str, rows: Vec<(i64, i64)>| Binding {
        name: name.into(),
        doc: doc.into(),
        rows,
        boxes: None,
    };
    Ok(vec![
        rows_binding("FK_FLOOR_NORMAL_DRAWS", "(slot, draw) of `Normal 1`/`Normal 2`, hidden once the floor cracks.", normal),
        rows_binding("FK_ARMOUR_DRAWS", "(slot, draw) of `FK Armour`, which `Battle Control` destroys unless the arena was won.", armour),
        rows_binding("FK_BREAK_FLOOR_EDGES", "(slot, edge) of `Break Floor`'s colliders, lifted once the floor breaks.", edges),
        Binding { name: "FK_ARMOUR_TINK".into(), doc: "World box of `FK Armour`'s `Tinger` nail bounce, which goes with the armour.".into(), rows: Vec::new(), boxes: Some(tink) },
    ])
}

/// `neutral_actor(actor)`: the boss as the generic actor bank cooks it, `Blank` for every body slot.
pub fn neutral_actor(control: &Json, limitations: &[String]) -> (Json, Vec<String>) {
    let mut control = control.clone();
    if let Json::Obj(fields) = &mut control {
        if let Some(slot) = fields.iter_mut().find(|f| f.0 == "art_bindings") {
            if let Json::Obj(b) = &mut slot.1 {
                for entry in b.iter_mut() {
                    entry.1 = js("Blank");
                }
            }
        }
    }
    let mut limits: Vec<String> = limitations
        .iter()
        .filter(|t| !t.contains("clips are cooked"))
        .cloned()
        .collect();
    limits.push("Every clip FalseyControl plays, the Head, the Death Head, the empty armour and the floor states are cooked by host/false_knight_art.py into this scene alone; the ActorSpec clip fields point at Blank.".into());
    (control, limits)
}

/// `additive_named` for the bank cook.
pub(crate) fn additive_named_pub(sc: &Scene, name: &str) -> Result<i64> {
    additive_named(sc, name)
}
