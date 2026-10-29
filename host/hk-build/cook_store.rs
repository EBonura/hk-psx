//! A shared, content-addressed store of the cooked state (`data/` and the cook caches under
//! `.hkpsx/`), so that a lane whose cooker inputs match a stored snapshot gets the cache hits of
//! a finished build without cooking, and so that a lane that did recook gives its identical
//! bytes back to the blocks the store already holds.
//!
//! Layout under `$HK_COOK_STORE` (outside every lane; the janitor never walks `_shared`):
//!
//! ```text
//! objects/<aa>/<sha256>   one read-only file per distinct content
//! keys/<key>.json         manifest of one snapshot: path -> sha256, size, exec bit
//! ```
//!
//! Files go in and out by APFS clone (`clonefile`), so a restore or a publish writes no data
//! blocks: a lane pays for a file only once it changes it. A store that is not on the lane's
//! volume falls back to copies (correct, just not free).
//!
//! The key names which snapshot to try, never whether a snapshot is right: every cache the
//! build consults (regions, menu, assets) re-hashes the outputs it vouches for, so a wrong or
//! partial snapshot is a cache miss and a normal cook, not a wrong disc.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::CString;
use std::fs::File;
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// The environment variable naming the store directory; unset means the store is off.
pub const STORE_ENV: &str = "HK_COOK_STORE";

/// Records which key the lane's cook state was last synchronised with.
const MARKER: &str = ".hkpsx/cook-store.json";

/// Files that must exist for a lane to count as holding a complete cook state.
const CANARIES: &[&str] = &[
    "data/room.hk",
    "data/regions.json",
    ".hkpsx/regions-cook-cache.json",
    ".hkpsx/assets-cache.json",
    ".hkpsx/menu-cache.json",
];

/// Paths under `.hkpsx/` that are scratch, per-lane identity or written after the cook caches
/// are stamped, so they never go in a snapshot (and a restore leaves them alone).
const LANE_STATE: &[&str] = &[
    ".hkpsx/validate",
    ".hkpsx/pgo",
    ".hkpsx/emulator",
    ".hkpsx/hud-tests",
    ".hkpsx/world-tests",
    ".hkpsx/native-route-probe",
    ".hkpsx/geo-runtime-tests",
    ".hkpsx/focus-audio-runtime-tests",
    ".hkpsx/boss-sim",
    ".hkpsx/doctor.json",
    ".hkpsx/sdk.json",
    // Stamps the lane's own `.venv` / `.psoxide`: copied to a lane without them they make the
    // driver skip the install that creates them.
    ".hkpsx/python-lock.sha256",
    ".hkpsx/build.json",
    ".hkpsx/build-normal.json",
    ".hkpsx/build-pgo.json",
    MARKER,
];

/// Generated after the asset cache is stamped; the asset cache does not vouch for them.
const WRITTEN_AFTER_CACHES: &[&str] = &["boot-art.hk", "boot_art.rs", "intro.exe"];

/// An object younger than this is never collected (a publish writes objects before its manifest).
const GC_GRACE_SECONDS: u64 = 3600;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A store directory.
pub struct Store {
    dir: PathBuf,
}

/// What a restore did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RestoreReport {
    pub files: usize,
    pub replaced: usize,
    pub pruned: usize,
    pub bytes: u64,
}

/// What a publish or dedupe did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct PublishReport {
    pub files: usize,
    pub new_objects: usize,
    pub new_bytes: u64,
    /// Files whose bytes the store already held and that were turned back into clones of it.
    pub relinked: usize,
    pub relinked_bytes: u64,
}

impl Store {
    /// The store named by `HK_COOK_STORE`, or `None` when it is unset or empty.
    pub fn from_env() -> Option<Store> {
        let dir = std::env::var_os(STORE_ENV).filter(|v| !v.is_empty())?;
        Some(Store {
            dir: PathBuf::from(dir),
        })
    }

    pub fn at(dir: &Path) -> Store {
        Store {
            dir: dir.to_path_buf(),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn object(&self, sha: &str) -> PathBuf {
        self.dir.join("objects").join(&sha[..2]).join(sha)
    }

    fn manifest(&self, key: &str) -> PathBuf {
        self.dir.join("keys").join(format!("{key}.json"))
    }

    pub fn has_key(&self, key: &str) -> bool {
        self.manifest(key).is_file()
    }

    fn read_manifest(&self, key: &str) -> Result<Value> {
        let path = self.manifest(key);
        let value: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
        if value["version"] != 1 || value["key"] != key || !value["files"].is_object() {
            return Err(format!("{} is not a version 1 manifest for {key}", path.display()).into());
        }
        Ok(value)
    }

    /// Every snapshot, newest first: (key, manifest).
    fn manifests(&self) -> Result<Vec<(String, Value)>> {
        let mut found = Vec::new();
        let Ok(entries) = std::fs::read_dir(self.dir.join("keys")) else {
            return Ok(found);
        };
        for entry in entries {
            let path = entry?.path();
            let Some(key) = path.file_stem().and_then(|s| s.to_str()).map(str::to_owned) else {
                continue;
            };
            if path.extension().is_some_and(|e| e == "json") {
                found.push((key.clone(), self.read_manifest(&key)?));
            }
        }
        found.sort_by_key(|(_, m)| std::cmp::Reverse(m["created"].as_u64().unwrap_or(0)));
        Ok(found)
    }
}

/// True when `rel` (repo-relative, forward slashes) belongs in a snapshot.
fn in_scope(rel: &str) -> bool {
    if !(rel.starts_with("data/") || rel.starts_with(".hkpsx/")) {
        return false;
    }
    if LANE_STATE
        .iter()
        .any(|skip| rel == *skip || rel.strip_prefix(skip).is_some_and(|r| r.starts_with('/')))
    {
        return false;
    }
    let name = rel.rsplit('/').next().unwrap_or(rel);
    !(name.ends_with(".tmp") || WRITTEN_AFTER_CACHES.contains(&name))
}

/// Every file of a snapshot's scope present under `root`, repo-relative and sorted.
/// Symlinks are skipped: cook output is plain files.
pub fn scope_files(root: &Path) -> Result<Vec<String>> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<()> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Ok(());
        };
        for entry in entries {
            let entry = entry?;
            let kind = entry.file_type()?;
            let rel = entry
                .path()
                .strip_prefix(root)?
                .to_string_lossy()
                .replace('\\', "/");
            if kind.is_dir() {
                let probe = format!("{rel}/");
                // Prune whole scratch directories before descending into them.
                if LANE_STATE.iter().any(|skip| probe == format!("{skip}/")) {
                    continue;
                }
                walk(root, &entry.path(), out)?;
            } else if kind.is_file() && in_scope(&rel) {
                out.push(rel);
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    walk(root, &root.join("data"), &mut files)?;
    walk(root, &root.join(".hkpsx"), &mut files)?;
    files.sort();
    Ok(files)
}

fn sha256_path(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(0))
}

/// A sibling name no other process or call of this one uses.
fn temp_sibling(path: &Path) -> PathBuf {
    let n = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(format!(".cs-{}-{n}", std::process::id()));
    path.with_file_name(name)
}

/// Copy `src` to the new path `dst` without writing data blocks where the volume allows it.
fn clone_file(src: &Path, dst: &Path) -> Result<()> {
    let from = CString::new(src.as_os_str().as_bytes())?;
    let to = CString::new(dst.as_os_str().as_bytes())?;
    // SAFETY: both pointers are NUL-terminated strings that outlive the call.
    let status = unsafe { libc::clonefile(from.as_ptr(), to.as_ptr(), 0) };
    if status == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    // Another volume or a filesystem without clones: an ordinary copy is still correct.
    if matches!(
        error.raw_os_error(),
        Some(libc::EXDEV) | Some(libc::ENOTSUP)
    ) {
        std::fs::copy(src, dst)?;
        return Ok(());
    }
    Err(format!("clonefile {} -> {}: {error}", src.display(), dst.display()).into())
}

/// Atomically make `dst` a clone of `src`, writable by the lane (or read-only for the store).
fn place_clone(src: &Path, dst: &Path, mode: u32) -> Result<()> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = temp_sibling(dst);
    clone_file(src, &temp)?;
    let placed = std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(mode))
        .and_then(|()| std::fs::rename(&temp, dst));
    if placed.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    Ok(placed?)
}

fn lane_mode(executable: bool) -> u32 {
    if executable {
        0o755
    } else {
        0o644
    }
}

/// The (device offset, length) extents of a file, or `None` where the filesystem will not say
/// (very small files stored inline, or a filesystem that is not APFS).
pub fn extents(path: &Path) -> Option<Vec<(i64, i64)>> {
    use std::os::unix::io::AsRawFd;
    let file = File::open(path).ok()?;
    let size = i64::try_from(file.metadata().ok()?.len()).ok()?;
    let mut found = Vec::new();
    let mut offset = 0i64;
    while offset < size {
        let mut map = libc::log2phys {
            l2p_flags: 0,
            l2p_contigbytes: size - offset,
            l2p_devoffset: offset,
        };
        // SAFETY: `map` is a live log2phys the kernel fills in; the descriptor is open.
        let status = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_LOG2PHYS_EXT, &mut map) };
        if status == -1 || map.l2p_contigbytes <= 0 {
            return None;
        }
        found.push((map.l2p_devoffset, map.l2p_contigbytes));
        offset += map.l2p_contigbytes;
    }
    Some(found)
}

/// True when both files occupy exactly the same blocks (clones of each other, untouched since).
fn same_blocks(a: &Path, b: &Path) -> bool {
    matches!((extents(a), extents(b)), (Some(x), Some(y)) if !x.is_empty() && x == y)
}

/// True when `path` already holds `sha`'s bytes: it shares the object's blocks or, where the
/// filesystem cannot map extents (tiny files), has the same hash.
fn up_to_date(path: &Path, object: &Path, sha: &str) -> bool {
    if same_blocks(path, object) {
        return true;
    }
    extents(path).is_none() && sha256_path(path).is_ok_and(|actual| actual == sha)
}

/// Whether the lane holds a complete cook state that was last synchronised with `key`.
pub fn lane_in_sync(root: &Path, key: &str) -> bool {
    let marker = std::fs::read(root.join(MARKER))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    marker.is_some_and(|m| m["key"] == key) && CANARIES.iter().all(|c| root.join(c).is_file())
}

fn write_marker(root: &Path, key: &str, how: &str) -> Result<()> {
    let path = root.join(MARKER);
    let temp = temp_sibling(&path);
    std::fs::write(
        &temp,
        serde_json::to_vec_pretty(&json!({"key": key, "how": how, "at": now_millis()}))?,
    )?;
    Ok(std::fs::rename(temp, path)?)
}

/// Record that the lane's cook state is the snapshot `key` (after a build that hit every cache).
pub fn mark_synchronised(root: &Path, key: &str) -> Result<()> {
    write_marker(root, key, "build")
}

/// Make the lane's cook state exactly the snapshot `key`: every manifest file becomes a clone of
/// its object, and files in scope that the snapshot does not hold are removed.
pub fn restore(root: &Path, store: &Store, key: &str) -> Result<RestoreReport> {
    let manifest = store.read_manifest(key)?;
    let files = manifest["files"]
        .as_object()
        .ok_or("manifest has no files")?;
    // Prove the whole snapshot is present before the lane's state is touched.
    for (rel, meta) in files {
        let sha = meta["sha256"]
            .as_str()
            .ok_or("manifest entry without sha256")?;
        let object = store.object(sha);
        let size = std::fs::metadata(&object)
            .map_err(|e| {
                format!(
                    "store object for {rel} is missing ({}): {e}",
                    object.display()
                )
            })?
            .len();
        if Some(size) != meta["size"].as_u64() {
            return Err(format!(
                "store object for {rel} has {size} bytes, manifest says {}",
                meta["size"]
            )
            .into());
        }
    }
    let mut report = RestoreReport::default();
    for (rel, meta) in files {
        let sha = meta["sha256"]
            .as_str()
            .ok_or("manifest entry without sha256")?;
        let object = store.object(sha);
        let target = root.join(rel);
        report.files += 1;
        report.bytes += meta["size"].as_u64().unwrap_or(0);
        if target.is_file() && up_to_date(&target, &object, sha) {
            continue;
        }
        place_clone(&object, &target, lane_mode(meta["exec"] == true))?;
        report.replaced += 1;
    }
    for rel in scope_files(root)? {
        if !files.contains_key(&rel) {
            std::fs::remove_file(root.join(&rel))?;
            report.pruned += 1;
        }
    }
    write_marker(root, key, "restore")?;
    Ok(report)
}

/// Hash every file in scope: path -> (sha256, size, executable).
fn snapshot_of(root: &Path) -> Result<BTreeMap<String, (String, u64, bool)>> {
    let mut found = BTreeMap::new();
    for rel in scope_files(root)? {
        let path = root.join(&rel);
        let meta = std::fs::metadata(&path)?;
        found.insert(
            rel,
            (
                sha256_path(&path)?,
                meta.len(),
                meta.permissions().mode() & 0o111 != 0,
            ),
        );
    }
    Ok(found)
}

/// Give the lane's identical bytes back to the store's blocks. With `ingest` also add the
/// objects the store lacks (as clones of the lane's files). Returns the hashed snapshot.
fn sync_objects(
    root: &Path,
    store: &Store,
    ingest: bool,
) -> Result<(BTreeMap<String, (String, u64, bool)>, PublishReport)> {
    let snapshot = snapshot_of(root)?;
    let mut report = PublishReport::default();
    for (rel, (sha, size, executable)) in &snapshot {
        let path = root.join(rel);
        let object = store.object(sha);
        report.files += 1;
        if object.is_file() {
            // Only files that own blocks can be given back; empty and unmappable ones cost nothing.
            let owns_blocks = extents(&path).is_some_and(|found| !found.is_empty());
            if owns_blocks && !same_blocks(&path, &object) {
                place_clone(&object, &path, lane_mode(*executable))?;
                report.relinked += 1;
                report.relinked_bytes += size;
            }
        } else if ingest {
            place_clone(&path, &object, 0o444)?;
            report.new_objects += 1;
            report.new_bytes += size;
        }
    }
    Ok((snapshot, report))
}

/// Add the lane's cook state to the store as snapshot `key` (the first publisher of a key wins),
/// then turn the lane's files that duplicate older objects back into clones of them.
pub fn publish(root: &Path, store: &Store, key: &str, label: &str) -> Result<PublishReport> {
    std::fs::create_dir_all(store.dir.join("keys"))?;
    let (snapshot, report) = sync_objects(root, store, true)?;
    let files: serde_json::Map<String, Value> = snapshot
        .iter()
        .map(|(rel, (sha, size, exec))| {
            (
                rel.clone(),
                json!({"sha256": sha, "size": size, "exec": exec}),
            )
        })
        .collect();
    let total: u64 = snapshot.values().map(|(_, size, _)| size).sum();
    let manifest = json!({
        "version": 1, "key": key, "label": label, "created": now_millis(),
        "total_bytes": total, "files": files,
    });
    let path = store.manifest(key);
    if path.exists() {
        return Err(format!("snapshot {key} already exists").into());
    }
    let temp = temp_sibling(&path);
    std::fs::write(&temp, serde_json::to_vec(&manifest)?)?;
    std::fs::rename(&temp, &path)?;
    write_marker(root, key, "publish")?;
    Ok(report)
}

/// Turn the lane's files that duplicate objects the store holds back into clones of them.
pub fn dedupe(root: &Path, store: &Store) -> Result<PublishReport> {
    Ok(sync_objects(root, store, false)?.1)
}

/// What a collection did or would do.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct GcReport {
    pub dropped_keys: Vec<String>,
    pub removed_objects: usize,
    pub removed_bytes: u64,
}

/// Keep the newest `keep` snapshots; drop the other manifests and every object no kept
/// manifest names (objects younger than an hour stay: a publish writes them first).
/// Without `apply` nothing is deleted.
pub fn gc(store: &Store, keep: usize, apply: bool) -> Result<GcReport> {
    let manifests = store.manifests()?;
    let mut report = GcReport::default();
    let mut wanted: BTreeSet<String> = BTreeSet::new();
    for (index, (key, manifest)) in manifests.iter().enumerate() {
        if index < keep {
            for meta in manifest["files"]
                .as_object()
                .into_iter()
                .flat_map(|f| f.values())
            {
                if let Some(sha) = meta["sha256"].as_str() {
                    wanted.insert(sha.to_owned());
                }
            }
        } else {
            report.dropped_keys.push(key.clone());
            if apply {
                std::fs::remove_file(store.manifest(key))?;
            }
        }
    }
    let Ok(buckets) = std::fs::read_dir(store.dir.join("objects")) else {
        return Ok(report);
    };
    for bucket in buckets {
        for entry in std::fs::read_dir(bucket?.path())? {
            let path = entry?.path();
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_owned();
            let meta = std::fs::metadata(&path)?;
            let age = meta
                .modified()
                .ok()
                .and_then(|m| m.elapsed().ok())
                .map_or(0, |d| d.as_secs());
            if !wanted.contains(&name) && age >= GC_GRACE_SECONDS {
                report.removed_objects += 1;
                report.removed_bytes += meta.len();
                if apply {
                    std::fs::remove_file(&path)?;
                }
            }
        }
    }
    Ok(report)
}

/// Re-hash every object; the names of the ones whose bytes no longer match their name.
pub fn verify(store: &Store) -> Result<Vec<String>> {
    let mut bad = Vec::new();
    let Ok(buckets) = std::fs::read_dir(store.dir.join("objects")) else {
        return Ok(bad);
    };
    for bucket in buckets {
        for entry in std::fs::read_dir(bucket?.path())? {
            let path = entry?.path();
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_owned();
            if sha256_path(&path)? != name {
                bad.push(name);
            }
        }
    }
    Ok(bad)
}

/// Print the snapshots and the store's size.
pub fn status(store: &Store, root: &Path) -> Result<()> {
    let manifests = store.manifests()?;
    println!("store {}", store.dir.display());
    for (key, m) in &manifests {
        println!(
            "  {}  {} files, {:.2} GB, {}  ({})",
            &key[..key.len().min(16)],
            m["files"].as_object().map_or(0, |f| f.len()),
            m["total_bytes"].as_u64().unwrap_or(0) as f64 / 1e9,
            m["label"].as_str().unwrap_or(""),
            m["created"].as_u64().unwrap_or(0) / 1000,
        );
    }
    let marker = std::fs::read(root.join(MARKER))
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
    match marker {
        Some(m) => println!(
            "this lane: synchronised with {} ({})",
            m["key"].as_str().unwrap_or("?"),
            m["how"].as_str().unwrap_or("?")
        ),
        None => println!("this lane: not synchronised with any snapshot"),
    }
    Ok(())
}

/// Physical space: for each directory, its logical bytes, the bytes its distinct physical
/// extents occupy, and how much of those extents no other listed directory also holds.
pub fn physical_report(dirs: &[PathBuf]) -> Result<()> {
    fn extents_of(dir: &Path) -> (u64, BTreeMap<i64, i64>) {
        let mut logical = 0u64;
        let mut distinct: BTreeMap<i64, i64> = BTreeMap::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(path) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&path) else {
                continue;
            };
            for entry in entries.flatten() {
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                if kind.is_dir() {
                    stack.push(entry.path());
                } else if kind.is_file() {
                    let size = entry.metadata().map_or(0, |m| m.len());
                    logical += size;
                    match extents(&entry.path()) {
                        Some(found) => {
                            for (offset, length) in found {
                                let slot = distinct.entry(offset).or_insert(0);
                                *slot = (*slot).max(length);
                            }
                        }
                        // Inline or unmappable: charge its size to this directory alone.
                        None => {
                            let n = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
                            distinct.insert(
                                -1 - i64::try_from(n).unwrap_or(0),
                                i64::try_from(size).unwrap_or(0),
                            );
                        }
                    }
                }
            }
        }
        (logical, distinct)
    }
    let per_dir: Vec<(u64, BTreeMap<i64, i64>)> = dirs.iter().map(|d| extents_of(d)).collect();
    for (i, dir) in dirs.iter().enumerate() {
        let (logical, mine) = &per_dir[i];
        let physical: i64 = mine.values().sum();
        let private: i64 = mine
            .iter()
            .filter(|(offset, _)| {
                **offset < 0
                    || per_dir
                        .iter()
                        .enumerate()
                        .all(|(j, (_, other))| j == i || !other.contains_key(offset))
            })
            .map(|(_, length)| *length)
            .sum();
        println!(
            "{}: logical {:.2} GB, physical {:.2} GB, not shared with the other listed directories {:.2} GB",
            dir.display(),
            *logical as f64 / 1e9,
            physical as f64 / 1e9,
            private as f64 / 1e9
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hk-cook-store-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn write(root: &Path, rel: &str, bytes: &[u8]) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, bytes).expect("write");
    }

    /// A lane with a few cooked files, big enough to own blocks, and the canaries.
    fn lane(name: &str) -> PathBuf {
        let root = scratch(name);
        for canary in CANARIES {
            write(&root, canary, format!("canary {canary}").as_bytes());
        }
        write(&root, "data/regions/chunk_001.hk", &vec![7u8; 300_000]);
        write(&root, "data/sub/deep.rs", &vec![9u8; 100_000]);
        write(&root, ".hkpsx/music/a.bin", &vec![3u8; 200_000]);
        write(&root, ".hkpsx/validate/run/dump.bin", &vec![1u8; 100_000]);
        write(&root, ".hkpsx/doctor.json", b"{}");
        write(&root, ".hkpsx/python-lock.sha256", b"abc");
        write(&root, "data/boot-art.hk", b"late");
        root
    }

    #[test]
    fn scope_keeps_cook_state_and_drops_lane_state() {
        let root = lane("scope");
        let files = scope_files(&root).expect("scope");
        assert!(files.contains(&"data/regions/chunk_001.hk".to_string()));
        assert!(files.contains(&".hkpsx/music/a.bin".to_string()));
        assert!(files.contains(&".hkpsx/assets-cache.json".to_string()));
        for skipped in [
            ".hkpsx/validate/run/dump.bin",
            ".hkpsx/doctor.json",
            ".hkpsx/python-lock.sha256",
            "data/boot-art.hk",
        ] {
            assert!(
                !files.contains(&skipped.to_string()),
                "{skipped} must stay out of snapshots"
            );
        }
        assert!(
            !in_scope("host/charms.py") && !in_scope(".hkpsx/validate") && !in_scope("data/x.tmp")
        );
        assert!(
            in_scope(".hkpsx/validated-things.json"),
            "a prefix of a name is not the directory"
        );
        std::fs::remove_dir_all(&root).expect("clean");
    }

    #[test]
    fn publish_then_restore_makes_another_lane_identical_and_leaves_its_scratch() {
        let source = lane("pub-src");
        let store_dir = scratch("pub-store");
        let store = Store::at(&store_dir);
        let published = publish(&source, &store, "k1", "main").expect("publish");
        assert!(published.new_objects > 0 && store.has_key("k1"));
        assert!(lane_in_sync(&source, "k1"));

        let other = lane("pub-dst");
        write(&other, "data/regions/chunk_001.hk", b"stale bytes");
        write(&other, "data/leftover.rs", b"not in the snapshot");
        write(&other, ".hkpsx/validate/mine.bin", b"keep me");
        assert!(!lane_in_sync(&other, "k1"));
        let report = restore(&other, &store, "k1").expect("restore");
        assert!(report.replaced > 0 && report.pruned == 1, "{report:?}");
        assert_eq!(
            std::fs::read(other.join("data/regions/chunk_001.hk")).unwrap(),
            vec![7u8; 300_000]
        );
        assert!(!other.join("data/leftover.rs").exists());
        assert!(
            other.join(".hkpsx/validate/mine.bin").exists(),
            "scratch is not the store's to remove"
        );
        assert!(lane_in_sync(&other, "k1") && !lane_in_sync(&other, "k2"));
        // A writable lane file: changing it must not reach the store.
        write(&other, "data/sub/deep.rs", b"changed in the lane");
        let again = restore(&other, &store, "k1").expect("second restore");
        assert_eq!(
            again.replaced, 1,
            "only the changed file is replaced: {again:?}"
        );
        assert_eq!(
            std::fs::read(other.join("data/sub/deep.rs")).unwrap(),
            vec![9u8; 100_000]
        );
        assert!(verify(&store).expect("verify").is_empty());
        for dir in [&source, &other, &store_dir] {
            let _ = std::process::Command::new("chmod")
                .arg("-R")
                .arg("u+w")
                .arg(dir)
                .status();
            std::fs::remove_dir_all(dir).expect("clean");
        }
    }

    #[test]
    fn restore_checks_the_whole_snapshot_before_touching_the_lane() {
        let source = lane("miss-src");
        let store_dir = scratch("miss-store");
        let store = Store::at(&store_dir);
        publish(&source, &store, "k1", "main").expect("publish");
        let manifest = store.read_manifest("k1").expect("manifest");
        let sha = manifest["files"]["data/sub/deep.rs"]["sha256"]
            .as_str()
            .expect("sha")
            .to_string();
        let object = store.object(&sha);
        std::fs::set_permissions(&object, std::fs::Permissions::from_mode(0o644)).expect("chmod");
        std::fs::remove_file(&object).expect("remove object");

        let other = lane("miss-dst");
        write(&other, "data/regions/chunk_001.hk", b"mine");
        assert!(restore(&other, &store, "k1").is_err());
        assert_eq!(
            std::fs::read(other.join("data/regions/chunk_001.hk")).unwrap(),
            b"mine"
        );
        assert!(!lane_in_sync(&other, "k1"));
        for dir in [&source, &other, &store_dir] {
            let _ = std::process::Command::new("chmod")
                .arg("-R")
                .arg("u+w")
                .arg(dir)
                .status();
            std::fs::remove_dir_all(dir).expect("clean");
        }
    }

    #[test]
    fn dedupe_gives_rewritten_identical_bytes_back_to_the_store() {
        let root = lane("dedupe");
        let store_dir = scratch("dedupe-store");
        let store = Store::at(&store_dir);
        publish(&root, &store, "k1", "main").expect("publish");
        let file = root.join("data/regions/chunk_001.hk");
        let object = store.object(&sha256_path(&file).expect("sha"));
        assert!(
            same_blocks(&file, &object),
            "a published file shares its object's blocks"
        );
        // A cooker rewrites the same bytes: a new file with its own blocks.
        std::fs::remove_file(&file).expect("rm");
        std::fs::write(&file, vec![7u8; 300_000]).expect("rewrite");
        assert!(!same_blocks(&file, &object));
        let report = dedupe(&root, &store).expect("dedupe");
        assert_eq!(report.relinked, 1, "{report:?}");
        assert!(same_blocks(&file, &object));
        assert_eq!(std::fs::read(&file).unwrap(), vec![7u8; 300_000]);
        for dir in [&root, &store_dir] {
            let _ = std::process::Command::new("chmod")
                .arg("-R")
                .arg("u+w")
                .arg(dir)
                .status();
            std::fs::remove_dir_all(dir).expect("clean");
        }
    }

    #[test]
    fn a_second_snapshot_stores_only_what_changed_and_gc_follows_the_kept_ones() {
        let root = lane("gc");
        let store_dir = scratch("gc-store");
        let store = Store::at(&store_dir);
        publish(&root, &store, "k1", "one").expect("publish 1");
        write(&root, "data/sub/deep.rs", &vec![5u8; 100_000]);
        let second = publish(&root, &store, "k2", "two").expect("publish 2");
        assert_eq!(
            second.new_objects, 1,
            "only the changed file is new: {second:?}"
        );
        assert!(
            publish(&root, &store, "k2", "again").is_err(),
            "the first publisher of a key wins"
        );

        // Objects are fresh, so nothing is collected inside the grace period.
        let fresh = gc(&store, 1, true).expect("gc");
        assert_eq!(fresh.dropped_keys, vec!["k1".to_string()]);
        assert_eq!(fresh.removed_objects, 0);
        assert!(!store.has_key("k1") && store.has_key("k2"));
        for dir in [&root, &store_dir] {
            let _ = std::process::Command::new("chmod")
                .arg("-R")
                .arg("u+w")
                .arg(dir)
                .status();
            std::fs::remove_dir_all(dir).expect("clean");
        }
    }
}
