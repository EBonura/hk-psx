//! The committed scene catalog: all 501 BuildSettings scenes with their area, cook state, size,
//! enemies, interactables and exits, as names, counts and byte sizes only (no game-derived assets).
//!
//! `docs/catalog/scenes.tsv` is the source of truth and `docs/CATALOG.md` is rendered from it.
//! `extract` rebuilds the TSV from the ignored whole-world import (`.hkpsx/world-import`), `render`
//! rebuilds the Markdown from the TSV. Both are deterministic, so a test can demand that
//! regenerating either is byte-identical to the committed file.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io::Read;
use std::path::Path;

/// Cook state of a scene. `Playable` scenes are the rows of `quality::SCENE_TABLE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Playable,
    CookedPack,
    NoEnvelope,
    Over128Views,
}

impl Status {
    pub const ALL: [Status; 4] = [
        Status::Playable,
        Status::CookedPack,
        Status::NoEnvelope,
        Status::Over128Views,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Status::Playable => "playable",
            Status::CookedPack => "cooked-pack",
            Status::NoEnvelope => "no-envelope",
            Status::Over128Views => "over-128-views",
        }
    }

    fn parse(text: &str) -> Result<Status, String> {
        Status::ALL
            .into_iter()
            .find(|s| s.label() == text)
            .ok_or_else(|| format!("unknown status {text:?}"))
    }
}

/// Areas in display order. `area_of` returns one of these.
pub const AREAS: [&str; 26] = [
    "Menus, helpers and tests",
    "Cinematics and openings",
    "King's Pass",
    "Dirtmouth",
    "Interior rooms",
    "Tram",
    "Forgotten Crossroads",
    "Greenpath",
    "Fungal Wastes",
    "Fog Canyon and Queen's Gardens",
    "City of Tears",
    "Resting Grounds",
    "Howling Cliffs",
    "Crystal Peak",
    "Deepnest",
    "Kingdom's Edge",
    "Colosseum of Fools",
    "Ancient Basin and Abyss",
    "Royal Waterways",
    "The Hive",
    "White Palace",
    "Grimm Troupe",
    "Dreams",
    "Hollow Knight finale",
    "Godhome",
    "Other",
];

/// Interactable classes in display order. A class is a serialized component type or a PlayMaker FSM
/// name, chosen from the 835 FSM names and 468 component types the import found. `class_of` maps
/// one onto the other. The full census by behaviour class is a separate piece of work; this is the
/// per-scene count the catalog needs to size a scene.
pub const INTERACTABLE_CLASSES: [&str; 24] = [
    "breakable",
    "breakable_wall",
    "reveal_mask",
    "persistent",
    "geo_rock",
    "bench",
    "lever",
    "switch",
    "toll_gate",
    "gate",
    "door",
    "chest",
    "shiny",
    "soul_totem",
    "grub",
    "lift",
    "tram",
    "stag",
    "shop",
    "conversation",
    "inspect",
    "battle",
    "boss_door",
    "boss_statue",
];

/// One scene of the catalog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub index: u32,
    pub scene: String,
    pub area: String,
    pub role: String,
    pub status: Status,
    pub views: Option<u32>,
    pub ram_bytes: Option<u64>,
    pub vram_bytes: Option<u64>,
    pub disc_bytes: Option<u64>,
    pub pages: Option<u32>,
    pub palettes: Option<u32>,
    pub enemies: u32,
    pub families: BTreeMap<String, u32>,
    pub interactables: BTreeMap<String, u32>,
    pub flags: Vec<String>,
    pub gates: u32,
    pub exits: Vec<String>,
    pub scripted_loads: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Catalog {
    pub world_fingerprint: String,
    pub matrix_fingerprint: String,
    pub rows: Vec<Row>,
}

/// The area of a scene, from its name and the import's primary role.
pub fn area_of(scene: &str, role: &str) -> &'static str {
    if role == "cinematic" {
        return AREAS[1];
    }
    let by_name = area_by_prefix(scene);
    match role {
        // Helper scenes keep the area their name gives them (Crossroads_10_preload, GG_Unlock).
        "menu" | "transition_or_bootstrap_helper" | "shared_player_prefab_scene" | "credits"
        | "completion_screen" => {
            if by_name == AREAS[25] {
                AREAS[0]
            } else {
                by_name
            }
        }
        _ => by_name,
    }
}

fn area_by_prefix(scene: &str) -> &'static str {
    let starts = |p: &str| scene.starts_with(p);
    if starts("_test") {
        return AREAS[0];
    }
    if starts("Opening_Sequence") || starts("Cutscene_") || starts("Cinematic_") {
        return AREAS[1];
    }
    if starts("Tutorial_") {
        return AREAS[2];
    }
    if scene == "Town" {
        return AREAS[3];
    }
    if starts("Room_Tram") {
        return AREAS[5];
    }
    if starts("Room_Final_Boss") {
        return AREAS[23];
    }
    if starts("Room_Colosseum") {
        return AREAS[16];
    }
    if starts("Room_GG") || starts("GG_") {
        return AREAS[24];
    }
    if starts("Room_spider") {
        return AREAS[14];
    }
    if starts("Room_") {
        return AREAS[4];
    }
    if starts("Crossroads") {
        return AREAS[6];
    }
    if starts("Fungus1") {
        return AREAS[7];
    }
    if starts("Fungus2") {
        return AREAS[8];
    }
    if starts("Fungus3") {
        return AREAS[9];
    }
    if starts("Ruins") {
        return AREAS[10];
    }
    if starts("RestingGrounds") {
        return AREAS[11];
    }
    if starts("Cliffs") {
        return AREAS[12];
    }
    if starts("Mines") {
        return AREAS[13];
    }
    if starts("Deepnest_East") {
        return AREAS[15];
    }
    if starts("Deepnest") {
        return AREAS[14];
    }
    if starts("Abyss") {
        return AREAS[17];
    }
    if starts("Waterways") {
        return AREAS[18];
    }
    if starts("Hive") {
        return AREAS[19];
    }
    if starts("White_Palace") {
        return AREAS[20];
    }
    if starts("Grimm") {
        return AREAS[21];
    }
    if starts("Dream") {
        return AREAS[22];
    }
    AREAS[25]
}

/// The interactable class of a component type or PlayMaker FSM name, if it has one.
pub fn class_of(component: &str, fsm_name: Option<&str>) -> Option<&'static str> {
    if let Some(name) = fsm_name {
        return match name {
            "breakable_wall_v2" | "break_floor" | "collapse small" | "quake_floor" => {
                Some("breakable_wall")
            }
            "unmasker" => Some("reveal_mask"),
            "Control Lever" => Some("lever"),
            "Switch Control" => Some("switch"),
            "Toll Gate" => Some("toll_gate"),
            "Mantis Gate" | "Bone Gate" | "Gate Control" | "Gate" => Some("gate"),
            "Door Control" => Some("door"),
            "Chest Control" => Some("chest"),
            "Shiny Control" => Some("shiny"),
            "soul_totem" => Some("soul_totem"),
            "Grub Control" => Some("grub"),
            "Lift Control" => Some("lift"),
            "Tram Control" => Some("tram"),
            "Stag Control" => Some("stag"),
            "Conversation Control" => Some("conversation"),
            "inspect_region" => Some("inspect"),
            "Battle Control" => Some("battle"),
            _ => None,
        };
    }
    match component {
        "Breakable" | "BreakableInfectedVine" | "BreakablePoleSimple" => Some("breakable"),
        "PersistentBoolItem" | "PersistentIntItem" => Some("persistent"),
        "GeoRock" => Some("geo_rock"),
        "RestBench" => Some("bench"),
        "BridgeLever" => Some("lever"),
        "ShopMenuStock" => Some("shop"),
        "BossSequenceDoor" => Some("boss_door"),
        "BossStatue" => Some("boss_statue"),
        _ => None,
    }
}

/// A GameObject name reduced to its family: trailing "(n)", digits, spaces and underscores go.
pub fn family_of(name: &str) -> String {
    let mut s = name.trim();
    loop {
        let before = s;
        if s.ends_with(')') {
            if let Some(open) = s.rfind('(') {
                let inner = &s[open + 1..s.len() - 1];
                if !inner.is_empty() && inner.bytes().all(|b| b.is_ascii_digit()) {
                    s = s[..open].trim_end();
                }
            }
        }
        let digits = s.trim_end_matches(|c: char| c.is_ascii_digit());
        if digits.len() != s.len() {
            s = digits.trim_end_matches(|c: char| c == ' ' || c == '_');
        }
        s = s.trim_end_matches(|c: char| c == ' ' || c == '_');
        if s == before {
            break;
        }
    }
    let cleaned: String = s
        .chars()
        .map(|c| if "\t\n\r|=;".contains(c) { ' ' } else { c })
        .collect();
    if cleaned.is_empty() {
        "(unnamed)".to_string()
    } else {
        cleaned
    }
}

fn kb(bytes: u64) -> u64 {
    (bytes + 512) / 1024
}

fn as_u32(v: &Value) -> Option<u32> {
    v.as_u64().map(|x| x as u32)
}

fn get<'a>(v: &'a Value, path: &[&str]) -> &'a Value {
    let mut cur = v;
    for key in path {
        cur = &cur[*key];
    }
    cur
}

/// What one scene's components say: enemy family counts and interactable class counts.
fn scan_components(dir: &Path, file: &str) -> Result<(BTreeMap<String, u32>, BTreeMap<String, u32>), String> {
    let path = dir.join(file).join("components.json.gz");
    let handle = std::fs::File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut raw = Vec::new();
    flate2::read::GzDecoder::new(handle)
        .read_to_end(&mut raw)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let doc: Value = serde_json::from_slice(&raw).map_err(|e| format!("{}: {e}", path.display()))?;
    let objects = doc["objects"].as_array().ok_or("components without objects")?;
    let mut names: BTreeMap<&str, &str> = BTreeMap::new();
    for o in objects {
        if o["type"] == "GameObject" {
            if let (Some(src), Some(name)) = (o["source"].as_str(), o["data"]["m_Name"].as_str()) {
                names.insert(src, name);
            }
        }
    }
    let mut families: BTreeMap<String, u32> = BTreeMap::new();
    let mut classes: BTreeMap<String, u32> = BTreeMap::new();
    for o in objects {
        let ty = o["type"].as_str().unwrap_or("");
        if ty == "HealthManager" {
            let id = o["data"]["m_GameObject"]["m_PathID"].as_i64().unwrap_or(0);
            let key = format!("{file}:{id}");
            let name = names.get(key.as_str()).copied().unwrap_or("");
            *families.entry(family_of(name)).or_insert(0) += 1;
        }
        let fsm = if ty == "PlayMakerFSM" {
            o["data"]["fsm"]["name"].as_str()
        } else {
            None
        };
        let class = if ty == "PlayMakerFSM" {
            fsm.and_then(|n| class_of("", Some(n)))
        } else {
            class_of(ty, None)
        };
        if let Some(class) = class {
            *classes.entry(class.to_string()).or_insert(0) += 1;
        }
    }
    Ok((families, classes))
}

fn read_json(path: &Path) -> Result<Value, String> {
    let text = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_slice(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Rebuild the catalog from the whole-world import directory (`.hkpsx/world-import`).
pub fn extract(world_import: &Path, playable: &[&str]) -> Result<Catalog, String> {
    let world = read_json(&world_import.join("world-catalog.json"))?;
    let matrix = read_json(&world_import.join("pack-matrix.json"))?;
    let scripted = read_json(&world_import.join("scripted-transitions.json"))?;
    let world_fingerprint = world["source_world_fingerprint"].as_str().ok_or("no world fingerprint")?.to_string();
    let matrix_fingerprint = matrix["inventory_fingerprint"].as_str().ok_or("no matrix fingerprint")?.to_string();
    if matrix["world_fingerprint"].as_str() != Some(world_fingerprint.as_str()) {
        return Err("pack-matrix.json was built from another world import than world-catalog.json".into());
    }
    let scenes = world["scenes"].as_array().ok_or("no scenes")?;
    let matrix_scenes: BTreeMap<&str, &Value> = matrix["scenes"]
        .as_array()
        .ok_or("no matrix scenes")?
        .iter()
        .filter_map(|s| s["scene_name"].as_str().map(|n| (n, s)))
        .collect();

    let mut gates: BTreeMap<&str, u32> = BTreeMap::new();
    let mut exits: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    for t in world["transition_points"].as_array().ok_or("no transition points")? {
        if let Some(src) = t["source_scene"].as_str() {
            *gates.entry(src).or_insert(0) += 1;
            if let Some(dst) = t["target_scene"].as_str().filter(|d| !d.is_empty()) {
                exits.entry(src).or_default().insert(dst.to_string());
            }
        }
    }
    let mut loads: BTreeMap<&str, u32> = BTreeMap::new();
    for a in scripted["actions"].as_array().ok_or("no scripted actions")? {
        if let Some(src) = a["source_scene"].as_str() {
            *loads.entry(src).or_insert(0) += 1;
        }
    }

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(3)
        .build()
        .map_err(|e| e.to_string())?;
    use rayon::prelude::*;
    let scanned: Vec<Result<(BTreeMap<String, u32>, BTreeMap<String, u32>), String>> = pool.install(|| {
        scenes
            .par_iter()
            .map(|s| scan_components(world_import, s["file"].as_str().unwrap_or("")))
            .collect()
    });

    let mut rows = Vec::new();
    for (s, scan) in scenes.iter().zip(scanned) {
        let (families, interactables) = scan?;
        let name = s["scene_name"].as_str().ok_or("scene without name")?;
        let role = s["primary_role"].as_str().unwrap_or("").to_string();
        let m = matrix_scenes.get(name).ok_or_else(|| format!("{name} missing from pack-matrix.json"))?;
        let cooking = &m["cooking"];
        let admission = cooking["admission"].as_str().unwrap_or("");
        let mut status = match admission {
            "isolated_pack_measured" => Status::CookedPack,
            "no_source_envelope" => Status::NoEnvelope,
            "isolated_pack_over_budget" => {
                let err = cooking["pack"]["error"].as_str().unwrap_or("");
                if err.contains("views exceeds the HKSCNE room limit") {
                    Status::Over128Views
                } else {
                    return Err(format!("{name}: over budget for another reason: {err}"));
                }
            }
            other => return Err(format!("{name}: unknown admission {other:?}")),
        };
        if playable.contains(&name) {
            if status != Status::CookedPack {
                return Err(format!("{name} is in SCENE_TABLE but its pack is {}", status.label()));
            }
            status = Status::Playable;
        }
        let mut flags = Vec::new();
        if interactables_boss(s) {
            flags.push("boss".to_string());
        }
        if m["unsupported_types"]
            .as_array()
            .map_or(false, |a| a.iter().any(|t| t.as_str().map_or(false, |t| t.starts_with("TextMesh"))))
        {
            flags.push("text".to_string());
        }
        rows.push(Row {
            index: as_u32(&s["index"]).ok_or("scene without index")?,
            scene: name.to_string(),
            area: area_of(name, &role).to_string(),
            role,
            status,
            views: as_u32(get(cooking, &["pack", "regions"])),
            ram_bytes: cooking["ps1_ram_bytes"].as_u64(),
            vram_bytes: cooking["ps1_vram_bytes"].as_u64(),
            disc_bytes: get(cooking, &["pack", "resident_stored_bytes"]).as_u64(),
            pages: as_u32(get(cooking, &["pack", "pages"])),
            palettes: as_u32(get(cooking, &["pack", "palettes"])),
            enemies: families.values().sum(),
            families,
            interactables,
            flags,
            gates: gates.get(name).copied().unwrap_or(0),
            exits: exits.get(name).map(|e| e.iter().cloned().collect()).unwrap_or_default(),
            scripted_loads: loads.get(name).copied().unwrap_or(0),
        });
    }
    rows.sort_by_key(|r| r.index);
    Ok(Catalog { world_fingerprint, matrix_fingerprint, rows })
}

/// A scene is a boss scene when the import found a BossSceneController in it.
fn interactables_boss(scene: &Value) -> bool {
    scene["content"]["bosses"]["BossSceneController"].as_u64().unwrap_or(0) > 0
}

const COLUMNS: [&str; 18] = [
    "index", "scene", "area", "role", "status", "views", "ram_bytes", "vram_bytes", "disc_bytes",
    "pages", "palettes", "enemies", "families", "interactables", "flags", "gates", "exits",
    "scripted_loads",
];

fn opt<T: std::fmt::Display>(v: &Option<T>) -> String {
    v.as_ref().map(|x| x.to_string()).unwrap_or_default()
}

fn pairs(m: &BTreeMap<String, u32>) -> String {
    // Most numerous first, then by name: stable and the first entries are the ones that matter.
    let mut items: Vec<(&String, &u32)> = m.iter().collect();
    items.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    items.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("|")
}

impl Catalog {
    pub fn to_tsv(&self) -> String {
        let mut out = String::new();
        out.push_str("# hk-psx scene catalog. Generated by hk-catalog from the whole-world import; do not edit.\n");
        out.push_str("# Names, counts and byte sizes only. Regenerate: cargo run --release --manifest-path host/Cargo.toml -p hk-cook --bin hk-catalog -- all <world-import dir>\n");
        let _ = writeln!(out, "# source_world_fingerprint={}", self.world_fingerprint);
        let _ = writeln!(out, "# pack_matrix_inventory_fingerprint={}", self.matrix_fingerprint);
        out.push_str(&COLUMNS.join("\t"));
        out.push('\n');
        for r in &self.rows {
            let cells = [
                r.index.to_string(),
                r.scene.clone(),
                r.area.clone(),
                r.role.clone(),
                r.status.label().to_string(),
                opt(&r.views),
                opt(&r.ram_bytes),
                opt(&r.vram_bytes),
                opt(&r.disc_bytes),
                opt(&r.pages),
                opt(&r.palettes),
                r.enemies.to_string(),
                pairs(&r.families),
                pairs(&r.interactables),
                r.flags.join("|"),
                r.gates.to_string(),
                r.exits.join("|"),
                r.scripted_loads.to_string(),
            ];
            out.push_str(&cells.join("\t"));
            out.push('\n');
        }
        out
    }

    pub fn from_tsv(text: &str) -> Result<Catalog, String> {
        let mut world_fingerprint = String::new();
        let mut matrix_fingerprint = String::new();
        let mut rows = Vec::new();
        let mut header_seen = false;
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix('#') {
                if let Some(v) = rest.trim().strip_prefix("source_world_fingerprint=") {
                    world_fingerprint = v.to_string();
                }
                if let Some(v) = rest.trim().strip_prefix("pack_matrix_inventory_fingerprint=") {
                    matrix_fingerprint = v.to_string();
                }
                continue;
            }
            if !header_seen {
                if line != COLUMNS.join("\t") {
                    return Err(format!("unexpected header {line:?}"));
                }
                header_seen = true;
                continue;
            }
            let c: Vec<&str> = line.split('\t').collect();
            if c.len() != COLUMNS.len() {
                return Err(format!("row has {} cells, expected {}: {line:?}", c.len(), COLUMNS.len()));
            }
            let num = |s: &str| -> Result<Option<u64>, String> {
                if s.is_empty() { Ok(None) } else { s.parse().map(Some).map_err(|e| format!("{s:?}: {e}")) }
            };
            let map = |s: &str| -> Result<BTreeMap<String, u32>, String> {
                let mut m = BTreeMap::new();
                for item in s.split('|').filter(|i| !i.is_empty()) {
                    let (k, v) = item.rsplit_once('=').ok_or_else(|| format!("bad pair {item:?}"))?;
                    m.insert(k.to_string(), v.parse().map_err(|e| format!("{item:?}: {e}"))?);
                }
                Ok(m)
            };
            let list = |s: &str| -> Vec<String> {
                s.split('|').filter(|i| !i.is_empty()).map(str::to_string).collect()
            };
            let families = map(c[12])?;
            rows.push(Row {
                index: c[0].parse().map_err(|e| format!("{:?}: {e}", c[0]))?,
                scene: c[1].to_string(),
                area: c[2].to_string(),
                role: c[3].to_string(),
                status: Status::parse(c[4])?,
                views: num(c[5])?.map(|v| v as u32),
                ram_bytes: num(c[6])?,
                vram_bytes: num(c[7])?,
                disc_bytes: num(c[8])?,
                pages: num(c[9])?.map(|v| v as u32),
                palettes: num(c[10])?.map(|v| v as u32),
                enemies: c[11].parse().map_err(|e| format!("{:?}: {e}", c[11]))?,
                families,
                interactables: map(c[13])?,
                flags: list(c[14]),
                gates: c[15].parse().map_err(|e| format!("{:?}: {e}", c[15]))?,
                exits: list(c[16]),
                scripted_loads: c[17].parse().map_err(|e| format!("{:?}: {e}", c[17]))?,
            });
        }
        if !header_seen {
            return Err("no header row".into());
        }
        Ok(Catalog { world_fingerprint, matrix_fingerprint, rows })
    }

    /// The per-area counts as a Markdown table. The completion ledger carries a copy and a test
    /// keeps the two equal.
    pub fn area_table(&self) -> String {
        let mut o = String::new();
        o.push_str("| Area | Scenes | Playable | Cooked pack | No envelope | Over 128 views | Enemies | Cooked KB |\n| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n");
        let mut totals = [0usize; 5];
        let mut tot_enemies = 0u64;
        let mut tot_kb = 0u64;
        for area in AREAS {
            let in_area: Vec<&Row> = self.rows.iter().filter(|r| r.area == area).collect();
            if in_area.is_empty() {
                continue;
            }
            let c = [
                in_area.len(),
                self.count(area, Status::Playable),
                self.count(area, Status::CookedPack),
                self.count(area, Status::NoEnvelope),
                self.count(area, Status::Over128Views),
            ];
            let enemies: u64 = in_area.iter().map(|r| r.enemies as u64).sum();
            let bytes: u64 = in_area.iter().filter_map(|r| r.disc_bytes).sum();
            let _ = writeln!(
                o,
                "| {area} | {} | {} | {} | {} | {} | {enemies} | {} |",
                c[0], c[1], c[2], c[3], c[4], kb(bytes)
            );
            for (t, v) in totals.iter_mut().zip(c) {
                *t += v;
            }
            tot_enemies += enemies;
            tot_kb += kb(bytes);
        }
        let _ = writeln!(
            o,
            "| **Total** | **{}** | **{}** | **{}** | **{}** | **{}** | **{tot_enemies}** | **{tot_kb}** |",
            totals[0], totals[1], totals[2], totals[3], totals[4]
        );
        o
    }

    fn count(&self, area: &str, status: Status) -> usize {
        self.rows.iter().filter(|r| r.area == area && r.status == status).count()
    }

    /// The Markdown catalog. Only the TSV feeds it.
    pub fn render_markdown(&self) -> String {
        let mut o = String::new();
        let total = self.rows.len();
        o.push_str("# Scene catalog\n\n");
        o.push_str("Generated from `docs/catalog/scenes.tsv` by `hk-catalog render`; do not edit by hand. The TSV is generated from the ignored whole-world import (`.hkpsx/world-import`) by `hk-catalog extract`. Both steps are deterministic and a test (`catalog::tests`) fails when either file drifts. It holds names, counts and byte sizes only, no game assets.\n\n");
        let _ = writeln!(o, "- Scenes: {total} (every BuildSettings scene; none excluded)");
        let _ = writeln!(o, "- Source world fingerprint: `{}`", self.world_fingerprint);
        let _ = writeln!(o, "- Pack matrix inventory fingerprint: `{}`", self.matrix_fingerprint);
        o.push_str("- Regenerate (from the repository root, with the import already done): `cargo run --release --manifest-path host/Cargo.toml -p hk-cook --bin hk-catalog -- all .hkpsx/world-import`\n\n");

        o.push_str("## Cook state\n\n");
        o.push_str("| State | Scenes | Meaning |\n| --- | ---: | --- |\n");
        let meanings = [
            "In `quality::SCENE_TABLE`: on the disc and playable.",
            "An isolated scene pack cooks (decoder and packet bound PASS) but the scene is not admitted to the disc.",
            "The importer found no tilemap, gate or camera envelope to cook views from.",
            "The scene needs more than the 128 views one HKSCNE scene header allows, so its pack fails.",
        ];
        for (s, m) in Status::ALL.into_iter().zip(meanings) {
            let n = self.rows.iter().filter(|r| r.status == s).count();
            let _ = writeln!(o, "| {} | {n} | {m} |", s.label());
        }
        o.push('\n');

        o.push_str("## Areas\n\n");
        o.push_str("Area comes from the scene name prefix (`area_of` in `host/hk-cook/src/catalog.rs`). Fungus3 holds both Fog Canyon and the Queen's Gardens and is not split. `Interior rooms` are the `Room_*` scenes that do not name their area. Enemies are HealthManager objects (bosses, minions and spawners included). Cooked KB is the summed stored pack size of the scenes that cook.\n\n");
        o.push_str(&self.area_table());
        o.push('\n');

        o.push_str("## Scenes without an envelope\n\n");
        o.push_str("These cannot cook as rooms today. The role is the importer's evidence-backed classification.\n\n");
        o.push_str("| # | Scene | Area | Role | Flags |\n| ---: | --- | --- | --- | --- |\n");
        for r in self.rows.iter().filter(|r| r.status == Status::NoEnvelope) {
            let _ = writeln!(o, "| {} | {} | {} | {} | {} |", r.index, r.scene, r.area, r.role, r.flags.join(", "));
        }
        o.push('\n');
        o.push_str("## Scenes over the 128-view limit\n\n");
        o.push_str("| # | Scene | Area | Views needed |\n| ---: | --- | --- | ---: |\n");
        for r in self.rows.iter().filter(|r| r.status == Status::Over128Views) {
            let _ = writeln!(o, "| {} | {} | {} | {} |", r.index, r.scene, r.area, opt(&r.views));
        }
        o.push('\n');

        o.push_str("## Enemy families\n\n");
        o.push_str("A family is a HealthManager object's GameObject name with trailing numbers removed. It is the source naming, not the 136 journal families, and one journal family can span several of these names.\n\n");
        let mut fam: BTreeMap<&str, (u32, u32, BTreeSet<&str>)> = BTreeMap::new();
        for r in &self.rows {
            for (name, n) in &r.families {
                let e = fam.entry(name.as_str()).or_insert((0, 0, BTreeSet::new()));
                e.0 += n;
                e.1 += 1;
                e.2.insert(r.area.as_str());
            }
        }
        let mut fam: Vec<_> = fam.into_iter().collect();
        fam.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.0.cmp(b.0)));
        let _ = writeln!(o, "{} families, {} instances.\n", fam.len(), fam.iter().map(|f| f.1 .0 as u64).sum::<u64>());
        o.push_str("| Family | Instances | Scenes | Areas |\n| --- | ---: | ---: | --- |\n");
        for (name, (n, scenes, areas)) in &fam {
            let areas: Vec<&str> = AREAS.iter().copied().filter(|a| areas.contains(a)).collect();
            let _ = writeln!(o, "| {name} | {n} | {scenes} | {} |", areas.join(", "));
        }
        o.push('\n');

        o.push_str("## Interactables\n\n");
        o.push_str("Counts by serialized component type or PlayMaker FSM name (`class_of`). `persistent` is PersistentBoolItem and PersistentIntItem, the objects the save must remember. `lever` is `Control Lever` plus BridgeLever; `Call Lever` is not counted. The behaviour-class census of every interactable is separate work.\n\n");
        let mut class_totals: BTreeMap<&str, (u32, u32)> = BTreeMap::new();
        for r in &self.rows {
            for (c, n) in &r.interactables {
                let e = class_totals.entry(c.as_str()).or_insert((0, 0));
                e.0 += n;
                e.1 += 1;
            }
        }
        o.push_str("| Class | Instances | Scenes |\n| --- | ---: | ---: |\n");
        for c in INTERACTABLE_CLASSES {
            if let Some((n, s)) = class_totals.get(c) {
                let _ = writeln!(o, "| {c} | {n} | {s} |");
            }
        }
        o.push('\n');

        o.push_str("## Playable scenes\n\n");
        o.push_str("The rows of `SCENE_TABLE`. RAM is the resident scene bytes the arena must hold (arena: 354,032 B), disc is the stored pack, VRAM the atlas bytes.\n\n");
        self.scene_table(&mut o, |r| r.status == Status::Playable);

        for area in AREAS {
            let in_area: Vec<&Row> = self.rows.iter().filter(|r| r.area == area).collect();
            if in_area.is_empty() {
                continue;
            }
            let _ = writeln!(o, "## {area}\n");
            self.scene_table(&mut o, |r| r.area == area);
        }
        o
    }

    fn scene_table(&self, o: &mut String, keep: impl Fn(&Row) -> bool) {
        o.push_str("| # | Scene | State | Views | RAM KB | VRAM KB | Disc KB | Enemies | Top families | Interactables | Gates | Exits | Flags |\n");
        o.push_str("| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | --- | --- | ---: | --- | --- |\n");
        for r in self.rows.iter().filter(|r| keep(r)) {
            let top: Vec<String> = {
                let mut items: Vec<(&String, &u32)> = r.families.iter().collect();
                items.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
                items.iter().take(3).map(|(k, v)| format!("{k} x{v}")).collect()
            };
            let inter: Vec<String> = INTERACTABLE_CLASSES
                .iter()
                .filter_map(|c| r.interactables.get(*c).map(|n| format!("{c} {n}")))
                .collect();
            let kbs = |v: Option<u64>| v.map(|b| kb(b).to_string()).unwrap_or_default();
            let _ = writeln!(
                o,
                "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
                r.index,
                r.scene,
                r.status.label(),
                opt(&r.views),
                kbs(r.ram_bytes),
                kbs(r.vram_bytes),
                kbs(r.disc_bytes),
                r.enemies,
                top.join(", "),
                inter.join(", "),
                r.gates,
                r.exits.join(", "),
                r.flags.join(", ")
            );
        }
        o.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quality::SCENE_TABLE;

    fn root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn committed_tsv() -> String {
        std::fs::read_to_string(root().join("docs/catalog/scenes.tsv")).expect("docs/catalog/scenes.tsv")
    }

    #[test]
    fn family_names_lose_their_numbering() {
        assert_eq!(family_of("Crawler 1"), "Crawler");
        assert_eq!(family_of("Zombie Runner (3)"), "Zombie Runner");
        assert_eq!(family_of("fluke_baby_02"), "fluke_baby");
        assert_eq!(family_of("Mage Blob 1 (2)"), "Mage Blob");
        assert_eq!(family_of("Climber"), "Climber");
        assert_eq!(family_of("42"), "(unnamed)");
        assert_eq!(family_of("A|B=C"), "A B C");
    }

    #[test]
    fn areas_follow_the_scene_prefix() {
        assert_eq!(area_of("Crossroads_10", "gameplay"), "Forgotten Crossroads");
        assert_eq!(area_of("Fungus1_04_boss", "gameplay"), "Greenpath");
        assert_eq!(area_of("Deepnest_East_01", "gameplay"), "Kingdom's Edge");
        assert_eq!(area_of("Deepnest_01", "gameplay"), "Deepnest");
        assert_eq!(area_of("GG_Hornet_1", "boss_arena"), "Godhome");
        assert_eq!(area_of("Menu_Title", "menu"), "Menus, helpers and tests");
        assert_eq!(area_of("Crossroads_10_preload", "transition_or_bootstrap_helper"), "Forgotten Crossroads");
        assert_eq!(area_of("_test_cocoon_1", "gameplay"), "Menus, helpers and tests");
        assert_eq!(area_of("Cinematic_Ending_A", "cinematic"), "Cinematics and openings");
        assert_eq!(area_of("Town", "gameplay"), "Dirtmouth");
        assert_eq!(area_of("Room_shop", "gameplay"), "Interior rooms");
    }

    #[test]
    fn every_class_name_is_listed() {
        for c in ["Breakable", "PersistentBoolItem", "GeoRock", "RestBench", "BridgeLever", "ShopMenuStock", "BossSequenceDoor", "BossStatue"] {
            assert!(INTERACTABLE_CLASSES.contains(&class_of(c, None).unwrap()));
        }
        for f in ["breakable_wall_v2", "unmasker", "Control Lever", "Switch Control", "Toll Gate", "Gate", "Door Control", "Chest Control", "Shiny Control", "soul_totem", "Grub Control", "Lift Control", "Tram Control", "Stag Control", "Conversation Control", "inspect_region", "Battle Control"] {
            assert!(INTERACTABLE_CLASSES.contains(&class_of("", Some(f)).unwrap()), "{f}");
        }
        assert_eq!(class_of("", Some("Call Lever")), None);
    }

    #[test]
    fn tsv_round_trips() {
        let text = committed_tsv();
        let cat = Catalog::from_tsv(&text).unwrap();
        assert_eq!(cat.to_tsv(), text);
    }

    #[test]
    fn committed_catalog_is_whole_and_agrees_with_the_scene_table() {
        let cat = Catalog::from_tsv(&committed_tsv()).unwrap();
        assert_eq!(cat.rows.len(), 501);
        for (i, r) in cat.rows.iter().enumerate() {
            assert_eq!(r.index as usize, i, "rows are in build order");
            assert!(AREAS.contains(&r.area.as_str()), "{}: {}", r.scene, r.area);
        }
        let names: BTreeSet<&str> = cat.rows.iter().map(|r| r.scene.as_str()).collect();
        assert_eq!(names.len(), 501, "scene names are unique");
        let playable: BTreeSet<&str> = cat
            .rows
            .iter()
            .filter(|r| r.status == Status::Playable)
            .map(|r| r.scene.as_str())
            .collect();
        let table: BTreeSet<&str> = SCENE_TABLE.iter().map(|r| r.scene_name).collect();
        assert_eq!(playable, table, "playable rows are exactly SCENE_TABLE");
        for r in &cat.rows {
            let cooked = matches!(r.status, Status::Playable | Status::CookedPack);
            assert_eq!(r.disc_bytes.is_some(), cooked, "{}", r.scene);
            assert_eq!(r.enemies, r.families.values().sum::<u32>(), "{}", r.scene);
        }
    }

    #[test]
    fn the_ledger_carries_the_current_area_table() {
        let cat = Catalog::from_tsv(&committed_tsv()).unwrap();
        let ledger = std::fs::read_to_string(root().join("docs/plans/COMPLETE_GAME_PROGRESS.md")).unwrap();
        assert!(
            ledger.contains(&cat.area_table()),
            "docs/plans/COMPLETE_GAME_PROGRESS.md area table is stale: copy it from docs/CATALOG.md"
        );
    }

    #[test]
    fn committed_markdown_is_rendered_from_the_committed_tsv() {
        let cat = Catalog::from_tsv(&committed_tsv()).unwrap();
        let md = std::fs::read_to_string(root().join("docs/CATALOG.md")).expect("docs/CATALOG.md");
        assert_eq!(cat.render_markdown(), md, "run: hk-catalog render");
    }

    /// Needs the whole-world import, which lives outside Git: set HK_CATALOG_WORLD_IMPORT or have
    /// `.hkpsx/world-import` under the repository root. Skips (and says so) otherwise.
    #[test]
    fn regenerating_from_the_world_import_is_byte_identical() {
        let dir = std::env::var_os("HK_CATALOG_WORLD_IMPORT")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| root().join(".hkpsx/world-import"));
        if !dir.join("world-catalog.json").is_file() {
            eprintln!("skipped: no world import at {}", dir.display());
            return;
        }
        let names: Vec<&str> = SCENE_TABLE.iter().map(|r| r.scene_name).collect();
        let fresh = extract(&dir, &names).unwrap().to_tsv();
        assert_eq!(fresh, committed_tsv(), "the import and the committed catalog disagree; run hk-catalog extract");
    }
}
