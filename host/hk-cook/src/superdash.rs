//! Read the Crystal Heart's timings from the Hero's Superdash PlayMaker FSM (host/superdash.py).
//!
//! HeroController carries a SUPER_DASH_SPEED field, but the FSM never reads it: the travel
//! velocity comes from the FSM's own `Superdash Speed` variable, so that is what is bound.

use crate::breakables::{jl, ks};
use crate::common::Result;
use crate::cook_audio::{jobj, js};
use crate::focus::{assert_that, field, hero_fsm, FsmReader};
use crate::music::value_json;
use crate::pyjson::Json;
use hk_unity::Source;

/// `source_superdash_values(source)`.
pub fn source_superdash_values(source: &Source) -> Result<Json> {
    let (_, fsm) = hero_fsm(source, "Superdash")?;
    let r = FsmReader::new(&fsm)?;
    // Both charges wait on the same variable.
    let mut charge: Vec<hk_unity::Value> = Vec::new();
    for state in ["Ground Charge", "Wall Charge"] {
        for a in r.actions(state, "Wait")? {
            let t = r.scalar(field(&a, "time")?)?;
            if !charge.iter().any(|c| c.py_eq(&t)) {
                charge.push(t);
            }
        }
    }
    assert_that(charge.len() == 1, "the two charge states disagree")?;
    let speed = r.actions("Right", "SetFloatValue")?.remove(0);
    assert_that(
        ks(field(&speed, "floatVariable")?, "name")? == "Current SD Speed"
            && ks(field(&speed, "floatValue")?, "name")? == "Superdash Speed",
        "superdash speed",
    )?;
    let negative = r.actions("Left", "SetFloatValue")?.remove(0);
    assert_that(
        ks(field(&negative, "floatValue")?, "name")? == "Superdash Speed neg",
        "negative speed",
    )?;
    let mut mirror = r.actions("Init", "SetFloatValue")?;
    mirror.extend(r.actions("Init", "FloatMultiply")?);
    let named = |a: &hk_unity::playmaker::Fields, key: &str| -> Option<String> {
        a.iter()
            .find(|(n, _)| n == key)
            .and_then(|(_, v)| v.get("name"))
            .and_then(hk_unity::Value::str)
    };
    assert_that(
        mirror.iter().any(|a| {
            named(a, "floatVariable").as_deref() == Some("Superdash Speed neg")
                && named(a, "floatValue").as_deref() == Some("Superdash Speed")
        }),
        "mirror copy",
    )?;
    let mut negated = false;
    for a in &mirror {
        if a.iter().any(|(n, _)| n == "multiplyBy")
            && named(a, "floatVariable").as_deref() == Some("Superdash Speed neg")
            && r.scalar(field(a, "multiplyBy")?)?
                .py_eq(&hk_unity::Value::F64(-1.0))
        {
            negated = true;
        }
    }
    assert_that(negated, "negation")?;
    let cancelable = r.actions("Dashing", "Wait")?;
    assert_that(
        cancelable.len() == 1 && !field(&cancelable[0], "realTime")?.truthy(),
        "cancelable wait",
    )?;
    let recover = r.actions("Hit Wall", "Wait")?;
    assert_that(recover.len() == 1, "recover wait")?;
    Ok(jobj(vec![
        ("speed", value_json(&r.scalar(field(&speed, "floatValue")?)?)),
        ("charge", value_json(&charge[0])),
        ("cancelable", value_json(&r.scalar(field(&cancelable[0], "time")?)?)),
        ("recover", value_json(&r.scalar(field(&recover[0], "time")?)?)),
        ("source_states", jl(r.state_names()?.iter().map(|s| js(s)).collect())),
        (
            "limitations",
            jl(vec![
                js("Cancelable state NORM CANCEL (jump, dash or attack out of the travel) is not wired until the attack integration"),
                js("SLOPE CANCEL, the Zero Timer that ends a travel stalled against a slope, is not modelled"),
                js("Charge, blast, trail and Hit Wall presentation are not cooked"),
            ]),
        ),
    ]))
}
