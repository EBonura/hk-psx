//! One room's objects, hierarchy and transforms (host/scene.py).
//!
//! A room the original additively loads a second scene into is read as one
//! scene: `SceneAdditiveLoadConditional` picks the scene from a PlayerData
//! bool answered from the fresh save, and the additive file's objects join the
//! room's id space shifted by `ADDITIVE_ID_BASE` per merged file.

use crate::activation::{self, Gates};
use crate::fresh_save::Start;
use crate::serialized::SerializedFile;
use crate::value::Value;
use crate::{base_name, Error, Obj, Result, Source};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

pub const ADDITIVE_ID_BASE: i64 = 100_000;

pub struct SceneObject {
    pub id: i64,
    pub typename: String,
    pub tree: Value,
}

pub struct Scene<'s> {
    pub source: &'s Source,
    pub base: Arc<SerializedFile>,
    /// Every object but standalone Meshes, in the order host/scene.py's dict holds them.
    pub objects: Vec<SceneObject>,
    index: HashMap<i64, usize>,
    pub errors: Vec<(String, String, String)>,
    /// Merged id to the object it really is (the MergedFile's `objects`).
    refs: HashMap<i64, Obj>,
    /// The merged externals list (the MergedFile's `externals`).
    pub externals: Vec<String>,
    /// (id base, file name), highest base first.
    origins: Vec<(i64, String)>,
    pub additive: Vec<String>,
    pub gos: HashMap<i64, usize>,
    pub transforms: HashMap<i64, usize>,
    pub go_transform: HashMap<i64, i64>,
    pub gated_off: HashSet<i64>,
    pub gates: Option<Gates>,
    world_cache: Mutex<HashMap<i64, [[f64; 4]; 4]>>,
    active_cache: Mutex<HashMap<i64, bool>>,
}

/// BuildSettings scene name to level file, with its order assertion.
pub fn build_settings(source: &Source) -> Result<&'static HashMap<String, String>> {
    static CACHE: OnceLock<std::result::Result<HashMap<String, String>, String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let file = source
                .file("globalgamemanagers")
                .map_err(|e| e.to_string())?;
            let settings = file
                .objects
                .iter()
                .find(|o| o.class_id == 141)
                .ok_or("no BuildSettings")?;
            let tree = source
                .read(&Obj {
                    file: file.clone(),
                    info: *settings,
                })
                .map_err(|e| e.to_string())?;
            let mut out = HashMap::new();
            for (i, path) in tree
                .get("scenes")
                .and_then(Value::list)
                .unwrap_or(&[])
                .iter()
                .enumerate()
            {
                let p = path.str().unwrap_or_default();
                let name = p.rsplit('/').next().unwrap_or("");
                let name = name.strip_suffix(".unity").unwrap_or(name).to_string();
                out.insert(name, format!("level{i}"));
            }
            if out.get("Town").map(String::as_str) != Some("level7") {
                return Err("BuildSettings scene order changed".into());
            }
            Ok(out)
        })
        .as_ref()
        .map_err(|e| Error::Format(e.clone()))
}

/// Move one additive object's references into the merged id space.
fn retarget(tree: &mut Value, id_base: i64, externals: &HashMap<i32, i32>) {
    match tree {
        Value::Map(fields) => {
            if fields.len() == 2
                && fields.iter().any(|(k, _)| &**k == "m_PathID")
                && fields.iter().any(|(k, _)| &**k == "m_FileID")
            {
                let path = fields
                    .iter()
                    .find(|(k, _)| &**k == "m_PathID")
                    .and_then(|(_, v)| v.int())
                    .unwrap_or(0);
                if path != 0 {
                    let file = fields
                        .iter()
                        .find(|(k, _)| &**k == "m_FileID")
                        .and_then(|(_, v)| v.int())
                        .unwrap_or(0);
                    for (k, v) in fields.iter_mut() {
                        if file != 0 && &**k == "m_FileID" {
                            *v = Value::Int(externals[&(file as i32)] as i64);
                        } else if file == 0 && &**k == "m_PathID" {
                            *v = Value::Int(path + id_base);
                        }
                    }
                }
                return;
            }
            for (_, v) in fields.iter_mut() {
                retarget(v, id_base, externals);
            }
        }
        Value::List(items) => {
            for v in items {
                retarget(v, id_base, externals);
            }
        }
        _ => {}
    }
}

impl<'s> Scene<'s> {
    pub fn new(source: &'s Source, name: &str) -> Result<Scene<'s>> {
        Scene::open(source, name, true)
    }

    pub fn open(source: &'s Source, name: &str, merge_additive: bool) -> Result<Scene<'s>> {
        let base = source.file(name)?;
        let mut sc = Scene {
            source,
            base: base.clone(),
            objects: Vec::new(),
            index: HashMap::new(),
            errors: Vec::new(),
            refs: HashMap::new(),
            externals: base.externals.iter().map(|e| e.path.clone()).collect(),
            origins: Vec::new(),
            additive: Vec::new(),
            gos: HashMap::new(),
            transforms: HashMap::new(),
            go_transform: HashMap::new(),
            gated_off: HashSet::new(),
            gates: None,
            world_cache: Mutex::default(),
            active_cache: Mutex::default(),
        };
        for info in &base.objects {
            sc.refs.insert(
                info.path_id,
                Obj {
                    file: base.clone(),
                    info: *info,
                },
            );
        }
        sc.read(&base, 0, None);
        sc.reindex();
        let merged = if merge_additive {
            sc.merge()?
        } else {
            Vec::new()
        };
        let mut origins = vec![(0, base_name(&base.name).to_string())];
        origins.extend(
            merged
                .iter()
                .map(|(b, f): &(i64, Arc<SerializedFile>)| (*b, base_name(&f.name).to_string())),
        );
        origins.sort_by(|a, b| b.cmp(a));
        sc.origins = origins;
        sc.additive = merged
            .iter()
            .map(|(_, f)| base_name(&f.name).to_string())
            .collect();
        if !merged.is_empty() {
            sc.reindex();
        }
        let gates = Gates::new(&sc)?;
        sc.gated_off = gates.off.clone();
        sc.gates = Some(gates);
        Ok(sc)
    }

    fn read(
        &mut self,
        file: &Arc<SerializedFile>,
        id_base: i64,
        externals: Option<&HashMap<i32, i32>>,
    ) {
        let source = self.source;
        let objs: Vec<Obj> = file
            .objects
            .iter()
            .filter(|i| i.class_id != 43)
            .map(|i| Obj {
                file: file.clone(),
                info: *i,
            })
            .collect();
        // Reading is independent per object; the order is restored after.
        let results: Vec<(Result<String>, Result<Value>)> = {
            use rayon::prelude::*;
            objs.par_iter()
                .map(|o| (source.typename(o), source.read(o)))
                .collect()
        };
        for (o, (typename, tree)) in objs.into_iter().zip(results) {
            match (typename, tree) {
                (Ok(t), Ok(mut v)) => {
                    if id_base != 0 {
                        retarget(&mut v, id_base, externals.unwrap());
                    }
                    let id = o.path_id() + id_base;
                    self.index.insert(id, self.objects.len());
                    self.objects.push(SceneObject {
                        id,
                        typename: t,
                        tree: v,
                    });
                }
                (t, v) => {
                    let t = t.unwrap_or_else(|e| format!("?{e}"));
                    let e = v.err().map(|e| e.to_string()).unwrap_or_default();
                    self.errors.push((o.sid(), t, e));
                }
            }
        }
    }

    fn reindex(&mut self) {
        self.gos.clear();
        self.transforms.clear();
        self.go_transform.clear();
        for (i, o) in self.objects.iter().enumerate() {
            match o.typename.as_str() {
                "GameObject" => {
                    self.gos.insert(o.id, i);
                }
                "Transform" => {
                    self.transforms.insert(o.id, i);
                }
                _ => {}
            }
        }
        // In object order, so a GameObject with two transforms keeps the later one, as the dict does.
        for o in self.objects.iter().filter(|o| o.typename == "Transform") {
            if let Some(g) = o
                .tree
                .get("m_GameObject")
                .and_then(|p| p.get("m_PathID"))
                .and_then(Value::int)
            {
                self.go_transform.insert(g, o.id);
            }
        }
    }

    pub fn object(&self, id: i64) -> Option<&SceneObject> {
        self.index.get(&id).map(|&i| &self.objects[i])
    }
    pub fn go(&self, gid: i64) -> Option<&Value> {
        self.gos.get(&gid).map(|&i| &self.objects[i].tree)
    }
    pub fn transform(&self, tid: i64) -> Option<&Value> {
        self.transforms.get(&tid).map(|&i| &self.objects[i].tree)
    }

    fn father_of(&self, tid: i64) -> i64 {
        self.transform(tid)
            .and_then(|t| t.get("m_Father"))
            .and_then(|f| f.get("m_PathID"))
            .and_then(Value::int)
            .unwrap_or(0)
    }
    fn go_of_transform(&self, tid: i64) -> i64 {
        self.transform(tid)
            .and_then(|t| t.get("m_GameObject"))
            .and_then(|f| f.get("m_PathID"))
            .and_then(Value::int)
            .unwrap_or(0)
    }
    fn is_active_flag(&self, gid: i64) -> bool {
        self.go(gid)
            .and_then(|g| g.get("m_IsActive"))
            .is_some_and(Value::truthy)
    }

    /// `active` without the load-time gates.
    pub fn authored_active(&self, mut gid: i64) -> bool {
        loop {
            if !self.gos.contains_key(&gid)
                || !self.go_transform.contains_key(&gid)
                || !self.is_active_flag(gid)
            {
                return false;
            }
            let father = self.father_of(self.go_transform[&gid]);
            if father == 0 {
                return true;
            }
            if !self.transforms.contains_key(&father) {
                return false;
            }
            gid = self.go_of_transform(father);
        }
    }

    fn merge(&mut self) -> Result<Vec<(i64, Arc<SerializedFile>)>> {
        let source = self.source;
        let mut merged: Vec<(i64, Arc<SerializedFile>)> = Vec::new();
        let mut loaders: Vec<i64> = self
            .objects
            .iter()
            .filter(|o| o.typename == "SceneAdditiveLoadConditional")
            .map(|o| o.id)
            .collect();
        loaders.sort();
        for sid in loaders {
            let tree = self.object(sid).unwrap().tree.clone();
            let f = |k: &str| tree.get(k).cloned().unwrap_or(Value::Bool(false));
            let go = tree
                .get("m_GameObject")
                .and_then(|p| p.get("m_PathID"))
                .and_then(Value::int)
                .unwrap_or(0);
            if !f("m_Enabled").truthy() || !self.authored_active(go) {
                continue;
            }
            if [
                "needsPlayerDataInt",
                "extraBoolTests",
                "extraIntTests",
                "isIntValue",
                "usePersistentBoolItem",
                "doorTrigger",
            ]
            .iter()
            .any(|k| f(k).truthy())
            {
                return Err(Error::Format(format!(
                    "unsupported SceneAdditiveLoadConditional test set: {}:{sid}",
                    base_name(&self.base.name)
                )));
            }
            let playerdata = source.fresh_save()?;
            let value = activation::fresh_save_bool(playerdata, &f("needsPlayerDataBool"))
                .map_err(|r| Error::Format(r.0))?;
            let wanted = if value == f("playerDataBoolValue").truthy() {
                f("sceneNameToLoad")
            } else {
                f("altSceneNameToLoad")
            };
            let wanted = wanted.str().unwrap_or_default();
            if wanted.is_empty() {
                continue;
            }
            let level = build_settings(source)?
                .get(&wanted)
                .ok_or_else(|| Error::Missing(format!("no scene {wanted}")))?;
            let file = source.file(level)?;
            let id_base = ADDITIVE_ID_BASE * (merged.len() as i64 + 1);
            let base_max = self
                .base
                .objects
                .iter()
                .map(|o| o.path_id)
                .max()
                .unwrap_or(0);
            let file_max = file.objects.iter().map(|o| o.path_id).max().unwrap_or(0);
            if base_max >= id_base || file_max >= ADDITIVE_ID_BASE {
                return Err(Error::Format(
                    "additive merge id base overlaps a source file".into(),
                ));
            }
            let mut paths: Vec<String> = self.externals.clone();
            let mut map = HashMap::new();
            for (i, e) in file.externals.iter().enumerate() {
                if !paths.contains(&e.path) {
                    paths.push(e.path.clone());
                }
                map.insert(
                    i as i32 + 1,
                    paths.iter().position(|p| *p == e.path).unwrap() as i32 + 1,
                );
            }
            self.read(&file, id_base, Some(&map));
            for info in &file.objects {
                self.refs.insert(
                    info.path_id + id_base,
                    Obj {
                        file: file.clone(),
                        info: *info,
                    },
                );
            }
            for e in &file.externals {
                if !self.externals.contains(&e.path) {
                    self.externals.push(e.path.clone());
                }
            }
            merged.push((id_base, file));
        }
        Ok(merged)
    }

    /// `file:id` of one object, under the file it was serialized in.
    pub fn sid(&self, id: i64) -> String {
        for (base, name) in &self.origins {
            if id > *base {
                return format!("{name}:{}", id - base);
            }
        }
        format!("{}:{id}", base_name(&self.base.name))
    }

    /// host/source.py `ref` against this scene's (possibly merged) file.
    pub fn deref(&self, pptr: &Value) -> Result<Obj> {
        let (file_id, path_id) = pptr
            .pptr()
            .ok_or_else(|| Error::Format("not a PPtr".into()))?;
        if path_id == 0 {
            return Err(Error::Format("null source reference".into()));
        }
        if file_id == 0 {
            return self
                .refs
                .get(&path_id)
                .cloned()
                .ok_or_else(|| Error::Missing(format!("object {path_id} not in scene")));
        }
        let path = self
            .externals
            .get(file_id as usize - 1)
            .ok_or_else(|| Error::Format("bad PPtr file id".into()))?;
        let file = self.source.file(path)?;
        self.source.object(&file, path_id)
    }

    /// A transform's world matrix; Err when it or an ancestor is not a Transform of this scene.
    pub fn world(&self, tid: i64) -> Result<[[f64; 4]; 4]> {
        if let Some(m) = self.world_cache.lock().unwrap().get(&tid) {
            return Ok(*m);
        }
        let t = self
            .transform(tid)
            .ok_or_else(|| Error::Missing(format!("transform {tid} not in scene")))?;
        let v = |a: &str, k: &str| {
            t.get(a)
                .and_then(|x| x.get(k))
                .and_then(Value::float)
                .unwrap_or(0.0)
        };
        let (x, y, z, w) = (
            v("m_LocalRotation", "x"),
            v("m_LocalRotation", "y"),
            v("m_LocalRotation", "z"),
            v("m_LocalRotation", "w"),
        );
        let (px, py, pz) = (
            v("m_LocalPosition", "x"),
            v("m_LocalPosition", "y"),
            v("m_LocalPosition", "z"),
        );
        let mut r = [
            [
                1.0 - 2.0 * (y * y + z * z),
                2.0 * (x * y - z * w),
                2.0 * (x * z + y * w),
                px,
            ],
            [
                2.0 * (x * y + z * w),
                1.0 - 2.0 * (x * x + z * z),
                2.0 * (y * z - x * w),
                py,
            ],
            [
                2.0 * (x * z - y * w),
                2.0 * (y * z + x * w),
                1.0 - 2.0 * (x * x + y * y),
                pz,
            ],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let sc = [
            v("m_LocalScale", "x"),
            v("m_LocalScale", "y"),
            v("m_LocalScale", "z"),
        ];
        for row in r.iter_mut().take(3) {
            for (col, s) in sc.iter().enumerate() {
                row[col] *= s;
            }
        }
        let father = self.father_of(tid);
        if father != 0 {
            let a = self.world(father)?;
            let mut out = [[0.0; 4]; 4];
            for i in 0..4 {
                for j in 0..4 {
                    let mut s = 0.0;
                    for k in 0..4 {
                        s += a[i][k] * r[k][j];
                    }
                    out[i][j] = s;
                }
            }
            r = out;
        }
        self.world_cache.lock().unwrap().insert(tid, r);
        Ok(r)
    }

    pub fn point(&self, gid: i64, x: f64, y: f64, z: f64) -> Result<[f64; 3]> {
        let tid = *self
            .go_transform
            .get(&gid)
            .ok_or_else(|| Error::Missing(format!("object {gid} has no transform")))?;
        let m = self.world(tid)?;
        let v = [x, y, z, 1.0];
        let mut out = [0.0; 3];
        for (i, o) in out.iter_mut().enumerate() {
            let mut s = 0.0;
            for j in 0..4 {
                s += m[i][j] * v[j];
            }
            *o = s;
        }
        Ok(out)
    }

    pub fn active(&self, gid: i64) -> bool {
        if let Some(&a) = self.active_cache.lock().unwrap().get(&gid) {
            return a;
        }
        let answer = (|| {
            if !self.gos.contains_key(&gid) || !self.go_transform.contains_key(&gid) {
                return false;
            }
            if !self.is_active_flag(gid) || self.gated_off.contains(&gid) {
                return false;
            }
            let f = self.father_of(self.go_transform[&gid]);
            if f != 0 && !self.transforms.contains_key(&f) {
                return false;
            }
            f == 0 || self.active(self.go_of_transform(f))
        })();
        self.active_cache.lock().unwrap().insert(gid, answer);
        answer
    }
}

impl Source {
    /// The fresh-save PlayerData defaults, read once per source.
    pub fn fresh_save(&self) -> Result<&HashMap<String, Start>> {
        self.fresh_save
            .get_or_init(|| {
                crate::fresh_save::playerdata_defaults(&self.directory.join("Managed"))
                    .map_err(|e| e.to_string())
            })
            .as_ref()
            .map_err(|e| Error::Format(e.clone()))
    }
}
