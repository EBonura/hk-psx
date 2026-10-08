//! A Managed/ folder of assemblies with cross-assembly type resolution, the
//! way Mono.Cecil's default resolver sees it (TypeRefs through AssemblyRefs,
//! type forwarders, nested types).

use crate::sig::{self, Type};
use crate::tables::Coded;
use crate::{Assembly, Error, Result, Table};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TypeId {
    pub asm: usize,
    pub rid: u32,
}

/// Per-assembly indexes built on first use.
#[derive(Default)]
struct Index {
    top: HashMap<(String, String), u32>,
    enclosing: HashMap<u32, u32>,
    nested: HashMap<u32, Vec<u32>>,
    generics: HashMap<u32, Vec<String>>,
    attributes: HashMap<u32, Vec<u32>>,
    constants: HashMap<u32, ()>,
    method_owner: Vec<u32>,
    forwarded: HashMap<(String, String), String>,
}

pub struct Loaded {
    pub asm: Assembly,
    index: Index,
}

pub struct Managed {
    dir: PathBuf,
    names: HashMap<String, usize>,
    files: Vec<String>,
    slots: Vec<OnceLock<Option<Arc<Loaded>>>>,
    builtins: OnceLock<Vec<Option<TypeId>>>,
}

fn build_index(asm: &Assembly) -> Index {
    let mut ix = Index::default();
    for rid in 1..=asm.rows(Table::NestedClass) {
        let inner = asm.get(Table::NestedClass, rid, 0);
        let outer = asm.get(Table::NestedClass, rid, 1);
        ix.enclosing.insert(inner, outer);
        ix.nested.entry(outer).or_default().push(inner);
    }
    for rid in 1..=asm.rows(Table::TypeDef) {
        if !ix.enclosing.contains_key(&rid) {
            let name = asm.string(asm.get(Table::TypeDef, rid, 1)).to_string();
            let ns = asm.string(asm.get(Table::TypeDef, rid, 2)).to_string();
            ix.top.entry((ns, name)).or_insert(rid);
        }
    }
    let mut params: Vec<(u32, u16, String)> = Vec::new();
    for rid in 1..=asm.rows(Table::GenericParam) {
        let number = asm.get(Table::GenericParam, rid, 0) as u16;
        if let Some((Table::TypeDef, owner)) = Coded::TypeOrMethodDef.decode(asm.get(Table::GenericParam, rid, 2)) {
            params.push((owner, number, asm.string(asm.get(Table::GenericParam, rid, 3)).to_string()));
        }
    }
    params.sort_by_key(|p| (p.0, p.1));
    for (owner, _, name) in params {
        ix.generics.entry(owner).or_default().push(name);
    }
    for rid in 1..=asm.rows(Table::CustomAttribute) {
        if let Some((Table::Field, field)) = Coded::HasCustomAttribute.decode(asm.get(Table::CustomAttribute, rid, 0)) {
            ix.attributes.entry(field).or_default().push(rid);
        }
    }
    for rid in 1..=asm.rows(Table::Constant) {
        if let Some((Table::Field, field)) = Coded::HasConstant.decode(asm.get(Table::Constant, rid, 1)) {
            ix.constants.insert(field, ());
        }
    }
    for rid in 1..=asm.rows(Table::ExportedType) {
        if let Some((Table::AssemblyRef, r)) = Coded::Implementation.decode(asm.get(Table::ExportedType, rid, 4)) {
            let name = asm.string(asm.get(Table::ExportedType, rid, 2)).to_string();
            let ns = asm.string(asm.get(Table::ExportedType, rid, 3)).to_string();
            ix.forwarded.entry((ns, name)).or_insert_with(|| asm.string(asm.get(Table::AssemblyRef, r, 6)).to_string());
        }
    }
    let methods = asm.rows(Table::MethodDef);
    ix.method_owner = vec![0; methods as usize + 1];
    let types = asm.rows(Table::TypeDef);
    for rid in 1..=types {
        let start = asm.get(Table::TypeDef, rid, 5);
        let end = if rid < types { asm.get(Table::TypeDef, rid + 1, 5) } else { methods + 1 };
        for m in start..end.min(methods + 1) {
            ix.method_owner[m as usize] = rid;
        }
    }
    ix
}

impl Managed {
    /// Every `*.dll` in `dir`; each is parsed on first use.
    pub fn new(dir: impl Into<PathBuf>) -> Managed {
        let dir = dir.into();
        let mut files: Vec<String> = std::fs::read_dir(&dir)
            .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".dll")).collect())
            .unwrap_or_default();
        files.sort();
        let names = files.iter().enumerate().map(|(i, f)| (f.clone(), i)).collect();
        let slots = files.iter().map(|_| OnceLock::new()).collect();
        Managed { dir, names, files, slots, builtins: OnceLock::new() }
    }

    /// The assembly whose file is `<name>.dll` (or `name` when it already ends in .dll).
    pub fn assembly(&self, name: &str) -> Option<usize> {
        let index = match self.names.get(name) {
            Some(&i) => i,
            None => *self.names.get(&format!("{name}.dll"))?,
        };
        let loaded = self.slots[index].get_or_init(|| {
            let asm = Assembly::open(&self.dir.join(&self.files[index])).ok()?;
            let index = build_index(&asm);
            Some(Arc::new(Loaded { asm, index }))
        });
        loaded.as_ref().map(|_| index)
    }

    pub fn loaded(&self, asm: usize) -> &Loaded {
        self.slots[asm].get().and_then(|l| l.as_deref()).expect("assembly index of a loaded assembly")
    }

    pub fn find(&self, asm: usize, namespace: &str, name: &str) -> Option<TypeId> {
        let l = self.loaded(asm);
        let key = (namespace.to_string(), name.to_string());
        if let Some(&rid) = l.index.top.get(&key) {
            return Some(TypeId { asm, rid });
        }
        // Type forwarders.
        let target = l.index.forwarded.get(&key)?;
        let t = self.assembly(target)?;
        self.find(t, namespace, name)
    }

    pub fn nested(&self, outer: TypeId, name: &str) -> Option<TypeId> {
        let l = self.loaded(outer.asm);
        l.index
            .nested
            .get(&outer.rid)?
            .iter()
            .find(|&&rid| l.asm.string(l.asm.get(Table::TypeDef, rid, 1)) == name)
            .map(|&rid| TypeId { asm: outer.asm, rid })
    }

    /// Resolve a TypeDef/TypeRef row of `asm` to its definition.
    pub fn resolve(&self, asm: usize, table: Table, rid: u32) -> Option<TypeId> {
        match table {
            Table::TypeDef => Some(TypeId { asm, rid }),
            Table::TypeRef => {
                let l = self.loaded(asm);
                let a = &l.asm;
                let name = a.string(a.get(Table::TypeRef, rid, 1)).to_string();
                let ns = a.string(a.get(Table::TypeRef, rid, 2)).to_string();
                match Coded::ResolutionScope.decode(a.get(Table::TypeRef, rid, 0)) {
                    Some((Table::AssemblyRef, r)) => {
                        let target = a.string(a.get(Table::AssemblyRef, r, 6)).to_string();
                        let t = self.assembly(&target)?;
                        self.find(t, &ns, &name)
                    }
                    Some((Table::TypeRef, r)) => {
                        let outer = self.resolve(asm, Table::TypeRef, r)?;
                        self.nested(outer, &name)
                    }
                    Some((Table::Module, _)) | None => self.find(asm, &ns, &name),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// The corlib definition of a built-in element type.
    pub fn builtin(&self, e: u8) -> Option<TypeId> {
        let table = self.builtins.get_or_init(|| {
            let corlib = self.assembly("mscorlib");
            (0..=0x1cu8).map(|e| corlib.and_then(|c| self.find(c, "System", sig::builtin_name(e)))).collect()
        });
        table.get(e as usize).copied().flatten()
    }

    pub fn name(&self, t: TypeId) -> String {
        let l = self.loaded(t.asm);
        l.asm.string(l.asm.get(Table::TypeDef, t.rid, 1)).to_string()
    }
    pub fn namespace(&self, t: TypeId) -> String {
        let l = self.loaded(t.asm);
        l.asm.string(l.asm.get(Table::TypeDef, t.rid, 2)).to_string()
    }
    pub fn enclosing(&self, t: TypeId) -> Option<TypeId> {
        self.loaded(t.asm).index.enclosing.get(&t.rid).map(|&rid| TypeId { asm: t.asm, rid })
    }
    /// Cecil's FullName: `Ns.Name`, nested types as `Ns.Outer/Inner`.
    pub fn full_name(&self, t: TypeId) -> String {
        match self.enclosing(t) {
            Some(outer) => format!("{}/{}", self.full_name(outer), self.name(t)),
            None => {
                let ns = self.namespace(t);
                if ns.is_empty() {
                    self.name(t)
                } else {
                    format!("{ns}.{}", self.name(t))
                }
            }
        }
    }
    pub fn flags(&self, t: TypeId) -> u32 {
        let l = self.loaded(t.asm);
        l.asm.get(Table::TypeDef, t.rid, 0)
    }
    /// The Extends column as a signature type (None for interfaces and System.Object).
    pub fn base(&self, t: TypeId) -> Result<Option<Type>> {
        let l = self.loaded(t.asm);
        let coded = l.asm.get(Table::TypeDef, t.rid, 3);
        match Coded::TypeDefOrRef.decode(coded) {
            Some((_, 0)) | None => Ok(None),
            Some((Table::TypeSpec, rid)) => Ok(Some(self.type_spec(t.asm, rid)?)),
            Some((table, rid)) => Ok(Some(Type::Named { table, rid, value_type: false })),
        }
    }
    pub fn type_spec(&self, asm: usize, rid: u32) -> Result<Type> {
        let l = self.loaded(asm);
        let blob = l.asm.blob(l.asm.get(Table::TypeSpec, rid, 0));
        sig::SigReader { d: blob, p: 0 }.ty()
    }
    pub fn generic_params(&self, t: TypeId) -> Vec<String> {
        self.loaded(t.asm).index.generics.get(&t.rid).cloned().unwrap_or_default()
    }
    /// Field row ids of a type, in declaration order.
    pub fn fields(&self, t: TypeId) -> std::ops::Range<u32> {
        let l = self.loaded(t.asm);
        let a = &l.asm;
        let start = a.get(Table::TypeDef, t.rid, 4);
        let end = if t.rid < a.rows(Table::TypeDef) { a.get(Table::TypeDef, t.rid + 1, 4) } else { a.rows(Table::Field) + 1 };
        start..end.max(start)
    }
    pub fn field_name(&self, asm: usize, rid: u32) -> String {
        let l = self.loaded(asm);
        l.asm.string(l.asm.get(Table::Field, rid, 1)).to_string()
    }
    pub fn field_flags(&self, asm: usize, rid: u32) -> u16 {
        self.loaded(asm).asm.get(Table::Field, rid, 0) as u16
    }
    pub fn field_type(&self, asm: usize, rid: u32) -> Result<Type> {
        let l = self.loaded(asm);
        sig::field_type(l.asm.blob(l.asm.get(Table::Field, rid, 2)))
    }
    pub fn field_has_constant(&self, asm: usize, rid: u32) -> bool {
        self.loaded(asm).index.constants.contains_key(&rid)
    }
    /// (Name, FullName) of each custom attribute type on a field.
    pub fn field_attributes(&self, asm: usize, rid: u32) -> Vec<(String, String)> {
        let l = self.loaded(asm);
        let a = &l.asm;
        let mut out = Vec::new();
        for &ca in l.index.attributes.get(&rid).map(Vec::as_slice).unwrap_or(&[]) {
            let owner = match Coded::CustomAttributeType.decode(a.get(Table::CustomAttribute, ca, 1)) {
                Some((Table::MethodDef, m)) => Some((Table::TypeDef, l.index.method_owner.get(m as usize).copied().unwrap_or(0))),
                Some((Table::MemberRef, m)) => Coded::MemberRefParent.decode(a.get(Table::MemberRef, m, 0)),
                _ => None,
            };
            let names = match owner {
                Some((Table::TypeDef, r)) => Some((a.string(a.get(Table::TypeDef, r, 1)), a.string(a.get(Table::TypeDef, r, 2)))),
                Some((Table::TypeRef, r)) => Some((a.string(a.get(Table::TypeRef, r, 1)), a.string(a.get(Table::TypeRef, r, 2)))),
                _ => None,
            };
            if let Some((name, ns)) = names {
                let full = if ns.is_empty() { name.to_string() } else { format!("{ns}.{name}") };
                out.push((name.to_string(), full));
            }
        }
        out
    }
    /// The defining assembly's name, e.g. "mscorlib".
    pub fn assembly_name(&self, asm: usize) -> String {
        self.loaded(asm).asm.name().to_string()
    }
}

impl From<Error> for String {
    fn from(e: Error) -> String {
        e.0
    }
}
