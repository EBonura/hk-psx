//! Complete source ambience loops, loaded from CD once into the voices
//! ambience owns. The longest loop lives in main RAM, feeding a bounded SPU
//! ring on a voice of its own; every other stem borrows one of the pooled
//! voices for as long as it is audible, so the resident set may be longer than
//! the pool. Call after audio::init. This module never resets the shared SPU
//! or owns CD IO.
use psx_spu::{self as spu, Adsr, Pitch, SpuAddr, Voice, Volume};
#[path="volume.rs"]mod volume;
static mut LEVEL:u8=10;
#[path = "ambience_state.rs"]
mod state;
#[path = "audio_stream.rs"]
mod stream;
// The cooked table, which is where the stem count, the streamed clip's index
// and the whole voice budget come from.
pub use state::{data::*, STREAM_INDEX};
pub const CLIPS: &[AmbienceClip] = &AMBIENCE_CLIPS;
#[repr(align(4))]
struct Source([u8; AMBIENCE_STREAM_SOURCE_BYTES]);
#[no_mangle]
pub static mut HK_AMBIENCE_STREAM_SOURCE: Source = Source([0; AMBIENCE_STREAM_SOURCE_BYTES]);
static mut MIXER: state::Mixer = state::Mixer::new();

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
pub static mut HK_AMBIENCE_GAINS: [u32; state::STEMS] = [0; state::STEMS];
/// Which SPU voices the bank is driving, and how many stems a cue could not
/// key on because the pool was dry. A replay reads both: the first is the
/// evidence that a stem is on the voice the mixer says it is, the second is
/// zero for every cue pair the catalogue contains.
#[no_mangle]
pub static mut HK_AMBIENCE_VOICE_MASK: u32 = 0;
#[no_mangle]
pub static mut HK_AMBIENCE_VOICE_DENIALS: u32 = 0;

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
        HK_AMBIENCE_VOICE_MASK = MIXER.voice_mask();
        HK_AMBIENCE_VOICE_DENIALS = MIXER.denied as u32;
        for i in 0..state::STEMS {
            HK_AMBIENCE_GAINS[i] = if MIXER.playing & (1 << i) != 0 {
                MIXER.gains[i] as u32
            } else {
                0
            };
        }
    }
}

/// Every voice ambience owns, silenced. Which stem is on which of them is not
/// fixed, so the bank is quietened by voice rather than by clip.
fn silence() {
    Voice::new(AMBIENCE_STREAM_VOICE).set_volume(Volume::SILENCE, Volume::SILENCE);
    for &voice in AMBIENCE_POOL_VOICES.iter() {
        Voice::new(voice).set_volume(Volume::SILENCE, Volume::SILENCE);
    }
}

/// Startup/retry only: silence before replacing any resident bytes.
pub fn begin_load() {
    stream::stop();
    silence();
    Voice::key_off(state::VOICE_MASK);
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
        AMBIENCE_SPU_START
    } else {
        let Some(end) = CLIPS[index - 1]
            .spu_address
            .checked_add(CLIPS[index - 1].spu_bytes as u32)
        else {
            return false;
        };
        end
    };
    let Some(end) = start.checked_add(clip.spu_bytes as u32) else {
        return false;
    };
    if clip.spu_address != start
        || start % 16 != 0
        || start < AMBIENCE_SPU_START
        || end > AMBIENCE_SPU_END
        || clip.spu_bytes != if index == STREAM_INDEX { AMBIENCE_STREAM_BYTES } else { clip.byte_len }
        || (index == STREAM_INDEX && bytes.len() != AMBIENCE_STREAM_SOURCE_BYTES)
        || clip.pitch == 0
        || clip.pitch > 0x3FFF
        || !state::valid_clip(bytes, clip.byte_len, clip.checksum)
    {
        return false;
    }
    if index == STREAM_INDEX {
        unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), (&raw mut HK_AMBIENCE_STREAM_SOURCE.0).cast::<u8>(), bytes.len()); }
    } else {
        spu::upload_adpcm(SpuAddr::new(start), bytes);
    }
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
    silence();
    // The streamed stem's voice is its own for the life of the bank, so its
    // envelope is set once here; audio_stream::start writes the rest of its
    // registers. A pooled voice is configured at key-on instead, because the
    // stem that had it last configured it for a different loop.
    Voice::new(AMBIENCE_STREAM_VOICE).set_adsr(Adsr::sample_one_shot());
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
    let mut keyed = 0u32;
    for i in 0..state::STEMS {
        if start & (1 << i) == 0 {
            continue;
        }
        // Silence first: a pooled voice was driving another loop the last time
        // it was used, and nothing it is still releasing should be heard at
        // this stem's volume.
        let voice = Voice::new(unsafe { MIXER.voice(i) });
        voice.set_volume(Volume::SILENCE, Volume::SILENCE);
        if i != STREAM_INDEX {
            // Everything that selects this loop rather than the last one, set
            // here because the voice is not this stem's between cues. This
            // preset holds full sustain like sample(), but releases promptly
            // on key-off. END+REPEAT loops do not enter release. The authored
            // mixer controls fades; explicit volume zero guarantees disabled
            // stems mute.
            voice.set_pitch(Pitch::raw(CLIPS[i].pitch));
            voice.set_start_addr(SpuAddr::new(CLIPS[i].spu_address));
            voice.set_adsr(Adsr::sample_one_shot());
            keyed |= voice.mask();
        }
    }
    if start != 0 {
        if start & (1 << STREAM_INDEX) != 0 {
            let source = unsafe { &*(&raw const HK_AMBIENCE_STREAM_SOURCE.0) };
            if !stream::start(source, CLIPS[STREAM_INDEX].spu_address, psx_rt::interrupts::vblank_count()) {
                return false;
            }
        }
        Voice::key_on(keyed);
        // The loop point and the gain only after key-on: key-on latches the
        // repeat address from the ADPCM loop flag, and a stem that is audible
        // for the register write between the two is audible at silence.
        for i in 0..state::STEMS {
            if start & (1 << i) == 0 {
                continue;
            }
            let voice = Voice::new(unsafe { MIXER.voice(i) });
            if i != STREAM_INDEX {
                voice.set_loop_addr(SpuAddr::new(CLIPS[i].spu_address));
            }
            let gain = Volume(volume::scale(unsafe { MIXER.gains[i] },unsafe {LEVEL}));
            voice.set_volume(gain, gain);
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

/// Observe actual SPU progress, independently of simulation catch-up ticks.
pub fn service() {
    if unsafe { !MIXER.ready || MIXER.playing & (1 << STREAM_INDEX) == 0 } { return; }
    let source = unsafe { &*(&raw const HK_AMBIENCE_STREAM_SOURCE.0) };
    if matches!(stream::service(source, psx_rt::interrupts::vblank_count()), stream::Service::Fault) {
        let voice = Voice::new(AMBIENCE_STREAM_VOICE);
        voice.set_volume(Volume::SILENCE, Volume::SILENCE);
        Voice::key_off(voice.mask());
    }
}

/// Once per 60 Hz tick, independently of gameplay pause and grid activation.
pub fn tick() {
    // Settled loops run wholly in the SPU: no per-tick divisions or MMIO writes.
    if unsafe { !MIXER.ready || !MIXER.transitioning } {
        return;
    }
    let stop = unsafe { MIXER.tick() };
    for i in 0..state::STEMS {
        // A stem holding no voice has nothing to write to, and the voice it
        // would have written to before now belongs to another stem.
        let voice = unsafe { MIXER.voice(i) };
        if voice == state::NO_VOICE {
            continue;
        }
        let gain = unsafe {
            if MIXER.playing & (1 << i) != 0 {
                MIXER.gains[i]
            } else {
                0
            }
        };
        let gain=Volume(volume::scale(gain,unsafe {LEVEL}));
        Voice::new(voice).set_volume(gain,gain);
    }
    if stop != 0 {
        if stop & (1 << STREAM_INDEX) != 0 { stream::stop(); }
        let mut keyed = 0u32;
        for i in 0..state::STEMS {
            if stop & (1 << i) != 0 && i != STREAM_INDEX {
                keyed |= Voice::new(unsafe { MIXER.voice(i) }).mask();
            }
        }
        Voice::key_off(keyed);
        // Only now that the SPU has been told, so no cue this frame can hand a
        // pooled voice to another stem while the old one is still keying off.
        unsafe { MIXER.release(stop); }
    }
    publish();
}

/// Preserve each source stem's fade/mix while changing the user's level.
pub fn set_volume(level:u8) {
    unsafe {LEVEL=level.min(10);}
    for i in 0..state::STEMS {
        let voice=unsafe {MIXER.voice(i)};
        if voice==state::NO_VOICE {continue;}
        let source=unsafe {if MIXER.playing&(1<<i)!=0 {MIXER.gains[i]}else{0}};
        let gain=Volume(volume::scale(source,level));
        Voice::new(voice).set_volume(gain,gain);
    }
}
