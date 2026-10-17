//! Boss tier, family 9: audio. A sound is a request; the clip is the cooker's id for the original's AudioClip.
use crate::fsm3::{r_stateless, Ctx, R_CONT};
use crate::fsm3_world::k;
use hk_format::fsm3::{ops::audio, NONE8};
use psx_math::int32::mul_shr_i32;

const ONE: i32 = crate::ONE;

#[inline(never)]
pub fn entry(c: &mut Ctx<'_, '_>, op: u8) -> u16 {
    if op == audio::AUDIO_PLAY_SIMPLE {
        // AudioPlaySimple [gameObject, volume, oneShotClip]
        let b = c.skip();
        let (vol, clip, t) = (c.fixed(b), c.int(b + 1), c.otarget());
        c.world().request(k::AUDIO, t, 0, [clip, vol, ONE]);
    } else if op == audio::AUDIO_PLAYER_ONE_SHOT {
        // AudioPlayerOneShot [audioPlayer, spawnPoint, audioClips[], weights[], pitchMin, pitchMax, volume, delay, store]
        let (n, clips) = c.array(2);
        let (_, weights) = c.array(3);
        let pick = super::flow::weighted_index(c, weights, n).unwrap_or(0);
        let clip = c.value_of(c.array_word(clips, pick), false);
        one_shot(c, clip, 4);
    } else if op == audio::AUDIO_PLAYER_ONE_SHOT_SINGLE {
        // AudioPlayerOneShotSingle [audioPlayer, spawnPoint, audioClip, pitchMin, pitchMax, volume, delay, store]
        let clip = c.int(2);
        one_shot(c, clip, 3);
    } else if op == audio::AUDIO_PLAY_RANDOM {
        // AudioPlayRandom [gameObject, audioClips[], weights[], pitchMin, pitchMax]
        let (n, clips) = c.array(1);
        let (_, weights) = c.array(2);
        let pick = super::flow::weighted_index(c, weights, n).unwrap_or(0);
        let clip = c.value_of(c.array_word(clips, pick), false);
        let t = c.otarget();
        let (lo, hi) = (c.fixed(3), c.fixed(4));
        let pitch = lo + mul_shr_i32(hi - lo, (c.rand() >> 8) as i32, 24);
        c.world().request(k::AUDIO, t, 0, [clip, ONE, pitch]);
    } else if op == audio::AUDIO_PLAY_IN_STATE {
        // AudioPlayInState [gameObject, volume]: the object's own source plays while the state lasts.
        let t = c.otarget();
        let vol = c.fixed(c.skip());
        c.world().request(k::AUDIO, t, 2, [-1, vol, ONE]);
    } else if op == audio::AUDIO_STOP {
        // AudioStop [gameObject]
        let t = c.otarget();
        c.world().request(k::AUDIO_STOP, t, 0, [0; 3]);
    } else if op == audio::APPLY_MUSIC_CUE {
        // ApplyMusicCue [musicCue, delayTime, transitionTime]
        let (cue, delay, time) = (c.int(0), c.fixed(1), c.fixed(2));
        c.world().request(k::MUSIC, NONE8, 0, [cue, delay, time]);
    } else if op == audio::TRANSITION_TO_AUDIO_SNAPSHOT {
        // TransitionToAudioSnapshot [snapshot, transitionTime]
        let (snap, time) = (c.int(0), c.fixed(1));
        c.world().request(k::SNAPSHOT, NONE8, 0, [snap, time, 0]);
    } else {
        return R_CONT;
    }
    r_stateless(c.flags)
}

/// The shared tail of the two one-shot players: operands from `at` are pitchMin, pitchMax, volume, delay.
#[inline(never)]
fn one_shot(c: &mut Ctx<'_, '_>, clip: i32, at: u8) {
    let (lo, hi, vol, delay) = (c.fixed(at), c.fixed(at + 1), c.fixed(at + 2), c.fixed(at + 3));
    let pitch = lo + mul_shr_i32(hi - lo, (c.rand() >> 8) as i32, 24);
    let point = c.obj(1);
    c.world().request(k::AUDIO, point, 1, [clip, vol, pitch]);
    let _ = delay;
}
