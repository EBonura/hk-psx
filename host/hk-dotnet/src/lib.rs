//! Read-only .NET assembly reader: PE sections, the CLI metadata tables and
//! heaps (ECMA-335 partition II), and type signatures. It replaces dnfile and
//! Mono.Cecil for hk-psx's host tools, which only ever read the game's
//! Managed/*.dll; nothing is executed.

pub mod il;
pub mod managed;
pub mod sig;
mod tables;

pub use tables::{Coded, Table};
use std::path::Path;

#[derive(Debug)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;

fn err<T>(msg: impl Into<String>) -> Result<T> {
    Err(Error(msg.into()))
}

pub(crate) fn u16_at(d: &[u8], o: usize) -> Result<u16> {
    d.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]])).ok_or_else(|| Error("truncated assembly".into()))
}
pub(crate) fn u32_at(d: &[u8], o: usize) -> Result<u32> {
    d.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok_or_else(|| Error("truncated assembly".into()))
}

struct Section {
    va: u32,
    size: u32,
    raw: u32,
}

/// One loaded assembly.
pub struct Assembly {
    pub file_name: String,
    data: Vec<u8>,
    sections: Vec<Section>,
    strings: (usize, usize),
    blob: (usize, usize),
    us: (usize, usize),
    pub tables: tables::Tables,
}

impl Assembly {
    pub fn open(path: &Path) -> Result<Assembly> {
        let data = std::fs::read(path).map_err(|e| Error(format!("{}: {e}", path.display())))?;
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Assembly::parse(data, name)
    }

    pub fn parse(data: Vec<u8>, file_name: String) -> Result<Assembly> {
        let pe = u32_at(&data, 0x3c)? as usize;
        if data.get(pe..pe + 4) != Some(b"PE\0\0") {
            return err(format!("{file_name}: not a PE file"));
        }
        let coff = pe + 4;
        let sections_n = u16_at(&data, coff + 2)? as usize;
        let opt_size = u16_at(&data, coff + 16)? as usize;
        let opt = coff + 20;
        let magic = u16_at(&data, opt)?;
        let dirs = opt + if magic == 0x20b { 112 } else { 96 };
        let clr_rva = u32_at(&data, dirs + 14 * 8)?;
        let mut sections = Vec::new();
        let st = opt + opt_size;
        for i in 0..sections_n {
            let s = st + i * 40;
            sections.push(Section { size: u32_at(&data, s + 8)?, va: u32_at(&data, s + 12)?, raw: u32_at(&data, s + 20)? });
        }
        let mut asm = Assembly { file_name, data, sections, strings: (0, 0), blob: (0, 0), us: (0, 0), tables: Default::default() };
        if clr_rva == 0 {
            return err(format!("{}: not a .NET assembly", asm.file_name));
        }
        let cli = asm.offset(clr_rva)?;
        let md = asm.offset(u32_at(&asm.data, cli + 8)?)?;
        if u32_at(&asm.data, md)? != 0x424a_5342 {
            return err(format!("{}: bad metadata signature", asm.file_name));
        }
        let vlen = u32_at(&asm.data, md + 12)? as usize;
        let mut p = md + 16 + vlen;
        let streams = u16_at(&asm.data, p + 2)? as usize;
        p += 4;
        let mut tables_at = None;
        for _ in 0..streams {
            let off = md + u32_at(&asm.data, p)? as usize;
            let size = u32_at(&asm.data, p + 4)? as usize;
            let name_start = p + 8;
            let name_len = asm.data[name_start..].iter().position(|&b| b == 0).ok_or_else(|| Error("bad stream name".into()))?;
            let name = std::str::from_utf8(&asm.data[name_start..name_start + name_len]).unwrap_or("").to_string();
            p = name_start + (name_len + 4) / 4 * 4;
            match name.as_str() {
                "#~" | "#-" => tables_at = Some(off),
                "#Strings" => asm.strings = (off, size),
                "#Blob" => asm.blob = (off, size),
                "#US" => asm.us = (off, size),
                _ => {}
            }
        }
        let at = tables_at.ok_or_else(|| Error("no metadata tables".into()))?;
        asm.tables = tables::Tables::parse(&asm.data, at)?;
        Ok(asm)
    }

    pub fn offset(&self, rva: u32) -> Result<usize> {
        for s in &self.sections {
            if rva >= s.va && rva < s.va + s.size.max(1) {
                return Ok((rva - s.va + s.raw) as usize);
            }
        }
        err(format!("{}: rva {rva:#x} outside every section", self.file_name))
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn string(&self, index: u32) -> &str {
        let (off, size) = self.strings;
        let start = off + index as usize;
        if index as usize >= size {
            return "";
        }
        let end = self.data[start..].iter().position(|&b| b == 0).map_or(self.data.len(), |n| start + n);
        std::str::from_utf8(&self.data[start..end]).unwrap_or("")
    }

    pub fn blob(&self, index: u32) -> &[u8] {
        let (off, size) = self.blob;
        if index as usize >= size {
            return &[];
        }
        let mut p = off + index as usize;
        let len = match sig::compressed(&self.data, &mut p) {
            Some(n) => n as usize,
            None => return &[],
        };
        self.data.get(p..p + len).unwrap_or(&[])
    }

    /// A #US user string (UTF-16), as `ldstr` loads it.
    pub fn user_string(&self, index: u32) -> String {
        let (off, size) = self.us;
        if index as usize >= size {
            return String::new();
        }
        let mut p = off + index as usize;
        let len = sig::compressed(&self.data, &mut p).unwrap_or(0) as usize;
        let bytes = self.data.get(p..p + len.saturating_sub(1)).unwrap_or(&[]);
        let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&units)
    }

    pub fn rows(&self, t: Table) -> u32 {
        self.tables.rows(t)
    }

    /// Column `col` of row `rid` (1-based) of table `t`.
    pub fn get(&self, t: Table, rid: u32, col: usize) -> u32 {
        self.tables.get(&self.data, t, rid, col)
    }

    /// The assembly's own name (Assembly table), e.g. "mscorlib".
    pub fn name(&self) -> &str {
        if self.rows(Table::Assembly) == 0 {
            return "";
        }
        self.string(self.get(Table::Assembly, 1, 7))
    }
}
