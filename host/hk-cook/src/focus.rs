//! Derive the no-charm FocusParams and clip requirements from Windows assets (host/focus.py),
//! plus the PlayMaker readers the other hero-value modules (superdash, dream_nail, spells) share.
//!
//! No retail payload is embedded; the values come from the installed assets and CIL.

use crate::breakables::{jb, jf, jfloats, jl, k, kf, ki, kl, ks};
use crate::common::{err, py_round, Result};
use crate::cook_audio::{jobj, js, sha, u};
use crate::music::value_json;
use crate::pyjson::Json;
use hk_dotnet::il::{self, Instruction, Operand};
use hk_dotnet::Assembly;
use hk_unity::playmaker::{action_fields, Fields};
use hk_unity::serialized::Cursor;
use hk_unity::typetree::{Flavor, Reader};
use hk_unity::{Obj, Source, Value};

pub const FOCUS_CLIPS: [&str; 4] = ["Focus", "Focus Get", "Focus End", "Focus Get Once"];

/// `combat.ticks`.
pub use crate::combat::ticks;

/// The MonoBehaviours of `resources.assets` whose script class is `class`, in file order.
pub(crate) fn behaviours(source: &Source, class: &str) -> Result<Vec<Obj>> {
    let file = u(source.file("resources.assets"))?;
    let mut out = Vec::new();
    for info in &file.objects {
        if info.class_id != 114 {
            continue;
        }
        let o = Obj {
            file: file.clone(),
            info: *info,
        };
        if u(source.typename(&o))? == class {
            out.push(o);
        }
    }
    Ok(out)
}

/// The Knight's HeroController behaviour and the GameObject it sits on.
pub(crate) fn hero(source: &Source) -> Result<(Obj, i64)> {
    let o = behaviours(source, "HeroController")?
        .into_iter()
        .next()
        .ok_or_else(String::new)?;
    let head = u(source.mono_head(&o))?;
    let gid = ki(k(&head, "m_GameObject")?, "m_PathID")?;
    Ok((o, gid))
}

/// `charms.hero_constants(source)`: HeroController's serialized scalars, ahead of its
/// runtime state block.
pub fn hero_constants(source: &Source) -> Result<(Obj, Value)> {
    let (hero, _) = hero(source)?;
    let mut node = (*u(source.mono_node(&hero))?).clone();
    let cut = node
        .children
        .iter()
        .position(|n| &*n.name == "hero_state")
        .ok_or("HeroController has no hero_state")?;
    node.children.truncate(cut);
    let values = u(Reader {
        c: Cursor::new(u(hero.raw())?, hero.file.big_endian),
        flavor: Flavor::Python,
    }
    .read(&node))?;
    Ok((hero, values))
}

/// `superdash._fsm(source, name)`: the Hero's PlayMaker FSM of that name.
pub(crate) fn hero_fsm(source: &Source, name: &str) -> Result<(Obj, Value)> {
    let (_, gid) = hero(source)?;
    for o in behaviours(source, "PlayMakerFSM")? {
        let head = u(source.mono_head(&o))?;
        if ki(k(&head, "m_GameObject")?, "m_PathID")? != gid {
            continue;
        }
        let fsm = k(&u(source.read(&o))?, "fsm")?.clone();
        if ks(&fsm, "name")? == name {
            return Ok((o, fsm));
        }
    }
    err(format!("no {name} FSM on the Hero"))
}

/// `fsm_variables(fsm)`: an FSM's declared variables by name; a duplicate whose values disagree is an error.
pub fn fsm_variables(fsm: &Value) -> Result<Vec<(String, Value)>> {
    let mut out: Vec<(String, Value)> = Vec::new();
    let Value::Map(groups) = k(fsm, "variables")? else {
        return Ok(out);
    };
    for (_, group) in groups {
        let Some(items) = group.list() else { continue };
        for v in items {
            if !matches!(v, Value::Map(_)) || v.get("name").is_none() {
                continue;
            }
            let name = v.get("name").and_then(Value::str).unwrap_or_default();
            let value = v.get("value").cloned();
            if let Some(existing) = out.iter().find(|e| e.0 == name) {
                let same = match &value {
                    Some(x) => existing.1.py_eq(x),
                    None => false,
                };
                if !same {
                    return err(format!(
                        "FSM {} declares {} twice with different values",
                        crate::breakables::pystr(&ks(fsm, "name").unwrap_or_default()),
                        crate::breakables::pystr(&name)
                    ));
                }
            }
            if let Some(value) = value {
                match out.iter_mut().find(|e| e.0 == name) {
                    Some(slot) => slot.1 = value,
                    None => out.push((name, value)),
                }
            }
        }
    }
    Ok(out)
}

/// A hero FSM with its states by name and its variables.
pub(crate) struct FsmReader<'a> {
    pub fsm: &'a Value,
    pub variables: Vec<(String, Value)>,
}

impl<'a> FsmReader<'a> {
    pub fn new(fsm: &'a Value) -> Result<Self> {
        Ok(FsmReader {
            fsm,
            variables: fsm_variables(fsm)?,
        })
    }

    pub fn state(&self, name: &str) -> Result<&'a Value> {
        kl(self.fsm, "states")?
            .iter()
            .find(|s| ks(s, "name").is_ok_and(|n| n == name))
            .ok_or_else(|| format!("'{name}'"))
    }

    pub fn state_names(&self) -> Result<Vec<String>> {
        kl(self.fsm, "states")?
            .iter()
            .map(|s| ks(s, "name"))
            .collect()
    }

    /// `actions(state, kind)`: the enabled actions of that short name, with their fields.
    pub fn actions(&self, state: &str, kind: &str) -> Result<Vec<Fields>> {
        let d = k(self.state(state)?, "actionData")?;
        let names = kl(d, "actionNames")?;
        let enabled = kl(d, "actionEnabled")?;
        let mut out = Vec::new();
        for (i, n) in names.iter().enumerate() {
            let n = n.str().unwrap_or_default();
            if n.rsplit('.').next() == Some(kind) && enabled.get(i).is_some_and(Value::truthy) {
                out.push(u(action_fields(d, i, false))?);
            }
        }
        Ok(out)
    }

    /// `scalar(value)` of focus.py: the variable's value when the field uses one.
    pub fn scalar(&self, value: &Value) -> Result<Value> {
        if k(value, "useVariable")?.truthy() {
            let name = ks(value, "name")?;
            return self
                .variables
                .iter()
                .find(|e| e.0 == name)
                .map(|e| e.1.clone())
                .ok_or_else(|| crate::breakables::pystr(&name));
        }
        Ok(k(value, "value")?.clone())
    }
}

/// A field of decoded action fields.
pub(crate) fn field<'a>(f: &'a Fields, name: &str) -> Result<&'a Value> {
    f.iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v)
        .ok_or_else(|| format!("'{name}'"))
}

pub(crate) fn assert_that(ok: bool, what: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        err(format!("assertion failed: {what}"))
    }
}

/// `actors._literal` of one instruction.
fn literal_int(i: &Instruction) -> Option<i64> {
    match (i.name, i.operand) {
        ("ldc.i4", Operand::I32(v)) => Some(v as i64),
        ("ldc.i4.s", Operand::I8(v)) => Some(v as i64),
        ("ldc.i4.m1", _) => Some(-1),
        (name, _)
            if name.starts_with("ldc.i4.")
                && name.chars().last().is_some_and(|c| c.is_ascii_digit()) =>
        {
            Some(name[name.len() - 1..].parse().unwrap())
        }
        _ => None,
    }
}

/// `source_focus_values(source, hero_constants)`.
pub fn source_focus_values(source: &Source, hero_constants: Option<&Value>) -> Result<Json> {
    let file = u(source.file("resources.assets"))?;
    let (hero_obj, gid) = hero(source)?;
    let mut spell: Option<(Obj, Value)> = None;
    for o in behaviours(source, "PlayMakerFSM")? {
        let head = u(source.mono_head(&o))?;
        if ki(k(&head, "m_GameObject")?, "m_PathID")? != gid {
            continue;
        }
        let fsm = k(&u(source.read(&o))?, "fsm")?.clone();
        if ks(&fsm, "name")? == "Spell Control" {
            spell = Some((o, fsm));
            break;
        }
    }
    let (fsm_o, fsm) = spell.ok_or_else(String::new)?;
    let r = FsmReader::new(&fsm)?;
    let wait = |state: &str| -> Result<Value> {
        let a = r.actions(state, "Wait")?;
        assert_that(
            a.len() == 1 && !field(&a[0], "realTime")?.truthy(),
            "single Wait",
        )?;
        r.scalar(field(&a[0], "time")?)
    };
    let speed = r.actions("Set Focus Speed", "SetFloatValue")?.remove(0);
    assert_that(
        ks(field(&speed, "floatVariable")?, "name")? == "Time Per MP Drain"
            && ks(field(&speed, "floatValue")?, "name")? == "Time Per MP Drain UnCH",
        "drain speed",
    )?;
    let charm = r
        .actions("Set Focus Speed", "PlayerDataBoolTest")?
        .remove(0);
    assert_that(
        r.scalar(field(&charm, "boolName")?)?.str().as_deref() == Some("equippedCharm_7")
            && field(&charm, "isFalse")?.str().as_deref() == Some("FINISHED"),
        "charm test",
    )?;
    let cost_gate = r.actions("Can Focus?", "IntCompare")?;
    assert_that(cost_gate.len() == 1, "one cost gate")?;
    assert_that(
        ks(field(&cost_gate[0], "integer1")?, "name")? == "MP"
            && ks(field(&cost_gate[0], "integer2")?, "name")? == "Focus MP amount"
            && field(&cost_gate[0], "lessThan")?.str().as_deref() == Some("CANCEL"),
        "cost gate",
    )?;
    let mut names = std::collections::BTreeSet::new();
    for a in r.actions("Can Focus?", "GetPlayerDataInt")? {
        names.insert(r.scalar(field(&a, "intName")?)?.str().unwrap_or_default());
    }
    assert_that(
        names
            == ["MPCharge", "focusMP_amount"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        "player data ints",
    )?;
    let initial_grace = r.actions("First Grace Check", "FloatCompare")?.remove(0);
    let repeat_grace = r.actions("Grace Check", "FloatCompare")?.remove(0);
    for state in ["First Grace Check", "Grace Check"] {
        let restore = r.actions(state, "SendMessage")?.remove(0);
        let call = field(&restore, "functionCall")?;
        assert_that(
            ks(call, "FunctionName")? == "SetMPCharge"
                && ks(k(call, "IntParameter")?, "name")? == "Start MP",
            "restore",
        )?;
    }
    for state in [
        "Focus Cancel",
        "Focus Heal",
        "Focus Get Finish",
        "Cancel All",
    ] {
        let mut stops = false;
        for a in r.actions(state, "CallMethodProper")? {
            if r.scalar(field(&a, "methodName")?)?.str().as_deref() == Some("StopMPDrain") {
                stops = true;
            }
        }
        assert_that(stops, "StopMPDrain")?;
    }
    let heal = r.actions("Set HP Amount", "SetIntValue")?.remove(0);
    assert_that(
        ks(field(&heal, "intVariable")?, "name")? == "Health Increase",
        "heal variable",
    )?;
    let mut animation: Option<(Obj, Vec<Value>)> = None;
    for o in behaviours(source, "tk2dSpriteAnimation")? {
        let t = u(source.read(&o))?;
        let clips = kl(&t, "clips")?.to_vec();
        let have: Vec<String> = clips
            .iter()
            .filter_map(|c| c.get("name").and_then(Value::str))
            .collect();
        if FOCUS_CLIPS.iter().all(|c| have.iter().any(|h| h == c)) {
            animation = Some((o, clips));
            break;
        }
    }
    let Some((animation, clips)) = animation else {
        return err("source focus clips missing");
    };
    let clip = |name: &str| -> Result<&Value> {
        clips
            .iter()
            .rev()
            .find(|c| c.get("name").and_then(Value::str).as_deref() == Some(name))
            .ok_or_else(|| format!("'{name}'"))
    };
    let own_constants;
    let hero_constants: &Value = match hero_constants {
        Some(c) => c,
        None => {
            own_constants = self::hero_constants(source)?.1;
            &own_constants
        }
    };
    let assembly_path = source.directory.join("Managed/Assembly-CSharp.dll");
    let asm = Assembly::open(&assembly_path).map_err(|e| e.0)?;
    let wanted = [
        ("PlayerData", "SetupNewPlayerData"),
        ("HeroController", "CanFocus"),
        ("HeroController", "StartMPDrain"),
        ("HeroController", "StopMPDrain"),
        ("HeroController", "Update"),
        ("HeroController", "SetMPCharge"),
    ];
    let mut found: Vec<(u32, u32, &str, &str, u32)> = Vec::new();
    for (t, m) in wanted {
        for (trid, mrid, name, rva) in asm.methods_of(t) {
            if name == m {
                found.push((trid, mrid, t, m, rva));
            }
        }
    }
    found.sort_by_key(|f| (f.0, f.1));
    let mut methods: Vec<(String, Json)> = Vec::new();
    let mut costs: Vec<Option<i64>> = Vec::new();
    for (_, _, t, m, rva) in found {
        let (code_size, bytes) = il::whole_body(&asm, rva).map_err(|e| e.0)?;
        let mut input = code_size.to_le_bytes().to_vec();
        input.extend_from_slice(bytes);
        let key = format!("{t}.{m}");
        let hash = Json::Str(sha(&input));
        match methods.iter_mut().find(|e| e.0 == key) {
            Some(slot) => slot.1 = hash,
            None => methods.push((key, hash)),
        }
        if (t, m) == ("PlayerData", "SetupNewPlayerData") {
            let code = il::body(&asm, rva).map_err(|e| e.0)?;
            let ins = il::decode(code).map_err(|e| e.0)?;
            for (i, op) in ins.iter().enumerate() {
                if op.name == "stfld" && i > 0 {
                    if let Operand::Token(token) = op.operand {
                        if asm.token_name(token) == Some("focusMP_amount") {
                            costs.push(literal_int(&ins[i - 1]));
                        }
                    }
                }
            }
        }
    }
    let Some(&Some(cost)) = (costs.len() == 1).then(|| &costs[0]) else {
        return err("focus cost initializer changed");
    };
    let f = |v: Result<Value>| -> Result<Json> { Ok(value_json(&v?)) };
    let frames = |c: &Value| -> Result<f64> { Ok(kl(c, "frames")?.len() as f64) };
    let end = clip("Focus End")?;
    let mut clip_obj = Vec::new();
    for name in FOCUS_CLIPS {
        let c = clip(name)?;
        clip_obj.push((
            name.to_string(),
            jobj(vec![
                ("fps", value_json(k(c, "fps")?)),
                ("wrapMode", value_json(k(c, "wrapMode")?)),
                ("loopStart", value_json(k(c, "loopStart")?)),
                ("frames", Json::Int(frames(c)? as i64)),
            ]),
        ));
    }
    let resources =
        std::fs::read(source.directory.join("resources.assets")).map_err(|e| e.to_string())?;
    let _ = &file;
    Ok(jobj(vec![
        ("hold_seconds", f(wait("Button Down"))?),
        ("start_seconds", f(wait("Focus Start"))?),
        ("drain_seconds", f(r.scalar(field(&speed, "floatValue")?))?),
        ("heal_seconds", f(wait("Focus Heal"))?),
        ("cancel_seconds", jf(frames(end)? / kf(end, "fps")?)),
        ("finish_seconds", f(wait("Focus Get Finish"))?),
        ("first_grace_seconds", f(r.scalar(field(&initial_grace, "float2")?))?),
        ("repeat_grace_seconds", f(r.scalar(field(&repeat_grace, "float2")?))?),
        ("cost", Json::Int(cost)),
        ("heal_amount", f(r.scalar(field(&heal, "intValue")?))?),
        (
            "attack_recovery_seconds",
            value_json(k(hero_constants, "ATTACK_RECOVERY_TIME")?),
        ),
        ("hero", Json::Str(hero_obj.sid())),
        ("spell_fsm", Json::Str(fsm_o.sid())),
        ("animation_library", Json::Str(animation.sid())),
        ("clips", Json::Obj(clip_obj)),
        ("resources_sha256", Json::Str(sha(&resources))),
        (
            "assembly_sha256",
            Json::Str(sha(&std::fs::read(&assembly_path).map_err(|e| e.to_string())?)),
        ),
        ("method_sha256", Json::Obj(methods)),
        (
            "limitations",
            jl(vec![
                js("No charms, spells, dream focus, SOUL reserve or particles implemented"),
                js("Update/PlayMaker event ordering resampled to deterministic60Hz; original-input timing capture not yet available"),
                js("Grace refund restores cycle-start SOUL; damage/scene interruption bypasses grace"),
                js("Full health is not an entry rejection in the observed Can Focus? state"),
            ]),
        ),
    ]))
    .and_then(|values| {
        // Reject parameters outside the bounded subset.
        generated_focus_params(&values)?;
        Ok(values)
    })
}

fn jnum(j: &Json, key: &str) -> Result<f64> {
    match crate::breakables::jget(j, key) {
        Some(Json::Float(f)) => Ok(*f),
        Some(Json::Int(i)) => Ok(*i as f64),
        _ => Err(format!("'{key}'")),
    }
}

/// `generated_focus_params(values)`.
pub fn generated_focus_params(values: &Json) -> Result<String> {
    let mut fields: Vec<(String, i64)> = Vec::new();
    for name in [
        "hold",
        "start",
        "heal",
        "cancel",
        "finish",
        "first_grace",
        "repeat_grace",
        "attack_recovery",
    ] {
        fields.push((
            format!("{name}_ticks"),
            ticks(jnum(values, &format!("{name}_seconds"))?),
        ));
    }
    fields.push((
        "drain_interval_us".to_string(),
        py_round(jnum(values, "drain_seconds")? * 1_000_000.0),
    ));
    // `cost` and `heal_amount` must be integers, not floats.
    for key in ["cost", "heal_amount"] {
        match crate::breakables::jget(values, key) {
            Some(Json::Int(i)) => fields.push((key.to_string(), *i)),
            _ => return err("focus values exceed the bounded no-charm implementation"),
        }
    }
    if fields.iter().any(|(_, v)| !(*v > 0 && *v <= 65535)) || fields[8].1 < 16667 {
        return err("focus values exceed the bounded no-charm implementation");
    }
    Ok(format!(
        "pub const FOCUS_PARAMS: hk_sim::FocusParams = hk_sim::FocusParams {{{}}};\n",
        fields
            .iter()
            .map(|(k, v)| format!("{k}:{v}"))
            .collect::<Vec<_>>()
            .join(",")
    ))
}

#[allow(dead_code)]
fn _keep() {
    let _ = (jb, jfloats);
}
