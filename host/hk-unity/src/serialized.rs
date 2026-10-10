//! Unity SerializedFile (format 22) metadata: header, types, object table,
//! externals. Object payloads stay in the memory map and are read on demand.

use crate::{Error, Result};
use memmap2::Mmap;
use std::fs::File;
use std::path::Path;

/// A byte cursor over one buffer with Unity's endianness switch.
pub struct Cursor<'a> {
    pub data: &'a [u8],
    pub pos: usize,
    pub big_endian: bool,
}

macro_rules! read_num {
    ($name:ident, $t:ty) => {
        pub fn $name(&mut self) -> Result<$t> {
            const N: usize = std::mem::size_of::<$t>();
            let bytes: [u8; N] = self.take(N)?.try_into().unwrap();
            Ok(if self.big_endian {
                <$t>::from_be_bytes(bytes)
            } else {
                <$t>::from_le_bytes(bytes)
            })
        }
    };
}

impl<'a> Cursor<'a> {
    pub fn new(data: &'a [u8], big_endian: bool) -> Self {
        Cursor {
            data,
            pos: 0,
            big_endian,
        }
    }
    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.data.len())
            .ok_or(Error::Eof)?;
        let out = &self.data[self.pos..end];
        self.pos = end;
        Ok(out)
    }
    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }
    pub fn align(&mut self, n: usize) {
        self.pos += (n - self.pos % n) % n;
    }
    read_num!(u8, u8);
    read_num!(i8, i8);
    read_num!(u16, u16);
    read_num!(i16, i16);
    read_num!(u32, u32);
    read_num!(i32, i32);
    read_num!(u64, u64);
    read_num!(i64, i64);
    read_num!(f32, f32);
    read_num!(f64, f64);
    pub fn cstr(&mut self) -> Result<String> {
        let rest = &self.data[self.pos.min(self.data.len())..];
        let len = rest.iter().position(|&b| b == 0).ok_or(Error::Eof)?;
        self.pos += len + 1;
        Ok(String::from_utf8_lossy(&rest[..len]).into_owned())
    }
}

#[derive(Debug, Clone)]
pub struct SerializedType {
    pub class_id: i32,
    pub script_type_index: i16,
}

#[derive(Debug, Clone, Copy)]
pub struct ObjectInfo {
    pub path_id: i64,
    pub byte_start: usize,
    pub byte_size: usize,
    pub class_id: i32,
    pub type_index: usize,
}

#[derive(Debug, Clone)]
pub struct External {
    pub path: String,
}

pub struct SerializedFile {
    /// Path relative to the data directory, as the reader was asked for it.
    pub name: String,
    data: Mmap,
    pub format: u32,
    pub unity_version: String,
    pub big_endian: bool,
    pub types: Vec<SerializedType>,
    /// In file order, which is the order UnityPy's `objects` dict iterates.
    pub objects: Vec<ObjectInfo>,
    index: std::collections::HashMap<i64, usize>,
    pub externals: Vec<External>,
}

impl SerializedFile {
    pub fn open(path: &Path, name: String) -> Result<Self> {
        let file = File::open(path).map_err(|e| Error::Io(path.display().to_string(), e))?;
        // SAFETY: the install is a read-only build input that nothing rewrites while we run.
        let data =
            unsafe { Mmap::map(&file) }.map_err(|e| Error::Io(path.display().to_string(), e))?;
        Self::parse(data, name)
    }

    fn parse(data: Mmap, name: String) -> Result<Self> {
        let mut c = Cursor::new(&data, true);
        let _metadata_size = c.u32()?;
        let _file_size = c.u32()?;
        let format = c.u32()?;
        let _data_offset = c.u32()?;
        if format != 22 {
            return Err(Error::Format(format!(
                "{name}: serialized format {format}, only 22 is supported"
            )));
        }
        let big_endian = c.u8()? != 0;
        c.take(3)?;
        let _metadata_size = c.u32()?;
        let _file_size = c.i64()?;
        let data_offset = c.i64()? as u64;
        let _unknown = c.i64()?;
        c.big_endian = big_endian;
        let unity_version = c.cstr()?;
        let _platform = c.i32()?;
        let type_trees = c.u8()? != 0;
        if type_trees {
            return Err(Error::Format(format!(
                "{name}: embedded type trees are not supported"
            )));
        }
        let type_count = c.i32()?;
        let mut types = Vec::with_capacity(type_count.max(0) as usize);
        for _ in 0..type_count {
            let class_id = c.i32()?;
            let _stripped = c.u8()?;
            let script_type_index = c.i16()?;
            if class_id == 114 {
                c.take(16)?; // script id
            }
            c.take(16)?; // old type hash
            types.push(SerializedType {
                class_id,
                script_type_index,
            });
        }
        let object_count = c.i32()?;
        let mut objects = Vec::with_capacity(object_count.max(0) as usize);
        let mut index = std::collections::HashMap::with_capacity(object_count.max(0) as usize);
        for _ in 0..object_count {
            c.align(4);
            let path_id = c.i64()?;
            let byte_start = c.i64()? as u64 + data_offset;
            let byte_size = c.u32()? as usize;
            let type_index = c.i32()? as usize;
            let class_id = types
                .get(type_index)
                .ok_or_else(|| Error::Format(format!("{name}: bad type index")))?
                .class_id;
            // A duplicate path id replaces the earlier entry in place, as a dict assignment would.
            match index.get(&path_id) {
                Some(&i) => {
                    objects[i] = ObjectInfo {
                        path_id,
                        byte_start: byte_start as usize,
                        byte_size,
                        class_id,
                        type_index,
                    }
                }
                None => {
                    index.insert(path_id, objects.len());
                    objects.push(ObjectInfo {
                        path_id,
                        byte_start: byte_start as usize,
                        byte_size,
                        class_id,
                        type_index,
                    });
                }
            }
        }
        let script_count = c.i32()?;
        for _ in 0..script_count {
            c.i32()?;
            c.align(4);
            c.i64()?;
        }
        let external_count = c.i32()?;
        let mut externals = Vec::with_capacity(external_count.max(0) as usize);
        for _ in 0..external_count {
            c.cstr()?;
            c.take(16)?;
            c.i32()?;
            externals.push(External { path: c.cstr()? });
        }
        // Reference types (format >= 20) carry no trees here; nothing after them is needed.
        Ok(SerializedFile {
            name,
            data,
            format,
            unity_version,
            big_endian,
            types,
            objects,
            index,
            externals,
        })
    }

    pub fn object(&self, path_id: i64) -> Option<&ObjectInfo> {
        self.index.get(&path_id).map(|&i| &self.objects[i])
    }

    pub fn bytes(&self, info: &ObjectInfo) -> Result<&[u8]> {
        self.data
            .get(info.byte_start..info.byte_start + info.byte_size)
            .ok_or(Error::Eof)
    }
}
