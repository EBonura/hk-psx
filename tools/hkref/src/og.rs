//! Original side: the macOS Steam build (Unity 2020.2.2f1, Mono, x86_64 under
//! Rosetta) cloned copy-on-write, patched with the reference driver, and run in
//! batch mode with Metal (so cameras render) inside an audio-denying sandbox.

use crate::profile::Profile;
use crate::trace::{Event, Trace};
use crate::util::{self, append_pid};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub struct Env { pub work: PathBuf, pub dotnet: PathBuf, pub hkref_dir: PathBuf, pub steam: PathBuf }

/// The `tools/hkref` directory this binary was built from (mod sources, patcher).
pub fn hkref_dir() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")) }
/// The hk-psx checkout root.
pub fn repo_root() -> PathBuf { hkref_dir().join("../..") }

fn find_dotnet() -> PathBuf {
    // HKREF_DOTNET names the SDK root (the directory holding the `dotnet` binary).
    if let Ok(d) = std::env::var("HKREF_DOTNET") { return PathBuf::from(d); }
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let mut roots = vec![repo_root().join(".hkpsx/og-reference/deps/dotnet")];
    if let Ok(d) = std::env::var("DOTNET_ROOT") { roots.push(PathBuf::from(d)); }
    roots.extend(["/usr/local/share/dotnet", "/opt/homebrew/opt/dotnet/libexec", "/usr/local/opt/dotnet/libexec"].iter().map(PathBuf::from));
    roots.push(home.join(".dotnet"));
    roots.into_iter().find(|r| r.join("dotnet").exists()).unwrap_or_else(|| PathBuf::from("/usr/local/share/dotnet"))
}

impl Env {
    pub fn detect(work: &Path) -> Env {
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
        Env {
            work: work.to_path_buf(),
            dotnet: find_dotnet(),
            hkref_dir: hkref_dir(),
            steam: std::env::var("HKREF_GAME").map(PathBuf::from)
                .unwrap_or(home.join("Library/Application Support/Steam/steamapps/common/Hollow Knight/hollow_knight.app")),
        }
    }
    pub fn app(&self) -> PathBuf { self.work.join("og/game/hollow_knight.app") }
    pub fn managed(&self) -> PathBuf { self.app().join("Contents/Resources/Data/Managed") }
    fn dotnet_cmd(&self) -> Command {
        let mut c = Command::new(self.dotnet.join("dotnet"));
        c.env("DOTNET_NOLOGO", "1").env("DOTNET_CLI_TELEMETRY_OPTOUT", "1").env("DOTNET_SKIP_FIRST_TIME_EXPERIENCE", "1");
        c
    }
    fn csc(&self) -> Result<PathBuf, String> {
        let sdk = self.dotnet.join("sdk");
        let mut v: Vec<PathBuf> = fs::read_dir(&sdk).map_err(|e| format!("no .NET SDK under {} ({e}); set HKREF_DOTNET", self.dotnet.display()))?
            .filter_map(|e| e.ok().map(|e| e.path().join("Roslyn/bincore/csc.dll"))).filter(|p| p.exists()).collect();
        v.sort();
        v.pop().ok_or_else(|| format!("no Roslyn csc.dll under {}", sdk.display()))
    }
}

/// Clone the Steam app (APFS copy-on-write, no disk cost until a file is
/// rewritten), compile the driver against the game's own assemblies and patch
/// the clone's Assembly-CSharp.dll. The Steam copy is never written; the repo
/// ships no game files (the driver is compiled against, and the patcher reads,
/// the assemblies of the copy of the game on this machine).
pub fn build(env: &Env) -> Result<(), String> {
    if !env.steam.join("Contents/Resources/Data/Managed/Assembly-CSharp.dll").exists() {
        return Err(format!("no Hollow Knight at {} (set HKREF_GAME to the hollow_knight.app path)", env.steam.display()));
    }
    if !env.app().exists() {
        fs::create_dir_all(env.app().parent().unwrap()).map_err(|e| e.to_string())?;
        let st = Command::new("cp").arg("-cR").arg(&env.steam).arg(env.app()).status().map_err(|e| e.to_string())?;
        if !st.success() { return Err("clone failed".into()); }
    }
    // Marker the patcher requires: it refuses to touch anything outside an hkref clone.
    fs::write(env.app().join(".hkref-copy"), "private copy made by hkref; safe to patch\n").map_err(|e| e.to_string())?;
    // A real steam_appid.txt next to the binary, and SteamAppId in the environment of
    // every launch below: without them the game calls RestartAppIfNecessary and
    // launches the Steam client.
    let _ = fs::write(env.app().join("Contents/MacOS/steam_appid.txt"), "367520\n");
    let managed = env.managed();
    let steam_managed = env.steam.join("Contents/Resources/Data/Managed");
    let src = env.hkref_dir.join("mod");
    let bin = env.work.join("og/build"); fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
    let out = bin.join("HKReference.dll");
    let mut c = env.dotnet_cmd();
    c.arg(env.csc()?).args(["-nologo", "-noconfig", "-nostdlib+", "-target:library"]).arg(format!("-out:{}", out.display()));
    for n in ["mscorlib", "netstandard", "System", "System.Core", "Assembly-CSharp", "UnityEngine", "UnityEngine.CoreModule", "UnityEngine.Physics2DModule",
        "UnityEngine.AudioModule", "UnityEngine.AnimationModule", "UnityEngine.ParticleSystemModule", "UnityEngine.InputLegacyModule", "UnityEngine.UI",
        "PlayMaker", "UnityEngine.ImageConversionModule", "UnityEngine.ScreenCaptureModule"] {
        c.arg(format!("-r:{}", steam_managed.join(format!("{n}.dll")).display()));
    }
    let mut files: Vec<PathBuf> = fs::read_dir(&src).map_err(|e| e.to_string())?.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().map_or(false, |x| x == "cs")).collect();
    files.sort();
    c.args(&files);
    let o = c.output().map_err(|e| e.to_string())?;
    if !o.status.success() { return Err(format!("csc failed:\n{}", String::from_utf8_lossy(&o.stdout))); }
    // The patcher (Mono.Cecil from NuGet; the first build needs the package cache or network).
    let pdir = bin.join("patcher");
    let mut b = env.dotnet_cmd();
    b.args(["build", "-c", "Release", "-v", "q", "--nologo"]).arg(env.hkref_dir.join("patcher")).arg("-o").arg(&pdir);
    let o = b.output().map_err(|e| e.to_string())?;
    if !o.status.success() { return Err(format!("patcher build failed:\n{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))); }
    let saves = env.work.join("og/saves"); fs::create_dir_all(&saves).map_err(|e| e.to_string())?;
    let mut p = env.dotnet_cmd();
    p.arg(pdir.join("Patcher.dll")).arg(steam_managed.join("Assembly-CSharp.dll")).arg(&managed).arg(&out).arg(&saves);
    let o = p.output().map_err(|e| e.to_string())?;
    if !o.status.success() { return Err(format!("patcher failed:\n{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))); }
    eprintln!("[og] driver built and clone patched");
    Ok(())
}

pub struct Window { pub frames: usize, pub rows: Vec<(usize, u16)>, pub start: Option<(f64, f64, f64)>, pub shot_frames: Vec<usize> }

const SANDBOX: &str = "(version 1)(allow default)(deny mach-lookup (global-name-prefix \"com.apple.audio\"))(deny mach-lookup (global-name \"com.apple.coreaudio.audiohald\"))";

pub fn run(env: &Env, p: &Profile, w: &Window) -> Result<PathBuf, String> {
    let out = p.dir.join("og");
    let _ = fs::remove_dir_all(&out);
    fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    fs::write(out.join("input.csv"), crate::tape::rows_to_csv(&w.rows)).map_err(|e| e.to_string())?;
    let home = env.work.join("og/fakehome"); fs::create_dir_all(&home).map_err(|e| e.to_string())?;
    let exe = env.app().join("Contents/MacOS/Hollow Knight");
    let mut c = Command::new("sandbox-exec");
    c.args(["-p", SANDBOX]).arg(&exe).args(["-batchmode", "-logFile"]).arg(out.join("unity.log"))
        .current_dir(env.work.join("og"))
        .env("SteamAppId", "367520").env("SteamGameId", "367520")
        .env("HOME", &home).env("CFFIXED_USER_HOME", &home)
        .env("HK_REFERENCE_OUTPUT", &out).env("HK_REFERENCE_MAX_FRAMES", w.frames.to_string()).env("HK_REFERENCE_MAX_SECONDS", p.og_timeout.to_string());
    if !p.scene.is_empty() { c.env("HK_REFERENCE_SCENE", format!("{}:{}", p.scene, p.gate)); }
    if let Some((x, y, f)) = w.start {
        c.env("HK_REFERENCE_TELEPORT", format!("{x:.4},{y:.4}")).env("HK_REFERENCE_FACE", if f < 0.0 { "-1" } else { "1" });
    }
    c.env("HK_REFERENCE_SEED", p.seed.to_string());
    if !p.sweep.is_empty() { c.env("HK_REFERENCE_SWEEP", p.sweep.join(",")).env("HK_REFERENCE_SWEEP_FRAMES", p.sweep_frames.to_string()); }
    if !p.fx_off.is_empty() { c.env("HK_REFERENCE_DISABLE_FX", p.fx_off.join(",")); }
    if !p.player_data.is_empty() { c.env("HK_REFERENCE_PD", &p.player_data); }
    if !p.hide.is_empty() { c.env("HK_REFERENCE_HIDE", p.hide.join(",")); }
    if !w.shot_frames.is_empty() {
        c.env("HK_REFERENCE_SHOT_AT", w.shot_frames.iter().map(|f| f.to_string()).collect::<Vec<_>>().join(","));
        c.env("HK_REFERENCE_SHOT_SIZE", "960x540");
    }
    let o = fs::File::create(out.join("stdout.txt")).map_err(|e| e.to_string())?;
    let child = c.stdout(Stdio::from(o.try_clone().map_err(|e| e.to_string())?)).stderr(Stdio::from(o)).spawn().map_err(|e| format!("original: {e}"))?;
    append_pid(&env.work, &format!("og-{}", p.name), child.id());
    eprintln!("[og] original pid {} (muted: sandboxed audio, volume 0), {} frames", child.id(), w.frames);
    let ok = util::wait_with_timeout(child, p.og_timeout + 60)?;
    if !ok { return Err(format!("original exited non-zero or timed out; see {}", out.join("driver.log").display())); }
    Ok(out)
}

fn split_csv(line: &str) -> Vec<String> {
    let mut out = Vec::new(); let mut cur = String::new(); let mut q = false;
    for ch in line.chars() {
        match ch { '"' => q = !q, ',' if !q => { out.push(std::mem::take(&mut cur)); } _ => cur.push(ch) }
    }
    out.push(cur); out
}

pub fn table(path: &Path) -> Vec<BTreeMap<String, String>> {
    let Ok(t) = fs::read_to_string(path) else { return vec![] };
    let mut l = t.lines();
    let Some(h) = l.next() else { return vec![] };
    let head = split_csv(h);
    l.filter_map(|x| { let c = split_csv(x); if c.len() < head.len() { return None; } Some(head.iter().cloned().zip(c).collect()) }).collect()
}

fn num(r: &BTreeMap<String, String>, k: &str) -> f64 { r.get(k).and_then(|v| v.parse().ok()).unwrap_or(f64::NAN) }
fn flag(r: &BTreeMap<String, String>, k: &str) -> bool { r.get(k).map_or(false, |v| v == "True") }

pub fn normalise(p: &Profile, t0: i64, anchors: &BTreeMap<String, (f64, f64)>) -> Result<(Trace, Vec<Event>), String> {
    let dir = p.dir.join("og");
    let mut last: BTreeMap<i64, BTreeMap<String, String>> = BTreeMap::new();
    for r in table(&dir.join("state.csv")) {
        let f: i64 = r.get("test_frame").and_then(|v| v.parse().ok()).unwrap_or(-1);
        if f >= 0 { last.insert(f, r); }
    }
    if last.is_empty() { return Err("original produced no gameplay frames (see driver.log)".into()); }
    let cam: BTreeMap<i64, BTreeMap<String, String>> = table(&dir.join("camera.csv")).into_iter()
        .filter_map(|r| { let f: i64 = r.get("test_frame")?.parse().ok()?; if f >= 0 { Some((f, r)) } else { None } }).collect();
    let actors = table(&dir.join("actors.csv"));
    let mut by_frame: BTreeMap<i64, Vec<&BTreeMap<String, String>>> = BTreeMap::new();
    for a in &actors { if let Some(f) = a.get("test_frame").and_then(|v| v.parse::<i64>().ok()) { by_frame.entry(f).or_default().push(a); } }
    // pick one instance per enemy pattern: the matching actor nearest the port's
    // actor at the window start (so both sides follow the same individual), else
    // the first that ever appears
    let mut chosen: BTreeMap<String, String> = BTreeMap::new();
    for e in &p.enemies {
        let anchor = anchors.get(&e.chan);
        let mut best: Option<(f64, String)> = None;
        for a in &actors {
            if !a.get("name").map_or(false, |n| n.contains(&e.pattern)) { continue; }
            if num(a, "test_frame") > 3.0 { continue; }
            let d = anchor.map_or(0.0, |(x, y)| ((num(a, "x") - x).powi(2) + (num(a, "y") - y).powi(2)).sqrt());
            if best.as_ref().map_or(true, |(bd, _)| d < *bd) { best = Some((d, a["id"].clone())); }
        }
        if let Some((_, id)) = best { chosen.insert(e.chan.clone(), id); }
    }
    let mut tr = Trace::default(); let mut ev = Vec::new();
    let (mut pa, mut pj, mut php) = (false, false, f64::NAN);
    let mut pe: BTreeMap<String, (f64, String)> = BTreeMap::new();
    for (f, r) in &last {
        let t = t0 + f;
        tr.push_tick(t);
        tr.set("hero.x", num(r, "x")); tr.set("hero.y", num(r, "y"));
        tr.set("hero.vx", num(r, "vx")); tr.set("hero.vy", num(r, "vy"));
        tr.set("hero.face", if flag(r, "facing_right") { 1.0 } else { -1.0 });
        tr.set("hero.hp", num(r, "health")); tr.set("hero.soul", num(r, "soul"));
        tr.set("input.pad", num(r, "buttons"));
        for k in ["hero_state", "clip"] { tr.set_text(&format!("s.hero.{}", k.trim_start_matches("hero_")), r.get(k).map(String::as_str).unwrap_or("")); }
        tr.set("hero.clip_frame", num(r, "clip_frame"));
        tr.set("og.scene_ok", if r.get("scene").map_or(false, |s| *s == p.scene || p.scene.is_empty()) { 1.0 } else { 0.0 });
        if let Some(c) = cam.get(f) { tr.set("cam.x", num(c, "camera_x")); tr.set("cam.y", num(c, "camera_y")); }
        let (a, j, hp) = (flag(r, "attacking"), flag(r, "jumping"), num(r, "health"));
        if a && !pa { ev.push(Event { tick: t, kind: "hero.attack".into(), name: String::new(), value: 1.0 }); }
        if j && !pj { ev.push(Event { tick: t, kind: "hero.jump".into(), name: String::new(), value: 1.0 }); }
        if hp < php { ev.push(Event { tick: t, kind: "hero.hurt".into(), name: String::new(), value: php - hp }); }
        pa = a; pj = j; php = hp;
        if let Some(list) = by_frame.get(f) {
            for (chan, id) in &chosen {
                if let Some(a) = list.iter().find(|a| a["id"] == *id) {
                    let hp = num(a, "hp");
                    tr.set(&format!("{chan}.x"), num(a, "x")); tr.set(&format!("{chan}.y"), num(a, "y")); tr.set(&format!("{chan}.hp"), hp);
                    let fsm = a.get("fsms").cloned().unwrap_or_default();
                    tr.set_text(&format!("s.{chan}.fsm"), &fsm);
                    if let Some((php, pf)) = pe.get(chan) {
                        if hp < *php { ev.push(Event { tick: t, kind: format!("{chan}.hit"), name: String::new(), value: php - hp }); ev.push(Event { tick: t, kind: "enemy.hit".into(), name: chan.clone(), value: php - hp }); }
                        if *pf != fsm {
                            ev.push(Event { tick: t, kind: format!("{chan}.fsm"), name: fsm.clone(), value: 0.0 });
                            if let Some(m) = p.enemies.iter().find(|m| m.chan == *chan) {
                                for (sub, kind) in &m.events { if fsm.contains(sub.as_str()) && !pf.contains(sub.as_str()) { ev.push(Event { tick: t, kind: kind.clone(), name: sub.clone(), value: 1.0 }); } }
                            }
                        }
                    }
                    pe.insert(chan.clone(), (hp, fsm));
                }
            }
        }
    }
    for r in table(&dir.join("audio-calls.csv")) {
        let q: i64 = r.get("queued_test_frame").and_then(|v| v.parse().ok()).unwrap_or(-1);
        let op = r.get("operation").map(String::as_str).unwrap_or("");
        if q >= 0 && (op == "Play" || op == "PlayOneShot") {
            let clip = r.get("clip_or_snapshot").cloned().unwrap_or_default();
            ev.push(Event { tick: t0 + q, kind: "sfx".into(), name: if clip.is_empty() { r.get("hierarchy").cloned().unwrap_or_default() } else { clip }, value: num(&r, "volume_scale") });
        }
    }
    ev.sort_by_key(|e| e.tick);
    Ok((tr, ev))
}
