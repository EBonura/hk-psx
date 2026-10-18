//! The stag stations' table (`data/stags.rs`): where each station's stag and bell are, which PlayerData
//! bool opens it, what the bell costs, where a passenger arrives, and the texts the stag module shows.
//!
//! The props themselves (grate, bell stand, bell, the stag and its arrival animations) are objects of
//! `interactables.rs`; this table is what `game/src/stag.rs` keeps beside them: the station number the
//! source's `Stag Control` carries (`Station Position Number`), its `Station Opened Bool`, the trigger
//! boxes (`Travel Range`, the bell's own box) and the prompt markers. Names and prices are read from the
//! game's localization sheets (`language.rs`), and the stag's first conversation is wrapped to the panel
//! the way host/read_points.py wraps a tablet.
use crate::breakables::{k, ks};
use crate::common::{components, err, kid, kids, py_max, py_min, variables, Result};
use crate::language::sheet;
use hk_unity::scene::Scene;
use hk_unity::{Source, Value};
use serde_json::Value as J;
use std::path::Path;

/// PlayerData bools the port gives a bit (game/src/persist.rs), by the source's name.
const OPEN_BITS: [(&str, u32); 1] = [("openedCrossroads", 27)];
/// The localization key of a station's name on the stag's map, by `Station Position Number`.
const NAMES: [(i64, &str); 2] = [(0, "MAP_NAME_DIRTMOUTH"), (1, "MAP_NAME_CROSSROADS")];
const PANEL_WIDTH: u32 = 272;
const LINES_PER_PAGE: usize = 8;
const ALWAYS_OPEN: u32 = 255;

fn q16(v: f64) -> i64 {
    crate::common::py_round(v * 65536.0)
}

/// `read_points.py wrap_page`: the panel's advances, words wrapped to the panel width.
fn wrap_page(text: &str, advances: &[u32]) -> Result<Vec<Vec<String>>> {
    let text = text.replace("<br>", "\n").replace(['’', '‘'], "'").replace(['“', '”'], "\"");
    if text.contains('<') || text.contains('>') || text.chars().any(|c| (c as u32) > 126 || ((c as u32) < 32 && c != '\n')) {
        return err(format!("unsupported panel text markup or character in {text:?}"));
    }
    let width = |s: &str| -> u32 { s.bytes().map(|b| advances[(b - 32) as usize]).sum() };
    let mut lines: Vec<String> = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split_whitespace() {
            let candidate = if line.is_empty() { word.to_owned() } else { format!("{line} {word}") };
            if width(&candidate) > PANEL_WIDTH {
                if line.is_empty() || width(word) > PANEL_WIDTH {
                    return err("a word exceeds the panel width");
                }
                lines.push(std::mem::take(&mut line));
                line = word.to_owned();
            } else {
                line = candidate;
            }
        }
        lines.push(line);
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    Ok(lines.chunks(LINES_PER_PAGE).map(<[String]>::to_vec).chain(if lines.is_empty() { Some(vec![]) } else { None }).collect())
}

fn pages(text: &str, advances: &[u32]) -> Result<Vec<Vec<String>>> {
    let mut out = Vec::new();
    for page in text.split("<page>") {
        out.extend(wrap_page(page, advances)?);
    }
    Ok(out)
}

fn rust_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn rust_pages(p: &[Vec<String>]) -> String {
    format!("&[{}]", p.iter().map(|page| format!("&[{}]", page.iter().map(|l| rust_str(l)).collect::<Vec<_>>().join(","))).collect::<Vec<_>>().join(","))
}

fn box_of(sc: &Scene, gid: i64) -> Result<[f64; 4]> {
    let all: Vec<_> = components(sc, gid)?.into_iter().filter(|c| c.1.ends_with("Collider2D")).collect();
    if all.len() != 1 {
        return err("a stag trigger without exactly one collider");
    }
    let polys = crate::common::collider_polygons(sc, gid, all[0].1, all[0].2)?;
    let xs = polys.iter().flatten().map(|p| p.0);
    let ys = polys.iter().flatten().map(|p| p.1);
    Ok([py_min(xs.clone()), py_min(ys.clone()), py_max(xs), py_max(ys)])
}

fn advances(root: &Path) -> Result<Vec<u32>> {
    let text = std::fs::read_to_string(root.join("data/read_points.rs")).map_err(|e| e.to_string())?;
    let at = text.find("ADVANCES:[u8;95]=[").ok_or("data/read_points.rs has no ADVANCES")?;
    let body = &text[at + 18..];
    let body = &body[..body.find(']').ok_or("ADVANCES unterminated")?];
    let v: Vec<u32> = body.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    if v.len() != 95 {
        return err("ADVANCES is not 95 glyphs");
    }
    Ok(v)
}

fn fsm_var(fsm: &Value, name: &str) -> Option<Value> {
    variables(fsm).into_iter().find(|(k, _)| k == name).map(|(_, v)| v)
}

pub fn main(root: &Path, source: Option<&Path>) -> Result<()> {
    let source = match source {
        Some(p) => Source::new(p),
        None => Source::from_doctor(root),
    }
    .map_err(|e| e.to_string())?;
    let report: J = serde_json::from_slice(&std::fs::read(root.join("data/regions.json")).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let regions = report["regions"].as_array().ok_or("regions")?;
    let adv = advances(root)?;
    let prices = sheet(&source, "Prices", "EN")?;
    let ui = sheet(&source, "UI", "EN")?;
    let prompts = sheet(&source, "Prompts", "EN")?;
    let stag = sheet(&source, "Stag", "EN")?;
    let text = |sheet: &[(String, String)], key: &str| -> Result<String> {
        sheet.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()).ok_or_else(|| format!("no {key} in the language sheet"))
    };
    let mut rows = String::new();
    let mut count = 0;
    let mut summary = Vec::new();
    for row in crate::quality::SCENE_TABLE.iter() {
        let sc = Scene::new(&source, row.file).map_err(|e| e.to_string())?;
        let find = |fsm: &str| -> Option<(i64, &Value)> {
            sc.objects.iter().find(|o| o.typename == "PlayMakerFSM" && o.tree.get("fsm").and_then(|f| f.get("name")).and_then(Value::str).as_deref() == Some(fsm)).and_then(
                |o| Some((crate::common::go_of(&o.tree)?, o.tree.get("fsm")?)),
            )
        };
        let Some((body, stag_fsm)) = find("Stag Control") else { continue };
        let (box_, bell_fsm) = find("Stag Bell").ok_or("a stag station without its Stag Bell")?;
        let number = fsm_var(stag_fsm, "Station Position Number").and_then(|v| v.int()).ok_or("Station Position Number")?;
        let open_name = fsm_var(stag_fsm, "Station Opened Bool").and_then(|v| v.str()).ok_or("Station Opened Bool")?;
        let default = source.fresh_save().map_err(|e| e.to_string())?.get(&open_name);
        let open_bit = match default {
            Some(hk_unity::fresh_save::Start::Int(1)) => ALWAYS_OPEN,
            Some(hk_unity::fresh_save::Start::Int(0)) => OPEN_BITS
                .iter()
                .find(|(n, _)| *n == open_name)
                .map(|(_, b)| *b)
                .ok_or_else(|| format!("{open_name} starts false and has no PlayerData bit in stags.rs"))?,
            other => return err(format!("{open_name} starts as {other:?}")),
        };
        let cost = match fsm_var(bell_fsm, "Price String Name").and_then(|v| v.str()) {
            Some(key) if !key.is_empty() && open_bit != ALWAYS_OPEN => text(&prices, &key)?.parse::<u32>().map_err(|e| e.to_string())?,
            _ => 0,
        };
        let body_kids = kids(&sc, body)?;
        let box_kids = kids(&sc, box_)?;
        let range = box_of(&sc, kid(&body_kids, "Travel Range").ok_or("no Travel Range")?)?;
        let inspect = box_of(&sc, box_)?;
        let at = |gid: i64| sc.point(gid, 0.0, 0.0, 0.0).map_err(|e| e.to_string());
        let stag_marker = at(kid(&body_kids, "Prompt Marker Trv").ok_or("no Prompt Marker Trv")?)?;
        let bell_marker = at(kid(&box_kids, "Prompt Marker").ok_or("no Prompt Marker")?)?;
        // A passenger arrives on the platform in front of the stag, which is where the Travel Range
        // stands, and drops onto the floor from a little above it.
        let spawn = [(range[0] + range[2]) / 2.0, range[1] + 0.8];
        let region = regions
            .iter()
            .position(|r| {
                r["scene_id"].as_u64() == Some(row.scene_id as u64)
                    && r["activation_bounds"].as_array().is_some_and(|b| {
                        let v: Vec<f64> = b.iter().filter_map(J::as_f64).collect();
                        v.len() == 4 && v[0] <= spawn[0] && spawn[0] <= v[2] && v[1] <= spawn[1] && spawn[1] <= v[3]
                    })
            })
            .ok_or_else(|| format!("no view of {} holds the arrival point {spawn:?}", row.scene_name))?;
        let name_key = NAMES.iter().find(|(n, _)| *n == number).map(|(_, k)| *k).ok_or_else(|| format!("no name key for station {number}"))?;
        let name = wrap_text_line(&text(&ui, name_key)?)?;
        let _ = k;
        let _ = ks;
        rows.push_str(&format!(
            "Station{{scene:{},number:{number},open_bit:{open_bit},cost:{cost},range:[{}],inspect:[{}],stag_marker:[{},{}],bell_marker:[{},{}],spawn:[{},{}],region:{region},name:{}}},",
            row.scene_id,
            range.iter().map(|v| q16(*v).to_string()).collect::<Vec<_>>().join(","),
            inspect.iter().map(|v| q16(*v).to_string()).collect::<Vec<_>>().join(","),
            q16(stag_marker[0]),
            q16(stag_marker[1]),
            q16(bell_marker[0]),
            q16(bell_marker[1]),
            q16(spawn[0]),
            q16(spawn[1]),
            rust_str(&name)
        ));
        count += 1;
        summary.push(format!("{} (station {number}, opens on {open_name} bit {open_bit}, bell costs {cost}, arrival {spawn:?} in view {region})", row.scene_name));
    }
    let meet = pages(&text(&stag, "STAG_MEET")?, &adv)?;
    let toll = pages(&text(&prompts, "TOLLBOOTH_STAG")?, &adv)?;
    let one = |p: &[Vec<String>]| -> Result<String> {
        match p {
            [page] => Ok(format!("&[{}]", page.iter().map(|l| rust_str(l)).collect::<Vec<_>>().join(","))),
            _ => err("the toll prompt is more than one page"),
        }
    };
    let lines = [
        "// Generated by host/hk-cook/src/stags.rs from the stag stations' FSMs and the localization sheets.".to_owned(),
        "#[derive(Clone, Copy)] pub struct Station {pub scene:u8,pub number:u8,pub open_bit:u8,pub cost:u16,pub range:[i32;4],pub inspect:[i32;4],pub stag_marker:[i32;2],pub bell_marker:[i32;2],pub spawn:[i32;2],pub region:u16,pub name:&'static str}".to_owned(),
        "/// `open_bit` 255: the station is open on a new game (`openedTown`).".to_owned(),
        format!("pub static STATIONS:[Station;{count}]=[{rows}];"),
        "/// `STAG_MEET`, the stag's first conversation, as panel pages.".to_owned(),
        format!("pub static MEET:&[&[&str]]={};", rust_pages(&meet)),
        "/// `TOLLBOOTH_STAG`, the bell's question.".to_owned(),
        format!("pub static TOLL:&[&str]={};", one(&toll)?),
        format!("pub const YES:&str={};", rust_str(&text(&prompts, "YES")?)),
        format!("pub const NO:&str={};", rust_str(&text(&prompts, "NO")?)),
        format!("pub const NOT_ENOUGH_GEO:&str={};", rust_str(&text(&prompts, "NOT_ENOUGH_GEO")?)),
        format!("pub const LISTEN:&str={};", rust_str(&capitalise(&text(&prompts, "LISTEN")?))),
        format!("pub const TRAVEL:&str={};", rust_str(&capitalise(&text(&prompts, "TRAVEL")?))),
        format!("pub const INSPECT:&str={};", rust_str(&capitalise(&text(&prompts, "INSPECT")?))),
        String::new(),
    ];
    std::fs::write(root.join("data/stags.rs"), lines.join("\n")).map_err(|e| e.to_string())?;
    println!("{} stag stations: {}", count, summary.join("; "));
    Ok(())
}

/// The prompt markers spell a label with a capital and lower case after it ("Listen").
fn capitalise(s: &str) -> String {
    let lower = s.to_lowercase();
    let mut c = lower.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// A label on one line of the panel: ASCII only.
fn wrap_text_line(s: &str) -> Result<String> {
    if s.chars().any(|c| !(' '..='~').contains(&c)) {
        return err(format!("{s:?} is not panel text"));
    }
    Ok(s.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat() -> Vec<u32> {
        vec![6; 95]
    }

    #[test]
    fn text_wraps_at_the_panel_width_and_pages_at_the_authored_break() {
        // 272 / 6 = 45 glyphs a line.
        let p = pages("aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii jjjj kkkk<page>second", &flat()).unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!(p[0], ["aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii", "jjjj kkkk"]);
        assert_eq!(p[1], ["second"]);
    }

    #[test]
    fn a_page_of_more_than_eight_lines_continues_on_the_next() {
        let long = (0..20).map(|i| format!("{i:0>40}")).collect::<Vec<_>>().join(" ");
        let p = wrap_page(&long, &flat()).unwrap();
        assert_eq!(p.iter().map(Vec::len).collect::<Vec<_>>(), [8, 8, 4]);
    }

    #[test]
    fn curly_quotes_become_straight_and_other_marks_are_refused() {
        assert_eq!(wrap_page("it’s", &flat()).unwrap(), [["it's"]]);
        assert!(wrap_page("a <b> c", &flat()).is_err());
        assert!(wrap_page("café", &flat()).is_err());
    }

    #[test]
    fn labels_are_capitalised_like_the_prompt_markers() {
        assert_eq!(capitalise("LISTEN"), "Listen");
    }
}
