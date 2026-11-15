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
//! hk-walkplan SCENE_NAME START_X START_Y GOAL_X GOAL_Y [--tol T] [--open X0,Y0,X1,Y1]... [--loose] [--reach] [--no-hazards] [--switched]
//! ```
//!
//! Positions are the Knight's origin in world units (a floor at y 2 holds him at 3.39). `--tol` is how near the goal
//! counts (0.4). `--open` leaves out the edges inside a box: a gate or a breakable wall the walk is meant to find open
//! (the cooked edges hold every battle gate closed and every breakable standing). `--loose` keeps jumps that only land
//! from the exact take-off x (the default drops a jump unless a take-off 0.3 of a unit either side (`HK_WALKPLAN_MARGIN`) lands on the same
//! floor, with floor under it); `--reach` prints how high and far a jump goes at each hold length. The scene's hazards (acid, spikes, from
//! the cooked region list) are never walked or fallen into; `--no-hazards` ignores them.
//!
//! Not modelled: enemies, hazards, hard-landing locks (a walk waits for `HK_WALK_READY`), moving platforms, breakables
//! that the walk has not been told are open. The output is a start; the disc is the judge.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::path::{Path, PathBuf};

mod hop;
mod room;

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
    Jump { dir: i32, held: u32, steer: u32, from_x: i32, tight: Tight },
}

/// A nail hit's recoil carries the Knight this far (world units; docs/ROUTES.md, 2026-11-15): a landing is wide when it
/// stands at least the recoil plus one walking step inside the floor on both sides, so a hit at the landing does not carry
/// him off.
pub const RECOIL: f64 = 0.7;
/// How far a foe's hit throws the Knight sideways (world units): `recoil_speed` 983040 for `recoil_ticks` 12 of `VITAL_PARAMS`.
pub const HIT_THROW: f64 = 3.0;
/// How far the Knight walks in one tick (world units).
pub const WALK_STEP: f64 = 0.138;
/// The room a landing wants on each side (world units).
pub const LAND_MARGIN: f64 = RECOIL + WALK_STEP;
/// How far (world units) the apex of a jump onto a ledge must clear the ledge top (the landing height): one tick less of
/// holding the button must still reach it.
pub const APEX_MARGIN: f64 = 0.4;
/// Ticks of cost a tight hop pays over a wide one, and per world unit of the margin it lacks.
const TIGHT_PENALTY: u32 = 400;
const TIGHT_SLOPE: f64 = 150.0;

/// Why a planned jump is not steady: the hops that are, are planned in a plain way; a tight one gets a closed-loop recipe
/// (`hop.rs`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tight {
    /// The landing is closer than `LAND_MARGIN` to an end of its floor (or the floor is narrower than two margins).
    pub edge: bool,
    /// The apex clears the ledge it lands on by less than `APEX_MARGIN`.
    pub apex: bool,
    /// The take-off is steady only within the fine margin (`HK_WALKPLAN_FINE`).
    pub fragile: bool,
}

impl Tight {
    pub fn any(self) -> bool {
        self.edge || self.apex || self.fragile
    }

    /// `edge,apex,fragile` (those that hold), or `-`.
    pub fn flags(self) -> String {
        let all = [(self.edge, "edge"), (self.apex, "apex"), (self.fragile, "fragile")];
        let named: Vec<&str> = all.iter().filter(|(on, _)| *on).map(|(_, n)| *n).collect();
        if named.is_empty() { "-".to_string() } else { named.join(",") }
    }

    pub fn from_flags(text: &str) -> Tight {
        Tight { edge: text.contains("edge"), apex: text.contains("apex"), fragile: text.contains("fragile") }
    }
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

thread_local! {
    /// The room's damaging boxes (acid, spikes; Q16, x0 y0 x1 y1) and whether the Knight's body touched one since the
    /// last `clear_touch`. A plan never goes through one: the disc would cost a mask and put him back on a ledge.
    static HAZARDS: std::cell::RefCell<Vec<Edge>> = const { std::cell::RefCell::new(Vec::new()) };
    static TOUCHED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// How far (Q16) a take-off may be off its mark and still land on the same floor: `HK_WALKPLAN_MARGIN` in world units
/// (0.3 of a unit by default; the walk's own positioning is about a tenth to a fifth off on the disc).
fn margin() -> i32 {
    static MARGIN: std::sync::OnceLock<i32> = std::sync::OnceLock::new();
    *MARGIN.get_or_init(|| {
        let units = std::env::var("HK_WALKPLAN_MARGIN").ok().and_then(|v| v.parse::<f64>().ok());
        units.map_or(ONE * 3 / 10, |u| (u * f64::from(ONE)).round() as i32)
    })
}

/// The close-to-the-mark margin a fragile jump may still be planned at (`HK_WALKPLAN_FINE`, world units), if any.
fn fine_margin() -> Option<i32> {
    static FINE: std::sync::OnceLock<Option<i32>> = std::sync::OnceLock::new();
    *FINE.get_or_init(|| {
        std::env::var("HK_WALKPLAN_FINE").ok().and_then(|v| v.parse::<f64>().ok()).map(|u| (u * f64::from(ONE)).round() as i32)
    })
}

/// Extra ticks of cost for a jump that is steady only within the fine margin.
const FRAGILE_PENALTY: u32 = 600;

/// A jump the plan must not take again: where it starts (a cell) and the pad it is given. A hop that missed on the disc,
/// even with its retries, is banned and the room is planned again from the same place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ban {
    pub cell: (i32, i32),
    pub dir: i32,
    pub held: u32,
    pub steer: u32,
}

thread_local! {
    /// How many states a search may expand before it gives up: the room's own plan gets 400,000; the way back from a floor a miss
    /// can end on gets far fewer (`hop.rs`), since a floor with no way back would otherwise be searched through whole.
    pub(crate) static EXPAND_LIMIT: std::cell::Cell<usize> = const { std::cell::Cell::new(400_000) };
}

thread_local! {
    static BANNED: std::cell::RefCell<Vec<Ban>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn set_banned(bans: Vec<Ban>) {
    BANNED.with(|b| *b.borrow_mut() = bans);
}

fn is_banned(player: &Player, dir: i32, held: u32, steer: u32) -> bool {
    BANNED.with(|b| b.borrow().iter().any(|x| x.cell == cell(player) && x.dir == dir && x.held == held && x.steer == steer))
}

thread_local! {
    /// A gate's trigger box (Q16): a state whose body overlaps it has reached the goal, whatever the goal point says.
    /// A vertical exit is taken by touching the box from below or above, not by standing at its middle.
    static GOAL_BOX: std::cell::Cell<Option<Edge>> = const { std::cell::Cell::new(None) };
}

fn set_goal_box(bounds: Option<[f64; 4]>) {
    let q = |v: f64| (v * f64::from(ONE)).round() as i32;
    GOAL_BOX.with(|g| g.set(bounds.map(|b| [q(b[0]), q(b[1]), q(b[2]), q(b[3])])));
}

fn touches_goal_box(player: &Player) -> bool {
    GOAL_BOX.with(|g| {
        g.get().is_some_and(|b| {
            let body = [player.x - PARAMS.half_width, player.y + PARAMS.bottom, player.x + PARAMS.half_width, player.y + PARAMS.top];
            body[0] < b[2] && body[2] > b[0] && body[1] < b[3] && body[3] > b[1]
        })
    })
}

fn in_goal_box(player: &Player) -> bool {
    player.grounded && touches_goal_box(player)
}

fn set_hazards(boxes: Vec<Edge>) {
    HAZARDS.with(|h| *h.borrow_mut() = boxes);
}

fn clear_touch() {
    TOUCHED.with(|t| t.set(false));
}

fn touched() -> bool {
    TOUCHED.with(|t| t.get())
}

/// The scene's hazard boxes, from the cooked region list (each box once).
fn load_hazards(root: &Path, scene: &str) -> Result<Vec<Edge>, String> {
    let path = root.join("data/regions.json");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let q = |v: &serde_json::Value| (v.as_f64().unwrap_or(0.0) * f64::from(ONE)).round() as i32;
    let mut boxes: Vec<Edge> = Vec::new();
    for region in json["regions"].as_array().into_iter().flatten().filter(|r| r["scene_name"] == scene) {
        for hazard in region["hazards"].as_array().into_iter().flatten() {
            let b = &hazard["bounds"];
            let boxed = [q(&b[0]), q(&b[1]), q(&b[2]), q(&b[3])];
            if !boxes.contains(&boxed) {
                boxes.push(boxed);
            }
        }
    }
    Ok(boxes)
}

fn step(player: &mut Player, edges: &[Edge], dir: i32, jump: bool) {
    player.step(PARAMS, dir, jump, edges.len(), |i| edges[i]);
    HAZARDS.with(|h| {
        let body = [player.x - PARAMS.half_width, player.y + PARAMS.bottom, player.x + PARAMS.half_width, player.y + PARAMS.top];
        if h.borrow().iter().any(|b| body[0] < b[2] && body[2] > b[0] && body[1] < b[3] && body[3] > b[1]) {
            TOUCHED.with(|t| t.set(true));
        }
    });
}

/// A state's cell: a quarter unit each way.
pub(crate) fn cell(player: &Player) -> (i32, i32) {
    ((units(player.x) * 4.0).round() as i32, (units(player.y) * 4.0).round() as i32)
}

/// The room's edges: every region of the scene, each edge once.
fn load_edges(root: &Path, scene: &str, open: &[[f64; 4]]) -> Result<Vec<Edge>, String> {
    let path = root.join("data/regions.json");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let regions = json["regions"].as_array().ok_or("regions.json has no regions")?;
    let mut edges: Vec<Edge> = Vec::new();
    // An edge a switch takes out of the room is out wherever it appears: the regions of a scene overlap, so a
    // neighbouring pack holds the same collider again without the binding.
    let mut blanked: Vec<Edge> = Vec::new();
    let mut found = false;
    for region in regions.iter().filter(|r| r["scene_name"] == scene) {
        found = true;
        let file = region["path"].as_str().ok_or("region without a path")?;
        let bytes = std::fs::read(root.join(file)).map_err(|e| format!("{file}: {e}"))?;
        let blank = linked_edges(region);
        let sources = region["edge_sources"].as_array();
        for (index, edge) in room_edges(&bytes).ok_or_else(|| format!("{file}: not an HKROOM02 pack"))?.into_iter().enumerate() {
            // HK_WALKPLAN_SOURCES=x0,y0,x1,y1: the scene object that owns each edge touching the box (world units).
            if let Some(b) = std::env::var("HK_WALKPLAN_SOURCES").ok().map(|v| v.split(',').filter_map(|n| n.parse::<f64>().ok()).collect::<Vec<_>>()).filter(|b| b.len() == 4) {
                let (x0, y0, x1, y1) = (units(edge[0]), units(edge[1]), units(edge[2]), units(edge[3]));
                if x0.max(x1) >= b[0] && x0.min(x1) <= b[2] && y0.max(y1) >= b[1] && y0.min(y1) <= b[3] {
                    let source = sources.and_then(|s| s.get(index)).and_then(|v| v.as_str()).unwrap_or("?");
                    eprintln!("source {source}: edge ({x0:.2}, {y0:.2}) -> ({x1:.2}, {y1:.2}) in {file}");
                }
            }
            if blank.contains(&index) {
                blanked.push(edge);
            } else if !edges.contains(&edge) {
                edges.push(edge);
            }
        }
    }
    if !found {
        return Err(format!("no scene named {scene}"));
    }
    edges.retain(|e| !blanked.contains(e));
    edges.retain(|e| !inside_any(e, open));
    // HK_WALKPLAN_EDGES=x0,y0,x1,y1 lists the room's edges (world units) that touch the box: what the planner stands on.
    if let Some(b) = std::env::var("HK_WALKPLAN_EDGES")
        .ok()
        .map(|v| v.split(',').filter_map(|n| n.parse::<f64>().ok()).collect::<Vec<_>>())
        .filter(|b| b.len() == 4)
    {
        for e in &edges {
            let (x0, y0, x1, y1) = (units(e[0]), units(e[1]), units(e[2]), units(e[3]));
            if x0.max(x1) >= b[0] && x0.min(x1) <= b[2] && y0.max(y1) >= b[1] && y0.min(y1) <= b[3] {
                eprintln!("edge ({x0:.2}, {y0:.2}) -> ({x1:.2}, {y1:.2})");
            }
        }
    }
    Ok(edges)
}

/// The edge indices of one region that the switches (host/world_links.py, docs/WORLD_LINKS.md) take out of the
/// room as the report leaves it: the `Activated Plat` of a vine platform is absent until its vine is cut. With
/// `--switched` (or HK_WALKPLAN_SWITCHED=1, which the room planner reads too) every switch is taken as hit: the
/// gates it opens are out and the platforms are in.
fn linked_edges(region: &serde_json::Value) -> Vec<usize> {
    // HK_WALKPLAN_SWITCHED: 1 takes every switch as hit, a list of state indices (`119,115`) only those.
    let setting = std::env::var("HK_WALKPLAN_SWITCHED").unwrap_or_default();
    let listed: Vec<u64> = setting.split(',').filter_map(|v| v.trim().parse().ok()).collect();
    let mut blank = Vec::new();
    for binding in region["remote_art_bindings"].as_array().into_iter().flatten() {
        if binding.get("state_scene").is_none() {
            continue;
        }
        let switched = setting == "1"
            || binding["state_index"].as_u64().is_some_and(|i| listed.contains(&i));
        if binding["inverted"].as_bool().unwrap_or(false) != switched {
            blank.extend(binding["edge_indices"].as_array().into_iter().flatten().filter_map(|v| v.as_u64()).map(|v| v as usize));
        }
    }
    blank
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

/// Where a jump lands: the state, the ticks it took and the highest the Knight's origin got.
struct Flight {
    player: Player,
    ticks: u32,
    apex: i32,
}

/// What can happen to a jump on its way: a shove (a nail hit's recoil, a foe's knock) at one tick, a body in a hazard box.
#[derive(Clone, Copy, Default)]
struct Disturbance {
    /// The tick the Knight is moved sideways at, and by how much (Q16).
    shove: Option<(u32, i32)>,
}

/// How a flight ended when it did not land cleanly.
enum Ending {
    Landed(Flight),
    /// The body touched a hazard box on the way (the disc costs a mask and respawns him at the marker).
    Hazard,
    /// It never came down on a floor (the room's bottom, or a jump that did not leave the ground).
    Lost,
}

fn fly(from: &Player, edges: &[Edge], dir: i32, held: u32, steer: u32, shove: Disturbance) -> Ending {
    let mut player = *from;
    let (mut airborne, mut apex) = (false, player.y);
    clear_touch();
    for tick in 0..260u32 {
        let pad = if tick < steer { dir } else { 0 };
        if let Some((at, dx)) = shove.shove {
            if tick == at {
                player.x += dx;
            }
        }
        step(&mut player, edges, pad, tick < held);
        if touched() {
            return Ending::Hazard;
        }
        apex = apex.max(player.y);
        airborne |= !player.grounded;
        if airborne && player.grounded {
            return Ending::Landed(Flight { player, ticks: tick + 1, apex });
        }
        if (!airborne && tick > 4) || units(player.y) < -30.0 {
            return Ending::Lost;
        }
    }
    Ending::Lost
}

fn simulate_jump(from: &Player, edges: &[Edge], dir: i32, held: u32, steer: u32) -> Option<Flight> {
    match fly(from, edges, dir, held, steer, Disturbance::default()) {
        Ending::Landed(flight) => Some(flight),
        _ => None,
    }
}

/// The room (world units) a standing Knight has to the left and to the right on his floor: how far he can walk each way
/// before the floor ends, a wall stops him or the floor steps.
fn floor_ends(edges: &[Edge], player: &Player) -> (f64, f64) {
    let reach = |dir: i32| {
        let mut now = *player;
        for _ in 0..4000 {
            let mut next = now;
            step(&mut next, edges, dir, false);
            if !next.grounded || (next.y - now.y).abs() > ONE / 8 || (next.x - now.x).abs() < ONE / 50 {
                break;
            }
            now = next;
        }
        units((now.x - player.x).abs())
    };
    (reach(-1), reach(1))
}

/// Whether a landing with `left` and `right` of floor to either side is tight: it lacks `LAND_MARGIN` on a side. (A floor
/// narrower than two margins is tight wherever he lands; the plan prefers its middle by the cost below.)
fn landing_margin_lacks(left: f64, right: f64) -> f64 {
    (LAND_MARGIN - left.min(right)).max(0.0)
}

/// How much the apex of a jump onto a higher ledge lacks of `APEX_MARGIN` over the ledge top (the landing height); a jump
/// that does not climb lacks nothing.
fn apex_margin_lacks(from_y: i32, landed_y: i32, apex: i32) -> f64 {
    if units(landed_y - from_y) <= 0.3 {
        return 0.0;
    }
    (APEX_MARGIN - units(apex - landed_y)).max(0.0)
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

fn search(edges: &[Edge], start: Player, goal: (f64, f64), tolerance: f64, robust: bool) -> Option<Plan> {
    const HELD: [u32; 9] = [5, 8, 12, 16, 20, 24, 28, 34, 42];
    const STEER: [u32; 11] = [0, 8, 16, 24, 32, 40, 52, 70, 100, 140, 260];
    let mut nodes = vec![Node { player: start, cost: 0, parent: 0, act: Act::Start }];
    let mut best: HashMap<(i32, i32), u32> = HashMap::from([(cell(&start), 0)]);
    let mut heap = BinaryHeap::from([Reverse((0u32, 0usize))]);
    let mut expanded = 0;
    // The room a landing has either side, by cell: the same landing cells come up over and over.
    let mut extents: HashMap<(i32, i32), (f64, f64)> = HashMap::new(); // the floor's ends (world x) by landing cell
    while let Some(Reverse((cost, index))) = heap.pop() {
        if cost > nodes[index].cost {
            continue;
        }
        let player = nodes[index].player;
        if in_goal_box(&player) || (units(player.x) - goal.0).abs() <= tolerance && (units(player.y) - goal.1).abs() <= 0.35 {
            return Some(Plan { nodes, goal: index, expanded });
        }
        expanded += 1;
        if expanded > EXPAND_LIMIT.with(|l| l.get()) {
            report_nearest(&nodes, goal);
            return None;
        }
        let mut found: Vec<(Player, u32, Act)> = Vec::new();
        for dir in [-1, 1] {
            let mut next = player;
            clear_touch();
            step(&mut next, edges, dir, false);
            step(&mut next, edges, dir, false);
            if (next.x - player.x).abs() < ONE / 20 || touched() {
                continue;
            }
            if next.grounded {
                found.push((next, cost + 2, Act::Walk { dir }));
                continue;
            }
            // Walked off a ledge: keep the direction down, or let go at once.
            for hold in [true, false] {
                let (mut fall, mut ticks) = (next, 2);
                clear_touch();
                while !fall.grounded && ticks < 200 {
                    step(&mut fall, edges, if hold { dir } else { 0 }, false);
                    ticks += 1;
                    // A bottom exit is a gap in the floor: falling through its trigger takes it.
                    if touches_goal_box(&fall) {
                        fall.grounded = true;
                    }
                }
                if fall.grounded && units(fall.y) > -30.0 && !touched() {
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
                    let Some(Flight { player: landed, ticks, apex }) = simulate_jump(&player, edges, dir, held, steer) else { continue };
                    if is_banned(&player, dir, held.min(ticks), steer.min(ticks)) {
                        continue;
                    }
                    if (landed.x - player.x).abs() < ONE / 4 && (landed.y - player.y).abs() < ONE / 8 {
                        continue;
                    }
                    // A jump that lands on the same floor from take-offs `shift` either side of the mark.
                    let steady_within = |shift: i32| {
                        [-shift, shift].into_iter().all(|shift| {
                            let mut early = player;
                            early.x += shift;
                            // Off the mark there must still be floor under the take-off (a shifted state keeps its
                            // grounded flag, so settle it one tick first).
                            step(&mut early, edges, 0, false);
                            if !early.grounded {
                                return false;
                            }
                            simulate_jump(&early, edges, dir, held, steer).is_some_and(|f| {
                                (f.player.y - landed.y).abs() < ONE / 3 && (f.player.x - landed.x).abs() < ONE
                            })
                        })
                    };
                    // `HK_WALKPLAN_FINE` (world units) lets a room whose only way on is a fragile jump be planned: such a
                    // jump (steady only that close to its mark) costs `FRAGILE_PENALTY` ticks more, so a search never takes
                    // one while a steadier way exists.
                    let steady = !robust || steady_within(margin());
                    let fragile = !steady && fine_margin().is_some_and(|fine| steady_within(fine));
                    let steady = steady || fragile;
                    let mut extra = if fragile { FRAGILE_PENALTY } else { 0 };
                    // A landing (and an apex) with room to spare: a jump that lacks it costs more, in proportion to what it
                    // lacks, so a plan takes the wide one whenever the room has one; the tight ones that remain are marked.
                    let mut tight = Tight { fragile, ..Tight::default() };
                    if robust && steady && !touches_goal_box(&landed) {
                        let (lo, hi) = *extents.entry(cell(&landed)).or_insert_with(|| {
                            let (left, right) = floor_ends(edges, &landed);
                            (units(landed.x) - left, units(landed.x) + right)
                        });
                        let (left, right) = ((units(landed.x) - lo).max(0.0), (hi - units(landed.x)).max(0.0));
                        let (lack_edge, lack_apex) = (landing_margin_lacks(left, right), apex_margin_lacks(player.y, landed.y, apex));
                        tight.edge = lack_edge > 0.0;
                        tight.apex = lack_apex > 0.0;
                        for lack in [lack_edge, lack_apex] {
                            if lack > 0.0 {
                                extra += TIGHT_PENALTY + (lack * TIGHT_SLOPE) as u32;
                            }
                        }
                    }
                    // The pad is only held until the landing: a longer hold would walk on along the floor.
                    if steady {
                        found.push((landed, cost + ticks + 1 + extra, Act::Jump { dir, held: held.min(ticks), steer: steer.min(ticks), from_x: player.x, tight }));
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
    report_nearest(&nodes, goal);
    None
}

/// Where the search got closest to the goal when it found no path: the nearest reachable standing place (the next
/// obstacle to open, or the ledge to look at).
fn report_nearest(nodes: &[Node], goal: (f64, f64)) {
    let distance = |n: &Node| {
        let (dx, dy) = (units(n.player.x) - goal.0, units(n.player.y) - goal.1);
        dx * dx + dy * dy
    };
    if std::env::var_os("HK_WALKPLAN_DUMP").is_some() {
        for n in nodes {
            eprintln!("reached {:.2} {:.2}", units(n.player.x), units(n.player.y));
        }
    }
    if let Some(best) = nodes.iter().min_by(|a, b| distance(a).total_cmp(&distance(b))) {
        eprintln!(
            "hk-walkplan: nearest reachable place to the goal: x {:.2} y {:.2} ({} places reached)",
            units(best.player.x),
            units(best.player.y),
            nodes.len()
        );
    }
}

fn side(dir: i32) -> &'static str {
    if dir < 0 {
        "left"
    } else {
        "right"
    }
}

/// The walk fragment for a plan (see the header of `host/hk-build/walk.rs` for the directives), one line each.
fn walk_lines(plan: &Plan, edges: &[Edge], start: &Player) -> Vec<String> {
    let mut path = Vec::new();
    let mut at = plan.goal;
    while at != 0 {
        path.push(at);
        at = plan.nodes[at].parent;
    }
    path.reverse();
    let mut out = vec![format!("at {:.2} {:.2}", units(start.x), units(start.y))];
    let mut walking: Option<(i32, f64)> = None;
    let flush = |walking: &mut Option<(i32, f64)>, out: &mut Vec<String>| {
        if let Some((_, x)) = walking.take() {
            out.push(format!("walk {x:.2}"));
        }
    };
    for &index in &path {
        let node = &plan.nodes[index];
        match node.act {
            Act::Start => {}
            Act::Walk { dir } => {
                if walking.is_some_and(|(d, _)| d != dir) {
                    flush(&mut walking, &mut out);
                }
                walking = Some((dir, units(node.player.x)));
            }
            Act::Fall { dir, hold, ledge_x } => {
                flush(&mut walking, &mut out);
                let x = if hold { units(node.player.x) } else { units(ledge_x) + 0.35 * f64::from(dir) };
                out.push(format!("drop {x:.2} {:.2}", units(node.player.y)));
            }
            Act::Jump { dir, held, steer, from_x, tight } => {
                flush(&mut walking, &mut out);
                let from = &plan.nodes[node.parent].player;
                // Reversing at the take-off costs a jump its first ticks on the disc: come in moving the way it goes.
                if dir != 0 {
                    let run = run_up(from, edges, -dir);
                    // Stop the run-up 0.4 short of the edge or wall: the walk's positioning is a tenth to a fifth
                    // of a unit off, and a run-up that overshoots the floor walks the Knight off it.
                    if run > 0.6 {
                        out.push(format!("walk {:.2}   # run-up", units(from_x) - (run - 0.4) * f64::from(dir)));
                    }
                }
                out.push(format!("walk {:.2}   # take-off", units(from_x)));
                if dir == 0 {
                    out.push(format!("hold cross for {held}"));
                } else {
                    out.push(format!("hold {}+cross for {held}", side(dir)));
                    if steer > held {
                        out.push(format!("hold {} for {}", side(dir), steer - held));
                    }
                }
                let y = units(node.player.y);
                // The jump's own numbers ride on the landing line (`hop::JumpSpec::parse` reads them back): the stand it
                // starts from, its pad and what makes it tight. No tick count: the next line the walk waits for is the
                // ground (`HK_WALK_READY`), not a rest.
                out.push(format!(
                    "wait until y between {:.2} {:.2} timeout 240   # land at x {:.2} jump {dir} {held} {steer} from {:.3} {:.3} tight {}",
                    y - 0.15,
                    y + 0.15,
                    units(node.player.x),
                    units(from_x),
                    units(from.y),
                    tight.flags()
                ));
                out.push("wait until HK_WALK_READY == 1 timeout 300".to_string());
            }
        }
    }
    flush(&mut walking, &mut out);
    out
}

/// What a jump from the start reaches in open air (no walls): for each number of ticks the button is held, the apex
/// above the take-off and the horizontal distance by the time it is back at the take-off height with the direction held
/// throughout. The ceiling of the planner's reach, whatever the room.
fn print_reach(_edges: &[Edge], _start: &Player) {
    // A flat floor at y 0 under open air, so only the jump itself is measured.
    let floor = [[100 * ONE, 0, -100 * ONE, 0]];
    for held in [4, 8, 12, 16, 20, 24, 28, 34, 42, 60] {
        let mut player = Player::spawn(0, 2 * ONE);
        for _ in 0..60 {
            step(&mut player, &floor, 0, false);
        }
        let (ground, mut apex, mut ticks) = (player.y, 0, 0);
        loop {
            step(&mut player, &floor, 1, ticks < held);
            apex = apex.max(player.y - ground);
            ticks += 1;
            if (player.grounded && ticks > 3) || ticks > 400 {
                break;
            }
        }
        eprintln!(
            "reach: jump held {held:2} ticks: apex {:.2} above the take-off, {ticks} ticks in the air, {:.2} across (direction held)",
            units(apex),
            units(player.x)
        );
    }
}

fn usage() -> String {
    "usage: hk-walkplan SCENE_NAME START_X START_Y GOAL_X GOAL_Y [--tol T] [--open X0,Y0,X1,Y1]... [--loose] [--reach] [--no-hazards] [--switched]".to_string()
}

/// The Knight settled on the floor under (x, y), world units.
fn standing(edges: &[Edge], x: f64, y: f64) -> Result<Player, String> {
    let q = |v: f64| (v * f64::from(ONE)).round() as i32;
    let mut start = Player::spawn(q(x), q(y + 0.3));
    for _ in 0..200 {
        step(&mut start, edges, 0, false);
        if start.grounded {
            return Ok(start);
        }
    }
    Err(format!("the start ({x}, {y}) is not over a floor"))
}

/// How far the Knight can walk either way from (x, y) and stay on the same floor, world units: (left, right), with half
/// a unit kept back from each end. A retreat that stays inside it never walks off the platform or into a wall.
fn floor_span(edges: &[Edge], x: f64, y: f64) -> Result<(f64, f64), String> {
    let start = standing(edges, x, y)?;
    let reach = |dir: i32| {
        let mut now = start;
        for _ in 0..4000 {
            let mut next = now;
            step(&mut next, edges, dir, false);
            if !next.grounded || (next.y - now.y).abs() > ONE / 8 || (next.x - now.x).abs() < ONE / 50 {
                break;
            }
            now = next;
        }
        units(now.x)
    };
    let (left, right) = (reach(-1), reach(1));
    Ok(((left + 0.5).min(units(start.x)), (right - 0.5).max(units(start.x))))
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 5 {
        return Err(usage());
    }
    let number = |i: usize| args[i].parse::<f64>().map_err(|_| usage());
    let (start_xy, goal) = ((number(1)?, number(2)?), (number(3)?, number(4)?));
    let (mut tolerance, mut open, mut robust, mut reach, mut hazards) = (0.4, Vec::new(), true, false, true);
    let mut rest = args[5..].iter();
    while let Some(flag) = rest.next() {
        match flag.as_str() {
            "--loose" => {
                robust = false;
                continue;
            }
            "--reach" => {
                reach = true;
                continue;
            }
            "--no-hazards" => {
                hazards = false;
                continue;
            }
            "--switched" => {
                std::env::set_var("HK_WALKPLAN_SWITCHED", "1");
                continue;
            }
            _ => {}
        }
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
    if hazards {
        set_hazards(load_hazards(&root, &args[0])?);
    }
    let start = standing(&edges, start_xy.0, start_xy.1)?;
    if reach {
        print_reach(&edges, &start);
    }
    let plan = search(&edges, start, goal, tolerance, robust).ok_or("no path from the start to the goal (enemies, gates and breakables are not modelled; try --open)")?;
    eprintln!("{} states reached, {} ticks, {} expanded", plan.nodes.len(), plan.nodes[plan.goal].cost, plan.expanded);
    for line in walk_lines(&plan, &edges, &start) {
        println!("{line}");
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = if args.first().is_some_and(|a| a == "room") { room::run(&args[1..]) } else { run() };
    if let Err(message) = result {
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
        let plan = search(&edges, start, (6.5, 5.39), 0.5, true).expect("a path onto the block");
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
        assert!(search(&edges, start, (15.0, 3.39), 0.5, true).is_none());
    }

    #[test]
    fn the_floor_span_stops_at_a_wall_and_at_the_edge() {
        let edges = room();
        // Left of the step block the floor runs from x -10 to the wall at 5.
        let (lo, hi) = floor_span(&edges, 0.0, 3.39).expect("a floor");
        assert!((lo - -9.5).abs() < 0.4 && hi > 3.5 && hi < 5.0, "{lo} {hi}");
    }

    #[test]
    fn edges_inside_an_open_box_are_left_out() {
        let edges = room();
        let open = [[4.9, 1.9, 8.1, 4.1]];
        assert_eq!(edges.iter().filter(|e| inside_any(e, &open)).count(), 3);
    }

    /// A room of a floor at y 2 and one block (x `from` to `to`, top at `top`), as the cooked packs draw it.
    fn block_room(from: f64, to: f64, top: f64) -> Vec<Edge> {
        let line = |a: (f64, f64), b: (f64, f64)| [q(a.0), q(a.1), q(b.0), q(b.1)];
        vec![
            line((30.0, 2.0), (to, 2.0)),
            line((to, top), (to, 2.0)),
            line((to, top), (from, top)),
            line((from, 2.0), (from, top)),
            line((from, 2.0), (-10.0, 2.0)),
        ]
    }

    /// The jumps of a plan, in order.
    fn jumps(plan: &Plan) -> Vec<Act> {
        let mut out = Vec::new();
        let mut at = plan.goal;
        while at != 0 {
            if matches!(plan.nodes[at].act, Act::Jump { .. }) {
                out.push(plan.nodes[at].act);
            }
            at = plan.nodes[at].parent;
        }
        out.reverse();
        out
    }

    fn tightness(act: Act) -> Tight {
        match act {
            Act::Jump { tight, .. } => tight,
            _ => Tight::default(),
        }
    }

    #[test]
    fn a_landing_with_room_for_a_recoil_is_wide_and_a_narrow_ledge_is_marked_tight() {
        // A block 6 wide: a landing can stand a recoil and a step inside both ends of it.
        let wide = block_room(5.0, 11.0, 4.0);
        let start = standing(0.0, &wide);
        let plan = search(&wide, start, (8.0, 5.39), 3.0, true).expect("a path onto the block");
        let tight = tightness(*jumps(&plan).last().expect("a jump"));
        assert!(!tight.edge && !tight.apex, "{tight:?}");
        let landed = plan.nodes[plan.goal].player;
        let (left, right) = floor_ends(&wide, &landed);
        assert!(left >= LAND_MARGIN && right >= LAND_MARGIN, "{left} {right}");
        // A block a unit wide has no room for a recoil whatever the jump: the plan still lands on it, marked tight.
        let narrow = block_room(5.0, 6.0, 4.0);
        let start = standing(0.0, &narrow);
        let plan = search(&narrow, start, (5.5, 5.39), 0.5, true).expect("a path onto the block");
        let tight = tightness(*jumps(&plan).last().expect("a jump"));
        assert!(tight.edge && tight.any(), "{tight:?}");
    }

    #[test]
    fn a_wide_landing_is_taken_when_the_goal_allows_one() {
        // The goal is anywhere on the block's top: a plan that lands in its middle costs less than one that lands at the end.
        let block = block_room(5.0, 12.0, 4.0);
        let start = standing(0.0, &block);
        let plan = search(&block, start, (8.5, 5.39), 3.6, true).expect("a path onto the block");
        let landed = plan.nodes[plan.goal].player;
        let (left, right) = floor_ends(&block, &landed);
        assert!(left >= LAND_MARGIN && right >= LAND_MARGIN, "landed {:.2}: {left} {right}", units(landed.x));
    }

    #[test]
    fn a_jump_that_barely_clears_the_ledge_top_is_marked_for_its_apex() {
        // The highest a standing jump goes, then a ledge a tenth under it: any jump onto it clears it by less than the margin.
        let flat = block_room(5.0, 12.0, 4.0);
        let stand = standing(0.0, &flat);
        let apex = match fly(&stand, &flat, 0, 42, 0, Disturbance::default()) {
            Ending::Landed(flight) => units(flight.apex - stand.y),
            _ => panic!("a jump in the open lands"),
        };
        let top = 2.0 + apex - 0.1;
        let ledge = block_room(5.0, 12.0, top);
        let start = standing(0.0, &ledge);
        let plan = search(&ledge, start, (8.5, top + 1.39), 3.6, true).expect("a path onto the ledge");
        let tight = tightness(*jumps(&plan).last().expect("a jump"));
        assert!(tight.apex, "apex {apex:.2}, ledge {top:.2}: {tight:?}");
        assert!(apex_margin_lacks(stand.y, stand.y + q(top - 2.0), stand.y + q(apex)) > 0.0);
        assert_eq!(apex_margin_lacks(stand.y, stand.y, stand.y + q(apex)), 0.0, "a jump that does not climb lacks nothing");
    }

    #[test]
    fn a_banned_jump_is_not_planned_again() {
        let block = block_room(5.0, 12.0, 4.0);
        let start = standing(0.0, &block);
        let first = search(&block, start, (8.5, 5.39), 3.6, true).expect("a path");
        let act = *jumps(&first).first().expect("a jump");
        let Act::Jump { dir, held, steer, from_x, .. } = act else { unreachable!() };
        let node = first.nodes.iter().find(|n| matches!(n.act, Act::Jump { from_x: f, .. } if f == from_x)).expect("the node");
        let from = first.nodes[node.parent].player;
        set_banned(vec![Ban { cell: cell(&from), dir, held, steer }]);
        let second = search(&block, start, (8.5, 5.39), 3.6, true);
        set_banned(Vec::new());
        let came_back = second.is_some_and(|p| {
            jumps(&p).iter().any(|a| matches!(a, Act::Jump { dir: d, held: h, steer: s, from_x: f, .. } if (*d, *h, *s, *f) == (dir, held, steer, from_x)))
        });
        assert!(!came_back, "the banned jump came back");
    }

    #[test]
    fn a_planned_jump_carries_its_numbers_on_its_landing_line_and_no_plan_line_is_a_tick_rest() {
        let block = block_room(5.0, 12.0, 4.0);
        let start = standing(0.0, &block);
        let plan = search(&block, start, (8.5, 5.39), 3.6, true).expect("a path");
        let lines = walk_lines(&plan, &block, &start);
        let landing = lines.iter().find(|l| l.contains("land at x")).expect("a landing line");
        let spec = hop::JumpSpec::parse(landing).expect("the jump's numbers");
        assert!(spec.dir != 0 || spec.steer == 0);
        let rests = lines.iter().filter(|l| l.split_whitespace().next() == Some("wait") && l.split_whitespace().nth(1).is_some_and(|n| n.parse::<u32>().is_ok())).count();
        assert_eq!(rests, 0, "{lines:?}");
    }
}
