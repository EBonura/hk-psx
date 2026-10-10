//! Source-derived Crossroads Blocker (`Blocker Control`) controller.
//!
//! The Blocker is the port's first turret: no `Rigidbody2D`, no `Walker`, no
//! `Recoil`. It sits on its own `Terrain Block` and does one thing, which is
//! open when the hero comes near, lob a `Shot Mawlek` on a timer, and shut
//! again when the hero walks under it. One tick is 1/60 s and motion is Q16.16
//! units per second. The caller owns the trigger overlaps, the animation clock
//! (clip completion arrives through `Senses::completed` with the token this
//! controller handed out, as `crate::zombie_shield` does) and the shot pool.
//!
//! Three things about the source are worth keeping in view, because they are
//! what the state machine below is shaped by rather than decoration:
//!
//! - The shell is `SetInvincible` with `invincibleFromDirection` left at the
//!   authored 0, and `HealthManager::IsBlockingByDirection` returns true for
//!   every cardinal in that case (read from the installed CIL: the method
//!   returns 1 as soon as `invincible` is set and the direction field is zero,
//!   before it reaches the per-cardinal switch). So a closed Blocker blocks
//!   everything, including a pogo, and there is no direction table here of the
//!   kind the Zombie Shield needs.
//! - `Init` is the only place facing is decided. `Direction` branches on the
//!   `Facing Right` FSM variable, not on the transform, and the `Right` branch
//!   writes a world-space `Shot Origin` and a positive `X Speed` pair. So the
//!   placement's transform mirror drives the drawn sprite and nothing else;
//!   the shot always leaves towards the side `Facing Right` names.
//! - `Attack Choose` is a 50/50 `SendRandomEvent` between GOOP and ROLLER, and
//!   ROLLER falls through `Can Roller?` to GOOP whenever `fireballLevel` is 0
//!   or a spat roller is still alive. The caller answers `Can Roller?`
//!   (`Senses::can_roller`); the roller itself is the Mound Baldur's FSM
//!   (`crate::baldur::Baldur::spawned`). The 50/50 is only drawn when ROLLER
//!   could be taken, so a Knight without the spell keeps the old goop cycle.
use crate::ONE;

/// `Alert Range New`, `Attack Range` and `Unalert Range` as world boxes
/// relative to the actor origin, Q16.
///
/// These are prefab geometry, not placement geometry: both admitted Blockers
/// carry byte-identical child transforms and colliders, and both are mirrored
/// placements, so one set of constants covers them the way `crate::aspid`'s
/// alert radii cover every Aspid. `host/blocker.py` recomputes all three from
/// the placement's own world matrix and refuses anything that differs, so a
/// Blocker with its own geometry becomes an unadmitted record rather than one
/// that runs against somebody else's boxes.
pub const ALERT: [i32; 4] = [-37122, -226637, 957772, 512589];
/// `Attack Range`: the hero under the Blocker, which is what shuts it.
pub const ATTACK: [i32; 4] = [-162202, -229293, 427134, 733486];
/// `Unalert Range`: the room-sized box whose own trigger FSM drives the
/// `Unalert Range` bool. Only a placement that carries that child sleeps.
pub const UNALERT: [i32; 4] = [-785266, -652750, 1873773, 771067];
/// `Shot Origin`, which `Right` writes and `SpawnObjectFromGlobalPool` adds to
/// the spawn point's world position. A world offset, not a local one: the
/// action adds two `Vector3`s without consulting a transform.
pub const SHOT_ORIGIN: [i32; 2] = [180879, 51118];
/// `Shot Y Speed`, the one component of the launch that is not randomised.
pub const SHOT_VY: i32 = 20 * ONE;
/// `X Speed Min` and `X Speed Max`, as the `Right` branch writes them. The
/// `Left` branch writes -1 and -15, which is a wider spread and the opposite
/// sign; no admitted placement takes it, so the recognizer refuses one that
/// would rather than guessing at the sampler's behaviour with a reversed range.
pub const SHOT_VX: [i32; 2] = [3 * ONE, 15 * ONE];
/// `Idle`'s `WaitRandom(0.8, 1.2)` in ticks, inclusive, as [lo, hi].
pub const IDLE_TICKS: [i32; 2] = [48, 72];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clip {
    /// `Idle`, the open and waiting loop; the shared `ActorSpec::walk_clip`.
    Idle,
    /// `Closed`, the one-frame shut pose; the shared `ActorSpec::turn_clip`.
    Closed,
    Open,
    Close1,
    Close2,
    /// `Shoot Antic`.
    Antic,
    /// `Shoot CD`, which `Fire` starts and `Shot Anim End` waits out.
    Cooldown,
    Hit,
}
impl Clip {
    /// Clips carried by `ActorController::Blocker`; Idle and Closed are the
    /// shared `ActorSpec` slots every controller already has.
    pub const COUNT: usize = 6;
    pub const fn slot(self) -> Option<usize> {
        match self {
            Self::Idle | Self::Closed => None,
            other => Some(other as usize - 2),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Animation {
    pub clip: Clip,
    pub serial: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// `Dormant`: shut, invincible, watching `Alert Range New`.
    Dormant,
    /// `Open`, which clears the invincibility on its way past.
    Opening,
    /// `Idle`, counting `WaitRandom` down to the next shot.
    Idle,
    /// `Shot Antic`.
    Antic,
    /// `Fire` -> `Roller Assign` -> `Shot Anim End`. The source takes all three
    /// on one frame because every action in `Fire` finishes on entry and
    /// `Roller Assign`'s `BoolTest` sends FINISHED straight through for a shot
    /// that is not a Roller; the shot is away and `Shoot CD` is still running.
    Cooldown,
    /// `Close` or `Sleep 1`, which share the `Close1` clip and differ only in
    /// where the pair ends up.
    Close1 {
        sleep: bool,
    },
    /// `Close2` or `Sleep 2`.
    Close2 {
        sleep: bool,
    },
    /// `Closed`: shut and invincible, waiting for the hero to leave `Attack
    /// Range` so it can open again.
    Shut,
    /// `Hit`.
    Hit,
    Dead,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Play(Animation),
    /// `Fire`: one `Shot Mawlek` at the actor's position plus `offset`, with
    /// the launch velocity `SetVelocity2d` writes.
    Fire {
        offset: [i32; 2],
        velocity: [i32; 2],
    },
    /// `Fire` from the `Roller` branch: a `Spawn Roller v2` at the same point
    /// with the same launch, told to roll the way the Blocker faces
    /// (`Roller Assign`).
    Roller {
        offset: [i32; 2],
        velocity: [i32; 2],
    },
}
/// Ordered commands of one callback. The longest is a `Fire` that both spawns
/// the shot and starts `Shoot CD` on the same tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Actions {
    commands: [Option<Action>; 2],
    len: u8,
}
impl Actions {
    const fn new() -> Self {
        Self {
            commands: [None; 2],
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
/// Sampled once per tick. The three ranges are `CheckAlertRangeByName` against
/// `Alert Range New` and `Attack Range`, and the `Unalert Range` bool the
/// room-sized trigger child drives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Senses {
    pub in_alert_range: bool,
    pub in_attack_range: bool,
    pub in_unalert_range: bool,
    pub completed: Option<Animation>,
    /// `Can Roller?`: PlayerData `fireballLevel` above 0 and no spat roller
    /// still alive. False keeps `Attack Choose` on GOOP without drawing its
    /// 50/50, so a Knight without the spell sees the exact cycle he always did.
    pub can_roller: bool,
}
impl Default for Senses {
    fn default() -> Self {
        Self {
            in_alert_range: false,
            in_attack_range: false,
            // A placement with no trigger child has the bool authored true, and
            // `sleeps` is what carries that; the default here is the awake one.
            in_unalert_range: true,
            completed: None,
            can_roller: false,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Blocker {
    phase: Phase,
    /// `Unalert Range` can reach this placement's bool. False where the source
    /// authored it true and left no trigger child to clear it, which is a
    /// Blocker that never returns to `Dormant` once it has opened.
    sleeps: bool,
    /// `Idle`'s `WaitRandom` countdown.
    wait: u16,
    animation: Option<Animation>,
    serial: u32,
    /// `Rollering`, which `Roller` sets and `Goop` clears.
    rollering: bool,
}
impl Blocker {
    /// `Pause` -> `Init` -> `Direction` -> `Right`/`Left` -> `Dormant`, which
    /// the source walks through on its first two frames. `Init`'s own work is
    /// the invincibility, the `Closed` clip and the four `FindChild` lookups,
    /// all of which are the state below rather than anything to run.
    pub const fn new(sleeps: bool) -> Self {
        Self {
            phase: Phase::Dormant,
            sleeps,
            wait: 0,
            animation: None,
            serial: 0,
            rollering: false,
        }
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn animation(&self) -> Option<Animation> {
        self.animation
    }
    pub fn clip(&self) -> Clip {
        self.animation.map_or(Clip::Closed, |a| a.clip)
    }
    /// `HealthManager::invincible` as the FSM's `SetInvincible` calls leave it.
    /// `invincibleFromDirection` stays at the authored 0 throughout, so this is
    /// a plain flag and every cardinal is blocked while it is set.
    pub fn invincible(&self) -> bool {
        matches!(
            self.phase,
            Phase::Dormant | Phase::Close1 { .. } | Phase::Close2 { .. } | Phase::Shut
        )
    }
    pub fn die(&mut self) {
        self.phase = Phase::Dead;
        self.animation = None;
    }
    /// `HealthManager::TakeDamage`'s TOOK DAMAGE, which this FSM answers with a
    /// global transition from every state. Only a hit that was not blocked
    /// sends it, so a shut Blocker never reaches here.
    ///
    /// `Hit Pause` is a single `NextFrameEvent` before `Hit`; that frame is not
    /// reproduced, and neither is the `BLOCKER DAMAGED` it sends to a host FSM
    /// this FSM does not have.
    pub fn took_damage(&mut self) -> Actions {
        let mut out = Actions::new();
        if self.phase != Phase::Dead {
            self.phase = Phase::Hit;
            self.play(Clip::Hit, &mut out);
        }
        out
    }
    fn play(&mut self, clip: Clip, out: &mut Actions) {
        self.serial = self.serial.wrapping_add(1);
        let token = Animation {
            clip,
            serial: self.serial,
        };
        self.animation = Some(token);
        out.push(Action::Play(token));
    }
    /// `Get Fireball` -> `Action?` -> `Open`. The first reads `fireballLevel`
    /// for a branch this controller does not take and the second is an audio
    /// snapshot, so neither has anything to do here.
    fn open(&mut self, out: &mut Actions) {
        self.phase = Phase::Opening;
        self.play(Clip::Open, out);
    }
    /// `Close` (`sleep` false) or `Sleep 1` (true). Both raise the shell again.
    fn close(&mut self, sleep: bool, out: &mut Actions) {
        self.phase = Phase::Close1 { sleep };
        self.play(Clip::Close1, out);
    }
    /// `Idle`, entered from `Open`, `Shot Anim End` or `Hit`.
    ///
    /// The source runs both of `Idle`'s `BoolTest`s on entry, in the authored
    /// order: `In Attack Range` first, then `Unalert Range`. So a hero standing
    /// underneath shuts the Blocker before the sleep test is ever reached, and
    /// that order is kept here rather than merged into one condition.
    fn enter_idle(&mut self, senses: Senses, wait: u16, out: &mut Actions) {
        self.phase = Phase::Idle;
        self.wait = wait;
        self.play(Clip::Idle, out);
        if senses.in_attack_range {
            self.close(false, out);
        } else if self.sleeps && !senses.in_unalert_range {
            self.close(true, out);
        }
    }
    /// One 60 Hz tick.
    ///
    /// `sample` draws an inclusive range from the caller's own deterministic
    /// generator, standing in for `WaitRandom` and `RandomFloat`. It is not
    /// Unity RNG parity, the same way the Runner's and the Shield's are not.
    pub fn tick(&mut self, senses: Senses, mut sample: impl FnMut([i32; 2]) -> i32) -> Actions {
        let mut out = Actions::new();
        let complete = self.animation.is_some() && senses.completed == self.animation;
        match self.phase {
            Phase::Dead => {}
            // `Dormant`'s `CheckAlertRangeByName`/`BoolTest` pair, every frame.
            Phase::Dormant => {
                if senses.in_alert_range {
                    self.open(&mut out);
                }
            }
            Phase::Opening => {
                if complete {
                    let wait = Self::wait_ticks(&mut sample);
                    self.enter_idle(senses, wait, &mut out);
                }
            }
            Phase::Idle => {
                // `Idle`'s only every-frame test; the sleep one ran on entry.
                if senses.in_attack_range {
                    self.close(false, &mut out);
                } else if self.wait == 0 {
                    // `Attack Choose`: SendRandomEvent 0.5 between GOOP and
                    // ROLLER, and `Can Roller?` folds ROLLER into GOOP unless
                    // the spell is had and no roller is out.
                    self.rollering = senses.can_roller && sample([0, 1]) == 1;
                    self.phase = Phase::Antic;
                    self.play(Clip::Antic, &mut out);
                } else {
                    self.wait -= 1;
                }
            }
            Phase::Antic => {
                if complete {
                    self.fire(&mut sample, &mut out);
                }
            }
            // `Shot Anim End`, which carries the same every-frame attack test
            // as `Idle` and otherwise waits `Shoot CD` out.
            Phase::Cooldown => {
                if senses.in_attack_range {
                    self.close(false, &mut out);
                } else if complete {
                    let wait = Self::wait_ticks(&mut sample);
                    self.enter_idle(senses, wait, &mut out);
                }
            }
            Phase::Close1 { sleep } => {
                if complete {
                    self.phase = Phase::Close2 { sleep };
                    self.play(Clip::Close2, &mut out);
                }
            }
            Phase::Close2 { sleep } => {
                if complete {
                    self.phase = if sleep { Phase::Dormant } else { Phase::Shut };
                    self.play(Clip::Closed, &mut out);
                }
            }
            // `Closed`: FAR is the attack test reading false.
            Phase::Shut => {
                if !senses.in_attack_range {
                    self.open(&mut out);
                }
            }
            Phase::Hit => {
                if complete {
                    let wait = Self::wait_ticks(&mut sample);
                    self.enter_idle(senses, wait, &mut out);
                }
            }
        }
        out
    }
    fn wait_ticks(sample: &mut impl FnMut([i32; 2]) -> i32) -> u16 {
        let value = sample(IDLE_TICKS);
        assert!(
            value >= IDLE_TICKS[0] && value <= IDLE_TICKS[1],
            "Blocker idle wait outside WaitRandom(0.8, 1.2)"
        );
        value as u16
    }
    /// `Fire`: the shot, then `Shoot CD`, then straight on to `Shot Anim End`.
    fn fire(&mut self, sample: &mut impl FnMut([i32; 2]) -> i32, out: &mut Actions) {
        let vx = sample(SHOT_VX);
        assert!(
            vx >= SHOT_VX[0] && vx <= SHOT_VX[1],
            "Blocker shot speed outside RandomFloat(X Speed Min, X Speed Max)"
        );
        self.phase = Phase::Cooldown;
        out.push(if self.rollering {
            Action::Roller {
                offset: SHOT_ORIGIN,
                velocity: [vx, SHOT_VY],
            }
        } else {
            Action::Fire {
                offset: SHOT_ORIGIN,
                velocity: [vx, SHOT_VY],
            }
        });
        self.play(Clip::Cooldown, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Midpoint of whatever range is asked for, so a test reads as the source's
    /// own timings rather than as a sampler's.
    fn mid(range: [i32; 2]) -> i32 {
        (range[0] + range[1]) / 2
    }
    /// Advance `ticks` frames, feeding each tick's clip completion back in, and
    /// answer how many actions the controller asked for along the way.
    fn run(blocker: &mut Blocker, senses: Senses, ticks: usize) -> usize {
        let mut seen = 0;
        for _ in 0..ticks {
            let mut senses = senses;
            senses.completed = blocker.animation();
            seen += blocker.tick(senses, mid).len();
        }
        seen
    }

    #[test]
    fn dormant_is_invincible_until_the_alert_range_opens_it() {
        let mut blocker = Blocker::new(true);
        assert!(blocker.invincible());
        assert_eq!(blocker.clip(), Clip::Closed);
        // Nothing in range: it stays shut and hands out no work at all.
        assert!(blocker.tick(Senses::default(), mid).is_empty());
        let senses = Senses {
            in_alert_range: true,
            ..Senses::default()
        };
        let actions = blocker.tick(senses, mid);
        assert_eq!(actions.len(), 1);
        assert_eq!(blocker.phase(), Phase::Opening);
        assert_eq!(blocker.clip(), Clip::Open);
        // `Open` clears the invincibility on entry, before the clip finishes.
        assert!(!blocker.invincible());
    }

    #[test]
    fn a_full_shot_cycle_waits_anticipates_and_fires_once() {
        let mut blocker = Blocker::new(true);
        let senses = Senses {
            in_alert_range: true,
            ..Senses::default()
        };
        blocker.tick(senses, mid);
        // Open completes, Idle starts its WaitRandom.
        let mut senses = senses;
        senses.completed = blocker.animation();
        blocker.tick(senses, mid);
        assert_eq!(blocker.phase(), Phase::Idle);
        let wait = mid(IDLE_TICKS) as usize;
        let idle = run(
            &mut blocker,
            Senses {
                in_alert_range: true,
                ..Senses::default()
            },
            wait,
        );
        // The wait is spent in Idle and nothing else plays while it runs.
        assert_eq!(idle, 0);
        assert_eq!(blocker.phase(), Phase::Idle);
        let actions = blocker.tick(
            Senses {
                in_alert_range: true,
                ..Senses::default()
            },
            mid,
        );
        assert_eq!(actions.len(), 1);
        assert_eq!(blocker.phase(), Phase::Antic);
        // The antic runs out and the shot leaves with `Shoot CD` behind it.
        let mut senses = Senses {
            in_alert_range: true,
            ..Senses::default()
        };
        senses.completed = blocker.animation();
        let actions = blocker.tick(senses, mid);
        assert_eq!(actions.len(), 2);
        assert_eq!(
            actions.iter().next(),
            Some(Action::Fire {
                offset: SHOT_ORIGIN,
                velocity: [mid(SHOT_VX), SHOT_VY]
            })
        );
        assert_eq!(blocker.phase(), Phase::Cooldown);
        assert_eq!(blocker.clip(), Clip::Cooldown);
        // Never invincible anywhere in the open half of the cycle.
        assert!(!blocker.invincible());
    }

    #[test]
    fn the_hero_underneath_shuts_it_and_leaving_opens_it_again() {
        let mut blocker = Blocker::new(true);
        let mut senses = Senses {
            in_alert_range: true,
            in_attack_range: true,
            ..Senses::default()
        };
        blocker.tick(senses, mid);
        senses.completed = blocker.animation();
        // `Idle`'s entry test sees the hero underneath and closes at once.
        blocker.tick(senses, mid);
        assert_eq!(blocker.phase(), Phase::Close1 { sleep: false });
        assert_eq!(blocker.clip(), Clip::Close1);
        assert!(blocker.invincible());
        senses.completed = blocker.animation();
        blocker.tick(senses, mid);
        assert_eq!(blocker.phase(), Phase::Close2 { sleep: false });
        senses.completed = blocker.animation();
        blocker.tick(senses, mid);
        // `Closed`, not `Dormant`: a hero underneath is not a hero gone.
        assert_eq!(blocker.phase(), Phase::Shut);
        assert!(blocker.invincible());
        // It stays shut while the hero is still there, then reopens.
        assert!(blocker.tick(senses, mid).is_empty());
        let actions = blocker.tick(
            Senses {
                in_alert_range: true,
                ..Senses::default()
            },
            mid,
        );
        assert_eq!(actions.len(), 1);
        assert_eq!(blocker.phase(), Phase::Opening);
    }

    #[test]
    fn only_a_placement_with_the_trigger_child_ever_sleeps() {
        for sleeps in [true, false] {
            let mut blocker = Blocker::new(sleeps);
            let mut senses = Senses {
                in_alert_range: true,
                in_unalert_range: false,
                ..Senses::default()
            };
            blocker.tick(senses, mid);
            senses.completed = blocker.animation();
            blocker.tick(senses, mid);
            if sleeps {
                assert_eq!(blocker.phase(), Phase::Close1 { sleep: true });
                // Both shut clips, then back to `Dormant` rather than `Closed`.
                senses.completed = blocker.animation();
                blocker.tick(senses, mid);
                senses.completed = blocker.animation();
                blocker.tick(senses, mid);
                assert_eq!(blocker.phase(), Phase::Dormant);
                assert!(blocker.invincible());
            } else {
                assert_eq!(blocker.phase(), Phase::Idle);
            }
        }
    }

    #[test]
    fn took_damage_interrupts_the_attack_and_restarts_the_wait() {
        let mut blocker = Blocker::new(true);
        let senses = Senses {
            in_alert_range: true,
            ..Senses::default()
        };
        blocker.tick(senses, mid);
        let mut senses = senses;
        senses.completed = blocker.animation();
        blocker.tick(senses, mid);
        run(
            &mut blocker,
            Senses {
                in_alert_range: true,
                ..Senses::default()
            },
            mid(IDLE_TICKS) as usize + 1,
        );
        assert_eq!(blocker.phase(), Phase::Antic);
        let actions = blocker.took_damage();
        assert_eq!(actions.len(), 1);
        assert_eq!(blocker.phase(), Phase::Hit);
        assert_eq!(blocker.clip(), Clip::Hit);
        // A hit lands only while the shell is down, and `Hit` does not raise it.
        assert!(!blocker.invincible());
        let mut senses = Senses {
            in_alert_range: true,
            ..Senses::default()
        };
        senses.completed = blocker.animation();
        blocker.tick(senses, mid);
        assert_eq!(blocker.phase(), Phase::Idle);
        // A hero who keeps hitting never lets the wait run out, so the Blocker
        // never fires: the source's own answer to being stood next to.
        for _ in 0..4 {
            blocker.took_damage();
            let mut senses = Senses {
                in_alert_range: true,
                ..Senses::default()
            };
            senses.completed = blocker.animation();
            blocker.tick(senses, mid);
            assert_eq!(blocker.phase(), Phase::Idle);
        }
    }

    #[test]
    fn death_stops_every_clip_and_every_later_tick() {
        let mut blocker = Blocker::new(true);
        let senses = Senses {
            in_alert_range: true,
            ..Senses::default()
        };
        blocker.tick(senses, mid);
        blocker.die();
        assert_eq!(blocker.phase(), Phase::Dead);
        assert_eq!(blocker.animation(), None);
        assert!(!blocker.invincible());
        assert!(blocker.tick(senses, mid).is_empty());
        assert!(blocker.took_damage().is_empty());
    }

    #[test]
    fn clip_slots_cover_the_controller_array_exactly_once() {
        let carried = [
            Clip::Open,
            Clip::Close1,
            Clip::Close2,
            Clip::Antic,
            Clip::Cooldown,
            Clip::Hit,
        ];
        let mut seen = [false; Clip::COUNT];
        for clip in carried {
            let slot = clip.slot().expect("carried clip has a controller slot");
            assert!(!seen[slot], "{clip:?} reuses slot {slot}");
            seen[slot] = true;
        }
        assert!(seen.iter().all(|s| *s));
        assert_eq!(Clip::Idle.slot(), None);
        assert_eq!(Clip::Closed.slot(), None);
    }

    /// `Attack Choose` only reaches ROLLER when `Can Roller?` lets it, and
    /// then the same `Fire` launches a Roller instead of the goop.
    #[test]
    fn the_roller_branch_needs_can_roller_and_keeps_the_goop_launch() {
        for (can_roller, pick, rolls) in [(false, 1, false), (true, 0, false), (true, 1, true)] {
            let mut blocker = Blocker::new(true);
            let awake = Senses {
                in_alert_range: true,
                can_roller,
                ..Senses::default()
            };
            let mut sample = |r: [i32; 2]| if r == [0, 1] { pick } else { mid(r) };
            blocker.tick(awake, &mut sample);
            let mut fired = None;
            for _ in 0..400 {
                let mut senses = awake;
                senses.completed = blocker.animation();
                for action in blocker.tick(senses, &mut sample).iter() {
                    match action {
                        Action::Fire { velocity, .. } => fired = fired.or(Some((false, velocity))),
                        Action::Roller { velocity, .. } => fired = fired.or(Some((true, velocity))),
                        _ => {}
                    }
                }
            }
            let (rolled, velocity) = fired.expect("a shot or a roller within 400 ticks");
            assert_eq!(rolled, rolls, "can_roller {can_roller} pick {pick}");
            assert_eq!(velocity, [mid(SHOT_VX), SHOT_VY]);
        }
    }
}
