//! HKFSM03 family 10: particles and effects. The tier keeps no emitter objects: the action raises a request the
//! actor's adapter presents (or, until it has the art, does not).
use crate::fsm3::{r_stateless, Ctx, FX_PARTICLES_PLAY, FX_PARTICLES_STOP, R_CONT};
use hk_format::fsm3::ops::fx;

#[inline(never)]
pub fn entry(c: &mut Ctx<'_, '_>, op: u8) -> u16 {
    if op == fx::PLAY_PARTICLE_EMITTER {
        // PlayParticleEmitter [gameObject, emit]
        c.env().fx |= FX_PARTICLES_PLAY;
        r_stateless(c.flags)
    } else if op == fx::STOP_PARTICLE_EMITTER {
        // StopParticleEmitter [gameObject]
        c.env().fx |= FX_PARTICLES_STOP;
        r_stateless(c.flags)
    } else {
        R_CONT
    }
}
