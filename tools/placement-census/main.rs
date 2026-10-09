//! Placement census: what the original game places in each scene, against what
//! the port spawns.
//!
//! The original side is the ground truth:
//!
//! * the hkref scene survey: the original run to each scene, 60 frames of
//!   settling, then every HealthManager with its world position, activity and
//!   FSM states (`hkref survey`, `actors-all.csv`);
//! * the whole-world import (`.hkpsx/world-import/<file>/`): the scene files'
//!   objects, for each actor's world scale and PersistentBoolItem, and every
//!   DamageHero shape with a collider.
//!
//! The port side is what ships:
//!
//! * the packed metadata banks (`.hkpsx/world-metadata-packed/scene_N.hkwm`),
//!   which the guest reads: every actor placement with its position and flags;
//! * `data/regions.json`, for refusal reasons, colliders and hazards;
//! * the regions' `room.hk` terrain, run through the guest's own spawn solver
//!   (`hk_sim::resolve_actor_spawn` and `Player::step`) for the gravity-bound
//!   families, so a floating or sunken spawn shows;
//! * `data/actor_persistence.rs`, the table that keeps killed enemies dead.
//!
//! Every enemy of the original that is active in its scene lands in exactly one
//! status: `ok`, `snap` (rests where the original does not), `unadmitted` (the
//! cook listed it and refused it a controller; the reason is the cook's own) or
//! `absent` (the cook does not list it). Cooked actors the original does not
//! have active are `extra`. On top of those, each placement the port spawns is
//! held to four contract checks (bank agrees with the cook, facing, scale,
//! persistence) and each DamageHero shape the source can hurt with to one
//! (hazard cooked). The report ends in pass/fail counts per scene and in total.
//!
//! Usage (from the repository root):
//!   cargo run --release --manifest-path tools/placement-census/Cargo.toml -- \
//!       --root <tree holding data/ and .hkpsx/> --survey <hkref run dir>... \
//!       --out census.json --md census.md [--strict]
use flate2::read::GzDecoder;
use hk_format::{Room, WorldMeta};
use hk_sim::{resolve_actor_spawn, Params, Player, ONE as Q};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const SCALE: f64 = 65536.0;
/// The guest's actor pool (`hk_sim::MAX_ACTORS`).
const POOL: usize = hk_sim::MAX_ACTORS;
/// `world_meta::KIND_ACTOR`.
const KIND_ACTOR: u16 = 5;

// --- survey ----------------------------------------------------------------------

struct Og {
    name: String,
    pos: [f64; 3],
    hp: i64,
    active: bool,
    fsms: String,
}

/// One CSV record per line; quoted fields may hold commas but not newlines (the
/// survey writer flattens them).
fn csv_fields(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}

fn load_survey(dirs: &[PathBuf]) -> BTreeMap<String, Vec<Og>> {
    let mut scenes: BTreeMap<String, Vec<Og>> = BTreeMap::new();
    for dir in dirs {
        let path = dir.join("actors-all.csv");
        let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let mut lines = text.lines();
        let head = csv_fields(lines.next().expect("empty survey"));
        let col = |n: &str| {
            head.iter()
                .position(|h| h == n)
                .unwrap_or_else(|| panic!("{} lacks {n}", path.display()))
        };
        let (cs, cn, cx, cy, cz, ch, ca, cf) = (
            col("scene"),
            col("name"),
            col("x"),
            col("y"),
            col("z"),
            col("hp"),
            col("active_in_hierarchy"),
            col("fsms"),
        );
        for l in lines {
            let f = csv_fields(l);
            if f.len() < head.len() {
                continue;
            }
            let num = |i: usize| f[i].parse::<f64>().unwrap_or(0.0);
            scenes.entry(f[cs].clone()).or_default().push(Og {
                name: f[cn].clone(),
                pos: [num(cx), num(cy), num(cz)],
                hp: f[ch].parse().unwrap_or(0),
                active: f[ca] == "1",
                fsms: f[cf].clone(),
            });
        }
    }
    scenes
}

// --- cooked side -----------------------------------------------------------------

struct Cooked {
    /// The numeric part of the source id, which the bank keys on.
    id: u32,
    name: String,
    pos: [f64; 3],
    supported: bool,
    kind: String,
    reason: String,
    /// The first body collider's world box, as `generated_actor_records` reads it.
    box_world: Option<[f64; 4]>,
    control_gravity: Option<f64>,
    visual_scale: Option<[f64; 2]>,
    /// The tk2dSprite component's own `_scale`, which multiplies the transform's.
    sprite_scale: [f64; 2],
    game_object: i64,
}

fn f64s(v: &Value, n: usize) -> Option<Vec<f64>> {
    let a = v.as_array()?;
    if a.len() < n {
        return None;
    }
    a.iter().take(n).map(|x| x.as_f64()).collect()
}

/// One cooked view: where it stands and where its terrain lives.
struct View {
    base: String,
    interaction: [f64; 4],
    collision: [f64; 4],
}

fn inside(b: &[f64; 4], p: [f64; 3]) -> bool {
    b[0] <= p[0] && p[0] <= b[2] && b[1] <= p[1] && p[1] <= b[3]
}

fn load_views(regions: &Value) -> BTreeMap<String, Vec<View>> {
    let mut out: BTreeMap<String, Vec<View>> = BTreeMap::new();
    for r in regions["regions"].as_array().expect("regions") {
        let b4 = |k: &str| {
            f64s(&r[k], 4)
                .map(|v| [v[0], v[1], v[2], v[3]])
                .unwrap_or([0.0; 4])
        };
        out.entry(r["scene_name"].as_str().unwrap_or("").to_string())
            .or_default()
            .push(View {
                base: r["base_path"].as_str().unwrap_or("").to_string(),
                interaction: b4("interaction_bounds"),
                collision: b4("collision_bounds"),
            });
    }
    out
}

fn source_number(source: &str) -> u32 {
    source
        .rsplit(':')
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(u32::MAX)
}

fn load_cooked(regions: &Value) -> BTreeMap<String, Vec<Cooked>> {
    let mut by_scene: BTreeMap<String, BTreeMap<String, Cooked>> = BTreeMap::new();
    for r in regions["regions"].as_array().expect("regions") {
        let scene = r["scene_name"].as_str().unwrap_or("").to_string();
        for a in r["actors"].as_array().into_iter().flatten() {
            let source = a["source"].as_str().unwrap_or("").to_string();
            let entry = by_scene.entry(scene.clone()).or_default();
            if entry.contains_key(&source) {
                continue;
            }
            let control = &a["movement_control"];
            let kind = control["kind"].as_str().unwrap_or("").to_string();
            let reason = ["movement_error", "pending_movement_error"]
                .iter()
                .find_map(|k| a[*k].as_str())
                .or_else(|| {
                    a["limitations"]
                        .as_array()
                        .and_then(|l| l.first())
                        .and_then(|v| v.as_str())
                })
                .unwrap_or("")
                .to_string();
            let box_world = a["colliders"].as_array().and_then(|cs| {
                let wanted = control["trigger_body"].as_bool().unwrap_or(false);
                cs.iter()
                    .find(|c| {
                        c["trigger"].as_bool().unwrap_or(false) == wanted
                            && c.get("bounds").is_some()
                    })
                    .and_then(|c| f64s(&c["bounds"], 4))
            });
            entry.insert(
                source.clone(),
                Cooked {
                    id: a["spec_source_id"]
                        .as_u64()
                        .map(|v| v as u32)
                        .unwrap_or_else(|| source_number(&source)),
                    name: a["name"].as_str().unwrap_or("").to_string(),
                    pos: f64s(&a["position"], 3)
                        .map(|v| [v[0], v[1], v[2]])
                        .unwrap_or([0.0; 3]),
                    supported: a["movement_supported"].as_bool().unwrap_or(false),
                    kind,
                    reason,
                    box_world: box_world.map(|b| [b[0], b[1], b[2], b[3]]),
                    control_gravity: control["parameters"]["gravity_scale"].as_f64(),
                    visual_scale: f64s(&a["visual_scale"], 2).map(|v| [v[0], v[1]]),
                    sprite_scale: [
                        a["tk2dSprite"]["_scale"]["x"].as_f64().unwrap_or(1.0),
                        a["tk2dSprite"]["_scale"]["y"].as_f64().unwrap_or(1.0),
                    ],
                    game_object: a["game_object"].as_i64().unwrap_or(0),
                },
            );
        }
    }
    by_scene
        .into_iter()
        .map(|(k, v)| (k, v.into_values().collect()))
        .collect()
}

// --- packed banks ----------------------------------------------------------------

/// One actor object of a scene's metadata bank.
#[derive(Clone, Copy)]
struct BankActor {
    flags: u16,
    x: i32,
    y: i32,
}

fn load_bank(dir: &Path, scene_id: usize) -> Option<HashMap<u32, BankActor>> {
    let bytes = fs::read(dir.join(format!("scene_{scene_id}.hkwm"))).ok()?;
    let bank = WorldMeta::parse(&bytes).ok()?;
    let mut out = HashMap::new();
    for r in 0..bank.region_count() {
        let region = bank.region(r)?;
        for o in region.objects().flatten() {
            if o.kind() != KIND_ACTOR {
                continue;
            }
            // Supported placements key on their scene-unique state id (an object
            // merged in from an additive scene has a shifted one); refused ones
            // carry no state and key on the source id.
            let id = if o.flags() & 64 != 0 {
                o.state_id()
            } else {
                o.source_id()
            };
            out.entry(id).or_insert(BankActor {
                flags: o.flags(),
                x: o.extra(1),
                y: o.extra(2),
            });
        }
    }
    Some(out)
}

// --- original scene files --------------------------------------------------------

/// What the census reads of one scene's whole-world import.
struct Import {
    objects: HashMap<i64, Value>,
    /// GameObject path id to its Transform's path id.
    transform_of: HashMap<i64, i64>,
    /// GameObject path id to its PersistentBoolItem's (semi, dontSave).
    pbi_of: HashMap<i64, (bool, bool)>,
    hm_gos: BTreeSet<i64>,
    /// Enabled DamageHero components: (GameObject path id, source id).
    damage_hero: Vec<(i64, String)>,
    /// GameObject source id to its colliders' (active, enabled).
    colliders: HashMap<String, Vec<(bool, bool)>>,
    /// GameObject path id to its own `m_IsActive`.
    go_active: HashMap<i64, bool>,
    /// Every pickup-family FSM: (GameObject path id, FSM name).
    pickups: Vec<(i64, String)>,
}

/// The FSMs that hand the Knight something in the slice (host/pickups.py).
const CHEST: &str = "Chest Control";
const PICKUP_FSMS: &[&str] = &[
    "Chest Control",
    "Shiny Control",
    "Heart Container Control",
    "Vessel Fragment Control",
];

fn gz_json(path: &Path) -> Option<Value> {
    let f = fs::File::open(path).ok()?;
    let mut text = String::new();
    GzDecoder::new(std::io::BufReader::new(f))
        .read_to_string(&mut text)
        .ok()?;
    serde_json::from_str(&text).ok()
}

fn load_import(dir: &Path, file: &str) -> Option<Import> {
    let base = dir.join(file);
    let comps = gz_json(&base.join("components.json.gz"))?;
    let geo = gz_json(&base.join("geometry.json.gz"))?;
    let mut imp = Import {
        objects: HashMap::new(),
        transform_of: HashMap::new(),
        pbi_of: HashMap::new(),
        hm_gos: BTreeSet::new(),
        damage_hero: Vec::new(),
        colliders: HashMap::new(),
        go_active: HashMap::new(),
        pickups: Vec::new(),
    };
    for o in comps["objects"].as_array()? {
        let id = source_number(o["source"].as_str().unwrap_or("")) as i64;
        let ty = o["type"].as_str().unwrap_or("");
        let go = o["data"]["m_GameObject"]["m_PathID"].as_i64().unwrap_or(0);
        match ty {
            "Transform" => {
                imp.transform_of.insert(go, id);
                imp.objects.insert(id, o.clone());
            }
            "PersistentBoolItem" => {
                imp.pbi_of.insert(
                    go,
                    (
                        o["data"]["semiPersistent"].as_i64().unwrap_or(0) != 0,
                        o["data"]["dontSave"].as_i64().unwrap_or(0) != 0,
                    ),
                );
            }
            "HealthManager" => {
                imp.hm_gos.insert(go);
            }
            "GameObject" => {
                imp.go_active.insert(
                    id,
                    o["data"]["m_IsActive"]
                        .as_bool()
                        .unwrap_or_else(|| o["data"]["m_IsActive"].as_i64().unwrap_or(1) != 0),
                );
            }
            "PlayMakerFSM" => {
                let name = o["data"]["fsm"]["name"].as_str().unwrap_or("");
                if PICKUP_FSMS.contains(&name) {
                    imp.pickups.push((go, name.to_string()));
                }
            }
            "DamageHero"
                if o["data"]["m_Enabled"].as_i64().unwrap_or(1) != 0
                    && o["data"]["damageDealt"].as_i64().unwrap_or(0) > 0 =>
            {
                imp.damage_hero
                    .push((go, o["source"].as_str().unwrap_or("").to_string()));
            }
            _ => {}
        }
    }
    for c in geo["colliders"].as_array()? {
        if let Some(go) = c["game_object"].as_str() {
            imp.colliders.entry(go.to_string()).or_default().push((
                c["active_hierarchy"].as_bool().unwrap_or(false),
                c["enabled"].as_bool().unwrap_or(false),
            ));
        }
    }
    Some(imp)
}

impl Import {
    /// Whether a GameObject and every ancestor are active (`activeInHierarchy`).
    fn active(&self, go: i64) -> bool {
        let mut go = go;
        for _ in 0..64 {
            if !self.go_active.get(&go).copied().unwrap_or(true) {
                return false;
            }
            let Some(t) = self.transform_of.get(&go).and_then(|t| self.objects.get(t)) else {
                return true;
            };
            let Some(father) = t["data"]["m_Father"]["m_PathID"]
                .as_i64()
                .filter(|f| *f != 0)
            else {
                return true;
            };
            let Some(parent) = self
                .objects
                .get(&father)
                .and_then(|o| o["data"]["m_GameObject"]["m_PathID"].as_i64())
            else {
                return true;
            };
            go = parent;
        }
        true
    }

    /// The world scale of a GameObject: its Transform's local scale times every
    /// ancestor's.
    fn world_scale(&self, go: i64) -> Option<[f64; 2]> {
        let mut t = *self.transform_of.get(&go)?;
        let (mut sx, mut sy) = (1.0, 1.0);
        for _ in 0..64 {
            let o = self.objects.get(&t)?;
            sx *= o["data"]["m_LocalScale"]["x"].as_f64()?;
            sy *= o["data"]["m_LocalScale"]["y"].as_f64()?;
            let father = o["data"]["m_Father"]["m_PathID"].as_i64()?;
            if father == 0 {
                return Some([sx, sy]);
            }
            t = father;
        }
        None
    }
}

// --- spawn solver ----------------------------------------------------------------

struct Rest {
    y: f64,
    ticks: u32,
    grounded: bool,
    /// The spawn pose was pushed out of terrain by this much.
    pushed: f64,
}

fn round(v: f64) -> i32 {
    // Python's round() is half-even; the difference is one Q16 unit.
    let r = v.round();
    if (v - v.trunc()).abs() == 0.5 && (r as i64) % 2 != 0 {
        (r - v.signum()) as i32
    } else {
        r as i32
    }
}

/// What `Actor::advance_body` does to a gravity-bound placement before anything
/// else moves it: resolve the spawn overlap once, then fall under the source
/// gravity until the bounded solver reports ground. Mirrors the guest for the
/// controllers that take that path (Walker, Runner, Zombie Shield, Husk Guard).
fn settle(c: &Cooked, edges: &[[i32; 4]]) -> Option<Rest> {
    let b = c.box_world?;
    let bounds = [
        round((b[0] - c.pos[0]) * SCALE),
        round((b[1] - c.pos[1]) * SCALE),
        round((b[2] - c.pos[0]) * SCALE),
        round((b[3] - c.pos[1]) * SCALE),
    ];
    let (x, y) = (round(c.pos[0] * SCALE), round(c.pos[1] * SCALE));
    let offset = bounds[0] + (bounds[2] - bounds[0]) / 2;
    let foot_skin = if matches!(c.kind.as_str(), "ZombieSwipeWalker" | "ZombieShield") {
        983
    } else {
        0
    };
    let gravity = if c.kind == "ZombieSwipeWalker" {
        round(c.control_gravity.unwrap_or(1.0) * 60.0 * SCALE)
    } else {
        60 * Q
    };
    let p = Params {
        gravity,
        fall: 100 * Q,
        half_width: (bounds[2] - bounds[0]) / 2,
        bottom: bounds[1] - foot_skin,
        top: bounds[3],
        ..Params::ZERO
    };
    let count = edges.len();
    let edge = |i: usize| edges[i];
    let mut body = Player::spawn(x + offset, y);
    let before = (body.x, body.y);
    if !resolve_actor_spawn(&mut body, p, count, edge) {
        return None;
    }
    let pushed = ((body.x - before.0).abs().max((body.y - before.1).abs())) as f64 / SCALE;
    let (mut ticks, mut quiet) = (0, 0);
    while ticks < 1200 && quiet < 3 {
        body.step(p, 0, false, count, edge);
        ticks += 1;
        quiet = if body.grounded && body.vy == 0 {
            quiet + 1
        } else {
            0
        };
    }
    Some(Rest {
        y: body.y as f64 / SCALE,
        ticks,
        grounded: quiet >= 3,
        pushed,
    })
}

struct Edges {
    root: PathBuf,
    cache: HashMap<String, Option<Vec<[i32; 4]>>>,
}

impl Edges {
    fn get(&mut self, base: &str) -> Option<&Vec<[i32; 4]>> {
        let root = self.root.clone();
        self.cache
            .entry(base.to_string())
            .or_insert_with(|| {
                let bytes = fs::read(root.join(base)).ok()?;
                let room = Room::parse(&bytes).ok()?;
                Some((0..room.counts[5]).map(|i| room.edge(i)).collect())
            })
            .as_ref()
    }
}

// --- persistence table -----------------------------------------------------------

/// `(scene, source id)` of every row of `data/actor_persistence.rs`, with its group.
fn load_persist_table(path: &Path) -> Option<HashMap<(u16, u32), u16>> {
    let text = fs::read_to_string(path).ok()?;
    let body = text.split("PERSISTENT_ACTORS").nth(1)?;
    let start = body.find("= &[")? + 4;
    let end = start + body[start..].find("];")?;
    let mut out = HashMap::new();
    for tuple in body[start..end].split('(').skip(1) {
        let inner = tuple.split(')').next()?;
        let f: Vec<&str> = inner.split(',').map(str::trim).collect();
        if f.len() < 3 {
            continue;
        }
        let group = f[2]
            .strip_prefix("0x")
            .map(|h| u16::from_str_radix(h, 16).unwrap_or(0))
            .or_else(|| f[2].parse().ok())
            .unwrap_or(0);
        out.insert((f[0].parse().ok()?, f[1].parse().ok()?), group);
    }
    Some(out)
}

// --- census ----------------------------------------------------------------------

/// Families whose body the guest drops under gravity on the shared solver.
fn grounded_family(kind: &str) -> bool {
    matches!(
        kind,
        "WalkLeftRight" | "ZombieSwipeWalker" | "ZombieShield" | "HuskGuard"
    )
}

/// Families whose facing the cook reads off the placement's transform mirror.
fn mirror_driven(kind: &str) -> bool {
    matches!(
        kind,
        "WalkLeftRight" | "ZombieSwipeWalker" | "ZombieShield" | "HuskGuard" | "Blocker"
    )
}

/// Families that keep a death of their own (a battle scene's `Activated`, the
/// Blocker's terrain block), so the persistence table does not carry them.
fn own_persistence(kind: &str) -> bool {
    matches!(kind, "Blocker" | "Mawlek" | "FalseKnight" | "GruzMother")
}

fn reason_class(r: &str) -> String {
    let r = r.trim();
    let r = r.split(':').next().unwrap_or(r);
    r.chars().take(90).collect()
}

/// "Crawler (2)" and "Fly 3" are instances of "Crawler" and "Fly".
fn base_name(n: &str) -> String {
    let s = n.replace("(Clone)", "");
    let s = s.trim();
    let s = match s.rfind(" (") {
        Some(i)
            if s.ends_with(')') && s[i + 2..s.len() - 1].bytes().all(|b| b.is_ascii_digit()) =>
        {
            &s[..i]
        }
        _ => s,
    };
    let t = s.trim_end_matches(|c: char| c.is_ascii_digit()).trim_end();
    if t.is_empty() {
        s.to_string()
    } else {
        t.to_string()
    }
}

#[derive(Default)]
struct Args {
    root: PathBuf,
    survey: Vec<PathBuf>,
    banks: Option<PathBuf>,
    import: Option<PathBuf>,
    out: Option<PathBuf>,
    md: Option<PathBuf>,
    tol: f64,
    strict: bool,
}

fn parse_args() -> Args {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut a = Args {
        root: PathBuf::from("."),
        tol: 0.25,
        ..Default::default()
    };
    let mut i = 0;
    while i < argv.len() {
        let v = argv.get(i + 1).cloned().unwrap_or_default();
        match argv[i].as_str() {
            "--strict" => {
                a.strict = true;
                i += 1;
                continue;
            }
            "--root" => a.root = PathBuf::from(v),
            "--survey" => a.survey.push(PathBuf::from(v)),
            "--banks" => a.banks = Some(PathBuf::from(v)),
            "--world-import" => a.import = Some(PathBuf::from(v)),
            "--out" => a.out = Some(PathBuf::from(v)),
            "--md" => a.md = Some(PathBuf::from(v)),
            "--snap-tol" => a.tol = v.parse().expect("--snap-tol <units>"),
            other => panic!("unknown argument {other}"),
        }
        i += 2;
    }
    assert!(
        !a.survey.is_empty(),
        "need at least one --survey <hkref run dir>"
    );
    a
}

type Checks = BTreeMap<&'static str, (u64, u64)>;

fn note(c: &mut Checks, name: &'static str, ok: bool) {
    let e = c.entry(name).or_default();
    if ok {
        e.0 += 1
    } else {
        e.1 += 1
    }
}

fn check_issue(issues: &mut Vec<Value>, scene: &str, c: &Cooked, check: &str, detail: String) {
    issues.push(json!({"scene": scene, "name": c.name, "status": format!("check_{check}"), "detail": detail, "cooked": [c.pos[0], c.pos[1]], "kind": c.kind}));
}

fn main() {
    let args = parse_args();
    let root = args.root.clone();
    let banks_dir = args
        .banks
        .clone()
        .unwrap_or_else(|| root.join(".hkpsx/world-metadata-packed"));
    let import_dir = args
        .import
        .clone()
        .unwrap_or_else(|| root.join(".hkpsx/world-import"));
    let text = fs::read_to_string(root.join("data/regions.json")).expect("data/regions.json");
    let regions: Value = serde_json::from_str(&text).expect("regions.json");
    drop(text);
    let og = load_survey(&args.survey);
    let cooked = load_cooked(&regions);
    let views = load_views(&regions);
    let persist = load_persist_table(&root.join("data/actor_persistence.rs"));
    let mut edges = Edges {
        root: root.clone(),
        cache: HashMap::new(),
    };

    let scenes: Vec<(String, String, usize)> = regions["scenes"]
        .as_array()
        .expect("scenes")
        .iter()
        .map(|s| {
            (
                s["scene_name"].as_str().unwrap_or("").to_string(),
                s["file"].as_str().unwrap_or("").to_string(),
                s["scene_id"].as_u64().unwrap_or(0) as usize,
            )
        })
        .collect();
    // Cooked hazards per scene, by their DamageHero source.
    let mut hazards: HashMap<String, BTreeSet<String>> = HashMap::new();
    for r in regions["regions"].as_array().unwrap() {
        for h in r["hazards"].as_array().into_iter().flatten() {
            hazards
                .entry(r["scene_name"].as_str().unwrap_or("").to_string())
                .or_default()
                .insert(h["source"].as_str().unwrap_or("").to_string());
        }
    }

    // Cooked chests and pickups per scene (a chest's own shiny is a pickup too).
    let mut pickups: HashMap<String, BTreeSet<String>> = HashMap::new();
    for r in regions["regions"].as_array().unwrap() {
        let p = &r["pickups"];
        for kind in ["chests", "pickups"] {
            for x in p[kind].as_array().into_iter().flatten() {
                pickups
                    .entry(r["scene_name"].as_str().unwrap_or("").to_string())
                    .or_default()
                    .insert(x["source"].as_str().unwrap_or("").to_string());
            }
        }
    }

    let mut report_scenes = Vec::new();
    let mut totals: BTreeMap<&str, u64> = BTreeMap::new();
    let mut checks: Checks = BTreeMap::new();
    let mut reasons: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_name: BTreeMap<String, BTreeMap<&str, u64>> = BTreeMap::new();
    let mut issues: Vec<Value> = Vec::new();
    let (empty_og, empty_cooked): (Vec<Og>, Vec<Cooked>) = (Vec::new(), Vec::new());
    let mut scenes_pass = 0;

    for (scene, file, scene_id) in &scenes {
        let ogs = og.get(scene).unwrap_or(&empty_og);
        let cs = cooked.get(scene).unwrap_or(&empty_cooked);
        let imp = load_import(&import_dir, file);
        let bank = load_bank(&banks_dir, *scene_id);
        // Greedy nearest pairing of same-named objects, smallest distance first.
        let mut pairs: Vec<(f64, usize, usize)> = Vec::new();
        for (oi, o) in ogs.iter().enumerate() {
            for (ci, c) in cs.iter().enumerate() {
                if o.name == c.name {
                    pairs.push((
                        ((o.pos[0] - c.pos[0]).powi(2) + (o.pos[1] - c.pos[1]).powi(2)).sqrt(),
                        oi,
                        ci,
                    ));
                }
            }
        }
        pairs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let mut og_to_c: Vec<Option<usize>> = vec![None; ogs.len()];
        let mut c_used = vec![false; cs.len()];
        for (_, oi, ci) in pairs {
            if og_to_c[oi].is_none() && !c_used[ci] {
                og_to_c[oi] = Some(ci);
                c_used[ci] = true;
            }
        }
        let mut counts: BTreeMap<&str, u64> = BTreeMap::new();
        let mut scene_checks: Checks = BTreeMap::new();
        let mut rows = Vec::new();

        // 1. The original's active enemies.
        for (oi, o) in ogs.iter().enumerate() {
            if !o.active {
                continue;
            }
            let (status, detail, c) = match og_to_c[oi] {
                None => ("absent", String::new(), None),
                Some(ci) => {
                    let c = &cs[ci];
                    if !c.supported {
                        ("unadmitted", c.reason.clone(), Some(c))
                    } else if grounded_family(&c.kind) {
                        // The view that seats it: first whose interaction box holds
                        // the placement, else first whose collision apron does.
                        let vs = views.get(scene).map(|v| v.as_slice()).unwrap_or(&[]);
                        let view = vs
                            .iter()
                            .find(|v| inside(&v.interaction, c.pos))
                            .or_else(|| vs.iter().find(|v| inside(&v.collision, c.pos)));
                        match view.and_then(|v| edges.get(&v.base)).and_then(|e| settle(c, e)) {
                            None => ("snap", "spawn solver cannot place it (no room edges or overlap unresolved)".to_string(), Some(c)),
                            Some(r) => {
                                let dy = r.y - o.pos[1];
                                if !r.grounded {
                                    ("snap", format!("never lands: falls {:.2} in {} ticks, original rests at y {:.3}", c.pos[1] - r.y, r.ticks, o.pos[1]), Some(c))
                                } else if dy.abs() > args.tol {
                                    ("snap", format!("rests at y {:.3}, original at {:.3} (port - original = {:+.3}); spawn pushed {:.3}", r.y, o.pos[1], dy, r.pushed), Some(c))
                                } else {
                                    ("ok", String::new(), Some(c))
                                }
                            }
                        }
                    } else {
                        ("ok", String::new(), Some(c))
                    }
                }
            };
            *counts.entry(status).or_default() += 1;
            *totals.entry(status).or_default() += 1;
            *by_name
                .entry(base_name(&o.name))
                .or_default()
                .entry(status)
                .or_default() += 1;
            if status == "unadmitted" {
                *reasons.entry(reason_class(&detail)).or_default() += 1;
            }
            if status != "ok" {
                issues.push(json!({"scene": scene, "name": o.name, "status": status, "detail": detail,
                    "original": [o.pos[0], o.pos[1]], "cooked": c.map(|c| json!([c.pos[0], c.pos[1]])), "kind": c.map(|c| c.kind.clone()), "hp": o.hp}));
            }
            rows.push(json!({"name": o.name, "status": status, "detail": detail, "original": [o.pos[0], o.pos[1]], "cooked": c.map(|c| [c.pos[0], c.pos[1]]), "kind": c.map(|c| c.kind.clone()), "fsms": o.fsms}));
        }

        // 2. Cooked actors the original does not have active. A refused one is
        // never spawned, and a boss the arena script owns is placed dormant on
        // purpose, so neither is a difference in play; the rest are.
        for (ci, c) in cs.iter().enumerate() {
            let status = match og_to_c.iter().position(|m| *m == Some(ci)) {
                Some(oi) if ogs[oi].active => continue,
                _ if !c.supported => "extra_refused",
                _ if own_persistence(&c.kind) => "extra_arena",
                Some(_) => "extra_gated",
                None => "extra_unknown",
            };
            *counts.entry(status).or_default() += 1;
            *totals.entry(status).or_default() += 1;
            issues.push(json!({"scene": scene, "name": c.name, "status": status, "detail": if c.supported { "admitted" } else { "refused" }, "cooked": [c.pos[0], c.pos[1]], "kind": c.kind}));
            rows.push(json!({"name": c.name, "status": status, "cooked": [c.pos[0], c.pos[1]], "kind": c.kind, "admitted": c.supported}));
        }

        // 3. Contract checks on every placement the port spawns.
        let supported: Vec<&Cooked> = cs.iter().filter(|c| c.supported).collect();
        for c in &supported {
            // the bank agrees with the cook
            let banked = bank.as_ref().and_then(|b| b.get(&c.id).copied());
            let bank_ok = banked.is_some_and(|b| {
                b.flags & 64 != 0
                    && (b.x - round(c.pos[0] * SCALE)).abs() <= 1
                    && (b.y - round(c.pos[1] * SCALE)).abs() <= 1
            });
            note(&mut scene_checks, "bank", bank_ok);
            if !bank_ok {
                let why = match banked {
                    None => "no admitted bank object".to_string(),
                    Some(b) => format!(
                        "bank flags {:#x} at ({}, {}) disagree with the cook",
                        b.flags, b.x, b.y
                    ),
                };
                check_issue(&mut issues, scene, c, "bank", why);
            }
            if let Some(imp) = &imp {
                // facing and scale against the source transform
                if let (Some(s), Some(b)) = (imp.world_scale(c.game_object), banked) {
                    let dir = if b.flags & 1 != 0 { 1 } else { -1 };
                    if mirror_driven(&c.kind) {
                        let ok = dir == if s[0] < 0.0 { 1 } else { -1 };
                        note(&mut scene_checks, "facing", ok);
                        if !ok {
                            check_issue(
                                &mut issues,
                                scene,
                                c,
                                "facing",
                                format!("source scale x {:+.3}, bank direction {dir}", s[0]),
                            );
                        }
                    }
                    if let Some(v) = c.visual_scale {
                        let want = [
                            (s[0] * c.sprite_scale[0]).abs(),
                            (s[1] * c.sprite_scale[1]).abs(),
                        ];
                        let ok = (v[0] - want[0]).abs() < 1e-3 && (v[1] - want[1]).abs() < 1e-3;
                        note(&mut scene_checks, "scale", ok);
                        if !ok {
                            check_issue(&mut issues, scene, c, "scale", format!("source scale ({:.3}, {:.3}) with sprite scale, cooked visual ({:.3}, {:.3})", want[0], want[1], v[0], v[1]));
                        }
                    }
                }
                // a killed enemy with a PersistentBoolItem stays dead
                if c.id < 100_000 && !own_persistence(&c.kind) {
                    if let Some(&(_semi, dont_save)) = imp.pbi_of.get(&c.game_object) {
                        if !dont_save {
                            let ok = persist
                                .as_ref()
                                .is_some_and(|t| t.contains_key(&(*scene_id as u16, c.id)));
                            note(&mut scene_checks, "persistence", ok);
                            if !ok {
                                check_issue(&mut issues, scene, c, "persistence", "source PersistentBoolItem keeps its death, the port seats it again".into());
                            }
                        }
                    }
                }
            }
        }
        // the guest pool
        note(&mut scene_checks, "pool", supported.len() <= POOL);
        if supported.len() > POOL {
            issues.push(json!({"scene": scene, "name": "(scene)", "status": "check_pool", "detail": format!("{} supported placements for {POOL} slots", supported.len())}));
        }

        // 4. Hazards: every DamageHero shape with an active collider is cooked.
        if let Some(imp) = &imp {
            let cooked_h = hazards.get(scene);
            for (go, source) in &imp.damage_hero {
                if imp.hm_gos.contains(go) {
                    continue;
                }
                let live = imp
                    .colliders
                    .get(&format!("{file}:{go}"))
                    .is_some_and(|cs| cs.iter().any(|&(a, e)| a && e));
                if !live {
                    continue;
                }
                let ok = cooked_h.is_some_and(|h| h.contains(source));
                note(&mut scene_checks, "hazard", ok);
                if !ok {
                    issues.push(json!({"scene": scene, "name": source, "status": "check_hazard", "detail": "DamageHero with an active collider is not cooked"}));
                }
            }
        }

        // 5. Pickups: every chest, shiny, heart piece and vessel fragment the
        // scene shows is cooked, and so is the shiny inside each chest.
        if let Some(imp) = &imp {
            let live = imp.pickups.iter().filter(|(go, _)| imp.active(*go)).count();
            let chests = imp
                .pickups
                .iter()
                .filter(|(go, name)| name == CHEST && imp.active(*go))
                .count();
            let want = live + chests;
            let have = pickups.get(scene).map_or(0, |p| p.len());
            note(&mut scene_checks, "pickup", want == have);
            if want != have {
                issues.push(json!({"scene": scene, "name": "(scene)", "status": "check_pickup", "detail": format!("the source shows {want} pickups ({chests} chests with a shiny inside), the cook has {have}")}));
            }
        }

        for (k, v) in &scene_checks {
            let e = checks.entry(k).or_default();
            e.0 += v.0;
            e.1 += v.1;
        }
        let expected: u64 = ["ok", "snap", "unadmitted", "absent"]
            .iter()
            .map(|k| counts.get(k).copied().unwrap_or(0))
            .sum();
        let bad: u64 = [
            "snap",
            "unadmitted",
            "absent",
            "extra_gated",
            "extra_unknown",
        ]
        .iter()
        .map(|k| counts.get(k).copied().unwrap_or(0))
        .sum::<u64>()
            + scene_checks.values().map(|v| v.1).sum::<u64>();
        if bad == 0 {
            scenes_pass += 1;
        }
        report_scenes.push(json!({"scene": scene, "expected": expected, "counts": counts, "checks": scene_checks, "pass": bad == 0, "rows": rows}));
    }

    let expected_total: u64 = ["ok", "snap", "unadmitted", "absent"]
        .iter()
        .map(|k| totals.get(k).copied().unwrap_or(0))
        .sum();
    let (check_pass, check_fail) = checks.values().fold((0, 0), |a, v| (a.0 + v.0, a.1 + v.1));
    let summary = json!({"scenes": scenes.len(), "scenes_pass": scenes_pass, "scenes_fail": scenes.len() - scenes_pass,
        "expected_active_enemies": expected_total, "totals": totals, "checks": checks, "checks_pass": check_pass, "checks_fail": check_fail,
        "snap_tolerance": args.tol, "persistence_table": persist.is_some(),
        "unadmitted_reasons": reasons, "by_family": by_name});
    let report = json!({"summary": summary, "issues": issues, "scenes": report_scenes});
    if let Some(p) = &args.out {
        fs::write(p, serde_json::to_string_pretty(&report).unwrap()).expect("write json");
    }
    let md_text = markdown(&report, &summary);
    match &args.md {
        Some(p) => fs::write(p, &md_text).expect("write md"),
        None => print!("{md_text}"),
    }
    let enemy_fail = [
        "snap",
        "unadmitted",
        "absent",
        "extra_gated",
        "extra_unknown",
    ]
    .iter()
    .map(|k| totals.get(k).copied().unwrap_or(0))
    .sum::<u64>();
    println!("scenes pass {scenes_pass}/{}; enemies {:?}; checks pass {check_pass} fail {check_fail} {checks:?}", scenes.len(), totals);
    if args.strict && (enemy_fail != 0 || check_fail != 0) {
        std::process::exit(1);
    }
}

fn markdown(report: &Value, summary: &Value) -> String {
    let mut m = String::new();
    m.push_str("# Placement census\n\n");
    let t = &summary["totals"];
    let g = |k: &str| t[k].as_u64().unwrap_or(0);
    let exp = summary["expected_active_enemies"].as_u64().unwrap_or(0);
    m.push_str(&format!(
        "{} of {} scenes pass. {} active enemies in the original: {} ok, {} snap, {} unadmitted, {} absent. Cooked actors the original does not have active: {} gated in the original and {} unknown (a difference in play); {} refused by the cook and {} boss arenas (not one). Contract checks: {} pass, {} fail.\n\n",
        summary["scenes_pass"], summary["scenes"], exp, g("ok"), g("snap"), g("unadmitted"), g("absent"), g("extra_gated"), g("extra_unknown"), g("extra_refused"), g("extra_arena"), summary["checks_pass"], summary["checks_fail"]
    ));
    m.push_str("| contract check | pass | fail |\n|---|---|---|\n");
    for (k, v) in summary["checks"].as_object().unwrap() {
        m.push_str(&format!("| {k} | {} | {} |\n", v[0], v[1]));
    }
    m.push_str("\n| scene | enemies | ok | snap | unadmitted | absent | extra | check fails | verdict |\n|---|---|---|---|---|---|---|---|---|\n");
    for s in report["scenes"].as_array().unwrap() {
        let c = &s["counts"];
        let n = |k: &str| c[k].as_u64().unwrap_or(0);
        let check_fails: u64 = s["checks"]
            .as_object()
            .unwrap()
            .values()
            .map(|v| v[1].as_u64().unwrap_or(0))
            .sum();
        if s["expected"].as_u64().unwrap_or(0) == 0
            && n("extra_gated") + n("extra_unknown") == 0
            && check_fails == 0
        {
            continue;
        }
        m.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            s["scene"].as_str().unwrap_or(""),
            s["expected"],
            n("ok"),
            n("snap"),
            n("unadmitted"),
            n("absent"),
            n("extra_gated") + n("extra_unknown"),
            check_fails,
            if s["pass"].as_bool().unwrap_or(false) {
                "pass"
            } else {
                "FAIL"
            }
        ));
    }
    m.push_str("\n## Unadmitted, by the cook's reason\n\n");
    let mut r: Vec<(&String, u64)> = summary["unadmitted_reasons"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k, v.as_u64().unwrap_or(0)))
        .collect();
    r.sort_by_key(|row| std::cmp::Reverse(row.1));
    for (k, v) in r {
        m.push_str(&format!("- {v}: {k}\n"));
    }
    m.push_str("\n## By enemy family\n\n| family | ok | snap | unadmitted | absent |\n|---|---|---|---|---|\n");
    for (name, st) in summary["by_family"].as_object().unwrap() {
        let n = |k: &str| st[k].as_u64().unwrap_or(0);
        m.push_str(&format!(
            "| {name} | {} | {} | {} | {} |\n",
            n("ok"),
            n("snap"),
            n("unadmitted"),
            n("absent")
        ));
    }
    // Check failures grouped, then the enemy rows.
    let mut grouped: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for i in report["issues"].as_array().unwrap() {
        let st = i["status"].as_str().unwrap_or("");
        if st == "snap" || st.starts_with("extra") || st.starts_with("check_") {
            grouped.entry(st.to_string()).or_default().push(format!(
                "{} / {}: {}",
                i["scene"].as_str().unwrap_or(""),
                i["name"].as_str().unwrap_or(""),
                i["detail"].as_str().unwrap_or("")
            ));
        }
    }
    for (st, rows) in grouped {
        m.push_str(&format!("\n## {st} ({})\n\n", rows.len()));
        for r in rows.iter().take(80) {
            m.push_str(&format!("- {r}\n"));
        }
        if rows.len() > 80 {
            m.push_str(&format!(
                "- ... and {} more (see the JSON)\n",
                rows.len() - 80
            ));
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn survey_fields_keep_quoted_commas_and_quotes() {
        assert_eq!(
            csv_fields(r#"Crossroads_01,12,"Crawler, 1",8"#),
            ["Crossroads_01", "12", "Crawler, 1", "8"]
        );
        assert_eq!(csv_fields(r#"a,"say ""hi""",c"#), ["a", r#"say "hi""#, "c"]);
        assert_eq!(csv_fields("a,,c"), ["a", "", "c"]);
    }

    #[test]
    fn instances_share_a_family_name() {
        assert_eq!(base_name("Crawler (2)"), "Crawler");
        assert_eq!(base_name("Fly 3"), "Fly");
        assert_eq!(
            base_name("Hatcher Baby Spawner (21)"),
            "Hatcher Baby Spawner"
        );
        assert_eq!(base_name("Zombie Runner 1"), "Zombie Runner");
        assert_eq!(base_name("42"), "42");
    }

    #[test]
    fn the_persistence_table_reads_back_what_the_cook_writes() {
        let path =
            std::env::temp_dir().join(format!("hk-census-persist-{}.rs", std::process::id()));
        fs::write(
            &path,
            "pub const GROUPS: usize = 2;\npub const PERSISTENT_ACTORS: &[hk_sim::PersistentActor] = &[\n    (2, 5195, 0x8000), // Zombie Runner (1)\n    (19, 77, 0x0001), // Hatcher\n];\n",
        )
        .unwrap();
        let table = load_persist_table(&path).unwrap();
        fs::remove_file(&path).unwrap();
        assert_eq!(table.len(), 2);
        assert_eq!(table[&(2, 5195)], 0x8000);
        assert_eq!(table[&(19, 77)], 1);
    }

    #[test]
    fn the_solver_rests_a_walker_on_the_floor_it_would_have_fallen_to() {
        // A 1x1 box standing 3 units over a floor at y = 1 settles on it.
        let c = Cooked {
            id: 1,
            name: "Crawler".into(),
            pos: [0.0, 4.0, 0.0],
            supported: true,
            kind: "WalkLeftRight".into(),
            reason: String::new(),
            box_world: Some([-0.5, 3.0, 0.5, 4.0]),
            control_gravity: None,
            visual_scale: None,
            sprite_scale: [1.0, 1.0],
            game_object: 1,
        };
        let floor = [[-10 * Q, Q, 10 * Q, Q]];
        let rest = settle(&c, &floor).unwrap();
        assert!(rest.grounded);
        assert!(
            (rest.y - 2.0).abs() < 0.01,
            "rests with its box bottom on y = 1, got {}",
            rest.y
        );
        // No floor: it never lands.
        let rest = settle(&c, &[[0, 0, 0, 0]]).unwrap();
        assert!(!rest.grounded);
    }
}
