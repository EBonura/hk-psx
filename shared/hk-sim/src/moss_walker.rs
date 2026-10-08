//! Source-derived Moss Walker (Mosscreep) `Moss Walker` FSM for the floor
//! placements: buried in a tuft until the Knight comes in sight, shakes and
//! appears, walks at 3 units/s turning at ledges (and, walking left, at
//! walls), and buries itself again once the Knight has been seen and left.
//! Evidence: hk-enemies scratch/research/moss_walker.md (FSM dump of
//! level128/134/136/139/147, Assembly-CSharp and PlayMaker IL).
//!
//! The caller owns the gravity body against terrain (gravityScale 1), the
//! three child point rays (`climber_ray_hit` geometry: origin mirrors with
//! the scale, direction turns with the rotation only), the `Wake Range`
//! circle and its terrain line, hits, corpse and draws. Rays are cast only
//! when `needs` asks, as the source casts them (on Walking's enter and every
//! third Update after it; Ground Range once per Turn Check).
use crate::ONE;

pub const HEALTH: i16 = 10;
pub const WALK_SPEED: i32 = 3 * ONE;
/// `Wake Range`: CircleCollider2D r 8.27 on the actor origin (Q16).
pub const WAKE_RADIUS: i32 = 541983;
/// `Wake Pause` WaitRandom 0..1 s.
pub const WAKE_PAUSE_TICKS: u16 = 60;
/// `Shake` Wait 1.2 s.
pub const SHAKE_TICKS: u16 = 72;
/// `Appear`: five frames at 10 fps, to completion.
pub const APPEAR_TICKS: u16 = 30;
/// `Walk Start` Wait 0.1 s.
pub const WALK_START_TICKS: u16 = 6;
/// `Turn`: three frames at 12 fps, to completion.
pub const TURN_TICKS: u16 = 15;
/// `Bury`: five frames at 12 fps, to completion.
pub const BURY_TICKS: u16 = 25;
/// `Wake` sets Hide Timer to Random 3..5 s; `Check Hide` resets it to 1 s.
pub const HIDE_TICKS: [u16; 2] = [180, 300];
pub const HIDE_RECHECK_TICKS: u16 = 60;
/// RayCast2dV2 repeatInterval 3.
pub const RAY_INTERVAL: u8 = 3;
/// Child ray origins (local, facing-left frame) and lengths: `Edge Range`
/// down 1.0, `Wall Range` along local -x 0.5, `Ground Range` down 1.0.
pub const EDGE_ORIGIN: [i32; 2] = [-59638, -42598];
pub const WALL_ORIGIN: [i32; 2] = [-26214, -24904];
pub const GROUND_ORIGIN: [i32; 2] = [0, -20316];
pub const EDGE_LENGTH: i32 = ONE;
pub const WALL_LENGTH: i32 = ONE / 2;
pub const GROUND_LENGTH: i32 = ONE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Rest,
    WakePause,
    Shake,
    Appear,
    WalkStart,
    Walking,
    TurnCheck,
    CancelFrame,
    Turn,
    Bury,
    Dead,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Clip {
    Walk,
    Turn,
    Rest,
    Shake,
    Appear,
    Bury,
}
impl Clip {
    pub const COUNT: usize = 6;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sound {
    /// `moss_flyer_walker_emerge` plus one of `ceiling_dropper_look_2/4/5`.
    Emerge,
    /// The footstep loop starts (`Walk Start`) or stops (`Hide`).
    LoopOn,
    LoopOff,
}
/// Which senses the next tick reads; the caller computes only these.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Needs {
    pub wake_range: bool,
    pub walk_rays: bool,
    pub ground_ray: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Senses {
    /// Hero overlapping the 8.27 circle and an unobstructed line to it.
    pub wake_range: bool,
    /// `Edge Range` and `Wall Range` hits, when `walk_rays` was asked.
    pub edge: bool,
    pub wall: bool,
    pub ground: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Step {
    pub play: Option<Clip>,
    pub sound: Option<Sound>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MossWalker {
    phase: Phase,
    timer: u16,
    hide_timer: u16,
    ray_clock: u8,
    edge: bool,
    wall: bool,
    encountered: bool,
    /// +1 right (source scale.x -1), -1 left: Check Dir reads the scale.
    facing: i8,
    vx: i32,
    clip: Clip,
    rng: u32,
}
impl MossWalker {
    /// A floor placement. `roams` is the `Roams` bool: it skips the burial
    /// and walks from the first frame, with a Hide Timer of zero.
    pub fn new(facing: i8, roams: bool, seed: u32) -> Self {
        let mut m = Self { phase: Phase::Rest, timer: 0, hide_timer: 0, ray_clock: 0, edge: true, wall: false,
            encountered: false, facing, vx: 0, clip: Clip::Rest, rng: seed };
        if roams {
            let mut step = Step::default();
            m.activate(&mut step);
        }
        m
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn facing(&self) -> i32 {
        self.facing as i32
    }
    pub fn clip(&self) -> Clip {
        self.clip
    }
    /// The body's x velocity the source last set (SetVelocity2d), which the
    /// caller's frictionless body keeps until terrain stops it.
    pub fn vx(&self) -> i32 {
        self.vx
    }
    /// A blocking contact zeroed the body's x velocity (Box2D, no friction).
    pub fn stopped(&mut self) {
        self.vx = 0;
    }
    /// Buried: invincible with no hit effect, no contact damage, and an
    /// active NonBouncer (a down-slash does not pogo off the tuft).
    pub fn hidden(&self) -> bool {
        matches!(self.phase, Phase::Rest | Phase::WakePause | Phase::Shake | Phase::Appear | Phase::Bury)
    }
    pub fn dead(&self) -> bool {
        self.phase == Phase::Dead
    }
    pub fn die(&mut self) {
        self.phase = Phase::Dead;
        self.vx = 0;
    }
    pub fn needs(&self) -> Needs {
        Needs {
            wake_range: matches!(self.phase, Phase::Rest)
                || (matches!(self.phase, Phase::WalkStart | Phase::Walking) && self.hide_timer <= 1),
            walk_rays: self.phase == Phase::Walking && self.ray_clock % RAY_INTERVAL == 0,
            ground_ray: self.phase == Phase::TurnCheck,
        }
    }
    fn random(&mut self) -> u32 {
        self.rng = self.rng.wrapping_mul(1664525).wrapping_add(1013904223);
        self.rng >> 8
    }
    fn play(&mut self, clip: Clip, step: &mut Step) {
        self.clip = clip;
        step.play = Some(clip);
    }
    /// `Activate` -> `Check Dir` -> `Up Right`/`Left Down` -> `Walk Start`.
    fn activate(&mut self, step: &mut Step) {
        self.walk_start(step);
    }
    fn walk_start(&mut self, step: &mut Step) {
        self.phase = Phase::WalkStart;
        self.timer = WALK_START_TICKS;
        self.vx = self.facing as i32 * WALK_SPEED;
        step.sound = Some(Sound::LoopOn);
        self.play(Clip::Walk, step);
    }
    fn walking(&mut self) {
        // Walking's enter: Wall false, Edge true, then both casts.
        self.phase = Phase::Walking;
        self.ray_clock = 0;
        self.edge = true;
        self.wall = false;
    }
    /// `Set Encountered` -> `Check Hide`: bury when the Knight has been met
    /// and is gone, else restart the velocity and walk on for a second.
    fn check_hide(&mut self, wake: bool, step: &mut Step) {
        if !self.encountered && wake {
            self.encountered = true;
        }
        self.hide_timer = HIDE_RECHECK_TICKS;
        if !self.encountered || wake {
            self.vx = self.facing as i32 * WALK_SPEED;
            self.walking();
        } else {
            self.phase = Phase::Bury;
            self.timer = BURY_TICKS;
            self.vx = 0;
            step.sound = Some(Sound::LoopOff);
            self.play(Clip::Bury, step);
        }
    }
    /// One nominal 60 Hz frame.
    pub fn tick(&mut self, senses: Senses) -> Step {
        let mut step = Step::default();
        match self.phase {
            Phase::Dead => {}
            Phase::Rest => {
                self.encountered = true;
                if senses.wake_range {
                    self.phase = Phase::WakePause;
                    self.timer = ((self.random() as u64 * (WAKE_PAUSE_TICKS as u64 + 1)) >> 24) as u16;
                    if self.timer == 0 {
                        self.shake(&mut step);
                    }
                }
            }
            Phase::WakePause => {
                self.timer -= 1;
                if self.timer == 0 {
                    self.shake(&mut step);
                }
            }
            Phase::Shake => {
                self.timer -= 1;
                if self.timer == 0 {
                    self.phase = Phase::Appear;
                    self.timer = APPEAR_TICKS;
                    self.play(Clip::Appear, &mut step);
                    let span = (HIDE_TICKS[1] - HIDE_TICKS[0]) as u64 + 1;
                    self.hide_timer = HIDE_TICKS[0] + ((self.random() as u64 * span) >> 24) as u16;
                }
            }
            Phase::Appear => {
                self.timer -= 1;
                if self.timer == 0 {
                    self.activate(&mut step);
                }
            }
            Phase::WalkStart | Phase::Walking => {
                self.hide_timer = self.hide_timer.saturating_sub(1);
                if self.hide_timer == 0 {
                    self.check_hide(senses.wake_range, &mut step);
                    return step;
                }
                if self.phase == Phase::WalkStart {
                    self.timer -= 1;
                    if self.timer == 0 {
                        self.walking();
                    }
                    return step;
                }
                if self.ray_clock % RAY_INTERVAL == 0 {
                    self.edge = senses.edge;
                    self.wall = senses.wall;
                }
                self.ray_clock = self.ray_clock.wrapping_add(1);
                if !self.edge || self.wall {
                    self.phase = Phase::TurnCheck;
                }
            }
            Phase::TurnCheck => {
                if senses.ground {
                    self.phase = Phase::Turn;
                    self.timer = TURN_TICKS;
                    self.vx = 0;
                    self.play(Clip::Turn, &mut step);
                } else {
                    // `Cancel Frame`: Walking again on the next frame, whose
                    // enter resets the ray bools.
                    self.phase = Phase::CancelFrame;
                }
            }
            Phase::CancelFrame => {
                self.walking();
            }
            Phase::Turn => {
                self.timer -= 1;
                if self.timer == 0 {
                    // `Flip` -> `Check Dir` -> `Walk Start`.
                    self.facing = -self.facing;
                    self.walk_start(&mut step);
                }
            }
            Phase::Bury => {
                self.timer -= 1;
                if self.timer == 0 {
                    // `Rest` keeps the last Bury frame showing.
                    self.phase = Phase::Rest;
                }
            }
        }
        step
    }
    fn shake(&mut self, step: &mut Step) {
        self.phase = Phase::Shake;
        self.timer = SHAKE_TICKS;
        step.sound = Some(Sound::Emerge);
        self.play(Clip::Shake, step);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn walk_to(m: &mut MossWalker, phase: Phase, senses: Senses, limit: u32) -> u32 {
        for t in 0..limit {
            if m.phase() == phase {
                return t;
            }
            m.tick(senses);
        }
        panic!("never reached {phase:?}: {:?}", m.phase());
    }
    const SEEN: Senses = Senses { wake_range: true, edge: true, wall: false, ground: true };
    #[test]
    fn buried_until_seen_then_shakes_appears_and_walks_left() {
        let mut m = MossWalker::new(-1, false, 9);
        for _ in 0..100 {
            m.tick(Senses { wake_range: false, ..SEEN });
            assert_eq!(m.phase(), Phase::Rest);
            assert!(m.hidden());
        }
        m.tick(SEEN);
        let t = walk_to(&mut m, Phase::Shake, SEEN, WAKE_PAUSE_TICKS as u32 + 2);
        assert!(t <= WAKE_PAUSE_TICKS as u32 + 1);
        assert_eq!(walk_to(&mut m, Phase::Appear, SEEN, 100), SHAKE_TICKS as u32);
        assert_eq!(walk_to(&mut m, Phase::WalkStart, SEEN, 100), APPEAR_TICKS as u32);
        assert!(!m.hidden());
        assert_eq!(m.vx(), -WALK_SPEED, "authored scale 1 faces left and walks left");
        assert_eq!(m.clip(), Clip::Walk);
    }
    #[test]
    fn a_lost_ledge_turns_it_only_when_its_feet_still_touch_ground() {
        let mut m = MossWalker::new(-1, true, 1);
        walk_to(&mut m, Phase::Walking, SEEN, 20);
        assert!(m.needs().walk_rays, "Walking casts on enter");
        let ledge = Senses { edge: false, ..SEEN };
        m.tick(ledge);
        assert_eq!(m.phase(), Phase::TurnCheck);
        m.tick(Senses { ground: false, ..ledge });
        assert_eq!(m.phase(), Phase::CancelFrame);
        m.tick(ledge);
        assert_eq!(m.phase(), Phase::Walking);
        m.tick(ledge);
        assert_eq!(m.phase(), Phase::TurnCheck);
        let step = m.tick(ledge);
        assert_eq!((m.phase(), step.play, m.vx()), (Phase::Turn, Some(Clip::Turn), 0));
        assert_eq!(walk_to(&mut m, Phase::WalkStart, SEEN, 40), TURN_TICKS as u32);
        assert_eq!((m.facing(), m.vx()), (1, WALK_SPEED));
    }
    #[test]
    fn met_and_left_it_buries_after_its_hide_timer() {
        let mut m = MossWalker::new(-1, false, 4);
        m.tick(SEEN);
        walk_to(&mut m, Phase::WalkStart, SEEN, 300);
        let gone = Senses { wake_range: false, ..SEEN };
        let t = walk_to(&mut m, Phase::Bury, gone, 400);
        assert!((HIDE_TICKS[0] as u32 - 1..=HIDE_TICKS[1] as u32).contains(&t), "{t}");
        assert!(m.hidden());
        assert_eq!(walk_to(&mut m, Phase::Rest, gone, 40), BURY_TICKS as u32);
    }
    #[test]
    fn a_roamer_walks_until_it_has_met_the_hero_and_lost_it() {
        let mut m = MossWalker::new(1, true, 2);
        assert_eq!((m.phase(), m.vx()), (Phase::WalkStart, WALK_SPEED));
        let gone = Senses { wake_range: false, ..SEEN };
        for _ in 0..600 {
            m.tick(gone);
            assert!(!m.hidden(), "never met: it keeps walking");
        }
        // Seen at a one-second check: `Set Encountered`.
        for _ in 0..HIDE_RECHECK_TICKS + 1 {
            m.tick(SEEN);
            assert!(!m.hidden());
        }
        let t = walk_to(&mut m, Phase::Bury, gone, 200);
        assert!(t <= HIDE_RECHECK_TICKS as u32 + 1, "{t}");
    }
}
