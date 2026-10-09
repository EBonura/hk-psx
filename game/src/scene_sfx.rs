//! Per-scene one-shot banks (host/scene_sfx.py), read with the scene at its gate.
//!
//! The resident banks hold what every scene can play. A sound only some scenes
//! need lives in its scene's bank: one pack chunk first in that scene's disc
//! group, so the gate load reads it on the way to the scene with no extra seek,
//! and `disc::admit_scenes` hands it here after the scene's ambience stems are
//! in. Each clip goes to the SPU address the cook placed it at, in bytes no stem
//! that can sound in this scene uses; a resident stem the bank overwrites is
//! forgotten (`ambience::forget`) so it reloads when a cue wants it again.
//!
//! Playback: `play(EVENT)` with the event constants below, on the one voice
//! ambience leaves for this (`SCENE_SFX_VOICE`). An event the current scene's
//! bank does not hold (not cooked for this scene, or refused for room) is a
//! no-op, counted in `HK_SCENE_SFX_MISSED`, so a call site never has to know
//! which scenes carry which sounds.
use psx_spu::{self as spu, Adsr, Pitch, SpuAddr, Voice, Volume};
#[path = "volume.rs"]
mod volume;
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/scene_sfx.rs"));
const VOICE: u8 = crate::ambience::SCENE_SFX_VOICE;
const NONE: usize = usize::MAX;
static mut CURRENT: usize = NONE;
/// Plays per event, and plays asked for that the resident bank does not hold.
#[no_mangle]
pub static mut HK_SCENE_SFX: [u32; EVENTS] = [0; EVENTS];
#[no_mangle]
pub static mut HK_SCENE_SFX_MISSED: u32 = 0;
/// Banks uploaded, bytes uploaded, and the scene id (plus one) now resident.
#[no_mangle]
pub static mut HK_SCENE_SFX_LOADS: u32 = 0;
#[no_mangle]
pub static mut HK_SCENE_SFX_BYTES: u32 = 0;
#[no_mangle]
pub static mut HK_SCENE_SFX_SCENE: u32 = 0;

fn entries(scene: usize) -> &'static [(u8, u32, u32, u32, u32, u16)] {
    let (_, _, first, count) = BANKS[scene];
    &ENTRIES[first as usize..first as usize + count as usize]
}
/// The chunk `scene`'s group carries: its length and FNV-1a.
pub const fn chunk(scene: usize) -> (usize, u32) {
    let (len, fnv, _, _) = BANKS[scene];
    (len as usize, fnv)
}
/// The resident bank is gone (a new admission began, or something else wrote
/// its bytes). Nothing plays from it until the next `upload`.
pub fn invalidate() {
    unsafe {
        CURRENT = NONE;
        HK_SCENE_SFX_SCENE = 0;
    }
}
/// Another bank is about to write `lo..hi`: drop the resident one if it
/// overlaps, so a play never keys on bytes that are no longer its clip.
pub fn overwritten(lo: u32, hi: u32) {
    let current = unsafe { CURRENT };
    if current != NONE
        && entries(current)
            .iter()
            .any(|&(_, a, _, n, _, _)| a < hi && lo < a + n)
    {
        invalidate();
    }
}
/// `bytes` is the checked chunk for guest scene `scene`. Uploads every entry to
/// its cooked address, a slice at a time with the pad serviced between.
pub fn upload(scene: usize, bytes: &[u8]) {
    invalidate();
    Voice::key_off(1 << VOICE);
    assert!(bytes.len() == BANKS[scene].0 as usize);
    let mut total = 0u32;
    for &(_, address, offset, len, _, _) in entries(scene) {
        assert!(address % 16 == 0 && offset % 16 == 0);
        crate::ambience::forget(address, address + len);
        let clip = &bytes[offset as usize..(offset + len) as usize];
        for (slice, part) in clip.chunks(4096).enumerate() {
            spu::upload_adpcm(SpuAddr::new(address + (slice * 4096) as u32), part);
            crate::input::checkpoint();
        }
        total += len;
    }
    unsafe {
        CURRENT = scene;
        HK_SCENE_SFX_SCENE = scene as u32 + 1;
        HK_SCENE_SFX_LOADS = HK_SCENE_SFX_LOADS.saturating_add(1);
        HK_SCENE_SFX_BYTES = HK_SCENE_SFX_BYTES.saturating_add(total);
    }
}
/// Where the resident bank holds `event`, with the rate it was cooked at, its
/// gain and its pitch register, for a caller that plays it on a voice of its own.
fn lookup(event: u8) -> Option<(u32, u32, i16, u16)> {
    let current = unsafe { CURRENT };
    let found = if current == NONE {
        None
    } else {
        entries(current).iter().find(|e| e.0 == event)
    };
    let Some(&(_, address, _, _, rate, pitch)) = found else {
        unsafe {
            HK_SCENE_SFX_MISSED = HK_SCENE_SFX_MISSED.saturating_add(1);
        }
        return None;
    };
    // A fitted scene cooks a clip at its own rate (host/scene_sfx.py `fit`),
    // so the rate and pitch come from the entry; the gain is the event's.
    let (_, gain, _) = EVENT_PARAMS[event as usize];
    unsafe {
        HK_SCENE_SFX[event as usize] = HK_SCENE_SFX[event as usize].saturating_add(1);
    }
    Some((address, rate, gain, pitch))
}
/// A scene sound on the resident banks' shared voice rather than the scene
/// voice: for a long clip (the False Knight's roars) that the scene voice's
/// next one-shot would otherwise cut, where nothing else on the shared voice
/// sounds at the same time.
pub fn play_shared(event: u8) {
    if let Some((address, rate, gain, pitch)) = lookup(event) {
        crate::audio::shared_clip(address, rate, gain, pitch);
    }
}
/// Whether the resident bank holds `event`, without counting a miss.
pub fn resident(event: u8) -> bool {
    let current = unsafe { CURRENT };
    current != NONE && entries(current).iter().any(|e| e.0 == event)
}
/// `play` with the clip's pitch scaled by `scale` / 4096.
pub fn play_pitched(event: u8, scale: u32) {
    let Some((address, rate, gain, pitch)) = lookup(event) else {
        return;
    };
    Voice::key_off(1 << VOICE);
    let voice = Voice::new(VOICE);
    voice.configure_sample(
        SpuAddr::new(address),
        rate,
        Volume(volume::scale(gain, crate::audio::level())),
        Adsr::sample_one_shot(),
    );
    voice.set_pitch(Pitch::raw((pitch as u32 * scale / 4096).min(0x3fff) as u16));
    Voice::key_on(1 << VOICE);
}
/// One scene sound on the scene voice, if the resident bank holds it.
pub fn play(event: u8) {
    let Some((address, rate, gain, pitch)) = lookup(event) else {
        return;
    };
    Voice::key_off(1 << VOICE);
    let voice = Voice::new(VOICE);
    voice.configure_sample(
        SpuAddr::new(address),
        rate,
        Volume(volume::scale(gain, crate::audio::level())),
        Adsr::sample_one_shot(),
    );
    voice.set_pitch(Pitch::raw(pitch));
    Voice::key_on(1 << VOICE);
}
