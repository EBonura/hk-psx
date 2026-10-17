//! The room's nail-struck switches and the gates they open, as plain tables.
//!
//! Four PlayMaker FSMs of the admitted rooms, read from the source scenes:
//!
//! * `toll switch` (Crossroads_03's `Toll Gate Switch`): a nail hit sets
//!   `Activated`, shakes the camera, plays its hit clip and waits 1.0 s; `Open`
//!   then sends OPEN to the two `Toll Gate`s. A scene loaded with it already
//!   activated plays the activated pose and destroys the gates outright.
//! * `Switch Control` (Crossroads_06's `Gate Switch`, Town's `Mines Lever`): the
//!   same hit with a 0.75 s wait; `Open` sends ACTIVATE to its target. Loaded
//!   activated, `Activated` plays the pose and sends ACTIVATE, then runs `Open`.
//! * `Toll Gate`: OPEN plays `Toll Gate Open` to its last frame, then
//!   `Destroy Self` (collider and art gone); DESTROY goes there at once.
//! * `Pillar Control` (Crossroads_06's `Raising Pillar`): ACTIVATE starts
//!   `Shake` (0.75 s), `Open` (collider off, plays to its end), `Finish Opening`
//!   (0.5 s) and holds `Opened`. Loaded with its own `Activated` set it starts
//!   in `Opened`.
//!
//! A machine is states, events and actions in plain tables, so a host can read
//! the same shape a script bank would hold: a state has the actions it runs on
//! entry, an optional wait that leads to `then` (PlayMaker's FINISHED), and the
//! events it answers. Anything the source does that this game has no effect for
//! (the sounds, the dust emitters, the `Pause` frame, the `SetPlayerData` bool)
//! is left out; they change nothing a route can observe. The caller owns the
//! hit test against the switch's trigger, the camera shake, the persisted
//! `Activated`, the terrain edges and the art; this type owns only the order.
//! Every number is a 16-bit tick count at the 60 Hz simulation rate.

/// Events a machine answers, sent by the caller or by another machine's action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// The nail overlapped the switch's trigger box.
    NailHit,
    /// A toll switch's `OPEN`.
    Open,
    /// A switch's `ACTIVATE`.
    Activate,
    /// `DestroyObject` on a gate: gone without playing its clip.
    Destroy,
}

/// The clip an instance plays, by role; the cook binds each to a cooked clip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Idle,
    Hit,
    Activated,
    Opening,
    Shake,
    Finish,
    Opened,
}
pub const ROLES: usize = 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Start the clip of this role.
    Clip(Role),
    /// `PersistentBoolItem`: the switch's `Activated` must be saved.
    Persist,
    /// `EnemyKillShake` to the camera.
    Shake,
    /// `SendEventByName` to the machine's targets.
    Send(Event),
    /// `SetCollider`/`SetPolygonCollider` false on the owner: its terrain edges lift.
    ColliderOff,
    /// `DestroySelf`: the art is gone too.
    Destroy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wait {
    None,
    /// `Wait`, in ticks.
    Ticks(u16),
    /// `Tk2dWatchAnimationEvents`: the clip entered with the state has played out.
    ClipEnd,
}

pub struct State {
    pub name: &'static str,
    pub enter: &'static [Action],
    pub wait: Wait,
    /// Where a finished wait leads (`FINISHED`).
    pub then: Option<u8>,
    pub on: &'static [(Event, u8)],
}

pub struct Machine {
    pub name: &'static str,
    /// The state of a fresh `Activated` false.
    pub start: u8,
    /// The state a scene loaded with `Activated` true begins in.
    pub restored: u8,
    pub states: &'static [State],
}

const fn state(
    name: &'static str,
    enter: &'static [Action],
    wait: Wait,
    then: Option<u8>,
    on: &'static [(Event, u8)],
) -> State {
    State { name, enter, wait, then, on }
}

/// `toll switch`.
pub static TOLL_SWITCH: Machine = Machine {
    name: "toll switch",
    start: 0,
    restored: 3,
    states: &[
        state("Idle", &[Action::Clip(Role::Idle)], Wait::None, None, &[(Event::NailHit, 1)]),
        state(
            "Hit",
            &[Action::Persist, Action::Shake, Action::Clip(Role::Hit)],
            Wait::Ticks(60),
            Some(2),
            &[],
        ),
        state("Open", &[Action::Send(Event::Open)], Wait::None, None, &[]),
        state(
            "Activate",
            &[Action::Clip(Role::Activated), Action::Send(Event::Destroy)],
            Wait::None,
            None,
            &[],
        ),
    ],
};

/// `Switch Control`.
pub static SWITCH_CONTROL: Machine = Machine {
    name: "Switch Control",
    start: 0,
    restored: 3,
    states: &[
        state("Idle", &[Action::Clip(Role::Idle)], Wait::None, None, &[(Event::NailHit, 1)]),
        state(
            "Hit",
            &[Action::Persist, Action::Shake, Action::Clip(Role::Hit)],
            Wait::Ticks(45),
            Some(2),
            &[],
        ),
        state("Open", &[Action::Send(Event::Activate)], Wait::None, None, &[]),
        // The source's `Activated` ends with FINISHED into `Open`, which sends again.
        state(
            "Activated",
            &[Action::Clip(Role::Activated), Action::Send(Event::Activate)],
            Wait::Ticks(1),
            Some(2),
            &[],
        ),
    ],
};

/// `Toll Gate`.
pub static TOLL_GATE: Machine = Machine {
    name: "Toll Gate",
    start: 0,
    restored: 2,
    states: &[
        state(
            "Idle",
            &[Action::Clip(Role::Idle)],
            Wait::None,
            None,
            &[(Event::Open, 1), (Event::Destroy, 2)],
        ),
        state(
            "Open",
            &[Action::Clip(Role::Opening)],
            Wait::ClipEnd,
            Some(2),
            &[(Event::Destroy, 2)],
        ),
        state("Destroy Self", &[Action::ColliderOff, Action::Destroy], Wait::None, None, &[]),
    ],
};

/// `Pillar Control`.
pub static PILLAR_CONTROL: Machine = Machine {
    name: "Pillar Control",
    start: 0,
    restored: 4,
    states: &[
        state("Idle", &[Action::Clip(Role::Idle)], Wait::None, None, &[(Event::Activate, 1)]),
        state("Shake", &[Action::Clip(Role::Shake)], Wait::Ticks(45), Some(2), &[]),
        state(
            "Open",
            &[Action::Clip(Role::Opening), Action::ColliderOff],
            Wait::ClipEnd,
            Some(3),
            &[],
        ),
        state("Finish Opening", &[Action::Clip(Role::Finish)], Wait::Ticks(30), Some(4), &[]),
        state("Opened", &[Action::Clip(Role::Opened), Action::ColliderOff], Wait::None, None, &[]),
    ],
};

/// What one step asked of the caller, in the order the source runs it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Actions {
    items: [Option<Action>; 6],
    len: u8,
}
impl Actions {
    pub const fn none() -> Self {
        Self { items: [None; 6], len: 0 }
    }
    fn push(&mut self, action: Action) {
        // The longest state entry has three actions and a step enters at most
        // two states.
        assert!((self.len as usize) < self.items.len());
        self.items[self.len as usize] = Some(action);
        self.len += 1;
    }
    pub fn iter(&self) -> impl Iterator<Item = Action> + '_ {
        self.items[..self.len as usize].iter().flatten().copied()
    }
    pub fn contains(&self, action: Action) -> bool {
        self.iter().any(|a| a == action)
    }
}

/// One switch, gate or pillar in a room.
#[derive(Clone, Copy)]
pub struct Instance {
    machine: &'static Machine,
    state: u8,
    age: u16,
    /// Ticks each role's clip plays for (`Wait::ClipEnd`), from the cooked clips.
    clip_ticks: [u16; ROLES],
    /// The clip entered with the state, for `Wait::ClipEnd`.
    playing: Option<Role>,
}
impl Instance {
    /// A machine as its scene loads it: `activated` is the persisted `Activated`.
    /// The state's entry actions are returned so the caller can show its pose.
    pub fn new(machine: &'static Machine, clip_ticks: [u16; ROLES], activated: bool) -> (Self, Actions) {
        let mut me = Self { machine, state: 0, age: 0, clip_ticks, playing: None };
        let mut out = Actions::none();
        me.enter(if activated { machine.restored } else { machine.start }, &mut out);
        (me, out)
    }
    pub fn state(&self) -> &'static str {
        self.machine.states[self.state as usize].name
    }
    fn enter(&mut self, state: u8, out: &mut Actions) {
        self.state = state;
        self.age = 0;
        self.playing = None;
        for &action in self.machine.states[state as usize].enter {
            if let Action::Clip(role) = action {
                self.playing = Some(role);
            }
            out.push(action);
        }
    }
    /// Deliver an event (PlayMaker's synchronous `SendEvent`).
    pub fn event(&mut self, event: Event, out: &mut Actions) {
        let current = &self.machine.states[self.state as usize];
        if let Some(&(_, to)) = current.on.iter().find(|&&(e, _)| e == event) {
            self.enter(to, out);
        }
    }
    /// One 60 Hz tick: a finished wait leads to the next state. A state entered
    /// by an event counts its wait from the next tick.
    pub fn tick(&mut self, out: &mut Actions) {
        let current = &self.machine.states[self.state as usize];
        let Some(then) = current.then else {
            return;
        };
        self.age = self.age.saturating_add(1);
        let due = match current.wait {
            Wait::None => false,
            Wait::Ticks(n) => self.age >= n,
            Wait::ClipEnd => self.age >= self.playing.map_or(0, |r| self.clip_ticks[r as usize]),
        };
        if due {
            self.enter(then, out);
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    /// Idle, Hit, Activated, Opening, Shake, Finish, Opened clip lengths.
    const CLIPS: [u16; ROLES] = [0, 20, 0, 36, 0, 0, 0];

    fn run(i: &mut Instance, ticks: u16) -> Vec<(u16, Action)> {
        let mut seen = Vec::new();
        for t in 1..=ticks {
            let mut out = Actions::none();
            i.tick(&mut out);
            seen.extend(out.iter().map(|a| (t, a)));
        }
        seen
    }

    #[test]
    fn a_toll_switch_waits_a_second_after_the_hit_then_opens_its_gates() {
        let (mut switch, enter) = Instance::new(&TOLL_SWITCH, CLIPS, false);
        assert!(enter.contains(Action::Clip(Role::Idle)));
        let mut hit = Actions::none();
        switch.event(Event::NailHit, &mut hit);
        assert_eq!(
            hit.iter().collect::<Vec<_>>(),
            [Action::Persist, Action::Shake, Action::Clip(Role::Hit)]
        );
        // A second hit while it is in `Hit` changes nothing.
        let mut again = Actions::none();
        switch.event(Event::NailHit, &mut again);
        assert!(again.iter().next().is_none());
        let after = run(&mut switch, 80);
        assert_eq!(after, [(60, Action::Send(Event::Open))]);
        assert_eq!(switch.state(), "Open");
    }

    #[test]
    fn a_toll_gate_plays_its_clip_through_and_is_then_gone() {
        let (mut gate, _) = Instance::new(&TOLL_GATE, CLIPS, false);
        let mut out = Actions::none();
        gate.event(Event::Open, &mut out);
        assert_eq!(out.iter().collect::<Vec<_>>(), [Action::Clip(Role::Opening)]);
        // `Opening` plays 36 ticks of clip; the collider stays until it ends.
        let after = run(&mut gate, 50);
        assert_eq!(after, [(36, Action::ColliderOff), (36, Action::Destroy)]);
        assert_eq!(gate.state(), "Destroy Self");
    }

    #[test]
    fn a_switch_loaded_activated_destroys_its_gates_without_a_clip() {
        let (_, enter) = Instance::new(&TOLL_SWITCH, CLIPS, true);
        assert_eq!(
            enter.iter().collect::<Vec<_>>(),
            [Action::Clip(Role::Activated), Action::Send(Event::Destroy)]
        );
        let (mut gate, _) = Instance::new(&TOLL_GATE, CLIPS, false);
        let mut out = Actions::none();
        gate.event(Event::Destroy, &mut out);
        assert_eq!(out.iter().collect::<Vec<_>>(), [Action::ColliderOff, Action::Destroy]);
        let (_, restored) = Instance::new(&TOLL_GATE, CLIPS, true);
        assert!(restored.contains(Action::Destroy));
    }

    #[test]
    fn the_gate_switch_and_the_pillar_run_the_source_waits_end_to_end() {
        let (mut switch, _) = Instance::new(&SWITCH_CONTROL, CLIPS, false);
        let (mut pillar, _) = Instance::new(&PILLAR_CONTROL, CLIPS, false);
        let mut hit = Actions::none();
        switch.event(Event::NailHit, &mut hit);
        let mut sent = Vec::new();
        let mut log = Vec::new();
        for t in 1..=300u16 {
            let mut out = Actions::none();
            switch.tick(&mut out);
            for a in out.iter() {
                if let Action::Send(e) = a {
                    sent.push((t, e));
                    let mut p = Actions::none();
                    pillar.event(e, &mut p);
                    log.extend(p.iter().map(|a| (t, a)));
                }
            }
            let mut p = Actions::none();
            pillar.tick(&mut p);
            log.extend(p.iter().map(|a| (t, a)));
        }
        // 0.75 s to ACTIVATE; the pillar shakes 0.75 s, opens (36 ticks of clip,
        // collider off from its start) and finishes in 0.5 s.
        assert_eq!(sent, [(45, Event::Activate)]);
        assert_eq!(
            log,
            [
                (45, Action::Clip(Role::Shake)),
                (89, Action::Clip(Role::Opening)),
                (89, Action::ColliderOff),
                (125, Action::Clip(Role::Finish)),
                (155, Action::Clip(Role::Opened)),
                (155, Action::ColliderOff),
            ]
        );
        assert_eq!(pillar.state(), "Opened");
    }

    #[test]
    fn a_pillar_loaded_with_its_bool_set_starts_open_and_the_activated_switch_agrees() {
        let (pillar, enter) = Instance::new(&PILLAR_CONTROL, CLIPS, true);
        assert_eq!(pillar.state(), "Opened");
        assert!(enter.contains(Action::ColliderOff));
        let (mut switch, enter) = Instance::new(&SWITCH_CONTROL, CLIPS, true);
        assert!(enter.contains(Action::Send(Event::Activate)));
        // `Activated` ends into `Open`, which sends ACTIVATE a second time.
        let after = run(&mut switch, 5);
        assert_eq!(after, [(1, Action::Send(Event::Activate))]);
    }
}
