//! The Gruz Mother's contract and art, cooked into Crossroads_04 as a postpass. Ported from
//! the Gruz Mother section of host/false_knight_art.py.
//!
//! The third boss bank through the same decomposition. host/gruzzer.py admits the one `Giant Fly`
//! with the one-frame `Charge` clip in both generic slots; this module checks every number
//! shared/hk-sim/src/gruz_mother.rs runs against the installed source and cooks every clip the
//! body, its corpse and the corpse's burster play into the scene's own bank.

use crate::actor_art::{Clip, Frame};
use crate::atlas::{Atlas, Quantizer, MAX_TEXTURE_AXIS};
use crate::common::{component_records, err, get, Result};
use crate::const_eval::{rust_constants, Num};
use crate::cook::{focal, tk_sprite, CAM_Z};
use crate::cook_audio::u;
use crate::false_knight::{fsm_digest, named};
use crate::false_knight_art::{component, q};
use crate::fk_bank::{
    append_bank, plan, scenery_rects, ArtClipRec, SourceArt, SpriteKey, SpriteRec,
};
use crate::mawlek_art::{
    children_dict, clip_in, fields_of_fsms, fl2, fl4, fsm_in, fval, kid, near, req1, require, st,
};
use crate::prefab::{actions_of, num, one_of, value_of, wait_seconds, MultiPrefab};
use crate::pyjson::Json;
use crate::recog::{states, variables, xy};
use crate::runner::ticks;
use hk_unity::scene::Scene;
use hk_unity::{Source, Value};
use std::collections::HashMap;
use std::sync::Arc;

/// `hk_sim::gruz_mother::Clip` order. All seventeen come from one library; `Corpse Fly` is the corpse
/// prefab's default `Fly`.
pub const CLIPS: [&str; 17] = [
    "Sleep",
    "Wake",
    "Fly",
    "Charge Antic",
    "Charge",
    "Charge Recover",
    "Slam Down",
    "Slam Up",
    "Slam End",
    "Corpse Fly",
    "Death",
    "Fall",
    "Wiggle",
    "Stop",
    "Gurgle Once",
    "Gurgle Loop",
    "Burst",
];
fn library_clip(name: &str) -> &str {
    if name == "Corpse Fly" {
        "Fly"
    } else {
        name
    }
}
/// What the fight shows most goes to the static pages first.
pub const PRIORITY: [&str; 17] = [
    "Fly",
    "Sleep",
    "Charge Antic",
    "Charge",
    "Charge Recover",
    "Slam Down",
    "Slam Up",
    "Slam End",
    "Wake",
    "Corpse Fly",
    "Death",
    "Fall",
    "Wiggle",
    "Stop",
    "Gurgle Once",
    "Gurgle Loop",
    "Burst",
];
pub const SCENE_PAGE_LIMIT: usize = 18;
pub const STREAM_BYTES_LIMIT: i64 = 96 * 1024;
const CORPSE: &str = "Corpse Big Fly 1";
const BURSTER: &str = "Corpse Big Fly Burster";
const SCALE: f64 = 1.25;

/// Structural digests of every state machine the fight runs.
const FSM_SHA256: [(&str, &str); 5] = [
    (
        "bouncer_control",
        "18b3af97f5b91be591cfeab5a39a64e6cb5b8495f45a49a5c6500e7a12b8dfcc",
    ),
    (
        "Big Fly Control",
        "c5bb9452a43ba4a1608efbb9b0bb72db2b42f9b8416c1fa722464d2f1ab5b377",
    ),
    (
        "Battle Control",
        "658e51436bdc380aa83db0e1f02ae68addcbafb50449e8575076da29c29510c5",
    ),
    (
        "corpse",
        "fc20d5e005b2b3ad17a11971b1104435e146618ad87e53c16c52ac1ba5c71c5c",
    ),
    (
        "burster",
        "a466f287c7484796c4a86f0d26f29a154f852bc01ab89c8d8ab8e3ba127fe440",
    ),
];

/// `GRUZ_CONTRACT['CLIPS']`: (frames, fps, wrapMode).
const CONTRACT_CLIPS: [(&str, usize, f64, i64); 16] = [
    ("Sleep", 9, 12.0, 0),
    ("Wake", 4, 10.0, 2),
    ("Fly", 8, 12.0, 0),
    ("Charge Antic", 4, 12.0, 2),
    ("Charge", 1, 30.0, 0),
    ("Charge Recover", 10, 12.0, 1),
    ("Slam Down", 2, 8.0, 2),
    ("Slam Up", 2, 8.0, 2),
    ("Slam End", 12, 12.0, 1),
    ("Death", 4, 20.0, 3),
    ("Fall", 3, 12.0, 2),
    ("Wiggle", 3, 12.0, 3),
    ("Stop", 3, 12.0, 2),
    ("Gurgle Once", 5, 10.0, 2),
    ("Gurgle Loop", 4, 10.0, 0),
    ("Burst", 7, 12.0, 2),
];

struct C;
impl C {
    const HEALTH: f64 = 90.0;
    const INVULNERABLE_SECONDS: f64 = 0.25;
    const CONTACT_DAMAGE: f64 = 1.0;
    const WAKE_SPEED_Y: f64 = 2.5;
    const FLY_SECONDS: f64 = 1.0;
    const BUZZ_SPEED: f64 = 5.0;
    const SUPER_WAIT_SECONDS: [f64; 2] = [2.0, 2.8];
    const CHOOSE_MAX: [f64; 2] = [3.0, 2.0];
    const CHARGES_IN_A_ROW: f64 = 3.0;
    const SLAMS_IN_A_ROW: f64 = 2.0;
    const CHARGE_ANTIC_SECONDS: f64 = 0.75;
    const CHARGE_BACK_SPEED: f64 = 3.0;
    const CHARGE_SPEED: f64 = 26.0;
    const CHARGE_RECOVER_SECONDS: f64 = 0.3;
    const SUPER_END_SECONDS: f64 = 0.5;
    const SLAM_ANTIC_SECONDS: f64 = 0.5;
    const SLAM_SECONDS: [f64; 2] = [2.5, 3.0];
    const SLAM_SPEED: f64 = 50.0;
    const SLAM_ANGLES_LEFT: [f64; 2] = [100.0, 260.0];
    const SLAM_ANGLES_RIGHT: [f64; 2] = [80.0, 280.0];
    const SLAM_END_SECONDS: f64 = 0.75;
    const SLAM_DECEL: f64 = 0.85;
    const CORPSE_SECONDS: [f64; 3] = [0.5, 3.0, 1.0];
    const BURSTER_SPEED: [f64; 2] = [12.5, 20.0];
    const BURSTER_GRAVITY: f64 = 1.0;
    const BURSTER_BOUNCE: [f64; 2] = [0.5, 1.0];
    const BURSTER_INIT_SECONDS: f64 = 0.1;
    const BURSTER_GEO: f64 = 50.0;
    const BURSTER_GEO_FLING: [[f64; 2]; 3] = [[15.0, 30.0], [80.0, 100.0], [0.75, 0.75]];
    const BURSTER_SECONDS: [f64; 7] = [1.0, 0.5, 2.0, 2.0, 2.0, 1.9, 0.16];
    const BATTLE_ENEMIES: f64 = 7.0;
    const END_WAIT_SECONDS: f64 = 2.0;
}

fn flt(v: &Value, k: &str) -> Result<f64> {
    num(get(v, k)?).ok_or_else(|| format!("{k} is not a number"))
}

fn truthy_of(v: &Value, k: &str) -> bool {
    v.get(k).is_some_and(Value::truthy)
}

fn gruz_require(what: &str, got: &[f64], want: &[f64]) -> Result<()> {
    require(&format!("Gruz Mother {what}"), got, want)
        .map_err(|e| e.replacen("Mawlek source moved: ", "", 1))
}

fn g1(what: &str, got: f64, want: f64) -> Result<()> {
    gruz_require(what, &[got], &[want])
}

/// The objects the contract and the art are read from.
pub struct Sources {
    pub body: i64,
    pub children: Vec<(String, i64)>,
    pub fsms: Vec<(String, Value)>,
    pub components: Vec<(String, Value)>,
    pub corpse: MultiPrefab,
    pub burster: MultiPrefab,
    pub battle: i64,
}

impl Sources {
    fn comp(&self, kind: &str) -> Result<&Value> {
        self.components
            .iter()
            .find(|c| c.0 == kind)
            .map(|c| &c.1)
            .ok_or_else(|| format!("Gruz Mother lacks {kind}"))
    }
    fn fsm(&self, name: &str) -> Result<&Value> {
        self.fsms
            .iter()
            .find(|f| f.0 == name)
            .map(|f| &f.1)
            .ok_or_else(|| format!("missing FSM {name}"))
    }
}

fn prefab_fsm(p: &MultiPrefab, name: &str) -> Result<Value> {
    let found: Vec<Value> = p
        .parts
        .iter()
        .find(|x| x.0 == "PlayMakerFSM")
        .map(|x| {
            x.1.iter()
                .filter_map(|(_, t)| t.get("fsm"))
                .filter(|f| f.get("name").and_then(Value::str).as_deref() == Some(name))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    if found.len() != 1 {
        return err(format!("{} lacks its {name} FSM", p.name()));
    }
    Ok(found[0].clone())
}

/// `gruz_sources(s, sc)`.
pub fn sources(sc: &Scene, source: &Source) -> Result<Sources> {
    let Some(body) = crate::gruzzer::giant_fly(sc)? else {
        return err("no Giant Fly in the scene");
    };
    let children = children_dict(sc, body)?;
    let mut fsms: Vec<(String, Value)> = fields_of_fsms(sc, body);
    let battle = named(sc, "Battle Scene")?;
    fsms.push((
        "Battle Control".into(),
        fsm_in(&fields_of_fsms(sc, battle), "Battle Control")?,
    ));
    let mut components: Vec<(String, Value)> = Vec::new();
    for (_, kind, tree) in component_records(sc, body) {
        match components.iter_mut().find(|c| c.0 == kind) {
            Some(slot) => slot.1 = tree.clone(),
            None => components.push((kind.to_string(), tree.clone())),
        }
    }
    let death = components
        .iter()
        .find(|c| c.0 == "EnemyDeathEffects")
        .map(|c| &c.1)
        .ok_or("Gruz Mother lacks EnemyDeathEffects")?;
    let corpse_o = u(sc.deref(get(death, "corpsePrefab")?))?;
    let corpse = MultiPrefab::read(source, corpse_o.clone())?;
    if corpse.name() != CORPSE {
        return err(format!(
            "Gruz Mother corpse prefab is now {}",
            corpse.name()
        ));
    }
    let corpse_fsm = prefab_fsm(&corpse, "corpse")?;
    match fsms.iter_mut().find(|f| f.0 == "corpse") {
        Some(slot) => slot.1 = corpse_fsm.clone(),
        None => fsms.push(("corpse".into(), corpse_fsm.clone())),
    }
    let sts = states(&corpse_fsm)?;
    let blow = st(&sts, "Blow")?;
    let mut bursters: Vec<MultiPrefab> = Vec::new();
    for (fields, _) in actions_of(blow, "CreateObject")? {
        let Some(reference) = hk_unity::playmaker::field(&fields, "gameObject")
            .filter(|g| g.is_map())
            .and_then(|g| g.get("value"))
        else {
            continue;
        };
        let b = MultiPrefab::read(source, u(source.deref(&corpse_o.file, reference))?)?;
        if b.name() == BURSTER {
            bursters.push(b);
        }
    }
    if bursters.len() != 1 {
        return err("the Gruz Mother corpse no longer blows out one burster");
    }
    let burster = bursters.remove(0);
    let burster_fsm = prefab_fsm(&burster, "burster")?;
    match fsms.iter_mut().find(|f| f.0 == "burster") {
        Some(slot) => slot.1 = burster_fsm,
        None => fsms.push(("burster".into(), burster_fsm)),
    }
    Ok(Sources {
        body,
        children,
        fsms,
        components,
        corpse,
        burster,
        battle,
    })
}

/// `gruz_check_contract(s, sc, src)`.
pub fn check_contract(sc: &Scene, source: &Source, src: &Sources) -> Result<Vec<(String, String)>> {
    let digests: Vec<(String, String)> = src
        .fsms
        .iter()
        .map(|(name, fsm)| Ok((name.clone(), fsm_digest(fsm, &[])?)))
        .collect::<Result<_>>()?;
    for (name, digest) in FSM_SHA256 {
        let got = digests.iter().find(|d| d.0 == name).map(|d| d.1.as_str());
        if got != Some(digest) {
            return err(format!("Gruz Mother FSM {name} changed: {got:?}"));
        }
    }
    let health = src.comp("HealthManager")?;
    g1("hp", flt(health, "hp")?, C::HEALTH)?;
    g1(
        "invulnerableTime",
        flt(health, "invulnerableTime")?,
        C::INVULNERABLE_SECONDS,
    )?;
    if ["smallGeoDrops", "mediumGeoDrops", "largeGeoDrops"]
        .iter()
        .any(|k| truthy_of(health, k))
        || get(get(health, "battleScene")?, "m_PathID")?.truthy()
    {
        return err("Gruz Mother now drops Geo or counts in its arena");
    }
    if src
        .components
        .iter()
        .any(|c| c.0 == "DamageHero" || c.0 == "Recoil")
    {
        return err("Gruz Mother body now hurts or recoils on its own");
    }
    let damager = kid(&src.children, "Hero Damager")?;
    g1(
        "Hero Damager",
        flt(component(sc, damager, "DamageHero")?, "damageDealt")?,
        C::CONTACT_DAMAGE,
    )?;
    let m = u(sc.world(*sc.go_transform.get(&src.body).ok_or("no transform")?))?;
    gruz_require("scale", &[m[0][0], m[1][1]], &[SCALE, SCALE])?;
    let big = src.fsm("Big Fly Control")?;
    let sts = states(big)?;
    let wait = |name: &str| -> Result<f64> { Ok(wait_seconds(st(&sts, name)?)?[0]) };
    let v = one_of(st(&sts, "Wake")?, "SetVelocity2d")?;
    // x is PlayMaker's None: it keeps the sleeping body's zero.
    let x = hk_unity::playmaker::field(&v, "x").ok_or("Wake lacks x")?;
    if !x.get("useVariable").is_some_and(Value::truthy) || x.get("name").is_some_and(Value::truthy)
    {
        return err("Wake now sets an x velocity");
    }
    g1("Wake velocity", fval(&v, "y")?, C::WAKE_SPEED_Y)?;
    g1("Fly", wait("Fly")?, C::FLY_SECONDS)?;
    let r = one_of(st(&sts, "Buzz")?, "RandomFloat")?;
    gruz_require(
        "Buzz",
        &[fval(&r, "min")?, fval(&r, "max")?],
        &C::SUPER_WAIT_SECONDS,
    )?;
    let mut chooses = actions_of(st(&sts, "Super Choose")?, "SendRandomEventV2")?;
    if chooses.len() != 1 {
        return err("Super Choose needs exactly one SendRandomEventV2");
    }
    let (_, index) = chooses.remove(0);
    let params: Vec<Value> =
        crate::prefab::action_parameters(get(st(&sts, "Super Choose")?, "actionData")?, index)?
            .into_iter()
            .map(|p| p.1)
            .collect();
    let events: Vec<String> = params
        .iter()
        .filter(|p| matches!(p, Value::Str(_)))
        .filter_map(Value::str)
        .collect();
    let literals: Vec<f64> = params
        .iter()
        .filter(|p| p.is_map() && !p.get("useVariable").is_some_and(Value::truthy))
        .map(value_of)
        .collect::<Result<_>>()?;
    if events != ["CHARGE", "SLAM"] || literals != [1.0, 1.0, C::CHOOSE_MAX[0], C::CHOOSE_MAX[1]] {
        return err("Super Choose changed");
    }
    g1(
        "Charge Antic in a row",
        fval(
            &one_of(st(&sts, "Charge Antic")?, "IntCompare")?,
            "integer2",
        )?,
        C::CHARGES_IN_A_ROW,
    )?;
    g1(
        "Slam Antic in a row",
        fval(&one_of(st(&sts, "Slam Antic")?, "IntCompare")?, "integer2")?,
        C::SLAMS_IN_A_ROW,
    )?;
    g1(
        "Charge Antic",
        wait("Charge Antic")?,
        C::CHARGE_ANTIC_SECONDS,
    )?;
    g1(
        "Charge back",
        fval(
            &one_of(st(&sts, "Charge Antic")?, "SetVelocityAsAngle")?,
            "speed",
        )?,
        C::CHARGE_BACK_SPEED,
    )?;
    g1(
        "Charge back angle",
        fval(&one_of(st(&sts, "Charge Antic")?, "FloatAdd")?, "add")?,
        180.0,
    )?;
    g1(
        "Charge",
        fval(&one_of(st(&sts, "Charge")?, "SetVelocityAsAngle")?, "speed")?,
        C::CHARGE_SPEED,
    )?;
    for side in ["L", "R", "U", "D"] {
        let name = format!("Charge Recover {side}");
        g1(&name, wait(&name)?, C::CHARGE_RECOVER_SECONDS)?;
        let divides: Vec<f64> = actions_of(st(&sts, &name)?, "FloatDivide")?
            .iter()
            .map(|(f, _)| fval(f, "divideBy"))
            .collect::<Result<_>>()?;
        let muls = actions_of(st(&sts, &name)?, "FloatMultiply")?;
        if muls.len() != 1 {
            return err(format!("{name} needs exactly one FloatMultiply"));
        }
        let axis = if side == "L" || side == "R" {
            "Self Vel X"
        } else {
            "Self Vel Y"
        };
        let var_name = hk_unity::playmaker::field(&muls[0].0, "floatVariable")
            .and_then(|f| f.get("name"))
            .and_then(Value::str)
            .unwrap_or_default();
        if divides != [2.0, 2.0] || fval(&muls[0].0, "multiplyBy")? != -1.0 || var_name != axis {
            return err(format!("{name} no longer halves and mirrors {axis}"));
        }
    }
    g1(
        "Recover End",
        fval(
            &one_of(st(&sts, "Recover End")?, "SetFloatValue")?,
            "floatValue",
        )?,
        C::SUPER_END_SECONDS,
    )?;
    g1("Slam Antic", wait("Slam Antic")?, C::SLAM_ANTIC_SECONDS)?;
    let r = one_of(st(&sts, "Slam Antic")?, "RandomFloat")?;
    gruz_require(
        "Slam Time",
        &[fval(&r, "min")?, fval(&r, "max")?],
        &C::SLAM_SECONDS,
    )?;
    let big_vars = variables(big);
    let var_of = |vars: &Vec<(String, &Value)>, k: &str| -> Result<f64> {
        vars.iter()
            .find(|(n, _)| n == k)
            .and_then(|(_, v)| num(v))
            .ok_or_else(|| format!("missing variable {k}"))
    };
    g1(
        "Slam Speed",
        var_of(&big_vars, "Slam Speed")?,
        C::SLAM_SPEED,
    )?;
    for (name, want) in [
        ("Go Left", C::SLAM_ANGLES_LEFT),
        ("Turn Left", C::SLAM_ANGLES_LEFT),
        ("Go Right", C::SLAM_ANGLES_RIGHT),
        ("Turn Right", C::SLAM_ANGLES_RIGHT),
    ] {
        let values: Vec<f64> = actions_of(st(&sts, name)?, "SetFloatValue")?
            .iter()
            .map(|(f, _)| fval(f, "floatValue"))
            .collect::<Result<_>>()?;
        gruz_require(name, &values, &want)?;
    }
    for (name, sign) in [("Turn Left", 1.0), ("Turn Right", -1.0)] {
        g1(
            &format!("{name} scale"),
            fval(&one_of(st(&sts, name)?, "SetScale")?, "x")?,
            sign * SCALE,
        )?;
    }
    g1("Slam End", wait("Slam End")?, C::SLAM_END_SECONDS)?;
    g1(
        "Slam End decel",
        fval(
            &one_of(st(&sts, "Slam End")?, "DecelerateV2")?,
            "deceleration",
        )?,
        C::SLAM_DECEL,
    )?;
    g1(
        "Slam End time",
        fval(
            &one_of(st(&sts, "Slam End")?, "SetFloatValue")?,
            "floatValue",
        )?,
        0.0,
    )?;
    let bouncer = variables(src.fsm("bouncer_control")?);
    g1("bouncer Speed", var_of(&bouncer, "Speed")?, C::BUZZ_SPEED)?;
    let flag = |vars: &Vec<(String, &Value)>, k: &str| -> Result<bool> {
        vars.iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.truthy())
            .ok_or_else(|| format!("missing variable {k}"))
    };
    if !flag(&bouncer, "Starts Inactive")? || flag(&bouncer, "Start Up")? {
        return err("bouncer_control no longer starts stopped");
    }
    let corpse_fsm = src.fsm("corpse")?;
    let corpse = states(corpse_fsm)?;
    let waits: Vec<f64> = ["Init", "Steam", "Ready"]
        .iter()
        .map(|n| wait_seconds(st(&corpse, n)?).map(|w| w[0]))
        .collect::<Result<_>>()?;
    gruz_require("corpse waits", &waits, &C::CORPSE_SECONDS)?;
    let v = one_of(st(&corpse, "Blow")?, "SetVelocity2d")?;
    let scale_x = fval(
        &one_of(st(&corpse, "Blow")?, "FloatMultiply")?,
        "multiplyBy",
    )?;
    gruz_require(
        "burster launch",
        &[scale_x * SCALE, fval(&v, "y")?],
        &C::BURSTER_SPEED,
    )?;
    if src.corpse.parts.iter().any(|p| p.0 == "Rigidbody2D") {
        return err("the Gruz Mother corpse now has a body and would be flung");
    }
    let burster_fsm = src.fsm("burster")?;
    let burster = states(burster_fsm)?;
    let waits: Vec<f64> = [
        "Landed",
        "Stop Emit",
        "Stop",
        "Gurg 1",
        "Gurg 2",
        "Gurg 3",
        "Burst",
    ]
    .iter()
    .map(|n| wait_seconds(st(&burster, n)?).map(|w| w[0]))
    .collect::<Result<_>>()?;
    gruz_require("burster waits", &waits, &C::BURSTER_SECONDS)?;
    g1(
        "burster Initiate",
        wait_seconds(st(&burster, "Initiate")?)?[0],
        C::BURSTER_INIT_SECONDS,
    )?;
    let geo = one_of(st(&burster, "Geo")?, "FlingObjectsFromGlobalPool")?;
    gruz_require(
        "Geo count",
        &[fval(&geo, "spawnMin")?, fval(&geo, "spawnMax")?],
        &[C::BURSTER_GEO, C::BURSTER_GEO],
    )?;
    let [speed, angle, spread] = C::BURSTER_GEO_FLING;
    gruz_require(
        "Geo speed",
        &[fval(&geo, "speedMin")?, fval(&geo, "speedMax")?],
        &speed,
    )?;
    gruz_require(
        "Geo angle",
        &[fval(&geo, "angleMin")?, fval(&geo, "angleMax")?],
        &angle,
    )?;
    gruz_require(
        "Geo spread",
        &[
            fval(&geo, "originVariationX")?,
            fval(&geo, "originVariationY")?,
        ],
        &spread,
    )?;
    let rb = src.burster.tree("Rigidbody2D")?;
    let bounce = src.burster.tree("ObjectBounce")?;
    g1(
        "burster gravity",
        flt(rb, "m_GravityScale")?,
        C::BURSTER_GRAVITY,
    )?;
    gruz_require(
        "burster bounce",
        &[flt(bounce, "bounceFactor")?, flt(bounce, "speedThreshold")?],
        &C::BURSTER_BOUNCE,
    )?;
    let battle_fsm = src.fsm("Battle Control")?;
    let battle = states(battle_fsm)?;
    g1(
        "Battle Enemies",
        fval(&one_of(st(&battle, "Start")?, "SetIntValue")?, "intValue")?,
        C::BATTLE_ENEMIES,
    )?;
    g1(
        "End Wait",
        wait_seconds(st(&battle, "End Wait")?)?[0],
        C::END_WAIT_SECONDS,
    )?;
    let animator = src.comp("tk2dSpriteAnimator")?;
    let library = u(source.read(&u(sc.deref(get(animator, "library")?))?))?;
    let by_name = crate::recog::clips_by_name(&library)?;
    for (name, frames, fps, wrap) in CONTRACT_CLIPS {
        let clip = by_name
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| *v)
            .ok_or_else(|| format!("no clip {name}"))?;
        gruz_require(
            name,
            &[
                get(clip, "frames")?.list().map_or(0, <[Value]>::len) as f64,
                flt(clip, "fps")?,
                get(clip, "wrapMode")?.int().unwrap_or(-1) as f64,
            ],
            &[frames as f64, fps, wrap as f64],
        )?;
    }
    Ok(digests)
}

fn ct(s: f64) -> i64 {
    ticks(s)
}

/// `gruz_expected_rust()`: `GRUZ_CONTRACT` in the guest's units, keyed by the Rust constant names.
fn expected_rust() -> Vec<(&'static str, Vec<i64>)> {
    let t = |p: &[f64]| -> Vec<i64> { p.iter().map(|v| ticks(*v)).collect() };
    let clip_ticks = |name: &str| -> i64 {
        let (_, frames, fps, _) = CONTRACT_CLIPS.iter().find(|c| c.0 == name).unwrap();
        ticks(*frames as f64 / fps)
    };
    let angle = C::SLAM_ANGLES_RIGHT[0].to_radians();
    let one = |v: i64| vec![v];
    vec![
        ("HEALTH", one(C::HEALTH as i64)),
        ("INVULNERABLE_TICKS", one(ticks(0.2))),
        ("CONTACT_DAMAGE", one(C::CONTACT_DAMAGE as i64)),
        ("WAKE_SPEED_Y", one(q(C::WAKE_SPEED_Y))),
        ("WAKE_TICKS", one(clip_ticks("Wake"))),
        ("FLY_TICKS", one(ct(C::FLY_SECONDS))),
        ("BUZZ_SPEED", one(q(C::BUZZ_SPEED))),
        ("SUPER_WAIT_TICKS", t(&C::SUPER_WAIT_SECONDS)),
        (
            "CHOOSE_MAX",
            C::CHOOSE_MAX.iter().map(|v| *v as i64).collect(),
        ),
        ("CHARGES_IN_A_ROW", one(C::CHARGES_IN_A_ROW as i64)),
        ("SLAMS_IN_A_ROW", one(C::SLAMS_IN_A_ROW as i64)),
        // Both antics end on the `Charge Antic` clip's completion before their Wait.
        (
            "CHARGE_ANTIC_TICKS",
            one(ct(C::CHARGE_ANTIC_SECONDS).min(clip_ticks("Charge Antic"))),
        ),
        ("CHARGE_BACK_SPEED", one(q(C::CHARGE_BACK_SPEED))),
        ("CHARGE_SPEED", one(q(C::CHARGE_SPEED))),
        ("CHARGE_RECOVER_TICKS", one(ct(C::CHARGE_RECOVER_SECONDS))),
        ("SUPER_END_TICKS", one(ct(C::SUPER_END_SECONDS))),
        (
            "SLAM_ANTIC_TICKS",
            one(ct(C::SLAM_ANTIC_SECONDS).min(clip_ticks("Charge Antic"))),
        ),
        ("SLAM_TICKS", t(&C::SLAM_SECONDS)),
        ("SLAM_SPEED", one(q(C::SLAM_SPEED))),
        ("SLAM_DIRECTION", vec![q(angle.cos()), q(angle.sin())]),
        ("SLAM_HIT_TICKS", one(clip_ticks("Slam Down"))),
        ("SLAM_END_TICKS", one(ct(C::SLAM_END_SECONDS))),
        // DecelerateV2 runs on FixedUpdate (50 Hz); the guest steps at 60 Hz.
        ("SLAM_DECEL", one(q(C::SLAM_DECEL.powf(50.0 / 60.0)))),
        ("CORPSE_TICKS", t(&C::CORPSE_SECONDS)),
        (
            "BURSTER_SPEED",
            C::BURSTER_SPEED.iter().map(|v| q(*v)).collect(),
        ),
        ("BURSTER_GRAVITY", one(q(C::BURSTER_GRAVITY * 60.0))),
        ("BURSTER_BOUNCE", one(q(C::BURSTER_BOUNCE[0]))),
        ("BURSTER_BOUNCE_THRESHOLD", one(q(C::BURSTER_BOUNCE[1]))),
        ("BURSTER_INIT_TICKS", one(ct(C::BURSTER_INIT_SECONDS))),
        ("BURSTER_GEO", one(C::BURSTER_GEO as i64)),
        ("BURSTER_TICKS", t(&C::BURSTER_SECONDS)),
        ("BATTLE_ENEMIES", one(C::BATTLE_ENEMIES as i64)),
    ]
}

/// `gruz_check_rust()`.
pub fn check_rust(root: &std::path::Path) -> Result<()> {
    let path = root.join("shared/hk-sim/src/gruz_mother.rs");
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
            return err(format!(
                "shared/hk-sim/src/gruz_mother.rs {name} is {got:?}, the source says {want:?}"
            ));
        }
    }
    Ok(())
}

/// The geometry the guest needs beside the art.
pub struct Geometry {
    pub damager_box: [f64; 4],
    pub range: Vec<[f64; 2]>,
    pub burster_box: [f64; 4],
    pub fly_spawn: [f64; 2],
}

impl Geometry {
    pub fn to_json(&self) -> Json {
        Json::Obj(vec![
            ("damager_box".into(), fl4(self.damager_box)),
            (
                "range".into(),
                Json::List(self.range.iter().map(|p| fl2(*p)).collect()),
            ),
            ("burster_box".into(), fl4(self.burster_box)),
            ("fly_spawn".into(), fl2(self.fly_spawn)),
        ])
    }
}

/// `gruz_geometry(s, sc, src)`: `Battle Range`, `Hero Damager` and the burster's box, relative to their owner's transform.
pub fn geometry(sc: &Scene, src: &Sources) -> Result<Geometry> {
    let origin = u(sc.world(*sc.go_transform.get(&src.body).ok_or("no transform")?))?;
    let (ox, oy) = (origin[0][3], origin[1][3]);
    let damager = kid(&src.children, "Hero Damager")?;
    let m = u(sc.world(*sc.go_transform.get(&damager).ok_or("no transform")?))?;
    let bx = component(sc, damager, "BoxCollider2D")?;
    if !get(bx, "m_IsTrigger")?.truthy() {
        return err("Hero Damager is no longer a trigger");
    }
    let (off, size) = (xy(bx, "m_Offset")?, xy(bx, "m_Size")?);
    let cx = m[0][3] + off[0] * m[0][0] - ox;
    let cy = m[1][3] + off[1] * m[1][1] - oy;
    let (hw, hh) = (
        (size[0] * m[0][0]).abs() / 2.0,
        (size[1] * m[1][1]).abs() / 2.0,
    );
    let damager_box = [cx - hw, cy - hh, cx + hw, cy + hh];
    let range_go = kid(&src.children, "Battle Range")?;
    let m = u(sc.world(*sc.go_transform.get(&range_go).ok_or("no transform")?))?;
    let poly = component(sc, range_go, "PolygonCollider2D")?;
    let paths = get(get(poly, "m_Points")?, "m_Paths")?
        .list()
        .unwrap_or(&[]);
    if paths.len() != 1
        || !(3..=16).contains(&paths[0].list().map_or(0, <[Value]>::len))
        || !get(poly, "m_IsTrigger")?.truthy()
    {
        return err("Battle Range is no longer one trigger path of at most 16 points");
    }
    let poff = xy(poly, "m_Offset")?;
    let mut range = Vec::new();
    for p in paths[0].list().unwrap_or(&[]) {
        range.push([
            m[0][3] + (flt(p, "x")? + poff[0]) * m[0][0] - ox,
            m[1][3] + (flt(p, "y")? + poff[1]) * m[1][1] - oy,
        ]);
    }
    let bbox = src.burster.tree("BoxCollider2D")?;
    let scale = xy(src.burster.tree("Transform")?, "m_LocalScale")?;
    let (boff, bsize) = (xy(bbox, "m_Offset")?, xy(bbox, "m_Size")?);
    let (bx0, by0) = (boff[0] * scale[0], boff[1] * scale[1]);
    let (bw, bh) = (
        bsize[0] * scale[0].abs() / 2.0,
        bsize[1] * scale[1].abs() / 2.0,
    );
    let spawn = named(sc, "Fly Spawn")?;
    let p = u(sc.point(spawn, 0.0, 0.0, 0.0))?;
    Ok(Geometry {
        damager_box,
        range,
        burster_box: [bx0 - bw, by0 - bh, bx0 + bw, by0 + bh],
        fly_spawn: [p[0], p[1]],
    })
}

/// Each sprite's tk2d collider box, `None` for an unset one.
pub type SpriteBoxes = Vec<(SpriteKey, Option<[f64; 4]>)>;

/// `gruz_source_art(s, sc, src)`: every sprite of the seventeen clips at the 1.25 scale all three objects draw at,
/// and the collider each tk2d definition writes into the body's BoxCollider2D (`None` for an unset one).
pub fn source_art(sc: &Scene, source: &Source, src: &Sources) -> Result<(SourceArt, SpriteBoxes)> {
    let project = focal() / -CAM_Z;
    let unit = Value::Map(vec![
        ("r".into(), Value::F64(1.0)),
        ("g".into(), Value::F64(1.0)),
        ("b".into(), Value::F64(1.0)),
        ("a".into(), Value::F64(1.0)),
    ]);
    let animator = src.comp("tk2dSpriteAnimator")?;
    let lib_o = u(sc.deref(get(animator, "library")?))?;
    let library = u(source.read(&lib_o))?;
    for prefab in [&src.corpse, &src.burster] {
        let (anim_o, anim_tree) = prefab.first("tk2dSpriteAnimator")?;
        if u(source.deref(&anim_o.file, get(anim_tree, "library")?))?.sid() != lib_o.sid() {
            return err(format!(
                "{} no longer shares the Giant Fly library",
                prefab.name()
            ));
        }
        if !get(prefab.tree("tk2dSprite")?, "_color")?.py_eq(&unit) {
            return err(format!("{} is tinted", prefab.name()));
        }
    }
    let tk = src.comp("tk2dSprite")?;
    let tks = get(tk, "_scale")?;
    if !get(tk, "_color")?.py_eq(&unit) || flt(tks, "x")? != 1.0 || flt(tks, "y")? != 1.0 {
        return err("Giant Fly sprite is tinted or scaled");
    }
    let mut textures: HashMap<String, Arc<hk_pil::Image>> = HashMap::new();
    let mut collections: HashMap<String, Value> = HashMap::new();
    let mut sprites: Vec<(SpriteKey, SpriteRec)> = Vec::new();
    let mut boxes: Vec<(SpriteKey, Option<[f64; 4]>)> = Vec::new();
    let mut clips: Vec<(String, ArtClipRec)> = Vec::new();
    for name in CLIPS {
        let clip = clip_in(&library, library_clip(name))?;
        let mut keys = Vec::new();
        for frame in get(&clip, "frames")?.list().unwrap_or(&[]) {
            let co = u(source.deref(&lib_o.file, get(frame, "spriteCollection")?))?;
            let sid = co.sid();
            if !collections.contains_key(&sid) {
                collections.insert(sid.clone(), u(source.read(&co))?);
            }
            let index = get(frame, "spriteId")?.int().unwrap_or(0);
            let key = SpriteKey::Gruz(sid.clone(), index, SCALE.to_bits());
            if !sprites.iter().any(|s| s.0 == key) {
                let (image, b) = tk_sprite(
                    source,
                    &co.file,
                    &collections[&sid],
                    index as usize,
                    &mut textures,
                )?;
                let b = [b[0] * SCALE, b[1] * SCALE, b[2] * SCALE, b[3] * SCALE];
                let (w, h) = (
                    ((b[2] - b[0]) * project).ceil() as i64,
                    ((b[3] - b[1]) * project).ceil() as i64,
                );
                if w > MAX_TEXTURE_AXIS as i64 || h > MAX_TEXTURE_AXIS as i64 {
                    return err(format!(
                        "Gruz Mother {name} frame {w}x{h} exceeds the texture axis"
                    ));
                }
                sprites.push((
                    key.clone(),
                    SpriteRec {
                        image,
                        box_: b,
                        w: w.max(1) as usize,
                        h: h.max(1) as usize,
                    },
                ));
                // tk2d writes a Box sprite's own collider into the object's BoxCollider2D whenever the frame
                // changes (colliderType 2: the centre and the half extents); an Unset sprite (0) leaves the last box.
                let definition = get(&collections[&sid], "spriteDefinitions")?
                    .list()
                    .unwrap_or(&[])
                    .get(index as usize)
                    .ok_or("sprite definition")?;
                let kind = get(definition, "colliderType")?.int().unwrap_or(-1);
                let collider = match kind {
                    2 => {
                        let v = get(definition, "colliderVertices")?.list().unwrap_or(&[]);
                        let (c, h) = (
                            v.first().ok_or("collider vertices")?,
                            v.get(1).ok_or("collider vertices")?,
                        );
                        let (cx, cy, hx, hy) =
                            (flt(c, "x")?, flt(c, "y")?, flt(h, "x")?, flt(h, "y")?);
                        Some([
                            (cx - hx) * SCALE,
                            (cy - hy) * SCALE,
                            (cx + hx) * SCALE,
                            (cy + hy) * SCALE,
                        ])
                    }
                    0 => None,
                    other => {
                        return err(format!(
                            "Gruz Mother {name} sprite has collider type {other}"
                        ))
                    }
                };
                boxes.push((key.clone(), collider));
            }
            keys.push(key);
        }
        let mut record = clip.clone();
        if get(&clip, "wrapMode")?.int() == Some(3) {
            // tk2d PingPong over n frames is the 2n-2 loop 0..n-1..1.
            let n = keys.len();
            let tail: Vec<SpriteKey> = (1..n.saturating_sub(1))
                .rev()
                .map(|i| keys[i].clone())
                .collect();
            keys.extend(tail);
            if let Value::Map(fields) = &mut record {
                for (k, v) in fields.iter_mut() {
                    match &**k {
                        "wrapMode" => *v = Value::Int(0),
                        "frames" => *v = Value::List(vec![Value::Bool(false); keys.len()]),
                        "loopStart" => *v = Value::Int(0),
                        _ => {}
                    }
                }
                if !fields.iter().any(|(k, _)| &**k == "loopStart") {
                    fields.push(("loopStart".into(), Value::Int(0)));
                }
            }
        }
        clips.push((name.to_string(), ArtClipRec { record, keys }));
    }
    Ok((
        SourceArt {
            sprites,
            clips,
            floor_frames: Vec::new(),
            objects: Json::Null,
        },
        boxes,
    ))
}

/// `gruz_rust_table(...)`: data/gruz_art.rs.
pub fn rust_table(
    anchor_clip: i64,
    tables: &crate::fk_bank::BankTables,
    geo: &Geometry,
    scene_id: i64,
    sprite_boxes: &[Option<[f64; 4]>],
) -> String {
    let boxed = |v: [f64; 4]| {
        format!(
            "[{}]",
            v.iter()
                .map(|x| q(*x).to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let points = geo
        .range
        .iter()
        .map(|p| format!("[{}, {}]", q(p[0]), q(p[1])))
        .collect::<Vec<_>>()
        .join(", ");
    let mut lines: Vec<String> = vec![
        "// Generated by host/false_knight_art.py (Gruz Mother) from the installed source; do not edit.".into(),
        "// Gruz Mother's parts and geometry: see shared/hk-sim/src/gruz_mother.rs.".into(),
        format!("pub const GZ_SCENE: usize = {scene_id};"),
        format!("pub const GZ_ART_ANCHOR_CLIP: u16 = {anchor_clip};"),
        "/// Per sprite: first part (frames after the anchor clip's first), parts, streamed.".into(),
        format!("pub const GZ_ART_SPRITES: [(u16, u8, bool); {}] = [", tables.sprite_rows.len()),
    ];
    lines.extend(
        tables
            .sprite_rows
            .iter()
            .map(|(a, b, c)| format!("    ({a}, {b}, {}),", if *c { "true" } else { "false" })),
    );
    lines.push("];".into());
    lines.push(
        "/// Per art clip, in `hk_sim::gruz_mother::Clip` order: first sequence entry,".into(),
    );
    lines.push("/// frames, fps (Q16), wrap, loop start.".into());
    lines.push(format!(
        "pub const GZ_ART_CLIPS: [(u16, u8, u32, u8, u8); {}] = [",
        tables.clip_rows.len()
    ));
    lines.extend(
        tables
            .clip_rows
            .iter()
            .map(|(a, b, c, d, e, n)| format!("    ({a}, {b}, {c}, {d}, {e}), // {n}")),
    );
    lines.push("];".into());
    lines.push(format!(
        "pub const GZ_ART_SEQUENCE: [u16; {}] = [{}];",
        tables.sequence.len(),
        tables
            .sequence
            .iter()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    ));
    lines.push(
        "/// `Hero Damager`'s trigger and `Battle Range`'s polygon, relative to the body, as"
            .into(),
    );
    lines.push(
        "/// authored (facing left), Q16; and the burster's box relative to its transform.".into(),
    );
    lines.push(format!(
        "pub const GZ_DAMAGER_BOX: [i32; 4] = {};",
        boxed(geo.damager_box)
    ));
    lines.push(format!(
        "pub const GZ_RANGE: [[i32; 2]; {}] = [{}];",
        geo.range.len(),
        points
    ));
    lines.push(format!(
        "pub const GZ_BURSTER_BOX: [i32; 4] = {};",
        boxed(geo.burster_box)
    ));
    lines.push("/// Per sprite, the collider its tk2d definition writes into the body's".into());
    lines.push("/// BoxCollider2D, relative to the object, as authored (facing left), Q16;".into());
    lines.push("/// `[0, 0, 0, 0]` for a sprite that leaves the last one in place.".into());
    lines.push(format!(
        "pub const GZ_SPRITE_BOX: [[i32; 4]; {}] = [",
        sprite_boxes.len()
    ));
    lines.extend(
        sprite_boxes
            .iter()
            .map(|b| format!("    {},", b.map_or("[0, 0, 0, 0]".to_string(), boxed))),
    );
    lines.push("];".into());
    lines.join("\n") + "\n"
}

/// A region of the Gruz Mother's scene.
pub struct GruzRegion {
    pub chunk_id: i64,
    pub scene_id: i64,
}

/// What `gruz_cook_scene_bank` hands back for the caller to finish once the bank's clip base is known.
pub struct GruzWriter {
    anchor: usize,
    tables: crate::fk_bank::BankTables,
    geo: Geometry,
    sprite_boxes: Vec<Option<[f64; 4]>>,
    scene_id: i64,
    report: Vec<(String, Json)>,
}

impl GruzWriter {
    pub fn report(&self) -> Json {
        Json::Obj(self.report.clone())
    }

    /// `write(clip_base)`: data/gruz_art.rs and the art report.
    pub fn write(&mut self, root: &std::path::Path, clip_base: usize) -> Result<()> {
        let text = rust_table(
            (self.anchor + clip_base) as i64,
            &self.tables,
            &self.geo,
            self.scene_id,
            &self.sprite_boxes,
        );
        let path = root.join("data/gruz_art.rs");
        if std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
            std::fs::create_dir_all(root.join("data")).map_err(|e| e.to_string())?;
            std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        match self.report.iter_mut().find(|f| f.0 == "anchor_clip") {
            Some(slot) => slot.1 = Json::Int((self.anchor + clip_base) as i64),
            None => self.report.push((
                "anchor_clip".into(),
                Json::Int((self.anchor + clip_base) as i64),
            )),
        }
        let dir = root.join(".hkpsx/gruz-mother");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::fs::write(
            dir.join("art.json"),
            crate::pyjson::dumps(&Json::Obj(self.report.clone())) + "\n",
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// `gruz_cook_scene_bank(s, sc, actor, rows, atlas, frames, clips)`.
#[allow(clippy::too_many_arguments)]
pub fn cook_scene_bank(
    sc: &Scene,
    source: &Source,
    rows: &[GruzRegion],
    root: &std::path::Path,
    atlas: &mut Atlas,
    frames: &mut Vec<Frame>,
    clips: &mut Vec<Clip>,
    quantize: Quantizer,
) -> Result<GruzWriter> {
    let src = sources(sc, source)?;
    let digests = check_contract(sc, source, &src)?;
    check_rust(root)?;
    let geo = geometry(sc, &src)?;
    let (art, boxes) = source_art(sc, source, &src)?;
    // append_bank numbers sprites in this order; the box table follows it.
    let mut order: Vec<SpriteKey> = Vec::new();
    for name in CLIPS {
        for k in &art.clip(name)?.keys {
            if !order.contains(k) {
                order.push(k.clone());
            }
        }
    }
    let sprite_boxes: Vec<Option<[f64; 4]>> = order
        .iter()
        .map(|k| boxes.iter().find(|b| &b.0 == k).and_then(|b| b.1))
        .collect();
    let mut base = Vec::new();
    for r in rows {
        let path = root.join(format!("data/regions/region-{:03}/room.hk", r.chunk_id));
        base.push(std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?);
    }
    let scenery = scenery_rects(&base)?;
    let planned = plan(
        &art,
        &scenery,
        quantize,
        &PRIORITY,
        SCENE_PAGE_LIMIT,
        STREAM_BYTES_LIMIT,
        "Gruz Mother",
    )?;
    let names: Vec<String> = CLIPS.iter().map(|s| s.to_string()).collect();
    let tables = append_bank(
        atlas,
        frames,
        clips,
        &art,
        &planned,
        &names,
        "Gruz Mother parts",
    )?;
    let mut report: Vec<(String, Json)> = vec![("decisions".into(), Json::List(planned.decisions))];
    if let Json::Obj(totals) = planned.totals {
        report.extend(totals);
    }
    report.push((
        "art_clips".into(),
        Json::List(names.iter().map(|n| Json::Str(n.clone())).collect()),
    ));
    report.push((
        "fsm_sha256".into(),
        Json::Obj(
            digests
                .into_iter()
                .map(|(k, v)| (k, Json::Str(v)))
                .collect(),
        ),
    ));
    report.push(("geometry".into(), geo.to_json()));
    report.push((
        "anchor_clip_in_bank".into(),
        Json::Int(tables.anchor as i64),
    ));
    report.push((
        "code_sha256".into(),
        Json::Str(crate::cook_audio::sha(
            &[
                include_bytes!("gruz_art.rs").as_slice(),
                include_bytes!("fk_bank.rs").as_slice(),
            ]
            .concat(),
        )),
    ));
    let scene_id = rows
        .first()
        .map(|r| r.scene_id)
        .ok_or("a Gruz Mother bank needs at least one region")?;
    Ok(GruzWriter {
        anchor: tables.anchor,
        tables,
        geo,
        sprite_boxes,
        scene_id,
        report,
    })
}

#[allow(dead_code)]
fn unused(_: &[f64]) -> bool {
    near(0.0, 0.0) && req1("", 0.0, 0.0).is_ok()
}
