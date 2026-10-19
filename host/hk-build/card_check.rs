//! `cargo hk-build card-check CARD.mcd...`: what the console's memory card manager
//! sees in a 128 KiB card image.
//!
//! The BIOS card manager (and a game reading through the BIOS) accepts a save
//! only if the directory block and the save's own first frame follow the
//! standard layout, frame by frame, XOR checksums included. PSoXide cannot run
//! a BIOS (its HLE BIOS has no manager screen), so this is the layout rules
//! written out and checked on the bytes the guest's own emulator run left in
//! the `.mcd`. It is evidence about the format, not a substitute for putting
//! the card in a console (docs/SAVE.md says what to look at there).
//!
//! Layout (psx-spx "Memory Card Data Format"; psx-mc/fs.rs writes the same):
//!
//! ```text
//! frame 0        "MC", zeros, [127] = XOR of [0..127)
//! frames 1..=15  one entry per data block: [0] state 51 first, 52 middle, 53 last, A0 free
//!                (A1..A3 deleted), [4..8] size (first block: 0x2000 per block of the file),
//!                [8..10] next block index or FFFF, [10..30] name, [127] XOR
//! frames 16..=35 broken sector list, each FF FF FF FF ... with its XOR
//! frames 36..=63 spare/write-test frames
//! block 1..=15   first block of a file: frame 0 "SC", [2] icon flag 11/12/13, [3] blocks,
//!                [4..68] title (Shift-JIS), [0x60..0x80] 16-colour icon palette; icon frames follow
//! ```
use std::fmt::Write as _;

pub const CARD_SIZE: usize = 128 * 1024;
const FRAME: usize = 128;

#[derive(Debug, PartialEq, Eq)]
pub enum Severity {
    /// The manager would not list or would reject the save or the card.
    Error,
    /// Listed, but not as a retail save would be.
    Warning,
}

#[derive(Debug)]
pub struct Issue {
    pub severity: Severity,
    pub text: String,
}

#[derive(Debug)]
pub struct Save {
    pub block: usize,
    pub name: String,
    pub blocks: usize,
    pub icon_frames: u8,
    pub title: String,
}

#[derive(Debug, Default)]
pub struct Report {
    pub issues: Vec<Issue>,
    pub saves: Vec<Save>,
    pub free_blocks: usize,
}

impl Report {
    pub fn errors(&self) -> impl Iterator<Item = &Issue> {
        self.issues.iter().filter(|i| i.severity == Severity::Error)
    }
    fn error(&mut self, text: String) {
        self.issues.push(Issue { severity: Severity::Error, text });
    }
    fn warn(&mut self, text: String) {
        self.issues.push(Issue { severity: Severity::Warning, text });
    }
    /// The listing a card manager shows, then every issue.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for s in &self.saves {
            let _ = writeln!(
                out,
                "  block {:2}  {:<20} {} block(s), {} icon frame(s)  \"{}\"",
                s.block, s.name, s.blocks, s.icon_frames, s.title
            );
        }
        let _ = writeln!(out, "  {} free block(s)", self.free_blocks);
        for i in &self.issues {
            let _ = writeln!(out, "  {}: {}", if i.severity == Severity::Error { "ERROR" } else { "warning" }, i.text);
        }
        out
    }
}

fn xor(frame: &[u8]) -> u8 {
    frame[..FRAME - 1].iter().fold(0, |a, &b| a ^ b)
}

/// Decode a title the way the manager would show it: double-width Latin back to
/// ASCII, other double-byte characters as `?`, half-width ASCII as is. Returns
/// the text and whether any of it was single-byte (half-width) ASCII.
fn title(bytes: &[u8]) -> Result<(String, bool), String> {
    let mut out = String::new();
    let mut half = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            0 => break,
            0x20..=0x7e => {
                half = true;
                out.push(b as char);
                i += 1;
            }
            0xa1..=0xdf => {
                out.push('?');
                i += 1;
            }
            0x81..=0x9f | 0xe0..=0xfc => {
                let Some(&t) = bytes.get(i + 1) else { return Err("title ends inside a double-byte character".into()) };
                if !(0x40..=0xfc).contains(&t) || t == 0x7f {
                    return Err(format!("invalid Shift-JIS trail byte {t:#04x} after {b:#04x}"));
                }
                out.push(match (b, t) {
                    (0x82, 0x4f..=0x58) => (b'0' + (t - 0x4f)) as char,
                    (0x82, 0x60..=0x79) => (b'A' + (t - 0x60)) as char,
                    (0x82, 0x81..=0x9a) => (b'a' + (t - 0x81)) as char,
                    (0x81, 0x40) => ' ',
                    _ => '?',
                });
                i += 2;
            }
            _ => return Err(format!("invalid Shift-JIS byte {b:#04x} in the title")),
        }
    }
    if bytes[i.min(bytes.len())..].iter().any(|&b| b != 0) {
        return Err("title is not zero padded after its terminator".into());
    }
    Ok((out, half))
}

pub fn check(card: &[u8]) -> Report {
    let mut r = Report::default();
    if card.len() != CARD_SIZE {
        r.error(format!("card image is {} bytes, not {CARD_SIZE}", card.len()));
        return r;
    }
    let frame = |n: usize| &card[n * FRAME..(n + 1) * FRAME];
    let head = frame(0);
    if head[..2] != *b"MC" {
        r.error("frame 0 does not start with \"MC\": the card is not formatted".into());
        return r;
    }
    if xor(head) != head[FRAME - 1] {
        r.error("frame 0 checksum is wrong".into());
    }
    // Directory.
    let mut state = [0u8; 15];
    let mut link = [0u16; 15];
    let mut size = [0u32; 15];
    for i in 0..15 {
        let e = frame(1 + i);
        if xor(e) != e[FRAME - 1] {
            r.error(format!("directory frame {} checksum is wrong", 1 + i));
        }
        state[i] = e[0];
        link[i] = u16::from_le_bytes([e[8], e[9]]);
        size[i] = u32::from_le_bytes([e[4], e[5], e[6], e[7]]);
        let valid = matches!(e[0], 0x51..=0x53) || (0xa0..=0xaf).contains(&e[0]);
        if !valid {
            r.error(format!("directory frame {} has state {:#04x}", 1 + i, e[0]));
        }
    }
    for n in 16..36 {
        let f = frame(n);
        if f[..4] != [0xff; 4] {
            r.warn(format!("broken-sector list frame {n} names a bad sector"));
        }
        if xor(f) != f[FRAME - 1] {
            r.error(format!("broken-sector list frame {n} checksum is wrong"));
        }
    }
    r.free_blocks = state.iter().filter(|s| (0xa0..=0xaf).contains(*s)).count();
    // Chains.
    let mut seen = 0u16;
    for first in 0..15 {
        if state[first] != 0x51 {
            continue;
        }
        let e = frame(1 + first);
        let name_bytes = &e[10..30];
        let name_len = name_bytes.iter().position(|&b| b == 0).unwrap_or(20);
        let name = String::from_utf8_lossy(&name_bytes[..name_len]).into_owned();
        if name_bytes[..name_len].iter().any(|&b| !(0x20..=0x7e).contains(&b)) || name_len == 0 {
            r.error(format!("block {} has a name that is not printable ASCII", first + 1));
        }
        if name_len >= 2 && !matches!(&name[..2], "BA" | "BE" | "BI") {
            r.warn(format!("{name}: region prefix {:?} is not BA, BE or BI", &name[..2]));
        }
        let mut chain = vec![first];
        let mut at = first;
        loop {
            if seen & (1 << at) != 0 {
                r.error(format!("{name}: block {} is reached twice", at + 1));
                break;
            }
            seen |= 1 << at;
            let next = link[at];
            if next == 0xffff {
                if state[at] == 0x52 {
                    r.error(format!("{name}: middle block {} has no link", at + 1));
                }
                break;
            }
            if next as usize >= 15 || !matches!(state[next as usize], 0x52 | 0x53) {
                r.error(format!("{name}: block {} links to {next}, which is not a continuation", at + 1));
                break;
            }
            chain.push(next as usize);
            at = next as usize;
            if chain.len() > 15 {
                r.error(format!("{name}: link chain does not end"));
                break;
            }
        }
        if size[first] as usize != chain.len() * 0x2000 {
            r.error(format!(
                "{name}: directory size {:#x} is not {} block(s) of 0x2000",
                size[first],
                chain.len()
            ));
        }
        // The save's own first frame.
        let sc = frame((first + 1) * 64);
        if sc[..2] != *b"SC" {
            r.error(format!("{name}: block {} frame 0 does not start with \"SC\"", first + 1));
            continue;
        }
        let icon_frames = match sc[2] {
            0x11 => 1,
            0x12 => 2,
            0x13 => 3,
            other => {
                r.error(format!("{name}: icon flag {other:#04x} is not 0x11, 0x12 or 0x13"));
                0
            }
        };
        if sc[3] as usize != chain.len() {
            r.error(format!("{name}: SC header says {} block(s), the chain has {}", sc[3], chain.len()));
        }
        let text = match title(&sc[4..68]) {
            Ok((text, half)) => {
                if half {
                    r.warn(format!(
                        "{name}: title \"{text}\" has half-width ASCII; retail saves use double-width Shift-JIS"
                    ));
                }
                text
            }
            Err(why) => {
                r.error(format!("{name}: {why}"));
                String::new()
            }
        };
        if sc[0x60..0x80].iter().all(|&b| b == 0) {
            r.warn(format!("{name}: icon palette is all zero, the icon will be invisible"));
        }
        r.saves.push(Save { block: first + 1, name, blocks: chain.len(), icon_frames, title: text });
    }
    for i in 0..15 {
        if matches!(state[i], 0x52 | 0x53) && seen & (1 << i) == 0 {
            r.error(format!("block {} is a continuation no file owns", i + 1));
        }
    }
    r
}

/// `cargo hk-build card-check FILE...`: print each card's listing, exit 1 on any error.
pub fn run(files: &[String]) -> Result<(), String> {
    let mut failed = false;
    for file in files {
        let bytes = std::fs::read(file).map_err(|e| format!("{file}: {e}"))?;
        let report = check(&bytes);
        println!("{file}");
        print!("{}", report.render());
        failed |= report.errors().next().is_some();
    }
    if failed {
        Err("a card failed the layout check".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_xor(card: &mut [u8], frame: usize) {
        let f = &mut card[frame * FRAME..(frame + 1) * FRAME];
        f[FRAME - 1] = xor(f);
    }
    /// A freshly formatted card, as the BIOS leaves it.
    fn formatted() -> Vec<u8> {
        let mut c = vec![0u8; CARD_SIZE];
        c[0] = b'M';
        c[1] = b'C';
        set_xor(&mut c, 0);
        for i in 1..=15 {
            let f = &mut c[i * FRAME..(i + 1) * FRAME];
            f[0] = 0xa0;
            f[8] = 0xff;
            f[9] = 0xff;
            set_xor(&mut c, i);
        }
        for n in 16..36 {
            let f = &mut c[n * FRAME..(n + 1) * FRAME];
            f[..4].fill(0xff);
            f[8] = 0xff;
            f[9] = 0xff;
            set_xor(&mut c, n);
        }
        c
    }
    const SJIS_TITLE: [u8; 12] = [0x82, 0x67, 0x82, 0x6e, 0x82, 0x6b, 0x81, 0x40, 0x82, 0x6a, 0x82, 0x4f];
    /// One save in block `at` (1..=15), `blocks` long when `at..at+blocks` are consecutive.
    fn save(c: &mut Vec<u8>, at: usize, blocks: usize, title: &[u8]) {
        for k in 0..blocks {
            let i = at + k;
            let f = &mut c[i * FRAME..(i + 1) * FRAME];
            f.fill(0);
            f[0] = if k == 0 { 0x51 } else if k + 1 == blocks { 0x53 } else { 0x52 };
            if k == 0 {
                f[4..8].copy_from_slice(&((blocks * 0x2000) as u32).to_le_bytes());
                f[10..29].copy_from_slice(b"BASLUS-00000HKPSX00");
            }
            let next = if k + 1 < blocks { (i + 1 - 1) as u16 } else { 0xffff };
            f[8..10].copy_from_slice(&next.to_le_bytes());
            set_xor(c, i);
        }
        let b = (at) * 64 * FRAME;
        c[b] = b'S';
        c[b + 1] = b'C';
        c[b + 2] = 0x11;
        c[b + 3] = blocks as u8;
        c[b + 4..b + 4 + title.len()].copy_from_slice(title);
        c[b + 0x60] = 0xff;
        c[b + 0x61] = 0x7f;
    }

    #[test]
    fn a_formatted_card_is_clean_and_empty() {
        let r = check(&formatted());
        assert!(r.issues.is_empty(), "{:?}", r.issues);
        assert_eq!((r.saves.len(), r.free_blocks), (0, 15));
    }

    #[test]
    fn a_double_width_titled_save_lists_as_the_manager_would() {
        let mut c = formatted();
        save(&mut c, 1, 1, &SJIS_TITLE);
        let r = check(&c);
        assert!(r.issues.is_empty(), "{:?}", r.issues);
        assert_eq!(r.saves.len(), 1);
        assert_eq!((r.saves[0].block, r.saves[0].name.as_str(), r.saves[0].title.as_str()), (1, "BASLUS-00000HKPSX00", "HOL K0"));
        assert_eq!(r.free_blocks, 14);
    }

    #[test]
    fn a_half_width_title_warns_but_lists() {
        let mut c = formatted();
        save(&mut c, 1, 1, b"HOLLOW KNIGHT PSX");
        let r = check(&c);
        assert_eq!(r.errors().count(), 0);
        assert!(r.issues.iter().any(|i| i.severity == Severity::Warning && i.text.contains("half-width")));
        assert_eq!(r.saves[0].title, "HOLLOW KNIGHT PSX");
    }

    #[test]
    fn what_the_manager_refuses_is_an_error() {
        // A save whose directory size does not match its chain.
        let mut c = formatted();
        save(&mut c, 1, 1, &SJIS_TITLE);
        c[FRAME + 4] = 0x00;
        c[FRAME + 5] = 0x40;
        set_xor(&mut c, 1);
        assert!(check(&c).errors().any(|i| i.text.contains("directory size")));
        // A flipped directory byte the checksum catches.
        let mut c = formatted();
        save(&mut c, 2, 1, &SJIS_TITLE);
        c[2 * FRAME + 11] ^= 1;
        assert!(check(&c).errors().any(|i| i.text.contains("checksum")));
        // No SC header.
        let mut c = formatted();
        save(&mut c, 1, 1, &SJIS_TITLE);
        c[64 * FRAME] = 0;
        assert!(check(&c).errors().any(|i| i.text.contains("SC")));
        // Not formatted at all.
        assert!(check(&vec![0u8; CARD_SIZE]).errors().any(|i| i.text.contains("not formatted")));
        assert!(check(&vec![0xffu8; CARD_SIZE]).errors().count() > 0);
        // An orphan continuation block.
        let mut c = formatted();
        c[3 * FRAME] = 0x53;
        set_xor(&mut c, 3);
        assert!(check(&c).errors().any(|i| i.text.contains("no file owns")));
        // A title that is not Shift-JIS.
        let mut c = formatted();
        save(&mut c, 1, 1, &[0x82, 0x20]);
        assert!(check(&c).errors().any(|i| i.text.contains("Shift-JIS")));
    }

    #[test]
    fn a_two_block_chain_checks_out() {
        let mut c = formatted();
        save(&mut c, 3, 2, &SJIS_TITLE);
        let r = check(&c);
        assert!(r.issues.is_empty(), "{:?}", r.issues);
        assert_eq!(r.saves[0].blocks, 2);
    }

    /// Every committed card fixture was written by the filesystem layer the
    /// shipping game uses, so each has to be a card the manager would list.
    /// Their titles are the old half-width ASCII (a warning), never an error.
    #[test]
    fn every_committed_card_fixture_has_a_valid_layout() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/cards");
        let mut checked = 0;
        for entry in std::fs::read_dir(dir).expect("tools/cards").flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "mcd") {
                continue;
            }
            let report = check(&std::fs::read(&path).unwrap());
            assert_eq!(report.errors().count(), 0, "{}: {:?}", path.display(), report.issues);
            checked += 1;
        }
        assert!(checked > 10, "{checked} fixtures");
    }
}
