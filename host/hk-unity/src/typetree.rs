//! Type trees and the value reader.
//!
//! The reader follows UnityPy 1.25.3's `TypeTreeHelper.read_value` exactly,
//! quirks included, because the Python cookers it replaces saw the values that
//! reader produced: an empty or overlong string reads as "" without consuming
//! its bytes, `pair` is a two-element tuple, and on the pure-Python path (which
//! host/source.py forces for MonoBehaviours) aligned arrays of 16-bit integers
//! swap signedness.

use crate::serialized::Cursor;
use crate::value::Value;
use crate::{Error, Result};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

pub const ALIGN: u32 = 0x4000;

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub ty: Arc<str>,
    pub name: Arc<str>,
    pub meta: u32,
    pub children: Vec<Node>,
}

impl Node {
    pub fn aligned(&self) -> bool {
        self.meta & ALIGN != 0
    }

    /// Rebuild a tree from depth-first (level, type, name, meta) rows.
    pub fn from_rows(rows: &[(u32, &str, &str, u32)]) -> Result<Node> {
        fn build(rows: &[(u32, &str, &str, u32)], i: &mut usize) -> Node {
            let (level, ty, name, meta) = rows[*i];
            *i += 1;
            let mut children = Vec::new();
            while *i < rows.len() && rows[*i].0 == level + 1 {
                children.push(build(rows, i));
            }
            Node { ty: ty.into(), name: name.into(), meta, children }
        }
        if rows.is_empty() {
            return Err(Error::Format("empty type tree".into()));
        }
        let mut i = 0;
        let root = build(rows, &mut i);
        if i != rows.len() {
            return Err(Error::Format("type tree rows do not form one tree".into()));
        }
        Ok(root)
    }

    pub fn rows(&self) -> Vec<(u32, String, String, u32)> {
        fn walk(n: &Node, level: u32, out: &mut Vec<(u32, String, String, u32)>) {
            out.push((level, n.ty.to_string(), n.name.to_string(), n.meta));
            for c in &n.children {
                walk(c, level + 1, out);
            }
        }
        let mut out = Vec::new();
        walk(self, 0, &mut out);
        out
    }
}

const BUILTIN: &str = include_str!("../data/typetrees.tsv");

type Row<'a> = (u32, &'a str, &'a str, u32);

/// Built-in class trees keyed by (Unity version, class id); see data/PROVENANCE.md.
pub fn builtin(unity_version: &str, class_id: i32) -> Result<Arc<Node>> {
    static TABLE: OnceLock<HashMap<(String, i32), Arc<Node>>> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut table = HashMap::new();
        let mut key: Option<(String, i32)> = None;
        let mut rows: Vec<Row> = Vec::new();
        let mut flush = |key: &mut Option<(String, i32)>, rows: &mut Vec<Row>| {
            if let Some(k) = key.take() {
                table.insert(k, Arc::new(Node::from_rows(rows).expect("vendored type tree")));
            }
            rows.clear();
        };
        for line in BUILTIN.lines() {
            let f: Vec<&str> = line.split('\t').collect();
            if f[0] == "class" {
                flush(&mut key, &mut rows);
                key = Some((f[1].to_string(), f[2].parse().unwrap()));
            } else {
                rows.push((f[0].parse().unwrap(), f[1], f[2], f[3].parse().unwrap()));
            }
        }
        flush(&mut key, &mut rows);
        table
    });
    table
        .get(&(unity_version.to_string(), class_id))
        .cloned()
        .ok_or_else(|| Error::Format(format!("no built-in type tree for class {class_id} in {unity_version}")))
}

/// Which UnityPy reader produced the values the Python tools saw.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    /// The C extension (UnityPyBoost), used for every non-MonoBehaviour.
    Boost,
    /// The pure-Python reader, which host/source.py forces for MonoBehaviours.
    Python,
}

pub struct Reader<'a> {
    pub c: Cursor<'a>,
    pub flavor: Flavor,
}

impl<'a> Reader<'a> {
    pub fn read(&mut self, node: &Node) -> Result<Value> {
        self.value(node, false)
    }

    fn scalar(&mut self, ty: &str) -> Result<Option<Value>> {
        let c = &mut self.c;
        Ok(Some(match ty {
            "SInt8" => Value::Int(c.i8()? as i64),
            "UInt8" | "char" => Value::Int(c.u8()? as i64),
            "short" | "SInt16" => Value::Int(c.i16()? as i64),
            "unsigned short" | "UInt16" => Value::Int(c.u16()? as i64),
            "int" | "SInt32" => Value::Int(c.i32()? as i64),
            "unsigned int" | "UInt32" | "Type*" => Value::Int(c.u32()? as i64),
            "long long" | "SInt64" => Value::Int(c.i64()?),
            "unsigned long long" | "UInt64" | "FileSize" => Value::UInt(c.u64()?),
            "float" => Value::F32(c.f32()?),
            "double" => Value::F64(c.f64()?),
            "bool" => Value::Bool(c.u8()? != 0),
            "string" => Value::Str(self.aligned_string()?),
            "TypelessData" => {
                let n = c.i32()?;
                Value::Bytes(c.take(n.max(0) as usize)?.to_vec())
            }
            _ => return Ok(None),
        }))
    }

    fn aligned_string(&mut self) -> Result<Vec<u8>> {
        let c = &mut self.c;
        let len = c.i32()?;
        if len > 0 && (len as usize) <= c.remaining() {
            let s = c.take(len as usize)?.to_vec();
            c.align(4);
            Ok(s)
        } else {
            Ok(Vec::new())
        }
    }

    fn value(&mut self, node: &Node, has_registry: bool) -> Result<Value> {
        let mut align = node.aligned();
        let value = if let Some(v) = self.scalar(&node.ty)? {
            v
        } else if &*node.ty == "pair" {
            let a = self.value(&node.children[0], has_registry)?;
            let b = self.value(&node.children[1], has_registry)?;
            Value::List(vec![a, b])
        } else if &*node.ty == "ReferencedObject" {
            return Err(Error::Format("ReferencedObject fields are not supported".into()));
        } else if node.children.first().is_some_and(|c| &*c.ty == "Array") {
            let array = &node.children[0];
            align |= array.aligned();
            let size = self.c.i32()?;
            if size < 0 {
                return Err(Error::Format("Negative length read from TypeTree".into()));
            }
            let sub = &array.children[1];
            if sub.aligned() {
                self.array(sub, size as usize, has_registry)?
            } else {
                let mut items = Vec::with_capacity((size as usize).min(1 << 16));
                for _ in 0..size {
                    items.push(self.value(sub, has_registry)?);
                }
                Value::List(items)
            }
        } else {
            let mut fields = Vec::with_capacity(node.children.len());
            let mut registry = has_registry;
            for child in &node.children {
                if &*child.ty == "ManagedReferencesRegistry" {
                    if registry {
                        continue;
                    }
                    registry = true;
                }
                fields.push((child.name.clone(), self.value(child, registry)?));
            }
            Value::Map(fields)
        };
        if align {
            self.c.align(4);
        }
        Ok(value)
    }

    fn pair_half(&mut self, node: &Node, has_registry: bool) -> Result<Value> {
        match self.scalar(&node.ty)? {
            Some(v) => Ok(v),
            None => self.value(node, has_registry),
        }
    }

    /// `read_value_array`: an array whose element node carries the align flag.
    fn array(&mut self, node: &Node, size: usize, has_registry: bool) -> Result<Value> {
        let mut align = node.aligned();
        let python = self.flavor == Flavor::Python;
        let ty: &str = &node.ty;
        let mut items = Vec::with_capacity(size.min(1 << 16));
        match ty {
            // UnityPy's read_u_short_array unpacks "h" and read_short_array "H".
            "short" | "SInt16" if python => {
                for _ in 0..size {
                    items.push(Value::Int(self.c.u16()? as i64));
                }
            }
            "unsigned short" | "UInt16" if python => {
                for _ in 0..size {
                    items.push(Value::Int(self.c.i16()? as i64));
                }
            }
            "string" | "TypelessData" | "SInt8" | "UInt8" | "char" | "short" | "SInt16" | "unsigned short" | "UInt16" | "int"
            | "SInt32" | "unsigned int" | "UInt32" | "Type*" | "long long" | "SInt64" | "unsigned long long" | "UInt64"
            | "FileSize" | "float" | "double" | "bool" => {
                for _ in 0..size {
                    items.push(self.scalar(ty)?.unwrap());
                }
            }
            "pair" => {
                // Scalar halves go through the bare read function, without
                // their own node's align flag; anything else through read_value.
                for _ in 0..size {
                    let a = self.pair_half(&node.children[0], has_registry)?;
                    let b = self.pair_half(&node.children[1], has_registry)?;
                    items.push(Value::List(vec![a, b]));
                }
            }
            _ if node.children.first().is_some_and(|c| &*c.ty == "Array") => {
                let array = &node.children[0];
                align |= array.aligned();
                let sub = &array.children[1];
                for _ in 0..size {
                    let n = self.c.i32()?;
                    if sub.aligned() {
                        items.push(self.array(sub, n.max(0) as usize, has_registry)?);
                    } else {
                        let mut inner = Vec::new();
                        for _ in 0..n {
                            inner.push(self.value(sub, has_registry)?);
                        }
                        items.push(Value::List(inner));
                    }
                }
            }
            _ => {
                for _ in 0..size {
                    let mut fields = Vec::with_capacity(node.children.len());
                    for child in &node.children {
                        fields.push((child.name.clone(), self.value(child, has_registry)?));
                    }
                    items.push(Value::Map(fields));
                }
            }
        }
        if align {
            self.c.align(4);
        }
        Ok(Value::List(items))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(rows: &[(u32, &str, &str, u32)]) -> Node {
        Node::from_rows(rows).unwrap()
    }

    #[test]
    fn empty_and_overlong_strings_read_as_empty_without_consuming() {
        let n = node(&[(0, "Base", "Base", 0), (1, "string", "a", 0), (1, "int", "b", 0)]);
        // a: length 0, then b = 7
        let data = [0, 0, 0, 0, 7, 0, 0, 0];
        let v = Reader { c: Cursor::new(&data, false), flavor: Flavor::Boost }.read(&n).unwrap();
        assert_eq!(v.get("a"), Some(&Value::Str(vec![])));
        assert_eq!(v.get("b"), Some(&Value::Int(7)));
        // a: length 100 (past the end) reads "" and leaves the bytes for b.
        let data = [100, 0, 0, 0, 9, 0, 0, 0];
        let v = Reader { c: Cursor::new(&data, false), flavor: Flavor::Boost }.read(&n).unwrap();
        assert_eq!(v.get("b"), Some(&Value::Int(9)));
    }

    #[test]
    fn python_flavor_swaps_aligned_u16_array_signedness() {
        let n = node(&[(0, "Base", "Base", 0), (1, "vector", "v", 0), (2, "Array", "Array", 0), (3, "int", "size", 0), (3, "UInt16", "data", ALIGN)]);
        let data = [1, 0, 0, 0, 0xff, 0xff];
        let py = Reader { c: Cursor::new(&data, false), flavor: Flavor::Python }.read(&n).unwrap();
        let boost = Reader { c: Cursor::new(&data, false), flavor: Flavor::Boost }.read(&n).unwrap();
        assert_eq!(py.get("v"), Some(&Value::List(vec![Value::Int(-1)])));
        assert_eq!(boost.get("v"), Some(&Value::List(vec![Value::Int(65535)])));
    }
}
