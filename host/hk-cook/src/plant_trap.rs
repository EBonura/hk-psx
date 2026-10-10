//! Recognize the Greenpath Plant Trap (Snapper Trap) for guest admission. Ported from
//! host/plant_trap.py.
//!
//! The controller lives in shared/hk-sim/src/plant_trap.rs; this module admits only a
//! placement whose serialized shape matches the one that was read to write it.
//!
//! The object has no collider of its own. tk2d builds a `BoxCollider2D` from the sprite
//! definition of the frame showing (`colliderType` 2 is a box centred on `colliderVertices[0]`
//! with half extents `colliderVertices[1]`, and 1 disables it), so the jaws hurt and can be hurt
//! only on the `Snap` frames and the first three `Retract` frames. This module reads those boxes
//! off the sprite definitions and proves them against the constants `plant_trap.rs` holds,
//! together with the `Detector` child's trigger box.

use crate::common::{component_records, err, get, py_round, Result};
use crate::cook_audio::{jobj, js, u};
use crate::fsm_pins::{check_action, check_transitions, state, Pin, PinRow};
use crate::pyjson::Json;
use crate::recog::{
    check_assemblies, child_map, eq_field, flag, identity_basis, only, plain_sprite,
};
use crate::runner::{fsm_fingerprint, ASSEMBLIES};
use hk_unity::scene::Scene;
use hk_unity::{Source, Value};

/// The cooked clips beyond walk and turn, in `plant_trap::Clip::slot` order.
pub(crate) const CLIP_SLOTS: [&str; 3] = ["ready", "snap", "retract"];
const CONTROL_FSM: &str = "Plant Trap Control";
const DAMAGES_FSM: &str = "damages_enemy";
const CONTROL_SHA256: &str = "fb0b5b0a8df9c655fe3407d7f84dfdf0b2025e92979c5ab561152783fd9b2b32";
const DAMAGES_SHA256: &str = "1f384ad187e2b1e643be82fdc7d486e3d28bf1221d6def2c6e17642b06b84629";
const DETECT_SHA256: &str = "830731ad9c6eb938fd0102ea2405832553d5e3753bec08a2e07aed16d1492736";
const COMPONENTS: [&str; 17] = [
    "AudioSource",
    "DamageHero",
    "EnemyDeathEffects",
    "EnemyDreamnailReaction",
    "ExtraDamageable",
    "HealthManager",
    "InfectedEnemyEffects",
    "MeshFilter",
    "MeshRenderer",
    "PersistentBoolItem",
    "PlayMakerCollisionEnter2D",
    "PlayMakerFSM",
    "PlayMakerFSM",
    "SpriteFlash",
    "Transform",
    "tk2dSprite",
    "tk2dSpriteAnimator",
];
/// name: (frames, fps, wrapMode, loopStart)
const CLIPS: [(&str, usize, f64, i64, i64); 5] = [
    ("Idle", 1, 12.0, 2, 0),
    ("Snap Ready", 6, 12.0, 1, 1),
    ("Snap", 3, 12.0, 2, 0),
    ("Retract", 7, 12.0, 2, 0),
    ("Death", 7, 12.0, 2, 0),
];
/// The frames that define a collider, as [x0, y0, x1, y1] Q16 (plant_trap.rs):
/// clip -> frame -> box. Every other frame of the clips above defines none.
const SNAP_BOXES: [[i64; 4]; 3] = [
    [-129024, -167936, 134144, -54272],
    [-56320, -167936, 41984, 89088],
    [-56320, -167936, 49152, 36864],
];
const RETRACT_BOXES: [[i64; 4]; 3] = [
    [-56320, -167936, 49152, 36864],
    [-34816, -167936, 33792, 36864],
    [-34816, -167936, 30720, -1024],
];
const DETECT_Q16: [i64; 4] = [-72704, -167936, 89088, -85283];

fn wanted(clip: &str, index: usize) -> Option<[i64; 4]> {
    match clip {
        "Snap" => SNAP_BOXES.get(index).copied(),
        "Retract" => RETRACT_BOXES.get(index).copied(),
        _ => None,
    }
}

use Pin::{F, S};
/// `Plant Trap Control`'s audited waits, in seconds (plant_trap.rs holds them in ticks).
#[rustfmt::skip]
const ACTIONS: &[PinRow] = &[
    ("Ready", "Wait", &[("time", F(0.75))]),
    ("Ready", "Tk2dPlayAnimation", &[("clipName", S("Snap Ready"))]),
    ("Snap", "Wait", &[("time", F(1.0))]),
    ("Snap", "Tk2dPlayAnimation", &[("clipName", S("Snap"))]),
    ("Retract", "Tk2dPlayAnimationWithEvents", &[("clipName", S("Retract")), ("animationTriggerEvent", S("")), ("animationCompleteEvent", S("FINISHED"))]),
    ("Cooldown", "Wait", &[("time", F(0.5))]),
];
#[rustfmt::skip]
const TRANSITIONS: &[(&str, &[(&str, &str)])] = &[
    ("Idle", &[("DETECT", "Ready")]),
    ("Ready", &[("FINISHED", "Snap")]),
    ("Init", &[("FINISHED", "Idle")]),
    ("Snap", &[("WAIT", "Retract")]),
    ("Retract", &[("FINISHED", "Cooldown")]),
    ("Cooldown", &[("FINISHED", "Init")]),
];

fn q16(v: f64) -> i64 {
    py_round(v * 65536.0)
}

fn fsms(records: &[(i64, &str, &Value)]) -> Result<()> {
    let found: Vec<&Value> = records
        .iter()
        .filter(|r| r.1 == "PlayMakerFSM")
        .map(|r| get(r.2, "fsm"))
        .collect::<Result<_>>()?;
    let mut by_name: Vec<(String, &Value)> = Vec::new();
    for fsm in &found {
        let name = get(fsm, "name")?.str().unwrap_or_default();
        match by_name.iter_mut().find(|k| k.0 == name) {
            Some(slot) => slot.1 = fsm,
            None => by_name.push((name, fsm)),
        }
    }
    let mut names: Vec<&str> = by_name.iter().map(|k| k.0.as_str()).collect();
    names.sort();
    if names != [CONTROL_FSM, DAMAGES_FSM] || found.len() != 2 {
        return err("no single Plant Trap Control and damages_enemy FSM pair");
    }
    let find = |n: &str| by_name.iter().find(|k| k.0 == n).map(|k| k.1).unwrap();
    let (control, damages) = (find(CONTROL_FSM), find(DAMAGES_FSM));
    if get(control, "startState")?.str().as_deref() != Some("Init")
        || fsm_fingerprint(control)? != CONTROL_SHA256
        || fsm_fingerprint(damages)? != DAMAGES_SHA256
    {
        return err("unverified Plant Trap FSM variant");
    }
    check_transitions("Plant Trap", control, TRANSITIONS, &[])?;
    for &(st, action, expected) in ACTIONS {
        check_action("Plant Trap", state(control, st)?, action, expected)?;
    }
    Ok(())
}

/// `_detector`: the `Detector` child, a trigger box on the Knight-only layer whose FSM tells the trap.
fn detector(sc: &Scene, gid: i64, origin: [f64; 2]) -> Result<()> {
    let tid = *sc.go_transform.get(&gid).ok_or("actor has no transform")?;
    let children = child_map(sc, tid)?;
    let mut names: Vec<&str> = children.iter().map(|c| c.0.as_str()).collect();
    names.sort();
    if names != ["Detector", "Ready Grass"] {
        return err(format!(
            "unsupported Plant Trap children: {}",
            names.join(", ")
        ));
    }
    let (kid, ktid) = children.iter().find(|c| c.0 == "Detector").unwrap().1;
    let records = component_records(sc, kid);
    let boxes: Vec<&Value> = records
        .iter()
        .filter(|r| r.1 == "BoxCollider2D")
        .map(|r| r.2)
        .collect();
    let fsms: Vec<&Value> = records
        .iter()
        .filter(|r| r.1 == "PlayMakerFSM")
        .map(|r| get(r.2, "fsm"))
        .collect::<Result<_>>()?;
    if !sc.active(kid)
        || get(sc.go(kid).ok_or("no such GameObject")?, "m_Layer")?.int() != Some(13)
        || boxes.len() != 1
        || fsms.len() != 1
        || !flag(boxes[0], "m_Enabled")?
        || !flag(boxes[0], "m_IsTrigger")?
        || !eq_field(boxes[0], "m_EdgeRadius", &Value::Int(0))?
        || fsm_fingerprint(fsms[0])? != DETECT_SHA256
    {
        return err("unsupported Plant Trap Detector");
    }
    let matrix = u(sc.world(ktid))?;
    if !identity_basis(&matrix) {
        return err("rotated or scaled Plant Trap Detector");
    }
    let bx = boxes[0];
    let (size, offset) = (
        crate::recog::xy(bx, "m_Size")?,
        crate::recog::xy(bx, "m_Offset")?,
    );
    let (x, y) = (matrix[0][3] - origin[0], matrix[1][3] - origin[1]);
    let actual = [
        q16(x + offset[0] - size[0] / 2.0),
        q16(y + offset[1] - size[1] / 2.0),
        q16(x + offset[0] + size[0] / 2.0),
        q16(y + offset[1] + size[1] / 2.0),
    ];
    if actual != DETECT_Q16 {
        return err(format!(
            "Plant Trap Detector box {actual:?} is not the admitted {DETECT_Q16:?}"
        ));
    }
    Ok(())
}

/// The `[x0, y0, x1, y1]` Q16 collider tk2d builds for a sprite definition: its centre and half extents.
pub(crate) fn definition_box(definition: &Value) -> Result<[i64; 4]> {
    let v = get(definition, "colliderVertices")?.list().unwrap_or(&[]);
    let (c, h) = (
        v.first().ok_or("collider vertices")?,
        v.get(1).ok_or("collider vertices")?,
    );
    let f = |p: &Value, k: &str| get(p, k)?.float().ok_or_else(|| "not a number".to_string());
    let (cx, cy, hx, hy) = (f(c, "x")?, f(c, "y")?, f(h, "x")?, f(h, "y")?);
    Ok([q16(cx - hx), q16(cy - hy), q16(cx + hx), q16(cy + hy)])
}

/// `_clips`: the library's clip contract and the collider boxes of its frames.
fn clips(sc: &Scene, source: &Source, animator: &Value) -> Result<hk_unity::Obj> {
    if !flag(animator, "m_Enabled")?
        || flag(animator, "isRealtime")?
        || flag(animator, "playAutomatically")?
    {
        return err("Plant Trap requires enabled scaled-time animation it starts itself");
    }
    let library_o = u(sc.deref(get(animator, "library")?))?;
    let library = u(source.read(&library_o))?;
    let by_name = crate::recog::clips_by_name(&library)?;
    let mut collections: Vec<(i64, Value)> = Vec::new();
    for (name, frames, fps, wrap, loop_start) in CLIPS {
        let clip = by_name.iter().find(|(k, _)| k == name).map(|(_, v)| *v);
        let ok = crate::recog::clip_is(clip, frames, fps, wrap)
            && clip
                .and_then(|c| c.get("loopStart"))
                .map_or(0, |v| v.int().unwrap_or(-1))
                == loop_start
            && !get(clip.unwrap(), "frames")?
                .list()
                .unwrap_or(&[])
                .iter()
                .any(|f| f.get("triggerEvent").is_some_and(Value::truthy));
        if !ok {
            return err(format!("unsupported Plant Trap animation: {name}"));
        }
        for (index, frame) in get(clip.unwrap(), "frames")?
            .list()
            .unwrap_or(&[])
            .iter()
            .enumerate()
        {
            let collection_o = u(source.deref(&library_o.file, get(frame, "spriteCollection")?))?;
            if !collections.iter().any(|c| c.0 == collection_o.path_id()) {
                collections.push((collection_o.path_id(), u(source.read(&collection_o))?));
            }
            let collection = &collections
                .iter()
                .find(|c| c.0 == collection_o.path_id())
                .unwrap()
                .1;
            let definition = get(collection, "spriteDefinitions")?
                .list()
                .unwrap_or(&[])
                .get(get(frame, "spriteId")?.int().unwrap_or(-1) as usize)
                .ok_or("sprite definition")?;
            let want = wanted(name, index);
            if !eq_field(definition, "physicsEngine", &Value::Int(1))? {
                return err(format!(
                    "Plant Trap sprite is not a 2D physics sprite: {name}"
                ));
            }
            let collider_type = get(definition, "colliderType")?;
            let Some(want) = want else {
                if collider_type.py_eq(&Value::Int(2)) && (name == "Snap" || name == "Retract") {
                    return err(format!(
                        "unexpected Plant Trap collider: {name} frame {index}"
                    ));
                }
                if (name == "Snap Ready" || name == "Idle") && collider_type.py_eq(&Value::Int(2)) {
                    return err(format!(
                        "unexpected Plant Trap collider: {name} frame {index}"
                    ));
                }
                continue;
            };
            let actual = definition_box(definition)?;
            if !collider_type.py_eq(&Value::Int(2)) || actual != want {
                return err(format!(
                    "Plant Trap collider {name} frame {index} {actual:?} is not the admitted {want:?}"
                ));
            }
        }
    }
    Ok(library_o)
}

/// A rooted trap that snaps shut when the Knight stands over it.
pub fn recognize(
    sc: &Scene,
    source: &Source,
    gid: i64,
    position: [f64; 3],
    health: &Value,
) -> Result<Json> {
    check_assemblies(source, "Plant Trap")?;
    let records = component_records(sc, gid);
    fsms(&records)?;
    let mut kinds: Vec<&str> = records.iter().map(|r| r.1).collect();
    kinds.sort();
    if kinds != COMPONENTS {
        return err("unsupported Plant Trap component set");
    }
    let layer = get(sc.go(gid).ok_or("no such GameObject")?, "m_Layer")?
        .int()
        .unwrap_or(-1);
    if layer != 11 {
        return err(format!("Plant Trap outside the enemy layer: layer {layer}"));
    }
    let matrix = u(sc.world(*sc.go_transform.get(&gid).ok_or("actor has no transform")?))?;
    if !identity_basis(&matrix) {
        return err("unsupported Plant Trap rotation or scale");
    }
    if position[2].abs() > 0.01 {
        return err("Plant Trap depth differs from guest source plane");
    }
    detector(sc, gid, [position[0], position[1]])?;
    let mut bad_health = !eq_field(health, "hp", &Value::Int(16))?
        || !eq_field(health, "smallGeoDrops", &Value::Int(9))?;
    for key in [
        "invincible",
        "invincibleFromDirection",
        "hasSpecialDeath",
        "hasAlternateHitAnimation",
        "damageOverride",
        "megaFlingGeo",
        "mediumGeoDrops",
        "largeGeoDrops",
    ] {
        bad_health |= flag(health, key)?;
    }
    if bad_health {
        return err("unsupported Plant Trap HealthManager variant");
    }
    let (_, damage) = only(&records, "DamageHero", "Plant Trap")?;
    if !eq_field(damage, "damageDealt", &Value::Int(1))?
        || !eq_field(damage, "hazardType", &Value::Int(1))?
        || !flag(damage, "m_Enabled")?
    {
        return err("unsupported Plant Trap contact damage");
    }
    // `actor[kind]` is the last component of the kind.
    let last = |kind: &str| -> Result<&Value> {
        records
            .iter()
            .rev()
            .find(|r| r.1 == kind)
            .map(|r| r.2)
            .ok_or_else(|| format!("missing {kind}"))
    };
    if !plain_sprite(last("tk2dSprite")?)? {
        return err("unsupported Plant Trap sprite");
    }
    let library_o = clips(sc, source, last("tk2dSpriteAnimator")?)?;
    // The spec needs some box for its near test and the spawn: the open jaws' first frame.
    Ok(jobj(vec![
        ("kind", js("PlantTrap")),
        ("guest_enabled", Json::Bool(true)),
        ("bounds_q16", Json::List(SNAP_BOXES[0].iter().map(|&v| Json::Int(v)).collect())),
        (
            "fsm_sha256",
            jobj(vec![(CONTROL_FSM, js(CONTROL_SHA256)), (DAMAGES_FSM, js(DAMAGES_SHA256))]),
        ),
        ("assemblies_sha256", Json::Obj(ASSEMBLIES.iter().map(|(k, h)| (k.to_string(), js(h))).collect())),
        ("library_source", Json::Str(library_o.sid())),
        (
            "art_bindings",
            jobj(vec![
                ("walk", js("Snap Ready")),
                ("turn", js("Snap Ready")),
                ("ready", js("Snap Ready")),
                ("snap", js("Snap")),
                ("retract", js("Retract")),
            ]),
        ),
        (
            "limitations",
            Json::List(
                [
                    "The hurt box is the collider tk2d builds for the frame showing, read from the sprite definitions; contact is box overlap, as for every other actor.",
                    "`damages_enemy` never meets another enemy; the Ready Grass puff, the snap sound and the death puffs are not presented.",
                    "At rest it shows the last Retract frame, the sprite it is authored with and returns to.",
                ]
                .iter()
                .map(|s| js(s))
                .collect(),
            ),
        ),
    ]))
}
