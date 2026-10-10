//! No-charm Spell Control focus subset, sourced from the Windows prefab FSM.
//! Durations are resampled to60Hz; SOUL drain retains sub-tick phase instead of
//! rounding each0.027s charge to two ticks. No spells, charms or reserve vessel.
use crate::{VitalParams, Vitals};
#[derive(Clone, Copy, Debug)]
pub struct FocusParams {
    pub hold_ticks: u16,
    pub start_ticks: u16,
    pub heal_ticks: u16,
    pub cancel_ticks: u16,
    pub finish_ticks: u16,
    pub first_grace_ticks: u16,
    pub repeat_grace_ticks: u16,
    pub drain_interval_us: u16,
    pub cost: u16,
    pub heal_amount: u16,
    pub attack_recovery_ticks: u16,
}
#[derive(Clone, Copy, Debug)]
pub struct FocusInput {
    pub held: bool,
    pub grounded: bool,
    pub can_start: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusClip {
    Focus,
    Get,
    End,
    GetOnce,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Idle,
    Hold,
    Start,
    Drain,
    Heal,
    Cancel,
    Finish,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FocusEvents {
    pub started: bool,
    pub completed: bool,
    pub healed: u16,
    pub drained: u16,
    pub refunded: u16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Focus {
    phase: Phase,
    age: u16,
    animation_age: u32,
    drain_phase: u32,
    spent: u16,
    start_soul: u16,
    refocusing: bool,
    was_held: bool,
    blocked: bool,
}
impl Default for Focus {
    fn default() -> Self {
        Self::new()
    }
}
impl Focus {
    pub const fn new() -> Self {
        Self {
            phase: Phase::Idle,
            age: 0,
            animation_age: 0,
            drain_phase: 0,
            spent: 0,
            start_soul: 0,
            refocusing: false,
            was_held: false,
            blocked: false,
        }
    }
    pub fn locks_control(&self) -> bool {
        matches!(
            self.phase,
            Phase::Start | Phase::Drain | Phase::Heal | Phase::Cancel | Phase::Finish
        )
    }
    /// The original's `Lines Anim` plays from the `Focus` state (the drain, and the heal
    /// between two drains) until a cancel or the finish.
    pub fn lines_active(&self) -> bool {
        matches!(self.phase, Phase::Drain | Phase::Heal)
    }
    pub fn animation(&self) -> Option<(FocusClip, u32)> {
        let clip = match self.phase {
            Phase::Start => FocusClip::Focus,
            Phase::Drain => {
                if self.refocusing {
                    FocusClip::Get
                } else {
                    FocusClip::Focus
                }
            }
            Phase::Heal => FocusClip::Get,
            Phase::Cancel => FocusClip::End,
            Phase::Finish => FocusClip::GetOnce,
            _ => return None,
        };
        Some((clip, self.animation_age))
    }
    /// HERO DAMAGED / scene interruption stops drain immediately, without the
    /// release/leave-ground grace refund. Held input cannot restart it silently.
    pub fn interrupt(&mut self) {
        self.phase = Phase::Idle;
        self.age = 0;
        self.animation_age = 0;
        self.drain_phase = 0;
        self.spent = 0;
        self.blocked = true;
    }
    fn start(&mut self) {
        self.phase = Phase::Start;
        self.age = 0;
        self.animation_age = 0;
        self.refocusing = false;
        self.spent = 0;
        self.drain_phase = 0;
    }
    fn drain(&mut self, soul: u16) {
        self.phase = Phase::Drain;
        self.age = 0;
        self.drain_phase = 0;
        self.spent = 0;
        self.start_soul = soul;
    }
    fn cancel(&mut self, p: FocusParams, v: &mut Vitals, e: &mut FocusEvents) {
        if self.phase == Phase::Drain {
            let grace = if self.refocusing {
                p.repeat_grace_ticks
            } else {
                p.first_grace_ticks
            };
            if self.age <= grace {
                e.refunded = self.start_soul.saturating_sub(v.soul);
                // Original SetMPCharge restores the exact cycle-start value.
                v.soul = self.start_soul;
            }
        }
        self.phase = Phase::Cancel;
        self.age = 0;
        self.animation_age = 0;
        self.drain_phase = 0;
    }
    /// One simulation tick. Caller supplies CanFocus's ground/control/nail
    /// recovery gate, suppresses normal movement/jump/nail while locked, and
    /// calls interrupt after any accepted damage or scene/reset event.
    pub fn step(
        &mut self,
        p: FocusParams,
        vp: VitalParams,
        input: FocusInput,
        v: &mut Vitals,
    ) -> FocusEvents {
        let mut e = FocusEvents::default();
        let pressed = input.held && !self.was_held;
        self.was_held = input.held;
        if !input.held {
            self.blocked = false;
        }
        if !v.can_control() {
            self.interrupt();
            return e;
        }
        if self.phase == Phase::Idle {
            if !pressed || self.blocked {
                return e;
            }
            self.phase = Phase::Hold;
            self.age = 0;
        }
        if matches!(self.phase, Phase::Start | Phase::Drain) && (!input.held || !input.grounded) {
            self.cancel(p, v, &mut e);
            return e;
        }
        self.age = self.age.saturating_add(1);
        self.animation_age = self.animation_age.saturating_add(1);
        match self.phase {
            Phase::Hold => {
                if !input.held {
                    self.phase = Phase::Idle;
                    return e;
                }
                if self.age >= p.hold_ticks {
                    if input.can_start && input.grounded && p.cost > 0 && v.soul >= p.cost {
                        // The original entry gate deliberately does not test HP.
                        self.start();
                        e.started = true;
                    } else {
                        self.phase = Phase::Idle;
                        self.blocked = true;
                    }
                }
            }
            Phase::Start => {
                if self.age >= p.start_ticks {
                    self.drain(v.soul);
                }
            }
            Phase::Drain => {
                // Units are microseconds*60: one60Hz tick adds1,000,000.
                // The verified no-charm interval exceeds one tick, so this
                // branch consumes at most one SOUL and cannot form a work loop.
                assert!(p.drain_interval_us >= 16667);
                self.drain_phase += 1_000_000;
                let period = u32::from(p.drain_interval_us) * 60;
                if self.drain_phase >= period {
                    self.drain_phase -= period;
                    let before = v.soul;
                    v.take_soul(1);
                    e.drained = before - v.soul;
                    self.spent += 1;
                    if self.spent >= p.cost {
                        let before = v.health;
                        v.heal(vp, p.heal_amount);
                        e.completed = true;
                        e.healed = v.health - before;
                        self.phase = Phase::Heal;
                        self.age = 0;
                        self.animation_age = 0;
                        self.refocusing = true;
                        self.drain_phase = 0;
                    }
                }
            }
            Phase::Heal => {
                if self.age >= p.heal_ticks {
                    if v.health >= vp.max_health || v.soul < p.cost {
                        self.phase = Phase::Finish;
                        self.age = 0;
                        self.animation_age = 0;
                        self.blocked = true;
                    } else {
                        self.drain(v.soul);
                        if !input.held || !input.grounded {
                            self.cancel(p, v, &mut e);
                        }
                    }
                }
            }
            Phase::Cancel => {
                if self.age >= p.cancel_ticks {
                    if input.held && input.grounded && input.can_start && v.soul >= p.cost {
                        self.start();
                        e.started = true;
                    } else {
                        self.phase = Phase::Idle;
                    }
                }
            }
            Phase::Finish => {
                if self.age >= p.finish_ticks {
                    self.phase = Phase::Idle;
                }
            }
            Phase::Idle => {}
        }
        e
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    const P: FocusParams = FocusParams {
        hold_ticks: 15,
        start_ticks: 15,
        heal_ticks: 12,
        cancel_ticks: 15,
        finish_ticks: 14,
        first_grace_ticks: 12,
        repeat_grace_ticks: 27,
        drain_interval_us: 27000,
        cost: 33,
        heal_amount: 1,
        attack_recovery_ticks: 6,
    };
    const V: VitalParams = VitalParams {
        max_health: 5,
        max_soul: 99,
        nail_damage: 5,
        soul_per_hit: 11,
        invulnerable_ticks: 79,
        hazard_invulnerable_ticks: 40,
        recoil_ticks: 12,
        freeze_ticks: 19,
        death_ticks: 171,
        recoil_speed: 15 * 65536,
    };
    const HELD: FocusInput = FocusInput {
        held: true,
        grounded: true,
        can_start: true,
    };
    fn tick(f: &mut Focus, v: &mut Vitals, n: usize) {
        for _ in 0..n {
            f.step(P, V, HELD, v);
        }
    }
    fn setup(health: u16, soul: u16) -> (Focus, Vitals) {
        let mut v = Vitals::new(V);
        v.health = health;
        v.soul = soul;
        (Focus::new(), v)
    }
    #[test]
    fn timed_progressive_drain_heals_on_33rd_charge_only() {
        let (mut f, mut v) = setup(3, 99);
        tick(&mut f, &mut v, 30);
        assert_eq!(v.soul, 99);
        tick(&mut f, &mut v, 1);
        assert_eq!(v.soul, 99);
        tick(&mut f, &mut v, 1);
        assert_eq!(v.soul, 98);
        tick(&mut f, &mut v, 51);
        assert_eq!((v.soul, v.health), (67, 3));
        let e = f.step(P, V, HELD, &mut v);
        assert!(e.completed);
        assert_eq!(e.healed, 1);
        assert_eq!((v.soul, v.health), (66, 4));
    }
    #[test]
    fn early_release_and_leaving_ground_refund_but_late_release_does_not() {
        for (age, refund) in [(12, true), (13, false)] {
            for ground_loss in [false, true] {
                let (mut f, mut v) = setup(3, 66);
                tick(&mut f, &mut v, 30 + age);
                let before = v.soul;
                let e = f.step(
                    P,
                    V,
                    FocusInput {
                        held: ground_loss,
                        grounded: !ground_loss,
                        ..HELD
                    },
                    &mut v,
                );
                assert_eq!(v.soul, if refund { 66 } else { before });
                assert_eq!(e.refunded > 0, refund);
                assert_eq!(v.health, 3);
                assert_eq!(f.animation(), Some((FocusClip::End, 0)));
            }
        }
    }
    #[test]
    fn damage_never_refunds_and_held_button_does_not_restart() {
        let (mut f, mut v) = setup(4, 66);
        tick(&mut f, &mut v, 35);
        let soul = v.soul;
        v.hurt(V, 1, 1, false);
        f.interrupt();
        assert!(!f.locks_control());
        assert_eq!(v.soul, soul);
        for _ in 0..200 {
            v.tick();
            f.step(P, V, HELD, &mut v);
        }
        assert!(!f.locks_control());
        assert_eq!(v.soul, soul);
        assert_eq!(v.health, 3);
    }
    #[test]
    fn held_repeat_has_longer_refund_grace_and_no_second_start_delay() {
        let (mut f, mut v) = setup(2, 99);
        tick(&mut f, &mut v, 84);
        assert_eq!((v.soul, v.health), (66, 3));
        tick(&mut f, &mut v, 12 + 27);
        assert!(v.soul < 66);
        let e = f.step(
            P,
            V,
            FocusInput {
                held: false,
                ..HELD
            },
            &mut v,
        );
        assert!(e.refunded > 0);
        assert_eq!(v.soul, 66);
        let (mut f, mut v) = setup(2, 99);
        tick(&mut f, &mut v, 150);
        assert_eq!((v.soul, v.health), (33, 4));
    }
    #[test]
    fn full_health_still_spends_one_cycle_then_stops_until_release() {
        let (mut f, mut v) = setup(5, 99);
        tick(&mut f, &mut v, 300);
        assert_eq!((v.soul, v.health), (66, 5));
        assert!(!f.locks_control());
        f.step(
            P,
            V,
            FocusInput {
                held: false,
                ..HELD
            },
            &mut v,
        );
        tick(&mut f, &mut v, 84);
        assert_eq!((v.soul, v.health), (33, 5));
    }
    #[test]
    fn insufficient_soul_airborne_and_unrecovered_attack_cannot_start() {
        for input in [
            FocusInput {
                grounded: false,
                ..HELD
            },
            FocusInput {
                can_start: false,
                ..HELD
            },
        ] {
            let (mut f, mut v) = setup(2, 99);
            for _ in 0..100 {
                f.step(P, V, input, &mut v);
            }
            assert!(!f.locks_control());
            assert_eq!(v.soul, 99);
        }
        let (mut f, mut v) = setup(2, 32);
        tick(&mut f, &mut v, 100);
        assert!(!f.locks_control());
        assert_eq!(v.soul, 32);
    }
    #[test]
    fn tap_and_start_cancel_cost_nothing_and_end_animation_releases_control() {
        let (mut f, mut v) = setup(3, 66);
        tick(&mut f, &mut v, 14);
        f.step(
            P,
            V,
            FocusInput {
                held: false,
                ..HELD
            },
            &mut v,
        );
        assert!(!f.locks_control());
        assert_eq!(v.soul, 66);
        tick(&mut f, &mut v, 16);
        assert!(f.locks_control());
        f.step(
            P,
            V,
            FocusInput {
                held: false,
                ..HELD
            },
            &mut v,
        );
        for _ in 0..15 {
            f.step(
                P,
                V,
                FocusInput {
                    held: false,
                    ..HELD
                },
                &mut v,
            );
        }
        assert!(!f.locks_control());
        assert_eq!(v.soul, 66);
    }
}
