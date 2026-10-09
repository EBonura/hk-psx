//! The guest keeps a column index over each view's edges and visits only the edges a query's box
//! can meet (`runner_senses::EdgeColumns`); the host-side enemy tests visit all of them. This
//! runs the walker's wall and ledge sweeps and the sight line over every cooked room both ways,
//! from a grid of positions, and reports any place the two disagree.
use super::*;
use crate::world_data::load_scene;
use hk_sim::runner_senses::{self as q, EdgeColumns, EdgeMask};
use std::cell::RefCell;
use std::collections::BTreeMap;

thread_local! {
    static COLUMNS: RefCell<EdgeColumns> = RefCell::new(EdgeColumns::ALL);
}
fn near(bounds: [i32; 4]) -> EdgeMask {
    COLUMNS.with(|c| c.borrow().near(bounds))
}

pub fn run(names: &BTreeMap<usize, String>) {
    let (mut rooms, mut queries, mut bad) = (0u64, 0u64, 0u64);
    let mut longest = 0usize;
    let mut reports = Vec::new();
    for (scene, name) in names {
        for region in load_scene(*scene) {
            let room = region.room();
            let count = room.counts[5];
            longest = longest.max(count);
            let edges: Vec<[i32; 4]> = (0..count).map(|i| room.edge(i)).collect();
            COLUMNS.with(|c| *c.borrow_mut() = EdgeColumns::build(&edges));
            rooms += 1;
            let b = region.collision_bounds;
            let step = ONE / 2;
            let mut x = b[0];
            while x <= b[2] {
                let mut y = b[1];
                while y <= b[3] {
                    for direction in [-1, 1] {
                        let all = q::walker_queries([x, y], direction, count, |i| edges[i]);
                        let idx = q::walker_queries_near(q::Shape::RUNNER, [x, y], direction, count, |i| edges[i], near);
                        queries += 1;
                        if all != idx {
                            bad += 1;
                            if reports.len() < 20 {
                                reports.push(format!("{name} region {} walker at ({:.2},{:.2}) dir {direction}: all {all:?} indexed {idx:?}", region.global_id, x as f64 / 65536.0, y as f64 / 65536.0));
                            }
                        }
                    }
                    // Sight to a point two units up and to the right, alert range assumed.
                    let hero = [x + 3 * ONE, y + 2 * ONE];
                    let all = q::line_of_sight([x, y], hero, true, count, |i| edges[i]);
                    let idx = q::line_of_sight_near([x, y], hero, true, count, |i| edges[i], near);
                    queries += 1;
                    if all != idx {
                        bad += 1;
                        if reports.len() < 20 {
                            reports.push(format!("{name} region {} sight at ({:.2},{:.2}): all {all:?} indexed {idx:?}", region.global_id, x as f64 / 65536.0, y as f64 / 65536.0));
                        }
                    }
                    y += step;
                }
                x += step;
            }
        }
    }
    println!("{rooms} rooms, longest edge list {longest}, {queries} queries, {bad} disagreements");
    for r in reports {
        println!("{r}");
    }
}
