//! What a new save starts from: the constants `PlayerData.SetupNewPlayerData`
//! stores, read from its CIL (host/items.py `playerdata_defaults`).

use crate::{Error, Result};
use hk_dotnet::il::{self, Operand};
use hk_dotnet::Assembly;
use std::collections::HashMap;
use std::path::Path;

/// One field's starting value; `Unknown` when the stored value was not a plain constant.
#[derive(Clone, Debug, PartialEq)]
pub enum Start {
    Int(i64),
    Float(f64),
    Str(String),
    Unknown,
}

const TRACKED: &[&str] = &[
    "charmsOwned", "charmSlots", "charmSlotsFilled", "hasCharm", "overcharmed", "canOvercharm", "salubraNotch1",
    "salubraNotch2", "salubraNotch3", "salubraNotch4", "notchShroomOgres", "notchFogCanyon", "gotGrimmNotch",
    "heartPieces", "heartPieceCollected", "heartPieceMax", "maxHealth", "maxHealthBase", "maxHealthCap",
    "vesselFragments", "vesselFragmentCollected", "MPReserveMax", "nailSmithUpgrades", "nailDamage", "honedNail",
    "trinket1", "trinket2", "trinket3", "trinket4", "foundTrinket1", "foundTrinket2", "foundTrinket3", "foundTrinket4",
    "geo", "simpleKeys", "rancidEggs", "ore", "grubsCollected", "dreamOrbs",
];
const CHARMS: usize = 40;

/// Python's `repr(str)` with the surrounding quote characters stripped the
/// way host/items.py does (`.strip("'")`).
fn ldstr_text(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut out = String::new();
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push(quote);
    out.trim_matches('\'').to_string()
}

pub fn playerdata_defaults(managed: &Path) -> Result<HashMap<String, Start>> {
    let asm = Assembly::open(&managed.join("Assembly-CSharp.dll")).map_err(|e| Error::Format(e.0))?;
    let (_, _, _, rva) = asm
        .methods_of("PlayerData")
        .into_iter()
        .find(|(_, _, name, rva)| name == "SetupNewPlayerData" && *rva != 0)
        .ok_or_else(|| Error::Format("PlayerData::SetupNewPlayerData not found".into()))?;
    let code = il::body(&asm, rva).map_err(|e| Error::Format(e.0))?;
    let mut values = HashMap::new();
    let mut pending = Start::Unknown;
    for ins in il::decode(code).map_err(|e| Error::Format(e.0))? {
        match (ins.name, ins.operand) {
            ("ldc.i4.m1", _) => pending = Start::Int(-1),
            (n, _) if n.starts_with("ldc.i4.") && n.len() == 8 && n.as_bytes()[7].is_ascii_digit() => {
                pending = Start::Int((n.as_bytes()[7] - b'0') as i64)
            }
            ("ldc.i4", Operand::I32(v)) => pending = Start::Int(v as i64),
            ("ldc.i4.s", Operand::I8(v)) => pending = Start::Int(v as i64),
            ("ldc.r4", Operand::F32(v)) => pending = Start::Float(v as f64),
            ("ldstr", Operand::Token(t)) => pending = Start::Str(ldstr_text(&asm.user_string(t & 0x00ff_ffff))),
            ("stfld", Operand::Token(t)) => {
                let name = asm.token_name(t).unwrap_or("").to_string();
                values.insert(name, std::mem::replace(&mut pending, Start::Unknown));
            }
            ("ldarg.0", _) => {}
            _ => pending = Start::Unknown,
        }
    }
    for n in 1..=CHARMS {
        for k in ["gotCharm", "equippedCharm", "newCharm", "charmCost"] {
            if !values.contains_key(&format!("{k}_{n}")) {
                return Err(Error::Format(format!("PlayerData no longer starts {k}_{n}")));
            }
        }
        if !matches!(values[&format!("charmCost_{n}")], Start::Int(c) if c >= 1) {
            return Err(Error::Format("a charm notch cost is not a positive int".into()));
        }
    }
    for f in TRACKED {
        if !values.contains_key(*f) {
            return Err(Error::Format(format!("PlayerData no longer starts {f}")));
        }
    }
    Ok(values)
}
