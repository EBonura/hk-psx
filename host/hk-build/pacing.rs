//! Frame pacing of a route replay and the ceilings its pin file holds it to.
//!
//! The judged frames are the gameplay frames of the replay: the run of vblanks between two
//! display flips while `HK_GAME_MODE` is at its most common value and `HK_ROOM_LOAD_STATE` at its
//! most common (resident) value, so the title screen, loads and gates stay out (the same rule
//! as tools/frame-pacing). The first frame after the game re-enters those states is the view
//! bind (render::init, the template and vignette rebuild and the first cold scenery pass, five
//! or six vblanks); a bind of a new view inside a scene (`HK_VIEW_BINDS`) costs the same. Both
//! are load work and are counted apart, not judged.
//!
//! Three figures come out, and `docs/ROUTES.md` ("Frame pacing") says why:
//!
//! * `frame_time`: the mean frame time in hundredths of a vblank (the sum of the judged frames'
//!   vblanks over their number, rounded up). A tape is a fixed number of polls, so the time it
//!   spans does not change when the game gets slower; the number of frames drawn in it does,
//!   and this figure moves with it whether a frame takes two vblanks or three.
//! * `long_frames`: the judged frames of five vblanks or more, the tail that a mean can hide. Four
//!   would be the natural line but a frame of four is a three-vblank frame nudged over the edge: moving
//!   the code in memory changed that count from 7 to 15 on a route whose mean did not move (the layout
//!   check in docs/ROUTES.md); five is a stall.
//! * `pacing`: the frames over two vblanks, the 30 fps bar. Secondary: once most frames are late
//!   a count of them stops moving (a route at 137 of 142 can only gain five), so its ceiling is
//!   held below the frame total wherever that leaves room above the measurement, and a route
//!   with nearly every frame late leaves the checking to `frame_time`.
use super::{routes, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

/// Vblanks from which a frame is a stall (the tail check).
pub const LONG_FRAME: u64 = 5;

/// One replay's judged frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Pacing {
    /// Judged gameplay frames.
    pub frames: u64,
    /// Their vblanks in total.
    pub vblanks: u64,
    /// Frames over two vblanks.
    pub over: u64,
    /// Frames of five vblanks or more.
    pub long: u64,
    /// Longest frame, in vblanks.
    pub longest: u64,
    /// Longest run of consecutive frames over two vblanks.
    pub streak: u64,
    /// Frames spent binding a view (not judged) and the longest of them.
    pub bind: u64,
    pub bind_longest: u64,
}

/// The ceilings a pin file holds a route to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacingPins {
    pub pacing: u64,
    pub frame_time: Option<u64>,
    pub long_frames: Option<u64>,
}

impl Pacing {
    /// Mean frame time in hundredths of a vblank, rounded up.
    pub fn frame_time(&self) -> u64 {
        (self.vblanks * 100).div_ceil(self.frames.max(1))
    }

    /// The report line (without the route name).
    pub fn report(&self, pins: &PacingPins) -> String {
        let ceiling = |c: Option<u64>| c.map_or("none".to_string(), |c| c.to_string());
        format!(
            "pacing: frame time {} of {} frames (ceiling {}), {} at {}+ vblanks (ceiling {}), {} over 2 vblanks (ceiling {}), longest {}, worst streak {}; view binds {}, longest {}",
            self.frame_time(),
            self.frames,
            ceiling(pins.frame_time),
            self.long,
            LONG_FRAME,
            ceiling(pins.long_frames),
            self.over,
            pins.pacing,
            self.longest,
            self.streak,
            self.bind,
            self.bind_longest
        )
    }

    /// Which ceilings the replay is over, as sentences.
    pub fn failures(&self, pins: &PacingPins) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(ceiling) = pins.frame_time {
            if self.frame_time() > ceiling {
                out.push(format!("frame time {} over {ceiling} (hundredths of a vblank)", self.frame_time()));
            }
        }
        if let Some(ceiling) = pins.long_frames {
            if self.long > ceiling {
                out.push(format!("{} frames of {LONG_FRAME}+ vblanks over {ceiling}", self.long));
            }
        }
        if self.over > pins.pacing {
            out.push(format!("{} frames over 2 vblanks over {}", self.over, pins.pacing));
        }
        out
    }

    /// The ceilings this measurement is blessed at. `frame_time`: the measurement plus 5 percent,
    /// rounded up (the same code at another address moved it by 1 percent on most routes and 10
    /// percent down at worst). `long_frames`: plus 25 percent, rounded up, and 2 frames. `pacing`:
    /// plus 30 percent, rounded up, and 2 frames, held under the frame total when that leaves it
    /// above the measurement plus 5 percent and 2 frames.
    pub fn ceilings(&self) -> PacingPins {
        let plus = |m: u64, percent: u64| (m * (100 + percent)).div_ceil(100);
        let floor = plus(self.over, 5) + 2;
        PacingPins {
            pacing: floor.max((plus(self.over, 30) + 2).min(self.frames.saturating_sub(1))),
            frame_time: Some(plus(self.frame_time(), 5)),
            long_frames: Some(plus(self.long, 25) + 2),
        }
    }
}

/// Fold per-row figures into [`Pacing`]. `rows` is `(gameplay, flip, view binds)` per route-log
/// row: whether the row is a judged gameplay tick, whether the display flipped, and the
/// `HK_VIEW_BINDS` word (a change inside gameplay is a bind of another view).
pub fn fold(rows: impl IntoIterator<Item = (bool, bool, Option<u32>)>) -> Pacing {
    let mut p = Pacing::default();
    let (mut since, mut streak, mut fresh, mut seen) = (0u64, 0u64, true, None::<u32>);
    for (gameplay, flip, binds) in rows {
        if let Some(b) = binds {
            // A bind inside a scene (the camera crossing into another view) is the same work without a load.
            if seen.is_some_and(|s| s != b) && gameplay {
                fresh = true;
            }
            seen = Some(b);
        }
        if !gameplay {
            since = 0;
            fresh = true;
            continue;
        }
        since += 1;
        if flip {
            if fresh {
                p.bind += 1;
                p.bind_longest = p.bind_longest.max(since);
                fresh = false;
            } else {
                p.frames += 1;
                p.vblanks += since;
                p.longest = p.longest.max(since);
                if since >= LONG_FRAME {
                    p.long += 1;
                }
                if since > 2 {
                    p.over += 1;
                    streak += 1;
                    p.streak = p.streak.max(streak);
                } else {
                    streak = 0;
                }
            }
            since = 0;
        }
    }
    p
}

/// Pacing of one replay directory (`command.json` and `route.csv`).
pub fn measure(output: &Path) -> Result<Pacing> {
    let command: Value = serde_json::from_slice(&std::fs::read(output.join("command.json"))?)?;
    let column = |name: &str| {
        command["watches"][name]
            .as_str()
            .map(|a| format!("ram_{:0>8}", a.trim_start_matches("0x")))
    };
    let text = std::fs::read_to_string(output.join("route.csv"))?;
    let mut lines = text.lines();
    let header: Vec<&str> = lines
        .next()
        .ok_or("route.csv is empty")?
        .split(',')
        .collect();
    let at = |name: &str| header.iter().position(|h| *h == name);
    let flip = at("display_start_changed").ok_or("route.csv has no display_start_changed")?;
    let (game, load, binds) = (
        column("HK_GAME_MODE").and_then(|c| at(&c)),
        column("HK_ROOM_LOAD_STATE").and_then(|c| at(&c)),
        column("HK_VIEW_BINDS").and_then(|c| at(&c)),
    );
    let rows: Vec<Vec<&str>> = lines.map(|l| l.split(',').collect()).collect();
    let mode = |index: Option<usize>| -> Option<&str> {
        let index = index?;
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for r in &rows {
            *counts.entry(r[index]).or_default() += 1;
        }
        counts.into_iter().max_by_key(|&(_, n)| n).map(|(v, _)| v)
    };
    // Gameplay is `HK_GAME_MODE` 1. The most common value is only a fallback: on a short route (the
    // cheat tapes) the load and the play last about as long, and the count alone picked the load.
    let play = match game {
        Some(g) if rows.iter().any(|r| r[g] == "1") => Some("1"),
        _ => mode(game),
    };
    // The load state the play runs in: the commonest among the play rows.
    let ready = {
        let mut counts: HashMap<&str, usize> = HashMap::new();
        if let Some(l) = load {
            for r in rows.iter().filter(|r| game.is_none_or(|g| Some(r[g]) == play)) {
                *counts.entry(r[l]).or_default() += 1;
            }
        }
        counts.into_iter().max_by_key(|&(_, n)| n).map(|(v, _)| v)
    };
    let pacing = fold(rows.iter().map(|r| {
        let gameplay = game.is_none_or(|g| Some(r[g]) == play) && load.is_none_or(|l| Some(r[l]) == ready);
        (gameplay, r[flip] == "1", binds.and_then(|b| r[b].parse().ok()))
    }));
    if pacing.frames == 0 {
        return Err("the replay has no judged gameplay frame".into());
    }
    Ok(pacing)
}

/// Replace or add the pacing keys of a pin file, keeping its comments and other lines.
/// The three keys sit together, where `pacing` was (or at the top).
pub fn rewrite_pins(text: &str, pins: &PacingPins) -> String {
    let block = format!(
        "pacing = {}\nframe_time = {}\nlong_frames = {}\n",
        pins.pacing,
        pins.frame_time.unwrap_or(0),
        pins.long_frames.unwrap_or(0)
    );
    let mut out = String::new();
    let mut placed = false;
    for line in text.lines() {
        let key = line.split_once('=').map(|(k, _)| k.trim());
        match key {
            Some("pacing") if !placed => {
                out.push_str(&block);
                placed = true;
            }
            Some("pacing" | "frame_time" | "long_frames") => {}
            _ => {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    if placed { out } else { format!("{block}{out}") }
}

/// `cargo hk-build pace-pins [RUN_DIR]`: write the ceilings of every route from a validate run.
/// A route with a cut list whose open-loop tape did not reach its pins is skipped: it drifted
/// and its closed-loop run is what judges it.
pub fn write_pins(root: &Path, run: &Path) -> Result<()> {
    for route in routes::load(root)? {
        let dir = run.join(&route.name);
        if !dir.join("route.csv").is_file() {
            println!("{}: no replay in {}, left alone", route.name, run.display());
            continue;
        }
        let command: Value = serde_json::from_slice(&std::fs::read(dir.join("command.json"))?)?;
        let held = route.pins.iter().all(|pin| match command["final_ram"][pin.symbol.as_str()].as_u64() {
            Some(v) if pin.at_least => v >= pin.value,
            Some(v) => v == pin.value,
            None => false,
        });
        if route.cuts.is_some() && !held {
            println!("{}: the open-loop tape drifted on this disc, left alone", route.name);
            continue;
        }
        let measured = measure(&dir)?;
        let pins = measured.ceilings();
        let path = root.join(format!("tools/tapes/{}.pins", route.name));
        let text = std::fs::read_to_string(&path)?;
        std::fs::write(&path, rewrite_pins(&text, &pins))?;
        println!("{}: {}", route.name, measured.report(&pins));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `n` gameplay frames of `vblanks` each, as route-log rows.
    fn frames(n: usize, vblanks: usize) -> Vec<(bool, bool, Option<u32>)> {
        (0..n)
            .flat_map(|_| (0..vblanks).map(move |v| (true, v + 1 == vblanks, Some(0))))
            .collect()
    }

    #[test]
    fn a_first_frame_is_a_bind_and_loads_are_not_judged() {
        let mut rows = vec![(false, false, Some(0)); 5];
        rows.extend(frames(1, 5)); // the bind
        rows.extend(frames(3, 2));
        rows.extend(vec![(false, true, Some(0)); 9]); // a load that ends in a flip
        let p = fold(rows);
        assert_eq!((p.frames, p.vblanks, p.bind, p.bind_longest), (3, 6, 1, 5));
    }

    #[test]
    fn frame_time_moves_when_every_frame_is_already_late() {
        // 142 frames of three vblanks (the first is the view bind), then a route 7 percent slower.
        let before = fold(frames(142, 3));
        let mut slower = frames(112, 3);
        slower.extend(frames(30, 4));
        let after = fold(slower);
        assert_eq!((before.over, after.over), (141, 141)); // every frame late either way
        assert!(after.frame_time() > before.frame_time());
        assert_eq!(before.frame_time(), 300);
        assert!(after.frame_time() > 315);
        // The count of late frames does not move; the primary ceilings do.
        let pins = before.ceilings();
        assert!(after.failures(&pins).iter().any(|f| f.contains("frame time")));
        assert!(before.failures(&pins).is_empty());
    }

    #[test]
    fn the_tail_is_counted_and_a_mean_cannot_hide_it() {
        let mut rows = frames(200, 2);
        rows.extend(frames(3, 5));
        let p = fold(rows);
        assert_eq!((p.long, p.longest, p.streak), (3, 5, 3)); // a frame of four would not count
        let mut pins = p.ceilings();
        pins.frame_time = Some(1000);
        pins.long_frames = Some(1);
        assert_eq!(p.failures(&pins).len(), 1);
    }

    #[test]
    fn ceilings_follow_the_rule() {
        let p = Pacing { frames: 142, vblanks: 142 * 3 - 5, over: 137, long: 1, ..Pacing::default() };
        let c = p.ceilings();
        assert_eq!(p.frame_time(), 297);
        assert_eq!(c.frame_time, Some(312)); // 297 x 1.05 = 311.85
        assert_eq!(c.long_frames, Some(4)); // 1.25 x 1 rounded up, and 2
        assert_eq!(c.pacing, 146); // 1.3 x 137 + 2 = 181 is held under the frame total, 141, but not under 1.05 x 137 + 2
        let clean = Pacing { frames: 100, vblanks: 200, ..Pacing::default() };
        let c = clean.ceilings();
        assert_eq!((c.frame_time, c.long_frames, c.pacing), (Some(210), Some(2), 2));
        let some_late = Pacing { frames: 200, vblanks: 460, over: 100, ..Pacing::default() };
        assert_eq!(some_late.ceilings().pacing, 132); // 1.3 x 100 + 2, under 199
        // Every frame late: the late count has nothing left to say and stays above the measurement.
        let all = Pacing { frames: 50, vblanks: 150, over: 50, ..Pacing::default() };
        assert_eq!(all.ceilings().pacing, 55);
    }

    #[test]
    fn rewriting_keeps_comments_and_other_keys() {
        let text = "pacing = 26\n# why\ncard = a.mcd\nframe_time = 1\nHK_A = 3\n";
        let pins = PacingPins { pacing: 9, frame_time: Some(250), long_frames: Some(0) };
        let out = rewrite_pins(text, &pins);
        assert_eq!(out, "pacing = 9\nframe_time = 250\nlong_frames = 0\n# why\ncard = a.mcd\nHK_A = 3\n");
        assert_eq!(rewrite_pins(&out, &pins), out);
        assert!(rewrite_pins("card = a.mcd\n", &pins).starts_with("pacing = 9\n"));
    }
}
