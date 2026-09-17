//! Single-command driver: `cargo run --release -- build` cooks (cached), builds
//! the guest, replaces the sole disc pair, writes the build report and replays
//! the validation routes. The Unity extraction and cookers stay in `host/*.py`;
//! this binary owns the order, the caches and the pass/fail verdict.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Cooker inputs, one repo-relative path per line; shared with host/regions.py.
const COOK_INPUTS: &str = include_str!("../cook_inputs.txt");
const ASSET_SCRIPTS: &[&str] = &[
    "title_music.py", "cook_audio.py", "cook_hud.py", "ambience.py", "focus_audio.py", "read_points.py",
    "geo.py", "geo_audio.py", "runner_audio.py", "lifeblood.py", "break_effects.py", "great_door.py",
];
/// Route tapes replayed against the final CUE; the last three also run the scene-gate verifier.
const ROUTES: &[(&str, bool)] = &[("focus", false), ("gate-return", true), ("town-reset", true), ("town-resident", true), ("town-well", false), ("crossroads-floor", false), ("crossroads-exit", true)];

struct Options {
    hollow_knight: Option<PathBuf>,
    sdk_source: Option<PathBuf>,
    frontend: Option<PathBuf>,
    telemetry: bool,
    recook: bool,
    validate: bool,
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut file = std::fs::File::open(path)?;
    std::io::copy(&mut file, &mut hasher)?;
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn read_json(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}

fn run(program: impl AsRef<Path>, args: &[&str], cwd: &Path) -> Result<()> {
    let program = program.as_ref();
    println!("+ {} {}", program.display(), args.join(" "));
    let status = Command::new(program).args(args).current_dir(cwd).status()?;
    if !status.success() {
        return Err(format!("{} failed with {status}", program.display()).into());
    }
    Ok(())
}

fn venv_python(root: &Path) -> PathBuf {
    root.join(".venv/bin/python")
}

fn python_step(root: &Path, script: &str, args: &[&str]) -> Result<()> {
    let script = root.join(script);
    let mut all = vec![script.to_str().unwrap()];
    all.extend_from_slice(args);
    run(venv_python(root), &all, root)
}

fn ensure_python(root: &Path) -> Result<()> {
    let venv = venv_python(root);
    if !venv.exists() {
        run("python3", &["-m", "venv", root.join(".venv").to_str().unwrap()], root)?;
    }
    let requirements = root.join("host/requirements.lock");
    let stamp = root.join(".hkpsx/python-lock.sha256");
    let current = sha256_file(&requirements)?;
    if std::fs::read_to_string(&stamp).ok().as_deref() != Some(&current) {
        run(&venv, &["-m", "pip", "install", "--requirement", requirements.to_str().unwrap()], root)?;
        std::fs::write(&stamp, current)?;
    }
    Ok(())
}

/// Hydrate the pinned PSoXide SDK snapshot once; a changed cache is an error, never silently replaced.
fn sdk(root: &Path, source: &Path) -> Result<()> {
    let pin = read_json(&root.join("sdk.lock.json"))?;
    let revision = pin["revision"].as_str().ok_or("sdk.lock.json lacks revision")?;
    let destination = root.join(".psoxide");
    let state = root.join(".hkpsx/sdk.json");
    if state.exists() {
        let record = read_json(&state)?;
        let files = record["files"].as_object().ok_or("sdk.json lacks files")?;
        let intact = record["revision"] == revision
            && files.iter().all(|(p, digest)| {
                let path = destination.join(p);
                path.is_file() && sha256_file(&path).ok().as_deref() == digest.as_str()
            });
        if intact {
            return Ok(());
        }
        return Err("Pinned SDK cache changed. Move .psoxide and .hkpsx/sdk.json aside before rehydrating.".into());
    }
    let archive = root.join(".hkpsx/sdk-archive.tar");
    run("git", &["-C", source.to_str().unwrap(), "archive", revision, "-o", archive.to_str().unwrap()], root)?;
    std::fs::create_dir_all(&destination)?;
    let listing = Command::new("tar").args(["-tf", archive.to_str().unwrap()]).output()?;
    if !listing.status.success() {
        return Err("tar listing failed".into());
    }
    run("tar", &["-xf", archive.to_str().unwrap(), "-C", destination.to_str().unwrap()], root)?;
    let mut files = serde_json::Map::new();
    for member in String::from_utf8(listing.stdout)?.lines().filter(|m| !m.ends_with('/')) {
        files.insert(member.to_string(), Value::String(sha256_file(&destination.join(member))?));
    }
    std::fs::write(&state, serde_json::to_string_pretty(&serde_json::json!({"revision": revision, "files": files}))?)?;
    std::fs::remove_file(archive)?;
    Ok(())
}

fn selected_source(root: &Path) -> Result<PathBuf> {
    let doctor = read_json(&root.join(".hkpsx/doctor.json"))?;
    Ok(PathBuf::from(doctor["installs"][0]["data_directory"].as_str().ok_or("doctor.json lacks a selected install")?))
}

fn inputs_unchanged(source: &Path, provenance: &Value) -> bool {
    provenance["inputs"].as_object().is_some_and(|inputs| {
        inputs.iter().all(|(p, meta)| {
            let path = source.join(p);
            path.is_file() && sha256_file(&path).ok().as_deref() == meta["sha256"].as_str()
        })
    })
}

fn code_fingerprint(root: &Path) -> Result<Value> {
    let mut code = serde_json::Map::new();
    for line in COOK_INPUTS.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        code.insert(line.to_string(), Value::String(sha256_file(&root.join(line))?));
    }
    Ok(Value::Object(code))
}

/// Recook only when the Windows inputs, the cooker code or the generated outputs changed.
fn cook_cached(root: &Path, force: bool) -> Result<()> {
    let provenance = root.join(".hkpsx/regions-provenance.json");
    let cache = root.join(".hkpsx/regions-cook-cache.json");
    let code = code_fingerprint(root)?;
    let source = selected_source(root)?;
    if !force && provenance.exists() && cache.exists() {
        let prov = read_json(&provenance)?;
        let old = read_json(&cache)?;
        let outputs_intact = old["outputs"].as_object().is_some_and(|outputs| {
            outputs.iter().all(|(p, h)| {
                let path = root.join(p);
                path.is_file() && sha256_file(&path).ok().as_deref() == h.as_str()
            })
        });
        if prov["source"].as_str() == source.to_str() && old["code"] == code && inputs_unchanged(&source, &prov) && outputs_intact {
            println!("Cook cache hit: source hashes and generated outputs match.");
            return Ok(());
        }
    }
    python_step(root, "host/regions.py", &[])?;
    let mut outputs: BTreeMap<String, String> = BTreeMap::new();
    for name in ["data/room.hk", "data/params.rs", "data/regions.json", "data/pogo.rs"] {
        outputs.insert(name.into(), sha256_file(&root.join(name))?);
    }
    for entry in std::fs::read_dir(root.join("data/regions"))? {
        let path = entry?.path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if name.starts_with("chunk_") && name.ends_with(".hk") {
            outputs.insert(format!("data/regions/{name}"), sha256_file(&path)?);
        }
    }
    std::fs::write(&cache, serde_json::to_string_pretty(&serde_json::json!({"code": code, "outputs": outputs}))?)?;
    Ok(())
}

fn menu_cached(root: &Path, force: bool) -> Result<()> {
    let provenance = root.join(".hkpsx/menu-provenance.json");
    let cache = root.join(".hkpsx/menu-cache.json");
    let mut code = serde_json::Map::new();
    for name in ["cook_menu.py", "source.py", "requirements.lock"] {
        code.insert(name.into(), Value::String(sha256_file(&root.join("host").join(name))?));
    }
    let code = Value::Object(code);
    let source = selected_source(root)?;
    if !force && provenance.exists() && cache.exists() {
        let prov = read_json(&provenance)?;
        let old = read_json(&cache)?;
        let output = root.join(prov["output"]["path"].as_str().unwrap_or(""));
        if old["code"] == code
            && prov["source"].as_str() == source.to_str()
            && output.is_file()
            && sha256_file(&output).ok().as_deref() == prov["output"]["sha256"].as_str()
            && inputs_unchanged(&source, &prov)
        {
            println!("Menu cache hit: source hashes and generated output match.");
            return Ok(());
        }
    }
    python_step(root, "host/cook_menu.py", &[])?;
    std::fs::write(&cache, serde_json::to_string_pretty(&serde_json::json!({"code": code}))?)?;
    Ok(())
}

/// Files whose bytes decide every asset script and the guest prepass: all host
/// and tool Python, the cooked region report and the cook cache verdict.
fn assets_key(root: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut files: Vec<PathBuf> = Vec::new();
    for dir in ["host", "tools"] {
        for entry in std::fs::read_dir(root.join(dir))? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "py" || e == "c") {
                files.push(path);
            }
        }
    }
    files.push(root.join("data/regions.json"));
    files.push(root.join(".hkpsx/regions-cook-cache.json"));
    files.push(root.join(".hkpsx/menu-provenance.json"));
    files.sort();
    for path in files {
        hasher.update(path.strip_prefix(root).unwrap_or(&path).to_string_lossy().as_bytes());
        hasher.update(sha256_file(&path)?.as_bytes());
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Every generated file the guest link reads: all of data/ plus the packed
/// scene, certificate and effect-art state under .hkpsx.
fn asset_outputs(root: &Path) -> Result<BTreeMap<String, String>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, String>) -> Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                walk(root, &path, out)?;
            } else if path.is_file() && !path.file_name().is_some_and(|n| n.to_string_lossy().ends_with(".tmp")) {
                out.insert(path.strip_prefix(root).unwrap().to_string_lossy().to_string(), sha256_file(&path)?);
            }
        }
        Ok(())
    }
    let mut out = BTreeMap::new();
    walk(root, &root.join("data"), &mut out)?;
    for state in ["packed-scenes.json", "scene-certificates.json", "ambience.json", "title-music.json", "focus-audio.json",
        "geo-provenance.json", "geo-audio-provenance.json", "lifeblood-provenance.json", "hud-provenance.json",
        "audio-provenance.json", "great-door.json", "break-effects/report.json", "opaque-tiles/report.json",
        "opaque-groups/report.json", "scenery-geometry-budgets.json", "selected-regions.json"] {
        let path = root.join(".hkpsx").join(state);
        if path.is_file() {
            out.insert(format!(".hkpsx/{state}"), sha256_file(&path)?);
        }
    }
    Ok(out)
}

/// Run the asset scripts and the guest prepass unless their inputs and every
/// generated output match the previous successful run. Returns true on a hit.
fn assets_cached(root: &Path, force: bool) -> Result<bool> {
    let cache = root.join(".hkpsx/assets-cache.json");
    let key = assets_key(root)?;
    if !force && cache.exists() {
        let old = read_json(&cache)?;
        if old["key"].as_str() == Some(key.as_str()) {
            let intact = old["outputs"].as_object().is_some_and(|outputs| {
                outputs.iter().all(|(p, h)| {
                    let path = root.join(p);
                    path.is_file() && sha256_file(&path).ok().as_deref() == h.as_str()
                })
            });
            if intact {
                println!("Asset cache hit: cooker code, region report and generated outputs match.");
                return Ok(true);
            }
        }
    }
    for script in ASSET_SCRIPTS {
        python_step(root, &format!("host/{script}"), &[])?;
    }
    run("cargo", &["run", "--quiet", "--manifest-path", "shared/hk-format/Cargo.toml", "--example", "check", "--", "data/room.hk"], root)?;
    python_step_module(root, "build_guest", "prepass")?;
    let outputs = asset_outputs(root)?;
    std::fs::write(&cache, serde_json::to_string_pretty(&serde_json::json!({"key": key, "outputs": outputs}))?)?;
    Ok(false)
}

/// Run one function of a host module in the project venv.
fn python_step_module(root: &Path, module: &str, function: &str) -> Result<()> {
    let code = format!("import sys;sys.path.insert(0,'host');import {module};{module}.{function}()");
    run(venv_python(root), &["-c", &code], root)
}

fn build(root: &Path, options: &Options) -> Result<()> {
    std::fs::create_dir_all(root.join(".hkpsx"))?;
    let doctor = root.join("tools/doctor.py");
    let mut args = vec![doctor.to_str().unwrap()];
    if let Some(path) = &options.hollow_knight {
        args.extend(["--hollow-knight", path.to_str().unwrap()]);
    }
    run("python3", &args, root)?;
    ensure_python(root)?;
    let sdk_source = options.sdk_source.clone().unwrap_or_else(|| root.parent().unwrap().join("PSoXide-sdk"));
    sdk(root, &sdk_source)?;
    cook_cached(root, options.recook)?;
    menu_cached(root, options.recook)?;
    let assets_hit = assets_cached(root, options.recook)?;
    let telemetry: &[&str] = if options.telemetry { &["--telemetry"] } else { &[] };
    let mut guest_args = telemetry.to_vec();
    if assets_hit {
        guest_args.push("--no-prepass");
    }
    python_step(root, "host/build_guest.py", &guest_args)?;
    python_step(root, "host/build_report.py", telemetry)?;
    if options.validate {
        validate(root, options)?;
    }
    Ok(())
}

/// Replay every route tape against the final CUE; a fault or an incomplete tape fails the build.
fn validate(root: &Path, options: &Options) -> Result<()> {
    let frontend = options
        .frontend
        .clone()
        .or_else(|| std::env::var_os("HK_PSX_FRONTEND").map(PathBuf::from))
        .unwrap_or_else(|| root.parent().unwrap().join("PSoXide-emulator/target/release/frontend"));
    if !frontend.is_file() {
        return Err(format!("emulator frontend not found at {} (pass --frontend)", frontend.display()).into());
    }
    let out = root.join(".hkpsx/validate");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out)?;
    // Replays are independent deterministic emulator runs: spawn them all at
    // once and collect in route order, so the wall time is the longest route.
    let children: Vec<_> = ROUTES
        .iter()
        .map(|(name, _)| {
            let tape = root.join(format!("tools/tapes/{name}.pxtape"));
            let output = out.join(name);
            Command::new(venv_python(root))
                .args([root.join("tools/replay_cue.py").to_str().unwrap(), "--tape", tape.to_str().unwrap(), "--output",
                    output.to_str().unwrap(), "--screenshot-interval", "400", "--frontend", frontend.to_str().unwrap()])
                .current_dir(root)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
        })
        .collect();
    let mut summary = serde_json::Map::new();
    let mut failed = Vec::new();
    for ((name, gates), child) in ROUTES.iter().zip(children) {
        let output = out.join(name);
        let replay = child?.wait_with_output()?;
        let text = String::from_utf8_lossy(&replay.stdout).to_string();
        std::fs::write(out.join(format!("{name}.log")), format!("{text}{}", String::from_utf8_lossy(&replay.stderr)))?;
        let verdict: Value = text.find('{').and_then(|start| serde_json::from_str(&text[start..]).ok()).unwrap_or(Value::Null);
        let mut ok = replay.status.success() && verdict["completed"] == true;
        if ok && *gates {
            let check = Command::new(venv_python(root))
                .args([root.join("tools/validate_scene_gates.py").to_str().unwrap(), output.to_str().unwrap()])
                .current_dir(root)
                .output()?;
            std::fs::write(out.join(format!("{name}-gates.log")), [check.stdout.as_slice(), check.stderr.as_slice()].concat())?;
            ok = check.status.success();
        }
        let ram = &verdict["final_ram"];
        println!(
            "{} {name}: completed={} faults={} x={} y={} region={} scene_loads={} gate_loads={}",
            if ok { "PASS" } else { "FAIL" }, verdict["completed"], verdict["faults"], ram["HK_PLAYER_X"], ram["HK_PLAYER_Y"],
            ram["HK_REGION_ID"], ram["HK_SCENE_LOADS"], ram["HK_SCENE_GATE_LOADS"]
        );
        if !ok {
            failed.push(*name);
        }
        summary.insert(name.to_string(), serde_json::json!({"pass": ok, "verdict": verdict}));
    }
    let bin = root.join("dist/hk-psx.exe");
    summary.insert("exe_sha256".into(), Value::String(if bin.is_file() { sha256_file(&bin)? } else { String::new() }));
    std::fs::write(out.join("summary.json"), serde_json::to_string_pretty(&Value::Object(summary))?)?;
    if failed.is_empty() {
        println!("All {} routes pass on the final CUE.", ROUTES.len());
        Ok(())
    } else {
        Err(format!("routes failed: {}", failed.join(", ")).into())
    }
}

fn usage() -> ! {
    eprintln!(
        "usage: cargo run --release -- <build|validate|help> [--recook] [--telemetry] [--no-validate]\n       \
         [--hollow-knight DIR] [--sdk-source DIR] [--frontend PATH]\n\
         build     cook (cached), build the guest, replace the sole hk-psx.bin/.cue, report, then validate\n\
         validate  replay the route tapes in tools/tapes against the current CUE"
    );
    std::process::exit(2)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let action = args.next().unwrap_or_else(|| "build".into());
    let mut options = Options { hollow_knight: None, sdk_source: None, frontend: None, telemetry: false, recook: false, validate: true };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--recook" => options.recook = true,
            "--telemetry" => options.telemetry = true,
            "--no-validate" => options.validate = false,
            "--hollow-knight" => options.hollow_knight = Some(PathBuf::from(args.next().unwrap_or_else(|| usage()))),
            "--sdk-source" => options.sdk_source = Some(PathBuf::from(args.next().unwrap_or_else(|| usage()))),
            "--frontend" => options.frontend = Some(PathBuf::from(args.next().unwrap_or_else(|| usage()))),
            _ => usage(),
        }
    }
    let root = root();
    let result = match action.as_str() {
        "build" => build(&root, &options),
        "validate" => validate(&root, &options),
        _ => usage(),
    };
    if let Err(error) = result {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
