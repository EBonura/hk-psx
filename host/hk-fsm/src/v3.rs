//! HKFSM03 host-side encoder (docs/FSM-V2-DESIGN.md section 2.1).
//!
//! This is a measuring encoder: it takes the original PlayMaker data of one tree
//! (every FSM under a HealthManager root), lowers it to the proposed typed
//! bytecode, serializes the bank, decodes the bytes back and compares. There is no
//! guest runtime behind it. Where the design left a field open, the choice made
//! here is written next to the code and reported by `fsm3-sizes`.
use crate::families::{family, FAMILIES};
use hk_unity::Value;
use std::collections::{BTreeMap, HashMap};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

// Operand tags (3 bits) over a 13-bit payload.
pub const T_CONST: u16 = 0;
pub const T_CELL: u16 = 1;
pub const T_BIT: u16 = 2;
pub const T_OBJ: u16 = 3;
pub const T_EVENT: u16 = 4;
pub const T_NONE: u16 = 5;
pub const T_INLINE: u16 = 6;
pub const T_ARRAY: u16 = 7;
const TAGS: [&str; 8] = ["CONST", "CELL", "BIT", "OBJ", "EVENT", "NONE", "INLINE", "ARRAY"];

// Action header flags (high nibble of byte 0).
pub const F_UPDATE: u8 = 1;
pub const F_FIXED: u8 = 2;
pub const F_LATE: u8 = 4;
/// The first FsmOwnerDefault parameter is "use owner" and its operand is omitted.
pub const F_OWNER: u8 = 8;

/// Action types that keep state between ticks, so the kernel re-runs them every tick
/// (UPDATE) and the cook reserves a scratch slot for them. A hand list: PlayMaker has
/// no flag for this and the handler set is not written yet.
const STATEFUL: &[&str] = &[
    "Wait", "WaitRandom", "RandomWait", "Tk2dPlayAnimationWithEvents", "Tk2dWatchAnimationEvents", "iTweenMoveTo",
    "iTweenMoveBy", "iTweenScaleTo", "iTweenScaleBy", "iTweenRotateTo", "iTweenRotateBy", "EaseFloat", "EaseColor",
    "SpawnObjectFromGlobalPoolOverTime", "SpawnObjectFromGlobalPoolOverTimeV2", "FlingObjectsFromGlobalPoolTime",
    "FadeAudio", "WaitForHeroInPosition", "AnimatorPlayStateWait", "NextFrameEvent",
];

pub fn opd(tag: u16, payload: u32) -> Result<u16> {
    if payload > 0x1fff {
        return Err(format!("operand payload {payload} exceeds 13 bits (tag {})", TAGS[tag as usize]).into());
    }
    Ok((tag << 13) | payload as u16)
}

/// (family index, op index within the family) for one action type: the position among
/// the census types of that family in name order.
pub fn op_of(action: &str) -> Option<(u8, u8)> {
    let fam = family(action);
    let f = FAMILIES.iter().position(|x| *x == fam)?;
    let mut names: Vec<&str> = crate::families::all_types().filter(|t| family(t) == fam).collect();
    names.sort();
    Some((f as u8, names.iter().position(|n| *n == action)? as u8))
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActionIr {
    pub family: u8,
    pub op: u8,
    pub flags: u8,
    pub operands: Vec<u16>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StateIr {
    pub transitions: Vec<(u8, u8)>,
    pub actions: Vec<ActionIr>,
}

#[derive(Clone, Debug, Default)]
pub struct FsmIr {
    pub name: String,
    pub start: u8,
    pub states: Vec<StateIr>,
    pub globals: Vec<(u8, u8)>,
    pub arrays: Vec<u16>,
    pub pool: Vec<u32>,
    /// (slot, value) for non-zero initial cells.
    pub init: Vec<(u16, i32)>,
    pub bit_init: Vec<u8>,
    pub obj_init: Vec<u8>,
    pub cells: u16,
    pub bits: u16,
    pub objs: u16,
    pub scratch: u8,
    pub fin_bytes: u8,
    pub events: u16,
}

/// Byte sizes of the sections of one serialized FSM, and of its runtime block.
#[derive(Clone, Debug, Default)]
pub struct Sizes {
    pub dir: usize,
    pub state_table: usize,
    pub transitions: usize,
    pub actions: usize,
    pub arrays: usize,
    pub pool: usize,
    pub init: usize,
    pub pad: usize,
    pub runtime: usize,
}

impl Sizes {
    pub fn bank(&self) -> usize {
        self.dir + self.state_table + self.transitions + self.actions + self.arrays + self.pool + self.init + self.pad
    }
}

#[derive(Default, Debug)]
pub struct Stats {
    /// Per action type: set of base arities seen (operands with the owner operand counted).
    pub arity: BTreeMap<String, BTreeMap<usize, usize>>,
    pub owner_flag: usize,
    pub owner_operand: usize,
    pub specific_owner: usize,
    pub unbound_vars: BTreeMap<String, usize>,
    pub class_mismatch: usize,
    pub float_overflow: usize,
    pub inline_hits: usize,
    pub const_hits: usize,
    pub operands: usize,
    pub actions: usize,
    pub tag_counts: [usize; 8],
    pub unresolved_fsm_refs: usize,
    pub unsupported: BTreeMap<String, usize>,
}

pub struct TreeCtx {
    pub events: Vec<String>,
    event_ix: HashMap<String, u16>,
    pub strings: HashMap<String, u32>,
    pub entities: HashMap<i64, u8>,
}

impl TreeCtx {
    pub fn new() -> Self {
        let mut t = TreeCtx { events: vec![], event_ix: HashMap::new(), strings: HashMap::new(), entities: HashMap::new() };
        t.event("FINISHED");
        t
    }
    pub fn event(&mut self, name: &str) -> u16 {
        if let Some(i) = self.event_ix.get(name) {
            return *i;
        }
        let i = self.events.len() as u16;
        self.events.push(name.to_owned());
        self.event_ix.insert(name.to_owned(), i);
        i
    }
    pub fn string(&mut self, s: &str) -> u32 {
        let n = self.strings.len() as u32;
        *self.strings.entry(s.to_owned()).or_insert(n)
    }
    pub fn entity(&mut self, path_id: i64) -> u8 {
        if path_id == 0 {
            return 0xff;
        }
        let n = self.entities.len() as u8;
        *self.entities.entry(path_id).or_insert(n)
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Class {
    Float,
    Int,
    Bool,
    Str,
    Go,
    Vec2,
    Vec3,
    Quat,
    Color,
    Rect,
    Other,
}

impl Class {
    fn width(self) -> u16 {
        match self {
            Class::Vec2 => 2,
            Class::Vec3 => 3,
            Class::Quat | Class::Color | Class::Rect => 4,
            _ => 1,
        }
    }
    fn tag(self) -> u16 {
        match self {
            Class::Bool => T_BIT,
            Class::Go => T_OBJ,
            _ => T_CELL,
        }
    }
}

fn q16(f: f64, stats: &mut Stats) -> i32 {
    let n = (f * 65536.0).round();
    if n > i32::MAX as f64 || n < i32::MIN as f64 || !n.is_finite() {
        stats.float_overflow += 1;
        return if n > 0.0 { i32::MAX } else { i32::MIN };
    }
    n as i32
}

fn num(v: Option<&Value>) -> f64 {
    v.and_then(|v| match v {
        Value::F32(x) => Some(*x as f64),
        Value::F64(x) => Some(*x),
        Value::Int(x) => Some(*x as f64),
        Value::UInt(x) => Some(*x as f64),
        Value::Bool(b) => Some(*b as i64 as f64),
        _ => None,
    })
    .unwrap_or(0.0)
}

struct Slot {
    class: Class,
    base: u16,
}

/// One FSM being lowered.
pub struct Enc<'a> {
    pub tree: &'a mut TreeCtx,
    pub stats: &'a mut Stats,
    fsm: &'a Value,
    vars: HashMap<String, Slot>,
    ir: FsmIr,
    owner_obj: Option<u16>,
    literal_objs: HashMap<i64, u16>,
    pool_obj: HashMap<(i32, i64), ()>,
}

struct Param {
    name: String,
    kind: i64,
    pos: i64,
    size: i64,
}

struct ActionData<'a> {
    d: &'a Value,
    bytes: Vec<u8>,
}

impl<'a> ActionData<'a> {
    fn list(&self, k: &str) -> &'a [Value] {
        self.d.get(k).and_then(Value::list).unwrap_or(&[])
    }
    fn item(&self, key: &str, pos: i64) -> Result<&'a Value> {
        self.list(key).get(pos as usize).ok_or_else(|| format!("{key}[{pos}] missing").into())
    }
    fn raw(&self, p: &Param) -> Result<&[u8]> {
        let lo = p.pos as usize;
        self.bytes.get(lo..lo + p.size as usize).ok_or_else(|| "byteData range".into())
    }
}

/// Decoded compact scalar: (use variable, variable name, raw value bytes).
fn compact(raw: &[u8], width: usize) -> Result<(bool, String, &[u8])> {
    if raw.len() < width + 1 {
        return Err("truncated compact parameter".into());
    }
    Ok((raw[width] != 0, String::from_utf8_lossy(&raw[width + 1..]).into_owned(), &raw[..width]))
}

impl<'a> Enc<'a> {
    pub fn new(tree: &'a mut TreeCtx, stats: &'a mut Stats, fsm: &'a Value) -> Result<Self> {
        let mut e = Enc { tree, stats, fsm, vars: HashMap::new(), ir: FsmIr::default(), owner_obj: None, literal_objs: HashMap::new(), pool_obj: HashMap::new() };
        e.ir.name = fsm.get("name").and_then(Value::str).unwrap_or_default();
        e.declare()?;
        Ok(e)
    }

    /// Storage map from the declared variables (DESIGN 2.1). Declaration order is the
    /// serialized group order, so the layout is reproducible.
    fn declare(&mut self) -> Result<()> {
        let groups: [(&str, Class); 14] = [
            ("floatVariables", Class::Float),
            ("intVariables", Class::Int),
            ("enumVariables", Class::Int),
            ("stringVariables", Class::Str),
            ("vector2Variables", Class::Vec2),
            ("vector3Variables", Class::Vec3),
            ("colorVariables", Class::Color),
            ("rectVariables", Class::Rect),
            ("quaternionVariables", Class::Quat),
            ("boolVariables", Class::Bool),
            ("gameObjectVariables", Class::Go),
            ("objectVariables", Class::Other),
            ("materialVariables", Class::Other),
            ("textureVariables", Class::Other),
        ];
        let vars = self.fsm.get("variables").ok_or("variables")?;
        for (key, class) in groups.into_iter().chain([("arrayVariables", Class::Other)]) {
            for item in vars.get(key).and_then(Value::list).unwrap_or(&[]) {
                let name = item.get("name").and_then(Value::str).unwrap_or_default();
                let value = item.get("value");
                let base = match class.tag() {
                    T_BIT => {
                        let b = self.ir.bits;
                        self.ir.bits += 1;
                        if num(value) != 0.0 {
                            let (byte, bit) = ((b / 8) as usize, b % 8);
                            if self.ir.bit_init.len() <= byte {
                                self.ir.bit_init.resize(byte + 1, 0);
                            }
                            self.ir.bit_init[byte] |= 1 << bit;
                        }
                        b
                    }
                    T_OBJ => {
                        let o = self.ir.objs;
                        self.ir.objs += 1;
                        let pid = value.and_then(Value::pptr).map(|p| p.1).unwrap_or(0);
                        let e = self.tree.entity(pid);
                        self.ir.obj_init.push(e);
                        o
                    }
                    _ => {
                        let c = self.ir.cells;
                        self.ir.cells += class.width();
                        let comps: Vec<f64> = match class {
                            Class::Vec2 => vec![num(value.and_then(|v| v.get("x"))), num(value.and_then(|v| v.get("y")))],
                            Class::Vec3 => ["x", "y", "z"].iter().map(|k| num(value.and_then(|v| v.get(k)))).collect(),
                            Class::Quat => ["x", "y", "z", "w"].iter().map(|k| num(value.and_then(|v| v.get(k)))).collect(),
                            Class::Color => ["r", "g", "b", "a"].iter().map(|k| num(value.and_then(|v| v.get(k)))).collect(),
                            Class::Rect => ["x", "y", "width", "height"].iter().map(|k| num(value.and_then(|v| v.get(k)))).collect(),
                            Class::Str => {
                                let s = value.and_then(Value::str).unwrap_or_default();
                                vec![if s.is_empty() { 0.0 } else { (self.tree.string(&s) + 1) as f64 / 65536.0 }]
                            }
                            Class::Int | Class::Other => vec![num(value) / 65536.0],
                            _ => vec![num(value)],
                        };
                        for (i, f) in comps.iter().enumerate() {
                            let v = q16(*f, self.stats);
                            if v != 0 {
                                self.ir.init.push((c + i as u16, v));
                            }
                        }
                        c
                    }
                };
                if !name.is_empty() {
                    self.vars.insert(name, Slot { class, base });
                }
            }
        }
        Ok(())
    }

    fn pool(&mut self, v: u32) -> Result<u16> {
        let i = match self.ir.pool.iter().position(|x| *x == v) {
            Some(i) => i,
            None => {
                self.ir.pool.push(v);
                self.ir.pool.len() - 1
            }
        };
        opd(T_CONST, i as u32)
    }

    /// A run of `vals.len()` consecutive pool entries (literal vectors).
    fn pool_run(&mut self, vals: &[u32]) -> Result<u16> {
        let n = vals.len();
        let at = (0..=self.ir.pool.len().saturating_sub(n)).find(|&i| self.ir.pool.len() >= i + n && self.ir.pool[i..i + n] == *vals);
        let at = match at {
            Some(i) => i,
            None => {
                self.ir.pool.extend_from_slice(vals);
                self.ir.pool.len() - n
            }
        };
        opd(T_CONST, at as u32)
    }

    fn var(&mut self, name: &str, expect: Class) -> Result<u16> {
        if name.is_empty() {
            return opd(T_NONE, 0);
        }
        if let Some(s) = self.vars.get(name) {
            let (class, base) = (s.class, s.base);
            let ok = class == expect || matches!((class, expect), (Class::Int, Class::Float) | (Class::Float, Class::Int) | (Class::Other, _));
            if !ok {
                self.stats.class_mismatch += 1;
            }
            return opd(class.tag(), base as u32);
        }
        // A PlayMaker global variable (not declared on the FSM): bind it to a hidden slot.
        *self.stats.unbound_vars.entry(name.to_owned()).or_default() += 1;
        let base = match expect.tag() {
            T_BIT => {
                self.ir.bits += 1;
                self.ir.bits - 1
            }
            T_OBJ => {
                self.ir.objs += 1;
                self.ir.obj_init.push(0xff);
                self.ir.objs - 1
            }
            _ => {
                self.ir.cells += expect.width();
                self.ir.cells - expect.width()
            }
        };
        self.vars.insert(name.to_owned(), Slot { class: expect, base });
        opd(expect.tag(), base as u32)
    }

    fn lit_float(&mut self, f: f64) -> Result<u16> {
        if f.fract() == 0.0 && f.abs() <= 4095.0 {
            self.stats.inline_hits += 1;
            return opd(T_INLINE, (f as i32 as u32) & 0x1fff);
        }
        self.stats.const_hits += 1;
        let v = q16(f, self.stats);
        self.pool(v as u32)
    }
    fn lit_int(&mut self, n: i64) -> Result<u16> {
        if (-4096..=4095).contains(&n) {
            self.stats.inline_hits += 1;
            return opd(T_INLINE, (n as i32 as u32) & 0x1fff);
        }
        self.stats.const_hits += 1;
        self.pool(n as i32 as u32)
    }
    fn string_const(&mut self, s: &str) -> Result<u16> {
        let id = self.tree.string(s);
        self.pool(id)
    }

    fn event_ref(&mut self, name: &str) -> Result<u16> {
        if name.is_empty() {
            return opd(T_NONE, 0);
        }
        let e = self.tree.event(name);
        opd(T_EVENT, e as u32)
    }

    /// An FsmGameObject (or the gameObject of an owner default) as an OBJ operand.
    fn game_object(&mut self, item: &Value) -> Result<u16> {
        let use_var = item.get("useVariable").is_some_and(Value::truthy);
        let name = item.get("name").and_then(Value::str).unwrap_or_default();
        if use_var {
            return self.var(&name, Class::Go);
        }
        let pid = item.get("value").and_then(Value::pptr).map(|p| p.1).unwrap_or(0);
        if pid == 0 {
            return opd(T_NONE, 0);
        }
        // A literal scene object: a hidden OBJ slot initialised to its entity.
        if let Some(s) = self.literal_objs.get(&pid) {
            return opd(T_OBJ, *s as u32);
        }
        let slot = self.ir.objs;
        self.ir.objs += 1;
        let e = self.tree.entity(pid);
        self.ir.obj_init.push(e);
        self.literal_objs.insert(pid, slot);
        opd(T_OBJ, slot as u32)
    }

    fn owner_self(&mut self) -> Result<u16> {
        if let Some(s) = self.owner_obj {
            return opd(T_OBJ, s as u32);
        }
        let slot = self.ir.objs;
        self.ir.objs += 1;
        self.ir.obj_init.push(0);
        self.owner_obj = Some(slot);
        opd(T_OBJ, slot as u32)
    }

    fn scalar(&mut self, class: Class, use_var: bool, name: &str, lit: f64) -> Result<u16> {
        if use_var {
            return self.var(name, class);
        }
        match class {
            Class::Float => self.lit_float(lit),
            Class::Int => self.lit_int(lit as i64),
            Class::Bool => {
                self.stats.inline_hits += 1;
                opd(T_INLINE, (lit != 0.0) as u32)
            }
            _ => Err("scalar class".into()),
        }
    }

    /// One non-array parameter to one operand. `flags`/`owner_first` carry the action
    /// state needed to drop plumbing parameters.
    fn param(&mut self, ad: &ActionData, p: &Param, flags: &mut u8, owner_first: &mut bool) -> Result<Vec<u16>> {
        let one = |x: u16| Ok(vec![x]);
        match p.kind {
            1 => {
                let on = ad.raw(p)?.first().copied().unwrap_or(0) != 0;
                match p.name.as_str() {
                    "everyFrame" => {
                        if on {
                            *flags |= F_UPDATE
                        }
                        Ok(vec![])
                    }
                    "lateUpdate" => {
                        if on {
                            *flags |= F_LATE
                        }
                        Ok(vec![])
                    }
                    "fixedUpdate" => {
                        if on {
                            *flags |= F_FIXED
                        }
                        Ok(vec![])
                    }
                    _ => {
                        self.stats.inline_hits += 1;
                        one(opd(T_INLINE, on as u32)?)
                    }
                }
            }
            0 | 7 => {
                let r = ad.raw(p)?;
                let n = i32::from_le_bytes(r.try_into().map_err(|_| "int size")?);
                one(self.lit_int(n as i64)?)
            }
            2 => {
                let r = ad.raw(p)?;
                let f = f32::from_le_bytes(r.try_into().map_err(|_| "float size")?);
                one(self.lit_float(f as f64)?)
            }
            3 => {
                let s = String::from_utf8_lossy(ad.raw(p)?).into_owned();
                one(self.string_const(&s)?)
            }
            5 | 24 => {
                let key = if p.kind == 5 {
                    ad.item("unityObjectParams", p.pos)?.pptr().unwrap_or((0, 0))
                } else {
                    let it = ad.item("fsmObjectParams", p.pos)?;
                    if it.get("useVariable").is_some_and(Value::truthy) {
                        let name = it.get("name").and_then(Value::str).unwrap_or_default();
                        return one(self.var(&name, Class::Other)?);
                    }
                    it.get("value").and_then(Value::pptr).unwrap_or((0, 0))
                };
                self.pool_obj.insert(key, ());
                let id = self.tree.string(&format!("obj:{}:{}", key.0, key.1));
                one(self.pool(id)?)
            }
            15 | 16 => {
                let (class, w) = if p.kind == 15 { (Class::Float, 4) } else { (Class::Int, 4) };
                let (uv, name, v) = if p.size == 0 {
                    let it = ad.item(if p.kind == 15 { "fsmFloatParams" } else { "fsmIntParams" }, p.pos)?;
                    (it.get("useVariable").is_some_and(Value::truthy), it.get("name").and_then(Value::str).unwrap_or_default(), num(it.get("value")))
                } else {
                    let r = ad.raw(p)?;
                    let (uv, name, v) = compact(r, w)?;
                    let val = if p.kind == 15 { f32::from_le_bytes(v.try_into().unwrap()) as f64 } else { i32::from_le_bytes(v.try_into().unwrap()) as f64 };
                    (uv, name, val)
                };
                one(self.scalar(class, uv, &name, v)?)
            }
            17 => {
                let (uv, name, v) = if p.size == 0 {
                    let it = ad.item("fsmBoolParams", p.pos)?;
                    (it.get("useVariable").is_some_and(Value::truthy), it.get("name").and_then(Value::str).unwrap_or_default(), num(it.get("value")))
                } else {
                    let r = ad.raw(p)?;
                    let (uv, name, v) = compact(r, 1)?;
                    (uv, name, v[0] as f64)
                };
                one(self.scalar(Class::Bool, uv, &name, v)?)
            }
            28 | 37 | 35 => {
                let (class, n) = match p.kind {
                    28 => (Class::Vec3, 3),
                    37 => (Class::Vec2, 2),
                    _ => (Class::Quat, 4),
                };
                let (uv, name, vals): (bool, String, Vec<f64>) = if p.size == 0 {
                    let key = match p.kind {
                        28 => "fsmVector3Params",
                        37 => "fsmVector2Params",
                        _ => "fsmQuaternionParams",
                    };
                    let it = ad.item(key, p.pos)?;
                    let v = it.get("value");
                    let ks: &[&str] = if n == 3 { &["x", "y", "z"] } else if n == 2 { &["x", "y"] } else { &["x", "y", "z", "w"] };
                    (it.get("useVariable").is_some_and(Value::truthy), it.get("name").and_then(Value::str).unwrap_or_default(), ks.iter().map(|k| num(v.and_then(|v| v.get(k)))).collect())
                } else {
                    let r = ad.raw(p)?;
                    let (uv, name, v) = compact(r, 4 * n)?;
                    (uv, name, v.chunks(4).map(|c| f32::from_le_bytes(c.try_into().unwrap()) as f64).collect())
                };
                if uv {
                    return one(self.var(&name, class)?);
                }
                let words: Vec<u32> = vals.iter().map(|f| q16(*f, self.stats) as u32).collect();
                one(self.pool_run(&words)?)
            }
            18 => {
                let it = ad.item("fsmStringParams", p.pos)?;
                let uv = it.get("useVariable").is_some_and(Value::truthy);
                let name = it.get("name").and_then(Value::str).unwrap_or_default();
                if uv {
                    return one(self.var(&name, Class::Str)?);
                }
                let s = it.get("value").and_then(Value::str).unwrap_or_default();
                if p.name == "sendEvent" {
                    return one(self.event_ref(&s)?);
                }
                one(self.string_const(&s)?)
            }
            19 => {
                let it = ad.item("fsmGameObjectParams", p.pos)?;
                one(self.game_object(it)?)
            }
            20 => {
                let it = ad.item("fsmOwnerDefaultParams", p.pos)?;
                let owner = it.get("ownerOption").and_then(Value::int) == Some(0);
                if owner {
                    if *owner_first {
                        *owner_first = false;
                        *flags |= F_OWNER;
                        return Ok(vec![]);
                    }
                    return one(self.owner_self()?);
                }
                self.stats.specific_owner += 1;
                if *owner_first {
                    *owner_first = false;
                }
                let go = it.get("gameObject").ok_or("owner default gameObject")?;
                one(self.game_object(go)?)
            }
            23 => {
                let s = String::from_utf8_lossy(ad.raw(p)?).into_owned();
                one(self.event_ref(&s)?)
            }
            31 => {
                // target enum (+ excludeSelf, sendToChildren), the target object, the FSM name.
                let it = ad.item("fsmEventTargetParams", p.pos)?;
                let target = it.get("target").and_then(Value::int).unwrap_or(0) as u32;
                let ex = num(it.get("excludeSelf").and_then(|v| v.get("value"))) as u32;
                let ch = num(it.get("sendToChildren").and_then(|v| v.get("value"))) as u32;
                let go = it.get("gameObject").and_then(|g| g.get("gameObject"));
                let own = it.get("gameObject").and_then(|g| g.get("ownerOption")).and_then(Value::int);
                let obj = match (own, go) {
                    (Some(1), Some(g)) => self.game_object(g)?,
                    _ => opd(T_NONE, 0)?,
                };
                let fsm_name = it.get("fsmName").and_then(|v| v.get("value")).and_then(Value::str).unwrap_or_default();
                let name_op = if fsm_name.is_empty() { opd(T_NONE, 0)? } else { self.string_const(&fsm_name)? };
                Ok(vec![opd(T_INLINE, target | ex << 4 | ch << 5)?, obj, name_op])
            }
            21 => {
                let it = ad.item("functionCallParams", p.pos)?;
                let m = it.get("FunctionName").and_then(Value::str).unwrap_or_default();
                let method = self.string_const(&m)?;
                let arg = self.lit_int(num(it.get("IntParameter").and_then(|v| v.get("value"))) as i64)?;
                Ok(vec![method, arg])
            }
            39 => {
                let it = ad.item("fsmVarParams", p.pos)?;
                let name = it.get("variableName").and_then(Value::str).unwrap_or_default();
                one(self.var(&name, Class::Other)?)
            }
            k => {
                *self.stats.unsupported.entry(format!("kind {k} ({})", p.name)).or_default() += 1;
                Ok(vec![])
            }
        }
    }

    /// Lower one enabled action. Returns the IR and the number of operands it would
    /// have had with the owner operand present (for the arity check).
    fn action(&mut self, state: &Value, index: usize, short: &str) -> Result<(ActionIr, usize)> {
        let d = state.get("actionData").ok_or("actionData")?;
        let bytes: Vec<u8> = d.get("byteData").and_then(Value::list).unwrap_or(&[]).iter().map(|v| v.int().unwrap_or(0) as u8).collect();
        let ad = ActionData { d, bytes };
        let names = ad.list("paramName");
        let kinds = ad.list("paramDataType");
        let poss = ad.list("paramDataPos");
        let sizes = ad.list("paramByteDataSize");
        let starts = ad.list("actionStartIndex");
        let s = starts[index].int().ok_or("start")? as usize;
        let e = if index + 1 < starts.len() { starts[index + 1].int().ok_or("end")? as usize } else { names.len() };
        let params: Vec<Param> = (s..e)
            .map(|j| Param { name: names[j].str().unwrap_or_default(), kind: kinds[j].int().unwrap_or(-1), pos: poss[j].int().unwrap_or(0), size: sizes[j].int().unwrap_or(0) })
            .collect();
        let (family, op) = op_of(short).ok_or_else(|| format!("unmapped action {short}"))?;
        let mut flags = 0u8;
        let mut owner_first = true;
        let mut operands: Vec<u16> = vec![];
        let mut i = 0;
        let mut had_owner_flag = false;
        while i < params.len() {
            let p = &params[i];
            i += 1;
            if p.kind == 12 {
                let n = ad.list("arrayParamSizes").get(p.pos as usize).and_then(Value::int).ok_or("array size")? as usize;
                let mut elems = vec![];
                for _ in 0..n {
                    let q = params.get(i).ok_or("array element")?;
                    i += 1;
                    let mut f = 0u8;
                    let mut of = false;
                    elems.extend(self.param(&ad, q, &mut f, &mut of)?);
                }
                let at = self.ir.arrays.len();
                self.ir.arrays.push(elems.len() as u16);
                self.ir.arrays.extend(elems);
                operands.push(opd(T_ARRAY, at as u32)?);
                continue;
            }
            let before = flags;
            let ops = self.param(&ad, p, &mut flags, &mut owner_first)?;
            if flags & F_OWNER != 0 && before & F_OWNER == 0 {
                had_owner_flag = true;
            }
            operands.extend(ops);
        }
        if had_owner_flag {
            self.stats.owner_flag += 1;
        }
        if STATEFUL.contains(&short) {
            flags |= F_UPDATE;
        }
        let base = operands.len() + had_owner_flag as usize;
        for o in &operands {
            self.stats.tag_counts[(*o >> 13) as usize] += 1;
        }
        self.stats.operands += operands.len();
        self.stats.actions += 1;
        Ok((ActionIr { family, op, flags, operands }, base))
    }

    pub fn encode(mut self) -> Result<FsmIr> {
        let states = self.fsm.get("states").and_then(Value::list).ok_or("states")?;
        let names: Vec<String> = states.iter().map(|s| s.get("name").and_then(Value::str).unwrap_or_default()).collect();
        if states.len() > 255 {
            return Err("more than 255 states".into());
        }
        let state_ix = |n: &str| -> Result<u8> { names.iter().position(|x| x == n).map(|i| i as u8).ok_or_else(|| format!("missing state {n}").into()) };
        let start = self.fsm.get("startState").and_then(Value::str).unwrap_or_default();
        self.ir.start = state_ix(&start)?;
        let mut max_actions = 0;
        let mut max_scratch = 0;
        for st in states {
            let mut ir = StateIr { transitions: vec![], actions: vec![] };
            for t in st.get("transitions").and_then(Value::list).unwrap_or(&[]) {
                let ev = t.get("fsmEvent").and_then(|e| e.get("name")).and_then(Value::str).unwrap_or_default();
                let to = t.get("toState").and_then(Value::str).unwrap_or_default();
                let e = self.tree.event(&ev);
                if e > 255 {
                    return Err("more than 255 tree events".into());
                }
                ir.transitions.push((e as u8, state_ix(&to)?));
            }
            let mut scratch = 0;
            for (short, idx) in hk_unity::playmaker::enabled(st) {
                let (a, base) = self.action(st, idx, &short)?;
                *self.stats.arity.entry(short.clone()).or_default().entry(base).or_default() += 1;
                if STATEFUL.contains(&short.as_str()) {
                    scratch += 1;
                }
                ir.actions.push(a);
            }
            max_actions = max_actions.max(ir.actions.len());
            max_scratch = max_scratch.max(scratch);
            self.ir.states.push(ir);
        }
        for t in self.fsm.get("globalTransitions").and_then(Value::list).unwrap_or(&[]) {
            let ev = t.get("fsmEvent").and_then(|e| e.get("name")).and_then(Value::str).unwrap_or_default();
            let to = t.get("toState").and_then(Value::str).unwrap_or_default();
            let e = self.tree.event(&ev);
            self.ir.globals.push((e as u8, state_ix(&to)?));
        }
        self.ir.scratch = max_scratch as u8;
        self.ir.fin_bytes = max_actions.div_ceil(8) as u8;
        self.ir.events = self.fsm.get("events").and_then(Value::list).map_or(0, <[_]>::len) as u16;
        Ok(self.ir)
    }
}

// ---- serialization -------------------------------------------------------------

/// Directory record per FSM. The design assumed 16 B; the fields it lists do not fit.
pub const DIR_RECORD: usize = 32;
pub const HEADER: usize = 16;

fn put16(b: &mut Vec<u8>, v: u16) {
    b.extend(v.to_le_bytes());
}
fn put32(b: &mut Vec<u8>, v: u32) {
    b.extend(v.to_le_bytes());
}
fn align(b: &mut Vec<u8>, n: usize, pad: &mut usize) {
    while b.len() % n != 0 {
        b.push(0);
        *pad += 1;
    }
}

/// Serialize a tree. Returns the bank and per-FSM sizes (FSM sections only; the header
/// and event map are in `tree_overhead`).
pub fn serialize(fsms: &[FsmIr], n_events: usize, n_entities: usize, n_strings: usize) -> Result<(Vec<u8>, Vec<Sizes>, usize)> {
    let mut b: Vec<u8> = vec![];
    b.extend(b"HKF3");
    b.extend([1, fsms.len() as u8, n_events.min(255) as u8, n_entities.min(255) as u8]);
    put16(&mut b, n_strings as u16);
    b.extend([0u8; 6]);
    let dir_at = b.len();
    b.resize(dir_at + DIR_RECORD * fsms.len(), 0);
    let mut sizes = vec![Sizes::default(); fsms.len()];
    // Event map: per tree event, a bitmask of the FSMs that handle it (transitions).
    let mask_bytes = fsms.len().div_ceil(8);
    let map_at = b.len();
    b.resize(map_at + n_events * mask_bytes, 0);
    for (f, ir) in fsms.iter().enumerate() {
        for (e, _) in ir.states.iter().flat_map(|s| s.transitions.iter()).chain(ir.globals.iter()) {
            b[map_at + *e as usize * mask_bytes + f / 8] |= 1 << (f % 8);
        }
    }
    let event_map = n_events * mask_bytes;
    let mut pad = 0;
    align(&mut b, 2, &mut pad);
    let tree_overhead = HEADER + event_map + pad;
    for (f, ir) in fsms.iter().enumerate() {
        let sz = &mut sizes[f];
        sz.dir = DIR_RECORD;
        let mut pad = 0;
        align(&mut b, 2, &mut pad);
        let start = b.len();
        // state table: block offset (in u16 units from the FSM start), actions, transitions
        let table_at = b.len();
        b.resize(table_at + 4 * ir.states.len(), 0);
        sz.state_table = 4 * ir.states.len();
        let mut blocks = vec![];
        for st in &ir.states {
            blocks.push(((b.len() - start) / 2) as u16);
            for (e, t) in &st.transitions {
                b.extend([*e, *t]);
                sz.transitions += 2;
            }
            for a in &st.actions {
                b.extend([a.family | a.flags << 4, a.op]);
                sz.actions += 2;
                for o in &a.operands {
                    put16(&mut b, *o);
                    sz.actions += 2;
                }
            }
        }
        for (i, st) in ir.states.iter().enumerate() {
            let at = table_at + 4 * i;
            b[at..at + 2].copy_from_slice(&blocks[i].to_le_bytes());
            b[at + 2] = u8::try_from(st.actions.len()).map_err(|_| "more than 255 actions in a state")?;
            b[at + 3] = u8::try_from(st.transitions.len()).map_err(|_| "more than 255 transitions in a state")?;
        }
        let globals_at = b.len();
        for (e, t) in &ir.globals {
            b.extend([*e, *t]);
        }
        sz.transitions += 2 * ir.globals.len();
        let arrays_at = b.len();
        for w in &ir.arrays {
            put16(&mut b, *w);
        }
        sz.arrays = 2 * ir.arrays.len();
        let mut p = 0;
        align(&mut b, 4, &mut p);
        sz.pad += p;
        let pool_at = b.len();
        for v in &ir.pool {
            put32(&mut b, *v);
        }
        sz.pool = 4 * ir.pool.len();
        let init_at = b.len();
        let wide = ir.cells > 255;
        for (slot, v) in &ir.init {
            if wide {
                put16(&mut b, *slot);
            } else {
                b.push(*slot as u8);
            }
            put32(&mut b, *v as u32);
        }
        sz.init = ir.init.len() * (if wide { 6 } else { 5 });
        // bit and object initial images
        let bi = if ir.bit_init.iter().any(|x| *x != 0) { (ir.bits as usize).div_ceil(8) } else { 0 };
        let mut bimg = ir.bit_init.clone();
        bimg.resize(bi, 0);
        b.extend(&bimg);
        b.extend(&ir.obj_init);
        sz.init += bi + ir.obj_init.len();
        let end = b.len();
        sz.pad += pad;
        // directory record
        let d = dir_at + DIR_RECORD * f;
        let rec = &mut b[d..d + DIR_RECORD];
        rec[0] = ir.states.len() as u8;
        rec[1] = ir.globals.len() as u8;
        rec[2] = ir.start;
        rec[3] = ir.fin_bytes;
        rec[4..6].copy_from_slice(&ir.cells.to_le_bytes());
        rec[6..8].copy_from_slice(&ir.bits.to_le_bytes());
        rec[8..10].copy_from_slice(&ir.objs.to_le_bytes());
        rec[10] = ir.scratch;
        rec[11] = wide as u8;
        for (k, v) in [start, globals_at, arrays_at, pool_at, init_at, end].iter().enumerate() {
            rec[12 + 2 * k..14 + 2 * k].copy_from_slice(&(*v as u16).to_le_bytes());
        }
        rec[24..26].copy_from_slice(&(ir.pool.len() as u16).to_le_bytes());
        rec[26..28].copy_from_slice(&(ir.init.len() as u16).to_le_bytes());
        rec[28..30].copy_from_slice(&(ir.arrays.len() as u16).to_le_bytes());
        // runtime block: FSM header 12 B + cells + bit image + object handles + scratch + finished bitset
        sz.runtime = 12 + 4 * ir.cells as usize + (ir.bits as usize).div_ceil(8) + ir.objs as usize + 4 * ir.scratch as usize + ir.fin_bytes as usize;
    }
    if b.len() > 0xffff {
        return Err(format!("bank {} bytes exceeds 16-bit offsets", b.len()).into());
    }
    Ok((b, sizes, tree_overhead))
}

/// Decode a serialized bank back to IR. `arity(family, op, flags)` is the operand count
/// the op schema (code, not data, in the guest) would report.
pub fn deserialize(b: &[u8], arity: &dyn Fn(u8, u8, u8) -> usize) -> Result<Vec<FsmIr>> {
    if &b[..4] != b"HKF3" {
        return Err("magic".into());
    }
    let n = b[5] as usize;
    let rd = |at: usize| u16::from_le_bytes([b[at], b[at + 1]]) as usize;
    let mut out = vec![];
    for f in 0..n {
        let d = HEADER + DIR_RECORD * f;
        let r = &b[d..d + DIR_RECORD];
        let (states, globals, start) = (r[0] as usize, r[1] as usize, r[2]);
        let u = |k: usize| u16::from_le_bytes([r[k], r[k + 1]]) as usize;
        let (sec, globals_at, arrays_at, pool_at, init_at, end) = (u(12), u(14), u(16), u(18), u(20), u(22));
        let mut ir = FsmIr { start, cells: u(4) as u16, bits: u(6) as u16, objs: u(8) as u16, scratch: r[10], fin_bytes: r[3], ..Default::default() };
        for s in 0..states {
            let t = sec + 4 * s;
            let (off, na, nt) = (rd(t), b[t + 2] as usize, b[t + 3] as usize);
            let mut at = sec + 2 * off;
            let mut st = StateIr { transitions: vec![], actions: vec![] };
            for _ in 0..nt {
                st.transitions.push((b[at], b[at + 1]));
                at += 2;
            }
            for _ in 0..na {
                let (fam, flags, op) = (b[at] & 0xf, b[at] >> 4, b[at + 1]);
                at += 2;
                let k = arity(fam, op, flags);
                let operands = (0..k).map(|i| rd(at + 2 * i) as u16).collect();
                at += 2 * k;
                st.actions.push(ActionIr { family: fam, op, flags, operands });
            }
            ir.states.push(st);
        }
        for g in 0..globals {
            ir.globals.push((b[globals_at + 2 * g], b[globals_at + 2 * g + 1]));
        }
        ir.arrays = (0..u(28)).map(|i| rd(arrays_at + 2 * i) as u16).collect();
        ir.pool = (0..u(24)).map(|i| u32::from_le_bytes(b[pool_at + 4 * i..pool_at + 4 * i + 4].try_into().unwrap())).collect();
        let wide = r[11] != 0;
        let mut at = init_at;
        for _ in 0..u(26) {
            let slot = if wide { let s = rd(at) as u16; at += 2; s } else { let s = b[at] as u16; at += 1; s };
            ir.init.push((slot, i32::from_le_bytes(b[at..at + 4].try_into().unwrap())));
            at += 4;
        }
        let rest = &b[at..end];
        let bi = rest.len().saturating_sub(ir.objs as usize);
        ir.bit_init = rest[..bi].to_vec();
        ir.obj_init = rest[bi..].to_vec();
        out.push(ir);
    }
    Ok(out)
}
