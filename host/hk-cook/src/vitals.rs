//! Verified no-charm vital values and the generated parameter blocks. Ported from
//! host/actors.py (`source_vital_values`, `generated_nail_response_params`,
//! `generated_vital_params`).
//!
//! The values are read from the installed CIL initializers, never by executing
//! managed game code.

use crate::common::{err, py_round, Result};
use crate::cook_audio::sha;
use crate::pyjson::Json;
use crate::runner::ticks;
use hk_dotnet::il::{self, Instruction, Operand};
use hk_dotnet::Assembly;
use hk_unity::Source;

/// `combat.HIT_EVASION_SECONDS` is not needed here; the value is read from the CIL.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Literal {
    Int(i64),
    Float(f64),
}

impl Literal {
    fn json(self) -> Json {
        match self {
            Literal::Int(i) => Json::Int(i),
            Literal::Float(f) => Json::Float(f),
        }
    }
}

/// `_literal`: the constant an `ldc.*` instruction pushes.
fn literal(i: &Instruction) -> Option<Literal> {
    match (i.name, i.operand) {
        ("ldc.r4", Operand::F32(f)) => Some(Literal::Float(f as f64)),
        ("ldc.r8", Operand::F64(f)) => Some(Literal::Float(f)),
        ("ldc.i4", Operand::I32(v)) => Some(Literal::Int(v as i64)),
        ("ldc.i4.s", Operand::I8(v)) => Some(Literal::Int(v as i64)),
        ("ldc.i4.m1", _) => Some(Literal::Int(-1)),
        (name, _) if name.starts_with("ldc.i4.") && name.chars().last().is_some_and(|c| c.is_ascii_digit()) => Some(Literal::Int(name[name.len() - 1..].parse().unwrap())),
        _ => None,
    }
}

/// The constants `charms.hero_constants` reads from HeroController's serialized scalars.
pub type HeroConstants = [(String, f64)];

fn constant(constants: &HeroConstants, key: &str) -> Result<f64> {
    constants.iter().find(|c| c.0 == key).map(|c| c.1).ok_or_else(|| format!("hero constant {key} missing"))
}

fn flt(v: f64) -> Json {
    Json::Float(v)
}

/// `source_vital_values(source, hero_constants)`.
pub fn source_vital_values(source: &Source, constants: &HeroConstants) -> Result<Json> {
    let path = source.directory.join("Managed/Assembly-CSharp.dll");
    let asm = Assembly::open(&path).map_err(|e| e.0)?;
    let wanted = [("HealthManager", "NonFatalHit"), ("HeroController", ".ctor"), ("HeroController", "SoulGain"), ("PlayerData", "SetupNewPlayerData")];
    let mut methods: Vec<((&str, &str), Vec<Instruction>)> = Vec::new();
    for &(type_name, method_name) in &wanted {
        for (_, _, name, rva) in asm.methods_of(type_name) {
            if name == method_name {
                let code = il::body(&asm, rva).map_err(|e| e.0)?;
                let decoded = il::decode(code).map_err(|e| e.0)?;
                match methods.iter_mut().find(|m| m.0 == (type_name, method_name)) {
                    Some(slot) => slot.1 = decoded,
                    None => methods.push(((type_name, method_name), decoded)),
                }
            }
        }
    }
    if methods.len() != wanted.len() {
        return err("installed CIL vital methods missing");
    }
    let method = |key: (&str, &str)| -> &Vec<Instruction> { &methods.iter().find(|m| m.0 == key).unwrap().1 };
    // `assignment(key, field)`: the one literal stored into `field` by the method.
    let assignment = |key: (&str, &str), field: &str| -> Result<Literal> {
        let instructions = method(key);
        let mut found = Vec::new();
        for (i, ins) in instructions.iter().enumerate() {
            if ins.name != "stfld" || i == 0 {
                continue;
            }
            let Operand::Token(token) = ins.operand else { continue };
            if asm.token_name(token) == Some(field) {
                if let Some(value) = literal(&instructions[i - 1]) {
                    found.push(value);
                }
            }
        }
        if found.len() != 1 {
            return err(format!("expected one literal assignment for {key:?}/{field}, got {found:?}"));
        }
        Ok(found[0])
    };
    let setup = ("PlayerData", "SetupNewPlayerData");
    let mut values: Vec<(String, Json)> = Vec::new();
    for (key, field) in [("max_health", "maxHealth"), ("initial_health", "health"), ("nail_damage", "nailDamage"), ("max_soul", "maxMP"), ("focus_cost", "focusMP_amount")] {
        values.push((key.to_string(), assignment(setup, field)?.json()));
    }
    let soul_literals: Vec<Literal> = method(("HeroController", "SoulGain")).iter().filter_map(literal).collect();
    if soul_literals.first() != Some(&Literal::Int(11)) && soul_literals.first() != Some(&Literal::Float(11.0)) {
        return err("unvalidated no-charm SoulGain control flow");
    }
    values.push(("soul_per_hit".into(), soul_literals[0].json()));
    values.push(("death_wait_seconds".into(), assignment(("HeroController", ".ctor"), "DEATH_WAIT")?.json()));
    values.push(("enemy_hit_evasion_seconds".into(), assignment(("HealthManager", "NonFatalHit"), "evasionByHitRemaining")?.json()));
    for field in ["INVUL_TIME", "RECOIL_DURATION", "RECOIL_VELOCITY", "DAMAGE_FREEZE_DOWN", "DAMAGE_FREEZE_WAIT", "DAMAGE_FREEZE_UP"] {
        let value = constant(constants, field)?;
        if !value.is_finite() || value < 0.0 {
            return err(format!("invalid HeroController scalar {field}"));
        }
        values.push((field.to_string(), flt(value)));
    }
    values.push(("assembly_sha256".into(), Json::Str(sha(&std::fs::read(&path).map_err(|e| e.to_string())?))));
    let mut source_methods: Vec<String> = wanted.iter().map(|p| format!("{}.{}", p.0, p.1)).collect();
    source_methods.extend(["HeroController.TakeDamage", "HeroController.StartInvulnerable", "HeroController.CanTakeDamage", "HeroController.StartRecoil coroutine", "HeroController.FixedUpdate", "HealthManager.Hit", "HealthManager.Die"].map(String::from));
    values.push(("source_methods".into(), Json::List(source_methods.into_iter().map(Json::Str).collect())));
    Ok(Json::Obj(values))
}

/// `generated_nail_response_params(hero_constants, fixed_dt)`.
pub fn generated_nail_response_params(constants: &HeroConstants, fixed_dt: f64) -> Result<String> {
    let c = |k: &str| constant(constants, k);
    let fields = [
        ("recoil_ticks", ticks((c("RECOIL_HOR_STEPS")? + 1.0) * fixed_dt)),
        ("recoil_speed", py_round(c("RECOIL_HOR_VELOCITY")? * 65536.0)),
        ("bounce_ticks", ticks(c("BOUNCE_TIME")?)),
        ("high_bounce_ticks", ticks(c("BOUNCE_TIME")? + 0.03)),
        ("bounce_speed", py_round(c("BOUNCE_VELOCITY")? * 65536.0)),
        ("down_speed", py_round(c("RECOIL_DOWN_VELOCITY")? * 65536.0)),
    ];
    if fields.iter().any(|(k, v)| !(0..=if k.ends_with("ticks") { 65535 } else { 0x7fff_ffff }).contains(v)) {
        return err("nail response exceeds bounded representation");
    }
    Ok(format!("pub const NAIL_RESPONSE_PARAMS: hk_sim::NailResponseParams = hk_sim::NailResponseParams {{{}}};\n", fields.iter().map(|(k, v)| format!("{k}:{v}")).collect::<Vec<_>>().join(",")))
}

/// `generated_vital_params(values)`.
pub fn generated_vital_params(values: &Json) -> Result<String> {
    let get = |k: &str| -> Result<&Json> {
        match values {
            Json::Obj(f) => f.iter().find(|x| x.0 == k).map(|x| &x.1).ok_or_else(|| format!("vital value {k} missing")),
            _ => err("vital values are not an object"),
        }
    };
    let f = |k: &str| -> Result<f64> {
        match get(k)? {
            Json::Int(i) => Ok(*i as f64),
            Json::Float(x) => Ok(*x),
            _ => err(format!("vital value {k} is not a number")),
        }
    };
    // These four are stored through unchanged, so they have to be ints already.
    let int = |k: &str| -> Result<i64> {
        match get(k)? {
            Json::Int(i) => Ok(*i),
            _ => err("vital parameter exceeds guest fixed-width representation"),
        }
    };
    let freeze = ["DAMAGE_FREEZE_DOWN", "DAMAGE_FREEZE_WAIT", "DAMAGE_FREEZE_UP"].iter().try_fold(0.0, |acc, k| f(k).map(|v| acc + v))?;
    let fields = [
        ("max_health", int("max_health")?),
        ("max_soul", int("max_soul")?),
        ("nail_damage", int("nail_damage")?),
        ("soul_per_hit", int("soul_per_hit")?),
        ("invulnerable_ticks", ticks(f("INVUL_TIME")? + f("DAMAGE_FREEZE_DOWN")?)),
        ("hazard_invulnerable_ticks", ticks(f("INVUL_TIME")? / 2.0 + f("DAMAGE_FREEZE_DOWN")?)),
        ("recoil_ticks", ticks(f("RECOIL_DURATION")?)),
        ("freeze_ticks", ticks(freeze)),
        ("death_ticks", ticks(f("death_wait_seconds")?)),
        ("recoil_speed", py_round(f("RECOIL_VELOCITY")? * 65536.0)),
    ];
    if fields.iter().any(|(k, v)| !(0..=if *k == "recoil_speed" { 0x7fff_ffff } else { 65535 }).contains(v)) {
        return err("vital parameter exceeds guest fixed-width representation");
    }
    Ok(format!(
        "pub const VITAL_PARAMS: hk_sim::VitalParams = hk_sim::VitalParams {{{}}};\npub const ENEMY_HIT_EVASION_TICKS: u16 = {};\n",
        fields.iter().map(|(k, v)| format!("{k}:{v}")).collect::<Vec<_>>().join(","),
        ticks(f("enemy_hit_evasion_seconds")?)
    ))
}
