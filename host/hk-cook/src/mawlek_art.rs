//! Brooding Mawlek's contract and art, cooked into Crossroads_09 as a postpass. Ported from
//! host/mawlek_art.py.
//!
//! `CONTRACT` is every number shared/hk-sim/src/mawlek.rs runs, and `check_contract` reads
//! each one back out of the installed source (the five FSMs, the Walker, the HealthManager,
//! the shot prefab and the corpse prefab) and refuses the cook if one moved. The bank is cooked
//! through the False Knight's decomposition and residency plan (`fk_bank`).

use crate::actor_art::{Clip, Frame};
use crate::atlas::{Atlas, Quantizer, MAX_TEXTURE_AXIS};
use crate::common::{component_records, err, get, py_round, Result};
use crate::const_eval::{rust_constants, Num};
use crate::cook::{focal, tk_sprite, CAM_Z};
use crate::cook_audio::u;
use crate::false_knight::fsm_digest;
use crate::false_knight_art::{component, q};
use crate::fk_bank::{append_bank, plan, round6, scenery_rects, ArtClipRec, SourceArt, SpriteKey, SpriteRec};
use crate::prefab::{actions_of, enum_param, num, one_of, value_of, vector3_param, wait_seconds, MultiPrefab};
use crate::pyjson::Json;
use crate::recog::{state, states, variables, xy};
use crate::runner::ticks;
use hk_pil::Image;
use hk_unity::scene::Scene;
use hk_unity::{Obj, Source, Value};
use std::collections::HashMap;
use std::sync::Arc;

/// `hk_sim::mawlek::Clip` order. `Dummy Blank` is the one clip nothing draws.
pub const CLIPS: [&str; 20] = [
    "Body Idle", "Body Walk", "Idle Turn", "Dummy Blank", "Dummy Lurk", "Dummy Intro Jump", "Dummy Intro Land", "Dummy Roar", "Roar Cooldown", "Dummy Shoot Antic", "Dummy Shoot", "Dummy Jump Antic", "Dummy Jump", "Dummy Land", "Arm Idle", "Arm Swipe Antic", "Arm Swipe", "Arm Swipe Cooldown", "Head Idle", "Head Spit",
];
const BLANK: &str = "Dummy Blank";
/// The art past `Clip`: the projectile's two clips, the mouth splash, and the corpse.
const EXTRA: [&str; 4] = ["Shot", "Shot Impact", "Spit Effect", "Corpse"];
const BODY_NAME: &str = "Mawlek Body";
const SHOT_NAME: &str = "Shot Mawlek NoDrip";
const CORPSE_NAME: &str = "Corpse Egg Guardian";
pub const PRIORITY: [&str; 24] = [
    "Body Idle", "Body Walk", "Arm Idle", "Head Idle", "Head Spit", "Shot", "Shot Impact", "Spit Effect", "Idle Turn", "Arm Swipe Antic", "Arm Swipe", "Arm Swipe Cooldown", "Dummy Jump Antic", "Dummy Jump", "Dummy Land", "Dummy Intro Land", "Dummy Shoot Antic", "Dummy Shoot", "Dummy Lurk", "Dummy Intro Jump", "Dummy Roar", "Roar Cooldown", "Corpse", BLANK,
];
pub const SCENE_PAGE_LIMIT: usize = 18;
pub const STREAM_BYTES_LIMIT: i64 = 96 * 1024;

pub fn art_clips() -> Vec<String> {
    CLIPS.iter().chain(EXTRA.iter()).map(|s| s.to_string()).collect()
}

/// Which child plays a clip, so which transform scale its art is cooked at.
fn part_of(name: &str) -> &'static str {
    if ["Body Idle", "Body Walk", "Idle Turn"].contains(&name) {
        "Body"
    } else if name.starts_with("Arm") {
        "Arm"
    } else if name.starts_with("Head") {
        "Head"
    } else {
        "Dummy"
    }
}

fn child_name(part: &str) -> &'static str {
    match part {
        "Body" => BODY_NAME,
        "Dummy" => "Dummy",
        "Arm" => "Mawlek Arm R",
        _ => "Mawlek Head",
    }
}

/// Structural digests of every state machine the fight runs.
const FSM_SHA256: [(&str, &str); 7] = [
    ("Mawlek Control", "cef5f4a5b9d286e8ed579cf816306c890258bc5322b0622f0a97343cd6a7a082"),
    ("Mawlek Arm Control", "ed06cecf199accdf78d97a6d50b61061294146c63cf7578f16aee309fbe2f93b"),
    ("Mawlek Arm Control L", "9e56a3438f228eda19f93439e52219c62693a213cb3870d2f9c0878436d45bf3"),
    ("Mawlek Head", "b8a923a20552965e5159a14c9adc64d5c10818e601fb5f0313e6199c436686e3"),
    ("nail_clash_tink", "5e71bc12aae4f613ddd3c20e34f5a291cd2ed977ae334dda6e3c1051f9337b9a"),
    ("Battle Control", "06096697a5390bc878dd7820765f2822e9f059c152ed63e8bd02fedd6b554280"),
    ("corpse", "4b81de6397e3941ef8fbd33c89995db012f8cec009e3c6b0cd41521ddf1ad367"),
];

/// `CONTRACT['CLIPS']`: (frames, fps) per clip.
const CONTRACT_CLIPS: [(&str, usize, f64); 20] = [
    ("Body Idle", 3, 10.0), ("Body Walk", 3, 12.0), ("Idle Turn", 6, 10.0), ("Dummy Blank", 1, 30.0), ("Dummy Lurk", 3, 10.0), ("Dummy Intro Jump", 9, 12.0), ("Dummy Intro Land", 3, 12.0), ("Dummy Roar", 4, 15.0), ("Roar Cooldown", 5, 12.0), ("Dummy Shoot Antic", 8, 12.0), ("Dummy Shoot", 3, 10.0), ("Dummy Jump Antic", 3, 10.0), ("Dummy Jump", 7, 10.0), ("Dummy Land", 6, 12.0), ("Arm Idle", 5, 12.0), ("Arm Swipe Antic", 13, 16.0), ("Arm Swipe", 1, 15.0), ("Arm Swipe Cooldown", 3, 15.0), ("Head Idle", 3, 12.0), ("Head Spit", 3, 12.0),
];

struct Contract;
impl Contract {
    const HEALTH: f64 = 300.0;
    const INVULNERABLE_SECONDS: f64 = 0.15;
    const CONTACT_DAMAGE: f64 = 1.0;
    const WAKE_SECONDS: f64 = 0.166;
    const WAKE_JUMP_SECONDS: f64 = 0.1;
    const WAKE_JUMP_SPEED: f64 = 53.0;
    const WAKE_JUMP_ANGLE: f64 = 90.0;
    const GRAVITY: f64 = 3.0;
    const LURK_DEPTH: f64 = 3.16;
    const WAKE_DEPTH_SECONDS: f64 = 0.5;
    const WAKE_ROAR_SECONDS: f64 = 2.0;
    const IDLE_SECONDS: [f64; 2] = [2.0, 3.0];
    const JUMP_SPEED_Y: f64 = 68.0;
    const JUMP_SECONDS: f64 = 0.1;
    const JUMP_X_FACTOR: f64 = 1.25;
    const LAND_SECONDS: f64 = 0.5;
    const LAND_2_SECONDS: f64 = 0.25;
    const JUMP_COOLDOWN_SECONDS: f64 = 0.25;
    const SPIT_COOLDOWN_SECONDS: f64 = 1.75;
    const IN_A_ROW: f64 = 3.0;
    const REPEAT_ABOVE: f64 = 75.0;
    const SPIT_SHOTS: f64 = 25.0;
    const SPIT_SPEED: [f64; 2] = [32.0, 35.0];
    const SPIT_ANGLES_LEFT: [f64; 2] = [92.0, 105.0];
    const SPIT_ANGLES_RIGHT: [f64; 2] = [75.0, 88.0];
    const HEAD_IDLE_SECONDS: [f64; 2] = [0.3, 0.6];
    const HEAD_ANTIC_SECONDS: f64 = 0.083;
    const HEAD_SHOOT_SECONDS: f64 = 0.25;
    const HEAD_SHOT_SPEED: f64 = 27.0;
    const HEAD_ANGLES_LEFT: [f64; 2] = [95.0, 105.0];
    const HEAD_ANGLES_RIGHT: [f64; 2] = [75.0, 85.0];
    const ARM_PAUSE_SECONDS: f64 = 0.15;
    const WALK_SPEED: f64 = 3.0;
    const WALK_SECONDS: [f64; 2] = [1.0, 3.0];
    const PAUSE_SECONDS: [f64; 2] = [1.0, 3.0];
    const CORPSE_SECONDS: [f64; 3] = [1.5, 3.0, 1.0];
    const DEATH_SILENCE_SECONDS: f64 = 2.0;
    const BLOW_WAIT_SECONDS: f64 = 5.5;
    const END_WAIT_SECONDS: f64 = 5.0;
}

/// `_near`: within 1e-5.
pub(crate) fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-5
}

/// `_require(what, got, want)`.
pub(crate) fn require(what: &str, got: &[f64], want: &[f64]) -> Result<()> {
    if got.len() == want.len() && got.iter().zip(want).all(|(g, w)| near(*g, *w)) {
        Ok(())
    } else {
        err(format!("Mawlek source moved: {what} is {got:?}, the guest runs {want:?}"))
    }
}

pub(crate) fn req1(what: &str, got: f64, want: f64) -> Result<()> {
    require(what, &[got], &[want])
}

pub(crate) fn flt(v: &Value, k: &str) -> Result<f64> {
    num(get(v, k)?).ok_or_else(|| format!("{k} is not a number"))
}

pub(crate) fn fields_of_fsms(sc: &Scene, gid: i64) -> Vec<(String, Value)> {
    let mut out: Vec<(String, Value)> = Vec::new();
    for (_, kind, data) in component_records(sc, gid) {
        if kind == "PlayMakerFSM" {
            if let Some(f) = data.get("fsm") {
                let name = f.get("name").and_then(Value::str).unwrap_or_default();
                match out.iter_mut().find(|x| x.0 == name) {
                    Some(slot) => slot.1 = f.clone(),
                    None => out.push((name, f.clone())),
                }
            }
        }
    }
    out
}

pub(crate) fn fsm_in(list: &[(String, Value)], name: &str) -> Result<Value> {
    list.iter().find(|f| f.0 == name).map(|f| f.1.clone()).ok_or_else(|| format!("missing FSM {name}"))
}

/// `mawlek._children(sc, gid)`: name to game object over every transform parented to it, the last winning.
pub(crate) fn children_dict(sc: &Scene, gid: i64) -> Result<Vec<(String, i64)>> {
    let tid = *sc.go_transform.get(&gid).ok_or("object has no transform")?;
    let mut out: Vec<(String, i64)> = Vec::new();
    for o in sc.objects.iter().filter(|o| o.typename == "Transform") {
        if get(get(&o.tree, "m_Father")?, "m_PathID")?.int() != Some(tid) {
            continue;
        }
        let g = crate::common::go_of(&o.tree).unwrap_or(0);
        let name = get(sc.go(g).ok_or("child without a GameObject")?, "m_Name")?.str().unwrap_or_default();
        match out.iter_mut().find(|c| c.0 == name) {
            Some(slot) => slot.1 = g,
            None => out.push((name, g)),
        }
    }
    Ok(out)
}

pub(crate) fn kid(kids: &[(String, i64)], name: &str) -> Result<i64> {
    kids.iter().find(|k| k.0 == name).map(|k| k.1).ok_or_else(|| format!("missing child {name}"))
}

/// The objects the contract and the art are read from.
pub struct Sources {
    pub body: i64,
    pub children: Vec<(String, i64)>,
    pub fsms: Vec<(String, Value)>,
    pub components: Vec<(String, Value)>,
    pub shot: MultiPrefab,
    pub corpse: MultiPrefab,
    pub battle: i64,
}

impl Sources {
    fn comp(&self, kind: &str) -> Result<&Value> {
        self.components.iter().find(|c| c.0 == kind).map(|c| &c.1).ok_or_else(|| format!("Mawlek lacks {kind}"))
    }
    fn fsm(&self, name: &str) -> Result<&Value> {
        self.fsms.iter().find(|f| f.0 == name).map(|f| &f.1).ok_or_else(|| format!("missing FSM {name}"))
    }
}

pub(crate) fn first_named(sc: &Scene, name: &str) -> Result<i64> {
    sc.objects.iter().filter(|o| o.typename == "GameObject").find(|o| o.tree.get("m_Name").and_then(Value::str).as_deref() == Some(name)).map(|o| o.id).ok_or_else(|| format!("no {name}"))
}

/// `sources(s, sc)`.
pub fn sources(sc: &Scene, source: &Source) -> Result<Sources> {
    let body = first_named(sc, BODY_NAME)?;
    let children = children_dict(sc, body)?;
    let mut fsms: Vec<(String, Value)> = Vec::new();
    fsms.push(("Mawlek Control".into(), fsm_in(&fields_of_fsms(sc, body), "Mawlek Control")?));
    let arm_r = fields_of_fsms(sc, kid(&children, "Mawlek Arm R")?);
    let arm_l = fields_of_fsms(sc, kid(&children, "Mawlek Arm L")?);
    fsms.push(("Mawlek Arm Control".into(), fsm_in(&arm_r, "Mawlek Arm Control")?));
    // The left arm's `Init` lists the same three actions in another order, so each arm is pinned by its own digest.
    fsms.push(("Mawlek Arm Control L".into(), fsm_in(&arm_l, "Mawlek Arm Control")?));
    fsms.push(("nail_clash_tink".into(), fsm_in(&arm_r, "nail_clash_tink")?));
    if fsm_digest(&fsm_in(&arm_l, "nail_clash_tink")?, &[])? != fsm_digest(&fsm_in(&arm_r, "nail_clash_tink")?, &[])? {
        return err("the two Mawlek arms no longer parry the same way");
    }
    fsms.push(("Mawlek Head".into(), fsm_in(&fields_of_fsms(sc, kid(&children, "Mawlek Head")?), "Mawlek Head")?));
    let battle = first_named(sc, "Battle Scene")?;
    fsms.push(("Battle Control".into(), fsm_in(&fields_of_fsms(sc, battle), "Battle Control")?));
    let mut components: Vec<(String, Value)> = Vec::new();
    for (_, kind, tree) in component_records(sc, body) {
        match components.iter_mut().find(|c| c.0 == kind) {
            Some(slot) => slot.1 = tree.clone(),
            None => components.push((kind.to_string(), tree.clone())),
        }
    }
    let shoot_state = {
        let sts = states(&fsms[0].1)?;
        state(&sts, "Shoot").ok_or("no Shoot state")?.clone()
    };
    let fling = one_of(&shoot_state, "FlingObjectsFromGlobalPool")?;
    let shot_ref = hk_unity::playmaker::field(&fling, "gameObject").and_then(|g| g.get("value")).ok_or("no shot reference")?.clone();
    let shot_o = u(sc.deref(&shot_ref))?;
    let death = components.iter().find(|c| c.0 == "EnemyDeathEffects").map(|c| &c.1).ok_or("Mawlek lacks EnemyDeathEffects")?;
    let corpse_o = u(sc.deref(get(death, "corpsePrefab")?))?;
    let corpse = MultiPrefab::read(source, corpse_o)?;
    if corpse.name() != CORPSE_NAME {
        return err(format!("Mawlek corpse prefab is now {}", corpse.name()));
    }
    let corpse_fsm: Vec<Value> = corpse.parts.iter().find(|p| p.0 == "PlayMakerFSM").map(|p| p.1.iter().filter_map(|(_, t)| t.get("fsm")).filter(|f| f.get("name").and_then(Value::str).as_deref() == Some("corpse")).cloned().collect()).unwrap_or_default();
    if corpse_fsm.len() != 1 {
        return err("Mawlek corpse lacks its corpse FSM");
    }
    fsms.push(("corpse".into(), corpse_fsm[0].clone()));
    let shot = MultiPrefab::read(source, shot_o)?;
    if shot.name() != SHOT_NAME {
        return err(format!("Mawlek spits {}", shot.name()));
    }
    Ok(Sources { body, children, fsms, components, shot, corpse, battle })
}

pub(crate) fn st<'a>(sts: &[(String, &'a Value)], name: &str) -> Result<&'a Value> {
    state(sts, name).ok_or_else(|| format!("no state {name}"))
}

pub(crate) fn fval(fields: &hk_unity::playmaker::Fields, k: &str) -> Result<f64> {
    value_of(hk_unity::playmaker::field(fields, k).ok_or_else(|| format!("missing {k}"))?)
}

/// `check_contract(s, sc, src)`: read every number `CONTRACT` names out of the source and refuse a change.
pub fn check_contract(sc: &Scene, source: &Source, src: &Sources) -> Result<Vec<(String, String)>> {
    for (name, digest) in FSM_SHA256 {
        let got = fsm_digest(src.fsm(name)?, &[])?;
        if got != digest {
            return err(format!("Mawlek FSM {name} changed: {got}"));
        }
    }
    let health = src.comp("HealthManager")?;
    req1("hp", flt(health, "hp")?, Contract::HEALTH)?;
    req1("invulnerableTime", flt(health, "invulnerableTime")?, Contract::INVULNERABLE_SECONDS)?;
    req1("DamageHero", flt(src.comp("DamageHero")?, "damageDealt")?, Contract::CONTACT_DAMAGE)?;
    if ["smallGeoDrops", "mediumGeoDrops", "largeGeoDrops"].iter().any(|k| health.get(k).is_some_and(Value::truthy)) {
        return err("Mawlek now drops Geo");
    }
    let walker = src.comp("Walker")?;
    req1("walkSpeedR", flt(walker, "walkSpeedR")?, Contract::WALK_SPEED)?;
    req1("walkSpeedL", -flt(walker, "walkSpeedL")?, Contract::WALK_SPEED)?;
    require("pauseWait", &[flt(walker, "pauseWaitMin")?, flt(walker, "pauseWaitMax")?], &Contract::WALK_SECONDS)?;
    require("pauseTime", &[flt(walker, "pauseTimeMin")?, flt(walker, "pauseTimeMax")?], &Contract::PAUSE_SECONDS)?;
    for (flag, want) in [("preventScaleChange", 1.0), ("preventTurningToFaceHero", 1.0), ("pauses", 1.0), ("turnAfterIdlePercentage", 0.0), ("ignoreHoles", 0.0), ("startInactive", 1.0), ("ambush", 0.0), ("waitForHeroX", 0.0), ("preventTurn", 0.0)] {
        if flt(walker, flag)? != want {
            return err(format!("Mawlek Walker {flag} is {}", flt(walker, flag)?));
        }
    }
    let clip_names = ["idleClip", "walkClip", "turnClip"].map(|k| walker.get(k).and_then(Value::str).unwrap_or_default());
    if clip_names != ["Body Idle", "Body Walk", "Idle Turn"] {
        return err("Mawlek Walker clips changed");
    }
    let control = src.fsm("Mawlek Control")?;
    let sts = states(control)?;
    let wait = |name: &str| -> Result<f64> { Ok(wait_seconds(st(&sts, name)?)?[0]) };
    req1("Wake", wait("Wake")?, Contract::WAKE_SECONDS)?;
    req1("Wake Jump", wait("Wake Jump")?, Contract::WAKE_JUMP_SECONDS)?;
    let angle = one_of(st(&sts, "Wake Jump")?, "SetVelocityAsAngle")?;
    req1("Wake Jump speed", fval(&angle, "speed")?, Contract::WAKE_JUMP_SPEED)?;
    req1("Wake Jump angle", fval(&angle, "angle")?, Contract::WAKE_JUMP_ANGLE)?;
    req1("Wake In Air gravity", fval(&one_of(st(&sts, "Wake In Air")?, "SetGravity2dScale")?, "gravityScale")?, Contract::GRAVITY)?;
    req1("Init gravity", fval(&one_of(st(&sts, "Init")?, "SetGravity2dScale")?, "gravityScale")?, 0.0)?;
    let mut tweens = actions_of(st(&sts, "Wake In Air")?, "iTweenMoveBy")?;
    if tweens.len() != 1 {
        return err("Wake In Air needs exactly one iTweenMoveBy");
    }
    let (tween, tween_index) = tweens.remove(0);
    req1("iTween time", fval(&tween, "time")?, Contract::WAKE_DEPTH_SECONDS)?;
    if enum_param(st(&sts, "Wake In Air")?, tween_index, "easeType")? != Some(21) {
        return err("Wake In Air no longer tweens linearly");
    }
    require("iTween vector", &vector3_param(st(&sts, "Wake In Air")?, tween_index, "vector")?, &[0.0, 0.0, -Contract::LURK_DEPTH])?;
    let z = u(sc.world(*sc.go_transform.get(&src.body).ok_or("no transform")?))?[2][3];
    req1("Mawlek z", z, Contract::LURK_DEPTH)?;
    req1("Wake Roar", wait("Wake Roar")?, Contract::WAKE_ROAR_SECONDS)?;
    let idle = one_of(st(&sts, "Idle")?, "RandomFloat")?;
    require("Idle", &[fval(&idle, "min")?, fval(&idle, "max")?], &Contract::IDLE_SECONDS)?;
    for name in ["Jump", "Jump 2"] {
        let v = one_of(st(&sts, name)?, "SetVelocity2d")?;
        req1(&format!("{name} y"), fval(&v, "y")?, Contract::JUMP_SPEED_Y)?;
        req1(name, wait(name)?, Contract::JUMP_SECONDS)?;
    }
    for name in ["Detect Hero Pos 3", "Aim Return"] {
        let muls = actions_of(st(&sts, name)?, "FloatMultiply")?;
        if muls.len() != 2 {
            return err(format!("{name} needs two FloatMultiply actions"));
        }
        req1(&format!("{name} factor"), fval(&muls[0].0, "multiplyBy")?, Contract::JUMP_X_FACTOR)?;
    }
    req1("Land", wait("Land")?, Contract::LAND_SECONDS)?;
    req1("Land 2", wait("Land 2")?, Contract::LAND_2_SECONDS)?;
    req1("Land 2 cooldown", fval(&one_of(st(&sts, "Land 2")?, "SetFloatValue")?, "floatValue")?, Contract::JUMP_COOLDOWN_SECONDS)?;
    req1("Shoot cooldown", fval(&one_of(st(&sts, "Shoot")?, "SetFloatValue")?, "floatValue")?, Contract::SPIT_COOLDOWN_SECONDS)?;
    for name in ["Super Jump", "Detect Hero Pos 2"] {
        req1(&format!("{name} in a row"), fval(&one_of(st(&sts, name)?, "IntCompare")?, "integer2")?, Contract::IN_A_ROW)?;
    }
    req1("Repeat Check", fval(&one_of(st(&sts, "Repeat Check")?, "FloatCompare")?, "float2")?, Contract::REPEAT_ABOVE)?;
    let spray = one_of(st(&sts, "Shoot")?, "FlingObjectsFromGlobalPool")?;
    require("spit count", &[fval(&spray, "spawnMin")?, fval(&spray, "spawnMax")?], &[Contract::SPIT_SHOTS, Contract::SPIT_SHOTS])?;
    let vars = variables(control);
    let var = |k: &str| -> Result<f64> { vars.iter().find(|(n, _)| n == k).and_then(|(_, v)| num(v)).ok_or_else(|| format!("missing variable {k}")) };
    require("Shot Speed", &[var("Shot Speed")?, var("Shot Speed Max")?], &Contract::SPIT_SPEED)?;
    for (side, want) in [("L", Contract::SPIT_ANGLES_LEFT), ("R", Contract::SPIT_ANGLES_RIGHT)] {
        let values: Vec<f64> = actions_of(st(&sts, side)?, "SetFloatValue")?.iter().map(|(f, _)| fval(f, "floatValue")).collect::<Result<_>>()?;
        require(&format!("spit {side}"), &values, &want)?;
    }
    let head_fsm = src.fsm("Mawlek Head")?;
    let head = states(head_fsm)?;
    let hidle = one_of(st(&head, "Idle")?, "RandomFloat")?;
    require("Head Idle", &[fval(&hidle, "min")?, fval(&hidle, "max")?], &Contract::HEAD_IDLE_SECONDS)?;
    req1("Head Shoot Antic", wait_seconds(st(&head, "Shoot Antic")?)?[0], Contract::HEAD_ANTIC_SECONDS)?;
    req1("Head Shoot", wait_seconds(st(&head, "Shoot")?)?[0], Contract::HEAD_SHOOT_SECONDS)?;
    let hvars = variables(head_fsm);
    req1("Head Shot Speed", hvars.iter().find(|(n, _)| n == "Shot Speed").and_then(|(_, v)| num(v)).ok_or("missing variable Shot Speed")?, Contract::HEAD_SHOT_SPEED)?;
    let hspray = one_of(st(&head, "Shoot")?, "FlingObjectsFromGlobalPool")?;
    require("head count", &[fval(&hspray, "spawnMin")?, fval(&hspray, "spawnMax")?], &[1.0, 1.0])?;
    for (side, want) in [("L", Contract::HEAD_ANGLES_LEFT), ("R", Contract::HEAD_ANGLES_RIGHT)] {
        let values: Vec<f64> = actions_of(st(&head, side)?, "SetFloatValue")?.iter().map(|(f, _)| fval(f, "floatValue")).collect::<Result<_>>()?;
        require(&format!("head {side}"), &values, &want)?;
    }
    let arm_fsm = src.fsm("Mawlek Arm Control")?;
    let arm = states(arm_fsm)?;
    req1("Re attack Pause", wait_seconds(st(&arm, "Re attack Pause")?)?[0], Contract::ARM_PAUSE_SECONDS)?;
    let corpse_fsm = src.fsm("corpse")?;
    let corpse = states(corpse_fsm)?;
    let waits: Vec<f64> = ["Init", "Steam", "Ready"].iter().map(|n| wait_seconds(st(&corpse, n)?).map(|w| w[0])).collect::<Result<_>>()?;
    require("corpse waits", &waits, &Contract::CORPSE_SECONDS)?;
    let mut snaps = actions_of(st(&corpse, "Music")?, "TransitionToAudioSnapshot")?;
    if snaps.len() != 1 {
        return err("corpse Music needs exactly one TransitionToAudioSnapshot");
    }
    req1("corpse silence", fval(&snaps.remove(0).0, "transitionTime")?, Contract::DEATH_SILENCE_SECONDS)?;
    let battle_fsm = src.fsm("Battle Control")?;
    let battle = states(battle_fsm)?;
    req1("Blow Wait", wait_seconds(st(&battle, "Blow Wait")?)?[0], Contract::BLOW_WAIT_SECONDS)?;
    req1("End Wait", wait_seconds(st(&battle, "End Wait")?)?[0], Contract::END_WAIT_SECONDS)?;
    let animator = component(sc, src.body, "tk2dSpriteAnimator")?;
    let library = u(source.read(&u(sc.deref(get(animator, "library")?))?))?;
    let by_name = crate::recog::clips_by_name(&library)?;
    for (name, frames, fps) in CONTRACT_CLIPS {
        let clip = by_name.iter().find(|(k, _)| k == name).map(|(_, v)| *v).ok_or_else(|| format!("no clip {name}"))?;
        require(name, &[get(clip, "frames")?.list().map_or(0, <[Value]>::len) as f64, flt(clip, "fps")?], &[frames as f64, fps])?;
    }
    src.fsms.iter().map(|(name, fsm)| Ok((name.clone(), fsm_digest(fsm, &[])?))).collect()
}

/// `_box(tree, matrix)`: a BoxCollider2D under a world matrix, as a world box.
pub(crate) fn world_box(tree: &Value, m: &[[f64; 4]; 4]) -> Result<[f64; 4]> {
    let (off, size) = (xy(tree, "m_Offset")?, xy(tree, "m_Size")?);
    let cx = m[0][3] + off[0] * m[0][0];
    let cy = m[1][3] + off[1] * m[1][1];
    let (hw, hh) = ((size[0] * m[0][0]).abs() / 2.0, (size[1] * m[1][1]).abs() / 2.0);
    Ok([cx - hw, cy - hh, cx + hw, cy + hh])
}

pub(crate) fn fl2(v: [f64; 2]) -> Json {
    Json::List(v.iter().map(|&f| Json::Float(f)).collect())
}

pub(crate) fn fl4(v: [f64; 4]) -> Json {
    Json::List(v.iter().map(|&f| Json::Float(f)).collect())
}

/// The geometry the guest needs beside the art, relative to the body's transform.
pub struct Geometry {
    pub dummy: [f64; 2],
    pub arms: [[f64; 2]; 2],
    pub head: [f64; 2],
    pub spit: [f64; 2],
    pub head_box: [f64; 4],
    pub arm_ranges: [[f64; 4]; 2],
    pub arm_hitboxes: [[f64; 4]; 2],
    pub corpse_box: [f64; 4],
    pub corpse_fling: f64,
}

impl Geometry {
    pub fn to_json(&self) -> Json {
        Json::Obj(vec![
            ("dummy".into(), fl2(self.dummy)),
            ("arms".into(), Json::List(self.arms.iter().map(|a| fl2(*a)).collect())),
            ("head".into(), fl2(self.head)),
            ("spit".into(), fl2(self.spit)),
            ("head_box".into(), fl4(self.head_box)),
            ("arm_ranges".into(), Json::List(self.arm_ranges.iter().map(|a| fl4(*a)).collect())),
            ("arm_hitboxes".into(), Json::List(self.arm_hitboxes.iter().map(|a| fl4(*a)).collect())),
            ("corpse_box".into(), fl4(self.corpse_box)),
            ("corpse_fling".into(), Json::Float(self.corpse_fling)),
        ])
    }
}

/// `geometry(s, sc, src)`.
pub fn geometry(sc: &Scene, src: &Sources) -> Result<Geometry> {
    let body = src.body;
    let origin = u(sc.world(*sc.go_transform.get(&body).ok_or("no transform")?))?;
    let (ox, oy) = (origin[0][3], origin[1][3]);
    let unrotated = |gid: i64| -> Result<[[f64; 4]; 4]> {
        let m = u(sc.world(*sc.go_transform.get(&gid).ok_or("no transform")?))?;
        if m[0][1].abs() > 1e-6 || m[1][0].abs() > 1e-6 {
            return err(format!("{} is rotated", get(sc.go(gid).ok_or("no GameObject")?, "m_Name")?.str().unwrap_or_default()));
        }
        Ok(m)
    };
    let offset = |name: &str, turned: bool| -> Result<[f64; 2]> {
        let gid = kid(&src.children, name)?;
        let m = if turned { u(sc.world(*sc.go_transform.get(&gid).ok_or("no transform")?))? } else { unrotated(gid)? };
        Ok([m[0][3] - ox, m[1][3] - oy])
    };
    let rel = |b: [f64; 4]| [b[0] - ox, b[1] - oy, b[2] - ox, b[3] - oy];
    let head = kid(&src.children, "Mawlek Head")?;
    let head_box = rel(world_box(component(sc, head, "BoxCollider2D")?, &unrotated(head)?)?);
    let (mut ranges, mut hitboxes) = (Vec::new(), Vec::new());
    for name in ["Mawlek Arm R", "Mawlek Arm L"] {
        let gid = kid(&src.children, name)?;
        let attack = kid(&children_dict(sc, gid)?, "Attack Range")?;
        ranges.push(rel(world_box(component(sc, attack, "BoxCollider2D")?, &unrotated(attack)?)?));
        let poly = component(sc, gid, "PolygonCollider2D")?;
        let m = unrotated(gid)?;
        let poff = xy(poly, "m_Offset")?;
        let mut points: Vec<(f64, f64)> = Vec::new();
        for path in get(get(poly, "m_Points")?, "m_Paths")?.list().unwrap_or(&[]) {
            for p in path.list().unwrap_or(&[]) {
                points.push((m[0][3] + (flt(p, "x")? + poff[0]) * m[0][0], m[1][3] + (flt(p, "y")? + poff[1]) * m[1][1]));
            }
        }
        if points.is_empty() {
            return err(format!("{name} has an empty swipe collider"));
        }
        let min = |f: fn(&(f64, f64)) -> f64| points.iter().map(f).fold(f64::INFINITY, |a, b| if b < a { b } else { a });
        let max = |f: fn(&(f64, f64)) -> f64| points.iter().map(f).fold(f64::NEG_INFINITY, |a, b| if b > a { b } else { a });
        hitboxes.push(rel([min(|p| p.0), min(|p| p.1), max(|p| p.0), max(|p| p.1)]));
    }
    let shot_box = xy(src.shot.tree("BoxCollider2D")?, "m_Size")?;
    if !(near(shot_box[0], 0.640625) && near(shot_box[1], 0.5625)) {
        return err("Shot Mawlek NoDrip box differs from the pooled goop box");
    }
    if !near(flt(src.shot.tree("Rigidbody2D")?, "m_GravityScale")?, 0.6) || flt(src.shot.tree("DamageHero")?, "damageDealt")? != 1.0 {
        return err("Shot Mawlek NoDrip body differs from the pooled goop");
    }
    let corpse_box = src.corpse.tree("BoxCollider2D")?;
    let corpse_scale = xy(src.corpse.tree("Transform")?, "m_LocalScale")?;
    if !near(flt(src.corpse.tree("Rigidbody2D")?, "m_GravityScale")?, 1.0) {
        return err("Mawlek corpse gravity changed");
    }
    let death = src.comp("EnemyDeathEffects")?;
    let (coff, csize) = (xy(corpse_box, "m_Offset")?, xy(corpse_box, "m_Size")?);
    Ok(Geometry {
        dummy: offset("Dummy", false)?,
        arms: [offset("Mawlek Arm R", false)?, offset("Mawlek Arm L", false)?],
        head: offset("Mawlek Head", false)?,
        spit: offset("Spit Effect", true)?,
        head_box,
        arm_ranges: [ranges[0], ranges[1]],
        arm_hitboxes: [hitboxes[0], hitboxes[1]],
        corpse_box: [
            coff[0] * corpse_scale[0] - csize[0] * corpse_scale[0] / 2.0,
            coff[1] * corpse_scale[1] - csize[1] * corpse_scale[1] / 2.0,
            coff[0] * corpse_scale[0] + csize[0] * corpse_scale[0] / 2.0,
            coff[1] * corpse_scale[1] + csize[1] * corpse_scale[1] / 2.0,
        ],
        corpse_fling: flt(death, "corpseFlingSpeed")?,
    })
}

struct ArtBuilder<'a> {
    source: &'a Source,
    textures: HashMap<String, Arc<Image>>,
    collections: HashMap<String, Value>,
    sprites: Vec<(SpriteKey, SpriteRec)>,
    clips: Vec<(String, ArtClipRec)>,
}

impl ArtBuilder<'_> {
    /// `add(lib_o, clip, sx, sy, name, turn, tint)`.
    #[allow(clippy::too_many_arguments)]
    fn add(&mut self, lib_o: &Obj, clip: &Value, sx: f64, sy: f64, name: &str, turn: i64, tint: Option<&Value>) -> Result<()> {
        // A quarter turn is cooked into the art: the box and the image turn together.
        if ![0, 90, -90].contains(&turn) {
            return err(format!("Mawlek {name} is turned {turn} degrees"));
        }
        let project = focal() / -CAM_Z;
        let tint_repr = match tint {
            None => "None".to_string(),
            Some(t) => {
                let part = |k: &str| -> Result<String> { Ok(crate::pyfloat::repr(flt(t, k)?)) };
                format!("{{'r': {}, 'g': {}, 'b': {}, 'a': {}}}", part("r")?, part("g")?, part("b")?, part("a")?)
            }
        };
        let mut keys = Vec::new();
        for frame in get(clip, "frames")?.list().unwrap_or(&[]) {
            let co = u(self.source.deref(&lib_o.file, get(frame, "spriteCollection")?))?;
            let sid = co.sid();
            if !self.collections.contains_key(&sid) {
                self.collections.insert(sid.clone(), u(self.source.read(&co))?);
            }
            let index = get(frame, "spriteId")?.int().unwrap_or(0);
            let key = SpriteKey::Part(sid.clone(), index, round6(sx).to_bits(), round6(sy).to_bits(), turn, tint_repr.clone());
            if !self.sprites.iter().any(|s| s.0 == key) {
                let (mut image, b) = tk_sprite(self.source, &co.file, &self.collections[&sid], index as usize, &mut self.textures)?;
                let mut b = [b[0] * sx, b[1] * sy, b[2] * sx, b[3] * sy];
                if let Some(t) = tint {
                    // tk2dSprite `_color` multiplies the texel colour.
                    let k = [flt(t, "r")?, flt(t, "g")?, flt(t, "b")?];
                    for px in image.data.chunks_exact_mut(4) {
                        for c in 0..3 {
                            px[c] = py_round(px[c] as f64 * k[c]).clamp(0, 255) as u8;
                        }
                    }
                }
                if turn == -90 {
                    image = image.quarter_turn(false);
                    b = [b[1], -b[2], b[3], -b[0]];
                } else if turn == 90 {
                    image = image.quarter_turn(true);
                    b = [-b[3], b[0], -b[1], b[2]];
                }
                let (w, h) = (((b[2] - b[0]) * project).ceil() as i64, ((b[3] - b[1]) * project).ceil() as i64);
                if w > MAX_TEXTURE_AXIS as i64 || h > MAX_TEXTURE_AXIS as i64 {
                    return err(format!("Mawlek {name} frame {w}x{h} exceeds the texture axis"));
                }
                self.sprites.push((key.clone(), SpriteRec { image, box_: b, w: w.max(1) as usize, h: h.max(1) as usize }));
            }
            keys.push(key);
        }
        self.clips.push((name.to_string(), ArtClipRec { record: clip.clone(), keys }));
        Ok(())
    }
}

pub(crate) fn clip_in(library: &Value, name: &str) -> Result<Value> {
    get(library, "clips")?.list().unwrap_or(&[]).iter().find(|c| c.get("name").and_then(Value::str).as_deref() == Some(name)).cloned().ok_or_else(|| format!("no clip {name}"))
}

/// `source_art(s, sc, src)`: every sprite the bank needs; also the spit effect's turn in degrees.
pub fn source_art(sc: &Scene, source: &Source, src: &Sources) -> Result<(SourceArt, i64)> {
    let mut b = ArtBuilder { source, textures: HashMap::new(), collections: HashMap::new(), sprites: Vec::new(), clips: Vec::new() };
    let part_scale = |gid: i64| -> Result<(f64, f64)> {
        let m = u(sc.world(*sc.go_transform.get(&gid).ok_or("no transform")?))?;
        let tk = component(sc, gid, "tk2dSprite")?;
        if !get(tk, "_color")?.py_eq(&Value::Map(vec![("r".into(), Value::F64(1.0)), ("g".into(), Value::F64(1.0)), ("b".into(), Value::F64(1.0)), ("a".into(), Value::F64(1.0))])) {
            return err(format!("{} is tinted", get(sc.go(gid).ok_or("no GameObject")?, "m_Name")?.str().unwrap_or_default()));
        }
        let s = xy(tk, "_scale")?;
        Ok(((m[0][0] * s[0]).abs(), (m[1][1] * s[1]).abs()))
    };
    for name in CLIPS {
        let part = part_of(name);
        let gid = if part == "Body" { src.body } else { kid(&src.children, child_name(part))? };
        let lib_o = u(sc.deref(get(component(sc, gid, "tk2dSpriteAnimator")?, "library")?))?;
        let clip = clip_in(&u(source.read(&lib_o))?, name)?;
        if name == BLANK {
            b.clips.push((name.to_string(), ArtClipRec { record: clip, keys: Vec::new() }));
            continue;
        }
        let (sx, sy) = part_scale(gid)?;
        b.add(&lib_o, &clip, sx, sy, name, 0, None)?;
    }
    // The projectile, at its prefab scale times EnemyBullet's scaleMin.
    let animator = src.shot.tree("tk2dSpriteAnimator")?;
    let (anim_obj, _) = src.shot.first("tk2dSpriteAnimator")?;
    let lib_o = u(source.deref(&anim_obj.file, get(animator, "library")?))?;
    let library = u(source.read(&lib_o))?;
    let scale = xy(src.shot.tree("Transform")?, "m_LocalScale")?[0] * flt(src.shot.tree("EnemyBullet")?, "scaleMin")?;
    let tk = src.shot.tree("tk2dSprite")?;
    let tks = xy(tk, "_scale")?;
    for (name, clip_name) in [("Shot", "Idle"), ("Shot Impact", "Impact")] {
        let clip = clip_in(&library, clip_name)?;
        b.add(&lib_o, &clip, scale * tks[0].abs(), scale * tks[1].abs(), name, 0, None)?;
    }
    // `Spit Effect` plays `Enemy Shot` once, turned the way its transform is.
    let spit = kid(&src.children, "Spit Effect")?;
    let m = u(sc.world(*sc.go_transform.get(&spit).ok_or("no transform")?))?;
    let spit_tk = component(sc, spit, "tk2dSprite")?;
    let spit_lib = u(sc.deref(get(component(sc, spit, "tk2dSpriteAnimator")?, "library")?))?;
    let spit_clip = clip_in(&u(source.read(&spit_lib))?, "Enemy Shot")?;
    let sts = xy(spit_tk, "_scale")?;
    let sx = crate::pyfloat::hypot(m[0][0], m[1][0]) * sts[0].abs();
    let sy = crate::pyfloat::hypot(m[0][1], m[1][1]) * sts[1].abs();
    let turn = py_round(m[1][0].atan2(m[0][0]).to_degrees());
    let color = get(spit_tk, "_color")?;
    if flt(color, "a")? != 1.0 {
        return err("Spit Effect is translucent");
    }
    b.add(&spit_lib, &spit_clip, sx, sy, "Spit Effect", turn, Some(color))?;
    // The corpse's animator plays `Dummy Roar` from the Mawlek's library at the corpse's own scale.
    let canim = src.corpse.tree("tk2dSpriteAnimator")?;
    let (corpse_anim_obj, _) = src.corpse.first("tk2dSpriteAnimator")?;
    let clib_o = u(source.deref(&corpse_anim_obj.file, get(canim, "library")?))?;
    let clib = u(source.read(&clib_o))?;
    let default = get(canim, "defaultClipId")?.int().unwrap_or(-1);
    let cclip = get(&clib, "clips")?.list().unwrap_or(&[]).get(default as usize).cloned().ok_or("default clip")?;
    if get(&cclip, "name")?.str().as_deref() != Some("Dummy Roar") || !get(canim, "playAutomatically")?.truthy() {
        return err("the Mawlek corpse no longer roars");
    }
    let ctk = xy(src.corpse.tree("tk2dSprite")?, "_scale")?;
    let cscale = xy(src.corpse.tree("Transform")?, "m_LocalScale")?;
    b.add(&clib_o, &cclip, (cscale[0] * ctk[0]).abs(), (cscale[1] * ctk[1]).abs(), "Corpse", 0, None)?;
    Ok((SourceArt { sprites: b.sprites, clips: b.clips, floor_frames: Vec::new(), objects: Json::Null }, turn))
}

/// `expected_rust()`: `CONTRACT` in the guest's units, keyed by the Rust constant names.
fn expected_rust() -> Vec<(&'static str, Vec<i64>)> {
    let c = |s: f64| ticks(s);
    let t = |p: [f64; 2]| vec![ticks(p[0]), ticks(p[1])];
    let clip_ticks: Vec<i64> = CONTRACT_CLIPS.iter().map(|(_, frames, fps)| ticks(*frames as f64 / fps)).collect();
    let one = |v: i64| vec![v];
    vec![
        ("HEALTH", one(Contract::HEALTH as i64)),
        ("INVULNERABLE_TICKS", one(ticks(0.2))),
        ("CONTACT_DAMAGE", one(Contract::CONTACT_DAMAGE as i64)),
        ("WAKE_TICKS", one(c(Contract::WAKE_SECONDS))),
        ("WAKE_JUMP_TICKS", one(c(Contract::WAKE_JUMP_SECONDS))),
        ("WAKE_JUMP_SPEED", one(q(Contract::WAKE_JUMP_SPEED))),
        ("GRAVITY", one(q(Contract::GRAVITY))),
        ("LURK_DEPTH", one(q(Contract::LURK_DEPTH))),
        ("WAKE_DEPTH_TICKS", one(c(Contract::WAKE_DEPTH_SECONDS))),
        ("WAKE_ROAR_TICKS", one(c(Contract::WAKE_ROAR_SECONDS))),
        ("IDLE_TICKS", t(Contract::IDLE_SECONDS)),
        ("JUMP_SPEED_Y", one(q(Contract::JUMP_SPEED_Y))),
        ("JUMP_TICKS", one(c(Contract::JUMP_SECONDS))),
        ("JUMP_X_FACTOR", one(q(Contract::JUMP_X_FACTOR))),
        ("LAND_TICKS", one(c(Contract::LAND_SECONDS))),
        ("LAND_2_TICKS", one(c(Contract::LAND_2_SECONDS))),
        ("JUMP_COOLDOWN_TICKS", one(c(Contract::JUMP_COOLDOWN_SECONDS))),
        ("SPIT_COOLDOWN_TICKS", one(c(Contract::SPIT_COOLDOWN_SECONDS))),
        ("IN_A_ROW", one(Contract::IN_A_ROW as i64)),
        ("REPEAT_ABOVE", one(py_round(Contract::REPEAT_ABOVE))),
        ("SPIT_SHOTS", one(Contract::SPIT_SHOTS as i64)),
        ("SPIT_SPEED", Contract::SPIT_SPEED.iter().map(|v| q(*v)).collect()),
        ("SPIT_ANGLES_LEFT", Contract::SPIT_ANGLES_LEFT.iter().map(|v| py_round(*v)).collect()),
        ("SPIT_ANGLES_RIGHT", Contract::SPIT_ANGLES_RIGHT.iter().map(|v| py_round(*v)).collect()),
        ("HEAD_IDLE_TICKS", t(Contract::HEAD_IDLE_SECONDS)),
        ("HEAD_ANTIC_TICKS", one(c(Contract::HEAD_ANTIC_SECONDS))),
        ("HEAD_SHOOT_TICKS", one(c(Contract::HEAD_SHOOT_SECONDS))),
        ("HEAD_SHOT_SPEED", one(q(Contract::HEAD_SHOT_SPEED))),
        ("HEAD_ANGLES_LEFT", Contract::HEAD_ANGLES_LEFT.iter().map(|v| py_round(*v)).collect()),
        ("HEAD_ANGLES_RIGHT", Contract::HEAD_ANGLES_RIGHT.iter().map(|v| py_round(*v)).collect()),
        ("ARM_PAUSE_TICKS", one(c(Contract::ARM_PAUSE_SECONDS))),
        ("WALK_SPEED", one(q(Contract::WALK_SPEED))),
        ("WALK_TICKS", t(Contract::WALK_SECONDS)),
        ("PAUSE_TICKS", t(Contract::PAUSE_SECONDS)),
        ("CORPSE_TICKS", Contract::CORPSE_SECONDS.iter().map(|v| ticks(*v)).collect()),
        ("DEATH_SILENCE_TICKS", one(c(Contract::DEATH_SILENCE_SECONDS))),
        ("ARENA_END_TICKS", one(c(Contract::BLOW_WAIT_SECONDS + Contract::END_WAIT_SECONDS))),
        ("HEART_PIECE_TICKS", one(c(Contract::BLOW_WAIT_SECONDS))),
        ("CLIP_TICKS", clip_ticks),
    ]
}

/// `check_rust()`: shared/hk-sim/src/mawlek.rs must carry exactly the numbers the source says.
pub fn check_rust(root: &std::path::Path) -> Result<()> {
    let path = root.join("shared/hk-sim/src/mawlek.rs");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let rust = rust_constants(&text);
    for (name, want) in expected_rust() {
        let got = rust.iter().find(|r| r.0 == name).map(|r| &r.1);
        let ok = match got {
            Some(Num::List(_)) => got.unwrap().eq_ints(&want),
            Some(n) => want.len() == 1 && n.eq_int(want[0]),
            None => false,
        };
        if !ok {
            return err(format!("shared/hk-sim/src/mawlek.rs {name} is {got:?}, the source says {want:?}"));
        }
    }
    Ok(())
}

/// `rust_table(anchor_clip, sprite_rows, clip_rows, sequence, geo, scene_id)`: data/mawlek_art.rs.
pub fn rust_table(anchor_clip: i64, tables: &crate::fk_bank::BankTables, geo: &Geometry, scene_id: i64) -> String {
    let pair = |v: [f64; 2]| format!("[{}, {}]", q(v[0]), q(v[1]));
    let boxed = |v: [f64; 4]| format!("[{}]", v.iter().map(|x| q(*x).to_string()).collect::<Vec<_>>().join(", "));
    let mut lines: Vec<String> = vec![
        "// Generated by host/mawlek_art.py from the installed source; do not edit.".into(),
        "// Brooding Mawlek's parts and geometry: see docs/MAWLEK.md.".into(),
        format!("pub const MW_SCENE: usize = {scene_id};"),
        format!("pub const MW_ART_ANCHOR_CLIP: u16 = {anchor_clip};"),
        "/// Per sprite: first part (frames after the anchor clip's first), parts, streamed.".into(),
        format!("pub const MW_ART_SPRITES: [(u16, u8, bool); {}] = [", tables.sprite_rows.len()),
    ];
    lines.extend(tables.sprite_rows.iter().map(|(a, b, c)| format!("    ({a}, {b}, {}),", if *c { "true" } else { "false" })));
    lines.push("];".into());
    lines.push("/// Per art clip, in `hk_sim::mawlek::Clip` order then the extras: first sequence entry,".into());
    lines.push("/// frames, fps (Q16), wrap, loop start.".into());
    lines.push(format!("pub const MW_ART_CLIPS: [(u16, u8, u32, u8, u8); {}] = [", tables.clip_rows.len()));
    lines.extend(tables.clip_rows.iter().map(|(a, b, c, d, e, n)| format!("    ({a}, {b}, {c}, {d}, {e}), // {n}")));
    lines.push("];".into());
    lines.push(format!("pub const MW_ART_SEQUENCE: [u16; {}] = [{}];", tables.sequence.len(), tables.sequence.iter().map(i64::to_string).collect::<Vec<_>>().join(", ")));
    lines.push("/// Each child's transform relative to the body's, Q16 world units.".into());
    lines.push(format!("pub const MW_DUMMY_OFFSET: [i32; 2] = {};", pair(geo.dummy)));
    lines.push(format!("pub const MW_ARM_OFFSET: [[i32; 2]; 2] = [{}, {}];", pair(geo.arms[0]), pair(geo.arms[1])));
    lines.push(format!("pub const MW_HEAD_OFFSET: [i32; 2] = {};", pair(geo.head)));
    lines.push(format!("pub const MW_SPIT_OFFSET: [i32; 2] = {};", pair(geo.spit)));
    lines.push("/// The Head's BoxCollider2D, each arm's `Attack Range` and the bounds of its swipe".into());
    lines.push("/// PolygonCollider2D, relative to the body, Q16 `[x0, y0, x1, y1]`.".into());
    lines.push(format!("pub const MW_HEAD_BOX: [i32; 4] = {};", boxed(geo.head_box)));
    lines.push(format!("pub const MW_ARM_RANGE: [[i32; 4]; 2] = [{}, {}];", boxed(geo.arm_ranges[0]), boxed(geo.arm_ranges[1])));
    lines.push(format!("pub const MW_ARM_HITBOX: [[i32; 4]; 2] = [{}, {}];", boxed(geo.arm_hitboxes[0]), boxed(geo.arm_hitboxes[1])));
    lines.push("/// `Corpse Egg Guardian`'s box relative to its transform, and EnemyDeathEffects' fling speed.".into());
    lines.push(format!("pub const MW_CORPSE_BOX: [i32; 4] = {};", boxed(geo.corpse_box)));
    lines.push(format!("pub const MW_CORPSE_FLING: i32 = {};", q(geo.corpse_fling)));
    lines.join("\n") + "\n"
}

/// A region of Mawlek's scene.
pub struct MawlekRegion {
    pub chunk_id: i64,
    pub scene_id: i64,
}

/// What `cook_scene_bank` hands back for the caller to finish once the bank's clip base is known.
pub struct MawlekWriter {
    anchor: usize,
    tables: crate::fk_bank::BankTables,
    geo: Geometry,
    scene_id: i64,
    report: Vec<(String, Json)>,
}

impl MawlekWriter {
    pub fn report(&self) -> Json {
        Json::Obj(self.report.clone())
    }

    /// `write(clip_base)`: data/mawlek_art.rs and the art report.
    pub fn write(&mut self, root: &std::path::Path, clip_base: usize) -> Result<()> {
        let text = rust_table((self.anchor + clip_base) as i64, &self.tables, &self.geo, self.scene_id);
        let path = root.join("data/mawlek_art.rs");
        if std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
            std::fs::create_dir_all(root.join("data")).map_err(|e| e.to_string())?;
            std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        match self.report.iter_mut().find(|f| f.0 == "anchor_clip") {
            Some(slot) => slot.1 = Json::Int((self.anchor + clip_base) as i64),
            None => self.report.push(("anchor_clip".into(), Json::Int((self.anchor + clip_base) as i64))),
        }
        let dir = root.join(".hkpsx/mawlek");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::fs::write(dir.join("art.json"), crate::pyjson::dumps(&Json::Obj(self.report.clone())) + "\n").map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// `cook_scene_bank(s, sc, actor, rows, atlas, frames, clips)`.
#[allow(clippy::too_many_arguments)]
pub fn cook_scene_bank(sc: &Scene, source: &Source, rows: &[MawlekRegion], root: &std::path::Path, atlas: &mut Atlas, frames: &mut Vec<Frame>, clips: &mut Vec<Clip>, quantize: Quantizer) -> Result<MawlekWriter> {
    let src = sources(sc, source)?;
    let digests = check_contract(sc, source, &src)?;
    check_rust(root)?;
    let geo = geometry(sc, &src)?;
    let (art, turn) = source_art(sc, source, &src)?;
    let mut base = Vec::new();
    for r in rows {
        let path = root.join(format!("data/regions/region-{:03}/room.hk", r.chunk_id));
        base.push(std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?);
    }
    let scenery = scenery_rects(&base)?;
    let planned = plan(&art, &scenery, quantize, &PRIORITY, SCENE_PAGE_LIMIT, STREAM_BYTES_LIMIT, "Mawlek")?;
    let names = art_clips();
    let tables = append_bank(atlas, frames, clips, &art, &planned, &names, "Mawlek parts")?;
    let mut report: Vec<(String, Json)> = vec![("decisions".into(), Json::List(planned.decisions))];
    if let Json::Obj(totals) = planned.totals {
        report.extend(totals);
    }
    report.push(("art_clips".into(), Json::List(names.iter().map(|n| Json::Str(n.clone())).collect())));
    report.push(("fsm_sha256".into(), Json::Obj(digests.into_iter().map(|(k, v)| (k, Json::Str(v))).collect())));
    report.push(("geometry".into(), geo.to_json()));
    report.push(("spit_turn_degrees".into(), Json::Int(turn)));
    report.push(("anchor_clip_in_bank".into(), Json::Int(tables.anchor as i64)));
    report.push(("code_sha256".into(), Json::Str(crate::cook_audio::sha(&[include_bytes!("mawlek_art.rs").as_slice(), include_bytes!("fk_bank.rs").as_slice()].concat()))));
    let scene_id = rows.first().map(|r| r.scene_id).ok_or("a Mawlek bank needs at least one region")?;
    Ok(MawlekWriter { anchor: tables.anchor, tables, geo, scene_id, report })
}
