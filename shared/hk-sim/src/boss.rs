//! Source `Battle Control` arena lifecycle, shared by every boss room.
//!
//! Recovered from the False Knight's Battle Scene (Crossroads_10_boss); the
//! same FSM drives the other arenas, so the second boss reuses this rather than
//! growing its own copy. Evidence and numbers: host/false_knight.py.
//!
//! The caller owns the trigger box, the gate objects, the camera lock and the
//! persistent store. This type owns only the order the source imposes on them,
//! which is where a boss room softlocks if it is guessed rather than read.

/// `End Wait`: the gates stay shut for two seconds after the last enemy dies.
pub const END_WAIT_TICKS: u16 = 120;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// `Detect`: gates open, waiting for the hero to cross the trigger.
    Waiting,
    /// `Start`: gates shut, camera locked, the boss has been told to begin.
    Fighting,
    /// `Kill Zombies`: the boss has cleared the room's ordinary enemies and the
    /// arena is counting down to zero.
    Clearing,
    /// `End Wait`: everything is dead, the gates have not opened yet.
    Ending,
    /// `End`, and the `Activate` branch a later visit takes.
    Open,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// `BG CLOSE`, broadcast to every gate in the room and the boss scene.
    CloseGates,
    /// `BG OPEN`, once the fight is over in this visit.
    OpenGates,
    /// `BG QUICK OPEN`, taken on entry when the arena is already activated:
    /// the gates are placed open with no animation and no sound.
    QuickOpenGates,
    /// `BATTLE START`, broadcast; the boss leaves its dormant state on it.
    StartBattle,
    /// `ACTIVATE`, sent to the arena floor on the already-defeated path.
    ActivateFloor,
    CameraLock(bool),
    /// `Kill Zombies`: 9999 damage to each pre-battle enemy, ignoring invulnerability.
    KillMinions,
    /// The `Activated` PersistentBoolItem becomes true and must be saved.
    Persist,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Actions {
    values: [Option<Action>; 6],
    count: u8,
}
impl Actions {
    const fn new() -> Self {
        Self { values: [None; 6], count: 0 }
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
pub struct Arena {
    phase: Phase,
    /// `Battle Enemies`, which the source sets to 1 for the boss alone. The
    /// pre-battle enemies are killed outright and never counted.
    enemies: i32,
    timer: u16,
    end_wait_ticks: u16,
    activated: bool,
}
impl Arena {
    /// `Pause` -> `Init`: an arena whose `Activated` bool survived a save takes
    /// the `Activate` branch on the first frame and never fights again.
    pub fn new(activated: bool) -> Self {
        Self {
            phase: if activated { Phase::Open } else { Phase::Waiting },
            enemies: 0,
            timer: 0,
            end_wait_ticks: END_WAIT_TICKS,
            activated,
        }
    }
    /// Brooding Mawlek's `Battle Control` waits `Blow Wait` then `End Wait`
    /// (10.5 s) between BATTLE END and BG OPEN, where the False Knight's waits 2.
    pub fn with_end_wait(activated: bool, ticks: u16) -> Self {
        Self { end_wait_ticks: ticks, ..Self::new(activated) }
    }
    /// The actions `Init` performs before its first idle frame. Calling this
    /// exactly once after `new` reproduces the source's load-time branch, so a
    /// reloaded save re-opens the gates and re-activates the floor rather than
    /// leaving the hero sealed in a room whose boss is gone.
    pub fn enter(&self) -> Actions {
        let mut out = Actions::new();
        if self.activated {
            out.push(Action::QuickOpenGates);
            out.push(Action::ActivateFloor);
        } else {
            out.push(Action::CameraLock(false));
        }
        out
    }
    pub fn phase(self) -> Phase {
        self.phase
    }
    pub fn activated(self) -> bool {
        self.activated
    }
    pub fn enemies(self) -> i32 {
        self.enemies
    }
    /// `Detect`: the hero's collider entered the Battle Scene trigger.
    pub fn hero_entered(&mut self) -> Actions {
        let mut out = Actions::new();
        if self.phase != Phase::Waiting {
            return out;
        }
        self.phase = Phase::Fighting;
        self.enemies = 1;
        out.push(Action::CameraLock(true));
        out.push(Action::StartBattle);
        out.push(Action::CloseGates);
        out
    }
    /// `KILL ALL ENEMIES`, which the boss sends once it has landed. Until then
    /// the arena cannot end, even with the counter at zero, because `Start` has
    /// no every-frame comparison; only `Kill Zombies` has one.
    pub fn kill_all_enemies(&mut self) -> Actions {
        let mut out = Actions::new();
        if self.phase != Phase::Fighting {
            return out;
        }
        self.phase = Phase::Clearing;
        out.push(Action::KillMinions);
        out
    }
    /// `Start`'s SetIntValue on `Battle Enemies`, for an arena that counts
    /// more than its boss: Gruz Mother's `Battle Control` sets 7, the reserve
    /// flies, and its boss is not one of them.
    pub fn set_enemies(&mut self, enemies: i32) {
        self.enemies = enemies;
    }
    /// `Decrement Battle Enemies`, sent by the boss just before its death event.
    pub fn enemy_died(&mut self) {
        self.enemies -= 1;
    }
    pub fn tick(&mut self) -> Actions {
        let mut out = Actions::new();
        match self.phase {
            Phase::Clearing => {
                if self.enemies <= 0 {
                    self.phase = Phase::Ending;
                    self.timer = self.end_wait_ticks;
                    // `End Wait` writes Activated before the wait, so a save
                    // taken during those two seconds already counts as cleared.
                    self.activated = true;
                    out.push(Action::Persist);
                }
            }
            Phase::Ending => {
                self.timer -= 1;
                if self.timer == 0 {
                    self.phase = Phase::Open;
                    out.push(Action::OpenGates);
                }
            }
            Phase::Waiting | Phase::Fighting | Phase::Open => {}
        }
        out
    }
}
