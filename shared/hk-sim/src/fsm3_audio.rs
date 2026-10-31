//! HKFSM03 family 9: audio.
use crate::fsm3::{r_stateless, Ctx, R_CONT, SOUND_SIMPLE, SOUND_STARTLE};
use hk_format::fsm3::ops::audio;

#[inline(never)]
pub fn entry(c: &mut Ctx<'_, '_>, op: u8) -> u16 {
    if op == audio::AUDIO_PLAYER_ONE_SHOT {
        // AudioPlayerOneShot: the Vengefly's startle cry is its only user.
        c.env().sound |= SOUND_STARTLE;
        r_stateless(c.flags)
    } else if op == audio::AUDIO_PLAY_SIMPLE {
        // AudioPlaySimple [gameObject, volume, oneShotClip]: a one-shot on the actor's own source.
        c.env().sound |= SOUND_SIMPLE;
        r_stateless(c.flags)
    } else if op == audio::SET_AUDIO_PITCH {
        // SetAudioPitch [gameObject, pitch]
        let pitch = c.fixed(c.skip());
        c.env().pitch = pitch;
        r_stateless(c.flags)
    } else {
        R_CONT
    }
}
