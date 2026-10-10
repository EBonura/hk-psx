//! The #~ stream: table row counts, column layouts (ECMA-335 II.22) and
//! coded indexes (II.24.2.6). Rows stay in the file; columns are decoded on
//! access.

use crate::{err, u16_at, u32_at, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Table {
    Module = 0,
    TypeRef = 1,
    TypeDef = 2,
    FieldPtr = 3,
    Field = 4,
    MethodPtr = 5,
    MethodDef = 6,
    ParamPtr = 7,
    Param = 8,
    InterfaceImpl = 9,
    MemberRef = 10,
    Constant = 11,
    CustomAttribute = 12,
    FieldMarshal = 13,
    DeclSecurity = 14,
    ClassLayout = 15,
    FieldLayout = 16,
    StandAloneSig = 17,
    EventMap = 18,
    EventPtr = 19,
    Event = 20,
    PropertyMap = 21,
    PropertyPtr = 22,
    Property = 23,
    MethodSemantics = 24,
    MethodImpl = 25,
    ModuleRef = 26,
    TypeSpec = 27,
    ImplMap = 28,
    FieldRva = 29,
    EncLog = 30,
    EncMap = 31,
    Assembly = 32,
    AssemblyProcessor = 33,
    AssemblyOs = 34,
    AssemblyRef = 35,
    AssemblyRefProcessor = 36,
    AssemblyRefOs = 37,
    File = 38,
    ExportedType = 39,
    ManifestResource = 40,
    NestedClass = 41,
    GenericParam = 42,
    MethodSpec = 43,
    GenericParamConstraint = 44,
}

const ALL: [Table; 45] = {
    use Table::*;
    [
        Module,
        TypeRef,
        TypeDef,
        FieldPtr,
        Field,
        MethodPtr,
        MethodDef,
        ParamPtr,
        Param,
        InterfaceImpl,
        MemberRef,
        Constant,
        CustomAttribute,
        FieldMarshal,
        DeclSecurity,
        ClassLayout,
        FieldLayout,
        StandAloneSig,
        EventMap,
        EventPtr,
        Event,
        PropertyMap,
        PropertyPtr,
        Property,
        MethodSemantics,
        MethodImpl,
        ModuleRef,
        TypeSpec,
        ImplMap,
        FieldRva,
        EncLog,
        EncMap,
        Assembly,
        AssemblyProcessor,
        AssemblyOs,
        AssemblyRef,
        AssemblyRefProcessor,
        AssemblyRefOs,
        File,
        ExportedType,
        ManifestResource,
        NestedClass,
        GenericParam,
        MethodSpec,
        GenericParamConstraint,
    ]
};

/// Coded index kinds: the tables a tag selects, in tag order (None = unused tag).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Coded {
    TypeDefOrRef,
    HasConstant,
    HasCustomAttribute,
    HasFieldMarshal,
    HasDeclSecurity,
    MemberRefParent,
    HasSemantics,
    MethodDefOrRef,
    MemberForwarded,
    Implementation,
    CustomAttributeType,
    ResolutionScope,
    TypeOrMethodDef,
}

impl Coded {
    pub fn targets(self) -> &'static [Option<Table>] {
        use Table::*;
        match self {
            Coded::TypeDefOrRef => &[Some(TypeDef), Some(TypeRef), Some(TypeSpec)],
            Coded::HasConstant => &[Some(Field), Some(Param), Some(Property)],
            Coded::HasCustomAttribute => &[
                Some(MethodDef),
                Some(Field),
                Some(TypeRef),
                Some(TypeDef),
                Some(Param),
                Some(InterfaceImpl),
                Some(MemberRef),
                Some(Module),
                Some(DeclSecurity),
                Some(Property),
                Some(Event),
                Some(StandAloneSig),
                Some(ModuleRef),
                Some(TypeSpec),
                Some(Assembly),
                Some(AssemblyRef),
                Some(File),
                Some(ExportedType),
                Some(ManifestResource),
                Some(GenericParam),
                Some(GenericParamConstraint),
                Some(MethodSpec),
            ],
            Coded::HasFieldMarshal => &[Some(Field), Some(Param)],
            Coded::HasDeclSecurity => &[Some(TypeDef), Some(MethodDef), Some(Assembly)],
            Coded::MemberRefParent => &[
                Some(TypeDef),
                Some(TypeRef),
                Some(ModuleRef),
                Some(MethodDef),
                Some(TypeSpec),
            ],
            Coded::HasSemantics => &[Some(Event), Some(Property)],
            Coded::MethodDefOrRef => &[Some(MethodDef), Some(MemberRef)],
            Coded::MemberForwarded => &[Some(Field), Some(MethodDef)],
            Coded::Implementation => &[Some(File), Some(AssemblyRef), Some(ExportedType)],
            Coded::CustomAttributeType => &[None, None, Some(MethodDef), Some(MemberRef), None],
            Coded::ResolutionScope => &[
                Some(Module),
                Some(ModuleRef),
                Some(AssemblyRef),
                Some(TypeRef),
            ],
            Coded::TypeOrMethodDef => &[Some(TypeDef), Some(MethodDef)],
        }
    }
    pub fn tag_bits(self) -> u32 {
        let n = self.targets().len() as u32;
        32 - (n - 1).leading_zeros()
    }
    /// Split a coded value into (table, rid).
    pub fn decode(self, value: u32) -> Option<(Table, u32)> {
        let bits = self.tag_bits();
        let tag = (value & ((1 << bits) - 1)) as usize;
        let table = (*self.targets().get(tag)?)?;
        Some((table, value >> bits))
    }
}

#[derive(Clone, Copy)]
enum Col {
    U16,
    U32,
    Str,
    Guid,
    Blob,
    Idx(Table),
    Code(Coded),
}

fn schema(t: Table) -> &'static [Col] {
    use Col::*;
    use Table as T;
    match t {
        T::Module => &[U16, Str, Guid, Guid, Guid],
        T::TypeRef => &[Code(Coded::ResolutionScope), Str, Str],
        T::TypeDef => &[
            U32,
            Str,
            Str,
            Code(Coded::TypeDefOrRef),
            Idx(T::Field),
            Idx(T::MethodDef),
        ],
        T::FieldPtr => &[Idx(T::Field)],
        T::Field => &[U16, Str, Blob],
        T::MethodPtr => &[Idx(T::MethodDef)],
        T::MethodDef => &[U32, U16, U16, Str, Blob, Idx(T::Param)],
        T::ParamPtr => &[Idx(T::Param)],
        T::Param => &[U16, U16, Str],
        T::InterfaceImpl => &[Idx(T::TypeDef), Code(Coded::TypeDefOrRef)],
        T::MemberRef => &[Code(Coded::MemberRefParent), Str, Blob],
        T::Constant => &[U16, Code(Coded::HasConstant), Blob],
        T::CustomAttribute => &[
            Code(Coded::HasCustomAttribute),
            Code(Coded::CustomAttributeType),
            Blob,
        ],
        T::FieldMarshal => &[Code(Coded::HasFieldMarshal), Blob],
        T::DeclSecurity => &[U16, Code(Coded::HasDeclSecurity), Blob],
        T::ClassLayout => &[U16, U32, Idx(T::TypeDef)],
        T::FieldLayout => &[U32, Idx(T::Field)],
        T::StandAloneSig => &[Blob],
        T::EventMap => &[Idx(T::TypeDef), Idx(T::Event)],
        T::EventPtr => &[Idx(T::Event)],
        T::Event => &[U16, Str, Code(Coded::TypeDefOrRef)],
        T::PropertyMap => &[Idx(T::TypeDef), Idx(T::Property)],
        T::PropertyPtr => &[Idx(T::Property)],
        T::Property => &[U16, Str, Blob],
        T::MethodSemantics => &[U16, Idx(T::MethodDef), Code(Coded::HasSemantics)],
        T::MethodImpl => &[
            Idx(T::TypeDef),
            Code(Coded::MethodDefOrRef),
            Code(Coded::MethodDefOrRef),
        ],
        T::ModuleRef => &[Str],
        T::TypeSpec => &[Blob],
        T::ImplMap => &[U16, Code(Coded::MemberForwarded), Str, Idx(T::ModuleRef)],
        T::FieldRva => &[U32, Idx(T::Field)],
        T::EncLog => &[U32, U32],
        T::EncMap => &[U32],
        T::Assembly => &[U32, U16, U16, U16, U16, U32, Blob, Str, Str],
        T::AssemblyProcessor => &[U32],
        T::AssemblyOs => &[U32, U32, U32],
        T::AssemblyRef => &[U16, U16, U16, U16, U32, Blob, Str, Str, Blob],
        T::AssemblyRefProcessor => &[U32, Idx(T::AssemblyRef)],
        T::AssemblyRefOs => &[U32, U32, U32, Idx(T::AssemblyRef)],
        T::File => &[U32, Str, Blob],
        T::ExportedType => &[U32, U32, Str, Str, Code(Coded::Implementation)],
        T::ManifestResource => &[U32, U32, Str, Code(Coded::Implementation)],
        T::NestedClass => &[Idx(T::TypeDef), Idx(T::TypeDef)],
        T::GenericParam => &[U16, U16, Code(Coded::TypeOrMethodDef), Str],
        T::MethodSpec => &[Code(Coded::MethodDefOrRef), Blob],
        T::GenericParamConstraint => &[Idx(T::GenericParam), Code(Coded::TypeDefOrRef)],
    }
}

/// File offset of row 1, row size, and each column's (offset, width).
type Layout = (usize, usize, Vec<(usize, usize)>);

#[derive(Default)]
pub struct Tables {
    rows: Vec<u32>,
    /// Per table: file offset of row 1, row size, column (offset, width) pairs.
    layout: Vec<Layout>,
}

impl Tables {
    pub fn parse(data: &[u8], at: usize) -> Result<Tables> {
        let heaps = data[at + 6];
        let valid = u32_at(data, at + 8)? as u64 | (u32_at(data, at + 12)? as u64) << 32;
        let mut p = at + 24;
        let mut rows = vec![0u32; 64];
        for (i, r) in rows.iter_mut().enumerate() {
            if valid >> i & 1 == 1 {
                *r = u32_at(data, p)?;
                p += 4;
            }
        }
        if valid >> 45 != 0 {
            return err("metadata uses tables past GenericParamConstraint");
        }
        let str_w = if heaps & 1 != 0 { 4 } else { 2 };
        let guid_w = if heaps & 2 != 0 { 4 } else { 2 };
        let blob_w = if heaps & 4 != 0 { 4 } else { 2 };
        let width = |c: Col| -> usize {
            match c {
                Col::U16 => 2,
                Col::U32 => 4,
                Col::Str => str_w,
                Col::Guid => guid_w,
                Col::Blob => blob_w,
                Col::Idx(t) => {
                    if rows[t as usize] < 1 << 16 {
                        2
                    } else {
                        4
                    }
                }
                Col::Code(k) => {
                    let max = k
                        .targets()
                        .iter()
                        .map(|t| t.map_or(0, |t| rows[t as usize]))
                        .max()
                        .unwrap_or(0);
                    if max < 1 << (16 - k.tag_bits()) {
                        2
                    } else {
                        4
                    }
                }
            }
        };
        let mut layout = Vec::with_capacity(ALL.len());
        for t in ALL {
            let mut cols = Vec::new();
            let mut size = 0;
            for &c in schema(t) {
                let w = width(c);
                cols.push((size, w));
                size += w;
            }
            layout.push((p, size, cols));
            p += size * rows[t as usize] as usize;
        }
        Ok(Tables { rows, layout })
    }

    pub fn rows(&self, t: Table) -> u32 {
        self.rows.get(t as usize).copied().unwrap_or(0)
    }

    pub fn get(&self, data: &[u8], t: Table, rid: u32, col: usize) -> u32 {
        let (start, size, cols) = &self.layout[t as usize];
        let (off, w) = cols[col];
        let at = start + (rid as usize - 1) * size + off;
        if w == 2 {
            u16_at(data, at).unwrap_or(0) as u32
        } else {
            u32_at(data, at).unwrap_or(0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coded_index_tags() {
        assert_eq!(Coded::TypeDefOrRef.tag_bits(), 2);
        assert_eq!(Coded::HasCustomAttribute.tag_bits(), 5);
        assert_eq!(Coded::CustomAttributeType.tag_bits(), 3);
        assert_eq!(Coded::MemberForwarded.tag_bits(), 1);
        // TypeRef row 5 as a TypeDefOrRef: (5 << 2) | 1.
        assert_eq!(Coded::TypeDefOrRef.decode(21), Some((Table::TypeRef, 5)));
        assert_eq!(
            Coded::CustomAttributeType.decode((7 << 3) | 3),
            Some((Table::MemberRef, 7))
        );
        assert_eq!(Coded::CustomAttributeType.decode(1), None);
    }
}
