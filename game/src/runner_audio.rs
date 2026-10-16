//! Resident Zombie Runner bank: per-actor walk loops and shared chase calls.
//! Uploaded once after the Geo bank; never resets the SPU. Voices 21 and 22
//! carry one Runner's movement loop each, voice 23 the creature calls.
use psx_spu::{self as spu, Adsr, Pitch, SpuAddr, Voice, Volume};
#[path = "volume.rs"]
mod volume;
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../data/runner-audio.rs"
));
mod mossman {
    include!(concat!(env!("CARGO_MANIFEST_DIR"),"/../data/mossman-runner-audio.rs"));
}
pub const MOSSMAN_BANK_BYTES:usize=mossman::BANK_BYTES;
pub const MOSSMAN_BANK_CHECKSUM:u32=mossman::BANK_CHECKSUM;
pub fn scene_is_mossman(scene_id:usize)->bool {mossman::SCENE_IDS.contains(&(scene_id as u16))}
fn clips(mossman:bool)->&'static [hk_sim::runner_audio::Clip] {if mossman {&mossman::CLIPS}else{&CLIPS}}
pub fn family_ready(mossman:bool)->bool {unsafe{HK_RUNNER_AUDIO_READY==if mossman{2}else{1}}}
pub fn validate_fragment(mossman:bool,offset:usize,data:&[u8])->bool {
    hk_sim::runner_audio::valid_manifest(if mossman{MOSSMAN_BANK_BYTES}else{BANK_BYTES},clips(mossman))
        && hk_sim::runner_audio::valid_fragment(clips(mossman),offset,data)
}
/// Revoke publication and silence all three voices before any SPU overwrite.
pub fn begin_overlay() -> bool {
    unsafe { HK_RUNNER_AUDIO_READY = 0; LOOP_OWNERS = [0; 2]; }
    for n in LOOP_VOICES.iter().copied().chain([CALL_VOICE]) {
        let voice = Voice::new(n);
        voice.set_volume(Volume::SILENCE, Volume::SILENCE);
        voice.set_adsr(Adsr { lower: 0x000f, upper: 0 });
        Voice::key_off(1 << n);
    }
    // SPU voice register +0x0c is the current envelope, not the ADSR setting.
    // Publication stays revoked on timeout; no sample upload occurs then.
    for poll in 0..2_000_000 {
        if poll & 4095 == 0 { crate::input::checkpoint(); }
        if LOOP_VOICES.iter().copied().chain([CALL_VOICE]).all(|n| unsafe {
            core::ptr::read_volatile((0x1f80_1c00usize + n as usize * 16 + 12) as *const u16) == 0
        }) { return true; }
        core::hint::spin_loop();
    }
    false
}
pub fn upload_fragment(offset:usize,data:&[u8]) {
    assert!(offset%16==0 && data.len()%16==0 && offset+data.len()<=20464);
    for (i,part) in data.chunks(1024).enumerate() {
        spu::upload_adpcm(SpuAddr::new(BANK_BASE+(offset+i*1024) as u32),part);
        crate::input::poll_only();
    }
}
pub fn finish_overlay(mossman:bool) {unsafe{HK_RUNNER_AUDIO_READY=if mossman{2}else{1};}}
#[no_mangle]
pub static mut HK_RUNNER_AUDIO_FAMILY_MISMATCH:u32=0;
fn require_family(mossman:bool)->bool {
    if family_ready(mossman){true}else{unsafe{HK_RUNNER_AUDIO_FAMILY_MISMATCH=HK_RUNNER_AUDIO_FAMILY_MISMATCH.saturating_add(1)};false}
}
const LOOP_VOICES: [u8; 2] = [21, 22];
const CALL_VOICE: u8 = 23;
const GAIN: i16 = 5461;
static mut LEVEL: u8 = 10;
/// Source id of the Runner owning each loop voice, 0 when free.
static mut LOOP_OWNERS: [u32; 2] = [0; 2];
#[no_mangle]
pub static mut HK_RUNNER_AUDIO_READY: u32 = 0;
#[no_mangle]
pub static mut HK_RUNNER_LOOP_STARTS: u32 = 0;
#[no_mangle]
pub static mut HK_RUNNER_CALLS: u32 = 0;
#[no_mangle]
pub static mut HK_RUNNER_LOOP_DROPPED: u32 = 0;

/// Called after the Geo bank upload; validates the complete immutable bank first.
/// `bank` is the disc chunk staged in the scene arena, checked for length and
/// checksum by the loader; the SPU keeps the samples and the staging bytes go.
pub fn upload(bank: &[u8]) {
    if unsafe { HK_RUNNER_AUDIO_READY != 0 } {
        return;
    }
    // Bootstrap and scene overlays use the same complete manifest and ADPCM
    // contract, including the reserved tail and authored loop/terminal flags.
    assert!(bank.len() == BANK_BYTES && validate_fragment(false, 0, bank));
    spu::upload_adpcm(SpuAddr::new(BANK_BASE), bank);
    unsafe {
        HK_RUNNER_AUDIO_READY = 1;
    }
}
pub fn set_volume(level: u8) {
    unsafe {
        LEVEL = level.min(10);
    }
    if unsafe { HK_RUNNER_AUDIO_READY == 0 } {
        return;
    }
    let gain = Volume(volume::scale(GAIN, unsafe { LEVEL }));
    for voice in LOOP_VOICES.iter().copied().chain([CALL_VOICE]) {
        Voice::new(voice).set_volume(gain, gain);
    }
}
pub fn ready() -> bool {
    unsafe { HK_RUNNER_AUDIO_READY != 0 }
}
fn loop_slot(source: u32) -> Option<usize> {
    unsafe { LOOP_OWNERS.iter().position(|&owner| owner == source) }
}
/// Start (or restart) a Runner's walk loop on its voice. A third simultaneous
/// Runner is counted as dropped rather than stealing a live voice.
pub fn loop_start(source: u32) {
    loop_start_for(source,false)
}
pub fn loop_start_for(source:u32,mossman:bool) {
    if !require_family(mossman) {
        return;
    }
    let slot = match loop_slot(source).or_else(|| loop_slot(0)) {
        Some(slot) => slot,
        None => {
            unsafe {
                HK_RUNNER_LOOP_DROPPED = HK_RUNNER_LOOP_DROPPED.saturating_add(1);
            }
            return;
        }
    };
    unsafe {
        LOOP_OWNERS[slot] = source;
    }
    let voice = Voice::new(LOOP_VOICES[slot]);
    // The source AudioSource plays the walk loop at its authored initial pitch.
    let (start, _, _, pitch, _, _) = clips(mossman)[0];
    Voice::key_off(1 << LOOP_VOICES[slot]);
    let gain = Volume(volume::scale(GAIN, unsafe { LEVEL }));
    voice.set_volume(gain, gain);
    voice.set_pitch(Pitch::raw(pitch));
    voice.set_start_addr(SpuAddr::new(start));
    voice.set_loop_addr(SpuAddr::new(start));
    voice.set_adsr(Adsr::sample_one_shot());
    Voice::key_on(1 << LOOP_VOICES[slot]);
    unsafe {
        HK_RUNNER_LOOP_STARTS = HK_RUNNER_LOOP_STARTS.saturating_add(1);
    }
}
/// Stop a Runner's walk loop and free its voice.
pub fn loop_stop(source: u32) {
    if !ready() {
        return;
    }
    if let Some(slot) = loop_slot(source) {
        Voice::key_off(1 << LOOP_VOICES[slot]);
        unsafe {
            LOOP_OWNERS[slot] = 0;
        }
    }
}
/// Source AudioPlayRandom: one of two calls at the controller's pitch (Q16
/// multiplier of the clip's nominal pitch, clamped to the authored bounds).
pub fn call(pitch_q16: i32, variant: u8) {
    call_for(pitch_q16,variant,false)
}
pub fn call_for(pitch_q16:i32,variant:u8,mossman:bool) {
    if !require_family(mossman) {
        return;
    }
    let bank=clips(mossman);
    if variant as usize>=bank.len()-1 {return;}
    let (start, bytes, pitch, low, high, _) = bank[1+variant as usize];
    // max/min rather than clamp: i64::clamp's panic message formats its bounds, which
    // links the 64-bit Debug printer and its software divide into the guest.
    let scaled =
        ((pitch as i64 * pitch_q16.max(0) as i64) >> 16).max(low as i64).min(high as i64) as u16;
    let voice = Voice::new(CALL_VOICE);
    Voice::key_off(1 << CALL_VOICE);
    voice.configure_sample(
        SpuAddr::new(start),
        11025,
        Volume(volume::scale(GAIN, unsafe { LEVEL })),
        Adsr::sample_one_shot(),
    );
    voice.set_pitch(Pitch::raw(scaled));
    let _ = bytes;
    Voice::key_on(1 << CALL_VOICE);
    unsafe {
        HK_RUNNER_CALLS = HK_RUNNER_CALLS.saturating_add(1);
    }
}
/// Scene leave or reset: silence every Runner voice and free the loop slots.
pub fn reset() {
    if !ready() {
        return;
    }
    for voice in LOOP_VOICES.iter().copied().chain([CALL_VOICE]) {
        Voice::key_off(1 << voice);
    }
    unsafe {
        LOOP_OWNERS = [0; 2];
    }
}
