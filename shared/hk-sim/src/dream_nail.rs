//! The Dream Nail, from the Hero's `Dream Nail` PlayMaker FSM.
//!
//! It is not another nail swing: it takes control outright, runs four phases
//! whose lengths are their tk2d clips rather than serialized times, and pays in
//! SOUL taken from the target rather than dealing damage. Setting and warping
//! to a Dream Gate is a separate branch of the same FSM and is not modelled.
use crate::{Player, ONE};

#[derive(Clone, Copy)]
pub struct DreamNailParams {
    pub start_ticks: u16,
    pub charge_ticks: u16,
    pub antic_ticks: u16,
    pub slash_ticks: u16,
    /// The slash becomes cancelable this far in; the hitbox stays live.
    pub cancelable_ticks: u16,
    /// `EnemyDreamnailReaction::RecieveDreamImpact` adds this much MP charge.
    pub soul: u16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DreamPhase {
    Off,
    Start,
    Charge,
    Antic,
    Slash,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DreamNail {
    pub phase: DreamPhase,
    pub tick: u16,
    was_button: bool,
    /// Nothing in the admitted scenes grants it; the Cheats row does.
    pub has_dream_nail: bool,
}
impl Default for DreamNail {
    fn default() -> Self {
        Self::new()
    }
}
impl DreamNail {
    pub const fn new() -> Self {
        Self {
            phase: DreamPhase::Off,
            tick: 0,
            was_button: false,
            has_dream_nail: false,
        }
    }
    /// Take Control runs from Start until End, so the Knight has no input for
    /// the whole sequence. Callers treat this like the Focus and bench locks.
    pub fn locks_control(&self) -> bool {
        self.phase != DreamPhase::Off
    }
    /// Slash activates the Hitbox on entry and End deactivates it, so it is
    /// live for exactly the Slash phase.
    pub fn hitting(&self) -> bool {
        self.phase == DreamPhase::Slash
    }
    /// One 60 Hz tick. `blocked` carries the CanDreamNail conditions the caller
    /// owns: a pause, a dialogue, a scripted sequence or a hazard respawn.
    pub fn tick(&mut self, p: DreamNailParams, button: bool, player: &Player, blocked: bool) {
        let pressed = button && !self.was_button;
        self.was_button = button;
        if self.phase == DreamPhase::Off {
            // CanDreamNail: the Dream Nail, both feet down and not falling, no
            // dash running and not inside an attack's recovery window.
            let can = self.has_dream_nail
                && !blocked
                && player.grounded
                && player.vy > -ONE / 10
                && player.dash_left == 0
                && !player.attack_recovering;
            if pressed && can {
                self.phase = DreamPhase::Start;
                self.tick = 0;
            }
            return;
        }
        self.tick += 1;
        // Charge watches the button: letting go there cancels the whole thing.
        if self.phase == DreamPhase::Charge && !button {
            self.phase = DreamPhase::Off;
            self.tick = 0;
            return;
        }
        let (length, next) = match self.phase {
            DreamPhase::Start => (p.start_ticks, DreamPhase::Charge),
            DreamPhase::Charge => (p.charge_ticks, DreamPhase::Antic),
            DreamPhase::Antic => (p.antic_ticks, DreamPhase::Slash),
            DreamPhase::Slash => (p.slash_ticks, DreamPhase::Off),
            DreamPhase::Off => unreachable!(),
        };
        if self.tick >= length {
            self.phase = next;
            self.tick = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const P: DreamNailParams = DreamNailParams {
        start_ticks: 20,
        charge_ticks: 35,
        antic_ticks: 25,
        slash_ticks: 33,
        cancelable_ticks: 18,
        soul: 33,
    };
    fn grounded() -> Player {
        let mut p = Player::spawn(0, 0);
        p.grounded = true;
        p
    }
    fn run(d: &mut DreamNail, p: &Player, button: bool, ticks: u16) {
        for _ in 0..ticks {
            d.tick(P, button, p, false);
        }
    }
    #[test]
    fn without_the_dream_nail_the_button_does_nothing() {
        let (mut d, p) = (DreamNail::new(), grounded());
        d.tick(P, true, &p, false);
        assert_eq!(d.phase, DreamPhase::Off);
    }
    #[test]
    fn it_refuses_to_start_in_the_air_or_while_falling() {
        let mut d = DreamNail::new();
        d.has_dream_nail = true;
        let mut p = grounded();
        p.grounded = false;
        d.tick(P, true, &p, false);
        assert_eq!(
            d.phase,
            DreamPhase::Off,
            "CanDreamNail wants both feet down"
        );
        p.grounded = true;
        p.vy = -ONE;
        d.was_button = false;
        d.tick(P, true, &p, false);
        assert_eq!(d.phase, DreamPhase::Off, "and a velocity above -0.1");
    }
    #[test]
    fn the_four_phases_run_to_their_clip_lengths_and_the_hitbox_is_the_slash() {
        let mut d = DreamNail::new();
        d.has_dream_nail = true;
        let p = grounded();
        d.tick(P, true, &p, false);
        assert_eq!(d.phase, DreamPhase::Start);
        assert!(d.locks_control());
        run(&mut d, &p, true, P.start_ticks);
        assert_eq!(d.phase, DreamPhase::Charge);
        run(&mut d, &p, true, P.charge_ticks);
        assert_eq!(d.phase, DreamPhase::Antic);
        assert!(!d.hitting(), "the hitbox is not live before the slash");
        run(&mut d, &p, true, P.antic_ticks);
        assert_eq!(d.phase, DreamPhase::Slash);
        assert!(d.hitting());
        run(&mut d, &p, true, P.slash_ticks);
        assert_eq!(d.phase, DreamPhase::Off);
        assert!(!d.locks_control());
    }
    #[test]
    fn letting_go_during_the_charge_cancels_it_but_not_later() {
        let mut d = DreamNail::new();
        d.has_dream_nail = true;
        let p = grounded();
        d.tick(P, true, &p, false);
        run(&mut d, &p, true, P.start_ticks);
        assert_eq!(d.phase, DreamPhase::Charge);
        d.tick(P, false, &p, false);
        assert_eq!(
            d.phase,
            DreamPhase::Off,
            "ListenForDreamNail cancels the charge"
        );
        // Past the charge the release is ignored: the swing is committed.
        d.was_button = false;
        d.tick(P, true, &p, false);
        run(&mut d, &p, true, P.start_ticks + P.charge_ticks);
        assert_eq!(d.phase, DreamPhase::Antic);
        run(&mut d, &p, false, P.antic_ticks);
        assert_eq!(d.phase, DreamPhase::Slash);
    }
}
