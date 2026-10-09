//! Goams, falling stalactites and grub jars: world props with a behaviour,
//! plus the water drip and animated decor tables. Ported from host/props.py,
//! whose output it reproduces byte for byte.
//!
//! None of the three props is an enemy (no HealthManager), so the actor
//! pipeline never sees them, and the region cook treated them as scenery: a
//! Goam (`Worm Control`) was only its always-on DamageHero box, invisible; a
//! stalactite (`StalactiteControl`) was a static sprite with an always-on damage
//! box, although the source sets its damage to zero until it falls; a grub jar
//! (`Bottle Control` with its `Grub Control` child) was a static glass sprite.
//!
//! Their art is small and shared per family, so it lives in linked RAM and
//! reaches VRAM through the 64x64 animation slots, as the Shade's does; two CLUT
//! rows at (320,493) upward, out of the block host/shade.py reserved
//! (docs/BUDGET.md). Per region, the cooked draws each prop replaces are listed
//! for hiding, the same binding shape lifeblood.py and geo.py use. Each prop
//! carries its own damage box from its source collider, and names the cooked
//! hazard of that collider (if the cook made one) so the guest ignores it: the
//! guest decides when a prop hurts.
//!
//! Placements are read from each scene that holds one (found through the region
//! report), so only those scenes are loaded.
//!
//! Water drips (`WaterDrip`): a drop hangs on `Idle`, plays `Drip`, falls on
//! `Fall` under gravity until its collider touches terrain, steps down by
//! impactTranslation, plays `Impact`, and starts over. Where each drop lands is
//! measured here against the scene's terrain colliders, so game/src/drip.rs only
//! keeps the clock. The drip art itself is appended to the scene actor banks by
//! host/regions.py (host/props.py `drip_art`, still Python).

use crate::common::*;
use crate::cook::{focal, native_sprite, tk_sprite, CAM_Z};
use crate::materials::quantize_alpha_coverage;
use crate::pyjson::{dumps, Json};
use hk_pil::resample::Filter;
use hk_pil::{Image, Mode};
use hk_unity::scene::Scene;
use hk_unity::{Obj, Source, Value};
use serde_json::Value as J;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

const CLUT: (u16, u16, u16, u16) = (320, 493, 16, 1);
const CLUT_ROWS: usize = 2;
const SLOT: usize = 64;
const WORM_TRANSITIONS: &[(&str, &[(&str, &str)])] = &[
    ("Initialise", &[("FINISHED", "Up"), ("DOWN", "Down")]),
    ("Up", &[("WAIT", "Retract")]),
    ("Retract", &[("WAIT", "Down")]),
    ("Down", &[("WAIT", "Burst Rocks?")]),
    ("Burst Rocks?", &[("FINISHED", "Burst")]),
    ("Burst", &[("WAIT", "Up")]),
];
const WORM_STATES: [&str; 4] = ["Up", "Retract", "Down", "Burst"];
const GRUB_TRANSITIONS: &[(&str, &[(&str, &str)])] = &[
    ("Init", &[("FINISHED", "Idle")]),
    (
        "Idle",
        &[("ENTER", "Hero Close"), ("FREE", "Free"), ("CRY", "Cry")],
    ),
    ("Hero Close", &[("EXIT", "Sad Wait"), ("FREE", "Free")]),
    ("Free", &[("FINISHED", "Leave")]),
    ("Leave", &[("FINISHED", "Dig")]),
    ("Dig", &[("FINISHED", "Destroy")]),
];
const BOTTLE_TRANSITIONS: &[(&str, &[(&str, &str)])] = &[
    ("Idle", &[("NAIL HIT", "Shatter")]),
    (
        "Shatter",
        &[("FINISHED", "Destroy Self"), ("CANCEL", "Return Pause")],
    ),
];

fn scale() -> f64 {
    focal() / -CAM_Z
}

#[derive(Clone, Debug, PartialEq)]
struct Clip {
    name: String,
    frames: Vec<usize>,
    fps: f64,
    wrap: i64,
    loop_start: i64,
}

/// Frames of all three families, packed per 256x256 palette sheet.
struct Art<'s> {
    source: &'s Source,
    textures: HashMap<String, Arc<Image>>,
    images: Vec<Image>,
    boxes: Vec<[f64; 4]>,
    names: Vec<String>,
    keys: HashMap<(String, String, i64), usize>,
}

impl<'s> Art<'s> {
    fn add(&mut self, key: (String, String, i64), im: Image, b: [f64; 4], name: &str) -> usize {
        let s = scale();
        let dim = |lo: f64, hi: f64| (((hi - lo) * s).ceil() as i64).max(1) as usize;
        let (w, h) = (dim(b[0], b[2]), dim(b[1], b[3]));
        self.images.push(im.resize(w, h, Filter::Lanczos));
        self.boxes.push(b);
        self.names.push(name.to_string());
        self.keys.insert(key, self.images.len() - 1);
        self.images.len() - 1
    }

    fn tk(
        &mut self,
        file: &Arc<hk_unity::serialized::SerializedFile>,
        collection: &Value,
        index: i64,
        name: &str,
    ) -> Result<usize> {
        let obj = self
            .source
            .deref(file, collection)
            .map_err(|e| e.to_string())?;
        let key = ("tk".to_string(), obj.sid(), index);
        if let Some(&k) = self.keys.get(&key) {
            return Ok(k);
        }
        let tree = self.source.read(&obj).map_err(|e| e.to_string())?;
        let (im, b) = tk_sprite(
            self.source,
            &obj.file,
            &tree,
            index as usize,
            &mut self.textures,
        )?;
        Ok(self.add(key, im, b, name))
    }

    /// A tk2d sprite cut at `factor` times its authored size, for art the
    /// guest only ever draws at that size or smaller.
    fn tk_scaled(
        &mut self,
        file: &Arc<hk_unity::serialized::SerializedFile>,
        collection: &Value,
        index: i64,
        factor: f64,
        name: &str,
    ) -> Result<usize> {
        let obj = self
            .source
            .deref(file, collection)
            .map_err(|e| e.to_string())?;
        let key = ("tk-scaled".to_string(), obj.sid(), index);
        if let Some(&k) = self.keys.get(&key) {
            return Ok(k);
        }
        let tree = self.source.read(&obj).map_err(|e| e.to_string())?;
        let (im, b) = tk_sprite(
            self.source,
            &obj.file,
            &tree,
            index as usize,
            &mut self.textures,
        )?;
        Ok(self.add(key, im, b.map(|v| v * factor), name))
    }

    fn unity(&mut self, sc: &Scene, sprite: &Value, flip_x: bool, name: &str) -> Result<usize> {
        let obj = sc.deref(sprite).map_err(|e| e.to_string())?;
        let key = ("sprite".to_string(), obj.sid(), flip_x as i64);
        if let Some(&k) = self.keys.get(&key) {
            return Ok(k);
        }
        let (mut im, mut b) = native_sprite(self.source, &obj)?;
        if flip_x {
            im = im.flip_left_right();
            b = [-b[2], b[1], -b[0], b[3]];
        }
        Ok(self.add(key, im, b, name))
    }

    fn clip(&mut self, sc: &Scene, library_ref: &Value, name: &str, who: &str) -> Result<Clip> {
        let lib_obj: Obj = sc.deref(library_ref).map_err(|e| e.to_string())?;
        let library = self.source.read(&lib_obj).map_err(|e| e.to_string())?;
        let clips: Vec<&Value> = get(&library, "clips")?
            .list()
            .unwrap_or(&[])
            .iter()
            .filter(|c| c.get("name").and_then(Value::str).as_deref() == Some(name))
            .collect();
        if clips.len() != 1 {
            return err(format!("{who}: missing clip {name}"));
        }
        let clip = clips[0];
        let mut frames = Vec::new();
        for f in get(clip, "frames")?.list().unwrap_or(&[]) {
            frames.push(self.tk(
                &lib_obj.file,
                get(f, "spriteCollection")?,
                int_of(f, "spriteId")?,
                &format!("{who} {name}"),
            )?);
        }
        let wrap = int_of(clip, "wrapMode")?;
        if !(0..=2).contains(&wrap) {
            return err(format!("{who}: unsupported wrap mode {wrap} on {name}"));
        }
        Ok(Clip {
            name: name.to_string(),
            frames,
            fps: f64_of(clip, "fps")?,
            wrap,
            loop_start: clip.get("loopStart").and_then(Value::int).unwrap_or(0),
        })
    }
}

fn clip_name(state: &Value, who: &str) -> Result<String> {
    let plays = actions(state, "Tk2dPlayAnimation")?;
    if plays.len() != 1 {
        return err(format!(
            "{who}: expected one clip in {}",
            str_of(state, "name")?
        ));
    }
    scalar(field(&plays[0], "clipName")?)
        .str()
        .ok_or_else(|| "clip name is not a string".into())
}

/// props.py `_wait`: float(the one Wait's time).
fn wait(state: &Value, who: &str) -> Result<f64> {
    let waits = actions(state, "Wait")?;
    if waits.len() != 1 {
        return err(format!(
            "{who}: expected one Wait in {}",
            str_of(state, "name")?
        ));
    }
    py_float(&scalar(field(&waits[0], "time")?))
}

/// Python `float(x)` on a value the reader produced.
fn py_float(v: &Value) -> Result<f64> {
    match v {
        Value::Str(s) => String::from_utf8_lossy(s)
            .trim()
            .parse()
            .map_err(|_| "could not convert string to float".into()),
        other => other.float().ok_or_else(|| "not a number".into()),
    }
}

struct Goam {
    scene: i64,
    position: [f64; 2],
    quarter: i64,
    mirror: i64,
    stretch: f64,
    start_down: bool,
    collider_on: bool,
    hazard: Option<J>,
    hurt: [f64; 4],
    up_wait: f64,
    down_wait: f64,
    clips: Vec<Clip>,
    name: String,
    scene_name: String,
}

struct Stalactite {
    scene: i64,
    name: String,
    position: [f64; 2],
    frame: usize,
    embedded: Option<usize>,
    /// The embedded version's `PolygonCollider2D` box (a `Breakable`: any nail
    /// hit breaks it) and its offset from the stalactite, relative to it.
    embedded_hit: [f64; 4],
    embedded_dy: f64,
    hazard: Option<J>,
    hurt: [f64; 4],
    trigger: [f64; 4],
    fall_delay: f64,
    gravity_scale: f64,
    hit_velocity: f64,
    rocks: Rocks,
    draw: String,
    scene_name: String,
}

/// What an upward slash flings (`FlingObjects`): `Random.Range(spawnMin,
/// spawnMax + 1)` of the `hitUpRockPrefabs` rock, each at an integer
/// `Random.Range(speedMin, speedMax)` (so `speedMax` itself never) toward a
/// float angle in [0, 360). The rock prefab's components decide the rest:
/// DebrisParticle picks its sprite, scale and black tint and gives it one
/// torque step of -vx; ObjectBounce reflects it off terrain; FinishingRigidBody
/// shrinks it away once it has slept for waitDuration, or as soon as it has
/// been off screen for ten frames.
#[derive(Clone, PartialEq, Debug)]
struct Rocks {
    prefab: String,
    frames: Vec<usize>,
    count: [i64; 2],
    speed: [i64; 2],
    scale: [f64; 2],
    black: f64,
    /// BoxCollider2D in the rock's own units, as [x0, y0, x1, y1].
    collider: [f64; 4],
    /// Degrees per second of spin per unit per second of vx, at scale 1: one
    /// 50 Hz step of AddTorque(-vx) on a box of mass 1.
    spin: f64,
    gravity_scale: f64,
    bounce: f64,
    bounce_threshold: f64,
    wait: f64,
    shrink: f64,
}

fn rocks(sc: &Scene, art: &mut Art, control: &Value) -> Result<Rocks> {
    let source = art.source;
    let go = sc
        .deref(get(control, "hitUpRockPrefabs")?)
        .map_err(|e| e.to_string())?;
    let tree = source.read(&go).map_err(|e| e.to_string())?;
    let mut parts: HashMap<String, (Obj, Value)> = HashMap::new();
    for c in get(&tree, "m_Component")?.list().unwrap_or(&[]) {
        let obj = source
            .deref(&go.file, get(c, "component")?)
            .map_err(|e| e.to_string())?;
        let name = source.typename(&obj).map_err(|e| e.to_string())?;
        let value = source.read(&obj).map_err(|e| e.to_string())?;
        if parts.insert(name.clone(), (obj, value)).is_some() {
            return err(format!("stalactite rock has two {name}"));
        }
    }
    let part = |name: &str| {
        parts
            .get(name)
            .map(|p| &p.1)
            .ok_or_else(|| format!("stalactite rock without {name}"))
    };
    let num = |v: &Value, key: &str| py_float(get(v, key)?);
    let (sprite, debris, body, collider, bounce, finish) = (
        part("tk2dSprite")?,
        part("DebrisParticle")?,
        part("Rigidbody2D")?,
        part("BoxCollider2D")?,
        part("ObjectBounce")?,
        part("FinishingRigidBody")?,
    );
    // Only the shape the guest runs: a whole-unit sprite scale, a mass-1 body
    // without drag, a frictional box that does not bounce by itself, and a
    // rock that is recycled where it ends.
    let unit = |v: &Value| -> Result<bool> { Ok(num(v, "x")? == 1.0 && num(v, "y")? == 1.0) };
    if !unit(get(sprite, "_scale")?)?
        || num(body, "m_Mass")? != 1.0
        || num(body, "m_LinearDamping")? != 0.0
        || get(body, "m_UseAutoMass")?.truthy()
    {
        return err("stalactite rock body changed");
    }
    if get(finish, "conclusion")?.int() != Some(1) || get(finish, "persistOffScreen")?.truthy() {
        return err("stalactite rock no longer recycles off screen");
    }
    let material = source
        .deref(&parts["BoxCollider2D"].0.file, get(collider, "m_Material")?)
        .map_err(|e| e.to_string())?;
    if num(
        &source.read(&material).map_err(|e| e.to_string())?,
        "bounciness",
    )? != 0.0
    {
        return err("stalactite rock material bounces");
    }
    let (size, offset) = (get(collider, "m_Size")?, get(collider, "m_Offset")?);
    let (w, h, ox, oy) = (
        num(size, "x")?,
        num(size, "y")?,
        num(offset, "x")?,
        num(offset, "y")?,
    );
    let collection_file = parts["tk2dSprite"].0.file.clone();
    let collection_obj = source
        .deref(&collection_file, get(sprite, "collection")?)
        .map_err(|e| e.to_string())?;
    let collection = source.read(&collection_obj).map_err(|e| e.to_string())?;
    let definitions = get(&collection, "spriteDefinitions")?.list().unwrap_or(&[]);
    let scale = [num(debris, "scaleMin")?, num(debris, "scaleMax")?];
    let mut frames = Vec::new();
    for id in get(debris, "randomSpriteIds")?.list().unwrap_or(&[]) {
        let name = id.str().ok_or("rock sprite id is not a name")?;
        let index = definitions
            .iter()
            .position(|d| d.get("name").and_then(Value::str).as_deref() == Some(name.as_str()))
            .ok_or(format!("no rock sprite {name}"))?;
        frames.push(art.tk_scaled(
            &collection_file,
            get(sprite, "collection")?,
            index as i64,
            scale[1],
            "Stalactite rock",
        )?);
    }
    let (speed_min, speed_max) = (int_of(control, "speedMin")?, int_of(control, "speedMax")?);
    if frames.is_empty() || speed_max <= speed_min || !(0.0 < scale[0] && scale[0] <= scale[1]) {
        return err("stalactite rock fling changed");
    }
    let inertia = (w * w + h * h) / 12.0;
    Ok(Rocks {
        prefab: go.sid(),
        frames,
        count: [int_of(control, "spawnMin")?, int_of(control, "spawnMax")?],
        speed: [speed_min, speed_max - 1],
        scale,
        black: num(debris, "blackChance")?,
        collider: [ox - w / 2.0, oy - h / 2.0, ox + w / 2.0, oy + h / 2.0],
        spin: 0.02 * (180.0 / std::f64::consts::PI) / inertia,
        gravity_scale: num(body, "m_GravityScale")?,
        bounce: num(bounce, "bounceFactor")?,
        bounce_threshold: num(bounce, "speedThreshold")?,
        wait: num(finish, "waitDuration")?,
        shrink: num(finish, "shrinkDuration")?,
    })
}

struct Grub {
    scene: i64,
    local: usize,
    source: String,
    position: [f64; 2],
    grub_position: [f64; 2],
    mirror: i64,
    glass: usize,
    body: [f64; 4],
    reach: [f64; 4],
    close: [f64; 4],
    cry_wait: [f64; 2],
    free_wait: f64,
    leave_wait: f64,
    clips: Vec<Clip>,
    draw: String,
    scene_name: String,
}

fn point2(sc: &Scene, gid: i64) -> Result<[f64; 2]> {
    let p = sc.point(gid, 0.0, 0.0, 0.0).map_err(|e| e.to_string())?;
    Ok([p[0], p[1]])
}

fn world(sc: &Scene, gid: i64) -> Result<[[f64; 4]; 4]> {
    sc.world(*sc.go_transform.get(&gid).ok_or("no transform")?)
        .map_err(|e| e.to_string())
}

fn goams(sc: &Scene, art: &mut Art, hazards: &HashMap<String, J>) -> Result<Vec<Goam>> {
    let mut out = Vec::new();
    for o in sc.objects.iter().filter(|o| o.typename == "GameObject") {
        let gid = o.id;
        let name = str_of(&o.tree, "m_Name")?;
        if !name.starts_with("Worm") || !sc.active(gid) {
            continue;
        }
        let records = component_records(sc, gid);
        if !has_fsm(&records, "Worm Control") {
            continue;
        }
        let f = fsm(&records, "Worm Control")?;
        if f.get("startState").and_then(Value::str).as_deref() != Some("Initialise") {
            return err("Worm Control begins elsewhere");
        }
        let st = states(f, WORM_TRANSITIONS, "Goam")?;
        let vars = variables(f);
        let (box_id, bx) = one(&records, "BoxCollider2D")?;
        let damage: Vec<&Value> = records
            .iter()
            .filter(|r| r.1 == "DamageHero")
            .map(|r| r.2)
            .collect();
        if damage.len() != 1
            || !damage[0]
                .get("damageDealt")
                .is_some_and(|d| d.py_eq(&Value::Int(1)))
        {
            return err("Goam contact damage changed");
        }
        let (_, animator) = one(&records, "tk2dSpriteAnimator")?;
        let (q, mirror, stretch) = quarter(&world(sc, gid)?)?;
        let hazard = hazards.get(&sc.sid(box_id)).cloned();
        let mut clips = Vec::new();
        for s in WORM_STATES {
            clips.push(art.clip(
                sc,
                get(animator, "library")?,
                &clip_name(state(&st, s)?, "Goam")?,
                "Goam",
            )?);
        }
        out.push(Goam {
            scene: 0,
            position: point2(sc, gid)?,
            quarter: q,
            mirror,
            stretch,
            start_down: vars
                .iter()
                .find(|(k, _)| k == "Start Down")
                .is_some_and(|(_, v)| v.truthy()),
            collider_on: get(bx, "m_Enabled")?.truthy(),
            hazard,
            hurt: box_world(sc, gid, bx)?,
            up_wait: wait(state(&st, "Up")?, "Goam")?,
            down_wait: wait(state(&st, "Down")?, "Goam")?,
            clips,
            name,
            scene_name: String::new(),
        });
    }
    Ok(out)
}

fn stalactites(sc: &Scene, art: &mut Art, hazards: &HashMap<String, J>) -> Result<Vec<Stalactite>> {
    let mut out = Vec::new();
    for o in &sc.objects {
        if o.typename != "StalactiteControl" || !get(&o.tree, "m_Enabled")?.truthy() {
            continue;
        }
        let gid = go_of(&o.tree).unwrap_or(0);
        if !sc.active(gid) {
            continue;
        }
        let records = component_records(sc, gid);
        let (poly_id, poly) = one(&records, "PolygonCollider2D")?;
        let (_, body) = one(&records, "Rigidbody2D")?;
        let (renderer_id, renderer) = one(&records, "SpriteRenderer")?;
        let hazard = hazards.get(&sc.sid(poly_id)).cloned();
        let polygon = collider_polygons(sc, gid, "PolygonCollider2D", poly)?.remove(0);
        let here = point2(sc, gid)?;
        let ks = kids(sc, gid)?;
        let alert = kid(&ks, "Alert Range New").ok_or("stalactite without its trigger")?;
        let alert_records = component_records(sc, alert);
        let (_, trigger) = one(&alert_records, "BoxCollider2D")?;
        let mut embedded_frame = None;
        let (mut embedded_hit, mut embedded_dy) = ([0.0; 4], 0.0);
        if let Some(embedded) = kid(&ks, "Embedded") {
            // The fallen stalactite: `Breakable` + a trigger polygon, switched on at the landing.
            let embedded_records = component_records(sc, embedded);
            if !embedded_records.iter().any(|r| r.1 == "Breakable") {
                return err("the embedded stalactite is no longer a Breakable");
            }
            let (epoly_id, epoly) = one(&embedded_records, "PolygonCollider2D")?;
            let polygon = collider_polygons(sc, embedded, "PolygonCollider2D", epoly)?.remove(0);
            let _ = epoly_id;
            embedded_hit = [
                py_min(polygon.iter().map(|p| p.0)) - here[0],
                py_min(polygon.iter().map(|p| p.1)) - here[1],
                py_max(polygon.iter().map(|p| p.0)) - here[0],
                py_max(polygon.iter().map(|p| p.1)) - here[1],
            ];
            embedded_dy = point2(sc, embedded)?[1] - here[1];
            let er: Vec<&Value> = component_records(sc, embedded)
                .into_iter()
                .filter(|r| r.1 == "SpriteRenderer")
                .map(|r| r.2)
                .collect();
            if let Some(r) = er.first() {
                embedded_frame = Some(art.unity(
                    sc,
                    get(r, "m_Sprite")?,
                    get(r, "m_FlipX")?.truthy(),
                    "Stalactite embedded",
                )?);
            }
        }
        let (q, mirror, _) = quarter(&world(sc, gid)?)?;
        if q != 0 || mirror != 1 {
            return err("rotated stalactite unsupported");
        }
        let frame = art.unity(
            sc,
            get(renderer, "m_Sprite")?,
            get(renderer, "m_FlipX")?.truthy(),
            "Stalactite",
        )?;
        // Cut right after the first stalactite frame, so the rock parts sit
        // inside the stalactites' run of the art (host/code_modules.py gives
        // a part no frame names to the kind before it).
        let rocks = rocks(sc, art, &o.tree)?;
        out.push(Stalactite {
            scene: 0,
            name: str_of(sc.go(gid).ok_or("no GameObject")?, "m_Name")?,
            position: here,
            frame,
            embedded: embedded_frame,
            embedded_hit,
            embedded_dy,
            hazard,
            hurt: [
                py_min(polygon.iter().map(|p| p.0)) - here[0],
                py_min(polygon.iter().map(|p| p.1)) - here[1],
                py_max(polygon.iter().map(|p| p.0)) - here[0],
                py_max(polygon.iter().map(|p| p.1)) - here[1],
            ],
            trigger: box_world(sc, alert, trigger)?,
            fall_delay: py_float(get(&o.tree, "fallDelay")?)?,
            gravity_scale: py_float(get(body, "m_GravityScale")?)?,
            hit_velocity: py_float(get(&o.tree, "hitVelocity")?)?,
            rocks,
            draw: sc.sid(renderer_id),
            scene_name: String::new(),
        });
    }
    Ok(out)
}

fn grubs(sc: &Scene, art: &mut Art) -> Result<Vec<Grub>> {
    let mut out = Vec::new();
    for o in sc.objects.iter().filter(|o| o.typename == "GameObject") {
        let gid = o.id;
        if str_of(&o.tree, "m_Name")? != "Grub Bottle" || !sc.active(gid) {
            continue;
        }
        let records = component_records(sc, gid);
        let bottle = fsm(&records, "Bottle Control")?;
        states(bottle, BOTTLE_TRANSITIONS, "grub jar")?;
        if !records.iter().any(|r| r.1 == "PersistentBoolItem") {
            return err("grub jar without its saved state");
        }
        let (_, bx) = one(&records, "BoxCollider2D")?;
        let (renderer_id, renderer) = one(&records, "SpriteRenderer")?;
        let ks = kids(sc, gid)?;
        let (Some(grub), Some(hero_range)) = (kid(&ks, "Grub"), kid(&ks, "Hero Range")) else {
            return err("grub jar without its grub or its range");
        };
        let grecords = component_records(sc, grub);
        let control = fsm(&grecords, "Grub Control")?;
        let st = states(control, GRUB_TRANSITIONS, "grub")?;
        let (_, animator) = one(&grecords, "tk2dSpriteAnimator")?;
        let (_, close) = one(&grecords, "BoxCollider2D")?;
        let range_records = component_records(sc, hero_range);
        let (_, reach) = one(&range_records, "BoxCollider2D")?;
        let (q, mirror, _) = quarter(&world(sc, grub)?)?;
        if q != 0 {
            return err("rotated grub unsupported");
        }
        let cry = actions(state(&st, "Idle")?, "WaitRandom")?;
        if cry.len() != 1 {
            return err("grub cry timer changed");
        }
        let lib_ref = get(animator, "library")?;
        let library = sc
            .source
            .read(&sc.deref(lib_ref).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let default = int_of(animator, "defaultClipId")?;
        let idle_name = str_of(
            get(&library, "clips")?
                .list()
                .and_then(|l| l.get(default as usize))
                .ok_or("bad defaultClipId")?,
            "name",
        )?;
        let clips = vec![
            art.clip(sc, lib_ref, &idle_name, "Grub")?,
            art.clip(
                sc,
                lib_ref,
                &clip_name(state(&st, "Hero Close")?, "grub")?,
                "Grub",
            )?,
            art.clip(
                sc,
                lib_ref,
                &clip_name(state(&st, "Leave")?, "grub")?,
                "Grub",
            )?,
        ];
        let position = point2(sc, gid)?;
        let grub_position = point2(sc, grub)?;
        let glass = art.unity(
            sc,
            get(renderer, "m_Sprite")?,
            get(renderer, "m_FlipX")?.truthy(),
            "Grub jar",
        )?;
        out.push(Grub {
            scene: 0,
            local: 0,
            source: sc.sid(gid),
            position,
            grub_position,
            mirror,
            glass,
            body: box_world(sc, gid, bx)?,
            reach: box_world(sc, hero_range, reach)?,
            close: box_world(sc, grub, close)?,
            cry_wait: [
                py_float(&scalar(field(&cry[0], "timeMin")?))?,
                py_float(&scalar(field(&cry[0], "timeMax")?))?,
            ],
            free_wait: wait(state(&st, "Free")?, "grub")?,
            leave_wait: wait(state(&st, "Leave")?, "grub")?,
            clips,
            draw: sc.sid(renderer_id),
            scene_name: String::new(),
        });
    }
    Ok(out)
}

/// What `pack` returns: palettes, texel blob, parts, (first part, count) per frame, sheets.
type Packed = (
    Vec<[u8; 32]>,
    Vec<u8>,
    Vec<Part>,
    Vec<(usize, usize)>,
    Vec<Image>,
);

struct Part {
    offset: usize,
    width: usize,
    height: usize,
    clut: usize,
    bounds: [f64; 4],
}

/// props.py `pack`: frames to row strips of at most 64 texels, quantized per palette sheet.
fn pack(art: &Art) -> Result<Packed> {
    fn shelf(sizes: &[(usize, usize)]) -> Option<Vec<(usize, usize)>> {
        let limit = 256;
        let mut order: Vec<usize> = (0..sizes.len()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(sizes[i].1));
        let mut pos = vec![(0, 0); sizes.len()];
        let (mut x, mut y, mut row) = (0, 0, 0);
        for i in order {
            let (w, h) = sizes[i];
            if w > limit || h > limit {
                return None;
            }
            if x + w > limit {
                x = 0;
                y += row;
                row = 0;
            }
            if y + h > limit {
                return None;
            }
            pos[i] = (x, y);
            x += w;
            row = row.max(h);
        }
        Some(pos)
    }
    for (i, im) in art.images.iter().enumerate() {
        if im.width > SLOT {
            return err(format!(
                "{} frame ({}, {}) is wider than an animation slot",
                art.names[i], im.width, im.height
            ));
        }
    }
    let size = |i: usize| (art.images[i].width, art.images[i].height);
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for grub_family in [true, false] {
        let mut current: Vec<usize> = Vec::new();
        for i in (0..art.images.len()).filter(|&i| art.names[i].starts_with("Grub") == grub_family)
        {
            let mut candidate = current.clone();
            candidate.push(i);
            if shelf(&candidate.iter().map(|&j| size(j)).collect::<Vec<_>>()).is_none() {
                groups.push(std::mem::take(&mut current));
                current = vec![i];
            } else {
                current = candidate;
            }
        }
        if !current.is_empty() {
            groups.push(current);
        }
    }
    if groups.len() > CLUT_ROWS {
        return err(format!(
            "prop art needs {} palettes, {} rows are free",
            groups.len(),
            CLUT_ROWS
        ));
    }
    let (mut palettes, mut blob, mut parts, mut frames, mut sheets) = (
        Vec::new(),
        Vec::new(),
        Vec::new(),
        vec![(0, 0); art.images.len()],
        Vec::new(),
    );
    for (clut, group) in groups.iter().enumerate() {
        let positions =
            shelf(&group.iter().map(|&j| size(j)).collect::<Vec<_>>()).ok_or("shelf failed")?;
        let mut sheet = Image::new(Mode::Rgba, 256, 256);
        for (&i, &(x, y)) in group.iter().zip(&positions) {
            sheet.paste(&art.images[i], x as i64, y as i64, None);
        }
        let q = quantize_alpha_coverage(&sheet, 128)?;
        palettes.push(q.palette);
        for (&i, &(x0, y0)) in group.iter().zip(&positions) {
            let (im, b) = (&art.images[i], art.boxes[i]);
            let first = parts.len();
            let mut top = 0;
            while top < im.height {
                let h = SLOT.min(im.height - top);
                let stride = im.width.div_ceil(4) * 2;
                let mut texels = vec![0u8; stride * h];
                for y in 0..h {
                    for x in 0..im.width {
                        let byte = q.packed[(y + y0 + top) * q.width.div_ceil(2) + (x + x0) / 2];
                        let n = (byte >> (((x + x0) & 1) * 4)) & 15;
                        texels[y * stride + x / 2] |= n << ((x & 1) * 4);
                    }
                }
                let span = b[3] - b[1];
                let bounds = [
                    b[0],
                    b[3] - span * (top + h) as f64 / im.height as f64,
                    b[2],
                    b[3] - span * top as f64 / im.height as f64,
                ];
                parts.push(Part {
                    offset: blob.len(),
                    width: im.width,
                    height: h,
                    clut,
                    bounds,
                });
                blob.extend_from_slice(&texels);
                top += SLOT;
            }
            frames[i] = (first, parts.len() - first);
        }
        sheets.push(sheet);
    }
    Ok((palettes, blob, parts, frames, sheets))
}

/// What a nail hit does to a stalactite: the speed a sideways or downward hit
/// bats it away at, and the rocks an upward one shatters it into.
fn stalactite_hits(
    first: Option<&Stalactite>,
    gravity: f64,
    time_to_sleep: f64,
) -> Result<Vec<String>> {
    let Some(s) = first else {
        return Ok(vec![
            "pub const STALACTITE_HIT_SPEED:i32=0;".into(),
            "pub const STALACTITE_ROCK_FRAMES:&[u16]=&[];".into(),
            "pub const STALACTITE_ROCKS:RockSpec=RockSpec{count:[0,0],speed:[0,0],scale:[0,0],art_scale:65536,black:0,collider:[0,0,0,0],spin:0,gravity:0,bounce:0,bounce_threshold:0,sleep_ticks:0,wait_ticks:0,shrink_ticks:0};".into(),
        ]);
    };
    let r = &s.rocks;
    let ticks = |seconds: f64| py_round(seconds * 60.0);
    Ok(vec![
        format!("pub const STALACTITE_HIT_SPEED:i32={};", q16(s.hit_velocity)?),
        format!("pub const STALACTITE_ROCK_FRAMES:&[u16]=&{};", rust_array(&r.frames)),
        format!(
            "pub const STALACTITE_ROCKS:RockSpec=RockSpec{{count:{},speed:{},scale:{},art_scale:{},black:{},collider:{},spin:{},gravity:{},bounce:{},bounce_threshold:{},sleep_ticks:{},wait_ticks:{},shrink_ticks:{}}};",
            rust_array(&r.count),
            rust_array(&r.speed),
            rust_array(&[q16(r.scale[0])?, q16(r.scale[1])?]),
            q16(r.scale[1])?,
            q16(r.black)?,
            rust_array(&r.collider.iter().map(|&v| q16(v)).collect::<Result<Vec<_>>>()?),
            q16(r.spin)?,
            q16(gravity * r.gravity_scale)?,
            q16(r.bounce)?,
            q16(r.bounce_threshold)?,
            ticks(time_to_sleep),
            ticks(r.wait),
            ticks(r.shrink)
        ),
    ])
}

fn read_json(path: &Path) -> Result<J> {
    let text = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_slice(&text).map_err(|e| format!("{}: {e}", path.display()))
}

fn region_scene_json(root: &Path, chunk_id: i64) -> Result<J> {
    read_json(&root.join(format!("data/regions/region-{chunk_id:03}/scene.json")))
}

fn jstr(v: &J, k: &str) -> String {
    v.get(k).and_then(J::as_str).unwrap_or("").to_string()
}

/// Scene name to (file, id) from the region report's scene table.
fn scene_table(report: &J) -> HashMap<String, (String, i64)> {
    report["scenes"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|s| {
                    (
                        jstr(s, "scene_name"),
                        (jstr(s, "file"), s["scene_id"].as_i64().unwrap_or(0)),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

fn physics2d(source: &Source) -> Result<Value> {
    let file = source
        .file("globalgamemanagers")
        .map_err(|e| e.to_string())?;
    let info = file
        .objects
        .iter()
        .find(|o| o.class_id == 19)
        .ok_or("no Physics2DSettings")?;
    source
        .read(&Obj {
            file: file.clone(),
            info: *info,
        })
        .map_err(|e| e.to_string())
}

fn gravity(source: &Source) -> Result<f64> {
    Ok(-f64_of(get(&physics2d(source)?, "m_Gravity")?, "y")?)
}

fn sha_hex(b: &[u8]) -> String {
    Sha256::digest(b)
        .iter()
        .map(|x| format!("{x:02x}"))
        .collect()
}

/// Run what `python3 host/props.py` runs: the prop cook, then decor, then drips.
pub fn main(root: &Path, source_dir: Option<&Path>) -> Result<()> {
    let source = match source_dir {
        Some(d) => Source::new(d),
        None => Source::from_doctor(root),
    }
    .map_err(|e| e.to_string())?;
    let report = read_json(&root.join("data/regions.json"))?;
    let summary = cook(root, &source, &report)?;
    println!("{summary}");
    println!("decor {}", generate_decor(root, &report)?);
    println!(
        "drips {}",
        generate_drips(root, &report, &source, gravity(&source)?)?
    );
    Ok(())
}

fn cook(root: &Path, source: &Source, report: &J) -> Result<String> {
    let table = scene_table(report);
    let regions = report["regions"]
        .as_array()
        .ok_or("report without regions")?;
    let scene_jsons: Vec<J> = {
        use rayon::prelude::*;
        regions
            .par_iter()
            .map(|g| region_scene_json(root, g["chunk_id"].as_i64().unwrap_or(0)))
            .collect::<Result<Vec<_>>>()?
    };
    let mut hazards: HashMap<String, HashMap<String, J>> = HashMap::new();
    let mut want: Vec<String> = Vec::new();
    for (g, sj) in regions.iter().zip(&scene_jsons) {
        let scene = jstr(g, "scene_name");
        for h in g
            .get("hazards")
            .and_then(J::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[])
        {
            hazards
                .entry(scene.clone())
                .or_default()
                .insert(jstr(h, "collider_source"), h.clone());
            let name = jstr(h, "name");
            if (name.starts_with("Worm") || name.starts_with("Stalactite"))
                && !want.contains(&scene)
            {
                want.push(scene.clone());
            }
        }
        let draws = sj["draws"].as_array().map(Vec::as_slice).unwrap_or(&[]);
        if draws.iter().any(|d| {
            let n = jstr(d, "name");
            n == "Grub Bottle" || n.starts_with("Stalactite Hazard")
        }) && !want.contains(&scene)
        {
            want.push(scene.clone());
        }
    }
    let g = gravity(source)?;
    let mut art = Art {
        source,
        textures: HashMap::new(),
        images: Vec::new(),
        boxes: Vec::new(),
        names: Vec::new(),
        keys: HashMap::new(),
    };
    let (mut all_goams, mut all_stalactites, mut all_grubs) = (Vec::new(), Vec::new(), Vec::new());
    want.sort_by_key(|n| table[n].1);
    let empty = HashMap::new();
    for scene_name in &want {
        let (file, id) = table[scene_name].clone();
        let sc = Scene::new(source, &file).map_err(|e| e.to_string())?;
        let h = hazards.get(scene_name).unwrap_or(&empty);
        for mut x in goams(&sc, &mut art, h)? {
            x.scene = id;
            x.scene_name = scene_name.clone();
            all_goams.push(x);
        }
        for mut x in stalactites(&sc, &mut art, h)? {
            x.scene = id;
            x.scene_name = scene_name.clone();
            all_stalactites.push(x);
        }
        for (local, mut x) in grubs(&sc, &mut art)?.into_iter().enumerate() {
            x.scene = id;
            x.scene_name = scene_name.clone();
            x.local = local;
            all_grubs.push(x);
        }
    }
    let (palettes, blob, parts, frames, sheets) = pack(&art)?;
    // One clip table, checked identical across each family.
    let mut clip_frames: Vec<usize> = Vec::new();
    let mut clip_rows: Vec<(usize, usize, i64, i64, i64)> = Vec::new();
    let mut clip_index: Vec<((String, Vec<usize>), usize)> = Vec::new();
    let mut clip_id = |c: &Clip| -> usize {
        let key = (c.name.clone(), c.frames.clone());
        if let Some((_, i)) = clip_index.iter().find(|(k, _)| *k == key) {
            return *i;
        }
        let i = clip_rows.len();
        clip_index.push((key, i));
        clip_rows.push((
            clip_frames.len(),
            c.frames.len(),
            py_round(c.fps * 256.0),
            c.wrap,
            c.loop_start,
        ));
        clip_frames.extend(&c.frames);
        i
    };
    let mut family = |members: Vec<&Vec<Clip>>, n: usize, who: &str| -> Result<Vec<usize>> {
        let mut tables: Vec<Vec<usize>> = Vec::new();
        for m in members {
            let t: Vec<usize> = m.iter().map(&mut clip_id).collect();
            if !tables.contains(&t) {
                tables.push(t);
            }
        }
        if tables.len() > 1 {
            return err(format!("{who} members disagree on their clips"));
        }
        Ok(tables.pop().unwrap_or(vec![0; n]))
    };
    let goam_clips = family(all_goams.iter().map(|g| &g.clips).collect(), 4, "Goam")?;
    let grub_clips = family(all_grubs.iter().map(|g| &g.clips).collect(), 3, "grub")?;
    let same = |v: Vec<Vec<f64>>| {
        v.windows(2).all(|w| {
            w[0].iter()
                .map(|x| x.to_bits())
                .eq(w[1].iter().map(|x| x.to_bits()))
        })
    };
    for (field, vals) in [
        (
            "up_wait",
            all_goams
                .iter()
                .map(|g| vec![g.up_wait])
                .collect::<Vec<_>>(),
        ),
        (
            "down_wait",
            all_goams.iter().map(|g| vec![g.down_wait]).collect(),
        ),
    ] {
        if !same(vals) {
            return err(format!("Goam members disagree on {field}"));
        }
    }
    for (field, vals) in [
        (
            "free_wait",
            all_grubs
                .iter()
                .map(|g| vec![g.free_wait])
                .collect::<Vec<_>>(),
        ),
        (
            "leave_wait",
            all_grubs.iter().map(|g| vec![g.leave_wait]).collect(),
        ),
        (
            "cry_wait",
            all_grubs.iter().map(|g| g.cry_wait.to_vec()).collect(),
        ),
    ] {
        if !same(vals) {
            return err(format!("grub members disagree on {field}"));
        }
    }
    for s in &all_stalactites {
        if s.fall_delay != all_stalactites[0].fall_delay
            || s.gravity_scale != all_stalactites[0].gravity_scale
            || s.hit_velocity != all_stalactites[0].hit_velocity
        {
            return err("stalactites disagree on their fall");
        }
        if s.rocks != all_stalactites[0].rocks {
            return err("stalactites disagree on their rocks");
        }
    }
    let ticks = |seconds: f64| py_round(seconds * 60.0);
    let hazard_id = |h: &Option<J>| -> i64 {
        match h {
            Some(J::Object(m)) if !m.is_empty() => jstr(h.as_ref().unwrap(), "source")
                .rsplit(':')
                .next()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
            _ => 0,
        }
    };
    let hide: std::collections::HashSet<&str> = all_stalactites
        .iter()
        .map(|s| s.draw.as_str())
        .chain(all_grubs.iter().map(|g| g.draw.as_str()))
        .collect();
    let mut bindings: Vec<(i64, Vec<usize>)> = Vec::new();
    for (g, sj) in regions.iter().zip(&scene_jsons) {
        let off: Vec<usize> = sj["draws"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[])
            .iter()
            .enumerate()
            .filter(|(_, d)| hide.contains(jstr(d, "source").as_str()))
            .map(|(i, _)| i)
            .collect();
        if !off.is_empty() {
            bindings.push((g["chunk_id"].as_i64().unwrap_or(0) - 1, off));
        }
    }
    let b4 = |b: &[f64]| -> Result<String> {
        Ok(rust_array(
            &b.iter().map(|&v| q16(v)).collect::<Result<Vec<_>>>()?,
        ))
    };
    let mut rust: Vec<String> = vec![
        "// Generated Goam, stalactite and grub jar art and placements (host/props.py).".into(),
        format!(
            "pub const CLUT_RECT:(u16,u16,u16,u16)=({},{},{},{});",
            CLUT.0, CLUT.1, CLUT.2, CLUT.3
        ),
        format!("pub const PALETTE_COUNT:usize={};", palettes.len()),
    ];
    let mut p = Vec::new();
    for x in &parts {
        p.push(format!(
            "Part{{offset:{},width:{},height:{},clut:{},bounds:{}}}",
            x.offset,
            x.width,
            x.height,
            x.clut,
            b4(&x.bounds)?
        ));
    }
    rust.push(format!("pub const PARTS:&[Part]=&[{}];", p.join(",")));
    rust.push(format!(
        "pub const FRAMES:&[(u16,u16)]=&[{}];",
        frames
            .iter()
            .map(|(a, b)| format!("({a},{b})"))
            .collect::<Vec<_>>()
            .join(",")
    ));
    rust.push(format!(
        "pub const CLIP_FRAMES:&[u16]=&{};",
        rust_array(&clip_frames)
    ));
    rust.push(format!(
        "pub const CLIPS:&[Clip]=&[{}];",
        clip_rows
            .iter()
            .map(|c| format!(
                "Clip{{first:{},count:{},fps:{},wrap:{},loop_start:{}}}",
                c.0, c.1, c.2, c.3, c.4
            ))
            .collect::<Vec<_>>()
            .join(",")
    ));
    rust.push(format!(
        "pub const GOAM_CLIPS:[u16;4]={};",
        rust_array(&goam_clips)
    ));
    rust.push(format!(
        "pub const GOAM_UP_TICKS:u16={};",
        all_goams.first().map_or(0, |g| ticks(g.up_wait))
    ));
    rust.push(format!(
        "pub const GOAM_DOWN_TICKS:u16={};",
        all_goams.first().map_or(0, |g| ticks(g.down_wait))
    ));
    let mut gs = Vec::new();
    for g in &all_goams {
        gs.push(format!(
            "Goam{{scene:{},position:{},quarter:{},mirror:{},stretch:{},start_down:{},collider_on:{},hurt:{},hazard:{}}}",
            g.scene,
            b4(&g.position)?,
            g.quarter,
            g.mirror,
            q16(g.stretch)?,
            g.start_down,
            g.collider_on,
            b4(&g.hurt)?,
            hazard_id(&g.hazard)
        ));
    }
    rust.push(format!("pub const GOAMS:&[Goam]=&[{}];", gs.join(",")));
    rust.push(format!(
        "pub const STALACTITE_DELAY_TICKS:u16={};",
        all_stalactites.first().map_or(0, |s| ticks(s.fall_delay))
    ));
    rust.push(format!(
        "pub const STALACTITE_GRAVITY:i32={};",
        match all_stalactites.first() {
            Some(s) => q16(g * s.gravity_scale)?,
            None => 0,
        }
    ));
    let mut ss = Vec::new();
    for s in &all_stalactites {
        ss.push(format!(
            "Stalactite{{scene:{},position:{},frame:{},embedded:{},embedded_hit:{},embedded_dy:{},hurt:{},trigger:{},hazard:{}}}",
            s.scene,
            b4(&s.position)?,
            s.frame,
            s.embedded.map_or(65535, |e| e),
            b4(&s.embedded_hit)?,
            q16(s.embedded_dy)?,
            b4(&s.hurt)?,
            b4(&s.trigger)?,
            hazard_id(&s.hazard)
        ));
    }
    rust.push(format!(
        "pub const STALACTITES:&[Stalactite]=&[{}];",
        ss.join(",")
    ));
    // Named after the STALACTITES table and never with `frame:`, so the art
    // split in host/code_modules.py still reads only the table above.
    rust.extend(stalactite_hits(
        all_stalactites.first(),
        g,
        py_float(get(&physics2d(source)?, "m_TimeToSleep")?)?,
    )?);
    rust.push(format!(
        "pub const GRUB_CLIPS:[u16;3]={};",
        rust_array(&grub_clips)
    ));
    rust.push(match all_grubs.first() {
        Some(g) => format!(
            "pub const GRUB_CRY_TICKS:[u16;2]={};",
            rust_array(&[ticks(g.cry_wait[0]), ticks(g.cry_wait[1])])
        ),
        None => "pub const GRUB_CRY_TICKS:[u16;2]=[0,0];".into(),
    });
    rust.push(format!(
        "pub const GRUB_FREE_TICKS:u16={};",
        all_grubs.first().map_or(0, |g| ticks(g.free_wait))
    ));
    rust.push(format!(
        "pub const GRUB_LEAVE_TICKS:u16={};",
        all_grubs.first().map_or(0, |g| ticks(g.leave_wait))
    ));
    let mut grs = Vec::new();
    for g in &all_grubs {
        grs.push(format!(
            "Grub{{scene:{},local:{},jar:{},grub:{},mirror:{},glass:{},body:{},reach:{},close:{}}}",
            g.scene,
            g.local,
            b4(&g.position)?,
            b4(&g.grub_position)?,
            g.mirror,
            g.glass,
            b4(&g.body)?,
            b4(&g.reach)?,
            b4(&g.close)?
        ));
    }
    rust.push(format!("pub const GRUBS:&[Grub]=&[{}];", grs.join(",")));
    rust.push("/// Cooked draws each prop replaces, per catalogue region, sorted.".into());
    rust.push(format!(
        "pub const BINDINGS:&[(u16,&[u16])]=&[{}];",
        bindings
            .iter()
            .map(|(r, off)| format!("({r},&{})", rust_array(off)))
            .collect::<Vec<_>>()
            .join(",")
    ));
    let text = rust.join("\n") + "\n";
    let mut payload: Vec<u8> = palettes.iter().flat_map(|p| p.to_vec()).collect();
    payload.extend_from_slice(&blob);
    std::fs::write(root.join("data/props.rs"), &text).map_err(|e| e.to_string())?;
    std::fs::write(root.join("data/props.hk"), &payload).map_err(|e| e.to_string())?;
    let out = root.join(".hkpsx/props");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    for (i, sheet) in sheets.iter().enumerate() {
        crate::png::save_rgba(&out.join(format!("art{i}.png")), sheet)?;
    }
    let pairs = |v: Vec<(String, String)>| {
        Json::List(
            v.into_iter()
                .map(|(a, b)| Json::List(vec![Json::Str(a), Json::Str(b)]))
                .collect(),
        )
    };
    let regions_bytes = std::fs::read(root.join("data/regions.json")).map_err(|e| e.to_string())?;
    let provenance = Json::Obj(vec![
        ("goams".into(), Json::Int(all_goams.len() as i64)),
        ("stalactites".into(), Json::Int(all_stalactites.len() as i64)),
        ("grubs".into(), Json::Int(all_grubs.len() as i64)),
        ("frames".into(), Json::Int(frames.len() as i64)),
        ("parts".into(), Json::Int(parts.len() as i64)),
        ("bytes".into(), Json::Int(payload.len() as i64)),
        ("palettes".into(), Json::Int(palettes.len() as i64)),
        (
            "placements".into(),
            Json::Obj(vec![
                ("goam".into(), pairs(all_goams.iter().map(|g| (g.scene_name.clone(), g.name.clone())).collect())),
                ("stalactite".into(), pairs(all_stalactites.iter().map(|s| (s.scene_name.clone(), s.name.clone())).collect())),
                ("grub".into(), pairs(all_grubs.iter().map(|g| (g.scene_name.clone(), g.source.clone())).collect())),
            ]),
        ),
        ("payload_sha256".into(), Json::Str(sha_hex(&payload))),
        ("rust_sha256".into(), Json::Str(sha_hex(text.as_bytes()))),
        ("region_metadata_sha256".into(), Json::Str(sha_hex(&regions_bytes))),
        ("tool_sha256".into(), Json::Str(crate::tool_sha256())),
        (
            "limitations".into(),
            Json::List(vec![
                Json::Str("Goam dust, churn and burst particles and the burst sound are not presented.".into()),
                Json::Str("A stalactite's fall trail and the landing's Thunk Effect are not presented, its Strike Nail R is the Slash Impact stand-in (shown only in scenes with grass), and its dust and sounds (break_effects, scene_sfx) play only where their art and bytes fit. A batted one hurts no enemy and embeds when its tip meets terrain.".into()),
                Json::Str("Stalactite rocks fly as boxes in the Geo coin pool (its 50 Hz swept contacts and ObjectBounce reflection, stopped dead at rest, at one collider size for every scale); they do not spin, tumble or push each other, share the pool's slots with Geo, count their lifetime from first contact and are not recycled off screen.".into()),
                Json::Str("A grub cries silently, faces the Knight only by its authored mirror, and its jar leaves no glass or debris when it breaks.".into()),
            ]),
        ),
    ]);
    std::fs::create_dir_all(root.join(".hkpsx")).map_err(|e| e.to_string())?;
    std::fs::write(
        root.join(".hkpsx/props-provenance.json"),
        dumps(&provenance),
    )
    .map_err(|e| e.to_string())?;
    Ok(format!(
        "Props: {} Goams, {} stalactites, {} grub jars; {} frames in {} slots, {} bytes, {} palettes",
        all_goams.len(),
        all_stalactites.len(),
        all_grubs.len(),
        frames.len(),
        parts.len(),
        payload.len(),
        palettes.len()
    ))
}

/// data/decor.rs: per view, the animated tk2d scenery host/cook.py cooked.
fn generate_decor(root: &Path, report: &J) -> Result<String> {
    let (mut views, mut groups, mut steps) = (Vec::new(), Vec::new(), Vec::<i64>::new());
    let mut shared: Vec<(Vec<i64>, usize)> = Vec::new();
    for (slot, row) in report["regions"]
        .as_array()
        .ok_or("no regions")?
        .iter()
        .enumerate()
    {
        let first = groups.len();
        for g in row
            .get("decor")
            .and_then(J::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[])
        {
            let draws: Vec<i64> = g["draws"]
                .as_array()
                .ok_or("decor without draws")?
                .iter()
                .map(|v| v.as_i64().unwrap_or(0))
                .collect();
            if draws
                .iter()
                .enumerate()
                .any(|(i, &d)| d != draws[0] + i as i64)
            {
                return err(format!(
                    "decor {} draws are not consecutive in view {slot}",
                    jstr(g, "source")
                ));
            }
            let sequence: Vec<i64> = g["sequence"]
                .as_array()
                .ok_or("decor without sequence")?
                .iter()
                .map(|v| v.as_i64().unwrap_or(0))
                .collect();
            if draws.len() > 255 || draws[0] > 65535 || sequence.len() > 255 {
                return err(format!(
                    "decor {} does not fit the table",
                    jstr(g, "source")
                ));
            }
            let at = match shared.iter().find(|(s, _)| *s == sequence) {
                Some((_, at)) => *at,
                None => {
                    let at = steps.len();
                    shared.push((sequence.clone(), at));
                    steps.extend(&sequence);
                    at
                }
            };
            if at > 65535 {
                return err("decor step table exceeds u16");
            }
            let fps = g["fps"].as_f64().ok_or("decor fps")?;
            groups.push((
                draws[0],
                draws.len(),
                py_round(fps * 256.0),
                g["wrap"].as_i64().unwrap_or(0),
                g["loop_start"].as_i64().unwrap_or(0),
                at,
                sequence.len(),
            ));
        }
        if groups.len() > first {
            views.push((slot, first, groups.len() - first));
        }
    }
    let lines = [
        "// Generated by host/props.py from the decor groups host/cook.py records.".to_string(),
        "/// (catalogue slot, first group, group count), sorted by slot.".into(),
        format!(
            "pub static VIEWS:&[(u16,u16,u16)]=&[{}];",
            views
                .iter()
                .map(|(a, b, c)| format!("({a},{b},{c}),"))
                .collect::<String>()
        ),
        "/// (first draw, draws, fps x256, wrap, loop start, first step, steps).".into(),
        format!(
            "pub static GROUPS:&[(u16,u8,u16,u8,u8,u16,u8)]=&[{}];",
            groups
                .iter()
                .map(|g| format!("({},{},{},{},{},{},{}),", g.0, g.1, g.2, g.3, g.4, g.5, g.6))
                .collect::<String>()
        ),
        "/// Each clip step's draw offset within its group.".into(),
        format!(
            "pub static STEPS:&[u8]=&[{}];",
            steps
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .join(",")
        ),
    ];
    std::fs::write(root.join("data/decor.rs"), lines.join("\n") + "\n")
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "{{'views': {}, 'groups': {}, 'steps': {}}}",
        views.len(),
        groups.len(),
        steps.len()
    ))
}

struct Drip {
    position: [f64; 3],
    drip: Value,
    bottom: f64,
    x_scale: f64,
    half_width: f64,
    offset_x: f64,
}

/// props.py `drip_sources`: active WaterDrips with the component values the runtime assumes.
fn drip_sources(sc: &Scene) -> Result<Vec<Drip>> {
    let mut gids: Vec<i64> = sc.gos.keys().copied().collect();
    gids.sort();
    let mut out = Vec::new();
    for gid in gids {
        if !sc.active(gid) {
            continue;
        }
        let mut comps: Vec<(&str, &Value)> = Vec::new();
        for (_, t, c) in components(sc, gid)? {
            match comps.iter_mut().find(|(k, _)| *k == t) {
                Some(slot) => slot.1 = c,
                None => comps.push((t, c)),
            }
        }
        let comp = |k: &str| comps.iter().find(|(t, _)| *t == k).map(|(_, v)| *v);
        let Some(w) = comp("WaterDrip") else { continue };
        let name = str_of(sc.go(gid).unwrap(), "m_Name")?;
        let bx = match comp("BoxCollider2D") {
            Some(b) if !get(b, "m_IsTrigger")?.truthy() => b,
            _ => return err(format!("water drip {name} has no solid box collider")),
        };
        let m = world(sc, gid)?;
        let sprite = comp("tk2dSprite").ok_or("water drip without tk2dSprite")?;
        let sscale = get(sprite, "_scale")?;
        if m[0][1].abs() > 1e-6
            || m[1][0].abs() > 1e-6
            || (m[1][1] - 1.0).abs() > 1e-6
            || (f64_of(sscale, "x")? - 1.0).abs() > 1e-6
            || (f64_of(sscale, "y")? - 1.0).abs() > 1e-6
        {
            return err("water drip is rotated or scaled on y");
        }
        let (off, size) = (get(bx, "m_Offset")?, get(bx, "m_Size")?);
        out.push(Drip {
            position: sc.point(gid, 0.0, 0.0, 0.0).map_err(|e| e.to_string())?,
            drip: w.clone(),
            bottom: f64_of(off, "y")? - f64_of(size, "y")? / 2.0,
            x_scale: m[0][0],
            half_width: m[0][0].abs() * f64_of(size, "x")? / 2.0,
            offset_x: m[0][0] * f64_of(off, "x")?,
        });
    }
    Ok(out)
}

/// props.py `terrain_below`: the highest terrain surface under [x0, x1] at or below y.
fn terrain_below(sc: &Scene, x0: f64, x1: f64, y: f64) -> Result<Option<f64>> {
    let mut best: Option<f64> = None;
    for o in &sc.objects {
        let typ = o.typename.as_str();
        if !matches!(
            typ,
            "EdgeCollider2D" | "BoxCollider2D" | "PolygonCollider2D"
        ) {
            continue;
        }
        let t = &o.tree;
        let gid = go_of(t).ok_or("collider without GameObject")?;
        let Some(go) = sc.go(gid) else { continue };
        if !sc.active(gid)
            || !get(t, "m_Enabled")?.truthy()
            || get(t, "m_IsTrigger")?.truthy()
            || !get(go, "m_Layer")?.py_eq(&Value::Int(8))
        {
            continue;
        }
        let off = get(t, "m_Offset")?;
        let (ox, oy) = (f64_of(off, "x")?, f64_of(off, "y")?);
        let xy = |p: &Value| -> Result<(f64, f64)> { Ok((f64_of(p, "x")?, f64_of(p, "y")?)) };
        let paths: Vec<Vec<(f64, f64)>> = match typ {
            "BoxCollider2D" => {
                let (w, h) = (
                    f64_of(get(t, "m_Size")?, "x")? / 2.0,
                    f64_of(get(t, "m_Size")?, "y")? / 2.0,
                );
                vec![vec![(-w, -h), (w, -h), (w, h), (-w, h), (-w, -h)]]
            }
            "EdgeCollider2D" => vec![get(t, "m_Points")?
                .list()
                .unwrap_or(&[])
                .iter()
                .map(xy)
                .collect::<Result<_>>()?],
            _ => {
                let mut v = Vec::new();
                for p in get(get(t, "m_Points")?, "m_Paths")?.list().unwrap_or(&[]) {
                    let pts: Vec<(f64, f64)> = p
                        .list()
                        .unwrap_or(&[])
                        .iter()
                        .map(xy)
                        .collect::<Result<_>>()?;
                    if !pts.is_empty() {
                        let mut closed = pts.clone();
                        closed.push(pts[0]);
                        v.push(closed);
                    }
                }
                v
            }
        };
        for path in paths {
            let pts: Vec<[f64; 3]> = path
                .iter()
                .map(|&(px, py)| {
                    sc.point(gid, px + ox, py + oy, 0.0)
                        .map_err(|e| e.to_string())
                })
                .collect::<Result<_>>()?;
            for w in pts.windows(2) {
                let (a, b) = (w[0], w[1]);
                let (lo, hi) = if a[0] <= b[0] {
                    (a[0], b[0])
                } else {
                    (b[0], a[0])
                };
                if hi < x0 || lo > x1 {
                    continue;
                }
                let at = |x: f64| {
                    if a[0] == b[0] {
                        a[1]
                    } else {
                        a[1] + (x - a[0]) * (b[1] - a[1]) / (b[0] - a[0])
                    }
                };
                // Python max(lo, x0) and min(hi, x1): the first argument on ties.
                let ys = [
                    at(if x0 > lo { x0 } else { lo }),
                    at(if x1 < hi { x1 } else { hi }),
                ];
                let top = py_max(ys);
                if top <= y + 1e-4 && best.is_none_or(|b| top > b) {
                    best = Some(top);
                }
            }
        }
    }
    Ok(best)
}

/// Python's `str()` of a JSON number as json.loads gave it.
fn py_num(v: &J) -> String {
    match v.as_i64() {
        Some(i) if !v.is_f64() => i.to_string(),
        _ => crate::pyfloat::repr(v.as_f64().unwrap_or(0.0)),
    }
}

/// data/drips.rs: every drip of every shipped scene, where it lands, the art.
fn generate_drips(root: &Path, report: &J, source: &Source, gravity: f64) -> Result<String> {
    let table = scene_table(report);
    let regions = report["regions"].as_array().ok_or("no regions")?;
    let art: Vec<(usize, &J)> = regions
        .iter()
        .enumerate()
        .filter_map(|(slot, row)| {
            row.get("drip_art")
                .filter(|a| {
                    !a.is_null()
                        && *a != &J::Bool(false)
                        && a.as_object().is_some_and(|o| !o.is_empty())
                })
                .map(|a| (slot, a))
        })
        .collect();
    if art.is_empty() {
        return err("no view carries the water drip art: run host/regions.py first");
    }
    let first = art[0].1;
    if art
        .iter()
        .any(|(_, a)| a["clips"] != first["clips"] || a["rects"] != first["rects"])
    {
        return err("views disagree about the water drip art");
    }
    let mut scenes: Vec<String> = Vec::new();
    for (slot, _) in &art {
        let n = jstr(&regions[*slot], "scene_name");
        if !scenes.contains(&n) {
            scenes.push(n);
        }
    }
    scenes.sort_by_key(|n| table[n].1);
    let mut rows: Vec<(i64, i64, i64, i64, i64)> = Vec::new();
    let mut params: Vec<[u64; 4]> = Vec::new();
    for name in &scenes {
        let (file, id) = &table[name];
        let sc = Scene::new(source, file).map_err(|e| e.to_string())?;
        let drips = drip_sources(&sc)?;
        if drips.len() > 16 {
            return err(format!("{name} has {} water drips, past 16", drips.len()));
        }
        for d in drips {
            let w = &d.drip;
            let p = [
                f64_of(w, "idleTimeMin")?,
                f64_of(w, "idleTimeMax")?,
                f64_of(w, "fallVelocity")?,
                f64_of(w, "impactTranslation")?,
            ];
            let key = p.map(f64::to_bits);
            if !params.contains(&key) {
                params.push(key);
            }
            let (x, y) = (d.position[0], d.position[1]);
            let left = x + d.offset_x - d.half_width;
            let right = x + d.offset_x + d.half_width;
            let fall = match terrain_below(&sc, left, right, y + d.bottom)? {
                None => 65535,
                Some(ground) => {
                    let drop = (y + d.bottom) - ground;
                    let (v, g) = (-p[2], gravity);
                    let t = ((-v + (v * v + 2.0 * g * drop).sqrt()) / g * 60.0).ceil() as i64;
                    t.min(65534)
                }
            };
            let (px, py) = (py_round(x * 64.0), py_round(y * 64.0));
            if !(-32768..32768).contains(&px) || !(-32768..32768).contains(&py) {
                return err(format!("{name} water drip is outside the i16 table range"));
            }
            rows.push((*id, px, py, fall, py_round(d.x_scale * 4096.0)));
        }
    }
    if params.len() != 1 {
        return err("water drips carry different parameters");
    }
    // Counter.most_common(1): the highest count, the first inserted on ties.
    let mut by_scene: Vec<(i64, Vec<(i64, usize)>)> = Vec::new();
    for (slot, a) in &art {
        let sid = table[&jstr(&regions[*slot], "scene_name")].1;
        let fb = a["frame_base"].as_i64().unwrap_or(0);
        let idx = match by_scene.iter().position(|(s, _)| *s == sid) {
            Some(i) => i,
            None => {
                by_scene.push((sid, Vec::new()));
                by_scene.len() - 1
            }
        };
        let counts = &mut by_scene[idx].1;
        match counts.iter_mut().find(|(f, _)| *f == fb) {
            Some(c) => c.1 += 1,
            None => counts.push((fb, 1)),
        }
    }
    let common: HashMap<i64, i64> = by_scene
        .iter()
        .map(|(s, counts)| {
            let best = counts
                .iter()
                .fold(None::<(i64, usize)>, |acc, &(f, c)| match acc {
                    Some((_, bc)) if bc >= c => acc,
                    _ => Some((f, c)),
                });
            (*s, best.unwrap().0)
        })
        .collect();
    let mut scene_art: Vec<(i64, i64)> = common.iter().map(|(a, b)| (*a, *b)).collect();
    scene_art.sort();
    let view_art: Vec<(usize, i64)> = art
        .iter()
        .filter_map(|(slot, a)| {
            let fb = a["frame_base"].as_i64().unwrap_or(0);
            let sid = table[&jstr(&regions[*slot], "scene_name")].1;
            (fb != common[&sid]).then_some((*slot, fb))
        })
        .collect();
    let [idle_min, idle_max, velocity, impact] = params[0].map(f64::from_bits);
    let q = |v: f64| py_round(v * 65536.0);
    let clips = first["clips"].as_array().ok_or("drip art without clips")?;
    let lines = [
        "// Generated by host/props.py from the source WaterDrip objects.".to_string(),
        "/// (scene, x, y in 1/64 units, ticks the fall takes (65535: nothing below), x scale q12).".into(),
        format!("pub static DRIPS:&[(u8,i16,i16,u16,i16)]=&[{}];", rows.iter().map(|r| format!("({},{},{},{},{}),", r.0, r.1, r.2, r.3, r.4)).collect::<String>()),
        format!("pub const IDLE_MIN:u16={};", py_round(idle_min * 60.0)),
        format!("pub const IDLE_MAX:u16={};", py_round(idle_max * 60.0)),
        format!("pub const FALL_SPEED:i32={};", q(-velocity)),
        format!("pub const GRAVITY:i32={};", q(gravity)),
        format!("pub const IMPACT_DROP:i32={};", q(-impact)),
        "/// `Idle`, `Drip`, `Fall`, `Impact`: sprites (indices into SPRITE_RECT) and rate.".into(),
        format!(
            "pub const CLIP_FRAMES:[&[u8];4]=[{}];",
            clips.iter().map(|c| format!("&[{}],", c["frames"].as_array().map(|f| f.iter().map(py_num).collect::<Vec<_>>().join(",")).unwrap_or_default())).collect::<String>()
        ),
        format!("pub const CLIP_FPS:[u8;4]=[{}];", clips.iter().map(|c| format!("{},", c["fps"].as_f64().unwrap_or(0.0).trunc() as i64)).collect::<String>()),
        format!(
            "pub static SPRITE_RECT:&[[u8;4]]=&[{}];",
            first["rects"].as_array().map(|r| r.iter().map(|x| format!("[{}],", x.as_array().map(|v| v.iter().map(py_num).collect::<Vec<_>>().join(",")).unwrap_or_default())).collect::<String>()).unwrap_or_default()
        ),
        format!(
            "pub static SPRITE_BOX:&[[i32;4]]=&[{}];",
            first["boxes"].as_array().map(|r| r.iter().map(|x| format!("[{}],", x.as_array().map(|v| v.iter().map(|n| q(n.as_f64().unwrap_or(0.0)).to_string()).collect::<Vec<_>>().join(",")).unwrap_or_default())).collect::<String>()).unwrap_or_default()
        ),
        "/// The drip sheet's frame in a scene's views: (scene, the frame most of".into(),
        "/// its views carry it at), then (catalogue slot, frame) for the views that".into(),
        "/// differ (a bench or an NPC bank shifts a view's own frames). Both sorted.".into(),
        format!("pub static SCENE_ART:&[(u8,u16)]=&[{}];", scene_art.iter().map(|(s, f)| format!("({s},{f}),")).collect::<String>()),
        format!("pub static VIEW_ART:&[(u16,u16)]=&[{}];", view_art.iter().map(|(s, f)| format!("({s},{f}),")).collect::<String>()),
    ];
    std::fs::write(root.join("data/drips.rs"), lines.join("\n") + "\n")
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "{{'drips': {}, 'views': {}, 'no_ground': {}}}",
        rows.len(),
        art.len(),
        rows.iter().filter(|r| r.3 == 65535).count()
    ))
}
