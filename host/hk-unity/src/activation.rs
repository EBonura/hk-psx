//! PlayerData gates the original evaluates on scene load, answered at cook
//! time against the fresh save (host/activation.py; its docstring explains
//! the three authored shapes). Only removal is applied, and anything
//! unfamiliar is refused, leaving the object where it is.

use crate::fresh_save::Start;
use crate::playmaker::{action_fields, enabled, field, Fields};
use crate::scene::Scene;
use crate::value::Value;
use crate::Result;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

const GATE_COMPONENTS: &[(&str, bool)] = &[("DeactivateIfPlayerdataFalse", false), ("DeactivateIfPlayerdataTrue", true)];
const REPORTED_COMPONENTS: &[&str] = &["ActivateIfPlayerdataTrue", "DeactivateIfPlayerdataFalseDelayed"];
const GATE_VOCABULARY: &[&str] =
    &["NextFrameEvent", "PlayerDataBoolTest", "BoolTest", "GetOwner", "ActivateGameObject", "ActivateAllChildren", "DestroySelf", "FindChild"];
const ACTIVATION_ACTIONS: &[&str] = &["ActivateGameObject", "ActivateAllChildren"];
const GATE_STATE_LIMIT: usize = 6;

/// This FSM or component is not a recognized gate (or reading it failed).
#[derive(Debug)]
pub struct Refused(pub String);

enum Stop {
    Refused(String),
    Inert(String),
}

fn refuse<T>(msg: impl Into<String>) -> std::result::Result<T, Stop> {
    Err(Stop::Refused(msg.into()))
}

/// One PlayerData bool as a new save starts it, or a refusal.
pub fn fresh_save_bool(playerdata: &HashMap<String, Start>, name: &Value) -> std::result::Result<bool, Refused> {
    let Value::Str(raw) = name else { return Err(Refused("not a field SetupNewPlayerData starts".into())) };
    let name = String::from_utf8_lossy(raw).into_owned();
    match playerdata.get(&name) {
        None => Err(Refused(format!("{name:?} is not a field SetupNewPlayerData starts"))),
        Some(Start::Int(v)) if *v == 0 || *v == 1 => Ok(*v == 1),
        Some(_) => Err(Refused(format!("{name} does not start as a bool"))),
    }
}

fn vocabulary(fsm: &Value) -> HashSet<String> {
    fsm.get("states").and_then(Value::list).unwrap_or(&[]).iter().flat_map(enabled).map(|(k, _)| k).collect()
}

fn gate_like(actions: &HashSet<String>) -> bool {
    actions.contains("PlayerDataBoolTest") && actions.iter().any(|a| ACTIVATION_ACTIONS.contains(&a.as_str()) || a == "DestroySelf")
}

fn within_vocabulary(actions: &HashSet<String>) -> bool {
    actions.iter().all(|a| GATE_VOCABULARY.contains(&a.as_str()))
}

fn outside(actions: &HashSet<String>) -> String {
    let mut v: Vec<&String> = actions.iter().filter(|a| !GATE_VOCABULARY.contains(&a.as_str())).collect();
    v.sort();
    v.into_iter().cloned().collect::<Vec<_>>().join(", ")
}

fn transitions(state: &Value) -> Vec<(Value, Value)> {
    let mut out: Vec<(Value, Value)> = Vec::new();
    for t in state.get("transitions").and_then(Value::list).unwrap_or(&[]) {
        let event = match t.get("fsmEvent") {
            Some(e) if e.is_map() => e.get("name").cloned(),
            _ => t.get("eventName").cloned(),
        };
        let event = event.unwrap_or(Value::Bool(false));
        let to = t.get("toState").cloned().unwrap_or(Value::Bool(false));
        match out.iter_mut().find(|(k, _)| k.py_eq(&event)) {
            Some(slot) => slot.1 = to,
            None => out.push((event, to)),
        }
    }
    out
}

fn branch(name: Option<&Value>) -> std::result::Result<String, Stop> {
    match name {
        Some(Value::Str(s)) if s.is_empty() => Err(Stop::Inert("the fresh-save branch of this gate sends no event".into())),
        Some(Value::Str(s)) => Ok(String::from_utf8_lossy(s).into_owned()),
        _ => refuse("a gate branch is not an event name"),
    }
}

fn resolve(value: Option<&Value>, variables: &BTreeMap<String, Value>) -> std::result::Result<Value, Stop> {
    let Some(v) = value.filter(|v| v.is_map() && v.get("useVariable").is_some()) else {
        return refuse("a gate field is not a compact scalar");
    };
    if !v.get("useVariable").unwrap().truthy() {
        return Ok(v.get("value").cloned().unwrap_or(Value::Bool(false)));
    }
    let name = v.get("name").and_then(Value::str).unwrap_or_default();
    variables.get(&name).cloned().ok_or_else(|| Stop::Refused(format!("a gate reads undeclared variable {name:?}")))
}

/// An FSM's variables as name to (value, overridable by an instance).
fn declared(fsm: &Value) -> std::result::Result<BTreeMap<String, (Value, bool)>, Stop> {
    let mut out: BTreeMap<String, (Value, bool)> = BTreeMap::new();
    let Some(Value::Map(groups)) = fsm.get("variables") else { return refuse("FSM without variables") };
    for (_, group) in groups {
        let Value::List(items) = group else { continue };
        for v in items {
            if !v.is_map() || v.get("name").is_none() {
                continue;
            }
            let name = v.get("name").and_then(Value::str).unwrap_or_default();
            let value = v.get("value").cloned().unwrap_or(Value::Bool(false));
            let none = v.get("value").is_none();
            if let Some((old, _)) = out.get(&name) {
                if !old.py_eq(&value) || none {
                    return refuse(format!("a gate declares {name:?} twice with different values"));
                }
            }
            out.insert(name, (value, v.get("showInInspector").is_some_and(Value::truthy)));
        }
    }
    Ok(out)
}

type Template = (Value, Option<String>);
type TemplateCache = Mutex<HashMap<(String, i64), Arc<Template>>>;
/// The FSM a component runs, its variable values and its template's name.
type Instance = (Value, BTreeMap<String, Value>, Option<String>);

fn templates() -> &'static TemplateCache {
    static T: OnceLock<TemplateCache> = OnceLock::new();
    T.get_or_init(Mutex::default)
}

/// The FSM one PlayMakerFSM component runs and its variable values.
fn instantiate(scene: &Scene, component: &Value) -> std::result::Result<Instance, Stop> {
    let fsm = component.get("fsm").cloned().unwrap_or(Value::Bool(false));
    let reference = component.get("fsmTemplate");
    if !reference.and_then(|r| r.get("m_PathID")).is_some_and(Value::truthy) {
        let vars = declared(&fsm)?.into_iter().map(|(k, (v, _))| (k, v)).collect();
        return Ok((fsm, vars, None));
    }
    let template = (|| -> Result<Arc<Template>> {
        let obj = scene.deref(reference.unwrap())?;
        let key = (obj.file.name.clone(), obj.path_id());
        if let Some(t) = templates().lock().unwrap().get(&key) {
            return Ok(t.clone());
        }
        let asset = scene.source.read(&obj)?;
        let t = Arc::new((asset.get("fsm").cloned().unwrap_or(Value::Bool(false)), asset.get("m_Name").and_then(Value::str)));
        templates().lock().unwrap().insert(key, t.clone());
        Ok(t)
    })()
    .map_err(|e| Stop::Refused(format!("the FSM template could not be read: {e}")))?;
    let overrides = declared(&fsm)?;
    let mut values = BTreeMap::new();
    for (name, (value, exposed)) in declared(&template.0)? {
        let v = if exposed && overrides.contains_key(&name) { overrides[&name].0.clone() } else { value };
        values.insert(name, v);
    }
    Ok((template.0.clone(), values, template.1.clone()))
}

enum Walked {
    Named(BTreeMap<String, bool>),
    Own(bool),
    Children(bool),
    Destroy,
}

fn walk(fsm: &Value, variables: &BTreeMap<String, Value>, pd: &HashMap<String, Start>) -> std::result::Result<(Walked, Vec<String>), Stop> {
    if fsm.get("globalTransitions").is_some_and(Value::truthy) {
        return refuse("a gate with a global transition can leave its result");
    }
    let mut states: Vec<(String, &Value)> = Vec::new();
    for state in fsm.get("states").and_then(Value::list).unwrap_or(&[]) {
        let name = state.get("name").and_then(Value::str).unwrap_or_default();
        if states.iter().any(|(n, _)| *n == name) {
            return refuse("a gate declares one state name twice");
        }
        states.push((name, state));
    }
    if states.len() > GATE_STATE_LIMIT {
        return refuse(format!("{} states is larger than either recognized gate", states.len()));
    }
    let mut name = fsm.get("startState").and_then(Value::str).unwrap_or_default();
    let mut owner_variable: Option<String> = None;
    let mut seen = HashSet::new();
    let mut fields: Vec<String> = Vec::new();
    let mut found: BTreeMap<String, String> = BTreeMap::new();
    let mut maybe: HashSet<String> = HashSet::new();
    loop {
        if !seen.insert(name.clone()) {
            return refuse("a gate loops before it activates anything");
        }
        let Some(&(_, state)) = states.iter().find(|(n, _)| *n == name) else {
            return refuse(format!("a gate branches to unknown state {name:?}"));
        };
        let actions = enabled(state);
        let d = state.get("actionData").ok_or_else(|| Stop::Refused("unreadable: no actionData".into()))?;
        let mut pending: Option<String> = None;
        maybe.clear();
        let mut named: BTreeMap<String, bool> = BTreeMap::new();
        for (position, (kind, index)) in actions.iter().enumerate() {
            let last = position == actions.len() - 1;
            let f: Fields = action_fields(d, *index, true).map_err(|e| Stop::Refused(format!("unreadable: {e}")))?;
            let get = |k: &str| field(&f, k);
            let str_of = |v: Option<&Value>| v.and_then(Value::str);
            if kind == "FindChild" {
                let (target, store) = (get("gameObject"), get("storeResult"));
                let child = match get("childName") {
                    Some(c) if c.is_map() => resolve(Some(c), variables)?,
                    other => other.cloned().unwrap_or(Value::Bool(false)),
                };
                if !target.is_some_and(|t| t.is_map() && t.get("ownerOption").is_some_and(|o| o.py_eq(&Value::Int(0)))) {
                    return refuse("FindChild searches something other than the owner");
                }
                let Value::Str(child) = child else { return refuse("FindChild names no child") };
                if child.is_empty() {
                    return refuse("FindChild names no child");
                }
                let store = match store {
                    Some(s) if s.is_map() && s.get("useVariable").is_some_and(Value::truthy) && s.get("name").is_some_and(Value::truthy) => s,
                    _ => return refuse("FindChild does not store into a variable"),
                };
                found.insert(str_of(store.get("name")).unwrap(), String::from_utf8_lossy(&child).into_owned());
                continue;
            }
            if kind == "ActivateGameObject" && get("gameObject").is_some_and(|g| g.is_map() && g.get("ownerOption").is_some_and(|o| o.py_eq(&Value::Int(1)))) {
                let target = get("gameObject").unwrap().get("gameObject");
                let key = target.and_then(|t| t.get("name")).and_then(Value::str);
                let ok = target.is_some_and(|t| t.is_map() && t.get("useVariable").is_some_and(Value::truthy)) && key.as_ref().is_some_and(|k| found.contains_key(k));
                if !ok {
                    return refuse("a gate activates an object it did not find among its own children");
                }
                if get("everyFrame").is_some_and(Value::truthy) || get("resetOnExit").is_some_and(Value::truthy) {
                    return refuse("a gate child activation is repeated or undone on exit");
                }
                let Value::Bool(activate) = resolve(get("activate"), variables)? else {
                    return refuse("a gate child activation does not carry a plain flag");
                };
                named.insert(found[&key.unwrap()].clone(), activate);
                if last {
                    return Ok((Walked::Named(named), fields));
                }
                continue;
            }
            if !named.is_empty() {
                return refuse("a gate state sets found children and then does something else");
            }
            match kind.as_str() {
                "GetOwner" => {
                    let store = get("storeGameObject");
                    match store {
                        Some(s) if s.is_map() && s.get("useVariable").is_some_and(Value::truthy) && s.get("name").is_some_and(Value::truthy) => {
                            owner_variable = str_of(s.get("name"));
                        }
                        _ => return refuse("GetOwner does not store into a variable"),
                    }
                }
                "NextFrameEvent" => {
                    if !last {
                        return refuse("NextFrameEvent is not the last action of its state");
                    }
                    match get("sendEvent") {
                        Some(Value::Str(s)) if !s.is_empty() => pending = Some(String::from_utf8_lossy(s).into_owned()),
                        _ => return refuse("NextFrameEvent sends no event"),
                    }
                }
                "PlayerDataBoolTest" => {
                    let fieldname = resolve(get("boolName"), variables)?;
                    let value = fresh_save_bool(pd, &fieldname).map_err(|r| Stop::Refused(r.0))?;
                    fields.push(fieldname.str().unwrap_or_default());
                    pending = Some(branch(if value { get("isTrue") } else { get("isFalse") })?);
                }
                "BoolTest" => {
                    if get("everyFrame").is_some_and(Value::truthy) {
                        return refuse("a gate BoolTest repeats every frame");
                    }
                    let value = resolve(get("boolVariable"), variables)?;
                    pending = Some(branch(if value.truthy() { get("isTrue") } else { get("isFalse") })?);
                }
                "ActivateGameObject" => {
                    if !last {
                        return refuse("ActivateGameObject is not the last action of its state");
                    }
                    let target = get("gameObject");
                    if !target.is_some_and(|t| t.is_map() && t.get("ownerOption").is_some_and(|o| o.py_eq(&Value::Int(0)))) {
                        return refuse("a gate activates something other than its own owner");
                    }
                    if get("everyFrame").is_some_and(Value::truthy) || get("resetOnExit").is_some_and(Value::truthy) {
                        return refuse("a gate activation is repeated or undone on exit");
                    }
                    return Ok((Walked::Own(resolve(get("activate"), variables)?.truthy()), fields));
                }
                "ActivateAllChildren" => {
                    if !last {
                        return refuse("ActivateAllChildren is not the last action of its state");
                    }
                    let target = get("gameObject");
                    let ok = target.is_some_and(|t| t.is_map() && t.get("useVariable").is_some_and(Value::truthy))
                        && target.and_then(|t| t.get("name")).and_then(Value::str) == owner_variable;
                    if !ok {
                        return refuse("a gate activates the children of something other than its owner");
                    }
                    let Some(Value::Bool(activate)) = get("activate") else {
                        return refuse("ActivateAllChildren does not carry a plain flag");
                    };
                    return Ok((Walked::Children(*activate), fields));
                }
                "GetPosition" => {
                    if get("everyFrame").is_some_and(Value::truthy) {
                        return refuse("a gate GetPosition repeats every frame");
                    }
                }
                "FloatInRange" => {
                    if get("everyFrame").is_some_and(Value::truthy) {
                        return refuse("a gate FloatInRange repeats every frame");
                    }
                    for key in ["trueEvent", "falseEvent"] {
                        match get(key) {
                            Some(Value::Str(s)) => {
                                if !s.is_empty() {
                                    maybe.insert(String::from_utf8_lossy(s).into_owned());
                                }
                            }
                            _ => return refuse("a runtime test branch is not an event name"),
                        }
                    }
                }
                "DestroySelf" => {
                    if !last {
                        return refuse("DestroySelf is not the last action of its state");
                    }
                    if !maybe.is_empty() {
                        return refuse("a runtime test can leave the destroying state first");
                    }
                    if resolve(get("detachChildren"), variables)?.truthy() {
                        return refuse("a gate that detaches its children leaves them in the world");
                    }
                    return Ok((Walked::Destroy, fields));
                }
                other => return refuse(format!("{other} is not part of a recognized gate")),
            }
            if pending.is_some() {
                break;
            }
        }
        let Some(pending) = pending else {
            return refuse(format!("gate state {name:?} activates nothing and sends no event"));
        };
        if maybe.iter().any(|m| *m != pending) {
            return refuse(format!("a runtime test in gate state {name:?} can leave it by another event"));
        }
        let ts = transitions(state);
        let Some((_, to)) = ts.iter().find(|(e, _)| e.str().as_deref() == Some(pending.as_str()) && matches!(e, Value::Str(_))) else {
            return refuse(format!("gate state {name:?} has no transition for {pending}"));
        };
        name = to.str().unwrap_or_default();
    }
}

/// Which objects of one scene the load-time gates remove.
pub struct Gates {
    pub off: HashSet<i64>,
    /// (source, object id, gate, field)
    pub removed: Vec<(String, i64, String, String)>,
    pub refused: Vec<(String, String, String)>,
    pub inert: Vec<(String, String, String)>,
    pub activates: usize,
    pub other_fsms: usize,
}

impl Gates {
    pub fn new(scene: &Scene) -> Result<Gates> {
        let pd = scene.source.fresh_save()?;
        let mut g = Gates { off: HashSet::new(), removed: Vec::new(), refused: Vec::new(), inert: Vec::new(), activates: 0, other_fsms: 0 };
        let mut children: HashMap<i64, Vec<i64>> = HashMap::new();
        for o in scene.objects.iter().filter(|o| o.typename == "Transform") {
            let f = o.tree.get("m_Father").and_then(|p| p.get("m_PathID")).and_then(Value::int).unwrap_or(0);
            let gid = o.tree.get("m_GameObject").and_then(|p| p.get("m_PathID")).and_then(Value::int).unwrap_or(0);
            children.entry(f).or_default().push(gid);
        }
        for o in &scene.objects {
            let Some(gid) = o.tree.get("m_GameObject").and_then(|p| p.get("m_PathID")).and_then(Value::int) else { continue };
            if !scene.gos.contains_key(&gid) {
                continue;
            }
            let sid = o.id;
            if REPORTED_COMPONENTS.contains(&o.typename.as_str()) {
                g.refuse(scene, sid, &o.typename, "not a load-time removal");
            } else if let Some(&(_, fires_on)) = GATE_COMPONENTS.iter().find(|(n, _)| *n == o.typename) {
                if !o.tree.get("m_Enabled").is_some_and(Value::truthy) {
                    g.refuse(scene, sid, &o.typename, "the component is disabled");
                    continue;
                }
                let name = o.tree.get("boolName").cloned().unwrap_or(Value::Bool(false));
                match fresh_save_bool(pd, &name) {
                    Err(r) => g.refuse(scene, sid, &o.typename, &r.0),
                    Ok(v) if v == fires_on => g.remove(scene, sid, gid, &o.typename, &name.str().unwrap_or_default()),
                    Ok(_) => {}
                }
            } else if o.typename == "PlayMakerFSM" {
                g.fsm(scene, sid, gid, &o.tree, pd, &children);
            }
        }
        Ok(g)
    }

    fn refuse(&mut self, scene: &Scene, sid: i64, gate: &str, reason: &str) {
        self.refused.push((scene.sid(sid), gate.to_string(), reason.to_string()));
    }

    fn remove(&mut self, scene: &Scene, sid: i64, gid: i64, gate: &str, field: &str) {
        self.off.insert(gid);
        self.removed.push((scene.sid(sid), gid, gate.to_string(), field.to_string()));
    }

    fn fsm(&mut self, scene: &Scene, sid: i64, gid: i64, tree: &Value, pd: &HashMap<String, Start>, children: &HashMap<i64, Vec<i64>>) {
        let own = tree.get("fsm").cloned().unwrap_or(Value::Bool(false));
        let actions = vocabulary(&own);
        if !gate_like(&actions) {
            self.other_fsms += 1;
            return;
        }
        let gate = own.get("name").and_then(Value::str).unwrap_or_default();
        if !within_vocabulary(&actions) && !actions.contains("DestroySelf") {
            return self.refuse(scene, sid, &gate, &format!("the FSM does more than gate activation: {}", outside(&actions)));
        }
        if !tree.get("m_Enabled").is_some_and(Value::truthy) {
            return self.refuse(scene, sid, &gate, "the FSM component is disabled");
        }
        if !scene.go_transform.contains_key(&gid) {
            return self.refuse(scene, sid, &gate, "the gate owner has no transform in this scene");
        }
        let outcome = (|| {
            let (fsm, variables, _template) = instantiate(scene, tree)?;
            let actions = vocabulary(&fsm);
            if !gate_like(&actions) || (!within_vocabulary(&actions) && !actions.contains("DestroySelf")) {
                return refuse("the FSM it runs is not the pure gate its own copy is");
            }
            let (scope, fields) = walk(&fsm, &variables, pd)?;
            if !matches!(scope, Walked::Destroy) && !within_vocabulary(&actions) {
                return refuse(format!("the FSM does more than gate activation: {}", outside(&actions)));
            }
            Ok((scope, fields))
        })();
        let (scope, fields) = match outcome {
            Ok(x) => x,
            Err(Stop::Inert(reason)) => {
                self.inert.push((scene.sid(sid), gate, reason));
                return;
            }
            Err(Stop::Refused(r)) => return self.refuse(scene, sid, &gate, &r),
        };
        let field = fields.join(", ");
        let tid = scene.go_transform[&gid];
        let kids_of = || children.get(&tid).cloned().unwrap_or_default();
        match scope {
            Walked::Named(named) => {
                let mut kids: HashMap<String, i64> = HashMap::new();
                for k in kids_of() {
                    if let Some(go) = scene.go(k) {
                        kids.insert(go.get("m_Name").and_then(Value::str).unwrap_or_default(), k);
                    }
                }
                if let Some(child) = named.keys().find(|c| !kids.contains_key(*c)) {
                    let msg = format!("the found child {child:?} is not a direct child");
                    return self.refuse(scene, sid, &gate, &msg);
                }
                for (child, on) in named {
                    if on {
                        self.activates += 1;
                    } else {
                        self.remove(scene, sid, kids[&child], &gate, &field);
                    }
                }
            }
            Walked::Own(activate) => self.apply(scene, sid, gid, vec![gid], activate, &gate, &field),
            Walked::Destroy => self.apply(scene, sid, gid, vec![gid], false, &gate, &field),
            Walked::Children(activate) => {
                let mut v = kids_of();
                v.sort();
                self.apply(scene, sid, gid, v, activate, &gate, &field)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn apply(&mut self, scene: &Scene, sid: i64, _gid: i64, mut targets: Vec<i64>, activate: bool, gate: &str, field: &str) {
        targets.retain(|t| scene.gos.contains_key(t));
        if activate {
            // The other branch of the same gate: counted, never applied.
            self.activates += 1;
            return;
        }
        for t in targets {
            self.remove(scene, sid, t, gate, field);
        }
    }
}
