//! The scenery layer of host/cook.py: how a sprite becomes a draw of the room's
//! atlas, which renderers a scene leaves out, and the tk2d decor and wall draws.
//!
//! Everything here is a function of one scene plus the atlas being filled; the
//! room-level `cook()` that calls it stays in Python until its other callees are
//! ported.

use crate::atlas::Atlas;
use crate::common::{err, f64_of, get, int_of, py_round, Result};
use crate::cook::{focal, tk_sprite, CAM_Z};
use crate::cook_audio::u;
use crate::pyjson::Json;
use crate::quantize::atlas_quantizer;
use hk_pil::resample::Filter;
use hk_pil::{Image, Mode};
use hk_unity::scene::Scene;
use hk_unity::{Obj, Source, Value};
use std::collections::{BTreeSet, HashMap};

/// Euclidean distance as CPython's `math.dist` takes it (hypot of the differences).
fn dist(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    crate::pyfloat::hypot_n(&[a[0] - b[0], a[1] - b[1], a[2] - b[2]])
}

/// quality.py: pages a view's static textures may take.
pub const STATIC_PAGE_BUDGET: usize = 19;
/// quality.py: CLUT slots per view (four disjoint banks).
pub const TEXTURE_BUDGET: usize = 416;
/// quality.py: the long-axis texel cap of a scenery texture.
pub const SCENERY_TEXEL_CAP: i64 = 252;

/// `SCENERY_SCENE_CAPS.get(scene_name, SCENERY_TEXEL_CAP)`.
pub fn scenery_cap(scene_name: &str) -> i64 {
    match scene_name {
        "Tutorial_01" => 96,
        "Town" => 160,
        "Crossroads_50" | "Fungus1_10" => 48,
        _ => SCENERY_TEXEL_CAP,
    }
}

/// `scenery_dimensions(width, height, cap)`: one stable integer sampling size.
pub fn scenery_dimensions(width: f64, height: f64, cap: i64) -> Result<(usize, usize)> {
    if !(1..=252).contains(&cap) || ![width, height].iter().all(|v| v.is_finite() && *v > 0.0) {
        return err("Invalid scenery dimensions/cap");
    }
    let reduction = f64::min(1.0, cap as f64 / f64::max(width, height));
    let one = |v: f64| (cap as f64).min((v * reduction).ceil()).max(1.0) as usize;
    Ok((one(width), one(height)))
}

/// `native_draw_range(scale, points)`: the reason a draw would fail hk-format's
/// Geometry check, or None when it is admissible.
pub fn native_draw_range(scale: f64, points: &[[f64; 3]]) -> Option<String> {
    let q12 = py_round(scale * 4096.0);
    if !(1..=262144).contains(&q12) {
        return Some(format!(
            "draw scale {q12}/4096 outside native range 1..262144"
        ));
    }
    if points
        .iter()
        .any(|p| (0..2).any(|k| py_round(p[k] * scale * 256.0).abs() > 8_000_000))
    {
        return Some("draw coordinate exceeds native Q24.8 range".to_string());
    }
    None
}

const EDGE_SNAP: f64 = 0.005;

/// A terrain segment as the cook stores it.
pub type Segment = ([f64; 2], [f64; 2]);

/// `cooked_edge(a, b, region)`: one source terrain segment as the cook stores
/// it (clip to the collision bounds, refuse a slope, snap a near-axis edge), or
/// why it is refused.
pub fn cooked_edge(
    a: [f64; 2],
    b: [f64; 2],
    collision_bounds: [f64; 4],
    allow_slopes: bool,
) -> std::result::Result<Segment, &'static str> {
    let bounds = collision_bounds;
    if f64::max(a[0], b[0]) < bounds[0]
        || f64::min(a[0], b[0]) > bounds[2]
        || f64::max(a[1], b[1]) < bounds[1]
        || f64::min(a[1], b[1]) > bounds[3]
    {
        return Err("outside collision bounds");
    }
    if (a[0] - b[0]).abs() > EDGE_SNAP && (a[1] - b[1]).abs() > EDGE_SNAP && !allow_slopes {
        return Err("sloped terrain edge");
    }
    let mut b = b;
    if (a[0] - b[0]).abs() <= EDGE_SNAP {
        b[0] = a[0];
    } else if (a[1] - b[1]).abs() <= EDGE_SNAP {
        b[1] = a[1];
    }
    Ok((a, b))
}

/// `unsupported_sprite_behavior(name, parent_name, component_types)`.
pub fn unsupported_sprite_behavior(
    name: &str,
    parent_name: &str,
    component_types: &[String],
) -> Option<&'static str> {
    // The named scene helper is driven by a PlayMaker fade/cover state machine;
    // its ordinary material is also used by valid scenery, so the material must
    // never be the exclusion rule.
    if name == "Inverse Remasker"
        && parent_name == "mask_container"
        && component_types.iter().any(|t| t == "PlayMakerFSM")
    {
        return Some("FSM-controlled inverse remasker");
    }
    None
}

/// `black_member_image(image, renderer)`: a reveal member drawn black, as the
/// texels it really shows, or None for a member that keeps its colours.
pub fn black_member_image(image: &Image, color: [f64; 3]) -> Option<Image> {
    let rgba = image.to_rgba();
    if !(color[0] == 0.0 && color[1] == 0.0 && color[2] == 0.0) {
        let visible: Vec<&[u8]> = rgba.data.chunks_exact(4).filter(|p| p[3] >= 16).collect();
        let brightest = visible.iter().map(|p| p[0].max(p[1]).max(p[2])).max();
        if brightest.is_none_or(|m| m >= 8) {
            return None;
        }
    }
    let mut black = Image::new(Mode::Rgba, rgba.width, rgba.height);
    for (out, p) in black
        .data
        .chunks_exact_mut(4)
        .zip(rgba.data.chunks_exact(4))
    {
        out[3] = p[3];
    }
    Some(black)
}

/// MonoBehaviours that switch their own GameObject off shortly after it is enabled.
const SELF_DISABLING_EFFECTS: [&str; 1] = ["WaveEffectControl"];

fn components_of(sc: &Scene, gid: i64) -> Vec<i64> {
    sc.go(gid)
        .and_then(|g| g.get("m_Component"))
        .and_then(Value::list)
        .unwrap_or(&[])
        .iter()
        .filter_map(|c| {
            c.get("component")
                .and_then(|p| p.get("m_PathID"))
                .and_then(Value::int)
        })
        .collect()
}

/// `self_disabling_effect(sc, gid)`.
pub fn self_disabling_effect(sc: &Scene, gid: i64) -> bool {
    components_of(sc, gid).into_iter().any(|id| {
        sc.object(id)
            .is_some_and(|o| SELF_DISABLING_EFFECTS.contains(&o.typename.as_str()))
    })
}

fn jstr(s: &str) -> Json {
    Json::Str(s.to_string())
}

fn jobj(fields: Vec<(&str, Json)>) -> Json {
    Json::Obj(
        fields
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    )
}

/// `unsupported_remasker(s, sc, gid)`: the explicit omission record of a
/// FSM-controlled inverse remasker, or None.
pub fn unsupported_remasker(source: &Source, sc: &Scene, gid: i64) -> Result<Option<Json>> {
    let g = sc.go(gid).ok_or("no such GameObject")?;
    let tid = *sc
        .go_transform
        .get(&gid)
        .ok_or("GameObject without transform")?;
    let father = get(
        get(sc.transform(tid).ok_or("no transform")?, "m_Father")?,
        "m_PathID",
    )?
    .int()
    .unwrap_or(0);
    let parent = if father != 0 {
        let t = sc.transform(father).ok_or("parent is not a transform")?;
        let pg = get(get(t, "m_GameObject")?, "m_PathID")?.int().unwrap_or(0);
        get(sc.go(pg).ok_or("parent has no GameObject")?, "m_Name")?
            .str()
            .unwrap_or_default()
    } else {
        String::new()
    };
    let mut components: Vec<Obj> = Vec::new();
    for c in get(g, "m_Component")?.list().unwrap_or(&[]) {
        components.push(u(sc.deref(get(c, "component")?))?);
    }
    let mut names = Vec::new();
    for c in &components {
        names.push(u(source.typename(c))?);
    }
    let name = get(g, "m_Name")?.str().unwrap_or_default();
    let Some(kind) = unsupported_sprite_behavior(&name, &parent, &names) else {
        return Ok(None);
    };
    // SpriteRenderer's class id.
    let render = components
        .iter()
        .find(|c| c.class_id() == 212)
        .ok_or("no SpriteRenderer")?;
    let r = u(source.read(render))?;
    let mut materials = Vec::new();
    for reference in get(&r, "m_Materials")?.list().unwrap_or(&[]) {
        let mo = u(sc.deref(reference))?;
        let m = u(source.read(&mo))?;
        let so = u(source.deref(&mo.file, get(&m, "m_Shader")?))?;
        let shader = u(source.read(&so))?;
        let shader_name = shader
            .get("m_ParsedForm")
            .and_then(|f| f.get("m_Name"))
            .and_then(Value::str)
            .or_else(|| shader.get("m_Name").and_then(Value::str))
            .unwrap_or_default();
        materials.push(jobj(vec![
            ("id", Json::Str(mo.sid())),
            (
                "name",
                Json::Str(get(&m, "m_Name")?.str().unwrap_or_default()),
            ),
            ("shader_id", Json::Str(so.sid())),
            ("shader_name", Json::Str(shader_name)),
        ]));
    }
    let controllers: Vec<Json> = components
        .iter()
        .zip(&names)
        .filter(|(_, n)| n.as_str() == "PlayMakerFSM" || n.as_str() == "PersistentBoolItem")
        .map(|(c, _)| Json::Str(c.sid()))
        .collect();
    let sprite = u(sc.deref(get(&r, "m_Sprite")?))?;
    Ok(Some(jobj(vec![
        ("id", Json::Str(render.sid())),
        ("type", jstr(kind)),
        (
            "game_object",
            Json::Str(format!("{}:{gid}", hk_unity::base_name(&sc.base.name))),
        ),
        ("name", Json::Str(name)),
        ("parent", Json::Str(parent)),
        ("sprite", Json::Str(sprite.sid())),
        ("materials", Json::List(materials)),
        (
            "mask_interaction",
            Json::Int(get(&r, "m_MaskInteraction")?.int().unwrap_or(0)),
        ),
        ("controllers", Json::List(controllers)),
        (
            "error",
            jstr("Omitted: authored cover/fade state and persistent activation are unsupported; rendering this helper as static scenery produces an opaque screen cover."),
        ),
    ])))
}

/// A sprite's rect and mesh box: `native_sprite_geometry`'s (sprite, box).
#[derive(Clone, Debug)]
pub struct SpriteGeometry {
    pub width: f64,
    pub height: f64,
    pub bounds: [f64; 4],
}

/// `native_sprite_geometry(o)`.
pub fn native_sprite_geometry(source: &Source, o: &Obj) -> Result<SpriteGeometry> {
    let sp = u(source.read(o))?;
    let mesh = u(hk_unity::texture::sprite_mesh(get(&sp, "m_RD")?))?;
    if mesh.vertices.is_empty() {
        return err("sprite has no mesh vertices");
    }
    let xs = mesh.vertices.iter().map(|v| v[0] as f64);
    let ys = mesh.vertices.iter().map(|v| v[1] as f64);
    let rect = get(&sp, "m_Rect")?;
    Ok(SpriteGeometry {
        width: f64_of(rect, "width")?,
        height: f64_of(rect, "height")?,
        bounds: [
            crate::common::py_min(xs.clone()),
            crate::common::py_min(ys.clone()),
            crate::common::py_max(xs),
            crate::common::py_max(ys),
        ],
    })
}

/// A scene sprite and the renderer alpha it is drawn at (the alpha as its bits).
pub type TexelKey = (String, u64);

/// `scene_sprite_texels(s, sc, cap, shared_geometry)`: (sprite id, renderer
/// alpha) -> the one texel size its scenery texture has in this scene.
pub fn scene_sprite_texels(
    source: &Source,
    sc: &Scene,
    cap: i64,
    shared_geometry: &mut HashMap<String, SpriteGeometry>,
) -> Result<Vec<(TexelKey, (usize, usize))>> {
    let focal = focal();
    let mut out: Vec<(TexelKey, (usize, usize))> = Vec::new();
    let mut at: HashMap<TexelKey, usize> = HashMap::new();
    for o in &sc.objects {
        if o.typename != "SpriteRenderer" {
            continue;
        }
        let t = &o.tree;
        let sprite = get(t, "m_Sprite")?;
        if get(sprite, "m_PathID")?.int().unwrap_or(0) == 0
            || get(t, "m_DrawMode")?.int().unwrap_or(0) != 0
        {
            continue;
        }
        let gid = get(get(t, "m_GameObject")?, "m_PathID")?.int().unwrap_or(0);
        // A renderer whose transform chain leaves the scene file has no world
        // position to project; no view draws it either.
        let Ok(origin) = sc.point(gid, 0.0, 0.0, 0.0) else {
            continue;
        };
        let z = origin[2];
        if z <= CAM_Z + 2.0 {
            continue;
        }
        let Ok(obj) = sc.deref(sprite) else { continue };
        let sid = obj.sid();
        if !shared_geometry.contains_key(&sid) {
            match native_sprite_geometry(source, &obj) {
                Ok(g) => {
                    shared_geometry.insert(sid.clone(), g);
                }
                Err(_) => continue,
            }
        }
        let g = &shared_geometry[&sid];
        let [x0, y0, x1, y1] = g.bounds;
        let mut points = Vec::new();
        for (x, y) in [(x0, y1), (x1, y1), (x0, y0)] {
            points.push(u(sc.point(gid, x, y, 0.0))?);
        }
        let scale = focal / (z - CAM_Z);
        let w = dist(&points[0], &points[1]) * scale;
        let h = dist(&points[0], &points[2]) * scale;
        if w < 0.5 || h < 0.5 {
            continue;
        }
        let full = scenery_dimensions(w, h, cap)?;
        let k = f64::min(
            1.0,
            f64::min(g.width / full.0 as f64, g.height / full.1 as f64),
        );
        let full = (
            ((full.0 as f64 * k).ceil() as usize).max(1),
            ((full.1 as f64 * k).ceil() as usize).max(1),
        );
        let alpha = f64_of(get(t, "m_Color")?, "a")?;
        let key = (sid, alpha.to_bits());
        match at.get(&key) {
            Some(&i) => {
                let old = out[i].1;
                out[i].1 = (old.0.max(full.0), old.1.max(full.1));
            }
            None => {
                at.insert(key.clone(), out.len());
                out.push((key, full));
            }
        }
    }
    Ok(out)
}

/// A scenery texture of the atlas: sprite id, sampled size, renderer alpha, cap.
pub type TextureKey = (String, usize, usize, u64, i64);

/// `scenery_texture(...)`: the atlas index of one sprite sampled at `target`
/// (or its natural size under `cap`), with the sampled pixels cached in
/// `shared_pixels`.
#[allow(clippy::too_many_arguments)]
pub fn scenery_texture(
    atlas: &mut Atlas,
    images: &mut HashMap<TextureKey, usize>,
    shared_pixels: &mut HashMap<TextureKey, Image>,
    sid: &str,
    image: impl FnOnce() -> Result<Image>,
    width: f64,
    height: f64,
    alpha: f64,
    cap: i64,
    target: Option<(usize, usize)>,
) -> Result<(usize, TextureKey)> {
    let target = match target {
        Some(t) => t,
        None => scenery_dimensions(width, height, cap)?,
    };
    let key: TextureKey = (sid.to_string(), target.0, target.1, alpha.to_bits(), cap);
    if !images.contains_key(&key) {
        if !shared_pixels.contains_key(&key) {
            let mut im = image()?.to_rgba();
            for p in im.data.chunks_exact_mut(4) {
                p[3] = py_round(p[3] as f64 * alpha) as u8;
            }
            shared_pixels.insert(key.clone(), im.resize(target.0, target.1, Filter::Lanczos));
        }
        let index = atlas.add(
            &shared_pixels[&key],
            target.0 as f64,
            target.1 as f64,
            false,
            &atlas_quantizer,
        )?;
        images.insert(key.clone(), index);
    }
    Ok((images[&key], key))
}

/// One scenery draw.
#[derive(Clone, Debug, PartialEq)]
pub struct Draw {
    pub source: String,
    pub sprite: String,
    pub name: String,
    pub texture: usize,
    pub points: Vec<[f64; 3]>,
    pub scale: f64,
    pub tint: [i64; 3],
    pub z: f64,
    pub order: i64,
    pub layer: i64,
}

fn tint_of(color: &Value) -> Result<[i64; 3]> {
    let one = |c: &str| -> Result<i64> { Ok(py_round(f64_of(color, c)? * 128.0).min(255)) };
    Ok([one("r")?, one("g")?, one("b")?])
}

/// The projection constants every draw of a view shares.
pub struct View {
    /// The camera range test: ((x0, x1), (y0, y1)).
    pub cull: Option<((f64, f64), (f64, f64))>,
    pub cap: i64,
}

fn out_of_view(cull: &((f64, f64), (f64, f64)), points: &[[f64; 3]], scale: f64) -> bool {
    let ((cx0, cx1), (cy0, cy1)) = *cull;
    let xs = points.iter().map(|p| p[0]);
    let ys = points.iter().map(|p| p[1]);
    crate::common::py_max(xs.clone()) < cx0 - 160.0 / scale
        || crate::common::py_min(xs) > cx1 + 160.0 / scale
        || crate::common::py_max(ys.clone()) < cy0 - 120.0 / scale
        || crate::common::py_min(ys) > cy1 + 120.0 / scale
}

/// The caches a room's scenery draws share.
#[derive(Default)]
pub struct SceneryCaches {
    pub images: HashMap<TextureKey, usize>,
    pub shared_pixels: HashMap<TextureKey, Image>,
}

/// `tk2d_wall_draw(...)`: `Break Wall 2`'s tk2dSprite as one scenery draw, or
/// None when the camera-range test (`view.cull`) drops it.
pub fn tk2d_wall_draw(
    source: &Source,
    sc: &Scene,
    renderer_source: &str,
    atlas: &mut Atlas,
    caches: &mut SceneryCaches,
    view: &View,
) -> Result<Option<Draw>> {
    let rid: i64 = renderer_source
        .split(':')
        .nth(1)
        .and_then(|v| v.parse().ok())
        .ok_or("bad renderer source")?;
    let object = sc.object(rid).ok_or("no such renderer")?;
    if object.typename != "tk2dSprite" {
        return err("tk2d wall renderer is not a tk2dSprite");
    }
    let sprite = &object.tree;
    let gid = get(get(sprite, "m_GameObject")?, "m_PathID")?
        .int()
        .unwrap_or(0);
    let renderer = components_of(sc, gid)
        .into_iter()
        .filter_map(|id| sc.object(id))
        .find(|o| o.typename == "MeshRenderer")
        .map(|o| &o.tree);
    let Some(renderer) = renderer.filter(|r| get(r, "m_Enabled").is_ok_and(Value::truthy)) else {
        return err("tk2d wall has no enabled MeshRenderer");
    };
    let collection_o = u(sc.deref(get(sprite, "collection")?))?;
    let collection = u(source.read(&collection_o))?;
    let sprite_id = int_of(sprite, "_spriteId")?;
    let (image, bounds) = tk_sprite(
        source,
        &collection_o.file,
        &collection,
        sprite_id as usize,
        &mut HashMap::new(),
    )?;
    let k = get(sprite, "_scale")?;
    let (kx, ky) = (f64_of(k, "x")?, f64_of(k, "y")?);
    let (x0, y0, x1, y1) = (
        bounds[0] * kx,
        bounds[1] * ky,
        bounds[2] * kx,
        bounds[3] * ky,
    );
    let mut points = Vec::new();
    for (x, y) in [(x0, y1), (x1, y1), (x0, y0), (x1, y0)] {
        points.push(u(sc.point(gid, x, y, 0.0))?);
    }
    let z = points[0][2];
    let scale = focal() / (z - CAM_Z);
    if let Some(cull) = &view.cull {
        // The camera-range test every SpriteRenderer of a view passes.
        if out_of_view(cull, &points, scale) {
            return Ok(None);
        }
    }
    let w = dist(&points[0], &points[1]) * scale;
    let h = dist(&points[0], &points[2]) * scale;
    if let Some(reason) = native_draw_range(scale, &points) {
        return err(reason);
    }
    let color = get(sprite, "_color")?;
    let sprite_sid = format!("{}:{sprite_id}", collection_o.sid());
    let (texture, _) = scenery_texture(
        atlas,
        &mut caches.images,
        &mut caches.shared_pixels,
        &sprite_sid,
        || Ok(image),
        w,
        h,
        f64_of(color, "a")?,
        view.cap,
        None,
    )?;
    Ok(Some(Draw {
        source: renderer_source.to_string(),
        sprite: sprite_sid,
        name: get(sc.go(gid).ok_or("no GameObject")?, "m_Name")?
            .str()
            .unwrap_or_default(),
        texture,
        points,
        scale,
        tint: tint_of(color)?,
        z,
        order: int_of(renderer, "m_SortingOrder")?,
        layer: int_of(renderer, "m_SortingLayer")?,
    }))
}

/// Components that say nothing about what drives an object.
const DECOR_PLAIN: [&str; 27] = [
    "GameObject",
    "Transform",
    "SpriteRenderer",
    "MeshRenderer",
    "MeshFilter",
    "tk2dSprite",
    "tk2dSpriteAnimator",
    "Animator",
    "PlayFromRandomFrameMecanim",
    "SetZ",
    "SetZRandom",
    "AudioSource",
    "BoxCollider2D",
    "CircleCollider2D",
    "PolygonCollider2D",
    "EdgeCollider2D",
    "Rigidbody2D",
    "ParticleSystem",
    "ParticleSystemRenderer",
    "ParticleSystemAutoRecycle",
    "ParticleSystemCollisionLagFix",
    "ReduceParticleEffects",
    "NonBouncer",
    "NonThunker",
    "SpriteFlash",
    "ObjectBounce",
    "SpinSelfSimple",
];

/// `decor_behaviours(sc, gid)`: behaviours on an object and its ancestors,
/// script names, FSMs by name.
pub fn decor_behaviours(sc: &Scene, mut gid: i64) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    while sc.gos.contains_key(&gid) {
        for id in components_of(sc, gid) {
            let Some(o) = sc.object(id) else { continue };
            if DECOR_PLAIN.contains(&o.typename.as_str()) {
                continue;
            }
            if o.typename == "PlayMakerFSM" {
                let name = o
                    .tree
                    .get("fsm")
                    .and_then(|f| f.get("name"))
                    .and_then(Value::str)
                    .unwrap_or_default();
                found.insert(format!("FSM:{name}"));
            } else {
                found.insert(o.typename.clone());
            }
        }
        let father = sc
            .go_transform
            .get(&gid)
            .and_then(|t| sc.transform(*t))
            .and_then(|t| t.get("m_Father"))
            .and_then(|f| f.get("m_PathID"))
            .and_then(Value::int)
            .unwrap_or(0);
        if father == 0 || sc.transform(father).is_none() {
            break;
        }
        gid = sc
            .transform(father)
            .and_then(|t| t.get("m_GameObject"))
            .and_then(|f| f.get("m_PathID"))
            .and_then(Value::int)
            .unwrap_or(0);
    }
    found
}

/// The decor family of a behaviour set (`DECOR_BEHAVIOURS`).
pub fn decor_family(behaviours: &BTreeSet<String>) -> Option<&'static str> {
    let names: Vec<&str> = behaviours.iter().map(String::as_str).collect();
    match names.as_slice() {
        [] => Some("loop"),
        ["LiftChain"] => Some("chain"),
        ["FSM:glow_bug", "PlayMakerFixedUpdate"] => Some("glow_bug"),
        _ => None,
    }
}

/// One decor object: an active, rendered tk2dSprite whose behaviours name a family.
#[derive(Clone, Debug)]
pub struct DecorSource {
    pub sprite_id: i64,
    pub gid: i64,
    pub renderer_source: String,
    pub family: &'static str,
    pub animator: Option<Value>,
    pub renderer: Value,
}

/// `decor_sources(sc)`, by ascending object id.
pub fn decor_sources(sc: &Scene) -> Vec<DecorSource> {
    let mut ids: Vec<i64> = sc
        .objects
        .iter()
        .filter(|o| o.typename == "tk2dSprite")
        .map(|o| o.id)
        .collect();
    ids.sort_unstable();
    let mut out = Vec::new();
    for i in ids {
        let Some(o) = sc.object(i) else { continue };
        let Some(gid) = o
            .tree
            .get("m_GameObject")
            .and_then(|g| g.get("m_PathID"))
            .and_then(Value::int)
        else {
            continue;
        };
        if !sc.gos.contains_key(&gid) || !sc.active(gid) {
            continue;
        }
        let Some(family) = decor_family(&decor_behaviours(sc, gid)) else {
            continue;
        };
        let (mut renderer, mut animator) = (None, None);
        for id in components_of(sc, gid) {
            match sc.object(id) {
                Some(c) if c.typename == "MeshRenderer" => renderer = Some(c.tree.clone()),
                Some(c) if c.typename == "tk2dSpriteAnimator" => animator = Some(c.tree.clone()),
                _ => {}
            }
        }
        let Some(renderer) = renderer else { continue };
        if !renderer.get("m_Enabled").is_some_and(Value::truthy) {
            continue;
        }
        out.push(DecorSource {
            sprite_id: i,
            gid,
            renderer_source: sc.sid(i),
            family,
            animator,
            renderer,
        });
    }
    out
}

/// A decor object's playing clip and its draws.
#[derive(Clone, Debug)]
pub struct DecorClip {
    pub draws: Vec<Draw>,
    pub family: &'static str,
    pub source: String,
    pub fps: f64,
    pub wrap: i64,
    pub loop_start: i64,
    pub sequence: Vec<usize>,
}

/// `decor_draws(...)`: one scenery draw per unique frame of the object's
/// playing clip; None when no frame reaches any camera position of the view.
pub fn decor_draws(
    source: &Source,
    sc: &Scene,
    record: &DecorSource,
    atlas: &mut Atlas,
    caches: &mut SceneryCaches,
    view: &View,
) -> Result<Option<DecorClip>> {
    let sprite = &sc.object(record.sprite_id).ok_or("no sprite")?.tree;
    let gid = record.gid;
    let collection_o = u(sc.deref(get(sprite, "collection")?))?;
    let animator = record.animator.as_ref();
    let playing = animator.filter(|a| {
        get(a, "playAutomatically").is_ok_and(Value::truthy)
            && get(a, "defaultClipId")
                .ok()
                .and_then(Value::int)
                .is_some_and(|c| c >= 0)
    });
    let (steps, fps, wrap, loop_start): (Vec<(Obj, i64)>, f64, i64, i64) = match playing {
        Some(a) => {
            let library_o = u(sc.deref(get(a, "library")?))?;
            let library = u(source.read(&library_o))?;
            let clip_id = int_of(a, "defaultClipId")? as usize;
            let clip = get(&library, "clips")?
                .list()
                .and_then(|c| c.get(clip_id))
                .ok_or("default clip out of range")?;
            let mut steps = Vec::new();
            for f in get(clip, "frames")?.list().unwrap_or(&[]) {
                steps.push((
                    u(source.deref(&library_o.file, get(f, "spriteCollection")?))?,
                    int_of(f, "spriteId")?,
                ));
            }
            (
                steps,
                f64_of(clip, "fps")?,
                crate::actor_art::guest_wrap(clip)?,
                clip.get("loopStart").and_then(Value::int).unwrap_or(0),
            )
        }
        None => (
            vec![(collection_o.clone(), int_of(sprite, "_spriteId")?)],
            0.0,
            2,
            0,
        ),
    };
    let mut unique: Vec<(String, i64)> = Vec::new();
    let mut sequence = Vec::new();
    for (co, index) in &steps {
        let key = (co.sid(), *index);
        let at = match unique.iter().position(|k| *k == key) {
            Some(i) => i,
            None => {
                unique.push(key);
                unique.len() - 1
            }
        };
        sequence.push(at);
    }
    let k = get(sprite, "_scale")?;
    let (kx, ky) = (f64_of(k, "x")?, f64_of(k, "y")?);
    let color = get(sprite, "_color")?;
    let mut collections: HashMap<String, Value> = HashMap::new();
    let mut textures = HashMap::new();
    let mut draws = Vec::new();
    let mut visible = false;
    let cull = view.cull.ok_or("a decor view needs its camera range")?;
    for (sid, index) in &unique {
        let co = &steps
            .iter()
            .find(|(c, _)| c.sid() == *sid)
            .ok_or("frame collection vanished")?
            .0;
        if !collections.contains_key(sid) {
            collections.insert(sid.clone(), u(source.read(co))?);
        }
        let (image, bounds) = tk_sprite(
            source,
            &co.file,
            &collections[sid],
            *index as usize,
            &mut textures,
        )?;
        let (x0, y0, x1, y1) = (
            bounds[0] * kx,
            bounds[1] * ky,
            bounds[2] * kx,
            bounds[3] * ky,
        );
        let mut points = Vec::new();
        for (x, y) in [(x0, y1), (x1, y1), (x0, y0), (x1, y0)] {
            points.push(u(sc.point(gid, x, y, 0.0))?);
        }
        let z = points[0][2];
        if z <= CAM_Z + 2.0 {
            return Ok(None);
        }
        let scale = focal() / (z - CAM_Z);
        if !out_of_view(&cull, &points, scale) {
            visible = true;
        }
        let w = dist(&points[0], &points[1]) * scale;
        let h = dist(&points[0], &points[2]) * scale;
        if let Some(reason) = native_draw_range(scale, &points) {
            return err(reason);
        }
        let frame_sid = format!("{sid}:{index}");
        let (texture, _) = scenery_texture(
            atlas,
            &mut caches.images,
            &mut caches.shared_pixels,
            &frame_sid,
            || Ok(image),
            w,
            h,
            f64_of(color, "a")?,
            view.cap,
            None,
        )?;
        draws.push(Draw {
            source: record.renderer_source.clone(),
            sprite: frame_sid,
            name: get(sc.go(gid).ok_or("no GameObject")?, "m_Name")?
                .str()
                .unwrap_or_default(),
            texture,
            points,
            scale,
            tint: tint_of(color)?,
            z,
            order: int_of(&record.renderer, "m_SortingOrder")?,
            layer: int_of(&record.renderer, "m_SortingLayer")?,
        });
    }
    if !visible {
        return Ok(None);
    }
    Ok(Some(DecorClip {
        draws,
        family: record.family,
        source: record.renderer_source.clone(),
        fps,
        wrap,
        loop_start,
        sequence,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimensions_shrink_to_the_cap_and_refuse_nonsense() {
        assert_eq!(scenery_dimensions(10.2, 3.0, 96).unwrap(), (11, 3));
        assert_eq!(scenery_dimensions(400.0, 200.0, 252).unwrap(), (252, 126));
        assert!(scenery_dimensions(0.0, 4.0, 96).is_err());
        assert!(scenery_dimensions(4.0, f64::NAN, 96).is_err());
        assert!(scenery_dimensions(4.0, 4.0, 253).is_err());
    }

    #[test]
    fn a_near_axis_edge_snaps_and_a_slope_is_refused() {
        let b = [0.0, 0.0, 100.0, 100.0];
        let (a, c) = cooked_edge([10.0, 5.0], [10.004, 9.0], b, false).unwrap();
        assert_eq!((a, c), ([10.0, 5.0], [10.0, 9.0]));
        assert_eq!(
            cooked_edge([10.0, 5.0], [14.0, 9.0], b, false),
            Err("sloped terrain edge")
        );
        assert!(cooked_edge([10.0, 5.0], [14.0, 9.0], b, true).is_ok());
        assert_eq!(
            cooked_edge([200.0, 5.0], [210.0, 5.0], b, false),
            Err("outside collision bounds")
        );
    }

    #[test]
    fn draw_range_names_the_limit_it_breaks() {
        assert!(native_draw_range(1.0, &[[0.0, 0.0, 0.0]]).is_none());
        assert!(native_draw_range(0.0001, &[]).unwrap().contains("scale"));
        assert!(native_draw_range(40.0, &[[4000.0, 0.0, 0.0]])
            .unwrap()
            .contains("Q24.8"));
    }
}
