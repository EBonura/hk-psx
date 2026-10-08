//! Signature blobs (ECMA-335 II.23.2): compressed integers and types.

use crate::tables::Coded;
use crate::{Error, Result, Table};

pub fn compressed(d: &[u8], p: &mut usize) -> Option<u32> {
    let b0 = *d.get(*p)? as u32;
    if b0 & 0x80 == 0 {
        *p += 1;
        Some(b0)
    } else if b0 & 0xc0 == 0x80 {
        let b1 = *d.get(*p + 1)? as u32;
        *p += 2;
        Some((b0 & 0x3f) << 8 | b1)
    } else {
        let b = d.get(*p..*p + 4)?;
        *p += 4;
        Some((b0 & 0x1f) << 24 | (b[1] as u32) << 16 | (b[2] as u32) << 8 | b[3] as u32)
    }
}

/// A type in a signature. Class and value types name a TypeDef, TypeRef or
/// TypeSpec row of the assembly the signature came from.
#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    /// ELEMENT_TYPE_* for the built-in types (void, bool, char, i1..u8, r4, r8, string, object, i, u, typedref).
    Builtin(u8),
    Named { table: Table, rid: u32, value_type: bool },
    GenericInst { base: Box<Type>, args: Vec<Type> },
    SzArray(Box<Type>),
    Array(Box<Type>, u32),
    Var(u32),
    MVar(u32),
    Ptr(Box<Type>),
    ByRef(Box<Type>),
    FnPtr,
}

pub struct SigReader<'a> {
    pub d: &'a [u8],
    pub p: usize,
}

impl SigReader<'_> {
    fn u(&mut self) -> Result<u32> {
        compressed(self.d, &mut self.p).ok_or_else(|| Error("truncated signature".into()))
    }
    fn byte(&mut self) -> Result<u8> {
        let b = *self.d.get(self.p).ok_or_else(|| Error("truncated signature".into()))?;
        self.p += 1;
        Ok(b)
    }

    pub fn ty(&mut self) -> Result<Type> {
        let e = self.byte()?;
        Ok(match e {
            0x01..=0x0e | 0x16 | 0x18 | 0x19 | 0x1c => Type::Builtin(e),
            0x0f => Type::Ptr(Box::new(self.ty()?)),
            0x10 => Type::ByRef(Box::new(self.ty()?)),
            0x11 | 0x12 => {
                let coded = self.u()?;
                let (table, rid) = Coded::TypeDefOrRef.decode(coded).ok_or_else(|| Error("bad TypeDefOrRef".into()))?;
                Type::Named { table, rid, value_type: e == 0x11 }
            }
            0x13 => Type::Var(self.u()?),
            0x1e => Type::MVar(self.u()?),
            0x14 => {
                let elem = self.ty()?;
                let rank = self.u()?;
                let sizes = self.u()?;
                for _ in 0..sizes {
                    self.u()?;
                }
                let lows = self.u()?;
                for _ in 0..lows {
                    self.u()?;
                }
                Type::Array(Box::new(elem), rank)
            }
            0x15 => {
                let base = self.ty()?;
                let n = self.u()?;
                let mut args = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    args.push(self.ty()?);
                }
                Type::GenericInst { base: Box::new(base), args }
            }
            0x1d => Type::SzArray(Box::new(self.ty()?)),
            0x1b => {
                // Skip a method signature.
                let _conv = self.byte()?;
                let n = self.u()?;
                self.ty()?;
                for _ in 0..n {
                    self.ty()?;
                }
                Type::FnPtr
            }
            0x1f | 0x20 => {
                self.u()?; // custom modifier type, ignored
                return self.ty();
            }
            0x45 => return self.ty(), // pinned
            other => return Err(Error(format!("unsupported element type {other:#x}"))),
        })
    }
}

/// The type of a field signature blob (0x06, then custom modifiers, then the type).
pub fn field_type(blob: &[u8]) -> Result<Type> {
    if blob.first() != Some(&0x06) {
        return Err(Error("not a field signature".into()));
    }
    SigReader { d: blob, p: 1 }.ty()
}

/// System type name of a built-in element type.
pub fn builtin_name(e: u8) -> &'static str {
    match e {
        0x01 => "Void",
        0x02 => "Boolean",
        0x03 => "Char",
        0x04 => "SByte",
        0x05 => "Byte",
        0x06 => "Int16",
        0x07 => "UInt16",
        0x08 => "Int32",
        0x09 => "UInt32",
        0x0a => "Int64",
        0x0b => "UInt64",
        0x0c => "Single",
        0x0d => "Double",
        0x0e => "String",
        0x16 => "TypedReference",
        0x18 => "IntPtr",
        0x19 => "UIntPtr",
        0x1c => "Object",
        _ => "?",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compressed_integers_and_field_types() {
        let mut p = 0;
        assert_eq!(compressed(&[0x03], &mut p), Some(3));
        p = 0;
        assert_eq!(compressed(&[0x80, 0x80], &mut p), Some(0x80));
        p = 0;
        assert_eq!(compressed(&[0xc0, 0x00, 0x40, 0x00], &mut p), Some(0x4000));
        // field sig: List<int>[] -> SZARRAY GENERICINST CLASS TypeRef#2 1 I4
        let t = field_type(&[0x06, 0x1d, 0x15, 0x12, 0x09, 0x01, 0x08]).unwrap();
        assert_eq!(
            t,
            Type::SzArray(Box::new(Type::GenericInst { base: Box::new(Type::Named { table: Table::TypeRef, rid: 2, value_type: false }), args: vec![Type::Builtin(0x08)] }))
        );
    }
}
