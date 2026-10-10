//! Normal-controls Focus proof against the packaged disc.
use super::{ram_word, sha256_file, Result};
use serde_json::{json, Map, Value};
use std::path::Path;
use std::process::Command;

const TAPE: &str = "tools/tapes/earned-focus.pxtape";
const ROUTE: &str = "tools/tapes/earned-focus.route";
const CAPTURES: &[(&str, u32)] = &[("charging", 2160), ("flash", 2220), ("healed", 2400)];
const SYMBOLS: &[&str] = &[
    "HK_GAME_MODE",
    "HK_CHEATS",
    "HK_DEATHS",
    "HK_HEALTH",
    "HK_SOUL",
    "HK_ENEMY_HITS",
    "HK_ENEMY_KILLS",
    "HK_FOCUS_STARTED",
    "HK_FOCUS_COMPLETED",
    "HK_FOCUS_HEALED",
    "HK_FOCUS_DRAINED",
    "HK_FOCUS_REFUNDED",
    "HK_FOCUS_LOCKED",
    "__psx_rt_fault_count",
    "HK_ROOM_LOAD_ERROR",
];

fn require(state: &Map<String, Value>, name: &str, expected: u32) -> Result<()> {
    let actual = state[name]
        .as_u64()
        .ok_or(format!("missing Focus marker {name}"))?;
    if actual != u64::from(expected) {
        return Err(format!("{name}: expected {expected}, got {actual}").into());
    }
    Ok(())
}

pub(super) fn validate(root: &Path, frontend: &Path, library: &Path, out: &Path) -> Result<()> {
    let build: Value = serde_json::from_slice(&std::fs::read(root.join(".hkpsx/build.json"))?)?;
    let outputs = build["outputs"]
        .as_object()
        .ok_or("build report has no output hashes")?;
    for (path, meta) in outputs {
        let wanted = meta["sha256"].as_str().ok_or("build output has no hash")?;
        if sha256_file(Path::new(path))? != wanted {
            return Err(format!("build output changed since report: {path}").into());
        }
    }
    let map_path = build["link_map"]["path"]
        .as_str()
        .ok_or("build report has no map")?;
    let map_hash = build["link_map"]["sha256"]
        .as_str()
        .ok_or("build map has no hash")?;
    if sha256_file(Path::new(map_path))? != map_hash {
        return Err("build map changed since report".into());
    }
    let cue = library.join("hk-psx.cue");
    let bin = library.join("hk-psx.bin");
    let cue_hash = sha256_file(&cue)?;
    let bin_hash = sha256_file(&bin)?;
    let reported_hash = |kind: &str| -> Result<&str> {
        let path = build["artifacts"][kind]
            .as_str()
            .ok_or(format!("build report has no {kind} artifact"))?;
        outputs
            .get(path)
            .and_then(|meta| meta["sha256"].as_str())
            .ok_or_else(|| format!("build report has no {kind} output hash").into())
    };
    if cue_hash != reported_hash("cue")? {
        return Err("selected Focus CUE differs from the build report".into());
    }
    if bin_hash != reported_hash("bin")? {
        return Err("selected Focus disc differs from the build report".into());
    }
    // The build may append CDDA FILE entries after the data track. Its first
    // FILE must still point to the verified BIN in this selected library.
    let cue_text = std::fs::read_to_string(&cue)?;
    let data_file = cue_text
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("FILE "))
        .ok_or("selected Focus CUE has no data FILE")?;
    if data_file != "FILE \"hk-psx.bin\" BINARY" {
        return Err(
            format!("selected Focus CUE points at a different data BIN: {data_file}").into(),
        );
    }
    let tape = root.join(TAPE);
    let mut states = Map::new();
    let mut commands = Map::new();
    std::fs::create_dir_all(out)?;
    for &(label, poll) in CAPTURES {
        let capture = out.join(label);
        std::fs::create_dir_all(&capture)?;
        let args = vec![
            "launch".to_string(),
            "--path".into(),
            cue.display().to_string(),
            "--embedded-playtest".into(),
            "--config-dir".into(),
            capture.join("emulator").display().to_string(),
            "--steps".into(),
            "6000000000".into(),
            "--input-tape".into(),
            tape.display().to_string(),
            "--stop-at-poll".into(),
            poll.to_string(),
            "--dump-display".into(),
            capture.join("cpu.ppm").display().to_string(),
            "--dump-hw".into(),
            capture.join("hw.ppm").display().to_string(),
            "--dump-ram".into(),
            capture.join("ram.bin").display().to_string(),
        ];
        let result = Command::new(frontend)
            .args(&args)
            .current_dir(root)
            .output()?;
        let log = [result.stdout.as_slice(), result.stderr.as_slice()].concat();
        std::fs::write(capture.join("replay.log"), &log)?;
        if !result.status.success() {
            return Err(format!(
                "earned Focus {label} replay failed: {}",
                String::from_utf8_lossy(&log)
            )
            .into());
        }
        let log_text = String::from_utf8_lossy(&log);
        // A video tick can carry more than one pad read. The frontend stops
        // after that tick, so its observed poll count can pass the requested
        // bound by a small number. Record the actual count in the receipt.
        let observed = log_text
            .split_whitespace()
            .find_map(|word| {
                word.strip_prefix("port1-polls=")
                    .and_then(|n| n.parse::<u32>().ok())
            })
            .ok_or("earned Focus replay omitted its poll count")?;
        if observed < poll || observed > poll + 3 {
            return Err(
                format!("earned Focus {label} requested poll {poll}, observed {observed}").into(),
            );
        }
        let mut state = Map::new();
        for &symbol in SYMBOLS {
            state.insert(symbol.into(), json!(ram_word(root, &capture, symbol)?));
        }
        for &(symbol, value) in &[
            ("HK_GAME_MODE", 1),
            ("HK_CHEATS", 0),
            ("HK_DEATHS", 0),
            ("HK_ENEMY_HITS", 4),
            ("HK_ENEMY_KILLS", 2),
            ("HK_FOCUS_STARTED", 1),
            ("HK_FOCUS_REFUNDED", 0),
            ("__psx_rt_fault_count", 0),
            ("HK_ROOM_LOAD_ERROR", 0),
        ] {
            require(&state, symbol, value)?;
        }
        match label {
            "charging" => {
                for &(symbol, value) in &[
                    ("HK_HEALTH", 4),
                    ("HK_FOCUS_COMPLETED", 0),
                    ("HK_FOCUS_HEALED", 0),
                    ("HK_FOCUS_LOCKED", 1),
                ] {
                    require(&state, symbol, value)?;
                }
                let soul = state["HK_SOUL"].as_u64().unwrap();
                let drained = state["HK_FOCUS_DRAINED"].as_u64().unwrap();
                if !(11 < soul && soul < 44 && soul + drained == 44) {
                    return Err(format!(
                        "charge did not consume earned SOUL progressively: {soul}+{drained}"
                    )
                    .into());
                }
            }
            "flash" | "healed" => {
                for &(symbol, value) in &[
                    ("HK_HEALTH", 5),
                    ("HK_SOUL", 11),
                    ("HK_FOCUS_COMPLETED", 1),
                    ("HK_FOCUS_HEALED", 1),
                    ("HK_FOCUS_DRAINED", 33),
                ] {
                    require(&state, symbol, value)?;
                }
                require(&state, "HK_FOCUS_LOCKED", u32::from(label == "flash"))?;
            }
            _ => unreachable!(),
        }
        commands.insert(
            label.into(),
            json!({"frontend": frontend, "args": args, "requested_poll": poll, "observed_polls": observed}),
        );
        states.insert(label.into(), Value::Object(state));
    }
    let report = json!({
        "disc": cue, "cue_sha256": cue_hash, "bin_sha256": bin_hash,
        "map": map_path, "map_sha256": map_hash,
        "frontend": frontend, "frontend_sha256": sha256_file(frontend)?,
        "tape": tape, "tape_sha256": sha256_file(&tape)?,
        "route": root.join(ROUTE), "route_sha256": sha256_file(&root.join(ROUTE))?,
        "normal_damage": true, "guest_state_injection": false,
        "states": states, "commands": commands,
    });
    std::fs::write(out.join("report.json"), serde_json::to_vec_pretty(&report)?)?;
    println!("earned Focus: four hits, two kills, real damage, 44 to 11 SOUL, one healed mask");
    Ok(())
}
