//! Source-derived Greenpath Pigeon (`Pigeon` FSM) controller.
//!
//! The Pigeon is the port's first critter rather than its first enemy, and the
//! distinction is the whole shape of what follows. It carries one hit point, no
//! `DamageHero`, no `Recoil`, no Geo and an `EnemyDeathEffectsNoEffect`, and its
//! single collider is a **trigger** on the `Interactive Object` layer. So it
//! cannot hurt the hero, cannot be stood on, and nothing it flies through ever
//! stops it: the caller moves it by its own velocity with no terrain solve, and
//! that is the source's behaviour, not a simplification of it.
//!
//! One tick is 1/60 s and motion is Q16.16 units per second. The caller owns
//! the `Hero Range` overlap and its raycast, the animation clock and the body.
//!
//! Three things about the source shape the state machine below.
//!
//! - `Right` and `Left` are `AddForce2d` with `ForceMode2D.Force` and
//!   `everyFrame`, on a body with `mass = 1`, `gravityScale = 0` and no drag.
//!   `Rigidbody2D.AddForce` adds `f * fixedDeltaTime / mass` per fixed step, so
//!   the authored force is a constant **acceleration** in units per second
//!   squared and the step rate cancels out of it. That is why this integrates
//!   at 60 Hz with no accumulator where `crate::vengefly` needs one: the Buzzer
//!   reads per-step velocity constants, this reads an acceleration.
//! - Facing is decided three times and the placement only gets the first word.
//!   `Set Frame` ends on a 50/50 `SendRandomEvent` into `Invert Scale`, which
//!   is a `FlipScale` and so negates the authored mirror rather than writing a
//!   pose of its own, and `Right`/`Left` then overwrite the result outright
//!   with an absolute scale for the flight direction. So the placement's
//!   mirror seeds `new` and stops mattering the moment the bird moves.
//! - `Fly`'s `CheckTargetDirection` sends the *opposite* event to the side the
//!   hero is on: the hero on the right sends `2`, which is the `Left` state. It
//!   flies away, always.
use crate::ONE;

/// `Hero Range`'s circle centre relative to the actor origin, Q16, and its
/// radius, both in the authored placement frame.
///
/// These are prefab geometry rather than placement geometry: every Greenpath
/// placement carries the same child transforms at the same uniform 0.8 size,
/// and the circle is centred on x = 0 so a mirrored placement does not move it
/// either. One pair of constants therefore covers all of them, the way
/// `crate::blocker`'s trigger boxes cover both Blockers. `host/pigeon.py`
/// recomputes both from the placement's own world matrix and refuses anything
/// that differs, so a Pigeon with its own geometry becomes an unadmitted record
/// rather than one that runs against somebody else's circle.
pub const HERO_RANGE_CENTER: [i32; 2] = [0, 39426];
/// 5.072 world units: the authored 6.34 radius through the placement's scale.
pub const HERO_RANGE_RADIUS: i32 = 332399;
/// `Fly`'s `Translate` of +0.5 in `Space.Self`, which `Transform.Translate`
/// resolves through `TransformDirection` and so does not scale: a plain half
/// unit of world lift on the takeoff frame.
pub const TAKEOFF_RISE: i32 = ONE / 2;
/// `Fly`'s `RandomFloat(10, 35)` into `Rise Force`, as an acceleration.
pub const RISE_FORCE: [i32; 2] = [10 * ONE, 35 * ONE];
/// `Right`'s `RandomFloat(35, 75)` into `Side Force`. `Left` writes the same
/// span negated, so the sign is the flight direction and the span is shared.
pub const SIDE_FORCE: [i32; 2] = [35 * ONE, 75 * ONE];
/// `Right`/`Left`'s `Wait(5)` before `Destroy`'s `DestroySelf`.
pub const LIFE_TICKS: u16 = 300;
/// `Set Frame`'s `RandomInt(0, 41)` with `inclusiveMax`, which `Tk2dPlayFrame`
/// then seeks to. The library runs at 12 fps throughout, so one source frame is
/// five 60 Hz animation ticks; `Idle 02` is only 41 frames long and the guest
/// clip clock wraps a looping clip, exactly as tk2d does.
pub const START_FRAMES: [i32; 2] = [0, 41];
/// 60 Hz ticks per source frame at the library's 12 fps.
pub const TICKS_PER_FRAME: u32 = 5;
/// Where a flying Pigeon is taken off the board rather than allowed to leave
/// the guest's validated +/-512-unit coordinate range.
///
/// The source keeps the object for a full five seconds, by which point it has
/// travelled hundreds of units and is long outside the room. The actor
/// neighbourhood gate suspends it well before this, so in practice nothing
/// reaches it; it is here so that the bound is proven rather than argued.
pub const WORLD_EDGE: i32 = 480 * ONE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clip {
    /// `Idle 01`; the shared `ActorSpec::walk_clip`.
    Idle1,
    /// `Fly`; the shared `ActorSpec::turn_clip`.
    Fly,
    /// `Idle 02`.
    Idle2,
    /// `Idle 03`.
    Idle3,
}
impl Clip {
    /// Clips carried by `ActorController::Pigeon`; `Idle1` and `Fly` are the
    /// shared `ActorSpec` slots every controller already has.
    pub const COUNT: usize = 2;
    pub const fn slot(self) -> Option<usize> {
        match self {
            Self::Idle1 | Self::Fly => None,
            other => Some(other as usize - 2),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// `Set Size` -> `Set Anim` -> `1`/`2`/`3` -> `Set Frame`, which the source
    /// walks through on its first frames and which pick the whole of a perched
    /// bird's appearance: which idle loop, which frame of it, which mirror.
    Spawn,
    /// `Idle`, watching `Hero Range` and `Enemy Range` every frame.
    Idle,
    /// `Right` or `Left`, accelerating away until `Wait(5)` reaches `Destroy`.
    Flying,
    /// `Destroy`'s `DestroySelf`. Nothing draws and nothing ticks after this.
    Gone,
    Dead,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Play a clip from the given 60 Hz animation tick, as `crate::vengefly`.
    Play(Clip, u32),
    /// `Fly`'s `Translate`: lift the body by this much on the takeoff frame.
    Rise(i32),
}
/// Ordered commands of one callback. The longest is the startup chain reaching
/// `Idle` and being startled on the same frame, which the source does too: the
/// idle loop is played and seeked, and then `Fly` replaces it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Actions {
    commands: [Option<Action>; 3],
    len: u8,
}
impl Actions {
    const fn new() -> Self {
        Self {
            commands: [None; 3],
            len: 0,
        }
    }
    fn push(&mut self, action: Action) {
        self.commands[self.len as usize] = Some(action);
        self.len += 1;
    }
    pub fn iter(&self) -> impl Iterator<Item = Action> + '_ {
        self.commands[..self.len as usize]
            .iter()
            .map(|a| a.unwrap())
    }
    pub fn len(&self) -> usize {
        self.len as usize
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// Sampled once per tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Senses {
    /// The `Hero Range` child's own answer: the hero inside its circle *and* an
    /// unobstructed line to it, which is what its FSM writes into the Pigeon's
    /// `Hero Range` bool every frame it is awake.
    pub can_see_hero: bool,
    /// `CheckTargetDirection` against the hero, which decides which way it goes.
    pub hero_right: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pigeon {
    phase: Phase,
    clip: Clip,
    /// Sprite mirror, -1 for the art as authored and +1 for the flipped one,
    /// matching the sign `ActorPlacement::initial_direction` carries.
    facing: i8,
    /// `Rise Force` and `Side Force` as an acceleration, Q16 units per second
    /// squared. Zero until takeoff.
    force: [i32; 2],
    velocity: [i32; 2],
    life: u16,
    rng: u32,
}
impl Pigeon {
    /// `tk2dSpriteAnimator.playAutomatically` holds `Idle 01` for the frame
    /// before the FSM's own startup chain runs, which is what `Spawn` is.
    ///
    /// `facing` is the placement's authored mirror, because `Set Frame`'s
    /// `Invert Scale` is a `FlipScale` and so negates whatever the placement
    /// stands at rather than writing a pose of its own. It is the only thing
    /// of the transform the controller reads, and `Right`/`Left` overwrite it
    /// on takeoff with an absolute scale the way the source does.
    pub const fn new(seed: u32, facing: i8) -> Self {
        Self {
            phase: Phase::Spawn,
            clip: Clip::Idle1,
            facing,
            force: [0; 2],
            velocity: [0; 2],
            life: 0,
            rng: seed,
        }
    }
    pub fn phase(self) -> Phase {
        self.phase
    }
    pub fn clip(self) -> Clip {
        self.clip
    }
    pub fn facing(self) -> i32 {
        self.facing as i32
    }
    pub fn velocity(self) -> [i32; 2] {
        self.velocity
    }
    pub fn die(&mut self) {
        self.phase = Phase::Dead;
        self.velocity = [0; 2];
        self.force = [0; 2];
    }
    /// The bird has left the board: `Destroy`, or the caller's coordinate edge.
    pub fn leave(&mut self) {
        if self.phase != Phase::Dead {
            self.phase = Phase::Gone;
        }
        self.velocity = [0; 2];
        self.force = [0; 2];
    }
    fn random(&mut self) -> u32 {
        self.rng = self.rng.wrapping_mul(1664525).wrapping_add(1013904223);
        self.rng >> 8
    }
    /// `RandomFloat(low, high)` over Q16 values, deterministic. Not Unity RNG
    /// parity, the same way the Runner's and the Blocker's are not.
    fn range(&mut self, [low, high]: [i32; 2]) -> i32 {
        let span = (high - low) as i64;
        low + ((self.random() as i64 * span) >> 24) as i32
    }
    /// `RandomInt(low, high)` with `inclusiveMax`, and `SendRandomEvent` over
    /// equally weighted events, which is the same draw.
    fn index(&mut self, count: u32) -> u32 {
        self.random() % count
    }
    /// One 60 Hz tick.
    pub fn tick(&mut self, senses: Senses) -> Actions {
        let mut out = Actions::new();
        match self.phase {
            Phase::Dead | Phase::Gone => {}
            Phase::Spawn => {
                // `Set Anim`'s three-way `SendRandomEvent`, then `Set Frame`'s
                // `RandomInt` seek and its own 50/50 into `Invert Scale`.
                // `Set Size`'s `RandomFloat(0.8, 1.0)` rescale has nowhere to
                // go here: the art is cooked at the authored scale and the
                // guest cannot resize a cooked frame. See the recognizer.
                self.clip = match self.index(3) {
                    0 => Clip::Idle1,
                    1 => Clip::Idle2,
                    _ => Clip::Idle3,
                };
                let frame = self.range(START_FRAMES) as u32;
                if self.index(2) == 1 {
                    self.facing = -self.facing;
                }
                self.phase = Phase::Idle;
                out.push(Action::Play(self.clip, frame * TICKS_PER_FRAME));
                // `Idle`'s two `BoolTest`s are `everyFrame`, so they also run
                // on the frame the state is entered.
                if senses.can_see_hero {
                    self.take_off(senses, &mut out);
                }
            }
            Phase::Idle => {
                if senses.can_see_hero {
                    self.take_off(senses, &mut out);
                }
            }
            Phase::Flying => {
                // `AddForce2d` once per fixed step, then the body moves. The
                // caller owns the move; this owns the velocity it moves by.
                self.velocity[0] += self.force[0] / 60;
                self.velocity[1] += self.force[1] / 60;
                if self.life == 0 {
                    self.leave();
                } else {
                    self.life -= 1;
                }
            }
        }
        out
    }
    /// `Fly` and the `Right`/`Left` it reaches on the same frame.
    ///
    /// `Fly`'s other four actions have nowhere to go: `ActivateAllChildren`
    /// turns off the two range triggers this controller no longer reads,
    /// `AudioPlayRandom` and the `ACTIVATE` that wakes the `Waker` are both
    /// recorded omissions, and `Tk2dPlayAnimation` is the `Play` below.
    fn take_off(&mut self, senses: Senses, out: &mut Actions) {
        let rise = self.range(RISE_FORCE);
        let side = self.range(SIDE_FORCE);
        // `CheckTargetDirection` sends `2` (the `Left` state) when the hero is
        // on the right, so this is away from the hero rather than towards it.
        let away = if senses.hero_right { -1 } else { 1 };
        self.force = [side * away, rise];
        // `Right` resets the transform scale to +1 and `Left` mirrors the
        // sprite on top of it, so the flight direction replaces whatever
        // `Invert Scale` left behind. Positive source scale is the art as
        // authored, which this port's facing writes as -1.
        self.facing = -away as i8;
        self.velocity = [self.force[0] / 60, self.force[1] / 60];
        self.phase = Phase::Flying;
        self.life = LIFE_TICKS;
        self.clip = Clip::Fly;
        out.push(Action::Rise(TAKEOFF_RISE));
        out.push(Action::Play(Clip::Fly, 0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feed `ticks` quiet frames and answer how much work was asked for.
    fn run(bird: &mut Pigeon, senses: Senses, ticks: usize) -> usize {
        (0..ticks).map(|_| bird.tick(senses).len()).sum()
    }

    #[test]
    fn a_perched_bird_picks_one_idle_loop_and_a_frame_of_it_and_then_sits() {
        let mut bird = Pigeon::new(7, -1);
        assert_eq!(bird.phase(), Phase::Spawn);
        assert_eq!(bird.clip(), Clip::Idle1);
        let actions = bird.tick(Senses::default());
        assert_eq!(actions.len(), 1);
        let Some(Action::Play(clip, tick)) = actions.iter().next() else {
            panic!("the startup chain plays an idle loop");
        };
        assert!(matches!(clip, Clip::Idle1 | Clip::Idle2 | Clip::Idle3));
        assert_eq!(clip, bird.clip());
        // `RandomInt(0, 41)` seeks a source frame, five animation ticks each.
        assert!(tick <= START_FRAMES[1] as u32 * TICKS_PER_FRAME);
        assert_eq!(tick % TICKS_PER_FRAME, 0);
        assert_eq!(bird.phase(), Phase::Idle);
        assert_eq!(bird.velocity(), [0; 2]);
        // Nothing in range: it never asks for anything again.
        assert_eq!(run(&mut bird, Senses::default(), 600), 0);
        assert_eq!(bird.phase(), Phase::Idle);
    }

    #[test]
    fn the_three_idle_loops_and_both_mirrors_are_all_reachable() {
        let mut clips = [false; 3];
        let mut mirrors = [false; 2];
        for seed in 0..64u32 {
            let mut bird = Pigeon::new(seed.wrapping_mul(2654435761), -1);
            bird.tick(Senses::default());
            clips[match bird.clip() {
                Clip::Idle1 => 0,
                Clip::Idle2 => 1,
                Clip::Idle3 => 2,
                Clip::Fly => panic!("a perched bird does not start on Fly"),
            }] = true;
            mirrors[usize::from(bird.facing() > 0)] = true;
        }
        assert!(clips.iter().all(|seen| *seen), "every `Set Anim` branch");
        assert!(
            mirrors.iter().all(|seen| *seen),
            "both `Set Frame` branches"
        );
    }

    #[test]
    fn it_flies_away_from_the_hero_and_accelerates_the_whole_way() {
        for hero_right in [false, true] {
            let mut bird = Pigeon::new(11, -1);
            bird.tick(Senses::default());
            let senses = Senses {
                can_see_hero: true,
                hero_right,
            };
            let actions = bird.tick(senses);
            assert_eq!(actions.len(), 2);
            assert_eq!(actions.iter().next(), Some(Action::Rise(TAKEOFF_RISE)));
            assert_eq!(bird.phase(), Phase::Flying);
            assert_eq!(bird.clip(), Clip::Fly);
            // Away from the hero, and the mirror follows the flight direction.
            let v = bird.velocity();
            assert_eq!(v[0] > 0, !hero_right);
            assert!(v[1] > 0, "it always rises");
            assert_eq!(bird.facing(), if hero_right { 1 } else { -1 });
            // Constant acceleration: every tick adds the same amount again.
            let step = v;
            bird.tick(senses);
            assert_eq!(bird.velocity(), [v[0] + step[0], v[1] + step[1]]);
            // The authored spans bound it, read back through one second.
            run(&mut bird, senses, 58);
            let after = bird.velocity();
            assert!(after[0].abs() >= SIDE_FORCE[0] && after[0].abs() <= SIDE_FORCE[1]);
            assert!(after[1] >= RISE_FORCE[0] && after[1] <= RISE_FORCE[1]);
        }
    }

    #[test]
    fn the_five_second_wait_is_the_whole_of_a_flight() {
        let mut bird = Pigeon::new(3, -1);
        bird.tick(Senses::default());
        let senses = Senses {
            can_see_hero: true,
            hero_right: true,
        };
        bird.tick(senses);
        run(&mut bird, senses, LIFE_TICKS as usize);
        assert_eq!(
            bird.phase(),
            Phase::Flying,
            "still airborne on the last frame"
        );
        bird.tick(senses);
        assert_eq!(bird.phase(), Phase::Gone);
        assert_eq!(bird.velocity(), [0; 2]);
        // Gone stays gone, and so does the work it no longer asks for.
        assert_eq!(run(&mut bird, senses, 60), 0);
    }

    #[test]
    fn a_bird_taken_off_the_board_or_killed_stops_for_good() {
        let senses = Senses {
            can_see_hero: true,
            hero_right: false,
        };
        let mut left = Pigeon::new(5, -1);
        left.tick(Senses::default());
        left.tick(senses);
        left.leave();
        assert_eq!(left.phase(), Phase::Gone);
        assert_eq!(run(&mut left, senses, 30), 0);
        let mut killed = Pigeon::new(5, -1);
        killed.tick(Senses::default());
        killed.die();
        assert_eq!(killed.phase(), Phase::Dead);
        assert_eq!(killed.velocity(), [0; 2]);
        assert_eq!(run(&mut killed, senses, 30), 0);
        // A dead bird is not resurrected by leaving the board.
        killed.leave();
        assert_eq!(killed.phase(), Phase::Dead);
    }

    #[test]
    fn the_hero_walking_up_on_the_first_frame_still_startles_it() {
        // `Idle`'s tests are `everyFrame`, so the entry frame runs them too and
        // a bird spawned with the hero already beside it never sits down.
        let mut bird = Pigeon::new(29, -1);
        let actions = bird.tick(Senses {
            can_see_hero: true,
            hero_right: true,
        });
        // The idle loop is still played and seeked, and `Fly` then replaces it
        // on the same frame, which is the order the source runs them in.
        assert_eq!(actions.len(), 3);
        let mut played = [None; 3];
        let mut count = 0;
        for action in actions.iter() {
            if let Action::Play(clip, _) = action {
                played[count] = Some(clip);
                count += 1;
            }
        }
        assert_eq!(count, 2);
        assert!(matches!(
            played[0],
            Some(Clip::Idle1 | Clip::Idle2 | Clip::Idle3)
        ));
        assert_eq!(played[1], Some(Clip::Fly));
        assert_eq!(bird.phase(), Phase::Flying);
        assert_eq!(bird.clip(), Clip::Fly);
    }

    #[test]
    fn clip_slots_cover_the_controller_array_exactly_once() {
        let carried = [Clip::Idle2, Clip::Idle3];
        let mut seen = [false; Clip::COUNT];
        for clip in carried {
            let slot = clip.slot().expect("carried clip has a controller slot");
            assert!(!seen[slot], "{clip:?} reuses slot {slot}");
            seen[slot] = true;
        }
        assert!(seen.iter().all(|s| *s));
        assert_eq!(Clip::Idle1.slot(), None);
        assert_eq!(Clip::Fly.slot(), None);
    }
}
