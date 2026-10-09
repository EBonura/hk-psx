//! Read-only access to the Unity data of the Windows Hollow Knight install.
//!
//! The Rust replacement for host/source.py (UnityPy 1.25.3 on Unity
//! 6000.0.61f1). Files are memory-mapped and parsed once per `Source`, which
//! is shared across threads; object values are read on demand.

pub mod activation;
pub mod fresh_save;
pub mod generator;
pub mod playmaker;
pub mod scene;
pub mod serialized;
pub mod texture;
pub mod typetree;
pub mod value;

use serialized::{ObjectInfo, SerializedFile};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use typetree::{Flavor, Node, Reader};
pub use value::Value;

pub const UNITY_VERSION: &str = "6000.0.61f1";

#[derive(Debug)]
pub enum Error {
    Io(String, std::io::Error),
    Eof,
    Format(String),
    Missing(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(path, e) => write!(f, "{path}: {e}"),
            Error::Eof => write!(f, "read past the end of the data"),
            Error::Format(s) | Error::Missing(s) => f.write_str(s),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// A MonoScript's (assembly, namespace, class).
pub type Script = (String, String, String);

/// One object of one loaded file.
#[derive(Clone)]
pub struct Obj {
    pub file: Arc<SerializedFile>,
    pub info: ObjectInfo,
}

impl Obj {
    pub fn path_id(&self) -> i64 {
        self.info.path_id
    }
    pub fn class_id(&self) -> i32 {
        self.info.class_id
    }
    pub fn raw(&self) -> Result<&[u8]> {
        self.file.bytes(&self.info)
    }
    /// `file:path_id` with the file's base name, as host/source.py's `sid`.
    pub fn sid(&self) -> String {
        format!("{}:{}", base_name(&self.file.name), self.info.path_id)
    }
}

pub fn base_name(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

type Slot = Arc<OnceLock<std::result::Result<Arc<SerializedFile>, String>>>;

pub struct Source {
    pub directory: PathBuf,
    files: Mutex<HashMap<String, Slot>>,
    generator: OnceLock<std::result::Result<generator::Generator, String>>,
    mono_nodes: Mutex<HashMap<(String, String), Arc<Node>>>,
    scripts: Mutex<HashMap<(String, i64), Arc<Script>>>,
    fresh_save: OnceLock<std::result::Result<HashMap<String, fresh_save::Start>, String>>,
    resources: Mutex<HashMap<PathBuf, Arc<memmap2::Mmap>>>,
    /// SpriteHelper's per-file texture cache, keyed as UnityPy keys it: (sprite file, texture path id).
    sprite_textures: Mutex<HashMap<(String, i64), Arc<hk_pil::Image>>>,
}

impl Source {
    /// The data directory (`hollow_knight_Data`) of a Windows install.
    pub fn new(directory: impl Into<PathBuf>) -> Result<Source> {
        let directory = directory.into();
        if !directory.parent().is_some_and(|p| p.join("hollow_knight.exe").is_file()) {
            return Err(Error::Format("Windows source with hollow_knight.exe required".into()));
        }
        let source = Source { directory, files: Mutex::default(), generator: OnceLock::new(), mono_nodes: Mutex::default(), scripts: Mutex::default(), fresh_save: OnceLock::new(), resources: Mutex::default(), sprite_textures: Mutex::default() };
        let header = source.file("globalgamemanagers")?;
        if header.unity_version != UNITY_VERSION || header.format != 22 {
            return Err(Error::Format(format!("Unvalidated source serialization: {}/{}", header.unity_version, header.format)));
        }
        Ok(source)
    }

    /// The install the repository's `.hkpsx/doctor.json` selected.
    pub fn from_doctor(repo_root: &Path) -> Result<Source> {
        let path = repo_root.join(".hkpsx/doctor.json");
        let text = std::fs::read_to_string(&path).map_err(|e| Error::Io(path.display().to_string(), e))?;
        let key = "\"data_directory\": \"";
        let start = text.find(key).ok_or_else(|| Error::Format("doctor.json lacks data_directory".into()))? + key.len();
        let end = start + text[start..].find('"').ok_or_else(|| Error::Format("doctor.json is malformed".into()))?;
        Source::new(&text[start..end])
    }

    /// Resolve one Unity external without discarding its serialized path
    /// (host/source.py `source_path`).
    pub fn resolve(&self, name: &str) -> Result<(PathBuf, String)> {
        let relative = PathBuf::from(name.replace('\\', "/"));
        if relative.is_absolute() || relative.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err(Error::Format(format!("Unsafe Unity external path: {name}")));
        }
        let mut candidates = vec![relative.clone()];
        let first_is_library = relative.components().next().is_some_and(|c| c.as_os_str() == "Library");
        let file_name = relative.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        if first_is_library && file_name.starts_with("unity ") {
            candidates.push(PathBuf::from("Resources").join(&file_name));
        }
        for candidate in candidates {
            let path = self.directory.join(&candidate);
            if path.is_file() {
                return Ok((path, candidate.to_string_lossy().into_owned()));
            }
        }
        Err(Error::Missing(format!("Unity external not found: {name}")))
    }

    /// The files loaded so far, by the path relative to the data directory
    /// (the keys of host/source.py's `files`), sorted.
    pub fn loaded_files(&self) -> Vec<String> {
        let mut names: Vec<String> = self.files.lock().unwrap().iter().filter(|(_, slot)| slot.get().is_some()).map(|(k, _)| k.clone()).collect();
        names.sort();
        names
    }

    /// A loaded file, parsed once per source even when threads race for it.
    pub fn file(&self, name: &str) -> Result<Arc<SerializedFile>> {
        let (path, key) = self.resolve(name)?;
        let slot = self.files.lock().unwrap().entry(key.clone()).or_default().clone();
        slot.get_or_init(|| SerializedFile::open(&path, key).map(Arc::new).map_err(|e| e.to_string()))
            .clone()
            .map_err(Error::Format)
    }

    pub fn object(&self, file: &Arc<SerializedFile>, path_id: i64) -> Result<Obj> {
        let info = *file.object(path_id).ok_or_else(|| Error::Missing(format!("{}:{path_id} not found", file.name)))?;
        Ok(Obj { file: file.clone(), info })
    }

    /// Follow a PPtr written in `file` (host/source.py `ref`).
    pub fn deref(&self, file: &Arc<SerializedFile>, pptr: &Value) -> Result<Obj> {
        let (file_id, path_id) = pptr.pptr().ok_or_else(|| Error::Format("not a PPtr".into()))?;
        if path_id == 0 {
            return Err(Error::Format("null source reference".into()));
        }
        let target = if file_id == 0 {
            file.clone()
        } else {
            let ext = file.externals.get(file_id as usize - 1).ok_or_else(|| Error::Format("bad PPtr file id".into()))?;
            self.file(&ext.path)?
        };
        self.object(&target, path_id)
    }

    fn builtin_node(&self, obj: &Obj) -> Result<Arc<Node>> {
        typetree::builtin(&obj.file.unity_version, obj.class_id())
    }

    /// The MonoBehaviour header (m_GameObject, m_Enabled, m_Script, m_Name),
    /// read without checking that it consumed the whole object.
    pub fn mono_head(&self, obj: &Obj) -> Result<Value> {
        let node = self.builtin_node(obj)?;
        let mut r = Reader { c: serialized::Cursor::new(obj.raw()?, obj.file.big_endian), flavor: Flavor::Python };
        r.read(&node)
    }

    /// The MonoScript a MonoBehaviour runs: (assembly, namespace, class).
    pub fn mono_script(&self, obj: &Obj) -> Result<Arc<Script>> {
        let head = self.mono_head(obj)?;
        let script = self.deref(&obj.file, head.get("m_Script").ok_or_else(|| Error::Format("no m_Script".into()))?)?;
        let key = (script.file.name.clone(), script.path_id());
        if let Some(hit) = self.scripts.lock().unwrap().get(&key) {
            return Ok(hit.clone());
        }
        let tree = self.read_builtin(&script)?;
        let s = |k: &str| tree.get(k).and_then(Value::str).ok_or_else(|| Error::Format(format!("MonoScript lacks {k}")));
        let value = Arc::new((s("m_AssemblyName")?, s("m_Namespace")?, s("m_ClassName")?));
        self.scripts.lock().unwrap().insert(key, value.clone());
        Ok(value)
    }

    /// host/source.py `typename`: the class id's name, or a MonoBehaviour's script class.
    pub fn typename(&self, obj: &Obj) -> Result<String> {
        // UnityPy names a type by its ClassIDType enum, which disagrees with
        // the tree's root type for three globalgamemanagers singletons.
        match obj.class_id() {
            114 => Ok(self.mono_script(obj)?.2.clone()),
            94 => Ok("ScriptMapper".into()),
            98 => Ok("DelayedCallManager".into()),
            655991488 => Ok("Unknown (655991488)".into()),
            _ => Ok(self.builtin_node(obj)?.ty.to_string()),
        }
    }

    fn read_builtin(&self, obj: &Obj) -> Result<Value> {
        let node = self.builtin_node(obj)?;
        read_exact(obj, &node, Flavor::Boost)
    }

    /// The generated tree of one MonoBehaviour's script class, with
    /// host/source.py's two fixes (m_Enabled aligned; string[] repaired).
    pub fn mono_node(&self, obj: &Obj) -> Result<Arc<Node>> {
        let script = self.mono_script(obj)?;
        let (assembly, namespace, class) = (&script.0, &script.1, &script.2);
        let full = if namespace.is_empty() { class.clone() } else { format!("{namespace}.{class}") };
        let assembly = if assembly.ends_with(".dll") { assembly.clone() } else { format!("{assembly}.dll") };
        let key = (assembly.clone(), full.clone());
        if let Some(n) = self.mono_nodes.lock().unwrap().get(&key) {
            return Ok(n.clone());
        }
        let generator = self
            .generator
            .get_or_init(|| generator::Generator::load(&self.directory.join("Managed"), UNITY_VERSION).map_err(|e| e.to_string()))
            .as_ref()
            .map_err(|e| Error::Format(e.clone()))?;
        let mut node = generator.nodes(&assembly, &full)?;
        for child in &mut node.children {
            if &*child.name == "m_Enabled" {
                child.meta |= typetree::ALIGN;
            }
        }
        repair_string_arrays(&mut node);
        let node = Arc::new(node);
        self.mono_nodes.lock().unwrap().insert(key, node.clone());
        Ok(node)
    }

    /// host/source.py `read`.
    pub fn read(&self, obj: &Obj) -> Result<Value> {
        if obj.class_id() != 114 {
            return self.read_builtin(obj);
        }
        let node = self.mono_node(obj)?;
        let tree = read_exact(obj, &node, Flavor::Python)?;
        let head = self.mono_head(obj)?;
        if tree.get("m_Script") != head.get("m_Script") {
            return Err(Error::Format("generated header differs from native header".into()));
        }
        Ok(tree)
    }
}

fn read_exact(obj: &Obj, node: &Node, flavor: Flavor) -> Result<Value> {
    let raw = obj.raw()?;
    let mut r = Reader { c: serialized::Cursor::new(raw, obj.file.big_endian), flavor };
    let v = r.read(node)?;
    if r.c.pos != raw.len() {
        return Err(Error::Format(format!("Expected to read {} bytes, but only read {} bytes", raw.len(), r.c.pos)));
    }
    Ok(v)
}

/// A generated `string[]` container can be typed `string`; UnityPy would read
/// it as one scalar string. Retype such a node so it reads as a vector.
fn repair_string_arrays(node: &mut Node) {
    if &*node.ty == "string" && node.children.len() == 1 && &*node.children[0].ty == "Array" {
        let data = &node.children[0].children;
        if data.len() == 2 && &*data[1].ty == "string" {
            node.ty = "vector".into();
        }
    }
    for child in &mut node.children {
        repair_string_arrays(child);
    }
}

impl Source {
    /// A `.resS`/`.resource` file, mapped once.
    pub fn resource(&self, path: &Path) -> Result<Arc<memmap2::Mmap>> {
        if let Some(m) = self.resources.lock().unwrap().get(path) {
            return Ok(m.clone());
        }
        let file = std::fs::File::open(path).map_err(|e| Error::Io(path.display().to_string(), e))?;
        // SAFETY: read-only build input.
        let map = Arc::new(unsafe { memmap2::Mmap::map(&file) }.map_err(|e| Error::Io(path.display().to_string(), e))?);
        self.resources.lock().unwrap().insert(path.to_path_buf(), map.clone());
        Ok(map)
    }

    /// The unflipped texture image SpriteHelper.get_image caches.
    pub fn texture_cached(&self, sprite_file: &str, tex: &Obj) -> Result<Arc<hk_pil::Image>> {
        let key = (sprite_file.to_string(), tex.path_id());
        if let Some(i) = self.sprite_textures.lock().unwrap().get(&key) {
            return Ok(i.clone());
        }
        let img = Arc::new(texture::texture_image(self, tex, false)?);
        self.sprite_textures.lock().unwrap().insert(key, img.clone());
        Ok(img)
    }
}
