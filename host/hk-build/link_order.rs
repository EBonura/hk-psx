//! The guest's committed link order (`game/hot-text-order.txt`), kept alive across repins.
//!
//! The list names functions by their mangled symbol, which carries the crate disambiguator
//! (`Cs<hash>_`) of every crate in the path, and those hashes change with the dependency graph, the
//! features (a twin build has other hashes than the ship) and the SDK pin. So the order lives in the
//! file as a sequence of hash-free names, each written once per hash it is known under, and
//! `cargo hk-build link-order --map MAP...` writes it again from the link maps of the builds that
//! should follow it: for every name, every mangled variant found in any of the maps, in the order
//! of the file. A name that no map contains is dropped and listed.

use std::path::Path;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// The symbol with every crate disambiguator (`Cs...`/`Cd...` up to its `_`) replaced by `C_`.
pub fn normalize(name: &str) -> String {
    let bytes = name.as_bytes();
    let mut out = String::with_capacity(name.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'C' && i + 1 < bytes.len() && (bytes[i + 1] == b's' || bytes[i + 1] == b'd') {
            let mut j = i + 2;
            while j < bytes.len() && bytes[j].is_ascii_alphanumeric() {
                j += 1;
            }
            if j > i + 3 && j < bytes.len() && bytes[j] == b'_' {
                out.push_str("C_");
                i = j + 1;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

/// The `.text.<mangled>` input sections of an LLD map, in address order.
pub fn text_sections(map: &Path) -> Result<Vec<(u32, String)>> {
    let mut sections = Vec::new();
    for line in std::fs::read_to_string(map)?.lines() {
        let Some(start) = line.find(":(.text.") else { continue };
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 4 {
            continue;
        }
        let (Ok(address), Ok(size)) = (u32::from_str_radix(fields[0], 16), u32::from_str_radix(fields[2], 16)) else {
            continue;
        };
        if size == 0 {
            continue;
        }
        sections.push((address, line[start + 8..].trim_end_matches(')').to_string()));
    }
    sections.sort();
    Ok(sections)
}

/// The order file's symbols (comments and blanks left out).
pub fn listed(text: &str) -> Vec<&str> {
    text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).collect()
}

/// `link-order --map MAP...`: write `game/hot-text-order.txt` again for these maps.
pub fn regenerate(root: &Path, maps: &[std::path::PathBuf]) -> Result<()> {
    if maps.is_empty() {
        return Err("link-order needs at least one --map".into());
    }
    let path = root.join("game/hot-text-order.txt");
    let old = std::fs::read_to_string(&path)?;
    let mut names: Vec<String> = Vec::new();
    for symbol in listed(&old) {
        let n = normalize(symbol);
        if !names.contains(&n) {
            names.push(n);
        }
    }
    let mut variants: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    for map in maps {
        for (_, section) in text_sections(map)? {
            let n = normalize(&section);
            let entry = variants.entry(n).or_default();
            if !entry.contains(&section) {
                entry.push(section);
            }
        }
    }
    let header: Vec<&str> = old.lines().take_while(|l| l.trim_start().starts_with('#')).collect();
    let mut out = String::new();
    for h in &header {
        out.push_str(h);
        out.push('\n');
    }
    let (mut kept, mut dropped) = (0, Vec::new());
    for n in &names {
        match variants.get(n) {
            Some(list) => {
                for v in list {
                    out.push_str(v);
                    out.push('\n');
                }
                kept += 1;
            }
            None => dropped.push(n.clone()),
        }
    }
    std::fs::write(&path, out)?;
    println!("link-order: {kept} of {} names written, {} dropped", names.len(), dropped.len());
    for d in dropped.iter().take(10) {
        println!("  dropped: {d}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::normalize;

    #[test]
    fn the_crate_disambiguators_go_and_the_rest_stays() {
        let a = "_RNvMs_NtCsa761aVayPYW_6hk_psx7enemiesNtB5_5Actor12apply_mawlek";
        let b = "_RNvMs_NtCsk28q54l69oa_6hk_psx7enemiesNtB5_5Actor12apply_mawlek";
        assert_eq!(normalize(a), normalize(b));
        assert_eq!(normalize(a), "_RNvMs_NtC_6hk_psx7enemiesNtB5_5Actor12apply_mawlek");
        assert_eq!(normalize("memset"), "memset");
        assert_eq!(normalize("_RNvCs1a_6hk_sim3foo"), "_RNvC_6hk_sim3foo");
    }
}
