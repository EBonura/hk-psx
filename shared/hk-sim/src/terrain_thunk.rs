//! `HeroController.CheckForTerrainThunk`: the nail striking terrain.
//!
//! Every swing starts the coroutine (`attackDir` 0 for a side slash, 1 up, 2 down). For
//! `NAIL_TERRAIN_CHECK_TIME` (0.12 s) it box-casts once per frame from the Knight's collider,
//! a 0.45-unit square, until something answers: sideways from 0.25 above the collider's centre
//! for 2.0 units (right when `facingRight` differs from `wallSliding`), up from its top or down
//! from its bottom for 1.5 units, against the Terrain layers. A hit on a non-trigger collider
//! whose GameObject has no active `NonThunker` spawns `nailTerrainImpactEffectPrefab`, whose
//! AudioSource plays the tink, and ends the checks. The source then recoils the Knight off a
//! side or up hit; the port does not (it never has), so this models the sound's trigger only.
//!
//! The cast here is the swept box against the room's terrain edges: an edge that crosses or
//! lies in the box's whole path counts, the nearest or not. An edge with both ends in one of
//! the scene's `NonThunker` boxes is skipped. No charm lengthens the nail in the port, so the
//! distances are the uncharmed ones.
use crate::{combat::segments_intersect, empty_edge, Player, ONE};

/// 0.12 s at 60 Hz: the checks at 0, 1/60, ... 7/60 s (the timer is tested after each frame's
/// `deltaTime` comes off it, and the eighth leaves it at 0.0033 s).
pub const CHECK_TICKS: u8 = 8;
/// Half the 0.45-unit cast box.
pub const HALF: i32 = 14_746;
/// How far the side cast starts above the collider's centre (0.25 units).
pub const SIDE_RISE: i32 = ONE / 4;
pub const SIDE_DISTANCE: i32 = 2 * ONE;
pub const VERTICAL_DISTANCE: i32 = ONE + ONE / 2;
/// Slack on a `NonThunker` box, so an edge on its outline counts as inside it.
const SLACK: i32 = 4096;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TerrainThunk {
    left: u8,
    /// `attackDir`: 0 side, 1 up, 2 down.
    dir: u8,
}

impl TerrainThunk {
    pub const fn new() -> Self {
        Self { left: 0, dir: 0 }
    }
    /// A swing of nail `kind` (normal, alternate, up, down) began: the coroutine starts over.
    pub fn start(&mut self, kind: u16) {
        self.left = CHECK_TICKS;
        self.dir = match kind {
            2 => 1,
            3 => 2,
            _ => 0,
        };
    }
    pub fn active(&self) -> bool {
        self.left != 0
    }
    /// The world box the cast sweeps this tick for a Knight whose collider spans `body`
    /// (x0, y0, x1, y1), facing `facing` (+1 right).
    pub fn swept(&self, body: [i32; 4], facing: i32, wall_sliding: bool) -> [i32; 4] {
        let cx = body[0] / 2 + body[2] / 2;
        match self.dir {
            1 => [cx - HALF, body[3] - HALF, cx + HALF, body[3] + HALF + VERTICAL_DISTANCE],
            2 => [cx - HALF, body[1] - HALF - VERTICAL_DISTANCE, cx + HALF, body[1] + HALF],
            _ => {
                let cy = body[1] / 2 + body[3] / 2 + SIDE_RISE;
                let right = (facing > 0) != wall_sliding;
                if right {
                    [cx - HALF, cy - HALF, cx + HALF + SIDE_DISTANCE, cy + HALF]
                } else {
                    [cx - HALF - SIDE_DISTANCE, cy - HALF, cx + HALF, cy + HALF]
                }
            }
        }
    }
    /// One frame of the coroutine. True when this frame's cast struck terrain: play the tink.
    /// `count` edges are read through `edge`; `no_thunk` are the scene's `NonThunker` boxes.
    pub fn tick(
        &mut self,
        player: &Player,
        body: [i32; 4],
        count: usize,
        edge: impl Fn(usize) -> [i32; 4],
        no_thunk: &[[i32; 4]],
    ) -> bool {
        if self.left == 0 {
            return false;
        }
        self.left -= 1;
        let swept = self.swept(body, player.facing, player.wall_sliding);
        let hit = (0..count).any(|i| {
            let e = edge(i);
            !empty_edge(&e) && edge_in_box(e, swept) && !no_thunk.iter().any(|b| edge_within(e, *b))
        });
        if hit {
            self.left = 0;
        }
        hit
    }
}

fn inside(p: [i32; 2], b: [i32; 4]) -> bool {
    p[0] >= b[0] && p[0] <= b[2] && p[1] >= b[1] && p[1] <= b[3]
}

/// Whether segment `e` (x0, y0, x1, y1) touches box `b`.
fn edge_in_box(e: [i32; 4], b: [i32; 4]) -> bool {
    if e[0].max(e[2]) < b[0] || e[0].min(e[2]) > b[2] || e[1].max(e[3]) < b[1] || e[1].min(e[3]) > b[3] {
        return false;
    }
    let (p, q) = ([e[0], e[1]], [e[2], e[3]]);
    if inside(p, b) || inside(q, b) {
        return true;
    }
    let c = [[b[0], b[1]], [b[2], b[1]], [b[2], b[3]], [b[0], b[3]]];
    (0..4).any(|j| segments_intersect(p, q, c[j], c[(j + 1) % 4]))
}

/// Whether both ends of `e` lie in `b` grown by `SLACK`.
fn edge_within(e: [i32; 4], b: [i32; 4]) -> bool {
    let g = [b[0] - SLACK, b[1] - SLACK, b[2] + SLACK, b[3] + SLACK];
    inside([e[0], e[1]], g) && inside([e[2], e[3]], g)
}

#[cfg(test)]
mod tests {
    use super::*;
    // A Knight 1 unit wide and 2 tall standing at the origin.
    const BODY: [i32; 4] = [-ONE / 2, 0, ONE / 2, 2 * ONE];
    fn player(facing: i32) -> Player {
        let mut p = Player::spawn(0, 0);
        p.facing = facing;
        p
    }
    fn run(t: &mut TerrainThunk, p: &Player, edges: &[[i32; 4]], no: &[[i32; 4]]) -> bool {
        t.tick(p, BODY, edges.len(), |i| edges[i], no)
    }
    // A wall 2 units to the right (inside the 2.225 reach of the side cast) and one 3 units left.
    const WALL_RIGHT: [i32; 4] = [2 * ONE, -4 * ONE, 2 * ONE, 4 * ONE];
    const WALL_LEFT: [i32; 4] = [-3 * ONE, -4 * ONE, -3 * ONE, 4 * ONE];

    #[test]
    fn a_side_slash_tinks_off_the_wall_it_faces_and_only_once() {
        let mut t = TerrainThunk::new();
        t.start(0);
        assert!(run(&mut t, &player(1), &[WALL_RIGHT], &[]));
        assert!(!t.active());
        assert!(!run(&mut t, &player(1), &[WALL_RIGHT], &[]));
        t.start(1);
        t.dir = 0;
        assert!(!run(&mut t, &player(-1), &[WALL_RIGHT, WALL_LEFT], &[]), "3 units is past the reach");
    }

    #[test]
    fn the_checks_run_eight_frames_then_stop() {
        let mut t = TerrainThunk::new();
        t.start(0);
        for _ in 0..CHECK_TICKS {
            assert!(t.active());
            assert!(!run(&mut t, &player(1), &[WALL_LEFT], &[]));
        }
        assert!(!t.active());
        assert!(!run(&mut t, &player(1), &[WALL_RIGHT], &[]));
    }

    #[test]
    fn a_wall_slide_casts_away_from_the_wall() {
        let mut t = TerrainThunk::new();
        t.start(0);
        let mut p = player(1);
        p.wall_sliding = true;
        assert!(!run(&mut t, &p, &[WALL_RIGHT], &[]));
        let near_left = [-2 * ONE, -4 * ONE, -2 * ONE, 4 * ONE];
        assert!(run(&mut t, &p, &[near_left], &[]));
    }

    #[test]
    fn up_and_down_casts_reach_a_unit_and_a_half() {
        let ceiling = [-4 * ONE, 3 * ONE + ONE / 2, 4 * ONE, 3 * ONE + ONE / 2];
        let floor = [-4 * ONE, -ONE - ONE / 2, 4 * ONE, -ONE - ONE / 2];
        let mut t = TerrainThunk::new();
        t.start(2);
        assert!(run(&mut t, &player(1), &[ceiling], &[]));
        t.start(3);
        assert!(run(&mut t, &player(1), &[floor], &[]));
        t.start(2);
        assert!(!run(&mut t, &player(1), &[floor], &[]));
        let high = [-4 * ONE, 4 * ONE, 4 * ONE, 4 * ONE];
        t.start(2);
        assert!(!run(&mut t, &player(1), &[high], &[]), "4 units up is past 2 + 1.725");
    }

    #[test]
    fn a_non_thunker_box_and_a_removed_edge_give_no_tink() {
        let door = [2 * ONE, -ONE, 2 * ONE + ONE / 2, 3 * ONE];
        let door_face = [2 * ONE, -ONE, 2 * ONE, 3 * ONE];
        let mut t = TerrainThunk::new();
        t.start(0);
        assert!(!run(&mut t, &player(1), &[door_face, [0; 4]], &[door]));
        t.start(0);
        assert!(run(&mut t, &player(1), &[door_face], &[]));
    }

    #[test]
    fn a_segment_crossing_the_box_without_an_end_inside_counts() {
        let diagonal = [ONE, -4 * ONE, 3 * ONE, 6 * ONE];
        assert!(edge_in_box(diagonal, [0, 0, 2 * ONE + HALF, ONE]));
        assert!(!edge_in_box(diagonal, [-3 * ONE, 0, 0, ONE]));
    }
}
