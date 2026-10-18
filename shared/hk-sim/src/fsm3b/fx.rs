//! Boss tier, family 10: particle emitters.
use crate::fsm3::{r_stateless, Ctx, R_CONT};
use crate::fsm3_world::k;
use hk_format::fsm3::ops::fx;

#[cfg_attr(target_arch = "mips", optimize(size))]
#[inline(never)]
pub fn entry(c: &mut Ctx<'_, '_>, op: u8) -> u16 {
    if op == fx::PLAY_PARTICLE_EMITTER {
        // PlayParticleEmitter [gameObject, emit]: emit > 0 is a burst of that many.
        let (t, emit) = (c.otarget(), c.int(c.skip()));
        c.world().request(k::PARTICLES, t, 1, [emit, 0, 0]);
        r_stateless(c.flags)
    } else if op == fx::STOP_PARTICLE_EMITTER {
        // StopParticleEmitter [gameObject]
        let t = c.otarget();
        c.world().request(k::PARTICLES, t, 0, [0; 3]);
        r_stateless(c.flags)
    } else {
        super::unknown(c, 10, op)
    }
}
