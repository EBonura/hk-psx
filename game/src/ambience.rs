//! Complete source ambience loops, loaded from CD once into SPU voices 6..11.
//! Call after audio::init. This module never resets the shared SPU or owns CD IO.
use psx_spu::{self as spu, Adsr, Pitch, SpuAddr, Voice, Volume};
#[path="volume.rs"]mod volume;
static mut LEVEL:u8=10;
#[path = "ambience_state.rs"]
mod state;
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/ambience.rs"));
pub const CLIPS: &[AmbienceClip] = &AMBIENCE_CLIPS;
static mut MIXER: state::Mixer = state::Mixer::new();
const VOICE_MASK: u32 = (state::ALL as u32) << 6;

#[no_mangle]
pub static mut HK_AMBIENCE_READY: u32 = 0;
#[no_mangle]
pub static mut HK_AMBIENCE_LOADED_MASK: u32 = 0;
#[no_mangle]
pub static mut HK_AMBIENCE_PLAYING_MASK: u32 = 0;
#[no_mangle]
pub static mut HK_AMBIENCE_START_COUNT: u32 = 0;
#[no_mangle]
pub static mut HK_AMBIENCE_TRANSITIONS: u32 = 0;
#[no_mangle]
pub static mut HK_AMBIENCE_SCENE: u32 = u32::MAX;
#[no_mangle]
pub static mut HK_AMBIENCE_FADE_TICKS: u32 = 0;
#[no_mangle]
pub static mut HK_AMBIENCE_GAINS: [u32; 6] = [0; 6];

fn publish() {
    unsafe {
        HK_AMBIENCE_READY = MIXER.ready as u32;
        HK_AMBIENCE_LOADED_MASK = MIXER.loaded as u32;
        HK_AMBIENCE_PLAYING_MASK = MIXER.playing as u32;
        HK_AMBIENCE_SCENE = if MIXER.scene == u8::MAX {
            u32::MAX
        } else {
            MIXER.scene as u32
        };
        HK_AMBIENCE_FADE_TICKS = MIXER.elapsed as u32;
        for i in 0..state::STEMS {
            HK_AMBIENCE_GAINS[i] = if MIXER.playing & (1 << i) != 0 {
                MIXER.gains[i] as u32
            } else {
                0
            };
        }
    }
}

/// Startup/retry only: silence before replacing any resident bytes.
pub fn begin_load() {
    for clip in CLIPS {
        Voice::new(clip.voice).set_volume(Volume::SILENCE, Volume::SILENCE);
    }
    Voice::key_off(VOICE_MASK);
    unsafe {
        MIXER = state::Mixer::new();
        HK_AMBIENCE_START_COUNT = 0;
        HK_AMBIENCE_TRANSITIONS = 0;
    }
    publish();
}

/// Validate a whole clip in the caller's reusable arena, then complete SDK DMA.
/// Failed validation never modifies SPU RAM or marks a clip resident.
pub fn upload(index: usize, bytes: &[u8]) -> bool {
    if index >= CLIPS.len() || is_ready() {
        return false;
    }
    let clip = CLIPS[index];
    let start = if index == 0 {
        0x8D00
    } else {
        let Some(end) = CLIPS[index - 1]
            .spu_address
            .checked_add(CLIPS[index - 1].byte_len as u32)
        else {
            return false;
        };
        end
    };
    let Some(end) = start.checked_add(clip.byte_len as u32) else {
        return false;
    };
    if clip.voice != 6 + index as u8
        || clip.spu_address != start
        || start % 16 != 0
        || start < 0x8D00
        || end > 0x80000
        || clip.pitch == 0
        || clip.pitch > 0x3FFF
        || !state::valid_clip(bytes, clip.byte_len, clip.checksum)
    {
        return false;
    }
    spu::upload_adpcm(SpuAddr::new(start), bytes);
    unsafe {
        MIXER.loaded |= 1 << index;
    }
    publish();
    true
}

pub fn is_ready() -> bool {
    unsafe { MIXER.ready }
}

/// Configure only after the complete checksummed bank is resident; no key-on yet.
pub fn finish_load() -> bool {
    if is_ready() {
        return true;
    }
    if unsafe { MIXER.loaded } != state::ALL {
        return false;
    }
    for clip in CLIPS {
        let voice = Voice::new(clip.voice);
        voice.set_volume(Volume::SILENCE, Volume::SILENCE);
        voice.set_pitch(Pitch::raw(clip.pitch));
        voice.set_start_addr(SpuAddr::new(clip.spu_address));
        voice.set_loop_addr(SpuAddr::new(clip.spu_address));
        // This preset holds full sustain like sample(), but releases promptly
        // on key-off. END+REPEAT loops do not enter release. The authored mixer
        // controls fades; explicit volume zero guarantees disabled stems mute.
        voice.set_adsr(Adsr::sample_one_shot());
    }
    unsafe {
        MIXER.finish();
    }
    publish();
    true
}

/// 0 = Tutorial Cave, 1 = covered Town Surface. Repeated scene IDs are no-ops.
pub fn set_scene(scene: u8) -> bool {
    if !is_ready() || scene as usize >= AMBIENCE_SCENES.len() {
        return false;
    }
    if unsafe { MIXER.scene } == scene {
        return true;
    }
    let cue = AMBIENCE_SCENES[scene as usize];
    let start = unsafe { MIXER.cue(scene, cue.mask, cue.gains, cue.fade_ticks) };
    for i in 0..state::STEMS {
        if start & (1 << i) != 0 {
            let gain = Volume(volume::scale(unsafe { MIXER.gains[i] },unsafe {LEVEL}));
            Voice::new(CLIPS[i].voice).set_volume(gain, gain);
        }
    }
    if start != 0 {
        Voice::key_on((start as u32) << 6);
        for i in 0..state::STEMS {
            if start & (1 << i) != 0 {
                Voice::new(CLIPS[i].voice).set_loop_addr(SpuAddr::new(CLIPS[i].spu_address));
            }
        }
    }
    unsafe {
        HK_AMBIENCE_START_COUNT = HK_AMBIENCE_START_COUNT.saturating_add(start.count_ones());
        HK_AMBIENCE_TRANSITIONS = HK_AMBIENCE_TRANSITIONS.saturating_add(1);
    }
    if cue.fade_ticks == 0 {
        tick();
    } else {
        publish();
    }
    true
}

/// Once per 60 Hz tick, independently of gameplay pause and grid activation.
pub fn tick() {
    // Settled loops run wholly in the SPU: no per-tick divisions or MMIO writes.
    if unsafe { !MIXER.ready || !MIXER.transitioning } {
        return;
    }
    let stop = unsafe { MIXER.tick() };
    for i in 0..state::STEMS {
        let gain = unsafe {
            if MIXER.playing & (1 << i) != 0 {
                MIXER.gains[i]
            } else {
                0
            }
        };
        let gain=Volume(volume::scale(gain,unsafe {LEVEL}));
        Voice::new(CLIPS[i].voice).set_volume(gain,gain);
    }
    if stop != 0 {
        Voice::key_off((stop as u32) << 6);
    }
    publish();
}

/// Preserve each source stem's fade/mix while changing the user's level.
pub fn set_volume(level:u8) {
    unsafe {LEVEL=level.min(10);}
    for i in 0..state::STEMS {
        let source=unsafe {if MIXER.playing&(1<<i)!=0 {MIXER.gains[i]}else{0}};
        let gain=Volume(volume::scale(source,level));
        Voice::new(CLIPS[i].voice).set_volume(gain,gain);
    }
}
