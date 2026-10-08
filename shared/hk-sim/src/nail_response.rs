//! HeroController/NailSlash no-charm recoil and normal pogo response.
//! Source fixed-step horizontal duration is resampled to the guest's 60 Hz.
use crate::{Params, Player};
#[derive(Clone, Copy, Debug)]
pub struct NailResponseParams {
    pub recoil_ticks: u16,
    pub recoil_speed: i32,
    pub bounce_ticks: u16,
    /// `HeroController.BounceHigh` starts its bounce timer at -0.03 s, so a
    /// `BigBouncer` pogo holds the bounce speed that much longer.
    pub high_bounce_ticks: u16,
    pub bounce_speed: i32,
    pub down_speed: i32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NailResponse {
    pub recoil_left: u16,
    pub recoil_direction: i32,
    pub bounce_left: u16,
    finish_bounce: bool,
}
impl NailResponse {
    pub const fn new() -> Self {
        Self {
            recoil_left: 0,
            recoil_direction: 0,
            bounce_left: 0,
            finish_bounce: false,
        }
    }
    /// Only call for an authored NailSlash-compatible target. Health damage is
    /// independent: OnTriggerStay also produces recoil while a target evades.
    pub fn contact(&mut self, p: NailResponseParams, kind: u16, facing: i32, player: &mut Player) {
        match kind {
            0 | 1 if self.recoil_left == 0 => {
                self.recoil_left = p.recoil_ticks;
                self.recoil_direction = -facing;
            }
            2 => {
                player.jumping = false;
                player.vy = player.vy.min(p.down_speed);
            }
            3 if self.bounce_left == 0 && !self.finish_bounce => {
                self.bounce_left = p.bounce_ticks;
            }
            _ => {}
        }
    }
    /// NailSlash's down-slash answer for a collider carrying `BigBouncer`
    /// (the Acid Flyer's body): `HeroController.BounceHigh`. It refuses while a
    /// bounce is running, as `Bounce` does, so whichever lands first wins.
    pub fn bounce_high(&mut self, p: NailResponseParams) {
        if self.bounce_left == 0 && !self.finish_bounce {
            self.bounce_left = p.high_bounce_ticks;
        }
    }
    /// Wrap normal movement only. Damage recoil uses its own path and clears
    /// this state. Gravity still runs after the source's per-tick bounce speed.
    pub fn step(
        &mut self,
        response: NailResponseParams,
        mut p: Params,
        player: &mut Player,
        dir: i32,
        jump: bool,
        count: usize,
        edge: impl Fn(usize) -> [i32; 4],
    ) {
        let face = if dir == 0 {
            player.facing
        } else {
            dir.signum()
        };
        let mut vx = dir.clamp(-1, 1) * p.speed;
        if self.recoil_left != 0 {
            self.recoil_left -= 1;
            vx = if self.recoil_direction < 0 {
                if vx > -response.recoil_speed {
                    -response.recoil_speed
                } else {
                    vx - response.recoil_speed
                }
            } else if vx < response.recoil_speed {
                response.recoil_speed
            } else {
                vx + response.recoil_speed
            };
        }
        if self.finish_bounce {
            self.finish_bounce = false;
            player.vy = 0; // HeroController.Update's CancelBounce expiry.
        }
        let bouncing = self.bounce_left != 0;
        if bouncing {
            self.bounce_left -= 1;
            self.finish_bounce = self.bounce_left == 0;
            player.vy = response.bounce_speed;
            player.jumping = false;
            player.was_jump = false;
            player.grounded = false;
        }
        p.speed = vx.abs();
        player.step(p, vx.signum(), jump && !bouncing, count, edge);
        player.facing = face;
        if bouncing {
            player.was_jump = jump;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ONE;
    const R: NailResponseParams = NailResponseParams {
        recoil_ticks: 11,
        recoil_speed: 15 * ONE / 4,
        bounce_ticks: 15,
        high_bounce_ticks: 17,
        bounce_speed: 12 * ONE,
        down_speed: 0,
    };
    fn p() -> Params {
        Params { speed: 8 * ONE, jump: 16 * ONE, gravity: 48 * ONE, fall: 20 * ONE, hold_ticks: 12, min_ticks: 5, half_width: ONE / 4, bottom: -ONE, ..Params::ZERO }
    }
    #[test]
    fn downslash_holds_source_velocity_then_stops_instead_of_ballistic_jump() {
        let mut response = NailResponse::new();
        let mut player = Player::spawn(0, 10 * ONE);
        player.vy = -10 * ONE;
        response.contact(R, 3, 1, &mut player);
        for tick in 0..15 {
            response.contact(R, 3, 1, &mut player); // staying on target cannot extend a bounce
            response.step(R, p(), &mut player, 0, false, 0, |_| [0; 4]);
            assert_eq!(player.vy, R.bounce_speed - p().gravity / 60, "tick {tick}");
        }
        response.step(R, p(), &mut player, 0, false, 0, |_| [0; 4]);
        assert_eq!(player.vy, -p().gravity / 60);
    }
    #[test]
    fn horizontal_recoil_opposes_hit_preserves_facing_and_has_bounded_duration() {
        let mut response = NailResponse::new();
        let mut player = Player::spawn(0, 10 * ONE);
        response.contact(R, 0, 1, &mut player);
        for _ in 0..11 {
            response.contact(R, 0, 1, &mut player);
            response.step(R, p(), &mut player, 1, false, 0, |_| [0; 4]);
            assert_eq!(player.facing, 1);
        }
        let x = player.x;
        response.step(R, p(), &mut player, 1, false, 0, |_| [0; 4]);
        assert!(player.x > x);
        assert_eq!(x, -(R.recoil_speed / 60) * 11);
    }
    #[test]
    fn upslash_clamps_rising_velocity_without_inventing_downward_kick() {
        let mut response = NailResponse::new();
        let mut player = Player::spawn(0, 0);
        player.vy = 10 * ONE;
        player.jumping = true;
        response.contact(R, 2, 1, &mut player);
        assert_eq!(player.vy, 0);
        assert!(!player.jumping);
        player.vy = -2 * ONE;
        response.contact(R, 2, 1, &mut player);
        assert_eq!(player.vy, -2 * ONE);
    }
}
