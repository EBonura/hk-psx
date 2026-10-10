//! Waves arenas, as `data/battle_waves.rs`.
//!
//! Crossroads_22's `Battle Scene` fights in four waves of ordinary enemies and
//! kills what stood in the room first. The guest seats only the standing
//! enemies with the scene, then summons each wave's members into the slots the
//! last moment freed (host/battle.py explains the pool). This writes what it
//! needs from the source: the trigger box, the size of every wave, and for every
//! enemy that belongs to the arena the wave it is summoned in or the removal
//! that takes it out.
//!
//! The shape is read from the `Battle Control`, `summon` and `Remove on battle
//! start` FSMs and refused when it is not the audited one, because
//! `hk_sim::waves::WaveArena` is a hand port of exactly that shape.
//!
//! One word per member: the wave in the low three bits (0 for a removed
//! enemy) and `REMOVED` (0x80) for one that `Remove on battle start` kills.

use crate::actors::{self, Row};
use crate::battle::{has_fsm, name_of, parent};
use crate::common::{component_records, err, get, Result};
use crate::false_knight::{arena_trigger_world_box, named};
use crate::recog::Want::{B, F, I, S};
use crate::recog::{check_actions, scalar, state, states, transitions, ActionRow};
use hk_unity::scene::Scene;
use hk_unity::{Source, Value};
use serde_json::Value as J;
use std::path::Path;

/// Waves an arena holds (`hk_sim::waves::MAX_WAVES`).
const MAX_WAVES: usize = 4;
/// Members one wave holds (`hk_sim::waves::MAX_MEMBERS`).
const MAX_MEMBERS: usize = 4;
const REMOVED: u8 = 0x80;

/// `Battle Control`'s states and what each leads to, in the FSM's own order.
#[rustfmt::skip]
const TRANSITIONS: &[(&str, &[(&str, &str)])] = &[
    ("Pause", &[("FINISHED", "Init")]),
    ("Init", &[("ACTIVATE", "Activated"), ("HIT", "Wave 1")]),
    ("Activated", &[]),
    ("Wave 1", &[("END", "Pause W 1")]),
    ("Wave 2", &[("END", "Pause W 2")]),
    ("Wave 3", &[("END", "Pause W 3")]),
    ("Wave 4", &[("END", "End Pause")]),
    ("End Pause", &[("FINISHED", "Blob Open")]),
    ("End", &[]),
    ("Blob Open", &[("FINISHED", "End")]),
    ("Pause W 1", &[("FINISHED", "Wave 2")]),
    ("Pause W 2", &[("FINISHED", "Wave 3")]),
    ("Pause W 3", &[("FINISHED", "Wave 4")]),
];
/// The rows that do not depend on the arena; the waves' sizes are added per scene.
#[rustfmt::skip]
const ACTIONS: &[ActionRow] = &[
    ("Init", "Trigger2dEvent", &[("sendEvent", S("HIT"))]),
    ("Init", "BoolTest", &[("isTrue", S("ACTIVATE")), ("everyFrame", B(false))]),
    ("Activated", "SendEventByName", &[("sendEvent", S("BLOB OPEN Q"))]),
    ("Wave 2", "SendEventByName", &[("sendEvent", S("SUMMON"))]),
    ("Wave 3", "SendEventByName", &[("sendEvent", S("SUMMON"))]),
    ("Wave 4", "SendEventByName", &[("sendEvent", S("SUMMON"))]),
    ("End Pause", "Wait", &[("time", F(1.0))]),
    ("End Pause", "SetBoolValue", &[("boolValue", B(true))]),
    ("Blob Open", "Wait", &[("time", F(2.0))]),
    ("Blob Open", "SendEventByName", &[("sendEvent", S("BLOB OPEN"))]),
    ("End", "SendEventByName", &[("sendEvent", S("BG OPEN"))]),
    ("Pause W 1", "Wait", &[("time", F(0.75))]),
    ("Pause W 2", "Wait", &[("time", F(0.75))]),
    ("Pause W 3", "Wait", &[("time", F(0.75))]),
];
/// `summon`: `Random Pause` then `Enter`, then `Summon` alerts the member.
#[rustfmt::skip]
const SUMMON_ACTIONS: &[ActionRow] = &[
    ("Random Pause", "WaitRandom", &[("timeMin", F(0.25)), ("timeMax", F(1.0))]),
    ("Enter", "RandomFloat", &[("min", F(0.75)), ("max", F(1.2000000476837158))]),
    ("Summon", "SendEventByName", &[("sendEvent", S("ALERT"))]),
];
const SUMMON_TRANSITIONS: &[(&str, &[(&str, &str)])] = &[
    ("Init", &[("SUMMON", "Random Pause")]),
    ("Enter", &[("FINISHED", "Summon")]),
    ("Summon", &[]),
    ("Random Pause", &[("FINISHED", "Enter")]),
];
const REMOVE_TRANSITIONS: &[(&str, &[(&str, &str)])] =
    &[("Idle", &[("BG CLOSE", "Die")]), ("Die", &[])];
/// What `Die` does to an enemy that stood in the room, by kind of enemy: a
/// Spitter dies on the spot (`InstaDeath`, its Geo set by `SetGeoDrop`) and the
/// placed Hatcher destroys itself. Both leave nothing to draw; the guest drops
/// the body without a corpse and without Geo. (A Hatcher Baby sends itself
/// `CENTIPEDE DEATH` instead and is not a removed enemy; battle.rs.)
const REMOVE_DIE: &[&[&str]] = &[
    &["InstaDeath", "SetGeoDrop"],
    &["FindChild", "ActivateGameObject", "DestroyObject"],
];

/// One arena of a scene.
#[derive(Debug, PartialEq)]
pub struct Arena {
    pub scene: usize,
    /// `Battle Scene`'s trigger box in Q16 world units (x0, y0, x1, y1).
    pub trigger: [i64; 4],
    pub sizes: Vec<u8>,
    /// (source id, word) per member, sorted by source id.
    pub members: Vec<(i64, u8)>,
}

fn fsm_of<'a>(records: &[(i64, &str, &'a Value)], name: &str) -> Option<&'a Value> {
    records
        .iter()
        .filter(|r| r.1 == "PlayMakerFSM")
        .filter_map(|r| r.2.get("fsm"))
        .find(|f| f.get("name").and_then(Value::str).as_deref() == Some(name))
}

fn check(
    fsm: &Value,
    start: &str,
    trans: &[(&str, &[(&str, &str)])],
    actions: &[ActionRow],
    who: &str,
) -> Result<()> {
    if get(fsm, "startState")?.str().as_deref() != Some(start) {
        return err(format!("{who} begins in an unsupported state"));
    }
    if fsm
        .get("globalTransitions")
        .and_then(Value::list)
        .is_some_and(|l| !l.is_empty())
    {
        return err(format!("unsupported {who} global transitions"));
    }
    let sts = states(fsm)?;
    if sts.len() != trans.len() {
        return err(format!("unsupported {who} states"));
    }
    for &(name, expected) in trans {
        let ok = state(&sts, name)
            .map(transitions)
            .transpose()?
            .is_some_and(|t| {
                t.len() == expected.len()
                    && t.iter()
                        .zip(expected)
                        .all(|(a, b)| a.0 == b.0 && a.1 == b.1)
            });
        if !ok {
            return err(format!("unsupported {who} transitions: {name}"));
        }
    }
    check_actions(&sts, actions, who)
}

/// The arena of one scene, or None when no enemy of it belongs to one.
pub fn scene_arena(
    sc: &Scene,
    source: &Source,
    catalogue: &actors::Catalogue,
    scene: usize,
) -> Result<Option<Arena>> {
    let rows: Vec<Row> = actors::scan(sc, source, catalogue)?;
    let waved: Vec<&Row> = rows.iter().filter(|r| r.battle_wave != 0).collect();
    if waved.is_empty() {
        return Ok(None);
    }
    let who = &sc.base.name;
    if let Some(r) = waved.iter().find(|r| !r.supported) {
        return err(format!("{who}: wave member {} is not admitted", r.source));
    }
    let top = waved.iter().map(|r| r.battle_wave).max().unwrap() as usize;
    if top > MAX_WAVES {
        return err(format!("{who}: more waves than the guest holds"));
    }
    let mut sizes = vec![0u8; top];
    for r in &waved {
        if r.battle_wave == 0 {
            continue;
        }
        sizes[r.battle_wave as usize - 1] += 1;
    }
    if sizes.iter().any(|&n| n == 0 || n as usize > MAX_MEMBERS) {
        return err(format!(
            "{who}: a wave is empty or larger than the guest holds"
        ));
    }
    if top != MAX_WAVES {
        // The audited FSM has exactly four waves; fewer would be a different shape.
        return err(format!("{who}: not the audited four wave arena"));
    }
    // Battle Control, with each wave's `Battle Enemies` equal to its members.
    let battle = named(sc, "Battle Scene")?;
    let records = component_records(sc, battle);
    let fsm = fsm_of(&records, "Battle Control").ok_or("Battle Scene has no Battle Control")?;
    let size_fields: Vec<[(&str, crate::recog::Want); 1]> =
        sizes.iter().map(|&n| [("intValue", I(n as i64))]).collect();
    let compare: [(&str, crate::recog::Want); 5] = [
        ("integer2", I(0)),
        ("equal", S("END")),
        ("lessThan", S("END")),
        ("greaterThan", S("")),
        ("everyFrame", B(true)),
    ];
    let mut actions: Vec<ActionRow> = ACTIONS.to_vec();
    let names = ["Wave 1", "Wave 2", "Wave 3", "Wave 4"];
    for (i, name) in names.iter().enumerate() {
        actions.push((name, "SetIntValue", &size_fields[i]));
        actions.push((name, "IntCompare", &compare));
    }
    check(fsm, "Pause", TRANSITIONS, &actions, "Battle Control")?;
    // Wave 1 sends two events, BG CLOSE to the gates and SUMMON to its members.
    let sts = states(fsm)?;
    let first = get(state(&sts, "Wave 1").ok_or("missing state")?, "actionData")?;
    let mut sent: Vec<String> = Vec::new();
    for (i, name) in get(first, "actionNames")?
        .list()
        .unwrap_or(&[])
        .iter()
        .enumerate()
    {
        if name.str().is_some_and(|n| n.ends_with("SendEventByName")) {
            let fields =
                hk_unity::playmaker::action_fields(first, i, false).map_err(|e| e.to_string())?;
            if let Some(event) = hk_unity::playmaker::field(&fields, "sendEvent") {
                sent.push(scalar(event).str().unwrap_or_default());
            }
        }
    }
    if sent != ["BG CLOSE", "SUMMON"] {
        return err("unsupported Battle Control events: Wave 1");
    }
    // Every member's summoner, and the removal of everything that stood first.
    let mut members: Vec<(i64, u8)> = Vec::new();
    for r in &rows {
        if r.battle_wave != 0 {
            let group = parent(sc, r.game_object)?.ok_or("wave member without a summoner")?;
            let summon_records = component_records(sc, group);
            let summon = fsm_of(&summon_records, "summon")
                .ok_or_else(|| format!("{who}: {} has no summoner", r.source))?;
            check(summon, "Init", SUMMON_TRANSITIONS, SUMMON_ACTIONS, "summon")?;
            members.push((r.spec_source_id, r.battle_wave));
        } else if r.battle_removable && r.supported {
            let records = component_records(sc, r.game_object);
            let remove = fsm_of(&records, "Remove on battle start")
                .ok_or_else(|| format!("{who}: {} has no removal", r.source))?;
            check(
                remove,
                "Idle",
                REMOVE_TRANSITIONS,
                &[],
                "Remove on battle start",
            )?;
            let sts = states(remove)?;
            let die = get(state(&sts, "Die").ok_or("missing state")?, "actionData")?;
            let enabled: Vec<String> = get(die, "actionNames")?
                .list()
                .unwrap_or(&[])
                .iter()
                .zip(get(die, "actionEnabled")?.list().unwrap_or(&[]))
                .filter(|(_, e)| e.truthy())
                .filter_map(|(n, _)| n.str())
                .map(|n| n.rsplit('.').next().unwrap_or("").to_string())
                .collect();
            if !REMOVE_DIE.iter().any(|shape| *shape == enabled.as_slice()) {
                return err(format!(
                    "{who}: {} is removed in an unsupported way",
                    r.source
                ));
            }
            members.push((r.spec_source_id, REMOVED));
        }
    }
    members.sort();
    let trigger = arena_trigger_world_box(sc)?;
    let q16 = |v: f64| crate::common::py_round(v * 65536.0);
    // `has_fsm` and `name_of` keep the names the doc above uses honest.
    let _ = (has_fsm, name_of);
    Ok(Some(Arena {
        scene,
        trigger: [
            q16(trigger[0]),
            q16(trigger[1]),
            q16(trigger[2]),
            q16(trigger[3]),
        ],
        sizes,
        members,
    }))
}

/// The generated Rust source for the arenas, sorted by scene.
pub fn render(arenas: &[Arena]) -> String {
    let mut out = String::new();
    out.push_str("// Generated by hk-cook battle-waves. Do not edit.\n");
    out.push_str(
        "// Waves arenas: the trigger box (Q16 world units), the size of each wave, and per\n",
    );
    out.push_str(
        "// member (scene, source id, word): the wave in the low three bits, or 0x80 for an\n",
    );
    out.push_str(
        "// enemy the arena's `Remove on battle start` kills. Sorted by scene, then source id.\n",
    );
    out.push_str("pub struct Arena {\n    pub scene: u16,\n    pub trigger: [i32; 4],\n    pub sizes: &'static [u8],\n}\n");
    out.push_str("pub const REMOVED: u8 = 0x80;\n");
    out.push_str("pub const ARENAS: &[Arena] = &[\n");
    for a in arenas {
        let sizes: Vec<String> = a.sizes.iter().map(u8::to_string).collect();
        out.push_str(&format!(
            "    Arena {{ scene: {}, trigger: [{}, {}, {}, {}], sizes: &[{}] }},\n",
            a.scene,
            a.trigger[0],
            a.trigger[1],
            a.trigger[2],
            a.trigger[3],
            sizes.join(", ")
        ));
    }
    out.push_str("];\n");
    out.push_str("pub const MEMBERS: &[(u16, u32, u8)] = &[\n");
    for a in arenas {
        for (id, word) in &a.members {
            out.push_str(&format!("    ({}, {}, {:#04x}),\n", a.scene, id, word));
        }
    }
    out.push_str("];\n");
    out
}

pub fn main(root: &Path, source_dir: Option<&Path>) -> Result<()> {
    let source = match source_dir {
        Some(d) => Source::new(d),
        None => Source::from_doctor(root),
    }
    .map_err(|e| e.to_string())?;
    let text = std::fs::read(root.join("data/regions.json"))
        .map_err(|e| format!("data/regions.json: {e}"))?;
    let report: J = serde_json::from_slice(&text).map_err(|e| format!("data/regions.json: {e}"))?;
    let scenes = report["scenes"].as_array().ok_or("report without scenes")?;
    let catalogue: Vec<(String, [f64; 4], String)> = scenes
        .iter()
        .map(|s| {
            let b: Vec<f64> = s["runtime_bounds"]
                .as_array()
                .map(|a| a.iter().map(|v| v.as_f64().unwrap_or(0.0)).collect())
                .unwrap_or_default();
            if b.len() != 4 {
                return err("scene without runtime_bounds");
            }
            Ok((
                s["file"].as_str().unwrap_or("").to_string(),
                [b[0], b[1], b[2], b[3]],
                s["scene_name"].as_str().unwrap_or("").to_string(),
            ))
        })
        .collect::<Result<_>>()?;
    let mut arenas = Vec::new();
    for (index, s) in scenes.iter().enumerate() {
        let scene = s["scene_id"].as_u64().unwrap_or(index as u64) as usize;
        let sc =
            Scene::new(&source, s["file"].as_str().unwrap_or("")).map_err(|e| e.to_string())?;
        if let Some(arena) = scene_arena(&sc, &source, &catalogue, scene)? {
            arenas.push(arena);
        }
    }
    arenas.sort_by_key(|a| a.scene);
    let path = root.join("data/battle_waves.rs");
    std::fs::write(&path, render(&arenas)).map_err(|e| format!("{}: {e}", path.display()))?;
    println!(
        "battle waves: {} arena(s), {} members",
        arenas.len(),
        arenas.iter().map(|a| a.members.len()).sum::<usize>()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_renders_as_the_sorted_words_the_guest_searches() {
        let arena = Arena {
            scene: 22,
            trigger: [-65536, 131072, 196608, 262144],
            sizes: vec![2, 3, 3, 4],
            members: vec![(1154, REMOVED), (2001, 1), (2002, 4)],
        };
        let text = render(&[arena]);
        assert!(text.contains(
            "Arena { scene: 22, trigger: [-65536, 131072, 196608, 262144], sizes: &[2, 3, 3, 4] }"
        ));
        assert!(text.contains("(22, 1154, 0x80),"));
        assert!(text.contains("(22, 2002, 0x04),"));
    }
}
