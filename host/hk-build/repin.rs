//! Pacing ceilings that move by chance are re-pinned by the validate run itself, with a record,
//! instead of being chased by hand; and a determinism check proves the same disc plays the same
//! route twice, which is what makes any remaining exact pin trustworthy (docs/ROUTES.md,
//! "Closed loop first").
//!
//! The rule is the coordinator's of 2026-10-29: a route with a cut list keeps its exact pins;
//! every other route is judged on its outcome pins, and its pacing ceilings follow the disc
//! unless the frame time is a real slowdown. A lane chases a move only when an outcome pin fails.
use crate::pacing::{Pacing, PacingPins};
use crate::routes::Route;
use serde_json::Value;
use std::path::{Path, PathBuf};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// A frame time more than this many percent over the pinned ceiling is a slowdown, not drift:
/// the replay fails and the lane investigates (perf must never get slower).
pub const SLOWDOWN_PERCENT: u64 = 10;

/// Where re-pins are recorded (committed with the change that caused them).
pub const LOG: &str = "docs/REPIN-LOG.md";

/// One ceiling that moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub route: String,
    pub field: &'static str,
    pub old: u64,
    pub new: u64,
    /// What the replay measured against it.
    pub measured: u64,
}

/// What to do about a route whose pacing ceilings the replay is over.
#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    /// Move these ceilings (only the ones that were over).
    Repin(Vec<Change>),
    /// Do not: the frame time is a slowdown. The sentence says by how much.
    Slowdown(String),
}

/// Decide for an open-loop route that has no cut list, whose outcome pins held and whose only
/// failures are pacing ceilings.
pub fn decide(route: &Route, measured: &Pacing) -> Decision {
    let ceilings = measured.ceilings();
    if let Some(pinned) = route.frame_time {
        let limit = (pinned * (100 + SLOWDOWN_PERCENT)).div_ceil(100);
        if measured.frame_time() > limit {
            return Decision::Slowdown(format!(
                "frame time {} is more than {SLOWDOWN_PERCENT} percent over the pinned {pinned} (limit {limit}): a slowdown, not drift",
                measured.frame_time()
            ));
        }
    }
    let mut changes = Vec::new();
    let mut push = |field: &'static str, old: u64, new: Option<u64>, measured: u64| {
        if let Some(new) = new {
            if measured > old {
                changes.push(Change {
                    route: route.name.clone(),
                    field,
                    old,
                    new,
                    measured,
                });
            }
        }
    };
    if let Some(old) = route.frame_time {
        push(
            "frame_time",
            old,
            ceilings.frame_time,
            measured.frame_time(),
        );
    }
    if let Some(old) = route.long_frames {
        push("long_frames", old, ceilings.long_frames, measured.long);
    }
    push("pacing", route.pacing, Some(ceilings.pacing), measured.over);
    Decision::Repin(changes)
}

/// The pin file's pacing keys with `changes` applied; the other keys and every comment stay.
pub fn rewritten(route: &Route, text: &str, changes: &[Change]) -> String {
    let mut pins = PacingPins {
        pacing: route.pacing,
        frame_time: route.frame_time,
        long_frames: route.long_frames,
    };
    for change in changes {
        match change.field {
            "frame_time" => pins.frame_time = Some(change.new),
            "long_frames" => pins.long_frames = Some(change.new),
            _ => pins.pacing = change.new,
        }
    }
    crate::pacing::rewrite_pins(text, &pins)
}

/// Write the new ceilings into the pin files and record them in [`LOG`].
pub fn apply(
    root: &Path,
    routes: &[Route],
    changes: &[Change],
    disc_sha: &str,
    when: &str,
) -> Result<()> {
    let mut names: Vec<&str> = changes.iter().map(|c| c.route.as_str()).collect();
    names.dedup();
    let mut entry = format!(
        "\n## {when}, disc {}\n\n",
        &disc_sha[..disc_sha.len().min(12)]
    );
    for name in names {
        let route = routes
            .iter()
            .find(|r| r.name == name)
            .ok_or("unknown route in a re-pin")?;
        let mine: Vec<Change> = changes
            .iter()
            .filter(|c| c.route == name)
            .cloned()
            .collect();
        let path = root.join(format!("tools/tapes/{name}.pins"));
        std::fs::write(
            &path,
            rewritten(route, &std::fs::read_to_string(&path)?, &mine),
        )?;
        let parts: Vec<String> = mine
            .iter()
            .map(|c| {
                format!(
                    "{} {} to {} (measured {})",
                    c.field, c.old, c.new, c.measured
                )
            })
            .collect();
        entry.push_str(&format!("- {name}: {}\n", parts.join(", ")));
    }
    let log = root.join(LOG);
    let mut text = std::fs::read_to_string(&log).unwrap_or_else(|_| {
        "# Re-pin log\n\nPacing ceilings that `cargo hk-build validate --repin` moved on its own, newest last. \
         A frame time more than 10 percent over its pin is not re-pinned (it fails); everything here is drift.\n"
            .to_string()
    });
    text.push_str(&entry);
    std::fs::write(log, text)?;
    Ok(())
}

/// `YYYY-MM-DD HH:MM` (UTC) for a Unix time.
pub fn utc_stamp(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02} UTC",
        rest / 3600,
        rest % 3600 / 60
    )
}

/// What differs between two replays of the same route on the same disc, empty when identical:
/// the final RAM words of `command.json` and the bytes of `route.csv`.
pub fn differences(first: &Path, second: &Path) -> Result<Vec<String>> {
    let ram = |dir: &Path| -> Result<Value> {
        let record: Value = serde_json::from_slice(&std::fs::read(dir.join("command.json"))?)?;
        Ok(record["final_ram"].clone())
    };
    let (a, b) = (ram(first)?, ram(second)?);
    let mut found = Vec::new();
    let empty = serde_json::Map::new();
    let (ao, bo) = (
        a.as_object().unwrap_or(&empty),
        b.as_object().unwrap_or(&empty),
    );
    for key in ao.keys().chain(bo.keys().filter(|k| !ao.contains_key(*k))) {
        if ao.get(key) != bo.get(key) {
            found.push(format!(
                "{key}: {} then {}",
                ao.get(key).unwrap_or(&Value::Null),
                bo.get(key).unwrap_or(&Value::Null)
            ));
        }
    }
    let rows = |dir: &Path| std::fs::read(dir.join("route.csv"));
    if rows(first)? != rows(second)? {
        found.push("route.csv differs (the per-poll telemetry of the two replays)".to_string());
    }
    Ok(found)
}

/// The two output directories a determinism run of `name` uses.
pub fn run_dirs(root: &Path, name: &str) -> (PathBuf, PathBuf) {
    let base = root.join(".hkpsx/determinism");
    (base.join("a").join(name), base.join("b").join(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pacing::fold;

    /// `n` gameplay frames of `vblanks` each, as route-log rows.
    fn frames(n: usize, vblanks: usize) -> Vec<(bool, bool, Option<u32>)> {
        (0..n)
            .flat_map(|_| (0..vblanks).map(move |v| (true, v + 1 == vblanks, Some(0))))
            .collect()
    }

    fn measured(frames_of_three: usize, frames_of_two: usize) -> Pacing {
        let mut rows = vec![(true, true, Some(0)); 1]; // the bind
        rows.extend(frames(frames_of_three, 3));
        rows.extend(frames(frames_of_two, 2));
        fold(rows)
    }

    fn route(frame_time: u64, long: u64, pacing: u64) -> Route {
        Route {
            name: "r".into(),
            frame_time: Some(frame_time),
            long_frames: Some(long),
            pacing,
            ..Route::default()
        }
    }

    #[test]
    fn drift_moves_only_the_ceilings_that_were_over() {
        // 20 frames of 3 and 80 of 2: frame time 220, 20 frames over 2 vblanks.
        let m = measured(20, 80);
        assert_eq!(m.frame_time(), 220);
        let Decision::Repin(changes) = decide(&route(225, 2, 18), &m) else {
            panic!("drift")
        };
        let fields: Vec<_> = changes.iter().map(|c| c.field).collect();
        assert_eq!(
            fields,
            ["pacing"],
            "frame time and tail were fine: {changes:?}"
        );
        assert_eq!(changes[0].old, 18);
        assert!(
            changes[0].new >= 20,
            "the new ceiling holds the measurement"
        );
    }

    #[test]
    fn a_real_slowdown_is_not_blessed() {
        let m = measured(20, 80); // frame time 220
        let Decision::Slowdown(why) = decide(&route(190, 2, 99), &m) else {
            panic!("slowdown")
        };
        assert!(why.contains("220") && why.contains("190"), "{why}");
        // Within 10 percent of the pin is drift (210 limit for 191? 191 * 1.1 = 210.1 -> 211).
        assert!(matches!(decide(&route(205, 2, 99), &m), Decision::Repin(_)));
    }

    #[test]
    fn rewriting_keeps_comments_and_untouched_pins() {
        let r = route(225, 2, 18);
        let text =
            "card = a.mcd\n# why\npacing = 18\nframe_time = 225\nlong_frames = 2\nHK_A = 1\n";
        let change = Change {
            route: "r".into(),
            field: "pacing",
            old: 18,
            new: 30,
            measured: 25,
        };
        assert_eq!(
            rewritten(&r, text, &[change]),
            "card = a.mcd\n# why\npacing = 30\nframe_time = 225\nlong_frames = 2\nHK_A = 1\n"
        );
    }

    #[test]
    fn the_stamp_is_a_utc_date() {
        assert_eq!(utc_stamp(0), "1970-01-01 00:00 UTC");
        assert_eq!(utc_stamp(1_793_300_000), "2026-10-29 18:53 UTC");
    }

    #[test]
    fn two_identical_replays_agree_and_a_moved_word_is_named() {
        let dir = std::env::temp_dir().join(format!("hk-determinism-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (a, b) = (dir.join("a"), dir.join("b"));
        for (d, health) in [(&a, 5), (&b, 5)] {
            std::fs::create_dir_all(d).unwrap();
            std::fs::write(
                d.join("command.json"),
                format!("{{\"final_ram\": {{\"HK_HEALTH\": {health}, \"HK_GEO\": 40}}}}"),
            )
            .unwrap();
            std::fs::write(d.join("route.csv"), "1,2\n3,4\n").unwrap();
        }
        assert!(differences(&a, &b).unwrap().is_empty());
        std::fs::write(
            b.join("command.json"),
            "{\"final_ram\": {\"HK_HEALTH\": 4, \"HK_GEO\": 40}}",
        )
        .unwrap();
        std::fs::write(b.join("route.csv"), "1,2\n3,5\n").unwrap();
        let found = differences(&a, &b).unwrap();
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found[0].starts_with("HK_HEALTH: 5 then 4"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
