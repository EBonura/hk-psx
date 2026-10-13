//! One concrete Vengefly action adapter; no virtual game-service callbacks.
use crate::{fsm::Error, fsm_chase::Environment, vengefly_fsm::State, ONE};
#[derive(Clone, Copy)]
pub struct Sound {
    pub variant: u8,
    pub pitch: i32,
    pub volume: i32,
}
pub struct Chasing<'a, 'b> {
    pub state: &'a mut State,
    pub room: &'a hk_format::Room<'b>,
    pub clips: [u16; 5],
    pub position: [i32; 2],
    pub hero: [i32; 2],
    pub facing: i32,
    pub can_see: bool,
    pub in_range: bool,
    pub sound: Option<Sound>,
    pub direction_messages: u8,
}
fn owner(role: i32) -> Result<(), Error> {
    if role == 1 {
        Ok(())
    } else {
        Err(Error::Parameter)
    }
}
fn range(rng: &mut u32, lo: i32, hi: i32) -> i32 {
    *rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
    lo + (((*rng >> 8) as i64 * (hi - lo) as i64) >> 24) as i32
}
impl Environment for Chasing<'_, '_> {
    #[inline(never)]
    fn tick(&self) -> u32 {
        self.state.animation_tick
    }
    #[inline(never)]
    fn elapsed_ticks(&self, real: bool) -> Result<u16, Error> {
        if real {
            Err(Error::UnsupportedAction)
        } else {
            Ok(1)
        }
    }
    #[inline(never)]
    fn can_see_hero(&self) -> Result<bool, Error> {
        Ok(self.can_see)
    }
    #[inline(never)]
    fn in_alert_range(&self, role: i32) -> Result<bool, Error> {
        if role == 2 {
            Ok(self.in_range)
        } else {
            Err(Error::Parameter)
        }
    }
    #[inline(never)]
    fn find_alert_range(&self, role: i32) -> Result<i32, Error> {
        owner(role)?;
        Ok(2)
    }
    #[inline(never)]
    fn velocity(&self, role: i32, space: i32) -> Result<[i32; 2], Error> {
        owner(role)?;
        if space != 0 && space != 1 {
            return Err(Error::Parameter);
        }
        Ok(self.state.velocity)
    }
    #[inline(never)]
    fn set_velocity(&mut self, role: i32, v: [Option<i32>; 2]) -> Result<(), Error> {
        owner(role)?;
        for (axis, value) in v.into_iter().enumerate() {
            if let Some(value) = value {
                self.state.velocity[axis] = value;
            }
        }
        Ok(())
    }
    #[inline(never)]
    fn add_force(&mut self, role: i32, force: [i32; 2], limits: [i32; 3]) -> Result<(), Error> {
        owner(role)?;
        self.state.queue_force_and_clamp(force, limits)
    }
    #[inline(never)]
    fn decelerate(&mut self, role: i32, amount: i32) -> Result<(), Error> {
        owner(role)?;
        self.state.decelerate(amount)
    }
    #[inline(never)]
    fn set_rotation_z(&mut self, role: i32, angle: i32, space: i32) -> Result<(), Error> {
        owner(role)?;
        if angle == 0 && space == 0 {
            Ok(())
        } else {
            Err(Error::UnsupportedAction)
        }
    }
    #[inline(never)]
    fn idle_buzz(
        &mut self,
        role: i32,
        p: [i32; 5],
        enter: bool,
        rng: &mut u32,
    ) -> Result<(), Error> {
        owner(role)?;
        if p != [
            crate::buzz::WAIT_MIN,
            crate::buzz::WAIT_MAX,
            crate::buzz::IDLE_SPEED_MAX,
            crate::buzz::IDLE_ACCELERATION_MAX,
            crate::buzz::ROAMING_RANGE,
        ] {
            return Err(Error::UnsupportedAction);
        }
        if enter {
            self.state.buzz.enter(self.position);
        }
        let dt = if enter {
            ONE / 60
        } else {
            (crate::fsm_chase::FixedClock::FIXED_UNITS as u64 * ONE as u64
                / crate::fsm_chase::FixedClock::SOURCE_RATE as u64) as i32
        };
        self.state.buzz.step_timed(
            self.position,
            &mut self.state.velocity,
            p[2],
            p[3],
            dt,
            &mut |lo, hi| range(rng, lo, hi),
        );
        Ok(())
    }
    #[inline(never)]
    fn chase(
        &mut self,
        role: i32,
        target: i32,
        speed: i32,
        accel: i32,
        _: bool,
    ) -> Result<(), Error> {
        owner(role)?;
        if target != 3 {
            return Err(Error::Parameter);
        }
        for axis in 0..2 {
            let delta = if self.hero[axis] > self.position[axis] {
                accel
            } else {
                -accel
            };
            self.state.velocity[axis] = self.state.velocity[axis]
                .checked_add(delta)
                .ok_or(Error::Parameter)?;
        }
        crate::buzz::clamp(&mut self.state.velocity, speed);
        Ok(())
    }
    #[inline(never)]
    fn play_animation(&mut self, role: i32, clip: i32) -> Result<(), Error> {
        owner(role)?;
        if !(0..=4).contains(&clip) {
            return Err(Error::Parameter);
        }
        self.state.clip = clip as u8;
        self.state.animation_tick = 0;
        Ok(())
    }
    #[inline(never)]
    fn play_frame(&mut self, role: i32, frame: i32) -> Result<(), Error> {
        owner(role)?;
        let clip = self
            .room
            .clip(self.clips[self.state.clip as usize] as usize);
        if frame < 0 || frame as u32 >= clip[1] || clip[2] == 0 {
            return Err(Error::Parameter);
        }
        self.state.animation_tick = (frame as u64 * 60 * 65536 / clip[2] as u64) as u32;
        Ok(())
    }
    #[inline(never)]
    fn animation_complete(&self, role: i32) -> Result<bool, Error> {
        owner(role)?;
        let clip = self
            .room
            .clip(self.clips[self.state.clip as usize] as usize);
        Ok(clip[3] & 65535 == 2
            && self.state.animation_tick as u64 * clip[2] as u64 >= clip[1] as u64 * 60 * 65536)
    }
    #[inline(never)]
    fn face_direction(
        &mut self,
        role: i32,
        right: bool,
        clip: Option<i32>,
        pause: Option<i32>,
    ) -> Result<(), Error> {
        owner(role)?;
        if right {
            return Err(Error::UnsupportedAction);
        }
        if self.state.face_pause != 0 {
            return Ok(());
        }
        let direction = self.state.velocity[0].signum();
        if direction != 0 && direction != self.facing {
            self.facing = direction;
            if let Some(clip) = clip {
                self.play_animation(role, clip)?;
            }
            if let Some(pause) = pause {
                self.state.face_pause = u16::try_from((pause as i64 * 60 + 65535) / 65536)
                    .map_err(|_| Error::Parameter)?;
            }
        }
        Ok(())
    }
    #[inline(never)]
    fn face_object(
        &mut self,
        roles: [i32; 2],
        right: bool,
        clip: Option<i32>,
        reset: bool,
    ) -> Result<(), Error> {
        if roles != [1, 3] || right {
            return Err(Error::UnsupportedAction);
        }
        let direction = (self.hero[0] - self.position[0]).signum();
        if direction != 0 {
            self.facing = direction;
        }
        if let Some(clip) = clip {
            self.play_animation(1, clip)?;
            if reset {
                self.state.animation_tick = 0;
            }
        }
        Ok(())
    }
    #[inline(never)]
    fn set_audio_pitch(&mut self, role: i32, pitch: i32) -> Result<(), Error> {
        owner(role)?;
        self.state.audio_pitch = pitch;
        Ok(())
    }
    #[inline(never)]
    fn send_direction_message(&mut self) -> Result<(), Error> {
        self.direction_messages = self.direction_messages.saturating_add(1);
        Ok(())
    }
    #[inline(never)]
    fn audio_one_shot(
        &mut self,
        role: i32,
        weights: [i32; 3],
        pitch: [i32; 2],
        volume: i32,
        rng: &mut u32,
    ) -> Result<(), Error> {
        owner(role)?;
        let total = weights
            .iter()
            .try_fold(0i32, |total, w| total.checked_add(*w))
            .ok_or(Error::Parameter)?;
        let value = range(rng, 0, total);
        let mut threshold = 0;
        let mut variant = 2;
        for (i, w) in weights.iter().enumerate() {
            threshold += w;
            if value < threshold {
                variant = i as u8;
                break;
            }
        }
        self.sound = Some(Sound {
            variant,
            pitch: range(rng, pitch[0], pitch[1]),
            volume,
        });
        Ok(())
    }
}
