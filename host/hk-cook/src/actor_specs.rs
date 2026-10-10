//! The `ActorSpec` expressions and placement words the guest links, per supported
//! actor. Ported from host/actors.py (`generated_actor_records`,
//! `scene_actor_bank`, `MAX_SCENE_ACTORS`) and host/effects.py `generated_corpse`.
//!
//! The split keeps the linked table a catalogue of types: every value the source
//! authors on the placement rather than on the prefab leaves the spec and rides in
//! the scene's metadata bank. The cooked clip indices and the validated corpse come
//! from the clip cook, so they are inputs here (`SpecActor`), exactly as the Python
//! actor dict carried them after the clips were cooked.

use crate::actors::Row;
use crate::common::{err, get, py_round, Result};
use crate::pyfloat;
use crate::pyjson::Json;
use crate::runner::ticks;
use hk_unity::Value;

/// `MAX_SCENE_ACTORS`: how many placements of a scene the guest pool can seat.
pub const MAX_SCENE_ACTORS: usize = 32;
/// `combat.HIT_EVASION_SECONDS`.
const HIT_EVASION_SECONDS: f64 = 0.2;

/// One actor as the spec generator sees it after the clip cook.
pub struct SpecActor<'a> {
    pub row: &'a Row,
    /// `actor['<slot>_clip']` bindings the clip cook attached.
    pub clips: &'a [(String, i64)],
    /// `actor['corpse']`: the validated corpse record, if any.
    pub corpse: Option<&'a Json>,
}

impl SpecActor<'_> {
    fn clip(&self, key: &str) -> Option<i64> {
        self.clips.iter().find(|c| c.0 == key).map(|c| c.1)
    }
    fn has_corpse(&self) -> bool {
        self.corpse.is_some_and(truthy)
    }
}

/// Per supported actor `source`: its index into the spec list and its placement.
pub type PlacedActors = Vec<(String, (usize, Placement))>;

/// The words that locate and orient one placement in the scene's metadata bank.
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    pub source_id: i64,
    pub x: i64,
    pub y: i64,
    pub initial_direction: i64,
    pub random_start_direction: bool,
    pub start_alert: bool,
    pub start_right: bool,
    pub rotation_quarter: i64,
    /// The enemy waits, FSMs disabled, until the camera's ActiveRegion meets its collider.
    pub fsm_activator: bool,
}

impl Placement {
    /// The placement dict, key order as Python built it.
    pub fn to_json(&self) -> Json {
        Json::Obj(vec![
            ("source_id".into(), Json::Int(self.source_id)),
            ("x".into(), Json::Int(self.x)),
            ("y".into(), Json::Int(self.y)),
            (
                "initial_direction".into(),
                Json::Int(self.initial_direction),
            ),
            (
                "random_start_direction".into(),
                Json::Bool(self.random_start_direction),
            ),
            ("start_alert".into(), Json::Bool(self.start_alert)),
            ("start_right".into(), Json::Bool(self.start_right)),
            ("rotation_quarter".into(), Json::Int(self.rotation_quarter)),
            ("fsm_activator".into(), Json::Bool(self.fsm_activator)),
        ])
    }
}

/// A view of the scene: the actors of one region, in cook order.
pub struct Region<'a> {
    pub chunk_id: i64,
    pub actors: Vec<SpecActor<'a>>,
}

fn truthy(j: &Json) -> bool {
    match j {
        Json::Null => false,
        Json::Bool(b) => *b,
        Json::Int(i) => *i != 0,
        Json::Float(f) => *f != 0.0,
        Json::Str(s) => !s.is_empty(),
        Json::List(l) => !l.is_empty(),
        Json::Obj(o) => !o.is_empty(),
    }
}

/// Python `str()` of a JSON scalar, as an f-string interpolates it.
fn py(j: &Json) -> String {
    match j {
        Json::Null => "None".into(),
        Json::Bool(true) => "True".into(),
        Json::Bool(false) => "False".into(),
        Json::Int(i) => i.to_string(),
        Json::Float(f) => pyfloat::repr(*f),
        Json::Str(s) => s.clone(),
        other => crate::pyjson::dumps(other),
    }
}

fn cj<'a>(c: &'a Json, key: &str) -> Option<&'a Json> {
    match c {
        Json::Obj(f) => f.iter().find(|k| k.0 == key).map(|k| &k.1),
        _ => None,
    }
}

fn need<'a>(c: &'a Json, key: &str) -> Result<&'a Json> {
    cj(c, key).ok_or_else(|| format!("control lacks {key}"))
}

fn num(j: &Json) -> Result<f64> {
    match j {
        Json::Int(i) => Ok(*i as f64),
        Json::Float(f) => Ok(*f),
        Json::Bool(b) => Ok(*b as i64 as f64),
        _ => err("expected a number"),
    }
}

fn int(c: &Json, key: &str) -> Result<i64> {
    match need(c, key)? {
        Json::Int(i) => Ok(*i),
        Json::Float(f) if f.fract() == 0.0 => Ok(*f as i64),
        _ => err(format!("{key} is not an int")),
    }
}

fn list<'a>(c: &'a Json, key: &str) -> Result<&'a [Json]> {
    match need(c, key)? {
        Json::List(l) => Ok(l),
        _ => err(format!("{key} is not a list")),
    }
}

/// `'[' + ','.join(map(str, values)) + ']'`.
fn bracket(values: &[Json]) -> String {
    format!("[{}]", values.iter().map(py).collect::<Vec<_>>().join(","))
}

fn lower(b: bool) -> &'static str {
    if b {
        "true"
    } else {
        "false"
    }
}

/// `effects.generated_corpse`.
pub fn generated_corpse(record: Option<&Json>) -> Result<String> {
    let Some(record) = record else {
        return Ok("None".into());
    };
    if [
        "air_clip",
        "land_clip",
        "bounds",
        "spawn_offset",
        "bounce_factor",
    ]
    .iter()
    .any(|k| cj(record, k).is_none())
    {
        return err("corpse source missing cooked art/geometry");
    }
    let bounce = match need(record, "bounce_factor")? {
        Json::Int(i) if (0..=65536).contains(i) => *i,
        _ => return err("corpse bounce factor outside Q16 range"),
    };
    let get_or =
        |k: &str, default: i64| -> String { cj(record, k).map_or(default.to_string(), py) };
    let to_int = |k: &str| -> Result<i64> {
        cj(record, k).map_or(Ok(0), |j| num(j).map(|f| f.trunc() as i64))
    };
    let fields = [
        ("air_clip", py(need(record, "air_clip")?)),
        ("land_clip", py(need(record, "land_clip")?)),
        ("bounce_factor", bounce.to_string()),
        ("fling_speed", get_or("fling_speed", 15 * 65536)),
        ("gravity", get_or("gravity", 48 * 65536)),
        (
            "breaker",
            lower(cj(record, "breaker").is_some_and(truthy)).to_string(),
        ),
        ("smash_bounces", to_int("smash_bounces")?.to_string()),
        (
            "remove_after_land",
            to_int("remove_after_land")?.to_string(),
        ),
        ("hold_ticks", to_int("hold_ticks")?.to_string()),
        ("bounds", bracket(list(record, "bounds")?)),
        ("spawn_offset", bracket(list(record, "spawn_offset")?)),
    ];
    Ok(format!(
        "Some(hk_sim::CorpseSpec {{{}}})",
        fields
            .iter()
            .map(|(k, v)| format!("{k}:{v}"))
            .collect::<Vec<_>>()
            .join(",")
    ))
}

fn hm_flag(health: &Value, key: &str) -> Result<bool> {
    Ok(get(health, key)?.truthy())
}

/// `generated_actor_records(region)`: (ActorSpec expression, placement) per supported actor.
pub fn generated_actor_records(actors: &[SpecActor]) -> Result<Vec<(String, Placement)>> {
    let mut output: Vec<(String, Placement)> = Vec::new();
    for actor in actors {
        let row = actor.row;
        if !row.supported {
            continue;
        }
        let who = &row.source;
        if actor.clip("walk_clip").is_none() || actor.clip("turn_clip").is_none() {
            return err(format!("supported actor is missing cooked clips: {who}"));
        }
        if output.len() == MAX_SCENE_ACTORS {
            return err("guest actor pool exceeds 32");
        }
        let (mut start_alert, mut start_right, mut rotation_q16) = (false, false, 0i64);
        let health = &row.health_manager;
        let control = &row
            .control
            .as_ref()
            .ok_or("supported actor without a control")?
            .1;
        let kind = match need(control, "kind")? {
            Json::Str(k) => k.as_str(),
            _ => return err("control kind"),
        };
        // A family whose whole hurt surface is a trigger says so.
        let wanted = cj(control, "trigger_body").is_some_and(truthy);
        let bodies: Vec<&Json> = match &row.colliders {
            Json::List(l) => l
                .iter()
                .filter(|c| {
                    cj(c, "bounds").is_some() && cj(c, "trigger").is_some_and(truthy) == wanted
                })
                .collect(),
            _ => Vec::new(),
        };
        if bodies.is_empty()
            || bodies
                .iter()
                .any(|c| cj(c, "bounds") != cj(bodies[0], "bounds"))
        {
            return err("actor requires unsupported distinct body colliders");
        }
        let invincible = match cj(control, "invincible") {
            Some(j) => truthy(j),
            None => hm_flag(health, "invincible")?,
        };
        let claims = cj(control, "invincible").is_some_and(truthy)
            || cj(control, "special_death").is_some_and(truthy);
        let from_direction = cj(control, "invincible_from_direction").map_or(Ok(0.0), num)?;
        if (hm_flag(health, "hasSpecialDeath")? && !claims)
            || hm_flag(health, "hasAlternateHitAnimation")?
            || get(health, "invincibleFromDirection")?.float() != Some(from_direction)
        {
            return err("actor requires unsupported HealthManager variant");
        }
        let (x, y) = (row.position[0], row.position[1]);
        let box_ = list(bodies[0], "bounds")?;
        let b: Vec<f64> = box_.iter().map(num).collect::<Result<_>>()?;
        let bounds = [
            py_round((b[0] - x) * 65536.0),
            py_round((b[1] - y) * 65536.0),
            py_round((b[2] - x) * 65536.0),
            py_round((b[3] - y) * 65536.0),
        ];
        let clip = |key: &str| -> Result<i64> {
            actor
                .clip(key)
                .ok_or_else(|| format!("missing cooked clip {key}"))
        };
        let all_present = |keys: &[String]| keys.iter().all(|k| actor.clip(k).is_some());
        let slot_keys = |slots: &[(&str, &str)]| -> Vec<String> {
            slots.iter().map(|s| format!("{}_clip", s.0)).collect()
        };
        let join_clips = |keys: &[String]| -> Result<String> {
            Ok(keys
                .iter()
                .map(|k| clip(k).map(|c| c.to_string()))
                .collect::<Result<Vec<_>>>()?
                .join(","))
        };
        let keyed = |keys: &[&str]| -> Vec<String> { keys.iter().map(|s| s.to_string()).collect() };

        let mut extra_clips: Vec<String> = Vec::new();
        let (
            controller,
            speed,
            turn_ticks,
            turn_cooldown_ticks,
            initial_direction,
            random_start_direction,
        );
        match kind {
            "ZombieSwipeWalker" => {
                let runner_clips = keyed(&[
                    "idle_clip",
                    "anticipate_clip",
                    "lunge_clip",
                    "cooldown_clip",
                ]);
                if !all_present(&runner_clips) {
                    return err(format!("Runner is missing cooked clips: {who}"));
                }
                if !actor.has_corpse() {
                    return err(format!("Runner is missing validated corpse: {who}"));
                }
                let p = need(control, "parameters")?;
                let default_attack = Json::Obj(vec![("kind".into(), Json::Str("Swipe".into()))]);
                let attack = cj(p, "attack").unwrap_or(&default_attack);
                let attack_text = if cj(attack, "kind") == Some(&Json::Str("Leap".into())) {
                    format!(
                        "hk_sim::runner::Attack::Leap {{trigger_ticks:{},jump_speed_y:{},jump_x_factor:{},idle_ticks:{}}}",
                        num(need(attack, "trigger_ticks")?)?.trunc() as i64,
                        py_round(num(need(attack, "jump_speed_y")?)? * 65536.0),
                        py_round(num(need(attack, "jump_x_factor")?)? * 65536.0),
                        ticks(num(need(attack, "idle_time")?)?)
                    )
                } else {
                    "hk_sim::runner::Attack::Swipe".to_string()
                };
                let pair = |key: &str, i: usize| -> Result<String> {
                    Ok(py(list(p, key)?.get(i).ok_or("short list")?))
                };
                let gravity =
                    py_round(cj(p, "gravity_scale").map_or(Ok(1.0), num)? * 60.0 * 65536.0);
                let params = format!(
                    "hk_sim::runner::Params {{walk_speed:{},lunge_speed:{},walking_wait:[{},{}],paused_wait:[{},{}],attack:{attack_text},gravity:{gravity}}}",
                    pair("walk_velocity_q16", 1)?,
                    pair("lunge_velocity_q16", 1)?,
                    pair("walking_wait_endpoints_ticks", 0)?,
                    pair("walking_wait_endpoints_ticks", 1)?,
                    pair("pause_endpoints_ticks", 0)?,
                    pair("pause_endpoints_ticks", 1)?
                );
                let alert = bracket(list(control, "alert_bounds_q16")?);
                let named = runner_clips
                    .iter()
                    .map(|k| clip(k).map(|c| format!("{k}:{c}")))
                    .collect::<Result<Vec<_>>>()?
                    .join(",");
                controller = format!(
                    "hk_sim::ActorController::Runner {{{named},params:{params},alert:{alert}}}"
                );
                extra_clips = runner_clips;
                speed = num(need(p, "walk_speed")?)?;
                turn_ticks = 10;
                turn_cooldown_ticks = 60;
                initial_direction = num(need(p, "initial_direction")?)? as i64;
                random_start_direction = false;
            }
            "Climber" => {
                if actor.clip("stun_clip").is_none() || !actor.has_corpse() {
                    return err(format!("Climber is missing cooked clips or corpse: {who}"));
                }
                controller = format!(
                    "hk_sim::ActorController::Climber {{stun_clip:{}}}",
                    clip("stun_clip")?
                );
                speed = 2.0;
                turn_ticks = 15;
                turn_cooldown_ticks = 0;
                initial_direction = 1;
                random_start_direction = false;
                start_right = truthy(need(control, "start_right")?);
                rotation_q16 = int(control, "rotation_q16")?;
            }
            "Vengefly" | "Baldur" | "Aspid" => {
                let keys = match kind {
                    "Vengefly" => keyed(&["startle_clip", "chase_clip", "turn_fly_clip"]),
                    "Baldur" => keyed(&["start_clip", "roll_clip", "stop_clip"]),
                    _ => keyed(&["fire_clip", "shot_clip", "impact_clip"]),
                };
                if !all_present(&keys) || !actor.has_corpse() {
                    return err(format!("{kind} is missing cooked clips or corpse: {who}"));
                }
                let named = keys
                    .iter()
                    .map(|k| clip(k).map(|c| format!("{k}:{c}")))
                    .collect::<Result<Vec<_>>>()?
                    .join(",");
                controller = format!("hk_sim::ActorController::{kind} {{{named}}}");
                extra_clips = keys;
                speed = 0.0;
                turn_ticks = 0;
                turn_cooldown_ticks = 0;
                initial_direction = -1;
                random_start_direction = false;
                if kind == "Aspid" {
                    start_alert = cj(control, "start_alert").is_some_and(truthy);
                }
            }
            "Gruzzer" => {
                if !actor.has_corpse() {
                    return err(format!("Gruzzer is missing validated corpse: {who}"));
                }
                controller = "hk_sim::ActorController::Gruzzer".into();
                (
                    speed,
                    turn_ticks,
                    turn_cooldown_ticks,
                    initial_direction,
                    random_start_direction,
                ) = (0.0, 0, 0, -1, false);
            }
            "GruzzerReserve" => {
                if !actor.has_corpse() {
                    return err(format!(
                        "Gruzzer reserve is missing validated corpse: {who}"
                    ));
                }
                controller = format!(
                    "hk_sim::ActorController::GruzzerReserve {{origin:{}}}",
                    bracket(list(control, "origin")?)
                );
                (
                    speed,
                    turn_ticks,
                    turn_cooldown_ticks,
                    initial_direction,
                    random_start_direction,
                ) = (0.0, 0, 0, -1, false);
            }
            "AcidFlyer" => {
                if !actor.has_corpse() {
                    return err(format!("Acid Flyer is missing validated corpse: {who}"));
                }
                controller = format!(
                    "hk_sim::ActorController::AcidFlyer {{amount:{},speed:{},lead:{},shell:{}}}",
                    py(need(control, "amount")?),
                    py(need(control, "speed")?),
                    bracket(list(control, "lead")?),
                    bracket(list(control, "shell")?)
                );
                (
                    speed,
                    turn_ticks,
                    turn_cooldown_ticks,
                    initial_direction,
                    random_start_direction,
                ) = (0.0, 0, 0, -1, false);
            }
            "Mosquito" => {
                let keys = slot_keys(&crate::vengefly::MOSQUITO_SLOTS);
                if !all_present(&keys) || !actor.has_corpse() {
                    return err(format!("Mosquito is missing cooked clips or corpse: {who}"));
                }
                controller = format!(
                    "hk_sim::ActorController::Mosquito {{clips:[{}],tile:{}}}",
                    join_clips(&keys)?,
                    bracket(list(control, "tile")?)
                );
                extra_clips = keys;
                (
                    speed,
                    turn_ticks,
                    turn_cooldown_ticks,
                    initial_direction,
                    random_start_direction,
                ) = (0.0, 0, 0, -1, false);
            }
            "MossWalker" => {
                let keys = slot_keys(&crate::climber::MOSS_WALKER_SLOTS);
                if !all_present(&keys) || !actor.has_corpse() {
                    return err(format!(
                        "Moss Walker is missing cooked clips or corpse: {who}"
                    ));
                }
                controller = format!(
                    "hk_sim::ActorController::MossWalker {{clips:[{}]}}",
                    join_clips(&keys)?
                );
                extra_clips = keys;
                (
                    speed,
                    turn_ticks,
                    turn_cooldown_ticks,
                    initial_direction,
                    random_start_direction,
                ) = (0.0, 0, 0, -1, false);
                // `Roams` rides the placement's start-alert word: a roamer starts awake.
                start_alert = truthy(need(control, "roams")?);
            }
            "GruzMother" => {
                controller = "hk_sim::ActorController::GruzMother".into();
                (
                    speed,
                    turn_ticks,
                    turn_cooldown_ticks,
                    random_start_direction,
                ) = (0.0, 0, 0, false);
                initial_direction = int(control, "initial_direction")?;
            }
            "Hatcher" => {
                if actor.clip("fire_clip").is_none() {
                    return err(format!("Hatcher is missing cooked clips: {who}"));
                }
                controller = format!(
                    "hk_sim::ActorController::Hatcher {{fire_clip:{}}}",
                    clip("fire_clip")?
                );
                extra_clips = keyed(&["fire_clip"]);
                (
                    speed,
                    turn_ticks,
                    turn_cooldown_ticks,
                    initial_direction,
                    random_start_direction,
                ) = (0.0, 0, 0, -1, false);
                start_alert = truthy(need(control, "start_alert")?);
            }
            "HatcherBaby" => {
                controller = "hk_sim::ActorController::HatcherBaby".into();
                (
                    speed,
                    turn_ticks,
                    turn_cooldown_ticks,
                    initial_direction,
                    random_start_direction,
                ) = (0.0, 0, 0, -1, false);
            }
            "ZombieShield" => {
                let slots: Vec<(&str, &str)> = crate::zombie_shield::SLOT_CLIPS.to_vec();
                let keys = slot_keys(&slots);
                if !all_present(&keys) {
                    return err(format!("Zombie Shield is missing cooked clips: {who}"));
                }
                controller = format!(
                    "hk_sim::ActorController::ZombieShield {{clips:[{}],attack:{}}}",
                    join_clips(&keys)?,
                    bracket(list(control, "attack_bounds_q16")?)
                );
                extra_clips = keys;
                speed = num(need(control, "walk_speed")?)?;
                turn_ticks = int(control, "turn_ticks")?;
                turn_cooldown_ticks = int(control, "turn_cooldown_ticks")?;
                initial_direction = int(control, "initial_direction")?;
                random_start_direction = false;
            }
            "HuskGuard" => {
                let slots = slot_keys(&crate::husk_guard::CLIP_SLOTS);
                let mut keys = slots.clone();
                keys.extend(keyed(&["spurt_clip", "slam_clip"]));
                if !all_present(&keys) || !actor.has_corpse() {
                    return err(format!(
                        "Husk Guard is missing cooked clips or corpse: {who}"
                    ));
                }
                controller = format!(
                    "hk_sim::ActorController::HuskGuard {{clips:[{}],spurt_clip:{},slam_clip:{}}}",
                    join_clips(&slots)?,
                    clip("spurt_clip")?,
                    clip("slam_clip")?
                );
                extra_clips = keys;
                speed = 0.0;
                turn_ticks = 0;
                turn_cooldown_ticks = 0;
                initial_direction = int(control, "initial_direction")?;
                random_start_direction = false;
            }
            "Blocker" => {
                let slots = slot_keys(&crate::blocker::CLIP_SLOTS);
                let mut keys = slots.clone();
                keys.extend(keyed(&["shot_clip", "impact_clip"]));
                if !all_present(&keys) {
                    return err(format!("Blocker is missing cooked clips: {who}"));
                }
                controller = format!(
                    "hk_sim::ActorController::Blocker {{clips:[{}],shot_clip:{},impact_clip:{},sleeps:{}}}",
                    join_clips(&slots)?,
                    clip("shot_clip")?,
                    clip("impact_clip")?,
                    lower(truthy(need(control, "sleeps")?))
                );
                extra_clips = keys;
                speed = 0.0;
                turn_ticks = 0;
                turn_cooldown_ticks = 0;
                initial_direction = int(control, "initial_direction")?;
                random_start_direction = false;
            }
            "Pigeon" => {
                let keys = slot_keys(&crate::pigeon::CLIP_SLOTS);
                if !all_present(&keys) {
                    return err(format!("Pigeon is missing cooked clips: {who}"));
                }
                controller = format!(
                    "hk_sim::ActorController::Pigeon {{clips:[{}]}}",
                    join_clips(&keys)?
                );
                extra_clips = keys;
                speed = 0.0;
                turn_ticks = 0;
                turn_cooldown_ticks = 0;
                initial_direction = int(control, "initial_direction")?;
                random_start_direction = false;
            }
            "EggSac" => {
                if actor.clip("idle_clip").is_none() || !actor.has_corpse() {
                    return err(format!("Egg Sac is missing cooked clips or corpse: {who}"));
                }
                controller = format!(
                    "hk_sim::ActorController::Static {{idle_clip:{}}}",
                    clip("idle_clip")?
                );
                extra_clips = keyed(&["idle_clip"]);
                (
                    speed,
                    turn_ticks,
                    turn_cooldown_ticks,
                    initial_direction,
                    random_start_direction,
                ) = (0.0, 0, 0, -1, false);
            }
            "FalseKnight" => {
                let keys = keyed(&[
                    "jump_antic_clip",
                    "land_clip",
                    "stun_opened_clip",
                    "attack_clip",
                    "barrel_clip",
                ]);
                if !all_present(&keys) {
                    return err(format!("False Knight is missing cooked clips: {who}"));
                }
                let trigger = list(control, "arena_trigger_world")?
                    .iter()
                    .map(|v| num(v).map(|f| py_round(f * 65536.0).to_string()))
                    .collect::<Result<Vec<_>>>()?
                    .join(",");
                let barrel = need(control, "barrel")?;
                let spawn = list(barrel, "spawn_world")?;
                let spawn_y = py_round(num(spawn.get(1).ok_or("spawn_world")?)? * 65536.0);
                let named = keys
                    .iter()
                    .map(|k| clip(k).map(|c| format!("{k}:{c}")))
                    .collect::<Result<Vec<_>>>()?
                    .join(",");
                controller = format!("hk_sim::ActorController::FalseKnight {{{named},trigger:[{trigger}],barrel_spawn_y:{spawn_y}}}");
                extra_clips = keys;
                speed = 0.0;
                turn_ticks = int(control, "turn_ticks")?;
                turn_cooldown_ticks = 0;
                initial_direction = int(control, "initial_direction")?;
                random_start_direction = false;
            }
            "Mawlek" => {
                controller = format!(
                    "hk_sim::ActorController::Mawlek {{wake:{}}}",
                    bracket(list(control, "wake_q16")?)
                );
                speed = 0.0;
                turn_ticks = 0;
                turn_cooldown_ticks = 0;
                initial_direction = int(control, "initial_direction")?;
                random_start_direction = false;
            }
            "WalkLeftRight" => {
                controller = "hk_sim::ActorController::Crawler".into();
                speed = num(need(control, "speed")?)?;
                turn_ticks = int(control, "turn_ticks")?;
                turn_cooldown_ticks = int(control, "turn_cooldown_ticks")?;
                initial_direction = int(control, "initial_direction")?;
                random_start_direction = truthy(need(control, "random_start_direction")?);
            }
            other => return err(format!("unsupported generated actor controller: {other}")),
        }
        // Every binding the spec names fits the guest's u16.
        let mut clip_fields = vec!["walk_clip".to_string(), "turn_clip".to_string()];
        clip_fields.extend(extra_clips);
        for key in &clip_fields {
            if !(0..=65535).contains(&clip(key)?) {
                return err("actor clip binding exceeds u16");
            }
        }
        let recoil = row.part("Recoil");
        let recoil_f = |key: &str| {
            recoil
                .and_then(|r| r.get(key))
                .and_then(Value::float)
                .unwrap_or(0.0)
        };
        // A recognizer whose FSM switches DamageHero on later names the value.
        let damage = match cj(control, "contact_damage") {
            Some(j) => py(j),
            None => row
                .part("DamageHero")
                .and_then(|d| d.get("damageDealt"))
                .map_or("0".to_string(), |v| py(&crate::music::value_json(v))),
        };
        // EnemyDreamnailReaction pays SOUL once, unless it sets noSoul or starts suppressed.
        let dream_soul = match row.part("EnemyDreamnailReaction") {
            Some(d) if !get(d, "noSoul")?.truthy() && !get(d, "startSuppressed")?.truthy() => 33,
            _ => 0,
        };
        if rotation_q16.rem_euclid(90 * 65536) != 0 {
            return err(format!("Climber rotation is not a quarter turn: {who}"));
        }
        let placement = Placement {
            source_id: row.spec_source_id,
            x: py_round(x * 65536.0),
            y: py_round(y * 65536.0),
            initial_direction,
            random_start_direction,
            start_alert,
            start_right,
            rotation_quarter: rotation_q16.div_euclid(90 * 65536).rem_euclid(4),
            fsm_activator: row.components.iter().any(|c| c.1 == "FSMActivator")
                && !controller.contains("GruzzerReserve")
                && !controller.contains("HatcherBaby"),
        };
        if placement.initial_direction != -1 && placement.initial_direction != 1 {
            return err(format!("actor initial direction is not a facing: {who}"));
        }
        let (recoil_speed, recoil_ticks) =
            crate::runner::recoil_fixed(recoil_f("recoilSpeedBase"), recoil_f("recoilDuration"));
        let fields = [
            ("bounds", format!("[{}]", bounds.iter().map(i64::to_string).collect::<Vec<_>>().join(","))),
            (
                "health",
                format!(
                    "hk_sim::EnemyParams {{health:{},contact_damage:{damage},evasion_ticks:{},invincible:{},damage_override:{}}}",
                    get(health, "hp")?.int().ok_or("hp")?,
                    ticks(HIT_EVASION_SECONDS),
                    lower(invincible),
                    lower(hm_flag(health, "damageOverride")?)
                ),
            ),
            ("controller", controller),
            ("walk", format!("hk_sim::WalkParams {{speed:{},turn_ticks:{turn_ticks},turn_cooldown_ticks:{turn_cooldown_ticks}}}", py_round(speed * 65536.0))),
            ("walk_clip", clip("walk_clip")?.to_string()),
            ("turn_clip", clip("turn_clip")?.to_string()),
            ("corpse", generated_corpse(actor.corpse)?),
            ("recoil_speed", recoil_speed.to_string()),
            ("recoil_ticks", recoil_ticks.to_string()),
            ("dream_soul", dream_soul.to_string()),
        ];
        output.push((
            format!(
                "hk_sim::ActorSpec {{{}}}",
                fields
                    .iter()
                    .map(|(k, v)| format!("{k}:{v}"))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            placement,
        ));
    }
    Ok(output)
}

/// `scene_actor_bank(rows)`: one scene's distinct ActorSpec expressions, and per
/// supported actor `source` its index into that list with its placement.
pub fn scene_actor_bank(regions: &[Region]) -> Result<(Vec<String>, PlacedActors)> {
    let mut order: Vec<&Region> = regions.iter().collect();
    order.sort_by_key(|r| r.chunk_id);
    let mut specs: Vec<String> = Vec::new();
    let mut placed: PlacedActors = Vec::new();
    for region in order {
        let supported: Vec<&SpecActor> = region.actors.iter().filter(|a| a.row.supported).collect();
        let cooked = supported
            .iter()
            .filter(|a| a.clip("walk_clip").is_some())
            .count();
        if cooked == 0 {
            continue;
        }
        if cooked != supported.len() {
            return err("view carries both cooked and uncooked supported actors");
        }
        let records = generated_actor_records(&region.actors)?;
        if supported.len() != records.len() {
            return err("supported actor records and generated specs disagree");
        }
        for (actor, (text, placement)) in supported.iter().zip(records) {
            let index = match specs.iter().position(|s| *s == text) {
                Some(i) => i,
                None => {
                    specs.push(text);
                    specs.len() - 1
                }
            };
            match placed.iter_mut().find(|p| p.0 == actor.row.source) {
                Some(slot) => slot.1 = (index, placement),
                None => placed.push((actor.row.source.clone(), (index, placement))),
            }
        }
    }
    if placed.len() > MAX_SCENE_ACTORS {
        return err("scene actor placements exceed the 32-slot guest pool");
    }
    Ok((specs, placed))
}

/// `generated_actor_specs(region)`: the ActorSpec expression of each supported actor, in order.
pub fn generated_actor_specs(actors: &[SpecActor]) -> Result<Vec<String>> {
    Ok(generated_actor_records(actors)?
        .into_iter()
        .map(|(text, _)| text)
        .collect())
}

/// `actor_placements(region)`: the placement words of each supported actor, in order.
pub fn actor_placements(actors: &[SpecActor]) -> Result<Vec<Placement>> {
    Ok(generated_actor_records(actors)?
        .into_iter()
        .map(|(_, placement)| placement)
        .collect())
}

/// `generated_actor_region(region)`: the inline `&[ActorSpec,...]` form (fixtures and tests).
pub fn generated_actor_region(actors: &[SpecActor]) -> Result<String> {
    Ok(format!("&[{}]", generated_actor_specs(actors)?.join(",")))
}
