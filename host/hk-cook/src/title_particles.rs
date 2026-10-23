//! The particle systems of the title styles, read from `Menu_Styles` (level1) and sampled into the
//! tracks the PS1 title plays back.
//!
//! The source's systems are Unity shuriken systems in world space: a box or a disc emits particles of
//! a random life, start speed and size, a velocity module pushes them along the emitter's local
//! axes, gravity bends them, a gradient fades them and a curve scales them. Their emitters sit at
//! all angles in front of the title camera (`title_style.rs`), so a particle's path is a plain curve
//! in world space that the camera projects.
//!
//! The guest does not simulate. For every system the cooker draws the steady-state population from the
//! module settings (`prewarm` is on), throws away what never crosses the 320x240 view, and keeps a
//! few alternative tracks per slot: a track is a quadratic in the particle's age on the screen, its
//! size and its colour gain. A slot plays one of its tracks per life, chosen by a hash of the life
//! number (menu.rs `emitters`), so the picture neither repeats a particle at once nor costs RAM.
use crate::title_style::{apply, identity, mul, paths, pptr, quat, trs, vec3, Mat, HEIGHT, WIDTH};
use hk_unity::scene::Scene;
use hk_unity::Value;
use std::collections::HashMap;

/// The camera's focal length in pixels for the title's 24 degree vertical field.
pub fn focal(fov_degrees: f64) -> f64 {
    (HEIGHT as f64 / 2.0) / (fov_degrees.to_radians() / 2.0).tan()
}

/// A Unity min/max value: a constant, two constants to pick between, or a curve times a scalar.
#[derive(Clone, Debug)]
pub struct MinMax {
    pub low: f64,
    pub high: f64,
    pub curve: Option<Vec<Key>>,
}

/// An animation curve key.
#[derive(Clone, Copy, Debug)]
pub struct Key {
    pub time: f64,
    pub value: f64,
    pub in_slope: f64,
    pub out_slope: f64,
}

/// The Hermite value of `keys` at `t` (Unity's `AnimationCurve.Evaluate`, clamped at both ends).
pub fn eval_curve(keys: &[Key], t: f64) -> f64 {
    let (Some(first), Some(last)) = (keys.first(), keys.last()) else { return 0.0 };
    if t <= first.time {
        return first.value;
    }
    if t >= last.time {
        return last.value;
    }
    let i = keys.windows(2).position(|w| t >= w[0].time && t <= w[1].time).unwrap_or(0);
    let (a, b) = (keys[i], keys[i + 1]);
    let dt = b.time - a.time;
    let s = (t - a.time) / dt;
    let (m0, m1) = (a.out_slope * dt, b.in_slope * dt);
    let (s2, s3) = (s * s, s * s * s);
    (2.0 * s3 - 3.0 * s2 + 1.0) * a.value
        + (s3 - 2.0 * s2 + s) * m0
        + (-2.0 * s3 + 3.0 * s2) * b.value
        + (s3 - s2) * m1
}

fn f(v: Option<&Value>) -> f64 {
    v.and_then(Value::float).unwrap_or(0.0)
}

fn keys_of(curve: Option<&Value>) -> Vec<Key> {
    curve
        .and_then(|c| c.get("m_Curve"))
        .and_then(Value::list)
        .unwrap_or(&[])
        .iter()
        .map(|k| Key {
            time: f(k.get("time")),
            value: f(k.get("value")),
            in_slope: f(k.get("inSlope")),
            out_slope: f(k.get("outSlope")),
        })
        .collect()
}

/// A `MinMaxCurve` as a value or range (a curve is kept whole, scaled by `scalar`).
pub fn min_max(v: Option<&Value>) -> MinMax {
    let Some(v) = v else { return MinMax { low: 0.0, high: 0.0, curve: None } };
    let state = v.get("minMaxState").and_then(Value::int).unwrap_or(0);
    let scalar = f(v.get("scalar"));
    match state {
        3 => MinMax { low: f(v.get("minScalar")), high: scalar, curve: None },
        1 | 2 => {
            let keys: Vec<Key> = keys_of(v.get("maxCurve"))
                .into_iter()
                .map(|k| Key { value: k.value * scalar, in_slope: k.in_slope * scalar, out_slope: k.out_slope * scalar, ..k })
                .collect();
            MinMax { low: scalar, high: scalar, curve: Some(keys) }
        }
        _ => MinMax { low: scalar, high: scalar, curve: None },
    }
}

/// A particle gradient: the start colour of one or two colours, and the alpha and colour keys.
#[derive(Clone, Debug)]
pub struct Gradient {
    pub state: i64,
    pub min: [f64; 4],
    pub max: [f64; 4],
    pub colour_keys: Vec<(f64, [f64; 3])>,
    pub alpha_keys: Vec<(f64, f64)>,
}

fn colour(v: Option<&Value>) -> [f64; 4] {
    let g = |k: &str| v.and_then(|c| c.get(k)).and_then(Value::float).unwrap_or(1.0);
    [g("r"), g("g"), g("b"), g("a")]
}

fn gradient(v: Option<&Value>) -> Option<Gradient> {
    let v = v?;
    let g = v.get("maxGradient");
    let count = |k: &str| g.and_then(|x| x.get(k)).and_then(Value::int).unwrap_or(0) as usize;
    let key = |i: usize| g.and_then(|x| x.get(&format!("key{i}")));
    let time = |name: &str, i: usize| f(g.and_then(|x| x.get(&format!("{name}{i}")))) / 65535.0;
    Some(Gradient {
        state: v.get("minMaxState").and_then(Value::int).unwrap_or(0),
        min: colour(v.get("minColor")),
        max: colour(v.get("maxColor")),
        colour_keys: (0..count("m_NumColorKeys"))
            .map(|i| {
                let c = colour(key(i));
                (time("ctime", i), [c[0], c[1], c[2]])
            })
            .collect(),
        alpha_keys: (0..count("m_NumAlphaKeys")).map(|i| (time("atime", i), colour(key(i))[3])).collect(),
    })
}

impl Gradient {
    /// The alpha of the gradient at `u` of the life.
    pub fn alpha(&self, u: f64) -> f64 {
        let ks = &self.alpha_keys;
        let (Some(first), Some(last)) = (ks.first(), ks.last()) else { return 1.0 };
        if u <= first.0 {
            return first.1;
        }
        if u >= last.0 {
            return last.1;
        }
        let i = ks.windows(2).position(|w| u >= w[0].0 && u <= w[1].0).unwrap_or(0);
        let (a, b) = (ks[i], ks[i + 1]);
        a.1 + (b.1 - a.1) * (u - a.0) / (b.0 - a.0).max(1e-9)
    }
}

/// How a system's particles meet the picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    /// `Particles/Additive` (twice the colour and the tint's alpha) or its soft variant.
    Additive,
    AdditiveSoft,
    /// `Particles/Multiply` and the plain sprite shader: darkens the picture by the coverage.
    Darken,
}

/// One particle system as the scene holds it.
#[derive(Debug)]
pub struct System {
    pub path: String,
    pub world: Mat,
    pub looping: bool,
    pub life: MinMax,
    pub speed: MinMax,
    pub size: MinMax,
    pub gravity: MinMax,
    pub max_particles: f64,
    pub start: Gradient,
    pub over_life: Option<Gradient>,
    pub rate: f64,
    pub shape: i64,
    pub shape_scale: [f64; 3],
    pub shape_radius: f64,
    pub size_over_life: Option<Vec<Key>>,
    pub velocity: Option<([MinMax; 3], bool)>,
    pub texture: String,
    pub blend: Blend,
    pub tint: [f64; 4],
    /// Frames of the texture sheet (`UVModule` over the whole life), 1 without one.
    pub frames: usize,
    /// The renderer's `maxParticleSize`: the largest a particle may be, as a fraction of the viewport.
    pub max_size: f64,
}

/// Every particle system under `Menu_Styles/<style>` that is active (the style's own object is
/// switched on when it loads, so only what is below it counts).
pub fn systems(
    source: &hk_unity::Source,
    scene: &Scene,
    style: &str,
) -> Result<Vec<System>, String> {
    let paths = paths(scene);
    let mut go_tr: HashMap<i64, i64> = HashMap::new();
    for o in &scene.objects {
        if o.typename == "Transform" || o.typename == "RectTransform" {
            go_tr.insert(pptr(&o.tree, "m_GameObject"), o.id);
        }
    }
    let world = |go: i64| -> Mat {
        let mut chain = Vec::new();
        let mut t = go_tr.get(&go).copied().unwrap_or(0);
        while t != 0 {
            let Some(tr) = scene.object(t) else { break };
            chain.push(trs(
                vec3(tr.tree.get("m_LocalPosition")),
                quat(tr.tree.get("m_LocalRotation")),
                vec3(tr.tree.get("m_LocalScale")),
            ));
            t = pptr(&tr.tree, "m_Father");
        }
        let mut m = identity();
        for local in chain.iter().rev() {
            m = mul(&m, local);
        }
        m
    };
    let root = format!("Menu_Styles/{style}");
    let prefix = format!("{root}/");
    let active = |go: i64| scene.object(go).and_then(|g| g.tree.get("m_IsActive")).map(Value::truthy).unwrap_or(true);
    let mut out = Vec::new();
    for o in &scene.objects {
        if o.typename != "ParticleSystem" {
            continue;
        }
        let go = pptr(&o.tree, "m_GameObject");
        let Some(path) = paths.get(&go) else { continue };
        if !path.starts_with(&prefix) {
            continue;
        }
        let mut visible = true;
        let mut check = path.clone();
        while check != root {
            if let Some(id) = paths.iter().find(|(_, p)| **p == check).map(|(id, _)| *id) {
                visible &= active(id);
            }
            match check.rfind('/') {
                Some(i) => check.truncate(i),
                None => break,
            }
        }
        let renderer = scene
            .objects
            .iter()
            .find(|r| r.typename == "ParticleSystemRenderer" && pptr(&r.tree, "m_GameObject") == go);
        let Some(renderer) = renderer else { continue };
        if !visible || !renderer.tree.get("m_Enabled").map(Value::truthy).unwrap_or(true) {
            continue;
        }
        let t = &o.tree;
        let initial = t.get("InitialModule");
        let emission = t.get("EmissionModule");
        if !emission.and_then(|e| e.get("enabled")).map(Value::truthy).unwrap_or(false) {
            continue;
        }
        let shape = t.get("ShapeModule");
        let sc = shape.and_then(|s| s.get("m_Scale"));
        let size_module = t.get("SizeModule");
        let velocity = t.get("VelocityModule");
        let colour_module = t.get("ColorModule");
        let uv = t.get("UVModule");
        // The material and its main texture.
        let material_object = renderer
            .tree
            .get("m_Materials")
            .and_then(Value::list)
            .and_then(|l| l.first())
            .and_then(|p| scene.deref(p).ok())
            .ok_or("no particle material")?;
        let material = source.read(&material_object).map_err(|e| e.to_string())?;
        let file = &material_object.file;
        let shader_name = material
            .get("m_Shader")
            .and_then(|p| source.deref(file, p).ok())
            .and_then(|s| source.read(&s).ok())
            .and_then(|s| {
                s.get("m_ParsedForm").and_then(|f| f.get("m_Name")).and_then(Value::str).or_else(|| s.get("m_Name").and_then(Value::str))
            })
            .unwrap_or_default();
        let saved = material.get("m_SavedProperties");
        let texture = saved
            .and_then(|p| p.get("m_TexEnvs"))
            .and_then(Value::list)
            .unwrap_or(&[])
            .iter()
            .filter_map(|e| e.list())
            .find(|e| e.first().and_then(Value::str).as_deref() == Some("_MainTex"))
            .and_then(|e| e.get(1))
            .and_then(|e| e.get("m_Texture"))
            .and_then(|p| source.deref(file, p).ok())
            .and_then(|t| source.read(&t).ok())
            .and_then(|t| t.get("m_Name").and_then(Value::str))
            .unwrap_or_default();
        let tint = saved
            .and_then(|p| p.get("m_Colors"))
            .and_then(Value::list)
            .unwrap_or(&[])
            .iter()
            .filter_map(|e| e.list())
            .find(|e| e.first().and_then(Value::str).as_deref() == Some("_TintColor"))
            .map(|e| colour(e.get(1)))
            .unwrap_or([0.5, 0.5, 0.5, 0.5]);
        let blend = if shader_name.contains("Additive (Soft)") {
            Blend::AdditiveSoft
        } else if shader_name.contains("Additive") {
            Blend::Additive
        } else {
            Blend::Darken
        };
        let uv_on = uv.and_then(|u| u.get("enabled")).map(Value::truthy).unwrap_or(false);
        let frames = if uv_on {
            (f(uv.and_then(|u| u.get("tilesX"))).max(1.0) * f(uv.and_then(|u| u.get("tilesY"))).max(1.0)) as usize
        } else {
            1
        };
        out.push(System {
            path: path.clone(),
            world: world(go),
            looping: t.get("looping").map(Value::truthy).unwrap_or(true),
            life: min_max(initial.and_then(|i| i.get("startLifetime"))),
            speed: min_max(initial.and_then(|i| i.get("startSpeed"))),
            size: min_max(initial.and_then(|i| i.get("startSize"))),
            gravity: min_max(initial.and_then(|i| i.get("gravityModifier"))),
            max_particles: f(initial.and_then(|i| i.get("maxNumParticles"))),
            start: gradient(initial.and_then(|i| i.get("startColor"))).ok_or("no start colour")?,
            over_life: colour_module
                .filter(|c| c.get("enabled").map(Value::truthy).unwrap_or(false))
                .and_then(|c| gradient(c.get("gradient"))),
            rate: min_max(emission.and_then(|e| e.get("rateOverTime"))).high,
            shape: shape.and_then(|s| s.get("type")).and_then(Value::int).unwrap_or(5),
            shape_scale: [f(sc.and_then(|s| s.get("x"))), f(sc.and_then(|s| s.get("y"))), f(sc.and_then(|s| s.get("z")))],
            shape_radius: f(shape.and_then(|s| s.get("radius")).and_then(|r| r.get("value"))),
            size_over_life: size_module
                .filter(|m| m.get("enabled").map(Value::truthy).unwrap_or(false))
                .and_then(|m| min_max(m.get("curve")).curve),
            velocity: velocity.filter(|v| v.get("enabled").map(Value::truthy).unwrap_or(false)).map(|v| {
                (
                    [min_max(v.get("x")), min_max(v.get("y")), min_max(v.get("z"))],
                    v.get("inWorldSpace").map(Value::truthy).unwrap_or(false),
                )
            }),
            texture,
            blend,
            tint,
            frames,
            max_size: renderer.tree.get("m_MaxParticleSize").and_then(Value::float).unwrap_or(0.5),
        });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// A small xorshift generator: the tracks are reproducible from the scene alone.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1)
    }
    pub fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    pub fn between(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.next()
    }
}

/// A particle's path on the screen: `p(u) = p0 + b u + c u^2` for `u` of the life in 0..1.
#[derive(Clone, Copy, Debug)]
pub struct Track {
    pub p0: [f64; 2],
    pub b: [f64; 2],
    pub c: [f64; 2],
    /// Width of the quad in pixels at the start size (before the size curve).
    pub size: f64,
    /// Start colour with its alpha folded in, in 0..1 per channel (before the over-life alpha).
    pub colour: [f64; 3],
    /// How much of the life the particle is inside the screen.
    pub on_screen: f64,
}

fn project(cam: [f64; 3], focal: f64, p: [f64; 3]) -> Option<[f64; 2]> {
    let z = p[2] - cam[2];
    (z > 1.0).then(|| [WIDTH as f64 / 2.0 + (p[0] - cam[0]) * focal / z, HEIGHT as f64 / 2.0 - (p[1] - cam[1]) * focal / z])
}

fn rotate(m: &Mat, v: [f64; 3]) -> [f64; 3] {
    // The rotation part with scale removed: each column normalised.
    let col = |j: usize| {
        let n = (m[0][j] * m[0][j] + m[1][j] * m[1][j] + m[2][j] * m[2][j]).sqrt().max(1e-12);
        [m[0][j] / n, m[1][j] / n, m[2][j] / n]
    };
    let (x, y, z) = (col(0), col(1), col(2));
    [0, 1, 2].map(|i| x[i] * v[0] + y[i] * v[1] + z[i] * v[2])
}

impl System {
    /// The mean life in seconds.
    pub fn mean_life(&self) -> f64 {
        (self.life.low + self.life.high) / 2.0
    }
    /// Particles alive at once in the steady state.
    pub fn alive(&self) -> f64 {
        (self.rate * self.mean_life()).min(if self.max_particles > 0.0 { self.max_particles } else { 1000.0 })
    }
    /// One particle with the life `life` seconds, projected, and its quad size in pixels at the
    /// start, or `None` when it is behind the camera. `canvas` is the picture it meets (its
    /// brightness sets how much a darkening particle takes away).
    pub fn particle(&self, rng: &mut Rng, life: f64, cam: [f64; 3], focal: f64, canvas: &[[f64; 3]]) -> Option<Track> {
        // Where it starts.
        let (local, dir) = match self.shape {
            10 => {
                let r = self.shape_radius * rng.next().sqrt();
                let a = rng.next() * std::f64::consts::TAU;
                ([r * a.cos(), r * a.sin(), 0.0], [a.cos(), a.sin(), 0.0])
            }
            _ => (
                [
                    (rng.next() - 0.5) * self.shape_scale[0],
                    (rng.next() - 0.5) * self.shape_scale[1],
                    (rng.next() - 0.5) * self.shape_scale[2],
                ],
                [0.0, 0.0, 1.0],
            ),
        };
        let p0 = apply(&self.world, local);
        let speed = rng.between(self.speed.low, self.speed.high);
        let mut v = rotate(&self.world, [dir[0] * speed, dir[1] * speed, dir[2] * speed]);
        if let Some((axes, in_world)) = &self.velocity {
            let l = [0, 1, 2].map(|i| rng.between(axes[i].low, axes[i].high));
            let w = if *in_world { l } else { rotate(&self.world, l) };
            for i in 0..3 {
                v[i] += w[i];
            }
        }
        let g = -9.81 * rng.between(self.gravity.low, self.gravity.high);
        let at = |u: f64| [p0[0] + v[0] * life * u, p0[1] + v[1] * life * u + 0.5 * g * (life * u).powi(2), p0[2] + v[2] * life * u];
        let pts: Vec<Option<[f64; 2]>> = (0..=8).map(|k| project(cam, focal, at(k as f64 / 8.0))).collect();
        let (s, m, e) = (pts[0]?, pts[4]?, pts[8]?);
        let c = [2.0 * (e[0] + s[0] - 2.0 * m[0]), 2.0 * (e[1] + s[1] - 2.0 * m[1])];
        let b = [e[0] - s[0] - c[0], e[1] - s[1] - c[1]];
        let depth = p0[2] - cam[2];
        let size_world = rng.between(self.size.low, self.size.high);
        let size = size_world * focal / depth;
        let on = pts.iter().flatten().filter(|p| p[0] > -size && p[0] < WIDTH as f64 + size && p[1] > -size && p[1] < HEIGHT as f64 + size).count();
        // The start colour: one colour, or a blend of two.
        let mix = rng.next();
        let c4 = if self.start.state == 2 || self.start.state == 3 {
            [0, 1, 2, 3].map(|i| self.start.min[i] + (self.start.max[i] - self.start.min[i]) * mix)
        } else {
            self.start.max
        };
        let colour = match self.blend {
            Blend::Additive => [0, 1, 2].map(|i| (1.8 * c4[i] * c4[3] * self.tint[i] / 0.75).min(1.99)),
            Blend::AdditiveSoft => [0, 1, 2].map(|i| (c4[i] * c4[3]).min(1.99)),
            Blend::Darken => {
                let (x, y) = (m[0].clamp(0.0, WIDTH as f64 - 1.0) as usize, m[1].clamp(0.0, HEIGHT as f64 - 1.0) as usize);
                let px = canvas[y * WIDTH + x];
                let luma = 0.299 * px[0] + 0.587 * px[1] + 0.114 * px[2];
                let a = c4[3] * (1.0 - (c4[0] + c4[1] + c4[2]) / 3.0).max(0.0);
                [a * luma; 3]
            }
        };
        Some(Track { p0: s, b, c, size, colour, on_screen: on as f64 / 9.0 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flat_curve_stays_flat_and_a_two_key_curve_with_zero_slopes_is_smoothstep() {
        let flat = [Key { time: 0.0, value: 0.5, in_slope: 0.0, out_slope: 0.0 }, Key { time: 1.0, value: 0.5, in_slope: 0.0, out_slope: 0.0 }];
        assert!((eval_curve(&flat, 0.3) - 0.5).abs() < 1e-12);
        let step = [Key { time: 0.0, value: 0.0, in_slope: 0.0, out_slope: 0.0 }, Key { time: 1.0, value: 1.0, in_slope: 0.0, out_slope: 0.0 }];
        assert!((eval_curve(&step, 0.5) - 0.5).abs() < 1e-12);
        assert!(eval_curve(&step, 0.25) < 0.25);
    }

    #[test]
    fn a_projected_box_particle_with_no_motion_stays_put() {
        let sys = System {
            path: String::new(),
            world: identity(),
            looping: true,
            life: MinMax { low: 2.0, high: 2.0, curve: None },
            speed: MinMax { low: 0.0, high: 0.0, curve: None },
            size: MinMax { low: 1.0, high: 1.0, curve: None },
            gravity: MinMax { low: 0.0, high: 0.0, curve: None },
            max_particles: 10.0,
            start: Gradient { state: 0, min: [1.0; 4], max: [1.0; 4], colour_keys: vec![], alpha_keys: vec![] },
            over_life: None,
            rate: 5.0,
            shape: 5,
            shape_scale: [0.0; 3],
            shape_radius: 1.0,
            size_over_life: None,
            velocity: None,
            texture: String::new(),
            blend: Blend::Additive,
            tint: [0.75; 4],
            frames: 1,
            max_size: 0.5,
        };
        let canvas = vec![[0.0; 3]; WIDTH * HEIGHT];
        let t = sys.particle(&mut Rng::new(1), 2.0, [0.0, 0.0, -10.0], 500.0, &canvas).expect("in front");
        assert!((t.p0[0] - 160.0).abs() < 1e-9 && (t.p0[1] - 120.0).abs() < 1e-9);
        assert!(t.b[0].abs() < 1e-9 && t.c[1].abs() < 1e-9);
    }
}
