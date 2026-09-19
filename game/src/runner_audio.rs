//! Resident Zombie Runner bank: per-actor walk loops and shared chase calls.
//! Uploaded once after the Geo bank; never resets the SPU. Voices 21 and 22
//! carry one Runner's movement loop each, voice 23 the creature calls.
use psx_spu::{self as spu, Adsr, Pitch, SpuAddr, Voice, Volume};
#[path = "volume.rs"] mod volume;
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/runner-audio.rs"));
const LOOP_VOICES: [u8; 2] = [21, 22];
const CALL_VOICE: u8 = 23;
const GAIN: i16 = 5461;
static mut LEVEL: u8 = 10;
/// Source id of the Runner owning each loop voice, 0 when free.
static mut LOOP_OWNERS: [u32; 2] = [0; 2];
#[no_mangle] pub static mut HK_RUNNER_AUDIO_READY: u32 = 0;
#[no_mangle] pub static mut HK_RUNNER_LOOP_STARTS: u32 = 0;
#[no_mangle] pub static mut HK_RUNNER_CALLS: u32 = 0;
#[no_mangle] pub static mut HK_RUNNER_LOOP_DROPPED: u32 = 0;

/// Called after the Geo bank upload; validates the complete immutable bank first.
/// `bank` is the disc chunk staged in the scene arena, checked for length and
/// checksum by the loader; the SPU keeps the samples and the staging bytes go.
pub fn upload(bank: &[u8]) {
    if unsafe { HK_RUNNER_AUDIO_READY != 0 } { return; }
    assert!(bank.len() == BANK_BYTES);
    assert!(BANK_BASE % 16 == 0 && BANK_BASE as usize + BANK_BYTES <= 0x80000);
    let mut expected = BANK_BASE;
    for &(start, bytes, pitch, low, high, loops) in &CLIPS {
        // The loop's authored initial pitch (its bounds) sits below the clip's nominal rate.
        assert!(start == expected && bytes % 16 == 0 && bytes > 0 && pitch > 0 && low <= high);
        let data = &bank[(start - BANK_BASE) as usize..(start - BANK_BASE) as usize + bytes];
        for (block, chunk) in data.chunks_exact(16).enumerate() {
            let last = (block + 1) * 16 == bytes;
            // ADPCM header byte: shift in the low nibble (0..12), filter in the high (0..4).
            assert!(chunk[0] & 0x0F <= 12 && chunk[0] >> 4 <= 4);
            // Loops repeat to their start; one-shots end in the terminal silence block.
            if loops { assert!(chunk[1] & 1 == u8::from(last)); } else { assert!(chunk[1] == u8::from(last)); }
        }
        expected = start + bytes as u32;
    }
    assert!(expected == BANK_BASE + BANK_BYTES as u32);
    spu::upload_adpcm(SpuAddr::new(BANK_BASE), bank);
    unsafe { HK_RUNNER_AUDIO_READY = 1; }
}
pub fn set_volume(level: u8) {
    unsafe { LEVEL = level.min(10); }
    if unsafe { HK_RUNNER_AUDIO_READY == 0 } { return; }
    let gain = Volume(volume::scale(GAIN, unsafe { LEVEL }));
    for voice in LOOP_VOICES.iter().copied().chain([CALL_VOICE]) { Voice::new(voice).set_volume(gain, gain); }
}
pub fn ready() -> bool { unsafe { HK_RUNNER_AUDIO_READY != 0 } }
fn loop_slot(source: u32) -> Option<usize> {
    unsafe { LOOP_OWNERS.iter().position(|&owner| owner == source) }
}
/// Start (or restart) a Runner's walk loop on its voice. A third simultaneous
/// Runner is counted as dropped rather than stealing a live voice.
pub fn loop_start(source: u32) {
    if !ready() { return; }
    let slot = match loop_slot(source).or_else(|| loop_slot(0)) {
        Some(slot) => slot,
        None => { unsafe { HK_RUNNER_LOOP_DROPPED = HK_RUNNER_LOOP_DROPPED.saturating_add(1); } return; }
    };
    unsafe { LOOP_OWNERS[slot] = source; }
    let voice = Voice::new(LOOP_VOICES[slot]);
    // The source AudioSource plays the walk loop at its authored initial pitch.
    let (start, _, _, pitch, _, _) = CLIPS[0];
    Voice::key_off(1 << LOOP_VOICES[slot]);
    let gain = Volume(volume::scale(GAIN, unsafe { LEVEL }));
    voice.set_volume(gain, gain);
    voice.set_pitch(Pitch::raw(pitch));
    voice.set_start_addr(SpuAddr::new(start));
    voice.set_loop_addr(SpuAddr::new(start));
    voice.set_adsr(Adsr::sample_one_shot());
    Voice::key_on(1 << LOOP_VOICES[slot]);
    unsafe { HK_RUNNER_LOOP_STARTS = HK_RUNNER_LOOP_STARTS.saturating_add(1); }
}
/// Stop a Runner's walk loop and free its voice.
pub fn loop_stop(source: u32) {
    if !ready() { return; }
    if let Some(slot) = loop_slot(source) {
        Voice::key_off(1 << LOOP_VOICES[slot]);
        unsafe { LOOP_OWNERS[slot] = 0; }
    }
}
/// Source AudioPlayRandom: one of two calls at the controller's pitch (Q16
/// multiplier of the clip's nominal pitch, clamped to the authored bounds).
pub fn call(pitch_q16: i32, variant: u8) {
    if !ready() { return; }
    let (start, bytes, pitch, low, high, _) = CLIPS[1 + usize::from(variant != 0)];
    let scaled = ((pitch as i64 * pitch_q16.max(0) as i64) >> 16).clamp(low as i64, high as i64) as u16;
    let voice = Voice::new(CALL_VOICE);
    Voice::key_off(1 << CALL_VOICE);
    voice.configure_sample(SpuAddr::new(start), 11025, Volume(volume::scale(GAIN, unsafe { LEVEL })), Adsr::sample_one_shot());
    voice.set_pitch(Pitch::raw(scaled));
    let _ = bytes;
    Voice::key_on(1 << CALL_VOICE);
    unsafe { HK_RUNNER_CALLS = HK_RUNNER_CALLS.saturating_add(1); }
}
/// Scene leave or reset: silence every Runner voice and free the loop slots.
pub fn reset() {
    if !ready() { return; }
    for voice in LOOP_VOICES.iter().copied().chain([CALL_VOICE]) { Voice::key_off(1 << voice); }
    unsafe { LOOP_OWNERS = [0; 2]; }
}
