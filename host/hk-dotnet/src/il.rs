//! CIL method bodies (ECMA-335 II.25.4) and instruction decoding (partition III).

use crate::{u16_at, u32_at, Assembly, Error, Result, Table};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Operand {
    None,
    I8(i8),
    U8(u8),
    U16(u16),
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    /// A metadata token (table << 24 | rid), or a #US string token (0x70).
    Token(u32),
    /// Branch target as an absolute IL offset.
    Target(u32),
    /// Number of switch targets (targets are not kept).
    Switch(u32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Instruction {
    pub offset: u32,
    pub name: &'static str,
    pub operand: Operand,
}

#[derive(Clone, Copy)]
enum Kind {
    None,
    I8,
    U8,
    U16,
    I32,
    I64,
    F32,
    F64,
    Token,
    Br8,
    Br32,
    Switch,
}

fn one(op: u8) -> Option<(&'static str, Kind)> {
    use Kind::*;
    Some(match op {
        0x00 => ("nop", None),
        0x01 => ("break", None),
        0x02 => ("ldarg.0", None),
        0x03 => ("ldarg.1", None),
        0x04 => ("ldarg.2", None),
        0x05 => ("ldarg.3", None),
        0x06 => ("ldloc.0", None),
        0x07 => ("ldloc.1", None),
        0x08 => ("ldloc.2", None),
        0x09 => ("ldloc.3", None),
        0x0a => ("stloc.0", None),
        0x0b => ("stloc.1", None),
        0x0c => ("stloc.2", None),
        0x0d => ("stloc.3", None),
        0x0e => ("ldarg.s", U8),
        0x0f => ("ldarga.s", U8),
        0x10 => ("starg.s", U8),
        0x11 => ("ldloc.s", U8),
        0x12 => ("ldloca.s", U8),
        0x13 => ("stloc.s", U8),
        0x14 => ("ldnull", None),
        0x15 => ("ldc.i4.m1", None),
        0x16 => ("ldc.i4.0", None),
        0x17 => ("ldc.i4.1", None),
        0x18 => ("ldc.i4.2", None),
        0x19 => ("ldc.i4.3", None),
        0x1a => ("ldc.i4.4", None),
        0x1b => ("ldc.i4.5", None),
        0x1c => ("ldc.i4.6", None),
        0x1d => ("ldc.i4.7", None),
        0x1e => ("ldc.i4.8", None),
        0x1f => ("ldc.i4.s", I8),
        0x20 => ("ldc.i4", I32),
        0x21 => ("ldc.i8", I64),
        0x22 => ("ldc.r4", F32),
        0x23 => ("ldc.r8", F64),
        0x25 => ("dup", None),
        0x26 => ("pop", None),
        0x27 => ("jmp", Token),
        0x28 => ("call", Token),
        0x29 => ("calli", Token),
        0x2a => ("ret", None),
        0x2b => ("br.s", Br8),
        0x2c => ("brfalse.s", Br8),
        0x2d => ("brtrue.s", Br8),
        0x2e => ("beq.s", Br8),
        0x2f => ("bge.s", Br8),
        0x30 => ("bgt.s", Br8),
        0x31 => ("ble.s", Br8),
        0x32 => ("blt.s", Br8),
        0x33 => ("bne.un.s", Br8),
        0x34 => ("bge.un.s", Br8),
        0x35 => ("bgt.un.s", Br8),
        0x36 => ("ble.un.s", Br8),
        0x37 => ("blt.un.s", Br8),
        0x38 => ("br", Br32),
        0x39 => ("brfalse", Br32),
        0x3a => ("brtrue", Br32),
        0x3b => ("beq", Br32),
        0x3c => ("bge", Br32),
        0x3d => ("bgt", Br32),
        0x3e => ("ble", Br32),
        0x3f => ("blt", Br32),
        0x40 => ("bne.un", Br32),
        0x41 => ("bge.un", Br32),
        0x42 => ("bgt.un", Br32),
        0x43 => ("ble.un", Br32),
        0x44 => ("blt.un", Br32),
        0x45 => ("switch", Switch),
        0x46 => ("ldind.i1", None),
        0x47 => ("ldind.u1", None),
        0x48 => ("ldind.i2", None),
        0x49 => ("ldind.u2", None),
        0x4a => ("ldind.i4", None),
        0x4b => ("ldind.u4", None),
        0x4c => ("ldind.i8", None),
        0x4d => ("ldind.i", None),
        0x4e => ("ldind.r4", None),
        0x4f => ("ldind.r8", None),
        0x50 => ("ldind.ref", None),
        0x51 => ("stind.ref", None),
        0x52 => ("stind.i1", None),
        0x53 => ("stind.i2", None),
        0x54 => ("stind.i4", None),
        0x55 => ("stind.i8", None),
        0x56 => ("stind.r4", None),
        0x57 => ("stind.r8", None),
        0x58 => ("add", None),
        0x59 => ("sub", None),
        0x5a => ("mul", None),
        0x5b => ("div", None),
        0x5c => ("div.un", None),
        0x5d => ("rem", None),
        0x5e => ("rem.un", None),
        0x5f => ("and", None),
        0x60 => ("or", None),
        0x61 => ("xor", None),
        0x62 => ("shl", None),
        0x63 => ("shr", None),
        0x64 => ("shr.un", None),
        0x65 => ("neg", None),
        0x66 => ("not", None),
        0x67 => ("conv.i1", None),
        0x68 => ("conv.i2", None),
        0x69 => ("conv.i4", None),
        0x6a => ("conv.i8", None),
        0x6b => ("conv.r4", None),
        0x6c => ("conv.r8", None),
        0x6d => ("conv.u4", None),
        0x6e => ("conv.u8", None),
        0x6f => ("callvirt", Token),
        0x70 => ("cpobj", Token),
        0x71 => ("ldobj", Token),
        0x72 => ("ldstr", Token),
        0x73 => ("newobj", Token),
        0x74 => ("castclass", Token),
        0x75 => ("isinst", Token),
        0x76 => ("conv.r.un", None),
        0x79 => ("unbox", Token),
        0x7a => ("throw", None),
        0x7b => ("ldfld", Token),
        0x7c => ("ldflda", Token),
        0x7d => ("stfld", Token),
        0x7e => ("ldsfld", Token),
        0x7f => ("ldsflda", Token),
        0x80 => ("stsfld", Token),
        0x81 => ("stobj", Token),
        0x82 => ("conv.ovf.i1.un", None),
        0x83 => ("conv.ovf.i2.un", None),
        0x84 => ("conv.ovf.i4.un", None),
        0x85 => ("conv.ovf.i8.un", None),
        0x86 => ("conv.ovf.u1.un", None),
        0x87 => ("conv.ovf.u2.un", None),
        0x88 => ("conv.ovf.u4.un", None),
        0x89 => ("conv.ovf.u8.un", None),
        0x8a => ("conv.ovf.i.un", None),
        0x8b => ("conv.ovf.u.un", None),
        0x8c => ("box", Token),
        0x8d => ("newarr", Token),
        0x8e => ("ldlen", None),
        0x8f => ("ldelema", Token),
        0x90 => ("ldelem.i1", None),
        0x91 => ("ldelem.u1", None),
        0x92 => ("ldelem.i2", None),
        0x93 => ("ldelem.u2", None),
        0x94 => ("ldelem.i4", None),
        0x95 => ("ldelem.u4", None),
        0x96 => ("ldelem.i8", None),
        0x97 => ("ldelem.i", None),
        0x98 => ("ldelem.r4", None),
        0x99 => ("ldelem.r8", None),
        0x9a => ("ldelem.ref", None),
        0x9b => ("stelem.i", None),
        0x9c => ("stelem.i1", None),
        0x9d => ("stelem.i2", None),
        0x9e => ("stelem.i4", None),
        0x9f => ("stelem.i8", None),
        0xa0 => ("stelem.r4", None),
        0xa1 => ("stelem.r8", None),
        0xa2 => ("stelem.ref", None),
        0xa3 => ("ldelem", Token),
        0xa4 => ("stelem", Token),
        0xa5 => ("unbox.any", Token),
        0xb3 => ("conv.ovf.i1", None),
        0xb4 => ("conv.ovf.u1", None),
        0xb5 => ("conv.ovf.i2", None),
        0xb6 => ("conv.ovf.u2", None),
        0xb7 => ("conv.ovf.i4", None),
        0xb8 => ("conv.ovf.u4", None),
        0xb9 => ("conv.ovf.i8", None),
        0xba => ("conv.ovf.u8", None),
        0xc2 => ("refanyval", Token),
        0xc3 => ("ckfinite", None),
        0xc6 => ("mkrefany", Token),
        0xd0 => ("ldtoken", Token),
        0xd1 => ("conv.u2", None),
        0xd2 => ("conv.u1", None),
        0xd3 => ("conv.i", None),
        0xd4 => ("conv.ovf.i", None),
        0xd5 => ("conv.ovf.u", None),
        0xd6 => ("add.ovf", None),
        0xd7 => ("add.ovf.un", None),
        0xd8 => ("mul.ovf", None),
        0xd9 => ("mul.ovf.un", None),
        0xda => ("sub.ovf", None),
        0xdb => ("sub.ovf.un", None),
        0xdc => ("endfinally", None),
        0xdd => ("leave", Br32),
        0xde => ("leave.s", Br8),
        0xdf => ("stind.i", None),
        0xe0 => ("conv.u", None),
        _ => return Option::None,
    })
}

fn two(op: u8) -> Option<(&'static str, Kind)> {
    use Kind::*;
    Some(match op {
        0x00 => ("arglist", None),
        0x01 => ("ceq", None),
        0x02 => ("cgt", None),
        0x03 => ("cgt.un", None),
        0x04 => ("clt", None),
        0x05 => ("clt.un", None),
        0x06 => ("ldftn", Token),
        0x07 => ("ldvirtftn", Token),
        0x09 => ("ldarg", U16),
        0x0a => ("ldarga", U16),
        0x0b => ("starg", U16),
        0x0c => ("ldloc", U16),
        0x0d => ("ldloca", U16),
        0x0e => ("stloc", U16),
        0x0f => ("localloc", None),
        0x11 => ("endfilter", None),
        0x12 => ("unaligned.", U8),
        0x13 => ("volatile.", None),
        0x14 => ("tail.", None),
        0x15 => ("initobj", Token),
        0x16 => ("constrained.", Token),
        0x17 => ("cpblk", None),
        0x18 => ("initblk", None),
        0x19 => ("no.", U8),
        0x1a => ("rethrow", None),
        0x1c => ("sizeof", Token),
        0x1d => ("refanytype", None),
        0x1e => ("readonly.", None),
        _ => return Option::None,
    })
}

/// The IL bytes of the method body at `rva`.
pub fn body(asm: &Assembly, rva: u32) -> Result<&[u8]> {
    let d = asm.data();
    let at = asm.offset(rva)?;
    let first = *d
        .get(at)
        .ok_or_else(|| Error("truncated method body".into()))?;
    let (start, size) = if first & 3 == 2 {
        (at + 1, (first >> 2) as usize)
    } else {
        let header = (u16_at(d, at)? >> 12) as usize * 4;
        (at + header, u32_at(d, at + 4)? as usize)
    };
    d.get(start..start + size)
        .ok_or_else(|| Error("truncated method body".into()))
}

pub fn decode(code: &[u8]) -> Result<Vec<Instruction>> {
    let mut out = Vec::new();
    let mut p = 0usize;
    let bad = || Error("truncated instruction".into());
    while p < code.len() {
        let offset = p as u32;
        let (name, kind) = if code[p] == 0xfe {
            p += 2;
            two(*code.get(p - 1).ok_or_else(bad)?)
                .ok_or_else(|| Error(format!("unknown opcode fe {:02x}", code[p - 1])))?
        } else {
            p += 1;
            one(code[p - 1]).ok_or_else(|| Error(format!("unknown opcode {:02x}", code[p - 1])))?
        };
        let take = |p: &mut usize, n: usize| -> Result<&[u8]> {
            let s = code.get(*p..*p + n).ok_or_else(bad)?;
            *p += n;
            Ok(s)
        };
        let operand = match kind {
            Kind::None => Operand::None,
            Kind::I8 => Operand::I8(take(&mut p, 1)?[0] as i8),
            Kind::U8 => Operand::U8(take(&mut p, 1)?[0]),
            Kind::U16 => Operand::U16(u16::from_le_bytes(take(&mut p, 2)?.try_into().unwrap())),
            Kind::I32 => Operand::I32(i32::from_le_bytes(take(&mut p, 4)?.try_into().unwrap())),
            Kind::I64 => Operand::I64(i64::from_le_bytes(take(&mut p, 8)?.try_into().unwrap())),
            Kind::F32 => Operand::F32(f32::from_le_bytes(take(&mut p, 4)?.try_into().unwrap())),
            Kind::F64 => Operand::F64(f64::from_le_bytes(take(&mut p, 8)?.try_into().unwrap())),
            Kind::Token => Operand::Token(u32::from_le_bytes(take(&mut p, 4)?.try_into().unwrap())),
            Kind::Br8 => {
                let d = take(&mut p, 1)?[0] as i8 as i64;
                Operand::Target((p as i64 + d) as u32)
            }
            Kind::Br32 => {
                let d = i32::from_le_bytes(take(&mut p, 4)?.try_into().unwrap()) as i64;
                Operand::Target((p as i64 + d) as u32)
            }
            Kind::Switch => {
                let n = u32::from_le_bytes(take(&mut p, 4)?.try_into().unwrap());
                take(&mut p, n as usize * 4)?;
                Operand::Switch(n)
            }
        };
        out.push(Instruction {
            offset,
            name,
            operand,
        });
    }
    Ok(out)
}

impl Assembly {
    /// Methods of the TypeDef rows named `type_name` (any namespace), in table
    /// order: (type rid, method rid, method name, rva).
    pub fn methods_of(&self, type_name: &str) -> Vec<(u32, u32, String, u32)> {
        let types = self.rows(Table::TypeDef);
        let methods = self.rows(Table::MethodDef);
        let mut out = Vec::new();
        for t in 1..=types {
            if self.string(self.get(Table::TypeDef, t, 1)) != type_name {
                continue;
            }
            let start = self.get(Table::TypeDef, t, 5);
            let end = if t < types {
                self.get(Table::TypeDef, t + 1, 5)
            } else {
                methods + 1
            };
            for m in start..end {
                out.push((
                    t,
                    m,
                    self.string(self.get(Table::MethodDef, m, 3)).to_string(),
                    self.get(Table::MethodDef, m, 0),
                ));
            }
        }
        out
    }

    /// The Name column of the row a token names (Field, MethodDef, MemberRef, TypeDef, TypeRef).
    pub fn token_name(&self, token: u32) -> Option<&str> {
        let rid = token & 0x00ff_ffff;
        let col = match token >> 24 {
            0x01 => (Table::TypeRef, 1),
            0x02 => (Table::TypeDef, 1),
            0x04 => (Table::Field, 1),
            0x06 => (Table::MethodDef, 3),
            0x0a => (Table::MemberRef, 1),
            _ => return None,
        };
        if rid == 0 || rid > self.rows(col.0) {
            return None;
        }
        Some(self.string(self.get(col.0, rid, col.1)))
    }
}
