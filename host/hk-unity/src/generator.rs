//! MonoBehaviour type trees generated from the managed assemblies.
//!
//! Unity strips script type trees from player builds, so UnityPy rebuilds
//! them with TypeTreeGeneratorAPI, whose default backend is AssetsTools.NET's
//! MonoCecilTempGenerator. This is a port of that generator's rules
//! (AssetsTools.NET, MIT licence, Copyright (c) nesrak1; see
//! THIRD_PARTY_NOTICES.md): which fields serialize, how generics bind, the
//! special Unity value types, and the serialization depth limit. Its output
//! is checked tree by tree against TypeTreeGeneratorAPI 0.0.10 on every
//! script class of the install.

use crate::typetree::{Node, ALIGN};
use crate::{Error, Result};
use hk_dotnet::managed::{Managed, TypeId};
use hk_dotnet::sig::Type;
use hk_dotnet::Table;
use std::path::Path;

/// AssetsTools' GetSerializationLimit for any Unity newer than 2020.1.4.
const SERIALIZATION_LIMIT: i32 = 10;

const SPECIAL: &[&str] = &[
    "UnityEngine.Color",
    "UnityEngine.Color32",
    "UnityEngine.Gradient",
    "UnityEngine.Vector2",
    "UnityEngine.Vector3",
    "UnityEngine.Vector4",
    "UnityEngine.LayerMask",
    "UnityEngine.Quaternion",
    "UnityEngine.Bounds",
    "UnityEngine.Rect",
    "UnityEngine.RectOffset",
    "UnityEngine.Matrix4x4",
    "UnityEngine.AnimationCurve",
    "UnityEngine.GUIStyle",
    "UnityEngine.Vector2Int",
    "UnityEngine.Vector3Int",
    "UnityEngine.PropertyName",
    "UnityEngine.BoundsInt",
];
const BLACKLISTED: &[&str] = &[
    "mscorlib",
    "mscorlib.dll",
    "netstandard",
    "netstandard.dll",
    "System.Core",
    "System.Core.dll",
    "System",
    "System.dll",
    "System.Private.CoreLib",
    "System.Private.CoreLib.dll",
    "System.Collections",
    "System.Collections.dll",
    "System.Collections.NonGeneric",
    "System.Collections.NonGeneric.dll",
];
const PRIMITIVE_NAMES: &[&str] = &[
    "Boolean", "Char", "IntPtr", "UIntPtr", "SByte", "Byte", "Int16", "UInt16", "Int32", "UInt32",
    "Int64", "UInt64", "Single", "Double",
];

fn base_to_primitive(full: &str) -> String {
    match full {
        "System.Boolean" => "UInt8",
        "System.SByte" => "SInt8",
        "System.Byte" => "UInt8",
        "System.Char" => "UInt16",
        "System.Int16" => "SInt16",
        "System.UInt16" => "UInt16",
        "System.Int32" => "int",
        "System.UInt32" => "unsigned int",
        "System.Int64" => "SInt64",
        "System.UInt64" => "UInt64",
        "System.Double" => "double",
        "System.Single" => "float",
        "System.String" => "string",
        other => other,
    }
    .to_string()
}

/// AssetTypeValueField.GetValueTypeByTypeName then CommonMonoTemplateHelper.TypeAligns.
fn type_aligns(ty: &str) -> bool {
    matches!(
        ty,
        "bool"
            | "SInt8"
            | "char"
            | "UInt8"
            | "unsigned char"
            | "SInt16"
            | "short"
            | "UInt16"
            | "unsigned short"
    )
}

fn nerr(what: &str) -> Error {
    Error::Format(format!("generator: {what}"))
}

/// A template field.
#[derive(Clone, Debug)]
struct F {
    name: String,
    ty: String,
    aligned: bool,
    children: Vec<F>,
}

fn f(name: &str, ty: &str, aligned: bool, children: Vec<F>) -> F {
    F {
        name: name.into(),
        ty: ty.into(),
        aligned,
        children,
    }
}
fn leaf(name: &str, ty: &str) -> F {
    f(name, ty, false, vec![])
}
fn array(field: &F) -> Vec<F> {
    vec![f(
        "Array",
        "Array",
        true,
        vec![
            leaf("size", "int"),
            f("data", &field.ty, false, field.children.clone()),
        ],
    )]
}
fn vector(field: F) -> F {
    let children = array(&field);
    f(&field.name, "vector", false, children)
}
fn vector_with_type(field: F) -> F {
    let children = array(&field);
    f(&field.name, &field.ty, false, children)
}
fn string_children() -> Vec<F> {
    array(&leaf("data", "char"))
}
fn string(name: &str) -> F {
    f(name, "string", false, string_children())
}
fn floats(names: &[&str]) -> Vec<F> {
    names.iter().map(|n| leaf(n, "float")).collect()
}
fn ints(names: &[&str]) -> Vec<F> {
    names.iter().map(|n| leaf(n, "int")).collect()
}
fn pptr(name: &str, ty: &str) -> F {
    f(name, &format!("PPtr<{ty}>"), false, pptr_children())
}
fn pptr_children() -> Vec<F> {
    vec![leaf("m_FileID", "int"), leaf("m_PathID", "SInt64")]
}
fn rgbaf(name: &str) -> F {
    f(name, "ColorRGBA", false, floats(&["r", "g", "b", "a"]))
}
fn rect_offset(name: &str) -> F {
    f(
        name,
        "RectOffset",
        false,
        ints(&["m_Left", "m_Right", "m_Top", "m_Bottom"]),
    )
}
fn gui_style_state(name: &str) -> F {
    f(
        name,
        "GUIStyleState",
        false,
        vec![pptr("m_Background", "Texture2D"), rgbaf("m_TextColor")],
    )
}

fn special_unity(name: &str) -> Option<Vec<F>> {
    Some(match name {
        "Gradient" => {
            let mut v: Vec<F> = (0..8).map(|i| rgbaf(&format!("key{i}"))).collect();
            v.extend((0..8).map(|i| f(&format!("ctime{i}"), "UInt16", false, vec![])));
            v.extend((0..8).map(|i| f(&format!("atime{i}"), "UInt16", false, vec![])));
            v.push(f("m_Mode", "UInt8", false, vec![]));
            v.push(f("m_ColorSpace", "SInt8", false, vec![]));
            v.push(f("m_NumColorKeys", "UInt8", false, vec![]));
            v.push(f("m_NumAlphaKeys", "UInt8", true, vec![]));
            v
        }
        "AnimationCurve" => {
            let mut key = floats(&["time", "value", "inSlope", "outSlope"]);
            key.push(leaf("weightedMode", "int"));
            key.extend(floats(&["inWeight", "outWeight"]));
            vec![
                vector(f("m_Curve", "Keyframe", false, key)),
                leaf("m_PreInfinity", "int"),
                leaf("m_PostInfinity", "int"),
                leaf("m_RotationOrder", "int"),
            ]
        }
        "LayerMask" => vec![leaf("m_Bits", "unsigned int")],
        "Bounds" => vec![
            f("m_Center", "Vector3f", false, floats(&["x", "y", "z"])),
            f("m_Extent", "Vector3f", false, floats(&["x", "y", "z"])),
        ],
        "BoundsInt" => vec![
            f("m_Position", "int3_storage", false, ints(&["x", "y", "z"])),
            f("m_Size", "int3_storage", false, ints(&["x", "y", "z"])),
        ],
        "Rect" => floats(&["x", "y", "width", "height"]),
        "RectOffset" => ints(&["m_Left", "m_Right", "m_Top", "m_Bottom"]),
        "Color32" => vec![leaf("rgba", "unsigned int")],
        "GUIStyle" => {
            let mut v = vec![string("m_Name")];
            for s in [
                "m_Normal",
                "m_Hover",
                "m_Active",
                "m_Focused",
                "m_OnNormal",
                "m_OnHover",
                "m_OnActive",
                "m_OnFocused",
            ] {
                v.push(gui_style_state(s));
            }
            for s in ["m_Border", "m_Margin", "m_Padding", "m_Overflow"] {
                v.push(rect_offset(s));
            }
            v.push(pptr("m_Font", "Font"));
            v.extend(ints(&["m_FontSize", "m_FontStyle", "m_Alignment"]));
            v.push(f("m_WordWrap", "bool", false, vec![]));
            v.push(f("m_RichText", "bool", true, vec![]));
            v.extend(ints(&["m_TextClipping", "m_ImagePosition"]));
            v.push(f("m_ContentOffset", "Vector2f", false, floats(&["x", "y"])));
            v.extend(floats(&["m_FixedWidth", "m_FixedHeight"]));
            v.push(f("m_StretchWidth", "bool", false, vec![]));
            v.push(f("m_StretchHeight", "bool", true, vec![]));
            v
        }
        "Vector2Int" => ints(&["x", "y"]),
        "Vector3Int" => ints(&["x", "y", "z"]),
        "PropertyName" => vec![string("id")],
        "SphericalHarmonicsL2" => (0..27)
            .map(|i| leaf(&format!("sh[{i:2}]"), "float"))
            .collect(),
        _ => return None,
    })
}

/// A type as Mono.Cecil's TypeReference sees it.
#[derive(Clone, Debug)]
enum TRef {
    /// A definition, with arguments when it is a generic instance.
    Def(TypeId, Option<Vec<TRef>>),
    Array(Box<TRef>, bool),
    /// A pointer or by-ref type: resolves to its element, like any Cecil TypeSpecification.
    Wrap(Box<TRef>, &'static str),
    Param(String),
    Unresolved(String),
}

/// TypeDefWithSelfRef: a type with its generic parameter bindings.
#[derive(Clone, Debug)]
struct TD {
    tref: TRef,
    def: Option<TypeId>,
    map: Vec<(String, TD)>,
}

impl TD {
    fn lookup(&self, name: &str) -> Option<&TD> {
        self.map.iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }
    fn set(&mut self, name: String, value: TD) {
        match self.map.iter_mut().find(|(k, _)| *k == name) {
            Some(slot) => slot.1 = value,
            None => self.map.push((name, value)),
        }
    }
}

pub struct Generator {
    managed: Managed,
}

impl Generator {
    pub fn load(managed: &Path, _unity_version: &str) -> Result<Generator> {
        Ok(Generator {
            managed: Managed::new(managed),
        })
    }

    fn name_of(&self, t: &TRef) -> String {
        match t {
            TRef::Def(id, _) => self.managed.name(*id),
            TRef::Array(e, vector) => {
                format!("{}{}", self.name_of(e), if *vector { "[]" } else { "[,]" })
            }
            TRef::Wrap(e, suffix) => format!("{}{suffix}", self.name_of(e)),
            TRef::Param(n) | TRef::Unresolved(n) => n.clone(),
        }
    }

    fn resolve(t: &TRef) -> Option<TypeId> {
        match t {
            TRef::Def(id, _) => Some(*id),
            TRef::Array(e, _) | TRef::Wrap(e, _) => Self::resolve(e),
            _ => None,
        }
    }

    fn td(&self, tref: TRef) -> TD {
        let def = Self::resolve(&tref);
        let mut td = TD {
            tref: tref.clone(),
            def,
            map: Vec::new(),
        };
        let inner = match &tref {
            TRef::Array(e, _) => (**e).clone(),
            other => other.clone(),
        };
        if let (TRef::Def(_, Some(args)), Some(d)) = (&inner, def) {
            let params = self.managed.generic_params(d);
            for (i, arg) in args.iter().enumerate() {
                if let Some(p) = params.get(i) {
                    if td.lookup(p).is_none() {
                        td.map.push((p.clone(), self.td(arg.clone())));
                    }
                }
            }
        }
        td
    }

    fn assign(&self, td: &mut TD, parent: &TD) {
        if parent.map.is_empty() {
            return;
        }
        let (TRef::Def(_, Some(args)), Some(d)) = (td.tref.clone(), td.def) else {
            return;
        };
        let params = self.managed.generic_params(d);
        for (i, arg) in args.iter().enumerate() {
            let Some(p) = params.get(i).cloned() else {
                continue;
            };
            if let TRef::Param(name) = arg {
                if let Some(mapped) = parent.lookup(name) {
                    td.set(p, mapped.clone());
                }
            } else {
                let mut g = self.td(arg.clone());
                self.assign(&mut g, parent);
                td.set(p, g);
            }
        }
    }

    fn solidify(&self, parent: &TD, mut td: TD) -> TD {
        self.assign(&mut td, parent);
        match parent.lookup(&self.name_of(&td.tref)) {
            Some(r) => r.clone(),
            None => td,
        }
    }

    /// A signature type in the context of the type that declares it.
    fn tref(&self, asm: usize, owner: Option<TypeId>, t: &Type) -> TRef {
        match t {
            Type::Builtin(e) => match self.managed.builtin(*e) {
                Some(id) => TRef::Def(id, None),
                None => TRef::Unresolved(hk_dotnet::sig::builtin_name(*e).into()),
            },
            Type::Named {
                table: Table::TypeSpec,
                rid,
                ..
            } => match self.managed.type_spec(asm, *rid) {
                Ok(spec) => self.tref(asm, owner, &spec),
                Err(_) => TRef::Unresolved("?".into()),
            },
            Type::Named { table, rid, .. } => match self.managed.resolve(asm, *table, *rid) {
                Some(id) => TRef::Def(id, None),
                None => TRef::Unresolved(self.row_name(asm, *table, *rid)),
            },
            Type::GenericInst { base, args } => match self.tref(asm, owner, base) {
                TRef::Def(id, _) => TRef::Def(
                    id,
                    Some(args.iter().map(|a| self.tref(asm, owner, a)).collect()),
                ),
                other => other,
            },
            Type::SzArray(e) => TRef::Array(Box::new(self.tref(asm, owner, e)), true),
            Type::Array(e, _) => TRef::Array(Box::new(self.tref(asm, owner, e)), false),
            Type::Var(n) => {
                let names = owner
                    .map(|o| self.managed.generic_params(o))
                    .unwrap_or_default();
                TRef::Param(
                    names
                        .get(*n as usize)
                        .cloned()
                        .unwrap_or_else(|| format!("!{n}")),
                )
            }
            Type::Ptr(e) => TRef::Wrap(Box::new(self.tref(asm, owner, e)), "*"),
            Type::ByRef(e) => TRef::Wrap(Box::new(self.tref(asm, owner, e)), "&"),
            _ => TRef::Unresolved("?".into()),
        }
    }

    fn row_name(&self, asm: usize, table: Table, rid: u32) -> String {
        let l = self.managed.loaded(asm);
        match table {
            Table::TypeRef => l.asm.string(l.asm.get(Table::TypeRef, rid, 1)).to_string(),
            _ => "?".into(),
        }
    }

    fn full_name(&self, id: TypeId) -> String {
        self.managed.full_name(id)
    }

    /// Cecil's BaseType.FullName (generic instances carry their arguments, so
    /// they never equal a plain name); None when the type has no base.
    fn base(&self, id: TypeId) -> Result<Option<(String, TRef)>> {
        let Some(t) = self.managed.base(id).map_err(|e| nerr(&e.0))? else {
            return Ok(None);
        };
        let tref = self.tref(id.asm, Some(id), &t);
        let name = match &tref {
            TRef::Def(d, None) => self.full_name(*d),
            TRef::Def(d, Some(_)) => format!("{}<...>", self.full_name(*d)),
            other => self.name_of(other),
        };
        Ok(Some((name, tref)))
    }

    fn is_interface(&self, id: TypeId) -> bool {
        self.managed.flags(id) & 0x20 != 0
    }
    fn is_abstract(&self, id: TypeId) -> bool {
        self.managed.flags(id) & 0x80 != 0
    }
    fn is_serializable(&self, id: TypeId) -> bool {
        self.managed.flags(id) & 0x2000 != 0
    }
    fn base_is(&self, id: TypeId, full: &str) -> bool {
        matches!(self.base(id), Ok(Some((n, _))) if n == full)
    }
    fn is_enum(&self, id: TypeId) -> bool {
        self.base_is(id, "System.Enum")
    }
    fn is_value_type(&self, id: TypeId) -> bool {
        self.is_enum(id)
            || (self.base_is(id, "System.ValueType") && self.full_name(id) != "System.Enum")
    }
    fn is_primitive(&self, id: TypeId) -> bool {
        self.managed.namespace(id) == "System"
            && self.managed.enclosing(id).is_none()
            && PRIMITIVE_NAMES.contains(&self.managed.name(id).as_str())
    }
    fn enum_underlying(&self, id: TypeId) -> Result<String> {
        for rid in self.managed.fields(id) {
            if self.managed.field_flags(id.asm, rid) & 0x10 == 0 {
                let t = self
                    .managed
                    .field_type(id.asm, rid)
                    .map_err(|e| nerr(&e.0))?;
                return Ok(match self.tref(id.asm, Some(id), &t) {
                    TRef::Def(d, None) => self.full_name(d),
                    other => self.name_of(&other),
                });
            }
        }
        Err(nerr("enum without an instance field"))
    }
    fn derives_from_ue_object(&self, id: TypeId) -> Result<bool> {
        let Some((base, tref)) = self.base(id)? else {
            return Ok(false);
        };
        if self.is_interface(id) {
            return Ok(false);
        }
        if base == "UnityEngine.Object" || self.full_name(id) == "UnityEngine.Object" {
            return Ok(true);
        }
        if base != "System.Object" {
            let next =
                Self::resolve(&tref).ok_or_else(|| nerr(&format!("cannot resolve base {base}")))?;
            return self.derives_from_ue_object(next);
        }
        Ok(false)
    }

    /// The fields after the MonoBehaviour header, as AssetsTools' Read builds them.
    fn read(&self, assembly: &str, full_name: &str) -> Result<(String, Vec<F>)> {
        let asm = self
            .managed
            .assembly(assembly)
            .ok_or_else(|| nerr(&format!("{assembly} not found")))?;
        let (ns, name) = full_name.rsplit_once('.').unwrap_or(("", full_name));
        let mut parts = name.split('/');
        let mut id = self
            .managed
            .find(asm, ns, parts.next().unwrap())
            .ok_or_else(|| nerr(&format!("{full_name} not found")))?;
        for inner in parts {
            id = self
                .managed
                .nested(id, inner)
                .ok_or_else(|| nerr(&format!("{full_name} not found")))?;
        }
        let mut uses_reference = false;
        let mut out = Vec::new();
        let td = self.td(TRef::Def(id, None));
        self.type_load(
            &td,
            &mut out,
            SERIALIZATION_LIMIT,
            true,
            &mut uses_reference,
        )?;
        // A [SerializeReference] field anywhere in the tree adds the registry
        // at the end, but TypeTreeGeneratorAPI 0.0.10 only does so for a root
        // that is a UnityEngine.Object (VisualTreeAsset gets one, the plain
        // serializable TemplateAsset does not). Every MonoBehaviour is one.
        if uses_reference && self.derives_from_ue_object(id)? {
            let referenced = f(
                "data",
                "ReferencedObject",
                false,
                vec![
                    leaf("rid", "SInt64"),
                    f(
                        "type",
                        "ReferencedManagedType",
                        false,
                        vec![string("class"), string("ns"), string("asm")],
                    ),
                    leaf("data", "ReferencedObjectData"),
                ],
            );
            let ref_ids = f(
                "RefIds",
                "vector",
                false,
                vec![f(
                    "Array",
                    "Array",
                    true,
                    vec![leaf("size", "int"), referenced],
                )],
            );
            out.push(f(
                "references",
                "ManagedReferencesRegistry",
                false,
                vec![leaf("version", "int"), ref_ids],
            ));
        }
        Ok((self.managed.name(id), out))
    }

    fn type_load(
        &self,
        td: &TD,
        out: &mut Vec<F>,
        mut depth: i32,
        recursive_call: bool,
        uses_reference: &mut bool,
    ) -> Result<()> {
        if !recursive_call {
            depth -= 1;
        }
        let id = td.def.ok_or_else(|| nerr("type does not resolve"))?;
        let (base, base_ref) = self
            .base(id)?
            .ok_or_else(|| nerr(&format!("{} has no base type", self.full_name(id))))?;
        if !matches!(
            base.as_str(),
            "System.Object"
                | "UnityEngine.Object"
                | "UnityEngine.MonoBehaviour"
                | "UnityEngine.ScriptableObject"
        ) {
            let mut base_td = self.td(base_ref);
            self.assign(&mut base_td, td);
            self.type_load(&base_td, out, depth, true, uses_reference)?;
        }
        out.extend(self.read_types(td, depth, uses_reference)?);
        Ok(())
    }

    fn list_or_array_element(&self, t: &TD) -> Result<Option<TD>> {
        if let TRef::Array(e, _) = &t.tref {
            return Ok(Some(self.td((**e).clone())));
        }
        if let (TRef::Def(_, Some(_)), Some(d)) = (&t.tref, t.def) {
            if self.full_name(d) == "System.Collections.Generic.List`1" {
                return t
                    .lookup("T")
                    .cloned()
                    .map(Some)
                    .ok_or_else(|| nerr("List`1 without T"));
            }
        }
        Ok(None)
    }

    fn field_tref(&self, owner: TypeId, rid: u32) -> Result<TRef> {
        let t = self
            .managed
            .field_type(owner.asm, rid)
            .map_err(|e| nerr(&e.0))?;
        Ok(self.tref(owner.asm, Some(owner), &t))
    }

    fn acceptable_fields(&self, td: &TD, depth: i32) -> Result<Vec<u32>> {
        let id = td.def.ok_or_else(|| nerr("type does not resolve"))?;
        let mut valid = Vec::new();
        for rid in self.managed.fields(id) {
            let flags = self.managed.field_flags(id.asm, rid);
            let attrs = self.managed.field_attributes(id.asm, rid);
            let public = flags & 6 == 6;
            if !(public
                || attrs.iter().any(|(_, full)| {
                    full == "UnityEngine.SerializeField" || full == "UnityEngine.SerializeReference"
                }))
            {
                continue;
            }
            if flags & 0x10 != 0
                || flags & 0x80 != 0
                || flags & 0x20 != 0
                || self.managed.field_has_constant(id.asm, rid)
            {
                continue;
            }
            let mut sft = self.solidify(td, self.td(self.field_tref(id, rid)?));
            if let Some(elem) = self.list_or_array_element(&sft)? {
                if depth < 0 {
                    continue;
                }
                if self.list_or_array_element(&elem)?.is_some() {
                    continue;
                }
                sft = self.solidify(td, elem);
            } else {
                let sdef = sft.def.ok_or_else(|| nerr("field type does not resolve"))?;
                if self.full_name(id) == self.full_name(sdef) && !self.derives_from_ue_object(id)? {
                    continue;
                }
            }
            if let Some(ftd) = sft.def {
                if self.is_valid_def(&attrs, ftd, depth)? {
                    valid.push(rid);
                }
            }
        }
        Ok(valid)
    }

    fn is_valid_def(&self, attrs: &[(String, String)], t: TypeId, depth: i32) -> Result<bool> {
        let full = self.full_name(t);
        if self.is_primitive(t) || full == "System.String" {
            return Ok(true);
        }
        if self.is_enum(t) {
            let u = self.enum_underlying(t)?;
            return Ok(u != "System.Int64" && u != "System.UInt64");
        }
        if depth < 0 {
            return Ok(self.is_value_type(t)
                && (self.is_serializable(t) || SPECIAL.contains(&full.as_str())));
        }
        if self.derives_from_ue_object(t)? || SPECIAL.contains(&full.as_str()) {
            return Ok(true);
        }
        if attrs.iter().any(|(name, _)| name == "SerializeReference") {
            return Ok(!self.is_value_type(t) && self.managed.generic_params(t).is_empty());
        }
        if BLACKLISTED.contains(&self.managed.assembly_name(t.asm).as_str()) {
            return Ok(false);
        }
        Ok(!self.is_abstract(t) && self.is_serializable(t))
    }

    fn read_types(&self, td: &TD, depth: i32, uses_reference: &mut bool) -> Result<Vec<F>> {
        let id = td.def.ok_or_else(|| nerr("type does not resolve"))?;
        let mut children = Vec::new();
        for rid in self.acceptable_fields(td, depth)? {
            let mut ft = self.solidify(td, self.td(self.field_tref(id, rid)?));
            let mut is_array = false;
            if let TRef::Array(e, vector) = ft.tref.clone() {
                is_array = vector;
                if vector {
                    ft = self.solidify(td, self.td(*e));
                }
            } else if ft
                .def
                .is_some_and(|d| self.full_name(d) == "System.Collections.Generic.List`1")
            {
                is_array = true;
                ft = ft
                    .map
                    .first()
                    .map(|(_, v)| v.clone())
                    .ok_or_else(|| nerr("List`1 without an argument"))?;
            }
            let d = ft.def.ok_or_else(|| nerr("field type does not resolve"))?;
            let full = self.full_name(d);
            let attrs = self.managed.field_attributes(id.asm, rid);
            let mut primitive = false;
            let mut derives = false;
            let mut managed_reference = false;
            let ty = if self.is_enum(d) {
                primitive = true;
                base_to_primitive(&self.enum_underlying(d)?)
            } else if self.is_primitive(d) {
                primitive = true;
                base_to_primitive(&full)
            } else if full == "System.String" {
                "string".to_string()
            } else if self.derives_from_ue_object(d)? {
                derives = true;
                format!("PPtr<${}>", self.managed.name(d))
            } else if attrs.iter().any(|(name, _)| name == "SerializeReference") {
                managed_reference = true;
                *uses_reference = true;
                "managedReference".to_string()
            } else {
                self.managed.name(d)
            };
            let kids = if primitive {
                vec![]
            } else if full == "System.String" {
                string_children()
            } else if SPECIAL.contains(&full.as_str()) {
                match special_unity(&self.managed.name(d)) {
                    Some(k) => k,
                    None => {
                        let mut v = Vec::new();
                        self.type_load(&ft, &mut v, depth, false, uses_reference)?;
                        v
                    }
                }
            } else if derives {
                pptr_children()
            } else if managed_reference {
                vec![leaf("rid", "SInt64")]
            } else if self.is_serializable(d) {
                let mut v = Vec::new();
                self.type_load(&ft, &mut v, depth, false, uses_reference)?;
                v
            } else {
                vec![]
            };
            let name = self.managed.field_name(id.asm, rid);
            let field = f(&name, &ty, type_aligns(&ty), kids);
            children.push(if is_array {
                // TypeTreeGeneratorAPI 0.0.10 keeps the element type name for
                // string arrays (host/source.py repairs that when reading).
                if primitive || derives {
                    vector(field)
                } else {
                    vector_with_type(field)
                }
            } else {
                field
            });
        }
        Ok(children)
    }

    /// The whole tree TypeTreeGeneratorAPI returns: root, header, fields.
    pub fn nodes(&self, assembly: &str, full_name: &str) -> Result<Node> {
        let (class, fields) = self.read(assembly, full_name)?;
        let mut rows: Vec<(u32, String, String, u32)> = vec![
            (0, class, "Base".into(), 0),
            (1, "PPtr<GameObject>".into(), "m_GameObject".into(), ALIGN),
            (2, "int".into(), "m_FileID".into(), 0),
            (2, "SInt64".into(), "m_PathID".into(), 0),
            (1, "UInt8".into(), "m_Enabled".into(), 0),
            (1, "PPtr<MonoScript>".into(), "m_Script".into(), ALIGN),
            (2, "int".into(), "m_FileID".into(), 0),
            (2, "SInt64".into(), "m_PathID".into(), 0),
            (1, "string".into(), "m_Name".into(), ALIGN),
        ];
        fn walk(x: &F, level: u32, rows: &mut Vec<(u32, String, String, u32)>) {
            rows.push((
                level,
                x.ty.clone(),
                x.name.clone(),
                if x.aligned { ALIGN } else { 0 },
            ));
            for c in &x.children {
                walk(c, level + 1, rows);
            }
        }
        for x in &fields {
            walk(x, 1, &mut rows);
        }
        let borrowed: Vec<(u32, &str, &str, u32)> = rows
            .iter()
            .map(|(l, t, n, m)| (*l, t.as_str(), n.as_str(), *m))
            .collect();
        Node::from_rows(&borrowed)
    }
}
