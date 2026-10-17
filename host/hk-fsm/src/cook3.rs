//! The HKFSM03 cooker: original PlayMaker trees to checked banks (docs/FSM-V2-DESIGN.md).
//!
//! `v3.rs` lowers one FSM and serializes a tree. This file adds what a shipped bank needs on top:
//! every action type must have a measured operand count, the actions with a contract the handlers
//! rely on are checked against it, the delayed-event queue is sized from the bank, and the result is
//! parsed back with the guest's own `Bank::parse`.
use crate::v3::*;
use hk_format::fsm3::{ops, Bank};
use hk_unity::Value;

/// A cooked tree: the bank and the names its ids stand for.
pub struct Cooked {
    pub bank: Vec<u8>,
    pub events: Vec<String>,
    pub strings: Vec<String>,
    pub irs: Vec<FsmIr>,
    /// FSM names in bank order.
    pub names: Vec<String>,
}

impl Cooked {
    /// The id of a tree event by name.
    pub fn event(&self, name: &str) -> Option<u8> {
        self.events.iter().position(|e| e == name).map(|i| i as u8)
    }
    pub fn string_id(&self, text: &str) -> Option<u32> {
        self.strings.iter().position(|s| s == text).map(|i| i as u32)
    }
    /// State id by name in FSM `f`.
    pub fn state(&self, f: usize, name: &str) -> Option<u8> {
        self.irs[f].state_names.iter().position(|s| s == name).map(|i| i as u8)
    }
    /// The first slot of a declared variable in FSM `f`.
    pub fn var(&self, f: usize, name: &str) -> Option<u16> {
        self.irs[f].vars.iter().find(|v| v.0 == name).map(|v| v.2)
    }
}

/// Action types that queue an event for later or for another FSM; each site may have one pending.
const SENDERS: &[&str] = &["SendEventByName", "SendEvent", "SendRandomEvent"];

/// Lower and serialize the FSMs of one actor tree. `owners` gives each FSM's GameObject path id (the
/// first entity is the actor itself).
pub fn cook_tree(fsms: &[(i64, &Value)]) -> Result<Cooked> {
    cook_tree_with(fsms, None)
}

/// `cook_tree` with the PlayerData field list (name -> slot is the position) that the player-data actions
/// resolve their field names against.
pub fn cook_tree_with(fsms: &[(i64, &Value)], fields: Option<&[String]>) -> Result<Cooked> {
    let mut ctx = TreeCtx::new();
    let mut stats = Stats::default();
    for (go, _) in fsms {
        ctx.entity(*go);
    }
    let mut irs = vec![];
    let mut names = vec![];
    for (_, fsm) in fsms {
        let ir = Enc::new(&mut ctx, &mut stats, fsm)?.encode()?;
        names.push(ir.name.clone());
        irs.push(ir);
    }
    if !stats.unsupported.is_empty() {
        return Err(format!("unsupported parameter forms: {:?}", stats.unsupported).into());
    }
    if stats.string_const_vars > 0 && stats.string_writers > 0 {
        return Err("a string variable is written, so it cannot be folded to its initial text".into());
    }
    if stats.class_mismatch != 0 || stats.float_overflow != 0 {
        return Err(format!("{} variable class mismatches, {} Q16.16 overflows {:?}", stats.class_mismatch, stats.float_overflow, stats.overflowed).into());
    }
    for ir in &irs {
        for st in &ir.states {
            for a in &st.actions {
                if !ops::known(a.family, a.op) {
                    return Err(format!("FSM {}: an action type without a measured operand count (family {}, op {})", ir.name, a.family, a.op).into());
                }
            }
        }
    }
    check_contracts(&irs, &ctx)?;
    resolve_player_data(&mut irs, &ctx, fields)?;
    let senders = irs
        .iter()
        .flat_map(|ir| ir.states.iter().flat_map(|s| s.actions.iter()))
        .filter(|a| SENDERS.iter().any(|n| op_of(n) == Some((a.family, a.op))))
        .count();
    let queue_cap = u8::try_from(senders.min(16)).unwrap_or(16);
    let (bank, _, _) = serialize(&irs, ctx.events.len(), ctx.entities.len(), ctx.strings.len(), queue_cap)?;
    Bank::parse(&bank).map_err(|e| format!("cooked bank failed the guest's own check: {e:?}"))?;
    let mut strings = vec![String::new(); ctx.strings.len()];
    for (s, i) in &ctx.strings {
        strings[*i as usize] = s.clone();
    }
    Ok(Cooked { bank, events: ctx.events.clone(), strings, irs, names })
}

/// The names behind a cooked tree's ids, as lines the oracle replay reads (`tools/hkref` traces use names):
/// `fsm<TAB>index<TAB>name`, `state<TAB>fsm<TAB>index<TAB>name`, `var<TAB>fsm<TAB>name<TAB>tag<TAB>slot`,
/// `event<TAB>index<TAB>name`.
/// `write_names` for the boss cooker (bin fsm3-boss).
pub fn write_names_pub(root: &std::path::Path, label: &str, c: &Cooked) -> Result<()> {
    write_names(root, label, c)
}

fn write_names(root: &std::path::Path, label: &str, c: &Cooked) -> Result<()> {
    use std::fmt::Write;
    let mut out = String::new();
    for (f, ir) in c.irs.iter().enumerate() {
        writeln!(out, "fsm\t{f}\t{}", ir.name)?;
        for (i, n) in ir.state_names.iter().enumerate() {
            writeln!(out, "state\t{f}\t{i}\t{n}")?;
        }
        for (n, tag, slot) in &ir.vars {
            writeln!(out, "var\t{f}\t{n}\t{tag}\t{slot}")?;
        }
    }
    for (i, e) in c.events.iter().enumerate() {
        writeln!(out, "event\t{i}\t{e}")?;
    }
    for (i, t) in c.strings.iter().enumerate() {
        writeln!(out, "string\t{i}\t{}", t.replace('\t', " ").replace('\n', " "))?;
    }
    std::fs::write(root.join(format!("data/{label}_names.txt")), out)?;
    Ok(())
}

/// The action families a set of cooked trees uses, one line of space-separated family numbers to
/// `data/<label>_families.txt`. `host/code_modules.py` turns it into the family modules a room carries, so a
/// room loads only the action code its enemies' and scripts' banks can reach.
fn write_families(root: &std::path::Path, label: &str, banks: &[&Cooked]) -> Result<()> {
    let mut used = std::collections::BTreeSet::new();
    for c in banks {
        for ir in &c.irs {
            for st in &ir.states {
                for a in &st.actions {
                    used.insert(a.family);
                }
            }
        }
    }
    let line: Vec<String> = used.iter().map(|f| f.to_string()).collect();
    std::fs::write(root.join(format!("data/{label}_families.txt")), line.join(" ") + "\n")?;
    Ok(())
}

/// Per-op contracts the handlers rely on and do not re-check at run time.
fn check_contracts(irs: &[FsmIr], ctx: &TreeCtx) -> Result<()> {
    let walk = op_of("WalkLeftRight");
    let wait = op_of("Wait");
    let text = |ir: &FsmIr, w: u16| -> Option<&str> {
        let (tag, p) = (w >> 13, (w & 0x1fff) as usize);
        if tag != T_CONST {
            return None;
        }
        let id = *ir.pool.get(p)? as usize;
        ctx.strings.iter().find(|(_, i)| **i as usize == id).map(|(s, _)| s.as_str())
    };
    for ir in irs {
        for st in &ir.states {
            for a in &st.actions {
                let key = Some((a.family, a.op));
                if key == walk {
                    // [speed, spriteFacesLeft, groundLayer, turnDelay, walkAnim, turnAnim, startLeft, startRight, keepDirection]
                    let faces_left = a.operands[1] == opd(T_INLINE, 1)?;
                    let strings_ok = text(ir, a.operands[2]) == Some("Terrain") && text(ir, a.operands[4]) == Some("walk") && text(ir, a.operands[5]) == Some("turn");
                    if !faces_left || !strings_ok {
                        return Err("WalkLeftRight art or layer binding differs from the Walker contract".into());
                    }
                    if a.operands[3] >> 13 != T_INLINE {
                        return Err("WalkLeftRight turnDelay must be a literal".into());
                    }
                }
                if key == op_of("FindGameObject") {
                    // [objectName, withTag, store]: only the camera query exists.
                    if text(ir, a.operands[1]) != Some("MainCamera") || text(ir, a.operands[0]) != Some("") {
                        return Err("FindGameObject query other than the main camera".into());
                    }
                }
                if key == op_of("FaceDirection") {
                    // [gameObject?, spriteFacesRight, ...]: the sprite faces left natively.
                    let b = (a.flags & F_OWNER == 0) as usize;
                    if a.operands[b] != opd(T_INLINE, 0)? {
                        return Err("FaceDirection of a sprite that faces right".into());
                    }
                }
                if key == op_of("CheckCollisionSide") || key == op_of("CheckCollisionSideEnter") {
                    // [4 stored outputs, 4 events, otherLayer, otherLayerNumber, ignoreTriggers]
                    let none = opd(T_NONE, 0)?;
                    if a.operands[..4].iter().any(|o| *o != none) || a.operands[4..8].iter().any(|o| o >> 13 != T_EVENT) || a.operands[10] != opd(T_INLINE, 0)? {
                        return Err("CheckCollisionSide with stored outputs, a missing event or trigger filtering".into());
                    }
                }
                if key == op_of("RandomFloat") {
                    let lit = |w: u16| -> Option<i32> {
                        match w >> 13 {
                            T_INLINE => Some(inline_value(w & 0x1fff) << 16),
                            T_CONST => Some(ir.pool[(w & 0x1fff) as usize] as i32),
                            _ => None,
                        }
                    };
                    if let (Some(lo), Some(hi)) = (lit(a.operands[0]), lit(a.operands[1])) {
                        if hi < lo {
                            return Err("RandomFloat with max below min".into());
                        }
                    }
                }
                if key == wait {
                    // [time, finishEvent, realTime, scratch]: the time is a literal under 500 s.
                    let (tag, p) = (a.operands[0] >> 13, (a.operands[0] & 0x1fff) as usize);
                    let secs = match tag {
                        T_INLINE => inline_value(p as u16) as i64,
                        T_CONST => (ir.pool[p] as i32 >> 16) as i64,
                        // A variable's initial value is the cook's to bound; the run-time range is the author's.
                        T_CELL => 0,
                        _ => return Err("Wait time must be a literal or a variable".into()),
                    };
                    if !(0..=500).contains(&secs) {
                        return Err("Wait time outside 0..=500 s".into());
                    }
                }
            }
        }
    }
    Ok(())
}

fn inline_value(p: u16) -> i32 {
    hk_format::fsm3::inline_value(p)
}

/// The Crawler placements the port admits: `(scene, source id) -> (owner GameObject, FSM source)`,
/// from the cooked regions report. The selection is the one `cook_crawlers` made for HKFSM01.
fn crawler_placements(root: &std::path::Path) -> Result<std::collections::BTreeMap<(u16, u32), (i64, String)>> {
    let report: serde_json::Value = serde_json::from_slice(&std::fs::read(root.join("data/regions.json"))?)?;
    let mut selected = std::collections::BTreeMap::new();
    for region in report["regions"].as_array().ok_or("regions")? {
        let scene = u16::try_from(region["scene_id"].as_u64().ok_or("scene")?)?;
        for actor in region["actors"].as_array().ok_or("actors")? {
            if actor["movement_control"]["kind"].as_str() != Some("WalkLeftRight") {
                continue;
            }
            let owner = actor["game_object"].as_i64().ok_or("owner")?;
            let id = u32::try_from(actor["spec_source_id"].as_u64().or_else(|| actor["source"].as_str()?.rsplit_once(':')?.1.parse().ok()).ok_or("source ID")?)?;
            let fsm = actor["movement_control"]["fsm"].as_str().ok_or("FSM source")?.to_owned();
            if let Some(previous) = selected.insert((scene, id), (owner, fsm.clone())) {
                if previous != (owner, fsm) {
                    return Err("conflicting Crawler aliases".into());
                }
            }
        }
    }
    Ok(selected)
}

/// Cook every admitted Crawler's own graph (identical graphs share one bank) to `data/crawler3.hkf3`
/// and `data/crawler_fsm3.rs`.
pub fn cook_crawlers3(root: &std::path::Path) -> Result<()> {
    use std::fmt::Write;
    let source = hk_unity::Source::from_doctor(root)?;
    let selected = crawler_placements(root)?;
    if selected.is_empty() {
        return Err("no admitted Crawler placement".into());
    }
    let mut banks: Vec<Cooked> = vec![];
    let mut offsets: Vec<usize> = vec![];
    let mut bundle = vec![];
    let mut rows = String::new();
    for ((scene, id), (owner, sid)) in &selected {
        let (file, path) = sid.rsplit_once(':').ok_or("FSM ID")?;
        let file = source.file(file)?;
        let object = source.object(&file, path.parse()?)?;
        let tree = source.read(&object)?;
        if tree.get("m_GameObject").and_then(|v| v.get("m_PathID")).and_then(hk_unity::Value::int) != Some(*owner) {
            return Err("Crawler FSM owner mismatch".into());
        }
        let fsm = tree.get("fsm").ok_or("FSM")?;
        if fsm.get("name").and_then(hk_unity::Value::str).as_deref() != Some("Crawler") {
            return Err("Crawler graph name".into());
        }
        let cooked = cook_tree(&[(*owner, fsm)])?;
        let index = match banks.iter().position(|b| b.bank == cooked.bank) {
            Some(i) => i,
            None => {
                offsets.push(bundle.len());
                bundle.extend_from_slice(&cooked.bank);
                banks.push(cooked);
                banks.len() - 1
            }
        };
        writeln!(rows, "({scene},{id},{},{}),", offsets[index], banks[index].bank.len())?;
    }
    let first = &banks[0];
    let ev = |name: &str| first.event(name).ok_or_else(|| format!("Crawler event {name}"));
    let runtime = banks.iter().map(|b| Bank::parse(&b.bank).map(|x| x.runtime_bytes()).unwrap_or(0)).max().unwrap_or(0);
    let table = format!(
        "// Original Crawler graphs as HKFSM03 banks, deduplicated only after exact byte comparison.\n\
         pub static BANK:[u8;{}]=*include_bytes!(\"crawler3.hkf3\");\n\
         pub static GRAPHS:&[(u16,u32,u16,u16)]=&[\n{rows}];\n\
         pub const RUNTIME_BYTES:usize={runtime};\n\
         pub const EV_GO_LEFT:u8={};\npub const EV_GO_RIGHT:u8={};\n",
        bundle.len(),
        ev("GO LEFT")?,
        ev("GO RIGHT")?
    );
    write_names(root, "crawler3", &banks[0])?;
    write_families(root, "crawler3", &banks.iter().collect::<Vec<_>>())?;
    std::fs::write(root.join("data/crawler3.hkf3"), &bundle)?;
    std::fs::write(root.join("data/crawler_fsm3.rs"), table)?;
    println!("Crawler: {} placements, {} distinct graphs, {} bank bytes, {} runtime bytes", selected.len(), banks.len(), bundle.len(), runtime);
    Ok(())
}

/// Cook the original Gruzzer `Bouncer Control` graph (one graph serves every placement) to
/// `data/gruzzer3.hkf3` and `data/gruzzer_fsm3.rs`.
pub fn cook_gruzzer3(root: &std::path::Path) -> Result<()> {
    let source = hk_unity::Source::from_doctor(root)?;
    let file = source.file("level43")?;
    let object = source.object(&file, 6396)?;
    let tree = source.read(&object)?;
    let owner = tree.get("m_GameObject").and_then(|v| v.get("m_PathID")).and_then(hk_unity::Value::int).ok_or("Gruzzer owner")?;
    let fsm = tree.get("fsm").ok_or("original Gruzzer FSM")?;
    if fsm.get("name").and_then(hk_unity::Value::str).as_deref() != Some("Bouncer Control") {
        return Err("Gruzzer graph name".into());
    }
    let cooked = cook_tree(&[(owner, fsm)])?;
    let need = |what: &str, v: Option<u8>| v.ok_or_else(|| format!("Gruzzer {what}"));
    let state = |n: &str| need(n, cooked.state(0, n));
    let var = |n: &str| cooked.var(0, n).ok_or_else(|| format!("Gruzzer variable {n}"));
    let ev = |n: &str| need(n, cooked.event(n));
    let runtime = Bank::parse(&cooked.bank).map_err(|e| format!("{e:?}"))?.runtime_bytes();
    let table = format!(
        "// The original Gruzzer graph as an HKFSM03 bank.\n\
         pub static BANK:[u8;{}]=*include_bytes!(\"gruzzer3.hkf3\");\n\
         pub const RUNTIME_BYTES:usize={runtime};\n\
         pub const STATE_INITIALISE:u8={};\npub const STATE_STOPPED:u8={};\n\
         pub const CELL_ANGLE:u16={};\npub const EV_BONK_UP:u8={};\npub const EV_BONK_RIGHT:u8={};\npub const EV_BONK_DOWN:u8={};\npub const EV_BONK_LEFT:u8={};\n\
         pub const EV_WAKE:u8={};\npub const EV_STOP:u8={};\n",
        cooked.bank.len(),
        state("Initialise")?,
        state("Stopped")?,
        var("Angle")?,
        ev("BONK UP")?,
        ev("BONK RIGHT")?,
        ev("BONK DOWN")?,
        ev("BONK LEFT")?,
        ev("WAKE")?,
        ev("STOP")?
    );
    write_names(root, "gruzzer3", &cooked)?;
    write_families(root, "gruzzer3", &[&cooked])?;
    std::fs::write(root.join("data/gruzzer3.hkf3"), &cooked.bank)?;
    std::fs::write(root.join("data/gruzzer_fsm3.rs"), table)?;
    println!("Gruzzer: {} bank bytes, {} runtime bytes", cooked.bank.len(), runtime);
    Ok(())
}

/// Cook the admitted Vengefly placements: both owned FSMs (`chaser` and `flyer_receive_direction_msg`) as one
/// tree, chaser first. Every placement must yield the same bank. Writes `data/vengefly3.hkf3` and
/// `data/vengefly_fsm3.rs`.
pub fn cook_vengeflies3(root: &std::path::Path) -> Result<()> {
    use hk_unity::Value as V;
    use std::collections::BTreeMap;
    let source = hk_unity::Source::from_doctor(root)?;
    let metadata: serde_json::Value = serde_json::from_slice(&std::fs::read(root.join("data/regions.json"))?)?;
    let mut placements: BTreeMap<(u64, u64), (i64, Vec<String>)> = BTreeMap::new();
    for region in metadata["regions"].as_array().ok_or("regions")? {
        for actor in region["actors"].as_array().ok_or("actors")? {
            if actor["movement_control"]["kind"].as_str() != Some("Vengefly") {
                continue;
            }
            let key = (region["scene_id"].as_u64().ok_or("scene")?, actor["spec_source_id"].as_u64().ok_or("source")?);
            let owner = actor["game_object"].as_i64().ok_or("owner")?;
            let graphs = actor["fsm_ids"].as_array().ok_or("FSM ids")?.iter().map(|id| id.as_str().map(str::to_owned).ok_or("FSM id")).collect::<std::result::Result<Vec<_>, _>>()?;
            if let Some(previous) = placements.insert(key, (owner, graphs.clone())) {
                if previous != (owner, graphs) {
                    return Err("conflicting Vengefly region aliases".into());
                }
            }
        }
    }
    if placements.is_empty() {
        return Err("no admitted Vengefly placement".into());
    }
    let mut bank: Option<Cooked> = None;
    let mut rows = String::new();
    let mut receiver_disabled = 0;
    for ((scene, id), (owner, graphs)) in &placements {
        if graphs.len() != 2 {
            return Err("Vengefly requires both owned FSMs".into());
        }
        let mut named: BTreeMap<String, V> = BTreeMap::new();
        let mut enabled_by_name: BTreeMap<String, i64> = BTreeMap::new();
        for sid in graphs {
            let (file, path) = sid.rsplit_once(':').ok_or("FSM identity")?;
            let file = source.file(file)?;
            let object = source.object(&file, path.parse()?)?;
            let tree = source.read(&object)?;
            if tree.get("m_GameObject").and_then(|v| v.get("m_PathID")).and_then(V::int) != Some(*owner) {
                return Err("Vengefly FSM owner mismatch".into());
            }
            let enabled = tree.get("m_Enabled").and_then(V::int).ok_or("FSM enablement")?;
            let fsm = tree.get("fsm").ok_or("FSM")?.clone();
            let name = fsm.get("name").and_then(V::str).ok_or("FSM name")?;
            // A disabled component never runs and nothing in the port enables it again.
            enabled_by_name.insert(name.clone(), enabled);
            named.insert(name, fsm);
        }
        let (Some(chaser), Some(receiver)) = (named.get("chaser"), named.get("flyer_receive_direction_msg")) else {
            return Err("unexpected Vengefly FSM set".into());
        };
        let cooked = cook_tree(&[(*owner, chaser), (*owner, receiver)])?;
        let mask = (enabled_by_name["chaser"] != 0) as u8 | ((enabled_by_name["flyer_receive_direction_msg"] != 0) as u8) << 1;
        // A disabled chaser never runs. A disabled receiver is the same as an enabled one that nothing messages: it idles
        // until another object sends it a direction, and nothing in the port does.
        if mask & 1 == 0 {
            return Err(format!("Vengefly scene {scene} source {id}: the chaser component is disabled").into());
        }
        if mask != 3 {
            receiver_disabled += 1;
        }
        match &bank {
            None => bank = Some(cooked),
            Some(first) if first.bank != cooked.bank => return Err("Vengefly placements cook to different banks".into()),
            _ => {}
        }
        use std::fmt::Write;
        writeln!(rows, "({scene},{id}),")?;
    }
    let b = bank.ok_or("bank")?;
    let ev = |n: &str| b.event(n).ok_or_else(|| format!("Vengefly event {n}"));
    let st = |n: &str| b.state(0, n).ok_or_else(|| format!("Vengefly state {n}"));
    let sid = |n: &str| b.string_id(n).ok_or_else(|| format!("Vengefly string {n}"));
    let runtime = Bank::parse(&b.bank).map_err(|e| format!("{e:?}"))?.runtime_bytes();
    let table = format!(
        "// The original Vengefly pair (chaser, flyer_receive_direction_msg) as one HKFSM03 bank.\n\
         pub static BANK:[u8;{}]=*include_bytes!(\"vengefly3.hkf3\");\n\
         pub static PLACEMENTS:&[(u16,u32)]=&[\n{}];\n\
         pub const RUNTIME_BYTES:usize={runtime};\n\
         pub const EV_TOOK_DAMAGE:u8={};\npub const EV_GO_UP:u8={};\npub const EV_GO_LEFT:u8={};\npub const EV_GO_RIGHT:u8={};\npub const EV_GO_DOWN:u8={};\n\
         pub const STATE_INITIATE:u8={};\npub const STATE_IDLE:u8={};\npub const STATE_STARTLES:u8={};\npub const STATE_STARTLE:u8={};\n\
         pub const STATE_CHASE_START:u8={};\npub const STATE_IN_SIGHT:u8={};\npub const STATE_OUT_OF_SIGHT:u8={};\npub const STATE_STOP:u8={};\n\
         pub const CLIP_IDLE:u32={};\npub const CLIP_TURN_TO_IDLE:u32={};\npub const CLIP_STARTLE:u32={};\npub const CLIP_CHASE:u32={};\npub const CLIP_TURN_TO_FLY:u32={};\n",
        b.bank.len(),
        rows.lines().map(|l| format!("    {l}\n")).collect::<String>().replace("\n\n", "\n"),
        ev("TOOK DAMAGE")?, ev("GO UP")?, ev("GO LEFT")?, ev("GO RIGHT")?, ev("GO DOWN")?,
        st("Initiate")?, st("Idle")?, st("Startles?")?, st("Startle")?, st("Chase Start")?, st("Chase - In Sight")?, st("Chase - Out of Sight")?, st("Stop")?,
        sid("Idle")?, sid("TurnToIdle")?, sid("Startle")?, sid("Chase")?, sid("TurnToFly")?
    );
    write_names(root, "vengefly3", &b)?;
    write_families(root, "vengefly3", &[&b])?;
    std::fs::write(root.join("data/vengefly3.hkf3"), &b.bank)?;
    std::fs::write(root.join("data/vengefly_fsm3.rs"), table)?;
    println!("Vengefly: {} placements ({receiver_disabled} with the receiver disabled), {} bank bytes, {} runtime bytes", placements.len(), b.bank.len(), runtime);
    Ok(())
}

/// The PlayerData actions name a field; the runtime wants its slot in the save record's reserve.
const PLAYER_DATA: &[&str] = &["SetPlayerDataBool", "SetPlayerDataInt", "GetPlayerDataBool", "GetPlayerDataInt", "PlayerDataBoolTest"];

/// `resolve_player_data` for the boss cooker (boss.rs).
pub fn resolve_player_data_pub(irs: &mut [FsmIr], ctx: &TreeCtx, fields: Option<&[String]>) -> Result<()> {
    resolve_player_data(irs, ctx, fields)
}

fn resolve_player_data(irs: &mut [FsmIr], ctx: &TreeCtx, fields: Option<&[String]>) -> Result<()> {
    let keys: Vec<Option<(u8, u8)>> = PLAYER_DATA.iter().map(|n| op_of(n)).collect();
    for ir in irs.iter_mut() {
        for si in 0..ir.states.len() {
            for ai in 0..ir.states[si].actions.len() {
                let a = &ir.states[si].actions[ai];
                if !keys.contains(&Some((a.family, a.op))) {
                    continue;
                }
                let fields = fields.ok_or("a PlayerData action in a tree cooked without a field list")?;
                // The reads and the test lead with the object they ask when it is not the owner: the field name follows it.
                let has_object = ["GetPlayerDataBool", "GetPlayerDataInt", "PlayerDataBoolTest"].iter().any(|n| op_of(n) == Some((a.family, a.op)));
                let name_at = (has_object && a.flags & F_OWNER == 0) as usize;
                let w = a.operands[name_at];
                let id = if w >> 13 == T_CONST { ir.pool[(w & 0x1fff) as usize] } else { return Err("PlayerData field name is not a constant".into()) };
                let name = ctx.strings.iter().find(|(_, i)| **i == id).map(|(s, _)| s.clone()).ok_or("PlayerData field string")?;
                let slot = fields.iter().position(|f| *f == name).ok_or_else(|| format!("PlayerData field {name} is not in the save record's list"))? as u32;
                let at = match ir.pool.iter().position(|v| *v == slot) {
                    Some(i) => i,
                    None => {
                        ir.pool.push(slot);
                        ir.pool.len() - 1
                    }
                };
                ir.states[si].actions[ai].operands[name_at] = opd(T_CONST, at as u32)?;
            }
        }
    }
    Ok(())
}

/// Cook every acting scene script (the instances `host/cook_scripts.py` selected, in `.hkpsx/script-bank.json`)
/// as HKFSM03 banks: `data/scripts3.hkf3` and `data/script_fsm3.rs`. The PlayerData slot order is the save
/// record's, so the cook takes it from the same file.
pub fn cook_scripts3(root: &std::path::Path) -> Result<()> {
    use hk_unity::Value as V;
    use std::fmt::Write;
    let source = hk_unity::Source::from_doctor(root)?;
    let report: serde_json::Value = serde_json::from_slice(&std::fs::read(root.join(".hkpsx/script-bank.json"))?)?;
    let fields: Vec<String> = report["player_data_fields"].as_array().ok_or("fields")?.iter().map(|v| v.as_str().unwrap_or_default().to_owned()).collect();
    let mut banks: Vec<Cooked> = vec![];
    let mut offsets: Vec<usize> = vec![];
    let mut bundle = vec![];
    let mut rows = String::new();
    let mut max_runtime = 0;
    for inst in report["instances"].as_array().ok_or("instances")? {
        let scene = inst["scene"].as_u64().ok_or("scene")?;
        let (file_name, go) = inst["source"].as_str().ok_or("source")?.rsplit_once(':').ok_or("source id")?;
        let go: i64 = go.parse()?;
        let want = inst["fsm"].as_str().ok_or("fsm name")?;
        let file = source.file(file_name)?;
        let owner = source.read(&source.object(&file, go)?)?;
        let mut found = None;
        for reference in owner.get("m_Component").and_then(V::list).ok_or("components")? {
            let path = reference.get("component").and_then(|v| v.get("m_PathID")).and_then(V::int).ok_or("component path")?;
            let object = source.object(&file, path)?;
            if object.class_id() == 114 && source.typename(&object)? == "PlayMakerFSM" {
                let tree = source.read(&object)?;
                if tree.get("fsm").and_then(|f| f.get("name")).and_then(V::str).as_deref() == Some(want) {
                    if found.is_some() {
                        return Err("two same-named FSMs on one script owner".into());
                    }
                    found = Some(tree);
                }
            }
        }
        let tree = found.ok_or("script FSM not found on its owner")?;
        let cooked = cook_tree_with(&[(go, tree.get("fsm").ok_or("fsm")?)], Some(&fields))?;
        let index = match banks.iter().position(|b| b.bank == cooked.bank) {
            Some(i) => i,
            None => {
                max_runtime = max_runtime.max(Bank::parse(&cooked.bank).map_err(|e| format!("{e:?}"))?.runtime_bytes());
                offsets.push(bundle.len());
                bundle.extend_from_slice(&cooked.bank);
                banks.push(cooked);
                banks.len() - 1
            }
        };
        writeln!(rows, "    ({scene},{},{},{}),", inst["source_id"].as_u64().ok_or("source id")? & 0x7fff_ffff, offsets[index], banks[index].bank.len())?;
    }
    let events = |b: &Cooked, n: &str| b.event(n);
    let _ = events;
    let table = format!(
        "// The acting scene scripts as HKFSM03 banks (host/cook_scripts.py chose them).\n\
         pub static BANK:[u8;{}]=*include_bytes!(\"scripts3.hkf3\");\n\
         /// (scene, owner source id, bank offset, bank length)\n\
         pub static INSTANCES:&[(u16,u32,u16,u16)]=&[\n{rows}];\n\
         pub const RUNTIME_BYTES:usize={};\n",
        bundle.len(),
        max_runtime
    );
    write_families(root, "scripts3", &banks.iter().collect::<Vec<_>>())?;
    std::fs::write(root.join("data/scripts3.hkf3"), &bundle)?;
    std::fs::write(root.join("data/script_fsm3.rs"), table)?;
    println!("Scripts: {} instances, {} distinct banks, {} bank bytes, {} runtime bytes at most", report["instances"].as_array().map_or(0, Vec::len), banks.len(), bundle.len(), max_runtime);
    Ok(())
}

/// Every HKFSM03 bank the guest links, in the order the build needs them (after `cook_scripts.py`, which names the
/// scripts, and the regions report, which places the enemies).
pub fn cook_all(root: &std::path::Path) -> Result<()> {
    cook_crawlers3(root)?;
    cook_gruzzer3(root)?;
    cook_vengeflies3(root)?;
    cook_hornet3(root)?;
    cook_scripts3(root)
}

/// Greenpath's Hornet (Fungus1_04_boss): its fight's FSMs and the Encounter's as one boss tree. Writes `data/hornet3.hkf3`,
/// `data/hornet3_fsm3.rs` and `data/hornet3_names.txt`. The tree reads the Steam level file directly.
pub fn cook_hornet3(root: &std::path::Path) -> Result<()> {
    let source = hk_unity::Source::from_doctor(root)?;
    cook_boss_label(root, &source, "hornet3", "level133", crate::boss::hornet_inputs(&source)?, true)?;
    cook_boss_label(root, &source, "hornet_corpse3", "sharedassets105.assets", crate::boss::hornet_corpse_inputs(&source)?, false)
}

fn cook_boss_label(root: &std::path::Path, source: &hk_unity::Source, label: &str, level: &str, inputs: (i64, Vec<(i64, String)>), fight: bool) -> Result<()> {
    use hk_unity::Value as V;
    let (boss, sources) = inputs;
    let mut values: Vec<(i64, V)> = vec![];
    for (go, sid) in &sources {
        let (file, path) = sid.rsplit_once(':').ok_or("FSM identity")?;
        let file = source.file(file)?;
        let tree = source.read(&source.object(&file, path.parse()?)?)?;
        values.push((*go, tree.get("fsm").ok_or("fsm")?.clone()));
    }
    let fsms: Vec<(i64, &V)> = values.iter().map(|(g, v)| (*g, v)).collect();
    // The PlayerData fields Hornet's FSMs read and write, in the order of the adapter's local reserve.
    let fields: Vec<String> = crate::boss::HORNET_FIELDS.iter().map(|s| s.to_string()).collect();
    let cooked = crate::boss::cook_boss_tree(source, level, &fsms, &[boss], Some(&fields))?;
    std::fs::write(root.join(format!("data/{label}.hkf3")), &cooked.cooked.bank)?;
    std::fs::write(root.join(format!("data/{label}_fsm3.rs")), {
        // The fight's numbers ride with its table; the corpse only needs the event it sends.
        let mut extra = if fight { crate::boss::hornet_extra(source, boss, &cooked.cooked)? } else { crate::boss::corpse_extra(&cooked.cooked) };
        extra.push_str(&crate::boss::clips_table(source, level, &cooked)?);
        crate::boss::rust_table(label, &cooked, &extra)
    })?;
    write_names(root, label, &cooked.cooked)?;
    write_families(root, label, &[&cooked.cooked])
}
