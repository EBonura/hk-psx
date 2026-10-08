//! Source-derived Brooding Mawlek (`Mawlek Control`, Crossroads_09).
//!
//! Every constant here was read back out of the serialized FSMs, the Walker
//! and the corpse prefab by host/mawlek_art.py, which asserts the same values
//! against the installed source and fails the cook if they move.
//! tests/test_mawlek.py compares the two sides so they cannot drift.
//!
//! The source is five cooperating state machines on one body, and so is this:
//!
//! * `Mawlek Control` on the body: lurk in the background, wake, jump out,
//!   roar, then alternate a walk with a super attack, a 25-shot spit or a
//!   leap at the hero followed by a leap back to where it woke.
//! * The body's `Walker`, which walks it left and right between attacks. Its
//!   `preventScaleChange` is set, so it turns without ever mirroring.
//! * `Mawlek Arm Control` on each arm: swipe whenever the hero is inside that
//!   arm's `Attack Range`, with a DamageHero collider live for the swipe.
//! * `Mawlek Head`: spit one shot at the hero every 0.3 to 0.6 s.
//! * The corpse prefab's `corpse`: the long death, then the explosion.
//!
//! The super attacks put the arms and the head to sleep and hide the body;
//! the `Dummy` child plays the whole-body art instead. `Super Ready` waits
//! for all three children to finish what they are doing before it chooses.
//!
//! The caller owns the dynamic body, terrain, the hero, the HealthManager
//! (300 hp, invincible until `Start`), the shot pool, every draw and effect,
//! and the arena (`crate::boss::Arena`). This type owns what happens next.
use crate::ONE;

pub const HEALTH: i16 = 300;
/// HealthManager.NonFatalHit's evasionByHitRemaining, the IL literal 0.2 s.
/// The serialized invulnerableTime (0.15 s) is read by nothing.
pub const INVULNERABLE_TICKS: u16 = 12;
pub const CONTACT_DAMAGE: u16 = 1;

/// `Wake`: 0.166 s of `Dummy Intro Jump` before the leap.
pub const WAKE_TICKS: u16 = 10;
/// `Wake Jump`: SetVelocityAsAngle 90 at 53, held 0.1 s with gravity still 0.
pub const WAKE_JUMP_TICKS: u16 = 6;
pub const WAKE_JUMP_SPEED: i32 = 53 * ONE;
/// `Wake In Air` sets gravity scale 3.0 and nothing ever sets it back.
pub const GRAVITY: i32 = 3 * ONE;
/// The body lurks at z 3.16 and `Wake In Air`'s iTweenMoveBy brings it to 0
/// over 0.5 s, linear; `Wake Land` pins z to 0.
pub const LURK_DEPTH: i32 = 207094;
pub const WAKE_DEPTH_TICKS: u16 = 30;
/// `Wake Roar` waits 2 s.
pub const WAKE_ROAR_TICKS: u16 = 120;
/// `Idle`: RandomFloat 2..3 s before the next super attack.
pub const IDLE_TICKS: [u16; 2] = [120, 180];
/// `Jump`/`Jump 2`: SetVelocity2d y 68, held 0.1 s.
pub const JUMP_SPEED_Y: i32 = 68 * ONE;
pub const JUMP_TICKS: u16 = 6;
/// `Detect Hero Pos 3` and `Aim Return`: X Distance is 1.25 times the gap.
pub const JUMP_X_FACTOR: i32 = 81920;
/// `Land` waits 0.5 s; `Land 2` waits 0.25 s and sets Cooldown Time 0.25.
pub const LAND_TICKS: u16 = 30;
pub const LAND_2_TICKS: u16 = 15;
pub const JUMP_COOLDOWN_TICKS: u16 = 15;
/// `Shoot` sets Cooldown Time 1.75.
pub const SPIT_COOLDOWN_TICKS: u16 = 105;
/// `Super Jump` and `Detect Hero Pos 2` switch attack once this many of one
/// kind have gone in a row (IntCompare greaterThan 3).
pub const IN_A_ROW: u8 = 3;
/// `Repeat Check`: a second super attack when RandomFloat 0..100 is above 75.
pub const REPEAT_ABOVE: u32 = 75;
/// `Shoot`: FlingObjectsFromGlobalPool 25 `Shot Mawlek NoDrip` from the body,
/// at Shot Speed 32 to Shot Speed Max 35, between the angles `L`/`R` set.
pub const SPIT_SHOTS: u8 = 25;
pub const SPIT_SPEED: [i32; 2] = [32 * ONE, 35 * ONE];
pub const SPIT_ANGLES_LEFT: [i32; 2] = [92, 105];
pub const SPIT_ANGLES_RIGHT: [i32; 2] = [75, 88];
/// `Mawlek Head`: `Idle` waits RandomFloat 0.3..0.6 s, `Shoot Antic` 0.083 s,
/// `Shoot` 0.25 s or the end of `Head Spit`, whichever is first; one shot at 27.
pub const HEAD_IDLE_TICKS: [u16; 2] = [18, 36];
pub const HEAD_ANTIC_TICKS: u16 = 5;
pub const HEAD_SHOOT_TICKS: u16 = 15;
pub const HEAD_SHOT_SPEED: i32 = 27 * ONE;
pub const HEAD_ANGLES_LEFT: [i32; 2] = [95, 105];
pub const HEAD_ANGLES_RIGHT: [i32; 2] = [75, 85];
/// `Mawlek Arm Control`'s `Re attack Pause`, 0.15 s.
pub const ARM_PAUSE_TICKS: u16 = 9;
/// Walker: walkSpeedR 3 (walkSpeedL -3), pauseWait and pauseTime 1..3 s,
/// and the one-second turn cooldown every Walker starts at BeginTurning.
pub const WALK_SPEED: i32 = 3 * ONE;
pub const WALK_TICKS: [u16; 2] = [60, 180];
pub const PAUSE_TICKS: [u16; 2] = [60, 180];
pub const TURN_COOLDOWN_TICKS: u16 = 60;
/// The corpse's `Init` 1.5 s, `Steam` 3 s and `Ready` 1 s; `Music` before
/// them fades the mixer to `Silent` over 2 s.
pub const CORPSE_TICKS: [u16; 3] = [90, 180, 60];
pub const DEATH_SILENCE_TICKS: u16 = 120;
/// `Battle Control`: `Blow Wait` 5.5 s after BATTLE END, then `End Wait` 5 s,
/// then BG OPEN. `Activated` is written as `Blow Wait` starts.
pub const ARENA_END_TICKS: u16 = 630;
/// `End Wait` activates the Heart Piece this far into the arena's end.
pub const HEART_PIECE_TICKS: u16 = 330;

/// Every clip the fight plays, on whichever part plays it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Clip {
    BodyIdle,
    BodyWalk,
    IdleTurn,
    DummyBlank,
    DummyLurk,
    DummyIntroJump,
    DummyIntroLand,
    DummyRoar,
    RoarCooldown,
    DummyShootAntic,
    DummyShoot,
    DummyJumpAntic,
    DummyJump,
    DummyLand,
    ArmIdle,
    ArmSwipeAntic,
    ArmSwipe,
    ArmSwipeCooldown,
    HeadIdle,
    HeadSpit,
}
/// Whole-clip durations at 60 Hz, in `Clip` order, ceilinged as host/combat.py
/// does: frames / fps. Looping clips report one cycle; nothing waits on them.
pub const CLIP_TICKS: [u16; 20] = [18, 15, 36, 2, 18, 45, 15, 16, 25, 40, 18, 18, 42, 30, 25, 49, 4, 12, 15, 15];
impl Clip {
    pub fn ticks(self) -> u16 {
        CLIP_TICKS[self as usize]
    }
}
/// The objects a clip plays on. `Arm(0)` is `Mawlek Arm R`, `Arm(1)` the
/// mirrored `Mawlek Arm L`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Body,
    Dummy,
    Arm(u8),
    Head,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// `Dormant`: lurking in the background until the hero enters `Alert Range New`.
    Dormant,
    Wake,
    WakeJump,
    WakeAir,
    WakeLand,
    WakeRoar,
    RoarEnd,
    Idle,
    SuperReady,
    SuperSpit,
    Shoot,
    JumpAntic,
    Jump,
    JumpAir,
    Land,
    ReturnJump,
    ReturnAir,
    ReturnLand,
    SuperCooldown,
    /// The corpse's `Init`, `Steam`, `Ready`, then `Blow` and nothing.
    CorpseInit,
    CorpseSteam,
    CorpseReady,
    Gone,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// `Wake Jump`: mawlek_jump_offscreen and mawlek_jump, AverageShake.
    WakeJump,
    /// `Wake Land`: zombie_guard_club, BigShake.
    WakeLand,
    /// `Wake Roar`: mawlek_scream.
    Roar,
    /// `Super Spit`: mawlek_big_spit.
    BigSpit,
    /// `Jump`/`Jump 2`: mawlek_jump, EnemyKillShake.
    Jump,
    /// `Land`/`Land 2`: zombie_guard_club, AverageShake.
    Land,
    /// `Swipe Antic`: mawlek_call, and `Swipe`: mawlek_whip.
    ArmCall,
    ArmWhip,
    /// `Mawlek Head` `Shoot`: mawlek_spit or mawlek_spit_b.
    HeadSpit,
    /// The corpse: `Init` boss_final_hit and AverageShake, `Steam`
    /// boss_gushing and BigShake, `Sting` the Boss Defeat sting, `Blow`
    /// boss_explode and BigShake.
    CorpseInit,
    CorpseSteam,
    Sting,
    Blow,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Play(Part, Clip),
    /// SetMeshRenderer on the body: off while the Dummy carries the art.
    Mesh(bool),
    /// SetScale on the Dummy: +1 draws its art as authored, -1 mirrored.
    DummyScale(i8),
    Velocity([i32; 2]),
    VelocityX(i32),
    Gravity(i32),
    /// `Start`'s SetInvincible false: the body can be hurt from here on.
    Vulnerable,
    /// SetCollider on the Head: its box is a second nail target.
    HeadCollider(bool),
    /// SetProperty PolygonCollider2D.enabled on one arm: its DamageHero.
    ArmHitbox(u8, bool),
    /// FlingObjectsFromGlobalPool from the body: `count` shots, each at a
    /// random speed and angle (degrees) inside these ranges.
    Spray { count: u8, speed: [i32; 2], angles: [i32; 2] },
    /// The Head's one shot, from the Head.
    HeadShot { speed: i32, angles: [i32; 2] },
    /// PLAY to `Spit Effect`.
    SpitEffect,
    /// `Wake`'s START to `Battle Scene`.
    StartBattle,
    /// `Title`: the MAWLEK area title.
    Title,
    /// `Music`: ApplyMusicCue EnemyBattle and the `Normal` snapshot.
    Music,
    /// `Wake Roar`'s ROAR ENTER to the hero's `Roar Lock` (true) and `Roar
    /// End`'s ROAR EXIT (false): the Knight holds still while it roars.
    RoarLock(bool),
    /// The death: the corpse is spawned and its `Music` state sends BATTLE
    /// END and fades the mixer to `Silent`.
    Died,
    /// `Blow`: the corpse's mesh goes off.
    Blown,
    Effect(Effect),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Actions {
    values: [Option<Action>; 16],
    count: u8,
}
impl Actions {
    const fn new() -> Self {
        Self { values: [None; 16], count: 0 }
    }
    fn push(&mut self, action: Action) {
        self.values[self.count as usize] = Some(action);
        self.count += 1;
    }
    pub fn iter(&self) -> impl Iterator<Item = Action> + '_ {
        self.values[..self.count as usize].iter().map(|a| a.unwrap())
    }
    pub fn contains(&self, action: Action) -> bool {
        self.iter().any(|a| a == action)
    }
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Senses {
    pub self_x: i32,
    pub hero_x: i32,
    /// The Head's world x, which is what `Mawlek Head` compares.
    pub head_x: i32,
    /// CheckCollisionSide bottom contact after the caller's step.
    pub grounded: bool,
    /// The hero is inside `Alert Range New`. Read only while dormant.
    pub hero_in_wake: bool,
    /// The hero is inside each arm's `Attack Range`, in `Part::Arm` order.
    pub hero_in_arm: [bool; 2],
    /// The Walker's two Sweeps in the facing it walks: a wall within reach,
    /// and floor under the edge ahead. Read only while it walks.
    pub wall_ahead: bool,
    pub floor_ahead: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Walk {
    Stopped,
    Walking,
    Paused,
    Turning,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Arm {
    Dormant,
    Idle,
    Antic,
    Swipe,
    Cooldown,
    Pause,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Head {
    Dormant,
    Idle,
    Antic,
    Shoot,
}
/// `(cos, sin)` of whole degrees 75..=105, Q16: every angle the spits use.
const ANGLES: [[i32; 2]; 31] = [
    [16962, 63303], [15855, 63589], [14742, 63856], [13626, 64104], [12505, 64332], [11380, 64540],
    [10252, 64729], [9121, 64898], [7987, 65048], [6850, 65177], [5712, 65287], [4572, 65376],
    [3430, 65446], [2287, 65496], [1144, 65526], [0, 65536], [-1144, 65526], [-2287, 65496],
    [-3430, 65446], [-4572, 65376], [-5712, 65287], [-6850, 65177], [-7987, 65048], [-9121, 64898],
    [-10252, 64729], [-11380, 64540], [-12505, 64332], [-13626, 64104], [-14742, 63856], [-15855, 63589],
    [-16962, 63303],
];
/// FlingObjectsFromGlobalPool's velocity: `speed` along `angle` degrees, the
/// angle given in quarter degrees so a Random.Range over a float interval
/// lands between the whole-degree rows, which are interpolated.
pub fn fling(speed: i32, quarter_degrees: i32) -> [i32; 2] {
    let at = (quarter_degrees - 75 * 4).clamp(0, 30 * 4);
    let (row, frac) = ((at / 4) as usize, at % 4);
    let next = (row + 1).min(30);
    let lerp = |k: usize| ANGLES[row][k] + (ANGLES[next][k] - ANGLES[row][k]) * frac / 4;
    [scale(speed, lerp(0)), scale(speed, lerp(1))]
}
/// One FlingObjectsFromGlobalPool draw from `rng`: a speed inside `speed`
/// (to 1/256 of a unit) and an angle inside `angles` (to a quarter degree).
/// The guest's shot spawner draws the Spray's 25 with this, from a seed the
/// controller hands it, so a replay fires the same shots.
pub fn shot_velocity(rng: &mut u32, speed: [i32; 2], angles: [i32; 2]) -> [i32; 2] {
    let mut next = || {
        *rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
        *rng >> 8
    };
    let steps = ((speed[1] - speed[0]) >> 8) as u32 + 1;
    let s = speed[0] + ((next() % steps) << 8) as i32;
    let a = angles[0] * 4 + (next() % ((angles[1] - angles[0]) * 4 + 1) as u32) as i32;
    fling(s, a)
}
fn scale(value: i32, factor: i32) -> i32 {
    ((value as i64 * factor as i64) >> 16) as i32
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mawlek {
    phase: Phase,
    timer: u16,
    /// The wait the current `Mawlek Control` state holds for: `Super Wait` in
    /// `Idle`, `Cooldown Time` in `Super Cooldown`.
    wait: u16,
    rng: u32,
    jumps: u8,
    spits: u8,
    repeated: bool,
    head_collider: bool,
    dummy_scale: i8,
    /// `X Distance`, Q16 units a second, which `In Air` holds every frame.
    x_distance: i32,
    /// `Start X`, read by `Init`, which `Aim Return` jumps back towards.
    start_x: i32,
    walk: Walk,
    facing: i8,
    walk_timer: u16,
    turn_cooldown: u8,
    arms: [(Arm, u8); 2],
    head: Head,
    head_timer: u8,
}
impl Mawlek {
    /// `Pause` -> `Init` -> `Dormant`: gravity 0, the Walker stopped, the body
    /// blank and the Dummy lurking. `start_x` is the placement's own x and
    /// `facing` the Walker's, from the placement's scale against rightScale.
    pub fn new(seed: u32, start_x: i32, facing: i8) -> Self {
        Self {
            phase: Phase::Dormant,
            timer: 0,
            wait: 0,
            rng: seed | 1,
            jumps: 0,
            spits: 0,
            repeated: false,
            head_collider: false,
            dummy_scale: 1,
            x_distance: 0,
            start_x,
            walk: Walk::Stopped,
            facing,
            walk_timer: 0,
            turn_cooldown: 0,
            arms: [(Arm::Dormant, 0); 2],
            head: Head::Dormant,
            head_timer: 0,
        }
    }
    /// A defeated arena's `Activate` destroys the body before it ever draws.
    pub fn gone() -> Self {
        Self { phase: Phase::Gone, ..Self::new(1, 0, -1) }
    }
    /// The clips `Init` leaves playing, for the caller to seat once.
    pub const INITIAL_CLIPS: [(Part, Clip); 5] = [(Part::Body, Clip::DummyBlank), (Part::Dummy, Clip::DummyLurk),
        (Part::Arm(0), Clip::DummyBlank), (Part::Arm(1), Clip::DummyBlank), (Part::Head, Clip::DummyBlank)];
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn dummy_scale(&self) -> i8 {
        self.dummy_scale
    }
    pub fn head_collider(&self) -> bool {
        self.head_collider
    }
    pub fn walker_facing(&self) -> i8 {
        self.facing
    }
    /// Whether the Walker reads its two Sweeps this tick.
    pub fn walking(&self) -> bool {
        self.phase == Phase::Idle && self.walk == Walk::Walking && self.turn_cooldown == 0
    }
    /// Whether any arm reads its `Attack Range` this tick.
    pub fn arm_watching(&self, arm: usize) -> bool {
        self.arms[arm].0 == Arm::Idle
    }
    /// The body's z, Q16: the lurk depth until `Wake In Air` tweens it to 0.
    pub fn depth(&self) -> i32 {
        match self.phase {
            Phase::Dormant | Phase::Wake | Phase::WakeJump => LURK_DEPTH,
            Phase::WakeAir => {
                let left = WAKE_DEPTH_TICKS.saturating_sub(self.timer) as i32;
                LURK_DEPTH * left / WAKE_DEPTH_TICKS as i32
            }
            _ => 0,
        }
    }
    pub fn dead(&self) -> bool {
        matches!(self.phase, Phase::CorpseInit | Phase::CorpseSteam | Phase::CorpseReady | Phase::Gone)
    }
    /// Out of `Dormant` and not yet dead: the fight is running.
    pub fn active(&self) -> bool {
        self.phase != Phase::Dormant && !self.dead()
    }
    /// The `Active` bools `Super Ready` waits on with BoolNoneTrue.
    fn children_busy(&self) -> bool {
        matches!(self.head, Head::Antic | Head::Shoot)
            || self.arms.iter().any(|(p, _)| matches!(p, Arm::Antic | Arm::Swipe | Arm::Cooldown))
    }
    pub fn random(&mut self) -> u32 {
        self.rng = self.rng.wrapping_mul(1664525).wrapping_add(1013904223);
        self.rng >> 8
    }
    /// Random.Range over a float interval, quantized to whole ticks.
    fn range(&mut self, [lo, hi]: [u16; 2]) -> u16 {
        lo + (self.random() % (hi - lo + 1) as u32) as u16
    }
    fn enter(&mut self, phase: Phase) {
        self.phase = phase;
        self.timer = 0;
    }

    /// One 60 Hz step. Returns what the body, its children and the world do.
    pub fn tick(&mut self, senses: Senses) -> Actions {
        let mut out = Actions::new();
        self.timer = self.timer.saturating_add(1);
        match self.phase {
            Phase::Dormant => {
                if senses.hero_in_wake {
                    self.enter(Phase::Wake);
                    out.push(Action::StartBattle);
                    out.push(Action::Play(Part::Dummy, Clip::DummyIntroJump));
                }
            }
            Phase::Wake => {
                if self.timer >= WAKE_TICKS {
                    self.enter(Phase::WakeJump);
                    out.push(Action::Velocity([0, WAKE_JUMP_SPEED]));
                    out.push(Action::Effect(Effect::WakeJump));
                }
            }
            Phase::WakeJump => {
                if self.timer >= WAKE_JUMP_TICKS {
                    self.enter(Phase::WakeAir);
                    out.push(Action::Gravity(GRAVITY));
                }
            }
            Phase::WakeAir => {
                if senses.grounded {
                    self.enter(Phase::WakeLand);
                    out.push(Action::Play(Part::Dummy, Clip::DummyIntroLand));
                    out.push(Action::VelocityX(0));
                    out.push(Action::Effect(Effect::WakeLand));
                }
            }
            Phase::WakeLand => {
                if self.timer >= Clip::DummyIntroLand.ticks() {
                    // `Title` finishes on the frame it raises the card.
                    out.push(Action::Title);
                    self.enter(Phase::WakeRoar);
                    out.push(Action::Play(Part::Dummy, Clip::DummyRoar));
                    out.push(Action::RoarLock(true));
                    out.push(Action::Effect(Effect::Roar));
                }
            }
            Phase::WakeRoar => {
                if self.timer >= WAKE_ROAR_TICKS {
                    self.enter(Phase::RoarEnd);
                    out.push(Action::RoarLock(false));
                    out.push(Action::Play(Part::Dummy, Clip::RoarCooldown));
                }
            }
            Phase::RoarEnd => {
                if self.timer >= Clip::RoarCooldown.ticks() {
                    out.push(Action::Music);
                    self.start(&mut out);
                }
            }
            Phase::Idle => {
                if self.timer >= self.wait {
                    self.super_ready(&mut out);
                } else {
                    self.walker(senses, &mut out);
                }
            }
            Phase::SuperReady => {
                if !self.children_busy() {
                    self.super_select(senses, &mut out);
                }
            }
            Phase::SuperSpit => {
                if self.timer >= Clip::DummyShootAntic.ticks() {
                    // `Detect Hero Pos`, `L`/`R`, then `Shoot`.
                    self.enter(Phase::Shoot);
                    let angles = if senses.self_x >= senses.hero_x { SPIT_ANGLES_LEFT } else { SPIT_ANGLES_RIGHT };
                    out.push(Action::SpitEffect);
                    out.push(Action::Spray { count: SPIT_SHOTS, speed: SPIT_SPEED, angles });
                    out.push(Action::Play(Part::Dummy, Clip::DummyShoot));
                }
            }
            Phase::Shoot => {
                if self.timer >= Clip::DummyShoot.ticks() {
                    self.super_cooldown(SPIT_COOLDOWN_TICKS, &mut out);
                }
            }
            Phase::JumpAntic => {
                if self.timer >= Clip::DummyJumpAntic.ticks() {
                    // `Detect Hero Pos 3`, `L 3`/`R 3`, then `Jump`.
                    self.x_distance = scale(senses.hero_x - senses.self_x, JUMP_X_FACTOR);
                    self.face_dummy(if senses.self_x >= senses.hero_x { -1 } else { 1 }, &mut out);
                    self.launch(Phase::Jump, &mut out);
                }
            }
            Phase::Jump | Phase::ReturnJump => {
                if self.timer >= JUMP_TICKS {
                    let air = if self.phase == Phase::Jump { Phase::JumpAir } else { Phase::ReturnAir };
                    self.enter(air);
                    out.push(Action::VelocityX(self.x_distance));
                }
            }
            Phase::JumpAir | Phase::ReturnAir => {
                if senses.grounded {
                    let (next, clip) = if self.phase == Phase::JumpAir {
                        (Phase::Land, Clip::DummyLand)
                    } else {
                        (Phase::ReturnLand, Clip::DummyIntroLand)
                    };
                    self.enter(next);
                    out.push(Action::Velocity([0, 0]));
                    out.push(Action::Play(Part::Dummy, clip));
                    out.push(Action::Effect(Effect::Land));
                } else {
                    // SetVelocity2d x every frame, y left to gravity.
                    out.push(Action::VelocityX(self.x_distance));
                }
            }
            Phase::Land => {
                if self.timer >= LAND_TICKS {
                    // `Aim Return`: equal goes LEFT, less and greater both go
                    // RIGHT, so every return but an exact one takes `R 4`.
                    self.x_distance = scale(self.start_x - senses.self_x, JUMP_X_FACTOR);
                    self.face_dummy(if self.x_distance == 0 { 1 } else { -1 }, &mut out);
                    self.launch(Phase::ReturnJump, &mut out);
                }
            }
            Phase::ReturnLand => {
                if self.timer >= LAND_2_TICKS {
                    self.super_cooldown(JUMP_COOLDOWN_TICKS, &mut out);
                }
            }
            Phase::SuperCooldown => {
                if self.timer >= self.wait {
                    // `Repeat Check`: a repeated attack always returns to
                    // `Start`; otherwise a quarter of them go again.
                    if !self.repeated && self.random() % 10000 > REPEAT_ABOVE * 100 {
                        self.repeated = true;
                        self.super_ready(&mut out);
                    } else {
                        self.start(&mut out);
                    }
                }
            }
            Phase::CorpseInit => {
                if self.timer >= CORPSE_TICKS[0] {
                    self.enter(Phase::CorpseSteam);
                    out.push(Action::Effect(Effect::CorpseSteam));
                }
            }
            Phase::CorpseSteam => {
                if self.timer >= CORPSE_TICKS[1] {
                    self.enter(Phase::CorpseReady);
                }
            }
            Phase::CorpseReady => {
                if self.timer >= CORPSE_TICKS[2] {
                    // `Sting` finishes at once; `Blow` has nothing to wait on.
                    self.enter(Phase::Gone);
                    out.push(Action::Effect(Effect::Sting));
                    out.push(Action::Blown);
                    out.push(Action::Effect(Effect::Blow));
                }
            }
            Phase::Gone => {}
        }
        if !self.dead() {
            self.children(senses, &mut out);
        }
        out
    }
    /// The HealthManager death. The body and its children go with the object
    /// and the corpse takes its place; the corpse's `Music` state sends BATTLE
    /// END and fades the mixer on its first frame, then `Init` runs.
    pub fn die(&mut self) -> Actions {
        let mut out = Actions::new();
        if self.dead() {
            return out;
        }
        self.enter(Phase::CorpseInit);
        self.walk = Walk::Stopped;
        self.head = Head::Dormant;
        self.arms = [(Arm::Dormant, 0); 2];
        self.head_collider = false;
        out.push(Action::Died);
        out.push(Action::Effect(Effect::CorpseInit));
        out
    }
    /// `Start`: the Dummy blank, the head target and the body back, WAKE to
    /// the children and the Walker walking again.
    fn start(&mut self, out: &mut Actions) {
        self.enter(Phase::Idle);
        self.wait = self.range(IDLE_TICKS);
        self.repeated = false;
        self.head_collider = true;
        out.push(Action::Play(Part::Dummy, Clip::DummyBlank));
        out.push(Action::HeadCollider(true));
        out.push(Action::Vulnerable);
        out.push(Action::Play(Part::Body, Clip::BodyIdle));
        out.push(Action::Mesh(true));
        for arm in 0..2 {
            if self.arms[arm].0 == Arm::Dormant {
                self.arm_idle(arm, out);
            }
        }
        if self.head == Head::Dormant {
            self.head_idle(out);
        }
        // StartWalker with no direction: StartMoving begins walking the way
        // it already faces, then the turn cooldown is cleared.
        if self.walk == Walk::Stopped {
            self.begin_walking(out);
        }
        self.turn_cooldown = 0;
    }
    /// `Super Ready`: StopWalker, x velocity 0, `Body Idle`.
    fn super_ready(&mut self, out: &mut Actions) {
        self.enter(Phase::SuperReady);
        self.walk = Walk::Stopped;
        out.push(Action::VelocityX(0));
        out.push(Action::Play(Part::Body, Clip::BodyIdle));
    }
    /// `Super Select`, which also takes the head target away, then one of the
    /// two attacks with the in-a-row counters that force the other.
    fn super_select(&mut self, senses: Senses, out: &mut Actions) {
        self.head_collider = false;
        out.push(Action::HeadCollider(false));
        let spit = self.random() & 1 == 0;
        if spit && self.spits <= IN_A_ROW || !spit && self.jumps > IN_A_ROW {
            // `Detect Hero Pos 2`, `L 2`/`R 2`, `Super Spit`.
            self.spits += 1;
            self.jumps = 0;
            self.face_dummy(if senses.self_x >= senses.hero_x { 1 } else { -1 }, out);
            self.enter(Phase::SuperSpit);
            self.sleep_children(out);
            out.push(Action::Play(Part::Dummy, Clip::DummyShootAntic));
            out.push(Action::Effect(Effect::BigSpit));
        } else {
            // `Super Jump`.
            self.jumps += 1;
            self.spits = 0;
            self.enter(Phase::JumpAntic);
            self.sleep_children(out);
            out.push(Action::Play(Part::Dummy, Clip::DummyJumpAntic));
        }
    }
    /// SLEEP to the children, `Dummy Blank` on the body and its mesh off.
    fn sleep_children(&mut self, out: &mut Actions) {
        for arm in 0..2u8 {
            if self.arms[arm as usize].0 == Arm::Swipe {
                out.push(Action::ArmHitbox(arm, false));
            }
            self.arms[arm as usize] = (Arm::Dormant, 0);
            out.push(Action::Play(Part::Arm(arm), Clip::DummyBlank));
        }
        self.head = Head::Dormant;
        out.push(Action::Play(Part::Head, Clip::DummyBlank));
        out.push(Action::Play(Part::Body, Clip::DummyBlank));
        out.push(Action::Mesh(false));
    }
    fn face_dummy(&mut self, scale: i8, out: &mut Actions) {
        self.dummy_scale = scale;
        out.push(Action::DummyScale(scale));
    }
    /// `Jump` and `Jump 2`: `Dummy Jump`, (X Distance, 68), EnemyKillShake.
    fn launch(&mut self, phase: Phase, out: &mut Actions) {
        self.enter(phase);
        out.push(Action::Play(Part::Dummy, Clip::DummyJump));
        out.push(Action::Velocity([self.x_distance, JUMP_SPEED_Y]));
        out.push(Action::Effect(Effect::Jump));
    }
    /// `Super Cooldown`: the body and the children's idle art back, the Dummy
    /// blank, and a wait of `Cooldown Time`.
    fn super_cooldown(&mut self, ticks: u16, out: &mut Actions) {
        self.enter(Phase::SuperCooldown);
        self.wait = ticks;
        out.push(Action::Play(Part::Body, Clip::BodyIdle));
        out.push(Action::Mesh(true));
        out.push(Action::Play(Part::Dummy, Clip::DummyBlank));
        out.push(Action::Play(Part::Arm(0), Clip::ArmIdle));
        out.push(Action::Play(Part::Arm(1), Clip::ArmIdle));
        out.push(Action::Play(Part::Head, Clip::HeadIdle));
    }
    fn begin_walking(&mut self, out: &mut Actions) {
        self.walk = Walk::Walking;
        self.walk_timer = self.range(WALK_TICKS);
        out.push(Action::Play(Part::Body, Clip::BodyWalk));
        out.push(Action::VelocityX(self.facing as i32 * WALK_SPEED));
    }
    /// The Walker's Update while `Idle` runs. `preventTurningToFaceHero` is
    /// set, so a wall or a hole ahead are the only reasons it turns, and
    /// `turnAfterIdlePercentage` is 0, so a pause resumes the same way.
    fn walker(&mut self, senses: Senses, out: &mut Actions) {
        self.turn_cooldown = self.turn_cooldown.saturating_sub(1);
        match self.walk {
            Walk::Stopped => {}
            Walk::Walking => {
                if self.turn_cooldown == 0 && (senses.wall_ahead || !senses.floor_ahead) {
                    self.walk = Walk::Turning;
                    self.walk_timer = Clip::IdleTurn.ticks();
                    self.turn_cooldown = TURN_COOLDOWN_TICKS as u8;
                    out.push(Action::VelocityX(0));
                    out.push(Action::Play(Part::Body, Clip::IdleTurn));
                    return;
                }
                self.walk_timer = self.walk_timer.saturating_sub(1);
                if self.walk_timer == 0 {
                    self.walk = Walk::Paused;
                    self.walk_timer = self.range(PAUSE_TICKS);
                    out.push(Action::VelocityX(0));
                    out.push(Action::Play(Part::Body, Clip::BodyIdle));
                }
            }
            Walk::Paused => {
                self.walk_timer = self.walk_timer.saturating_sub(1);
                if self.walk_timer == 0 {
                    self.begin_walking(out);
                }
            }
            Walk::Turning => {
                self.walk_timer = self.walk_timer.saturating_sub(1);
                if self.walk_timer == 0 {
                    self.facing = -self.facing;
                    self.begin_walking(out);
                }
            }
        }
    }
    fn arm_idle(&mut self, arm: usize, out: &mut Actions) {
        self.arms[arm] = (Arm::Idle, 0);
        out.push(Action::Play(Part::Arm(arm as u8), Clip::ArmIdle));
    }
    fn head_idle(&mut self, out: &mut Actions) {
        self.head = Head::Idle;
        self.head_timer = self.range(HEAD_IDLE_TICKS) as u8;
        out.push(Action::Play(Part::Head, Clip::HeadIdle));
    }
    /// `Mawlek Arm Control` twice and `Mawlek Head`, each on its own clock.
    fn children(&mut self, senses: Senses, out: &mut Actions) {
        for arm in 0..2usize {
            let part = Part::Arm(arm as u8);
            let (phase, timer) = &mut self.arms[arm];
            *timer = timer.saturating_add(1);
            match *phase {
                Arm::Dormant => {}
                // CheckAlertRangeByName every frame, the first on entry.
                Arm::Idle => {
                    if senses.hero_in_arm[arm] {
                        *phase = Arm::Antic;
                        *timer = 0;
                        out.push(Action::Play(part, Clip::ArmSwipeAntic));
                        out.push(Action::Effect(Effect::ArmCall));
                    }
                }
                Arm::Antic => {
                    if *timer as u16 >= Clip::ArmSwipeAntic.ticks() {
                        *phase = Arm::Swipe;
                        *timer = 0;
                        out.push(Action::Play(part, Clip::ArmSwipe));
                        out.push(Action::ArmHitbox(arm as u8, true));
                        out.push(Action::Effect(Effect::ArmWhip));
                    }
                }
                Arm::Swipe => {
                    if *timer as u16 >= Clip::ArmSwipe.ticks() {
                        *phase = Arm::Cooldown;
                        *timer = 0;
                        out.push(Action::Play(part, Clip::ArmSwipeCooldown));
                        out.push(Action::ArmHitbox(arm as u8, false));
                    }
                }
                Arm::Cooldown => {
                    if *timer as u16 >= Clip::ArmSwipeCooldown.ticks() {
                        *phase = Arm::Pause;
                        *timer = 0;
                    }
                }
                Arm::Pause => {
                    if *timer as u16 >= ARM_PAUSE_TICKS {
                        self.arm_idle(arm, out);
                        if senses.hero_in_arm[arm] {
                            self.arms[arm] = (Arm::Antic, 0);
                            out.push(Action::Play(part, Clip::ArmSwipeAntic));
                            out.push(Action::Effect(Effect::ArmCall));
                        }
                    }
                }
            }
        }
        self.head_timer = self.head_timer.saturating_sub(1);
        match self.head {
            Head::Dormant => {}
            Head::Idle => {
                if self.head_timer == 0 {
                    self.head = Head::Antic;
                    self.head_timer = HEAD_ANTIC_TICKS as u8;
                    out.push(Action::Play(Part::Head, Clip::HeadSpit));
                }
            }
            Head::Antic => {
                if self.head_timer == 0 {
                    // `Detect Hero Pos`, `L`/`R`, then `Shoot`, which ends at
                    // whichever comes first of its wait and `Head Spit`'s end.
                    self.head = Head::Shoot;
                    self.head_timer = HEAD_SHOOT_TICKS.min(Clip::HeadSpit.ticks() - HEAD_ANTIC_TICKS) as u8;
                    let angles = if senses.head_x >= senses.hero_x { HEAD_ANGLES_LEFT } else { HEAD_ANGLES_RIGHT };
                    out.push(Action::SpitEffect);
                    out.push(Action::HeadShot { speed: HEAD_SHOT_SPEED, angles });
                    out.push(Action::Effect(Effect::HeadSpit));
                }
            }
            Head::Shoot => {
                if self.head_timer == 0 {
                    self.head_idle(out);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn senses() -> Senses {
        Senses { self_x: 61 * ONE, hero_x: 55 * ONE, head_x: 61 * ONE, grounded: true, floor_ahead: true,
            ..Senses::default() }
    }
    /// Run until `phase`, feeding `senses`, and return the ticks it took.
    fn until(m: &mut Mawlek, s: Senses, phase: Phase, limit: u32) -> u32 {
        for t in 1..=limit {
            m.tick(s);
            if m.phase() == phase {
                return t;
            }
        }
        panic!("{phase:?} not reached in {limit} ticks, at {:?}", m.phase());
    }
    fn woken() -> Mawlek {
        let mut m = Mawlek::new(7, 61 * ONE, -1);
        let mut s = senses();
        s.hero_in_wake = true;
        let first = m.tick(s);
        assert!(first.contains(Action::StartBattle));
        s.hero_in_wake = false;
        s.grounded = false;
        until(&mut m, s, Phase::WakeAir, 40);
        s.grounded = true;
        until(&mut m, s, Phase::Idle, 400);
        m
    }

    #[test]
    fn it_lurks_until_the_wake_box_and_then_runs_the_intro_in_order() {
        let mut m = Mawlek::new(3, 61 * ONE, -1);
        let mut s = senses();
        for _ in 0..600 {
            assert!(m.tick(s).is_empty(), "a lurking Mawlek does nothing");
        }
        assert_eq!(m.depth(), LURK_DEPTH);
        s.hero_in_wake = true;
        m.tick(s);
        s.hero_in_wake = false;
        assert_eq!(until(&mut m, s, Phase::WakeJump, 20), WAKE_TICKS as u32);
        s.grounded = false;
        assert_eq!(until(&mut m, s, Phase::WakeAir, 20), WAKE_JUMP_TICKS as u32);
        for _ in 0..WAKE_DEPTH_TICKS {
            m.tick(s);
        }
        assert_eq!(m.depth(), 0, "the iTween reaches the play plane in 0.5 s");
        s.grounded = true;
        m.tick(s);
        assert_eq!(m.phase(), Phase::WakeLand);
        let mut title = false;
        let mut music = false;
        let mut vulnerable = false;
        for _ in 0..(15 + 120 + 25 + 2) {
            for a in m.tick(s).iter() {
                title |= a == Action::Title;
                music |= a == Action::Music;
                if a == Action::Vulnerable {
                    assert!(title && music, "Start follows Title and Music");
                    vulnerable = true;
                }
            }
        }
        assert!(title && music && vulnerable);
        assert_eq!(m.phase(), Phase::Idle);
        assert!(m.head_collider());
    }

    #[test]
    fn idle_walks_then_chooses_a_super_and_the_counters_force_a_switch() {
        let mut spits_in_row = 0;
        let mut jumps_in_row = 0;
        for seed in 0..40 {
            let mut m = woken();
            m.rng = seed * 7919 + 1;
            let mut s = senses();
            let mut last = None;
            for _ in 0..8 {
                until(&mut m, s, Phase::SuperReady, 400);
                // It waits there while the Head finishes a spit.
                while m.phase() == Phase::SuperReady {
                    m.tick(s);
                }
                let kind = m.phase();
                assert!(matches!(kind, Phase::SuperSpit | Phase::JumpAntic), "{kind:?}");
                if Some(kind) == last {
                    if kind == Phase::SuperSpit { spits_in_row += 1 } else { jumps_in_row += 1 }
                } else {
                    spits_in_row = 1;
                    jumps_in_row = 1;
                }
                assert!(spits_in_row <= IN_A_ROW as u32 + 1 && jumps_in_row <= IN_A_ROW as u32 + 1);
                last = Some(kind);
                // Finish the attack: jumps need to leave and reach the ground.
                s.grounded = false;
                for _ in 0..30 {
                    m.tick(s);
                }
                s.grounded = true;
            }
        }
    }

    #[test]
    fn the_spit_waits_for_its_antic_and_fires_twenty_five_towards_the_hero() {
        let mut m = woken();
        let s = senses();
        m.spits = 0;
        m.jumps = IN_A_ROW + 1; // force the spit
        until(&mut m, s, Phase::SuperReady, 400);
        while m.phase() == Phase::SuperReady {
            m.tick(s);
        }
        assert_eq!(m.phase(), Phase::SuperSpit);
        let mut fired = None;
        for t in 1..=60 {
            for a in m.tick(s).iter() {
                if let Action::Spray { count, angles, .. } = a {
                    fired = Some((t, count, angles));
                }
            }
            if fired.is_some() {
                break;
            }
        }
        let (t, count, angles) = fired.expect("the spit fires");
        assert_eq!(t, Clip::DummyShootAntic.ticks() as u32);
        assert_eq!(count, 25);
        assert_eq!(angles, SPIT_ANGLES_LEFT, "the hero is to the left");
        for _ in 0..(18 + 105) {
            m.tick(s);
        }
        assert!(matches!(m.phase(), Phase::Idle | Phase::SuperReady));
    }

    #[test]
    fn the_leap_aims_at_the_hero_and_the_return_aims_at_start_x() {
        let mut m = woken();
        let mut s = senses();
        m.jumps = 0;
        m.spits = IN_A_ROW + 1; // force the jump
        until(&mut m, s, Phase::SuperReady, 400);
        while m.phase() == Phase::SuperReady {
            m.tick(s);
        }
        assert_eq!(m.phase(), Phase::JumpAntic);
        let launch = (0..30).find_map(|_| m.tick(s).iter().find_map(|a| match a {
            Action::Velocity(v) if v[1] == JUMP_SPEED_Y => Some(v),
            _ => None,
        })).expect("it leaps");
        assert_eq!(launch[0], scale(s.hero_x - s.self_x, JUMP_X_FACTOR));
        assert_eq!(m.dummy_scale(), -1, "L 3 mirrors the Dummy when the hero is left");
        s.grounded = false;
        until(&mut m, s, Phase::JumpAir, 10);
        s.grounded = true;
        s.self_x = 54 * ONE;
        until(&mut m, s, Phase::Land, 2);
        let back = (0..40).find_map(|_| m.tick(s).iter().find_map(|a| match a {
            Action::Velocity(v) if v[1] == JUMP_SPEED_Y => Some(v),
            _ => None,
        })).expect("it leaps back");
        assert_eq!(back[0], scale(7 * ONE, JUMP_X_FACTOR));
        assert_eq!(m.dummy_scale(), -1, "Aim Return takes R 4 for any nonzero distance");
    }

    #[test]
    fn super_ready_waits_for_a_swiping_arm() {
        let mut m = woken();
        let mut s = senses();
        s.hero_in_arm = [true, false];
        let mut swiped = false;
        for _ in 0..30 {
            swiped |= m.tick(s).contains(Action::ArmHitbox(0, true));
        }
        assert!(!swiped, "the antic comes first");
        assert!(m.children_busy());
        let hit_on = (0..40).any(|_| m.tick(s).contains(Action::ArmHitbox(0, true)));
        assert!(hit_on);
        s.hero_in_arm = [false, false];
        m.phase = Phase::SuperReady;
        for _ in 0..(4 + 12) {
            m.tick(s);
            if !m.children_busy() {
                break;
            }
            assert_eq!(m.phase(), Phase::SuperReady);
        }
    }

    #[test]
    fn the_head_spits_every_half_second_or_so_while_awake() {
        let mut m = woken();
        let s = senses();
        let mut shots = 0;
        for _ in 0..(10 * 60) {
            if let Some(Action::HeadShot { angles, speed }) = m.tick(s).iter().find(|a| matches!(a, Action::HeadShot { .. })) {
                assert_eq!(angles, HEAD_ANGLES_LEFT);
                assert_eq!(speed, HEAD_SHOT_SPEED);
                shots += 1;
            }
        }
        // Idle 0.3..0.6 s plus 0.083 s antic plus ~0.17 s shoot, while awake.
        assert!(shots > 5, "{shots}");
    }

    #[test]
    fn death_runs_the_corpse_and_ends_in_the_blow() {
        let mut m = woken();
        let s = senses();
        let died = m.die();
        assert!(died.contains(Action::Died));
        assert!(m.die().is_empty(), "one death");
        let mut blown = None;
        for t in 1..=400 {
            if m.tick(s).contains(Action::Blown) {
                blown = Some(t);
                break;
            }
        }
        assert_eq!(blown, Some(90 + 180 + 60));
        // The guest shows the Heart Piece on the Blow, so the corpse's three
        // waits must add up to `Blow Wait`'s 5.5 s.
        assert_eq!(CORPSE_TICKS.iter().sum::<u16>(), HEART_PIECE_TICKS);
        assert_eq!(m.phase(), Phase::Gone);
    }

    #[test]
    fn fling_matches_the_whole_degree_rows_and_stays_in_range() {
        assert_eq!(fling(ONE, 90 * 4), [0, ONE]);
        let v = fling(32 * ONE, 75 * 4);
        assert!(v[0] > 0 && v[1] > 30 * ONE);
        let mut rng = 9;
        for _ in 0..500 {
            let v = shot_velocity(&mut rng, SPIT_SPEED, SPIT_ANGLES_LEFT);
            assert!(v[0] < 0, "92..105 degrees always goes left");
            let speed2 = (v[0] as i64).pow(2) + (v[1] as i64).pow(2);
            assert!(speed2 <= (35i64 * ONE as i64 + 64).pow(2) && speed2 >= (32i64 * ONE as i64 - 64).pow(2));
        }
    }
}
