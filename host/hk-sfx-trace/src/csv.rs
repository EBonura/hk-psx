//! Tiny CSV and TSV readers for the driver's logs and the census tables.

use std::collections::BTreeMap;
use std::path::Path;

pub type Row = BTreeMap<String, String>;

/// One CSV line with `"` quoting and `""` escapes.
pub fn split(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}

/// A CSV file with a header row. Missing file reads as empty.
pub fn table(path: &Path) -> Vec<Row> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return vec![];
    };
    let mut lines = text.lines();
    let Some(head) = lines.next() else {
        return vec![];
    };
    let head = split(head);
    lines
        .filter_map(|l| {
            let c = split(l);
            (c.len() >= head.len()).then(|| head.iter().cloned().zip(c).collect())
        })
        .collect()
}

/// A tab separated census table: `# ` header line, `#` comments skipped.
pub fn tsv(path: &Path) -> Result<Vec<Vec<String>>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .map(|l| l.split('\t').map(String::from).collect())
        .collect())
}
