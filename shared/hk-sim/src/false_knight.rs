//! Source-derived False Knight (`FalseyControl`, Crossroads_10_boss).
//!
//! Every constant here was read back out of the serialized FSM by
//! host/false_knight.py, which asserts the same values against the installed
//! source and fails the cook if they move. tests/test_false_knight.py compares
//! the two sides so they cannot drift.
//!
//! The caller owns the dynamic body, the terrain queries, the hero position,
//! the Head's HealthManager, the barrel pool and every effect. This type owns
//! the fight: which attack comes next, how long each one takes, when the armour
//! opens, when a phase advances and when the arena may end.
//!
//! Damage model, from `Check Health` and the Head's `Health Check`: the body
//! carries 65 hp and restores to 65 every time it reaches zero, which is what
//! staggers it; the exposed Head carries 40 and restores to 40 when it reaches
//! zero, which ends the stagger and advances a phase. Three of those kill it.
//! Neither HealthManager drops Geo; the reward is the PlayerData set.
use crate::ONE;

pub const HEALTH: i16 = 65;
pub const HEAD_HEALTH: i16 = 40;
/// HealthManager.NonFatalHit's evasionByHitRemaining, the IL literal 0.2 s.
/// The serialized invulnerableTime (0.25 s and 0.15 s) is read by nothing.
pub const INVULNERABLE_TICKS: u16 = 12;
pub const HEAD_INVULNERABLE_TICKS: u16 = 12;
pub const CONTACT_DAMAGE: u16 = 1;
pub const STAGGERS: u8 = 3;
/// `R Attack Antic` sets Rages to 8; `Turn` spends one per slam.
pub const RAGE_SLAMS: u8 = 8;
/// `Opened`: the armour shuts again five seconds after it opens, and the timer
/// restarts on every hit because `Hit` re-enters `Opened`.
pub const STUN_WINDOW_TICKS: u16 = 300;

pub const TURN_TICKS: u16 = 10;
pub const JUMP_ANTIC_TICKS: u16 = 18;
pub const LAND_TICKS: u16 = 30;
pub const JA_RECOIL_TICKS: u16 = 10;
pub const JA_RECOIL2_TICKS: u16 = 6;
pub const JA_END_TICKS: u16 = 10;
pub const JA_SLAM_TICKS: u16 = 5;
pub const SLAM_ANTIC_TICKS: u16 = 72;
pub const SLAM_STRIKE_TICKS: u16 = 8;
pub const SLAM_RECOVER_TICKS: u16 = 25;
pub const RUN_ANTIC_TICKS: u16 = 10;
pub const ROLL_STOP_TICKS: u16 = 30;
pub const ROLL_END_TICKS: u16 = 20;
pub const PLOP_SHORT_TICKS: u16 = 72;
pub const PLOP_LONG_TICKS: u16 = 150;
pub const OPEN_TICKS: u16 = 20;
pub const STUN_HIT_TICKS: u16 = 15;
pub const RECOVER_TICKS: u16 = 30;
pub const IDLE_PAUSE_TICKS: u16 = 30;
pub const RAGE_ANTIC_TICKS: u16 = 42;
pub const RAGE_SLAM_TICKS: u16 = 15;
pub const RAGE_TURN_TICKS: u16 = 25;
pub const RAGE_PARTICLE_TICKS: u16 = 6;
pub const RAGE_END_TICKS: u16 = 45;
pub const DEATH_LAND_TICKS: u16 = 120;
pub const FIRST_IDLE_TICKS: u16 = 90;
/// `Death Anim Start` 1 s, `Steam` 3 s, `Ready` 1 s, the Death Head 1 clip
/// (10 frames at 10 fps) and `Death Head Land` 1.5 s, in that order.
pub const DEATH_TAIL_TICKS: [u16; 5] = [60, 180, 60, 60, 90];

pub const RUN_SPEED: i32 = 14 * ONE;
pub const RUN_TRIGGER_DISTANCE: i32 = 21 * ONE;
pub const RUN_STOP_DISTANCE: i32 = 14 * ONE;
pub const STUN_ROLL_SPEED: i32 = 10 * ONE;
/// `Stun Land` ends early once X Speed drops below minus this tolerance.
pub const STUN_ROLL_STOP_SPEED: i32 = 3 * ONE;
pub const JA_RECOIL_SPEED: i32 = 3 * ONE;
pub const JA_OFFSET: i32 = 3 * ONE;
pub const TOWARDS_CLAMP: i32 = 12 * ONE;
pub const RANDOM_JUMP_MIN: i32 = 5 * ONE;
pub const RANDOM_JUMP_MAX: i32 = 10 * ONE;
pub const WALL_RAY_DISTANCE: i32 = 8 * ONE;
pub const FALL_RAY_DISTANCE: i32 = 622592; // 9.5
pub const SLAM_SKIP_JUMP_DISTANCE: i32 = 12 * ONE;
pub const SLAM_OVERSHOOT: [i32; 2] = [12 * ONE, 18 * ONE];
pub const HERO_X_CLAMP: [i32; 2] = [15 * ONE, 42 * ONE];
pub const RAGE_POINT_X: i32 = 1893990; // 28.9
pub const FINAL_POINT_X: i32 = 34 * ONE;
pub const SHOCKWAVE_SPEED: i32 = 22 * ONE;
pub const SHOCKWAVE_X_ORIGIN: i32 = 360448; // 5.5
/// Launch speeds, from each state's SetVelocity2d.
pub const JUMP_SPEED_Y: i32 = 90 * ONE;
pub const SLAM_JUMP_SPEED_Y: i32 = 105 * ONE;
pub const STUN_JUMP_SPEED_Y: i32 = 20 * ONE;
pub const DEATH_FALL_SPEED_Y: i32 = 15 * ONE;
/// SetGravity2dScale, Q16. The serialized Rigidbody2D scale is zero; the FSM
/// writes one of these every time it leaves the ground.
pub const GRAVITY_IDLE: i32 = 25559; // .39
pub const GRAVITY_JUMP: i32 = 8192; // .125
pub const GRAVITY_JUMP_ATTACK: i32 = 7864; // .12
pub const GRAVITY_RAGE_JUMP: i32 = 19661; // .30
pub const GRAVITY_DEATH_JUMP: i32 = 13107; // .20
pub const GRAVITY_STUN: i32 = ONE;
/// `Rise` and `Fall` reshape vertical speed every step instead of integrating
/// gravity alone: 0.85 while rising, 1.15 while falling.
pub const RISE_MULTIPLIER: i32 = 55706;
pub const FALL_MULTIPLIER: i32 = 75366;
/// `Towards`: 0.9 of the gap, and `JA Antic`: 0.58 of it.
pub const TOWARDS_FACTOR: i32 = 58982;
pub const JA_ANTIC_FACTOR: i32 = 38011;
pub const RAGE_JUMP_FACTOR: i32 = 68813; // 1.05
pub const FINAL_JUMP_FACTOR: i32 = 49807; // 0.76

/// `Turn R`/`Turn L` count up to three free turns before forcing an attack.
pub const TURNS_BEFORE_ATTACK: i32 = 3;
/// The two child hitboxes, in the source's facing-right frame (+x ahead) and
/// already multiplied by the body's 1.3 transform scale, as `[x0,y0,x1,y1]`
/// offsets from the transform. A caller facing left mirrors x.
///
/// `Head` is the 40 hp target the armour exposes, and it sits *above* the body
/// box rather than inside it, which is why a caller cannot reuse `ActorSpec`'s
/// bounds for it. The source parks it at y 150 while the armour is shut and
/// drops it back on `Opened`, which `Action::HeadExposed` reports.
pub const HEAD_BOX: [i32; 4] = [-68157, -91587, 68157, 54100];
/// `Hitter` is the DamageHero trigger the attacks switch on; it reaches nearly
/// eight units ahead of the boss, which is what makes the slam and the jump
/// attack cover ground the body never touches.
pub const HITTER_BOX: [i32; 4] = [22630, -366080, 527155, -149094];
/// `Row Check` returns to Move Choice once JA In A Row is already above three.
pub const JUMP_ATTACKS_IN_A_ROW: i32 = 3;
/// `S Check Hero Pos` returns once Slam In A Row is already above two.
pub const SLAMS_IN_A_ROW: i32 = 2;

/// `To Phase 2` and `To Phase 3` rewrite these; index 0 is the FSM's own start.
/// Idle wait bounds in ticks, then the inclusive barrel counts the jump attack
/// and the slam ask the summoner for.
pub const IDLE_TICKS: [[u16; 2]; 3] = [[60, 60], [48, 60], [48, 60]];
pub const JUMP_BARRELS: [[u8; 2]; 3] = [[0, 0], [2, 3], [2, 2]];
pub const SLAM_BARRELS: [[u8; 2]; 3] = [[0, 0], [2, 3], [3, 4]];

/// `FK Barrel Summon`'s `PersonalObjectPool` reserve, which is also the most
/// barrels one SUMMON ever asks for (the rage's eight).
pub const BARREL_POOL: usize = 8;
/// `summon`'s `Spawn`: RandomFloat between `Summon Min` and `Summon Max`, Q16.
/// The y is the summoner's own transform, which the caller carries in its
/// `ActorSpec` because it belongs to a different source object.
pub const BARREL_SPAWN_X: [i32; 2] = [863764, 2901279];
/// The `WaitRandom` that both `Determine Spawns` and `Spawn` hold, so the first
/// barrel of a burst is as late as every later one.
pub const BARREL_GAP_TICKS: [u16; 2] = [9, 15];
/// `Falling Barrel`'s Rigidbody2D gravity scale 0.325 against the installed
/// Physics2DSettings gravity of 60, Q16 units per second squared.
pub const BARREL_GRAVITY: i32 = 1277952;
/// Its BoxCollider2D, 1.18 by 1.11 centred on the transform, as half extents.
pub const BARREL_HALF: [i32; 2] = [38666, 36372];
/// Its `DamageHero`, which is the whole reason a barrel is worth presenting.
pub const BARREL_DAMAGE: u16 = 1;

/// Source `summon` on `FK Barrel Summon`, the object that actually makes the
/// barrels. It is not part of `FalseyControl` and it is not modelled as part of
/// `FalseKnight` here either: the boss writes `Spawns` and sends SUMMON, and
/// this counts the burst out at its own pace.
///
/// `Determine Spawns` waits before the first spawn, `Spawn` waits after every
/// one, and both waits are the same `WaitRandom`, so a burst of n barrels takes
/// n gaps and the first barrel is never on the same frame as the slam. The
/// count itself comes from the caller: the FSM's own `RandomInt 6..8` is
/// disabled in the serialized state machine and `FalseyControl` writes `Spawns`
/// through SetFsmInt instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Summon {
    /// `Spawns`, counted down by `Spawn`'s IntOperator.
    remaining: u8,
    timer: u16,
    rng: u32,
}
impl Summon {
    pub const fn new(seed: u32) -> Self {
        Self { remaining: 0, timer: 0, rng: seed | 1 }
    }
    pub fn remaining(self) -> u8 {
        self.remaining
    }
    fn random(&mut self) -> u32 {
        self.rng = self.rng.wrapping_mul(1664525).wrapping_add(1013904223);
        self.rng >> 8
    }
    /// `SUMMON` into `Idle`. A second SUMMON while the loop is still running is
    /// dropped, because `Determine Spawns`, `Spawn` and `Check Spawns` declare
    /// no transition for it and PlayMaker only answers the current state's.
    pub fn summon(&mut self, spawns: u8) {
        if self.remaining != 0 || spawns == 0 {
            return;
        }
        self.remaining = spawns;
        self.timer = self.gap();
    }
    fn gap(&mut self) -> u16 {
        let [low, high] = BARREL_GAP_TICKS;
        low + (((self.random() * (high - low + 1) as u32) >> 24) as u16)
    }
    /// One 60 Hz frame; `Some(x)` is one `SpawnObjectFromGlobalPool`, in Q16
    /// world x. At most one a frame, because the shortest gap is nine ticks.
    pub fn tick(&mut self) -> Option<i32> {
        if self.remaining == 0 {
            return None;
        }
        self.timer -= 1;
        if self.timer != 0 {
            return None;
        }
        self.remaining -= 1;
        if self.remaining != 0 {
            self.timer = self.gap();
        }
        let [low, high] = BARREL_SPAWN_X;
        Some(low + ((self.random() as i64 * (high - low) as i64) >> 24) as i32)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clip {
    Idle,
    Turn,
    JumpAntic,
    Jump,
    Land,
    Run,
    RunAntic,
    JumpAttackUp,
    JumpAttackHit1,
    JumpAttackHit2,
    JumpAttackHit3,
    AttackAntic,
    Attack,
    AttackRecover,
    Rage,
    StunRoll,
    StunRollEnd,
    StunOpen,
    StunOpened,
    StunHit,
    StunRecover,
    DeathFall,
    DeathLand,
    DeathSpaz,
    Blank,
}
/// Whole-clip durations at 60 Hz, in `Clip` order, ceilinged the way
/// host/combat.py does. The source clip names are, in the same order: Idle,
/// Turn, Jump Antic, Jump, Land, Run, Run Antic, Jump Attack Up, Jump Attack
/// Hit 1, Jump Attack Hit 2, Jump Attack Hit 3, Attack Antic, Attack, Attack
/// Recover, Rage, Stun Roll, Stun Roll End, Stun Open, Stun Opened, Stun Hit,
/// Stun Recover, Death Fall, Death Land, Death Spaz, Blank.
pub const CLIP_TICKS: [u16; 25] = [
    25, 10, 18, 20, 30, 25, 10, 25, 10, 10, 10, 30, 12, 25, 25, 25, 20, 20, 5, 15, 30, 15, 25, 15, 2,
];
impl Clip {
    /// Looping clips report their cycle; nothing waits on their completion.
    pub fn ticks(self) -> u16 {
        CLIP_TICKS[self as usize]
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// `Dormant`: hanging above the arena until BATTLE START.
    Dormant,
    /// `Start Fall` and `Rubble End`: the entrance drop.
    Entrance,
    /// `State 1` after the drop and after every plain landing.
    Landing,
    /// `First Idle`: 1.5 s before the scripted opening jump.
    FirstIdle,
    Idle,
    Turn,
    JumpAntic,
    Airborne,
    JumpAttackAntic,
    JumpAttackAir,
    JumpAttackHit,
    JumpAttackSlam,
    JumpAttackRecoil,
    JumpAttackRecoil2,
    JumpAttackEnd,
    SlamAntic,
    SlamAir,
    SlamLand,
    SlamStrikeAntic,
    SlamStrike,
    SlamRecover,
    RunAntic,
    Run,
    /// `Stun Start` through `Stun Land`: the armour bounces and rolls.
    StunRoll,
    StunRollEnd,
    /// `Pause Short` / `Pause Long`, chosen by `falseKnightFirstPlop`.
    StunPause,
    StunOpening,
    /// `Opened`: the head is exposed and the five-second timer is running.
    Opened,
    StunHit,
    /// `Recover` and `Stun Fail`: the armour closes, with or without a phase.
    Recovering,
    /// `Idle Pause` before the rage jump.
    RagePause,
    RageJumpAntic,
    RageAir,
    RageLand,
    RageAntic,
    RageSlam,
    RageTurn,
    RageEnd,
    /// The death sequence, from `JA Antic 2` to the HealthManager death event.
    DeathJumpAntic,
    DeathAir,
    DeathHit,
    DeathFall,
    DeathLand,
    DeathOpening,
    /// `Opened 2`: the last exposure, with no timeout.
    DeathOpened,
    DeathHit2,
    /// `Death Anim Start` onward, one step of `DEATH_TAIL_TICKS` at a time.
    Dying(u8),
    Dead,
}
/// The `CameraShake` sends and the `AudioPlaySimple` one-shots the controller
/// fires on the same states it changes phase on. They are named after what the
/// source state does rather than after a clip or a shake, because a state that
/// shakes and plays at once is one event to the caller and the caller owns both
/// the camera and the SPU.
///
/// `Slam`'s and `Rage Slam`'s own `false_knight_strike_ground` is not resident
/// (host/cook_audio.py's refusal table carries its measured size), so those two
/// carry their shake alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// `S Land` and `State 2`: AverageShake with the landing voice.
    LandingShake,
    /// `Land Noise`: the landing voice with no shake.
    Landing,
    /// `S Attack` and `JA Hit 2`: the mace swing.
    Swing,
    /// `Slam`, `JA Slam`, `Stun Start` and `Start Fall`: BigShake.
    BigShake,
    /// `Rage Slam`: AverageShake.
    RageSlam,
    /// `Jump`, `S Jump`, `JA Jump` and `Jump 2`: EnemyKillShake and the jump.
    JumpShake,
    /// `JA Jump 2`: the jump with no shake.
    Jump,
    /// `Jump 2`, the rage's own leap: EnemyKillShake, the jump, and
    /// `FKnight_Rage`, the roar every rage opens with.
    RageJump,
    /// `Start Fall`: BigShake and `false_knight_ceiling_break` as the boss
    /// drops out of the ceiling.
    Entrance,
    /// `State 2` then `Rubble End`: AverageShake, the landing, and
    /// `false_knight_land_1st_time`.
    EntranceLanding,
    /// `Slam` and `JA Slam`: BigShake and `false_knight_strike_ground`.
    Slam,
    /// `Stun Start`: BigShake and the armour's `false_knight_damage_armour_final`.
    StunStart,
    /// `Stun Land`: the roll.
    Roll,
    /// `Run`: `SetAudioClip` puts `zombie_guard_footstep` on the boss's own
    /// AudioSource and `AudioPlay` plays it. That source has Loop off, so it is
    /// one play per run, cut by the `AudioStop` in `JA Check Hero Pos`.
    RunStart,
    /// `Open Uuup` and `Death Open`: the armour creaking open.
    ArmourOpen,
    /// `Hit` and `Hit 2`: the Head's cry.
    HeadHit,
    /// `Voice?` and `Voice? 2`: an attack cry, from the second phase on.
    Voice,
    /// `Death Land`: the heavy landing below the broken floor.
    DeathLand,
    /// `Floor Break`: BigShake, the floor's own voice and the slam's.
    FloorBreak,
    /// `Death Anim Start`: EnemyKillShake, `boss_final_hit`, and the
    /// `falseKnightDefeated` PlayerData write the source makes here.
    DeathStart,
    /// `Steam`: BigShake and the dying roar.
    Steam,
    /// `Blow`: BigShake as the armour splits.
    Blow,
}
/// What the `Head` child plays. It rides the body's transform and is parked
/// out of sight whenever [`FalseKnight::head_visible`] is false.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeadClip {
    /// The animator's default clip, and `Health Check`'s answer to STUN.
    Idle,
    /// `Hit` and `Hit 2` restart it on every landed head hit.
    Hit,
    /// `Death Anim Start`.
    Spaz,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Play(Clip),
    /// The Hitter overlay plays the attack art while the body plays Blank.
    PlayHitter(Clip),
    Velocity([i32; 2]),
    VelocityX(i32),
    Gravity(i32),
    Facing(i32),
    /// The Hitter's DamageHero trigger, switched by SetCollider/ActivateGameObject.
    Hitter(bool),
    Invincible(bool),
    ContactDamage(u16),
    /// The head hitbox is parked at y 150 while closed and dropped when opened.
    HeadExposed(bool),
    Kinematic(bool),
    /// `S Attack Recover`: a ground wave from the given local x, at 22 units/s.
    Shockwave { x: i32, right: bool },
    /// `SUMMON` with the Spawns the phase's RandomInt chose.
    SummonBarrels(u8),
    /// `CRACK` then `DESTROY` to the arena floor.
    CrackFloor,
    BreakFloor,
    /// `KILL ALL ENEMIES` from `Rubble End`, once the entrance drop lands.
    KillAllEnemies,
    /// A phase boundary was crossed; the value is the new Stunned Amount.
    Staggered(u8),
    /// `Decrement Battle Enemies`, immediately before the death event.
    Died,
    /// `Pause Long` sets falseKnightFirstPlop, which shortens every later plop.
    SetFirstPlop,
    /// `Tk2dPlayAnimation` on the `Head` child.
    Head(HeadClip),
    /// `Blow`: the body plays `Body`, the Head goes, and the `Death Head` drops
    /// out of the armour moving in the facing direction.
    Blow,
    /// `Death Head Land`: the Death Head stops and plays `Death Head 2`.
    DeathHeadLand,
    /// A `CameraShake` send, an `AudioPlaySimple`, or both, from the state this
    /// transition entered. Pushed last in every list, because nothing else in
    /// the list depends on it and the position then never has to be argued.
    Effect(Effect),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Actions {
    values: [Option<Action>; 8],
    count: u8,
}
impl Actions {
    const fn new() -> Self {
        Self { values: [None; 8], count: 0 }
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Senses {
    pub self_x: i32,
    pub hero_x: i32,
    /// GetDistance between the two transforms, Q16.
    pub distance: i32,
    /// Vertical speed after the caller's step, Q16 units per second.
    pub velocity_y: i32,
    pub velocity_x: i32,
    /// CheckCollisionSide bottom contact this frame.
    pub grounded: bool,
    /// The 8-unit terrain rays `Walls Check` casts before a random jump.
    pub wall_left: bool,
    pub wall_right: bool,
    /// The 9.5-unit downward ray the jump attack uses to commit its slam.
    pub ground_below: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FalseKnight {
    phase: Phase,
    timer: u16,
    clip_timer: u16,
    /// `Facing Right`; the transform scale carries the same sign.
    facing_right: bool,
    /// `Stunned Amount`, which is what selects the phase table.
    stunned: u8,
    rages: u8,
    turns: i32,
    jump_count: i32,
    ja_in_a_row: i32,
    slam_in_a_row: i32,
    jump_x: i32,
    recoil_speed: i32,
    shockwave_right: bool,
    first_jump: bool,
    first_plop: bool,
    /// `Recover` leads into the rage; `Stun Fail` returns straight to the fight.
    recover_to_rage: bool,
    rng: u32,
}
impl FalseKnight {
    /// `first_plop` is the saved `falseKnightFirstPlop`, which only matters for
    /// the length of the first pause after the armour lands.
    pub fn new(seed: u32, first_plop: bool) -> Self {
        Self {
            phase: Phase::Dormant,
            timer: 0,
            clip_timer: 0,
            facing_right: false,
            stunned: 0,
            rages: 0,
            turns: 0,
            jump_count: 0,
            ja_in_a_row: 0,
            slam_in_a_row: 0,
            jump_x: 0,
            recoil_speed: 0,
            shockwave_right: false,
            first_jump: false,
            first_plop,
            recover_to_rage: false,
            rng: seed | 1,
        }
    }
    pub fn phase(self) -> Phase {
        self.phase
    }
    pub fn stunned(self) -> u8 {
        self.stunned
    }
    pub fn facing_right(self) -> bool {
        self.facing_right
    }
    /// The phase table index, clamped: after the third stagger the boss is on
    /// its death path and never reads it again.
    pub fn table(self) -> usize {
        (self.stunned as usize).min(IDLE_TICKS.len() - 1)
    }
    /// The armour is open, so the nail reaches the Head instead of the body.
    pub fn head_exposed(self) -> bool {
        matches!(self.phase, Phase::Opened | Phase::StunHit | Phase::DeathOpened | Phase::DeathHit2)
    }
    /// The `Head` child is drawn: `Opened` and `Opened 2` move it into the
    /// armour and it stays until `Recover`, `Stun Fail` or `Blow` takes it away.
    pub fn head_visible(self) -> bool {
        self.head_exposed() || matches!(self.phase, Phase::Dying(0..=2))
    }
    /// The body refuses damage: `Stun Start` and `JA Antic 2` set Invincible.
    pub fn invincible(self) -> bool {
        matches!(
            self.phase,
            Phase::StunRoll
                | Phase::StunRollEnd
                | Phase::StunPause
                | Phase::StunOpening
                | Phase::Opened
                | Phase::StunHit
                | Phase::Recovering
                | Phase::DeathJumpAntic
                | Phase::DeathAir
                | Phase::DeathHit
                | Phase::DeathFall
                | Phase::DeathLand
                | Phase::DeathOpening
                | Phase::DeathOpened
                | Phase::DeathHit2
                | Phase::Dying(_)
                | Phase::Dead
        )
    }

    fn random(&mut self) -> u32 {
        self.rng = self.rng.wrapping_mul(1664525).wrapping_add(1013904223);
        self.rng >> 8
    }
    fn range(&mut self, low: i32, high: i32) -> i32 {
        low + ((self.random() as i64 * (high - low) as i64) >> 24) as i32
    }
    fn choose(&mut self, count: u32) -> u32 {
        (self.random() * count) >> 24
    }
    fn play(&mut self, clip: Clip, out: &mut Actions) {
        self.clip_timer = clip.ticks();
        out.push(Action::Play(clip));
    }
    fn face(&mut self, right: bool, out: &mut Actions) {
        self.facing_right = right;
        out.push(Action::Facing(if right { 1 } else { -1 }));
    }

    /// `BATTLE START` from the arena's `Start`.
    pub fn battle_start(&mut self) -> Actions {
        let mut out = Actions::new();
        if self.phase != Phase::Dormant {
            return out;
        }
        self.phase = Phase::Entrance;
        // `Start Fall`: the boss is hanging kinematic above the arena until here.
        out.push(Action::Kinematic(false));
        out.push(Action::Gravity(GRAVITY_STUN));
        out.push(Action::Effect(Effect::Entrance));
        out
    }
    /// `Check Health`: the body reached zero, so it staggers and restores to 65.
    /// Refused once `JA Fall 2` has sent FALLEN, which makes that FSM inert.
    pub fn body_reached_zero(&mut self) -> Actions {
        let mut out = Actions::new();
        if self.invincible() || matches!(self.phase, Phase::Dormant | Phase::Entrance) {
            return out;
        }
        // `Check Direction` turns to face the hero before `Stun Start`; the
        // roll always travels away from the new facing.
        self.phase = Phase::StunRoll;
        self.timer = 0;
        out.push(Action::Invincible(true));
        out.push(Action::ContactDamage(0));
        out.push(Action::Gravity(GRAVITY_STUN));
        let speed = if self.facing_right { -STUN_ROLL_SPEED } else { STUN_ROLL_SPEED };
        out.push(Action::Velocity([speed, STUN_JUMP_SPEED_Y]));
        self.play(Clip::StunRoll, &mut out);
        out.push(Action::Effect(Effect::StunStart));
        out
    }
    /// The exposed Head took a nail hit without dying.
    pub fn head_hit(&mut self) -> Actions {
        let mut out = Actions::new();
        match self.phase {
            Phase::Opened | Phase::StunHit => self.phase = Phase::StunHit,
            Phase::DeathOpened | Phase::DeathHit2 => self.phase = Phase::DeathHit2,
            _ => return out,
        }
        self.timer = STUN_HIT_TICKS;
        self.play(Clip::StunHit, &mut out);
        out.push(Action::Head(HeadClip::Hit));
        out.push(Action::Effect(Effect::HeadHit));
        out
    }
    /// `Health Check`: the Head reached zero, so it restores to 40 and sends
    /// STUN END. During the death exposure this begins the death animation.
    pub fn head_reached_zero(&mut self) -> Actions {
        let mut out = Actions::new();
        match self.phase {
            Phase::Opened | Phase::StunHit => {
                self.stunned += 1;
                self.phase = Phase::Recovering;
                self.recover_to_rage = true;
                self.timer = RECOVER_TICKS;
                out.push(Action::HeadExposed(false));
                out.push(Action::Kinematic(false));
                out.push(Action::ContactDamage(CONTACT_DAMAGE));
                out.push(Action::Staggered(self.stunned));
                self.play(Clip::StunRecover, &mut out);
                out.push(Action::Head(HeadClip::Idle));
            }
            Phase::DeathOpened | Phase::DeathHit2 => {
                self.phase = Phase::Dying(0);
                self.timer = DEATH_TAIL_TICKS[0];
                out.push(Action::HeadExposed(false));
                self.play(Clip::DeathSpaz, &mut out);
                out.push(Action::Head(HeadClip::Spaz));
                out.push(Action::Effect(Effect::DeathStart));
            }
            _ => {}
        }
        out
    }

    fn enter_idle(&mut self, out: &mut Actions) {
        self.phase = Phase::Idle;
        let [low, high] = IDLE_TICKS[self.table()];
        self.timer = low + self.choose((high - low + 1) as u32) as u16;
        out.push(Action::Gravity(GRAVITY_IDLE));
        out.push(Action::Hitter(false));
        self.play(Clip::Idle, out);
    }
    /// `Move Choice`: chase when far, otherwise one of three equally weighted
    /// attacks. A branch whose counter is spent sends RETURN, which re-enters
    /// this state; every branch resets the others' counters, so one is always
    /// available. The loop is bounded anyway, because a guest that spun here
    /// would hang rather than misbehave.
    fn move_choice(&mut self, senses: Senses, out: &mut Actions) {
        self.turns = 0;
        if senses.distance > RUN_TRIGGER_DISTANCE {
            self.phase = Phase::RunAntic;
            self.timer = RUN_ANTIC_TICKS;
            self.play(Clip::RunAntic, out);
            return;
        }
        for _ in 0..8 {
            let taken = match self.choose(3) {
                0 => self.try_slam(senses, out),
                1 => self.try_jump_attack(senses, out),
                _ => self.try_jump(senses, out),
            };
            if taken {
                return;
            }
        }
        self.slam_in_a_row = 0;
        self.try_slam(senses, out);
    }
    /// `Determine Jump`: one plain jump per visit, and none at all in phase 3.
    fn try_jump(&mut self, senses: Senses, out: &mut Actions) -> bool {
        if self.jump_count > 0 || self.stunned as i32 == 2 {
            return false;
        }
        self.jump_count += 1;
        if self.choose(2) == 0 {
            // `Walls Check`: the ray that hits decides the direction, and the
            // clamp that follows discards the sampled magnitude, leaving 5.
            self.jump_x = if senses.wall_left {
                RANDOM_JUMP_MIN
            } else if senses.wall_right {
                -RANDOM_JUMP_MIN
            } else {
                let sampled = self.range(-RANDOM_JUMP_MAX, RANDOM_JUMP_MAX);
                if sampled > 0 {
                    sampled.max(RANDOM_JUMP_MIN)
                } else {
                    sampled.min(-RANDOM_JUMP_MIN)
                }
            };
        } else {
            let hero = senses.hero_x.clamp(HERO_X_CLAMP[0], HERO_X_CLAMP[1]);
            let gap = (((hero - senses.self_x) as i64 * TOWARDS_FACTOR as i64) >> 16) as i32;
            self.jump_x = gap.clamp(-TOWARDS_CLAMP, TOWARDS_CLAMP);
        }
        self.phase = Phase::JumpAntic;
        self.timer = JUMP_ANTIC_TICKS;
        self.play(Clip::JumpAntic, out);
        true
    }
    /// `Row Check`, which is the budget only the Move Choice branch pays.
    fn try_jump_attack(&mut self, senses: Senses, out: &mut Actions) -> bool {
        if self.ja_in_a_row > JUMP_ATTACKS_IN_A_ROW {
            return false;
        }
        self.slam_in_a_row = 0;
        self.ja_in_a_row += 1;
        self.jump_attack(senses, out);
        true
    }
    /// `JA Check Hero Pos`: aim three units past the hero. `Run` reaches this
    /// directly, so closing the distance never spends the in-a-row budget.
    fn jump_attack(&mut self, senses: Senses, out: &mut Actions) {
        self.jump_count = 0;
        let hero_right = senses.hero_x > senses.self_x;
        let (offset, recoil) = if hero_right { (-JA_OFFSET, -JA_RECOIL_SPEED) } else { (JA_OFFSET, JA_RECOIL_SPEED) };
        self.recoil_speed = recoil;
        let gap = senses.hero_x + offset - senses.self_x;
        self.jump_x = (((gap as i64 * JA_ANTIC_FACTOR as i64) >> 16) as i32).clamp(-TOWARDS_CLAMP, TOWARDS_CLAMP);
        self.phase = Phase::JumpAttackAntic;
        self.timer = JUMP_ANTIC_TICKS;
        out.push(Action::VelocityX(0));
        self.play(Clip::JumpAntic, out);
        // `Voice?` sits between `Row Check`/`Run` and `JA Check Hero Pos`.
        if self.stunned != 0 {
            out.push(Action::Effect(Effect::Voice));
        }
    }
    /// `S Check Hero Pos`: leap 12 to 18 units past the hero, then slam back.
    /// From twelve units or further out it skips the leap and slams in place.
    fn try_slam(&mut self, senses: Senses, out: &mut Actions) -> bool {
        if self.slam_in_a_row > SLAMS_IN_A_ROW {
            return false;
        }
        self.ja_in_a_row = 0;
        self.slam_in_a_row += 1;
        self.jump_count = 0;
        let hero_right = senses.hero_x > senses.self_x;
        let overshoot = self.range(SLAM_OVERSHOOT[0], SLAM_OVERSHOOT[1]);
        let gap = if hero_right {
            senses.hero_x - overshoot - senses.self_x
        } else {
            senses.hero_x + overshoot - senses.self_x
        };
        self.shockwave_right = hero_right;
        if senses.distance >= SLAM_SKIP_JUMP_DISTANCE {
            self.phase = Phase::SlamStrikeAntic;
            self.timer = SLAM_ANTIC_TICKS;
            self.play(Clip::AttackAntic, out);
            // `S Antic` SMASH -> `Voice? 2` -> `S Attack Antic`.
            if self.stunned != 0 {
                out.push(Action::Effect(Effect::Voice));
            }
            return true;
        }
        self.jump_x = (((gap as i64 * TOWARDS_FACTOR as i64) >> 16) as i32).clamp(-TOWARDS_CLAMP, TOWARDS_CLAMP);
        self.phase = Phase::SlamAntic;
        self.timer = JUMP_ANTIC_TICKS;
        self.play(Clip::JumpAntic, out);
        true
    }
    fn barrels(&mut self, table: [[u8; 2]; 3]) -> u8 {
        let [low, high] = table[self.table()];
        if high <= low {
            low
        } else {
            low + self.choose((high - low + 1) as u32) as u8
        }
    }

    /// One 60 Hz frame. `senses` describes the world after the caller stepped
    /// the body with the velocity this returned last frame.
    pub fn tick(&mut self, senses: Senses) -> Actions {
        let mut out = Actions::new();
        self.clip_timer = self.clip_timer.saturating_sub(1);
        match self.phase {
            Phase::Dormant | Phase::Dead => {}
            Phase::Entrance => {
                if senses.grounded {
                    // `Rubble End` clears the room before `State 1` lands.
                    out.push(Action::KillAllEnemies);
                    self.phase = Phase::Landing;
                    self.timer = LAND_TICKS;
                    out.push(Action::VelocityX(0));
                    self.play(Clip::Land, &mut out);
                    // `State 2` is the entrance's own landing state, which this
                    // transition enters ahead of `Rubble End`.
                    out.push(Action::Effect(Effect::EntranceLanding));
                }
            }
            Phase::Landing => {
                self.timer -= 1;
                if self.timer == 0 {
                    // `Check`: the opening jump is scripted, every later one is not.
                    if self.first_jump {
                        self.enter_idle(&mut out);
                    } else {
                        self.phase = Phase::FirstIdle;
                        self.timer = FIRST_IDLE_TICKS;
                        out.push(Action::Gravity(GRAVITY_IDLE));
                        out.push(Action::Hitter(false));
                        self.play(Clip::Idle, &mut out);
                    }
                }
            }
            Phase::FirstIdle => {
                self.timer -= 1;
                if self.timer == 0 {
                    self.first_jump = true;
                    // `First Idle` jumps straight into `Random`, skipping the
                    // wall rays and the jump budget `Determine Jump` applies.
                    let sampled = self.range(-RANDOM_JUMP_MAX, RANDOM_JUMP_MAX);
                    self.jump_x = if sampled > 0 { sampled.max(RANDOM_JUMP_MIN) } else { sampled.min(-RANDOM_JUMP_MIN) };
                    self.phase = Phase::JumpAntic;
                    self.timer = JUMP_ANTIC_TICKS;
                    self.play(Clip::JumpAntic, &mut out);
                }
            }
            Phase::Idle => {
                let hero_right = senses.hero_x > senses.self_x;
                if hero_right != self.facing_right {
                    // `Turn R`/`Turn L`, with the fourth turn forcing an attack.
                    if self.turns == TURNS_BEFORE_ATTACK {
                        self.move_choice(senses, &mut out);
                        return out;
                    }
                    self.turns += 1;
                    self.phase = Phase::Turn;
                    self.timer = TURN_TICKS;
                    self.face(hero_right, &mut out);
                    self.play(Clip::Turn, &mut out);
                    return out;
                }
                self.timer -= 1;
                if self.timer == 0 {
                    self.move_choice(senses, &mut out);
                }
            }
            Phase::Turn => {
                self.timer -= 1;
                if self.timer == 0 {
                    self.enter_idle(&mut out);
                }
            }
            Phase::JumpAntic | Phase::SlamAntic | Phase::RageJumpAntic | Phase::DeathJumpAntic => {
                self.timer -= 1;
                if self.timer == 0 {
                    // `Jump`, `S Jump` and `Jump 2` each send EnemyKillShake.
                    // The last jump of the fight, `JA Jump 2`, is the one launch
                    // that sends nothing, so the effect rides the same table
                    // rather than the shared tail below.
                    let (gravity, speed, clip, next, effect) = match self.phase {
                        Phase::JumpAntic => (GRAVITY_JUMP, JUMP_SPEED_Y, Clip::Jump, Phase::Airborne, Some(Effect::JumpShake)),
                        Phase::SlamAntic => (GRAVITY_JUMP, SLAM_JUMP_SPEED_Y, Clip::Jump, Phase::SlamAir, Some(Effect::JumpShake)),
                        Phase::RageJumpAntic => (GRAVITY_RAGE_JUMP, SLAM_JUMP_SPEED_Y, Clip::Jump, Phase::RageAir, Some(Effect::RageJump)),
                        _ => (GRAVITY_DEATH_JUMP, JUMP_SPEED_Y, Clip::JumpAttackUp, Phase::DeathAir, Some(Effect::Jump)),
                    };
                    self.phase = next;
                    out.push(Action::Gravity(gravity));
                    out.push(Action::Velocity([self.jump_x, speed]));
                    self.play(clip, &mut out);
                    if let Some(effect) = effect {
                        out.push(Action::Effect(effect));
                    }
                }
            }
            Phase::JumpAttackAntic => {
                self.timer -= 1;
                if self.timer == 0 {
                    self.phase = Phase::JumpAttackAir;
                    out.push(Action::Gravity(GRAVITY_JUMP_ATTACK));
                    out.push(Action::Velocity([self.jump_x, JUMP_SPEED_Y]));
                    self.play(Clip::JumpAttackUp, &mut out);
                    // `JA Jump`, the fourth of the four launches that shake.
                    out.push(Action::Effect(Effect::JumpShake));
                }
            }
            Phase::Airborne | Phase::SlamAir | Phase::RageAir => {
                // `Rise`/`Fall`: shape the vertical speed, then land.
                let rising = senses.velocity_y > 0;
                let factor = if rising { RISE_MULTIPLIER } else { FALL_MULTIPLIER };
                out.push(Action::Velocity([senses.velocity_x, ((senses.velocity_y as i64 * factor as i64) >> 16) as i32]));
                if !rising && senses.grounded {
                    out.push(Action::VelocityX(0));
                    self.timer = LAND_TICKS;
                    self.play(Clip::Land, &mut out);
                    // `Land Noise` plays the landing voice alone and `S Land`
                    // adds AverageShake to it; the rage landing state plays no
                    // clip and sends no shake, so it is the one that gets none.
                    let (next, effect) = match self.phase {
                        Phase::Airborne => (Phase::Landing, Some(Effect::Landing)),
                        Phase::SlamAir => (Phase::SlamLand, Some(Effect::LandingShake)),
                        _ => (Phase::RageLand, None),
                    };
                    self.phase = next;
                    if let Some(effect) = effect {
                        out.push(Action::Effect(effect));
                    }
                }
            }
            Phase::JumpAttackAir | Phase::DeathAir => {
                let rising = senses.velocity_y > 0;
                let factor = if rising { RISE_MULTIPLIER } else { FALL_MULTIPLIER };
                out.push(Action::Velocity([senses.velocity_x, ((senses.velocity_y as i64 * factor as i64) >> 16) as i32]));
                // `JA Fall` commits as soon as terrain is within 9.5 units.
                if !rising && senses.ground_below {
                    out.push(Action::Hitter(true));
                    out.push(Action::Play(Clip::Blank));
                    out.push(Action::PlayHitter(Clip::JumpAttackHit1));
                    self.phase = if self.phase == Phase::JumpAttackAir { Phase::JumpAttackHit } else { Phase::DeathHit };
                }
            }
            Phase::JumpAttackHit => {
                if senses.grounded {
                    self.phase = Phase::JumpAttackSlam;
                    self.timer = JA_SLAM_TICKS;
                    out.push(Action::VelocityX(0));
                    // This transition is the entry to `JA Slam`, whose own wait
                    // the phase below holds, so the impact shake belongs here
                    // rather than five ticks later with `JA Hit 2`'s swing.
                    out.push(Action::Effect(Effect::Slam));
                }
            }
            Phase::JumpAttackSlam => {
                self.timer -= 1;
                if self.timer == 0 {
                    // `Barrels?`: the jump attack only summons from phase 3 on.
                    if self.stunned >= 2 {
                        let count = self.barrels(JUMP_BARRELS);
                        out.push(Action::SummonBarrels(count));
                    }
                    self.phase = Phase::JumpAttackRecoil;
                    self.timer = JA_RECOIL_TICKS;
                    out.push(Action::Hitter(false));
                    out.push(Action::VelocityX(self.recoil_speed));
                    self.play(Clip::JumpAttackHit2, &mut out);
                    // `JA Hit 2` swings the mace back out of the floor.
                    out.push(Action::Effect(Effect::Swing));
                }
            }
            Phase::JumpAttackRecoil => {
                self.timer -= 1;
                if self.timer == 0 {
                    self.phase = Phase::JumpAttackRecoil2;
                    self.timer = JA_RECOIL2_TICKS;
                    self.recoil_speed /= 2;
                    out.push(Action::VelocityX(self.recoil_speed));
                    self.play(Clip::JumpAttackHit3, &mut out);
                }
            }
            Phase::JumpAttackRecoil2 => {
                self.timer -= 1;
                if self.timer == 0 {
                    self.phase = Phase::JumpAttackEnd;
                    out.push(Action::Gravity(GRAVITY_IDLE));
                    out.push(Action::VelocityX(0));
                }
            }
            Phase::JumpAttackEnd => {
                // `JA End` waits out the rest of the clip `JA Recoil 2` started.
                if self.clip_timer == 0 {
                    self.enter_idle(&mut out);
                }
            }
            Phase::SlamLand => {
                self.timer -= 1;
                if self.timer == 0 {
                    self.phase = Phase::SlamStrikeAntic;
                    self.timer = SLAM_ANTIC_TICKS;
                    self.play(Clip::AttackAntic, &mut out);
                    // `S Land` -> `Voice? 2` -> `S Attack Antic`.
                    if self.stunned != 0 {
                        out.push(Action::Effect(Effect::Voice));
                    }
                }
            }
            Phase::SlamStrikeAntic => {
                self.timer -= 1;
                if self.timer == 0 {
                    self.phase = Phase::SlamStrike;
                    self.timer = SLAM_STRIKE_TICKS;
                    out.push(Action::Hitter(true));
                    out.push(Action::Play(Clip::Blank));
                    out.push(Action::PlayHitter(Clip::Attack));
                    // `S Attack` is the swing itself, ahead of the `Slam` below.
                    out.push(Action::Effect(Effect::Swing));
                }
            }
            Phase::SlamStrike => {
                self.timer -= 1;
                if self.timer == 0 {
                    // `S Attack Recover`: the wave, the barrels and the recovery.
                    let count = self.barrels(SLAM_BARRELS);
                    let origin = if self.shockwave_right { SHOCKWAVE_X_ORIGIN } else { -SHOCKWAVE_X_ORIGIN };
                    self.phase = Phase::SlamRecover;
                    self.timer = SLAM_RECOVER_TICKS;
                    out.push(Action::SummonBarrels(count));
                    out.push(Action::Shockwave { x: origin, right: self.shockwave_right });
                    out.push(Action::Hitter(false));
                    self.play(Clip::AttackRecover, &mut out);
                    // `Slam`, which this transition passes through on its way to
                    // `S Attack Recover`, is where the mace reaches the floor.
                    out.push(Action::Effect(Effect::Slam));
                }
            }
            Phase::SlamRecover => {
                self.timer -= 1;
                if self.timer == 0 {
                    self.enter_idle(&mut out);
                }
            }
            Phase::RunAntic => {
                self.timer -= 1;
                if self.timer == 0 {
                    self.phase = Phase::Run;
                    let speed = if self.facing_right { RUN_SPEED } else { -RUN_SPEED };
                    out.push(Action::VelocityX(speed));
                    self.play(Clip::Run, &mut out);
                    out.push(Action::Effect(Effect::RunStart));
                }
            }
            Phase::Run => {
                let speed = if self.facing_right { RUN_SPEED } else { -RUN_SPEED };
                out.push(Action::VelocityX(speed));
                if senses.distance < RUN_STOP_DISTANCE {
                    self.jump_attack(senses, &mut out);
                }
            }
            Phase::StunRoll => {
                // `Stun In Air` waits for a bottom contact the launch broke, so
                // the frame the roll starts on cannot end it.
                if senses.grounded && senses.velocity_y <= 0 {
                    self.timer += 1;
                    if self.timer == 1 {
                        out.push(Action::Effect(Effect::Roll));
                    }
                    // `Stun Land`: the roll ends the moment the body is moving
                    // left faster than the three-unit tolerance, otherwise it
                    // rides out the half-second wait.
                    if senses.velocity_x < -STUN_ROLL_STOP_SPEED || self.timer >= ROLL_STOP_TICKS {
                        self.phase = Phase::StunRollEnd;
                        self.timer = ROLL_END_TICKS;
                        out.push(Action::Kinematic(true));
                        out.push(Action::Velocity([0, 0]));
                        self.play(Clip::StunRollEnd, &mut out);
                    }
                }
            }
            Phase::StunRollEnd => {
                self.timer -= 1;
                if self.timer == 0 {
                    self.phase = Phase::StunPause;
                    self.timer = if self.first_plop { PLOP_SHORT_TICKS } else { PLOP_LONG_TICKS };
                    if !self.first_plop {
                        self.first_plop = true;
                        out.push(Action::SetFirstPlop);
                    }
                }
            }
            Phase::StunPause => {
                self.timer -= 1;
                if self.timer == 0 {
                    self.phase = Phase::StunOpening;
                    self.timer = OPEN_TICKS;
                    self.play(Clip::StunOpen, &mut out);
                    out.push(Action::Effect(Effect::ArmourOpen));
                }
            }
            Phase::StunOpening => {
                self.timer -= 1;
                if self.timer == 0 {
                    self.phase = Phase::Opened;
                    self.timer = STUN_WINDOW_TICKS;
                    out.push(Action::HeadExposed(true));
                    self.play(Clip::StunOpened, &mut out);
                }
            }
            Phase::Opened => {
                self.timer -= 1;
                if self.timer == 0 {
                    // `Stun Fail`: the armour shuts with no phase advance.
                    self.phase = Phase::Recovering;
                    self.recover_to_rage = false;
                    self.timer = RECOVER_TICKS;
                    out.push(Action::HeadExposed(false));
                    out.push(Action::Kinematic(false));
                    out.push(Action::ContactDamage(CONTACT_DAMAGE));
                    self.play(Clip::StunRecover, &mut out);
                }
            }
            Phase::StunHit => {
                self.timer -= 1;
                if self.timer == 0 {
                    // `Hit` returns to `Opened`, which restarts the five seconds.
                    self.phase = Phase::Opened;
                    self.timer = STUN_WINDOW_TICKS;
                    self.play(Clip::StunOpened, &mut out);
                }
            }
            Phase::Recovering => {
                self.timer -= 1;
                if self.timer == 0 {
                    out.push(Action::Invincible(false));
                    if self.recover_to_rage {
                        self.phase = Phase::RagePause;
                        self.timer = IDLE_PAUSE_TICKS;
                        self.play(Clip::Idle, &mut out);
                    } else {
                        // A stagger the player failed to convert costs nothing.
                        self.enter_idle(&mut out);
                    }
                }
            }
            Phase::RagePause => {
                self.timer -= 1;
                if self.timer == 0 {
                    self.phase = Phase::RageJumpAntic;
                    self.timer = JUMP_ANTIC_TICKS;
                    let gap = RAGE_POINT_X - senses.self_x;
                    self.jump_x = ((gap as i64 * RAGE_JUMP_FACTOR as i64) >> 16) as i32;
                    self.play(Clip::JumpAntic, &mut out);
                }
            }
            Phase::RageLand => {
                self.timer -= 1;
                if self.timer == 0 {
                    self.phase = Phase::RageAntic;
                    self.timer = RAGE_ANTIC_TICKS;
                    self.rages = RAGE_SLAMS;
                    out.push(Action::SummonBarrels(RAGE_SLAMS));
                    self.play(Clip::AttackAntic, &mut out);
                }
            }
            Phase::RageAntic => {
                self.timer -= 1;
                if self.timer == 0 {
                    self.phase = Phase::RageSlam;
                    self.timer = RAGE_SLAM_TICKS;
                    out.push(Action::Hitter(true));
                    out.push(Action::PlayHitter(Clip::Rage));
                    out.push(Action::Play(Clip::Blank));
                    self.clip_timer = Clip::Rage.ticks();
                }
            }
            Phase::RageSlam => {
                self.timer -= 1;
                if self.timer == 0 {
                    // `Floor Crack?`: only the last slam of the phase-3 rage
                    // cracks the floor the death jump later breaks.
                    if self.stunned == 2 && self.rages == 1 {
                        out.push(Action::CrackFloor);
                    }
                    self.phase = Phase::RageTurn;
                    self.timer = RAGE_PARTICLE_TICKS;
                    // The state this leaves is `Rage`; `Rage Slam` is the one it
                    // enters, and its AverageShake is all of it this port has.
                    out.push(Action::Effect(Effect::RageSlam));
                }
            }
            Phase::RageTurn => {
                self.timer = self.timer.saturating_sub(1);
                // `Particle Pause`, `Particle End` then `Anim End`, which waits
                // out the Hitter's Rage clip before the turn.
                if self.timer == 0 && self.clip_timer == 0 {
                    self.rages -= 1;
                    self.face(!self.facing_right, &mut out);
                    if self.rages > 0 {
                        self.phase = Phase::RageSlam;
                        self.timer = RAGE_SLAM_TICKS;
                        out.push(Action::PlayHitter(Clip::Rage));
                        self.clip_timer = Clip::Rage.ticks();
                    } else if self.stunned >= STAGGERS {
                        // `Rage Check` BREAK2: the fight is over.
                        self.phase = Phase::DeathJumpAntic;
                        self.timer = JUMP_ANTIC_TICKS;
                        let gap = FINAL_POINT_X - senses.self_x;
                        self.jump_x = ((gap as i64 * FINAL_JUMP_FACTOR as i64) >> 16) as i32;
                        out.push(Action::Invincible(true));
                        out.push(Action::Hitter(false));
                        self.face(true, &mut out);
                        self.play(Clip::JumpAntic, &mut out);
                    } else {
                        self.phase = Phase::RageEnd;
                        self.timer = RAGE_END_TICKS;
                        out.push(Action::Hitter(false));
                        self.play(Clip::Idle, &mut out);
                    }
                }
            }
            Phase::RageEnd => {
                self.timer -= 1;
                if self.timer == 0 {
                    self.enter_idle(&mut out);
                }
            }
            Phase::DeathHit => {
                if senses.grounded {
                    // `Floor Break`: the cracked floor gives way.
                    self.phase = Phase::DeathFall;
                    out.push(Action::Hitter(false));
                    out.push(Action::ContactDamage(0));
                    out.push(Action::Gravity(GRAVITY_STUN));
                    out.push(Action::BreakFloor);
                    out.push(Action::Velocity([0, DEATH_FALL_SPEED_Y]));
                    self.play(Clip::DeathFall, &mut out);
                    out.push(Action::Effect(Effect::FloorBreak));
                }
            }
            Phase::DeathFall => {
                if senses.grounded && senses.velocity_y <= 0 {
                    self.phase = Phase::DeathLand;
                    self.timer = DEATH_LAND_TICKS;
                    out.push(Action::Kinematic(true));
                    self.play(Clip::DeathLand, &mut out);
                    out.push(Action::Effect(Effect::DeathLand));
                }
            }
            Phase::DeathLand => {
                self.timer -= 1;
                if self.timer == 0 {
                    self.phase = Phase::DeathOpening;
                    self.timer = OPEN_TICKS;
                    self.play(Clip::StunOpen, &mut out);
                    out.push(Action::Effect(Effect::ArmourOpen));
                }
            }
            Phase::DeathOpening => {
                self.timer -= 1;
                if self.timer == 0 {
                    // `Opened 2` has no timeout: the armour stays open until the
                    // player finishes the head.
                    self.phase = Phase::DeathOpened;
                    out.push(Action::HeadExposed(true));
                    self.play(Clip::StunOpened, &mut out);
                }
            }
            Phase::DeathOpened => {}
            Phase::DeathHit2 => {
                self.timer -= 1;
                if self.timer == 0 {
                    self.phase = Phase::DeathOpened;
                    self.play(Clip::StunOpened, &mut out);
                }
            }
            Phase::Dying(step) => {
                self.timer -= 1;
                if self.timer == 0 {
                    let next = step + 1;
                    if (next as usize) < DEATH_TAIL_TICKS.len() {
                        self.phase = Phase::Dying(next);
                        self.timer = DEATH_TAIL_TICKS[next as usize];
                        // `Steam`, then `Blow` (after `Ready`), then `Death
                        // Head Land` once the Death Head's clip has played.
                        match next {
                            1 => out.push(Action::Effect(Effect::Steam)),
                            3 => {
                                out.push(Action::Blow);
                                out.push(Action::Effect(Effect::Blow));
                            }
                            4 => out.push(Action::DeathHeadLand),
                            _ => {}
                        }
                    } else {
                        // `Decrement Battle Enemies` then `Cough`, which raises
                        // the HealthManager death event the arena is waiting on.
                        self.phase = Phase::Dead;
                        out.push(Action::Died);
                    }
                }
            }
        }
        out
    }
}
