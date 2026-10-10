//! Original Breakable particle families, emitted into the existing bounded pool.
//! Ported from host/break_effects.py, whose output it reproduces byte for byte.
//!
//! Fresh Windows source only. No room mutation and no replacement generic
//! particles. Source curves are sampled to a shared 33-entry fixed-point table;
//! PS1 alpha uses three prequantized levels. Unity collision/damping/RNG remain
//! approximations.
//!
//! Every Breakable debris part that carries a ParticleSystem becomes one
//! emitter of one deduplicated style; a hidden wall's or cracked floor's dust,
//! rock and bit emitters join them through `collect_secret`, cut to the secret
//! budgets. Each scene then gets one HKFX0001 chunk (styles, emitters, art,
//! VRAM uploads, frames and texels), and the linked side (`data/break_effects.rs`)
//! holds the shared curve table and the chunk manifest.
//!
//! Only what the effect cook consumes of host/breakables.py's
//! `breakable_sources` is ported (identity, state index, debris parts, angle
//! offset). The Python computed and could refuse on the rest of each record
//! (audio, mask fades, hit polygons) without using it here.

use crate::common::{component_records, err, get, kid, kids, py_round, Result};
use crate::cook::{focal, CAM_Z};
use crate::materials::quantize_alpha_coverage;
use crate::pyjson::{dumps, Json};
use hk_pil::resample::Filter;
use hk_pil::{Image, Mode};
use hk_unity::scene::Scene;
use hk_unity::serialized::SerializedFile;
use hk_unity::{Obj, Source, Value};
use serde_json::Value as J;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

const VRAM_RECTS: [(i64, i64, i64, i64); 3] =
    [(352, 176, 32, 64), (360, 32, 24, 32), (352, 64, 32, 32)];
const ALPHAS: [u32; 3] = [43, 85, 128];
/// Slots in the guest particle pool, game/src/particles.rs CAPACITY. An
/// emitter authored above this can never finish emitting even into an empty
/// pool, so it is refused here rather than shipped as a style the guest
/// truncates. 224 is the smallest multiple of 32 (the guest's pending-collision
/// bitmap word) above the largest measured source emitter, Crossroads_09's 210.
const POOL_CAPACITY: i64 = 224;
/// Texture-sheet rows a random-row emitter may draw from (measured: 3, 4, 6).
const MAX_SHEET_ROWS: i64 = 8;
/// geo.py `MAX_VRAM_BYTES`.
const MAX_VRAM_BYTES: i64 = 8192;
/// A secret's bursts, cut to fit the PS1: every particle of a burst is a pool
/// slot and a packet for its life, so each burst keeps its emitters'
/// proportions within these totals and each emitter its authored emission time.
const SECRET_HIT_BUDGET: i64 = 24;
const SECRET_BREAK_BUDGET: i64 = 96;
/// `Emit(n)` releases its n at once.
const EMIT_RATE: f64 = 10000.0;
const FX_MAGIC: &[u8] = b"HKFX0001";
const FX_LIMITS: [(&str, usize); 5] = [
    ("styles", 18),
    ("emitters", 100),
    ("art", 48),
    ("uploads", 48),
    ("frames", 96),
];
const FAMILY_WALL: i64 = 1;
const FAMILY_WALL_TK2D: i64 = 2;
const FAMILY_FLOOR: i64 = 3;
const FACING_STATES: [&str; 4] = ["Hit Right", "Hit Up", "Hit Left", "Hit Down"];
const MAX_SCENE_BREAKABLES: usize = 128;
/// breakables.py: serialized rigid-fling variants measured across every
/// debrisPart of every Breakable in the admitted scenes.
const RIGID_BODIES: [(f64, f64, f64, f64, f64); 3] = [
    (0.0, 1.0, 0.05, 1.0, 0.0),
    (0.0, 1.0, 0.05, 0.9, 0.0),
    (0.0, 1.0, 3.0, 1.0, 0.0),
];
const RIGID_BOUNCE_FACTORS: [f64; 3] = [0.1, 0.4, 0.5];
const LIMITATIONS: [&str; 5] = [
    "Deterministic RNG,60Hz integration/damping,33-point curves,radius/edge collision and initial-lifetime collision-loss approximation differ from nativeUnity particle physics.",
    "Three baked opacity levels and sourceRGB palette quantization approximate Sprites/Lit lighting/blending.",
    "Shared 224-slot particle pool reports capacity drops; authored maximum-per-emitter counts and lifetimes preserved.",
    "Town graves use source uniform-XY planar Hierarchy scaling; nonuniform-XY/sheared/out-of-plane and local-space Hierarchy emitters fail closed.",
    "Geo-rock hit jitter/gleam/chips,containingParticles probability events and mainLifeblood cocoon are separate systems.",
];

// ---------------------------------------------------------------- values

fn num(v: &Value) -> Result<f64> {
    v.float()
        .or_else(|| v.int().map(|i| i as f64))
        .ok_or_else(|| "not a number".to_string())
}
fn f(v: &Value, key: &str) -> Result<f64> {
    num(get(v, key)?)
}
fn i(v: &Value, key: &str) -> Result<i64> {
    let x = get(v, key)?;
    x.int()
        .or(if let Value::Bool(b) = x {
            Some(*b as i64)
        } else {
            None
        })
        .ok_or_else(|| format!("{key} is not an integer"))
}
fn truthy(v: &Value, key: &str) -> Result<bool> {
    Ok(get(v, key)?.truthy())
}
fn set(v: &mut Value, key: &str, x: Value) {
    if let Value::Map(fields) = v {
        if let Some(slot) = fields.iter_mut().find(|(k, _)| &**k == key) {
            slot.1 = x;
        } else {
            fields.push((Arc::from(key), x));
        }
    }
}
fn get_mut<'a>(v: &'a mut Value, key: &str) -> Result<&'a mut Value> {
    match v {
        Value::Map(fields) => fields
            .iter_mut()
            .find(|(k, _)| &**k == key)
            .map(|(_, x)| x)
            .ok_or_else(|| format!("missing {key}")),
        _ => err(format!("missing {key}")),
    }
}
fn map(pairs: Vec<(&str, Value)>) -> Value {
    Value::Map(pairs.into_iter().map(|(k, v)| (Arc::from(k), v)).collect())
}

/// `q`: Q16 fixed point.
fn q(v: f64) -> Result<i64> {
    if !v.is_finite() || v.abs() >= 32768.0 {
        return err("break effect fixed point range");
    }
    Ok(py_round(v * 65536.0))
}

/// particles.py `curve_value`: Unity's Hermite AnimationCurve.
fn curve_value(curve: &Value, t: f64) -> Result<f64> {
    let points = get(curve, "m_Curve")?.list().unwrap_or(&[]);
    if points.is_empty() {
        return err("empty particle curve");
    }
    for p in points {
        if i(p, "weightedMode")? != 0 {
            return err("weighted particle curve unsupported");
        }
    }
    let (first, last) = (&points[0], &points[points.len() - 1]);
    if t <= f(first, "time")? {
        return f(first, "value");
    }
    if t >= f(last, "time")? {
        return f(last, "value");
    }
    for w in points.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        let (ta, tb) = (f(a, "time")?, f(b, "time")?);
        if ta <= t && t <= tb {
            let dt = tb - ta;
            let u = (t - ta) / dt;
            let u3 = u.powf(3.0);
            return Ok((2.0 * u3 - 3.0 * u * u + 1.0) * f(a, "value")?
                + (u3 - 2.0 * u * u + u) * dt * f(a, "outSlope")?
                + (-2.0 * u3 + 3.0 * u * u) * f(b, "value")?
                + (u3 - u * u) * dt * f(b, "inSlope")?);
        }
    }
    err("particle curve ordering")
}

/// particles.py `gradient_alpha`.
fn gradient_alpha(g: &Value, t: f64) -> Result<f64> {
    let n = i(g, "m_NumAlphaKeys")?;
    let mut keys = Vec::new();
    for k in 0..n {
        keys.push((
            f(g, &format!("atime{k}"))? / 65535.0,
            f(get(g, &format!("key{k}"))?, "a")?,
        ));
    }
    if i(g, "m_Mode")? != 0 || keys.is_empty() {
        return err("unsupported gradient interpolation");
    }
    if t <= keys[0].0 {
        return Ok(keys[0].1);
    }
    if t >= keys[keys.len() - 1].0 {
        return Ok(keys[keys.len() - 1].1);
    }
    for w in keys.windows(2) {
        let ((a, x), (b, y)) = (w[0], w[1]);
        if a <= t && t <= b {
            return Ok(x + (y - x) * (t - a) / (b - a));
        }
    }
    err("gradient ordering")
}

/// `values`: a MinMaxCurve's [low, high] at `t`.
fn values(c: &Value, t: f64) -> Result<[f64; 2]> {
    let mode = i(c, "minMaxState")?;
    Ok(match mode {
        0 => [f(c, "scalar")?; 2],
        3 => [f(c, "minScalar")?, f(c, "scalar")?],
        1 => [curve_value(get(c, "maxCurve")?, t)? * f(c, "scalar")?; 2],
        2 => [
            curve_value(get(c, "minCurve")?, t)? * f(c, "minScalar")?,
            curve_value(get(c, "maxCurve")?, t)? * f(c, "scalar")?,
        ],
        _ => return err("unknown particle curve"),
    })
}
fn is_zero(v: [f64; 2]) -> bool {
    v == [0.0, 0.0]
}
fn sorted2(v: [f64; 2]) -> [f64; 2] {
    if v[1] < v[0] {
        [v[1], v[0]]
    } else {
        v
    }
}

// ---------------------------------------------------------------- styles

#[derive(Clone, PartialEq, Debug)]
struct Sample {
    size: [i64; 2],
    alpha: [i64; 2],
    spin: [i64; 2],
}

#[derive(Clone, PartialEq, Debug)]
struct Style {
    life: [i64; 2],
    speed: [i64; 2],
    size: [i64; 2],
    rotation: [i64; 2],
    colors: [[i64; 3]; 2],
    start_alpha: [i64; 2],
    count: i64,
    rate: i64,
    shape: i64,
    radius: i64,
    arc: i64,
    shape_scale: [i64; 3],
    force: [[i64; 2]; 3],
    velocity: [[i64; 2]; 3],
    limit: i64,
    dampen: i64,
    spin_speed: [i64; 2],
    spin_range: [i64; 2],
    collision: bool,
    bounce: i64,
    collision_dampen: i64,
    life_loss: i64,
    kill_speed: i64,
    radius_scale: i64,
    samples: Vec<Sample>,
    cells: i64,
    texture: String,
}

fn ints<const N: usize>(v: [i64; N]) -> Json {
    Json::List(v.iter().map(|&x| Json::Int(x)).collect())
}
impl Sample {
    fn json(&self) -> Json {
        Json::Obj(vec![
            ("size".into(), ints(self.size)),
            ("alpha".into(), ints(self.alpha)),
            ("spin".into(), ints(self.spin)),
        ])
    }
    /// `json.dumps(sample, sort_keys=True)`.
    fn sorted_text(&self) -> String {
        let l = |v: [i64; 2]| format!("[{}, {}]", v[0], v[1]);
        format!(
            "{{\"alpha\": {}, \"size\": {}, \"spin\": {}}}",
            l(self.alpha),
            l(self.size),
            l(self.spin)
        )
    }
}
impl Style {
    fn json(&self) -> Json {
        let pairs = |v: &[[i64; 2]; 3]| Json::List(v.iter().map(|p| ints(*p)).collect());
        Json::Obj(vec![
            ("life".into(), ints(self.life)),
            ("speed".into(), ints(self.speed)),
            ("size".into(), ints(self.size)),
            ("rotation".into(), ints(self.rotation)),
            (
                "colors".into(),
                Json::List(self.colors.iter().map(|c| ints(*c)).collect()),
            ),
            ("start_alpha".into(), ints(self.start_alpha)),
            ("count".into(), Json::Int(self.count)),
            ("rate".into(), Json::Int(self.rate)),
            ("shape".into(), Json::Int(self.shape)),
            ("radius".into(), Json::Int(self.radius)),
            ("arc".into(), Json::Int(self.arc)),
            ("shape_scale".into(), ints(self.shape_scale)),
            ("force".into(), pairs(&self.force)),
            ("velocity".into(), pairs(&self.velocity)),
            ("limit".into(), Json::Int(self.limit)),
            ("dampen".into(), Json::Int(self.dampen)),
            ("spin_speed".into(), ints(self.spin_speed)),
            ("spin_range".into(), ints(self.spin_range)),
            ("collision".into(), Json::Bool(self.collision)),
            ("bounce".into(), Json::Int(self.bounce)),
            ("collision_dampen".into(), Json::Int(self.collision_dampen)),
            ("life_loss".into(), Json::Int(self.life_loss)),
            ("kill_speed".into(), Json::Int(self.kill_speed)),
            ("radius_scale".into(), Json::Int(self.radius_scale)),
            (
                "samples".into(),
                Json::List(self.samples.iter().map(Sample::json).collect()),
            ),
            ("cells".into(), Json::Int(self.cells)),
            ("texture".into(), Json::Str(self.texture.clone())),
        ])
    }
    fn samples_key(&self) -> String {
        format!(
            "[{}]",
            self.samples
                .iter()
                .map(Sample::sorted_text)
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

/// `particle_scale`: static uniform-XY planar Hierarchy scaling reduced to one
/// world-space scalar.
fn particle_scale(ps: &Value, m: &[[f64; 4]; 4]) -> Result<f64> {
    let mode = i(ps, "scalingMode")?;
    if mode == 2 {
        return Ok(1.0);
    }
    let columns: Vec<[f64; 3]> = (0..3).map(|j| [m[0][j], m[1][j], m[2][j]]).collect();
    let lengths: Vec<f64> = columns
        .iter()
        .map(|c| (0.0 + c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt())
        .collect();
    if mode != 0 {
        return err(format!(
            "unsupported particle scaling mode {mode} at scale [{:.4},{:.4},{:.4}]",
            lengths[0], lengths[1], lengths[2]
        ));
    }
    if i(ps, "moveWithTransform")? != 1 {
        return err("Hierarchy particles require world simulation");
    }
    let scale = lengths[0];
    if !scale.is_finite()
        || scale <= 0.0
        || lengths.iter().any(|v| !v.is_finite() || *v <= 0.0)
        || (lengths[1] - scale).abs() > scale * 1e-6
    {
        return err("Hierarchy particle XY scale must be positive and uniform");
    }
    for a in 0..3 {
        for b in 0..a {
            let d = 0.0
                + columns[a][0] * columns[b][0]
                + columns[a][1] * columns[b][1]
                + columns[a][2] * columns[b][2];
            if d.abs() > scale * scale * 1e-6 {
                return err("Hierarchy particle transform must not shear");
            }
        }
    }
    if [(0, 2), (1, 2), (2, 0), (2, 1)]
        .iter()
        .any(|&(a, b)| m[a][b].abs() > 1e-12)
        || i(get(ps, "ShapeModule")?, "type")? != 10
    {
        return err("Hierarchy particle emission must stay in XY plane");
    }
    for module in ["ForceModule", "VelocityModule"] {
        let md = get(ps, module)?;
        if truthy(md, "enabled")? && !is_zero(values(get(md, "z")?, 0.0)?) {
            return err("Hierarchy particle motion must stay in XY plane");
        }
    }
    if !is_zero(values(
        get(get(ps, "InitialModule")?, "gravityModifier")?,
        0.0,
    )?) {
        return err("Hierarchy particle gravity scaling needs native validation");
    }
    Ok(scale)
}

/// `emits_nothing`: the source emission is provably zero particles.
fn emits_nothing(ps: &Value) -> Result<bool> {
    let em = get(ps, "EmissionModule")?;
    if !truthy(em, "enabled")? {
        return Ok(true);
    }
    let zero =
        |c: &Value| -> Result<bool> { Ok(i(c, "minMaxState")? == 0 && f(c, "scalar")? == 0.0) };
    Ok(i(em, "m_BurstCount")? == 0
        && zero(get(em, "rateOverTime")?)?
        && zero(get(em, "rateOverDistance")?)?)
}

fn style(
    ps: &Value,
    gravity: f64,
    scale: f64,
    played: bool,
    random_gravity: bool,
) -> Result<Style> {
    let im = get(ps, "InitialModule")?;
    let shape = get(ps, "ShapeModule")?;
    let uv = get(ps, "UVModule")?;
    let em = get(ps, "EmissionModule")?;
    let Value::Map(fields) = ps else {
        return err("particle system is not a map");
    };
    let mut enabled: Vec<String> = fields
        .iter()
        .filter(|(k, v)| {
            k.ends_with("Module") && v.is_map() && v.get("enabled").is_some_and(Value::truthy)
        })
        .map(|(k, _)| k.to_string())
        .collect();
    const SUPPORTED: [&str; 12] = [
        "InitialModule",
        "ShapeModule",
        "EmissionModule",
        "SizeModule",
        "RotationModule",
        "ColorModule",
        "UVModule",
        "VelocityModule",
        "ForceModule",
        "ClampVelocityModule",
        "RotationBySpeedModule",
        "CollisionModule",
    ];
    if enabled.iter().any(|k| k == "SubModule") {
        let subs = get(get(ps, "SubModule")?, "subEmitters")?
            .list()
            .unwrap_or(&[]);
        let mut all_null = true;
        for e in subs {
            if get(get(e, "emitter")?, "m_PathID")?.truthy() {
                all_null = false;
            }
        }
        if all_null {
            enabled.retain(|k| k != "SubModule");
        }
    }
    let unsupported: Vec<&String> = enabled
        .iter()
        .filter(|k| !SUPPORTED.contains(&k.as_str()))
        .collect();
    if !unsupported.is_empty() {
        // Python prints the set; with more than one member its order is the
        // string hash order of that run, so a sorted list stands in for it.
        let mut names: Vec<String> = unsupported.iter().map(|k| format!("'{k}'")).collect();
        names.sort();
        return err(format!(
            "unsupported particle modules: {{{}}}",
            names.join(", ")
        ));
    }
    if truthy(ps, "looping")? {
        return err("looping particle system");
    }
    if !truthy(ps, "playOnAwake")? && !played {
        return err("particle system does not play on awake and no resolved action plays it");
    }
    let scaling = i(ps, "scalingMode")?;
    if scaling != 0 && scaling != 2 {
        return err(format!("unsupported particle scaling mode {scaling}"));
    }
    let space = i(ps, "moveWithTransform")?;
    if space != 0 && space != 1 {
        return err(format!("unsupported particle simulation space {space}"));
    }
    if truthy(im, "size3D")?
        || truthy(im, "rotation3D")?
        || i(im, "gravitySource")? != 0
        || i(em, "m_BurstCount")? != 0
        || !is_zero(values(get(em, "rateOverDistance")?, 0.0)?)
    {
        return err("particle emission mode");
    }
    let shape_type = i(shape, "type")?;
    if (shape_type != 5 && shape_type != 10) || !truthy(shape, "enabled")? {
        return err("particle shape");
    }
    for k in ["x", "y", "z"] {
        if f(get(shape, "m_Position")?, k)? != 0.0 || f(get(shape, "m_Rotation")?, k)? != 0.0 {
            return err("particle shape transform");
        }
    }
    if f(shape, "randomDirectionAmount")? != 0.0
        || f(shape, "sphericalDirectionAmount")? != 0.0
        || f(shape, "randomPositionAmount")? != 0.0
        || i(get(shape, "radius")?, "mode")? != 0
        || i(get(shape, "arc")?, "mode")? != 0
    {
        return err("particle shape random mode");
    }
    let uv_on = truthy(uv, "enabled")?;
    if uv_on
        && (i(uv, "tilesX")? != 1
            || i(uv, "animationType")? != 1
            || i(uv, "rowMode")? != 1
            || !(1..=MAX_SHEET_ROWS).contains(&i(uv, "tilesY")?))
    {
        return err("particle random row");
    }
    let start = get(im, "startColor")?;
    let start_mode = i(start, "minMaxState")?;
    if start_mode != 0 && start_mode != 2 {
        return err(format!("particle start color mode {start_mode}"));
    }
    let ends = if start_mode == 0 {
        [get(start, "maxColor")?, get(start, "maxColor")?]
    } else {
        [get(start, "minColor")?, get(start, "maxColor")?]
    };
    let mut colors = [[0i64; 3]; 2];
    for (n, end) in ends.iter().enumerate() {
        for (c, k) in ["r", "g", "b"].iter().enumerate() {
            colors[n][c] = py_round(f(end, k)? * 128.0);
        }
    }
    // max() keeps the first of equal values.
    let (a0, a1) = (f(ends[0], "a")?, f(ends[1], "a")?);
    let initial_alpha = if a1 > a0 { a1 } else { a0 };
    let mut start_alpha = [255i64; 2];
    if initial_alpha != 0.0 {
        for (n, end) in ends.iter().enumerate() {
            start_alpha[n] = py_round(f(end, "a")? / initial_alpha * 255.0);
        }
    }
    if start_alpha.iter().any(|v| !(0..=255).contains(v)) {
        return err(format!(
            "particle start color alpha [{}, {}]",
            start_alpha[0], start_alpha[1]
        ));
    }
    if colors.iter().flatten().any(|v| !(0..=255).contains(v)) {
        return err("particle color range");
    }
    let collision = get(ps, "CollisionModule")?;
    let collision_on = truthy(collision, "enabled")?;
    if collision_on
        && (i(collision, "type")? != 1
            || i(collision, "collisionMode")? != 1
            || i(get(collision, "collidesWith")?, "m_Bits")? != 256
            || truthy(collision, "colliderForce")?)
    {
        return err("particle terrain collision");
    }
    let force = get(ps, "ForceModule")?;
    let vel = get(ps, "VelocityModule")?;
    let limit = get(ps, "ClampVelocityModule")?;
    let (force_on, vel_on, limit_on) = (
        truthy(force, "enabled")?,
        truthy(vel, "enabled")?,
        truthy(limit, "enabled")?,
    );
    if force_on && (!truthy(force, "inWorldSpace")? || truthy(force, "randomizePerFrame")?) {
        return err("particle force space");
    }
    if vel_on && !truthy(vel, "inWorldSpace")? {
        return err("particle velocity space");
    }
    if vel_on {
        for k in [
            "orbitalX",
            "orbitalY",
            "orbitalZ",
            "orbitalOffsetX",
            "orbitalOffsetY",
            "orbitalOffsetZ",
            "radial",
        ] {
            if !is_zero(values(get(vel, k)?, 0.0)?) {
                return err("particle orbital/speed modifier");
            }
        }
        if values(get(vel, "speedModifier")?, 0.0)? != [1.0, 1.0] {
            return err("particle orbital/speed modifier");
        }
    }
    if limit_on && (truthy(limit, "separateAxis")? || !is_zero(values(get(limit, "drag")?, 0.0)?)) {
        return err("particle velocity limit");
    }
    let size_module = get(ps, "SizeModule")?;
    let color_module = get(ps, "ColorModule")?;
    let rotation_module = get(ps, "RotationModule")?;
    let mut samples = Vec::new();
    for n in 0..33 {
        let t = n as f64 / 32.0;
        let size = if truthy(size_module, "enabled")? {
            values(get(size_module, "curve")?, t)?
        } else {
            [1.0, 1.0]
        };
        let mut alphas = [1.0, 1.0];
        if truthy(color_module, "enabled")? {
            let g = get(color_module, "gradient")?;
            let mode = i(g, "minMaxState")?;
            if mode != 1 && mode != 3 {
                return err("particle color curve mode");
            }
            let gs = if mode == 1 {
                [get(g, "maxGradient")?, get(g, "maxGradient")?]
            } else {
                [get(g, "minGradient")?, get(g, "maxGradient")?]
            };
            for gradient in gs {
                for k in 0..i(gradient, "m_NumColorKeys")? {
                    let key = get(gradient, &format!("key{k}"))?;
                    for c in ["r", "g", "b"] {
                        if (f(key, c)? - 1.0).abs() > 1e-6 {
                            return err("particle nonwhite color curve");
                        }
                    }
                }
            }
            alphas = [gradient_alpha(gs[0], t)?, gradient_alpha(gs[1], t)?];
        }
        let rotation = if truthy(rotation_module, "enabled")? {
            values(get(rotation_module, "curve")?, t)?
        } else {
            [0.0, 0.0]
        };
        samples.push(Sample {
            size: [q(size[0])?, q(size[1])?],
            alpha: alphas.map(|a| py_round(a * initial_alpha * 255.0).clamp(0, 255)),
            spin: [q(rotation[0].to_degrees())?, q(rotation[1].to_degrees())?],
        });
    }
    let initial = |key: &str| -> Result<[f64; 2]> { Ok(sorted2(values(get(im, key)?, 0.0)?)) };
    let rate = values(get(em, "rateOverTime")?, 0.0)?;
    if rate[0] != rate[1] || rate[0] <= 0.0 {
        return err("particle emission rate");
    }
    let count =
        i(im, "maxNumParticles")?.min((rate[0] * f(ps, "lengthInSec")? - 1e-4).ceil() as i64);
    let speedspin = get(ps, "RotationBySpeedModule")?;
    let omega = if truthy(speedspin, "enabled")? {
        values(get(speedspin, "curve")?, 0.0)?
    } else {
        [0.0, 0.0]
    };
    let mut forcev = [[0.0f64; 2]; 3];
    for (n, k) in ["x", "y", "z"].iter().enumerate() {
        if force_on {
            forcev[n] = values(get(force, k)?, 0.0)?;
        }
    }
    let gravityv = values(get(im, "gravityModifier")?, 0.0)?;
    if gravityv[0] != gravityv[1] {
        // A random gravity multiplier is a per-particle force: the guest draws
        // each particle's force from the style's range with one seed.
        if !random_gravity {
            return err("particle random gravity");
        }
        let lh = sorted2([gravity * gravityv[0], gravity * gravityv[1]]);
        forcev[1] = [forcev[1][0] + lh[0], forcev[1][1] + lh[1]];
    } else {
        forcev[1] = [
            forcev[1][0] + gravity * gravityv[0],
            forcev[1][1] + gravity * gravityv[0],
        ];
    }
    let life = initial("startLifetime")?.map(|v| py_round(v * 60.0).max(1));
    let qs = |v: [f64; 2]| -> Result<[i64; 2]> { Ok([q(v[0])?, q(v[1])?]) };
    let mut velocity = [[0i64; 2]; 3];
    for (n, k) in ["x", "y", "z"].iter().enumerate() {
        if vel_on {
            let v = values(get(vel, k)?, 0.0)?;
            velocity[n] = [q(v[0] * scale)?, q(v[1] * scale)?];
        }
    }
    let shape_scale = get(shape, "m_Scale")?;
    let range = get(speedspin, "range")?;
    let cells = if uv_on { i(uv, "tilesY")? } else { 1 };
    let st = Style {
        life,
        speed: qs(initial("startSpeed")?.map(|v| v * scale))?,
        size: qs(initial("startSize")?.map(|v| v * scale))?,
        rotation: qs(initial("startRotation")?.map(f64::to_degrees))?,
        colors,
        start_alpha,
        count,
        rate: q(rate[0])?,
        shape: shape_type,
        radius: q(f(get(shape, "radius")?, "value")?)?,
        arc: q(f(get(shape, "arc")?, "value")?)?,
        shape_scale: [
            q(f(shape_scale, "x")?)?,
            q(f(shape_scale, "y")?)?,
            q(f(shape_scale, "z")?)?,
        ],
        force: [
            qs(forcev[0].map(|v| v * scale))?,
            qs(forcev[1].map(|v| v * scale))?,
            qs(forcev[2].map(|v| v * scale))?,
        ],
        velocity,
        limit: if limit_on {
            q(values(get(limit, "magnitude")?, 0.0)?[0])?
        } else {
            -1
        },
        dampen: if limit_on { q(f(limit, "dampen")?)? } else { 0 },
        spin_speed: qs(omega.map(f64::to_degrees))?,
        spin_range: [q(f(range, "x")?)?, q(f(range, "y")?)?],
        collision: collision_on,
        bounce: q(values(get(collision, "m_Bounce")?, 0.0)?[0])?,
        collision_dampen: q(values(get(collision, "m_Dampen")?, 0.0)?[0])?,
        life_loss: q(values(get(collision, "m_EnergyLossOnCollision")?, 0.0)?[0])?,
        kill_speed: q(f(collision, "minKillSpeed")?)?,
        radius_scale: q(f(collision, "radiusScale")?)?,
        samples,
        cells,
        texture: String::new(),
    };
    if !(0 < count && count <= POOL_CAPACITY) {
        return err(format!(
            "particle source capacity: {count} particles against a {POOL_CAPACITY}-slot pool"
        ));
    }
    let longest = st.life[0].max(st.life[1]);
    if longest > 65535 {
        return err(format!(
            "particle source capacity: lifetime {longest} ticks"
        ));
    }
    Ok(st)
}

// ---------------------------------------------------------------- views

/// What `part_emitter` reads of a scene: the scene itself, or a CreateObject
/// prefab presented the same way (`PrefabView`).
pub(crate) trait View {
    fn component_refs(&self, gid: i64) -> Result<Vec<Value>>;
    fn deref(&self, source: &Source, pptr: &Value) -> Result<Obj>;
    fn world_of(&self, gid: i64) -> Result<[[f64; 4]; 4]>;
}

impl View for Scene<'_> {
    fn component_refs(&self, gid: i64) -> Result<Vec<Value>> {
        let go = self.go(gid).ok_or("no such GameObject")?;
        get(go, "m_Component")?
            .list()
            .unwrap_or(&[])
            .iter()
            .map(|c| get(c, "component").cloned())
            .collect::<Result<_>>()
    }
    fn deref(&self, _: &Source, pptr: &Value) -> Result<Obj> {
        Scene::deref(self, pptr).map_err(|e| e.to_string())
    }
    fn world_of(&self, gid: i64) -> Result<[[f64; 4]; 4]> {
        let tid = *self.go_transform.get(&gid).ok_or("no transform")?;
        self.world(tid).map_err(|e| e.to_string())
    }
}

type M4 = [[f64; 4]; 4];

fn mul4(a: &M4, r: &M4) -> M4 {
    let mut out = [[0.0; 4]; 4];
    for (row, out_row) in out.iter_mut().enumerate() {
        for (col, cell) in out_row.iter_mut().enumerate() {
            // Python's sum() starts from the integer 0.
            let mut s = 0.0f64;
            for k in 0..4 {
                s = if k == 0 {
                    a[row][k] * r[k][col]
                } else {
                    s + a[row][k] * r[k][col]
                };
            }
            *cell = s;
        }
    }
    out
}

/// `euler_basis`: Unity's Quaternion.Euler as a 3x3 basis (Z, then X, then Y).
fn euler_basis(d: [f64; 3]) -> [[f64; 3]; 3] {
    let (x, y, z) = (d[0].to_radians(), d[1].to_radians(), d[2].to_radians());
    let rot = |axis: char, a: f64| -> [[f64; 3]; 3] {
        let (c, s) = (a.cos(), a.sin());
        match axis {
            'x' => [[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]],
            'y' => [[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]],
            _ => [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]],
        }
    };
    let mul = |a: [[f64; 3]; 3], b: [[f64; 3]; 3]| -> [[f64; 3]; 3] {
        let mut o = [[0.0; 3]; 3];
        for (i, row) in o.iter_mut().enumerate() {
            for (j, cell) in row.iter_mut().enumerate() {
                *cell = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j];
            }
        }
        o
    };
    mul(mul(rot('y', y), rot('x', x)), rot('z', z))
}

/// `PrefabView`: a CreateObject prefab presented the way `part_emitter` reads
/// a scene. `Instantiate(prefab, position, rotation)` replaces the root's
/// serialized position and rotation and keeps its scale, so `world` composes
/// the spawn transform with each object's own local chain below the root.
struct PrefabView {
    file: Arc<SerializedFile>,
    gos: HashMap<i64, Value>,
    transforms: HashMap<i64, Value>,
    go_transform: HashMap<i64, i64>,
    root: i64,
    root_tid: i64,
    spawn: M4,
}

impl PrefabView {
    /// `rotation` is the spawn's Euler angles, or None for `Spawn(prefab, position)`,
    /// which keeps the prefab root's own rotation.
    fn new(
        source: &Source,
        file: Arc<SerializedFile>,
        root_gid: i64,
        origin: [f64; 3],
        rotation: Option<[f64; 3]>,
    ) -> Result<Self> {
        let read = |id: i64| -> Result<Value> {
            let o = source.object(&file, id).map_err(|e| e.to_string())?;
            source.read(&o).map_err(|e| e.to_string())
        };
        let (mut gos, mut transforms, mut go_transform) =
            (HashMap::new(), HashMap::new(), HashMap::new());
        let mut pending = vec![root_gid];
        while let Some(gid) = pending.pop() {
            if gos.contains_key(&gid) {
                continue;
            }
            let go = read(gid)?;
            let mut tid = None;
            for c in get(&go, "m_Component")?.list().unwrap_or(&[]) {
                let o = source
                    .deref(&file, get(c, "component")?)
                    .map_err(|e| e.to_string())?;
                if o.class_id() == 4 {
                    tid = Some(o.path_id());
                    break;
                }
            }
            let tid = tid.ok_or("prefab object carries no Transform")?;
            gos.insert(gid, go);
            go_transform.insert(gid, tid);
            let transform = read(tid)?;
            for child in get(&transform, "m_Children")?
                .list()
                .unwrap_or(&[])
                .to_vec()
            {
                let cid = i(&child, "m_PathID")?;
                let kid = read(cid)?;
                pending.push(i(get(&kid, "m_GameObject")?, "m_PathID")?);
                transforms.insert(cid, kid);
            }
            transforms.insert(tid, transform);
        }
        let root_tid = go_transform[&root_gid];
        let s = get(&transforms[&root_tid], "m_LocalScale")?.clone();
        let basis = match rotation {
            Some(r) => euler_basis(r),
            None => {
                let q = get(&transforms[&root_tid], "m_LocalRotation")?;
                let (x, y, z, w) = (f(q, "x")?, f(q, "y")?, f(q, "z")?, f(q, "w")?);
                [
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
                ]
            }
        };
        let scale = [f(&s, "x")?, f(&s, "y")?, f(&s, "z")?];
        let mut spawn = [[0.0, 0.0, 0.0, 1.0]; 4];
        for r in 0..3 {
            spawn[r] = [
                basis[r][0] * scale[0],
                basis[r][1] * scale[1],
                basis[r][2] * scale[2],
                origin[r],
            ];
        }
        Ok(Self {
            file,
            gos,
            transforms,
            go_transform,
            root: root_gid,
            root_tid,
            spawn,
        })
    }

    fn local(&self, tid: i64) -> Result<M4> {
        if tid == self.root_tid {
            let mut m = [[0.0; 4]; 4];
            for (k, row) in m.iter_mut().enumerate() {
                row[k] = 1.0;
            }
            return Ok(m);
        }
        let t = &self.transforms[&tid];
        let rq = get(t, "m_LocalRotation")?;
        let (x, y, z, w) = (f(rq, "x")?, f(rq, "y")?, f(rq, "z")?, f(rq, "w")?);
        let (s, p) = (get(t, "m_LocalScale")?, get(t, "m_LocalPosition")?);
        let mut r = [
            [
                1.0 - 2.0 * (y * y + z * z),
                2.0 * (x * y - z * w),
                2.0 * (x * z + y * w),
                f(p, "x")?,
            ],
            [
                2.0 * (x * y + z * w),
                1.0 - 2.0 * (x * x + z * z),
                2.0 * (y * z - x * w),
                f(p, "y")?,
            ],
            [
                2.0 * (x * z - y * w),
                2.0 * (y * z + x * w),
                1.0 - 2.0 * (x * x + y * y),
                f(p, "z")?,
            ],
            [0.0, 0.0, 0.0, 1.0],
        ];
        for row in r.iter_mut().take(3) {
            for (col, k) in ["x", "y", "z"].iter().enumerate() {
                row[col] *= f(s, k)?;
            }
        }
        let father = i(get(t, "m_Father")?, "m_PathID")?;
        if father == 0 || !self.transforms.contains_key(&father) {
            return err("prefab object is not under the instantiated root");
        }
        Ok(mul4(&self.local(father)?, &r))
    }

    fn subtree(&self) -> Result<Vec<i64>> {
        let mut out = vec![self.root];
        let mut n = 0;
        while n < out.len() {
            let tid = self.go_transform.get(&out[n]).copied();
            n += 1;
            let Some(tid) = tid else { continue };
            for child in get(&self.transforms[&tid], "m_Children")?
                .list()
                .unwrap_or(&[])
            {
                let cid = i(child, "m_PathID")?;
                if let Some(kid) = self.transforms.get(&cid) {
                    out.push(i(get(kid, "m_GameObject")?, "m_PathID")?);
                }
            }
        }
        Ok(out)
    }
}

impl View for PrefabView {
    fn component_refs(&self, gid: i64) -> Result<Vec<Value>> {
        let go = self.gos.get(&gid).ok_or("no such prefab GameObject")?;
        get(go, "m_Component")?
            .list()
            .unwrap_or(&[])
            .iter()
            .map(|c| get(c, "component").cloned())
            .collect::<Result<_>>()
    }
    fn deref(&self, source: &Source, pptr: &Value) -> Result<Obj> {
        source.deref(&self.file, pptr).map_err(|e| e.to_string())
    }
    fn world_of(&self, gid: i64) -> Result<M4> {
        let tid = *self.go_transform.get(&gid).ok_or("no transform")?;
        Ok(mul4(&self.spawn, &self.local(tid)?))
    }
}

// ---------------------------------------------------------------- emitters

pub(crate) struct Emitter {
    style: Style,
    system: Obj,
    texture: Obj,
    texture_id: String,
    matrix: M4,
    shader: String,
    scaling_mode: i64,
    world_parameter_scale: f64,
    ps_sha256: String,
    /// `prefab_emitters`: `file:gid` of the prefab part.
    part: Option<String>,
}

pub(crate) enum Found {
    None,
    Silent,
    Emitter(Box<Emitter>),
}

/// A pair list (`m_Colors`, `m_TexEnvs`) as Python's dict() reads it: the last
/// pair with the key wins.
fn pair<'a>(list: &'a Value, key: &str) -> Result<&'a Value> {
    list.list()
        .unwrap_or(&[])
        .iter()
        .filter_map(|p| p.list())
        .rfind(|p| p.len() == 2 && p[0].str().as_deref() == Some(key))
        .map(|p| &p[1])
        .ok_or_else(|| format!("no {key}"))
}

/// `part_emitter`: one debrisPart's emitter, Silent when a played system
/// provably emits nothing, or None when the part is not a particle system.
#[allow(clippy::too_many_arguments)]
pub(crate) fn part_emitter(
    source: &Source,
    view: &dyn View,
    gid: i64,
    gravity: f64,
    played: bool,
    relaxed: bool,
    emit: Option<i64>,
    unloop: bool,
) -> Result<Found> {
    let mut cs: Vec<(String, Obj, Value)> = Vec::new();
    for r in view.component_refs(gid)? {
        let o = view.deref(source, &r)?;
        let name = source.typename(&o).map_err(|e| e.to_string())?;
        let tree = source.read(&o).map_err(|e| e.to_string())?;
        if let Some(slot) = cs.iter_mut().find(|c| c.0 == name) {
            *slot = (name, o, tree);
        } else {
            cs.push((name, o, tree));
        }
    }
    let Some((_, po, ps)) = cs.iter().find(|c| c.0 == "ParticleSystem").cloned() else {
        return Ok(Found::None);
    };
    let (_, _, renderer) = cs
        .iter()
        .find(|c| c.0 == "ParticleSystemRenderer")
        .ok_or("ParticleSystemRenderer")?;
    if played && emits_nothing(&ps)? && emit.is_none() {
        return Ok(Found::Silent);
    }
    let materials = get(renderer, "m_Materials")?.list().unwrap_or(&[]);
    let mo = view.deref(source, materials.first().ok_or("no material")?)?;
    let mat = source.read(&mo).map_err(|e| e.to_string())?;
    let so = source
        .deref(&mo.file, get(&mat, "m_Shader")?)
        .map_err(|e| e.to_string())?;
    let shader_tree = source.read(&so).map_err(|e| e.to_string())?;
    let shader = get(get(&shader_tree, "m_ParsedForm")?, "m_Name")?
        .str()
        .unwrap_or_default();
    let (mode, align) = (
        i(renderer, "m_RenderMode")?,
        i(renderer, "m_RenderAlignment")?,
    );
    if !(shader == "Sprites/Lit" || shader == "Sprites/Default") || mode != 0 || align != 0 {
        return err(format!(
            "particle renderer material {shader} mode {mode}/{align}"
        ));
    }
    let props = get(&mat, "m_SavedProperties")?;
    let tint = pair(get(props, "m_Colors")?, "_Color")?;
    let tint = [f(tint, "r")?, f(tint, "g")?, f(tint, "b")?, f(tint, "a")?];
    if tint.iter().any(|v| !(0.0..=1.0).contains(v)) {
        return err("particle material tint");
    }
    let tex_ref = get(pair(get(props, "m_TexEnvs")?, "_MainTex")?, "m_Texture")?;
    let texture = source.deref(&mo.file, tex_ref).map_err(|e| e.to_string())?;
    let sid = texture.sid();
    let mut matrix = view.world_of(gid)?;
    let mut ps = ps;
    if unloop {
        // An FSM-driven loop (rate set while a state runs) cooked as its repeating burst.
        set(&mut ps, "looping", Value::Int(0));
    }
    if relaxed {
        (ps, matrix) = secret_relax(&ps, &matrix, emit)?;
    }
    let scale = particle_scale(&ps, &matrix)?;
    let mut st = style(&ps, gravity, scale, played, relaxed)?;
    st.texture = sid.clone();
    for color in st.colors.iter_mut() {
        for (c, v) in color.iter_mut().enumerate() {
            *v = py_round(*v as f64 * tint[c]);
        }
    }
    for sample in st.samples.iter_mut() {
        for v in sample.alpha.iter_mut() {
            *v = py_round(*v as f64 * tint[3]);
        }
    }
    let mut text = String::new();
    crate::pyjson::dumps_sorted(&ps, &mut text);
    let ps_sha256 = Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Ok(Found::Emitter(Box::new(Emitter {
        style: st,
        system: po,
        texture,
        texture_id: sid,
        matrix,
        shader,
        scaling_mode: i(&ps, "scalingMode")?,
        world_parameter_scale: scale,
        ps_sha256,
        part: None,
    })))
}

/// `secret_relax`: a hidden wall's or cracked floor's emitter in the guest's
/// two shapes. Local scaling becomes Shape scaling, a Cone a circle sector of
/// twice its angle about its world axis, a SingleSidedEdge a box turned so its
/// +Z launch is the edge's +Y normal, and local-space force and velocity are
/// taken in world axes. `emit` stands for `ParticleSystem.Emit(n)`.
fn secret_relax(ps: &Value, matrix: &M4, emit: Option<i64>) -> Result<(Value, M4)> {
    let mut ps = ps.clone();
    let capped = i(get(&ps, "InitialModule")?, "maxNumParticles")?.min(POOL_CAPACITY);
    set(
        get_mut(&mut ps, "InitialModule")?,
        "maxNumParticles",
        Value::Int(capped),
    );
    if let Some(n) = emit {
        let length = f(&ps, "lengthInSec")?;
        let em = get_mut(&mut ps, "EmissionModule")?;
        set(em, "enabled", Value::Int(1));
        set(em, "m_BurstCount", Value::Int(0));
        set(
            em,
            "rateOverTime",
            map(vec![
                ("minMaxState", Value::Int(0)),
                ("scalar", Value::F64(n as f64 / length)),
            ]),
        );
        set(
            em,
            "rateOverDistance",
            map(vec![
                ("minMaxState", Value::Int(0)),
                ("scalar", Value::Int(0)),
            ]),
        );
        set(
            get_mut(&mut ps, "InitialModule")?,
            "maxNumParticles",
            Value::Int(n),
        );
    }
    if i(&ps, "scalingMode")? == 1 {
        set(&mut ps, "scalingMode", Value::Int(2));
    }
    for module in ["ForceModule", "VelocityModule"] {
        if truthy(get(&ps, module)?, "enabled")? {
            set(get_mut(&mut ps, module)?, "inWorldSpace", Value::Int(1));
        }
    }
    let mut m = *matrix;
    let shape_type = i(get(&ps, "ShapeModule")?, "type")?;
    if shape_type == 4 {
        let mut axis = [matrix[0][2], matrix[1][2]];
        if !axis.iter().any(|v| v.abs() > 1e-9) {
            axis = [0.0, 1.0];
        }
        let angle_v = get(get(&ps, "ShapeModule")?, "angle")?.clone();
        let angle = if angle_v.is_map() {
            f(&angle_v, "value")?
        } else {
            num(&angle_v)?
        };
        let start = axis[1].atan2(axis[0]).to_degrees() - angle;
        let (c, s) = (start.to_radians().cos(), start.to_radians().sin());
        let sx = crate::pyfloat::hypot(matrix[0][0], matrix[1][0]);
        let sx = if sx == 0.0 { 1.0 } else { sx };
        m = [
            [c * sx, -s * sx, 0.0, matrix[0][3]],
            [s * sx, c * sx, 0.0, matrix[1][3]],
            [0.0, 0.0, 1.0, matrix[2][3]],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let shape = get_mut(&mut ps, "ShapeModule")?;
        set(shape, "type", Value::Int(10));
        let arc = get_mut(shape, "arc")?;
        set(arc, "value", Value::F64(2.0 * angle));
        set(arc, "mode", Value::Int(0));
        if angle_v.is_map() {
            set(get_mut(shape, "angle")?, "value", Value::Int(0));
        }
    } else if shape_type == 12 {
        // (x, y, z) -> (x, z, -y): the box's local +Z is the edge's +Y.
        for (r, row) in m.iter_mut().enumerate().take(3) {
            *row = [matrix[r][0], -matrix[r][2], matrix[r][1], matrix[r][3]];
        }
        m[3] = [0.0, 0.0, 0.0, 1.0];
        let radius = f(get(get(&ps, "ShapeModule")?, "radius")?, "value")?;
        let shape = get_mut(&mut ps, "ShapeModule")?;
        set(shape, "type", Value::Int(5));
        set(
            shape,
            "m_Scale",
            map(vec![
                ("x", Value::F64(2.0 * radius)),
                ("y", Value::F64(0.0)),
                ("z", Value::F64(0.0)),
            ]),
        );
        set(get_mut(shape, "radius")?, "mode", Value::Int(0));
    }
    Ok((ps, m))
}

/// `prefab_emitters`: every emitter a fixed CreateObject prefab instantiates.
#[allow(clippy::too_many_arguments)]
pub(crate) fn prefab_emitters(
    source: &Source,
    file: &Arc<SerializedFile>,
    view_of: &dyn View,
    reference: &Value,
    gravity: f64,
    origin: [f64; 3],
    rotation: Option<[f64; 3]>,
    relaxed: bool,
) -> Result<Vec<Emitter>> {
    let prefab = view_of.deref(source, reference)?;
    let _ = file;
    let tree = source.read(&prefab).map_err(|e| e.to_string())?;
    if tree.get("m_Component").is_none() {
        return err("CreateObject target is not a GameObject");
    }
    let view = PrefabView::new(
        source,
        prefab.file.clone(),
        prefab.path_id(),
        origin,
        rotation,
    )?;
    let mut found = Vec::new();
    for gid in view.subtree()? {
        if let Found::Emitter(mut e) =
            part_emitter(source, &view, gid, gravity, true, relaxed, None, false)?
        {
            e.part = Some(format!("{}:{gid}", hk_unity::base_name(&view.file.name)));
            found.push(*e);
        }
    }
    Ok(found)
}

// ---------------------------------------------------------------- breakables

struct Breakable {
    source: String,
    name: String,
    state_index: usize,
    angle_offset: f64,
    debris: Vec<i64>,
}

/// The part of breakables.py `breakable_sources` the effect cook consumes:
/// every enabled, active Breakable of the scene's own file, with its sorted
/// ordinal among all of them.
fn breakable_sources(
    source: &Source,
    sc: &Scene,
    wanted: &std::collections::HashSet<String>,
) -> Result<Vec<Breakable>> {
    let file = hk_unity::base_name(&sc.base.name).to_string();
    let mut ids: Vec<i64> = Vec::new();
    for info in &sc.base.objects {
        if info.class_id == 114 {
            let o = Obj {
                file: sc.base.clone(),
                info: *info,
            };
            if source.typename(&o).map_err(|e| e.to_string())? == "Breakable" {
                ids.push(info.path_id);
            }
        }
    }
    ids.sort();
    if ids.len() > MAX_SCENE_BREAKABLES {
        return err(format!(
            "Breakable scene state budget exceeded: {} > {MAX_SCENE_BREAKABLES}",
            ids.len()
        ));
    }
    let mut out = Vec::new();
    for (state_index, &index) in ids.iter().enumerate() {
        let source_id = format!("{file}:{index}");
        if !wanted.contains(&source_id) {
            continue;
        }
        let tree = &sc
            .object(index)
            .ok_or("Breakable schema was not successfully read")?
            .tree;
        let gid = local_id(get(tree, "m_GameObject")?)?;
        let mut debris = Vec::new();
        for r in get(tree, "debrisParts")?.list().unwrap_or(&[]) {
            if i(r, "m_PathID")? != 0 {
                debris.push(local_id(r)?);
            }
        }
        out.push(Breakable {
            source: source_id,
            name: get(sc.go(gid).ok_or("no GameObject")?, "m_Name")?
                .str()
                .unwrap_or_default(),
            state_index,
            angle_offset: f(tree, "angleOffset")?,
            debris,
        });
    }
    Ok(out)
}

fn local_id(r: &Value) -> Result<i64> {
    if i(r, "m_FileID")? != 0 {
        return err("external Breakable scene-object reference unsupported");
    }
    i(r, "m_PathID")
}

/// breakables.py `rigid_fragment`: Some when the part is a rigid fling the
/// solver has been shown, None when it is not a rigid fling at all, Err with
/// the reason when it is one the solver has not been shown.
fn rigid_fragment(sc: &Scene, gid: i64) -> Result<bool> {
    let mut components: Vec<(String, &Value)> = Vec::new();
    let go = sc.go(gid).ok_or("no GameObject")?;
    for c in get(go, "m_Component")?.list().unwrap_or(&[]) {
        let id = local_id(get(c, "component")?)?;
        if let Some(o) = sc.object(id) {
            if let Some(slot) = components.iter_mut().find(|x| x.0 == o.typename) {
                slot.1 = &o.tree;
            } else {
                components.push((o.typename.clone(), &o.tree));
            }
        }
    }
    let has = |k: &str| components.iter().any(|c| c.0 == k);
    let comp = |k: &str| components.iter().find(|c| c.0 == k).map(|c| c.1);
    if !has("Rigidbody2D") || !has("SpriteRenderer") {
        return Ok(false);
    }
    let colliders = components
        .iter()
        .filter(|c| c.0.ends_with("Collider2D"))
        .count();
    let spins: Vec<&str> = ["SpinSelf", "SpinSelfSimple"]
        .into_iter()
        .filter(|k| has(k))
        .collect();
    if colliders != 1 {
        return err(format!(
            "rigid fragment needs exactly one collider, has {colliders}"
        ));
    }
    if spins.len() > 1 {
        return err("rigid fragment carries two spin behaviours");
    }
    let Some(bounce) = comp("ObjectBounce") else {
        return err("rigid fragment has no ObjectBounce landing behaviour");
    };
    let body = comp("Rigidbody2D").unwrap();
    let r4 = |v: f64| (v * 10000.0).round_ties_even() / 10000.0;
    let fields = (
        num(get(body, "m_BodyType")?)?,
        num(get(body, "m_Mass")?)?,
        r4(f(body, "m_AngularDamping")?),
        r4(f(body, "m_GravityScale")?),
        num(get(body, "m_Constraints")?)?,
    );
    if truthy(body, "m_UseAutoMass")? || f(body, "m_LinearDamping")? != 0.0 {
        return err("rigid fragment uses auto mass or linear damping");
    }
    let close = |a: f64, b: f64| (a - b).abs() <= 1e-5;
    if !RIGID_BODIES.iter().any(|v| {
        close(fields.0, v.0)
            && close(fields.1, v.1)
            && close(fields.2, v.2)
            && close(fields.3, v.3)
            && close(fields.4, v.4)
    }) {
        return err(format!("unmeasured rigid fragment body {fields:?}"));
    }
    if !RIGID_BOUNCE_FACTORS
        .iter()
        .any(|&v| close(f(bounce, "bounceFactor").unwrap_or(f64::NAN), v))
    {
        return err(format!(
            "unmeasured fragment bounce factor {}",
            crate::pyfloat::repr(f(bounce, "bounceFactor")?)
        ));
    }
    if f(bounce, "speedThreshold")? != 1.0
        || ["playSound", "playAnimationOnBounce", "sendFSMEvent"]
            .iter()
            .any(|k| bounce.get(k).is_some_and(Value::truthy))
    {
        return err("fragment bounce drives sound, animation or an FSM event");
    }
    if spins.first() == Some(&"SpinSelfSimple") {
        let spin = comp("SpinSelfSimple").unwrap();
        if truthy(spin, "randomStartRotation")? || truthy(spin, "waitForCall")? {
            return err("SpinSelfSimple fragment waits for a call or randomizes its start");
        }
    }
    Ok(true)
}

// ---------------------------------------------------------------- secrets

/// One plan entry: (`child`|`create`|`pool`, child name or FSM state, `Emit(n)`).
type PlanEntry = (&'static str, String, Option<i64>);
/// A rectangle request: (key, width, height, x alignment).
type Item = (String, i64, i64, i64);
/// Builds the bank for trimmed styles and their textures.
type BankFn<'a> = dyn FnMut(&[Style], &[String]) -> Result<Bank> + 'a;

fn emitter_plan(family: i64, facing: usize) -> Vec<(i64, Vec<PlanEntry>)> {
    let hit = FACING_STATES[facing].to_string();
    let child = |n: &str, e: Option<i64>| ("child", n.to_string(), e);
    if family == FAMILY_WALL {
        return vec![
            (
                3,
                vec![child("Particle_rocks_small", Some(5)), ("pool", hit, None)],
            ),
            (
                0,
                vec![
                    child("Particle_rocks_large", None),
                    ("create", "Break".into(), None),
                ],
            ),
        ];
    }
    if family == FAMILY_WALL_TK2D {
        return vec![
            (
                3,
                vec![
                    child("Particle_rocks_small", Some(5)),
                    ("create", hit, None),
                ],
            ),
            (
                0,
                vec![
                    child("Dust Break 1", None),
                    child("Dust Break 2", None),
                    child("Particle_rocks_large", None),
                    ("create", "Break".into(), None),
                ],
            ),
        ];
    }
    if family == FAMILY_FLOOR {
        return [1, 2, 0]
            .into_iter()
            .map(|stage| {
                let n = if stage == 0 { 3 } else { stage };
                (
                    stage,
                    ["Dust Hit", "Pt Bits", "Pt Wood"]
                        .iter()
                        .map(|name| child(&format!("{name} {n}"), None))
                        .collect(),
                )
            })
            .collect();
    }
    vec![
        (1, vec![child("Dust Hit 1", None)]),
        (2, vec![child("Dust Hit 2", None)]),
        (
            0,
            vec![
                child("Dust Hit 3", None),
                child("Dust Break 1", None),
                child("Dust Break 2", None),
            ],
        ),
    ]
}

/// Python's repr of a plan entry tuple, as the refusal records it.
fn entry_repr(e: &PlanEntry) -> String {
    let s = |x: &str| format!("'{}'", x.replace('\\', "\\\\").replace('\'', "\\'"));
    match e.0 {
        "child" => format!(
            "({}, {}, {})",
            s(e.0),
            s(&e.1),
            e.2.map_or("None".into(), |n| n.to_string())
        ),
        _ => format!("({}, {})", s(e.0), s(&e.1)),
    }
}

/// secret_breaks.py `_children`: direct children by name, first match.
fn children(sc: &Scene, gid: i64) -> Result<Vec<(String, i64)>> {
    let t = sc
        .transform(*sc.go_transform.get(&gid).ok_or("no transform")?)
        .ok_or("no transform")?;
    let mut out: Vec<(String, i64)> = Vec::new();
    for c in get(t, "m_Children")?.list().unwrap_or(&[]) {
        let ct = sc.transform(i(c, "m_PathID")?).ok_or("child transform")?;
        let kid = i(get(ct, "m_GameObject")?, "m_PathID")?;
        let name = get(sc.go(kid).ok_or("child GameObject")?, "m_Name")?
            .str()
            .unwrap_or_default();
        if !out.iter().any(|(n, _)| *n == name) {
            out.push((name, kid));
        }
    }
    Ok(out)
}

struct Spawn {
    reference: Value,
    origin: [f64; 3],
    rotation: [f64; 3],
}

fn list_at<'a>(data: &'a Value, key: &str, n: usize) -> Result<&'a Value> {
    get(data, key)?
        .list()
        .and_then(|l| l.get(n))
        .ok_or_else(|| format!("{key}[{n}]"))
}
fn int_at(data: &Value, key: &str, n: usize) -> Result<i64> {
    list_at(data, key, n)?
        .int()
        .ok_or_else(|| format!("{key}[{n}]"))
}

/// breakables.py `_action_slots`: parameter name to flat-run index for one action.
fn action_slots(data: &Value, index: usize) -> Result<Vec<(String, usize)>> {
    let names = get(data, "actionNames")?.list().unwrap_or(&[]);
    let params = get(data, "paramName")?.list().unwrap_or(&[]);
    let start = int_at(data, "actionStartIndex", index)? as usize;
    let end = if index + 1 < names.len() {
        int_at(data, "actionStartIndex", index + 1)? as usize
    } else {
        params.len()
    };
    let mut out: Vec<(String, usize)> = Vec::new();
    for (k, param) in params.iter().enumerate().take(end).skip(start) {
        let name = param.str().unwrap_or_default();
        if let Some(slot) = out.iter_mut().find(|s| s.0 == name) {
            slot.1 = k;
        } else {
            out.push((name, k));
        }
    }
    Ok(out)
}
fn slot(slots: &[(String, usize)], name: &str) -> Option<usize> {
    slots.iter().find(|s| s.0 == name).map(|s| s.1)
}

/// breakables.py `_vector_parameter`: one serialized FsmVector3, None when unset.
fn vector_parameter(data: &Value, slot: usize) -> Result<Option<[f64; 3]>> {
    if int_at(data, "paramDataType", slot)? != 28 || int_at(data, "paramByteDataSize", slot)? != 13
    {
        return err("unsupported serialized vector parameter");
    }
    let raw: Vec<u8> = match get(data, "byteData")? {
        Value::Bytes(b) => b.clone(),
        Value::List(l) => l.iter().map(|v| v.int().unwrap_or(0) as u8).collect(),
        _ => return err("byteData"),
    };
    let start = int_at(data, "paramDataPos", slot)?;
    if start < 0 || start as usize + 13 > raw.len() {
        return err("action parameter byte range");
    }
    let s = start as usize;
    if raw[s + 12] != 0 {
        return Ok(None);
    }
    let v: [f64; 3] = std::array::from_fn(|k| {
        f32::from_le_bytes(raw[s + 4 * k..s + 4 * k + 4].try_into().unwrap()) as f64
    });
    if !v.iter().all(|x| x.is_finite()) {
        return err("non-finite action vector");
    }
    Ok(Some(v))
}

/// breakables.py `_owner_variable`: the object variable GetOwner stores.
fn owner_variable(fsm: &Value) -> Result<Option<String>> {
    for state in get(fsm, "states")?.list().unwrap_or(&[]) {
        let data = get(state, "actionData")?;
        for (index, raw) in get(data, "actionNames")?
            .list()
            .unwrap_or(&[])
            .iter()
            .enumerate()
        {
            if !list_at(data, "actionEnabled", index)?.truthy()
                || !raw.str().unwrap_or_default().ends_with(".GetOwner")
            {
                continue;
            }
            let Some(s) = slot(&action_slots(data, index)?, "storeGameObject") else {
                continue;
            };
            if int_at(data, "paramDataType", s)? != 19 {
                continue;
            }
            let stored = list_at(
                data,
                "fsmGameObjectParams",
                int_at(data, "paramDataPos", s)? as usize,
            )?;
            let name = stored.get("name").and_then(Value::str).unwrap_or_default();
            if stored.get("useVariable").is_some_and(Value::truthy) && !name.is_empty() {
                return Ok(Some(name));
            }
        }
    }
    Ok(None)
}

/// breakables.py `_spawn_transform`: where the spawned object lands.
fn spawn_transform(
    sc: &Scene,
    gid: i64,
    fsm: &Value,
    data: &Value,
    slots: &[(String, usize)],
) -> Result<([f64; 3], [f64; 3])> {
    let point = list_at(
        data,
        "fsmGameObjectParams",
        int_at(data, "paramDataPos", slot(slots, "spawnPoint").unwrap())? as usize,
    )?;
    let offset = vector_parameter(data, slot(slots, "position").unwrap())?;
    let rotation = vector_parameter(data, slot(slots, "rotation").unwrap())?;
    if i(get(point, "value")?, "m_PathID")? != 0
        || !point.get("useVariable").is_some_and(Value::truthy)
    {
        return err("the spawn point is not the FSM object variable this port can resolve");
    }
    let owner = owner_variable(fsm)?;
    let name = point.get("name").and_then(Value::str);
    if owner.is_none() || name != owner {
        return err(format!(
            "the spawn point {} is not the object GetOwner stores",
            name.map_or("None".into(), |n| format!("'{n}'"))
        ));
    }
    let mut origin = sc.point(gid, 0.0, 0.0, 0.0).map_err(|e| e.to_string())?;
    if let Some(o) = offset {
        origin = [origin[0] + o[0], origin[1] + o[1], origin[2] + o[2]];
    }
    let Some(rotation) = rotation else {
        return err("the spawn takes its rotation from the spawn point, which is not read");
    };
    Ok((origin, rotation))
}

/// breakables.py `_prefab_particles`: whether a prefab reference carries a ParticleSystem.
fn prefab_particles(source: &Source, sc: &Scene, reference: &Value) -> Result<bool> {
    if i(reference, "m_PathID")? == 0 {
        return Ok(false);
    }
    let prefab = sc.deref(reference).map_err(|e| e.to_string())?;
    let tree = source.read(&prefab).map_err(|e| e.to_string())?;
    let Some(components) = tree.get("m_Component") else {
        return err("CreateObject target is not a GameObject");
    };
    for c in components.list().unwrap_or(&[]) {
        if let Ok(o) = get(c, "component")
            .and_then(|r| source.deref(&prefab.file, r).map_err(|e| e.to_string()))
        {
            if o.class_id() == 198 {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// breakables.py `_create_object_prefabs` (`CreateObject`) and secret_breaks.py
/// `pool_spawn_prefabs` (`SpawnObjectFromGlobalPool`): every enabled spawn of
/// one state whose prefab carries particles, with where it lands.
fn spawn_prefabs(
    source: &Source,
    sc: &Scene,
    gid: i64,
    fsm: &Value,
    state_name: &str,
    pool: bool,
) -> Result<Vec<Spawn>> {
    let action = if pool {
        ".SpawnObjectFromGlobalPool"
    } else {
        ".CreateObject"
    };
    let mut out = Vec::new();
    for state in get(fsm, "states")?.list().unwrap_or(&[]) {
        if get(state, "name")?.str().as_deref() != Some(state_name) {
            continue;
        }
        let data = get(state, "actionData")?;
        for (index, raw) in get(data, "actionNames")?
            .list()
            .unwrap_or(&[])
            .iter()
            .enumerate()
        {
            if !list_at(data, "actionEnabled", index)?.truthy()
                || !raw.str().unwrap_or_default().ends_with(action)
            {
                continue;
            }
            let slots = action_slots(data, index)?;
            if !["gameObject", "spawnPoint", "position", "rotation"]
                .iter()
                .all(|k| slot(&slots, k).is_some())
            {
                return err(if pool {
                    "unsupported serialized SpawnObjectFromGlobalPool parameters"
                } else {
                    "unsupported serialized CreateObject parameters"
                });
            }
            let go_slot = slot(&slots, "gameObject").unwrap();
            if !pool && int_at(data, "paramDataType", go_slot)? != 19 {
                return err("unsupported serialized CreateObject target");
            }
            let reference = list_at(
                data,
                "fsmGameObjectParams",
                int_at(data, "paramDataPos", go_slot)? as usize,
            )?;
            if reference.get("useVariable").is_some_and(Value::truthy) {
                continue;
            }
            let value = get(reference, "value")?;
            if !prefab_particles(source, sc, value)? {
                continue;
            }
            let (origin, rotation) = spawn_transform(sc, gid, fsm, data, &slots)?;
            out.push(Spawn {
                reference: value.clone(),
                origin,
                rotation,
            });
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- collect

#[derive(Clone)]
struct EmitterRow {
    scene: i64,
    owner: i64,
    source: i64,
    style: usize,
    angle_offset: i64,
    origin: [i64; 3],
    basis: [[i64; 3]; 3],
}
impl EmitterRow {
    fn json(&self) -> Json {
        Json::Obj(vec![
            ("scene".into(), Json::Int(self.scene)),
            ("owner".into(), Json::Int(self.owner)),
            ("source".into(), Json::Int(self.source)),
            ("style".into(), Json::Int(self.style as i64)),
            ("angle_offset".into(), Json::Int(self.angle_offset)),
            ("origin".into(), ints(self.origin)),
            (
                "basis".into(),
                Json::List(self.basis.iter().map(|r| ints(*r)).collect()),
            ),
        ])
    }
}
fn placement(m: &M4) -> Result<([i64; 3], [[i64; 3]; 3])> {
    let mut origin = [0; 3];
    let mut basis = [[0; 3]; 3];
    for r in 0..3 {
        origin[r] = q(m[r][3])?;
        for c in 0..3 {
            basis[r][c] = q(m[r][c])?;
        }
    }
    Ok((origin, basis))
}

struct Collected {
    styles: Vec<Style>,
    emitters: Vec<EmitterRow>,
    textures: Vec<(String, Obj)>,
    records: Vec<Json>,
    ignored: Vec<Json>,
}
impl Collected {
    fn style_index(&mut self, st: Style) -> usize {
        match self.styles.iter().position(|s| *s == st) {
            Some(k) => k,
            None => {
                self.styles.push(st);
                self.styles.len() - 1
            }
        }
    }
    fn texture(&mut self, sid: &str, obj: &Obj) {
        if let Some(slot) = self.textures.iter_mut().find(|t| t.0 == sid) {
            slot.1 = obj.clone();
        } else {
            self.textures.push((sid.to_string(), obj.clone()));
        }
    }
}
fn ignore(owner: Json, part: Json, reason: String) -> Json {
    Json::Obj(vec![
        ("owner".into(), owner),
        ("part".into(), part),
        ("reason".into(), Json::Str(reason)),
    ])
}

/// `scene_gravity`: PhysicsManager's gravity, which the particle systems use.
pub(crate) fn scene_gravity(source: &Source) -> Result<f64> {
    let file = source
        .file("globalgamemanagers")
        .map_err(|e| e.to_string())?;
    let info = file
        .objects
        .iter()
        .find(|o| o.class_id == 55)
        .ok_or("no PhysicsManager")?;
    let t = source
        .read(&Obj {
            file: file.clone(),
            info: *info,
        })
        .map_err(|e| e.to_string())?;
    f(get(&t, "m_Gravity")?, "y")
}

fn jstr(v: &J, k: &str) -> String {
    v.get(k).and_then(J::as_str).unwrap_or_default().to_string()
}

fn collect(source: &Source, metadata: &J) -> Result<Collected> {
    let gravity = scene_gravity(source)?;
    let regions = metadata["regions"].as_array().ok_or("no regions")?;
    let wanted: std::collections::HashSet<String> = regions
        .iter()
        .flat_map(|r| r["breakables"].as_array().into_iter().flatten())
        .map(|b| jstr(b, "source"))
        .collect();
    let mut c = Collected {
        styles: Vec::new(),
        emitters: Vec::new(),
        textures: Vec::new(),
        records: Vec::new(),
        ignored: Vec::new(),
    };
    let mut stalactite_scenes: Vec<(i64, String)> = Vec::new();
    for desc in metadata["scenes"].as_array().ok_or("no scenes")? {
        let scene_id = desc["scene_id"].as_i64().ok_or("scene id")?;
        let sc = Scene::new(source, &jstr(desc, "file")).map_err(|e| e.to_string())?;
        let file = hk_unity::base_name(&sc.base.name).to_string();
        for b in breakable_sources(source, &sc, &wanted)? {
            for &gid in &b.debris {
                let part = format!("{file}:{gid}");
                let found = match part_emitter(source, &sc, gid, gravity, false, false, None, false)
                {
                    Ok(x) => x,
                    Err(e) => {
                        c.ignored.push(ignore(
                            Json::Str(b.source.clone()),
                            Json::Str(part),
                            format!("unsupported particle style: {e}"),
                        ));
                        continue;
                    }
                };
                let Found::Emitter(e) = found else {
                    let reason = match rigid_fragment(&sc, gid) {
                        Err(e) => format!("unhandled rigid fragment: {e}"),
                        Ok(true) => "rigid fragment".into(),
                        Ok(false) => "unhandled nonparticle part".into(),
                    };
                    c.ignored
                        .push(ignore(Json::Str(b.source.clone()), Json::Str(part), reason));
                    continue;
                };
                let (origin, basis) = placement(&e.matrix)?;
                let style = c.style_index(e.style.clone());
                c.emitters.push(EmitterRow {
                    scene: scene_id,
                    owner: scene_id * 128 + b.state_index as i64,
                    source: e.system.path_id(),
                    style,
                    angle_offset: q(b.angle_offset)?,
                    origin,
                    basis,
                });
                c.texture(&e.texture_id, &e.texture);
                c.records.push(Json::Obj(vec![
                    ("owner".into(), Json::Str(b.source.clone())),
                    ("name".into(), Json::Str(b.name.clone())),
                    ("part".into(), Json::Str(part)),
                    ("system".into(), Json::Str(e.system.sid())),
                    ("texture".into(), Json::Str(e.texture_id.clone())),
                    ("shader".into(), Json::Str(e.shader.clone())),
                    ("scaling_mode".into(), Json::Int(e.scaling_mode)),
                    (
                        "world_parameter_scale".into(),
                        Json::Float(e.world_parameter_scale),
                    ),
                    ("ps_sha256".into(), Json::Str(e.ps_sha256.clone())),
                ]));
            }
        }
        // {x['source']: x}: the first occurrence's place, the last one's value.
        let mut secrets: Vec<(String, &J)> = Vec::new();
        for region in regions
            .iter()
            .filter(|r| r["scene_id"].as_i64() == Some(scene_id))
        {
            for x in region["secrets"].as_array().into_iter().flatten() {
                let key = jstr(x, "source");
                if let Some(slot) = secrets.iter_mut().find(|s| s.0 == key) {
                    slot.1 = x;
                } else {
                    secrets.push((key, x));
                }
            }
        }
        for (_, secret) in secrets {
            collect_secret(source, &sc, scene_id, secret, gravity, &mut c)?;
        }
        // Only where the props cook places stalactites (host/hk-cook/src/props.rs).
        let stalactites = regions
            .iter()
            .filter(|r| r["scene_id"].as_i64() == Some(scene_id))
            .any(|r| {
                r["hazards"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|h| jstr(h, "name").starts_with("Stalactite"))
            });
        if stalactites {
            stalactite_scenes.push((scene_id, jstr(desc, "file")));
        }
    }
    // After every scene's own emitters, so their style numbers stay put.
    for (scene_id, file) in stalactite_scenes {
        let sc = Scene::new(source, &file).map_err(|e| e.to_string())?;
        collect_stalactites(source, &sc, scene_id, gravity, &mut c)?;
    }
    let scenes: Vec<i64> = metadata["scenes"]
        .as_array()
        .ok_or("no scenes")?
        .iter()
        .filter_map(|d| d["scene_id"].as_i64())
        .collect();
    collect_hero_dust(source, gravity, &scenes, &mut c)?;
    if c.styles.len() > 253 {
        return err("particle style IDs");
    }
    Ok(c)
}

/// The Knight's Focus dust: `Dust L` and `Dust R`, children of the Knight's `Focus Effects`,
/// which the Spell Control FSM sets to 60 particles a second while Focus runs and back to
/// none when it ends. Cooked as the 0.1 s burst of six the loop repeats, in every scene (the
/// Knight can focus anywhere), at the Knight's own place; the guest raises one burst every six
/// ticks while Focus runs, moved to the Knight (game/src/frame.rs). Optional like the
/// stalactites' effects: a scene whose art budget has no room for the dust texture goes without.
pub const HERO_DUST_OWNER: i64 = 0xFFFF;
const HERO_DUST_BURST: i64 = 6;
fn collect_hero_dust(
    source: &Source,
    gravity: f64,
    scenes: &[i64],
    c: &mut Collected,
) -> Result<()> {
    let file = source.file("resources.assets").map_err(|e| e.to_string())?;
    let read = |id: i64| -> Result<Value> {
        let o = source.object(&file, id).map_err(|e| e.to_string())?;
        source.read(&o).map_err(|e| e.to_string())
    };
    let transform_of = |gid: i64| -> Result<Value> {
        for comp in get(&read(gid)?, "m_Component")?.list().unwrap_or(&[]) {
            let o = source
                .deref(&file, get(comp, "component")?)
                .map_err(|e| e.to_string())?;
            if o.class_id() == 4 {
                return source.read(&o).map_err(|e| e.to_string());
            }
        }
        err("no Transform")
    };
    const DUST: [(i64, &str); 2] = [(4650, "Dust L"), (5097, "Dust R")];
    let father = i(get(&transform_of(DUST[0].0)?, "m_Father")?, "m_PathID")?;
    let focus_effects = i(get(&read(father)?, "m_GameObject")?, "m_PathID")?;
    let mut found = Vec::new();
    {
        let view = PrefabView::new(
            source,
            file.clone(),
            focus_effects,
            [0.0, 0.0, 0.0],
            Some([0.0, 0.0, 0.0]),
        )?;
        for (gid, name) in DUST {
            if get(&read(gid)?, "m_Name")?.str().as_deref() != Some(name) {
                return err(format!("Focus dust {gid} is not {name}"));
            }
            match part_emitter(
                source,
                &view,
                gid,
                gravity,
                true,
                true,
                Some(HERO_DUST_BURST),
                true,
            )? {
                Found::Emitter(e) => found.push(*e),
                _ => return err(format!("{name} is not a particle system")),
            }
        }
    }
    for &scene in scenes {
        for e in &found {
            let style = c.style_index(e.style.clone());
            let (origin, basis) = placement(&e.matrix)?;
            c.emitters.push(EmitterRow {
                scene,
                owner: HERO_DUST_OWNER,
                source: e.system.path_id(),
                style,
                angle_offset: 0,
                origin,
                basis,
            });
            c.texture(&e.texture_id, &e.texture);
        }
    }
    for e in &found {
        c.records.push(Json::Obj(vec![
            ("owner".into(), Json::Str("Knight / Focus Effects".into())),
            (
                "name".into(),
                Json::Str("Dust L and Dust R, per Focus tick".into()),
            ),
            (
                "part".into(),
                Json::Str(format!("resources.assets:{}", e.system.path_id())),
            ),
            ("system".into(), Json::Str(e.system.sid())),
            ("texture".into(), Json::Str(e.texture_id.clone())),
            ("shader".into(), Json::Str(e.shader.clone())),
            ("scaling_mode".into(), Json::Int(e.scaling_mode)),
            ("ps_sha256".into(), Json::Str(e.ps_sha256.clone())),
        ]));
    }
    Ok(())
}

/// Stalactite effect owners: `STALACTITE_OWNER | slot << 2 | kind`, the slot
/// being the stalactite's order in its scene (game/src/props.rs) and the kind
/// 0 for an upward slash's `hitUpEffectPrefabs`, 1 for `landEffectPrefabs`, 2 for
/// the fallen version's Breakable debris.
pub const STALACTITE_OWNER: i64 = 0xC000;

/// StalactiteControl's particle prefabs, spawned with `Spawn(prefab, position)`
/// at the stalactite: cooked at its hanging position, and moved by the guest
/// to where it is when it breaks or lands. Like a secret's, they run through
/// `secret_relax` (their dust is a cone).
fn collect_stalactites(
    source: &Source,
    sc: &Scene,
    scene_id: i64,
    gravity: f64,
    c: &mut Collected,
) -> Result<()> {
    let mut slot = 0;
    for o in &sc.objects {
        if o.typename != "StalactiteControl" || !truthy(&o.tree, "m_Enabled")? {
            continue;
        }
        let gid = i(get(&o.tree, "m_GameObject")?, "m_PathID")?;
        if !sc.active(gid) {
            continue;
        }
        let origin = sc.point(gid, 0.0, 0.0, 0.0).map_err(|e| e.to_string())?;
        for (kind, field) in [(0, "hitUpEffectPrefabs"), (1, "landEffectPrefabs")] {
            for reference in get(&o.tree, field)?.list().unwrap_or(&[]) {
                if i(reference, "m_PathID")? == 0 {
                    continue;
                }
                let owner = STALACTITE_OWNER | slot << 2 | kind;
                let found = match prefab_emitters(
                    source, &sc.base, sc, reference, gravity, origin, None, true,
                ) {
                    Ok(found) => found,
                    Err(e) => {
                        c.ignored.push(ignore(
                            Json::Str(sc.sid(o.id)),
                            Json::Str(field.into()),
                            format!("unsupported stalactite particle: {e}"),
                        ));
                        continue;
                    }
                };
                for e in found {
                    let style = c.style_index(e.style.clone());
                    let (origin, basis) = placement(&e.matrix)?;
                    c.emitters.push(EmitterRow {
                        scene: scene_id,
                        owner,
                        source: e.system.path_id(),
                        style,
                        angle_offset: 0,
                        origin,
                        basis,
                    });
                    c.texture(&e.texture_id, &e.texture);
                    c.records.push(Json::Obj(vec![
                        ("owner".into(), Json::Str(sc.sid(o.id))),
                        ("name".into(), Json::Str(field.into())),
                        ("part".into(), Json::Str(e.part.clone().unwrap_or_default())),
                        ("system".into(), Json::Str(e.system.sid())),
                        ("texture".into(), Json::Str(e.texture_id.clone())),
                        ("shader".into(), Json::Str(e.shader.clone())),
                        ("scaling_mode".into(), Json::Int(e.scaling_mode)),
                        ("ps_sha256".into(), Json::Str(e.ps_sha256.clone())),
                    ]));
                }
            }
        }
        // Kind 2: the fallen stalactite is its `Embedded` child, a Breakable
        // whose debris parts a nail hit breaks into (cooked at the child's
        // place, moved by the guest to where the stalactite lies).
        if let Some(embedded) = kid(&kids(sc, gid).map_err(|e| e.to_string())?, "Embedded") {
            let owner = STALACTITE_OWNER | slot << 2 | 2;
            for record in component_records(sc, embedded)
                .into_iter()
                .filter(|r| r.1 == "Breakable")
            {
                let angle_offset = f(record.2, "angleOffset")?;
                for r in get(record.2, "debrisParts")?.list().unwrap_or(&[]) {
                    if i(r, "m_PathID")? == 0 {
                        continue;
                    }
                    let part = i(r, "m_PathID")?;
                    let found =
                        match part_emitter(source, sc, part, gravity, false, false, None, false) {
                            Ok(Found::Emitter(e)) => e,
                            Ok(_) => continue,
                            Err(e) => {
                                c.ignored.push(ignore(
                                    Json::Str(sc.sid(o.id)),
                                    Json::Str("embedded debris".into()),
                                    format!("unsupported stalactite particle: {e}"),
                                ));
                                continue;
                            }
                        };
                    let (origin, basis) = placement(&found.matrix)?;
                    let style = c.style_index(found.style.clone());
                    c.emitters.push(EmitterRow {
                        scene: scene_id,
                        owner,
                        source: found.system.path_id(),
                        style,
                        angle_offset: q(angle_offset)?,
                        origin,
                        basis,
                    });
                    c.texture(&found.texture_id, &found.texture);
                    c.records.push(Json::Obj(vec![
                        ("owner".into(), Json::Str(sc.sid(o.id))),
                        (
                            "name".into(),
                            Json::Str("embedded Breakable debrisParts".into()),
                        ),
                        (
                            "part".into(),
                            Json::Str(format!("{}:{part}", hk_unity::base_name(&sc.base.name))),
                        ),
                        ("system".into(), Json::Str(found.system.sid())),
                        ("texture".into(), Json::Str(found.texture_id.clone())),
                        ("shader".into(), Json::Str(found.shader.clone())),
                        ("scaling_mode".into(), Json::Int(found.scaling_mode)),
                        ("ps_sha256".into(), Json::Str(found.ps_sha256.clone())),
                    ]));
                }
            }
        }
        slot += 1;
    }
    Ok(())
}

fn collect_secret(
    source: &Source,
    sc: &Scene,
    scene_id: i64,
    secret: &J,
    gravity: f64,
    c: &mut Collected,
) -> Result<()> {
    let wanted = jstr(secret, "source");
    let fsm_object = sc
        .objects
        .iter()
        .filter(|o| o.typename == "PlayMakerFSM")
        .rfind(|o| sc.sid(o.id) == wanted)
        .ok_or("secret FSM")?;
    let gid = i(get(&fsm_object.tree, "m_GameObject")?, "m_PathID")?;
    let fsm = get(&fsm_object.tree, "fsm")?;
    let base = scene_id * 128 + secret["state_index"].as_i64().ok_or("state_index")?;
    let family = secret["family"].as_i64().ok_or("family")?;
    let facing = secret["facing"].as_i64().ok_or("facing")? as usize;
    for (stage, entries) in emitter_plan(family, facing) {
        let mut found: Vec<(Emitter, Option<i64>, String)> = Vec::new();
        for entry in &entries {
            let step = (|| -> Result<()> {
                if entry.0 == "child" {
                    let kids = children(sc, gid)?;
                    let Some(&(_, child)) = kids.iter().find(|k| k.0 == entry.1) else {
                        return Ok(());
                    };
                    if !sc.active(child) {
                        return Ok(());
                    }
                    if let Found::Emitter(e) =
                        part_emitter(source, sc, child, gravity, true, true, entry.2, false)?
                    {
                        found.push((*e, entry.2, sc.sid(child)));
                    }
                } else {
                    for prefab in spawn_prefabs(source, sc, gid, fsm, &entry.1, entry.0 == "pool")?
                    {
                        for e in prefab_emitters(
                            source,
                            &sc.base,
                            sc,
                            &prefab.reference,
                            gravity,
                            prefab.origin,
                            Some(prefab.rotation),
                            true,
                        )? {
                            let part = e.part.clone().unwrap_or_default();
                            found.push((e, None, part));
                        }
                    }
                }
                Ok(())
            })();
            if let Err(e) = step {
                c.ignored.push(ignore(
                    Json::Str(wanted.clone()),
                    Json::Str(entry_repr(entry)),
                    format!("unsupported secret particle: {e}"),
                ));
            }
        }
        if found.is_empty() {
            continue;
        }
        let counts: Vec<i64> = found
            .iter()
            .map(|(e, emit, _)| emit.unwrap_or(e.style.count))
            .collect();
        let budget = if stage == 0 {
            SECRET_BREAK_BUDGET
        } else {
            SECRET_HIT_BUDGET
        };
        let factor = (budget as f64 / counts.iter().sum::<i64>() as f64).min(1.0);
        for ((e, emit, part), count) in found.iter().zip(&counts) {
            let mut st = e.style.clone();
            let kept = ((*count as f64 * factor).floor() as i64).max(1);
            let rate = if emit.is_some() {
                EMIT_RATE
            } else {
                st.rate as f64 / 65536.0 * kept as f64 / st.count as f64
            };
            st.count = kept;
            st.rate = q(rate)?;
            let style = c.style_index(st);
            let (origin, basis) = placement(&e.matrix)?;
            c.emitters.push(EmitterRow {
                scene: scene_id,
                owner: base | stage << 13,
                source: e.system.path_id(),
                style,
                angle_offset: 0,
                origin,
                basis,
            });
            c.texture(&e.texture_id, &e.texture);
            c.records.push(Json::Obj(vec![
                ("owner".into(), Json::Str(wanted.clone())),
                ("name".into(), Json::Str(jstr(secret, "name"))),
                ("stage".into(), Json::Int(stage)),
                ("part".into(), Json::Str(part.clone())),
                ("system".into(), Json::Str(e.system.sid())),
                ("texture".into(), Json::Str(e.texture_id.clone())),
                ("shader".into(), Json::Str(e.shader.clone())),
                ("authored_count".into(), Json::Int(*count)),
                ("kept_count".into(), Json::Int(kept)),
                ("scaling_mode".into(), Json::Int(e.scaling_mode)),
                ("ps_sha256".into(), Json::Str(e.ps_sha256.clone())),
            ]));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- art

/// geo.py `place_rectangles`: bounded deterministic MaxRects search; CLUT X
/// remains 16-word aligned.
fn place_rectangles(
    items: &[Item],
    rectangles: &[(i64, i64, i64, i64)],
) -> Result<HashMap<String, (i64, i64, i64, i64)>> {
    for (n, &(x, y, w, h)) in rectangles.iter().enumerate() {
        if !(0 <= x && x < x + w && x + w <= 1024 && 0 <= y && y < y + h && y + h <= 512) {
            return err("Geo rectangle outside VRAM");
        }
        if rectangles[..n]
            .iter()
            .any(|&(ox, oy, ow, oh)| x < ox + ow && x + w > ox && y < oy + oh && y + h > oy)
        {
            return err("Overlapping Geo rectangles");
        }
    }
    let keys: std::collections::HashSet<&String> = items.iter().map(|i| &i.0).collect();
    if keys.len() != items.len() || items.iter().any(|&(_, w, h, a)| w <= 0 || h <= 0 || a <= 0) {
        return err("invalid Geo allocations");
    }
    let total: i64 = items.iter().map(|&(_, w, h, _)| w * h * 2).sum();
    if total > MAX_VRAM_BYTES {
        return err("Geo VRAM exceeds8KiB");
    }
    type Key = (i64, i64, String);
    let orders: [fn(&Item) -> Key; 3] = [
        |v| (-v.2, -v.1, v.0.clone()),
        |v| (-v.1 * v.2, -v.2, v.0.clone()),
        |v| (-v.1, -v.2, v.0.clone()),
    ];
    for order in orders {
        for mode in 0..2 {
            let mut free: Vec<(i64, i64, i64, i64)> = rectangles.to_vec();
            let mut placed: HashMap<String, (i64, i64, i64, i64)> = HashMap::new();
            let mut sorted: Vec<&(String, i64, i64, i64)> = items.iter().collect();
            sorted.sort_by_key(|v| order(v));
            for &(ref key, w, h, alignment) in sorted {
                let mut candidates: Vec<(i64, i64, i64, i64)> = Vec::new();
                for &(rx, ry, rw, rh) in &free {
                    let x = (rx + alignment - 1).div_euclid(alignment) * alignment;
                    if x + w > rx + rw || h > rh {
                        continue;
                    }
                    let (dw, dh) = (rx + rw - x - w, rh - h);
                    let score = if mode == 0 {
                        (dw.min(dh), dw.max(dh))
                    } else {
                        (rw * rh - w * h, dw.min(dh))
                    };
                    candidates.push((score.0, score.1, ry, x));
                }
                let Some(&(_, _, y, x)) = candidates.iter().min() else {
                    break;
                };
                placed.insert(key.clone(), (x, y, w, h));
                let mut new = Vec::new();
                for &(rx, ry, rw, rh) in &free {
                    if x >= rx + rw || x + w <= rx || y >= ry + rh || y + h <= ry {
                        new.push((rx, ry, rw, rh));
                        continue;
                    }
                    if x > rx {
                        new.push((rx, ry, x - rx, rh));
                    }
                    if x + w < rx + rw {
                        new.push((x + w, ry, rx + rw - x - w, rh));
                    }
                    if y > ry {
                        new.push((rx, ry, rw, y - ry));
                    }
                    if y + h < ry + rh {
                        new.push((rx, y + h, rw, ry + rh - y - h));
                    }
                }
                let mut unique = new.clone();
                unique.sort();
                unique.dedup();
                free = unique
                    .into_iter()
                    .filter(|r| {
                        !new.iter().any(|o| {
                            r != o
                                && o.0 <= r.0
                                && o.1 <= r.1
                                && o.0 + o.2 >= r.0 + r.2
                                && o.1 + o.3 >= r.1 + r.3
                        })
                    })
                    .collect();
            }
            if placed.len() == items.len() {
                return Ok(placed);
            }
        }
    }
    let sizes: Vec<String> = items
        .iter()
        .map(|&(_, w, h, _)| format!("({w}, {h})"))
        .collect();
    err(format!(
        "Geo textures do not fit reserved VRAM fragments: {total} bytes [{}]",
        sizes.join(", ")
    ))
}

struct Bank {
    blob: Vec<u8>,
    uploads: Vec<[i64; 5]>,
    art: Vec<[i64; 6]>,
    frames: Vec<(String, Vec<[i64; 3]>)>,
    sheet: Image,
}

fn target_size(size: f64) -> i64 {
    (size / 65536.0 * focal() / -CAM_Z).ceil() as i64
}

/// `art_bank`: every style's cells cut from its source sheet (`textures`, in
/// first-use order) at its projected size, one joint palette per opacity level.
fn art_bank(styles: &[Style], textures: &[(String, Arc<Image>)]) -> Result<Bank> {
    let mut cells: Vec<Image> = Vec::new();
    let mut families: Vec<(String, usize, usize)> = Vec::new();
    for (sid, im) in textures {
        let group: Vec<&Style> = styles.iter().filter(|s| &s.texture == sid).collect();
        let mut rows: Vec<i64> = group.iter().map(|s| s.cells).collect();
        rows.sort();
        rows.dedup();
        if rows.len() != 1 {
            return err("mixed sheet grid");
        }
        let count = rows[0] as usize;
        let biggest = group
            .iter()
            .map(|s| s.size[0].max(s.size[1]))
            .max()
            .unwrap_or(0);
        let target = target_size(biggest as f64) as usize;
        let (w, h) = (im.width, im.height / count);
        let start = cells.len();
        for n in 0..count {
            let cell = im.crop_int(
                0,
                ((count - 1 - n) * h) as i64,
                w as i64,
                ((count - n) * h) as i64,
            );
            cells.push(cell.resize(target, target, Filter::Lanczos));
        }
        families.push((sid.clone(), start, count));
    }
    let mut sheet = Image::new(Mode::Rgba, 128, cells.len().div_ceil(4) * 32);
    if cells.iter().any(|c| c.width > 32 || c.height > 32) {
        return err("particle art exceeds source-projection bank");
    }
    for (n, c) in cells.iter().enumerate() {
        sheet.paste(c, ((n % 4) * 32) as i64, ((n / 4) * 32) as i64, None);
    }
    let mut palettes: Vec<[u8; 32]> = Vec::new();
    let mut planes: Vec<(i64, i64, Vec<u8>)> = Vec::new();
    let mut mapping: Vec<(usize, usize, i64, i64)> = Vec::new();
    for alpha in ALPHAS {
        let qz = quantize_alpha_coverage(&sheet, alpha)?;
        if !palettes.contains(&qz.palette) {
            palettes.push(qz.palette);
        }
        let pal = palettes.iter().position(|p| *p == qz.palette).unwrap();
        for (n, c) in cells.iter().enumerate() {
            let stride = c.width.div_ceil(4) * 2;
            let mut data = vec![0u8; stride * c.height];
            for y in 0..c.height {
                for x in 0..c.width {
                    let (sx, sy) = ((n % 4) * 32 + x, (n / 4) * 32 + y);
                    let index =
                        (qz.packed[sy * qz.width.div_ceil(2) + sx / 2] >> (4 * (sx & 1))) & 15;
                    data[y * stride + x / 2] |= index << (4 * (x & 1));
                }
            }
            let item = ((stride / 2) as i64, c.height as i64, data);
            if !planes.contains(&item) {
                planes.push(item.clone());
            }
            let plane = planes.iter().position(|p| *p == item).unwrap();
            mapping.push((plane, pal, c.width as i64, c.height as i64));
        }
    }
    let mut items: Vec<(String, i64, i64, i64)> = (0..palettes.len())
        .map(|n| (format!("p{n}"), 16, 1, 16))
        .collect();
    items.extend(
        planes
            .iter()
            .enumerate()
            .map(|(n, p)| (format!("t{n}"), p.0, p.1, 1)),
    );
    let allocated = place_rectangles(&items, &VRAM_RECTS)?;
    let (mut blob, mut uploads) = (Vec::new(), Vec::new());
    for (key, ..) in &items {
        let (x, y, w, h) = allocated[key];
        let n: usize = key[1..].parse().unwrap();
        uploads.push([blob.len() as i64, x, y, w, h]);
        if key.starts_with('p') {
            blob.extend_from_slice(&palettes[n]);
        } else {
            blob.extend_from_slice(&planes[n].2);
        }
    }
    let art = mapping
        .iter()
        .map(|&(t, p, w, h)| {
            let (x, y, ..) = allocated[&format!("t{t}")];
            let (px, py, ..) = allocated[&format!("p{p}")];
            [
                (x % 64) * 4,
                y % 256,
                w,
                h,
                (py << 6) | (px >> 4),
                (x / 64) | ((y / 256) << 4),
            ]
        })
        .collect();
    let total = cells.len();
    let frames = families
        .into_iter()
        .map(|(sid, start, count)| {
            (
                sid,
                (0..count)
                    .map(|n| std::array::from_fn(|level| (level * total + start + n) as i64))
                    .collect(),
            )
        })
        .collect();
    Ok(Bank {
        blob,
        uploads,
        art,
        frames,
        sheet,
    })
}

fn style_art_cost(st: &Style) -> i64 {
    let target = target_size(st.size[0].max(st.size[1]).max(1) as f64);
    st.cells * target * target
}

struct SceneArt {
    styles: Vec<Style>,
    emitters: Vec<EmitterRow>,
    bank: Option<Bank>,
}

/// `scene_art`: one scene's trimmed styles, emitters and art bank inside the
/// fixed VRAM reservation, dropping the costliest style while the bank overflows.
/// `bank` builds the art for the trimmed styles and their textures (sids in
/// first-use order).
fn scene_art(
    scene: i64,
    styles: &[Style],
    all: &[EmitterRow],
    ignored: &mut Vec<Json>,
    bank: &mut BankFn,
) -> Result<SceneArt> {
    let emitters: Vec<&EmitterRow> = all.iter().filter(|e| e.scene == scene).collect();
    let used: std::collections::HashSet<usize> = emitters.iter().map(|e| e.style).collect();
    // A style only stalactite emitters use is added after the scene's own art
    // is settled, and only where it still fits, so it never displaces a style
    // the scene had.
    let optional = |n: usize| {
        emitters
            .iter()
            .filter(|e| e.style == n)
            .all(|e| e.owner >= STALACTITE_OWNER)
    };
    let mut kept: Vec<usize> = (0..styles.len())
        .filter(|&n| used.contains(&n) && !optional(n))
        .collect();
    let extra: Vec<usize> = (0..styles.len())
        .filter(|&n| used.contains(&n) && optional(n))
        .collect();
    let build =
        |kept: &[usize], bank: &mut BankFn| -> Result<std::result::Result<SceneArt, String>> {
            let trimmed: Vec<Style> = kept.iter().map(|&n| styles[n].clone()).collect();
            let trimmed_emitters: Vec<EmitterRow> = emitters
                .iter()
                .filter_map(|e| {
                    kept.iter()
                        .position(|&k| k == e.style)
                        .map(|new| EmitterRow {
                            style: new,
                            ..(*e).clone()
                        })
                })
                .collect();
            if trimmed_emitters.is_empty() {
                return Ok(Ok(SceneArt {
                    styles: Vec::new(),
                    emitters: Vec::new(),
                    bank: None,
                }));
            }
            let mut textures: Vec<String> = Vec::new();
            for s in &trimmed {
                if !textures.contains(&s.texture) {
                    textures.push(s.texture.clone());
                }
            }
            match bank(&trimmed, &textures) {
                Ok(b) => Ok(Ok(SceneArt {
                    styles: trimmed,
                    emitters: trimmed_emitters,
                    bank: Some(b),
                })),
                Err(error) if error.contains("VRAM") => Ok(Err(error)),
                Err(error) => Err(error),
            }
        };
    let mut result = SceneArt {
        styles: Vec::new(),
        emitters: Vec::new(),
        bank: None,
    };
    while !kept.is_empty() {
        match build(&kept, bank)? {
            Ok(art) => {
                result = art;
                break;
            }
            Err(error) => {
                let dropped = *kept
                    .iter()
                    .max_by_key(|&&n| (style_art_cost(&styles[n]), n))
                    .unwrap();
                for e in &emitters {
                    if e.style == dropped {
                        ignored.push(ignore(
                            Json::Int(e.owner),
                            Json::Int(e.source),
                            format!("particle art VRAM budget: style {dropped} dropped from scene {scene} ({error})"),
                        ));
                    }
                }
                kept.retain(|&n| n != dropped);
            }
        }
    }
    for n in extra {
        let reason = if kept.len() >= FX_LIMITS[0].1 {
            Some(format!(
                "particle style budget: style {n} dropped from scene {scene}"
            ))
        } else {
            let mut with = kept.clone();
            with.push(n);
            with.sort();
            match build(&with, bank)? {
                Ok(art) => {
                    result = art;
                    kept = with;
                    None
                }
                Err(error) => Some(format!(
                    "particle art VRAM budget: style {n} dropped from scene {scene} ({error})"
                )),
            }
        };
        if let Some(reason) = reason {
            for e in emitters.iter().filter(|e| e.style == n) {
                ignored.push(ignore(
                    Json::Int(e.owner),
                    Json::Int(e.source),
                    reason.clone(),
                ));
            }
        }
    }
    Ok(result)
}

// ---------------------------------------------------------------- output

fn pack_scene_effects(data: &SceneArt, curves: &mut Vec<String>) -> Result<Vec<u8>> {
    let empty_frames = Vec::new();
    let bank = data.bank.as_ref();
    let mut frames: Vec<[i64; 3]> = Vec::new();
    let mut style_records = Vec::new();
    for st in &data.styles {
        let key = st.samples_key();
        let curve = match curves.iter().position(|k| *k == key) {
            Some(n) => n,
            None => {
                curves.push(key);
                curves.len() - 1
            }
        };
        let cells = bank
            .and_then(|b| b.frames.iter().find(|f| f.0 == st.texture))
            .map_or(&empty_frames, |f| &f.1);
        let first = frames.len();
        frames.extend(cells.iter().copied());
        let mut r = Vec::with_capacity(168);
        let h = |r: &mut Vec<u8>, v: i64| r.extend_from_slice(&(v as u16).to_le_bytes());
        let w = |r: &mut Vec<u8>, v: i64| r.extend_from_slice(&(v as i32).to_le_bytes());
        for v in st.life {
            h(&mut r, v);
        }
        for v in st.speed.iter().chain(&st.size).chain(&st.rotation) {
            w(&mut r, *v);
        }
        for v in st.colors.iter().flatten() {
            r.push(*v as u8);
        }
        h(&mut r, st.count);
        w(&mut r, st.rate);
        r.push(st.shape as u8);
        r.extend(st.start_alpha.iter().map(|&v| v as u8));
        r.push(0);
        w(&mut r, st.radius);
        w(&mut r, st.arc);
        for v in st
            .shape_scale
            .iter()
            .chain(st.force.iter().flatten())
            .chain(st.velocity.iter().flatten())
        {
            w(&mut r, *v);
        }
        w(&mut r, st.limit);
        w(&mut r, st.dampen);
        for v in st.spin_speed.iter().chain(&st.spin_range) {
            w(&mut r, *v);
        }
        r.extend_from_slice(&(st.collision as u32).to_le_bytes());
        for v in [
            st.bounce,
            st.collision_dampen,
            st.life_loss,
            st.kill_speed,
            st.radius_scale,
        ] {
            w(&mut r, v);
        }
        for v in [curve as i64, first as i64, cells.len() as i64, 0] {
            h(&mut r, v);
        }
        debug_assert_eq!(r.len(), 168);
        style_records.push(r);
    }
    let (art, uploads): (&[[i64; 6]], &[[i64; 5]]) =
        bank.map_or((&[], &[]), |b| (&b.art, &b.uploads));
    let counts = [
        data.styles.len(),
        data.emitters.len(),
        art.len(),
        uploads.len(),
        frames.len(),
    ];
    for ((name, limit), n) in FX_LIMITS.iter().zip(counts) {
        if n > *limit {
            return err(format!(
                "scene effect {name} {n} exceed the guest capacity {limit}"
            ));
        }
    }
    let mut out = FX_MAGIC.to_vec();
    for n in counts.iter().chain(&[0]) {
        out.extend_from_slice(&(*n as u32).to_le_bytes());
    }
    for r in style_records {
        out.extend(r);
    }
    for e in &data.emitters {
        out.push(e.scene as u8);
        out.push(0);
        out.extend_from_slice(&(e.owner as u16).to_le_bytes());
        out.extend_from_slice(&(e.source as u32).to_le_bytes());
        out.push(e.style as u8);
        out.extend_from_slice(&[0, 0, 0]);
        out.extend_from_slice(&(e.angle_offset as i32).to_le_bytes());
        for v in e.origin.iter().chain(e.basis.iter().flatten()) {
            out.extend_from_slice(&(*v as i32).to_le_bytes());
        }
    }
    for a in art {
        out.extend(a[..4].iter().map(|&v| v as u8));
        out.extend_from_slice(&(a[4] as u16).to_le_bytes());
        out.extend_from_slice(&(a[5] as u16).to_le_bytes());
    }
    let mut texel_base = out.len() + uploads.len() * 12 + frames.len() * 6;
    texel_base += (4 - texel_base % 4) % 4;
    for u in uploads {
        out.extend_from_slice(&((u[0] as usize + texel_base) as u32).to_le_bytes());
        for v in &u[1..] {
            out.extend_from_slice(&(*v as u16).to_le_bytes());
        }
    }
    for fr in &frames {
        for v in fr {
            out.extend_from_slice(&(*v as u16).to_le_bytes());
        }
    }
    out.resize(texel_base, 0);
    if let Some(b) = bank {
        out.extend_from_slice(&b.blob);
    }
    out.resize(out.len().div_ceil(4) * 4, 0);
    out[28..32].copy_from_slice(&(texel_base as u32).to_le_bytes());
    Ok(out)
}

/// scene_bank.py `fnv`.
pub(crate) fn fnv(data: &[u8]) -> u32 {
    data.iter().fold(0x811c9dc5u32, |v, &b| {
        (v ^ b as u32).wrapping_mul(0x01000193)
    })
}
/// region_delta.py `compressed`: HLZC with LZ4 HC level 9, or the raw bytes
/// when that is no smaller.
pub(crate) fn compressed(data: &[u8]) -> Vec<u8> {
    let mut packed = b"HLZC".to_vec();
    packed.extend_from_slice(&(data.len() as u32).to_le_bytes());
    packed.extend(hk_lz4::compress_hc(data));
    if packed.len() < data.len() {
        packed
    } else {
        data.to_vec()
    }
}

struct Chunk {
    scene_id: i64,
    raw_len: usize,
    raw_fnv: u32,
    stored_len: usize,
    stored_fnv: u32,
    path: String,
    raw_sha256: String,
}

fn generate(curves: &[String], manifest: &[Chunk]) -> Result<String> {
    let mut out = vec![
        "// Generated from Windows source; source IDs/hashes in ignored break-effects report."
            .to_string(),
    ];
    for (n, key) in curves.iter().enumerate() {
        let samples: Vec<J> = serde_json::from_str(key).map_err(|e| e.to_string())?;
        let l = |v: &J| {
            format!(
                "[{}]",
                v.as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            )
        };
        let text: Vec<String> = samples
            .iter()
            .map(|s| {
                format!(
                    "Sample{{size:{},alpha:{},spin:{}}}",
                    l(&s["size"]),
                    l(&s["alpha"]),
                    l(&s["spin"])
                )
            })
            .collect();
        out.push(format!("static CURVE_{n}:&[Sample]=&[{}];", text.join(",")));
    }
    out.push(format!(
        "pub static CURVES:&[&[Sample]]=&[{}];",
        (0..curves.len())
            .map(|n| format!("CURVE_{n}"))
            .collect::<Vec<_>>()
            .join(",")
    ));
    for (name, limit) in FX_LIMITS {
        out.push(format!(
            "pub const FX_MAX_{}:usize={limit};",
            name.to_uppercase()
        ));
    }
    out.push(format!(
        "pub static EFFECT_ART_MANIFEST:&[EffectArtDesc]=&[{}];",
        manifest
            .iter()
            .map(|m| format!(
                "EffectArtDesc{{raw_len:{},raw_fnv:{},stored_len:{},stored_fnv:{}}}",
                m.raw_len, m.raw_fnv, m.stored_len, m.stored_fnv
            ))
            .collect::<Vec<_>>()
            .join(",")
    ));
    out.push("pub static EMITTER_TRACK_BASES:&[u16]=&[];".into());
    out.push("pub static PARTICLE_TRACKS:&[[[u32;2];33]]=&[];".into());
    Ok(out.join("\n") + "\n")
}

fn sha_hex(b: &[u8]) -> String {
    Sha256::digest(b)
        .iter()
        .map(|x| format!("{x:02x}"))
        .collect()
}

fn scene_json(data: &SceneArt) -> Json {
    let bank = data.bank.as_ref();
    let fields = vec![
        (
            "styles".into(),
            Json::List(data.styles.iter().map(Style::json).collect()),
        ),
        (
            "emitters".into(),
            Json::List(data.emitters.iter().map(EmitterRow::json).collect()),
        ),
        (
            "uploads".into(),
            Json::List(bank.map_or(Vec::new(), |b| {
                b.uploads
                    .iter()
                    .map(|u| {
                        Json::Obj(
                            ["offset", "x", "y", "w", "h"]
                                .iter()
                                .zip(u)
                                .map(|(k, v)| (k.to_string(), Json::Int(*v)))
                                .collect(),
                        )
                    })
                    .collect()
            })),
        ),
        (
            "art".into(),
            Json::List(bank.map_or(Vec::new(), |b| {
                b.art
                    .iter()
                    .map(|a| {
                        Json::Obj(
                            ["u", "v", "w", "h", "clut", "tpage"]
                                .iter()
                                .zip(a)
                                .map(|(k, v)| (k.to_string(), Json::Int(*v)))
                                .collect(),
                        )
                    })
                    .collect()
            })),
        ),
        (
            "frames".into(),
            Json::Obj(bank.map_or(Vec::new(), |b| {
                b.frames
                    .iter()
                    .map(|(sid, cells)| {
                        (
                            sid.clone(),
                            Json::List(cells.iter().map(|c| ints(*c)).collect()),
                        )
                    })
                    .collect()
            })),
        ),
    ];
    Json::Obj(fields)
}

/// The cook: writes data/break_effects.rs and .hkpsx/break-effects/ under
/// `root` and returns the summary line.
pub fn cook(root: &Path, source: &Source) -> Result<String> {
    let metadata: J = serde_json::from_slice(
        &std::fs::read(root.join("data/regions.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let mut c = collect(source, &metadata)?;
    let scenes = metadata["scenes"].as_array().ok_or("no scenes")?;
    for (n, s) in scenes.iter().enumerate() {
        if s["scene_id"].as_i64() != Some(n as i64) {
            return err("scene ids are not the catalogue order");
        }
    }
    let dest = root.join(".hkpsx/break-effects");
    std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    let mut ignored = std::mem::take(&mut c.ignored);
    let (mut manifest, mut curves, mut scene_rows, mut total_vram, mut kept) =
        (Vec::new(), Vec::new(), Vec::new(), 0usize, 0usize);
    let mut arts: Vec<(i64, SceneArt)> = Vec::new();
    let mut images: HashMap<String, Arc<Image>> = HashMap::new();
    for scene in 0..scenes.len() as i64 {
        let data = scene_art(
            scene,
            &c.styles,
            &c.emitters,
            &mut ignored,
            &mut |styles, sids| {
                let mut textures = Vec::new();
                for sid in sids {
                    if !images.contains_key(sid) {
                        let obj = &c.textures.iter().find(|t| &t.0 == sid).ok_or("texture")?.1;
                        let im = hk_unity::texture::texture_image(source, obj, true)
                            .map_err(|e| e.to_string())?
                            .to_rgba();
                        images.insert(sid.clone(), Arc::new(im));
                    }
                    textures.push((sid.clone(), images[sid].clone()));
                }
                art_bank(styles, &textures)
            },
        )?;
        kept += data.emitters.len();
        total_vram = total_vram.max(data.bank.as_ref().map_or(0, |b| b.blob.len()));
        arts.push((scene, data));
    }
    // Curves of the stalactite styles go after every other, so the scenes
    // without stalactites keep their curve numbers.
    for optional in [false, true] {
        for (_, data) in &arts {
            for (k, st) in data.styles.iter().enumerate() {
                let only = data
                    .emitters
                    .iter()
                    .filter(|e| e.style == k)
                    .all(|e| e.owner >= STALACTITE_OWNER);
                let key = st.samples_key();
                if only == optional && !curves.contains(&key) {
                    curves.push(key);
                }
            }
        }
    }
    for (scene, data) in arts {
        let raw = pack_scene_effects(&data, &mut curves)?;
        let stored = compressed(&raw);
        let name = format!("scene_{scene}.hkfx.z");
        std::fs::write(dest.join(&name), &stored).map_err(|e| e.to_string())?;
        manifest.push(Chunk {
            scene_id: scene,
            raw_len: raw.len(),
            raw_fnv: fnv(&raw),
            stored_len: stored.len(),
            stored_fnv: fnv(&stored),
            path: format!(".hkpsx/break-effects/{name}"),
            raw_sha256: sha_hex(&raw),
        });
        if let Some(b) = &data.bank {
            crate::png::save_rgba(&dest.join(format!("art_{scene}.png")), &b.sheet)?;
        }
        scene_rows.push((scene, data));
    }
    let code = generate(&curves, &manifest)?;
    std::fs::write(root.join("data/break_effects.rs"), &code).map_err(|e| e.to_string())?;
    let mut files: Vec<String> = Vec::new();
    for s in scenes {
        let f = jstr(s, "file");
        if !files.contains(&f) {
            files.push(f);
        }
    }
    files.push("globalgamemanagers".into());
    files.push("Managed/Assembly-CSharp.dll".into());
    let mut source_files = Vec::new();
    for name in files {
        if source_files
            .iter()
            .any(|(k, _): &(String, Json)| *k == name)
        {
            continue;
        }
        let path = source.directory.join(&name);
        let bytes = std::fs::read(&path).map_err(|e| format!("{name}: {e}"))?;
        source_files.push((name, Json::Str(sha_hex(&bytes))));
    }
    let active = scene_rows
        .iter()
        .filter(|(_, d)| !d.emitters.is_empty())
        .count();
    let report = Json::Obj(vec![
        ("format".into(), Json::Str("HKBREAK02".into())),
        (
            "scenes".into(),
            Json::Obj(
                scene_rows
                    .iter()
                    .map(|(k, d)| (k.to_string(), scene_json(d)))
                    .collect(),
            ),
        ),
        ("source_records".into(), Json::List(c.records)),
        ("other_parts".into(), Json::List(ignored)),
        (
            "largest_scene_vram_bytes".into(),
            Json::Int(total_vram as i64),
        ),
        (
            "vram_rects".into(),
            Json::List(
                VRAM_RECTS
                    .iter()
                    .map(|r| ints([r.0, r.1, r.2, r.3]))
                    .collect(),
            ),
        ),
        (
            "chunks".into(),
            Json::List(
                manifest
                    .iter()
                    .map(|m| {
                        Json::Obj(vec![
                            ("scene_id".into(), Json::Int(m.scene_id)),
                            ("raw_len".into(), Json::Int(m.raw_len as i64)),
                            ("raw_fnv".into(), Json::Int(m.raw_fnv as i64)),
                            ("stored_len".into(), Json::Int(m.stored_len as i64)),
                            ("stored_fnv".into(), Json::Int(m.stored_fnv as i64)),
                            ("path".into(), Json::Str(m.path.clone())),
                            ("raw_sha256".into(), Json::Str(m.raw_sha256.clone())),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("rust_sha256".into(), Json::Str(sha_hex(code.as_bytes()))),
        (
            "particle_tracks".into(),
            Json::Obj(vec![
                ("bytes".into(), Json::Int(0)),
                ("fallback_emitters".into(), Json::Int(kept as i64)),
                ("format".into(), Json::Str("scalar path only".into())),
            ]),
        ),
        ("source_files".into(), Json::Obj(source_files)),
        (
            "limitations".into(),
            Json::List(
                LIMITATIONS
                    .iter()
                    .map(|s| Json::Str(s.to_string()))
                    .collect(),
            ),
        ),
    ]);
    std::fs::write(dest.join("report.json"), dumps(&report)).map_err(|e| e.to_string())?;
    Ok(format!("Break effects: {kept} source emitters across {active} scenes, largest sheet {total_vram} VRAM bytes"))
}

pub fn main(root: &Path, source_dir: Option<&Path>) -> Result<()> {
    let source = match source_dir {
        Some(d) => Source::new(d).map_err(|e| e.to_string())?,
        None => Source::from_doctor(root).map_err(|e| e.to_string())?,
    };
    println!("{}", cook(root, &source)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(texture: &str, cells: i64, size: [i64; 2]) -> Style {
        Style {
            life: [1, 1],
            speed: [0, 0],
            size,
            rotation: [0, 0],
            colors: [[128; 3]; 2],
            start_alpha: [255, 255],
            count: 1,
            rate: 65536,
            shape: 10,
            radius: 0,
            arc: 0,
            shape_scale: [0; 3],
            force: [[0; 2]; 3],
            velocity: [[0; 2]; 3],
            limit: -1,
            dampen: 0,
            spin_speed: [0, 0],
            spin_range: [0, 0],
            collision: false,
            bounce: 0,
            collision_dampen: 0,
            life_loss: 0,
            kill_speed: 0,
            radius_scale: 0,
            samples: Vec::new(),
            cells,
            texture: texture.into(),
        }
    }
    fn sheet() -> Arc<Image> {
        let mut im = Image::new(Mode::Rgba, 36, 144);
        for y in 0..144 {
            for x in 0..36 {
                let at = (y * 36 + x) * 4;
                im.data[at..at + 4].copy_from_slice(&[
                    255,
                    (70 * (y / 36)) as u8,
                    50,
                    [0, 128, 255][x % 3],
                ]);
            }
        }
        Arc::new(im)
    }

    #[test]
    fn every_original_cell_and_three_alpha_variants_have_disjoint_legal_art() {
        let styles = [style("original:1", 4, [32768, 52429])];
        let textures = [("original:1".to_string(), sheet())];
        let bank = art_bank(&styles, &textures).unwrap();
        assert_eq!(bank.art.len(), 12);
        let frames = &bank.frames[0].1;
        assert_eq!(frames.len(), 4);
        let mut all: Vec<i64> = frames.iter().flatten().copied().collect();
        all.sort();
        assert_eq!(all, (0..12).collect::<Vec<_>>());
        let mut cells = std::collections::HashSet::new();
        for u in &bank.uploads {
            let [_, ux, uy, uw, uh] = *u;
            assert!(VRAM_RECTS
                .iter()
                .any(|&(x, y, w, h)| x <= ux && y <= uy && ux + uw <= x + w && uy + uh <= y + h));
            for x in ux..ux + uw {
                for y in uy..uy + uh {
                    assert!(cells.insert((x, y)), "uploads overlap");
                }
            }
        }
        assert_eq!(cells.len() * 2, bank.blob.len());
        for a in &bank.art {
            assert!(a[0] + a[2] <= 256 && a[1] + a[3] <= 256);
        }
        let again = art_bank(&styles, &textures).unwrap();
        assert_eq!(
            (again.blob, again.uploads, again.art, again.frames),
            (bank.blob, bank.uploads, bank.art, bank.frames)
        );
    }

    #[test]
    fn a_mixed_source_sheet_layout_is_refused() {
        let styles = [
            style("original:1", 4, [1, 1]),
            style("original:1", 3, [1, 1]),
        ];
        let e = art_bank(&styles, &[("original:1".to_string(), sheet())])
            .err()
            .unwrap();
        assert!(e.contains("mixed sheet"));
    }

    #[test]
    fn an_overflowing_bank_drops_the_costliest_style_not_the_scene() {
        let styles = [
            style("a", 1, [65536, 65536]),
            style("b", 8, [65536 * 4, 65536 * 4]),
        ];
        let row = |style: usize, owner: i64, source: i64| EmitterRow {
            scene: 0,
            owner,
            source,
            style,
            angle_offset: 0,
            origin: [0; 3],
            basis: [[0; 3]; 3],
        };
        let emitters = [row(0, 10, 1), row(1, 11, 2)];
        let mut calls = Vec::new();
        let mut ignored = Vec::new();
        let data = scene_art(0, &styles, &emitters, &mut ignored, &mut |trimmed, _| {
            calls.push(trimmed.len());
            if trimmed.len() > 1 {
                return err("reserved VRAM fragments do not fit");
            }
            Ok(Bank {
                blob: Vec::new(),
                uploads: Vec::new(),
                art: Vec::new(),
                frames: vec![("a".into(), vec![[0, 1, 2]])],
                sheet: Image::new(Mode::Rgba, 1, 1),
            })
        })
        .unwrap();
        assert_eq!(calls, [2, 1]);
        assert_eq!(
            data.emitters.iter().map(|e| e.owner).collect::<Vec<_>>(),
            [10]
        );
        assert_eq!(ignored.len(), 1);
        let text = dumps(&ignored[0]);
        assert!(text.contains("\"owner\": 11") && text.contains("style 1 dropped"));
    }

    #[test]
    fn rectangles_keep_clut_alignment_and_refuse_what_does_not_fit() {
        let items = vec![
            ("p0".to_string(), 16, 1, 16),
            ("t0".to_string(), 8, 30, 1),
            ("t1".to_string(), 5, 20, 1),
        ];
        let placed = place_rectangles(&items, &VRAM_RECTS).unwrap();
        assert_eq!(placed["p0"].0 % 16, 0);
        let big = vec![("t0".to_string(), 65, 64, 1)];
        assert_eq!(
            place_rectangles(&big, &VRAM_RECTS).err().unwrap(),
            "Geo VRAM exceeds8KiB"
        );
    }

    #[test]
    fn chunks_compress_only_when_smaller_and_decode_back() {
        let raw = vec![7u8; 4096];
        let stored = compressed(&raw);
        assert_eq!(&stored[..4], b"HLZC");
        assert_eq!(hk_lz4::decompress(&stored[8..], raw.len()).unwrap(), raw);
        assert_eq!(compressed(b"abc"), b"abc");
        assert_eq!(fnv(b""), 0x811c9dc5);
    }

    #[test]
    fn fixed_point_rejects_unbounded_or_nonfinite_source() {
        for x in [f64::INFINITY, f64::NAN, 32768.0, -32768.0] {
            assert!(q(x).is_err());
        }
        assert_eq!(q(-0.5).unwrap(), -32768);
    }
}
