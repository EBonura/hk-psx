//! What a chained fragment runs around a planned hop: the guard against a foe in its box, the landing test that takes the
//! whole landing floor, and the way back from where a miss ends (a lower floor, or the island a hazard respawn puts the
//! Knight on).
//!
//! The planner (`search`) knows the room's collision and `Player::step`, nothing else. A hop that is wide (a landing at least
//! `LAND_MARGIN` inside its floor on both sides, an apex `APEX_MARGIN` over the ledge top) is run by a loop that tries it
//! again from where it stands. A hop that is tight, and every hop whose miss ends on a floor it cannot simply walk back from,
//! also gets a recovery: `misses` flies the jump with the disturbances a player meets (a take-off off its mark, a pad a tick
//! short or long, a recoil in the air, a body in a hazard box), takes the floors those flights end on and the respawn markers a
//! hazard sends him to, and plans a path from each back to the hop's stand point with the same search that planned the room.
//! The recovery is a guarded block at the head of the hop's loop, so a pass that starts on that floor first climbs back.

use hk_sim::{Player, ONE};

use crate::{
    fly, floor_ends, search, standing, step, units, walk_lines, Disturbance, Edge, Ending, Tight,
    HIT_THROW, RECOIL,
};

/// States a search for the way back from a miss may expand: a way back is a few hops, not the room.
const RECOVERY_EXPANSIONS: usize = 1_500;
/// Floors a hop gets a recovery for at most: the first few distinct ones its disturbed flights end on (sloped ground makes many).
const MAX_MISS_FLOORS: usize = 4;

/// A jump as the planner wrote it on its landing line (`# land at x X jump DIR HELD STEER from X Y tight FLAGS`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JumpSpec {
    pub dir: i32,
    pub held: u32,
    pub steer: u32,
    /// Where the Knight stands before the jump (the take-off floor), world units.
    pub from: (f64, f64),
    pub tight: Tight,
}

impl JumpSpec {
    /// The spec in a line's comment, if it carries one.
    pub fn parse(line: &str) -> Option<JumpSpec> {
        let words: Vec<&str> = line.split('#').nth(1)?.split_whitespace().collect();
        let at = words.iter().position(|w| *w == "jump")?;
        let [dir, held, steer, "from", fx, fy, "tight", flags] = words.get(at + 1..at + 9)? else {
            return None;
        };
        Some(JumpSpec {
            dir: dir.parse().ok()?,
            held: held.parse().ok()?,
            steer: steer.parse().ok()?,
            from: (fx.parse().ok()?, fy.parse().ok()?),
            tight: Tight::from_flags(flags),
        })
    }
}

/// A hazard respawn marker: the box that moves it when the Knight's body touches it, and where it puts him.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Checkpoint {
    pub bounds: [f64; 4],
    pub spawn: (f64, f64),
}

/// The room a hop is judged in.
pub struct Room<'a> {
    pub edges: &'a [Edge],
    pub checkpoints: &'a [Checkpoint],
    /// Where the room starts: the respawn marker until a checkpoint box is touched.
    pub arrival: (f64, f64),
}

/// A floor a miss ends on: its height and its extent, and a standing Knight on it.
#[derive(Clone, Copy, Debug)]
pub struct Floor {
    pub y: f64,
    pub lo: f64,
    pub hi: f64,
    pub on: Player,
}

impl Floor {
    pub fn of(edges: &[Edge], player: Player) -> Floor {
        let (left, right) = floor_ends(edges, &player);
        Floor { y: units(player.y), lo: units(player.x) - left, hi: units(player.x) + right, on: player }
    }

    /// True when the same floor: the same height and an overlap.
    fn same(&self, other: &Floor) -> bool {
        (self.y - other.y).abs() < 0.3 && self.lo <= other.hi + 0.1 && other.lo <= self.hi + 0.1
    }

    /// The test that the Knight stands on this floor, as a gate expression.
    pub fn expr(&self, q: &impl Fn(f64) -> i64) -> String {
        format!(
            "i32(HK_PLAYER_X) >= {} && i32(HK_PLAYER_X) <= {} && abs(i32(HK_PLAYER_Y) - {}) <= {}",
            q(self.lo - 0.05),
            q(self.hi + 0.05),
            q(self.y),
            q(0.3)
        )
    }
}

fn in_box(player: &Player, b: &[f64; 4], half_width: f64) -> bool {
    let (x, y) = (units(player.x), units(player.y));
    x + half_width > b[0] && x - half_width < b[2] && y + 1.5 > b[1] && y - 0.2 < b[3]
}

/// The respawn marker the Knight carries after a jump from `spec` flies as planned: the one before it, or the last checkpoint
/// box its body touches on the way.
pub fn marker_after(room: &Room, edges: &[Edge], spec: &JumpSpec, before: (f64, f64)) -> (f64, f64) {
    let Ok(start) = standing(edges, spec.from.0, spec.from.1) else { return before };
    let mut player = start;
    let mut marker = before;
    for tick in 0..260u32 {
        let pad = if tick < spec.steer { spec.dir } else { 0 };
        step(&mut player, edges, pad, tick < spec.held);
        for c in room.checkpoints {
            if in_box(&player, &c.bounds, 0.25) {
                marker = c.spawn;
            }
        }
        if tick > 4 && player.grounded {
            break;
        }
    }
    marker
}

/// The floors a hop can end on instead of its landing: every flight with a take-off off its mark, a pad a tick short or
/// long and a recoil in the air that does not come down on `target` or on the take-off floor, and the island of each
/// respawn marker in `markers` that a flight into a hazard box would send him to.
pub fn misses(room: &Room, spec: &JumpSpec, target: Floor, markers: &[(f64, f64)]) -> Vec<Floor> {
    let edges = room.edges;
    let Ok(stand) = standing(edges, spec.from.0, spec.from.1) else { return Vec::new() };
    let home = Floor::of(edges, stand);
    let mut out: Vec<Floor> = Vec::new();
    let mut note = |floor: Floor, out: &mut Vec<Floor>| {
        if out.len() < MAX_MISS_FLOORS && !floor.same(&target) && !floor.same(&home) && !out.iter().any(|f| f.same(&floor)) {
            out.push(floor);
        }
    };
    let mut hazard = false;
    let shift_q = |u: f64| (u * f64::from(ONE)).round() as i32;
    let recoil = shift_q(RECOIL);
    // A foe's hit throws him this far sideways (`recoil_speed` over `recoil_ticks` of `VITAL_PARAMS`, 3 units).
    let hit = shift_q(HIT_THROW);
    for shift in [0.0, -0.3, 0.3] {
        let mut from = stand;
        from.x += shift_q(shift);
        step(&mut from, edges, 0, false);
        if !from.grounded {
            continue;
        }
        for dheld in [-1i32, 0, 1] {
            let held = (spec.held as i32 + dheld).max(2) as u32;
            let steer = if spec.dir == 0 { 0 } else { (spec.steer as i32 + dheld).max(held as i32) as u32 };
            // Disturbances: none, then the recoil of a hit a third and two thirds of the way through the flight, either way.
            let mut shoves = vec![None];
            for at in [6u32, 14, 24, 36] {
                for dx in [-recoil, recoil, -hit, hit] {
                    shoves.push(Some((at, dx)));
                }
            }
            for shove in shoves {
                match fly(&from, edges, spec.dir, held, steer, Disturbance { shove }) {
                    Ending::Landed(flight) => note(Floor::of(edges, flight.player), &mut out),
                    Ending::Hazard => hazard = true,
                    Ending::Lost => {}
                }
            }
        }
    }
    if hazard {
        for &(x, y) in markers {
            if let Ok(on) = standing(edges, x, y) {
                note(Floor::of(edges, on), &mut out);
            }
        }
    }
    out
}

/// The guarded recovery blocks for `spec`: for each floor a miss can end on, a block that runs when the Knight stands on it
/// and walks him back to the stand point of the hop (a plan with the room's own search), `none` for a floor with no way back.
/// Each is `if EXPR` ... `end`, the lines of the plan between.
pub fn recoveries(room: &Room, name: &str, spec: &JumpSpec, target: Floor, markers: &[(f64, f64)], q: &impl Fn(f64) -> i64) -> Vec<String> {
    // The same hop is asked for again every round of the room planner (and twice, play and ship version): remember it.
    let key = format!("{name} {spec:?} {:.2} {:.2} {:.2} {markers:?}", target.y, target.lo, target.hi);
    if let Some(done) = RECOVERIES.with(|c| c.borrow().get(&key).cloned()) {
        return done;
    }
    let out = recoveries_uncached(room, name, spec, target, markers, q);
    RECOVERIES.with(|c| c.borrow_mut().insert(key, out.clone()));
    out
}

thread_local! {
    static RECOVERIES: std::cell::RefCell<std::collections::HashMap<String, Vec<String>>> = std::cell::RefCell::new(std::collections::HashMap::new());
}

fn recoveries_uncached(room: &Room, name: &str, spec: &JumpSpec, target: Floor, markers: &[(f64, f64)], q: &impl Fn(f64) -> i64) -> Vec<String> {
    let mut out = Vec::new();
    for floor in misses(room, spec, target, markers).iter() {
        crate::EXPAND_LIMIT.with(|l| l.set(RECOVERY_EXPANSIONS));
        let found = search(room.edges, floor.on, spec.from, 0.5, true);
        crate::EXPAND_LIMIT.with(|l| l.set(400_000));
        let Some(plan) = found else {
            eprintln!("{name}: no way back from the floor at y {:.2}, x {:.2} to {:.2}", floor.y, units(floor.on.x), spec.from.0);
            continue;
        };
        let lines = walk_lines(&plan, room.edges, &floor.on);
        out.push(format!("if expr {}   # {name}: back from the floor at y {:.2} (x {:.1} to {:.1})", floor.expr(q), floor.y, floor.lo, floor.hi));
        out.extend(lines.into_iter().filter(|l| !l.starts_with("at ") && !l.starts_with("wait until y between")));
        out.push("end".to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_landing_line_gives_back_the_jump_it_describes() {
        let line = "wait until y between 5.24 5.54 timeout 240   # land at x 16.50 jump 1 34 37 from 35.650 13.390 tight edge,apex";
        let spec = JumpSpec::parse(line).expect("a spec");
        assert_eq!((spec.dir, spec.held, spec.steer), (1, 34, 37));
        assert_eq!(spec.from, (35.65, 13.39));
        assert!(spec.tight.edge && spec.tight.apex && !spec.tight.fragile);
        assert!(JumpSpec::parse("wait until y between 5 6 timeout 240   # land at x 16.50").is_none());
    }

    fn q16(v: f64) -> i32 {
        (v * f64::from(ONE)).round() as i32
    }

    fn line(a: (f64, f64), b: (f64, f64)) -> Edge {
        [q16(a.0), q16(a.1), q16(b.0), q16(b.1)]
    }

    /// A start ledge G (y 2.5, x -10 to 4), a pit under the gap (floor y 0, x 4 to 9), and a far ledge T (y 2.5, x 9 to 20):
    /// a jump across the gap that falls short lands in the pit, from where one jump up the pit's wall is the way back.
    fn pit() -> Vec<Edge> {
        vec![
            line((4.0, 2.5), (-10.0, 2.5)),
            line((4.0, 2.5), (4.0, 0.0)),
            line((9.0, 0.0), (4.0, 0.0)),
            line((9.0, 0.0), (9.0, 2.5)),
            line((20.0, 2.5), (9.0, 2.5)),
            line((20.0, 2.5), (20.0, 0.0)),
        ]
    }

    fn across() -> JumpSpec {
        JumpSpec { dir: 1, held: 30, steer: 60, from: (3.6, 3.89), tight: Tight::default() }
    }

    /// The same hop with a jump that only just reaches the far ledge: a shove or a short pad drops it in the pit.
    fn short() -> JumpSpec {
        JumpSpec { held: 20, steer: 44, ..across() }
    }

    #[test]
    fn a_hop_over_a_gap_can_miss_onto_the_pit_floor_and_the_way_back_is_planned() {
        let edges = pit();
        let room = Room { edges: &edges, checkpoints: &[], arrival: (0.0, 3.89) };
        let target = Floor::of(&edges, standing(&edges, 12.0, 3.89).expect("far ledge"));
        let q = |v: f64| (v * 65536.0).round() as i64;
        let floors = misses(&room, &short(), target, &[]);
        assert!(floors.iter().any(|f| (f.y - 1.39).abs() < 0.1), "a flight that falls short ends in the pit: {floors:?}");
        let blocks = recoveries(&room, "x-t-h001", &short(), target, &[], &q);
        assert!(blocks.first().is_some_and(|l| l.starts_with("if expr i32(HK_PLAYER_X) >= ")), "{blocks:?}");
        assert_eq!(blocks.last().map(String::as_str), Some("end"));
        assert!(blocks.iter().any(|l| l.starts_with("hold ") && l.contains("+cross")), "the way out of the pit is a jump: {blocks:?}");
        assert!(!blocks.iter().any(|l| l.starts_with("at ")), "{blocks:?}");
    }

    #[test]
    fn a_respawn_marker_that_a_hazard_sends_him_to_is_a_node_with_a_way_back() {
        let edges = pit();
        // Spikes over the pit floor: a flight that falls into the pit touches them.
        crate::set_hazards(vec![line((4.0, 0.0), (9.0, 1.2))]);
        let room = Room { edges: &edges, checkpoints: &[], arrival: (0.0, 3.89) };
        let target = Floor::of(&edges, standing(&edges, 12.0, 3.89).expect("far ledge"));
        // The marker is on the far ledge, where it can only be left again by going back over the gap.
        let marker = (15.0, 3.89);
        let floors = misses(&room, &short(), target, &[marker]);
        crate::set_hazards(Vec::new());
        // The marker's floor is the target floor here (a respawn there is a landing as planned): nothing to recover from.
        assert!(floors.iter().all(|f| !f.same(&target)), "{floors:?}");
        // On the pit floor a marker is a miss floor of its own.
        crate::set_hazards(vec![line((5.0, 0.0), (8.0, 1.2))]);
        let floors = misses(&room, &short(), target, &[(6.5, 1.39)]);
        crate::set_hazards(Vec::new());
        assert!(floors.iter().any(|f| (f.y - 1.39).abs() < 0.1), "the island of the marker is a floor with a way back: {floors:?}");
    }

    #[test]
    fn the_marker_after_a_hop_is_the_last_checkpoint_box_its_flight_touches() {
        let edges = pit();
        let room = Room {
            edges: &edges,
            checkpoints: &[Checkpoint { bounds: [10.0, 2.5, 18.0, 9.0], spawn: (14.0, 3.89) }],
            arrival: (0.0, 3.89),
        };
        assert_eq!(marker_after(&room, &edges, &across(), (0.0, 3.89)), (14.0, 3.89));
        let elsewhere = Room { checkpoints: &[Checkpoint { bounds: [30.0, 2.5, 40.0, 9.0], spawn: (35.0, 3.89) }], ..room };
        assert_eq!(marker_after(&elsewhere, &edges, &across(), (0.0, 3.89)), (0.0, 3.89));
    }
}
