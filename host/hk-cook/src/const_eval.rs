//! Reading `pub const NAME: type = value;` out of a guest module the way host/mawlek_art.py
//! and host/false_knight_art.py do: every match of the declaration pattern, comments dropped,
//! `ONE` replaced by 65536, and the expression evaluated as Python would evaluate it. A
//! declaration whose value is not a plain numeric expression is skipped.

use crate::common::{err, Result};

#[derive(Clone, Debug, PartialEq)]
pub enum Num {
    Int(i128),
    Float(f64),
    List(Vec<Num>),
}

impl Num {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Num::Int(i) => Some(*i as f64),
            Num::Float(f) => Some(*f),
            Num::List(_) => None,
        }
    }

    /// Python `==` against an integer.
    pub fn eq_int(&self, v: i64) -> bool {
        self.as_f64() == Some(v as f64)
    }

    /// Python `==` against a list of integers.
    pub fn eq_ints(&self, v: &[i64]) -> bool {
        matches!(self, Num::List(l) if l.len() == v.len() && l.iter().zip(v).all(|(a, b)| a.eq_int(*b)))
    }
}

struct Parser<'a> {
    t: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.at < self.t.len() && self.t[self.at].is_ascii_whitespace() {
            self.at += 1;
        }
    }

    fn eat(&mut self, s: &str) -> bool {
        self.ws();
        if self.t[self.at..].starts_with(s.as_bytes()) {
            self.at += s.len();
            true
        } else {
            false
        }
    }

    fn bin(&mut self, level: usize) -> Result<Num> {
        const OPS: [&[&str]; 6] = [
            &["|"],
            &["^"],
            &["&"],
            &["<<", ">>"],
            &["+", "-"],
            &["//", "*", "/", "%"],
        ];
        if level == OPS.len() {
            return self.unary();
        }
        let mut left = self.bin(level + 1)?;
        'outer: loop {
            self.ws();
            for op in OPS[level] {
                // `*` must not swallow `**`, and `/` must not swallow `//`.
                if self.t[self.at..].starts_with(op.as_bytes())
                    && !(*op == "*" && self.t[self.at..].starts_with(b"**"))
                    && !(*op == "/" && self.t[self.at..].starts_with(b"//"))
                {
                    self.at += op.len();
                    let right = self.bin(level + 1)?;
                    left = apply(op, left, right)?;
                    continue 'outer;
                }
            }
            return Ok(left);
        }
    }

    fn unary(&mut self) -> Result<Num> {
        if self.eat("-") {
            return match self.unary()? {
                Num::Int(i) => Ok(Num::Int(-i)),
                Num::Float(f) => Ok(Num::Float(-f)),
                Num::List(_) => err("not a number"),
            };
        }
        if self.eat("+") {
            return self.unary();
        }
        self.atom()
    }

    fn atom(&mut self) -> Result<Num> {
        self.ws();
        if self.eat("(") {
            let v = self.bin(0)?;
            if !self.eat(")") {
                return err("expected )");
            }
            return Ok(v);
        }
        if self.eat("[") {
            let mut items = Vec::new();
            if self.eat("]") {
                return Ok(Num::List(items));
            }
            loop {
                items.push(self.bin(0)?);
                if self.eat("]") {
                    return Ok(Num::List(items));
                }
                if !self.eat(",") {
                    return err("expected ,");
                }
                if self.eat("]") {
                    return Ok(Num::List(items));
                }
            }
        }
        let start = self.at;
        while self.at < self.t.len()
            && (self.t[self.at].is_ascii_alphanumeric()
                || self.t[self.at] == b'_'
                || self.t[self.at] == b'.')
        {
            self.at += 1;
        }
        let word = std::str::from_utf8(&self.t[start..self.at])
            .unwrap()
            .replace('_', "");
        if word.is_empty() {
            return err("expected a value");
        }
        if let Ok(i) = word.parse::<i128>() {
            return Ok(Num::Int(i));
        }
        if let Some(hex) = word.strip_prefix("0x") {
            if let Ok(i) = i128::from_str_radix(hex, 16) {
                return Ok(Num::Int(i));
            }
        }
        word.parse::<f64>()
            .map(Num::Float)
            .map_err(|_| "not a numeric literal".to_string())
    }
}

fn apply(op: &str, a: Num, b: Num) -> Result<Num> {
    let (Some(x), Some(y)) = (a.as_f64(), b.as_f64()) else {
        return err("list arithmetic");
    };
    if let (Num::Int(p), Num::Int(q)) = (&a, &b) {
        let (p, q) = (*p, *q);
        return Ok(match op {
            "+" => Num::Int(p + q),
            "-" => Num::Int(p - q),
            "*" => Num::Int(p * q),
            "/" => Num::Float(x / y),
            "//" if q != 0 => Num::Int(p.div_euclid(q)),
            "%" if q != 0 => Num::Int(p.rem_euclid(q)),
            "<<" if (0..100).contains(&q) => Num::Int(p << q),
            ">>" if (0..100).contains(&q) => Num::Int(p >> q),
            "&" => Num::Int(p & q),
            "|" => Num::Int(p | q),
            "^" => Num::Int(p ^ q),
            _ => return err("unsupported operation"),
        });
    }
    Ok(Num::Float(match op {
        "+" => x + y,
        "-" => x - y,
        "*" => x * y,
        "/" => x / y,
        _ => return err("unsupported operation"),
    }))
}

/// `eval(raw.replace('ONE', '65536'))`.
fn evaluate(raw: &str) -> Result<Num> {
    let text = raw.replace("ONE", "65536");
    let mut p = Parser {
        t: text.as_bytes(),
        at: 0,
    };
    let v = p.bin(0)?;
    p.ws();
    if p.at != p.t.len() {
        return err("trailing input");
    }
    Ok(v)
}

/// `rust_constants(path)`: every constant of a module that evaluates, in file order.
pub fn rust_constants(text: &str) -> Vec<(String, Num)> {
    let mut found: Vec<(String, Num)> = Vec::new();
    let bytes = text.as_bytes();
    let mut from = 0;
    while let Some(rel) = text[from..].find("pub const ") {
        let start = from + rel + "pub const ".len();
        from = start;
        // (\w+)\s*:\s*[^=]+=\s*([^;]+);
        let mut i = start;
        while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
            i += 1;
        }
        if i == start {
            continue;
        }
        let name = &text[start..i];
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b':' {
            continue;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let ty_start = i;
        while i < bytes.len() && bytes[i] != b'=' {
            i += 1;
        }
        if i == ty_start || i >= bytes.len() {
            continue;
        }
        i += 1;
        let expr_start = i;
        while i < bytes.len() && bytes[i] != b';' {
            i += 1;
        }
        if i == expr_start || i >= bytes.len() {
            continue;
        }
        let raw: String = text[expr_start..i]
            .lines()
            .map(|l| l.split("//").next().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n");
        if let Ok(v) = evaluate(raw.trim()) {
            match found.iter_mut().find(|f| f.0 == name) {
                Some(slot) => slot.1 = v,
                None => found.push((name.to_string(), v)),
            }
        }
        from = i;
    }
    found
}
