//! Render the checked King's Pass route as pad steps for psoxide-gate.
//!
//! The frontend tape is sampled on port-1 polls. Gate steps advance video
//! ticks. Read the route CSV from a replay of the exact final disc; its first
//! two columns map video ticks to tape frames. Never assume those clocks are
//! equivalent, especially while a scene is loading and pad polls stop.
use std::collections::BTreeSet;
use std::env;
use std::fs;

// The final save checkpoint is at 11_500. Capture at least 200 more video
// ticks so the save is observed in sustained play, not at the trace edge.
const MIN_END: usize = 11_700;
// This tape enables invincibility for the traversal. Its zero-death outcome
// proves save progression, not survival under normal damage. Separate normal
// controls routes cover damage, death, respawn and Focus healing.
// Checkpoint ticks and values came from the read-only King's Pass route
// capture. Regenerate the pad schedule from the final disc's trace, then
// review these semantic checkpoints against that same trace before blessing.
const CHECKPOINTS: &[(usize, &str, &[(&str, u32)])] = &[
    (200, "title", &[("HK_GAME_MODE", 0)]),
    (210, "start-menu", &[("HK_GAME_MODE", 0)]),
    (
        650,
        "kings-start",
        &[("HK_GAME_MODE", 1), ("HK_REGION_ID", 1), ("HK_CHEATS", 1)],
    ),
    (
        1_800,
        "kings-climb",
        &[("HK_GAME_MODE", 1), ("HK_REGION_ID", 8)],
    ),
    (
        5_000,
        "kings-pass",
        &[("HK_GAME_MODE", 1), ("HK_REGION_ID", 30)],
    ),
    (
        10_000,
        "great-door",
        &[
            ("HK_GAME_MODE", 1),
            ("HK_GREAT_DOOR_OPENED", 1),
            ("HK_SCENE_GATE_LOADS", 1),
        ],
    ),
    (
        11_100,
        "dirtmouth",
        &[("HK_GAME_MODE", 1), ("HK_REGION_ID", 100)],
    ),
    (
        11_400,
        "bench",
        &[("HK_BENCH_RESTS", 1), ("HK_SAVE_WRITES", 0)],
    ),
    (
        11_500,
        "saved",
        &[
            ("HK_REGION_ID", 100),
            ("HK_GREAT_DOOR_OPENED", 1),
            ("HK_SCENE_GATE_LOADS", 1),
            ("HK_BENCH_RESTS", 1),
            ("HK_SAVE_WRITES", 1),
            ("HK_SAVE_ERRORS", 0),
            ("HK_DEATHS", 0),
        ],
    ),
];

fn tick_cursors(trace: &str, min_end: usize) -> Result<Vec<usize>, String> {
    if !trace.starts_with("route_tick,tape_frame,") {
        return Err("route trace is missing route_tick,tape_frame header".into());
    }
    let mut cursors = Vec::new();
    let mut skipped = 0usize;
    for line in trace.lines().skip(1) {
        let mut fields = line.split(',');
        let tick = fields
            .next()
            .ok_or("missing route tick")?
            .parse::<usize>()
            .map_err(|e| e.to_string())?;
        let poll = fields
            .next()
            .ok_or("missing tape frame")?
            .parse::<usize>()
            .map_err(|e| e.to_string())?;
        if tick != cursors.len() {
            return Err(format!("route trace skips video tick {tick}"));
        }
        if cursors.last().is_some_and(|prior_poll| poll < *prior_poll) {
            return Err("route trace tape cursor decreases".into());
        }
        if let Some(&prior_poll) = cursors.last() {
            if poll > prior_poll.saturating_add(1) {
                skipped += poll - prior_poll - 1;
                eprintln!(
                    "gate journey: video tick {tick} skips tape cursors {}..{} ({} -> {poll})",
                    prior_poll + 1,
                    poll - 1,
                    prior_poll
                );
            }
        }
        cursors.push(poll);
    }
    if cursors.is_empty() {
        return Err("empty route trace".into());
    }
    let last_tick = cursors.len() - 1;
    if last_tick < min_end {
        return Err(format!(
            "route trace ends at video tick {last_tick}, before required tick {min_end}"
        ));
    }
    eprintln!(
        "gate journey: trace covers ticks 0..={last_tick}; {skipped} tape cursors skipped between video ticks"
    );
    Ok(cursors)
}

fn pad_frames(route: &str, cursors: &[usize]) -> Result<Vec<BTreeSet<String>>, String> {
    let max_poll = *cursors.last().ok_or("empty route trace")?;
    let mut samples = vec![BTreeSet::new(); max_poll + 1];
    for event in route.trim().split(',') {
        let mut fields = event.split(':');
        let poll = fields
            .next()
            .ok_or("missing poll")?
            .parse::<usize>()
            .map_err(|e| e.to_string())?;
        let button = fields.next().ok_or("missing button")?;
        let hold = fields
            .next()
            .ok_or("missing hold")?
            .parse::<usize>()
            .map_err(|e| e.to_string())?;
        if fields.next().is_some()
            || hold == 0
            || poll == 0
            || ![
                "cross", "circle", "square", "triangle", "start", "select", "up", "down", "left",
                "right", "l1", "r1", "l2", "r2",
            ]
            .contains(&button)
        {
            return Err(format!("invalid route event {event}"));
        }
        for sample in poll..poll.checked_add(hold).ok_or("poll overflow")? {
            samples
                .get_mut(sample)
                .ok_or("route event exceeds trace polls")?
                .insert(button.into());
        }
    }
    // A route CSV row is written at the completed tick boundary after the
    // frontend installs samples[tape_cursor]. That pad state is live until the
    // next cursor change, including ticks where the guest does not poll (for
    // example while loading a CD scene). A cursor jump may skip samples that
    // have no video-tick interval; do not invent a gate press for them.
    cursors
        .iter()
        .map(|&cursor| {
            samples
                .get(cursor)
                .cloned()
                .ok_or_else(|| format!("trace cursor {cursor} exceeds tape samples"))
        })
        .collect()
}

fn emit_segment(out: &mut String, active: &BTreeSet<String>, duration: usize) {
    if duration == 0 {
        return;
    }
    out.push_str("\n[[step]]\n");
    if active.is_empty() {
        out.push_str(&format!("wait = {duration}\n"));
    } else {
        let buttons = active
            .iter()
            .map(|b| format!("\"{b}\""))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!("press = [{buttons}]\nhold = {duration}\n"));
    }
}

fn render(route: &str, trace: &str) -> Result<String, String> {
    let frames = pad_frames(route, &tick_cursors(trace, MIN_END)?)?;
    let end = frames.len() - 1;
    let mut out = format!("# Generated by tools/gate_journey.rs from tools/tapes/journey-kings.route\n# and the exact final-disc route.csv. See scratch receipt for their hashes.\n# Invincibility is enabled for this traversal; zero deaths is not a survival check.\nname = \"hk-psx\"\ntitle = \"Hollow Knight: invincible King's Pass traversal and Dirtmouth save\"\ndisc = [\"build/gate/hk-psx.cue\"]\nsymbols = [\"build/hk-psx-normal.map\"]\nmax_ticks = {}\n", end + 80);
    let mut prior = 0;
    let mut active = BTreeSet::new();
    let mut next_check = 0;
    for tick in 0..=end {
        let checkpoint = CHECKPOINTS.get(next_check).filter(|c| c.0 == tick);
        let next_state = frames.get(tick).cloned().unwrap_or_default();
        if checkpoint.is_some() || next_state != active || tick == end {
            emit_segment(&mut out, &active, tick - prior);
            prior = tick;
            active = next_state;
        }
        if let Some((_, name, checks)) = checkpoint {
            out.push_str(&format!("\n[[step]]\ncheckpoint = \"{name}\"\n"));
            out.push_str("[[step.assert]]\nkind = \"not_flat\"\n");
            out.push_str("[[step.assert]]\nkind = \"pc_not_stuck\"\nwindow = 60\n");
            for (symbol, value) in *checks {
                out.push_str(&format!("[[step.assert]]\nkind = \"ram\"\nsym = \"{symbol}\"\nop = \"eq\"\nvalue = {value}\n"));
            }
            next_check += 1;
        }
    }
    Ok(out)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let route = args
        .next()
        .ok_or("usage: gate_journey ROUTE FINAL_ROUTE_CSV")?;
    let trace = args
        .next()
        .ok_or("usage: gate_journey ROUTE FINAL_ROUTE_CSV")?;
    print!(
        "{}",
        render(&fs::read_to_string(route)?, &fs::read_to_string(trace)?)?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cursor_mapping_holds_state_across_no_poll_ticks() {
        let cursors = tick_cursors(
            "route_tick,tape_frame,other\n0,0,0\n1,1,0\n2,1,0\n3,1,0\n4,2,0\n",
            4,
        )
        .unwrap();
        let frames = pad_frames("1:right:1", &cursors).unwrap();
        assert!(frames[1].contains("right"));
        assert!(frames[2].contains("right"));
        assert!(frames[3].contains("right"));
        assert!(!frames[4].contains("right"));
        assert!(!frames[4].contains("right"));
    }
    #[test]
    fn overlapping_buttons_and_boundaries_are_retained() {
        let map = (0..=MIN_END).collect::<Vec<_>>();
        let frames = pad_frames("9:start:1,149:right:3,150:cross:1", &map).unwrap();
        assert!(frames[9].contains("start"));
        assert!(frames[149].contains("right"));
        assert!(frames[150].contains("cross"));
        assert!(frames[150].contains("right"));
        assert!(!frames[152].contains("right"));
    }
    #[test]
    fn skipped_cursor_gets_no_invented_gate_tick() {
        let cursors =
            tick_cursors("route_tick,tape_frame,other\n0,0,0\n1,2,0\n2,3,0\n", 2).unwrap();
        let frames = pad_frames("1:cross:1,2:right:1", &cursors).unwrap();
        assert!(!frames.iter().any(|state| state.contains("cross")));
        assert!(frames[1].contains("right"));
        assert!(!frames[2].contains("right"));
    }
    #[test]
    fn missing_video_tick_fails_closed() {
        assert!(tick_cursors("route_tick,tape_frame,other\n0,0,0\n2,1,0\n", 2).is_err());
    }
    #[test]
    fn short_trace_fails_closed() {
        assert!(tick_cursors("route_tick,tape_frame,other\n0,0,0\n1,1,0\n", 2).is_err());
    }
    #[test]
    fn initial_poll_press_needs_explicit_frontend_start_proof() {
        assert!(pad_frames("0:start:1", &[0, 1]).is_err());
    }
}
