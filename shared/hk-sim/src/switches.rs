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
//!   activated, `Activated` plays the pose and sends ACTIVATE AGAIN, then runs `Open`, which sends ACTIVATE.
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
    /// A switch's `ACTIVATE AGAIN`: what a switch loaded already activated sends first.
    ActivateAgain,
    /// `DestroyObject` on a gate: gone without playing its clip.
    Destroy,
    /// The station's toll was paid (`Stag Bell`'s `Yes`).
    Paid,
    /// The bell stand finished appearing: the nail-struck `Bell` may ring.
    BellReady,
    /// `STAG BELL RUNG`: the stag is called to this station.
    Rung,
    /// The arrival animation played out (the stag steps off).
    Arrived,
    /// The stag was already here when the scene loaded.
    Present,
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
    /// `SetMeshRenderer` false: the art is gone, the collider is whatever it was.
    Hide,
    /// `SetMeshRenderer` true.
    Show,
    /// The stag is calling itself to this station: `stagPosition` becomes this one.
    Arrive,
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
    /// Events answered from every state (PlayMaker's global transitions).
    pub global: &'static [(Event, u8)],
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
    global: &[],
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
    global: &[],
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
        // The source's `Activated` sends ACTIVATE AGAIN and ends with FINISHED into `Open`,
        // which sends ACTIVATE.
        state(
            "Activated",
            &[Action::Clip(Role::Activated), Action::Send(Event::ActivateAgain)],
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
    global: &[],
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
    global: &[],
    states: &[
        state("Idle", &[Action::Clip(Role::Idle)], Wait::None, None, &[(Event::Activate, 1)]),
        // `SetBoolValue Activated`: the pillar's own PersistentBoolItem.
        state("Shake", &[Action::Persist, Action::Clip(Role::Shake)], Wait::Ticks(45), Some(2), &[]),
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

/// `Gate` (Fungus1_22's Metal Gate): ACTIVATE waits half a second, plays `Gate Open` with its collider
/// off and then hides the art; ACTIVATE AGAIN (a switch loaded activated) skips to the open pose.
pub static METAL_GATE: Machine = Machine {
    name: "Gate",
    start: 0,
    restored: 0,
    global: &[(Event::ActivateAgain, 4)],
    states: &[
        state("Idle", &[Action::Clip(Role::Idle)], Wait::None, None, &[(Event::Activate, 1)]),
        state("Open Pause", &[], Wait::Ticks(30), Some(2), &[]),
        state("Open", &[Action::Clip(Role::Opening), Action::ColliderOff], Wait::ClipEnd, Some(3), &[]),
        state("Opened", &[Action::Hide], Wait::None, None, &[]),
        state("Activate", &[Action::ColliderOff, Action::Clip(Role::Opened)], Wait::None, None, &[]),
    ],
};

/// `Grate Control` (the stag tunnel's grate): OPEN plays `Grate_disappear` and hides it; a station
/// that is open when the scene loads has no grate at all.
pub static STAG_GRATE: Machine = Machine {
    name: "Grate Control",
    start: 0,
    restored: 2,
    global: &[],
    states: &[
        state("Idle", &[Action::Clip(Role::Idle)], Wait::None, None, &[(Event::Open, 1)]),
        state("Open", &[Action::Clip(Role::Opening)], Wait::ClipEnd, Some(2), &[]),
        state("Opened", &[Action::Hide], Wait::None, None, &[]),
    ],
};

/// `Stag Bell` once its toll is paid (the yes/no box and the Geo are the stag module's): the box over
/// the bell falls away for 3.5 s, the grate opens, 2.3 s later the bell stand rises and the bell
/// may be rung. A station that is open on load starts with the stand up.
pub static STAG_BELL_BOX: Machine = Machine {
    name: "Stag Bell",
    start: 0,
    restored: 6,
    global: &[],
    states: &[
        state("Box", &[Action::Clip(Role::Idle)], Wait::None, None, &[(Event::Paid, 1)]),
        // `Pause Before Box Drop`.
        state("Pause Before Box Drop", &[], Wait::Ticks(30), Some(2), &[]),
        state("Box Disappear Anim", &[Action::Clip(Role::Opening)], Wait::Ticks(210), Some(3), &[]),
        state("Open Grate", &[Action::Send(Event::Open)], Wait::Ticks(1), Some(4), &[]),
        state("Bell Up Pause", &[Action::Hide], Wait::Ticks(138), Some(5), &[]),
        state("Bell Up", &[Action::Show, Action::Clip(Role::Shake)], Wait::ClipEnd, Some(6), &[]),
        state("Opened", &[Action::Clip(Role::Opened), Action::Send(Event::BellReady)], Wait::None, None, &[]),
    ],
};

/// `Bell Control` (the nail-struck bell): hidden until the stand is up, then each nail hit rings it for
/// 0.4 s and tells the stag.
pub static STAG_BELL: Machine = Machine {
    name: "Bell Control",
    start: 0,
    restored: 1,
    global: &[],
    states: &[
        state("Inactive", &[Action::Hide], Wait::None, None, &[(Event::BellReady, 1)]),
        state("Detect", &[Action::Show, Action::Clip(Role::Idle)], Wait::None, None, &[(Event::NailHit, 2)]),
        state("Ring", &[Action::Clip(Role::Opening), Action::Send(Event::Rung)], Wait::Ticks(24), Some(1), &[]),
    ],
};

/// The stag's body: away until the bell calls it, then it waits for its arrival animation to end and
/// stands idle (`Activate`); a stag that is here on load is idle at once.
pub static STAG_BODY: Machine = Machine {
    name: "Stag Control",
    start: 0,
    restored: 2,
    global: &[(Event::Present, 2)],
    states: &[
        state("Away", &[Action::Hide], Wait::None, None, &[(Event::Rung, 1)]),
        state("Arriving", &[Action::Arrive], Wait::None, None, &[(Event::Arrived, 2)]),
        state("Idle", &[Action::Show, Action::Clip(Role::Idle)], Wait::None, None, &[]),
    ],
};

/// The stag's `Arrival Anim` and `Arrival Dust` children: hidden, then 1 s after the bell rings they play
/// their clip once, shaking the camera, and report back.
pub static STAG_ARRIVAL: Machine = Machine {
    name: "Arrival",
    start: 0,
    restored: 3,
    global: &[],
    states: &[
        state("Hidden", &[Action::Hide], Wait::None, None, &[(Event::Rung, 1)]),
        state("Arrive Pause", &[], Wait::Ticks(60), Some(2), &[]),
        state("Arrive Anim", &[Action::Show, Action::Clip(Role::Opening), Action::Shake], Wait::ClipEnd, Some(3), &[]),
        state("Arrived", &[Action::Hide, Action::Send(Event::Arrived)], Wait::None, None, &[]),
    ],
};

/// A sprite that only plays its default clip (the bell's `Front`).
pub static STATIC_PROP: Machine = Machine {
    name: "Static",
    start: 0,
    restored: 0,
    global: &[],
    states: &[state("Idle", &[Action::Clip(Role::Idle)], Wait::None, None, &[])],
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
        let to = current
            .on
            .iter()
            .chain(self.machine.global)
            .find(|&&(e, _)| e == event)
            .map(|&(_, to)| to);
        if let Some(to) = to {
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
                (45, Action::Persist),
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
        assert!(enter.contains(Action::Send(Event::ActivateAgain)));
        // `Activated` ends into `Open`, which sends ACTIVATE.
        let after = run(&mut switch, 5);
        assert_eq!(after, [(1, Action::Send(Event::Activate))]);
    }

    #[test]
    fn a_metal_gate_waits_then_opens_and_is_hidden_when_its_clip_ends() {
        let (mut gate, _) = Instance::new(&METAL_GATE, CLIPS, false);
        let mut out = Actions::none();
        gate.event(Event::Activate, &mut out);
        assert!(out.iter().next().is_none());
        let log = run(&mut gate, 100);
        // Half a second, then `Gate Open` (36 ticks here) with the collider off from its start,
        // and the art is hidden once the clip has played.
        assert_eq!(
            log,
            [(30, Action::Clip(Role::Opening)), (30, Action::ColliderOff), (66, Action::Hide)]
        );
        assert_eq!(gate.state(), "Opened");
    }

    #[test]
    fn a_metal_gate_sent_activate_again_opens_at_once_and_keeps_its_art() {
        let (mut gate, _) = Instance::new(&METAL_GATE, CLIPS, false);
        let mut out = Actions::none();
        gate.event(Event::ActivateAgain, &mut out);
        assert_eq!(out.iter().collect::<Vec<_>>(), [Action::ColliderOff, Action::Clip(Role::Opened)]);
        // The ACTIVATE that follows from the switch's `Open` changes nothing.
        let mut again = Actions::none();
        gate.event(Event::Activate, &mut again);
        assert!(again.iter().next().is_none());
    }

    #[test]
    fn a_pillar_ignores_activate_again_and_persists_when_it_starts_to_shake() {
        let (mut pillar, _) = Instance::new(&PILLAR_CONTROL, CLIPS, false);
        let mut out = Actions::none();
        pillar.event(Event::ActivateAgain, &mut out);
        assert!(out.iter().next().is_none());
        pillar.event(Event::Activate, &mut out);
        assert_eq!(out.iter().collect::<Vec<_>>(), [Action::Persist, Action::Clip(Role::Shake)]);
    }

    #[test]
    fn a_paid_station_drops_its_box_opens_the_grate_and_raises_the_bell() {
        let clips = [0, 0, 0, 0, 12, 0, 0];
        let (mut bell, enter) = Instance::new(&STAG_BELL_BOX, clips, false);
        assert!(enter.contains(Action::Clip(Role::Idle)));
        let mut out = Actions::none();
        bell.event(Event::Paid, &mut out);
        assert!(out.iter().next().is_none());
        let log = {
            let mut seen = Vec::new();
            for t in 1..=500u16 {
                let mut out = Actions::none();
                bell.tick(&mut out);
                seen.extend(out.iter().map(|a| (t, a)));
            }
            seen
        };
        assert_eq!(
            log,
            [
                (30, Action::Clip(Role::Opening)),
                (240, Action::Send(Event::Open)),
                (241, Action::Hide),
                (379, Action::Show),
                (379, Action::Clip(Role::Shake)),
                (391, Action::Clip(Role::Opened)),
                (391, Action::Send(Event::BellReady)),
            ]
        );
        // A station that is open on load starts with the stand up and tells its bell.
        let (_, restored) = Instance::new(&STAG_BELL_BOX, clips, true);
        assert!(restored.contains(Action::Send(Event::BellReady)));
    }

    #[test]
    fn the_bell_rings_only_once_its_stand_is_up_and_calls_the_stag() {
        let (mut bell, enter) = Instance::new(&STAG_BELL, CLIPS, false);
        assert!(enter.contains(Action::Hide));
        let mut out = Actions::none();
        bell.event(Event::NailHit, &mut out);
        assert!(out.iter().next().is_none());
        bell.event(Event::BellReady, &mut out);
        assert!(out.contains(Action::Clip(Role::Idle)));
        let mut ring = Actions::none();
        bell.event(Event::NailHit, &mut ring);
        assert_eq!(ring.iter().collect::<Vec<_>>(), [Action::Clip(Role::Opening), Action::Send(Event::Rung)]);
        // A second hit while it rings changes nothing; after 0.4 s it is ready again.
        let mut again = Actions::none();
        bell.event(Event::NailHit, &mut again);
        assert!(again.iter().next().is_none());
        let after = run(&mut bell, 30);
        assert_eq!(after, [(24, Action::Show), (24, Action::Clip(Role::Idle))]);
    }

    #[test]
    fn the_stag_arrives_one_second_after_the_bell_and_stands_when_the_animation_ends() {
        let clips = [0, 0, 0, 20, 0, 0, 0];
        let (mut anim, _) = Instance::new(&STAG_ARRIVAL, clips, false);
        let (mut body, enter) = Instance::new(&STAG_BODY, CLIPS, false);
        assert!(enter.contains(Action::Hide));
        let mut out = Actions::none();
        anim.event(Event::Rung, &mut out);
        body.event(Event::Rung, &mut out);
        let mut log = Vec::new();
        for t in 1..=100u16 {
            let mut o = Actions::none();
            anim.tick(&mut o);
            for a in o.iter() {
                log.push((t, a));
                if a == Action::Send(Event::Arrived) {
                    let mut b = Actions::none();
                    body.event(Event::Arrived, &mut b);
                    log.extend(b.iter().map(|a| (t, a)));
                }
            }
        }
        assert_eq!(
            log,
            [
                (60, Action::Show),
                (60, Action::Clip(Role::Opening)),
                (60, Action::Shake),
                (80, Action::Hide),
                (80, Action::Send(Event::Arrived)),
                (80, Action::Show),
                (80, Action::Clip(Role::Idle)),
            ]
        );
        assert_eq!(body.state(), "Idle");
        // A stag that is here on load is idle at once, and a station that has one ignores the bell.
        let (mut here, _) = Instance::new(&STAG_BODY, CLIPS, false);
        let mut p = Actions::none();
        here.event(Event::Present, &mut p);
        assert_eq!(here.state(), "Idle");
        let (arrival, _) = Instance::new(&STAG_ARRIVAL, clips, true);
        assert_eq!(arrival.state(), "Arrived");
    }
}
