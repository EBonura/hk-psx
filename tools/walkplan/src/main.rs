//! `hk-walkplan`: plans a walk through one room and prints it as a walk fragment (docs/ROUTES.md, "Planned walks").
//!
//! The room is its own collision edges (the cooked `data/regions/chunk_N.hk` packs of every region of the scene)
//! and the Knight is the guest's own `Player::step` with the guest's `PARAMS` (`data/params.rs`), so a hop that
//! works here is the hop the disc makes. The search is Dijkstra over grounded states: from each one the Knight
//! walks two ticks either way, walks off a ledge (holding the direction or not), or jumps (take-off direction,
//! ticks the jump is held, ticks the direction is held); the cost is ticks. A jump is kept only when a take-off a
//! sixth of a unit to either side still lands on the same floor, because the walk's own positioning is only that exact.
//!
//! ```text
//! hk-walkplan SCENE_NAME START_X START_Y GOAL_X GOAL_Y [--tol T] [--open X0,Y0,X1,Y1]...
//! ```
//!
//! Positions are the Knight's origin in world units (a floor at y 2 holds him at 3.39). `--tol` is how near the goal
//! counts (0.4). `--open` leaves out the edges inside a box: a gate or a breakable wall the walk is meant to find open
//! (the cooked edges hold every battle gate closed and every breakable standing).
//!
//! Not modelled: enemies, hazards, hard-landing locks (a walk waits for `HK_WALK_READY`), moving platforms, breakables
//! that the walk has not been told are open. The output is a start; the disc is the judge.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::path::{Path, PathBuf};

use hk_sim::{Player, ONE};

include!("../../../data/params.rs");

/// One collision edge in Q16 world units: x0, y0, x1, y1.
type Edge = [i32; 4];

/// How a state was reached from its parent.
#[derive(Clone, Copy)]
enum Act {
    Start,
    Walk { dir: i32 },
    Fall { dir: i32, hold: bool, ledge_x: i32 },
    Jump { dir: i32, held: u32, steer: u32, from_x: i32 },
}

struct Node {
    player: Player,
    cost: u32,
    parent: usize,
    act: Act,
}

fn units(q16: i32) -> f64 {
    f64::from(q16) / f64::from(ONE)
}

fn step(player: &mut Player, edges: &[Edge], dir: i32, jump: bool) {
    player.step(PARAMS, dir, jump, edges.len(), |i| edges[i]);
}

/// A state's cell: a quarter unit each way.
fn cell(player: &Player) -> (i32, i32) {
    ((units(player.x) * 4.0).round() as i32, (units(player.y) * 4.0).round() as i32)
}

/// The room's edges: every region of the scene, each edge once.
fn load_edges(root: &Path, scene: &str, open: &[[f64; 4]]) -> Result<Vec<Edge>, String> {
    let path = root.join("data/regions.json");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let regions = json["regions"].as_array().ok_or("regions.json has no regions")?;
    let mut edges: Vec<Edge> = Vec::new();
    let mut found = false;
    for region in regions.iter().filter(|r| r["scene_name"] == scene) {
        found = true;
        let file = region["path"].as_str().ok_or("region without a path")?;
        let bytes = std::fs::read(root.join(file)).map_err(|e| format!("{file}: {e}"))?;
        for edge in room_edges(&bytes).ok_or_else(|| format!("{file}: not an HKROOM02 pack"))? {
            if !edges.contains(&edge) {
                edges.push(edge);
            }
        }
    }
    if !found {
        return Err(format!("no scene named {scene}"));
    }
    edges.retain(|e| !inside_any(e, open));
    Ok(edges)
}

/// Whether an edge lies inside one of the boxes (x0, y0, x1, y1 in world units), with a third of a unit to spare.
fn inside_any(edge: &Edge, boxes: &[[f64; 4]]) -> bool {
    boxes.iter().any(|b| {
        let (xs, ys) = ([units(edge[0]), units(edge[2])], [units(edge[1]), units(edge[3])]);
        b[0] - 0.3 <= xs[0].min(xs[1])
            && xs[0].max(xs[1]) <= b[2] + 0.3
            && b[1] - 0.3 <= ys[0].min(ys[1])
            && ys[0].max(ys[1]) <= b[3] + 0.3
    })
}

/// The edge records of an `HKROOM02` pack (section 4 of `hk_format::Room`: 16 bytes each, after the textures,
/// draws, clips and frames).
fn room_edges(bytes: &[u8]) -> Option<Vec<Edge>> {
    if bytes.len() < 40 || &bytes[..8] != b"HKROOM02" {
        return None;
    }
    let word = |at: usize| i32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"));
    let count = |i: usize| word(8 + i * 4) as usize;
    let first = 40 + count(1) * 16 + count(2) * 44 + count(3) * 20 + count(4) * 16;
    if bytes.len() < first + count(5) * 16 {
        return None;
    }
    Some((0..count(5)).map(|i| [0, 1, 2, 3].map(|k| word(first + i * 16 + k * 4))).collect())
}

/// Where a jump lands: the state and the ticks it took.
fn simulate_jump(from: &Player, edges: &[Edge], dir: i32, held: u32, steer: u32) -> Option<(Player, u32)> {
    let mut player = *from;
    let mut airborne = false;
    for tick in 0..260u32 {
        let pad = if tick < steer { dir } else { 0 };
        step(&mut player, edges, pad, tick < held);
        airborne |= !player.grounded;
        if airborne && player.grounded {
            return Some((player, tick + 1));
        }
        if (!airborne && tick > 4) || units(player.y) < -30.0 {
            return None;
        }
    }
    None
}

/// The distance the Knight can walk away from a take-off (up to 0.8) and still stand on the same floor.
fn run_up(player: &Player, edges: &[Edge], away: i32) -> f64 {
    let mut now = *player;
    for _ in 0..6 {
        let mut next = now;
        step(&mut next, edges, away, false);
        if !next.grounded || (next.y - now.y).abs() > ONE / 8 || (next.x - now.x).abs() < ONE / 50 {
            break;
        }
        now = next;
    }
    units((now.x - player.x).abs())
}

struct Plan {
    nodes: Vec<Node>,
    goal: usize,
    expanded: usize,
}

fn search(edges: &[Edge], start: Player, goal: (f64, f64), tolerance: f64) -> Option<Plan> {
    const HELD: [u32; 9] = [5, 8, 12, 16, 20, 24, 28, 34, 42];
    const STEER: [u32; 11] = [0, 8, 16, 24, 32, 40, 52, 70, 100, 140, 260];
    let mut nodes = vec![Node { player: start, cost: 0, parent: 0, act: Act::Start }];
    let mut best: HashMap<(i32, i32), u32> = HashMap::from([(cell(&start), 0)]);
    let mut heap = BinaryHeap::from([Reverse((0u32, 0usize))]);
    let mut expanded = 0;
    while let Some(Reverse((cost, index))) = heap.pop() {
        if cost > nodes[index].cost {
            continue;
        }
        let player = nodes[index].player;
        if (units(player.x) - goal.0).abs() <= tolerance && (units(player.y) - goal.1).abs() <= 0.35 {
            return Some(Plan { nodes, goal: index, expanded });
        }
        expanded += 1;
        if expanded > 400_000 {
            return None;
        }
        let mut found: Vec<(Player, u32, Act)> = Vec::new();
        for dir in [-1, 1] {
            let mut next = player;
            step(&mut next, edges, dir, false);
            step(&mut next, edges, dir, false);
            if (next.x - player.x).abs() < ONE / 20 {
                continue;
            }
            if next.grounded {
                found.push((next, cost + 2, Act::Walk { dir }));
                continue;
            }
            // Walked off a ledge: keep the direction down, or let go at once.
            for hold in [true, false] {
                let (mut fall, mut ticks) = (next, 2);
                while !fall.grounded && ticks < 200 {
                    step(&mut fall, edges, if hold { dir } else { 0 }, false);
                    ticks += 1;
                }
                if fall.grounded && units(fall.y) > -30.0 {
                    found.push((fall, cost + ticks, Act::Fall { dir, hold, ledge_x: next.x }));
                }
            }
        }
        for dir in [-1, 0, 1] {
            for held in HELD {
                for steer in STEER {
                    let senseless = (dir == 0 && steer != 0) || (dir != 0 && steer != 0 && steer < held);
                    if senseless {
                        continue;
                    }
                    let Some((landed, ticks)) = simulate_jump(&player, edges, dir, held, steer) else { continue };
                    if (landed.x - player.x).abs() < ONE / 4 && (landed.y - player.y).abs() < ONE / 8 {
                        continue;
                    }
                    let steady = [-ONE / 7, ONE / 7].into_iter().all(|shift| {
                        let mut early = player;
                        early.x += shift;
                        simulate_jump(&early, edges, dir, held, steer).is_some_and(|(l, _)| {
                            (l.y - landed.y).abs() < ONE / 3 && (l.x - landed.x).abs() < ONE
                        })
                    });
                    if steady {
                        found.push((landed, cost + ticks + 1, Act::Jump { dir, held, steer, from_x: player.x }));
                    }
                }
            }
        }
        for (next, next_cost, act) in found {
            if best.get(&cell(&next)).is_none_or(|&b| next_cost < b) {
                best.insert(cell(&next), next_cost);
                nodes.push(Node { player: next, cost: next_cost, parent: index, act });
                heap.push(Reverse((next_cost, nodes.len() - 1)));
            }
        }
    }
    None
}

fn side(dir: i32) -> &'static str {
    if dir < 0 {
        "left"
    } else {
        "right"
    }
}

/// The walk fragment for a plan (see the header of `host/hk-build/walk.rs` for the directives).
fn print_walk(plan: &Plan, edges: &[Edge], start: &Player) {
    let mut path = Vec::new();
    let mut at = plan.goal;
    while at != 0 {
        path.push(at);
        at = plan.nodes[at].parent;
    }
    path.reverse();
    println!("at {:.2} {:.2}", units(start.x), units(start.y));
    let mut walking: Option<(i32, f64)> = None;
    let flush = |walking: &mut Option<(i32, f64)>| {
        if let Some((_, x)) = walking.take() {
            println!("walk {x:.2}");
        }
    };
    for &index in &path {
        let node = &plan.nodes[index];
        match node.act {
            Act::Start => {}
            Act::Walk { dir } => {
                if walking.is_some_and(|(d, _)| d != dir) {
                    flush(&mut walking);
                }
                walking = Some((dir, units(node.player.x)));
            }
            Act::Fall { dir, hold, ledge_x } => {
                flush(&mut walking);
                let x = if hold { units(node.player.x) } else { units(ledge_x) + 0.35 * f64::from(dir) };
                println!("drop {x:.2} {:.2}", units(node.player.y));
            }
            Act::Jump { dir, held, steer, from_x } => {
                flush(&mut walking);
                // Reversing at the take-off costs a jump its first ticks on the disc: come in moving the way it goes.
                if dir != 0 {
                    let run = run_up(&plan.nodes[node.parent].player, edges, -dir);
                    if run > 0.25 {
                        println!("walk {:.2}   # run-up", units(from_x) - (run - 0.05) * f64::from(dir));
                    }
                }
                println!("walk {:.2}   # take-off", units(from_x));
                if dir == 0 {
                    println!("hold cross for {held}");
                } else {
                    println!("hold {}+cross for {held}", side(dir));
                    if steer > held {
                        println!("hold {} for {}", side(dir), steer - held);
                    }
                }
                let y = units(node.player.y);
                println!("wait until y between {:.2} {:.2} timeout 240   # land at x {:.2}", y - 0.15, y + 0.15, units(node.player.x));
                println!("wait 6");
                println!("wait until HK_WALK_READY == 1 timeout 300");
            }
        }
    }
    flush(&mut walking);
}

fn usage() -> String {
    "usage: hk-walkplan SCENE_NAME START_X START_Y GOAL_X GOAL_Y [--tol T] [--open X0,Y0,X1,Y1]...".to_string()
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 5 {
        return Err(usage());
    }
    let number = |i: usize| args[i].parse::<f64>().map_err(|_| usage());
    let (start_xy, goal) = ((number(1)?, number(2)?), (number(3)?, number(4)?));
    let (mut tolerance, mut open) = (0.4, Vec::new());
    let mut rest = args[5..].iter();
    while let Some(flag) = rest.next() {
        let value = rest.next().ok_or_else(usage)?;
        match flag.as_str() {
            "--tol" => tolerance = value.parse().map_err(|_| usage())?,
            "--open" => {
                let b: Vec<f64> = value.split(',').filter_map(|v| v.parse().ok()).collect();
                open.push(<[f64; 4]>::try_from(b).map_err(|_| usage())?);
            }
            _ => return Err(usage()),
        }
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let edges = load_edges(&root, &args[0], &open)?;
    let q = |v: f64| (v * f64::from(ONE)).round() as i32;
    let mut start = Player::spawn(q(start_xy.0), q(start_xy.1 + 0.3));
    for _ in 0..200 {
        step(&mut start, &edges, 0, false);
        if start.grounded {
            break;
        }
    }
    if !start.grounded {
        return Err(format!("the start ({}, {}) is not over a floor", start_xy.0, start_xy.1));
    }
    let plan = search(&edges, start, goal, tolerance).ok_or("no path from the start to the goal (enemies, gates and breakables are not modelled; try --open)")?;
    eprintln!("{} states reached, {} ticks, {} expanded", plan.nodes.len(), plan.nodes[plan.goal].cost, plan.expanded);
    print_walk(&plan, &edges, &start);
    Ok(())
}

fn main() {
    if let Err(message) = run() {
        eprintln!("hk-walkplan: {message}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(v: f64) -> i32 {
        (v * f64::from(ONE)).round() as i32
    }

    /// A floor at y 2 from x -10 to 20 and a step block on it (x 5 to 8, top at y 4), as the cooked packs draw
    /// them: a top surface runs right to left.
    fn room() -> Vec<Edge> {
        let line = |a: (f64, f64), b: (f64, f64)| [q(a.0), q(a.1), q(b.0), q(b.1)];
        vec![
            line((20.0, 2.0), (8.0, 2.0)),
            line((8.0, 4.0), (8.0, 2.0)),
            line((8.0, 4.0), (5.0, 4.0)),
            line((5.0, 2.0), (5.0, 4.0)),
            line((5.0, 2.0), (-10.0, 2.0)),
        ]
    }

    fn standing(x: f64, edges: &[Edge]) -> Player {
        let mut player = Player::spawn(q(x), q(3.7));
        for _ in 0..100 {
            step(&mut player, edges, 0, false);
        }
        player
    }

    #[test]
    fn the_knight_stands_on_the_floor_at_the_height_the_walks_use() {
        let edges = room();
        let player = standing(0.0, &edges);
        assert!(player.grounded);
        assert!((units(player.y) - 3.39).abs() < 0.02, "{}", units(player.y));
    }

    #[test]
    fn a_step_block_is_climbed_and_the_walk_says_how() {
        let edges = room();
        let start = standing(0.0, &edges);
        let plan = search(&edges, start, (6.5, 5.39), 0.5).expect("a path onto the block");
        let landed = plan.nodes[plan.goal].player;
        assert!(landed.grounded && (units(landed.y) - 5.39).abs() < 0.1);
        assert!(plan.nodes.iter().any(|n| matches!(n.act, Act::Jump { .. })));
    }

    #[test]
    fn a_goal_behind_a_wall_that_is_too_high_has_no_path() {
        let mut edges = room();
        edges[2] = [q(8.0), q(40.0), q(5.0), q(40.0)];
        edges[1] = [q(8.0), q(40.0), q(8.0), q(2.0)];
        edges[3] = [q(5.0), q(2.0), q(5.0), q(40.0)];
        let start = standing(0.0, &edges);
        assert!(search(&edges, start, (15.0, 3.39), 0.5).is_none());
    }

    #[test]
    fn edges_inside_an_open_box_are_left_out() {
        let edges = room();
        let open = [[4.9, 1.9, 8.1, 4.1]];
        assert_eq!(edges.iter().filter(|e| inside_any(e, &open)).count(), 3);
    }
}
