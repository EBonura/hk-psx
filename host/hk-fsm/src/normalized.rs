//! Cook-time operand lowering. Graph order/events stay byte-for-byte ordered;
//! each component's variable identities map to disjoint lossless cells.
use hk_format::fsm::{Program, NORMAL_MAGIC};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
pub fn pair(banks: &[crate::Cooked]) -> Result<(Vec<Vec<u8>>, Vec<Vec<u8>>, [i32; 11])> {
    if banks.len() != 2 {
        return Err("normalized pair needs two ordered banks".into());
    }
    let mut maps = Vec::new();
    let mut initial = [0i32; 11];
    let mut counts = [0usize; 3];
    for bank in banks {
        let p = Program::parse(&bank.bytes).map_err(|e| format!("source graph {e:?}"))?;
        let mut map = Vec::new();
        for i in 0..p.variable_count() {
            let kind = p.variable_kind(i).ok_or("variable kind")?;
            let group = match kind {
                2 => 0,
                1 => 1,
                3 => 2,
                _ => return Err("variable type".into()),
            };
            let index = counts[group];
            counts[group] += 1;
            if counts[group] > [4, 5, 2][group] {
                return Err("lossless pair capacity".into());
            }
            let slot = [0, 4, 9][group] + index;
            initial[slot] = p.initial(i).ok_or("initial cell")?;
            if kind == 1 && !(0..=1).contains(&initial[slot])
                || kind == 3 && !(0..=3).contains(&initial[slot])
            {
                return Err("initial typed cell".into());
            }
            map.push(slot as u8);
        }
        maps.push(map);
    }
    if counts != [4, 5, 2] {
        return Err("normalized pair variable layout changed".into());
    }
    let mut lowered = Vec::new();
    for (component, bank) in banks.iter().enumerate() {
        let p = Program::parse(&bank.bytes).map_err(|e| format!("source graph {e:?}"))?;
        let word = |at| u16::from_le_bytes(bank.bytes[at..at + 2].try_into().unwrap());
        let operands = word(14);
        let operand_start = 32
            + p.state_count() as usize * 8
            + p.action_count() as usize * 8
            + word(12) as usize * 4;
        let mut bytes = bank.bytes[..operand_start].to_vec();
        bytes[..8].copy_from_slice(NORMAL_MAGIC);
        bytes[16..18].copy_from_slice(&11u16.to_le_bytes());
        bytes[18..20].copy_from_slice(&0u16.to_le_bytes());
        for i in 0..operands {
            let o = p.operand(i).ok_or("source operand")?;
            let value = match o.kind {
                0 | 3 => {
                    let literal = if o.kind == 0 {
                        p.constant(o.index).ok_or("literal")?
                    } else {
                        o.index as i32
                    };
                    if !(-(1 << 30)..(1 << 30)).contains(&literal) {
                        return Err("normalized signed literal range".into());
                    }
                    literal as u32 & 0x7fffffff
                }
                1 => {
                    0x80000000
                        | *maps[component]
                            .get(o.index as usize)
                            .ok_or("component cell")? as u32
                }
                2 => 0x80010000 | o.index as u32,
                4 => 0x80020000,
                _ => return Err("source operand type".into()),
            };
            bytes.extend(value.to_le_bytes());
        }
        for (i, value) in initial.iter().enumerate() {
            bytes.extend(value.to_le_bytes());
            bytes.extend([
                if i < 4 {
                    2
                } else if i < 9 {
                    1
                } else {
                    3
                },
                0,
                0,
                0,
            ]);
        }
        let len = bytes.len() as u32;
        bytes[28..32].copy_from_slice(&len.to_le_bytes());
        let n = Program::parse(&bytes).map_err(|e| format!("normalized graph {e:?}"))?;
        if n.action_groups()
            .map_err(|e| format!("normalized actions {e:?}"))?
            != p.action_groups()
                .map_err(|e| format!("source actions {e:?}"))?
        {
            return Err("lowering changed kernel admission".into());
        }
        // Every original operand remains independently represented. No literal
        // None is converted to zero and no component aliases another's cell.
        for i in 0..operands {
            let before = p.operand(i).unwrap();
            let after = n.operand(i).unwrap();
            match before.kind {
                0 => {
                    if after.kind != 0 || n.constant(after.index) != p.constant(before.index) {
                        return Err("literal lowering".into());
                    }
                }
                1 => {
                    if after.kind != 1
                        || after.index != maps[component][before.index as usize] as u16
                        || n.variable_kind(after.index) != p.variable_kind(before.index)
                    {
                        return Err("cell lowering".into());
                    }
                }
                2 | 4 => {
                    if before != after {
                        return Err("event/None lowering".into());
                    }
                }
                3 => {
                    if after.kind != 0 || n.constant(after.index) != Some(before.index as i32) {
                        return Err("role lowering".into());
                    }
                }
                _ => unreachable!(),
            }
        }
        lowered.push(bytes);
    }
    Ok((lowered, maps, initial))
}
