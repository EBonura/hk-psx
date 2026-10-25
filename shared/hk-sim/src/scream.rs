//! Howling Wraiths and Abyss Shriek, from the Hero's `Spell Control` FSM (`Scream Antic1`,
//! `Scream Burst 1`, `End Roar`, `Scream End` and the level 2 twins) and the `Scr Heads` children of
//! the Knight prefab.
//!
//! Cast on a tap of the spell button with Up held at the press. The Knight stops where he is (in the
//! air too: gravity off, control off) for the `Scream Start` clip, the soul is paid, three heads (left,
//! right and up) hurt for most of the roar, and he is held through the roar and the two clips that end
//! it.
use crate::strike::{HitSpec, Strike};
use crate::Player;

/// Values the cook reads from the install (host/hk-cook ability_params.rs).
#[derive(Clone, Copy)]
pub struct ScreamParams {
    /// `Scream Start`'s clip.
    pub antic_ticks: u8,
    /// The roar's `Wait` (`Scream Burst`), `End Roar`'s wait and `Scream End`'s clip.
    pub roar_ticks: u8,
    pub end_roar_ticks: u8,
    pub end_ticks: u8,
    /// `Scr Heads` (level 1) and `Scr Heads 2` (level 2): left, right, up. Their windows count from the
    /// roar's first tick.
    pub heads: [[HitSpec; 3]; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreamPhase {
    Off,
    /// `Scream Start`.
    Antic,
    /// `Scream Burst`: the roar and its heads.
    Roar,
    /// `End Roar` and `Scream End`.
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scream {
    pub phase: ScreamPhase,
    /// Ticks into the phase.
    pub tick: u16,
    /// Ticks since the roar began (the heads' clock), for as long as it runs.
    pub roar: u16,
    /// 1 for Howling Wraiths, 2 for Abyss Shriek.
    pub level: u8,
}
impl Default for Scream {
    fn default() -> Self {
        Self::new()
    }
}
impl Scream {
    pub const fn new() -> Self {
        Self { phase: ScreamPhase::Off, tick: 0, roar: 0, level: 0 }
    }
    pub fn locks_control(&self) -> bool {
        self.phase != ScreamPhase::Off
    }
    pub fn begin(&mut self, level: u8) {
        *self = Self { phase: ScreamPhase::Antic, tick: 0, roar: 0, level };
    }
    pub fn cancel(&mut self) {
        *self = Self::new();
    }
    /// One tick, after the Knight has been held (gravity off and velocity zero are the caller's, for
    /// as long as `locks_control`). Returns true on the tick the roar begins (the soul is paid, the wave
    /// and the sound start).
    pub fn tick(&mut self, p: &ScreamParams) -> bool {
        match self.phase {
            ScreamPhase::Off => false,
            ScreamPhase::Antic => {
                self.tick += 1;
                if self.tick >= u16::from(p.antic_ticks) {
                    *self = Self { phase: ScreamPhase::Roar, tick: 0, roar: 0, level: self.level };
                    return true;
                }
                false
            }
            ScreamPhase::Roar => {
                self.tick += 1;
                self.roar += 1;
                if self.tick >= u16::from(p.roar_ticks) {
                    self.phase = ScreamPhase::End;
                    self.tick = 0;
                }
                false
            }
            ScreamPhase::End => {
                self.tick += 1;
                self.roar += 1;
                if self.tick >= u16::from(p.end_roar_ticks) + u16::from(p.end_ticks) {
                    self.cancel();
                }
                false
            }
        }
    }
    /// The heads that hurt this tick, written to `out`: how many.
    pub fn strikes(&self, p: &ScreamParams, player: &Player, out: &mut [Strike]) -> usize {
        if !matches!(self.phase, ScreamPhase::Roar | ScreamPhase::End) {
            return 0;
        }
        let mut n = 0;
        for h in &p.heads[usize::from(self.level.clamp(1, 2)) - 1] {
            if h.live(self.roar) && n < out.len() {
                out[n] = h.place(player.x, player.y, player.facing);
                n += 1;
            }
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const HEAD: HitSpec = HitSpec {
        points: [[-256, 0], [256, 0], [256, 256], [-256, 256], [0; 2]],
        len: 4,
        damage: 13,
        start: 3,
        end: 27,
    };
    const P: ScreamParams = ScreamParams {
        antic_ticks: 6,
        roar_ticks: 18,
        end_roar_ticks: 9,
        end_ticks: 3,
        heads: [[HEAD; 3], [HitSpec { damage: 20, ..HEAD }; 3]],
    };
    #[test]
    fn the_roar_follows_the_antic_and_lasts_the_clips_it_is_made_of() {
        let mut s = Scream::new();
        s.begin(1);
        assert!(s.locks_control());
        let mut roared = None;
        let mut ticks = 0;
        while s.locks_control() {
            if s.tick(&P) {
                roared = Some(ticks);
            }
            ticks += 1;
        }
        assert_eq!(roared, Some(P.antic_ticks - 1));
        assert_eq!(ticks, P.antic_ticks + P.roar_ticks + P.end_roar_ticks + P.end_ticks);
    }
    #[test]
    fn the_heads_hurt_between_their_triggers() {
        let (mut s, k) = (Scream::new(), Player::spawn(0, 0));
        s.begin(2);
        let mut out = [Strike::NONE; 4];
        let mut live = 0;
        for _ in 0..60 {
            if s.strikes(&P, &k, &mut out) == 3 {
                live += 1;
                assert_eq!(out[0].damage, 20, "Abyss Shriek's heads");
            }
            s.tick(&P);
        }
        assert_eq!(live, 24, "from the third to the 27th tick of the roar");
    }
}
