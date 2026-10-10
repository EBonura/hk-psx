//! Hidden walls and cracked floors as multi-hit secret breakables
//! (host/secret_breaks.py): the recognised FSM instances of host/breakables.py
//! turned into what the port runs. Every number here is a literal of one of the
//! pinned definitions, so the digest is what keeps it true.

use crate::breakables::{
    self, descendants, file_of, jb, jf, jfloats, jget, ji, jl, jset, jstr, jstrs, k, kf, ki, ks,
    local_id, point, polygons_json, Comp,
};
use crate::common::{collider_polygons, components, err, Result};
use crate::cook_audio::{jobj, js, u};
use crate::music::value_json;
use crate::pyjson::Json;
use crate::scenery::native_sprite_geometry;
use hk_unity::scene::Scene;
use hk_unity::{Source, Value};
use std::collections::{BTreeSet, HashMap};

pub const FAMILY_WALL: i64 = 1;
pub const FAMILY_WALL_TK2D: i64 = 2;
pub const FAMILY_FLOOR: i64 = 3;
pub const FAMILY_FLOOR_OPEN: i64 = 4;

/// State indices come from the top of the scene's Breakable range.
pub const TOP_STATE: i64 = breakables::MAX_SCENE_BREAKABLES as i64 - 1;
pub const WALL_LOCKOUT_TICKS: i64 = 12;
pub const WALL_RECOIL_TICKS: i64 = 6;
pub const WALL_RECOIL_UNITS: f64 = 0.1;
/// `IntSwitch Facing`: the direction the wall's kinematic body first moves in each.
pub const WALL_RECOIL: [[i64; 2]; 4] = [[-1, 0], [0, 0], [1, 0], [0, 1]];
pub const FLOOR_LOCKOUT_TICKS: i64 = 15;
pub const FLOOR_PARTS: [&str; 5] = ["floor 1", "floor 2", "wood small", "wood large", "Solid"];
pub const FLOOR_MASK_CHILD: &str = "msk_generic";
pub const FLOOR_STRIKE_OFFSET: [f64; 2] = [0.0, -2.0];
pub const FLOOR_MASK_FADE_SECONDS: f64 = 0.4;

/// The sag per hit as the actions run: (child, Translate y, Rotate z degrees).
const FLOOR_SAG: [&[(&str, f64, f64)]; 2] = [
    &[
        ("floor 1", -0.05, 0.0),
        ("floor 2", -0.10, 0.0),
        ("floor 1", 0.0, -2.5),
        ("floor 2", 0.0, 3.5),
    ],
    &[
        ("floor 1", -0.10, 0.0),
        ("floor 2", -0.20, 0.0),
        ("floor 1", 0.0, -2.5),
        ("floor 2", 0.0, 2.5),
        ("floor 2", -0.15, 0.0),
    ],
];

/// Direct children by name, as Transform.Find sees them (first match).
fn children(sc: &Scene, gid: i64) -> Result<Vec<(String, i64)>> {
    let tid = *sc.go_transform.get(&gid).ok_or("'gid'")?;
    let mut out: Vec<(String, i64)> = Vec::new();
    for child in breakables::kl(sc.transform(tid).ok_or("'tid'")?, "m_Children")? {
        let t = sc
            .transform(ki(child, "m_PathID")?)
            .ok_or("child transform")?;
        let kid = ki(k(t, "m_GameObject")?, "m_PathID")?;
        let name = ks(sc.go(kid).ok_or("child object")?, "m_Name")?;
        if !out.iter().any(|e| e.0 == name) {
            out.push((name, kid));
        }
    }
    Ok(out)
}

fn child(list: &[(String, i64)], name: &str) -> Option<i64> {
    list.iter().find(|e| e.0 == name).map(|e| e.1)
}

/// Enabled SpriteRenderers with a sprite on active objects, as drawn.
fn renderers(sc: &Scene, gids: &BTreeSet<i64>) -> Result<Vec<Json>> {
    let mut out = Vec::new();
    for &gid in gids {
        if !sc.active(gid) {
            continue;
        }
        for (index, kind, tree) in components(sc, gid)? {
            if kind == "SpriteRenderer"
                && k(tree, "m_Enabled")?.truthy()
                && ki(k(tree, "m_Sprite")?, "m_PathID")? != 0
            {
                out.push(Json::Str(sc.sid(index)));
            }
        }
    }
    Ok(out)
}

/// Enabled solid terrain colliders (layer 8), keyed as the cook keys edges.
fn solid_colliders(sc: &Scene, gids: &BTreeSet<i64>) -> Result<Vec<Json>> {
    let mut out = Vec::new();
    for &gid in gids {
        if !sc.active(gid)
            || ki(sc.go(gid).ok_or("'gid'")?, "m_Layer")? != breakables::TERRAIN_LAYER
        {
            continue;
        }
        for (index, kind, tree) in components(sc, gid)? {
            if kind.ends_with("Collider2D")
                && k(tree, "m_Enabled")?.truthy()
                && !k(tree, "m_IsTrigger")?.truthy()
            {
                out.push(Json::Str(sc.sid(index)));
            }
        }
    }
    Ok(out)
}

fn camera_locks(sc: &Scene, gids: &BTreeSet<i64>) -> Result<Vec<Json>> {
    let mut out = Vec::new();
    for &gid in gids {
        if !sc.active(gid) {
            continue;
        }
        for (index, kind, _) in components(sc, gid)? {
            if kind == "CameraLockArea" {
                out.push(Json::Str(sc.sid(index)));
            }
        }
    }
    Ok(out)
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

// ------------------------------------------------------------------ transforms

type Quat = [f64; 4];
type M4 = [[f64; 4]; 4];

fn quat_mul(a: Quat, b: Quat) -> Quat {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

fn quat_rotate(q: Quat, v: [f64; 3]) -> [f64; 3] {
    let [x, y, z, w] = q;
    let [vx, vy, vz] = v;
    let (tx, ty, tz) = (
        2.0 * (y * vz - z * vy),
        2.0 * (z * vx - x * vz),
        2.0 * (x * vy - y * vx),
    );
    [
        vx + w * tx + y * tz - z * ty,
        vy + w * ty + z * tx - x * tz,
        vz + w * tz + x * ty - y * tx,
    ]
}

fn euler_z(degrees: f64) -> Quat {
    let a = degrees.to_radians() / 2.0;
    [0.0, 0.0, a.sin(), a.cos()]
}

fn quat_of(t: &Value) -> Result<Quat> {
    let q = k(t, "m_LocalRotation")?;
    Ok([kf(q, "x")?, kf(q, "y")?, kf(q, "z")?, kf(q, "w")?])
}

/// The rotation-only chain Transform.rotation returns (scale ignored).
fn world_rotation(sc: &Scene, mut tid: i64) -> Result<Quat> {
    let mut q = [0.0, 0.0, 0.0, 1.0];
    let mut chain = Vec::new();
    while tid != 0 {
        chain.push(tid);
        let t = sc.transform(tid).ok_or_else(|| format!("{tid}"))?;
        tid = ki(k(t, "m_Father")?, "m_PathID")?;
    }
    for t in chain.iter().rev() {
        q = quat_mul(q, quat_of(sc.transform(*t).unwrap())?);
    }
    Ok(q)
}

fn matrix(position: [f64; 3], rotation: Quat, scale: [f64; 3]) -> M4 {
    let [x, y, z, w] = rotation;
    let r = [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - z * w),
            2.0 * (x * z + y * w),
        ],
        [
            2.0 * (x * y + z * w),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - x * w),
        ],
        [
            2.0 * (x * z - y * w),
            2.0 * (y * z + x * w),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ];
    let mut m = [[0.0; 4]; 4];
    for i in 0..3 {
        m[i] = [
            r[i][0] * scale[0],
            r[i][1] * scale[1],
            r[i][2] * scale[2],
            position[i],
        ];
    }
    m[3] = [0.0, 0.0, 0.0, 1.0];
    m
}

fn mul(a: &M4, b: &M4) -> M4 {
    let mut out = [[0.0; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            let mut s = 0.0;
            for kk in 0..4 {
                s += a[i][kk] * b[kk][j];
            }
            out[i][j] = s;
        }
    }
    out
}

fn det3(c: &[[f64; 3]; 3]) -> f64 {
    c[0][0] * (c[1][1] * c[2][2] - c[1][2] * c[2][1])
        - c[0][1] * (c[1][0] * c[2][2] - c[1][2] * c[2][0])
        + c[0][2] * (c[1][0] * c[2][1] - c[1][1] * c[2][0])
}

/// Solve `m * (x, y, z, 1) = p` for the 3x3-invertible affine `m`.
fn inverse_point(m: &M4, p: [f64; 3]) -> Result<[f64; 3]> {
    let a = [
        [m[0][0], m[0][1], m[0][2]],
        [m[1][0], m[1][1], m[1][2]],
        [m[2][0], m[2][1], m[2][2]],
    ];
    let b = [p[0] - m[0][3], p[1] - m[1][3], p[2] - m[2][3]];
    let det = det3(&a);
    if det.abs() < 1e-12 {
        return err("singular floor part transform");
    }
    let col = |j: usize| {
        let mut c = a;
        for i in 0..3 {
            c[i][j] = b[i];
        }
        det3(&c)
    };
    Ok([col(0) / det, col(1) / det, col(2) / det])
}

fn sprite_box(source: &Source, sc: &Scene, renderer: &Value) -> Result<[f64; 4]> {
    let o = u(sc.deref(k(renderer, "m_Sprite")?))?;
    let g = native_sprite_geometry(source, &o)?;
    let [mut x0, mut y0, mut x1, mut y1] = g.bounds;
    if k(renderer, "m_FlipX")?.truthy() {
        (x0, x1) = (-x0, -x1);
    }
    if k(renderer, "m_FlipY")?.truthy() {
        (y0, y1) = (-y0, -y1);
    }
    Ok([x0, y0, x1, y1])
}

struct Plank {
    parent: M4,
    parent_rotation: Quat,
    position: [f64; 3],
    rotation: Quat,
    scale: [f64; 3],
    renderer: String,
    bounds: [f64; 4],
}

/// `floor_sag(source, sc, parts)`: world quads of each sagging plank after hit 1 and hit 2.
fn floor_sag(source: &Source, sc: &Scene, parts: &[(String, i64)]) -> Result<Vec<(String, Json)>> {
    let mut state: Vec<(&str, Plank)> = Vec::new();
    for name in ["floor 1", "floor 2"] {
        let Some(gid) = child(parts, name) else {
            return err(format!("cracked floor has no '{name}' child"));
        };
        let mut renderer = Vec::new();
        for (i, kind, t) in components(sc, gid)? {
            if kind == "SpriteRenderer" && k(t, "m_Enabled")?.truthy() {
                renderer.push((i, t));
            }
        }
        if renderer.len() != 1 {
            return err(format!(
                "'{name}' carries {} sprite renderers, not one",
                renderer.len()
            ));
        }
        let tid = *sc.go_transform.get(&gid).ok_or("'gid'")?;
        let t = sc.transform(tid).ok_or("'tid'")?;
        let father = ki(k(t, "m_Father")?, "m_PathID")?;
        let p = k(t, "m_LocalPosition")?;
        let s = k(t, "m_LocalScale")?;
        let parent = if father != 0 {
            u(sc.world(father))?
        } else {
            matrix([0.0; 3], [0.0, 0.0, 0.0, 1.0], [1.0; 3])
        };
        state.push((
            name,
            Plank {
                parent,
                parent_rotation: if father != 0 {
                    world_rotation(sc, father)?
                } else {
                    [0.0, 0.0, 0.0, 1.0]
                },
                position: [kf(p, "x")?, kf(p, "y")?, kf(p, "z")?],
                rotation: quat_of(t)?,
                scale: [kf(s, "x")?, kf(s, "y")?, kf(s, "z")?],
                renderer: sc.sid(renderer[0].0),
                bounds: sprite_box(source, sc, renderer[0].1)?,
            },
        ));
    }
    let mut stages: Vec<(&str, Vec<Json>)> = state.iter().map(|(n, _)| (*n, Vec::new())).collect();
    for ops in FLOOR_SAG {
        for (name, dy, angle) in ops {
            let part = &mut state.iter_mut().find(|e| e.0 == *name).unwrap().1;
            if *dy != 0.0 {
                let world_q = quat_mul(part.parent_rotation, part.rotation);
                let delta = quat_rotate(world_q, [0.0, *dy, 0.0]);
                let v = [part.position[0], part.position[1], part.position[2], 1.0];
                let mut world = [0.0; 3];
                for i in 0..3 {
                    let mut s = 0.0;
                    for (j, vj) in v.iter().enumerate() {
                        s += part.parent[i][j] * vj;
                    }
                    world[i] = s;
                }
                for i in 0..3 {
                    world[i] += delta[i];
                }
                part.position = inverse_point(&part.parent, world)?;
            }
            if *angle != 0.0 {
                part.rotation = quat_mul(part.rotation, euler_z(*angle));
            }
        }
        for (name, part) in &state {
            let m = mul(
                &part.parent,
                &matrix(part.position, part.rotation, part.scale),
            );
            let [x0, y0, x1, y1] = part.bounds;
            let quad: Vec<Json> = [(x0, y1), (x1, y1), (x0, y0), (x1, y0)]
                .iter()
                .map(|(x, y)| {
                    jfloats(&[
                        m[0][0] * x + m[0][1] * y + m[0][3],
                        m[1][0] * x + m[1][1] * y + m[1][3],
                    ])
                })
                .collect();
            stages
                .iter_mut()
                .find(|e| e.0 == *name)
                .unwrap()
                .1
                .push(jl(quad));
        }
    }
    Ok(state
        .iter()
        .map(|(name, part)| {
            let s = stages.iter().find(|e| e.0 == *name).unwrap().1.clone();
            (
                name.to_string(),
                jobj(vec![("renderer", js(&part.renderer)), ("stages", jl(s))]),
            )
        })
        .collect())
}

// --------------------------------------------------------------------- records

fn sorted_sids(sc: &Scene, gids: &BTreeSet<i64>) -> Json {
    let mut v: Vec<String> = gids.iter().map(|g| sc.sid(*g)).collect();
    v.sort();
    jstrs(&v)
}

/// `_wall(source, sc, record)`.
fn wall(sc: &Scene, record: &Json) -> Result<Json> {
    let gid = jint_of(record, "gid");
    let family = if jget(record, "definition").map(jstr).as_deref() == Some("breakable_wall_v2") {
        FAMILY_WALL
    } else {
        FAMILY_WALL_TK2D
    };
    let subtree: BTreeSet<i64> = descendants(sc, gid)?.into_iter().collect();
    let mut colliders = vec![jget(record, "collider_source")
        .cloned()
        .unwrap_or(Json::Null)];
    let mut locks: Vec<Json> = Vec::new();
    if family == FAMILY_WALL {
        let kids = children(sc, gid)?;
        if let Some(camera) = child(&kids, "Camera Locks") {
            let under: BTreeSet<i64> = descendants(sc, camera)?.into_iter().collect();
            colliders.extend(solid_colliders(sc, &under)?);
            locks.extend(camera_locks(sc, &under)?);
        }
    }
    let facing = match jget(record, "facing") {
        Some(Json::Int(i)) => *i,
        Some(Json::Float(f)) => *f as i64,
        _ => 0,
    };
    let renderer_source = jget(record, "renderer_source")
        .cloned()
        .unwrap_or(Json::Null);
    let recoil = WALL_RECOIL[facing as usize];
    let origin = point(sc, gid)?;
    let uncovers: Vec<Json> = match jget(record, "uncovers") {
        Some(Json::List(l)) => l
            .iter()
            .map(|e| jget(e, "game_object").cloned().unwrap_or(Json::Null))
            .collect(),
        _ => vec![],
    };
    Ok(jobj(vec![
        ("family", ji(family)),
        (
            "hits",
            jget(record, "nail_hits").cloned().unwrap_or(Json::Null),
        ),
        (
            "facing",
            jget(record, "facing").cloned().unwrap_or(Json::Null),
        ),
        ("spell", jb(true)),
        ("hero_range", Json::Null),
        ("lockout_ticks", ji(WALL_LOCKOUT_TICKS)),
        ("off_renderer_sources", jl(vec![renderer_source.clone()])),
        (
            "moving",
            jl(vec![jobj(vec![
                ("renderer", renderer_source.clone()),
                ("recoil", jl(vec![ji(recoil[0]), ji(recoil[1])])),
            ])]),
        ),
        ("recoil_ticks", ji(WALL_RECOIL_TICKS)),
        ("recoil_units", jf(WALL_RECOIL_UNITS)),
        ("collider_sources", jl(colliders)),
        ("camera_lock_sources", jl(locks)),
        ("strike_origin", jfloats(&origin[..2])),
        ("uncovers", jl(uncovers)),
        (
            "renderer_type",
            jget(record, "renderer_type").cloned().unwrap_or(Json::Null),
        ),
        ("renderer_source", renderer_source),
        ("subtree", sorted_sids(sc, &subtree)),
    ]))
}

fn jint_of(record: &Json, key: &str) -> i64 {
    match jget(record, key) {
        Some(Json::Int(i)) => *i,
        _ => 0,
    }
}

/// `_floor(source, sc, record)`.
fn floor(source: &Source, sc: &Scene, record: &Json) -> Result<Json> {
    let gid = jint_of(record, "gid");
    let family =
        if jget(record, "hit_gate").map(jstr).as_deref() == Some("Hero Range and attack type") {
            FAMILY_FLOOR
        } else {
            FAMILY_FLOOR_OPEN
        };
    let parts = children(sc, gid)?;
    let missing: Vec<&str> = FLOOR_PARTS
        .iter()
        .copied()
        .filter(|n| child(&parts, n).is_none())
        .collect();
    if !missing.is_empty() {
        let shown: Vec<String> = missing.iter().map(|n| breakables::pystr(n)).collect();
        return err(format!("cracked floor lacks [{}]", shown.join(", ")));
    }
    let mut hidden: BTreeSet<i64> = BTreeSet::new();
    for name in FLOOR_PARTS {
        hidden.extend(descendants(sc, child(&parts, name).unwrap())?);
    }
    let mut hero_range = Json::Null;
    if family == FAMILY_FLOOR {
        let Some(detector) = child(&children(sc, gid)?, "Hero Range") else {
            return err("cracked floor has no Hero Range child");
        };
        let mut boxes: Vec<Comp> = Vec::new();
        for c in components(sc, detector)? {
            if c.1 == "BoxCollider2D" && k(c.2, "m_IsTrigger")?.truthy() {
                boxes.push(c);
            }
        }
        if boxes.len() != 1 {
            return err("Hero Range needs exactly one trigger box");
        }
        let polys = collider_polygons(sc, detector, "BoxCollider2D", boxes[0].2)?;
        let points: Vec<(f64, f64)> = polys.iter().flatten().copied().collect();
        hero_range = jfloats(&box_of(&points));
    }
    let sag = floor_sag(source, sc, &parts)?;
    let mut masks: Vec<Json> = Vec::new();
    if let Some(mask) = child(&parts, FLOOR_MASK_CHILD) {
        if sc.active(mask) {
            masks.push(Json::Str(sc.sid(mask)));
        }
    }
    let origin = point(sc, gid)?;
    let moving: Vec<Json> = ["floor 1", "floor 2"]
        .iter()
        .map(|name| {
            let s = &sag.iter().find(|e| e.0 == *name).unwrap().1;
            jobj(vec![
                ("renderer", jget(s, "renderer").cloned().unwrap()),
                ("stages", jget(s, "stages").cloned().unwrap()),
            ])
        })
        .collect();
    let subtree: BTreeSet<i64> = descendants(sc, gid)?.into_iter().collect();
    let with_masks = !masks.is_empty();
    Ok(jobj(vec![
        ("family", ji(family)),
        ("hits", ji(breakables::CRACKED_FLOOR_NAIL_HITS)),
        ("facing", ji(0)),
        ("spell", jb(false)),
        ("hero_range", hero_range),
        ("lockout_ticks", ji(FLOOR_LOCKOUT_TICKS)),
        ("off_renderer_sources", jl(renderers(sc, &hidden)?)),
        ("moving", jl(moving)),
        (
            "collider_sources",
            jget(record, "solid_collider_sources")
                .cloned()
                .unwrap_or(jl(vec![])),
        ),
        ("camera_lock_sources", jl(camera_locks(sc, &hidden)?)),
        (
            "strike_origin",
            jfloats(&[
                origin[0] + FLOOR_STRIKE_OFFSET[0],
                origin[1] + FLOOR_STRIKE_OFFSET[1],
            ]),
        ),
        ("uncovers", jl(masks)),
        (
            "mask_fade_seconds",
            if with_masks {
                jf(FLOOR_MASK_FADE_SECONDS)
            } else {
                Json::Null
            },
        ),
        ("subtree", sorted_sids(sc, &subtree)),
    ]))
}

/// `secret_states(sc)`: FSM source -> state index for every hidden wall and cracked floor.
pub fn secret_states(sc: &Scene) -> Result<Vec<(String, i64)>> {
    let mut found: Vec<String> = Vec::new();
    let mut objects: Vec<&hk_unity::scene::SceneObject> = sc.objects.iter().collect();
    objects.sort_by_key(|o| o.id);
    for o in objects {
        if o.typename != "PlayMakerFSM" || !k(&o.tree, "m_Enabled")?.truthy() {
            continue;
        }
        let gid = ki(k(&o.tree, "m_GameObject")?, "m_PathID")?;
        if sc.go(gid).is_none() || !sc.active(gid) {
            continue;
        }
        let fsm = k(&o.tree, "fsm")?;
        let is_secret = (|| -> Result<bool> {
            Ok(breakables::hidden_wall_shape(fsm)?.is_some()
                || breakables::cracked_floor_shape(fsm)?.is_some())
        })();
        if let Ok(true) = is_secret {
            found.push(sc.sid(o.id));
        }
    }
    let mut breakable_count = 0usize;
    for o in &sc.base.objects {
        if o.class_id == 114 {
            let obj = hk_unity::Obj {
                file: sc.base.clone(),
                info: *o,
            };
            if u(sc.source.typename(&obj))? == "Breakable" {
                breakable_count += 1;
            }
        }
    }
    if breakable_count + found.len() > breakables::MAX_SCENE_BREAKABLES {
        return err(format!(
            "secret states collide with Breakable ordinals: {breakable_count} + {}",
            found.len()
        ));
    }
    Ok(found
        .into_iter()
        .enumerate()
        .map(|(n, sid)| (sid, TOP_STATE - n as i64))
        .collect())
}

/// `secret_drivers(sc)`: mask GameObject -> the state of the secret whose break uncovers it.
pub fn secret_drivers(sc: &Scene) -> Result<HashMap<i64, i64>> {
    let states = secret_states(sc)?;
    let state_of = |sid: &str| states.iter().find(|e| e.0 == sid).map(|e| e.1);
    let mut out: HashMap<i64, i64> = HashMap::new();
    for (target, driver) in breakables::uncover_drivers(sc)? {
        if let Some(s) = jget(&driver, "source").map(jstr).and_then(|d| state_of(&d)) {
            out.insert(target, s);
        }
    }
    let mut objects: Vec<&hk_unity::scene::SceneObject> = sc.objects.iter().collect();
    objects.sort_by_key(|o| o.id);
    for o in objects {
        if o.typename != "PlayMakerFSM" {
            continue;
        }
        let Some(state) = state_of(&sc.sid(o.id)) else {
            continue;
        };
        let fsm = k(&o.tree, "fsm")?;
        match breakables::cracked_floor_shape(fsm) {
            Ok(Some(_)) => {}
            _ => continue,
        }
        let gid = ki(k(&o.tree, "m_GameObject")?, "m_PathID")?;
        if let Some(mask) = child(&children(sc, gid)?, FLOOR_MASK_CHILD) {
            if sc.active(mask) {
                out.insert(mask, state);
            }
        }
    }
    Ok(out)
}

/// `secret_sources(source, sc, bounds, errors)`: every hidden wall and cracked floor
/// of the scene, as a secret record.
pub fn secret_sources(
    source: &Source,
    sc: &Scene,
    bounds: Option<[f64; 4]>,
    mut errors: Option<&mut Vec<Json>>,
) -> Result<Vec<Json>> {
    let mut found: Vec<Json> = Vec::new();
    for pass in 0..2 {
        let mut refused: Vec<Json> = Vec::new();
        let records = if pass == 0 {
            breakables::hidden_walls(sc, Some(&mut refused), false, None)?
        } else {
            breakables::cracked_floors(sc, Some(&mut refused), false)?
        };
        for record in records {
            let built = if pass == 0 {
                wall(sc, &record)
            } else {
                floor(source, sc, &record)
            };
            let mut secret = match built {
                Ok(s) => s,
                Err(error) => {
                    refused.push(jobj(vec![
                        ("id", jget(&record, "source").cloned().unwrap_or(Json::Null)),
                        ("type", js("secret")),
                        ("error", Json::Str(error)),
                    ]));
                    continue;
                }
            };
            let mut persist = Vec::new();
            for (i, kind, t) in components(sc, jint_of(&record, "gid"))? {
                if kind == "PersistentBoolItem" {
                    persist.push(jobj(vec![
                        ("source", Json::Str(sc.sid(i))),
                        ("semi_persistent", jb(k(t, "semiPersistent")?.truthy())),
                        ("dont_save", jb(k(t, "dontSave")?.truthy())),
                    ]));
                }
            }
            for key in [
                "source",
                "game_object",
                "name",
                "definition",
                "fsm_sha256",
                "hit_polygons",
                "box",
            ] {
                jset(
                    &mut secret,
                    key,
                    jget(&record, key).cloned().unwrap_or(Json::Null),
                );
            }
            jset(&mut secret, "persistence", jl(persist));
            found.push(secret);
        }
        if let Some(list) = errors.as_deref_mut() {
            list.extend(refused);
        }
    }
    let states = secret_states(sc)?;
    for secret in &mut found {
        let sid = jget(secret, "source").map(jstr).unwrap_or_default();
        let state = states
            .iter()
            .find(|e| e.0 == sid)
            .map(|e| e.1)
            .ok_or_else(|| format!("'{sid}'"))?;
        jset(secret, "state_index", ji(state));
    }
    found.sort_by_key(|s| -jint_of(s, "state_index"));
    let Some(b) = bounds else { return Ok(found) };
    let touches = |j: Option<&Json>| -> bool {
        let Some(Json::List(l)) = j else { return false };
        if l.len() < 4 {
            return false;
        }
        let f = |i: usize| match &l[i] {
            Json::Float(f) => *f,
            Json::Int(i) => *i as f64,
            _ => f64::NAN,
        };
        !(f(2) < b[0] || f(0) > b[2] || f(3) < b[1] || f(1) > b[3])
    };
    Ok(found
        .into_iter()
        .filter(|s| touches(jget(s, "box")) || touches(jget(s, "hero_range")))
        .collect())
}

/// `bind_secrets(records, draws, edges)`: region draw and edge indices for each secret.
pub fn bind_secrets(
    records: &[Json],
    draw_sources: &[String],
    edge_sources: &[String],
) -> Vec<Json> {
    let mut draw_ids: HashMap<&str, usize> = HashMap::new();
    for (i, s) in draw_sources.iter().enumerate() {
        draw_ids.entry(s).or_insert(i);
    }
    let strs = |j: Option<&Json>| -> Vec<String> {
        match j {
            Some(Json::List(l)) => l.iter().map(jstr).collect(),
            _ => vec![],
        }
    };
    records
        .iter()
        .map(|record| {
            let mut bound = record.clone();
            let off = strs(jget(record, "off_renderer_sources"));
            let colliders = strs(jget(record, "collider_sources"));
            jset(
                &mut bound,
                "off_draws",
                jl(off
                    .iter()
                    .filter_map(|s| draw_ids.get(s.as_str()).map(|&i| ji(i as i64)))
                    .collect()),
            );
            jset(
                &mut bound,
                "edge_indices",
                jl(edge_sources
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| colliders.contains(s))
                    .map(|(i, _)| ji(i as i64))
                    .collect()),
            );
            let moving: Vec<Json> = match jget(record, "moving") {
                Some(Json::List(l)) => l
                    .iter()
                    .map(|m| {
                        let mut m = m.clone();
                        let r = jget(&m, "renderer").map(jstr).unwrap_or_default();
                        jset(
                            &mut m,
                            "draw",
                            draw_ids
                                .get(r.as_str())
                                .map_or(Json::Null, |&i| ji(i as i64)),
                        );
                        m
                    })
                    .collect(),
                _ => vec![],
            };
            jset(&mut bound, "moving_draws", jl(moving));
            bound
        })
        .collect()
}

/// Keeps the helpers `secret_sources` shares with its callers in use.
#[allow(dead_code)]
fn _shared() {
    let _ = (file_of, local_id, polygons_json, value_json);
}
