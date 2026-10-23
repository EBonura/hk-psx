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
/// Bytes of a clip sent to the SPU between pad checkpoints. The SPU port stalls the CPU on
/// every write, so a slice takes a good fraction of a frame; 1 KiB keeps a late slice
/// from carrying a poll over a VBlank (4 KiB did, in the first load of the False Knight
/// journey).
const UPLOAD_SLICE: usize = 1024;
static mut CURRENT: usize = NONE;
/// The looping event now keyed on the scene voice (`loop_tick`), NONE once anything else
/// takes the voice: the next `loop_tick` then starts the loop again.
static mut LOOPING: usize = NONE;
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
        LOOPING = NONE;
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
    for &(event, address, offset, len, _, _) in entries(scene) {
        assert!(address % 16 == 0 && offset % 16 == 0);
        crate::ambience::forget(address, address + len);
        let clip = &bytes[offset as usize..(offset + len) as usize];
        for (slice, part) in clip.chunks(UPLOAD_SLICE).enumerate() {
            spu::upload_adpcm(SpuAddr::new(address + (slice * UPLOAD_SLICE) as u32), part);
            crate::input::checkpoint();
        }
        if event == MINER_IDLE_HUM || event == ZOMBIE_GUARD_RUN_LOOP {
            make_loop(address, clip);
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
/// Turn an uploaded one-shot into a looping sample. The cook ends every clip with a silent
/// END block after its data; the first data block gets the loop-start flag and the last one
/// loop-end with repeat, so the SPU plays the data round and round until key off.
fn make_loop(address: u32, clip: &[u8]) {
    let last = clip.len() - 32;
    for (at, flags) in [(0, 4u8), (last, 3u8)] {
        let mut block = [0u8; 16];
        block.copy_from_slice(&clip[at..at + 16]);
        block[1] = flags;
        spu::upload_adpcm(SpuAddr::new(address + at as u32), &block);
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
/// The scene-bank event that is the break clip of the ordinary Breakable `id` (scene * 128 + state),
/// for the families whose clip is not the resident wall hit (barrels, poles, wagons).
pub fn break_sound(id: usize) -> Option<u8> {
    let id = u16::try_from(id).ok()?;
    BREAK_SOUNDS
        .binary_search_by_key(&id, |&(owner, _)| owner)
        .ok()
        .map(|at| BREAK_SOUNDS[at].1)
}
/// Whether the resident bank holds `event`, without counting a miss.
pub fn resident(event: u8) -> bool {
    let current = unsafe { CURRENT };
    current != NONE && entries(current).iter().any(|e| e.0 == event)
}
/// `play` with the clip's pitch scaled by `scale` / 4096. One body for every caller: inlined into
/// each call site it cost more than the sounds it plays.
#[inline(never)]
pub fn play_pitched(event: u8, scale: u32) {
    let Some((address, rate, gain, pitch)) = lookup(event) else {
        return;
    };
    unsafe {
        LOOPING = NONE;
    }
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
    play_pitched(event, 4096);
}
/// A looped scene sound (`make_loop`) at `level`/4096 of its gain, on the scene voice. Call every
/// frame the source can be heard: it starts the loop when the voice is not already playing it
/// (the first call, or after a one-shot took the voice) and otherwise only moves the volume.
/// A zero level silences it without key off, so it keeps its place in the loop.
pub fn loop_tick(event: u8, level: i32) {
    let Some(&(_, address, _, _, rate, pitch)) = (unsafe {
        let current = CURRENT;
        if current == NONE {
            None
        } else {
            entries(current).iter().find(|e| e.0 == event)
        }
    }) else {
        return;
    };
    let (_, gain, _) = EVENT_PARAMS[event as usize];
    let volume = Volume(volume::scale(
        (i32::from(gain) * level.clamp(0, 4096) / 4096) as i16,
        crate::audio::level(),
    ));
    let voice = Voice::new(VOICE);
    if unsafe { LOOPING } != event as usize {
        Voice::key_off(1 << VOICE);
        voice.configure_sample(SpuAddr::new(address), rate, volume, Adsr::sample_one_shot());
        voice.set_pitch(Pitch::raw(pitch));
        Voice::key_on(1 << VOICE);
        unsafe {
            LOOPING = event as usize;
        }
    } else {
        voice.set_volume(volume, volume);
    }
}
/// Stop a looped scene sound that `loop_tick` started (the source went away).
pub fn loop_stop() {
    if unsafe { LOOPING } != NONE {
        Voice::key_off(1 << VOICE);
        unsafe {
            LOOPING = NONE;
        }
    }
}
/// Myla, the `Miner` of Crossroads_45. Her AudioSource hums (`Miner_idle_hum`, looped, volume
/// 0.854, rolloff 39 to 56.3 from the camera) from the start of the scene; her `Miner FX` FSM
/// plays one of two pickaxe clips when her sprite reaches frame id 20, the fifth frame of
/// `Idle` (`MYLA_STRIKE_FRAME`), through the same AudioSource and so the same rolloff.
pub const MYLA_STRIKE_FRAME: usize = 4;
const MYLA_VOLUME: i32 = 3498;
const MYLA_ROLLOFF: (i32, i32) = (39, 56);
fn myla_level(camera: (i32, i32), at: [i32; 2]) -> i32 {
    hk_sim::fixed::source_rolloff(at[0] - camera.0, at[1] - camera.1, MYLA_ROLLOFF.0, MYLA_ROLLOFF.1) * MYLA_VOLUME / 4096
}
/// Every frame the scene is resident: the hum, at the camera's distance from her.
pub fn myla_hum(camera: (i32, i32), at: Option<[i32; 2]>) {
    match at {
        Some(at) if resident(MINER_IDLE_HUM) => loop_tick(MINER_IDLE_HUM, myla_level(camera, at)),
        _ => loop_stop(),
    }
}
/// Each frame her `Idle` clip is at `frame` (its own index within the clip): one pickaxe clip
/// on the frame the strike starts, if she is within earshot.
pub fn myla_strike(camera: (i32, i32), at: [i32; 2], frame: usize) {
    static mut WAS: bool = false;
    static mut RANDOM: u32 = 0x6d79_6c61;
    let striking = frame == MYLA_STRIKE_FRAME;
    let rising = striking && unsafe { !WAS };
    unsafe {
        WAS = striking;
    }
    if !rising || !resident(MYLA_PICKAXE_1) {
        return;
    }
    let level = myla_level(camera, at);
    if level == 0 {
        return;
    }
    let event = unsafe {
        RANDOM = RANDOM.wrapping_mul(1664525).wrapping_add(1013904223);
        if RANDOM >> 31 == 0 {
            MYLA_PICKAXE_1
        } else {
            MYLA_PICKAXE_2
        }
    };
    if let Some((address, rate, gain, pitch)) = lookup(event) {
        let gain = (i32::from(gain) * level / 4096).min(i32::from(i16::MAX)) as i16;
        crate::audio::shared_clip(address, rate, gain, pitch);
    }
}
/// The conversation with her opens: her `Conversation Control` plays `Miner_01_sing`. Call every
/// frame with whether she is speaking; it plays on the rising edge, on the shared voice so the
/// hum on the scene voice carries on under it.
pub fn myla_sing(speaking: bool) {
    static mut WAS: bool = false;
    let rising = speaking && unsafe { !WAS };
    unsafe {
        WAS = speaking;
    }
    if rising && resident(MINER_01_SING) {
        play_shared(MINER_01_SING);
    }
}
