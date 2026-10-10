//! Source-derived Acid Flyer (Duranda, Greenpath's Fungus1_09).
//!
//! Every constant is read back out of the serialized FSMs by host/gruzzer.py's
//! Acid Flyer recognizer, which refuses a placement whose numbers moved.
//!
//! The source has no AI: it is a pogo platform over the acid.
//!
//! * `Tween` (one per placement, with its own `Move Vector` m and `Speed` s):
//!   wait 0.5 s, then iTweenMoveBy m in world space at speed s (so |m|/s
//!   seconds), easeInOutSine, then -m the same way, then SetPosition back to
//!   where it started, and again. The transform moves; the Rigidbody2D follows.
//! * `Acid Flyer`: `Idle` plays `Fly` once and runs FaceObject at the hero
//!   every frame, playing `TurnToFly` from its first frame on each flip (the
//!   art faces left; strictly left of the hero faces right).
//!
//! Three placements carry a second `Tween` FSM whose `Init` has no Wait. It
//! starts first, and the standard one's MoveBy disposes it half a second
//! later (iTween.ConflictCheck), so the flyer follows the standard tween from
//! wherever the lead left it until the first `Reset Pos` snaps it back.
//!
//! The caller owns the HealthManager (30 hp, invincible from up and down,
//! `Spell Vulnerable`), the two contact boxes (the body, which mirrors, and
//! the detached `Shell`, which does not), the high pogo off the body box, the
//! corpse and every draw. This type owns where the body is and which clip.
use crate::ONE;

/// `Tween` Init's Wait 0.5 s.
pub const START_WAIT_TICKS: u16 = 30;
pub const HEALTH: i16 = 30;
/// `HealthManager.Invincible`: a blocked hit ignores further hits for 0.15 s
/// (`evasionByHitRemaining`).
pub const BLOCK_EVASION_TICKS: u16 = 9;
pub const CONTACT_DAMAGE: u16 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Clip {
    Fly,
    TurnToFly,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Wait,
    Up,
    Down,
    Dead,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AcidFlyer {
    /// `Move Vector` y, Q16 units.
    amount: i32,
    /// Ticks per half cycle: |m| / s at 60 Hz, at least one.
    half_ticks: u16,
    /// The disposed lead tween's `Move Vector` y and half cycle (0: none).
    lead_amount: i32,
    lead_ticks: u16,
    /// Where the lead left the body, carried until the first `Reset Pos`.
    bias: i32,
    tick: u16,
    phase: Phase,
    facing_right: bool,
    clip: Clip,
}
/// sin(pi/2 * i/64), Q16, i = 0..=64: the quarter wave easeInOutSine needs.
const QUARTER: [u32; 65] = [
    0, 1608, 3216, 4821, 6424, 8022, 9616, 11204, 12785, 14359, 15924, 17479, 19024, 20557, 22078,
    23586, 25080, 26558, 28020, 29466, 30893, 32303, 33692, 35062, 36410, 37736, 39040, 40320,
    41576, 42806, 44011, 45190, 46341, 47464, 48559, 49624, 50660, 51665, 52639, 53581, 54491,
    55368, 56212, 57022, 57798, 58538, 59244, 59914, 60547, 61145, 61705, 62228, 62714, 63162,
    63572, 63944, 64277, 64571, 64827, 65043, 65220, 65358, 65457, 65516, 65536,
];
/// easeInOutSine as iTween computes it, for p in [0, ONE]: (1 - cos(pi p)) / 2,
/// which is sin^2(pi p / 2).
pub fn ease_in_out_sine(p: i32) -> i32 {
    let p = p.clamp(0, ONE) as u32;
    let at = p * 64;
    let (i, frac) = ((at >> 16) as usize, at & 0xffff);
    let s = if i >= 64 {
        QUARTER[64]
    } else {
        QUARTER[i] + (((QUARTER[i + 1] - QUARTER[i]) * frac) >> 16)
    };
    ((s as u64 * s as u64) >> 16) as i32
}
/// iTween's speed mode: the move takes |amount| / speed seconds.
fn half_ticks(amount: i32, speed: i32) -> u16 {
    assert!(speed > 0, "Acid Flyer speed must be positive");
    ((amount.unsigned_abs() as u64 * 60 + speed as u64 / 2) / speed as u64)
        .clamp(1, u16::MAX as u64) as u16
}
fn eased(amount: i32, tick: u16, ticks: u16) -> i32 {
    let p = (tick.min(ticks) as i64 * ONE as i64 / ticks as i64) as i32;
    ((amount as i64 * ease_in_out_sine(p) as i64) >> 16) as i32
}
impl AcidFlyer {
    /// `amount` is the placement's `Move Vector` y and `speed` its `Speed`,
    /// both Q16; the half cycle is their quotient in ticks.
    pub fn new(amount: i32, speed: i32) -> Self {
        Self::with_lead(amount, speed, [0, 0])
    }
    /// `lead` is the second, Wait-less `Tween` FSM's `Move Vector` y and
    /// `Speed` (speed 0: the placement has none).
    pub fn with_lead(amount: i32, speed: i32, lead: [i32; 2]) -> Self {
        let lead_ticks = if lead[1] > 0 {
            half_ticks(lead[0], lead[1])
        } else {
            0
        };
        Self {
            amount,
            half_ticks: half_ticks(amount, speed),
            lead_amount: lead[0],
            lead_ticks,
            bias: 0,
            tick: 0,
            phase: Phase::Wait,
            facing_right: false,
            clip: Clip::Fly,
        }
    }
    pub fn half_ticks(&self) -> u16 {
        self.half_ticks
    }
    pub fn facing_right(&self) -> bool {
        self.facing_right
    }
    pub fn clip(&self) -> Clip {
        self.clip
    }
    pub fn dead(&self) -> bool {
        self.phase == Phase::Dead
    }
    pub fn die(&mut self) {
        self.phase = Phase::Dead;
    }
    /// The tween's displacement from the authored position, Q16.
    pub fn offset(&self) -> i32 {
        match self.phase {
            Phase::Up => self.bias + eased(self.amount, self.tick, self.half_ticks),
            Phase::Down => self.bias + self.amount - eased(self.amount, self.tick, self.half_ticks),
            Phase::Wait if self.lead_ticks != 0 => {
                eased(self.lead_amount, self.tick, self.lead_ticks)
            }
            Phase::Wait | Phase::Dead => 0,
        }
    }
    /// One 60 Hz step: the tween, then FaceObject. Returns the clip to start
    /// from its first frame, if one starts.
    pub fn tick(&mut self, self_x: i32, hero_x: i32) -> Option<Clip> {
        if self.phase == Phase::Dead {
            return None;
        }
        self.tick += 1;
        match self.phase {
            Phase::Wait if self.tick >= START_WAIT_TICKS => {
                self.bias = self.offset();
                self.phase = Phase::Up;
                self.tick = 0;
            }
            Phase::Up if self.tick >= self.half_ticks => {
                self.phase = Phase::Down;
                self.tick = 0;
            }
            // `Reset Pos` snaps to the start; `Tween Up` begins again.
            Phase::Down if self.tick >= self.half_ticks => {
                self.phase = Phase::Up;
                self.tick = 0;
                self.bias = 0;
            }
            _ => {}
        }
        let right = self_x < hero_x;
        if right != self.facing_right {
            self.facing_right = right;
            self.clip = Clip::TurnToFly;
            return Some(Clip::TurnToFly);
        }
        None
    }
}
/// `invincibleFromDirection` 7: the nail is refused when it travels up or down
/// (cardinals 1 and 3); a spell never is (`Spell Vulnerable`).
pub fn blocks(cardinal: u8, spell: bool) -> bool {
    !spell && (cardinal == 1 || cardinal == 3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ease_is_the_sine_curve_at_its_ends_and_middle() {
        assert_eq!(ease_in_out_sine(0), 0);
        assert_eq!(ease_in_out_sine(ONE), ONE);
        assert!((ease_in_out_sine(ONE / 2) - ONE / 2).abs() <= 2);
        assert!(
            (ease_in_out_sine(ONE / 4) - 9598).abs() <= 8,
            "{}",
            ease_in_out_sine(ONE / 4)
        );
    }

    #[test]
    fn it_waits_then_bobs_up_and_back_and_snaps_to_its_start() {
        // Acid Flyer (4): m -2, s 4: half a second each way.
        let mut fly = AcidFlyer::new(-2 * ONE, 4 * ONE);
        assert_eq!(fly.half_ticks(), 30);
        for _ in 0..START_WAIT_TICKS {
            fly.tick(0, 10 * ONE);
            assert_eq!(fly.offset(), 0);
        }
        let mut lowest = 0;
        for _ in 0..30 {
            fly.tick(0, 10 * ONE);
            lowest = lowest.min(fly.offset());
        }
        assert_eq!(lowest, -2 * ONE);
        for _ in 0..30 {
            fly.tick(0, 10 * ONE);
        }
        assert_eq!(fly.offset(), 0, "back at the start after a full cycle");
    }

    #[test]
    fn a_disposed_lead_tween_offsets_the_first_cycle_only() {
        // Acid Flyer (1): A +8.5 at 4.0, B +9.5 at 5.25. B runs alone for the
        // half second A waits, which leaves the body ~1.68 up.
        let mut fly = AcidFlyer::with_lead(
            8 * ONE + ONE / 2,
            4 * ONE,
            [9 * ONE + ONE / 2, 5 * ONE + ONE / 4],
        );
        for _ in 0..START_WAIT_TICKS {
            fly.tick(0, ONE);
        }
        // The half cycle rounds to whole ticks (108.6 -> 109), hence the slack.
        assert!((fly.offset() - 110100).abs() < 1500, "{}", fly.offset());
        let bias = fly.offset();
        for _ in 0..fly.half_ticks() {
            fly.tick(0, ONE);
        }
        assert_eq!(fly.offset(), bias + 8 * ONE + ONE / 2);
        for _ in 0..fly.half_ticks() {
            fly.tick(0, ONE);
        }
        assert_eq!(fly.offset(), 0, "Reset Pos snaps to where Init found it");
        for _ in 0..fly.half_ticks() {
            fly.tick(0, ONE);
        }
        assert_eq!(fly.offset(), 8 * ONE + ONE / 2);
    }

    #[test]
    fn it_turns_to_the_hero_with_the_turn_clip() {
        let mut fly = AcidFlyer::new(ONE, 4 * ONE);
        assert_eq!(fly.tick(0, -ONE), None, "authored facing left, hero left");
        assert_eq!(fly.tick(0, ONE), Some(Clip::TurnToFly));
        assert!(fly.facing_right());
        assert_eq!(fly.tick(0, ONE), None);
        assert_eq!(fly.tick(0, 0), Some(Clip::TurnToFly), "equal x faces left");
    }

    #[test]
    fn only_side_nail_hits_and_spells_get_through() {
        assert!(!blocks(0, false) && !blocks(2, false));
        assert!(blocks(1, false) && blocks(3, false));
        assert!(!blocks(1, true) && !blocks(3, true));
    }
}
