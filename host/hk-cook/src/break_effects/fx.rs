//! Effects of objects the Breakable cook does not read, cooked as emitters of owners from
//! `FX_OWNER` up and raised by the guest (game/src/fx.rs) where they happen.
//!
//! Two kinds. A particle prefab or a particle system of the scene (the Great Door's dust, a grub
//! jar's glass) goes through the same reader as every Breakable's debris. A rigid sprite object
//! that the original flings (`Particle Rock Small`, `Spatter Blue`, `Blue Glob`, the cocoon's cap)
//! is a burst here: one style whose particles collide with the terrain like the rigid bodies do,
//! cut from the object's own sprites. Both are optional in the scene's art budget (owners at or
//! above `STALACTITE_OWNER`), so a scene that has no room for them goes without and never
//! loses a style of its own.
//!
//! Source ids (installed Windows build, `file:path id`):
//! - Great Door (level6, FSM `Great Door`, state `Hit` and `Break 1`): `Door_dust`
//!   sharedassets6:486, `Dust Hit Med` sharedassets6:500, `Rubble Fall Short` sharedassets6:465,
//!   `Particle Rock Small` resources.assets:4328, `Particle Rock Small Transient` :5223.
//! - Health Cocoon (`HealthCocoon.flingPrefabs`): `Spatter Blue` resources.assets:5399 x50
//!   (speed 5..20, angle 100..440, origin +-1), `Blue Glob` sharedassets6:495 x8..10 (speed 2..8,
//!   origin +-1.5), the `Cocoon Cap` rigid body knocked with `capHitForce` 10.
//! - Grub jar (`Grub Bottle`): its `Pt Glass L` and `Pt Glass S` particle systems.

use super::*;
use crate::cook::{native_sprite, tk_sprite};

pub const FX_OWNER: i64 = 0xD000;
/// World units a light cell is cut for: 32 texels, the most a cell holds.
const DISC_CELL: f64 = 2.2;
/// `Dust Hit Med`, where the nail met an object (anchor: the hit).
pub const FX_HIT_DUST: i64 = FX_OWNER | 0x01;
/// The Great Door's `Door_dust` (cooked in place) and its `Rubble Fall Short`.
pub const FX_DOOR_DUST: i64 = FX_OWNER | 0x10;
pub const FX_DOOR_RUBBLE: i64 = FX_OWNER | 0x11;
/// The door's `Particle Rock Small Transient` spray (anchor: the door).
pub const FX_DOOR_CHIPS: i64 = FX_OWNER | 0x12;
/// `Particle Rock Small` at the nail (anchor: the hit).
pub const FX_HIT_CHIPS: i64 = FX_OWNER | 0x13;
/// The nail's impact (`Strike Nail R`): a flash and spark streaks (anchor: the hit).
pub const FX_HIT_FLASH: i64 = FX_OWNER | 0x14;
/// The pale light that runs up the Great Door's face with every hit (cooked in place).
pub const FX_DOOR_PULSE: i64 = FX_OWNER | 0x15;
/// The cocoon's spatters and globs (anchor: the cocoon), and its cap flown either way.
pub const FX_COCOON: i64 = FX_OWNER | 0x20;
pub const FX_COCOON_CAP_RIGHT: i64 = FX_OWNER | 0x21;
pub const FX_COCOON_CAP_LEFT: i64 = FX_OWNER | 0x22;
/// A struck Health Scuttler's blood (`SpawnBlood`, colour (0.145, 0.656, 0.860)).
pub const FX_SCUTTLER_BLOOD: i64 = FX_OWNER | 0x23;
/// A grub jar's `Pt Glass L` and `Pt Glass S` (anchor: the jar).
pub const FX_JAR_GLASS: i64 = FX_OWNER | 0x30;
/// The False Knight's landing and slam dust, and the mist along his arena's floor.
pub const FX_FK_DUST: i64 = FX_OWNER | 0x40;
pub const FX_FK_MIST: i64 = FX_OWNER | 0x41;
/// The orange sparks of his first landing.
pub const FX_FK_FIRE: i64 = FX_OWNER | 0x42;
/// Crossroads_10, the False Knight's arena (host/quality.py `scene_id`).
pub const FK_SCENE_ID: i64 = 11;

/// Where the cocoon's bursts leave it, from the cocoon's origin: the `Splat Sprite`'s place, and
/// the `Cocoon Cap`'s local position (Tutorial_01, level6).
const COCOON_BODY: [f64; 3] = [0.388, 0.668, 0.0];
/// Whether the cocoon's flung cap is cooked (see `collect_cocoon`).
const COCOON_CAP_ART: bool = false;
const COCOON_CAP: [f64; 3] = [0.28, 2.31, 0.01];

/// Smoke emitters cooked with half their authored puffs (see `prefab_owner`).
const THIN_SMOKE: [i64; 2] = [FX_DOOR_DUST, FX_HIT_DUST];

/// What a scene carries, found while the first pass has it open.
#[derive(Clone, Copy, Default)]
pub struct Kinds {
    pub door: bool,
    pub cocoon: bool,
    pub jar: bool,
    /// A Geo Rock stands here: its hits raise dust and chips like the door's.
    pub rocks: bool,
    /// A Breakable stands here: its hits raise dust.
    pub breakables: bool,
    /// The False Knight's arena: his landings and slams raise dust and mist.
    pub boss: bool,
}
impl Kinds {
    pub fn any(self) -> bool {
        self.door || self.cocoon || self.jar || self.rocks || self.breakables || self.boss
    }
}

pub fn detect(sc: &Scene) -> Kinds {
    let mut k = Kinds::default();
    for o in &sc.objects {
        match o.typename.as_str() {
            "HealthCocoon" => k.cocoon = true,
            "Breakable" => k.breakables = true,
            "GameObject" => match o.tree.get("m_Name").and_then(Value::str).as_deref() {
                Some("Hallownest_Main_Gate") => k.door = true,
                Some("Grub Bottle") => k.jar = true,
                Some(name) if name.starts_with("Geo Rock") => k.rocks = true,
                _ => {}
            },
            _ => {}
        }
    }
    k
}

// ---------------------------------------------------------------- bursts

/// One flung sprite object as a particle burst.
struct Burst {
    count: i64,
    life: [i64; 2],
    /// Metres (world units) per second.
    speed: [f64; 2],
    /// Launch angles in degrees, counter-clockwise from +X; a range is a circle sector.
    angle: [f64; 2],
    /// Half extents of the box the pieces start in (zero: all at the anchor).
    spread: [f64; 2],
    gravity_scale: f64,
    bounce: f64,
    /// Fraction of its life a collision costs; a splash replaces it.
    life_loss: f64,
    /// Degrees per second of spin, plus or minus.
    spin: f64,
    /// Multiples of the cell's world side.
    size: [f64; 2],
    /// Share of the drawn size the terrain contact uses.
    radius: f64,
    /// (cell, ticks, quarter-scale): a piece that lands turns into this cell for this long.
    splash: Option<(u8, u8, u8)>,
    /// Full size until this fraction of the life, then shrinking away.
    fade_from: f64,
    /// Drawn this many times the size the art is cut for (the cell keeps its texels).
    zoom: f64,
}

/// Sprites stacked as the cells of one sheet (cell 0 at the bottom, where the cook's cell
/// numbering starts), centred on square canvases. Returns the sheet and the world side of a cell.
fn sheet(frames: &[(hk_pil::Image, [f64; 4])]) -> Result<(hk_pil::Image, f64)> {
    let mut side = 1usize;
    let mut per_px = 0.0f64;
    for (im, b) in frames {
        side = side.max(im.width).max(im.height);
        if per_px == 0.0 && im.width > 0 {
            per_px = (b[2] - b[0]) / im.width as f64;
        }
    }
    if per_px <= 0.0 {
        return err("fx sprite without size");
    }
    let n = frames.len();
    let mut out = hk_pil::Image::new(hk_pil::Mode::Rgba, side, side * n);
    for (k, (im, _)) in frames.iter().enumerate() {
        let top = (n - 1 - k) * side;
        out.paste(
            im,
            ((side - im.width) / 2) as i64,
            (top + (side - im.height) / 2) as i64,
            None,
        );
    }
    Ok((out, side as f64 * per_px))
}

fn curve(fade_from: f64) -> Vec<Sample> {
    (0..33)
        .map(|n| {
            let t = n as f64 / 32.0;
            // Fully opaque, and shrinking away over the last part of its life as a rigid
            // piece's FinishingRigidBody does.
            let s = if t <= fade_from {
                1.0
            } else {
                ((1.0 - t) / (1.0 - fade_from)).clamp(0.0, 1.0)
            };
            Sample {
                size: [py_round(s * 65536.0), py_round(s * 65536.0)],
                alpha: [255, 255],
                spin: [0, 0],
            }
        })
        .collect()
}

fn burst_style(b: &Burst, side: f64, gravity: f64, cells: i64, texture: &str) -> Result<Style> {
    let circle = b.spread == [0.0, 0.0] && b.angle[1] > b.angle[0];
    let (shape, arc, radius, shape_scale, velocity) = if circle {
        (10, b.angle[1] - b.angle[0], 0.0, [1.0, 1.0, 1.0], [[0.0; 2]; 3])
    } else {
        // A box start with a cone of velocities: the speed range is carried by the axes
        // (the guest adds `velocity` to a zero launch).
        let (lo, hi) = (b.angle[0].to_radians(), b.angle[1].to_radians());
        let xs = [b.speed[0] * lo.cos(), b.speed[1] * hi.cos()];
        let ys = [b.speed[0] * lo.sin().min(hi.sin()), b.speed[1] * lo.sin().max(hi.sin())];
        let ordered = |v: [f64; 2]| [v[0].min(v[1]), v[0].max(v[1])];
        (
            5,
            0.0,
            0.0,
            [2.0 * b.spread[0], 2.0 * b.spread[1], 0.0],
            [ordered(xs), ordered(ys), [0.0; 2]],
        )
    };
    let qs = |v: [f64; 2]| -> Result<[i64; 2]> { Ok([q(v[0])?, q(v[1])?]) };
    let mut force = [[0i64; 2]; 3];
    let g = q(gravity * b.gravity_scale)?;
    force[1] = [g, g];
    let spin = q(b.spin)?;
    Ok(Style {
        life: b.life,
        speed: if circle { qs(b.speed)? } else { [0, 0] },
        size: qs([side * b.size[0] / b.zoom, side * b.size[1] / b.zoom])?,
        rotation: [0, q(359.0)?],
        colors: [[128; 3]; 2],
        start_alpha: [255; 2],
        additive: 0,
        count: b.count,
        rate: q(10000.0)?,
        shape,
        radius: q(radius)?,
        arc: q(arc)?,
        shape_scale: [q(shape_scale[0])?, q(shape_scale[1])?, q(shape_scale[2])?],
        force,
        velocity: [qs(velocity[0])?, qs(velocity[1])?, qs(velocity[2])?],
        limit: -1,
        dampen: 0,
        spin_speed: [0, 0],
        spin_range: [0, 0],
        collision: true,
        bounce: q(b.bounce)?,
        collision_dampen: 0,
        life_loss: q(b.life_loss)?,
        kill_speed: 0,
        radius_scale: q(b.radius)?,
        opaque: true,
        stretch: 0,
        art_size: 0,
        // Landing splash (ticks, cell, quarters), and always flying on outside the coverage.
        splash: 0x8000
            | b.splash.map_or(0, |(cell, ticks, quarters)| {
                i64::from(ticks) | i64::from(cell) << 8 | i64::from(quarters) << 12
            }),
        samples: curve(b.fade_from)
            .into_iter()
            .map(|mut s| {
                s.size = s.size.map(|v| py_round(v as f64 * b.zoom));
                s.spin = [spin.min(-spin), spin.max(-spin)];
                s
            })
            .collect(),
        cells,
        texture: texture.to_string(),
    })
}

fn rotation_about_z(degrees: f64) -> [[f64; 3]; 3] {
    let (s, c) = degrees.to_radians().sin_cos();
    [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]]
}

#[allow(clippy::too_many_arguments)]
fn push_burst(
    c: &mut Collected,
    scene_id: i64,
    owner: i64,
    b: &Burst,
    frames: &[(hk_pil::Image, [f64; 4])],
    gravity: f64,
    origin: [f64; 3],
    key: &str,
    what: &str,
) -> Result<()> {
    let (image, side) = sheet(frames)?;
    let texture = format!("fx:{key}");
    let style = burst_style(b, side, gravity, frames.len() as i64, &texture)?;
    if !c.synthetic.iter().any(|t| t.0 == texture) {
        c.synthetic.push((texture.clone(), Arc::new(image)));
    }
    let mut m = [[0.0; 4]; 4];
    let rot = if b.spread == [0.0, 0.0] && b.angle[1] > b.angle[0] {
        rotation_about_z(b.angle[0])
    } else {
        rotation_about_z(0.0)
    };
    for r in 0..3 {
        for k in 0..3 {
            m[r][k] = rot[r][k];
        }
        m[r][3] = origin[r];
    }
    m[3][3] = 1.0;
    let (placed, basis) = placement(&m)?;
    let style = c.style_index(style);
    c.emitters.push(EmitterRow {
        scene: scene_id,
        owner,
        source: owner * 16 + key.bytes().fold(0i64, |h, b| (h * 31 + b as i64) % 4093),
        style,
        angle_offset: 0,
        origin: placed,
        basis,
    });
    c.records.push(Json::Obj(vec![
        ("owner".into(), Json::Str(format!("fx {owner:#x}"))),
        ("name".into(), Json::Str(what.into())),
        ("part".into(), Json::Str(key.into())),
        ("system".into(), Json::Str("synthetic burst".into())),
        ("texture".into(), Json::Str(texture)),
        ("shader".into(), Json::Str("Sprites/Default".into())),
        ("scaling_mode".into(), Json::Int(0)),
        ("ps_sha256".into(), Json::Str(String::new())),
    ]));
    Ok(())
}

// ---------------------------------------------------------------- light

/// A flash, streak, halo, petal or chunk the cook draws itself: light of the effects' own that the
/// original's prefabs make with sprites and lit particles. They are soft additive styles (the
/// guest's soft path adds the texel at its tint times its opacity, `B + F`), so one grey art cell
/// serves every colour: the style's tint names it.
struct Glow {
    /// Art: `disc` (a radial falloff), `bar` (a thin line, bright in the middle), `lens` (a
    /// petal, pointed at both ends) or `chunk` (an angular shard).
    art: &'static str,
    count: i64,
    life: [i64; 2],
    /// A circle sector of launch angles (degrees, counter-clockwise from +X) and speeds in
    /// world units per second; `None` starts in place with `velocity` alone.
    angle: Option<[f64; 2]>,
    speed: [f64; 2],
    velocity: [f64; 2],
    /// Half extents of the box the particles start in.
    spread: [f64; 2],
    /// World units on a side (the width of a stretched one).
    size: [f64; 2],
    /// World units on a side that the art is cut for, when that is less than the size drawn: the
    /// cell keeps its few texels and the quad is stretched (the effects' VRAM is eight kilobytes).
    cell: f64,
    /// The size at the end of the life, as a multiple of the start's.
    grow: f64,
    /// Opacity at the start (0..1), held for `hold` of the life and fading to nothing after.
    alpha: f64,
    hold: f64,
    /// The colour a fully lit texel adds, 0..255 a channel.
    tint: [f64; 3],
    /// World units of length on each side per unit of speed (0: a square quad).
    stretch: f64,
    gravity_scale: f64,
    /// Drawn over the Knight and his slash rather than behind him.
    front: bool,
    /// Degrees per second of spin, plus or minus.
    spin: f64,
    /// Lands on the terrain and bounces (`bounce`), like the rigid pieces.
    collision: bool,
    bounce: f64,
    /// The cells are the frames of a clip, shown in turn over the particle's life.
    anim: bool,
}
impl Glow {
    const fn new(art: &'static str) -> Glow {
        Glow {
            art,
            count: 1,
            life: [6, 6],
            angle: None,
            speed: [0.0, 0.0],
            velocity: [0.0, 0.0],
            spread: [0.0, 0.0],
            size: [1.0, 1.0],
            cell: 1.0,
            grow: 1.0,
            alpha: 1.0,
            hold: 0.0,
            tint: [255.0, 255.0, 255.0],
            stretch: 0.0,
            gravity_scale: 0.0,
            front: false,
            spin: 0.0,
            collision: false,
            bounce: 0.0,
            anim: false,
        }
    }
}

/// The colour a style stores for a texel that should add `tint`: the guest's soft draw (game/src/
/// break_effects.rs `SOFT_GAIN_HALVES` 6) multiplies it by 3 at full opacity, and the GPU reads 128
/// as 1.0.
fn soft_colors(tint: [f64; 3]) -> [[i64; 3]; 2] {
    let c = tint.map(|v| (v * 128.0 / 255.0 / 3.0).round().clamp(0.0, 255.0) as i64);
    [c, c]
}

/// A 32 by 32 grey cell of light. RGB is the brightness (the soft draw adds it); the alpha only
/// marks where there is any.
fn glow_image(kind: &str) -> hk_pil::Image {
    let n = 32usize;
    let mut im = hk_pil::Image::new(hk_pil::Mode::Rgba, n, n);
    for y in 0..n {
        for x in 0..n {
            let u = (x as f64 + 0.5) / n as f64 * 2.0 - 1.0;
            let v = (y as f64 + 0.5) / n as f64 * 2.0 - 1.0;
            let k = match kind {
                // A soft disc: bright core, long tail.
                "disc" => (1.0 - (u * u + v * v).sqrt()).max(0.0).powf(1.4),
                // A line along x: bright in the middle, fading to both ends, thin across.
                "bar" => {
                    let along = (1.0 - u.abs()).max(0.0).powf(0.6);
                    let across = (1.0 - v.abs() / 0.2).max(0.0);
                    along * across
                }
                // A petal: widest at a third of its length, pointed ends, lit along its middle.
                "lens" | "leaf" => {
                    let w = 0.55 * (1.0 - u * u).max(0.0) * (1.0 - 0.25 * u);
                    if w <= 0.0 || v.abs() >= w {
                        0.0
                    } else {
                        (0.55 + 0.45 * (1.0 - v.abs() / w)) * (1.0 - 0.35 * u.abs())
                    }
                }
                // A glass shard: a long thin triangle, its two facets lit differently.
                "shard" => {
                    // Points of the triangle: (-0.95, 0.05), (0.9, -0.28), (0.55, 0.32).
                    let inside = |x: f64, y: f64| {
                        let e = |ax: f64, ay: f64, bx: f64, by: f64| (bx - ax) * (y - ay) - (by - ay) * (x - ax);
                        let (d1, d2, d3) = (
                            e(-0.95, 0.05, 0.9, -0.28),
                            e(0.9, -0.28, 0.55, 0.32),
                            e(0.55, 0.32, -0.95, 0.05),
                        );
                        !((d1 < 0.0 || d2 < 0.0 || d3 < 0.0) && (d1 > 0.0 || d2 > 0.0 || d3 > 0.0))
                    };
                    if inside(u, v) {
                        let facet = if v > 0.05 - 0.2 * (u + 0.95) { 1.0 } else { 0.62 };
                        facet * (0.78 + 0.22 * ((u + 1.0) / 2.0))
                    } else {
                        0.0
                    }
                }
                // A jagged piece: a pentagon-ish polygon, shaded by a light from the top left.
                _ => {
                    let a = v.atan2(u);
                    let edge = 0.78 + 0.18 * (a * 3.0 + 0.7).sin() + 0.08 * (a * 5.0).cos();
                    let d = (u * u + v * v).sqrt();
                    if d >= edge {
                        0.0
                    } else {
                        (0.62 + 0.38 * (-(u + v) * 0.7).clamp(-1.0, 1.0)).clamp(0.3, 1.0)
                    }
                }
            };
            let l = (k.clamp(0.0, 1.0) * 255.0).round() as u8;
            let at = (y * n + x) * 4;
            let opaque = if kind == "chunk" || kind == "shard" { k > 0.0 } else { l >= 44 };
            im.data[at..at + 4].copy_from_slice(&[l, l, l, if opaque { 255 } else { 0 }]);
        }
    }
    im
}

fn glow_curve(g: &Glow, scale: f64) -> Vec<Sample> {
    let spin = py_round(g.spin * 65536.0);
    (0..33)
        .map(|n| {
            let t = n as f64 / 32.0;
            let size = scale * (1.0 + (g.grow - 1.0) * t);
            let alpha = if t <= g.hold {
                1.0
            } else {
                ((1.0 - t) / (1.0 - g.hold)).clamp(0.0, 1.0)
            };
            Sample {
                size: [py_round(size * 65536.0); 2],
                alpha: [py_round(alpha * 255.0).clamp(0, 255); 2],
                spin: [spin.min(-spin), spin.max(-spin)],
            }
        })
        .collect()
}

/// One light effect as an emitter of `owner`, started at `origin` (world, from the owner's anchor).
fn push_glow(
    c: &mut Collected,
    scene_id: i64,
    owner: i64,
    g: &Glow,
    gravity: f64,
    origin: [f64; 3],
    what: &str,
) -> Result<()> {
    let key = format!("glow_{}", g.art);
    let frames = [(glow_image(g.art), [-0.5, -0.5, 0.5, 0.5])];
    push_glow_art(c, scene_id, owner, g, &frames, &key, gravity, origin, what)
}

/// The light a sprite throws: its luminance times its alpha as the grey of a cell (the guest's soft
/// draw adds the cell at the style's tint), the alpha only marking where there is any.
fn light_frames(frames: &[(hk_pil::Image, [f64; 4])]) -> Vec<(hk_pil::Image, [f64; 4])> {
    frames
        .iter()
        .map(|(im, b)| {
            let mut out = im.to_rgba();
            for px in out.data.chunks_exact_mut(4) {
                let l = (0.299 * f64::from(px[0]) + 0.587 * f64::from(px[1]) + 0.114 * f64::from(px[2]))
                    * f64::from(px[3])
                    / 255.0;
                let l = l.round().clamp(0.0, 255.0) as u8;
                px.copy_from_slice(&[l, l, l, if l >= 24 { 255 } else { 0 }]);
            }
            (out, *b)
        })
        .collect()
}

/// `push_glow` with the art of a clip or a sprite: `frames` are light cells (`light_frames`) and `size`
/// is in world units whatever the sheet's own size.
#[allow(clippy::too_many_arguments)]
fn push_glow_art(
    c: &mut Collected,
    scene_id: i64,
    owner: i64,
    g: &Glow,
    frames: &[(hk_pil::Image, [f64; 4])],
    key: &str,
    gravity: f64,
    origin: [f64; 3],
    what: &str,
) -> Result<()> {
    let key = key.to_string();
    let texture = format!("fx:{key}");
    let (sheet_image, side) = sheet(frames)?;
    let width_scale = if g.art == "bar" { 0.5 } else { 1.0 };
    let burst = Burst {
        count: g.count,
        life: g.life,
        speed: g.speed,
        angle: g.angle.unwrap_or([0.0, 0.0]),
        spread: g.spread,
        gravity_scale: g.gravity_scale,
        bounce: g.bounce,
        life_loss: 0.0,
        spin: g.spin,
        // A bar keeps the rows it lights (see `art_bank`), which are a fifth of the canvas: half the
        // quad's old width puts that band at about a pixel and a half.
        size: [g.size[0] * width_scale / side, g.size[1] * width_scale / side],
        radius: 0.4,
        splash: None,
        fade_from: 1.0,
        zoom: 1.0,
    };
    let mut style = burst_style(&burst, side, gravity, frames.len() as i64, &texture)?;
    // The art is cut for `cell` world units and the quad drawn larger.
    let scale = 1.0;
    // The cells of the light the cook draws itself are cut at the largest size, 32 texels along: a smooth
    // falloff stretches without showing its texels, and a bar or petal keeps only its lit rows.
    style.art_size = if matches!(g.art, "disc" | "bar" | "leaf") { q(DISC_CELL)? } else { q(g.cell.min(g.size[0].max(g.size[1])))? };
    style.collision = g.collision;
    style.additive = 1 | i64::from(g.front) << 1 | i64::from(g.anim) << 2;
    style.stretch = q(g.stretch)?;
    style.colors = soft_colors(g.tint);
    // The opacity is the style's start alpha, so the curves are few: full, held for `hold`, then gone.
    style.start_alpha = [py_round(g.alpha * 255.0).clamp(0, 255); 2];
    style.samples = glow_curve(g, scale);
    style.opaque = true;
    style.splash = if g.collision { 0x8000 } else { 0 };
    if g.angle.is_none() {
        // A start in place: the style carries its velocity (burst_style's cone is for the rest).
        style.velocity = [
            [q(g.velocity[0])?; 2],
            [q(g.velocity[1])?; 2],
            [0; 2],
        ];
    }
    if !c.synthetic.iter().any(|t| t.0 == texture) {
        c.synthetic.push((texture.clone(), Arc::new(sheet_image)));
    }
    let mut m = [[0.0; 4]; 4];
    let rot = match g.angle {
        Some(a) => rotation_about_z(a[0]),
        None => rotation_about_z(0.0),
    };
    for r in 0..3 {
        for k in 0..3 {
            m[r][k] = rot[r][k];
        }
        m[r][3] = origin[r];
    }
    m[3][3] = 1.0;
    let (placed, basis) = placement(&m)?;
    let style = c.style_index(style);
    c.emitters.push(EmitterRow {
        scene: scene_id,
        owner,
        source: owner * 16 + key.bytes().fold(7i64, |h, b| (h * 31 + b as i64) % 4093) + c.emitters.len() as i64 % 7,
        style,
        angle_offset: 0,
        origin: placed,
        basis,
    });
    c.records.push(Json::Obj(vec![
        ("owner".into(), Json::Str(format!("fx {owner:#x}"))),
        ("name".into(), Json::Str(what.into())),
        ("part".into(), Json::Str(key)),
        ("system".into(), Json::Str("synthetic light".into())),
        ("texture".into(), Json::Str(texture)),
        ("shader".into(), Json::Str("Legacy Shaders/Particles/Additive (Soft)".into())),
        ("scaling_mode".into(), Json::Int(0)),
        ("ps_sha256".into(), Json::Str(String::new())),
    ]));
    Ok(())
}

// ---------------------------------------------------------------- sprites

fn native(source: &Source, file: &str, id: i64) -> Result<(hk_pil::Image, [f64; 4])> {
    let f = source.file(file).map_err(|e| e.to_string())?;
    let o = source.object(&f, id).map_err(|e| e.to_string())?;
    let (im, b) = native_sprite(source, &o)?;
    Ok((im.to_rgba(), b))
}

fn tk2d_named(
    source: &Source,
    file: &str,
    collection: i64,
    names: &[&str],
) -> Result<Vec<(hk_pil::Image, [f64; 4])>> {
    let f = source.file(file).map_err(|e| e.to_string())?;
    let o = source.object(&f, collection).map_err(|e| e.to_string())?;
    let coll = source.read(&o).map_err(|e| e.to_string())?;
    let defs = get(&coll, "spriteDefinitions")?.list().unwrap_or(&[]);
    let mut textures = HashMap::new();
    let mut out = Vec::new();
    for name in names {
        let index = defs
            .iter()
            .position(|d| d.get("name").and_then(Value::str).as_deref() == Some(*name))
            .ok_or(format!("no tk2d sprite {name}"))?;
        let (im, b) = tk_sprite(source, &o.file, &coll, index, &mut textures)?;
        out.push((im.to_rgba(), b));
    }
    Ok(out)
}

fn tk2d_ids(
    source: &Source,
    file: &str,
    collection: i64,
    ids: &[usize],
) -> Result<Vec<(hk_pil::Image, [f64; 4])>> {
    let f = source.file(file).map_err(|e| e.to_string())?;
    let o = source.object(&f, collection).map_err(|e| e.to_string())?;
    let coll = source.read(&o).map_err(|e| e.to_string())?;
    let mut textures = HashMap::new();
    let mut out = Vec::new();
    for &id in ids {
        let (im, b) = tk_sprite(source, &o.file, &coll, id, &mut textures)?;
        out.push((im.to_rgba(), b));
    }
    Ok(out)
}

// ---------------------------------------------------------------- collection

fn prefab_obj(source: &Source, file: &str, id: i64) -> Result<Obj> {
    let f = source.file(file).map_err(|e| e.to_string())?;
    source.object(&f, id).map_err(|e| e.to_string())
}

fn push_found(
    c: &mut Collected,
    scene_id: i64,
    owner: i64,
    found: Vec<Emitter>,
    what: &str,
    shift: [f64; 3],
) -> Result<()> {
    for mut e in found {
        for r in 0..3 {
            e.matrix[r][3] -= shift[r];
        }
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
            ("owner".into(), Json::Str(format!("fx {owner:#x}"))),
            ("name".into(), Json::Str(what.into())),
            ("part".into(), Json::Str(e.part.clone().unwrap_or_default())),
            ("system".into(), Json::Str(e.system.sid())),
            ("texture".into(), Json::Str(e.texture_id.clone())),
            ("shader".into(), Json::Str(e.shader.clone())),
            ("scaling_mode".into(), Json::Int(e.scaling_mode)),
            ("ps_sha256".into(), Json::Str(e.ps_sha256.clone())),
        ]));
    }
    Ok(())
}

/// A particle prefab spawned at `origin` (euler `rotation`), or an ignored record saying why not.
#[allow(clippy::too_many_arguments)]
fn prefab_owner(
    source: &Source,
    c: &mut Collected,
    scene_id: i64,
    owner: i64,
    file: &str,
    id: i64,
    gravity: f64,
    origin: [f64; 3],
    rotation: Option<[f64; 3]>,
    what: &str,
) -> Result<()> {
    let prefab = prefab_obj(source, file, id)?;
    match prefab_emitters_obj(source, &prefab, gravity, origin, rotation, true) {
        Ok(mut found) => {
            // The smoke is drawn by the soft path (a quarter of its light), which is faint on a
            // dark room and costs the same fill as an opaque piece: half of the authored puffs
            // keep the cloud and the frame time (measured: the door's hits took a third of its
            // presented frames with all of them).
            if THIN_SMOKE.contains(&owner) {
                for e in &mut found {
                    e.style.count = (e.style.count + 1) / 2;
                }
            }
            // The door's cloud read as speckle: a few puffs, nearly twice the size.
            if owner == FX_DOOR_DUST || owner == FX_DOOR_RUBBLE {
                for e in &mut found {
                    e.style.count = (e.style.count * 3 / 5).max(1);
                    if owner == FX_DOOR_DUST {
                        for sample in &mut e.style.samples {
                            sample.size = sample.size.map(|v| v * 9 / 5);
                        }
                    }
                }
            }
            push_found(c, scene_id, owner, found, what, [0.0; 3])
        }
        Err(e) => {
            c.ignored.push(ignore(
                Json::Str(format!("fx {owner:#x}")),
                Json::Str(format!("{file}:{id}")),
                format!("unsupported fx particle {what}: {e}"),
            ));
            Ok(())
        }
    }
}

pub fn collect(
    source: &Source,
    sc: &Scene,
    scene_id: i64,
    kinds: Kinds,
    gravity: f64,
    c: &mut Collected,
) -> Result<()> {
    if kinds.door || kinds.rocks || kinds.breakables {
        collect_hits(source, scene_id, gravity, kinds.door || kinds.rocks, kinds.door, c)?;
    }
    if kinds.door {
        collect_door(source, sc, scene_id, gravity, c)?;
    }
    if kinds.cocoon {
        collect_cocoon(source, sc, scene_id, gravity, c)?;
    }
    if kinds.jar {
        collect_jar(source, sc, scene_id, gravity, c)?;
    }
    if kinds.boss {
        collect_boss(scene_id, gravity, c)?;
    }
    Ok(())
}

/// The False Knight's dust (hkref frames of the fight: a grey-white cloud at every landing and slam,
/// heavy white mist along the floor), made of the soft glow cells the door's dust uses, so the
/// arena's effect art (nearly full) takes no new texels. The dust is a fan of puffs that rise and
/// spread from the feet, the mist a few wide, faint discs that drift along the floor and last.
fn collect_boss(scene_id: i64, gravity: f64, c: &mut Collected) -> Result<()> {
    push_glow(
        c,
        scene_id,
        FX_FK_DUST,
        &Glow {
            count: 8,
            life: [34, 50],
            angle: Some([10.0, 170.0]),
            speed: [1.5, 5.0],
            spread: [3.0, 0.2],
            size: [3.6, 3.6],
            cell: 1.2,
            grow: 1.3,
            alpha: 0.3,
            hold: 0.25,
            tint: [170.0, 180.0, 200.0],
            ..Glow::new("disc")
        },
        gravity,
        [0.0, 0.4, 0.0],
        "False Knight landing dust",
    )?;
    push_glow(
        c,
        scene_id,
        FX_FK_MIST,
        &Glow {
            count: 6,
            life: [150, 210],
            velocity: [0.5, 0.1],
            spread: [9.0, 0.3],
            size: [7.0, 2.8],
            cell: 1.8,
            grow: 1.3,
            alpha: 0.13,
            hold: 0.5,
            tint: [165.0, 180.0, 205.0],
            ..Glow::new("disc")
        },
        gravity,
        [0.0, 0.8, 0.0],
        "False Knight floor mist",
    )?;
    // The entrance landing's sparks (the original also has orange balls of light along the floor: the
    // glow cells, cut at 13 texels, drew them as hard blocks and for two frames only, so they are not cooked).
    push_glow(
        c,
        scene_id,
        FX_FK_FIRE,
        &Glow {
            count: 40,
            life: [40, 60],
            angle: Some([55.0, 125.0]),
            speed: [5.0, 16.0],
            spread: [8.0, 0.3],
            size: [0.5, 0.5],
            cell: 0.35,
            alpha: 1.0,
            hold: 0.5,
            tint: [255.0, 140.0, 40.0],
            gravity_scale: 0.7,
            ..Glow::new("disc")
        },
        gravity,
        [0.0; 3],
        "False Knight entrance sparks",
    )
}

fn named_go(sc: &Scene, name: &str) -> Option<i64> {
    sc.objects
        .iter()
        .find(|o| {
            o.typename == "GameObject"
                && o.tree.get("m_Name").and_then(Value::str).as_deref() == Some(name)
        })
        .map(|o| o.id)
}

/// Rock chips of `Particle Rock Small`: `DebrisParticle` picks one of `rock_01..04`
/// (tk2d collection resources.assets:22286), scale 1.25..2, rigid, ObjectBounce 0.5.
fn rock_frames(source: &Source) -> Result<Vec<(hk_pil::Image, [f64; 4])>> {
    tk2d_named(
        source,
        "resources.assets",
        22286,
        &["rock_01", "rock_02", "rock_03", "rock_04"],
    )
}

/// What a nail hit raises where it lands (the door's `Hit`, a Geo Rock's `Hit`): `Dust Hit Med` at
/// the nail (euler 180, 90, 270) and `Particle Rock Small` 3..5 at the nail, speed 15..20, angle 100..140.
fn collect_hits(
    source: &Source,
    scene_id: i64,
    gravity: f64,
    chips: bool,
    flash: bool,
    c: &mut Collected,
) -> Result<()> {
    // The scene of the Great Door has no room for the hit dust's art beside the strike's, and the
    // strike's own light stands in for it there.
    if !flash {
        prefab_owner(
            source,
            c,
            scene_id,
            FX_HIT_DUST,
            "sharedassets6.assets",
            500,
            gravity,
            [0.0; 3],
            Some([180.0, 90.0, 270.0]),
            "Dust Hit Med",
        )?;
    }
    if !chips {
        return Ok(());
    }
    push_burst(
        c,
        scene_id,
        FX_HIT_CHIPS,
        &Burst {
            count: 3,
            life: [90, 150],
            speed: [15.0, 20.0],
            angle: [100.0, 140.0],
            spread: [0.0, 0.0],
            gravity_scale: 1.0,
            bounce: 0.5,
            life_loss: 0.0,
            spin: 300.0,
            size: [2.2, 3.4],
            radius: 0.45,
            splash: None,
            fade_from: 0.8,
            zoom: 1.8,
        },
        &rock_frames(source)?,
        gravity,
        [0.0; 3],
        "rocks",
        "Particle Rock Small, 3..5 at the nail",
    )?;
    if !flash {
        return Ok(());
    }
    // The lavender pool of light the strike casts on the floor around the Knight.
    push_glow(
        c,
        scene_id,
        FX_HIT_FLASH,
        &Glow {
            count: 1,
            life: [20, 20],
            size: [6.5, 4.5],
            cell: 0.9,
            alpha: 0.4,
            hold: 0.25,
            tint: [190.0, 175.0, 255.0],
            ..Glow::new("disc")
        },
        gravity,
        [0.0, -0.5, 0.0],
        "Strike light pool",
    )?;
    // `Strike Nail R` and the nail's `Slash Impact` streaks at the hit.
    push_nail_strike(
        c,
        scene_id,
        FX_HIT_FLASH,
        gravity,
        [0.0; 3],
        18,
        &[
            // Three thick streaks fall from the hit through the floor line, two long thin ones cross the door.
            Streaks { count: 3, fan: [-100.0, -80.0], speed: [8.0, 14.0], width: [0.6, 0.8], stretch: 0.08, life: [8, 12] },
            Streaks { count: 2, fan: [15.0, 45.0], speed: [25.0, 35.0], width: [0.18, 0.24], stretch: 0.11, life: [7, 10] },
        ],
        "Strike Nail R",
    )
}

/// A group of the thin white streaks a nail strike throws (the `Slash Impact` sprites: one long tapered
/// streak each): a fan of directions, the speed it flies at, its width, and the length each side
/// as a multiple of the speed.
struct Streaks {
    count: i64,
    fan: [f64; 2],
    speed: [f64; 2],
    width: [f64; 2],
    stretch: f64,
    life: [i64; 2],
}

/// What every nail strike on an object that has `slashEffects` throws (the cocoon's list is `Slash Impact
/// R` and `Strike Nail R`): a white core, a ring of thin short needles (the spark cluster of `Strike Nail R`
/// is about fifteen of them round a white point) and the long streaks of `Slash Impact`, which each object
/// aims its own way (`groups`). All of it white with a trace of lilac: the original's light is not cold.
#[allow(clippy::too_many_arguments)]
fn push_nail_strike(
    c: &mut Collected,
    scene_id: i64,
    owner: i64,
    gravity: f64,
    origin: [f64; 3],
    needles: i64,
    groups: &[Streaks],
    what: &str,
) -> Result<()> {
    let tint = [250.0, 245.0, 255.0];
    push_glow(
        c,
        scene_id,
        owner,
        &Glow {
            count: 1,
            life: [3, 3],
            size: [1.1, 1.1],
            cell: 0.6,
            alpha: 1.0,
            hold: 0.4,
            tint,
            front: true,
            ..Glow::new("disc")
        },
        gravity,
        origin,
        what,
    )?;
    push_glow(
        c,
        scene_id,
        owner,
        &Glow {
            count: needles,
            life: [5, 8],
            angle: Some([0.0, 360.0]),
            speed: [3.0, 7.0],
            size: [0.14, 0.2],
            cell: 1.0,
            alpha: 1.0,
            hold: 0.4,
            tint,
            stretch: 0.26,
            front: true,
            ..Glow::new("bar")
        },
        gravity,
        origin,
        what,
    )?;
    for g in groups {
        push_glow(
            c,
            scene_id,
            owner,
            &Glow {
                count: g.count,
                life: g.life,
                angle: Some(g.fan),
                speed: g.speed,
                size: g.width,
                cell: 1.0,
                alpha: 1.0,
                hold: 0.35,
                tint,
                stretch: g.stretch,
                front: true,
                ..Glow::new("bar")
            },
            gravity,
            origin,
            what,
        )?;
    }
    Ok(())
}

fn collect_door(
    source: &Source,
    sc: &Scene,
    scene_id: i64,
    gravity: f64,
    c: &mut Collected,
) -> Result<()> {
    let gate = named_go(sc, "Hallownest_Main_Gate").ok_or("no gate")?;
    let door = kid(&kids(sc, gate)?, "Door").ok_or("no Door under the gate")?;
    let at = sc.point(door, 0.0, 0.0, 0.0).map_err(|e| e.to_string())?;
    // The original's `Door_dust` and `Rubble Fall Short` are clouds of dust sprites; the sprite
    // art went to the screen as flat grey patches, so they are soft glows of the cook's own: rising
    // clouds over the door on every hit (below), and dust falling from the lintel on the stage hits.
    push_glow(
        c,
        scene_id,
        FX_DOOR_RUBBLE,
        &Glow {
            count: 6,
            life: [40, 64],
            velocity: [0.0, -3.0],
            spread: [3.0, 0.8],
            size: [2.4, 3.6],
            cell: 0.6,
            grow: 1.3,
            alpha: 0.4,
            hold: 0.2,
            tint: [150.0, 138.0, 128.0],
            ..Glow::new("disc")
        },
        gravity,
        [185.0, 75.0, 0.1],
        "Rubble falling from the lintel",
    )?;
    let rocks = rock_frames(source)?;
    // `Hit`: SpawnRandomObjectsV2 Particle Rock Small Transient 10..12 over the door's height,
    // speed 15..25, angle 100..140, origin spread (0.5, 4.5) about the door.
    push_burst(
        c,
        scene_id,
        FX_DOOR_CHIPS,
        &Burst {
            count: 6,
            life: [90, 150],
            speed: [15.0, 25.0],
            angle: [100.0, 140.0],
            spread: [0.5, 4.5],
            gravity_scale: 1.0,
            bounce: 0.0,
            life_loss: 0.0,
            spin: 300.0,
            size: [2.4, 3.6],
            radius: 0.45,
            splash: None,
            fade_from: 0.8,
            zoom: 1.8,
        },
        &rocks,
        gravity,
        at,
        "rocks",
        "Particle Rock Small Transient, 10..12 at the door",
    )?;
    // Every hit sends a pale light up the door's face, and a few soft clouds of dust rise from it.
    push_glow(
        c,
        scene_id,
        FX_DOOR_PULSE,
        &Glow {
            count: 1,
            life: [10, 10],
            velocity: [0.0, 9.0],
            size: [2.2, 2.2],
            cell: 0.6,
            alpha: 0.36,
            hold: 0.1,
            tint: [190.0, 190.0, 255.0],
            stretch: 0.4,
            ..Glow::new("disc")
        },
        gravity,
        [at[0] - 0.6, at[1] - 4.0, at[2] - 0.2],
        "Great Door light pulse",
    )?;
    push_glow(
        c,
        scene_id,
        FX_DOOR_DUST,
        &Glow {
            count: 5,
            life: [36, 52],
            velocity: [0.0, 2.2],
            spread: [0.9, 3.5],
            size: [2.8, 4.2],
            cell: 0.6,
            grow: 1.3,
            alpha: 0.32,
            hold: 0.2,
            tint: [160.0, 145.0, 135.0],
            ..Glow::new("disc")
        },
        gravity,
        at,
        "Door dust, rising clouds",
    )?;
    Ok(())
}

fn collect_cocoon(
    source: &Source,
    sc: &Scene,
    scene_id: i64,
    gravity: f64,
    c: &mut Collected,
) -> Result<()> {
    // Spatter Blue: SpatterOrange sprites[0] flies (stretched), sprites[1..] splash.
    let spatter = [1802, 2025]
        .iter()
        .map(|&id| native(source, "resources.assets", id))
        .collect::<Result<Vec<_>>>()?;
    // `Spatter Blue` x50, flung: the small round blue bubbles of the original (sprite 1802 flies, 2025
    // is the splash it leaves), speed 5 to 20 over a full circle, falling under gravity.
    push_burst(
        c,
        scene_id,
        FX_COCOON,
        &Burst {
            count: 36,
            life: [120, 180],
            speed: [5.0, 20.0],
            angle: [100.0, 440.0],
            spread: [0.0, 0.0],
            gravity_scale: 1.0,
            bounce: 0.0,
            life_loss: 0.0,
            spin: 0.0,
            size: [1.15, 1.35],
            radius: 0.3,
            splash: Some((1, 12, 4)),
            fade_from: 0.8,
            zoom: 1.5,
        },
        &spatter,
        gravity,
        COCOON_BODY,
        "spatter_blue",
        "Spatter Blue x50",
    )?;
    // The `Splat Effect` the cocoon turns on (cyan, scale 2 by 3, 6 frames at 24 a second) is a burst
    // of 12 to 16 separate aqua leaves that fan up and out of it: leaves here, each a tapered petal
    // (a petal cell cut 32 texels long) flown outward along its own length.
    push_glow(
        c,
        scene_id,
        FX_COCOON,
        &Glow {
            count: 14,
            life: [12, 18],
            angle: Some([10.0, 170.0]),
            speed: [7.0, 15.0],
            size: [1.0, 1.4],
            alpha: 0.95,
            hold: 0.5,
            tint: [0.0, 228.0, 255.0],
            stretch: 0.02,
            front: true,
            ..Glow::new("leaf")
        },
        gravity,
        COCOON_BODY,
        "Splat Effect leaves",
    )?;
    // The nail's own strike: `slashEffects` are `Slash Impact R` and `Strike Nail R`.
    push_nail_strike(
        c,
        scene_id,
        FX_COCOON,
        gravity,
        COCOON_BODY,
        10,
        &[Streaks { count: 3, fan: [18.0, 42.0], speed: [26.0, 34.0], width: [0.18, 0.24], stretch: 0.11, life: [8, 12] }],
        "Cocoon slashEffects",
    )?;
    // `ScuttlerControl`'s SpawnBlood: 12 to 18 drops, speed 4 to 22, angle 30 to 150, in the spatter's
    // blue; it shares the spatters' sheet.
    push_burst(
        c,
        scene_id,
        FX_SCUTTLER_BLOOD,
        &Burst {
            count: 15,
            life: [90, 150],
            speed: [4.0, 22.0],
            angle: [30.0, 150.0],
            spread: [0.0, 0.0],
            gravity_scale: 1.0,
            bounce: 0.0,
            life_loss: 0.0,
            spin: 0.0,
            size: [0.7, 1.0],
            radius: 0.3,
            splash: Some((1, 12, 4)),
            fade_from: 0.8,
            zoom: 1.0,
        },
        &spatter,
        gravity,
        [0.0; 3],
        "spatter_blue",
        "Health Scuttler blood x12..18",
    )?;
    // Blue Glob: tk2d sharedassets6:1135, Glob Fall flies, Glob Land lies.
    let glob = tk2d_ids(source, "sharedassets6.assets", 1135, &[95, 101])?;
    push_burst(
        c,
        scene_id,
        FX_COCOON,
        &Burst {
            count: 9,
            life: [150, 240],
            speed: [2.0, 8.0],
            angle: [100.0, 440.0],
            spread: [0.0, 0.0],
            gravity_scale: 0.9,
            bounce: 0.0,
            life_loss: 0.0,
            spin: 0.0,
            size: [1.0, 2.4],
            radius: 0.3,
            splash: Some((1, 100, 4)),
            fade_from: 0.8,
            zoom: 2.0,
        },
        &glob,
        gravity,
        COCOON_BODY,
        "blue_glob",
        "Blue Glob x8..10",
    )?;
    // The cap: the Cocoon Cap's SpriteRenderer sprite, knocked away from the Knight. Tutorial_01's
    // effect art has no room for it beside the splat and the globs (the cook drops the costliest
    // style first, and it was always dropped there), so it is not cooked while `COCOON_CAP` is off.
    for o in sc.objects.iter().filter(|o| COCOON_CAP_ART && o.typename == "HealthCocoon") {
        let cap = get(&o.tree, "cap")?;
        let body = sc.object(i(cap, "m_PathID")?).ok_or("cap body")?;
        let go = i(get(&body.tree, "m_GameObject")?, "m_PathID")?;
        let renderer = component_records(sc, go)
            .into_iter()
            .find(|r| r.1 == "SpriteRenderer")
            .ok_or("cap sprite renderer")?;
        let sprite = sc
            .deref(get(renderer.2, "m_Sprite")?)
            .map_err(|e| e.to_string())?;
        let (im, b) = native_sprite(source, &sprite)?;
        let cap_frames = vec![(im.to_rgba(), b)];
        for (owner, angle) in [(FX_COCOON_CAP_RIGHT, 30.0), (FX_COCOON_CAP_LEFT, 150.0)] {
            push_burst(
                c,
                scene_id,
                owner,
                &Burst {
                    count: 1,
                    life: [150, 150],
                    speed: [10.0, 10.0],
                    angle: [angle, angle + 1.0],
                    spread: [0.0, 0.0],
                    gravity_scale: 1.0,
                    bounce: 0.3,
                    life_loss: 0.0,
                    spin: 300.0,
                    size: [1.0, 1.0],
                    radius: 0.4,
                    splash: None,
                    fade_from: 0.8,
                    zoom: 1.0,
                },
                &cap_frames,
                gravity,
                COCOON_CAP,
                "cocoon_cap",
                "Cocoon Cap, capHitForce 10",
            )?;
        }
        break;
    }
    Ok(())
}

fn collect_jar(
    source: &Source,
    sc: &Scene,
    scene_id: i64,
    gravity: f64,
    c: &mut Collected,
) -> Result<()> {
    // The first jar's `Pt Glass L` and `Pt Glass S`, cooked relative to the jar: the guest raises
    // them at whichever jar broke.
    for o in sc.objects.iter().filter(|o| {
        o.typename == "GameObject"
            && o.tree.get("m_Name").and_then(Value::str).as_deref() == Some("Grub Bottle")
    }) {
        let jar = o.id;
        let origin = sc.point(jar, 0.0, 0.0, 0.0).map_err(|e| e.to_string())?;
        for (name, gid) in kids(sc, jar)? {
            if !name.starts_with("Pt Glass") {
                continue;
            }
            match part_emitter(source, sc, gid, gravity, true, true, None, false, false) {
                Ok(Found::Emitter(mut e)) => {
                    // One opacity level, a third of the art: the jars' own scene (Crossroads_05)
                    // had no room for the glass at three.
                    e.style.opaque = true;
                    // Lit glass: the original's own four pieces (`Pt Glass`, 107 by 428 texels, pale
                    // translucent polygons) as light added to the room in a pale blue.
                    e.style.additive = 1;
                    let sheet_im = hk_unity::texture::texture_image(source, &e.texture, true)
                        .map_err(|x| x.to_string())?
                        .to_rgba();
                    let key = "fx:glass_light".to_string();
                    if !c.synthetic.iter().any(|t| t.0 == key) {
                        let light = light_frames(&[(sheet_im, [0.0, 0.0, 1.0, 1.0])]).remove(0).0;
                        c.synthetic.push((key.clone(), Arc::new(light)));
                    }
                    e.style.texture = key;
                    e.style.colors = soft_colors([170.0, 180.0, 215.0]);
                    e.style.start_alpha = [170, 170];
                    e.style.speed[0] = e.style.speed[0].max(5 * 65536);
                    // The pieces are cut small (13 texels) and drawn large: four 32-texel cells of the
                    // big ones did not fit Crossroads_05's art and the cook dropped them, which left
                    // only the small pieces.
                    e.style.art_size = q(0.9)?;
                    // The original's own speed over the jar, the large pieces 28 (the original has
                    // 20) and a sixth of the small ones.
                    e.style.count = if name.ends_with('L') { e.style.count * 11 / 10 } else { e.style.count / 6 };
                    for sample in &mut e.style.samples {
                        sample.size = sample.size.map(|v| v * 3 / 2);
                    }
                    push_found(c, scene_id, FX_JAR_GLASS, vec![*e], &name, origin)?;
                }
                Ok(_) => {}
                Err(e) => c.ignored.push(ignore(
                    Json::Str(format!("fx {FX_JAR_GLASS:#x}")),
                    Json::Str(format!("{name} {gid}")),
                    format!("unsupported jar glass: {e}"),
                )),
            }
        }
        // The nail's strike on the jar: the spark cluster and the white streaks.
        push_nail_strike(
            c,
            scene_id,
            FX_JAR_GLASS,
            gravity,
            [0.0, 0.7, 0.0],
            14,
            &[
                Streaks { count: 4, fan: [-8.0, 8.0], speed: [14.0, 20.0], width: [0.22, 0.3], stretch: 0.05, life: [6, 9] },
                Streaks { count: 2, fan: [172.0, 188.0], speed: [14.0, 20.0], width: [0.22, 0.3], stretch: 0.05, life: [6, 9] },
            ],
            "Grub jar strike",
        )?;
        break;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn burst() -> Burst {
        Burst {
            count: 50,
            life: [120, 180],
            speed: [5.0, 20.0],
            angle: [100.0, 440.0],
            spread: [0.0, 0.0],
            gravity_scale: 1.0,
            bounce: 0.0,
            life_loss: 0.0,
            spin: 0.0,
            size: [1.15, 1.35],
            radius: 0.3,
            splash: Some((1, 12, 4)),
            fade_from: 0.8,
            zoom: 1.0,
        }
    }

    #[test]
    fn a_circle_burst_is_a_style_the_guest_loads() {
        let s = burst_style(&burst(), 0.5, -60.0, 2, "fx:test").unwrap();
        assert_eq!(s.samples.len(), 33);
        assert_eq!((s.shape, s.arc), (10, 340 * 65536));
        assert_eq!(s.speed, [5 * 65536, 20 * 65536]);
        assert_eq!(s.force[1], [-60 * 65536; 2]);
        assert!(s.count <= POOL_CAPACITY && s.rate > 0 && s.life[0] > 0);
        // free flight, ticks 12, cell 1, quarters 4.
        assert_eq!(s.splash, 0x8000 | 12 | 1 << 8 | 4 << 12);
        assert!(s.splash >> 8 & 15 > 0, "the splash is another cell, never the flying one");
    }

    #[test]
    fn a_box_burst_carries_its_cone_in_the_axis_ranges() {
        let b = Burst {
            spread: [0.5, 4.5],
            angle: [100.0, 140.0],
            speed: [15.0, 25.0],
            splash: None,
            ..burst()
        };
        let s = burst_style(&b, 0.5, -60.0, 4, "fx:test").unwrap();
        assert_eq!((s.shape, s.speed), (5, [0, 0]));
        assert_eq!(s.shape_scale[..2], [65536, 9 * 65536]);
        // Up and to the left: x negative, y positive, and the speed range spans both ends.
        assert!(s.velocity[0][1] < 0 && s.velocity[1][0] > 0);
        assert!(s.velocity[1][1] >= 25 * 65536 * 7660 / 10000, "sin 100 at 25 m/s");
    }

    #[test]
    fn a_sprite_becomes_the_grey_of_its_light() {
        let mut im = hk_pil::Image::new(hk_pil::Mode::Rgba, 2, 1);
        im.data.copy_from_slice(&[255, 255, 255, 255, 255, 255, 255, 40]);
        let out = light_frames(&[(im, [0.0, 0.0, 0.2, 0.1])]);
        assert_eq!(out.len(), 1);
        let d = &out[0].0.data;
        // A white opaque texel is full light; a faint one (alpha 40) is dim, its luminance times its
        // alpha. The box is the sprite's own.
        assert_eq!(d[..4], [255, 255, 255, 255]);
        assert_eq!(d[4..7], [40, 40, 40]);
        assert_eq!(out[0].1, [0.0, 0.0, 0.2, 0.1]);
    }

    #[test]
    fn sheets_stack_cells_from_the_bottom_on_square_canvases() {
        let a = hk_pil::Image::new(hk_pil::Mode::Rgba, 20, 30);
        let b = hk_pil::Image::new(hk_pil::Mode::Rgba, 40, 10);
        let (im, side) = sheet(&[(a, [0.0, 0.0, 0.2, 0.3]), (b, [0.0, 0.0, 0.4, 0.1])]).unwrap();
        assert_eq!((im.width, im.height), (40, 80));
        assert!((side - 0.4).abs() < 1e-9, "40 px at 0.01 units a pixel");
    }
}
