//! `cargo hk-build phase-sweep`: re-pick a walk's phase rests (docs/ROUTES.md, "Phase rests").
//!
//! A walk that fights moving enemies is judged by where the frame phase leaves them, so a few of its
//! `wait N` lines are phase picks: another value is another phase, and a build whose startup or layout
//! changed needs another pick. A rest is tagged in its own comment (`wait 7  # phase-rest 06b: ...`).
//! This tries a fixed list of values for a named rest on every build it is given and prints pass or
//! fail per build and where the failures stopped, then the values that pass on all of them. It never
//! edits a walk file: a person writes the chosen value and the comment.

use std::path::{Path, PathBuf};

use crate::{routes, walk, Result};

/// The values tried when `--values` is not given.
pub const DEFAULT_VALUES: [u32; 13] = [5, 7, 13, 17, 23, 31, 38, 41, 47, 51, 57, 63, 67];

/// One tagged rest in a walk's expanded text.
#[derive(Debug, PartialEq)]
pub struct Rest {
    pub name: String,
    /// Index of its line in the expanded text.
    pub line: usize,
    pub value: u32,
}

/// The tagged rests of an expanded walk: `wait N` lines whose comment holds `phase-rest NAME`.
pub fn rests(text: &str) -> Vec<Rest> {
    let mut found = Vec::new();
    for (line, source) in text.lines().enumerate() {
        let Some((code, comment)) = source.split_once('#') else { continue };
        let mut words = code.split_whitespace();
        if words.next() != Some("wait") {
            continue;
        }
        let (Some(value), None) = (words.next().and_then(|v| v.parse().ok()), words.next()) else { continue };
        let Some((_, tag)) = comment.split_once("phase-rest ") else { continue };
        let name: String = tag.chars().take_while(|c| c.is_alphanumeric() || matches!(c, '-' | '_')).collect();
        if !name.is_empty() {
            found.push(Rest { name, line, value });
        }
    }
    found
}

/// The text with the rest's value replaced.
pub fn with_value(text: &str, rest: &Rest, value: u32) -> String {
    let mut out = String::new();
    for (line, source) in text.lines().enumerate() {
        if line == rest.line {
            out.push_str(&source.replacen(&format!("wait {}", rest.value), &format!("wait {value}"), 1));
        } else {
            out.push_str(source);
        }
        out.push('\n');
    }
    out
}

/// Where a failed walk stopped, from the gate's headline: the step it aborted on (`loop NAME`), or its start.
pub fn stopped_at(headline: &str) -> String {
    match headline.split("ABORTED: ").nth(1) {
        Some(aborted) => match aborted.split('`').nth(1) {
            Some(step) => step.split_whitespace().take(2).collect::<Vec<_>>().join(" "),
            None => aborted.split(" (").next().unwrap_or(aborted).chars().take(70).collect(),
        },
        None => headline.chars().take(60).collect(),
    }
}

/// `phase-sweep WALK [--rest NAME]... [--values A,B,...] --library DIR [--library DIR]...`
pub fn run(root: &Path, args: &mut dyn Iterator<Item = String>) -> Result<()> {
    let usage = "usage: phase-sweep WALK [--rest NAME]... [--values A,B,...] --library DIR [--library DIR]...";
    let walk_name = args.next().ok_or(usage)?;
    let mut names = Vec::new();
    let mut values: Vec<u32> = DEFAULT_VALUES.to_vec();
    let mut libraries: Vec<PathBuf> = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--rest" => names.push(args.next().ok_or(usage)?),
            "--values" => {
                values = args
                    .next()
                    .ok_or(usage)?
                    .split(',')
                    .map(|v| v.trim().parse().map_err(|_| format!("--values: {v} is not a number")))
                    .collect::<std::result::Result<_, _>>()?
            }
            "--library" => libraries.push(PathBuf::from(args.next().ok_or(usage)?)),
            _ => return Err(usage.into()),
        }
    }
    if libraries.is_empty() {
        return Err(format!("{usage}\n(walks play shipping discs: name each build's library, one per build to try)").into());
    }
    let (_, script) = walk::scripts(root)?
        .into_iter()
        .find(|(name, _)| *name == walk_name)
        .ok_or_else(|| format!("no walk named {walk_name} in tools/walks"))?;
    let text = walk::expand(&script)?;
    let tagged = rests(&text);
    if tagged.is_empty() {
        return Err(format!("{walk_name} has no `# phase-rest NAME` rests").into());
    }
    let chosen: Vec<&Rest> = if names.is_empty() {
        tagged.iter().collect()
    } else {
        names
            .iter()
            .map(|n| tagged.iter().find(|r| r.name == *n).ok_or_else(|| format!("{walk_name} has no phase rest named {n}")))
            .collect::<std::result::Result<_, _>>()?
    };
    let gate = routes::Gate::resolve(None)?;
    let out = root.join(".hkpsx/phase-sweep");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out)?;
    let label = |library: &Path| library.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    for rest in chosen {
        println!("\n{walk_name}, rest {} (the file has {}):", rest.name, rest.value);
        let mut passing = Vec::new();
        for &value in &values {
            let candidate = with_value(&text, rest, value);
            let mut all = true;
            let mut cells = Vec::new();
            for library in &libraries {
                let map = library.join("game.map");
                if !map.is_file() {
                    return Err(format!("{}: no game.map beside the disc", library.display()).into());
                }
                let run = format!("{}-{}-{value}-{}", walk_name, rest.name, label(library));
                let (ok, headline) = walk::play_text(&gate.binary, library, &map, &out, &run, &candidate)?;
                all &= ok;
                cells.push(if ok { format!("{}: PASS", label(library)) } else { format!("{}: FAIL at {}", label(library), stopped_at(&headline)) });
            }
            println!("  {value:>3}  {}", cells.join("   "));
            if all {
                passing.push(value);
            }
        }
        let list: Vec<String> = passing.iter().map(u32::to_string).collect();
        println!("  passes on every build: {}", if list.is_empty() { "none".into() } else { list.join(", ") });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "name w\nwait 300\nwait 7   # phase-rest six-b: rest before 06b\nwait 38 # phase-rest greeting\nwait 9 # a plain rest\nloop x\nwait 51 # phase-rest in-loop\nend\n";

    #[test]
    fn tagged_rests_are_found_by_name_and_line() {
        let found = rests(TEXT);
        let names: Vec<(&str, usize, u32)> = found.iter().map(|r| (r.name.as_str(), r.line, r.value)).collect();
        assert_eq!(names, [("six-b", 2, 7), ("greeting", 3, 38), ("in-loop", 6, 51)]);
    }

    #[test]
    fn only_the_tagged_line_changes() {
        let found = rests(TEXT);
        let changed = with_value(TEXT, &found[1], 47);
        assert!(changed.contains("wait 47 # phase-rest greeting"));
        assert!(changed.contains("wait 7   # phase-rest six-b") && changed.contains("wait 9 # a plain rest"));
        assert_eq!(changed.lines().count(), TEXT.lines().count());
    }

    #[test]
    fn a_failure_names_the_step_it_stopped_on() {
        let head = "FAIL w 63 checks | ABORTED: step 68298 `loop d06b-h013 until expr abs(i32(HK_PLAYER_X) - 1) <= 2 max 3`: loop timed out";
        assert_eq!(stopped_at(head), "loop d06b-h013");
        assert_eq!(stopped_at("FAIL w 5 checks: 4 pass, 1 fail"), "FAIL w 5 checks: 4 pass, 1 fail");
        assert_eq!(stopped_at("FAIL w | ABORTED: step 108, iteration 18, step 5: hold_until timed out after 90 ticks (value = 0)"), "step 108, iteration 18, step 5: hold_until timed out after 90 ticks");
    }
}
