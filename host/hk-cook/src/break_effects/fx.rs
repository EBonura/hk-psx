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
/// `Dust Hit Med`, where the nail met an object (anchor: the hit).
pub const FX_HIT_DUST: i64 = FX_OWNER | 0x01;
/// The Great Door's `Door_dust` (cooked in place) and its `Rubble Fall Short`.
pub const FX_DOOR_DUST: i64 = FX_OWNER | 0x10;
pub const FX_DOOR_RUBBLE: i64 = FX_OWNER | 0x11;
/// The door's `Particle Rock Small Transient` spray (anchor: the door).
pub const FX_DOOR_CHIPS: i64 = FX_OWNER | 0x12;
/// `Particle Rock Small` at the nail (anchor: the hit).
pub const FX_HIT_CHIPS: i64 = FX_OWNER | 0x13;
/// The cocoon's spatters and globs (anchor: the cocoon), and its cap flown either way.
pub const FX_COCOON: i64 = FX_OWNER | 0x20;
pub const FX_COCOON_CAP_RIGHT: i64 = FX_OWNER | 0x21;
pub const FX_COCOON_CAP_LEFT: i64 = FX_OWNER | 0x22;
/// A struck Health Scuttler's blood (`SpawnBlood`, colour (0.145, 0.656, 0.860)).
pub const FX_SCUTTLER_BLOOD: i64 = FX_OWNER | 0x23;
/// A grub jar's `Pt Glass L` and `Pt Glass S` (anchor: the jar).
pub const FX_JAR_GLASS: i64 = FX_OWNER | 0x30;
/// `HardLandEffect`'s dust (`Knight Hard Land Dust B` and `F`) and its two flung rock bursts, at
/// the Knight, in every scene.
pub const FX_HARD_LAND_DUST: i64 = FX_OWNER | 0x40;
pub const FX_HARD_LAND_ROCKS_HIGH: i64 = FX_OWNER | 0x41;
pub const FX_HARD_LAND_ROCKS_LOW: i64 = FX_OWNER | 0x42;

/// Where the cocoon's bursts leave it, from the cocoon's origin: the `Splat Sprite`'s place, and
/// the `Cocoon Cap`'s local position (Tutorial_01, level6).
const COCOON_BODY: [f64; 3] = [0.388, 0.668, 0.0];
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
}
impl Kinds {
    pub fn any(self) -> bool {
        self.door || self.cocoon || self.jar || self.rocks || self.breakables
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
        size: qs([side * b.size[0], side * b.size[1]])?,
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
        // Landing splash (ticks, cell, quarters), and always flying on outside the coverage.
        splash: 0x8000
            | b.splash.map_or(0, |(cell, ticks, quarters)| {
                i64::from(ticks) | i64::from(cell) << 8 | i64::from(quarters) << 12
            }),
        samples: curve(b.fade_from)
            .into_iter()
            .map(|mut s| {
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

/// What a hard landing raises in every scene (`HardLandEffect.OnEnable` for the Dust environment,
/// prefab resources.assets:4666): the child `Dust`'s two particle systems, and two
/// `SpawnRandomObjectsV2` bursts of `Particle Rock Small Shrinker` (2 to 3 each, speed 12 to 15,
/// angles 95 to 140 and 40 to 85, from 0.9 below the Knight; the rock rests 8 s and shrinks over 1 s).
/// The impact lines (a sprite animation) are not cooked. Optional like the Focus dust: a scene whose
/// art budget has no room goes without.
pub fn collect_hard_land(
    source: &Source,
    scenes: &[i64],
    gravity: f64,
    c: &mut Collected,
) -> Result<()> {
    super::collect_hero_effect(
        source,
        gravity,
        scenes,
        c,
        FX_HARD_LAND_DUST,
        &[
            (5272, "Knight Hard Land Dust B"),
            (7198, "Knight Hard Land Dust F"),
        ],
        None,
        false,
        ("HardLandEffect", "Dust: Knight Hard Land Dust B and F"),
    )?;
    let rocks = rock_frames(source)?;
    for (owner, angle, what) in [
        (
            FX_HARD_LAND_ROCKS_HIGH,
            [95.0, 140.0],
            "Particle Rock Small Shrinker, 2..3 at 95..140 degrees",
        ),
        (
            FX_HARD_LAND_ROCKS_LOW,
            [40.0, 85.0],
            "Particle Rock Small Shrinker, 2..3 at 40..85 degrees",
        ),
    ] {
        for &scene_id in scenes {
            push_burst(
                c,
                scene_id,
                owner,
                &Burst {
                    count: 3,
                    life: [480, 540],
                    speed: [12.0, 15.0],
                    angle,
                    spread: [0.0, 0.0],
                    gravity_scale: 1.0,
                    bounce: 0.5,
                    life_loss: 0.0,
                    spin: 300.0,
                    size: [1.25, 2.0],
                    radius: 0.45,
                    splash: None,
                    fade_from: 0.89,
                },
                &rocks,
                gravity,
                [0.0, -0.9, 0.0],
                "rocks",
                what,
            )?;
        }
    }
    Ok(())
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
        collect_hits(source, scene_id, gravity, kinds.door || kinds.rocks, c)?;
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
    Ok(())
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
    c: &mut Collected,
) -> Result<()> {
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
    if !chips {
        return Ok(());
    }
    push_burst(
        c,
        scene_id,
        FX_HIT_CHIPS,
        &Burst {
            count: 4,
            life: [90, 150],
            speed: [15.0, 20.0],
            angle: [100.0, 140.0],
            spread: [0.0, 0.0],
            gravity_scale: 1.0,
            bounce: 0.5,
            life_loss: 0.0,
            spin: 300.0,
            size: [1.25, 2.0],
            radius: 0.45,
            splash: None,
            fade_from: 0.8,
        },
        &rock_frames(source)?,
        gravity,
        [0.0; 3],
        "rocks",
        "Particle Rock Small, 3..5 at the nail",
    )
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
    // `Hit`: Door_dust at the door + (-0.5, 0, -1), euler (1, 0, -180).
    prefab_owner(
        source,
        c,
        scene_id,
        FX_DOOR_DUST,
        "sharedassets6.assets",
        486,
        gravity,
        [at[0] - 0.5, at[1], at[2] - 1.0],
        Some([1.0, 0.0, -180.0]),
        "Door_dust",
    )?;
    // `Break 1` and `Break 2`: Rubble Fall Short at the fixed world point (185, 75, 0.1), euler (90, -180, 0).
    prefab_owner(
        source,
        c,
        scene_id,
        FX_DOOR_RUBBLE,
        "sharedassets6.assets",
        465,
        gravity,
        [185.0, 75.0, 0.1],
        Some([90.0, -180.0, 0.0]),
        "Rubble Fall Short",
    )?;
    let rocks = rock_frames(source)?;
    // `Hit`: SpawnRandomObjectsV2 Particle Rock Small Transient 10..12 over the door's height,
    // speed 15..25, angle 100..140, origin spread (0.5, 4.5) about the door.
    push_burst(
        c,
        scene_id,
        FX_DOOR_CHIPS,
        &Burst {
            count: 11,
            life: [90, 150],
            speed: [15.0, 25.0],
            angle: [100.0, 140.0],
            spread: [0.5, 4.5],
            gravity_scale: 1.0,
            bounce: 0.0,
            life_loss: 0.0,
            spin: 300.0,
            size: [1.25, 2.0],
            radius: 0.45,
            splash: None,
            fade_from: 0.8,
        },
        &rocks,
        gravity,
        at,
        "rocks",
        "Particle Rock Small Transient, 10..12 at the door",
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
    push_burst(
        c,
        scene_id,
        FX_COCOON,
        &Burst {
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
        },
        &spatter,
        gravity,
        COCOON_BODY,
        "spatter_blue",
        "Spatter Blue x50",
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
            size: [0.6, 1.6],
            radius: 0.3,
            splash: Some((1, 100, 4)),
            fade_from: 0.8,
        },
        &glob,
        gravity,
        COCOON_BODY,
        "blue_glob",
        "Blue Glob x8..10",
    )?;
    // The cap: the Cocoon Cap's SpriteRenderer sprite, knocked away from the Knight.
    for o in sc.objects.iter().filter(|o| o.typename == "HealthCocoon") {
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
    fn sheets_stack_cells_from_the_bottom_on_square_canvases() {
        let a = hk_pil::Image::new(hk_pil::Mode::Rgba, 20, 30);
        let b = hk_pil::Image::new(hk_pil::Mode::Rgba, 40, 10);
        let (im, side) = sheet(&[(a, [0.0, 0.0, 0.2, 0.3]), (b, [0.0, 0.0, 0.4, 0.1])]).unwrap();
        assert_eq!((im.width, im.height), (40, 80));
        assert!((side - 0.4).abs() < 1e-9, "40 px at 0.01 units a pixel");
    }
}
