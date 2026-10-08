//! Source Geo one-shots, resident above ambience. No event-time CD access.
use psx_spu::{self as spu, Adsr, SpuAddr, Voice, Volume};
#[path = "volume.rs"] mod volume;
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/geo-audio.rs"));
mod existing_sfx {
    include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/sfx.rs"));
    pub fn rock_break() -> (u32, u32, i16) { SAMPLES[0] }
}
const BASE: u32 = 0x14000;
static mut LEVEL: u8 = 10;
static mut RANDOM: u32 = 0x47454f;
#[no_mangle] pub static mut HK_GEO_AUDIO_READY: u32 = 0;
#[no_mangle] pub static mut HK_GEO_PICKUP_SFX: u32 = 0;
#[no_mangle] pub static mut HK_GEO_ROCK_HIT_SFX: u32 = 0;
#[no_mangle] pub static mut HK_GEO_ROCK_BREAK_SFX: u32 = 0;

pub fn ready() -> bool { unsafe { HK_GEO_AUDIO_READY != 0 } }

/// Called after the existing SFX/ambience initialization; never resets the SPU.
/// `bank` is the disc chunk staged in the scene arena, checked for length and
/// checksum by the loader; the SPU keeps the samples and the staging bytes go.
pub fn upload(bank: &[u8]) {
    if unsafe { HK_GEO_AUDIO_READY != 0 } { return; }
    assert!(bank.len() == BANK_BYTES);
    assert!(BANK_BYTES <= (0x18000 - BASE) as usize);
    assert_eq!(SAMPLES[0].0, BASE);
    // Validate the complete immutable bank before uploading or admitting playback.
    for i in 0..SAMPLES.len() {
        let (start, rate, gain) = SAMPLES[i];
        let end = if i + 1 < SAMPLES.len() { SAMPLES[i + 1].0 } else { BASE + BANK_BYTES as u32 };
        assert!(start >= BASE && end > start && end <= 0x18000);
        assert!(start % 16 == 0 && (end - start) % 16 == 0);
        assert!(rate == 11025 && gain == 5461);
        let bytes = &bank[(start - BASE) as usize..(end - BASE) as usize];
        for (block, data) in bytes.chunks_exact(16).enumerate() {
            let terminal = (block + 1) * 16 == bytes.len();
            // Any SPU filter (0..4) and shift (0..12): the SDK encoder uses
            // all five filters, where the old cooker only ever wrote filter 0.
            assert!(data[0] & 0x0F <= 12 && data[0] >> 4 <= 4 && data[1] == u8::from(terminal));
            if terminal { assert!(data[2..].iter().all(|&b| b == 0)); }
        }
    }
    spu::upload_adpcm(SpuAddr::new(BASE), bank);
    unsafe { HK_GEO_AUDIO_READY = 1; }
}

/// Applies to already playing voices without restarting any sample.
pub fn set_volume(level: u8) {
    unsafe { LEVEL = level.min(10); }
    if unsafe { HK_GEO_AUDIO_READY == 0 } { return; }
    let gain = Volume(volume::scale(5461, level));
    for voice in 12..=14 { Voice::new(voice).set_volume(gain, gain); }
}

fn choice(count: u32) -> usize {
    // Separate deterministic PRNG: audio never perturbs gameplay's random state.
    unsafe {
        RANDOM = RANDOM.wrapping_mul(1664525).wrapping_add(1013904223);
        ((u64::from(RANDOM) * u64::from(count)) >> 32) as usize
    }
}
fn play(voice: u8, sample: (u32, u32, i16)) {
    assert!(unsafe { HK_GEO_AUDIO_READY != 0 });
    Voice::key_off(1 << voice);
    // The pinned SDK's fast-release one-shot points END to reserved silence.
    // sample() would have indefinite release; this retains the full source clip.
    Voice::new(voice).configure_sample(SpuAddr::new(sample.0), sample.1,
        Volume(volume::scale(sample.2, unsafe { LEVEL })), Adsr::sample_one_shot());
    Voice::key_on(1 << voice);
}
/// Value of this picked-up coin (1, 5 or 25), not a summed batch value.
pub fn pickup(value: u32) {
    assert!(matches!(value, 1 | 5 | 25));
    let variant = choice(2);
    let index = if variant == 0 { 0 } else if value == 5 { 2 } else { 1 };
    play(12, SAMPLES[index]);
    unsafe { HK_GEO_PICKUP_SFX = HK_GEO_PICKUP_SFX.saturating_add(1); }
}
pub fn hit() {
    play(13, SAMPLES[3 + choice(3)]);
    unsafe { HK_GEO_ROCK_HIT_SFX = HK_GEO_ROCK_HIT_SFX.saturating_add(1); }
}
pub fn break_rock() {
    // Source Destroy has two variants. This exact first clip already resides in
    // the SFX bank; the second complete clip cannot fit the remaining SPU budget.
    play(14, existing_sfx::rock_break());
    unsafe { HK_GEO_ROCK_BREAK_SFX = HK_GEO_ROCK_BREAK_SFX.saturating_add(1); }
}
