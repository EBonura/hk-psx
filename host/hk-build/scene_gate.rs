//! Scene residency checks for the two release-qualified baseline input gaps.
//! Ordinary routes keep using the existing strict scene-gate validator.
use super::{sha256_file, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn word(value: &Value, name: &str) -> Result<u64> {
    value[name]
        .as_u64()
        .ok_or_else(|| format!("missing scene gate word {name}").into())
}

fn source(root: &Path, path: &Value) -> Result<PathBuf> {
    let path = PathBuf::from(path.as_str().ok_or("missing cooked source path")?);
    Ok(if path.is_absolute() {
        path
    } else {
        root.join(path)
    })
}

fn map_buffer(map: &str, name: &str, expected_size: u64) -> Result<usize> {
    let symbol = format!("hk_psx::disc::{name}");
    let row = map
        .lines()
        .find(|line| line.split_whitespace().last() == Some(symbol.as_str()))
        .ok_or_else(|| format!("missing {symbol} map symbol"))?;
    let fields: Vec<_> = row.split_whitespace().collect();
    if fields.len() != 5 || fields[3] != "1" {
        return Err(format!("invalid {symbol} map row").into());
    }
    let size = u64::from_str_radix(fields[2], 16)?;
    if size != expected_size {
        return Err(format!("{symbol} allocation differs from cooked arena").into());
    }
    Ok((usize::from_str_radix(fields[0], 16)?) & 0x1f_ffff)
}

fn resident_bytes(root: &Path, item: &Value) -> Result<Vec<u8>> {
    let path = source(root, &item["resident_raw_path"])?;
    let expected = item["resident_raw_sha256"]
        .as_str()
        .ok_or("missing resident source hash")?;
    if sha256_file(&path)? != expected {
        return Err("resident cooked source changed".into());
    }
    Ok(std::fs::read(path)?)
}

fn watch(
    row: &[&str],
    columns: &HashMap<String, usize>,
    record: &Value,
    name: &str,
) -> Result<u64> {
    let address = record["watches"][name]
        .as_str()
        .ok_or_else(|| format!("missing replay watch {name}"))?;
    let column = format!("ram_{}", address.trim_start_matches("0x"));
    let index = columns.get(&column).ok_or("missing watch CSV column")?;
    Ok(row.get(*index).ok_or("short watch CSV row")?.parse()?)
}

pub fn validate(root: &Path, output: &Path) -> Result<()> {
    let record: Value = serde_json::from_slice(&std::fs::read(output.join("command.json"))?)?;
    let build: Value = serde_json::from_slice(&std::fs::read(output.join("build-report.json"))?)?;
    let baseline = record["faults"] == json!(["HK_INPUT_MISSED_VBLANKS"]);
    if record["completed"] != true
        || record["exit_code"] != 0
        || record["inputs_unchanged"] != true
        || !(baseline || record["faults"] == json!([]))
    {
        return Err("baseline replay receipt is incomplete, changed or has another fault".into());
    }
    let pack = &build["packed_scenes"];
    if pack["residency"] != "scene_gate" {
        return Err("scene residency is not exclusive".into());
    }
    let state = &record["final_ram"];
    let chunk = word(state, "HK_REGION_ID")?;
    if word(state, "HK_GAME_MODE")? != 1 {
        return Err("replay ended outside gameplay".into());
    }
    let scenes = pack["scenes"].as_array().ok_or("missing packed scenes")?;
    let owners: Vec<_> = scenes
        .iter()
        .filter(|scene| {
            scene["source_rooms"].as_array().is_some_and(|rooms| {
                rooms
                    .iter()
                    .any(|room| room["chunk_id"].as_u64() == Some(chunk))
            })
        })
        .collect();
    if owners.len() != 1 {
        return Err("final region does not have one cooked scene owner".into());
    }
    let scene = owners[0];
    let map = std::fs::read_to_string(output.join("game.map"))?;
    let ram = std::fs::read(output.join("ram.bin"))?;
    let raw = resident_bytes(root, scene)?;
    let arena = map_buffer(&map, "BUFFERS", word(pack, "arena_bytes")?)?;
    let address = arena + word(scene, "ram_offset")? as usize;
    if ram.get(address..address + raw.len()) != Some(raw.as_slice()) {
        return Err("resident scene differs from cooked bytes".into());
    }

    let coverage = &build["scene_coverage"];
    let coverage_bytes = {
        let bundles = coverage["bundles"]
            .as_array()
            .ok_or("missing or malformed scene coverage bundles")?;
        let owners: Vec<_> = bundles
            .iter()
            .filter(|item| item["scene_id"] == scene["scene_id"])
            .collect();
        if owners.len() != 1 {
            return Err("coverage bank does not have one scene owner".into());
        }
        let owner = owners[0];
        let raw_path = source(root, &owner["raw_path"])?;
        let bytes = std::fs::read(&raw_path)?;
        if bytes.len() as u64 != word(owner, "raw_len")?
            || sha256_file(&raw_path)?
                != owner["raw_sha256"]
                    .as_str()
                    .ok_or("missing coverage hash")?
            || word(state, "HK_COVERAGE_SCENE")? != word(owner, "scene_index")? + 1
            || word(state, "HK_COVERAGE_BYTES")? != bytes.len() as u64
            || word(state, "HK_COVERAGE_LOADS")? != word(state, "HK_SCENE_LOADS")?
        {
            return Err("coverage owner, length, source or admission count differs".into());
        }
        let at = map_buffer(&map, "COVERAGE_BUFFER", word(coverage, "arena_bytes")?)?;
        if bytes.len() as u64 > word(coverage, "arena_bytes")?
            || ram.get(at..at + bytes.len()) != Some(bytes.as_slice())
        {
            return Err("resident coverage differs from cooked bytes".into());
        }
        Some(bytes.len())
    };
    let gate_loads = word(state, "HK_SCENE_GATE_LOADS")?;
    if gate_loads < 1 || word(state, "HK_SCENE_LOADS")? != gate_loads + 1 {
        return Err("scene replacement/admission count differs".into());
    }
    if word(state, "HK_INPUT_LOADING_TICKS")? == 0 || word(state, "HK_INPUT_LOADING_SAMPLES")? == 0
    {
        return Err("loading input service not observed".into());
    }
    let input_source = std::fs::read_to_string(root.join("game/src/input_queue.rs"))?;
    let capacity = input_source
        .lines()
        .find_map(|line| line.trim().strip_prefix("pub const CAPACITY: usize = "))
        .ok_or("missing input queue capacity")?
        .trim_end_matches(';')
        .parse::<u64>()?;
    if word(state, "HK_INPUT_QUEUE_PEAK")? >= capacity {
        return Err("input queue reached capacity".into());
    }

    let mut regions = HashMap::new();
    for item in scenes {
        for room in item["source_rooms"]
            .as_array()
            .ok_or("scene has no source rooms")?
        {
            regions.insert(word(room, "chunk_id")?, word(item, "scene_id")?);
        }
    }
    let csv = std::fs::read_to_string(output.join("route.csv"))?;
    let mut lines = csv.lines();
    let columns: HashMap<_, _> = lines
        .next()
        .ok_or("empty route CSV")?
        .split(',')
        .enumerate()
        .map(|(index, name)| (name.to_string(), index))
        .collect();
    let sectors = if record["watches"]["HK_SCENE_SECTORS_READ"].is_string() {
        "HK_SCENE_SECTORS_READ"
    } else {
        "HK_CD_SECTORS_READ"
    };
    let mut last: Option<(u64, Option<u64>, Vec<u64>)> = None;
    let mut checked = 0usize;
    let mut max_town_x: Option<f64> = None;
    for line in lines {
        let row: Vec<_> = line.split(',').collect();
        if watch(&row, &columns, &record, "HK_GAME_MODE")? != 1 {
            last = None;
            continue;
        }
        let region = watch(&row, &columns, &record, "HK_REGION_ID")?;
        let scene_id = regions.get(&region).copied();
        if scene_id == Some(1) {
            let x = watch(&row, &columns, &record, "HK_PLAYER_X")? as f64 / 65_536.0;
            max_town_x = Some(max_town_x.map_or(x, |old| old.max(x)));
        }
        let mut counters = vec![
            watch(&row, &columns, &record, sectors)?,
            watch(&row, &columns, &record, "HK_VRAM_UPLOAD_BYTES")?,
        ];
        if coverage_bytes.is_some() {
            for name in [
                "HK_COVERAGE_LOADS",
                "HK_COVERAGE_SCENE",
                "HK_COVERAGE_BYTES",
            ] {
                counters.push(watch(&row, &columns, &record, name)?);
            }
        }
        let snapshot = (
            watch(&row, &columns, &record, "HK_SCENE_GATE_LOADS")?,
            scene_id,
            counters,
        );
        if let Some(previous) = &last {
            if snapshot.0 == previous.0 && snapshot.1 == previous.1 {
                if snapshot.2 != previous.2 {
                    return Err("static scene transfer occurred during gameplay".into());
                }
                checked += 1;
            }
        }
        last = Some(snapshot);
    }
    if checked == 0 {
        return Err("no stable gameplay interval checked".into());
    }
    let result = json!({
        "status": "PASS",
        "route_status": if baseline { "BASELINE" } else { "PASS" },
        "final_scene": scene["scene_id"],
        "resident_bytes_exact": raw.len(),
        "coverage_bytes_exact": coverage_bytes,
        "gate_loads": gate_loads,
        "loading_ticks": state["HK_INPUT_LOADING_TICKS"],
        "loading_samples": state["HK_INPUT_LOADING_SAMPLES"],
        "queue_peak": state["HK_INPUT_QUEUE_PEAK"],
        "max_town_x": max_town_x,
        "stable_gameplay_intervals": checked,
        "limitations": "Emulator replay and memory evidence; no physical-console timing or original scripting parity.",
    });
    std::fs::write(
        output.join("scene-gate-validation.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    Ok(())
}
