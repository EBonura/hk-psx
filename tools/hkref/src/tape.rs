//! PXITAPE2 poll-bound input tapes (one 6-byte sample per port-1 poll) and the
//! original-game input CSV (`test_frame,buttons`, change rows only).

use std::fs;
use std::path::Path;

pub const MAGIC: &[u8; 8] = b"PXITAPE2";

/// Active-high PS1 button bits, as the port and the reference driver use them.
pub const BUTTONS: &[(&str, u16)] = &[
    ("select", 0x0001),
    ("start", 0x0008),
    ("up", 0x0010),
    ("right", 0x0020),
    ("down", 0x0040),
    ("left", 0x0080),
    ("l2", 0x0100),
    ("r2", 0x0200),
    ("l1", 0x0400),
    ("r1", 0x0800),
    ("triangle", 0x1000),
    ("circle", 0x2000),
    ("cross", 0x4000),
    ("square", 0x8000),
];

/// The masks the original-side virtual device maps (Start, D-pad, Circle=cast,
/// Cross=jump, Square=attack).
pub const ORIGINAL_MASK: u16 = 0x0008 | 0x00F0 | 0x2000 | 0x4000 | 0x8000;

pub fn read(path: &Path) -> Result<Vec<u16>, String> {
    let d = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if d.len() < 16 || &d[..8] != MAGIC {
        return Err(format!("{}: not a PXITAPE2 tape", path.display()));
    }
    let count = u32::from_le_bytes(d[8..12].try_into().unwrap()) as usize;
    if d.len() != 16 + count * 6 {
        return Err(format!("{}: size/count mismatch", path.display()));
    }
    Ok((0..count)
        .map(|i| u16::from_le_bytes([d[16 + i * 6], d[17 + i * 6]]))
        .collect())
}

pub fn write(path: &Path, masks: &[u16]) -> Result<(), String> {
    let mut d = Vec::with_capacity(16 + masks.len() * 6);
    d.extend_from_slice(MAGIC);
    d.extend_from_slice(&(masks.len() as u32).to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes());
    for m in masks {
        d.extend_from_slice(&m.to_le_bytes());
        d.extend_from_slice(&[128, 128, 128, 128]);
    }
    fs::write(path, d).map_err(|e| format!("{}: {e}", path.display()))
}

/// `poll:button:hold,...` (the tools/validate.py route syntax). Buttons may be
/// joined with `+` (e.g. `right+cross`).
pub fn from_events(events: &str, count: usize) -> Result<Vec<u16>, String> {
    let mut s = vec![0u16; count];
    for ev in events.split(',').map(str::trim).filter(|e| !e.is_empty()) {
        let p: Vec<&str> = ev.split(':').collect();
        if p.len() != 3 {
            return Err(format!("bad event '{ev}'"));
        }
        let (poll, hold): (usize, usize) = (
            p[0].parse().map_err(|_| format!("bad poll in '{ev}'"))?,
            p[2].parse().map_err(|_| format!("bad hold in '{ev}'"))?,
        );
        if hold < 1 || poll + hold > count {
            return Err(format!("event '{ev}' outside the tape"));
        }
        let mut mask = 0u16;
        for name in p[1].split('+') {
            mask |= BUTTONS
                .iter()
                .find(|(n, _)| *n == name)
                .ok_or(format!("unknown button '{name}'"))?
                .1;
        }
        for m in &mut s[poll..poll + hold] {
            *m |= mask;
        }
    }
    Ok(s)
}

/// Change-compressed original input rows for the window `[from, from+len)`.
/// Frame 0 is always written. Buttons the original driver cannot press are
/// reported through `unmapped` and dropped.
pub fn window_rows(
    masks: &[u16],
    from: usize,
    len: usize,
    unmapped: &mut u16,
) -> Vec<(usize, u16)> {
    let mut rows = Vec::new();
    let mut prev: Option<u16> = None;
    for f in 0..len {
        let m = masks.get(from + f).copied().unwrap_or(0);
        *unmapped |= m & !ORIGINAL_MASK;
        let m = m & ORIGINAL_MASK;
        if prev != Some(m) {
            rows.push((f, m));
            prev = Some(m);
        }
    }
    rows
}

pub fn rows_to_csv(rows: &[(usize, u16)]) -> String {
    let mut s = String::from("test_frame,buttons\n");
    for (f, m) in rows {
        s.push_str(&format!("{f},{m}\n"));
    }
    s
}

pub fn describe(masks: &[u16]) -> String {
    let mut out = String::new();
    let mut prev = 0u16;
    for (i, &m) in masks.iter().enumerate() {
        if m != prev {
            let names: Vec<&str> = BUTTONS
                .iter()
                .filter(|(_, b)| m & b != 0)
                .map(|(n, _)| *n)
                .collect();
            out.push_str(&format!(
                "{i}: {}\n",
                if names.is_empty() {
                    "-".into()
                } else {
                    names.join("+")
                }
            ));
            prev = m;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_round_trip_through_a_tape_and_window_rows() {
        let m = from_events("2:right:3,4:square:1,7:right+cross:2", 12).unwrap();
        assert_eq!(m[2], 0x20);
        assert_eq!(m[4], 0x20 | 0x8000);
        assert_eq!(m[7], 0x20 | 0x4000);
        let mut unmapped = 0;
        let rows = window_rows(&m, 2, 8, &mut unmapped);
        assert_eq!(rows[0], (0, 0x20));
        assert_eq!(unmapped, 0);
        assert_eq!(
            rows_to_csv(&rows).lines().next().unwrap(),
            "test_frame,buttons"
        );
    }

    #[test]
    fn unbound_buttons_are_reported_and_dropped() {
        let m = from_events("0:r1:2", 4).unwrap();
        let mut unmapped = 0;
        let rows = window_rows(&m, 0, 4, &mut unmapped);
        assert_eq!(unmapped, 0x0800);
        assert!(rows.iter().all(|(_, mask)| *mask == 0));
    }
}
