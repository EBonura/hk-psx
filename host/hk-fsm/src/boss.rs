//! Boss trees (docs/FSM-V2-DESIGN.md 2.4, 2.5): the HKFSM03 bank of a boss's FSMs plus the scene objects they act on.
//!
//! A boss is a graph of GameObjects, not a body: the cooker lists the boss's subtree (its hit boxes, its sensing ranges,
//! its needle, its effect sprites), numbers them in the order the bank's object handles use, and writes each one's
//! starting state for the runtime's `World` (shared/hk-sim `fsm3_world`). It also resolves what the original's actions
//! name at run time but the tree fixes at build time: another FSM's variable, by (FSM name, variable name).
use crate::cook3::Cooked;
use crate::v3::*;
use hk_format::fsm3::{ops, Bank};
use hk_unity::{Source, Value};
use std::collections::{BTreeMap, HashMap};

/// One scene object of a boss tree, in tree handle order.
#[derive(Clone, Debug)]
pub struct EntRec {
    pub path_id: i64,
    pub name: String,
    /// Handle of the parent among the tree's objects (None for the root or an object outside the tree).
    pub parent: Option<u8>,
    pub active: bool,
    pub pos: [f64; 3],
    pub scale: [f64; 2],
    /// The z Euler angle of the local rotation, degrees.
    pub rot: f64,
    pub mesh: bool,
    pub collider: bool,
    pub trigger: bool,
    pub kinematic: bool,
    /// Has a Rigidbody2D.
    pub body: bool,
    pub gravity: f64,
    pub damage: u32,
    pub box_size: [f64; 2],
    pub box_offset: [f64; 2],
    /// Component class names on the object (for the adapter's reading of what it is).
    pub components: Vec<String>,
}

fn v3(v: Option<&Value>) -> [f64; 3] {
    let g = |k: &str| v.and_then(|v| v.get(k)).and_then(Value::float).unwrap_or(0.0);
    [g("x"), g("y"), g("z")]
}

/// Read the objects of `roots`' subtrees and `extra` from `level`, assigning tree handles through `ctx` (objects the
/// FSMs already name keep the handle they have).
pub fn read_entities(source: &Source, level: &str, roots: &[i64], extra: &[i64], ctx: &mut TreeCtx) -> Result<Vec<EntRec>> {
    let file = source.file(level)?;
    // A GameObject of this file; an id that is another class (a prefab of another file shares ids) reads as nothing.
    let go_tree = |pid: i64| -> Result<Value> {
        let obj = source.object(&file, pid)?;
        if obj.class_id() != 1 {
            return Err("not a GameObject".into());
        }
        Ok(source.read(&obj)?)
    };
    // Walk the subtrees through the Transform children, in file order.
    let mut order: Vec<i64> = vec![];
    let transform_of = |go: &Value| -> Result<i64> {
        for c in go.get("m_Component").and_then(Value::list).ok_or("components")? {
            let id = c.get("component").and_then(|v| v.get("m_PathID")).and_then(Value::int).ok_or("component")?;
            if source.object(&file, id)?.class_id() == 4 {
                return Ok(id);
            }
        }
        Err("object without a Transform".into())
    };
    let mut stack: Vec<i64> = roots.iter().rev().copied().collect();
    while let Some(g) = stack.pop() {
        if order.contains(&g) {
            continue;
        }
        order.push(g);
        let t = source.read(&source.object(&file, transform_of(&go_tree(g)?)?)?)?;
        let mut kids = vec![];
        for ch in t.get("m_Children").and_then(Value::list).unwrap_or(&[]) {
            let tid = ch.get("m_PathID").and_then(Value::int).ok_or("child")?;
            let tt = source.read(&source.object(&file, tid)?)?;
            kids.push(tt.get("m_GameObject").and_then(|v| v.get("m_PathID")).and_then(Value::int).ok_or("child object")?);
        }
        stack.extend(kids.into_iter().rev());
    }
    for e in extra {
        if !order.contains(e) {
            order.push(*e);
        }
    }
    for g in &order {
        ctx.entity(*g);
    }
    // Handles in tree order.
    let mut by_handle: BTreeMap<u8, i64> = BTreeMap::new();
    for (&p, &h) in &ctx.entities {
        by_handle.insert(h, p);
    }
    let handle_of: HashMap<i64, u8> = ctx.entities.clone();
    let mut out = vec![];
    for (&h, &pid) in &by_handle {
        let go = match go_tree(pid) {
            Ok(g) if g.get("m_Name").is_some() => g,
            // A pool prefab or an object of another file: listed so the handle exists, with nothing in it.
            _ => {
                out.push(EntRec {
                    path_id: pid, name: String::new(), parent: None, active: false, pos: [0.0; 3], scale: [1.0; 2], rot: 0.0, mesh: false,
                    collider: false, trigger: false, kinematic: false, body: false, gravity: 0.0, damage: 0, box_size: [0.0; 2], box_offset: [0.0; 2],
                    components: vec![],
                });
                continue;
            }
        };
        let name = go.get("m_Name").and_then(Value::str).unwrap_or_default();
        let active = go.get("m_IsActive").is_some_and(Value::truthy);
        let mut rec = EntRec {
            path_id: pid, name, parent: None, active, pos: [0.0; 3], scale: [1.0; 2], rot: 0.0, mesh: false, collider: false, trigger: false,
            kinematic: false, body: false, gravity: 0.0, damage: 0, box_size: [0.0; 2], box_offset: [0.0; 2], components: vec![],
        };
        for c in go.get("m_Component").and_then(Value::list).unwrap_or(&[]) {
            let id = c.get("component").and_then(|v| v.get("m_PathID")).and_then(Value::int).ok_or("component")?;
            let obj = source.object(&file, id)?;
            let kind = if obj.class_id() == 114 { source.typename(&obj)? } else { class_name(obj.class_id()).to_owned() };
            rec.components.push(kind.clone());
            let tree = source.read(&obj)?;
            match kind.as_str() {
                "Transform" => {
                    rec.pos = v3(tree.get("m_LocalPosition"));
                    let s = v3(tree.get("m_LocalScale"));
                    rec.scale = [s[0], s[1]];
                    let q = tree.get("m_LocalRotation");
                    let qf = |k: &str| q.and_then(|v| v.get(k)).and_then(Value::float).unwrap_or(0.0);
                    rec.rot = 2.0 * qf("z").atan2(qf("w")).to_degrees();
                    if let Some(f) = tree.get("m_Father").and_then(|v| v.get("m_PathID")).and_then(Value::int).filter(|f| *f != 0) {
                        let ft = source.read(&source.object(&file, f)?)?;
                        let pg = ft.get("m_GameObject").and_then(|v| v.get("m_PathID")).and_then(Value::int).ok_or("parent object")?;
                        rec.parent = handle_of.get(&pg).copied();
                    }
                }
                "MeshRenderer" | "SpriteRenderer" => rec.mesh = tree.get("m_Enabled").is_some_and(Value::truthy),
                "BoxCollider2D" | "CircleCollider2D" | "PolygonCollider2D" => {
                    rec.collider |= tree.get("m_Enabled").is_some_and(Value::truthy);
                    rec.trigger |= tree.get("m_IsTrigger").is_some_and(Value::truthy);
                    if kind == "BoxCollider2D" {
                        let s = tree.get("m_Size");
                        let o = tree.get("m_Offset");
                        let g = |v: Option<&Value>, k: &str| v.and_then(|v| v.get(k)).and_then(Value::float).unwrap_or(0.0);
                        rec.box_size = [g(s, "x"), g(s, "y")];
                        rec.box_offset = [g(o, "x"), g(o, "y")];
                    }
                }
                "Rigidbody2D" => {
                    rec.body = true;
                    rec.kinematic = tree.get("m_BodyType").and_then(Value::int) == Some(1) || tree.get("m_IsKinematic").is_some_and(Value::truthy);
                    rec.gravity = tree.get("m_GravityScale").and_then(Value::float).unwrap_or(1.0);
                }
                "DamageHero" => rec.damage = tree.get("damageDealt").and_then(Value::int).unwrap_or(1) as u32,
                _ => {}
            }
        }
        out.push(rec);
        let _ = h;
    }
    Ok(out)
}

fn class_name(id: i32) -> &'static str {
    match id {
        4 => "Transform",
        23 => "MeshRenderer",
        33 => "MeshFilter",
        50 => "Rigidbody2D",
        58 => "CircleCollider2D",
        60 => "PolygonCollider2D",
        61 => "BoxCollider2D",
        82 => "AudioSource",
        198 => "ParticleSystem",
        199 => "ParticleSystemRenderer",
        212 => "SpriteRenderer",
        _ => "Other",
    }
}

/// A boss tree: the bank, the object table and the names its ids stand for.
pub struct BossCooked {
    pub cooked: Cooked,
    pub ents: Vec<EntRec>,
    /// Per FSM, the handle of the object it is attached to.
    pub owners: Vec<u8>,
}

/// Every PlayMakerFSM of one level file: `(owner GameObject, source id, FSM name)`, in file order.
pub fn level_fsms(source: &Source, level: &str) -> Result<Vec<(i64, String, String)>> {
    let file = source.file(level)?;
    let mut out = vec![];
    for info in &file.objects {
        if info.class_id != 114 {
            continue;
        }
        let obj = source.object(&file, info.path_id)?;
        if source.typename(&obj).map_or(true, |t| t != "PlayMakerFSM") {
            continue;
        }
        let tree = source.read(&obj)?;
        let go = tree.get("m_GameObject").and_then(|p| p.pptr()).map(|p| p.1).ok_or("FSM owner")?;
        let name = tree.get("fsm").and_then(|f| f.get("name")).and_then(Value::str).unwrap_or_default();
        out.push((go, obj.sid(), name));
    }
    Ok(out)
}

/// The GameObject of `level` named `name` (it must be the only one).
pub fn find_object(source: &Source, level: &str, name: &str) -> Result<i64> {
    let file = source.file(level)?;
    let mut found = None;
    for info in &file.objects {
        if info.class_id != 1 {
            continue;
        }
        let obj = source.object(&file, info.path_id)?;
        let tree = source.read(&obj)?;
        if tree.get("m_Name").and_then(Value::str).as_deref() == Some(name) && found.replace(info.path_id).is_some() {
            return Err(format!("{level} has two objects named {name}").into());
        }
    }
    found.ok_or_else(|| format!("{level} has no object named {name}").into())
}

/// The PlayerData fields Hornet's FSMs read and write, in the order of the adapter's local reserve (`hornet::HORNET_FIELDS`).
pub const HORNET_FIELDS: [&str; 6] = ["hornetGreenpath", "hornet1Defeated", "disablePause", "dreamerScene1", "hasDash", "equippedCharm_15"];

/// Greenpath's Hornet (Fungus1_04_boss, level133): the boss object, the FSMs of its subtree and the Encounter FSM that
/// wakes it. Returns `(boss object, [(owner object, FSM source id)])`.
pub fn hornet_inputs(source: &Source) -> Result<(i64, Vec<(i64, String)>)> {
    const LEVEL: &str = "level133";
    let boss = find_object(source, LEVEL, "Hornet Boss 1")?;
    let kids = crate::census::hierarchy(source, LEVEL)?;
    let mut tree = vec![];
    let mut stack = vec![boss];
    while let Some(g) = stack.pop() {
        tree.push(g);
        if let Some(c) = kids.get(&g) {
            stack.extend(c.iter().copied());
        }
    }
    let fsms = level_fsms(source, LEVEL)?;
    // The Encounter FSM rides in the same tree: it wakes the boss and shuts the gates.
    let mut sources: Vec<(i64, String)> = vec![];
    for g in &tree {
        for (go, sid, _) in fsms.iter().filter(|(go, _, _)| go == g) {
            sources.push((*go, sid.clone()));
        }
    }
    let encounter: Vec<_> = fsms.iter().filter(|(_, _, n)| n == "Encounter").collect();
    match encounter.as_slice() {
        [(go, sid, _)] => sources.push((*go, sid.clone())),
        _ => return Err(format!("{} Encounter FSMs in {LEVEL}", encounter.len()).into()),
    }
    Ok((boss, sources))
}

/// `Corpse Hornet 1`, the prefab Hornet's death effects spawn (sharedassets105): the object and the FSMs of its subtree.
/// Its Control FSM writes `hornet1Defeated` in its first state and sends `HORNET LEAVE` to the Cloak Corpse in its last.
pub fn hornet_corpse_inputs(source: &Source) -> Result<(i64, Vec<(i64, String)>)> {
    const FILE: &str = "sharedassets105.assets";
    const ROOT: i64 = 30;
    let kids = crate::census::hierarchy(source, FILE)?;
    let mut tree = vec![];
    let mut stack = vec![ROOT];
    while let Some(g) = stack.pop() {
        tree.push(g);
        if let Some(c) = kids.get(&g) {
            stack.extend(c.iter().copied());
        }
    }
    let fsms = level_fsms(source, FILE)?;
    let sources: Vec<(i64, String)> =
        tree.iter().flat_map(|g| fsms.iter().filter(move |(go, _, _)| go == g).map(|(go, sid, _)| (*go, sid.clone()))).collect();
    if sources.is_empty() {
        return Err("Corpse Hornet 1 has no FSM".into());
    }
    Ok((ROOT, sources))
}

/// Cook the FSMs `fsms` (`(owner object, FSM)`) as one boss tree whose scene objects are the subtrees of `roots` plus the
/// objects the FSMs name. `fields` is the PlayerData list the player-data actions resolve against.
pub fn cook_boss_tree(source: &Source, level: &str, fsms: &[(i64, &Value)], roots: &[i64], fields: Option<&[String]>) -> Result<BossCooked> {
    let mut ctx = TreeCtx::new();
    ctx.boss = true;
    let mut stats = Stats::default();
    for (go, _) in fsms {
        ctx.entity(*go);
    }
    let mut irs = vec![];
    for (go, fsm) in fsms {
        let mut ir = Enc::new(&mut ctx, &mut stats, fsm)?.encode()?;
        ir.owner = ctx.entity(*go);
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
    // Resolve PlayerData names, then cross-FSM variables. Names the cooker needs from the string table are read before the
    // entity table adds its own.
    crate::cook3::resolve_player_data_pub(&mut irs, &ctx, fields)?;
    resolve_fsm_vars(&mut irs, &ctx)?;
    check_boss_contracts(&irs, &ctx)?;
    let ents = read_entities(source, level, roots, &[], &mut ctx)?;
    for e in &ents {
        ctx.string(&e.name);
    }
    if ents.len() > 32 {
        return Err(format!("{} scene objects, the runtime's table holds 32", ents.len()).into());
    }
    let senders = irs
        .iter()
        .flat_map(|ir| ir.states.iter().flat_map(|s| s.actions.iter()))
        .filter(|a| ["SendEventByName", "SendEvent", "SendRandomEvent"].iter().any(|n| op_of(n) == Some((a.family, a.op))))
        .count();
    // A send may reach several FSMs, so the queue is wider than the number of sites.
    let queue_cap = u8::try_from((senders * 2).min(32)).unwrap_or(32);
    let (bank, _, _) = serialize_tier(&irs, ctx.events.len(), ctx.entities.len(), ctx.strings.len(), queue_cap, true)?;
    Bank::parse(&bank).map_err(|e| format!("cooked bank failed the guest's own check: {e:?}"))?;
    let mut strings = vec![String::new(); ctx.strings.len()];
    for (s, i) in &ctx.strings {
        strings[*i as usize] = s.clone();
    }
    let owners = irs.iter().map(|ir| ir.owner).collect();
    let names = irs.iter().map(|ir| ir.name.clone()).collect();
    let cooked = Cooked { bank, events: ctx.events.clone(), strings, irs, names };
    Ok(BossCooked { cooked, ents, owners })
}

/// The text a constant operand names, if it is a string constant.
fn const_text<'a>(ir: &FsmIr, ctx: &'a TreeCtx, w: u16) -> Option<&'a str> {
    let (tag, p) = (w >> 13, (w & 0x1fff) as usize);
    if tag != T_CONST {
        return None;
    }
    let id = *ir.pool.get(p)?;
    ctx.strings.iter().find(|(_, i)| **i == id).map(|(s, _)| s.as_str())
}

/// `SetFsm*` / `GetFsm*`: the FSM and variable are named by strings; when exactly one FSM of the tree has that name and a
/// variable of that name, the operands become the FSM's index and the variable's slot. A pair the tree does not have is
/// left as names (the engine owns it).
fn resolve_fsm_vars(irs: &mut [FsmIr], ctx: &TreeCtx) -> Result<()> {
    let kinds: Vec<(&str, u16)> = vec![
        ("SetFsmBool", T_BIT),
        ("SetFsmFloat", T_CELL),
        ("SetFsmString", T_CELL),
        ("SetFsmGameObject", T_OBJ),
        ("GetFsmInt", T_CELL),
        ("GetFsmGameObject", T_OBJ),
    ];
    let table: Vec<(String, Vec<(String, u16, u16)>)> = irs.iter().map(|ir| (ir.name.clone(), ir.vars.clone())).collect();
    for ir in irs.iter_mut() {
        let pool_len = ir.pool.len();
        let _ = pool_len;
        for si in 0..ir.states.len() {
            for ai in 0..ir.states[si].actions.len() {
                let a = ir.states[si].actions[ai].clone();
                let Some((_, tag)) = kinds.iter().find(|(n, _)| op_of(n) == Some((a.family, a.op))) else { continue };
                // [gameObject?, fsmName, variableName, value]
                let b = (a.flags & F_OWNER == 0) as usize;
                let (Some(fsm_name), Some(var_name)) = (const_text(ir, ctx, a.operands[b]), const_text(ir, ctx, a.operands[b + 1])) else {
                    return Err("SetFsm/GetFsm with a variable FSM or variable name".into());
                };
                let mut hits = vec![];
                for (fi, (fname, vars)) in table.iter().enumerate() {
                    // An empty FSM name addresses the object's first FSM, which the cooker cannot name.
                    if (fsm_name.is_empty() || fname == fsm_name) && vars.iter().any(|(n, t, _)| n == var_name && t == tag) {
                        hits.push((fi, vars.iter().find(|(n, _, _)| n == var_name).map(|v| v.2).unwrap_or(0)));
                    }
                }
                if hits.len() == 1 {
                    ir.states[si].actions[ai].operands[b] = opd(T_INLINE, hits[0].0 as u32)?;
                    ir.states[si].actions[ai].operands[b + 1] = opd(T_INLINE, hits[0].1 as u32)?;
                } else if hits.len() > 1 {
                    return Err(format!("{}/{}: FSM {fsm_name:?} variable {var_name:?} is ambiguous ({} FSMs)", ir.name, ir.state_names[si], hits.len()).into());
                } else {
                    // The engine's: 255 says so; the names ride along as string ids.
                    let ids = (ir.pool.iter().position(|v| Some(*v) == ctx.strings.get(fsm_name).copied()), ir.pool.iter().position(|v| Some(*v) == ctx.strings.get(var_name).copied()));
                    let _ = ids;
                    ir.states[si].actions[ai].operands[b] = opd(T_INLINE, 255)?;
                }
            }
        }
    }
    Ok(())
}

/// What the boss tier's handlers rely on and do not re-check at run time.
fn check_boss_contracts(irs: &[FsmIr], ctx: &TreeCtx) -> Result<()> {
    let activate = op_of("ActivateGameObject");
    let ease_ok = |e: i64| matches!(e, 0..=5 | 12..=14 | 21);
    let _ = ease_ok;
    for ir in irs {
        for st in &ir.states {
            for a in &st.actions {
                if Some((a.family, a.op)) == activate {
                    // [gameObject, activate, recursive, resetOnExit]: the kernel has no state-exit hook.
                    let b = (a.flags & F_OWNER == 0) as usize;
                    if a.operands[b + 2] != opd(T_INLINE, 0)? {
                        return Err(format!("{}: ActivateGameObject with resetOnExit", ir.name).into());
                    }
                }
                let _ = ctx;
            }
        }
    }
    Ok(())
}

/// The entity table and the constants the Hornet adapter reads, as the Rust source `data/<label>_fsm3.rs` carries.
pub fn rust_table(label: &str, b: &BossCooked, extra: &str) -> String {
    use std::fmt::Write;
    let q = |f: f64| (f * 65536.0).round() as i32;
    let strings: HashMap<&str, usize> = b.cooked.strings.iter().enumerate().map(|(i, s)| (s.as_str(), i)).collect();
    let mut out = String::new();
    let _ = writeln!(out, "// {label}: the boss tree's bank and scene objects (host/hk-fsm boss.rs).");
    let bank_len = b.cooked.bank.len();
    let _ = writeln!(out, "pub static BANK:[u8;{bank_len}]=*include_bytes!(\"{label}.hkf3\");");
    let runtime = Bank::parse(&b.cooked.bank).map(|x| x.runtime_bytes()).unwrap_or(0);
    let _ = writeln!(out, "pub const RUNTIME_BYTES:usize={runtime};");
    let _ = writeln!(out, "pub static ENTS:[crate::fsm3_world::EntInit;{}]=[", b.ents.len());
    for e in &b.ents {
        let mut flags = 0u8;
        if e.active {
            flags |= 1;
        }
        if e.mesh {
            flags |= 2;
        }
        if e.collider {
            flags |= 4;
        }
        if e.trigger {
            flags |= 8;
        }
        if e.kinematic {
            flags |= 16;
        }
        if e.body {
            flags |= 64;
        }
        let name = strings.get(e.name.as_str()).copied().unwrap_or(0xffff);
        let _ = writeln!(
            out,
            "    crate::fsm3_world::EntInit{{parent:{},flags:{flags},pos:[{},{},{}],scale:[{},{}],rot:{},gravity:{},damage:{},name:{name},box_size:[{},{}],box_offset:[{},{}]}},// {}",
            e.parent.unwrap_or(255),
            q(e.pos[0]),
            q(e.pos[1]),
            q(e.pos[2]),
            q(e.scale[0]),
            q(e.scale[1]),
            q(e.rot),
            q(e.gravity),
            e.damage,
            q(e.box_size[0]),
            q(e.box_size[1]),
            q(e.box_offset[0]),
            q(e.box_offset[1]),
            e.name
        );
    }
    let _ = writeln!(out, "];");
    let _ = writeln!(out, "pub static OWNERS:[u8;{}]={:?};", b.owners.len(), b.owners);
    out.push_str(extra);
    out
}
