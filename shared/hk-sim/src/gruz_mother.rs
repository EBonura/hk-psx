//! Source-derived Gruz Mother (`Giant Fly`, Crossroads_04).
//!
//! Every constant here is read back out of the serialized FSMs and prefabs by
//! host/gruz_mother_art.py, which asserts the same values against the
//! installed source and fails the cook if they move.
//!
//! The source is four state machines that run one after another on one body
//! and two prefabs, and so is this:
//!
//! * `Big Fly Control` on `Giant Fly`: asleep and invincible until the hero is
//!   inside `Battle Range` (then only asleep), woken by the first hit, then a
//!   loop of `Buzz` and a super attack: a charge at the hero that rebounds off
//!   whatever it hits, or a slam that bounces between floor and ceiling.
//! * `bouncer_control` on the same body, which flies it during `Buzz`. It is
//!   the Gruzzer's `Bouncer Control` (its table copied here) at speed 5, woken by
//!   `Buzz` and stopped by `Super Choose`.
//! * The corpse prefab's `corpse` (`Corpse Big Fly 1`): no Rigidbody2D, so it
//!   hangs where the body died, steams, and blows.
//! * The `Corpse Big Fly Burster` it blows out: flung, it drops 50 Geo, lands,
//!   gurgles, bursts and moves `Fly Spawn` (the seven parked Gruzzers) to
//!   itself, which is what wakes them.
//!
//! The caller owns the bodies against terrain (it reports the blocked side
//! after each step), the hero, the HealthManager (90 hp, invincible until the
//! hero enters `Battle Range`), Geo, the parked flies, every draw, sound and
//! effect, and the arena (`crate::boss::Arena`). This type owns what happens
//! next.
use crate::gruzzer::Side;
use crate::ONE;

pub const HEALTH: i16 = 90;
/// HealthManager.NonFatalHit's evasionByHitRemaining, the IL literal 0.2 s.
/// The serialized invulnerableTime (0.25 s) is read by nothing.
pub const INVULNERABLE_TICKS: u16 = 12;
/// `Hero Damager`'s DamageHero, live from `Fly` on.
pub const CONTACT_DAMAGE: u16 = 1;
/// `Wake`: SetVelocity2d (0, 2.5) while `Wake` plays once (4 frames at 10 fps).
pub const WAKE_SPEED_Y: i32 = 163840;
pub const WAKE_TICKS: u16 = 24;
/// `Fly`: velocity 0 and Wait 1 s, then `Hero Damager` and the music.
pub const FLY_TICKS: u16 = 60;
/// `bouncer_control` Speed.
pub const BUZZ_SPEED: i32 = 5 * ONE;
/// `Buzz`: RandomFloat 2..2.8 s `Super Wait`.
pub const SUPER_WAIT_TICKS: [u16; 2] = [120, 168];
/// `Super Choose`'s SendRandomEventV2: CHARGE and SLAM at weight 1, each
/// refused once its tracking int reaches its max (3 and 2).
pub const CHOOSE_MAX: [u8; 2] = [3, 2];
/// `Charge Antic`/`Slam Antic`: CANCEL MOVE when `Charges In A Row` is above
/// 3, `Slams In A Row` above 2.
pub const CHARGES_IN_A_ROW: u8 = 3;
pub const SLAMS_IN_A_ROW: u8 = 2;
/// `Charge Antic`: back off from the hero at 3. Its Wait is 0.75 s, but the
/// state ends when the `Charge Antic` clip (4 frames at 12 fps) completes:
/// `Wake`'s Tk2dWatchAnimationEvents assigned the animator's AnimationCompleted
/// delegate and nothing clears it, so every Once clip that completes later
/// sends FINISHED to whatever state is active. The original's trace shows it
/// (19 test frames from `Charge Antic` to `Charge`, 19 from `Slam Antic` to
/// `Launch Up`).
pub const CHARGE_ANTIC_TICKS: u16 = 20;
pub const CHARGE_BACK_SPEED: i32 = 3 * ONE;
/// `Charge`: SetVelocityAsAngle at the hero, 26.
pub const CHARGE_SPEED: i32 = 26 * ONE;
/// `Charge Recover *`: half the charge velocity, mirrored off the wall, 0.3 s.
pub const CHARGE_RECOVER_TICKS: u16 = 18;
/// `Recover End` sets `Super End Time` 0.5; `Slam End` sets 0.
pub const SUPER_END_TICKS: u16 = 30;
/// `Slam Antic`: jitter, ended by the same `Charge Antic` clip completing
/// before its 0.5 s Wait (see `CHARGE_ANTIC_TICKS`); `Slam Time` RandomFloat
/// 2.5..3 s.
pub const SLAM_ANTIC_TICKS: u16 = 20;
pub const SLAM_TICKS: [u16; 2] = [150, 180];
pub const SLAM_SPEED: i32 = 50 * ONE;
/// `Go Left` 100/260 degrees, `Go Right` 80/280: (cos, sin) of 80 degrees, Q16.
pub const SLAM_DIRECTION: [i32; 2] = [11380, 64540];
/// `Slam Down`/`Slam Up` play once (2 frames at 8 fps).
pub const SLAM_HIT_TICKS: u16 = 15;
/// `Slam Down`'s Translate (0, -0.5): the body sinks into the floor it hit and
/// Box2D pushes it back out only slowly (the original launches from 0.27
/// lower than where it landed).
pub const SLAM_SINK: i32 = ONE / 2;
/// `Slam End`: Wait 0.75 s (the clip loops a section, so the wait ends it).
pub const SLAM_END_TICKS: u16 = 45;
/// DecelerateV2 0.85 per 50 Hz FixedUpdate, as a 60 Hz factor: 0.85^(5/6).
pub const SLAM_DECEL: i32 = 57235;
/// The corpse's `Init` 0.5 s, `Steam` 3 s and `Ready` 1 s.
pub const CORPSE_TICKS: [u16; 3] = [30, 180, 60];
/// `Blow`: SetVelocity2d on the burster, x = the corpse's x scale times 10
/// (+/-1.25 * 10), y 20.
pub const BURSTER_SPEED: [i32; 2] = [819200, 20 * ONE];
/// The burster's Rigidbody2D gravityScale 1 and ObjectBounce 0.5 (threshold 1).
pub const BURSTER_GRAVITY: i32 = 60 * ONE;
pub const BURSTER_BOUNCE: i32 = 32768;
pub const BURSTER_BOUNCE_THRESHOLD: i32 = ONE;
/// `Initiate` 0.1 s, then `Geo` flings 50 Geo Small.
pub const BURSTER_INIT_TICKS: u16 = 6;
pub const BURSTER_GEO: u16 = 50;
/// `Landed` 1 s, `Stop Emit` 0.5 s, `Stop` 2 s, `Gurg 1` 2 s, `Gurg 2` 2 s,
/// `Gurg 3` 1.9 s, `Burst` 0.16 s.
pub const BURSTER_TICKS: [u16; 7] = [60, 30, 120, 120, 120, 114, 10];
/// `Battle Control`'s `Start`: Battle Enemies 7, the seven reserve flies.
pub const BATTLE_ENEMIES: i32 = 7;

/// Every clip the fight plays, in host/gruz_mother_art.py's bank order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Clip {
    Sleep,
    Wake,
    Fly,
    ChargeAntic,
    Charge,
    ChargeRecover,
    SlamDown,
    SlamUp,
    SlamEnd,
    /// The corpse prefab's default `Fly`, then `Death` from `Steam`.
    CorpseFly,
    Death,
    /// The burster's default `Fall`, then `Wiggle`, `Stop`, the gurgles, `Burst`.
    Fall,
    Wiggle,
    Stop,
    GurgleOnce,
    GurgleLoop,
    Burst,
}
pub const CLIPS: usize = 17;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// `Invincible`: asleep, and the nail is refused until the hero is in range.
    Invincible,
    /// `Sleep`: asleep and vulnerable; the first hit wakes it.
    Sleep,
    Wake,
    Fly,
    Buzz,
    ChargeAntic,
    Charge,
    ChargeRecover,
    SuperEnd,
    SlamAntic,
    /// `Launch Up`/`Launch Down`'s NextFrameEvent before `Flying`.
    Launch,
    Flying,
    SlamHit,
    SlamEnd,
    CorpseInit,
    CorpseSteam,
    CorpseReady,
    BursterInit,
    BursterAir,
    Landed,
    StopEmit,
    Stop,
    Gurg1,
    Gurg2,
    Gurg3,
    Burst,
    /// `Spawn Flies 2` has run: the burster rests on its last frame.
    Spawned,
    /// `Battle Control`'s `Activate` destroyed it on a won arena.
    Gone,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sound {
    /// `Wake Sound`: big_fly_snore_startle.
    Startle,
    /// The recover and slam states: big_fly_wall_hit.
    WallHit,
    /// The corpse: `Init` boss_final_hit, `Steam` boss_gushing, `Blow` the
    /// explosion and the Boss Defeat sting.
    FinalHit,
    Gushing,
    Explode,
    /// The burster's gurgles: big_fly_stomache_problems_1, _2 and
    /// _final_and_explode.
    Gurgle1,
    Gurgle2,
    GurgleFinal,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Loop {
    /// AudioStop on the body.
    Off,
    /// big_fly_flying, the body's own AudioSource.
    Flying,
    /// big_fly_charge_loop, swapped in by `Charge`'s SetAudioClip.
    Charge,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shake {
    Average,
    Big,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Play(Clip),
    /// SetInvincible on the body.
    Invincible(bool),
    /// `Wake`'s DestroyObject on `Snore` (its loop and its zzz).
    SnoreOff,
    /// `Wake`: the BIGFLY area title, and START to `Battle Scene`.
    Title,
    StartBattle,
    /// `Fly`: ActivateGameObject `Hero Damager`, and ApplyMusicCue EnemyBattle.
    HeroDamager,
    Music,
    Loop(Loop),
    Sound(Sound),
    Shake(Shake),
    /// CameraShake `RumblingMed` (corpse `Steam`) or `RumblingSmall` (`Gurg 3`).
    Rumble(bool),
    /// A wall or slam impact: the dust, the slam effect and the rocks.
    Impact,
    /// The death: the corpse replaces the body, and its `Init` sets the
    /// arena's `Activated`.
    Died,
    /// `Blow`'s one-shot of the Boss Defeat sting.
    Sting,
    /// `Blow`: the corpse is gone and the burster is flung from where it hung.
    Blown,
    /// The burster's `Geo` state: FlingObjectsFromGlobalPool 50 Geo Small.
    Geo,
    /// `Spawn`: the burst's blood and effects.
    Burst,
    /// `Spawn Flies 2`: `Fly Spawn` moved to the burster.
    ReleaseFlies,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Actions {
    values: [Option<Action>; 12],
    count: u8,
}
impl Actions {
    const fn new() -> Self {
        Self {
            values: [None; 12],
            count: 0,
        }
    }
    fn push(&mut self, action: Action) {
        self.values[self.count as usize] = Some(action);
        self.count += 1;
    }
    pub fn iter(&self) -> impl Iterator<Item = Action> + '_ {
        self.values[..self.count as usize]
            .iter()
            .map(|a| a.unwrap())
    }
    pub fn contains(&self, action: Action) -> bool {
        self.iter().any(|a| a == action)
    }
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}
/// What the body's velocity is this tick. `Polar` is the bouncer's angle,
/// which the caller turns into a vector with its own sine table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    Vector([i32; 2]),
    Polar { angle: i32, speed: i32 },
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Senses {
    pub position: [i32; 2],
    pub hero: [i32; 2],
    /// The hero's collider inside `Battle Range`. Read only while asleep.
    pub hero_in_range: bool,
    /// The side the caller's last step was blocked on, in CheckCollisionSide
    /// order (up, right, down, left).
    pub bonk: Option<Side>,
    /// The burster touched the floor this step.
    pub landed: bool,
}

/// `bouncer_control`: the Gruzzer's `Bouncer Control` (crate::gruzzer) with
/// its Speed 5 and `Starts Inactive`, so it waits `Stopped` until a WAKE.
/// Its own copy rather than crate::gruzzer's: this boss streams as its own
/// code module, and a module may not call into another one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Bouncer {
    flying: bool,
    /// Q16 degrees, counter-clockwise from +x.
    angle: i32,
    facing_right: bool,
    rng: u32,
}
impl Bouncer {
    fn random(&mut self) -> u32 {
        self.rng = self.rng.wrapping_mul(1664525).wrapping_add(1013904223);
        self.rng >> 8
    }
    /// RandomFloat over [low, high) degrees, then `Left or Right?`.
    fn aim(&mut self, low: i32, high: i32) {
        let span = (high - low) * ONE;
        self.angle = low * ONE + ((self.random() as i64 * span as i64) >> 24) as i32;
        self.facing_right =
            self.angle < 90 * ONE || (self.angle >= 270 * ONE && self.angle < 360 * ONE);
    }
    /// WAKE: `Aim` at RandomFloat 0..360 and fly.
    fn wake(&mut self) {
        self.flying = true;
        self.aim(0, 360);
    }
    /// The global STOP.
    fn stop(&mut self) {
        self.flying = false;
    }
    /// FaceDirection from the velocity: +1 when x is positive.
    fn facing(&self) -> i32 {
        let a = self.angle.rem_euclid(360 * ONE);
        if self.flying && (a < 90 * ONE || a > 270 * ONE) {
            1
        } else {
            -1
        }
    }
    /// The CheckCollisionSide events of `Fly 2`, the Gruzzer's table.
    fn bonk(&mut self, side: Side) {
        if !self.flying {
            return;
        }
        // A table rather than a match: the match compiled to a jump table
        // whose `jr` the hazard scanner could not bound, and it redirected a
        // neighbouring table's entry, which a code module may not have moved.
        // Up and Down choose by facing, Right and Left by the vertical half.
        const RANGES: [[i32; 2]; 8] = [
            [190, 220],
            [320, 350],
            [190, 220],
            [140, 170],
            [140, 170],
            [10, 40],
            [320, 350],
            [10, 40],
        ];
        let by_facing = matches!(side, Side::Up | Side::Down);
        let select = if by_facing {
            self.facing_right
        } else {
            self.angle < 180 * ONE
        };
        let index = match side {
            Side::Up => 0,
            Side::Right => 2,
            Side::Down => 4,
            Side::Left => 6,
        } + select as usize;
        let [low, high] = RANGES[index];
        self.aim(low, high);
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GruzMother {
    phase: Phase,
    timer: u16,
    /// The current wait: `Super Wait`, `Slam Time` or `Super End Time`.
    wait: u16,
    clip: Clip,
    facing_right: bool,
    velocity: [i32; 2],
    bouncer: Bouncer,
    charges: u8,
    slams: u8,
    tracking: [u8; 2],
    slam_up: bool,
    slam_right: bool,
    /// `Timer`, which only `Flying` advances.
    slam_timer: u16,
    /// The unit vector `Charge Antic`'s GetAngleToTarget2D took.
    charge: [i32; 2],
    rng: u32,
}
impl GruzMother {
    pub fn new(seed: u32) -> Self {
        Self {
            phase: Phase::Invincible,
            timer: 0,
            wait: 0,
            clip: Clip::Sleep,
            facing_right: false,
            velocity: [0; 2],
            bouncer: Bouncer {
                flying: false,
                angle: 0,
                facing_right: false,
                rng: seed ^ 0x5bd1_e995,
            },
            charges: 0,
            slams: 0,
            tracking: [0; 2],
            slam_up: false,
            slam_right: false,
            slam_timer: 0,
            charge: [ONE, 0],
            rng: seed,
        }
    }
    pub fn gone() -> Self {
        Self {
            phase: Phase::Gone,
            ..Self::new(1)
        }
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn clip(&self) -> Clip {
        self.clip
    }
    /// The art is authored facing left; `true` draws it mirrored.
    pub fn facing_right(&self) -> bool {
        self.facing_right
    }
    /// SetInvincible: only `Invincible` refuses the nail.
    pub fn invincible(&self) -> bool {
        self.phase == Phase::Invincible
    }
    pub fn asleep(&self) -> bool {
        matches!(self.phase, Phase::Invincible | Phase::Sleep)
    }
    /// Past the HealthManager death: the corpse or the burster.
    pub fn dead(&self) -> bool {
        matches!(
            self.phase,
            Phase::CorpseInit | Phase::CorpseSteam | Phase::CorpseReady
        ) || self.burster()
            || self.phase == Phase::Gone
    }
    /// Sunk by `Slam Down`'s Translate for as long as that state lasts.
    pub fn sunk(&self) -> bool {
        self.phase == Phase::SlamHit && self.clip == Clip::SlamDown
    }
    pub fn burster(&self) -> bool {
        matches!(
            self.phase,
            Phase::BursterInit
                | Phase::BursterAir
                | Phase::Landed
                | Phase::StopEmit
                | Phase::Stop
                | Phase::Gurg1
                | Phase::Gurg2
                | Phase::Gurg3
                | Phase::Burst
                | Phase::Spawned
        )
    }
    /// `Hero Damager` is live from `Fly` until the body dies.
    pub fn hurts(&self) -> bool {
        !self.asleep() && !self.dead() && self.phase != Phase::Wake
    }
    pub fn motion(&self) -> Motion {
        if self.phase == Phase::Buzz && self.bouncer.flying {
            return Motion::Polar {
                angle: self.bouncer.angle,
                speed: BUZZ_SPEED,
            };
        }
        Motion::Vector(self.velocity)
    }
    /// The burster's velocity after a bounce the caller resolved.
    pub fn set_velocity(&mut self, velocity: [i32; 2]) {
        self.velocity = velocity;
    }
    pub fn random(&mut self) -> u32 {
        self.rng = self.rng.wrapping_mul(1664525).wrapping_add(1013904223);
        self.rng >> 8
    }
    fn range(&mut self, [lo, hi]: [u16; 2]) -> u16 {
        lo + (self.random() % (hi - lo + 1) as u32) as u16
    }
    fn enter(&mut self, phase: Phase) {
        self.phase = phase;
        self.timer = 0;
    }
    fn play(&mut self, clip: Clip, out: &mut Actions) {
        self.clip = clip;
        out.push(Action::Play(clip));
    }
    /// FaceObject at the hero, spriteFacesRight false.
    fn face(&mut self, senses: &Senses) {
        if senses.hero[0] != senses.position[0] {
            self.facing_right = senses.hero[0] > senses.position[0];
        }
    }
    /// `Buzz`: WAKE to the bouncer (Aim at a random angle) and `Super Wait`.
    fn buzz(&mut self, out: &mut Actions) {
        self.enter(Phase::Buzz);
        out.push(Action::Loop(Loop::Flying));
        self.bouncer.wake();
        self.facing_right = self.bouncer.facing() > 0;
        self.wait = self.range(SUPER_WAIT_TICKS);
    }
    /// `Super Choose` with its two CANCEL MOVE loops folded in: the same draw
    /// repeats until an antic accepts it, as the source state machine does
    /// within one frame.
    fn choose(&mut self, senses: &Senses, out: &mut Actions) {
        self.velocity = [0; 2];
        self.bouncer.stop();
        loop {
            // SendRandomEventV2: equal weights; a pick whose tracking int has
            // reached its max is drawn again.
            let pick = (self.random() & 1) as usize;
            if self.tracking[pick] >= CHOOSE_MAX[pick] {
                continue;
            }
            let count = self.tracking[pick] + 1;
            self.tracking = [0; 2];
            self.tracking[pick] = count;
            if pick == 0 {
                if self.charges > CHARGES_IN_A_ROW {
                    continue;
                }
                self.charges += 1;
                self.slams = 0;
                self.enter(Phase::ChargeAntic);
                out.push(Action::Loop(Loop::Off));
                self.play(Clip::ChargeAntic, out);
                self.face(senses);
                // GetAngleToTarget2D then 180 degrees round at speed 3.
                let toward = direction(senses.position, senses.hero);
                self.velocity = [
                    -scale(toward[0], CHARGE_BACK_SPEED),
                    -scale(toward[1], CHARGE_BACK_SPEED),
                ];
                // The charge angle is taken here and kept for `Charge`.
                self.charge = toward;
            } else {
                if self.slams > SLAMS_IN_A_ROW {
                    continue;
                }
                self.slams += 1;
                self.charges = 0;
                self.enter(Phase::SlamAntic);
                self.play(Clip::ChargeAntic, out);
                self.face(senses);
                // `Check Direction` compares the gap read here.
                self.slam_right = senses.hero[0] - senses.position[0] > 0;
                self.wait = self.range(SLAM_TICKS);
            }
            return;
        }
    }
    fn launch(&mut self, up: bool, out: &mut Actions) {
        self.slam_up = up;
        self.enter(Phase::Launch);
        self.play(Clip::Fly, out);
        self.velocity = self.slam_velocity();
    }
    fn slam_velocity(&self) -> [i32; 2] {
        let x = if self.slam_right {
            SLAM_DIRECTION[0]
        } else {
            -SLAM_DIRECTION[0]
        };
        let y = if self.slam_up {
            SLAM_DIRECTION[1]
        } else {
            -SLAM_DIRECTION[1]
        };
        [scale(x, SLAM_SPEED), scale(y, SLAM_SPEED)]
    }
    fn super_end(&mut self, ticks: u16, out: &mut Actions) {
        self.enter(Phase::SuperEnd);
        self.play(Clip::Fly, out);
        out.push(Action::Loop(Loop::Flying));
        self.wait = ticks;
        if ticks == 0 {
            self.buzz(out);
        }
    }

    /// `Flying`: Timer counts, the velocity is set every frame, and a contact
    /// turns the slam (a wall) or lands it (floor or ceiling).
    fn flying(&mut self, senses: &Senses, out: &mut Actions) {
        self.slam_timer = self.slam_timer.saturating_add(1);
        self.velocity = self.slam_velocity();
        if self.slam_timer > self.wait {
            self.enter(Phase::SlamEnd);
            self.play(Clip::SlamEnd, out);
            self.velocity = [
                scale(self.velocity[0], SLAM_DECEL),
                scale(self.velocity[1], SLAM_DECEL),
            ];
            return;
        }
        let Some(side) = senses.bonk else { return };
        match side {
            // `Turn Left`/`Turn Right`: SetScale and the other pair of angles,
            // still going the same way up or down.
            Side::Right | Side::Left => {
                self.slam_right = side == Side::Left;
                self.facing_right = self.slam_right;
                let up = self.slam_up;
                self.launch(up, out);
            }
            // `Slam Down`/`Slam Up`: stop, the impact, then the other way.
            Side::Down | Side::Up => {
                self.slam_up = side == Side::Down;
                self.enter(Phase::SlamHit);
                self.velocity = [0; 2];
                out.push(Action::Sound(Sound::WallHit));
                out.push(Action::Impact);
                out.push(Action::Shake(Shake::Average));
                let clip = if side == Side::Down {
                    Clip::SlamDown
                } else {
                    Clip::SlamUp
                };
                self.play(clip, out);
            }
        }
    }
    /// The HealthManager's TAKE DAMAGE: only `Sleep` answers it.
    pub fn took_damage(&mut self) -> Actions {
        let mut out = Actions::new();
        if self.phase == Phase::Sleep {
            out.push(Action::Sound(Sound::Startle));
            self.wake(&mut out);
        }
        out
    }
    fn wake(&mut self, out: &mut Actions) {
        self.enter(Phase::Wake);
        out.push(Action::Title);
        out.push(Action::SnoreOff);
        out.push(Action::Shake(Shake::Average));
        out.push(Action::StartBattle);
        self.play(Clip::Wake, out);
        self.velocity = [0, WAKE_SPEED_Y];
    }
    /// The HealthManager death: the corpse spawns where the body hangs, and
    /// its `Init` writes the arena's `Activated`.
    pub fn die(&mut self) -> Actions {
        let mut out = Actions::new();
        if self.dead() {
            return out;
        }
        self.bouncer.stop();
        self.velocity = [0; 2];
        self.enter(Phase::CorpseInit);
        out.push(Action::Loop(Loop::Off));
        // `Scale Check` -> `Music` (the mixer to `Silent` over 2 s, which ends
        // the battle music) -> `Init` (boss_final_hit, `Activated`).
        out.push(Action::Died);
        out.push(Action::Sound(Sound::FinalHit));
        self.play(Clip::CorpseFly, &mut out);
        out
    }

    /// One 60 Hz step.
    pub fn tick(&mut self, senses: Senses) -> Actions {
        let mut out = Actions::new();
        self.timer = self.timer.saturating_add(1);
        match self.phase {
            Phase::Invincible => {
                if senses.hero_in_range {
                    self.enter(Phase::Sleep);
                    out.push(Action::Invincible(false));
                }
            }
            Phase::Sleep => {
                if !senses.hero_in_range {
                    self.enter(Phase::Invincible);
                    out.push(Action::Invincible(true));
                }
            }
            Phase::Wake => {
                if self.timer >= WAKE_TICKS {
                    // `Fly` runs every action on entry; its Wait only holds it.
                    self.enter(Phase::Fly);
                    out.push(Action::Loop(Loop::Flying));
                    self.play(Clip::Fly, &mut out);
                    self.velocity = [0; 2];
                    out.push(Action::HeroDamager);
                    out.push(Action::Music);
                }
            }
            Phase::Fly => {
                if self.timer >= FLY_TICKS {
                    self.buzz(&mut out);
                }
            }
            Phase::Buzz => {
                if let Some(side) = senses.bonk {
                    self.bouncer.bonk(side);
                }
                // FaceDirection every frame from the bouncer's velocity.
                self.facing_right = self.bouncer.facing() > 0;
                if self.timer >= self.wait {
                    self.choose(&senses, &mut out);
                }
            }
            Phase::ChargeAntic => {
                if self.timer >= CHARGE_ANTIC_TICKS {
                    self.enter(Phase::Charge);
                    self.play(Clip::Charge, &mut out);
                    out.push(Action::Loop(Loop::Charge));
                    self.velocity = [
                        scale(self.charge[0], CHARGE_SPEED),
                        scale(self.charge[1], CHARGE_SPEED),
                    ];
                }
            }
            Phase::Charge => {
                if let Some(side) = senses.bonk {
                    // GetVelocity2d everyFrame kept the velocity from before the
                    // contact: halve it and mirror the axis that hit.
                    let [vx, vy] = [self.velocity[0] / 2, self.velocity[1] / 2];
                    self.velocity = match side {
                        Side::Left | Side::Right => [-vx, vy],
                        Side::Up | Side::Down => [vx, -vy],
                    };
                    self.enter(Phase::ChargeRecover);
                    out.push(Action::Loop(Loop::Off));
                    out.push(Action::Sound(Sound::WallHit));
                    out.push(Action::Impact);
                    out.push(Action::Shake(Shake::Average));
                    self.play(Clip::ChargeRecover, &mut out);
                }
            }
            Phase::ChargeRecover => {
                if self.timer >= CHARGE_RECOVER_TICKS {
                    // `Recover End`: stop, `Super End Time` 0.5.
                    self.velocity = [0; 2];
                    self.super_end(SUPER_END_TICKS, &mut out);
                }
            }
            Phase::SuperEnd => {
                if self.timer >= self.wait {
                    self.buzz(&mut out);
                }
            }
            Phase::SlamAntic => {
                if self.timer >= SLAM_ANTIC_TICKS {
                    // `Check Direction`, `Go Left`/`Go Right`, `Launch Up`.
                    self.slam_timer = 0;
                    self.launch(true, &mut out);
                }
            }
            // NextFrameEvent: `Flying` is entered, and runs, the next frame.
            Phase::Launch => {
                self.enter(Phase::Flying);
                self.flying(&senses, &mut out);
            }
            Phase::Flying => self.flying(&senses, &mut out),
            Phase::SlamHit => {
                if self.timer >= SLAM_HIT_TICKS {
                    let up = self.slam_up;
                    self.launch(up, &mut out);
                }
            }
            Phase::SlamEnd => {
                self.velocity = [
                    scale(self.velocity[0], SLAM_DECEL),
                    scale(self.velocity[1], SLAM_DECEL),
                ];
                if self.timer >= SLAM_END_TICKS {
                    self.super_end(0, &mut out);
                }
            }
            Phase::CorpseInit => {
                if self.timer >= CORPSE_TICKS[0] {
                    self.enter(Phase::CorpseSteam);
                    self.play(Clip::Death, &mut out);
                    out.push(Action::Sound(Sound::Gushing));
                    out.push(Action::Shake(Shake::Big));
                    out.push(Action::Rumble(true));
                }
            }
            Phase::CorpseSteam => {
                if self.timer >= CORPSE_TICKS[1] {
                    self.enter(Phase::CorpseReady);
                }
            }
            Phase::CorpseReady => {
                if self.timer >= CORPSE_TICKS[2] {
                    // `Blow`, then `Destroy Self`; the burster takes over.
                    self.enter(Phase::BursterInit);
                    out.push(Action::Rumble(false));
                    out.push(Action::Sound(Sound::Explode));
                    out.push(Action::Sting);
                    out.push(Action::Blown);
                    self.play(Clip::Fall, &mut out);
                    let x = if self.facing_right {
                        -BURSTER_SPEED[0]
                    } else {
                        BURSTER_SPEED[0]
                    };
                    self.velocity = [x, BURSTER_SPEED[1]];
                }
            }
            Phase::BursterInit => {
                if self.timer >= BURSTER_INIT_TICKS {
                    self.enter(Phase::BursterAir);
                    out.push(Action::Geo);
                }
            }
            Phase::BursterAir => {
                if senses.landed {
                    self.enter(Phase::Landed);
                    self.play(Clip::Wiggle, &mut out);
                }
            }
            Phase::Landed => {
                if self.timer >= BURSTER_TICKS[0] {
                    self.enter(Phase::StopEmit);
                }
            }
            Phase::StopEmit => {
                if self.timer >= BURSTER_TICKS[1] {
                    self.enter(Phase::Stop);
                    self.play(Clip::Stop, &mut out);
                }
            }
            Phase::Stop => {
                if self.timer >= BURSTER_TICKS[2] {
                    self.enter(Phase::Gurg1);
                    out.push(Action::Sound(Sound::Gurgle1));
                    self.play(Clip::GurgleOnce, &mut out);
                }
            }
            Phase::Gurg1 => {
                if self.timer >= BURSTER_TICKS[3] {
                    self.enter(Phase::Gurg2);
                    out.push(Action::Sound(Sound::Gurgle2));
                    self.play(Clip::GurgleOnce, &mut out);
                }
            }
            Phase::Gurg2 => {
                if self.timer >= BURSTER_TICKS[4] {
                    self.enter(Phase::Gurg3);
                    out.push(Action::Sound(Sound::GurgleFinal));
                    out.push(Action::Rumble(true));
                    self.play(Clip::GurgleLoop, &mut out);
                }
            }
            Phase::Gurg3 => {
                if self.timer >= BURSTER_TICKS[5] {
                    self.enter(Phase::Burst);
                    self.play(Clip::Burst, &mut out);
                }
            }
            Phase::Burst => {
                if self.timer >= BURSTER_TICKS[6] {
                    self.enter(Phase::Spawned);
                    out.push(Action::Rumble(false));
                    out.push(Action::Shake(Shake::Average));
                    out.push(Action::Burst);
                    out.push(Action::ReleaseFlies);
                }
            }
            Phase::Spawned | Phase::Gone => {}
        }
        out
    }
}
fn scale(value: i32, factor: i32) -> i32 {
    ((value as i64 * factor as i64) >> 16) as i32
}
/// The unit vector from `from` to `to`, Q16: GetAngleToTarget2D followed by
/// SetVelocityAsAngle is this times the speed.
pub fn direction(from: [i32; 2], to: [i32; 2]) -> [i32; 2] {
    let d = [to[0] as i64 - from[0] as i64, to[1] as i64 - from[1] as i64];
    let length = psx_math::int32::isqrt_u64((d[0] * d[0] + d[1] * d[1]) as u64) as i64;
    if length == 0 {
        return [ONE, 0];
    }
    [
        (d[0] * ONE as i64 / length) as i32,
        (d[1] * ONE as i64 / length) as i32,
    ]
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    const HERE: [i32; 2] = [100 * ONE, 16 * ONE];

    fn senses(hero: [i32; 2], in_range: bool) -> Senses {
        Senses {
            position: HERE,
            hero,
            hero_in_range: in_range,
            bonk: None,
            landed: false,
        }
    }
    fn run(boss: &mut GruzMother, s: Senses, ticks: u16) -> Vec<Action> {
        let mut all = Vec::new();
        for _ in 0..ticks {
            all.extend(boss.tick(s).iter());
        }
        all
    }
    /// Wake it with a hit in range and run it to its first `Buzz`.
    fn awake(seed: u32) -> GruzMother {
        let mut boss = GruzMother::new(seed);
        boss.tick(senses([95 * ONE, 12 * ONE], true));
        boss.took_damage();
        run(
            &mut boss,
            senses([95 * ONE, 12 * ONE], true),
            WAKE_TICKS + FLY_TICKS,
        );
        assert_eq!(boss.phase(), Phase::Buzz);
        boss
    }

    #[test]
    fn it_sleeps_invincible_until_the_hero_is_in_range() {
        let mut boss = GruzMother::new(7);
        assert!(boss.invincible() && boss.asleep());
        assert!(
            boss.took_damage().is_empty(),
            "a hit out of range is refused, not a wake"
        );
        let actions = boss.tick(senses([90 * ONE, 12 * ONE], true));
        assert!(actions.contains(Action::Invincible(false)));
        assert_eq!(boss.phase(), Phase::Sleep);
        let actions = boss.tick(senses([60 * ONE, 12 * ONE], false));
        assert!(actions.contains(Action::Invincible(true)));
        assert_eq!(boss.phase(), Phase::Invincible);
    }

    #[test]
    fn the_first_hit_wakes_it_into_the_fight() {
        let mut boss = GruzMother::new(7);
        boss.tick(senses([90 * ONE, 12 * ONE], true));
        let wake = boss.took_damage();
        for action in [
            Action::Sound(Sound::Startle),
            Action::Title,
            Action::StartBattle,
            Action::SnoreOff,
            Action::Play(Clip::Wake),
            Action::Shake(Shake::Average),
        ] {
            assert!(wake.contains(action), "{action:?}");
        }
        assert_eq!(boss.motion(), Motion::Vector([0, WAKE_SPEED_Y]));
        assert!(!boss.hurts(), "Hero Damager is still off while it wakes");
        let fly = run(&mut boss, senses([90 * ONE, 12 * ONE], true), WAKE_TICKS);
        assert!(fly.contains(&Action::HeroDamager) && fly.contains(&Action::Music));
        assert_eq!(boss.phase(), Phase::Fly);
        assert_eq!(boss.motion(), Motion::Vector([0; 2]));
        assert!(boss.hurts());
        run(&mut boss, senses([90 * ONE, 12 * ONE], true), FLY_TICKS);
        assert_eq!(boss.phase(), Phase::Buzz);
        let Motion::Polar { speed, .. } = boss.motion() else {
            panic!("Buzz flies by the bouncer's angle")
        };
        assert_eq!(speed, BUZZ_SPEED);
    }

    #[test]
    fn super_choose_never_repeats_past_its_limits() {
        for seed in 1..40u32 {
            let mut boss = awake(seed);
            let mut kinds = Vec::new();
            for _ in 0..40 {
                // Skip to the end of `Super Wait`, then read which antic it chose.
                boss.timer = boss.wait;
                boss.tick(senses([95 * ONE, 12 * ONE], true));
                kinds.push(boss.phase());
                // Back to `Buzz` directly: the attack itself is covered below.
                boss.buzz(&mut Actions::new());
            }
            let mut run_length = 1;
            for pair in kinds.windows(2) {
                run_length = if pair[0] == pair[1] {
                    run_length + 1
                } else {
                    1
                };
                let limit = if pair[1] == Phase::ChargeAntic { 3 } else { 2 };
                assert!(run_length <= limit, "seed {seed}: {kinds:?}");
            }
            assert!(
                kinds.contains(&Phase::ChargeAntic) && kinds.contains(&Phase::SlamAntic),
                "seed {seed}"
            );
        }
    }

    fn until(boss: &mut GruzMother, phase: Phase, s: Senses) {
        for _ in 0..2000 {
            if boss.phase() == phase {
                return;
            }
            boss.tick(s);
        }
        panic!("never reached {phase:?}");
    }

    #[test]
    fn a_charge_backs_off_rushes_the_hero_and_rebounds_off_the_wall() {
        let hero = [110 * ONE, 16 * ONE];
        let mut boss = (1..200)
            .map(awake)
            .find(|b| {
                let mut b = *b;
                b.timer = b.wait;
                b.tick(senses(hero, true));
                b.phase() == Phase::ChargeAntic
            })
            .expect("some seed charges first");
        boss.timer = boss.wait;
        boss.tick(senses(hero, true));
        assert_eq!(boss.phase(), Phase::ChargeAntic);
        assert!(
            boss.facing_right(),
            "FaceObject turns to a hero on the right"
        );
        assert_eq!(boss.motion(), Motion::Vector([-CHARGE_BACK_SPEED, 0]));
        run(&mut boss, senses(hero, true), CHARGE_ANTIC_TICKS);
        assert_eq!(boss.phase(), Phase::Charge);
        assert_eq!(boss.motion(), Motion::Vector([CHARGE_SPEED, 0]));
        let hit = boss.tick(Senses {
            bonk: Some(Side::Right),
            ..senses(hero, true)
        });
        assert!(
            hit.contains(Action::Sound(Sound::WallHit))
                && hit.contains(Action::Play(Clip::ChargeRecover))
        );
        assert_eq!(boss.motion(), Motion::Vector([-CHARGE_SPEED / 2, 0]));
        run(&mut boss, senses(hero, true), CHARGE_RECOVER_TICKS);
        assert_eq!(boss.phase(), Phase::SuperEnd);
        assert_eq!(boss.motion(), Motion::Vector([0; 2]));
        run(&mut boss, senses(hero, true), SUPER_END_TICKS);
        assert_eq!(boss.phase(), Phase::Buzz);
    }

    #[test]
    fn a_slam_bounces_floor_to_ceiling_and_turns_at_walls() {
        let hero = [80 * ONE, 16 * ONE];
        let mut boss = (1..200)
            .map(awake)
            .find(|b| {
                let mut b = *b;
                b.timer = b.wait;
                b.tick(senses(hero, true));
                b.phase() == Phase::SlamAntic
            })
            .expect("some seed slams first");
        boss.timer = boss.wait;
        boss.tick(senses(hero, true));
        assert!(!boss.facing_right());
        run(&mut boss, senses(hero, true), SLAM_ANTIC_TICKS);
        assert_eq!(boss.phase(), Phase::Launch);
        // `Go Left`: up at 100 degrees.
        assert_eq!(
            boss.motion(),
            Motion::Vector([
                -scale(SLAM_DIRECTION[0], SLAM_SPEED),
                scale(SLAM_DIRECTION[1], SLAM_SPEED)
            ])
        );
        boss.tick(senses(hero, true));
        assert_eq!(boss.phase(), Phase::Flying);
        let up = boss.tick(Senses {
            bonk: Some(Side::Up),
            ..senses(hero, true)
        });
        assert!(up.contains(Action::Play(Clip::SlamUp)));
        run(&mut boss, senses(hero, true), SLAM_HIT_TICKS);
        assert_eq!(boss.phase(), Phase::Launch);
        let Motion::Vector(v) = boss.motion() else {
            panic!()
        };
        assert!(
            v[0] < 0 && v[1] < 0,
            "down and still left after the ceiling: {v:?}"
        );
        boss.tick(senses(hero, true));
        boss.tick(Senses {
            bonk: Some(Side::Left),
            ..senses(hero, true)
        });
        assert!(boss.facing_right(), "`Turn Right` mirrors it");
        let Motion::Vector(v) = boss.motion() else {
            panic!()
        };
        assert!(v[0] > 0 && v[1] < 0, "down and now right: {v:?}");
        // Timer runs out: `Slam End` decelerates, then straight back to `Buzz`.
        until(&mut boss, Phase::SlamEnd, senses(hero, true));
        let Motion::Vector(before) = boss.motion() else {
            panic!()
        };
        boss.tick(senses(hero, true));
        let Motion::Vector(after) = boss.motion() else {
            panic!()
        };
        assert!(after[0].abs() < before[0].abs());
        run(&mut boss, senses(hero, true), SLAM_END_TICKS);
        assert_eq!(boss.phase(), Phase::Buzz);
    }

    #[test]
    fn death_steams_blows_and_the_burster_releases_the_flies() {
        let mut boss = awake(3);
        let died = boss.die();
        assert!(died.contains(Action::Died) && died.contains(Action::Play(Clip::CorpseFly)));
        assert!(boss.dead() && !boss.hurts());
        assert!(boss.die().is_empty(), "one death");
        let still = senses([90 * ONE, 12 * ONE], true);
        let steam = run(&mut boss, still, CORPSE_TICKS[0]);
        assert!(
            steam.contains(&Action::Play(Clip::Death)) && steam.contains(&Action::Rumble(true))
        );
        let blow = run(&mut boss, still, CORPSE_TICKS[1] + CORPSE_TICKS[2]);
        assert!(blow.contains(&Action::Blown) && blow.contains(&Action::Sting));
        assert!(boss.burster());
        let x = if boss.facing_right() {
            -BURSTER_SPEED[0]
        } else {
            BURSTER_SPEED[0]
        };
        assert_eq!(boss.motion(), Motion::Vector([x, BURSTER_SPEED[1]]));
        let geo = run(&mut boss, still, BURSTER_INIT_TICKS);
        assert!(geo.contains(&Action::Geo));
        run(&mut boss, still, 30);
        assert_eq!(
            boss.phase(),
            Phase::BursterAir,
            "nothing happens until it lands"
        );
        boss.tick(Senses {
            landed: true,
            ..still
        });
        assert_eq!(boss.phase(), Phase::Landed);
        let rest: u16 = BURSTER_TICKS.iter().sum();
        let tail = run(&mut boss, still, rest - 1);
        assert!(!tail.contains(&Action::ReleaseFlies));
        let release = run(&mut boss, still, 1);
        assert!(release.contains(&Action::ReleaseFlies) && release.contains(&Action::Burst));
        assert_eq!(boss.phase(), Phase::Spawned);
        assert!(run(&mut boss, still, 600).is_empty());
    }

    #[test]
    fn a_won_arena_has_no_body() {
        let mut boss = GruzMother::gone();
        assert!(boss.dead());
        assert!(boss.tick(senses(HERE, true)).is_empty());
        assert!(boss.took_damage().is_empty() && boss.die().is_empty());
    }
}
