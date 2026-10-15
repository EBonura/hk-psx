//! Census of PlayMaker actions in enemy and boss FSMs (docs/FSM-V2-DESIGN.md section 1).
//!
//! fsm-census ORIGINAL_WINDOWS_DATA SOURCE_OBJECTS_JSONL_GZ OUTPUT_DIR
//!
//! Writes census.md (every table of the design) and census.json (the raw counts the
//! tables derive from). Retail files and the cache are read only.
use hk_fsm_cook::census::{load_fsms, load_roots, Fsm, Root};
use hk_fsm_cook::families::{family, FAMILIES};
use hk_unity::Source;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write;

/// Action types hk-sim's v1 VM implements today (the "v1" column).
const V1: [&str; 16] = [
    "GetOwner", "FindGameObject", "BoolTest", "GetDistance", "FloatCompare", "FloatSwitch", "SetBoolValue",
    "SetFloatValue", "RandomFloat", "FaceDirection", "SetVelocityAsAngle", "CheckCollisionSide",
    "CheckCollisionSideEnter", "Wait", "Translate", "WalkLeftRight",
];
/// (label, scene, GameObject) in the order the design brings them up. The first
/// 23 rows are the boss table; all 24 entries after the starters are "the bosses".
const STARTERS: [(&str, &str, &str); 3] = [
    ("Crawler", "Crossroads_05", "Crawler"),
    ("Gruzzer", "Crossroads_04", "Fly"),
    ("Vengefly", "Fungus1_20_v02", "Buzzer"),
];
const BOSSES: [(&str, &str, &str); 24] = [
    ("Hornet 1", "Fungus1_04_boss", "Hornet Boss 1"),
    ("Mantis Lords", "Fungus2_15_boss", "Mantis Lord"),
    ("Soul Master", "Ruins1_24_boss", "Mage Lord"),
    ("Collector", "Ruins2_11_boss", "Jar Collector"),
    ("Watcher Knights", "Ruins2_03_boss", "Black Knight 1"),
    ("Dung Defender", "Waterways_05_boss", "Dung Defender"),
    ("Flukemarm", "Waterways_12_boss", "Fluke Mother"),
    ("Uumuu", "Fungus3_archive_02_boss", "Mega Jellyfish"),
    ("Traitor Lord", "Fungus3_23_boss", "Mantis Traitor Lord"),
    ("Crystal Guardian", "Mines_18_boss", "Mega Zombie Beam Miner (1)"),
    ("Nosk", "Deepnest_32", "Mimic Spider"),
    ("Hive Knight", "Hive_05", "Hive Knight"),
    ("Hornet 2", "Deepnest_East_Hornet_boss", "Hornet Boss 2"),
    ("Broken Vessel", "Abyss_19", "Infected Knight"),
    ("Grimm", "Grimm_Main_Tent_boss", "Grimm Boss"),
    ("White Defender", "Dream_04_White_Defender", "White Defender"),
    ("Radiance", "Dream_Final_Boss", "Radiance"),
    ("Hollow Knight", "Room_Final_Boss_Core", "Hollow Knight Boss"),
    ("False Knight (native today)", "Crossroads_10_boss", "False Knight New"),
    ("Gruz Mother (native today)", "Crossroads_04", "Giant Fly"),
    ("Brooding Mawlek", "Crossroads_09", "Mawlek Body"),
    ("Vengefly King", "Fungus1_20_v02", "Giant Buzzer"),
    ("Lost Kin", "Dream_03_Infected_Knight", "Lost Kin"),
    ("Grey Prince", "Dream_Mighty_Zote", "Grey Prince"),
];

type Counts = BTreeMap<String, u32>;

struct Corpus {
    roots: Vec<Root>,
    fsm: HashMap<String, Fsm>,
}

impl Corpus {
    fn counts(&self, sid: &str) -> Counts {
        let mut c = Counts::new();
        for s in &self.fsm[sid].states {
            for a in &s.enabled {
                *c.entry(a.clone()).or_default() += 1;
            }
        }
        c
    }
    fn tree_types(&self, r: &Root) -> BTreeSet<String> {
        r.fsms.iter().flat_map(|(_, sid, _)| self.counts(sid).into_keys()).collect()
    }
    fn root_types(&self, r: &Root) -> BTreeSet<String> {
        r.fsms.iter().filter(|(g, _, _)| *g == r.go).flat_map(|(_, sid, _)| self.counts(sid).into_keys()).collect()
    }
    fn pick(&self, scene: &str, name: &str) -> Option<&Root> {
        self.roots.iter().find(|r| r.scene == scene && r.name == name)
    }
}

fn pct(n: f64, d: f64) -> String {
    format!("{:.1}%", 100.0 * n / d)
}

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let a: Vec<_> = std::env::args().collect();
    if a.len() != 4 {
        return Err("usage: fsm-census ORIGINAL_WINDOWS_DATA SOURCE_OBJECTS_JSONL_GZ OUTPUT_DIR".into());
    }
    let threads = std::thread::available_parallelism().map_or(2, |n| n.get().min(4));
    let source = Source::new(&a[1])?;
    let t0 = std::time::Instant::now();
    let roots = load_roots(&source, std::path::Path::new(&a[2]), threads)?;
    eprintln!("{} HealthManager placements in {:?}", roots.len(), t0.elapsed());
    let mut sids: Vec<String> = roots.iter().flat_map(|r| r.fsms.iter().map(|f| f.1.clone())).collect();
    sids.sort();
    sids.dedup();
    let (fsm, errors) = load_fsms(&source, &sids, threads)?;
    eprintln!("{} FSM objects read, {} errors, {:?}", fsm.len(), errors.len(), t0.elapsed());
    let c = Corpus { roots, fsm };
    let mut md = String::new();
    let mut js = serde_json::Map::new();

    // ---- 1.1 what was counted
    let main: Vec<&Root> = c.roots.iter().filter(|r| r.is_main()).collect();
    let refs: usize = c.roots.iter().map(|r| r.fsms.len()).sum();
    let mut inst: Counts = Counts::new();
    let mut inst_distinct: Counts = Counts::new();
    let mut defuse: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut rootuse: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
    // First appearance (root, FSM reference, state, action) breaks rank ties: the order
    // the original scratch census used, kept so the design's tables reproduce.
    let mut first: HashMap<String, usize> = HashMap::new();
    for (i, r) in c.roots.iter().enumerate() {
        for (_, sid, _) in &r.fsms {
            for s in &c.fsm[sid].states {
                for a in &s.enabled {
                    let n = first.len();
                    first.entry(a.clone()).or_insert(n);
                }
            }
            for (t, n) in c.counts(sid) {
                *inst.entry(t.clone()).or_default() += n;
                defuse.entry(t.clone()).or_default().insert(c.fsm[sid].def.clone());
                rootuse.entry(t).or_default().insert(i);
            }
        }
    }
    for f in c.fsm.values() {
        for s in &f.states {
            for t in &s.enabled {
                *inst_distinct.entry(t.clone()).or_default() += 1;
            }
        }
    }
    let defs: BTreeMap<&str, &Fsm> = c.fsm.values().map(|f| (f.def.as_str(), f)).collect();
    let total_inst: u32 = inst.values().sum();
    let with_actions = |r: &&Root, root_only: bool| {
        let s = if root_only { c.root_types(r) } else { c.tree_types(r) };
        !s.is_empty()
    };
    let main_tree_actions = main.iter().filter(|r| with_actions(r, false)).count();
    let main_root_actions = main.iter().filter(|r| with_actions(r, true)).count();
    writeln!(md, "## 1.1 What was counted\n")?;
    writeln!(md, "| | count |\n|---|---:|")?;
    writeln!(md, "| HealthManager roots (enemy and boss placements, incl. prefabs) | {} |", c.roots.len())?;
    writeln!(md, "| of which outside GG_*, Dream_* and prefabs (\"main game\") | {} |", main.len())?;
    writeln!(md, "| FSM instances on those trees (distinct objects) | {} |", c.fsm.len())?;
    writeln!(md, "| FSM references (an FSM under nested HealthManagers counts per root) | {refs} |")?;
    writeln!(md, "| distinct FSM definitions (hash of name, state/action order, events) | {} |", defs.len())?;
    writeln!(md, "| distinct enabled action types | {} |", inst.len())?;
    writeln!(md, "| enabled action instances (per root reference, the table basis) | {total_inst} |")?;
    writeln!(md, "| enabled action instances (distinct FSM objects) | {} |", inst_distinct.values().sum::<u32>())?;
    writeln!(md, "| main-game roots with at least one enabled action on the root GameObject's own FSMs | {main_root_actions} |")?;
    writeln!(md, "| main-game roots with at least one enabled action anywhere in the tree | {main_tree_actions} |")?;
    writeln!(md, "| unreadable FSM objects | {} |", errors.len())?;
    let unmapped: Vec<_> = inst.keys().filter(|t| family(t) == "other").collect();
    writeln!(md, "| action types missing from the family map | {} {:?} |\n", unmapped.len(), unmapped)?;
    if let Some(h) = c.pick("Fungus1_04_boss", "Hornet Boss 1") {
        for (_, sid, _) in &h.fsms {
            let f = &c.fsm[sid];
            if f.name == "Control" && f.states.len() > 50 {
                writeln!(md, "Cross-check: `Hornet Boss 1 Control` has {} states, {} enabled actions, {} in the widest state, {} variables, {} distinct action types.\n", f.states.len(), f.enabled_total(), f.max_state_actions(), f.var_total(), c.counts(sid).len())?;
            }
        }
    }

    // ---- ranking: FSM definitions using the type, ties by first appearance
    let boss_sets: Vec<(&str, BTreeSet<String>, &Root)> = BOSSES
        .iter()
        .filter_map(|(l, s, n)| c.pick(s, n).map(|r| (*l, c.tree_types(r), r)))
        .collect();
    let nb = boss_sets.len();
    let bossuse = |t: &str| boss_sets.iter().filter(|(_, s, _)| s.contains(t)).count();
    let mut rank: Vec<&String> = inst.keys().collect();
    rank.sort_by(|a, b| {
        defuse[*b].len().cmp(&defuse[*a].len()).then(first[*a].cmp(&first[*b]))
    });

    // ---- 1.2 coverage curve
    writeln!(md, "## 1.2 Vocabulary coverage by rank\n")?;
    writeln!(md, "| Types supported (by FSM-definition rank) | share of all enabled action instances | FSM definitions fully runnable (of {}) | main-game enemy trees fully runnable (of {}) |\n|---:|---:|---:|---:|", defs.len(), main_tree_actions)?;
    let dt: Vec<BTreeSet<String>> = main.iter().map(|r| c.tree_types(r)).filter(|s| !s.is_empty()).collect();
    let dr: Vec<BTreeSet<String>> = main.iter().map(|r| c.root_types(r)).filter(|s| !s.is_empty()).collect();
    let def_types: Vec<BTreeSet<String>> = defs.values().map(|f| f.states.iter().flat_map(|s| s.enabled.iter().cloned()).collect()).collect();
    let mut curve = vec![];
    for n in [16usize, 32, 64, 96, 104, 128, 160, 200, 250, 338] {
        let s: BTreeSet<String> = rank.iter().take(n).map(|t| (*t).clone()).collect();
        let share: u32 = inst.iter().filter(|(t, _)| s.contains(*t)).map(|(_, n)| *n).sum();
        let d = def_types.iter().filter(|x| x.is_subset(&s)).count();
        let tr = dt.iter().filter(|x| x.is_subset(&s)).count();
        let ro = dr.iter().filter(|x| x.is_subset(&s)).count();
        writeln!(md, "| {n} | {} | {d} | {tr} |", pct(share as f64, total_inst as f64))?;
        curve.push(serde_json::json!({"types":n,"share":share as f64/total_inst as f64,"defs":d,"trees":tr,"root_fsms_only":ro}));
    }
    js.insert("curve".into(), curve.into());
    // the first four consumers
    let mut closure: BTreeSet<String> = BTreeSet::new();
    for (l, s, n) in STARTERS.iter().chain(BOSSES.iter().take(1)) {
        let _ = l;
        if let Some(r) = c.pick(s, n) {
            closure.extend(c.tree_types(r));
        }
    }
    let cshare: u32 = inst.iter().filter(|(t, _)| closure.contains(*t)).map(|(_, n)| *n).sum();
    writeln!(md, "\nClosure of Crawler, Gruzzer, Vengefly, Hornet 1: {} types, {} of instances, {} of {} FSM definitions fully, {} of {} main-game enemy trees ({} counting root-GameObject FSMs only).\n",
        closure.len(), pct(cshare as f64, total_inst as f64),
        def_types.iter().filter(|x| x.is_subset(&closure)).count(), defs.len(),
        dt.iter().filter(|x| x.is_subset(&closure)).count(), dt.len(), dr.iter().filter(|x| x.is_subset(&closure)).count())?;
    let all_boss: BTreeSet<String> = boss_sets.iter().flat_map(|(_, s, _)| s.iter().cloned()).collect();
    let mut with_v1 = all_boss.clone();
    with_v1.extend(V1.iter().map(|t| t.to_string()).filter(|t| inst.contains_key(t)));
    let mut with_starters = all_boss.clone();
    for (_, s, n) in STARTERS {
        if let Some(r) = c.pick(s, n) {
            with_starters.extend(c.tree_types(r));
        }
    }
    writeln!(md, "Union of action types over the {nb} listed bosses: {}; with the v1 types: {}; with the three starters: {}.\n", all_boss.len(), with_v1.len(), with_starters.len())?;

    // ---- 1.3 families
    writeln!(md, "## 1.3 Families\n\n| family | types (of {}) | enabled instances | share | types in Hornet 1 | types used by any of {nb} bosses |\n|---|---:|---:|---:|---:|---:|", inst.len())?;
    let hornet = boss_sets.iter().find(|(l, _, _)| *l == "Hornet 1").map(|(_, s, _)| s.clone()).unwrap_or_default();
    let mut fam_rows: Vec<(String, usize, u32, usize, usize)> = vec![];
    for f in FAMILIES {
        let ts: Vec<&String> = inst.keys().filter(|t| family(t) == f).collect();
        fam_rows.push((f.to_owned(), ts.len(), ts.iter().map(|t| inst[*t]).sum(), ts.iter().filter(|t| hornet.contains(**t)).count(), ts.iter().filter(|t| bossuse(t) > 0).count()));
    }
    fam_rows.sort_by(|a, b| b.2.cmp(&a.2));
    for (f, n, i, h, b) in &fam_rows {
        writeln!(md, "| {f} | {n} | {i} | {} | {h} | {b} |", pct(*i as f64, total_inst as f64))?;
    }

    // ---- 1.4 bosses
    writeln!(md, "\n## 1.4 What each boss needs\n\n| boss (scene) | FSMs | states (largest FSM) | enabled actions | max actions/state | variables | GameObjects | types | types beyond v1 | new vs earlier rows |\n|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|")?;
    let v1: BTreeSet<&str> = V1.into_iter().collect();
    let mut base: BTreeSet<String> = BTreeSet::new();
    let mut boss_rows = vec![];
    for (l, s, n) in STARTERS.iter().chain(BOSSES.iter().take(20)) {
        let Some(r) = c.pick(s, n) else {
            writeln!(md, "| {l} ({s}) | MISSING |")?;
            continue;
        };
        let fs: Vec<&Fsm> = r.fsms.iter().map(|(_, sid, _)| &c.fsm[sid]).collect();
        let ts = c.tree_types(r);
        let new = ts.difference(&base).count();
        base.extend(ts.iter().cloned());
        let acts: usize = fs.iter().map(|f| f.enabled_total()).sum();
        let row = (fs.len(), fs.iter().map(|f| f.states.len()).sum::<usize>(), fs.iter().map(|f| f.states.len()).max().unwrap_or(0), acts,
            fs.iter().map(|f| f.max_state_actions()).max().unwrap_or(0), fs.iter().map(|f| f.var_total()).sum::<usize>(), r.ngo, ts.len(),
            ts.iter().filter(|t| !v1.contains(t.as_str())).count(), new);
        writeln!(md, "| {l} ({s}) | {} | {} ({}) | {} | {} | {} | {} | {} | {} | {} |", row.0, row.1, row.2, row.3, row.4, row.5, row.6, row.7, row.8, row.9)?;
        boss_rows.push(serde_json::json!({"boss":l,"scene":s,"fsms":row.0,"states":row.1,"largest":row.2,"actions":row.3,"max_state_actions":row.4,"variables":row.5,"gameobjects":row.6,"types":row.7,"beyond_v1":row.8,"new":row.9}));
    }
    js.insert("bosses".into(), boss_rows.into());
    // hard limits
    writeln!(md, "\nHard limits (over the {nb} listed bosses | over every FSM read):\n")?;
    let bf: Vec<(&str, Vec<&Fsm>, &Root)> = boss_sets.iter().map(|(l, _, r)| (*l, r.fsms.iter().map(|(_, sid, _)| &c.fsm[sid]).collect(), *r)).collect();
    let mx = |f: &dyn Fn(&Fsm) -> usize| (bf.iter().flat_map(|(_, v, _)| v.iter().map(|x| f(x))).max().unwrap_or(0), c.fsm.values().map(f).max().unwrap_or(0));
    let l = mx(&|f| f.states.len());
    writeln!(md, "- states in one FSM: {} | {}", l.0, l.1)?;
    let l = mx(&|f| f.events);
    writeln!(md, "- events in one FSM: {} | {}", l.0, l.1)?;
    let l = mx(&|f| f.max_state_actions());
    writeln!(md, "- enabled actions in one state: {} | {}", l.0, l.1)?;
    let l = mx(&|f| f.var_total());
    writeln!(md, "- variables in one FSM: {} | {}", l.0, l.1)?;
    let tv = bf.iter().map(|(l, v, _)| (v.iter().map(|f| f.var_total()).sum::<usize>(), *l)).max().unwrap_or((0, ""));
    writeln!(md, "- variables in one boss tree: {} ({})", tv.0, tv.1)?;
    let tg = bf.iter().map(|(l, _, r)| (r.ngo, *l)).max().unwrap_or((0, ""));
    writeln!(md, "- GameObjects in one boss tree: {} ({})", tg.0, tg.1)?;
    let tf = bf.iter().map(|(l, v, _)| (v.len(), *l)).max().unwrap_or((0, ""));
    writeln!(md, "- FSMs in one boss tree: {} ({})", tf.0, tf.1)?;
    let over_vars = defs.values().filter(|f| f.var_total() > 16).count();
    let over_acts = defs.values().filter(|f| f.max_state_actions() > 16).count();
    writeln!(md, "- FSM definitions beyond v1's limits: {over_vars} on variables (16), {over_acts} on actions per state (16) of {}", defs.len())?;
    // variable classes
    writeln!(md, "\nVariable classes by tree (runtime bytes: 4 per float/int, 1 bit per bool, 8 per vec2, 12 per vec3, 1 per GameObject handle, 16 per colour; strings become ids):\n")?;
    for (l, v, _) in &bf {
        let mut t: BTreeMap<&str, usize> = BTreeMap::new();
        for f in v {
            for (k, n) in &f.vars {
                *t.entry(k.as_str()).or_default() += n;
            }
        }
        let g = |k: &str| t.get(k).copied().unwrap_or(0);
        let bytes = 4 * g("floatVariables") + 4 * g("intVariables") + g("boolVariables").div_ceil(8) + 8 * g("vector2Variables") + 12 * g("vector3Variables") + g("gameObjectVariables") + 16 * g("colorVariables");
        if ["Hornet 1", "Hornet 2", "Hollow Knight", "Radiance", "Grimm", "Broken Vessel", "Crawler"].contains(l) {
            writeln!(md, "- {l}: {t:?} => {bytes} B")?;
        }
    }

    // ---- appendix A
    writeln!(md, "\n## Appendix A: top 80 action types by coverage\n\n| # | action | family | enabled instances | FSM defs (of {}) | enemy roots (of {}) | bosses (of {nb}) | v1 | Hornet 1 |\n|---:|---|---|---:|---:|---:|---:|:-:|:-:|", defs.len(), c.roots.len())?;
    for (i, t) in rank.iter().take(80).enumerate() {
        writeln!(md, "| {} | {t} | {} | {} | {} | {} | {} | {} | {} |", i + 1, family(t), inst[*t], defuse[*t].len(), rootuse[*t].len(), bossuse(t), if v1.contains(t.as_str()) { "x" } else { "" }, if hornet.contains(*t) { "x" } else { "" })?;
    }

    std::fs::create_dir_all(&a[3])?;
    std::fs::write(format!("{}/census.md", a[3]), &md)?;
    js.insert("rank".into(), rank.iter().map(|t| serde_json::json!({"action":t,"instances":inst[*t],"defs":defuse[*t].len(),"roots":rootuse[*t].len(),"bosses":bossuse(t),"family":family(t)})).collect::<Vec<_>>().into());
    std::fs::write(format!("{}/census.json", a[3]), serde_json::to_vec_pretty(&js)?)?;
    println!("{md}");
    Ok(())
}
