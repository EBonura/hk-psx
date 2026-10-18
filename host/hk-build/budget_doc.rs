//! Keep docs/BUDGET.md's pinned figures equal to the build that was just made.
//!
//! The ordinary build's memory figures are quoted in four places of the doc: the allocation table,
//! the scene arena sentence and the static end address. `tests/test_budget_doc.py` pins all of them
//! to `.hkpsx/build-normal.json`, and it used to fail on every build that moved memory, after the
//! cook, the link and the disc were already made (an hour of build to report a stale number a person
//! then edited by hand). The doc quotes the build, so the build writes it: this rewrites exactly the
//! pinned figures from the report and leaves the dated prose alone. The test stays as the check that
//! the rewrite found every place.
use serde_json::Value;
use std::path::Path;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Table row label to the `memory` key in the build report it quotes.
const ROWS: &[(&str, &str)] = &[
    ("Linked code", "code_bytes"),
    ("Code-to-data alignment", "code_data_alignment_bytes"),
    ("Linked data, including menu and HUD", "data_bytes"),
    ("BSS, including shared scene arena and runtime pools", "bss_bytes"),
    ("Total static span", "static_span_bytes"),
    ("Linker stack exclusion", "stack_reserved_bytes"),
    ("Unallocated gap before reserved stack", "unallocated_before_stack_bytes"),
    ("Room module pool, inside that gap", "module_pool_bytes"),
    ("Free RAM below the module pool", "free_below_pool_bytes"),
];

/// `1234567` as `1,234,567`, the doc's style.
fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn memory_u64(memory: &Value, key: &str) -> Result<u64> {
    memory[key]
        .as_u64()
        .ok_or_else(|| format!("build report memory.{key} is missing").into())
}

/// Rewrite the pinned figures of `doc` from the report's `memory` object. Returns the new text.
pub fn sync_text(doc: &str, memory: &Value) -> Result<String> {
    let mut lines: Vec<String> = doc.lines().map(str::to_string).collect();
    for (label, key) in ROWS {
        let value = grouped(memory_u64(memory, key)?);
        let prefix = format!("| {label} |");
        let line = lines
            .iter_mut()
            .find(|l| l.starts_with(&prefix) && l.ends_with('|'))
            .ok_or_else(|| format!("docs/BUDGET.md has no `{label}` table row"))?;
        *line = format!("{prefix}{value}|");
    }
    let mut text = lines.join("\n");
    if doc.ends_with('\n') {
        text.push('\n');
    }
    // "The single scene arena occupies 410,612 bytes, ..."
    let arena = grouped(memory_u64(memory, "scene_arena_bytes")?);
    text = replace_after(&text, "scene arena occupies ", &arena, |c| {
        c.is_ascii_digit() || c == ','
    })?;
    // "Static data ends at `0x801d3b4c`; ..."
    let end = memory["static_end"]
        .as_str()
        .ok_or("build report memory.static_end is missing")?;
    text = replace_after(&text, "Static data ends at `", end, |c| c != '`')?;
    Ok(text)
}

/// Replace the run of characters accepted by `in_value` that follows the first `marker`.
fn replace_after(
    text: &str,
    marker: &str,
    value: &str,
    in_value: impl Fn(char) -> bool,
) -> Result<String> {
    let at = text
        .find(marker)
        .ok_or_else(|| format!("docs/BUDGET.md lost the sentence containing `{marker}`"))?
        + marker.len();
    let len: usize = text[at..]
        .chars()
        .take_while(|c| in_value(*c))
        .map(char::len_utf8)
        .sum();
    Ok(format!("{}{}{}", &text[..at], value, &text[at + len..]))
}

/// Write docs/BUDGET.md from `.hkpsx/build-normal.json`. Returns whether the file changed.
pub fn sync(root: &Path) -> Result<bool> {
    let report: Value = serde_json::from_slice(&std::fs::read(root.join(".hkpsx/build-normal.json"))?)?;
    let path = root.join("docs/BUDGET.md");
    let old = std::fs::read_to_string(&path)?;
    let new = sync_text(&old, &report["memory"])?;
    if new == old {
        return Ok(false);
    }
    std::fs::write(&path, new)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn memory() -> Value {
        json!({
            "code_bytes": 800180, "code_data_alignment_bytes": 0, "data_bytes": 255192,
            "bss_bytes": 796924, "static_span_bytes": 1854364, "stack_reserved_bytes": 49152,
            "unallocated_before_stack_bytes": 127844, "module_pool_bytes": 102400,
            "free_below_pool_bytes": 25264, "scene_arena_bytes": 410724, "static_end": "0x801d4c50",
        })
    }

    const DOC: &str = "# Budget\n\n| Main RAM allocation | Bytes |\n| --- | ---: |\n\
| Linked code |802,240|\n| Code-to-data alignment |0|\n| Linked data, including menu and HUD |1|\n\
| BSS, including shared scene arena and runtime pools |2|\n| Total static span |3|\n\
| BIOS/kernel reservation |65,536|\n| Linker stack exclusion |4|\n| Space above initial stack pointer |256|\n\
| Unallocated gap before reserved stack |5|\n| Room module pool, inside that gap |6|\n\
| Free RAM below the module pool |7|\n\nProse about 802,240 stays.\n\n\
Static data ends at `0x801d3b4c`; the reserved stack begins at `0x801f3f00`.\n\n\
There is no heap. The single scene arena occupies 410,612 bytes, replacing five\n";

    #[test]
    fn pinned_figures_follow_the_report_and_prose_stays() {
        let out = sync_text(DOC, &memory()).unwrap();
        assert!(out.contains("| Linked code |800,180|\n"));
        assert!(out.contains("| Free RAM below the module pool |25,264|\n"));
        assert!(out.contains("| Total static span |1,854,364|\n"));
        assert!(out.contains("| BIOS/kernel reservation |65,536|\n"), "unpinned rows are untouched");
        assert!(out.contains("Prose about 802,240 stays."));
        assert!(out.contains("Static data ends at `0x801d4c50`; the reserved"));
        assert!(out.contains("scene arena occupies 410,724 bytes, replacing"));
        assert!(out.ends_with("replacing five\n"));
    }

    #[test]
    fn a_second_sync_changes_nothing() {
        let once = sync_text(DOC, &memory()).unwrap();
        assert_eq!(sync_text(&once, &memory()).unwrap(), once);
    }

    #[test]
    fn a_vanished_row_or_sentence_is_an_error_not_a_silent_skip() {
        assert!(sync_text(&DOC.replace("| Linked code |", "| Code |"), &memory()).is_err());
        assert!(sync_text(&DOC.replace("scene arena occupies", "arena holds"), &memory()).is_err());
    }

    #[test]
    fn grouping_matches_the_docs_style() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(25264), "25,264");
        assert_eq!(grouped(1854364), "1,854,364");
    }
}
