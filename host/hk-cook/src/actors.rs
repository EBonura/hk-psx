//! Source HealthManager actors and the recognizers that admit their movement
//! (host/actors.py `actor_sources`), ported one recognizer at a time.
//!
//! `scan` returns, for every enabled and active HealthManager of a scene, the
//! control record the ported recognizers give it. The recognizers are tried in
//! the order of `actor_sources`; an actor none of them admits has no control
//! here (the Python gives it one of the recognizers not ported yet, or a
//! refusal). The parity gate (hk-cook-parity actors) compares the controls of
//! the kinds ported against the Python oracle's.

use crate::aspid;
use crate::baldur;
use crate::battle::{self, pool_peak, Member};
use crate::blocker;
use crate::climber;
use crate::colliders;
use crate::common::{component_records, err, get, path_id, Result};
use crate::false_knight;
use crate::gruzzer;
use crate::hatcher;
use crate::husk_guard;
use crate::mawlek;
use crate::pigeon;
use crate::pyjson::Json;
use crate::runner;
use crate::vengefly;
use crate::zombie_shield;
use hk_unity::scene::Scene;
use hk_unity::{Source, Value};

/// One actor: where it is, and the control record a ported recognizer admitted.
pub struct Row {
    pub source: String,
    pub game_object: i64,
    pub name: String,
    pub control: Option<(String, Json)>,
    /// `movement_supported`: a control that is not an additive-scene merge the controller refuses.
    pub supported: bool,
    /// The HealthManager's object id, scene-unique (`spec_source_id`).
    pub spec_source_id: i64,
    pub position: [f64; 3],
    pub health_manager: Value,
    /// `_colliders`, as the JSON dicts the cook reports hold.
    pub colliders: Json,
    /// `components`: object id to kind, in object order.
    pub components: Vec<(i64, String)>,
    /// The last component of each kind the cook reads off the actor (`tk2dSprite`,
    /// `tk2dSpriteAnimator`, `Recoil`, `DamageHero`, `Rigidbody2D`, `EnemyDreamnailReaction`).
    pub parts: Vec<(String, i64, Value)>,
    /// `fsm_ids`: the PlayMakerFSM components, as `file:path_id`.
    pub fsm_ids: Vec<String>,
    pub limitations: Vec<String>,
    /// 1 based arena wave the enemy is summoned in, or 0 (host/battle.py).
    pub battle_wave: u8,
    /// Killed by the arena's `Remove on battle start`.
    pub battle_removable: bool,
}

impl Row {
    /// `battle.actor_member`: (wave, removable) for the pool budget.
    pub fn member(&self) -> Member {
        (self.battle_wave, self.battle_removable)
    }
    /// `actor[kind]`: the last component of one of the recorded kinds.
    pub fn part(&self, kind: &str) -> Option<&Value> {
        self.parts.iter().find(|p| p.0 == kind).map(|p| &p.2)
    }
}

/// `actor['limitations']` as `actor_sources` leaves it for an admitted control.
fn control_limitations(kind: &str, control: &Json) -> Vec<String> {
    let listed = || -> Vec<String> {
        match control {
            Json::Obj(f) => match f.iter().find(|k| k.0 == "limitations") {
                Some((_, Json::List(l))) => l
                    .iter()
                    .filter_map(|s| {
                        if let Json::Str(s) = s {
                            Some(s.clone())
                        } else {
                            None
                        }
                    })
                    .collect(),
                _ => Vec::new(),
            },
            _ => Vec::new(),
        }
    };
    match kind {
        "WalkLeftRight" => vec![
            "WalkLeftRight subset: global GO LEFT/RIGHT events and first-crawler activation are not executed.".to_string(),
            "Initial random direction requires deterministic guest sampling; Unity RNG sequence is not reproduced.".to_string(),
        ],
        "ZombieSwipeWalker" => {
            let mut l: Vec<String> = listed().into_iter().filter(|s| !s.starts_with("Guest actor/clip/sensing/audio bindings")).collect();
            l.push("Charge Dust and source death effects are not presented; audio uses the resident Runner bank.".to_string());
            l
        }
        _ => listed(),
    }
}

/// `dict(control, guest_enabled=True)`: the key keeps its position.
fn guest_enabled(control: Json) -> Json {
    match control {
        Json::Obj(mut fields) => {
            if let Some(slot) = fields.iter_mut().find(|(k, _)| k == "guest_enabled") {
                slot.1 = Json::Bool(true);
            }
            Json::Obj(fields)
        }
        other => other,
    }
}

/// The catalogue: each scene's file, runtime bounds and name (quality.SCENE_TABLE).
pub type Catalogue = [(String, [f64; 4], String)];

pub fn scan(sc: &Scene, source: &Source, catalogue: &Catalogue) -> Result<Vec<Row>> {
    scan_with(sc, source, catalogue, true, None)
}

/// `actor_sources(sc, bounds)`: only the actors standing inside `bounds` (x0, y0, x1, y1).
pub fn scan_in(
    sc: &Scene,
    source: &Source,
    catalogue: &Catalogue,
    bounds: [f64; 4],
) -> Result<Vec<Row>> {
    scan_with(sc, source, catalogue, true, Some(bounds))
}

/// `hatcher._others`: the supported actors of the scene with the Hatcher family refused.
fn supported_others(sc: &Scene, source: &Source, catalogue: &Catalogue) -> Result<Vec<Member>> {
    let members: Vec<Member> = scan_with(sc, source, catalogue, false, None)?
        .iter()
        .filter(|r| r.supported)
        .map(Row::member)
        .collect();
    // The pool holds the arena's largest moment, not every placement of it.
    if pool_peak(&members) > 32 {
        return err("actor region exceeds bounded 32-slot guest pool");
    }
    Ok(members)
}

/// `actor_sources`; `family` is false while the Hatcher's own scene budget is measured.
fn scan_with(
    sc: &Scene,
    source: &Source,
    catalogue: &Catalogue,
    family: bool,
    bounds: Option<[f64; 4]>,
) -> Result<Vec<Row>> {
    let others_cache = std::cell::RefCell::new(None::<Vec<Member>>);
    let others = || -> Result<Vec<Member>> {
        if let Some(n) = others_cache.borrow().as_ref() {
            return Ok(n.clone());
        }
        let n = supported_others(sc, source, catalogue)?;
        *others_cache.borrow_mut() = Some(n.clone());
        Ok(n)
    };
    let mut rows = Vec::new();
    for o in &sc.objects {
        if o.typename != "HealthManager" || !get(&o.tree, "m_Enabled")?.truthy() {
            continue;
        }
        let gid = path_id(get(&o.tree, "m_GameObject")?).unwrap_or(0);
        if !sc.active(gid) {
            continue;
        }
        if let Some(b) = bounds {
            let p = u(sc.point(gid, 0.0, 0.0, 0.0))?;
            if !(b[0] <= p[0] && p[0] <= b[2] && b[1] <= p[1] && p[1] <= b[3]) {
                continue;
            }
        }
        let name = get(
            sc.go(gid).ok_or_else(|| "no such GameObject".to_string())?,
            "m_Name",
        )?
        .str()
        .unwrap_or_default();
        let mut control = None;
        let records = component_records(sc, gid);
        if control.is_none() {
            if let Ok(found) = walker_control(sc, source, gid) {
                control = Some(("WalkLeftRight".to_string(), found));
            }
        }
        if control.is_none()
            && name.starts_with("Roller")
            && records.iter().any(|r| r.1 == "LineOfSightDetector")
        {
            if let Ok(found) = baldur::recognize(sc, source, gid) {
                control = Some(("Baldur".to_string(), found));
            }
        }
        if control.is_none()
            && name == "Giant Fly"
            && records.iter().any(|r| r.1 == "PlayMakerCollisionStay2D")
        {
            if let Ok(found) = gruzzer::recognize_giant_fly(sc, gid, &o.tree) {
                control = Some(("GruzMother".to_string(), found));
            }
        }
        if control.is_none()
            && name.starts_with("Fly")
            && records.iter().any(|r| r.1 == "PlayMakerCollisionStay2D")
        {
            if let Ok(found) = gruzzer::recognize(sc, source, gid, catalogue) {
                let kind = match &found {
                    Json::Obj(f) => match f.iter().find(|k| k.0 == "kind") {
                        Some((_, Json::Str(k))) => k.clone(),
                        _ => "Gruzzer".to_string(),
                    },
                    _ => "Gruzzer".to_string(),
                };
                control = Some((kind, found));
            }
        }
        if control.is_none()
            && name.starts_with("Spitter")
            && records.iter().any(|r| r.1 == "PersonalObjectPool")
        {
            if let Ok(found) = aspid::recognize(sc, source, gid) {
                control = Some(("Aspid".to_string(), found));
            }
        }
        if family
            && control.is_none()
            && name.starts_with("Hatcher Baby")
            && records.iter().any(|r| r.1 == "ObjectBounce")
        {
            if let Ok(found) = hatcher::recognize_baby(sc, source, gid, catalogue, &others) {
                control = Some(("HatcherBaby".to_string(), found));
            }
        }
        if family
            && control.is_none()
            && name.starts_with("Hatcher")
            && !name.starts_with("Hatcher Baby")
            && records.iter().any(|r| r.1 == "LineOfSightDetector")
        {
            let position = u(sc.point(gid, 0.0, 0.0, 0.0))?;
            if let Ok(found) = hatcher::recognize(sc, source, gid, position, catalogue, &others) {
                control = Some(("Hatcher".to_string(), found));
            }
        }
        if control.is_none()
            && name.starts_with("False Knight")
            && records.iter().any(|r| r.1 == "EnemyHitEffectsArmoured")
        {
            if let Ok(found) = false_knight::recognize_placement(sc, source, gid, &o.tree) {
                control = Some(("FalseKnight".to_string(), found));
            }
        }
        if control.is_none() && records.iter().any(|r| r.1 == "Climber") {
            if let Ok(found) = climber::recognize(sc, source, gid) {
                control = Some(("Climber".to_string(), found));
            }
        }
        if control.is_none()
            && name.starts_with("Moss Walker")
            && records.iter().any(|r| r.1 == "NonBouncer")
        {
            if let Ok(found) = climber::recognize_moss_walker(sc, source, gid, &o.tree) {
                control = Some(("MossWalker".to_string(), found));
            }
        }
        if control.is_none()
            && name.starts_with("Buzzer")
            && records.iter().any(|r| r.1 == "LineOfSightDetector")
        {
            if let Ok(found) = vengefly::recognize(sc, source, gid) {
                control = Some(("Vengefly".to_string(), found));
            }
        }
        if control.is_none()
            && name.starts_with("Acid Flyer")
            && records.iter().any(|r| r.1 == "BigBouncer")
        {
            if let Ok(found) = vengefly::recognize_acid_flyer(sc, source, gid, &o.tree) {
                control = Some(("AcidFlyer".to_string(), found));
            }
        }
        if control.is_none()
            && name.starts_with("Mosquito")
            && records.iter().any(|r| r.1 == "LineOfSightDetector")
        {
            if let Ok(found) = vengefly::recognize_mosquito(sc, source, gid, &o.tree) {
                control = Some(("Mosquito".to_string(), found));
            }
        }
        if control.is_none()
            && name.starts_with("Egg Sac")
            && !records.iter().any(|r| r.1 == "PlayMakerFSM")
        {
            if let Ok(found) = egg_sac_control(sc, source, gid, &o.tree) {
                control = Some(("EggSac".to_string(), found));
            }
        }
        if control.is_none() && name == "Mawlek Body" && records.iter().any(|r| r.1 == "Walker") {
            if let Ok(found) = mawlek::recognize_placement(sc, gid, &o.tree) {
                control = Some(("Mawlek".to_string(), found));
            }
        }
        if control.is_none()
            && name.starts_with("Zombie Shield")
            && records.iter().any(|r| r.1 == "Walker")
        {
            let position = u(sc.point(gid, 0.0, 0.0, 0.0))?;
            if let Ok(found) = zombie_shield::recognize(sc, source, gid, position) {
                control = Some(("ZombieShield".to_string(), found));
            }
        }
        if control.is_none() && name.starts_with("Zombie Guard") {
            let position = u(sc.point(gid, 0.0, 0.0, 0.0))?;
            let file = hk_unity::base_name(&sc.base.name);
            let scene_name = catalogue.iter().find(|c| c.0 == file).map(|c| c.2.as_str());
            if let Ok(found) = husk_guard::recognize(sc, source, gid, position, scene_name) {
                control = Some(("HuskGuard".to_string(), found));
            }
        }
        if control.is_none()
            && name.starts_with("Blocker")
            && records.iter().any(|r| r.1 == "PersonalObjectPool")
        {
            let position = u(sc.point(gid, 0.0, 0.0, 0.0))?;
            if let Ok(found) = blocker::recognize(sc, source, gid, position) {
                control = Some(("Blocker".to_string(), found));
            }
        }
        if control.is_none()
            && name.starts_with("Pigeon")
            && records.iter().any(|r| r.1 == "EnemyDeathEffectsNoEffect")
        {
            let position = u(sc.point(gid, 0.0, 0.0, 0.0))?;
            if let Ok(found) = pigeon::recognize(sc, source, gid, position, &o.tree) {
                control = Some(("Pigeon".to_string(), found));
            }
        }
        // The Runner gate comes after the named gates in actor_sources; the
        // recognizers ported so far are disjoint from it by name and component.
        if control.is_none() {
            if let Some(found) = runner::candidate(sc, source, o)? {
                control = Some((
                    "ZombieSwipeWalker".to_string(),
                    guest_enabled(found.control),
                ));
            }
        }
        // Content merged in from an additive scene is admitted one controller at a time.
        let supported = control.as_ref().is_some_and(|c| o.id < 100000 || matches!(&c.1, Json::Obj(f) if f.iter().any(|k| k.0 == "admit_from_additive_scene" && matches!(k.1, Json::Bool(true)))));
        let limitations = match &control {
            Some((kind, c)) => control_limitations(kind, c),
            None => vec![
                "Enemy PlayMaker movement and state transitions remain unsupported.".to_string(),
            ],
        };
        let mut parts: Vec<(String, i64, Value)> = Vec::new();
        for &(id, kind, tree) in &records {
            if matches!(
                kind,
                "tk2dSprite"
                    | "tk2dSpriteAnimator"
                    | "Recoil"
                    | "DamageHero"
                    | "Rigidbody2D"
                    | "EnemyDreamnailReaction"
            ) {
                match parts.iter_mut().find(|p| p.0 == kind) {
                    Some(slot) => *slot = (kind.to_string(), id, tree.clone()),
                    None => parts.push((kind.to_string(), id, tree.clone())),
                }
            }
        }
        let mut fsm_ids = Vec::new();
        for r in get(sc.go(gid).ok_or("no such GameObject")?, "m_Component")?
            .list()
            .unwrap_or(&[])
        {
            let obj = u(sc.deref(get(r, "component")?))?;
            if obj.class_id() == 114 && u(source.typename(&obj))? == "PlayMakerFSM" {
                fsm_ids.push(obj.sid());
            }
        }
        let (battle_wave, battle_removable) = battle::membership(sc, gid, &records)?;
        rows.push(Row {
            source: sc.sid(o.id),
            game_object: gid,
            name,
            control,
            supported,
            spec_source_id: o.id,
            position: u(sc.point(gid, 0.0, 0.0, 0.0))?,
            health_manager: o.tree.clone(),
            colliders: colliders::colliders(sc, gid, &records)?,
            components: records.iter().map(|r| (r.0, r.1.to_string())).collect(),
            parts,
            fsm_ids,
            limitations,
            battle_wave,
            battle_removable,
        });
    }
    Ok(rows)
}

/// Value helper for the recognizers: a clone of a field, or the missing-field error.
pub fn field(v: &Value, key: &str) -> Result<Value> {
    match v.get(key) {
        Some(x) => Ok(x.clone()),
        None => err(format!("missing field {key}")),
    }
}

// --- walker_control and the Egg Sac (host/actors.py) -----------------------------

use crate::cook_audio::{jobj, js, u};
use crate::music::value_json;
use crate::recog::xy;
use crate::runner::ticks;

fn byte_vec(data: &Value) -> Result<Vec<u8>> {
    Ok(match get(data, "byteData")? {
        Value::Bytes(b) => b.clone(),
        other => other
            .list()
            .unwrap_or(&[])
            .iter()
            .map(|x| x.int().unwrap_or(0) as u8)
            .collect(),
    })
}

/// actors.py `action_fields`: the narrow typed action parameters WalkLeftRight uses.
fn narrow_action_fields(data: &Value, index: usize) -> Result<Vec<(String, Value)>> {
    let list = |k: &str| -> Result<Vec<Value>> { Ok(get(data, k)?.list().unwrap_or(&[]).to_vec()) };
    let starts = list("actionStartIndex")?;
    let names = list("paramName")?;
    let start = starts
        .get(index)
        .and_then(Value::int)
        .ok_or("action index out of range")? as usize;
    let end = starts
        .get(index + 1)
        .and_then(Value::int)
        .map_or(names.len(), |e| e as usize);
    let (kinds, positions, sizes, bytes) = (
        list("paramDataType")?,
        list("paramDataPos")?,
        list("paramByteDataSize")?,
        byte_vec(data)?,
    );
    let mut fields: Vec<(String, Value)> = Vec::new();
    for i in start..end {
        let name = names.get(i).and_then(Value::str).unwrap_or_default();
        let (kind, pos, size) = (
            kinds[i].int().unwrap_or(0),
            positions[i].int().unwrap_or(0) as usize,
            sizes[i].int().unwrap_or(0),
        );
        let value = if kind == 1 && size == 1 {
            Value::Bool(bytes.get(pos).copied().unwrap_or(0) != 0)
        } else if kind == 2 && size == 4 {
            Value::F32(f32::from_le_bytes(
                bytes
                    .get(pos..pos + 4)
                    .ok_or("short float parameter")?
                    .try_into()
                    .unwrap(),
            ))
        } else if matches!(kind, 15 | 17 | 18) {
            let key = match kind {
                15 => "fsmFloatParams",
                17 => "fsmBoolParams",
                _ => "fsmStringParams",
            };
            let item = list(key)?
                .get(pos)
                .cloned()
                .ok_or("parameter index out of range")?;
            if get(&item, "useVariable")?.truthy() {
                return err(format!("dynamic action parameter unsupported: {name}"));
            }
            get(&item, "value")?.clone()
        } else if kind == 20 {
            let owner = list("fsmOwnerDefaultParams")?
                .get(pos)
                .cloned()
                .ok_or("owner index out of range")?;
            if get(&owner, "ownerOption")?.int() != Some(0) {
                return err("external action owner unsupported");
            }
            Value::Str(b"owner".to_vec())
        } else {
            return err(format!("unsupported action parameter {name}/{kind}/{size}"));
        };
        match fields.iter_mut().find(|f| f.0 == name) {
            Some(slot) => slot.1 = value,
            None => fields.push((name, value)),
        }
    }
    Ok(fields)
}

/// The last component of a kind among the records (`actor[kind] = component`).
fn last_of<'a>(records: &[(i64, &str, &'a Value)], kind: &str) -> Option<&'a Value> {
    records.iter().rev().find(|r| r.1 == kind).map(|r| r.2)
}

/// `walker_control`: recognize the observed Crawler Walk state and original C# action.
pub fn walker_control(sc: &Scene, source: &Source, gid: i64) -> Result<Json> {
    let records = component_records(sc, gid);
    let layer = get(sc.go(gid).ok_or("no such GameObject")?, "m_Layer")?.int();
    if layer != Some(11) {
        return err(format!(
            "Crawler outside the enemy layer: layer {}",
            layer.unwrap_or(-1)
        ));
    }
    for (_, typ, tree) in &records {
        if matches!(*typ, "BigBouncer" | "BounceShroom")
            || (*typ == "NonBouncer" && get(tree, "active")?.truthy())
        {
            return err(format!("unsupported Crawler nail response variant: {typ}"));
        }
    }
    let mut matching: Vec<(String, &Value)> = Vec::new();
    for (cid, typ, data) in &records {
        if *typ == "PlayMakerFSM"
            && get(get(data, "fsm")?, "name")?.str().as_deref() == Some("Crawler")
        {
            matching.push((sc.sid(*cid), get(data, "fsm")?));
        }
    }
    if matching.len() != 1 {
        return err("no single supported Crawler FSM");
    }
    let (sid, fsm) = (matching[0].0.clone(), matching[0].1);
    if get(fsm, "startState")?.str().as_deref() != Some("Walk") {
        return err("Crawler begins in unsupported state");
    }
    let first: Vec<&Value> = get(get(fsm, "variables")?, "boolVariables")?
        .list()
        .unwrap_or(&[])
        .iter()
        .filter(|v| v.get("name").and_then(Value::str).as_deref() == Some("First Crawler"))
        .filter_map(|v| v.get("value"))
        .collect();
    if !(first.len() == 1 && first[0].int() == Some(0)) {
        return err("First Crawler wait/event behavior unsupported");
    }
    let state = get(fsm, "states")?
        .list()
        .unwrap_or(&[])
        .iter()
        .find(|s| s.get("name").and_then(Value::str).as_deref() == Some("Walk"))
        .ok_or("no Walk state")?;
    let data = get(state, "actionData")?;
    let names: Vec<String> = get(data, "actionNames")?
        .list()
        .unwrap_or(&[])
        .iter()
        .filter_map(Value::str)
        .collect();
    let enabled: Vec<Option<i64>> = get(data, "actionEnabled")?
        .list()
        .unwrap_or(&[])
        .iter()
        .map(Value::int)
        .collect();
    if names
        != [
            "HutongGames.PlayMaker.Actions.BoolTest",
            "HutongGames.PlayMaker.Actions.WalkLeftRight",
        ]
        || enabled != [Some(1), Some(1)]
    {
        return err("Crawler Walk action sequence changed");
    }
    let fields = narrow_action_fields(data, 1)?;
    let f = |k: &str| {
        fields
            .iter()
            .find(|x| x.0 == k)
            .map(|x| &x.1)
            .ok_or_else(|| format!("missing field {k}"))
    };
    let (speed, delay) = (
        f("walkSpeed")?.float().ok_or("walkSpeed")?,
        f("turnDelay")?.float().ok_or("turnDelay")?,
    );
    if f("groundLayer")?.str().as_deref() != Some("Terrain") || speed <= 0.0 || speed > 16.0 {
        return err("unsupported walker terrain/speed");
    }
    if !(0.0..=10.0).contains(&delay) {
        return err("walker cooldown exceeds tick bound");
    }
    let tid = *sc.go_transform.get(&gid).ok_or("actor has no transform")?;
    let m = u(sc.world(tid))?;
    if [(0, 1), (1, 0), (0, 2), (1, 2)]
        .iter()
        .any(|&(r, c)| m[r][c].abs() > 0.00001)
    {
        return err("rotated crawler movement unsupported");
    }
    if m[0][0].abs() < 0.001 || m[1][1] <= 0.0 {
        return err("unsupported crawler transform");
    }
    let animator = last_of(&records, "tk2dSpriteAnimator").ok_or("no tk2dSpriteAnimator")?;
    let library_object = u(sc.deref(get(animator, "library")?))?;
    let library = u(source.read(&library_object))?;
    let clips = get(&library, "clips")?.list().unwrap_or(&[]);
    let clip = |name: &str| {
        clips
            .iter()
            .rev()
            .find(|c| c.get("name").and_then(Value::str).as_deref() == Some(name))
            .ok_or_else(|| format!("no clip {name}"))
    };
    let (walk_name, turn_name) = (
        str_of_value(f("walkAnimName")?)?,
        str_of_value(f("turnAnimName")?)?,
    );
    let (walk, turn) = (clip(&walk_name)?, clip(&turn_name)?);
    let (turn_frames, turn_fps) = (
        get(turn, "frames")?.list().map_or(0, <[Value]>::len),
        get(turn, "fps")?.float().unwrap_or(0.0),
    );
    let (walk_frames, walk_fps) = (
        get(walk, "frames")?.list().map_or(0, <[Value]>::len),
        get(walk, "fps")?.float().unwrap_or(0.0),
    );
    if turn_frames == 0 || turn_fps <= 0.0 || walk_frames == 0 || walk_fps <= 0.0 {
        return err("invalid crawler animation duration");
    }
    let direction = (if m[0][0] > 0.0 { 1 } else { -1 })
        * (if f("spriteFacesLeft")?.truthy() {
            -1
        } else {
            1
        });
    Ok(jobj(vec![
        ("kind", js("WalkLeftRight")),
        ("fsm", Json::Str(sid)),
        ("action_state", js("Walk")),
        (
            "source_action_fields",
            Json::Obj(
                fields
                    .iter()
                    .map(|(k, v)| (k.clone(), value_json(v)))
                    .collect(),
            ),
        ),
        ("speed", Json::Float(speed)),
        ("turn_cooldown_ticks", Json::Int(ticks(delay))),
        (
            "turn_ticks",
            Json::Int(ticks(turn_frames as f64 / turn_fps)),
        ),
        ("initial_direction", Json::Int(direction)),
        (
            "random_start_direction",
            Json::Bool(
                !(f("startLeft")?.truthy()
                    || f("startRight")?.truthy()
                    || f("keepDirection")?.truthy()),
            ),
        ),
        ("library_source", Json::Str(library_object.sid())),
        ("walk_clip_name", Json::Str(walk_name)),
        ("turn_clip_name", Json::Str(turn_name)),
        (
            "ray_parameters",
            jobj(vec![
                ("ahead_margin", Json::Float(0.1)),
                ("height_above_bottom", Json::Float(0.5)),
                ("down_length", Json::Float(1.0)),
            ]),
        ),
        (
            "source_methods",
            Json::List(
                [
                    "WalkLeftRight.SetupStartingDirection",
                    "WalkLeftRight.Walk coroutine",
                    "WalkLeftRight.Turn coroutine",
                    "WalkLeftRight.CheckWall",
                    "WalkLeftRight.CheckFloor",
                    "WalkLeftRight.CheckIsGrounded",
                ]
                .iter()
                .map(|s| js(s))
                .collect(),
            ),
        ),
    ]))
}

fn str_of_value(v: &Value) -> Result<String> {
    v.str().ok_or_else(|| "not a string".to_string())
}

const EGG_SAC_COMPONENTS: [&str; 15] = [
    "AudioSource",
    "BoxCollider2D",
    "EnemyDeathEffects",
    "EnemyDreamnailReaction",
    "ExtraDamageable",
    "HealthManager",
    "InfectedEnemyEffects",
    "MeshFilter",
    "MeshRenderer",
    "PersistentBoolItem",
    "SetZ",
    "SpriteFlash",
    "Transform",
    "tk2dSprite",
    "tk2dSpriteAnimator",
];
const EGG_SAC_BODY_SIZE: [f64; 2] = [1.7711232900619507, 1.8317594528198242];
const EGG_SAC_BODY_OFFSET: [f64; 2] = [-0.04640769958496094, -0.37363290786743164];
/// name: (frames, fps, wrapMode, loopStart)
const EGG_SAC_CLIPS: [(&str, usize, f64, i64, i64); 3] = [
    ("Idle", 4, 12.0, 0, 0),
    ("Death", 4, 12.0, 1, 1),
    ("Burst", 4, 18.0, 2, 0),
];

/// `egg_sac_control`: the Egg Sac, a destructible with no FSM and no rigid body.
pub fn egg_sac_control(sc: &Scene, source: &Source, gid: i64, health: &Value) -> Result<Json> {
    let records = component_records(sc, gid);
    if get(sc.go(gid).ok_or("no such GameObject")?, "m_Layer")?.int() != Some(11) {
        return err("Egg Sac outside the enemy layer");
    }
    let mut kinds: Vec<&str> = records.iter().map(|r| r.1).collect();
    kinds.sort();
    let mut want = EGG_SAC_COMPONENTS.to_vec();
    want.sort();
    if kinds != want {
        return err("unsupported Egg Sac component set");
    }
    let tid = *sc.go_transform.get(&gid).ok_or("actor has no transform")?;
    let m = u(sc.world(tid))?;
    if (0..2).any(|r| (0..2).any(|c| (m[r][c] - if r == c { 1.0 } else { 0.0 }).abs() > 1e-6)) {
        return err("unsupported Egg Sac rotation or scale");
    }
    let body = records
        .iter()
        .find(|r| r.1 == "BoxCollider2D")
        .map(|r| r.2)
        .ok_or("no body")?;
    let (size, offset) = (xy(body, "m_Size")?, xy(body, "m_Offset")?);
    if !get(body, "m_Enabled")?.truthy()
        || get(body, "m_IsTrigger")?.truthy()
        || get(body, "m_EdgeRadius")?.float() != Some(0.0)
        || (0..2).any(|k| {
            (size[k] - EGG_SAC_BODY_SIZE[k]).abs() > 1e-6
                || (offset[k] - EGG_SAC_BODY_OFFSET[k]).abs() > 1e-6
        })
    {
        return err("unsupported Egg Sac body collider");
    }
    if get(health, "hp")?.int().unwrap_or(0) <= 0
        || [
            "invincible",
            "invincibleFromDirection",
            "hasSpecialDeath",
            "hasAlternateHitAnimation",
            "damageOverride",
            "megaFlingGeo",
        ]
        .iter()
        .any(|k| health.get(k).is_some_and(Value::truthy))
    {
        return err("unsupported Egg Sac HealthManager variant");
    }
    let sprite = last_of(&records, "tk2dSprite").ok_or("no sprite")?;
    let one4 = Value::Map(
        ["r", "g", "b", "a"]
            .iter()
            .map(|k| ((*k).into(), Value::F64(1.0)))
            .collect(),
    );
    let one3 = Value::Map(
        ["x", "y", "z"]
            .iter()
            .map(|k| ((*k).into(), Value::F64(1.0)))
            .collect(),
    );
    if !get(sprite, "_color")?.py_eq(&one4) || !get(sprite, "_scale")?.py_eq(&one3) {
        return err("unsupported Egg Sac sprite scale/color");
    }
    let animator = last_of(&records, "tk2dSpriteAnimator").ok_or("no animator")?;
    if !get(animator, "m_Enabled")?.truthy()
        || !get(animator, "playAutomatically")?.truthy()
        || get(animator, "isRealtime")?.truthy()
    {
        return err("unsupported Egg Sac animator startup");
    }
    let library_object = u(sc.deref(get(animator, "library")?))?;
    let library = u(source.read(&library_object))?;
    let all = get(&library, "clips")?.list().unwrap_or(&[]);
    let default = get(animator, "defaultClipId")?.int().unwrap_or(-1);
    if all
        .get(default as usize)
        .and_then(|c| c.get("name"))
        .and_then(Value::str)
        .as_deref()
        != Some("Idle")
    {
        return err("Egg Sac does not start on Idle");
    }
    let by_name = crate::recog::clips_by_name(&library)?;
    for (name, frames, fps, wrap, loop_start) in EGG_SAC_CLIPS {
        let clip = by_name.iter().find(|(k, _)| k == name).map(|(_, v)| *v);
        let ok = clip.is_some_and(|c| {
            c.get("frames").and_then(Value::list).map(<[Value]>::len) == Some(frames)
                && c.get("fps").and_then(Value::float) == Some(fps)
                && c.get("wrapMode").and_then(Value::int) == Some(wrap)
                && c.get("loopStart").map_or(0, |l| l.int().unwrap_or(-1)) == loop_start
        });
        if !ok {
            return err(format!("unsupported Egg Sac animation: {name}"));
        }
        if clip
            .unwrap()
            .get("frames")
            .and_then(Value::list)
            .unwrap_or(&[])
            .iter()
            .any(|f| f.get("triggerEvent").is_some_and(Value::truthy))
        {
            return err(format!("unsupported Egg Sac animation events: {name}"));
        }
    }
    Ok(jobj(vec![
        ("kind", js("EggSac")),
        ("guest_enabled", Json::Bool(true)),
        ("idle_clip_name", js("Idle")),
        ("library_source", Json::Str(library_object.sid())),
        (
            "limitations",
            Json::List(
                [
                    "No FSM, rigid body, Recoil or DamageHero on the source object: the guest loops Idle at the authored transform and answers only the nail.",
                    "PersistentBoolItem is recorded, not honoured: a killed Egg Sac returns on a scene reload.",
                    "The looping idle AudioSource, the SetZ depth and the death particles are not presented.",
                ]
                .iter()
                .map(|s| js(s))
                .collect(),
            ),
        ),
    ]))
}
