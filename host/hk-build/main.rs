//! Single-command driver: `cargo hk-build build` cooks (cached), builds
//! the guest, replaces the sole disc pair, writes the build report and replays
//! the validation routes. The Unity extraction and cookers stay in `host/*.py`;
//! this binary owns the order, the caches and the pass/fail verdict.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

mod brightness;
mod budget_doc;
mod card_check;
mod cook_store;
mod disc;
mod earned_focus;
mod intro;
mod pacing;
mod region_cache;
mod repin;
mod routes;
mod scene_gate;
mod verdicts;
mod walk;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// The region report exactly as `host/regions.py` wrote it. `data/regions.json` is rewritten by the
/// asset stage (`crossroads22_wave::prepare`, which refuses an already rewritten file), so every cache
/// key and every asset-stage restart starts from this copy, never from the live report.
const REGIONS_BASE: &str = ".hkpsx/regions-base.json";

/// Cooker inputs, one repo-relative path per line; shared with host/regions.py.
const COOK_INPUTS: &str = include_str!("../cook_inputs.txt");
const ASSET_SCRIPTS: &[&str] = &[
    // The title and fight songs as XA-ADPCM, and the resident one-shots, ported to Rust
    // (hk-cook src/xa_music.rs, src/cook_audio.rs).
    "rust:xa-music",
    "rust:cook-audio",
    "cook_hud.py",
    // The two SPU banks stacked above ambience, before it: ambience reads their
    // bases and sizes from data/focus-audio.rs and data/runner-audio.rs to
    // place itself below them (and refuses an overlap), and neither bank's
    // size depends on ambience.
    "rust:focus-audio",
    "rust:runner-audio",
    "rust:ambience",
    // The text panel's ornaments (their palette is the font's ramp) and the HUD's
    // Geo icon, which read_points.py pastes into the glyph strip.
    "rust:dialogue-art",
    "read_points.py",
    "geo.py",
    "rust:geo-audio",
    "lifeblood.py",
    // Breakable and secret particle effects, ported to Rust (hk-cook).
    "rust:break-effects",
    "great_door.py",
    // Chests and pickups: their frames are bound in regions.json by the region
    // postpass, which this turns into data/pickups.rs.
    "pickups.py",
    // The arena gates, after both tables that share the guest's scripted-edge
    // scratch: it reads data/lifeblood.rs and data/great_door.rs to check the
    // union in one catalogue slot rather than its own rows alone.
    "battle_gates.py",
    // Which killed enemies the source keeps dead (data/actor_persistence.rs).
    // Reads the source scenes and the region report.
    "rust:actor-persistence",
    // The Blockers' Terrain Block edges, after the gates: its scratch budget
    // check adds the gates' rows in the same catalogue slot.
    "blocker_terrain.py",
    // Soul totems and area titles, read from the source scenes; the totems
    // before the scene sounds, which give their scenes the totem's sound.
    "soul_totems.py",
    "title_cards.py",
    // Per-scene one-shot banks: placed around ambience's stems (ambience.json)
    // and selected from the gates, benches, secrets and actors the cook admitted.
    // Ported to Rust (hk-cook src/scene_sfx.rs).
    "rust:scene-sfx",
    "shade.py",
    "rust:focus-art",
    "ability_art.py",
    // Goams, stalactites and grub jars: their placements come from the region
    // report and the cooked scene draws regions.py wrote. Ported to Rust
    // (hk-cook), so it runs in-process.
    "rust:props",
    // Area music premixes, ported to Rust (hk-cook src/area_music.rs). After
    // ambience, whose gate graph it reads from the region report.
    "rust:area-music",
    // After read_points.py: the charm catalogue wraps its descriptions against
    // the glyph advances that cook writes into data/read_points.rs.
    "charms.py",
    // Sly's shop, which the guest links now that Room_shop is admitted. It
    // needs geo.py's report for the economy, read_points.py's advances to wrap
    // its descriptions and items.py's fuse rules, so it follows all three.
    "items.py",
    "shops.py",
    // The quick map's art and tables, then Cornifer's conversation, which
    // wraps its pages against read_points.py's advances and reads his cooked
    // placement out of data/regions.json.
    "game_map.py",
    "cornifer.py",
    // The Snail Shaman's art, the spell orb and their conversation.
    "shaman.py",
    // The compiled FSM programs. Independent of the others: it reads the source
    // scenes and .hkpsx/selected-regions.json and writes data/scripts.rs.
    "cook_scripts.py",
];

/// Tapes `pgo` profiles when none is given: the King's Pass climb and the
/// Crossroads gate walk. With every delay-slot filler search on, this recipe
/// took the ordinary build from 20.80 to 22.74 fps on the first and from 19.57
/// to 20.79 on the second, counting gameplay frames only (boot and scene-gate
/// loads excluded), and left 89,440 bytes free below the stack reservation.
const PGO_TAPES: &[&str] = &["kings-climb", "crossroads-gate"];

struct Options {
    hollow_knight: Option<PathBuf>,
    sdk_source: Option<PathBuf>,
    emulator_source: Option<PathBuf>,
    frontend: Option<PathBuf>,
    /// `psoxide-gate` for closed-loop routes (HK_PSX_GATE and HK_PSX_GATE_SHA256 otherwise).
    gate: Option<PathBuf>,
    telemetry: bool,
    recook: bool,
    validate: bool,
    tapes: Vec<PathBuf>,
    /// Replays run at once by validate (default: every route).
    jobs: Option<usize>,
    /// Delete each passing route's replay dumps once it is judged, keeping its record.
    prune: bool,
    /// `validate`: move the pacing ceilings of open-loop routes that drifted (host/hk-build/repin.rs).
    repin: bool,
    /// `validate`: replay only the open-loop routes with no stored passing verdict for this disc and judge (host/hk-build/verdicts.rs).
    resume: bool,
    /// `disc`: the guest build directory to pack (default build/), the library
    /// to put the pair in (default the disc library) and Red Book tracks to
    /// add, for comparing against an older disc.
    work: Option<PathBuf>,
    library: Option<PathBuf>,
    cdda: Vec<PathBuf>,
    /// `build`/`disc`: the opening movies on a new game (a build with this flag has the player in its boot art).
    intro: bool,
    /// `build`/`disc`: a shipping build without them.
    no_intro: bool,
}

/// The repository root: this package lives in host/hk-build.
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root")
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut file = std::fs::File::open(path)?;
    std::io::copy(&mut file, &mut hasher)?;
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
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
    run_python(root, &all)
}

/// Run the venv Python, again if it dies of SIGSEGV. UnityPy's native texture
/// and audio decoders crash now and then on inputs they read fine the next
/// time (seen in ability_art.py and cook_audio.py on unchanged inputs); a
/// rerun rewrites whatever the crashed run left behind.
fn run_python(root: &Path, args: &[&str]) -> Result<()> {
    use std::os::unix::process::ExitStatusExt;
    let program = venv_python(root);
    for attempt in 1..=3 {
        println!("+ {} {}", program.display(), args.join(" "));
        let status = Command::new(&program)
            .args(args)
            .current_dir(root)
            .status()?;
        if status.success() {
            return Ok(());
        }
        if status.signal() != Some(11) || attempt == 3 {
            return Err(format!("{} failed with {status}", program.display()).into());
        }
        println!("  SIGSEGV (attempt {attempt} of 3), running it again");
    }
    unreachable!()
}

fn ensure_python(root: &Path) -> Result<()> {
    let venv = venv_python(root);
    if !venv.exists() {
        run(
            "python3",
            &["-m", "venv", root.join(".venv").to_str().unwrap()],
            root,
        )?;
    }
    let requirements = root.join("host/requirements.lock");
    let stamp = root.join(".hkpsx/python-lock.sha256");
    let current = sha256_file(&requirements)?;
    if std::fs::read_to_string(&stamp).ok().as_deref() != Some(&current) {
        run(
            &venv,
            &[
                "-m",
                "pip",
                "install",
                "--requirement",
                requirements.to_str().unwrap(),
            ],
            root,
        )?;
        std::fs::write(&stamp, current)?;
    }
    Ok(())
}

fn sdk_source(root: &Path, options: &Options) -> PathBuf {
    options
        .sdk_source
        .clone()
        .unwrap_or_else(|| root.parent().unwrap().join("PSoXide"))
}

/// Hydrate the pinned PSoXide SDK snapshot once; a changed cache is an error, never silently replaced.
fn sdk(root: &Path, source: &Path) -> Result<()> {
    let pin = read_json(&root.join("sdk.lock.json"))?;
    let revision = pin["revision"]
        .as_str()
        .ok_or("sdk.lock.json lacks revision")?;
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
        return Err(
            "Pinned SDK cache changed. Move .psoxide and .hkpsx/sdk.json aside before rehydrating."
                .into(),
        );
    }
    let archive = root.join(".hkpsx/sdk-archive.tar");
    run(
        "git",
        &[
            "-C",
            source.to_str().unwrap(),
            "archive",
            revision,
            "-o",
            archive.to_str().unwrap(),
        ],
        root,
    )?;
    std::fs::create_dir_all(&destination)?;
    let listing = Command::new("tar")
        .args(["-tf", archive.to_str().unwrap()])
        .output()?;
    if !listing.status.success() {
        return Err("tar listing failed".into());
    }
    run(
        "tar",
        &[
            "-xf",
            archive.to_str().unwrap(),
            "-C",
            destination.to_str().unwrap(),
        ],
        root,
    )?;
    let mut files = serde_json::Map::new();
    for member in String::from_utf8(listing.stdout)?
        .lines()
        .filter(|m| !m.ends_with('/'))
    {
        files.insert(
            member.to_string(),
            Value::String(sha256_file(&destination.join(member))?),
        );
    }
    std::fs::write(
        &state,
        serde_json::to_string_pretty(&serde_json::json!({"revision": revision, "files": files}))?,
    )?;
    std::fs::remove_file(archive)?;
    Ok(())
}

fn selected_source(root: &Path) -> Result<PathBuf> {
    let doctor = read_json(&root.join(".hkpsx/doctor.json"))?;
    Ok(PathBuf::from(
        doctor["installs"][0]["data_directory"]
            .as_str()
            .ok_or("doctor.json lacks a selected install")?,
    ))
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
    for line in COOK_INPUTS
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        code.insert(
            line.to_string(),
            Value::String(sha256_file(&root.join(line))?),
        );
    }
    Ok(Value::Object(code))
}

/// Bind an interrupted region cook to every file in the selected retail data
/// directory before any region is reused. The final provenance remains a
/// separate completion record and is never synthesised here.
fn source_input_fingerprints(source: &Path) -> Result<Value> {
    fn visit(root: &Path, dir: &Path, inputs: &mut BTreeMap<String, String>) -> Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let kind = entry.file_type()?;
            if kind.is_dir() {
                visit(root, &path, inputs)?;
            } else if kind.is_file() {
                let relative = path
                    .strip_prefix(root)?
                    .to_string_lossy()
                    .replace('\\', "/");
                inputs.insert(relative, sha256_file(&path)?);
            } else {
                return Err(format!("unsupported source entry: {}", path.display()).into());
            }
        }
        Ok(())
    }
    let source = source.canonicalize()?;
    let mut inputs = BTreeMap::new();
    visit(&source, &source, &mut inputs)?;
    Ok(serde_json::json!({"source": source, "inputs": inputs}))
}

fn write_source_input_fingerprints(root: &Path, source: &Path) -> Result<Value> {
    let value = source_input_fingerprints(source)?;
    let path = root.join(".hkpsx/regions-source-inputs.json");
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, serde_json::to_vec(&value)?)?;
    std::fs::rename(temp, path)?;
    std::env::set_var(
        "HK_SOURCE_FINGERPRINT_CHECKED",
        sha256_file(&root.join(".hkpsx/regions-source-inputs.json"))?,
    );
    std::env::set_var("HK_REGION_CACHE_HELPER", std::env::current_exe()?);
    Ok(value)
}

/// The generated files the region cook vouches for, with their hashes: the room bank, the
/// parameters, the region report as the cook left it (`REGIONS_BASE`, never the live
/// `data/regions.json`, which the asset stage rewrites) and every region chunk.
fn region_outputs(root: &Path) -> Result<BTreeMap<String, String>> {
    let mut outputs: BTreeMap<String, String> = BTreeMap::new();
    for name in ["data/room.hk", "data/params.rs", REGIONS_BASE] {
        outputs.insert(name.into(), sha256_file(&root.join(name))?);
    }
    for entry in std::fs::read_dir(root.join("data/regions"))? {
        let path = entry?.path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if name.starts_with("chunk_") && name.ends_with(".hk") {
            outputs.insert(format!("data/regions/{name}"), sha256_file(&path)?);
        }
    }
    Ok(outputs)
}

/// The recorded outputs (path to sha256) that are missing or whose bytes differ now, in path
/// order; `None` when `recorded` is not a table at all.
fn stale_outputs(root: &Path, recorded: &Value) -> Option<Vec<String>> {
    let table = recorded.as_object()?;
    Some(
        table
            .iter()
            .filter(|(p, h)| {
                let path = root.join(p);
                !(path.is_file() && sha256_file(&path).ok().as_deref() == h.as_str())
            })
            .map(|(p, _)| p.clone())
            .collect(),
    )
}

/// `3 generated output(s) differ from the last run, first: a, b, c`.
fn describe_stale(stale: &[String]) -> String {
    format!(
        "{} generated output(s) differ from the last run, first: {}",
        stale.len(),
        stale.iter().take(5).map(String::as_str).collect::<Vec<_>>().join(", ")
    )
}

/// Recook only when the Windows inputs, the cooker code or the generated outputs changed.
fn cook_cached(root: &Path, force: bool) -> Result<()> {
    let provenance = root.join(".hkpsx/regions-provenance.json");
    let cache = root.join(".hkpsx/regions-cook-cache.json");
    let code = code_fingerprint(root)?;
    let source = selected_source(root)?;
    let source_inputs = write_source_input_fingerprints(root, &source)?;
    let source_manifest_sha256 = sha256_file(&root.join(".hkpsx/regions-source-inputs.json"))?;
    if !force && provenance.exists() && cache.exists() {
        let prov = read_json(&provenance)?;
        let old = read_json(&cache)?;
        // A cache written before the region report was kept apart does not record it: recook once.
        let stale = if old["outputs"].get(REGIONS_BASE).is_some() {
            stale_outputs(root, &old["outputs"])
        } else {
            None
        };
        if prov["source"].as_str() == source.to_str()
            && old["code"] == code
            && old["source_manifest_sha256"].as_str() == Some(&source_manifest_sha256)
            && inputs_unchanged(&source, &prov)
            && stale.as_ref().is_some_and(Vec::is_empty)
        {
            println!("Cook cache hit: source hashes and generated outputs match.");
            return Ok(());
        }
        match stale {
            Some(stale) if !stale.is_empty() => println!("Cook cache miss: {}", describe_stale(&stale)),
            Some(_) => println!("Cook cache miss: cooker code or retail source changed."),
            None => println!("Cook cache miss: no record of the region report as the cook left it."),
        }
    }
    python_step(root, "host/regions.py", region_cook_args(&provenance))?;
    hk_cook::mossman_scene::cook(root)?;
    hk_cook::shaman_runner_scene::cook(root)?;
    // Hornet's sprite clips join Fungus1_04's banks once the scene is admitted (host/hk-cook hornet_scene.rs).
    hk_cook::hornet_scene::cook(root)?;
    if source_input_fingerprints(&source)? != source_inputs || code_fingerprint(root)? != code {
        return Err("retail source or cooker code changed during region cook".into());
    }
    // The asset stage rewrites data/regions.json (the arena waves, see
    // `assets_cached`), so the cache can only vouch for the report as the region
    // cook left it: keep that copy and record it instead of the live file.
    std::fs::copy(root.join("data/regions.json"), root.join(REGIONS_BASE))?;
    let outputs = region_outputs(root)?;
    std::fs::write(
        &cache,
        serde_json::to_string_pretty(
            &serde_json::json!({"code": code, "source_manifest_sha256": source_manifest_sha256, "outputs": outputs}),
        )?,
    )?;
    Ok(())
}

/// A fresh worktree can precook in parallel once the build driver has hashed
/// the selected retail source into an interrupted-cook input manifest.
fn region_cook_args(provenance: &Path) -> &'static [&'static str] {
    if provenance.exists()
        || provenance
            .with_file_name("regions-source-inputs.json")
            .exists()
    {
        &[]
    } else {
        &["--serial"]
    }
}

fn menu_cached(root: &Path, force: bool) -> Result<()> {
    let provenance = root.join(".hkpsx/menu-provenance.json");
    let cache = root.join(".hkpsx/menu-cache.json");
    let mut code = serde_json::Map::new();
    for name in ["cook_menu.py", "source.py", "requirements.lock"] {
        code.insert(
            name.into(),
            Value::String(sha256_file(&root.join("host").join(name))?),
        );
    }
    // The title art carries the glyph advances, and is made from the styles' composed backgrounds.
    code.insert(
        "font-metrics.json".into(),
        Value::String(sha256_file(&root.join("data/font-metrics.json"))?),
    );
    for def in hk_cook::title_style::STYLES {
        let style = def.object;
        code.insert(
            format!("title-style:{style}"),
            Value::String(sha256_file(&root.join(".hkpsx/title-styles").join(format!("{style}.png")))?),
        );
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
            && root.join("data/menu.rs").is_file()
            && sha256_file(&output).ok().as_deref() == prov["output"]["sha256"].as_str()
            && inputs_unchanged(&source, &prov)
        {
            println!("Menu cache hit: source hashes and generated output match.");
            return Ok(());
        }
    }
    python_step(root, "host/cook_menu.py", &[])?;
    std::fs::write(
        &cache,
        serde_json::to_string_pretty(&serde_json::json!({"code": code}))?,
    )?;
    Ok(())
}

/// Files whose bytes decide every asset script and the guest prepass: all host
/// and tool Python, the cooked region report and the cook cache verdict.
/// Every file under `dir`, recursively.
fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_files(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

fn assets_key(root: &Path) -> Result<String> {
    let mut files = asset_code_files(root)?;
    files.push(root.join(REGIONS_BASE));
    files.push(root.join(".hkpsx/regions-cook-cache.json"));
    files.push(root.join(".hkpsx/menu-provenance.json"));
    hash_files(root, files)
}

/// Every cooker source the asset stage runs: the Python and C scripts of host/ and tools/, the Rust
/// cookers and the readers they share (host/hk-cook, hk-unity, hk-pil, hk-dotnet).
fn asset_code_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> = Vec::new();
    for dir in ["host", "tools"] {
        for entry in std::fs::read_dir(root.join(dir))? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "py" || e == "c") {
                files.push(path);
            }
        }
    }
    for dir in [
        "host/hk-cook/src",
        "host/hk-unity/src",
        "host/hk-unity/data",
        "host/hk-pil/src",
        "host/hk-dotnet/src",
    ] {
        collect_files(&root.join(dir), &mut files)?;
    }
    Ok(files)
}

/// One digest over the repo-relative names and contents of `files`, in path order.
fn hash_files(root: &Path, mut files: Vec<PathBuf>) -> Result<String> {
    let mut hasher = Sha256::new();
    files.sort();
    for path in files {
        hasher.update(
            path.strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .as_bytes(),
        );
        hasher.update(sha256_file(&path)?.as_bytes());
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// The name of the cook state these inputs produce in the shared cook store: the region cooker's
/// code, the retail source manifest (`.hkpsx/regions-source-inputs.json`, written by
/// `write_source_input_fingerprints`) and every asset cooker's code, but not the cook's own
/// outputs, so a lane can compute it before it has cooked anything. It only picks which snapshot
/// to try; the three caches still verify every output they vouch for.
fn cook_store_key(root: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(b"hk-cook-store-v1\n");
    hasher.update(serde_json::to_vec(&code_fingerprint(root)?)?);
    hasher.update(sha256_file(&root.join(".hkpsx/regions-source-inputs.json"))?.as_bytes());
    hasher.update(hash_files(root, asset_code_files(root)?)?.as_bytes());
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// A short name for the snapshot a publish records: the checkout's branch and commit.
fn cook_store_label(root: &Path) -> String {
    let git = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default()
    };
    format!("{} {}", git(&["rev-parse", "--abbrev-ref", "HEAD"]), git(&["rev-parse", "--short", "HEAD"]))
}

/// Before the region cook: if the shared store holds the snapshot for these cooker inputs and
/// this lane is not already at it, make the lane's cook state that snapshot, so the three
/// caches hit. Never fails the build: any problem is a warning and a normal cook.
fn cook_store_restore(root: &Path) {
    let Some(store) = cook_store::Store::from_env() else {
        return;
    };
    let outcome = (|| -> Result<()> {
        let source = selected_source(root)?;
        write_source_input_fingerprints(root, &source)?;
        let key = cook_store_key(root)?;
        let short = &key[..16];
        if cook_store::lane_in_sync(root, &key) {
            println!("Cook store: this lane is already at snapshot {short}.");
        } else if store.has_key(&key) {
            let r = cook_store::restore(root, &store, &key)?;
            println!(
                "Cook store: restored snapshot {short}: {} files, {:.2} GB, {} replaced, {} stale removed.",
                r.files,
                r.bytes as f64 / 1e9,
                r.replaced,
                r.pruned
            );
        } else {
            println!("Cook store: no snapshot for {short}; this build cooks and publishes one.");
        }
        Ok(())
    })();
    if let Err(error) = outcome {
        println!("Cook store: restore skipped ({error}); cooking normally.");
    }
}

/// After the asset stage: publish this lane's cook state under its key if the store lacks it,
/// and give bytes the store already holds back to its blocks. Never fails the build.
fn cook_store_publish(root: &Path, asset_hit: bool) {
    let Some(store) = cook_store::Store::from_env() else {
        return;
    };
    let outcome = (|| -> Result<()> {
        let key = cook_store_key(root)?;
        let short = &key[..16];
        if store.has_key(&key) {
            let r = cook_store::dedupe(root, &store)?;
            if asset_hit {
                cook_store::mark_synchronised(root, &key)?;
            }
            println!(
                "Cook store: snapshot {short} exists; {} files ({:.2} GB) given back to the store's blocks.",
                r.relinked,
                r.relinked_bytes as f64 / 1e9
            );
        } else {
            let r = cook_store::publish(root, &store, &key, &cook_store_label(root))?;
            println!(
                "Cook store: published snapshot {short}: {} files, {} new objects ({:.2} GB), {} files relinked ({:.2} GB).",
                r.files,
                r.new_objects,
                r.new_bytes as f64 / 1e9,
                r.relinked,
                r.relinked_bytes as f64 / 1e9
            );
        }
        Ok(())
    })();
    if let Err(error) = outcome {
        println!("Cook store: publish skipped ({error}).");
    }
}

/// Every generated file the guest link reads: all of data/ plus the packed
/// scene, certificate and effect-art state under .hkpsx.
fn asset_outputs(root: &Path) -> Result<BTreeMap<String, String>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, String>) -> Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                walk(root, &path, out)?;
            } else if path.is_file()
                && !path.file_name().is_some_and(|n| {
                    let n = n.to_string_lossy();
                    // The guest build writes the boot art chunk (and, for `--intro`, the
                    // player it carries) after the cookers have run and the cache is
                    // stamped, so they are not cooker outputs: counting them made every
                    // rebuild after a first one recook everything.
                    n.ends_with(".tmp") || matches!(&*n, "boot-art.hk" | "boot_art.rs" | "intro.exe")
                })
            {
                out.insert(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .to_string(),
                    sha256_file(&path)?,
                );
            }
        }
        Ok(())
    }
    let mut out = BTreeMap::new();
    walk(root, &root.join("data"), &mut out)?;
    for state in [
        "packed-scenes.json",
        "scene-certificates.json",
        "ambience.json",
        "xa-music.json",
        "area-music.json",
        "focus-audio.json",
        "geo-provenance.json",
        "geo-audio-provenance.json",
        "lifeblood-provenance.json",
        "hud-provenance.json",
        "audio-provenance.json",
        "great-door.json",
        "battle-gates.json",
        "scene-sfx.json",
        "break-effects/report.json",
        "opaque-tiles/report.json",
        "opaque-groups/report.json",
        "scenery-geometry-budgets.json",
        "selected-regions.json",
    ] {
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
            let stale = stale_outputs(root, &old["outputs"]);
            if stale.as_ref().is_some_and(Vec::is_empty) {
                println!(
                    "Asset cache hit: cooker code, region report and generated outputs match."
                );
                return Ok(true);
            }
            match stale {
                Some(stale) => println!("Asset cache miss: {}", describe_stale(&stale)),
                None => println!("Asset cache miss: the cache records no generated outputs."),
            }
        } else {
            println!("Asset cache miss: cooker code, region report or cache verdict changed.");
        }
    }
    // The asset cookers (charms, items, shops, NPCs, scripts) read the admitted scene list from
    // .hkpsx/selected-regions.json, which the prepass writes only after them: a fresh clone had
    // none, and a rerun would feed them the last run's list. Both it and the live report start
    // from the report as the region cook wrote it, as in a cold build (`prepare` refuses a
    // report the arena waves already rewrote). A cache hit must leave both as that run ended
    // them, because the steps after the asset stage saw them that way in the cold build.
    for live in ["data/regions.json", ".hkpsx/selected-regions.json"] {
        std::fs::copy(root.join(REGIONS_BASE), root.join(live))?;
    }
    for script in ASSET_SCRIPTS {
        match script.strip_prefix("rust:") {
            Some(tool) => rust_step(root, tool)?,
            None => python_step(root, &format!("host/{script}"), &[])?,
        }
    }
    run(
        "cargo",
        &[
            "run",
            "--quiet",
            "--manifest-path",
            "shared/hk-format/Cargo.toml",
            "--example",
            "check",
            "--",
            "data/room.hk",
        ],
        root,
    )?;
    hk_cook::crossroads22_wave::prepare(root)?;
    python_step_module(root, "build_guest", "prepass")?;
    hk_cook::crossroads22_wave::finish(root)?;
    hk_cook::mossman_scene::bind_generated_specs(root)?;
    hk_cook::crawler_recoil::bind_generated_specs(root)?;
    hk_cook::shaman_runner_scene::bind_generated_specs(root)?;
    // Scene admission and the prepass can change the report these mutable
    // bindings certify, even when their generated Rust bytes are unchanged.
    python_step(root, "host/geo.py", &[])?;
    python_step(root, "host/lifeblood.py", &[])?;
    // Certificates bind to the final deduplicated bank geometry and palette
    // words, so they follow the pack. Tiles before groups (groups read the
    // tile report), both before the per-scene bundles.
    for tool in ["opaque-tiles", "opaque-groups", "scene-certificates"] {
        rust_step(root, tool)?;
    }
    let outputs = asset_outputs(root)?;
    std::fs::write(
        &cache,
        serde_json::to_string_pretty(&serde_json::json!({"key": key, "outputs": outputs}))?,
    )?;
    Ok(false)
}

/// Whether a build carries the opening movies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IntroPlan {
    /// A test disc (any guest feature, such as `route-input`, whose replays press New Game by poll
    /// count and must not meet a movie first) or `--no-intro`.
    Without,
    /// `--intro`: the build fails when the movies cannot be cooked.
    Required,
    /// The shipping disc: the movies when this machine can cook them, a warning when it cannot.
    IfPossible,
}

/// True when the guest is built without features: the shipping disc.
fn shipping_guest() -> bool {
    std::env::var_os("HK_GUEST_FEATURES").is_none_or(|f| f.is_empty())
}

fn intro_plan(options: &Options, shipping: bool) -> IntroPlan {
    if options.no_intro {
        IntroPlan::Without
    } else if options.intro {
        IntroPlan::Required
    } else if shipping {
        IntroPlan::IfPossible
    } else {
        IntroPlan::Without
    }
}

/// The opening movies for a disc that carries them: cooked here when they are
/// not yet, and the player compiled into `data/intro.exe` for the guest build.
fn intro_stage(root: &Path, plan: IntroPlan) -> Result<Option<disc::Intro>> {
    if plan == IntroPlan::Without {
        return Ok(None);
    }
    let movies = match intro::cooked(root) {
        Some(movies) => movies,
        None => {
            intro::cook(root, &selected_source(root)?)?;
            match (intro::cooked(root), plan) {
                (Some(movies), _) => movies,
                (None, IntroPlan::Required) => return Err("no intro movies were cooked".into()),
                (None, _) => return Ok(None),
            }
        }
    };
    intro::compile(root, &root.join(".psoxide"))?;
    // The guest build reads these: the player goes into the boot art and the
    // `intro` feature on.
    std::env::set_var("HK_INTRO", "1");
    Ok(Some(disc::Intro {
        movies: intro::MOVIE_FILES.iter().map(|n| movies.join(n)).collect(),
    }))
}

/// Where the finished disc goes: the user's disc library, which a build replaces
/// only once the new pair is complete.
fn disc_library() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
        .join("Downloads")
        .join("ps1 games")
}

fn selected_library(options: &Options) -> PathBuf {
    options.library.clone().unwrap_or_else(disc_library)
}

/// Pack the disc from the guest build in `work` (its patched EXE, `modules.json`
/// and the cooked chunks): WORLD.PAK in read order and the XA song file.
fn package_disc(root: &Path, work: &Path, library: &Path, intro: Option<&disc::Intro>) -> Result<()> {
    let songs = disc::Songs {
        xa: vec![root.join("data/music.xa")],
        cdda: Vec::new(),
    };
    disc::package(
        root,
        &work.join("hk-psx-patched.exe"),
        work,
        library,
        &songs,
        intro,
    )
}

/// Run one of the cookers already ported to Rust (host/hk-cook), in-process.
fn rust_step(root: &Path, tool: &str) -> Result<()> {
    println!("+ hk-cook {tool}");
    let start = std::time::Instant::now();
    match tool {
        "actor-persistence" => hk_cook::actor_persistence::main(root, None)?,
        "props" => hk_cook::props::main(root, None)?,
        "break-effects" => hk_cook::break_effects::main(root, None)?,
        "scene-sfx" => hk_cook::scene_sfx::main(root, None)?,
        "xa-music" => hk_cook::xa_music::main(root, None)?,
        "cook-audio" => hk_cook::cook_audio::main(root, None)?,
        "geo-audio" => hk_cook::geo_audio::main(root, None)?,
        "focus-audio" => hk_cook::focus_audio::main(root, None)?,
        "focus-art" => hk_cook::focus_art::main(root, None)?,
        "dialogue-art" => hk_cook::dialogue_art::main(root, None)?,
        "title-style" => hk_cook::title_style::main(root, None, &[])?,
        "ambience" => hk_cook::ambience::main(root, None)?,
        "runner-audio" => {
            hk_cook::runner_audio::main(root, &[], None)?;
            hk_cook::mossman_audio::cook(root)?;
        }
        "area-music" => hk_cook::area_music::main(root, None)?,
        "opaque-tiles" => hk_cook::opaque_tiles::cook(root, 2, false)?,
        "opaque-groups" => hk_cook::opaque_groups::main(root)?,
        "scene-certificates" => hk_cook::scene_certificates::main(root, &[])?,
        other => return Err(format!("no Rust cooker named {other}").into()),
    }
    println!("  hk-cook {tool}: {:.1?}", start.elapsed());
    Ok(())
}

/// Run one function of a host module in the project venv.
fn python_step_module(root: &Path, module: &str, function: &str) -> Result<()> {
    let code =
        format!("import sys;sys.path.insert(0,'host');import {module};{module}.{function}()");
    run_python(root, &["-c", &code])
}

fn prepare_cook(root: &Path, options: &Options) -> Result<()> {
    std::fs::create_dir_all(root.join(".hkpsx"))?;
    let doctor = root.join("tools/doctor.py");
    let mut args = vec![doctor.to_str().unwrap()];
    if let Some(path) = &options.hollow_knight {
        args.extend(["--hollow-knight", path.to_str().unwrap()]);
    }
    run("python3", &args, root)?;
    ensure_python(root)?;
    // Hydrate the pinned SDK before either suite: both compile guest modules
    // against its crates, so after a pin bump they would fail on a missing
    // .psoxide rather than on anything in this checkout.
    sdk(root, &sdk_source(root, options))?;
    // The XA encoder is exported from the same checkout (hk-cook xa_music).
    std::env::set_var("HK_SDK_SOURCE", sdk_source(root, options));
    // Hornet's boss trees come first: the cook below appends her sprite clips to Fungus1_04's banks and reads which clips
    // each of her objects plays from the manifests this writes (data/hornet3_art.json).
    hk_fsm_cook::cook3::cook_hornet3(root).map_err(|e| e.to_string())?;
    cook_store_restore(root);
    cook_cached(root, options.recook)?;
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BuildMode {
    CookOnly,
    Normal,
    Pgo,
}

impl BuildMode {
    fn runs_guest(self) -> bool {
        self != Self::CookOnly
    }
}

/// `pgo` is `build` with a profile-guided guest in place of the ordinary one.
fn build(root: &Path, options: &Options, mode: BuildMode) -> Result<()> {
    prepare_cook(root, options)?;
    if !mode.runs_guest() {
        return Ok(());
    }
    // `--intro`: the player compiled into data/intro.exe and HK_INTRO set before
    // the guest build reads them; the movies go onto the disc packed below.
    // The glyph strip and its advances (data/font-metrics.json) and the poem's cells (data/poem-font.hk, which
    // the intro stage reads) come out of this stage; the title art cooked next carries the advances. A fifth
    // of a second, so it runs every build, and before the intro stage on a fresh data directory.
    rust_step(root, "dialogue-art")?;
    let intro = intro_stage(root, intro_plan(options, shipping_guest()))?;
    let pgo = mode == BuildMode::Pgo;
    // The title styles' backgrounds and logos, which the title art cooked next is made from.
    rust_step(root, "title-style")?;
    menu_cached(root, options.recook)?;
    let asset_hit = assets_cached(root, options.recook)?;
    cook_store_publish(root, asset_hit);
    // Every HKFSM03 bank the guest links: the enemies' original graphs and the scene scripts.
    hk_fsm_cook::cook3::cook_all(root).map_err(|e| e.to_string())?;
    // The boot art chunk and its offsets (data/boot_art.rs) are generated data
    // the guest-compile tests include. build_guest.py writes them too, but only
    // after the suites below, so a checkout that had never built since the
    // boot art landed failed its host tests. Packing four cooked files is
    // cheap, and the asset cache does not track these outputs, so write them on
    // every build rather than behind that cache.
    python_step_module(root, "build_guest", "boot_art")?;
    // Likewise the room-module tables (data/modules.rs, data/props_art.rs),
    // which build_guest.py otherwise writes only when it links the guest.
    python_step_module(root, "build_guest", "module_tables")?;
    // The host suite is cheap and nothing else ran it, so a cooker regression
    // used to survive whole builds: adding a required constant to
    // `generated_params` broke a fixture and nine builds shipped before anyone
    // noticed. It runs after the cook because its fixtures read what the cook
    // generates (data/*.rs, the audio banks, data/regions.json), which a fresh
    // clone does not have; on a warm checkout the cook is a cache hit, so this
    // still runs before anything slow. HK_PRE_BUILD skips what can only be
    // checked against this build's own report (docs/BUDGET.md), which runs
    // after build_report below.
    let status = Command::new(venv_python(root))
        .args(["-m", "unittest", "discover", "-s", "tests", "-q"])
        .env("HK_PRE_BUILD", "1")
        .current_dir(root)
        .status()?;
    if !status.success() {
        return Err(format!("the host test suite failed with {status}").into());
    }
    // And the simulation's own suite, for the same reason: the charm board
    // landed with four hk-sim test targets failing to compile, and nothing here
    // ran them, so it shipped. These path-include the guest modules (and so
    // their generated data), so they are the only check that the guest's own
    // logic still holds before its link.
    run(
        "cargo",
        &["test", "-q", "--manifest-path", "shared/hk-sim/Cargo.toml"],
        root,
    )?;
    let telemetry: &[&str] = if options.telemetry {
        &["--telemetry"]
    } else {
        &[]
    };
    // assets_cached has run the prepass or proven its outputs current either
    // way, so the guest build never repeats it (it ran pack_scenes a second
    // time on every build that recooked assets).
    let mut guest_args = telemetry.to_vec();
    guest_args.push("--no-prepass");
    if pgo {
        profile_guided_guest(root, options, &guest_args)?;
    } else {
        python_step(root, "host/build_guest.py", &guest_args)?;
        package_disc(root, &root.join("build"), &selected_library(options), intro.as_ref())?;
    }
    let mut report_args = telemetry.to_vec();
    if pgo {
        // Its own report file: docs/BUDGET.md pins the ordinary build's figures.
        report_args.push("--pgo");
    }
    python_step(root, "host/build_report.py", &report_args)?;
    // The doc pins the shipping build. A disc built with guest features (the route-input test
    // disc, card-diag) has other figures and must not rewrite it.
    let shipping = shipping_guest();
    if !pgo && shipping && budget_doc::sync(root)? {
        println!("docs/BUDGET.md: pinned figures rewritten from .hkpsx/build-normal.json (commit the doc with this change)");
    }
    check_hot_order(root, !pgo && !options.telemetry)?;
    if options.validate {
        validate(root, options)?;
    }
    if !pgo {
        // docs/BUDGET.md pins the ordinary build's figures, so it is checked
        // against the report this build wrote, after the replays, so a stale
        // table never hides their verdict.
        run(
            venv_python(root),
            &["-m", "unittest", "tests.test_budget_doc"],
            root,
        )?;
    }
    Ok(())
}

/// Build the canonical guest optimised with the emulator's own instruction counts.
///
/// PSoXide counts every guest instruction exactly, so no instrumented build is
/// needed: a build with profiling line tables replays each tape, the pinned
/// SDK's psoxide-pgo turns the PC histogram into an LLVM sample profile through
/// the matching ELF's DWARF, and the canonical build is compiled with it.
/// Profile names carry crate hashes that depend on the checkout path, so the
/// profile is rebuilt here every time and never committed.
fn profile_guided_guest(root: &Path, options: &Options, guest_args: &[&str]) -> Result<()> {
    if root.to_string_lossy().chars().any(char::is_whitespace) {
        return Err(format!(
            "pgo cannot run from {}: the guest RUSTFLAGS are split on whitespace, so the profile \
             and linker paths inside this checkout cannot be passed. Use a checkout path without spaces.",
            root.display()
        )
        .into());
    }
    if options.frontend.is_none() && std::env::var_os("HK_PSX_FRONTEND").is_none() {
        return Err("profiling requires an explicit retained --frontend or HK_PSX_FRONTEND".into());
    }
    let (frontend, emulator_revision) = pinned_frontend(root, options)?;
    let tapes: Vec<PathBuf> = if options.tapes.is_empty() {
        PGO_TAPES
            .iter()
            .map(|name| root.join(format!("tools/tapes/{name}.pxtape")))
            .collect()
    } else {
        options.tapes.clone()
    };
    let work = root.join(".hkpsx/pgo");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work)?;
    let (collect, elf) = (work.join("collect"), work.join("elf"));
    let telemetry: &[&str] = if options.telemetry {
        &["--telemetry"]
    } else {
        &[]
    };
    let mut args = guest_args.to_vec();
    args.extend(["--profile", "collect", "--work", collect.to_str().unwrap()]);
    python_step(root, "host/build_guest.py", &args)?;
    package_disc(root, &collect, &collect.join("disc"), None)?;
    let mut args = telemetry.to_vec();
    args.extend([
        "--no-prepass",
        "--profile",
        "elf",
        "--work",
        elf.to_str().unwrap(),
    ]);
    python_step(root, "host/build_guest.py", &args)?;

    let mut samples: BTreeMap<String, u64> = BTreeMap::new();
    for (index, tape) in tapes.iter().enumerate() {
        let data = std::fs::read(tape)?;
        if data.len() < 16 || &data[..8] != b"PXITAPE2" {
            return Err(format!("{} is not a poll-bound tape", tape.display()).into());
        }
        // Stop where the tape ends, as tools/replay_cue.py does.
        let count = u32::from_le_bytes(data[8..12].try_into()?);
        let start = u32::from_le_bytes(data[12..16].try_into()?);
        let log = work.join(format!("pc-{index}.csv"));
        let mut command = Command::new(&frontend);
        command
            .args(["launch", "--path"])
            .arg(disc::disc_cue(&collect.join("disc")))
            .args(["--embedded-playtest", "--config-dir"])
            .arg(work.join(format!("emulator-{index}")))
            .args(["--steps", "6000000000", "--input-tape"])
            .arg(tape)
            .args([
                "--stop-at-poll",
                &(start + count).to_string(),
                "--pc-sample-log",
            ])
            .arg(&log)
            // Prime, so the sampler cannot fall into step with a loop.
            .args(["--pc-sample-instructions", "61"]);
        let stem = tape
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        if let Some(card) = routes::load(root)?
            .into_iter()
            .find(|route| route.name == stem)
            .and_then(|route| route.card)
        {
            // The replay writes the card back: use a scratch copy so the fixture stays put.
            let fixture = root.join("tools/cards").join(&card);
            let scratch = work.join(format!("{stem}.mcd"));
            match std::fs::copy(&fixture, &scratch) {
                Ok(_) => {
                    command.arg("--memcard").arg(scratch);
                }
                Err(error) => println!(
                    "{stem}: warning: card fixture tools/cards/{card} is unreadable ({error}); profiling on a freshly formatted card"
                ),
            }
        }
        println!("+ {command:?}");
        if !command.status()?.success() {
            return Err(format!("profiling replay of {} failed", tape.display()).into());
        }
        for line in std::fs::read_to_string(&log)?.lines().skip(1) {
            let mut fields = line.split(',');
            if let (Some(pc), Some(Ok(count))) =
                (fields.next(), fields.next().map(str::parse::<u64>))
            {
                *samples.entry(pc.to_string()).or_default() += count;
            }
        }
        std::fs::remove_file(&log)?;
    }
    if samples.is_empty() {
        return Err("the profiling replays recorded no samples".into());
    }
    let merged = work.join("pc.csv");
    let mut text = String::from("pc,samples\n");
    for (pc, count) in &samples {
        text.push_str(&format!("{pc},{count}\n"));
    }
    std::fs::write(&merged, text)?;
    let profile = work.join("hk-psx.prof");
    let tool = root.join(".psoxide/tools/psoxide-pgo/Cargo.toml");
    run(
        "cargo",
        &[
            "run",
            "--locked",
            "--release",
            "--manifest-path",
            tool.to_str().unwrap(),
            "--",
            elf.join("hk-psx.elf").to_str().unwrap(),
            merged.to_str().unwrap(),
            profile.to_str().unwrap(),
        ],
        root,
    )?;
    std::fs::remove_file(&merged)?;
    // What the profile came from, so two runs can be compared by hash.
    let provenance = serde_json::json!({
        "emulator_revision": emulator_revision,
        "frontend_sha256": sha256_file(&frontend)?,
        "tapes": tapes.iter().map(|t| Ok(serde_json::json!({"path": t, "sha256": sha256_file(t)?})))
            .collect::<Result<Vec<_>>>()?,
        "profile_sha256": sha256_file(&profile)?,
    });
    std::fs::write(
        work.join("provenance.json"),
        serde_json::to_string_pretty(&provenance)?,
    )?;
    let mut args = telemetry.to_vec();
    args.extend(["--no-prepass", "--profile", profile.to_str().unwrap()]);
    python_step(root, "host/build_guest.py", &args)?;
    package_disc(root, &root.join("build"), &selected_library(options), None)
}

/// The emulator `pgo` profiles with: the revision in emulator.lock.json,
/// exported from the sibling PSoXide-emulator checkout (or --emulator-source)
/// and built once under .hkpsx/emulator/<revision>.
///
/// The PC histogram is the emulator's instruction stream, so it moves with the
/// emulator's timing: vblank waits, CD and DMA latencies all change how often a
/// loop runs. Two runs profiled by two different frontends produced two
/// different PGO images from the same commit. An explicit --frontend or
/// HK_PSX_FRONTEND selects the retained emulator for profiling as well as
/// validation. Profiling binds it to HK_PSX_FRONTEND_SHA256 and records its
/// external identity instead of claiming the lock file's source revision.
fn pinned_frontend(root: &Path, options: &Options) -> Result<(PathBuf, String)> {
    if let Some(path) = options.frontend.clone().or_else(|| {
        std::env::var_os("HK_PSX_FRONTEND").map(PathBuf::from)
    }) {
        let expected = std::env::var("HK_PSX_FRONTEND_SHA256")
            .map_err(|_| "an external profiling frontend requires HK_PSX_FRONTEND_SHA256")?;
        return external_profile_frontend(path, &expected);
    }
    let pin = read_json(&root.join("emulator.lock.json"))?;
    let revision = pin["revision"]
        .as_str()
        .ok_or("emulator.lock.json lacks revision")?
        .to_string();
    let tree = root.join(".hkpsx/emulator").join(&revision);
    let target = tree.join("target");
    let binary = target.join("release/frontend");
    let stamp = tree.join(".hk-frontend.json");
    if let (Ok(record), true) = (read_json(&stamp), binary.is_file()) {
        if record["revision"] == revision.as_str()
            && record["sha256"].as_str() == Some(&sha256_file(&binary)?)
        {
            return Ok((binary, revision));
        }
    }
    let source = options
        .emulator_source
        .clone()
        .unwrap_or_else(|| root.parent().unwrap().join("PSoXide-emulator"));
    std::fs::create_dir_all(&tree)?;
    let archive = root.join(".hkpsx/emulator-archive.tar");
    run(
        "git",
        &[
            "-C",
            source.to_str().unwrap(),
            "archive",
            &revision,
            "-o",
            archive.to_str().unwrap(),
        ],
        root,
    )?;
    run(
        "tar",
        &[
            "-xf",
            archive.to_str().unwrap(),
            "-C",
            tree.to_str().unwrap(),
        ],
        root,
    )?;
    std::fs::remove_file(&archive)?;
    // Its SDK component comes from the same checkout this build's SDK does.
    let sdk = format!("sdk={}", sdk_source(root, options).display());
    hydrate_components(&tree, &sdk)?;
    println!(
        "+ cargo build --locked --release -p frontend (CARGO_TARGET_DIR={})",
        target.display()
    );
    let status = Command::new("cargo")
        .args(["build", "--locked", "--release", "-p", "frontend"])
        .env("CARGO_TARGET_DIR", &target)
        .current_dir(&tree)
        .status()?;
    if !status.success() {
        return Err(
            format!("building the pinned frontend at {revision} failed with {status}").into(),
        );
    }
    std::fs::write(
        &stamp,
        serde_json::to_string_pretty(&serde_json::json!({
        "revision": revision, "sha256": sha256_file(&binary)?}))?,
    )?;
    Ok((binary, revision))
}

/// Where the SDK's `psoxide-link` package is installed from, as in the
/// emulator's Makefile.
const SDK_GIT: &str = "https://github.com/EBonura/PSoXide";

/// Verify an explicitly selected profiling frontend without compiling a fallback.
fn external_profile_frontend(path: PathBuf, expected: &str) -> Result<(PathBuf, String)> {
    if expected.len() != 64 || !expected.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("HK_PSX_FRONTEND_SHA256 must contain a SHA256 digest".into());
    }
    if !path.is_file() {
        return Err(format!("profiling frontend not found at {}", path.display()).into());
    }
    let actual = sha256_file(&path)?;
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(format!("profiling frontend SHA256 mismatch at {}", path.display()).into());
    }
    Ok((path, format!("external:sha256:{actual}")))
}

/// The program and arguments that hydrate an exported emulator tree from the
/// local SDK (`sdk` is a `sdk=<checkout>` source). A pin that still ships
/// `tools/bootstrap-components.py` is hydrated by it, as before; a newer pin
/// dropped the script for the SDK's `psoxide-components`, which is built once
/// per locked SDK revision into the tree's `.tools/` and given the same flags.
fn components_command(tree: &Path, sdk: &str) -> Result<(PathBuf, Vec<String>)> {
    let root = tree
        .to_str()
        .ok_or("the emulator tree path is not UTF-8")?
        .to_string();
    if tree.join("tools/bootstrap-components.py").is_file() {
        let args = [
            "tools/bootstrap-components.py",
            "--root",
            &root,
            "--source",
            sdk,
        ];
        return Ok((
            "python3".into(),
            args.iter().map(|a| a.to_string()).collect(),
        ));
    }
    let lock = read_json(&tree.join("components.lock.json"))?;
    let revision = lock["components"]["sdk"]["revision"]
        .as_str()
        .ok_or("the emulator's components.lock.json lacks the sdk revision")?;
    let binary = tree
        .join(".tools")
        .join(format!("sdk-{revision}"))
        .join("bin/psoxide-components");
    Ok((
        binary,
        ["--root", &root, "--source", sdk]
            .iter()
            .map(|a| a.to_string())
            .collect(),
    ))
}

/// Hydrate an exported emulator tree's locked components from the local SDK,
/// installing `psoxide-components` first when the pin needs it and it is not
/// there yet.
fn hydrate_components(tree: &Path, sdk: &str) -> Result<()> {
    let (program, args) = components_command(tree, sdk)?;
    if !program.is_absolute() {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        return run(program, &args, tree);
    }
    if !program.is_file() {
        let install = program
            .parent()
            .and_then(Path::parent)
            .ok_or("bad psoxide-components path")?;
        let revision = install
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_prefix("sdk-"))
            .ok_or("bad psoxide-components path")?;
        run(
            "cargo",
            &[
                "install",
                "--locked",
                "--git",
                SDK_GIT,
                "--rev",
                revision,
                "--root",
                install
                    .to_str()
                    .ok_or("the emulator tree path is not UTF-8")?,
                "psoxide-link",
            ],
            tree,
        )?;
    }
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    run(program, &args, tree)
}

/// The emulator the route replays run on: --frontend or HK_PSX_FRONTEND when
/// given, else the one pinned in emulator.lock.json, the same `pgo` uses.
/// Validation used to take whatever ../PSoXide-emulator last built, so a
/// verdict depended on the sibling checkout's state rather than this one's.
fn frontend(root: &Path, options: &Options) -> Result<PathBuf> {
    let given = options
        .frontend
        .clone()
        .or_else(|| std::env::var_os("HK_PSX_FRONTEND").map(PathBuf::from));
    let frontend = match given {
        Some(path) => path,
        None => pinned_frontend(root, options)?.0,
    };
    if !frontend.is_file() {
        return Err(format!(
            "emulator frontend not found at {} (pass --frontend)",
            frontend.display()
        )
        .into());
    }
    Ok(frontend)
}

/// The scratch copy of a route's card fixture for one replay (the replay writes the card back, so the
/// fixture stays put). A pin file that names a card the repository does not hold is said so and the
/// replay starts on a freshly formatted card, which is what a route with no `card` gets.
fn scratch_card(root: &Path, route: &routes::Route, scratch_dir: &Path) -> Option<PathBuf> {
    let card = route.card.as_ref()?;
    let fixture = root.join("tools/cards").join(card);
    let scratch = scratch_dir.join(format!("{}.mcd", route.name));
    match std::fs::copy(&fixture, &scratch) {
        Ok(_) => Some(scratch),
        Err(error) => {
            println!(
                "{}: warning: card fixture tools/cards/{card} is unreadable ({error}); the replay starts on a freshly formatted card",
                route.name
            );
            None
        }
    }
}

/// `determinism`: replay each selected open-loop route twice on the same disc and fail if the two
/// runs differ in any final telemetry word or in the per-poll route log. A route that is not
/// deterministic cannot be pinned; this finds it without pinning a single number
/// (HK_VALIDATE_ONLY=a,b narrows; the journey segments chain cards and are skipped).
fn determinism(root: &Path, options: &Options) -> Result<()> {
    let frontend = frontend(root, options)?;
    let library = selected_library(options);
    let only: Option<Vec<String>> =
        std::env::var("HK_VALIDATE_ONLY").ok().map(|list| list.split(',').map(str::to_owned).collect());
    let base = root.join(".hkpsx/determinism");
    let _ = std::fs::remove_dir_all(&base);
    let (first, second) = (base.join("a"), base.join("b"));
    std::fs::create_dir_all(&first)?;
    std::fs::create_dir_all(&second)?;
    let mut bad = Vec::new();
    let mut checked = 0usize;
    for route in routes::load(root)? {
        if route.journey.is_some() || only.as_ref().is_some_and(|o| !o.iter().any(|x| *x == route.name)) {
            continue;
        }
        let spawn = |out: &Path| -> std::io::Result<std::process::Child> {
            let card = scratch_card(root, &route, out);
            replay_command(root, &frontend, out, &library, &route.name, card).spawn()
        };
        let (a, b);
        if options.jobs.unwrap_or(2) >= 2 {
            let (ca, cb) = (spawn(&first)?, spawn(&second)?);
            (a, b) = (ca.wait_with_output()?, cb.wait_with_output()?);
        } else {
            a = spawn(&first)?.wait_with_output()?;
            b = spawn(&second)?.wait_with_output()?;
        }
        checked += 1;
        let (da, db) = repin::run_dirs(root, &route.name);
        let found = if a.status.success() && b.status.success() {
            repin::differences(&da, &db)?
        } else {
            let tail = |o: &std::process::Output| {
                let text = String::from_utf8_lossy(&o.stderr).to_string() + &String::from_utf8_lossy(&o.stdout);
                text.lines().rev().take(3).collect::<Vec<_>>().join(" | ")
            };
            vec![format!("a replay failed (exit {:?} and {:?}): {}", a.status.code(), b.status.code(), tail(&a))]
        };
        if found.is_empty() {
            println!("PASS {}: two replays identical", route.name);
            let _ = std::fs::remove_dir_all(&da);
            let _ = std::fs::remove_dir_all(&db);
        } else {
            println!("FAIL {}: not deterministic", route.name);
            for line in found.iter().take(8) {
                println!("{}: {line}", route.name);
            }
            bad.push(route.name.clone());
        }
    }
    if bad.is_empty() {
        println!("{checked} route(s) deterministic on this disc.");
        Ok(())
    } else {
        Err(format!("nondeterministic: {}", bad.join(", ")).into())
    }
}

/// Replay every route tape against the final CUE; a fault or an incomplete tape fails the build.
/// The replay of one route tape, against `card` when the route boots with one.
fn replay_command(
    root: &Path,
    frontend: &Path,
    out: &Path,
    library: &Path,
    name: &str,
    card: Option<PathBuf>,
) -> Command {
    let tape = root.join(format!("tools/tapes/{name}.pxtape"));
    let mut args = vec![
        root.join("tools/replay_cue.py"),
        "--tape".into(),
        tape,
        "--output".into(),
        out.join(name),
        "--screenshot-interval".into(),
        "400".into(),
        "--frontend".into(),
        frontend.to_path_buf(),
        "--cue".into(),
        disc::disc_cue(library),
    ];
    if let Some(card) = card {
        args.extend(["--memcard".into(), card]);
    }
    let mut command = Command::new(venv_python(root));
    command
        .args(args)
        .current_dir(root)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    command
}

/// If a built-in renderer I-cache order is selected (game/build.rs,
/// game/hot-text-order.txt), verify every listed symbol's section is in the link map, at
/// ascending addresses in list order, with nothing else between them. A
/// renamed or inlined function would otherwise drop out of the list silently
/// and the layout would drift again. The names are mangled, so they carry the
/// crate's hash (`Cs..._6hk_psx`), which changes with the features and the
/// dependency graph: an ordinary build fails and prints the list under the
/// hash it found, a telemetry or pgo build only warns.
fn check_hot_order(root: &Path, strict: bool) -> Result<()> {
    let order = std::fs::read_to_string(root.join("game/hot-text-order.txt"))?;
    let wanted: Vec<&str> = order
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    if wanted.is_empty() {
        println!("No built-in hot text order selected; ordinary linker placement selected, route pacing checked separately");
        return Ok(());
    }
    let build: Value = serde_json::from_slice(&std::fs::read(root.join(".hkpsx/build.json"))?)?;
    let map = build["link_map"]["path"]
        .as_str()
        .ok_or("build.json has no link map")?;
    // LLD input-section rows: VMA LMA Size Align <object>:(<section>).
    let mut sections: Vec<(u32, u32, String)> = Vec::new();
    for line in std::fs::read_to_string(map)?.lines() {
        let Some(start) = line.find(":(.text.") else {
            continue;
        };
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 4 {
            continue;
        }
        let (Ok(address), Ok(size)) = (
            u32::from_str_radix(fields[0], 16),
            u32::from_str_radix(fields[2], 16),
        ) else {
            continue;
        };
        let name = line[start + 8..].trim_end_matches(')').to_string();
        sections.push((address, size, name));
    }
    sections.sort();
    let crate_hash = |name: &str| {
        name.find("_6hk_psx").and_then(|end| {
            name[..end]
                .rfind("Cs")
                .map(|start| name[start..end].to_string())
        })
    };
    let missing: Vec<&str> = wanted
        .iter()
        .copied()
        .filter(|w| !sections.iter().any(|s| s.2 == *w))
        .collect();
    if !missing.is_empty() {
        let found = sections.iter().find_map(|s| crate_hash(&s.2));
        let listed = crate_hash(wanted[0]);
        let mut message = format!(
            "hot-text-order.txt: {} of {} functions are not in the link ({})",
            missing.len(),
            wanted.len(),
            missing[0]
        );
        if let (Some(found), Some(listed)) = (found, listed) {
            if found != listed {
                let list: Vec<String> = wanted.iter().map(|w| w.replace(&listed, &found)).collect();
                message += &format!("; the crate hash is {found}, the list has {listed}. The list under this hash:\n{}", list.join("\n"));
            }
        }
        if strict {
            return Err(message.into());
        }
        println!("warning: {message}");
        return Ok(());
    }
    let mut previous: Option<usize> = None;
    for symbol in &wanted {
        let at = sections.iter().position(|s| s.2 == *symbol).unwrap_or(0);
        if let Some(p) = previous {
            if at != p + 1 {
                return Err(format!(
                    "hot-text-order.txt: {symbol} is not placed right after {}",
                    sections[p].2
                )
                .into());
            }
        }
        previous = Some(at);
    }
    let first = sections
        .iter()
        .find(|s| s.2 == wanted[0])
        .map(|s| s.0)
        .unwrap_or(0);
    println!(
        "Hot text order holds: {} functions from {first:#010x}",
        wanted.len()
    );
    Ok(())
}

/// One `#[no_mangle]` word of a replay's final RAM (`<output>/ram.bin`), at its
/// address in the build's link map (.hkpsx/build.json `link_map`), for
/// symbols tools/replay_cue.py does not watch.
fn ram_word(root: &Path, output: &Path, symbol: &str) -> Result<u32> {
    let build: Value = serde_json::from_slice(&std::fs::read(root.join(".hkpsx/build.json"))?)?;
    let map = build["link_map"]["path"]
        .as_str()
        .ok_or("build.json has no link map")?;
    // LLD map rows: VMA LMA Size Align Name.
    let address = std::fs::read_to_string(map)?
        .lines()
        .find_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            (fields.len() == 5 && fields[4] == symbol)
                .then(|| u32::from_str_radix(fields[0], 16).ok())
                .flatten()
        })
        .ok_or_else(|| format!("{symbol} is not in {map}"))?;
    let ram = std::fs::read(output.join("ram.bin"))?;
    let at = (address & 0x1f_ffff) as usize;
    let bytes = ram
        .get(at..at + 4)
        .ok_or("ram.bin is shorter than the symbol's address")?;
    Ok(u32::from_le_bytes(bytes.try_into()?))
}

const BASELINE_ROUTE_FRONTEND_SHA256: &str =
    "6fcd365e56011d071fde86ee5afb5086ed1a49a0c0e6a9cdc5af9d7e09e55e16";
const BASELINE_DISC_SHA256: &str =
    "f3fdc3e9094acf86eeb0ecddbb9489c82e8747e0200627c02c7d097d1e4726d5";

fn first_input_gap(output: &Path, record: &Value) -> Result<Option<(u64, u64)>> {
    let address = record["watches"]["HK_INPUT_MISSED_VBLANKS"]
        .as_str()
        .ok_or("missing input gap watch in replay receipt")?;
    let column = format!("ram_{}", address.trim_start_matches("0x"));
    let route = std::fs::read_to_string(output.join("route.csv"))?;
    let mut lines = route.lines();
    let header: Vec<_> = lines.next().ok_or("empty route CSV")?.split(',').collect();
    let index = header
        .iter()
        .position(|name| *name == column)
        .ok_or("input gap watch missing from route CSV")?;
    for line in lines {
        let fields: Vec<_> = line.split(',').collect();
        if fields
            .get(index)
            .ok_or("short route CSV row")?
            .parse::<u32>()?
            != 0
        {
            return Ok(Some((fields[0].parse()?, fields[1].parse()?)));
        }
    }
    Ok(None)
}

fn baseline_input_gap(
    name: &str,
    replay_exit: Option<i32>,
    record: &Value,
    pinned_route_frontend: bool,
    disc_sha: &str,
    tape_sha: &str,
    input_card_sha: Option<&str>,
    first_gap: Option<(u64, u64)>,
) -> bool {
    let (expected_tape, expected_card, expected_gap) = match name {
        "journey-false-knight" => (
            "0e3ac648402af2214399c9f30e543f42090f5e8bb95e7b9bbabd535187182683",
            "36834f395904f269cb16a7f27e3f17813d868a6ce4b3fcb5f6993b92ae27c9c5",
            (754, 394),
        ),
        "journey-reload" => (
            "5b9505c5b7a1485e37accdada8559a3cb594c737196f98344b4fbfd0c7d67f29",
            "f3cf2f4c697781b1b77716d237251ed2e7888ea56ca23ca54467e7002b34f380",
            (755, 394),
        ),
        _ => return false,
    };
    pinned_route_frontend
        && disc_sha == BASELINE_DISC_SHA256
        && tape_sha == expected_tape
        && input_card_sha == Some(expected_card)
        && first_gap == Some(expected_gap)
        && replay_exit == Some(1)
        && record["exit_code"] == 0
        && record["completed"] == true
        && record["inputs_unchanged"] == true
        && record["faults"] == serde_json::json!(["HK_INPUT_MISSED_VBLANKS"])
        && record["maximum_observed"]["HK_INPUT_MISSED_VBLANKS"] == 1
        && record["final_ram"]["HK_INPUT_MISSED_VBLANKS"] == 1
}

/// Replay every route tape against the final CUE; a fault or an incomplete tape fails the build.
fn validate(root: &Path, options: &Options) -> Result<()> {
    let frontend = frontend(root, options)?;
    let pinned_route_frontend = sha256_file(&frontend)? == BASELINE_ROUTE_FRONTEND_SHA256;
    let library = selected_library(options);
    let disc_sha = sha256_file(&disc::disc_bin(&library))?;
    let out = root.join(".hkpsx/validate");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out)?;
    // Replays are independent deterministic emulator runs: by default spawn
    // the independent routes at once and collect in route order. `--jobs N`
    // caps all replay concurrency, including the journey. `--prune` deletes each
    // passing route's dumps once it is judged (keeping its command.json), for a disk that cannot hold
    // every route's dumps at once (about 50 MB each).
    let all_routes = routes::load(root)?;
    // HK_VALIDATE_ONLY=route,route replays just those routes (the journey when "journey" is listed; no
    // power-cut or earned-focus checks), for work on one route.
    let only: Option<Vec<String>> =
        std::env::var("HK_VALIDATE_ONLY").ok().map(|list| list.split(',').map(str::to_owned).collect());
    let selected = |route: &routes::Route| {
        only.as_ref().is_none_or(|o| {
            o.iter().any(|x| *x == route.name || (route.journey.is_some() && x == "journey"))
        })
    };
    let plain: Vec<&routes::Route> =
        all_routes.iter().filter(|r| r.journey.is_none() && selected(r)).collect();
    // A journey segment boots from the card the previous one wrote: selecting one runs the ones before it.
    let last_journey = routes::last_journey_picked(&all_routes, |r| selected(r));
    let chain: Vec<&routes::Route> = all_routes
        .iter()
        .filter(|r| r.journey.is_some() && (selected(r) || routes::needs_card_from(r, last_journey)))
        .collect();
    let brought: Vec<&str> =
        chain.iter().filter(|r| !selected(r)).map(|r| r.name.as_str()).collect();
    if !brought.is_empty() {
        println!("Also running {} first: a journey segment boots from the card the one before it wrote.", brought.join(", "));
    }
    let jobs = options.jobs.unwrap_or(plain.len()).max(1);
    let gate = if all_routes.iter().any(|r| r.cuts.is_some()) {
        Some(gate_binary(options)?)
    } else {
        None
    };
    let spawn = |route: &routes::Route| {
        let card = scratch_card(root, route, &out);
        replay_command(root, &frontend, &out, &library, &route.name, card).spawn()
    };
    // The journey is one save carried through power cycles, so its segments
    // run in order after the independent routes: the first boots with no card
    // at all and every later one boots from a copy of the card its predecessor
    // wrote. A segment whose predecessor failed still runs, against whatever
    // that predecessor left, and fails on its own pins.
    let mut summary = serde_json::Map::new();
    let failed: std::cell::RefCell<Vec<String>> = std::cell::RefCell::new(Vec::new());
    let repins: std::cell::RefCell<Vec<repin::Change>> = std::cell::RefCell::new(Vec::new());
    let mut baseline_qualified = 0usize;
    // What a stored verdict depends on besides the route's own files: the judge and the replayer.
    let judge_sha = verdicts::key(&[
        &sha256_file(&std::env::current_exe()?)?,
        &sha256_file(&root.join("tools/replay_cue.py"))?,
        &sha256_file(&root.join("tools/validate_scene_gates.py"))?,
        &sha256_file(&frontend)?,
    ]);
    // Only an open-loop route with no cut list has a verdict worth keeping: the journey segments chain
    // cards, and a route with a cut list is judged again by the gate.
    let verdict_key = |route: &routes::Route| -> Result<Option<String>> {
        if route.journey.is_some() || route.cuts.is_some() {
            return Ok(None);
        }
        let tapes = root.join("tools/tapes");
        // A card the repository lacks is a fact about this route, not a reason to stop the run.
        let card = match &route.card {
            Some(card) => sha256_file(&root.join("tools/cards").join(card)).unwrap_or_else(|_| format!("no card file {card}")),
            None => String::new(),
        };
        Ok(Some(verdicts::key(&[
            &disc_sha,
            &judge_sha,
            &sha256_file(&tapes.join(format!("{}.pxtape", route.name)))?,
            &sha256_file(&tapes.join(format!("{}.pins", route.name)))?,
            &card,
        ])))
    };
    let mut evaluate = |route: &routes::Route,
                        replay: Option<std::io::Result<std::process::Output>>,
                        input_card_sha: Option<&str>,
                        key: Option<&str>|
     -> Result<()> {
        let name = route.name.as_str();
        let repins_before = repins.borrow().len();
        let Some(replay) = replay else {
            // A stored passing verdict for exactly this disc, judge, tape, pins and card.
            let entry = key.and_then(|k| verdicts::load(root, name, k)).ok_or("the stored verdict vanished")?;
            println!("CACHED {}", entry.line);
            verdicts::restore_record(root, name, &out.join(name))?;
            summary.insert(name.to_string(), entry.summary);
            if entry.baseline_qualified {
                baseline_qualified += 1;
            }
            return Ok(());
        };
        let gates = route.scene_gate;
        let output = out.join(name);
        let replay = replay?;
        let text = String::from_utf8_lossy(&replay.stdout).to_string();
        std::fs::write(
            out.join(format!("{name}.log")),
            format!("{text}{}", String::from_utf8_lossy(&replay.stderr)),
        )?;
        let verdict: Value = text
            .find('{')
            .and_then(|start| serde_json::from_str(&text[start..]).ok())
            .unwrap_or(Value::Null);
        // Both long False Knight segments inherit one missed VBlank in the
        // initial load on the canonical and old-main controls under frontend
        // 0868022. This release exception is bounded to that exact recorded
        // fault; all other faults, input drops and changed inputs remain fatal.
        let record: Value = std::fs::read(output.join("command.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or(Value::Null);
        let is_baseline_route = ["journey-false-knight", "journey-reload"].contains(&name);
        let tape_sha = if is_baseline_route {
            sha256_file(&root.join(format!("tools/tapes/{name}.pxtape")))?
        } else {
            String::new()
        };
        let first_gap = if is_baseline_route && record["completed"] == true {
            first_input_gap(&output, &record)?
        } else {
            None
        };
        let baseline_input_gap = baseline_input_gap(
            name,
            replay.status.code(),
            &record,
            pinned_route_frontend,
            &disc_sha,
            &tape_sha,
            input_card_sha,
            first_gap,
        );
        let mut ok =
            (replay.status.success() || baseline_input_gap) && verdict["completed"] == true;
        if ok && gates {
            if baseline_input_gap {
                let check = scene_gate::validate(root, &output);
                std::fs::write(
                    out.join(format!("{name}-gates.log")),
                    format!("{check:?}\n"),
                )?;
                ok = check.is_ok();
            } else {
                let check = Command::new(venv_python(root))
                    .args([
                        root.join("tools/validate_scene_gates.py").to_str().unwrap(),
                        output.to_str().unwrap(),
                    ])
                    .current_dir(root)
                    .output()?;
                std::fs::write(
                    out.join(format!("{name}-gates.log")),
                    [check.stdout.as_slice(), check.stderr.as_slice()].concat(),
                )?;
                ok = check.status.success();
            }
        }
        let ram = &verdict["final_ram"];
        // Every route: no scenery was drawn with another draw's vignette word
        // (render::HK_VIGNETTE_STALE). A view bind used to leave the old view's
        // words in place for up to sixteen frames, so scenery went dark or
        // vanished at each bind (Manny's King's Pass tape, 2026-10-03).
        match ram_word(root, &output, "HK_VIGNETTE_STALE") {
            Ok(0) => {}
            Ok(stale) => {
                println!("{name}: HK_VIGNETTE_STALE={stale} expected 0");
                ok = false;
            }
            Err(e) => {
                println!("{name}: HK_VIGNETTE_STALE unreadable: {e}");
                ok = false;
            }
        }
        // Every route: the camera never sat outside the cooked range of the view
        // being drawn (render::HK_CAMERA_VIEW_MISS), and no prop's art arrived
        // after its first visible frame (modules::HK_MODULE_ART_LATE). Either
        // one is scenery that vanishes or pops in while walking (Manny's King's
        // Pass tape, 2026-10-03).
        for symbol in ["HK_CAMERA_VIEW_MISS", "HK_MODULE_ART_LATE"] {
            match ram_word(root, &output, symbol) {
                Ok(0) => {}
                Ok(count)
                    if symbol == "HK_MODULE_ART_LATE"
                        && count as u64 <= route.art_late => {}
                Ok(count) => {
                    println!("{name}: {symbol}={count} expected 0");
                    ok = false;
                }
                Err(e) => {
                    println!("{name}: {symbol} unreadable: {e}");
                    ok = false;
                }
            }
        }
        // The route's pins. A route with a cut list is judged on its closed-loop run (below):
        // this tape counts polls, and a load that took other polls than the baseline's moves
        // everything after it. Its pins still tell us whether the tape played as recorded,
        // which is what the pacing figure is worth.
        let closed = route.cuts.is_some();
        let mut open_pins_hold = true;
        for pin in &route.pins {
            let held = ram[pin.symbol.as_str()].as_u64().is_some_and(|v| pin.op.holds(v, pin.value));
            if !held {
                open_pins_hold = false;
                if !closed {
                    println!(
                        "{name}: {}={} expected {}{}",
                        pin.symbol,
                        ram[pin.symbol.as_str()],
                        pin.op.words(),
                        pin.value
                    );
                    ok = false;
                }
            }
        }
        // Frame pacing (host/hk-build/pacing.rs, docs/ROUTES.md "Frame pacing"): the mean frame
        // time and the frames of four vblanks or more are the primary ceilings, the frames over
        // two vblanks the secondary one. A closed-loop route whose tape drifted on this disc did
        // not play the route that was measured, so its figures are not compared.
        if name.starts_with("journey-") {
            match brightness::failures(&output) {
                Ok(dark) => {
                    if let Ok(line) = brightness::summary(&output) {
                        println!("{name}: {line}");
                    }
                    for line in &dark {
                        println!("{name}: {line}");
                    }
                    if !dark.is_empty() {
                        ok = false;
                    }
                }
                Err(e) => {
                    println!("{name}: brightness unreadable: {e}");
                    ok = false;
                }
            }
        }
        match pacing::measure(&output) {
            Ok(measured) => {
                if closed && !open_pins_hold {
                    println!("{name}: {}, not judged: the open-loop tape drifted on this disc (the closed-loop run judges the route)", measured.report(&route.pacing_pins()));
                } else {
                    println!("{name}: {}", measured.report(&route.pacing_pins()));
                    let failures = measured.failures(&route.pacing_pins());
                    for failure in &failures {
                        println!("{name}: pacing over its ceiling: {failure}");
                    }
                    if !failures.is_empty() {
                        // An open-loop route with no cut list is judged on its outcome pins (above);
                        // its pacing follows the disc unless the frame time is a real slowdown.
                        match repin::decide(route, &measured) {
                            repin::Decision::Repin(changes) if options.repin && !closed && ok => {
                                println!("REPIN {name}: pacing ceilings follow the disc ({} moved)", changes.len());
                                repins.borrow_mut().extend(changes);
                            }
                            repin::Decision::Slowdown(why) if options.repin && !closed && ok => {
                                println!("{name}: {why}");
                                ok = false;
                            }
                            _ => ok = false,
                        }
                    }
                }
            }
            Err(e) => {
                println!("{name}: pacing unreadable: {e}");
                ok = false;
            }
        }
        let line = format!(
            "{} {name}: completed={} faults={} x={} y={} region={} scene_loads={} gate_loads={}",
            if !ok {
                "FAIL"
            } else if baseline_input_gap {
                "BASELINE"
            } else {
                "PASS"
            },
            verdict["completed"],
            verdict["faults"],
            ram["HK_PLAYER_X"],
            ram["HK_PLAYER_Y"],
            ram["HK_REGION_ID"],
            ram["HK_SCENE_LOADS"],
            ram["HK_SCENE_GATE_LOADS"]
        );
        println!("{line}");
        if !ok {
            failed.borrow_mut().push(name.to_string());
        } else if baseline_input_gap {
            baseline_qualified += 1;
        }
        if ok && options.prune {
            // The dumps and captures go; the replay's record stays, which
            // the power-cut check reads (bench-save/command.json).
            for entry in std::fs::read_dir(&output)? {
                let path = entry?.path();
                let keep = path.file_name().is_some_and(|n| {
                    ["command.json", "replay.log", "frontend-help.txt"]
                        .iter()
                        .any(|k| n == *k)
                });
                if keep {
                    continue;
                }
                if path.is_dir() {
                    std::fs::remove_dir_all(&path)?
                } else {
                    std::fs::remove_file(&path)?
                }
            }
        }
        let entry = serde_json::json!({"pass": ok && !baseline_input_gap, "baseline_qualified": ok && baseline_input_gap, "verdict": verdict});
        // A pass is kept for `--resume`, unless this very run moved the route's pins.
        if ok && repins.borrow().len() == repins_before {
            if let Some(key) = key {
                let stored = verdicts::Entry {
                    key: key.to_string(),
                    baseline_qualified: baseline_input_gap,
                    line,
                    summary: entry.clone(),
                };
                if let Err(error) = verdicts::store(root, name, &stored, &output) {
                    println!("{name}: verdict not kept ({error})");
                }
            }
        }
        summary.insert(name.to_string(), entry);
        Ok(())
    };
    // Closed-loop routes (those with a cut list) are judged by `psoxide-gate` walking the
    // tape stretch by stretch through the scene loads; see docs/ROUTES.md.
    let mut closed_summary = serde_json::Map::new();
    let mut judge_closed = |route: &routes::Route, run: routes::ClosedRun| -> Result<Option<PathBuf>> {
        let result = routes::finish_closed(run)?;
        println!("{} {} (closed loop): {}", if result.ok { "PASS" } else { "FAIL" }, route.name, result.headline);
        for line in &result.failures {
            println!("{}: {line}", route.name);
        }
        if !result.ok && !failed.borrow().iter().any(|n| *n == route.name) {
            failed.borrow_mut().push(route.name.clone());
        }
        closed_summary.insert(route.name.clone(), serde_json::json!({"pass": result.ok, "headline": result.headline}));
        Ok(result.card)
    };
    for chunk in plain.chunks(jobs) {
        let keys: Vec<Option<String>> = chunk.iter().map(|route| verdict_key(route)).collect::<Result<_>>()?;
        let cached: Vec<bool> = chunk
            .iter()
            .zip(&keys)
            .map(|(route, key)| {
                options.resume && key.as_deref().is_some_and(|k| verdicts::load(root, &route.name, k).is_some())
            })
            .collect();
        let children: Vec<_> = chunk
            .iter()
            .zip(&cached)
            .map(|(route, cached)| if *cached { None } else { Some(spawn(route)) })
            .collect();
        for ((route, child), key) in chunk.iter().zip(children).zip(&keys) {
            let replay = child.map(|c| c.and_then(|c| c.wait_with_output()));
            evaluate(route, replay, None, key.as_deref())?;
        }
        if let Some(gate) = &gate {
            let runs: Vec<_> = chunk
                .iter()
                .filter(|route| route.cuts.is_some())
                .map(|route| (*route, routes::spawn_closed(root, gate, &library, &out, route, None, false)))
                .collect();
            for (route, run) in runs {
                judge_closed(route, run?)?;
            }
        }
    }
    let mut open_card: Option<PathBuf> = None;
    let mut closed_card: Option<PathBuf> = None;
    for route in &chain {
        let name = route.name.as_str();
        let card = out.join(format!("{name}.mcd"));
        if let Some(previous) = &open_card {
            let _ = std::fs::copy(previous, &card);
        }
        let input_card_sha = if card.is_file() {
            Some(sha256_file(&card)?)
        } else {
            None
        };
        let output = replay_command(root, &frontend, &out, &library, name, Some(card.clone())).output();
        evaluate(route, Some(output), input_card_sha.as_deref(), None)?;
        open_card = Some(card);
        if let Some(gate) = &gate {
            if route.cuts.is_some() {
                let run = routes::spawn_closed(root, gate, &library, &out, route, closed_card.as_deref(), false)?;
                closed_card = judge_closed(route, run)?;
            }
        }
    }
    let repins = repins.into_inner();
    if !repins.is_empty() {
        let stamp = repin::utc_stamp(
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()),
        );
        repin::apply(root, &all_routes, &repins, &disc_sha, &stamp)?;
        println!("{} pacing ceiling(s) moved; recorded in {} (commit it with the change)", repins.len(), repin::LOG);
    }
    summary.insert("closed_loop".into(), Value::Object(closed_summary));
    summary.insert("disc_sha256".into(), Value::String(disc_sha.clone()));
    summary.insert(
        "gate_sha256".into(),
        Value::String(gate.as_ref().map(|g| g.sha256.clone()).unwrap_or_default()),
    );
    let bin = root.join("dist/hk-psx.exe");
    summary.insert(
        "exe_sha256".into(),
        Value::String(if bin.is_file() {
            sha256_file(&bin)?
        } else {
            String::new()
        }),
    );
    std::fs::write(
        out.join("summary.json"),
        serde_json::to_string_pretty(&Value::Object(summary))?,
    )?;
    let failed = failed.into_inner();
    if failed.is_empty() && only.is_some() {
        println!("{} selected routes pass.", plain.len() + chain.len());
        return Ok(());
    }
    if failed.is_empty() {
        println!(
            "{} unqualified routes pass; {baseline_qualified} routes complete under the documented one-count baseline input gap on the final CUE.",
            plain.len() + chain.len() - baseline_qualified
        );
        // Card writes free a file's directory entry before rewriting it, so a
        // save that used one file had a window where an interruption left
        // nothing loadable. Prove the alternating slots close it.
        let cut = Command::new(venv_python(root))
            .args([
                root.join("tools/validate_power_cut.py").to_str().unwrap(),
                "--output",
                out.join("power-cut").to_str().unwrap(),
            ])
            .current_dir(root)
            .output()?;
        std::fs::write(
            out.join("power-cut.log"),
            [cut.stdout.as_slice(), cut.stderr.as_slice()].concat(),
        )?;
        print!("{}", String::from_utf8_lossy(&cut.stdout));
        if !cut.status.success() {
            return Err(format!(
                "power-cut check failed: {}",
                String::from_utf8_lossy(&cut.stderr)
            )
            .into());
        }
        // The long save journey deliberately grants invincibility. The walks play without a cheat or a
        // recorded input: the whole road from the title, and the earned Focus proof on game state (the
        // tape proof `earned-focus` went red when new games moved to the top-gate drop).
        let gate = gate.as_ref().ok_or("the walks need psoxide-gate (--gate, or HK_PSX_GATE and HK_PSX_GATE_SHA256)")?;
        let failed = play_walks(root, &gate.binary, &library, &out.join("walks"), None)?;
        if failed.is_empty() {
            Ok(())
        } else {
            Err(format!("walks failed: {}", failed.join(", ")).into())
        }
    } else {
        Err(format!("routes failed: {}", failed.join(", ")).into())
    }
}

/// Play the walk scripts of `tools/walks` (docs/ROUTES.md, Walks) and report each. The walks play the
/// shipping disc (no route-input feature): `HK_WALK_LIBRARY` names its library when `library` is the
/// route-input twin the tapes need, otherwise they play `library`. Returns the names that failed.
fn play_walks(root: &Path, gate: &Path, library: &Path, out: &Path, only: Option<&[String]>) -> Result<Vec<String>> {
    let walk_library = std::env::var_os("HK_WALK_LIBRARY").map(PathBuf::from).unwrap_or_else(|| library.to_path_buf());
    let map = if walk_library.join("game.map").is_file() {
        walk_library.join("game.map")
    } else {
        let build: Value = serde_json::from_slice(&std::fs::read(root.join(".hkpsx/build.json"))?)?;
        PathBuf::from(build["link_map"]["path"].as_str().ok_or("build.json has no link map")?)
    };
    let mut failed = Vec::new();
    for (name, script) in walk::scripts(root)? {
        if !only.is_none_or(|o| o.iter().any(|x| *x == name || x == "walks")) {
            continue;
        }
        let (ok, headline) = walk::play(gate, &walk_library, &map, out, &name, &script)?;
        println!("{} {name} (walk): {headline}", if ok { "PASS" } else { "FAIL" });
        if !ok {
            failed.push(name);
        }
    }
    Ok(failed)
}

/// The `psoxide-gate` binary for closed-loop routes (`--gate`, or `HK_PSX_GATE` with its SHA-256).
fn gate_binary(options: &Options) -> Result<routes::Gate> {
    routes::Gate::resolve(options.gate.as_deref())
}

/// `cargo hk-build measure`, after one route's measuring run: read the neutral ticks per load
/// from the gate log and write them into the route's cuts file.
fn measured(root: &Path, out: &Path, route: &routes::Route) -> Result<()> {
    let cuts = route.cuts.as_ref().ok_or("route has no cut list")?;
    let log = std::fs::read_to_string(out.join(format!("{}-sd", route.name)).join("gate.log"))?;
    let pads = routes::parse_probes(&log, cuts.loads.len())?;
    let path = root.join(format!("tools/tapes/{}.cuts", route.name));
    let text = routes::set_pads(&std::fs::read_to_string(&path)?, &pads);
    std::fs::write(&path, text)?;
    println!("{}: neutral ticks per load {:?} -> {}", route.name, pads, path.display());
    Ok(())
}

/// `cargo hk-build routes`: play only the closed-loop routes (those with a cut list) on the
/// selected disc, without the open-loop replays. HK_VALIDATE_ONLY narrows the list.
fn closed_routes(root: &Path, options: &Options, measure: bool) -> Result<()> {
    let library = selected_library(options);
    let gate = gate_binary(options)?;
    let out = root.join(".hkpsx/validate-closed");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out)?;
    let only: Option<Vec<String>> =
        std::env::var("HK_VALIDATE_ONLY").ok().map(|list| list.split(',').map(str::to_owned).collect());
    let everything = routes::load(root)?;
    let named = |r: &routes::Route| {
        only.as_ref()
            .is_none_or(|o| o.iter().any(|x| *x == r.name || (r.journey.is_some() && x == "journey")))
    };
    // A journey segment boots from the card the previous one wrote: selecting one runs the ones before it.
    let last_journey = routes::last_journey_picked(&everything, |r| r.cuts.is_some() && named(r));
    let selected: Vec<routes::Route> = everything
        .iter()
        .filter(|r| r.cuts.is_some())
        .filter(|r| named(r) || routes::needs_card_from(r, last_journey))
        .cloned()
        .collect();
    let brought: Vec<&str> = selected.iter().filter(|r| !named(r)).map(|r| r.name.as_str()).collect();
    if !brought.is_empty() {
        println!("Also running {} first: a journey segment boots from the card the one before it wrote.", brought.join(", "));
    }
    let jobs = options.jobs.unwrap_or(2).max(1);
    let mut failed = Vec::new();
    let mut judge = |route: &routes::Route, run: routes::ClosedRun| -> Result<Option<PathBuf>> {
        let result = routes::finish_closed(run)?;
        // A measuring run fails its probes on purpose (the gate prints a failing assert's value).
        let ok = result.ok
            || (measure
                && result.failures.iter().all(|l| l.contains("[probe-") || l.as_str() == result.headline.trim()));
        println!("{} {} (closed loop): {}", if ok { "PASS" } else { "FAIL" }, route.name, result.headline);
        for line in result.failures.iter().filter(|l| !(measure && (l.contains("[probe-") || l.as_str() == result.headline.trim()))) {
            println!("{}: {line}", route.name);
        }
        if !ok {
            failed.push(route.name.clone());
        }
        Ok(result.card)
    };
    let plain: Vec<&routes::Route> = selected.iter().filter(|r| r.journey.is_none()).collect();
    for chunk in plain.chunks(jobs) {
        let runs: Vec<_> = chunk
            .iter()
            .map(|route| (*route, routes::spawn_closed(root, &gate, &library, &out, route, None, measure)))
            .collect();
        for (route, run) in runs {
            judge(route, run?)?;
            if measure {
                measured(root, &out, route)?;
            }
        }
    }
    let mut card: Option<PathBuf> = None;
    for route in selected.iter().filter(|r| r.journey.is_some()) {
        let run = routes::spawn_closed(root, &gate, &library, &out, route, card.as_deref(), measure)?;
        card = judge(route, run)?;
        if measure {
            measured(root, &out, route)?;
        }
    }
    // The walk scripts (tools/walks): journeys written as goals, played the same way.
    if !measure {
        failed.extend(play_walks(root, &gate.binary, &library, &out, only.as_deref())?);
    }
    if failed.is_empty() {
        println!("{} closed-loop routes pass.", selected.len());
        Ok(())
    } else {
        Err(format!("closed-loop routes failed: {}", failed.join(", ")).into())
    }
}

fn usage() -> ! {
    eprintln!(
        "usage: cargo hk-build <cook|build|pgo|validate|routes|cuts|tape-hold|tape-stall|tape-extend|pace-pins|earned-focus|gate-receipt|disc|card-check|intro-cook|store|determinism|help> [--recook] [--telemetry] [--no-validate]\n       \
         [--hollow-knight DIR] [--sdk-source DIR] [--emulator-source DIR] [--frontend PATH] [--tape PATH]... [--jobs N] [--prune] [--repin] [--resume]\n\
         cook      verify source and SDK, then cook regions only; stop before guest work\n\
         build     cook (cached), build the guest, replace 'Hollow Knight (hk-psx)'.bin/.cue in --library, report, then validate\n\
         pgo       build, with the guest optimised by a profile of --tape replays (default: kings-climb\n          \
         and crossroads-gate), replayed on the emulator pinned in emulator.lock.json (built once\n          \
         from --emulator-source, default ../PSoXide-emulator); needs a checkout path without spaces\n\
         validate  replay the route tapes in tools/tapes against the current CUE, on the pinned\n          \
         emulator unless --frontend (or HK_PSX_FRONTEND) names another\n\
         determinism  replay each selected open-loop route twice on the same disc and fail on any difference\n          \
         (HK_VALIDATE_ONLY narrows); validate --repin moves drifted pacing ceilings and logs them in docs/REPIN-LOG.md;\n          \
         validate --resume skips the open-loop routes that already passed on this disc, judge, tape, pins and card\n\
         measure   play the closed-loop routes without the route lock and write each load's\n          \
         neutral ticks (telemetry) into the cuts files\n\
         routes    play only the closed-loop routes (those with a .cuts file) with psoxide-gate (--gate, or\n          \
         HK_PSX_GATE and HK_PSX_GATE_SHA256) against the current CUE\n\
         cuts      write NAME.cuts from the baseline replays in --work (default .hkpsx/validate), for every\n          \
         --tape NAME with two or more scene loads (HK_CUTS_MIN=1 lets a one-load route have a cut list, for a wait)\n\
         pace-pins [RUN_DIR]  rewrite the pacing ceilings of the route pin files from a validate run (default .hkpsx/validate)\n\
         tape-hold NAME FIRST LAST BUTTON  hold BUTTON over polls FIRST..LAST of tools/tapes/NAME.pxtape\n\
         tape-extend NAME POLLS  add POLLS neutral polls to the end of tools/tapes/NAME.pxtape (then tape-hold presses into them)\n\
         tape-stall NAME POLL TICKS  repeat sample POLL of tools/tapes/NAME.pxtape TICKS more times (a hard landing's lock)\n\
         card-check  list a memory card image (.mcd) the way the console's card manager would, and check its layout\n          \
         earned-focus  prove four earned hits, two kills, real damage, and one Focus heal\n\
         gate-receipt  check one baseline-qualified scene-gate replay at --work\n\
         disc      pack the disc from the guest build in --work (default build/) into --library\n          \
         (default the disc library): WORLD.PAK and the XA songs, plus any --cdda-track\n          \
         --library applies to build, pgo, validate and disc\n\
         disc --intro  the same disc with the opening movies (the shipping disc has them already; a\n          \
         disc built with guest features, the replay gates' route-input one, or --no-intro, has not)\n\
         store status|key|restore [--force]|publish [--label L]|dedupe|gc [--keep N] [--apply]|verify|phys DIR...  the shared cook store\n          \
         (HK_COOK_STORE=DIR; docs/COOK-STORE.md): restore exits 3 when it holds no snapshot for this checkout\n\
         intro-cook  encode the install's Prologue and Intro clips into .hkpsx/intro (host/hk-intro-cook; --recook\n          cooks again) and compile the player into data/intro.exe"
    );
    std::process::exit(2)
}

/// `store status|key|restore|publish|dedupe|gc|verify|phys`: the shared cook store (docs/COOK-STORE.md).
/// Exit status 0 is success; `restore` exits 3 when the store has no snapshot for this checkout.
fn store_command(root: &Path, args: &[String]) -> Result<i32> {
    let sub = args.first().map(String::as_str).unwrap_or("status");
    if sub == "phys" {
        let dirs: Vec<PathBuf> = args[1..].iter().map(PathBuf::from).collect();
        cook_store::physical_report(&dirs)?;
        return Ok(0);
    }
    let store = cook_store::Store::from_env()
        .ok_or_else(|| format!("set {} to the store directory", cook_store::STORE_ENV))?;
    let flag = |name: &str| args.iter().any(|a| a == name);
    let value = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1));
    // The key needs the selected install (doctor.json) and the source manifest.
    let key = || -> Result<String> {
        std::fs::create_dir_all(root.join(".hkpsx"))?;
        if !root.join(".hkpsx/doctor.json").exists() {
            run("python3", &[root.join("tools/doctor.py").to_str().unwrap()], root)?;
        }
        write_source_input_fingerprints(root, &selected_source(root)?)?;
        cook_store_key(root)
    };
    match sub {
        "status" => {
            cook_store::status(&store, root)?;
            Ok(0)
        }
        "key" => {
            println!("{}", key()?);
            Ok(0)
        }
        "restore" => {
            let key = key()?;
            if cook_store::lane_in_sync(root, &key) && !flag("--force") {
                println!("this lane is already at snapshot {}", &key[..16]);
            } else if store.has_key(&key) {
                let r = cook_store::restore(root, &store, &key)?;
                println!(
                    "restored snapshot {}: {} files, {:.2} GB, {} replaced, {} stale removed",
                    &key[..16],
                    r.files,
                    r.bytes as f64 / 1e9,
                    r.replaced,
                    r.pruned
                );
            } else {
                println!("no snapshot for {} in {}", &key[..16], store.dir().display());
                return Ok(3);
            }
            Ok(0)
        }
        "publish" => {
            let key = key()?;
            let label = value("--label").cloned().unwrap_or_else(|| cook_store_label(root));
            let r = cook_store::publish(root, &store, &key, &label)?;
            println!("published {}: {} files, {} new objects, {} relinked", &key[..16], r.files, r.new_objects, r.relinked);
            Ok(0)
        }
        "dedupe" => {
            let r = cook_store::dedupe(root, &store)?;
            println!("{} files ({:.2} GB) given back to the store's blocks", r.relinked, r.relinked_bytes as f64 / 1e9);
            Ok(0)
        }
        "gc" => {
            let keep = value("--keep").and_then(|v| v.parse().ok()).unwrap_or(3);
            let r = cook_store::gc(&store, keep, flag("--apply"))?;
            println!(
                "{} {} snapshot(s) beyond the newest {keep}, {} object(s) ({:.2} GB){}",
                if flag("--apply") { "dropped" } else { "would drop" },
                r.dropped_keys.len(),
                r.removed_objects,
                r.removed_bytes as f64 / 1e9,
                if flag("--apply") { "" } else { " (dry run; pass --apply)" }
            );
            Ok(0)
        }
        "verify" => {
            let bad = cook_store::verify(&store)?;
            println!("{} corrupt object(s)", bad.len());
            Ok(if bad.is_empty() { 0 } else { 1 })
        }
        _ => usage(),
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let action = args.next().unwrap_or_else(|| "build".into());
    if action == "card-check" {
        let files: Vec<String> = args.collect();
        if files.is_empty() {
            usage();
        }
        if let Err(error) = card_check::run(&files) {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
        return;
    }
    if action == "walk" {
        // walk SCRIPT LIBRARY OUT: compile a walk script (docs/ROUTES.md, Walks) to a psoxide-gate journey for the disc in LIBRARY.
        let (script, library, out) = (args.next(), args.next(), args.next());
        let (Some(script), Some(library), Some(out)) = (script, library, out) else {
            usage()
        };
        let result = (|| -> Result<()> {
            let library = std::path::PathBuf::from(library);
            let cue = disc::disc_cue(&library);
            let map = library.join("game.map");
            let text = walk::expand(std::path::Path::new(&script))?;
            let toml = walk::compile(&text, &walk::Setup { disc: &cue, symbols: &map })?;
            std::fs::write(&out, toml)?;
            Ok(())
        })();
        if let Err(error) = result {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
        return;
    }
    if action == "tape-hold" {
        // tape-hold NAME FIRST LAST BUTTON: hold BUTTON (cross, or several joined with +) over polls FIRST..LAST of tools/tapes/NAME.pxtape.
        let (name, first, last, button) = (args.next(), args.next(), args.next(), args.next());
        let (Some(name), Some(Ok(first)), Some(Ok(last)), Some(button)) =
            (name, first.map(|v| v.parse::<usize>()), last.map(|v| v.parse::<usize>()), button)
        else {
            usage()
        };
        let result = (|| -> Result<()> {
            let mut mask = 0;
            for name in button.split('+') {
                mask |= routes::button_mask(name).ok_or(format!("unknown button {name}"))?;
            }
            let path = root().join(format!("tools/tapes/{name}.pxtape"));
            std::fs::write(&path, routes::hold_buttons(&std::fs::read(&path)?, first, last, mask)?)?;
            println!("{name}: {button} held over polls {first}..{last}");
            Ok(())
        })();
        if let Err(error) = result {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
        return;
    }
    if action == "tape-extend" {
        // tape-extend NAME POLLS: add POLLS neutral polls to the end of tools/tapes/NAME.pxtape.
        let (name, polls) = (args.next(), args.next());
        let (Some(name), Some(Ok(polls))) = (name, polls.map(|v| v.parse::<usize>())) else {
            usage()
        };
        let result = (|| -> Result<()> {
            let path = root().join(format!("tools/tapes/{name}.pxtape"));
            std::fs::write(&path, routes::extend_tape(&std::fs::read(&path)?, polls)?)?;
            println!("{name}: {polls} neutral polls added");
            Ok(())
        })();
        if let Err(error) = result {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
        return;
    }
    if action == "tape-stall" {
        // tape-stall NAME POLL TICKS: repeat sample POLL of tools/tapes/NAME.pxtape TICKS more times.
        let (name, poll, ticks) = (args.next(), args.next(), args.next());
        let (Some(name), Some(Ok(poll)), Some(Ok(ticks))) =
            (name, poll.map(|v| v.parse::<usize>()), ticks.map(|v| v.parse::<usize>()))
        else {
            usage()
        };
        let result = (|| -> Result<()> {
            let path = root().join(format!("tools/tapes/{name}.pxtape"));
            std::fs::write(&path, routes::stall_sample(&std::fs::read(&path)?, poll, ticks)?)?;
            println!("{name}: {ticks} ticks added after poll {poll}");
            Ok(())
        })();
        if let Err(error) = result {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
        return;
    }
    if action == "pace-pins" {
        // pace-pins [RUN_DIR]: rewrite the pacing ceilings of every route pin file from a validate run
        // (default .hkpsx/validate): the rule in pacing.rs, documented in docs/ROUTES.md.
        let dir = args.next().map(PathBuf::from).unwrap_or_else(|| root().join(".hkpsx/validate"));
        if let Err(error) = pacing::write_pins(&root(), &dir) {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
        return;
    }
    if action == "store" {
        let rest: Vec<String> = args.collect();
        match store_command(&root(), &rest) {
            Ok(code) => std::process::exit(code),
            Err(error) => {
                eprintln!("error: {error}");
                std::process::exit(1);
            }
        }
    }
    if action == "region-cache-check" || action == "region-cache-record" {
        let region = args.next().unwrap_or_else(|| usage());
        if args.next().is_some() {
            usage();
        }
        if let Err(error) = region_cache::run(&root(), &action, &region) {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
        return;
    }
    let mut options = Options {
        hollow_knight: None,
        sdk_source: None,
        emulator_source: None,
        frontend: None,
        gate: None,
        telemetry: false,
        recook: false,
        validate: true,
        tapes: Vec::new(),
        jobs: None,
        prune: false,
        repin: false,
        resume: false,
        work: None,
        library: None,
        cdda: Vec::new(),
        intro: false,
        no_intro: false,
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--recook" => options.recook = true,
            "--telemetry" => options.telemetry = true,
            "--no-validate" => options.validate = false,
            "--hollow-knight" => {
                options.hollow_knight = Some(PathBuf::from(args.next().unwrap_or_else(|| usage())))
            }
            "--sdk-source" => {
                options.sdk_source = Some(PathBuf::from(args.next().unwrap_or_else(|| usage())))
            }
            "--emulator-source" => {
                options.emulator_source =
                    Some(PathBuf::from(args.next().unwrap_or_else(|| usage())))
            }
            "--frontend" => {
                options.frontend = Some(PathBuf::from(args.next().unwrap_or_else(|| usage())))
            }
            "--gate" => options.gate = Some(PathBuf::from(args.next().unwrap_or_else(|| usage()))),
            "--tape" => options
                .tapes
                .push(PathBuf::from(args.next().unwrap_or_else(|| usage()))),
            "--jobs" => {
                options.jobs = Some(
                    args.next()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or_else(|| usage()),
                )
            }
            "--prune" => options.prune = true,
            "--repin" => options.repin = true,
            "--resume" => options.resume = true,
            "--intro" => options.intro = true,
            "--no-intro" => options.no_intro = true,
            "--work" => options.work = Some(PathBuf::from(args.next().unwrap_or_else(|| usage()))),
            "--library" => {
                options.library = Some(PathBuf::from(args.next().unwrap_or_else(|| usage())))
            }
            "--cdda-track" => options
                .cdda
                .push(PathBuf::from(args.next().unwrap_or_else(|| usage()))),
            _ => usage(),
        }
    }
    let root = root();
    std::env::set_var("HK_DISC_LIBRARY", selected_library(&options));
    let result = match action.as_str() {
        "cook" => build(&root, &options, BuildMode::CookOnly),
        "build" => build(&root, &options, BuildMode::Normal),
        "pgo" => build(&root, &options, BuildMode::Pgo),
        "validate" => validate(&root, &options),
        "determinism" => determinism(&root, &options),
        "routes" => closed_routes(&root, &options, false),
        "measure" => closed_routes(&root, &options, true),
        "cuts" => routes::write_cuts(&root, &options.work.clone().unwrap_or_else(|| root.join(".hkpsx/validate")), &options.tapes.iter().filter_map(|t| t.file_stem().map(|s| s.to_string_lossy().to_string())).collect::<Vec<_>>(), std::env::var("HK_CUTS_MIN").ok().and_then(|v| v.parse().ok()).unwrap_or(2)),
        "earned-focus" => frontend(&root, &options).and_then(|frontend| {
            earned_focus::validate(
                &root,
                &frontend,
                &selected_library(&options),
                &root.join(".hkpsx/earned-focus"),
            )
        }),
        "gate-receipt" => options
            .work
            .as_ref()
            .ok_or_else(|| "gate-receipt needs --work REPLAY_DIRECTORY".into())
            .and_then(|work: &PathBuf| scene_gate::validate(&root, work)),
        "disc" => {
            let work = options.work.clone().unwrap_or_else(|| root.join("build"));
            let library = selected_library(&options);
            let songs = disc::Songs {
                xa: vec![root.join("data/music.xa")],
                cdda: options.cdda.clone(),
            };
            intro_stage(&root, intro_plan(&options, shipping_guest())).and_then(|stage| {
                disc::package(
                    &root,
                    &work.join("hk-psx-patched.exe"),
                    &work,
                    &library,
                    &songs,
                    stage.as_ref(),
                )
            })
        }
        "intro-cook" => {
            if options.recook {
                let _ = std::fs::remove_dir_all(intro::dir(&root));
            }
            // The movies when they are not cooked yet, and the player, so that a guest build that
            // follows has data/intro.exe.
            intro_stage(&root, IntroPlan::Required).map(|_| ())
        }
        _ => usage(),
    };
    if let Err(error) = result {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(intro: bool, no_intro: bool) -> Options {
        Options {
            hollow_knight: None,
            sdk_source: None,
            emulator_source: None,
            frontend: None,
            gate: None,
            telemetry: false,
            recook: false,
            validate: false,
            tapes: Vec::new(),
            jobs: None,
            prune: false,
        repin: false,
        resume: false,
            work: None,
            library: None,
            cdda: Vec::new(),
            intro,
            no_intro,
        }
    }

    #[test]
    fn the_shipping_disc_carries_the_movies_and_a_test_disc_does_not() {
        // No guest feature: the disc a player gets.
        assert_eq!(intro_plan(&options(false, false), true), IntroPlan::IfPossible);
        // A route-input (or any featured) disc is pressed through by poll count and must not meet a movie.
        assert_eq!(intro_plan(&options(false, false), false), IntroPlan::Without);
        // `--intro` asks for them whatever the features, and fails when they cannot be cooked.
        assert_eq!(intro_plan(&options(true, false), false), IntroPlan::Required);
        assert_eq!(intro_plan(&options(true, false), true), IntroPlan::Required);
        // `--no-intro` wins on a shipping disc.
        assert_eq!(intro_plan(&options(false, true), true), IntroPlan::Without);
    }

    #[test]
    fn external_profile_frontend_is_hash_bound_and_records_external_identity() {
        let dir = tree("external-profile");
        let path = dir.join("retained-frontend");
        std::fs::write(&path, b"abc").unwrap();
        let digest = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let (selected, identity) = external_profile_frontend(path.clone(), digest).unwrap();
        assert_eq!(selected, path);
        assert_eq!(identity, format!("external:sha256:{digest}"));
        std::fs::write(&path, b"changed").unwrap();
        assert!(external_profile_frontend(path.clone(), digest).is_err());
        std::fs::remove_file(&path).unwrap();
        assert!(external_profile_frontend(path, digest).is_err());
        assert!(!dir.join(".hkpsx/emulator").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn external_profile_frontend_rejects_missing_or_malformed_recorded_digest() {
        for digest in ["", "abc", "gggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggg"] {
            assert!(external_profile_frontend(PathBuf::from("absent"), digest).is_err());
        }
    }

    #[test]
    fn baseline_input_exception_requires_exact_completed_control() {
        let good = serde_json::json!({
            "exit_code": 0,
            "completed": true,
            "inputs_unchanged": true,
            "faults": ["HK_INPUT_MISSED_VBLANKS"],
            "maximum_observed": {"HK_INPUT_MISSED_VBLANKS": 1},
            "final_ram": {"HK_INPUT_MISSED_VBLANKS": 1},
        });
        let tape = "0e3ac648402af2214399c9f30e543f42090f5e8bb95e7b9bbabd535187182683";
        let card = "36834f395904f269cb16a7f27e3f17813d868a6ce4b3fcb5f6993b92ae27c9c5";
        let check = |name, status, record: &Value, frontend, disc, tape, card, gap| {
            baseline_input_gap(name, status, record, frontend, disc, tape, card, gap)
        };
        let valid = |record: &Value| {
            check(
                "journey-false-knight",
                Some(1),
                record,
                true,
                BASELINE_DISC_SHA256,
                tape,
                Some(card),
                Some((754, 394)),
            )
        };
        assert!(valid(&good));
        assert!(check(
            "journey-reload",
            Some(1),
            &good,
            true,
            BASELINE_DISC_SHA256,
            "5b9505c5b7a1485e37accdada8559a3cb594c737196f98344b4fbfd0c7d67f29",
            Some("f3cf2f4c697781b1b77716d237251ed2e7888ea56ca23ca54467e7002b34f380"),
            Some((755, 394))
        ));
        assert!(!check(
            "bench-save",
            Some(1),
            &good,
            true,
            BASELINE_DISC_SHA256,
            tape,
            Some(card),
            Some((754, 394))
        ));
        assert!(!check(
            "journey-false-knight",
            Some(0),
            &good,
            true,
            BASELINE_DISC_SHA256,
            tape,
            Some(card),
            Some((754, 394))
        ));
        assert!(!check(
            "journey-false-knight",
            Some(1),
            &good,
            false,
            BASELINE_DISC_SHA256,
            tape,
            Some(card),
            Some((754, 394))
        ));
        assert!(!check(
            "journey-false-knight",
            Some(1),
            &good,
            true,
            "other disc",
            tape,
            Some(card),
            Some((754, 394))
        ));
        assert!(!check(
            "journey-false-knight",
            Some(1),
            &good,
            true,
            BASELINE_DISC_SHA256,
            "other tape",
            Some(card),
            Some((754, 394))
        ));
        assert!(!check(
            "journey-false-knight",
            Some(1),
            &good,
            true,
            BASELINE_DISC_SHA256,
            tape,
            Some("other card"),
            Some((754, 394))
        ));
        assert!(!check(
            "journey-false-knight",
            Some(1),
            &good,
            true,
            BASELINE_DISC_SHA256,
            tape,
            Some(card),
            Some((755, 394))
        ));
        let mut bad = good.clone();
        bad["faults"] = serde_json::json!(["HK_INPUT_MISSED_VBLANKS", "HK_INPUT_FAULT"]);
        assert!(!valid(&bad));
        let mut bad = good.clone();
        bad["maximum_observed"]["HK_INPUT_MISSED_VBLANKS"] = serde_json::json!(2);
        assert!(!valid(&bad));
        let mut bad = good.clone();
        bad["completed"] = serde_json::json!(false);
        assert!(!valid(&bad));
        let mut bad = good.clone();
        bad["exit_code"] = serde_json::json!(1);
        assert!(!valid(&bad));
        let mut bad = good.clone();
        bad["inputs_unchanged"] = serde_json::json!(false);
        assert!(!valid(&bad));
        assert!(!valid(&Value::Null));
    }

    #[test]
    fn cook_action_stops_before_guest_stage() {
        assert!(!BuildMode::CookOnly.runs_guest());
        assert!(BuildMode::Normal.runs_guest());
        assert!(BuildMode::Pgo.runs_guest());
    }

    #[test]
    fn fresh_region_cook_uses_fingerprinted_parallel_workers() {
        let dir = tree("region-cook-args");
        let provenance = dir.join(".hkpsx/regions-provenance.json");
        assert_eq!(region_cook_args(&provenance), &["--serial"]);
        std::fs::create_dir_all(provenance.parent().unwrap()).unwrap();
        std::fs::write(
            provenance.with_file_name("regions-source-inputs.json"),
            "{}",
        )
        .unwrap();
        assert!(region_cook_args(&provenance).is_empty());
        std::fs::write(&provenance, "{}\n").unwrap();
        assert!(region_cook_args(&provenance).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn source_fingerprints_change_with_content_and_reject_symlinks() {
        let dir = tree("source-inputs");
        let source = dir.join("retail");
        std::fs::create_dir_all(source.join("sub")).unwrap();
        std::fs::write(source.join("sub/data"), b"first").unwrap();
        let first = source_input_fingerprints(&source).unwrap();
        assert_eq!(first["inputs"].as_object().unwrap().len(), 1);
        std::fs::write(source.join("sub/data"), b"other").unwrap();
        assert_ne!(source_input_fingerprints(&source).unwrap(), first);
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(source.join("sub/data"), source.join("alias")).unwrap();
            assert!(source_input_fingerprints(&source).is_err());
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn selected_library_reaches_report_paths_and_replay_without_canonical_disc() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let library =
            std::env::temp_dir().join(format!("hk-library-test-{}-{nonce}", std::process::id()));
        let options = Options {
            hollow_knight: None,
            sdk_source: None,
            emulator_source: None,
            frontend: None,
            gate: None,
            telemetry: false,
            recook: false,
            validate: false,
            tapes: Vec::new(),
            jobs: Some(2),
            prune: false,
        repin: false,
        resume: false,
            work: None,
            library: Some(library.clone()),
            cdda: Vec::new(),
            intro: false,
            no_intro: false,
        };
        assert_eq!(selected_library(&options), library);
        assert_ne!(selected_library(&options), disc_library());
        let command = replay_command(
            &root(),
            Path::new("/unused/frontend"),
            Path::new("/unused/output"),
            &library,
            "focus",
            None,
        );
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        let cue_arg = args.iter().position(|arg| arg == "--cue").unwrap();
        assert_eq!(
            args[cue_arg + 1],
            disc::disc_cue(&library).to_string_lossy()
        );
        let python = std::process::Command::new("python3")
            .arg("-c")
            .arg("import sys;sys.path.insert(0,'host');from paths import artifacts;print(artifacts()['cue'])")
            .current_dir(root())
            .env("HK_DISC_LIBRARY", &library)
            .output()
            .unwrap();
        assert!(
            python.status.success(),
            "{}",
            String::from_utf8_lossy(&python.stderr)
        );
        assert_eq!(
            String::from_utf8(python.stdout).unwrap().trim(),
            disc::disc_cue(&library).to_string_lossy()
        );
        assert!(!library.exists(), "path selection must not write a disc");
    }

    fn tree(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hk-build-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("tools")).unwrap();
        std::fs::write(
            dir.join("components.lock.json"),
            r#"{"components": {"sdk": {"revision": "0123456789abcdef0123456789abcdef01234567"}}}"#,
        )
        .unwrap();
        dir
    }

    #[test]
    fn an_old_pin_is_hydrated_by_its_python_script() {
        let dir = tree("old");
        std::fs::write(dir.join("tools/bootstrap-components.py"), "").unwrap();
        let (program, args) = components_command(&dir, "sdk=/sdk").unwrap();
        assert_eq!(program, PathBuf::from("python3"));
        assert_eq!(
            args,
            [
                "tools/bootstrap-components.py",
                "--root",
                dir.to_str().unwrap(),
                "--source",
                "sdk=/sdk"
            ]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_new_pin_is_hydrated_by_psoxide_components_for_its_locked_sdk() {
        let dir = tree("new");
        let (program, args) = components_command(&dir, "sdk=/sdk").unwrap();
        assert_eq!(
            program,
            dir.join(".tools/sdk-0123456789abcdef0123456789abcdef01234567/bin/psoxide-components")
        );
        assert_eq!(
            args,
            ["--root", dir.to_str().unwrap(), "--source", "sdk=/sdk"]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A tree with the directories `assets_key` reads and the three cache records it names.
    fn asset_key_tree(name: &str) -> PathBuf {
        let dir = tree(name);
        for sub in [
            "host/hk-cook/src",
            "host/hk-unity/src",
            "host/hk-unity/data",
            "host/hk-pil/src",
            "host/hk-dotnet/src",
            ".hkpsx",
            "data",
        ] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        std::fs::write(dir.join("host/charms.py"), "x = 1\n").unwrap();
        std::fs::write(dir.join("host/hk-cook/src/lib.rs"), "// cook\n").unwrap();
        std::fs::write(dir.join(REGIONS_BASE), "{\"base\": 1}").unwrap();
        std::fs::write(dir.join("data/regions.json"), "{\"base\": 1}").unwrap();
        std::fs::write(dir.join(".hkpsx/regions-cook-cache.json"), "{}").unwrap();
        std::fs::write(dir.join(".hkpsx/menu-provenance.json"), "{}").unwrap();
        dir
    }

    #[test]
    fn the_asset_key_ignores_the_report_the_asset_stage_rewrites() {
        let dir = asset_key_tree("key-live");
        let cold = assets_key(&dir).unwrap();
        // crossroads22_wave::prepare rewrites the live report during the asset stage.
        std::fs::write(dir.join("data/regions.json"), "{\"base\": 1, \"crossroads22_wave_artifact\": \"x\"}")
            .unwrap();
        assert_eq!(assets_key(&dir).unwrap(), cold, "a no-op rebuild must keep the key");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_asset_key_follows_every_real_input() {
        let dir = asset_key_tree("key-inputs");
        let cold = assets_key(&dir).unwrap();
        let changed = |path: &str, bytes: &str| {
            let file = dir.join(path);
            let before = std::fs::read(&file).unwrap();
            std::fs::write(&file, bytes).unwrap();
            let key = assets_key(&dir).unwrap();
            std::fs::write(&file, before).unwrap();
            key
        };
        assert_ne!(changed(REGIONS_BASE, "{\"base\": 2}"), cold, "the region cook's report");
        assert_ne!(changed("host/charms.py", "x = 2\n"), cold, "a Python cooker");
        assert_ne!(changed("host/hk-cook/src/lib.rs", "// other\n"), cold, "a Rust cooker");
        assert_ne!(changed(".hkpsx/menu-provenance.json", "{\"m\": 1}"), cold, "the menu cook");
        assert_eq!(assets_key(&dir).unwrap(), cold, "restoring the inputs restores the key");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_card_fixture_the_repository_lacks_is_a_warning_and_a_blank_card_not_an_abort() {
        let dir = tree("missing-card");
        std::fs::create_dir_all(dir.join("tools/cards")).unwrap();
        std::fs::write(dir.join("tools/cards/there.mcd"), b"card").unwrap();
        let route = |card: Option<&str>| routes::Route {
            name: "r".into(),
            card: card.map(str::to_owned),
            ..routes::Route::default()
        };
        assert_eq!(scratch_card(&dir, &route(None), &dir), None, "no card named");
        assert_eq!(scratch_card(&dir, &route(Some("missing.mcd")), &dir), None, "named, not in the repository");
        let copied = scratch_card(&dir, &route(Some("there.mcd")), &dir).unwrap();
        assert_eq!(std::fs::read(copied).unwrap(), b"card");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_store_key_follows_cooker_code_and_the_source_not_the_cook_outputs() {
        let dir = asset_key_tree("store-key");
        for line in COOK_INPUTS.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            let path = dir.join(line);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            if !path.exists() {
                std::fs::write(path, "cook input\n").unwrap();
            }
        }
        std::fs::write(dir.join(".hkpsx/regions-source-inputs.json"), "{\"inputs\": {}}").unwrap();
        let cold = cook_store_key(&dir).unwrap();
        let changed = |path: &str, bytes: &str| {
            let file = dir.join(path);
            let before = std::fs::read(&file).unwrap();
            std::fs::write(&file, bytes).unwrap();
            let key = cook_store_key(&dir).unwrap();
            std::fs::write(&file, before).unwrap();
            key
        };
        // A lane that has cooked nothing yet computes the same key as one that has.
        assert_eq!(changed(REGIONS_BASE, "{\"base\": 2}"), cold, "the region report is an output");
        assert_eq!(changed(".hkpsx/menu-provenance.json", "{\"m\": 1}"), cold, "the menu cook is an output");
        assert_eq!(changed(".hkpsx/regions-cook-cache.json", "{\"c\": 1}"), cold, "the region cache is an output");
        assert_ne!(changed("host/charms.py", "x = 2\n"), cold, "a Python cooker");
        assert_ne!(changed("host/hk-cook/src/lib.rs", "// other\n"), cold, "a Rust cooker");
        assert_ne!(changed(".hkpsx/regions-source-inputs.json", "{\"inputs\": {\"a\": \"b\"}}"), cold, "the retail source");
        assert_eq!(cook_store_key(&dir).unwrap(), cold);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_cook_vouches_for_the_report_as_it_left_it_not_the_live_file() {
        let dir = asset_key_tree("cook-outputs");
        std::fs::create_dir_all(dir.join("data/regions")).unwrap();
        std::fs::write(dir.join("data/room.hk"), "room").unwrap();
        std::fs::write(dir.join("data/params.rs"), "params").unwrap();
        std::fs::write(dir.join("data/regions/chunk_001.hk"), "chunk").unwrap();
        let recorded = serde_json::to_value(region_outputs(&dir).unwrap()).unwrap();
        assert!(recorded.get("data/regions.json").is_none());
        assert_eq!(stale_outputs(&dir, &recorded), Some(vec![]));
        // The asset stage rewrites the live report: the record still holds.
        std::fs::write(dir.join("data/regions.json"), "mutated").unwrap();
        assert_eq!(stale_outputs(&dir, &recorded), Some(vec![]));
        // A real change to a cooked output, or a vanished one, is caught and named.
        std::fs::write(dir.join("data/regions/chunk_001.hk"), "other").unwrap();
        std::fs::remove_file(dir.join("data/room.hk")).unwrap();
        assert_eq!(
            stale_outputs(&dir, &recorded),
            Some(vec!["data/regions/chunk_001.hk".to_string(), "data/room.hk".to_string()])
        );
        assert_eq!(stale_outputs(&dir, &serde_json::json!(null)), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
